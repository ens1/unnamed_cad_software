//! Join native planar sectors before treating complete or radial-ended rims.
//! Only existing shared edges are removed. Boundary edges, vertices and their
//! authored identities survive; no coordinate welding or fitted wire is used.
use crate::{native_bounds::Enclosure, shell_history::ShellHistory};
use cad_core::{CadError, Result};
use monstertruck::modeling::*;
use std::collections::{HashMap, HashSet};

const EPS: f64 = 1e-7;
const MAX_FACES: usize = 4096;
const MAX_EDGES: usize = 65_536;
const MAX_WORK: usize = 4_000_000;

fn unsupported(message: &str) -> CadError {
    CadError::Unsupported(format!("Circular cap coalescing: {message}"))
}
fn charge(work: &mut usize, count: usize) -> Result<()> {
    *work = work.saturating_add(count);
    if *work > MAX_WORK {
        return Err(unsupported("Native cap proof exceeds its work budget"));
    }
    Ok(())
}

fn circle_budget(curve: &Curve, work: &mut usize) -> Result<()> {
    let (controls, knots) = match curve {
        Curve::BsplineCurve(curve) => (curve.control_points().len(), curve.knot_vector().len()),
        Curve::NurbsCurve(curve) => (curve.control_points().len(), curve.knot_vector().len()),
        _ => return charge(work, 1),
    };
    let degree = knots
        .checked_sub(controls.saturating_add(1))
        .ok_or_else(|| unsupported("A native circular boundary has inconsistent knot counts"))?;
    // Bernstein recognition may repeatedly partition the knot vector and
    // combine polynomial coefficients; charge before cloning the carrier.
    charge(
        work,
        controls
            .saturating_mul(controls)
            .saturating_mul(degree.saturating_add(1)),
    )
}

struct Planar {
    origin: Point3,
    normal: Vector3,
    bounds: Enclosure,
}
fn planar(face: &Face, work: &mut usize) -> Result<Option<Planar>> {
    let surface = face.oriented_surface();
    let normal = match &surface {
        Surface::Plane(plane) => plane.normal(),
        Surface::BsplineSurface(surface) => {
            let ((a, b), (c, d)) = surface.range_tuple();
            surface.normal(a * 0.5 + b * 0.5, c * 0.5 + d * 0.5)
        }
        Surface::NurbsSurface(surface) => {
            let ((a, b), (c, d)) = surface.range_tuple();
            surface.normal(a * 0.5 + b * 0.5, c * 0.5 + d * 0.5)
        }
        Surface::RevolutionSurface(processor)
            if matches!(processor.entity().entity_curve(), Curve::Line(_)) =>
        {
            // The native derivative proposes orientation only. The complete
            // analytic face support below must certify the plane independently.
            surface.normal(0.5, 0.5)
        }
        _ => return Ok(None),
    };
    if !normal.magnitude2().is_finite() || normal.magnitude2() <= 1e-18 {
        return Ok(None);
    }
    let Some(vertex) = face.vertex_iter().next() else {
        return Ok(None);
    };
    let bounds = match crate::native_bounds::face(face) {
        Ok(bounds) => bounds,
        Err(CadError::Unsupported(_)) => return Ok(None),
        Err(error) => return Err(error),
    };
    charge(work, bounds.len())?;
    let result = Planar {
        origin: vertex.point(),
        normal: normal.normalize(),
        bounds,
    };
    let (low, high) = result.bounds.projection(result.origin, result.normal)?;
    Ok((low >= -EPS && high <= EPS).then_some(result))
}
fn coplanar(candidate: &Planar, plane: &Planar, work: &mut usize) -> Result<bool> {
    if candidate.normal.dot(plane.normal) < 1. - 1e-10 {
        return Ok(false);
    }
    charge(work, candidate.bounds.len())?;
    let (low, high) = candidate.bounds.projection(plane.origin, plane.normal)?;
    Ok(low >= -EPS && high <= EPS)
}

pub(super) struct Coalesced {
    pub solid: Solid,
    pub faces: HashMap<FaceId, FaceId>,
}

