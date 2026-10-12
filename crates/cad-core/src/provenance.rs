//! Application-owned, self-contained topology source closures.
//!
//! A closure stores each complete source descriptor once, with structural
//! table links instead of recursively escaped copies. Table order is canonical
//! and never derives from positions, native topology IDs, or digests. Legacy
//! tree roles remain readable and normalize to the same identity as closures.
use crate::{CadError, Result, TopologyRef};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};

pub const CONTRACT: &str = "tcad.topology-closure";
pub const VERSION: u64 = 1;
const LINK: &str = "$tcad.source";
const MAX_BYTES: usize = 16 * 1024 * 1024;
const MAX_WORK: usize = 1_048_576;
const MAX_REFERENCES: usize = 65_536;
const MAX_DEPTH: usize = 256;

fn invalid(message: &str) -> CadError {
    CadError::Invalid(format!("Topology source closure: {message}"))
}
fn exhausted(kind: &str) -> CadError {
    CadError::Unsupported(format!(
        "Topology provenance: {kind} budget exhausted while canonicalizing source closure"
    ))
}

pub fn is_closure(value: &Value) -> bool {
    value.get("contract").and_then(Value::as_str) == Some(CONTRACT)
}
fn link(index: usize) -> Value {
    serde_json::json!({LINK:index})
}
fn link_index(value: &Value) -> Result<Option<usize>> {
    let Some(object) = value.as_object() else {
        return Ok(None);
    };
    let Some(index) = object.get(LINK) else {
        return Ok(None);
    };
    if object.len() != 1 {
        return Err(invalid("A source link has extra fields"));
    }
    index
        .as_u64()
        .and_then(|n| usize::try_from(n).ok())
        .map(Some)
        .ok_or_else(|| invalid("A source link needs a nonnegative table index"))
}
fn structured(role: &str) -> Option<(&str, &str)> {
    let (contract, payload) = role.split_once(':')?;
    (matches!(
        contract,
        "construction" | "pattern" | "hole-position" | "hole-intersection"
    ) && payload.trim_start().starts_with('{'))
    .then_some((contract, payload))
}
fn source(value: &Value) -> bool {
    value.get("producer").is_some_and(Value::is_object)
        && value.get("role").is_some()
        && value.get("topology").is_some_and(Value::is_string)
}

