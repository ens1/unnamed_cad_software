//! Exact circular edge rounds on convex planar stock. All selected edges are
//! constructed together; a corner is never rounded against an already changed
//! neighbour. No tessellation or fitted relay-sphere surface is authoritative.
use crate::{planar::BoundaryPlane, shell_history::ShellHistory};
use cad_core::{CadError, Result};
use monstertruck::modeling::*;
use std::collections::{HashMap, HashSet};

const EPS: f64 = 1e-7;
const MAX_EDGES: usize = 256;
const MAX_FACES: usize = 2048;
const MAX_PROTECTED_CONTROLS: usize = 65_536;
const MAX_SEPARATION_WORK: usize = 8_000_000;

fn invalid(message: &str) -> CadError {
    CadError::Invalid(format!("Fillet: {message}"))
}
fn unsupported(message: &str) -> CadError {
    CadError::Unsupported(format!("Fillet: {message}"))
}
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum PointRole {
    Face(FaceId),
    Edge(EdgeId),
}
struct Corner {
    vertex: Vertex,
    edges: Vec<EdgeId>,
    faces: Vec<FaceId>,
    selected: Vec<EdgeId>,
    center: Option<Point3>,
    points: HashMap<PointRole, Vertex>,
    mitre: Option<Edge>,
}
struct Blend {
    source: Edge,
    faces: [FaceId; 2],
    axis: Vector3,
    origin: Point3,
    ends: [End; 2],
    rails: [Edge; 2],
}
struct End {
    arc: Edge,
    controls: [Vector4; 3],
}

fn finite(point: Point3) -> bool {
    point.x.is_finite() && point.y.is_finite() && point.z.is_finite()
}
fn homogeneous(point: Point3, weight: f64) -> Vector4 {
    point.to_vec().extend(1.) * weight
}
fn point(control: Vector4) -> Point3 {
    Point3::from_vec(control.truncate() / control.w)
}
fn plane_center(
    corner: &Corner,
    planes: &HashMap<FaceId, BoundaryPlane>,
    r: f64,
) -> Result<Point3> {
    let normals = corner
        .faces
        .iter()
        .map(|face| planes[face].normal)
        .collect::<Vec<_>>();
    let matrix = Matrix3::from_cols(normals[0], normals[1], normals[2]).transpose();
    if matrix.determinant().abs() < 1e-9 {
        return Err(unsupported(
            "The corner's supporting planes are too ill-conditioned for a finite-radius construction",
        ));
    }
    let inverse = matrix
        .invert()
        .ok_or_else(|| unsupported("The corner's supporting planes are singular"))?;
    let center = corner.vertex.point() - inverse * Vector3::new(r, r, r);
    finite(center)
        .then_some(center)
        .ok_or_else(|| invalid("The corner's radius produced a non-finite center"))
}

fn corner_role(
    corner: &Corner,
    face: FaceId,
    selected: EdgeId,
    adjacency: &HashMap<EdgeId, Vec<FaceId>>,
) -> Result<PointRole> {
    let other = corner
        .edges
        .iter()
        .find(|edge| **edge != selected && adjacency[edge].contains(&face))
        .ok_or_else(|| unsupported("A corner does not have two boundary edges on its wall"))?;
    Ok(if corner.selected.contains(other) {
        PointRole::Face(face)
    } else {
        PointRole::Edge(*other)
    })
}
fn corner_point(corner: &mut Corner, role: PointRole, value: Point3) -> Result<Vertex> {
    if let Some(existing) = corner.points.get(&role) {
        if (existing.point() - value).magnitude() > EPS {
            return Err(unsupported(
                "The selected corner requires a transition beyond a circular edge round",
            ));
        }
        return Ok(existing.clone());
    }
    let vertex = Vertex::new(value);
    corner.points.insert(role, vertex.clone());
    Ok(vertex)
}