/// A complete circle or an exact concentric sector has an unambiguous
/// orientation from its outer circular run. Native edge incidence, monotone
/// arcs, radial lines and the original validated sectors establish its bounded
/// perimeter; no polygonal approximation supplies winding or containment.
fn outer_boundary(wire: &Wire, normal: Vector3, work: &mut usize) -> Result<bool> {
    let mut circles = Vec::new();
    let mut straight = Vec::new();
    for (index, edge) in wire.iter().enumerate() {
        circle_budget(&edge.curve(), work)?;
        match crate::bounded_carriers::circle(&edge.oriented_curve()) {
            Ok(((center, axis, radius), sweep)) => {
                if axis.dot(normal).abs() < 1. - 1e-10 {
                    return Err(unsupported("A cap circle leaves its supporting plane"));
                }
                circles.push((index, center, radius, sweep * axis.dot(normal)));
            }
            Err(CadError::Unsupported(_)) => {
                if !crate::planar::straight_edge(edge)? {
                    return Err(unsupported(
                        "Merged turned caps require circles or exact radial sector boundaries",
                    ));
                }
                straight.push(edge);
            }
            Err(error) => return Err(error),
        }
    }
    let Some((_, center, _, _)) = circles.first().copied() else {
        return Err(unsupported("A merged turned cap has no circular boundary"));
    };
    if circles
        .iter()
        .any(|(_, other, _, _)| center.distance(*other) > EPS)
    {
        return Err(unsupported(
            "A merged sector requires concentric circular boundaries",
        ));
    }
    let outer_radius = circles
        .iter()
        .map(|(_, _, radius, _)| *radius)
        .fold(0., f64::max);
    let inner_radius = circles
        .iter()
        .map(|(_, _, radius, _)| *radius)
        .fold(f64::INFINITY, f64::min);
    let annular = outer_radius - inner_radius > EPS;
    let mut classes = vec![0u8; wire.len()];
    let mut sweeps = [0.; 2];
    let mut signs = [0.; 2];
    for (index, _, radius, sweep) in &circles {
        let class = if (radius - outer_radius).abs() <= EPS {
            0
        } else if (radius - inner_radius).abs() <= EPS {
            1
        } else {
            return Err(unsupported(
                "A radial sector has more than two circular radii",
            ));
        };
        if signs[class] != 0. && signs[class] != sweep.signum() {
            return Err(unsupported("A sector boundary reverses circular traversal"));
        }
        signs[class] = sweep.signum();
        sweeps[class] += sweep;
        classes[*index] = (class + 1) as u8;
    }
    if straight.is_empty() {
        if annular || (sweeps[0].abs() - std::f64::consts::TAU).abs() > 1e-8 {
            return Err(unsupported(
                "A cap boundary has uncertain complete circular winding",
            ));
        }
        return Ok(sweeps[0] > 0.);
    }
    if sweeps[0].abs() >= std::f64::consts::TAU - 1e-8
        || sweeps[0].abs() <= 1e-8
        || (annular && (sweeps[0] + sweeps[1]).abs() > 1e-8)
    {
        return Err(unsupported(
            "The radial sector has incompatible bounded arc traversals",
        ));
    }
    for class in 1..=if annular { 2 } else { 1 } {
        let runs = classes
            .iter()
            .enumerate()
            .filter(|(index, current)| {
                **current == class && classes[(index + classes.len() - 1) % classes.len()] != class
            })
            .count();
        if runs != 1 {
            return Err(unsupported(
                "A radial sector must have one run on each circular radius",
            ));
        }
    }
    for edge in straight {
        let start = edge.front().point() - center;
        let end = edge.back().point() - center;
        let radial = if start.magnitude2() > end.magnitude2() {
            start
        } else {
            end
        };
        if !radial.magnitude2().is_finite() || radial.magnitude() <= EPS {
            return Err(unsupported(
                "A radial cap edge collapses at its circle axis",
            ));
        }
        let perpendicular = normal.cross(radial.normalize());
        let enclosure = crate::native_bounds::curve(&edge.curve())?;
        charge(work, enclosure.len())?;
        let (low, high) = enclosure.projection(center, perpendicular)?;
        if low < -EPS || high > EPS {
            return Err(unsupported("A noncircular sector boundary is not radial"));
        }
    }
    Ok(sweeps[0] > 0.)
}