struct Encoder {
    work: usize,
    bytes: usize,
    nodes: Vec<Value>,
    depths: Vec<usize>,
    keys: BTreeMap<String, usize>,
}
impl Encoder {
    fn new() -> Self {
        Self {
            work: MAX_WORK,
            bytes: MAX_BYTES,
            nodes: vec![],
            depths: vec![],
            keys: BTreeMap::new(),
        }
    }
    fn visit(&mut self, depth: usize) -> Result<()> {
        if depth > MAX_DEPTH {
            return Err(exhausted("Source depth (256)"));
        }
        self.work = self
            .work
            .checked_sub(1)
            .ok_or_else(|| exhausted("Source traversal (1048576)"))?;
        Ok(())
    }
    fn encoded(&mut self, value: &Value) -> Result<String> {
        let text = bounded_json(value, self.bytes)?;
        self.bytes = self
            .bytes
            .checked_sub(text.len())
            .ok_or_else(|| exhausted("Distinct source serialization (16 MiB)"))?;
        Ok(text)
    }
    fn parse(&mut self, text: &str) -> Result<Value> {
        if text.len() > MAX_BYTES {
            return Err(exhausted("Legacy source input (16 MiB)"));
        }
        self.work = self
            .work
            .checked_sub(text.len().div_ceil(64))
            .ok_or_else(|| exhausted("Legacy source parsing (1048576)"))?;
        serde_json::from_str(text).map_err(|_| {
            invalid("A structured source contains malformed or excessively nested JSON")
        })
    }
    fn signature(&mut self, value: Value, depth: usize) -> Result<Value> {
        self.visit(depth)?;
        // A descriptor owns its own constructor grammar; a closure owns its
        // table scope. Their ordinary literals must not be interpreted as
        // incidence descriptor strings by this legacy signature decoder.
        if source(&value) || is_closure(&value) {
            return Ok(value);
        }
        match value {
            Value::String(text) if text.starts_with('{') || text.starts_with('[') => {
                let parsed = self.parse(&text)?;
                self.signature(parsed, depth + 1)
            }
            Value::Array(values) => values
                .into_iter()
                .map(|v| self.signature(v, depth + 1))
                .collect::<Result<Vec<_>>>()
                .map(Value::Array),
            Value::Object(values) => values
                .into_iter()
                .map(|(k, v)| Ok((k, self.signature(v, depth + 1)?)))
                .collect::<Result<Map<_, _>>>()
                .map(Value::Object),
            value => Ok(value),
        }
    }
    fn fold(&mut self, mut value: Value, depth: usize, allow_links: bool) -> Result<Value> {
        self.visit(depth)?;
        if is_closure(&value) {
            return self.import(value, depth + 1);
        }
        if let Some(index) = link_index(&value)? {
            if !allow_links || index >= self.nodes.len() {
                return Err(invalid("A source link appears outside its table"));
            }
            return Ok(value);
        }
        if source(&value) {
            let object = value.as_object_mut().unwrap();
            object.remove("signature");
            if let Some(role) = object.get("role").and_then(Value::as_str)
                && let Some((contract, payload)) = structured(role)
            {
                let contract = contract.to_owned();
                let intent = self.parse(payload)?;
                object.insert(
                    "role".into(),
                    serde_json::json!({"contract":contract,"intent":intent}),
                );
            }
            let object = std::mem::take(object);
            let folded = object
                .into_iter()
                .map(|(k, v)| Ok((k, self.fold(v, depth + 1, allow_links)?)))
                .collect::<Result<Map<_, _>>>()?;
            return self.intern(Value::Object(folded));
        }
        if let Some(purpose) = value.get_mut("purpose")
            && purpose.get("operation").and_then(Value::as_str) == Some("topology")
            && matches!(
                purpose.get("constructor").and_then(Value::as_str),
                Some("source_boundary_component" | "source_endpoint_component")
            )
            && let Some(signature) = purpose.get_mut("source_signature")
        {
            *signature = self.signature(std::mem::take(signature), depth + 1)?;
        }
        match value {
            Value::Object(values) => values
                .into_iter()
                .map(|(k, v)| Ok((k, self.fold(v, depth + 1, allow_links)?)))
                .collect::<Result<Map<_, _>>>()
                .map(Value::Object),
            Value::Array(values) => values
                .into_iter()
                .map(|v| self.fold(v, depth + 1, allow_links))
                .collect::<Result<Vec<_>>>()
                .map(Value::Array),
            value => Ok(value),
        }
    }
    fn intern(&mut self, value: Value) -> Result<Value> {
        // Dependencies have already been interned. Their indices are private
        // join keys here; finish assigns content-canonical indices.
        let key = bounded_json(&value, MAX_BYTES)?;
        if let Some(index) = self.keys.get(&key) {
            return Ok(link(*index));
        }
        if self.nodes.len() >= MAX_REFERENCES {
            return Err(exhausted("Distinct source descriptors (65536)"));
        }
        self.bytes = self
            .bytes
            .checked_sub(key.len())
            .ok_or_else(|| exhausted("Distinct source serialization (16 MiB)"))?;
        let mut dependencies = BTreeSet::new();
        links(&value, &mut dependencies)?;
        let depth = dependencies
            .into_iter()
            .map(|i| {
                self.depths
                    .get(i)
                    .copied()
                    .ok_or_else(|| invalid("A source link is unresolved"))
            })
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .max()
            .unwrap_or(0)
            + 1;
        if depth > MAX_DEPTH {
            return Err(exhausted("Source ancestry depth (256)"));
        }
        let index = self.nodes.len();
        self.nodes.push(value);
        self.depths.push(depth);
        self.keys.insert(key, index);
        Ok(link(index))
    }
    fn import(&mut self, mut value: Value, depth: usize) -> Result<Value> {
        if value.get("version").and_then(Value::as_u64) != Some(VERSION) {
            return Err(invalid("Unsupported source-closure version"));
        }
        let root = value
            .get_mut("root")
            .map(std::mem::take)
            .ok_or_else(|| invalid("The source closure has no root"))?;
        let references = value
            .get_mut("references")
            .and_then(Value::as_array_mut)
            .map(std::mem::take)
            .ok_or_else(|| invalid("The source closure has no reference table"))?;
        if references.len() > MAX_REFERENCES {
            return Err(exhausted("Imported source descriptors (65536)"));
        }
        let mut mapped = Vec::with_capacity(references.len());
        for (index, reference) in references.into_iter().enumerate() {
            self.visit(depth)?;
            if !source(&reference) {
                return Err(invalid(
                    "A closure table entry is not a complete source descriptor",
                ));
            }
            let reference = replace_links(reference, &mapped, index)?;
            // Imported links already address our table. Do not re-import them
            // as free-standing values; the descriptor is ready to intern.
            mapped.push(self.fold(reference, depth + 1, true)?);
        }
        let root = replace_links(root, &mapped, mapped.len())?;
        Ok(root)
    }
    fn finish(mut self, root: Value) -> Result<Value> {
        // Imported tables may contain entries unused by their root. They are
        // not ancestry and must neither change identity nor displace the
        // canonical indices of reachable descriptors.
        let mut reachable = BTreeSet::new();
        links(&root, &mut reachable)?;
        let mut pending = reachable.iter().copied().collect::<Vec<_>>();
        while let Some(index) = pending.pop() {
            self.visit(0)?;
            let node = self
                .nodes
                .get(index)
                .ok_or_else(|| invalid("The closure root has a missing source"))?;
            let mut dependencies = BTreeSet::new();
            links(node, &mut dependencies)?;
            for dependency in dependencies {
                if reachable.insert(dependency) {
                    pending.push(dependency);
                }
            }
        }
        let mut mapped = vec![None; self.nodes.len()];
        let mut references = vec![];
        let mut levels = BTreeMap::<usize, Vec<usize>>::new();
        for (index, depth) in self.depths.iter().copied().enumerate() {
            if reachable.contains(&index) {
                levels.entry(depth).or_default().push(index);
            }
        }
        for (_, old_indices) in levels {
            let mut level = BTreeMap::<String, (Value, Vec<usize>)>::new();
            for old in old_indices {
                self.visit(0)?;
                let mut value = canonical_links(self.nodes[old].clone(), &mapped)?;
                canonicalize(&mut value)?;
                let key = self.encoded(&value)?;
                level
                    .entry(key)
                    .or_insert_with(|| (value, vec![]))
                    .1
                    .push(old);
            }
            for (_, (value, old_indices)) in level {
                let index = references.len();
                references.push(value);
                for old in old_indices {
                    mapped[old] = Some(index);
                }
            }
        }
        let mut root = canonical_links(root, &mapped)?;
        canonicalize(&mut root)?;
        let closure = serde_json::json!({"contract":CONTRACT,"version":VERSION,"root":root,"references":references});
        bounded_json(&closure, MAX_BYTES)?;
        Ok(closure)
    }
}

