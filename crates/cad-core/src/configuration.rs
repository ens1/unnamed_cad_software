//! Native, versioned configuration definitions, inheritance, and local design tables.
use crate::{Assembly, CadError, Document, Expr, Id, Part, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};
pub mod counts;
pub mod dimensions;
pub mod sketch_dimensions;
pub use dimensions::{DimensionField, FIELD_CONTRACT_VERSION};
pub const CONTRACT_VERSION: u32 = 8;
fn supported_version(value: u32) -> bool {
    (1..=CONTRACT_VERSION).contains(&value)
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BomName {
    #[default]
    Document,
    Configuration,
    Custom,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct BomOptions {
    #[serde(default)]
    pub exclude: Option<bool>,
    #[serde(default)]
    pub name: Option<BomName>,
    #[serde(default)]
    pub part_number: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Overrides {
    /// Native parameter IDs. Part legacy maps remain readable; these values take precedence.
    #[serde(default)]
    pub parameters: BTreeMap<Id, Expr>,
    /// Feature IDs in the owning part and typed expression paths within their operations.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub feature_dimensions: BTreeMap<Id, BTreeMap<String, Expr>>,
    /// Stable sketch and driving constraint IDs, resolved before sketch solving.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub sketch_dimensions: BTreeMap<Id, BTreeMap<Id, Expr>>,
    #[serde(default)]
    pub features: BTreeMap<Id, bool>,
    #[serde(default)]
    pub sketches: BTreeMap<Id, bool>,
    #[serde(default)]
    pub components: BTreeMap<Id, bool>,
    #[serde(default)]
    pub component_patterns: BTreeMap<Id, bool>,
    #[serde(default)]
    pub mates: BTreeMap<Id, bool>,
    /// Owning-assembly deposited weld IDs. True suppresses, false restores.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub assembly_welds: BTreeMap<Id, bool>,
    /// Saved rigid-group IDs. True releases its locks; false restores them.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub rigid_groups: BTreeMap<Id, bool>,
    #[serde(default)]
    pub component_configurations: BTreeMap<Id, Option<Id>>,
    /// Stable owning-assembly occurrence IDs. Each omitted coordinate inherits.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub component_placements: BTreeMap<Id, PoseOverride>,
    /// Canonical 0..1 source hints for free Path/Slot/Cam mates, never hard fractions.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub mate_curve_seeds: BTreeMap<Id, f64>,
    #[serde(default)]
    pub material: Option<Id>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finish: Option<Id>,
    #[serde(default)]
    pub properties: BTreeMap<String, String>,
    #[serde(default)]
    pub bom: BomOptions,
}
/// Authoritative millimetres and Euler XYZ degrees, matching `Placement`.
/// These are source seeds; assembly mate solving remains authoritative.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct PoseOverride {
    #[serde(default)]
    pub translation: [Option<f64>; 3],
    #[serde(default)]
    pub rotation_deg: [Option<f64>; 3],
}
impl PoseOverride {
    pub fn is_empty(&self) -> bool {
        self.translation
            .iter()
            .chain(&self.rotation_deg)
            .all(Option::is_none)
    }
    pub fn apply(&self, pose: &mut crate::Placement) {
        for axis in 0..3 {
            if let Some(value) = self.translation[axis] {
                pose.translation[axis] = value;
            }
            if let Some(value) = self.rotation_deg[axis] {
                pose.rotation_deg[axis] = value;
            }
        }
    }
    fn merge(&mut self, next: &Self) {
        for axis in 0..3 {
            if next.translation[axis].is_some() {
                self.translation[axis] = next.translation[axis];
            }
            if next.rotation_deg[axis].is_some() {
                self.rotation_deg[axis] = next.rotation_deg[axis];
            }
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", content = "configuration", rename_all = "snake_case")]
pub enum PlacementScope {
    Base,
    Named(Id),
}
impl PlacementScope {
    pub fn selected(doc: &Document, assembly: &str) -> Self {
        doc.active_configurations
            .get(assembly)
            .cloned()
            .map(Self::Named)
            .unwrap_or(Self::Base)
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Definition {
    #[serde(default = "version")]
    pub version: u32,
    pub id: Id,
    pub name: String,
    #[serde(default)]
    pub parent: Option<Id>,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub overrides: Overrides,
}
fn version() -> u32 {
    CONTRACT_VERSION
}
impl Definition {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            version: CONTRACT_VERSION,
            id: crate::id(),
            name: name.into(),
            parent: None,
            description: String::new(),
            overrides: Overrides::default(),
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Resolved {
    pub version: u32,
    pub id: Id,
    pub name: String,
    pub ancestors: Vec<Id>,
    pub overrides: Overrides,
}
fn invalid(s: impl Into<String>) -> CadError {
    CadError::Invalid(format!("Configuration: {}", s.into()))
}
pub fn names(doc: &Document, definition: &str) -> BTreeMap<Id, String> {
    if let Some(p) = doc.parts.get(definition) {
        p.configurations
            .keys()
            .map(|id| {
                (
                    id.clone(),
                    p.configuration_details
                        .get(id)
                        .map(|c| c.name.clone())
                        .unwrap_or_else(|| id.clone()),
                )
            })
            .chain(
                p.configuration_details
                    .iter()
                    .map(|(id, c)| (id.clone(), c.name.clone())),
            )
            .collect()
    } else {
        doc.assemblies
            .get(definition)
            .map(|a| {
                a.configurations
                    .iter()
                    .map(|(id, c)| (id.clone(), c.name.clone()))
                    .collect()
            })
            .unwrap_or_default()
    }
}
pub fn exists(doc: &Document, definition: &str, configuration: &str) -> bool {
    names(doc, definition).contains_key(configuration)
}
pub fn definitions(doc: &Document, definition: &str) -> BTreeMap<Id, Definition> {
    if let Some(p) = doc.parts.get(definition) {
        let mut result = p.configuration_details.clone();
        for (id, parameters) in &p.configurations {
            let entry = result.entry(id.clone()).or_insert_with(|| Definition {
                version: CONTRACT_VERSION,
                id: id.clone(),
                name: id.clone(),
                parent: None,
                description: String::new(),
                overrides: Overrides::default(),
            });
            let mut merged = parameters.clone();
            merged.extend(entry.overrides.parameters.clone());
            entry.overrides.parameters = merged;
        }
        result
    } else {
        doc.assemblies
            .get(definition)
            .map(|a| a.configurations.clone())
            .unwrap_or_default()
    }
}
fn merge(target: &mut Overrides, next: &Overrides) {
    target.parameters.extend(next.parameters.clone());
    for (feature, fields) in &next.feature_dimensions {
        target
            .feature_dimensions
            .entry(feature.clone())
            .or_default()
            .extend(fields.clone());
    }
    target.features.extend(next.features.clone());
    for (sketch, dimensions) in &next.sketch_dimensions {
        target
            .sketch_dimensions
            .entry(sketch.clone())
            .or_default()
            .extend(dimensions.clone());
    }
    target.sketches.extend(next.sketches.clone());
    target.components.extend(next.components.clone());
    target
        .component_patterns
        .extend(next.component_patterns.clone());
    target.mates.extend(next.mates.clone());
    target.assembly_welds.extend(next.assembly_welds.clone());
    target.rigid_groups.extend(next.rigid_groups.clone());
    target
        .component_configurations
        .extend(next.component_configurations.clone());
    for (instance, pose) in &next.component_placements {
        target
            .component_placements
            .entry(instance.clone())
            .or_default()
            .merge(pose);
    }
    target
        .mate_curve_seeds
        .extend(next.mate_curve_seeds.clone());
    target.properties.extend(next.properties.clone());
    if next.finish.is_some() {
        target.finish = next.finish.clone();
    }
    if next.material.is_some() {
        target.material = next.material.clone();
    }
    if next.bom.exclude.is_some() {
        target.bom.exclude = next.bom.exclude;
    }
    if next.bom.name.is_some() {
        target.bom.name = next.bom.name.clone();
    }
    if next.bom.part_number.is_some() {
        target.bom.part_number = next.bom.part_number.clone();
    }
    if next.bom.description.is_some() {
        target.bom.description = next.bom.description.clone();
    }
}
pub fn resolve(doc: &Document, definition: &str, key: &str) -> Result<Resolved> {
    let definitions = definitions(doc, definition);
    let mut path = Vec::new();
    let mut seen = BTreeSet::new();
    let mut current = Some(key.to_string());
    while let Some(id) = current {
        if !seen.insert(id.clone()) {
            return Err(invalid(format!("Derived configuration cycle at {id}")));
        }
        let c = definitions
            .get(&id)
            .ok_or_else(|| invalid(format!("Missing named configuration {id}")))?;
        path.push(id);
        current = c.parent.clone();
    }
    path.reverse();
    let mut overrides = Overrides::default();
    for id in &path {
        if let Some(p) = doc.parts.get(definition)
            && let Some(values) = p.configurations.get(id)
        {
            overrides.parameters.extend(values.clone());
        }
        merge(&mut overrides, &definitions[id].overrides);
    }
    Ok(Resolved {
        version: CONTRACT_VERSION,
        id: key.into(),
        name: definitions[key].name.clone(),
        ancestors: path,
        overrides,
    })
}
fn require_keys<'a>(
    keys: impl Iterator<Item = &'a Id>,
    valid: impl Fn(&str) -> bool,
    label: &str,
) -> Result<()> {
    for key in keys {
        if !valid(key) {
            return Err(invalid(format!("Unknown {label} {key}")));
        }
    }
    Ok(())
}
/// Discover actual expression fields and bounded unsigned repetition counts by native feature identity.
pub fn dimension_fields(doc: &Document, definition: &str) -> Result<Vec<DimensionField>> {
    let Some(part) = doc.parts.get(definition) else {
        if doc.assemblies.contains_key(definition) {
            return Ok(Vec::new());
        }
        return Err(invalid("Feature dimension owner is missing"));
    };
    part.features
        .iter()
        .map(dimensions::fields)
        .collect::<Result<Vec<_>>>()
        .map(|groups| groups.into_iter().flatten().collect())
}
fn validate_dimensions(
    part: &Part,
    overrides: &BTreeMap<Id, BTreeMap<String, Expr>>,
) -> Result<()> {
    for (id, fields) in overrides {
        let feature = part
            .features
            .iter()
            .find(|feature| &feature.id == id)
            .ok_or_else(|| invalid(format!("Unknown dimensional feature {id} in {}", part.name)))?;
        let available = dimensions::fields(feature)?
            .into_iter()
            .map(|field| (field.path.clone(), field))
            .collect::<BTreeMap<_, _>>();
        let operation = serde_json::to_value(&feature.operation)?;
        for (path, expression) in fields {
            dimensions::resolve_pointer(&operation, path)
                .map_err(|error| invalid(format!("{} {path}: {error}", feature.name)))?;
            let field=available.get(path).ok_or_else(||invalid(format!(
                    "{} no longer has dimensional field {path}; remove or replace its configuration override",
                    feature.name
                )))?;
            dimensions::validate_source(field, expression)
                .map_err(|error| invalid(format!("{} {path}: {error}", feature.name)))?;
        }
    }
    Ok(())
}
fn dimension_column(key: &str) -> Result<(&str, &str)> {
    let (feature, path) = key.split_once(":/").ok_or_else(|| {
        invalid("Dimension columns require dimension:FEATURE_ID:/operation/field")
    })?;
    if feature.is_empty() || path.is_empty() {
        return Err(invalid(
            "Dimension column feature identity and field cannot be empty",
        ));
    }
    // Keep the initial slash consumed by the separator above.
    Ok((feature, &key[feature.len() + 1..]))
}
pub fn validate(doc: &Document) -> Result<()> {
    for definition in doc.parts.keys().chain(doc.assemblies.keys()) {
        let configurations = definitions(doc, definition);
        let mut used = BTreeSet::new();
        for (key, c) in &configurations {
            if !supported_version(c.version)
                || c.id != *key
                || key.is_empty()
                || key.len() > 256
                || key
                    .chars()
                    .any(|c| c.is_control() || matches!(c, '/' | '\\' | ':'))
                || c.name.trim().is_empty()
                || !used.insert(c.name.to_lowercase())
            {
                return Err(invalid(
                    "Invalid identity/version or duplicate configuration name",
                ));
            }
            let resolved = resolve(doc, definition, key)?;
            let o = &resolved.overrides;
            for pose in c.overrides.component_placements.values() {
                validate_pose(pose)?;
            }
            for seed in c.overrides.mate_curve_seeds.values() {
                if !seed.is_finite() || !(0.0..=1.0).contains(seed) {
                    return Err(invalid(
                        "Configured free curve mate seeds must be finite fractions in 0..1",
                    ));
                }
            }
            if o.bom.name == Some(BomName::Custom)
                && !o
                    .bom
                    .part_number
                    .as_ref()
                    .is_some_and(|n| !n.trim().is_empty())
            {
                return Err(invalid("Custom BOM naming requires a part number"));
            }
            if let Some(p) = doc.parts.get(definition) {
                require_keys(
                    o.parameters.keys(),
                    |id| p.parameters.contains_key(id),
                    "parameter",
                )?;
                require_keys(
                    o.features.keys(),
                    |id| p.features.iter().any(|f| f.id == id),
                    "feature",
                )?;
                validate_dimensions(p, &o.feature_dimensions)?;
                counts::validate(doc, definition, o)?;
                sketch_dimensions::validate(p, &o.sketch_dimensions)?;
                require_keys(
                    o.sketches.keys(),
                    |id| p.sketches.contains_key(id),
                    "sketch",
                )?;
                if !o.components.is_empty()
                    || !o.assembly_welds.is_empty()
                    || !o.rigid_groups.is_empty()
                    || !o.mates.is_empty()
                    || !o.component_configurations.is_empty()
                    || !o.component_patterns.is_empty()
                    || !o.component_placements.is_empty()
                    || !o.mate_curve_seeds.is_empty()
                {
                    return Err(invalid("Component/mate overrides require an assembly"));
                }
                if let Some(material) = &o.material
                    && !doc.materials.contains_key(material)
                {
                    return Err(invalid("Configured material is missing"));
                }
                if let Some(finish) = &o.finish
                    && !doc.finishes.contains_key(finish)
                {
                    return Err(invalid("Configured surface finish is missing"));
                }
            } else if let Some(a) = doc.assemblies.get(definition) {
                validate_assembly_placements(a, o)?;
                require_keys(
                    o.assembly_welds.keys(),
                    |id| a.weld_beads.contains_key(id),
                    "assembly weld",
                )?;
                require_keys(
                    o.rigid_groups.keys(),
                    |id| a.rigid_groups.contains_key(id),
                    "rigid group",
                )?;
                require_keys(
                    o.parameters.keys(),
                    |id| a.parameters.contains_key(id),
                    "assembly parameter",
                )?;
                require_keys(
                    o.component_patterns.keys(),
                    |id| a.component_patterns.contains_key(id),
                    "component pattern",
                )?;
                require_keys(
                    o.components.keys(),
                    |id| a.instances.contains_key(id),
                    "component",
                )?;
                require_keys(
                    o.mates.keys(),
                    |id| {
                        a.mates.iter().any(|m| m.id == id)
                            || a.component_patterns
                                .values()
                                .any(|p| p.mates.iter().any(|m| m.mate == id))
                    },
                    "mate",
                )?;
                if !o.features.is_empty()
                    || !o.feature_dimensions.is_empty()
                    || !o.sketch_dimensions.is_empty()
                    || !o.sketches.is_empty()
                    || o.material.is_some()
                    || o.finish.is_some()
                {
                    return Err(invalid(
                        "Feature/sketch/material/finish overrides require a part",
                    ));
                }
                for (id, config) in &o.component_configurations {
                    let instance = a
                        .instances
                        .get(id)
                        .ok_or_else(|| invalid("Configured component is missing"))?;
                    if let Some(config) = config
                        && !exists(doc, &instance.definition, config)
                    {
                        return Err(invalid("Selected child configuration is missing"));
                    }
                }
            }
        }
    }
    for (definition, configuration) in &doc.active_configurations {
        if !exists(doc, definition, configuration) {
            return Err(invalid("The selected named configuration is missing"));
        }
    }
    Ok(())
}
fn validate_pose(pose: &PoseOverride) -> Result<()> {
    if pose
        .translation
        .iter()
        .chain(&pose.rotation_deg)
        .flatten()
        .any(|value| !value.is_finite())
    {
        return Err(invalid(
            "Component placement coordinates must be finite millimetres/degrees",
        ));
    }
    let mut placement = crate::Placement::default();
    pose.apply(&mut placement);
    if placement.matrix().iter().any(|value| !value.is_finite()) {
        return Err(invalid(
            "Component placement does not produce a finite rigid frame",
        ));
    }
    Ok(())
}
fn validate_assembly_placements(assembly: &Assembly, overrides: &Overrides) -> Result<()> {
    let generated = crate::component_pattern::generated(assembly);
    for (id, pose) in &overrides.component_placements {
        validate_pose(pose)?;
        let instance = assembly
            .instances
            .get(id)
            .ok_or_else(|| invalid(format!("Configured placement component {id} is missing")))?;
        if generated.contains(id) {
            return Err(invalid(format!(
                "{} is driven by a component pattern; configure its seed or pattern instead",
                instance.name
            )));
        }
        if instance.translation_expressions.is_some()
            && pose.translation.iter().any(Option::is_some)
        {
            return Err(invalid(format!(
                "{} translation is expression-driven; configure its driving parameters instead of numeric placement",
                instance.name
            )));
        }
        let mut effective = instance.placement.clone();
        pose.apply(&mut effective);
        if effective.matrix().iter().any(|value| !value.is_finite()) {
            return Err(invalid(format!(
                "{} configured placement has a non-finite rigid frame",
                instance.name
            )));
        }
    }
    for (id, seed) in &overrides.mate_curve_seeds {
        if !seed.is_finite() || !(0.0..=1.0).contains(seed) {
            return Err(invalid(
                "Configured free curve mate seeds must be finite fractions in 0..1",
            ));
        }
        let mate = assembly
            .mates
            .iter()
            .find(|mate| &mate.id == id)
            .ok_or_else(|| invalid(format!("Configured curve mate {id} is missing")))?;
        if !matches!(
            mate.relationship
                .as_deref()
                .and_then(crate::assembly_geometry::Relationship::curve_position),
            Some(crate::assembly_geometry::CurvePosition::Free { .. })
        ) {
            return Err(invalid(format!(
                "Mate {id} has no free Path/Slot/Cam parameter; hard fractions cannot be overridden as placement seeds"
            )));
        }
    }
    Ok(())
}
/// Atomically author solved placement hints in an explicit source scope.
/// Expressions continue to own translation; callers supply only numeric pose fields
/// that are not expression-owned. Named edits never modify base or sibling seeds.
pub fn set_component_placements(
    doc: &mut Document,
    assembly: &str,
    scope: &PlacementScope,
    poses: &BTreeMap<Id, PoseOverride>,
    curve_seeds: &BTreeMap<Id, f64>,
) -> Result<()> {
    let mut candidate = doc.clone();
    let source = candidate
        .assemblies
        .get(assembly)
        .ok_or_else(|| invalid("Placement assembly is missing"))?;
    let proposed = Overrides {
        component_placements: poses.clone(),
        mate_curve_seeds: curve_seeds.clone(),
        ..Overrides::default()
    };
    validate_assembly_placements(source, &proposed)?;
    match scope {
        PlacementScope::Base => {
            let source = candidate
                .assemblies
                .get_mut(assembly)
                .ok_or_else(|| invalid("Placement assembly is missing"))?;
            for (id, pose) in poses {
                pose.apply(
                    &mut source
                        .instances
                        .get_mut(id)
                        .ok_or_else(|| invalid("Placement component is missing"))?
                        .placement,
                );
            }
            apply_curve_seeds(source, curve_seeds)?;
        }
        PlacementScope::Named(id) => {
            let source = candidate
                .assemblies
                .get_mut(assembly)
                .ok_or_else(|| invalid("Placement assembly is missing"))?;
            let configuration = source
                .configurations
                .get_mut(id)
                .ok_or_else(|| invalid("Placement configuration is missing"))?;
            configuration.version = CONTRACT_VERSION;
            for (instance, pose) in poses {
                configuration
                    .overrides
                    .component_placements
                    .entry(instance.clone())
                    .or_default()
                    .merge(pose);
            }
            configuration
                .overrides
                .mate_curve_seeds
                .extend(curve_seeds.clone());
        }
    }
    candidate.validate()?;
    *doc = candidate;
    Ok(())
}
fn apply_curve_seeds(assembly: &mut Assembly, seeds: &BTreeMap<Id, f64>) -> Result<()> {
    for (id, value) in seeds {
        let mate = assembly
            .mates
            .iter_mut()
            .find(|mate| &mate.id == id)
            .ok_or_else(|| invalid("Configured curve mate is missing"))?;
        let position =
            mate.relationship
                .as_deref_mut()
                .and_then(|relationship| match relationship {
                    crate::assembly_geometry::Relationship::Path { position, .. }
                    | crate::assembly_geometry::Relationship::Slot { position, .. }
                    | crate::assembly_geometry::Relationship::Cam { position, .. } => {
                        Some(position)
                    }
                    _ => None,
                });
        if let Some(crate::assembly_geometry::CurvePosition::Free { seed }) = position {
            *seed = *value;
        } else {
            return Err(invalid(
                "A curve placement seed cannot replace a hard mate fraction",
            ));
        }
    }
    Ok(())
}
fn apply_part(part: &mut Part, o: &Overrides) -> Result<()> {
    validate_dimensions(part, &o.feature_dimensions)?;
    sketch_dimensions::apply(part, &o.sketch_dimensions)?;
    for feature in &mut part.features {
        if let Some(fields) = o.feature_dimensions.get(&feature.id) {
            let available = dimensions::fields(feature)?
                .into_iter()
                .map(|field| (field.path.clone(), field))
                .collect::<BTreeMap<_, _>>();
            let mut operation = serde_json::to_value(&feature.operation)?;
            for (path, expression) in fields {
                let field = available
                    .get(path)
                    .ok_or_else(|| invalid("The configured field no longer exists"))?;
                if field.kind == dimensions::FieldKind::Count {
                    // Counts need the complete configured parameter context, so
                    // they are installed in a second stage by counts::apply.
                    continue;
                }
                let (pointer, _) = dimensions::resolve_pointer(&operation, path)?;
                let value = operation.pointer_mut(&pointer).ok_or_else(|| {
                    invalid(format!("{} has no dimensional field {path}", feature.name))
                })?;
                *value = dimensions::authored_value(field, expression)?;
            }
            feature.operation = serde_json::from_value(operation)?;
        }
    }
    for (id, e) in &o.parameters {
        part.parameters
            .get_mut(id)
            .ok_or_else(|| invalid("Configured parameter is missing"))?
            .expression = e.clone();
    }
    for f in &mut part.features {
        if let Some(s) = o.features.get(&f.id) {
            f.suppressed = *s;
        }
        if let crate::Operation::Profile {
            profile: crate::Profile::Sketch { sketch },
            ..
        } = &f.operation
            && let Some(s) = o.sketches.get(sketch)
        {
            f.suppressed = *s;
        }
    }
    if let Some(finish) = &o.finish {
        part.finish = finish.clone();
    }
    if let Some(material) = &o.material {
        part.material = material.clone();
    }
    part.properties.extend(o.properties.clone());
    let mut out = Vec::new();
    for id in &part.outputs {
        if let Some(feature) = part.features.iter().find(|f| &f.id == id) {
            if !feature.suppressed {
                out.push(id.clone());
            } else {
                let mut next = feature.operation.consumed_outputs();
                let mut seen = BTreeSet::new();
                while let Some(input) = next.pop() {
                    if input.definition != part.id || !seen.insert(input.node.clone()) {
                        continue;
                    }
                    if let Some(f) = part.features.iter().find(|f| f.id == input.node) {
                        if f.suppressed {
                            next.extend(f.operation.consumed_outputs());
                        } else if f.operation.produces_solid() || f.operation.is_surface() {
                            out.push(f.id.clone());
                        }
                    }
                }
            }
        }
    }
    out.sort();
    out.dedup();
    part.outputs = out;
    for feature in part.features.iter().filter(|f| !f.suppressed) {
        for dependency in part.feature_dependencies(feature) {
            if dependency.definition == part.id
                && let Some(source) = part.features.iter().find(|f| f.id == dependency.node)
                && source.suppressed
                && (matches!(source.operation, crate::Operation::Profile { .. })
                    || source.operation.is_reference()
                    || source.operation.consumed_outputs().is_empty())
            {
                return Err(invalid(format!(
                    "{} requires suppressed {}; suppress the dependent feature or restore its source",
                    feature.name, source.name
                )));
            }
        }
    }
    Ok(())
}
fn apply_assembly(assembly: &mut Assembly, o: &Overrides) -> Result<()> {
    validate_assembly_placements(assembly, o)?;
    assembly.properties.extend(o.properties.clone());
    for (id, e) in &o.parameters {
        assembly
            .parameters
            .get_mut(id)
            .ok_or_else(|| invalid("Configured assembly parameter is missing"))?
            .expression = e.clone();
    }
    for (id, config) in &o.component_configurations {
        if let Some(i) = assembly.instances.get_mut(id) {
            i.configuration = config.clone();
        }
    }
    for (id, pose) in &o.component_placements {
        pose.apply(
            &mut assembly
                .instances
                .get_mut(id)
                .ok_or_else(|| invalid("Configured placement component is missing"))?
                .placement,
        );
    }
    apply_curve_seeds(assembly, &o.mate_curve_seeds)?;
    for (id, suppressed) in &o.assembly_welds {
        assembly
            .weld_beads
            .get_mut(id)
            .ok_or_else(|| invalid("Configured assembly weld is missing"))?
            .suppressed = *suppressed;
    }
    for (id, suppressed) in &o.rigid_groups {
        assembly
            .rigid_groups
            .get_mut(id)
            .ok_or_else(|| invalid("Configured rigid group is missing"))?
            .suppressed = *suppressed;
    }
    assembly
        .instances
        .retain(|id, _| !o.components.get(id).copied().unwrap_or(false));
    assembly.mates.retain(|m| {
        !o.mates.get(&m.id).copied().unwrap_or(false)
            && m.participants()
                .iter()
                .all(|id| assembly.instances.contains_key(*id))
    });
    Ok(())
}
pub fn apply_definition(
    doc: &mut Document,
    definition: &str,
    configuration: &str,
) -> Result<Resolved> {
    let c = resolve(doc, definition, configuration)?;
    counts::apply_parameters(doc, definition, &c.overrides.parameters)?;
    apply_overrides(doc, definition, &c.overrides)?;
    counts::apply(doc, definition, &c.overrides)?;
    if let Some(a) = doc.assemblies.get(definition).cloned() {
        let prepared = crate::component_pattern::prepare(doc, &a, &c.overrides)?;
        doc.assemblies.insert(definition.into(), prepared);
    }
    Ok(c)
}
fn apply_overrides(doc: &mut Document, definition: &str, overrides: &Overrides) -> Result<()> {
    if let Some(p) = doc.parts.get_mut(definition) {
        apply_part(p, overrides)
    } else if let Some(a) = doc.assemblies.get_mut(definition) {
        apply_assembly(a, overrides)
    } else {
        Err(invalid("Configuration definition is missing"))
    }
}
pub fn effective(doc: &Document) -> Result<Document> {
    let mut d = doc.clone();
    let mut selected = BTreeMap::new();
    // Stage every selected parameter scope before any count expression. These
    // parameter-only writes do not materialize recursive configuration variants.
    for (definition, key) in &doc.active_configurations {
        let resolved = resolve(doc, definition, key)?;
        counts::apply_parameters(&mut d, definition, &resolved.overrides.parameters)?;
        selected.insert(definition.clone(), resolved.overrides);
    }
    for (definition, overrides) in &selected {
        apply_overrides(&mut d, definition, overrides)?;
    }
    for (definition, overrides) in &selected {
        counts::apply(&mut d, definition, overrides)?;
    }
    for definition in doc.assemblies.keys() {
        let overrides = selected.get(definition).cloned().unwrap_or_default();
        let prepared =
            crate::component_pattern::prepare(&d, &d.assemblies[definition], &overrides)?;
        d.assemblies.insert(definition.clone(), prepared);
    }
    Ok(d)
}
/// Re-materialize from authoritative base definitions, preserving other selected scopes.
pub fn source_variant(doc: &Document, definition: &str, key: Option<&str>) -> Document {
    let mut d = doc.clone();
    if let Some(key) = key {
        d.active_configurations
            .insert(definition.into(), key.into());
    } else {
        d.active_configurations.remove(definition);
    }
    d
}
pub fn variant(doc: &Document, definition: &str, key: Option<&str>) -> Result<Document> {
    effective(&source_variant(doc, definition, key))
}
pub fn effective_part(doc: &Document, definition: &str, key: Option<&str>) -> Result<Part> {
    let d = variant(doc, definition, key)?;
    d.parts
        .get(definition)
        .cloned()
        .ok_or_else(|| invalid("Configured part is missing"))
}
pub fn selected<'a>(
    doc: &'a Document,
    definition: &str,
    explicit: Option<&'a Id>,
) -> Option<&'a Id> {
    explicit.or_else(|| doc.active_configurations.get(definition))
}
pub fn key(definition: &str, configuration: Option<&str>) -> Id {
    configuration
        .map(|c| format!("{definition}@{c}"))
        .unwrap_or_else(|| definition.into())
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Table {
    pub version: u32,
    pub definition: Id,
    pub configurations: Vec<Definition>,
}
pub fn table(doc: &Document, definition: &str) -> Table {
    Table {
        version: CONTRACT_VERSION,
        definition: definition.into(),
        configurations: definitions(doc, definition).into_values().collect(),
    }
}
pub fn put(doc: &mut Document, definition: &str, configuration: Definition) -> Result<()> {
    if let Some(part) = doc.parts.get_mut(definition) {
        part.configurations.remove(&configuration.id);
        part.configuration_details
            .insert(configuration.id.clone(), configuration);
    } else if let Some(assembly) = doc.assemblies.get_mut(definition) {
        assembly
            .configurations
            .insert(configuration.id.clone(), configuration);
    } else {
        return Err(invalid("Configuration owner is missing"));
    }
    Ok(())
}
pub fn remove(doc: &mut Document, definition: &str, configuration: &str) -> Result<()> {
    if doc
        .active_configurations
        .get(definition)
        .is_some_and(|c| c == configuration)
    {
        return Err(invalid(
            "Select another configuration before deleting the active one",
        ));
    }
    if definitions(doc, definition)
        .values()
        .any(|c| c.parent.as_deref() == Some(configuration))
    {
        return Err(invalid(
            "Reparent derived configurations before deleting their parent",
        ));
    }
    if let Some(p) = doc.parts.get_mut(definition) {
        p.configurations.remove(configuration);
        p.configuration_details.remove(configuration);
    } else if let Some(a) = doc.assemblies.get_mut(definition) {
        a.configurations.remove(configuration);
    } else {
        return Err(invalid("Configuration owner is missing"));
    }
    Ok(())
}
fn quote(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\"\""))
}
pub fn csv(table: &Table) -> Result<String> {
    let mut keys = BTreeSet::new();
    for c in &table.configurations {
        for key in c.overrides.parameters.keys() {
            keys.insert(format!("parameter:{key}"));
        }
        for (feature, fields) in &c.overrides.feature_dimensions {
            for path in fields.keys() {
                keys.insert(format!("dimension:{feature}:{path}"));
            }
        }
        for (sketch, dimensions) in &c.overrides.sketch_dimensions {
            for constraint in dimensions.keys() {
                keys.insert(format!("sketch-dimension:{sketch}:{constraint}"));
            }
        }
        for key in c.overrides.features.keys() {
            keys.insert(format!("feature:{key}"));
        }
        for key in c.overrides.sketches.keys() {
            keys.insert(format!("sketch:{key}"));
        }
        for key in c.overrides.components.keys() {
            keys.insert(format!("component:{key}"));
        }
        for key in c.overrides.component_patterns.keys() {
            keys.insert(format!("pattern:{key}"));
        }
        for key in c.overrides.mates.keys() {
            keys.insert(format!("mate:{key}"));
        }
        for key in c.overrides.assembly_welds.keys() {
            keys.insert(format!("weld:{key}"));
        }
        for key in c.overrides.rigid_groups.keys() {
            keys.insert(format!("group:{key}"));
        }
        for key in c.overrides.component_configurations.keys() {
            keys.insert(format!("configuration:{key}"));
        }
        for (key, pose) in &c.overrides.component_placements {
            for (axis, field) in ["x", "y", "z", "rx", "ry", "rz"].iter().enumerate() {
                let value = if axis < 3 {
                    pose.translation[axis]
                } else {
                    pose.rotation_deg[axis - 3]
                };
                if value.is_some() {
                    keys.insert(format!("placement:{key}:{field}"));
                }
            }
        }
        for key in c.overrides.mate_curve_seeds.keys() {
            keys.insert(format!("curve-seed:{key}"));
        }
        for key in c.overrides.properties.keys() {
            keys.insert(format!("property:{key}"));
        }
    }
    let mut headers = vec![
        "id".into(),
        "name".into(),
        "parent".into(),
        "description".into(),
        "material".into(),
        "finish".into(),
        "bom.exclude".into(),
        "bom.name".into(),
        "bom.part_number".into(),
        "bom.description".into(),
    ];
    headers.extend(keys);
    let mut out = headers
        .iter()
        .map(|s| quote(s))
        .collect::<Vec<_>>()
        .join(",")
        + "\n";
    for c in &table.configurations {
        let mut cells = Vec::new();
        for h in &headers {
            cells.push(quote(&cell(c, h)?));
        }
        out.push_str(&(cells.join(",") + "\n"));
    }
    Ok(out)
}
fn cell(c: &Definition, h: &str) -> Result<String> {
    let o = &c.overrides;
    Ok(match h {
        "id" => c.id.clone(),
        "name" => c.name.clone(),
        "parent" => c.parent.clone().unwrap_or_default(),
        "description" => c.description.clone(),
        "material" => o.material.clone().unwrap_or_default(),
        "finish" => o.finish.clone().unwrap_or_default(),
        "bom.exclude" => o.bom.exclude.map(|b| b.to_string()).unwrap_or_default(),
        "bom.name" => o
            .bom
            .name
            .as_ref()
            .map(|n| {
                match n {
                    BomName::Document => "document",
                    BomName::Configuration => "configuration",
                    BomName::Custom => "custom",
                }
                .into()
            })
            .unwrap_or_default(),
        "bom.part_number" => o.bom.part_number.clone().unwrap_or_default(),
        "bom.description" => o.bom.description.clone().unwrap_or_default(),
        _ => {
            let (kind, key) = h
                .split_once(':')
                .ok_or_else(|| invalid("Unknown design-table column"))?;
            match kind {
                "parameter" => o
                    .parameters
                    .get(key)
                    .map(|e| e.0.clone())
                    .unwrap_or_default(),
                "dimension" => {
                    let (feature, path) = dimension_column(key)?;
                    o.feature_dimensions
                        .get(feature)
                        .and_then(|fields| fields.get(path))
                        .map(|e| e.0.clone())
                        .unwrap_or_default()
                }
                "sketch-dimension" => {
                    let (sketch, constraint) = sketch_dimensions::column(key)?;
                    o.sketch_dimensions
                        .get(sketch)
                        .and_then(|dimensions| dimensions.get(constraint))
                        .map(|expression| expression.0.clone())
                        .unwrap_or_default()
                }
                "feature" => o
                    .features
                    .get(key)
                    .map(|s| s.to_string())
                    .unwrap_or_default(),
                "sketch" => o
                    .sketches
                    .get(key)
                    .map(|s| s.to_string())
                    .unwrap_or_default(),
                "component" => o
                    .components
                    .get(key)
                    .map(|s| s.to_string())
                    .unwrap_or_default(),
                "pattern" => o
                    .component_patterns
                    .get(key)
                    .map(|s| s.to_string())
                    .unwrap_or_default(),
                "mate" => o.mates.get(key).map(|s| s.to_string()).unwrap_or_default(),
                "weld" => o
                    .assembly_welds
                    .get(key)
                    .map(|s| s.to_string())
                    .unwrap_or_default(),
                "group" => o
                    .rigid_groups
                    .get(key)
                    .map(|value| value.to_string())
                    .unwrap_or_default(),
                "configuration" => o
                    .component_configurations
                    .get(key)
                    .map(|v| v.clone().unwrap_or_else(|| "default".into()))
                    .unwrap_or_default(),
                "placement" => {
                    let (instance, axis) = placement_column(key)?;
                    o.component_placements
                        .get(instance)
                        .and_then(|pose| {
                            if axis < 3 {
                                pose.translation[axis]
                            } else {
                                pose.rotation_deg[axis - 3]
                            }
                        })
                        .map(|v| format!("{v} {}", if axis < 3 { "mm" } else { "deg" }))
                        .unwrap_or_default()
                }
                "curve-seed" => o
                    .mate_curve_seeds
                    .get(key)
                    .map(ToString::to_string)
                    .unwrap_or_default(),
                "property" => o.properties.get(key).cloned().unwrap_or_default(),
                _ => return Err(invalid("Unknown design-table column")),
            }
        }
    })
}
fn parse_csv(text: &str) -> Result<Vec<Vec<String>>> {
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut cell = String::new();
    let mut chars = text.chars().peekable();
    let mut quoted = false;
    let mut closed = false;
    while let Some(c) = chars.next() {
        if quoted {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    cell.push('"');
                } else {
                    quoted = false;
                    closed = true;
                }
            } else {
                cell.push(c);
            }
        } else {
            match c {
                '"' if cell.is_empty() && !closed => quoted = true,
                ',' => {
                    row.push(std::mem::take(&mut cell));
                    closed = false;
                }
                '\n' => {
                    row.push(std::mem::take(&mut cell));
                    rows.push(std::mem::take(&mut row));
                    closed = false;
                }
                '\r' if chars.peek() == Some(&'\n') => {}
                _ if closed => return Err(invalid("Unexpected text after quoted CSV cell")),
                _ => cell.push(c),
            }
        }
    }
    if quoted {
        return Err(invalid("Unterminated quoted CSV cell"));
    }
    if !cell.is_empty() || !row.is_empty() {
        row.push(cell);
        rows.push(row);
    }
    Ok(rows)
}
pub fn from_csv(definition: &str, text: &str) -> Result<Table> {
    let mut rows = parse_csv(text)?;
    if rows.is_empty() {
        return Err(invalid("The design table is empty"));
    }
    let headers = rows.remove(0);
    if !headers.iter().any(|h| h == "name") {
        return Err(invalid("A design table requires a name column"));
    }
    let mut unique = BTreeSet::new();
    if headers.iter().any(|h| !unique.insert(h)) {
        return Err(invalid("Duplicate design-table columns"));
    }
    let mut configurations = Vec::new();
    for (index, row) in rows.into_iter().enumerate() {
        if row.iter().all(|c| c.trim().is_empty()) {
            continue;
        }
        if row.len() != headers.len() {
            return Err(invalid(format!(
                "Row {} has the wrong number of columns",
                index + 2
            )));
        }
        let mut c = Definition::new("Configuration");
        for (h, value) in headers.iter().zip(row) {
            if value.is_empty() {
                continue;
            }
            set_cell(&mut c, h, &value)?;
        }
        configurations.push(c);
    }
    Ok(Table {
        version: CONTRACT_VERSION,
        definition: definition.into(),
        configurations,
    })
}
fn boolean(value: &str) -> Result<bool> {
    match value.trim().to_lowercase().as_str() {
        "true" | "yes" | "1" | "suppressed" => Ok(true),
        "false" | "no" | "0" | "resolved" => Ok(false),
        _ => Err(invalid("Suppression/exclusion cells require true or false")),
    }
}
fn placement_column(key: &str) -> Result<(&str, usize)> {
    let (instance, field) = key
        .rsplit_once(':')
        .ok_or_else(|| invalid("Placement columns require placement:INSTANCE_ID:x/y/z/rx/ry/rz"))?;
    if instance.is_empty() {
        return Err(invalid("A placement component ID cannot be empty"));
    }
    let axis = ["x", "y", "z", "rx", "ry", "rz"]
        .iter()
        .position(|name| *name == field)
        .ok_or_else(|| invalid("Placement field must be x, y, z, rx, ry, or rz"))?;
    Ok((instance, axis))
}
/// Parse an exact fixed pose cell. Unitless values use mm/degrees; equations
/// belong to driving assembly parameters rather than hidden placement state.
pub fn placement_value(text: &str, axis: usize) -> Result<f64> {
    if axis >= 6 {
        return Err(invalid("Unknown placement coordinate"));
    }
    let quantity = crate::expression::evaluate(text, |_| {
        Err(invalid(
            "Placement cells are fixed poses; configure driving parameters for equations",
        ))
    })?;
    let dimension = if axis < 3 {
        crate::Dimension::Length
    } else {
        crate::Dimension::Angle
    };
    let value = if quantity.matches(crate::Dimension::Scalar) {
        quantity.value
    } else if quantity.matches(dimension) {
        if axis < 3 {
            quantity.value
        } else {
            quantity.value.to_degrees()
        }
    } else {
        return Err(invalid(
            "Placement cells require length or angle units appropriate to the field",
        ));
    };
    if !value.is_finite() {
        return Err(invalid("Placement coordinates must be finite"));
    }
    Ok(value)
}
fn set_cell(c: &mut Definition, h: &str, value: &str) -> Result<()> {
    match h {
        "id" => c.id = value.into(),
        "name" => c.name = value.into(),
        "parent" => c.parent = Some(value.into()),
        "description" => c.description = value.into(),
        "material" => c.overrides.material = Some(value.into()),
        "finish" => c.overrides.finish = Some(value.into()),
        "bom.exclude" => c.overrides.bom.exclude = Some(boolean(value)?),
        "bom.name" => {
            c.overrides.bom.name = Some(match value {
                "document" => BomName::Document,
                "configuration" => BomName::Configuration,
                "custom" => BomName::Custom,
                _ => {
                    return Err(invalid(
                        "BOM name must be document, configuration, or custom",
                    ));
                }
            })
        }
        "bom.part_number" => c.overrides.bom.part_number = Some(value.into()),
        "bom.description" => c.overrides.bom.description = Some(value.into()),
        _ => {
            let (kind, key) = h
                .split_once(':')
                .ok_or_else(|| invalid(format!("Unknown design-table column {h}")))?;
            if key.is_empty() {
                return Err(invalid("A design-table reference ID cannot be empty"));
            }
            let o = &mut c.overrides;
            match kind {
                "parameter" => {
                    o.parameters.insert(key.into(), value.into());
                }
                "dimension" => {
                    let (feature, path) = dimension_column(key)?;
                    o.feature_dimensions
                        .entry(feature.into())
                        .or_default()
                        .insert(path.into(), value.into());
                }
                "sketch-dimension" => {
                    let (sketch, constraint) = sketch_dimensions::column(key)?;
                    o.sketch_dimensions
                        .entry(sketch.into())
                        .or_default()
                        .insert(constraint.into(), value.into());
                }
                "feature" => {
                    o.features.insert(key.into(), boolean(value)?);
                }
                "sketch" => {
                    o.sketches.insert(key.into(), boolean(value)?);
                }
                "component" => {
                    o.components.insert(key.into(), boolean(value)?);
                }
                "pattern" => {
                    o.component_patterns.insert(key.into(), boolean(value)?);
                }
                "mate" => {
                    o.mates.insert(key.into(), boolean(value)?);
                }
                "weld" => {
                    o.assembly_welds.insert(key.into(), boolean(value)?);
                }
                "group" => {
                    o.rigid_groups.insert(key.into(), boolean(value)?);
                }
                "configuration" => {
                    o.component_configurations.insert(
                        key.into(),
                        if value == "default" {
                            None
                        } else {
                            Some(value.into())
                        },
                    );
                }
                "placement" => {
                    let (instance, axis) = placement_column(key)?;
                    let number = placement_value(value, axis)?;
                    let pose = o.component_placements.entry(instance.into()).or_default();
                    if axis < 3 {
                        pose.translation[axis] = Some(number);
                    } else {
                        pose.rotation_deg[axis - 3] = Some(number);
                    }
                }
                "curve-seed" => {
                    let seed: f64 = value
                        .parse()
                        .map_err(|_| invalid("Curve seed cells require a fraction in 0..1"))?;
                    if !seed.is_finite() || !(0.0..=1.0).contains(&seed) {
                        return Err(invalid(
                            "Curve seed cells require a finite fraction in 0..1",
                        ));
                    }
                    o.mate_curve_seeds.insert(key.into(), seed);
                }
                "property" => {
                    o.properties.insert(key.into(), value.into());
                }
                _ => return Err(invalid(format!("Unknown design-table column {h}"))),
            }
        }
    }
    Ok(())
}
fn named_id(value: &str, choices: &[(Id, String)], label: &str) -> Result<Id> {
    if choices.iter().any(|(id, _)| id == value) {
        return Ok(value.into());
    }
    let matches = choices
        .iter()
        .filter(|(_, name)| name.eq_ignore_ascii_case(value))
        .collect::<Vec<_>>();
    if matches.len() == 1 {
        Ok(matches[0].0.clone())
    } else {
        Err(invalid(format!("Unknown or ambiguous {label} {value}")))
    }
}
fn normalize_map<T>(
    map: &mut BTreeMap<Id, T>,
    choices: &[(Id, String)],
    label: &str,
) -> Result<()> {
    let previous = std::mem::take(map);
    for (key, value) in previous {
        let id = named_id(&key, choices, label)?;
        if map.insert(id, value).is_some() {
            return Err(invalid(format!("Repeated {label} override")));
        }
    }
    Ok(())
}
pub fn import(doc: &mut Document, table: Table, replace: bool) -> Result<()> {
    let mut candidate = doc.clone();
    import_into(&mut candidate, table, replace)?;
    validate(&candidate)?;
    *doc = candidate;
    Ok(())
}
fn import_into(doc: &mut Document, mut table: Table, replace: bool) -> Result<()> {
    if !supported_version(table.version) {
        return Err(invalid("Unsupported design-table version"));
    }
    let owner = table.definition.clone();
    let existing = definitions(doc, &owner);
    let mut identities = BTreeMap::new();
    for configuration in &mut table.configurations {
        if !existing.contains_key(&configuration.id)
            && let Some(previous) = existing
                .values()
                .find(|c| c.name.eq_ignore_ascii_case(&configuration.name))
        {
            identities.insert(configuration.id.clone(), previous.id.clone());
            configuration.id = previous.id.clone();
        }
    }
    let choices = existing
        .values()
        .chain(table.configurations.iter())
        .map(|c| (c.id.clone(), c.name.clone()))
        .collect::<BTreeMap<_, _>>()
        .into_iter()
        .collect::<Vec<_>>();
    for c in &mut table.configurations {
        if !supported_version(c.version) {
            return Err(invalid("Unsupported configuration version"));
        }
        c.version = CONTRACT_VERSION;
        if let Some(parent) = &mut c.parent {
            *parent = if let Some(id) = identities.get(parent) {
                id.clone()
            } else {
                named_id(parent, &choices, "parent configuration")?
            };
        }
        let o = &mut c.overrides;
        let parameter_choices = doc
            .parts
            .get(&owner)
            .map(|p| {
                p.parameters
                    .iter()
                    .map(|(id, p)| (id.clone(), p.name.clone()))
                    .collect::<Vec<_>>()
            })
            .or_else(|| {
                doc.assemblies.get(&owner).map(|a| {
                    a.parameters
                        .iter()
                        .map(|(id, p)| (id.clone(), p.name.clone()))
                        .collect()
                })
            })
            .ok_or_else(|| invalid("Design-table owner is missing"))?;
        normalize_map(&mut o.parameters, &parameter_choices, "parameter")?;
        if let Some(p) = doc.parts.get(&owner) {
            normalize_map(
                &mut o.feature_dimensions,
                &p.features
                    .iter()
                    .map(|f| (f.id.clone(), f.name.clone()))
                    .collect::<Vec<_>>(),
                "feature dimension owner",
            )?;
            validate_dimensions(p, &o.feature_dimensions)?;
            normalize_map(
                &mut o.sketch_dimensions,
                &p.sketches
                    .values()
                    .map(|sketch| (sketch.id.clone(), sketch.name.clone()))
                    .collect::<Vec<_>>(),
                "sketch dimension owner",
            )?;
            sketch_dimensions::validate(p, &o.sketch_dimensions)?;
            normalize_map(
                &mut o.features,
                &p.features
                    .iter()
                    .map(|f| (f.id.clone(), f.name.clone()))
                    .collect::<Vec<_>>(),
                "feature",
            )?;
            normalize_map(
                &mut o.sketches,
                &p.sketches
                    .values()
                    .map(|s| (s.id.clone(), s.name.clone()))
                    .collect::<Vec<_>>(),
                "sketch",
            )?;
            if let Some(finish) = &mut o.finish {
                *finish = named_id(
                    finish,
                    &doc.finishes
                        .values()
                        .map(|f| (f.id.clone(), f.name.clone()))
                        .collect::<Vec<_>>(),
                    "finish",
                )?;
            }
            if let Some(material) = &mut o.material {
                *material = named_id(
                    material,
                    &doc.materials
                        .values()
                        .map(|m| (m.id.clone(), m.name.clone()))
                        .collect::<Vec<_>>(),
                    "material",
                )?;
            }
        }
        if let Some(a) = doc.assemblies.get(&owner) {
            let instances = a
                .instances
                .values()
                .map(|i| (i.id.clone(), i.name.clone()))
                .collect::<Vec<_>>();
            normalize_map(&mut o.components, &instances, "component")?;
            normalize_map(
                &mut o.assembly_welds,
                &a.weld_beads
                    .values()
                    .map(|bead| (bead.id.clone(), bead.name.clone()))
                    .collect::<Vec<_>>(),
                "assembly weld",
            )?;
            normalize_map(
                &mut o.rigid_groups,
                &a.rigid_groups
                    .values()
                    .map(|group| (group.id.clone(), group.name.clone()))
                    .collect::<Vec<_>>(),
                "rigid group",
            )?;
            normalize_map(&mut o.component_configurations, &instances, "component")?;
            normalize_map(
                &mut o.component_placements,
                &instances,
                "placement component",
            )?;
            normalize_map(
                &mut o.mate_curve_seeds,
                &a.mates
                    .iter()
                    .map(|m| (m.id.clone(), m.kind.clone()))
                    .collect::<Vec<_>>(),
                "curve mate",
            )?;
            normalize_map(
                &mut o.component_patterns,
                &a.component_patterns
                    .values()
                    .map(|p| (p.id.clone(), p.name.clone()))
                    .collect::<Vec<_>>(),
                "component pattern",
            )?;
            for (id, configuration) in &mut o.component_configurations {
                if let Some(configuration) = configuration {
                    *configuration = named_id(
                        configuration,
                        &names(doc, &a.instances[id].definition)
                            .into_iter()
                            .collect::<Vec<_>>(),
                        "child configuration",
                    )?;
                }
            }
        }
    }
    if replace {
        if let Some(p) = doc.parts.get_mut(&owner) {
            p.configurations.clear();
            p.configuration_details.clear();
        } else if let Some(a) = doc.assemblies.get_mut(&owner) {
            a.configurations.clear();
        } else {
            return Err(invalid("Design-table owner is missing"));
        }
    }
    let mut used = BTreeSet::new();
    for configuration in table.configurations {
        if !used.insert(configuration.id.clone()) {
            return Err(invalid("Duplicate design-table configuration IDs"));
        }
        put(doc, &owner, configuration)?;
    }
    Ok(())
}
pub fn read(path: &Path, definition: &str) -> Result<Table> {
    let text = std::fs::read_to_string(path)?;
    if path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("csv"))
    {
        from_csv(definition, &text)
    } else {
        let t: Table = serde_json::from_str(&text)?;
        if t.definition != definition {
            return Err(invalid("The imported table belongs to another definition"));
        }
        Ok(t)
    }
}
pub fn write(doc: &Document, definition: &str, path: &Path) -> Result<()> {
    let t = table(doc, definition);
    let bytes = if path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("csv"))
    {
        csv(&t)?.into_bytes()
    } else {
        serde_json::to_vec_pretty(&t)?
    };
    crate::persistence::atomic_write(path, &bytes)
}
/// Produce one component's local configuration in its parent's parameter context.
pub fn scope(
    source: &Document,
    context: &Document,
    definition: &str,
    configuration: Option<&str>,
) -> Result<Document> {
    let mut d = context.clone();
    if let Some(part) = source.parts.get(definition) {
        d.parts.insert(definition.into(), part.clone());
    } else if let Some(assembly) = source.assemblies.get(definition) {
        d.assemblies.insert(definition.into(), assembly.clone());
    } else {
        return Err(invalid("The configured definition is missing"));
    }
    if let Some(configuration) = configuration {
        apply_definition(&mut d, definition, configuration)?;
        d.active_configurations
            .insert(definition.into(), configuration.into());
    } else {
        d.active_configurations.remove(definition);
        if let Some(a) = d.assemblies.get(definition).cloned() {
            let prepared = crate::component_pattern::prepare(&d, &a, &Overrides::default())?;
            d.assemblies.insert(definition.into(), prepared);
        }
    }
    Ok(d)
}
/// Native placement traversal retains every stable component path and selected configuration.
pub fn placed_parts(
    doc: &Document,
    evaluated: Option<&BTreeMap<Id, crate::assembly_geometry::EvaluatedAssembly>>,
) -> Result<Vec<crate::PlacedPart>> {
    struct PlacementState {
        matrix: nalgebra::Matrix4<f64>,
        path: Vec<Id>,
        name: String,
        excluded: bool,
        finish: Option<Id>,
    }
    struct Walker<'a> {
        source: &'a Document,
        evaluated: Option<&'a BTreeMap<Id, crate::assembly_geometry::EvaluatedAssembly>>,
        active: BTreeSet<Id>,
        parts: Vec<crate::PlacedPart>,
    }
    impl Walker<'_> {
        fn walk(
            &mut self,
            context: &Document,
            definition: &str,
            configuration: Option<Id>,
            placement: PlacementState,
        ) -> Result<()> {
            let PlacementState {
                matrix,
                path,
                name,
                excluded,
                finish,
            } = placement;
            let scoped = scope(self.source, context, definition, configuration.as_deref())?;
            check_geometry_context(self.source, &scoped, definition)?;
            let excluded = excluded
                || configuration
                    .as_ref()
                    .map(|c| resolve(self.source, definition, c))
                    .transpose()?
                    .and_then(|c| c.overrides.bom.exclude)
                    .unwrap_or(false);
            if let Some(p) = scoped.parts.get(definition) {
                self.parts.push(crate::PlacedPart {
                    path,
                    definition: definition.into(),
                    name: if name.is_empty() {
                        p.name.clone()
                    } else {
                        name
                    },
                    matrix: std::array::from_fn(|r| std::array::from_fn(|c| matrix[(r, c)])),
                    configuration: configuration.clone(),
                    mesh_key: key(definition, configuration.as_deref()),
                    bom_excluded: excluded,
                    finish: Some(finish.unwrap_or_else(|| p.finish.clone())),
                });
                return Ok(());
            }
            if !self.active.insert(definition.into()) {
                return Err(invalid("Assembly hierarchy/configuration cycle"));
            }
            let a = &scoped.assemblies[definition];
            let evaluated = self
                .evaluated
                .and_then(|e| e.get(&key(definition, configuration.as_deref())));
            for instance in a.instances.values() {
                let placement = evaluated
                    .and_then(|e| e.placements.get(&instance.id))
                    .cloned()
                    .unwrap_or_else(|| instance.placement.clone());
                let mut placement = placement;
                if evaluated.is_none()
                    && let Some(expressions) = &instance.translation_expressions
                {
                    for (axis, e) in expressions.iter().enumerate() {
                        placement.translation[axis] = crate::expression::value(
                            &scoped,
                            definition,
                            e,
                            crate::Dimension::Length,
                        )?;
                    }
                }
                let config = selected(
                    &scoped,
                    &instance.definition,
                    instance.configuration.as_ref(),
                )
                .cloned();
                let mut child_path = path.clone();
                child_path.push(instance.id.clone());
                self.walk(
                    &scoped,
                    &instance.definition,
                    config,
                    PlacementState {
                        matrix: matrix * placement.matrix(),
                        path: child_path,
                        name: instance.name.clone(),
                        excluded,
                        finish: instance.finish.clone().or_else(|| finish.clone()),
                    },
                )?;
            }
            self.active.remove(definition);
            Ok(())
        }
    }
    let effective = effective(doc)?;
    let mut w = Walker {
        source: doc,
        evaluated,
        active: BTreeSet::new(),
        parts: Vec::new(),
    };
    w.walk(
        &effective,
        &doc.root,
        doc.active_configurations.get(&doc.root).cloned(),
        PlacementState {
            matrix: nalgebra::Matrix4::identity(),
            path: Vec::new(),
            name: String::new(),
            excluded: false,
            finish: None,
        },
    )?;
    Ok(w.parts)
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Evaluated {
    pub version: u32,
    pub selected: BTreeMap<Id, Id>,
    pub definitions: BTreeMap<Id, BTreeMap<Id, Resolved>>,
    pub part_materials: BTreeMap<Id, Id>,
    pub part_properties: BTreeMap<Id, BTreeMap<String, String>>,
}
pub fn evaluated(doc: &Document) -> Result<Evaluated> {
    let effective = effective(doc)?;
    let mut definitions = BTreeMap::new();
    for definition in doc.parts.keys().chain(doc.assemblies.keys()) {
        let mut resolved = BTreeMap::new();
        for id in names(doc, definition).keys() {
            resolved.insert(id.clone(), resolve(doc, definition, id)?);
        }
        definitions.insert(definition.clone(), resolved);
    }
    Ok(Evaluated {
        version: CONTRACT_VERSION,
        selected: doc.active_configurations.clone(),
        definitions,
        part_materials: effective
            .parts
            .iter()
            .map(|(id, p)| (id.clone(), p.material.clone()))
            .collect(),
        part_properties: effective
            .parts
            .iter()
            .map(|(id, p)| (id.clone(), p.properties.clone()))
            .collect(),
    })
}

