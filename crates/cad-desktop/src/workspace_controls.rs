//! Compact workspace controls projected independently of their default overlays.
use crate::{Workspace, presentation_controls as controls, ui};
use bevy_egui::egui;
use cad_core::presentation::SemanticElement;

/// View-only task state. These keys never enter the CAD document or its undo history.
const DIAGNOSTICS: &str = "task-panel:diagnostics";
const HISTORY: &str = "task-panel:history";

pub(crate) fn required_action(w: &Workspace) -> Option<&'static str> {
    if w.dialog.as_deref() != Some("feature") {
        return None;
    }
    if w.fields.keys().any(|key| key.ends_with("source-invalid"))
        || !crate::quantity_display::source_fields_valid(w)
    {
        return None;
    }
    let feature = w
        .view
        .active_part
        .as_ref()
        .and_then(|owner| w.session.visible().parts.get(owner))
        .and_then(|part| {
            part.features
                .iter()
                .find(|f| Some(&f.id) == w.fields.get("feature-editor-id"))
        })?;
    match &feature.operation {
        cad_core::Operation::Loft { profiles, .. } if profiles.len() < 2 => {
            Some("Select at least two sketch sections")
        }
        cad_core::Operation::Sweep {
            path, path_sketch, ..
        } if path_sketch.is_none() && path.len() < 2 => Some("Select a sweep path"),
        cad_core::Operation::Fillet { edges, .. } | cad_core::Operation::Chamfer { edges, .. }
            if edges.is_empty() =>
        {
            Some("Pick edges on the model")
        }
        cad_core::Operation::Draft { faces, .. } if faces.is_empty() => Some("Pick faces to draft"),
        cad_core::Operation::Weldment { operation } => match operation.as_ref() {
            cad_core::weldment::WeldmentOperation::Members {
                path,
                members,
                member_mode,
                ..
            } if members.is_empty()
                && (*member_mode == cad_core::weldment::MemberMode::Explicit
                    || crate::fabrication_ui::structural_path_members(w, path).is_empty()) =>
            {
                Some("Pick member paths")
            }
            _ => None,
        },
        _ => None,
    }
}

pub(crate) fn task_title(w: &Workspace) -> String {
    if let Some(pattern) = w
        .fields
        .get("assembly-id")
        .and_then(|owner| w.session.visible().assemblies.get(owner))
        .and_then(|assembly| {
            w.fields
                .get("component-pattern-editor")
                .and_then(|id| assembly.component_patterns.get(id))
        })
    {
        return pattern.name.clone();
    }
    if w.fields.contains_key("mate-editor") {
        return "Mate".into();
    }
    if w.fields.contains_key("assembly-weld-editor") {
        return "Weld Bead".into();
    }
    if let Some(feature) = w
        .view
        .active_part
        .as_ref()
        .and_then(|owner| w.session.visible().parts.get(owner))
        .and_then(|part| {
            part.features
                .iter()
                .find(|f| Some(&f.id) == w.fields.get("feature-editor-id"))
        })
        && w.dialog.as_deref() == Some("feature")
    {
        return feature.name.clone();
    }
    if let Some(sketch) = &w.sketch {
        return sketch.name.clone();
    }
    match w.dialog.as_deref() {
        Some("assembly-motion") => "Move / Rotate Component".into(),
        Some("component-insert") => "Insert Component".into(),
        Some("reference-repair") => "Repair References".into(),
        Some("feature-order") => "Feature Order".into(),
        Some("assembly") => "Assembly".into(),
        Some("configurations") => "Configurations and Equations".into(),
        Some("drawings") => "Drawing".into(),
        _ if !w.dialog_title.is_empty() => w.dialog_title.clone(),
        _ => "Operation".into(),
    }
}

