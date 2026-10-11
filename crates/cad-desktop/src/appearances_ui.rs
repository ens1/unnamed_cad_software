//! Visual finishes and studio lighting use the shared document transaction.
//! Engineering materials remain independent of these renderer-owned effects.
use crate::{Workspace, cad_ui, presentation_controls as controls};
use bevy_egui::egui;
use cad_core::{
    CadError, Id, Result,
    appearance::{Finish, Lighting},
    commands::Command,
    presentation::SemanticElement,
};
use serde_json::{Value, json};

const EDITOR: &str = "appearance-editor";
const TOOL: &str = "appearance-tool";
const SOURCE: &str = "appearance-source";
const INVALID: &str = "appearance-source-invalid";
const IMPORT: &str = "appearance.texture.import/";
const CLEAR: &str = "appearance.texture.clear/";

fn selected(w: &Workspace) -> Option<Id> {
    w.fields
        .get(EDITOR)
        .cloned()
        .filter(|id| w.session.visible().finishes.contains_key(id))
}
fn initial(w: &Workspace) -> Option<Id> {
    if let Ok(placed) = w.session.visible().placed_parts()
        && let Some(part) = placed.iter().find(|placed| {
            w.view.active_part.as_ref() == Some(&placed.definition)
                && w.view
                    .active_instance
                    .as_ref()
                    .is_none_or(|id| placed.path.last() == Some(id))
        })
        && let Ok(finish) = cad_core::appearance::resolve_finish(w.session.visible(), part)
    {
        return Some(finish.id.clone());
    }
    w.view
        .active_part
        .as_ref()
        .and_then(|id| w.session.visible().parts.get(id))
        .map(|part| part.finish.clone())
        .filter(|id| w.session.visible().finishes.contains_key(id))
        .or_else(|| w.session.visible().finishes.keys().next().cloned())
}
fn load(w: &mut Workspace, id: &str) {
    w.fields.insert(EDITOR.into(), id.into());
    w.fields.remove(INVALID);
    w.fields.remove(SOURCE);
    w.fields.retain(|key, _| {
        !key.starts_with("presentation-field:finish:")
            && !key.starts_with("appearance-field-invalid:finish:")
            && !matches!(
                key.as_str(),
                "appearance-field-invalid:appearance:source"
                    | "presentation-field:appearance:source"
            )
    });
}
fn available(w: &Workspace) -> bool {
    w.sketch.is_none() && (w.tool_checkpoint.is_none() || w.fields.contains_key(TOOL))
}
pub fn begin(w: &mut Workspace, lighting: bool) {
    if !available(w) {
        w.status = "Apply or cancel the current operation before editing visual finishes".into();
        return;
    }
    w.start_tool();
    w.fields.insert(TOOL.into(), "true".into());
    if !w.fields.contains_key(EDITOR)
        && let Some(id) = initial(w)
    {
        load(w, &id);
    }
    w.fields.insert(
        "appearance-tab".into(),
        if lighting { "lighting" } else { "finish" }.into(),
    );
    w.open_editor("appearances");
}
pub fn current_source_is_valid(w: &Workspace) -> bool {
    !w.fields.contains_key(INVALID)
        && (!w.fields.contains_key(TOOL) || crate::render_appearance::resources_are_current(w))
        && !w
            .fields
            .keys()
            .any(|key| key.starts_with("appearance-field-invalid:"))
        && w.appearance_diagnostics.is_empty()
        && w.fields.get(SOURCE).is_none_or(|source| {
            serde_json::from_str::<Finish>(source).is_ok_and(|finish| {
                selected(w).as_ref() == Some(&finish.id)
                    && w.session.visible().finishes.get(&finish.id) == Some(&finish)
            })
        })
}
pub fn field_error(w: &mut Workspace, element: &SemanticElement, error: Option<String>) {
    if !element
        .data
        .get("setter")
        .and_then(Value::as_str)
        .is_some_and(|setter| setter.starts_with("appearance-"))
    {
        return;
    }
    let key = format!("appearance-field-invalid:{}", element.id);
    if let Some(error) = error {
        w.fields.insert(key, error);
    } else {
        w.fields.remove(&key);
    }
}
fn checked_dispatch(
    w: &mut Workspace,
    command: Command,
    expected: Value,
    finish: Option<&str>,
) -> Result<()> {
    w.dispatch(command);
    let actual = finish
        .map(|id| serde_json::to_value(w.session.visible().finishes.get(id)))
        .unwrap_or_else(|| serde_json::to_value(&w.session.visible().lighting))?;
    if actual != expected {
        return Err(CadError::Invalid(w.status.clone()));
    }
    w.rendered_key.clear();
    Ok(())
}
fn put(w: &mut Workspace, finish: Finish) -> Result<()> {
    let id = finish.id.clone();
    let expected = serde_json::to_value(&finish)?;
    checked_dispatch(
        w,
        Command::PutFinish {
            finish: Box::new(finish),
        },
        expected,
        Some(&id),
    )?;
    w.fields.remove(INVALID);
    if w.fields.contains_key(SOURCE) {
        w.fields.insert(
            SOURCE.into(),
            serde_json::to_string_pretty(&w.session.visible().finishes[&id])?,
        );
    }
    Ok(())
}
fn target_instance(w: &Workspace) -> Option<(Id, Id)> {
    let id = w.view.active_instance.as_ref()?;
    let mut choices = w
        .session
        .visible()
        .assemblies
        .values()
        .filter_map(|assembly| {
            assembly
                .instances
                .values()
                .find(|instance| &instance.id == id)
                .map(|instance| (assembly.id.clone(), instance.id.clone()))
        });
    let target = choices.next()?;
    choices.next().is_none().then_some(target)
}
fn can_delete(w: &Workspace, id: &str) -> bool {
    cad_core::appearance::delete_reason(w.session.visible(), id).is_none()
}
fn choose_id(w: &Workspace, arguments: Option<&Value>) -> Option<Id> {
    arguments
        .and_then(|arguments| arguments.get("finish"))
        .and_then(Value::as_str)
        .map(str::to_owned)
        .or_else(|| selected(w))
}
pub fn action(w: &mut Workspace, command: &str, arguments: Option<&Value>) -> bool {
    if matches!(command, "appearance" | "appearances" | "lighting") {
        begin(w, command == "lighting");
        return true;
    }
    if !command.starts_with("appearance.") {
        return false;
    }
    if !available(w) {
        w.status = "Apply or cancel the current operation before editing visual finishes".into();
        return true;
    }
    if matches!(command, "appearance.tab.finish" | "appearance.tab.lighting") {
        w.fields.insert(
            "appearance-tab".into(),
            if command.ends_with("lighting") {
                "lighting"
            } else {
                "finish"
            }
            .into(),
        );
        return true;
    }
    if !w.fields.contains_key(TOOL) {
        begin(w, false);
    }
    let finish = choose_id(w, arguments);
    let result = (|| -> Result<()> {
        match command {
            "appearance.edit" => {
                if let Some(id) = &finish {
                    load(w, id);
                }
                w.open_editor("appearances");
            }
            "appearance.new" | "appearance.duplicate" => {
                let source = finish
                    .as_ref()
                    .and_then(|id| w.session.visible().finishes.get(id))
                    .or_else(|| w.session.visible().finishes.values().next())
                    .ok_or_else(|| {
                        CadError::Invalid(
                            "Create a visual finish in the source before duplicating it".into(),
                        )
                    })?;
                let mut new = source.clone();
                new.id = cad_core::id();
                new.name = if command == "appearance.duplicate" {
                    format!("{} copy", source.name)
                } else {
                    "Custom finish".into()
                };
                if command == "appearance.new" {
                    new.textures = Default::default();
                }
                let id = new.id.clone();
                put(w, new)?;
                load(w, &id);
            }
            "appearance.delete" => {
                let id = finish
                    .as_ref()
                    .ok_or_else(|| CadError::Invalid("Choose a finish to delete".into()))?;
                if !can_delete(w, id) {
                    return Err(CadError::Invalid("Reassign parts, occurrences and configurations using this finish before deleting it".into()));
                }
                w.dispatch(Command::DeleteFinish { finish: id.clone() });
                if w.session.visible().finishes.contains_key(id) {
                    return Err(CadError::Invalid(w.status.clone()));
                }
                if let Some(id) = initial(w) {
                    load(w, &id);
                }
            }
            "appearance.assign-part" => {
                let definition = arguments
                    .and_then(|v| v.get("definition"))
                    .and_then(Value::as_str)
                    .map(str::to_owned)
                    .or_else(|| w.view.active_part.clone())
                    .ok_or_else(|| {
                        CadError::Invalid("Select a part to assign its visual finish".into())
                    })?;
                let finish = finish.ok_or_else(|| CadError::Invalid("Choose a finish".into()))?;
                w.dispatch(Command::SetPartFinish { definition, finish });
            }
            "appearance.assign-instance" | "appearance.inherit-instance" => {
                let target = arguments
                    .and_then(|v| {
                        Some((
                            v.get("assembly")?.as_str()?.into(),
                            v.get("instance")?.as_str()?.into(),
                        ))
                    })
                    .or_else(|| target_instance(w))
                    .ok_or_else(|| {
                        CadError::Invalid(
                            "Select one component occurrence to override its visual finish".into(),
                        )
                    })?;
                w.dispatch(Command::SetInstanceFinish {
                    assembly: target.0,
                    instance: target.1,
                    finish: if command == "appearance.inherit-instance" {
                        None
                    } else {
                        finish
                    },
                });
            }
            "appearance.texture.import" | "appearance.texture.clear" => {
                let id =
                    finish.ok_or_else(|| CadError::Invalid("Choose a visual finish".into()))?;
                let slot = arguments
                    .and_then(|v| v.get("slot"))
                    .and_then(Value::as_str)
                    .ok_or_else(|| CadError::Invalid("Choose the texture map to replace".into()))?;
                texture(w, &id, slot, command == "appearance.texture.import")?;
            }
            _ if command.starts_with(IMPORT) || command.starts_with(CLEAR) => {
                let payload = command
                    .strip_prefix(IMPORT)
                    .or_else(|| command.strip_prefix(CLEAR))
                    .unwrap_or_default();
                let (id, slot) = payload.rsplit_once('/').ok_or_else(|| {
                    CadError::Invalid("Texture action is missing its finish and map".into())
                })?;
                texture(w, id, slot, command.starts_with(IMPORT))?;
            }
            _ => {
                return Err(CadError::Invalid(format!(
                    "Unknown visual finish command: {command}"
                )));
            }
        }
        w.rendered_key.clear();
        Ok(())
    })();
    if let Err(error) = result {
        w.status = error.to_string();
    }
    true
}
fn texture(w: &mut Workspace, finish_id: &str, slot: &str, import: bool) -> Result<()> {
    if ![
        "base_color",
        "normal",
        "roughness",
        "metallic",
        "occlusion",
        "height",
    ]
    .contains(&slot)
    {
        return Err(CadError::Invalid("Unknown texture map".into()));
    }
    let finish = w
        .session
        .visible()
        .finishes
        .get(finish_id)
        .cloned()
        .ok_or_else(|| CadError::Invalid("The visual finish no longer exists".into()))?;
    let mut document = w.session.visible().clone();
    let resource = if import {
        let Some(path) = rfd::FileDialog::new()
            .set_title("Import a visual texture map")
            .add_filter(
                "Texture images",
                &["png", "jpg", "jpeg", "tif", "tiff", "webp"],
            )
            .pick_file()
        else {
            return Ok(());
        };
        crate::texture_assets::validate_texture_file(&path)?;
        Some(cad_core::persistence::import_resource(
            &mut document,
            &w.root(),
            &path,
        )?)
    } else {
        None
    };
    let mut source = serde_json::to_value(&finish)?;
    source["textures"][slot] = json!(resource);
    let finish: Finish = serde_json::from_value(source)?;
    document.finishes.insert(finish_id.into(), finish);
    document.validate()?;
    let expected = document.fingerprint();
    w.dispatch(Command::ReplaceDocument { document });
    if w.session.visible().fingerprint() != expected {
        return Err(CadError::Invalid(w.status.clone()));
    }
    load(w, finish_id);
    Ok(())
}

