//! Versioned manufacturing BOMs derived from configured component definitions.
//! Occurrence paths remain available after grouping; no renderer identities are used.
use crate::{CadError, Document, Id, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const CONTRACT_VERSION: u32 = 1;
pub const MAX_ROWS: usize = 100_000;
pub const MAX_DEPTH: usize = 256;

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    #[default]
    PartsOnly,
    TopLevel,
    Indented,
}
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Grouping {
    #[default]
    DefinitionConfiguration,
    DefinitionConfigurationFinish,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Column {
    Item,
    PartNumber,
    Description,
    Quantity,
    Material,
    Finish,
    Configuration,
    Definition,
    ComponentType,
    Depth,
    Manufacturing {
        field: crate::manufacturing_columns::Column,
    },
    Property {
        key: String,
        label: String,
    },
}
impl Column {
    pub fn label(&self) -> &str {
        match self {
            Self::Item => "Item",
            Self::PartNumber => "Part number",
            Self::Description => "Description",
            Self::Quantity => "Qty",
            Self::Material => "Material",
            Self::Finish => "Finish",
            Self::Configuration => "Configuration",
            Self::Definition => "Definition",
            Self::ComponentType => "Type",
            Self::Depth => "Level",
            Self::Manufacturing { field } => field.label(),
            Self::Property { label, .. } => label,
        }
    }
}
pub fn default_columns() -> Vec<Column> {
    vec![
        Column::Item,
        Column::PartNumber,
        Column::Description,
        Column::Quantity,
        Column::Material,
    ]
}
pub fn column_choices() -> Vec<Column> {
    let mut columns = vec![
        Column::Item,
        Column::PartNumber,
        Column::Description,
        Column::Quantity,
        Column::Material,
        Column::Finish,
        Column::Configuration,
        Column::Definition,
        Column::ComponentType,
        Column::Depth,
    ];
    columns.extend(
        crate::manufacturing_columns::Column::choices()
            .into_iter()
            .map(|field| Column::Manufacturing { field }),
    );
    columns.push(Column::Property {
        key: "part_number".into(),
        label: "Custom property".into(),
    });
    columns
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Options {
    #[serde(default)]
    pub mode: Mode,
    #[serde(default)]
    pub grouping: Grouping,
    #[serde(default = "default_columns")]
    pub columns: Vec<Column>,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            mode: Mode::default(),
            grouping: Grouping::default(),
            columns: default_columns(),
        }
    }
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    Part,
    Assembly,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Occurrence {
    /// Stable full component path relative to the requested document root.
    pub path: Vec<Id>,
    pub parent_path: Vec<Id>,
    pub definition: Id,
    pub configuration: Option<Id>,
    pub finish: Option<Id>,
    /// Definition-to-root placement. Used by associative manufacturing callouts.
    pub matrix: [[f64; 4]; 4],
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Row {
    pub key: String,
    pub definition: Id,
    pub configuration: Option<Id>,
    pub kind: SourceKind,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub configuration_name: String,
    /// The root's immediate children have level zero.
    pub depth: usize,
    pub parent_key: Option<String>,
    pub quantity: usize,
    pub occurrences: Vec<Occurrence>,
    pub part_number: String,
    pub description: String,
    pub material: String,
    pub material_id: Option<Id>,
    pub finish: String,
    pub finish_id: Option<Id>,
    pub properties: BTreeMap<String, String>,
    #[serde(
        default,
        skip_serializing_if = "crate::manufacturing_columns::Data::is_empty"
    )]
    pub manufacturing: crate::manufacturing_columns::Data,
}

