//! View-only history commands. No evaluation request or document command is issued here.
use crate::Workspace;
use cad_core::{NodeRef, protocol::Evaluation};
pub fn current(w: &Workspace) -> Option<&Evaluation> {
    if let Some(evaluation) = crate::reference_repair_ui::display_evaluation(w) {
        return Some(evaluation);
    }
    w.session
        .draft
        .as_ref()
        .and_then(|d| d.evaluation.as_ref())
        .or(w.session.evaluation.as_ref())
        .filter(|e| e.valid && e.identity == w.session.identity())
}
pub fn inspect(w: &mut Workspace, target: NodeRef) -> bool {
    if w.tool_checkpoint.is_some() || w.sketch.is_some() {
        w.status = "Finish or cancel the current operation before inspecting history".into();
        return false;
    }
    let Some(evaluation) = current(w) else {
        w.status = "History becomes available when the current model finishes evaluating".into();
        return false;
    };
    if let Err(e) = cad_core::history::inspectable(evaluation, &target) {
        w.status = e.to_string();
        return false;
    }
    let Some(feature) = w
        .session
        .visible()
        .parts
        .get(&target.definition)
        .and_then(|p| p.features.iter().find(|f| f.id == target.node))
    else {
        return false;
    };
    let name = feature.name.clone();
    if w.view.active_part.as_ref() != Some(&target.definition) {
        w.view.active_instance = None;
    }
    w.view.active_part = Some(target.definition.clone());
    w.view.active_node = Some(target.node.clone());
    w.view.history_cursor = Some(target);
    w.selected = None;
    w.context_selection = None;
    w.selected_feature_path = None;
    w.selection_popup = None;
    w.candidates.clear();
    w.active_plane = None;
    w.gesture = None;
    w.rendered_key.clear();
    w.status = format!("History · {name} · cached display · Latest restores the current model");
    true
}
pub fn latest(w: &mut Workspace) -> bool {
    if w.view.history_cursor.take().is_none() {
        return false;
    }
    w.selected = None;
    w.context_selection = None;
    w.selected_feature_path = None;
    w.selection_popup = None;
    w.candidates.clear();
    w.active_plane = None;
    w.gesture = None;
    w.rendered_key.clear();
    w.status = "Latest model · history inspection ended".into();
    true
}
pub fn reconcile(w: &mut Workspace) {
    let invalid = w.view.history_cursor.as_ref().is_some_and(|cursor| {
        w.view.active_part.as_ref() != Some(&cursor.definition)
            || current(w).is_none_or(|e| cad_core::history::inspectable(e, cursor).is_err())
    });
    if invalid {
        latest(w);
    }
}
