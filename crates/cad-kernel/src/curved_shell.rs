//! Exact conical/cylindrical wall offsets with intersected planar end caps.
//! Circular containment is checked over the complete axial interval. Shared
//! topology and construction ancestry survive both open and closed shells.
use crate::{
    analytic_offset::{self, Carrier, EPS},
    shell_history::ShellHistory,
};
use cad_core::{CadError, Result};
use monstertruck::{
    geometry::prelude::{TryIntoHomogeneousBsplineCurve, TryIntoHomogeneousBsplineSurface},
    modeling::*,
};
use std::collections::{HashMap, HashSet};

fn invalid(message: &str) -> CadError {
    CadError::Invalid(format!("Shell: {message}"))
}
fn unsupported(message: &str) -> CadError {
    CadError::Unsupported(format!("Shell: {message}"))
}
fn point(p: Vector4) -> Result<Point3> {
    if p.w <= 0. || ![p.x, p.y, p.z, p.w].iter().all(|x| x.is_finite()) {
        return Err(unsupported(
            "Turned shell requires finite positive rational controls",
        ));
    }
    let p = Point3::from_vec(p.truncate() / p.w);
    if ![p.x, p.y, p.z].iter().all(|x| x.is_finite()) {
        return Err(invalid("Offset exceeds finite coordinates"));
    }
    Ok(p)
}
struct Wall {
    carrier: Carrier,
    distance: f64,
    contour: usize,
}
struct Turned {
    bottom: Face,
    top: Face,
    origin: Point3,
    axis: Vector3,
    height: f64,
    walls: HashMap<FaceId, Wall>,
    top_wires: Vec<usize>,
}
fn circle_wire(wire: &Wire, origin: Point3, axis: Vector3) -> Option<f64> {
    if !wire.is_closed() || wire.is_empty() {
        return None;
    }
    let mut radius = None;
    let mut sweep = 0.;
    let mut sign = 0.;
    for edge in wire {
        let ((center, normal, r), angle) =
            crate::bounded_carriers::circle(&edge.oriented_curve()).ok()?;
        if center.distance(origin) > EPS || normal.dot(axis).abs() < 1. - 1e-10 {
            return None;
        }
        if radius.is_some_and(|value: f64| (value - r).abs() > EPS) {
            return None;
        }
        radius = Some(r);
        let angle = angle * normal.dot(axis).signum();
        if sign != 0. && angle.signum() != sign {
            return None;
        }
        sign = angle.signum();
        sweep += angle;
    }
    ((sweep.abs() - std::f64::consts::TAU).abs() <= 1e-8)
        .then_some(radius?)
        .filter(|r| r.is_finite() && *r > EPS)
}
fn recognize(
    solid: &Solid,
    thickness: f64,
    overrides: &HashMap<FaceId, f64>,
) -> Result<Option<Turned>> {
    if solid.boundaries().len() != 1 {
        return Ok(None);
    }
    let faces = solid.face_iter().cloned().collect::<Vec<_>>();
    let mut carriers = HashMap::new();
    for face in &faces {
        let Some(carrier) = analytic_offset::recognize(face)? else {
            return Ok(None);
        };
        carriers.insert(face.id(), carrier);
    }
    let caps = faces
        .iter()
        .filter(|face| matches!(carriers[&face.id()], Carrier::Plane { .. }))
        .collect::<Vec<_>>();
    if caps.len() != 2 {
        return Ok(None);
    }
    let bottom = caps[0];
    let top = caps[1];
    let Carrier::Plane {
        origin: bottom_point,
        ..
    } = carriers[&bottom.id()]
    else {
        unreachable!()
    };
    let Carrier::Plane {
        origin: top_point, ..
    } = carriers[&top.id()]
    else {
        unreachable!()
    };
    let axis = -bottom.oriented_surface().normal(0., 0.).normalize();
    if !axis.magnitude().is_finite()
        || top.oriented_surface().normal(0., 0.).normalize().dot(axis) < 1. - 1e-10
    {
        return Ok(None);
    }
    let height = (top_point - bottom_point).dot(axis);
    if !height.is_finite() || height <= EPS {
        return Ok(None);
    }
    let Some(wall) = faces
        .iter()
        .find(|face| matches!(carriers[&face.id()], Carrier::Cone { .. }))
    else {
        return Ok(None);
    };
    let Carrier::Cone {
        origin: wall_origin,
        axis: wall_axis,
        ..
    } = carriers[&wall.id()]
    else {
        unreachable!()
    };
    if wall_axis.dot(axis).abs() < 1. - 1e-10 {
        return Ok(None);
    }
    let origin = wall_origin + axis * (bottom_point - wall_origin).dot(axis);
    let lower = bottom.boundaries();
    let upper = top.boundaries();
    if lower.len() != upper.len() {
        return Ok(None);
    }
    let Some(bottom_radii) = lower
        .iter()
        .map(|wire| circle_wire(wire, origin, axis))
        .collect::<Option<Vec<_>>>()
    else {
        return Ok(None);
    };
    let Some(top_radii) = upper
        .iter()
        .map(|wire| circle_wire(wire, origin + axis * height, axis))
        .collect::<Option<Vec<_>>>()
    else {
        return Ok(None);
    };
    if lower.len() > 2 {
        return Err(unsupported(
            "Coaxial turned shell has more than one cavity loop",
        ));
    }
    if lower.len() == 2
        && (bottom_radii[0] <= bottom_radii[1] + EPS || top_radii[0] <= top_radii[1] + EPS)
    {
        return Err(invalid("Turned shell boundary nesting is inconsistent"));
    }
    let mut walls = HashMap::new();
    let mut top_wires = vec![None; lower.len()];
    for face in faces
        .iter()
        .filter(|face| face.id() != bottom.id() && face.id() != top.id())
    {
        let Carrier::Cone {
            origin: wall_origin,
            axis: wall_axis,
            radius,
            slope,
        } = carriers[&face.id()]
        else {
            return Ok(None);
        };
        if wall_axis.dot(axis).abs() < 1. - 1e-10
            || (wall_origin - origin).cross(axis).magnitude() > EPS
        {
            return Ok(None);
        }
        let rb = radius + slope * (origin - wall_origin).dot(wall_axis);
        let rt = radius + slope * (origin + axis * height - wall_origin).dot(wall_axis);
        let Some(contour) = bottom_radii.iter().position(|r| (*r - rb).abs() <= EPS) else {
            return Ok(None);
        };
        let Some(top_index) = top_radii.iter().position(|r| (*r - rt).abs() <= EPS) else {
            return Ok(None);
        };
        if top_wires[contour].is_some_and(|index| index != top_index) {
            return Err(invalid("Turned shell wall correspondence is ambiguous"));
        }
        top_wires[contour] = Some(top_index);
        let boundary = face.boundaries();
        if boundary.len() != 1 {
            return Ok(None);
        }
        for edge in &boundary[0] {
            let curve = edge.oriented_curve();
            if let Ok(((center, normal, _), _)) = crate::bounded_carriers::circle(&curve) {
                let z = (center - origin).dot(axis);
                if normal.dot(axis).abs() < 1. - 1e-10 || z.abs() > EPS && (z - height).abs() > EPS
                {
                    return Ok(None);
                }
            } else {
                if crate::bounded_carriers::line(&curve).is_err() {
                    return Ok(None);
                }
                let a = edge.front().point();
                let b = edge.back().point();
                let za = (a - origin).dot(axis);
                let zb = (b - origin).dot(axis);
                if !((za.abs() <= EPS && (zb - height).abs() <= EPS)
                    || (zb.abs() <= EPS && (za - height).abs() <= EPS))
                {
                    return Ok(None);
                }
                let ra = a - origin - axis * za;
                let rb = b - origin - axis * zb;
                if ra.normalize().dot(rb.normalize()) < 1. - 1e-10 {
                    return Ok(None);
                }
            }
        }
        let distance = overrides.get(&face.id()).copied().unwrap_or(thickness);
        walls.insert(
            face.id(),
            Wall {
                carrier: carriers[&face.id()],
                distance,
                contour,
            },
        );
    }
    for contour in 0..lower.len() {
        let group = walls
            .values()
            .filter(|wall| wall.contour == contour)
            .collect::<Vec<_>>();
        let Some(first) = group.first() else {
            return Ok(None);
        };
        if group.iter().any(|wall| {
            (wall.distance - first.distance).abs() > EPS || !wall.carrier.same(first.carrier)
        }) {
            return Err(unsupported(
                "Angular patches of one conical wall require equal thickness",
            ));
        }
    }
    let Some(top_wires) = top_wires.into_iter().collect() else {
        return Ok(None);
    };
    Ok(Some(Turned {
        bottom: bottom.clone(),
        top: top.clone(),
        origin,
        axis,
        height,
        walls,
        top_wires,
    }))
}
struct Refit {
    origin: Point3,
    axis: Vector3,
    old_height: f64,
    lower: f64,
    upper: f64,
    carrier: Carrier,
    distance: f64,
}
impl Refit {
    fn point(&self, p: Point3) -> Result<Point3> {
        let Carrier::Cone {
            origin,
            axis,
            radius,
            slope,
        } = self.carrier
        else {
            return Err(invalid("Missing turned wall carrier"));
        };
        let z = (p - self.origin).dot(self.axis);
        let next_z = self.lower + (self.upper - self.lower) * z / self.old_height;
        let old_r = radius + slope * (p - origin).dot(axis);
        let on_axis = self.origin + self.axis * next_z;
        let next_r = radius
            + slope * (on_axis - origin).dot(axis)
            + self.distance * (1. + slope * slope).sqrt();
        if !old_r.is_finite() || !next_r.is_finite() || old_r <= EPS || next_r <= EPS {
            return Err(invalid(
                "Wall thickness collapses the conical cavity or reaches its axis",
            ));
        }
        let result = on_axis + (p - self.origin - self.axis * z) * (next_r / old_r);
        if ![result.x, result.y, result.z].iter().all(|x| x.is_finite()) {
            return Err(invalid("Offset exceeds finite coordinates"));
        }
        Ok(result)
    }
    fn curve(&self, curve: &Curve) -> Result<Curve> {
        if crate::axis_geometry::line_axis(curve).is_ok() {
            let (a, b) = curve.range_tuple();
            return Ok(Curve::Line(Line(
                self.point(curve.subs(a))?,
                self.point(curve.subs(b))?,
            )));
        }
        crate::bounded_carriers::circle(curve)?;
        let mut native = curve
            .try_into_homogeneous_bspline_curve()
            .ok_or_else(|| unsupported("Turned boundary is not an exact rational curve"))?;
        for p in native.control_points_mut() {
            *p = self.point(point(*p)?)?.to_homogeneous() * p.w;
        }
        Ok(Curve::NurbsCurve(NurbsCurve::new(native)))
    }
    fn surface(&self, surface: &Surface) -> Result<Surface> {
        if let Surface::RevolutionSurface(processor) = surface {
            let inverse = processor
                .transform()
                .invert()
                .ok_or_else(|| invalid("Singular rotational carrier"))?;
            let profile = processor
                .entity()
                .entity_curve()
                .transformed(*processor.transform());
            let profile = self.curve(&profile)?.transformed(inverse);
            return Ok(Surface::RevolutionSurface(processor.map_ref(|source| {
                RevolutionSurface::by_revolution(profile, source.origin(), source.axis())
            })));
        }
        let mut net = surface
            .try_into_homogeneous_bspline_surface()
            .ok_or_else(|| unsupported("Turned wall has no exact rational carrier"))?;
        for p in net.control_points_mut() {
            *p = self.point(point(*p)?)?.to_homogeneous() * p.w;
        }
        Ok(Surface::NurbsSurface(NurbsSurface::new(net)))
    }
}

