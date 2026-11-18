//! View-only history commands. No evaluation request or document command is issued here.
use crate::{Workspace, command_icons, presentation_controls as controls, ui};
use bevy_egui::egui;
use cad_core::{
    NodeRef, presentation::SemanticElement, protocol::Evaluation,
    selection_actions::SelectionTarget,
};
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
        w.edge_selection_path = None;
    }
    w.view.active_part = Some(target.definition.clone());
    w.view.active_node = Some(target.node.clone());
    w.context_selection = Some(SelectionTarget::Feature {
        definition: target.definition.clone(),
        feature: target.node.clone(),
    });
    w.view.history_cursor = Some(target);
    w.selected = None;
    w.selected_component = None;
    w.geometry_selection.clear();
    w.selected_feature_path = None;
    w.selection_popup = None;
    w.candidates.clear();
    w.active_plane = None;
    w.gesture = None;
    w.rendered_key.clear();
    w.status = format!("Viewing the model after {name}");
    true
}
pub fn latest(w: &mut Workspace) -> bool {
    if w.view.history_cursor.take().is_none() {
        return false;
    }
    w.selected = None;
    w.selected_component = None;
    w.geometry_selection.clear();
    w.context_selection = None;
    w.selected_feature_path = None;
    w.selection_popup = None;
    w.candidates.clear();
    w.active_plane = None;
    w.gesture = None;
    w.rendered_key.clear();
    w.status = "Showing the latest model".into();
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

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum StationLayout {
    List,
    Timeline,
}

pub(crate) fn return_control(ui: &mut egui::Ui, w: &mut Workspace, id: &str, role: &str) {
    if cad_core::history::latest_disabled_reason(
        &w.view,
        w.tool_checkpoint.is_some() || w.sketch.is_some(),
    )
    .is_some()
    {
        return;
    }
    let icon =
        |p: &egui::Painter, c, s, color| command_icons::paint(p, c, s, color, "history.latest");
    if controls::icon_button(
        ui,
        id,
        role,
        &["history-return"],
        "Return to latest",
        true,
        false,
        Some(&icon),
    )
    .on_hover_text("Show the latest model (Esc)")
    .clicked()
    {
        ui::action(w, "history.latest");
    }
}

pub(crate) fn browser_label(w: &Workspace) -> &'static str {
    if w.view.active_part.is_some() {
        "Model features"
    } else {
        "Assembly components"
    }
}

pub(crate) fn empty(ui: &mut egui::Ui) {
    controls::label(
        ui,
        "timeline:empty",
        "timeline-empty",
        &[],
        "No features yet",
        "ready",
    );
}

/// Both orientations use the same station descriptors and command routing.
/// Only the list uses links between the stations; the timeline shows tool icons.
pub(crate) fn stations(
    ui: &mut egui::Ui,
    w: &mut Workspace,
    elements: &[SemanticElement],
    layout: StationLayout,
) {
    let part = w.view.active_part.clone();
    let mut stations = elements
        .iter()
        .filter(|item| {
            item.role == "history-station" && item.relationships.first() == part.as_ref()
        })
        .peekable();
    let scroll_id = ui.id().with("history-scroll-to");
    if stations.peek().is_none() {
        if part.is_none() {
            controls::label(
                ui,
                "timeline:empty",
                "timeline-empty",
                &[],
                "Select a part to see its history",
                "ready",
            );
        } else {
            empty(ui);
        }
        ui.data_mut(|data| data.remove::<String>(scroll_id));
        return;
    }
    if w.view.history_cursor.is_none() {
        ui.data_mut(|data| data.remove::<String>(scroll_id));
    }
    let mut previous_bottom = None;
    for element in stations {
        let active = element.state == "selected";
        let (response, marker) = station_button(ui, element, layout);
        let response = response
            .on_hover_ui(|ui| station_tooltip(ui, w, element))
            .on_disabled_hover_text(element.value.as_deref().unwrap_or("History unavailable"));
        if let Some((top, bottom)) = marker {
            if let Some(last) = previous_bottom {
                connect(ui, element, last, top);
            }
            previous_bottom = Some(bottom);
        }
        if active
            && ui.data_mut(|data| {
                let last = data.get_temp_mut_or_default::<String>(scroll_id);
                if last == &element.id {
                    return false;
                }
                last.clone_from(&element.id);
                true
            })
        {
            response.scroll_to_me(None);
        }
        let feature_target = || {
            Some(SelectionTarget::Feature {
                definition: element.relationships.first()?.clone(),
                feature: element.relationships.get(1)?.clone(),
            })
        };
        if response.double_clicked() {
            if let Some(target) = feature_target() {
                crate::selection_ui::execute(w, &target, "feature.edit");
            }
        } else if response.clicked() {
            ui::semantic_action(w, element, "history.inspect");
        }
        response.context_menu(|ui| {
            if let Some(target) = feature_target() {
                crate::selection_ui::contents(ui, w, &target);
            }
        });
    }
}

