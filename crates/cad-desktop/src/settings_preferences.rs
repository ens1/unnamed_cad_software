//! Personal application preferences. Document settings remain in CAD source.
//!
//! This adapter owns local file persistence; the settings dialog stages a copy
//! and only writes it after Apply. The legacy navigation file is read once when
//! no unified preference file exists, then the unified file becomes authoritative.
use cad_core::{
    CadError, Result, appearance, navigation, persistence::atomic_write, settings::ModelSettings,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Read,
    path::{Path, PathBuf},
};

pub const CONTRACT_VERSION: u32 = 1;
pub const MIN_UI_SCALE: f32 = 0.75;
pub const MAX_UI_SCALE: f32 = 1.5;
pub const MIN_RECOVERY_INTERVAL_SECONDS: u64 = 1;
pub const MAX_RECOVERY_INTERVAL_SECONDS: u64 = 3600;
const MAX_PREFERENCE_BYTES: u64 = 256 * 1024;
const MAX_LEGACY_NAVIGATION_BYTES: u64 = 32 * 1024;

/// Source-only finish snapshot. Texture bytes live in the personal immutable
/// resource store and are copied into each new document's unsaved root.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DefaultFinish {
    pub version: u32,
    pub finish: appearance::Finish,
    pub resources: BTreeMap<cad_core::Id, cad_core::Resource>,
}
impl DefaultFinish {
    fn document(&self) -> cad_core::Document {
        let mut document = cad_core::Document::blank_part("Personal finish validation");
        document.resources = self.resources.clone();
        document
            .finishes
            .insert(self.finish.id.clone(), self.finish.clone());
        document
    }
    fn validate(&self) -> Result<()> {
        let used = self
            .finish
            .textures
            .resources()
            .cloned()
            .collect::<BTreeSet<_>>();
        if self.version != 1
            || used.len() != self.resources.len()
            || self.resources.len() > 6
            || self
                .resources
                .iter()
                .any(|(id, resource)| id != &resource.id || !used.contains(id))
        {
            return Err(CadError::Invalid("The personal finish requires version 1 and exactly its referenced texture resources".into()));
        }
        self.document().validate()
    }
    fn package(&self) -> Result<cad_core::finish_library::Package> {
        self.validate()?;
        let package = cad_core::finish_library::export(
            &self.document(),
            &finish_root(),
            std::slice::from_ref(&self.finish.id),
        )?;
        validate_default_textures(&package)?;
        Ok(package)
    }
}

/// Defaults affect newly created models; they never rewrite an opened document.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Preferences {
    pub version: u32,
    pub ui_scale: f32,
    pub navigation: navigation::Preferences,
    /// View-only topology outlines, independent of saved engineering definitions.
    pub show_part_edges: bool,
    /// Show nearby circular-edge centers and analytic axes without selecting them.
    pub hover_centerlines: bool,
    /// Show nearby straight-edge midpoint guides without selecting them.
    pub hover_midpoints: bool,
    pub watch_presentation_files: bool,
    pub recovery_enabled: bool,
    pub recovery_interval_seconds: u64,
    /// Generate a saved-model README preview before a TCAD Git commit.
    pub readme_preview_on_commit: bool,
    pub default_model: ModelSettings,
    /// Portable engineering snapshot copied into new documents, outside project identity scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_material: Option<cad_core::Material>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_finish: Option<DefaultFinish>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub library_favorites: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub library_recent: Vec<String>,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            version: CONTRACT_VERSION,
            ui_scale: 1.,
            navigation: navigation::Preferences::default(),
            show_part_edges: true,
            hover_centerlines: true,
            hover_midpoints: true,
            watch_presentation_files: true,
            recovery_enabled: true,
            recovery_interval_seconds: 2,
            readme_preview_on_commit: false,
            default_model: ModelSettings::default(),
            default_material: None,
            default_finish: None,
            library_favorites: vec![],
            library_recent: vec![],
        }
    }
}

