//! Exact wall offsets for proven constant-section straight/circular prisms.
use cad_core::{CadError, Document, Expr, Result, Sketch, SketchCurve, SketchMember};
use monstertruck::{
    geometry::prelude::{TryIntoHomogeneousBsplineCurve, TryIntoHomogeneousBsplineSurface},
    modeling::*,
};
use std::collections::{BTreeMap, HashMap, HashSet};

const EPS: f64 = 1e-7;
fn invalid(message: impl std::fmt::Display) -> CadError {
    CadError::Invalid(format!("Shell: {message}"))
}
fn finite(p: Point3) -> bool {
    [p.x, p.y, p.z].iter().all(|v| v.is_finite())
}
fn rational_point(h: Vector4) -> Option<Point3> {
    if ![h.x, h.y, h.z, h.w].iter().all(|v| v.is_finite()) || h.w <= 0. {
        return None;
    }
    let point = Point3::new(h.x / h.w, h.y / h.w, h.z / h.w);
    finite(point).then_some(point)
}
fn same_curve(a: &Curve, b: &Curve, delta: Vector3) -> bool {
    let Some(mut a) = a.try_into_homogeneous_bspline_curve() else {
        return false;
    };
    let Some(mut b) = b.try_into_homogeneous_bspline_curve() else {
        return false;
    };
    a.clamp();
    b.clamp();
    if a.degree() != b.degree()
        || a.control_points().len() != b.control_points().len()
        || a.knot_vector().len() != b.knot_vector().len()
    {
        return false;
    }
    let (a0, a1) = a.range_tuple();
    let (b0, b1) = b.range_tuple();
    if !(a1 - a0).is_finite() || !(b1 - b0).is_finite() || a1 <= a0 || b1 <= b0 {
        return false;
    }
    for reverse in [false, true] {
        let controls = a
            .control_points()
            .iter()
            .zip(b.control_points().iter().enumerate())
            .all(|(ha, (index, _))| {
                let hb = b.control_points()[if reverse {
                    b.control_points().len() - 1 - index
                } else {
                    index
                }];
                let Some(pa) = rational_point(*ha) else {
                    return false;
                };
                let Some(pb) = rational_point(hb) else {
                    return false;
                };
                let wa = ha.w / a.control_points()[0].w;
                let wb = hb.w
                    / b.control_points()[if reverse {
                        b.control_points().len() - 1
                    } else {
                        0
                    }]
                    .w;
                wa.is_finite()
                    && wb.is_finite()
                    && (wa - wb).abs() <= 1e-12 * wa.abs().max(wb.abs())
                    && (pa + delta).distance(pb) <= EPS
            });
        let knots = a.knot_vector().iter().enumerate().all(|(index, knot)| {
            let ka = (*knot - a0) / (a1 - a0);
            let kb = if reverse {
                (b1 - b.knot_vector()[b.knot_vector().len() - 1 - index]) / (b1 - b0)
            } else {
                (b.knot_vector()[index] - b0) / (b1 - b0)
            };
            ka.is_finite() && kb.is_finite() && (ka - kb).abs() <= 1e-12
        });
        if controls && knots {
            return true;
        }
    }
    false
}
fn ruled_translation(face: &Face, axis: Vector3, height: f64) -> bool {
    if let Surface::Plane(_) = face.surface() {
        let normal = face.oriented_surface().normal(0., 0.).normalize();
        return normal.magnitude2().is_finite() && normal.dot(axis).abs() < 1e-10;
    }
    let Some(mut surface) = face.surface().try_into_homogeneous_bspline_surface() else {
        return false;
    };
    if surface.vdegree() != 1 {
        if surface.udegree() == 1 {
            surface.invert();
        } else {
            return false;
        }
    }
    if surface.vdegree() != 1
        || surface.control_points().is_empty()
        || surface.control_points()[0].len() != 2
    {
        return false;
    }
    let first = &surface.control_points()[0];
    let Some(p) = rational_point(first[0]) else {
        return false;
    };
    let Some(q) = rational_point(first[1]) else {
        return false;
    };
    let delta = q - p;
    if (delta - axis * height).magnitude() > EPS && (delta + axis * height).magnitude() > EPS {
        return false;
    }
    surface.control_points().iter().all(|row| {
        let Some(p) = rational_point(row[0]) else {
            return false;
        };
        let Some(q) = rational_point(row[1]) else {
            return false;
        };
        let ratio = row[1].w / row[0].w;
        ratio.is_finite()
            && (ratio - first[1].w / first[0].w).abs() <= 1e-12 * ratio.abs()
            && (q - p - delta).magnitude() <= EPS
    })
}
struct Prism {
    bottom: Face,
    top: Face,
    axis: Vector3,
    height: f64,
    top_wires: Vec<usize>,
}
fn recognize(solid: &Solid) -> Option<Prism> {
    if solid.boundaries().len() != 1 {
        return None;
    }
    let faces: Vec<_> = solid.face_iter().cloned().collect();
    for bottom in faces
        .iter()
        .filter(|face| matches!(face.surface(), Surface::Plane(_)))
    {
        let axis = -bottom.oriented_surface().normal(0., 0.).normalize();
        if !axis.magnitude2().is_finite() {
            continue;
        }
        let origin = bottom.vertex_iter().next()?.point();
        for top in faces
            .iter()
            .filter(|face| face.id() != bottom.id() && matches!(face.surface(), Surface::Plane(_)))
        {
            if top.oriented_surface().normal(0., 0.).normalize().dot(axis) < 1. - 1e-10 {
                continue;
            }
            let height = (top.vertex_iter().next()?.point() - origin).dot(axis);
            if !height.is_finite() || height <= EPS {
                continue;
            }
            let lower = bottom.boundaries();
            let upper = top.boundaries();
            if lower.len() != upper.len() {
                continue;
            }
            let delta = axis * height;
            let mut used = HashSet::new();
            let mut top_wires = Vec::new();
            for wire in &lower {
                let candidates: Vec<_> = upper
                    .iter()
                    .enumerate()
                    .filter(|(index, candidate)| {
                        !used.contains(index)
                            && wire.len() == candidate.len()
                            && wire.iter().all(|edge| {
                                candidate
                                    .iter()
                                    .filter(|other| {
                                        same_curve(&edge.curve(), &other.curve(), delta)
                                    })
                                    .count()
                                    == 1
                            })
                    })
                    .map(|(index, _)| index)
                    .collect();
                if candidates.len() != 1 {
                    break;
                }
                used.insert(candidates[0]);
                top_wires.push(candidates[0]);
            }
            if top_wires.len() != lower.len() {
                continue;
            }
            let ends: HashSet<_> = bottom
                .edge_iter()
                .chain(top.edge_iter())
                .map(|edge| edge.id())
                .collect();
            let walls: Vec<_> = faces
                .iter()
                .filter(|face| face.id() != bottom.id() && face.id() != top.id())
                .collect();
            if !walls.iter().all(|wall| {
                let boundaries = wall.boundaries();
                boundaries.len() == 1
                    && ruled_translation(wall, axis, height)
                    && wall.edge_iter().all(|edge| {
                        ends.contains(&edge.id())
                            || crate::axis_geometry::line_axis(&edge.curve()).is_ok_and(|line| {
                                Vector3::from(line.direction).dot(axis).abs() > 1. - 1e-10
                            })
                    })
                    && wall.vertex_iter().all(|vertex| {
                        let z = (vertex.point() - origin).dot(axis);
                        z.abs() <= EPS || (z - height).abs() <= EPS
                    })
            }) {
                continue;
            }
            if lower.iter().flatten().any(|edge| {
                walls
                    .iter()
                    .filter(|wall| {
                        wall.edge_iter()
                            .any(|candidate| candidate.id() == edge.id())
                    })
                    .count()
                    != 1
            }) {
                continue;
            }
            return Some(Prism {
                bottom: bottom.clone(),
                top: top.clone(),
                axis,
                height,
                top_wires,
            });
        }
    }
    None
}

