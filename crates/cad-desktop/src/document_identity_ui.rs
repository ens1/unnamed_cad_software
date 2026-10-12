//! Document names are editable metadata over the existing shared draft.
use crate::{Workspace, cad_ui, presentation_controls as controls};
use bevy_egui::egui;
use cad_core::{CadError, Document, Result, commands::Command, presentation::SemanticElement};
use serde_json::{Value, json};

pub const CONTRACT_VERSION: u32 = 1;
const TOOL: &str = "document-identity-tool";
const DOCUMENT: &str = "document-identity-document";
const TOKEN: &str = "document-identity-token";
const NAME: &str = "document-identity-name";
const PART: &str = "document-identity-part";
const PART_NAME: &str = "document-identity-part-before-name";
const SYNC: &str = "document-identity-sync-part";
const INVALID: &str = "document-identity-invalid:";

pub fn active(w: &Workspace) -> bool {
    w.fields.contains_key(TOOL)
}
fn available(w: &Workspace) -> bool {
    w.sketch.is_none()
        && w.settings.is_none()
        && !crate::drawing_interaction_ui::active(w)
        && (w.tool_checkpoint.is_none() || active(w))
}
fn standalone_part(document: &Document) -> Option<&str> {
    if document.parts.len() != 1 || document.assemblies.len() > 1 {
        return None;
    }
    let part = document.parts.values().next()?;
    if document.root != part.id
        && document.properties.get("document_kind").map(String::as_str) != Some("part")
    {
        return None;
    }
    let one_part_root = document.root == part.id
        || document
            .assemblies
            .get(&document.root)
            .is_some_and(|assembly| {
                assembly.instances.len() == 1
                    && assembly
                        .instances
                        .values()
                        .next()
                        .is_some_and(|instance| instance.definition == part.id)
            });
    (one_part_root && !cad_core::library::is_definition_pinned(document, &part.id))
        .then_some(part.id.as_str())
}
fn validate_name(name: &str) -> Result<()> {
    if name.trim().is_empty() {
        return Err(CadError::Invalid("Enter a document name".into()));
    }
    if name.chars().count() > 512 || name.chars().any(char::is_control) {
        return Err(CadError::Invalid(
            "Use a single-line document name of at most 512 characters".into(),
        ));
    }
    Ok(())
}
pub fn begin(w: &mut Workspace) {
    if !available(w) {
        w.status = "Apply or cancel the active operation before renaming the document".into();
        return;
    }
    if active(w) {
        w.open_editor("document-identity");
        return;
    }
    let document = w.session.visible();
    let name = document.name.clone();
    let document_id = document.id.clone();
    let part = standalone_part(document)
        .and_then(|id| document.parts.get(id))
        .map(|part| (part.id.clone(), part.name.clone()));
    w.start_tool();
    w.fields.insert(TOOL.into(), "true".into());
    w.fields.insert(DOCUMENT.into(), document_id);
    w.fields.insert(TOKEN.into(), cad_core::id());
    w.fields.insert(NAME.into(), name);
    w.fields
        .insert("focus-request:document-identity:name".into(), "true".into());
    w.fields.insert(SYNC.into(), part.is_some().to_string());
    if let Some((id, name)) = part {
        w.fields.insert(PART.into(), id);
        w.fields.insert(PART_NAME.into(), name);
    }
    w.open_editor("document-identity");
    w.status = "Rename the document, then confirm to save its name".into();
}
pub fn cancel(w: &mut Workspace) {
    w.fields.remove("focus-request:document-identity:name");
    w.fields.retain(|key, _| {
        !key.starts_with("document-identity-")
            && !key.starts_with("presentation-field:document-identity:")
    });
}
fn matches_control(w: &Workspace, element: &SemanticElement) -> bool {
    active(w)
        && available(w)
        && element.data.get("setter").and_then(Value::as_str) == Some("document-identity")
        && element.data.get("document").and_then(Value::as_str)
            == Some(w.session.visible().id.as_str())
        && element.data.get("token").and_then(Value::as_str)
            == w.fields.get(TOKEN).map(String::as_str)
        && w.fields.get(DOCUMENT).map(String::as_str) == Some(w.session.visible().id.as_str())
}
pub fn current_source_is_valid(w: &Workspace) -> bool {
    !active(w)
        || (w.fields.get(DOCUMENT).map(String::as_str) == Some(w.session.visible().id.as_str())
            && w.fields
                .get(NAME)
                .is_some_and(|name| validate_name(name).is_ok())
            && !w.fields.keys().any(|key| key.starts_with(INVALID)))
}
pub fn ready_to_confirm(w: &mut Workspace) -> bool {
    if current_source_is_valid(w) {
        true
    } else {
        w.status = "Correct the document name before confirming".into();
        false
    }
}
pub fn field_is_invalid(w: &Workspace, element: &SemanticElement) -> bool {
    element.data.get("setter").and_then(Value::as_str) == Some("document-identity")
        && w.fields.contains_key(&format!("{INVALID}{}", element.id))
}
pub fn field_error(w: &mut Workspace, element: &SemanticElement, error: Option<String>) {
    if !matches_control(w, element) {
        return;
    }
    let key = format!("{INVALID}{}", element.id);
    if let Some(error) = error {
        w.fields.insert(key, error);
        w.confirm_requested = false;
    } else {
        w.fields.remove(&key);
    }
}
fn submit(w: &mut Workspace) -> Result<()> {
    let name = w
        .fields
        .get(NAME)
        .ok_or_else(|| CadError::Invalid("The document name editor is stale".into()))?
        .clone();
    validate_name(&name)?;
    let mut document = w.session.visible().clone();
    if w.fields.get(DOCUMENT).map(String::as_str) != Some(document.id.as_str()) {
        return Err(CadError::Invalid(
            "The rename action belongs to another document".into(),
        ));
    }
    document.name = name.trim().into();
    if let Some(part_id) = w.fields.get(PART) {
        if standalone_part(&document) != Some(part_id.as_str()) {
            return Err(CadError::Invalid(
                "The standalone part changed; reopen Rename Document".into(),
            ));
        }
        let part = document
            .parts
            .get_mut(part_id)
            .ok_or_else(|| CadError::Invalid("The part name target is missing".into()))?;
        part.name = if w.fields.get(SYNC).is_some_and(|value| value == "true") {
            document.name.clone()
        } else {
            w.fields
                .get(PART_NAME)
                .ok_or_else(|| CadError::Invalid("The original part name is missing".into()))?
                .clone()
        };
    }
    if w.session.visible() != &document {
        let expected = document.clone();
        w.dispatch(Command::ReplaceDocument { document });
        if w.session.visible() != &expected {
            return Err(CadError::Invalid(w.status.clone()));
        }
    }
    Ok(())
}
pub fn action(w: &mut Workspace, command: &str) -> bool {
    match command {
        "document.rename" => begin(w),
        "document.rename.apply" if active(w) => {
            if ready_to_confirm(w) {
                if w.session.draft.is_some() {
                    w.confirm_tool();
                } else {
                    w.cancel_tool();
                    w.status = "Name unchanged; editor closed".into();
                }
            }
        }
        "document.rename.cancel" if active(w) => w.cancel_tool(),
        _ => return false,
    }
    true
}
pub fn semantic_action(w: &mut Workspace, command: &str, arguments: &Value) -> bool {
    if !matches!(command, "document.rename.apply" | "document.rename.cancel") {
        return false;
    }
    if !active(w)
        || arguments.get("document").and_then(Value::as_str)
            != Some(w.session.visible().id.as_str())
        || arguments.get("token").and_then(Value::as_str) != w.fields.get(TOKEN).map(String::as_str)
    {
        w.status = "The rename action is stale; use the current editor".into();
        return true;
    }
    action(w, command)
}
pub fn value(w: &Workspace, element: &SemanticElement) -> Option<Value> {
    if !matches_control(w, element) {
        return None;
    }
    match element.data.get("field")?.as_str()? {
        "name" => Some(json!(w.fields.get(NAME)?)),
        "sync-part" => Some(json!(
            w.fields.get(SYNC).is_some_and(|value| value == "true")
        )),
        _ => None,
    }
}
pub fn set_value(w: &mut Workspace, element: &SemanticElement, value: Value) -> Result<()> {
    if !matches_control(w, element) {
        return Err(CadError::Invalid(
            "The rename control is stale; reopen Rename Document".into(),
        ));
    }
    match element.data.get("field").and_then(Value::as_str) {
        Some("name") => {
            let name = value
                .as_str()
                .ok_or_else(|| CadError::Invalid("Enter a document name".into()))?;
            w.fields.insert(NAME.into(), name.into());
        }
        Some("sync-part") if w.fields.contains_key(PART) => {
            let sync = value.as_bool().ok_or_else(|| {
                CadError::Invalid("Choose whether to synchronize the part name".into())
            })?;
            w.fields.insert(SYNC.into(), sync.to_string());
            // The checkbox remains usable while the user is replacing a name.
            // Its choice is applied together with the next valid name; it must
            // not acquire a second, unrelated name-validation error.
            if w.fields
                .get(NAME)
                .is_none_or(|name| validate_name(name).is_err())
            {
                return Ok(());
            }
        }
        _ => return Err(CadError::Invalid("The rename field is unavailable".into())),
    }
    submit(w)
}
pub fn semantics(w: &Workspace) -> Vec<SemanticElement> {
    let mut elements = vec![SemanticElement {
        id: "workspace.document-identity".into(),
        role: "document-identity-editor".into(),
        label: "Rename Document".into(),
        value: Some(w.session.visible().name.clone()),
        state: if active(w) { "open" } else { "closed" }.into(),
        commands: vec!["document.rename".into()],
        relationships: vec![w.session.visible().id.clone()],
        data: json!({"contract":"tcad.document-identity","version":CONTRACT_VERSION,"renderer":"document-identity-editor",
            "enabled":available(w),"transaction":"shared-document-draft","identities_preserved":true,
            "save_path_changes":false,"undo":"one-confirmed-operation"}),
    }];
    if !active(w) {
        return elements;
    }
    for (field, label, value_type, value) in [
        (
            "name",
            "Document name",
            "string",
            json!(w.fields.get(NAME).cloned().unwrap_or_default()),
        ),
        (
            "sync-part",
            "Use document name for part",
            "boolean",
            json!(w.fields.get(SYNC).is_some_and(|value| value == "true")),
        ),
    ] {
        if field == "sync-part" && !w.fields.contains_key(PART) {
            continue;
        }
        elements.push(SemanticElement { id: format!("document-identity:{field}"), role: "document-identity-field".into(),label:label.into(),
            value: Some(value.as_str().map(str::to_owned).unwrap_or_else(|| value.to_string())),state:"draft".into(),commands:vec![],
            relationships:vec!["workspace.document-identity".into()],
            data:json!({"setter":"document-identity","field":field,"json":value,"value_type":value_type,
                "editable":available(w),"document":w.fields.get(DOCUMENT),"token":w.fields.get(TOKEN),"editor":w.fields.get(TOKEN),
                "focus_request":(field=="name").then_some("focus-request:document-identity:name"),
                "classes":["modeling-field"],"accessibility_label":label}),
        });
    }
    for (command, label, enabled) in [
        (
            "document.rename.apply",
            "Rename Document",
            current_source_is_valid(w),
        ),
        ("document.rename.cancel", "Cancel", true),
    ] {
        elements.push(SemanticElement {id:format!("command:{command}"),role:"document-identity-action".into(),label:label.into(),
            value:None,state:if enabled{"available"}else{"disabled"}.into(),commands:vec![command.into()],
            relationships:vec!["workspace.document-identity".into()],
            data:json!({"enabled":enabled,"document":w.fields.get(DOCUMENT),"token":w.fields.get(TOKEN),"apply_requires_current_validated_evaluation":command.ends_with(".apply")})});
    }
    elements.push(SemanticElement { id:"document-identity:help".into(),role:"document-identity-help".into(),label:
        "The name is saved with the project. Feature IDs, linked references and the project folder stay unchanged.".into(),
        value:w.project.as_ref().map(|path|path.display().to_string()),state:"ready".into(),commands:vec![],relationships:vec!["workspace.document-identity".into()],data:json!({}),
    });
    elements
}
pub fn controls(ui: &mut egui::Ui, w: &mut Workspace) {
    if !active(w) {
        begin(w);
    }
    if !active(w) {
        return;
    }
    controls::label(
        ui,
        "document-identity:title",
        "document-identity-heading",
        &[],
        "Rename Document",
        "editing",
    );
    cad_ui::confirmation(ui, w);
    for element in semantics(w)
        .into_iter()
        .filter(|element| element.role == "document-identity-field")
    {
        crate::ui::semantic_field(ui, w, &element);
    }
    for error in w
        .fields
        .iter()
        .filter(|(key, _)| key.starts_with(INVALID))
        .map(|(_, value)| value)
    {
        controls::label(
            ui,
            "document-identity:error",
            "document-identity-error",
            &[],
            error,
            "invalid",
        );
    }
    controls::label(
        ui,
        "document-identity:help",
        "document-identity-help",
        &[],
        "Names are saved in the project. The project folder and all source IDs remain unchanged.",
        "ready",
    );
}
