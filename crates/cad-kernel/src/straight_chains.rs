//! Exact topology normalization for selected collinear planar edge chains.
//!
//! Authored sketch segments and Boolean partitions can divide one physical
//! wedge into several coplanar faces. Remove only shared native planar seams,
//! then combine selected collinear edges at valence-two vertices. The native
//! treatment still constructs and validates the complete resulting corner.
use super::{EPS, invalid, preserve_surface, unsupported};
use crate::{planar::BoundaryPlane, shell_history::ShellHistory};
use cad_core::Result;
use monstertruck::modeling::*;
use std::collections::{HashMap, HashSet};

const MAX_WORK: usize = 4_000_000;
const MAX_EDGES: usize = 65_536;

fn charge(work: &mut usize, n: usize) -> Result<()> {
    *work = work.saturating_add(n);
    if *work > MAX_WORK {
        return Err(unsupported(
            "Straight-chain normalization exceeds its work budget",
        ));
    }
    Ok(())
}
fn absolute(edge: &Edge) -> Edge {
    if edge.orientation() {
        edge.clone()
    } else {
        edge.inverse()
    }
}
fn direction_from(edge: &Edge, vertex: VertexId) -> Vector3 {
    if edge.front().id() == vertex {
        edge.back().point() - edge.front().point()
    } else {
        edge.front().point() - edge.back().point()
    }
}

pub(super) struct Prepared {
    pub solid: Solid,
    pub selected: Vec<EdgeId>,
    pub first_face: Option<FaceId>,
}

/// Joining one connected set of native planar faces by deleting paired,
/// oppositely oriented seam occurrences preserves its domain. This constructor
/// admits a single resulting wire; multiply-connected planar unions remain for
/// the general arrangement solver and are never reordered by sampled areas.
fn merge_planar(
    members: &[FaceId],
    faces: &HashMap<FaceId, Face>,
    anchor: &BoundaryPlane,
    selected: &HashSet<EdgeId>,
    history: &mut ShellHistory,
    work: &mut usize,
) -> Result<Option<Face>> {
    let mut occurrences: HashMap<EdgeId, Vec<Edge>> = HashMap::new();
    for id in members {
        for edge in faces[id].edge_iter() {
            charge(work, 1)?;
            occurrences.entry(edge.id()).or_default().push(edge);
        }
    }
    let mut boundary = Vec::new();
    for (id, edges) in occurrences {
        match edges.as_slice() {
            [edge] => boundary.push(edge.clone()),
            [a, b] if a.orientation() != b.orientation() && !selected.contains(&id) => {}
            _ => return Ok(None),
        }
    }
    if boundary.is_empty() || boundary.len() > MAX_EDGES {
        return Err(unsupported(
            "A planar straight-chain support has excessive or absent boundaries",
        ));
    }
    let mut ordered = boundary
        .iter()
        .map(|edge| {
            Ok((
                crate::shell_history::closure(&[history.edge(edge.id())?])?.to_string(),
                edge.front().id(),
            ))
        })
        .collect::<Result<Vec<_>>>()?;
    ordered.sort_by(|a, b| a.0.cmp(&b.0));
    if ordered.windows(2).any(|pair| pair[0].0 == pair[1].0) {
        return Err(unsupported(
            "Planar chain boundaries have ambiguous source ancestry",
        ));
    }
    let mut outgoing = HashMap::new();
    let mut incoming = HashSet::new();
    for edge in boundary {
        if !incoming.insert(edge.back().id()) || outgoing.insert(edge.front().id(), edge).is_some()
        {
            return Ok(None);
        }
    }
    if incoming.iter().any(|id| !outgoing.contains_key(id)) {
        return Ok(None);
    }
    let start = ordered[0].1;
    let mut cursor = start;
    let mut visited = HashSet::new();
    let mut wire = Wire::new();
    loop {
        charge(work, 1)?;
        let Some(edge) = outgoing.get(&cursor) else {
            return Ok(None);
        };
        if !visited.insert(edge.id()) {
            return Ok(None);
        }
        wire.push_back(edge.clone());
        cursor = edge.back().id();
        if cursor == start {
            break;
        }
    }
    if visited.len() != outgoing.len() {
        return Ok(None);
    }
    let seed = if anchor.normal.x.abs() < 0.8 {
        Vector3::unit_x()
    } else {
        Vector3::unit_y()
    };
    let u = anchor.normal.cross(seed).normalize();
    let v = anchor.normal.cross(u).normalize();
    let scale = wire
        .vertex_iter()
        .map(|vertex| vertex.point().distance(anchor.origin))
        .fold(1., f64::max);
    let plane = Plane::new(
        anchor.origin,
        anchor.origin + u * scale,
        anchor.origin + v * scale,
    );
    if !plane.normal().magnitude2().is_finite() || plane.normal().dot(anchor.normal) < 1. - 1e-10 {
        return Err(unsupported(
            "A merged planar support has no stable native parameter frame",
        ));
    }
    for id in members {
        let bounds = crate::native_bounds::face(&faces[id])?;
        charge(work, bounds.len())?;
        let (low, high) = bounds.projection(plane.origin(), plane.normal())?;
        if low < -EPS || high > EPS {
            return Err(unsupported(
                "The merged support plane does not contain all source patches",
            ));
        }
    }
    let result = Face::try_new(vec![wire], Surface::Plane(plane))
        .map_err(|_| unsupported("Planar chain seam removal does not preserve its boundary"))?;
    crate::validation::surface(&Shell::from(vec![result.clone()]))?;
    let sources = members
        .iter()
        .map(|id| history.face(*id))
        .collect::<Result<Vec<_>>>()?;
    history.construct_face(
        result.id(),
        &sources,
        serde_json::json!({"constructor":"coalesced_planar_chain_support"}),
    )?;
    Ok(Some(result))
}