pub(crate) fn concise_problem(detail: &str) -> String {
    // Preserve the actual failure category; full kernel/reference detail remains
    // independently available in the diagnostics descriptor and expansion.
    let message = detail.lines().next().unwrap_or(detail).trim();
    let message = message
        .rsplit_once(": ")
        .map(|(_, message)| message)
        .unwrap_or(message);
    let mut text = message.chars().take(150).collect::<String>();
    if message.chars().count() > 150 {
        text.push('…');
    }
    text
}

pub(crate) fn task_status(w: &Workspace) -> (String, String) {
    if let Some(prompt) = required_action(w) {
        return ("required".into(), prompt.into());
    }
    if let Some(status) = crate::assembly_motion_ui::compact_status(w) {
        return status;
    }
    if let Some(error) = w
        .session
        .draft
        .as_ref()
        .and_then(|draft| draft.error.as_deref())
    {
        return ("invalid".into(), concise_problem(error));
    }
    if w.fields.keys().any(|key| key.ends_with("source-invalid")) {
        return (
            "invalid".into(),
            "Correct the current source or references".into(),
        );
    }
    if !crate::quantity_display::source_fields_valid(w) {
        return ("invalid".into(), "Correct the highlighted value".into());
    }
    if w.confirm_requested {
        return ("pending".into(), "Validating before confirmation…".into());
    }
    let (label, state) = validation_summary(w);
    (state.into(), label.into())
}

pub(crate) fn diagnostic_details(w: &Workspace) -> String {
    let mut details = crate::operation_preview::tooltip(w)
        .lines()
        .map(str::to_owned)
        .filter(|line| !line.is_empty())
        .collect::<std::collections::BTreeSet<_>>();
    if let Some(detail) = crate::assembly_motion_ui::detail_status(w) {
        details.insert(detail);
    }
    details.extend(
        w.fields
            .iter()
            .filter(|(key, _)| {
                key.starts_with("operation-editor:invalid:")
                    || key.starts_with("structural-member-invalid:")
                    || key.ends_with("source-error")
            })
            .map(|(_, message)| message.clone()),
    );
    if w.fields.keys().any(|key| key.ends_with("source-invalid")) {
        details.insert(w.status.clone());
    }
    details.into_iter().collect::<Vec<_>>().join("\n")
}

pub(crate) fn disclosure(ui: &mut egui::Ui, w: &mut Workspace, group: &str, label: &str) -> bool {
    let open = w
        .fields
        .contains_key(&format!("task-panel:references:{group}"));
    if controls::button(
        ui,
        &format!("{group}/reference-expansion"),
        "task-reference-toggle",
        &["secondary-control"],
        &format!("{} {label}", if open { "▾" } else { "▸" }),
        true,
        open,
    )
    .clicked()
    {
        toggle_field(w, &format!("task-panel:references:{group}"));
    }
    w.fields
        .contains_key(&format!("task-panel:references:{group}"))
}

pub(crate) fn task_status_control(ui: &mut egui::Ui, w: &Workspace) {
    let (state, text) = task_status(w);
    let props = controls::properties(
        "workspace.task-status",
        "task-status",
        &["task-status"],
        &state,
    );
    if props.get("display").is_some_and(|v| v == "none") {
        return;
    }
    let height = controls::number(&props, "height", 24.).max(0.);
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), height),
        egui::Sense::hover(),
    );
    let mut row = ui.new_child(egui::UiBuilder::new().max_rect(rect).id_salt("task-status"));
    row.set_clip_rect(rect.intersect(ui.clip_rect()));
    controls::label(
        &mut row,
        "workspace.task-status",
        "task-status",
        &["task-status"],
        &text,
        &state,
    )
    .on_hover_text(if state == "invalid" {
        "Adjust the selected geometry or parameters; expand Diagnostics for the full report."
    } else {
        text.as_str()
    });
}

