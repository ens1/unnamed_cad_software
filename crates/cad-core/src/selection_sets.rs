//! Bounded selection over native incidence. Display polylines do not establish
//! connectivity or tangency; expanded intent consists solely of source references.
use crate::{
    CadError, NodeRef, Result, TopologyRef,
    protocol::{EdgeData, MeshData, NATIVE_SELECTION_VERSION},
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

pub const CONTRACT_VERSION: u32 = 1;
const MAX_EDGES: usize = 65_536;
const MAX_INCIDENCE: usize = 1_048_576;
const MAX_WORK: usize = 1_048_576;
const MAX_SOURCE_BYTES: usize = 64 * 1024 * 1024;
/// Angular tolerance is independent of tessellation and display scale.
const TANGENT_SINE: f64 = 1e-6;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    FaceBoundary,
    ClosedLoop,
    TangentChain,
}
impl Mode {
    pub fn suffix(self) -> &'static str {
        match self {
            Self::FaceBoundary => "boundary",
            Self::ClosedLoop => "loop",
            Self::TangentChain => "tangent",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::FaceBoundary => "Select face boundaries",
            Self::ClosedLoop => "Select complete rim / loop",
            Self::TangentChain => "Select tangent chain",
        }
    }
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "boundary" => Some(Self::FaceBoundary),
            "loop" => Some(Self::ClosedLoop),
            "tangent" => Some(Self::TangentChain),
            _ => None,
        }
    }
}

