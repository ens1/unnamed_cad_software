//! Personal presentation choices contain a complete layout/style pair. They
//! remain separate from CAD source and can be previewed and cancelled together.
use crate::{Workspace, presentation_resources as resources};
use cad_core::{
    CadError, Result,
    persistence::atomic_write,
    presentation::{LayoutDocument, Theme, default_layout, default_theme},
};
use serde::{Deserialize, Serialize};
use std::{io::Read, path::PathBuf};

const MAX_CHOICES: usize = 256;
const MAX_RECORD_BYTES: u64 = 16 * 1024;

#[derive(Clone, Debug)]
pub struct Choice {
    pub id: String,
    pub name: String,
    pub description: String,
}

#[derive(Clone)]
struct Entry {
    choice: Choice,
    manifest: PathBuf,
    repository: Option<cad_core::content_repositories::ResolvedEntry>,
}

#[derive(Clone)]
pub struct State {
    pub selected: String,
    entries: Vec<Entry>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            selected: "classic".into(),
            entries: Vec::new(),
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    schema_version: u32,
    id: String,
    name: String,
    description: String,
}

/// Capture only presentation resources: model, draft, camera and selections
/// remain owned by their existing transactions.
pub struct Snapshot {
    resources: resources::State,
    layout: LayoutDocument,
    theme: Theme,
    layout_text: String,
    theme_text: String,
    active_layout: String,
}

#[derive(Clone)]
pub struct PendingSave(resources::PendingPersonal);

fn invalid(message: impl Into<String>) -> CadError {
    CadError::Invalid(message.into())
}

fn directory() -> PathBuf {
    resources::personal_directory().join("themes")
}

/// Inventory is refreshed on startup and import, avoiding filesystem work in
/// every settings frame. Packages are fully validated only when selected.
pub fn refresh(w: &mut Workspace) {
    let mut entries = Vec::new();
    let mut errors = Vec::new();
    let paths = match std::fs::read_dir(directory()) {
        Ok(paths) => Some(paths),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => {
            errors.push(format!("Could not read personal theme library: {error}"));
            None
        }
    };
    for item in paths.into_iter().flatten().take(MAX_CHOICES + 1) {
        if entries.len() >= MAX_CHOICES {
            errors.push(format!(
                "Personal theme library is limited to {MAX_CHOICES} packages"
            ));
            break;
        }
        let result = (|| -> Result<Option<Entry>> {
            let item = item?;
            if !item.file_type()?.is_dir() {
                return Ok(None);
            }
            let folder = item.path();
            let record_path = folder.join("library.json");
            let mut source = String::new();
            std::fs::File::open(&record_path)?
                .take(MAX_RECORD_BYTES + 1)
                .read_to_string(&mut source)?;
            if source.len() as u64 > MAX_RECORD_BYTES {
                return Err(invalid("Theme library record exceeds 16 KiB"));
            }
            let record: Record = serde_json::from_str(&source)?;
            let folder_name = folder
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or_default();
            if record.schema_version != 1
                || record.id != format!("imported.{folder_name}")
                || record.name.chars().count() > 160
                || record.description.chars().count() > 512
            {
                return Err(invalid(format!(
                    "Invalid theme library record: {}",
                    record_path.display()
                )));
            }
            let manifest = folder.join("presentation.json");
            if !manifest.is_file() {
                return Err(invalid(format!(
                    "Theme package is missing: {}",
                    manifest.display()
                )));
            }
            Ok(Some(Entry {
                choice: Choice {
                    id: record.id,
                    name: record.name,
                    description: record.description,
                },
                manifest,
                repository: None,
            }))
        })();
        match result {
            Ok(Some(entry)) => entries.push(entry),
            Ok(None) => {}
            Err(error) => errors.push(error.to_string()),
        }
    }
    for repository in
        cad_core::content_repositories::entries(cad_core::content_repositories::ContentKind::Themes)
    {
        if entries.len() >= MAX_CHOICES {
            errors.push(format!(
                "Theme library is limited to {MAX_CHOICES} packages"
            ));
            break;
        }
        entries.push(Entry { choice: Choice {
            id: format!("repository:{}:{}", repository.repository_id, repository.entry.id),
            name: format!("{} · {}", repository.entry.name, repository.repository_name),
            description: "Installed Git repository theme. Edit its layout and CSS files in the checkout; valid saves reload when presentation watching is enabled.".into(),
        }, manifest: repository.path.clone(), repository: Some(repository) });
    }
    entries.sort_by(|left, right| {
        left.choice
            .name
            .to_lowercase()
            .cmp(&right.choice.name.to_lowercase())
            .then_with(|| left.choice.id.cmp(&right.choice.id))
    });
    w.presentation_resources.library.entries = entries;
    if !errors.is_empty() {
        w.presentation_resources.diagnostic = Some(format!("Theme library: {}", errors.join("; ")));
    }
}

