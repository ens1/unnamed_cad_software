use super::*;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::Component,
};

pub(super) const MAX_FILE: u64 = 100 * 1024 * 1024;
const MAX_TOTAL: u64 = 1024 * 1024 * 1024;
const MAX_FILES: usize = 25_000;
const MAX_MANIFEST: u64 = 4 * 1024 * 1024;

#[derive(Clone)]
pub(super) struct FileRecord {
    pub sha256: String,
    pub size: u64,
}
pub(super) struct Snapshot {
    pub root: PathBuf,
    pub manifest: LibraryManifest,
    pub files: BTreeMap<String, FileRecord>,
    pub fingerprint: String,
}

pub(super) fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub(super) fn read_bounded(path: &Path, maximum: u64) -> Result<Vec<u8>> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(target_os = "macos")]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(0x100 | 0x4);
    }
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(0x20000 | 0x800);
    }
    let mut file = options.open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.len() > maximum {
        return Err(invalid(format!(
            "{} exceeds the supported file size",
            path.display()
        )));
    }
    let mut bytes = Vec::new();
    (&mut file).take(maximum + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > maximum {
        return Err(invalid("The library file grew while it was being read"));
    }
    Ok(bytes)
}

pub(super) fn relative_path(value: &str) -> Result<PathBuf> {
    if value.is_empty()
        || value.len() > 4096
        || value.contains('\\')
        || value.chars().any(char::is_control)
    {
        return Err(invalid(
            "Library paths must be portable relative file paths",
        ));
    }
    let path = Path::new(value);
    for part in path.components() {
        let Component::Normal(part) = part else {
            return Err(invalid("Library paths must stay inside their repository"));
        };
        let name = part
            .to_str()
            .ok_or_else(|| invalid("Library paths must use UTF-8"))?;
        if name.contains(':')
            || name.ends_with(['.', ' '])
            || matches!(
                name.to_ascii_lowercase().as_str(),
                ".git" | ".tcad-cache" | ".tcad-recovery"
            )
        {
            return Err(invalid(
                "Library paths cannot use Git, cache or recovery folders",
            ));
        }
    }
    Ok(path.to_path_buf())
}

pub(super) fn canonical_root(path: &Path) -> Result<PathBuf> {
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    // macOS /var and /tmp aliases are part of the trusted parent path, but the
    // selected repository itself and every resource underneath it must be real.
    if path
        .components()
        .any(|part| matches!(part, Component::ParentDir))
    {
        return Err(invalid(
            "Choose a repository path without parent-directory components",
        ));
    }
    let metadata = fs::symlink_metadata(&path)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(invalid(
            "Choose a real library directory, not a symbolic link",
        ));
    }
    Ok(path.canonicalize()?)
}

pub(super) fn checked_file(root: &Path, relative: &str) -> Result<PathBuf> {
    let mut file = root.to_path_buf();
    for part in relative_path(relative)?.components() {
        file.push(part);
        if fs::symlink_metadata(&file)?.file_type().is_symlink() {
            return Err(invalid(format!(
                "Library resources cannot be symbolic links: {relative}"
            )));
        }
    }
    if !fs::metadata(&file)?.is_file() {
        return Err(invalid(format!("Library entry is not a file: {relative}")));
    }
    if !file.canonicalize()?.starts_with(root) {
        return Err(invalid(
            "Library resources must stay inside their repository",
        ));
    }
    Ok(file)
}

fn identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 160
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

pub(super) fn validate_manifest(manifest: &LibraryManifest) -> Result<()> {
    if manifest.version != CONTRACT_VERSION {
        return Err(invalid("Unsupported content library version"));
    }
    if !identifier(&manifest.id)
        || manifest.name.trim().is_empty()
        || manifest.name.len() > 512
        || manifest.name.chars().any(char::is_control)
        || manifest.description.len() > 16_384
        || manifest.entries.len() > 10_000
    {
        return Err(invalid(
            "Invalid content library identity, name or entry count",
        ));
    }
    let mut ids = BTreeSet::new();
    for entry in &manifest.entries {
        if !identifier(&entry.id)
            || !ids.insert(&entry.id)
            || entry.name.trim().is_empty()
            || entry.name.len() > 512
            || entry.name.chars().any(char::is_control)
        {
            return Err(invalid(
                "Library entries need distinct IDs and readable names",
            ));
        }
        relative_path(&entry.path)?;
        let extension = Path::new(&entry.path)
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        let supported = match entry.kind {
            ContentKind::WeldmentProfiles | ContentKind::Materials | ContentKind::StandardParts => {
                extension == "json"
            }
            ContentKind::Themes => extension == "json",
            ContentKind::Finishes => matches!(extension.as_str(), "tcadfinishes" | "json"),
            ContentKind::Textures => matches!(
                extension.as_str(),
                "png" | "jpg" | "jpeg" | "webp" | "bmp" | "tif" | "tiff" | "hdr" | "exr"
            ),
        };
        if !supported {
            return Err(invalid(format!(
                "{} has an unsupported data file for {}",
                entry.name,
                entry.kind.label()
            )));
        }
    }
    Ok(())
}