/// Part geometry is definition/configuration owned. Reject occurrence-driven external
/// equations until their context can be represented by a distinct native definition.
pub fn check_geometry_context(
    source: &Document,
    context: &Document,
    definition: &str,
) -> Result<()> {
    let expected = effective(&source_variant(
        source,
        definition,
        context
            .active_configurations
            .get(definition)
            .map(String::as_str),
    ))?;
    let value = if let Some(part) = context.parts.get(definition) {
        let mut part = part.clone();
        part.configurations.clear();
        part.configuration_details.clear();
        part.features.retain(|f| !f.suppressed);
        let sketches = part
            .features
            .iter()
            .filter_map(|f| match &f.operation {
                crate::Operation::Profile {
                    profile: crate::Profile::Sketch { sketch },
                    ..
                } => Some(sketch.clone()),
                _ => None,
            })
            .collect::<BTreeSet<_>>();
        part.sketches.retain(|id, _| sketches.contains(id));
        serde_json::to_value(part)?
    } else if let Some(assembly) = context.assemblies.get(definition) {
        let mut assembly = assembly.clone();
        assembly.configurations.clear();
        serde_json::to_value(assembly)?
    } else {
        return Ok(());
    };
    fn strings(v: &serde_json::Value, out: &mut Vec<String>) {
        match v {
            serde_json::Value::String(s) => out.push(s.clone()),
            serde_json::Value::Array(a) => {
                for v in a {
                    strings(v, out);
                }
            }
            serde_json::Value::Object(o) => {
                for v in o.values() {
                    strings(v, out);
                }
            }
            _ => {}
        }
    }
    let mut expressions = Vec::new();
    strings(&value, &mut expressions);
    // Integer operation fields no longer contain their authored expressions.
    // Include those sources in the external-equation proof so occurrence
    // contexts cannot silently reuse geometry with differently driven counts.
    if let Some(configuration) = context.active_configurations.get(definition) {
        let resolved = resolve(source, definition, configuration)?;
        expressions.extend(
            counts::expressions(source, definition, &resolved.overrides)?
                .into_iter()
                .map(|expression| expression.0),
        );
    }
    let mut pending = expressions
        .into_iter()
        .filter_map(|s| crate::expression::references(&s).ok())
        .flatten()
        .map(|name| (definition.to_string(), name))
        .collect::<Vec<_>>();
    let mut seen = BTreeSet::new();
    while let Some((owner, name)) = pending.pop() {
        let (scope, key) = name.rsplit_once("::").unwrap_or((&owner, &name));
        let scope = if context.parts.contains_key(scope) || context.assemblies.contains_key(scope) {
            scope.to_string()
        } else {
            let owners = context
                .parts
                .values()
                .filter(|p| p.name == scope)
                .map(|p| p.id.clone())
                .chain(
                    context
                        .assemblies
                        .values()
                        .filter(|a| a.name == scope)
                        .map(|a| a.id.clone()),
                )
                .collect::<Vec<_>>();
            if owners.len() != 1 {
                continue;
            }
            owners[0].clone()
        };
        if !seen.insert((scope.clone(), key.to_string())) {
            continue;
        }
        let parameters = context
            .parts
            .get(&scope)
            .map(|p| &p.parameters)
            .or_else(|| context.assemblies.get(&scope).map(|a| &a.parameters));
        let Some(parameter) =
            parameters.and_then(|p| p.get(key).or_else(|| p.values().find(|p| p.name == key)))
        else {
            continue;
        };
        let baseline = expected
            .parts
            .get(&scope)
            .map(|p| &p.parameters)
            .or_else(|| expected.assemblies.get(&scope).map(|a| &a.parameters))
            .and_then(|p| p.get(key).or_else(|| p.values().find(|p| p.name == key)));
        if scope != definition && !baseline.is_some_and(|p| p == parameter) {
            return Err(CadError::Unsupported("An occurrence configuration changes an external equation used by this component. Give the component its own explicit configuration or independent definition; occurrence-driven geometry is not yet supported".into()));
        }
        pending.extend(
            crate::expression::references(&parameter.expression.0)?
                .into_iter()
                .map(|n| (scope.clone(), n)),
        );
    }
    Ok(())
}