pub fn same_reference(a: &TopologyRef, b: &TopologyRef) -> bool {
    a.topology == b.topology && a.producer == b.producer && a.role == b.role
}
fn unavailable(reason: &str) -> CadError {
    CadError::Unsupported(reason.into())
}
fn key(reference: &TopologyRef) -> (&str, &str, &str, &str) {
    (
        &reference.producer.definition,
        &reference.producer.node,
        &reference.topology,
        &reference.role,
    )
}
fn validate_limits(mesh: &MeshData) -> Result<()> {
    if mesh.edges.len() > MAX_EDGES || mesh.faces.len() > MAX_EDGES {
        return Err(unavailable("Native selection exceeds its topology bound"));
    }
    let mut incidence = 0usize;
    let mut bytes = 0usize;
    let mut charge = |reference: &TopologyRef| -> Result<()> {
        incidence = incidence.saturating_add(1);
        bytes = bytes
            .saturating_add(reference.role.len())
            .saturating_add(reference.topology.len())
            .saturating_add(reference.producer.definition.len())
            .saturating_add(reference.producer.node.len());
        if incidence > MAX_INCIDENCE || bytes > MAX_SOURCE_BYTES {
            Err(unavailable(
                "Native selection source incidence exceeds its resource bound",
            ))
        } else {
            Ok(())
        }
    };
    for face in &mesh.faces {
        charge(&face.reference)?;
    }
    for edge in &mesh.edges {
        charge(&edge.reference)?;
        for face in &edge.faces {
            charge(face)?;
        }
        if let Some(native) = &edge.native {
            for boundary in &native.boundaries {
                charge(&boundary.face)?;
            }
        }
    }
    Ok(())
}
struct Graph<'a> {
    edges: Vec<&'a EdgeData>,
    vertices: BTreeMap<u32, Vec<(usize, usize)>>,
    work: usize,
}
impl<'a> Graph<'a> {
    fn new(mesh: &'a MeshData, body: &NodeRef, component: u32) -> Result<Self> {
        if mesh.edges.len() > MAX_EDGES {
            return Err(unavailable("Native selection exceeds its edge bound"));
        }
        let mut graph = Self {
            edges: vec![],
            vertices: BTreeMap::new(),
            work: 0,
        };
        let mut identities = BTreeSet::new();
        let mut boundaries = BTreeMap::new();
        let mut incidence = 0usize;
        for edge in &mesh.edges {
            if edge.body.as_ref() != Some(body) {
                continue;
            }
            let native = edge.native.as_ref().ok_or_else(|| {
                unavailable("Rebuild this body to obtain native selection incidence")
            })?;
            if native.version != NATIVE_SELECTION_VERSION {
                return Err(unavailable(
                    "Native selection cache version is incompatible; rebuild this body",
                ));
            }
            if !identities.insert(key(&edge.reference)) {
                return Err(unavailable(
                    "Native selection has ambiguous source references across body components",
                ));
            }
            if native.component != component {
                continue;
            }
            graph.charge(
                native
                    .boundaries
                    .len()
                    .saturating_add(edge.faces.len())
                    .saturating_add(2),
            )?;
            if native
                .endpoints
                .iter()
                .flatten()
                .any(|value| !value.is_finite())
            {
                return Err(unavailable("Native selection contains nonfinite endpoints"));
            }
            let face_identities = edge.faces.iter().map(key).collect::<BTreeSet<_>>();
            for boundary in &native.boundaries {
                if !face_identities.contains(&key(&boundary.face)) {
                    return Err(unavailable(
                        "Native selection face and boundary incidence disagree",
                    ));
                }
                if let Some(previous) = boundaries.insert(boundary.boundary, &boundary.face)
                    && !same_reference(previous, &boundary.face)
                {
                    return Err(unavailable(
                        "Native selection boundary identity is ambiguous",
                    ));
                }
            }
            incidence = incidence
                .saturating_add(native.boundaries.len())
                .saturating_add(2);
            if incidence > MAX_INCIDENCE {
                return Err(unavailable(
                    "Native selection incidence is too large or has ambiguous source references",
                ));
            }
            let index = graph.edges.len();
            for (end, vertex) in native.vertices.iter().enumerate() {
                graph
                    .vertices
                    .entry(*vertex)
                    .or_default()
                    .push((index, end));
            }
            graph.edges.push(edge);
        }
        Ok(graph)
    }
    fn charge(&mut self, count: usize) -> Result<()> {
        self.work = self.work.saturating_add(count);
        if self.work > MAX_WORK {
            Err(unavailable(
                "Native selection traversal exceeds its work bound",
            ))
        } else {
            Ok(())
        }
    }
    fn continuation(
        &mut self,
        edge: usize,
        end: usize,
        tangent: bool,
    ) -> Result<Option<(usize, usize)>> {
        let native = self.edges[edge].native.as_ref().unwrap();
        let vertex = native.vertices[end];
        let incoming = native.tangents[end];
        self.charge(self.vertices.get(&vertex).map_or(0, Vec::len))?;
        let incidence = self.vertices.get(&vertex).cloned().unwrap_or_default();
        let mut found = None;
        for (candidate, candidate_end) in incidence {
            if candidate == edge {
                continue;
            }
            let accepts = !tangent
                || incoming
                    .zip(self.edges[candidate].native.as_ref().unwrap().tangents[candidate_end])
                    .is_some_and(|(a, b)| tangent_pair(a, b));
            if !accepts {
                continue;
            }
            if found.replace((candidate, candidate_end)).is_some() {
                return Err(unavailable(if tangent {
                    "The tangent chain branches; choose a face boundary or individual edges"
                } else {
                    "This edge belongs to several possible loops; choose a face boundary"
                }));
            }
        }
        Ok(found)
    }
    fn walk(
        &mut self,
        seed: usize,
        tangent: bool,
        require_closed: bool,
    ) -> Result<BTreeSet<usize>> {
        let mut selected = BTreeSet::from([seed]);
        let mut visited = BTreeSet::new();
        let mut pending = VecDeque::from([(seed, 0), (seed, 1)]);
        let mut open = false;
        while let Some((edge, end)) = pending.pop_front() {
            self.charge(1)?;
            if !visited.insert((edge, end)) {
                continue;
            }
            let native = self.edges[edge].native.as_ref().unwrap();
            if native.vertices[0] == native.vertices[1] {
                continue;
            }
            if let Some((next, at)) = self.continuation(edge, end, tangent)? {
                selected.insert(next);
                pending.push_back((next, 1 - at));
            } else {
                open = true;
            }
        }
        if require_closed && open {
            return Err(unavailable(
                "No unambiguous complete closed rim or loop continues from this edge",
            ));
        }
        if !require_closed && selected.len() == 1 {
            return Err(unavailable(
                "No native tangent continuation is available for this edge",
            ));
        }
        Ok(selected)
    }
}
fn tangent_pair(a: [f64; 3], b: [f64; 3]) -> bool {
    if a.iter().chain(&b).any(|v| !v.is_finite()) {
        return false;
    }
    let lengths = [
        a.iter().map(|v| v * v).sum::<f64>(),
        b.iter().map(|v| v * v).sum::<f64>(),
    ];
    if lengths.iter().any(|v| (*v - 1.).abs() > 1e-10) {
        return false;
    }
    let dot = a.iter().zip(&b).map(|(a, b)| a * b).sum::<f64>();
    let cross = [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ];
    dot < 0. && cross.iter().map(|v| v * v).sum::<f64>() <= TANGENT_SINE * TANGENT_SINE
}

