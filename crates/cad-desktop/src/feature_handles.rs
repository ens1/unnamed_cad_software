//! On-model controls edit the same feature expressions as the property manager.
//! The projected leader and arrow are disposable and never become CAD geometry.
use crate::{Workspace, presentation_controls as controls, viewport};
use bevy::math::DVec3;
use bevy_egui::egui;
use cad_core::{
    CadError, Dimension, Expr, ExtrudeEnd, Feature, NodeRef, Operation, PlacedPart, Plane, Result,
    TopologyRef,
    presentation::SemanticElement,
    protocol::{EvaluatedSketch, Evaluation, MeshData},
    reference_geometry::{AxisSource, PlaneSource, PointSource},
};
use serde_json::{Value, json};

#[derive(Clone, Copy, PartialEq)]
enum Field {
    Depth,
    Reverse,
    Radius,
}
impl Field {
    fn key(self) -> &'static str {
        match self {
            Self::Depth => "depth",
            Self::Reverse => "reverse-depth",
            Self::Radius => "radius",
        }
    }
    fn label(self) -> &'static str {
        match self {
            Self::Depth => "Depth",
            Self::Reverse => "Depth 2",
            Self::Radius => "Radius",
        }
    }
    fn source(self, operation: &Operation) -> Option<(&Expr, &'static str)> {
        match (self, operation) {
            (
                Self::Depth,
                Operation::Extrude {
                    distance, options, ..
                },
            ) if matches!(options.end, ExtrudeEnd::Blind | ExtrudeEnd::MidPlane) => {
                Some((distance, "/distance"))
            }
            (Self::Reverse, Operation::Extrude { options, .. })
                if !matches!(
                    options.end,
                    ExtrudeEnd::MidPlane | ExtrudeEnd::ThroughAllBoth
                ) =>
            {
                if let Some(second) = &options.direction2 {
                    (second.end == ExtrudeEnd::Blind)
                        .then_some((&second.distance, "/options/direction2/distance"))
                } else if options.end == ExtrudeEnd::Blind {
                    Some((
                        options.reverse_distance.as_ref()?,
                        "/options/reverse_distance",
                    ))
                } else {
                    None
                }
            }
            (Self::Radius, Operation::Fillet { radius, .. }) => Some((radius, "/radius")),
            _ => None,
        }
    }
    fn update(self, operation: &mut Operation, value: Expr) -> Result<()> {
        match (self, operation) {
            (Self::Depth, Operation::Extrude { distance, .. }) => *distance = value,
            (Self::Reverse, Operation::Extrude { options, .. }) => {
                if let Some(second) = &mut options.direction2 {
                    second.distance = value
                } else if let Some(second) = &mut options.reverse_distance {
                    *second = value
                } else {
                    return Err(invalid("Direction 2 is no longer active"));
                }
            }
            (Self::Radius, Operation::Fillet { radius, .. }) => *radius = value,
            _ => return Err(invalid("This feature dimension has changed")),
        }
        Ok(())
    }
}
const FIELDS: [Field; 3] = [Field::Depth, Field::Reverse, Field::Radius];
fn invalid(s: &str) -> CadError {
    CadError::Invalid(s.into())
}
fn active(w: &Workspace) -> Option<(&str, &Feature, &str)> {
    if w.dialog.as_deref() != Some("feature")
        || w.tool_checkpoint.is_none()
        || w.confirm_requested
        || w.settings.is_some()
        || w.fields.contains_key("feature-source-invalid")
    {
        return None;
    }
    let owner = w.view.active_part.as_deref()?;
    let id = w.fields.get("feature-editor-id")?;
    let f = w
        .session
        .visible()
        .parts
        .get(owner)?
        .features
        .iter()
        .find(|f| &f.id == id)?;
    Some((owner, f, w.fields.get("feature-editor-generation")?))
}
fn overridden(w: &Workspace, owner: &str, f: &Feature, path: &str) -> bool {
    let doc = w.session.visible();
    if doc
        .parts
        .get(owner)
        .is_none_or(|p| p.configuration_details.is_empty())
    {
        return false;
    }
    let occurrence = placement(w, owner).and_then(|(_, i)| i.configuration.as_ref());
    occurrence
        .or_else(|| doc.active_configurations.get(owner))
        .and_then(|id| cad_core::configuration::resolve(doc, owner, id).ok())
        .is_some_and(|config| {
            config
                .overrides
                .feature_dimensions
                .get(&f.id)
                .is_some_and(|fields| fields.contains_key(path))
        })
}
// Materializing a configuration can be expensive. Keep one resolved document
// while the editor is open; every actual source edit has a new epoch.
struct Configured {
    epoch: String,
    owner: String,
    key: Option<String>,
    document: cad_core::Document,
}
thread_local! {
    static CONFIGURED:std::cell::RefCell<Option<Configured>>=const {std::cell::RefCell::new(None)};
}
fn in_configuration<T>(
    w: &Workspace,
    owner: &str,
    text: &str,
    read: impl FnOnce(&cad_core::Document) -> Result<T>,
) -> Result<T> {
    let doc = w.session.visible();
    if crate::quantity_display::expression_with(&doc.settings, text).is_some() {
        return read(doc);
    }
    let occurrence = placement(w, owner).and_then(|(_, i)| i.configuration.as_ref());
    let key = cad_core::configuration::selected(doc, owner, occurrence);
    if key.is_none() && doc.active_configurations.is_empty() {
        return read(doc);
    }
    CONFIGURED.with_borrow_mut(|cached| {
        let epoch = w.session.source_epoch();
        if cached
            .as_ref()
            .is_none_or(|c| c.epoch != epoch || c.owner != owner || c.key.as_ref() != key)
        {
            *cached = Some(Configured {
                epoch: epoch.into(),
                owner: owner.into(),
                key: key.cloned(),
                document: cad_core::configuration::variant(doc, owner, key.map(String::as_str))?,
            });
        }
        read(&cached.as_ref().unwrap().document)
    })
}
fn length(w: &Workspace, owner: &str, expr: &Expr) -> Result<f64> {
    in_configuration(w, owner, &expr.0, |doc| {
        cad_core::expression::value(doc, owner, expr, Dimension::Length)
    })
}
fn descriptor(w: &Workspace, field: Field) -> Option<SemanticElement> {
    let (owner, feature, token) = active(w)?;
    let (source, path) = field.source(&feature.operation)?;
    if overridden(w, owner, feature, path) {
        return None;
    }
    let id = format!("on-model:{}:{}", feature.id, field.key());
    let draggable =
        crate::quantity_display::expression_with(&w.session.visible().settings, &source.0)
            .is_some();
    Some(SemanticElement {
        id: id.clone(),
        role: "feature-dimension-field".into(),
        label: field.label().into(),
        value: Some(source.0.clone()),
        state: "draft".into(),
        commands: vec![],
        relationships: vec![feature.id.clone()],
        data: json!({"contract":"tcad.presentation.feature-dimension","version":1,"editor":token,"document":w.session.visible().id,
          "owner":owner,"feature":feature.id,"field":field.key(),"source_path":path,"setter":"feature-handle-dimension","json":source.0,
          "draggable":draggable,"value_type":"quantity","dimension":"length","unit":w.session.visible().settings.display_length_unit.symbol(),"editable":true,
          "classes":["modeling-field","viewport-dimension"],"focus_request":format!("focus-request:{id}"),
          "accessibility_label":format!("{} of {}. Enter a length or expression.",field.label(),feature.name)}),
    })
}
pub(crate) fn semantics(w: &Workspace) -> Vec<SemanticElement> {
    FIELDS
        .into_iter()
        .filter_map(|field| {
            let mut e = descriptor(w, field)?;
            if let Some(r) = ruler(w, field) {
                e.data["anchor_mm"] = json!(r.start.to_array());
                e.data["direction_per_mm"] = json!(r.per_mm.to_array());
            }
            Some(e)
        })
        .collect()
}

