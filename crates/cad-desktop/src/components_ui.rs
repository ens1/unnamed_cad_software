//! Component insertion and placement use shared source and semantic controls.
use crate::{Workspace, presentation_controls as controls};
use bevy_egui::egui;
use cad_core::{commands::Command, presentation::SemanticElement, *};
use serde_json::{Value, json};
use std::collections::BTreeSet;

const INSERT: &str = "component-insert";
const INVALID: &str = "component-placement-invalid:";
fn error(message: &str) -> CadError {
    CadError::Invalid(message.into())
}

pub fn cancel(w: &mut Workspace) {
    w.fields
        .retain(|key, _| !key.starts_with(INVALID) && !key.starts_with("component-insert-"));
}
pub fn begin_insert(w: &mut Workspace) {
    if w.tool_checkpoint.is_some()
        || w.sketch.is_some()
        || w.confirm_requested
        || w.settings.is_some()
    {
        w.status = "Finish or cancel the current operation before inserting a component".into();
        return;
    }
    let owner = w
        .fields
        .get("assembly-id")
        .filter(|id| w.session.visible().assemblies.contains_key(*id))
        .cloned()
        .unwrap_or_else(|| w.session.visible().root.clone());
    if !w.session.visible().assemblies.contains_key(&owner) {
        w.status = "Choose an owning assembly before inserting a component".into();
        return;
    }
    if pinned_owner(w.session.visible(), &owner) {
        w.status =
            "Make the linked assembly unique before inserting components into its definition"
                .into();
        return;
    }
    cancel(w);
    w.activate_context(None, None);
    w.start_tool();
    w.open_editor(INSERT);
    w.fields.insert("component-insert-owner".into(), owner);
    w.fields.insert("component-insert-id".into(), id());
    w.fields.insert("component-insert-generation".into(), id());
    w.status =
        "Choose a definition or local TCAD project, inspect the preview, then OK Component".into();
}
fn insertion_base(w: &Workspace) -> Result<Document> {
    if w.dialog.as_deref() != Some(INSERT) || w.settings.is_some() {
        return Err(error("Open Insert Component before changing its source"));
    }
    w.tool_checkpoint
        .as_ref()
        .map(|session| session.visible().clone())
        .ok_or_else(|| error("The component insertion checkpoint is unavailable"))
}
fn owner(w: &Workspace) -> Result<&str> {
    w.fields
        .get("component-insert-owner")
        .map(String::as_str)
        .filter(|owner| w.session.visible().assemblies.contains_key(*owner))
        .ok_or_else(|| error("The owning assembly is no longer available"))
}
fn can_insert(doc: &Document, owner: &str, definition: &str) -> bool {
    if doc.parts.contains_key(definition) {
        return true;
    }
    if !doc.assemblies.contains_key(definition) {
        return false;
    }
    let mut pending = vec![definition];
    let mut seen = BTreeSet::new();
    while let Some(definition) = pending.pop() {
        if definition == owner {
            return false;
        }
        if seen.insert(definition)
            && let Some(assembly) = doc.assemblies.get(definition)
        {
            pending.extend(
                assembly
                    .instances
                    .values()
                    .map(|instance| instance.definition.as_str()),
            );
        }
    }
    true
}
fn pinned_owner(doc: &Document, owner: &str) -> bool {
    doc.libraries
        .values()
        .any(|link| link.pinned_definitions.contains_key(owner))
}
fn definition_choices(w: &Workspace, owner: &str) -> Vec<Value> {
    let Ok(doc) = insertion_base(w) else {
        return vec![];
    };
    std::iter::once(json!({"value":"","label":"Choose definition"}))
        .chain(
            doc.parts
                .values()
                .map(|part| (&part.id, &part.name))
                .chain(
                    doc.assemblies
                        .values()
                        .map(|assembly| (&assembly.id, &assembly.name)),
                )
                .filter(|(definition, _)| can_insert(&doc, owner, definition))
                .map(|(definition, name)| json!({"value":definition,"label":name})),
        )
        .collect()
}
fn prior_instance(w: &Workspace, owner: &str) -> Option<Instance> {
    let component = w.fields.get("component-insert-id")?;
    w.session
        .visible()
        .assemblies
        .get(owner)?
        .instances
        .get(component)
        .cloned()
}
fn publish(w: &mut Workspace, document: Document, owner: &str, component: &str) -> Result<()> {
    document.validate()?;
    let expected = document
        .assemblies
        .get(owner)
        .and_then(|assembly| assembly.instances.get(component))
        .cloned()
        .ok_or_else(|| error("The insertion has no preview component"))?;
    let command = library::replace_command(w.session.visible(), document, &w.root())?;
    w.dispatch(command);
    if w.session
        .visible()
        .assemblies
        .get(owner)
        .and_then(|assembly| assembly.instances.get(component))
        != Some(&expected)
    {
        return Err(error(&w.status));
    }
    w.fields
        .insert("component-insert-id".into(), component.into());
    w.fields.remove("component-insert-error");
    w.fields.retain(|key, _| !key.starts_with(INVALID));
    Ok(())
}
fn preview_definition(w: &mut Workspace, definition: &str) -> Result<()> {
    let owner = owner(w)?.to_string();
    let mut document = insertion_base(w)?;
    if !can_insert(&document, &owner, definition) {
        return Err(error(
            "Choose a definition that does not create an assembly cycle",
        ));
    }
    let prior = prior_instance(w, &owner);
    let component = w
        .fields
        .get("component-insert-id")
        .cloned()
        .unwrap_or_else(id);
    let name = prior
        .as_ref()
        .filter(|instance| instance.definition == definition)
        .map(|instance| instance.name.clone())
        .unwrap_or_else(|| {
            cad_core::occurrence_names::insertion(
                &document,
                &owner,
                &presentation::definition_name(&document, definition),
            )
        });
    let parent = document
        .assemblies
        .get_mut(&owner)
        .ok_or_else(|| error("The parent assembly is missing"))?;
    parent.instances.insert(
        component.clone(),
        Instance {
            id: component.clone(),
            name,
            definition: definition.into(),
            placement: prior
                .as_ref()
                .map(|instance| instance.placement.clone())
                .unwrap_or_default(),
            finish: None,
            configuration: None,
            fixed: parent.instances.is_empty(),
            translation_expressions: None,
        },
    );
    publish(w, document, &owner, &component)?;
    w.fields
        .insert("component-insert-kind".into(), "definition".into());
    w.fields.remove("component-insert-file");
    Ok(())
}
fn preview_file(w: &mut Workspace, source: &std::path::Path) -> Result<()> {
    let owner = owner(w)?.to_string();
    let prior = prior_instance(w, &owner);
    let mut document = insertion_base(w)?;
    let before = document
        .assemblies
        .get(&document.root)
        .ok_or_else(|| error("Linking needs an assembly root"))?
        .instances
        .keys()
        .cloned()
        .collect::<BTreeSet<_>>();
    let link = library::link(&mut document, &w.root(), source, None, None)?;
    let definition = &document
        .libraries
        .get(&link)
        .ok_or_else(|| error("The component link is missing"))?
        .definition;
    let inserted = document
        .assemblies
        .get(&document.root)
        .unwrap()
        .instances
        .values()
        .find(|instance| !before.contains(&instance.id) && &instance.definition == definition)
        .map(|instance| instance.id.clone())
        .ok_or_else(|| error("The linked source did not create an occurrence"))?;
    let mut instance = document
        .assemblies
        .get_mut(&document.root)
        .unwrap()
        .instances
        .remove(&inserted)
        .unwrap();
    let component = w
        .fields
        .get("component-insert-id")
        .cloned()
        .unwrap_or(inserted);
    instance.id = component.clone();
    instance.placement = prior.map(|instance| instance.placement).unwrap_or_default();
    let parent = document
        .assemblies
        .get_mut(&owner)
        .ok_or_else(|| error("The parent assembly is missing"))?;
    instance.fixed = parent.instances.is_empty();
    parent.instances.insert(component.clone(), instance);
    publish(w, document, &owner, &component)?;
    w.fields
        .insert("component-insert-kind".into(), "file".into());
    w.fields.insert(
        "component-insert-file".into(),
        source.to_string_lossy().into(),
    );
    w.fields.remove("component-insert-definition");
    Ok(())
}
pub fn placement_semantics(w: &Workspace, assembly: &str, component: &str) -> Vec<SemanticElement> {
    let Some(parent) = w.session.visible().assemblies.get(assembly) else {
        return vec![];
    };
    let Some(instance) = parent.instances.get(component) else {
        return vec![];
    };
    let generated = component_pattern::generated(parent).contains(&instance.id);
    let pinned = pinned_owner(w.session.visible(), assembly);
    let configured = w
        .session
        .visible()
        .active_configurations
        .contains_key(assembly);
    let mut elements = vec![SemanticElement {
        id: format!("component:{component}:frame"), role: "component-placement-frame".into(),
        label: if configured {"Base placement seed · parent assembly coordinates"} else {"Placement seed · parent assembly coordinates"}.into(),
        value: Some("X/Y/Z translation; Euler X/Y/Z rotation. Mates determine the current validated solved pose.".into()),
        state: "ready".into(), commands: vec![], relationships: vec![assembly.into(), component.into()],
        data: json!({"contract":"tcad.presentation.component-placement","version":1,"coordinate_frame":"parent_assembly","assembly":assembly,"component":component,"placement":instance.placement,"classes":["modeling-status"]}),
    }];
    for rotation in [false, true] {
        for axis in 0..3 {
            let value = if rotation {
                instance.placement.rotation_deg[axis]
            } else {
                instance.placement.translation[axis]
            };
            let editable = w.settings.is_none()
                && !generated
                && !pinned
                && (rotation || instance.translation_expressions.is_none());
            elements.push(SemanticElement {
                id: format!("component:{component}:{}:{axis}", if rotation {"rotation"} else {"translation"}),
                role: "component-placement-field".into(), label: format!("{} {}", ["X", "Y", "Z"][axis], if rotation {"rotation"} else {"position"}),
                value: Some(value.to_string()), state: if editable {"editable"} else {"readonly"}.into(), commands: vec![],
                relationships: vec![assembly.into(), component.into()],
                data: json!({"contract":"tcad.presentation.component-placement","version":1,"setter":"component-placement","document":w.session.visible().id,"assembly":assembly,"component":component,"axis":axis,"rotation":rotation,
                    "json":value,"editable":editable,"value_type":"number","accepts_quantity_text":true,"dimension":if rotation {"angle"} else {"length"},"unit":if rotation {"deg"} else {"mm"},"classes":["modeling-field"],"accessibility_label":format!("Parent assembly {} {}", ["X", "Y", "Z"][axis], if rotation {"rotation"} else {"position"})}),
            });
        }
    }
    elements
}
pub fn current_value(w: &Workspace, element: &SemanticElement) -> Option<Value> {
    let assembly = element.data["assembly"].as_str()?;
    let component = element.data["component"].as_str()?;
    let axis = element.data["axis"].as_u64()? as usize;
    let placement = &w
        .session
        .visible()
        .assemblies
        .get(assembly)?
        .instances
        .get(component)?
        .placement;
    Some(json!(if element.data["rotation"].as_bool() == Some(true) {
        *placement.rotation_deg.get(axis)?
    } else {
        *placement.translation.get(axis)?
    }))
}
pub fn set_value(w: &mut Workspace, element: &SemanticElement, value: Value) -> Result<()> {
    let assembly = element.data["assembly"]
        .as_str()
        .ok_or_else(|| error("Choose an assembly"))?;
    let component = element.data["component"]
        .as_str()
        .ok_or_else(|| error("Choose a component"))?;
    let axis = element.data["axis"]
        .as_u64()
        .filter(|axis| *axis < 3)
        .ok_or_else(|| error("Choose X, Y or Z"))? as usize;
    let fresh = placement_semantics(w, assembly, component)
        .into_iter()
        .find(|fresh| fresh.id == element.id)
        .ok_or_else(|| error("The component placement field is no longer available"))?;
    for key in ["document", "assembly", "component", "axis", "rotation"] {
        if fresh.data[key] != element.data[key] {
            return Err(error("Use the current component placement field"));
        }
    }
    if fresh.data["editable"] != true {
        return Err(error(
            "This placement is driven or cannot be edited in the current context",
        ));
    }
    let rotation = element.data["rotation"].as_bool() == Some(true);
    let mut instance = w.session.visible().assemblies[assembly].instances[component].clone();
    let number = if let Some(number) = value.as_f64() {
        number
    } else {
        let text = value
            .as_str()
            .ok_or_else(|| error("Enter a coordinate or quantity"))?;
        let expression = if text.trim().parse::<f64>().is_ok() {
            format!("{text} {}", if rotation { "deg" } else { "mm" })
        } else {
            text.into()
        };
        let number = expression::value(
            w.session.visible(),
            assembly,
            &Expr(expression),
            if rotation {
                Dimension::Angle
            } else {
                Dimension::Length
            },
        )?;
        if rotation {
            number.to_degrees()
        } else {
            number
        }
    };
    if !number.is_finite() {
        return Err(error("Enter a finite coordinate"));
    }
    if rotation {
        instance.placement.rotation_deg[axis] = number;
    } else {
        instance.placement.translation[axis] = number;
    }
    let mut source = w.session.visible().clone();
    source
        .assemblies
        .get_mut(assembly)
        .unwrap()
        .instances
        .insert(component.into(), instance.clone());
    source.validate()?;
    w.dispatch(Command::PutInstance {
        assembly: assembly.into(),
        instance: instance.clone(),
    });
    if w.session
        .visible()
        .assemblies
        .get(assembly)
        .and_then(|assembly| assembly.instances.get(component))
        != Some(&instance)
    {
        return Err(error(&w.status));
    }
    Ok(())
}
pub fn insert_value(w: &Workspace, element: &SemanticElement) -> Option<Value> {
    (element.data["setter"] == "component-insert" && w.dialog.as_deref() == Some(INSERT)).then(
        || {
            json!(
                w.fields
                    .get("component-insert-definition")
                    .cloned()
                    .unwrap_or_default()
            )
        },
    )
}
pub fn set_insert_value(w: &mut Workspace, element: &SemanticElement, value: Value) -> Result<()> {
    if element.data["setter"] != "component-insert"
        || element.data["document"] != w.session.visible().id
    {
        return Err(error("Use the current component definition picker"));
    }
    let owner = owner(w)?.to_string();
    if element.data["owner"] != owner
        || element.data["editor"] != json!(w.fields.get("component-insert-generation"))
    {
        return Err(error(
            "The insertion context changed; use its current definition picker",
        ));
    }
    let definition = value
        .as_str()
        .ok_or_else(|| error("Choose an existing definition"))?;
    if !definition_choices(w, &owner)
        .iter()
        .any(|choice| choice["value"] == definition)
    {
        return Err(error(
            "The selected definition is no longer available in this insertion context",
        ));
    }
    w.fields
        .insert("component-insert-definition".into(), definition.into());
    let result = if definition.is_empty() {
        Err(error("Choose a definition or local TCAD project"))
    } else {
        preview_definition(w, definition)
    };
    if let Err(error) = &result {
        w.fields
            .insert("component-insert-error".into(), error.to_string());
    }
    result
}
pub fn field_error(w: &mut Workspace, element: &SemanticElement, message: Option<String>) {
    if element.data["setter"] == "component-insert" {
        if let Some(message) = message {
            w.fields.insert("component-insert-error".into(), message);
        } else {
            w.fields.remove("component-insert-error");
        }
        return;
    }
    if element.data["setter"] != "component-placement" {
        return;
    }
    let key = format!("{INVALID}{}", element.id);
    if let Some(message) = message {
        w.fields.insert(key, message);
    } else {
        w.fields.remove(&key);
    }
}
pub fn field_is_invalid(w: &Workspace, element: &SemanticElement) -> bool {
    if element.data["setter"] == "component-insert" {
        w.fields.contains_key("component-insert-error")
    } else {
        w.fields.contains_key(&format!("{INVALID}{}", element.id))
    }
}
pub fn current_source_is_valid(w: &Workspace) -> bool {
    if w.fields.keys().any(|key| key.starts_with(INVALID)) {
        return false;
    }
    if crate::command_palette::context_editor(w) != Some(INSERT) {
        return true;
    }
    !w.fields.contains_key("component-insert-error")
        && owner(w)
            .ok()
            .and_then(|owner| prior_instance(w, owner))
            .is_some()
}
fn insertion_semantics(w: &Workspace) -> Vec<SemanticElement> {
    if w.dialog.as_deref() != Some(INSERT) {
        return vec![];
    }
    let Ok(owner) = owner(w) else {
        return vec![];
    };
    let current = w
        .fields
        .get("component-insert-definition")
        .cloned()
        .unwrap_or_default();
    let enabled = w.settings.is_none() && w.tool_checkpoint.is_some();
    let info = |key: &str, role: &str, label: String, state: &str, data: Value| SemanticElement {
        id: format!("component-insert/{key}"),
        role: role.into(),
        label,
        value: None,
        state: state.into(),
        commands: vec![],
        relationships: vec![owner.into()],
        data,
    };
    let mut elements = vec![
        info("frame", "component-placement-frame", format!("Into {} · parent assembly coordinates", presentation::definition_name(w.session.visible(), owner)), "ready", json!({"owner":owner,"coordinate_frame":"parent_assembly","classes":["modeling-status"]})),
        info("help", "component-insert-help", "Choose a definition to preview it immediately, or select project.json from a local TCAD project. Replacing the source keeps one preview component at its current placement. OK Component adds it; Cancel restores the starting document.".into(), "ready", json!({"classes":["modeling-status"]})),
        SemanticElement {
            id: "component-insert/definition".into(), role: "component-insert-field".into(), label: "Existing definition".into(), value: Some(current.clone()), state: if enabled {"editable"} else {"readonly"}.into(), commands: vec![], relationships: vec![owner.into()],
            data: json!({"contract":"tcad.presentation.component-insert","version":1,"setter":"component-insert","document":w.session.visible().id,"owner":owner,"editor":w.fields.get("component-insert-generation"),"json":current,"choices":definition_choices(w,owner),"value_type":"choice","editable":enabled,"classes":["modeling-field"],"accessibility_label":"Existing component definition"}),
        },
    ];
    for (command, label, available) in [
        (
            "component.insert-definition",
            "Preview selected definition",
            enabled && !current.is_empty(),
        ),
        ("component.insert-file", "From TCAD Project…", enabled),
    ] {
        elements.push(SemanticElement {
            id: command.into(), role: "component-insert-action".into(), label: label.into(), value: None, state: if available {"ready"} else {"disabled"}.into(), commands: vec![command.into()], relationships: vec![owner.into()],
            data: json!({"document":w.session.visible().id,"owner":owner,"classes":["semantic-control","modeling-action"],"actions":[{"command":command,"label":label,"enabled":available,"arguments":{}}]}),
        });
    }
    if let Some(instance) = prior_instance(w, owner) {
        let linked = w.fields.get("component-insert-file");
        elements.push(info("source", "component-insert-source", format!("Preview · {}{}", instance.name, if linked.is_some() {" · linked project"} else {" · shared definition"}), "ready", json!({"source_file":linked,"component":instance.id,"definition":instance.definition,"classes":["modeling-status"]})));
        elements.extend(placement_semantics(w, owner, &instance.id));
    } else {
        elements.push(info(
            "empty",
            "component-insert-status",
            "No component selected · choose a source before OK Component".into(),
            "pending",
            json!({"classes":["modeling-status"]}),
        ));
    }
    if let Some(message) = w.fields.get("component-insert-error") {
        elements.push(info(
            "error",
            "component-insert-status",
            message.clone(),
            "invalid",
            json!({"classes":["modeling-status"]}),
        ));
    }
    elements
}
pub fn semantics(w: &Workspace) -> Vec<SemanticElement> {
    let mut elements = insertion_semantics(w);
    let inserted = w.fields.get("component-insert-id");
    for assembly in w.session.visible().assemblies.values() {
        for component in assembly.instances.keys() {
            if w.dialog.as_deref() != Some(INSERT) || inserted != Some(component) {
                elements.extend(placement_semantics(w, &assembly.id, component));
            }
        }
    }
    elements
}
pub fn show_placement(ui: &mut egui::Ui, w: &mut Workspace, assembly: &str, component: &str) {
    for element in placement_semantics(w, assembly, component) {
        if element.role == "component-placement-field" {
            crate::ui::semantic_field(ui, w, &element);
        } else {
            controls::label(
                ui,
                &element.id,
                &element.role,
                &["modeling-status"],
                &element.label,
                &element.state,
            )
            .on_hover_text(element.value.unwrap_or_default());
        }
    }
}
pub fn action(w: &mut Workspace, command: &str) -> bool {
    if command == "assembly.insert-existing" {
        begin_insert(w);
        return true;
    }
    if !matches!(
        command,
        "component.insert-definition" | "component.insert-file"
    ) {
        return false;
    }
    let result: Result<()> = (|| {
        insertion_base(w)?;
        if command == "component.insert-file" {
            if let Some(source) = rfd::FileDialog::new()
                .set_title("Choose project.json from a TCAD project")
                .add_filter("TCAD project", &["json"])
                .pick_file()
            {
                preview_file(w, &source)?;
            }
        } else {
            let definition = w
                .fields
                .get("component-insert-definition")
                .cloned()
                .ok_or_else(|| error("Choose an existing definition first"))?;
            preview_definition(w, &definition)?;
        }
        Ok(())
    })();
    if let Err(error) = result {
        w.status = error.to_string();
        w.fields
            .insert("component-insert-error".into(), w.status.clone());
    }
    true
}
pub fn show(ui: &mut egui::Ui, w: &mut Workspace) {
    for element in insertion_semantics(w) {
        match element.role.as_str() {
            "component-insert-field" | "component-placement-field" => {
                crate::ui::semantic_field(ui, w, &element)
            }
            "component-insert-action" => {
                if controls::button(
                    ui,
                    &element.id,
                    &element.role,
                    &["semantic-control", "modeling-action"],
                    &element.label,
                    element
                        .action_descriptors()
                        .iter()
                        .any(|action| action.enabled),
                    false,
                )
                .clicked()
                {
                    action(w, &element.commands[0]);
                }
            }
            _ => {
                controls::label(
                    ui,
                    &element.id,
                    &element.role,
                    &["modeling-status"],
                    &element.label,
                    &element.state,
                );
            }
        }
    }
}
