//! Independently composable drawing controls over staged native sheet definitions.
use crate::Workspace;
use bevy_egui::egui;
use cad_core::{
    CadError, Drawing, DrawingDimension, Id, Result, drawing::*, drawing_tables::*,
    presentation::SemanticElement,
};
use serde_json::{Value, json};

const CONTRACT: &str = "tcad.presentation.drawing-editor";
const VERSION: u32 = 1;
const ACTION: &str = "drawing-editor.action";
const GENERATION: &str = "drawing-editor-generation";

#[derive(Default)]
pub struct Cache {
    key: Option<Value>,
    elements: Vec<SemanticElement>,
    source: Option<SourceValue>,
}
struct SourceValue {
    identity: cad_core::protocol::RequestIdentity,
    sheet: Id,
    text: Option<String>,
    value: Value,
}

fn invalid(message: &str) -> CadError {
    CadError::Invalid(message.into())
}
fn collection_key(w: &Workspace, sheet: &str, collection: &str) -> String {
    format!(
        "drawing-control:collection:{}:{sheet}:{collection}",
        w.fields.get(GENERATION).map(String::as_str).unwrap_or("")
    )
}
fn collection_tokens(w: &Workspace, sheet: &str, collection: &str, count: usize) -> Vec<String> {
    let saved = w
        .fields
        .get(&collection_key(w, sheet, collection))
        .and_then(|s| serde_json::from_str::<Vec<String>>(s).ok());
    (0..count)
        .map(|i| {
            saved
                .as_ref()
                .and_then(|v| v.get(i))
                .cloned()
                .unwrap_or_else(|| {
                    format!(
                        "{}-slot-{i}",
                        if saved.is_some() {
                            "untracked"
                        } else {
                            "source"
                        }
                    )
                })
        })
        .collect()
}
fn append_token(w: &mut Workspace, sheet: &str, collection: &str, count: usize) -> String {
    let mut tokens = collection_tokens(w, sheet, collection, count);
    let token = cad_core::id();
    tokens.push(token.clone());
    w.fields.insert(
        collection_key(w, sheet, collection),
        json!(tokens).to_string(),
    );
    token
}
fn remove_token(w: &mut Workspace, sheet: &str, collection: &str, count: usize, index: usize) {
    let mut tokens = collection_tokens(w, sheet, collection, count);
    if index < tokens.len() {
        tokens.remove(index);
    }
    w.fields.insert(
        collection_key(w, sheet, collection),
        json!(tokens).to_string(),
    );
}
fn active_sheet_id(w: &Workspace) -> Option<&str> {
    w.fields
        .get("drawing-active")
        .filter(|id| w.session.visible().drawings.iter().any(|s| &s.id == *id))
        .or_else(|| w.session.visible().drawings.first().map(|s| &s.id))
        .map(String::as_str)
}
fn active_sheet(w: &Workspace) -> Option<Drawing> {
    crate::drawing_ui::sheet_source(w, active_sheet_id(w)?)
}
fn section(w: &Workspace, sheet: &Drawing) -> String {
    let requested = w
        .fields
        .get("drawing-section")
        .map(String::as_str)
        .unwrap_or("sheet");
    if requested == "items" {
        let item = w.fields.get("drawing-item");
        if sheet.tables.iter().any(|v| Some(&v.id) == item) {
            "tables"
        } else if sheet.dimensions.iter().any(|v| Some(&v.id) == item) {
            "dimensions"
        } else {
            "annotations"
        }
        .into()
    } else {
        requested.into()
    }
}
fn escape(key: &str) -> String {
    key.replace('~', "~0").replace('/', "~1")
}
fn unescape(key: &str) -> String {
    key.replace("~1", "/").replace("~0", "~")
}
fn value_caption(value: &Value) -> String {
    value
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| value.to_string())
}
fn choice<T: serde::Serialize>(label: impl Into<String>, value: T) -> Value {
    json!({"label":label.into(),"value":value})
}
fn choices(values: &[(&str, &str)]) -> Vec<Value> {
    values
        .iter()
        .map(|(value, label)| choice(*label, *value))
        .collect()
}

