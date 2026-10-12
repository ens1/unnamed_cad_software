//! Bounded personal indexes of immutable native sketch-block source copies.
use super::resources::{self, Package, Source};
use crate::{CadError, Id, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{File, OpenOptions},
    io::Read,
    path::{Path, PathBuf},
};
pub const CONTRACT_VERSION: u32 = 1;
pub const MAX_ENTRIES: usize = 512;
pub const MAX_INDEX_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_LIBRARY_BYTES: u64 = 128 * 1024 * 1024;
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Entry {
    pub id: Id,
    pub name: String,
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub favorite: bool,
    #[serde(default)]
    pub archived: bool,
    pub file: String,
    pub sha256: String,
    pub package_sha256: String,
    pub bytes: u64,
    pub package_id: Id,
    pub source: Source,
    pub points: usize,
    pub members: usize,
    pub constraints: usize,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Catalog {
    pub version: u32,
    pub revision: u64,
    pub entries: BTreeMap<Id, Entry>,
}
impl Default for Catalog {
    fn default() -> Self {
        Self {
            version: CONTRACT_VERSION,
            revision: 0,
            entries: BTreeMap::new(),
        }
    }
}
fn invalid(text: impl Into<String>) -> CadError {
    CadError::Invalid(format!("Sketch-block library: {}", text.into()))
}
fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn checksum(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|c| c.is_ascii_hexdigit())
}
fn valid_digest(value: &str) -> bool {
    value.len() == 64
        && value.bytes().all(|c| c.is_ascii_hexdigit())
        && value == value.to_ascii_lowercase()
}
fn bounded_text(value: &str, max: usize, empty: bool) -> bool {
    (empty || !value.trim().is_empty())
        && value.len() <= max
        && !value.chars().any(char::is_control)
}
impl Catalog {
    pub fn validate(&self) -> Result<()> {
        if self.version != CONTRACT_VERSION || self.entries.len() > MAX_ENTRIES {
            return Err(invalid("Expected version 1 and at most 512 entries"));
        }
        let mut hashes = BTreeSet::new();
        let mut bytes = 0_u64;
        for (id, e) in &self.entries {
            if id != &e.id
                || !bounded_text(id, 256, false)
                || id.chars().any(|c| matches!(c, '/' | '\\' | ':'))
                || !bounded_text(&e.name, 128, false)
                || !bounded_text(&e.category, 128, true)
                || e.tags.len() > 32
                || e.tags.iter().any(|tag| !bounded_text(tag, 64, false))
                || !valid_digest(&e.sha256)
                || !valid_digest(&e.package_sha256)
                || e.file != format!("blocks/{}.tcadblock", e.sha256)
                || e.bytes == 0
                || e.bytes > resources::MAX_BYTES as u64
                || !hashes.insert(&e.sha256)
                || e.points > resources::MAX_POINTS
                || e.members > super::MAX_MEMBERS
                || e.constraints > resources::MAX_CONSTRAINTS
                || !bounded_text(&e.package_id, 256, false)
                || !checksum(&e.source.document_sha256)
                || [&e.source.definition, &e.source.sketch, &e.source.block]
                    .into_iter()
                    .any(|s| !bounded_text(s, 256, false))
            {
                return Err(invalid(
                    "Entry identity, metadata, source, counts or pinned resource path is invalid",
                ));
            }
            bytes = bytes
                .checked_add(e.bytes)
                .ok_or_else(|| invalid("Library size overflow"))?;
        }
        if bytes > MAX_LIBRARY_BYTES {
            return Err(invalid(
                "Pinned library copies exceed 128 MiB; export/archive management does not automatically discard source bytes",
            ));
        }
        Ok(())
    }
}
pub fn path(root: &Path) -> PathBuf {
    root.join("index.json")
}
fn read_bounded(path: &Path, limit: usize) -> Result<Vec<u8>> {
    let file = File::open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.len() > limit as u64 {
        return Err(invalid(format!(
            "{} is not a bounded regular file (limit {} bytes)",
            path.display(),
            limit
        )));
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take(limit as u64 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        return Err(invalid("Source file grew beyond its byte bound"));
    }
    Ok(bytes)
}
pub fn load(root: &Path) -> Result<Catalog> {
    let bytes = match read_bounded(&path(root), MAX_INDEX_BYTES) {
        Ok(bytes) => bytes,
        Err(CadError::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Catalog::default());
        }
        Err(error) => return Err(error),
    };
    let catalog: Catalog = serde_json::from_slice(&bytes)?;
    catalog.validate()?;
    Ok(catalog)
}
fn lock(root: &Path) -> Result<File> {
    std::fs::create_dir_all(root)?;
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(root.join(".library-lock"))?;
    file.try_lock()
        .map_err(|e| invalid(format!("Another library action is in progress: {e}")))?;
    Ok(file)
}
fn save(root: &Path, catalog: &mut Catalog) -> Result<()> {
    catalog.revision = catalog
        .revision
        .checked_add(1)
        .ok_or_else(|| invalid("Catalog revision limit reached"))?;
    catalog.validate()?;
    let bytes = serde_json::to_vec_pretty(catalog)?;
    if bytes.len() > MAX_INDEX_BYTES {
        return Err(invalid("Index exceeds 4 MiB"));
    }
    crate::persistence::atomic_write(&path(root), &bytes)
}
fn package(bytes: &[u8]) -> Result<Package> {
    if bytes.is_empty() || bytes.len() > resources::MAX_BYTES {
        return Err(invalid("Block source must be nonempty and at most 16 MiB"));
    }
    let package = serde_json::from_slice(bytes)?;
    resources::validate(&package)?;
    Ok(package)
}
fn short_name(value: &str) -> String {
    let value = value.trim();
    let mut end = value.len().min(128);
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    if end == 0 {
        "Sketch block".into()
    } else {
        value[..end].into()
    }
}
pub fn add_bytes(root: &Path, bytes: &[u8]) -> Result<(Catalog, Id)> {
    let source = package(bytes)?;
    let sha256 = digest(bytes);
    let _lock = lock(root)?;
    let mut catalog = load(root)?;
    if let Some(entry) = catalog.entries.values_mut().find(|e| e.sha256 == sha256) {
        if entry.package_sha256 != resources::sha256(&source)?
            || entry.package_id != source.id
            || entry.source != source.source
            || entry.bytes != bytes.len() as u64
            || entry.points != source.definition.points.len()
            || entry.members != source.definition.members.len()
            || entry.constraints != source.definition.constraints.len()
        {
            return Err(invalid(
                "The indexed identity/provenance/counts disagree with this pinned native source",
            ));
        }
        // Re-admission verifies the stored immutable copy; do not accept a damaged cache.
        let pinned = crate::persistence::safe_join(root, &entry.file)?;
        if digest(&read_bounded(&pinned, resources::MAX_BYTES)?) != sha256 {
            return Err(invalid(
                "Existing pinned library bytes changed; retain the index and repair its resource explicitly",
            ));
        }
        let id = entry.id.clone();
        if entry.archived {
            entry.archived = false;
            save(root, &mut catalog)?;
        }
        return Ok((catalog, id));
    }
    let id = crate::id();
    let file = format!("blocks/{sha256}.tcadblock");
    let entry = Entry {
        id: id.clone(),
        name: short_name(&source.definition.name),
        category: String::new(),
        tags: vec![],
        favorite: false,
        archived: false,
        file: file.clone(),
        sha256: sha256.clone(),
        package_sha256: resources::sha256(&source)?,
        bytes: bytes.len() as u64,
        package_id: source.id.clone(),
        source: source.source.clone(),
        points: source.definition.points.len(),
        members: source.definition.members.len(),
        constraints: source.definition.constraints.len(),
    };
    catalog.entries.insert(id.clone(), entry);
    catalog.validate()?;
    let target = crate::persistence::safe_join(root, &file)?;
    let existed = target.exists();
    if existed {
        if digest(&read_bounded(&target, resources::MAX_BYTES)?) != sha256 {
            return Err(invalid(
                "The digest-named library resource was altered; repair it before adding this source",
            ));
        }
    } else {
        crate::persistence::atomic_write(&target, bytes)?;
    }
    if let Err(error) = save(root, &mut catalog) {
        if !existed {
            let _ = std::fs::remove_file(target);
        }
        return Err(error);
    }
    Ok((catalog, id))
}
pub fn add_file(root: &Path, source: &Path) -> Result<(Catalog, Id)> {
    add_bytes(root, &read_bounded(source, resources::MAX_BYTES)?)
}
pub fn add_package(root: &Path, source: &Package) -> Result<(Catalog, Id)> {
    resources::validate(source)?;
    add_bytes(root, &serde_json::to_vec_pretty(source)?)
}
pub fn pinned_bytes(root: &Path, id: &str, expected_sha: &str) -> Result<(Entry, Vec<u8>)> {
    let catalog = load(root)?;
    let entry = catalog
        .entries
        .get(id)
        .filter(|e| e.sha256 == expected_sha)
        .cloned()
        .ok_or_else(|| invalid("This library entry changed or was removed; refresh the browser"))?;
    let path = crate::persistence::safe_join(root, &entry.file)?;
    if std::fs::symlink_metadata(&path)?.file_type().is_symlink() {
        return Err(invalid(
            "Pinned library sources must be regular copied files",
        ));
    }
    let bytes = read_bounded(&path, resources::MAX_BYTES)?;
    if bytes.len() as u64 != entry.bytes || digest(&bytes) != entry.sha256 {
        return Err(invalid(
            "Pinned library bytes changed or are missing; the original source is never guessed",
        ));
    }
    let source = package(&bytes)?;
    if resources::sha256(&source)? != entry.package_sha256
        || source.id != entry.package_id
        || source.source != entry.source
        || source.definition.points.len() != entry.points
        || source.definition.members.len() != entry.members
        || source.definition.constraints.len() != entry.constraints
    {
        return Err(invalid(
            "The pinned native source disagrees with its indexed identity/provenance/counts",
        ));
    }
    Ok((entry, bytes))
}
pub fn read_entry(root: &Path, id: &str, expected_sha: &str) -> Result<Package> {
    let (entry, bytes) = pinned_bytes(root, id, expected_sha)?;
    if entry.archived {
        return Err(invalid("Restore this removed entry before inserting it"));
    }
    package(&bytes)
}
pub struct Metadata {
    pub name: String,
    pub category: String,
    pub tags: Vec<String>,
    pub favorite: bool,
    pub archived: bool,
}
pub fn update_metadata(
    root: &Path,
    id: &str,
    expected_sha: &str,
    expected_revision: u64,
    metadata: Metadata,
) -> Result<Catalog> {
    let _lock = lock(root)?;
    let mut catalog = load(root)?;
    if catalog.revision != expected_revision {
        return Err(invalid(
            "The library changed in another control/window; refresh before editing metadata",
        ));
    }
    let entry = catalog
        .entries
        .get_mut(id)
        .filter(|e| e.sha256 == expected_sha)
        .ok_or_else(|| invalid("Library target changed; refresh before editing"))?;
    entry.name = metadata.name;
    entry.category = metadata.category;
    entry.tags = metadata.tags;
    entry.favorite = metadata.favorite;
    entry.archived = metadata.archived;
    save(root, &mut catalog)?;
    Ok(catalog)
}
pub fn export(root: &Path, id: &str, expected_sha: &str, destination: &Path) -> Result<()> {
    let (_, bytes) = pinned_bytes(root, id, expected_sha)?;
    crate::persistence::atomic_write(destination, &bytes)
}
