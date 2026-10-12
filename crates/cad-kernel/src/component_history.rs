//! Source-only identities for disconnected descendants of one native carrier.
//! Native IDs join topology during this evaluation; none enters a saved role.
use super::{TopologyHistory, construction_reference};
use cad_core::{CadError, NodeRef, Result, TopologyRef};
use monstertruck::modeling::*;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

pub(super) const MAX_HISTORY_BYTES: usize = 64 * 1024 * 1024;
const MAX_STEP_BYTES: usize = 16 * 1024 * 1024;
const MAX_WORK: usize = 1_048_576;

fn unsupported(message: &str) -> CadError {
    CadError::Unsupported(format!("Topology provenance: {message}"))
}

pub(super) fn budget_error() -> CadError {
    unsupported("Published topology history storage budget exhausted (64 MiB)")
}
pub(super) fn face_limit_error() -> CadError {
    unsupported("Distinct native face budget exhausted (16384 faces)")
}
pub(super) fn edge_limit_error() -> CadError {
    unsupported("Distinct native edge budget exhausted (65536 edges)")
}

pub(super) struct Budget {
    bytes: usize,
    work: usize,
    references: BTreeMap<(NodeRef, String, String), String>,
}
impl Budget {
    pub(super) fn new() -> Self {
        Self {
            bytes: MAX_STEP_BYTES,
            work: MAX_WORK,
            references: BTreeMap::new(),
        }
    }
    pub(super) fn charge(&mut self, count: usize) -> Result<()> {
        self.work = self.work.checked_sub(count).ok_or_else(|| {
            unsupported("Topology traversal budget exhausted (1048576 work units)")
        })?;
        Ok(())
    }
    pub(super) fn encoded(&mut self, value: &impl serde::Serialize) -> Result<String> {
        struct Buffer<'a> {
            bytes: Vec<u8>,
            remaining: &'a mut usize,
        }
        impl std::io::Write for Buffer<'_> {
            fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                if bytes.len() > *self.remaining {
                    return Err(std::io::Error::other(
                        "Source ancestry serialization budget",
                    ));
                }
                *self.remaining -= bytes.len();
                self.bytes.extend_from_slice(bytes);
                Ok(bytes.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let mut buffer = Buffer {
            bytes: Vec::new(),
            remaining: &mut self.bytes,
        };
        serde_json::to_writer(&mut buffer, value)
            .map_err(|_| unsupported("Topology step serialization budget exhausted (16 MiB)"))?;
        String::from_utf8(buffer.bytes)
            .map_err(|_| CadError::Invalid("Topology provenance serialization is not UTF-8".into()))
    }
    pub(super) fn reference(&mut self, reference: &TopologyRef) -> Result<String> {
        self.charge(1)?;
        if reference.role.len() > MAX_STEP_BYTES {
            return Err(unsupported(
                "Source descriptor input exceeds 16 MiB before normalization",
            ));
        }
        let key = (
            reference.producer.clone(),
            reference.role.clone(),
            reference.topology.clone(),
        );
        if let Some(encoded) = self.references.get(&key) {
            return Ok(encoded.clone());
        }
        // Bound normalization/parsing before constructing an expanded value.
        self.bytes = self
            .bytes
            .checked_sub(reference.role.len())
            .ok_or_else(|| {
                unsupported("Distinct source descriptor storage budget exhausted (16 MiB)")
            })?;
        let encoded = self.encoded(&construction_reference(reference)?)?;
        self.references.insert(key, encoded.clone());
        Ok(encoded)
    }
}

pub(super) fn history_size(history: &TopologyHistory) -> usize {
    let mut bytes = history
        .faces
        .values()
        .chain(history.edges.values())
        .fold(0usize, |n, r| {
            n.saturating_add(r.role.len()).saturating_add(256)
        });
    for carrier in history
        .unresolved_curves
        .iter()
        .chain(&history.unresolved_surfaces)
    {
        bytes = bytes.saturating_add(carrier.len()).saturating_add(256);
    }
    for (carrier, references) in history
        .generated_curves
        .iter()
        .chain(&history.generated_surfaces)
    {
        bytes = bytes.saturating_add(carrier.len()).saturating_add(256);
        for r in references {
            bytes = bytes.saturating_add(r.role.len()).saturating_add(256);
        }
    }
    bytes
}