pub(crate) fn diagnostics(ui: &mut egui::Ui, w: &mut Workspace) {
    let details = diagnostic_details(w);
    if details.is_empty() {
        return;
    }
    let open = w.fields.contains_key(DIAGNOSTICS);
    if controls::button(
        ui,
        "workspace.task-diagnostics",
        "task-section-toggle",
        &["secondary-control"],
        if open {
            "▾ Diagnostics"
        } else {
            "▸ Diagnostics"
        },
        true,
        open,
    )
    .clicked()
    {
        toggle_field(w, DIAGNOSTICS);
    }
    if open {
        controls::label(
            ui,
            "workspace.task-diagnostic-details",
            "task-diagnostics",
            &["technical-details"],
            &details,
            "ready",
        );
    }
}

pub(crate) fn history_open(w: &Workspace) -> bool {
    w.fields.contains_key(HISTORY)
}

pub(crate) fn confirmation_preview(ui: &mut egui::Ui) {
    controls::label(
        ui,
        "workspace.theme-confirmation-preview",
        "theme-preview-heading",
        &[],
        "Completion control preview",
        "ready",
    );
    ui.horizontal_wrapped(|ui| {
        for (id, role, label) in [
            ("finish", "apply-action", "Finish"),
            ("save", "apply-action", "Save"),
            ("cancel", "cancel-action", "Cancel"),
        ] {
            controls::button(
                ui,
                &format!("theme-preview.{id}"),
                role,
                &["confirmation-control", "theme-preview"],
                label,
                true,
                false,
            )
            .on_hover_text("Appearance preview; no model changes");
        }
    });
    controls::label(
        ui,
        "workspace.theme-confirmation-preview-help",
        "theme-preview-help",
        &[],
        "Use auto width for labels that fit. Explicit CSS width, height and shape are preserved.",
        "ready",
    );
}
fn toggle_field(w: &mut Workspace, key: &str) {
    if w.fields.remove(key).is_none() {
        w.fields.insert(key.into(), "true".into());
    }
}

pub(crate) fn reference_controls(w: &Workspace, group: &str, label: &str) -> Vec<SemanticElement> {
    let expanded = w
        .fields
        .contains_key(&format!("task-panel:references:{group}"));
    let query = w
        .fields
        .get(&format!("task-panel:query:{group}"))
        .cloned()
        .unwrap_or_default();
    vec![
        SemanticElement {
            id: format!("{group}/reference-expansion"),
            role: "task-reference-toggle".into(),
            label: format!("Browse {label}"),
            value: None,
            state: if expanded { "selected" } else { "ready" }.into(),
            commands: vec!["workspace.task-reference-toggle".into()],
            relationships: vec![group.into()],
            data: serde_json::json!({"contract":"tcad.presentation.task-panel","version":1,"group":group,"document":w.session.visible().id,"expanded":expanded,"presentation_only":true}),
        },
        SemanticElement {
            id: format!("{group}/reference-search"),
            role: "task-reference-search".into(),
            label: format!("Find {label}"),
            value: Some(query.clone()),
            state: if expanded { "ready" } else { "inactive" }.into(),
            commands: vec![],
            relationships: vec![group.into()],
            data: serde_json::json!({"contract":"tcad.presentation.task-panel","version":1,"setter":"task-panel-query","group":group,"document":w.session.visible().id,"json":query,"value_type":"text","editable":true,"presentation_only":true}),
        },
    ]
}