pub(crate) fn shell(
    solid: &Solid,
    thickness: f64,
    removed: &[FaceId],
    outward: bool,
    overrides: &HashMap<FaceId, f64>,
    history: &mut ShellHistory,
) -> Result<Option<Vec<Solid>>> {
    // Complete spherical shells retain their exact analytic carrier and seam
    // topology. Openings are connected to their exact inward/outward boundary.
    if solid.boundaries().len() == 1
        && solid.face_iter().all(|face| {
            matches!(
                analytic_offset::recognize(face),
                Ok(Some(Carrier::Sphere { .. }))
            )
        })
    {
        return spherical(solid, thickness, removed, outward, overrides, history).map(Some);
    }
    let Some(turned) = recognize(solid, thickness, overrides)? else {
        return crate::turned_shell::shell(solid, thickness, removed, outward, overrides, history);
    };
    if removed
        .iter()
        .any(|id| *id != turned.bottom.id() && *id != turned.top.id())
    {
        return Ok(None);
    }
    let open_bottom = removed.contains(&turned.bottom.id());
    let open_top = removed.contains(&turned.top.id());
    let direction = if outward { 1. } else { -1. };
    let bottom_thickness = overrides
        .get(&turned.bottom.id())
        .copied()
        .unwrap_or(thickness);
    let top_thickness = overrides
        .get(&turned.top.id())
        .copied()
        .unwrap_or(thickness);
    let lower = if open_bottom {
        0.
    } else {
        -direction * bottom_thickness
    };
    let upper = turned.height
        + if open_top {
            0.
        } else {
            direction * top_thickness
        };
    if !lower.is_finite() || !upper.is_finite() || upper - lower <= EPS {
        return Err(invalid("End-wall thickness collapses the cavity"));
    }
    let mut refits = HashMap::new();
    for (face, wall) in &turned.walls {
        let source = solid
            .face_iter()
            .find(|source| source.id() == *face)
            .ok_or_else(|| invalid("Wall source is missing"))?;
        refits.insert(
            *face,
            Refit {
                origin: turned.origin,
                axis: turned.axis,
                old_height: turned.height,
                lower,
                upper,
                carrier: wall.carrier,
                distance: direction * wall.distance * analytic_offset::side(source, wall.carrier)?,
            },
        );
    }
    let mut edges = HashMap::new();
    let mut vertices: HashMap<VertexId, Vertex> = HashMap::new();
    for face in solid
        .face_iter()
        .filter(|face| turned.walls.contains_key(&face.id()))
    {
        let refit = &refits[&face.id()];
        for edge in face.edge_iter() {
            for vertex in [edge.absolute_front(), edge.absolute_back()] {
                let p = refit.point(vertex.point())?;
                if let Some(existing) = vertices.get(&vertex.id()) {
                    if existing.point().distance(p) > EPS {
                        return Err(invalid("Adjacent offset walls do not meet"));
                    }
                } else {
                    vertices.insert(vertex.id(), builder::vertex(p));
                }
            }
            if let std::collections::hash_map::Entry::Vacant(slot) = edges.entry(edge.id()) {
                slot.insert(
                    Edge::try_new(
                        &vertices[&edge.absolute_front().id()],
                        &vertices[&edge.absolute_back().id()],
                        refit.curve(&edge.curve())?,
                    )
                    .map_err(|e| invalid(&e.to_string()))?,
                );
            }
        }
    }
    // Radial nesting is affine over the complete height; endpoint inequalities
    // therefore certify that opposite walls cannot cross in between.
    let mut radii = HashMap::new();
    for (id, wall) in &turned.walls {
        let refit = &refits[id];
        let Carrier::Cone {
            origin,
            axis,
            radius,
            slope,
        } = wall.carrier
        else {
            unreachable!()
        };
        let r = |z: f64| {
            radius
                + slope * (turned.origin + turned.axis * z - origin).dot(axis)
                + refit.distance * (1. + slope * slope).sqrt()
        };
        let pair = [r(lower), r(upper)];
        if pair.iter().any(|r| !r.is_finite() || *r <= EPS) {
            return Err(invalid("Thickness closes a conical wall at the axis"));
        }
        radii.insert(wall.contour, pair);
    }
    if let (Some(outer), Some(inner)) = (radii.get(&0), radii.get(&1))
        && (0..2).any(|i| outer[i] <= inner[i] + EPS)
    {
        return Err(invalid(
            "Thickness makes the inner and outer conical walls collide",
        ));
    }
    let mut changed = Shell::new();
    let mut mapped_faces = HashMap::new();
    for face in solid.face_iter() {
        let wires = face
            .boundaries()
            .into_iter()
            .map(|wire| {
                wire.iter()
                    .map(|edge| {
                        let mapped = edges.get(&edge.id()).ok_or_else(|| {
                            unsupported("End cap has an internal subdivision without wall ancestry")
                        })?;
                        Ok(if edge.orientation() {
                            mapped.clone()
                        } else {
                            mapped.inverse()
                        })
                    })
                    .collect::<Result<Wire>>()
            })
            .collect::<Result<Vec<_>>>()?;
        let mapped = if let Some(refit) = refits.get(&face.id()) {
            Face::try_new(wires, refit.surface(&face.oriented_surface())?)
                .map_err(|e| invalid(&e.to_string()))?
        } else {
            builder::try_attach_plane(wires).map_err(|e| invalid(&e.to_string()))?
        };
        history.construct_face(mapped.id(),&[history.face(face.id())?],serde_json::json!({"operation":"shell","constructor":if outward{"outer_normal_offset"}else{"inner_normal_offset"}}))?;
        mapped_faces.insert(face.id(), mapped.clone());
        changed.push(mapped);
    }
    for (source, edge) in &edges {
        history.construct_edge(
            edge.id(),
            &[history.edge(*source)?],
            serde_json::json!({"operation":"shell","constructor":"normal_offset_boundary"}),
        )?;
    }
    crate::validation::solid(
        &Solid::try_new(vec![changed.clone()]).map_err(|e| invalid(&e.to_string()))?,
    )?;
    let original = &solid.boundaries()[0];
    let (outer, inner) = if outward {
        (&changed, original)
    } else {
        (original, &changed)
    };
    let original_open = removed.iter().copied().collect::<HashSet<_>>();
    let changed_open = removed
        .iter()
        .map(|id| mapped_faces[id].id())
        .collect::<HashSet<_>>();
    let (outer_open, inner_open) = if outward {
        (&changed_open, &original_open)
    } else {
        (&original_open, &changed_open)
    };
    let mut boundary = outer
        .iter()
        .filter(|face| !outer_open.contains(&face.id()))
        .cloned()
        .collect::<Shell>();
    boundary.extend(
        inner
            .iter()
            .filter(|face| !inner_open.contains(&face.id()))
            .map(Face::inverse),
    );
    for original_cap in [&turned.bottom, &turned.top] {
        if !original_open.contains(&original_cap.id()) {
            continue;
        }
        let mapped = &mapped_faces[&original_cap.id()];
        let before = original_cap.boundaries();
        let after = mapped.boundaries();
        let indices = if original_cap.id() == turned.top.id() {
            turned.top_wires.clone()
        } else {
            (0..before.len()).collect()
        };
        for (contour, index) in indices.into_iter().enumerate() {
            let (outer, inner) = if outward {
                (&after[index], &before[index])
            } else {
                (&before[index], &after[index])
            };
            let wires = if contour == 0 {
                vec![outer.clone(), inner.inverse()]
            } else {
                vec![inner.inverse(), outer.clone()]
            };
            let rim = builder::try_attach_plane(wires).map_err(|e| invalid(&e.to_string()))?;
            let mut sources = vec![history.face(original_cap.id())?];
            sources.extend(
                before[index]
                    .iter()
                    .map(|edge| history.edge(edge.id()))
                    .collect::<Result<Vec<_>>>()?,
            );
            history.rim(
                rim.id(),
                original_cap.id(),
                &sources,
                serde_json::json!({"operation":"shell","constructor":"normal_offset_opening"}),
            )?;
            boundary.push(rim);
        }
    }
    let components = boundary.connected_components();
    let results = if removed.is_empty() {
        vec![Solid::try_new(components).map_err(|e| invalid(&e.to_string()))?]
    } else {
        components
            .into_iter()
            .map(|shell| Solid::try_new(vec![shell]).map_err(|e| invalid(&e.to_string())))
            .collect::<Result<Vec<_>>>()?
    };
    for solid in &results {
        crate::validation::solid(solid)?;
    }
    Ok(Some(results))
}

