//! Supporting-plane stock and exact circular pocket reconstruction for Shell.
//! Source topology partitions into a closed outer envelope and independently
//! capped voids. Offset voids are inserted directly with shared circular wires;
//! display triangulation and approximate intersection curves are never used.
use cad_core::{CadError, Result};
use monstertruck::modeling::*;
use std::collections::{HashMap, HashSet};

const EPS: f64 = 1e-7;
fn invalid(message: &str) -> CadError {
    CadError::Invalid(format!("Shell pockets: {message}"))
}
fn normal(face: &Face) -> Vector3 {
    face.oriented_surface().normal(0., 0.).normalize()
}
#[derive(Clone)]
struct Opening {
    face: FaceId,
    edges: HashSet<EdgeId>,
}
struct DrillOrigin {
    sources: Vec<cad_core::TopologyRef>,
    entry: Opening,
    end: Option<Opening>,
}
struct ExpandedOpening {
    source: HashSet<EdgeId>,
    result: HashSet<EdgeId>,
}

pub(crate) fn shell(
    solid: &Solid,
    thickness: f64,
    removed: &[FaceId],
    outward: bool,
    overrides: &HashMap<FaceId, f64>,
    history: &mut crate::shell_history::ShellHistory,
) -> Result<Option<Vec<Solid>>> {
    // Inward stock offsets enlarge void walls and deepen a blind floor. An
    // outward shell requires extension of each entry carrier, handled elsewhere.
    if outward || solid.boundaries().len() != 1 {
        return Ok(None);
    }
    let hull = match crate::adapter::solid_control_points(solid) {
        Ok(hull) => hull,
        Err(CadError::Unsupported(_)) => return Ok(None),
        Err(error) => return Err(error),
    };
    let mut supporting = Vec::<(Face, Face)>::new();
    let mut caps = Vec::<Face>::new();
    let mut openings = HashMap::new();
    for face in solid.face_iter() {
        if !matches!(face.surface(), Surface::Plane(_)) {
            continue;
        }
        let n = normal(face);
        let Some(origin) = face.vertex_iter().next().map(|vertex| vertex.point()) else {
            return Ok(None);
        };
        if !n.magnitude2().is_finite()
            || hull.iter().any(|point| {
                let distance = (*point - origin).dot(n);
                !distance.is_finite() || distance > EPS
            })
        {
            continue;
        }
        let wires = face.boundaries();
        if wires.is_empty()
            || wires[0].len() < 3
            || wires[0]
                .iter()
                .any(|edge| crate::axis_geometry::line_axis(&edge.curve()).is_err())
        {
            return Ok(None);
        }
        // Only interior circular openings are reconstructed. A cut touching
        // the stock silhouette cannot be mistaken for a closed pocket.
        for wire in &wires[1..] {
            if crate::prismatic_shell::wire_circle(wire)?.is_none() {
                return Ok(None);
            }
            let cap = builder::try_attach_plane(vec![wire.inverse()])
                .map_err(|error| invalid(&error.to_string()))?;
            let mut sources = vec![history.face(face.id())?];
            sources.extend(
                wire.iter()
                    .map(|edge| history.edge(edge.id()))
                    .collect::<Result<Vec<_>>>()?,
            );
            history.construct_face(
                cap.id(),
                &sources,
                serde_json::json!({"operation":"shell","constructor":"pocket_entry_cap"}),
            )?;
            openings.insert(
                cap.id(),
                Opening {
                    face: face.id(),
                    edges: wire.iter().map(|edge| edge.id()).collect(),
                },
            );
            caps.push(cap);
        }
        let envelope = Face::try_new(vec![wires[0].clone()], face.oriented_surface())
            .map_err(|error| invalid(&error.to_string()))?;
        history.retain_face(envelope.id(), face.id())?;
        supporting.push((face.clone(), envelope));
    }
    if caps.is_empty()
        || supporting.len() < 4
        || removed
            .iter()
            .any(|id| !supporting.iter().any(|(source, _)| source.id() == *id))
    {
        return Ok(None);
    }
    let envelope_shell: Shell = supporting.iter().map(|(_, face)| face.clone()).collect();
    let envelope = match Solid::try_new(vec![envelope_shell]) {
        Ok(solid) => solid,
        Err(_) => return Ok(None),
    };
    let fixed = supporting
        .iter()
        .filter(|(source, _)| removed.contains(&source.id()))
        .map(|(_, face)| face.id())
        .collect::<Vec<_>>();
    let amounts = supporting
        .iter()
        .filter_map(|(source, face)| {
            overrides
                .get(&source.id())
                .map(|amount| (face.id(), *amount))
        })
        .collect::<HashMap<_, _>>();
    let inner_stock = match crate::polyhedron::offset_envelope(
        &envelope, thickness, &fixed, false, &amounts, history,
    ) {
        Err(CadError::Unsupported(_)) => return Ok(None),
        other => other?,
    };
    let supporting_ids = supporting
        .iter()
        .map(|(source, _)| source.id())
        .collect::<HashSet<_>>();
    let cap_ids = caps.iter().map(Face::id).collect::<HashSet<_>>();
    let mut voids: Shell = solid
        .face_iter()
        .filter(|face| !supporting_ids.contains(&face.id()))
        .map(Face::inverse)
        .collect();
    voids.extend(caps);
    let mut drills = Vec::new();
    let mut drill_origins = HashMap::new();
    for (ordinal, component) in voids.connected_components().into_iter().enumerate() {
        let entry_ids = component
            .iter()
            .filter(|face| cap_ids.contains(&face.id()))
            .map(Face::id)
            .collect::<Vec<_>>();
        let void = match Solid::try_new(vec![component]) {
            Ok(solid) => solid,
            Err(_) => return Ok(None),
        };
        crate::validation::solid(&void)?;
        let Some(expanded) = crate::prismatic_shell::expanded_drill(
            &void,
            &entry_ids,
            thickness,
            overrides,
            format!("shell-pocket-{ordinal}"),
        )?
        else {
            return Ok(None);
        };
        let sources = void
            .face_iter()
            .map(|face| history.face(face.id()))
            .collect::<Result<Vec<_>>>()?;
        let entry = openings
            .get(&expanded.entry)
            .cloned()
            .ok_or_else(|| invalid("Expanded drill entry lost its source opening"))?;
        let end = openings.get(&expanded.end).cloned();
        drill_origins.insert(
            expanded.drill.point.clone(),
            DrillOrigin {
                sources,
                entry,
                end,
            },
        );
        drills.push(expanded.drill);
    }
    let Some((inner, tools)) = crate::drilled_pockets::cut(&inner_stock, &drills)? else {
        return Err(CadError::Unsupported("Shell thickness makes expanded pocket envelopes intersect another pocket or a stock wall; trimming those offset intersections is required".into()));
    };
    let stock_faces = &inner_stock.boundaries()[0];
    if inner.boundaries()[0].len() < stock_faces.len() {
        return Err(invalid(
            "Expanded pocket insertion changed stock constructor ownership",
        ));
    }
    for (source, face) in stock_faces
        .iter()
        .zip(&inner.boundaries()[0][..stock_faces.len()])
    {
        history.retain_face(face.id(), source.id())?;
    }
    let mut expanded_openings: HashMap<FaceId, Vec<ExpandedOpening>> = HashMap::new();
    for (point, tool) in tools {
        let origin = drill_origins
            .get(&point)
            .ok_or_else(|| invalid("Expanded pocket has no source void constructor"))?;
        for (index, face) in tool.face_iter().enumerate() {
            history.construct_face(face.id(),&origin.sources,serde_json::json!({"operation":"shell","constructor":"expanded_pocket","face":index}))?;
        }
        let mut seen = HashSet::new();
        for (index, edge) in tool
            .edge_iter()
            .filter(|edge| seen.insert(edge.id()))
            .enumerate()
        {
            history.construct_edge(edge.id(),&origin.sources,serde_json::json!({"operation":"shell","constructor":"expanded_pocket","edge":index}))?;
        }
        let faces = &tool.boundaries()[0];
        if origin.end.is_none()
            && !inner
                .face_iter()
                .any(|face| face.id() == faces[faces.len() - 1].id())
        {
            return Err(CadError::Unsupported("Shell thickness makes an expanded blind pocket floor reach another stock wall; trimming the intersecting offset envelopes is required".into()));
        }
        expanded_openings
            .entry(origin.entry.face)
            .or_default()
            .push(ExpandedOpening {
                source: origin.entry.edges.clone(),
                result: faces[0].boundaries()[0]
                    .iter()
                    .map(|edge| edge.id())
                    .collect(),
            });
        if let Some(end) = &origin.end {
            expanded_openings
                .entry(end.face)
                .or_default()
                .push(ExpandedOpening {
                    source: end.edges.clone(),
                    result: faces[faces.len() - 1].boundaries()[0]
                        .iter()
                        .map(|edge| edge.id())
                        .collect(),
                });
        }
    }
    let mut inner_openings = Vec::new();
    let mut rims = Vec::new();
    for (index, (source, _)) in supporting
        .iter()
        .enumerate()
        .filter(|(_, (source, _))| removed.contains(&source.id()))
    {
        let cap = &inner.boundaries()[0][index];
        inner_openings.push(cap.id());
        let source_wires = source.boundaries();
        let inner_wires = cap.boundaries();
        if source_wires.len() != inner_wires.len() {
            return Err(invalid(
                "Wall offset changes the selected opening's contour count",
            ));
        }
        let rim =
            builder::try_attach_plane(vec![source_wires[0].clone(), inner_wires[0].inverse()])
                .map_err(|error| invalid(&error.to_string()))?;
        let mut sources = vec![history.face(source.id())?];
        sources.extend(
            source_wires[0]
                .iter()
                .map(|edge| history.edge(edge.id()))
                .collect::<Result<Vec<_>>>()?,
        );
        history.rim(
            rim.id(),
            source.id(),
            &sources,
            serde_json::json!({"operation":"shell","constructor":"opening_ring"}),
        )?;
        rims.push(rim);
        let mut used = HashSet::new();
        for wire in &source_wires[1..] {
            let source_edges = wire.iter().map(|edge| edge.id()).collect::<HashSet<_>>();
            let origins = expanded_openings
                .get(&source.id())
                .into_iter()
                .flatten()
                .filter(|opening| opening.source == source_edges)
                .collect::<Vec<_>>();
            if origins.len() != 1 {
                return Err(invalid(
                    "Expanded circular opening lost its source contour constructor",
                ));
            }
            let target_edges = &origins[0].result;
            let mut candidates = Vec::new();
            for (index, other) in inner_wires.iter().enumerate().skip(1) {
                if used.contains(&index) {
                    continue;
                }
                if other.len() == target_edges.len()
                    && other.iter().all(|edge| target_edges.contains(&edge.id()))
                {
                    candidates.push(index);
                }
            }
            if candidates.len() != 1 {
                return Err(invalid(
                    "Cannot associate an expanded pocket rim with its source opening",
                ));
            }
            used.insert(candidates[0]);
            let rim =
                builder::try_attach_plane(vec![inner_wires[candidates[0]].inverse(), wire.clone()])
                    .map_err(|error| invalid(&error.to_string()))?;
            let mut sources = vec![history.face(source.id())?];
            sources.extend(
                wire.iter()
                    .map(|edge| history.edge(edge.id()))
                    .collect::<Result<Vec<_>>>()?,
            );
            history.rim(
                rim.id(),
                source.id(),
                &sources,
                serde_json::json!({"operation":"shell","constructor":"opening_ring"}),
            )?;
            rims.push(rim);
        }
    }
    let mut boundary: Shell = solid
        .face_iter()
        .filter(|face| !removed.contains(&face.id()))
        .cloned()
        .collect();
    boundary.extend(
        inner
            .face_iter()
            .filter(|face| !inner_openings.contains(&face.id()))
            .map(Face::inverse),
    );
    boundary.extend(rims);
    let components = boundary.connected_components();
    let results = if removed.is_empty() {
        vec![Solid::try_new(components).map_err(|error| invalid(&error.to_string()))?]
    } else {
        components
            .into_iter()
            .map(|component| {
                Solid::try_new(vec![component]).map_err(|error| invalid(&error.to_string()))
            })
            .collect::<Result<Vec<_>>>()?
    };
    for result in &results {
        crate::validation::solid(result)?;
    }
    Ok(Some(results))
}