pub(crate) fn reference_filter(
    ui: &mut egui::Ui,
    w: &mut Workspace,
    group: &str,
    label: &str,
    selected: usize,
    total: usize,
) -> Option<String> {
    let open = w
        .fields
        .contains_key(&format!("task-panel:references:{group}"));
    let title = format!(
        "{} {label} · {selected} selected / {total}",
        if open { "▾" } else { "▸" }
    );
    if controls::button(
        ui,
        &format!("{group}/reference-expansion"),
        "task-reference-toggle",
        &["secondary-control"],
        &title,
        true,
        open,
    )
    .clicked()
    {
        toggle_field(w, &format!("task-panel:references:{group}"));
    }
    if !w
        .fields
        .contains_key(&format!("task-panel:references:{group}"))
    {
        return None;
    }
    let element = reference_controls(w, group, label).remove(1);
    crate::ui::semantic_field(ui, w, &element);
    Some(
        w.fields
            .get(&format!("task-panel:query:{group}"))
            .cloned()
            .unwrap_or_default()
            .to_lowercase(),
    )
}
pub(crate) fn query_value(w: &Workspace, element: &SemanticElement) -> Option<serde_json::Value> {
    Some(
        element.data["group"]
            .as_str()
            .and_then(|group| w.fields.get(&format!("task-panel:query:{group}")))
            .cloned()
            .unwrap_or_default()
            .into(),
    )
}
pub(crate) fn set_query(
    w: &mut Workspace,
    element: &SemanticElement,
    value: serde_json::Value,
) -> cad_core::Result<()> {
    let Some(group) = element.data["group"]
        .as_str()
        .filter(|group| group.len() <= 512)
    else {
        return Err(cad_core::CadError::Invalid(
            "Reference browser identity is unavailable".into(),
        ));
    };
    if element.data["document"].as_str() != Some(w.session.visible().id.as_str()) {
        return Err(cad_core::CadError::Invalid(
            "Reference browser belongs to another document".into(),
        ));
    }
    let Some(query) = value.as_str().filter(|query| query.len() <= 4096) else {
        return Err(cad_core::CadError::Invalid(
            "Search text is too long".into(),
        ));
    };
    w.fields
        .insert(format!("task-panel:query:{group}"), query.into());
    Ok(())
}
pub(crate) fn task_action(w: &mut Workspace, command: &str, data: &serde_json::Value) -> bool {
    if command != "workspace.task-reference-toggle" {
        return false;
    }
    if data["document"].as_str() == Some(w.session.visible().id.as_str())
        && let Some(group) = data["group"].as_str().filter(|group| group.len() <= 512)
    {
        toggle_field(w, &format!("task-panel:references:{group}"));
    }
    true
}

pub(crate) fn in_place(w: &Workspace) -> bool {
    w.view.isolation
        && w.session
            .visible()
            .properties
            .get("document_kind")
            .is_none_or(|kind| kind != "part")
}

fn context(w: &Workspace) -> String {
    let document = w.session.visible();
    let mut text = w
        .view
        .active_part
        .as_ref()
        .and_then(|id| document.parts.get(id))
        .map(|part| part.name.clone())
        .unwrap_or_else(|| document.name.clone());
    if let Some(sketch) = &w.sketch {
        text.push_str(&format!(" › {} · Editing", sketch.name));
    } else if in_place(w) {
        text.push_str(" › Editing in place");
    }
    if let Some(cursor) = &w.view.history_cursor {
        let name = document
            .parts
            .get(&cursor.definition)
            .and_then(|part| {
                part.features
                    .iter()
                    .find(|feature| feature.id == cursor.node)
            })
            .map(|feature| feature.name.as_str())
            .unwrap_or("Feature");
        text.push_str(&format!(" › History: {name}"));
    }
    text
}

pub(crate) fn toolbar_state(w: &Workspace) -> &'static str {
    if w.tool_checkpoint.is_some() {
        "editing"
    } else if w.view.history_cursor.is_some() {
        "inspection"
    } else if w.selected.is_some() {
        "selected"
    } else {
        "ready"
    }
}

