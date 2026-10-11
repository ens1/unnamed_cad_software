//! Platform input adapter and view-only navigation. Geometry never consumes these events.
use crate::*;
use bevy::window::PrimaryWindow;
use bevy::winit::{RawWinitWindowEvent, WINIT_WINDOWS};
use bevy_egui::egui;
use cad_core::navigation::{Event, Motion, Phase, PrecisionModifier, Preferences};
use std::time::Duration;

const QUIET: Duration = Duration::from_millis(180);
const FRAME_EVENT_LIMIT: usize = 256;
#[derive(Clone, Copy)]
pub enum Mode {
    Orbit,
    Pan,
    Zoom,
    Native,
}
pub struct Capture {
    owner: u64,
    last: Instant,
    anchor: egui::Pos2,
    rect: egui::Rect,
    mode: Mode,
    pointer_drag: bool,
}
#[derive(Default)]
pub struct Runtime {
    events: Vec<Event>,
    raw_scroll: bool,
    raw_pinch: bool,
    raw_rotate: bool,
    active: u8,
    before: u8,
    ui_frame: Option<u64>,
    external: bool,
    blocked: bool,
    last: Option<Instant>,
    handled: bool,
    /// Used only to exclude touchscreen mouse emulation from camera ownership.
    touches: std::collections::BTreeSet<(egui::TouchDeviceId, egui::TouchId)>,
    touch_input: bool,
}
fn stream(motion: Motion) -> u8 {
    match motion {
        Motion::Scroll { .. } => 1,
        Motion::Pinch { .. } => 2,
        Motion::Rotate { .. } => 4,
        Motion::Pan { .. } => 8,
        _ => 0,
    }
}
fn phase(value: winit::event::TouchPhase) -> Phase {
    match value {
        winit::event::TouchPhase::Started => Phase::Start,
        winit::event::TouchPhase::Moved => Phase::Move,
        winit::event::TouchPhase::Ended => Phase::End,
        winit::event::TouchPhase::Cancelled => Phase::Cancel,
    }
}
fn push(runtime: &mut Runtime, event: Event) {
    if runtime.events.len() == FRAME_EVENT_LIMIT {
        runtime.events.clear();
        runtime.events.push(Event {
            phase: Some(Phase::Cancel),
            motion: Motion::Cancel,
        });
        runtime.blocked = true;
        runtime.active = 0;
        return;
    }
    if runtime.blocked
        && matches!(
            runtime.events.first().map(|e| e.motion),
            Some(Motion::Cancel)
        )
    {
        return;
    }
    if event.phase == Some(Phase::Start)
        && runtime.active == 0
        && runtime.last.is_none_or(|last| last.elapsed() >= QUIET)
    {
        runtime.blocked = false;
        runtime.external = false;
    }
    let bit = stream(event.motion);
    match event.phase {
        Some(Phase::Start) => runtime.active |= bit,
        Some(Phase::End | Phase::Cancel) => runtime.active &= !bit,
        _ => {}
    }
    if matches!(event.motion, Motion::Cancel) {
        runtime.active = 0;
    }
    runtime.events.push(event);
    runtime.last = Some(Instant::now());
}
/// Read the original primary-window events: the existing egui bridge drops scroll phases
/// and broadcasts pinch. Never forward a second pinch into egui.
pub fn collect_input(
    mut events: MessageReader<RawWinitWindowEvent>,
    windows: Query<(Entity, &Window), With<PrimaryWindow>>,
    mut w: ResMut<Workspace>,
    _main_thread: bevy::ecs::system::NonSendMarker,
) {
    let ui_scale = w.view.ui_scale.clamp(0.75, 1.5);
    let runtime = &mut w.navigation_input;
    runtime.before = runtime.active;
    runtime.events.clear();
    runtime.raw_scroll = false;
    runtime.raw_pinch = false;
    runtime.raw_rotate = false;
    runtime.handled = false;
    let Ok((entity, window)) = windows.single() else {
        return;
    };
    let scale = window.resolution.scale_factor() * ui_scale;
    for raw in events.read() {
        if WINIT_WINDOWS.with_borrow(|windows| windows.get_window_entity(raw.window_id))
            != Some(entity)
        {
            continue;
        }
        use winit::event::{MouseScrollDelta, WindowEvent};
        let event = match &raw.event {
            WindowEvent::MouseWheel {
                delta, phase: p, ..
            } => {
                runtime.raw_scroll = true;
                let (delta, lines) = match delta {
                    MouseScrollDelta::LineDelta(x, y) => ([*x * 50., *y * 50.], true),
                    MouseScrollDelta::PixelDelta(p) => {
                        ([p.x as f32 / scale, p.y as f32 / scale], false)
                    }
                };
                Event {
                    phase: Some(phase(*p)),
                    motion: Motion::Scroll { delta, lines },
                }
            }
            WindowEvent::PinchGesture {
                delta, phase: p, ..
            } => {
                runtime.raw_pinch = true;
                Event {
                    phase: Some(phase(*p)),
                    motion: Motion::Pinch {
                        logarithm: *delta as f32,
                    },
                }
            }
            WindowEvent::RotationGesture {
                delta, phase: p, ..
            } => {
                runtime.raw_rotate = true;
                Event {
                    phase: Some(phase(*p)),
                    motion: Motion::Rotate {
                        radians: -delta.to_radians(),
                    },
                }
            }
            WindowEvent::PanGesture {
                delta, phase: p, ..
            } => Event {
                phase: Some(phase(*p)),
                motion: Motion::Pan {
                    delta: [delta.x / scale, delta.y / scale],
                },
            },
            WindowEvent::DoubleTapGesture { .. } => Event {
                phase: None,
                motion: Motion::SmartZoom,
            },
            WindowEvent::Focused(false) | WindowEvent::CursorLeft { .. } => Event {
                phase: Some(Phase::Cancel),
                motion: Motion::Cancel,
            },
            _ => continue,
        };
        push(runtime, event);
    }
}
/// Cancel owns any tail until a quiet interval; a new click cannot reacquire smoothed input.
pub fn stop(w: &mut Workspace) {
    w.gesture = None;
    w.navigation_input.blocked = true;
    w.navigation_input.external = false;
    w.navigation_input.last = Some(Instant::now());
}
/// A modeling gesture owns this frame, including queued platform gesture tails.
pub(crate) fn capture_model(w: &mut Workspace) {
    stop(w);
    w.navigation_input.handled = true;
}
pub fn begin_ui(w: &mut Workspace, ctx: &egui::Context) {
    let frame = ctx.cumulative_frame_nr();
    if w.navigation_input.ui_frame == Some(frame) {
        return;
    }
    w.navigation_input.ui_frame = Some(frame);
    let (focused, pressed, events, touch) = ctx.input(|i| {
        (
            i.focused,
            i.pointer.any_pressed(),
            i.events.clone(),
            i.multi_touch(),
        )
    });
    let mut touch_event = false;
    for event in &events {
        if let egui::Event::Touch {
            device_id,
            id,
            phase,
            ..
        } = event
        {
            touch_event = true;
            let key = (*device_id, *id);
            match phase {
                egui::TouchPhase::Start | egui::TouchPhase::Move => {
                    if w.navigation_input.touches.len() < 16 {
                        w.navigation_input.touches.insert(key);
                    }
                }
                egui::TouchPhase::End | egui::TouchPhase::Cancel => {
                    w.navigation_input.touches.remove(&key);
                }
            }
        }
    }
    if !focused {
        w.navigation_input.touches.clear();
    }
    w.navigation_input.touch_input =
        touch_event || !w.navigation_input.touches.is_empty() || touch.is_some();
    if !focused
        || pressed
        || w.navigation_input.touch_input
        || ctx.egui_wants_keyboard_input()
        || w.navigation_input.events.iter().any(|e| {
            relevant(e.motion, &w.view.navigation)
                && (!finite(*e)
                    || matches!(e.motion, Motion::Cancel)
                    || e.phase == Some(Phase::Cancel))
        })
    {
        stop(w);
    }
    let rotation = w.view.navigation.native_rotation;
    let runtime = &mut w.navigation_input;
    if !runtime.raw_scroll {
        for event in &events {
            if let egui::Event::MouseWheel { unit, delta, .. } = event {
                let (factor, lines) = match unit {
                    egui::MouseWheelUnit::Point => (1., false),
                    egui::MouseWheelUnit::Line => (50., true),
                    egui::MouseWheelUnit::Page => (500., true),
                };
                push(
                    runtime,
                    Event {
                        phase: None,
                        motion: Motion::Scroll {
                            delta: [delta.x * factor, delta.y * factor],
                            lines,
                        },
                    },
                );
            }
        }
    }
    // Explicit zoom events only: InputState::zoom_delta also includes Ctrl-wheel smoothing.
    if !runtime.raw_pinch && !runtime.touch_input {
        for event in &events {
            if let egui::Event::Zoom(factor) = event {
                push(
                    runtime,
                    Event {
                        phase: None,
                        motion: Motion::Pinch {
                            logarithm: factor.ln(),
                        },
                    },
                );
            }
        }
    }
    if !runtime.raw_rotate && !runtime.touch_input {
        for event in &events {
            if let egui::Event::Rotate(radians) = event {
                push(
                    runtime,
                    Event {
                        phase: None,
                        motion: Motion::Rotate { radians: *radians },
                    },
                );
            }
        }
    }
    if runtime.before & active_mask(rotation) == 0
        && runtime.events.iter().any(|e| e.phase == Some(Phase::Start))
    {
        runtime.external = false;
    }
    let quiet = runtime.active & active_mask(rotation) == 0
        && runtime.last.is_none_or(|last| last.elapsed() >= QUIET);
    if quiet {
        runtime.blocked = false;
        runtime.external = false;
        if w.gesture
            .as_ref()
            .is_some_and(|g| !g.pointer_drag && g.last.elapsed() >= QUIET)
        {
            w.gesture = None;
        }
    }
}
pub fn finish_ui(w: &mut Workspace) {
    if w.navigation_input
        .events
        .iter()
        .any(|e| relevant(e.motion, &w.view.navigation))
        && !w.navigation_input.handled
        && w.gesture.is_none()
    {
        w.navigation_input.external =
            w.navigation_input.active & active_mask(w.view.navigation.native_rotation) != 0
                || !w
                    .navigation_input
                    .events
                    .iter()
                    .any(|e| e.phase == Some(Phase::End));
    }
}
fn edit_capture(w: &Workspace, ctx: &egui::Context) -> bool {
    ctx.egui_wants_keyboard_input()
        || crate::assembly_motion_ui::dragging(w)
        || w.sketch.as_ref().is_some_and(|s| {
            s.drag.is_some()
                || !s.chain.is_empty()
                || !s.clicks.is_empty()
                || s.dimension.as_ref().is_some_and(|d| d.focus_value)
                || s.edit.as_ref().is_some_and(|e| {
                    e.dimension_group
                        .as_ref()
                        .is_some_and(|d| d.annotation_drag.is_some())
                        || ctx.input(|i| i.pointer.any_down())
                })
        })
}
fn precision(p: &Preferences, m: egui::Modifiers) -> f32 {
    let active = match p.precision_modifier {
        PrecisionModifier::Alt => m.alt,
        PrecisionModifier::AltShift => m.alt && m.shift,
        PrecisionModifier::None => false,
    };
    if active { p.precision_scale } else { 1. }
}
fn sign(invert: bool) -> f32 {
    if invert { -1. } else { 1. }
}
fn zoom(w: &mut Workspace, logarithm: f32, rect: egui::Rect, anchor: egui::Pos2, speed: f32) {
    let old = w.view.distance;
    w.view.distance = (old
        * (-logarithm.clamp(-2., 2.)
            * speed
            * w.view.navigation.zoom_sensitivity
            * sign(w.view.navigation.invert_zoom))
        .exp())
    .clamp(0.005, 10000.);
    let (_, r, u) = viewport::basis(w);
    let [horizontal, vertical] = viewport::focal(w, rect);
    let shift = (r * (anchor.x - rect.center().x) / horizontal
        - u * (anchor.y - rect.center().y) / vertical)
        * (old - w.view.distance);
    w.view.target = (Vec3::from_array(w.view.target) + shift).to_array();
}
fn pan(w: &mut Workspace, delta: [f32; 2], speed: f32) {
    let (_, r, u) = viewport::basis(w);
    let p = &w.view.navigation;
    let shift = (r * delta[0] * sign(p.invert_pan[0]) - u * delta[1] * sign(p.invert_pan[1]))
        * p.pan_sensitivity
        * w.view.distance
        * speed;
    w.view.target = (Vec3::from_array(w.view.target) + shift).to_array();
}
fn orbit(w: &mut Workspace, delta: [f32; 2], speed: f32) {
    let p = &w.view.navigation;
    w.view.orbit[0] += delta[0] * p.orbit_sensitivity * speed * sign(p.invert_orbit[0]);
    w.view.orbit[1] -= delta[1] * p.orbit_sensitivity * speed * sign(p.invert_orbit[1]);
    w.view.orbit = w.view.orbit.map(|a| a.rem_euclid(std::f32::consts::TAU));
}
fn finite(event: Event) -> bool {
    match event.motion {
        Motion::Scroll { delta, .. } | Motion::Pan { delta } => delta
            .into_iter()
            .all(|v| v.is_finite() && v.abs() <= 100000.),
        Motion::Pinch { logarithm } => logarithm.is_finite(),
        Motion::Rotate { radians } => radians.is_finite() && radians.abs() <= std::f32::consts::TAU,
        _ => true,
    }
}
fn active_mask(rotation: bool) -> u8 {
    if rotation { 15 } else { 11 }
}
fn relevant(motion: Motion, p: &Preferences) -> bool {
    match motion {
        Motion::Rotate { .. } => p.native_rotation,
        Motion::SmartZoom => p.smart_zoom,
        _ => true,
    }
}
fn native_motion(motion: Motion, p: &Preferences) -> bool {
    matches!(motion, Motion::Pinch { .. } | Motion::Pan { .. })
        || matches!(motion, Motion::Rotate { .. }) && p.native_rotation
}
fn native_delta(motion: Motion) -> bool {
    match motion {
        Motion::Pinch { logarithm } => logarithm != 0.,
        Motion::Rotate { radians } => radians != 0.,
        Motion::Pan { delta } => delta != [0.; 2],
        _ => false,
    }
}
/// Partition in event order. Native duplicate suppression cannot cross an ended segment
/// into the following scroll gesture, but also covers an emulated scroll before its native
/// Start when both belong to the same segment. Admission already bounds this to 256 events.
fn native_segments(events: &[Event], before: u8, p: &Preferences) -> Vec<bool> {
    let mask = active_mask(p.native_rotation);
    let mut active = before & mask;
    let mut first = 0;
    let mut native = active & 14 != 0;
    let mut result = vec![false; events.len()];
    for (index, event) in events.iter().enumerate() {
        if !relevant(event.motion, p) {
            continue;
        }
        let bit = stream(event.motion) & mask;
        native |= active & 14 != 0
            || native_motion(event.motion, p)
                && (matches!(event.phase, Some(Phase::Start | Phase::Move))
                    || native_delta(event.motion)
                    || active & bit != 0);
        match event.phase {
            Some(Phase::Start) => active |= bit,
            Some(Phase::End) => active &= !bit,
            Some(Phase::Cancel) => active = 0,
            _ => {}
        }
        if matches!(event.phase, Some(Phase::End | Phase::Cancel)) && active == 0 {
            result[first..=index].fill(native);
            first = index + 1;
            native = false;
        }
    }
    result[first..].fill(native);
    result
}
fn capture_mode(w: &Workspace, modifiers: egui::Modifiers, native: bool, lines: bool) -> Mode {
    if native {
        Mode::Native
    } else if modifiers.command || modifiers.ctrl || lines {
        Mode::Zoom
    } else if if w.sketch.is_some() {
        !modifiers.shift
    } else {
        modifiers.shift ^ w.view.navigation.alternate_pan
    } {
        Mode::Pan
    } else {
        Mode::Orbit
    }
}
fn acquire(w: &mut Workspace, response: &egui::Response, mode: Mode, pointer_drag: bool) {
    let anchor = response.hover_pos().unwrap_or(response.rect.center());
    w.gesture = Some(Capture {
        owner: response.id.value(),
        last: Instant::now(),
        anchor,
        rect: response.rect,
        mode,
        pointer_drag,
    });
}
pub fn navigate(w: &mut Workspace, ctx: &egui::Context, response: &egui::Response) -> bool {
    if w.settings.is_some() {
        stop(w);
        return false;
    }
    if w.navigation_input.handled {
        return false;
    }
    let (pressed, focused, modifiers, drag, down) = ctx.input(|i| {
        (
            i.pointer.any_pressed(),
            i.focused,
            i.modifiers,
            i.pointer.delta(),
            i.pointer.any_down(),
        )
    });
    if pressed || !focused || w.navigation_input.touch_input || edit_capture(w, ctx) {
        stop(w);
        w.navigation_input.handled = true;
        return false;
    }
    let pointer_drag = response.dragged() && w.sketch.is_none();
    let other_pointer = ctx.egui_is_using_pointer() && !pointer_drag;
    if other_pointer {
        stop(w);
        w.navigation_input.handled = true;
        return false;
    }
    if w.gesture.as_ref().is_some_and(|g| g.pointer_drag && !down) {
        w.gesture = None;
    }
    let events = w.navigation_input.events.clone();
    if events.iter().any(|event| {
        relevant(event.motion, &w.view.navigation)
            && (!finite(*event)
                || matches!(event.motion, Motion::Cancel)
                || event.phase == Some(Phase::Cancel))
    }) {
        stop(w);
        w.navigation_input.handled = true;
        return false;
    }
    let eligible = response.hovered();
    let owner = response.id.value();
    let speed = precision(&w.view.navigation, modifiers);
    let mut changed = false;
    if pointer_drag && eligible {
        if w.gesture.is_none() {
            let mode = capture_mode(w, modifiers, false, false);
            acquire(w, response, mode, true);
        }
        if let Some(capture) = w
            .gesture
            .as_ref()
            .filter(|g| g.owner == owner && g.pointer_drag)
        {
            let (mode, rect, anchor) = (capture.mode, capture.rect, capture.anchor);
            match mode {
                Mode::Pan => pan(w, [drag.x, drag.y], speed),
                Mode::Zoom => zoom(w, drag.y * 0.006, rect, anchor, speed),
                _ => orbit(w, [drag.x, drag.y], speed),
            }
            changed = drag != egui::Vec2::ZERO;
            w.navigation_input.handled = true;
        }
    } else if !w.navigation_input.blocked && !w.navigation_input.external {
        let mask = active_mask(w.view.navigation.native_rotation);
        let mut active = w.navigation_input.before & mask;
        let segments = native_segments(&events, active, &w.view.navigation);
        for (index, event) in events.into_iter().enumerate() {
            let segment_native = segments[index];
            if !relevant(event.motion, &w.view.navigation) {
                continue;
            }
            if matches!(event.motion, Motion::SmartZoom) {
                if !eligible {
                    continue;
                }
                w.gesture = None;
            }
            let bit = stream(event.motion) & mask;
            // End applies to the old capture first. A subsequent Start establishes new intent,
            // even when it arrives within the ordinary wheel quiet interval.
            if event.phase == Some(Phase::Start) && active == 0 {
                w.gesture = None;
            }
            if w.gesture.is_none() && eligible && event.phase != Some(Phase::End) {
                let native = segment_native || active & 14 != 0;
                let lines = matches!(event.motion, Motion::Scroll { lines: true, .. });
                let mode = capture_mode(w, modifiers, native, lines);
                acquire(w, response, mode, false);
            }
            match event.phase {
                Some(Phase::Start) => active |= bit,
                Some(Phase::End) => active &= !bit,
                _ => {}
            }
            if let Some(capture) = w
                .gesture
                .as_ref()
                .filter(|g| g.owner == owner && !g.pointer_drag)
            {
                let (mode, rect, anchor) = (capture.mode, capture.rect, capture.anchor);
                w.navigation_input.handled = true;
                match event.motion {
                    Motion::Scroll { delta, .. }
                        if !segment_native && active & 14 == 0 && !matches!(mode, Mode::Native) =>
                    {
                        match mode {
                            Mode::Pan => pan(w, delta, speed),
                            Mode::Zoom => zoom(w, delta[1] * 0.006, rect, anchor, speed),
                            _ => orbit(w, delta, speed),
                        }
                        changed |= delta != [0.; 2];
                    }
                    Motion::Pinch { logarithm } => {
                        zoom(w, logarithm, rect, anchor, speed);
                        changed |= logarithm != 0.;
                    }
                    Motion::Rotate { radians } => {
                        w.view.roll = (w.view.roll
                            + radians
                                * speed
                                * w.view.navigation.roll_sensitivity
                                * sign(w.view.navigation.invert_roll))
                        .rem_euclid(std::f32::consts::TAU);
                        changed |= radians != 0.;
                    }
                    Motion::Pan { delta } => {
                        pan(w, delta, speed);
                        changed |= delta != [0.; 2];
                    }
                    Motion::SmartZoom => {
                        let selected = w.selected.is_some();
                        viewport::fit(w, selected);
                        changed = true;
                        break;
                    }
                    _ => {}
                }
            }
            if event.phase == Some(Phase::End)
                && active == 0
                && w.gesture
                    .as_ref()
                    .is_some_and(|g| g.owner == owner && !g.pointer_drag)
            {
                w.gesture = None;
            }
        }
    }
    if changed {
        sanitize(&mut w.view);
        if let Some(g) = &mut w.gesture {
            g.last = Instant::now();
        }
    }
    changed
}
pub fn sanitize(view: &mut PresentationState) {
    if !view.roll.is_finite() {
        view.roll = 0.;
    }
    if !view.distance.is_finite() || !(0.005..=10000.).contains(&view.distance) {
        view.distance = 3.5;
    }
    if !view.target.iter().all(|v| v.is_finite()) {
        view.target = [0.; 3];
    }
    if !view.orbit.iter().all(|v| v.is_finite()) {
        view.orbit = [0.65, 0.4];
    }
    view.orbit = view.orbit.map(|a| a.rem_euclid(std::f32::consts::TAU));
    view.roll = view.roll.rem_euclid(std::f32::consts::TAU);
    let [yaw, pitch] = view.orbit;
    let target = Vec3::from_array(view.target);
    let offset = Vec3::new(
        yaw.sin() * pitch.cos(),
        pitch.sin(),
        yaw.cos() * pitch.cos(),
    ) * view.distance;
    if !(target + offset - target).is_finite()
        || (target + offset - target).length_squared() < 1e-12
    {
        view.target = [0.; 3];
    }
}
fn save(w: &mut Workspace) {
    crate::settings::save_live_application(w);
}
pub fn set(w: &mut Workspace, pointer: &str, value: serde_json::Value) {
    match w.view.navigation.set(pointer, value) {
        Ok(()) => {
            stop(w);
            save(w);
        }
        Err(e) => w.status = e.to_string(),
    }
}
pub fn upright(w: &mut Workspace) {
    let offset = viewport::eye(w) - Vec3::from_array(w.view.target);
    if offset.x.hypot(offset.z) > 1e-6 {
        w.view.orbit[0] = offset.x.atan2(offset.z);
    }
    w.view.orbit[1] = (offset.y / w.view.distance).clamp(-1., 1.).asin();
    w.view.roll = 0.;
    stop(w);
}
pub fn action(w: &mut Workspace, command: &str, data: Option<&serde_json::Value>) -> bool {
    if !command.starts_with("navigation.") {
        return false;
    }
    if command == "navigation.preferences" {
        crate::settings::open(w, Some("application.navigation"));
        return true;
    }
    if command == "navigation.set" {
        if let Some((pointer, value)) =
            data.and_then(|d| Some((d.get("pointer")?.as_str()?, d.get("json")?.clone())))
        {
            set(w, pointer, value);
        } else {
            w.status = "Bind a navigation preference field to set its value".into();
        }
        return true;
    }
    if command == "navigation.reset" {
        w.view.navigation = Preferences::default();
        sanitize(&mut w.view);
        stop(w);
        save(w);
        return true;
    }
    if command == "navigation.stop" {
        stop(w);
        return true;
    }
    if w.sketch
        .as_ref()
        .is_some_and(|s| s.drag.is_some() || !s.chain.is_empty() || !s.clicks.is_empty())
    {
        w.status = "Finish the active sketch gesture before changing the view".into();
        return true;
    }
    stop(w);
    match command {
        "navigation.upright" => upright(w),
        "navigation.fit-all" => viewport::fit(w, false),
        "navigation.fit-selection" => viewport::fit(w, true),
        "navigation.zoom-in" => w.view.distance = (w.view.distance / 1.25).max(0.005),
        "navigation.zoom-out" => w.view.distance = (w.view.distance * 1.25).min(10000.),
        "navigation.orbit-left" => orbit(w, [-30., 0.], 1.),
        "navigation.orbit-right" => orbit(w, [30., 0.], 1.),
        "navigation.orbit-up" => orbit(w, [0., 30.], 1.),
        "navigation.orbit-down" => orbit(w, [0., -30.], 1.),
        "navigation.pan-left" => pan(w, [-30., 0.], 1.),
        "navigation.pan-right" => pan(w, [30., 0.], 1.),
        "navigation.pan-up" => pan(w, [0., -30.], 1.),
        "navigation.pan-down" => pan(w, [0., 30.], 1.),
        _ => w.status = "Unknown navigation action".into(),
    }
    true
}
pub fn hint(w: &Workspace) -> String {
    let profile = if w.sketch.is_some() || w.view.navigation.alternate_pan {
        "Two fingers: pan · Shift: orbit"
    } else {
        "Two fingers: orbit · Shift: pan"
    };
    format!(
        "{profile} · Pinch / Ctrl or ⌘ scroll: zoom · F: fit · Shift+F: fit selection · Controls: navigation preferences"
    )
}
pub fn semantic_control(
    ui: &mut egui::Ui,
    w: &mut Workspace,
    element: &cad_core::presentation::SemanticElement,
) {
    let Some(pointer) = element.data.get("pointer").and_then(|v| v.as_str()) else {
        return;
    };
    let source = serde_json::to_value(&w.view.navigation).unwrap_or_default();
    let Some(current) = source.pointer(pointer) else {
        return;
    };
    let mut value = current.clone();
    let changed = if let Some(v) = value.as_bool() {
        let mut candidate = v;
        let changed = ui.checkbox(&mut candidate, &element.label).changed();
        value = serde_json::json!(candidate);
        changed
    } else if pointer == "/precision_modifier" {
        let mut candidate = w.view.navigation.precision_modifier;
        egui::ComboBox::from_id_salt(("navigation", pointer))
            .selected_text(format!("{:?}", candidate))
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut candidate, PrecisionModifier::Alt, "Alt: precision");
                ui.selectable_value(
                    &mut candidate,
                    PrecisionModifier::AltShift,
                    "Alt + Shift: precision",
                );
                ui.selectable_value(
                    &mut candidate,
                    PrecisionModifier::None,
                    "No precision modifier",
                );
            });
        ui.label(&element.label);
        value = serde_json::to_value(candidate).unwrap_or_default();
        candidate != w.view.navigation.precision_modifier
    } else if let Some(v) = value.as_f64() {
        let mut candidate = v as f32;
        let range = match pointer {
            "/orbit_sensitivity" => 0.0005..=0.012,
            "/pan_sensitivity" => 0.0002..=0.008,
            "/precision_scale" => 0.02..=0.8,
            _ => 0.1..=4.,
        };
        let changed = ui
            .add(
                egui::Slider::new(&mut candidate, range)
                    .text(&element.label)
                    .logarithmic(true),
            )
            .changed();
        value = serde_json::json!(candidate);
        changed
    } else {
        false
    };
    if changed {
        set(w, pointer, value);
    }
}
