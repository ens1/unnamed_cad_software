//! Dense semantic manufacturing metadata editor over the shared document draft.
use crate::{Workspace, cad_ui, presentation_controls as controls};
use bevy_egui::egui;
use cad_core::{
    CadError, Result,
    commands::Command,
    model_properties::{self, Edit, ReferencePolicy, Scope},
    presentation::SemanticElement,
};
use serde_json::{Value, json};

const TOOL: &str = "properties-tool";
const OWNER: &str = "properties-owner";
const CONFIGURATION: &str = "properties-configuration";
const SELECTED: &str = "properties-selected";
const NEW_KEY: &str = "properties-new-key";
const NEW_VALUE: &str = "properties-new-value";
const RENAME: &str = "properties-rename";
const REFERENCES: &str = "properties-keep-bindings";
const FILTER: &str = "properties-filter";
const COMMON: &str = "properties-common";
const ERRORS: &str = "properties-field-invalid:";

fn available(w: &Workspace) -> bool {
    w.sketch.is_none()
        && (w.tool_checkpoint.is_none()
            || w.fields.contains_key(TOOL)
            || w.fields.contains_key("material-tool")
            || w.fields.contains_key("appearance-tool"))
}
fn default_owner(w: &Workspace) -> String {
    let document = w.session.visible();
    let definition = w
        .selected
        .as_ref()
        .map(|selection| &selection.definition)
        .or(w.view.active_part.as_ref())
        .unwrap_or(&document.root);
    if document.parts.contains_key(definition) || document.assemblies.contains_key(definition) {
        format!("definition:{definition}")
    } else {
        "document".into()
    }
}
pub fn scope(w: &Workspace) -> Scope {
    let owner = w
        .fields
        .get(OWNER)
        .cloned()
        .unwrap_or_else(|| default_owner(w));
    let Some(definition) = owner.strip_prefix("definition:") else {
        return Scope::Document;
    };
    // Retain a deleted scope as unresolved until the user chooses another;
    // never redirect a stale configuration edit to the base definition.
    let configuration = w
        .fields
        .get(CONFIGURATION)
        .filter(|configuration| !configuration.is_empty())
        .cloned();
    Scope::Definition {
        definition: definition.into(),
        configuration,
    }
}
fn editable(w: &Workspace) -> bool {
    available(w)
        && model_properties::local(w.session.visible(), &scope(w)).is_ok()
        && model_properties::editable(w.session.visible(), &scope(w))
}
fn clear_selection(w: &mut Workspace) {
    w.fields.remove(SELECTED);
    w.fields.remove(RENAME);
    w.fields.remove(REFERENCES);
}
pub fn begin(w: &mut Workspace) {
    if w.fields.contains_key("material-tool") && !crate::materials_ui::current_source_is_valid(w)
        || w.fields.contains_key("appearance-tool")
            && !crate::appearances_ui::current_source_is_valid(w)
    {
        w.status = "Correct the current material or appearance fields before switching to model properties".into();
        return;
    }
    if !available(w) {
        w.status = "Apply or cancel the active operation before editing model properties".into();
        return;
    }
    w.start_tool();
    w.fields.insert(TOOL.into(), "true".into());
    if !w.fields.contains_key(OWNER) {
        let owner = default_owner(w);
        if let Some(definition) = owner.strip_prefix("definition:") {
            let configuration = w
                .session
                .visible()
                .active_configurations
                .get(definition)
                .cloned()
                .unwrap_or_default();
            w.fields.insert(CONFIGURATION.into(), configuration);
        }
        w.fields.insert(OWNER.into(), owner);
    }
    w.open_editor("properties");
}
pub fn cancel(w: &mut Workspace) {
    w.fields.retain(|key, _| {
        !key.starts_with("properties-") && !key.starts_with("presentation-field:model-properties:")
    });
}
pub fn current_source_is_valid(w: &Workspace) -> bool {
    !w.fields.keys().any(|key| key.starts_with(ERRORS))
        && (!w.fields.contains_key(TOOL)
            || model_properties::local(w.session.visible(), &scope(w)).is_ok())
}
pub fn field_is_invalid(w: &Workspace, element: &SemanticElement) -> bool {
    w.fields.contains_key(&format!("{ERRORS}{}", element.id))
}
pub fn field_error(w: &mut Workspace, element: &SemanticElement, error: Option<String>) {
    if !matches!(
        element.data.get("setter").and_then(Value::as_str),
        Some("properties-ui" | "model-property")
    ) {
        return;
    }
    let key = format!("{ERRORS}{}", element.id);
    if let Some(error) = error {
        w.fields.insert(key, error);
    } else {
        w.fields.remove(&key);
    }
}
fn dispatch(w: &mut Workspace, edit: Edit) -> Result<()> {
    let mut expected = w.session.visible().clone();
    model_properties::apply(&mut expected, edit.clone())?;
    w.dispatch(Command::EditModelProperties { edit });
    if w.session.visible() != &expected {
        return Err(CadError::Invalid(w.status.clone()));
    }
    Ok(())
}
fn reference_policy(w: &Workspace) -> ReferencePolicy {
    if w.fields
        .get(REFERENCES)
        .is_some_and(|value| value == "true")
    {
        ReferencePolicy::KeepBindings
    } else {
        ReferencePolicy::RejectUnresolved
    }
}
pub fn action(w: &mut Workspace, command: &str, arguments: Option<&Value>) -> bool {
    if command == "properties" {
        begin(w);
        return true;
    }
    if !command.starts_with("properties.") {
        return false;
    }
    if command == "properties.select" {
        if let Some(key) = arguments
            .and_then(|arguments| arguments.get("key"))
            .and_then(Value::as_str)
        {
            w.fields.insert(SELECTED.into(), key.into());
            w.fields.insert(RENAME.into(), key.into());
            w.fields.remove(REFERENCES);
        }
        return true;
    }
    if !current_source_is_valid(w) {
        w.status = "Correct the current property fields or choose a valid scope before changing property keys".into();
        return true;
    }
    if !editable(w) {
        w.status = "Make the linked component unique, or finish the active operation, before editing properties".into();
        return true;
    }
    if !w.fields.contains_key(TOOL) {
        begin(w);
        if !w.fields.contains_key(TOOL) {
            return true;
        }
    }
    let result =
        (|| -> Result<()> {
            let scope = scope(w);
            match command {
                "properties.add" => {
                    let key = w.fields.get(NEW_KEY).cloned().unwrap_or_default();
                    model_properties::validate_key(&key)?;
                    if model_properties::local(w.session.visible(), &scope)?.contains_key(&key) {
                        return Err(CadError::Invalid(
                            "This scope already has that key; edit its value instead".into(),
                        ));
                    }
                    let value = w.fields.get(NEW_VALUE).cloned().unwrap_or_default();
                    dispatch(
                        w,
                        Edit::Set {
                            scope,
                            key: key.clone(),
                            value,
                        },
                    )?;
                    clear_selection(w);
                    w.fields.insert(SELECTED.into(), key);
                    w.fields.remove(NEW_KEY);
                    w.fields.remove(NEW_VALUE);
                }
                "properties.remove" | "properties.inherit" => {
                    let key = arguments
                        .and_then(|arguments| arguments.get("key"))
                        .and_then(Value::as_str)
                        .or_else(|| w.fields.get(SELECTED).map(String::as_str))
                        .ok_or_else(|| CadError::Invalid("Select a local property value".into()))?
                        .to_owned();
                    let references = reference_policy(w);
                    dispatch(
                        w,
                        Edit::Remove {
                            scope,
                            key,
                            references,
                        },
                    )?;
                    clear_selection(w);
                }
                "properties.rename" => {
                    let key =
                        w.fields.get(SELECTED).cloned().ok_or_else(|| {
                            CadError::Invalid("Select a local property key".into())
                        })?;
                    let new_key = w.fields.get(RENAME).cloned().unwrap_or_default();
                    let references = reference_policy(w);
                    dispatch(
                        w,
                        Edit::Rename {
                            scope,
                            key,
                            new_key: new_key.clone(),
                            references,
                        },
                    )?;
                    w.fields.insert(SELECTED.into(), new_key);
                    w.fields.remove(REFERENCES);
                }
                "properties.copy-inherited" => {
                    let key =
                        w.fields.get(SELECTED).cloned().ok_or_else(|| {
                            CadError::Invalid("Select an inherited property".into())
                        })?;
                    let value = model_properties::effective(w.session.visible(), &scope)?
                        .get(&key)
                        .cloned()
                        .ok_or_else(|| {
                            CadError::Invalid("This property has no inherited value".into())
                        })?;
                    dispatch(w, Edit::Set { scope, key, value })?;
                }
                _ => return Err(CadError::Invalid("Unknown model property action".into())),
            }
            Ok(())
        })();
    if let Err(error) = result {
        w.status = error.to_string();
    }
    true
}
fn field(
    id: String,
    label: &str,
    setter: &str,
    field: &str,
    value: Value,
    editable: bool,
) -> SemanticElement {
    SemanticElement {
        id,
        role: "model-property-field".into(),
        label: label.into(),
        value: Some(
            value
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| value.to_string()),
        ),
        state: if editable { "draft" } else { "readonly" }.into(),
        commands: vec![],
        relationships: vec!["workspace.properties".into()],
        data: json!({"setter":setter,"field":field,"json":value,"editable":editable,"value_type":"string","classes":["modeling-field"],"accessibility_label":label}),
    }
}
fn owner_field(w: &Workspace) -> SemanticElement {
    let document = w.session.visible();
    let choices = std::iter::once(json!({"value":"document","label":format!("Document · {}",document.name)}))
        .chain(document.parts.values().map(|part|json!({"value":format!("definition:{}",part.id),"label":format!("Part · {}",part.name)})))
        .chain(document.assemblies.values().map(|assembly|json!({"value":format!("definition:{}",assembly.id),"label":format!("Assembly · {}",assembly.name)}))).collect::<Vec<_>>();
    let mut element = field(
        "model-properties:owner".into(),
        "Owner",
        "properties-ui",
        "owner",
        json!(
            w.fields
                .get(OWNER)
                .cloned()
                .unwrap_or_else(|| default_owner(w))
        ),
        available(w),
    );
    element.data["choices"] = json!(choices);
    element
}
fn configuration_field(w: &Workspace) -> Option<SemanticElement> {
    let definition = scope(w).definition()?.to_owned();
    let mut element = field(
        "model-properties:configuration".into(),
        "Values for",
        "properties-ui",
        "configuration",
        json!(scope(w).configuration().unwrap_or_default()),
        available(w),
    );
    element.data["choices"] = json!(
        std::iter::once(json!({"value":"","label":"Base definition"}))
            .chain(
                cad_core::configuration::names(w.session.visible(), &definition)
                    .into_iter()
                    .map(
                        |(id, name)| json!({"value":id,"label":format!("Configuration · {name}")})
                    )
            )
            .collect::<Vec<_>>()
    );
    Some(element)
}
fn selected_rows(w: &Workspace) -> Vec<model_properties::Row> {
    let common = w.fields.get(COMMON).is_none_or(|value| value == "true");
    let filter = w
        .fields
        .get(FILTER)
        .map(|value| value.to_lowercase())
        .unwrap_or_default();
    model_properties::rows(w.session.visible(), &scope(w), common)
        .unwrap_or_default()
        .into_iter()
        .filter(|row| {
            filter.is_empty()
                || format!("{} {} {}", row.key, row.label, row.value)
                    .to_lowercase()
                    .contains(&filter)
        })
        .collect()
}
fn row_field(w: &Workspace, row: &model_properties::Row) -> SemanticElement {
    let scope = scope(w);
    let source = serde_json::to_string(&scope).unwrap_or_default();
    let encoded = row
        .key
        .as_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let mut element = field(
        format!("model-properties:{source}:{encoded}"),
        &row.label,
        "model-property",
        &row.key,
        json!(row.value),
        editable(w),
    );
    element.data["scope"] = json!(scope);
    element.data["key"] = json!(row.key);
    element.data["origin"] = json!(row.origin);
    element.data["inherited"] = json!(row.inherited);
    element.data["local"] = json!(row.local_value.is_some());
    if row.value.contains('\n') || matches!(row.key.as_str(), "notes" | "finish_notes") {
        element.data["multiline"] = true.into();
    }
    if row.inherited {
        element.state = "inherited".into();
    }
    if !model_properties::editable(w.session.visible(), &scope) {
        element.data["diagnostics"] = json!([
            "This definition is a pinned library snapshot. Make a component unique before editing its properties."
        ]);
    }
    element
}
fn action_element(command: &str, label: &str, enabled: bool, reason: &str) -> SemanticElement {
    SemanticElement {
        id: format!("command:{command}"),
        role: "model-property-action".into(),
        label: label.into(),
        value: (!enabled).then(|| reason.into()),
        state: if enabled { "ready" } else { "disabled" }.into(),
        commands: vec![command.into()],
        relationships: vec!["workspace.properties".into()],
        data: json!({"enabled":enabled,"disabled_reason":reason,"accessibility_label":label}),
    }
}
pub fn semantics(w: &Workspace) -> Vec<SemanticElement> {
    let scope = scope(w);
    let mut elements = vec![
        SemanticElement {
            id: "workspace.properties".into(),
            role: "model-properties-editor".into(),
            label: "Model Properties".into(),
            value: None,
            state: if w.dialog.as_deref() == Some("properties") {
                "open"
            } else {
                "closed"
            }
            .into(),
            commands: vec!["properties".into()],
            relationships: scope.definition().map(str::to_owned).into_iter().collect(),
            data: json!({"renderer":"model-properties-editor","version":model_properties::CONTRACT_VERSION,"scope":scope,"transaction":"shared-document-draft","stable_property_keys":true}),
        },
        owner_field(w),
    ];
    elements.extend(configuration_field(w));
    elements.push(field(
        "model-properties:filter".into(),
        "Find property",
        "properties-ui",
        "filter",
        json!(w.fields.get(FILTER).cloned().unwrap_or_default()),
        true,
    ));
    let mut common = field(
        "model-properties:common".into(),
        "Show common fields",
        "properties-ui",
        "common",
        json!(w.fields.get(COMMON).is_none_or(|value| value == "true")),
        true,
    );
    common.data["value_type"] = "boolean".into();
    elements.push(common);
    for row in selected_rows(w) {
        let field = row_field(w, &row);
        elements.push(SemanticElement {id:format!("{}:select",field.id),role:"model-property-row".into(),label:format!("{} · {}",row.key, match &row.origin {model_properties::Origin::Unset=>"Unset".to_owned(),model_properties::Origin::Document=>"Document".to_owned(),model_properties::Origin::Base=>"Base".to_owned(),model_properties::Origin::Configuration{name,..}=>format!("Configuration · {name}")}),value:Some(row.value.clone()),state:if w.fields.get(SELECTED)==Some(&row.key){"selected"}else if row.inherited{"inherited"}else{"local"}.into(),commands:vec!["properties.select".into()],relationships:vec![field.id.clone()],data:json!({"key":row.key,"origin":row.origin,"local":row.local_value.is_some(),"inherited":row.inherited})});
        elements.push(field);
    }
    for (name, label, stored) in [
        ("new-key", "Custom key", NEW_KEY),
        ("new-value", "Value", NEW_VALUE),
        ("rename", "New key", RENAME),
    ] {
        elements.push(field(
            format!("model-properties:{name}"),
            label,
            "properties-ui",
            name,
            json!(w.fields.get(stored).cloned().unwrap_or_default()),
            editable(w),
        ));
    }
    let mut policy = field(
        "model-properties:keep-bindings".into(),
        "Keep existing bindings even if their values become blank",
        "properties-ui",
        "keep-bindings",
        json!(reference_policy(w) == ReferencePolicy::KeepBindings),
        editable(w),
    );
    policy.data["value_type"] = "boolean".into();
    elements.push(policy);
    let local = model_properties::local(w.session.visible(), &scope).unwrap_or_default();
    let selected = w.fields.get(SELECTED);
    let has_local = selected.is_some_and(|key| local.contains_key(key));
    let has_inherited = selected.is_some_and(|key| {
        !local.contains_key(key)
            && model_properties::effective(w.session.visible(), &scope)
                .is_ok_and(|values| values.contains_key(key))
    });
    let keys_editable = editable(w) && current_source_is_valid(w);
    for (command, label, enabled, reason) in [
        (
            "properties.add",
            "Add / override property",
            keys_editable
                && w.fields
                    .get(NEW_KEY)
                    .is_some_and(|key| model_properties::validate_key(key).is_ok()),
            "Enter a printable key and choose an editable scope",
        ),
        (
            "properties.remove",
            if scope.configuration().is_some() {
                "Remove override · inherit"
            } else {
                "Remove selected property"
            },
            keys_editable && has_local,
            "Select a local property; inherited values belong to their source scope",
        ),
        (
            "properties.rename",
            "Rename local key · keep existing bindings",
            keys_editable
                && has_local
                && w.fields.get(RENAME).is_some_and(|key| {
                    model_properties::validate_key(key).is_ok() && Some(key) != selected
                }),
            "Select a local key and enter a different valid key",
        ),
        (
            "properties.copy-inherited",
            "Copy inherited value into this configuration",
            keys_editable && has_inherited,
            "Select an inherited value",
        ),
    ] {
        elements.push(action_element(
            command,
            label,
            enabled,
            if current_source_is_valid(w) {
                reason
            } else {
                "Correct invalid property fields before changing property keys"
            },
        ));
    }
    elements
}
pub fn value(w: &Workspace, element: &SemanticElement) -> Option<Value> {
    let field = element.data.get("field")?.as_str()?;
    match element.data.get("setter")?.as_str()? {
        "model-property" => {
            let scope: Scope = serde_json::from_value(element.data.get("scope")?.clone()).ok()?;
            Some(json!(
                model_properties::effective(w.session.visible(), &scope)
                    .ok()?
                    .get(field)
                    .cloned()
                    .unwrap_or_default()
            ))
        }
        "properties-ui" => Some(match field {
            "owner" => json!(
                w.fields
                    .get(OWNER)
                    .cloned()
                    .unwrap_or_else(|| default_owner(w))
            ),
            "configuration" => json!(scope(w).configuration().unwrap_or_default()),
            "filter" => json!(w.fields.get(FILTER).cloned().unwrap_or_default()),
            "common" => json!(w.fields.get(COMMON).is_none_or(|value| value == "true")),
            "new-key" => json!(w.fields.get(NEW_KEY).cloned().unwrap_or_default()),
            "new-value" => json!(w.fields.get(NEW_VALUE).cloned().unwrap_or_default()),
            "rename" => json!(w.fields.get(RENAME).cloned().unwrap_or_default()),
            "keep-bindings" => json!(reference_policy(w) == ReferencePolicy::KeepBindings),
            _ => return None,
        }),
        _ => None,
    }
}
pub fn set_value(w: &mut Workspace, element: &SemanticElement, value: Value) -> Result<()> {
    let field = element
        .data
        .get("field")
        .and_then(Value::as_str)
        .ok_or_else(|| CadError::Invalid("The property field is missing".into()))?;
    if element.data.get("setter").and_then(Value::as_str) == Some("properties-ui")
        && matches!(field, "owner" | "configuration")
        && w.fields
            .keys()
            .any(|key| key.starts_with(ERRORS) && key != &format!("{ERRORS}{}", element.id))
    {
        return Err(CadError::Invalid(
            "Correct the current property fields before changing their source scope".into(),
        ));
    }
    match element.data.get("setter").and_then(Value::as_str) {
        Some("properties-ui") => match field {
            "owner" => {
                let owner = value
                    .as_str()
                    .ok_or_else(|| CadError::Invalid("Choose a document or model owner".into()))?;
                if owner != "document"
                    && owner.strip_prefix("definition:").is_none_or(|id| {
                        !w.session.visible().parts.contains_key(id)
                            && !w.session.visible().assemblies.contains_key(id)
                    })
                {
                    return Err(CadError::Invalid("The property owner is missing".into()));
                }
                w.fields.insert(OWNER.into(), owner.into());
                w.fields.insert(CONFIGURATION.into(), String::new());
                clear_selection(w);
            }
            "configuration" => {
                let configuration = value.as_str().ok_or_else(|| {
                    CadError::Invalid("Choose a named configuration or base".into())
                })?;
                if !configuration.is_empty()
                    && scope(w).definition().is_none_or(|definition| {
                        !cad_core::configuration::exists(
                            w.session.visible(),
                            definition,
                            configuration,
                        )
                    })
                {
                    return Err(CadError::Invalid("The configuration is missing".into()));
                }
                w.fields.insert(CONFIGURATION.into(), configuration.into());
                clear_selection(w);
            }
            "common" | "keep-bindings" => {
                let value = value
                    .as_bool()
                    .ok_or_else(|| CadError::Invalid("Choose enabled or disabled".into()))?;
                w.fields.insert(
                    if field == "common" {
                        COMMON
                    } else {
                        REFERENCES
                    }
                    .into(),
                    value.to_string(),
                );
            }
            "filter" | "new-key" | "new-value" | "rename" => {
                let value = value
                    .as_str()
                    .ok_or_else(|| CadError::Invalid("Enter property text".into()))?;
                let stored = match field {
                    "filter" => FILTER,
                    "new-key" => NEW_KEY,
                    "new-value" => NEW_VALUE,
                    _ => RENAME,
                };
                w.fields.insert(stored.into(), value.into());
            }
            _ => return Err(CadError::Invalid("Unknown property browser field".into())),
        },
        Some("model-property") => {
            if !available(w) {
                return Err(CadError::Invalid(
                    "Finish the active operation before editing properties".into(),
                ));
            }
            if !w.fields.contains_key(TOOL) {
                begin(w);
                if !w.fields.contains_key(TOOL) {
                    return Err(CadError::Invalid(w.status.clone()));
                }
            }
            let scope: Scope = serde_json::from_value(
                element
                    .data
                    .get("scope")
                    .cloned()
                    .ok_or_else(|| CadError::Invalid("The property scope is missing".into()))?,
            )?;
            let value = value
                .as_str()
                .ok_or_else(|| CadError::Invalid("Enter a property text value".into()))?
                .to_owned();
            dispatch(
                w,
                Edit::Set {
                    scope,
                    key: field.into(),
                    value,
                },
            )?;
            if w.fields.get(SELECTED).is_none_or(|key| key != field) {
                w.fields.remove(REFERENCES);
                w.fields.insert(RENAME.into(), field.into());
            }
            w.fields.insert(SELECTED.into(), field.into());
        }
        _ => {
            return Err(CadError::Invalid(
                "No property setter exists for this control".into(),
            ));
        }
    }
    Ok(())
}
fn render_action(ui: &mut egui::Ui, w: &mut Workspace, element: &SemanticElement) {
    let enabled = element
        .data
        .get("enabled")
        .and_then(Value::as_bool)
        .unwrap_or(element.state != "disabled");
    if controls::button(
        ui,
        &element.id,
        &element.role,
        &["semantic-control"],
        &element.label,
        enabled,
        false,
    )
    .on_hover_text(
        element
            .data
            .get("disabled_reason")
            .and_then(Value::as_str)
            .unwrap_or_default(),
    )
    .clicked()
    {
        action(w, &element.commands[0], Some(&element.data));
    }
}
fn impact_data(w: &mut Workspace, key: &str) -> Vec<model_properties::Impact> {
    let scope = scope(w);
    let draft = w.session.draft.as_ref();
    let identity = format!(
        "{}:{}:{}:{}:{key}",
        w.session.generation,
        w.session.committed.revision,
        draft
            .map(|draft| format!("{}:{}", draft.id, draft.edit))
            .unwrap_or_default(),
        serde_json::to_string(&scope).unwrap_or_default()
    );
    if w.fields.get("properties-impact-identity") != Some(&identity) {
        let impact = model_properties::impacts(w.session.visible(), &scope, key);
        w.fields
            .insert("properties-impact-identity".into(), identity);
        w.fields.insert(
            "properties-impact-data".into(),
            serde_json::to_string(&impact).unwrap_or_default(),
        );
        return impact;
    }
    w.fields
        .get("properties-impact-data")
        .and_then(|data| serde_json::from_str(data).ok())
        .unwrap_or_default()
}
pub fn show(ui: &mut egui::Ui, w: &mut Workspace) {
    if !available(w) {
        controls::label(
            ui,
            "model-properties:busy",
            "model-property-help",
            &[],
            "Finish the active operation before editing properties",
            "disabled",
        );
        return;
    }
    if !w.fields.contains_key(TOOL) {
        begin(w);
        if !w.fields.contains_key(TOOL) {
            return;
        }
    }
    cad_ui::confirmation(ui, w);
    if let Err(error) = model_properties::local(w.session.visible(), &scope(w)) {
        controls::label(
            ui,
            "model-properties:scope-error",
            "model-property-error",
            &[],
            &error.to_string(),
            "invalid",
        );
    }
    let elements = semantics(w);
    for id in [
        "model-properties:owner",
        "model-properties:configuration",
        "model-properties:filter",
        "model-properties:common",
    ] {
        if let Some(element) = elements.iter().find(|element| element.id == id) {
            crate::ui::semantic_field(ui, w, element);
        }
    }
    if !model_properties::editable(w.session.visible(), &scope(w)) {
        controls::label(
            ui,
            "model-properties:pinned",
            "model-property-help",
            &[],
            "This definition is a pinned library snapshot. Make a component unique before editing its properties.",
            "readonly",
        );
    }
    let props = controls::properties("model-properties:rows", "model-property-list", &[], "ready");
    egui::ScrollArea::vertical()
        .id_salt("model-property-rows")
        .max_height(controls::number(&props, "max-height", 360.))
        .show(ui, |ui| {
            for row in selected_rows(w) {
                let element = row_field(w, &row);
                ui.horizontal(|ui| {
                    if controls::button(
                        ui,
                        &format!("{}:select", element.id),
                        "model-property-row",
                        &["semantic-control"],
                        &row.key,
                        true,
                        w.fields.get(SELECTED) == Some(&row.key),
                    )
                    .on_hover_text(
                        "Select this key to inspect linked uses, rename or remove its local value",
                    )
                    .clicked()
                    {
                        action(w, "properties.select", Some(&json!({"key":row.key})));
                    }
                    let origin = match &row.origin {
                        model_properties::Origin::Unset => "Unset".to_owned(),
                        model_properties::Origin::Document => "Document".to_owned(),
                        model_properties::Origin::Base => "Base".to_owned(),
                        model_properties::Origin::Configuration { name, .. } => name.clone(),
                    };
                    controls::label(
                        ui,
                        &format!("{}:origin", element.id),
                        "model-property-origin",
                        &[],
                        &format!(
                            "{}{}",
                            if row.inherited { "Inherited · " } else { "" },
                            origin
                        ),
                        if row.inherited { "inherited" } else { "local" },
                    );
                });
                crate::ui::semantic_field(ui, w, &element);
            }
        });
    controls::label(
        ui,
        "model-properties:add-heading",
        "model-property-heading",
        &[],
        "Custom property / local override",
        "ready",
    );
    for id in ["model-properties:new-key", "model-properties:new-value"] {
        if let Some(element) = elements.iter().find(|element| element.id == id) {
            crate::ui::semantic_field(ui, w, element);
        }
    }
    if let Some(element) = elements
        .iter()
        .find(|element| element.id == "command:properties.add")
    {
        render_action(ui, w, element);
    }
    if let Some(key) = w.fields.get(SELECTED).cloned() {
        let impacts = impact_data(w, &key);
        controls::label(
            ui,
            "model-properties:selected-heading",
            "model-property-heading",
            &[],
            &format!("{key} · {} linked uses", impacts.len()),
            "selected",
        );
        for impact in &impacts {
            controls::label(
                ui,
                &format!("model-properties:impact:{}", impact.id),
                "model-property-impact",
                &[],
                &impact.label,
                "associated",
            )
            .on_hover_text(&impact.detail);
        }
        if !impacts.is_empty() {
            controls::label(
                ui,
                "model-properties:binding-help",
                "model-property-help",
                &[],
                "Renaming retains the old drawing/BOM keys. Missing values display blank or use the listed fallback; value edits preserve every binding.",
                "associated",
            );
            if let Some(element) = elements
                .iter()
                .find(|element| element.id == "model-properties:keep-bindings")
            {
                crate::ui::semantic_field(ui, w, element);
            }
        }
        if let Some(element) = elements
            .iter()
            .find(|element| element.id == "model-properties:rename")
        {
            crate::ui::semantic_field(ui, w, element);
        }
        for command in [
            "properties.copy-inherited",
            "properties.remove",
            "properties.rename",
        ] {
            if let Some(element) = elements
                .iter()
                .find(|element| element.id == format!("command:{command}"))
            {
                render_action(ui, w, element);
            }
        }
    }
    for (id, error) in w.fields.iter().filter(|(key, _)| key.starts_with(ERRORS)) {
        controls::label(ui, id, "model-property-error", &[], error, "invalid");
    }
}
