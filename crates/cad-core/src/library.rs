//! External components are pinned source snapshots; updates are deliberate document edits.
use crate::{
    persistence::{FileStore, ProjectStore, atomic_write, project_root, safe_join},
    *,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ComponentLink {
    pub id: Id,
    pub name: String,
    pub source_path: String,
    pub source_document: Id,
    pub source_definition: Id,
    pub source_revision: u64,
    pub source_sha256: String,
    pub snapshot_resource: Id,
    pub definition: Id,
    pub pinned_definitions: BTreeMap<Id, String>,
}
fn hash(bytes: impl AsRef<[u8]>) -> String {
    format!("{:x}", Sha256::digest(bytes.as_ref()))
}
fn mapped(link: &str, id: &str) -> String {
    let h = hash(format!("{link}:{id}"));
    format!(
        "{}-{}-{}-{}-{}",
        &h[..8],
        &h[8..12],
        &h[12..16],
        &h[16..20],
        &h[20..32]
    )
}
fn source_path(root: &Path, path: &str) -> PathBuf {
    let p = Path::new(path);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        root.join(p)
    }
}
pub fn available_update(link: &ComponentLink, root: &Path) -> Result<Option<(u64, String)>> {
    let source = FileStore.load(&source_path(root, &link.source_path))?;
    Ok((source.fingerprint() != link.source_sha256)
        .then(|| (source.revision, source.fingerprint())))
}
pub fn validate_pins(doc: &Document) -> Result<()> {
    for (key, link) in &doc.libraries {
        if key != &link.id || !doc.resources.contains_key(&link.snapshot_resource) {
            return Err(CadError::Invalid(
                "Broken component library snapshot".into(),
            ));
        }
        for (definition, pinned) in &link.pinned_definitions {
            let bytes = if let Some(part) = doc.parts.get(definition) {
                serde_json::to_vec(part)?
            } else if let Some(assembly) = doc.assemblies.get(definition) {
                serde_json::to_vec(assembly)?
            } else {
                return Err(CadError::Invalid(format!(
                    "Missing pinned component {definition}"
                )));
            };
            if &hash(bytes) != pinned {
                return Err(CadError::Invalid(format!(
                    "{} is a pinned component. Make an instance unique or explicitly update its library reference",
                    link.name
                )));
            }
        }
    }
    Ok(())
}
/// Copies a complete source snapshot, preserving its dependency closure and immutable resource bytes.
/// Stable IDs are namespaced by the link identity, so updates retain instance and feature references.
pub fn link(
    doc: &mut Document,
    root: &Path,
    source: &Path,
    definition: Option<&str>,
    update: Option<&str>,
) -> Result<Id> {
    let source = source_path(root, &source.to_string_lossy());
    let src = FileStore.load(&source)?;
    let old = update
        .map(|id| {
            doc.libraries
                .get(id)
                .cloned()
                .ok_or_else(|| CadError::Invalid("Unknown component link".into()))
        })
        .transpose()?;
    if let Some(old) = &old
        && old.source_document != src.id
    {
        return Err(CadError::Invalid(
            "An update must retain the source document identity".into(),
        ));
    }
    let source_definition = definition
        .map(str::to_string)
        .or_else(|| old.as_ref().map(|l| l.source_definition.clone()))
        .unwrap_or_else(|| src.root.clone());
    if !src.parts.contains_key(&source_definition)
        && !src.assemblies.contains_key(&source_definition)
    {
        return Err(CadError::Invalid(
            "Unknown source component definition".into(),
        ));
    }
    let link_id = old.as_ref().map(|l| l.id.clone()).unwrap_or_else(id);
    let mut mapping = BTreeMap::new();
    for key in src
        .parts
        .keys()
        .chain(src.assemblies.keys())
        .chain(src.materials.keys())
        .chain(src.finishes.keys())
        .chain(src.resources.keys())
    {
        mapping.insert(key.clone(), mapped(&link_id, key));
    }
    for part in src.parts.values() {
        for f in &part.features {
            mapping.insert(f.id.clone(), mapped(&link_id, &f.id));
            if let crate::Operation::ReferenceCurve { definition } = &f.operation {
                for key in definition.source_ids() {
                    mapping.insert(key.clone(), mapped(&link_id, key));
                }
            }
            if let crate::Operation::Surface {
                operation: crate::surface::SurfaceOperation::Ruled { sources, .. },
            } = &f.operation
            {
                for source in sources {
                    mapping.insert(source.id.clone(), mapped(&link_id, &source.id));
                }
            }
            if let crate::Operation::PlacementPattern { definition } = &f.operation {
                for record in &definition.placements {
                    mapping.insert(record.id.clone(), mapped(&link_id, &record.id));
                }
            }
        }
        for sketch in part.sketches.values() {
            mapping.insert(sketch.id.clone(), mapped(&link_id, &sketch.id));
            for key in sketch.points.keys() {
                mapping.insert(key.clone(), mapped(&link_id, key));
            }
            for key in sketch.members.keys() {
                mapping.insert(key.clone(), mapped(&link_id, key));
            }
            for key in sketch.circles.keys() {
                mapping.insert(key.clone(), mapped(&link_id, key));
            }
            for c in &sketch.constraints {
                mapping.insert(c.id.clone(), mapped(&link_id, &c.id));
            }
            for key in sketch
                .offsets
                .keys()
                .chain(sketch.fillets.keys())
                .chain(sketch.chamfers.keys())
                .chain(sketch.transforms.keys())
                .chain(sketch.texts.keys())
                .chain(sketch.equations.keys())
                .chain(sketch.blocks.keys())
                .chain(sketch.block_instances.keys())
                .chain(sketch.dimension_groups.keys())
            {
                mapping.insert(key.clone(), mapped(&link_id, key));
            }
            for block in sketch.blocks.values() {
                for key in block.points.keys().chain(block.members.keys()) {
                    mapping.insert(key.clone(), mapped(&link_id, key));
                }
                for constraint in &block.constraints {
                    mapping.insert(constraint.id.clone(), mapped(&link_id, &constraint.id));
                }
            }
        }
    }
    for assembly in src.assemblies.values() {
        for i in assembly.instances.values() {
            mapping.insert(i.id.clone(), mapped(&link_id, &i.id));
        }
        for m in &assembly.mates {
            mapping.insert(m.id.clone(), mapped(&link_id, &m.id));
        }
        for view in assembly.exploded_views.values() {
            mapping.insert(view.id.clone(), mapped(&link_id, &view.id));
            for step in &view.steps {
                mapping.insert(step.id.clone(), mapped(&link_id, &step.id));
            }
        }
        for inspection in assembly.inspections.values() {
            mapping.insert(inspection.id.clone(), mapped(&link_id, &inspection.id));
        }
        for weld in assembly.weld_beads.values() {
            mapping.insert(weld.id.clone(), mapped(&link_id, &weld.id));
        }
        for group in assembly.rigid_groups.values() {
            mapping.insert(group.id.clone(), mapped(&link_id, &group.id));
        }
        for pattern in assembly.component_patterns.values() {
            mapping.insert(pattern.id.clone(), mapped(&link_id, &pattern.id));
            for mate in &pattern.mates {
                mapping.insert(mate.mate.clone(), mapped(&link_id, &mate.mate));
            }
        }
    }
    for owner in src.parts.keys().chain(src.assemblies.keys()) {
        for key in crate::configuration::names(&src, owner).keys() {
            mapping.insert(key.clone(), mapped(&link_id, key));
        }
    }
    let mut json = serde_json::to_value(&src)?;
    // Default finishes are omitted from ordinary source to preserve legacy hashes.
    // A linked snapshot must still namespace them and every default assignment.
    json["finishes"] = serde_json::to_value(&src.finishes)?;
    for part in src.parts.values() {
        json["parts"][&part.id]["finish"] = serde_json::Value::String(part.finish.clone());
    }
    // Drawings and review state are not part of the component dependency closure.
    json["drawings"] = serde_json::json!([]);
    json["comments"] = serde_json::json!([]);
    json["libraries"] = serde_json::json!({});
    remap_source(&mut json, &mapping, "")?;
    let mut imported: Document = serde_json::from_value(json)?;
    let source_root = project_root(&source);
    for resource in imported.resources.values_mut() {
        let bytes = std::fs::read(safe_join(&source_root, &resource.path)?)?;
        if hash(&bytes) != resource.sha256 {
            return Err(CadError::Invalid(
                "The component resource changed since its source revision".into(),
            ));
        }
        let extension = Path::new(&resource.path)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("bin");
        resource.path = format!("imports/{}.{}", resource.sha256, extension);
        atomic_write(&safe_join(root, &resource.path)?, &bytes)?;
    }
    if let Some(old) = &old {
        for definition in old.pinned_definitions.keys() {
            doc.parts.remove(definition);
            doc.assemblies.remove(definition);
        }
    }
    // Definition selections are host state. Preserve existing host choices on an
    // explicit update; adopt source selections only for newly linked definitions.
    for (definition, configuration) in &imported.active_configurations {
        doc.active_configurations
            .entry(definition.clone())
            .or_insert_with(|| configuration.clone());
    }
    if let Some(old) = &old {
        for definition in old.pinned_definitions.keys() {
            if let Some(configuration) = doc.active_configurations.get(definition)
                && !crate::configuration::exists(&imported, definition, configuration)
            {
                return Err(CadError::Invalid("The library update removes the host-selected configuration; choose another configuration before updating".into()));
            }
        }
    }
    let mut pins = BTreeMap::new();
    for (key, part) in imported.parts {
        pins.insert(key.clone(), hash(serde_json::to_vec(&part)?));
        doc.parts.insert(key, part);
    }
    for (key, assembly) in imported.assemblies {
        pins.insert(key.clone(), hash(serde_json::to_vec(&assembly)?));
        doc.assemblies.insert(key, assembly);
    }
    doc.materials.extend(imported.materials);
    doc.finishes.extend(imported.finishes);
    doc.resources.extend(imported.resources);
    let bytes = serde_json::to_vec_pretty(&src)?;
    let sha = hash(&bytes);
    let snapshot = mapped(&link_id, "source-snapshot");
    let path = format!("libraries/{sha}.json");
    atomic_write(&safe_join(root, &path)?, &bytes)?;
    doc.resources.insert(
        snapshot.clone(),
        Resource {
            id: snapshot.clone(),
            path,
            sha256: sha,
            kind: "component_snapshot".into(),
        },
    );
    let definition = mapping[&source_definition].clone();
    let name = crate::presentation::definition_name(&src, &source_definition);
    doc.libraries.insert(
        link_id.clone(),
        ComponentLink {
            id: link_id.clone(),
            name: name.clone(),
            source_path: source.to_string_lossy().into(),
            source_document: src.id.clone(),
            source_definition,
            source_revision: src.revision,
            source_sha256: src.fingerprint(),
            snapshot_resource: snapshot,
            definition: definition.clone(),
            pinned_definitions: pins,
        },
    );
    if old.is_none() {
        let instance = id();
        doc.assemblies
            .get_mut(&doc.root)
            .ok_or_else(|| CadError::Invalid("Linking needs an assembly root".into()))?
            .instances
            .insert(
                instance.clone(),
                Instance {
                    id: instance,
                    name,
                    definition,
                    placement: Default::default(),
                    finish: None,
                    configuration: None,
                    fixed: false,
                    translation_expressions: None,
                },
            );
    }
    doc.validate()?;
    Ok(link_id)
}