fn coalesce(
    solid: &Solid,
    selected: &HashSet<EdgeId>,
    history: &mut ShellHistory,
    work: &mut usize,
) -> Result<(Solid, HashMap<FaceId, FaceId>)> {
    let faces = solid
        .face_iter()
        .map(|face| (face.id(), face.clone()))
        .collect::<HashMap<_, _>>();
    let mut adjacency: HashMap<EdgeId, Vec<FaceId>> = HashMap::new();
    let mut seeds = HashSet::new();
    for face in solid.face_iter() {
        for edge in face.edge_iter() {
            charge(work, 1)?;
            adjacency.entry(edge.id()).or_default().push(face.id());
            if selected.contains(&edge.id()) {
                seeds.insert(face.id());
            }
        }
    }
    if adjacency.len() > MAX_EDGES || adjacency.values().any(|faces| faces.len() != 2) {
        return Err(unsupported(
            "Straight-chain normalization requires bounded manifold edge incidence",
        ));
    }
    let mut seeds = seeds
        .into_iter()
        .map(|id| {
            Ok((
                crate::shell_history::closure(&[history.face(id)?])?.to_string(),
                id,
            ))
        })
        .collect::<Result<Vec<_>>>()?;
    seeds.sort_by(|a, b| a.0.cmp(&b.0));
    let mut planes = HashMap::new();
    let mut assigned = HashSet::new();
    let mut mapping = HashMap::new();
    let mut replacements = HashMap::new();
    for (_, seed) in seeds {
        if assigned.contains(&seed) {
            continue;
        }
        if let std::collections::hash_map::Entry::Vacant(entry) = planes.entry(seed) {
            entry.insert(crate::planar::boundary_plane(&faces[&seed])?);
        }
        let Some(anchor) = planes[&seed].clone() else {
            continue;
        };
        let mut pending = vec![seed];
        let mut members = Vec::new();
        while let Some(id) = pending.pop() {
            if !assigned.insert(id) {
                continue;
            }
            members.push(id);
            for edge in faces[&id].edge_iter() {
                if selected.contains(&edge.id()) {
                    continue;
                }
                for neighbor in &adjacency[&edge.id()] {
                    charge(work, 1)?;
                    if assigned.contains(neighbor) {
                        continue;
                    }
                    if let std::collections::hash_map::Entry::Vacant(entry) =
                        planes.entry(*neighbor)
                    {
                        entry.insert(crate::planar::boundary_plane(&faces[neighbor])?);
                    }
                    let Some(plane) = &planes[neighbor] else {
                        continue;
                    };
                    if plane.normal.dot(anchor.normal) < 1. - 1e-10 {
                        continue;
                    }
                    let bounds = crate::native_bounds::face(&faces[neighbor])?;
                    charge(work, bounds.len())?;
                    let (low, high) = bounds.projection(anchor.origin, anchor.normal)?;
                    if low >= -EPS && high <= EPS {
                        pending.push(*neighbor);
                    }
                }
            }
        }
        if members.len() > 1
            && let Some(merged) = merge_planar(&members, &faces, &anchor, selected, history, work)?
        {
            for id in members {
                mapping.insert(id, merged.id());
            }
            replacements.insert(merged.id(), merged);
        }
    }
    let mut emitted = HashSet::new();
    let mut shells = Vec::new();
    for shell in solid.boundaries() {
        let mut output = Shell::new();
        for face in shell {
            if let Some(id) = mapping.get(&face.id()) {
                if emitted.insert(*id) {
                    output.push(replacements[id].clone());
                }
            } else {
                output.push(face.clone());
            }
        }
        shells.push(output);
    }
    let result = Solid::try_new(shells)
        .map_err(|_| unsupported("Planar chain seam removal opens the source body"))?;
    crate::validation::solid(&result)?;
    Ok((result, mapping))
}

