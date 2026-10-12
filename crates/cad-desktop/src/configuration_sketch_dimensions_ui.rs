//! Configuration controls for stable driving sketch dimensions.
use super::{Workspace, owner, source, submit};
use bevy_egui::egui;
use cad_core::{
    configuration::{self, Definition, Resolved, sketch_dimensions::DimensionField},
    presentation::SemanticElement,
};
use serde_json::{Value, json};

fn identity(owner: &str, configuration: &str, dimension: &DimensionField, field: &str) -> String {
    format!(
        "configuration-sketch-dimension:{owner}/{configuration}/{}/{}:{field}",
        dimension.sketch, dimension.constraint
    )
}
pub(super) fn cell(
    ui: &mut egui::Ui,
    owner: &str,
    configuration: &mut Definition,
    dimension: &DimensionField,
    resolved: Option<&Resolved>,
) -> bool {
    let inherited = resolved
        .and_then(|resolved| resolved.overrides.sketch_dimensions.get(&dimension.sketch))
        .and_then(|dimensions| dimensions.get(&dimension.constraint))
        .unwrap_or(&dimension.expression);
    let mut explicit = configuration
        .overrides
        .sketch_dimensions
        .get(&dimension.sketch)
        .is_some_and(|dimensions| dimensions.contains_key(&dimension.constraint));
    let mut changed = false;
    ui.horizontal(|ui| {
        if ui
            .checkbox(&mut explicit, "")
            .on_hover_text("Override this driving sketch dimension in this configuration")
            .changed()
        {
            changed = true;
            if explicit {
                configuration
                    .overrides
                    .sketch_dimensions
                    .entry(dimension.sketch.clone())
                    .or_default()
                    .insert(dimension.constraint.clone(), inherited.clone());
            } else if let Some(dimensions) = configuration
                .overrides
                .sketch_dimensions
                .get_mut(&dimension.sketch)
            {
                dimensions.remove(&dimension.constraint);
                if dimensions.is_empty() {
                    configuration
                        .overrides
                        .sketch_dimensions
                        .remove(&dimension.sketch);
                }
            }
        }
        if let Some(expression) = configuration
            .overrides
            .sketch_dimensions
            .get_mut(&dimension.sketch)
            .and_then(|dimensions| dimensions.get_mut(&dimension.constraint))
        {
            changed |= crate::presentation_controls::expression(
                ui,
                &identity(owner, &configuration.id, dimension, "expression"),
                &dimension.label,
                expression,
            );
        } else {
            crate::presentation_controls::label(
                ui,
                &identity(owner, &configuration.id, dimension, "expression"),
                "configuration-sketch-dimension-field",
                &["inherited"],
                &format!("{}: {}", dimension.label, inherited.0),
                "inherited",
            );
        }
    });
    if changed {
        configuration.version = configuration::CONTRACT_VERSION;
    }
    changed
}
fn local(w: &Workspace, owner: &str, id: &str) -> Option<Definition> {
    source(w)
        .ok()
        .filter(|configuration| configuration.id == id && super::owner(w).as_deref() == Some(owner))
        .or_else(|| configuration::definitions(w.session.visible(), owner).remove(id))
}
pub(super) fn semantics(w: &Workspace) -> Vec<SemanticElement> {
    let Some(owner) = owner(w) else {
        return vec![];
    };
    let Some(part) = w.session.visible().parts.get(&owner) else {
        return vec![];
    };
    let dimensions = configuration::sketch_dimensions::fields(part);
    let mut out = Vec::new();
    for configuration in configuration::definitions(w.session.visible(), &owner).into_values() {
        let Some(configuration) = local(w, &owner, &configuration.id) else {
            continue;
        };
        let resolved = configuration::resolve(w.session.visible(), &owner, &configuration.id).ok();
        for dimension in &dimensions {
            let explicit = configuration
                .overrides
                .sketch_dimensions
                .get(&dimension.sketch)
                .and_then(|dimensions| dimensions.get(&dimension.constraint));
            let expression = explicit
                .or_else(|| {
                    resolved
                        .as_ref()
                        .and_then(|resolved| {
                            resolved.overrides.sketch_dimensions.get(&dimension.sketch)
                        })
                        .and_then(|dimensions| dimensions.get(&dimension.constraint))
                })
                .unwrap_or(&dimension.expression);
            for (field, value, editable) in [
                ("override", json!(explicit.is_some()), true),
                ("expression", json!(expression.0), explicit.is_some()),
            ] {
                out.push(SemanticElement {
                    id: identity(&owner, &configuration.id, dimension, field), role: "configuration-sketch-dimension-field".into(),
                    label: format!("{} · {}{}", configuration.name, dimension.label, if field == "override" { " override" } else { "" }),
                    value: Some(if field == "override" { explicit.is_some().to_string() } else { expression.0.clone() }),
                    state: if explicit.is_some() { "overridden" } else { "inherited" }.into(), commands: vec![],
                    relationships: vec![owner.clone(), format!("{owner}@{}", configuration.id), dimension.sketch.clone(), dimension.constraint.clone()],
                    data: json!({"contract":"tcad.configuration.sketch-dimension-field","version":configuration::sketch_dimensions::CONTRACT_VERSION,
                        "setter":"configuration-dimension","owner":owner,"configuration":configuration.id,"sketch":dimension.sketch,"constraint":dimension.constraint,"field":field,"json":value,"value_type":if field == "override" {"boolean"} else {"expression"},"editable":editable,"unit":if dimension.angular {"deg"}else{"mm"},"driving":true,"source":dimension}),
                });
            }
        }
    }
    out
}
fn parts(element: &SemanticElement) -> Option<(&str, &str, &str, &str, &str)> {
    Some((
        element.data["owner"].as_str()?,
        element.data["configuration"].as_str()?,
        element.data["sketch"].as_str()?,
        element.data["constraint"].as_str()?,
        element.data["field"].as_str()?,
    ))
}
pub(super) fn current_value(w: &Workspace, element: &SemanticElement) -> Option<Value> {
    let (owner, id, sketch, constraint, field) = parts(element)?;
    let configuration = local(w, owner, id)?;
    let explicit = configuration
        .overrides
        .sketch_dimensions
        .get(sketch)
        .and_then(|dimensions| dimensions.get(constraint));
    if field == "override" {
        return Some(json!(explicit.is_some()));
    }
    let base = configuration::sketch_dimensions::fields(w.session.visible().parts.get(owner)?)
        .into_iter()
        .find(|dimension| dimension.sketch == sketch && dimension.constraint == constraint)?;
    let resolved = configuration::resolve(w.session.visible(), owner, id).ok();
    Some(json!(
        explicit
            .or_else(|| resolved
                .as_ref()
                .and_then(|resolved| resolved.overrides.sketch_dimensions.get(sketch))
                .and_then(|dimensions| dimensions.get(constraint)))
            .unwrap_or(&base.expression)
            .0
    ))
}
pub(super) fn set_value(
    w: &mut Workspace,
    element: &SemanticElement,
    value: Value,
) -> cad_core::Result<()> {
    let invalid = |message: &str| cad_core::CadError::Invalid(message.into());
    let (owner, id, sketch, constraint, field) =
        parts(element).ok_or_else(|| invalid("Missing sketch dimension configuration identity"))?;
    if w.dialog.as_deref() != Some("configurations")
        || super::owner(w).as_deref() != Some(owner)
        || w.tool_checkpoint.is_none()
    {
        return Err(invalid(
            "Open this definition's Configurations editor before editing sketch dimensions",
        ));
    }
    let base = configuration::sketch_dimensions::fields(
        w.session
            .visible()
            .parts
            .get(owner)
            .ok_or_else(|| invalid("Sketch dimension part is missing"))?,
    )
    .into_iter()
    .find(|dimension| dimension.sketch == sketch && dimension.constraint == constraint)
    .ok_or_else(|| invalid("This driving sketch dimension no longer exists"))?;
    if w.fields.get("configuration-editor").map(String::as_str) != Some(id)
        && !super::current_source_is_valid(w)
    {
        return Err(invalid(
            "Correct the current configuration before editing another row",
        ));
    }
    let mut configuration =
        local(w, owner, id).ok_or_else(|| invalid("The configuration no longer exists"))?;
    match field {
        "override" => {
            if value
                .as_bool()
                .ok_or_else(|| invalid("Dimension override requires a boolean"))?
            {
                let resolved = configuration::resolve(w.session.visible(), owner, id)?;
                let expression = resolved
                    .overrides
                    .sketch_dimensions
                    .get(sketch)
                    .and_then(|dimensions| dimensions.get(constraint))
                    .unwrap_or(&base.expression)
                    .clone();
                configuration
                    .overrides
                    .sketch_dimensions
                    .entry(sketch.into())
                    .or_default()
                    .entry(constraint.into())
                    .or_insert(expression);
            } else if let Some(dimensions) =
                configuration.overrides.sketch_dimensions.get_mut(sketch)
            {
                dimensions.remove(constraint);
                if dimensions.is_empty() {
                    configuration.overrides.sketch_dimensions.remove(sketch);
                }
            }
        }
        "expression" => {
            let expression = value
                .as_str()
                .ok_or_else(|| invalid("A driving dimension requires expression text"))?;
            configuration
                .overrides
                .sketch_dimensions
                .get_mut(sketch)
                .and_then(|dimensions| dimensions.get_mut(constraint))
                .ok_or_else(|| invalid("Enable the dimension override before editing it"))?
                .0 = expression.into();
        }
        _ => return Err(invalid("Unknown sketch dimension configuration field")),
    }
    configuration.version = configuration::CONTRACT_VERSION;
    w.fields
        .insert("configuration-editor".into(), configuration.id.clone());
    w.json = serde_json::to_string_pretty(&configuration)?;
    submit(w, owner, configuration.clone());
    if configuration::definitions(w.session.visible(), owner).get(id) != Some(&configuration) {
        return Err(invalid(&w.status));
    }
    Ok(())
}