pub(crate) fn validation_summary(w: &Workspace) -> (&'static str, &'static str) {
    if let Some(prompt) = required_action(w) {
        return (prompt, "required");
    }
    match w.session.draft.as_ref() {
        Some(draft) if draft.pending => ("Evaluating proposed geometry", "pending"),
        Some(draft) if draft.error.is_some() => ("Preview failed", "invalid"),
        Some(draft)
            if draft
                .evaluation
                .as_ref()
                .is_some_and(|e| e.valid && e.identity == w.session.identity()) =>
        {
            ("Valid proposed geometry", "valid")
        }
        Some(_) => ("Waiting for current validation", "pending"),
        None if !w
            .session
            .evaluation
            .as_ref()
            .is_some_and(|e| e.identity == w.session.identity()) =>
        {
            ("Evaluating committed source", "pending")
        }
        None if w.session.evaluation.as_ref().is_some_and(|e| !e.valid) => {
            ("Committed source has failed features", "invalid")
        }
        None => ("Committed model", "valid"),
    }
}

pub(crate) fn semantics(w: &Workspace) -> Vec<SemanticElement> {
    let mut out = Vec::new();
    if w.dialog.as_deref() == Some("presentation") || w.settings.is_some() {
        for (id, role, label) in [
            (
                "workspace.theme-confirmation-preview",
                "theme-preview-heading",
                "Completion control preview",
            ),
            ("theme-preview.finish", "apply-action", "Finish"),
            ("theme-preview.save", "apply-action", "Save"),
            ("theme-preview.cancel", "cancel-action", "Cancel"),
        ] {
            out.push(SemanticElement {id:id.into(),role:role.into(),label:label.into(),value:None,state:"ready".into(),commands:vec![],relationships:vec!["workspace.theme-confirmation-preview".into()],data:serde_json::json!({"presentation_only":true,"display_only":true,"theme_preview":true,"classes":["confirmation-control","theme-preview"]})});
        }
    }
    if w.tool_checkpoint.is_some() || w.session.draft.is_some() {
        let (state, status) = task_status(w);
        for (id, role, label, value, command, open) in [
            (
                "workspace.task-header",
                "task-header",
                "Current operation",
                task_title(w),
                None,
                true,
            ),
            (
                "workspace.task-status",
                "task-status",
                "Operation status",
                status,
                None,
                true,
            ),
            (
                "workspace.task-diagnostics",
                "task-section-toggle",
                "Diagnostics",
                diagnostic_details(w),
                Some("workspace.task-diagnostics"),
                w.fields.contains_key(DIAGNOSTICS),
            ),
            (
                "workspace.task-history",
                "task-section-toggle",
                "Model history",
                String::new(),
                Some("workspace.task-history"),
                history_open(w),
            ),
            (
                "workspace.task-history-panel",
                "task-history-panel",
                "Model history",
                String::new(),
                None,
                history_open(w),
            ),
        ] {
            out.push(SemanticElement { id:id.into(), role:role.into(), label:label.into(), value:Some(value),
                state:if role == "task-status" { state.clone() } else if open { "open".into() } else { "closed".into() },
                commands:command.into_iter().map(str::to_owned).collect(), relationships:vec!["workspace.active_operation.parameters".into()],
                data:serde_json::json!({"contract":"tcad.presentation.task-panel","version":1,"presentation_only":true,"expanded":open,"renderer":role}) });
        }
    }
    for (id, role, label, renderer, available) in [
        (
            "viewport.toolbar",
            "viewport-toolbar",
            "Model context and view controls",
            "viewport-toolbar",
            true,
        ),
        (
            "viewport.start-hint",
            "viewport-start-hint",
            "Start a sketch",
            "viewport-start-hint",
            w.sketch.is_none() && w.tool_checkpoint.is_none(),
        ),
        (
            "viewport.view-cube",
            "view-cube",
            "View orientation cube",
            "view-cube",
            true,
        ),
        (
            "viewport.candidates",
            "selection-candidates",
            "Selection candidates",
            "selection-candidates",
            w.candidates.len() > 1,
        ),
        (
            "viewport.isolation",
            "isolation-controls",
            "Edit in place controls",
            "isolation-controls",
            in_place(w) && w.sketch.is_none(),
        ),
        (
            "viewport.navigation-hint",
            "navigation-hint",
            "Navigation instructions",
            "navigation-hint",
            true,
        ),
    ] {
        out.push(SemanticElement {
            id: id.into(), role: role.into(), label: label.into(), value: None,
            state: if !available { "inactive" } else if role == "viewport-toolbar" { toolbar_state(w) } else { "ready" }.into(),
            commands: vec![], relationships: vec!["workspace.viewport".into()],
            data: serde_json::json!({"contract":"tcad.presentation.workspace-controls","version":1,"renderer":renderer,"available":available}),
        });
    }
    for (id, role, label, value) in [
        (
            "workspace.model-context",
            "model-context-label",
            "Model context",
            context(w),
        ),
        (
            "workspace.selection-summary",
            "selection-summary",
            "Selection",
            selection_summary(w),
        ),
        (
            "workspace.candidate-summary",
            "selection-candidate-summary",
            "Pick candidate",
            candidate_summary(w),
        ),
        (
            "workspace.navigation-instructions",
            "navigation-instructions",
            "Navigation",
            crate::navigation::hint(w),
        ),
    ] {
        out.push(SemanticElement {
            id: id.into(), role: role.into(), label: label.into(), value: Some(value),
            state: "ready".into(), commands: vec![], relationships: vec!["workspace.viewport".into()],
            data: serde_json::json!({"contract":"tcad.presentation.workspace-controls","version":1}),
        });
    }
    for (id, role, label) in [
        (
            "workspace.history-context",
            "model-context-label",
            "Cached history",
        ),
        (
            "workspace.sketch-start-title",
            "workspace-heading",
            "Start your first sketch",
        ),
        (
            "workspace.sketch-start-description",
            "workspace-description",
            "Choose a plane",
        ),
    ] {
        out.push(SemanticElement { id: id.into(), role: role.into(), label: label.into(), value: None,
            state: "ready".into(), commands: vec![], relationships: vec!["workspace.viewport".into()], data: serde_json::json!({"contract":"tcad.presentation.workspace-controls","version":1}) });
    }
    let (validation, state) = validation_summary(w);
    for (id, role, label, value) in [
        (
            "workspace.validation-state",
            "validation-summary",
            "Validation",
            validation,
        ),
        (
            "workspace.status-message",
            "status-message",
            "Status",
            w.status.as_str(),
        ),
    ] {
        out.push(SemanticElement {id:id.into(),role:role.into(),label:label.into(),value:Some(value.into()),state:state.into(),commands:vec![],relationships:vec!["workspace.draft.status".into()],data:serde_json::json!({"contract":"tcad.presentation.workspace-controls","version":1,"current_identity":w.session.identity()})});
    }
    let idle = w.sketch.is_none() && w.tool_checkpoint.is_none() && w.settings.is_none();
    for (command, label, enabled, selected, parent) in [
        (
            "workspace.selection-actions",
            "Actions…",
            crate::selection_ui::current(w).is_some() && idle,
            false,
            "viewport.toolbar",
        ),
        (
            "workspace.ghost-others",
            "Ghost others",
            in_place(w) && w.sketch.is_none(),
            w.view.ghost_others,
            "viewport.isolation",
        ),
        (
            "workspace.hide-others",
            "Hide others",
            w.view.isolation && w.sketch.is_none(),
            !w.view.ghost_others,
            "viewport.isolation",
        ),
        (
            "workspace.reference-picking",
            "Pick references",
            in_place(w) && w.sketch.is_none(),
            w.fields.contains_key("pick-context-references"),
            "viewport.isolation",
        ),
        (
            "workspace.finish-editing",
            "Finish editing",
            in_place(w) && idle,
            false,
            "viewport.isolation",
        ),
        (
            "workspace.clear-pins",
            "Clear pins",
            !w.view.pinned.is_empty(),
            false,
            "viewport.isolation",
        ),
        (
            "workspace.sketch-front",
            "Front",
            idle && w.view.active_part.is_some(),
            false,
            "viewport.start-hint",
        ),
        (
            "workspace.sketch-top",
            "Top",
            idle && w.view.active_part.is_some(),
            false,
            "viewport.start-hint",
        ),
        (
            "workspace.sketch-right",
            "Right",
            idle && w.view.active_part.is_some(),
            false,
            "viewport.start-hint",
        ),
    ] {
        out.push(SemanticElement {
            id: format!("workspace-control/{command}"), role: "workspace-action".into(),
            label: label.into(), value: (!enabled).then(|| "Unavailable in the current editing context".into()),
            state: if !enabled { "disabled" } else if selected { "selected" } else { "ready" }.into(),
            commands: vec![command.into()], relationships: vec![parent.into()],
            data: serde_json::json!({"presentation_control":true,"classes":["semantic-control","viewport-control"],"enabled":enabled}),
        });
    }
    out
}

