//! Semantic modeling editors. Every native control is a view of the same typed
//! field/action descriptor consumed by user layouts; no widget owns a feature.
use crate::{Workspace, presentation_controls as controls};
use bevy_egui::egui;
use cad_core::{
    CadError, Feature, NodeRef, Operation, Result,
    presentation::SemanticElement,
    reference_geometry::{AxisSource, OriginAxis, OriginPlane, PlaneSource, PointSource},
};
use serde_json::{Value, json};

const CONTRACT: &str = "tcad.presentation.operation-editor";
const VERSION: u32 = 1;
const ACTION: &str = "operation-editor.action";
mod common;

/// Disposable value projection. A displayed field must not regenerate all of
/// the editor's topology/provenance choices just to read its current value.
#[derive(Default)]
pub struct ValueCache {
    key: Option<Value>,
    source: Value,
}

pub fn supports(operation: &Operation) -> bool {
    common::supports(operation)
        || matches!(
            operation,
            Operation::Surface { .. }
                | Operation::DirectEdit { .. }
                | Operation::Sheet { .. }
                | Operation::Weldment { .. }
                | Operation::WeldBead { .. }
                | Operation::Mold { .. }
        )
}

fn current(w: &Workspace) -> Option<(&str, &Feature)> {
    if crate::command_palette::context_editor(w) != Some("feature") || w.tool_checkpoint.is_none() {
        return None;
    }
    let owner = w.view.active_part.as_deref()?;
    let id = w.fields.get("feature-editor-id")?;
    let feature = w
        .session
        .visible()
        .parts
        .get(owner)?
        .features
        .iter()
        .find(|f| &f.id == id)?;
    supports(&feature.operation).then_some((owner, feature))
}

struct Fields<'a> {
    w: &'a Workspace,
    owner: &'a str,
    feature: &'a Feature,
    source: Value,
    out: Vec<SemanticElement>,
    group: &'static str,
    revision: Value,
}
impl<'a> Fields<'a> {
    fn new(w: &'a Workspace, owner: &'a str, feature: &'a Feature) -> Self {
        Self {
            w,
            owner,
            feature,
            source: editable_source(feature),
            out: vec![],
            group: "definition",
            revision: json!([
                w.session.generation,
                w.session.committed.revision,
                w.session.draft.as_ref().map(|d| (&d.id, d.edit))
            ]),
        }
    }
    fn identity(&self, key: &str) -> String {
        format!("operation-editor:{}/{key}", self.feature.id)
    }
    fn metadata(&self) -> Value {
        json!({"contract": CONTRACT, "version": VERSION,
            "document":self.w.session.visible().id,"owner":self.owner,"feature":self.feature.id,
            "editor":self.w.fields.get("feature-editor-generation"),"group":self.group,"source_revision":self.revision})
    }
    fn enabled(&self) -> bool {
        !self.w.fields.contains_key("feature-source-invalid") && self.w.settings.is_none()
    }
    fn group(&mut self, group: &'static str, label: &str) {
        self.group = group;
        let mut data = self.metadata();
        data["classes"] = json!(["modeling-group", "inspector-section", group]);
        self.out.push(SemanticElement {
            id: self.identity(&format!("group/{group}")),
            role: "operation-editor-group".into(),
            label: label.into(),
            value: None,
            state: "draft".into(),
            commands: vec![],
            relationships: vec![self.feature.id.clone()],
            data,
        });
    }
    fn field(&mut self, path: &str, label: &str, choices: Vec<Value>, unit: Option<&str>) {
        let Some(value) = self.source.pointer(path).cloned() else {
            return;
        };
        let mut data = self.metadata();
        data["setter"] = json!("operation-editor");
        data["pointer"] = json!(path);
        data["json"] = value.clone();
        data["editable"] = json!(self.enabled());
        data["choices"] = json!(choices);
        data["unit"] = json!(unit);
        data["accepts_quantity_text"] =
            json!(value.is_number() && unit.is_some_and(|u| !u.is_empty()));
        data["value_type"] = json!(if !choices.is_empty() {
            if value.is_object() {
                "reference"
            } else {
                "choice"
            }
        } else if value.is_boolean() {
            "boolean"
        } else if value.is_number() {
            "number"
        } else if unit.is_some() {
            "quantity"
        } else {
            "text"
        });
        data["dimension"] = json!(match unit {
            Some("mm") => "length",
            Some("deg") => "angle",
            _ => "scalar",
        });
        data["accessibility_label"] = json!(label);
        data["classes"] = json!(["modeling-field", self.group]);
        let id = self.identity(path.trim_start_matches('/'));
        self.out.push(SemanticElement {
            id,
            role: "operation-editor-field".into(),
            label: label.into(),
            value: Some(
                value
                    .as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| value.to_string()),
            ),
            state: if self.enabled() { "draft" } else { "disabled" }.into(),
            commands: vec![],
            relationships: vec![self.feature.id.clone()],
            data,
        });
    }
    fn quantity(&mut self, path: &str, label: &str, unit: &str) {
        self.field(path, label, vec![], Some(unit));
    }
    fn toggle(&mut self, path: &str, label: &str) {
        self.field(path, label, vec![], None);
    }
    fn choice(&mut self, path: &str, label: &str, mut choices: Vec<Value>) {
        if let Some(current) = self.source.pointer(path)
            && let Some(kind) = current.get("kind").and_then(Value::as_str)
            && matches!(
                kind,
                "coordinates" | "explicit" | "axis" | "plane_normal" | "two_points"
            )
            && let Some(choice) = choices.iter_mut().find(|c| {
                c.get("value")
                    .and_then(|v| v.get("kind"))
                    .and_then(Value::as_str)
                    == Some(kind)
            })
        {
            choice["value"] = current.clone();
        }
        if let Some(current) = self.source.pointer(path)
            && !choices.iter().any(|c| c.get("value") == Some(current))
        {
            let label = current
                .get("kind")
                .and_then(Value::as_str)
                .and_then(|kind| {
                    choices
                        .iter()
                        .find(|c| {
                            c.get("value")
                                .and_then(|v| v.get("kind"))
                                .and_then(Value::as_str)
                                == Some(kind)
                        })
                        .and_then(|c| c.get("label"))
                        .and_then(Value::as_str)
                })
                .unwrap_or("Saved reference / option");
            choices.push(json!({"value":current,"label":label}));
        }
        self.field(path, label, choices, None);
    }
    fn coordinates(&mut self, path: &str, label: &str, unit: &str) {
        for (i, axis) in ["X", "Y", "Z"].iter().enumerate() {
            self.quantity(&format!("{path}/{i}"), &format!("{label} {axis}"), unit);
        }
    }
    fn action(&mut self, key: &str, label: &str, mutation: Value, enabled: bool, reason: &str) {
        let id = self.identity(&format!("action/{key}"));
        let available = self.enabled() && enabled;
        let mut data = self.metadata();
        data["element"] = json!(id);
        data["mutation"] = mutation;
        data["actions"] = json!([{"command":ACTION,"label":label,"enabled":available,"disabled_reason":(!available).then_some(reason),"arguments":data.clone()}]);
        data["classes"] = json!(["modeling-action", self.group]);
        self.out.push(SemanticElement {
            id,
            role: "operation-editor-action".into(),
            label: label.into(),
            value: (!available).then(|| reason.into()),
            state: if available { "ready" } else { "disabled" }.into(),
            commands: vec![ACTION.into()],
            relationships: vec![self.feature.id.clone()],
            data,
        });
    }
    fn note(&mut self, key: &str, label: &str, state: &str) {
        self.out.push(SemanticElement {
            id: self.identity(&format!("note/{key}")),
            role: "operation-editor-help".into(),
            label: label.into(),
            value: None,
            state: state.into(),
            commands: vec![],
            relationships: vec![self.feature.id.clone()],
            data: self.metadata(),
        });
    }
    fn optional_expression(&mut self, path: &str, label: &str, default: &str, unit: &str) {
        let enabled = self.source.pointer(path).is_some_and(|v| !v.is_null());
        let id = self.identity(&format!("{}/override", path.trim_start_matches('/')));
        let mut data = self.metadata();
        data["setter"] = json!("operation-editor");
        data["pointer"] = json!(path);
        data["optional_default"] = json!(default);
        data["json"] = json!(enabled);
        data["editable"] = json!(self.enabled());
        data["classes"] = json!(["modeling-field", self.group]);
        data["accessibility_label"] = json!(format!("Override {label}; otherwise inherit stock"));
        self.out.push(SemanticElement {
            id,
            role: "operation-editor-field".into(),
            label: format!("Override {label}"),
            value: Some(enabled.to_string()),
            state: if self.enabled() { "draft" } else { "disabled" }.into(),
            commands: vec![],
            relationships: vec![self.feature.id.clone()],
            data,
        });
        if enabled {
            self.quantity(path, label, unit);
        }
    }
    fn reference(&mut self, path: &str, label: &str, kind: &str, optional: bool) {
        let mut choices = node_choices(self.w, self.owner, &self.feature.id, kind);
        if optional {
            choices.insert(0, json!({"value":null,"label":"New body / group"}));
        }
        self.choice(path, label, choices);
    }
    fn plane(&mut self, path: &str, label: &str) {
        self.choice(
            path,
            label,
            plane_choices(self.w, self.owner, &self.feature.id),
        );
    }
    fn axis(&mut self, path: &str, label: &str, optional: bool) {
        let mut choices = axis_choices(self.w, self.owner, &self.feature.id);
        if optional {
            choices.insert(0, json!({"value":null,"label":"Explicit axis"}));
        }
        self.choice(path, label, choices);
    }
    fn point(&mut self, path: &str, label: &str) {
        self.choice(
            path,
            label,
            point_choices(self.w, self.owner, &self.feature.id),
        );
        match self
            .source
            .pointer(path)
            .and_then(|v| v.get("kind"))
            .and_then(Value::as_str)
        {
            Some("coordinates") => {
                self.coordinates(&format!("{path}/coordinates"), "Position", "mm")
            }
            Some("edge_point") => self.quantity(
                &format!("{path}/source/parameter"),
                "Edge parameter (0–1)",
                "",
            ),
            Some("plane_origin") => self.plane(&format!("{path}/plane"), "Origin plane"),
            Some("axis_origin") => self.axis(&format!("{path}/axis"), "Origin axis", false),
            _ => {}
        }
    }
    fn direction(&mut self, path: &str, label: &str) {
        let kind = self
            .source
            .pointer(path)
            .and_then(|v| v.get("kind"))
            .and_then(Value::as_str)
            .unwrap_or("");
        let choices = vec![
            json!({"label":"Explicit components","value":{"kind":"explicit","direction":["0","0","1"]}}),
            json!({"label":"Axis","value":{"kind":"axis","axis":{"kind":"origin","axis":"z"}}}),
            json!({"label":"Plane normal","value":{"kind":"plane_normal","plane":{"kind":"origin","plane":"top"}}}),
            json!({"label":"Two points","value":{"kind":"two_points","points":[{"kind":"coordinates","coordinates":["0 mm","0 mm","0 mm"]},{"kind":"coordinates","coordinates":["0 mm","0 mm","10 mm"]}]}}),
        ];
        let kind = kind.to_string();
        self.choice(path, label, choices);
        match kind.as_str() {
            "explicit" => self.coordinates(&format!("{path}/direction"), "Component", ""),
            "axis" => self.axis(&format!("{path}/axis"), "Direction axis", false),
            "plane_normal" => self.plane(&format!("{path}/plane"), "Normal to plane"),
            "two_points" => {
                self.point(&format!("{path}/points/0"), "From point");
                self.point(&format!("{path}/points/1"), "To point");
            }
            _ => {}
        }
    }
    fn array_actions(&mut self, path: &str, index: usize, key: &str, order: bool) {
        let n = self
            .source
            .pointer(path)
            .and_then(Value::as_array)
            .map_or(0, Vec::len);
        self.action(
            &format!("{key}/remove"),
            "Remove",
            json!({"kind":"remove","pointer":path,"index":index}),
            true,
            "Finish the source edit first",
        );
        if order {
            self.action(&format!("{key}/up"),"Move up",json!({"kind":"swap","pointer":path,"first":index,"second":index.saturating_sub(1)}),index>0,"Already first");
            self.action(
                &format!("{key}/down"),
                "Move down",
                json!({"kind":"swap","pointer":path,"first":index,"second":index+1}),
                index + 1 < n,
                "Already last",
            );
        }
    }
    fn node_array(&mut self, path: &str, label: &str, kind: &str) {
        let entries = self
            .source
            .pointer(path)
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        for (index, value) in entries.iter().enumerate() {
            let occurrence = entries[..index]
                .iter()
                .filter(|prior| *prior == value)
                .count();
            let key = format!(
                "{path}/{}/occurrence-{occurrence}",
                item_identity(value, index)
            );
            self.reference(
                &format!("{path}/{index}"),
                &format!("{label} {}", index + 1),
                kind,
                false,
            );
            let id = self.identity(&format!("{key}/source"));
            if let Some(field) = self.out.last_mut() {
                field.id = id;
            }
            self.array_actions(path, index, &key, true);
        }
        let available = node_choices(self.w, self.owner, &self.feature.id, kind);
        let next = available
            .iter()
            .find_map(|c| c.get("value").filter(|v| !entries.contains(v)))
            .cloned();
        self.action(
            &format!("{path}/add"),
            &format!("Add {label}"),
            json!({"kind":"append","pointer":path,"value":next}),
            next.is_some(),
            "No additional compatible source is available",
        );
    }
    fn topology_array(&mut self, path: &str, body: &NodeRef, topology: &str) {
        let entries = self
            .source
            .pointer(path)
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        for (index, value) in entries.iter().enumerate() {
            let key = format!("{path}/{}", item_identity(value, index));
            self.note(
                &key,
                &format!(
                    "{} {}",
                    if topology == "edge" { "Edge" } else { "Face" },
                    index + 1
                ),
                "ready",
            );
            self.array_actions(path, index, &key, false);
        }
        let picked = self
            .w
            .selected
            .as_ref()
            .filter(|s| {
                s.reference.topology == topology
                    && crate::reference_ui::selected_body(self.w, s) == *body
                    && crate::selection_ui::edge_occurrence(self.w, &s.path)
            })
            .map(|s| json!(s.reference));
        let next = picked.filter(|v| !entries.contains(v));
        self.action(
            &format!("{path}/add"),
            &format!("Add selected {topology}"),
            json!({"kind":"append","pointer":path,"value":next}),
            next.is_some(),
            "Select geometry on the input body that is not already listed",
        );
        self.action(
            &format!("{path}/clear"),
            "Clear selection",
            json!({"kind":"set","pointer":path,"value":[]}),
            !entries.is_empty(),
            "The selection is empty",
        );
    }
}

