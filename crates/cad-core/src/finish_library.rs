//! Lazy authored visual catalog and portable checksum-pinned finish libraries.
//! This contract carries appearance only, never density, strength or CAD geometry.
use crate::{CadError, Document, Id, Resource, Result, appearance::*};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    io::Read,
    path::Path,
    sync::{
        OnceLock, RwLock,
        atomic::{AtomicBool, Ordering},
    },
};

pub const LIBRARY_VERSION: u32 = 1;
pub const LIBRARY_FORMAT: &str = "tcad.surface-finishes";
pub const MAX_PACKAGE_BYTES: usize = 256 * 1024 * 1024;
pub const MAX_TEXTURE_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_TOTAL_TEXTURE_BYTES: usize = 64 * 1024 * 1024;
/// Finish presets are optional repository content. Only definitions copied into
/// a document become model data; browsing never installs a texture resource.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CatalogEntry {
    pub finish: Finish,
    pub category: String,
    pub tags: Vec<String>,
    pub source: String,
}
#[derive(Clone)]
struct CatalogSource {
    entry: crate::content_repositories::ResolvedEntry,
    original_id: Id,
}
#[derive(Default)]
struct Catalog {
    generation: Option<u64>,
    entries: Vec<CatalogEntry>,
    sources: BTreeMap<Id, CatalogSource>,
    diagnostics: Vec<String>,
}
fn cache() -> &'static RwLock<Catalog> {
    static CACHE: OnceLock<RwLock<Catalog>> = OnceLock::new();
    CACHE.get_or_init(|| RwLock::new(Catalog::default()))
}
static REFRESHING: AtomicBool = AtomicBool::new(false);
struct RefreshClaim;
impl RefreshClaim {
    fn acquire() -> Option<Self> {
        REFRESHING
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .ok()
            .map(|_| Self)
    }
}
impl Drop for RefreshClaim {
    fn drop(&mut self) {
        REFRESHING.store(false, Ordering::Release);
    }
}
/// Call after a repository refresh in the background. One decoder owns the
/// refresh; UI readers retain the prior snapshot without blocking on package IO.
pub fn refresh_catalog() {
    if let Some(_claim) = RefreshClaim::acquire() {
        rebuild_catalog();
    }
}
fn rebuild_catalog() {
    use crate::content_repositories::{self, ContentKind};
    let snapshot = content_repositories::catalog_snapshot();
    let mut next = Catalog {
        generation: Some(snapshot.generation),
        ..Catalog::default()
    };
    for entry in snapshot
        .entries
        .into_iter()
        .filter(|entry| entry.entry.kind == ContentKind::Finishes)
    {
        let result = content_repositories::read_entry(&entry, MAX_PACKAGE_BYTES)
            .and_then(|bytes| decode(&bytes));
        match result {
            Ok(package) => {
                for mut finish in package.finishes {
                    if next.entries.len() >= 4096 {
                        next.diagnostics.push("Installed finish libraries exceed 4096 presets; remove a library to show the remainder".into());
                        break;
                    }
                    let original_id = finish.id.clone();
                    finish.id = format!(
                        "repository-finish-{:x}",
                        Sha256::digest(
                            format!("{}:{}:{}", entry.repository_id, entry.entry.id, original_id)
                                .as_bytes()
                        )
                    );
                    let metadata = finish.metadata.get_or_insert_with(FinishMetadata::default);
                    metadata.catalog_id = Some(finish.id.clone());
                    let category = if metadata.category.is_empty() {
                        entry.repository_name.clone()
                    } else {
                        metadata.category.clone()
                    };
                    next.sources.insert(
                        finish.id.clone(),
                        CatalogSource {
                            entry: entry.clone(),
                            original_id,
                        },
                    );
                    next.entries.push(CatalogEntry {
                        category,
                        tags: metadata.tags.clone(),
                        source: entry.repository_name.clone(),
                        finish,
                    });
                }
            }
            Err(error) => next.diagnostics.push(format!(
                "{} / {}: {error}",
                entry.repository_name, entry.entry.name
            )),
        }
    }
    next.entries.sort_by(|left, right| {
        (&left.category, &left.finish.name).cmp(&(&right.category, &right.finish.name))
    });
    if let Ok(mut current) = cache().write() {
        *current = next;
    }
}
fn ensure_catalog() {
    let generation = crate::content_repositories::catalog_generation();
    if cache()
        .read()
        .is_ok_and(|cached| cached.generation == Some(generation))
    {
        return;
    }
    if let Some(claim) = RefreshClaim::acquire() {
        // Acquisition happens before spawning, so frames cannot queue duplicate
        // 256 MiB package decodes while the background manager is warming data.
        // Dropping the closure on a spawn failure releases the claim as well.
        let _ = std::thread::Builder::new()
            .name("tcad-finish-catalog".into())
            .spawn(move || {
                let _claim = claim;
                rebuild_catalog();
            });
    }
}
pub fn catalog() -> Vec<CatalogEntry> {
    ensure_catalog();
    cache()
        .read()
        .map(|cached| cached.entries.clone())
        .unwrap_or_default()
}
pub fn catalog_diagnostics() -> Vec<String> {
    ensure_catalog();
    cache()
        .read()
        .map(|cached| cached.diagnostics.clone())
        .unwrap_or_else(|_| vec!["Finish library cache is unavailable".into()])
}
/// Return the selected finish and only its checksum-pinned images. Re-reading
/// checks the reviewed repository digest before document resource installation.
pub fn package_for_catalog(catalog_id: &str) -> Result<Package> {
    ensure_catalog();
    let generation = crate::content_repositories::catalog_generation();
    let source = cache()
        .read()
        .ok()
        .filter(|cached| cached.generation == Some(generation))
        .and_then(|cached| cached.sources.get(catalog_id).cloned())
        .ok_or_else(|| {
            invalid("The selected repository finish is unavailable or its library is still refreshing; wait for Content Libraries to finish")
        })?;
    let bytes = crate::content_repositories::read_entry(&source.entry, MAX_PACKAGE_BYTES)?;
    let package = decode(&bytes)?;
    let mut finish = package
        .finishes
        .into_iter()
        .find(|finish| finish.id == source.original_id)
        .ok_or_else(|| {
            invalid("The selected repository finish no longer exists; refresh Content Libraries")
        })?;
    finish.id = catalog_id.into();
    finish
        .metadata
        .get_or_insert_with(FinishMetadata::default)
        .catalog_id = Some(catalog_id.into());
    let used = finish
        .textures
        .resources()
        .cloned()
        .collect::<BTreeSet<_>>();
    let mut selected = Package {
        format: LIBRARY_FORMAT.into(),
        version: LIBRARY_VERSION,
        finishes: vec![finish],
        textures: package
            .textures
            .into_iter()
            .filter(|texture| used.contains(&texture.resource.id))
            .collect(),
    };
    pin_legacy_maps(&mut selected)?;
    validate_package(&selected)?;
    Ok(selected)
}
/// Copy the preset and exact texture bytes into an isolated document draft.
/// Existing assignments are independent of future repository updates/removal.
pub fn install_catalog(doc: &mut Document, root: &Path, catalog_id: &str) -> Result<Id> {
    let package = package_for_catalog(catalog_id)?;
    install(doc, root, &package)?
        .into_iter()
        .next()
        .ok_or_else(|| invalid("The repository finish package is empty"))
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PortableTexture {
    pub resource: Resource,
    /// Exact encoded image bytes, pinned by the resource checksum. No file URI.
    pub bytes: Vec<u8>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Package {
    pub format: String,
    pub version: u32,
    pub finishes: Vec<Finish>,
    #[serde(default)]
    pub textures: Vec<PortableTexture>,
}
fn invalid(message: &str) -> CadError {
    CadError::Invalid(message.into())
}
fn bounded_read(path: &Path, maximum: usize) -> Result<Vec<u8>> {
    let file = File::open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > maximum as u64 {
        return Err(invalid(
            "Visual library textures must be nonempty regular files within the 64 MiB texture bound",
        ));
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take(maximum as u64 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > maximum {
        return Err(invalid("Texture grew beyond the library size bound"));
    }
    Ok(bytes)
}
pub fn export(doc: &Document, root: &Path, ids: &[Id]) -> Result<Package> {
    let mut finishes = vec![];
    let mut resources = BTreeSet::new();
    let mut selected = BTreeSet::new();
    for id in ids {
        if !selected.insert(id) {
            continue;
        }
        let finish = doc
            .finishes
            .get(id)
            .ok_or_else(|| invalid("An exported finish no longer exists"))?;
        validate_finish(doc, finish)?;
        resources.extend(finish.textures.resources().cloned());
        finishes.push(finish.clone());
    }
    let mut textures = vec![];
    let mut total = 0_usize;
    for id in resources {
        let resource = doc
            .resources
            .get(&id)
            .ok_or_else(|| invalid("An exported finish texture is missing"))?;
        let bytes = bounded_read(
            &crate::persistence::safe_join(root, &resource.path)?,
            MAX_TEXTURE_BYTES,
        )?;
        total = total.saturating_add(bytes.len());
        if total > MAX_TOTAL_TEXTURE_BYTES {
            return Err(invalid(
                "A portable finish library supports at most 64 MiB of encoded texture bytes; export smaller finish groups",
            ));
        }
        textures.push(PortableTexture {
            resource: resource.clone(),
            bytes,
        });
    }
    let mut package = Package {
        format: LIBRARY_FORMAT.into(),
        version: LIBRARY_VERSION,
        finishes,
        textures,
    };
    pin_legacy_maps(&mut package)?;
    validate_package(&package)?;
    Ok(package)
}
/// A portable export must not depend on an optional installed legacy map.
/// Resolve once, include exact bytes, and remove the external identity from the
/// exported copy. The document's existing finish is never rewritten here.
fn pin_legacy_maps(package: &mut Package) -> Result<()> {
    use crate::content_repositories::{self, ContentKind};
    let entries = content_repositories::entries(ContentKind::Textures);
    for finish in &mut package.finishes {
        if finish.textures.height.is_some() {
            continue;
        }
        let Some(map_id) = finish.builtin_bump.repository_entry() else {
            continue;
        };
        let matches = entries
            .iter()
            .filter(|entry| entry.entry.id == map_id)
            .collect::<Vec<_>>();
        let [entry] = matches.as_slice() else {
            return Err(CadError::Invalid(format!(
                "Finish {} needs one installed texture named {map_id}. Get its library and pin the intended image before exporting or using this repository finish.",
                finish.name
            )));
        };
        let bytes = content_repositories::read_entry(entry, MAX_TEXTURE_BYTES)?;
        let kind = entry
            .path
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or("")
            .to_lowercase();
        if !TEXTURE_KINDS.contains(&kind.as_str()) {
            return Err(invalid("The legacy map is not a supported image file"));
        }
        let sha256 = format!("{:x}", Sha256::digest(&bytes));
        let resource_id = if let Some(existing) = package
            .textures
            .iter()
            .find(|texture| texture.resource.sha256 == sha256 && texture.resource.kind == kind)
        {
            existing.resource.id.clone()
        } else {
            let resource_id = format!("legacy-map-{sha256}");
            if package
                .textures
                .iter()
                .any(|texture| texture.resource.id == resource_id)
            {
                return Err(invalid("Legacy texture resource identity collision"));
            }
            package.textures.push(PortableTexture {
                resource: Resource {
                    id: resource_id.clone(),
                    path: format!("resources/{resource_id}.{kind}"),
                    kind,
                    sha256,
                },
                bytes,
            });
            resource_id
        };
        finish.textures.height = Some(resource_id);
        finish.builtin_bump = BuiltinBump::None;
    }
    Ok(())
}
fn validate_package(package: &Package) -> Result<()> {
    if package.format != LIBRARY_FORMAT || package.version != LIBRARY_VERSION {
        return Err(invalid("Unsupported visual finish library format/version"));
    }
    if package.finishes.is_empty() || package.finishes.len() > 1024 || package.textures.len() > 1024
    {
        return Err(invalid(
            "A portable finish library requires 1–1024 finishes and at most 1024 textures",
        ));
    }
    let mut document = Document::empty("Visual finish library validation");
    let mut total = 0_usize;
    for texture in &package.textures {
        total = total.saturating_add(texture.bytes.len());
        if texture.bytes.is_empty()
            || texture.bytes.len() > MAX_TEXTURE_BYTES
            || total > MAX_TOTAL_TEXTURE_BYTES
        {
            return Err(invalid(
                "A finish library supports at most 64 MiB of encoded texture bytes",
            ));
        }
        if format!("{:x}", Sha256::digest(&texture.bytes)) != texture.resource.sha256 {
            return Err(invalid(
                "Portable texture bytes do not match their pinned SHA-256 checksum",
            ));
        }
        if document
            .resources
            .insert(texture.resource.id.clone(), texture.resource.clone())
            .is_some()
        {
            return Err(invalid("Duplicate portable texture identity"));
        }
    }
    let mut ids = BTreeSet::new();
    let mut used = BTreeSet::new();
    for finish in &package.finishes {
        if !ids.insert(&finish.id) {
            return Err(invalid("Duplicate finish identity in portable library"));
        }
        validate_finish(&document, finish)?;
        used.extend(finish.textures.resources().cloned());
    }
    if used.len() != package.textures.len()
        || package
            .textures
            .iter()
            .any(|texture| !used.contains(&texture.resource.id))
    {
        return Err(invalid(
            "Portable libraries may include only textures referenced by their finishes",
        ));
    }
    Ok(())
}
pub fn encode(package: &Package) -> Result<Vec<u8>> {
    validate_package(package)?;
    let bytes = serde_json::to_vec(package)?;
    if bytes.len() > MAX_PACKAGE_BYTES {
        return Err(invalid(
            "Portable finish library exceeds the 256 MiB JSON size bound",
        ));
    }
    Ok(bytes)
}
pub fn decode(bytes: &[u8]) -> Result<Package> {
    if bytes.is_empty() || bytes.len() > MAX_PACKAGE_BYTES {
        return Err(invalid(
            "Finish library must be nonempty JSON no larger than 256 MiB",
        ));
    }
    let package: Package = serde_json::from_slice(bytes)?;
    validate_package(&package)?;
    Ok(package)
}
/// Stage immutable assets and install finishes atomically in the supplied source
/// clone. UI Apply still waits for current bounded image decoding readiness.
pub fn install(doc: &mut Document, root: &Path, package: &Package) -> Result<Vec<Id>> {
    validate_package(package)?;
    let mut staged = doc.clone();
    let mut mapping = BTreeMap::new();
    for texture in &package.textures {
        let resource = &texture.resource;
        let existing = staged
            .resources
            .values()
            .find(|existing| {
                existing.sha256 == resource.sha256
                    && existing.kind.eq_ignore_ascii_case(&resource.kind)
                    && crate::persistence::safe_join(root, &existing.path)
                        .ok()
                        .and_then(|path| bounded_read(&path, MAX_TEXTURE_BYTES).ok())
                        .is_some_and(|bytes| {
                            format!("{:x}", Sha256::digest(bytes)) == resource.sha256
                        })
            })
            .map(|resource| resource.id.clone());
        let id = if let Some(id) = existing {
            id
        } else {
            crate::persistence::import_resource_bytes(
                &mut staged,
                root,
                Path::new(&format!("texture.{}", resource.kind)),
                &texture.bytes,
            )?
        };
        mapping.insert(resource.id.clone(), id);
    }
    let mut installed = vec![];
    for source in &package.finishes {
        let mut finish = source.clone();
        for slot in [
            &mut finish.textures.base_color,
            &mut finish.textures.normal,
            &mut finish.textures.roughness,
            &mut finish.textures.metallic,
            &mut finish.textures.occlusion,
            &mut finish.textures.height,
        ] {
            if let Some(id) = slot.as_mut() {
                *id = mapping
                    .get(id)
                    .ok_or_else(|| invalid("Imported finish texture is missing"))?
                    .clone();
            }
        }
        if let Some(existing) = staged
            .finishes
            .values()
            .find(|existing| **existing == finish)
        {
            installed.push(existing.id.clone());
            continue;
        }
        if staged.finishes.contains_key(&finish.id) {
            finish.id = crate::id();
        }
        validate_finish(&staged, &finish)?;
        installed.push(finish.id.clone());
        staged.finishes.insert(finish.id.clone(), finish);
    }
    staged.validate()?;
    *doc = staged;
    Ok(installed)
}
