//! Optional, data-only libraries. Installations belong to the user, not a CAD document.
//! Repository updates create a separate checkout and require an explicit apply.
use crate::{CadError, Result, forge::CancelToken, repository_models::GitAuthor};
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    sync::{
        Mutex, OnceLock,
        atomic::{AtomicU64, Ordering},
    },
};

mod git;
mod operations;
mod storage;
pub use operations::*;

pub const CONTRACT_VERSION: u32 = 1;
pub const MANIFEST_FILE: &str = "tcad-library.json";

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContentKind {
    WeldmentProfiles,
    Textures,
    Themes,
    Materials,
    StandardParts,
    Finishes,
}

impl ContentKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::WeldmentProfiles => "Weldment profiles",
            Self::Textures => "Textures",
            Self::Themes => "Themes",
            Self::Materials => "Materials",
            Self::StandardParts => "Standard parts",
            Self::Finishes => "Finishes",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LibraryEntry {
    pub id: String,
    pub name: String,
    pub kind: ContentKind,
    pub path: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LibraryManifest {
    pub version: u32,
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub entries: Vec<LibraryEntry>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InstalledRepository {
    /// Personal installation identity; a fork gets a distinct identity.
    pub id: String,
    pub root: PathBuf,
    pub manifest: LibraryManifest,
    /// Reviewed commit, or None while a new local repository is still uncommitted.
    pub revision: Option<String>,
    /// Source used to inspect updates. Publishing always has its own reviewed URL.
    pub remote: Option<String>,
    pub branch: String,
    pub enabled: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Registry {
    pub version: u32,
    pub repositories: Vec<InstalledRepository>,
}
impl Default for Registry {
    fn default() -> Self {
        Self {
            version: CONTRACT_VERSION,
            repositories: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ResolvedEntry {
    pub repository_id: String,
    pub repository_name: String,
    pub root: PathBuf,
    pub revision: Option<String>,
    pub entry: LibraryEntry,
    pub path: PathBuf,
    pub sha256: String,
}

#[derive(Clone, Debug, Default)]
pub struct CatalogSnapshot {
    pub generation: u64,
    pub entries: Vec<ResolvedEntry>,
    pub notices: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RepositoryInspection {
    pub repository: InstalledRepository,
    pub fingerprint: String,
    pub branch: String,
    pub head: Option<String>,
    pub status: String,
    pub file_count: usize,
    pub files: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CommitRequest {
    pub repository_id: String,
    pub message: String,
    pub author: GitAuthor,
    pub expected_fingerprint: String,
    pub expected_head: Option<String>,
    pub expected_branch: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CommitResult {
    pub repository: InstalledRepository,
    pub commit: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PublishRequest {
    pub repository_id: String,
    pub clone_url: String,
    pub branch: String,
    /// None means the user reviewed an empty target branch.
    pub expected_remote_commit: Option<String>,
    pub remote_reviewed: bool,
    pub expected_local_commit: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PublishResult {
    pub repository: InstalledRepository,
    pub commit: String,
    pub branch: String,
    pub notices: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UpdateCandidate {
    pub repository_id: String,
    pub previous_root: PathBuf,
    pub previous_revision: Option<String>,
    pub previous_branch: String,
    pub previous_fingerprint: String,
    pub checkout: InstalledRepository,
    pub fingerprint: String,
    pub added: Vec<String>,
    pub removed: Vec<String>,
    pub changed: Vec<String>,
}

static REGISTRY_LOCK: Mutex<()> = Mutex::new(());
static REFRESH_LOCK: Mutex<()> = Mutex::new(());
static CACHE: OnceLock<Mutex<Option<CatalogSnapshot>>> = OnceLock::new();
static GENERATION: AtomicU64 = AtomicU64::new(0);

fn invalid(message: impl Into<String>) -> CadError {
    CadError::Invalid(message.into())
}

pub fn registry_path() -> Result<PathBuf> {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .ok_or_else(|| invalid("The user home folder is unavailable"))?;
    Ok(PathBuf::from(home).join(".tcad/content-repositories.json"))
}

pub fn load_registry() -> Result<Registry> {
    storage::load_registry()
}

pub fn save_registry(registry: &Registry) -> Result<()> {
    let _guard = REGISTRY_LOCK
        .lock()
        .map_err(|_| invalid("Content library preferences are unavailable"))?;
    storage::save_registry(registry)?;
    refresh_catalog()?;
    Ok(())
}

/// Explicit refresh, intended for a worker. Consumers use the cached snapshot in frames.
pub fn refresh_catalog() -> Result<CatalogSnapshot> {
    refresh_catalog_with_cancel(&CancelToken::default())
}

pub fn refresh_catalog_with_cancel(cancel: &CancelToken) -> Result<CatalogSnapshot> {
    cancel.check()?;
    let _refresh = REFRESH_LOCK
        .lock()
        .map_err(|_| invalid("Content library refresh is unavailable"))?;
    let registry = load_registry()?;
    let mut snapshot = CatalogSnapshot::default();
    for repository in registry
        .repositories
        .iter()
        .filter(|repository| repository.enabled)
    {
        cancel.check()?;
        match storage::snapshot(&repository.root, cancel) {
            Ok(library) => {
                let git = git::Git::local(&library.root);
                let revision = match git.validate_root(cancel).and_then(|_| git.head(cancel)) {
                    Ok(revision) => revision,
                    Err(error) => {
                        cancel.check()?;
                        snapshot
                            .notices
                            .push(format!("{}: {error}", repository.manifest.name));
                        continue;
                    }
                };
                for entry in &library.manifest.entries {
                    if let Some(file) = library.files.get(&entry.path) {
                        snapshot.entries.push(ResolvedEntry {
                            repository_id: repository.id.clone(),
                            repository_name: library.manifest.name.clone(),
                            root: library.root.clone(),
                            revision: revision.clone(),
                            entry: entry.clone(),
                            path: library.root.join(&entry.path),
                            sha256: file.sha256.clone(),
                        });
                    }
                }
            }
            Err(error) => {
                cancel.check()?;
                snapshot
                    .notices
                    .push(format!("{}: {error}", repository.manifest.name));
            }
        }
    }
    cancel.check()?;
    let mut cache = CACHE
        .get_or_init(|| Mutex::new(None))
        .lock()
        .map_err(|_| invalid("Content library cache is unavailable"))?;
    snapshot.generation = GENERATION.fetch_add(1, Ordering::Relaxed) + 1;
    *cache = Some(snapshot.clone());
    Ok(snapshot)
}

pub fn catalog_snapshot() -> CatalogSnapshot {
    if let Ok(cache) = CACHE.get_or_init(|| Mutex::new(None)).lock()
        && let Some(snapshot) = &*cache
    {
        return snapshot.clone();
    }
    refresh_catalog().unwrap_or_else(|error| {
        let mut snapshot = CatalogSnapshot {
            generation: 0,
            notices: vec![error.to_string()],
            ..Default::default()
        };
        if let Ok(mut cache) = CACHE.get_or_init(|| Mutex::new(None)).lock() {
            if let Some(existing) = &*cache {
                return existing.clone();
            }
            snapshot.generation = GENERATION.fetch_add(1, Ordering::Relaxed) + 1;
            *cache = Some(snapshot.clone());
        }
        snapshot
    })
}

pub fn catalog_generation() -> u64 {
    GENERATION.load(Ordering::Relaxed)
}

pub fn entries(kind: ContentKind) -> Vec<ResolvedEntry> {
    catalog_snapshot()
        .entries
        .into_iter()
        .filter(|entry| entry.entry.kind == kind)
        .collect()
}

/// Recheck the path and snapshot digest at consumption, including locally edited content.
pub fn read_entry(entry: &ResolvedEntry, max_bytes: usize) -> Result<Vec<u8>> {
    let root = storage::canonical_root(&entry.root)?;
    let path = storage::checked_file(&root, &entry.entry.path)?;
    if path != entry.path {
        return Err(invalid(
            "The library entry path changed; refresh the library",
        ));
    }
    let bytes = storage::read_bounded(&path, max_bytes.min(storage::MAX_FILE as usize) as u64)?;
    if storage::digest(&bytes) != entry.sha256 {
        return Err(invalid(
            "The library entry changed; refresh the library before using it",
        ));
    }
    Ok(bytes)
}

/// Validate and read a library manifest without installing it.
pub fn inspect_manifest(root: &Path, cancel: &CancelToken) -> Result<LibraryManifest> {
    Ok(storage::snapshot(root, cancel)?.manifest)
}