fn item_identity(value: &Value, index: usize) -> String {
    if let Some(id) = value.get("id").and_then(Value::as_str) {
        return format!("@{id}");
    }
    if let (Some(def), Some(node)) = (
        value.get("definition").and_then(Value::as_str),
        value.get("node").and_then(Value::as_str),
    ) {
        return format!("@{def}:{node}");
    }
    if let Some(member) = value.get("member").and_then(Value::as_str) {
        return format!("@{}:{member}", value.get("group").unwrap_or(&Value::Null));
    }
    if let (Some(producer), Some(role)) = (value.get("producer"), value.get("role")) {
        return format!("@{producer}:{role}");
    }
    format!("position-{index}")
}
fn choice<T: serde::Serialize>(label: impl Into<String>, value: T) -> Value {
    json!({"label":label.into(),"value":value})
}
fn enum_choices(values: &[(&str, &str)]) -> Vec<Value> {
    values
        .iter()
        .map(|(value, label)| choice(*label, *value))
        .collect()
}
fn node_choices(w: &Workspace, owner: &str, current: &str, kind: &str) -> Vec<Value> {
    w.session
        .visible()
        .parts
        .values()
        .filter(|part| kind != "solid-local" || part.id == owner)
        .flat_map(|p| {
            p.features
                .iter()
                .take_while(|f| f.id != current)
                .filter(move |f| {
                    !cad_core::configuration::feature_suppressed(w.session.visible(), &p.id, f)
                })
                .filter(move |f| match kind {
                    "profile" => matches!(f.operation, Operation::Profile { .. }),
                    "curve-path" => matches!(
                        f.operation,
                        Operation::Profile { .. }
                            | Operation::Sketch3d { .. }
                            | Operation::ReferenceCurve { .. }
                    ),
                    "surface" => f.operation.is_surface(),
                    "solid" | "solid-local" => f.operation.produces_solid(),
                    "sheet" => matches!(f.operation, Operation::Sheet { .. }),
                    "weldment" => matches!(f.operation, Operation::Weldment { .. }),
                    _ => false,
                })
                .map(move |f| {
                    choice(
                        if p.id == owner {
                            f.name.clone()
                        } else {
                            format!("{} / {}", p.name, f.name)
                        },
                        p.reference(&f.id),
                    )
                })
        })
        .collect()
}
fn preceding<'a>(
    w: &'a Workspace,
    owner: &'a str,
    current: &'a str,
) -> impl Iterator<Item = &'a Feature> {
    w.session
        .visible()
        .parts
        .get(owner)
        .into_iter()
        .flat_map(move |p| {
            p.features
                .iter()
                .take_while(move |f| f.id != current)
                .filter(move |f| {
                    !cad_core::configuration::feature_suppressed(w.session.visible(), owner, f)
                })
        })
}
fn plane_choices(w: &Workspace, owner: &str, current: &str) -> Vec<Value> {
    let mut choices = [OriginPlane::Front, OriginPlane::Top, OriginPlane::Right]
        .into_iter()
        .map(|p| choice(p.label(), PlaneSource::Origin { plane: p }))
        .collect::<Vec<_>>();
    for f in preceding(w, owner, current) {
        let feature = NodeRef {
            definition: owner.into(),
            node: f.id.clone(),
        };
        match f.operation {
            Operation::ReferencePlane { .. } => {
                choices.push(choice(&f.name, PlaneSource::Datum { feature }))
            }
            Operation::ReferenceCoordinateSystem { .. } => {
                for plane in [OriginPlane::Front, OriginPlane::Top, OriginPlane::Right] {
                    choices.push(choice(
                        format!("{} · {}", f.name, plane.label()),
                        PlaneSource::CoordinateSystem {
                            feature: feature.clone(),
                            plane,
                        },
                    ));
                }
            }
            _ => {}
        }
    }
    if let Some(s) = w.selected.as_ref().filter(|s| {
        s.definition == owner && crate::reference_ui::selected_body(w, s).node != current
    }) {
        if s.reference.topology == "face" {
            choices.push(choice(
                "Selected planar face",
                PlaneSource::Face {
                    reference: s.reference.clone(),
                    body: crate::reference_ui::selected_body(w, s),
                },
            ));
        } else if let Ok(source) = PlaneSource::from_datum(&s.reference) {
            choices.push(choice("Selected plane", source));
        }
    }
    choices
}
fn axis_choices(w: &Workspace, owner: &str, current: &str) -> Vec<Value> {
    let mut choices = [OriginAxis::X, OriginAxis::Y, OriginAxis::Z]
        .into_iter()
        .map(|a| choice(a.label(), AxisSource::Origin { axis: a }))
        .collect::<Vec<_>>();
    for f in preceding(w, owner, current) {
        let feature = NodeRef {
            definition: owner.into(),
            node: f.id.clone(),
        };
        match &f.operation {
            Operation::ReferenceAxis { .. } => {
                choices.push(choice(&f.name, AxisSource::Datum { feature }))
            }
            Operation::ReferenceCoordinateSystem { .. } => {
                for axis in [OriginAxis::X, OriginAxis::Y, OriginAxis::Z] {
                    choices.push(choice(
                        format!("{} · {}", f.name, axis.label()),
                        AxisSource::CoordinateSystem {
                            feature: feature.clone(),
                            axis,
                        },
                    ));
                }
            }
            Operation::Sketch3d { definition } => {
                for (index, (id, member)) in definition.members.iter().enumerate() {
                    let source = match member.curve {
                        cad_core::sketch3d::Curve::Line { .. } => Some(AxisSource::SketchLine {
                            profile: feature.clone(),
                            member: id.clone(),
                        }),
                        cad_core::sketch3d::Curve::Circle { .. }
                        | cad_core::sketch3d::Curve::Arc { .. } => Some(AxisSource::SketchCircle {
                            profile: feature.clone(),
                            member: id.clone(),
                        }),
                        _ => None,
                    };
                    if let Some(source) = source {
                        choices.push(choice(
                            format!("{} · 3D member {}", f.name, index + 1),
                            source,
                        ));
                    }
                }
            }
            Operation::Profile {
                profile: cad_core::Profile::Sketch { sketch },
                ..
            } => {
                if let Some(sketch) = w.session.visible().parts[owner].sketches.get(sketch) {
                    for (index, (id, member)) in sketch.members.iter().enumerate() {
                        match member.curve {
                            cad_core::SketchCurve::Line { .. } => choices.push(choice(
                                format!("{} · Line {}", f.name, index + 1),
                                AxisSource::SketchLine {
                                    profile: feature.clone(),
                                    member: id.clone(),
                                },
                            )),
                            cad_core::SketchCurve::Circle { .. }
                            | cad_core::SketchCurve::Arc { .. } => choices.push(choice(
                                format!("{} · Circle/arc {}", f.name, index + 1),
                                AxisSource::SketchCircle {
                                    profile: feature.clone(),
                                    member: id.clone(),
                                },
                            )),
                            _ => {}
                        }
                    }
                    for (index, id) in sketch.circles.keys().enumerate() {
                        choices.push(choice(
                            format!("{} · Circle {}", f.name, index + 1),
                            AxisSource::SketchCircle {
                                profile: feature.clone(),
                                member: id.clone(),
                            },
                        ));
                    }
                }
            }
            _ => {}
        }
    }
    if let Some(s) = w.selected.as_ref().filter(|s| {
        s.definition == owner && crate::reference_ui::selected_body(w, s).node != current
    }) {
        match s.reference.topology.as_str() {
            "edge" | "face" => choices.push(choice(
                "Selected edge / rotational face",
                crate::reference_ui::picked_axis(w, s),
            )),
            "axis" => {
                if let Ok(source) = AxisSource::from_datum(&s.reference) {
                    choices.push(choice("Selected axis", source));
                }
            }
            _ => {}
        }
    }
    choices
}
fn point_choices(w: &Workspace, owner: &str, current: &str) -> Vec<Value> {
    let mut choices = vec![choice("Coordinates", PointSource::default())];
    for f in preceding(w, owner, current) {
        let feature = NodeRef {
            definition: owner.into(),
            node: f.id.clone(),
        };
        match &f.operation {
            Operation::ReferencePoint { .. } => {
                choices.push(choice(&f.name, PointSource::Datum { feature }))
            }
            Operation::ReferenceCoordinateSystem { .. } => choices.push(choice(
                format!("{} · Origin", f.name),
                PointSource::CoordinateSystemOrigin { feature },
            )),
            Operation::ReferencePlane { .. } => choices.push(choice(
                format!("{} · Origin", f.name),
                PointSource::PlaneOrigin {
                    plane: PlaneSource::Datum { feature },
                },
            )),
            Operation::ReferenceAxis { .. } => choices.push(choice(
                format!("{} · Origin", f.name),
                PointSource::AxisOrigin {
                    axis: AxisSource::Datum { feature },
                },
            )),
            Operation::Sketch3d { definition } => {
                for (index, id) in definition.points.keys().enumerate() {
                    choices.push(choice(
                        format!("{} · 3D point {}", f.name, index + 1),
                        PointSource::SketchPoint {
                            profile: feature.clone(),
                            point: id.clone(),
                        },
                    ));
                }
            }
            Operation::Profile {
                profile: cad_core::Profile::Sketch { sketch },
                ..
            } => {
                if let Some(sketch) = w.session.visible().parts[owner].sketches.get(sketch) {
                    for (index, id) in sketch.points.keys().enumerate() {
                        choices.push(choice(
                            format!("{} · Point {}", f.name, index + 1),
                            PointSource::SketchPoint {
                                profile: feature.clone(),
                                point: id.clone(),
                            },
                        ));
                    }
                }
            }
            _ => {}
        }
    }
    for plane in [OriginPlane::Front, OriginPlane::Top, OriginPlane::Right] {
        choices.push(choice(
            format!("{} · Origin", plane.label()),
            PointSource::PlaneOrigin {
                plane: PlaneSource::Origin { plane },
            },
        ));
    }
    if let Some(s) = w.selected.as_ref().filter(|s| {
        s.definition == owner
            && s.reference.topology == "edge"
            && crate::reference_ui::selected_body(w, s).node != current
    }) {
        choices.push(choice(
            "Selected edge · native parameter",
            PointSource::EdgePoint {
                source: Box::new(cad_core::reference_geometry::EdgePointSource {
                    body: crate::reference_ui::selected_body(w, s),
                    reference: s.reference.clone(),
                    parameter: "0.5".into(),
                }),
            },
        ));
    }
    choices
}

