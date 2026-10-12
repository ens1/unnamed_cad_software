//! One-time component copies over a captured validated assembly and shared draft.
use crate::{Workspace, presentation_controls as controls};
use bevy_egui::egui;
use cad_core::{
    CadError, Id, Result,
    assembly_copy::{self, Baseline, Built, Intent, Slot},
    commands::Command,
    configuration::PlacementScope,
    presentation::SemanticElement,
    protocol::RequestIdentity,
};
use serde_json::{Value, json};
const ACTION: &str = "assembly.copy-action";
const ERRORS: &str = "assembly-copy-field-error:";
pub struct State {
    token: Id,
    baseline: Baseline,
    intent: Intent,
    accepted: Option<Intent>,
    expected: RequestIdentity,
    sequence: u64,
    error: Option<String>,
    result: Option<Built>,
}
fn invalid(message: &str) -> CadError {
    CadError::Invalid(format!("Copy With Mates: {message}"))
}
fn parsed(w: &Workspace) -> Result<Intent> {
    let state = w
        .assembly_copy
        .as_ref()
        .ok_or_else(|| invalid("The copy editor is closed"))?;
    let intent: Intent = serde_json::from_str(&w.json)?;
    if intent.id != state.intent.id
        || intent.assembly != state.baseline.assembly.id
        || intent.version != assembly_copy::CONTRACT_VERSION
    {
        return Err(invalid("Editor identity, owner and version cannot change"));
    }
    Ok(intent)
}
fn source_current(w: &Workspace) -> bool {
    w.assembly_copy.as_ref().is_none_or(|state| {
        state.expected == w.session.identity()
            && state.baseline.source.id == w.session.visible().id
            && state.baseline.source.root == w.session.visible().root
    })
}
pub fn current_source_is_valid(w: &Workspace) -> bool {
    w.assembly_copy.as_ref().is_none_or(|state| {
        source_current(w)
            && state.error.is_none()
            && parsed(w).is_ok_and(|intent| state.accepted.as_ref() == Some(&intent))
            && !w.fields.keys().any(|key| key.starts_with(ERRORS))
    })
}
pub fn cancel(w: &mut Workspace) {
    w.assembly_copy = None;
    w.fields
        .retain(|key, _| !key.starts_with("assembly-copy-") && !key.starts_with(ERRORS));
}
fn publish(w: &mut Workspace, mut intent: Intent) -> Result<()> {
    if !source_current(w) {
        return Err(invalid(
            "Assembly source changed outside this operation; cancel and recapture current solved poses",
        ));
    }
    let mut state = w
        .assembly_copy
        .take()
        .ok_or_else(|| invalid("The copy editor is closed"))?;
    state.sequence = state.sequence.wrapping_add(1);
    let result = (|| -> Result<()> {
        if intent.id != state.intent.id || intent.assembly != state.baseline.assembly.id {
            return Err(invalid("Copy identity and owning assembly cannot change"));
        }
        assembly_copy::synchronize(&state.baseline, &mut intent)?;
        let built = assembly_copy::build(&state.baseline, &intent)?;
        w.dispatch(Command::ReplaceDocument {
            document: built.document.clone(),
        });
        if w.session.visible().fingerprint() != built.document.fingerprint() {
            return Err(invalid(&w.status));
        }
        state.expected = w.session.identity();
        state.accepted = Some(intent.clone());
        state.result = Some(built);
        Ok(())
    })();
    state.intent = intent;
    state.error = result.as_ref().err().map(ToString::to_string);
    w.json = serde_json::to_string_pretty(&state.intent).unwrap_or_default();
    w.assembly_copy = Some(state);
    // A seed/mate policy edit may remove fields. Their old text diagnostics
    // must not trap Apply after the corresponding constraint was deselected.
    let fields = semantics(w)
        .into_iter()
        .filter(|element| element.role == "assembly-copy-field")
        .map(|element| element.id)
        .collect::<std::collections::BTreeSet<_>>();
    w.fields.retain(|key, _| {
        key.strip_prefix(ERRORS)
            .is_none_or(|id| fields.contains(id))
    });
    result
}
pub fn new(w: &mut Workspace, owner: &str) {
    if w.tool_checkpoint.is_some()
        || w.session.draft.is_some()
        || w.sketch.is_some()
        || w.confirm_requested
        || w.assembly_motion.is_some()
        || w.settings.is_some()
    {
        w.status = "Confirm or cancel the current operation before copying components".into();
        return;
    }
    let current = w
        .session
        .evaluation
        .as_ref()
        .filter(|evaluation| evaluation.valid && evaluation.identity == w.session.identity());
    let Some(evaluation) = current else {
        w.status="Wait for the current validated assembly evaluation before capturing components and mates".into();
        return;
    };
    let baseline = match Baseline::capture(
        w.session.visible(),
        evaluation,
        &w.session.identity(),
        owner,
    ) {
        Ok(baseline) => baseline,
        Err(error) => {
            w.status = error.to_string();
            return;
        }
    };
    let available = baseline.available();
    if available.is_empty() {
        w.status = "Insert an ordinary component before copying it with mates".into();
        return;
    }
    let mut intent = Intent::new(
        owner.into(),
        PlacementScope::selected(w.session.visible(), owner),
    );
    for selected in w.geometry_selection.iter().chain(w.selected.iter()) {
        if let Some(seed) = selected
            .path
            .first()
            .filter(|seed| available.contains(seed))
            && !intent.seeds.contains(seed)
        {
            intent.seeds.push(seed.clone());
        }
    }
    if intent.seeds.is_empty() {
        if let Some(seed) = w
            .view
            .active_instance
            .as_ref()
            .filter(|id| available.contains(id))
        {
            intent.seeds.push(seed.clone());
        } else {
            intent.seeds.push(available[0].clone());
        }
    }
    let identity = w.session.identity();
    w.start_tool();
    w.fields.insert("assembly-id".into(), owner.into());
    w.fields
        .insert("assembly-copy-editor".into(), intent.id.clone());
    w.view.active_part = None;
    w.view.active_node = None;
    w.json = serde_json::to_string_pretty(&intent).unwrap_or_default();
    w.assembly_copy = Some(State {
        token: cad_core::id(),
        baseline,
        intent: intent.clone(),
        accepted: None,
        expected: identity,
        sequence: 0,
        error: None,
        result: None,
    });
    w.open_editor("assembly");
    if let Err(error) = publish(w, intent) {
        w.status = error.to_string();
    }
}
pub fn begin(w: &mut Workspace, owner: &str) {
    new(w, owner);
}
fn source(intent: &Intent) -> Value {
    let mut value = json!(intent);
    for entry in value["external"].as_array_mut().into_iter().flatten() {
        for binding in entry["bindings"].as_array_mut().into_iter().flatten() {
            if binding.get("reference").is_none() {
                binding["reference"] = Value::Null;
            }
        }
    }
    value
}
fn identity(state: &State, key: &str) -> String {
    format!("assembly-copy:{}/{key}", state.intent.id)
}
fn meta(state: &State) -> Value {
    json!({"contract":"tcad.presentation.assembly-copy","version":1,"document":state.baseline.source.id,"assembly":state.intent.assembly,"editor":state.token,"sequence":state.sequence,"source_identity":state.expected,"captured_identity":state.baseline.identity,"shared_checkpoint":true,"requires_current_validation":true})
}
struct Fields<'a> {
    w: &'a Workspace,
    state: &'a State,
    source: Value,
    out: Vec<SemanticElement>,
    group: &'static str,
}
impl Fields<'_> {
    fn group(&mut self, key: &'static str, label: &str) {
        self.group = key;
        let mut data = meta(self.state);
        data["classes"] = json!(["modeling-group", key]);
        self.out.push(SemanticElement {
            id: identity(self.state, &format!("group/{key}")),
            role: "assembly-copy-group".into(),
            label: label.into(),
            value: None,
            state: "draft".into(),
            commands: vec![],
            relationships: vec![self.state.intent.assembly.clone()],
            data,
        });
    }
    fn field(&mut self, path: &str, label: &str, unit: Option<&str>, mut choices: Vec<Value>) {
        let Some(value) = self.source.pointer(path).cloned() else {
            return;
        };
        if !choices.is_empty() && !choices.iter().any(|choice| choice["value"] == value) {
            choices.push(json!({"label":"Saved option / reference","value":value}));
        }
        let id = identity(self.state, path.trim_start_matches('/'));
        let enabled = source_current(self.w) && self.w.settings.is_none();
        let mut data = meta(self.state);
        data["setter"] = json!("assembly-copy");
        data["pointer"] = json!(path);
        data["json"] = value.clone();
        data["editable"] = json!(enabled);
        data["choices"] = json!(choices);
        data["unit"] = json!(unit);
        data["group"] = json!(self.group);
        data["classes"] = json!(["modeling-field", self.group]);
        data["accessibility_label"] = json!(label);
        data["value_type"] = json!(if !choices.is_empty() {
            if value.is_object() {
                "reference"
            } else {
                "choice"
            }
        } else if value.is_boolean() {
            "boolean"
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
        if let Some(error) = self.w.fields.get(&format!("{ERRORS}{id}")) {
            data["diagnostics"] = json!([error]);
        }
        self.out.push(SemanticElement {
            id,
            role: "assembly-copy-field".into(),
            label: label.into(),
            value: Some(
                value
                    .as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| value.to_string()),
            ),
            state: if enabled { "draft" } else { "disabled" }.into(),
            commands: vec![],
            relationships: vec![self.state.intent.assembly.clone()],
            data,
        });
    }
    fn action(&mut self, key: &str, label: &str, mutation: Value, enabled: bool) {
        let id = identity(self.state, &format!("action/{key}"));
        let mut data = meta(self.state);
        data["element"] = json!(id);
        data["mutation"] = mutation;
        data["actions"] = json!([{"command":ACTION,"label":label,"enabled":enabled&&source_current(self.w),"arguments":data.clone()}]);
        self.out.push(SemanticElement {
            id,
            role: "assembly-copy-action".into(),
            label: label.into(),
            value: None,
            state: if enabled { "ready" } else { "disabled" }.into(),
            commands: vec![ACTION.into()],
            relationships: vec![self.state.intent.assembly.clone()],
            data,
        });
    }
    fn note(&mut self, key: &str, label: &str, value: Option<String>, state: &str) {
        self.out.push(SemanticElement {
            id: identity(self.state, key),
            role: "assembly-copy-help".into(),
            label: label.into(),
            value,
            state: state.into(),
            commands: vec![],
            relationships: vec![self.state.intent.assembly.clone()],
            data: meta(self.state),
        });
    }
}
fn selected_reference(
    w: &Workspace,
    state: &State,
) -> Option<(Id, cad_core::assembly_geometry::Reference)> {
    let (instance, reference) =
        crate::mate_ui::selected_reference(w, &state.baseline.assembly).ok()?;
    state
        .baseline
        .assembly
        .instances
        .contains_key(&instance)
        .then_some((instance, reference))
}
fn slot_label(slot: &Slot) -> String {
    match slot {
        Slot::First => "First participant".into(),
        Slot::Second => "Second participant".into(),
        Slot::Auxiliary { index } => format!("Additional participant {}", index + 1),
        Slot::Support => "Mechanical support".into(),
    }
}
pub fn semantics(w: &Workspace) -> Vec<SemanticElement> {
    let Some(state) = w.assembly_copy.as_ref() else {
        return vec![];
    };
    let intent = parsed(w).unwrap_or_else(|_| state.intent.clone());
    let mut fields = Fields {
        w,
        state,
        source: source(&intent),
        out: vec![],
        group: "definition",
    };
    fields.group("seeds", "Components to copy");
    fields.note("scope","Copies become ordinary editable occurrences and mates. Assembly insertion is shared across configurations; selected named scope stores initial pose hints. Configure suppression separately.",None,"ready");
    for seed in state.baseline.available() {
        let component = &state.baseline.assembly.instances[&seed];
        let mut seeds = intent.seeds.clone();
        let selected = seeds.contains(&seed);
        if selected {
            seeds.retain(|id| id != &seed);
        } else {
            seeds.push(seed.clone());
        }
        fields.action(
            &format!("seed/{seed}"),
            &format!("{} {}", if selected { "✓" } else { "+" }, component.name),
            json!({"kind":"seeds","value":seeds}),
            true,
        );
    }
    fields.field("/count", "New copied groups", Some(""), vec![]);
    fields.field("/keep_fixed", "Retain source grounding", None, vec![]);
    fields.group("placement", "Initial copied placements");
    for (path, label, unit) in [
        ("translation", "Translation per new group", "mm"),
        ("rotation", "Rotation per new group", "deg"),
        ("pivot", "Assembly rotation pivot", "mm"),
    ] {
        for (index, axis) in ["X", "Y", "Z"].iter().enumerate() {
            fields.field(
                &format!("/{path}/{index}"),
                &format!("{label} {axis}"),
                Some(unit),
                vec![],
            );
        }
    }
    let mut scopes = vec![json!({"label":"Base initial placement","value":PlacementScope::Base})];
    if let Some(id) = state
        .baseline
        .source
        .active_configurations
        .get(&intent.assembly)
    {
        scopes.push(json!({"label":"Current named configuration initial pose","value":PlacementScope::Named(id.clone())}));
    }
    fields.field("/scope", "Initial pose scope", None, scopes);
    fields.group("mates", "Copied constraints");
    fields.note("internal","All active internal mates and rigid-group lock equations are copied with the group. Copied constraints retain native nested references and may be edited independently after Apply.",None,"ready");
    let selected = selected_reference(w, state);
    for (index, entry) in intent.external.iter().enumerate() {
        let Some(mate) = state
            .baseline
            .assembly
            .mates
            .iter()
            .find(|mate| mate.id == entry.source)
        else {
            continue;
        };
        let name = |id: &str| {
            state
                .baseline
                .assembly
                .instances
                .get(id)
                .map(|instance| instance.name.as_str())
                .unwrap_or("Missing component")
        };
        fields.field(
            &format!("/external/{index}/enabled"),
            &format!(
                "Copy outside {} mate · {} / {}",
                mate.kind,
                name(&mate.first),
                name(&mate.second)
            ),
            None,
            vec![],
        );
        if !entry.enabled {
            continue;
        }
        for (binding_index, binding) in entry.bindings.iter().enumerate() {
            let path = format!("/external/{index}/bindings/{binding_index}");
            let old = state.baseline.assembly.instances.get(&binding.source);
            let choices = state
                .baseline
                .assembly
                .instances
                .values()
                .filter(|target| {
                    mate.kind == "lock"
                        || binding.slot == Slot::Support
                        || old.is_some_and(|source| source.definition == target.definition)
                        || selected.as_ref().is_some_and(|(id, _)| id == &target.id)
                            && binding.reference.is_some()
                })
                .map(|target| json!({"label":target.name,"value":target.id}))
                .collect();
            fields.field(
                &format!("{path}/target"),
                &format!(
                    "Rebind {} · {}",
                    slot_label(&binding.slot),
                    name(&binding.source)
                ),
                None,
                choices,
            );
            if binding.slot != Slot::Support && mate.kind != "lock" {
                let selected = selected.clone();
                fields.action(&format!("reference/{index}/{binding_index}"),"Use selected native face / edge / datum",json!({"kind":"reference","entry":index,"binding":binding_index,"value":selected}),selected.is_some());
                fields.action(
                    &format!("retain-reference/{index}/{binding_index}"),
                    "Retain source geometry on matching definition",
                    json!({"kind":"retain_reference","entry":index,"binding":binding_index}),
                    binding.reference.is_some(),
                );
                fields.note(&format!("reference-status/{index}/{binding_index}"),if binding.reference.is_some(){"Explicit selected geometry replaces the outside endpoint."}else{"Same-definition target retains the complete source geometry path; choose model geometry to rebind another definition."},None,"ready");
            }
        }
    }
    if let Some(error) = &state.error {
        fields.note(
            "error",
            "Copy definition needs correction",
            Some(error.clone()),
            "invalid",
        );
    }
    if let Some(result) = &state.result {
        fields.note(
            "result",
            "Current copy proposal",
            Some(format!(
                "{} components, {} copied mates; {} outside mates explicitly skipped",
                result.inserted.len(),
                result.mates.len(),
                result.skipped_external
            )),
            "ready",
        );
    }
    if !source_current(w) {
        fields.note(
            "stale",
            "Assembly source changed; cancel and recapture solved geometry before applying.",
            None,
            "invalid",
        );
    }
    fields.out
}
fn live(w: &Workspace, element: &SemanticElement) -> Result<SemanticElement> {
    let fresh = semantics(w)
        .into_iter()
        .find(|fresh| fresh.id == element.id)
        .ok_or_else(|| invalid("The copy field or action is no longer available"))?;
    for key in [
        "document",
        "assembly",
        "editor",
        "sequence",
        "source_identity",
    ] {
        if fresh.data[key] != element.data[key] {
            return Err(invalid(
                "Copy source changed; use the current semantic control",
            ));
        }
    }
    if !source_current(w) {
        return Err(invalid(
            "Assembly source changed; cancel and recapture the operation",
        ));
    }
    Ok(fresh)
}
pub fn current_value(w: &Workspace, element: &SemanticElement) -> Option<Value> {
    live(w, element).ok()?;
    source(&parsed(w).ok()?)
        .pointer(element.data["pointer"].as_str()?)
        .cloned()
}
pub fn field_is_invalid(w: &Workspace, element: &SemanticElement) -> bool {
    element.data["setter"].as_str() == Some("assembly-copy")
        && w.fields.contains_key(&format!("{ERRORS}{}", element.id))
}
pub fn field_error(w: &mut Workspace, element: &SemanticElement, error: Option<String>) {
    if element.data["setter"].as_str() != Some("assembly-copy") {
        return;
    }
    let key = format!("{ERRORS}{}", element.id);
    if let Some(error) = error {
        w.fields.insert(key, error);
    } else {
        w.fields.remove(&key);
    }
}
pub fn set_value(w: &mut Workspace, element: &SemanticElement, value: Value) -> Result<()> {
    let element = live(w, element)?;
    let pointer = element.data["pointer"]
        .as_str()
        .ok_or_else(|| invalid("The copy field has no pointer"))?;
    let mut next = source(&parsed(w)?);
    *next
        .pointer_mut(pointer)
        .ok_or_else(|| invalid("The copy field disappeared"))? = value;
    if pointer.ends_with("/target") {
        let parent = pointer.trim_end_matches("/target");
        if let Some(binding) = next.pointer_mut(parent).and_then(Value::as_object_mut) {
            binding.insert("reference".into(), Value::Null);
        }
    }
    publish(w, serde_json::from_value(next)?)
}
pub fn action(w: &mut Workspace, command: &str, data: Option<&Value>) -> bool {
    if command == "assembly.copy-with-mates" {
        let owner = w.session.visible().root.clone();
        begin(w, &owner);
        return true;
    }
    if command == "assembly.copy-preview-source" {
        let result = parsed(w).and_then(|intent| publish(w, intent));
        if result.is_ok() {
            w.fields.retain(|key, _| !key.starts_with(ERRORS));
        }
        if let Err(error) = result {
            w.status = error.to_string();
        }
        return true;
    }
    if command != ACTION {
        return false;
    }
    let result = (|| -> Result<()> {
        let data = data.ok_or_else(|| invalid("Choose a current copy action"))?;
        let id = data["element"]
            .as_str()
            .ok_or_else(|| invalid("Copy action identity is missing"))?;
        let element = semantics(w)
            .into_iter()
            .find(|element| element.id == id)
            .ok_or_else(|| invalid("The copy action is no longer available"))?;
        live(
            w,
            &SemanticElement {
                data: data.clone(),
                ..element.clone()
            },
        )?;
        if element.data["mutation"] != data["mutation"]
            || !element
                .action_descriptors()
                .iter()
                .any(|action| action.enabled)
        {
            return Err(invalid(
                "Selected geometry changed; use the current copy action",
            ));
        }
        let mutation = &data["mutation"];
        let mut intent = parsed(w)?;
        match mutation["kind"].as_str() {
            Some("seeds") => intent.seeds = serde_json::from_value(mutation["value"].clone())?,
            Some("reference") | Some("retain_reference") => {
                let index = mutation["entry"]
                    .as_u64()
                    .ok_or_else(|| invalid("Mate mapping index missing"))?
                    as usize;
                let binding_index = mutation["binding"]
                    .as_u64()
                    .ok_or_else(|| invalid("Mate participant index missing"))?
                    as usize;
                let binding = intent
                    .external
                    .get_mut(index)
                    .and_then(|entry| entry.bindings.get_mut(binding_index))
                    .ok_or_else(|| invalid("Mate participant mapping changed"))?;
                if mutation["kind"] == "reference" {
                    let (target, reference) = serde_json::from_value(mutation["value"].clone())?;
                    binding.target = target;
                    binding.reference = Some(reference);
                } else {
                    binding.reference = None;
                }
            }
            _ => return Err(invalid("Unknown copy action")),
        }
        publish(w, intent)
    })();
    if let Err(error) = result {
        w.status = error.to_string();
    }
    true
}
pub fn show(ui: &mut egui::Ui, w: &mut Workspace) {
    for element in semantics(w) {
        match element.role.as_str() {
            "assembly-copy-group" => {
                controls::label(
                    ui,
                    &element.id,
                    &element.role,
                    &["modeling-group"],
                    &element.label,
                    &element.state,
                );
            }
            "assembly-copy-field" => crate::ui::semantic_field(ui, w, &element),
            "assembly-copy-action"
                if controls::button(
                    ui,
                    &element.id,
                    &element.role,
                    &["modeling-action"],
                    &element.label,
                    element
                        .action_descriptors()
                        .iter()
                        .any(|action| action.enabled),
                    false,
                )
                .clicked() =>
            {
                action(w, ACTION, Some(&element.data));
            }
            "assembly-copy-help" => {
                controls::label(
                    ui,
                    &element.id,
                    &element.role,
                    &[],
                    &element.label,
                    &element.state,
                );
                if let Some(value) = &element.value {
                    controls::label(
                        ui,
                        &format!("{}:value", element.id),
                        "tool-status",
                        &[],
                        value,
                        &element.state,
                    );
                }
            }
            _ => {}
        }
    }
    egui::CollapsingHeader::new("Advanced Source")
        .id_salt("assembly-copy-source")
        .show(ui, |ui| {
            ui.add(
                egui::TextEdit::multiline(&mut w.json)
                    .code_editor()
                    .desired_rows(8),
            );
            if controls::button(
                ui,
                "assembly-copy-preview-source",
                "assembly-copy-action",
                &[],
                "Preview edited copy source",
                source_current(w),
                false,
            )
            .clicked()
            {
                action(w, "assembly.copy-preview-source", None);
            }
        });
    crate::cad_ui::confirmation(ui, w);
}
