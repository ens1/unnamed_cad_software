//! Application-owned shell thickness and side intent.
use crate::{Dimension, Document, Expr, TopologyRef, presentation::SemanticElement};
use serde::{Deserialize, Serialize};

pub const CONTRACT_VERSION: u32 = 1;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct FaceThickness {
    pub face: TopologyRef,
    pub thickness: Expr,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Options {
    pub outward: bool,
    pub face_thicknesses: Vec<FaceThickness>,
}
pub fn is_default(options: &Options) -> bool {
    options == &Options::default()
}
pub fn semantic_fields(
    doc: &Document,
    owner: &str,
    node: &str,
    thickness: &Expr,
    options: &Options,
    state: &str,
) -> Vec<SemanticElement> {
    let mut out = vec![SemanticElement {
        id: format!("{node}::shell::thickness"),
        role: "feature-field".into(),
        label: "Wall Thickness".into(),
        value: Some(thickness.0.clone()),
        state: state.into(),
        commands: vec!["feature.edit".into()],
        relationships: vec![node.into()],
        data: serde_json::json!({"contract":"tcad.shell","version":CONTRACT_VERSION,"field":"thickness","type":"quantity","dimension":Dimension::Length,"expression":thickness,"evaluated":crate::expression::value(doc,owner,thickness,Dimension::Length).ok(),"editable":true}),
    }];
    out.extend(options.face_thicknesses.iter().enumerate().map(|(index, wall)|SemanticElement {
        id:format!("{node}::shell::wall::{index}"), role:"feature-field".into(),label:format!("Face {} Thickness",index+1),
        value:Some(wall.thickness.0.clone()),state:state.into(),commands:vec!["feature.edit".into()],relationships:vec![node.into(),wall.face.producer.node.clone()],
        data:serde_json::json!({"contract":"tcad.shell","version":CONTRACT_VERSION,"field":"face_thickness","index":index,"reference":wall.face,"type":"quantity","dimension":Dimension::Length,"expression":wall.thickness,"evaluated":crate::expression::value(doc,owner,&wall.thickness,Dimension::Length).ok(),"editable":true}),
    }));
    out.push(SemanticElement {id:format!("{node}::shell::options"),role:"feature-options".into(),label:"Shell Options".into(),value:None,state:state.into(),commands:vec!["feature.edit".into()],relationships:vec![node.into()],data:serde_json::json!({"contract":"tcad.shell","version":CONTRACT_VERSION,"options":options,"side":["inward","outward"],"thickness_unit":"mm","opening_faces":"associative_model_faces","editable":true})});
    out
}
