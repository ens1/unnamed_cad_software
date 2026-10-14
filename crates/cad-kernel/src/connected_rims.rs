//! Simultaneous exact treatments of complete tangent line/circle rim contours.
//!
//! A contour is one topology graph: cap rails, wall rails and meridian seams
//! are shared at source vertices. Exact rational product surfaces replace the
//! whole connected rim together; independent sequential rounds never fill a
//! missing junction. The normal-collar certificate covers the complete curve
//! domains and protects every unselected cap contour.
use crate::{
    analytic_offset::{self, Carrier},
    general_chamfer::Treatment,
    planar::ChamferSpec,
    shell_history::ShellHistory,
};
use cad_core::{CadError, Result};
use monstertruck::{
    geometry::prelude::{TryIntoHomogeneousBsplineCurve, TryIntoHomogeneousBsplineSurface},
    modeling::*,
};
use std::collections::{HashMap, HashSet};

const EPS: f64 = 1e-7;
const REL: f64 = 1e-10;
const MAX_EDGES: usize = 512;
const MAX_CONTROLS: usize = 131_072;

fn invalid(message: &str) -> CadError {
    CadError::Invalid(format!("Connected rim: {message}"))
}
fn unsupported(message: &str) -> CadError {
    CadError::Unsupported(format!("Connected rim: {message}"))
}
fn point(h: Vector4) -> Result<Point3> {
    if ![h.x, h.y, h.z, h.w].into_iter().all(f64::is_finite) || h.w <= 0. {
        return Err(unsupported(
            "A rational carrier needs finite positive weights",
        ));
    }
    let result = Point3::from_vec(h.truncate() / h.w);
    if ![result.x, result.y, result.z]
        .into_iter()
        .all(f64::is_finite)
    {
        return Err(invalid("A rational point exceeds finite coordinates"));
    }
    Ok(result)
}
fn same_control(a: Vector4, b: Vector4) -> Result<bool> {
    use crate::native_bounds::Interval;
    // A common exact positive denominator makes the complete rational image
    // difference a convex combination of its control differences. Merely
    // near-equal weights or knots cannot borrow this certificate.
    point(a)?;
    point(b)?;
    if a.w != b.w || a.w <= 0. {
        return Ok(false);
    }
    let weight = Interval::exact(a.w)?;
    let mut square = Interval::exact(0.)?;
    for (a, b) in [(a.x, b.x), (a.y, b.y), (a.z, b.z)] {
        let difference = Interval::exact(a)?.sub(Interval::exact(b)?)?.div(weight)?;
        let bound = Interval::exact(difference.low.abs().max(difference.high.abs()))?;
        square = square.add(bound.mul(bound)?)?;
    }
    Ok(square.sqrt()?.high <= EPS / 8.)
}
pub(crate) fn same_curve(a: &Curve, b: &Curve) -> Result<bool> {
    let Some(mut a) = a.try_into_homogeneous_bspline_curve() else {
        return Ok(false);
    };
    let Some(mut b) = b.try_into_homogeneous_bspline_curve() else {
        return Ok(false);
    };
    a.clamp();
    b.clamp();
    if a.degree() != b.degree()
        || a.control_points().len() != b.control_points().len()
        || a.knot_vector().len() != b.knot_vector().len()
    {
        return Ok(false);
    }
    if a.knot_vector() != b.knot_vector() {
        return Ok(false);
    }
    for (x, y) in a.control_points().iter().zip(b.control_points()) {
        if !same_control(*x, *y)? {
            return Ok(false);
        }
    }
    Ok(true)
}
/// Whole native control-net equality, allowing a parameter transpose. This
/// recognizes the exact product family, not a fitted/sampled lookalike.
pub(crate) fn same_surface(a: &Surface, b: &Surface) -> Result<bool> {
    let Some(a) = a.try_into_homogeneous_bspline_surface() else {
        return Ok(false);
    };
    let Some(mut b) = b.try_into_homogeneous_bspline_surface() else {
        return Ok(false);
    };
    for _ in 0..2 {
        let x = a.control_points();
        let y = b.control_points();
        if a.udegree() == b.udegree()
            && a.vdegree() == b.vdegree()
            && x.len() == y.len()
            && !x.is_empty()
            && x[0].len() == y[0].len()
            && a.knot_vector_u().len() == b.knot_vector_u().len()
            && a.knot_vector_v().len() == b.knot_vector_v().len()
        {
            let mut equal = true;
            for (x, y) in x.iter().flatten().zip(y.iter().flatten()) {
                equal &= same_control(*x, *y)?;
            }
            for (x, y) in [
                (a.knot_vector_u(), b.knot_vector_u()),
                (a.knot_vector_v(), b.knot_vector_v()),
            ] {
                equal &= x == y;
            }
            if equal {
                return Ok(true);
            }
        }
        b.invert();
    }
    Ok(false)
}
pub(crate) fn product_surface(
    source: &Curve,
    cap: &Curve,
    wall: &Curve,
    treatment: Treatment,
) -> Result<Surface> {
    let Some(native) = source.try_into_homogeneous_bspline_curve() else {
        return Err(unsupported(
            "A connected strip needs an exact rational source",
        ));
    };
    let Some(cap) = cap.try_into_homogeneous_bspline_curve() else {
        return Err(unsupported(
            "A connected strip needs an exact rational cap rail",
        ));
    };
    let Some(wall) = wall.try_into_homogeneous_bspline_curve() else {
        return Err(unsupported(
            "A connected strip needs an exact rational wall rail",
        ));
    };
    if native.control_points().len() != cap.control_points().len()
        || native.control_points().len() != wall.control_points().len()
        || native.knot_vector() != cap.knot_vector()
        || native.knot_vector() != wall.knot_vector()
    {
        return Err(unsupported(
            "A connected strip lost its exact common rational parameterization",
        ));
    }
    for ((source, cap), wall) in native
        .control_points()
        .iter()
        .zip(cap.control_points())
        .zip(wall.control_points())
    {
        point(*source)?;
        point(*cap)?;
        point(*wall)?;
        if source.w != cap.w || source.w != wall.w {
            return Err(unsupported(
                "A connected strip needs one exact common positive denominator",
            ));
        }
    }
    let weight = std::f64::consts::FRAC_1_SQRT_2;
    let rows = native
        .control_points()
        .iter()
        .zip(cap.control_points())
        .zip(wall.control_points())
        .map(|((original, a), b)| {
            if matches!(treatment, Treatment::Fillet) {
                vec![*a, *original * weight, *b]
            } else {
                vec![*a, *b]
            }
        })
        .collect();
    Ok(Surface::NurbsSurface(NurbsSurface::new(
        BsplineSurface::try_new(
            (
                native.knot_vector().clone(),
                KnotVector::bezier_knot(if matches!(treatment, Treatment::Fillet) {
                    2
                } else {
                    1
                }),
            ),
            rows,
        )
        .map_err(|_| invalid("A connected strip has invalid native product controls"))?,
    )))
}
fn along(edge: &Edge, first: &Vertex) -> Result<Edge> {
    if edge.front().id() == first.id() {
        Ok(edge.clone())
    } else if edge.back().id() == first.id() {
        Ok(edge.inverse())
    } else {
        Err(invalid("A native endpoint lost its adjoining edge"))
    }
}
fn occurrence(edge: &Edge, source: &Edge, replacement: &Edge) -> Edge {
    if edge.orientation() == source.orientation() {
        replacement.clone()
    } else {
        replacement.inverse()
    }
}
struct Wall {
    face: Face,
    carrier: Carrier,
    side: f64,
    generators: [Edge; 2],
}
struct Stock {
    cap: Face,
    opposite: FaceId,
    normal: Vector3,
    height: f64,
    walls: HashMap<EdgeId, Wall>,
}
/// Recognize the complete finite extrusion, including all unselected contours.
/// Every side is a complete native four-edge translation patch. No nearby face
/// can hide inside a profile-only proof or be silently discarded.
fn stock(solid: &Solid, cap: &Face) -> Result<Option<Stock>> {
    if solid.boundaries().len() != 1 || cap.edge_iter().count() > MAX_EDGES {
        return Ok(None);
    }
    let Some(plane) = crate::planar::boundary_plane(cap)? else {
        return Ok(None);
    };
    let n = plane.normal;
    let cap_edges = cap
        .edge_iter()
        .map(|e| (e.id(), e))
        .collect::<HashMap<_, _>>();
    let mut walls = HashMap::new();
    let mut height: Option<f64> = None;
    let mut opposite: Option<FaceId> = None;
    for source in cap_edges.values() {
        let adjoining = solid
            .face_iter()
            .filter(|f| f.id() != cap.id() && f.edge_iter().any(|e| e.id() == source.id()))
            .collect::<Vec<_>>();
        let [face] = adjoining.as_slice() else {
            return Ok(None);
        };
        let boundaries = face.boundaries();
        if boundaries.len() != 1 || boundaries[0].len() != 4 {
            return Ok(None);
        }
        let Some(carrier) = analytic_offset::recognize(face)? else {
            return Ok(None);
        };
        match carrier {
            Carrier::Plane { normal, .. } if normal.dot(n).abs() <= REL => {
                crate::bounded_carriers::line(&source.oriented_curve())?;
            }
            Carrier::Cone {
                axis, slope: 0., ..
            } if axis.dot(n).abs() > 1. - REL => {
                crate::bounded_carriers::circle(&source.oriented_curve())?;
            }
            _ => return Ok(None),
        }
        let mut generators = Vec::new();
        let mut other_ends = Vec::new();
        for vertex in [source.front(), source.back()] {
            let candidates = face
                .edge_iter()
                .filter(|e| {
                    e.id() != source.id()
                        && (e.front().id() == vertex.id() || e.back().id() == vertex.id())
                })
                .collect::<Vec<_>>();
            let [edge] = candidates.as_slice() else {
                return Ok(None);
            };
            let edge = along(edge, vertex)?;
            if crate::bounded_carriers::line(&edge.oriented_curve()).is_err() {
                return Ok(None);
            }
            let delta = edge.back().point() - edge.front().point();
            let h = -delta.dot(n);
            if h <= EPS || (delta + n * h).magnitude() > EPS / 8. {
                return Ok(None);
            }
            if let Some(previous) = height {
                if (previous - h).abs() > EPS / 8. {
                    return Ok(None);
                }
            } else {
                height = Some(h);
            }
            other_ends.push(edge.back().clone());
            generators.push(edge);
        }
        let remaining = face
            .edge_iter()
            .filter(|e| e.id() != source.id() && !generators.iter().any(|g| g.id() == e.id()))
            .collect::<Vec<_>>();
        let [other] = remaining.as_slice() else {
            return Ok(None);
        };
        let other = along(other, &other_ends[0])?;
        if other.back().id() != other_ends[1].id() {
            return Ok(None);
        }
        let h = height.ok_or_else(|| invalid("An extrusion has no axial interval"))?;
        if !same_curve(
            &source
                .oriented_curve()
                .transformed(Matrix4::from_translation(-n * h)),
            &other.oriented_curve(),
        )? {
            return Ok(None);
        }
        let caps = solid
            .face_iter()
            .filter(|f| f.id() != face.id() && f.edge_iter().any(|e| e.id() == other.id()))
            .collect::<Vec<_>>();
        let [other_cap] = caps.as_slice() else {
            return Ok(None);
        };
        let Some(other_plane) = crate::planar::boundary_plane(other_cap)? else {
            return Ok(None);
        };
        if other_plane.normal.dot(n) > -1. + REL {
            return Ok(None);
        }
        if let Some(previous) = opposite {
            if previous != other_cap.id() {
                return Ok(None);
            }
        } else {
            opposite = Some(other_cap.id());
        }
        walls.insert(
            source.id(),
            Wall {
                face: (*face).clone(),
                carrier,
                side: analytic_offset::side(face, carrier)?,
                generators: [generators.remove(0), generators.remove(0)],
            },
        );
    }
    let Some(opposite) = opposite else {
        return Ok(None);
    };
    let wall_ids = walls.values().map(|w| w.face.id()).collect::<HashSet<_>>();
    if wall_ids.len() != walls.len()
        || solid
            .face_iter()
            .any(|f| f.id() != cap.id() && f.id() != opposite && !wall_ids.contains(&f.id()))
    {
        return Ok(None);
    }
    Ok(Some(Stock {
        cap: cap.clone(),
        opposite,
        normal: n,
        height: height.unwrap_or(0.),
        walls,
    }))
}
fn distances(
    spec: &ChamferSpec,
    treatment: Treatment,
    cap: &Face,
    walls: &HashMap<EdgeId, Wall>,
    selected: &HashSet<EdgeId>,
) -> Result<[f64; 2]> {
    if matches!(treatment, Treatment::Fillet) {
        return Ok([spec.distance, spec.distance]);
    }
    let first_cap = match spec.first_face {
        None => true,
        Some(id) if id == cap.id() => true,
        Some(id) if selected.iter().all(|e| walls[e].face.id() == id) => false,
        Some(_) => return Err(invalid("Distance 1 face must adjoin every selected rim")),
    } != spec.flip;
    let second = if let Some(angle) = spec.angle {
        if !angle.is_finite() || angle <= 0. || angle >= std::f64::consts::FRAC_PI_2 {
            return Err(invalid("Chamfer angle must be between zero and 90 degrees"));
        }
        spec.distance * angle.tan()
    } else {
        spec.second_distance.unwrap_or(spec.distance)
    };
    Ok(if first_cap {
        [spec.distance, second]
    } else {
        [second, spec.distance]
    })
}
struct Joint {
    cap: Vertex,
    wall: Vertex,
    meridian: Edge,
    axial: Edge,
}
struct Rim {
    source: Edge,
    cap: Edge,
    wall: Edge,
    blend: Face,
    collar: Face,
}
pub(crate) fn construct(
    solid: &Solid,
    selected: &[EdgeId],
    spec: &ChamferSpec,
    treatment: Treatment,
    history: &mut ShellHistory,
) -> Result<Option<Solid>> {
    if selected.is_empty() {
        return Ok(None);
    }
    let selected = selected.iter().copied().collect::<HashSet<_>>();
    let caps = solid
        .face_iter()
        .filter(|f| {
            selected
                .iter()
                .all(|id| f.edge_iter().any(|e| e.id() == *id))
        })
        .collect::<Vec<_>>();
    let mut recognized = None;
    for cap in caps {
        if let Some(value) = stock(solid, cap)? {
            recognized = Some(value);
            break;
        }
    }
    let Some(stock) = recognized else {
        return Ok(None);
    };
    let [radial, depth] = distances(spec, treatment, &stock.cap, &stock.walls, &selected)?;
    if [radial, depth].iter().any(|v| !v.is_finite() || *v <= EPS) {
        return Err(invalid(
            "Setbacks must be finite and exceed modeling tolerance",
        ));
    }
    if depth >= stock.height - EPS {
        return Err(invalid(
            "The rim consumes the complete axial wall; reduce its size",
        ));
    }
    let walls = stock
        .walls
        .iter()
        .map(|(edge, wall)| (*edge, wall.face.clone()))
        .collect();
    if !crate::parallel_offset::certify_cap_collars(&stock.cap, &walls, &selected, -radial, 0.)? {
        return Ok(None);
    }
    let mut joints: HashMap<VertexId, Joint> = HashMap::new();
    let mut rims = HashMap::new();
    let mut controls = 0usize;
    for wire in stock.cap.boundaries() {
        if !wire.iter().all(|e| selected.contains(&e.id())) {
            continue;
        }
        for source in &wire {
            let wall = &stock.walls[&source.id()];
            let source_curve = source.oriented_curve();
            let Some(native) = source_curve.try_into_homogeneous_bspline_curve() else {
                return Ok(None);
            };
            controls = controls.saturating_add(native.control_points().len() * 3);
            if controls > MAX_CONTROLS {
                return Err(unsupported(
                    "Connected strips exceed the rational control budget",
                ));
            }
            let cap_curve = wall
                .carrier
                .offset_curve(&source_curve, -radial * wall.side)?;
            let wall_curve =
                source_curve.transformed(Matrix4::from_translation(-stock.normal * depth));
            let weight = if matches!(treatment, Treatment::Fillet) {
                std::f64::consts::FRAC_1_SQRT_2
            } else {
                1.
            };
            for (index, vertex) in [source.front(), source.back()].into_iter().enumerate() {
                let a = wall
                    .carrier
                    .offset_point(vertex.point(), -radial * wall.side)?;
                let b = vertex.point() - stock.normal * depth;
                if let Some(previous) = joints.get(&vertex.id()) {
                    if previous.cap.point().distance(a) > EPS / 8.
                        || previous.wall.point().distance(b) > EPS / 8.
                    {
                        return Err(unsupported(
                            "The tangent junction has no single native setback vertex",
                        ));
                    }
                } else {
                    let a = Vertex::new(a);
                    let b = Vertex::new(b);
                    let profile = if matches!(treatment, Treatment::Fillet) {
                        vec![
                            a.point().to_homogeneous(),
                            vertex.point().to_homogeneous() * weight,
                            b.point().to_homogeneous(),
                        ]
                    } else {
                        vec![a.point().to_homogeneous(), b.point().to_homogeneous()]
                    };
                    let meridian = Edge::try_new(
                        &a,
                        &b,
                        Curve::NurbsCurve(NurbsCurve::new(
                            BsplineCurve::try_new(
                                KnotVector::bezier_knot(profile.len() - 1),
                                profile,
                            )
                            .map_err(|_| {
                                invalid("A junction profile has invalid rational controls")
                            })?,
                        )),
                    )
                    .map_err(|_| invalid("A junction profile has inconsistent endpoints"))?;
                    let axial = builder::line(vertex, &b);
                    let mut incident = wire
                        .iter()
                        .filter(|edge| {
                            edge.front().id() == vertex.id() || edge.back().id() == vertex.id()
                        })
                        .map(|edge| {
                            Ok((
                                crate::shell_history::closure(&[history.edge(edge.id())?])?
                                    .to_string(),
                                edge,
                            ))
                        })
                        .collect::<Result<Vec<_>>>()?;
                    incident.sort_by(|a, b| a.0.cmp(&b.0));
                    let mut sources = vec![history.face(stock.cap.id())?];
                    let mut endpoints = Vec::new();
                    for (_, edge) in incident {
                        let reference = history.edge(edge.id())?;
                        endpoints.push(serde_json::json!({"edge":reference,"endpoint":if edge.absolute_front().id()==vertex.id(){"start"}else{"end"}}));
                        sources.push(reference);
                        sources.push(history.face(stock.walls[&edge.id()].face.id())?);
                    }
                    history.construct_edge(meridian.id(),&sources,serde_json::json!({"operation":treatment.name(),"constructor":"connected_meridian","endpoints":endpoints}))?;
                    history.construct_edge(axial.id(),&[history.edge(wall.generators[index].id())?],serde_json::json!({"operation":treatment.name(),"constructor":"connected_axial_collar"}))?;
                    joints.insert(
                        vertex.id(),
                        Joint {
                            cap: a,
                            wall: b,
                            meridian,
                            axial,
                        },
                    );
                }
            }
            let start = &joints[&source.front().id()];
            let end = &joints[&source.back().id()];
            let cap = Edge::try_new(&start.cap, &end.cap, cap_curve.clone())
                .map_err(|_| invalid("A connected cap rail has inconsistent endpoints"))?;
            let side = Edge::try_new(&start.wall, &end.wall, wall_curve.clone())
                .map_err(|_| invalid("A connected wall rail has inconsistent endpoints"))?;
            history.construct_edge(cap.id(),&[history.edge(source.id())?,history.face(stock.cap.id())?],serde_json::json!({"operation":treatment.name(),"constructor":"connected_cap_rail"}))?;
            history.construct_edge(side.id(),&[history.edge(source.id())?,history.face(wall.face.id())?],serde_json::json!({"operation":treatment.name(),"constructor":"connected_wall_rail"}))?;
            let mut surface = product_surface(&source_curve, &cap_curve, &wall_curve, treatment)?;
            let (a, b) = source_curve.range_tuple();
            let middle = source_curve.subs((a + b) * 0.5);
            let outward = wall.carrier.normal(middle)? * wall.side;
            if surface
                .normal((a + b) * 0.5, 0.5)
                .dot(stock.normal + outward)
                < 0.
            {
                surface.invert();
            }
            let boundary = Wire::from(vec![
                cap.inverse(),
                start.meridian.clone(),
                side.clone(),
                end.meridian.inverse(),
            ]);
            let blend = Face::try_new(vec![boundary], surface)
                .map_err(|_| invalid("A connected blend strip has inconsistent boundaries"))?;
            history.construct_face(blend.id(),&[history.edge(source.id())?,history.face(stock.cap.id())?,history.face(wall.face.id())?],serde_json::json!({"operation":treatment.name(),"constructor":"connected_exact_rim_strip"}))?;
            let collar = Face::try_new(
                vec![Wire::from(vec![
                    source.inverse(),
                    start.axial.clone(),
                    side.clone(),
                    end.axial.inverse(),
                ])],
                wall.face.oriented_surface(),
            )
            .map_err(|_| invalid("A removed wall collar has inconsistent boundaries"))?;
            rims.insert(
                source.id(),
                Rim {
                    source: source.clone(),
                    cap,
                    wall: side,
                    blend,
                    collar,
                },
            );
        }
    }
    if rims.len() != selected.len() {
        return Ok(None);
    }
    let mut trimmed = HashMap::new();
    for (edge, wall) in &stock.walls {
        if !selected.contains(edge) {
            continue;
        }
        for generator in &wall.generators {
            if trimmed.contains_key(&generator.id()) {
                continue;
            }
            let joint = &joints[&generator.front().id()];
            let replacement = builder::line(&joint.wall, generator.back());
            history.retain_edge(replacement.id(), generator.id())?;
            trimmed.insert(generator.id(), (generator.clone(), replacement));
        }
    }
    let cap_wires = stock
        .cap
        .boundaries()
        .into_iter()
        .map(|wire| {
            wire.iter()
                .map(|e| {
                    rims.get(&e.id())
                        .map_or_else(|| e.clone(), |r| r.cap.clone())
                })
                .collect()
        })
        .collect();
    let cap = Face::try_new(cap_wires, stock.cap.oriented_surface())
        .map_err(|_| invalid("The connected cap rails do not form a valid face"))?;
    history.retain_face(cap.id(), stock.cap.id())?;
    let mut output = Shell::new();
    for face in solid.face_iter() {
        if face.id() == stock.cap.id() {
            output.push(cap.clone());
            continue;
        }
        if face.id() == stock.opposite {
            output.push(face.clone());
            continue;
        }
        let affected = face
            .edge_iter()
            .any(|e| rims.contains_key(&e.id()) || trimmed.contains_key(&e.id()));
        if !affected {
            output.push(face.clone());
            continue;
        }
        let wires = face
            .boundaries()
            .into_iter()
            .map(|wire| {
                wire.iter()
                    .map(|edge| {
                        if let Some(rim) = rims.get(&edge.id()) {
                            occurrence(edge, &rim.source, &rim.wall)
                        } else if let Some((source, replacement)) = trimmed.get(&edge.id()) {
                            occurrence(edge, source, replacement)
                        } else {
                            edge.clone()
                        }
                    })
                    .collect()
            })
            .collect();
        let mapped = Face::try_new(wires, face.oriented_surface())
            .map_err(|_| invalid("A trimmed stock wall has inconsistent boundaries"))?;
        history.retain_face(mapped.id(), face.id())?;
        output.push(mapped);
    }
    // Stable traversal follows source cap contours, never HashMap addresses.
    for wire in stock.cap.boundaries() {
        for edge in wire {
            if let Some(rim) = rims.get(&edge.id()) {
                output.push(rim.blend.clone());
            }
        }
    }
    let output = Solid::try_new(vec![output])
        .map_err(|_| invalid("Connected rim strips do not close the native body"))?;
    crate::validation::solid(&output)?;
    let material = (|| -> Result<Option<crate::constructor_delta::Native>> {
        let mut removed = Vec::new();
        for (index, wire) in stock.cap.boundaries().into_iter().enumerate() {
            if !wire.iter().all(|e| rims.contains_key(&e.id())) {
                continue;
            }
            let rail: Wire = wire.iter().map(|e| rims[&e.id()].cap.clone()).collect();
            let annulus = if index == 0 {
                vec![wire.clone(), rail.inverse()]
            } else {
                vec![rail.inverse(), wire.clone()]
            };
            let mut region = Shell::from(vec![
                Face::try_new(annulus, stock.cap.oriented_surface())
                    .map_err(|_| invalid("The native removal collar has invalid cap boundaries"))?,
            ]);
            for edge in &wire {
                region.push(rims[&edge.id()].collar.clone());
                region.push(rims[&edge.id()].blend.inverse());
            }
            let region = Solid::try_new(vec![region])
                .map_err(|_| invalid("The connected native removal collar is not closed"))?;
            crate::validation::solid(&region)?;
            removed.push(region);
        }
        Ok(Some(crate::constructor_delta::Native {
            added: Vec::new(),
            removed,
        }))
    })();
    history.capture_material(material);
    Ok(Some(output))
}