fn surface_fields(fields: &mut Fields<'_>, operation: &cad_core::surface::SurfaceOperation) {
    use cad_core::surface::SurfaceOperation as S;
    const P: &str = "/operation/operation";
    fields.group("sources", "Sources");
    match operation {
        S::Planar { .. } => {
            fields.reference(&format!("{P}/profile"), "Boundary sketch", "profile", false)
        }
        S::Ruled { sources, .. } => {
            let available = crate::reference_curves_ui::sources(
                fields.w.session.visible(),
                fields.owner,
                &fields.feature.id,
            )
            .into_iter()
            .map(|(label, value)| choice(label, value))
            .collect::<Vec<_>>();
            for (index, source) in sources.iter().enumerate() {
                let path = format!("{P}/sources/{index}");
                fields.choice(
                    &format!("{path}/source"),
                    &format!("Curve {}", index + 1),
                    available.clone(),
                );
                let id = fields.identity(&format!("curve/{}/source", source.id));
                if let Some(field) = fields.out.last_mut() {
                    field.id = id;
                }
                fields.toggle(&format!("{path}/reverse"), "Reverse curve");
                let id = fields.identity(&format!("curve/{}/reverse", source.id));
                if let Some(field) = fields.out.last_mut() {
                    field.id = id;
                }
                fields.array_actions(
                    &format!("{P}/sources"),
                    index,
                    &format!("curve/{}", source.id),
                    true,
                );
            }
            let next = crate::reference_curves_ui::selected_edge(fields.w, &fields.feature.id)
                .map(|source| json!(source))
                .or_else(|| available.first().and_then(|c| c.get("value")).cloned());
            fields.action(
                "curve/add",
                "Add curve",
                json!({"kind":"append_segment","pointer":format!("{P}/sources"),"source":next}),
                next.is_some() && sources.len() < 256,
                "Select an edge or create a source curve; maximum 256 sources",
            );
            fields.group("direction", "Direction");
            fields.direction(&format!("{P}/direction"), "Extension direction");
            fields.quantity(&format!("{P}/distance"), "Extension", "mm");
            fields.toggle(&format!("{P}/reverse"), "Reverse face orientation");
        }
        S::MidSurface { .. } => {
            let choices = crate::surface_ui::face_choices(fields.w, &fields.feature.id)
                .into_iter()
                .map(|(label, value)| choice(label, value))
                .collect::<Vec<_>>();
            fields.choice(&format!("{P}/faces/0"), "First face", choices.clone());
            fields.choice(&format!("{P}/faces/1"), "Second face", choices);
            fields.group("correspondence", "Correspondence");
            fields.quantity(&format!("{P}/fraction"), "Position between faces (0–1)", "");
            fields.toggle(&format!("{P}/correspondence/swap_uv"), "Swap U and V");
            fields.toggle(&format!("{P}/correspondence/reverse_u"), "Reverse U");
            fields.toggle(&format!("{P}/correspondence/reverse_v"), "Reverse V");
            fields.toggle(&format!("{P}/reverse"), "Reverse face orientation");
        }
        S::Extrude { profile_normal, .. } => {
            fields.reference(&format!("{P}/profile"), "Section sketch", "profile", false);
            fields.group("extent", "Extent");
            fields.quantity(&format!("{P}/distance"), "Depth", "mm");
            fields.toggle(&format!("{P}/mid_plane"), "Mid-plane");
            fields.toggle(&format!("{P}/reverse"), "Reverse direction");
            fields.toggle(&format!("{P}/profile_normal"), "Normal to sketch");
            if !profile_normal {
                fields.coordinates(&format!("{P}/direction"), "Direction", "");
            }
        }
        S::Revolve { axis_reference, .. } => {
            fields.reference(&format!("{P}/profile"), "Section sketch", "profile", false);
            fields.group("axis", "Axis and angle");
            fields.axis(&format!("{P}/axis_reference"), "Rotation axis", true);
            if axis_reference.is_none() {
                fields.coordinates(&format!("{P}/origin"), "Axis origin", "mm");
                fields.coordinates(&format!("{P}/axis"), "Axis direction", "");
            }
            fields.quantity(&format!("{P}/angle"), "Angle", "deg");
        }
        S::Loft { .. } => {
            fields.node_array(&format!("{P}/profiles"), "Section", "profile");
            fields.note("order","Sections connect in this order. Move a section up or down to change correspondence.","ready");
        }
        S::Sweep {
            path, path_sketch, ..
        } => {
            fields.reference(&format!("{P}/profile"), "Section sketch", "profile", false);
            let mut choices =
                node_choices(fields.w, fields.owner, &fields.feature.id, "curve-path");
            choices.insert(0, choice("Explicit path points", Value::Null));
            fields.choice(&format!("{P}/path_sketch"), "Path sketch", choices);
            if path_sketch.is_none() {
                fields.group("path", "Path points");
                for (index, _) in path.iter().enumerate() {
                    fields.coordinates(
                        &format!("{P}/path/{index}"),
                        &format!("Point {}", index + 1),
                        "mm",
                    );
                    fields.array_actions(
                        &format!("{P}/path"),
                        index,
                        &format!("path/{index}"),
                        true,
                    );
                }
                let value = path
                    .last()
                    .map(|p| [p[0], p[1], p[2] + 10.])
                    .unwrap_or([0., 0., 0.]);
                fields.action(
                    "path/add",
                    "Add path point",
                    json!({"kind":"append","pointer":format!("{P}/path"),"value":value}),
                    path.len() < 4096,
                    "Maximum 4096 path points",
                );
            }
        }
        S::Fill { .. } => {
            for i in 0..4 {
                fields.reference(
                    &format!("{P}/boundaries/{i}"),
                    &format!("Boundary {}", i + 1),
                    "profile",
                    false,
                );
            }
            fields.toggle(&format!("{P}/reverse"), "Reverse face orientation");
            fields.note(
                "order",
                "Choose four end-connected boundaries in perimeter order.",
                "ready",
            );
        }
        S::Trim { .. } => {
            fields.reference(&format!("{P}/surface"), "Surface", "surface", false);
            fields.reference(&format!("{P}/profile"), "Trim sketch", "profile", false);
            fields.toggle(&format!("{P}/keep_inside"), "Keep inside boundary");
        }
        S::TrimDomain { .. } => {
            fields.reference(&format!("{P}/surface"), "Surface", "surface", false);
            fields.group("domain", "Parameter bounds");
            for (path, label) in [
                ("u/0", "U start"),
                ("u/1", "U end"),
                ("v/0", "V start"),
                ("v/1", "V end"),
            ] {
                fields.quantity(&format!("{P}/{path}"), label, "");
            }
            fields.note(
                "normalized",
                "U and V use the normalized native domain from 0 to 1.",
                "ready",
            );
        }
        S::Untrim { .. } | S::Reverse { .. } => {
            fields.reference(&format!("{P}/surface"), "Surface", "surface", false)
        }
        S::Extend { surface, .. } => {
            fields.reference(&format!("{P}/surface"), "Surface", "surface", false);
            fields.group("selection", "Boundary edges");
            fields.topology_array(&format!("{P}/edges"), surface, "edge");
            fields.note(
                "all-edges",
                "An empty edge list extends every boundary edge.",
                "ready",
            );
            fields.group("extent", "Extent");
            fields.quantity(&format!("{P}/distance"), "Extension", "mm");
        }
        S::Offset { .. } => {
            fields.reference(&format!("{P}/surface"), "Surface", "surface", false);
            fields.quantity(&format!("{P}/distance"), "Normal offset", "mm");
        }
        S::Knit { .. } => {
            fields.node_array(&format!("{P}/surfaces"), "Surface", "surface");
            fields.group("result", "Result");
            fields.quantity(&format!("{P}/tolerance"), "Knit tolerance", "mm");
            fields.toggle(&format!("{P}/make_solid"), "Form solid");
            fields.toggle(&format!("{P}/reverse"), "Reverse face orientation");
        }
        S::Thicken { .. } => {
            fields.reference(&format!("{P}/surface"), "Surface", "surface", false);
            fields.group("extent", "Extent");
            fields.quantity(&format!("{P}/thickness"), "Thickness", "mm");
            fields.toggle(&format!("{P}/mid_plane"), "Both sides equally");
            fields.toggle(&format!("{P}/reverse"), "Reverse direction");
        }
    }
}
fn direct_fields(fields: &mut Fields<'_>, definition: &cad_core::direct_edit::Definition) {
    use cad_core::direct_edit::Method;
    const P: &str = "/operation/definition";
    fields.group("sources", "Source body");
    fields.reference(&format!("{P}/body"), "Body", "solid-local", false);
    fields.group("selection", "Faces");
    fields.topology_array(&format!("{P}/faces"), &definition.body, "face");
    fields.group("method", "Face treatment");
    match &definition.method {
        Method::Offset { .. } => fields.quantity(
            &format!("{P}/method/distance"),
            "Normal offset (+ outward)",
            "mm",
        ),
        Method::Move { .. } => {
            fields.coordinates(&format!("{P}/method/translation"), "Translation", "mm");
            fields.axis(&format!("{P}/method/axis"), "Rotation axis", false);
            fields.quantity(&format!("{P}/method/angle"), "Rotation", "deg");
        }
        Method::DeleteHeal => fields.note(
            "heal",
            "Extend the remaining supporting faces to heal the selected openings.",
            "ready",
        ),
        Method::Replace { .. } => {
            fields.plane(&format!("{P}/method/plane"), "Replacement plane");
            fields.toggle(&format!("{P}/method/reverse"), "Reverse material side");
        }
    }
}
fn sheet_attachment_fields(fields: &mut Fields<'_>, body: &NodeRef, attachment: &Option<NodeRef>) {
    const P: &str = "/operation/operation";
    fields.reference(&format!("{P}/body"), "Sheet stock", "sheet", false);
    if let Ok(stock) = cad_core::sheet_metal::stock(fields.w.session.visible(), body) {
        fields.group("attachment", "Bend attachment");
        let choices = crate::fabrication_ui::sheet_attachment_choices(fields.w, &stock)
            .into_iter()
            .map(|(value, label)| choice(label, value))
            .collect();
        fields.choice(&format!("{P}/attachment"), "Attach bend to", choices);
        if attachment.is_none() {
            let choices = crate::fabrication_ui::sheet_base_members(fields.w, &stock)
                .into_iter()
                .map(|(value, label)| choice(label, value))
                .collect();
            fields.choice(&format!("{P}/member"), "Base sketch edge", choices);
        } else {
            fields.note(
                "terminal-association",
                "Full-width terminal edge; the source base member follows the selected attachment.",
                "ready",
            );
        }
        fields
            .out
            .extend(crate::fabrication_sources_ui::sheet_controls(fields.w));
    }
}
fn sheet_fields(fields: &mut Fields<'_>, operation: &cad_core::sheet_metal::SheetOperation) {
    use cad_core::sheet_metal::SheetOperation as S;
    const P: &str = "/operation/operation";
    fields.group("sources", "Sources");
    match operation {
        S::Base { .. } => {
            fields.reference(&format!("{P}/profile"), "Base sketch", "profile", false);
            fields.group("stock", "Sheet stock");
            fields.quantity(&format!("{P}/thickness"), "Thickness", "mm");
            fields.quantity(&format!("{P}/inside_radius"), "Default inside radius", "mm");
            fields.quantity(&format!("{P}/k_factor"), "K factor (0–1)", "");
            thickness_side(fields, &format!("{P}/side"));
        }
        S::Tab { .. } | S::NormalCut { .. } => {
            fields.reference(&format!("{P}/body"), "Sheet stock", "sheet", false);
            fields.reference(
                &format!("{P}/profile"),
                if matches!(operation, S::NormalCut { .. }) {
                    "Parallel cut sketch / stock face"
                } else {
                    "Base-plane closed sketch"
                },
                "profile",
                false,
            );
            fields.note("blank-edit","Tabs are coplanar with the base sketch; normal cuts accept parallel sketches including a stock face. Blank edits propagate to both states and preserve bend attachment sidewalls.","ready");
        }
        S::EdgeFlange {
            body, attachment, ..
        } => {
            sheet_attachment_fields(fields, body, attachment);
            fields.group("bend", "Bend");
            fields.quantity(&format!("{P}/length"), "Straight tangent length", "mm");
            fields.quantity(&format!("{P}/angle"), "Signed bend angle (±180°)", "deg");
            fields.optional_expression(
                &format!("{P}/inside_radius"),
                "inside radius",
                "1 mm",
                "mm",
            );
            fields.optional_expression(&format!("{P}/k_factor"), "K factor", "0.4", "");
            fields.note("flange-scope","Exact full-width circular bend; tangent length starts after the bend. Chained terminal attachments preserve source ancestry.","ready");
        }
        S::Hem {
            body, attachment, ..
        } => {
            sheet_attachment_fields(fields, body, attachment);
            fields.group("hem", "Open hem");
            fields.quantity(&format!("{P}/length"), "Return tangent length", "mm");
            fields.quantity(&format!("{P}/gap"), "Clear hem gap", "mm");
            fields.toggle(&format!("{P}/reverse"), "Reverse bend side");
            fields.optional_expression(&format!("{P}/k_factor"), "K factor", "0.4", "");
            fields.note("hem-scope","Open 180° return: radius is half the clear gap. Closed crushed hems require forming support.","ready");
        }
        S::Jog {
            body, attachment, ..
        } => {
            sheet_attachment_fields(fields, body, attachment);
            fields.group("jog", "Two-bend jog");
            fields.quantity(&format!("{P}/offset"), "Signed mid-surface offset", "mm");
            fields.quantity(&format!("{P}/length"), "Terminal tangent length", "mm");
            fields.quantity(&format!("{P}/angle"), "Bend magnitude (0–90°)", "deg");
            fields.optional_expression(
                &format!("{P}/inside_radius"),
                "inside radius",
                "1 mm",
                "mm",
            );
            fields.optional_expression(&format!("{P}/k_factor"), "K factor", "0.4", "");
            fields.note("jog-scope","Two equal opposite circular bends. Offset controls bend side and derives straight web length from radius and thickness; flat output carries two bend lines.","ready");
        }
        S::Flatten { .. } => {
            fields.reference(&format!("{P}/body"), "Sheet stock", "sheet", false);
            fields.choice(
                &format!("{P}/state"),
                "Output state",
                enum_choices(&[("flat", "Flat pattern"), ("formed", "Formed sheet")]),
            );
        }
    }
}
fn thickness_side(fields: &mut Fields<'_>, path: &str) {
    fields.choice(
        path,
        "Thickness direction",
        enum_choices(&[
            ("positive", "Along sketch normal"),
            ("negative", "Reverse"),
            ("mid_plane", "Mid-plane"),
        ]),
    );
}
fn weldment_fields(fields: &mut Fields<'_>, operation: &cad_core::weldment::WeldmentOperation) {
    use cad_core::weldment::WeldmentOperation as W;
    const P: &str = "/operation/operation";
    fields.group("sources", "Sources");
    match operation {
        W::Members {
            section,
            path,
            members,
            ..
        } => {
            fields.reference(&format!("{P}/body"), "Append to weldment", "weldment", true);
            let sections =
                crate::fabrication_ui::section_choices(fields.w, &fields.feature.id, section)
                    .into_iter()
                    .map(|(source, label)| choice(label, source))
                    .collect();
            fields.choice(&format!("{P}/section"), "Closed section sketch", sections);
            let paths = crate::fabrication_ui::structural_path_choices(
                fields.w,
                &fields.feature.id,
                path,
                section,
            )
            .into_iter()
            .map(|(source, label)| choice(label, source))
            .collect();
            fields.choice(&format!("{P}/path"), "Path sketch", paths);
            fields.group("members", "Path members");
            member_selection(
                fields,
                &format!("{P}/members"),
                crate::fabrication_ui::structural_path_members(fields.w, path)
                    .into_iter()
                    .map(|(value, label)| choice(label, value))
                    .collect(),
                members.is_empty(),
            );
            fields.group("placement", "Section placement");
            let mut points = vec![choice("Sketch origin", Value::Null)];
            if let Some(sketch) = crate::fabrication_ui::source_sketch(fields.w, section) {
                points.extend(
                    sketch
                        .points
                        .iter()
                        .map(|(id, p)| choice(format!("({:.2}, {:.2}) mm", p[0], p[1]), id)),
                );
            }
            fields.choice(&format!("{P}/anchor"), "Section locating point", points);
            fields.quantity(&format!("{P}/roll"), "Section rotation", "deg");
            fields.quantity(&format!("{P}/profile_offset/0"), "Profile offset X", "mm");
            fields.quantity(&format!("{P}/profile_offset/1"), "Profile offset Y", "mm");
            fields.quantity(
                &format!("{P}/start_offset"),
                "Start shortening (+) / extension (−)",
                "mm",
            );
            fields.quantity(
                &format!("{P}/end_offset"),
                "Finish shortening (+) / extension (−)",
                "mm",
            );
            fields.choice(
                &format!("{P}/corner"),
                "Corner treatment",
                enum_choices(&[
                    ("miter", "Miter"),
                    ("butt", "Independent butt"),
                    ("butt_trim", "Trim to primary member"),
                ]),
            );
            fields.quantity(&format!("{P}/joint_gap"), "Joint gap", "mm");
            if let W::Members {
                corner: cad_core::weldment::Corner::ButtTrim,
                ..
            } = operation
            {
                fields.group("priority", "Member priority");
                let all = crate::fabrication_ui::structural_path_members(fields.w, path);
                let selected = if members.is_empty() {
                    all.iter().map(|(id, _)| id.clone()).collect::<Vec<_>>()
                } else {
                    members.clone()
                };
                for (id, label) in all.into_iter().filter(|(id, _)| selected.contains(id)) {
                    let mut ordered = selected.clone();
                    ordered.retain(|member| member != &id);
                    ordered.insert(0, id.clone());
                    fields.action(
                        &format!("priority/{id}"),
                        &format!("Make primary: {label}"),
                        json!({"kind":"set","pointer":format!("{P}/members"),"value":ordered}),
                        selected.first() != Some(&id),
                        "This member already has first priority",
                    );
                }
                fields.note("priority","At each branch the first incident member stays full length; later members trim to its convex outer wall. All uses saved source ID order.","ready");
            }
            fields.note("curved-stock","Circular arcs and closed circles use exact tangent transport and true developed lengths. Angled curved ends and spline stock coping remain unsupported.","ready");
        }
        W::Trim { body, members, .. } => {
            fields.reference(&format!("{P}/body"), "Weldment", "weldment", false);
            fields.group("members", "Members");
            let choices = cad_core::weldment::stock(fields.w.session.visible(), body)
                .ok()
                .map(|stock| {
                    crate::fabrication_ui::member_choices(fields.w, &stock)
                        .into_iter()
                        .map(|(value, label)| choice(label, value))
                        .collect()
                })
                .unwrap_or_default();
            member_selection(fields, &format!("{P}/members"), choices, members.is_empty());
            fields.group("trim", "End trim");
            member_end(fields, &format!("{P}/end"));
            fields.plane(&format!("{P}/plane"), "Trim plane");
            fields.quantity(&format!("{P}/offset"), "Plane offset", "mm");
        }
        W::EndCap { body, .. } => {
            fields.reference(&format!("{P}/body"), "Weldment", "weldment", false);
            let choices = cad_core::weldment::stock(fields.w.session.visible(), body)
                .ok()
                .map(|stock| {
                    crate::fabrication_ui::member_choices(fields.w, &stock)
                        .into_iter()
                        .map(|(value, label)| choice(label, value))
                        .collect()
                })
                .unwrap_or_default();
            fields.choice(&format!("{P}/member"), "Member", choices);
            member_end(fields, &format!("{P}/end"));
            fields.quantity(&format!("{P}/thickness"), "Cap thickness", "mm");
            fields.choice(
                &format!("{P}/contour"),
                "Cap contour",
                enum_choices(&[
                    ("outer", "Outer contour"),
                    ("inner", "Inner contour (inset)"),
                ]),
            );
            fields.quantity(&format!("{P}/offset"), "Outward gap (+) / recess (−)", "mm");
            fields.quantity(
                &format!("{P}/edge_offset"),
                "Contour oversize (+) / clearance (−)",
                "mm",
            );
            fields.note("caps","Inner caps need one hollow contour and nonpositive clearance; recess needs straight stock. Line/arc contour sizing and separate accessory cut-list rows are supported.","ready");
        }
        W::Gusset { .. } => {
            fields.reference(&format!("{P}/body"), "Append to weldment", "weldment", true);
            fields.reference(&format!("{P}/profile"), "Gusset sketch", "profile", false);
            fields.quantity(&format!("{P}/thickness"), "Thickness", "mm");
            thickness_side(fields, &format!("{P}/side"));
        }
    }
}
fn member_end(fields: &mut Fields<'_>, path: &str) {
    fields.choice(
        path,
        "Member end",
        enum_choices(&[("start", "Start"), ("finish", "Finish")]),
    );
}
fn weld_bead_fields(fields: &mut Fields<'_>, definition: &cad_core::weld_beads::Definition) {
    use cad_core::weld_beads::{Pattern, Profile};
    const P: &str = "/operation/definition";
    fields.group("sources", "Weld joint");
    fields.note(
        "joint-requirements",
        crate::weld_beads_ui::joint_help(definition),
        "ready",
    );
    fields.out.extend(crate::weld_beads_ui::feedback(
        fields.w,
        fields.owner,
        &fields.feature.id,
    ));
    fields
        .out
        .extend(crate::weld_beads_ui::pick_actions(fields.w));
    let mut seams = crate::weld_beads_ui::seam_choices(fields.w, fields.owner, &fields.feature.id)
        .into_iter()
        .map(|(source, label)| choice(label, source))
        .collect::<Vec<_>>();
    if let Some(source) = crate::weld_beads_ui::selected_seam(fields.w, true).filter(|source| {
        source.dependency().definition == fields.owner
            && source.dependency().node != fields.feature.id
    }) {
        seams.insert(0, choice("Selected joint edge / seam", source));
    }
    fields.choice(&format!("{P}/seam"), "Seam source", seams);
    fields.note("seam-intent","A model edge follows its native construction ancestry; a sketch/reference curve is an explicitly authored seam.","ready");
    let supports =
        crate::weld_beads_ui::support_choices(fields.w, fields.owner, &fields.feature.id)
            .into_iter()
            .map(|(source, label)| choice(label, source))
            .collect::<Vec<_>>();
    for (index, _) in definition.supports.iter().enumerate() {
        fields.choice(
            &format!("{P}/supports/{index}"),
            &format!("Support face {}", index + 1),
            supports.clone(),
        );
        fields.array_actions(
            &format!("{P}/supports"),
            index,
            &format!("support-{index}"),
            false,
        );
    }
    let selected = crate::weld_beads_ui::selected_supports(fields.w, true)
        .into_iter()
        .find(|face| {
            face.body.definition == fields.owner
                && face.body.node != fields.feature.id
                && !definition.supports.contains(face)
        });
    let maximum = 2;
    fields.action(
        "add-support",
        "Add selected support face",
        json!({"kind":"append","pointer":format!("{P}/supports"),"value":selected}),
        selected.is_some() && definition.supports.len() < maximum,
        "Pick joint support faces; fillets use two, butt beads use one planar face or two coplanar faces",
    );
    fields.action(
        "infer-supports",
        "Infer contact-proven joint faces",
        json!({"kind":"set","pointer":format!("{P}/supports"),"value":[]}),
        !definition.supports.is_empty(),
        "Adjacent faces are already inferred",
    );
    fields.group("profile", "Deposited weld metal");
    fields.choice(
        &format!("{P}/profile"),
        "Weld profile",
        enum_choices(&[("fillet", "Fillet bead"), ("butt", "Butt bead")]),
    );
    fields.quantity(
        &format!("{P}/size"),
        if definition.profile == Profile::Fillet {
            "Leg size"
        } else {
            "Bead width"
        },
        "mm",
    );
    fields.quantity(&format!("{P}/reinforcement"), "Crown rise", "mm");
    fields.choice(
        &format!("{P}/pattern"),
        "Deposition",
        vec![
            choice("Continuous", Pattern::Continuous),
            choice(
                "Intermittent",
                Pattern::Intermittent {
                    length: "20 mm".into(),
                    gap: "20 mm".into(),
                },
            ),
        ],
    );
    if matches!(definition.pattern, Pattern::Intermittent { .. }) {
        fields.quantity(&format!("{P}/pattern/length"), "Stitch length", "mm");
        fields.quantity(&format!("{P}/pattern/gap"), "Clear gap", "mm");
    }
    fields.quantity(&format!("{P}/start_offset"), "Start offset", "mm");
    fields.quantity(&format!("{P}/end_offset"), "End offset", "mm");
    if !matches!(
        definition.seam,
        cad_core::weld_beads::SeamSource::ModelEdge { .. }
    ) && definition.supports.is_empty()
    {
        fields.group("placement", "Authored seam orientation");
        fields.coordinates(&format!("{P}/direction"), "Section direction", "");
        fields.quantity(&format!("{P}/roll"), "Section roll", "deg");
    } else {
        fields.note("joint-orientation", "The native joint faces determine bead orientation. Direction and roll apply only to an authored seam without support faces.", "ready");
        if definition
            .values(fields.w.session.visible(), fields.owner)
            .is_ok_and(|values| values.roll_rad.abs() > 1e-10)
        {
            fields.action(
                "reset-joint-roll",
                "Use joint orientation",
                json!({"kind":"set","pointer":format!("{P}/roll"),"value":"0 deg"}),
                true,
                "",
            );
        }
    }
    fields.group("appearance", "Finish and engineering material");
    let doc = fields.w.session.visible();
    let mut finishes = vec![choice("Inherit part finish", Value::Null)];
    finishes.extend(
        doc.finishes
            .values()
            .map(|finish| choice(&finish.name, &finish.id)),
    );
    let mut materials = vec![choice("Inherit part material", Value::Null)];
    materials.extend(
        doc.materials
            .values()
            .map(|material| choice(&material.name, &material.id)),
    );
    fields.choice(&format!("{P}/finish"), "Optical finish", finishes);
    fields.choice(&format!("{P}/material"), "Engineering material", materials);
    fields.toggle(&format!("{P}/suppressed"), "Suppress deposited bead");
    fields.note("native-solid","The bead is a separate native solid. Size, crown, stitches and offsets rebuild before Apply. Joint support trims are validated.","ready");
}
fn member_selection(fields: &mut Fields<'_>, path: &str, choices: Vec<Value>, all: bool) {
    let entries = fields
        .source
        .pointer(path)
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    fields.note(
        &format!("{path}/summary"),
        if all {
            "All members"
        } else {
            "Selected members"
        },
        "ready",
    );
    for candidate in &choices {
        let Some(value) = candidate.get("value") else {
            continue;
        };
        let selected = all || entries.contains(value);
        let key = item_identity(value, 0);
        fields.action(&format!("{path}/{key}/toggle"),candidate.get("label").and_then(Value::as_str).unwrap_or("Member"),json!({"kind":"member_toggle","pointer":path,"value":value,"choices":choices.iter().filter_map(|c|c.get("value")).collect::<Vec<_>>() }),true,"Finish the source edit first");
        if let Some(element) = fields.out.last_mut() {
            element.state = if selected { "selected" } else { "ready" }.into();
        }
    }
    fields.action(
        &format!("{path}/all"),
        "All members",
        json!({"kind":"set","pointer":path,"value":[]}),
        !all,
        "All members are already selected",
    );
}
fn mold_fields(fields: &mut Fields<'_>, operation: &cad_core::mold::MoldOperation) {
    use cad_core::mold::MoldOperation as M;
    const P: &str = "/operation/operation";
    fields.group("sources", "Source body");
    fields.reference(&format!("{P}/body"), "Body", "solid", false);
    match operation {
        M::DraftAnalysis { .. } => {
            fields.group("pull", "Pull direction");
            fields.axis(&format!("{P}/pull"), "Pull axis", false);
            fields.quantity(&format!("{P}/minimum"), "Minimum draft", "deg");
            fields.toggle(&format!("{P}/reverse"), "Reverse pull direction");
            fields.group("assessment", "Draft assessment");
            if let Some(report) =
                crate::mold_ui::report_for_feature(fields.w, fields.owner, &fields.feature.id)
            {
                fields.note(
                    "assessment",
                    if report.complete {
                        "Every face has an exact draft classification"
                    } else {
                        "Partial assessment: curved faces remain unassessed"
                    },
                    if report.complete { "ready" } else { "warning" },
                );
                for (index, face) in report.faces.iter().enumerate() {
                    let label = face
                        .angle_radians
                        .map(|a| {
                            format!(
                                "Face {} · {:?} · {:.3}°",
                                index + 1,
                                face.class,
                                a.to_degrees()
                            )
                        })
                        .unwrap_or_else(|| format!("Face {} · {:?}", index + 1, face.class));
                    fields.note(
                        &format!("face/{}", item_identity(&json!(face.face), index)),
                        &label,
                        "ready",
                    );
                }
                fields.note(
                    "parting-count",
                    &format!("{} exact planar parting edges", report.parting_edges.len()),
                    "ready",
                );
            } else {
                fields.note(
                    "assessment",
                    "Waiting for current source and pull-direction evaluation",
                    "pending",
                );
            }
            fields.note("release","Draft angles assess local normals; global undercut visibility and release checks remain separate.","ready");
        }
        M::PartingSurface { body, .. } => {
            fields.group("pull", "Pull direction");
            fields.axis(&format!("{P}/pull"), "Pull axis", false);
            fields.group("selection", "Parting edges");
            fields.topology_array(&format!("{P}/edges"), body, "edge");
            let analyzed=crate::mold_ui::source_assessment(fields.w,body).map(|(report,pull)|json!({"edges":report.parting_edges,"pull":pull,"direction":crate::mold_ui::perpendicular(report.pull)}));
            fields.action(
                "parting/analyzed",
                "Use analyzed parting edges",
                json!({"kind":"analyzed_parting","pointer":P,"value":analyzed}),
                analyzed.is_some(),
                "Evaluate Draft Analysis for the same source body first",
            );
            fields.group("extent", "Extension");
            fields.coordinates(&format!("{P}/direction"), "Direction", "");
            fields.quantity(&format!("{P}/width"), "Width", "mm");
            fields.toggle(&format!("{P}/reverse"), "Reverse face orientation");
        }
        M::ShutoffSurface { body, .. } => {
            fields.group("selection", "Opening edges");
            fields.topology_array(&format!("{P}/edges"), body, "edge");
            fields.toggle(&format!("{P}/reverse"), "Reverse face orientation");
            fields.note(
                "closed",
                "Select every edge of each closed planar opening.",
                "ready",
            );
        }
        M::Shrink { .. } => {
            fields.group("allowance", "Shrink allowance");
            fields.coordinates(&format!("{P}/factors"), "Factor", "");
            fields.note(
                "factor",
                "A factor of 1.02 applies 2% linear shrink allowance.",
                "ready",
            );
            fields.group("center", "Shrink center");
            fields.point(&format!("{P}/center"), "Center");
        }
        M::ToolingSplit { .. } => {
            fields.reference(
                &format!("{P}/stock"),
                "Closed mold stock sketch",
                "profile",
                false,
            );
            fields.group("split", "Tooling split");
            fields.quantity(&format!("{P}/depth"), "Stock depth", "mm");
            fields.plane(&format!("{P}/parting"), "Parting plane");
            fields.choice(
                &format!("{P}/side"),
                "Tool bodies",
                enum_choices(&[
                    ("both", "Core and cavity"),
                    ("core", "Core"),
                    ("cavity", "Cavity"),
                ]),
            );
        }
    }
}