fn allowed_data_file(path: &str) -> bool {
    let file = Path::new(path);
    let name = file
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    if matches!(
        name.to_ascii_lowercase().as_str(),
        ".gitignore" | ".gitattributes" | "license" | "copying" | "notice" | "authors"
    ) {
        return true;
    }
    matches!(
        file.extension()
            .and_then(|extension| extension.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase()
            .as_str(),
        "json"
            | "css"
            | "png"
            | "jpg"
            | "jpeg"
            | "webp"
            | "bmp"
            | "tif"
            | "tiff"
            | "hdr"
            | "exr"
            | "tcadfinishes"
            | "tcad"
            | "step"
            | "stp"
            | "iges"
            | "igs"
            | "stl"
            | "obj"
            | "mtl"
            | "gltf"
            | "glb"
            | "md"
            | "txt"
            | "csv"
            | "toml"
            | "yaml"
            | "yml"
            | "woff"
            | "woff2"
            | "ttf"
            | "otf"
    )
}

pub(super) fn snapshot(root: &Path, cancel: &CancelToken) -> Result<Snapshot> {
    cancel.check()?;
    let root = canonical_root(root)?;
    let ignored = ignored_untracked(&root, cancel)?;
    if is_ignored(MANIFEST_FILE, &ignored) {
        return Err(invalid(
            "tcad-library.json is ignored and untracked. Remove its ignore rule or track it in Git before using this library",
        ));
    }
    let manifest_bytes = read_bounded(&checked_file(&root, MANIFEST_FILE)?, MAX_MANIFEST)?;
    let manifest: LibraryManifest = serde_json::from_slice(&manifest_bytes)?;
    validate_manifest(&manifest)?;
    if let Some(entry) = manifest
        .entries
        .iter()
        .find(|entry| is_ignored(&entry.path, &ignored))
    {
        return Err(invalid(format!(
            "Library entry {} is ignored and untracked. Remove its ignore rule or track the resource in Git before adding it to the library",
            entry.path
        )));
    }
    let mut files = BTreeMap::new();
    let mut queue = vec![(root.clone(), 0usize)];
    let mut size = 0u64;
    let mut visited = 0usize;
    while let Some((directory, depth)) = queue.pop() {
        cancel.check()?;
        if depth > 24 {
            return Err(invalid("Library folders exceed 24 levels"));
        }
        for child in fs::read_dir(&directory)? {
            cancel.check()?;
            visited += 1;
            if visited > 50_000 {
                return Err(invalid(
                    "The library has more than 50,000 directory entries",
                ));
            }
            let child = child?;
            let name = child.file_name();
            let name = name
                .to_str()
                .ok_or_else(|| invalid("Library names must be UTF-8"))?;
            if matches!(
                name,
                ".git" | ".tcad-cache" | ".tcad-recovery" | ".DS_Store"
            ) {
                continue;
            }
            let path = child.path();
            let relative = path
                .strip_prefix(&root)
                .map_err(|_| invalid("Invalid library file path"))?
                .to_str()
                .ok_or_else(|| invalid("Library paths must be UTF-8"))?
                .replace('\\', "/");
            if is_ignored(&relative, &ignored) {
                continue;
            }
            relative_path(&relative)?;
            let metadata = fs::symlink_metadata(&path)?;
            if metadata.file_type().is_symlink() {
                return Err(invalid(format!(
                    "Library resources cannot be symbolic links: {relative}"
                )));
            }
            if metadata.is_dir() {
                queue.push((path, depth + 1));
                continue;
            }
            if !metadata.is_file() {
                return Err(invalid(format!("Unsupported library resource: {relative}")));
            }
            if !allowed_data_file(&relative) {
                return Err(invalid(format!(
                    "Library repositories contain data only. Remove or move the unsupported file: {relative}"
                )));
            }
            if files.len() >= MAX_FILES || metadata.len() > MAX_FILE {
                return Err(invalid("Library exceeds 25,000 files or 100 MiB per file"));
            }
            let bytes = read_bounded(&path, MAX_FILE)?;
            size = size
                .checked_add(bytes.len() as u64)
                .ok_or_else(|| invalid("Library is too large"))?;
            if size > MAX_TOTAL {
                return Err(invalid("Library content exceeds 1 GiB"));
            }
            files.insert(
                relative,
                FileRecord {
                    sha256: digest(&bytes),
                    size: bytes.len() as u64,
                },
            );
        }
        if queue.len() > MAX_FILES {
            return Err(invalid("Library has too many folders"));
        }
    }
    if files.get(MANIFEST_FILE).map(|record| &record.sha256) != Some(&digest(&manifest_bytes)) {
        return Err(invalid(
            "The library manifest changed while being read; refresh and try again",
        ));
    }
    validate_resources(&root, &manifest, cancel)?;
    for entry in &manifest.entries {
        if !files.contains_key(&entry.path) {
            return Err(invalid(format!(
                "The library entry is missing: {}",
                entry.path
            )));
        }
    }
    let mut fingerprint = Sha256::new();
    for (name, file) in &files {
        fingerprint.update((name.len() as u64).to_le_bytes());
        fingerprint.update(name.as_bytes());
        fingerprint.update(file.size.to_le_bytes());
        fingerprint.update(file.sha256.as_bytes());
    }
    Ok(Snapshot {
        root,
        manifest,
        files,
        fingerprint: format!("{:x}", fingerprint.finalize()),
    })
}

fn ignored_untracked(root: &Path, cancel: &CancelToken) -> Result<BTreeSet<String>> {
    let Ok(metadata) = fs::symlink_metadata(root.join(".git")) else {
        return Ok(BTreeSet::new());
    };
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Ok(BTreeSet::new());
    }
    let output = git::Git::local(root).run(
        &[
            "ls-files",
            "--others",
            "--ignored",
            "--exclude-standard",
            "--directory",
            "-z",
        ],
        cancel,
        &mut |_| {},
    )?;
    let mut ignored = BTreeSet::new();
    for path in output.split('\0').filter(|path| !path.is_empty()) {
        let path = path.trim_end_matches('/');
        relative_path(path)?;
        ignored.insert(path.to_owned());
        if ignored.len() > MAX_FILES {
            return Err(invalid("Library ignored paths exceed 25,000 entries"));
        }
    }
    Ok(ignored)
}

