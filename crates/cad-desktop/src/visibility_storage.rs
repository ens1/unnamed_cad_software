//! Project optical preferences. These never restore a camera, theme, or CAD source.
use cad_core::{
    CadError, Result,
    persistence::{atomic_write, project_root},
    presentation::{OverlayVisibility, PresentationState, ViewportVisibility},
};
use serde::{Deserialize, Serialize};
use std::{fs::File, io::Read, path::Path};

const CONTRACT: &str = "tcad.viewport-visibility";
const VERSION: u32 = 1;
const LIMIT: usize = 32 * 1024;
const LEGACY_LIMIT: usize = 2 * 1024 * 1024;

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct OpticalState {
    visibility: ViewportVisibility,
    sketch_annotation_visibility: [bool; 2],
    sketch_overlay_mode: OverlayVisibility,
    reference_overlay_mode: OverlayVisibility,
    sketch_overlay_xray: bool,
    reference_overlay_xray: bool,
}
impl Default for OpticalState {
    fn default() -> Self {
        Self::capture(&PresentationState::default())
    }
}
impl OpticalState {
    pub fn capture(view: &PresentationState) -> Self {
        Self {
            visibility: view.visibility.clone(),
            sketch_annotation_visibility: view.sketch_annotation_visibility,
            sketch_overlay_mode: view.sketch_overlay_mode.clone(),
            reference_overlay_mode: view.reference_overlay_mode.clone(),
            sketch_overlay_xray: view.sketch_overlay_xray,
            reference_overlay_xray: view.reference_overlay_xray,
        }
    }
    pub fn apply(self, view: &mut PresentationState) {
        view.visibility = self.visibility;
        view.sketch_annotation_visibility = self.sketch_annotation_visibility;
        view.sketch_overlay_mode = self.sketch_overlay_mode;
        view.reference_overlay_mode = self.reference_overlay_mode;
        view.sketch_overlay_xray = self.sketch_overlay_xray;
        view.reference_overlay_xray = self.reference_overlay_xray;
    }
}

#[derive(Serialize, Deserialize)]
struct SavedVisibility {
    contract: String,
    version: u32,
    document: cad_core::Id,
    #[serde(flatten)]
    optical: OpticalState,
}

fn read_bounded(path: &Path, limit: usize) -> Result<Option<Vec<u8>>> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let mut bytes = Vec::new();
    file.take(limit as u64 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        return Err(CadError::Invalid(format!(
            "Visibility settings exceed the {} KiB size limit",
            limit / 1024
        )));
    }
    Ok(Some(bytes))
}

/// Missing preferences preserve defaults (or the legacy presentation already loaded).
/// Invalid files report a notice and leave the supplied optical state untouched.
pub fn restore(project: &Path, document: &str, view: &mut PresentationState) -> Option<String> {
    let directory = project_root(project).join("ui");
    let path = directory.join("visibility.json");
    let result = (|| -> Result<Option<OpticalState>> {
        if let Some(bytes) = read_bounded(&path, LIMIT)? {
            let saved: SavedVisibility = serde_json::from_slice(&bytes)?;
            if saved.contract != CONTRACT || saved.version != VERSION {
                return Err(CadError::Unsupported(
                    "Unsupported viewport visibility format".into(),
                ));
            }
            if saved.document != document {
                return Err(CadError::Invalid(
                    "Viewport visibility belongs to a different project".into(),
                ));
            }
            return Ok(Some(saved.optical));
        }
        // Old project presentation packages stored these same fields together
        // with camera and layout state. Import only the optical subset on Open.
        read_bounded(&directory.join("workspace.json"), LEGACY_LIMIT)?
            .map(|bytes| serde_json::from_slice(&bytes).map_err(CadError::from))
            .transpose()
    })();
    match result {
        Ok(Some(optical)) => {
            optical.apply(view);
            None
        }
        Ok(None) => None,
        Err(error) => Some(format!(
            "Could not restore viewport visibility: {error}. Existing display choices were kept; the model opened normally."
        )),
    }
}

pub fn save(project: &Path, document: &str, view: &PresentationState) -> Result<()> {
    let saved = SavedVisibility {
        contract: CONTRACT.into(),
        version: VERSION,
        document: document.into(),
        optical: OpticalState::capture(view),
    };
    let bytes = serde_json::to_vec_pretty(&saved)?;
    if bytes.len() > LIMIT {
        return Err(CadError::Invalid(
            "Viewport visibility settings exceed the size limit".into(),
        ));
    }
    atomic_write(&project_root(project).join("ui/visibility.json"), &bytes)
}