pub fn semantics(w: &Workspace) -> Vec<SemanticElement> {
    if crate::sketch3d_ui::active(w) {
        return vec![];
    }
    if crate::structural_member_ui::active(w) {
        return vec![];
    }
    let Some((owner, feature)) = current(w) else {
        return vec![];
    };
    let mut fields = Fields::new(w, owner, feature);
    match &feature.operation {
        Operation::Surface { operation } => surface_fields(&mut fields, operation),
        Operation::DirectEdit { definition } => direct_fields(&mut fields, definition),
        Operation::Sheet { operation } => sheet_fields(&mut fields, operation),
        Operation::Weldment { operation } => weldment_fields(&mut fields, operation),
        Operation::WeldBead { definition } => weld_bead_fields(&mut fields, definition),
        Operation::Mold { operation } => mold_fields(&mut fields, operation),
        _ => common::fields(&mut fields),
    }
    for element in &mut fields.out {
        if let Some(error) = w.fields.get(&error_key(element)) {
            element.state = "error".into();
            element.data["diagnostics"] = json!([error]);
        }
    }
    fields.out
}

fn error_key(element: &SemanticElement) -> String {
    format!(
        "operation-editor:invalid:{}:{}:{}:{}:{}",
        element.data["document"],
        element.data["owner"],
        element.data["feature"],
        element.data["editor"],
        element.id
    )
}
pub fn field_is_invalid(w: &Workspace, element: &SemanticElement) -> bool {
    element.data.get("setter").and_then(Value::as_str) == Some("operation-editor")
        && w.fields.contains_key(&error_key(element))
}
pub fn field_error(w: &mut Workspace, element: &SemanticElement, error: Option<String>) {
    if element.data.get("setter").and_then(Value::as_str) != Some("operation-editor") {
        return;
    }
    let key = error_key(element);
    if let Some(error) = error {
        w.fields.insert(key, error);
        w.confirm_requested = false;
    } else {
        w.fields.remove(&key);
    }
}
pub fn current_source_is_valid(w: &Workspace) -> bool {
    // The usual valid case is queried by every Apply/toolbar presentation.
    // Avoid generating topology choices when there is no invalid field text.
    if !w
        .fields
        .keys()
        .any(|key| key.starts_with("operation-editor:invalid:"))
    {
        return true;
    }
    !semantics(w)
        .iter()
        .any(|element| field_is_invalid(w, element))
}