/// The caller must bind `mesh` to its current evaluation and one occurrence.
/// Mesh-local tokens are never returned, persisted, or compared across meshes.
pub fn expand(
    mesh: &MeshData,
    body: &NodeRef,
    seed: &TopologyRef,
    mode: Mode,
) -> Result<Vec<TopologyRef>> {
    validate_limits(mesh)?;
    let component = if mode == Mode::FaceBoundary {
        let faces = mesh
            .faces
            .iter()
            .filter(|face| {
                face.body.as_ref() == Some(body) && same_reference(&face.reference, seed)
            })
            .count();
        if seed.topology != "face" || faces != 1 {
            return Err(unavailable("Select one current native face on this body"));
        }
        let components = mesh
            .edges
            .iter()
            .filter(|edge| {
                edge.body.as_ref() == Some(body)
                    && edge.faces.iter().any(|face| same_reference(face, seed))
            })
            .filter_map(|edge| edge.native.as_ref().map(|native| native.component))
            .collect::<BTreeSet<_>>();
        if components.len() != 1 {
            return Err(unavailable(
                "The face boundary is incomplete or spans ambiguous native components",
            ));
        }
        *components.first().unwrap()
    } else {
        let mut edges = mesh.edges.iter().filter(|edge| {
            edge.body.as_ref() == Some(body) && same_reference(&edge.reference, seed)
        });
        let edge = edges
            .next()
            .ok_or_else(|| unavailable("Select one current native edge on this body"))?;
        if edges.next().is_some() {
            return Err(unavailable(
                "The selected native edge reference is ambiguous",
            ));
        }
        edge.native
            .as_ref()
            .ok_or_else(|| unavailable("Rebuild this body to obtain native selection incidence"))?
            .component
    };
    let mut graph = Graph::new(mesh, body, component)?;
    let indices = if mode == Mode::FaceBoundary {
        let indices = graph
            .edges
            .iter()
            .enumerate()
            .filter(|(_, edge)| {
                edge.native
                    .as_ref()
                    .unwrap()
                    .boundaries
                    .iter()
                    .any(|boundary| same_reference(&boundary.face, seed))
            })
            .map(|(index, _)| index)
            .collect::<BTreeSet<_>>();
        if indices.is_empty() {
            return Err(unavailable("This face has no complete native boundary"));
        }
        indices
    } else {
        let index = graph
            .edges
            .iter()
            .position(|edge| same_reference(&edge.reference, seed))
            .ok_or_else(|| unavailable("The selected edge is absent from its native component"))?;
        match mode {
            Mode::ClosedLoop => match graph.walk(index, true, true) {
                Ok(closed) => closed,
                Err(_) => graph.walk(index, false, true)?,
            },
            Mode::TangentChain => graph.walk(index, true, false)?,
            Mode::FaceBoundary => unreachable!(),
        }
    };
    let mut result = indices
        .into_iter()
        .map(|index| graph.edges[index].reference.clone())
        .collect::<Vec<_>>();
    result.sort_by(|a, b| key(a).cmp(&key(b)));
    Ok(result)
}