fn is_ignored(path: &str, ignored: &BTreeSet<String>) -> bool {
    ignored.contains(path)
        || path
            .match_indices('/')
            .any(|(index, _)| ignored.contains(&path[..index]))
}

pub(super) fn validate_resources(
    root: &Path,
    manifest: &LibraryManifest,
    cancel: &CancelToken,
) -> Result<()> {
    let ignored = ignored_untracked(root, cancel)?;
    for entry in &manifest.entries {
        cancel.check()?;
        if is_ignored(&entry.path, &ignored) {
            return Err(invalid(format!(
                "Library entry {} is ignored and untracked. Remove its ignore rule or track the resource in Git before adding it to the library",
                entry.path
            )));
        }
        let path = checked_file(root, &entry.path)?;
        if fs::metadata(&path)?.len() > MAX_FILE {
            return Err(invalid(format!(
                "The library entry exceeds 100 MiB: {}",
                entry.path
            )));
        }
        if entry.kind == ContentKind::Themes {
            validate_theme_references(root, &entry.path, &ignored)?;
        }
    }
    Ok(())
}

fn validate_theme_references(root: &Path, entry: &str, ignored: &BTreeSet<String>) -> Result<()> {
    let mut queue = vec![entry.to_owned()];
    let mut seen = BTreeSet::new();
    while let Some(relative) = queue.pop() {
        if is_ignored(&relative, ignored) {
            return Err(invalid(format!(
                "The theme references an ignored, untracked file: {relative}. Track it or remove the ignore rule before using this library"
            )));
        }
        if !seen.insert(relative.clone()) {
            continue;
        }
        if seen.len() > 256 {
            return Err(invalid("A theme references more than 256 files"));
        }
        let path = checked_file(root, &relative)?;
        let bytes = read_bounded(&path, MAX_MANIFEST)?;
        let mut references = Vec::new();
        if path.extension().is_some_and(|extension| extension == "css") {
            let text =
                std::str::from_utf8(&bytes).map_err(|_| invalid("Theme CSS must use UTF-8"))?;
            let mut remaining = text;
            loop {
                remaining = remaining.trim_start();
                if let Some(comment) = remaining.strip_prefix("/*") {
                    let end = comment
                        .find("*/")
                        .ok_or_else(|| invalid("Theme CSS contains an unclosed comment"))?;
                    remaining = &comment[end + 2..];
                    continue;
                }
                let Some(import) = remaining.strip_prefix("@import") else {
                    break;
                };
                let (path, rest) = import
                    .split_once(';')
                    .ok_or_else(|| invalid("CSS @import requires a terminating semicolon"))?;
                let path = path.trim();
                // Match the presentation loader's quoted leading local imports.
                let Some(quote @ ('\'' | '"')) = path.chars().next() else {
                    return Err(invalid("Theme imports must use quoted local paths"));
                };
                let path = path
                    .strip_prefix(quote)
                    .and_then(|path| path.strip_suffix(quote))
                    .filter(|path| !path.contains(quote))
                    .ok_or_else(|| invalid("Theme imports need one quoted local path"))?;
                references.push(path.to_owned());
                remaining = rest;
            }
        } else {
            let value: serde_json::Value = serde_json::from_slice(&bytes)?;
            if let Some(extends) = value.get("extends") {
                match extends {
                    serde_json::Value::String(path) => references.push(path.clone()),
                    serde_json::Value::Array(paths) => {
                        for path in paths {
                            references.push(
                                path.as_str()
                                    .ok_or_else(|| {
                                        invalid("Theme extends must contain local paths")
                                    })?
                                    .to_owned(),
                            );
                        }
                    }
                    _ => return Err(invalid("Theme extends must contain local paths")),
                }
            }
            for field in ["layout", "theme"] {
                if let Some(serde_json::Value::String(path)) = value.get(field) {
                    references.push(path.clone());
                }
            }
        }
        for reference in references {
            let relative = Path::new(&relative)
                .parent()
                .unwrap_or_else(|| Path::new(""))
                .join(relative_path(&reference)?);
            let name = relative
                .to_str()
                .ok_or_else(|| invalid("Theme paths must use UTF-8"))?
                .replace('\\', "/");
            checked_file(root, &name)?;
            queue.push(name);
        }
    }
    Ok(())
}