pub(super) fn prepare(
    solid: &Solid,
    selected: &[EdgeId],
    first_face: Option<FaceId>,
    history: &mut ShellHistory,
) -> Result<Option<Prepared>> {
    if selected.len() < 2 {
        return Ok(None);
    }
    let selected_set = selected.iter().copied().collect::<HashSet<_>>();
    let sources = solid
        .edge_iter()
        .filter(|edge| selected_set.contains(&edge.id()))
        .map(|edge| (edge.id(), absolute(&edge)))
        .collect::<HashMap<_, _>>();
    if sources.len() != selected_set.len() {
        return Ok(None);
    }
    let mut shared: HashMap<VertexId, Vec<EdgeId>> = HashMap::new();
    for edge in sources.values() {
        if !crate::planar::straight_edge(edge)? {
            return Ok(None);
        }
        for vertex in [edge.front(), edge.back()] {
            shared.entry(vertex.id()).or_default().push(edge.id());
        }
    }
    if !shared.iter().any(|(vertex, edges)| {
        let [a, b] = edges.as_slice() else {
            return false;
        };
        direction_from(&sources[a], *vertex)
            .normalize()
            .dot(direction_from(&sources[b], *vertex).normalize())
            < -1. + 1e-10
    }) {
        return Ok(None);
    }
    let mut work = 0;
    let (solid, mapping) = coalesce(solid, &selected_set, history, &mut work)?;
    let mut first_face = first_face.map(|id| mapping.get(&id).copied().unwrap_or(id));
    let mut incident: HashMap<VertexId, HashSet<EdgeId>> = HashMap::new();
    let mut adjacency: HashMap<EdgeId, HashSet<FaceId>> = HashMap::new();
    for face in solid.face_iter() {
        for edge in face.edge_iter() {
            charge(&mut work, 1)?;
            adjacency.entry(edge.id()).or_default().insert(face.id());
            for vertex in [edge.front(), edge.back()] {
                incident.entry(vertex.id()).or_default().insert(edge.id());
            }
        }
    }
    let mut continuation: HashMap<(EdgeId, VertexId), EdgeId> = HashMap::new();
    for (vertex, neighbors) in &incident {
        if neighbors.len() != 2 {
            continue;
        }
        let neighbors = neighbors.iter().copied().collect::<Vec<_>>();
        let (a, b) = (neighbors[0], neighbors[1]);
        if !selected_set.contains(&a)
            || !selected_set.contains(&b)
            || adjacency[&a].len() != 2
            || adjacency[&a] != adjacency[&b]
        {
            continue;
        }
        if direction_from(&sources[&a], *vertex)
            .normalize()
            .dot(direction_from(&sources[&b], *vertex).normalize())
            < -1. + 1e-10
        {
            continuation.insert((a, *vertex), b);
            continuation.insert((b, *vertex), a);
        }
    }
    let mut visited = HashSet::new();
    let mut remapped = HashMap::new();
    let mut replacements = HashMap::new();
    let mut output_selection = Vec::new();
    for id in selected {
        if visited.contains(id) {
            continue;
        }
        let mut members = HashSet::new();
        let mut pending = vec![*id];
        while let Some(id) = pending.pop() {
            charge(&mut work, 1)?;
            if !visited.insert(id) {
                continue;
            }
            members.insert(id);
            for vertex in [sources[&id].front(), sources[&id].back()] {
                if let Some(next) = continuation.get(&(id, vertex.id())) {
                    pending.push(*next);
                }
            }
        }
        if members.len() == 1 {
            output_selection.push(*id);
            continue;
        }
        let ends = members
            .iter()
            .flat_map(|id| {
                [sources[id].front(), sources[id].back()]
                    .into_iter()
                    .filter(|vertex| !continuation.contains_key(&(*id, vertex.id())))
            })
            .cloned()
            .collect::<Vec<_>>();
        let [a, b] = ends.as_slice() else {
            return Err(unsupported(
                "A selected straight chain is closed or has ambiguous end incidence",
            ));
        };
        let source_direction = sources[id].back().point() - sources[id].front().point();
        let (a, b) = if (b.point() - a.point()).dot(source_direction) > 0. {
            (a, b)
        } else {
            (b, a)
        };
        let delta = b.point() - a.point();
        let length = delta.magnitude();
        if !length.is_finite() || length <= EPS {
            return Err(invalid("The selected straight chain is collapsed"));
        }
        let direction = delta / length;
        let seed = if direction.x.abs() < 0.8 {
            Vector3::unit_x()
        } else {
            Vector3::unit_y()
        };
        let u = direction.cross(seed).normalize();
        let v = direction.cross(u).normalize();
        for member in &members {
            let edge = &sources[member];
            let bounds = crate::native_bounds::curve(&edge.curve())?;
            charge(&mut work, bounds.len())?;
            let (low, high) = bounds.projection(a.point(), direction)?;
            if low < -EPS || high > length + EPS {
                return Err(unsupported(
                    "A selected chain leaves its combined bounded interval",
                ));
            }
            for perpendicular in [u, v] {
                let (low, high) = bounds.projection(a.point(), perpendicular)?;
                if low < -EPS * 0.5 || high > EPS * 0.5 {
                    return Err(unsupported(
                        "Selected chain members do not share one bounded straight carrier",
                    ));
                }
            }
        }
        let replacement = builder::line(a, b);
        let origins = members
            .iter()
            .map(|id| history.edge(*id))
            .collect::<Result<Vec<_>>>()?;
        history.construct_edge(
            replacement.id(),
            &origins,
            serde_json::json!({"constructor":"coalesced_selected_straight_chain"}),
        )?;
        for member in members {
            remapped.insert(member, replacement.id());
        }
        output_selection.push(replacement.id());
        replacements.insert(replacement.id(), replacement);
    }
    if replacements.is_empty() {
        return Ok(None);
    }
    let mut shells = Vec::new();
    for shell in solid.boundaries() {
        let mut output = Shell::new();
        for face in shell {
            if !face
                .edge_iter()
                .any(|edge| remapped.contains_key(&edge.id()))
            {
                output.push(face.clone());
                continue;
            }
            let mut wires = Vec::new();
            for wire in face.boundaries() {
                let mut merged = Wire::new();
                for (index, edge) in wire.iter().enumerate() {
                    let Some(id) = remapped.get(&edge.id()) else {
                        merged.push_back(edge.clone());
                        continue;
                    };
                    if remapped.get(&wire[(index + wire.len() - 1) % wire.len()].id()) == Some(id) {
                        continue;
                    }
                    let replacement = &replacements[id];
                    let oriented = if edge.front().id() == replacement.front().id() {
                        replacement.clone()
                    } else if edge.front().id() == replacement.back().id() {
                        replacement.inverse()
                    } else {
                        return Err(invalid(
                            "A straight chain has no matching native endpoint on its wall",
                        ));
                    };
                    merged.push_back(oriented);
                }
                wires.push(merged);
            }
            let replacement = preserve_surface(face, wires).map_err(|_| {
                unsupported("Straight-chain boundary joining leaves its native wall")
            })?;
            history.retain_face(replacement.id(), face.id())?;
            if first_face == Some(face.id()) {
                // Keep the final native face identity for explicit Distance 1.
                first_face = Some(replacement.id());
            }
            output.push(replacement);
        }
        shells.push(output);
    }
    let result = Solid::try_new(shells)
        .map_err(|_| unsupported("Straight-chain joining does not preserve a closed body"))?;
    crate::validation::solid(&result)?;
    Ok(Some(Prepared {
        solid: result,
        selected: output_selection,
        first_face,
    }))
}