pub(crate) struct Fields<'a> {
    pub(crate) w: &'a Workspace,
    pub(crate) sheet: &'a Drawing,
    source: Value,
    pub(crate) out: Vec<SemanticElement>,
    group: String,
}
impl<'a> Fields<'a> {
    fn new(w: &'a Workspace, sheet: &'a Drawing) -> Self {
        Self {
            w,
            sheet,
            source: json!(sheet),
            out: vec![],
            group: "sheet".into(),
        }
    }
    fn id(&self, key: &str) -> String {
        format!("drawing-editor:{}/{key}", self.sheet.id)
    }
    fn data(&self) -> Value {
        json!({"contract":CONTRACT,"version":VERSION,"document":self.w.session.visible().id,"editor":self.w.fields.get(GENERATION),"sheet_id":self.sheet.id,"owner":self.sheet.id,"group":self.group})
    }
    fn address(&self, path: &str) -> (String, Value) {
        let mut value = &self.source;
        let mut identity = Vec::new();
        let mut address = Vec::new();
        let mut variable = false;
        for component in path.trim_start_matches('/').split('/') {
            let key = unescape(component);
            if let Some(array) = value.as_array() {
                let index = key.parse::<usize>().unwrap_or(usize::MAX);
                let item = array.get(index).unwrap_or(&Value::Null);
                if let Some(id) = item.get("id").and_then(Value::as_str) {
                    identity.push(format!("@{id}"));
                    address.push(json!({"id":id}));
                } else if variable {
                    let collection = identity.join("/");
                    let token = collection_tokens(self.w, &self.sheet.id, &collection, array.len())
                        .get(index)
                        .cloned()
                        .unwrap_or_else(|| format!("untracked-slot-{index}"));
                    identity.push(format!("@slot:{token}"));
                    address.push(json!({"slot":token,"collection":collection}));
                } else {
                    identity.push(key.clone());
                    address.push(json!({"index":index}));
                }
                value = item;
            } else {
                identity.push(escape(&key));
                address.push(json!({"key":key}));
                value = value.get(&key).unwrap_or(&Value::Null);
                variable = matches!(
                    key.as_str(),
                    "rows"
                        | "headers"
                        | "annotations"
                        | "datums"
                        | "column_widths_mm"
                        | "row_order"
                        | "leader_points_mm"
                );
            }
        }
        (identity.join("/"), json!(address))
    }
    pub(crate) fn group(&mut self, key: &str, label: &str) {
        self.group = key.into();
        let mut data = self.data();
        data["classes"] = json!(["drawing-group", key]);
        self.out.push(SemanticElement {
            id: self.id(&format!("group/{key}")),
            role: "drawing-editor-group".into(),
            label: label.into(),
            value: None,
            state: "draft".into(),
            commands: vec![],
            relationships: vec![self.sheet.id.clone()],
            data,
        });
    }
    fn emit(
        &mut self,
        path: &str,
        label: &str,
        value: Value,
        options: Vec<Value>,
        unit: Option<&str>,
    ) {
        let (identity, address) = self.address(path);
        let mut data = self.data();
        data["setter"] = json!("drawing-editor");
        data["pointer"] = json!(path);
        data["address"] = address;
        data["json"] = value.clone();
        data["editable"] = json!(self.w.settings.is_none());
        data["choices"] = json!(options);
        data["unit"] = json!(unit);
        data["accessibility_label"] = json!(label);
        data["classes"] = json!(["drawing-field", self.group]);
        data["value_type"] = json!(if !options.is_empty() {
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
        self.out.push(SemanticElement {
            id: self.id(&identity),
            role: "drawing-editor-field".into(),
            label: label.into(),
            value: Some(value_caption(&value)),
            state: if self.w.settings.is_some() {
                "disabled"
            } else {
                "draft"
            }
            .into(),
            commands: vec![],
            relationships: vec![self.sheet.id.clone()],
            data,
        });
    }
    pub(crate) fn field(
        &mut self,
        path: &str,
        label: &str,
        options: Vec<Value>,
        unit: Option<&str>,
    ) {
        let value = self.source.pointer(path).cloned().unwrap_or(Value::Null);
        self.emit(path, label, value, options, unit);
    }
    pub(crate) fn text(&mut self, path: &str, label: &str, multiline: bool) {
        self.field(path, label, vec![], None);
        if let Some(e) = self.out.last_mut() {
            e.data["multiline"] = json!(multiline);
        }
    }
    pub(crate) fn number(
        &mut self,
        path: &str,
        label: &str,
        unit: &str,
        min: Option<f64>,
        max: Option<f64>,
    ) {
        self.field(path, label, vec![], Some(unit));
        if let Some(e) = self.out.last_mut() {
            e.data["minimum"] = json!(min);
            e.data["maximum"] = json!(max);
        }
    }
    pub(crate) fn toggle(&mut self, path: &str, label: &str) {
        self.field(path, label, vec![], None);
    }
    pub(crate) fn select(&mut self, path: &str, label: &str, mut options: Vec<Value>) {
        let value = self.source.pointer(path).cloned().unwrap_or(Value::Null);
        if (path.ends_with("/value") || path.ends_with("/tolerance"))
            && let Some(kind) = value.get("kind").and_then(Value::as_str)
            && let Some(option) = options.iter_mut().find(|option| {
                option
                    .get("value")
                    .and_then(|v| v.get("kind"))
                    .and_then(Value::as_str)
                    == Some(kind)
            })
        {
            option["value"] = value.clone();
        }
        if !options.iter().any(|c| c.get("value") == Some(&value)) {
            options.push(choice("Saved reference / option", value));
        }
        self.field(path, label, options, None);
    }
    fn optional(&mut self, path: &str, label: &str, factory: &str) {
        let enabled = self.source.pointer(path).is_some_and(|v| !v.is_null());
        self.emit(path, label, json!(enabled), vec![], None);
        let id = self.id(&format!("{}/enabled", self.address(path).0));
        if let Some(e) = self.out.last_mut() {
            e.id = id;
            e.data["optional_factory"] = json!(factory);
        }
    }
    pub(crate) fn transient(&mut self, key: &str, label: &str, value: Value, options: Vec<Value>) {
        let empty = options.is_empty() && key != "preview";
        self.emit(
            &format!("/{key}"),
            label,
            if empty { json!("No items") } else { value },
            options,
            None,
        );
        let id = self.id(&format!("context/{key}"));
        if let Some(e) = self.out.last_mut() {
            e.id = id;
            e.data["transient"] = json!(key);
            if empty {
                e.data["editable"] = json!(false);
                e.state = "disabled".into();
            }
        }
    }
    pub(crate) fn action(
        &mut self,
        key: &str,
        label: &str,
        mutation: Value,
        enabled: bool,
        reason: &str,
    ) {
        let id = self.id(&format!("action/{key}"));
        let available = enabled && self.w.settings.is_none();
        let mut data = self.data();
        data["element"] = json!(id);
        data["mutation"] = mutation;
        data["classes"] = json!(["drawing-action", self.group]);
        data["actions"] = json!([{"command":ACTION,"label":label,"enabled":available,"disabled_reason":(!available).then_some(reason),"arguments":data.clone()}]);
        self.out.push(SemanticElement {
            id,
            role: "drawing-editor-action".into(),
            label: label.into(),
            value: (!available).then(|| reason.into()),
            state: if available { "ready" } else { "disabled" }.into(),
            commands: vec![ACTION.into()],
            relationships: vec![self.sheet.id.clone()],
            data,
        });
    }
    pub(crate) fn note(&mut self, key: &str, label: &str, state: &str) {
        self.out.push(SemanticElement {
            id: self.id(&format!("note/{key}")),
            role: "drawing-editor-help".into(),
            label: label.into(),
            value: None,
            state: state.into(),
            commands: vec![],
            relationships: vec![self.sheet.id.clone()],
            data: self.data(),
        });
    }
    pub(crate) fn coordinates(&mut self, path: &str, labels: &[&str], unit: &str) {
        for (i, label) in labels.iter().enumerate() {
            self.number(&format!("{path}/{i}"), label, unit, None, None);
        }
    }
    fn view(&mut self, path: &str) {
        self.select(
            path,
            "View",
            view_keys(self.sheet).iter().map(|v| choice(v, v)).collect(),
        );
    }
    fn point(&mut self, point: &str, reference: &str, label: &str) {
        let mut options = vec![choice("Explicit coordinates", Value::Null)];
        options.extend(
            crate::drawing_ui::reference_choices(self.w)
                .into_iter()
                .map(|(value, label)| choice(label, value)),
        );
        if let Some(PointReference::EdgePoint {
            edge,
            body,
            instance_path,
            ..
        }) = crate::drawing_ui::selected_anchor(self.w).reference
        {
            for (parameter, label) in [
                (0., "Selected edge start"),
                (1., "Selected edge end"),
                (0.5, "Selected edge midpoint"),
            ] {
                options.push(choice(
                    label,
                    PointReference::EdgePoint {
                        edge: edge.clone(),
                        body: body.clone(),
                        instance_path: instance_path.clone(),
                        parameter,
                    },
                ));
            }
        } else if let Some(reference) = crate::drawing_ui::selected_anchor(self.w).reference {
            options.push(choice("Selected face origin", reference));
        }
        self.select(reference, label, options);
        match self
            .source
            .pointer(reference)
            .cloned()
            .unwrap_or(Value::Null)
        {
            Value::Null => self.coordinates(point, &["Model X", "Model Y", "Model Z"], "mm"),
            value => {
                if value.get("kind").and_then(Value::as_str) == Some("edge_point") {
                    self.number(
                        &format!("{reference}/parameter"),
                        "Native edge parameter",
                        "",
                        Some(0.),
                        Some(1.),
                    );
                }
                let definition = value
                    .get("sketch")
                    .and_then(|v| v.get("definition"))
                    .or_else(|| {
                        value
                            .get("edge")
                            .and_then(|v| v.get("producer"))
                            .and_then(|v| v.get("definition"))
                    })
                    .or_else(|| {
                        value
                            .get("face")
                            .and_then(|v| v.get("producer"))
                            .and_then(|v| v.get("definition"))
                    })
                    .and_then(Value::as_str)
                    .unwrap_or("");
                self.select(
                    &format!("{reference}/instance_path"),
                    "Component occurrence",
                    occurrence_choices(self.w, definition),
                );
            }
        }
    }
    pub(crate) fn anchor(&mut self, path: &str, label: &str) {
        self.point(
            &format!("{path}/point"),
            &format!("{path}/reference"),
            label,
        );
    }
    fn curve(&mut self, path: &str, optional: bool) {
        let mut options = curve_choices(self.w, false);
        if optional {
            options.insert(0, choice("Choose circular geometry", Value::Null));
        }
        self.select(path, "Circle / arc / model edge", options);
        if let Some(value) = self.source.pointer(path) {
            let definition = value
                .get("sketch")
                .and_then(|v| v.get("definition"))
                .or_else(|| {
                    value
                        .get("edge")
                        .and_then(|v| v.get("producer"))
                        .and_then(|v| v.get("definition"))
                })
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            if !definition.is_empty() {
                self.select(
                    &format!("{path}/instance_path"),
                    "Component occurrence",
                    occurrence_choices(self.w, &definition),
                );
            }
        }
    }
    fn line(&mut self, path: &str, label: &str) {
        let mut options = curve_choices(self.w, true);
        options.insert(0, choice("Choose straight geometry", Value::Null));
        self.select(path, label, options);
    }
}
fn occurrence_choices(w: &Workspace, definition: &str) -> Vec<Value> {
    let mut out = vec![choice("Resolve unique occurrence", Vec::<Id>::new())];
    if crate::drawing_ui::evaluation(w).is_some() {
        out.extend(
            crate::drawing_ui::represented_instances(w)
                .iter()
                .filter(|i| i.definition == definition)
                .map(|i| choice(&i.name, &i.path)),
        );
    }
    out
}
fn curve_choices(w: &Workspace, straight: bool) -> Vec<Value> {
    let mut out = vec![];
    let source = active_sheet(w);
    if let Some(curve) = crate::drawing_ui::selected_curve(w)
        && let Some(evaluation) = crate::drawing_ui::evaluation(w)
        && let Some(sheet) = &source
        && if straight {
            linear(w.session.visible(), evaluation, sheet, &curve).is_ok()
        } else {
            circular(w.session.visible(), evaluation, sheet, &curve).is_ok()
        }
    {
        out.push(choice("Selected model edge", curve));
    }
    if let Some(e) = crate::drawing_ui::evaluation(w) {
        for instance in &crate::drawing_ui::represented_instances(w) {
            let mesh_key = if instance.mesh_key.is_empty() {
                &instance.definition
            } else {
                &instance.mesh_key
            };
            if let Some(mesh) = e.meshes.get(mesh_key) {
                for (index, edge) in mesh.edges.iter().enumerate() {
                    let reference = CurveReference::ModelEdge {
                        edge: edge.reference.clone(),
                        body: edge.body.clone(),
                        instance_path: instance.path.clone(),
                    };
                    if let Some(sheet) = &source
                        && if straight {
                            linear(w.session.visible(), e, sheet, &reference).is_ok()
                        } else {
                            circular(w.session.visible(), e, sheet, &reference).is_ok()
                        }
                    {
                        out.push(choice(
                            format!("{} / Edge {}", instance.name, index + 1),
                            reference,
                        ));
                    }
                }
            }
            if let Some(part) = w.session.visible().parts.get(&instance.definition) {
                let key = if instance.mesh_key.is_empty() {
                    &instance.definition
                } else {
                    &instance.mesh_key
                };
                for feature in &part.features {
                    if let Some(sketch) = e
                        .sketches
                        .get(&format!("{key}::{}", feature.id))
                        .and_then(|s| s.solved.as_ref())
                    {
                        let suppressed = cad_core::sketch_transform::suppressed_members(sketch);
                        for (member, m) in &sketch.members {
                            if suppressed.contains(member) {
                                continue;
                            }
                            if if straight {
                                matches!(m.curve, cad_core::SketchCurve::Line { .. })
                            } else {
                                matches!(
                                    m.curve,
                                    cad_core::SketchCurve::Circle { .. }
                                        | cad_core::SketchCurve::Arc { .. }
                                )
                            } {
                                out.push(choice(
                                    format!(
                                        "{} / {} · {}",
                                        instance.name,
                                        feature.name,
                                        if straight { "line" } else { "circle or arc" }
                                    ),
                                    CurveReference::SketchMember {
                                        sketch: part.reference(&feature.id),
                                        member: member.clone(),
                                        instance_path: instance.path.clone(),
                                    },
                                ));
                            }
                        }
                    }
                }
            }
        }
    }
    out
}
fn configuration_choices(w: &Workspace, owner: &str) -> Vec<Value> {
    let mut out = vec![choice("Represented / current", Value::Null)];
    out.extend(
        cad_core::configuration::names(w.session.visible(), owner)
            .iter()
            .map(|(id, name)| choice(name, id)),
    );
    out
}
fn source_choices(w: &Workspace, kind: &str) -> Vec<Value> {
    w.session
        .visible()
        .parts
        .values()
        .flat_map(|p| {
            p.features
                .iter()
                .filter(move |f| {
                    !cad_core::configuration::feature_suppressed(w.session.visible(), &p.id, f)
                        && match kind {
                            "holes" => matches!(
                                f.operation,
                                cad_core::Operation::Hole { .. }
                                    | cad_core::Operation::DrilledHoles { .. }
                            ),
                            "sheet" => matches!(f.operation, cad_core::Operation::Sheet { .. }),
                            "weldment" => {
                                matches!(f.operation, cad_core::Operation::Weldment { .. })
                            }
                            _ => false,
                        }
                })
                .map(|f| choice(format!("{} / {}", p.name, f.name), p.reference(&f.id)))
        })
        .collect()
}

fn context_fields(f: &mut Fields<'_>, section: &str) {
    f.group("context", "Drawing");
    f.transient(
        "sheet",
        "Sheet",
        json!(f.sheet.id),
        f.w.session
            .visible()
            .drawings
            .iter()
            .map(|s| choice(&s.name, &s.id))
            .collect(),
    );
    f.transient(
        "section",
        "Controls",
        json!(section),
        choices(&[
            ("sheet", "Sheet"),
            ("views", "Views"),
            ("format", "Format and title"),
            ("dimensions", "Dimensions"),
            ("annotations", "Annotations"),
            ("tables", "Tables"),
        ]),
    );
    for (kind, label) in [
        ("drawing.sheet-add", "New sheet"),
        ("drawing.sheet-template-load", "From template"),
    ] {
        f.action(
            kind,
            label,
            json!({"kind":"existing","command":kind}),
            true,
            "Close Settings first",
        );
    }
}
fn sheet_fields(f: &mut Fields<'_>) {
    f.group("scope", "Model scope");
    f.text("/name", "Sheet name", false);
    let scopes =
        f.w.session
            .visible()
            .parts
            .values()
            .map(|p| choice(&p.name, &p.id))
            .chain(
                f.w.session
                    .visible()
                    .assemblies
                    .values()
                    .map(|a| choice(&a.name, &a.id)),
            )
            .collect();
    f.select("/definition", "Model", scopes);
    f.group("paper", "Paper");
    f.number("/paper_mm/0", "Paper width", "mm", Some(50.), Some(2000.));
    f.number("/paper_mm/1", "Paper height", "mm", Some(50.), Some(2000.));
    for (label, size) in [
        ("A4", [297., 210.]),
        ("A3", [420., 297.]),
        ("A2", [594., 420.]),
        ("Letter", [279.4, 215.9]),
        ("Ledger", [431.8, 279.4]),
    ] {
        f.action(
            label,
            label,
            json!({"kind":"paper","value":size}),
            true,
            "Close Settings first",
        );
    }
    f.action(
        "orientation",
        "Swap orientation",
        json!({"kind":"orientation"}),
        true,
        "Close Settings first",
    );
    let mut exploded = vec![choice("Assembled", Value::Null)];
    if f.sheet.definition == f.w.session.visible().root
        && let Some(a) = f.w.session.visible().assemblies.get(&f.sheet.definition)
    {
        exploded.extend(
            a.exploded_views
                .values()
                .filter(|v| {
                    v.configuration.is_none()
                        || v.configuration
                            == f.w
                                .session
                                .visible()
                                .active_configurations
                                .get(&f.sheet.definition)
                                .cloned()
                })
                .map(|v| choice(&v.name, &v.id)),
        );
    }
    f.group("projection", "Projection");
    f.select("/exploded_view", "Assembly state", exploded);
    let mut display_states = vec![choice("Normal display", Value::Null)];
    if let Some(assembly) = f.w.session.visible().assemblies.get(&f.sheet.definition) {
        display_states.extend(
            assembly
                .display_states
                .values()
                .filter(|state| {
                    state.configuration.is_none()
                        || state.configuration.as_ref()
                            == f.w
                                .session
                                .visible()
                                .active_configurations
                                .get(&assembly.id)
                })
                .map(|state| {
                    choice(
                        &state.name,
                        json!(cad_core::display_states::Selection {
                            assembly: assembly.id.clone(),
                            state: state.id.clone(),
                            owner_path: vec![],
                        }),
                    )
                }),
        );
    }
    if let Some(selected) = &f.sheet.display_state
        && !selected.owner_path.is_empty()
    {
        let name =
            f.w.session
                .visible()
                .assemblies
                .get(&selected.assembly)
                .and_then(|assembly| assembly.display_states.get(&selected.state))
                .map(|state| state.name.as_str())
                .unwrap_or("Unavailable nested state");
        display_states.push(choice(name, json!(selected)));
    }
    f.select("/display_state", "Display state", display_states);
    let views = view_keys(f.sheet);
    f.transient(
        "projection",
        "Views",
        json!(if views == ["front", "top", "right"] {
            "standard"
        } else if views == ["front", "top", "right", "isometric"] {
            "standard-isometric"
        } else if views == ["front"] {
            "front"
        } else {
            "custom"
        }),
        choices(&[
            ("standard", "Front / Top / Right"),
            ("standard-isometric", "Standard + Isometric"),
            ("front", "Front only"),
            ("custom", "Custom views"),
        ]),
    );
    f.note(
        "projection-convention",
        "Third-angle projection · Top above Front, Right beside Front",
        "ready",
    );
    f.group("scale", "Scale");
    let saved_scale = f
        .sheet
        .view_layouts
        .get("front")
        .and_then(|layout| layout.scale);
    let mut scale_choices = vec![choice("Automatic standard scale", Value::Null)];
    for scale in [0.1, 0.2, 0.25, 0.5, 1., 2., 5., 10.] {
        scale_choices.push(choice(scale_label(scale), scale));
    }
    if let Some(scale) = saved_scale
        && !scale_choices
            .iter()
            .any(|option| option["value"].as_f64() == Some(scale))
    {
        scale_choices.push(choice(format!("{} (saved)", scale_label(scale)), scale));
    }
    f.transient(
        "scale",
        "Shared view scale",
        json!(saved_scale),
        scale_choices,
    );
    f.group("sheet-actions", "Manage sheet");
    for (command, label) in [
        ("drawing.sheet-duplicate", "Duplicate"),
        ("drawing.sheet-up", "Move up"),
        ("drawing.sheet-down", "Move down"),
        ("drawing.sheet-delete", "Delete sheet"),
        ("drawing.sheet-template-save", "Save template"),
        ("drawing.sheet-format-load", "Load format"),
    ] {
        f.action(
            command,
            label,
            json!({"kind":"existing","command":command}),
            true,
            "Close Settings first",
        );
    }
    f.group("notes", "Sheet notes");
    for (i, _) in f.sheet.annotations.iter().enumerate() {
        let path = format!("/annotations/{i}");
        f.text(&path, &format!("Note {}", i + 1), true);
        let key = f.address(&path).0;
        f.action(
            &format!("{key}/remove"),
            "Remove note",
            json!({"kind":"remove","pointer":"/annotations","index":i}),
            true,
            "Close Settings first",
        );
    }
    f.action(
        "note/add",
        "Add sheet note",
        json!({"kind":"append","pointer":"/annotations","value":""}),
        f.sheet.annotations.len() < 4096,
        "Maximum 4096 sheet notes",
    );
    for key in f.sheet.title_block.keys() {
        f.text(
            &format!("/title_block/{}", escape(key)),
            &format!("Legacy title · {key}"),
            false,
        );
    }
}
fn view_fields(f: &mut Fields<'_>) {
    f.group("views", "Included views");
    let resolved = crate::drawing_ui::evaluation(f.w).and_then(|evaluation| {
        build_sheet_preview(f.w.session.visible(), evaluation, f.sheet, 0).ok()
    });
    let views = view_keys(f.sheet);
    f.note("projection-convention", if views.iter().any(|view| view == "front") {
        "Third-angle projection: Top above Front, Right to the right. Automatic projected views share a standard drawing scale; saved placements and parent relationships remain editable."
    } else {
        "Independent views use standard fit scales. Include Front for automatic Top/Right projected alignment and a shared scale, or set an editable parent view relationship."
    }, "ready");
    for key in VIEW_KEYS {
        let selected = views.iter().any(|v| v == key);
        let mut updated = views.clone();
        if selected {
            updated.retain(|v| v != key);
        } else {
            updated.push(key.into());
        }
        let mut sheet = f.sheet.clone();
        let available = set_views(&mut sheet, updated).is_ok();
        f.action(&format!("view/{key}/toggle"),key,json!({"kind":"view-toggle","view":key}),available,"Keep at least one view and move its dimensions, annotations and hole tables before removing it");
        if selected && let Some(e) = f.out.last_mut() {
            e.state = "selected".into();
        }
    }
    for (i, key) in views.iter().enumerate() {
        f.group(
            &format!("view-{key}"),
            &format!("{key} · placement and scale"),
        );
        if let Some(layout) = resolved.as_ref().and_then(|sheet| sheet.views.get(key))
            && let Some(scale) = layout.scale
        {
            f.note(
                &format!("view/{key}/resolved-scale"),
                &format!(
                    "Resolved scale {}{}",
                    scale_label(scale),
                    layout
                        .alignment
                        .as_ref()
                        .map(|alignment| format!(" · follows {}", alignment.parent))
                        .unwrap_or_default()
                ),
                "ready",
            );
        }
        f.action(
            &format!("view/{key}/up"),
            "Move up",
            json!({"kind":"view-order","view":key,"step":-1}),
            i > 0,
            "Already first",
        );
        f.action(
            &format!("view/{key}/down"),
            "Move down",
            json!({"kind":"view-order","view":key,"step":1}),
            i + 1 < views.len(),
            "Already last",
        );
        let path = format!("/view_layouts/{}", escape(key));
        f.optional(&path, "Saved paper placement", "view");
        if f.sheet.view_layouts.contains_key(key) {
            f.coordinates(
                &format!("{path}/rect_mm"),
                &["Paper X", "Paper Y", "View width", "View height"],
                "mm",
            );
            let shared_scale = f.sheet.view_layouts[key]
                .alignment
                .as_ref()
                .is_some_and(|alignment| alignment.shared_scale);
            if !shared_scale {
                f.optional(
                    &format!("{path}/scale"),
                    "Fixed model-to-paper scale",
                    "scale",
                );
            } else {
                f.note(
                    &format!("view/{key}/inherited-scale"),
                    "Model-to-paper scale follows the parent view",
                    "ready",
                );
            }
            let relationship = format!("{path}/alignment");
            f.optional(&relationship, "Parent view relationship", "alignment");
            if let Some(alignment) = &f.sheet.view_layouts[key].alignment {
                f.select(
                    &format!("{relationship}/parent"),
                    "Parent view",
                    views
                        .iter()
                        .filter(|parent| *parent != key)
                        .map(|parent| choice(parent, parent))
                        .collect(),
                );
                f.select(
                    &format!("{relationship}/axis"),
                    "Paper alignment",
                    choices(&[
                        ("free", "Free placement"),
                        ("horizontal", "Horizontal projected origin"),
                        ("vertical", "Vertical projected origin"),
                    ]),
                );
                f.toggle(&format!("{relationship}/shared_scale"), "Use parent scale");
                f.note(&format!("view/{key}/relationship-note"), &format!("Aligned to {}. Dragging preserves the saved relationship; disable it for independent placement.", alignment.parent), "ready");
            }
            if !shared_scale && f.sheet.view_layouts[key].scale.is_some() {
                f.number(
                    &format!("{path}/scale"),
                    "Scale (paper/model)",
                    "",
                    Some(1e-6),
                    Some(1000.),
                );
            }
        } else {
            f.note(
                &format!("view/{key}/automatic"),
                "Automatic standard scale and projected placement. Enable Saved paper placement to override.",
                "ready",
            );
        }
    }
}
fn format_fields(f: &mut Fields<'_>) {
    f.group("format", "Sheet format");
    f.optional("/format", "Editable sheet format", "format");
    let Some(format) = &f.sheet.format else {
        return;
    };
    f.toggle("/format/border", "Border");
    f.coordinates(
        "/format/margins_mm",
        &["Left margin", "Bottom margin", "Right margin", "Top margin"],
        "mm",
    );
    if format.border {
        f.optional("/format/zones", "Border zones", "zones");
    }
    if format.zones.is_some() {
        f.number(
            "/format/zones/columns",
            "Zone columns",
            "",
            Some(1.),
            Some(100.),
        );
        f.number("/format/zones/rows", "Zone rows", "", Some(1.), Some(26.));
        f.number(
            "/format/zones/band_mm",
            "Zone band",
            "mm",
            Some(3.),
            Some(50.),
        );
    }
    f.group("title-block", "Title block");
    f.optional("/format/title_block", "Title block", "title_block");
    let Some(block) = &format.title_block else {
        return;
    };
    f.number(
        "/format/title_block/size_mm/0",
        "Block width",
        "mm",
        Some(8.),
        Some(2000.),
    );
    f.number(
        "/format/title_block/size_mm/1",
        "Block height",
        "mm",
        Some(8.),
        Some(2000.),
    );
    f.number(
        "/format/title_block/text_size_mm",
        "Title text size",
        "mm",
        Some(0.5),
        Some(10.),
    );
    let selected =
        f.w.fields
            .get("drawing-item")
            .filter(|id| block.cells.iter().any(|c| &c.id == *id))
            .or_else(|| block.cells.first().map(|c| &c.id));
    f.transient(
        "item",
        "Title field",
        json!(selected),
        block
            .cells
            .iter()
            .map(|c| choice(&c.label, &c.id))
            .collect(),
    );
    f.action(
        "title/add",
        "Add title row",
        json!({"kind":"title-add"}),
        block.cells.len() < 128,
        "Maximum 128 title cells",
    );
    if let Some((index, cell)) = block
        .cells
        .iter()
        .enumerate()
        .find(|(_, c)| Some(&c.id) == selected)
    {
        let path = format!("/format/title_block/cells/{index}");
        f.group("title-field", "Title field");
        f.text(&format!("{path}/label"), "Label", false);
        let options = [
            LinkedText::Literal {
                text: String::new(),
            },
            LinkedText::DocumentName,
            LinkedText::DocumentProperty {
                key: "revision".into(),
            },
            LinkedText::ModelName,
            LinkedText::ModelProperty {
                key: "part_number".into(),
            },
            LinkedText::ConfigurationName,
            LinkedText::MaterialName,
            LinkedText::SheetName,
            LinkedText::SheetNumber,
            LinkedText::SourceRevision,
            LinkedText::SourceFingerprint,
        ]
        .into_iter()
        .map(|v| choice(v.label(), v))
        .collect();
        f.select(&format!("{path}/value"), "Linked value", options);
        match cell.value {
            LinkedText::Literal { .. } => f.text(&format!("{path}/value/text"), "Text", true),
            LinkedText::DocumentProperty { .. } | LinkedText::ModelProperty { .. } => {
                f.text(&format!("{path}/value/key"), "Native property key", false)
            }
            _ => {}
        }
        match linked_text(f.w.session.visible(), f.sheet, &cell.value) {
            Ok(value) => f.note(
                "title-current",
                &format!("Current value · {value}"),
                "ready",
            ),
            Err(error) => f.note("title-current", &error.to_string(), "error"),
        }
        f.coordinates(
            &format!("{path}/rect_mm"),
            &["Cell X", "Cell Y", "Cell width", "Cell height"],
            "mm",
        );
        f.action(
            &format!("title/{}/remove", cell.id),
            "Delete title field",
            json!({"kind":"remove","pointer":"/format/title_block/cells","index":index}),
            true,
            "Close Settings first",
        );
    }
}
fn dimension_fields(f: &mut Fields<'_>) {
    f.group("dimension-list", "Dimensions");
    let selected =
        f.w.fields
            .get("drawing-item")
            .filter(|id| f.sheet.dimensions.iter().any(|v| &v.id == *id))
            .or_else(|| f.sheet.dimensions.first().map(|v| &v.id));
    f.transient(
        "item",
        "Dimension",
        json!(selected),
        f.sheet
            .dimensions
            .iter()
            .enumerate()
            .map(|(i, d)| choice(format!("{} · {:?}", i + 1, d.options.kind), &d.id))
            .collect(),
    );
    for (kind, label) in [
        ("aligned", "Aligned"),
        ("horizontal", "Horizontal"),
        ("vertical", "Vertical"),
        ("perpendicular", "Perpendicular edge separation"),
        ("angular", "Angular"),
        ("diameter", "Diameter"),
        ("radius", "Radius"),
        ("hole_callout", "Hole callout"),
    ] {
        f.action(
            &format!("dimension/add/{kind}"),
            label,
            json!({"kind":"dimension-add","dimension":kind}),
            true,
            "Close Settings first",
        );
    }
    let Some((index, d)) = f
        .sheet
        .dimensions
        .iter()
        .enumerate()
        .find(|(_, d)| Some(&d.id) == selected)
    else {
        return;
    };
    let path = format!("/dimensions/{index}");
    f.group("dimension", "Dimension");
    f.view(&format!("{path}/view"));
    f.select(
        &format!("{path}/options/kind"),
        "Type",
        choices(&[
            ("aligned", "Aligned"),
            ("horizontal", "Horizontal"),
            ("vertical", "Vertical"),
            ("perpendicular", "Perpendicular edge separation"),
            ("angular", "Angular"),
            ("diameter", "Diameter"),
            ("radius", "Radius"),
            ("hole_callout", "Hole callout"),
        ]),
    );
    match d.options.kind {
        DimensionKind::Diameter | DimensionKind::Radius | DimensionKind::HoleCallout => {
            f.curve(&format!("{path}/options/curve"), true)
        }
        DimensionKind::Perpendicular | DimensionKind::Horizontal | DimensionKind::Vertical
            if d.options.second_curve.is_some()
                || d.options.kind == DimensionKind::Perpendicular =>
        {
            f.line(&format!("{path}/options/curve"), "First straight edge");
            f.line(
                &format!("{path}/options/second_curve"),
                "Second straight edge",
            );
            f.note("perpendicular-intent", "Associative shortest separation between parallel edges in this view, independent of where you clicked along either edge.", "ready");
        }
        _ => {
            f.point(
                &format!("{path}/first"),
                &format!("{path}/options/first"),
                if d.options.kind == DimensionKind::Angular {
                    "Angle vertex"
                } else {
                    "First point"
                },
            );
            f.point(
                &format!("{path}/second"),
                &format!("{path}/options/second"),
                if d.options.kind == DimensionKind::Angular {
                    "First ray"
                } else {
                    "Second point"
                },
            );
        }
    }
    if d.options.kind == DimensionKind::Angular {
        f.point(
            &format!("{path}/options/third_point"),
            &format!("{path}/options/third"),
            "Second ray",
        );
    }
    if d.options.kind == DimensionKind::HoleCallout {
        let mut holes = source_choices(f.w, "holes");
        holes.insert(0, choice("Choose designed hole", Value::Null));
        f.select(
            &format!("{path}/options/hole_feature"),
            "Hole feature",
            holes,
        );
    }
    f.group("dimension-format", "Dimension formatting");
    f.number(
        &format!("{path}/offset_mm"),
        "Paper offset",
        "mm",
        None,
        None,
    );
    f.number(
        &format!("{path}/options/precision"),
        "Decimals",
        "",
        Some(0.),
        Some(8.),
    );
    f.select(
        &format!("{path}/options/units"),
        "Units",
        choices(&[("millimeters", "Millimeters"), ("inches", "Inches")]),
    );
    f.toggle(
        &format!("{path}/options/reference"),
        "Reference dimension (parentheses)",
    );
    let unit = if d.options.kind == DimensionKind::Angular {
        "deg"
    } else if d.options.units == Units::Inches {
        "in"
    } else {
        "mm"
    };
    let options = vec![
        choice("None", Tolerance::None),
        choice(
            "Symmetric",
            Tolerance::Symmetric {
                value: cad_core::Expr(format!("0.1 {unit}")),
            },
        ),
        choice(
            "Deviation",
            Tolerance::Deviation {
                lower: cad_core::Expr(format!("-0.1 {unit}")),
                upper: cad_core::Expr(format!("0.1 {unit}")),
            },
        ),
        choice(
            "Limits",
            Tolerance::Limits {
                lower: cad_core::Expr(format!("9.9 {unit}")),
                upper: cad_core::Expr(format!("10.1 {unit}")),
            },
        ),
        choice("Basic", Tolerance::Basic),
    ];
    f.select(&format!("{path}/options/tolerance"), "Tolerance", options);
    match d.options.tolerance {
        Tolerance::Symmetric { .. } => f.field(
            &format!("{path}/options/tolerance/value"),
            "± tolerance",
            vec![],
            Some(unit),
        ),
        Tolerance::Deviation { .. } | Tolerance::Limits { .. } => {
            f.field(
                &format!("{path}/options/tolerance/lower"),
                "Lower",
                vec![],
                Some(unit),
            );
            f.field(
                &format!("{path}/options/tolerance/upper"),
                "Upper",
                vec![],
                Some(unit),
            );
        }
        _ => {}
    }
    f.text(&format!("{path}/options/prefix"), "Prefix", false);
    f.text(&format!("{path}/options/suffix"), "Suffix", false);
    f.text(
        &format!("{path}/tolerance"),
        "Additional tolerance note",
        false,
    );
    let mut params = vec![choice("Measure referenced geometry", Value::Null)];
    params.extend(f.w.session.visible().parts.values().flat_map(|p| {
        p.parameters.iter().map(|(id, param)| {
            choice(
                format!("{} / {}", p.name, param.name),
                cad_core::ParameterRef {
                    definition: p.id.clone(),
                    parameter: id.clone(),
                },
            )
        })
    }));
    f.select(&format!("{path}/parameter"), "Named parameter", params);
    f.action(
        &format!("dimension/{}/remove", d.id),
        "Delete dimension",
        json!({"kind":"remove","pointer":"/dimensions","index":index}),
        true,
        "Close Settings first",
    );
}

fn annotation_fields(f: &mut Fields<'_>) {
    f.group("annotation-list", "Annotations");
    let selected =
        f.w.fields
            .get("drawing-item")
            .filter(|id| f.sheet.entities.iter().any(|a| &a.id == *id))
            .or_else(|| f.sheet.entities.first().map(|a| &a.id));
    f.transient(
        "item",
        "Annotation",
        json!(selected),
        f.sheet
            .entities
            .iter()
            .enumerate()
            .map(|(i, a)| choice(format!("{} · {}", i + 1, a.kind.label()), &a.id))
            .collect(),
    );
    for (kind, label) in [
        ("leader", "Note"),
        ("datum", "Datum"),
        ("geometric_tolerance", "GD&T"),
        ("datum_target", "Datum target"),
        ("revision_cloud", "Revision cloud"),
        ("tolerance_frame", "Tolerance frame"),
        ("center_mark", "Center mark"),
        ("center_line", "Centerline"),
        ("dimension_group", "Dimension group"),
        ("balloon", "BOM balloon"),
        ("cut_list_balloon", "Cut-list balloon"),
        ("surface_finish", "Surface finish"),
        ("weld_symbol", "Weld symbol"),
    ] {
        f.action(
            &format!("annotation/add/{kind}"),
            label,
            json!({"kind":"annotation-add","annotation":kind}),
            kind != "center_mark" || crate::drawing_ui::selected_curve(f.w).is_some(),
            "Select a circular model edge first",
        );
    }
    let Some((index, a)) = f
        .sheet
        .entities
        .iter()
        .enumerate()
        .find(|(_, a)| Some(&a.id) == selected)
    else {
        return;
    };
    let path = format!("/entities/{index}");
    f.group("annotation", "Annotation");
    if matches!(a.kind, AnnotationKind::DimensionGroup { .. }) {
        f.select(
            &format!("{path}/view"),
            "Orthographic view",
            view_keys(f.sheet)
                .into_iter()
                .filter(|view| matches!(view.as_str(), "front" | "top" | "right"))
                .map(|view| choice(&view, &view))
                .collect(),
        );
    } else {
        f.view(&format!("{path}/view"));
    }
    match &a.kind {
        AnnotationKind::DatumTarget { symbol, .. } => {
            f.anchor(&format!("{path}/kind/target"), "Native target attachment");
            crate::drawing_gdt_ui::target_fields(f, &format!("{path}/kind/symbol"), symbol);
        }
        AnnotationKind::RevisionCloud { cloud, .. } => {
            f.anchor(&format!("{path}/kind/target"), "Cloud attachment");
            crate::drawing_revision_ui::fields(f, &format!("{path}/kind/cloud"), cloud);
        }
        AnnotationKind::GeometricToleranceFrame { frame, .. } => {
            f.anchor(&format!("{path}/kind/target"), "Native feature attachment");
            crate::drawing_gdt_ui::frame_fields(f, &format!("{path}/kind/frame"), frame);
        }
        AnnotationKind::DimensionGroup { group } => {
            crate::drawing_dimension_groups_ui::fields(f, &format!("{path}/kind/group"), group)
        }
        AnnotationKind::SurfaceFinish { .. } | AnnotationKind::WeldSymbol { .. } => {
            f.anchor(&format!("{path}/kind/target"), "Model target");
            f.group("symbol", a.kind.label());
            for mut element in symbols::semantic_fields(f.sheet, a, "draft") {
                let Some(field) = element
                    .data
                    .get("field")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                else {
                    continue;
                };
                let pointer = format!("{path}{field}");
                let metadata = f.data();
                if let (Some(existing), Some(additional)) =
                    (element.data.as_object_mut(), metadata.as_object())
                {
                    existing.extend(additional.clone());
                }
                element.data["setter"] = json!("drawing-editor");
                element.data["pointer"] = json!(pointer);
                element.data["address"] = f.address(&pointer).1;
                element.data["classes"] = json!(["drawing-field", "symbol"]);
                if f.w.settings.is_some() {
                    element.data["editable"] = json!(false);
                    element.state = "disabled".into();
                }
                if element
                    .data
                    .get("optional_enabled")
                    .and_then(Value::as_bool)
                    == Some(true)
                {
                    element.data["optional_factory"] = json!("symbol_default");
                }
                f.out.push(element);
            }
        }
        AnnotationKind::Leader { .. } => {
            f.anchor(&format!("{path}/kind/target"), "Leader target");
            f.text(&format!("{path}/kind/text"), "Text", true);
        }
        AnnotationKind::Datum { .. } => {
            f.anchor(&format!("{path}/kind/target"), "Datum target");
            f.text(
                &format!("{path}/kind/identifier"),
                "Datum identifier",
                false,
            );
        }
        AnnotationKind::GeometricTolerance { datums, .. } => {
            f.anchor(&format!("{path}/kind/target"), "Tolerance target");
            f.select(
                &format!("{path}/kind/characteristic"),
                "Characteristic",
                choices(&[
                    ("position", "Position"),
                    ("flatness", "Flatness"),
                    ("straightness", "Straightness"),
                    ("circularity", "Circularity"),
                    ("cylindricity", "Cylindricity"),
                    ("parallelism", "Parallelism"),
                    ("perpendicularity", "Perpendicularity"),
                    ("angularity", "Angularity"),
                    ("profile_line", "Profile of line"),
                    ("profile_surface", "Profile of surface"),
                    ("circular_runout", "Circular runout"),
                    ("total_runout", "Total runout"),
                ]),
            );
            f.field(
                &format!("{path}/kind/value"),
                "Tolerance zone",
                vec![],
                Some("mm"),
            );
            f.toggle(&format!("{path}/kind/diameter"), "Diametrical zone");
            f.select(
                &format!("{path}/kind/material"),
                "Material condition",
                choices(&[("regardless", "RFS"), ("maximum", "MMC"), ("least", "LMC")]),
            );
            f.emit(
                &format!("{path}/kind/datums"),
                "Datum sequence",
                json!(datums.join(" ")),
                vec![],
                None,
            );
            if let Some(e) = f.out.last_mut() {
                e.data["join_array"] = json!(true);
            }
        }
        AnnotationKind::CenterMark { .. } => {
            f.curve(&format!("{path}/kind/curve"), false);
            f.number(
                &format!("{path}/kind/extension_mm"),
                "Paper extension",
                "mm",
                Some(0.),
                Some(1000.),
            );
        }
        AnnotationKind::CenterLine { .. } => {
            f.anchor(&format!("{path}/kind/first"), "First point");
            f.anchor(&format!("{path}/kind/second"), "Second point");
            f.number(
                &format!("{path}/kind/extension_mm"),
                "Paper extension",
                "mm",
                Some(0.),
                Some(1000.),
            );
        }
        AnnotationKind::CutListBalloon {
            table,
            row_key,
            source,
            ..
        } => {
            f.anchor(&format!("{path}/kind/target"), "Stock-member target");
            f.select(
                &format!("{path}/kind/table"),
                "Cut-list table",
                f.sheet
                    .tables
                    .iter()
                    .filter(|table| matches!(table.kind, TableKind::WeldmentCutList { .. }))
                    .map(|table| choice(&table.title, &table.id))
                    .collect(),
            );
            let rows = f
                .sheet
                .tables
                .iter()
                .find(|item| &item.id == table)
                .and_then(|table| {
                    crate::drawing_ui::evaluation(f.w).and_then(|evaluation| {
                        content_for_sheet(f.w.session.visible(), evaluation, f.sheet, table).ok()
                    })
                });
            let options = rows
                .map(|content| {
                    content
                        .entries
                        .into_iter()
                        .flat_map(|row| {
                            let candidates = match &row.source {
                                RowSource::StockGroup { sources, .. } => sources.clone(),
                                _ => vec![],
                            };
                            candidates.into_iter().map(move |source| {
                                choice(
                                    format!(
                                        "{} · {}",
                                        row.number,
                                        source.occurrence_path.join("/")
                                    ),
                                    json!({"row_key":row.key,"source":source}),
                                )
                            })
                        })
                        .collect()
                })
                .unwrap_or_default();
            f.emit(
                &format!("{path}/kind"),
                "Stock row / occurrence",
                json!({"row_key":row_key,"source":source}),
                options,
                None,
            );
            let id = f.id(&format!("{}/stock-source", f.address(&path).0));
            if let Some(element) = f.out.last_mut() {
                element.id = id;
                element.data["compound"] = json!("cut_list_balloon_item");
            }
            f.toggle(&format!("{path}/kind/show_quantity"), "Show quantity");
        }
        AnnotationKind::Balloon {
            table,
            definition,
            configuration,
            row_key,
            ..
        } => {
            f.anchor(&format!("{path}/kind/target"), "Balloon target");
            f.select(
                &format!("{path}/kind/table"),
                "BOM table",
                f.sheet
                    .tables
                    .iter()
                    .filter(|t| matches!(t.kind, TableKind::BillOfMaterials))
                    .map(|t| choice(&t.title, &t.id))
                    .collect(),
            );
            let options=f.sheet.tables.iter().find(|t|&t.id==table).and_then(|table|crate::drawing_ui::evaluation(f.w).and_then(|e|content_for_sheet(f.w.session.visible(),e,f.sheet,table).ok())).map(|content|content.items.into_iter().map(|row|choice(format!("{} · {}",row.number,row.cells.get(1).map(String::as_str).unwrap_or("Component")),json!({"definition":row.definition,"configuration":row.configuration,"row_key":row.key}))).collect()).unwrap_or_default();
            f.emit(
                &format!("{path}/kind"),
                "Component / configuration",
                json!({"definition":definition,"configuration":configuration,"row_key":row_key}),
                options,
                None,
            );
            let id = f.id(&format!("{}/component", f.address(&path).0));
            if let Some(e) = f.out.last_mut() {
                e.id = id;
                e.data["compound"] = json!("balloon_item");
            }
            f.toggle(&format!("{path}/kind/show_quantity"), "Show quantity");
        }
    }
    if !matches!(
        a.kind,
        AnnotationKind::CenterMark { .. } | AnnotationKind::CenterLine { .. }
    ) {
        f.coordinates(
            &format!("{path}/offset_mm"),
            &["Paper offset X", "Paper offset Y"],
            "mm",
        );
    }
    if matches!(
        a.kind,
        AnnotationKind::Leader { .. }
            | AnnotationKind::Datum { .. }
            | AnnotationKind::Balloon { .. }
            | AnnotationKind::CutListBalloon { .. }
            | AnnotationKind::GeometricTolerance { .. }
            | AnnotationKind::DatumTarget { .. }
            | AnnotationKind::GeometricToleranceFrame { .. }
    ) {
        f.group("leader-elbows", "Leader elbows");
        for (elbow, _) in a.leader_points_mm.iter().enumerate() {
            f.coordinates(
                &format!("{path}/leader_points_mm/{elbow}"),
                &["Elbow X from target", "Elbow Y from target"],
                "mm",
            );
            f.action(
                &format!("annotation/{}/elbow/{elbow}/remove", a.id),
                "Remove elbow",
                json!({"kind":"remove","pointer":format!("{path}/leader_points_mm"),"index":elbow}),
                true,
                "Close Settings first",
            );
        }
        f.action(&format!("annotation/{}/elbow/add", a.id), "Add elbow",
            json!({"kind":"append","pointer":format!("{path}/leader_points_mm"),"value":[a.offset_mm[0] * 0.5, a.offset_mm[1] * 0.5]}),
            a.leader_points_mm.len() < 64, "A leader can have at most 64 elbows");
    }
    f.action(
        &format!("annotation/{}/remove", a.id),
        "Delete annotation",
        json!({"kind":"remove","pointer":"/entities","index":index}),
        true,
        "Close Settings first",
    );
}
fn table_fields(f: &mut Fields<'_>) {
    f.group("table-list", "Tables");
    let selected =
        f.w.fields
            .get("drawing-table")
            .or_else(|| f.w.fields.get("drawing-item"))
            .filter(|id| f.sheet.tables.iter().any(|t| &t.id == *id))
            .or_else(|| f.sheet.tables.first().map(|t| &t.id));
    f.transient(
        "item",
        "Table",
        json!(selected),
        f.sheet
            .tables
            .iter()
            .map(|t| choice(&t.title, &t.id))
            .collect(),
    );
    for (kind, label) in [
        ("bill_of_materials", "BOM"),
        ("revisions", "Revisions"),
        ("bends", "Bends"),
        ("general", "General"),
        ("holes", "Holes"),
        ("weldment_cut_list", "Cut list"),
    ] {
        f.action(
            &format!("table/add/{kind}"),
            label,
            json!({"kind":"table-add","table":kind}),
            true,
            "Close Settings first",
        );
    }
    let Some((index, table)) = f
        .sheet
        .tables
        .iter()
        .enumerate()
        .find(|(_, t)| Some(&t.id) == selected)
    else {
        return;
    };
    let path = format!("/tables/{index}");
    f.group("table", "Table");
    f.note("table-kind", table.kind.name(), "ready");
    f.text(&format!("{path}/title"), "Title", false);
    f.coordinates(
        &format!("{path}/position_mm"),
        &["Paper X", "Paper Y"],
        "mm",
    );
    f.toggle(
        &format!("{path}/reserve_space"),
        "Reserve space below views",
    );
    f.number(
        &format!("{path}/row_height_mm"),
        "Row height",
        "mm",
        Some(3.),
        Some(100.),
    );
    f.number(
        &format!("{path}/text_size_mm"),
        "Text size",
        "mm",
        Some(0.5),
        Some(20.),
    );
    f.group("table-columns", "Columns");
    for (i, _) in table.column_widths_mm.iter().enumerate() {
        f.number(
            &format!("{path}/column_widths_mm/{i}"),
            &format!("Column {} width", i + 1),
            "mm",
            Some(4.),
            Some(1000.),
        );
    }
    crate::drawing_table_ui::production_fields(f, table, &path);
    match &table.kind {
        TableKind::BillOfMaterials => source_rows(f, table, &path),
        TableKind::Revisions { rows } => {
            f.group("revision-rows", "Revision rows");
            let selected =
                f.w.fields
                    .get("drawing-row")
                    .filter(|id| rows.iter().any(|r| &r.id == *id))
                    .or_else(|| rows.first().map(|r| &r.id));
            f.transient(
                "row",
                "Revision",
                json!(selected),
                rows.iter().map(|r| choice(&r.revision, &r.id)).collect(),
            );
            f.action(
                &format!("table/{}/revision/add", table.id),
                "Add revision row",
                json!({"kind":"revision-add","table":table.id}),
                rows.len() < 4096,
                "Maximum 4096 revision rows",
            );
            if let Some((i, row)) = rows
                .iter()
                .enumerate()
                .find(|(_, r)| Some(&r.id) == selected)
            {
                let rowpath = format!("{path}/kind/rows/{i}");
                for (key, label, multi) in [
                    ("revision", "Revision", false),
                    ("description", "Description", true),
                    ("date", "Date", false),
                    ("approved_by", "Approved by", false),
                ] {
                    f.text(&format!("{rowpath}/{key}"), label, multi);
                }
                f.action(
                    &format!("table/{}/revision/{}/remove", table.id, row.id),
                    "Remove row",
                    json!({"kind":"remove","pointer":format!("{path}/kind/rows"),"index":i}),
                    !cad_core::drawing_revision::referenced(f.sheet, &table.id, Some(&row.id)),
                    "Reattach or unlink revision clouds before deleting their revision row",
                );
            }
        }
        TableKind::General { headers, rows } => {
            for (i, _) in headers.iter().enumerate() {
                f.text(
                    &format!("{path}/kind/headers/{i}"),
                    &format!("Column {} heading", i + 1),
                    false,
                );
            }
            f.action(
                &format!("table/{}/column/add", table.id),
                "Add column",
                json!({"kind":"general-column","table":table.id,"add":true}),
                headers.len() < 32,
                "Maximum 32 columns",
            );
            f.action(
                &format!("table/{}/column/remove", table.id),
                "Remove last column",
                json!({"kind":"general-column","table":table.id,"add":false}),
                headers.len() > 1,
                "Keep at least one column",
            );
            f.group("general-rows", "General rows");
            let collection = f.address(&format!("{path}/kind/rows")).0;
            let tokens = collection_tokens(f.w, &f.sheet.id, &collection, rows.len());
            let selected =
                f.w.fields
                    .get("drawing-row")
                    .and_then(|token| tokens.iter().position(|id| id == token))
                    .unwrap_or(0);
            f.transient(
                "row",
                "Row",
                tokens
                    .get(selected)
                    .map(|v| json!(v))
                    .unwrap_or(Value::Null),
                tokens
                    .iter()
                    .enumerate()
                    .map(|(i, token)| choice(format!("Row {}", i + 1), token))
                    .collect(),
            );
            f.action(
                &format!("table/{}/row/add", table.id),
                "Add row",
                json!({"kind":"general-row-add","table":table.id}),
                rows.len() < 100_000,
                "Maximum 100000 general rows",
            );
            if let Some(row) = rows.get(selected) {
                for (i, _) in row.iter().enumerate() {
                    f.text(
                        &format!("{path}/kind/rows/{selected}/{i}"),
                        headers.get(i).map(String::as_str).unwrap_or("Cell"),
                        false,
                    );
                }
                f.action(
                    &format!("table/{}/row/{}/remove", table.id, tokens[selected]),
                    "Remove row",
                    json!({"kind":"remove","pointer":format!("{path}/kind/rows"),"index":selected}),
                    true,
                    "Close Settings first",
                );
            }
        }
        TableKind::Bends { source, .. } => {
            f.group("bend-source", "Bend source");
            f.select(
                &format!("{path}/kind/source"),
                "Sheet feature",
                source_choices(f.w, "sheet"),
            );
            f.select(
                &format!("{path}/kind/configuration"),
                "Sheet configuration",
                configuration_choices(f.w, &source.definition),
            );
        }
        TableKind::Holes {
            features,
            configuration,
            instance_path,
            view,
            datum,
            ..
        } => {
            f.group("hole-source", "Hole sources");
            let owner = features
                .first()
                .map(|s| s.definition.as_str())
                .unwrap_or("");
            let candidates = crate::drawing_table_ui::hole_candidates(f.w, f.sheet);
            let part_choices = candidates
                .iter()
                .map(|(s, _)| s.definition.clone())
                .collect::<std::collections::BTreeSet<_>>()
                .into_iter()
                .filter_map(|id| {
                    f.w.session
                        .visible()
                        .parts
                        .get(&id)
                        .map(|p| choice(&p.name, id.clone()))
                })
                .collect();
            f.emit(
                &format!("{path}/kind/features"),
                "Hole model",
                json!(owner),
                part_choices,
                None,
            );
            let id = f.id(&format!("table/{}/hole-model", table.id));
            if let Some(e) = f.out.last_mut() {
                e.id = id;
                e.data["compound"] = json!("hole_model");
            }
            for (source, label) in candidates.iter().filter(|(s, _)| s.definition == owner) {
                let selected = features.contains(source);
                f.action(&format!("table/{}/hole/{}/toggle",table.id,source.node),label,json!({"kind":"source-toggle","pointer":format!("{path}/kind/features"),"value":source,"allow_empty":false}),!selected||features.len()>1,"Keep at least one hole feature");
                if selected && let Some(e) = f.out.last_mut() {
                    e.state = "selected".into();
                }
            }
            f.select(
                &format!("{path}/kind/configuration"),
                "Hole configuration",
                configuration_choices(f.w, owner),
            );
            f.select(
                &format!("{path}/kind/instance_path"),
                "Hole occurrence",
                occurrence_choices(f.w, owner),
            );
            f.select(
                &format!("{path}/kind/view"),
                "Coordinate view",
                view_keys(f.sheet)
                    .iter()
                    .filter(|v| matches!(v.as_str(), "front" | "top" | "right"))
                    .map(|v| choice(v, v))
                    .collect(),
            );
            let mut datum_choices = vec![choice("Part origin", HoleDatum::ModelOrigin)];
            datum_choices.extend(
                crate::drawing_ui::reference_choices(f.w)
                    .into_iter()
                    .filter(|(r, _)| match r {
                        PointReference::SketchPoint { sketch, .. } => sketch.definition == owner,
                        _ => true,
                    })
                    .map(|(r, label)| choice(label, HoleDatum::Reference { point: Box::new(r) })),
            );
            if let Some(r) = crate::drawing_ui::selected_anchor(f.w).reference {
                datum_choices.push(choice(
                    "Selected model datum",
                    HoleDatum::Reference { point: Box::new(r) },
                ));
            }
            f.select(
                &format!("{path}/kind/datum"),
                "Coordinate datum",
                datum_choices,
            );
            f.text(&format!("{path}/kind/tag_prefix"), "Tag prefix", false);
            table_units(f, &path);
            let _ = (configuration, instance_path, view, datum);
            source_rows(f, table, &path);
        }
        TableKind::WeldmentCutList { scope, sources, .. } => {
            f.group("cut-list-source", "Cut-list sources");
            let mut scopes = crate::drawing_table_ui::represented_parts(f.w, f.sheet);
            scopes.push(f.sheet.definition.clone());
            scopes.sort();
            scopes.dedup();
            f.select(
                &format!("{path}/kind/scope"),
                "Model scope",
                scopes
                    .iter()
                    .map(|id| {
                        choice(
                            cad_core::presentation::definition_name(f.w.session.visible(), id),
                            id,
                        )
                    })
                    .collect(),
            );
            f.select(
                &format!("{path}/kind/configuration"),
                "Configuration",
                configuration_choices(f.w, scope),
            );
            let represented = crate::drawing_table_ui::represented_parts(f.w, f.sheet);
            let candidates = source_choices(f.w, "weldment")
                .into_iter()
                .filter(|c| {
                    c["value"]["definition"].as_str().is_some_and(|id| {
                        represented.iter().any(|p| p == id)
                            && (id == scope || scope == &f.sheet.definition)
                    })
                })
                .collect::<Vec<_>>();
            let candidate_values = candidates
                .iter()
                .map(|c| c["value"].clone())
                .collect::<Vec<_>>();
            f.action(
                &format!("table/{}/all", table.id),
                "All weldment outputs",
                json!({"kind":"set","pointer":format!("{path}/kind/sources"),"value":[]}),
                !sources.is_empty(),
                "All outputs are already included",
            );
            for candidate in candidates {
                let source = candidate["value"].clone();
                let selected = sources.is_empty() || sources.iter().any(|s| json!(s) == source);
                f.action(&format!("table/{}/source/{}/toggle",table.id,source["node"]),candidate["label"].as_str().unwrap_or("Weldment"),json!({"kind":"source-toggle","pointer":format!("{path}/kind/sources"),"value":source,"allow_empty":true,"choices":candidate_values}),true,"Close Settings first");
                if selected && let Some(e) = f.out.last_mut() {
                    e.state = "selected".into();
                }
            }
            table_units(f, &path);
            source_rows(f, table, &path);
        }
    }
    f.action(
        &format!("table/{}/remove", table.id),
        "Delete table",
        json!({"kind":"table-delete","table":table.id}),
        !cad_core::drawing_revision::referenced(f.sheet, &table.id, None),
        "Reattach or unlink revision clouds before deleting their revision table",
    );
}
fn table_units(f: &mut Fields<'_>, path: &str) {
    f.select(
        &format!("{path}/kind/units"),
        "Units",
        choices(&[("millimeters", "Millimeters"), ("inches", "Inches")]),
    );
    f.number(
        &format!("{path}/kind/precision"),
        "Decimals",
        "",
        Some(0.),
        Some(8.),
    );
}
fn source_rows(f: &mut Fields<'_>, table: &Table, path: &str) {
    f.group("source-rows", "Source rows");
    let content = crate::drawing_ui::evaluation(f.w)
        .map(|e| content_for_sheet(f.w.session.visible(), e, f.sheet, table));
    match content {
        Some(Ok(content)) => {
            let rows = if matches!(table.kind, TableKind::BillOfMaterials) {
                content
                    .items
                    .into_iter()
                    .map(|row| (row.key.clone(), row.cells, row.number))
                    .collect::<Vec<_>>()
            } else {
                content
                    .entries
                    .into_iter()
                    .map(|row| (row.key, row.cells, row.number))
                    .collect()
            };
            let selected =
                f.w.fields
                    .get("drawing-row")
                    .filter(|key| rows.iter().any(|(id, _, _)| id == *key))
                    .or_else(|| rows.first().map(|(id, _, _)| id));
            f.transient(
                "row",
                "Source row",
                json!(selected),
                rows.iter()
                    .map(|(key, cells, number)| {
                        choice(
                            format!(
                                "{} · {}",
                                number,
                                cells.get(1).map(String::as_str).unwrap_or("Item")
                            ),
                            key,
                        )
                    })
                    .collect(),
            );
            if let Some((key, cells, number)) = rows.iter().find(|(id, _, _)| Some(id) == selected)
            {
                let field_path = format!("{path}/item_numbers/{}", escape(key));
                f.emit(&field_path, "Item number", json!(number), vec![], Some(""));
                if let Some(e) = f.out.last_mut() {
                    e.data["minimum"] = json!(1);
                    e.data["maximum"] = json!(u32::MAX);
                    e.data["default_json"] = json!(number);
                    e.data["source_number"] = json!({"table":table.id,"row":key});
                }
                f.note("source-row-summary", &cells.join(" · "), "ready");
                if let Some(index) = rows.iter().position(|(id, _, _)| id == key) {
                    for (step, label) in [(-1, "Move row up"), (1, "Move row down")] {
                        let neighbor = (index as i64 + step)
                            .try_into()
                            .ok()
                            .filter(|i: &usize| *i < rows.len());
                        f.action(&format!("table/{}/{}/order/{step}",table.id,escape(key)),label,json!({"kind":"row-order","table":table.id,"row":key,"neighbor":neighbor.map(|i|rows[i].0.clone())}),neighbor.is_some()&&!matches!(table.kind,TableKind::BillOfMaterials),"This row cannot move farther or uses BOM order");
                    }
                }
            }
            f.action(
                &format!("table/{}/refresh", table.id),
                "Refresh source reservations",
                json!({"kind":"table-refresh","table":table.id}),
                true,
                "Wait for model evaluation",
            );
            f.note(
                "source-reservations",
                "Absent source rows retain their saved numbers and order.",
                "ready",
            );
        }
        Some(Err(error)) => {
            f.note("source-rows-error", &error.to_string(), "error");
            saved_row_fields(f, table, path);
        }
        None => f.note(
            "source-rows-pending",
            "Wait for the model evaluation to inspect native source rows.",
            "pending",
        ),
    }
}

fn saved_row_fields(f: &mut Fields<'_>, table: &Table, path: &str) {
    let selected =
        f.w.fields
            .get("drawing-row")
            .filter(|key| table.item_numbers.contains_key(*key))
            .or_else(|| table.item_numbers.keys().next());
    f.transient(
        "row",
        "Saved source row",
        json!(selected),
        table
            .item_numbers
            .keys()
            .enumerate()
            .map(|(index, key)| choice(format!("Saved row {}", index + 1), key))
            .collect(),
    );
    if let Some(key) = selected {
        f.number(
            &format!("{path}/item_numbers/{}", escape(key)),
            "Item number",
            "",
            Some(1.),
            Some(u32::MAX as f64),
        );
    }
}

pub fn semantics(w: &Workspace) -> Vec<SemanticElement> {
    if !crate::drawing_ui::is_workspace(w) {
        return vec![];
    }
    let native = crate::drawing_ui::evaluation(w);
    let key = json!({
        "identity": crate::source_cache::identity(w),
        "fields": w.fields.iter().filter(|(key, _)| key.starts_with("drawing-")).collect::<std::collections::BTreeMap<_, _>>(),
        "selection": w.selected.as_ref().map(|selection| json!({"instance":selection.instance,"path":selection.path,"definition":selection.definition,"reference":selection.reference,"body":selection.body,"point":selection.point})),
        "settings": w.settings.is_some(),
        "checkpoint": w.tool_checkpoint.is_some(),
        "confirmation": crate::cad_ui::confirmation_available(w),
        "native": native.map(|e| (&e.identity, e.valid, e.geometry_valid)),
    });
    if let Ok(cache) = w.drawing_editor_cache.lock()
        && cache.key.as_ref() == Some(&key)
    {
        return cache.elements.clone();
    }
    let elements = build_semantics(w);
    if let Ok(mut cache) = w.drawing_editor_cache.lock() {
        cache.key = Some(key);
        cache.elements = elements.clone();
    }
    elements
}

fn build_semantics(w: &Workspace) -> Vec<SemanticElement> {
    if !crate::drawing_ui::is_workspace(w) {
        return vec![];
    }
    let Some(sheet) = active_sheet(w) else {
        return empty_semantics(w);
    };
    let current_section = section(w, &sheet);
    let mut fields = Fields::new(w, &sheet);
    context_fields(&mut fields, &current_section);
    match current_section.as_str() {
        "views" => view_fields(&mut fields),
        "format" => format_fields(&mut fields),
        "dimensions" => dimension_fields(&mut fields),
        "annotations" => annotation_fields(&mut fields),
        "tables" => table_fields(&mut fields),
        _ => sheet_fields(&mut fields),
    }
    if !current_source_is_valid(w) {
        fields.group("validation", "Drawing validation");
        fields.note("source-invalid", "Correct the staged drawing fields before applying. Hidden invalid fields also remain pending.", "error");
    }
    fields.group("confirmation", "Preview and save");
    for (command, label, enabled) in [
        (
            "draft.apply",
            "Apply drawing",
            crate::cad_ui::confirmation_available(w),
        ),
        ("draft.cancel", "Cancel", w.tool_checkpoint.is_some()),
        (
            "file.export-pdf",
            "Export PDF",
            w.session.draft.is_none()
                && current_source_is_valid(w)
                && crate::drawing_ui::evaluation(w).is_some_and(|e| e.valid),
        ),
        (
            "file.export-drawing-dxf",
            "Export DXF",
            w.session.draft.is_none()
                && current_source_is_valid(w)
                && crate::drawing_ui::evaluation(w).is_some_and(|e| e.valid),
        ),
    ] {
        fields.action(
            command,
            label,
            json!({"kind":"confirmation","command":command}),
            enabled,
            "A current validated drawing is required",
        );
    }
    for element in &mut fields.out {
        if let Some(error) = field_message(w, element) {
            element.state = "error".into();
            element.data["diagnostics"] = json!([error]);
        }
    }
    fields.out
}
fn error_prefix(w: &Workspace) -> Option<String> {
    Some(format!(
        "drawing-control:invalid:{}:{}:",
        w.session.visible().id,
        w.fields.get(GENERATION)?
    ))
}
fn error_key(element: &SemanticElement) -> String {
    format!(
        "drawing-control:invalid:{}:{}:{}:{}",
        element
            .data
            .get("document")
            .and_then(Value::as_str)
            .unwrap_or(""),
        element
            .data
            .get("editor")
            .and_then(Value::as_str)
            .unwrap_or(""),
        element
            .data
            .get("sheet_id")
            .and_then(Value::as_str)
            .unwrap_or(""),
        element.id
    )
}
fn field_message(w: &Workspace, element: &SemanticElement) -> Option<String> {
    let value: Value = serde_json::from_str(w.fields.get(&error_key(element))?).ok()?;
    value.get("message")?.as_str().map(str::to_owned)
}
pub fn field_is_invalid(w: &Workspace, element: &SemanticElement) -> bool {
    element.data.get("setter").and_then(Value::as_str) == Some("drawing-editor")
        && field_message(w, element).is_some()
}
pub fn field_error(w: &mut Workspace, element: &SemanticElement, error: Option<String>) {
    if element.data.get("setter").and_then(Value::as_str) != Some("drawing-editor") {
        return;
    }
    let key = error_key(element);
    if let Some(message) = error {
        w.fields.insert(key,json!({"message":message,"sheet":element.data["sheet_id"],"address":element.data["address"],"transient":element.data["transient"],"source_number":element.data["source_number"]}).to_string());
        w.confirm_requested = false;
    } else {
        w.fields.remove(&key);
    }
}
fn resolve_address<'a>(
    w: &Workspace,
    sheet: &str,
    source: &'a Value,
    address: &Value,
) -> Option<&'a Value> {
    let mut value = source;
    for segment in address.as_array()? {
        value = if let Some(key) = segment.get("key").and_then(Value::as_str) {
            value.get(key)?
        } else if let Some(id) = segment.get("id").and_then(Value::as_str) {
            value
                .as_array()?
                .iter()
                .find(|v| v.get("id").and_then(Value::as_str) == Some(id))?
        } else if let Some(token) = segment.get("slot").and_then(Value::as_str) {
            let array = value.as_array()?;
            let collection = segment.get("collection")?.as_str()?;
            let index = collection_tokens(w, sheet, collection, array.len())
                .iter()
                .position(|id| id == token)?;
            array.get(index)?
        } else {
            value
                .as_array()?
                .get(segment.get("index")?.as_u64()? as usize)?
        };
    }
    Some(value)
}
pub fn current_source_is_valid(w: &Workspace) -> bool {
    if !crate::drawing_ui::current_source_is_valid(w) {
        return false;
    }
    let Some(prefix) = error_prefix(w) else {
        return true;
    };
    !w.fields
        .iter()
        .filter(|(key, _)| key.starts_with(&prefix))
        .any(|(_, record)| {
            let Ok(record) = serde_json::from_str::<Value>(record) else {
                return true;
            };
            if !record["transient"].is_null() {
                return true;
            }
            let Some(sheet) = record.get("sheet").and_then(Value::as_str) else {
                return true;
            };
            let Some(staged) = crate::drawing_ui::sheet_source(w, sheet)
                .filter(|_| w.session.visible().drawings.iter().any(|s| s.id == sheet))
            else {
                return false;
            };
            if let (Some(table), Some(row)) = (
                record["source_number"]["table"].as_str(),
                record["source_number"]["row"].as_str(),
            ) {
                let Some(table) = staged.tables.iter().find(|t| t.id == table) else {
                    return false;
                };
                if table.item_numbers.contains_key(row) {
                    return true;
                }
                return crate::drawing_ui::evaluation(w)
                    .and_then(|e| content_for_sheet(w.session.visible(), e, &staged, table).ok())
                    .is_none_or(|content| {
                        content.entries.iter().any(|entry| entry.key == row)
                            || content.items.iter().any(|item| item.key == row)
                    });
            }
            let source = json!(staged);
            resolve_address(w, sheet, &source, &record["address"]).is_some()
        })
}
pub fn reset(w: &mut Workspace) {
    w.fields.remove(GENERATION);
    w.fields.retain(|key, _| {
        !key.starts_with("drawing-control:invalid:")
            && !key.starts_with("drawing-control:collection:")
    });
}
fn empty_semantics(w: &Workspace) -> Vec<SemanticElement> {
    let mut data = json!({"contract":CONTRACT,"version":VERSION,"document":w.session.visible().id,"editor":w.fields.get(GENERATION),"group":"empty"});
    let mut elements = vec![SemanticElement {
        id: "drawing-editor:empty".into(),
        role: "drawing-editor-group".into(),
        label: "New drawing".into(),
        value: None,
        state: "ready".into(),
        commands: vec![],
        relationships: vec![],
        data: data.clone(),
    }];
    for (command, label) in [
        ("drawing.sheet-add", "New sheet"),
        ("drawing.sheet-template-load", "From template"),
    ] {
        let enabled = w.settings.is_none();
        data["command"] = json!(command);
        data["enabled"] = json!(enabled);
        data["actions"] = json!([{"command":command,"label":label,"enabled":enabled}]);
        elements.push(SemanticElement {
            id: format!("drawing-editor:{command}"),
            role: "drawing-editor-action".into(),
            label: label.into(),
            value: None,
            state: if enabled { "ready" } else { "disabled" }.into(),
            commands: vec![command.into()],
            relationships: vec![],
            data: data.clone(),
        });
    }
    elements
}
pub fn show(ui: &mut egui::Ui, w: &mut Workspace) {
    if !w.fields.contains_key(GENERATION) {
        w.fields.insert(GENERATION.into(), cad_core::id());
    }
    let properties = semantics(w)
        .into_iter()
        .filter(|element| element.data["group"] != "confirmation")
        .collect();
    crate::operation_editor_ui::render_elements(ui, w, properties);
}