fn invalid(message: &str) -> CadError {
    CadError::Invalid(message.into())
}
fn live_element(w: &Workspace, element: &SemanticElement) -> Result<SemanticElement> {
    let fresh = semantics(w)
        .into_iter()
        .find(|e| e.id == element.id)
        .ok_or_else(|| invalid("This modeling control is no longer available"))?;
    for key in ["document", "owner", "feature", "editor"] {
        if element.data.get(key) != fresh.data.get(key) {
            return Err(invalid(
                "The modeling editor has changed; use its current controls",
            ));
        }
    }
    if fresh.state == "disabled" || w.fields.contains_key("feature-source-invalid") {
        return Err(invalid(
            "Correct Advanced Source or close Settings before editing this feature",
        ));
    }
    Ok(fresh)
}
pub fn current_value(w: &Workspace, element: &SemanticElement) -> Option<Value> {
    let (owner, feature) = current(w)?;
    if element.data.get("document").and_then(Value::as_str) != Some(&w.session.visible().id)
        || element.data.get("owner").and_then(Value::as_str) != Some(owner)
        || element.data.get("feature").and_then(Value::as_str) != Some(&feature.id)
        || element.data.get("editor").and_then(Value::as_str)
            != w.fields
                .get("feature-editor-generation")
                .map(String::as_str)
    {
        return None;
    }
    if element.data.get("transient").and_then(Value::as_str) == Some("skip-input") {
        return Some(json!(
            w.fields
                .get(&transient_key(element))
                .and_then(|v| v.parse::<usize>().ok())
                .unwrap_or(1)
        ));
    }
    let key = json!([
        w.session.visible().id,
        w.session.generation,
        w.session.committed.revision,
        w.session.draft.as_ref().map(|d| (&d.id, d.edit)),
        owner,
        feature.id,
        w.fields.get("feature-editor-generation")
    ]);
    let mut cache = w.operation_values.lock().ok()?;
    if cache.key.as_ref() != Some(&key) {
        cache.source = editable_source(feature);
        cache.key = Some(key);
    }
    let value = cache
        .source
        .pointer(element.data.get("pointer")?.as_str()?)
        .cloned()
        .or_else(|| element.data.get("optional_default").map(|_| Value::Null))?;
    if element.data.get("optional_default").is_some() {
        Some(json!(!value.is_null()))
    } else {
        Some(value)
    }
}
fn replace_value(source: &mut Value, pointer: &str, value: Value) -> Result<()> {
    if let Some(target) = source.pointer_mut(pointer) {
        *target = value;
    } else {
        let (parent, key) = pointer
            .rsplit_once('/')
            .ok_or_else(|| invalid("Invalid operation field"))?;
        let key = key.replace("~1", "/").replace("~0", "~");
        source
            .pointer_mut(parent)
            .and_then(Value::as_object_mut)
            .ok_or_else(|| invalid("The operation field is no longer available"))?
            .insert(key, value);
    }
    Ok(())
}
fn publish(w: &mut Workspace, feature: Feature) -> Result<()> {
    let owner = w
        .view
        .active_part
        .clone()
        .ok_or_else(|| invalid("The editing part has closed"))?;
    let expected = feature.clone();
    w.dispatch(cad_core::commands::Command::PutFeature {
        definition: owner.clone(),
        feature: feature.into(),
        output: false,
    });
    if !w
        .session
        .visible()
        .parts
        .get(&owner)
        .and_then(|p| p.features.iter().find(|f| f.id == expected.id))
        .is_some_and(|f| f == &expected)
    {
        return Err(invalid(&w.status));
    }
    w.json = serde_json::to_string_pretty(&expected)?;
    Ok(())
}
pub fn set_value(w: &mut Workspace, element: &SemanticElement, value: Value) -> Result<()> {
    let result = set_field(w, element, value);
    field_error(w, element, result.as_ref().err().map(ToString::to_string));
    result
}
fn set_field(w: &mut Workspace, element: &SemanticElement, value: Value) -> Result<()> {
    let element = live_element(w, element)?;
    if element.data.get("transient").and_then(Value::as_str) == Some("skip-input") {
        let value = value
            .as_u64()
            .and_then(|v| usize::try_from(v).ok())
            .filter(|v| *v > 0)
            .ok_or_else(|| invalid("Enter a positive integer instance position"))?;
        w.fields.insert(transient_key(&element), value.to_string());
        w.confirm_requested = false;
        return Ok(());
    }
    let (_, feature) = current(w).ok_or_else(|| invalid("The modeling editor has closed"))?;
    let pointer = element
        .data
        .get("pointer")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid("This control has no value binding"))?;
    let descriptor = element.value_descriptor();
    if !descriptor.choices.is_empty()
        && !descriptor
            .choices
            .iter()
            .any(|c| c.get("value").unwrap_or(c) == &value)
    {
        return Err(invalid(
            "Choose one of the current compatible references or options",
        ));
    }
    let mut source = editable_source(feature);
    let before = source
        .pointer(pointer)
        .cloned()
        .or_else(|| element.data.get("optional_default").map(|_| Value::Null))
        .ok_or_else(|| invalid("The bound operation field has changed"))?;
    let next = if element
        .data
        .get("accepts_quantity_text")
        .and_then(Value::as_bool)
        == Some(true)
        && value.is_string()
    {
        let axis = if element.data.get("unit").and_then(Value::as_str) == Some("deg") {
            3
        } else {
            0
        };
        json!(cad_core::configuration::placement_value(
            value.as_str().unwrap(),
            axis
        )?)
    } else if let Some(default) = element.data.get("optional_default") {
        let enabled = value
            .as_bool()
            .ok_or_else(|| invalid("Expected an override toggle"))?;
        if enabled {
            if before.is_null() {
                default.clone()
            } else {
                before.clone()
            }
        } else {
            Value::Null
        }
    } else {
        value
    };
    if before == next {
        return Ok(());
    }
    replace_value(&mut source, pointer, next.clone())?;
    clear_dependent_selections(&mut source, pointer, &next, w)?;
    let parsed: Feature = serde_json::from_value(source)?;
    publish(w, parsed)
}
fn clear_dependent_selections(
    source: &mut Value,
    pointer: &str,
    next: &Value,
    w: &Workspace,
) -> Result<()> {
    let kind = source
        .pointer("/operation/kind")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    if pointer.ends_with("/body") {
        if kind == "direct_edit" {
            replace_value(source, "/operation/definition/faces", json!([]))?;
        }
        if kind == "chamfer" {
            replace_value(source, "/operation/options/first_face", Value::Null)?;
        }
        for path in [
            "/operation/edges",
            "/operation/faces",
            "/operation/removed_faces",
            "/operation/options/face_thicknesses",
        ] {
            if source.pointer(path).is_some() {
                replace_value(source, path, json!([]))?;
            }
        }
        for path in ["/operation/operation/edges", "/operation/operation/members"] {
            if source.pointer(path).is_some() {
                replace_value(source, path, json!([]))?;
            }
        }
        if let Ok(body) = serde_json::from_value::<NodeRef>(next.clone())
            && source.pointer("/operation/operation/member").is_some()
        {
            if kind == "sheet" {
                repair_sheet_target(source, w, true)?;
            } else if kind == "weldment"
                && let Ok(stock) = cad_core::weldment::stock(w.session.visible(), &body)
                && let Some((member, _)) = crate::fabrication_ui::member_choices(w, &stock)
                    .into_iter()
                    .next()
            {
                replace_value(source, "/operation/operation/member", json!(member))?;
            }
        }
    }
    if kind == "sheet" && pointer == "/operation/operation/attachment" {
        repair_sheet_target(source, w, false)?;
    }
    if pointer == "/operation/options/uniform" && next == &Value::Bool(false) {
        let factor = source.pointer("/operation/options/factors/0").cloned();
        if let Some(factor) = factor {
            replace_value(
                source,
                "/operation/options/factors",
                json!([factor.clone(), factor.clone(), factor]),
            )?;
        }
    }
    if pointer == "/operation/operation/surface"
        && source.pointer("/operation/operation/edges").is_some()
    {
        replace_value(source, "/operation/operation/edges", json!([]))?;
    }
    if pointer == "/operation/operation/path"
        && source.pointer("/operation/operation/members").is_some()
    {
        replace_value(source, "/operation/operation/members", json!([]))?;
    }
    if pointer == "/operation/operation/section"
        && source.pointer("/operation/operation/anchor").is_some()
    {
        replace_value(source, "/operation/operation/anchor", Value::Null)?;
    }
    Ok(())
}
fn repair_sheet_target(source: &mut Value, w: &Workspace, reset: bool) -> Result<()> {
    const P: &str = "/operation/operation";
    let body: NodeRef = serde_json::from_value(
        source
            .pointer(&format!("{P}/body"))
            .cloned()
            .ok_or_else(|| invalid("Sheet stock selection is missing"))?,
    )?;
    let stock = cad_core::sheet_metal::stock(w.session.visible(), &body)?;
    if reset {
        let (member, attachment) = crate::fabrication_ui::sheet_target(w, &stock)
            .ok_or_else(|| invalid("Sheet stock has no available base or terminal flange edge"))?;
        replace_value(source, &format!("{P}/member"), json!(member))?;
        replace_value(source, &format!("{P}/attachment"), json!(attachment))?;
        return Ok(());
    }
    let attachment: Option<NodeRef> = serde_json::from_value(
        source
            .pointer(&format!("{P}/attachment"))
            .cloned()
            .unwrap_or(Value::Null),
    )?;
    if let Some(attachment) = attachment {
        let parent = stock
            .flanges
            .iter()
            .find(|flange| {
                flange.source == attachment
                    && !stock
                        .flanges
                        .iter()
                        .any(|child| child.attachment.as_ref() == Some(&attachment))
            })
            .ok_or_else(|| {
                invalid("Choose an available terminal edge in the selected sheet stock")
            })?;
        replace_value(source, &format!("{P}/member"), json!(parent.member))?;
    } else {
        let choices = crate::fabrication_ui::sheet_base_members(w, &stock);
        let current = source
            .pointer(&format!("{P}/member"))
            .and_then(Value::as_str)
            .unwrap_or("");
        if !choices.iter().any(|(member, _)| member == current) {
            let (member, _) = choices.into_iter().next().ok_or_else(|| {
                invalid("No unused base edge remains; choose a terminal flange attachment")
            })?;
            replace_value(source, &format!("{P}/member"), json!(member))?;
        }
    }
    Ok(())
}
pub fn action(w: &mut Workspace, command: &str, args: Option<&Value>) -> bool {
    if command != ACTION {
        return false;
    }
    let result = (|| -> Result<()> {
        if w.settings.is_some() {
            return Err(invalid("Close Settings before editing this operation"));
        }
        let args =
            args.ok_or_else(|| invalid("Choose a modeling action from the active editor"))?;
        let id = args
            .get("element")
            .and_then(Value::as_str)
            .ok_or_else(|| invalid("This modeling action has no target"))?;
        let fresh = semantics(w)
            .into_iter()
            .find(|e| e.id == id)
            .ok_or_else(|| invalid("The modeling action is no longer available"))?;
        let snapshot = SemanticElement {
            data: args.clone(),
            ..fresh.clone()
        };
        let fresh = live_element(w, &snapshot)?;
        if args.get("source_revision") != fresh.data.get("source_revision")
            || args.get("mutation") != fresh.data.get("mutation")
        {
            return Err(invalid(
                "The source or selection changed; use the current modeling action",
            ));
        }
        let (_, feature) = current(w).ok_or_else(|| invalid("The modeling editor has closed"))?;
        let mut source = editable_source(feature);
        let mutation = fresh
            .data
            .get("mutation")
            .ok_or_else(|| invalid("This modeling action has no mutation"))?;
        let pointer = mutation
            .get("pointer")
            .and_then(Value::as_str)
            .ok_or_else(|| invalid("This modeling action has no source field"))?;
        match mutation.get("kind").and_then(Value::as_str).unwrap_or("") {
            "set" => replace_value(&mut source, pointer, mutation["value"].clone())?,
            "append" | "append_segment" => {
                let next = if mutation["kind"] == "append_segment" {
                    json!({"id":cad_core::id(),"source":mutation["source"],"reverse":false})
                } else {
                    mutation["value"].clone()
                };
                source
                    .pointer_mut(pointer)
                    .and_then(Value::as_array_mut)
                    .ok_or_else(|| invalid("The source list no longer exists"))?
                    .push(next);
            }
            "remove" => {
                let index = mutation
                    .get("index")
                    .and_then(Value::as_u64)
                    .ok_or_else(|| invalid("Missing selected item"))?
                    as usize;
                let array = source
                    .pointer_mut(pointer)
                    .and_then(Value::as_array_mut)
                    .ok_or_else(|| invalid("The source list no longer exists"))?;
                if index >= array.len() {
                    return Err(invalid("The selected item no longer exists"));
                }
                array.remove(index);
            }
            "swap" => {
                let first = mutation
                    .get("first")
                    .and_then(Value::as_u64)
                    .ok_or_else(|| invalid("Missing selected item"))?
                    as usize;
                let second = mutation
                    .get("second")
                    .and_then(Value::as_u64)
                    .ok_or_else(|| invalid("Missing destination"))?
                    as usize;
                let array = source
                    .pointer_mut(pointer)
                    .and_then(Value::as_array_mut)
                    .ok_or_else(|| invalid("The source list no longer exists"))?;
                if first >= array.len() || second >= array.len() {
                    return Err(invalid("The list order has changed"));
                }
                array.swap(first, second);
            }
            "member_toggle" => {
                let value = mutation["value"].clone();
                let array = source
                    .pointer_mut(pointer)
                    .and_then(Value::as_array_mut)
                    .ok_or_else(|| invalid("The member list no longer exists"))?;
                if array.is_empty() {
                    *array = mutation["choices"].as_array().cloned().unwrap_or_default();
                }
                if array.contains(&value) {
                    array.retain(|v| v != &value);
                } else {
                    array.push(value);
                }
            }
            "analyzed_parting" => {
                for key in ["edges", "pull", "direction"] {
                    replace_value(
                        &mut source,
                        &format!("{pointer}/{key}"),
                        mutation["value"][key].clone(),
                    )?;
                }
            }
            _ => return Err(invalid("Unsupported modeling action")),
        }
        let parsed: Feature = serde_json::from_value(source)?;
        publish(w, parsed)
    })();
    if let Err(error) = result {
        w.status = error.to_string();
    }
    true
}