fn binding(w: &Workspace, e: &SemanticElement) -> Option<Field> {
    let (owner, feature, token) = active(w)?;
    if e.data["editor"] != token
        || e.data["owner"] != owner
        || e.data["feature"] != feature.id
        || e.data["document"] != w.session.visible().id
    {
        return None;
    }
    FIELDS.into_iter().find(|f| {
        e.data["field"] == f.key()
            && f.source(&feature.operation)
                .is_some_and(|(_, path)| !overridden(w, owner, feature, path))
    })
}
pub(crate) fn current_value(w: &Workspace, e: &SemanticElement) -> Option<Value> {
    let field = binding(w, e)?;
    let (source, _) = field.source(&active(w)?.1.operation)?;
    Some(json!(source.0))
}
pub(crate) fn set_value(w: &mut Workspace, e: &SemanticElement, value: Value) -> Result<()> {
    let field =
        binding(w, e).ok_or_else(|| invalid("This feature dimension is no longer editable"))?;
    let (owner, feature, _) = active(w).unwrap();
    let text = value
        .as_str()
        .ok_or_else(|| invalid("Enter a length or expression"))?;
    let (expr, mm) = in_configuration(w, owner, text, |doc| {
        crate::quantity_display::input_length(doc, owner, text)
    })?;
    if !mm.is_finite() || mm <= 0. {
        return Err(invalid("Enter a positive length"));
    }
    if field
        .source(&feature.operation)
        .is_some_and(|(source, _)| *source == expr)
    {
        return Ok(());
    }
    let mut feature = feature.clone();
    field.update(&mut feature.operation, expr)?;
    w.json = serde_json::to_string_pretty(&feature)?;
    crate::ui::preview_feature(w, feature);
    if w.fields.contains_key("feature-source-invalid") {
        return Err(invalid(&w.status));
    }
    Ok(())
}

