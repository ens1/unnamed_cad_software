//! Operation-scoped, disposable display changes. Source transactions and Apply
//! eligibility remain owned by Session and its current validated evaluation.
use crate::Workspace;
use cad_core::{
    Document, NodeRef,
    presentation::SemanticElement,
    protocol::{
        Evaluation, OPERATION_PREVIEW_VERSION, OperationPreview, OperationPreviewRequest,
        PROTOCOL_VERSION, PreviewRepresentation, RequestIdentity,
    },
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RequestKey {
    pub baseline_identity: RequestIdentity,
    pub focus: Option<NodeRef>,
}

pub fn focus(w: &Workspace) -> Option<NodeRef> {
    if w.dialog.as_deref() != Some("feature") {
        return None;
    }
    let id = w.fields.get("feature-editor-id").cloned().or_else(|| {
        serde_json::from_str::<cad_core::Feature>(&w.json)
            .ok()
            .map(|feature| feature.id)
    })?;
    w.session.visible().parts.values().find_map(|part| {
        part.features
            .iter()
            .any(|feature| feature.id == id)
            .then(|| NodeRef {
                definition: part.id.clone(),
                node: id.clone(),
            })
    })
}

fn active(w: &Workspace) -> bool {
    !w.fields.contains_key("appearance-tool")
        && w.settings.is_none()
        && (focus(w).is_some() || w.session.draft.is_some())
}

fn baseline(w: &Workspace) -> &Document {
    w.tool_checkpoint
        .as_ref()
        .map(|session| session.visible())
        .unwrap_or(&w.session.committed)
}

pub fn request_key(w: &Workspace) -> Option<RequestKey> {
    if !active(w) {
        return None;
    }
    let baseline_identity = w
        .tool_checkpoint
        .as_ref()
        .map(|session| session.identity())
        .unwrap_or_else(|| RequestIdentity {
            protocol: PROTOCOL_VERSION,
            generation: w.session.generation,
            base_revision: w.session.committed.revision,
            draft: None,
            edit: 0,
            source: w.session.committed.fingerprint(),
            backend: w.session.backend.clone(),
        });
    Some(RequestKey {
        baseline_identity,
        focus: focus(w),
    })
}

pub fn request(w: &Workspace, key: &RequestKey) -> OperationPreviewRequest {
    OperationPreviewRequest {
        baseline: baseline(w).clone(),
        baseline_identity: key.baseline_identity.clone(),
        focus: key.focus.clone(),
    }
}

/// Source identity alone does not identify an editor: opening an existing
/// feature requests its highlight without modifying authoritative source.
pub fn accepts(w: &Workspace, evaluation: &Evaluation) -> bool {
    match (request_key(w), &evaluation.operation_preview) {
        (None, None) => true,
        (Some(key), Some(preview)) => {
            preview.version == OPERATION_PREVIEW_VERSION
                && preview.focus == key.focus
                && preview.baseline_identity == key.baseline_identity
        }
        _ => !evaluation.valid,
    }
}

pub fn displayed(w: &Workspace) -> Option<&OperationPreview> {
    if w.view.original || w.view.history_cursor.is_some() || !active(w) {
        return None;
    }
    let evaluation = w
        .session
        .draft
        .as_ref()
        .and_then(|draft| draft.preview_evaluation())
        .or(w.session.evaluation.as_ref())?;
    let preview = evaluation.operation_preview.as_ref()?;
    let key = w.requested_preview.as_ref()?;
    (preview.version == OPERATION_PREVIEW_VERSION
        && preview.focus == focus(w)
        && preview.focus == key.focus
        && preview.baseline_identity == key.baseline_identity)
        .then_some(preview)
}

pub fn summary(w: &Workspace) -> Option<(String, String)> {
    if !active(w) || w.view.original || w.view.history_cursor.is_some() {
        return None;
    }
    let retained = w
        .session
        .draft
        .as_ref()
        .is_some_and(|draft| draft.pending || draft.error.is_some());
    let invalid = w
        .session
        .draft
        .as_ref()
        .is_some_and(|draft| draft.error.is_some());
    let Some(preview) = displayed(w) else {
        if w.preview_error
            .as_ref()
            .is_some_and(|(key, _)| Some(key) == w.requested_preview.as_ref())
            || w.session
                .draft
                .as_ref()
                .is_some_and(|draft| draft.error.is_some())
        {
            return Some((
                "unavailable".into(),
                if invalid {
                    "Current edit is invalid · showing the previous model"
                } else {
                    "Localized highlight unavailable"
                }
                .into(),
            ));
        }
        return Some((
            "pending".into(),
            "Preparing localized operation preview…".into(),
        ));
    };
    let added = preview
        .changes
        .values()
        .chain(preview.occurrence_changes.values())
        .any(|change| change.added.is_some());
    let removed = preview
        .changes
        .values()
        .chain(preview.occurrence_changes.values())
        .any(|change| change.removed.is_some());
    let approximate = preview
        .changes
        .values()
        .chain(preview.occurrence_changes.values())
        .any(|change| change.representation == PreviewRepresentation::Tessellated);
    let missing = !preview.diagnostics.is_empty()
        || preview
            .changes
            .values()
            .chain(preview.occurrence_changes.values())
            .any(|change| {
                change.added.is_none() && change.removed.is_none() && !change.diagnostics.is_empty()
            });
    let unavailable = !added && !removed && missing;
    let state = if unavailable {
        "unavailable"
    } else if retained {
        "retained"
    } else if missing {
        "partial"
    } else if approximate {
        "approximate"
    } else {
        "ready"
    };
    let mut label = match (added, removed) {
        (true, true) => "Added and removed material highlighted",
        (true, false) => "Added material highlighted",
        (false, true) => "Removed material highlighted",
        (false, false) if unavailable => "Localized highlight unavailable",
        _ => "No material change",
    }
    .to_string();
    if retained {
        label.push_str(if invalid {
            " · current edit invalid; last valid preview"
        } else {
            " · updating; last valid preview"
        });
    }
    if approximate {
        label.push_str(" · display approximation");
    }
    if missing && !unavailable {
        label.push_str(" · some highlights unavailable");
    }
    Some((state.into(), label))
}

pub fn status_control(ui: &mut bevy_egui::egui::Ui, w: &Workspace) {
    if let Some((state, label)) = summary(w) {
        crate::presentation_controls::label(
            ui,
            "workspace.operation-preview",
            "operation-preview-status",
            &["property-control"],
            &label,
            &state,
        )
        .on_hover_text(tooltip(w));
    }
}

pub fn tooltip(w: &Workspace) -> String {
    let mut messages = Vec::new();
    if let Some(preview) = displayed(w) {
        messages.extend(
            preview
                .diagnostics
                .iter()
                .chain(
                    preview
                        .changes
                        .values()
                        .chain(preview.occurrence_changes.values())
                        .flat_map(|change| &change.diagnostics),
                )
                .map(|diagnostic| diagnostic.message.as_str()),
        );
    }
    if let Some((key, error)) = &w.preview_error
        && Some(key) == w.requested_preview.as_ref()
    {
        messages.push(error);
    }
    messages.join("\n")
}

pub fn semantics(w: &Workspace) -> Vec<SemanticElement> {
    let Some((state, label)) = summary(w) else {
        return vec![];
    };
    let preview = displayed(w);
    let retained = w
        .session
        .draft
        .as_ref()
        .is_some_and(|draft| draft.pending || draft.error.is_some());
    let mut elements = vec![SemanticElement {
        id: "workspace.operation-preview".into(),
        role: "operation-preview-status".into(),
        label: "Operation material preview".into(),
        value: Some(label),
        state: state.clone(),
        commands: vec![],
        relationships: vec!["workspace.draft.status".into()],
        data: serde_json::json!({"contract":"tcad.operation-preview","version":OPERATION_PREVIEW_VERSION,
            "focus":preview.and_then(|preview|preview.focus.as_ref()).or_else(||w.requested_preview.as_ref().and_then(|key|key.focus.as_ref())),
            "baseline_kind":preview.map(|preview|preview.baseline_kind),
            "baseline_identity":preview.map(|preview|&preview.baseline_identity),
            "tolerance_mm":preview.map(|preview|preview.tolerance_mm),"retained":retained,
            "added":preview.is_some_and(|preview|preview.changes.values().chain(preview.occurrence_changes.values()).any(|change|change.added.is_some())),
            "removed":preview.is_some_and(|preview|preview.changes.values().chain(preview.occurrence_changes.values()).any(|change|change.removed.is_some())),
            "placement_changes_valid":preview.is_some_and(|preview|preview.placement_changes_valid),
            "occurrence_changes":preview.map(|preview|preview.occurrence_changes.iter().map(|(path,change)|serde_json::json!({"path":path,"added":change.added.is_some(),"removed":change.removed.is_some(),"representation":change.representation})).collect::<Vec<_>>()),
            "diagnostic":tooltip(w),
            "display_only":true,"apply_eligible":false}),
    }];
    if let Some(preview) = preview {
        for (key, change) in &preview.changes {
            for (kind, mesh) in [
                ("added", change.added.as_ref()),
                ("removed", change.removed.as_ref()),
            ] {
                let Some(mesh) = mesh else {
                    continue;
                };
                elements.push(SemanticElement {
                    id:format!("operation-preview:{key}:{kind}"),role:format!("operation-preview-{kind}"),
                    label:format!("{} material",if kind == "added" {"Added"} else {"Removed"}),
                    value:Some(format!("{} display triangles",mesh.indices.len()/3)),state:state.clone(),commands:vec![],
                    relationships:vec!["workspace.operation-preview".into()],
                    data:serde_json::json!({"contract":"tcad.operation-preview","version":OPERATION_PREVIEW_VERSION,
                        "mesh_key":key,"kind":kind,"representation":change.representation,"tolerance_mm":preview.tolerance_mm,
                        "focus":preview.focus,"display_only":true,"pickable":false}),
                });
            }
        }
    }
    elements
}