fn invalid(message: impl ToString) -> CadError {
    CadError::Invalid(format!("BOM: {}", message.to_string()))
}
pub fn validate(options: &Options) -> Result<()> {
    if options.columns.is_empty() || options.columns.len() > 32 {
        return Err(invalid("Choose between one and 32 table columns"));
    }
    for column in &options.columns {
        if let Column::Property { key, label } = column
            && (key.trim().is_empty()
                || key.len() > 1024
                || label.trim().is_empty()
                || label.len() > 1024
                || key.chars().chain(label.chars()).any(char::is_control))
        {
            return Err(invalid(
                "Custom columns need a nonempty property key and readable label",
            ));
        }
    }
    Ok(())
}
pub fn item_key(definition: &str, configuration: Option<&str>) -> String {
    format!("{definition}@{}", configuration.unwrap_or_default())
}
fn row_key(
    options: &Options,
    definition: &str,
    configuration: Option<&str>,
    parent: Option<&str>,
    finish: Option<&str>,
) -> Result<String> {
    let base = item_key(definition, configuration);
    if options.mode == Mode::PartsOnly && options.grouping == Grouping::DefinitionConfiguration {
        return Ok(base);
    }
    let mut ancestors = Vec::new();
    if let Some(parent) = parent {
        let parent: serde_json::Value = serde_json::from_str(parent)
            .map_err(|_| invalid("Indented parent row identity is invalid"))?;
        ancestors.extend(
            parent
                .get("ancestors")
                .and_then(serde_json::Value::as_array)
                .cloned()
                .unwrap_or_default(),
        );
        ancestors.push(serde_json::json!({"definition":parent["definition"],"configuration":parent["configuration"],"finish":parent["finish"]}));
    }
    let key = serde_json::to_string(
        &serde_json::json!({"kind":"bom","mode":options.mode,"definition":definition,"configuration":configuration,"finish":if options.grouping == Grouping::DefinitionConfigurationFinish {finish}else{None},"ancestors":ancestors}),
    )?;
    if key.len() > 8192 {
        return Err(invalid(
            "Indented hierarchy identity exceeds the bounded saved row-key size",
        ));
    }
    Ok(key)
}

