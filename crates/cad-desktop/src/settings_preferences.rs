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
    pub default_model: ModelSettings,
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
            default_model: ModelSettings::default(),
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
        if !cad_core::Document::blank_part("Part1")
            .materials
            .contains_key(&self.default_model.default_new_part_material)
        {
            return Err(CadError::Invalid("Application defaults require a built-in engineering material; project materials stay in their document".into()));
        }
        if !appearance::default_finishes().contains_key(&self.default_model.default_new_part_finish)
        {
            return Err(CadError::Invalid(
                "Application defaults require a built-in visual finish; project finishes stay in their document"
                    .into(),
            ));
        }
        Ok(())
    }
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
                .and_then(|preferences| preferences.validate().map(|()| preferences))
            {
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
