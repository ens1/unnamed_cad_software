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
const SAVE_AS: &str = "document-identity-save-as";

pub fn pending_save_as(w: &Workspace) -> bool {
    active(w) && w.fields.get(SAVE_AS).is_some_and(|mode| mode == "true")
}
/// The desktop shell consumes this intent only after the current validated
/// rename draft has committed (or the unchanged-name editor has closed).
pub fn take_after_confirm(w: &mut Workspace) -> bool {
    w.fields.remove(SAVE_AS).is_some_and(|mode| mode == "true")
}
pub fn begin_save_as(w: &mut Workspace) {
    if !available(w) {
        w.status = "Apply or cancel the active operation before naming a Save As copy".into();
        return;
    }
    begin(w);
    if active(w) {
        w.fields.insert(SAVE_AS.into(), "true".into());
        w.fields.insert(SYNC.into(), "false".into());
        let stem = w
            .session
            .visible()
            .name
            .chars()
            .map(|c| if "/\\:*?\"<>|".contains(c) { '_' } else { c })
            .collect::<String>();
        let folder = if w.project.is_some() {
            format!("{stem}-copy.tcad")
        } else {
            format!("{stem}.tcad")
        };
        let parent = w
            .project
            .as_ref()
            .and_then(|path| path.parent())
            .and_then(|path| path.parent())
            .map(std::path::Path::to_path_buf)
            .or_else(|| std::env::current_dir().ok());
        if let Some(parent) = parent {
            w.fields
                .entry("document-identity-save-as-path".into())
                .or_insert_with(|| {
                    parent
                        .join(folder)
                        .join("project.json")
                        .display()
                        .to_string()
                });
        }
        w.status = "Choose the destination and document name, then Save Copy".into();
    }
}
pub fn confirmation_label(w: &Workspace) -> &'static str {
    if pending_save_as(w) {
        "Save Copy"
    } else {
        "Rename Document"
    }
}
fn destination(value: &str) -> Result<std::path::PathBuf> {
    let value = value.trim();
    if value.is_empty() || value.chars().any(char::is_control) {
        return Err(CadError::Invalid(
            "Choose a project destination or enter its folder path".into(),
        ));
    }
    let path = std::path::PathBuf::from(value);
    let path = if path.file_name().and_then(|name| name.to_str()) == Some("project.json") {
        path
    } else if path.extension().and_then(|extension| extension.to_str()) == Some("json") {
        path.with_extension("tcad").join("project.json")
    } else {
        path.join("project.json")
    };
    if path
        .parent()
        .is_some_and(|parent| parent.exists() && !parent.is_dir())
    {
        return Err(CadError::Invalid(
            "The destination must be a project folder".into(),
        ));
    }
    Ok(path)
}

