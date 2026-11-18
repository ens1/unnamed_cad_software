//! Create a named parameter in the active model draft and return to its editor.
use crate::{Workspace, presentation_controls as controls};
use bevy_egui::egui;
use cad_core::{
    CadError, Dimension, Document, Expr, Parameter, Result, commands::Command, expression,
    presentation::SemanticElement,
};
use serde_json::{Value, json};
use std::collections::BTreeMap;

const PREFIX: &str = "new-parameter:";
const ADD: &str = "parameter.add-to-draft";
const CANCEL: &str = "parameter.cancel-new";

pub fn active(w: &Workspace) -> bool {
    w.dialog.as_deref() == Some("parameter")
}

fn owner(w: &Workspace) -> &str {
    w.fields
        .get("new-parameter:owner")
        .map(String::as_str)
        .unwrap_or_default()
}
pub(crate) fn parameters<'a>(
    doc: &'a Document,
    owner: &str,
) -> Option<&'a BTreeMap<String, Parameter>> {
    doc.parts
        .get(owner)
        .map(|part| &part.parameters)
        .or_else(|| {
            doc.assemblies
                .get(owner)
                .map(|assembly| &assembly.parameters)
        })
}
fn default_value(w: &Workspace, kind: &str) -> String {
    match kind {
        "angle" => "90 deg".into(),
        "scalar" => "1".into(),
        _ => {
            let unit = w.session.visible().settings.display_length_unit.symbol();
            format!("{} {unit}", if unit == "mm" { 10 } else { 1 })
        }
    }
}
pub fn begin(w: &mut Workspace) {
    if active(w) {
        w.manager_tree = false;
        return;
    }
    let doc = w.session.visible();
    let definition = w
        .view
        .active_part
        .as_deref()
        .unwrap_or(&doc.root)
        .to_owned();
    if parameters(doc, &definition).is_none() {
        w.status = "Choose a part or assembly for the parameter.".into();
        return;
    }
    let value = default_value(w, "length");
    let document = doc.id.clone();
    w.fields.retain(|key, _| !key.starts_with(PREFIX));
    for (field, value) in [
        ("owner", definition),
        ("document", document),
        ("editor", cad_core::id()),
        ("return", w.dialog.clone().unwrap_or_default()),
        ("return-tree", w.manager_tree.to_string()),
        ("name", String::new()),
        ("kind", "length".into()),
        ("value", value),
    ] {
        w.fields.insert(format!("{PREFIX}{field}"), value);
    }
    w.fields
        .insert("focus-request:new-parameter:name".into(), "true".into());
    w.open_editor("parameter");
}
pub fn cancel(w: &mut Workspace) -> bool {
    if !active(w) {
        return false;
    }
    w.dialog = w
        .fields
        .remove("new-parameter:return")
        .filter(|previous| !previous.is_empty());
    w.manager_tree = w
        .fields
        .get("new-parameter:return-tree")
        .is_none_or(|value| value == "true");
    w.fields.retain(|key, _| !key.starts_with(PREFIX));
    w.fields.remove("focus-request:new-parameter:name");
    true
}
fn name(w: &Workspace) -> std::result::Result<&str, String> {
    let name = w
        .fields
        .get("new-parameter:name")
        .map(|name| name.trim())
        .unwrap_or("");
    if name.is_empty() {
        return Err("Enter a name, such as wall_thickness.".into());
    }
    if !name.starts_with(|c: char| c.is_alphabetic() || c == '_')
        || !name.chars().all(|c| c.is_alphanumeric() || c == '_')
    {
        return Err(
            "Start with a letter or underscore. Use letters, numbers and underscores.".into(),
        );
    }
    if expression::references(name).is_ok_and(|references| references.is_empty()) {
        return Err(format!("“{name}” is a unit. Choose a different name."));
    }
    let doc = w.session.visible();
    let parameters =
        parameters(doc, owner(w)).ok_or("The part or assembly is no longer available.")?;
    if parameters.contains_key(name)
        || parameters.values().any(|p| p.name == name)
        || doc
            .assemblies
            .get(owner(w))
            .is_some_and(|assembly| assembly.promoted_parameters.contains_key(name))
    {
        return Err(format!(
            "“{name}” already exists in this model. Choose a different name."
        ));
    }
    Ok(name)
}
fn expression_value(w: &Workspace) -> std::result::Result<(Expr, Dimension, String), String> {
    let doc = w.session.visible();
    let dimension = match w.fields.get("new-parameter:kind").map(String::as_str) {
        Some("length") => Dimension::Length,
        Some("angle") => Dimension::Angle,
        Some("scalar") => Dimension::Scalar,
        _ => return Err("Choose a parameter type.".into()),
    };
    let value = w
        .fields
        .get("new-parameter:value")
        .map(|v| v.trim())
        .unwrap_or("");
    if value.is_empty() {
        return Err("Enter a value or an expression.".into());
    }
    if let Ok(name) = name(w)
        && expression::references(value).is_ok_and(|references| {
            references
                .iter()
                .any(|reference| reference == name || reference == &format!("{}::{name}", owner(w)))
        })
    {
        return Err("A parameter cannot use itself in its expression.".into());
    }
    let mut source = Expr(value.into());
    let quantity = expression::quantity(doc, owner(w), &source).map_err(|error| match error {
        CadError::Invalid(message) => message,
        error => error.to_string(),
    })?;
    let result = if quantity.matches(dimension) {
        quantity.value
    } else if quantity.matches(Dimension::Scalar) {
        let (unit, scale) = match dimension {
            Dimension::Length => (
                doc.settings.display_length_unit.symbol(),
                doc.settings.display_length_unit.millimeters_per_unit(),
            ),
            Dimension::Angle => ("deg", std::f64::consts::PI / 180.),
            Dimension::Scalar => unreachable!(),
        };
        // A name must evaluate to its declared physical type wherever it is
        // referenced. Save implicit display units explicitly in its expression.
        source = Expr(if value.parse::<f64>().is_ok() {
            format!("{value} {unit}")
        } else {
            format!("({value}) * 1 {unit}")
        });
        quantity.value * scale
    } else {
        return Err(match dimension {
            Dimension::Length => "Enter a length, such as 10 mm or 0.5 in.",
            Dimension::Angle => "Enter an angle, such as 45 deg.",
            Dimension::Scalar => "Enter a unitless value or expression.",
        }
        .into());
    };
    if !result.is_finite() {
        return Err("Enter a finite value.".into());
    }
    let shown = match dimension {
        Dimension::Length => doc.settings.format_length(result),
        Dimension::Angle => doc.settings.format_angle_radians(result),
        Dimension::Scalar => result.to_string(),
    };
    Ok((source, dimension, shown))
}

