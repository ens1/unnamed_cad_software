//! The cursor supplies unconstrained sides. Typed sides become editable sketch dimensions.
use super::*;
use crate::presentation_controls as controls;
use cad_core::presentation::SemanticElement;
use serde_json::{Value, json};

const AXES: [&str; 2] = ["Width", "Height"];
const PLACE: &str = "sketch.place-sized-rectangle";

pub(super) struct Input {
    token: Id,
    values: [String; 2],
    cursor: [f64; 2],
    first_frame: bool,
    message: Option<String>,
}

pub(super) fn begin(sketch: &mut EditingSketch, point: [f64; 2], join: bool) {
    sketch.clicks = vec![point];
    sketch.click_join = vec![join];
    sketch.placement_error = None;
    sketch.rectangle = Some(Input {
        token: id(),
        values: Default::default(),
        cursor: point,
        first_frame: true,
        message: None,
    });
}

fn active(w: &Workspace) -> Option<(&EditingSketch, &Input)> {
    let sketch = w.sketch.as_ref()?;
    (matches!(sketch.tool, Tool::Rectangle | Tool::CenterRectangle) && sketch.clicks.len() == 1)
        .then_some((sketch, sketch.rectangle.as_ref()?))
}

pub(crate) fn semantics(w: &Workspace) -> Vec<SemanticElement> {
    let Some((s, input)) = active(w) else {
        return vec![];
    };
    let data = json!({"contract":"tcad.sketch.rectangle-placement","version":1,"document":w.session.visible().id,
        "sketch":s.sketch_id,"placement":input.token,"origin":s.clicks[0],"centered":s.tool==Tool::CenterRectangle});
    let mut elements = vec![];
    for (i, label) in AXES.iter().enumerate() {
        let mut data = data.clone();
        data["axis"] = json!(i);
        data["setter"] = json!("sketch-rectangle-size");
        data["value_type"] = json!("text");
        data["editable"] = json!(true);
        data["json"] = json!(input.values[i]);
        elements.push(SemanticElement {
            id: format!("sketch:{}/rectangle/{}", s.sketch_id, label.to_lowercase()),
            role: "sketch-placement-field".into(),
            label: (*label).into(),
            value: Some(input.values[i].clone()),
            state: "draft".into(),
            commands: vec![],
            relationships: vec![s.sketch_id.clone()],
            data,
        });
    }
    elements.push(SemanticElement {
        id: format!("sketch:{}/rectangle/place", s.sketch_id),
        role: "sketch-placement-action".into(),
        label: "Place rectangle".into(),
        value: None,
        state: "draft".into(),
        commands: vec![PLACE.into()],
        relationships: vec![s.sketch_id.clone()],
        data,
    });
    elements
}

fn matches_input(w: &Workspace, data: &Value) -> bool {
    active(w).is_some_and(|(s, input)| {
        data["document"] == w.session.visible().id
            && data["sketch"] == s.sketch_id
            && data["placement"] == input.token
    })
}
pub(crate) fn current_value(w: &Workspace, e: &SemanticElement) -> Option<Value> {
    if !matches_input(w, &e.data) {
        return None;
    }
    Some(json!(
        active(w)?.1.values.get(e.data["axis"].as_u64()? as usize)?
    ))
}
pub(crate) fn set_value(
    w: &mut Workspace,
    e: &SemanticElement,
    value: Value,
) -> cad_core::Result<()> {
    if !matches_input(w, &e.data) {
        return Err(CadError::Invalid(
            "This rectangle placement has ended".into(),
        ));
    }
    let input = w
        .sketch
        .as_mut()
        .and_then(|s| s.rectangle.as_mut())
        .unwrap();
    let target = e.data["axis"]
        .as_u64()
        .and_then(|axis| input.values.get_mut(axis as usize))
        .ok_or_else(|| CadError::Invalid("Unknown rectangle dimension".into()))?;
    *target = value
        .as_str()
        .ok_or_else(|| CadError::Invalid("Enter a length or expression".into()))?
        .to_owned();
    input.message = None;
    Ok(())
}