pub(super) fn load_registry() -> Result<Registry> {
    let path = registry_path()?;
    if !path.exists() {
        return Ok(Registry::default());
    }
    let registry: Registry = serde_json::from_slice(&read_bounded(&path, 4 * 1024 * 1024)?)?;
    validate_registry(&registry)?;
    Ok(registry)
}

fn validate_registry(registry: &Registry) -> Result<()> {
    if registry.version != CONTRACT_VERSION || registry.repositories.len() > 1000 {
        return Err(invalid("Unsupported content library preferences"));
    }
    let mut ids = BTreeSet::new();
    let mut roots = BTreeSet::new();
    for repository in &registry.repositories {
        if !identifier(&repository.id)
            || !ids.insert(&repository.id)
            || !roots.insert(&repository.root)
            || !repository.root.is_absolute()
        {
            return Err(invalid(
                "Content library installations need distinct IDs and absolute paths",
            ));
        }
        validate_manifest(&repository.manifest)?;
        if let Some(remote) = &repository.remote {
            crate::git_connections::RepositoryAddress::parse(remote)?;
        }
        if let Some(revision) = &repository.revision {
            git::validate_commit(revision)?;
        }
    }
    Ok(())
}

pub(super) fn save_registry(registry: &Registry) -> Result<()> {
    validate_registry(registry)?;
    let path = registry_path()?;
    let parent = path
        .parent()
        .ok_or_else(|| invalid("Invalid content preferences path"))?;
    fs::create_dir_all(parent)?;
    atomic_write(&path, &serde_json::to_vec_pretty(registry)?)
}

pub(super) fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let temporary = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let result = (|| {
        let mut file = options.open(&temporary)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::rename(&temporary, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result
}
