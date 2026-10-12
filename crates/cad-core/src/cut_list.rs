//! Source-linked manufacturing stock rows, independent of drawing widgets.
//! Equivalent stock is proved from authored section intent and current native
//! fabrication metadata. Display meshes never determine manufacturing identity.
use crate::{
    CadError, Document, Id, NodeRef, Result, manufacturing::CutListRow, protocol::Evaluation,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

pub const CONTRACT_VERSION: u32 = 1;
pub const MAX_ROWS: usize = 100_000;
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Grouping {
    #[default]
    BySource,
    EquivalentStock,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Column {
    Item,
    Description,
    Section,
    Length,
    StartCutAngle,
    EndCutAngle,
    Quantity,
    Material,
    Finish,
    Configuration,
    Definition,
    Source,
    BendRadius,
    BendSweep,
    Accessory,
    StockWidth,
    StockHeight,
    Thickness,
    StockBounds,
    Manufacturing {
        field: crate::manufacturing_columns::Column,
    },
}
impl Column {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Item => "Item",
            Self::Description => "Member",
            Self::Section => "Section",
            Self::Length => "Length",
            Self::StartCutAngle => "Start angle",
            Self::EndCutAngle => "Finish angle",
            Self::Quantity => "Qty",
            Self::Material => "Material",
            Self::Finish => "Finish",
            Self::Configuration => "Configuration",
            Self::Definition => "Definition",
            Self::Source => "Source",
            Self::BendRadius => "Bend radius",
            Self::BendSweep => "Bend sweep",
            Self::Accessory => "Accessory",
            Self::StockWidth => "Stock width",
            Self::StockHeight => "Stock height",
            Self::Thickness => "Thickness",
            Self::StockBounds => "Stock bounds",
            Self::Manufacturing { field } => field.label(),
        }
    }
}
pub fn default_columns() -> Vec<Column> {
    vec![
        Column::Item,
        Column::Description,
        Column::Section,
        Column::Length,
        Column::StartCutAngle,
        Column::EndCutAngle,
        Column::Quantity,
        Column::Material,
    ]
}
pub fn column_choices() -> Vec<Column> {
    let mut columns = vec![
        Column::Item,
        Column::Description,
        Column::Section,
        Column::Length,
        Column::StartCutAngle,
        Column::EndCutAngle,
        Column::Quantity,
        Column::Material,
        Column::Finish,
        Column::Configuration,
        Column::Definition,
        Column::Source,
        Column::BendRadius,
        Column::BendSweep,
        Column::Accessory,
        Column::StockWidth,
        Column::StockHeight,
        Column::Thickness,
        Column::StockBounds,
    ];
    columns.extend(
        crate::manufacturing_columns::Column::choices()
            .into_iter()
            .map(|field| Column::Manufacturing { field }),
    );
    columns
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Options {
    #[serde(default)]
    pub grouping: Grouping,
    #[serde(default = "default_columns")]
    pub columns: Vec<Column>,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            grouping: Grouping::BySource,
            columns: default_columns(),
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct Source {
    pub output: NodeRef,
    pub configuration: Option<Id>,
    pub member: crate::weldment::MemberRef,
    /// Full, stable occurrence path relative to the requested document root.
    pub occurrence_path: Vec<Id>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Group {
    pub key: String,
    /// Representative production values; all quantities and bindings are retained below.
    pub row: CutListRow,
    pub sources: Vec<Source>,
    pub quantity: usize,
    pub finish: String,
    pub material_id: Id,
    pub finish_id: Option<Id>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub configuration_names: Vec<String>,
    #[serde(
        default,
        skip_serializing_if = "crate::manufacturing_columns::Data::is_empty"
    )]
    pub manufacturing: crate::manufacturing_columns::Data,
}
fn invalid(message: impl ToString) -> CadError {
    CadError::Invalid(format!("Cut list: {}", message.to_string()))
}
pub fn validate(options: &Options) -> Result<()> {
    if options.columns.is_empty() || options.columns.len() > 32 {
        return Err(invalid("Choose between one and 32 columns"));
    }
    Ok(())
}
/// This legacy-compatible member key deliberately omits the final carrier output:
/// later trims/caps retain their original member identity and item reservation.
pub fn source_key(source: &Source) -> Result<String> {
    Ok(serde_json::to_string(
        &serde_json::json!({"kind":"cut_list", "configuration":source.configuration,"member":source.member}),
    )?)
}
/// Validate portable saved identities while retaining currently suppressed
/// members and pattern occurrences. Evaluation separately requires active data.
pub fn validate_source(doc: &Document, source: &Source) -> Result<()> {
    if source.occurrence_path.len() > crate::bom::MAX_DEPTH || source.member.member.is_empty() {
        return Err(invalid(
            "A saved source needs a bounded full occurrence path and member identity",
        ));
    }
    let mut owner = doc.root.clone();
    for id in &source.occurrence_path {
        let assembly = doc
            .assemblies
            .get(&owner)
            .ok_or_else(|| invalid("Saved source path crosses a missing assembly"))?;
        let definition = assembly
            .instances
            .get(id)
            .map(|instance| instance.definition.clone())
            .or_else(|| {
                assembly
                    .component_patterns
                    .values()
                    .flat_map(|pattern| &pattern.occurrences)
                    .find(|reservation| &reservation.instance == id)
                    .and_then(|reservation| assembly.instances.get(&reservation.seed))
                    .map(|instance| instance.definition.clone())
            })
            .ok_or_else(|| invalid("Saved source occurrence was removed"))?;
        owner = definition;
    }
    if owner != source.output.definition {
        return Err(invalid(
            "Saved source occurrence belongs to a different definition",
        ));
    }
    if let Some(configuration) = &source.configuration {
        crate::configuration::resolve(doc, &owner, configuration)?;
    }
    let part = doc
        .parts
        .get(&source.output.definition)
        .ok_or_else(|| invalid("Saved stock output part is missing"))?;
    let output = part
        .features
        .iter()
        .find(|feature| feature.id == source.output.node)
        .ok_or_else(|| invalid("Saved stock output was removed"))?;
    if let crate::Operation::StructuralMember { .. } = &output.operation {
        return if source.member.group == source.output
            && source.member.member == "legacy-structural-member"
        {
            Ok(())
        } else {
            Err(invalid("Legacy stock source identity is inconsistent"))
        };
    }
    if !matches!(output.operation, crate::Operation::Weldment { .. }) {
        return Err(invalid("Saved output is no longer native weldment stock"));
    }
    let group = doc
        .parts
        .get(&source.member.group.definition)
        .and_then(|part| {
            part.features
                .iter()
                .find(|feature| feature.id == source.member.group.node)
        })
        .ok_or_else(|| invalid("Saved member or accessory source was removed"))?;
    let crate::Operation::Weldment { operation } = &group.operation else {
        return Err(invalid("Saved member source changed feature type"));
    };
    let mut pending = vec![source.output.clone()];
    let mut visited = BTreeSet::new();
    while let Some(node) = pending.pop() {
        if !visited.insert(node.clone()) {
            continue;
        }
        if node == source.member.group {
            break;
        }
        let feature = doc
            .parts
            .get(&node.definition)
            .and_then(|part| part.features.iter().find(|feature| feature.id == node.node))
            .ok_or_else(|| invalid("Saved fabrication carrier ancestry was removed"))?;
        if let crate::Operation::Weldment { operation } = &feature.operation {
            pending.extend(operation.consumed_outputs());
        }
    }
    if !visited.contains(&source.member.group) {
        return Err(invalid(
            "Saved member no longer belongs to its selected output ancestry",
        ));
    }
    match operation.as_ref() {
        crate::weldment::WeldmentOperation::Members { path, .. } => {
            if !profile_member_exists(doc, path, &source.member.member)? {
                return Err(invalid("Saved structural path member was removed"));
            }
        }
        crate::weldment::WeldmentOperation::EndCap { .. } => {
            if source.member.member != "cap" {
                return Err(invalid("Saved cap member identity is inconsistent"));
            }
        }
        crate::weldment::WeldmentOperation::Gusset { profile, .. } => {
            let member = source
                .member
                .member
                .strip_prefix("gusset-contour-")
                .ok_or_else(|| invalid("Saved gusset region identity is inconsistent"))?;
            if !profile_member_exists(doc, profile, member)? {
                return Err(invalid("Saved gusset contour member was removed"));
            }
        }
        crate::weldment::WeldmentOperation::Trim { .. } => {
            return Err(invalid(
                "A trim carrier cannot replace a member's original group identity",
            ));
        }
    }
    Ok(())
}
fn profile_member_exists(doc: &Document, reference: &NodeRef, member: &str) -> Result<bool> {
    let part = doc
        .parts
        .get(&reference.definition)
        .ok_or_else(|| invalid("Source profile part is missing"))?;
    let feature = part
        .features
        .iter()
        .find(|feature| feature.id == reference.node)
        .ok_or_else(|| invalid("Source profile was removed"))?;
    let crate::Operation::Profile { profile, .. } = &feature.operation else {
        return Err(invalid("Source profile changed feature type"));
    };
    let indexed = |prefix: &str, count: usize| {
        member
            .strip_prefix(prefix)
            .and_then(|value| value.parse::<usize>().ok())
            .is_some_and(|index| index < count)
    };
    Ok(match profile {
        crate::Profile::Polygon { points } => indexed("legacy-profile-edge-", points.len()),
        crate::Profile::Circle { .. } => member == "circle",
        crate::Profile::Sketch { sketch } => {
            let sketch = part
                .sketches
                .get(sketch)
                .ok_or_else(|| invalid("Source sketch was removed"))?;
            sketch.members.contains_key(member)
                || indexed("legacy-segment-", sketch.segments.len())
                || indexed("legacy-profile-edge-", sketch.profile_order.len())
                || member
                    .strip_prefix("legacy-circle-")
                    .is_some_and(|id| sketch.circles.contains_key(id))
        }
    })
}
fn hashed(value: &impl Serialize) -> Result<String> {
    Ok(format!("{:x}", Sha256::digest(serde_json::to_vec(value)?)))
}

pub fn rows(doc: &Document, evaluation: &Evaluation, options: &Options) -> Result<Vec<Group>> {
    group(
        doc,
        evaluation,
        crate::manufacturing::cut_list_evaluated(doc, evaluation)?,
        options,
    )
}
/// Group rows already filtered to a drawing's represented configurations/outputs.
/// Exact source/member/occurrence bindings survive either grouping mode.
pub fn group(
    doc: &Document,
    evaluation: &Evaluation,
    rows: Vec<CutListRow>,
    options: &Options,
) -> Result<Vec<Group>> {
    validate(options)?;
    if !evaluation.valid && !evaluation.geometry_valid {
        return Err(invalid(
            "Stock grouping requires the current complete native geometry evaluation",
        ));
    }
    if rows.len() > MAX_ROWS {
        return Err(invalid("Enumeration exceeds the bounded stock row limit"));
    }
    let occurrences = crate::configuration::placed_parts(doc, Some(&evaluation.assemblies))?;
    if occurrences.len() > MAX_ROWS {
        return Err(invalid("Enumeration exceeds the bounded occurrence limit"));
    }
    let mut occurrence_groups: BTreeMap<(Id, Option<Id>), Vec<&crate::PlacedPart>> =
        BTreeMap::new();
    for occurrence in occurrences
        .iter()
        .filter(|occurrence| !occurrence.bom_excluded)
    {
        occurrence_groups
            .entry((
                occurrence.definition.clone(),
                occurrence.configuration.clone(),
            ))
            .or_default()
            .push(occurrence);
    }
    let mut grouped: BTreeMap<String, Group> = BTreeMap::new();
    let mut binding_count = 0usize;
    for mut row in rows {
        // Display labels are rebuilt from current names, including for legacy
        // metadata whose optional label field was absent. They never enter
        // production_values, stock_key, or source_key.
        row.section_label = row
            .section
            .as_ref()
            .and_then(|section| crate::manufacturing::section_label(doc, section));
        let configured =
            crate::configuration::variant(doc, &row.definition, row.configuration.as_deref())?;
        let part = configured
            .parts
            .get(&row.definition)
            .ok_or_else(|| invalid("Manufacturing part is missing"))?;
        let configuration_name = row
            .configuration
            .as_deref()
            .map(|id| {
                crate::configuration::resolve(doc, &row.definition, id)
                    .map(|configuration| configuration.name)
            })
            .transpose()?;
        let matching = occurrence_groups
            .get(&(row.definition.clone(), row.configuration.clone()))
            .map(Vec::as_slice)
            .unwrap_or_default();
        if matching.len() != row.quantity {
            return Err(invalid(
                "Stock quantity does not match its current configured occurrence bindings",
            ));
        }
        let member = row
            .member_reference
            .clone()
            .unwrap_or_else(|| crate::weldment::MemberRef {
                group: part.reference(&row.feature),
                member: "legacy-structural-member".into(),
            });
        let mut stock_keys: BTreeMap<Id, String> = BTreeMap::new();
        for instance in matching {
            binding_count = binding_count
                .checked_add(1)
                .ok_or_else(|| invalid("Stock binding count overflow"))?;
            if binding_count > MAX_ROWS {
                return Err(invalid(
                    "Stock source binding enumeration exceeds its bounded limit",
                ));
            }
            let source = Source {
                output: part.reference(&row.feature),
                configuration: row.configuration.clone(),
                member: member.clone(),
                occurrence_path: instance.path.clone(),
            };
            let finish_id = instance
                .finish
                .clone()
                .unwrap_or_else(|| part.finish.clone());
            let finish = doc
                .finishes
                .get(&finish_id)
                .map(|finish| finish.name.clone())
                .unwrap_or_else(|| finish_id.clone());
            let manufacturing = if options
                .columns
                .iter()
                .any(|column| matches!(column, Column::Manufacturing { .. }))
            {
                crate::manufacturing_columns::Data::from_definitions(
                    &configured,
                    configured.materials.get(&part.material),
                    configured.finishes.get(&finish_id),
                    Some(part),
                )
            } else {
                Default::default()
            };
            let key = match options.grouping {
                Grouping::BySource => source_key(&source)?,
                Grouping::EquivalentStock => {
                    if let Some(key) = stock_keys.get(&finish_id) {
                        key.clone()
                    } else {
                        let key = stock_key(
                            &configured,
                            evaluation,
                            &row,
                            &source,
                            &part.material,
                            &finish_id,
                        )?;
                        stock_keys.insert(finish_id.clone(), key.clone());
                        key
                    }
                }
            };
            if options.grouping == Grouping::BySource
                && let Some(existing) = grouped.get(&key)
                && (existing.row.feature != row.feature
                    || existing.row.definition != row.definition
                    || existing.material_id != part.material
                    || production_values(&existing.row) != production_values(&row))
            {
                return Err(CadError::Unsupported("Cut list: Parallel native outputs carry distinct production states for one member lineage; select a single output frontier or use equivalent stock grouping".into()));
            }
            let group = grouped.entry(key.clone()).or_insert_with(|| Group {
                key,
                row: row.clone(),
                sources: vec![],
                quantity: 0,
                finish: finish.clone(),
                material_id: part.material.clone(),
                finish_id: Some(finish_id.clone()),
                configuration_names: configuration_name.clone().into_iter().collect(),
                manufacturing: manufacturing.clone(),
            });
            group.quantity = group
                .quantity
                .checked_add(1)
                .ok_or_else(|| invalid("Grouped stock quantity overflow"))?;
            group.row.quantity = group.quantity;
            if group.finish_id.as_deref() != Some(&finish_id) {
                group.finish_id = None;
                group.finish = "Multiple finishes".into();
            }
            group.manufacturing.merge_finish(&manufacturing);
            if let Some(name) = &configuration_name
                && !group.configuration_names.contains(name)
            {
                group.configuration_names.push(name.clone());
            }
            group.sources.push(source);
        }
    }
    for group in grouped.values_mut() {
        group.sources.sort();
        group.sources.dedup();
        if group.sources.len() != group.quantity {
            return Err(invalid(
                "A stock member has duplicate output/occurrence bindings",
            ));
        }
    }
    Ok(grouped.into_values().collect())
}
fn stock_key(
    doc: &Document,
    evaluation: &Evaluation,
    row: &CutListRow,
    source: &Source,
    material: &str,
    finish: &str,
) -> Result<String> {
    let mut intent = stock_intent(doc, evaluation, row, source, material, finish)?;
    let object = intent
        .as_object_mut()
        .ok_or_else(|| invalid("Stock intent contract is invalid"))?;
    let source = object
        .remove("unproved_clocking_or_region")
        .unwrap_or(serde_json::Value::Null);
    object.remove("material");
    object.remove("finish");
    Ok(serde_json::to_string(
        &serde_json::json!({"kind":"stock","geometry_sha256":hashed(&intent)?,"source":source,"material":material,"finish":finish}),
    )?)
}

fn stock_intent(
    doc: &Document,
    evaluation: &Evaluation,
    row: &CutListRow,
    source: &Source,
    material: &str,
    finish: &str,
) -> Result<serde_json::Value> {
    use crate::{
        Operation,
        weldment::{AccessoryKind, Corner, WeldmentOperation},
    };
    let part = &doc.parts[&row.definition];
    let feature = part
        .features
        .iter()
        .find(|feature| feature.id == source.member.group.node)
        .ok_or_else(|| invalid("Stock source feature is missing"))?;
    let section = row
        .section
        .as_ref()
        .map(|section| profile_intent(doc, evaluation, section))
        .transpose()?;
    let mut orientation = serde_json::Value::Null;
    let mut unproved = serde_json::Value::Null;
    match (&row.accessory, &feature.operation) {
        (None, Operation::Weldment { operation }) => {
            if let WeldmentOperation::Members {
                roll,
                anchor,
                section,
                corner,
                ..
            } = operation.as_ref()
            {
                let anchor_position = anchor
                    .as_ref()
                    .map(|id| {
                        let key = format!(
                            "{}::{}",
                            crate::configuration::key(
                                &section.definition,
                                doc.active_configurations
                                    .get(&section.definition)
                                    .map(String::as_str)
                            ),
                            section.node
                        );
                        evaluation
                            .sketches
                            .get(&key)
                            .and_then(|sketch| sketch.points.get(id))
                            .copied()
                            .ok_or_else(|| {
                                invalid("Section locating point requires the current solved sketch")
                            })
                    })
                    .transpose()?;
                orientation = serde_json::json!({"roll_rad":crate::expression::value(doc,&row.definition,roll,crate::Dimension::Angle)?,"anchor":anchor_position});
                // Scalar end angles cannot prove the clocking of oblique cuts or
                // trimmed walls on asymmetric sections. Keep those members separate.
                if *corner == Corner::ButtTrim
                    || row.start_cut_angle_rad.is_some_and(|v| v.abs() > 1e-12)
                    || row.end_cut_angle_rad.is_some_and(|v| v.abs() > 1e-12)
                {
                    unproved = serde_json::to_value(&source.member)?;
                }
            } else {
                return Err(invalid(
                    "Structural member ancestry does not name a section/path group",
                ));
            }
        }
        (Some(AccessoryKind::EndCap), Operation::Weldment { operation }) => {
            let WeldmentOperation::EndCap {
                member,
                end,
                contour,
                edge_offset,
                ..
            } = operation.as_ref()
            else {
                return Err(invalid("Cap ancestry does not name an end cap"));
            };
            orientation = serde_json::json!({"contour":contour,"edge_offset_mm":crate::expression::value(doc,&row.definition,edge_offset,crate::Dimension::Length)?});
            let key = format!(
                "{}::{}",
                crate::configuration::key(&row.definition, row.configuration.as_deref()),
                row.feature
            );
            let parent = evaluation
                .weldments
                .get(&key)
                .and_then(|stock| {
                    stock
                        .members
                        .iter()
                        .find(|evaluated| &evaluated.reference == member)
                })
                .ok_or_else(|| {
                    invalid("Cap source needs the current evaluated supporting member")
                })?;
            let angle = if *end == crate::weldment::End::Start {
                parent.start_cut_angle_rad
            } else {
                parent.end_cut_angle_rad
            };
            if angle.abs() > 1e-12 {
                unproved = serde_json::to_value(&source.member)?;
            }
        }
        (Some(AccessoryKind::Gusset), Operation::Weldment { operation }) => {
            if !matches!(operation.as_ref(), WeldmentOperation::Gusset { .. }) {
                return Err(invalid("Gusset ancestry does not name a sketch plate"));
            }
            // A multi-region plate may contain different outlines with equal
            // bounds. Current metadata names its region but does not publish its
            // exact outline, so never combine different region identities.
            unproved = serde_json::to_value(&source.member)?;
        }
        (None, Operation::StructuralMember { section, roll, .. }) => {
            orientation = serde_json::json!({"legacy_section":legacy_profile(doc,&row.definition,section)?,"roll_rad":crate::expression::value(doc,&row.definition,roll,crate::Dimension::Angle)?});
        }
        _ => return Err(invalid("Unsupported source for equivalent stock grouping")),
    }
    Ok(
        serde_json::json!({"contract":CONTRACT_VERSION,"section":section,"orientation":orientation,"unproved_clocking_or_region":unproved,"material":material,"finish":finish,"length_mm":row.length_mm,"start_cut_angle_rad":row.start_cut_angle_rad,"end_cut_angle_rad":row.end_cut_angle_rad,"bend_radius_mm":row.bend_radius_mm,"sweep_angle_rad":row.sweep_angle_rad,"accessory":row.accessory,"blank_size_mm":row.blank_size_mm,"thickness_mm":row.thickness_mm,"blank_bounds_exact":row.blank_bounds_exact}),
    )
}
fn production_values(row: &CutListRow) -> serde_json::Value {
    serde_json::json!({"length_mm":row.length_mm,"section":row.section,"start":row.start_cut_angle_rad,"end":row.end_cut_angle_rad,"radius":row.bend_radius_mm,"sweep":row.sweep_angle_rad,"accessory":row.accessory,"blank":row.blank_size_mm,"thickness":row.thickness_mm,"exact":row.blank_bounds_exact})
}
fn legacy_profile(
    doc: &Document,
    owner: &str,
    profile: &crate::Profile,
) -> Result<serde_json::Value> {
    Ok(match profile {
        crate::Profile::Polygon{points}=>serde_json::json!({"kind":"polygon","points":points}),
        crate::Profile::Circle{radius,center}=>serde_json::json!({"kind":"circle","center":center,"radius":crate::expression::value(doc,owner,radius,crate::Dimension::Length)?}),
        crate::Profile::Sketch{..}=>return Err(CadError::Unsupported("Cut list: Legacy structural sketch profiles need conversion to native section/path stock before equivalent grouping".into())),
    })
}

fn profile_intent(
    doc: &Document,
    evaluation: &Evaluation,
    source: &NodeRef,
) -> Result<serde_json::Value> {
    use crate::{Dimension, Operation, Profile, SketchCurve};
    let part = doc
        .parts
        .get(&source.definition)
        .ok_or_else(|| invalid("Section/profile definition is missing"))?;
    let feature = part
        .features
        .iter()
        .find(|feature| feature.id == source.node && !feature.suppressed)
        .ok_or_else(|| invalid("Section/profile source is missing or suppressed"))?;
    let Operation::Profile { profile, .. } = &feature.operation else {
        return Err(invalid(
            "Equivalent stock requires an authored section profile",
        ));
    };
    Ok(match profile {
        Profile::Polygon { points } => serde_json::json!({"kind":"polygon","points":points}),
        Profile::Circle { radius, center } => {
            serde_json::json!({"kind":"circle","center":center,"radius":crate::expression::value(doc,&source.definition,radius,Dimension::Length)?})
        }
        Profile::Sketch { sketch } => {
            let key = format!(
                "{}::{}",
                crate::configuration::key(
                    &source.definition,
                    doc.active_configurations
                        .get(&source.definition)
                        .map(String::as_str)
                ),
                source.node
            );
            let evaluated = evaluation.sketches.get(&key).ok_or_else(|| {
                invalid("Equivalent stock requires the current solved section sketch")
            })?;
            let solved = evaluated
                .solved
                .as_ref()
                .ok_or_else(|| invalid("Solved native section intent is unavailable"))?;
            if solved.id != *sketch {
                return Err(invalid("Solved section identity does not match its source"));
            }
            let point = |id: &str| {
                solved
                    .points
                    .get(id)
                    .copied()
                    .ok_or_else(|| invalid("Solved section point is missing"))
            };
            let quantity = |expression: &crate::Expr, dimension| {
                crate::expression::value(doc, &source.definition, expression, dimension)
            };
            let mut curves = Vec::new();
            for (id, _) in solved
                .members
                .iter()
                .filter(|(_, member)| !member.construction)
            {
                let curve = crate::sketch_edit::effective_curve(solved, id)?;
                curves.push(match curve {
                    SketchCurve::Line{start,end}=>serde_json::json!({"kind":"line","start":point(&start)?,"end":point(&end)?}),
                    SketchCurve::Arc{start,through,end}=>serde_json::json!({"kind":"arc","start":point(&start)?,"through":point(&through)?,"end":point(&end)?}),
                    SketchCurve::Bezier{points}=>serde_json::json!({"kind":"bezier","points":points.iter().map(|id|point(id)).collect::<Result<Vec<_>>>()?}),
                    SketchCurve::Nurbs{points,weights,knots,degree}=>serde_json::json!({"kind":"nurbs","points":points.iter().map(|id|point(id)).collect::<Result<Vec<_>>>()?,"weights":weights,"knots":knots,"degree":degree}),
                    SketchCurve::Circle{center,radius}=>serde_json::json!({"kind":"circle","center":point(&center)?,"radius":quantity(&radius,Dimension::Length)?}),
                    SketchCurve::Ellipse{center,major,minor,angle}=>serde_json::json!({"kind":"ellipse","center":point(&center)?,"major":quantity(&major,Dimension::Length)?,"minor":quantity(&minor,Dimension::Length)?,"angle":quantity(&angle,Dimension::Angle)?}),
                });
            }
            if solved.members.is_empty() {
                if !solved.profile_order.is_empty() {
                    curves.push(serde_json::json!({"kind":"legacy_profile_polygon","points":solved.profile_order.iter().map(|id|point(id)).collect::<Result<Vec<_>>>()?}));
                }
                for segment in &solved.segments {
                    curves.push(serde_json::json!({"kind":"line","start":point(&segment[0])?,"end":point(&segment[1])?}));
                }
                for circle in solved.circles.values() {
                    curves.push(serde_json::json!({"kind":"circle","center":point(&circle.center)?,"radius":quantity(&circle.radius,Dimension::Length)?}));
                }
            }
            curves.sort_by_cached_key(|curve| curve.to_string());
            serde_json::json!({"kind":"sketch","curves":curves})
        }
    })
}

/// The designed straight member midpoint, curved member's start station, or
/// accessory center in the current root frame; never a fitted mesh center.
pub fn source_point(doc: &Document, evaluation: &Evaluation, source: &Source) -> Result<[f64; 3]> {
    source_points(doc, evaluation, std::slice::from_ref(source))?
        .remove(source)
        .ok_or_else(|| invalid("Designed stock source point is unavailable"))
}
/// Batch manufacturing placement lookup avoids traversing the full assembly
/// once for every stock row or auto balloon.
pub fn source_points(
    doc: &Document,
    evaluation: &Evaluation,
    sources: &[Source],
) -> Result<BTreeMap<Source, [f64; 3]>> {
    if !evaluation.valid && !evaluation.geometry_valid {
        return Err(invalid(
            "Source positions require the current complete native geometry evaluation",
        ));
    }
    if sources.len() > MAX_ROWS {
        return Err(invalid(
            "Source point enumeration exceeds its bounded limit",
        ));
    }
    let instances = crate::configuration::placed_parts(doc, Some(&evaluation.assemblies))?
        .into_iter()
        .filter(|instance| !instance.bom_excluded)
        .map(|instance| {
            (
                (
                    instance.path.clone(),
                    instance.definition.clone(),
                    instance.configuration.clone(),
                ),
                instance,
            )
        })
        .collect::<BTreeMap<_, _>>();
    let mut output = BTreeMap::new();
    for source in sources {
        let instance = instances
            .get(&(
                source.occurrence_path.clone(),
                source.output.definition.clone(),
                source.configuration.clone(),
            ))
            .ok_or_else(|| invalid("Source occurrence/configuration is not represented"))?;
        let local = local_point(doc, evaluation, source)?;
        output.insert(
            source.clone(),
            std::array::from_fn(|axis| {
                instance.matrix[axis][0] * local[0]
                    + instance.matrix[axis][1] * local[1]
                    + instance.matrix[axis][2] * local[2]
                    + instance.matrix[axis][3]
            }),
        );
    }
    Ok(output)
}
fn local_point(doc: &Document, evaluation: &Evaluation, source: &Source) -> Result<[f64; 3]> {
    let key = format!(
        "{}::{}",
        crate::configuration::key(&source.output.definition, source.configuration.as_deref()),
        source.output.node
    );
    let local = if let Some(stock) = evaluation.weldments.get(&key) {
        if stock.version != crate::weldment::CONTRACT_VERSION || stock.source != source.output {
            return Err(invalid("Source needs current native fabrication metadata"));
        }
        if let Some(member) = stock
            .members
            .iter()
            .find(|member| member.reference == source.member)
        {
            if member.path_kind == crate::weldment::MemberPathKind::Straight {
                std::array::from_fn(|axis| (member.start[axis] + member.end[axis]) * 0.5)
            } else {
                member.start
            }
        } else if let Some(accessory) = stock
            .accessories
            .iter()
            .find(|accessory| accessory.reference == source.member)
        {
            accessory.center
        } else {
            return Err(invalid(
                "Selected stock member/accessory is missing or suppressed",
            ));
        }
    } else {
        let part = crate::configuration::effective_part(
            doc,
            &source.output.definition,
            source.configuration.as_deref(),
        )?;
        let feature = part
            .features
            .iter()
            .find(|feature| feature.id == source.output.node && !feature.suppressed)
            .ok_or_else(|| invalid("Legacy structural source is missing"))?;
        let crate::Operation::StructuralMember { start, end, .. } = &feature.operation else {
            return Err(invalid("Source has no current native fabrication metadata"));
        };
        std::array::from_fn(|axis| (start[axis] + end[axis]) * 0.5)
    };
    Ok(local)
}

pub fn cells(
    group: &Group,
    columns: &[Column],
    number: u32,
    units: crate::drawing::Units,
    precision: usize,
) -> Vec<String> {
    let length = |value: f64| {
        format!(
            "{:.*}",
            precision,
            if units == crate::drawing::Units::Inches {
                value / 25.4
            } else {
                value
            }
        )
    };
    let optional_length = |value: Option<f64>| value.map(length).unwrap_or_default();
    let angle = |value: Option<f64>| {
        value
            .map(|value| format!("{:.*}", precision, value.to_degrees()))
            .unwrap_or_default()
    };
    let row = &group.row;
    columns
        .iter()
        .map(|column| match column {
            Column::Item => number.to_string(),
            Column::Description => row.description.clone(),
            Column::Section => crate::manufacturing::section_display(row, units, precision),
            Column::Length => length(row.length_mm),
            Column::StartCutAngle => angle(row.start_cut_angle_rad),
            Column::EndCutAngle => angle(row.end_cut_angle_rad),
            Column::Quantity => group.quantity.to_string(),
            Column::Material => row.material.clone(),
            Column::Finish => group.finish.clone(),
            Column::Manufacturing { field } => group.manufacturing.cell(field),
            Column::Configuration => {
                if group.configuration_names.is_empty() {
                    distinct(
                        group
                            .sources
                            .iter()
                            .map(|source| source.configuration.as_deref().unwrap_or_default()),
                    )
                } else {
                    distinct(group.configuration_names.iter().map(String::as_str))
                }
            }
            Column::Definition => distinct(
                group
                    .sources
                    .iter()
                    .map(|source| source.output.definition.as_str()),
            ),
            Column::Source => distinct(
                group
                    .sources
                    .iter()
                    .map(|source| source.member.member.as_str()),
            ),
            Column::BendRadius => optional_length(row.bend_radius_mm),
            Column::BendSweep => angle(row.sweep_angle_rad),
            Column::Accessory => row
                .accessory
                .map(|kind| {
                    match kind {
                        crate::weldment::AccessoryKind::EndCap => "End cap",
                        crate::weldment::AccessoryKind::Gusset => "Gusset",
                    }
                    .into()
                })
                .unwrap_or_default(),
            Column::StockWidth => optional_length(row.blank_size_mm.map(|size| size[0])),
            Column::StockHeight => optional_length(row.blank_size_mm.map(|size| size[1])),
            Column::Thickness => optional_length(row.thickness_mm),
            Column::StockBounds => row
                .blank_bounds_exact
                .map(|exact| {
                    if exact {
                        "Exact rectangle"
                    } else {
                        "Conservative control hull"
                    }
                    .into()
                })
                .unwrap_or_default(),
        })
        .collect()
}
fn distinct<'a>(values: impl Iterator<Item = &'a str>) -> String {
    values
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>()
        .join("; ")
}
pub fn csv(
    doc: &Document,
    evaluation: &Evaluation,
    options: &Options,
    units: crate::drawing::Units,
    precision: usize,
) -> Result<String> {
    if precision > 9 {
        return Err(invalid("Use at most nine displayed decimal places"));
    }
    let mut output = crate::bom::csv_record(options.columns.iter().map(Column::label));
    for (index, row) in rows(doc, evaluation, options)?.iter().enumerate() {
        output.push_str(&crate::bom::csv_record(
            cells(row, &options.columns, (index + 1) as u32, units, precision)
                .iter()
                .map(String::as_str),
        ));
    }
    Ok(output)
}
