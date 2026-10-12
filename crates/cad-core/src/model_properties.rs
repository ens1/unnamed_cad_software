//! Stable, text-valued manufacturing properties. Property keys are associations:
//! editing a value never changes a BOM column or drawing title-block binding.
use crate::{CadError, Document, Id, Result, drawing::LinkedText};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const CONTRACT_VERSION: u32 = 1;
pub const MAX_PROPERTIES: usize = 4096;
pub const MAX_KEY_BYTES: usize = 1024;
pub const MAX_VALUE_BYTES: usize = 65_536;
pub const MAX_MAP_BYTES: usize = 8 * 1024 * 1024;

/// Familiar names are conveniences, not a closed schema or renamed identities.
pub const COMMON_FIELDS: &[(&str, &str)] = &[
    ("part_number", "Part number"),
    ("description", "Description"),
    ("revision", "Revision"),
    ("drawing_number", "Drawing number"),
    ("designer", "Designer"),
    ("drawn_by", "Drawn by"),
    ("checked_by", "Checked by"),
    ("approved_by", "Approved by"),
    ("company", "Company"),
    ("project", "Project"),
    ("date", "Date"),
    ("stock_number", "Stock number"),
    ("manufacturer", "Manufacturer"),
    ("supplier", "Supplier"),
    ("finish_notes", "Finish notes"),
    ("notes", "Notes"),
];

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Scope {
    Document,
    Definition {
        definition: Id,
        configuration: Option<Id>,
    },
}
impl Scope {
    pub fn definition(&self) -> Option<&str> {
        match self {
            Self::Document => None,
            Self::Definition { definition, .. } => Some(definition),
        }
    }
    pub fn configuration(&self) -> Option<&str> {
        match self {
            Self::Definition { configuration, .. } => configuration.as_deref(),
            _ => None,
        }
    }
}
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReferencePolicy {
    #[default]
    RejectUnresolved,
    /// Existing bindings retain their exact key; the UI has disclosed the impact.
    KeepBindings,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Edit {
    Set {
        scope: Scope,
        key: String,
        value: String,
    },
    Remove {
        scope: Scope,
        key: String,
        references: ReferencePolicy,
    },
    Rename {
        scope: Scope,
        key: String,
        new_key: String,
        references: ReferencePolicy,
    },
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Origin {
    Unset,
    Document,
    Base,
    Configuration { configuration: Id, name: String },
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Row {
    pub key: String,
    pub label: String,
    pub value: String,
    pub local_value: Option<String>,
    pub origin: Origin,
    pub inherited: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Impact {
    pub id: String,
    pub label: String,
    pub consumer: String,
    pub detail: String,
    pub blank_if_missing: bool,
}
fn invalid(message: impl ToString) -> CadError {
    CadError::Invalid(format!("Model properties: {}", message.to_string()))
}
pub fn validate_key(key: &str) -> Result<()> {
    if key.trim().is_empty() || key.len() > MAX_KEY_BYTES || key.chars().any(char::is_control) {
        return Err(invalid(
            "A property key must be nonempty, printable and at most 1024 bytes",
        ));
    }
    Ok(())
}
pub fn validate_map(properties: &BTreeMap<String, String>) -> Result<()> {
    if properties.len() > MAX_PROPERTIES {
        return Err(invalid("A scope supports at most 4096 properties"));
    }
    let mut total = 0usize;
    for (key, value) in properties {
        validate_key(key)?;
        if value.len() > MAX_VALUE_BYTES
            || value
                .chars()
                .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
        {
            return Err(invalid(
                "Property values support at most 64 KiB of text without unsupported control characters",
            ));
        }
        total = total
            .checked_add(key.len() + value.len())
            .ok_or_else(|| invalid("Property map size overflow"))?;
        if total > MAX_MAP_BYTES {
            return Err(invalid(
                "A property scope exceeds the 8 MiB supported limit",
            ));
        }
    }
    Ok(())
}
pub fn validate(document: &Document) -> Result<()> {
    validate_map(&document.properties)?;
    for owner in document.parts.keys().chain(document.assemblies.keys()) {
        validate_map(base(document, owner)?)?;
        for configuration in crate::configuration::definitions(document, owner).values() {
            validate_map(&configuration.overrides.properties)?;
        }
    }
    Ok(())
}
pub fn base<'a>(document: &'a Document, definition: &str) -> Result<&'a BTreeMap<String, String>> {
    document
        .parts
        .get(definition)
        .map(|part| &part.properties)
        .or_else(|| {
            document
                .assemblies
                .get(definition)
                .map(|assembly| &assembly.properties)
        })
        .ok_or_else(|| invalid("The selected property owner no longer exists"))
}
pub fn local(document: &Document, scope: &Scope) -> Result<BTreeMap<String, String>> {
    match scope {
        Scope::Document => Ok(document.properties.clone()),
        Scope::Definition {
            definition,
            configuration: None,
        } => Ok(base(document, definition)?.clone()),
        Scope::Definition {
            definition,
            configuration: Some(configuration),
        } => {
            base(document, definition)?;
            crate::configuration::definitions(document, definition)
                .get(configuration)
                .map(|configuration| configuration.overrides.properties.clone())
                .ok_or_else(|| invalid("The named configuration no longer exists"))
        }
    }
}
pub fn effective(document: &Document, scope: &Scope) -> Result<BTreeMap<String, String>> {
    let mut values = match scope {
        Scope::Document => document.properties.clone(),
        Scope::Definition { definition, .. } => base(document, definition)?.clone(),
    };
    if let Scope::Definition {
        definition,
        configuration: Some(configuration),
    } = scope
    {
        values.extend(
            crate::configuration::resolve(document, definition, configuration)?
                .overrides
                .properties,
        );
    }
    Ok(values)
}
pub fn value(
    document: &Document,
    definition: &str,
    configuration: Option<&str>,
    key: &str,
) -> Result<String> {
    Ok(effective(
        document,
        &Scope::Definition {
            definition: definition.into(),
            configuration: configuration.map(str::to_owned),
        },
    )?
    .get(key)
    .cloned()
    .unwrap_or_default())
}
pub fn rows(document: &Document, scope: &Scope, include_common: bool) -> Result<Vec<Row>> {
    let values = effective(document, scope)?;
    let local = local(document, scope)?;
    let mut origins = values
        .keys()
        .map(|key| {
            (
                key.clone(),
                if matches!(scope, Scope::Document) {
                    Origin::Document
                } else {
                    Origin::Base
                },
            )
        })
        .collect::<BTreeMap<_, _>>();
    if let Scope::Definition {
        definition,
        configuration: Some(configuration),
    } = scope
    {
        let resolved = crate::configuration::resolve(document, definition, configuration)?;
        let definitions = crate::configuration::definitions(document, definition);
        for id in resolved.ancestors {
            let configuration = &definitions[&id];
            for key in configuration.overrides.properties.keys() {
                origins.insert(
                    key.clone(),
                    Origin::Configuration {
                        configuration: id.clone(),
                        name: configuration.name.clone(),
                    },
                );
            }
        }
    }
    let keys = values
        .keys()
        .cloned()
        .chain(if include_common {
            COMMON_FIELDS
                .iter()
                .map(|(key, _)| (*key).into())
                .collect::<Vec<String>>()
        } else {
            vec![]
        })
        .collect::<BTreeSet<_>>();
    Ok(keys
        .into_iter()
        .map(|key| Row {
            label: COMMON_FIELDS
                .iter()
                .find(|(candidate, _)| candidate == &key)
                .map(|(_, label)| (*label).into())
                .unwrap_or_else(|| key.clone()),
            value: values.get(&key).cloned().unwrap_or_default(),
            local_value: local.get(&key).cloned(),
            origin: origins.get(&key).cloned().unwrap_or(Origin::Unset),
            inherited: scope.configuration().is_some()
                && !local.contains_key(&key)
                && values.contains_key(&key),
            key,
        })
        .collect())
}
pub fn editable(document: &Document, scope: &Scope) -> bool {
    scope
        .definition()
        .is_none_or(|definition| !crate::library::is_definition_pinned(document, definition))
}
/// Actual source consumers only. External files are never scanned or rebound.
pub fn impacts(document: &Document, scope: &Scope, key: &str) -> Vec<Impact> {
    let mut out = vec![];
    for sheet in &document.drawings {
        if let Some(block) = sheet
            .format
            .as_ref()
            .and_then(|format| format.title_block.as_ref())
        {
            for cell in &block.cells {
                let matches = match (&cell.value, scope) {
                    (LinkedText::DocumentProperty { key: reference }, Scope::Document) => {
                        reference == key
                    }
                    (
                        LinkedText::ModelProperty { key: reference },
                        Scope::Definition { definition, .. },
                    ) => reference == key && definition == &sheet.definition,
                    _ => false,
                };
                if matches {
                    out.push(Impact { id: format!("{}:{}", sheet.id, cell.id), label: format!("{} · {}", sheet.name, cell.label), consumer: "drawing-title-block".into(), detail: "Keeps this exact property key and resolves its current source value; a missing value displays blank".into(), blank_if_missing: true });
                }
            }
        }
        if let Scope::Definition { definition, .. } = scope {
            let mut source = None;
            for table in &sheet.tables {
                if !matches!(
                    table.kind,
                    crate::drawing_tables::TableKind::BillOfMaterials
                ) {
                    continue;
                }
                if !table.bom_options.columns.iter().any(|column| match column {
                    crate::bom::Column::Property { key: reference, .. } => reference == key,
                    crate::bom::Column::PartNumber => key == "part_number",
                    crate::bom::Column::Description => key == "description",
                    _ => false,
                }) {
                    continue;
                }
                let source = source.get_or_insert_with(|| {
                    let mut source = document.clone();
                    source.root = sheet.definition.clone();
                    source
                });
                let uses_definition = crate::bom::rows(source, &table.bom_options)
                    .is_ok_and(|rows| rows.iter().any(|row| &row.definition == definition));
                if !uses_definition {
                    continue;
                }
                for (index, column) in table.bom_options.columns.iter().enumerate() {
                    let blank_if_missing = match column {
                        crate::bom::Column::Property { key: reference, .. } if reference == key => {
                            Some(true)
                        }
                        crate::bom::Column::PartNumber if key == "part_number" => Some(false),
                        crate::bom::Column::Description if key == "description" => Some(false),
                        _ => None,
                    };
                    if let Some(blank_if_missing) = blank_if_missing {
                        out.push(Impact { id: format!("{}:{}:{index}", sheet.id, table.id), label: format!("{} · {} · {}", sheet.name, table.title, column.label()), consumer: "drawing-bom-column".into(), detail: if blank_if_missing { "The column retains its exact key; missing row values display blank" } else { "Missing values fall back to the authored model name or configured BOM naming" }.into(), blank_if_missing });
                    }
                }
            }
        }
    }
    out
}
fn store(document: &mut Document, scope: &Scope, values: BTreeMap<String, String>) -> Result<()> {
    validate_map(&values)?;
    match scope {
        Scope::Document => document.properties = values,
        Scope::Definition {
            definition,
            configuration: None,
        } => {
            if let Some(part) = document.parts.get_mut(definition) {
                part.properties = values;
            } else if let Some(assembly) = document.assemblies.get_mut(definition) {
                assembly.properties = values;
            } else {
                return Err(invalid("The property owner no longer exists"));
            }
        }
        Scope::Definition {
            definition,
            configuration: Some(configuration),
        } => {
            let mut selected = crate::configuration::definitions(document, definition)
                .get(configuration)
                .cloned()
                .ok_or_else(|| invalid("The named configuration no longer exists"))?;
            selected.overrides.properties = values;
            crate::configuration::put(document, definition, selected)?;
        }
    }
    Ok(())
}
pub fn apply(document: &mut Document, edit: Edit) -> Result<()> {
    let (scope, key) = match &edit {
        Edit::Set { scope, key, .. }
        | Edit::Remove { scope, key, .. }
        | Edit::Rename { scope, key, .. } => (scope, key),
    };
    validate_key(key)?;
    if !editable(document, scope) {
        return Err(invalid(
            "Make the linked component unique before editing its manufacturing properties",
        ));
    }
    let mut values = local(document, scope)?;
    let (references, disappearing) = match &edit {
        Edit::Set { value, .. } => {
            values.insert(key.clone(), value.clone());
            (ReferencePolicy::RejectUnresolved, false)
        }
        Edit::Remove { references, .. } => {
            if values.remove(key).is_none() {
                return Err(invalid(
                    "This scope has no local value to remove; inherited values belong to their source scope",
                ));
            }
            (*references, true)
        }
        Edit::Rename {
            new_key,
            references,
            ..
        } => {
            validate_key(new_key)?;
            if new_key == key {
                return Ok(());
            }
            if effective(document, scope)?.contains_key(new_key) {
                return Err(invalid(
                    "The new key already exists or is inherited; no existing value was overwritten",
                ));
            }
            let value = values.remove(key).ok_or_else(|| {
                invalid("Only a local property key can be renamed; copy the inherited value first")
            })?;
            values.insert(new_key.clone(), value);
            (*references, true)
        }
    };
    let mut candidate = document.clone();
    store(&mut candidate, scope, values)?;
    if disappearing
        && references == ReferencePolicy::RejectUnresolved
        && !effective(&candidate, scope)?.contains_key(key)
    {
        let unresolved = impacts(document, scope, key)
            .into_iter()
            .filter(|impact| impact.blank_if_missing)
            .collect::<Vec<_>>();
        if !unresolved.is_empty() {
            return Err(invalid(format!(
                "This key supplies {} linked drawing/BOM fields. Keep the key, or explicitly keep their existing bindings after reviewing the impact",
                unresolved.len()
            )));
        }
    }
    candidate.validate()?;
    *document = candidate;
    Ok(())
}