/// Connected sectors reduce to one outer turned boundary and optional holes.
/// Their native orientation supplies nesting, inherited from the source shell.
fn ordered_wires(
    edges: Vec<Edge>,
    normal: Vector3,
    history: &ShellHistory,
    work: &mut usize,
) -> Result<Vec<Wire>> {
    if edges.is_empty() || edges.len() > MAX_EDGES {
        return Err(unsupported("A merged cap has no bounded native perimeter"));
    }
    let mut outgoing = HashMap::new();
    let mut incoming = HashSet::new();
    let mut keyed = Vec::new();
    let mut identities = HashSet::new();
    for edge in edges {
        charge(work, 1)?;
        let key = crate::shell_history::closure(&[history.edge(edge.id())?])?.to_string();
        charge(work, key.len())?;
        if !identities.insert(key.clone()) {
            return Err(unsupported("Cap edges have ambiguous source identities"));
        }
        keyed.push((key, edge.id(), edge.front().id()));
        if !incoming.insert(edge.back().id()) || outgoing.insert(edge.front().id(), edge).is_some()
        {
            return Err(unsupported(
                "A merged cap perimeter branches or touches itself",
            ));
        }
    }
    if incoming.len() != outgoing.len() || incoming.iter().any(|id| !outgoing.contains_key(id)) {
        return Err(unsupported("A merged cap perimeter is not closed"));
    }
    keyed.sort_by(|a, b| a.0.cmp(&b.0));
    let mut seen = HashSet::new();
    let mut loops = Vec::new();
    for (key, first, start) in keyed {
        if seen.contains(&first) {
            continue;
        }
        let mut cursor = start;
        let mut wire = Wire::new();
        loop {
            charge(work, 1)?;
            let edge = outgoing
                .get(&cursor)
                .ok_or_else(|| unsupported("A cap boundary has an unmatched native vertex"))?;
            if !seen.insert(edge.id()) {
                return Err(unsupported(
                    "A cap boundary revisits an edge before closing",
                ));
            }
            wire.push_back(edge.clone());
            cursor = edge.back().id();
            if cursor == start {
                break;
            }
        }
        loops.push((outer_boundary(&wire, normal, work)?, key, wire));
    }
    if loops.iter().filter(|(outer, _, _)| *outer).count() != 1 {
        return Err(unsupported(
            "Connected cap sectors do not have one oriented outer boundary",
        ));
    }
    loops.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    Ok(loops.into_iter().map(|(_, _, wire)| wire).collect())
}

fn merged_face(
    members: &[usize],
    faces: &[&Face],
    planes: &HashMap<usize, Option<Planar>>,
    selected: &HashSet<EdgeId>,
    history: &mut ShellHistory,
    work: &mut usize,
) -> Result<Face> {
    let anchor = planes[&members[0]]
        .as_ref()
        .ok_or_else(|| unsupported("The source cap has no certified plane"))?;
    let mut occurrences: HashMap<EdgeId, Vec<Edge>> = HashMap::new();
    for index in members {
        for edge in faces[*index].edge_iter() {
            charge(work, 1)?;
            occurrences.entry(edge.id()).or_default().push(edge);
        }
    }
    let mut boundary = Vec::new();
    for (id, edges) in occurrences {
        match edges.as_slice() {
            [edge] => boundary.push(edge.clone()),
            [a, b] if a.orientation() != b.orientation() && !selected.contains(&id) => {}
            _ => {
                return Err(unsupported(
                    "Selected, nonmanifold or co-oriented seams cannot be removed",
                ));
            }
        }
    }
    let wires = ordered_wires(boundary, anchor.normal, history, work)?;
    let surface = if let Surface::Plane(plane) = faces[members[0]].oriented_surface() {
        plane
    } else {
        let seed = if anchor.normal.x.abs() < 0.8 {
            Vector3::unit_x()
        } else {
            Vector3::unit_y()
        };
        let u = anchor.normal.cross(seed).normalize();
        let v = anchor.normal.cross(u).normalize();
        let scale = anchor
            .bounds
            .points()
            .iter()
            .map(|point| point.distance(anchor.origin))
            .fold(1., f64::max);
        Plane::new(
            anchor.origin,
            anchor.origin + u * scale,
            anchor.origin + v * scale,
        )
    };
    let normal = surface.normal();
    if !normal.magnitude2().is_finite() || normal.dot(anchor.normal) < 1. - 1e-10 {
        return Err(unsupported(
            "A native cap plane has no stable oriented parameter frame",
        ));
    }
    // Validate against the actual stored Plane frame, not the proposed normal:
    // adding basis points at large coordinates can round that frame.
    for index in members {
        let planar = planes[index]
            .as_ref()
            .ok_or_else(|| unsupported("A cap member lost its native plane certificate"))?;
        charge(work, planar.bounds.len())?;
        let (low, high) = planar.bounds.projection(surface.origin(), normal)?;
        if low < -EPS || high > EPS {
            return Err(unsupported(
                "The merged plane does not enclose every original cap patch",
            ));
        }
        for edge in faces[*index].edge_iter() {
            let enclosure = crate::native_bounds::curve(&edge.curve())?;
            charge(work, enclosure.len())?;
            let (low, high) = enclosure.projection(surface.origin(), normal)?;
            if low < -EPS || high > EPS {
                return Err(unsupported(
                    "A native cap boundary does not belong to the merged plane",
                ));
            }
        }
    }
    let face = Face::try_new(wires, Surface::Plane(surface))
        .map_err(|_| unsupported("The merged cap boundary is not an oriented native face"))?;
    crate::validation::surface(&Shell::from(vec![face.clone()]))?;
    let mut sources = Vec::new();
    let mut identities = HashSet::new();
    for index in members {
        let source = history.face(faces[*index].id())?;
        let key = crate::shell_history::closure(std::slice::from_ref(&source))?.to_string();
        charge(work, key.len())?;
        if !identities.insert(key) {
            return Err(unsupported("Cap patches have ambiguous source identities"));
        }
        sources.push(source);
    }
    history.construct_face(
        face.id(),
        &sources,
        serde_json::json!({"constructor":"coalesced_native_circular_cap"}),
    )?;
    Ok(face)
}

