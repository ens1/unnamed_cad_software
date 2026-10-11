//! Owned model gestures and disposable native-mesh placement previews.
use super::*;
use crate::viewport as scene;
use bevy_egui::egui::{Color32, Pos2, Stroke};

pub(super) struct Capture {
    owner: egui::Id,
    last_seen: u64,
    rect: egui::Rect,
    handle: Handle,
    before_session: cad_core::Session,
    before_values: Values,
    before_preference: Option<Placement>,
    before_outcome: Option<Outcome>,
    before_selection: Option<scene::Selection>,
    actual: Placement,
    local_anchor: [f64; 3],
    origin: [f64; 3],
    axis: [f64; 3],
    starting_point: [f64; 3],
    starting_parameter: f64,
    last_radial: [f64; 3],
    angle: f64,
    sampled: Instant,
    updated: bool,
}
struct Glyph {
    handle: Handle,
    points: Vec<Pos2>,
    label: Option<Pos2>,
    color: Color32,
    width: f32,
    hit: f32,
    size: f32,
    disabled: Option<String>,
}
fn pivot(state: &State) -> [f64; 3] {
    if state.values.pivot == Pivot::Picked {
        state.picked_anchor.unwrap_or([0.; 3])
    } else {
        [0.; 3]
    }
}
fn current_pose(w: &Workspace, state: &State, validated: bool) -> Option<Placement> {
    let evaluation = if validated {
        super::evaluation(w)
    } else {
        super::evaluation(w)
            .or_else(|| {
                state.placement_preview.as_ref().filter(|preview| {
                    state.outcome.is_some() && preview.identity == w.session.identity()
                })
            })
            .or_else(|| scene::display_evaluation(w))
    }?;
    let key = cad_core::configuration::key(
        &state.baseline.root,
        state
            .baseline
            .active_configurations
            .get(&state.baseline.root)
            .map(String::as_str),
    );
    evaluation
        .assemblies
        .get(&key)?
        .placements
        .get(&state.path[0])
        .cloned()
}
pub(super) fn preview(state: &mut State, w: &Workspace, outcome: &Outcome) {
    let Some(preview) = state.placement_preview.as_mut() else {
        return;
    };
    let key = cad_core::configuration::key(
        &outcome.assembly,
        state
            .baseline
            .active_configurations
            .get(&outcome.assembly)
            .map(String::as_str),
    );
    let Some(assembly) = preview.assemblies.get_mut(&key) else {
        return;
    };
    assembly.placements = outcome.placements.clone();
    assembly.report = Some(outcome.solution.report.clone());
    assembly.diagnostics = outcome.solution.diagnostics.clone();
    assembly.component_motion = outcome.solution.component_motion.clone();
    if let Ok(instances) =
        cad_core::configuration::placed_parts(w.session.visible(), Some(&preview.assemblies))
    {
        preview.instances = instances;
        preview.identity = w.session.identity();
        preview.valid = false;
    }
}
pub(super) fn finish(w: &mut Workspace) {
    if let Some(state) = w.assembly_motion.as_mut() {
        state.capture = None;
        state.blocked_owner = None;
        state.blocked_seen = None;
    }
    crate::navigation::capture_model(w);
}
pub(super) fn cancel(w: &mut Workspace) -> bool {
    let Some(mut state) = w.assembly_motion.take() else {
        return false;
    };
    let Some(capture) = state.capture.take() else {
        let blocked = state.blocked_owner.take().is_some();
        state.blocked_seen = None;
        w.assembly_motion = Some(state);
        return blocked;
    };
    state.worker.cancel();
    w.worker.cancel();
    w.session = capture.before_session;
    w.requested = None;
    w.requested_preview = None;
    w.confirm_requested = false;
    w.selected = capture.before_selection;
    w.selection_popup = None;
    state.expected_source = w.session.visible().fingerprint();
    state.values = capture.before_values;
    state.submitted = state.values.clone();
    state.accepted = state.values.clone();
    state.preferred_placement = capture.before_preference;
    state.outcome = capture.before_outcome;
    state.pending = None;
    state.needs_solve = false;
    state.error = None;
    state.confirm_after_solve = false;
    state.blocked_owner = None;
    state.blocked_seen = None;
    if let Some(outcome) = state.outcome.clone() {
        preview(&mut state, w, &outcome);
    } else if let Some(preview) = state.placement_preview.as_mut() {
        preview.assemblies = state.assemblies.clone();
        if let Ok(instances) =
            cad_core::configuration::placed_parts(w.session.visible(), Some(&preview.assemblies))
        {
            preview.instances = instances;
        }
        preview.identity = w.session.identity();
    }
    w.changed = Instant::now();
    w.rendered_key.clear();
    w.status = "Component drag canceled; earlier movement edits retained".into();
    w.assembly_motion = Some(state);
    crate::navigation::capture_model(w);
    true
}
pub(super) fn finish_ui(w: &mut Workspace, ctx: &egui::Context) {
    let frame = ctx.cumulative_frame_nr();
    let missing = w.assembly_motion.as_ref().is_some_and(|state| {
        state
            .capture
            .as_ref()
            .is_some_and(|capture| capture.last_seen != frame)
            || state.blocked_owner.is_some() && state.blocked_seen != Some(frame)
    });
    if missing {
        cancel(w);
    }
}
fn start(
    w: &mut Workspace,
    state: &mut State,
    owner: egui::Id,
    rect: egui::Rect,
    pointer: Pos2,
    handle: Handle,
    grab: Option<[f64; 3]>,
) -> Result<()> {
    if state.pending.is_some()
        || state.needs_solve
        || state.values != state.accepted
        || state.error.is_some()
    {
        return Err(CadError::Invalid(
            "Wait for the current movement preview before grabbing the component".into(),
        ));
    }
    let actual = current_pose(w, state, true).ok_or_else(|| {
        CadError::Invalid(
            "Wait for current geometry validation before dragging the component".into(),
        )
    })?;
    let local_anchor = pivot(state);
    let anchor = drag::anchor(&actual, local_anchor);
    let view = scene::ray(w, rect, rect.center()).1;
    let axis = handle.axis(view)?;
    let origin = if handle == Handle::ViewPlane {
        grab.unwrap_or(anchor)
    } else {
        anchor
    };
    let ray = scene::ray(w, rect, pointer);
    let starting_point = if handle == Handle::ViewPlane {
        drag::plane_hit(ray, origin, axis)?
    } else {
        origin
    };
    let starting_parameter = if matches!(handle, Handle::TranslationAxis { .. }) {
        drag::axis_parameter(ray, origin, axis)?
    } else {
        0.
    };
    let last_radial = if handle.rotation() {
        drag::radial(ray, origin, axis)?
    } else {
        [1., 0., 0.]
    };
    state.capture = Some(Capture {
        owner,
        last_seen: 0,
        rect,
        handle,
        before_session: w.session.clone(),
        before_values: state.values.clone(),
        before_preference: state.preferred_placement.clone(),
        before_outcome: state.outcome.clone(),
        before_selection: w.selected.clone(),
        actual: actual.clone(),
        local_anchor,
        origin,
        axis,
        starting_point,
        starting_parameter,
        last_radial,
        angle: 0.,
        sampled: Instant::now() - std::time::Duration::from_millis(100),
        updated: false,
    });
    state.preferred_placement = Some(actual);
    state.selected_handle = handle;
    w.confirm_requested = false;
    w.selection_popup = None;
    crate::navigation::capture_model(w);
    Ok(())
}
fn update(w: &mut Workspace, state: &mut State, pointer: Pos2, release: bool) -> Result<()> {
    let Some(capture) = state.capture.as_mut() else {
        return Ok(());
    };
    // Keep a request stable until its solve completes; sample the latest pointer
    // again immediately afterwards. Release always submits the exact last value.
    let ray = scene::ray(w, capture.rect, pointer);
    if capture.handle.rotation() {
        let radial = drag::radial(ray, capture.origin, capture.axis)?;
        capture.angle += drag::signed_angle(capture.last_radial, radial, capture.axis)?;
        capture.last_radial = radial;
    }
    if !release && (state.pending.is_some() || capture.sampled.elapsed().as_millis() < 80) {
        return Ok(());
    }
    let translation = if capture.handle == Handle::ViewPlane {
        let point = drag::plane_hit(ray, capture.origin, capture.axis)?;
        std::array::from_fn(|index| point[index] - capture.starting_point[index])
    } else if matches!(capture.handle, Handle::TranslationAxis { .. }) {
        let distance =
            drag::axis_parameter(ray, capture.origin, capture.axis)? - capture.starting_parameter;
        capture.axis.map(|value| value * distance)
    } else {
        [0.; 3]
    };
    let offsets = drag::translation_offsets(
        &state.placement,
        &capture.actual,
        capture.local_anchor,
        translation,
    );
    if !capture.updated
        && !capture.handle.rotation()
        && translation.iter().all(|value| value.abs() < 1e-10)
    {
        return Ok(());
    }
    let mut values = state.values.clone();
    values.translation = offsets.map(|value| Expr(format!("{value:.12} mm")));
    if capture.handle.rotation() {
        if !capture.updated && capture.angle.abs() < 1e-12 {
            return Ok(());
        }
        values.rotation = drag::rotation_offsets(
            &state.placement,
            &capture.actual,
            capture.axis,
            capture.angle,
        )?
        .map(|value| Expr(format!("{value:.12} deg")));
        values.mode = if capture.before_values.mode == Mode::Free {
            Mode::Free
        } else {
            Mode::Rotate
        };
    } else {
        values.mode = if capture.before_values.mode == Mode::Free {
            Mode::Free
        } else {
            Mode::Translate
        };
    }
    capture.sampled = Instant::now();
    capture.updated = true;
    if values != state.values {
        state.values = values;
        changed(state);
        w.confirm_requested = false;
    }
    Ok(())
}
fn distance(point: Pos2, a: Pos2, b: Pos2) -> f32 {
    let line = b - a;
    let fraction = if line.length_sq() > 1e-6 {
        ((point - a).dot(line) / line.length_sq()).clamp(0., 1.)
    } else {
        0.
    };
    point.distance(a + line * fraction)
}
fn handles(state: &State) -> Vec<Handle> {
    let mut handles = vec![];
    if state.values.mode != Mode::Rotate {
        handles.push(Handle::ViewPlane);
        handles.extend((0..3).map(|axis| Handle::TranslationAxis { axis }));
    }
    if state.values.mode != Mode::Translate {
        handles.extend((0..3).map(|axis| Handle::RotationAxis { axis }));
        handles.push(Handle::ViewRotation);
    }
    handles
}
fn glyphs(w: &Workspace, state: &State, rect: egui::Rect) -> Vec<Glyph> {
    let actual = current_pose(w, state, false).unwrap_or_else(|| state.placement.clone());
    let origin = drag::anchor(&actual, pivot(state));
    let Some(center) = scene::project(w, rect, origin) else {
        return vec![];
    };
    let view = scene::ray(w, rect, rect.center()).1;
    let accent = scene::selection_color(w);
    handles(state)
        .into_iter()
        .filter_map(|handle| {
            let control = id(state, &format!("handle.{}", handle.key()));
            let axis_class = match handle {
                Handle::TranslationAxis { axis: 0 } | Handle::RotationAxis { axis: 0 } => "axis-x",
                Handle::TranslationAxis { axis: 1 } | Handle::RotationAxis { axis: 1 } => "axis-y",
                Handle::TranslationAxis { axis: 2 } | Handle::RotationAxis { axis: 2 } => "axis-z",
                _ => "view-plane",
            };
            let active = state
                .capture
                .as_ref()
                .is_some_and(|capture| capture.handle == handle);
            let properties = controls::properties(
                &control,
                "assembly-motion-handle",
                &["viewport-handle", axis_class],
                if active {
                    "dragging"
                } else if state.selected_handle == handle {
                    "selected"
                } else {
                    "ready"
                },
            );
            if properties
                .get("display")
                .is_some_and(|value| value == "none")
            {
                return None;
            }
            let size = controls::number(&properties, "handle-size", 8.).clamp(2., 64.);
            let pixels = controls::number(
                &properties,
                "width",
                if handle.rotation() { 112. } else { 84. },
            )
            .clamp(16., 512.);
            let ray = scene::ray(w, rect, center + egui::vec2(pixels, 0.));
            let point = drag::plane_hit(ray, origin, view).ok()?;
            let extent = (0..3)
                .map(|index| (point[index] - origin[index]).powi(2))
                .sum::<f64>()
                .sqrt();
            let axis = handle.axis(view).ok()?;
            let mut disabled = None;
            let points = if handle == Handle::ViewPlane {
                vec![center]
            } else if !handle.rotation() {
                if drag::axis_parameter(scene::ray(w, rect, center), origin, axis).is_err() {
                    disabled = Some(
                        "This axis aligns with the view; rotate the view before dragging it".into(),
                    );
                }
                let end = std::array::from_fn(|index| origin[index] + axis[index] * extent);
                vec![center, scene::project(w, rect, end)?]
            } else {
                if drag::plane_hit(scene::ray(w, rect, center), origin, axis).is_err() {
                    disabled = Some(
                        "This rotation plane is edge-on; rotate the view before dragging it".into(),
                    );
                }
                let [u, v] = drag::frame(axis).ok()?;
                (0..=96)
                    .filter_map(|index| {
                        let angle = index as f64 * std::f64::consts::TAU / 96.;
                        let point = std::array::from_fn(|i| {
                            origin[i] + (u[i] * angle.cos() + v[i] * angle.sin()) * extent * 0.5
                        });
                        scene::project(w, rect, point)
                    })
                    .collect()
            };
            let color = properties
                .get("color")
                .and_then(|value| crate::ui::color(value))
                .unwrap_or(accent)
                .gamma_multiply(controls::number(&properties, "opacity", 1.).clamp(0., 1.));
            let label = if !handle.rotation() {
                points.last().copied()
            } else {
                None
            };
            Some(Glyph {
                handle,
                points,
                label,
                color,
                width: controls::number(&properties, "stroke-width", if active { 3. } else { 2. })
                    .clamp(0.5, 12.),
                hit: size.max(6.),
                size,
                disabled,
            })
        })
        .collect()
}
fn hit(glyphs: &[Glyph], pointer: Pos2) -> Option<&Glyph> {
    glyphs
        .iter()
        .filter_map(|glyph| {
            let distance = if glyph.points.len() == 1 {
                pointer.distance(glyph.points[0])
            } else {
                glyph
                    .points
                    .windows(2)
                    .map(|pair| distance(pointer, pair[0], pair[1]))
                    .fold(f32::INFINITY, f32::min)
            };
            (distance <= glyph.hit).then_some((glyph, distance))
        })
        .min_by(|(_, a), (_, b)| a.total_cmp(b))
        .map(|(glyph, _)| glyph)
}
fn paint(w: &Workspace, state: &State, ui: &mut egui::Ui, rect: egui::Rect, glyphs: &[Glyph]) {
    let painter = ui.painter().with_clip_rect(rect);
    let pointer = ui.input(|input| input.pointer.hover_pos());
    let hovered = pointer
        .filter(|point| rect.contains(*point))
        .and_then(|point| hit(glyphs, point));
    for glyph in glyphs {
        let color = if glyph.disabled.is_some() {
            glyph.color.gamma_multiply(0.35)
        } else {
            glyph.color
        };
        let stroke = Stroke::new(glyph.width, color);
        if glyph.points.len() == 1 {
            painter.rect_filled(
                egui::Rect::from_center_size(
                    glyph.points[0],
                    egui::vec2(glyph.size * 1.5, glyph.size * 1.5),
                ),
                egui::CornerRadius::same(2),
                color,
            );
        } else {
            painter.add(egui::Shape::line(glyph.points.clone(), stroke));
            if !glyph.handle.rotation() && glyph.points.len() == 2 {
                let tip = glyph.points[1];
                let direction = (tip - glyph.points[0]).normalized();
                let side = egui::vec2(-direction.y, direction.x);
                painter.add(egui::Shape::convex_polygon(
                    vec![
                        tip,
                        tip - direction * glyph.size + side * glyph.size * 0.5,
                        tip - direction * glyph.size - side * glyph.size * 0.5,
                    ],
                    color,
                    Stroke::NONE,
                ));
            }
        }
        if let Some(label) = glyph.label.filter(|_| glyph.handle != Handle::ViewPlane) {
            let text = match glyph.handle {
                Handle::TranslationAxis { axis: 0 } => "X",
                Handle::TranslationAxis { axis: 1 } => "Y",
                _ => "Z",
            };
            painter.text(
                label + egui::vec2(4., -4.),
                egui::Align2::LEFT_BOTTOM,
                text,
                egui::FontId::proportional(11.),
                color,
            );
        }
    }
    if let Some(glyph) = hovered {
        ui.ctx().set_cursor_icon(if glyph.disabled.is_some() {
            egui::CursorIcon::NotAllowed
        } else {
            egui::CursorIcon::Grab
        });
        if let Some(pointer) = pointer {
            ui.interact(
                egui::Rect::from_center_size(pointer, egui::vec2(2., 2.)),
                ui.id().with(("assembly-handle-hint", glyph.handle.key())),
                egui::Sense::hover(),
            )
            .on_hover_text(glyph.disabled.as_deref().unwrap_or(glyph.handle.label()));
        }
    }
    if let Some(goal) = intent(state).ok().and_then(|intent| intent.goal.position)
        && let Some(point) = scene::project(w, rect, goal)
        && state.pending.is_some()
    {
        let properties = controls::properties(
            &id(state, "requested-handle"),
            "assembly-motion-request",
            &["viewport-handle"],
            "pending",
        );
        if !properties
            .get("display")
            .is_some_and(|value| value == "none")
        {
            let color = properties
                .get("color")
                .and_then(|value| crate::ui::color(value))
                .unwrap_or(scene::selection_color(w));
            painter.circle_stroke(
                point,
                controls::number(&properties, "handle-size", 7.).clamp(2., 64.),
                Stroke::new(controls::number(&properties, "stroke-width", 1.5), color),
            );
        }
    }
}
pub(super) fn interact(
    w: &mut Workspace,
    ui: &mut egui::Ui,
    rect: egui::Rect,
    response: &egui::Response,
    blocked: &[egui::Rect],
) -> bool {
    if w.settings.is_some() || w.sketch.is_some() {
        return false;
    }
    let owner = response.id;
    let focused = ui.input(|input| input.focused);
    if !focused {
        return cancel(w);
    }
    let pointer = ui.input(|input| input.pointer.interact_pos());
    let primary = ui.input(|input| input.pointer.primary_down());
    let pressed = ui.input(|input| input.pointer.primary_pressed());
    if w.assembly_motion.is_none()
        && response.drag_started_by(egui::PointerButton::Primary)
        && w.tool_checkpoint.is_none()
        && matches!(w.dialog.as_deref(), None | Some("assembly"))
    {
        let origin = ui.input(|input| input.pointer.press_origin());
        if let Some(origin) = origin.filter(|point| {
            rect.contains(*point) && !blocked.iter().any(|blocked| blocked.contains(*point))
        }) && let Some(selection) = scene::pick(w, rect, origin)
            .into_iter()
            .find(|s| !s.path.is_empty())
            && movement_reason(w, &selection.path[0]).is_none()
        {
            scene::select(w, selection.clone());
            begin(w, selection.path, Mode::Translate);
        }
    }
    let Some(mut state) = w.assembly_motion.take() else {
        return false;
    };
    let mut consumed = state.capture.is_some() || state.blocked_owner.is_some();
    if state.blocked_owner.is_some() {
        if state.blocked_owner == Some(owner) {
            state.blocked_seen = Some(ui.ctx().cumulative_frame_nr());
        }
        if !primary {
            state.blocked_owner = None;
            state.blocked_seen = None;
        }
        w.assembly_motion = Some(state);
        crate::navigation::capture_model(w);
        return true;
    }
    if let Some(capture) = &state.capture {
        if capture.owner == owner {
            if capture.rect != rect {
                state.capture = None;
                w.status =
                    "Viewport changed; component drag ended with its current draft retained".into();
            } else {
                if let Some(pointer) = pointer
                    && let Err(error) = update(w, &mut state, pointer, !primary)
                {
                    w.status = error.to_string();
                }
                if !primary {
                    state.capture = None;
                }
                ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
                ui.ctx().request_repaint();
            }
        }
    } else if (pressed || response.drag_started_by(egui::PointerButton::Primary))
        && primary
        && let Some(origin) = ui
            .input(|input| input.pointer.press_origin())
            .filter(|point| {
                rect.contains(*point) && !blocked.iter().any(|blocked| blocked.contains(*point))
            })
    {
        let shapes = glyphs(w, &state, rect);
        let handle = hit(&shapes, origin);
        let selected = scene::pick(w, rect, origin)
            .into_iter()
            .find(|s| s.path.first() == state.path.first());
        if handle.is_some() || selected.is_some() {
            let chosen = handle.map(|glyph| glyph.handle).unwrap_or_else(|| {
                if state.values.mode == Mode::Rotate {
                    Handle::ViewRotation
                } else {
                    state.selected_handle
                }
            });
            if let Some(reason) = handle.and_then(|glyph| glyph.disabled.as_deref()) {
                w.status = reason.into();
                state.blocked_owner = Some(owner);
            } else {
                match start(
                    w,
                    &mut state,
                    owner,
                    rect,
                    origin,
                    chosen,
                    selected.map(|s| s.point),
                ) {
                    Ok(()) => {
                        if let Some(pointer) = pointer
                            && let Err(error) = update(w, &mut state, pointer, false)
                        {
                            w.status = error.to_string();
                        }
                    }
                    Err(error) => {
                        w.status = error.to_string();
                        state.blocked_owner = Some(owner);
                    }
                }
            }
            consumed = true;
        }
    }
    let shapes = glyphs(w, &state, rect);
    paint(w, &state, ui, rect, &shapes);
    if let Some(capture) = state
        .capture
        .as_mut()
        .filter(|capture| capture.owner == owner)
    {
        capture.last_seen = ui.ctx().cumulative_frame_nr();
    }
    if state.blocked_owner == Some(owner) {
        state.blocked_seen = Some(ui.ctx().cumulative_frame_nr());
    }
    w.assembly_motion = Some(state);
    if consumed {
        crate::navigation::capture_model(w);
    }
    consumed
}
pub(super) fn semantics(w: &Workspace, state: &State) -> Vec<SemanticElement> {
    let actual = current_pose(w, state, false).unwrap_or_else(|| state.placement.clone());
    let origin = drag::anchor(&actual, pivot(state));
    let view = scene::ray(
        w,
        egui::Rect::from_min_size(Pos2::ZERO, egui::vec2(100., 100.)),
        egui::pos2(50., 50.),
    )
    .1;
    let mut elements=handles(state).into_iter().map(|handle|SemanticElement {
        id:id(state,&format!("handle.{}",handle.key())),role:"assembly-motion-handle".into(),label:handle.label().into(),value:None,
        state:if state.capture.as_ref().is_some_and(|capture|capture.handle==handle){"dragging"}else if state.selected_handle==handle{"selected"}else{"ready"}.into(),
        commands:vec!["assembly.motion-handle".into()],relationships:vec![state.baseline.root.clone(),state.path[0].clone()],
        data:serde_json::json!({"contract":"tcad.assembly_motion.drag","version":drag::CONTRACT_VERSION,"token":state.token,"path":state.path,"handle":handle,"origin_mm":origin,"direction":handle.axis(view).ok(),"view_relative":matches!(handle,Handle::ViewPlane|Handle::ViewRotation),"persistent_mates":false,"physics":"hard_mates_and_travel_bounds","classes":["viewport-handle"]}),
    }).collect::<Vec<_>>();
    elements.push(SemanticElement{id:id(state,"requested-handle"),role:"assembly-motion-request".into(),label:"Requested movement point".into(),value:None,state:if state.pending.is_some(){"pending"}else{"ready"}.into(),commands:vec![],relationships:vec![state.path[0].clone()],data:serde_json::json!({"contract":"tcad.assembly_motion.drag","version":drag::CONTRACT_VERSION,"path":state.path,"goal":intent(state).ok(),"display_only":true})});
    elements
}