fn reliable(owner: &NodeRef, reference: &TopologyRef) -> bool {
    reference.producer != *owner
        || [
            "construction:",
            "pattern:",
            "hole-position:",
            "hole-intersection:",
        ]
        .iter()
        .any(|prefix| reference.role.starts_with(prefix))
}

struct Incidence {
    faces: HashMap<FaceId, Face>,
    edges: HashMap<EdgeId, Edge>,
    edge_faces: HashMap<EdgeId, HashSet<FaceId>>,
    vertex_edges: HashMap<VertexId, HashSet<EdgeId>>,
}

/// Incidence records refer to a descriptor table instead of embedding an
/// escaped, complete JSON descriptor at every boundary/endpoint occurrence.
#[derive(Default)]
struct SourceTable {
    indices: BTreeMap<String, usize>,
    references: Vec<serde_json::Value>,
}
impl SourceTable {
    fn source(&mut self, encoded: &str) -> Result<serde_json::Value> {
        let index = if let Some(index) = self.indices.get(encoded) {
            *index
        } else {
            let index = self.references.len();
            self.references.push(serde_json::from_str(encoded)?);
            self.indices.insert(encoded.into(), index);
            index
        };
        Ok(serde_json::json!({"$tcad.source":index}))
    }
    fn signature(self, value: serde_json::Value, budget: &mut Budget) -> Result<String> {
        let closure = cad_core::provenance::indexed_closure(value, self.references)?;
        budget.encoded(&closure)
    }
}
impl Incidence {
    fn new(shells: &[&Shell], budget: &mut Budget) -> Result<Self> {
        let mut result = Self {
            faces: HashMap::new(),
            edges: HashMap::new(),
            edge_faces: HashMap::new(),
            vertex_edges: HashMap::new(),
        };
        for face in shells.iter().flat_map(|shell| shell.iter()) {
            budget.charge(1)?;
            if result.faces.insert(face.id(), face.clone()).is_some() {
                continue;
            }
            if result.faces.len() > 16_384 {
                return Err(face_limit_error());
            }
            for edge in face.edge_iter() {
                budget.charge(1)?;
                result
                    .edge_faces
                    .entry(edge.id())
                    .or_default()
                    .insert(face.id());
                result
                    .edges
                    .entry(edge.id())
                    .or_insert_with(|| edge.clone());
                if result.edges.len() > 65_536 {
                    return Err(edge_limit_error());
                }
                for vertex in [edge.absolute_front(), edge.absolute_back()] {
                    result
                        .vertex_edges
                        .entry(vertex.id())
                        .or_default()
                        .insert(edge.id());
                }
            }
        }
        Ok(result)
    }
}

fn constructed(
    owner: &NodeRef,
    kind: &str,
    sources: &[&str],
    constructor: &str,
    signature: Option<&str>,
    budget: &mut Budget,
) -> Result<TopologyRef> {
    let sources = sources
        .iter()
        .map(|source| serde_json::from_str::<serde_json::Value>(source))
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let signature = signature
        .map(serde_json::from_str::<serde_json::Value>)
        .transpose()?;
    let payload = cad_core::provenance::construction_payload(serde_json::json!({
        "sources": sources,
        "purpose": {"operation":"topology", "constructor":constructor, "source_signature":signature},
        "topology":kind,
    }))?;
    let payload = budget.encoded(&payload)?;
    Ok(TopologyRef {
        producer: owner.clone(),
        role: format!("construction:{payload}"),
        topology: kind.into(),
        signature: None,
    })
}