fn finish_source(finish: &Finish) -> Value {
    let mut source = serde_json::to_value(finish).unwrap_or_default();
    for slot in [
        "base_color",
        "normal",
        "roughness",
        "metallic",
        "occlusion",
        "height",
    ] {
        if source["textures"].get(slot).is_none() {
            source["textures"][slot] = Value::Null;
        }
    }
    source
}
pub fn current_value(w: &Workspace, element: &SemanticElement) -> Option<Value> {
    match element.data.get("setter")?.as_str()? {
        "appearance-ui" => match element.data.get("field")?.as_str()? {
            "library" => Some(json!(selected(w).or_else(|| initial(w)))),
            "source" => Some(json!(
                w.fields
                    .get(SOURCE)
                    .cloned()
                    .or_else(|| selected(w)
                        .and_then(|id| w.session.visible().finishes.get(&id))
                        .and_then(|finish| serde_json::to_string_pretty(finish).ok()))
                    .unwrap_or_default()
            )),
            _ => None,
        },
        "appearance-finish" => finish_source(
            w.session
                .visible()
                .finishes
                .get(element.data.get("finish")?.as_str()?)?,
        )
        .pointer(element.data.get("field")?.as_str()?)
        .cloned(),
        "appearance-lighting" => serde_json::to_value(&w.session.visible().lighting)
            .ok()?
            .pointer(element.data.get("field")?.as_str()?)
            .cloned(),
        "appearance-part" => Some(json!(
            w.session
                .visible()
                .parts
                .get(element.data.get("definition")?.as_str()?)?
                .finish
        )),
        "appearance-instance" => Some(json!(
            w.session
                .visible()
                .assemblies
                .get(element.data.get("assembly")?.as_str()?)?
                .instances
                .values()
                .find(|instance| Some(instance.id.as_str())
                    == element.data.get("instance").and_then(Value::as_str))?
                .finish
        )),
        _ => None,
    }
}
pub fn set_value(w: &mut Workspace, element: &SemanticElement, value: Value) -> Result<()> {
    if !available(w) {
        return Err(CadError::Invalid(
            "Apply or cancel the current operation before editing visual finishes".into(),
        ));
    }
    let setter = element
        .data
        .get("setter")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if setter == "appearance-ui"
        && element.data.get("field").and_then(Value::as_str) == Some("library")
    {
        let id = value
            .as_str()
            .filter(|id| w.session.visible().finishes.contains_key(*id))
            .ok_or_else(|| CadError::Invalid("Choose a visual finish from the library".into()))?;
        load(w, id);
        return Ok(());
    }
    if !w.fields.contains_key(TOOL) {
        begin(w, false);
    }
    match setter {
        "appearance-ui" => match element.data.get("field").and_then(Value::as_str) {
            Some("library") => {
                let id = value
                    .as_str()
                    .filter(|id| w.session.visible().finishes.contains_key(*id))
                    .ok_or_else(|| {
                        CadError::Invalid("Choose a visual finish from the library".into())
                    })?;
                load(w, id);
            }
            Some("source") => {
                let source = value
                    .as_str()
                    .ok_or_else(|| CadError::Invalid("Finish source must be JSON text".into()))?;
                w.fields.insert(SOURCE.into(), source.into());
                let result = (|| -> Result<()> {
                    let finish: Finish = serde_json::from_str(source)?;
                    if selected(w).as_ref() != Some(&finish.id) {
                        return Err(CadError::Invalid(
                            "The finish identity cannot change in its editor".into(),
                        ));
                    }
                    put(w, finish)
                })();
                if let Err(error) = result {
                    w.fields.insert(INVALID.into(), error.to_string());
                    return Err(error);
                }
            }
            _ => {
                return Err(CadError::Invalid(
                    "Unknown visual finish editor field".into(),
                ));
            }
        },
        "appearance-finish" => {
            let id = element
                .data
                .get("finish")
                .and_then(Value::as_str)
                .ok_or_else(|| CadError::Invalid("Missing finish identity".into()))?;
            let mut source =
                finish_source(w.session.visible().finishes.get(id).ok_or_else(|| {
                    CadError::Invalid("The visual finish no longer exists".into())
                })?);
            let pointer = element
                .data
                .get("field")
                .and_then(Value::as_str)
                .ok_or_else(|| CadError::Invalid("Missing finish field".into()))?;
            *source
                .pointer_mut(pointer)
                .ok_or_else(|| CadError::Invalid("The finish field no longer exists".into()))? =
                value;
            put(w, serde_json::from_value(source)?)?;
        }
        "appearance-lighting" => {
            let mut source = serde_json::to_value(&w.session.visible().lighting)?;
            let pointer = element
                .data
                .get("field")
                .and_then(Value::as_str)
                .ok_or_else(|| CadError::Invalid("Missing lighting field".into()))?;
            *source
                .pointer_mut(pointer)
                .ok_or_else(|| CadError::Invalid("The lighting field no longer exists".into()))? =
                value;
            let lighting: Lighting = serde_json::from_value(source.clone())?;
            checked_dispatch(w, Command::SetLighting { lighting }, source, None)?;
        }
        "appearance-part" => {
            let definition = element
                .data
                .get("definition")
                .and_then(Value::as_str)
                .ok_or_else(|| CadError::Invalid("Missing part identity".into()))?;
            let finish = value
                .as_str()
                .ok_or_else(|| CadError::Invalid("Choose a named visual finish".into()))?;
            w.dispatch(Command::SetPartFinish {
                definition: definition.into(),
                finish: finish.into(),
            });
            if w.session
                .visible()
                .parts
                .get(definition)
                .is_none_or(|part| part.finish != finish)
            {
                return Err(CadError::Invalid(w.status.clone()));
            }
        }
        "appearance-instance" => {
            let assembly = element
                .data
                .get("assembly")
                .and_then(Value::as_str)
                .ok_or_else(|| CadError::Invalid("Missing assembly identity".into()))?;
            let instance = element
                .data
                .get("instance")
                .and_then(Value::as_str)
                .ok_or_else(|| CadError::Invalid("Missing component identity".into()))?;
            let finish: Option<Id> = serde_json::from_value(value)?;
            w.dispatch(Command::SetInstanceFinish {
                assembly: assembly.into(),
                instance: instance.into(),
                finish: finish.clone(),
            });
            if w.session
                .visible()
                .assemblies
                .get(assembly)
                .and_then(|assembly| {
                    assembly
                        .instances
                        .values()
                        .find(|source| source.id == instance)
                })
                .is_none_or(|instance| instance.finish != finish)
            {
                return Err(CadError::Invalid(w.status.clone()));
            }
        }
        _ => {
            return Err(CadError::Invalid(
                "No visual finish setter for this field".into(),
            ));
        }
    }
    w.rendered_key.clear();
    Ok(())
}