fn bounded_json(value: &Value, limit: usize) -> Result<String> {
    struct Buffer {
        bytes: Vec<u8>,
        limit: usize,
    }
    impl std::io::Write for Buffer {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if self.bytes.len().saturating_add(bytes.len()) > self.limit {
                return Err(std::io::Error::other("Topology source serialization limit"));
            }
            self.bytes.extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut buffer = Buffer {
        bytes: vec![],
        limit,
    };
    serde_json::to_writer(&mut buffer, value)
        .map_err(|_| exhausted("Closure output serialization (16 MiB)"))?;
    String::from_utf8(buffer.bytes).map_err(|_| invalid("Source serialization is not UTF-8"))
}
fn links(value: &Value, result: &mut BTreeSet<usize>) -> Result<()> {
    if let Some(index) = link_index(value)? {
        result.insert(index);
        return Ok(());
    }
    match value {
        Value::Array(values) => {
            for v in values {
                links(v, result)?;
            }
        }
        Value::Object(values) => {
            for v in values.values() {
                links(v, result)?;
            }
        }
        _ => {}
    }
    Ok(())
}
fn replace_links(value: Value, mapped: &[Value], before: usize) -> Result<Value> {
    // A nested closure owns its own table indices.
    if is_closure(&value) {
        return Ok(value);
    }
    if let Some(index) = link_index(&value)? {
        if index >= before {
            return Err(invalid(
                "A closure source link is cyclic or forward-referencing",
            ));
        }
        return mapped
            .get(index)
            .cloned()
            .ok_or_else(|| invalid("A closure source link is outside its table"));
    }
    match value {
        Value::Array(values) => values
            .into_iter()
            .map(|v| replace_links(v, mapped, before))
            .collect::<Result<Vec<_>>>()
            .map(Value::Array),
        Value::Object(values) => values
            .into_iter()
            .map(|(k, v)| Ok((k, replace_links(v, mapped, before)?)))
            .collect::<Result<Map<_, _>>>()
            .map(Value::Object),
        value => Ok(value),
    }
}
fn canonical_links(value: Value, mapped: &[Option<usize>]) -> Result<Value> {
    if let Some(index) = link_index(&value)? {
        return mapped
            .get(index)
            .copied()
            .flatten()
            .map(link)
            .ok_or_else(|| invalid("Source closure has a missing dependency"));
    }
    match value {
        Value::Array(values) => values
            .into_iter()
            .map(|v| canonical_links(v, mapped))
            .collect::<Result<Vec<_>>>()
            .map(Value::Array),
        Value::Object(values) => values
            .into_iter()
            .map(|(k, v)| Ok((k, canonical_links(v, mapped)?)))
            .collect::<Result<Map<_, _>>>()
            .map(Value::Object),
        value => Ok(value),
    }
}
fn sorted_set(values: &mut Vec<Value>) {
    values.sort_by_cached_key(Value::to_string);
    values.dedup();
}
fn sorted_incidence(values: &mut [Value]) {
    values.sort_by_cached_key(Value::to_string);
}
/// Reorder constructor source sets while preserving explicit source-table
/// indices. This is also used after library ownership/member ID remapping.
pub(crate) fn canonicalize(value: &mut Value) -> Result<()> {
    match value {
        Value::Array(values) => {
            for v in values {
                canonicalize(v)?;
            }
        }
        Value::Object(values) => {
            for v in values.values_mut() {
                canonicalize(v)?;
            }
            if let Some(Value::Array(sources)) = values.get_mut("sources") {
                let before = sources.clone();
                sorted_set(sources);
                let indices = sources
                    .iter()
                    .enumerate()
                    .map(|(index, source)| (source.to_string(), index))
                    .collect::<BTreeMap<_, _>>();
                let mapping = before
                    .iter()
                    .map(|old| indices[&old.to_string()])
                    .collect::<Vec<_>>();
                if let Some(purpose) = values.get_mut("purpose")
                    && purpose.get("source_table").and_then(Value::as_str)
                        == Some("canonical_construction_sources")
                {
                    fn index(value: &mut Value, mapping: &[usize]) -> Result<()> {
                        let old = value
                            .as_u64()
                            .and_then(|n| usize::try_from(n).ok())
                            .ok_or_else(|| {
                                invalid("A constructor source-table index is malformed")
                            })?;
                        *value = mapping
                            .get(old)
                            .copied()
                            .ok_or_else(|| {
                                invalid("A constructor source-table index is outside its table")
                            })?
                            .into();
                        Ok(())
                    }
                    if let Some(intervals) =
                        purpose.get_mut("intervals").and_then(Value::as_array_mut)
                    {
                        for interval in intervals.iter_mut() {
                            index(
                                interval
                                    .as_array_mut()
                                    .and_then(|v| v.first_mut())
                                    .ok_or_else(|| {
                                        invalid("A constructor interval is malformed")
                                    })?,
                                &mapping,
                            )?;
                        }
                        sorted_set(intervals);
                    }
                    if let Some(planes) = purpose.get_mut("planes").and_then(Value::as_array_mut) {
                        for plane in planes {
                            let plane = plane.as_array_mut().ok_or_else(|| {
                                invalid("A constructor plane source list is malformed")
                            })?;
                            for source in plane.iter_mut() {
                                index(source, &mapping)?;
                            }
                            sorted_set(plane);
                        }
                    }
                }
            }
            if let Some(carriers) = values.get_mut("carriers").and_then(Value::as_array_mut) {
                sorted_set(carriers);
            }
            if let Some(purpose) = values.get_mut("purpose")
                && purpose.get("operation").and_then(Value::as_str) == Some("topology")
            {
                let constructor = purpose
                    .get("constructor")
                    .and_then(Value::as_str)
                    .map(str::to_owned);
                if let Some(signature) = purpose
                    .get_mut("source_signature")
                    .and_then(Value::as_array_mut)
                {
                    // Only incidence sets are unordered. Tuple slots retain
                    // their distinct edge, neighbor, and endpoint meaning.
                    match constructor.as_deref() {
                        Some("source_boundary_component") => {
                            for wire in signature.iter_mut() {
                                if let Some(wire) = wire.as_array_mut() {
                                    for boundary in wire.iter_mut() {
                                        if let Some(neighbors) = boundary
                                            .as_array_mut()
                                            .and_then(|tuple| tuple.get_mut(1))
                                            .and_then(Value::as_array_mut)
                                        {
                                            sorted_set(neighbors);
                                        }
                                    }
                                    sorted_incidence(wire);
                                }
                            }
                            sorted_incidence(signature);
                        }
                        Some("source_endpoint_component") if signature.len() == 2 => {
                            if let Some(incident) = signature[0].as_array_mut() {
                                sorted_set(incident);
                            }
                            if let Some(ends) = signature[1].as_array_mut() {
                                for end in ends.iter_mut() {
                                    if let Some(tuple) = end.as_array_mut() {
                                        for part in tuple.iter_mut().take(2) {
                                            if let Some(set) = part.as_array_mut() {
                                                sorted_set(set);
                                            }
                                        }
                                        if let Some(originals) =
                                            tuple.get_mut(2).and_then(Value::as_array_mut)
                                        {
                                            for original in originals.iter_mut() {
                                                if let Some(star) = original.as_array_mut() {
                                                    for part in star {
                                                        if let Some(set) = part.as_array_mut() {
                                                            sorted_set(set);
                                                        }
                                                    }
                                                }
                                            }
                                            sorted_set(originals);
                                        }
                                    }
                                }
                                sorted_incidence(ends);
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
        _ => {}
    }
    Ok(())
}

/// Canonicalize a construction intent into a bounded, readable source DAG.
pub fn construction_payload(value: Value) -> Result<Value> {
    let mut encoder = Encoder::new();
    let root = encoder.fold(value, 0, false)?;
    encoder.finish(root)
}
/// Build an incidence signature without copying every source descriptor into
/// every incidence tuple. Root links address this input table; the result
/// imports nested closures and assigns canonical content-based table order.
pub fn indexed_closure(root: Value, references: Vec<Value>) -> Result<Value> {
    if references.len() > MAX_REFERENCES {
        return Err(exhausted("Indexed source descriptors (65536)"));
    }
    let mut encoder = Encoder::new();
    let mapped = references
        .into_iter()
        .map(|value| encoder.fold(value, 0, false))
        .collect::<Result<Vec<_>>>()?;
    let root = replace_links(root, &mapped, mapped.len())?;
    let root = encoder.fold(root, 0, true)?;
    encoder.finish(root)
}
/// Return a source descriptor with a canonical structured role. Native
/// numerical signatures deliberately do not participate in source identity.
pub fn normalized_reference(reference: &TopologyRef) -> Result<Value> {
    let role = if let Some((contract, payload)) = structured(&reference.role) {
        if payload.len() > MAX_BYTES {
            return Err(exhausted("Legacy source input (16 MiB)"));
        }
        let value = serde_json::from_str(payload)
            .map_err(|_| invalid("A structured source role is malformed or excessively nested"))?;
        serde_json::json!({"contract":contract,"intent":construction_payload(value)?})
    } else {
        Value::String(reference.role.clone())
    };
    Ok(serde_json::json!({"producer":reference.producer,"role":role,"topology":reference.topology}))
}
pub fn equivalent(first: &TopologyRef, second: &TopologyRef) -> Result<bool> {
    if first.producer != second.producer || first.topology != second.topology {
        return Ok(false);
    }
    if first.role == second.role {
        return Ok(true);
    }
    Ok(normalized_reference(first)? == normalized_reference(second)?)
}
/// A readable canonical map key, for saved-reference lookup tables. No digest
/// or current geometric signature participates in this identity.
pub fn canonical_key(reference: &TopologyRef) -> Result<String> {
    bounded_json(&normalized_reference(reference)?, MAX_BYTES)
}
/// Reuse a wanted descriptor canonicalized once for a bounded candidate search.
pub fn matches_normalized(
    actual: &TopologyRef,
    wanted: &TopologyRef,
    canonical: &Value,
) -> Result<bool> {
    if actual.producer != wanted.producer || actual.topology != wanted.topology {
        return Ok(false);
    }
    if actual.role == wanted.role {
        return Ok(true);
    }
    Ok(normalized_reference(actual)? == *canonical)
}