pub fn location_control(ui: &mut egui::Ui, w: &Workspace) {
    let state = if w.project.is_some() {
        "saved"
    } else {
        "unsaved"
    };
    let path = w.project.as_ref().map(|path| path.display().to_string());
    let label = path.as_ref().map_or_else(
        || "Unsaved project".into(),
        |_| {
            w.project
                .as_ref()
                .and_then(|path| path.parent())
                .and_then(|folder| folder.file_name())
                .map(|name| format!("Saved: {}", name.to_string_lossy()))
                .unwrap_or_else(|| "Saved project".into())
        },
    );
    controls::label(
        ui,
        "workspace.project-location",
        "project-location",
        &["status-control"],
        &label,
        state,
    )
    .on_hover_text(format!(
        "{}\n{}",
        w.session.committed.name,
        path.as_deref()
            .unwrap_or("Choose Save As to name and save this project")
    ));
}

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
            && (!pending_save_as(w)
                || w.fields
                    .get("document-identity-save-as-path")
                    .is_some_and(|path| destination(path).is_ok()))
            && !w.fields.keys().any(|key| key.starts_with(INVALID)))
}
pub fn ready_to_confirm(w: &mut Workspace) -> bool {
    if current_source_is_valid(w) {
        if pending_save_as(w)
            && let Some(path) = w
                .fields
                .get("document-identity-save-as-path")
                .and_then(|value| destination(value).ok())
        {
            w.fields.insert(
                "document-identity-save-as-path".into(),
                path.display().to_string(),
            );
        }
        true
    } else {
        w.status = "Correct the document name and save destination before confirming".into();
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
        "document.save-as-name" => crate::ui::action(w, "file.save-as"),
        "document.save-as-browse" if pending_save_as(w) => {
            if let Some(path) = crate::ui::choose_save_destination(w) {
                w.fields.insert(
                    "document-identity-save-as-path".into(),
                    path.display().to_string(),
                );
                w.fields.remove(&format!("{INVALID}document-identity:path"));
                w.fields.remove("presentation-field:document-identity:path");
            }
        }
        "document.rename.apply" if active(w) => {
            if ready_to_confirm(w) {
                // The common confirmation path handles validated drafts and
                // unchanged names, including the subsequent Save As request.
                w.confirm_tool();
            }
        }
        "document.rename.cancel" if active(w) => w.cancel_tool(),
        _ => return false,
    }
    true
}
pub fn semantic_action(w: &mut Workspace, command: &str, arguments: &Value) -> bool {
    if !matches!(
        command,
        "document.rename.apply" | "document.rename.cancel" | "document.save-as-browse"
    ) {
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
        "path" if pending_save_as(w) => {
            Some(json!(w.fields.get("document-identity-save-as-path")?))
        }
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
        Some("path") if pending_save_as(w) => {
            let path = value
                .as_str()
                .ok_or_else(|| CadError::Invalid("Enter a destination path".into()))?;
            w.fields
                .insert("document-identity-save-as-path".into(), path.into());
            destination(path)?;
            return Ok(());
        }
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
    let heading = if pending_save_as(w) {
        "Save As"
    } else {
        "Rename Document"
    };
    let mut elements = vec![SemanticElement {
        id: "workspace.document-identity".into(),
        role: "document-identity-editor".into(),
        label: heading.into(),
        value: Some(w.session.visible().name.clone()),
        state: if active(w) { "open" } else { "closed" }.into(),
        commands: vec!["document.rename".into()],
        relationships: vec![w.session.visible().id.clone()],
        data: json!({"contract":"tcad.document-identity","version":CONTRACT_VERSION,"renderer":"document-identity-editor",
            "enabled":available(w),"transaction":"shared-document-draft","identities_preserved":true,
            "save_path_changes":pending_save_as(w),"save_from_validated_staged_commit":pending_save_as(w),"promote_after_disk_success":pending_save_as(w),
            "project_path":w.project.as_ref().map(|path|path.display().to_string()),
            "save_as_destination":w.fields.get("document-identity-save-as-path"),
            "saved_document_name":w.session.committed.name,"undo":"one-confirmed-operation"}),
    }];
    elements.push(SemanticElement {
        id:"workspace.project-location".into(),role:"project-location".into(),
        label:w.session.committed.name.clone(),
        value:Some(w.project.as_ref().map(|path|path.display().to_string()).unwrap_or_else(||"Unsaved project".into())),
        state:if w.project.is_some(){"saved"}else{"unsaved"}.into(),
        commands:vec!["file.save-as".into(),"document.rename".into()],relationships:vec![w.session.visible().id.clone()],
        data:json!({"contract":"tcad.project-location","version":1,"document":w.session.visible().id,
            "project_path":w.project.as_ref().map(|path|path.display().to_string()),"name":w.session.committed.name,
            "draft_name":(w.session.visible().name!=w.session.committed.name).then_some(&w.session.visible().name),
            "actions":[{"command":"file.save-as","label":"Save As…","enabled":true},
                {"command":"document.rename","label":"Rename Document…","enabled":available(w)}]}),
    });
    if !active(w) {
        return elements;
    }
    if let Some(path) = w
        .fields
        .get("document-identity-save-as-path")
        .filter(|_| pending_save_as(w))
    {
        elements.push(SemanticElement {
            id: "document-identity:save-destination".into(),
            role: "document-identity-destination".into(),
            label: "Save destination".into(),
            value: Some(path.clone()),
            state: "draft".into(),
            commands: vec!["document.save-as-browse".into()],
            relationships: vec!["workspace.document-identity".into()],
            data: json!({"path":path,"document":w.fields.get(DOCUMENT),"token":w.fields.get(TOKEN),"filesystem_write_after_validated_commit":true}),
        });
    }
    for (field, label, value_type, value) in [
        (
            "path",
            "Project folder / manifest",
            "string",
            json!(
                w.fields
                    .get("document-identity-save-as-path")
                    .cloned()
                    .unwrap_or_default()
            ),
        ),
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
        if field == "path" && !pending_save_as(w) {
            continue;
        }
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
            confirmation_label(w),
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
        if pending_save_as(w) { "The folder name and document name are independent. Existing definition names are preserved unless you choose to rename the standalone part." }
        else { "The name is saved with the project. Feature IDs, linked references and the project folder stay unchanged." }.into(),
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
        if pending_save_as(w) {
            "Save As"
        } else {
            "Rename Document"
        },
        "editing",
    );
    cad_ui::confirmation(ui, w);
    for element in semantics(w)
        .into_iter()
        .filter(|element| element.role == "document-identity-field")
    {
        crate::ui::semantic_field(ui, w, &element);
        if element.data.get("field").and_then(Value::as_str) == Some("path")
            && controls::button(
                ui,
                "document-identity:browse",
                "document-identity-action",
                &["compact"],
                "Browse…",
                true,
                false,
            )
            .clicked()
        {
            action(w, "document.save-as-browse");
        }
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
        if pending_save_as(w) {
            "Folder and document names are independent. Definition names stay unchanged unless you select the part rename option."
        } else {
            "Names are saved in the project. The project folder and all source IDs remain unchanged."
        },
        "ready",
    );
}
