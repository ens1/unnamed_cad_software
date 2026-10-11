//! Saved planar solid splitting intent, independent of the geometry adapter.
use crate::{
    CadError, Document, Expr, NodeRef, Result, presentation::SemanticElement,
    reference_geometry::PlaneSource,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const CONTRACT_VERSION: u32 = 1;
pub const MAX_SCOPE: usize = 256;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Keep {
    Positive,
    Negative,
    #[default]
    Both,
}
impl Keep {
    pub fn label(self) -> &'static str {
        match self {
            Self::Positive => "Positive Side",
            Self::Negative => "Negative Side",
            Self::Both => "Both Sides",
        }
    }
}

/// A selected planar face supplies its complete supporting plane. Its finite
/// trims do not limit this operation; source stock remains the extent authority.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Definition {
    pub scope: Vec<NodeRef>,
    pub plane: Box<PlaneSource>,
    pub offset: Expr,
    #[serde(default)]
    pub reverse: bool,
    #[serde(default)]
    pub keep: Keep,
}
impl Definition {
    pub fn name(&self) -> &'static str {
        "Split"
    }
    pub fn dependencies(&self) -> Vec<NodeRef> {
        self.scope
            .iter()
            .cloned()
            .chain(self.plane.dependencies())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }
    pub fn consumed_outputs(&self) -> Vec<NodeRef> {
        self.scope.clone()
    }
    pub fn expressions(&self) -> Vec<&Expr> {
        vec![&self.offset]
    }
    pub fn validate(&self) -> Result<()> {
        if self.scope.is_empty() || self.scope.len() > MAX_SCOPE {
            return Err(CadError::Invalid(
                "Split requires one through 256 scoped solid outputs".into(),
            ));
        }
        if self
            .scope
            .iter()
            .any(|source| source.definition.trim().is_empty() || source.node.trim().is_empty())
            || self.scope.iter().collect::<BTreeSet<_>>().len() != self.scope.len()
        {
            return Err(CadError::Invalid(
                "Split scope must contain distinct, nonempty solid feature references".into(),
            ));
        }
        match self.plane.as_ref() {
            PlaneSource::Face { body, reference }
                if reference.topology != "face"
                    || reference.role.trim().is_empty()
                    || reference.producer.definition.trim().is_empty()
                    || reference.producer.node.trim().is_empty()
                    || body.definition.trim().is_empty()
                    || body.node.trim().is_empty() =>
            {
                Err(CadError::Invalid(
                    "Split needs an exact planar face reference and its owning output".into(),
                ))
            }
            PlaneSource::Datum { feature } | PlaneSource::CoordinateSystem { feature, .. }
                if feature.definition.trim().is_empty() || feature.node.trim().is_empty() =>
            {
                Err(CadError::Invalid(
                    "Split plane feature reference is empty".into(),
                ))
            }
            _ => Ok(()),
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
    let mut result = vec![
        SemanticElement {
            id: format!("{owner}::{node}::split::intent"),
            role: "feature-options".into(),
            label: "Split".into(),
            value: Some(definition.keep.label().into()),
            state: state.into(),
            commands: vec!["feature.edit".into()],
            relationships: definition
                .dependencies()
                .iter()
                .map(|source| format!("{}::{}", source.definition, source.node))
                .collect(),
            data: serde_json::json!({"contract":"tcad.split","version":CONTRACT_VERSION,"target":target,"source":definition,"keep_modes":["positive","negative","both"],"plane_sources":["origin","datum","coordinate_system","planar_solid_or_surface_face"],"planar_face_extent":"complete_supporting_plane","scope":"explicit_solid_outputs","construction":"exact_native_half_space","reference_identity":"source_and_side_construction_ancestry","empty_side":"valid_empty_result","editable":true}),
        },
        SemanticElement {
            id: format!("{owner}::{node}::split::offset"),
            role: "feature-field".into(),
            label: "Plane Offset".into(),
            value: Some(definition.offset.0.clone()),
            state: state.into(),
            commands: vec!["feature.edit".into()],
            relationships: vec![node.into()],
            data: serde_json::json!({"contract":"tcad.split","version":CONTRACT_VERSION,"target":target,"field":"offset","type":"quantity","dimension":crate::Dimension::Length,"expression":definition.offset,"evaluated":crate::expression::value(doc,owner,&definition.offset,crate::Dimension::Length).ok(),"positive_direction":"unreversed_source_plane_normal","editable":true}),
        },
    ];
    for (index, source) in definition.scope.iter().enumerate() {
        let name = doc
            .parts
            .get(&source.definition)
            .and_then(|part| {
                part.features
                    .iter()
                    .find(|feature| feature.id == source.node)
            })
            .map(|feature| feature.name.as_str())
            .unwrap_or("Missing source");
        result.push(SemanticElement { id:format!("{owner}::{node}::split::scope::{index}"),role:"feature-reference".into(),label:format!("Solid Output {}",index+1),value:Some(name.into()),state:state.into(),commands:vec!["feature.edit".into()],relationships:vec![format!("{}::{}",source.definition,source.node)],data:serde_json::json!({"contract":"tcad.split","version":CONTRACT_VERSION,"target":target,"field":"scope","index":index,"source":source,"editable":true}) });
    }
    result
}
