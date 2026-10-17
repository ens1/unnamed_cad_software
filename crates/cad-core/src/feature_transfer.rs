//! Selective, three-way feature transfer. Plans contain source definitions, never kernel objects.
//! An adapter must evaluate the complete candidate before committing it to the shared document.
use crate::{CadError, Document, Feature, Id, NodeRef, Operation, Part, Profile, Resource, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

pub const CONTRACT_VERSION: u32 = 1;

/// Last reviewed source values, scoped to transferred items. Unselected upstream features are absent.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Receipt {
    pub version: u32,
    pub source_document: Id,
    pub source_definition: Id,
    pub target_definition: Id,
    pub correspondence: BTreeMap<Id, Id>,
    pub features: BTreeMap<Id, Feature>,
    pub sketches: BTreeMap<Id, crate::Sketch>,
    pub parameters: BTreeMap<Id, crate::Parameter>,
    pub resources: BTreeMap<Id, Resource>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub materials: BTreeMap<Id, crate::Material>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub finishes: BTreeMap<Id, crate::appearance::Finish>,
}

pub fn validate(document: &Document) -> Result<()> {
    if document.feature_transfers.len() > 4096 {
        return Err(invalid("The document has too many transfer records."));
    }
    let mut size = 0_usize;
    for record in document.feature_transfers.values() {
        if record.version != CONTRACT_VERSION
            || record.source_document.is_empty()
            || record.source_definition.is_empty()
            || !document.parts.contains_key(&record.target_definition)
            || record.correspondence.get(&record.source_definition)
                != Some(&record.target_definition)
        {
            return Err(invalid("A saved transfer record has invalid ownership."));
        }
        size = size.saturating_add(serde_json::to_vec(record)?.len());
        if size > 32 * 1024 * 1024 {
            return Err(invalid(
                "Transfer ancestry exceeds the 32 MiB document limit.",
            ));
        }
        if record.correspondence.len() > 262_144 {
            return Err(invalid("Transfer ancestry contains too many identities."));
        }
    }
    Ok(())
}