fn original_endpoints(
    owner: &NodeRef,
    shells: &[&Shell],
    edge_sources: &HashMap<EdgeId, TopologyRef>,
    face_sources: &HashMap<FaceId, TopologyRef>,
    budget: &mut Budget,
) -> Result<HashMap<VertexId, BTreeSet<String>>> {
    let original = Incidence::new(shells, budget)?;
    let mut edges = HashMap::new();
    let mut faces = HashMap::new();
    for id in original.edges.keys() {
        if let Some(source) = edge_sources.get(id).filter(|r| reliable(owner, r)) {
            edges.insert(*id, budget.reference(source)?);
        }
    }
    for id in original.faces.keys() {
        if let Some(source) = face_sources.get(id).filter(|r| reliable(owner, r)) {
            faces.insert(*id, budget.reference(source)?);
        }
    }
    let mut groups: BTreeMap<String, Vec<VertexId>> = BTreeMap::new();
    for (vertex, adjacent) in &original.vertex_edges {
        let mut edge_keys = BTreeSet::new();
        let mut face_keys = BTreeSet::new();
        let mut complete = true;
        for edge in adjacent {
            let Some(source) = edges.get(edge) else {
                complete = false;
                break;
            };
            edge_keys.insert(source.as_str());
            let incident = &original.edge_faces[edge];
            if incident.is_empty() || incident.len() > 2 {
                complete = false;
                break;
            }
            budget.charge(incident.len() + 1)?;
            for face in incident {
                let Some(source) = faces.get(face) else {
                    complete = false;
                    break;
                };
                face_keys.insert(source.as_str());
            }
            if !complete {
                break;
            }
        }
        if complete && !edge_keys.is_empty() && !face_keys.is_empty() {
            let mut table = SourceTable::default();
            let edges = edge_keys
                .into_iter()
                .map(|source| table.source(source))
                .collect::<Result<Vec<_>>>()?;
            let faces = face_keys
                .into_iter()
                .map(|source| table.source(source))
                .collect::<Result<Vec<_>>>()?;
            let key = table.signature(serde_json::json!([edges, faces]), budget)?;
            groups.entry(key).or_default().push(*vertex);
        }
    }
    let mut result = HashMap::new();
    for (source, vertices) in groups {
        // The original vertex must itself have unambiguous source ancestry.
        // A native edge's start/end order is not an authored endpoint label
        // for adjacency-derived Boolean curves, and is deliberately excluded.
        if vertices.len() == 1 {
            result.insert(vertices[0], BTreeSet::from([source]));
        }
    }
    Ok(result)
}