/// The native property manager renders the descriptors as an ordinary compact
/// composition. Users can replace it with repeated fields/actions in any layout.
pub fn show(ui: &mut egui::Ui, w: &mut Workspace) -> bool {
    if crate::sketch3d_ui::show(ui, w) {
        return true;
    }
    if crate::structural_member_ui::show(ui, w) {
        return true;
    }
    if current(w).is_none() {
        return false;
    }
    let elements = semantics(w);
    render_elements(ui, w, elements);
    true
}

/// Render ordinary editor descriptors; source setters and commands remain
/// registered separately so other editor families share this composition.
pub(crate) fn render_elements(
    ui: &mut egui::Ui,
    w: &mut Workspace,
    elements: Vec<SemanticElement>,
) {
    let mut groups: Vec<(SemanticElement, Vec<SemanticElement>)> = vec![];
    for element in elements {
        if element.role.ends_with("-group") {
            groups.push((element, vec![]));
        } else if let Some((_, children)) = groups.last_mut() {
            children.push(element);
        }
    }
    for (group, children) in groups {
        let classes = group
            .data
            .get("classes")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>();
        let props = controls::properties(&group.id, &group.role, &classes, "draft");
        controls::inspector_section(
            ui,
            controls::InspectorSection {
                id: &group.id,
                role: &group.role,
                classes: &classes,
                label: &group.label,
                state: "draft",
                collapse: None,
                open: None,
            },
            |ui| {
                let mut render = |ui: &mut egui::Ui| {
                    let mut index = 0;
                    while index < children.len() {
                        let element = &children[index];
                        if element.data.get("inline_parent").is_some() {
                            index += 1;
                            continue;
                        }
                        if element.role.ends_with("-action") {
                            let end = children[index..]
                                .iter()
                                .position(|e| !e.role.ends_with("-action"))
                                .map_or(children.len(), |n| index + n);
                            ui.horizontal_wrapped(|ui| {
                                for element in &children[index..end] {
                                    if element.data.get("inline_parent").is_none() {
                                        render_action(ui, w, element);
                                    }
                                }
                            });
                            index = end;
                            continue;
                        }
                        if crate::selection_ui::render_edge_row(ui, w, element) {
                            index += 1;
                            continue;
                        }
                        if element.data.get("setter").is_some() {
                            crate::ui::semantic_field(ui, w, element);
                        } else {
                            controls::label(
                                ui,
                                &element.id,
                                &element.role,
                                &["modeling-help"],
                                &element
                                    .value
                                    .as_ref()
                                    .map(|v| format!("{} · {v}", element.label))
                                    .unwrap_or_else(|| element.label.clone()),
                                &element.state,
                            );
                        }
                        index += 1;
                    }
                };
                if props
                    .get("overflow")
                    .is_some_and(|v| matches!(v.as_str(), "auto" | "scroll"))
                {
                    egui::ScrollArea::vertical()
                        .id_salt(&group.id)
                        .max_height(controls::number(
                            &props,
                            "max-height",
                            ui.available_height().max(60.),
                        ))
                        .auto_shrink([false, true])
                        .show(ui, render);
                } else {
                    render(ui);
                }
            },
        );
    }
}

