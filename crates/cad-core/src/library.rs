//! External components are pinned source snapshots; updates are deliberate document edits.
use crate::{
    persistence::{FileStore, ProjectStore, atomic_write, project_root, safe_join},
    *,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};
mod snapshot_integrity;
pub use snapshot_integrity::{
    SnapshotProof, finish_is_pinned, hydrate_pins, material_is_pinned, replace_command, semantic,
    validate_replacement, validate_verified_replacement, verified_source,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pinned_materials: Option<BTreeMap<Id, String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pinned_finishes: Option<BTreeMap<Id, String>>,
    /// An immutable commit and an explicit update channel. Fetch never changes this pin.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git_source: Option<GitSource>,
    /// New links import only the selected definition's dependency closure.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub definition_closure: bool,
    /// Runtime source availability, never part of authoritative hashes.
    #[serde(skip)]
    pub pin_diagnostic: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct GitSource {
    pub remote: String,
    #[serde(default)]
    pub project_path: String,
    pub commit: String,
    pub update_ref: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub configuration: Option<Id>,
}
impl GitSource {
    pub fn validate(&self) -> Result<()> {
        crate::version_control::validate_remote(&self.remote)?;
        safe_join(Path::new("."), &self.project_path)?;
        if !matches!(self.commit.len(), 40 | 64)
            || !self.commit.bytes().all(|b| b.is_ascii_hexdigit())
            || self.update_ref.trim().is_empty()
            || self.update_ref.starts_with('-')
            || self.update_ref.chars().any(char::is_control)
        {
            return Err(CadError::Invalid(
                "Git component sources require an exact commit and a valid update reference".into(),
            ));
        }
        Ok(())
    }
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
    if let Some(git) = &link.git_source {
        let repository = git_repository(root, &git.remote, false)?;
        let source =
            crate::version_control::snapshot_at(&repository, &git.update_ref, &git.project_path)?;
        return Ok((source.fingerprint() != link.source_sha256)
            .then(|| (source.revision, source.fingerprint())));
    }
    let source = FileStore.load(&source_path(root, &link.source_path))?;
    Ok((source.fingerprint() != link.source_sha256)
        .then(|| (source.revision, source.fingerprint())))
}
pub fn is_definition_pinned(doc: &Document, definition: &str) -> bool {
    doc.libraries
        .values()
        .any(|link| link.pinned_definitions.contains_key(definition))
}
pub fn validate_pins(doc: &Document) -> Result<()> {
    if doc.libraries.len() > 4096 {
        return Err(CadError::Invalid(
            "At most 4096 component snapshot links are supported".into(),
        ));
    }
    for (key, link) in &doc.libraries {
        snapshot_integrity::validate_link_record(doc, link)?;
        if key != &link.id {
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
        snapshot_integrity::validate_material_finish_pins(doc, link)?;
    }
    Ok(())
}
pub(crate) fn namespace_snapshot(
    src: &Document,
    link_id: &str,
) -> Result<(Document, BTreeMap<Id, Id>)> {
    let mut mapping = BTreeMap::new();
    for key in src
        .parts
        .keys()
        .chain(src.assemblies.keys())
        .chain(src.materials.keys())
        .chain(src.finishes.keys())
        .chain(src.resources.keys())
    {
        mapping.insert(key.clone(), mapped(link_id, key));
    }
    for part in src.parts.values() {
        for assignment in &part.face_finishes {
            mapping.insert(assignment.id.clone(), mapped(link_id, &assignment.id));
        }
        for f in &part.features {
            mapping.insert(f.id.clone(), mapped(link_id, &f.id));
            if let crate::Operation::ReferenceCurve { definition } = &f.operation {
                for key in definition.source_ids() {
                    mapping.insert(key.clone(), mapped(link_id, key));
                }
            }
            if let crate::Operation::Sketch3d { definition } = &f.operation {
                for key in definition.source_ids() {
                    mapping.insert(key.clone(), mapped(link_id, key));
                }
            }
            if let crate::Operation::Surface {
                operation: crate::surface::SurfaceOperation::Ruled { sources, .. },
            } = &f.operation
            {
                for source in sources {
                    mapping.insert(source.id.clone(), mapped(link_id, &source.id));
                }
            }
            if let crate::Operation::PlacementPattern { definition } = &f.operation {
                for record in &definition.placements {
                    mapping.insert(record.id.clone(), mapped(link_id, &record.id));
                }
            }
        }
        for sketch in part.sketches.values() {
            mapping.insert(sketch.id.clone(), mapped(link_id, &sketch.id));
            for key in sketch.points.keys() {
                mapping.insert(key.clone(), mapped(link_id, key));
            }
            for key in sketch.members.keys() {
                mapping.insert(key.clone(), mapped(link_id, key));
            }
            for key in sketch.circles.keys() {
                mapping.insert(key.clone(), mapped(link_id, key));
            }
            for c in &sketch.constraints {
                mapping.insert(c.id.clone(), mapped(link_id, &c.id));
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
                mapping.insert(key.clone(), mapped(link_id, key));
            }
            for block in sketch.blocks.values() {
                for key in block.points.keys().chain(block.members.keys()) {
                    mapping.insert(key.clone(), mapped(link_id, key));
                }
                for constraint in &block.constraints {
                    mapping.insert(constraint.id.clone(), mapped(link_id, &constraint.id));
                }
            }
        }
    }
    for assembly in src.assemblies.values() {
        for state in assembly.display_states.values() {
            mapping.insert(state.id.clone(), mapped(link_id, &state.id));
            for target in &state.overrides {
                mapping.insert(target.id.clone(), mapped(link_id, &target.id));
            }
        }
        for i in assembly.instances.values() {
            mapping.insert(i.id.clone(), mapped(link_id, &i.id));
        }
        for m in &assembly.mates {
            mapping.insert(m.id.clone(), mapped(link_id, &m.id));
        }
        for view in assembly.exploded_views.values() {
            mapping.insert(view.id.clone(), mapped(link_id, &view.id));
            for step in &view.steps {
                mapping.insert(step.id.clone(), mapped(link_id, &step.id));
            }
        }
        for inspection in assembly.inspections.values() {
            mapping.insert(inspection.id.clone(), mapped(link_id, &inspection.id));
        }
        for weld in assembly.weld_beads.values() {
            mapping.insert(weld.id.clone(), mapped(link_id, &weld.id));
        }
        for group in assembly.rigid_groups.values() {
            mapping.insert(group.id.clone(), mapped(link_id, &group.id));
        }
        for pattern in assembly.component_patterns.values() {
            mapping.insert(pattern.id.clone(), mapped(link_id, &pattern.id));
            for mate in &pattern.mates {
                mapping.insert(mate.mate.clone(), mapped(link_id, &mate.mate));
            }
        }
    }
    for owner in src.parts.keys().chain(src.assemblies.keys()) {
        for key in crate::configuration::names(src, owner).keys() {
            mapping.insert(key.clone(), mapped(link_id, key));
        }
    }
    let mut json = serde_json::to_value(src)?;
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
    // Scene camera bookmarks are not part of a component's mechanical closure.
    // The verified original snapshot digest still covers their authored bytes.
    json["camera_views"] = serde_json::json!({});
    // Fork ancestry names the original source. Only its local targets are namespaced.
    let lineage = json
        .as_object_mut()
        .and_then(|object| object.remove("component_forks"));
    let transfers = json
        .as_object_mut()
        .and_then(|object| object.remove("feature_transfers"));
    remap_source(&mut json, &mapping, "")?;
    if let Some(serde_json::Value::Object(records)) = lineage {
        let mut remapped = serde_json::Map::new();
        for (definition, mut record) in records {
            let target = mapping.get(&definition).unwrap_or(&definition).clone();
            record["definition"] = serde_json::Value::String(target.clone());
            if let Some(correspondence) = record
                .get_mut("correspondence")
                .and_then(serde_json::Value::as_object_mut)
            {
                for value in correspondence.values_mut() {
                    if let Some(mapped) = value.as_str().and_then(|id| mapping.get(id)) {
                        *value = serde_json::Value::String(mapped.clone());
                    }
                }
            }
            remapped.insert(target, record);
        }
        json["component_forks"] = serde_json::Value::Object(remapped);
    }
    if let Some(serde_json::Value::Object(records)) = transfers {
        let mut remapped = serde_json::Map::new();
        for (receipt, mut record) in records {
            if let Some(target) = record["target_definition"]
                .as_str()
                .and_then(|id| mapping.get(id))
            {
                record["target_definition"] = serde_json::Value::String(target.clone());
            }
            if let Some(correspondence) = record
                .get_mut("correspondence")
                .and_then(serde_json::Value::as_object_mut)
            {
                for value in correspondence.values_mut() {
                    if let Some(mapped) = value.as_str().and_then(|id| mapping.get(id)) {
                        *value = serde_json::Value::String(mapped.clone());
                    }
                }
            }
            remapped.insert(mapped(link_id, &receipt), record);
        }
        json["feature_transfers"] = serde_json::Value::Object(remapped);
    }
    let imported: Document = serde_json::from_value(json)?;
    Ok((imported, mapping))
}

/// Retain the selected component and all referenced component definitions. This
/// deliberately retains shared material/resource catalogs and the full source
/// snapshot, so imported components remain reproducible offline.
pub(super) fn retain_definition_closure(document: &mut Document, definition: &str) -> Result<()> {
    let ids = document
        .parts
        .keys()
        .chain(document.assemblies.keys())
        .cloned()
        .collect::<BTreeSet<_>>();
    if !ids.contains(definition) {
        return Err(CadError::Invalid("Unknown component definition".into()));
    }
    let mut pending = vec![definition.to_string()];
    let mut retained = BTreeSet::new();
    while let Some(current) = pending.pop() {
        if !retained.insert(current.clone()) {
            continue;
        }
        let value = if let Some(part) = document.parts.get(&current) {
            serde_json::to_value(part)?
        } else {
            serde_json::to_value(&document.assemblies[&current])?
        };
        // Source references also occur in expressions and encoded provenance.
        // Conservatively retaining a literal match is safe; missing a dependency is not.
        let text = serde_json::to_string(&value)?;
        pending.extend(
            ids.iter()
                .filter(|id| !retained.contains(*id) && text.contains(id.as_str()))
                .cloned(),
        );
    }
    document.parts.retain(|id, _| retained.contains(id));
    document.assemblies.retain(|id, _| retained.contains(id));
    document
        .active_configurations
        .retain(|id, _| retained.contains(id));
    let mut json = serde_json::to_value(&*document)?;
    if let Some(records) = json
        .get_mut("component_forks")
        .and_then(serde_json::Value::as_object_mut)
    {
        records.retain(|id, _| retained.contains(id));
    }
    if let Some(records) = json
        .get_mut("feature_transfers")
        .and_then(serde_json::Value::as_object_mut)
    {
        records.retain(|_, record| {
            record["target_definition"]
                .as_str()
                .is_some_and(|id| retained.contains(id))
        });
    }
    *document = serde_json::from_value(json)?;
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
    let label = source
        .strip_prefix(root)
        .unwrap_or(&source)
        .to_string_lossy()
        .to_string();
    let mut candidate = doc.clone();
    let id = link_snapshot(
        &mut candidate,
        root,
        &project_root(&source),
        &label,
        &src,
        definition,
        update,
        None,
    )?;
    *doc = candidate;
    Ok(id)
}
#[allow(clippy::too_many_arguments)]
fn link_snapshot(
    doc: &mut Document,
    root: &Path,
    source_root: &Path,
    source_label: &str,
    src: &Document,
    definition: Option<&str>,
    update: Option<&str>,
    git_source: Option<GitSource>,
) -> Result<Id> {
    if crate::release::is_release_source(root) {
        return Err(CadError::Invalid(
            "This is a released source snapshot. Use Save As before linking or updating components"
                .into(),
        ));
    }
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
    if old
        .as_ref()
        .is_some_and(|old| old.source_definition != source_definition)
    {
        return Err(CadError::Invalid(
            "A component update must retain the source component identity".into(),
        ));
    }
    if !src.parts.contains_key(&source_definition)
        && !src.assemblies.contains_key(&source_definition)
    {
        return Err(CadError::Invalid(
            "Unknown source component definition".into(),
        ));
    }
    let link_id = old.as_ref().map(|l| l.id.clone()).unwrap_or_else(id);
    let (mut imported, mapping) = namespace_snapshot(src, &link_id)?;
    let definition_closure = old.as_ref().is_none_or(|old| old.definition_closure);
    if definition_closure {
        retain_definition_closure(&mut imported, &mapping[&source_definition])?;
    }
    for resource in imported.resources.values_mut() {
        let bytes = std::fs::read(safe_join(source_root, &resource.path)?)?;
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
            doc.component_forks.remove(definition);
        }
        doc.feature_transfers.retain(|_, record| {
            !old.pinned_definitions
                .contains_key(&record.target_definition)
        });
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
    let pinned_materials = imported
        .materials
        .iter()
        .map(|(id, material)| Ok((id.clone(), hash(serde_json::to_vec(material)?))))
        .collect::<Result<BTreeMap<_, _>>>()?;
    let pinned_finishes = imported
        .finishes
        .iter()
        .map(|(id, finish)| Ok((id.clone(), hash(serde_json::to_vec(finish)?))))
        .collect::<Result<BTreeMap<_, _>>>()?;
    doc.materials.extend(imported.materials);
    doc.finishes.extend(imported.finishes);
    doc.resources.extend(imported.resources);
    doc.component_forks.extend(imported.component_forks);
    doc.feature_transfers.extend(imported.feature_transfers);
    let bytes = serde_json::to_vec_pretty(src)?;
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
    let selected_configuration = git_source
        .as_ref()
        .and_then(|git| git.configuration.as_ref())
        .map(|configuration| {
            if !crate::configuration::exists(src, &source_definition, configuration) {
                return Err(CadError::Invalid(
                    "The selected source configuration does not exist".into(),
                ));
            }
            mapping.get(configuration).cloned().ok_or_else(|| {
                CadError::Invalid("The source configuration has no imported identity".into())
            })
        })
        .transpose()?;
    let name = crate::presentation::definition_name(src, &source_definition);
    doc.libraries.insert(
        link_id.clone(),
        ComponentLink {
            id: link_id.clone(),
            name: name.clone(),
            source_path: source_label.into(),
            source_document: src.id.clone(),
            source_definition,
            source_revision: src.revision,
            source_sha256: src.fingerprint(),
            snapshot_resource: snapshot,
            definition: definition.clone(),
            pinned_definitions: pins,
            pinned_materials: Some(pinned_materials),
            pinned_finishes: Some(pinned_finishes),
            git_source,
            definition_closure,
            pin_diagnostic: None,
        },
    );
    if old.is_none() {
        let instance = id();
        let assembly = doc
            .assemblies
            .get_mut(&doc.root)
            .ok_or_else(|| CadError::Invalid("Linking needs an assembly root".into()))?;
        let fixed = assembly.instances.is_empty();
        assembly.instances.insert(
            instance.clone(),
            Instance {
                id: instance,
                name,
                definition,
                placement: Default::default(),
                finish: None,
                configuration: selected_configuration,
                fixed,
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
                | "metadata"
                | "standard_part"
                | "library_profile"
                | "manufacturing"
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
                | "sketch_dimensions"
                | "__sketch_dimension_constraints"
                | "components"
                | "mates"
                | "component_configurations"
                | "component_placements"
                | "mate_curve_seeds"
                | "component_patterns"
                | "exploded_views"
                | "display_states"
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
            "id"
            | "definition"
            | "root"
            | "configuration"
            | "parent"
            | "node"
            | "material"
            | "default_new_part_material"
            | "finish"
            | "resource"
            | "instance"
            | "relative_to"
            | "sketch"
            | "block"
            | "outputs"
            | "profile_order"
            | "path"
            | "instance_path"
            | "occurrence_path"
            | "parent_path"
            | "seed"
            | "seeds"
            | "mate"
            | "source"
            | "sources"
            | "segments"
            | "entities"
            | "point"
            | "member"
            | "members"
            | "points"
            | "through"
            | "controls"
            | "lines"
            | "trim"
            | "arc"
            | "arcs"
            | "anchor"
            | "support" => true,
            "base_color" | "normal" | "roughness" | "metallic" | "occlusion" | "height" => {
                field == "textures" && object.get(key).is_some_and(serde_json::Value::is_string)
            }
            "environment" => field == "lighting",
            "datum" | "constraint" | "source_record" => true,
            "placement" => {
                object.get("operation").and_then(serde_json::Value::as_str)
                    == Some("placement_pattern")
            }
            "dynamic_excluded" | "target_vertex" | "path_member" | "path_start_vertex"
            | "gusset_contour" | "vertex" | "start_vertex" | "source_members" | "outer_members"
            | "region_member" => true,
            "first" | "second" => {
                object.contains_key("first_origin") && object.contains_key("second_origin")
                    || field == "relation"
                        && matches!(
                            object.get("kind").and_then(|value| value.as_str()),
                            Some(
                                "coincident"
                                    | "distance"
                                    | "axis_distance"
                                    | "angle"
                                    | "parallel"
                                    | "perpendicular"
                                    | "equal_length"
                                    | "tangent"
                            )
                        )
            }
            "line" => {
                field == "relation"
                    && matches!(
                        object.get("kind").and_then(|value| value.as_str()),
                        Some("on_line" | "point_line_distance")
                    )
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
                    } else if field == "sketch_dimensions" {
                        "__sketch_dimension_constraints"
                    } else if field == "__sketch_dimension_constraints" {
                        "__sketch_dimension_expression"
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
                    let key = if field == "item_numbers" {
                        remap_manufacturing_row_key(&key, mapping, owned_features)?
                    } else if field == "__feature_dimension_fields" {
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
                    && let Some(intent) = object.get_mut("intent")
                {
                    // Source sets can also be explicit indexed constructor
                    // tables. Reordering must rewrite the constructor indices.
                    crate::provenance::canonicalize(intent)?;
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
                if matches!(field, "row_order" | "row_key") {
                    *text = remap_manufacturing_row_key(text, mapping, owned_features)?;
                    return Ok(());
                }
                if is_reference {
                    if let Some(id) = mapping.get(text) {
                        *text = id.clone();
                    } else if !owned_features {
                        fn member(text: &str, mapping: &BTreeMap<String, String>) -> String {
                            if let Some(id) = mapping.get(text) {
                                return id.clone();
                            }
                            for prefix in ["gusset-contour-", "legacy-circle-"] {
                                if let Some(id) = text.strip_prefix(prefix) {
                                    return format!("{prefix}{}", member(id, mapping));
                                }
                            }
                            text.into()
                        }
                        *text = member(text, mapping);
                    }
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

/// Saved manufacturing row manifests carry identities explicitly; the
/// production geometry digest is invariant under ownership changes.
pub(crate) fn remap_manufacturing_row_key(
    key: &str,
    mapping: &BTreeMap<String, String>,
    owned_features: bool,
) -> Result<String> {
    if key.starts_with('{') {
        let mut value: serde_json::Value = serde_json::from_str(key)?;
        remap_source_inner(&mut value, mapping, "", owned_features)?;
        return Ok(serde_json::to_string(&value)?);
    }
    // Legacy opaque stock hashes lack a remappable identity manifest. Retain
    // the reservation; current source validation requires explicit reattachment
    // when the imported/unique production group cannot prove the same binding.
    if key.starts_with("stock:") || key.starts_with("bom:") {
        return Ok(key.into());
    }
    let candidates = mapping
        .iter()
        .filter_map(|(old, new)| {
            key.strip_prefix(&format!("{old}@"))
                .map(|configuration| (new, configuration))
        })
        .collect::<Vec<_>>();
    if candidates.len() > 1 {
        return Err(CadError::Unsupported(
            "Ambiguous legacy BOM row identity cannot be safely remapped; reattach this table row"
                .into(),
        ));
    }
    if let Some((definition, configuration)) = candidates.first() {
        let configuration = if owned_features {
            *configuration
        } else {
            mapping
                .get(*configuration)
                .map(String::as_str)
                .unwrap_or(configuration)
        };
        return Ok(crate::bom::item_key(
            definition,
            (!configuration.is_empty()).then_some(configuration),
        ));
    }
    Ok(key.into())
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
    if prefix == "member" {
        // Sheet-stock source descriptors use the authored sketch member as
        // their leaf role. Whole-document import namespaces that member;
        // Make Unique retains member IDs while replacing feature owners.
        return Ok(format!(
            "member:{}",
            (!owned_features)
                .then(|| mapping.get(payload))
                .flatten()
                .map(String::as_str)
                .unwrap_or(payload)
        ));
    }
    if !matches!(
        prefix,
        "pattern" | "hole-position" | "hole-intersection" | "construction"
    ) {
        return Ok(role.into());
    }
    if payload.trim_start().starts_with('{') {
        let value: serde_json::Value = serde_json::from_str(payload)
            .map_err(|_| CadError::Invalid("Malformed saved construction-role payload".into()))?;
        // Expand legacy embedded descriptor strings only into bounded table
        // entries, so ownership changes visit every actual source identity.
        let mut value = crate::provenance::construction_payload(value)?;
        remap_source_inner(&mut value, mapping, "", owned_features)?;
        let root = value.get_mut("root").ok_or_else(|| {
            CadError::Invalid("Saved source closure has no construction intent".into())
        })?;
        if prefix == "hole-intersection" {
            let carriers = root
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
            let sources = root
                .get_mut("sources")
                .and_then(serde_json::Value::as_array_mut)
                .ok_or_else(|| {
                    CadError::Invalid("Constructed topology role has no source identities".into())
                })?;
            // The shared encoder canonicalizes sources and their explicit
            // constructor indices together after all identity remapping.
            let _ = sources;
        }
        let value = crate::provenance::construction_payload(value)?;
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

/// A portable forge reference. The selected revision resolves to an immutable commit before import.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GitLinkRequest {
    pub remote: String,
    #[serde(default)]
    pub project_path: String,
    pub revision: String,
    pub update_ref: String,
    pub definition: Option<Id>,
    pub configuration: Option<Id>,
}
fn git_repository(root: &Path, remote: &str, fetch: bool) -> Result<PathBuf> {
    use crate::version_control::{Git, VersionControl};
    crate::version_control::validate_remote(remote)?;
    let _ = crate::version_control::exclude_project_caches(root);
    let cache = root.join(".tcad-cache/git");
    let repository = cache.join(hash(remote));
    std::fs::create_dir_all(&cache)?;
    if !repository.exists() {
        if !fetch {
            return Err(CadError::Invalid("This Git source is not cached. Fetch the source to check updates; the saved component remains available offline".into()));
        }
        Git.command(
            &cache,
            &[
                "clone",
                "--bare",
                "--no-hardlinks",
                "--",
                remote,
                repository
                    .to_str()
                    .ok_or_else(|| CadError::Invalid("Invalid cache path".into()))?,
            ],
        )?;
    } else {
        let origin = Git.command(&repository, &["remote", "get-url", "origin"])?;
        if origin.trim() != remote {
            return Err(CadError::Invalid(
                "The cached Git repository does not match its recorded source".into(),
            ));
        }
        if fetch {
            Git.command(
                &repository,
                &[
                    "fetch",
                    "--prune",
                    "origin",
                    "+refs/heads/*:refs/heads/*",
                    "+refs/tags/*:refs/tags/*",
                ],
            )?;
        }
    }
    Ok(repository)
}
/// Import a part or assembly at an exact Git commit. The host document changes only on success.
pub fn link_git(
    doc: &mut Document,
    root: &Path,
    request: &GitLinkRequest,
    update: Option<&str>,
) -> Result<Id> {
    let repository = git_repository(root, &request.remote, true)?;
    let commit = crate::version_control::resolve_commit(&repository, &request.revision)?;
    let destination = root.join(".tcad-cache/git-snapshots").join(id());
    let result = (|| {
        let source = crate::version_control::snapshot_resources_at(
            &repository,
            &commit,
            &request.project_path,
            &destination,
        )?;
        let git = GitSource {
            remote: request.remote.clone(),
            project_path: request.project_path.clone(),
            commit,
            update_ref: request.update_ref.clone(),
            configuration: request.configuration.clone(),
        };
        git.validate()?;
        let mut candidate = doc.clone();
        let linked = link_snapshot(
            &mut candidate,
            root,
            &destination,
            &request.remote,
            &source,
            request.definition.as_deref(),
            update,
            Some(git),
        )?;
        *doc = candidate;
        Ok(linked)
    })();
    let _ = std::fs::remove_dir_all(destination);
    result
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AffectedOccurrence {
    pub assembly: Id,
    pub instance: Id,
    pub name: String,
    pub definition: Id,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ComponentUpdateImpact {
    pub source_before: String,
    pub source_after: String,
    pub changed_definitions: Vec<Id>,
    pub affected_assemblies: Vec<Id>,
    pub occurrences: Vec<AffectedOccurrence>,
    pub mates: Vec<Id>,
    pub drawings: Vec<Id>,
    pub bom_rows: Vec<String>,
    pub changes: Vec<crate::version_control::SemanticChange>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ComponentUpdatePlan {
    pub base_fingerprint: String,
    pub link: Id,
    pub document: Document,
    pub impact: ComponentUpdateImpact,
}
pub fn update_impact(
    before: &Document,
    after: &Document,
    link: &str,
) -> Result<ComponentUpdateImpact> {
    let old = before
        .libraries
        .get(link)
        .ok_or_else(|| CadError::Invalid("Unknown component link".into()))?;
    let new = after
        .libraries
        .get(link)
        .ok_or_else(|| CadError::Invalid("Updated component link is missing".into()))?;
    let keys = old
        .pinned_definitions
        .keys()
        .chain(new.pinned_definitions.keys())
        .cloned()
        .collect::<BTreeSet<_>>();
    let changed_definitions = keys
        .into_iter()
        .filter(|id| old.pinned_definitions.get(id) != new.pinned_definitions.get(id))
        .collect::<Vec<_>>();
    let mut affected = changed_definitions.iter().cloned().collect::<BTreeSet<_>>();
    // Material and finish changes affect their consuming components even without geometry edits.
    if old.pinned_materials != new.pinned_materials || old.pinned_finishes != new.pinned_finishes {
        affected.extend(new.pinned_definitions.keys().cloned());
    }
    loop {
        let previous = affected.len();
        for assembly in before.assemblies.values().chain(after.assemblies.values()) {
            if assembly
                .instances
                .values()
                .any(|instance| affected.contains(&instance.definition))
            {
                affected.insert(assembly.id.clone());
            }
        }
        if previous == affected.len() {
            break;
        }
    }
    let mut occurrences = BTreeMap::new();
    let mut mates = BTreeSet::new();
    for assembly in before.assemblies.values().chain(after.assemblies.values()) {
        let instance_ids = assembly
            .instances
            .values()
            .filter(|instance| affected.contains(&instance.definition))
            .map(|instance| {
                occurrences.insert(
                    (assembly.id.clone(), instance.id.clone()),
                    AffectedOccurrence {
                        assembly: assembly.id.clone(),
                        instance: instance.id.clone(),
                        name: instance.name.clone(),
                        definition: instance.definition.clone(),
                    },
                );
                instance.id.clone()
            })
            .collect::<BTreeSet<_>>();
        mates.extend(
            assembly
                .mates
                .iter()
                .filter(|mate| {
                    instance_ids.contains(&mate.first) || instance_ids.contains(&mate.second)
                })
                .map(|mate| mate.id.clone()),
        );
    }
    let drawings = before
        .drawings
        .iter()
        .chain(&after.drawings)
        .filter(|drawing| affected.contains(&drawing.definition))
        .map(|drawing| drawing.id.clone())
        .collect::<BTreeSet<_>>();
    let old_rows = crate::bom::rows(before, &Default::default())?
        .into_iter()
        .map(|row| (row.key.clone(), row))
        .collect::<BTreeMap<_, _>>();
    let new_rows = crate::bom::rows(after, &Default::default())?
        .into_iter()
        .map(|row| (row.key.clone(), row))
        .collect::<BTreeMap<_, _>>();
    let bom_rows = old_rows
        .keys()
        .chain(new_rows.keys())
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .filter(|key| old_rows.get(key) != new_rows.get(key))
        .collect();
    let label = |link: &ComponentLink| {
        link.git_source
            .as_ref()
            .map(|git| git.commit.clone())
            .unwrap_or_else(|| {
                format!(
                    "revision {} · {}",
                    link.source_revision,
                    &link.source_sha256[..12]
                )
            })
    };
    Ok(ComponentUpdateImpact {
        source_before: label(old),
        source_after: label(new),
        changed_definitions,
        affected_assemblies: affected
            .iter()
            .filter(|id| before.assemblies.contains_key(*id) || after.assemblies.contains_key(*id))
            .cloned()
            .collect(),
        occurrences: occurrences.into_values().collect(),
        mates: mates.into_iter().collect(),
        drawings: drawings.into_iter().collect(),
        bom_rows,
        changes: crate::version_control::diff(before, after)?,
    })
}
/// Prepare an update for the common validated draft/Apply flow. Never applies it to committed state.
pub fn prepare_update(
    doc: &Document,
    root: &Path,
    link: &str,
    revision: Option<&str>,
) -> Result<ComponentUpdatePlan> {
    let source = doc
        .libraries
        .get(link)
        .ok_or_else(|| CadError::Invalid("Unknown component link".into()))?;
    let mut document = doc.clone();
    if let Some(git) = &source.git_source {
        link_git(
            &mut document,
            root,
            &GitLinkRequest {
                remote: git.remote.clone(),
                project_path: git.project_path.clone(),
                revision: revision.unwrap_or(&git.update_ref).into(),
                update_ref: git.update_ref.clone(),
                definition: Some(source.source_definition.clone()),
                configuration: git.configuration.clone(),
            },
            Some(link),
        )?;
    } else {
        self::link(
            &mut document,
            root,
            Path::new(&source.source_path),
            None,
            Some(link),
        )?;
    }
    let impact = update_impact(doc, &document, link)?;
    Ok(ComponentUpdatePlan {
        base_fingerprint: doc.fingerprint(),
        link: link.into(),
        document,
        impact,
    })
}
/// Fetching changes the local repository cache only. No assembly reference is moved.
pub fn check_update(
    link: &ComponentLink,
    root: &Path,
    fetch: bool,
) -> Result<Option<(u64, String)>> {
    if let Some(git) = &link.git_source {
        git_repository(root, &git.remote, fetch)?;
    }
    available_update(link, root)
}

/// Fetch and inspect source definitions without importing or updating any host component.
pub fn inspect_git_source(root: &Path, request: &GitLinkRequest) -> Result<(Document, String)> {
    let repository = git_repository(root, &request.remote, true)?;
    let commit = crate::version_control::resolve_commit(&repository, &request.revision)?;
    let document =
        crate::version_control::snapshot_at(&repository, &commit, &request.project_path)?;
    Ok((document, commit))
}
