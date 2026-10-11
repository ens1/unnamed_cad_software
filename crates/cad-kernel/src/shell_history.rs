//! Shell constructor provenance. Kernel IDs are transient join keys only;
//! persisted identities contain authored source references and constructor intent.
use crate::adapter::{MonstertruckAdapter, TopologyHistory, construction_reference};
use cad_core::{CadError, NodeRef, Result, TopologyRef};
use monstertruck::modeling::*;
use std::collections::{HashMap, HashSet};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Retained,
    Rim,
    Constructed,
}
#[derive(Clone)]
struct FaceOrigin {
    reference: TopologyRef,
    original: Option<FaceId>,
    kind: Kind,
}
pub(crate) struct ShellHistory {
    owner: NodeRef,
    operation: &'static str,
    faces: HashMap<FaceId, FaceOrigin>,
    edges: HashMap<EdgeId, TopologyRef>,
    original_adjacency: Vec<(EdgeId, Vec<FaceId>)>,
    generated: TopologyHistory,
}
fn unavailable(message: &str) -> CadError {
    CadError::Unsupported(format!("Shell provenance: {message}"))
}
pub(crate) enum FaceTag {
    Retained(FaceId),
    Constructed {
        sources: Vec<TopologyRef>,
        purpose: serde_json::Value,
    },
    Rim {
        opening: FaceId,
        sources: Vec<TopologyRef>,
        purpose: serde_json::Value,
    },
}
impl FaceTag {
    pub(crate) fn assign(self, history: &mut ShellHistory, face: FaceId) -> Result<()> {
        match self {
            Self::Retained(source) => history.retain_face(face, source),
            Self::Constructed { sources, purpose } => {
                history.construct_face(face, &sources, purpose)
            }
            Self::Rim {
                opening,
                sources,
                purpose,
            } => history.rim(face, opening, &sources, purpose),
        }
    }
}
impl ShellHistory {
    pub(crate) fn new(
        adapter: &MonstertruckAdapter,
        owner: &NodeRef,
        body: &NodeRef,
        solid: &Solid,
    ) -> Result<Self> {
        Self::new_operation(adapter, owner, body, solid, "shell")
    }
    pub(crate) fn new_operation(
        adapter: &MonstertruckAdapter,
        owner: &NodeRef,
        body: &NodeRef,
        solid: &Solid,
        operation: &'static str,
    ) -> Result<Self> {
        let mut faces = HashMap::new();
        let mut edges = HashMap::new();
        let mut adjacency: HashMap<EdgeId, Vec<FaceId>> = HashMap::new();
        for face in solid.face_iter() {
            let reference = adapter.face_source(body, face.id()).ok_or_else(|| {
                unavailable("A source wall has no authored construction reference")
            })?;
            faces.insert(
                face.id(),
                FaceOrigin {
                    reference,
                    original: Some(face.id()),
                    kind: Kind::Retained,
                },
            );
            for edge in face.edge_iter() {
                adjacency.entry(edge.id()).or_default().push(face.id());
                if let std::collections::hash_map::Entry::Vacant(slot) = edges.entry(edge.id()) {
                    slot.insert(adapter.edge_source(body, edge.id()).ok_or_else(|| {
                        unavailable("A source edge has no authored construction reference")
                    })?);
                }
            }
        }
        Ok(Self {
            owner: owner.clone(),
            operation,
            faces,
            edges,
            original_adjacency: adjacency.into_iter().collect(),
            generated: TopologyHistory::default(),
        })
    }
    pub(crate) fn owner(&self) -> &NodeRef {
        &self.owner
    }
    pub(crate) fn face(&self, id: FaceId) -> Result<TopologyRef> {
        self.faces
            .get(&id)
            .map(|origin| origin.reference.clone())
            .ok_or_else(|| unavailable("A constructor wall source is missing"))
    }
    pub(crate) fn edge(&self, id: EdgeId) -> Result<TopologyRef> {
        self.edges
            .get(&id)
            .cloned()
            .ok_or_else(|| unavailable("A constructor edge source is missing"))
    }
    pub(crate) fn retain_face(&mut self, face: FaceId, source: FaceId) -> Result<()> {
        let origin = self
            .faces
            .get(&source)
            .cloned()
            .ok_or_else(|| unavailable("A retained wall source is missing"))?;
        self.generated.inherit_face(face, origin.reference.clone());
        self.faces.insert(face, origin);
        Ok(())
    }
    pub(crate) fn construct_face(
        &mut self,
        face: FaceId,
        sources: &[TopologyRef],
        purpose: serde_json::Value,
    ) -> Result<()> {
        let reference = self
            .generated
            .constructed_face(&self.owner, face, sources, &purpose)?;
        self.faces.insert(
            face,
            FaceOrigin {
                reference,
                original: None,
                kind: Kind::Constructed,
            },
        );
        Ok(())
    }
    pub(crate) fn rim(
        &mut self,
        face: FaceId,
        opening: FaceId,
        sources: &[TopologyRef],
        purpose: serde_json::Value,
    ) -> Result<()> {
        self.construct_face(face, sources, purpose)?;
        let original = self.faces.get(&opening).and_then(|source| source.original);
        let entry = self.faces.get_mut(&face).expect("constructed face");
        entry.original = original;
        entry.kind = Kind::Rim;
        Ok(())
    }
    pub(crate) fn construct_edge(
        &mut self,
        edge: EdgeId,
        sources: &[TopologyRef],
        purpose: serde_json::Value,
    ) -> Result<()> {
        let reference = self
            .generated
            .constructed_edge(&self.owner, edge, sources, &purpose)?;
        self.edges.insert(edge, reference);
        Ok(())
    }
    /// Every output constructor must have tagged its face. Shared-edge identity
    /// then follows the exact topological adjacency, never a centroid or fit.
    pub(crate) fn finish(mut self, solids: &[Solid]) -> Result<TopologyHistory> {
        let mut adjacency: HashMap<EdgeId, Vec<FaceOrigin>> = HashMap::new();
        for face in solids.iter().flat_map(Solid::face_iter) {
            let origin = self
                .faces
                .get(&face.id())
                .cloned()
                .ok_or_else(|| unavailable("An output wall has no constructor provenance"))?;
            self.generated
                .inherit_face(face.id(), origin.reference.clone());
            for edge in face.edge_iter() {
                adjacency.entry(edge.id()).or_default().push(origin.clone());
            }
        }
        let mut constructed = HashSet::new();
        for (edge, origins) in adjacency {
            if let Some(reference) = self.edges.get(&edge) {
                self.generated.inherit_edge(edge, reference.clone());
                continue;
            }
            if origins.len() != 2 {
                return Err(unavailable(
                    "A shell edge has ambiguous incident wall ownership",
                ));
            }
            let retained = origins.iter().any(|origin| origin.kind == Kind::Retained)
                && origins
                    .iter()
                    .all(|origin| matches!(origin.kind, Kind::Retained | Kind::Rim));
            if retained && let (Some(a), Some(b)) = (origins[0].original, origins[1].original) {
                let inherited = self
                    .original_adjacency
                    .iter()
                    .filter(|(_, faces)| {
                        faces.len() == 2 && faces.contains(&a) && faces.contains(&b)
                    })
                    .map(|(edge, _)| edge)
                    .collect::<Vec<_>>();
                if inherited.len() == 1 {
                    self.generated.inherit_edge(edge, self.edge(*inherited[0])?);
                    continue;
                }
            }
            let sources = origins
                .into_iter()
                .map(|origin| origin.reference)
                .collect::<Vec<_>>();
            let reference = self.generated.constructed_edge(
                &self.owner,
                edge,
                &sources,
                &serde_json::json!({"operation":self.operation,"constructor":"shared_boundary"}),
            )?;
            if !constructed.insert(reference.role) {
                return Err(unavailable(
                    "Several new edges share the same constructor wall closure; explicit segmented-edge ancestry is required",
                ));
            }
        }
        Ok(self.generated)
    }
}
pub(crate) fn closure(references: &[TopologyRef]) -> Result<serde_json::Value> {
    let mut values = references
        .iter()
        .map(construction_reference)
        .collect::<Result<Vec<_>>>()?;
    values.sort_by_key(serde_json::Value::to_string);
    values.dedup();
    Ok(serde_json::Value::Array(values))
}
