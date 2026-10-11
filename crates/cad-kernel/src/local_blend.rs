//! Local exact treatments of straight planar wedges. A selected edge may be
//! convex or concave, and unrelated curved/pocketed topology is retained.
//! Adjacent selected edges at one vertex are left to the simultaneous corner
//! constructor: no sequential operation guesses the missing transition.
use super::Treatment;
use crate::{
    planar::{BoundaryPlane, ChamferSpec},
    shell_history::ShellHistory,
};
use cad_core::{CadError, Result};
use monstertruck::modeling::*;
use std::collections::{HashMap, HashSet};
const EPS: f64 = 1e-7;
const WORK_LIMIT: usize = 4_000_000;
fn invalid(s: &str) -> CadError {
    CadError::Invalid(format!("Edge treatment: {s}"))
}
fn unsupported(s: &str) -> CadError {
    CadError::Unsupported(format!("Edge treatment: {s}"))
}
fn absolute(edge: &Edge) -> Edge {
    if edge.orientation() {
        edge.clone()
    } else {
        edge.inverse()
    }
}
fn oriented(edge: &Edge, source: &Edge) -> Edge {
    if source.orientation() {
        edge.clone()
    } else {
        edge.inverse()
    }
}
struct End {
    source: Vertex,
    cap: BoundaryPlane,
    points: [Vertex; 2],
    neighbours: [EdgeId; 2],
    curve: Edge,
    controls: Vec<Vector4>,
}
struct Blend {
    edge: Edge,
    walls: [BoundaryPlane; 2],
    ends: [End; 2],
    rails: [Edge; 2],
    bounds: Vec<(Point3, Vector3)>,
}
fn outside(hull: &[Point3], planes: &[(Point3, Vector3)]) -> Result<bool> {
    if hull.is_empty() {
        return Ok(false);
    }
    let enclosure = crate::native_bounds::Enclosure::from_hull(hull.to_vec())?;
    for (origin, normal) in planes {
        if enclosure.projection(*origin, *normal)?.0 > EPS {
            return Ok(true);
        }
    }
    Ok(false)
}
fn curve_outside(curve: &Curve, planes: &[(Point3, Vector3)], work: &mut usize) -> Result<bool> {
    let enclosure = crate::native_bounds::curve(curve)?;
    *work = work.saturating_add(enclosure.len());
    if *work > WORK_LIMIT {
        return Err(unsupported("Collision proof exceeds its work budget"));
    }
    if outside(enclosure.points(), planes)? {
        return Ok(true);
    }
    let Some(curve) = crate::native_bounds::subdivision_curve(curve)? else {
        return Ok(false);
    };
    let mut stack = vec![(curve, 0)];
    while let Some((curve, depth)) = stack.pop() {
        *work = work.saturating_add(curve.count().saturating_mul(8));
        if *work > WORK_LIMIT {
            return Err(unsupported("Collision proof exceeds its work budget"));
        }
        if outside(curve.enclosure()?.points(), planes)? {
            continue;
        }
        if depth == 12 {
            return Ok(false);
        }
        let Some([a, b]) = curve.bisect(work, WORK_LIMIT)? else {
            return Ok(false);
        };
        stack.push((a, depth + 1));
        stack.push((b, depth + 1));
    }
    Ok(true)
}
pub(super) fn face_outside(
    face: &Face,
    planes: &[(Point3, Vector3)],
    work: &mut usize,
) -> Result<bool> {
    let enclosure = crate::native_bounds::face(face)?;
    *work = work.saturating_add(enclosure.len());
    if *work > WORK_LIMIT {
        return Err(unsupported("Collision proof exceeds its work budget"));
    }
    for (origin, normal) in planes {
        if enclosure.projection(*origin, *normal)?.0 > EPS {
            return Ok(true);
        }
    }
    if crate::planar::boundary_plane(face)?.is_some() {
        return Ok(false);
    }
    let Some(surface) = crate::native_bounds::subdivision_surface(face)? else {
        return Ok(false);
    };
    let mut stack = vec![(surface, 0)];
    while let Some((surface, depth)) = stack.pop() {
        *work = work.saturating_add(surface.count().saturating_mul(8));
        if *work > WORK_LIMIT {
            return Err(unsupported("Collision proof exceeds its work budget"));
        }
        if outside(surface.enclosure()?.points(), planes)? {
            continue;
        }
        if depth == 14 {
            return Ok(false);
        }
        let Some([a, b]) = surface.bisect(depth % 2, work, WORK_LIMIT)? else {
            return Ok(false);
        };
        stack.push((a, depth + 1));
        stack.push((b, depth + 1));
    }
    Ok(true)
}
fn distances(spec: &ChamferSpec, walls: &[BoundaryPlane; 2], cosine: f64) -> Result<[f64; 2]> {
    let first = match spec.first_face {
        Some(id) if id == walls[0].face.id() => 0,
        Some(id) if id == walls[1].face.id() => 1,
        Some(_) => return Err(invalid("Distance 1 face must adjoin every selected edge")),
        None => {
            let a = walls[0].normal;
            let b = walls[1].normal;
            usize::from(
                a.x.total_cmp(&b.x)
                    .then(a.y.total_cmp(&b.y))
                    .then(a.z.total_cmp(&b.z))
                    .is_gt(),
            )
        }
    } ^ usize::from(spec.flip);
    let second = if let Some(angle) = spec.angle {
        let interior = (-cosine).clamp(-1., 1.).acos();
        if !angle.is_finite() || angle <= 0. || angle >= std::f64::consts::PI - interior {
            return Err(invalid("The chamfer angle exceeds this material wedge"));
        }
        spec.distance * angle.sin() / (interior + angle).sin()
    } else {
        spec.second_distance.unwrap_or(spec.distance)
    };
    if !spec.distance.is_finite() || spec.distance <= EPS || !second.is_finite() || second <= EPS {
        return Err(invalid(
            "Setbacks must be finite and exceed modeling tolerance",
        ));
    }
    Ok(if first == 0 {
        [spec.distance, second]
    } else {
        [second, spec.distance]
    })
}
#[allow(clippy::too_many_arguments)]
fn end(
    source: &Vertex,
    edge: &Edge,
    walls: &[BoundaryPlane; 2],
    axis: Vector3,
    offsets: [Vector3; 2],
    weight: f64,
    faces: &HashMap<FaceId, Face>,
    adjacency: &HashMap<EdgeId, Vec<FaceId>>,
    vertices: &HashMap<VertexId, HashSet<FaceId>>,
    edges: &HashMap<EdgeId, Edge>,
    treatment: Treatment,
    history: &mut ShellHistory,
) -> Result<Option<End>> {
    let Some(incident) = vertices.get(&source.id()) else {
        return Ok(None);
    };
    if incident.len() != 3 {
        return Ok(None);
    }
    let Some(cap_id) = incident
        .iter()
        .find(|id| walls.iter().all(|wall| wall.face.id() != **id))
    else {
        return Ok(None);
    };
    let Some(cap) = crate::planar::boundary_plane(&faces[cap_id])? else {
        return Ok(None);
    };
    let denominator = axis.dot(cap.normal);
    if denominator.abs() < 1e-9 {
        return Ok(None);
    }
    let mut points = Vec::new();
    let mut neighbours = Vec::new();
    for (side, offset) in offsets.iter().enumerate() {
        let candidates = edges
            .values()
            .filter(|other| {
                other.id() != edge.id()
                    && (other.front().id() == source.id() || other.back().id() == source.id())
                    && adjacency[&other.id()].contains(&walls[side].face.id())
                    && adjacency[&other.id()].contains(cap_id)
            })
            .collect::<Vec<_>>();
        let [neighbour] = candidates.as_slice() else {
            return Ok(None);
        };
        if !crate::planar::straight_edge(neighbour)? {
            return Ok(None);
        }
        let p = source.point() + *offset - axis * (offset.dot(cap.normal) / denominator);
        if ![p.x, p.y, p.z].iter().all(|v| v.is_finite()) {
            return Err(invalid("The setback exceeds finite coordinates"));
        }
        let other = if neighbour.front().id() == source.id() {
            neighbour.back()
        } else {
            neighbour.front()
        };
        let direction = other.point() - source.point();
        let length = direction.magnitude2();
        let t = (p - source.point()).dot(direction) / length;
        if t <= EPS / length.sqrt()
            || t >= 1. - EPS / length.sqrt()
            || (p - source.point() - direction * t).magnitude() > EPS
        {
            return Err(invalid(
                "The setback consumes or leaves an adjacent edge; reduce its size",
            ));
        }
        points.push(Vertex::new(p));
        neighbours.push(neighbour.id());
    }
    let points: [Vertex; 2] = points
        .try_into()
        .map_err(|_| invalid("A terminal profile requires two tangent points"))?;
    let neighbours: [EdgeId; 2] = neighbours
        .try_into()
        .map_err(|_| invalid("A terminal profile requires two neighbouring edges"))?;
    let controls = match treatment {
        Treatment::Chamfer => vec![
            points[0].point().to_vec().extend(1.),
            points[1].point().to_vec().extend(1.),
        ],
        Treatment::Fillet => vec![
            points[0].point().to_vec().extend(1.),
            source.point().to_vec().extend(1.) * weight,
            points[1].point().to_vec().extend(1.),
        ],
    };
    let curve = Edge::try_new(
        &points[0],
        &points[1],
        Curve::NurbsCurve(NurbsCurve::new(BsplineCurve::new(
            KnotVector::bezier_knot(controls.len() - 1),
            controls.clone(),
        ))),
    )
    .map_err(|_| invalid("The terminal profile collapsed"))?;
    history.construct_edge(
        curve.id(),
        &[history.edge(edge.id())?, history.face(*cap_id)?],
        serde_json::json!({"operation":treatment.name(),"constructor":"local_terminal_profile"}),
    )?;
    Ok(Some(End {
        source: source.clone(),
        cap,
        points,
        neighbours,
        curve,
        controls,
    }))
}
fn changed_hull(edge: &Edge, ends: &[End; 2]) -> Result<Vec<Point3>> {
    let mut hull = vec![
        edge.front().point(),
        ends[0].points[0].point(),
        ends[0].points[1].point(),
        edge.back().point(),
        ends[1].points[0].point(),
        ends[1].points[1].point(),
    ];
    // The new surface copies these two exact homogeneous rows. Bounding their
    // outward projections includes any rounding introduced by the fillet
    // weight, which need not leave its projected middle control at the source
    // vertex. The remaining straight rails and removed wedge lie in this hull.
    for end in ends {
        hull.extend_from_slice(crate::native_bounds::curve(&end.curve.curve())?.points());
    }
    Ok(hull)
}
fn bounds(edge: &Edge, ends: &[End; 2]) -> Result<Vec<(Point3, Vector3)>> {
    let hull = changed_hull(edge, ends)?;
    let vertices = &hull[..6];
    let enclosure = crate::native_bounds::Enclosure::from_hull(hull.clone())?;
    let mut result = Vec::new();
    for [a, b, c] in [[0, 1, 2], [3, 4, 5], [0, 1, 3], [0, 2, 3], [1, 2, 4]] {
        let raw = (vertices[b] - vertices[a]).cross(vertices[c] - vertices[a]);
        let length = raw.magnitude();
        if !length.is_finite() || length <= EPS * EPS {
            return Err(invalid("The changed material region collapsed"));
        }
        let mut normal = raw / length;
        let (low, high) = enclosure.projection(vertices[a], normal)?;
        // Normals only propose a supporting direction. Certify its orientation
        // and support against the complete constructed control hull. outside()
        // accepts only query.low > EPS, so this identical threshold encloses
        // every changed point on the retained side of each candidate plane.
        if high <= EPS {
            // Already an outward supporting direction.
        } else if -low <= EPS {
            normal = -normal;
        } else {
            return Err(unsupported(
                "The terminal planes cannot certify a complete unfolded material wedge",
            ));
        }
        result.push((vertices[a], normal));
    }
    Ok(result)
}
pub(super) fn construct(
    solid: &Solid,
    selected: &[EdgeId],
    spec: &ChamferSpec,
    treatment: Treatment,
    history: &mut ShellHistory,
) -> Result<Option<Solid>> {
    if selected.is_empty() {
        return Ok(None);
    }
    let faces = solid
        .face_iter()
        .map(|face| (face.id(), face.clone()))
        .collect::<HashMap<_, _>>();
    if faces.len() > 4096 || selected.len() > cad_core::edge_features::MAX_SELECTED_EDGES {
        return Err(unsupported(
            "Selection exceeds the edge treatment work limit",
        ));
    }
    let mut edges = HashMap::new();
    let mut adjacency: HashMap<EdgeId, Vec<FaceId>> = HashMap::new();
    let mut vertices: HashMap<VertexId, HashSet<FaceId>> = HashMap::new();
    for face in solid.face_iter() {
        for edge in face.edge_iter() {
            edges.entry(edge.id()).or_insert_with(|| absolute(&edge));
            adjacency.entry(edge.id()).or_default().push(face.id());
            for vertex in [edge.front(), edge.back()] {
                vertices.entry(vertex.id()).or_default().insert(face.id());
            }
        }
    }
    let selected = selected.iter().copied().collect::<HashSet<_>>();
    let mut ordered = selected
        .iter()
        .map(|id| {
            Ok((
                crate::shell_history::closure(&[history.edge(*id)?])?.to_string(),
                *id,
            ))
        })
        .collect::<Result<Vec<_>>>()?;
    ordered.sort_by(|a, b| a.0.cmp(&b.0));
    let mut touched = HashSet::new();
    for id in &selected {
        let Some(edge) = edges.get(id) else {
            return Err(invalid("A selected edge is absent from the body"));
        };
        if !crate::planar::straight_edge(edge)? {
            return Ok(None);
        }
        for vertex in [edge.front(), edge.back()] {
            if !touched.insert(vertex.id()) {
                return Ok(None);
            }
        }
    }
    let mut blends = Vec::new();
    for (_, id) in ordered {
        let edge = &edges[&id];
        let adjacent = &adjacency[&id];
        let [a, b] = adjacent.as_slice() else {
            return Ok(None);
        };
        let (Some(a), Some(b)) = (
            crate::planar::boundary_plane(&faces[a])?,
            crate::planar::boundary_plane(&faces[b])?,
        ) else {
            return Ok(None);
        };
        let walls = [a, b];
        let axis = (edge.back().point() - edge.front().point()).normalize();
        let cosine = walls[0].normal.dot(walls[1].normal).clamp(-1., 1.);
        let sine = (1. - cosine * cosine).max(0.).sqrt();
        if sine < 1e-9 {
            return Ok(None);
        }
        let oriented = walls[0]
            .face
            .edge_iter()
            .find(|candidate| candidate.id() == id)
            .ok_or_else(|| invalid("A selected edge is absent from its wall"))?;
        let sign = walls[0]
            .normal
            .cross(walls[1].normal)
            .dot((oriented.back().point() - oriented.front().point()).normalize())
            .signum();
        let (offsets, weight) = match treatment {
            Treatment::Chamfer => {
                let d = distances(spec, &walls, cosine)?;
                (
                    [
                        -(walls[1].normal - walls[0].normal * cosine) * (sign * d[0] / sine),
                        -(walls[0].normal - walls[1].normal * cosine) * (sign * d[1] / sine),
                    ],
                    1.,
                )
            }
            Treatment::Fillet => {
                let r = spec.distance;
                if !r.is_finite() || r <= EPS {
                    return Err(invalid(
                        "Radius must be finite and exceed modeling tolerance",
                    ));
                }
                let center = -(walls[0].normal + walls[1].normal) * (sign * r / (1. + cosine));
                (
                    [
                        center + walls[0].normal * (sign * r),
                        center + walls[1].normal * (sign * r),
                    ],
                    ((1. + cosine) * 0.5).sqrt(),
                )
            }
        };
        let Some(first) = end(
            edge.front(),
            edge,
            &walls,
            axis,
            offsets,
            weight,
            &faces,
            &adjacency,
            &vertices,
            &edges,
            treatment,
            history,
        )?
        else {
            return Ok(None);
        };
        let Some(last) = end(
            edge.back(),
            edge,
            &walls,
            axis,
            offsets,
            weight,
            &faces,
            &adjacency,
            &vertices,
            &edges,
            treatment,
            history,
        )?
        else {
            return Ok(None);
        };
        let ends = [first, last];
        if ends[0]
            .controls
            .iter()
            .zip(&ends[1].controls)
            .any(|(a, b)| {
                let a = Point3::from_vec(a.truncate() / a.w);
                let b = Point3::from_vec(b.truncate() / b.w);
                (b - a).dot(axis) <= EPS
            })
        {
            return Err(invalid(
                "The terminal profiles meet or reverse; reduce the setback",
            ));
        }
        let rails = [
            builder::line(&ends[0].points[0], &ends[1].points[0]),
            builder::line(&ends[0].points[1], &ends[1].points[1]),
        ];
        for (side, rail) in rails.iter().enumerate() {
            history.construct_edge(rail.id(),&[history.edge(id)?,history.face(walls[side].face.id())?],serde_json::json!({"operation":treatment.name(),"constructor":"local_tangent_rail"}))?;
        }
        let bounds = bounds(edge, &ends)?;
        blends.push(Blend {
            edge: edge.clone(),
            walls,
            ends,
            rails,
            bounds,
        });
    }
    let mut work = 0usize;
    for blend in &blends {
        let local_faces = blend
            .walls
            .iter()
            .map(|wall| wall.face.id())
            .chain(blend.ends.iter().map(|end| end.cap.face.id()))
            .collect::<HashSet<_>>();
        let local_edges = std::iter::once(blend.edge.id())
            .chain(blend.ends.iter().flat_map(|end| end.neighbours))
            .collect::<HashSet<_>>();
        for face in solid.face_iter() {
            if local_faces.contains(&face.id()) {
                for edge in face
                    .edge_iter()
                    .filter(|edge| !local_edges.contains(&edge.id()))
                {
                    if !curve_outside(&edge.curve(), &blend.bounds, &mut work)? {
                        return Err(unsupported(
                            "The changed material region reaches another boundary on an adjacent face; reduce the setback",
                        ));
                    }
                }
            } else if !face_outside(face, &blend.bounds, &mut work)? {
                return Err(unsupported(
                    "The changed material region may intersect another face or cavity; reduce the setback",
                ));
            }
        }
        // Complete triangular prisms bound every fillet or bevel. Distinct
        // selected prisms must be separated before their topology is combined.
        for other in &blends {
            if other.edge.id() == blend.edge.id() {
                continue;
            }
            let hull = changed_hull(&other.edge, &other.ends)?;
            work = work.saturating_add(hull.len());
            if work > WORK_LIMIT {
                return Err(unsupported(
                    "Selected blend separation exceeds its work budget",
                ));
            }
            if !outside(&hull, &blend.bounds)? {
                let own = changed_hull(&blend.edge, &blend.ends)?;
                work = work.saturating_add(own.len());
                if work > WORK_LIMIT {
                    return Err(unsupported(
                        "Selected blend separation exceeds its work budget",
                    ));
                }
                if !outside(&own, &other.bounds)? {
                    return Err(invalid(
                        "Selected edge setbacks meet or overlap; reduce their size",
                    ));
                }
            }
        }
    }
    let mut endpoint: HashMap<(EdgeId, VertexId), Vertex> = HashMap::new();
    for blend in &blends {
        for end in &blend.ends {
            for side in 0..2 {
                endpoint.insert(
                    (end.neighbours[side], end.source.id()),
                    end.points[side].clone(),
                );
            }
        }
    }
    let mut trimmed = HashMap::new();
    for edge in edges.values().filter(|edge| !selected.contains(&edge.id())) {
        let a = endpoint
            .get(&(edge.id(), edge.front().id()))
            .unwrap_or(edge.front());
        let b = endpoint
            .get(&(edge.id(), edge.back().id()))
            .unwrap_or(edge.back());
        if a.id() == edge.front().id() && b.id() == edge.back().id() {
            continue;
        }
        let direction = edge.back().point() - edge.front().point();
        if (b.point() - a.point()).dot(direction) <= EPS * direction.magnitude() {
            return Err(invalid(
                "Selected treatments consume a shared neighbouring edge",
            ));
        }
        let output = builder::line(a, b);
        history.retain_edge(output.id(), edge.id())?;
        trimmed.insert(edge.id(), output);
    }
    let mut replacements = HashMap::new();
    for face in solid.face_iter() {
        let affected = blends.iter().any(|blend| {
            blend.walls.iter().any(|wall| wall.face.id() == face.id())
                || blend.ends.iter().any(|end| end.cap.face.id() == face.id())
        });
        if !affected {
            continue;
        }
        let mut wires = Vec::new();
        for wire in face.boundaries() {
            let segments = wire
                .iter()
                .map(|edge| -> Result<Edge> {
                    if let Some(blend) = blends.iter().find(|blend| blend.edge.id() == edge.id()) {
                        let side = blend
                            .walls
                            .iter()
                            .position(|wall| wall.face.id() == face.id())
                            .ok_or_else(|| {
                                invalid("Selected edge has an unexpected face incidence")
                            })?;
                        Ok(oriented(&blend.rails[side], edge))
                    } else {
                        Ok(trimmed
                            .get(&edge.id())
                            .map_or_else(|| edge.clone(), |new| oriented(new, edge)))
                    }
                })
                .collect::<Result<Vec<_>>>()?;
            let mut output = Wire::new();
            for index in 0..segments.len() {
                let current = &segments[index];
                let next = &segments[(index + 1) % segments.len()];
                output.push_back(current.clone());
                if current.back().id() != next.front().id() {
                    let end = blends
                        .iter()
                        .flat_map(|blend| &blend.ends)
                        .find(|end| {
                            end.cap.face.id() == face.id()
                                && end.source.id() == wire[index].back().id()
                        })
                        .ok_or_else(|| invalid("A terminal face has an unfilled corner"))?;
                    let arc = if end.curve.front().id() == current.back().id()
                        && end.curve.back().id() == next.front().id()
                    {
                        end.curve.clone()
                    } else if end.curve.back().id() == current.back().id()
                        && end.curve.front().id() == next.front().id()
                    {
                        end.curve.inverse()
                    } else {
                        return Err(invalid("A terminal profile cannot join its adjacent edges"));
                    };
                    output.push_back(arc);
                }
            }
            wires.push(output);
        }
        let output = Face::try_new(wires, face.oriented_surface())
            .map_err(|_| invalid("A setback leaves an inconsistent face boundary"))?;
        history.retain_face(output.id(), face.id())?;
        replacements.insert(face.id(), output);
    }
    let mut additions: HashMap<FaceId, Vec<Face>> = HashMap::new();
    for blend in &blends {
        let controls = blend.ends[0]
            .controls
            .iter()
            .zip(&blend.ends[1].controls)
            .map(|(a, b)| vec![*a, *b])
            .collect();
        let surface = NurbsSurface::new(BsplineSurface::new(
            (
                KnotVector::bezier_knot(blend.ends[0].controls.len() - 1),
                KnotVector::bezier_knot(1),
            ),
            controls,
        ));
        let wire = Wire::from(vec![
            blend.ends[0].curve.clone(),
            blend.rails[1].clone(),
            blend.ends[1].curve.inverse(),
            blend.rails[0].inverse(),
        ]);
        let mut face = Face::try_new(vec![wire], Surface::NurbsSurface(surface))
            .map_err(|_| invalid("The blend has inconsistent boundaries"))?;
        let source_occurrence = blend.walls[0]
            .face
            .edge_iter()
            .find(|edge| edge.id() == blend.edge.id())
            .ok_or_else(|| invalid("The source wall lost its selected boundary"))?;
        // On the new face rail 0 starts in the negative source-edge direction;
        // this already opposes an absolute-forward source wall occurrence.
        if !source_occurrence.orientation() {
            face.invert();
        }
        history.construct_face(face.id(),&[history.edge(blend.edge.id())?,history.face(blend.walls[0].face.id())?,history.face(blend.walls[1].face.id())?],serde_json::json!({"operation":treatment.name(),"constructor":"local_exact_planar_wedge"}))?;
        additions
            .entry(blend.walls[0].face.id())
            .or_default()
            .push(face);
    }
    let mut shells = Vec::new();
    for shell in solid.boundaries() {
        let mut output = Shell::new();
        for face in shell {
            output.push(
                replacements
                    .get(&face.id())
                    .cloned()
                    .unwrap_or_else(|| face.clone()),
            );
            if let Some(blends) = additions.remove(&face.id()) {
                output.extend(blends);
            }
        }
        shells.push(output);
    }
    let output = Solid::try_new(shells)
        .map_err(|_| invalid("Edge treatments do not form a closed oriented body"))?;
    crate::validation::solid(&output)?;
    Ok(Some(output))
}
