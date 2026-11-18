//! A disposable cutaway of the model. Only saving a named view or drawing authors a plane.
mod drawing;
pub(crate) mod mesh;
use crate::{Workspace, presentation_controls as controls, viewport};
use bevy::math::DVec3;
use bevy_egui::egui;
use cad_core::{
    CadError, Result, drawing_sections::Frame, presentation::SemanticElement,
    protocol::RequestIdentity, selection_actions::SelectionTarget,
};
use serde_json::{Value, json};

#[derive(Default, PartialEq)]
enum Pick {
    #[default]
    None,
    Plane,
    First,
    Second(Frame),
}
#[derive(Default)]
pub(crate) struct State {
    base: Option<Frame>,
    offset: f64,
    pub(crate) visible_caps: Option<(Frame, Vec<mesh::CapOutline>)>,
    pick: Pick,
    measured: Option<([f64; 3], [f64; 3])>,
    measurement_source: Option<RequestIdentity>,
    first_face: Option<viewport::Selection>,
    drag: Option<(f64, egui::Vec2)>,
}
pub(crate) fn available(w: &Workspace) -> bool {
    !w.view.drawing_workspace
        && w.view.history_cursor.is_none()
        && w.tool_checkpoint.is_none()
        && w.sketch.is_none()
        && !crate::sketch3d_ui::active(w)
}
pub(crate) fn plane(w: &Workspace) -> Option<Frame> {
    if w.view.drawing_workspace
        || w.view.history_cursor.is_some()
        || w.sketch.is_some()
        || w.tool_checkpoint.is_some() && !crate::camera_views_ui::active(w)
    {
        return None;
    }
    let mut f = w.live_section.base?;
    f.origin = std::array::from_fn(|i| f.origin[i] + w.live_section.offset * f.normal[i]);
    Some(f)
}
pub(crate) fn cap_hit(w: &Workspace, rect: egui::Rect, p: egui::Pos2) -> Option<f64> {
    let (frame, outlines) = w.live_section.visible_caps.as_ref()?;
    if Some(*frame) != plane(w) {
        return None;
    }
    let (o, d) = viewport::ray(w, rect, p);
    let denominator = DVec3::from(d).dot(frame.normal.into());
    if denominator.abs() < 1e-12 {
        return None;
    }
    let t = -frame.distance(o) / denominator;
    if t < 0. {
        return None;
    }
    let at = DVec3::from(o) + DVec3::from(d) * t - DVec3::from(frame.origin);
    let uv = [at.dot(frame.u.into()), at.dot(frame.v.into())];
    outlines
        .iter()
        .any(|outline| outline.contains(uv))
        .then_some(t)
}
pub(crate) fn restore(w: &mut Workspace, base: Option<Frame>) {
    w.rendered_key.clear();
    w.live_section = State {
        base,
        ..Default::default()
    };
}
fn from_axes(origin: [f64; 3], u: DVec3, v: DVec3) -> Option<Frame> {
    let u = u.try_normalize()?;
    let normal = u.cross(v).try_normalize()?;
    Some(Frame {
        origin,
        u: u.to_array(),
        v: normal.cross(u).to_array(),
        normal: normal.to_array(),
    })
}
fn face(w: &Workspace, pick: &viewport::Selection) -> Option<Frame> {
    if pick.reference.topology != "face" {
        return None;
    }
    let e = viewport::selection_evaluation(w)?;
    let inst = viewport::display_instances(w, e)
        .into_iter()
        .find(|i| i.path == pick.path && i.definition == pick.definition)?;
    let mut faces = viewport::selection_meshes(w, e, inst)
        .into_iter()
        .flat_map(|m| m.faces.iter())
        .filter(|f| viewport::selected_topology(pick, &f.reference, &f.body));
    let p = faces.next()?.plane.as_ref()?;
    if faces.next().is_some() {
        return None;
    }
    let at = DVec3::from(viewport::placed(p.origin, inst));
    let direction = |v: [f64; 3]| {
        DVec3::from(viewport::placed(
            (DVec3::from(p.origin) + DVec3::from(v)).to_array(),
            inst,
        )) - at
    };
    let mut f = from_axes(at.to_array(), direction(p.u), direction(p.v))?;
    // Use a point on the true plane close to the click as the on-screen handle.
    let n = DVec3::from(f.normal);
    f.origin = (DVec3::from(pick.point) - n * f.distance(pick.point)).to_array();
    Some(f)
}
fn towards_eye(w: &Workspace, f: Frame) -> Frame {
    if f.distance(crate::camera_views_ui::current(w).frame().eye_mm) < 0. {
        f.flipped()
    } else {
        f
    }
}
pub(crate) fn selection_action(w: &Workspace, target: &SelectionTarget) -> Option<SemanticElement> {
    if !available(w)
        || !matches!(target,SelectionTarget::Geometry{reference,..} if reference.topology=="face")
    {
        return None;
    }
    face(w, w.selected.as_ref()?)?;
    let mut e = SemanticElement {
        id: "selection:live-section".into(),
        role: "selection-action".into(),
        label: "Section Here".into(),
        value: Some("Look inside from this face".into()),
        state: "ready".into(),
        commands: vec!["selection.live-section".into()],
        relationships: vec![target.definition().into()],
        data: json!({"target":target,"group":"Inspect"}),
    };
    cad_core::selection_actions::menu_metadata(&mut e);
    Some(e)
}
const ACTIONS: &[(&str, &str)] = &[
    ("live-section.face", "Pick face"),
    ("live-section.front", "Front"),
    ("live-section.top", "Top"),
    ("live-section.right", "Right"),
    ("live-section.flip", "Flip"),
    ("live-section.measure", "Measure gap"),
    ("live-section.drawing", "Create section drawing"),
    ("live-section.close", "Close section"),
];
fn instruction(w: &Workspace) -> &'static str {
    match w.live_section.pick {
        Pick::Plane => "Pick a planar face or a base plane",
        Pick::First => "Pick the first flat face",
        Pick::Second(_) => "Pick a parallel face",
        Pick::None => "Section",
    }
}
fn toggle_plane(w: &mut Workspace, frame: Frame) {
    restore(w, Some(towards_eye(w, frame)));
    w.selection_popup = None;
}
pub(crate) fn action(w: &mut Workspace, cmd: &str) -> bool {
    if cmd != "viewport.live-section" && !ACTIONS.iter().any(|(op, _)| *op == cmd) {
        return false;
    }
    if cmd == "live-section.close" {
        restore(w, None);
        return true;
    }
    if !available(w) {
        w.status = "Return to the model and finish the current edit to use Live Section".into();
        return true;
    }
    if cmd != "viewport.live-section" && plane(w).is_none() && w.live_section.pick != Pick::Plane {
        w.status = "Open Live Section first".into();
        return true;
    }
    match cmd {
        "viewport.live-section" => {
            if let Some(f) = w.selected.as_ref().and_then(|s| face(w, s)) {
                toggle_plane(w, f)
            } else if plane(w).is_none() {
                w.live_section.pick = Pick::Plane;
            }
        }
        "live-section.face" => w.live_section.pick = Pick::Plane,
        "live-section.front" | "live-section.top" | "live-section.right" => {
            let p = crate::sketch::plane(match cmd {
                "live-section.front" => "Front Plane",
                "live-section.right" => "Right Plane",
                _ => "Top Plane",
            });
            if let Some(f) = from_axes(p.origin, p.u.into(), p.v.into()) {
                toggle_plane(w, f);
            }
        }
        "live-section.flip" => {
            if let Some(base) = w.live_section.base {
                let offset = w.live_section.offset;
                restore(w, Some(base.flipped()));
                w.live_section.offset = -offset;
            }
        }
        "live-section.measure" => {
            if plane(w).is_some() {
                w.live_section.pick = Pick::First;
                w.live_section.measured = None;
                w.live_section.measurement_source = None;
            }
        }
        "live-section.drawing" => {
            if let Err(err) = drawing::create(w) {
                w.status = err.to_string()
            }
        }
        _ => {}
    }
    w.selection_popup = None;
    true
}
pub(crate) fn escape(w: &mut Workspace) -> bool {
    if w.live_section.pick != Pick::None {
        w.live_section.pick = Pick::None;
        true
    } else if w.live_section.base.is_some() {
        restore(w, None);
        true
    } else {
        false
    }
}
fn pick_face(w: &mut Workspace, rect: egui::Rect, p: egui::Pos2) {
    let picked = viewport::pick_section_faces(w, rect, p)
        .into_iter()
        .next()
        .and_then(|s| Some((face(w, &s)?, s)));
    let Some((f, pick)) = picked else {
        w.status = "Choose a flat model face".into();
        return;
    };
    match w.live_section.pick {
        Pick::Plane => toggle_plane(w, f),
        Pick::First => {
            w.live_section.pick = Pick::Second(f);
            w.live_section.first_face = Some(pick);
            w.live_section.measurement_source =
                viewport::selection_evaluation(w).map(|e| e.identity.clone());
        }
        Pick::Second(start) => {
            if w.live_section.first_face.as_ref().is_some_and(|first| {
                first.path == pick.path
                    && viewport::selected_topology(first, &pick.reference, &pick.body)
            }) {
                w.status = "Pick a different flat face".into();
                return;
            }
            if DVec3::from(start.normal).cross(f.normal.into()).length() > 1e-7 {
                w.status = "Choose a face parallel to the first face".into();
                return;
            }
            let a = DVec3::from(f.origin) - DVec3::from(start.normal) * start.distance(f.origin);
            w.live_section.measured = Some((a.to_array(), f.origin));
            w.live_section.pick = Pick::None;
        }
        Pick::None => {}
    }
}
pub(crate) fn elements(w: &Workspace) -> Vec<SemanticElement> {
    if !available(w) || plane(w).is_none() && w.live_section.pick == Pick::None {
        return vec![];
    }
    let mut out = vec![SemanticElement {
        id: "viewport.live-section.controls".into(),
        role: "live-section-panel".into(),
        label: instruction(w).into(),
        value: None,
        state: "active".into(),
        commands: vec![],
        relationships: vec![],
        data: json!({}),
    }];
    let active = plane(w).is_some();
    for &(id, label) in ACTIONS {
        let needs_plane = matches!(
            id,
            "live-section.measure" | "live-section.flip" | "live-section.drawing"
        );
        out.push(SemanticElement {
            id: id.into(),
            role: "live-section-action".into(),
            label: label.into(),
            value: None,
            state: if needs_plane && !active {
                "disabled"
            } else {
                "ready"
            }
            .into(),
            commands: vec![id.into()],
            relationships: vec!["viewport.live-section.controls".into()],
            data: json!({}),
        });
    }
    if active {
        let unit = w.session.visible().settings.display_length_unit;
        let number = w.live_section.offset / unit.millimeters_per_unit();
        out.push(SemanticElement {
            id:"live-section.offset".into(),
            role:"live-section-field".into(),
            label:"Plane offset".into(),
            value:Some(crate::quantity_display::expression(&format!("{} mm",w.live_section.offset))),
            state:"ready".into(),
            commands:vec![],
            relationships:vec!["viewport.live-section.controls".into()],
            data:json!({"setter":"live-section","field":"offset","json":number,"control":"number","unit":unit.symbol(),"document":w.session.visible().id,"plane":w.live_section.base}),
        });
        if let Some((a, b)) = w.live_section.measured {
            let mm = DVec3::from(a).distance(b.into());
            out.push(SemanticElement {
                id: "live-section.gap".into(),
                role: "measurement".into(),
                label: "Plane spacing".into(),
                value: Some(crate::quantity_display::expression(&format!("{mm} mm"))),
                state: "ready".into(),
                commands: vec![],
                relationships: vec!["viewport.live-section.controls".into()],
                data: json!({"millimeters":mm}),
            });
        }
    }
    out
}
pub(crate) fn value(w: &Workspace, e: &SemanticElement) -> Option<Value> {
    (plane(w).is_some()
        && e.id == "live-section.offset"
        && e.data["document"] == w.session.visible().id
        && e.data["plane"] == json!(w.live_section.base))
    .then(|| {
        json!(
            w.live_section.offset
                / w.session
                    .visible()
                    .settings
                    .display_length_unit
                    .millimeters_per_unit()
        )
    })
}
pub(crate) fn set_value(w: &mut Workspace, e: &SemanticElement, v: Value) -> Result<()> {
    if !available(w) || value(w, e).is_none() {
        return Err(CadError::Invalid("Reopen the section controls".into()));
    }
    let x = v
        .as_f64()
        .or_else(|| v.as_str()?.parse().ok())
        .unwrap_or(f64::NAN)
        * w.session
            .visible()
            .settings
            .display_length_unit
            .millimeters_per_unit();
    if !x.is_finite() {
        return Err(CadError::Invalid("Enter a finite plane offset".into()));
    }
    w.live_section.offset = x;
    Ok(())
}
fn button(ui: &mut egui::Ui, w: &mut Workspace, id: &str, label: &str) -> bool {
    let clicked = controls::button(
        ui,
        id,
        "live-section-action",
        &["viewport-action"],
        label,
        true,
        false,
    )
    .clicked();
    if clicked {
        action(w, id);
    }
    clicked
}
pub(crate) fn overlay(
    w: &mut Workspace,
    ui: &mut egui::Ui,
    rect: egui::Rect,
    response: &egui::Response,
    reserved: &[egui::Rect],
) -> bool {
    if !available(w) || w.live_section.base.is_none() && w.live_section.pick == Pick::None {
        return false;
    }
    if w.live_section
        .measurement_source
        .as_ref()
        .is_some_and(|id| viewport::selection_evaluation(w).is_none_or(|e| &e.identity != id))
    {
        w.live_section.measured = None;
        w.live_section.measurement_source = None;
        if matches!(w.live_section.pick, Pick::Second(_)) {
            w.live_section.pick = Pick::First
        }
    }
    let anchor = egui::pos2(rect.left() + 12., rect.bottom() - 88.);
    let Some(slot) = controls::overlay_rect(
        "viewport.live-section.controls",
        "live-section-panel",
        rect,
        egui::Rect::from_min_size(
            anchor,
            egui::vec2((rect.width() - 24.).clamp(1., 500.), 80.),
        ),
    ) else {
        return false;
    };
    let mut panel = ui.new_child(egui::UiBuilder::new().max_rect(slot));
    controls::apply_properties(
        &mut panel,
        &controls::properties(
            "viewport.live-section.controls",
            "live-section-panel",
            &["viewport-overlay"],
            "active",
        ),
    );
    let unit = w.session.visible().settings.display_length_unit;
    let scale = unit.millimeters_per_unit();
    let drawn = egui::Frame::popup(panel.style())
        .show(&mut panel, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.strong(instruction(w));
                if let Some((a, b)) = w.live_section.measured {
                    let gap = crate::quantity_display::expression(&format!(
                        "{} mm",
                        DVec3::from(a).distance(b.into())
                    ));
                    ui.label(format!("Plane spacing  {gap}"));
                }
                if plane(w).is_some() {
                    let mut offset = w.live_section.offset / scale;
                    if ui
                        .add(
                            egui::DragValue::new(&mut offset)
                                .prefix("Offset ")
                                .suffix(format!(" {}", unit.symbol()))
                                .speed(0.25 / scale),
                        )
                        .on_hover_text("Drag to move the plane. Click to type an offset.")
                        .changed()
                        && offset.is_finite()
                    {
                        w.live_section.offset = offset * scale;
                    }
                }
            });
            ui.horizontal_wrapped(|ui| {
                if plane(w).is_none() || w.live_section.pick == Pick::Plane {
                    for &(id, name) in &ACTIONS[1..4] {
                        button(ui, w, id, name);
                    }
                } else {
                    button(ui, w, "live-section.face", "Change plane");
                    button(ui, w, "live-section.flip", "Flip");
                    button(ui, w, "live-section.measure", "Measure gap");
                    button(ui, w, "live-section.drawing", "To drawing");
                }
                button(ui, w, "live-section.close", "Close");
            });
        })
        .response
        .rect;
    let hover = ui.input(|i| i.pointer.hover_pos());
    if w.live_section.drag.is_none() && hover.is_some_and(|p| drawn.contains(p)) {
        return true;
    }
    if let Some((a, b)) = w.live_section.measured
        && let (Some(a), Some(b)) = (viewport::project(w, rect, a), viewport::project(w, rect, b))
    {
        let c = ui.visuals().selection.stroke.color;
        ui.painter().line_segment([a, b], egui::Stroke::new(2., c));
        for p in [a, b] {
            ui.painter().circle_filled(p, 3., c);
        }
    }
    if let Pick::Second(f) = w.live_section.pick
        && let Some(p) = viewport::project(w, rect, f.origin)
    {
        ui.painter()
            .circle_stroke(p, 5., ui.visuals().selection.stroke);
    }
    if w.live_section.pick != Pick::None
        && response.clicked()
        && let Some(p) = response.interact_pointer_pos()
        && !reserved.iter().any(|r| r.contains(p))
    {
        pick_face(w, rect, p);
        return true;
    }
    if let Some(f) = plane(w)
        && w.live_section.pick == Pick::None
        && let Some(origin) = viewport::project(w, rect, f.origin)
    {
        let mm = (crate::camera_views_ui::current(w).distance_mm * 0.025).max(0.001);
        let projected = viewport::project(
            w,
            rect,
            (DVec3::from(f.origin) + DVec3::from(f.normal) * mm).to_array(),
        )
        .map(|p| (p - origin) / (mm as f32))
        .unwrap_or_default();
        let derivative = if projected.length() * mm as f32 >= 3. {
            projected
        } else {
            egui::vec2(
                0.,
                -(rect.height() as f64 / crate::camera_views_ui::current(w).distance_mm.max(0.1))
                    as f32,
            )
        };
        let dir = derivative.normalized();
        let tip = origin + dir * 34.;
        if rect.shrink(8.).contains(tip)
            && !drawn.expand(5.).contains(tip)
            && !reserved.iter().any(|r| r.contains(tip))
            || w.live_section.drag.is_some()
        {
            let handle = ui
                .interact(
                    egui::Rect::from_center_size(tip, egui::vec2(24., 24.)),
                    response.id.with("section-offset"),
                    egui::Sense::drag(),
                )
                .on_hover_text("Drag the section plane");
            let c = ui.visuals().selection.stroke.color;
            ui.painter()
                .arrow(origin, dir * 34., egui::Stroke::new(2., c));
            ui.painter()
                .circle_stroke(tip, 5., egui::Stroke::new(2., c));
            if handle.drag_started() {
                w.live_section.drag = Some((w.live_section.offset, derivative));
            }
            if let Some(delta) = handle.total_drag_delta()
                && let Some((start, d)) = w.live_section.drag
            {
                w.live_section.offset = start + f64::from(delta.dot(d) / d.length_sq().max(1e-12));
            }
            if handle.drag_stopped() {
                w.live_section.drag = None;
            }
            if handle.hovered() || handle.dragged() {
                return true;
            }
        }
    }
    false
}
