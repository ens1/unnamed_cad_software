//! Compact workspace controls projected independently of their default overlays.
use crate::{Workspace, presentation_controls as controls, ui};
use bevy_egui::egui;
use cad_core::presentation::SemanticElement;

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
    } else if w.view.isolation {
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
            w.view.isolation && w.sketch.is_none(),
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
            w.view.isolation && w.sketch.is_none(),
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
            "workspace.finish-editing",
            "Finish editing",
            w.view.isolation && idle,
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
        String::new()
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
    if w.tool_checkpoint.is_some() {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            crate::cad_ui::confirmation(ui, w)
        });
    } else if w.selected.is_some() {
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
    if !w.view.isolation || w.sketch.is_some() {
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
    controls::label(
        ui,
        "workspace.candidate-summary",
        "selection-candidate-summary",
        &[],
        &candidate_summary(w),
        "ready",
    );
}

pub(crate) fn action(w: &mut Workspace, command: &str) -> bool {
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