fn live(w: &Workspace, element: &SemanticElement) -> Result<SemanticElement> {
    let fresh = build_semantics(w)
        .into_iter()
        .find(|e| e.id == element.id)
        .ok_or_else(|| invalid("This drawing control is no longer available"))?;
    for key in ["document", "editor", "sheet_id"] {
        if element.data.get(key) != fresh.data.get(key) {
            return Err(invalid(
                "The drawing editor or sheet changed; use the current controls",
            ));
        }
    }
    if w.settings.is_some() {
        return Err(invalid("Close Settings before editing drawings"));
    }
    Ok(fresh)
}
pub fn current_value(w: &Workspace, element: &SemanticElement) -> Option<Value> {
    let sheet = active_sheet_id(w)?;
    if !crate::drawing_ui::is_workspace(w)
        || w.settings.is_some()
        || element.data["contract"] != CONTRACT
        || element.data["setter"] != "drawing-editor"
        || element.data["document"] != w.session.visible().id
        || element.data["editor"] != json!(w.fields.get(GENERATION))
        || element.data["sheet_id"] != sheet
    {
        return None;
    }
    if element.data.get("transient").is_some() {
        return semantics(w)
            .into_iter()
            .find(|current| current.id == element.id)
            .map(|current| current.data["json"].clone());
    }
    let identity = crate::source_cache::identity(w);
    let text = w.fields.get(&format!("drawing-editor:{sheet}"));
    let mut cache = w.drawing_editor_cache.lock().ok()?;
    if !cache.source.as_ref().is_some_and(|source| {
        source.identity == identity && source.sheet == sheet && source.text.as_ref() == text
    }) {
        let value = if let Some(text) = text {
            serde_json::from_str::<Value>(text).ok()?
        } else {
            json!(
                w.session
                    .visible()
                    .drawings
                    .iter()
                    .find(|source| source.id == sheet)?
            )
        };
        cache.source = Some(SourceValue {
            identity,
            sheet: sheet.into(),
            text: text.cloned(),
            value,
        });
    }
    // Read stable object/slot addresses directly from one current source image.
    // Actual edits still regenerate the projection through `live` before dispatch.
    let source = &cache.source.as_ref()?.value;
    let value = resolve_address(w, sheet, source, element.data.get("address")?)
        .cloned()
        .unwrap_or_else(|| {
            element
                .data
                .get("default_json")
                .cloned()
                .unwrap_or(Value::Null)
        });
    if element.data.get("optional_factory").is_some() {
        Some(json!(!value.is_null()))
    } else if element.data.get("join_array").and_then(Value::as_bool) == Some(true) {
        Some(json!(
            value
                .as_array()?
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(" ")
        ))
    } else if element.data.get("compound").and_then(Value::as_str) == Some("balloon_item") {
        Some(
            json!({"definition":value["definition"],"configuration":value["configuration"],"row_key":value["row_key"]}),
        )
    } else if element.data.get("compound").and_then(Value::as_str) == Some("cut_list_balloon_item")
    {
        Some(json!({"row_key":value["row_key"],"source":value["source"]}))
    } else if element.data.get("compound").and_then(Value::as_str) == Some("hole_model") {
        Some(value.as_array()?.first()?.get("definition")?.clone())
    } else {
        Some(value)
    }
}

