//! Identify the equations holding a drag and expose their existing edit commands at the handle.
use super::*;
use crate::{presentation_controls as controls, sketch_relations_ui as relations};
use cad_core::{presentation::SemanticElement, sketch_actions as actions};
use std::collections::BTreeSet;

pub struct DragLimit {
    anchor: [f64; 2],
    loads: Vec<(Id, [f64; 2])>,
    pub constraints: Vec<Id>,
    reason: String,
}

pub(super) fn explain(
    w: &Workspace,
    gesture: &DragGesture,
    solved: &Sketch,
    goal: [f64; 2],
    viewport_rect: egui::Rect,
) -> Option<DragLimit> {
    let s = w.sketch.as_ref()?;
    let handle = &gesture.handle;
    let delta = [goal[0] - handle.position[0], goal[1] - handle.position[1]];
    let keys: BTreeSet<&str> = match &handle.target {
        HandleTarget::Point { point } => [point.as_str()].into_iter().collect(),
        HandleTarget::MoveMember { member } => solved
            .members
            .get(member)?
            .curve
            .points()
            .into_iter()
            .collect(),
        _ => BTreeSet::new(),
    };
    let project = |p| viewport::project(w, viewport_rect, sketch::world(&s.plane, &s.matrix, p));
    let mut loads = vec![];
    let mut anchor = handle.position;
    let count = keys.len() as f64;
    for key in keys {
        let old = gesture.display.points.get(key)?;
        let current = *solved.points.get(key)?;
        let requested = [old[0] + delta[0], old[1] + delta[1]];
        for axis in 0..2 {
            anchor[axis] += (current[axis] - old[axis]) / count;
        }
        if project(current)?.distance(project(requested)?) > 3. {
            loads.push((
                key.to_owned(),
                [requested[0] - current[0], requested[1] - current[1]],
            ));
        }
    }
    if loads.is_empty()
        && (handle.disabled_reason.is_none()
            || project(handle.position)?.distance(project(goal)?) <= 3.)
    {
        return None;
    }
    let mut constraints = vec![];
    for c in solved.constraints.iter().filter(|c| !c.driven) {
        let direct = match &handle.target {
            HandleTarget::CircleRadius { member } => {
                c.entities.first() == Some(member)
                    && matches!(c.kind.as_str(), "radius" | "diameter")
                    || c.kind == "equal_radius" && c.entities.get(1) == Some(member)
            }
            HandleTarget::EllipseMajor { member } => {
                c.kind == "major_radius" && c.entities.first() == Some(member)
            }
            HandleTarget::EllipseMinor { member } => {
                c.kind == "minor_radius" && c.entities.first() == Some(member)
            }
            HandleTarget::EllipseRotation { member } => {
                c.kind == "rotation" && c.entities.first() == Some(member)
            }
            _ => loads.iter().any(|(point, d)| {
                c.entities.first() == Some(point)
                    && match c.kind.as_str() {
                        "fix_x" => d[0].abs() > d[0].hypot(d[1]) * 1e-4,
                        "fix_y" => d[1].abs() > d[0].hypot(d[1]) * 1e-4,
                        "origin" => true,
                        _ => false,
                    }
            }),
        };
        if direct {
            constraints.push(c.id.clone());
        }
    }
    Some(DragLimit {
        anchor,
        loads,
        constraints,
        reason: handle
            .disabled_reason
            .clone()
            .unwrap_or_else(|| "Movement limited by sketch relations".into()),
    })
}

// Use only worker results for the current draft. This also fills in indirect
// constraints when the result arrives after releasing the pointer.
pub(crate) fn refresh(s: &mut sketch::EditingSketch) {
    let Some(limit) = &s.drag_limit else { return };
    if limit.loads.is_empty() {
        return;
    }
    if let Some(ids) = relations::motion(s).and_then(|a| a.drag_blockers(&limit.loads))
        && !ids.is_empty()
    {
        s.drag_limit.as_mut().unwrap().constraints = ids;
    }
}