impl Preferences {
    pub fn validate(&self) -> Result<()> {
        if self.version != CONTRACT_VERSION {
            return Err(CadError::Invalid(format!(
                "Unsupported application preference version {}",
                self.version
            )));
        }
        if !self.ui_scale.is_finite() || !(MIN_UI_SCALE..=MAX_UI_SCALE).contains(&self.ui_scale) {
            return Err(CadError::Invalid(
                "Interface scale must be between 75% and 150%".into(),
            ));
        }
        if !(MIN_RECOVERY_INTERVAL_SECONDS..=MAX_RECOVERY_INTERVAL_SECONDS)
            .contains(&self.recovery_interval_seconds)
        {
            return Err(CadError::Invalid(
                "Recovery interval must be between 1 and 3600 seconds".into(),
            ));
        }
        self.navigation.validate()?;
        for (label, entries, limit) in [
            ("favorites", &self.library_favorites, 512),
            ("recent", &self.library_recent, 64),
        ] {
            if entries.len() > limit
                || entries.iter().any(|id| {
                    id.is_empty()
                        || id.len() > 256
                        || !(id.starts_with("material:") || id.starts_with("finish:"))
                })
            {
                return Err(CadError::Invalid(format!(
                    "Invalid material/finish library {label}"
                )));
            }
        }
        self.default_model.validate_values()?;
        if let Some(material) = &self.default_material {
            cad_core::materials::validate(material)?;
            if material.id != self.default_model.default_new_part_material {
                return Err(CadError::Invalid(
                    "The portable default material must match the new-part preference".into(),
                ));
            }
        } else if !cad_core::Document::blank_part("Part1")
            .materials
            .contains_key(&self.default_model.default_new_part_material)
        {
            return Err(CadError::Invalid(
                "A custom new-part material requires its portable engineering snapshot".into(),
            ));
        }
        if let Some(snapshot) = &self.default_finish {
            snapshot.validate()?;
            if snapshot.finish.id != self.default_model.default_new_part_finish {
                return Err(CadError::Invalid(
                    "The portable finish must match the new-part preference".into(),
                ));
            }
        } else if !appearance::new_document_finishes()
            .contains_key(&self.default_model.default_new_part_finish)
        {
            return Err(CadError::Invalid(
                "A custom new-part finish requires its portable visual snapshot".into(),
            ));
        }
        Ok(())
    }
    pub fn validate_default_assets(&self) -> Result<()> {
        if let Some(snapshot) = &self.default_finish {
            snapshot.package()?;
        }
        Ok(())
    }
    pub fn apply_new_document_defaults(&self, document: &mut cad_core::Document) -> Result<()> {
        self.validate()?;
        let mut staged = document.clone();
        if let Some(material) = &self.default_material {
            staged
                .materials
                .insert(material.id.clone(), material.clone());
        }
        let installed_finish =
            if let Some(snapshot) = &self.default_finish {
                let package = snapshot.package()?;
                let root = crate::recovery_directory()
                    .join("unsaved")
                    .join(&document.id);
                let installed = cad_core::finish_library::install(&mut staged, &root, &package)?;
                Some(installed.first().cloned().ok_or_else(|| {
                    CadError::Invalid("The default finish was not installed".into())
                })?)
            } else {
                None
            };
        staged.settings = self.default_model.clone();
        if let Some(finish) = installed_finish {
            staged.settings.default_new_part_finish = finish;
        }
        for part in staged.parts.values_mut() {
            part.material = staged.settings.default_new_part_material.clone();
            part.finish = staged.settings.default_new_part_finish.clone();
        }
        staged.validate()?;
        *document = staged;
        Ok(())
    }
}

fn finish_root() -> PathBuf {
    personal_directory().join("library-defaults")
}

fn validate_default_textures(package: &cad_core::finish_library::Package) -> Result<()> {
    for texture in &package.textures {
        crate::texture_assets::validate_default_texture(&texture.bytes, &texture.resource.kind)?;
    }
    Ok(())
}

/// Pin exact source bytes before publishing the preference. Cancel may retain
/// unreferenced immutable bytes, but cannot publish the staged finish default.
pub fn capture_default_finish(
    document: &cad_core::Document,
    root: &Path,
    finish: &str,
) -> Result<DefaultFinish> {
    let package = cad_core::finish_library::export(document, root, &[finish.into()])?;
    capture_default_package(package)
}

pub fn capture_default_package(
    mut package: cad_core::finish_library::Package,
) -> Result<DefaultFinish> {
    validate_default_textures(&package)?;
    let source = package
        .finishes
        .first_mut()
        .ok_or_else(|| CadError::Invalid("Choose an existing finish".into()))?;
    source.id = cad_core::id();
    let mut personal = cad_core::Document::blank_part("Personal finish store");
    let ids = cad_core::finish_library::install(&mut personal, &finish_root(), &package)?;
    let finish = personal
        .finishes
        .get(&ids[0])
        .cloned()
        .ok_or_else(|| CadError::Invalid("The personal finish was not installed".into()))?;
    let resources = finish
        .textures
        .resources()
        .map(|id| {
            personal
                .resources
                .get(id)
                .cloned()
                .map(|resource| (id.clone(), resource))
                .ok_or_else(|| CadError::Invalid("The personal finish texture is missing".into()))
        })
        .collect::<Result<BTreeMap<_, _>>>()?;
    let snapshot = DefaultFinish {
        version: 1,
        finish,
        resources,
    };
    snapshot.validate()?;
    Ok(snapshot)
}

