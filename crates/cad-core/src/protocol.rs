use crate::{CadError, Document, Id, PlacedPart, Result, TopologyRef};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    io::{Read, Write},
};
pub const PROTOCOL_VERSION: u32 = 1;
pub const MAX_FRAME: usize = 256 * 1024 * 1024;
pub const OPERATION_PREVIEW_VERSION: u32 = 2;
pub const BODY_COUNTS_VERSION: u32 = 1;
/// Disposable inventory reported from native solids/surface bodies. Shells,
/// optical batches and output features are not individual solid bodies.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub struct BodyCounts {
    pub version: u32,
    pub solids: usize,
    pub surfaces: usize,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct RequestIdentity {
    pub protocol: u32,
    pub generation: u64,
    pub base_revision: u64,
    pub draft: Option<Id>,
    pub edit: u64,
    pub source: String,
    pub backend: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MeshData {
    pub positions: Vec<[f64; 3]>,
    pub normals: Vec<[f64; 3]>,
    pub indices: Vec<u32>,
    pub faces: Vec<FaceRange>,
    pub edges: Vec<EdgeData>,
    pub bounds: [[f64; 3]; 2],
    pub volume_mm3: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body_counts: Option<BodyCounts>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FaceRange {
    pub start: usize,
    pub count: usize,
    pub reference: TopologyRef,
    /// Evaluated output owner, independent from inherited construction provenance.
    #[serde(default)]
    pub body: Option<crate::NodeRef>,
    #[serde(default)]
    pub body_kind: BodyKind,
    #[serde(default)]
    pub plane: Option<crate::Plane>,
}
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BodyKind {
    #[default]
    Solid,
    Surface,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EdgeData {
    pub points: Vec<[f64; 3]>,
    pub reference: TopologyRef,
    /// Exact curve representation for projection; display points are never source geometry.
    #[serde(default)]
    pub curve: Option<crate::sketch_edit::Curve3>,
    #[serde(default)]
    pub faces: Vec<TopologyRef>,
    #[serde(default)]
    pub body: Option<crate::NodeRef>,
    /// Exact native incidence for selection expansion. This disposable metadata
    /// is scoped to this mesh/evaluation; only source references become intent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native: Option<NativeEdgeTopology>,
    /// Optional exact native carrier metadata for presentation guides. Positions
    /// are mesh-local millimeters; this cache never establishes design intent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub center: Option<EdgeCenter>,
}

pub const EDGE_CENTER_VERSION: u32 = 1;
/// Analytic center guides proved by the geometry adapter, independent from
/// display subdivision. Unavailable or unrecognized carriers have no guide.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EdgeCenter {
    /// Bounded non-retracing straight carrier; its midpoint is derived from the
    /// exact native endpoints rather than from the displayed polyline.
    Line {
        version: u32,
        start: [f64; 3],
        end: [f64; 3],
    },
    /// Complete or partial circular carrier. The normal and in-plane radial
    /// direction are unit vectors; radius is expressed in millimeters.
    Circle {
        version: u32,
        center: [f64; 3],
        normal: [f64; 3],
        radius_mm: f64,
        radial: [f64; 3],
    },
}

pub const NATIVE_SELECTION_VERSION: u32 = 1;
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NativeEdgeTopology {
    pub version: u32,
    /// Connected native shell token, never an authored body identity.
    pub component: u32,
    /// Opaque mesh-local native vertex tokens. Coincident positions do not join.
    pub vertices: [u32; 2],
    pub endpoints: [[f64; 3]; 2],
    /// Native one-sided derivatives pointing away from each endpoint, normalized.
    /// Missing or singular derivatives never establish a tangent continuation.
    pub tangents: [Option<[f64; 3]>; 2],
    pub boundaries: Vec<NativeEdgeBoundary>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NativeEdgeBoundary {
    /// Exact wire incidence token, scoped to this mesh/evaluation.
    pub boundary: u32,
    pub face: TopologyRef,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Diagnostic {
    pub severity: String,
    pub target: Option<Id>,
    pub message: String,
}
/// Optional display work requested alongside evaluation. It never changes CAD
/// definitions, the evaluated result's validity, or Apply eligibility.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OperationPreviewRequest {
    pub baseline: Document,
    pub baseline_identity: RequestIdentity,
    /// A focused feature compares its own evaluated input and output; generic
    /// edits compare the checkpoint document and current document instead.
    pub focus: Option<crate::NodeRef>,
}
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PreviewBaseline {
    FeatureInput,
    #[default]
    Checkpoint,
}
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PreviewRepresentation {
    #[default]
    ExactBrep,
    /// A bounded difference of tessellated display geometry. This cache is
    /// explicitly approximate and is never a modeling or export shape.
    Tessellated,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OperationPreviewChange {
    pub added: Option<MeshData>,
    pub removed: Option<MeshData>,
    #[serde(default)]
    pub representation: PreviewRepresentation,
    #[serde(default)]
    pub diagnostics: Vec<Diagnostic>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OperationPreview {
    pub version: u32,
    pub baseline_identity: RequestIdentity,
    pub focus: Option<crate::NodeRef>,
    pub baseline_kind: PreviewBaseline,
    pub tolerance_mm: f64,
    /// Mesh-keyed local material changes, shared across matching occurrences.
    pub changes: BTreeMap<Id, OperationPreviewChange>,
    /// Checkpoint occurrence membership was compared from validated native
    /// evaluations. Older display caches must not infer per-instance context.
    #[serde(default)]
    pub placement_changes_valid: bool,
    /// Added/removed complete native occurrences, keyed by full stable path.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub occurrence_changes: BTreeMap<String, OperationPreviewChange>,
    /// Placement context for a completely removed body or occurrence.
    pub baseline_instances: Vec<PlacedPart>,
    #[serde(default)]
    pub diagnostics: Vec<Diagnostic>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Evaluation {
    pub identity: RequestIdentity,
    pub meshes: BTreeMap<Id, MeshData>,
    pub instances: Vec<PlacedPart>,
    pub diagnostics: Vec<Diagnostic>,
    pub valid: bool,
    /// Complete current native geometry can remain usable for repairing an invalid
    /// drawing. This never authorizes Apply or export; those require `valid`.
    #[serde(default)]
    pub geometry_valid: bool,
    pub elapsed_ms: u64,
    #[serde(default)]
    pub sketches: BTreeMap<String, EvaluatedSketch>,
    /// Solved spatial sketch source and disposable display/diagnostic data.
    /// Native curve references remain independent from these polylines.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub spatial_sketches: BTreeMap<String, crate::sketch3d::Evaluated>,
    /// Parametric datum display and selection geometry; source intent remains in document features.
    #[serde(default)]
    pub references: BTreeMap<String, crate::reference_geometry::EvaluatedReference>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub reference_curves: BTreeMap<String, crate::reference_curves::Evaluated>,
    /// Disposable edge-only display caches, excluded from body/property integration.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub reference_curve_meshes: BTreeMap<String, MeshData>,
    #[serde(default)]
    pub assemblies: BTreeMap<String, crate::assembly_geometry::EvaluatedAssembly>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub exploded_views: BTreeMap<Id, crate::exploded_views::Evaluated>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub assembly_inspections: BTreeMap<Id, crate::assembly_inspection::Report>,
    #[serde(default)]
    pub properties: BTreeMap<String, crate::inspection::Properties>,
    #[serde(default)]
    pub sheets: BTreeMap<String, crate::sheet_metal::EvaluatedSheet>,
    #[serde(default)]
    pub weldments: BTreeMap<String, crate::weldment::EvaluatedWeldment>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub weld_beads: BTreeMap<String, crate::weld_beads::Evaluated>,
    /// Current native joint diagnostics, including failed beads. Display only;
    /// their presence never relaxes document validity or Apply eligibility.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub weld_joints: Vec<crate::weld_beads::JointPreview>,
    /// Assembly-owned deposited metal, separate from shared part definitions.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub assembly_welds: BTreeMap<String, crate::assembly_welds::Evaluated>,
    /// Exact source-linked analysis reports, separate from display meshes.
    #[serde(default)]
    pub mold_analyses: BTreeMap<String, crate::mold::DraftReport>,
    #[serde(default)]
    pub configurations: Option<crate::configuration::Evaluated>,
    /// Optional versioned display cache; never authoritative or eligible for Apply.
    #[serde(default)]
    pub history: std::sync::Arc<BTreeMap<String, crate::history::PartHistory>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operation_preview: Option<Box<OperationPreview>>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EvaluatedSketch {
    pub plane: crate::Plane,
    pub profile_order: Vec<Id>,
    pub points: BTreeMap<Id, [f64; 2]>,
    pub circles: BTreeMap<Id, EvaluatedCircle>,
    #[serde(default)]
    pub polylines: Vec<crate::sketch_geometry::SketchPolyline>,
    #[serde(default)]
    pub solved: Option<crate::Sketch>,
    /// Current validated material-region membership over authored native member
    /// identities. Closed hole wires belong to their surrounding region; this
    /// disposable metadata never replaces sketch or feature definitions.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub material_regions: Vec<EvaluatedMaterialRegion>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct EvaluatedMaterialRegion {
    pub outer_members: Vec<Id>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hole_members: Vec<Vec<Id>>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EvaluatedCircle {
    pub center: Id,
    pub radius_mm: f64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "command", rename_all = "snake_case")]
pub enum WorkerRequest {
    DisplayMesh {
        request: Box<crate::display_mesh::Request>,
    },
    AnalyzeSketch {
        identity: RequestIdentity,
        document: Document,
        owner: Id,
        sketch: Box<crate::Sketch>,
    },
    /// Transient spatial cursor goals. These are never saved sketch relations.
    SolveSketch3d {
        identity: RequestIdentity,
        document: Document,
        feature: crate::NodeRef,
        editor: Id,
        sequence: u64,
        targets: Vec<crate::sketch3d::DragTarget>,
    },
    ManipulateAssembly {
        identity: RequestIdentity,
        document: Document,
        assemblies: BTreeMap<Id, crate::assembly_geometry::EvaluatedAssembly>,
        intent: Box<crate::assembly_motion::Intent>,
    },
    Evaluate {
        identity: RequestIdentity,
        document: Document,
        project_root: String,
        tolerance: f64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        operation_preview: Option<Box<OperationPreviewRequest>>,
    },
    Export {
        identity: RequestIdentity,
        document: Document,
        project_root: String,
        path: String,
        format: String,
        tolerance: f64,
    },
    Release {
        identity: RequestIdentity,
        document: Document,
        project_root: String,
        path: String,
        tolerance: f64,
    },
    Import {
        identity: RequestIdentity,
        document: Document,
        project_root: String,
        resource: Id,
    },
    Shutdown,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WorkerResponse {
    DisplayMesh {
        output: Box<crate::display_mesh::Output>,
    },
    SketchAnalyzed {
        identity: RequestIdentity,
        owner: Id,
        sketch: Id,
        analysis: Box<crate::sketch_diagnostics::Analysis>,
    },
    Sketch3dSolved {
        identity: RequestIdentity,
        feature: crate::NodeRef,
        editor: Id,
        sequence: u64,
        solved: Box<crate::sketch3d::Definition>,
        report: crate::solver::SolveReport,
    },
    Sketch3dSolveFailed {
        identity: RequestIdentity,
        feature: crate::NodeRef,
        editor: Id,
        sequence: u64,
        message: String,
    },
    AssemblyManipulated {
        identity: RequestIdentity,
        outcome: Box<crate::assembly_motion::Outcome>,
    },
    Evaluation {
        result: Evaluation,
    },
    Exported {
        identity: RequestIdentity,
        path: String,
    },
    Imported {
        identity: RequestIdentity,
        document: Document,
    },
    Error {
        identity: Option<RequestIdentity>,
        message: String,
    },
}
pub fn write_frame(writer: &mut impl Write, value: &impl Serialize) -> Result<()> {
    let data = serde_json::to_vec(value)?;
    if data.len() > MAX_FRAME {
        return Err(CadError::Invalid("IPC frame exceeds limit".into()));
    }
    writer.write_all(&(data.len() as u32).to_le_bytes())?;
    writer.write_all(&data)?;
    writer.flush()?;
    Ok(())
}
pub fn read_frame<T: serde::de::DeserializeOwned>(reader: &mut impl Read) -> Result<T> {
    let mut size = [0u8; 4];
    reader.read_exact(&mut size)?;
    let size = u32::from_le_bytes(size) as usize;
    if size > MAX_FRAME {
        return Err(CadError::Invalid("IPC frame exceeds limit".into()));
    }
    let mut data = vec![0; size];
    reader.read_exact(&mut data)?;
    Ok(serde_json::from_slice(&data)?)
}
pub trait GeometryService {
    fn fingerprint(&self) -> &str;
    fn evaluate(
        &mut self,
        identity: RequestIdentity,
        document: &Document,
        project_root: &std::path::Path,
        tolerance: f64,
    ) -> Result<Evaluation>;
    fn export(
        &mut self,
        document: &Document,
        root: &std::path::Path,
        path: &std::path::Path,
        format: &str,
        tolerance: f64,
    ) -> Result<()>;
}
