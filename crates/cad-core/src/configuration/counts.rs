//! Scalar configured counts are evaluated after every selected parameter scope.
//! Native operation integers remain authoritative; this module never evaluates
//! geometry or recursively materializes a configuration variant.
use super::{Definition, Overrides, dimensions};
use crate::{CadError, Document, Expr, Id, Result};
use std::collections::BTreeMap;

pub(crate) fn apply_parameters(
    doc: &mut Document,
    owner: &str,
    values: &BTreeMap<Id, Expr>,
) -> Result<()> {
    let parameters = if let Some(part) = doc.parts.get_mut(owner) {
        &mut part.parameters
    } else if let Some(assembly) = doc.assemblies.get_mut(owner) {
        &mut assembly.parameters
    } else {
        return Err(CadError::Invalid(
            "Configuration parameter owner is missing".into(),
        ));
    };
    for (id, expression) in values {
        parameters
            .get_mut(id)
            .ok_or_else(|| CadError::Invalid("Configured parameter is missing".into()))?
            .expression = expression.clone();
    }
    Ok(())
}

/// Parameter-only context for a named variant. Sibling active scopes are staged
/// first and the target's explicit inherited overrides replace its base values.
pub fn parameter_context(doc: &Document, owner: &str, overrides: &Overrides) -> Result<Document> {
    let mut context = doc.clone();
    prepare_parameters(&mut context, owner, overrides)?;
    Ok(context)
}

fn prepare_parameters(context: &mut Document, owner: &str, overrides: &Overrides) -> Result<()> {
    let mut selected = Vec::new();
    for (definition, configuration) in &context.active_configurations {
        if definition != owner {
            let resolved = super::resolve(context, definition, configuration)?;
            selected.push((definition.clone(), resolved.overrides.parameters));
        }
    }
    for (definition, parameters) in selected {
        apply_parameters(context, &definition, &parameters)?;
    }
    apply_parameters(context, owner, &overrides.parameters)
}

/// Resolve an editor's current source without validating or publishing the draft.
pub fn preview_context(
    doc: &Document,
    owner: &str,
    configuration: &Definition,
) -> Result<Document> {
    let mut candidate = doc.clone();
    super::put(&mut candidate, owner, configuration.clone())?;
    let resolved = super::resolve(&candidate, owner, &configuration.id)?;
    prepare_parameters(&mut candidate, owner, &resolved.overrides)?;
    Ok(candidate)
}

/// A count's displayed source is inherited from the current editor definition,
/// including a newly chosen parent which has not yet been published.
pub fn preview_count(
    doc: &Document,
    owner: &str,
    configuration: &Definition,
    field: &super::DimensionField,
) -> Result<u64> {
    let context = preview_context(doc, owner, configuration)?;
    let resolved = super::resolve(&context, owner, &configuration.id)?;
    let expression = resolved
        .overrides
        .feature_dimensions
        .get(&field.feature)
        .and_then(|fields| fields.get(&field.path))
        .unwrap_or(&field.expression);
    dimensions::count_value(&context, owner, field, expression)
}

pub(crate) fn expressions(doc: &Document, owner: &str, overrides: &Overrides) -> Result<Vec<Expr>> {
    let Some(part) = doc.parts.get(owner) else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for feature in &part.features {
        if let Some(fields) = overrides.feature_dimensions.get(&feature.id) {
            for field in dimensions::fields(feature)? {
                if field.kind == dimensions::FieldKind::Count
                    && let Some(expression) = fields.get(&field.path)
                {
                    out.push(expression.clone());
                }
            }
        }
    }
    Ok(out)
}

pub(crate) fn values(
    context: &Document,
    owner: &str,
    overrides: &Overrides,
) -> Result<BTreeMap<Id, BTreeMap<String, u64>>> {
    let Some(part) = context.parts.get(owner) else {
        return Ok(BTreeMap::new());
    };
    let mut out = BTreeMap::new();
    for feature in &part.features {
        if let Some(fields) = overrides.feature_dimensions.get(&feature.id) {
            for field in dimensions::fields(feature)? {
                if field.kind == dimensions::FieldKind::Count
                    && let Some(expression) = fields.get(&field.path)
                {
                    let value = dimensions::count_value(context, owner, &field, expression)
                        .map_err(|error| {
                            CadError::Invalid(format!("{} {}: {error}", feature.name, field.path))
                        })?;
                    out.entry(feature.id.clone())
                        .or_insert_with(BTreeMap::new)
                        .insert(field.path, value);
                }
            }
        }
    }
    Ok(out)
}

pub(crate) fn validate(doc: &Document, owner: &str, overrides: &Overrides) -> Result<()> {
    if expressions(doc, owner, overrides)?.is_empty() {
        return Ok(());
    }
    let context = parameter_context(doc, owner, overrides)?;
    values(&context, owner, overrides)?;
    Ok(())
}

pub(crate) fn apply(doc: &mut Document, owner: &str, overrides: &Overrides) -> Result<()> {
    let values = values(doc, owner, overrides)?;
    let Some(part) = doc.parts.get_mut(owner) else {
        return Ok(());
    };
    for feature in &mut part.features {
        if let Some(fields) = values.get(&feature.id) {
            let mut operation = dimensions::operation_source(&feature.operation)?;
            for (path, value) in fields {
                let (pointer, _) = dimensions::resolve_pointer(&operation, path)?;
                *operation.pointer_mut(&pointer).ok_or_else(|| {
                    CadError::Invalid("Configured count field is missing".into())
                })? = (*value).into();
            }
            feature.operation = serde_json::from_value(operation)?;
        }
    }
    Ok(())
}