pub fn choices(w: &Workspace) -> Vec<Choice> {
    let mut choices = vec![
        Choice { id: "classic".into(), name: "Classic · Orange".into(), description: "Compact warm CAD workspace with a left feature tree, small rectangular controls and an orange accent.".into() },
    ];
    choices.extend(
        w.presentation_resources
            .library
            .entries
            .iter()
            .map(|entry| entry.choice.clone()),
    );
    let selected = selected(w);
    if !choices.iter().any(|choice| choice.id == selected) {
        let missing_pack = selected.starts_with("repository:") || selected == "nocturne";
        choices.push(Choice {
            id: selected,
            name: if missing_pack { "Saved theme · pack unavailable" } else { "Current / custom presentation" }.into(),
            description: if missing_pack { "Your saved theme remains usable without its optional repository. Reconnect the library to edit or update its source." } else { "The active project or custom layout and styling. Load a portable presentation package to add it to the library." }.into(),
        });
    }
    choices
}

pub(crate) fn repository_manifest(id: &str) -> Option<PathBuf> {
    cad_core::content_repositories::entries(cad_core::content_repositories::ContentKind::Themes)
        .into_iter()
        .find(|entry| format!("repository:{}:{}", entry.repository_id, entry.entry.id) == id)
        .map(|entry| entry.path)
}

/// A missing optional theme is advisory. Its already-applied layout and the
/// self-contained personal snapshot stay usable; no CAD draft is replaced.
pub fn fallback_notice(w: &Workspace) -> Option<cad_core::presentation::SemanticElement> {
    let selected = selected(w);
    let missing = (selected.starts_with("repository:") || selected == "nocturne")
        && !w
            .presentation_resources
            .library
            .entries
            .iter()
            .any(|entry| entry.choice.id == selected);
    let message = w.presentation_resources.library_notice.clone().or_else(|| missing.then(|| "The active theme's repository is not installed. Your current layout remains usable. Reconnect its library, or download another theme pack.".into()))?;
    let pack = cad_core::content_repositories::pack("themes").ok()?;
    Some(cad_core::presentation::SemanticElement {
        id: "workspace.presentation.library-notice".into(),
        role: "presentation-library-notice".into(),
        label: message.clone(),
        value: Some(message),
        state: "fallback".into(),
        commands: vec![
            "content-repositories.download-themes".into(),
            "content-repositories.show-themes".into(),
            "content-repositories".into(),
            "presentation.reset".into(),
        ],
        relationships: vec!["workspace.presentation.diagnostics".into()],
        data: serde_json::json!({"pack":"themes","selected_theme":selected,"repository_url":pack.clone_url,"web_url":pack.web_url,"fallback":"saved-presentation","blocks_modeling":false,"presentation_only":true}),
    })
}

pub fn selected(w: &Workspace) -> String {
    w.presentation_resources.library.selected.clone()
}

fn bundled(id: &str) -> Result<resources::Candidate> {
    let (layout, theme, active_layout) = match id {
        "classic" => (default_layout(), default_theme(), "instrument-rail"),
        _ => return Err(invalid(format!("Unknown bundled theme {id}"))),
    };
    Ok(resources::Candidate {
        layout_text: serde_json::to_string_pretty(&layout)?,
        theme_text: serde_json::to_string_pretty(&theme)?,
        layout,
        theme,
        active_layout: Some(active_layout.into()),
        layout_path: None,
        theme_path: None,
        library_id: Some(id.into()),
    })
}

