//! Saved direct-face editing intent. Exact support is supplied by the geometry adapter.
use crate::reference_geometry::{AxisSource, PlaneSource};
use crate::{Dimension, Document, Expr, NodeRef, TopologyRef, presentation::SemanticElement};
use serde::{Deserialize, Serialize};

pub const CONTRACT_VERSION: u32 = 1;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Definition {
    pub body: NodeRef,
    pub faces: Vec<TopologyRef>,
    pub method: Method,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Method {
    Offset {
        distance: Expr,
    },
    Move {
        translation: [Expr; 3],
        axis: Box<AxisSource>,
        angle: Expr,
    },
    DeleteHeal,
    Replace {
        plane: Box<PlaneSource>,
        #[serde(default)]
        reverse: bool,
    },
}
impl Definition {
    pub fn name(&self) -> &'static str {
        match self.method {
            Method::Offset { .. } => "Offset Face",
            Method::Move { .. } => "Move Face",
            Method::DeleteHeal => "Delete Face",
            Method::Replace { .. } => "Replace Face",
        }
    }
    pub fn dependencies(&self) -> Vec<NodeRef> {
        let mut out = vec![self.body.clone()];
        match &self.method {
            Method::Move { axis, .. } => out.extend(axis.dependencies()),
            Method::Replace { plane, .. } => out.extend(plane.dependencies()),
            _ => {}
        }
        out.sort();
        out.dedup();
        out
    }
    pub fn expressions(&self) -> Vec<&Expr> {
        match &self.method {
            Method::Offset { distance } => vec![distance],
            Method::Move {
                translation, angle, ..
            } => translation.iter().chain(std::iter::once(angle)).collect(),
            Method::Replace { .. } => vec![],
            Method::DeleteHeal => vec![],
        }
    }
}
pub fn semantic_fields(
    doc: &Document,
    owner: &str,
    node: &str,
    definition: &Definition,
    state: &str,
) -> Vec<SemanticElement> {
    let target = serde_json::json!({"kind":"feature","definition":owner,"feature":node});
    let quantities: Vec<(&str, &Expr, Dimension)> = match &definition.method {
        Method::Offset { distance } => vec![("Normal Offset", distance, Dimension::Length)],
        Method::Move {
            translation, angle, ..
        } => vec![
            ("X Translation", &translation[0], Dimension::Length),
            ("Y Translation", &translation[1], Dimension::Length),
            ("Z Translation", &translation[2], Dimension::Length),
            ("Rotation", angle, Dimension::Angle),
        ],
        _ => vec![],
    };
    let mut fields = quantities.into_iter().enumerate().map(|(index,(label,expression,dimension))|SemanticElement {
        id:format!("{node}::direct-edit::quantity::{index}"),role:"feature-field".into(),label:label.into(),value:Some(expression.0.clone()),state:state.into(),commands:vec!["feature.edit".into()],relationships:vec![node.into(),definition.body.node.clone()],
        data:serde_json::json!({"contract":"tcad.direct-edit","version":CONTRACT_VERSION,"target":target,"dimension":dimension,"expression":expression,"evaluated":crate::expression::value(doc,owner,expression,dimension).ok(),"editable":true}),
    }).collect::<Vec<_>>();
    fields.push(SemanticElement {id:format!("{node}::direct-edit::sources"),role:"feature-reference".into(),label:definition.name().into(),value:Some(format!("{} selected faces",definition.faces.len())),state:state.into(),commands:vec!["feature.edit".into()],relationships:vec![node.into(),definition.body.node.clone()],data:serde_json::json!({"contract":"tcad.direct-edit","version":CONTRACT_VERSION,"target":target,"source":definition,"geometry":"exact_convex_supporting_plane_reconstruction","healing":"reintersect_remaining_supporting_planes","editable":true})});
    fields
}
