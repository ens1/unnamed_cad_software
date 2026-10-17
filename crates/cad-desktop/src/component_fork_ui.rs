//! Fork editor over the shared validated document draft and semantic controls.
use crate::{Workspace, presentation_controls as controls};
use bevy_egui::egui;
use cad_core::{
    CadError, Document, Result,
    commands::Command,
    component_fork::{self, Intent, Mode, Plan, Target},
    presentation::SemanticElement,
    protocol::RequestIdentity,
    version_control::VersionControl,
};
use serde_json::{Value, json};
const ACTION: &str = "component.fork-action";
const ERRORS: &str = "component-fork-field-error:";

pub struct State {
    baseline: Document,
    starting_view: cad_core::presentation::PresentationState,
    starting_selection: Option<crate::viewport::Selection>,
    starting_component: Option<Vec<cad_core::Id>>,
    intent: Intent,
    accepted: Option<Intent>,
    expected: RequestIdentity,
    sequence: u64,
    result: Option<Plan>,
    error: Option<String>,
}
fn invalid(message: &str) -> CadError {
    CadError::Invalid(format!("Fork component: {message}"))
}
fn parsed(w: &Workspace) -> Result<Intent> {
    let state = w
        .component_fork
        .as_ref()
        .ok_or_else(|| invalid("The fork editor is closed"))?;
    let intent: Intent = serde_json::from_str(&w.json)?;
    if intent.id != state.intent.id
        || intent.definition != state.intent.definition
        || intent.target != state.intent.target
        || intent.source_commit != state.intent.source_commit
        || intent.version != component_fork::CONTRACT_VERSION
    {
        return Err(invalid(
            "Fork source, target, revision and identity cannot change within an operation",
        ));
    }
    Ok(intent)
}
fn source_current(w: &Workspace) -> bool {
    w.component_fork.as_ref().is_none_or(|state| {
        state.expected == w.session.identity() && state.baseline.id == w.session.visible().id
    })
}
pub fn current_source_is_valid(w: &Workspace) -> bool {
    w.component_fork.as_ref().is_none_or(|state| {
        source_current(w)
            && state.error.is_none()
            && parsed(w).is_ok_and(|intent| state.accepted.as_ref() == Some(&intent))
            && !w.fields.keys().any(|key| key.starts_with(ERRORS))
    })
}
pub fn cancel(w: &mut Workspace) {
    if let Some(state) = w.component_fork.take() {
        w.view = state.starting_view;
        w.selected = state.starting_selection;
        w.selected_component = state.starting_component;
        w.rendered_key.clear();
    }
    w.fields
        .retain(|key, _| !key.starts_with("component-fork-") && !key.starts_with(ERRORS));
}
pub fn finish(w: &mut Workspace) {
    w.component_fork = None;
    w.fields
        .retain(|key, _| !key.starts_with("component-fork-") && !key.starts_with(ERRORS));
}
fn publish(w: &mut Workspace, intent: Intent) -> Result<()> {
    if !source_current(w) {
        return Err(invalid(
            "The document changed outside the fork editor; cancel and start again",
        ));
    }
    let mut state = w
        .component_fork
        .take()
        .ok_or_else(|| invalid("The fork editor is closed"))?;
    state.sequence = state.sequence.wrapping_add(1);
    state.accepted = None;
    state.result = None;
    let result = (|| -> Result<()> {
        let built = component_fork::plan_with_resources(&state.baseline, &intent, &w.root())?;
        w.dispatch(Command::ReplaceDocument {
            document: built.document.clone(),
        });
        if w.session.visible().fingerprint() != built.document.fingerprint() {
            return Err(invalid(&w.status));
        }
        state.expected = w.session.identity();
        state.accepted = Some(intent.clone());
        if built.document.parts.contains_key(&built.definition) {
            w.view.active_part = Some(built.definition.clone());
        } else {
            w.view.active_part = None;
        }
        w.view.active_node = None;
        w.selected_component = built.occurrence_path.clone();
        state.result = Some(built);
        Ok(())
    })();
    state.intent = intent;
    state.error = result.as_ref().err().map(ToString::to_string);
    w.json = serde_json::to_string_pretty(&state.intent).unwrap_or_default();
    w.component_fork = Some(state);
    result
}