pub fn select(w: &mut Workspace, id: &str) -> Result<()> {
    if selected(w) == id
        && (id == "custom"
            || (id.starts_with("repository:") || id == "nocturne")
                && !w
                    .presentation_resources
                    .library
                    .entries
                    .iter()
                    .any(|entry| entry.choice.id == id))
    {
        return Ok(());
    }
    let (mut candidate, manifest) = if id == "classic" {
        (bundled(id)?, None)
    } else {
        let entry = w
            .presentation_resources
            .library
            .entries
            .iter()
            .find(|entry| entry.choice.id == id)
            .cloned()
            .ok_or_else(|| invalid(format!("Theme is not installed: {id}. Open Content Libraries to clone a theme repository.")))?;
        if let Some(repository) = &entry.repository {
            cad_core::content_repositories::read_entry(repository, 4 * 1024 * 1024)?;
        }
        let (candidate, _) = resources::read_library_source(w, &entry.manifest)?;
        (candidate, Some(entry.manifest))
    };
    candidate.library_id = Some(id.into());
    resources::validate(w, &candidate)?;
    resources::install_library(w, candidate, manifest);
    w.status = "Theme preview updated; model, draft, selection and camera retained".into();
    Ok(())
}

/// Flatten inherited files into a self-contained personal library package.
/// Import remains reusable even if its original file moves or Settings is
/// cancelled; selecting it is still only a presentation preview.
pub fn load(w: &mut Workspace, path: PathBuf) -> Result<String> {
    if w.presentation_resources.library.entries.len() >= MAX_CHOICES {
        return Err(invalid(format!(
            "Personal theme library is limited to {MAX_CHOICES} packages"
        )));
    }
    let (mut candidate, name) = resources::read_library_source(w, &path)?;
    let revision = cad_core::id();
    let id = format!("imported.{revision}");
    let folder = directory().join(revision);
    candidate.library_id = Some(id.clone());
    if candidate.active_layout.is_none() {
        candidate.active_layout = Some(
            if candidate.layout.layouts.contains_key(&w.view.active_layout) {
                w.view.active_layout.clone()
            } else if candidate.layout.layouts.contains_key("instrument-rail") {
                "instrument-rail".into()
            } else {
                candidate
                    .layout
                    .layouts
                    .keys()
                    .next()
                    .cloned()
                    .unwrap_or_default()
            },
        );
    }
    let record = Record { schema_version: 1, id: id.clone(), name: name.clone(), description: "Imported portable layout and style package; inherited resources are included in the personal library.".into() };
    resources::export_library(&candidate, &folder, &name)?;
    atomic_write(
        &folder.join("library.json"),
        &serde_json::to_vec_pretty(&record)?,
    )?;
    refresh(w);
    select(w, &id)?;
    Ok(id)
}

pub fn capture(w: &Workspace) -> Snapshot {
    Snapshot {
        resources: w.presentation_resources.clone(),
        layout: w.layout.clone(),
        theme: w.theme.clone(),
        layout_text: w.layout_text.clone(),
        theme_text: w.theme_text.clone(),
        active_layout: w.view.active_layout.clone(),
    }
}

pub fn restore(w: &mut Workspace, snapshot: Snapshot) {
    crate::assembly_motion_ui::finish_drag(w);
    let logical_size = w.presentation_resources.logical_size;
    crate::sketch_handles::end(w);
    crate::navigation::stop(w);
    if let Some(sketch) = &mut w.sketch
        && let Some(dimension) = sketch
            .edit
            .as_mut()
            .and_then(|editor| editor.dimension_group.as_mut())
    {
        dimension.annotation_drag = None;
    }
    w.layout = snapshot.layout;
    w.theme = snapshot.theme;
    w.layout_text = snapshot.layout_text;
    w.theme_text = snapshot.theme_text;
    w.view.active_layout = snapshot.active_layout;
    w.presentation_resources = snapshot.resources;
    w.presentation_resources.logical_size = logical_size;
    refresh(w);
}

pub fn prepare_selected(w: &Workspace) -> Result<PendingSave> {
    resources::prepare_personal(w).map(PendingSave)
}

pub fn commit_selected(pending: &PendingSave) -> Result<()> {
    resources::commit_personal(&pending.0)
}

pub fn rollback_selected(pending: &PendingSave) -> Result<()> {
    resources::rollback_personal(&pending.0)
}

pub fn finish_selected(w: &mut Workspace, pending: &PendingSave) {
    resources::finish_personal(w, &pending.0);
}

pub fn persist_selected(w: &mut Workspace) -> Result<()> {
    let pending = prepare_selected(w)?;
    commit_selected(&pending)?;
    finish_selected(w, &pending);
    Ok(())
}
