//! Saved camera-independent assembly visibility and optical arrangements.
use crate::{Workspace, presentation_controls as controls};
use bevy_egui::egui;
use cad_core::{
    CadError, Document, Id, Result,
    commands::Command,
    display_states::{Definition, Effect, Override, ScopedState, Selection, Visibility},
    presentation::SemanticElement,
};
use serde_json::{Value, json};
use std::collections::BTreeMap;
const EDITOR: &str = "display-state-editor";
const OWNER: &str = "display-state-owner";
const OWNER_PATH: &str = "display-state-owner-path";
const BEFORE: &str = "display-state-before";
const INVALID: &str = "display-state-source-invalid";
const TOKEN: &str = "display-state-editor-token";

#[derive(Clone, PartialEq, Eq)]
struct Stamp {
    document: Id,
    epoch: Id,
    generation: u64,
    revision: u64,
    draft: Option<(Id, u64)>,
    selection: Selection,
    configurations: BTreeMap<Id, Id>,
    original: bool,
}
#[derive(Default)]
pub struct Cache {
    stamp: Option<Stamp>,
    /// Actual authoritative source fingerprint computed for each new session stamp.
    source: String,
    scope: Option<ScopedState>,
    error: Option<String>,
    effects: BTreeMap<Vec<Id>, Effect>,
}
fn document(w: &Workspace) -> &Document {
    if w.view.original && w.view.history_cursor.is_none() {
        &w.session.committed
    } else {
        w.session.visible()
    }
}
fn refresh(w: &Workspace) {
    let Some(selection) = &w.view.display_state else {
        return;
    };
    let doc = document(w);
    let stamp = Stamp {
        document: doc.id.clone(),
        epoch: w.session.source_epoch().into(),
        generation: w.session.generation,
        revision: doc.revision,
        draft: w.session.draft.as_ref().map(|d| (d.id.clone(), d.edit)),
        selection: selection.clone(),
        configurations: doc.active_configurations.clone(),
        original: w.view.original,
    };
    let Ok(mut cache) = w.display_state_cache.lock() else {
        return;
    };
    if cache.stamp.as_ref() == Some(&stamp) {
        return;
    }
    cache.source = doc.fingerprint();
    match cad_core::display_states::scope(doc, selection) {
        Ok(scope) => {
            cache.scope = Some(scope);
            cache.error = None;
        }
        Err(error) => {
            cache.scope = None;
            cache.error = Some(error.to_string());
        }
    }
    cache.effects.clear();
    cache.stamp = Some(stamp);
}
pub fn effect(w: &Workspace, path: &[Id]) -> Effect {
    if w.view.display_state.is_none() {
        return Effect::default();
    }
    refresh(w);
    let Ok(mut cache) = w.display_state_cache.lock() else {
        return Effect::default();
    };
    if let Some(effect) = cache.effects.get(path) {
        return effect.clone();
    }
    let effect = cache
        .scope
        .as_ref()
        .map(|scope| cad_core::display_states::effect(scope, path))
        .unwrap_or_default();
    if cache.effects.len() < 65_536 {
        cache.effects.insert(path.to_vec(), effect.clone());
    }
    effect
}
pub fn diagnostics(w: &Workspace) -> Vec<String> {
    if w.view.display_state.is_none() {
        return vec![];
    }
    refresh(w);
    let Ok(cache) = w.display_state_cache.lock() else {
        return vec!["Display state cache is unavailable; normal visibility is shown".into()];
    };
    cache
        .error
        .iter()
        .cloned()
        .chain(
            cache
                .scope
                .iter()
                .flat_map(|scope| scope.diagnostics.iter().cloned()),
        )
        .collect()
}
pub fn pickable(w: &Workspace, path: &[Id]) -> bool {
    !matches!(
        effect(w, path).visibility,
        Some(Visibility::Hidden | Visibility::Ghost)
    )
}
fn parsed(w: &Workspace) -> Result<Definition> {
    let source: Definition = serde_json::from_str(&w.json)?;
    if w.fields.get(EDITOR) != Some(&source.id) {
        return Err(CadError::Invalid(
            "Display state identity cannot change in this editor".into(),
        ));
    }
    Ok(source)
}
fn owner_path(w: &Workspace) -> Vec<Id> {
    w.fields
        .get(OWNER_PATH)
        .and_then(|value| serde_json::from_str(value).ok())
        .unwrap_or_default()
}
pub fn current_source_is_valid(w: &Workspace) -> bool {
    if !w.fields.contains_key(EDITOR) {
        return true;
    }
    !w.fields.contains_key(INVALID)
        && parsed(w).is_ok_and(|source| {
            w.fields
                .get(OWNER)
                .and_then(|id| w.session.visible().assemblies.get(id))
                .is_some_and(|a| a.display_states.get(&source.id) == Some(&source))
        })
}
pub fn ready_to_confirm(w: &mut Workspace) -> bool {
    if !w.fields.contains_key(EDITOR) {
        return true;
    }
    if crate::render_appearance::resources_pending(w) {
        w.confirm_requested = true;
        w.status = "Waiting for current display-state texture resources".into();
        return false;
    }
    if !current_source_is_valid(w) || !w.appearance_diagnostics.is_empty() {
        w.confirm_requested = false;
        w.status = "Correct the display-state source or texture diagnostics before applying".into();
        return false;
    }
    true
}
fn submit(w: &mut Workspace, source: Definition) -> Result<()> {
    let owner = w
        .fields
        .get(OWNER)
        .cloned()
        .ok_or_else(|| CadError::Invalid("The display-state owner is missing".into()))?;
    let mut doc = w.session.visible().clone();
    Command::PutDisplayState {
        assembly: owner.clone(),
        state: Box::new(source.clone()),
    }
    .execute(&mut doc)?;
    let expected = doc.fingerprint();
    w.dispatch(Command::ReplaceDocument { document: doc });
    if w.session.visible().fingerprint() != expected {
        return Err(CadError::Invalid(w.status.clone()));
    }
    w.json = serde_json::to_string_pretty(&source)?;
    w.fields.remove(INVALID);
    w.view.display_state = Some(Selection {
        assembly: owner,
        state: source.id,
        owner_path: owner_path(w),
    });
    if w.selected
        .as_ref()
        .is_some_and(|selected| !pickable(w, &selected.path))
    {
        crate::exploded_views_ui::clear_selection(w);
    }
    w.geometry_selection = w
        .geometry_selection
        .iter()
        .filter(|selection| pickable(w, &selection.path))
        .cloned()
        .collect();
    w.hover_guide = None;
    w.hover_cache = Default::default();
    w.rendered_key.clear();
    Ok(())
}
fn owner_for(
    w: &Workspace,
    assembly: &str,
    state: &str,
    explicit: Option<Vec<Id>>,
) -> Result<Vec<Id>> {
    if let Some(path) = explicit {
        return Ok(path);
    }
    if assembly == w.session.visible().root {
        return Ok(vec![]);
    }
    if let Some(active) = &w.view.display_state
        && active.assembly == assembly
    {
        return Ok(active.owner_path.clone());
    }
    let path = w
        .selected
        .as_ref()
        .map(|s| s.path.clone())
        .or_else(|| w.edge_selection_path.clone())
        .unwrap_or_default();
    for count in (0..=path.len()).rev() {
        let selection = Selection {
            assembly: assembly.into(),
            state: state.into(),
            owner_path: path[..count].to_vec(),
        };
        if cad_core::display_states::scope(w.session.visible(), &selection).is_ok() {
            return Ok(selection.owner_path);
        }
    }
    Err(CadError::Invalid("Select an occurrence of the owning assembly or supply its full owner_path before showing this state".into()))
}
pub fn activate(w: &mut Workspace, selection: Option<Selection>) -> Result<()> {
    if let Some(selection) = &selection {
        let scope = cad_core::display_states::scope(w.session.visible(), selection)?;
        if !scope.applicable {
            return Err(CadError::Invalid(scope.diagnostics.join("\n")));
        }
    }
    w.view.display_state = selection;
    crate::exploded_views_ui::clear_selection(w);
    w.geometry_selection.clear();
    w.hover_guide = None;
    w.hover_cache = Default::default();
    w.rendered_key.clear();
    // No document command, evaluation request, mechanical solve or camera move.
    Ok(())
}
pub fn begin(w: &mut Workspace, assembly: &str, source: Definition, path: Vec<Id>) -> Result<()> {
    if w.sketch.is_some() || w.tool_checkpoint.is_some() || w.dialog.as_deref() == Some("feature") {
        return Err(CadError::Invalid(
            "Confirm or cancel the current operation before editing a display state".into(),
        ));
    }
    if !w.session.visible().assemblies.contains_key(assembly) {
        return Err(CadError::Invalid(
            "Display states belong to assemblies".into(),
        ));
    }
    let before = serde_json::to_string(&w.view.display_state)?;
    let existing = w
        .session
        .visible()
        .assemblies
        .get(assembly)
        .and_then(|a| a.display_states.get(&source.id))
        == Some(&source);
    let exploded = w.view.exploded_view.clone();
    let selected_path = w
        .selected
        .as_ref()
        .map(|s| s.path.clone())
        .or_else(|| w.edge_selection_path.clone());
    w.start_tool();
    w.view.exploded_view = exploded;
    crate::exploded_views_ui::clear_selection(w);
    w.geometry_selection.clear();
    w.edge_selection_path = selected_path;
    w.fields.insert(BEFORE.into(), before);
    w.fields.insert(OWNER.into(), assembly.into());
    w.fields
        .insert(OWNER_PATH.into(), serde_json::to_string(&path)?);
    w.fields.insert(EDITOR.into(), source.id.clone());
    w.fields.insert(TOKEN.into(), cad_core::id());
    w.fields.insert("assembly-id".into(), assembly.into());
    w.json = serde_json::to_string_pretty(&source)?;
    w.open_editor("assembly");
    if existing {
        activate(
            w,
            Some(Selection {
                assembly: assembly.into(),
                state: source.id,
                owner_path: path,
            }),
        )
    } else {
        submit(w, source)
    }
}
pub fn new(w: &mut Workspace) {
    let assembly = w.session.visible().root.clone();
    let Some(owner) = w.session.visible().assemblies.get(&assembly) else {
        return;
    };
    let name = (1..)
        .map(|index| format!("Display State{index}"))
        .find(|name| {
            !owner
                .display_states
                .values()
                .any(|state| state.name.eq_ignore_ascii_case(name))
        })
        .unwrap();
    if let Err(error) = begin(w, &assembly, Definition::new(name), vec![]) {
        w.status = error.to_string();
    }
}
pub fn cancel(w: &mut Workspace) {
    if let Some(before) = w.fields.remove(BEFORE) {
        w.view.display_state = serde_json::from_str(&before).ok().flatten();
        crate::exploded_views_ui::clear_selection(w);
        w.geometry_selection.clear();
        w.rendered_key.clear();
    }
    for field in [EDITOR, OWNER, OWNER_PATH, INVALID, TOKEN] {
        w.fields.remove(field);
    }
}
fn component_choices(w: &Workspace) -> Vec<(Vec<Id>, String)> {
    struct Walker<'a> {
        source: &'a Document,
        prefix: Vec<Id>,
        choices: BTreeMap<Vec<Id>, String>,
        work: usize,
    }
    impl Walker<'_> {
        fn walk(&mut self, context: &Document, owner: &str, path: &[Id], label: &str) {
            if path.len() >= 64 || self.work >= 16_384 || self.choices.len() >= 4096 {
                return;
            }
            let Some(assembly) = context.assemblies.get(owner) else {
                return;
            };
            for instance in assembly.instances.values() {
                self.work += 1;
                if self.work > 16_384 || self.choices.len() >= 4096 {
                    return;
                }
                let mut child = path.to_vec();
                child.push(instance.id.clone());
                if !child.starts_with(&self.prefix) && !self.prefix.starts_with(&child) {
                    continue;
                }
                let name = if label.is_empty() {
                    instance.name.clone()
                } else {
                    format!("{label} / {}", instance.name)
                };
                if child.starts_with(&self.prefix) && child.len() > self.prefix.len() {
                    self.choices
                        .insert(child[self.prefix.len()..].to_vec(), name.clone());
                }
                if context.assemblies.contains_key(&instance.definition) {
                    let selected = cad_core::configuration::selected(
                        context,
                        &instance.definition,
                        instance.configuration.as_ref(),
                    );
                    if let Ok(scoped) = cad_core::configuration::scope(
                        self.source,
                        context,
                        &instance.definition,
                        selected.map(String::as_str),
                    ) {
                        self.walk(&scoped, &instance.definition, &child, &name);
                    }
                }
            }
        }
    }
    let source = w.session.visible();
    let mut walker = Walker {
        source,
        prefix: owner_path(w),
        choices: BTreeMap::new(),
        work: 0,
    };
    if let Ok(context) = cad_core::configuration::effective(source) {
        walker.walk(&context, &source.root, &[], "");
    }
    walker.choices.into_iter().collect()
}
fn field(id: String, label: &str, value: Value, pointer: &str, choices: Value) -> SemanticElement {
    SemanticElement {
        id,
        role: "display-state-field".into(),
        label: label.into(),
        value: Some(value.to_string()),
        state: "draft".into(),
        commands: vec!["assembly.display-state-edit".into()],
        relationships: vec![],
        data: json!({"contract":"tcad.display-states","version":1,"setter":"display-state-field","field":pointer,"json":value,"editable":true,"choices":choices,"value_type":if choices.as_array().is_some_and(|a|!a.is_empty()) {"choice"} else {"text"}}),
    }
}
fn action(id: &str, label: &str, command: &str, data: Value, enabled: bool) -> SemanticElement {
    SemanticElement {
        id: id.into(),
        role: "display-state-action".into(),
        label: label.into(),
        value: None,
        state: if enabled { "enabled" } else { "disabled" }.into(),
        commands: vec![command.into()],
        relationships: vec![],
        data,
    }
}
fn picker(w: &Workspace) -> SemanticElement {
    let doc = w.session.visible();
    let mut choices = vec![json!({"value":null,"label":"Normal Display"})];
    if let Some(root) = doc.assemblies.get(&doc.root) {
        choices.extend(root.display_states.values().map(|state|json!({"value":Selection { assembly:root.id.clone(),state:state.id.clone(),owner_path:vec![] },"label":state.name})));
    }
    if let Some(active) = &w.view.display_state
        && !active.owner_path.is_empty()
    {
        choices.push(json!({"value":active,"label":doc.assemblies.get(&active.assembly).and_then(|a|a.display_states.get(&active.state)).map(|s|s.name.as_str()).unwrap_or("Unavailable display state")}));
    }
    SemanticElement {
        id: "workspace.display-state".into(),
        role: "display-state-picker".into(),
        label: "Display".into(),
        value: Some(serde_json::to_string(&w.view.display_state).unwrap_or_default()),
        state: "ready".into(),
        commands: vec![
            "assembly.display-state-show".into(),
            "assembly.display-state-normal".into(),
        ],
        relationships: vec![doc.root.clone()],
        data: json!({"contract":"tcad.display-states","version":1,"setter":"display-state-selection","json":w.view.display_state,"editable":true,"choices":choices,"value_type":"choice","presentation_only":true,"geometry_rebuild":false,"camera_changes":false}),
    }
}
pub fn elements(w: &Workspace) -> Vec<SemanticElement> {
    let mut elements = cad_core::display_states::semantic(
        w.session.visible(),
        w.view.display_state.as_ref(),
        "active",
    );
    elements.push(picker(w));
    elements.push(SemanticElement { id:"display-states:help".into(),role:"display-state-description".into(),label:"Saved visibility and finishes do not move components, change mass, solve mates or alter CAD exports. Ghost components can be selected from the tree; viewport picking ignores them.".into(),value:None,state:"active".into(),commands:vec![],relationships:vec![],data:json!({"contract":"tcad.display-states","version":1,"ghost_pickable":false}) });
    for (index, message) in diagnostics(w).into_iter().enumerate() {
        elements.push(SemanticElement { id:format!("display-state:diagnostic:{index}"),role:"display-state-diagnostic".into(),label:message,value:None,state:"warning".into(),commands:vec!["assembly.display-state-normal".into()],relationships:vec![],data:json!({"contract":"tcad.display-states","version":1,"selection":w.view.display_state,"fallback":"normal-display"}) });
    }
    let Some(editor_id) = w.fields.get(EDITOR) else {
        return elements;
    };
    let Some(owner) = w.fields.get(OWNER) else {
        return elements;
    };
    let identity = format!("{owner}::display-state:{editor_id}");
    elements.push(SemanticElement {
        id: format!("{identity}::source"), role: "display-state-source".into(),
        label: "Advanced source".into(), value: Some(w.json.clone()),
        state: if w.fields.contains_key(INVALID) { "invalid" } else { "draft" }.into(),
        commands: vec!["assembly.display-state-edit".into()], relationships: vec![identity.clone()],
        data: json!({"setter":"display-state-source","json":w.json,"editable":true,"value_type":"text","multiline":true,"rows":12,"editor":editor_id,"editor_token":w.fields.get(TOKEN)}),
    });
    let Ok(source) = parsed(w) else {
        return elements;
    };
    let Some(assembly) = w.session.visible().assemblies.get(owner) else {
        return elements;
    };
    let mut configurations = vec![json!({"value":null,"label":"All configurations"})];
    configurations.extend(
        cad_core::configuration::names(w.session.visible(), &assembly.id)
            .into_iter()
            .map(|(id, name)| json!({"value":id,"label":name})),
    );
    elements.push(field(
        format!("{identity}::name"),
        "Name",
        json!(source.name),
        "/name",
        json!([]),
    ));
    elements.push(field(
        format!("{identity}::configuration"),
        "Scope",
        json!(source.configuration),
        "/configuration",
        json!(configurations),
    ));
    let component_choices = component_choices(w)
        .into_iter()
        .map(|(path, label)| json!({"value":path,"label":label}))
        .collect::<Vec<_>>();
    let finish_choices = std::iter::once(json!({"value":null,"label":"Inherit finish"}))
        .chain(
            w.session
                .visible()
                .finishes
                .values()
                .map(|finish| json!({"value":finish.id,"label":finish.name})),
        )
        .collect::<Vec<_>>();
    for (index, target) in source.overrides.iter().enumerate() {
        let first = elements.len();
        let prefix = format!("{identity}::override:{}", target.id);
        elements.push(field(
            format!("{prefix}::path"),
            "Component path",
            json!(target.path),
            &format!("/overrides/{index}/path"),
            json!(component_choices),
        ));
        elements.push(field(format!("{prefix}::visibility"),"Visibility",json!(target.visibility),&format!("/overrides/{index}/visibility"),json!([{"value":null,"label":"Inherit visibility"},{"value":"visible","label":"Visible"},{"value":"hidden","label":"Hidden"},{"value":"ghost","label":"Ghost · not pickable"}])));
        elements.push(field(
            format!("{prefix}::finish"),
            "Surface finish",
            json!(target.finish),
            &format!("/overrides/{index}/finish"),
            json!(finish_choices),
        ));
        for element in &mut elements[first..] {
            element.data["override"] = json!(target.id);
            element.data["override_field"] = json!(
                element.data["field"]
                    .as_str()
                    .and_then(|field| field.rsplit('/').next())
                    .unwrap_or_default()
            );
        }
        elements.push(action(
            &format!("{prefix}::remove"),
            "Remove override",
            "assembly.display-state-remove-target",
            json!({"override":target.id}),
            true,
        ));
    }
    elements.push(action(
        &format!("{identity}::add-selected"),
        "Add selected component",
        "assembly.display-state-add-selected",
        json!({}),
        w.selected.is_some() || w.edge_selection_path.is_some(),
    ));
    elements.push(action(
        &format!("{identity}::add"),
        "Add component override",
        "assembly.display-state-add-target",
        json!({}),
        !component_choices.is_empty(),
    ));
    for element in &mut elements {
        if element.role.starts_with("display-state-") {
            element.data["editor"] = json!(source.id);
            element.data["editor_token"] = json!(w.fields.get(TOKEN));
        }
    }
    elements
}
fn field_pointer(source: &Definition, element: &SemanticElement) -> Result<String> {
    if element.data.get("editor").and_then(Value::as_str) != Some(source.id.as_str()) {
        return Err(CadError::Invalid(
            "This field belongs to a different display-state editor".into(),
        ));
    }
    if let Some(id) = element.data.get("override").and_then(Value::as_str) {
        let index = source
            .overrides
            .iter()
            .position(|target| target.id == id)
            .ok_or_else(|| {
                CadError::Invalid(
                    "The edited component override was removed; refresh the control".into(),
                )
            })?;
        let field = element
            .data
            .get("override_field")
            .and_then(Value::as_str)
            .filter(|field| matches!(*field, "path" | "visibility" | "finish"))
            .ok_or_else(|| CadError::Invalid("Invalid display-state component field".into()))?;
        return Ok(format!("/overrides/{index}/{field}"));
    }
    element
        .data
        .get("field")
        .and_then(Value::as_str)
        .filter(|field| matches!(*field, "/name" | "/configuration"))
        .map(str::to_owned)
        .ok_or_else(|| CadError::Invalid("Invalid display-state field".into()))
}
pub fn current_value(w: &Workspace, element: &SemanticElement) -> Option<Value> {
    match element.data.get("setter")?.as_str()? {
        "display-state-selection" => Some(json!(w.view.display_state)),
        "display-state-source" => Some(json!(w.json)),
        "display-state-field" => {
            let source = parsed(w).ok()?;
            serde_json::to_value(&source)
                .ok()?
                .pointer(&field_pointer(&source, element).ok()?)
                .cloned()
                .or_else(|| element.data.get("json").cloned())
        }
        _ => None,
    }
}
pub fn set_semantic(w: &mut Workspace, element: &SemanticElement, value: Value) -> bool {
    let setter = element
        .data
        .get("setter")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if setter == "display-state-selection" {
        let result = serde_json::from_value::<Option<Selection>>(value)
            .map_err(CadError::from)
            .and_then(|selection| activate(w, selection));
        if let Err(error) = result {
            w.status = error.to_string();
        }
        return true;
    }
    if !matches!(setter, "display-state-source" | "display-state-field") {
        return false;
    }
    if element.data.get("editor_token").and_then(Value::as_str)
        != w.fields.get(TOKEN).map(String::as_str)
    {
        w.status =
            "This field belongs to an earlier display-state editor; refresh the control".into();
        return true;
    }
    let result = (|| -> Result<()> {
        if setter == "display-state-source" {
            w.json = value
                .as_str()
                .ok_or_else(|| CadError::Invalid("Display state source requires text".into()))?
                .into();
            return submit(w, parsed(w)?);
        }
        let definition = parsed(w)?;
        let pointer = field_pointer(&definition, element)?;
        let mut source = serde_json::to_value(definition)?;
        source["configuration"] = source.get("configuration").cloned().unwrap_or(Value::Null);
        if let Some(overrides) = source.get_mut("overrides").and_then(Value::as_array_mut) {
            for target in overrides {
                target["visibility"] = target.get("visibility").cloned().unwrap_or(Value::Null);
                target["finish"] = target.get("finish").cloned().unwrap_or(Value::Null);
            }
        }
        *source.pointer_mut(&pointer).ok_or_else(|| {
            CadError::Invalid("The display state field no longer exists".into())
        })? = value;
        w.json = serde_json::to_string_pretty(&source)?;
        submit(w, serde_json::from_value(source)?)
    })();
    if let Err(error) = result {
        w.fields.insert(INVALID.into(), error.to_string());
        w.status = error.to_string();
    }
    true
}
pub fn handle_command(w: &mut Workspace, command: &str, args: Option<&Value>) -> bool {
    if !command.starts_with("assembly.display-state-") {
        return false;
    }
    let result = (|| -> Result<()> {
        if let Some(token) = args
            .and_then(|args| args.get("editor_token"))
            .and_then(Value::as_str)
            && w.fields.get(TOKEN).map(String::as_str) != Some(token)
        {
            return Err(CadError::Invalid(
                "This action belongs to an earlier display-state editor".into(),
            ));
        }
        if let Some(editor) = args
            .and_then(|args| args.get("editor"))
            .and_then(Value::as_str)
            && w.fields.get(EDITOR).map(String::as_str) != Some(editor)
        {
            return Err(CadError::Invalid(
                "This action belongs to a different display-state editor".into(),
            ));
        }
        if command == "assembly.display-state-new" {
            new(w);
            return Ok(());
        }
        if command == "assembly.display-state-normal" {
            return activate(w, None);
        }
        if matches!(
            command,
            "assembly.display-state-add-selected"
                | "assembly.display-state-add-target"
                | "assembly.display-state-remove-target"
        ) {
            let mut source = parsed(w)?;
            if command == "assembly.display-state-remove-target" {
                let id = args
                    .and_then(|args| args.get("override"))
                    .and_then(Value::as_str)
                    .ok_or_else(|| {
                        CadError::Invalid("Choose a component override to remove".into())
                    })?;
                source.overrides.retain(|target| target.id != id);
            } else {
                let prefix = owner_path(w);
                let path = if command == "assembly.display-state-add-selected" {
                    let path = w
                        .selected
                        .as_ref()
                        .map(|s| s.path.clone())
                        .or_else(|| w.edge_selection_path.clone())
                        .ok_or_else(|| CadError::Invalid("Select a component first".into()))?;
                    path.strip_prefix(prefix.as_slice())
                        .filter(|path| !path.is_empty())
                        .map(<[Id]>::to_vec)
                        .ok_or_else(|| {
                            CadError::Invalid(
                                "The selected component is outside this assembly occurrence".into(),
                            )
                        })?
                } else {
                    component_choices(w)
                        .into_iter()
                        .find(|(path, _)| {
                            !source.overrides.iter().any(|target| target.path == *path)
                        })
                        .map(|(path, _)| path)
                        .ok_or_else(|| {
                            CadError::Invalid(
                                "Every available component already has an override".into(),
                            )
                        })?
                };
                if !source.overrides.iter().any(|target| target.path == path) {
                    source.overrides.push(Override {
                        id: cad_core::id(),
                        path,
                        visibility: Some(Visibility::Visible),
                        finish: None,
                    });
                }
            }
            return submit(w, source);
        }
        let assembly = args
            .and_then(|args| args.get("assembly"))
            .and_then(Value::as_str)
            .map(str::to_owned)
            .or_else(|| w.fields.get(OWNER).cloned())
            .unwrap_or_else(|| w.session.visible().root.clone());
        let state = args
            .and_then(|args| args.get("state"))
            .and_then(Value::as_str)
            .map(str::to_owned)
            .or_else(|| w.fields.get(EDITOR).cloned())
            .or_else(|| {
                w.view
                    .display_state
                    .as_ref()
                    .map(|selection| selection.state.clone())
            })
            .ok_or_else(|| CadError::Invalid("Choose a saved display state".into()))?;
        let source = w
            .session
            .visible()
            .assemblies
            .get(&assembly)
            .and_then(|a| a.display_states.get(&state))
            .ok_or_else(|| CadError::Invalid("The selected display state no longer exists".into()))?
            .clone();
        let explicit = args
            .and_then(|args| args.get("owner_path"))
            .map(|path| serde_json::from_value(path.clone()))
            .transpose()?;
        let path = owner_for(w, &assembly, &state, explicit)?;
        match command {
            "assembly.display-state-show" => activate(
                w,
                Some(Selection {
                    assembly,
                    state,
                    owner_path: path,
                }),
            )?,
            "assembly.display-state-edit" => begin(w, &assembly, source, path)?,
            "assembly.display-state-duplicate" => {
                let mut source = source;
                source.id = cad_core::id();
                let names = &w.session.visible().assemblies[&assembly].display_states;
                source.name = (1..)
                    .map(|index| format!("{} Copy{index}", source.name))
                    .find(|name| {
                        !names
                            .values()
                            .any(|state| state.name.eq_ignore_ascii_case(name))
                    })
                    .unwrap();
                for target in &mut source.overrides {
                    target.id = cad_core::id();
                }
                begin(w, &assembly, source, path)?;
            }
            "assembly.display-state-delete" => {
                if w.tool_checkpoint.is_some() {
                    return Err(CadError::Invalid("Confirm or cancel the current editor before deleting a saved display state".into()));
                }
                let command = Command::DeleteDisplayState {
                    assembly: assembly.clone(),
                    state: state.clone(),
                };
                let mut staged = w.session.visible().clone();
                command.clone().execute(&mut staged)?;
                w.start_tool();
                w.fields
                    .insert(BEFORE.into(), serde_json::to_string(&w.view.display_state)?);
                w.dispatch(command);
                if w.session
                    .visible()
                    .assemblies
                    .get(&assembly)
                    .is_some_and(|a| a.display_states.contains_key(&state))
                {
                    return Err(CadError::Invalid(w.status.clone()));
                }
                if w.view
                    .display_state
                    .as_ref()
                    .is_some_and(|active| active.assembly == assembly && active.state == state)
                {
                    activate(w, None)?;
                }
                w.status =
                    "Display state deletion is pending; Apply or Cancel the shared draft".into();
            }
            "assembly.display-state-select-target" => {
                let target = args
                    .and_then(|args| args.get("override"))
                    .and_then(Value::as_str)
                    .and_then(|id| source.overrides.iter().find(|target| target.id == id))
                    .ok_or_else(|| {
                        CadError::Invalid("Select an existing component override".into())
                    })?;
                let mut selected = path;
                selected.extend(target.path.clone());
                w.edge_selection_path = Some(selected);
                w.rendered_key.clear();
            }
            _ => {
                return Err(CadError::Invalid(format!(
                    "Unknown display state command: {command}"
                )));
            }
        }
        Ok(())
    })();
    if let Err(error) = result {
        w.status = error.to_string();
    }
    true
}
pub fn semantic_action(w: &mut Workspace, element: &SemanticElement, command: &str) {
    handle_command(w, command, Some(&element.data));
}
fn action_button(ui: &mut egui::Ui, w: &mut Workspace, element: &SemanticElement) {
    if controls::button(
        ui,
        &element.id,
        &element.role,
        &[],
        &element.label,
        element.state != "disabled",
        element.state == "selected",
    )
    .clicked()
        && let Some(command) = element.commands.first()
    {
        handle_command(w, command, Some(&element.data));
    }
}
pub fn show(ui: &mut egui::Ui, w: &mut Workspace, assembly: &str) {
    if !w.fields.contains_key(EDITOR) {
        controls::label(
            ui,
            "display-states:title",
            "display-state-title",
            &[],
            "Display States",
            "active",
        );
        let states = w
            .session
            .visible()
            .assemblies
            .get(assembly)
            .map(|a| a.display_states.values().cloned().collect::<Vec<_>>())
            .unwrap_or_default();
        for source in states {
            ui.horizontal_wrapped(|ui| {
                controls::label(
                    ui,
                    &format!("display-state:{}:name", source.id),
                    "display-state-name",
                    &[],
                    &source.name,
                    "active",
                );
                for (label, command) in [
                    ("Show", "assembly.display-state-show"),
                    ("Edit", "assembly.display-state-edit"),
                    ("Copy", "assembly.display-state-duplicate"),
                    ("Delete", "assembly.display-state-delete"),
                ] {
                    action_button(
                        ui,
                        w,
                        &action(
                            &format!("display-state:{}:{command}", source.id),
                            label,
                            command,
                            json!({"assembly":assembly,"state":source.id}),
                            true,
                        ),
                    );
                }
            });
        }
        ui.horizontal_wrapped(|ui| {
            action_button(
                ui,
                w,
                &action(
                    "display-states:new",
                    "New Display State",
                    "assembly.display-state-new",
                    json!({}),
                    true,
                ),
            );
            action_button(
                ui,
                w,
                &action(
                    "display-states:normal",
                    "Normal Display",
                    "assembly.display-state-normal",
                    json!({}),
                    true,
                ),
            );
        });
    } else {
        let elements = elements(w);
        for element in elements.iter().filter(|element| {
            element.role == "display-state-field" || element.role == "display-state-action"
        }) {
            if element.role == "display-state-field" {
                crate::ui::semantic_field(ui, w, element);
            } else {
                action_button(ui, w, element);
            }
        }
        ui.collapsing("Advanced source", |ui| {
            if let Some(source) = elements
                .iter()
                .find(|element| element.role == "display-state-source")
            {
                crate::ui::semantic_field(ui, w, source);
            }
        });
        if let Some(error) = w.fields.get(INVALID) {
            ui.colored_label(ui.visuals().error_fg_color, error);
        }
    }
    controls::label(
        ui,
        "display-states:help",
        "display-state-description",
        &[],
        "Saved visibility and finishes do not move components, change mass, solve mates or alter CAD exports. Ghost components can be selected from the tree; viewport picking ignores them.",
        "active",
    );
    for (index, message) in diagnostics(w).iter().enumerate() {
        controls::label(
            ui,
            &format!("display-state:diagnostic:{index}"),
            "display-state-diagnostic",
            &[],
            message,
            "warning",
        );
    }
}
pub fn view_controls(ui: &mut egui::Ui, w: &mut Workspace) {
    let has_states = w
        .session
        .visible()
        .assemblies
        .get(&w.session.visible().root)
        .is_some_and(|a| !a.display_states.is_empty());
    if has_states || w.view.display_state.is_some() {
        crate::ui::semantic_field(ui, w, &picker(w));
    }
}
