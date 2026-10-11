//! Versioned edge-feature intent and presentation, independent of the B-rep adapter.
use crate::{Dimension, Document, Expr, Operation, TopologyRef, presentation::SemanticElement};
use serde::{Deserialize, Serialize};

pub const CONTRACT_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ChamferMethod {
    #[default]
    EqualDistance,
    TwoDistances,
    DistanceAngle,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct ChamferOptions {
    pub method: ChamferMethod,
    pub second_distance: Expr,
    pub angle: Expr,
    pub flip: bool,
    /// The face on which distance 1 is measured, resolved by construction provenance.
    pub first_face: Option<TopologyRef>,
}
impl Default for ChamferOptions {
    fn default() -> Self {
        Self {
            method: ChamferMethod::EqualDistance,
            second_distance: "2 mm".into(),
            angle: "45 deg".into(),
            flip: false,
            first_face: None,
        }
    }
}

pub fn semantic_fields(
    doc: &Document,
    owner: &str,
    node: &str,
    operation: &Operation,
    state: &str,
) -> Vec<SemanticElement> {
    let Operation::Chamfer {
        distance,
        options,
        edges,
        ..
    } = operation
    else {
        return vec![];
    };
    let mut fields = vec![("distance", "Distance 1", distance, Dimension::Length)];
    match options.method {
        ChamferMethod::EqualDistance => {}
        ChamferMethod::TwoDistances => fields.push((
            "second_distance",
            "Distance 2",
            &options.second_distance,
            Dimension::Length,
        )),
        ChamferMethod::DistanceAngle => {
            fields.push(("angle", "Angle", &options.angle, Dimension::Angle))
        }
    }
    let mut out: Vec<_> = fields.into_iter().map(|(key, label, expression, dimension)| SemanticElement {
        id: format!("{node}::chamfer::{key}"), role: "feature-field".into(), label: label.into(),
        value: Some(expression.0.clone()), state: state.into(), commands: vec!["feature.edit".into()],
        relationships: vec![node.into()],
        data: serde_json::json!({"contract":"tcad.chamfer","version":CONTRACT_VERSION,"field":key,"type":"quantity","dimension":dimension,"expression":expression,"evaluated":crate::expression::value(doc,owner,expression,dimension).ok(),"editable":true}),
    }).collect();
    out.push(SemanticElement {
        id: format!("{node}::chamfer::options"), role: "feature-options".into(), label: "Chamfer Options".into(),
        value: None, state: state.into(), commands: vec!["feature.edit".into()], relationships: vec![node.into()],
        data: serde_json::json!({"contract":"tcad.chamfer","version":CONTRACT_VERSION,"options":options,"edges":edges,"methods":["equal_distance","two_distances","distance_angle"],"unequal_geometry":"convex_straight_edges_between_supporting_planar_faces","distance_1_face":"explicit_reference_or_deterministic_normal_order","angle":"between_chamfer_and_distance_1_face"}),
    });
    out
}
