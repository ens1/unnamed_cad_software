//! Saved inspection intent and disposable, configuration-qualified pair reports.
//! Display triangles never become a native geometric authority or an error certificate.
use crate::{
    CadError, Dimension, Document, Expr, Id, NodeRef, Result, TopologyRef, protocol::Evaluation,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
pub const CONTRACT_VERSION: u32 = 1;
pub const MAX_TARGETS: usize = 64;
pub const MAX_INSPECTIONS: usize = 32;
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct Target {
    /// Complete leaf occurrence path relative to the owning root assembly.
    pub path: Vec<Id>,
    /// Exact expected leaf configuration. None denotes the base variant.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub configuration: Option<Id>,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    Interference,
    BoundaryDistance,
    Both,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Definition {
    pub version: u32,
    pub id: Id,
    pub name: String,
    /// None evaluates this saved scope in any owning assembly configuration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub configuration: Option<Id>,
    pub targets: Vec<Target>,
    pub mode: Mode,
    pub clearance: Expr,
    pub boolean_tolerance: Expr,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub suppressed: bool,
}
impl Definition {
    pub fn new(name: String) -> Self {
        Self {
            version: CONTRACT_VERSION,
            id: crate::id(),
            name,
            configuration: None,
            targets: vec![],
            mode: Mode::Both,
            clearance: "1 mm".into(),
            boolean_tolerance: "0.01 mm".into(),
            suppressed: false,
        }
    }
    pub fn expressions(&self) -> Vec<&Expr> {
        vec![&self.clearance, &self.boolean_tolerance]
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Occurrence {
    pub target: Target,
    pub definition: Id,
    pub name: String,
    pub mesh_key: Id,
    pub matrix: [[f64; 4]; 4],
    pub solid_count: usize,
    pub surface_faces: usize,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InactiveTarget {
    pub target: Target,
    pub message: String,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum InterferenceStatus {
    NotRequested,
    /// Conservative, finite, positive native carrier-control hulls separate.
    SeparatedNativeHulls,
    /// A nonempty native Boolean common passed closed B-rep validation.
    ValidatedCommonSolid,
    /// Empty native output alone does not settle tangency or missed intersection.
    NoCommonSolidReturned,
    Unavailable,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FaceReference {
    pub body: NodeRef,
    pub reference: TopologyRef,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BoundaryDistance {
    pub distance_mm: f64,
    pub witness_mm: [[f64; 3]; 2],
    pub first_face: FaceReference,
    pub second_face: FaceReference,
    pub below_requested: bool,
    pub representation: String,
    /// No certified deviation bound is available for native display tessellation.
    pub error_bound_mm: Option<f64>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Pair {
    pub id: Id,
    pub first: Target,
    pub second: Target,
    pub interference: InterferenceStatus,
    /// Number of nonempty validated native body-pair common results, not a union region count.
    pub common_solid_results: usize,
    /// Bounded f64 signed-boundary volume for planar straight-edge common results.
    /// No exact arithmetic or certified numerical error bound is supplied.
    pub approximate_common_volume_mm3: Option<f64>,
    /// A validated native volumetric common implies zero engineering clearance.
    /// Otherwise no native minimum-clearance certificate is supplied.
    pub engineering_clearance_mm: Option<f64>,
    pub boundary_distance: Option<BoundaryDistance>,
    pub diagnostics: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Report {
    pub version: u32,
    pub assembly: Id,
    pub configuration: Option<Id>,
    pub inspection: Id,
    pub clearance_mm: f64,
    pub boolean_tolerance_mm: f64,
    pub occurrences: Vec<Occurrence>,
    pub inactive_targets: Vec<InactiveTarget>,
    pub expected_pairs: usize,
    pub pairs: Vec<Pair>,
    pub diagnostics: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Selection {
    pub inspection: Id,
    pub pair: Id,
    pub show_witness: bool,
}
pub fn pair_id(first: &Target, second: &Target) -> Id {
    use sha2::{Digest, Sha256};
    let pair = if first <= second {
        (first, second)
    } else {
        (second, first)
    };
    format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&pair).unwrap_or_default())
    )
}
fn invalid(message: &str) -> CadError {
    CadError::Invalid(format!("Assembly inspection: {message}"))
}
fn identity(id: &str) -> bool {
    !id.is_empty() && id.len() <= 256 && !id.chars().any(char::is_control)
}
pub fn validate(doc: &Document) -> Result<()> {
    for assembly in doc.assemblies.values() {
        if assembly.inspections.len() > MAX_INSPECTIONS {
            return Err(invalid("At most 32 saved inspections per assembly"));
        }
        if !assembly.inspections.is_empty() && !identity(&assembly.id) {
            return Err(invalid(
                "The inspection owner needs a bounded native assembly identity",
            ));
        }
        let mut names = BTreeSet::new();
        for (id, definition) in &assembly.inspections {
            if id != &definition.id
                || !identity(id)
                || definition.version != CONTRACT_VERSION
                || definition.name.trim().is_empty()
                || definition.name.len() > 256
                || !names.insert(definition.name.to_lowercase())
                || definition.targets.len() > MAX_TARGETS
                || definition
                    .expressions()
                    .iter()
                    .any(|expression| expression.0.len() > 4096)
                || definition
                    .configuration
                    .as_ref()
                    .is_some_and(|id| !identity(id))
            {
                return Err(invalid(
                    "Invalid inspection identity, version, name, target count, expression or source-reference byte budget",
                ));
            }
            if definition
                .configuration
                .as_ref()
                .is_some_and(|configuration| {
                    !crate::configuration::exists(doc, &assembly.id, configuration)
                })
            {
                return Err(invalid("The linked assembly configuration is missing"));
            }
            let mut paths = BTreeSet::new();
            for target in &definition.targets {
                if target.path.is_empty()
                    || target.path.len() > 64
                    || target.path.iter().any(|id| !identity(id))
                    || !paths.insert(&target.path)
                    || target
                        .configuration
                        .as_ref()
                        .is_some_and(|id| !identity(id))
                {
                    return Err(invalid(
                        "Targets need distinct complete occurrence paths and valid configuration identities",
                    ));
                }
            }
            if serde_json::to_vec(&definition.targets)?.len() > 256 * 1024 {
                return Err(invalid(
                    "Selected target references exceed the 256 KiB source byte budget",
                ));
            }
        }
    }
    Ok(())
}
pub fn put(doc: &mut Document, assembly: &str, definition: Definition) -> Result<()> {
    let mut candidate = doc.clone();
    candidate
        .assemblies
        .get_mut(assembly)
        .ok_or_else(|| invalid("The owning assembly is missing"))?
        .inspections
        .insert(definition.id.clone(), definition);
    validate(&candidate)?;
    *doc = candidate;
    Ok(())
}
pub fn remove(doc: &mut Document, assembly: &str, id: &str) -> Result<()> {
    if doc
        .assemblies
        .get_mut(assembly)
        .ok_or_else(|| invalid("The owning assembly is missing"))?
        .inspections
        .remove(id)
        .is_none()
    {
        return Err(invalid("The saved inspection is missing"));
    }
    Ok(())
}
fn matching_report<'a>(doc: &Document, evaluation: &'a Evaluation, id: &str) -> Option<&'a Report> {
    let report = evaluation.assembly_inspections.get(id)?;
    let source = doc.assemblies.get(&doc.root)?.inspections.get(id)?;
    (report.version == CONTRACT_VERSION
        && report.inspection == id
        && report.assembly == doc.root
        && report.configuration.as_ref() == doc.active_configurations.get(&doc.root)
        && !source.suppressed
        && (source.configuration.is_none()
            || source.configuration.as_ref() == doc.active_configurations.get(&doc.root)))
    .then_some(report)
}
pub fn current<'a>(doc: &Document, evaluation: &'a Evaluation, id: &str) -> Option<&'a Report> {
    if !evaluation.valid || evaluation.identity.source != doc.fingerprint() {
        return None;
    }
    matching_report(doc, evaluation, id)
}
pub fn semantic_fields(
    doc: &Document,
    owner: &str,
    state: &str,
    evaluation: Option<&Evaluation>,
) -> Vec<crate::presentation::SemanticElement> {
    use crate::presentation::SemanticElement;
    let Some(assembly) = doc.assemblies.get(owner) else {
        return vec![];
    };
    let mut output = Vec::new();
    let evaluation = evaluation
        .filter(|e| owner == doc.root && e.valid && e.identity.source == doc.fingerprint());
    for definition in assembly.inspections.values() {
        let report = evaluation.and_then(|e| matching_report(doc, e, &definition.id));
        let target = serde_json::json!({"kind":"assembly_inspection","assembly":owner,"inspection":definition.id});
        output.push(SemanticElement{id:definition.id.clone(),role:"assembly-inspection".into(),label:definition.name.clone(),value:None,state:state.into(),commands:vec!["assembly.inspection-edit".into(),"assembly.inspection-delete".into()],relationships:vec![owner.into()],data:serde_json::json!({"contract":"tcad.assembly_inspection","version":CONTRACT_VERSION,"source":definition,"report":report,"target":target,"shared_draft":true,"requires_current_validation":true,"placements":"assembled_mate_solved","boundary_distance_precision":"approximate_without_certified_error_bound"})});
        for (field, label, expression) in [
            (
                "clearance",
                "Requested Minimum Clearance",
                &definition.clearance,
            ),
            (
                "boolean_tolerance",
                "Native Boolean Tolerance",
                &definition.boolean_tolerance,
            ),
        ] {
            output.push(SemanticElement{id:format!("{}::{field}",definition.id),role:"inspection-field".into(),label:label.into(),value:Some(expression.0.clone()),state:state.into(),commands:vec!["assembly.inspection-edit".into()],relationships:vec![definition.id.clone()],data:serde_json::json!({"type":"quantity","dimension":Dimension::Length,"expression":expression,"field":field,"target":target,"editable":true})});
        }
        if let Some(report) = report {
            for pair in &report.pairs {
                let mut pair_target = target.clone();
                pair_target["pair"] = serde_json::json!(pair.id);
                let name = |target: &Target| {
                    crate::occurrence_names::path_from(doc, owner, &target.path)
                        .unwrap_or_else(|| "Missing occurrence".into())
                };
                output.push(SemanticElement{id:format!("{}::{}",definition.id,pair.id),role:"inspection-pair".into(),label:format!("{} ↔ {}",name(&pair.first),name(&pair.second)),value:pair.boundary_distance.as_ref().map(|d|format!("{:.6} mm (approximate boundary distance)",d.distance_mm)),state:if pair.interference==InterferenceStatus::Unavailable||!pair.diagnostics.is_empty(){"uncertain".into()}else{state.into()},commands:vec!["assembly.inspection-highlight".into(),"assembly.inspection-select-first".into(),"assembly.inspection-select-second".into()],relationships:vec![definition.id.clone()],data:serde_json::json!({"target":pair_target,"result":pair,"assembled_occurrences":true,"clearance_error_bound":null})});
            }
        }
    }
    output
}