fn selection_summary(w: &Workspace) -> String {
    if w.geometry_selection.len() > 1 {
        format!("{} selected", w.geometry_selection.len())
    } else if let Some(selection) = &w.selected {
        format!("{} · {}", selection.name, selection.reference.topology)
    } else {
        crate::assembly_tree_ui::selection_label(w).unwrap_or_default()
    }
}

fn candidate_summary(w: &Workspace) -> String {
    if w.candidates.len() < 2 {
        return String::new();
    }
    format!(
        "Candidate {}/{} · Tab to cycle · {}",
        w.candidate_index + 1,
        w.candidates.len(),
        w.selected
            .as_ref()
            .map(|selection| selection.name.as_str())
            .unwrap_or("")
    )
}

fn button(
    ui: &mut egui::Ui,
    w: &mut Workspace,
    command: &str,
    label: &str,
    enabled: bool,
    selected: bool,
) {
    if controls::button(
        ui,
        &format!("workspace-control/{command}"),
        "workspace-action",
        &["semantic-control", "viewport-control"],
        label,
        enabled,
        selected,
    )
    .clicked()
    {
        ui::action(w, command);
    }
}

pub(crate) fn toolbar(ui: &mut egui::Ui, w: &mut Workspace) {
    if w.tool_checkpoint.is_none() {
        crate::fabrication_ui::state_controls(ui, w);
    }
    ui.horizontal(|ui| {
        controls::label(
            ui,
            "workspace.model-context",
            "model-context-label",
            &[],
            &context(w),
            "ready",
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            controls::menu_button(ui, "viewport.orientation-menu", "View", |ui| {
                crate::cad_ui::view_controls(ui, w)
            });
            button(ui, w, "viewport.fit", "Fit · F", true, false);
            if w.sketch.is_none() {
                crate::viewport::picking_controls(ui, w);
            }
        });
    });
    if w.view.history_cursor.is_some() {
        ui.horizontal(|ui| {
            controls::label(
                ui,
                "workspace.history-context",
                "model-context-label",
                &[],
                "Cached history",
                "inspection",
            );
            button(ui, w, "history.latest", "Latest", true, false);
        });
    }
    if w.tool_checkpoint.is_some() && !crate::presentation_composition::task_controls_hosted(w) {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            crate::cad_ui::confirmation(ui, w)
        });
    } else if w.selected.is_some() || w.selected_component.is_some() {
        ui.horizontal(|ui| {
            controls::label(
                ui,
                "workspace.selection-summary",
                "selection-summary",
                &[],
                &selection_summary(w),
                "selected",
            );
            button(
                ui,
                w,
                "workspace.selection-actions",
                "Actions…",
                true,
                false,
            );
        });
    }
}