fn button(ui: &mut egui::Ui, w: &mut Workspace, command: &str, label: &str, enabled: bool) {
    if controls::button(
        ui,
        &format!("command:{command}"),
        "appearance-action",
        &["semantic-control"],
        label,
        enabled,
        false,
    )
    .clicked()
    {
        action(w, command, None);
    }
}
fn field_element(
    id: &str,
    field: &str,
    label: &str,
    value: Value,
    lighting: bool,
    choices: Value,
    unit: Option<&str>,
) -> SemanticElement {
    SemanticElement {
        id: if lighting {
            format!("viewport-lighting::{field}")
        } else {
            format!("finish:{id}::{field}")
        },
        role: if lighting {
            "lighting-field"
        } else {
            "appearance-field"
        }
        .into(),
        label: label.into(),
        value: Some(
            value
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| value.to_string()),
        ),
        state: "draft".into(),
        commands: vec![],
        relationships: if lighting { vec![] } else { vec![id.into()] },
        data: json!({"setter":if lighting {"appearance-lighting"} else {"appearance-finish"},"finish":id,"field":field,"json":value,"editable":true,"choices":choices,"unit":unit,"accessibility_label":label}),
    }
}
struct Fields {
    elements: std::collections::BTreeMap<String, SemanticElement>,
}
impl Fields {
    fn new(w: &Workspace, id: &str, lighting: bool) -> Self {
        let setter = if lighting {
            "appearance-lighting"
        } else {
            "appearance-finish"
        };
        let elements = cad_core::appearance::semantic(
            w.session.visible(),
            w.view.active_part.as_deref(),
            "draft",
        )
        .into_iter()
        .filter(|element| {
            element.data.get("setter").and_then(Value::as_str) == Some(setter)
                && (lighting || element.data.get("finish").and_then(Value::as_str) == Some(id))
        })
        .filter_map(|element| Some((element.data.get("field")?.as_str()?.to_owned(), element)))
        .collect();
        Self { elements }
    }
}
fn field(
    ui: &mut egui::Ui,
    w: &mut Workspace,
    fields: &Fields,
    pointer: &str,
    label: &str,
    choices: Value,
) {
    let Some(mut element) = fields.elements.get(pointer).cloned() else {
        return;
    };
    element.label = label.into();
    if choices
        .as_array()
        .is_some_and(|choices| !choices.is_empty())
    {
        element.data["choices"] = choices;
    }
    crate::ui::semantic_field(ui, w, &element);
}
pub fn show(ui: &mut egui::Ui, w: &mut Workspace) {
    if !available(w) {
        ui.weak("Apply or cancel the active operation before editing visual finishes");
        return;
    }
    if !w.fields.contains_key(EDITOR) {
        if available(w) {
            if let Some(id) = initial(w) {
                load(w, &id);
            }
        } else {
            ui.weak("Finish the active operation first");
            return;
        }
    }
    if w.fields.contains_key(TOOL) {
        cad_ui::confirmation(ui, w);
    }
    if w.dialog.is_none() && !w.fields.contains_key(EDITOR) {
        return;
    }
    ui.horizontal(|ui| {
        for (tab, label) in [("finish", "Finishes"), ("lighting", "Lighting")] {
            if controls::button(
                ui,
                &format!("appearance:tab:{tab}"),
                "appearance-tab",
                &["semantic-control"],
                label,
                true,
                w.fields.get("appearance-tab").map(String::as_str) == Some(tab),
            )
            .clicked()
            {
                action(w, &format!("appearance.tab.{tab}"), None);
            }
        }
    });
    if w.fields.get("appearance-tab").map(String::as_str) == Some("lighting") {
        lighting_controls(ui, w);
        return;
    }
    let selected_id = selected(w);
    crate::ui::semantic_field(ui, w, &library_element(w));
    ui.horizontal_wrapped(|ui| {
        button(ui, w, "appearance.new", "New", true);
        button(
            ui,
            w,
            "appearance.duplicate",
            "Duplicate",
            selected_id.is_some(),
        );
        button(
            ui,
            w,
            "appearance.delete",
            "Delete",
            selected_id.as_ref().is_some_and(|id| can_delete(w, id)),
        );
    });
    ui.horizontal_wrapped(|ui| {
        button(
            ui,
            w,
            "appearance.assign-part",
            "Assign to part (base)",
            w.view.active_part.is_some(),
        );
        button(
            ui,
            w,
            "appearance.assign-instance",
            "Assign to occurrence",
            target_instance(w).is_some(),
        );
        button(
            ui,
            w,
            "appearance.inherit-instance",
            "Use part finish",
            target_instance(w).is_some(),
        );
    });
    ui.small("Visual finishes affect appearance. Engineering materials control mass and manufacturing data.");
    let Some(id) = selected(w) else {
        return;
    };
    let fields = Fields::new(w, &id, false);
    field(ui, w, &fields, "/name", "Finish name", json!([]));
    if let Some(finish) = w.session.visible().finishes.get(&id) {
        let mut color = finish.base_color.map(|value| value as f32);
        ui.horizontal(|ui| {
            ui.label("Base color");
            if ui.color_edit_button_rgb(&mut color).changed() {
                let element = field_element(
                    &id,
                    "/base_color",
                    "Base color",
                    json!(color),
                    false,
                    json!([]),
                    None,
                );
                if let Err(error) = set_value(w, &element, json!(color.map(f64::from))) {
                    w.status = error.to_string();
                }
            }
        });
    }
    egui::CollapsingHeader::new("Surface response")
        .id_salt("appearance:surface")
        .default_open(true)
        .show(ui, |ui| {
            for (pointer, label) in [
                ("/metallic", "Metallic"),
                ("/roughness", "Roughness"),
                ("/clearcoat", "Clear coat"),
                ("/clearcoat_roughness", "Coat roughness"),
                ("/normal_strength", "Normal strength"),
                ("/height_mm", "Bump depth (mm)"),
            ] {
                field(ui, w, &fields, pointer, label, json!([]));
            }
        });
    egui::CollapsingHeader::new("Texture maps").id_salt("appearance:textures").default_open(true).show(ui,|ui| {
        for (slot,label) in [("base_color","Color"),("normal","Normal"),("height","Height / bump"),("roughness","Roughness"),("metallic","Metallic"),("occlusion","Occlusion")] {
            let map=serde_json::to_value(w.session.visible().finishes.get(&id)).ok().and_then(|source|source.get("textures")?.get(slot)?.as_str().map(str::to_owned));
            let path=map.as_ref().and_then(|map|w.session.visible().resources.get(map)).map(|resource|resource.path.clone());
            ui.horizontal_wrapped(|ui| {
                ui.label(label);
                button(ui,w,&format!("{IMPORT}{id}/{slot}"),if map.is_some(){"Replace…"}else{"Import…"},true);
                button(ui,w,&format!("{CLEAR}{id}/{slot}"),"Clear",map.is_some());
            });
            if let Some(path)=path {ui.small(&path).on_hover_text(&path);} else if map.is_some(){ui.colored_label(ui.visuals().error_fg_color,"Missing pinned image; import a replacement.");}
            field(ui,w,&fields,&format!("/textures/{slot}"),"Pinned image",resource_choices(w));
        }
        ui.small("Grayscale height maps add lit surface relief. Bump depth is in mm. Color maps use sRGB; all other maps use linear data.");
    });
    egui::CollapsingHeader::new("Texture mapping").id_salt("appearance:mapping").default_open(false).show(ui,|ui| {
        field(ui,w,&fields,"/mapping/mode","Projection",json!([{"value":"triplanar","label":"Box / triplanar"},{"value":"planar","label":"Planar"}]));
        for (pointer,label,unit) in [("/mapping/scale_mm/0","Width","mm"),("/mapping/scale_mm/1","Height","mm"),("/mapping/rotation_deg","Direction","deg"),("/mapping/offset_mm/0","Offset U","mm"),("/mapping/offset_mm/1","Offset V","mm")] {field(ui,w,&fields,pointer,&format!("{label} ({unit})"),json!([]));}
    });
    egui::CollapsingHeader::new("Advanced source")
        .id_salt("appearance:source")
        .show(ui, |ui| {
            crate::ui::semantic_field(ui, w, &source_element(w));
        });
    if let Some(error) = w.fields.get(INVALID) {
        ui.colored_label(ui.visuals().error_fg_color, error);
    }
    diagnostics(ui, w);
}
fn resource_choices(w: &Workspace) -> Value {
    let mut choices = vec![json!({"value":null,"label":"None"})];
    choices.extend(
        w.session
            .visible()
            .resources
            .values()
            .filter(|resource| {
                ["png", "jpg", "jpeg", "tif", "tiff", "webp"].contains(&resource.kind.as_str())
            })
            .map(|resource| json!({"value":resource.id,"label":resource.path})),
    );
    json!(choices)
}
fn lighting_controls(ui: &mut egui::Ui, w: &mut Workspace) {
    let fields = Fields::new(w, "lighting", true);
    ui.small("Studio reflections and direct lights update while you design.");
    for (pointer, label, unit) in [
        ("/environment_rotation_deg", "Studio rotation", Some("deg")),
        ("/exposure", "Exposure", Some("stops")),
        ("/intensity", "Environment strength", None),
        ("/key_intensity", "Key light", None),
        ("/fill_intensity", "Fill light", None),
        ("/shadows", "Shadows", None),
    ] {
        field(
            ui,
            w,
            &fields,
            pointer,
            &unit
                .map(|unit| format!("{label} ({unit})"))
                .unwrap_or_else(|| label.into()),
            json!([]),
        );
    }
    diagnostics(ui, w);
}
pub fn lighting_view(ui: &mut egui::Ui, w: &mut Workspace) {
    if !available(w) {
        ui.weak("Apply or cancel the active operation before editing lighting");
        return;
    }
    if !w.fields.contains_key(EDITOR) {
        if available(w) {
            if let Some(id) = initial(w) {
                load(w, &id);
            }
        } else {
            ui.weak("Finish the active operation first");
            return;
        }
    }
    if w.fields.contains_key(TOOL) {
        cad_ui::confirmation(ui, w);
    }
    lighting_controls(ui, w);
}
fn diagnostics(ui: &mut egui::Ui, w: &Workspace) {
    for (key, error) in w
        .fields
        .iter()
        .filter(|(key, _)| key.starts_with("appearance-field-invalid:"))
    {
        ui.colored_label(ui.visuals().error_fg_color, error)
            .on_hover_text(key);
    }
    for error in &w.appearance_diagnostics {
        ui.colored_label(ui.visuals().error_fg_color, error);
    }
}
pub fn semantics(w: &Workspace) -> Vec<SemanticElement> {
    let mut elements = vec![library_element(w), source_element(w)];
    for (command, label, enabled) in [
        ("appearance.new", "New visual finish", available(w)),
        (
            "appearance.duplicate",
            "Duplicate visual finish",
            available(w) && selected(w).is_some(),
        ),
        (
            "appearance.delete",
            "Delete visual finish",
            available(w) && selected(w).as_ref().is_some_and(|id| can_delete(w, id)),
        ),
        (
            "appearance.assign-part",
            "Assign finish to base part",
            available(w) && w.view.active_part.is_some(),
        ),
        (
            "appearance.assign-instance",
            "Assign finish to occurrence",
            available(w) && target_instance(w).is_some(),
        ),
        (
            "appearance.inherit-instance",
            "Use part finish",
            available(w) && target_instance(w).is_some(),
        ),
    ] {
        elements.push(SemanticElement {id:format!("command:{command}"),role:"appearance-action".into(),label:label.into(),value:(!enabled).then(||"Select the required part or unused visual finish; finish the active operation first".into()),state:if enabled{"ready"}else{"disabled"}.into(),commands:vec![command.into()],relationships:vec!["workspace.appearances".into()],data:json!({"finish":selected(w),"accessibility_label":label})});
    }
    for (tab, label) in [("finish", "Finishes"), ("lighting", "Lighting")] {
        elements.push(SemanticElement {
            id: format!("appearance:tab:{tab}"),
            role: "appearance-tab".into(),
            label: label.into(),
            value: None,
            state: if w.fields.get("appearance-tab").map(String::as_str) == Some(tab) {
                "selected"
            } else {
                "ready"
            }
            .into(),
            commands: vec![format!("appearance.tab.{tab}")],
            relationships: vec!["workspace.appearances".into()],
            data: json!({"accessibility_label":label}),
        });
    }
    for (id, role, label) in [
        (
            "workspace.appearances",
            "appearances-editor",
            "Visual finishes",
        ),
        ("workspace.lighting", "lighting-editor", "Studio lighting"),
    ] {
        elements.push(SemanticElement {
            id: id.into(),
            role: role.into(),
            label: label.into(),
            value: None,
            state: "ready".into(),
            commands: vec![],
            relationships: vec![],
            data: json!({"renderer":role}),
        });
    }
    for finish in w.session.visible().finishes.values() {
        for slot in [
            "base_color",
            "normal",
            "roughness",
            "metallic",
            "occlusion",
            "height",
        ] {
            for (prefix, label) in [(IMPORT, "Import texture"), (CLEAR, "Clear texture")] {
                let command = format!("{prefix}{}/{slot}", finish.id);
                elements.push(SemanticElement {
                    id: format!("command:{command}"),
                    role: "appearance-action".into(),
                    label: format!("{label} · {slot}"),
                    value: None,
                    state: if available(w) { "ready" } else { "disabled" }.into(),
                    commands: vec![command],
                    relationships: vec![finish.id.clone()],
                    data: json!({"finish":finish.id,"slot":slot}),
                });
            }
        }
    }
    elements
}
pub fn reconcile_semantics(w: &Workspace, elements: &mut [SemanticElement]) {
    for element in elements {
        if element
            .data
            .get("setter")
            .and_then(Value::as_str)
            .is_some_and(|setter| setter.starts_with("appearance-"))
        {
            if !available(w) {
                element.data["editable"] = false.into();
                element.state = "disabled".into();
                element.data["diagnostics"] =
                    json!(["Apply or cancel the active operation before editing visual finishes"]);
            } else if let Some(error) = w
                .fields
                .get(&format!("appearance-field-invalid:{}", element.id))
            {
                element.state = "invalid".into();
                element.data["diagnostics"] = json!([error]);
            }
        }
    }
}
fn library_element(w: &Workspace) -> SemanticElement {
    SemanticElement {
        id: "appearance:library".into(),
        role: "appearance-library-selection".into(),
        label: "Visual finish".into(),
        value: selected(w).or_else(|| initial(w)),
        state: "ready".into(),
        commands: vec![],
        relationships: vec!["surface-finishes".into()],
        data: json!({"setter":"appearance-ui","field":"library","json":selected(w).or_else(||initial(w)),"editable":available(w),"choices":w.session.visible().finishes.values().map(|finish|json!({"value":finish.id,"label":finish.name})).collect::<Vec<_>>()}),
    }
}
fn source_element(w: &Workspace) -> SemanticElement {
    SemanticElement {
        id: "appearance:source".into(),
        role: "appearance-source".into(),
        label: "Finish JSON".into(),
        value: w.fields.get(SOURCE).cloned(),
        state: if w.fields.contains_key(INVALID) {
            "invalid"
        } else {
            "ready"
        }
        .into(),
        commands: vec![],
        relationships: selected(w).into_iter().collect(),
        data: json!({"setter":"appearance-ui","field":"source","json":w.fields.get(SOURCE).cloned().unwrap_or_default(),"editable":available(w)&&selected(w).is_some(),"multiline":true,"accessibility_label":"Advanced visual finish source"}),
    }
}