fn live(w: &Workspace, data: &Value) -> bool {
    active(w)
        && data["editor"].as_str().is_some()
        && data["editor"].as_str() == w.fields.get("new-parameter:editor").map(String::as_str)
        && data["document"] == w.session.visible().id
        && data["owner"] == owner(w)
}

pub fn current_value(w: &Workspace, element: &SemanticElement) -> Option<Value> {
    if !live(w, &element.data) {
        return None;
    }
    let field = element.data["field"].as_str()?;
    w.fields
        .get(&format!("{PREFIX}{field}"))
        .map(|value| json!(value))
}
pub fn set_value(w: &mut Workspace, element: &SemanticElement, value: Value) -> Result<()> {
    if !live(w, &element.data) {
        return Err(CadError::Invalid(
            "Reopen New parameter to edit this field.".into(),
        ));
    }
    let field = element.data["field"].as_str().unwrap_or("");
    let text = value
        .as_str()
        .ok_or_else(|| CadError::Invalid("Enter text for this field.".into()))?;
    match field {
        "name" => {}
        "value" => {
            w.fields
                .insert("new-parameter:edited-value".into(), "true".into());
        }
        "kind" if matches!(text, "length" | "angle" | "scalar") => {
            if !w.fields.contains_key("new-parameter:edited-value") {
                let value = default_value(w, text);
                w.fields.insert("new-parameter:value".into(), value);
            }
        }
        _ => return Err(CadError::Invalid("Unknown parameter field or type.".into())),
    }
    w.fields.insert(format!("{PREFIX}{field}"), text.into());
    Ok(())
}

pub fn action(w: &mut Workspace, command: &str, data: Option<&Value>) -> bool {
    if !matches!(command, ADD | CANCEL) {
        return false;
    }
    if !active(w) || data.is_some_and(|data| !live(w, data)) {
        return true;
    }
    if command == CANCEL {
        return cancel(w);
    }
    let (name, expression, dimension) = match (name(w), expression_value(w)) {
        (Ok(name), Ok((expression, dimension, _))) => (name.to_owned(), expression, dimension),
        (Err(reason), _) | (_, Err(reason)) => {
            w.status = reason;
            return true;
        }
    };
    let definition = owner(w).to_owned();
    let key = cad_core::id();
    w.dispatch(Command::PutParameter {
        definition: definition.clone(),
        parameter: key.clone(),
        value: Parameter {
            name: name.clone(),
            expression,
            dimension,
        },
    });
    if parameters(w.session.visible(), &definition).is_some_and(|params| params.contains_key(&key))
    {
        cancel(w);
        w.status = format!("Added {name} to the draft.");
    }
    true
}

