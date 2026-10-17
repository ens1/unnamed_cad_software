//! Reproducible publication artifacts, independent of a renderer or Git forge.
//! Generation uses a private source snapshot and exposes a package only after
//! every selected output succeeds. Git publication is a separate adapter.
use crate::{
    CadError, Document, Result,
    persistence::{FileStore, ProjectStore, atomic_write},
    protocol::{GeometryService, PROTOCOL_VERSION, RequestIdentity},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
};

pub const VERSION: u32 = 1;
pub const PREFERENCES_FILE: &str = "tcad-publish.json";
pub const MANIFEST_FILE: &str = "publication.json";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct OutputSelection {
    pub drawing_pdf: bool,
    pub drawing_dxf: bool,
    pub bom_csv: bool,
    pub step: bool,
    pub stl: bool,
    pub sketch_dxf: bool,
    pub flat_pattern_dxf: bool,
    pub cut_list_csv: bool,
}
impl Default for OutputSelection {
    fn default() -> Self {
        Self {
            drawing_pdf: true,
            drawing_dxf: true,
            bom_csv: true,
            step: true,
            stl: false,
            sketch_dxf: false,
            flat_pattern_dxf: false,
            cut_list_csv: false,
        }
    }
}
impl OutputSelection {
    pub fn count(&self) -> usize {
        [
            self.drawing_pdf,
            self.drawing_dxf,
            self.bom_csv,
            self.step,
            self.stl,
            self.sketch_dxf,
            self.flat_pattern_dxf,
            self.cut_list_csv,
        ]
        .into_iter()
        .filter(|value| *value)
        .count()
    }
}

/// Shared project choices. Authentication and local checkout locations stay in
/// machine preferences, never in the project or generated manufacturing files.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct Preferences {
    pub version: u32,
    pub repository_url: String,
    pub branch: String,
    pub project_folder: String,
    pub outputs: OutputSelection,
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            version: VERSION,
            repository_url: String::new(),
            branch: "main".into(),
            project_folder: String::new(),
            outputs: Default::default(),
        }
    }
}
impl Preferences {
    /// Empty destinations can be saved while configuring a project. Publishing
    /// additionally requires a repository and named branch.
    pub fn validate(&self) -> Result<()> {
        if self.version != VERSION {
            return Err(invalid(
                "Unsupported project publication preferences version",
            ));
        }
        if !self.repository_url.is_empty() {
            crate::git_connections::RepositoryAddress::parse(&self.repository_url)?;
        }
        if !self.branch.is_empty() {
            crate::repository_models::branch_name(&self.branch)?;
        }
        if !self.project_folder.is_empty() {
            crate::repository_models::relative_path(&self.project_folder)?;
        }
        Ok(())
    }
    pub fn validate_publish(&self) -> Result<()> {
        self.validate()?;
        if self.repository_url.trim().is_empty() || self.branch.trim().is_empty() {
            return Err(invalid("Choose the publication repository and branch"));
        }
        if self.outputs.count() == 0 {
            return Err(invalid("Choose at least one publication output"));
        }
        Ok(())
    }
}

fn invalid(message: impl Into<String>) -> CadError {
    CadError::Invalid(message.into())
}

pub fn read_preferences(project_root: &Path) -> Result<Preferences> {
    let path = project_root.join(PREFERENCES_FILE);
    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Preferences::default());
        }
        Err(error) => return Err(error.into()),
    };
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > 64 * 1024 {
        return Err(invalid(
            "Project Publish choices must be a regular JSON file under 64 KiB",
        ));
    }
    let preferences: Preferences = serde_json::from_slice(&fs::read(path)?)?;
    preferences.validate()?;
    Ok(preferences)
}