fn placement<'a>(w: &'a Workspace, owner: &str) -> Option<(&'a Evaluation, &'a PlacedPart)> {
    let checkpoint = w.tool_checkpoint.as_ref()?;
    let evaluation = checkpoint
        .draft
        .as_ref()
        .and_then(|d| d.preview_evaluation())
        .or(checkpoint.evaluation.as_ref())
        .filter(|e| e.valid)
        .or_else(|| viewport::display_evaluation(w))?;
    let known = w
        .selected
        .as_ref()
        .filter(|s| s.definition == owner)
        .map(|s| &s.path)
        .or(w.selected_feature_path.as_ref())
        .or(w.edge_selection_path.as_ref());
    let mut occurrences = viewport::display_instances(w, evaluation)
        .into_iter()
        .filter(|i| {
            i.definition == owner
                && known.is_none_or(|path| path == &i.path)
                && w.view
                    .active_instance
                    .as_ref()
                    .is_none_or(|id| i.path.contains(id) || (i.path.is_empty() && id == owner))
        });
    let instance = occurrences.next()?;
    occurrences
        .next()
        .is_none()
        .then_some((evaluation, instance))
}
struct Geometry<'a> {
    w: &'a Workspace,
    e: &'a Evaluation,
    i: &'a PlacedPart,
}
impl Geometry<'_> {
    fn key(&self, r: &NodeRef) -> String {
        let key = if r.definition == self.i.definition {
            cad_core::history::mesh_key(self.i)
        } else {
            &r.definition
        };
        format!("{key}::{}", r.node)
    }
    fn sketch(&self, r: &NodeRef) -> Option<&EvaluatedSketch> {
        self.e.sketches.get(&self.key(r))
    }
    fn point_in_sketch(&self, r: &NodeRef, key: &str) -> Option<DVec3> {
        let s = self.sketch(r)?;
        Some(sketch_point(&s.plane, *s.points.get(key)?))
    }
    fn mesh(&self, body: &NodeRef) -> Option<&MeshData> {
        let key = if body.definition == self.i.definition {
            cad_core::history::mesh_key(self.i)
        } else {
            &body.definition
        };
        self.e
            .history
            .get(key)
            .and_then(|h| h.meshes.get(&body.node))
            .map(|m| m.as_ref())
            .or_else(|| self.e.meshes.get(key))
    }
    fn face(&self, body: &NodeRef, face: &TopologyRef) -> Option<&Plane> {
        let mut faces = self.mesh(body)?.faces.iter().filter(|f| {
            f.body.as_ref() == Some(body)
                && cad_core::selection_sets::same_reference(&f.reference, face)
        });
        let plane = faces.next()?.plane.as_ref()?;
        faces.next().is_none().then_some(plane)
    }
    fn edge(&self, body: &NodeRef, edge: &TopologyRef) -> Option<&cad_core::protocol::EdgeData> {
        let mut edges = self.mesh(body)?.edges.iter().filter(|e| {
            e.body.as_ref() == Some(body)
                && cad_core::selection_sets::same_reference(&e.reference, edge)
        });
        let edge = edges.next()?;
        edges.next().is_none().then_some(edge)
    }
    fn length(&self, expr: &Expr) -> Option<f64> {
        length(self.w, &self.i.definition, expr).ok()
    }
    fn point(&self, source: &PointSource) -> Option<DVec3> {
        let datum = |feature| {
            self.e
                .references
                .get(&self.key(feature))
                .map(|r| r.geometry.origin())
        };
        match source {
            PointSource::Coordinates { coordinates: c } => Some(DVec3::new(
                self.length(&c[0])?,
                self.length(&c[1])?,
                self.length(&c[2])?,
            )),
            PointSource::SketchPoint { profile, point } => self.point_in_sketch(profile, point),
            PointSource::Datum { feature }
            | PointSource::CoordinateSystemOrigin { feature }
            | PointSource::PlaneOrigin {
                plane:
                    PlaneSource::Datum { feature } | PlaneSource::CoordinateSystem { feature, .. },
            }
            | PointSource::AxisOrigin {
                axis: AxisSource::Datum { feature } | AxisSource::CoordinateSystem { feature, .. },
            } => Some(datum(feature)?.into()),
            PointSource::PlaneOrigin {
                plane: PlaneSource::Origin { .. },
            }
            | PointSource::AxisOrigin {
                axis: AxisSource::Origin { .. },
            } => Some(DVec3::ZERO),
            PointSource::PlaneOrigin {
                plane: PlaneSource::Face { body, reference },
            } => Some(self.face(body, reference)?.origin.into()),
            PointSource::EdgePoint { source } => {
                let t = cad_core::expression::value(
                    self.w.session.visible(),
                    &self.i.definition,
                    &source.parameter,
                    Dimension::Scalar,
                )
                .ok()?;
                let edge = self
                    .edge(&source.body, &source.reference)?
                    .native
                    .as_ref()?;
                if t == 0. {
                    Some(edge.endpoints[0].into())
                } else if t == 1. {
                    Some(edge.endpoints[1].into())
                } else {
                    None
                }
            }
            _ => None,
        }
    }
    fn world(&self, p: DVec3) -> DVec3 {
        viewport::placed(p.to_array(), self.i).into()
    }
}
fn sketch_point(plane: &Plane, uv: [f64; 2]) -> DVec3 {
    DVec3::from(plane.origin) + DVec3::from(plane.u) * uv[0] + DVec3::from(plane.v) * uv[1]
}
fn normal(p: &Plane) -> Option<DVec3> {
    DVec3::from(p.u).cross(p.v.into()).try_normalize()
}
struct Ruler {
    start: DVec3,
    per_mm: DVec3,
}
fn ruler(w: &Workspace, field: Field) -> Option<Ruler> {
    let (owner, f, _) = active(w)?;
    let (e, i) = placement(w, owner)?;
    let g = Geometry { w, e, i };
    match &f.operation {
        Operation::Extrude {
            profile,
            direction,
            options,
            ..
        } => {
            let s = g.sketch(profile)?;
            let mut lo = [f64::INFINITY; 2];
            let mut hi = [f64::NEG_INFINITY; 2];
            let selected = options.contours.iter().find(|c| &c.profile == profile);
            for p in s
                .polylines
                .iter()
                .filter(|l| {
                    !l.construction && selected.is_none_or(|c| c.outer_members.contains(&l.member))
                })
                .flat_map(|l| &l.points)
            {
                for k in 0..2 {
                    lo[k] = lo[k].min(p[k]);
                    hi[k] = hi[k].max(p[k]);
                }
            }
            let uv = std::array::from_fn(|k| {
                if lo[k].is_finite() {
                    (lo[k] + hi[k]) * 0.5
                } else {
                    0.
                }
            });
            let station = sketch_point(&s.plane, uv);
            let n = normal(&s.plane)?;
            let axis = if options.profile_normal {
                if options.reverse_direction { -n } else { n }
            } else {
                DVec3::from(*direction).try_normalize()?
            };
            let shift = match &options.start {
                cad_core::extrusion::Start::SketchPlane => 0.,
                cad_core::extrusion::Start::Vertex { profile, point } => {
                    (g.point_in_sketch(profile, point)? - station).dot(n) / axis.dot(n)
                }
                cad_core::extrusion::Start::Point { point } => {
                    (g.point(point)? - station).dot(n) / axis.dot(n)
                }
                cad_core::extrusion::Start::Face { face, body } => {
                    let plane = g.face(body, face)?;
                    let n = normal(plane)?;
                    (DVec3::from(plane.origin) - station).dot(n) / axis.dot(n)
                }
            } + g.length(&options.start_offset)?;
            let start = station + axis * shift;
            let scale = if field == Field::Reverse {
                -1.
            } else if options.end == ExtrudeEnd::MidPlane {
                0.5
            } else {
                1.
            };
            Some(Ruler {
                start: g.world(start),
                per_mm: g.world(start + axis * scale) - g.world(start),
            })
        }
        Operation::Fillet { body, edges, .. } => {
            let edge = edges.iter().find_map(|r| g.edge(body, r))?;
            let points = &edge.points;
            let len = |p: &[[f64; 3]]| DVec3::from(p[0]).distance(p[1].into());
            let mut remaining = points.windows(2).map(len).sum::<f64>() * 0.5;
            for pair in points.windows(2) {
                let length = len(pair);
                if length > 1e-12 && remaining <= length {
                    let a = g.world(pair[0].into());
                    let b = g.world(pair[1].into());
                    let tangent = (b - a).try_normalize()?;
                    let frame = crate::camera_views_ui::current(w).frame();
                    let axis = DVec3::from(frame.forward)
                        .cross(tangent)
                        .try_normalize()
                        .unwrap_or(frame.up.into());
                    return Some(Ruler {
                        start: a.lerp(b, remaining / length),
                        per_mm: if axis.dot(frame.up.into()) < 0. {
                            -axis
                        } else {
                            axis
                        },
                    });
                }
                remaining -= length;
            }
            None
        }
        _ => None,
    }
}

