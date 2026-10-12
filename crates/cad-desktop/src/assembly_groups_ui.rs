//! Styleable rigid-group editing, current-pose capture and source-owned solver diagnostics.
use crate::{Workspace, presentation_controls as controls, viewport};
use bevy_egui::egui;
use cad_core::{
    CadError, Id, Placement, Result,
    assembly_groups::{self, Definition},
    commands::Command,
    presentation::SemanticElement,
};
use serde_json::{Value, json};
const EDITOR: &str = "assembly-group-editor";
const INVALID: &str = "assembly-group-source-invalid";
const ERRORS: &str = "assembly-group-field-error:";
const ACTION: &str = "assembly.group-action";
fn invalid(message: &str) -> CadError {
    CadError::Invalid(format!("Rigid Group: {message}"))
}
fn owner(w: &Workspace) -> Option<&str> {
    w.fields.get("assembly-id").map(String::as_str)
}
fn context(w: &Workspace) -> String {
    let doc = w.session.visible();
    json!({"document":doc.id,"root":doc.root,"configuration":doc.active_configurations,"instances":doc.assemblies.values().map(|a|(&a.id,a.instances.values().map(|i|(&i.id,&i.definition,&i.configuration)).collect::<Vec<_>>())).collect::<Vec<_>>()}).to_string()
}
fn parsed(w: &Workspace) -> Result<Definition> {
    let source: Definition = serde_json::from_str(&w.json)?;
    if w.fields.get(EDITOR) != Some(&source.id) {
        return Err(invalid("Saved group identity cannot change"));
    }
    Ok(source)
}
fn source_is_current(w: &Workspace) -> bool {
    if !w.fields.contains_key(EDITOR) {
        return true;
    }
    w.fields.get("assembly-group-context") == Some(&context(w))
        && parsed(w).is_ok_and(|source| {
            owner(w)
                .and_then(|owner| w.session.visible().assemblies.get(owner))
                .is_some_and(|assembly| {
                    if w.fields.contains_key("assembly-group-deletion") {
                        !assembly.rigid_groups.contains_key(&source.id)
                    } else {
                        assembly.rigid_groups.get(&source.id) == Some(&source)
                    }
                })
        })
}
pub fn current_source_is_valid(w: &Workspace) -> bool {
    source_is_current(w) && !w.fields.keys().any(|key| key.starts_with(ERRORS))
}
fn guard(w: &mut Workspace) {
    if source_is_current(w) {
        w.fields.remove(INVALID);
    } else {
        w.fields.insert(INVALID.into(), "true".into());
    }
}
pub fn cancel(w: &mut Workspace) {
    w.fields
        .retain(|key, _| !key.starts_with("assembly-group-") && !key.starts_with(ERRORS));
}
fn busy(w: &Workspace) -> bool {
    w.tool_checkpoint.is_some()
        || w.sketch.is_some()
        || w.session.draft.is_some()
        || w.confirm_requested
        || w.assembly_motion.is_some()
        || w.settings.is_some()
}
fn evaluation(w: &Workspace) -> Option<&cad_core::protocol::Evaluation> {
    viewport::display_evaluation(w).filter(|e| e.valid && e.identity == w.session.identity())
}
fn choices(w: &Workspace, assembly: &str, id: &str) -> Vec<(Id, String)> {
    let doc = w.session.visible();
    let config = doc.active_configurations.get(assembly);
    let scoped =
        match cad_core::configuration::scope(doc, doc, assembly, config.map(String::as_str)) {
            Ok(doc) => doc,
            Err(_) => return vec![],
        };
    let Some(owner) = scoped.assemblies.get(assembly) else {
        return vec![];
    };
    let generated = cad_core::component_pattern::generated(owner);
    owner
        .instances
        .values()
        .filter(|i| {
            !generated.contains(&i.id)
                && !owner
                    .rigid_groups
                    .values()
                    .any(|group| group.id != id && group.members.contains_key(&i.id))
        })
        .map(|i| (i.id.clone(), i.name.clone()))
        .collect()
}
fn start(w: &mut Workspace, assembly: &str, source: Definition) -> bool {
    if busy(w) {
        w.status = "Confirm or cancel the current operation before editing a rigid group".into();
        return false;
    }
    if !w.session.visible().assemblies.contains_key(assembly) {
        w.status = "Owning assembly is missing".into();
        return false;
    }
    let snapshot = context(w);
    w.start_tool();
    w.fields.insert("assembly-id".into(), assembly.into());
    w.fields.insert(EDITOR.into(), source.id.clone());
    w.fields.insert("assembly-group-context".into(), snapshot);
    w.fields
        .insert("assembly-group-generation".into(), cad_core::id());
    w.json = serde_json::to_string_pretty(&source).unwrap_or_default();
    w.open_editor("assembly");
    guard(w);
    true
}
fn submit(w: &mut Workspace, source: Definition) -> Result<()> {
    if w.fields.get("assembly-group-context") != Some(&context(w)) {
        return Err(invalid(
            "Assembly or configuration changed; cancel and reopen this group",
        ));
    }
    if w.fields.get(EDITOR) != Some(&source.id) {
        return Err(invalid("Saved group identity cannot change"));
    }
    // Keep invalid authored text available for correction without changing definitions.
    w.json = serde_json::to_string_pretty(&source)?;
    let result = (|| {
        let assembly = owner(w)
            .ok_or_else(|| invalid("Owning assembly is missing"))?
            .to_string();
        let mut doc = w.session.visible().clone();
        (Command::PutAssemblyGroup {
            assembly,
            group: Box::new(source.clone()),
        })
        .execute(&mut doc)?;
        w.dispatch(Command::ReplaceDocument { document: doc });
        if !source_is_current(w) {
            return Err(invalid(&w.status));
        }
        Ok(())
    })();
    guard(w);
    result
}
pub fn new(w: &mut Workspace, assembly: &str) {
    let doc = w.session.visible();
    let mut count = doc
        .assemblies
        .get(assembly)
        .map(|owner| owner.rigid_groups.len())
        .unwrap_or_default()
        + 1;
    while doc.assemblies.get(assembly).is_some_and(|owner| {
        owner.rigid_groups.values().any(|group| {
            group
                .name
                .eq_ignore_ascii_case(&format!("Rigid Group {count}"))
        })
    }) {
        count += 1;
    }
    let mut source = Definition::new(format!("Rigid Group {count}"));
    let participants = w
        .geometry_selection
        .iter()
        .map(|selection| selection.path.first().cloned())
        .chain(
            w.selected
                .as_ref()
                .map(|selection| selection.path.first().cloned()),
        )
        .flatten()
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    if assembly == doc.root
        && participants.len() >= 2
        && let Some(e) = evaluation(w)
    {
        match assembly_groups::capture(
            doc,
            e,
            assembly,
            source.clone(),
            &participants,
            &participants[0],
        ) {
            Ok(captured) => source = captured,
            Err(error) => {
                w.status = error.to_string();
                return;
            }
        }
    }
    if start(w, assembly, source.clone())
        && source.members.len() >= 2
        && let Err(error) = submit(w, source)
    {
        w.status = error.to_string();
    }
}
pub fn open(w: &mut Workspace, assembly: &str, id: &str) {
    if let Some(source) = w
        .session
        .visible()
        .assemblies
        .get(assembly)
        .and_then(|owner| owner.rigid_groups.get(id))
        .cloned()
    {
        start(w, assembly, source);
    } else {
        w.status = "Saved rigid group is missing".into();
    }
}
fn revision(w: &Workspace) -> Value {
    json!(w.session.identity())
}
fn metadata(w: &Workspace) -> Value {
    json!({"document":w.session.visible().id,"assembly":owner(w),"group":w.fields.get(EDITOR),"editor":w.fields.get("assembly-group-generation"),"source_revision":revision(w),"draft_source":w.json})
}
fn identity(w: &Workspace, key: &str) -> String {
    format!(
        "assembly-group-editor:{}/{key}",
        w.fields.get(EDITOR).map(String::as_str).unwrap_or("none")
    )
}
fn group(w: &Workspace, out: &mut Vec<SemanticElement>, key: &str, label: &str) {
    let mut data = metadata(w);
    data["contract"] = json!("tcad.assembly-groups");
    out.push(SemanticElement {
        id: identity(w, key),
        role: "assembly-group-section".into(),
        label: label.into(),
        value: None,
        state: "ready".into(),
        commands: vec![],
        relationships: vec![],
        data,
    });
}
fn field(
    w: &Workspace,
    out: &mut Vec<SemanticElement>,
    source: &Value,
    path: &str,
    label: &str,
    kind: &str,
    options: Vec<Value>,
) {
    let mut data = metadata(w);
    data["setter"] = json!("assembly-group");
    data["pointer"] = json!(path);
    data["field"] = json!(path);
    data["type"] = json!(kind);
    data["value_type"] = json!(kind);
    data["json"] = source.pointer(path).cloned().unwrap_or_default();
    data["classes"] = json!(["modeling-field", "assembly-group-field"]);
    data["accessibility_label"] = json!(label);
    data["editor_type"] = json!(kind);
    data["choices"] = json!(options);
    data["editable"] =
        json!(!w.fields.contains_key("assembly-group-deletion") && w.settings.is_none());
    data["contract"] = json!("tcad.assembly-groups");
    out.push(SemanticElement {
        id: identity(w, path),
        role: "assembly-group-field".into(),
        label: label.into(),
        value: source.pointer(path).map(|value| {
            value
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| value.to_string())
        }),
        state: if w
            .fields
            .contains_key(&format!("{ERRORS}{}", identity(w, path)))
        {
            "error"
        } else {
            "ready"
        }
        .into(),
        commands: vec![],
        relationships: vec![],
        data,
    });
}
fn command(
    w: &Workspace,
    out: &mut Vec<SemanticElement>,
    key: &str,
    label: &str,
    mutation: Value,
    enabled: bool,
    reason: &str,
) {
    let enabled =
        enabled && !w.fields.contains_key("assembly-group-deletion") && w.settings.is_none();
    let id = identity(w, key);
    let mut data = metadata(w);
    data["element"] = json!(id);
    data["mutation"] = mutation;
    data["actions"] = json!([{"command":ACTION,"label":label,"enabled":enabled,"disabled_reason":if enabled {None}else{Some(reason)},"scope":"view"}]);
    out.push(SemanticElement {
        id,
        role: "assembly-group-action".into(),
        label: label.into(),
        value: None,
        state: if enabled { "ready" } else { "disabled" }.into(),
        commands: vec![ACTION.into()],
        relationships: vec![],
        data,
    });
}
pub fn semantics(w: &Workspace) -> Vec<SemanticElement> {
    let doc = w.session.visible();
    let mut out = doc
        .assemblies
        .keys()
        .flat_map(|owner| assembly_groups::semantic(doc, evaluation(w), owner))
        .collect::<Vec<_>>();
    for element in &mut out {
        element.data["source_revision"] = revision(w);
    }
    if !w.fields.contains_key(EDITOR) {
        return out;
    }
    let Ok(source) = parsed(w) else {
        return out;
    };
    let raw = json!(source);
    let Some(assembly) = owner(w) else {
        return out;
    };
    let Some(base) = doc.assemblies.get(assembly) else {
        return out;
    };
    group(w, &mut out, "definition", "Rigid Group");
    field(w, &mut out, &raw, "/name", "Name", "text", vec![]);
    group(w, &mut out, "members", "Components");
    for (id, label) in choices(w, assembly, &source.id) {
        let selected = source.members.contains_key(&id);
        let enabled = selected || source.members.is_empty() || evaluation(w).is_some();
        command(
            w,
            &mut out,
            &format!("member/{id}"),
            &label,
            json!({"kind":"toggle_member","member":id}),
            enabled,
            "Wait for current validated component poses before adding a member",
        );
        if let Some(element) = out.last_mut() {
            element.value = Some(if selected { "Selected" } else { "Available" }.into());
            if selected {
                element.state = "selected".into();
            }
        }
    }
    let anchors=source.members.keys().map(|id|json!({"label":base.instances.get(id).map(|i|i.name.as_str()).unwrap_or("Missing component"),"value":id})).collect();
    field(
        w,
        &mut out,
        &raw,
        "/anchor",
        "Anchor component",
        "choice",
        anchors,
    );
    command(
        w,
        &mut out,
        "recapture",
        "Recapture current relative poses",
        json!({"kind":"recapture"}),
        source.members.len() >= 2 && evaluation(w).is_some(),
        "Choose at least two components and wait for current validated poses",
    );
    group(w, &mut out, "frames", "Captured Frames / Solver");
    for (id, pose) in &source.members {
        let name = base
            .instances
            .get(id)
            .map(|i| i.name.as_str())
            .unwrap_or("Missing component");
        let mut data = metadata(w);
        data["frame"] = json!(pose);
        data["member"] = json!(id);
        data["contract"] = json!("tcad.assembly-groups");
        out.push(SemanticElement {
            id: identity(w, &format!("frame/{id}")),
            role: "assembly-group-captured-frame".into(),
            label: name.into(),
            value: Some(format!(
                "X {:.4} · Y {:.4} · Z {:.4} mm · {:.3}/{:.3}/{:.3}°",
                pose.translation[0],
                pose.translation[1],
                pose.translation[2],
                pose.rotation_deg[0],
                pose.rotation_deg[1],
                pose.rotation_deg[2]
            )),
            state: "captured".into(),
            commands: vec![],
            relationships: vec![id.clone()],
            data,
        });
    }
    let solver_rows = out
        .iter()
        .filter(|element| {
            matches!(
                element.role.as_str(),
                "assembly-group-solver-link" | "assembly-group-component-motion"
            ) && element.data["group"] == source.id
        })
        .cloned()
        .collect::<Vec<_>>();
    for mut row in solver_rows {
        row.id = identity(w, &row.id);
        out.push(row);
    }
    if let Some(projected) = out
        .iter()
        .find(|element| {
            element.role == "assembly-rigid-group" && element.data["group"] == source.id
        })
        .cloned()
    {
        out.push(SemanticElement {
            id: identity(w, "solver"),
            role: "assembly-group-diagnostics".into(),
            label: "Current Solver Diagnostics".into(),
            value: projected.value,
            state: projected.state,
            commands: vec![],
            relationships: projected.relationships,
            data: projected.data,
        });
    }
    out
}
fn live(w: &Workspace, element: &SemanticElement) -> Result<SemanticElement> {
    let fresh = semantics(w)
        .into_iter()
        .find(|fresh| fresh.id == element.id)
        .ok_or_else(|| invalid("This group control is no longer available"))?;
    for key in [
        "document",
        "assembly",
        "group",
        "editor",
        "source_revision",
        "draft_source",
    ] {
        if fresh.data[key] != element.data[key] {
            return Err(invalid("Group source changed; use its current control"));
        }
    }
    if w.fields.get("assembly-group-context") != Some(&context(w)) {
        return Err(invalid("Group editing context changed; cancel and reopen"));
    }
    Ok(fresh)
}
pub fn current_value(w: &Workspace, element: &SemanticElement) -> Option<Value> {
    live(w, element).ok()?;
    json!(parsed(w).ok()?)
        .pointer(element.data["pointer"].as_str()?)
        .cloned()
}
pub fn is_invalid(w: &Workspace, element: &SemanticElement) -> bool {
    element.data["setter"] == "assembly-group"
        && w.fields.contains_key(&format!("{ERRORS}{}", element.id))
}
pub fn field_is_invalid(w: &Workspace, element: &SemanticElement) -> bool {
    is_invalid(w, element)
}
pub fn field_error(w: &mut Workspace, element: &SemanticElement, error: Option<String>) {
    if element.data["setter"] != "assembly-group" {
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
    if element.data["editable"] != true {
        return Err(invalid("This group field is unavailable"));
    }
    let mut source = parsed(w)?;
    match element.data["pointer"].as_str() {
        Some("/name") => {
            source.name = serde_json::from_value(value)?;
        }
        Some("/anchor") => {
            let anchor: String = serde_json::from_value(value)?;
            if source.members.len() >= 2 {
                assembly_groups::reanchor(&mut source, &anchor)?;
            } else {
                source.anchor = anchor;
            }
        }
        _ => return Err(invalid("This captured group field is read-only")),
    }
    if source.members.len() < 2 {
        w.json = serde_json::to_string_pretty(&source)?;
        guard(w);
        Ok(())
    } else {
        submit(w, source)
    }
}
fn active_action(w: &mut Workspace, data: &Value) -> Result<()> {
    let element = semantics(w)
        .into_iter()
        .find(|element| Some(element.id.as_str()) == data["element"].as_str())
        .ok_or_else(|| invalid("Group action is no longer available"))?;
    let element = live(w, &element)?;
    for key in [
        "document",
        "assembly",
        "group",
        "editor",
        "source_revision",
        "draft_source",
        "mutation",
    ] {
        if element.data[key] != data[key] {
            return Err(invalid(
                "Selection or group source changed; choose its current action",
            ));
        }
    }
    if !element
        .action_descriptors()
        .iter()
        .any(|action| action.enabled)
    {
        return Err(invalid("This group action is unavailable"));
    }
    let mut source = parsed(w)?;
    let assembly = owner(w)
        .ok_or_else(|| invalid("Owning assembly is missing"))?
        .to_string();
    match data["mutation"]["kind"].as_str() {
        Some("toggle_member") => {
            let id = data["mutation"]["member"]
                .as_str()
                .ok_or_else(|| invalid("Group participant is missing"))?
                .to_string();
            if source.members.remove(&id).is_some() {
                if source.anchor == id {
                    if let Some(anchor) = source.members.keys().next().cloned() {
                        if source.members.len() >= 2 {
                            assembly_groups::reanchor(&mut source, &anchor)?;
                        } else {
                            source.anchor = anchor;
                        }
                    } else {
                        source.anchor.clear();
                    }
                }
                if source.members.len() < 2 {
                    w.json = serde_json::to_string_pretty(&source)?;
                    guard(w);
                    return Ok(());
                }
            } else {
                let mut members = source.members.keys().cloned().collect::<Vec<_>>();
                members.push(id.clone());
                let anchor = if source.anchor.is_empty() {
                    id
                } else {
                    source.anchor.clone()
                };
                if members.len() < 2 {
                    source.anchor = anchor;
                    source
                        .members
                        .insert(members[0].clone(), Placement::default());
                    w.json = serde_json::to_string_pretty(&source)?;
                    guard(w);
                    return Ok(());
                }
                source = assembly_groups::capture(
                    w.session.visible(),
                    evaluation(w).ok_or_else(|| invalid("Wait for current evaluated poses"))?,
                    &assembly,
                    source,
                    &members,
                    &anchor,
                )?;
            }
        }
        Some("recapture") => {
            let members = source.members.keys().cloned().collect::<Vec<_>>();
            let anchor = source.anchor.clone();
            source = assembly_groups::capture(
                w.session.visible(),
                evaluation(w).ok_or_else(|| invalid("Wait for current evaluated poses"))?,
                &assembly,
                source,
                &members,
                &anchor,
            )?;
        }
        _ => return Err(invalid("Unknown group action")),
    }
    submit(w, source)
}
pub fn action(w: &mut Workspace, command: &str, data: Option<&Value>) -> bool {
    if !command.starts_with("assembly.group-") {
        return false;
    }
    let result = (|| -> Result<()> {
        if w.settings.is_some() {
            return Err(invalid("Close Settings before editing a group"));
        }
        if command == ACTION {
            return active_action(
                w,
                data.ok_or_else(|| invalid("Choose a current group action"))?,
            );
        }
        if command == "assembly.group-preview-source" {
            return submit(w, parsed(w)?);
        }
        let target = data.and_then(|data| data.get("target")).or(data);
        let assembly = target
            .and_then(|value| value["assembly"].as_str())
            .map(str::to_owned)
            .or_else(|| owner(w).map(str::to_owned))
            .unwrap_or_else(|| w.session.visible().root.clone());
        if command == "assembly.group-new" {
            new(w, &assembly);
            return Ok(());
        }
        let id = target
            .and_then(|value| value["group"].as_str())
            .map(str::to_owned)
            .or_else(|| w.fields.get(EDITOR).cloned())
            .ok_or_else(|| invalid("Select a saved group"))?;
        let source = w
            .session
            .visible()
            .assemblies
            .get(&assembly)
            .and_then(|owner| owner.rigid_groups.get(&id))
            .cloned()
            .ok_or_else(|| invalid("Saved group is missing"))?;
        if target.is_some_and(|value| {
            value
                .get("document")
                .is_some_and(|id| id.as_str() != Some(w.session.visible().id.as_str()))
                || value
                    .get("source")
                    .is_some_and(|saved| *saved != json!(source))
                || value
                    .get("source_revision")
                    .is_some_and(|saved| *saved != revision(w))
        }) {
            return Err(invalid(
                "Group action belongs to an older source; choose its current tree control",
            ));
        }
        match command {
            "assembly.group-edit" => open(w, &assembly, &id),
            "assembly.group-delete" => {
                if start(w, &assembly, source) {
                    w.dispatch(Command::DeleteAssemblyGroup {
                        assembly,
                        group: id,
                    });
                    w.fields
                        .insert("assembly-group-deletion".into(), "true".into());
                    guard(w);
                }
            }
            "assembly.group-suppress" => {
                let scope = cad_core::configuration::PlacementScope::selected(
                    w.session.visible(),
                    &assembly,
                );
                let config = w.session.visible().active_configurations.get(&assembly);
                let suppressed = cad_core::configuration::scope(
                    w.session.visible(),
                    w.session.visible(),
                    &assembly,
                    config.map(String::as_str),
                )?
                .assemblies[&assembly]
                    .rigid_groups[&id]
                    .suppressed;
                if start(w, &assembly, source) {
                    w.dispatch(Command::SuppressAssemblyGroup {
                        assembly: assembly.clone(),
                        group: id.clone(),
                        suppressed: !suppressed,
                        scope,
                    });
                    if let Some(source) = w.session.visible().assemblies[&assembly]
                        .rigid_groups
                        .get(&id)
                    {
                        w.json = serde_json::to_string_pretty(source)?;
                    }
                    guard(w);
                }
            }
            _ => return Err(invalid("Unknown group command")),
        }
        Ok(())
    })();
    if let Err(error) = result {
        w.status = error.to_string();
        guard(w);
    }
    true
}
pub fn show(ui: &mut egui::Ui, w: &mut Workspace) {
    guard(w);
    for element in semantics(w)
        .into_iter()
        .filter(|element| element.id.starts_with("assembly-group-editor:"))
    {
        match element.role.as_str() {
            "assembly-group-field" => crate::ui::semantic_field(ui, w, &element),
            "assembly-group-action"
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
                    element.state == "selected",
                )
                .clicked() =>
            {
                action(w, ACTION, Some(&element.data));
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
                        "field-value",
                        &[],
                        value,
                        &element.state,
                    );
                }
            }
        }
    }
    if let Some(draft) = &w.session.draft {
        controls::label(
            ui,
            "assembly-group-preview",
            "preview-status",
            &[],
            draft.error.as_deref().unwrap_or(if draft.pending {
                "Updating rigid-group constraints…"
            } else {
                "Current group preview evaluated"
            }),
            if draft.error.is_some() {
                "invalid"
            } else if draft.pending {
                "pending"
            } else {
                "ready"
            },
        );
    }
    if w.fields.contains_key(INVALID) {
        controls::label(
            ui,
            "assembly-group-source-status",
            "source-diagnostic",
            &[],
            "Choose at least two active components or correct the authored source before applying.",
            "invalid",
        );
    }
    egui::CollapsingHeader::new("Advanced Source")
        .id_salt("assembly-group-source")
        .show(ui, |ui| {
            if ui
                .add(
                    egui::TextEdit::multiline(&mut w.json)
                        .code_editor()
                        .desired_rows(8),
                )
                .changed()
            {
                guard(w);
            }
            if controls::button(
                ui,
                "assembly-group-preview-source",
                "assembly-group-action",
                &[],
                "Preview edited source",
                w.fields.get("assembly-group-context") == Some(&context(w))
                    && !w.fields.contains_key("assembly-group-deletion"),
                false,
            )
            .clicked()
            {
                action(w, "assembly.group-preview-source", None);
            }
        });
    crate::cad_ui::confirmation(ui, w);
}
pub fn tree(ui: &mut egui::Ui, w: &mut Workspace) {
    let groups = semantics(w)
        .into_iter()
        .filter(|element| element.role == "assembly-rigid-group")
        .collect::<Vec<_>>();
    if groups.is_empty() {
        return;
    }
    controls::label(
        ui,
        "assembly-groups-tree",
        "feature-tree-group",
        &[],
        "Rigid Groups",
        "ready",
    );
    for element in groups {
        ui.horizontal(|ui| {
            if controls::button(
                ui,
                &element.id,
                &element.role,
                &["feature-tree-row"],
                &element.label,
                !busy(w),
                false,
            )
            .clicked()
            {
                action(w, "assembly.group-edit", Some(&element.data));
            }
            for (command, label) in [
                ("assembly.group-suppress", "Suppress / restore"),
                ("assembly.group-delete", "Dissolve"),
            ] {
                if controls::button(
                    ui,
                    &format!("{}:{command}", element.id),
                    "assembly-group-action",
                    &["tree-action"],
                    label,
                    !busy(w),
                    false,
                )
                .clicked()
                {
                    action(w, command, Some(&element.data));
                }
            }
        });
    }
}
