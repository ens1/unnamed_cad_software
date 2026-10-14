//! Uniform shells of finite tangent-profile stock with exact rounded cap rims.
//!
//! Recognition compares the complete rational rim product, its two meridian
//! circles and every translated wall/base trim. A visually similar surface
//! cannot borrow the collar/focal/axial certificate. Native offset carriers
//! and reconstructed sharp base intersections share one endpoint/edge graph.
use crate::{
    analytic_offset::{self, Carrier},
    connected_rims,
    general_chamfer::Treatment,
    shell_history::ShellHistory,
};
use cad_core::{CadError, Result};
use monstertruck::modeling::*;
use std::collections::{HashMap, HashSet};
const EPS: f64 = 1e-7;
const REL: f64 = 1e-10;
const MAX_EDGES: usize = 512;
fn invalid(message: &str) -> CadError {
    CadError::Invalid(format!("Rounded rim shell: {message}"))
}
fn unsupported(message: &str) -> CadError {
    CadError::Unsupported(format!("Rounded rim shell: {message}"))
}
fn along(edge: &Edge, first: &Vertex) -> Option<Edge> {
    if edge.front().id() == first.id() {
        Some(edge.clone())
    } else if edge.back().id() == first.id() {
        Some(edge.inverse())
    } else {
        None
    }
}
struct Round {
    face: Face,
    carrier: Carrier,
    side: f64,
    radius: f64,
    meridians: [Edge; 2],
}
struct Segment {
    cap: Edge,
    top: Edge,
    base: Edge,
    generators: [Edge; 2],
    wall: Face,
    carrier: Carrier,
    side: f64,
    profile: Edge,
    round: Option<Round>,
}
struct Graph {
    cap: Face,
    base: Face,
    normal: Vector3,
    height: f64,
    profile: Face,
    segments: Vec<Segment>,
}
fn vertical(carrier: Carrier, n: Vector3) -> bool {
    matches!(carrier,Carrier::Plane {normal,..} if normal.dot(n).abs()<=REL)
        || matches!(carrier,Carrier::Cone {axis,slope:0.,..} if axis.dot(n).abs()>1.-REL)
}
fn neighbour(solid: &Solid, face: FaceId, edge: EdgeId) -> Option<&Face> {
    let faces = solid
        .face_iter()
        .filter(|f| f.id() != face && f.edge_iter().any(|e| e.id() == edge))
        .collect::<Vec<_>>();
    match faces.as_slice() {
        [face] => Some(*face),
        _ => None,
    }
}
fn recognize_from(solid: &Solid, cap: &Face) -> Result<Option<Graph>> {
    let Some(plane) = crate::planar::boundary_plane(cap)? else {
        return Ok(None);
    };
    let n = plane.normal;
    if cap.edge_iter().count() > MAX_EDGES {
        return Err(unsupported("Rounded stock exceeds the profile work budget"));
    }
    let mut segments = Vec::new();
    let mut vertices: HashMap<VertexId, Vertex> = HashMap::new();
    let mut profile_wires = Vec::new();
    let mut base_id = None;
    let mut height: Option<f64> = None;
    let mut any_round = false;
    for wire in cap.boundaries() {
        let mut profile_wire = Wire::new();
        let mut contour_radius: Option<f64> = None;
        let mut kind = None;
        for edge in &wire {
            let Some(adjacent) = neighbour(solid, cap.id(), edge.id()) else {
                return Ok(None);
            };
            let Some(adjacent_carrier) = analytic_offset::recognize(adjacent)? else {
                return Ok(None);
            };
            let (top, wall, round) = if vertical(adjacent_carrier, n) {
                (edge.clone(), adjacent.clone(), None)
            } else {
                let boundaries = adjacent.boundaries();
                if boundaries.len() != 1 || boundaries[0].len() != 4 {
                    return Ok(None);
                }
                let mut meridians = Vec::new();
                let mut radius: Option<f64> = None;
                let mut endpoints = Vec::new();
                for vertex in [edge.front(), edge.back()] {
                    let incident = adjacent
                        .edge_iter()
                        .filter(|e| {
                            e.id() != edge.id()
                                && (e.front().id() == vertex.id() || e.back().id() == vertex.id())
                        })
                        .collect::<Vec<_>>();
                    let [meridian] = incident.as_slice() else {
                        return Ok(None);
                    };
                    let Some(meridian) = along(meridian, vertex) else {
                        return Ok(None);
                    };
                    let Ok(((center, _, r), sweep)) =
                        crate::bounded_carriers::circle(&meridian.oriented_curve())
                    else {
                        return Ok(None);
                    };
                    if (vertex.point() - center - n * r).magnitude() > EPS / 8.
                        || (sweep.abs() - std::f64::consts::FRAC_PI_2).abs() * r > EPS / 8.
                        || (meridian.back().point() - center).dot(n).abs() > EPS / 8.
                    {
                        return Ok(None);
                    }
                    if let Some(previous) = radius {
                        if (previous - r).abs() > EPS / 8. {
                            return Ok(None);
                        }
                    } else {
                        radius = Some(r);
                    }
                    endpoints.push(meridian.back().clone());
                    meridians.push(meridian);
                }
                let others = adjacent
                    .edge_iter()
                    .filter(|e| e.id() != edge.id() && !meridians.iter().any(|m| m.id() == e.id()))
                    .collect::<Vec<_>>();
                let [top] = others.as_slice() else {
                    return Ok(None);
                };
                let Some(top) = along(top, &endpoints[0]) else {
                    return Ok(None);
                };
                if top.back().id() != endpoints[1].id() {
                    return Ok(None);
                }
                let Some(wall) = neighbour(solid, adjacent.id(), top.id()) else {
                    return Ok(None);
                };
                let Some(carrier) = analytic_offset::recognize(wall)? else {
                    return Ok(None);
                };
                if !vertical(carrier, n) {
                    return Ok(None);
                }
                let r =
                    radius.ok_or_else(|| invalid("A rounded contour has no meridian radius"))?;
                let source = top
                    .oriented_curve()
                    .transformed(Matrix4::from_translation(n * r));
                let expected =
                    carrier.offset_curve(&source, -r * analytic_offset::side(wall, carrier)?)?;
                if !connected_rims::same_curve(&expected, &edge.oriented_curve())? {
                    return Ok(None);
                }
                let product = connected_rims::product_surface(
                    &source,
                    &edge.oriented_curve(),
                    &top.oriented_curve(),
                    Treatment::Fillet,
                )?;
                if !connected_rims::same_surface(&product, &adjacent.surface())? {
                    return Ok(None);
                }
                let round = Round {
                    face: adjacent.clone(),
                    carrier: adjacent_carrier,
                    side: analytic_offset::side(adjacent, adjacent_carrier)?,
                    radius: r,
                    meridians: [meridians.remove(0), meridians.remove(0)],
                };
                any_round = true;
                (top, wall.clone(), Some(round))
            };
            let is_round = round.is_some();
            if let Some(previous) = kind {
                if previous != is_round {
                    return Ok(None);
                }
            } else {
                kind = Some(is_round);
            }
            let radius = round.as_ref().map_or(0., |r| r.radius);
            if let Some(previous) = contour_radius {
                if (previous - radius).abs() > EPS / 8. {
                    return Ok(None);
                }
            } else {
                contour_radius = Some(radius);
            }
            let Some(carrier) = analytic_offset::recognize(&wall)? else {
                return Ok(None);
            };
            let side = analytic_offset::side(&wall, carrier)?;
            let boundaries = wall.boundaries();
            if boundaries.len() != 1 || boundaries[0].len() != 4 {
                return Ok(None);
            }
            let mut generators = Vec::new();
            let mut ends = Vec::new();
            for vertex in [top.front(), top.back()] {
                let incident = wall
                    .edge_iter()
                    .filter(|e| {
                        e.id() != top.id()
                            && (e.front().id() == vertex.id() || e.back().id() == vertex.id())
                    })
                    .collect::<Vec<_>>();
                let [generator] = incident.as_slice() else {
                    return Ok(None);
                };
                let Some(generator) = along(generator, vertex) else {
                    return Ok(None);
                };
                if crate::bounded_carriers::line(&generator.oriented_curve()).is_err() {
                    return Ok(None);
                }
                let delta = generator.back().point() - generator.front().point();
                let h = -delta.dot(n) + radius;
                if h - radius <= EPS || (delta + n * (h - radius)).magnitude() > EPS / 8. {
                    return Ok(None);
                }
                if let Some(previous) = height {
                    if (previous - h).abs() > EPS / 8. {
                        return Ok(None);
                    }
                } else {
                    height = Some(h);
                }
                ends.push(generator.back().clone());
                generators.push(generator);
            }
            let others = wall
                .edge_iter()
                .filter(|e| e.id() != top.id() && !generators.iter().any(|g| g.id() == e.id()))
                .collect::<Vec<_>>();
            let [base] = others.as_slice() else {
                return Ok(None);
            };
            let Some(base) = along(base, &ends[0]) else {
                return Ok(None);
            };
            if base.back().id() != ends[1].id() {
                return Ok(None);
            }
            let Some(base_face) = neighbour(solid, wall.id(), base.id()) else {
                return Ok(None);
            };
            let Some(base_plane) = crate::planar::boundary_plane(base_face)? else {
                return Ok(None);
            };
            if base_plane.normal.dot(n) > -1. + REL {
                return Ok(None);
            }
            if let Some(previous) = base_id {
                if previous != base_face.id() {
                    return Ok(None);
                }
            } else {
                base_id = Some(base_face.id());
            }
            let source = top
                .oriented_curve()
                .transformed(Matrix4::from_translation(n * radius));
            let h = height.ok_or_else(|| invalid("Rounded stock has no finite height"))?;
            if !connected_rims::same_curve(
                &source.transformed(Matrix4::from_translation(-n * h)),
                &base.oriented_curve(),
            )? {
                return Ok(None);
            }
            for (vertex, p) in [edge.front(), edge.back()].into_iter().zip([
                top.front().point() + n * radius,
                top.back().point() + n * radius,
            ]) {
                if let Some(previous) = vertices.get(&vertex.id()) {
                    if previous.point().distance(p) > EPS / 8. {
                        return Ok(None);
                    }
                } else {
                    vertices.insert(vertex.id(), Vertex::new(p));
                }
            }
            let profile = Edge::try_new(
                &vertices[&edge.front().id()],
                &vertices[&edge.back().id()],
                source,
            )
            .map_err(|_| invalid("Recovered native stock profile has inconsistent endpoints"))?;
            profile_wire.push_back(profile.clone());
            segments.push(Segment {
                cap: edge.clone(),
                top,
                base,
                generators: [generators.remove(0), generators.remove(0)],
                wall,
                carrier,
                side,
                profile,
                round,
            });
        }
        profile_wires.push(profile_wire);
    }
    if !any_round {
        return Ok(None);
    }
    let Some(base_id) = base_id else {
        return Ok(None);
    };
    let Some(base) = solid.face_iter().find(|f| f.id() == base_id).cloned() else {
        return Ok(None);
    };
    let mut known = HashSet::from([cap.id(), base_id]);
    for segment in &segments {
        if !known.insert(segment.wall.id()) {
            return Ok(None);
        }
        if let Some(round) = &segment.round
            && !known.insert(round.face.id())
        {
            return Ok(None);
        }
    }
    if solid.face_iter().any(|face| !known.contains(&face.id())) {
        return Ok(None);
    }
    let profile = Face::try_new(profile_wires, cap.oriented_surface())
        .map_err(|_| invalid("Recovered finite stock profile has inconsistent native contours"))?;
    Ok(Some(Graph {
        cap: cap.clone(),
        base,
        normal: n,
        height: height.unwrap_or(0.),
        profile,
        segments,
    }))
}
fn recognize(solid: &Solid) -> Result<Option<Graph>> {
    if solid.boundaries().len() != 1 || solid.face_iter().count() > MAX_EDGES * 2 + 2 {
        return Ok(None);
    }
    for face in solid.face_iter() {
        if crate::planar::boundary_plane(face)?.is_some() {
            match recognize_from(solid, face) {
                Ok(Some(graph)) => return Ok(Some(graph)),
                Ok(None) | Err(CadError::Unsupported(_)) => {}
                Err(error) => return Err(error),
            }
        }
    }
    Ok(None)
}
fn vertex(vertices: &mut HashMap<VertexId, Vertex>, source: &Vertex, p: Point3) -> Result<Vertex> {
    if let Some(previous) = vertices.get(&source.id()) {
        if previous.point().distance(p) > EPS / 8. {
            return Err(unsupported(
                "Adjacent offset carriers disagree at their shared native vertex",
            ));
        }
        Ok(previous.clone())
    } else {
        let next = Vertex::new(p);
        vertices.insert(source.id(), next.clone());
        Ok(next)
    }
}
fn edge(
    edges: &mut HashMap<EdgeId, Edge>,
    source: &Edge,
    a: &Vertex,
    b: &Vertex,
    curve: Curve,
) -> Result<Edge> {
    if let Some(previous) = edges.get(&source.id()) {
        let previous = if source.orientation() {
            previous.clone()
        } else {
            previous.inverse()
        };
        if previous.front().id() != a.id()
            || previous.back().id() != b.id()
            || !connected_rims::same_curve(&previous.oriented_curve(), &curve)?
        {
            return Err(unsupported(
                "Adjacent offsets disagree on their complete shared native boundary",
            ));
        }
        Ok(previous)
    } else {
        let next = Edge::try_new(a, b, curve)
            .map_err(|_| invalid("A reconstructed offset edge has inconsistent endpoints"))?;
        edges.insert(
            source.id(),
            if source.orientation() {
                next.clone()
            } else {
                next.inverse()
            },
        );
        Ok(next)
    }
}
fn mapped_wire(wire: &Wire, edges: &HashMap<EdgeId, Edge>) -> Wire {
    wire.iter()
        .map(|e| {
            if e.orientation() {
                edges[&e.id()].clone()
            } else {
                edges[&e.id()].inverse()
            }
        })
        .collect()
}
pub(crate) fn shell(
    solid: &Solid,
    thickness: f64,
    removed: &[FaceId],
    outward: bool,
    overrides: &HashMap<FaceId, f64>,
    history: &mut ShellHistory,
) -> Result<Option<Vec<Solid>>> {
    let Some(graph) = recognize(solid)? else {
        return Ok(None);
    };
    if !thickness.is_finite() || thickness <= EPS {
        return Err(invalid(
            "Thickness must be finite and exceed modeling tolerance",
        ));
    }
    if removed
        .iter()
        .any(|id| *id != graph.cap.id() && *id != graph.base.id())
    {
        return Err(unsupported(
            "Rounded rim stock currently permits planar cap openings; lateral openings require additional corner intersections",
        ));
    }
    if overrides.iter().any(|(face, t)| {
        !removed.contains(face) && (!t.is_finite() || (*t - thickness).abs() > EPS / 8.)
    }) {
        return Err(unsupported(
            "The connected normal field requires uniform thickness; unequal walls need transition reconstruction",
        ));
    }
    let d = if outward { thickness } else { -thickness };
    let cap_open = removed.contains(&graph.cap.id());
    let base_open = removed.contains(&graph.base.id());
    let top_shift = if cap_open { 0. } else { d };
    let base_shift = if base_open { 0. } else { -d };
    if graph.height + top_shift - base_shift <= EPS {
        return Err(invalid(
            "Thickness makes the offset end planes meet or reverse",
        ));
    }
    let mut radius: f64 = 0.;
    for segment in &graph.segments {
        if let Some(round) = &segment.round {
            radius = radius.max(round.radius);
            if round.radius + d <= EPS {
                return Err(invalid(
                    "Thickness reaches a rounded rim's focal cylinder or torus",
                ));
            }
            if graph.height - round.radius - base_shift <= EPS {
                return Err(invalid(
                    "Thickness makes the rounded rim meet the opposite end plane",
                ));
            }
        }
    }
    let selected = graph
        .profile
        .edge_iter()
        .map(|e| e.id())
        .collect::<HashSet<_>>();
    let walls = graph
        .segments
        .iter()
        .map(|s| (s.profile.id(), s.wall.clone()))
        .collect();
    if !crate::parallel_offset::certify_cap_collars(
        &graph.profile,
        &walls,
        &selected,
        (-radius).min(d),
        0_f64.max(d),
    )? {
        return Ok(None);
    }
    let mut vertices = HashMap::new();
    let mut edges = HashMap::new();
    let mut faces = HashMap::new();
    for segment in &graph.segments {
        let round = segment.round.as_ref();
        let cap_curve = if round.is_some() {
            segment
                .cap
                .oriented_curve()
                .transformed(Matrix4::from_translation(graph.normal * d))
        } else {
            segment
                .carrier
                .offset_curve(&segment.cap.oriented_curve(), d * segment.side)?
                .transformed(Matrix4::from_translation(graph.normal * top_shift))
        };
        let top_curve = if round.is_some() {
            segment
                .carrier
                .offset_curve(&segment.top.oriented_curve(), d * segment.side)?
        } else {
            cap_curve.clone()
        };
        let base_curve = segment
            .carrier
            .offset_curve(&segment.base.oriented_curve(), d * segment.side)?
            .transformed(Matrix4::from_translation(graph.normal * base_shift));
        let (a, b) = cap_curve.range_tuple();
        let cap_start = vertex(&mut vertices, segment.cap.front(), cap_curve.subs(a))?;
        let cap_end = vertex(&mut vertices, segment.cap.back(), cap_curve.subs(b))?;
        edge(&mut edges, &segment.cap, &cap_start, &cap_end, cap_curve)?;
        let (a, b) = top_curve.range_tuple();
        let top_start = vertex(&mut vertices, segment.top.front(), top_curve.subs(a))?;
        let top_end = vertex(&mut vertices, segment.top.back(), top_curve.subs(b))?;
        let top = edge(
            &mut edges,
            &segment.top,
            &top_start,
            &top_end,
            top_curve.clone(),
        )?;
        let (a, b) = base_curve.range_tuple();
        let base_start = vertex(&mut vertices, segment.base.front(), base_curve.subs(a))?;
        let base_end = vertex(&mut vertices, segment.base.back(), base_curve.subs(b))?;
        let base = edge(
            &mut edges,
            &segment.base,
            &base_start,
            &base_end,
            base_curve.clone(),
        )?;
        let start = edge(
            &mut edges,
            &segment.generators[0],
            &top_start,
            &base_start,
            Curve::Line(Line(top_start.point(), base_start.point())),
        )?;
        let end = edge(
            &mut edges,
            &segment.generators[1],
            &top_end,
            &base_end,
            Curve::Line(Line(top_end.point(), base_end.point())),
        )?;
        let mut surface = connected_rims::product_surface(
            &top_curve,
            &top_curve,
            &base_curve,
            Treatment::Chamfer,
        )?;
        let (a, b) = top_curve.range_tuple();
        let original = segment.top.oriented_curve();
        let (x, y) = original.range_tuple();
        let expected = segment.carrier.normal(original.subs((x + y) * 0.5))? * segment.side;
        if surface.normal((a + b) * 0.5, 0.5).dot(expected) < 0. {
            surface.invert();
        }
        let wall = Face::try_new(
            vec![Wire::from(vec![top.inverse(), start, base, end.inverse()])],
            surface,
        )
        .map_err(|_| invalid("Reconstructed offset wall has inconsistent boundaries"))?;
        faces.insert(segment.wall.id(), wall);
        if let Some(round) = round {
            let mut meridians = Vec::new();
            for (index, source) in round.meridians.iter().enumerate() {
                let (a, b) = if index == 0 {
                    (&cap_start, &top_start)
                } else {
                    (&cap_end, &top_end)
                };
                let curve = round
                    .carrier
                    .offset_curve(&source.oriented_curve(), d * round.side)?;
                meridians.push(edge(&mut edges, source, a, b, curve)?);
            }
            let cap = if segment.cap.orientation() {
                edges[&segment.cap.id()].clone()
            } else {
                edges[&segment.cap.id()].inverse()
            };
            let surface = round
                .carrier
                .offset_surface(&round.face.oriented_surface(), d * round.side)?;
            let blend = Face::try_new(
                vec![Wire::from(vec![
                    cap.inverse(),
                    meridians.remove(0),
                    top.clone(),
                    meridians.remove(0).inverse(),
                ])],
                surface,
            )
            .map_err(|_| invalid("A rounded offset strip has inconsistent native boundaries"))?;
            faces.insert(round.face.id(), blend);
        }
    }
    for (source, open, distance) in [(&graph.cap, cap_open, d), (&graph.base, base_open, d)] {
        if open {
            continue;
        }
        let carrier = analytic_offset::recognize(source)?
            .ok_or_else(|| invalid("A stock cap lost its planar carrier"))?;
        let side = analytic_offset::side(source, carrier)?;
        let wires = source
            .boundaries()
            .iter()
            .map(|wire| mapped_wire(wire, &edges))
            .collect();
        let face = Face::try_new(
            wires,
            carrier.offset_surface(&source.oriented_surface(), distance * side)?,
        )
        .map_err(|_| invalid("Offset cap contours are inconsistent with their supporting plane"))?;
        faces.insert(source.id(), face);
    }
    let mut retained = solid
        .face_iter()
        .filter(|f| !removed.contains(&f.id()))
        .cloned()
        .collect::<Shell>();
    let changed = retained
        .iter()
        .map(|f| {
            faces
                .get(&f.id())
                .cloned()
                .ok_or_else(|| invalid("A retained face lost its reconstructed offset"))
        })
        .collect::<Result<Shell>>()?;
    for (source, face) in &faces {
        history.construct_face(
            face.id(),
            &[history.face(*source)?],
            serde_json::json!({"operation":"shell","constructor":"connected_rim_uniform_offset"}),
        )?;
    }
    for (source, e) in &edges {
        history.construct_edge(e.id(),&[history.edge(*source)?],serde_json::json!({"operation":"shell","constructor":"connected_native_offset_boundary"}))?;
    }
    let exposed = retained.extract_boundaries();
    if outward {
        retained.invert();
        retained.extend(changed.iter().cloned());
    } else {
        retained.extend(changed.inverse());
    }
    for wire in exposed {
        let mapped = mapped_wire(&wire, &edges);
        let rims = crate::loft_correspondence::skin(&wire, &mapped, None, None)?;
        for (source, rim) in wire.iter().zip(rims) {
            let rim = if outward { rim } else { rim.inverse() };
            history.construct_face(
                rim.id(),
                &[history.edge(source.id())?],
                serde_json::json!({"operation":"shell","constructor":"connected_cap_opening_rim"}),
            )?;
            retained.push(rim);
        }
    }
    let components = retained.connected_components();
    let results = if removed.is_empty() {
        vec![
            Solid::try_new(components)
                .map_err(|_| invalid("The inner and outer rounded boundaries do not close"))?,
        ]
    } else {
        components
            .into_iter()
            .map(|shell| {
                Solid::try_new(vec![shell])
                    .map_err(|_| invalid("Rounded opening rims do not close their wall component"))
            })
            .collect::<Result<Vec<_>>>()?
    };
    for result in &results {
        crate::validation::solid(result)?;
    }
    Ok(Some(results))
}