fn render_action(ui: &mut egui::Ui, w: &mut Workspace, element: &SemanticElement) {
    let classes = element
        .data
        .get("classes")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>();
    if controls::button(
        ui,
        &element.id,
        &element.role,
        &classes,
        &element.label,
        element.state != "disabled",
        element.state == "selected",
    )
    .on_hover_text(element.value.as_deref().unwrap_or(&element.label))
    .on_disabled_hover_text(element.value.as_deref().unwrap_or(&element.label))
    .clicked()
        && let Some(descriptor) = element.action_descriptors().into_iter().find(|a| a.enabled)
    {
        let mut target = element.clone();
        target.data = descriptor.arguments;
        crate::ui::semantic_action(w, &target, &descriptor.command);
    }
}

fn editable_source(feature: &Feature) -> Value {
    let mut source = json!(feature);
    common::normalize_source(&feature.operation, &mut source);
    if let Operation::Weldment { operation } = &feature.operation
        && let cad_core::weldment::WeldmentOperation::Members {
            profile_offset,
            member_mode,
            ..
        } = operation.as_ref()
    {
        // Default omission keeps saved legacy definitions compact. Controls
        // still expose every typed placement field without rewriting source.
        source["operation"]["operation"]["profile_offset"] = json!(profile_offset);
        source["operation"]["operation"]["member_mode"] = json!(member_mode);
    }
    if let Operation::WeldBead { definition } = &feature.operation {
        source["operation"]["definition"]["supports"] = json!(definition.supports);
        source["operation"]["definition"]["finish"] = json!(definition.finish);
        source["operation"]["definition"]["material"] = json!(definition.material);
    }
    if let Operation::Sheet { operation } = &feature.operation {
        use cad_core::sheet_metal::SheetOperation as S;
        if let S::EdgeFlange { attachment, .. }
        | S::Hem { attachment, .. }
        | S::Jog { attachment, .. } = operation.as_ref()
        {
            source["operation"]["operation"]["attachment"] = json!(attachment);
        }
    }
    source
}

fn transient_key(element: &SemanticElement) -> String {
    format!(
        "{}:{}",
        element.id,
        element
            .data
            .get("editor")
            .and_then(Value::as_str)
            .unwrap_or("")
    )
}