/// Snapshot every source descriptor before assigning component identities.
/// Complete canonical descriptors may distinguish siblings; their position,
/// traversal order, tessellation, and current fallback roles never may.
pub(super) fn resolve(
    owner: &NodeRef,
    shells: &[&Shell],
    originals: &[&Shell],
    original_edges: &HashMap<EdgeId, TopologyRef>,
    original_faces: &HashMap<FaceId, TopologyRef>,
    history: &mut TopologyHistory,
    budget: &mut Budget,
) -> Result<()> {
    let topology = Incidence::new(shells, budget)?;
    let mut faces = HashMap::new();
    let mut edges = HashMap::new();
    for id in topology.faces.keys() {
        if let Some(reference) = history.faces.get(id).filter(|r| reliable(owner, r)) {
            faces.insert(*id, budget.reference(reference)?);
        }
    }
    for id in topology.edges.keys() {
        if let Some(reference) = history.edges.get(id).filter(|r| reliable(owner, r)) {
            edges.insert(*id, budget.reference(reference)?);
        }
    }
    let mut derived = HashMap::new();
    for (id, adjacent) in &topology.edge_faces {
        budget.charge(adjacent.len())?;
        if history.edges.contains_key(id) || adjacent.len() != 2 {
            continue;
        }
        let Some(mut sources) = adjacent
            .iter()
            .map(|face| faces.get(face).map(String::as_str))
            .collect::<Option<Vec<_>>>()
        else {
            continue;
        };
        sources.sort_unstable();
        sources.dedup();
        if sources.len() != 2 {
            continue;
        }
        let reference = constructed(
            owner,
            "edge",
            &sources,
            "source_face_adjacency",
            None,
            budget,
        )?;
        edges.insert(*id, budget.reference(&reference)?);
        derived.insert(*id, reference);
    }
    let mut face_groups: BTreeMap<&str, Vec<FaceId>> = BTreeMap::new();
    for (id, source) in &faces {
        face_groups.entry(source).or_default().push(*id);
    }
    let mut edge_groups: BTreeMap<&str, Vec<EdgeId>> = BTreeMap::new();
    for (id, source) in &edges {
        edge_groups.entry(source).or_default().push(*id);
    }
    let endpoints = if edge_groups.values().any(|group| group.len() > 1) {
        original_endpoints(owner, originals, original_edges, original_faces, budget)?
    } else {
        HashMap::new()
    };
    let snapshot = Snapshot {
        topology: &topology,
        faces: &faces,
        edges: &edges,
        endpoints: &endpoints,
    };
    let mut changed_faces = Vec::new();
    let mut changed_edges = Vec::new();
    for (source, siblings) in face_groups.into_iter().filter(|(_, group)| group.len() > 1) {
        let mut signatures = BTreeSet::new();
        for id in siblings {
            let signature = snapshot.face(id, budget)?.ok_or_else(|| unsupported(
                "A split face has incomplete boundary ancestry; an unambiguous source component is required"))?;
            if !signatures.insert(signature.clone()) {
                return Err(unsupported(
                    "Split faces have indistinguishable source boundaries; explicit component ancestry is required",
                ));
            }
            changed_faces.push((
                id,
                constructed(
                    owner,
                    "face",
                    &[source],
                    "source_boundary_component",
                    Some(&signature),
                    budget,
                )?,
            ));
        }
    }
    for (source, siblings) in edge_groups.into_iter().filter(|(_, group)| group.len() > 1) {
        let mut signatures = BTreeSet::new();
        for id in siblings {
            let signature = snapshot.edge(id, budget)?.ok_or_else(|| unsupported(
                "A split edge has incomplete endpoint ancestry; an unambiguous source segment is required"))?;
            if !signatures.insert(signature.clone()) {
                return Err(unsupported(
                    "Split edges have indistinguishable source endpoint stars; explicit segmented ancestry is required",
                ));
            }
            changed_edges.push((
                id,
                constructed(
                    owner,
                    "edge",
                    &[source],
                    "source_endpoint_component",
                    Some(&signature),
                    budget,
                )?,
            ));
        }
    }
    // No mutation is visible until all duplicate groups have complete,
    // mutually distinguishing source signatures.
    history.edges.extend(derived);
    history.faces.extend(changed_faces);
    history.edges.extend(changed_edges);
    verify_unique(history, budget)
}