#[allow(clippy::too_many_arguments)]
fn end_profile(
    corner: &mut Corner,
    source: &Edge,
    faces: [FaceId; 2],
    axis: Vector3,
    origin: Point3,
    normals: [Vector3; 2],
    radius: f64,
    planes: &HashMap<FaceId, BoundaryPlane>,
    edges: &HashMap<EdgeId, Edge>,
    adjacency: &HashMap<EdgeId, Vec<FaceId>>,
    history: &mut ShellHistory,
) -> Result<End> {
    let (plane_origin, plane_normal, kind) = match corner.selected.len() {
        1 => {
            let cap = corner
                .faces
                .iter()
                .find(|face| !faces.contains(face))
                .unwrap();
            (planes[cap].origin, planes[cap].normal, "cap")
        }
        2 => {
            let other = corner
                .selected
                .iter()
                .find(|edge| **edge != source.id())
                .unwrap();
            let other = &edges[other];
            let direction = if other.front().id() == corner.vertex.id() {
                other.back().point() - other.front().point()
            } else {
                other.front().point() - other.back().point()
            }
            .normalize();
            let own = if source.front().id() == corner.vertex.id() {
                axis
            } else {
                -axis
            };
            (
                corner.vertex.point(),
                (own - direction).normalize(),
                "mitre",
            )
        }
        3 => (corner.center.unwrap(), axis, "sphere"),
        _ => {
            return Err(unsupported(
                "A selected edge has unsupported corner valence",
            ));
        }
    };
    let denominator = axis.dot(plane_normal);
    if denominator.abs() < 1e-9 {
        return Err(unsupported(
            "A terminal wall is parallel to the rounded edge",
        ));
    }
    let cosine = normals[0].dot(normals[1]).clamp(-1., 1.);
    let weight = ((1. + cosine) / 2.).sqrt();
    if weight <= 1e-8 {
        return Err(unsupported(
            "A nearly flat material wedge cannot carry a finite circular round",
        ));
    }
    let radial = [
        normals[0],
        (normals[0] + normals[1]) / (1. + cosine),
        normals[1],
    ];
    let weights = [1., weight, 1.];
    let mut controls = [Vector4::zero(); 3];
    for index in 0..3 {
        let base = origin + radius * radial[index];
        let value = base + axis * ((plane_origin - base).dot(plane_normal) / denominator);
        if !finite(value) {
            return Err(invalid("The radius produced non-finite boundary controls"));
        }
        controls[index] = homogeneous(value, weights[index]);
    }
    let first_role = corner_role(corner, faces[0], source.id(), adjacency)?;
    let last_role = corner_role(corner, faces[1], source.id(), adjacency)?;
    if (point(controls[2]) - point(controls[0])).magnitude() <= TOLERANCE {
        return Err(invalid(
            "The round's terminal arc is below the kernel's geometric precision; increase the radius",
        ));
    }
    let first = corner_point(corner, first_role, point(controls[0]))?;
    let last = corner_point(corner, last_role, point(controls[2]))?;
    let arc = if kind == "mitre"
        && let Some(existing) = &corner.mitre
    {
        let arc = if existing.front().id() == first.id() && existing.back().id() == last.id() {
            existing.clone()
        } else if existing.back().id() == first.id() && existing.front().id() == last.id() {
            existing.inverse()
        } else {
            return Err(invalid(
                "The adjoining round ends do not share terminal vertices",
            ));
        };
        let Curve::NurbsCurve(curve) = arc.oriented_curve() else {
            unreachable!()
        };
        for (new, prior) in controls.iter_mut().zip(curve.control_points()) {
            if (point(*new) - point(*prior)).magnitude() > EPS || (new.w - prior.w).abs() > 1e-9 {
                return Err(unsupported(
                    "The adjoining cylinders require a non-mitre corner transition",
                ));
            }
            *new = *prior;
        }
        arc
    } else {
        let curve = NurbsCurve::new(BsplineCurve::new(
            KnotVector::bezier_knot(2),
            controls.to_vec(),
        ));
        let arc = Edge::new(&first, &last, Curve::NurbsCurve(curve));
        let mut sources = corner
            .edges
            .iter()
            .map(|edge| history.edge(*edge))
            .collect::<Result<Vec<_>>>()?;
        sources.extend(
            corner
                .faces
                .iter()
                .map(|face| history.face(*face))
                .collect::<Result<Vec<_>>>()?,
        );
        // Source-edge identity distinguishes the three sphere arcs; a mitre arc
        // is deliberately shared by both selected edges at this corner.
        if kind != "mitre" {
            sources.push(history.edge(source.id())?);
        }
        history.construct_edge(arc.id(), &sources, serde_json::json!({"operation":"fillet","constructor":"end_arc","kind":kind,"selected_edge":crate::shell_history::closure(&[history.edge(source.id())?])?}))?;
        if kind == "mitre" {
            corner.mitre = Some(arc.clone());
        }
        arc
    };
    Ok(End { arc, controls })
}

