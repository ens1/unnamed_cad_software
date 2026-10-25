//! Native presentation and gesture capture for the application-owned handle contract.
use crate::{Workspace, sketch, sketch_ui, viewport};
use bevy_egui::egui::{self, Color32, Stroke};
use cad_core::{
    sketch_actions::SketchEntity,
    sketch_handles::{self as handles, HandleTarget, SketchHandle},
    *,
};
use std::time::Instant;

pub struct DragGesture {
    viewport: egui::Id,
    handle: SketchHandle,
    source: Sketch,
    display: Sketch,
    session: Session,
    selection: Vec<SketchEntity>,
    error: Option<String>,
    degrees_of_freedom: Option<usize>,
    feedback_state: crate::sketch_relations_ui::Feedback,
    history_len: usize,
    grab: [f64; 2],
    last_goal: Option<[f64; 2]>,
    feedback: String,
}
fn changed(a: &Sketch, b: &Sketch) -> bool {
    a.blocks != b.blocks
        || a.block_instances != b.block_instances
        || a.members != b.members
        || a.constraints != b.constraints
        || a.points.len() != b.points.len()
        || a.points.iter().any(|(key, p)| {
            b.points
                .get(key)
                .is_none_or(|q| (p[0] - q[0]).hypot(p[1] - q[1]) > 1e-8)
        })
}
/// Release capture; the gesture stays in the sketch's pending transaction.
pub fn end(w: &mut Workspace) {
    if let Some(s) = &mut w.sketch
        && let Some(gesture) = s.drag.take()
    {
        if !changed(&gesture.source, &sketch::sketch_definition(s)) {
            s.history.truncate(gesture.history_len);
        }
        w.gesture = None;
    }
}
pub fn cancel(w: &mut Workspace) -> bool {
    let Some(gesture) = w.sketch.as_mut().and_then(|s| s.drag.take()) else {
        return false;
    };
    w.worker.cancel();
    w.requested = None;
    w.confirm_requested = false;
    w.session = gesture.session;
    w.changed = Instant::now();
    w.rendered_key.clear();
    if let Some(s) = &mut w.sketch {
        s.history.truncate(gesture.history_len);
        sketch::restore(s, gesture.source);
        s.last_valid = Some(gesture.display);
        s.degrees_of_freedom = gesture.degrees_of_freedom;
        s.feedback = gesture.feedback_state;
        s.error = gesture.error;
        s.selection = gesture.selection;
        s.selected = s.selection.iter().find_map(|e| match e {
            SketchEntity::Point { point } => s.keys.iter().position(|key| key == point),
            _ => None,
        });
        s.selected_member = s.selection.iter().find_map(|e| match e {
            SketchEntity::Member { member } => Some(member.clone()),
            _ => None,
        });
    }
    w.gesture = None;
    w.status = "Drag canceled; earlier sketch edits retained".into();
    true
}
fn start(w: &mut Workspace, handle: SketchHandle, grab: [f64; 2], viewport: egui::Id) {
    let Some(s) = &w.sketch else {
        return;
    };
    let session = w.session.clone();
    let source = sketch::sketch_definition(s);
    let display = sketch::display_definition(s);
    let selection = s.selection.clone();
    let error = s.error.clone();
    let degrees_of_freedom = s.degrees_of_freedom;
    let feedback_state = s.feedback.clone();
    let history_len = s.history.len();
    let entity = handle.target.entity();
    sketch_ui::select(w, Some(entity), false);
    w.fields.remove("smart-dimension-mode");
    w.selection_popup = None;
    w.context_selection = None;
    w.confirm_requested = false;
    w.gesture = None;
    if let Some(s) = &mut w.sketch {
        s.dimension = None;
        s.history.push(source.clone());
        s.drag = Some(DragGesture {
            viewport,
            handle,
            source,
            display,
            session,
            selection,
            error,
            degrees_of_freedom,
            feedback_state,
            history_len,
            grab,
            last_goal: None,
            feedback: "Drag to edit · Alt bypasses snap · Esc cancels".into(),
        });
    }
}
fn update(w: &mut Workspace, cursor: [f64; 2], snap: bool) {
    let Some(s) = &w.sketch else {
        return;
    };
    let Some(g) = &s.drag else {
        return;
    };
    let mut goal = [
        g.handle.position[0] + cursor[0] - g.grab[0],
        g.handle.position[1] + cursor[1] - g.grab[1],
    ];
    if snap && s.snap {
        goal = goal.map(|v| (v / s.grid).round() * s.grid);
    }
    if g.last_goal == Some(goal) {
        return;
    }
    let handle = g.handle.clone();
    let mut source = g.source.clone();
    let result = handles::drag(
        &cad_core::solver::NativeSolver,
        w.session.visible(),
        &s.definition,
        &mut source,
        &handle,
        goal,
    );
    match result {
        Ok(_) => {
            let did_change = w
                .sketch
                .as_ref()
                .is_some_and(|s| changed(&source, &sketch::sketch_definition(s)));
            if let Some(s) = &mut w.sketch {
                if did_change {
                    if let Some(editor) = &mut s.edit
                        && matches!(
                            editor.kind,
                            crate::sketch_edit_ui::Kind::Spline
                                | crate::sketch_edit_ui::Kind::Conic
                                | crate::sketch_edit_ui::Kind::Text
                        )
                    {
                        editor.spline_points = source.points.clone();
                    }
                    s.points = s.keys.iter().map(|key| source.points[key]).collect();
                    s.blocks = source.blocks.clone();
                    s.block_instances = source.block_instances.clone();
                    if let Some(editor) =
                        s.edit.as_mut().and_then(|editor| editor.equation.as_mut())
                    {
                        editor.refresh(&source);
                    }
                    if let Some(block) = s.edit.as_mut().and_then(|editor| editor.block.as_mut()) {
                        block.refresh(&source);
                    }
                    s.members = source.members;
                }
                if let Some(g) = &mut s.drag {
                    g.last_goal = Some(goal);
                }
            }
            if did_change {
                sketch::solve(w);
            }
            let limited = match &handle.target {
                HandleTarget::Point { point } => w
                    .sketch
                    .as_ref()
                    .and_then(|s| {
                        s.keys
                            .iter()
                            .position(|key| key == point)
                            .map(|i| s.points[i])
                    })
                    .is_some_and(|p| (p[0] - goal[0]).hypot(p[1] - goal[1]) > 1e-4),
                _ => false,
            };
            let feedback = if limited {
                "Movement limited by dimensions / relations · Esc cancels"
            } else {
                "Live sketch preview · release to keep pending · Esc cancels"
            };
            if let Some(g) = w.sketch.as_mut().and_then(|s| s.drag.as_mut()) {
                g.feedback = feedback.into();
            }
            w.status = feedback.into();
        }
        Err(error) => {
            let feedback = format!("{error} · last valid sketch retained");
            if let Some(g) = w.sketch.as_mut().and_then(|s| s.drag.as_mut()) {
                g.last_goal = Some(goal);
                g.feedback = feedback.clone();
            }
            w.status = feedback;
        }
    }
}
pub fn overlay(w: &mut Workspace, ui: &mut egui::Ui, rect: egui::Rect) -> bool {
    let accent = viewport::selection_color(w);
    let Some(s) = &w.sketch else {
        return false;
    };
    if s.choosing_plane || s.tool != sketch::Tool::Select {
        return false;
    }
    if !ui.input(|i| i.focused) {
        return cancel(w);
    }
    let plane = s.plane.clone();
    let matrix = s.matrix;
    let source = sketch::sketch_definition(s);
    let display = sketch::display_definition(s);
    let selection = s.selection.clone();
    let related = crate::sketch_relations_ui::related_entities(s);
    let problems = crate::sketch_relations_ui::problem_entities(w);
    let point_motion = crate::sketch_relations_ui::motion(s)
        .map(|analysis| analysis.points.clone())
        .unwrap_or_default();
    let motion_keys = crate::sketch_relations_ui::selection_keys(&source, &selection);
    let fully_constrained = s.degrees_of_freedom == Some(0);
    let active_id = s.drag.as_ref().map(|g| g.handle.id.clone());
    let mut consumed = active_id.is_some();
    let painter = ui.painter().with_clip_rect(rect);
    if crate::sketch_relations_ui::motion_display(w) && active_id.is_none() {
        for (key, motion) in &point_motion {
            if !crate::sketch_overlay::point_visible(w, &display, key) {
                continue;
            }
            if !selection.is_empty() && !motion_keys.contains(key) {
                continue;
            }
            let Some(point) = display.points.get(key) else {
                continue;
            };
            let Some(pos) = viewport::project(w, rect, sketch::world(&plane, &matrix, *point))
            else {
                continue;
            };
            if !rect.contains(pos) {
                continue;
            }
            let state = if motion.directions.is_empty() {
                "constrained"
            } else if motion.directions.len() == 1 {
                "one-direction"
            } else {
                "two-directions"
            };
            let region = presentation::Region {
                id: format!("{}::motion.{key}", s.sketch_id),
                role: "sketch-point-motion".into(),
                component: None,
                rect: [pos.x, pos.y, 0., 0.],
                classes: vec![
                    if motion.generated {
                        "generated"
                    } else {
                        "source"
                    }
                    .into(),
                ],
                resizable: false,
            };
            let props = w.theme.properties(&region, &w.view.active_layout, state);
            if props.get("display").is_some_and(|value| value == "none")
                || props
                    .get("visibility")
                    .is_some_and(|value| value == "hidden")
            {
                continue;
            }
            let number = |key: &str, default: f32| {
                props
                    .get(key)
                    .and_then(|value| value.trim_end_matches("px").parse::<f32>().ok())
                    .filter(|value| value.is_finite())
                    .unwrap_or(default)
            };
            let length = number("size", 17.).clamp(6., 64.);
            let color = props
                .get("color")
                .and_then(|value| crate::ui::color(value))
                .unwrap_or(accent);
            let stroke = Stroke::new(number("stroke-width", 1.3).clamp(0.5, 6.), color);
            if motion.directions.is_empty() {
                let mark = pos + egui::vec2(6., -6.);
                painter.rect_stroke(
                    egui::Rect::from_center_size(mark, egui::vec2(4., 4.)),
                    0.,
                    stroke,
                    egui::StrokeKind::Middle,
                );
            }
            for direction in &motion.directions {
                let endpoint = [point[0] + direction[0], point[1] + direction[1]];
                if let Some(projected) =
                    viewport::project(w, rect, sketch::world(&plane, &matrix, endpoint))
                {
                    let delta = projected - pos;
                    if delta.length_sq() < 1e-10 {
                        continue;
                    }
                    let delta = delta.normalized() * length;
                    painter.line_segment([pos - delta, pos + delta], stroke);
                    for sign in [-1., 1.] {
                        let tip = pos + delta * sign;
                        let along = delta.normalized() * sign;
                        let side = egui::vec2(-along.y, along.x);
                        painter.line_segment([tip, tip - along * 4. + side * 2.5], stroke);
                        painter.line_segment([tip, tip - along * 4. - side * 2.5], stroke);
                    }
                }
            }
        }
    }
    // A selected spline exposes its actual control polygon, not tessellation vertices.
    for (key, member) in &display.members {
        if !w.view.visibility.sketch_points
            || !crate::sketch_overlay::member_visible(w, member.construction)
        {
            continue;
        }
        if let SketchCurve::Bezier { points } | SketchCurve::Nurbs { points, .. } = &member.curve
            && selection.iter().any(|e| {
                matches!(e, SketchEntity::Member { member } if member == key)
                    || matches!(e, SketchEntity::Point { point } if points.contains(point))
            })
        {
            for pair in points.windows(2) {
                if let (Some(a), Some(b)) = (
                    viewport::project(
                        w,
                        rect,
                        sketch::world(&plane, &matrix, display.points[&pair[0]]),
                    ),
                    viewport::project(
                        w,
                        rect,
                        sketch::world(&plane, &matrix, display.points[&pair[1]]),
                    ),
                ) {
                    painter.line_segment([a, b], Stroke::new(1., ui.visuals().weak_text_color()));
                }
            }
        }
    }
    let Some(mut list) = handles::handles(w.session.visible(), &s.definition, &source).ok() else {
        return consumed;
    };
    if let Some(block) = s.edit.as_ref().and_then(|editor| editor.block.as_ref())
        && block.mode == crate::sketch_blocks_ui::Mode::Definition
        && let Some(instance) = &block.instance_id
        && let Ok(controls) =
            handles::block_controls(w.session.visible(), &s.definition, &source, instance)
    {
        let outputs = controls
            .iter()
            .map(|handle| handle.target.entity().key().to_string())
            .collect::<std::collections::BTreeSet<_>>();
        list.retain(|handle| !outputs.contains(handle.target.entity().key()));
        list.extend(controls);
    }
    for mut handle in list {
        let entity = handle.target.entity();
        if !w.view.visibility.sketch_points
            || match &entity {
                SketchEntity::Point { point } => {
                    !crate::sketch_overlay::point_visible(w, &source, point)
                }
                SketchEntity::Member { member } => {
                    source.members.get(member).is_some_and(|member| {
                        !crate::sketch_overlay::member_visible(w, member.construction)
                    })
                }
                _ => false,
            }
        {
            continue;
        }
        let motion = match &entity {
            SketchEntity::Point { point } => point_motion.get(point),
            _ => None,
        };
        let movement = motion.map(|motion| {
            if motion.directions.is_empty() {
                "constrained"
            } else if motion.directions.len() == 1 {
                "one-direction"
            } else {
                "two-directions"
            }
        });
        let selected = selection.contains(&entity) || related.contains(entity.key());
        let unsatisfied = problems.contains(entity.key());
        let visible = matches!(handle.target, HandleTarget::Point { .. } | HandleTarget::BlockControl{..}) || selected
            || source.members.get(entity.key()).is_some_and(|member| selection.iter().any(|e| matches!(e,SketchEntity::Point { point } if member.curve.points().contains(&point.as_str()))));
        if !visible && active_id.as_ref() != Some(&handle.id) {
            continue;
        }
        if fully_constrained
            && matches!(
                handle.target,
                HandleTarget::Point { .. } | HandleTarget::MoveMember { .. }
            )
            && handle.disabled_reason.is_none()
        {
            handle.disabled_reason = Some("Sketch positions are fully constrained. Edit dimensions or relations to move this handle.".into());
        }
        let Some(pos) = viewport::project(w, rect, sketch::world(&plane, &matrix, handle.position))
        else {
            continue;
        };
        if !rect.contains(pos) && active_id.as_ref() != Some(&handle.id) {
            continue;
        }
        let dragging = active_id.as_ref() == Some(&handle.id);
        let locked = handle.disabled_reason.is_some();
        let region = presentation::Region {
            id: handle.id.clone(),
            role: "sketch-handle".into(),
            component: None,
            rect: [pos.x, pos.y, 0., 0.],
            classes: std::iter::once(handle.role.clone())
                .chain(movement.map(str::to_string))
                .collect(),
            resizable: false,
        };
        let props = w.theme.properties(
            &region,
            &w.view.active_layout,
            if dragging {
                "dragging"
            } else if selected {
                "selected"
            } else if unsatisfied {
                "unsatisfied"
            } else if locked {
                "locked"
            } else if ui
                .input(|i| i.pointer.hover_pos())
                .is_some_and(|p| p.distance(pos) < 10.)
            {
                "hovered"
            } else {
                movement.unwrap_or("ready")
            },
        );
        if crate::sketch_overlay::hidden(&props) && !dragging {
            continue;
        }
        let number = |key: &str, default: f32| {
            props
                .get(key)
                .and_then(|v| v.trim_end_matches("px").parse::<f32>().ok())
                .filter(|v| v.is_finite())
                .unwrap_or(default)
        };
        let size = number("size", 8.).clamp(4., 30.);
        let hit_size = number("hit-size", 20.).clamp(size + 4., 48.);
        let color = props
            .get("color")
            .and_then(|v| crate::ui::color(v))
            .unwrap_or_else(|| {
                if dragging || selected {
                    accent
                } else if unsatisfied {
                    ui.visuals().error_fg_color
                } else if locked || movement == Some("constrained") {
                    ui.visuals().weak_text_color()
                } else {
                    w.theme
                        .variables
                        .get("--sketch")
                        .and_then(|v| crate::ui::color(v))
                        .unwrap_or(Color32::from_rgb(45, 110, 210))
                }
            });
        let background = props
            .get("background")
            .and_then(|v| crate::ui::color(v))
            .unwrap_or(ui.visuals().window_fill());
        let allowed = match &entity {
            SketchEntity::Point { point } => crate::sketch_overlay::pick_point(w, &source, point),
            SketchEntity::Member { member } => crate::sketch_overlay::pick_member(
                w,
                source
                    .members
                    .get(member)
                    .is_some_and(|member| member.construction),
            ),
            _ => crate::sketch_overlay::pick_reference(w),
        };
        let point_handle = matches!(
            handle.target,
            HandleTarget::Point { .. } | HandleTarget::BlockControl { .. }
        );
        let wins_pick = !point_handle
            || w.sketch.as_ref().is_some_and(|s| {
                s.pick_candidates
                    .get(s.pick_index)
                    .is_some_and(|candidate| candidate.entity == entity)
                    && ui
                        .input(|input| input.pointer.hover_pos())
                        .is_some_and(|pointer| pointer.distance(pos) <= 9.)
            });
        let interactive = dragging || (allowed && wins_pick);
        let hit = ui.interact(
            egui::Rect::from_center_size(pos, egui::vec2(hit_size, hit_size)).intersect(rect),
            ui.make_persistent_id(("sketch-handle", &handle.id)),
            if !interactive {
                egui::Sense::hover()
            } else if w.fields.contains_key("smart-dimension-mode") {
                egui::Sense::click()
            } else {
                egui::Sense::click_and_drag()
            },
        );
        let stroke = Stroke::new(if hit.hovered() || dragging { 2. } else { 1.3 }, color);
        match handle.role.as_str() {
            "size" | "control" => {
                painter.rect(
                    egui::Rect::from_center_size(pos, egui::vec2(size, size)),
                    1.,
                    background,
                    stroke,
                    egui::StrokeKind::Middle,
                );
            }
            "move" => {
                painter.add(egui::Shape::convex_polygon(
                    vec![
                        pos + egui::vec2(0., -size * 0.7),
                        pos + egui::vec2(size * 0.7, 0.),
                        pos + egui::vec2(0., size * 0.7),
                        pos + egui::vec2(-size * 0.7, 0.),
                    ],
                    background,
                    stroke,
                ));
            }
            _ => {
                painter.circle(pos, size * 0.5, background, stroke);
            }
        }
        if handle.role == "center" {
            painter.line_segment([pos - egui::vec2(3., 0.), pos + egui::vec2(3., 0.)], stroke);
            painter.line_segment([pos - egui::vec2(0., 3.), pos + egui::vec2(0., 3.)], stroke);
        }
        if locked {
            painter.circle_filled(pos, 1.5, color);
        }
        if movement == Some("constrained") && !locked {
            painter.circle_filled(pos, 1., color);
        }
        if hit.hovered() && interactive {
            ui.ctx()
                .set_cursor_icon(if w.fields.contains_key("smart-dimension-mode") {
                    egui::CursorIcon::PointingHand
                } else if locked {
                    egui::CursorIcon::NotAllowed
                } else {
                    egui::CursorIcon::Grab
                });
            let label = handle.disabled_reason.as_deref().unwrap_or(&handle.label);
            let detail = motion
                .map(|motion| {
                    format!(
                        "\nLocal position: {}{}",
                        match motion.directions.len() {
                            0 => "constrained",
                            1 => "one direction of freedom",
                            _ => "two directions of freedom",
                        },
                        if motion.generated {
                            " · follows source geometry"
                        } else {
                            ""
                        }
                    )
                })
                .unwrap_or_default();
            hit.clone().on_hover_text(format!("{label}{detail}"));
        }
        if hit.clicked() || hit.secondary_clicked() {
            consumed = true;
            let multiselect =
                ui.input(|i| i.modifiers.shift || i.modifiers.command || i.modifiers.ctrl);
            let additive = multiselect
                || (w.fields.contains_key("smart-dimension-mode")
                    && w.sketch.as_ref().is_some_and(|s| s.selection.len() == 1));
            let preserve_context_pair = hit.secondary_clicked()
                && w.sketch
                    .as_ref()
                    .is_some_and(|s| s.selection.len() > 1 && s.selection.contains(&entity));
            if !preserve_context_pair {
                sketch_ui::select(w, Some(entity), additive);
            }
            if w.fields.contains_key("smart-dimension-mode") {
                sketch_ui::selection_changed(w);
            } else if w.sketch.as_ref().is_some_and(|s| !s.selection.is_empty())
                && (hit.secondary_clicked() || !multiselect)
            {
                crate::selection_ui::open(w, &hit, pos);
            }
        }
        if hit.drag_started() && !w.fields.contains_key("smart-dimension-mode") {
            consumed = true;
            if let Some(reason) = &handle.disabled_reason {
                w.status = reason.clone();
            } else if let Some(grab) = ui
                .input(|i| i.pointer.press_origin())
                .and_then(|p| sketch::hit_plane(w, rect, p, &plane, &matrix))
            {
                start(w, handle.clone(), grab, ui.id());
            }
        }
        if hit.dragged() || hit.drag_stopped() {
            consumed = true;
        }
    }
    if w.sketch
        .as_ref()
        .and_then(|s| s.drag.as_ref())
        .is_some_and(|g| g.viewport == ui.id())
    {
        consumed = true;
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        if let Some(pointer) = ui
            .input(|i| i.pointer.interact_pos())
            .filter(|p| rect.contains(*p))
        {
            if let Some(cursor) = sketch::hit_plane(w, rect, pointer, &plane, &matrix) {
                update(w, cursor, !ui.input(|i| i.modifiers.alt));
            }
            if let Some(g) = w.sketch.as_ref().and_then(|s| s.drag.as_ref()) {
                painter.text(
                    pointer + egui::vec2(12., 34.),
                    egui::Align2::LEFT_TOP,
                    &g.feedback,
                    egui::FontId::proportional(11.),
                    ui.visuals().text_color(),
                );
            }
        }
        if !ui.input(|i| i.pointer.primary_down()) {
            end(w);
        }
        ui.ctx().request_repaint();
    }
    consumed
}