fn set_at(source: &mut Value, path: &str, value: Value) -> Result<()> {
    let (parent, key) = path
        .rsplit_once('/')
        .ok_or_else(|| invalid("Invalid drawing field address"))?;
    if let Some(field) = source.pointer_mut(path) {
        *field = value;
        return Ok(());
    }
    if source.pointer(parent).is_none() && !parent.is_empty() {
        set_at(source, parent, json!({}))?;
    }
    let object = if parent.is_empty() {
        source.as_object_mut()
    } else {
        source.pointer_mut(parent).and_then(Value::as_object_mut)
    }
    .ok_or_else(|| invalid("The drawing field parent is no longer available"))?;
    object.insert(unescape(key), value);
    Ok(())
}
fn default_view(w: &Workspace, sheet: &Drawing, key: &str) -> ViewLayout {
    if let Some(layout) = crate::drawing_ui::evaluation(w)
        .and_then(|e| build_sheet(w.session.visible(), e, sheet).ok())
        .and_then(|s| s.views.get(key).cloned())
    {
        return ViewLayout {
            scale: None,
            ..layout
        };
    }
    let bounds = paper_bounds(sheet);
    let height = ((bounds[1][1] - bounds[0][1]) / 4.).max(8.);
    ViewLayout {
        rect_mm: [
            bounds[0][0] + 4.,
            bounds[1][1] - height - 7.,
            (bounds[1][0] - bounds[0][0] - 8.).max(8.),
            height,
        ],
        scale: None,
        alignment: None,
    }
}
fn optional_default(
    w: &Workspace,
    sheet: &Drawing,
    element: &SemanticElement,
    path: &str,
) -> Result<Value> {
    Ok(
        match element
            .data
            .get("optional_factory")
            .and_then(Value::as_str)
            .unwrap_or("")
        {
            "format" => json!(SheetFormat::default()),
            "zones" => json!(Zones {
                columns: 4,
                rows: 4,
                band_mm: 5.
            }),
            "title_block" => json!(SheetFormat::default().title_block),
            "scale" => {
                let child = unescape(path.split('/').nth(2).unwrap_or("front"));
                json!(
                    crate::drawing_ui::evaluation(w)
                        .and_then(|evaluation| build_sheet_preview(
                            w.session.visible(),
                            evaluation,
                            sheet,
                            0
                        )
                        .ok())
                        .and_then(|paper| paper.views.get(&child).and_then(|layout| layout.scale))
                        .unwrap_or(1.)
                )
            }
            "alignment" => {
                let child = unescape(path.split('/').nth(2).unwrap_or("front"));
                let parent = view_keys(sheet)
                    .into_iter()
                    .find(|key| key != &child)
                    .ok_or_else(|| {
                        invalid("Add another view before creating a parent relationship")
                    })?;
                let axis = match (child.as_str(), parent.as_str()) {
                    ("top", "front") | ("front", "top") => ViewAlignmentAxis::Vertical,
                    ("right", "front") | ("front", "right") => ViewAlignmentAxis::Horizontal,
                    _ => ViewAlignmentAxis::Free,
                };
                json!(ViewAlignment {
                    parent,
                    axis,
                    shared_scale: true
                })
            }
            "view" => json!(default_view(
                w,
                sheet,
                &unescape(path.rsplit('/').next().unwrap_or("front"))
            )),
            "symbol_default" => element
                .data
                .get("default")
                .cloned()
                .ok_or_else(|| invalid("Missing drawing symbol option default"))?,
            _ => return Err(invalid("Unsupported drawing option")),
        },
    )
}
fn publish(w: &mut Workspace, sheet: Drawing) {
    crate::drawing_ui::put(w, sheet);
}
pub fn set_value(w: &mut Workspace, element: &SemanticElement, value: Value) -> Result<()> {
    let result = set_field(w, element, value);
    field_error(w, element, result.as_ref().err().map(ToString::to_string));
    result
}
fn set_field(w: &mut Workspace, element: &SemanticElement, value: Value) -> Result<()> {
    let element = live(w, element)?;
    let descriptor = element.value_descriptor();
    if !descriptor.editable {
        return Err(invalid("This drawing control is read-only"));
    }
    if !descriptor.choices.is_empty()
        && !descriptor
            .choices
            .iter()
            .any(|c| c.get("value").unwrap_or(c) == &value)
    {
        return Err(invalid(
            "Choose one of the current drawing references or options",
        ));
    }
    if let Some(key) = element.data.get("transient").and_then(Value::as_str) {
        match key {
            "sheet" => {
                let id = value
                    .as_str()
                    .ok_or_else(|| invalid("Expected a drawing sheet"))?;
                crate::drawing_ui::select_sheet(w, id);
            }
            "section" => {
                w.fields.insert(
                    "drawing-section".into(),
                    value
                        .as_str()
                        .ok_or_else(|| invalid("Expected a drawing section"))?
                        .into(),
                );
            }
            "item" => {
                let id = value
                    .as_str()
                    .ok_or_else(|| invalid("Expected a drawing item"))?;
                w.fields.insert("drawing-item".into(), id.into());
                if let Some(sheet) = active_sheet(w)
                    && section(w, &sheet) == "tables"
                {
                    w.fields.insert("drawing-table".into(), id.into());
                }
            }
            "cloud-vertex" => {
                w.fields.insert(
                    "drawing-cloud-vertex".into(),
                    value
                        .as_str()
                        .ok_or_else(|| invalid("Choose a cloud vertex"))?
                        .into(),
                );
            }
            "gdt-row" => {
                w.fields.insert(
                    "drawing-gdt-row".into(),
                    value
                        .as_str()
                        .ok_or_else(|| invalid("Choose a frame segment"))?
                        .into(),
                );
            }
            "group-target" => {
                w.fields.insert(
                    "drawing-group-target".into(),
                    value
                        .as_str()
                        .ok_or_else(|| invalid("Choose a dimension group target"))?
                        .into(),
                );
            }
            "row" => {
                w.fields.insert(
                    "drawing-row".into(),
                    value
                        .as_str()
                        .ok_or_else(|| invalid("Expected a drawing row"))?
                        .into(),
                );
            }
            "projection" => {
                let mut sheet = active_sheet(w).ok_or_else(|| invalid("Select a sheet"))?;
                let views = match value.as_str().unwrap_or("") {
                    "standard" => vec!["front".into(), "top".into(), "right".into()],
                    "standard-isometric" => vec![
                        "front".into(),
                        "top".into(),
                        "right".into(),
                        "isometric".into(),
                    ],
                    "front" => vec!["front".into()],
                    "custom" => {
                        w.fields.insert("drawing-section".into(), "views".into());
                        return Ok(());
                    }
                    _ => return Err(invalid("Choose current projection views")),
                };
                set_views(&mut sheet, views)?;
                publish(w, sheet);
            }
            "scale" => {
                let mut sheet = active_sheet(w).ok_or_else(|| invalid("Select a sheet"))?;
                let evaluation = crate::drawing_ui::evaluation(w).ok_or_else(|| {
                    invalid("Wait for current model geometry before changing scale")
                })?;
                let resolved = build_sheet_preview(w.session.visible(), evaluation, &sheet, 0)?;
                for key in view_keys(&sheet) {
                    let mut layout = sheet
                        .view_layouts
                        .get(&key)
                        .cloned()
                        .or_else(|| resolved.views.get(&key).cloned())
                        .ok_or_else(|| invalid("Drawing view layout is unavailable"))?;
                    if value.is_null() {
                        layout.scale = None;
                    } else {
                        layout.scale = Some(
                            value
                                .as_f64()
                                .filter(|n| n.is_finite() && *n > 0.)
                                .ok_or_else(|| invalid("Enter a positive drawing scale"))?,
                        );
                    }
                    sheet.view_layouts.insert(key, layout);
                }
                publish(w, sheet);
            }
            "preview" => {
                if value
                    .as_bool()
                    .ok_or_else(|| invalid("Expected sheet preview toggle"))?
                {
                    w.fields.insert(
                        "drawing-preview".into(),
                        element.data["sheet_id"].as_str().unwrap_or("").into(),
                    );
                } else {
                    w.fields.remove("drawing-preview");
                }
            }
            _ => return Err(invalid("Unsupported drawing context control")),
        }
        return Ok(());
    }
    let sheet_id = element
        .data
        .get("sheet_id")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid("Missing drawing sheet"))?;
    let mut sheet = crate::drawing_ui::sheet_source(w, sheet_id)
        .ok_or_else(|| invalid("The drawing sheet no longer exists"))?;
    let mut source = json!(&sheet);
    let pointer = element
        .data
        .get("pointer")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid("Missing drawing field address"))?;
    let value = if element.data.get("optional_factory").is_some() {
        if value
            .as_bool()
            .ok_or_else(|| invalid("Expected a drawing option toggle"))?
        {
            if let Some(value) = source.pointer(pointer).filter(|v| !v.is_null()) {
                value.clone()
            } else {
                optional_default(w, &sheet, &element, pointer)?
            }
        } else {
            Value::Null
        }
    } else if element.data.get("join_array").and_then(Value::as_bool) == Some(true) {
        json!(
            value
                .as_str()
                .ok_or_else(|| invalid("Enter datum letters separated by spaces"))?
                .split_whitespace()
                .collect::<Vec<_>>()
        )
    } else {
        value
    };
    match element.data.get("compound").and_then(Value::as_str) {
        Some("balloon_item") => {
            set_at(
                &mut source,
                &format!("{pointer}/target/reference"),
                Value::Null,
            )?;
            set_at(
                &mut source,
                &format!("{pointer}/row_key"),
                value["row_key"].clone(),
            )?;
            set_at(
                &mut source,
                &format!("{pointer}/definition"),
                value["definition"].clone(),
            )?;
            set_at(
                &mut source,
                &format!("{pointer}/configuration"),
                value["configuration"].clone(),
            )?;
        }
        Some("cut_list_balloon_item") => {
            set_at(
                &mut source,
                &format!("{pointer}/row_key"),
                value["row_key"].clone(),
            )?;
            set_at(
                &mut source,
                &format!("{pointer}/source"),
                value["source"].clone(),
            )?;
            // Changing the stock occurrence must retire an old attachment explicitly.
            set_at(
                &mut source,
                &format!("{pointer}/target/reference"),
                Value::Null,
            )?;
        }
        Some("hole_model") => {
            let owner = value
                .as_str()
                .ok_or_else(|| invalid("Choose the hole model"))?;
            let candidate = crate::drawing_table_ui::hole_candidates(w, &sheet)
                .into_iter()
                .find(|(s, _)| s.definition == owner)
                .ok_or_else(|| invalid("This scope has no designed holes"))?
                .0;
            set_at(&mut source, pointer, json!([candidate]))?;
            let parent = pointer.rsplit_once('/').unwrap().0;
            set_at(&mut source, &format!("{parent}/configuration"), Value::Null)?;
            set_at(&mut source, &format!("{parent}/instance_path"), json!([]))?;
            set_at(
                &mut source,
                &format!("{parent}/datum"),
                json!(HoleDatum::ModelOrigin),
            )?;
        }
        _ => set_at(&mut source, pointer, value.clone())?,
    }
    if pointer == "/format/border" && value == Value::Bool(false) {
        set_at(&mut source, "/format/zones", Value::Null)?;
    }
    if pointer == "/definition" {
        set_at(&mut source, "/exploded_view", Value::Null)?;
        set_at(&mut source, "/display_state", Value::Null)?;
    }
    if pointer.ends_with("/options/kind")
        && !matches!(
            value.as_str(),
            Some("horizontal" | "vertical" | "perpendicular")
        )
    {
        let parent = pointer.rsplit_once('/').unwrap().0;
        set_at(&mut source, &format!("{parent}/second_curve"), Value::Null)?;
    }
    if pointer.ends_with("/kind/scope") {
        let parent = pointer.rsplit_once('/').unwrap().0;
        set_at(&mut source, &format!("{parent}/configuration"), Value::Null)?;
        set_at(&mut source, &format!("{parent}/sources"), json!([]))?;
    }
    if pointer.ends_with("/kind/source") {
        let parent = pointer.rsplit_once('/').unwrap().0;
        set_at(&mut source, &format!("{parent}/configuration"), Value::Null)?;
    }
    sheet = serde_json::from_value(source)?;
    crate::drawing_gdt_ui::normalize_mode(&mut sheet, pointer, &value)?;
    if (pointer.contains("/bom_options/") || pointer.contains("/cut_list_options/"))
        && let Some(evaluation) = crate::drawing_ui::evaluation(w)
    {
        let context = sheet.clone();
        let index = pointer
            .split('/')
            .nth(2)
            .and_then(|index| index.parse::<usize>().ok());
        if let Some(table) = index.and_then(|index| sheet.tables.get_mut(index)) {
            pin_source_rows(w.session.visible(), evaluation, &context, table)?;
        }
    }
    if pointer.contains("/item_numbers/") {
        w.fields.insert(
            "drawing-row".into(),
            unescape(pointer.rsplit('/').next().unwrap_or("")),
        );
    }
    publish(w, sheet);
    Ok(())
}

