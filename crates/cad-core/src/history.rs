//! Version 1 feature-history display cache and view-only inspection contracts.
use crate::{
    presentation::{PresentationState, SemanticElement},
    protocol::{Evaluation, MeshData},
    *,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
pub const CONTRACT_VERSION: u32 = 1;
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PartHistory {
    pub version: u32,
    /// Tessellations are shared by stages through stable feature IDs.
    pub meshes: BTreeMap<Id, Arc<MeshData>>,
    pub stages: BTreeMap<Id, HistoryStage>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HistoryStage {
    pub feature: Id,
    pub index: usize,
    pub bodies: Vec<Id>,
    pub sketches: Vec<Id>,
    pub unavailable_reason: Option<String>,
}
/// Body inputs replaced by this operation, excluding reference-only inputs.
fn replaced_bodies(feature: &Feature) -> Vec<NodeRef> {
    // Suppression restores every consumed body branch in the evaluator.
    if feature.suppressed {
        return feature.operation.consumed_outputs().into_iter().collect();
    }
    feature.operation.consumed_outputs()
}
/// Project body branches and unconsumed sketches at each station in the declared history.
pub fn stages(part: &Part, body_features: &BTreeSet<Id>) -> BTreeMap<Id, HistoryStage> {
    let positions: BTreeMap<_, _> = part
        .features
        .iter()
        .enumerate()
        .map(|(i, f)| (&f.id, i))
        .collect();
    let mut out = BTreeMap::new();
    let mut live_bodies = BTreeMap::new();
    let mut live_sketches = BTreeMap::new();
    let mut consumed_bodies = BTreeSet::new();
    let mut consumed_sketches = BTreeSet::new();
    let mut latest_dependency = None;
    for (index, feature) in part.features.iter().enumerate() {
        let dependencies = if feature.suppressed {
            feature.operation.consumed_outputs()
        } else {
            part.feature_dependencies(feature)
        };
        for reference in dependencies.iter().filter(|r| r.definition == part.id) {
            if let Some(position) = positions.get(&reference.node) {
                latest_dependency = Some(latest_dependency.unwrap_or(0).max(*position));
            }
        }
        let unavailable_reason = latest_dependency.filter(|i| *i > index).map(|_| {
            "This station depends on a later station; reorder the history to inspect this prefix.".into()
        });
        if body_features.contains(&feature.id) {
            for reference in replaced_bodies(feature)
                .into_iter()
                .filter(|r| r.definition == part.id)
            {
                if let Some(position) = positions.get(&reference.node) {
                    live_bodies.remove(position);
                }
                consumed_bodies.insert(reference.node);
            }
            if !feature.suppressed {
                for reference in dependencies.into_iter().filter(|r| r.definition == part.id) {
                    if let Some(position) = positions.get(&reference.node) {
                        live_sketches.remove(position);
                    }
                    consumed_sketches.insert(reference.node);
                }
            }
            if !consumed_bodies.contains(&feature.id) {
                live_bodies.insert(index, feature.id.clone());
            }
        }
        if !feature.suppressed
            && matches!(
                feature.operation,
                Operation::Profile { .. } | Operation::Sketch3d { .. }
            )
            && !consumed_sketches.contains(&feature.id)
        {
            live_sketches.insert(index, feature.id.clone());
        }
        // The endpoint must agree with the explicitly designated current outputs.
        let bodies = if index + 1 == part.features.len() {
            part.outputs.clone()
        } else {
            live_bodies.values().cloned().collect()
        };
        let sketches = live_sketches.values().cloned().collect();
        out.insert(
            feature.id.clone(),
            HistoryStage {
                feature: feature.id.clone(),
                index,
                bodies,
                sketches,
                unavailable_reason,
            },
        );
    }
    out
}
pub fn mesh_key(instance: &PlacedPart) -> &str {
    if instance.mesh_key.is_empty() {
        &instance.definition
    } else {
        &instance.mesh_key
    }
}
pub fn stage<'a>(evaluation: &'a Evaluation, key: &str, feature: &str) -> Option<&'a HistoryStage> {
    evaluation
        .history
        .get(key)
        .filter(|h| h.version == CONTRACT_VERSION)?
        .stages
        .get(feature)
}
pub fn inspectable(evaluation: &Evaluation, cursor: &NodeRef) -> Result<()> {
    for key in std::iter::once(cursor.definition.as_str()).chain(
        evaluation
            .instances
            .iter()
            .filter(|i| i.definition == cursor.definition)
            .map(mesh_key),
    ) {
        let h = evaluation
            .history
            .get(key)
            .filter(|h| h.version == CONTRACT_VERSION)
            .ok_or_else(|| {
                CadError::Unsupported(
                    "Feature-history display cache is unavailable for this evaluation".into(),
                )
            })?;
        let s = h
            .stages
            .get(&cursor.node)
            .ok_or_else(|| CadError::Invalid("History station is unavailable".into()))?;
        if let Some(reason) = &s.unavailable_reason {
            return Err(CadError::Unsupported(reason.clone()));
        }
        if s.bodies.iter().any(|id| !h.meshes.contains_key(id)) {
            return Err(CadError::Unsupported(
                "This history station has no cached body geometry".into(),
            ));
        }
    }
    Ok(())
}
/// Borrow cached meshes; inspection never copies a document or invokes geometry evaluation.
pub fn display_meshes<'a>(
    evaluation: &'a Evaluation,
    instance: &PlacedPart,
    cursor: Option<&NodeRef>,
) -> Vec<&'a MeshData> {
    let key = mesh_key(instance);
    if let Some(cursor) = cursor.filter(|c| c.definition == instance.definition) {
        let Some(h) = evaluation
            .history
            .get(key)
            .filter(|h| h.version == CONTRACT_VERSION)
        else {
            return vec![];
        };
        let Some(s) = h
            .stages
            .get(&cursor.node)
            .filter(|s| s.unavailable_reason.is_none())
        else {
            return vec![];
        };
        return s
            .bodies
            .iter()
            .filter_map(|id| h.meshes.get(id).map(Arc::as_ref))
            .collect();
    }
    evaluation.meshes.get(key).into_iter().collect()
}
pub fn project(session: &Session, view: &PresentationState, editing: bool) -> Vec<SemanticElement> {
    let current = session
        .draft
        .as_ref()
        .and_then(|d| d.evaluation.as_ref())
        .or(session.evaluation.as_ref())
        .filter(|e| e.valid && e.identity == session.identity());
    let mut out = vec![];
    if let Some(part) = view
        .active_part
        .as_ref()
        .and_then(|id| session.visible().parts.get(id))
    {
        for (index, feature) in part.features.iter().enumerate() {
            let target = part.reference(&feature.id);
            let reason = if editing {
                Some("Finish or cancel the current operation before inspecting history".into())
            } else if let Some(e) = current {
                inspectable(e, &target).err().map(|e| e.to_string())
            } else {
                Some("History becomes available when the current model finishes evaluating".into())
            };
            let selected = view.history_cursor.as_ref() == Some(&target);
            out.push(SemanticElement { id: format!("history:{}::{}", part.id,feature.id), role: "history-station".into(), label: feature.name.clone(),
                value: reason.clone().or_else(|| Some(format!("Station {} · {}",index+1,feature.operation.name()))),
                state: if reason.is_some() { "disabled" } else if selected { "selected" } else { "available" }.into(),
                commands: vec!["history.inspect".into()], relationships: vec![part.id.clone(),feature.id.clone()],
                data: serde_json::json!({"version":CONTRACT_VERSION,"target":target,"index":index,"enabled":reason.is_none(),"disabled_reason":reason,"suppressed":feature.suppressed,"operation_kind":feature.operation.name(),"icon_command":format!("feature.{}",feature.operation.name().to_lowercase().replace(' ', "-"))}),
            });
        }
    }
    let label = view
        .history_cursor
        .as_ref()
        .and_then(|c| {
            session
                .visible()
                .parts
                .get(&c.definition)?
                .features
                .iter()
                .find(|f| f.id == c.node)
        })
        .map(|f| format!("History · {}", f.name))
        .unwrap_or_else(|| "Latest model".into());
    out.push(SemanticElement { id: "history-view".into(), role: "history-view".into(), label, value: None,
        state: if view.history_cursor.is_some() { "inspecting" } else { "latest" }.into(), commands: vec!["history.latest".into()],
        relationships: view.history_cursor.as_ref().map(|c| vec![c.definition.clone(), c.node.clone()]).unwrap_or_default(),
        data: serde_json::json!({"version":CONTRACT_VERSION,"cursor":view.history_cursor,"view_only":true}),
    });
    out
}
