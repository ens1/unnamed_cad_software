use crate::{Workspace, presentation_controls as controls};
use bevy_egui::egui;
use cad_core::{Part, commands::Command, presentation::SemanticElement};

#[derive(Clone)]
struct Drag {
    document: String,
    source: String,
    definition: String,
    feature: String,
}

fn blocked(w: &Workspace) -> Option<&'static str> {
    if w.settings.is_some() || w.recovery_draft.is_some() {
        Some("Finish Settings or recovery before reordering features")
    } else if w.sketch.is_some()
        || w.tool_checkpoint.is_some() && w.dialog.as_deref() != Some("feature-order")
    {
        Some("Confirm or cancel the current operation before reordering features")
    } else {
        None
    }
}

fn prepare(w: &mut Workspace, definition: &str, feature: &str) {
    w.start_tool();
    w.view.active_part = Some(definition.into());
    w.view.active_node = Some(feature.into());
    w.fields
        .insert("feature-order-owner".into(), definition.into());
    w.fields
        .insert("feature-order-feature".into(), feature.into());
    w.open_editor("feature-order");
}

pub(crate) fn move_feature(w: &mut Workspace, definition: &str, feature: &str, earlier: bool) {
    if let Some(reason) = blocked(w) {
        w.status = reason.into();
        return;
    }
    let destination = w
        .session
        .visible()
        .parts
        .get(definition)
        .ok_or_else(|| cad_core::CadError::Invalid("The selected part no longer exists".into()))
        .and_then(|part| cad_core::feature_order::destination(part, feature, earlier));
    match destination {
        Ok(position) => {
            prepare(w, definition, feature);
            w.dispatch(Command::ReorderFeature {
                definition: definition.into(),
                feature: feature.into(),
                position,
            });
        }
        Err(error) => w.status = error.to_string(),
    }
}

fn move_relative(w: &mut Workspace, definition: &str, feature: &str, anchor: &str, before: bool) {
    if let Some(reason) = blocked(w) {
        w.status = reason.into();
        return;
    }
    let destination = w
        .session
        .visible()
        .parts
        .get(definition)
        .ok_or_else(|| cad_core::CadError::Invalid("The selected part no longer exists".into()))
        .and_then(|part| {
            cad_core::feature_order::relative_destination(part, feature, anchor, before)
        });
    match destination {
        Ok(_) => {
            prepare(w, definition, feature);
            w.dispatch(Command::ReorderFeatureRelative {
                definition: definition.into(),
                feature: feature.into(),
                anchor: anchor.into(),
                before,
            });
        }
        Err(error) => w.status = error.to_string(),
    }
}

/// Gestures use actual styled row bounds and stable source identities.
pub(crate) fn row(
    ui: &mut egui::Ui,
    w: &mut Workspace,
    part: &Part,
    feature: &str,
    response: egui::Response,
) -> egui::Response {
    if blocked(w).is_some()
        || w.view.history_cursor.is_some()
        || response.rect == egui::Rect::NOTHING
    {
        return response;
    }
    let response = response.interact(egui::Sense::click_and_drag());
    if response.drag_started_by(egui::PointerButton::Primary) {
        response.dnd_set_drag_payload(Drag {
            document: w.session.visible().id.clone(),
            source: w.session.visible().fingerprint(),
            definition: part.id.clone(),
            feature: feature.into(),
        });
        w.selection_popup = None;
        crate::navigation::stop(w);
    }
    if let Some(payload) = response.dnd_hover_payload::<Drag>() {
        let before = ui
            .input(|input| input.pointer.interact_pos())
            .is_some_and(|point| point.y < response.rect.center().y);
        let reason = if payload.document != w.session.visible().id
            || payload.source != w.session.visible().fingerprint()
        {
            Some("The document changed during this drag; drag the feature again".into())
        } else if payload.definition != part.id {
            Some("Features can only be reordered within their owning part".into())
        } else {
            cad_core::feature_order::relative_destination(part, &payload.feature, feature, before)
                .err()
                .map(|error| error.to_string())
        };
        let props = controls::properties(
            "feature-order.drop-marker",
            "feature-drop-marker",
            &[],
            if reason.is_some() {
                "disabled"
            } else {
                "ready"
            },
        );
        if !props.get("display").is_some_and(|value| value == "none") {
            let color = props
                .get("color")
                .and_then(|value| crate::ui::color(value))
                .unwrap_or_else(|| {
                    if reason.is_some() {
                        ui.visuals().error_fg_color
                    } else {
                        ui.visuals().selection.stroke.color
                    }
                });
            let y = if before {
                response.rect.top()
            } else {
                response.rect.bottom()
            };
            ui.painter().line_segment(
                [
                    egui::pos2(response.rect.left(), y),
                    egui::pos2(response.rect.right(), y),
                ],
                egui::Stroke::new(controls::number(&props, "stroke-width", 2.).max(0.), color),
            );
        }
        response.clone().on_hover_ui_at_pointer(|ui| {
            controls::label(
                ui,
                "feature-order.drop-hint",
                "feature-drop-hint",
                &[],
                reason.as_deref().unwrap_or(if before {
                    "Move before this feature"
                } else {
                    "Move after this feature"
                }),
                "ready",
            );
        });
        if response.dnd_release_payload::<Drag>().is_some() {
            if let Some(reason) = reason {
                w.status = reason;
            } else {
                move_relative(w, &part.id, &payload.feature, feature, before);
            }
        }
    }
    response
}