pub fn save_preferences(project_root: &Path, preferences: &Preferences) -> Result<()> {
    preferences.validate()?;
    let path = project_root.join(PREFERENCES_FILE);
    if fs::symlink_metadata(&path).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        return Err(invalid("Publish choices cannot replace a symbolic link"));
    }
    atomic_write(&path, &serde_json::to_vec_pretty(preferences)?)
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ArtifactFile {
    /// Portable path relative to the generated package root.
    pub path: String,
    pub sha256: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DrawingSource {
    NotSelected,
    SavedSheets,
    /// Existing drawing export behavior: A4 front, top, right and isometric
    /// projections of the root model, without invented dimensions or tolerances.
    AutomaticProjection,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Manifest {
    pub version: u32,
    pub document_id: crate::Id,
    pub name: String,
    pub source_sha256: String,
    pub revision: u64,
    pub application_version: String,
    pub backend: String,
    pub accuracy: crate::tolerance::Accuracy,
    pub outputs: OutputSelection,
    pub drawings: DrawingSource,
    pub saved_drawing_sheets: usize,
    #[serde(default)]
    pub readme_preview: bool,
    /// Fingerprints of generated files; the manifest cannot hash itself.
    pub files: Vec<ArtifactFile>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GeneratedPackage {
    pub version: u32,
    pub root: PathBuf,
    pub source_manifest: PathBuf,
    pub source_sha256: String,
    pub manifest: Manifest,
    /// Explicit artifacts to copy alongside source, including publication.json.
    pub files: Vec<ArtifactFile>,
}

/// Freeze source and resources before evaluating. Only a complete package is
/// moved to `destination`; failed generation never exposes partial publish data.
pub fn generate(
    service: &mut impl GeometryService,
    identity: &RequestIdentity,
    document: &Document,
    project_root: &Path,
    destination: &Path,
    preferences: &Preferences,
    display_tolerance_mm: f64,
) -> Result<GeneratedPackage> {
    preferences.validate_publish()?;
    document.validate()?;
    crate::tolerance::validate_display(display_tolerance_mm)?;
    if identity.protocol != PROTOCOL_VERSION
        || identity.backend != service.fingerprint()
        || identity.source != document.fingerprint()
        || identity.base_revision != document.revision
        || identity.draft.is_some()
    {
        return Err(invalid(
            "Publish requires the exact committed model snapshot",
        ));
    }
    if destination.exists() || fs::symlink_metadata(destination).is_ok() {
        return Err(invalid(
            "Choose a new folder for generated publication files",
        ));
    }
    let parent = destination.parent().unwrap_or(Path::new("."));
    fs::create_dir_all(parent)?;
    let staging = parent.join(format!(".tcad-publish-{}", crate::id()));
    fs::create_dir(&staging)?;
    let result = generate_staged(
        service,
        identity,
        document,
        project_root,
        &staging,
        preferences,
        display_tolerance_mm,
    )
    .and_then(|(manifest, files)| {
        fs::rename(&staging, destination)?;
        Ok(GeneratedPackage {
            version: VERSION,
            root: destination.to_path_buf(),
            source_manifest: destination.join("source/project.json"),
            source_sha256: document.fingerprint(),
            manifest,
            files,
        })
    });
    if result.is_err() {
        let _ = fs::remove_dir_all(&staging);
    }
    result
}

fn generate_staged(
    service: &mut impl GeometryService,
    identity: &RequestIdentity,
    document: &Document,
    project_root: &Path,
    staging: &Path,
    preferences: &Preferences,
    display_tolerance_mm: f64,
) -> Result<(Manifest, Vec<ArtifactFile>)> {
    let source = staging.join("source");
    FileStore.save(&source, document)?;
    crate::persistence::copy_resources(document, &[project_root.to_path_buf()], &source)?;
    crate::repository_models::copy_shared_presentation(project_root, &source)?;
    save_preferences(&source, preferences)?;
    let snapshot = crate::repository_models::inspect_model(
        &source.join("project.json"),
        &crate::forge::CancelToken::default(),
    )?;
    if snapshot.fingerprint != identity.source {
        return Err(invalid(
            "The frozen publication source does not match the requested model",
        ));
    }
    // Evaluate the frozen resource bytes, not files that can change while the
    // worker is running. Native geometry is retained by the persistent adapter.
    let evaluated = service.evaluate(identity.clone(), document, &source, display_tolerance_mm)?;
    if !evaluated.valid || evaluated.identity.source != identity.source {
        let details = evaluated
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.severity == "error")
            .map(|diagnostic| diagnostic.message.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        return Err(invalid(format!(
            "Publish requires a valid current model. {details}"
        )));
    }
    let outputs = &preferences.outputs;
    fs::create_dir(staging.join("outputs"))?;
    let mut files = Vec::new();
    for (selected, name, format) in [
        (outputs.drawing_pdf, "drawing.pdf", "pdf"),
        (outputs.drawing_dxf, "drawing.dxf", "drawing-dxf"),
        (outputs.bom_csv, "bom.csv", "bom"),
        (outputs.step, "model.step", "step"),
        (outputs.stl, "model.stl", "stl"),
        (outputs.sketch_dxf, "sketches.dxf", "dxf"),
        (
            outputs.flat_pattern_dxf,
            "flat-pattern.dxf",
            "flat-pattern-dxf",
        ),
        (outputs.cut_list_csv, "cutlist.csv", "cutlist"),
    ] {
        if !selected {
            continue;
        }
        let relative = format!("outputs/{name}");
        let path = staging.join(&relative);
        let export = match format {
            "pdf" => crate::manufacturing::drawing_pdf(document, &evaluated, &path),
            "drawing-dxf" => crate::drawing::dxf(document, &evaluated)
                .and_then(|data| atomic_write(&path, data.as_bytes())),
            "bom" => crate::production::bom_csv(document, &evaluated)
                .and_then(|data| atomic_write(&path, data.as_bytes())),
            "stl" => crate::manufacturing::stl(&evaluated, &path),
            "flat-pattern-dxf" => crate::sheet_metal::export_sheets(document, &evaluated)
                .and_then(|sheets| {
                    if sheets.is_empty() {
                        Err(invalid("No sheet-metal flat patterns are available; clear Flat-pattern DXF or add sheet metal"))
                    } else {
                        crate::sheet_metal::dxf(&sheets)
                    }
                })
                .and_then(|data| atomic_write(&path, data.as_bytes())),
            "cutlist" => crate::manufacturing::cut_list_evaluated(document, &evaluated)
                .and_then(|rows| {
                    if rows.is_empty() {
                        Err(invalid("No structural cut-list members are available; clear Cut-list CSV or add structural members"))
                    } else {
                        crate::production::cut_list_csv(document, &evaluated)
                    }
                })
                .and_then(|data| atomic_write(&path, data.as_bytes())),
            _ => service.export(document, &source, &path, format, display_tolerance_mm),
        };
        export.map_err(|error| invalid(format!("Could not generate {name}: {error}")))?;
        files.push(ArtifactFile {
            path: relative,
            sha256: format!("{:x}", Sha256::digest(fs::read(&path)?)),
        });
    }
    let readme_preview =
        refresh_readme_preview(project_root, staging, document, &evaluated, &mut files)?;
    let manifest = Manifest {
        version: VERSION,
        document_id: document.id.clone(),
        name: document.name.clone(),
        source_sha256: document.fingerprint(),
        revision: document.revision,
        application_version: env!("CARGO_PKG_VERSION").into(),
        backend: service.fingerprint().into(),
        accuracy: evaluated.accuracy,
        outputs: outputs.clone(),
        drawings: if !outputs.drawing_pdf && !outputs.drawing_dxf {
            DrawingSource::NotSelected
        } else if document.drawings.is_empty() {
            DrawingSource::AutomaticProjection
        } else {
            DrawingSource::SavedSheets
        },
        saved_drawing_sheets: document.drawings.len(),
        readme_preview,
        files: files.clone(),
    };
    let bytes = serde_json::to_vec_pretty(&manifest)?;
    atomic_write(&staging.join(MANIFEST_FILE), &bytes)?;
    files.push(ArtifactFile {
        path: MANIFEST_FILE.into(),
        sha256: format!("{:x}", Sha256::digest(&bytes)),
    });
    Ok((manifest, files))
}

/// An existing preview metadata file is the user's opt-in. Regenerate it from
/// this publication's evaluation; never ship an older picture with new source.
fn refresh_readme_preview(
    project_root: &Path,
    staging: &Path,
    document: &Document,
    evaluated: &crate::protocol::Evaluation,
    files: &mut Vec<ArtifactFile>,
) -> Result<bool> {
    let Some(metadata) = optional_file(project_root, "previews/model-preview.json", 1024 * 1024)?
    else {
        return Ok(false);
    };
    let metadata: crate::readme_preview::Metadata = serde_json::from_slice(&metadata)?;
    if metadata.version != crate::readme_preview::CONTRACT_VERSION {
        return Err(invalid(
            "The project's README preview version is unsupported; update its preview before publishing",
        ));
    }
    let existing = optional_file(project_root, "README.md", 4 * 1024 * 1024)?
        .map(String::from_utf8)
        .transpose()
        .map_err(|_| invalid("The project README must be UTF-8 to refresh its model preview"))?
        .unwrap_or_else(|| format!("# {}\n", document.name.replace(['\r', '\n'], " ")));
    let preview = crate::readme_preview::build(document, evaluated)?;
    let markdown = crate::readme_preview::update_markdown(&existing, &preview)?;
    for (path, bytes) in [
        ("README.md", markdown.into_bytes()),
        ("previews/model.stl", preview.stl),
        ("previews/model.png", preview.png),
        (
            "previews/model-preview.json",
            serde_json::to_vec_pretty(&preview.metadata)?,
        ),
    ] {
        atomic_write(&staging.join(path), &bytes)?;
        files.push(ArtifactFile {
            path: path.into(),
            sha256: format!("{:x}", Sha256::digest(&bytes)),
        });
    }
    Ok(true)
}

fn optional_file(root: &Path, relative: &str, limit: u64) -> Result<Option<Vec<u8>>> {
    let mut path = root.to_path_buf();
    for component in Path::new(relative).components() {
        path.push(component);
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        if metadata.file_type().is_symlink() {
            return Err(invalid("README preview paths must not be symbolic links"));
        }
    }
    let metadata = fs::metadata(&path)?;
    if !metadata.is_file() || metadata.len() > limit {
        return Err(invalid(format!(
            "README preview input exceeds its file limit: {relative}"
        )));
    }
    let bytes = fs::read(path)?;
    if bytes.len() as u64 > limit {
        return Err(invalid(format!(
            "README preview input grew beyond its file limit: {relative}"
        )));
    }
    Ok(Some(bytes))
}