fn receipt_key(source: &Document, from: &str, to: &str) -> Id {
    format!(
        "transfer-{:x}",
        Sha256::digest(format!("{}:{from}:{to}", source.id))
    )
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Resolution {
    KeepTarget,
    UseSource,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct NodeBinding {
    pub source: NodeRef,
    pub target: NodeRef,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ParameterBinding {
    pub source: crate::ParameterRef,
    pub target: crate::ParameterRef,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ParameterInput {
    pub source: crate::ParameterRef,
    pub label: String,
    pub dimension: crate::Dimension,
    pub target: Option<crate::ParameterRef>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Request {
    pub source_definition: Id,
    pub target_definition: Id,
    pub selected_features: BTreeSet<Id>,
    /// Existing upstream features are reused unless explicitly included here.
    pub include_dependencies: BTreeSet<Id>,
    /// Explicit input adaptation. These do not replace or modify the chosen target nodes.
    pub bindings: Vec<NodeBinding>,
    pub parameter_bindings: Vec<ParameterBinding>,
    /// Optional exact topology/sketch input repairs captured from an explicit user selection.
    pub repairs: Vec<crate::reference_repair::Repair>,
    /// Key is the stable `kind:source-id` shown by the plan.
    pub resolutions: BTreeMap<String, Resolution>,
    pub reconnect_outputs: bool,
    /// Explicitly accept a parallel result when the target has later body edits.
    pub allow_parallel_outputs: bool,
}
impl Default for Request {
    fn default() -> Self {
        Self {
            source_definition: String::new(),
            target_definition: String::new(),
            selected_features: BTreeSet::new(),
            include_dependencies: BTreeSet::new(),
            bindings: Vec::new(),
            parameter_bindings: Vec::new(),
            repairs: Vec::new(),
            resolutions: BTreeMap::new(),
            reconnect_outputs: true,
            allow_parallel_outputs: false,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Item {
    pub key: String,
    pub source: Id,
    pub target: Id,
    pub kind: String,
    pub label: String,
    pub state: String,
    pub selected: bool,
    pub message: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Conflict {
    pub key: String,
    pub label: String,
    pub message: String,
    pub resolution: Option<Resolution>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ResourceCopy {
    pub source: Resource,
    pub target: Resource,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Plan {
    pub contract_version: u32,
    pub source_fingerprint: String,
    pub target_fingerprint: String,
    pub source_definition: Id,
    pub target_definition: Id,
    pub items: Vec<Item>,
    pub conflicts: Vec<Conflict>,
    pub diagnostics: Vec<String>,
    pub resources: Vec<ResourceCopy>,
    pub reference_inputs: Vec<crate::reference_repair::Input>,
    pub parameter_inputs: Vec<ParameterInput>,
    pub candidate: Option<Document>,
}
impl Plan {
    /// Reject a stale preview, including changes made by another editor.
    pub fn document(&self, current: &Document) -> Result<Document> {
        if current.fingerprint() != self.target_fingerprint {
            return Err(invalid("The target changed. Build a new transfer preview."));
        }
        self.candidate.clone().ok_or_else(|| {
            invalid("Resolve the listed dependencies and conflicts before previewing.")
        })
    }
}

fn invalid(message: impl Into<String>) -> CadError {
    CadError::Invalid(format!("Feature transfer: {}", message.into()))
}
fn stable_id(source: &Document, part: &str, target: &str, id: &str) -> Id {
    let digest = format!(
        "{:x}",
        Sha256::digest(format!(
            "tcad.feature-transfer.v1:{}:{part}:{target}:{id}",
            source.id
        ))
    );
    format!("transfer-{}", &digest[..32])
}

fn lineage(document: &Document, part: &str) -> Option<Value> {
    document
        .component_forks
        .get(part)
        .and_then(|record| serde_json::to_value(record).ok())
}
fn correspondence(lineage: &Value) -> BTreeMap<Id, Id> {
    lineage
        .get("correspondence")
        .cloned()
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default()
}

/// Maps identities through retained fork/transfer ancestry in either direction.
/// Only explicit correspondence joins are followed; names and geometry never imply identity.
pub fn identity_map(
    source: &Document,
    target: &Document,
    from: &str,
    to: &str,
) -> BTreeMap<Id, Id> {
    ancestry_mapping(source, target, from, to).0
}

fn ancestry_mapping(
    source: &Document,
    target: &Document,
    from: &str,
    to: &str,
) -> (BTreeMap<Id, Id>, BTreeSet<Id>) {
    type Point = (Id, Id, Id); // document, definition, entity
    let mut graph = BTreeMap::<Point, BTreeSet<Point>>::new();
    let mut connect = |source_document: &str,
                       source_definition: &str,
                       target_document: &str,
                       target_definition: &str,
                       correspondence: &BTreeMap<Id, Id>| {
        for (a, b) in correspondence {
            let a = (source_document.into(), source_definition.into(), a.clone());
            let b = (target_document.into(), target_definition.into(), b.clone());
            graph.entry(a.clone()).or_default().insert(b.clone());
            graph.entry(b).or_default().insert(a);
        }
    };
    for document in [source, target] {
        for record in document.component_forks.values() {
            connect(
                &record.source_document,
                &record.source_definition,
                &document.id,
                &record.definition,
                &record.correspondence,
            );
        }
        for record in document.feature_transfers.values() {
            connect(
                &record.source_document,
                &record.source_definition,
                &document.id,
                &record.target_definition,
                &record.correspondence,
            );
        }
    }
    let mut result = BTreeMap::new();
    let mut ambiguous = BTreeSet::new();
    let mut visited = BTreeSet::new();
    for start in graph
        .keys()
        .filter(|(document, definition, _)| document == &source.id && definition == from)
    {
        if visited.contains(start) {
            continue;
        }
        let mut pending = vec![start.clone()];
        let mut source_ids = BTreeSet::new();
        let mut target_ids = BTreeSet::new();
        while let Some(point) = pending.pop() {
            if !visited.insert(point.clone()) {
                continue;
            }
            if point.0 == source.id && point.1 == from {
                source_ids.insert(point.2.clone());
            }
            if point.0 == target.id && point.1 == to {
                target_ids.insert(point.2.clone());
            }
            pending.extend(graph.get(&point).into_iter().flatten().cloned());
        }
        if source_ids.len() == 1 && target_ids.len() == 1 {
            result.insert(
                source_ids.into_iter().next().unwrap(),
                target_ids.into_iter().next().unwrap(),
            );
        } else if !target_ids.is_empty() {
            ambiguous.extend(source_ids);
        }
    }
    result.insert(from.into(), to.into());
    (result, ambiguous)
}

fn owned_ids(value: &Value, field: &str, ids: &mut BTreeSet<Id>) {
    match value {
        Value::Object(object) => {
            if let Some(id) = object.get("id").and_then(Value::as_str) {
                ids.insert(id.into());
            }
            if matches!(
                field,
                "points"
                    | "circles"
                    | "members"
                    | "sketches"
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
            ) {
                ids.extend(object.keys().cloned());
            }
            for (key, value) in object {
                if !matches!(
                    key.as_str(),
                    "properties" | "standard_part" | "source" | "resource"
                ) {
                    owned_ids(value, key, ids);
                }
            }
        }
        Value::Array(values) => {
            for value in values {
                owned_ids(value, field, ids);
            }
        }
        _ => {}
    }
}

fn remapped<T: Serialize + serde::de::DeserializeOwned>(
    value: &T,
    mapping: &BTreeMap<Id, Id>,
) -> Result<T> {
    let mut value = serde_json::to_value(value)?;
    crate::library::remap_source(&mut value, mapping, "")?;
    Ok(serde_json::from_value(value)?)
}

fn replace_expression_token(text: &str, name: &str, replacement: &str) -> String {
    let chars = text.chars().collect::<Vec<_>>();
    let mut out = String::new();
    let mut index = 0;
    while index < chars.len() {
        if chars[index] == '@' || chars[index].is_alphabetic() || chars[index] == '_' {
            let start = index;
            let explicit = chars[index] == '@';
            index += usize::from(explicit);
            let token_start = index;
            while index < chars.len()
                && (chars[index].is_alphanumeric()
                    || matches!(chars[index], '_' | '.' | ':')
                    || explicit && chars[index] == '-')
            {
                index += 1;
            }
            let token = chars[token_start..index].iter().collect::<String>();
            if token == name {
                out.push_str(replacement);
            } else {
                out.extend(chars[start..index].iter());
            }
        } else {
            out.push(chars[index]);
            index += 1;
        }
    }
    out
}
fn expression_container(field: &str, value: &Value) -> bool {
    if matches!(field, "definition" | "source" | "resource") && value.is_object() {
        return value.get("document_sha256").is_none() && value.get("sha256").is_none();
    }
    !matches!(
        field,
        "name"
            | "properties"
            | "metadata"
            | "standard_part"
            | "source"
            | "description"
            | "text"
            | "label"
            | "title"
            | "author"
            | "role"
            | "topology"
            | "path"
            | "source_path"
            | "resource_path"
            | "file_path"
            | "sha256"
            | "id"
            | "definition"
            | "node"
            | "material"
            | "finish"
            | "resource"
            | "configuration"
    )
}
/// Canonicalize qualified parameter expressions before changing definition ownership.
/// Human text, provenance, paths, and native reference labels are never rewritten.
pub(crate) fn normalize_definition_expressions(
    source: &Document,
    _owner: &str,
    value: &mut Value,
) -> Result<()> {
    match value {
        Value::Object(object) => {
            for (field, value) in object {
                if expression_container(field, value) {
                    normalize_definition_expressions(source, _owner, value)?;
                }
            }
        }
        Value::Array(values) => {
            for value in values {
                normalize_definition_expressions(source, _owner, value)?;
            }
        }
        Value::String(text) if text.contains("::") => {
            let Ok(names) = crate::expression::references(text) else {
                return Ok(());
            };
            for name in names {
                let Some((owner, key)) = name.rsplit_once("::") else {
                    continue;
                };
                let direct = source
                    .parts
                    .get(owner)
                    .map(|part| (&part.id, &part.parameters))
                    .or_else(|| {
                        source
                            .assemblies
                            .get(owner)
                            .map(|assembly| (&assembly.id, &assembly.parameters))
                    });
                let owners = source
                    .parts
                    .values()
                    .filter(|part| part.name == owner)
                    .map(|part| (&part.id, &part.parameters))
                    .chain(
                        source
                            .assemblies
                            .values()
                            .filter(|assembly| assembly.name == owner)
                            .map(|assembly| (&assembly.id, &assembly.parameters)),
                    )
                    .collect::<Vec<_>>();
                let (owner_id, parameters) = direct
                    .or_else(|| (owners.len() == 1).then(|| owners[0]))
                    .ok_or_else(|| {
                        invalid(format!(
                            "Parameter owner {owner} is missing or ambiguous in the source."
                        ))
                    })?;
                let keys = parameters
                    .iter()
                    .filter(|(_, parameter)| parameter.name == key)
                    .map(|(id, _)| id)
                    .collect::<Vec<_>>();
                let parameter = parameters
                    .get_key_value(key)
                    .map(|(id, _)| id)
                    .or_else(|| (keys.len() == 1).then(|| keys[0]))
                    .ok_or_else(|| {
                        invalid(format!(
                            "Parameter {key} is missing or ambiguous in the source."
                        ))
                    })?;
                *text = replace_expression_token(text, &name, &format!("@{owner_id}::{parameter}"));
            }
        }
        _ => {}
    }
    Ok(())
}

fn remapped_source<T: Serialize + serde::de::DeserializeOwned>(
    value: &T,
    source: &Document,
    owner: &str,
    mapping: &BTreeMap<Id, Id>,
    request: &Request,
) -> Result<T> {
    let mut value = serde_json::to_value(value)?;
    normalize_definition_expressions(source, owner, &mut value)?;
    crate::library::remap_source(&mut value, mapping, "")?;
    fn walk(value: &mut Value, mapping: &BTreeMap<Id, Id>, bindings: &[ParameterBinding]) {
        match value {
            Value::Object(object) => {
                for (field, value) in object {
                    if expression_container(field, value) {
                        walk(value, mapping, bindings);
                    }
                }
            }
            Value::Array(values) => {
                for value in values {
                    walk(value, mapping, bindings);
                }
            }
            Value::String(text) if text.contains("::") => {
                for binding in bindings {
                    let owner = mapping
                        .get(&binding.source.definition)
                        .unwrap_or(&binding.source.definition);
                    *text = replace_expression_token(
                        text,
                        &format!("{owner}::{}", binding.source.parameter),
                        &format!(
                            "@{}::{}",
                            binding.target.definition, binding.target.parameter
                        ),
                    );
                }
            }
            _ => {}
        }
    }
    walk(&mut value, mapping, &request.parameter_bindings);
    Ok(serde_json::from_value(value)?)
}

fn part_base(
    source: &Document,
    base: Option<&Document>,
    target: &Document,
    request: &Request,
) -> Option<Part> {
    fn normalized(document: &Document, part: &Part) -> Option<Part> {
        let mut context = document.clone();
        context.parts.insert(part.id.clone(), part.clone());
        let mut value = serde_json::to_value(part).ok()?;
        normalize_definition_expressions(&context, &part.id, &mut value).ok()?;
        serde_json::from_value(value).ok()
    }
    if let Some(part) = base.and_then(|d| d.parts.get(&request.source_definition)) {
        return normalized(base?, part);
    }
    let target_record = lineage(target, &request.target_definition);
    if let Some(record) = &target_record
        && record["source_document"].as_str() == Some(source.id.as_str())
        && record["source_definition"].as_str() == Some(request.source_definition.as_str())
    {
        let part = serde_json::from_value(record["source_snapshot"].clone()).ok()?;
        return normalized(source, &part);
    }
    let source_record = lineage(source, &request.source_definition)?;
    let original: Part = serde_json::from_value(source_record["source_snapshot"].clone()).ok()?;
    // The fork's original snapshot uses the original IDs, while comparisons use source IDs.
    remapped(
        &normalized(source, &original)?,
        &correspondence(&source_record),
    )
    .ok()
}

fn node_exists(document: &Document, node: &NodeRef) -> bool {
    document
        .parts
        .get(&node.definition)
        .is_some_and(|p| p.features.iter().any(|f| f.id == node.node))
}
fn mapped_node(node: &NodeRef, mapping: &BTreeMap<Id, Id>, bindings: &[NodeBinding]) -> NodeRef {
    if let Some(binding) = bindings.iter().find(|b| b.source == *node) {
        return binding.target.clone();
    }
    NodeRef {
        definition: mapping
            .get(&node.definition)
            .cloned()
            .unwrap_or_else(|| node.definition.clone()),
        node: mapping
            .get(&node.node)
            .cloned()
            .unwrap_or_else(|| node.node.clone()),
    }
}
fn bind_nodes(
    value: &mut Value,
    bindings: &[NodeBinding],
    mapping: &BTreeMap<Id, Id>,
) -> Result<()> {
    match value {
        Value::Object(object) => {
            // A body input and a face's construction producer are different references.
            // Rebinding a body must not invent a face on its replacement. Exact face/edge
            // replacement uses the separately reviewed Repair contract below.
            if object.contains_key("producer")
                && object.contains_key("role")
                && object.contains_key("topology")
            {
                return Ok(());
            }
            if object.len() == 2 && object.contains_key("definition") && object.contains_key("node")
            {
                let node: NodeRef = serde_json::from_value(Value::Object(object.clone()))?;
                if let Some(binding) = bindings.iter().find(|b| {
                    let mapped = mapped_node(&b.source, mapping, &[]);
                    node == mapped
                }) {
                    *value = serde_json::to_value(&binding.target)?;
                    return Ok(());
                }
            }
            for value in object.values_mut() {
                bind_nodes(value, bindings, mapping)?;
            }
        }
        Value::Array(values) => {
            for value in values {
                bind_nodes(value, bindings, mapping)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn choose<T: Serialize + Clone>(
    key: &str,
    label: &str,
    before: Option<&T>,
    ours: Option<&T>,
    theirs: &T,
    request: &Request,
    plan: &mut Plan,
) -> Result<Option<T>> {
    let o = ours.map(serde_json::to_value).transpose()?;
    let t = serde_json::to_value(theirs)?;
    let b = before.map(serde_json::to_value).transpose()?;
    if o.as_ref() == Some(&t) {
        return Ok(ours.cloned());
    }
    if o.is_none() && b.is_none() || o == b {
        return Ok(Some(theirs.clone()));
    }
    // Choosing an unchanged source must not undo a target-only edit or resurrect a deletion.
    if b.as_ref() == Some(&t) {
        return Ok(ours.cloned());
    }
    let resolution = request.resolutions.get(key).cloned();
    plan.conflicts.push(Conflict {
        key: key.into(),
        label: label.into(),
        message: if ours.is_none() {
            "The target deleted this item after the common version."
        } else if before.is_none() {
            "The target has a different item with this identity. No common value is available."
        } else {
            "Both designs changed this item after the common version."
        }
        .into(),
        resolution: resolution.clone(),
    });
    Ok(match resolution {
        Some(Resolution::UseSource) => Some(theirs.clone()),
        Some(Resolution::KeepTarget) | None => ours.cloned(),
    })
}

fn referenced_parameters(
    source: &Document,
    part: &Part,
    expressions: Vec<&crate::Expr>,
    request: &Request,
) -> Result<(BTreeSet<Id>, Vec<crate::ParameterRef>)> {
    let mut result = BTreeSet::new();
    let mut external = BTreeMap::new();
    let mut pending = Vec::new();
    for expression in expressions {
        pending.extend(crate::expression::references(&expression.0)?);
    }
    while let Some(name) = pending.pop() {
        let (owner, name) = name.rsplit_once("::").unwrap_or((&part.id, &name));
        if owner != part.id && owner != part.name {
            let mut expression = Value::String(format!("@{owner}::{name}"));
            normalize_definition_expressions(source, &part.id, &mut expression)?;
            let canonical = expression
                .as_str()
                .ok_or_else(|| invalid("Invalid parameter expression"))?
                .trim_start_matches('@');
            let (definition, parameter) = canonical
                .rsplit_once("::")
                .ok_or_else(|| invalid("Invalid parameter owner"))?;
            external.insert(
                (definition.to_string(), parameter.to_string()),
                crate::ParameterRef {
                    definition: definition.into(),
                    parameter: parameter.into(),
                },
            );
            continue;
        }
        let key = if part.parameters.contains_key(name) {
            Some(name.to_owned())
        } else {
            let matches = part
                .parameters
                .iter()
                .filter(|(_, p)| p.name == name)
                .map(|(id, _)| id)
                .collect::<Vec<_>>();
            if matches.len() > 1 {
                return Err(invalid(format!("Parameter name {name} is ambiguous.")));
            }
            matches.first().map(|id| (*id).clone())
        };
        if let Some(key) = key
            && result.insert(key.clone())
            && request.resolutions.get(&format!("parameter:{key}")) != Some(&Resolution::KeepTarget)
        {
            pending.extend(crate::expression::references(
                &part.parameters[&key].expression.0,
            )?);
        }
    }
    Ok((result, external.into_values().collect()))
}
fn parameter<'a>(
    document: &'a Document,
    reference: &crate::ParameterRef,
) -> Option<&'a crate::Parameter> {
    document
        .parts
        .get(&reference.definition)
        .map(|part| &part.parameters)
        .or_else(|| {
            document
                .assemblies
                .get(&reference.definition)
                .map(|assembly| &assembly.parameters)
        })?
        .get(&reference.parameter)
}

fn resource_ids(value: &Value, resources: &BTreeMap<Id, Resource>, output: &mut BTreeSet<Id>) {
    match value {
        Value::Object(object) => {
            for (key, value) in object {
                if key == "resource"
                    && let Some(id) = value.as_str()
                    && resources.contains_key(id)
                {
                    output.insert(id.into());
                }
                if !matches!(key.as_str(), "properties" | "source" | "metadata") {
                    resource_ids(value, resources, output);
                }
            }
        }
        Value::Array(values) => {
            for value in values {
                resource_ids(value, resources, output);
            }
        }
        _ => {}
    }
}

fn appearance_ids(value: &Value, materials: &mut BTreeSet<Id>, finishes: &mut BTreeSet<Id>) {
    match value {
        Value::Object(object) => {
            for (key, value) in object {
                if let Some(id) = value.as_str() {
                    if key == "material" {
                        materials.insert(id.into());
                    }
                    if key == "finish" {
                        finishes.insert(id.into());
                    }
                }
                if !matches!(key.as_str(), "properties" | "source" | "metadata") {
                    appearance_ids(value, materials, finishes);
                }
            }
        }
        Value::Array(values) => {
            for value in values {
                appearance_ids(value, materials, finishes);
            }
        }
        _ => {}
    }
}

/// Prepare a minimal transfer. Missing local dependencies are listed before they are added;
/// existing target inputs are reused unless the user asks to transfer their source change too.
pub fn prepare(
    source: &Document,
    base: Option<&Document>,
    target: &Document,
    request: &Request,
) -> Result<Plan> {
    let base = base.filter(|base| base.id == source.id);
    if request
        .bindings
        .iter()
        .map(|binding| &binding.source)
        .collect::<BTreeSet<_>>()
        .len()
        != request.bindings.len()
        || request
            .parameter_bindings
            .iter()
            .map(|binding| (&binding.source.definition, &binding.source.parameter))
            .collect::<BTreeSet<_>>()
            .len()
            != request.parameter_bindings.len()
    {
        return Err(invalid("Each source input must have only one replacement."));
    }
    let source_part = source
        .parts
        .get(&request.source_definition)
        .ok_or_else(|| invalid("Choose a source part."))?;
    let target_part = target
        .parts
        .get(&request.target_definition)
        .ok_or_else(|| invalid("Choose a target part."))?;
    if crate::library::is_definition_pinned(target, &target_part.id) {
        return Err(invalid(
            "Make the linked target part independent before changing its features.",
        ));
    }
    let mut plan = Plan {
        contract_version: CONTRACT_VERSION,
        source_fingerprint: source.fingerprint(),
        target_fingerprint: target.fingerprint(),
        source_definition: source_part.id.clone(),
        target_definition: target_part.id.clone(),
        items: vec![],
        conflicts: vec![],
        diagnostics: vec![],
        resources: vec![],
        reference_inputs: vec![],
        parameter_inputs: vec![],
        candidate: None,
    };
    if request.selected_features.is_empty() {
        plan.diagnostics
            .push("Select one or more source features.".into());
        return Ok(plan);
    }
    let receipt_key = receipt_key(source, &source_part.id, &target_part.id);
    let same_origin = |receipt: &&Receipt| {
        receipt.source_document == source.id
            && receipt.source_definition == source_part.id
            && receipt.target_definition == target_part.id
    };
    let previous_receipt = target
        .feature_transfers
        .get(&receipt_key)
        .or_else(|| target.feature_transfers.values().find(same_origin));
    let mut receipt = previous_receipt.cloned().unwrap_or_else(|| Receipt {
        version: CONTRACT_VERSION,
        source_document: source.id.clone(),
        source_definition: source_part.id.clone(),
        target_definition: target_part.id.clone(),
        correspondence: BTreeMap::new(),
        features: BTreeMap::new(),
        sketches: BTreeMap::new(),
        parameters: BTreeMap::new(),
        resources: BTreeMap::new(),
        materials: BTreeMap::new(),
        finishes: BTreeMap::new(),
    });
    let (mut mapping, ambiguous) =
        ancestry_mapping(source, target, &source_part.id, &target_part.id);
    if !ambiguous.is_empty() {
        plan.diagnostics.push(format!(
            "Saved ancestry gives conflicting identities for {} source item(s). Repair the ancestry records before transferring these related parts.",
            ambiguous.len()
        ));
    }
    let related = source_part.id == target_part.id || mapping.len() > 1;
    let base_part = related
        .then(|| {
            part_base(
                source,
                base.filter(|base| base.id == source.id),
                target,
                request,
            )
        })
        .flatten();
    let source_value = serde_json::to_value(source_part)?;
    let mut ids = BTreeSet::new();
    owned_ids(&source_value, "", &mut ids);
    for id in ids {
        mapping.entry(id.clone()).or_insert_with(|| {
            if related && source_part.id == target_part.id {
                id.clone()
            } else {
                stable_id(source, &source_part.id, &target_part.id, &id)
            }
        });
    }
    // Parameter keys are names, not global UUID identities. Preserve them and three-way check their values.
    for key in source_part.parameters.keys() {
        mapping.entry(key.clone()).or_insert_with(|| key.clone());
    }
    for binding in &request.bindings {
        if !node_exists(target, &binding.target) {
            plan.diagnostics.push(format!(
                "Replacement input {} no longer exists.",
                binding.target.node
            ));
        }
    }
    let source_features = source_part
        .features
        .iter()
        .map(|f| (&f.id, f))
        .collect::<BTreeMap<_, _>>();
    let mut included = request
        .selected_features
        .union(&request.include_dependencies)
        .cloned()
        .collect::<BTreeSet<_>>();
    let mut pending = included.iter().cloned().collect::<Vec<_>>();
    let mut dependencies = BTreeSet::new();
    while let Some(id) = pending.pop() {
        let Some(feature) = source_features.get(&id) else {
            plan.diagnostics
                .push(format!("Selected source feature {id} no longer exists."));
            continue;
        };
        let mapped = mapping.get(&id).unwrap_or(&id);
        let ours = target_part
            .features
            .iter()
            .find(|feature| &feature.id == mapped);
        if request.resolutions.get(&format!("feature:{id}")) == Some(&Resolution::KeepTarget)
            && ours.is_some()
        {
            continue;
        }
        let previous = previous_receipt
            .and_then(|receipt| receipt.features.get(&id))
            .or_else(|| {
                base_part
                    .as_ref()
                    .and_then(|part| part.features.iter().find(|feature| feature.id == id))
            });
        if previous == Some(*feature)
            && let Some(ours) = ours
        {
            let mapped_feature: Feature = remapped(*feature, &mapping)?;
            if ours.operation != mapped_feature.operation && request.bindings.is_empty() {
                continue;
            }
        }
        for node in source_part.feature_dependencies(feature) {
            let mapped = mapped_node(&node, &mapping, &request.bindings);
            if request.bindings.iter().any(|b| b.source == node) {
                continue;
            }
            if node.definition != source_part.id {
                if !node_exists(target, &mapped) {
                    plan.diagnostics.push(format!(
                        "Choose a target input for external feature {} in {}.",
                        node.node, node.definition
                    ));
                }
                continue;
            }
            if dependencies.insert(node.node.clone())
                && !node_exists(target, &mapped)
                && included.insert(node.node.clone())
            {
                pending.push(node.node);
            }
        }
    }
    // Include only selected features and required missing local inputs; never copy the entire source part.
    for feature in &source_part.features {
        if !included.contains(&feature.id) && !dependencies.contains(&feature.id) {
            continue;
        }
        let mapped = mapping
            .get(&feature.id)
            .cloned()
            .unwrap_or_else(|| feature.id.clone());
        let target_feature = target_part.features.iter().find(|f| f.id == mapped);
        let is_included = included.contains(&feature.id);
        plan.items.push(Item {
            key: format!("feature:{}", feature.id),
            source: feature.id.clone(),
            target: mapped,
            kind: "feature".into(),
            label: feature.name.clone(),
            selected: request.selected_features.contains(&feature.id),
            state: if !is_included {
                "reuse"
            } else if target_feature.is_some() {
                "compare"
            } else {
                "add"
            }
            .into(),
            message: if !is_included {
                "Uses the current target feature. Include it to transfer its upstream change."
            } else if !request.selected_features.contains(&feature.id) {
                "Required input for a selected feature."
            } else {
                "Selected for transfer."
            }
            .into(),
        });
    }
    let mut candidate = target.clone();
    let mut result = target_part.clone();
    let mut selected_sketches = BTreeSet::new();
    let mut required_resources = BTreeSet::new();
    let mut required_materials = BTreeSet::new();
    let mut required_finishes = BTreeSet::new();
    let mut expressions = Vec::new();
    let mut transferred = BTreeSet::new();
    for feature in source_part
        .features
        .iter()
        .filter(|f| included.contains(&f.id))
    {
        let mut incoming: Feature =
            remapped_source(feature, source, &source_part.id, &mapping, request)?;
        let mut op = serde_json::to_value(&incoming.operation)?;
        bind_nodes(&mut op, &request.bindings, &mapping)?;
        incoming.operation = serde_json::from_value(op)?;
        let before = previous_receipt
            .and_then(|r| r.features.get(&feature.id))
            .or_else(|| {
                base_part
                    .as_ref()
                    .and_then(|p| p.features.iter().find(|f| f.id == feature.id))
            })
            .map(|f| remapped(f, &mapping))
            .transpose()?;
        let ours = result.features.iter().find(|f| f.id == incoming.id);
        if let Some(chosen) = choose(
            &format!("feature:{}", feature.id),
            &feature.name,
            before.as_ref(),
            ours,
            &incoming,
            request,
            &mut plan,
        )? {
            if chosen.operation == incoming.operation
                && request.resolutions.get(&format!("feature:{}", feature.id))
                    != Some(&Resolution::KeepTarget)
            {
                if let Operation::Profile {
                    profile: Profile::Sketch { sketch },
                    ..
                } = &feature.operation
                {
                    selected_sketches.insert(sketch.clone());
                }
                expressions.extend(feature.operation.expressions());
                let value = serde_json::to_value(feature)?;
                resource_ids(&value, &source.resources, &mut required_resources);
                appearance_ids(&value, &mut required_materials, &mut required_finishes);
            }
            transferred.insert(chosen.id.clone());
            if let Some(index) = result.features.iter().position(|f| f.id == chosen.id) {
                result.features[index] = chosen;
            } else {
                result.features.push(chosen);
            }
        }
        receipt.features.insert(
            feature.id.clone(),
            remapped_source(
                feature,
                source,
                &source_part.id,
                &BTreeMap::new(),
                &Request::default(),
            )?,
        );
    }
    for id in selected_sketches {
        let sketch = source_part
            .sketches
            .get(&id)
            .ok_or_else(|| invalid("A selected profile has no source sketch."))?;
        let mut incoming: crate::Sketch =
            remapped_source(sketch, source, &source_part.id, &mapping, request)?;
        let mut value = serde_json::to_value(&incoming)?;
        bind_nodes(&mut value, &request.bindings, &mapping)?;
        incoming = serde_json::from_value(value)?;
        let before = previous_receipt
            .and_then(|r| r.sketches.get(&id))
            .or_else(|| base_part.as_ref().and_then(|p| p.sketches.get(&id)))
            .map(|s| remapped(s, &mapping))
            .transpose()?;
        let key = format!("sketch:{id}");
        let chosen = choose(
            &key,
            &sketch.name,
            before.as_ref(),
            result.sketches.get(&incoming.id),
            &incoming,
            request,
            &mut plan,
        )?;
        plan.items.push(Item {
            key,
            source: id.clone(),
            target: incoming.id.clone(),
            kind: "sketch".into(),
            label: sketch.name.clone(),
            state: "required".into(),
            selected: false,
            message: "Sketch used by a selected profile.".into(),
        });
        if let Some(chosen) = chosen {
            if request.resolutions.get(&format!("sketch:{id}")) != Some(&Resolution::KeepTarget)
                && (chosen == incoming || chosen.expressions() == sketch.expressions())
            {
                expressions.extend(sketch.expressions());
                resource_ids(
                    &serde_json::to_value(sketch)?,
                    &source.resources,
                    &mut required_resources,
                );
            }
            result.sketches.insert(chosen.id.clone(), chosen);
        }
        receipt.sketches.insert(
            id,
            remapped_source(
                sketch,
                source,
                &source_part.id,
                &BTreeMap::new(),
                &Request::default(),
            )?,
        );
    }
    let (local_parameters, external_parameters) =
        referenced_parameters(source, source_part, expressions, request)?;
    for reference in external_parameters {
        let source_parameter = parameter(source, &reference)
            .ok_or_else(|| invalid("An external source parameter is missing."))?;
        let target_reference = request
            .parameter_bindings
            .iter()
            .find(|binding| binding.source == reference)
            .map(|binding| binding.target.clone())
            .unwrap_or_else(|| crate::ParameterRef {
                definition: mapping
                    .get(&reference.definition)
                    .cloned()
                    .unwrap_or_else(|| reference.definition.clone()),
                parameter: reference.parameter.clone(),
            });
        let available = parameter(target, &target_reference)
            .filter(|parameter| parameter.dimension == source_parameter.dimension)
            .map(|_| target_reference);
        if available.is_none() {
            plan.diagnostics.push(format!(
                "Choose a target parameter for {}. The parameter must have the same units.",
                source_parameter.name
            ));
        }
        plan.parameter_inputs.push(ParameterInput {
            source: reference,
            label: source_parameter.name.clone(),
            dimension: source_parameter.dimension,
            target: available,
        });
    }
    for id in local_parameters {
        let parameter = &source_part.parameters[&id];
        let incoming: crate::Parameter =
            remapped_source(parameter, source, &source_part.id, &mapping, request)?;
        let before = previous_receipt
            .and_then(|r| r.parameters.get(&id))
            .or_else(|| base_part.as_ref().and_then(|p| p.parameters.get(&id)))
            .map(|p| remapped(p, &mapping))
            .transpose()?;
        let key = format!("parameter:{id}");
        let chosen = choose(
            &key,
            &parameter.name,
            before.as_ref(),
            result.parameters.get(&id),
            &incoming,
            request,
            &mut plan,
        )?;
        plan.items.push(Item {
            key,
            source: id.clone(),
            target: id.clone(),
            kind: "parameter".into(),
            label: parameter.name.clone(),
            state: "required".into(),
            selected: false,
            message: "Parameter used by a selected feature or sketch.".into(),
        });
        if let Some(parameter) = chosen {
            result.parameters.insert(id.clone(), parameter);
        }
        receipt.parameters.insert(
            id,
            remapped_source(
                parameter,
                source,
                &source_part.id,
                &BTreeMap::new(),
                &Request::default(),
            )?,
        );
    }
    for id in required_materials {
        let material = source
            .materials
            .get(&id)
            .ok_or_else(|| invalid("A selected feature uses a missing source material."))?;
        let incoming: crate::Material = remapped(material, &mapping)?;
        let before = previous_receipt
            .and_then(|r| r.materials.get(&id))
            .or_else(|| base.and_then(|d| d.materials.get(&id)))
            .map(|value| remapped(value, &mapping))
            .transpose()?;
        let key = format!("material:{id}");
        let chosen = choose(
            &key,
            &material.name,
            before.as_ref(),
            candidate.materials.get(&incoming.id),
            &incoming,
            request,
            &mut plan,
        )?;
        if let Some(chosen) = chosen {
            candidate.materials.insert(chosen.id.clone(), chosen);
        }
        plan.items.push(Item {
            key,
            source: id.clone(),
            target: incoming.id,
            kind: "material".into(),
            label: material.name.clone(),
            state: "required".into(),
            selected: false,
            message: "Material assigned to a transferred feature.".into(),
        });
        receipt.materials.insert(id, material.clone());
    }
    for id in required_finishes {
        let finish = source
            .finishes
            .get(&id)
            .ok_or_else(|| invalid("A selected feature uses a missing source finish."))?;
        let incoming: crate::appearance::Finish = remapped(finish, &mapping)?;
        let before = previous_receipt
            .and_then(|r| r.finishes.get(&id))
            .or_else(|| base.and_then(|d| d.finishes.get(&id)))
            .map(|value| remapped(value, &mapping))
            .transpose()?;
        let key = format!("finish:{id}");
        let chosen = choose(
            &key,
            &finish.name,
            before.as_ref(),
            candidate.finishes.get(&incoming.id),
            &incoming,
            request,
            &mut plan,
        )?;
        if let Some(chosen) = chosen {
            if chosen == incoming {
                required_resources.extend(finish.textures.resources().cloned());
            }
            candidate.finishes.insert(chosen.id.clone(), chosen);
        }
        plan.items.push(Item {
            key,
            source: id.clone(),
            target: incoming.id,
            kind: "finish".into(),
            label: finish.name.clone(),
            state: "required".into(),
            selected: false,
            message: "Finish assigned to a transferred feature.".into(),
        });
        receipt.finishes.insert(id, finish.clone());
    }
    for id in required_resources {
        let resource = source
            .resources
            .get(&id)
            .ok_or_else(|| invalid("A selected feature uses a missing source resource."))?;
        let incoming: Resource = remapped(resource, &mapping)?;
        let key = format!("resource:{id}");
        let before = previous_receipt
            .and_then(|r| r.resources.get(&id))
            .or_else(|| base.and_then(|d| d.resources.get(&id)))
            .map(|value| remapped(value, &mapping))
            .transpose()?;
        let current_resource = candidate.resources.get(&incoming.id);
        // Storage paths are local adapter details; compare the pinned bytes and resource class.
        let mut before = before;
        if let Some(before) = &mut before
            && let Some(current) = current_resource
            && before.sha256 == current.sha256
            && before.kind == current.kind
        {
            before.path = current.path.clone();
        }
        let chosen = if current_resource
            .is_some_and(|r| r.sha256 == resource.sha256 && r.kind == resource.kind)
        {
            current_resource.cloned()
        } else {
            choose(
                &key,
                &resource.path,
                before.as_ref(),
                current_resource,
                &incoming,
                request,
                &mut plan,
            )?
        };
        if let Some(mut chosen) = chosen {
            if chosen.sha256 == incoming.sha256 && chosen.kind == incoming.kind {
                // Imported resource paths are content-addressed to avoid overwriting another resource's bytes.
                let extension = std::path::Path::new(&resource.path)
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("bin");
                chosen.path = format!(
                    "resources/feature-transfer/{}.{}",
                    resource.sha256, extension
                );
                plan.resources.push(ResourceCopy {
                    source: resource.clone(),
                    target: chosen.clone(),
                });
            }
            candidate.resources.insert(incoming.id.clone(), chosen);
        }
        plan.items.push(Item {
            key,
            source: id.clone(),
            target: incoming.id,
            kind: "resource".into(),
            label: resource.path.clone(),
            state: "required".into(),
            selected: false,
            message: "Pinned resource used by the selected definitions.".into(),
        });
        receipt.resources.insert(id, resource.clone());
    }
    // Stable topological insertion preserves unrelated target feature order whenever dependencies permit.
    let mut ordered = Vec::new();
    let mut remaining = result.features.clone();
    let local_ids = remaining
        .iter()
        .map(|f| f.id.clone())
        .collect::<BTreeSet<_>>();
    let mut emitted = BTreeSet::new();
    while !remaining.is_empty() {
        let next = remaining.iter().position(|f| {
            result.feature_dependencies(f).iter().all(|d| {
                d.definition != result.id
                    || !local_ids.contains(&d.node)
                    || emitted.contains(&d.node)
            })
        });
        let Some(index) = next else {
            plan.diagnostics
                .push("The transferred features create a dependency cycle.".into());
            break;
        };
        let feature = remaining.remove(index);
        emitted.insert(feature.id.clone());
        ordered.push(feature);
    }
    if remaining.is_empty() {
        result.features = ordered;
    }
    if request.reconnect_outputs {
        for feature in &result.features {
            if !transferred.contains(&feature.id)
                || !(feature.operation.produces_solid() || feature.operation.is_surface())
            {
                continue;
            }
            let consumed = feature.operation.consumed_outputs();
            if !request.allow_parallel_outputs
                && !target_part
                    .features
                    .iter()
                    .any(|existing| existing.id == feature.id)
            {
                for input in consumed
                    .iter()
                    .filter(|input| input.definition == target_part.id)
                {
                    if let Some(output) = target_part.outputs.iter().find(|output| {
                        *output != &input.node && body_ancestor(target_part, output, &input.node)
                    }) {
                        let label = target_part
                            .features
                            .iter()
                            .find(|feature| &feature.id == output)
                            .map(|feature| feature.name.as_str())
                            .unwrap_or("current body");
                        plan.diagnostics.push(format!("{} uses an earlier body stage. Bind its body input to {label}, or explicitly allow separate results. This prevents overlapping copies of the part.", feature.name));
                    }
                }
            }
            result.outputs.retain(|id| {
                !consumed
                    .iter()
                    .any(|n| n.definition == result.id && n.node == *id)
            });
            // A downstream target operation already consuming this stage remains the visible output.
            let has_consumer = result.features.iter().any(|other| {
                other.id != feature.id
                    && other
                        .operation
                        .consumed_outputs()
                        .iter()
                        .any(|n| n.definition == result.id && n.node == feature.id)
            });
            if !has_consumer && !result.outputs.contains(&feature.id) {
                result.outputs.push(feature.id.clone());
            }
        }
    }
    candidate.parts.insert(result.id.clone(), result);
    let mut transferred_ids = BTreeSet::from([source_part.id.clone()]);
    for feature in receipt.features.values() {
        owned_ids(&serde_json::to_value(feature)?, "", &mut transferred_ids);
    }
    for sketch in receipt.sketches.values() {
        owned_ids(&serde_json::to_value(sketch)?, "", &mut transferred_ids);
    }
    transferred_ids.extend(receipt.parameters.keys().cloned());
    transferred_ids.extend(dependencies);
    receipt.correspondence.extend(
        mapping
            .iter()
            .filter(|(from, _)| transferred_ids.contains(*from))
            .map(|(from, to)| (from.clone(), to.clone())),
    );
    candidate.feature_transfers.retain(|_, previous| {
        previous.source_document != source.id
            || previous.source_definition != source_part.id
            || previous.target_definition != target_part.id
    });
    candidate.feature_transfers.insert(receipt_key, receipt);
    for id in &transferred {
        plan.reference_inputs
            .extend(crate::reference_repair::inputs(
                &candidate,
                &NodeRef {
                    definition: target_part.id.clone(),
                    node: id.clone(),
                },
            )?);
    }
    if request.repairs.iter().any(|repair| {
        repair.input.owner.definition != target_part.id
            || !transferred.contains(&repair.input.owner.node)
    }) {
        plan.diagnostics
            .push("Reference replacements must belong to transferred features.".into());
    } else if !request.repairs.is_empty()
        && let Err(error) = crate::reference_repair::repair(&mut candidate, &request.repairs)
    {
        plan.diagnostics.push(error.to_string());
    }
    // Retain newly imported correspondence so subsequent upstream/downstream transfers remain related.
    if let Some(record) = candidate.component_forks.get_mut(&target_part.id)
        && record.source_document == source.id
        && record.source_definition == source_part.id
    {
        for (from, to) in &mapping {
            if transferred_ids.contains(from) {
                record.correspondence.insert(from.clone(), to.clone());
            }
        }
    }
    if let Err(error) = candidate.validate() {
        plan.diagnostics.push(error.to_string());
    }
    if plan.diagnostics.is_empty() && plan.conflicts.iter().all(|c| c.resolution.is_some()) {
        plan.candidate = Some(candidate);
    }
    Ok(plan)
}

fn body_ancestor(part: &Part, output: &str, ancestor: &str) -> bool {
    let mut pending = vec![output.to_string()];
    let mut visited = BTreeSet::new();
    while let Some(id) = pending.pop() {
        if id == ancestor {
            return true;
        }
        if !visited.insert(id.clone()) {
            continue;
        }
        if let Some(feature) = part.features.iter().find(|feature| feature.id == id) {
            pending.extend(
                feature
                    .operation
                    .consumed_outputs()
                    .into_iter()
                    .filter(|node| node.definition == part.id)
                    .map(|node| node.node),
            );
        }
    }
    false
}

/// Copy only resource bytes named by a prepared plan. Hash verification precedes every write.
pub fn materialize(
    plan: &Plan,
    source_root: &std::path::Path,
    target_root: &std::path::Path,
) -> Result<()> {
    use std::io::Write;
    let candidate = plan
        .candidate
        .as_ref()
        .ok_or_else(|| invalid("The transfer plan is not ready."))?;
    fn checked(
        root: &std::path::Path,
        relative: &str,
        make_parents: bool,
    ) -> Result<std::path::PathBuf> {
        let root = root.canonicalize()?;
        let path = crate::persistence::safe_join(&root, relative)?;
        let mut current = root.clone();
        let components = std::path::Path::new(relative)
            .components()
            .collect::<Vec<_>>();
        for (index, component) in components.iter().enumerate() {
            current.push(component.as_os_str());
            let metadata = match std::fs::symlink_metadata(&current) {
                Ok(metadata) => Some(metadata),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    if make_parents && index + 1 < components.len() {
                        match std::fs::create_dir(&current) {
                            Ok(()) => {}
                            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                            Err(error) => return Err(error.into()),
                        }
                        Some(std::fs::symlink_metadata(&current)?)
                    } else {
                        None
                    }
                }
                Err(error) => return Err(error.into()),
            };
            if let Some(metadata) = metadata
                && (metadata.file_type().is_symlink()
                    || index + 1 < components.len() && !metadata.is_dir()
                    || index + 1 == components.len() && !metadata.is_file())
            {
                return Err(invalid(
                    "Transfer resource paths must use regular files and directories, without symbolic links.",
                ));
            }
        }
        Ok(path)
    }
    for copy in &plan.resources {
        if candidate.resources.get(&copy.target.id) != Some(&copy.target)
            || copy.target.sha256 != copy.source.sha256
            || copy.target.kind != copy.source.kind
            || copy.source.sha256.len() != 64
            || !copy
                .source
                .sha256
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
            || !copy.target.path.starts_with(&format!(
                "resources/feature-transfer/{}.",
                copy.source.sha256
            ))
            || copy.target.path.matches('/').count() != 2
        {
            return Err(invalid(
                "A resource copy does not match the reviewed candidate and its content-addressed destination.",
            ));
        }
        let bytes = std::fs::read(checked(source_root, &copy.source.path, false)?)?;
        if format!("{:x}", Sha256::digest(&bytes)) != copy.source.sha256 {
            return Err(invalid(format!(
                "Resource {} does not match its pinned hash.",
                copy.source.path
            )));
        }
        let path = checked(target_root, &copy.target.path, true)?;
        if path.exists() {
            if format!("{:x}", Sha256::digest(std::fs::read(&path)?)) != copy.target.sha256 {
                return Err(invalid(
                    "A transfer resource path already contains different bytes.",
                ));
            }
        } else {
            let temporary = path.with_file_name(format!(".transfer-{}.tmp", crate::id()));
            let result = (|| -> Result<()> {
                let mut file = std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&temporary)?;
                file.write_all(&bytes)?;
                file.sync_all()?;
                // A hard link publishes a complete file atomically and fails if the target appeared.
                match std::fs::hard_link(&temporary, &path) {
                    Ok(()) => Ok(()),
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                        let path = checked(target_root, &copy.target.path, false)?;
                        if format!("{:x}", Sha256::digest(std::fs::read(path)?))
                            == copy.target.sha256
                        {
                            Ok(())
                        } else {
                            Err(invalid(
                                "A different resource appeared at the destination during the transfer.",
                            ))
                        }
                    }
                    Err(error) => Err(error.into()),
                }
            })();
            let _ = std::fs::remove_file(&temporary);
            result?;
        }
    }
    Ok(())
}