fn spherical(
    solid: &Solid,
    thickness: f64,
    removed: &[FaceId],
    outward: bool,
    overrides: &HashMap<FaceId, f64>,
    history: &mut ShellHistory,
) -> Result<Vec<Solid>> {
    let faces = solid.face_iter().cloned().collect::<Shell>();
    let mut retained = faces
        .iter()
        .filter(|face| !removed.contains(&face.id()))
        .cloned()
        .collect::<Shell>();
    if retained.is_empty() {
        return Err(invalid(
            "At least one wall must remain after removing faces",
        ));
    }
    if overrides
        .iter()
        .any(|(face, t)| !removed.contains(face) && (*t - thickness).abs() > EPS)
    {
        return Err(unsupported(
            "Joined spherical patches require uniform normal wall thickness",
        ));
    }
    let offset = analytic_offset::offset(&retained, if outward { thickness } else { -thickness })?;
    for face in &retained {
        history.construct_face(offset.faces[&face.id()],&[history.face(face.id())?],serde_json::json!({"operation":"shell","constructor":if outward{"outer_normal_offset"}else{"inner_normal_offset"}}))?;
    }
    for (source, edge) in &offset.edges {
        history.construct_edge(
            edge.id(),
            &[history.edge(*source)?],
            serde_json::json!({"operation":"shell","constructor":"normal_offset_boundary"}),
        )?;
    }
    let exposed = retained.extract_boundaries();
    if outward {
        retained.invert();
        retained.extend(offset.shell.iter().cloned());
    } else {
        retained.extend(offset.shell.inverse());
    }
    for wire in exposed {
        let changed = wire
            .iter()
            .map(|edge| {
                if edge.orientation() {
                    offset.edges[&edge.id()].clone()
                } else {
                    offset.edges[&edge.id()].inverse()
                }
            })
            .collect::<Wire>();
        crate::surface_edit::extrudable_edges(wire.iter().cloned())?;
        crate::surface_edit::extrudable_edges(changed.iter().cloned())?;
        let rim = crate::loft_correspondence::skin(&wire, &changed, None, None)?;
        for (edge, face) in wire.iter().zip(rim.iter()) {
            let face = if outward {
                face.clone()
            } else {
                face.inverse()
            };
            history.construct_face(face.id(),&[history.edge(edge.id())?],serde_json::json!({"operation":"shell","constructor":"spherical_opening_normal_rim"}))?;
            retained.push(face);
        }
    }
    let components = retained.connected_components();
    let results = if removed.is_empty() {
        vec![Solid::try_new(components).map_err(|e| invalid(&e.to_string()))?]
    } else {
        components
            .into_iter()
            .map(|shell| Solid::try_new(vec![shell]).map_err(|e| invalid(&e.to_string())))
            .collect::<Result<Vec<_>>>()?
    };
    for result in &results {
        crate::validation::solid(result)?;
    }
    Ok(results)
}
