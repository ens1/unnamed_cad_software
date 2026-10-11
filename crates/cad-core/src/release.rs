use crate::{
    CadError, Document, Result,
    persistence::{FileStore, ProjectStore, atomic_write},
    protocol::{GeometryService, PROTOCOL_VERSION, RequestIdentity},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, path::Path};
#[derive(Serialize, Deserialize)]
pub struct ReleaseManifest {
    pub schema_version: u32,
    pub name: String,
    pub source_sha256: String,
    pub revision: u64,
    pub backend: String,
    pub tolerance_mm: f64,
    pub artifacts: BTreeMap<String, String>,
}
/// Immutable local release folder: source, exchange files and a hash manifest.
pub fn create(
    service: &mut impl GeometryService,
    doc: &Document,
    root: &Path,
    destination: &Path,
    tolerance: f64,
) -> Result<ReleaseManifest> {
    if destination.exists() {
        return Err(CadError::Invalid(
            "A release destination must not already exist".into(),
        ));
    }
    let parent = destination.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(parent)?;
    let staging = parent.join(format!(".release-{}", crate::id()));
    std::fs::create_dir(&staging)?;
    let result = (|| {
        let evaluated = service.evaluate(
            RequestIdentity {
                protocol: PROTOCOL_VERSION,
                generation: 0,
                base_revision: doc.revision,
                draft: None,
                edit: 0,
                source: doc.fingerprint(),
                backend: service.fingerprint().into(),
            },
            doc,
            root,
            tolerance,
        )?;
        if !evaluated.valid {
            return Err(CadError::Invalid(
                "A release requires a valid current evaluation".into(),
            ));
        }
        FileStore.save(&staging.join("source"), doc)?;
        for resource in doc.resources.values() {
            let from = crate::persistence::safe_join(root, &resource.path)?;
            let to = crate::persistence::safe_join(&staging.join("source"), &resource.path)?;
            atomic_write(&to, &std::fs::read(from)?)?;
        }
        let mut artifacts = BTreeMap::new();
        let mut formats = vec![
            ("assembly.step", "step"),
            ("assembly.stl", "stl"),
            ("drawing.pdf", "pdf"),
            ("drawing.dxf", "drawing-dxf"),
            ("profiles.dxf", "dxf"),
            ("bom.csv", "csv"),
        ];
        if !crate::sheet_metal::export_sheets(doc, &evaluated)?.is_empty() {
            formats.push(("flat-pattern.dxf", "flat-pattern-dxf"));
        }
        if !crate::manufacturing::cut_list_evaluated(doc, &evaluated)?.is_empty() {
            formats.push(("cutlist.csv", "cutlist"));
        }
        for (name, format) in formats {
            let path = staging.join(name);
            service.export(doc, root, &path, format, tolerance)?;
            artifacts.insert(
                name.into(),
                format!("{:x}", Sha256::digest(std::fs::read(path)?)),
            );
        }
        let manifest = ReleaseManifest {
            schema_version: 1,
            name: doc.name.clone(),
            source_sha256: doc.fingerprint(),
            revision: doc.revision,
            backend: service.fingerprint().into(),
            tolerance_mm: tolerance,
            artifacts,
        };
        atomic_write(
            &staging.join("release.json"),
            &serde_json::to_vec_pretty(&manifest)?,
        )?;
        std::fs::rename(&staging, destination)?;
        Ok(manifest)
    })();
    if result.is_err() {
        let _ = std::fs::remove_dir_all(staging);
    }
    result
}