fn oriented(edge: &Edge, first: &Vertex, last: &Vertex) -> Result<Edge> {
    if edge.front().id() == first.id() && edge.back().id() == last.id() {
        Ok(edge.clone())
    } else if edge.back().id() == first.id() && edge.front().id() == last.id() {
        Ok(edge.inverse())
    } else {
        Err(invalid("A reconstructed wall boundary cannot be joined"))
    }
}

/// Returns `None` for stock outside this constructor's proven planar class.
/// Once eligible, collapsed edges and excessive radii are explicit failures.
pub(crate) fn round(
    solid: &Solid,
    selected: &[EdgeId],
    radius: f64,
    history: &mut ShellHistory,
) -> Result<Option<Solid>> {
    if solid.boundaries().len() != 1 || selected.is_empty() {
        return Ok(None);
    }
    if !radius.is_finite() || radius <= EPS {
        return Err(invalid(
            "Radius must be finite and positive above the modeling tolerance",
        ));
    }
    if selected.len() > MAX_EDGES || solid.face_iter().count() > MAX_FACES {
        return Err(unsupported(
            "The round exceeds the native constructor's edge or face work limit",
        ));
    }
    let planes = match crate::planar::supporting_planes(solid) {
        Ok(planes) => planes,
        Err(CadError::Unsupported(_)) => return Ok(None),
        Err(error) => return Err(error),
    };
    if planes.len() < 4 {
        return Ok(None);
    }
    let planes = planes
        .into_iter()
        .map(|plane| (plane.face.id(), plane))
        .collect::<HashMap<_, _>>();
    let mut edges = HashMap::new();
    let mut adjacency: HashMap<EdgeId, Vec<FaceId>> = HashMap::new();
    let mut corners: HashMap<VertexId, Corner> = HashMap::new();
    let selected = selected.iter().copied().collect::<HashSet<_>>();
    for face in solid.face_iter() {
        if !planes.contains_key(&face.id()) {
            continue;
        }
        for edge in &face.boundaries()[0] {
            edges.entry(edge.id()).or_insert_with(|| edge.clone());
            adjacency.entry(edge.id()).or_default().push(face.id());
            for vertex in [edge.front(), edge.back()] {
                let corner = corners.entry(vertex.id()).or_insert_with(|| Corner {
                    vertex: vertex.clone(),
                    edges: vec![],
                    faces: vec![],
                    selected: vec![],
                    center: None,
                    points: HashMap::new(),
                    mitre: None,
                });
                if !corner.edges.contains(&edge.id()) {
                    corner.edges.push(edge.id());
                }
                if !corner.faces.contains(&face.id()) {
                    corner.faces.push(face.id());
                }
                if selected.contains(&edge.id()) && !corner.selected.contains(&edge.id()) {
                    corner.selected.push(edge.id());
                }
            }
        }
    }
    if selected.iter().any(|id| !edges.contains_key(id)) {
        return Ok(None);
    }
    if adjacency.values().any(|faces| faces.len() != 2) {
        return Ok(None);
    }
    // Retain holes and pockets only when positive control hulls prove that
    // their complete carriers miss every selected edge's removal slabs. The
    // wall-specific slab depths below come from all positive strip controls
    // and complete corner-ball bounds. Axial separation is deliberately not
    // assumed, preferring a refusal to an intersecting pocket.
    let mut protected = Vec::new();
    for face in solid.face_iter() {
        if planes.contains_key(&face.id()) {
            for wire in face.boundaries().iter().skip(1) {
                let mut hull = Vec::new();
                for edge in wire {
                    match crate::extrusion::curve_hull(&edge.curve()) {
                        Ok(points) => hull.extend(points),
                        Err(CadError::Unsupported(_)) => return Ok(None),
                        Err(error) => return Err(error),
                    }
                }
                protected.push(hull);
            }
        } else {
            match crate::extrusion::face_hull(face) {
                Ok(hull) => protected.push(hull),
                Err(CadError::Unsupported(_)) => return Ok(None),
                Err(error) => return Err(error),
            }
        }
    }
    let protected_controls = protected.iter().map(Vec::len).sum::<usize>();
    if protected_controls > MAX_PROTECTED_CONTROLS
        || protected_controls.saturating_mul(selected.len()) > MAX_SEPARATION_WORK
    {
        return Err(unsupported(
            "The pocket-separation proof exceeds the native control or work budget",
        ));
    }
    for corner in corners
        .values_mut()
        .filter(|corner| !corner.selected.is_empty())
    {
        if corner.edges.len() != 3 || corner.faces.len() != 3 {
            return Err(unsupported(
                "Selected corners currently require three incident stock edges and walls",
            ));
        }
        if corner.selected.len() == 2 {
            let common = corner
                .faces
                .iter()
                .find(|face| {
                    corner
                        .selected
                        .iter()
                        .all(|edge| adjacency[edge].contains(face))
                })
                .unwrap();
            if corner
                .faces
                .iter()
                .filter(|face| *face != common)
                .any(|face| planes[face].normal.dot(planes[common].normal).abs() > 1e-9)
            {
                return Err(unsupported(
                    "Two-edge corners require the common wall perpendicular to the remaining walls; this covers box and right-prism rim chains",
                ));
            }
        }
        if corner.selected.len() >= 2 {
            corner.center = Some(plane_center(corner, &planes, radius)?);
        }
        if corner.selected.len() == 3 {
            let center = corner.center.unwrap();
            // The complete corner ball is behind every nonincident stock
            // plane. This is a conservative analytical bound on its trimmed
            // spherical triangle, not a sampled containment guess.
            if planes
                .values()
                .filter(|plane| !corner.faces.contains(&plane.face.id()))
                .any(|plane| (center - plane.origin).dot(plane.normal) + radius > EPS)
            {
                return Err(invalid(
                    "Radius makes a spherical corner reach another stock wall; reduce the radius",
                ));
            }
        }
    }
    // Sort by authored source closure, rather than transient kernel address, so
    // both the shared mitre and topology construction remain deterministic.
    let mut selected_order = selected
        .iter()
        .copied()
        .map(|id| {
            Ok((
                crate::shell_history::closure(&[history.edge(id)?])?.to_string(),
                id,
            ))
        })
        .collect::<Result<Vec<_>>>()?;
    selected_order.sort_by(|a, b| a.0.cmp(&b.0));
    let selected_order = selected_order
        .into_iter()
        .map(|(_, id)| id)
        .collect::<Vec<_>>();
    let mut blends = HashMap::new();
    for id in &selected_order {
        let source = &edges[id];
        let adjacent = &adjacency[id];
        let faces = [adjacent[0], adjacent[1]];
        let normals = [planes[&faces[0]].normal, planes[&faces[1]].normal];
        let cosine = normals[0].dot(normals[1]);
        if cosine <= -1. + 1e-9 || cosine >= 1. - 1e-9 {
            return Err(unsupported(
                "The selected edge does not have a nondegenerate convex material wedge",
            ));
        }
        let axis = (source.back().point() - source.front().point()).normalize();
        let origin = source.front().point() - (normals[0] + normals[1]) * (radius / (1. + cosine));
        let first = end_profile(
            corners.get_mut(&source.front().id()).unwrap(),
            source,
            faces,
            axis,
            origin,
            normals,
            radius,
            &planes,
            &edges,
            &adjacency,
            history,
        )?;
        let last = end_profile(
            corners.get_mut(&source.back().id()).unwrap(),
            source,
            faces,
            axis,
            origin,
            normals,
            radius,
            &planes,
            &edges,
            &adjacency,
            history,
        )?;
        for index in 0..3 {
            if (point(last.controls[index]) - point(first.controls[index])).dot(axis) <= EPS {
                return Err(invalid(
                    "Radius consumes a selected edge or makes adjoining corner trims overlap; reduce the radius",
                ));
            }
        }
        for (end, vertex) in [(&first, source.front()), (&last, source.back())] {
            let corner = &corners[&vertex.id()];
            for plane in planes.values() {
                if corner.selected.len() == 3 {
                    // A rational circular arc's middle control can lie outside
                    // an oblique stock plane even when the entire
                    // actual circle is inside it. Its exact corner ball proves
                    // that containment instead; both true end curves are then
                    // safe, and their ruled cylinder lies in the halfspace.
                    if plane.signed(corner.center.unwrap()) + radius > EPS {
                        return Err(invalid("The corner ball does not fit its stock walls"));
                    }
                } else if end
                    .controls
                    .iter()
                    .any(|control| plane.signed(point(*control)) > EPS)
                {
                    return Err(invalid(
                        "Radius reaches another stock wall; reduce the radius",
                    ));
                }
            }
        }
        let rails = [
            builder::line(first.arc.front(), last.arc.front()),
            builder::line(first.arc.back(), last.arc.back()),
        ];
        for (index, rail) in rails.iter().enumerate() {
            let sources = [history.edge(*id)?, history.face(faces[index])?];
            history.construct_edge(
                rail.id(),
                &sources,
                serde_json::json!({"operation":"fillet","constructor":"tangent_rail"}),
            )?;
        }
        blends.insert(
            *id,
            Blend {
                source: source.clone(),
                faces,
                axis,
                origin,
                ends: [first, last],
                rails,
            },
        );
    }
    for hull in &protected {
        for blend in blends.values() {
            let safe = blend.faces.iter().any(|face| {
                let plane = &planes[face];
                let mut depth = blend
                    .ends
                    .iter()
                    .flat_map(|end| end.controls.iter())
                    .map(|control| -plane.signed(point(*control)))
                    .fold(0_f64, f64::max);
                // The removed wedge is bounded by the original edge and its
                // complete positive-control strip. At fully rounded vertices
                // also include the complete analytic sphere, including its
                // interior beyond the boundary circles.
                for vertex in [blend.source.front(), blend.source.back()] {
                    depth = depth.max(-plane.signed(vertex.point()));
                    let corner = &corners[&vertex.id()];
                    if corner.selected.len() == 3 {
                        depth = depth.max(-plane.signed(corner.center.unwrap()) + radius);
                    }
                }
                hull.iter().all(|point| plane.signed(*point) < -depth - EPS)
            });
            if !safe {
                return Err(unsupported(
                    "A hole or pocket is too close to the selected round for the native disjoint-carrier proof",
                ));
            }
        }
    }
    let mut trimmed = HashMap::new();
    for (id, edge) in edges.iter().filter(|(id, _)| !selected.contains(id)) {
        let endpoint = |vertex: &Vertex| -> Result<Vertex> {
            let corner = &corners[&vertex.id()];
            if corner.selected.is_empty() {
                return Ok(vertex.clone());
            }
            corner
                .points
                .get(&PointRole::Edge(*id))
                .cloned()
                .ok_or_else(|| invalid("A neighbouring sharp edge has no rounded terminal vertex"))
        };
        let first = endpoint(edge.front())?;
        let last = endpoint(edge.back())?;
        let delta = edge.back().point() - edge.front().point();
        let length = delta.magnitude2();
        let t0 = (first.point() - edge.front().point()).dot(delta) / length;
        let t1 = (last.point() - edge.front().point()).dot(delta) / length;
        if t0 < -1e-9
            || t1 > 1. + 1e-9
            || t1 - t0 <= EPS / length.sqrt()
            || (first.point() - edge.front().point() - delta * t0).magnitude() > EPS
            || (last.point() - edge.front().point() - delta * t1).magnitude() > EPS
        {
            return Err(invalid(
                "Radius consumes a neighbouring sharp edge or makes corner trims overlap; reduce the radius",
            ));
        }
        let line = if first.id() == edge.front().id() && last.id() == edge.back().id() {
            edge.clone()
        } else {
            builder::line(&first, &last)
        };
        history.retain_edge(line.id(), *id)?;
        trimmed.insert(*id, line);
    }
    let mut faces = Vec::new();
    for face in solid.face_iter() {
        if !planes.contains_key(&face.id()) {
            faces.push(face.clone());
            continue;
        }
        let source_wire = &face.boundaries()[0];
        let mut segments = Vec::new();
        for edge in source_wire {
            let new = if let Some(blend) = blends.get(&edge.id()) {
                let side = blend.faces.iter().position(|id| *id == face.id()).unwrap();
                if edge.front().id() == blend.source.front().id() {
                    blend.rails[side].clone()
                } else {
                    blend.rails[side].inverse()
                }
            } else {
                let line = &trimmed[&edge.id()];
                if edge.front().id() == edges[&edge.id()].front().id() {
                    line.clone()
                } else {
                    line.inverse()
                }
            };
            segments.push(new);
        }
        let mut wire = Wire::new();
        for index in 0..segments.len() {
            let current = &segments[index];
            let next = &segments[(index + 1) % segments.len()];
            wire.push_back(current.clone());
            if current.back().id() != next.front().id() {
                let vertex = source_wire[index].back();
                let corner = &corners[&vertex.id()];
                if corner.selected.len() != 1
                    || corner
                        .selected
                        .iter()
                        .any(|edge| adjacency[edge].contains(&face.id()))
                {
                    return Err(invalid(
                        "A rounded planar boundary has an unfilled corner gap",
                    ));
                }
                let blend = &blends[&corner.selected[0]];
                let end = if blend.source.front().id() == vertex.id() {
                    &blend.ends[0]
                } else {
                    &blend.ends[1]
                };
                wire.push_back(oriented(&end.arc, current.back(), next.front())?);
            }
        }
        let mut wires = vec![wire];
        wires.extend(face.boundaries().into_iter().skip(1));
        let output = Face::try_new(wires, face.oriented_surface())
            .map_err(|error| invalid(&error.to_string()))?;
        history.retain_face(output.id(), face.id())?;
        faces.push(output);
    }
    for id in &selected_order {
        let blend = &blends[id];
        let net = (0..3)
            .map(|index| vec![blend.ends[0].controls[index], blend.ends[1].controls[index]])
            .collect::<Vec<_>>();
        let surface = NurbsSurface::new(BsplineSurface::new(
            (KnotVector::bezier_knot(2), KnotVector::bezier_knot(1)),
            net,
        ));
        let boundary = Wire::from(vec![
            blend.ends[0].arc.clone(),
            blend.rails[1].clone(),
            blend.ends[1].arc.inverse(),
            blend.rails[0].inverse(),
        ]);
        let radial_point = surface.subs(0.5, 0.5);
        let along = (radial_point - blend.origin).dot(blend.axis);
        let radial = radial_point - (blend.origin + blend.axis * along);
        let mut output = Face::try_new(vec![boundary], Surface::NurbsSurface(surface.clone()))
            .map_err(|error| invalid(&error.to_string()))?;
        if surface.normal(0.5, 0.5).dot(radial) < 0. {
            output.invert();
        }
        let sources = [
            history.edge(blend.source.id())?,
            history.face(blend.faces[0])?,
            history.face(blend.faces[1])?,
        ];
        history.construct_face(
            output.id(),
            &sources,
            serde_json::json!({"operation":"fillet","constructor":"circular_cylinder"}),
        )?;
        faces.push(output);
    }
    let mut corner_order = corners
        .values()
        .filter(|corner| corner.selected.len() == 3)
        .map(|corner| {
            let sources = corner
                .edges
                .iter()
                .map(|edge| history.edge(*edge))
                .collect::<Result<Vec<_>>>()?;
            Ok((
                crate::shell_history::closure(&sources)?.to_string(),
                corner.vertex.id(),
            ))
        })
        .collect::<Result<Vec<_>>>()?;
    corner_order.sort_by(|a, b| a.0.cmp(&b.0));
    for (_, corner_id) in corner_order {
        let corner = &corners[&corner_id];
        let center = corner.center.unwrap();
        let mut arcs = corner
            .selected
            .iter()
            .map(|edge| {
                let blend = &blends[edge];
                if blend.source.front().id() == corner.vertex.id() {
                    blend.ends[0].arc.clone()
                } else {
                    blend.ends[1].arc.clone()
                }
            })
            .collect::<Vec<_>>();
        let mut wire = Wire::from(vec![arcs.remove(0)]);
        while !arcs.is_empty() {
            let last = wire.back().unwrap().back().id();
            let index = arcs
                .iter()
                .position(|arc| arc.front().id() == last || arc.back().id() == last)
                .ok_or_else(|| invalid("The spherical corner cannot join its cylinder arcs"))?;
            let arc = arcs.remove(index);
            wire.push_back(if arc.front().id() == last {
                arc
            } else {
                arc.inverse()
            });
        }
        if wire.front().unwrap().front().id() != wire.back().unwrap().back().id() {
            return Err(invalid("The spherical corner boundary is open"));
        }
        let normal0 = planes[&corner.faces[0]].normal;
        let normal1 = planes[&corner.faces[1]].normal;
        let z = (normal0 - normal1).normalize();
        // The common offset displacement has equal positive dot products
        // with all three normals and is perpendicular to n0 - n1. Point the
        // periodic parameter seam away from the entire normal cone; neither
        // pole belongs to that cone because their authored coefficients have
        // opposite signs on n0/n1.
        let outward = (corner.vertex.point() - center).normalize();
        let x = (-outward + z * outward.dot(z)).normalize();
        let y = z.cross(x).normalize();
        let frame = Matrix4::from_cols(
            x.extend(0.),
            y.extend(0.),
            z.extend(0.),
            center.to_vec().extend(1.),
        );
        let mut sphere = Processor::new(Sphere::new(Point3::origin(), radius));
        sphere.transform_by(frame);
        let surface = Surface::SphericalSurface(sphere);
        let a = wire[0].front().point();
        let b = wire[0].back().point();
        let c = wire[1].back().point();
        // A right-handed analytic sphere has outward carrier normals. Match the
        // actual boundary winding rather than guessing from kernel edge order.
        if (b - a).cross(c - b).dot(a - center) < 0. {
            wire.invert();
        }
        let output =
            Face::try_new(vec![wire], surface).map_err(|error| invalid(&error.to_string()))?;
        let mut sources = corner
            .edges
            .iter()
            .map(|edge| history.edge(*edge))
            .collect::<Result<Vec<_>>>()?;
        sources.extend(
            corner
                .faces
                .iter()
                .map(|face| history.face(*face))
                .collect::<Result<Vec<_>>>()?,
        );
        history.construct_face(
            output.id(),
            &sources,
            serde_json::json!({"operation":"fillet","constructor":"spherical_corner"}),
        )?;
        faces.push(output);
    }
    let output = Solid::try_new(vec![faces.into()]).map_err(|error| invalid(&error.to_string()))?;
    crate::validation::solid(&output)?;
    Ok(Some(output))
}
