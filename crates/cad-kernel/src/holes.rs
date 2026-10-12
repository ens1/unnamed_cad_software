//! Exact drill stock with shared circular station boundaries and planar shoulders.
use crate::adapter::{MonstertruckAdapter, Shape};
use cad_core::{
    CadError, Dimension, Document, NodeRef, Result, expression,
    holes::{Definition, End, Head},
};
use monstertruck::modeling::*;
use std::collections::{BTreeMap, BTreeSet};
fn invalid(message: impl std::fmt::Display) -> CadError {
    CadError::Invalid(format!("Drilled holes: {message}"))
}
fn positive(value: f64, label: &str) -> Result<f64> {
    if !value.is_finite() || value <= 1e-7 {
        Err(invalid(format!("{label} must be greater than 1e-7 mm")))
    } else {
        Ok(value)
    }
}
pub(crate) fn tool(origin: Point3, axis: Vector3, stations: &[(f64, f64)]) -> Result<Solid> {
    if ![origin.x, origin.y, origin.z, axis.x, axis.y, axis.z]
        .iter()
        .all(|value| value.is_finite())
        || stations.len() < 2
        || stations
            .iter()
            .any(|(depth, radius)| !depth.is_finite() || !radius.is_finite() || *radius <= 1e-7)
    {
        return Err(invalid(
            "Drill construction exceeds the finite coordinate or positive radius range",
        ));
    }
    let radial = axis
        .cross(if axis.x.abs() < 0.9 {
            Vector3::unit_x()
        } else {
            Vector3::unit_y()
        })
        .normalize();
    let wires = stations
        .iter()
        .map(|(depth, radius)| {
            let center = origin + axis * *depth;
            let wire: Wire = primitive::circle(center + radial * *radius, center, axis, 4);
            wire
        })
        .collect::<Vec<_>>();
    let first: Face = builder::try_attach_plane(vec![wires[0].clone()]).map_err(invalid)?;
    let mut shell: Shell = vec![first.inverse()].into();
    for (index, pair) in stations.windows(2).enumerate() {
        if (pair[1].0 - pair[0].0).abs() <= 1e-12 {
            if pair[0].1 <= pair[1].1 {
                return Err(invalid("A drill shoulder must step inward"));
            }
            let shoulder: Face =
                builder::try_attach_plane(vec![wires[index].clone(), wires[index + 1].inverse()])
                    .map_err(invalid)?;
            shell.push(shoulder);
        } else {
            if pair[1].0 <= pair[0].0 {
                return Err(invalid("Drill stations must progress into the body"));
            }
            shell.extend(
                builder::try_wire_homotopy::<Curve, Surface>(&wires[index], &wires[index + 1])
                    .map_err(invalid)?,
            );
        }
    }
    let last: Face =
        builder::try_attach_plane(vec![wires.last().unwrap().clone()]).map_err(invalid)?;
    shell.push(last);
    let solid = Solid::try_new(vec![shell]).map_err(invalid)?;
    crate::validation::solid(&solid)?;
    Ok(solid)
}
pub(crate) fn operation(
    adapter: &MonstertruckAdapter,
    doc: &Document,
    owner: &NodeRef,
    definition: &Definition,
    inputs: &BTreeMap<NodeRef, Shape>,
    tolerance: f64,
    generated: &mut crate::adapter::TopologyHistory,
) -> Result<Shape> {
    let body = match inputs.get(&definition.body) {
        Some(Shape::Bodies(b)) if !b.is_empty() => b,
        _ => return Err(invalid("Select a solid target body")),
    };
    let feature = doc
        .parts
        .get(&definition.positions.definition)
        .and_then(|p| {
            p.features.iter().find(|f| {
                f.id == definition.positions.node
                    && !cad_core::configuration::feature_suppressed(
                        doc,
                        &definition.positions.definition,
                        f,
                    )
            })
        })
        .ok_or_else(|| invalid("Hole-position sketch is missing or suppressed"))?;
    let cad_core::Operation::Profile {
        profile: cad_core::Profile::Sketch { sketch },
        plane,
        plane_reference,
        reference_body,
    } = &feature.operation
    else {
        return Err(invalid("Hole positions must be points in a native sketch"));
    };
    let source = doc.parts[&definition.positions.definition]
        .sketches
        .get(sketch)
        .ok_or_else(|| invalid("Hole-position sketch source is missing"))?;
    if definition.points.is_empty() || definition.points.len() > 1000 {
        return Err(invalid("Select 1..1000 hole-position sketch points"));
    }
    let mut seen = BTreeSet::new();
    let suppressed = cad_core::sketch_transform::suppressed_points(source);
    for point in &definition.points {
        if !source.points.contains_key(point) || suppressed.contains(point) || !seen.insert(point) {
            return Err(invalid(
                "Hole-position points must exist, be unsuppressed and be distinct",
            ));
        }
    }
    let plane = crate::adapter::KernelAdapter::sketch_plane(
        adapter,
        plane,
        plane_reference.as_ref(),
        reference_body.as_ref(),
        inputs,
    )?;
    let axis = Vector3::from(plane.u)
        .cross(Vector3::from(plane.v))
        .normalize()
        * if definition.reverse { 1. } else { -1. };
    if !axis.magnitude2().is_finite() {
        return Err(invalid("Hole-position sketch plane is degenerate"));
    }
    let length = |e| expression::value(doc, &owner.definition, e, Dimension::Length);
    let thread = definition
        .thread
        .as_ref()
        .map(|thread| thread.evaluate(doc, &owner.definition))
        .transpose()?;
    let radius = positive(length(definition.bore_diameter())? * 0.5, "Hole radius")?;
    let start = length(&definition.start_offset)?;
    let clearance = (tolerance * 2.).max(1e-6);
    let bounds = if definition.end == End::ThroughAll {
        Some(
            body.iter()
                .map(crate::native_bounds::solid)
                .collect::<Result<Vec<_>>>()?
                .into_iter()
                .flatten()
                .map(|bound| {
                    bound
                        .projection(Point3::origin(), axis)
                        .map(|(_, high)| high)
                })
                .collect::<Result<Vec<_>>>()?
                .into_iter()
                .fold(f64::NEG_INFINITY, f64::max),
        )
    } else {
        None
    };
    let mut drills: Vec<crate::drilled_pockets::Drill> = Vec::new();
    for point in &definition.points {
        let p = adapter.resolve_surface_point(
            doc,
            &owner.definition,
            &cad_core::reference_geometry::PointSource::SketchPoint {
                profile: definition.positions.clone(),
                point: point.clone(),
            },
            inputs,
        )?;
        let origin = Point3::from(p) + axis * start;
        if drills
            .iter()
            .any(|drill| (drill.origin - origin).magnitude2() < 1e-14)
        {
            return Err(invalid(
                "Hole center points resolve to the same position; choose distinct centers",
            ));
        }
        let depth = positive(
            if let Some(end) = bounds {
                end - origin.to_vec().dot(axis) + clearance
            } else {
                length(&definition.depth)?
            },
            "Hole depth",
        )?;
        let mut entry_head_depth = 0.;
        let mut stations = match &definition.head {
            Head::Simple => vec![(-clearance, radius)],
            Head::Counterbore {
                diameter,
                depth: head_depth,
            } => {
                let head_radius = positive(length(diameter)? * 0.5, "Counterbore radius")?;
                let head_depth = positive(length(head_depth)?, "Counterbore depth")?;
                if head_radius <= radius + 1e-7 || head_depth >= depth - 1e-7 {
                    return Err(invalid(
                        "Counterbore diameter must exceed the hole diameter and its depth must be less than the hole depth",
                    ));
                }
                if thread
                    .as_ref()
                    .is_some_and(|thread| head_radius * 2. < thread.nominal_diameter_mm - 1e-7)
                {
                    return Err(invalid(
                        "Counterbore diameter must accommodate the cosmetic thread nominal diameter",
                    ));
                }
                entry_head_depth = head_depth;
                vec![
                    (-clearance, head_radius),
                    (head_depth, head_radius),
                    (head_depth, radius),
                ]
            }
            Head::Countersink { diameter, angle } => {
                let head_radius = positive(length(diameter)? * 0.5, "Countersink radius")?;
                let angle = expression::value(doc, &owner.definition, angle, Dimension::Angle)?;
                if head_radius <= radius + 1e-7
                    || !(1e-6..std::f64::consts::PI - 1e-6).contains(&angle)
                {
                    return Err(invalid(
                        "Countersink diameter must exceed the hole diameter and included angle must be in (0,180) degrees",
                    ));
                }
                let slope = (angle * 0.5).tan();
                let head_depth = (head_radius - radius) / slope;
                if !head_depth.is_finite() || head_depth >= depth - 1e-7 {
                    return Err(invalid("Countersink taper exceeds the hole depth"));
                }
                if thread
                    .as_ref()
                    .is_some_and(|thread| head_radius * 2. < thread.nominal_diameter_mm - 1e-7)
                {
                    return Err(invalid(
                        "Countersink diameter must accommodate the cosmetic thread nominal diameter",
                    ));
                }
                entry_head_depth = head_depth;
                vec![
                    (-clearance, head_radius + clearance * slope),
                    (head_depth, radius),
                ]
            }
        };
        if let Some(thread) = &thread {
            thread.validate_bore(definition.end, depth, entry_head_depth)?;
        }
        stations.push((depth, radius));
        drills.push(crate::drilled_pockets::Drill {
            point: point.clone(),
            origin,
            axis,
            stations,
        });
    }
    let mut result = Vec::new();
    let mut cut_positions = BTreeSet::new();
    'stocks: for source in body {
        let body_source = adapter.body_construction_origin(&definition.body, source)?;
        if let Some((solid, tools)) = crate::drilled_pockets::cut(source, &drills)? {
            for (point, cutter) in tools {
                cut_positions.insert(point.clone());
                generated.drill_tool(owner, &point, &body_source, &cutter)?;
            }
            result.push(solid);
        } else {
            let mut solid = source.clone();
            let original = source
                .face_iter()
                .map(|face| face.id())
                .collect::<std::collections::HashSet<_>>();
            let mut changed_carriers = BTreeMap::new();
            for drill in &drills {
                let cutter = tool(drill.origin, drill.axis, &drill.stations)?;
                generated.drill_tool(owner, &drill.point, &body_source, &cutter)?;
                let previous = solid
                    .face_iter()
                    .map(|face| face.id())
                    .collect::<std::collections::HashSet<_>>();
                let carriers = cutter
                    .face_iter()
                    .map(|face| serde_json::to_string(&face.surface()))
                    .collect::<std::result::Result<BTreeSet<_>, _>>()?;
                let Some(next) = crate::boolean_ops::binary_with_history(
                    &solid,
                    &cutter,
                    crate::boolean_ops::Mode::Difference,
                    tolerance,
                    &BTreeSet::from([definition.body.clone()]),
                    crate::revolution::Combination {
                        adapter,
                        owner,
                        generated,
                    },
                )?
                else {
                    // A consumed scoped body contributes no remaining hole
                    // boundary. Other scoped bodies may still carry it; the
                    // feature-level position check below decides that intent.
                    continue 'stocks;
                };
                solid = next;
                for face in solid
                    .face_iter()
                    .filter(|face| !previous.contains(&face.id()))
                {
                    if carriers.contains(&serde_json::to_string(&face.surface())?) {
                        changed_carriers.insert(drill.point.clone(), carriers.clone());
                        break;
                    }
                }
            }
            let final_carriers = solid
                .face_iter()
                .filter(|face| !original.contains(&face.id()))
                .map(|face| serde_json::to_string(&face.surface()))
                .collect::<std::result::Result<BTreeSet<_>, _>>()?;
            for (point, carriers) in changed_carriers {
                if !carriers.is_disjoint(&final_carriers) {
                    cut_positions.insert(point);
                }
            }
            result.push(solid);
        }
    }
    if let Some(drill) = drills
        .iter()
        .find(|drill| !cut_positions.contains(&drill.point))
    {
        return Err(invalid(format!(
            "Hole position {} has no remaining cut boundary on the target; move the center, reverse entry, change the start/depth, or avoid a later hole consuming it",
            drill.point
        )));
    }
    Ok(Shape::Bodies(result))
}