#[derive(Clone)]
struct Drag {
    value: f64,
    pointer: egui::Pos2,
    per_mm: egui::Vec2,
}
// Pin the field while typing. A live edit can project the new cap beyond the
// window, or temporarily eliminate its geometry; the input must stay reachable.
fn exact_input(
    ui: &mut egui::Ui,
    w: &mut Workspace,
    element: &SemanticElement,
    panel: egui::Rect,
    id: egui::Id,
) -> bool {
    let mut form = ui.new_child(
        egui::UiBuilder::new()
            .id_salt(("exact-feature-dimension", id))
            .max_rect(panel),
    );
    let token = element.data["editor"].as_str().unwrap_or_default();
    let error = format!("presentation-field-error:{}:{token}", element.id);
    let mut dismiss = false;
    let response = egui::Frame::popup(form.style())
        .show(&mut form, |ui| {
            crate::ui::semantic_field(ui, w, element);
            if w.fields.contains_key(&error) && ui.small_button("Discard input").clicked() {
                w.fields.remove(&error);
                w.fields
                    .remove(&format!("presentation-field:{}:{token}", element.id));
                dismiss = true;
            }
        })
        .response;
    if !w.fields.contains_key(&error)
        && (response.clicked_elsewhere()
            || ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter)))
    {
        dismiss = true;
    }
    if dismiss {
        ui.data_mut(|d| d.remove::<egui::Rect>(id));
    }
    ui.input(|i| {
        i.pointer
            .hover_pos()
            .is_some_and(|p| response.rect.contains(p))
    })
}
pub(crate) fn viewport(
    w: &mut Workspace,
    ui: &mut egui::Ui,
    r: egui::Rect,
    blocked: &[egui::Rect],
) -> bool {
    if active(w).is_none() {
        CONFIGURED.with_borrow_mut(|cached| *cached = None);
        return false;
    }
    let mut captured = false;
    let mut labels = vec![];
    for field in FIELDS {
        let Some(element) = descriptor(w, field) else {
            // Changing end condition can remove a field containing unfinished
            // text. Only errors for still-applicable fields can block Apply.
            if let Some((_, feature, token)) = active(w) {
                let error = format!(
                    "presentation-field-error:on-model:{}:{}:{token}",
                    feature.id,
                    field.key()
                );
                w.fields.remove(&error);
            }
            continue;
        };
        let input_id =
            ui.make_persistent_id((&element.id, element.data["editor"].as_str(), "exact"));
        if let Some(open) = ui.data(|d| d.get_temp::<egui::Rect>(input_id)) {
            let inside = r.clamp(open.min);
            let corner = egui::pos2(
                inside.x.min((r.right() - open.width()).max(r.left())),
                inside.y.min((r.bottom() - open.height()).max(r.top())),
            );
            captured |= exact_input(
                ui,
                w,
                &element,
                egui::Rect::from_min_size(corner, open.size().min(r.size())),
                input_id,
            );
            continue;
        }
        let Some(ruler) = ruler(w, field) else {
            continue;
        };
        let Some((owner, feature, token)) = active(w) else {
            break;
        };
        let source = field.source(&feature.operation).unwrap().0;
        let Ok(value) = length(w, owner, source) else {
            continue;
        };
        let literal = element.data["draggable"] == true;
        let drag_id = ui.make_persistent_id((&element.id, token, "drag"));
        let Some(base) = viewport::project(w, r, ruler.start.to_array()) else {
            continue;
        };
        let tip_world = ruler.start + ruler.per_mm * value;
        let Some(mut tip) = viewport::project(w, r, tip_world.to_array()) else {
            continue;
        };
        let Some(unit) = viewport::project(w, r, (tip_world + ruler.per_mm).to_array()) else {
            continue;
        };
        let mut per_mm = unit - tip;
        // In a normal-to view the extrusion axis projects to a point. Offer a
        // short vertical leader at the cross section, scaled to the current zoom.
        let up = DVec3::from(crate::camera_views_ui::current(w).frame().up);
        let Some(up_sample) =
            viewport::project(w, r, (tip_world + up * ruler.per_mm.length()).to_array())
        else {
            continue;
        };
        let up_scale = up_sample.distance(tip);
        if per_mm.length() < (up_scale * 0.12).max(1e-4) {
            let sign = if field == Field::Reverse { 1. } else { -1. };
            per_mm = egui::vec2(0., sign * up_scale.max(0.01));
            tip = base
                + per_mm.normalized()
                    * (per_mm.length() * value as f32).clamp(28., r.height() * 0.4 + 28.);
        }
        let props = controls::properties(
            &element.id,
            "feature-dimension-handle",
            &["viewport-overlay"],
            "draft",
        );
        if props.get("display").is_some_and(|s| s == "none")
            || props.get("visibility").is_some_and(|s| s == "hidden")
        {
            continue;
        }
        // Keep a small blend or cut easy to grab without covering the source edge.
        if base.distance(tip) < 20. && per_mm.length_sq() > 1e-8 {
            tip = base + per_mm.normalized() * 20.;
        }
        let grip = controls::number(&props, "min-target-size", 24.).max(14.);
        let hit = egui::Rect::from_center_size(tip, egui::Vec2::splat(grip));
        let dragging = ui.ctx().is_being_dragged(drag_id);
        if !dragging && (!r.contains(tip) || blocked.iter().any(|b| b.intersects(hit))) {
            continue;
        }
        let color = props
            .get("color")
            .and_then(|s| crate::ui::color(s))
            .unwrap_or_else(|| viewport::selection_color(w));
        let stroke = egui::Stroke::new(controls::number(&props, "border-width", 1.5), color);
        ui.painter().line_segment([base, tip], stroke);
        if field != Field::Radius {
            let head = (tip - base).normalized();
            let back = tip - head * 10.;
            let side = egui::vec2(-head.y, head.x) * 5.;
            ui.painter().line_segment([back - side, tip], stroke);
            ui.painter().line_segment([back + side, tip], stroke);
        }
        ui.painter()
            .circle_filled(tip, (grip * 0.21).clamp(3., 8.), color);
        ui.painter()
            .circle_stroke(tip, (grip * 0.31).clamp(4.5, 12.), stroke);
        let response = ui
            .interact(
                hit,
                drag_id,
                if literal {
                    egui::Sense::click_and_drag()
                } else {
                    egui::Sense::click()
                },
            )
            .on_hover_cursor(if literal {
                egui::CursorIcon::Grab
            } else {
                egui::CursorIcon::PointingHand
            })
            .on_hover_text(if literal {
                "Drag to size. Click to enter a value."
            } else {
                "Click to edit the driving expression."
            });
        if response.drag_started_by(egui::PointerButton::Primary)
            && let Some(pointer) = ui.input(|i| i.pointer.press_origin())
        {
            ui.data_mut(|d| {
                d.insert_temp(
                    drag_id,
                    Drag {
                        value,
                        pointer,
                        per_mm,
                    },
                )
            });
        }
        if (response.dragged_by(egui::PointerButton::Primary)
            || response.drag_stopped_by(egui::PointerButton::Primary))
            && let Some(drag) = ui.data(|d| d.get_temp::<Drag>(drag_id))
            && let Some(pointer) = response.interact_pointer_pos()
        {
            let change = (pointer - drag.pointer).dot(drag.per_mm) / drag.per_mm.length_sq();
            let mm = (drag.value + f64::from(change)).max(1e-6);
            if change.is_finite() && (mm - value).abs() > 1e-8 {
                let result = set_value(w, &element, json!(format!("{mm:.8} mm")));
                if let Err(e) = result {
                    w.status = e.to_string();
                }
            }
        }
        if response.drag_stopped() {
            ui.data_mut(|d| d.remove::<Drag>(drag_id));
        }
        captured |= response.hovered()
            || response.is_pointer_button_down_on()
            || response.drag_stopped()
            || response.clicked();
        let size = egui::vec2(120., 28.).min(r.size());
        let clamp = |p: egui::Pos2| {
            egui::pos2(
                p.x.clamp(r.left(), (r.right() - size.x).max(r.left())),
                p.y.clamp(r.top(), (r.bottom() - size.y).max(r.top())),
            )
        };
        let offsets = [
            egui::vec2(14., -size.y * 0.5),
            egui::vec2(-14. - size.x, -size.y * 0.5),
            egui::vec2(-size.x * 0.5, 14.),
            egui::vec2(-size.x * 0.5, -14. - size.y),
        ];
        let default = offsets
            .into_iter()
            .map(|offset| egui::Rect::from_min_size(clamp(tip + offset), size))
            .find(|b| {
                !blocked
                    .iter()
                    .chain(&labels)
                    .any(|other| other.intersects(*b))
            });
        let Some(panel) = default.and_then(|default| {
            controls::overlay_rect(
                &format!("viewport.dimension.{}", field.key()),
                "viewport-dimension-input",
                r,
                default,
            )
        }) else {
            continue;
        };
        labels.push(panel);
        let mut bar = ui.new_child(
            egui::UiBuilder::new()
                .id_salt(("dimension-field", &element.id))
                .max_rect(panel),
        );
        let label = format!(
            "{}  {}",
            field.label(),
            w.session.visible().settings.format_length(value)
        );
        let chip = controls::button(
            &mut bar,
            &format!("{}:label", element.id),
            "dimension-value-button",
            &["viewport-dimension", "viewport-overlay"],
            &label,
            true,
            false,
        )
        .on_hover_text("Enter a length or expression");
        if chip.clicked() || response.clicked() {
            let size = egui::vec2(185., 78.).min(r.size());
            let location = egui::pos2(
                panel.left().min((r.right() - size.x).max(r.left())),
                panel.top().min((r.bottom() - size.y).max(r.top())),
            );
            ui.data_mut(|d| d.insert_temp(input_id, egui::Rect::from_min_size(location, size)));
            w.fields
                .insert(format!("focus-request:{}", element.id), "true".into());
            ui.ctx().request_repaint();
        }
        captured |= chip.hovered() || chip.clicked() || chip.is_pointer_button_down_on();
    }
    captured
}