pub fn semantics(w: &Workspace) -> Vec<SemanticElement> {
    let Some(s) = &w.sketch else { return vec![] };
    let Some(limit) = &s.drag_limit else {
        return vec![];
    };
    let prefix = format!("{}::drag-limit", s.sketch_id);
    let mut out = vec![SemanticElement {
        id: prefix.clone(),
        role: "sketch-drag-limit".into(),
        label: "Movement limited".into(),
        value: Some(limit.reason.clone()),
        state: "blocked".into(),
        commands: vec![],
        relationships: limit.constraints.clone(),
        data: serde_json::json!({"sketch":s.sketch_id,"position_mm":limit.anchor,"constraints":limit.constraints,"frame":"sketch-XY"}),
    }];
    for id in &limit.constraints {
        let Some(c) = s.constraints.iter().find(|c| &c.id == id && !c.driven) else {
            continue;
        };
        let dimension = actions::is_dimension(&c.kind);
        let command = if dimension {
            "sketch.edit-constraint"
        } else {
            "sketch.select-entity"
        };
        let label = if dimension {
            let unit = actions::dimension_unit(&c.kind);
            let value = expression::value(w.session.visible(), &s.definition, &c.value, unit)
                .map(|value| {
                    if unit == Dimension::Angle {
                        w.session.visible().settings.format_angle_radians(value)
                    } else {
                        w.session.visible().settings.format_length(value)
                    }
                })
                .unwrap_or_else(|_| c.value.0.clone());
            format!("{} · {value}", actions::label(&c.kind))
        } else {
            actions::label(&c.kind).into()
        };
        out.push(SemanticElement {
            id: format!("{prefix}.{id}"),
            role: "sketch-drag-driver".into(),
            label,
            value: None,
            state: if s.edit.is_none() && s.drag.is_none() {
                "ready"
            } else {
                "disabled"
            }
            .into(),
            commands: vec![command.into()],
            relationships: vec![s.sketch_id.clone(), id.clone()],
            data: serde_json::json!({"sketch":s.sketch_id,"constraint":id,"dimension":dimension}),
        });
    }
    out
}

pub(super) fn overlay(w: &mut Workspace, ui: &mut egui::Ui, viewport_rect: egui::Rect) -> bool {
    let Some(s) = &w.sketch else { return false };
    let Some(limit) = &s.drag_limit else {
        return false;
    };
    let Some(anchor) = viewport::project(
        w,
        viewport_rect,
        sketch::world(&s.plane, &s.matrix, limit.anchor),
    ) else {
        return false;
    };
    if !viewport_rect.contains(anchor) {
        return false;
    }
    let elements = semantics(w);
    let (panel, drivers) = elements
        .split_first()
        .expect("Active drag limit has a panel");
    let Some((rect, chosen, hovered)) =
        panel_ui(ui, viewport_rect, anchor, panel, drivers, 0., |_| {})
    else {
        return false;
    };
    let chosen = chosen.map(|index| &drivers[index]);
    if let Some(driver) = chosen {
        let command = &driver.commands[0];
        relations::semantic_action(w, driver, command);
        if let Some(editor) = w.sketch.as_mut().and_then(|s| s.dimension.as_mut()) {
            editor.nearby_anchor = Some([
                rect.left() - viewport_rect.left(),
                rect.top() - viewport_rect.top(),
            ]);
        }
    }
    hovered || chosen.is_some()
}

/// Both sketch editors use the same theme roles, placement and input controls.
pub(crate) fn panel_ui(
    ui: &mut egui::Ui,
    viewport_rect: egui::Rect,
    anchor: egui::Pos2,
    panel: &SemanticElement,
    drivers: &[SemanticElement],
    editor_height: f32,
    editor: impl FnOnce(&mut egui::Ui),
) -> Option<(egui::Rect, Option<usize>, bool)> {
    let height = if drivers.is_empty() {
        72.
    } else {
        30. + 32. * drivers.len().min(4) as f32
    };
    let size = egui::vec2(226., height + editor_height).min(viewport_rect.size());
    let min = (anchor + egui::vec2(14., 14.)).clamp(viewport_rect.min, viewport_rect.max - size);
    let rect = controls::overlay_rect_state(
        &panel.id,
        &panel.role,
        &panel.state,
        viewport_rect,
        egui::Rect::from_min_size(min, size),
    )?;
    let props = controls::properties(&panel.id, &panel.role, &["viewport-overlay"], &panel.state);
    let mut chosen = None;
    let shown = egui::Area::new(ui.make_persistent_id(&panel.id))
        .constrain_to(viewport_rect)
        .fixed_pos(rect.min)
        .order(egui::Order::Foreground)
        .movable(false)
        .show(ui.ctx(), |ui| {
            ui.set_max_width(rect.width());
            controls::apply_properties(ui, &props);
            let mut frame = controls::frame_from_properties(ui, &props);
            if !props.contains_key("background") {
                frame.fill = ui.visuals().panel_fill;
            }
            frame.show(ui, |ui| {
                controls::label(
                    ui,
                    &format!("{}.title", panel.id),
                    "sketch-drag-limit-title",
                    &["compact"],
                    &panel.label,
                    &panel.state,
                );
                if drivers.is_empty() {
                    if let Some(reason) = &panel.value {
                        ui.small(reason);
                    }
                } else {
                    egui::ScrollArea::vertical()
                        .id_salt(&panel.id)
                        .max_height((rect.height() - editor_height - 30.).max(18.))
                        .show(ui, |ui| {
                            for (index, driver) in drivers.iter().enumerate() {
                                if controls::button(
                                    ui,
                                    &driver.id,
                                    &driver.role,
                                    &["compact"],
                                    &driver.label,
                                    driver.state != "disabled",
                                    false,
                                )
                                .clicked()
                                {
                                    chosen = Some(index);
                                }
                            }
                        });
                }
                editor(ui);
            });
        });
    Some((
        shown.response.rect,
        chosen,
        shown.response.contains_pointer(),
    ))
}