pub(crate) fn wire_circle(wire: &Wire) -> Result<Option<(Point3, Vector3, f64)>> {
    let mut result: Option<(Point3, Vector3, f64)> = None;
    let mut sweep: f64 = 0.;
    for edge in wire {
        let (geometry, angle) = match crate::bounded_carriers::circle(&edge.oriented_curve()) {
            Ok(geometry) => geometry,
            Err(CadError::Unsupported(_)) => return Ok(None),
            Err(error) => return Err(error),
        };
        if result.is_some_and(|(center, normal, radius)| {
            center.distance(geometry.0) > EPS
                || (radius - geometry.2).abs() > EPS
                || normal.dot(geometry.1).abs() < 1. - 1e-10
        }) {
            return Ok(None);
        }
        if sweep != 0. && sweep.signum() != angle.signum() {
            return Ok(None);
        }
        sweep += angle;
        result = Some(geometry);
    }
    if (sweep.abs() - std::f64::consts::TAU).abs() > 1e-9 {
        return Ok(None);
    }
    Ok(result)
}

pub(crate) struct ExpandedDrill {
    pub drill: crate::drilled_pockets::Drill,
    pub entry: FaceId,
    pub end: FaceId,
}
pub(crate) fn expanded_drill(
    solid: &Solid,
    openings: &[FaceId],
    thickness: f64,
    overrides: &HashMap<FaceId, f64>,
    point: String,
) -> Result<Option<ExpandedDrill>> {
    let Some(prism) = recognize(solid) else {
        return Ok(None);
    };
    if prism.bottom.boundaries().len() != 1
        || openings.is_empty()
        || openings.len() > 2
        || openings
            .iter()
            .any(|id| *id != prism.bottom.id() && *id != prism.top.id())
    {
        return Ok(None);
    }
    let (entry, end, axis) = if openings.contains(&prism.bottom.id()) {
        (&prism.bottom, &prism.top, prism.axis)
    } else {
        (&prism.top, &prism.bottom, -prism.axis)
    };
    let Some((origin, normal, radius)) = wire_circle(&entry.boundaries()[0])? else {
        return Ok(None);
    };
    if normal.dot(axis).abs() < 1. - 1e-10 {
        return Ok(None);
    }
    let amounts = solid
        .face_iter()
        .filter(|face| face.id() != entry.id() && face.id() != end.id())
        .map(|face| overrides.get(&face.id()).copied().unwrap_or(thickness))
        .collect::<Vec<_>>();
    let Some(amount) = amounts.first().copied() else {
        return Ok(None);
    };
    if amounts.iter().any(|value| (*value - amount).abs() > EPS) {
        return Ok(None);
    }
    let radius = radius + amount;
    let end_amount = if openings.contains(&end.id()) {
        0.
    } else {
        overrides.get(&end.id()).copied().unwrap_or(thickness)
    };
    let depth = prism.height + end_amount;
    if !radius.is_finite() || !depth.is_finite() || !finite(origin + axis * depth) {
        return Err(invalid("Expanded pocket exceeds finite coordinates"));
    }
    Ok(Some(ExpandedDrill {
        entry: entry.id(),
        end: end.id(),
        drill: crate::drilled_pockets::Drill {
            point,
            origin,
            axis,
            stations: vec![(-EPS * 4., radius), (depth, radius)],
        },
    }))
}
struct OffsetSection {
    face: Face,
    origins: HashMap<EdgeId, OffsetEdge>,
}
#[derive(Clone)]
struct OffsetEdge {
    wall: FaceId,
    source: EdgeId,
    contour: usize,
    part: usize,
}
fn offset_section(
    doc: &Document,
    owner: &str,
    cap: &Face,
    distances: &[f64],
    inward: bool,
    walls: &HashMap<EdgeId, FaceId>,
) -> Result<OffsetSection> {
    let n = cap.oriented_surface().normal(0., 0.).normalize();
    let u = n
        .cross(if n.x.abs() < 0.8 {
            Vector3::unit_x()
        } else {
            Vector3::unit_y()
        })
        .normalize();
    let v = n.cross(u);
    let origin = cap
        .vertex_iter()
        .next()
        .ok_or_else(|| invalid("Section has no boundary"))?
        .point();
    let plane = cad_core::Plane {
        origin: [origin.x, origin.y, origin.z],
        u: [u.x, u.y, u.z],
        v: [v.x, v.y, v.z],
    };
    let xy = |p: Point3| {
        let d = p - origin;
        [d.dot(u), d.dot(v)]
    };
    let mut sketch: Sketch = serde_json::from_value(
        serde_json::json!({"id":"@shell-section","name":"Derived shell section","points":{},"segments":[],"profile_order":[]}),
    )?;
    let mut operations = Vec::new();
    let mut sources = Vec::new();
    let mut inputs = HashMap::new();
    for (index, wire) in cap.boundaries().iter().enumerate() {
        let mut members = Vec::new();
        for (ordinal, edge) in wire.iter().enumerate() {
            let start = format!("c{index}:p{ordinal}");
            let end = format!("c{index}:p{}", (ordinal + 1) % wire.len());
            sketch
                .points
                .insert(start.clone(), xy(edge.front().point()));
            let curve = if crate::axis_geometry::line_axis(&edge.curve()).is_ok() {
                crate::bounded_carriers::line(&edge.oriented_curve())?;
                SketchCurve::Line { start, end }
            } else {
                let ((_, normal, _), _) = crate::bounded_carriers::circle(&edge.oriented_curve())?;
                if normal.dot(n).abs() < 1. - 1e-10 {
                    return Err(invalid("Circular section edge is not in its cap plane"));
                }
                let curve = edge.oriented_curve();
                let (a, b) = curve.range_tuple();
                let through = format!("c{index}:a{ordinal}");
                sketch
                    .points
                    .insert(through.clone(), xy(curve.subs((a + b) * 0.5)));
                SketchCurve::Arc {
                    start,
                    through,
                    end,
                }
            };
            let member = format!("c{index}:m{ordinal}");
            inputs.insert(
                member.clone(),
                OffsetEdge {
                    wall: *walls
                        .get(&edge.id())
                        .ok_or_else(|| invalid("Offset member's source wall is missing"))?,
                    source: edge.id(),
                    contour: index,
                    part: 0,
                },
            );
            sketch.members.insert(
                member.clone(),
                SketchMember {
                    construction: true,
                    curve,
                },
            );
            members.push(member);
        }
        sources.push(members.clone());
        let operation = cad_core::sketch_edit::add_offset(
            &mut sketch,
            &members,
            Expr(format!("{} mm", distances[index])),
            if index == 0 { inward } else { !inward },
            false,
        )?;
        let mut offset = sketch.offsets.remove(&operation).unwrap();
        let mut points = BTreeMap::new();
        for (ordinal, member) in offset.members.iter_mut().enumerate() {
            let old = member.member.clone();
            member.member = format!("c{index}:offset:m{ordinal}");
            let mut generated = sketch.members.remove(&old).unwrap();
            for point in &mut member.points {
                let next = format!("c{index}:offset:p{}", points.len());
                let stable = points.entry(point.clone()).or_insert_with(|| next).clone();
                if let Some(value) = sketch.points.remove(point) {
                    sketch.points.insert(stable.clone(), value);
                }
                *point = stable;
            }
            generated.curve = SketchCurve::Line {
                start: member.points[0].clone(),
                end: member.points.last().unwrap().clone(),
            };
            sketch.members.insert(member.member.clone(), generated);
        }
        let operation = format!("c{index}:offset");
        sketch.offsets.insert(operation.clone(), offset);
        operations.push(operation);
    }
    cad_core::sketch_edit::derive(doc, owner, &mut sketch)?;
    let derived = operations
        .iter()
        .map(|operation| {
            sketch.offsets[operation]
                .members
                .iter()
                .map(|member| member.member.clone())
                .collect()
        })
        .collect::<Vec<_>>();
    crate::section_checks::nesting(&sketch, &sources, &derived, inward)?;
    if sketch
        .points
        .values()
        .any(|p| !finite(origin + u * p[0] + v * p[1]))
    {
        return Err(invalid(
            "Offset section exceeds finite document coordinates",
        ));
    }
    let built = crate::sketches::build_traced(doc, owner, &sketch, &plane)?;
    let generated_members = sketch
        .offsets
        .values()
        .flat_map(|operation| {
            operation
                .members
                .iter()
                .map(|member| (member.member.clone(), member.source.clone()))
        })
        .collect::<HashMap<_, _>>();
    let origins = built
        .members
        .into_iter()
        .map(|(edge, (member, part))| {
            let source = generated_members
                .get(&member)
                .and_then(|member| inputs.get(member))
                .ok_or_else(|| invalid("Offset curve lost its source constructor identity"))?;
            Ok((
                edge,
                OffsetEdge {
                    part,
                    ..source.clone()
                },
            ))
        })
        .collect::<Result<HashMap<_, _>>>()?;
    let crate::adapter::Shape::Sketch { mut faces, paths } = built.shape else {
        return Err(invalid("Offset section did not build native curves"));
    };
    if faces.len() != 1 || !paths.is_empty() {
        return Err(invalid(
            "Shell offset changes section topology or removes its cavity",
        ));
    }
    let face = faces.remove(0);
    if face.boundaries().len() != operations.len() {
        return Err(invalid("Shell offset closes or splits a boundary"));
    }
    Ok(OffsetSection { face, origins })
}
fn rings(
    outer: &Face,
    inner: &Face,
    outer_wires: &[Wire],
    inner_wires: &[Wire],
) -> Result<Vec<Face>> {
    let normal = outer.oriented_surface().normal(0., 0.).normalize();
    if normal.dot(inner.oriented_surface().normal(0., 0.).normalize()) < 1. - 1e-10 {
        return Err(invalid("Opening section orientations disagree"));
    }
    outer_wires
        .iter()
        .zip(inner_wires)
        .enumerate()
        .map(|(index, (outer, inner))| {
            let wires = if index == 0 {
                vec![outer.clone(), inner.inverse()]
            } else {
                vec![inner.inverse(), outer.clone()]
            };
            builder::try_attach_plane(wires).map_err(invalid)
        })
        .collect()
}
pub(crate) fn shell(
    doc: &Document,
    solid: &Solid,
    thickness: f64,
    removed: &[FaceId],
    outward: bool,
    overrides: &HashMap<FaceId, f64>,
    history: &mut crate::shell_history::ShellHistory,
) -> Result<Option<Vec<Solid>>> {
    let Some(prism) = recognize(solid) else {
        return Ok(None);
    };
    if removed
        .iter()
        .any(|id| *id != prism.bottom.id() && *id != prism.top.id())
    {
        return Ok(None);
    }
    let cap_bottom = overrides
        .get(&prism.bottom.id())
        .copied()
        .unwrap_or(thickness);
    let cap_top = overrides.get(&prism.top.id()).copied().unwrap_or(thickness);
    let mut distances = Vec::new();
    let mut walls = HashMap::new();
    for wire in prism.bottom.boundaries() {
        let values = wire
            .iter()
            .map(|edge| {
                solid
                    .face_iter()
                    .find(|face| {
                        face.id() != prism.bottom.id()
                            && face.id() != prism.top.id()
                            && face.edge_iter().any(|other| other.id() == edge.id())
                    })
                    .map(|face| {
                        walls.insert(edge.id(), face.id());
                        overrides.get(&face.id()).copied().unwrap_or(thickness)
                    })
                    .ok_or_else(|| invalid("Section wall association is missing"))
            })
            .collect::<Result<Vec<_>>>()?;
        let first = *values
            .first()
            .ok_or_else(|| invalid("Section boundary is empty"))?;
        if values.iter().any(|value| (*value - first).abs() > EPS) {
            return Ok(None);
        }
        distances.push(first);
    }
    let offset = match offset_section(
        doc,
        &history.owner().definition,
        &prism.bottom,
        &distances,
        !outward,
        &walls,
    ) {
        Err(CadError::Unsupported(_)) => return Ok(None),
        other => other?,
    };
    let bottom_open = removed.contains(&prism.bottom.id());
    let top_open = removed.contains(&prism.top.id());
    let lower = if bottom_open {
        0.
    } else if outward {
        -cap_bottom
    } else {
        cap_bottom
    };
    let upper = if top_open {
        prism.height
    } else if outward {
        prism.height + cap_top
    } else {
        prism.height - cap_top
    };
    if !lower.is_finite()
        || !upper.is_finite()
        || !(upper - lower).is_finite()
        || upper - lower <= EPS
    {
        return Err(invalid(
            "Wall thickness collapses the shell cavity or exceeds finite coordinates",
        ));
    }
    let direction = prism.axis * (upper - lower);
    for point in crate::extrusion::face_hull(&offset.face)? {
        if !finite(point + prism.axis * lower) || !finite(point + prism.axis * upper) {
            return Err(invalid("Offset shell exceeds finite document coordinates"));
        }
    }
    let base = offset.face.inverse();
    let section = builder::translated(&base, prism.axis * lower);
    let transformed_origins =
        base.boundaries()
            .iter()
            .flatten()
            .zip(section.boundaries().iter().flatten())
            .map(|(source, edge)| {
                Ok((
                    edge.id(),
                    offset.origins.get(&source.id()).cloned().ok_or_else(|| {
                        invalid("Translated offset edge lost its source constructor")
                    })?,
                ))
            })
            .collect::<Result<HashMap<_, _>>>()?;
    let section = if section.oriented_surface().normal(0., 0.).dot(direction) < 0. {
        section.inverse()
    } else {
        section
    };
    let section_edges = section
        .boundary_iters()
        .into_iter()
        .flatten()
        .map(|edge| edge.id())
        .collect::<Vec<_>>();
    let changed = crate::adapter::oriented_extrude(&section, direction);
    crate::validation::solid(&changed)?;
    let changed_faces = &changed.boundaries()[0];
    if changed_faces.len() != section_edges.len() + 2 {
        return Err(invalid(
            "Offset prism sweep changed its constructor topology",
        ));
    }
    let purpose = if outward {
        "outer_offset_wall"
    } else {
        "inner_offset_wall"
    };
    for (face, source) in [
        (changed_faces[0].id(), prism.bottom.id()),
        (changed_faces[changed_faces.len() - 1].id(), prism.top.id()),
    ] {
        history.construct_face(
            face,
            &[history.face(source)?],
            serde_json::json!({"operation":"shell","constructor":purpose}),
        )?;
    }
    let mut wall_contours = HashMap::new();
    for (face, edge) in changed_faces[1..changed_faces.len() - 1]
        .iter()
        .zip(section_edges)
    {
        let origin = transformed_origins
            .get(&edge)
            .ok_or_else(|| invalid("Swept wall lost its section constructor"))?;
        let mut sources = vec![history.face(origin.wall)?];
        let split = transformed_origins
            .values()
            .filter(|other| other.wall == origin.wall)
            .count()
            > 1;
        let purpose = if split {
            sources.push(history.edge(origin.source)?);
            serde_json::json!({"operation":"shell","constructor":purpose,"segment":crate::adapter::construction_reference(&history.edge(origin.source)?)?,"part":origin.part})
        } else {
            serde_json::json!({"operation":"shell","constructor":purpose})
        };
        history.construct_face(face.id(), &sources, purpose)?;
        wall_contours.insert(face.id(), origin.contour);
    }
    let (outer, inner) = if outward {
        (&changed, solid)
    } else {
        (solid, &changed)
    };
    let outer_bottom = if outward {
        changed_faces[0].clone()
    } else {
        prism.bottom.clone()
    };
    let outer_top = if outward {
        changed_faces[changed_faces.len() - 1].clone()
    } else {
        prism.top.clone()
    };
    let inner_bottom = if outward {
        prism.bottom.clone()
    } else {
        changed_faces[0].clone()
    };
    let inner_top = if outward {
        prism.top.clone()
    } else {
        changed_faces[changed_faces.len() - 1].clone()
    };
    let mut boundary: Shell = outer
        .face_iter()
        .filter(|face| {
            !(bottom_open && face.id() == outer_bottom.id()
                || top_open && face.id() == outer_top.id())
        })
        .cloned()
        .collect();
    boundary.extend(
        inner
            .face_iter()
            .filter(|face| {
                !(bottom_open && face.id() == inner_bottom.id()
                    || top_open && face.id() == inner_top.id())
            })
            .map(Face::inverse),
    );
    for (open, outer, inner, is_top) in [
        (bottom_open, &outer_bottom, &inner_bottom, false),
        (top_open, &outer_top, &inner_top, true),
    ] {
        if !open {
            continue;
        }
        let original = if is_top {
            prism.top.boundaries()
        } else {
            prism.bottom.boundaries()
        };
        let original: Vec<_> = if is_top {
            prism
                .top_wires
                .iter()
                .map(|index| original[*index].clone())
                .collect()
        } else {
            original
        };
        let changed_face = if outward { outer } else { inner };
        let mut new_wires = vec![None; original.len()];
        for wire in changed_face.boundaries() {
            let contours = wire
                .iter()
                .flat_map(|edge| {
                    changed_faces.iter().filter_map(|face| {
                        wall_contours
                            .get(&face.id())
                            .filter(|_| face.edge_iter().any(|other| other.id() == edge.id()))
                            .copied()
                    })
                })
                .collect::<HashSet<_>>();
            if contours.len() != 1 {
                return Err(invalid(
                    "Offset opening has ambiguous constructor contour ownership",
                ));
            }
            let index = *contours.iter().next().unwrap();
            if index >= new_wires.len() || new_wires[index].replace(wire).is_some() {
                return Err(invalid("Offset opening's contour constructor is repeated"));
            }
        }
        let new_wires = new_wires
            .into_iter()
            .map(|wire| {
                wire.ok_or_else(|| invalid("Offset opening's contour constructor is missing"))
            })
            .collect::<Result<Vec<_>>>()?;
        let (outer_wires, inner_wires) = if outward {
            (new_wires, original)
        } else {
            (original, new_wires)
        };
        let opening = if is_top {
            prism.top.id()
        } else {
            prism.bottom.id()
        };
        for (index, ring) in rings(outer, inner, &outer_wires, &inner_wires)?
            .into_iter()
            .enumerate()
        {
            let source_wire = if outward {
                &inner_wires[index]
            } else {
                &outer_wires[index]
            };
            let mut sources = vec![history.face(opening)?];
            sources.extend(
                source_wire
                    .iter()
                    .map(|edge| history.edge(edge.id()))
                    .collect::<Result<Vec<_>>>()?,
            );
            history.rim(
                ring.id(),
                opening,
                &sources,
                serde_json::json!({"operation":"shell","constructor":"opening_ring"}),
            )?;
            boundary.push(ring);
        }
    }
    let components = boundary.connected_components();
    let results = if removed.is_empty() {
        vec![Solid::try_new(components).map_err(invalid)?]
    } else {
        components
            .into_iter()
            .map(|shell| Solid::try_new(vec![shell]).map_err(invalid))
            .collect::<Result<Vec<_>>>()?
    };
    for result in &results {
        crate::validation::solid(result)?;
    }
    Ok(Some(results))
}
