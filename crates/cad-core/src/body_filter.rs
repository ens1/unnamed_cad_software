//! Whole-solid Keep/Delete intent, independent of native geometry and presentation.
use crate::{CadError, Document, NodeRef, Result, TopologyRef, presentation::SemanticElement};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const CONTRACT_VERSION: u32 = 1;
pub const MAX_SCOPE: usize = 256;
pub const MAX_SELECTIONS: usize = 4096;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    KeepSelected,
    DeleteSelected,
}

/// A construction face identifies one solid inside its owning feature output.
/// Native body indices and geometric signatures are never selection authority.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Selection {
    pub body: NodeRef,
    pub face: TopologyRef,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Definition {
    pub scope: Vec<NodeRef>,
    pub selected: Vec<Selection>,
    pub mode: Mode,
}

impl Definition {
    pub fn name(&self) -> &'static str {
        match self.mode {
            Mode::KeepSelected => "Keep Bodies",
            Mode::DeleteSelected => "Delete Bodies",
        }
    }

    pub fn dependencies(&self) -> Vec<NodeRef> {
        self.scope.clone()
    }

    pub fn consumed_outputs(&self) -> Vec<NodeRef> {
        self.scope.clone()
    }

    pub fn validate(&self) -> Result<()> {
        if self.scope.is_empty() || self.scope.len() > MAX_SCOPE {
            return Err(CadError::Invalid(
                "Keep/Delete Bodies requires 1–256 scoped solid outputs".into(),
            ));
        }
        let scope: BTreeSet<_> = self.scope.iter().cloned().collect();
        if scope.len() != self.scope.len()
            || self
                .scope
                .iter()
                .any(|body| body.definition.trim().is_empty() || body.node.trim().is_empty())
        {
            return Err(CadError::Invalid(
                "Keep/Delete Bodies scope must contain unique, nonempty feature references".into(),
            ));
        }
        if self.selected.is_empty() || self.selected.len() > MAX_SELECTIONS {
            return Err(CadError::Invalid(
                "Keep/Delete Bodies requires 1–4096 saved face anchors".into(),
            ));
        }
        for selected in &self.selected {
            if !scope.contains(&selected.body) {
                return Err(CadError::Invalid(
                    "Each selected body must belong to the saved filter scope".into(),
                ));
            }
            if selected.face.topology != "face"
                || selected.face.role.trim().is_empty()
                || selected.face.producer.definition.trim().is_empty()
                || selected.face.producer.node.trim().is_empty()
            {
                return Err(CadError::Invalid(
                    "A body selection requires an exact, nonempty source face reference".into(),
                ));
            }
        }
        Ok(())
    }
}

fn source_name(doc: &Document, body: &NodeRef) -> String {
    doc.parts
        .get(&body.definition)
        .and_then(|part| part.features.iter().find(|feature| feature.id == body.node))
        .map(|feature| feature.name.clone())
        .unwrap_or_else(|| "Missing source".into())
}

pub fn semantic_fields(
    doc: &Document,
    owner: &str,
    node: &str,
    definition: &Definition,
    state: &str,
) -> Vec<SemanticElement> {
    let target = serde_json::json!({"kind":"feature","definition":owner,"feature":node});
    let mut fields = vec![SemanticElement {
        id: format!("{owner}::{node}::body-filter::intent"),
        role: "feature-options".into(),
        label: definition.name().into(),
        value: Some(format!(
            "{} face anchors in {} outputs",
            definition.selected.len(),
            definition.scope.len()
        )),
        state: state.into(),
        commands: vec!["feature.edit".into()],
        relationships: definition
            .scope
            .iter()
            .map(|body| format!("{}::{}", body.definition, body.node))
            .collect(),
        data: serde_json::json!({"contract":"tcad.body-filter","version":CONTRACT_VERSION,"target":target,"definition":definition,"modes":["keep_selected","delete_selected"],"selection":"exact_face_ancestry_in_owning_output","geometry":"whole_native_solids_unchanged","multiple_faces_on_one_solid":"deduplicated","empty_result":"valid","editable":true}),
    }];
    for (index, body) in definition.scope.iter().enumerate() {
        fields.push(SemanticElement {
            id: format!("{owner}::{node}::body-filter::scope::{index}"),
            role: "feature-reference".into(),
            label: format!("Scope {}", index + 1),
            value: Some(source_name(doc, body)),
            state: state.into(),
            commands: vec!["feature.edit".into()],
            relationships: vec![format!("{}::{}", body.definition, body.node)],
            data: serde_json::json!({"contract":"tcad.body-filter","version":CONTRACT_VERSION,"target":target,"field":"scope","index":index,"body":body,"editable":true}),
        });
    }
    for (index, selected) in definition.selected.iter().enumerate() {
        fields.push(SemanticElement {
            id: format!("{owner}::{node}::body-filter::selection::{index}"),
            role: "feature-reference".into(),
            label: format!("Body Anchor {}", index + 1),
            value: Some(source_name(doc, &selected.body)),
            state: state.into(),
            commands: vec!["feature.edit".into()],
            relationships: vec![format!("{}::{}", selected.body.definition, selected.body.node), format!("{}::{}", selected.face.producer.definition, selected.face.producer.node)],
            data: serde_json::json!({"contract":"tcad.body-filter","version":CONTRACT_VERSION,"target":target,"field":"selected","index":index,"selection":selected,"editable":true}),
        });
    }
    fields
}