/// Remap native source identities and explicit parameter owners while preserving
/// human-authored text. Map shapes are source contracts, not arbitrary user objects.
pub(crate) fn remap_source(
    value: &mut serde_json::Value,
    mapping: &BTreeMap<String, String>,
    field: &str,
) -> Result<()> {
    remap_source_inner(value, mapping, field, false)
}
/// A unique part retains sketch/member/configuration/material identities.
/// Only feature/definition ownership and explicit parameter owners change.
pub(crate) fn remap_owned_features(
    value: &mut serde_json::Value,
    mapping: &BTreeMap<String, String>,
    field: &str,
) -> Result<()> {
    remap_source_inner(value, mapping, field, true)
}
fn remap_source_inner(
    value: &mut serde_json::Value,
    mapping: &BTreeMap<String, String>,
    field: &str,
    owned_features: bool,
) -> Result<()> {
    fn literal(field: &str) -> bool {
        matches!(
            field,
            "name"
                | "description"
                | "properties"
                | "title"
                | "title_block"
                | "annotations"
                | "text"
                | "author"
                | "approved_by"
                | "revision"
                | "date"
                | "part_number"
                | "headers"
                | "rows"
                | "label"
                | "hint"
                | "reason"
                | "source_path"
                | "source_project"
                | "snapshot_path"
                | "resource_path"
                | "file_path"
                | "filename"
                | "source_sha256"
                | "sha256"
                | "role"
                | "topology"
        )
    }
    fn identity_map(field: &str, owned_features: bool) -> bool {
        if owned_features {
            return matches!(field, "features" | "feature_dimensions");
        }
        matches!(
            field,
            "parts"
                | "assemblies"
                | "materials"
                | "finishes"
                | "resources"
                | "instances"
                | "sketches"
                | "points"
                | "circles"
                | "members"
                | "conversions"
                | "offsets"
                | "fillets"
                | "source_curves"
                | "arc_through"
                | "chamfers"
                | "transforms"
                | "splines"
                | "conics"
                | "texts"
                | "equations"
                | "blocks"
                | "block_instances"
                | "dimension_groups"
                | "configuration_details"
                | "configurations"
                | "active_configurations"
                | "features"
                | "feature_dimensions"
                | "components"
                | "mates"
                | "component_configurations"
                | "component_placements"
                | "mate_curve_seeds"
                | "component_patterns"
                | "exploded_views"
                | "inspections"
                | "weld_beads"
                | "assembly_welds"
                | "rigid_groups"
        )
    }
    fn reference_text(
        key: &str,
        object: &serde_json::Map<String, serde_json::Value>,
        field: &str,
        owned_features: bool,
    ) -> bool {
        if owned_features {
            return matches!(key, "definition" | "node" | "outputs")
                || key == "id"
                    && (object.contains_key("operation")
                        || object.contains_key("features") && object.contains_key("sketches"));
        }
        match key {
            "id" | "definition" | "root" | "configuration" | "parent" | "node" | "material"
            | "finish" | "resource" | "instance" | "relative_to" | "sketch" | "outputs"
            | "profile_order" | "path" | "instance_path" | "seed" | "seeds" | "mate" | "source"
            | "sources" | "segments" | "entities" | "point" | "member" | "members" | "points"
            | "through" | "controls" | "lines" | "trim" | "arc" | "arcs" | "anchor" | "support" => {
                true
            }
            "base_color" | "normal" | "roughness" | "metallic" | "occlusion" | "height" => {
                field == "textures" && object.get(key).is_some_and(serde_json::Value::is_string)
            }
            "datum" | "constraint" | "source_record" => true,
            "placement" => {
                object.get("operation").and_then(serde_json::Value::as_str)
                    == Some("placement_pattern")
            }
            "dynamic_excluded" | "target_vertex" | "path_member" | "path_start_vertex"
            | "vertex" | "start_vertex" | "source_members" | "outer_members" | "region_member" => {
                true
            }
            "first" | "second" => {
                object.contains_key("first_origin") && object.contains_key("second_origin")
            }
            "center" => {
                matches!(
                    object.get("kind").and_then(|v| v.as_str()),
                    Some("circle" | "ellipse" | "ellipse_arc" | "arc_slot")
                ) || object.contains_key("radius")
            }
            "start" | "end" => matches!(
                object.get("kind").and_then(|v| v.as_str()),
                Some("line" | "arc" | "arc_slot")
            ),
            "major" | "minor" => object.get("kind").and_then(|v| v.as_str()) == Some("ellipse_arc"),
            "corner" => object.contains_key("lines"),
            _ => false,
        }
    }
    fn walk(
        value: &mut serde_json::Value,
        mapping: &BTreeMap<String, String>,
        field: &str,
        is_reference: bool,
        owned_features: bool,
    ) -> Result<()> {
        if (literal(field)
            || field.ends_with("_path") && value.is_string() && !is_reference
            || field.ends_with("_hint"))
            && !(field == "role" && value.is_object())
        {
            return Ok(());
        }
        match value {
            serde_json::Value::Object(object) => {
                if field == "resource"
                    && object
                        .get("resource")
                        .is_some_and(serde_json::Value::is_string)
                    && object
                        .get("sha256")
                        .is_some_and(serde_json::Value::is_string)
                    && object
                        .get("source")
                        .and_then(|source| source.get("document_sha256"))
                        .is_some_and(serde_json::Value::is_string)
                {
                    // Pinned block-package provenance describes the exporting
                    // source, not identities in this importing document.
                    if let Some(local_ids) = object
                        .get_mut("local_ids")
                        .and_then(serde_json::Value::as_object_mut)
                    {
                        for identity in local_ids.values_mut() {
                            if let Some(replacement) =
                                identity.as_str().and_then(|identity| mapping.get(identity))
                            {
                                *identity = replacement.clone().into();
                            }
                        }
                    }
                    return Ok(());
                }
                if object.contains_key("producer")
                    && object.contains_key("topology")
                    && let Some(role) = object.get("role").and_then(|v| v.as_str())
                {
                    let role = remap_role(role, mapping, owned_features)?;
                    object.insert("role".into(), role.into());
                }
                if object.get("contract").and_then(|v| v.as_str()) == Some("hole-intersection")
                    && let Some(carriers) = object
                        .get_mut("intent")
                        .and_then(|v| v.get_mut("carriers"))
                        .and_then(serde_json::Value::as_array_mut)
                {
                    // The generic walk below remaps first; final canonical sorting follows it.
                    if carriers.len() != 2 {
                        return Err(CadError::Invalid(
                            "Hole intersection role requires two carriers".into(),
                        ));
                    }
                }
                let source = std::mem::take(object);
                for (key, mut value) in source.iter().map(|(k, v)| (k.clone(), v.clone())) {
                    let refs = reference_text(&key, &source, field, owned_features)
                        || !owned_features
                            && (matches!(
                                field,
                                "active_configurations" | "component_configurations"
                            ) || matches!(field, "points" | "members" | "arc_through")
                                && value.is_string());
                    // Dictionary keys are native identities or parameter names, not schema field names.
                    let child_field = if field == "feature_dimensions" {
                        "__feature_dimension_fields"
                    } else if field == "__feature_dimension_fields" {
                        "__feature_dimension_expression"
                    } else if matches!(field, "parameters" | "__parameter_expressions") {
                        "__parameter_expression"
                    } else if matches!(field, "active_configurations" | "component_configurations")
                        || matches!(field, "points" | "members" | "arc_through")
                            && value.is_string()
                    {
                        "__native_reference"
                    } else if identity_map(field, false) {
                        if matches!(field, "configurations")
                            && value.as_object().is_some_and(|o| {
                                !o.contains_key("id")
                                    && o.values().all(serde_json::Value::is_string)
                            })
                        {
                            "__parameter_expressions"
                        } else {
                            "__native_record"
                        }
                    } else {
                        &key
                    };
                    // File paths are literal strings; nested component paths are ID arrays.
                    if !(key == "path" && value.is_string() && child_field == key.as_str()) {
                        walk(&mut value, mapping, child_field, refs, owned_features)?;
                    }
                    let key = if field == "__feature_dimension_fields" {
                        key.split('/')
                            .map(|segment| {
                                let decoded = segment.replace("~1", "/").replace("~0", "~");
                                decoded
                                    .strip_prefix("@id=")
                                    .and_then(|source| mapping.get(source))
                                    .map(|id| {
                                        format!("@id={id}").replace('~', "~0").replace('/', "~1")
                                    })
                                    .unwrap_or_else(|| segment.into())
                            })
                            .collect::<Vec<_>>()
                            .join("/")
                    } else if identity_map(field, owned_features) {
                        mapping.get(&key).cloned().unwrap_or(key)
                    } else {
                        key
                    };
                    object.insert(key, value);
                }
                if object.get("contract").and_then(|v| v.as_str()) == Some("hole-intersection")
                    && let Some(carriers) = object
                        .get_mut("intent")
                        .and_then(|v| v.get_mut("carriers"))
                        .and_then(serde_json::Value::as_array_mut)
                {
                    carriers.sort_by_cached_key(serde_json::Value::to_string);
                }
                if object.get("contract").and_then(|v| v.as_str()) == Some("construction")
                    && let Some(sources) = object
                        .get_mut("intent")
                        .and_then(|value| value.get_mut("sources"))
                        .and_then(serde_json::Value::as_array_mut)
                {
                    sources.sort_by_cached_key(serde_json::Value::to_string);
                }
                if let Some(members) = object
                    .get_mut("source_members")
                    .and_then(serde_json::Value::as_array_mut)
                {
                    members.sort_by_cached_key(serde_json::Value::to_string);
                }
                for field in ["stock_sources", "carrier_sources"] {
                    if let Some(sources) = object
                        .get_mut(field)
                        .and_then(serde_json::Value::as_array_mut)
                    {
                        sources.sort_by_cached_key(serde_json::Value::to_string);
                    }
                }
            }
            serde_json::Value::Array(array) => {
                for value in array {
                    let field = if value.is_object() {
                        "__native_record"
                    } else {
                        field
                    };
                    walk(value, mapping, field, is_reference, owned_features)?;
                }
            }
            serde_json::Value::String(text) => {
                if is_reference && let Some(id) = mapping.get(text) {
                    *text = id.clone();
                }
                for (old, new) in mapping {
                    *text = text.replace(&format!("@{old}::"), &format!("@{new}::"));
                }
            }
            _ => {}
        }
        Ok(())
    }
    walk(value, mapping, field, false, owned_features)
}