pub fn semantics(w: &Workspace) -> Vec<SemanticElement> {
    if !active(w) {
        return vec![];
    }
    let doc = w.session.visible();
    let mut items = Vec::new();
    let element = |id: &str, role: &str, label: &str, state: &str| SemanticElement {
        id: id.into(),
        role: role.into(),
        label: label.into(),
        value: None,
        state: state.into(),
        commands: vec![],
        relationships: vec![owner(w).into()],
        data: json!({"owner": owner(w), "document": doc.id, "editor": w.fields.get("new-parameter:editor"),"contract":"tcad.named-parameter","version":1}),
    };
    let owner_label = cad_core::presentation::definition_name(doc, owner(w));
    items.push(element(
        "new-parameter:title",
        "inspector-title",
        "New parameter",
        "ready",
    ));
    items.push(element(
        "new-parameter:owner",
        "inspector-context",
        &owner_label,
        "ready",
    ));
    let valid_name = name(w);
    let result = expression_value(w);
    for (field, label) in [
        ("name", "Name"),
        ("kind", "Type"),
        ("value", "Value or expression"),
    ] {
        let mut item = element(
            &format!("{PREFIX}{field}"),
            "named-parameter-field",
            label,
            "draft",
        );
        let value = w
            .fields
            .get(&format!("{PREFIX}{field}"))
            .cloned()
            .unwrap_or_default();
        item.value = Some(value.clone());
        item.data["setter"] = json!("named-parameter");
        item.data["field"] = json!(field);
        item.data["json"] = json!(value);
        item.data["editable"] = json!(true);
        if field == "name" {
            item.data["focus_request"] = json!("focus-request:new-parameter:name");
        }
        if field == "kind" {
            item.data["choices"] = json!([{"value":"length","label":"Length"},{"value":"angle","label":"Angle"},{"value":"scalar","label":"Unitless number"}]);
        }
        items.push(item);
        let message = match field {
            "name" => valid_name.as_ref().err().map(|error| {
                (
                    error.clone(),
                    if value.trim().is_empty() {
                        "ready"
                    } else {
                        "error"
                    },
                )
            }),
            "value" => Some(match &result {
                Ok((_, _, shown)) => (format!("Result · {shown}"), "ready"),
                Err(error) => (error.clone(), "error"),
            }),
            _ => None,
        };
        if let Some((message, state)) = message {
            items.push(element(
                &format!("{PREFIX}{field}:message"),
                if state == "error" {
                    "field-diagnostic"
                } else {
                    "field-hint"
                },
                &message,
                state,
            ));
        }
    }
    let reason = valid_name.err().or_else(|| result.err());
    for (command, label) in [(ADD, "Add to draft"), (CANCEL, "Cancel")] {
        let disabled = command == ADD && reason.is_some();
        let mut item = element(
            command,
            "parameter-action",
            label,
            if disabled { "disabled" } else { "ready" },
        );
        item.commands = vec![command.into()];
        item.data["actions"] = json!([{"command": command,"enabled":!disabled,"disabled_reason":if disabled {reason.as_deref()} else {None}}]);
        item.value = if disabled { reason.clone() } else { None };
        items.push(item);
    }
    items
}
pub fn show(ui: &mut egui::Ui, w: &mut Workspace) {
    let mut actions = Vec::new();
    for item in semantics(w) {
        if !item.commands.is_empty() {
            actions.push(item);
        } else if item.role == "named-parameter-field" {
            crate::ui::semantic_field(ui, w, &item);
        } else {
            controls::label(ui, &item.id, &item.role, &[], &item.label, &item.state);
        }
    }
    ui.horizontal_wrapped(|ui| {
        for item in actions {
            if controls::button(
                ui,
                &item.id,
                &item.role,
                &[],
                &item.label,
                item.state != "disabled",
                false,
            )
            .on_disabled_hover_text(item.value.as_deref().unwrap_or_default())
            .clicked()
            {
                action(w, &item.id, Some(&item.data));
            }
        }
    });
}
