//! Application-owned revolution modes; angles remain unit-aware expressions.
use crate::{
    Dimension, Document, Expr, NodeRef, Operation, ThinExtrude, presentation::SemanticElement,
};
use serde::{Deserialize, Serialize};

pub const CONTRACT_VERSION: u32 = 1;

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Options {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub contours: Vec<crate::contours::Selection>,
    pub reverse: bool,
    pub midplane: bool,
    pub second_angle: Option<Expr>,
    pub start_angle: Option<Expr>,
    pub thin: Option<ThinExtrude>,
    pub scope: Vec<NodeRef>,
}
impl Options {
    pub fn is_default(&self) -> bool {
        self == &Self::default()
    }
    pub fn expressions(&self) -> Vec<&Expr> {
        let mut out = self
            .second_angle
            .iter()
            .chain(&self.start_angle)
            .collect::<Vec<_>>();
        if let Some(thin) = &self.thin {
            out.push(&thin.thickness);
            out.extend(thin.second_thickness.iter());
        }
        out
    }
}

pub fn semantic_fields(
    doc: &Document,
    owner: &str,
    node: &str,
    operation: &Operation,
    state: &str,
) -> Vec<SemanticElement> {
    let Operation::Revolve {
        angle,
        options,
        axis_reference,
        ..
    } = operation
    else {
        return vec![];
    };
    let mut fields = vec![("angle", "Direction 1 Angle", angle)];
    if let Some(angle) = &options.second_angle {
        fields.push(("second_angle", "Direction 2 Angle", angle));
    }
    if let Some(angle) = &options.start_angle {
        fields.push(("start_angle", "Start Angle", angle));
    }
    let mut out = fields.into_iter().map(|(key, label, expression)| SemanticElement {
        id: format!("{node}::revolution::{key}"), role: "feature-field".into(), label: label.into(),
        value: Some(expression.0.clone()), state: state.into(), commands: vec!["feature.edit".into()], relationships: vec![node.into()],
        data: serde_json::json!({"contract":"tcad.revolution","version":CONTRACT_VERSION,"field":key,"type":"quantity","dimension":Dimension::Angle,"expression":expression,"evaluated":crate::expression::value(doc,owner,expression,Dimension::Angle).ok(),"editable":true}),
    }).collect::<Vec<_>>();
    out.push(SemanticElement {id:format!("{node}::revolution::options"),role:"feature-options".into(),label:"Revolution Options".into(),value:None,state:state.into(),commands:vec!["feature.edit".into()],relationships:vec![node.into()],data:serde_json::json!({"contract":"tcad.revolution","version":CONTRACT_VERSION,"options":options,"axis":axis_reference,"geometry":"exact_planar_sections_in_one_axis_half_plane","total_angle":"(0,360] degrees","regions":crate::contours::semantics(&options.contours,options.thin.is_some()),"editable":true})});
    out
}