pub fn begin(w: &mut Workspace, definition: &str, target: Target) {
    if w.tool_checkpoint.is_some()
        || w.session.draft.is_some()
        || w.sketch.is_some()
        || w.confirm_requested
        || w.assembly_motion.is_some()
        || w.settings.is_some()
    {
        w.status = "Apply or cancel the current operation before forking a component".into();
        return;
    }
    let baseline = w.session.committed.clone();
    let mut intent = Intent::new(&baseline, definition, target);
    // A Git pointer is exact only when both the saved model and working tree match it.
    if w.project.is_some()
        && w.saved_source == baseline.fingerprint()
        && cad_core::version_control::Git
            .command(&w.root(), &["status", "--porcelain"])
            .is_ok_and(|status| status.trim().is_empty())
    {
        intent.source_commit = cad_core::version_control::resolve_commit(&w.root(), "HEAD").ok();
    }
    let starting_view = w.view.clone();
    let starting_selection = w.selected.clone();
    let starting_component = w.selected_component.clone();
    w.start_tool();
    let expected = w.session.identity();
    w.component_fork = Some(State {
        baseline,
        starting_view,
        starting_selection,
        starting_component,
        intent: intent.clone(),
        accepted: None,
        expected,
        sequence: 0,
        result: None,
        error: None,
    });
    w.fields
        .insert("component-fork-editor".into(), intent.id.clone());
    w.json = serde_json::to_string_pretty(&intent).unwrap_or_default();
    w.open_editor("assembly");
    if let Err(error) = publish(w, intent) {
        w.status = error.to_string();
    }
}
pub fn begin_selected(w: &mut Workspace) {
    let document = w.session.visible();
    let path = w
        .selected_component
        .clone()
        .or_else(|| w.selected.as_ref().map(|selected| selected.path.clone()))
        .filter(|path| !path.is_empty());
    if let Some(path) = path {
        let mut definition = document.root.clone();
        for instance in &path {
            let Some(next) = document
                .assemblies
                .get(&definition)
                .and_then(|assembly| assembly.instances.get(instance))
            else {
                w.status = "Select a current component occurrence before forking".into();
                return;
            };
            definition = next.definition.clone();
        }
        begin(w, &definition, Target::Occurrence { path });
    } else {
        let definition = w
            .view
            .active_part
            .clone()
            .unwrap_or_else(|| document.root.clone());
        let target = if definition == document.root {
            Target::Root
        } else {
            Target::DefinitionOnly
        };
        begin(w, &definition, target);
    }
}
fn meta(state: &State) -> Value {
    json!({"contract":"tcad.presentation.component-fork","version":1,"document":state.baseline.id,
        "definition":state.intent.definition,"editor":state.intent.id,"sequence":state.sequence,
        "source_identity":state.expected,"shared_checkpoint":true,"requires_current_validation":true})
}
fn element(
    state: &State,
    key: &str,
    role: &str,
    label: &str,
    value: Option<String>,
    data: Value,
) -> SemanticElement {
    SemanticElement {
        id: format!("component-fork:{}/{key}", state.intent.id),
        role: role.into(),
        label: label.into(),
        value,
        state: "draft".into(),
        commands: vec![],
        relationships: vec![state.intent.definition.clone()],
        data,
    }
}
pub fn semantics(w: &Workspace) -> Vec<SemanticElement> {
    let Some(state) = w.component_fork.as_ref() else {
        return vec![];
    };
    let intent = parsed(w).unwrap_or_else(|_| state.intent.clone());
    let enabled = source_current(w) && w.settings.is_none();
    let mut out = vec![element(
        state,
        "group/source",
        "component-fork-group",
        "Independent component",
        None,
        meta(state),
    )];
    let scope = match &intent.target {
        Target::DefinitionOnly => {
            "Create a definition. Keep existing instances unchanged.".to_string()
        }
        Target::Root => "Continue with the fork as this project's root design.".to_string(),
        Target::Occurrence { path } => format!(
            "Replace one occurrence at {} level(s). Shared enclosing assemblies become independent along this path.",
            path.len()
        ),
    };
    out.push(element(
        state,
        "scope",
        "component-fork-help",
        &scope,
        None,
        meta(state),
    ));
    let source = serde_json::to_value(&intent).unwrap_or(Value::Null);
    for (pointer, label, choices) in [
        ("/name", "New name", vec![]),
        (
            "/mode",
            "Child components",
            vec![
                json!({"label":"Reuse children","value":"reuse_children"}),
                json!({"label":"Fork selected children","value":"fork_selected"}),
                json!({"label":"Fork all dependencies","value":"fork_all"}),
            ],
        ),
    ] {
        let value = source.pointer(pointer).cloned().unwrap_or(Value::Null);
        let mut data = meta(state);
        data["setter"] = json!("component-fork");
        data["pointer"] = json!(pointer);
        data["json"] = value.clone();
        data["editable"] = json!(enabled);
        data["choices"] = json!(choices);
        data["value_type"] = json!(if choices.is_empty() { "text" } else { "choice" });
        data["accessibility_label"] = json!(label);
        data["classes"] = json!(["modeling-field"]);
        let mut control = element(
            state,
            pointer.trim_start_matches('/'),
            "component-fork-field",
            label,
            Some(value.as_str().unwrap_or_default().into()),
            data,
        );
        if let Some(error) = w.fields.get(&format!("{ERRORS}{}", control.id)) {
            control.data["diagnostics"] = json!([error]);
        }
        if !enabled {
            control.state = "disabled".into();
        }
        out.push(control);
    }
    if intent.mode == Mode::ForkSelected {
        out.push(element(
            state,
            "group/children",
            "component-fork-group",
            "Choose independent children",
            None,
            meta(state),
        ));
        for definition in
            component_fork::descendants(&state.baseline, &intent.definition).unwrap_or_default()
        {
            let selected = intent.selected_definitions.contains(&definition);
            let label = format!(
                "{} {}",
                if selected { "✓" } else { "+" },
                cad_core::presentation::definition_name(&state.baseline, &definition)
            );
            let mut item = element(
                state,
                &format!("child/{definition}"),
                "component-fork-action",
                &label,
                None,
                meta(state),
            );
            item.commands = vec![ACTION.into()];
            item.data["element"] = json!(item.id);
            item.data["mutation"] = json!({"kind":"toggle_child","definition":definition});
            item.data["actions"] = json!([{"command":ACTION,"label":label,"enabled":enabled,"arguments":item.data.clone()}]);
            out.push(item);
        }
    }
    if let Some(plan) = &state.result {
        out.push(element(
            state,
            "group/preview",
            "component-fork-group",
            "Current proposal",
            None,
            meta(state),
        ));
        out.push(element(
            state,
            "summary",
            "component-fork-help",
            &format!(
                "{} independent definitions; {} reused dependencies",
                plan.copied.len(),
                plan.reused.len()
            ),
            None,
            meta(state),
        ));
        for (index, change) in plan.copied.iter().enumerate() {
            out.push(element(
                state,
                &format!("copy/{index}"),
                "component-fork-help",
                &format!(
                    "{}{}",
                    change.name,
                    if change.enclosing {
                        " · enclosing assembly"
                    } else {
                        ""
                    }
                ),
                None,
                meta(state),
            ));
        }
        for (index, notice) in plan.notices.iter().enumerate() {
            out.push(element(
                state,
                &format!("notice/{index}"),
                "component-fork-help",
                notice,
                None,
                meta(state),
            ));
        }
    }
    if let Some(error) = &state.error {
        let mut item = element(
            state,
            "error",
            "component-fork-help",
            "Cannot apply this fork",
            Some(error.clone()),
            meta(state),
        );
        item.state = "invalid".into();
        out.push(item);
    }
    out
}
fn live(w: &Workspace, supplied: &SemanticElement) -> Result<SemanticElement> {
    let fresh = semantics(w)
        .into_iter()
        .find(|element| element.id == supplied.id)
        .ok_or_else(|| invalid("This fork control is no longer available"))?;
    for key in [
        "document",
        "definition",
        "editor",
        "sequence",
        "source_identity",
    ] {
        if fresh.data[key] != supplied.data[key] {
            return Err(invalid("The fork draft changed; use the current control"));
        }
    }
    if !source_current(w) {
        return Err(invalid("The source changed outside this operation"));
    }
    Ok(fresh)
}
pub fn current_value(w: &Workspace, element: &SemanticElement) -> Option<Value> {
    live(w, element).ok()?;
    serde_json::to_value(parsed(w).ok()?)
        .ok()?
        .pointer(element.data["pointer"].as_str()?)
        .cloned()
}
pub fn field_is_invalid(w: &Workspace, element: &SemanticElement) -> bool {
    element.data["setter"] == "component-fork"
        && w.fields.contains_key(&format!("{ERRORS}{}", element.id))
}
pub fn field_error(w: &mut Workspace, element: &SemanticElement, error: Option<String>) {
    if element.data["setter"] != "component-fork" {
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
    let fresh = live(w, element)?;
    let pointer = fresh.data["pointer"]
        .as_str()
        .ok_or_else(|| invalid("Field pointer is missing"))?;
    if !matches!(pointer, "/name" | "/mode") {
        return Err(invalid("This fork field cannot be edited"));
    }
    let mut source = serde_json::to_value(parsed(w)?)?;
    *source
        .pointer_mut(pointer)
        .ok_or_else(|| invalid("Field is missing"))? = value;
    publish(w, serde_json::from_value(source)?)
}
pub fn action(w: &mut Workspace, command: &str, data: Option<&Value>) -> bool {
    if command == "component.fork" {
        begin_selected(w);
        return true;
    }
    if command == "component.fork-preview-source" {
        let result = parsed(w).and_then(|intent| publish(w, intent));
        if let Err(error) = result {
            w.status = error.to_string();
        } else {
            w.fields.retain(|key, _| !key.starts_with(ERRORS));
        }
        return true;
    }
    if command != ACTION {
        return false;
    }
    let result = (|| -> Result<()> {
        let data = data.ok_or_else(|| invalid("Choose a current fork action"))?;
        let item = semantics(w)
            .into_iter()
            .find(|item| Some(item.id.as_str()) == data["element"].as_str())
            .ok_or_else(|| invalid("The fork action is no longer available"))?;
        live(
            w,
            &SemanticElement {
                data: data.clone(),
                ..item.clone()
            },
        )?;
        if item.data["mutation"] != data["mutation"]
            || !item
                .action_descriptors()
                .iter()
                .any(|action| action.enabled)
        {
            return Err(invalid("The fork action changed"));
        }
        let id = data["mutation"]["definition"]
            .as_str()
            .ok_or_else(|| invalid("Child identity is missing"))?;
        let mut intent = parsed(w)?;
        if !intent.selected_definitions.remove(id) {
            intent.selected_definitions.insert(id.into());
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
            "component-fork-field" => crate::ui::semantic_field(ui, w, &element),
            "component-fork-action" => {
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
                .clicked()
                {
                    action(w, ACTION, Some(&element.data));
                }
            }
            _ => {
                controls::label(
                    ui,
                    &element.id,
                    &element.role,
                    &["modeling-group"],
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
        }
    }
    egui::CollapsingHeader::new("Advanced Source")
        .id_salt("component-fork-source")
        .show(ui, |ui| {
            ui.add(
                egui::TextEdit::multiline(&mut w.json)
                    .code_editor()
                    .desired_rows(8),
            );
            if controls::button(
                ui,
                "component-fork-preview-source",
                "component-fork-action",
                &[],
                "Preview edited fork source",
                source_current(w),
                false,
            )
            .clicked()
            {
                action(w, "component.fork-preview-source", None);
            }
        });
    crate::cad_ui::confirmation(ui, w);
}
