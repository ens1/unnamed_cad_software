//! Immutable material/finish snapshots and verified, explicit component updates.
use super::{ComponentLink, hash, namespace_snapshot};
use crate::{CadError, Document, Id, Result, commands::Command, presentation::SemanticElement};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    io::Read,
    path::Path,
};

pub const MAX_SNAPSHOT_BYTES: usize = 64 * 1024 * 1024;
const MAX_PROOF_BYTES: usize = 128 * 1024 * 1024;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SnapshotProof {
    pub link: Id,
    /// Exact immutable Resource bytes, as UTF-8 JSON. Never an executable path.
    pub source_json: String,
}
struct Expected {
    definitions: BTreeMap<Id, String>,
    materials: BTreeMap<Id, String>,
    finishes: BTreeMap<Id, String>,
    resources: BTreeMap<Id, crate::Resource>,
}
fn invalid(message: impl ToString) -> CadError {
    CadError::Invalid(format!("Component snapshot: {}", message.to_string()))
}
fn digest_map<'a, T: Serialize + 'a>(
    values: impl Iterator<Item = (&'a Id, &'a T)>,
) -> Result<BTreeMap<Id, String>> {
    values
        .map(|(id, value)| Ok((id.clone(), hash(serde_json::to_vec(value)?))))
        .collect()
}
fn expected(document: &Document, link: &ComponentLink, json: &str) -> Result<Expected> {
    if json.len() > MAX_SNAPSHOT_BYTES {
        return Err(invalid(
            "Source snapshot exceeds the 64 MiB supported limit",
        ));
    }
    let resource = document
        .resources
        .get(&link.snapshot_resource)
        .ok_or_else(|| invalid("The pinned source resource is missing"))?;
    if resource.kind != "component_snapshot" || hash(json.as_bytes()) != resource.sha256 {
        return Err(invalid(
            "Source snapshot bytes do not match their pinned resource hash",
        ));
    }
    let source: Document = serde_json::from_str(json)?;
    source.validate()?;
    if source.id != link.source_document
        || source.revision != link.source_revision
        || source.fingerprint() != link.source_sha256
    {
        return Err(invalid(
            "Source snapshot identity, revision or canonical fingerprint does not match the saved component link; no migration was guessed",
        ));
    }
    let (mut namespaced, mapping) = namespace_snapshot(&source, &link.id)?;
    if mapping.get(&link.source_definition) != Some(&link.definition) {
        return Err(invalid(
            "The linked source definition does not match its namespaced snapshot identity",
        ));
    }
    let mut definitions = digest_map(namespaced.parts.iter())?;
    definitions.extend(digest_map(namespaced.assemblies.iter())?);
    for resource in namespaced.resources.values_mut() {
        let extension = Path::new(&resource.path)
            .extension()
            .and_then(|extension| extension.to_str())
            .unwrap_or("bin");
        resource.path = format!("imports/{}.{}", resource.sha256, extension);
    }
    Ok(Expected {
        definitions,
        materials: digest_map(namespaced.materials.iter())?,
        finishes: digest_map(namespaced.finishes.iter())?,
        resources: namespaced.resources,
    })
}
fn matches_expected(
    document: &Document,
    link: &ComponentLink,
    expected: &Expected,
    require_maps: bool,
) -> Result<()> {
    if link.pinned_definitions != expected.definitions {
        return Err(invalid(
            "Pinned component definitions do not match the verified source snapshot",
        ));
    }
    if link
        .pinned_materials
        .as_ref()
        .is_some_and(|pins| pins != &expected.materials)
        || link
            .pinned_finishes
            .as_ref()
            .is_some_and(|pins| pins != &expected.finishes)
        || require_maps && (link.pinned_materials.is_none() || link.pinned_finishes.is_none())
    {
        return Err(invalid(
            "Material/finish pin records do not match the verified source snapshot",
        ));
    }
    for (id, digest) in &expected.materials {
        let material = document
            .materials
            .get(id)
            .ok_or_else(|| invalid(format!("The pinned engineering material {id} is missing")))?;
        if hash(serde_json::to_vec(material)?) != *digest {
            return Err(invalid(
                "A linked engineering material differs from the original source snapshot; make the component unique and duplicate its material",
            ));
        }
    }
    for (id, digest) in &expected.finishes {
        let finish = document
            .finishes
            .get(id)
            .ok_or_else(|| invalid(format!("The pinned finish {id} is missing")))?;
        if hash(serde_json::to_vec(finish)?) != *digest {
            return Err(invalid(
                "A linked finish differs from the original source snapshot; duplicate it before editing a unique component",
            ));
        }
    }
    for (id, resource) in &expected.resources {
        if document.resources.get(id) != Some(resource) {
            return Err(invalid(
                "A linked texture or other resource differs from the verified source snapshot; reimport or explicitly update the component",
            ));
        }
    }
    Ok(())
}
fn read_snapshot(document: &Document, link: &ComponentLink, root: &Path) -> Result<String> {
    let resource = document
        .resources
        .get(&link.snapshot_resource)
        .ok_or_else(|| invalid("The exact saved source snapshot resource is missing"))?;
    let path = crate::persistence::safe_join(root, &resource.path)?;
    let file = File::open(path)?;
    if file.metadata()?.len() > MAX_SNAPSHOT_BYTES as u64 {
        return Err(invalid(
            "Source snapshot exceeds the 64 MiB supported limit",
        ));
    }
    let mut bytes = Vec::new();
    file.take(MAX_SNAPSHOT_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > MAX_SNAPSHOT_BYTES {
        return Err(invalid(
            "Source snapshot exceeds the 64 MiB supported limit",
        ));
    }
    String::from_utf8(bytes).map_err(|_| invalid("The saved source snapshot is not UTF-8 JSON"))
}
/// Populate legacy optional evidence only when the exact original snapshot and
/// all current imported definitions agree. Unavailable evidence stays read-only.
pub fn hydrate_pins(document: &mut Document, root: &Path) -> Vec<Id> {
    let mut enriched = Vec::new();
    for link in document.libraries.values().cloned().collect::<Vec<_>>() {
        let result = read_snapshot(document, &link, root)
            .and_then(|json| expected(document, &link, &json))
            .and_then(|value| {
                matches_expected(document, &link, &value, false)?;
                Ok(value)
            });
        if let Some(current) = document.libraries.get_mut(&link.id) {
            match result {
                Ok(value) => {
                    if current.pinned_materials.is_none() || current.pinned_finishes.is_none() {
                        enriched.push(link.id.clone());
                    }
                    current.pinned_materials = Some(value.materials);
                    current.pinned_finishes = Some(value.finishes);
                    current.pin_diagnostic = None;
                }
                Err(error) => {
                    current.pin_diagnostic = Some(format!(
                        "Original snapshot could not be verified: {error}. Linked assignments remain read-only; locate the original resource or explicitly update the source."
                    ))
                }
            }
        }
    }
    enriched
}
fn valid_hash(hash: &str) -> bool {
    hash.len() == 64 && hash.bytes().all(|c| c.is_ascii_hexdigit())
}
pub(super) fn validate_link_record(document: &Document, link: &ComponentLink) -> Result<()> {
    fn valid_id(id: &str) -> bool {
        !id.is_empty()
            && id.len() <= 256
            && !matches!(id, "." | "..")
            && !id
                .chars()
                .any(|c| c.is_control() || matches!(c, '/' | '\\' | ':'))
    }
    if [
        &link.id,
        &link.source_document,
        &link.source_definition,
        &link.snapshot_resource,
        &link.definition,
    ]
    .into_iter()
    .any(|id| !valid_id(id))
        || link.name.trim().is_empty()
        || link.name.len() > 4096
        || link.name.chars().any(char::is_control)
        || link.source_path.is_empty()
        || link.source_path.len() > 16_384
        || link.source_path.chars().any(char::is_control)
        || !valid_hash(&link.source_sha256)
        || !link.pinned_definitions.contains_key(&link.definition)
        || link.pinned_definitions.len() > 65_536
    {
        return Err(invalid(
            "A component link contains invalid identities, source text or pin records",
        ));
    }
    let snapshot = document
        .resources
        .get(&link.snapshot_resource)
        .ok_or_else(|| invalid("The saved source snapshot resource is missing"))?;
    if snapshot.kind != "component_snapshot"
        || snapshot.id != link.snapshot_resource
        || !valid_hash(&snapshot.sha256)
    {
        return Err(invalid(
            "The source resource must be an immutable component snapshot with a SHA-256 hash",
        ));
    }
    crate::persistence::safe_join(Path::new("."), &snapshot.path)?;
    for pins in std::iter::once(&link.pinned_definitions)
        .chain(link.pinned_materials.iter())
        .chain(link.pinned_finishes.iter())
    {
        if pins.len() > 65_536
            || pins
                .iter()
                .any(|(id, digest)| !valid_id(id) || !valid_hash(digest))
        {
            return Err(invalid(
                "Snapshot pin maps must contain bounded portable identities and SHA-256 hashes",
            ));
        }
    }
    Ok(())
}
pub(super) fn validate_material_finish_pins(
    document: &Document,
    link: &ComponentLink,
) -> Result<()> {
    if let Some(pins) = &link.pinned_materials {
        for (id, digest) in pins {
            let material = document
                .materials
                .get(id)
                .ok_or_else(|| invalid(format!("Missing pinned material {id}")))?;
            if !valid_hash(digest) || hash(serde_json::to_vec(material)?) != *digest {
                return Err(invalid(format!(
                    "{} contains a modified pinned engineering material; make the component unique and duplicate its material",
                    link.name
                )));
            }
        }
    }
    if let Some(pins) = &link.pinned_finishes {
        for (id, digest) in pins {
            let finish = document
                .finishes
                .get(id)
                .ok_or_else(|| invalid(format!("Missing pinned finish {id}")))?;
            if !valid_hash(digest) || hash(serde_json::to_vec(finish)?) != *digest {
                return Err(invalid(format!(
                    "{} contains a modified pinned finish; duplicate it before editing a unique component",
                    link.name
                )));
            }
        }
    }
    Ok(())
}
fn referenced_materials(document: &Document, link: &ComponentLink) -> BTreeSet<Id> {
    let mut ids = BTreeSet::new();
    if let Some(pins) = &link.pinned_materials {
        ids.extend(pins.keys().cloned());
    }
    for id in link.pinned_definitions.keys() {
        if let Some(part) = document.parts.get(id) {
            ids.insert(part.material.clone());
            ids.extend(
                part.configuration_details
                    .values()
                    .filter_map(|configuration| configuration.overrides.material.clone()),
            );
        }
    }
    ids
}
fn referenced_finishes(document: &Document, link: &ComponentLink) -> BTreeSet<Id> {
    let mut ids = BTreeSet::new();
    if let Some(pins) = &link.pinned_finishes {
        ids.extend(pins.keys().cloned());
    }
    for id in link.pinned_definitions.keys() {
        if let Some(part) = document.parts.get(id) {
            ids.insert(part.finish.clone());
            ids.extend(
                part.face_finishes
                    .iter()
                    .map(|assignment| assignment.finish.clone()),
            );
            ids.extend(
                part.configuration_details
                    .values()
                    .filter_map(|configuration| configuration.overrides.finish.clone()),
            );
        }
        if let Some(assembly) = document.assemblies.get(id) {
            ids.extend(
                assembly
                    .instances
                    .values()
                    .filter_map(|instance| instance.finish.clone()),
            );
            for state in assembly.display_states.values() {
                ids.extend(
                    state
                        .overrides
                        .iter()
                        .filter_map(|target| target.finish.clone()),
                );
            }
        }
    }
    ids
}
pub fn material_is_pinned(document: &Document, material: &str) -> bool {
    document
        .libraries
        .values()
        .any(|link| referenced_materials(document, link).contains(material))
}
pub fn finish_is_pinned(document: &Document, finish: &str) -> bool {
    document
        .libraries
        .values()
        .any(|link| referenced_finishes(document, link).contains(finish))
}
fn unchanged_links(before: &Document, after: &Document, allowed: &BTreeSet<Id>) -> Result<()> {
    for (id, link) in &before.libraries {
        let next = after.libraries.get(id).ok_or_else(|| invalid("Advanced Source cannot remove a component's immutable link evidence; make an instance unique instead"))?;
        if allowed.contains(id) {
            continue;
        }
        if serde_json::to_value(link)? != serde_json::to_value(next)? {
            return Err(invalid(
                "Linked-source pin evidence can change only through an explicit verified component update",
            ));
        }
        if before.resources.get(&link.snapshot_resource)
            != after.resources.get(&link.snapshot_resource)
        {
            return Err(invalid(
                "The immutable component snapshot resource can change only through an explicit verified update",
            ));
        }
        for material in referenced_materials(before, link) {
            if before.materials.get(&material) != after.materials.get(&material) {
                return Err(invalid(
                    "Advanced Source cannot alter a pinned material; duplicate it and assign the copy to a unique component",
                ));
            }
        }
        for finish in referenced_finishes(before, link) {
            if before.finishes.get(&finish) != after.finishes.get(&finish) {
                return Err(invalid(
                    "Advanced Source cannot alter a pinned finish; duplicate it and assign the copy to a unique component",
                ));
            }
            if let Some(finish) = before.finishes.get(&finish) {
                for resource in finish.textures.resources() {
                    if before.resources.get(resource) != after.resources.get(resource) {
                        return Err(invalid(
                            "Advanced Source cannot alter a pinned finish's texture resource; duplicate its finish and import a separate image",
                        ));
                    }
                }
            }
        }
    }
    for id in after.libraries.keys() {
        if !before.libraries.contains_key(id) && !allowed.contains(id) {
            return Err(invalid(
                "New component links require verified original snapshot evidence",
            ));
        }
    }
    Ok(())
}
pub fn validate_replacement(before: &Document, after: &Document) -> Result<()> {
    unchanged_links(before, after, &BTreeSet::new())
}
pub fn validate_verified_replacement(
    before: &Document,
    after: &Document,
    proofs: &[SnapshotProof],
) -> Result<()> {
    let mut allowed = BTreeSet::new();
    let mut bytes = 0usize;
    for proof in proofs {
        bytes = bytes
            .checked_add(proof.source_json.len())
            .ok_or_else(|| invalid("Snapshot proof size overflow"))?;
        if bytes > MAX_PROOF_BYTES || !allowed.insert(proof.link.clone()) {
            return Err(invalid(
                "Snapshot evidence exceeds the supported total size or repeats a link",
            ));
        }
        let link = after
            .libraries
            .get(&proof.link)
            .ok_or_else(|| invalid("Snapshot proof targets a missing component link"))?;
        if before
            .libraries
            .get(&proof.link)
            .is_some_and(|previous| previous.source_document != link.source_document)
        {
            return Err(invalid(
                "A component update must retain its original source document identity",
            ));
        }
        let value = expected(after, link, &proof.source_json)?;
        matches_expected(after, link, &value, true)?;
    }
    unchanged_links(before, after, &allowed)?;
    after.validate()
}
/// The desktop file-picker workflow supplies verified bytes; ordinary source
/// edits retain `ReplaceDocument` and cannot rewrite linked-component evidence.
pub fn replace_command(before: &Document, after: Document, root: &Path) -> Result<Command> {
    let mut proofs = vec![];
    for (id, link) in &after.libraries {
        let changed = before
            .libraries
            .get(id)
            .map(serde_json::to_value)
            .transpose()?
            != Some(serde_json::to_value(link)?);
        if changed {
            proofs.push(SnapshotProof {
                link: id.clone(),
                source_json: read_snapshot(&after, link, root)?,
            });
        }
    }
    if proofs.is_empty() {
        validate_replacement(before, &after)?;
        Ok(Command::ReplaceDocument { document: after })
    } else {
        validate_verified_replacement(before, &after, &proofs)?;
        Ok(Command::ReplaceLinkedComponentSnapshots {
            document: after,
            snapshots: proofs,
        })
    }
}
pub fn semantic(document: &Document) -> Vec<SemanticElement> {
    document.libraries.values().map(|link| {
        let verified = link.pinned_materials.is_some() && link.pinned_finishes.is_some() && link.pin_diagnostic.is_none();
        SemanticElement { id: format!("library-integrity:{}", link.id), role: "library-snapshot-integrity".into(), label: link.name.clone(), value: Some(link.pin_diagnostic.clone().unwrap_or_else(|| if verified { "Source definitions, engineering materials and finishes are pinned".into() } else { "Legacy source assignments are read-only until the original snapshot is verified".into() })), state: if verified { "verified" } else { "warning" }.into(), commands: vec!["library".into()], relationships: vec![link.id.clone(), link.definition.clone(), link.snapshot_resource.clone()], data: serde_json::json!({"contract":"tcad.component-snapshot-integrity","version":1,"link":link.id,"source_document":link.source_document,"source_revision":link.source_revision,"source_sha256":link.source_sha256,"pinned_material_count":link.pinned_materials.as_ref().map(BTreeMap::len),"pinned_finish_count":link.pinned_finishes.as_ref().map(BTreeMap::len),"legacy_unverified":!verified,"diagnostic":link.pin_diagnostic}) }
    }).collect()
}
