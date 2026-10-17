use crate::{CadError, Document, Id, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    path::{Path, PathBuf},
};
#[derive(Serialize, Deserialize)]
struct Manifest {
    schema_version: u32,
    document: Document,
    parts: BTreeMap<Id, String>,
    assemblies: BTreeMap<Id, String>,
}
pub trait ProjectStore {
    fn load(&self, path: &Path) -> Result<Document>;
    fn save(&self, path: &Path, document: &Document) -> Result<()>;
}
pub struct FileStore;
pub fn atomic_write(path: &Path, data: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temp = path.with_extension(format!("{}.tmp", crate::id()));
    let mut file = fs::File::create(&temp)?;
    file.write_all(data)?;
    file.sync_all()?;
    fs::rename(temp, path)?;
    #[cfg(unix)]
    if let Some(parent) = path.parent() {
        fs::File::open(parent)?.sync_all()?;
    }
    Ok(())
}
pub fn project_root(path: &Path) -> PathBuf {
    if path.extension().is_some_and(|e| e == "json") {
        path.parent().unwrap_or(Path::new(".")).to_path_buf()
    } else {
        path.to_path_buf()
    }
}
fn manifest_path(path: &Path) -> PathBuf {
    if path.extension().is_some_and(|e| e == "json") {
        path.to_path_buf()
    } else {
        path.join("project.json")
    }
}
pub fn safe_join(root: &Path, relative: &str) -> Result<PathBuf> {
    let p = Path::new(relative);
    if p.is_absolute()
        || p.components().any(|c| {
            matches!(
                c,
                std::path::Component::ParentDir | std::path::Component::Prefix(_)
            )
        })
    {
        return Err(CadError::Invalid(
            "Project paths must be portable relative paths".into(),
        ));
    }
    Ok(root.join(p))
}
impl ProjectStore for FileStore {
    fn load(&self, path: &Path) -> Result<Document> {
        let root = project_root(path);
        let journal = root.join(".tcad-recovery/pending.json");
        if journal.exists() {
            let pending: Vec<(String, String)> = serde_json::from_slice(&fs::read(&journal)?)?;
            for (staged, destination) in pending {
                let from = safe_join(&root, &staged)?;
                let to = safe_join(&root, &destination)?;
                if from.exists() {
                    if let Some(p) = to.parent() {
                        fs::create_dir_all(p)?;
                    }
                    fs::rename(from, to)?;
                }
            }
            fs::remove_file(journal)?;
        }
        let mut manifest: Manifest = serde_json::from_slice(&fs::read(manifest_path(path))?)?;
        if manifest.schema_version != 1 {
            return Err(CadError::Invalid("Unsupported project schema".into()));
        }
        for (id, file) in manifest.parts {
            let part = serde_json::from_slice(&fs::read(safe_join(&root, &file)?)?)?;
            manifest.document.parts.insert(id, part);
        }
        for (id, file) in manifest.assemblies {
            let a = serde_json::from_slice(&fs::read(safe_join(&root, &file)?)?)?;
            manifest.document.assemblies.insert(id, a);
        }
        crate::library::hydrate_pins(&mut manifest.document, &root);
        manifest.document.validate()?;
        Ok(manifest.document)
    }
    fn save(&self, path: &Path, doc: &Document) -> Result<()> {
        if crate::release::is_release_source(path) {
            return Err(CadError::Invalid("Released source is read-only. Use Save As to create a working copy outside the release folder".into()));
        }
        doc.validate()?;
        let root = project_root(path);
        fs::create_dir_all(&root)?;
        let mut manifest = Manifest {
            schema_version: 1,
            document: doc.clone(),
            parts: BTreeMap::new(),
            assemblies: BTreeMap::new(),
        };
        manifest.document.parts.clear();
        manifest.document.assemblies.clear();
        let mut files = vec![];
        for (id, p) in &doc.parts {
            let name = format!("parts/{id}.json");
            manifest.parts.insert(id.clone(), name.clone());
            files.push((name, serde_json::to_vec_pretty(p)?));
        }
        for (id, a) in &doc.assemblies {
            let name = format!("assemblies/{id}.json");
            manifest.assemblies.insert(id.clone(), name.clone());
            files.push((name, serde_json::to_vec_pretty(a)?));
        }
        let destination = manifest_path(path);
        let manifest_name = destination
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| CadError::Invalid("Invalid project filename".into()))?
            .to_string();
        files.push((manifest_name, serde_json::to_vec_pretty(&manifest)?));
        files.retain(|(name, data)| {
            std::fs::read(root.join(name)).ok().as_deref() != Some(data.as_slice())
        });
        if files.is_empty() {
            return Ok(());
        }
        let tx = crate::id();
        let mut journal = vec![];
        for (i, (name, data)) in files.iter().enumerate() {
            let staged = format!(".tcad-recovery/{tx}/{i}.json");
            atomic_write(&root.join(&staged), data)?;
            journal.push((staged, name.clone()));
        }
        atomic_write(
            &root.join(".tcad-recovery/pending.json"),
            &serde_json::to_vec(&journal)?,
        )?;
        for (from, to) in &journal {
            let to = safe_join(&root, to)?;
            if let Some(p) = to.parent() {
                fs::create_dir_all(p)?;
            }
            fs::rename(safe_join(&root, from)?, &to)?;
            #[cfg(unix)]
            if let Some(parent) = to.parent() {
                fs::File::open(parent)?.sync_all()?;
            }
        }
        fs::remove_file(root.join(".tcad-recovery/pending.json"))?;
        let _ = fs::remove_dir_all(root.join(format!(".tcad-recovery/{tx}")));
        Ok(())
    }
}
pub fn import_resource(doc: &mut Document, root: &Path, source: &Path) -> Result<Id> {
    let data = fs::read(source)?;
    import_resource_bytes(doc, root, source, &data)
}
/// Pin the exact bytes supplied by a caller, without reopening the source file.
/// The source path supplies only the portable resource kind; bounded readers and
/// format validators can retain the same buffer throughout validation and import.
pub fn import_resource_bytes(
    doc: &mut Document,
    root: &Path,
    source: &Path,
    data: &[u8],
) -> Result<Id> {
    use sha2::{Digest, Sha256};
    let hash = format!("{:x}", Sha256::digest(data));
    let extension = source
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("bin")
        .to_ascii_lowercase();
    let path = format!("imports/{hash}.{extension}");
    atomic_write(&root.join(&path), data)?;
    let id = crate::id();
    doc.resources.insert(
        id.clone(),
        crate::Resource {
            id: id.clone(),
            path,
            sha256: hash,
            kind: extension,
        },
    );
    Ok(id)
}
/// Resolve immutable resources from any source root, retaining the expected bytes and portable paths.
pub fn copy_resources(doc: &Document, sources: &[PathBuf], destination: &Path) -> Result<()> {
    use sha2::{Digest, Sha256};
    for resource in doc.resources.values() {
        let target = safe_join(destination, &resource.path)?;
        let matches = |bytes: &[u8]| format!("{:x}", Sha256::digest(bytes)) == resource.sha256;
        if fs::read(&target).ok().is_some_and(|bytes| matches(&bytes)) {
            continue;
        }
        let bytes = sources
            .iter()
            .filter_map(|root| safe_join(root, &resource.path).ok())
            .filter_map(|path| fs::read(path).ok())
            .find(|bytes| matches(bytes))
            .ok_or_else(|| {
                CadError::Invalid(format!(
                    "The exact resource {} is unavailable in the source projects",
                    resource.path
                ))
            })?;
        atomic_write(&target, &bytes)?;
    }
    Ok(())
}