struct Snapshot<'a> {
    topology: &'a Incidence,
    faces: &'a HashMap<FaceId, String>,
    edges: &'a HashMap<EdgeId, String>,
    endpoints: &'a HashMap<VertexId, BTreeSet<String>>,
}
impl Snapshot<'_> {
    fn face(&self, id: FaceId, budget: &mut Budget) -> Result<Option<String>> {
        let mut table = SourceTable::default();
        let mut wires = Vec::new();
        for wire in self.topology.faces[&id].boundaries() {
            if wire.is_empty() {
                return Ok(None);
            }
            let mut boundary = Vec::new();
            for edge in wire {
                let Some(source) = self.edges.get(&edge.id()) else {
                    return Ok(None);
                };
                let adjacent = &self.topology.edge_faces[&edge.id()];
                if adjacent.is_empty() || adjacent.len() > 2 {
                    return Ok(None);
                }
                budget.charge(adjacent.len() + 1)?;
                let Some(mut neighbors) = adjacent
                    .iter()
                    .filter(|neighbor| **neighbor != id)
                    .map(|neighbor| self.faces.get(neighbor).map(String::as_str))
                    .collect::<Option<Vec<_>>>()
                else {
                    return Ok(None);
                };
                neighbors.sort_unstable();
                neighbors.dedup();
                let source = table.source(source)?;
                let neighbors = neighbors
                    .into_iter()
                    .map(|source| table.source(source))
                    .collect::<Result<Vec<_>>>()?;
                boundary.push((source, neighbors));
            }
            boundary.sort_by_cached_key(|value| serde_json::json!(value).to_string());
            wires.push(boundary);
        }
        if wires.is_empty() {
            return Ok(None);
        }
        wires.sort_by_cached_key(|value| serde_json::json!(value).to_string());
        table.signature(serde_json::json!(wires), budget).map(Some)
    }

    fn edge(&self, id: EdgeId, budget: &mut Budget) -> Result<Option<String>> {
        let mut table = SourceTable::default();
        let adjacent = &self.topology.edge_faces[&id];
        if adjacent.is_empty() || adjacent.len() > 2 {
            return Ok(None);
        }
        budget.charge(adjacent.len())?;
        let Some(mut incident) = adjacent
            .iter()
            .map(|face| self.faces.get(face).map(String::as_str))
            .collect::<Option<Vec<_>>>()
        else {
            return Ok(None);
        };
        incident.sort_unstable();
        incident.dedup();
        let incident = incident
            .into_iter()
            .map(|source| table.source(source))
            .collect::<Result<Vec<_>>>()?;
        let edge = &self.topology.edges[&id];
        let mut ends = Vec::new();
        for vertex in [edge.absolute_front(), edge.absolute_back()] {
            let mut faces = BTreeSet::new();
            let mut edges = BTreeSet::new();
            for neighbor in &self.topology.vertex_edges[&vertex.id()] {
                let Some(source) = self.edges.get(neighbor) else {
                    return Ok(None);
                };
                edges.insert(source.as_str());
                let adjacent = &self.topology.edge_faces[neighbor];
                if adjacent.is_empty() || adjacent.len() > 2 {
                    return Ok(None);
                }
                budget.charge(adjacent.len() + 1)?;
                for face in adjacent {
                    let Some(source) = self.faces.get(face) else {
                        return Ok(None);
                    };
                    faces.insert(source.as_str());
                }
            }
            let original = self
                .endpoints
                .get(&vertex.id())
                .into_iter()
                .flatten()
                .map(|encoded| {
                    serde_json::from_str::<serde_json::Value>(encoded).map_err(CadError::from)
                })
                .collect::<Result<Vec<_>>>()?;
            let faces = faces
                .into_iter()
                .map(|source| table.source(source))
                .collect::<Result<Vec<_>>>()?;
            let edges = edges
                .into_iter()
                .map(|source| table.source(source))
                .collect::<Result<Vec<_>>>()?;
            ends.push((faces, edges, original));
        }
        ends.sort_by_cached_key(|value| serde_json::json!(value).to_string());
        table
            .signature(serde_json::json!([incident, ends]), budget)
            .map(Some)
    }
}

pub(super) fn verify_unique(history: &TopologyHistory, budget: &mut Budget) -> Result<()> {
    let mut faces = BTreeSet::new();
    for reference in history.faces.values() {
        if !faces.insert(budget.reference(reference)?) {
            return Err(unsupported(
                "Result faces share one source identity; complete split-region ancestry is required",
            ));
        }
    }
    let mut edges = BTreeSet::new();
    for reference in history.edges.values() {
        if !edges.insert(budget.reference(reference)?) {
            return Err(unsupported(
                "Result edges share one source identity; complete segmented ancestry is required",
            ));
        }
    }
    if history_size(history) > MAX_HISTORY_BYTES {
        return Err(unsupported(
            "Published topology history storage budget exhausted (64 MiB)",
        ));
    }
    Ok(())
}