fn get_array<'a>(source: &'a mut Value, path: &str) -> Result<&'a mut Vec<Value>> {
    source
        .pointer_mut(path)
        .and_then(Value::as_array_mut)
        .ok_or_else(|| invalid("The drawing source list no longer exists"))
}
fn selected_view(sheet: &Drawing) -> String {
    view_keys(sheet).first().cloned().unwrap_or("front".into())
}
fn create_dimension(w: &Workspace, sheet: &Drawing, kind: &str) -> Result<DrawingDimension> {
    let kind: DimensionKind = serde_json::from_value(json!(kind))?;
    let curve = crate::drawing_ui::selected_curve(w);
    let (first, second) = if let Some(CurveReference::ModelEdge {
        edge,
        body,
        instance_path,
    }) = &curve
    {
        (
            Some(PointReference::EdgePoint {
                edge: edge.clone(),
                body: body.clone(),
                instance_path: instance_path.clone(),
                parameter: 0.,
            }),
            Some(PointReference::EdgePoint {
                edge: edge.clone(),
                body: body.clone(),
                instance_path: instance_path.clone(),
                parameter: 1.,
            }),
        )
    } else {
        (None, None)
    };
    let hole_feature = w
        .selected
        .as_ref()
        .filter(|s| {
            w.session
                .visible()
                .parts
                .get(&s.reference.producer.definition)
                .is_some_and(|p| {
                    p.features.iter().any(|f| {
                        f.id == s.reference.producer.node
                            && matches!(
                                f.operation,
                                cad_core::Operation::Hole { .. }
                                    | cad_core::Operation::DrilledHoles { .. }
                            )
                    })
                })
        })
        .map(|s| s.reference.producer.clone());
    let mut dimension = DrawingDimension {
        id: cad_core::id(),
        view: selected_view(sheet),
        first: [0.; 3],
        second: [50., 0., 0.],
        offset_mm: 10.,
        parameter: None,
        tolerance: String::new(),
        options: DimensionOptions {
            kind,
            curve,
            first,
            second,
            hole_feature,
            third_point: [0., 0., 50.],
            ..Default::default()
        },
    };
    if let Some(evaluation) = crate::drawing_ui::evaluation(w) {
        let _ = cad_core::drawing_interaction::place_dimension_outside(
            w.session.visible(),
            evaluation,
            sheet,
            &mut dimension,
        );
    }
    Ok(dimension)
}
fn create_annotation(w: &Workspace, sheet: &Drawing, kind: &str) -> Result<Annotation> {
    let target = crate::drawing_ui::selected_anchor(w);
    let kind = match kind {
        "revision_cloud" => crate::drawing_revision_ui::create(w, sheet),
        "datum_target" => crate::drawing_gdt_ui::create_target(w, sheet)?,
        "tolerance_frame" => crate::drawing_gdt_ui::create_frame(w, sheet),
        "dimension_group" => {
            crate::drawing_dimension_groups_ui::create(w, sheet, &selected_view(sheet))
        }
        "leader" => AnnotationKind::Leader {
            target,
            text: "Note".into(),
        },
        "datum" => AnnotationKind::Datum {
            target,
            identifier: "A".into(),
        },
        "geometric_tolerance" => AnnotationKind::GeometricTolerance {
            target,
            characteristic: GeometricCharacteristic::Position,
            value: "0.1 mm".into(),
            diameter: false,
            material: MaterialCondition::Regardless,
            datums: vec![],
        },
        "center_mark" => AnnotationKind::CenterMark {
            curve: crate::drawing_ui::selected_curve(w)
                .ok_or_else(|| invalid("Select a circular edge before adding a center mark"))?,
            extension_mm: 3.,
        },
        "center_line" => AnnotationKind::CenterLine {
            first: target,
            second: Box::new(Anchor {
                point: [50., 0., 0.],
                reference: None,
            }),
            extension_mm: 3.,
        },
        "balloon" => crate::drawing_table_ui::default_balloon(w, sheet, target)?,
        "cut_list_balloon" => crate::drawing_table_ui::default_cut_list_balloon(w, sheet, target)?,
        "surface_finish" => AnnotationKind::SurfaceFinish {
            target,
            symbol: SurfaceFinish::default(),
        },
        "weld_symbol" => AnnotationKind::WeldSymbol {
            target,
            symbol: WeldSymbol::default(),
        },
        _ => return Err(invalid("Unsupported drawing annotation type")),
    };
    Ok(Annotation {
        leader_points_mm: vec![],
        id: cad_core::id(),
        view: selected_view(sheet),
        offset_mm: [15., 15.],
        kind,
    })
}
pub fn action(w: &mut Workspace, command: &str, args: Option<&Value>) -> bool {
    if command != ACTION {
        return false;
    }
    let result = (|| -> Result<()> {
        let args =
            args.ok_or_else(|| invalid("Use a drawing action from the current inspector"))?;
        let id = args
            .get("element")
            .and_then(Value::as_str)
            .ok_or_else(|| invalid("Missing drawing action target"))?;
        let fresh = semantics(w)
            .into_iter()
            .find(|e| e.id == id)
            .ok_or_else(|| invalid("This drawing action is no longer available"))?;
        let snapshot = SemanticElement {
            data: args.clone(),
            ..fresh.clone()
        };
        let fresh = live(w, &snapshot)?;
        if fresh.state == "disabled" {
            return Err(invalid(
                fresh
                    .value
                    .as_deref()
                    .unwrap_or("This drawing action is unavailable"),
            ));
        }
        let mutation = &fresh.data["mutation"];
        let kind = mutation["kind"]
            .as_str()
            .ok_or_else(|| invalid("Missing drawing action behavior"))?;
        let sheet_id = fresh.data["sheet_id"]
            .as_str()
            .ok_or_else(|| invalid("Missing drawing sheet"))?
            .to_string();
        if kind == "existing" {
            let command = mutation["command"]
                .as_str()
                .ok_or_else(|| invalid("Missing sheet command"))?;
            crate::drawing_ui::sheet_command(w, &sheet_id, command);
            return Ok(());
        }
        if kind == "confirmation" {
            match mutation["command"].as_str().unwrap_or("") {
                "draft.apply" => w.confirm_tool(),
                "draft.cancel" => w.cancel_tool(),
                "file.export-pdf" => crate::ui::export_format(w, "pdf", "drawing.pdf"),
                "file.export-drawing-dxf" => {
                    crate::ui::export_format(w, "drawing-dxf", "drawing.dxf")
                }
                _ => return Err(invalid("Unknown drawing confirmation action")),
            }
            return Ok(());
        }
        let mut sheet = crate::drawing_ui::sheet_source(w, &sheet_id)
            .ok_or_else(|| invalid("The drawing sheet no longer exists"))?;
        if crate::drawing_revision_ui::apply_action(w, &mut sheet, mutation)? {
            validate_definition(&sheet)?;
            publish(w, sheet);
            return Ok(());
        }
        if crate::drawing_gdt_ui::apply_action(w, &mut sheet, mutation)? {
            validate_definition(&sheet)?;
            publish(w, sheet);
            return Ok(());
        }
        if crate::drawing_dimension_groups_ui::apply_action(w, &mut sheet, mutation)? {
            validate_definition(&sheet)?;
            publish(w, sheet);
            return Ok(());
        }
        if crate::drawing_table_ui::apply_action(w, &mut sheet, mutation)? {
            validate_definition(&sheet)?;
            publish(w, sheet);
            return Ok(());
        }
        match kind {
            "paper" => sheet.paper_mm = serde_json::from_value(mutation["value"].clone())?,
            "orientation" => sheet.paper_mm.swap(0, 1),
            "view-toggle" => {
                let key = mutation["view"]
                    .as_str()
                    .ok_or_else(|| invalid("Missing drawing view"))?;
                let mut views = view_keys(&sheet);
                if views.iter().any(|v| v == key) {
                    views.retain(|v| v != key);
                } else {
                    views.push(key.into());
                }
                set_views(&mut sheet, views)?;
            }
            "view-order" => {
                let key = mutation["view"]
                    .as_str()
                    .ok_or_else(|| invalid("Missing drawing view"))?;
                let mut views = view_keys(&sheet);
                let index = views
                    .iter()
                    .position(|v| v == key)
                    .ok_or_else(|| invalid("The view no longer exists"))?;
                let destination = (index as i64 + mutation["step"].as_i64().unwrap_or(0))
                    .try_into()
                    .ok()
                    .filter(|i: &usize| *i < views.len())
                    .ok_or_else(|| invalid("The view order changed"))?;
                views.swap(index, destination);
                sheet.views = views;
            }
            "dimension-add" => {
                let d = create_dimension(
                    w,
                    &sheet,
                    mutation["dimension"].as_str().unwrap_or("aligned"),
                )?;
                w.fields.insert("drawing-item".into(), d.id.clone());
                sheet.dimensions.push(d);
            }
            "annotation-add" => {
                let a = create_annotation(
                    w,
                    &sheet,
                    mutation["annotation"].as_str().unwrap_or("leader"),
                )?;
                w.fields.insert("drawing-item".into(), a.id.clone());
                sheet.entities.push(a);
            }
            "table-add" => {
                let table = crate::drawing_table_ui::create(
                    w,
                    &sheet,
                    mutation["table"].as_str().unwrap_or("general"),
                )?;
                w.fields.insert("drawing-item".into(), table.id.clone());
                w.fields.insert("drawing-table".into(), table.id.clone());
                sheet.tables.push(table);
            }
            "title-add" => {
                let block = sheet
                    .format
                    .as_mut()
                    .and_then(|f| f.title_block.as_mut())
                    .ok_or_else(|| invalid("Enable a title block first"))?;
                let cell = TitleCell {
                    id: cad_core::id(),
                    label: "FIELD".into(),
                    value: LinkedText::Literal {
                        text: String::new(),
                    },
                    rect_mm: [0., block.size_mm[1], block.size_mm[0], 12.],
                };
                w.fields.insert("drawing-item".into(), cell.id.clone());
                block.cells.push(cell);
                block.size_mm[1] += 12.;
            }
            "table-delete" => {
                let table = mutation["table"]
                    .as_str()
                    .ok_or_else(|| invalid("Missing table"))?;
                if cad_core::drawing_revision::referenced(&sheet, table, None) {
                    return Err(invalid(
                        "Reattach or unlink revision clouds before deleting their revision table",
                    ));
                }
                sheet.tables.retain(|t| t.id != table);
                sheet.entities.retain(
                    |a| !matches!(&a.kind,AnnotationKind::Balloon{table:source,..} | AnnotationKind::CutListBalloon{table:source,..} if source==table),
                );
            }
            "table-refresh" => {
                let e = crate::drawing_ui::evaluation(w)
                    .ok_or_else(|| invalid("Wait for current model evaluation"))?;
                let context = sheet.clone();
                let table = sheet
                    .tables
                    .iter_mut()
                    .find(|t| Some(t.id.as_str()) == mutation["table"].as_str())
                    .ok_or_else(|| invalid("The table no longer exists"))?;
                pin_source_rows(w.session.visible(), e, &context, table)?;
            }
            "revision-add" => {
                let table = sheet
                    .tables
                    .iter_mut()
                    .find(|t| Some(t.id.as_str()) == mutation["table"].as_str())
                    .ok_or_else(|| invalid("The table no longer exists"))?;
                let TableKind::Revisions { rows } = &mut table.kind else {
                    return Err(invalid("The table type changed"));
                };
                let mut number = rows.len() + 1;
                while rows.iter().any(|r| r.revision == number.to_string()) {
                    number += 1;
                }
                let row = RevisionRow {
                    id: cad_core::id(),
                    revision: number.to_string(),
                    description: String::new(),
                    date: String::new(),
                    approved_by: String::new(),
                };
                w.fields.insert("drawing-row".into(), row.id.clone());
                rows.push(row);
            }
            "general-column" => {
                let index = sheet
                    .tables
                    .iter()
                    .position(|t| Some(t.id.as_str()) == mutation["table"].as_str())
                    .ok_or_else(|| invalid("The table no longer exists"))?;
                let TableKind::General { headers, rows } = &sheet.tables[index].kind else {
                    return Err(invalid("The table type changed"));
                };
                let add = mutation["add"] == Value::Bool(true);
                let context = Fields::new(w, &sheet);
                let mut collections = vec![
                    (
                        context.address(&format!("/tables/{index}/kind/headers")).0,
                        headers.len(),
                    ),
                    (
                        context
                            .address(&format!("/tables/{index}/column_widths_mm"))
                            .0,
                        sheet.tables[index].column_widths_mm.len(),
                    ),
                ];
                collections.extend(rows.iter().enumerate().map(|(row, cells)| {
                    (
                        context
                            .address(&format!("/tables/{index}/kind/rows/{row}"))
                            .0,
                        cells.len(),
                    )
                }));
                for (collection, count) in collections {
                    if add {
                        append_token(w, &sheet_id, &collection, count);
                    } else {
                        remove_token(w, &sheet_id, &collection, count, count.saturating_sub(1));
                    }
                }
                let table = &mut sheet.tables[index];
                let TableKind::General { headers, rows } = &mut table.kind else {
                    unreachable!()
                };
                if add {
                    headers.push(format!("Column {}", headers.len() + 1));
                    table.column_widths_mm.push(35.);
                    for row in rows {
                        row.push(String::new());
                    }
                } else {
                    headers.pop();
                    table.column_widths_mm.pop();
                    for row in rows {
                        row.pop();
                    }
                }
            }
            "general-row-add" => {
                let index = sheet
                    .tables
                    .iter()
                    .position(|t| Some(t.id.as_str()) == mutation["table"].as_str())
                    .ok_or_else(|| invalid("The table no longer exists"))?;
                let collection = Fields::new(w, &sheet)
                    .address(&format!("/tables/{index}/kind/rows"))
                    .0;
                let TableKind::General { rows, .. } = &sheet.tables[index].kind else {
                    return Err(invalid("The table type changed"));
                };
                let token = append_token(w, &sheet_id, &collection, rows.len());
                w.fields.insert("drawing-row".into(), token);
                let TableKind::General { headers, rows } = &mut sheet.tables[index].kind else {
                    unreachable!()
                };
                rows.push(vec![String::new(); headers.len()]);
            }
            "row-order" => {
                let context = sheet.clone();
                let table = sheet
                    .tables
                    .iter_mut()
                    .find(|t| Some(t.id.as_str()) == mutation["table"].as_str())
                    .ok_or_else(|| invalid("The table no longer exists"))?;
                let e = crate::drawing_ui::evaluation(w)
                    .ok_or_else(|| invalid("Wait for model evaluation"))?;
                pin_source_rows(w.session.visible(), e, &context, table)?;
                let first = table
                    .row_order
                    .iter()
                    .position(|k| Some(k.as_str()) == mutation["row"].as_str())
                    .ok_or_else(|| invalid("The source row no longer exists"))?;
                let second = table
                    .row_order
                    .iter()
                    .position(|k| Some(k.as_str()) == mutation["neighbor"].as_str())
                    .ok_or_else(|| invalid("The source row order changed"))?;
                table.row_order.swap(first, second);
            }
            "source-toggle" => {
                let pointer = mutation["pointer"]
                    .as_str()
                    .ok_or_else(|| invalid("Missing source selection"))?;
                let mut source = json!(&sheet);
                let next = mutation["value"].clone();
                let array = get_array(&mut source, pointer)?;
                if array.is_empty() && mutation["allow_empty"] == Value::Bool(true) {
                    *array = mutation["choices"].as_array().cloned().unwrap_or_default();
                }
                if array.contains(&next) {
                    array.retain(|v| v != &next);
                } else {
                    array.push(next);
                }
                sheet = serde_json::from_value(source)?;
            }
            "set" | "append" | "remove" => {
                let pointer = mutation["pointer"]
                    .as_str()
                    .ok_or_else(|| invalid("Missing drawing list"))?;
                if kind == "remove"
                    && pointer.starts_with("/tables/")
                    && pointer.ends_with("/kind/rows")
                {
                    let table = pointer
                        .split('/')
                        .nth(2)
                        .and_then(|index| index.parse::<usize>().ok())
                        .and_then(|index| sheet.tables.get(index));
                    if let Some(table) = table
                        && let TableKind::Revisions { rows } = &table.kind
                    {
                        let row = mutation["index"]
                            .as_u64()
                            .and_then(|index| usize::try_from(index).ok())
                            .and_then(|index| rows.get(index));
                        if row.is_some_and(|row| {
                            cad_core::drawing_revision::referenced(&sheet, &table.id, Some(&row.id))
                        }) {
                            return Err(invalid(
                                "Reattach or unlink revision clouds before deleting their revision row",
                            ));
                        }
                    }
                }
                let mut source = json!(&sheet);
                match kind {
                    "set" => set_at(&mut source, pointer, mutation["value"].clone())?,
                    "append" => {
                        let array = get_array(&mut source, pointer)?;
                        let collection = Fields::new(w, &sheet).address(pointer).0;
                        append_token(w, &sheet_id, &collection, array.len());
                        array.push(mutation["value"].clone());
                    }
                    _ => {
                        let array = get_array(&mut source, pointer)?;
                        let index = mutation["index"]
                            .as_u64()
                            .ok_or_else(|| invalid("Missing drawing list item"))?
                            as usize;
                        if index >= array.len() {
                            return Err(invalid("The drawing list item no longer exists"));
                        }
                        let count = array.len();
                        let anonymous = array[index].get("id").is_none();
                        array.remove(index);
                        if anonymous {
                            let collection = Fields::new(w, &sheet).address(pointer).0;
                            remove_token(w, &sheet_id, &collection, count, index);
                        }
                    }
                }
                sheet = serde_json::from_value(source)?;
            }
            _ => return Err(invalid("Unsupported drawing action")),
        }
        publish(w, sheet);
        Ok(())
    })();
    if let Err(error) = result {
        w.status = error.to_string();
        w.confirm_requested = false;
    }
    true
}
