//! Saved mold intent and report data. Kernel objects never enter this contract.
use crate::{
    Expr, NodeRef, TopologyRef,
    reference_geometry::{AxisSource, PlaneSource, PointSource},
};
use serde::{Deserialize, Serialize};

pub const CONTRACT_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ToolingSide {
    Core,
    Cavity,
    Both,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MoldOperation {
    DraftAnalysis {
        body: NodeRef,
        pull: AxisSource,
        minimum: Expr,
        #[serde(default)]
        reverse: bool,
    },
    PartingSurface {
        body: NodeRef,
        edges: Vec<TopologyRef>,
        pull: AxisSource,
        direction: [f64; 3],
        width: Expr,
        #[serde(default)]
        reverse: bool,
    },
    ShutoffSurface {
        body: NodeRef,
        edges: Vec<TopologyRef>,
        #[serde(default)]
        reverse: bool,
    },
    Shrink {
        body: NodeRef,
        factors: [Expr; 3],
        center: PointSource,
    },
    ToolingSplit {
        body: NodeRef,
        stock: NodeRef,
        depth: Expr,
        parting: PlaneSource,
        side: ToolingSide,
    },
}
impl MoldOperation {
    pub fn name(&self) -> &'static str {
        match self {
            Self::DraftAnalysis { .. } => "Draft Analysis",
            Self::PartingSurface { .. } => "Parting Surface",
            Self::ShutoffSurface { .. } => "Shut-off Surface",
            Self::Shrink { .. } => "Mold Shrink",
            Self::ToolingSplit { .. } => "Tooling Split",
        }
    }
    pub fn is_surface(&self) -> bool {
        matches!(
            self,
            Self::PartingSurface { .. } | Self::ShutoffSurface { .. }
        )
    }
    pub fn is_analysis(&self) -> bool {
        matches!(self, Self::DraftAnalysis { .. })
    }
    pub fn expressions(&self) -> Vec<&Expr> {
        match self {
            Self::DraftAnalysis { minimum, .. } => vec![minimum],
            Self::PartingSurface { width, .. } => vec![width],
            Self::Shrink {
                factors, center, ..
            } => factors.iter().chain(center.expressions()).collect(),
            Self::ToolingSplit { depth, .. } => vec![depth],
            _ => vec![],
        }
    }
    pub fn dependencies(&self) -> Vec<NodeRef> {
        match self {
            Self::DraftAnalysis { body, pull, .. } | Self::PartingSurface { body, pull, .. } => {
                std::iter::once(body.clone())
                    .chain(pull.dependencies())
                    .collect()
            }
            Self::ShutoffSurface { body, .. } => vec![body.clone()],
            Self::Shrink { body, center, .. } => std::iter::once(body.clone())
                .chain(center.dependencies())
                .collect(),
            Self::ToolingSplit {
                body,
                stock,
                parting,
                ..
            } => [body.clone(), stock.clone()]
                .into_iter()
                .chain(parting.dependencies())
                .collect(),
        }
    }
    pub fn consumed_outputs(&self) -> Vec<NodeRef> {
        match self {
            Self::Shrink { body, .. } => vec![body.clone()],
            _ => vec![],
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DraftClass {
    Positive,
    Negative,
    Insufficient,
    Neutral,
    Unassessed,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FaceDraft {
    pub face: TopologyRef,
    pub class: DraftClass,
    pub angle_radians: Option<f64>,
    pub reason: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DraftReport {
    pub version: u32,
    pub producer: NodeRef,
    pub source: NodeRef,
    pub pull: [f64; 3],
    pub minimum_radians: f64,
    pub faces: Vec<FaceDraft>,
    /// True only when every face has an exact classification. This is draft
    /// angle analysis, not a guarantee of global visibility or tool release.
    pub complete: bool,
    pub parting_edges: Vec<TopologyRef>,
}

pub fn semantic_fields(
    doc: &crate::Document,
    owner: &str,
    node: &str,
    operation: &MoldOperation,
    state: &str,
) -> Vec<crate::presentation::SemanticElement> {
    use crate::{Dimension, presentation::SemanticElement};
    let quantities: Vec<(&str, &str, &Expr, Dimension)> = match operation {
        MoldOperation::DraftAnalysis { minimum, .. } => {
            vec![("minimum", "Minimum Draft", minimum, Dimension::Angle)]
        }
        MoldOperation::PartingSurface { width, .. } => {
            vec![("width", "Parting Surface Width", width, Dimension::Length)]
        }
        MoldOperation::Shrink { factors, .. } => vec![
            (
                "x_factor",
                "X Shrink Factor",
                &factors[0],
                Dimension::Scalar,
            ),
            (
                "y_factor",
                "Y Shrink Factor",
                &factors[1],
                Dimension::Scalar,
            ),
            (
                "z_factor",
                "Z Shrink Factor",
                &factors[2],
                Dimension::Scalar,
            ),
        ],
        MoldOperation::ToolingSplit { depth, .. } => {
            vec![("depth", "Stock Depth", depth, Dimension::Length)]
        }
        _ => vec![],
    };
    let mut out: Vec<_> = quantities.into_iter().map(|(field, label, expression, dimension)| SemanticElement {
        id: format!("{node}::mold::{field}"), role: "feature-field".into(), label: label.into(), value: Some(expression.0.clone()), state: state.into(), commands: vec!["feature.edit".into()], relationships: vec![node.into()],
        data: serde_json::json!({"contract":"tcad.mold","version":CONTRACT_VERSION,"field":field,"type":"quantity","dimension":dimension,"expression":expression,"evaluated":crate::expression::value(doc,owner,expression,dimension).ok(),"editable":true}),
    }).collect();
    let capabilities = match operation {
        MoldOperation::DraftAnalysis { .. } => {
            serde_json::json!({"geometry":"exact_planar_face_normals","curved_faces":"explicitly_unassessed","pull":"associative_axis","report_only":true,"global_release_check":false})
        }
        MoldOperation::PartingSurface { .. } => {
            serde_json::json!({"geometry":"selected_explicit_line_bspline_nurbs_edges","construction":"linear_ruled","direction":"perpendicular_to_pull","automatic_edges":"planar_positive_negative_adjacency","implicit_intersections":false})
        }
        MoldOperation::ShutoffSurface { .. } => {
            serde_json::json!({"geometry":"selected_closed_coplanar_exact_edge_loops","construction":"planar_cap","continuity":"positional"})
        }
        MoldOperation::Shrink { .. } => {
            serde_json::json!({"geometry":"exact_affine_transformation","factors":"independent_positive_xyz","center":"associative_point","factor_convention":"1_plus_linear_shrink_allowance"})
        }
        MoldOperation::ToolingSplit { .. } => {
            serde_json::json!({"stock":"closed_sketch_mid_plane_extrusion","parting":"associative_plane_normal_to_stock","construction":"exact_boolean_core_cavity","curved_parting_surface":false,"cavity_source":"associative_part_or_shrink_output"})
        }
    };
    out.push(SemanticElement { id: format!("{node}::mold::options"), role: "feature-options".into(), label: operation.name().into(), value: None, state: state.into(), commands: vec!["feature.edit".into()], relationships: operation.dependencies().iter().map(|r| r.node.clone()).collect(), data: serde_json::json!({"contract":"tcad.mold","version":CONTRACT_VERSION,"source":operation,"body_kind":if operation.is_analysis(){"analysis"}else if operation.is_surface(){"surface"}else{"solid"},"capabilities":capabilities,"editable":true}) });
    out
}