pub(crate) fn scroll(ui: &egui::Ui) {
    if egui::DragAndDrop::payload::<Drag>(ui.ctx()).is_none() {
        return;
    }
    if let Some(pointer) = ui.input(|input| input.pointer.interact_pos()) {
        let bounds = ui.clip_rect();
        if !bounds.contains(pointer) {
            return;
        }
        let margin = 24_f32.min(bounds.height() * 0.25);
        let delta = if pointer.y < bounds.top() + margin {
            6.
        } else if pointer.y > bounds.bottom() - margin {
            -6.
        } else {
            0.
        };
        if delta != 0. {
            ui.scroll_with_delta(egui::vec2(0., delta));
            ui.ctx().request_repaint();
        }
    }
}

pub(crate) fn semantics(w: &Workspace) -> Vec<SemanticElement> {
    if w.dialog.as_deref() != Some("feature-order") {
        return vec![];
    }
    let Some(owner) = w.fields.get("feature-order-owner") else {
        return vec![];
    };
    let Some(feature) = w.fields.get("feature-order-feature") else {
        return vec![];
    };
    let Some(part) = w.session.visible().parts.get(owner) else {
        return vec![];
    };
    let source = w.session.visible().fingerprint();
    let from = part
        .features
        .iter()
        .position(|candidate| &candidate.id == feature)
        .unwrap_or(0);
    let range = cad_core::feature_order::destination_range(part, feature);
    let mut out = Vec::new();
    for (index, anchor) in part.features.iter().enumerate() {
        if &anchor.id == feature {
            continue;
        }
        for before in [true, false] {
            let position = index - usize::from(from < index) + usize::from(!before);
            let reason = match &range {
                Err(error) => Some(error.to_string()),
                Ok(_) if position == from => Some("The feature is already in that position".into()),
                Ok(range) if !range.contains(&position) => Some(
                    "This position would place a feature before one of its required inputs".into(),
                ),
                _ => None,
            };
            out.push(SemanticElement {
                id: format!("feature-order:{owner}:{feature}:{}:{}", anchor.id, if before { "before" } else { "after" }),
                role: "feature-reorder-target".into(), label: format!("{} {}", if before { "Before" } else { "After" }, anchor.name),
                value: reason.clone(), state: if reason.is_some() { "disabled" } else { "ready" }.into(),
                commands: vec!["feature.reorder-relative".into()], relationships: vec![owner.clone(), feature.clone(), anchor.id.clone()],
                data: serde_json::json!({"contract":"tcad.presentation.feature-order","version":2,"document":w.session.visible().id,
                    "source":source,"definition":owner,"feature":feature,"anchor":anchor.id,"before":before}),
            });
        }
    }
    out
}

pub(crate) fn action(w: &mut Workspace, command: &str, data: Option<&serde_json::Value>) -> bool {
    if command != "feature.reorder-relative" {
        return false;
    }
    let Some(data) = data else {
        w.status = "Choose a destination in the feature order manager".into();
        return true;
    };
    let Some(current) = semantics(w)
        .into_iter()
        .find(|element| &element.data == data)
    else {
        w.status = "The feature order changed; choose the destination again".into();
        return true;
    };
    if current.state == "disabled" {
        w.status = current.value.unwrap_or_default();
        return true;
    }
    if let (Some(owner), Some(feature), Some(anchor), Some(before)) = (
        data["definition"].as_str(),
        data["feature"].as_str(),
        data["anchor"].as_str(),
        data["before"].as_bool(),
    ) {
        move_relative(w, owner, feature, anchor, before);
    }
    true
}

pub(crate) fn show(ui: &mut egui::Ui, w: &mut Workspace) {
    controls::label(
        ui,
        "feature-order.description",
        "editor-hint",
        &[],
        "Preview the new order. Apply commits once; Cancel restores the starting order.",
        "ready",
    );
    for element in semantics(w) {
        if controls::button(
            ui,
            &element.id,
            &element.role,
            &["semantic-control"],
            &element.label,
            element.state != "disabled",
            false,
        )
        .on_disabled_hover_text(element.value.as_deref().unwrap_or(""))
        .clicked()
        {
            action(w, "feature.reorder-relative", Some(&element.data));
            break;
        }
    }
}