pub(crate) fn start_hint(ui: &mut egui::Ui, w: &mut Workspace) {
    ui.vertical_centered(|ui| {
        controls::label(
            ui,
            "workspace.sketch-start-title",
            "workspace-heading",
            &[],
            "Start your first sketch",
            "ready",
        );
        controls::label(
            ui,
            "workspace.sketch-start-description",
            "workspace-description",
            &[],
            "Choose a plane",
            "ready",
        );
        ui.horizontal(|ui| {
            for (command, label) in [
                ("workspace.sketch-front", "Front"),
                ("workspace.sketch-top", "Top"),
                ("workspace.sketch-right", "Right"),
            ] {
                button(
                    ui,
                    w,
                    command,
                    label,
                    w.sketch.is_none()
                        && w.tool_checkpoint.is_none()
                        && w.view.active_part.is_some(),
                    false,
                );
            }
        });
    });
}

pub(crate) fn isolation(ui: &mut egui::Ui, w: &mut Workspace) {
    if !in_place(w) || w.sketch.is_some() {
        return;
    }
    ui.horizontal_wrapped(|ui| {
        button(
            ui,
            w,
            "workspace.ghost-others",
            "Ghost others",
            true,
            w.view.ghost_others,
        );
        button(
            ui,
            w,
            "workspace.hide-others",
            "Hide others",
            true,
            !w.view.ghost_others,
        );
        button(
            ui,
            w,
            "workspace.finish-editing",
            "Finish editing",
            w.tool_checkpoint.is_none(),
            false,
        );
        button(
            ui,
            w,
            "workspace.reference-picking",
            "Pick references",
            true,
            w.fields.contains_key("pick-context-references"),
        );
        if !w.view.pinned.is_empty() {
            button(ui, w, "workspace.clear-pins", "Clear pins", true, false);
        }
    });
}