/// Application-owned configuration source and resolved metadata, independent of layout.
pub fn semantic(
    doc: &Document,
    evaluation: Option<&Evaluated>,
    state: &str,
) -> Vec<crate::presentation::SemanticElement> {
    let mut out = Vec::new();
    for owner in doc.parts.keys().chain(doc.assemblies.keys()) {
        let name = doc
            .parts
            .get(owner)
            .map(|p| p.name.clone())
            .or_else(|| doc.assemblies.get(owner).map(|a| a.name.clone()))
            .unwrap_or_default();
        out.push(crate::presentation::SemanticElement {id:format!("configuration-table:{owner}"),role:"configuration-table".into(),label:format!("{name} configurations"),value:doc.active_configurations.get(owner).and_then(|id|names(doc,owner).get(id).cloned()),state:state.into(),commands:vec!["configurations".into(),"equations".into()],relationships:vec![owner.clone()],data:serde_json::json!({"contract":"tcad.configuration","version":CONTRACT_VERSION,"owner":owner,"source":table(doc,owner),"selected":doc.active_configurations.get(owner),"evaluated":evaluation.and_then(|e|e.definitions.get(owner)),"local_table_formats":["json","csv"]})});
        for field in dimension_fields(doc, owner).unwrap_or_default() {
            out.push(crate::presentation::SemanticElement {
                id: format!("feature-dimension:{owner}/{}{}", field.feature, field.path),
                role: "feature-dimension-source".into(),
                label: field.label.clone(),
                value: Some(field.expression.0.clone()),
                state: state.into(),
                commands: vec!["configurations".into(), "equations".into()],
                relationships: vec![owner.clone(), field.feature.clone()],
                data: serde_json::json!({"contract":"tcad.configuration.dimension-source","version":FIELD_CONTRACT_VERSION,
                    "owner":owner,"source":field,"json":field.expression.0,"kind":field.kind,"maximum":field.maximum,"editable":false,
                    "scalar_count_expression":field.kind==dimensions::FieldKind::Count,"maximum_expression_count":dimensions::MAX_EXPRESSION_COUNT}),
            });
        }
        if let Some(part) = doc.parts.get(owner) {
            for field in sketch_dimensions::fields(part) {
                out.push(crate::presentation::SemanticElement {
                    id: format!("sketch-dimension:{owner}/{}/{}", field.sketch, field.constraint),
                    role: "sketch-dimension-source".into(), label: field.label.clone(), value: Some(field.expression.0.clone()),
                    state: state.into(), commands: vec!["configurations".into(), "equations".into()],
                    relationships: vec![owner.clone(), field.sketch.clone(), field.constraint.clone()],
                    data: serde_json::json!({"contract":"tcad.configuration.sketch-dimension-source","version":sketch_dimensions::CONTRACT_VERSION,
                        "owner":owner,"source":field,"json":field.expression.0,"driving":true,"editable":false}),
                });
            }
        }
        for configuration in definitions(doc, owner).values() {
            out.push(crate::presentation::SemanticElement {id:format!("{owner}@{}",configuration.id),role:"configuration".into(),label:configuration.name.clone(),value:Some(configuration.description.clone()),state:if doc.active_configurations.get(owner)==Some(&configuration.id){"selected"}else{state}.into(),commands:vec!["configurations".into()],relationships:configuration.parent.iter().map(|id|format!("{owner}@{id}")).chain(std::iter::once(owner.clone())).collect(),data:serde_json::json!({"version":CONTRACT_VERSION,"owner":owner,"source":configuration,"evaluated":evaluation.and_then(|e|e.definitions.get(owner)).and_then(|d|d.get(&configuration.id)),"controls":{"parameters":"unit-expression","suppression":"inherit-resolved-suppressed","material":"inherit-material","finish":"inherit-surface-finish","component_configurations":"inherit-selected-named","component_placements":"inherit-axis-placement-seeds","mate_curve_seeds":"inherit-free-path-seeds","properties":"text-map","bom":"inherit-exclusion-naming"}})});
        }
    }
    out
}