struct Size {
    vertices: [[f64; 2]; 4],
    dimensions: [Option<Expr>; 2],
}
fn resolve(w: &Workspace) -> cad_core::Result<Size> {
    let (s, input) =
        active(w).ok_or_else(|| CadError::Invalid("Pick the first rectangle point".into()))?;
    let a = s.clicks[0];
    let centered = s.tool == Tool::CenterRectangle;
    let mut corner = input.cursor;
    let mut dimensions = [None, None];
    for i in 0..2 {
        let text = input.values[i].trim();
        if text.is_empty() {
            continue;
        }
        let expr = crate::quantity_display::input_length(&w.session.visible().settings, text);
        let length = cad_core::expression::value(
            w.session.visible(),
            &s.definition,
            &expr,
            Dimension::Length,
        )
        .map_err(|e| CadError::Invalid(format!("{} · {e}", AXES[i])))?;
        if !length.is_finite() || length <= 0. {
            return Err(CadError::Invalid(format!("{} must be positive", AXES[i])));
        }
        let sign = if corner[i] < a[i] { -1. } else { 1. };
        corner[i] = a[i] + sign * length * if centered { 0.5 } else { 1. };
        dimensions[i] = Some(expr);
    }
    let opposite = if centered {
        [2. * a[0] - corner[0], 2. * a[1] - corner[1]]
    } else {
        a
    };
    let lo = [opposite[0].min(corner[0]), opposite[1].min(corner[1])];
    let hi = [opposite[0].max(corner[0]), opposite[1].max(corner[1])];
    Ok(Size {
        vertices: [lo, [hi[0], lo[1]], hi, [lo[0], hi[1]]],
        dimensions,
    })
}

fn finish(w: &mut Workspace, join_corner: bool) -> cad_core::Result<()> {
    let size = resolve(w)?;
    let s = w.sketch.as_mut().unwrap();
    for (i, label) in AXES.iter().enumerate() {
        if (size.vertices[2][i] - size.vertices[0][i]).abs() < 1e-8 {
            return Err(CadError::Invalid(format!(
                "Set {label} or move the pointer to size the rectangle"
            )));
        }
    }
    let first = s.clicks[0];
    let join_first = s.click_join.first() == Some(&true);
    let cursor = s.rectangle.as_ref().unwrap().cursor;
    let same = |p: [f64; 2], q: [f64; 2]| (p[0] - q[0]).hypot(p[1] - q[1]) < 1e-7;
    let join = |p| join_first && same(p, first) || join_corner && same(p, cursor);
    s.history.push(sketch_definition(s));
    let keys = size.vertices.map(|p| add_point(s, p, join(p)));
    for i in 0..4 {
        line(s, keys[i].clone(), keys[(i + 1) % 4].clone());
    }
    let constraint = |kind: &str, entities: Vec<Id>, value: Expr| SketchConstraint {
        id: id(),
        kind: kind.into(),
        entities,
        value,
        driven: false,
        distance_side: None,
        contact: None,
    };
    for (i, kind) in ["horizontal", "vertical", "horizontal", "vertical"]
        .iter()
        .enumerate()
    {
        s.constraints.push(constraint(
            kind,
            vec![keys[i].clone(), keys[(i + 1) % 4].clone()],
            "0 mm".into(),
        ));
    }
    for (i, expression) in size.dimensions.into_iter().enumerate() {
        if let Some(value) = expression {
            s.constraints.push(constraint(
                ["distance_x", "distance_y"][i],
                vec![keys[i].clone(), keys[i + 1].clone()],
                value,
            ));
        }
    }
    if s.tool == Tool::CenterRectangle {
        let center = add_point(s, first, join_first);
        let diagonal = id();
        s.members.insert(
            diagonal.clone(),
            SketchMember {
                construction: true,
                curve: SketchCurve::Line {
                    start: keys[0].clone(),
                    end: keys[2].clone(),
                },
            },
        );
        s.constraints.push(constraint(
            "midpoint",
            vec![center, diagonal],
            "0 mm".into(),
        ));
    }
    s.clicks.clear();
    s.click_join.clear();
    s.rectangle = None;
    s.placement_error = None;
    solve(w);
    Ok(())
}
fn place(w: &mut Workspace, join: bool) {
    if let Err(e) = finish(w, join)
        && let Some(input) = w.sketch.as_mut().and_then(|s| s.rectangle.as_mut())
    {
        input.message = Some(e.to_string());
        w.status = e.to_string();
    }
}
pub(crate) fn action(w: &mut Workspace, command: &str, data: &Value) -> bool {
    if command != PLACE {
        return false;
    }
    if matches_input(w, data) {
        place(w, false);
    }
    true
}