pub struct Loaded {
    pub preferences: Preferences,
    pub diagnostic: Option<String>,
    pub migrated_navigation: bool,
}

fn personal_directory() -> PathBuf {
    crate::recovery_directory()
        .parent()
        .unwrap_or(Path::new("."))
        .to_owned()
}

pub fn path() -> PathBuf {
    personal_directory().join("settings.json")
}

fn legacy_navigation_path() -> PathBuf {
    personal_directory().join("navigation.json")
}

fn read_bounded(path: &Path, limit: u64) -> std::io::Result<Vec<u8>> {
    let file = std::fs::File::open(path)?;
    if file.metadata()?.len() > limit {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("{} exceeds {} KiB", path.display(), limit / 1024),
        ));
    }
    let mut bytes = Vec::new();
    file.take(limit + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("{} exceeds {} KiB", path.display(), limit / 1024),
        ));
    }
    Ok(bytes)
}

fn restore_defaults(error: impl std::fmt::Display) -> Loaded {
    Loaded {
        preferences: Preferences::default(),
        diagnostic: Some(format!(
            "Application preferences could not be loaded; defaults restored: {error}"
        )),
        migrated_navigation: false,
    }
}

/// A reset bypasses saved preferences without overwriting the user's file.
/// Invalid or newer files are retained so the user can repair or reopen them.
pub fn load(reset: bool) -> Loaded {
    if reset {
        return Loaded {
            preferences: Preferences::default(),
            diagnostic: None,
            migrated_navigation: false,
        };
    }
    match read_bounded(&path(), MAX_PREFERENCE_BYTES) {
        Ok(bytes) => {
            return match serde_json::from_slice::<Preferences>(&bytes)
                .map_err(CadError::from)
                .and_then(|mut preferences| {
                    // Preserve explicitly selected defaults from before optional libraries.
                    let material = &preferences.default_model.default_new_part_material;
                    if preferences.default_material.is_none() && material != "aluminum" {
                        preferences.default_material =
                            cad_core::Document::legacy_materials().remove(material);
                    }
                    let finish = &preferences.default_model.default_new_part_finish;
                    if preferences.default_finish.is_none()
                        && !appearance::new_document_finishes().contains_key(finish)
                        && let Some(finish) = appearance::default_finishes().remove(finish)
                    {
                        preferences.default_finish = Some(DefaultFinish {
                            version: 1,
                            finish,
                            resources: BTreeMap::new(),
                        });
                    }
                    preferences.validate()?;
                    preferences.validate_default_assets()?;
                    Ok(preferences)
                }) {
                Ok(preferences) => Loaded {
                    preferences,
                    diagnostic: None,
                    migrated_navigation: false,
                },
                Err(error) => restore_defaults(error),
            };
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return restore_defaults(error),
    }
    let mut preferences = Preferences::default();
    match read_bounded(&legacy_navigation_path(), MAX_LEGACY_NAVIGATION_BYTES) {
        Ok(bytes) => match serde_json::from_slice::<navigation::Preferences>(&bytes)
            .map_err(CadError::from)
            .and_then(|navigation| navigation.validate().map(|()| navigation))
        {
            Ok(navigation) => {
                preferences.navigation = navigation;
                let diagnostic = save(&preferences).err().map(|error| {
                    format!(
                        "Navigation preferences were loaded, but the application preference migration could not be saved: {error}"
                    )
                });
                Loaded {
                    preferences,
                    diagnostic,
                    migrated_navigation: true,
                }
            }
            Err(error) => restore_defaults(error),
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Loaded {
            preferences,
            diagnostic: None,
            migrated_navigation: false,
        },
        Err(error) => restore_defaults(error),
    }
}

/// Validation happens before the durable replacement of the personal file.
pub fn save(preferences: &Preferences) -> Result<()> {
    preferences.validate()?;
    let bytes = serde_json::to_vec_pretty(preferences)?;
    if bytes.len() as u64 > MAX_PREFERENCE_BYTES {
        return Err(CadError::Invalid(
            "Application preferences exceed the 256 KiB limit".into(),
        ));
    }
    atomic_write(&path(), &bytes)
}