/// BOM definition traversal uses the same scoped configuration preparation as
/// placement evaluation, including generated pattern instances and exclusions.
pub fn rows(doc: &Document, options: &Options) -> Result<Vec<Row>> {
    rows_with_placements(doc, options, None)
}
pub fn rows_with_placements(
    doc: &Document,
    options: &Options,
    evaluated: Option<&BTreeMap<Id, crate::assembly_geometry::EvaluatedAssembly>>,
) -> Result<Vec<Row>> {
    validate(options)?;
    struct Walker<'a> {
        source: &'a Document,
        options: &'a Options,
        evaluated: Option<&'a BTreeMap<Id, crate::assembly_geometry::EvaluatedAssembly>>,
        active: BTreeSet<Id>,
        rows: BTreeMap<String, Row>,
        occurrence_count: usize,
    }
    struct Placement {
        path: Vec<Id>,
        matrix: nalgebra::Matrix4<f64>,
        finish: Option<Id>,
        parent: Option<String>,
        depth: usize,
        root: bool,
    }
    impl Walker<'_> {
        fn walk(
            &mut self,
            context: &Document,
            definition: &str,
            configuration: Option<Id>,
            placement: Placement,
        ) -> Result<()> {
            if placement.path.len() > MAX_DEPTH {
                return Err(invalid(
                    "Component hierarchy exceeds the bounded nesting depth",
                ));
            }
            self.occurrence_count = self
                .occurrence_count
                .checked_add(1)
                .ok_or_else(|| invalid("Occurrence count overflow"))?;
            if self.occurrence_count > MAX_ROWS {
                return Err(invalid(
                    "Component enumeration exceeds the bounded occurrence limit",
                ));
            }
            let scoped = crate::configuration::scope(
                self.source,
                context,
                definition,
                configuration.as_deref(),
            )?;
            crate::configuration::check_geometry_context(self.source, &scoped, definition)?;
            let configured = configuration
                .as_ref()
                .map(|c| crate::configuration::resolve(self.source, definition, c))
                .transpose()?;
            let bom = configured
                .as_ref()
                .map(|c| c.overrides.bom.clone())
                .unwrap_or_default();
            if bom.exclude.unwrap_or(false) {
                return Ok(());
            }
            let part = scoped.parts.get(definition);
            let assembly = scoped.assemblies.get(definition);
            let kind = if part.is_some() {
                SourceKind::Part
            } else if assembly.is_some() {
                SourceKind::Assembly
            } else {
                return Err(invalid("A component definition is missing"));
            };
            let name = part
                .map(|p| p.name.as_str())
                .or_else(|| assembly.map(|a| a.name.as_str()))
                .unwrap_or_default();
            let properties = part
                .map(|p| p.properties.clone())
                .or_else(|| assembly.map(|a| a.properties.clone()))
                .unwrap_or_default();
            let material_id = part.map(|p| p.material.clone());
            let finish_id = placement
                .finish
                .clone()
                .or_else(|| part.map(|p| p.finish.clone()));
            let manufacturing = if self
                .options
                .columns
                .iter()
                .any(|column| matches!(column, Column::Manufacturing { .. }))
            {
                crate::manufacturing_columns::Data::from_definitions(
                    &scoped,
                    crate::manufacturing_columns::project_material(&scoped, material_id.as_ref()),
                    crate::manufacturing_columns::project_finish(&scoped, finish_id.as_ref()),
                    part,
                )
            } else {
                Default::default()
            };
            let include = match self.options.mode {
                Mode::PartsOnly => kind == SourceKind::Part,
                Mode::TopLevel => !placement.root || kind == SourceKind::Part,
                Mode::Indented => !placement.root || kind == SourceKind::Part,
            };
            let mut parent = placement.parent.clone();
            if include {
                let key = row_key(
                    self.options,
                    definition,
                    configuration.as_deref(),
                    if self.options.mode == Mode::Indented {
                        parent.as_deref()
                    } else {
                        None
                    },
                    finish_id.as_deref(),
                )?;
                let number = match bom.name.unwrap_or_default() {
                    crate::configuration::BomName::Document => properties
                        .get("part_number")
                        .cloned()
                        .unwrap_or_else(|| name.into()),
                    crate::configuration::BomName::Configuration => configured
                        .as_ref()
                        .map(|c| c.name.clone())
                        .unwrap_or_else(|| name.into()),
                    crate::configuration::BomName::Custom => bom
                        .part_number
                        .filter(|s| !s.trim().is_empty())
                        .ok_or_else(|| invalid("Configured custom part number is required"))?,
                };
                let occurrence = Occurrence {
                    parent_path: placement.path[..placement.path.len().saturating_sub(1)].to_vec(),
                    path: placement.path.clone(),
                    definition: definition.into(),
                    configuration: configuration.clone(),
                    finish: finish_id.clone(),
                    matrix: std::array::from_fn(|r| {
                        std::array::from_fn(|c| placement.matrix[(r, c)])
                    }),
                };
                if self.rows.get(&key).is_some_and(|row| {
                    row.definition != definition
                        || row.configuration != configuration
                        || row.kind != kind
                }) {
                    return Err(invalid(
                        "Portable BOM identities collide; rename the conflicting definition/configuration IDs",
                    ));
                }
                let row = self.rows.entry(key.clone()).or_insert_with(|| Row {
                    key: key.clone(),
                    definition: definition.into(),
                    configuration: configuration.clone(),
                    kind,
                    configuration_name: configured
                        .as_ref()
                        .map(|configuration| configuration.name.clone())
                        .unwrap_or_default(),
                    depth: placement.depth,
                    parent_key: if self.options.mode == Mode::Indented {
                        parent.clone()
                    } else {
                        None
                    },
                    quantity: 0,
                    occurrences: vec![],
                    part_number: number,
                    description: bom.description.unwrap_or_else(|| {
                        properties
                            .get("description")
                            .cloned()
                            .unwrap_or_else(|| name.into())
                    }),
                    material: material_id
                        .as_ref()
                        .and_then(|id| scoped.materials.get(id))
                        .map(|m| m.name.clone())
                        .unwrap_or_else(|| material_id.clone().unwrap_or_default()),
                    material_id,
                    finish: finish_id
                        .as_ref()
                        .and_then(|id| scoped.finishes.get(id))
                        .map(|f| f.name.clone())
                        .unwrap_or_else(|| finish_id.clone().unwrap_or_default()),
                    finish_id: finish_id.clone(),
                    properties,
                    manufacturing: manufacturing.clone(),
                });
                row.quantity = row
                    .quantity
                    .checked_add(1)
                    .ok_or_else(|| invalid("Grouped quantity overflow"))?;
                if row.finish_id != finish_id {
                    row.finish_id = None;
                    row.finish = "Multiple finishes".into();
                }
                row.manufacturing.merge_finish(&manufacturing);
                row.occurrences.push(occurrence);
                parent = Some(key);
            }
            let Some(assembly) = assembly else {
                return Ok(());
            };
            if self.options.mode == Mode::TopLevel && !placement.root {
                return Ok(());
            }
            if !self.active.insert(definition.into()) {
                return Err(invalid(
                    "Assembly hierarchy or configuration contains a cycle",
                ));
            }
            let evaluated = self.evaluated.and_then(|values| {
                values.get(&crate::configuration::key(
                    definition,
                    configuration.as_deref(),
                ))
            });
            for instance in assembly.instances.values() {
                let mut pose = evaluated
                    .and_then(|value| value.placements.get(&instance.id))
                    .cloned()
                    .unwrap_or_else(|| instance.placement.clone());
                if evaluated.is_none()
                    && let Some(expressions) = &instance.translation_expressions
                {
                    for (axis, expression) in expressions.iter().enumerate() {
                        pose.translation[axis] = crate::expression::value(
                            &scoped,
                            definition,
                            expression,
                            crate::Dimension::Length,
                        )?;
                    }
                }
                let mut path = placement.path.clone();
                path.push(instance.id.clone());
                let configuration = crate::configuration::selected(
                    &scoped,
                    &instance.definition,
                    instance.configuration.as_ref(),
                )
                .cloned();
                self.walk(
                    &scoped,
                    &instance.definition,
                    configuration,
                    Placement {
                        path,
                        matrix: placement.matrix * pose.matrix(),
                        finish: instance.finish.clone().or_else(|| placement.finish.clone()),
                        parent: parent.clone(),
                        depth: if placement.root {
                            0
                        } else {
                            placement.depth + 1
                        },
                        root: false,
                    },
                )?;
            }
            self.active.remove(definition);
            Ok(())
        }
    }
    let context = crate::configuration::effective(doc)?;
    let mut walker = Walker {
        source: doc,
        options,
        evaluated,
        active: BTreeSet::new(),
        rows: BTreeMap::new(),
        occurrence_count: 0,
    };
    walker.walk(
        &context,
        &doc.root,
        doc.active_configurations.get(&doc.root).cloned(),
        Placement {
            path: vec![],
            matrix: nalgebra::Matrix4::identity(),
            finish: None,
            parent: None,
            depth: 0,
            root: true,
        },
    )?;
    if options.mode != Mode::Indented {
        return Ok(walker.rows.into_values().collect());
    }
    fn append(
        parent: Option<&str>,
        rows: &BTreeMap<String, Row>,
        children: &BTreeMap<Option<String>, Vec<String>>,
        output: &mut Vec<Row>,
    ) {
        if let Some(keys) = children.get(&parent.map(str::to_string)) {
            for key in keys {
                let row = &rows[key];
                output.push(row.clone());
                append(Some(&row.key), rows, children, output);
            }
        }
    }
    let mut children: BTreeMap<Option<String>, Vec<String>> = BTreeMap::new();
    for row in walker.rows.values() {
        children
            .entry(row.parent_key.clone())
            .or_default()
            .push(row.key.clone());
    }
    let mut output = vec![];
    append(None, &walker.rows, &children, &mut output);
    Ok(output)
}
pub fn cells(row: &Row, columns: &[Column], number: u32) -> Vec<String> {
    columns
        .iter()
        .map(|column| match column {
            Column::Item => number.to_string(),
            Column::PartNumber => row.part_number.clone(),
            Column::Description => row.description.clone(),
            Column::Quantity => row.quantity.to_string(),
            Column::Material => row.material.clone(),
            Column::Finish => row.finish.clone(),
            Column::Configuration => {
                if row.configuration_name.is_empty() {
                    row.configuration.clone().unwrap_or_default()
                } else {
                    row.configuration_name.clone()
                }
            }
            Column::Definition => row.definition.clone(),
            Column::ComponentType => match row.kind {
                SourceKind::Part => "Part",
                SourceKind::Assembly => "Assembly",
            }
            .into(),
            Column::Depth => (row.depth + 1).to_string(),
            Column::Manufacturing { field } => row.manufacturing.cell(field),
            Column::Property { key, .. } => row.properties.get(key).cloned().unwrap_or_default(),
        })
        .collect()
}
pub fn csv(doc: &Document, options: &Options) -> Result<String> {
    let mut out = csv_record(options.columns.iter().map(Column::label));
    for (index, row) in rows(doc, options)?.iter().enumerate() {
        out.push_str(&csv_record(
            cells(row, &options.columns, (index + 1) as u32)
                .iter()
                .map(String::as_str),
        ));
    }
    Ok(out)
}
pub(crate) fn csv_record<'a>(values: impl IntoIterator<Item = &'a str>) -> String {
    let mut out = values
        .into_iter()
        .map(|text| format!("\"{}\"", text.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(",");
    out.push('\n');
    out
}