fn station_button(
    ui: &mut egui::Ui,
    element: &SemanticElement,
    layout: StationLayout,
) -> (egui::Response, Option<(egui::Pos2, egui::Pos2)>) {
    let active = element.state == "selected";
    let enabled = element.state != "disabled";
    let stage = element.data["stage"].as_str().unwrap_or("included");
    let class = match layout {
        StationLayout::List => "history-row",
        StationLayout::Timeline => "timeline-station",
    };
    let classes = [class, stage];
    let marker = std::cell::Cell::new(None);
    let draw = |p: &egui::Painter, center, size: f32, color| {
        if layout == StationLayout::Timeline {
            let icon = element.data["icon_command"]
                .as_str()
                .unwrap_or("history.inspect");
            command_icons::paint(p, center, size, color, icon);
            return;
        }
        let radius = if active { size * 0.43 } else { size * 0.30 };
        let end = egui::vec2(0., radius);
        marker.set(Some((center - end, center + end)));
        let props = controls::properties(
            &element.id,
            "history-station",
            &classes,
            if active {
                "selected"
            } else if enabled {
                "ready"
            } else {
                "disabled"
            },
        );
        let stroke = egui::Stroke::new(controls::number(&props, "stroke-width", 1.2), color);
        if stage == "future" || active {
            p.circle_stroke(center, radius, stroke);
        }
        if stage != "future" {
            p.circle_filled(center, if active { size * 0.19 } else { radius }, color);
        }
    };
    let response = controls::icon_button(
        ui,
        &element.id,
        "history-station",
        &classes,
        &element.label,
        enabled,
        active,
        Some(&draw),
    );
    response.ctx.accesskit_node_builder(response.id, |node| {
        node.set_label(
            element.data["accessibility_label"]
                .as_str()
                .unwrap_or(&element.label),
        );
        node.set_toggled(if active {
            egui::accesskit::Toggled::True
        } else {
            egui::accesskit::Toggled::False
        });
    });
    (response, marker.get())
}

fn station_tooltip(ui: &mut egui::Ui, w: &Workspace, element: &SemanticElement) {
    let index = element.data["index"].as_u64().unwrap_or_default() as usize;
    let total = element.data["total"].as_u64().unwrap_or_default();
    let detail = element
        .relationships
        .first()
        .and_then(|id| w.session.visible().parts.get(id))
        .and_then(|part| part.features.get(index))
        .map(|feature| crate::cad_ui::operation_summary(&feature.operation));
    let step = format!("Step {} of {total}", index + 1);
    ui.label(detail.map_or_else(|| step.clone(), |detail| format!("{step} · {detail}")));
    if element.data["stage"] == "future" {
        ui.small("After the viewed step");
    }
    ui.small("Double-click to edit");
}

fn connect(ui: &egui::Ui, element: &SemanticElement, last: egui::Pos2, top: egui::Pos2) {
    let stage = element.data["stage"].as_str().unwrap_or("included");
    let owner = element
        .relationships
        .first()
        .map(String::as_str)
        .unwrap_or_default();
    let path = controls::properties(
        &format!("history-path:{owner}"),
        "history-path",
        &[stage],
        "ready",
    );
    let color = path
        .get("color")
        .and_then(|value| ui::color(value))
        .unwrap_or_else(|| ui.visuals().weak_text_color());
    ui.painter().line_segment(
        [last, top],
        egui::Stroke::new(controls::number(&path, "stroke-width", 1.), color),
    );
}