pub fn feature_suppressed(doc: &Document, owner: &str, feature: &crate::Feature) -> bool {
    let overrides = doc
        .active_configurations
        .get(owner)
        .and_then(|id| resolve(doc, owner, id).ok())
        .map(|r| r.overrides)
        .unwrap_or_default();
    let mut suppressed = overrides
        .features
        .get(&feature.id)
        .copied()
        .unwrap_or(feature.suppressed);
    if let crate::Operation::Profile {
        profile: crate::Profile::Sketch { sketch },
        ..
    } = &feature.operation
        && let Some(value) = overrides.sketches.get(sketch)
    {
        suppressed = *value;
    }
    suppressed
}
pub fn material(doc: &Document, definition: &str, configuration: Option<&str>) -> Result<Id> {
    let part = doc
        .parts
        .get(definition)
        .ok_or_else(|| invalid("Material part is missing"))?;
    let configuration = configuration.or_else(|| {
        doc.active_configurations
            .get(definition)
            .map(String::as_str)
    });
    Ok(configuration
        .map(|id| resolve(doc, definition, id))
        .transpose()?
        .and_then(|c| c.overrides.material)
        .unwrap_or_else(|| part.material.clone()))
}

/// Solid-output availability in the selected definition configuration.
pub fn has_solid_output(doc: &Document, owner: &str) -> bool {
    effective_part(
        doc,
        owner,
        doc.active_configurations.get(owner).map(String::as_str),
    )
    .is_ok_and(|p| {
        p.outputs.iter().any(|id| {
            p.features
                .iter()
                .any(|f| &f.id == id && !f.suppressed && f.operation.produces_solid())
        })
    })
}