/// Construction roles may embed source identities; they are versioned grammar,
/// never free-form display labels. JSON payloads retain nested TopologyRefs.
fn remap_role(
    role: &str,
    mapping: &BTreeMap<String, String>,
    owned_features: bool,
) -> Result<String> {
    let Some((prefix, payload)) = role.split_once(':') else {
        return Ok(role.into());
    };
    if !matches!(
        prefix,
        "pattern" | "hole-position" | "hole-intersection" | "construction"
    ) {
        return Ok(role.into());
    }
    if payload.starts_with('{') {
        let mut value: serde_json::Value = serde_json::from_str(payload)
            .map_err(|_| CadError::Invalid("Malformed saved construction-role payload".into()))?;
        remap_source_inner(&mut value, mapping, "", owned_features)?;
        if prefix == "hole-intersection" {
            let carriers = value
                .get_mut("carriers")
                .and_then(serde_json::Value::as_array_mut)
                .ok_or_else(|| {
                    CadError::Invalid("Hole intersection role has no carrier identities".into())
                })?;
            if carriers.len() != 2 {
                return Err(CadError::Invalid(
                    "Hole intersection role requires two carriers".into(),
                ));
            }
            carriers.sort_by_cached_key(serde_json::Value::to_string);
        }
        if prefix == "construction" {
            let sources = value
                .get_mut("sources")
                .and_then(serde_json::Value::as_array_mut)
                .ok_or_else(|| {
                    CadError::Invalid("Constructed topology role has no source identities".into())
                })?;
            sources.sort_by_cached_key(serde_json::Value::to_string);
        }
        return Ok(format!("{prefix}:{}", serde_json::to_string(&value)?));
    }
    match prefix {
        "hole-position" => {
            let fields = payload.split(':').collect::<Vec<_>>();
            if fields.len()!=3 || !matches!(fields[1],"edge"|"face") || fields[2].parse::<usize>().is_err() {
                return Err(CadError::Unsupported("Ambiguous legacy hole-position provenance; reselect this reference".into()));
            }
            Ok(format!("hole-position:{}:{}:{}",(!owned_features).then(||mapping.get(fields[0])).flatten().map(String::as_str).unwrap_or(fields[0]),fields[1],fields[2]))
        }
        "pattern" => {
            let fields = payload.splitn(8,':').collect::<Vec<_>>();
            if fields.len()!=8 || fields[0].parse::<usize>().is_err() || fields[1].parse::<usize>().is_err() || fields[2]!="body" || fields[3].parse::<usize>().is_err() || !matches!(fields[4],"edge"|"face") {
                return Err(CadError::Unsupported("Ambiguous legacy pattern provenance; reselect this reference".into()));
            }
            Ok(format!("pattern:{}:{}:body:{}:{}:{}:{}:{}",fields[0],fields[1],fields[3],fields[4],mapping.get(fields[5]).map(String::as_str).unwrap_or(fields[5]),mapping.get(fields[6]).map(String::as_str).unwrap_or(fields[6]),remap_role(fields[7],mapping,owned_features)?))
        }
        _ => Err(CadError::Unsupported("Ambiguous legacy hole-intersection provenance cannot be safely remapped; reselect this reference".into())),
    }
}