pub(crate) fn navigation_hint(ui: &mut egui::Ui, w: &mut Workspace) {
    controls::label(
        ui,
        "workspace.navigation-instructions",
        "navigation-instructions",
        &[],
        &crate::navigation::hint(w),
        "ready",
    );
}

pub(crate) fn candidates(ui: &mut egui::Ui, w: &mut Workspace) {
    crate::viewport::candidate_controls(ui, w);
}

pub(crate) fn action(w: &mut Workspace, command: &str) -> bool {
    match command {
        "workspace.task-diagnostics" => {
            toggle_field(w, DIAGNOSTICS);
            return true;
        }
        "workspace.task-history" => {
            toggle_field(w, HISTORY);
            return true;
        }
        _ => {}
    }
    if !command.starts_with("workspace.") {
        return false;
    }
    let available = semantics(w)
        .into_iter()
        .find(|element| element.commands.iter().any(|id| id == command));
    let Some(available) = available else {
        return false;
    };
    if available.state == "disabled" {
        w.status = available.value.unwrap_or_default();
        return true;
    }
    match command {
        "workspace.selection-actions" => w.open_editor("selection"),
        "workspace.ghost-others" => {
            w.view.ghost_others = !w.view.ghost_others;
            w.rendered_key.clear();
        }
        "workspace.hide-others" => {
            w.view.ghost_others = false;
            w.rendered_key.clear();
        }
        "workspace.reference-picking" => {
            if w.fields.remove("pick-context-references").is_some() {
                w.status = "Pick active geometry; surrounding context is display only".into();
            } else {
                w.fields
                    .insert("pick-context-references".into(), "true".into());
                w.status = "Reference picking includes visible surrounding components".into();
            }
            w.candidates.clear();
            w.candidate_index = 0;
        }
        "workspace.finish-editing" => w.isolate(),
        "workspace.clear-pins" => {
            w.view.pinned.clear();
            w.rendered_key.clear();
        }
        "workspace.sketch-front" | "workspace.sketch-top" | "workspace.sketch-right" => {
            let plane = match command {
                "workspace.sketch-front" => "Front Plane",
                "workspace.sketch-top" => "Top Plane",
                _ => "Right Plane",
            };
            crate::sketch::choose_origin(w, plane);
            ui::action(w, "sketch.new");
        }
        _ => return false,
    }
    true
}
