//! Independent component identities with explicit source ancestry and occurrence scope.
//! Plans are source-only drafts. The normal evaluator must validate a plan before Apply.
use crate::{CadError, Document, Id, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

pub const CONTRACT_VERSION: u32 = 1;
const MAX_DEFINITIONS: usize = 4096;
const MAX_LINEAGE_BYTES: usize = 32 * 1024 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Lineage {
    pub version: u32,
    pub id: Id,
    pub source_document: Id,
    pub source_definition: Id,
    pub source_fingerprint: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_commit: Option<String>,
    pub definition: Id,
    /// Source identity -> local identity. Expression parameter names retain their spelling.
    pub correspondence: BTreeMap<Id, Id>,
    /// Exactly one source definition, retained as a three-way merge base.
    /// This never contains the whole project, generated geometry, or an event log.
    pub source_snapshot: Value,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    #[default]
    ReuseChildren,
    ForkSelected,
    ForkAll,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Target {
    /// Create a definition without changing any existing occurrence.
    DefinitionOnly,
    /// Continue working on the new design at the document root.
    Root,
    /// Replace just this complete occurrence path from the document root.
    Occurrence { path: Vec<Id> },
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Intent {
    pub version: u32,
    pub id: Id,
    pub definition: Id,
    pub name: String,
    pub mode: Mode,
    #[serde(default)]
    pub selected_definitions: BTreeSet<Id>,
    pub target: Target,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_commit: Option<String>,
}
impl Intent {
    pub fn new(doc: &Document, definition: &str, target: Target) -> Self {
        Self {
            version: CONTRACT_VERSION,
            id: crate::id(),
            definition: definition.into(),
            name: format!(
                "{} fork",
                crate::presentation::definition_name(doc, definition)
            ),
            mode: Mode::ReuseChildren,
            selected_definitions: BTreeSet::new(),
            target,
            source_commit: None,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Change {
    pub source: Id,
    pub definition: Id,
    pub name: String,
    pub enclosing: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Plan {
    pub document: Document,
    pub definition: Id,
    pub copied: Vec<Change>,
    pub reused: Vec<Id>,
    pub occurrence_path: Option<Vec<Id>>,
    pub notices: Vec<String>,
}

fn invalid(message: impl Into<String>) -> CadError {
    CadError::Invalid(format!("Fork component: {}", message.into()))
}
fn snapshot(doc: &Document, definition: &str) -> Result<Value> {
    if let Some(part) = doc.parts.get(definition) {
        Ok(serde_json::to_value(part)?)
    } else if let Some(assembly) = doc.assemblies.get(definition) {
        Ok(serde_json::to_value(assembly)?)
    } else {
        Err(invalid("The source definition is missing"))
    }
}

/// Owned source identities only. User text and parameter names are not UUIDs.
pub fn owned_ids(value: &Value) -> BTreeSet<Id> {
    fn visit(value: &Value, field: &str, out: &mut BTreeSet<Id>) {
        if matches!(
            field,
            "properties" | "metadata" | "standard_part" | "library_profile" | "resource" | "role"
        ) {
            return;
        }
        match value {
            Value::Object(object) => {
                if let Some(id) = object.get("id").and_then(Value::as_str) {
                    out.insert(id.into());
                }
                if matches!(
                    field,
                    "sketches"
                        | "points"
                        | "members"
                        | "circles"
                        | "conversions"
                        | "offsets"
                        | "fillets"
                        | "chamfers"
                        | "transforms"
                        | "splines"
                        | "conics"
                        | "texts"
                        | "equations"
                        | "blocks"
                        | "block_instances"
                        | "dimension_groups"
                        | "configurations"
                        | "configuration_details"
                        | "instances"
                        | "component_patterns"
                        | "exploded_views"
                        | "display_states"
                        | "inspections"
                        | "weld_beads"
                        | "rigid_groups"
                ) {
                    out.extend(object.keys().cloned());
                }
                // Pattern reservation identities are generated-source contracts.
                if object.contains_key("seed") && object.contains_key("index") {
                    for key in ["instance", "mate"] {
                        if let Some(id) = object.get(key).and_then(Value::as_str) {
                            out.insert(id.into());
                        }
                    }
                }
                for (key, child) in object {
                    visit(child, key, out);
                }
            }
            Value::Array(values) => {
                for child in values {
                    visit(child, field, out);
                }
            }
            _ => {}
        }
    }
    let mut ids = BTreeSet::new();
    visit(value, "", &mut ids);
    ids
}

fn identity(namespace: &str, source: &str) -> Id {
    use sha2::{Digest, Sha256};
    let hex = format!(
        "{:x}",
        Sha256::digest(format!("tcad-component-fork:{namespace}:{source}"))
    );
    format!(
        "{}-{}-4{}-a{}-{}",
        &hex[..8],
        &hex[8..12],
        &hex[13..16],
        &hex[17..20],
        &hex[20..32]
    )
}
fn mapping(value: &Value, namespace: &str) -> BTreeMap<Id, Id> {
    owned_ids(value)
        .into_iter()
        .map(|id| {
            let next = identity(namespace, &id);
            (id, next)
        })
        .collect()
}
fn remap(value: &mut Value, ids: &BTreeMap<Id, Id>) -> Result<()> {
    crate::library::remap_source(value, ids, "")
}

/// Component and geometric dependency closure. Source expressions carry explicit owners.
pub fn dependencies(doc: &Document, definition: &str) -> Result<BTreeSet<Id>> {
    fn visit(value: &Value, definitions: &BTreeSet<&str>, out: &mut BTreeSet<Id>) {
        match value {
            Value::Object(object) => {
                if let Some(id) = object.get("definition").and_then(Value::as_str)
                    && definitions.contains(id)
                {
                    out.insert(id.into());
                }
                for (key, value) in object {
                    if !matches!(
                        key.as_str(),
                        "properties" | "metadata" | "standard_part" | "source_snapshot"
                    ) {
                        visit(value, definitions, out);
                    }
                }
            }
            Value::Array(values) => {
                for value in values {
                    visit(value, definitions, out);
                }
            }
            Value::String(text) => {
                for id in definitions {
                    if text.contains(&format!("@{id}::")) {
                        out.insert((*id).into());
                    }
                }
            }
            _ => {}
        }
    }
    let definitions = doc
        .parts
        .keys()
        .chain(doc.assemblies.keys())
        .map(String::as_str)
        .collect();
    let mut out = BTreeSet::new();
    let mut value = snapshot(doc, definition)?;
    crate::feature_transfer::normalize_definition_expressions(doc, definition, &mut value)?;
    visit(&value, &definitions, &mut out);
    out.remove(definition);
    Ok(out)
}
pub fn descendants(doc: &Document, definition: &str) -> Result<BTreeSet<Id>> {
    let mut pending = vec![definition.to_owned()];
    let mut found = BTreeSet::new();
    while let Some(id) = pending.pop() {
        if !found.insert(id.clone()) {
            continue;
        }
        if found.len() > MAX_DEFINITIONS {
            return Err(invalid("The dependency closure exceeds 4096 definitions"));
        }
        pending.extend(dependencies(doc, &id)?);
    }
    found.remove(definition);
    Ok(found)
}
fn copy_set(doc: &Document, intent: &Intent) -> Result<BTreeSet<Id>> {
    let reachable = descendants(doc, &intent.definition)?;
    if intent
        .selected_definitions
        .iter()
        .any(|id| !reachable.contains(id))
    {
        return Err(invalid(
            "A selected child is not a dependency of this component",
        ));
    }
    let mut set = BTreeSet::from([intent.definition.clone()]);
    match intent.mode {
        Mode::ReuseChildren => {}
        Mode::ForkAll => set.extend(reachable),
        Mode::ForkSelected => {
            set.extend(intent.selected_definitions.iter().cloned());
            // A selected child requires independent enclosing definitions all the
            // way back to the source; otherwise changing a child edits a shared owner.
            loop {
                let before = set.len();
                for candidate in &reachable {
                    if dependencies(doc, candidate)?
                        .iter()
                        .any(|child| set.contains(child))
                    {
                        set.insert(candidate.clone());
                    }
                }
                if before == set.len() {
                    break;
                }
            }
        }
    }
    Ok(set)
}

fn reject_context_bound(doc: &Document, definitions: &BTreeSet<Id>) -> Result<()> {
    for definition in definitions {
        if doc.parts.get(definition).is_some_and(|part| part.features.iter().any(|feature| {
            matches!(&feature.operation, crate::Operation::Pattern { options, .. } if options.requires_placement())
        })) {
            return Err(CadError::Unsupported("Fork component: an occurrence-bound part pattern needs an explicit new placement context. Use a local sketch/reference-axis direction before forking this definition".into()));
        }
    }
    Ok(())
}
fn store_clone(
    source: &Document,
    document: &mut Document,
    original: &str,
    mut value: Value,
    ids: &BTreeMap<Id, Id>,
    intent: &Intent,
    enclosing: bool,
) -> Result<Change> {
    let definition = ids
        .get(original)
        .ok_or_else(|| invalid("Missing reserved definition identity"))?
        .clone();
    if document.parts.contains_key(&definition) || document.assemblies.contains_key(&definition) {
        return Err(invalid(
            "The fork identity already exists; open a new fork operation",
        ));
    }
    value["name"] = if original == intent.definition && !enclosing {
        intent.name.clone()
    } else {
        format!(
            "{} fork",
            crate::presentation::definition_name(source, original)
        )
    }
    .into();
    let name = value["name"].as_str().unwrap_or("Fork").to_owned();
    if source.parts.contains_key(original) {
        document
            .parts
            .insert(definition.clone(), serde_json::from_value(value)?);
    } else {
        document
            .assemblies
            .insert(definition.clone(), serde_json::from_value(value)?);
    }
    let base = snapshot(source, original)?;
    fn mentioned(value: &Value, out: &mut BTreeSet<String>) {
        match value {
            Value::Object(object) => {
                out.extend(object.keys().cloned());
                for value in object.values() {
                    mentioned(value, out);
                }
            }
            Value::Array(values) => {
                for value in values {
                    mentioned(value, out);
                }
            }
            Value::String(value) => {
                out.insert(value.clone());
            }
            _ => {}
        }
    }
    let mut mentions = BTreeSet::new();
    mentioned(&base, &mut mentions);
    let mut canonical_base = base.clone();
    crate::feature_transfer::normalize_definition_expressions(
        source,
        original,
        &mut canonical_base,
    )?;
    mentioned(&canonical_base, &mut mentions);
    let mut correspondence = ids
        .iter()
        .filter(|(id, _)| {
            mentions.contains(*id)
                || mentions
                    .iter()
                    .any(|value| value.contains(&format!("@{id}::")))
        })
        .map(|(old, new)| (old.clone(), new.clone()))
        .collect::<BTreeMap<_, _>>();
    // Parameter keys are scoped expression names, not independently renamed records.
    if let Some(parameters) = base.get("parameters").and_then(Value::as_object) {
        for key in parameters.keys() {
            correspondence
                .entry(key.clone())
                .or_insert_with(|| key.clone());
        }
    }
    document.component_forks.insert(
        definition.clone(),
        Lineage {
            version: CONTRACT_VERSION,
            id: intent.id.clone(),
            source_document: source.id.clone(),
            source_definition: original.into(),
            source_fingerprint: source.fingerprint(),
            source_commit: intent.source_commit.clone(),
            definition: definition.clone(),
            correspondence,
            source_snapshot: base,
        },
    );
    if let Some(configuration) = source.active_configurations.get(original) {
        document.active_configurations.insert(
            definition.clone(),
            ids.get(configuration)
                .cloned()
                .unwrap_or_else(|| configuration.clone()),
        );
    }
    Ok(Change {
        source: original.into(),
        definition,
        name,
        enclosing,
    })
}

fn array_path(value: &Value) -> Option<Vec<Id>> {
    value
        .as_array()?
        .iter()
        .map(|id| id.as_str().map(str::to_owned))
        .collect()
}
/// Rewrite only explicit occurrence-qualified references entering one chosen path.
/// Mate paths are relative to their participant; all other paths are owner-relative.
fn route(value: &mut Value, selected: &[Id], ids: &BTreeMap<Id, Id>) -> Result<()> {
    match value {
        Value::Object(object) => {
            for key in ["instance_path", "occurrence_path", "path"] {
                if object
                    .get(key)
                    .and_then(array_path)
                    .is_some_and(|path| path.starts_with(selected))
                {
                    return remap(value, ids);
                }
            }
            let mut handled = BTreeSet::new();
            for (participant, key) in [
                ("first", "first_reference"),
                ("second", "second_reference"),
                ("instance", "reference"),
                ("instance", "geometry"),
            ] {
                let Some(first) = object
                    .get(participant)
                    .and_then(Value::as_str)
                    .map(str::to_owned)
                else {
                    continue;
                };
                let Some(reference) = object.get_mut(key) else {
                    continue;
                };
                let Some(tail) = reference.get("path").and_then(array_path) else {
                    continue;
                };
                let path: Vec<_> = std::iter::once(first).chain(tail).collect();
                if path.starts_with(selected) {
                    remap(reference, ids)?;
                }
                // Its path is participant-relative; do not inspect it a second time.
                handled.insert(key.to_owned());
            }
            // Named configurations can explicitly select the forked child's configuration.
            if selected.len() == 1
                && let Some(configurations) = object
                    .get_mut("component_configurations")
                    .and_then(Value::as_object_mut)
                && let Some(value) = configurations.get_mut(&selected[0])
                && let Some(next) = value.as_str().and_then(|id| ids.get(id))
            {
                *value = next.clone().into();
            }
            for (key, value) in object {
                if !handled.contains(key)
                    && !matches!(
                        key.as_str(),
                        "properties"
                            | "metadata"
                            | "source_snapshot"
                            | "component_forks"
                            | "instances"
                    )
                {
                    route(value, selected, ids)?;
                }
            }
        }
        Value::Array(values) => {
            for value in values {
                route(value, selected, ids)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn occurrence_owners(doc: &Document, path: &[Id], expected: &str) -> Result<Vec<Id>> {
    if path.is_empty() || path.len() > 64 {
        return Err(invalid(
            "Select a component occurrence path with 1 through 64 levels",
        ));
    }
    let mut definition = doc.root.clone();
    let mut owners = vec![];
    for instance in path {
        let assembly = doc
            .assemblies
            .get(&definition)
            .ok_or_else(|| invalid("The occurrence path crosses a missing assembly"))?;
        if crate::component_pattern::generated(assembly).contains(instance) {
            return Err(invalid(
                "Detach the selected patterned occurrence before forking it",
            ));
        }
        if assembly
            .component_patterns
            .values()
            .any(|pattern| pattern.seeds.contains(instance))
        {
            return Err(invalid(
                "Detach the component pattern before forking one of its seed occurrences; this keeps the other patterned occurrences unchanged",
            ));
        }
        owners.push(definition);
        definition = assembly
            .instances
            .get(instance)
            .ok_or_else(|| invalid("The selected component occurrence no longer exists"))?
            .definition
            .clone();
    }
    if definition != expected {
        return Err(invalid("The selected occurrence uses another definition"));
    }
    Ok(owners)
}

/// Find exact occurrence paths for an owner-local instance. More than one match
/// requires an explicit tree path; callers must never pick an arbitrary match.
pub fn occurrence_paths(doc: &Document, owner: &str, instance: &str) -> Result<Vec<Vec<Id>>> {
    let mut pending = vec![(doc.root.clone(), Vec::<Id>::new(), BTreeSet::<Id>::new())];
    let mut paths = vec![];
    let mut visited = 0usize;
    while let Some((definition, path, mut ancestors)) = pending.pop() {
        visited += 1;
        if visited > 100_000 || path.len() > 64 || !ancestors.insert(definition.clone()) {
            return Err(invalid(
                "Occurrence traversal exceeds its limit or contains a circular assembly",
            ));
        }
        let Some(assembly) = doc.assemblies.get(&definition) else {
            continue;
        };
        for child in assembly.instances.values() {
            let mut child_path = path.clone();
            child_path.push(child.id.clone());
            if definition == owner && child.id == instance {
                paths.push(child_path.clone());
            }
            if doc.assemblies.contains_key(&child.definition) {
                pending.push((child.definition.clone(), child_path, ancestors.clone()));
            }
        }
    }
    Ok(paths)
}

fn mapped_path(path: &[Id], selected: &[Id], ids: &BTreeMap<Id, Id>) -> Vec<Id> {
    if path.starts_with(selected) {
        path.iter()
            .map(|id| ids.get(id).cloned().unwrap_or_else(|| id.clone()))
            .collect()
    } else {
        path.to_vec()
    }
}
type Routes = Vec<(Vec<Id>, BTreeMap<Id, Id>)>;
fn route_all(
    value: &mut Value,
    selected: &[Id],
    ids: &BTreeMap<Id, Id>,
    enclosers: &Routes,
) -> Result<()> {
    route(value, selected, ids)?;
    for (prefix, own) in enclosers {
        route(value, prefix, own)?;
    }
    Ok(())
}
fn drawing_mapping(
    value: &Value,
    selected: &[Id],
    ids: &BTreeMap<Id, Id>,
    enclosers: &Routes,
) -> BTreeMap<Id, Id> {
    fn paths(value: &Value, out: &mut Vec<Vec<Id>>) {
        match value {
            Value::Object(object) => {
                for (key, value) in object {
                    if matches!(key.as_str(), "instance_path" | "occurrence_path" | "path")
                        && let Some(path) = array_path(value)
                    {
                        out.push(path);
                    } else {
                        paths(value, out);
                    }
                }
            }
            Value::Array(values) => {
                for value in values {
                    paths(value, out);
                }
            }
            _ => {}
        }
    }
    let mut references = vec![];
    paths(value, &mut references);
    let mut out = BTreeMap::new();
    if references.iter().any(|path| path.starts_with(selected)) {
        out.extend(ids.clone());
    }
    for (prefix, own) in enclosers {
        if references.iter().any(|path| path.starts_with(prefix)) {
            out.extend(own.clone());
        }
    }
    out
}
fn reserve_row(
    table: &mut crate::drawing_tables::Table,
    old: &str,
    new: &str,
    retain: bool,
) -> Result<()> {
    if old == new {
        return Ok(());
    }
    let number = if let Some(number) = table.item_numbers.get(new) {
        *number
    } else if !retain && let Some(number) = table.item_numbers.get(old) {
        *number
    } else {
        table
            .item_numbers
            .values()
            .max()
            .copied()
            .unwrap_or(0)
            .checked_add(1)
            .ok_or_else(|| invalid("Table numbering overflow"))?
    };
    if !retain {
        table.item_numbers.remove(old);
    }
    table.item_numbers.insert(new.into(), number);
    if !retain {
        for key in &mut table.row_order {
            if key == old {
                *key = new.into();
            }
        }
    }
    if !table.row_order.iter().any(|key| key == new) {
        table.row_order.push(new.into());
    }
    Ok(())
}
fn retarget_drawings(
    source: &Document,
    document: &mut Document,
    selected: &[Id],
    ids: &BTreeMap<Id, Id>,
    enclosers: &Routes,
) -> Result<()> {
    use crate::drawing::AnnotationKind;
    use crate::drawing_tables::TableKind;
    let after = document.clone();
    let mut comment_targets = BTreeMap::new();
    for sheet in &mut document.drawings {
        if sheet.definition != source.root {
            continue;
        }
        let mut row_maps = BTreeMap::<Id, BTreeMap<String, String>>::new();
        for table in &mut sheet.tables {
            match &table.kind {
                TableKind::BillOfMaterials => {
                    let before_rows = crate::bom::rows(source, &table.bom_options)?;
                    let after_rows = crate::bom::rows(&after, &table.bom_options)?;
                    let mut next = table.item_numbers.values().max().copied().unwrap_or(0);
                    for row in &before_rows {
                        if !table.item_numbers.contains_key(&row.key) {
                            next = next
                                .checked_add(1)
                                .ok_or_else(|| invalid("BOM numbering overflow"))?;
                            table.item_numbers.insert(row.key.clone(), next);
                        }
                    }
                    for row in &before_rows {
                        let paths = row
                            .occurrences
                            .iter()
                            .filter(|occurrence| {
                                occurrence.path.starts_with(selected)
                                    || enclosers
                                        .iter()
                                        .any(|(prefix, _)| occurrence.path.starts_with(prefix))
                            })
                            .map(|occurrence| {
                                let mut path = mapped_path(&occurrence.path, selected, ids);
                                for (prefix, own) in enclosers {
                                    if occurrence.path.starts_with(prefix) {
                                        path = path
                                            .iter()
                                            .map(|id| {
                                                own.get(id).cloned().unwrap_or_else(|| id.clone())
                                            })
                                            .collect();
                                    }
                                }
                                path
                            })
                            .collect::<BTreeSet<_>>();
                        if paths.is_empty() {
                            continue;
                        }
                        let successors = after_rows
                            .iter()
                            .filter(|row| {
                                row.occurrences
                                    .iter()
                                    .any(|occurrence| paths.contains(&occurrence.path))
                            })
                            .collect::<Vec<_>>();
                        if successors.len() != 1 {
                            return Err(invalid(
                                "The fork splits a BOM group into ambiguous rows; use a definition-based BOM grouping before forking",
                            ));
                        }
                        let new = &successors[0].key;
                        reserve_row(
                            table,
                            &row.key,
                            new,
                            after_rows.iter().any(|candidate| candidate.key == row.key),
                        )?;
                        row_maps
                            .entry(table.id.clone())
                            .or_default()
                            .insert(row.key.clone(), new.clone());
                    }
                }
                TableKind::Holes { instance_path, .. }
                    if instance_path.starts_with(selected)
                        || enclosers
                            .iter()
                            .any(|(prefix, _)| instance_path.starts_with(prefix)) =>
                {
                    let mut kind = serde_json::to_value(&table.kind)?;
                    let relevant = drawing_mapping(&kind, selected, ids, enclosers);
                    route_all(&mut kind, selected, ids, enclosers)?;
                    table.kind = serde_json::from_value(kind)?;
                    let keys = table
                        .item_numbers
                        .keys()
                        .chain(table.row_order.iter())
                        .cloned()
                        .collect::<BTreeSet<_>>();
                    for old in keys {
                        let new =
                            crate::library::remap_manufacturing_row_key(&old, &relevant, false)?;
                        reserve_row(table, &old, &new, false)?;
                        row_maps
                            .entry(table.id.clone())
                            .or_default()
                            .insert(old, new);
                    }
                }
                TableKind::WeldmentCutList { scope, .. } if scope == &source.root => {
                    // A root-scoped list retains old stock rows and adds identities
                    // for the independent stock. Geometry-only grouping stays shared.
                    let keys = table
                        .item_numbers
                        .keys()
                        .chain(table.row_order.iter())
                        .cloned()
                        .collect::<BTreeSet<_>>();
                    for old in keys {
                        let new = crate::library::remap_manufacturing_row_key(&old, ids, false)?;
                        if new != old {
                            reserve_row(table, &old, &new, true)?;
                            row_maps
                                .entry(table.id.clone())
                                .or_default()
                                .insert(old, new);
                        }
                    }
                    if let TableKind::WeldmentCutList { sources, .. } = &mut table.kind {
                        let mut additions = vec![];
                        for reference in sources.iter() {
                            let mut value = serde_json::to_value(reference)?;
                            remap(&mut value, ids)?;
                            let next: crate::NodeRef = serde_json::from_value(value)?;
                            if next != *reference {
                                additions.push(next);
                            }
                        }
                        sources.extend(additions);
                        sources.sort();
                        sources.dedup();
                    }
                }
                _ => {}
            }
        }
        for dimension in &mut sheet.dimensions {
            let before = serde_json::to_value(&*dimension)?;
            let mut next = before.clone();
            let relevant = drawing_mapping(&before, selected, ids, enclosers);
            route_all(&mut next, selected, ids, enclosers)?;
            if next != before {
                if let Some(value) = next.get_mut("parameter") {
                    remap(value, &relevant)?;
                }
                if let Some(value) = next.pointer_mut("/options/hole_feature") {
                    remap(value, &relevant)?;
                }
            }
            *dimension = serde_json::from_value(next)?;
        }
        for annotation in &mut sheet.entities {
            let before = serde_json::to_value(&annotation.kind)?;
            let mut next = before.clone();
            let relevant = drawing_mapping(&before, selected, ids, enclosers);
            route_all(&mut next, selected, ids, enclosers)?;
            let changed = next != before;
            if changed {
                match &annotation.kind {
                    AnnotationKind::Balloon {
                        table,
                        row_key,
                        definition,
                        configuration,
                        ..
                    } => {
                        let old = row_key.clone().unwrap_or_else(|| {
                            crate::bom::item_key(definition, configuration.as_deref())
                        });
                        let new = row_maps.get(table).and_then(|rows| rows.get(&old)).ok_or_else(|| invalid("An affected balloon has no unique forked BOM row; reattach its table before forking"))?;
                        next["row_key"] = new.clone().into();
                        if let Some(id) = relevant.get(definition) {
                            next["definition"] = id.clone().into();
                        }
                        if let Some(id) = configuration.as_ref().and_then(|id| relevant.get(id)) {
                            next["configuration"] = id.clone().into();
                        }
                    }
                    AnnotationKind::CutListBalloon { table, row_key, .. } => {
                        let new = row_maps
                            .get(table)
                            .and_then(|rows| rows.get(row_key))
                            .cloned()
                            .unwrap_or(crate::library::remap_manufacturing_row_key(
                                row_key, &relevant, false,
                            )?);
                        if !sheet.tables.iter().any(|candidate| candidate.id == *table
                            && matches!(&candidate.kind, TableKind::WeldmentCutList {scope,..} if scope == &source.root)) {
                            return Err(invalid("An affected cut-list balloon uses a part-only table. Change its table scope to this assembly before forking"));
                        }
                        next["row_key"] = new.into();
                    }
                    _ => {}
                }
            }
            annotation.kind = serde_json::from_value(next)?;
        }
        for table in &sheet.tables {
            if let Some(rows) = row_maps.get(&table.id) {
                for (old, new) in rows {
                    if old != new && !table.item_numbers.contains_key(old) {
                        comment_targets.insert(
                            crate::drawing_tables::row_id(table, old),
                            crate::drawing_tables::row_id(table, new),
                        );
                    }
                }
            }
        }
    }
    for comment in &mut document.comments {
        if let Some(target) = comment_targets.get(&comment.target) {
            comment.target = target.clone();
        }
    }
    Ok(())
}

pub fn plan(source: &Document, intent: &Intent) -> Result<Plan> {
    if intent.version != CONTRACT_VERSION
        || intent.id.is_empty()
        || intent.name.trim().is_empty()
        || intent.name.len() > 1024
    {
        return Err(invalid(
            "Set a valid fork name and supported contract version",
        ));
    }
    if intent.source_commit.as_ref().is_some_and(|commit| {
        !matches!(commit.len(), 40 | 64) || !commit.bytes().all(|byte| byte.is_ascii_hexdigit())
    }) {
        return Err(invalid(
            "Source commit must be an exact Git object identity",
        ));
    }
    if matches!(intent.target, Target::Root) && intent.definition != source.root {
        return Err(invalid(
            "Root replacement must fork the current document root",
        ));
    }
    let copied = copy_set(source, intent)?;
    reject_context_bound(source, &copied)?;
    let mut all = BTreeMap::new();
    for definition in &copied {
        all.extend(mapping(&snapshot(source, definition)?, &intent.id));
    }
    let definition = all[&intent.definition].clone();
    let mut document = source.clone();
    let mut changes = vec![];
    for original in &copied {
        let mut value = snapshot(source, original)?;
        crate::feature_transfer::normalize_definition_expressions(source, original, &mut value)?;
        remap(&mut value, &all)?;
        changes.push(store_clone(
            source,
            &mut document,
            original,
            value,
            &all,
            intent,
            false,
        )?);
    }
    let mut occurrence_path = None;
    let mut notices = vec!["The original definitions remain available. Forks do not follow source updates automatically.".into()];
    match &intent.target {
        Target::DefinitionOnly => notices.push("No existing occurrence is replaced.".into()),
        Target::Root => {
            document.root = definition.clone();
            notices.push("Existing drawings retain their original design references.".into());
        }
        Target::Occurrence { path } => {
            let owners = occurrence_owners(source, path, &intent.definition)?;
            let mut next = definition.clone();
            let mut branch_ids = all.clone();
            let mut enclosers = Routes::new();
            // Copy nested owners from the leaf upward. The root itself is the
            // working project; nested shared definitions remain untouched.
            for depth in (1..owners.len()).rev() {
                let owner = &owners[depth];
                let mut value = snapshot(source, owner)?;
                crate::feature_transfer::normalize_definition_expressions(
                    source, owner, &mut value,
                )?;
                let own = mapping(&value, &format!("{}:enclosing:{depth}", intent.id));
                route(&mut value, &path[depth..], &branch_ids)?;
                for (prefix, own) in &enclosers {
                    route(&mut value, &prefix[depth..], own)?;
                }
                value["instances"][&path[depth]]["definition"] = next.clone().into();
                if let Some(configuration) = value["instances"][&path[depth]]["configuration"]
                    .as_str()
                    .and_then(|id| branch_ids.get(id))
                    .cloned()
                {
                    value["instances"][&path[depth]]["configuration"] = configuration.into();
                }
                remap(&mut value, &own)?;
                let mut lineage_ids = branch_ids.clone();
                lineage_ids.extend(own.clone());
                changes.push(store_clone(
                    source,
                    &mut document,
                    owner,
                    value,
                    &lineage_ids,
                    intent,
                    true,
                )?);
                next = own[owner].clone();
                enclosers.push((path[..depth].to_vec(), own.clone()));
                branch_ids.extend(own);
            }
            let mut root = snapshot(source, &source.root)?;
            route_all(&mut root, path, &branch_ids, &enclosers)?;
            root["instances"][&path[0]]["definition"] = next.into();
            if let Some(configuration) = root["instances"][&path[0]]["configuration"]
                .as_str()
                .and_then(|id| branch_ids.get(id))
                .cloned()
            {
                root["instances"][&path[0]]["configuration"] = configuration.into();
            }
            document
                .assemblies
                .insert(source.root.clone(), serde_json::from_value(root)?);
            retarget_drawings(source, &mut document, path, &branch_ids, &enclosers)?;
            for sensor in document.sensors.values_mut() {
                let mut value = serde_json::to_value(&*sensor)?;
                route_all(&mut value, path, &branch_ids, &enclosers)?;
                *sensor = serde_json::from_value(value)?;
            }
            // A source part with assembly-occurrence-bound modeling intent is
            // itself shared. Changing it here would silently affect other uses.
            for part in source.parts.values() {
                let before = serde_json::to_value(part)?;
                let mut value = before.clone();
                route_all(&mut value, path, &branch_ids, &enclosers)?;
                if value != before {
                    return Err(CadError::Unsupported(format!(
                        "Fork component: {} contains modeling references bound to this assembly occurrence. Rebind or remove those context references before forking this branch",
                        part.name
                    )));
                }
            }
            occurrence_path = Some(
                path.iter()
                    .enumerate()
                    .map(|(index, id)| {
                        if index == 0 {
                            id.clone()
                        } else {
                            branch_ids.get(id).cloned().unwrap_or_else(|| id.clone())
                        }
                    })
                    .collect(),
            );
            if owners.len() > 1 {
                notices.push(format!("{} enclosing assemblies become independent for this occurrence. Other occurrences retain their original definitions.", owners.len() - 1));
            }
        }
    }
    validate(&document)?;
    document.validate()?;
    let reused = descendants(source, &intent.definition)?
        .difference(&copied)
        .cloned()
        .collect();
    Ok(Plan {
        document,
        definition,
        copied: changes,
        reused,
        occurrence_path,
        notices,
    })
}

/// Local/Git library forks keep their original upstream identities, rather than
/// treating the host's import namespace as a new unrelated design. Exact pinned
/// bytes supply the ancestry; a mutable source path is never consulted.
pub fn plan_with_resources(
    source: &Document,
    intent: &Intent,
    root: &std::path::Path,
) -> Result<Plan> {
    let mut plan = plan(source, intent)?;
    let mut verified = BTreeMap::new();
    for change in &plan.copied {
        let Some(link) = source
            .libraries
            .values()
            .find(|link| link.pinned_definitions.contains_key(&change.source))
        else {
            continue;
        };
        if !verified.contains_key(&link.id) {
            verified.insert(
                link.id.clone(),
                crate::library::verified_source(source, root, &link.id)?,
            );
        }
        let (upstream, imported) = &verified[&link.id];
        let original = imported
            .iter()
            .find(|(_, local)| *local == &change.source)
            .map(|(original, _)| original.clone())
            .ok_or_else(|| invalid("The linked source has no original definition identity"))?;
        let lineage = plan
            .document
            .component_forks
            .get_mut(&change.definition)
            .ok_or_else(|| invalid("The fork ancestry record is missing"))?;
        let local = lineage.correspondence.clone();
        let mut correspondence = BTreeMap::new();
        let original_snapshot = snapshot(upstream, &original)?;
        let mut canonical_snapshot = original_snapshot.clone();
        crate::feature_transfer::normalize_definition_expressions(
            upstream,
            &original,
            &mut canonical_snapshot,
        )?;
        let encoded = serde_json::to_string(&canonical_snapshot)?;
        for (original, host) in imported {
            if let Some(target) = local.get(host) {
                correspondence.insert(original.clone(), target.clone());
            } else if encoded.contains(&serde_json::to_string(original)?)
                || encoded.contains(&format!("@{original}::"))
            {
                // A shallow fork can still use a verified shared imported input.
                correspondence.insert(original.clone(), host.clone());
            }
        }
        if let Some(parameters) = original_snapshot
            .get("parameters")
            .and_then(Value::as_object)
        {
            for key in parameters.keys() {
                correspondence
                    .entry(key.clone())
                    .or_insert_with(|| key.clone());
            }
        }
        lineage.source_document = upstream.id.clone();
        lineage.source_definition = original;
        lineage.source_fingerprint = upstream.fingerprint();
        lineage.source_commit = link.git_source.as_ref().map(|git| git.commit.clone());
        lineage.correspondence = correspondence;
        lineage.source_snapshot = original_snapshot;
    }
    validate(&plan.document)?;
    plan.document.validate()?;
    Ok(plan)
}

pub fn validate(doc: &Document) -> Result<()> {
    if doc.component_forks.len() > MAX_DEFINITIONS
        || serde_json::to_vec(&doc.component_forks)?.len() > MAX_LINEAGE_BYTES
    {
        return Err(invalid(
            "Fork ancestry exceeds the 4096-record / 32 MiB project limit",
        ));
    }
    for (definition, lineage) in &doc.component_forks {
        if lineage.version != CONTRACT_VERSION
            || definition != &lineage.definition
            || lineage.source_document.is_empty()
            || lineage.id.is_empty()
            || lineage.source_snapshot.get("id").and_then(Value::as_str)
                != Some(lineage.source_definition.as_str())
            || lineage.correspondence.get(&lineage.source_definition) != Some(definition)
            || !doc.parts.contains_key(definition) && !doc.assemblies.contains_key(definition)
            || lineage.source_fingerprint.len() != 64
            || !lineage
                .source_fingerprint
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
            || lineage.source_commit.as_ref().is_some_and(|commit| {
                !matches!(commit.len(), 40 | 64)
                    || !commit.bytes().all(|byte| byte.is_ascii_hexdigit())
            })
        {
            return Err(invalid(
                "Fork ancestry identity or source snapshot is invalid",
            ));
        }
        if lineage.source_snapshot.get("features").is_some() {
            let _: crate::Part = serde_json::from_value(lineage.source_snapshot.clone())?;
        } else {
            let _: crate::Assembly = serde_json::from_value(lineage.source_snapshot.clone())?;
        }
    }
    Ok(())
}