pub(super) fn overlay(
    w: &mut Workspace,
    ui: &mut egui::Ui,
    r: egui::Rect,
    response: &egui::Response,
    toolbar: egui::Rect,
    cursor: Option<[f64; 2]>,
    join: bool,
) -> bool {
    let Some((s, input)) = active(w) else {
        return false;
    };
    let plane = s.plane.clone();
    let matrix = s.matrix;
    let token = input.token.clone();
    let default = egui::Rect::from_min_size(
        r.left_bottom() + egui::vec2(12., -73.),
        egui::vec2(235., 61.).min(r.size()),
    );
    let panel = controls::overlay_rect(
        "sketch.rectangle-size",
        "sketch-placement-input",
        r,
        default,
    );
    let pointer = ui.input(|i| i.pointer.hover_pos());
    let on_panel = panel.zip(pointer).is_some_and(|(r, p)| r.contains(p));
    if !on_panel
        && let Some(cursor) = cursor
        && let Some(input) = w.sketch.as_mut().and_then(|s| s.rectangle.as_mut())
    {
        input.cursor = cursor;
    }
    let mut enter = false;
    if let Some(panel) = panel {
        let settings = &w.session.visible().settings;
        let s = w.sketch.as_mut().unwrap();
        let input = s.rectangle.as_mut().unwrap();
        let mut form = ui.new_child(
            egui::UiBuilder::new()
                .id_salt(("rectangle-size", &token))
                .max_rect(panel),
        );
        let props = controls::properties(
            "sketch.rectangle-size",
            "sketch-placement-input",
            &["viewport-overlay"],
            "active",
        );
        controls::apply_properties(&mut form, &props);
        egui::Frame::popup(form.style()).show(&mut form, |ui| {
            ui.horizontal(|ui| {
                for (axis, label) in AXES.iter().enumerate() {
                    ui.vertical(|ui| {
                        ui.label(*label);
                        let id = ui.make_persistent_id(axis);
                        if input.first_frame && axis == 0 {
                            ui.memory_mut(|m| m.request_focus(id));
                        }
                        let measured = (input.cursor[axis] - s.clicks[0][axis]).abs()
                            * if s.tool == Tool::CenterRectangle {
                                2.
                            } else {
                                1.
                            };
                        let edit = ui.add(
                            egui::TextEdit::singleline(&mut input.values[axis])
                                .id(id)
                                .desired_width((panel.width() * 0.5 - 18.).max(20.))
                                .hint_text(settings.format_length(measured)),
                        ).on_hover_text("Type a driving length or expression. Tab changes fields. Enter places the rectangle.");
                        if edit.changed() {
                            input.message = None;
                        }
                        enter |= (edit.has_focus() || edit.lost_focus())
                            && ui.input_mut(|i| {
                                i.consume_key(egui::Modifiers::NONE, egui::Key::Enter)
                            });
                    });
                }
            });
            if let Some(message) = &input.message {
                ui.colored_label(ui.visuals().error_fg_color, message);
            }
        });
        input.first_frame = false;
    }
    if let Ok(size) = resolve(w) {
        let color = viewport::selection_color(w);
        let stroke = Stroke::new(1.6, color);
        draw_polyline(w, ui, r, (&plane, &matrix), &size.vertices, true, stroke);
        if let Some((s, _)) = active(w) {
            let mut points = size.vertices.to_vec();
            points.push(size.vertices[0]);
            if let Ok(paths) = cad_core::sketch_transform::dynamic_preview(
                w.session.visible(),
                &s.definition,
                &sketch_definition(s),
                &points,
            ) {
                for path in paths {
                    draw_polyline(w, ui, r, (&plane, &matrix), &path, false, stroke);
                }
            }
        }
    }
    let click = response.clicked()
        && !on_panel
        && pointer.is_some_and(|p| !toolbar.contains(p))
        && cursor.is_some();
    if enter || click {
        place(w, click && join);
    }
    // The next click belongs to the pending rectangle, including rejected inputs.
    true
}