pub(super) fn coalesce(
    solid: &Solid,
    selected: &[EdgeId],
    history: &mut ShellHistory,
) -> Result<Option<Coalesced>> {
    let selected = selected.iter().copied().collect::<HashSet<_>>();
    let faces = solid.face_iter().collect::<Vec<_>>();
    if faces.len() > MAX_FACES {
        return Err(unsupported("The source exceeds the native cap face budget"));
    }
    let mut work = 0;
    let mut adjacency: HashMap<EdgeId, Vec<usize>> = HashMap::new();
    let mut seeds = HashSet::new();
    for (index, face) in faces.iter().enumerate() {
        for edge in face.edge_iter() {
            charge(&mut work, 1)?;
            adjacency.entry(edge.id()).or_default().push(index);
            if selected.contains(&edge.id()) {
                circle_budget(&edge.curve(), &mut work)?;
                match crate::bounded_carriers::circle(&edge.curve()) {
                    Ok(_) => {
                        seeds.insert(index);
                    }
                    Err(CadError::Unsupported(_)) => {}
                    Err(error) => return Err(error),
                }
            }
        }
    }
    if adjacency.len() > MAX_EDGES || adjacency.values().any(|faces| faces.len() != 2) {
        return Err(unsupported(
            "The source has ambiguous or excessive native edge incidence",
        ));
    }
    let mut seeds = seeds
        .into_iter()
        .map(|index| {
            let key =
                crate::shell_history::closure(&[history.face(faces[index].id())?])?.to_string();
            charge(&mut work, key.len())?;
            Ok((key, index))
        })
        .collect::<Result<Vec<_>>>()?;
    seeds.sort_by(|a, b| a.0.cmp(&b.0));
    if seeds.windows(2).any(|pair| pair[0].0 == pair[1].0) {
        return Err(unsupported(
            "Selected cap patches have ambiguous source identities",
        ));
    }
    let mut planes = HashMap::new();
    let mut assigned = HashSet::new();
    let mut groups = Vec::new();
    for (_, seed) in seeds {
        if assigned.contains(&seed) {
            continue;
        }
        if let std::collections::hash_map::Entry::Vacant(entry) = planes.entry(seed) {
            entry.insert(planar(faces[seed], &mut work)?);
        }
        if planes[&seed].is_none() {
            continue;
        }
        let mut members = Vec::new();
        let mut pending = vec![seed];
        while let Some(index) = pending.pop() {
            if !assigned.insert(index) {
                continue;
            }
            members.push(index);
            for edge in faces[index].edge_iter() {
                for neighbor in &adjacency[&edge.id()] {
                    charge(&mut work, 1)?;
                    if assigned.contains(neighbor) || selected.contains(&edge.id()) {
                        continue;
                    }
                    if let std::collections::hash_map::Entry::Vacant(entry) =
                        planes.entry(*neighbor)
                    {
                        entry.insert(planar(faces[*neighbor], &mut work)?);
                    }
                    if let Some(candidate) = &planes[neighbor]
                        && coplanar(candidate, planes[&seed].as_ref().unwrap(), &mut work)?
                    {
                        pending.push(*neighbor);
                    }
                }
            }
        }
        if members.len() > 1 {
            groups.push(members);
        }
    }
    if groups.is_empty() {
        return Ok(None);
    }
    let mut candidate = history.clone();
    let mut replacements = HashMap::new();
    let mut output_faces = HashMap::new();
    for group in groups {
        let merged = merged_face(
            &group,
            &faces,
            &planes,
            &selected,
            &mut candidate,
            &mut work,
        )?;
        for index in group {
            replacements.insert(faces[index].id(), merged.id());
        }
        output_faces.insert(merged.id(), merged);
    }
    let mut emitted = HashSet::new();
    let mut shells = Vec::new();
    for shell in solid.boundaries() {
        let mut result = Shell::new();
        for face in shell {
            if let Some(id) = replacements.get(&face.id()) {
                if emitted.insert(*id) {
                    result.push(output_faces[id].clone());
                }
            } else {
                result.push(face.clone());
            }
        }
        shells.push(result);
    }
    let result = Solid::try_new(shells)
        .map_err(|_| unsupported("Native seam removal does not preserve a closed oriented body"))?;
    crate::validation::solid(&result)?;
    *history = candidate;
    Ok(Some(Coalesced {
        solid: result,
        faces: replacements,
    }))
}
