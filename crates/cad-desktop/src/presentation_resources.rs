//! Local presentation resources. Reloading replaces a validated resource pair;
//! it never dispatches a document command or changes an engineering transaction.
use crate::Workspace;
use bevy_egui::egui;
use cad_core::{
    CadError, Result,
    persistence::atomic_write,
    presentation::{LayoutDocument, SemanticElement, Theme, default_layout, default_theme},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Read,
    path::{Path, PathBuf},
    time::{Duration, Instant, SystemTime},
};

const MAX_FILE_BYTES: u64 = 4 * 1024 * 1024;
const MAX_TOTAL_BYTES: u64 = 16 * 1024 * 1024;
const POLL_INTERVAL: Duration = Duration::from_millis(400);
const SAVE_SETTLE: Duration = Duration::from_millis(250);
const MAX_STYLE_TARGETS: usize = 131_072;

#[derive(Clone, Debug, PartialEq, Eq)]
enum Target {
    Personal,
    Project(PathBuf),
    Files {
        layout: Option<PathBuf>,
        theme: Option<PathBuf>,
    },
    Package {
        path: PathBuf,
        boundary: Option<PathBuf>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Stamp {
    bytes: u64,
    modified: Option<SystemTime>,
}

#[derive(Clone)]
pub struct State {
    pub library: crate::theme_library::State,
    /// Current complete workspace in logical UI pixels; the render target is
    /// only one region and may use a different pixel density.
    pub logical_size: [f32; 2],
    pub watch: bool,
    pub diagnostic: Option<String>,
    pub library_notice: Option<String>,
    pub layout_path: Option<PathBuf>,
    pub theme_path: Option<PathBuf>,
    pub package_path: Option<PathBuf>,
    target: Option<Target>,
    project_root: Option<PathBuf>,
    watched: BTreeMap<PathBuf, Option<Stamp>>,
    polled: Instant,
    pending: Option<Instant>,
    applied_layout: String,
    applied_theme: String,
}

impl Default for State {
    fn default() -> Self {
        Self {
            library: crate::theme_library::State::default(),
            logical_size: [1440., 900.],
            watch: true,
            diagnostic: None,
            library_notice: None,
            layout_path: None,
            theme_path: None,
            package_path: None,
            target: None,
            project_root: None,
            watched: BTreeMap::new(),
            polled: Instant::now(),
            pending: None,
            applied_layout: String::new(),
            applied_theme: String::new(),
        }
    }
}

/// Portable package paths are relative to the manifest. Export is self-contained.
#[derive(Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Package {
    schema_version: u32,
    /// Legacy manifests are patches over the bundled presentation. Complete
    /// exported packages opt out so reload preserves their exact cascade.
    #[serde(default = "default_inherit_defaults")]
    inherit_defaults: bool,
    #[serde(default)]
    name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    theme_library_id: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    extends: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    layout: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    theme: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    active_layout: Option<String>,
}

fn default_inherit_defaults() -> bool {
    true
}

#[derive(Clone)]
pub(crate) struct Candidate {
    pub layout: LayoutDocument,
    pub theme: Theme,
    pub layout_text: String,
    pub theme_text: String,
    pub active_layout: Option<String>,
    pub layout_path: Option<PathBuf>,
    pub theme_path: Option<PathBuf>,
    pub library_id: Option<String>,
}

#[derive(Default)]
struct Reader {
    paths: BTreeSet<PathBuf>,
    sources: BTreeMap<PathBuf, String>,
    stamps: BTreeMap<PathBuf, Option<Stamp>>,
    stack: Vec<PathBuf>,
    bytes: u64,
    repository_boundary: Option<PathBuf>,
    forced_boundary: Option<PathBuf>,
    library_notice: Option<String>,
}

fn invalid(message: impl Into<String>) -> CadError {
    CadError::Invalid(message.into())
}

fn stamp(path: &Path) -> Option<Stamp> {
    std::fs::metadata(path).ok().map(|metadata| Stamp {
        bytes: metadata.len(),
        modified: metadata.modified().ok(),
    })
}

fn snapshot(paths: impl IntoIterator<Item = PathBuf>) -> BTreeMap<PathBuf, Option<Stamp>> {
    paths
        .into_iter()
        .map(|path| {
            let value = stamp(&path);
            (path, value)
        })
        .collect()
}

fn local_path(base: &Path, text: &str) -> Result<PathBuf> {
    if text.trim().is_empty() || text.contains("://") || text.starts_with("data:") {
        return Err(invalid("Presentation imports require a local file path"));
    }
    let path = Path::new(text);
    Ok(if path.is_absolute() {
        path.to_owned()
    } else {
        base.join(path)
    })
}

fn relative_package_path(base: &Path, text: &str) -> Result<PathBuf> {
    if Path::new(text).is_absolute() {
        return Err(invalid(format!("Package path must be relative: {text}")));
    }
    local_path(base, text)
}

fn parents(value: &mut Value) -> Result<Vec<String>> {
    let Some(value) = value
        .as_object_mut()
        .and_then(|object| object.remove("extends"))
    else {
        return Ok(Vec::new());
    };
    match value {
        Value::String(path) => Ok(vec![path]),
        Value::Array(values) => values
            .into_iter()
            .map(|value| {
                value
                    .as_str()
                    .map(str::to_owned)
                    .ok_or_else(|| invalid("extends must contain file paths"))
            })
            .collect(),
        _ => Err(invalid(
            "extends must be a file path or an array of file paths",
        )),
    }
}

/// Object overrides compose recursively; an array is intentionally replaced whole.
fn merge(base: &mut Value, overrides: Value) {
    if let (Some(base), Value::Object(overrides)) = (base.as_object_mut(), &overrides) {
        for (key, value) in overrides {
            merge(
                base.entry(key.clone()).or_insert(Value::Null),
                value.clone(),
            );
        }
    } else {
        *base = overrides;
    }
}

fn merge_theme(base: &mut Theme, next: Theme) {
    base.schema_version = next.schema_version;
    base.variables.extend(next.variables);
    base.rules.extend(next.rules);
}

impl Reader {
    fn track(&mut self, path: &Path) {
        self.paths.insert(path.to_owned());
        self.stamps
            .entry(path.to_owned())
            .or_insert_with(|| stamp(path));
    }

    fn verify_stable(&self) -> Result<()> {
        if let Some((path, _)) = self
            .stamps
            .iter()
            .find(|(path, expected)| stamp(path) != **expected)
        {
            return Err(invalid(format!(
                "{} changed while loading; waiting for a complete save",
                path.display()
            )));
        }
        Ok(())
    }

    fn file<T>(
        &mut self,
        path: &Path,
        parse: impl FnOnce(&mut Self, &str, &Path) -> Result<T>,
    ) -> Result<T> {
        self.track(path);
        if self.stack.is_empty() {
            self.repository_boundary = if let Some(root) = &self.forced_boundary {
                Some(std::fs::canonicalize(root)?)
            } else {
                cad_core::content_repositories::catalog_snapshot()
                    .entries
                    .iter()
                    .filter(|entry| {
                        entry.entry.kind == cad_core::content_repositories::ContentKind::Themes
                    })
                    .find(|entry| path.starts_with(&entry.root))
                    .map(|entry| std::fs::canonicalize(&entry.root))
                    .transpose()?
            };
        }
        let canonical = std::fs::canonicalize(path)
            .map_err(|error| invalid(format!("{}: {error}", path.display())))?;
        if self
            .repository_boundary
            .as_ref()
            .is_some_and(|root| !canonical.starts_with(root))
        {
            return Err(invalid(
                "Repository presentation imports must stay inside that library checkout",
            ));
        }
        self.track(&canonical);
        if self.stack.contains(&canonical) {
            return Err(invalid(format!(
                "Cyclic presentation import: {}",
                path.display()
            )));
        }
        if self.stack.len() >= 32 || self.paths.len() > 256 {
            return Err(invalid(
                "Presentation imports exceed 32 levels or 128 files",
            ));
        }
        let size = std::fs::metadata(&canonical)?.len();
        if size > MAX_FILE_BYTES {
            return Err(invalid(
                "Presentation resources exceed the 4 MiB file or 16 MiB import limit",
            ));
        }
        let source = if let Some(source) = self.sources.get(&canonical) {
            source.clone()
        } else {
            let mut source = String::new();
            std::fs::File::open(&canonical)?
                .take(MAX_FILE_BYTES + 1)
                .read_to_string(&mut source)
                .map_err(|error| invalid(format!("{}: {error}", path.display())))?;
            source
        };
        self.bytes = self.bytes.saturating_add(source.len() as u64);
        if source.len() as u64 > MAX_FILE_BYTES || self.bytes > MAX_TOTAL_BYTES {
            return Err(invalid(
                "Presentation resources exceed the 4 MiB file or 16 MiB import limit",
            ));
        }
        self.sources.insert(path.to_owned(), source.clone());
        self.sources.insert(canonical.clone(), source.clone());
        self.stack.push(canonical.clone());
        let result = parse(self, &source, canonical.parent().unwrap_or(Path::new(".")));
        self.stack.pop();
        result.map_err(|error| invalid(format!("{}: {error}", path.display())))
    }

    fn layout(&mut self, source: &str, base: &Path) -> Result<Value> {
        let mut own: Value = serde_json::from_str(source)?;
        let inherited = parents(&mut own)?;
        let mut value = Value::Object(Default::default());
        for parent in inherited {
            let next = self.file(&local_path(base, &parent)?, Self::layout)?;
            merge(&mut value, next);
        }
        merge(&mut value, own);
        Ok(value)
    }

    fn theme(&mut self, source: &str, base: &Path) -> Result<Theme> {
        let mut theme = Theme::default();
        if source.trim_start().starts_with('{') {
            let mut own: Value = serde_json::from_str(source)?;
            for parent in parents(&mut own)? {
                let next = self.file(&local_path(base, &parent)?, Self::theme)?;
                merge_theme(&mut theme, next);
            }
            merge_theme(&mut theme, serde_json::from_value(own)?);
        } else {
            let mut remaining = source;
            loop {
                remaining = leading_comments(remaining)?;
                if !remaining.starts_with("@import") {
                    break;
                }
                let (import, rest) = remaining
                    .split_once(';')
                    .ok_or_else(|| invalid("CSS @import requires a terminating semicolon"))?;
                let text = import[7..].trim();
                let quote = text
                    .chars()
                    .next()
                    .filter(|quote| *quote == '\'' || *quote == '"')
                    .ok_or_else(|| invalid("CSS @import accepts a quoted local file path"))?;
                let path = text
                    .strip_prefix(quote)
                    .and_then(|path| path.strip_suffix(quote))
                    .filter(|path| !path.contains(quote))
                    .ok_or_else(|| invalid("CSS @import accepts one quoted local file path"))?;
                let next = self.file(&local_path(base, path)?, Self::theme)?;
                merge_theme(&mut theme, next);
                remaining = rest;
            }
            // Compose first: imported and scoped variables retain their original
            // declaration positions, rather than becoming global variables.
            let local = Theme::parse_css(remaining)?;
            merge_theme(&mut theme, local);
        }
        Ok(theme)
    }

    fn package(&mut self, source: &str, base: &Path) -> Result<Candidate> {
        let package: Package = serde_json::from_str(source)?;
        if package.schema_version != 1 {
            return Err(invalid(
                "Unsupported presentation package version; expected 1",
            ));
        }
        let mut layout = if package.inherit_defaults {
            serde_json::to_value(default_layout())?
        } else {
            serde_json::json!({"schema_version":1,"components":{},"layouts":{}})
        };
        let mut theme = if package.inherit_defaults {
            default_theme()
        } else {
            Theme::default()
        };
        let mut active_layout = None;
        for parent in &package.extends {
            let next = self.file(&relative_package_path(base, parent)?, Self::package)?;
            merge(&mut layout, serde_json::to_value(next.layout)?);
            merge_theme(&mut theme, next.theme);
            if next.active_layout.is_some() {
                active_layout = next.active_layout;
            }
        }
        let layout_path = package
            .layout
            .as_ref()
            .map(|path| relative_package_path(base, path))
            .transpose()?;
        if let Some(path) = &layout_path {
            let next = self.file(path, Self::layout)?;
            merge(&mut layout, next);
        }
        let theme_path = package
            .theme
            .as_ref()
            .map(|path| relative_package_path(base, path))
            .transpose()?;
        if let Some(path) = &theme_path {
            let next = self.file(path, Self::theme)?;
            merge_theme(&mut theme, next);
        }
        let layout: LayoutDocument = serde_json::from_value(layout)?;
        if package.active_layout.is_some() {
            active_layout = package.active_layout;
        }
        Ok(Candidate {
            layout_text: serde_json::to_string_pretty(&layout)?,
            theme_text: serde_json::to_string_pretty(&theme)?,
            layout,
            theme,
            active_layout,
            layout_path,
            theme_path,
            library_id: package.theme_library_id,
        })
    }
}

fn leading_comments(mut source: &str) -> Result<&str> {
    loop {
        source = source.trim_start();
        let Some(rest) = source.strip_prefix("/*") else {
            return Ok(source);
        };
        let end = rest
            .find("*/")
            .ok_or_else(|| invalid("Unclosed CSS comment"))?;
        source = &rest[end + 2..];
    }
}

pub(crate) fn validate(w: &Workspace, candidate: &Candidate) -> Result<()> {
    candidate.layout.validate()?;
    candidate.theme.validate()?;
    candidate.theme.validate_renderer_extensions(&[])?;
    if let Some(name) = &candidate.active_layout
        && !candidate.layout.layouts.contains_key(name)
    {
        return Err(invalid(format!(
            "Package active_layout does not exist: {name}"
        )));
    }
    // Resolve against the same semantic snapshot used by the desktop adapter.
    // Resource syntax alone cannot prove scoped variables or repeated placement
    // identities: both depend on the candidate's actual composition.
    let semantics = crate::presentation_composition::semantics(w);
    let expanded = candidate.layout.expand_semantics(&semantics)?;
    let mut controls = crate::cad_ui::semantics(w);
    let mut settings_controls = crate::settings_ui::semantics(w);
    for element in &mut settings_controls {
        if element.role == "settings-field" {
            element.data["classes"] = serde_json::json!(["semantic-control"]);
        } else if element.id == "command:settings.ok" {
            element.data["classes"] = serde_json::json!(["primary"]);
        }
    }
    controls.extend(settings_controls.iter().cloned());
    controls.extend(
        self::semantics(w)
            .into_iter()
            .filter(|element| element.role != "presentation-resource-diagnostics")
            .map(|mut element| {
                element.data["parent"] = serde_json::json!("property-manager");
                element
            }),
    );
    let controls: BTreeMap<_, _> = controls
        .into_iter()
        .map(|element| (element.id.clone(), element))
        .collect();
    let width = w.presentation_resources.logical_size[0].max(1.);
    let height = w.presentation_resources.logical_size[1].max(1.);
    let mut checked = 0_usize;
    let mut check = |target: &cad_core::presentation::StyleTarget| -> Result<()> {
        checked += 1;
        if checked > MAX_STYLE_TARGETS {
            return Err(invalid(
                "Presentation contextual validation exceeds 131072 targets; reduce duplicate controls or compositions",
            ));
        }
        candidate.theme.properties_checked_for(target)?;
        Ok(())
    };
    for name in expanded.layouts.keys() {
        // Native settings is an overlay over the workspace rather than a
        // viewport region. Check its actual ancestor chain independently of
        // whether a custom layout additionally binds the same fields.
        let workspace_target = cad_core::presentation::Region {
            id: "workspace".into(),
            role: "workspace".into(),
            component: None,
            rect: [0.; 4],
            classes: vec![],
            resizable: false,
        }
        .style_target(name, "ready");
        let dialog_target = workspace_target.child(
            "settings:dialog",
            "settings-dialog",
            &["settings-surface".into()],
            "open",
        );
        check(&dialog_target)?;
        for element in settings_controls
            .iter()
            .filter(|element| element.role != "settings-dialog")
        {
            check_control(&dialog_target, element, &controls, 0, &mut check)?;
        }
        for editing in [false, true] {
            let regions = expanded
                .regions_presented(name, width, height, editing, &candidate.theme)
                .map_err(|error| {
                    invalid(format!("Composition {name} (editing={editing}): {error}"))
                })?;
            for region in &regions {
                let binding = expanded.binding_for_in(name, region);
                let bound = binding
                    .as_ref()
                    .and_then(|binding| binding.bind.as_deref())
                    .and_then(|id| semantics.iter().find(|element| element.id == id));
                let state = bound
                    .map(|element| element.state.as_str())
                    .unwrap_or(if editing { "editing" } else { "ready" });
                let parent_target = expanded.style_target(region, name, state);
                check(&parent_target)?;
                if region.rect[2] < 1. || region.rect[3] < 1. {
                    continue;
                }
                if let Some(control) = bound.and_then(|element| controls.get(&element.id)) {
                    check_control(&parent_target, control, &controls, 0, &mut check)?;
                }
                // A scalar/action binding takes precedence over a native view.
                // Validate composites only where their renderer actually runs.
                if binding
                    .as_ref()
                    .is_some_and(|binding| binding.command.is_some())
                    || bound.is_some_and(|element| {
                        element.data.get("renderer").and_then(Value::as_str)
                            != Some(region.role.as_str())
                    })
                {
                    continue;
                }
                for element in controls.values() {
                    // Menu actions live beneath their declared owning menu.
                    // Unlisted actions are dormant descriptors, not fake views.
                    if element.role == "menu-action" {
                        continue;
                    }
                    let parent = element
                        .data
                        .get("parent")
                        .and_then(Value::as_str)
                        .or_else(|| element.relationships.first().map(String::as_str))
                        .unwrap_or("property-manager");
                    if parent != region.role
                        && parent != region.id
                        && binding.as_ref().and_then(|binding| binding.bind.as_deref())
                            != Some(parent)
                    {
                        continue;
                    }
                    check_control(&parent_target, element, &controls, 0, &mut check)?;
                }
            }
        }
    }
    Ok(())
}

fn check_control(
    parent: &cad_core::presentation::StyleTarget,
    element: &SemanticElement,
    controls: &BTreeMap<String, SemanticElement>,
    depth: usize,
    check: &mut impl FnMut(&cad_core::presentation::StyleTarget) -> Result<()>,
) -> Result<()> {
    if depth > 32 {
        return Err(invalid(
            "Native presentation menu nesting exceeds 32 levels",
        ));
    }
    let classes = element
        .data
        .get("classes")
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let states: BTreeSet<_> = ["ready", "selected", "disabled", element.state.as_str()]
        .into_iter()
        .collect();
    for state in states {
        for interaction in ["", " hover", " focus focused", " hover focus focused"] {
            check(&parent.child(
                &element.id,
                &element.role,
                &classes,
                &format!("{state}{interaction}"),
            ))?;
        }
    }
    if element.role == "menu" {
        let menu = parent.child(&element.id, &element.role, &classes, "ready");
        for id in element
            .data
            .get("items")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            if let Some(child) = controls.get(id) {
                check_control(&menu, child, controls, depth + 1, check)?;
            }
        }
    }
    Ok(())
}

fn project_target(w: &Workspace) -> Option<PathBuf> {
    w.project.as_ref().map(|_| w.root())
}

pub(crate) fn personal_directory() -> PathBuf {
    crate::recovery_directory()
        .parent()
        .unwrap_or(Path::new("."))
        .join("presentation")
}

fn source_candidate(w: &Workspace, reader: &mut Reader) -> Result<Candidate> {
    if w.layout_text.len() as u64 > MAX_FILE_BYTES || w.theme_text.len() as u64 > MAX_FILE_BYTES {
        return Err(invalid(
            "Presentation source exceeds the 4 MiB per resource limit",
        ));
    }
    let directory = w.root().join("ui");
    let layout_path = w.presentation_resources.layout_path.clone();
    let theme_path = w.presentation_resources.theme_path.clone();
    let layout: LayoutDocument = serde_json::from_value(
        reader.layout(
            &w.layout_text,
            layout_path
                .as_deref()
                .and_then(Path::parent)
                .unwrap_or(&directory),
        )?,
    )?;
    let theme = reader.theme(
        &w.theme_text,
        theme_path
            .as_deref()
            .and_then(Path::parent)
            .unwrap_or(&directory),
    )?;
    Ok(Candidate {
        layout,
        theme,
        layout_text: w.layout_text.clone(),
        theme_text: w.theme_text.clone(),
        active_layout: None,
        layout_path,
        theme_path,
        library_id: Some(
            if w.layout_text == w.presentation_resources.applied_layout
                && w.theme_text == w.presentation_resources.applied_theme
            {
                w.presentation_resources.library.selected.clone()
            } else {
                "custom".into()
            },
        ),
    })
}

fn package_target(path: PathBuf) -> Target {
    let boundary = cad_core::content_repositories::catalog_snapshot()
        .entries
        .iter()
        .filter(|entry| entry.entry.kind == cad_core::content_repositories::ContentKind::Themes)
        .find(|entry| path.starts_with(&entry.root))
        .map(|entry| entry.root.clone());
    Target::Package { path, boundary }
}

fn load_target(target: &Target, reader: &mut Reader) -> Result<Candidate> {
    match target {
        Target::Personal => {
            let manifest = personal_directory().join("presentation.json");
            reader.track(&manifest);
            if manifest.is_file() {
                let saved = reader.file(&manifest, Reader::package)?;
                if let Some(path) = saved
                    .library_id
                    .as_deref()
                    .and_then(crate::theme_library::repository_manifest)
                {
                    // The saved self-contained theme is authoritative recovery
                    // data. An optional checkout can be offline, removed or have
                    // incomplete edits without making the workspace unusable.
                    match reader.file(&path, Reader::package) {
                        Ok(mut live) => {
                            live.library_id = saved.library_id;
                            Ok(live)
                        }
                        Err(error) => {
                            reader.library_notice = Some(format!(
                                "The theme repository is unavailable. Using your saved theme snapshot. {error}"
                            ));
                            Ok(saved)
                        }
                    }
                } else {
                    if saved
                        .library_id
                        .as_deref()
                        .is_some_and(|id| id.starts_with("repository:") || id == "nocturne")
                    {
                        reader.library_notice = Some("The theme repository is not installed. Using your saved theme snapshot; your model and edits are unchanged.".into());
                    }
                    Ok(saved)
                }
            } else {
                load_files(reader, None, None, None)
            }
        }
        Target::Package { path, boundary } => {
            reader.forced_boundary = boundary.clone();
            reader.file(path, Reader::package)
        }
        Target::Project(root) => {
            let directory = root.join("ui");
            let manifest = directory.join("presentation.json");
            reader.track(&manifest);
            if manifest.is_file() {
                return reader.file(&manifest, Reader::package);
            }
            let layout = directory.join("layout.json");
            let css = directory.join("theme.css");
            let json = directory.join("theme.json");
            for path in [&layout, &css, &json] {
                reader.track(path);
            }
            let personal = load_target(&Target::Personal, reader)?;
            load_files(
                reader,
                layout.is_file().then_some(layout),
                if css.is_file() {
                    Some(css)
                } else {
                    json.is_file().then_some(json)
                },
                Some(personal),
            )
        }
        Target::Files { layout, theme } => load_files(reader, layout.clone(), theme.clone(), None),
    }
}

fn load_files(
    reader: &mut Reader,
    layout_path: Option<PathBuf>,
    theme_path: Option<PathBuf>,
    base: Option<Candidate>,
) -> Result<Candidate> {
    let mut candidate = if let Some(base) = base {
        base
    } else {
        let layout = default_layout();
        let theme = default_theme();
        Candidate {
            layout_text: serde_json::to_string_pretty(&layout)?,
            theme_text: serde_json::to_string_pretty(&theme)?,
            layout,
            theme,
            active_layout: None,
            layout_path: None,
            theme_path: None,
            library_id: None,
        }
    };
    if layout_path.is_some() || theme_path.is_some() {
        candidate.library_id = None;
    }
    if let Some(path) = &layout_path {
        candidate.layout = serde_json::from_value(reader.file(path, Reader::layout)?)?;
        candidate.layout_text = reader
            .sources
            .get(path)
            .cloned()
            .ok_or_else(|| invalid("Layout source was not loaded"))?;
        candidate.layout_path = layout_path;
    }
    if let Some(path) = &theme_path {
        candidate.theme = reader.file(path, Reader::theme)?;
        candidate.theme_text = reader
            .sources
            .get(path)
            .cloned()
            .ok_or_else(|| invalid("Theme source was not loaded"))?;
        candidate.theme_path = theme_path;
    }
    Ok(candidate)
}

fn install(w: &mut Workspace, candidate: Candidate, paths: BTreeSet<PathBuf>) {
    // A layout reload invalidates paper hit geometry; retain earlier drawing edits.
    if crate::drawing_interaction_ui::active(w) {
        crate::drawing_interaction_ui::escape(w);
    }
    crate::assembly_motion_ui::finish_drag(w);
    // Release the obsolete hit geometry while keeping the current pending edit.
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
    w.layout = candidate.layout;
    w.theme = candidate.theme;
    if let Some(name) = candidate.active_layout {
        w.view.active_layout = name;
    }
    if !w.layout.layouts.contains_key(&w.view.active_layout) {
        w.view.active_layout = if w.layout.layouts.contains_key("instrument-rail") {
            "instrument-rail".into()
        } else {
            w.layout.layouts.keys().next().cloned().unwrap_or_default()
        };
    }
    w.layout_text = candidate.layout_text;
    w.theme_text = candidate.theme_text;
    let resources = &mut w.presentation_resources;
    resources.library.selected = candidate.library_id.unwrap_or_else(|| {
        if serde_json::to_value(&w.layout).ok() == serde_json::to_value(default_layout()).ok()
            && serde_json::to_value(&w.theme).ok() == serde_json::to_value(default_theme()).ok()
        {
            "classic".into()
        } else {
            "custom".into()
        }
    });
    resources.layout_path = candidate.layout_path;
    resources.theme_path = candidate.theme_path;
    resources.applied_layout.clone_from(&w.layout_text);
    resources.applied_theme.clone_from(&w.theme_text);
    resources.watched = snapshot(paths);
    resources.pending = None;
    resources.diagnostic = None;
    resources.library_notice = None;
}

fn report(w: &mut Workspace, error: impl std::fmt::Display) {
    let message = format!("Presentation retained: {error}");
    w.presentation_resources.diagnostic = Some(message.clone());
    w.status = message;
}

fn reload(w: &mut Workspace, target: Target) {
    let mut reader = Reader::default();
    let result = load_target(&target, &mut reader).and_then(|candidate| {
        validate(w, &candidate)?;
        reader.verify_stable()?;
        Ok(candidate)
    });
    match result {
        Ok(candidate) => {
            install(w, candidate, reader.paths);
            w.presentation_resources.library_notice = reader.library_notice;
            w.presentation_resources.target = Some(target.clone());
            w.presentation_resources.package_path = match &target {
                Target::Package { path, .. } => Some(path.clone()),
                Target::Personal if personal_directory().join("presentation.json").is_file() => {
                    Some(personal_directory().join("presentation.json"))
                }
                Target::Project(root) if root.join("ui/presentation.json").is_file() => {
                    Some(root.join("ui/presentation.json"))
                }
                _ => None,
            };
            w.status =
                "Presentation reloaded; document, draft, selection and camera retained".into();
        }
        Err(error) => {
            if w.presentation_resources
                .target
                .as_ref()
                .is_none_or(|current| current == &target)
            {
                w.presentation_resources.target = Some(target);
                w.presentation_resources.watched.extend(reader.stamps);
            }
            w.presentation_resources.pending = None;
            report(w, error);
        }
    }
}

pub fn initialize(w: &mut Workspace, recovery: bool) {
    crate::theme_library::refresh(w);
    w.presentation_resources.project_root = project_target(w);
    w.presentation_resources
        .applied_layout
        .clone_from(&w.layout_text);
    w.presentation_resources
        .applied_theme
        .clone_from(&w.theme_text);
    if recovery {
        w.presentation_resources.watch = false;
    } else {
        let original_status = w.status.clone();
        reload(
            w,
            project_target(w)
                .map(Target::Project)
                .unwrap_or(Target::Personal),
        );
        if w.presentation_resources.diagnostic.is_none() {
            w.status = original_status;
        }
    }
}

/// Called once per UI frame. Filesystem polling is throttled and saves settle
/// before parsing; failures leave both previous resources installed.
pub fn poll(w: &mut Workspace) {
    let now = Instant::now();
    if now.duration_since(w.presentation_resources.polled) < POLL_INTERVAL {
        return;
    }
    w.presentation_resources.polled = now;
    let project = project_target(w);
    if project != w.presentation_resources.project_root && w.settings.is_none() {
        w.presentation_resources.project_root = project.clone();
        let status = w.status.clone();
        reload(w, project.map(Target::Project).unwrap_or(Target::Personal));
        if w.presentation_resources.diagnostic.is_none() {
            w.status = status;
        }
        return;
    }
    if !w.presentation_resources.watch {
        return;
    }
    let next = snapshot(w.presentation_resources.watched.keys().cloned());
    if next != w.presentation_resources.watched {
        w.presentation_resources.watched = next;
        w.presentation_resources.pending = Some(now);
        return;
    }
    if !w
        .presentation_resources
        .pending
        .is_some_and(|changed| now.duration_since(changed) >= SAVE_SETTLE)
    {
        return;
    }
    w.presentation_resources.pending = None;
    if w.layout_text != w.presentation_resources.applied_layout
        || w.theme_text != w.presentation_resources.applied_theme
    {
        report(
            w,
            "External files changed while presentation source has unapplied edits. Apply source or choose Reload external files.",
        );
        return;
    }
    if let Some(target) = w.presentation_resources.target.clone() {
        reload(w, target);
    }
}

pub fn reset(w: &mut Workspace) {
    let candidate = Candidate {
        layout: default_layout(),
        theme: default_theme(),
        layout_text: serde_json::to_string_pretty(&default_layout()).unwrap_or_default(),
        theme_text: serde_json::to_string_pretty(&default_theme()).unwrap_or_default(),
        active_layout: Some("instrument-rail".into()),
        layout_path: None,
        theme_path: None,
        library_id: Some("classic".into()),
    };
    install(w, candidate, BTreeSet::new());
    w.presentation_resources.target = None;
    w.presentation_resources.package_path = None;
    w.presentation_resources.watch = false;
    w.status =
        "Bundled presentation restored; external reload paused. Document and draft retained."
            .into();
}

pub fn apply_sources(w: &mut Workspace) {
    let mut reader = Reader::default();
    match source_candidate(w, &mut reader).and_then(|candidate| {
        validate(w, &candidate)?;
        reader.verify_stable()?;
        Ok(candidate)
    }) {
        Ok(candidate) => {
            reader
                .paths
                .extend(w.presentation_resources.layout_path.iter().cloned());
            reader
                .paths
                .extend(w.presentation_resources.theme_path.iter().cloned());
            install(w, candidate, reader.paths);
            w.presentation_resources.target = Some(Target::Files {
                layout: w.presentation_resources.layout_path.clone(),
                theme: w.presentation_resources.theme_path.clone(),
            });
            w.presentation_resources.package_path = None;
            w.status =
                "Presentation source applied; document and pending CAD edits retained".into();
        }
        Err(error) => report(w, error),
    }
}

pub fn save_project(w: &mut Workspace) {
    if w.project.is_none() {
        report(
            w,
            "Save the CAD project before sharing presentation resources with it",
        );
        return;
    }
    let mut reader = Reader::default();
    let result = source_candidate(w, &mut reader).and_then(|candidate| {
        validate(w, &candidate)?;
        reader.verify_stable()?;
        let directory = w.root().join("ui");
        export_candidate(&candidate, &directory, "TCAD project presentation")?;
        atomic_write(
            &directory.join("workspace.json"),
            &serde_json::to_vec_pretty(&w.view)?,
        )?;
        Ok(())
    });
    match result {
        Ok(()) => {
            let root = w.root();
            reload(w, Target::Project(root));
            if w.presentation_resources.diagnostic.is_none() {
                w.status = "Saved validated presentation package to project ui/".into();
            }
        }
        Err(error) => report(w, error),
    }
}

fn export_candidate(candidate: &Candidate, directory: &Path, name: &str) -> Result<()> {
    let manifest = prepare_export(candidate, directory, name)?;
    atomic_write(&directory.join("presentation.json"), &manifest)
}

fn prepare_export(candidate: &Candidate, directory: &Path, name: &str) -> Result<Vec<u8>> {
    let revision = cad_core::id();
    let layout_name = format!("layout-{revision}.json");
    let theme_name = format!("theme-{revision}.json");
    let package = Package {
        schema_version: 1,
        inherit_defaults: false,
        name: name.into(),
        theme_library_id: candidate.library_id.clone(),
        layout: Some(layout_name.clone()),
        theme: Some(theme_name.clone()),
        active_layout: candidate.active_layout.clone(),
        ..Default::default()
    };
    // The manifest is written last, so a newly exported package is discoverable
    // only after its self-contained resources have been saved.
    atomic_write(
        &directory.join(layout_name),
        &serde_json::to_vec_pretty(&candidate.layout)?,
    )?;
    atomic_write(
        &directory.join(theme_name),
        &serde_json::to_vec_pretty(&candidate.theme)?,
    )?;
    Ok(serde_json::to_vec_pretty(&package)?)
}

/// Read a package or a standalone CSS/JSON theme using the same bounded,
/// inheritance-aware loader as the presentation source editor.
pub(crate) fn read_library_source(w: &Workspace, path: &Path) -> Result<(Candidate, String)> {
    let mut reader = Reader::default();
    let mut name = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("Imported theme")
        .to_owned();
    let candidate = reader.file(path, |reader, source, base| {
        let is_json = source.trim_start().starts_with('{');
        let package = if is_json {
            let value: Value = serde_json::from_str(source)?;
            if value.get("layouts").is_some() {
                return Err(invalid("Load a presentation manifest that references this layout, or a CSS/JSON theme file"));
            }
            value.get("rules").is_none()
                && value.get("variables").is_none()
                && [
                    "layout",
                    "theme",
                    "active_layout",
                    "name",
                    "inherit_defaults",
                    "theme_library_id",
                ]
                .iter()
                .any(|key| value.get(*key).is_some())
        } else {
            false
        };
        if package {
            let metadata: Package = serde_json::from_str(source)?;
            if !metadata.name.trim().is_empty() {
                name = metadata.name;
            }
            reader.package(source, base)
        } else {
            let theme = reader.theme(source, base)?;
            Ok(Candidate {
                layout: w.layout.clone(),
                layout_text: serde_json::to_string_pretty(&w.layout)?,
                theme_text: serde_json::to_string_pretty(&theme)?,
                theme,
                active_layout: Some(w.view.active_layout.clone()),
                layout_path: None,
                theme_path: Some(path.to_owned()),
                library_id: None,
            })
        }
    })?;
    validate(w, &candidate)?;
    reader.verify_stable()?;
    Ok((candidate, name.chars().take(160).collect()))
}

pub(crate) fn export_library(candidate: &Candidate, directory: &Path, name: &str) -> Result<()> {
    export_candidate(candidate, directory, name)
}

pub(crate) fn install_library(w: &mut Workspace, candidate: Candidate, package: Option<PathBuf>) {
    let watch = w.presentation_resources.watch;
    let paths = package
        .iter()
        .cloned()
        .chain(candidate.layout_path.iter().cloned())
        .chain(candidate.theme_path.iter().cloned())
        .collect();
    install(w, candidate, paths);
    w.presentation_resources.target = package.clone().map(package_target);
    w.presentation_resources.package_path = package;
    w.presentation_resources.watch = watch;
}

#[derive(Clone)]
pub(crate) struct PendingPersonal {
    manifest: PathBuf,
    previous: Option<Vec<u8>>,
    next: Vec<u8>,
    candidate: Candidate,
    paths: BTreeSet<PathBuf>,
}

fn installed_candidate(w: &Workspace) -> Result<Candidate> {
    Ok(Candidate {
        layout: w.layout.clone(),
        theme: w.theme.clone(),
        layout_text: serde_json::to_string_pretty(&w.layout)?,
        theme_text: serde_json::to_string_pretty(&w.theme)?,
        active_layout: Some(w.view.active_layout.clone()),
        layout_path: None,
        theme_path: None,
        library_id: Some(w.presentation_resources.library.selected.clone()),
    })
}

pub(crate) fn prepare_personal(w: &Workspace) -> Result<PendingPersonal> {
    let mut candidate = installed_candidate(w)?;
    validate(w, &candidate)?;
    let directory = personal_directory();
    let manifest = directory.join("presentation.json");
    let previous = match std::fs::File::open(&manifest) {
        Ok(file) => {
            let mut bytes = Vec::new();
            file.take(MAX_FILE_BYTES + 1).read_to_end(&mut bytes)?;
            if bytes.len() as u64 > MAX_FILE_BYTES {
                return Err(invalid(
                    "Existing personal presentation manifest exceeds 4 MiB",
                ));
            }
            Some(bytes)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error.into()),
    };
    let next = prepare_export(&candidate, &directory, "Personal TCAD presentation")?;
    let package: Package = serde_json::from_slice(&next)?;
    candidate.layout_path = package.layout.map(|name| directory.join(name));
    candidate.theme_path = package.theme.map(|name| directory.join(name));
    let paths = std::iter::once(manifest.clone())
        .chain(candidate.layout_path.iter().cloned())
        .chain(candidate.theme_path.iter().cloned())
        .collect();
    Ok(PendingPersonal {
        manifest,
        previous,
        next,
        candidate,
        paths,
    })
}

pub(crate) fn commit_personal(pending: &PendingPersonal) -> Result<()> {
    atomic_write(&pending.manifest, &pending.next)
}

pub(crate) fn rollback_personal(pending: &PendingPersonal) -> Result<()> {
    if let Some(previous) = &pending.previous {
        atomic_write(&pending.manifest, previous)
    } else {
        match std::fs::remove_file(&pending.manifest) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        }
    }
}

pub(crate) fn finish_personal(w: &mut Workspace, pending: &PendingPersonal) {
    let watch = w.presentation_resources.watch;
    install(w, pending.candidate.clone(), pending.paths.clone());
    w.presentation_resources.target = Some(Target::Personal);
    w.presentation_resources.package_path = Some(pending.manifest.clone());
    w.presentation_resources.watch = watch;
    if pending
        .candidate
        .library_id
        .as_deref()
        .and_then(crate::theme_library::repository_manifest)
        .is_some()
    {
        reload(w, Target::Personal);
    }
}

fn import_source(w: &mut Workspace, path: PathBuf, theme: bool) {
    let mut reader = Reader::default();
    let result = reader.file(&path, |reader, source, base| {
        let directory = w.root().join("ui");
        let layout_path = if theme {
            w.presentation_resources.layout_path.clone()
        } else {
            Some(path.clone())
        };
        let theme_path = if theme {
            Some(path.clone())
        } else {
            w.presentation_resources.theme_path.clone()
        };
        let layout_text = if theme {
            w.layout_text.clone()
        } else {
            source.into()
        };
        let theme_text = if theme {
            source.into()
        } else {
            w.theme_text.clone()
        };
        let layout = serde_json::from_value(reader.layout(
            &layout_text,
            if theme {
                layout_path
                    .as_deref()
                    .and_then(Path::parent)
                    .unwrap_or(&directory)
            } else {
                base
            },
        )?)?;
        let loaded_theme = reader.theme(
            &theme_text,
            if theme {
                base
            } else {
                theme_path
                    .as_deref()
                    .and_then(Path::parent)
                    .unwrap_or(&directory)
            },
        )?;
        let candidate = Candidate {
            layout,
            theme: loaded_theme,
            layout_text,
            theme_text,
            active_layout: None,
            layout_path,
            theme_path,
            library_id: None,
        };
        validate(w, &candidate)?;
        reader.verify_stable()?;
        Ok(candidate)
    });
    match result {
        Ok(candidate) => {
            reader.paths.extend(candidate.layout_path.iter().cloned());
            reader.paths.extend(candidate.theme_path.iter().cloned());
            install(w, candidate, reader.paths);
            w.presentation_resources.target = Some(Target::Files {
                layout: w.presentation_resources.layout_path.clone(),
                theme: w.presentation_resources.theme_path.clone(),
            });
            w.presentation_resources.package_path = None;
            w.presentation_resources.watch = true;
            w.status =
                "Imported validated presentation resource; external edits reload automatically"
                    .into();
        }
        Err(error) => report(w, error),
    }
}

pub fn action(w: &mut Workspace, command: &str) -> bool {
    match command {
        "presentation.apply-source" => apply_sources(w),
        "presentation.reset" => reset(w),
        "presentation.save-project" => save_project(w),
        "presentation.save-personal" => {
            if w.layout_text == w.presentation_resources.applied_layout
                && w.theme_text == w.presentation_resources.applied_theme
            {
                match crate::theme_library::persist_selected(w) {
                    Ok(()) => {
                        w.presentation_resources.watch = true;
                        w.status =
                            "Saved personal presentation default; CAD project source unchanged"
                                .into();
                    }
                    Err(error) => report(w, error),
                }
                return true;
            }
            let mut reader = Reader::default();
            let result = source_candidate(w, &mut reader).and_then(|mut candidate| {
                validate(w, &candidate)?;
                reader.verify_stable()?;
                candidate.active_layout = Some(w.view.active_layout.clone());
                export_candidate(
                    &candidate,
                    &personal_directory(),
                    "Personal TCAD presentation",
                )
            });
            match result {
                Ok(()) => {
                    reload(w, Target::Personal);
                    w.presentation_resources.watch = true;
                    if w.presentation_resources.diagnostic.is_none() {
                        w.status =
                            "Saved personal presentation default; CAD project source unchanged"
                                .into();
                    }
                }
                Err(error) => report(w, error),
            }
        }
        "presentation.toggle-watch" => {
            w.presentation_resources.watch = !w.presentation_resources.watch;
            crate::settings::save_live_application(w);
        }
        "presentation.reload-files" => {
            if let Some(target) = w.presentation_resources.target.clone() {
                reload(w, target);
            } else if let Some(root) = project_target(w) {
                reload(w, Target::Project(root));
            } else {
                reload(w, Target::Personal);
            }
        }
        "presentation.import-layout" | "presentation.import-theme" => {
            let theme = command.ends_with("theme");
            if let Some(path) = rfd::FileDialog::new()
                .set_title(if theme {
                    "Import theme"
                } else {
                    "Import layout"
                })
                .add_filter(
                    "Presentation source",
                    if theme {
                        &["css", "json"][..]
                    } else {
                        &["json"][..]
                    },
                )
                .pick_file()
            {
                import_source(w, path, theme);
            }
        }
        "presentation.import-package" => {
            if let Some(path) = rfd::FileDialog::new()
                .set_title("Import presentation package manifest")
                .add_filter("Presentation manifest", &["json"])
                .pick_file()
            {
                reload(w, package_target(path));
                w.presentation_resources.watch = true;
            }
        }
        "presentation.export-package" => {
            if let Some(directory) = rfd::FileDialog::new()
                .set_title("Export self-contained presentation package into folder")
                .pick_folder()
            {
                let mut reader = Reader::default();
                let candidate = if w.settings.is_some() {
                    installed_candidate(w)
                } else {
                    source_candidate(w, &mut reader)
                };
                let result = candidate.and_then(|mut candidate| {
                    validate(w, &candidate)?;
                    reader.verify_stable()?;
                    candidate.active_layout = Some(w.view.active_layout.clone());
                    export_candidate(
                        &candidate,
                        &directory.join("tcad-presentation"),
                        "TCAD presentation",
                    )
                });
                match result {
                    Ok(()) => {
                        w.presentation_resources.diagnostic = None;
                        w.status = format!(
                            "Exported portable presentation package to {}",
                            directory.join("tcad-presentation").display()
                        )
                    }
                    Err(error) => report(w, error),
                }
            }
        }
        _ => return false,
    }
    true
}

pub fn semantics(w: &Workspace) -> Vec<SemanticElement> {
    let mut elements = Vec::new();
    for (command, label) in [
        ("presentation.apply-source", "Apply presentation source"),
        ("presentation.reset", "Restore bundled presentation"),
        (
            "presentation.reload-files",
            "Reload external presentation files",
        ),
        ("presentation.save-project", "Save presentation to project"),
        (
            "presentation.save-personal",
            "Save personal presentation default",
        ),
        (
            "presentation.toggle-watch",
            "Toggle automatic external presentation reload",
        ),
        ("presentation.import-layout", "Import layout"),
        ("presentation.import-theme", "Import theme"),
        ("presentation.import-package", "Import presentation package"),
        ("presentation.export-package", "Export presentation package"),
        (
            "content-repositories.download-themes",
            "Download themes pack",
        ),
        ("content-repositories.show-themes", "View themes pack"),
    ] {
        elements.push(SemanticElement {
            id: format!("workspace.{command}"),
            role: "presentation-resource-action".into(),
            label: label.into(),
            state: "ready".into(),
            commands: vec![command.into()],
            value: None,
            relationships: vec![],
            data: serde_json::json!({"accessibility_label": label, "presentation_only": true,"classes":["compact"]}),
        });
    }
    elements.extend(crate::theme_library::fallback_notice(w));
    elements.push(SemanticElement {
        id: "workspace.presentation.diagnostics".into(), role: "presentation-resource-diagnostics".into(),
        label: "Presentation resource diagnostics".into(), value: w.presentation_resources.diagnostic.clone(),
        state: if w.presentation_resources.diagnostic.is_some() { "failed" } else { "valid" }.into(),
        commands: vec!["presentation.reload-files".into(), "presentation.reset".into()], relationships: vec![],
        data: serde_json::json!({"watch":w.presentation_resources.watch,"layout_path":w.presentation_resources.layout_path,"theme_path":w.presentation_resources.theme_path,"package_path":w.presentation_resources.package_path}),
    });
    for (field, label, json) in [
        ("layout", "Layout source", serde_json::json!(w.layout_text)),
        ("theme", "Theme source", serde_json::json!(w.theme_text)),
        (
            "watch",
            "Reload external edits automatically",
            serde_json::json!(w.presentation_resources.watch),
        ),
    ] {
        elements.push(SemanticElement {
            id: format!("workspace.presentation.{field}"), role: "presentation-resource-field".into(),
            label: label.into(), value: Some(json.as_str().map(str::to_owned).unwrap_or_else(|| json.to_string())),
            state: "ready".into(), commands: vec![], relationships: vec!["workspace.presentation.diagnostics".into()],
            data: serde_json::json!({"setter":"presentation-resource","field":field,"editable":true,"json":json,"value_type":if field == "watch" {"boolean"} else {"string"},"multiline":field != "watch","presentation_only":true,"classes":if field == "watch" {vec!["toggle"]} else {vec![]}}),
        });
    }
    elements
}

/// Shared setter for native and user-composed source editors.
pub fn set_value(w: &mut Workspace, field: &str, value: Value) -> Result<()> {
    match field {
        "layout" | "theme" => {
            let source = value
                .as_str()
                .ok_or_else(|| invalid("Expected presentation source text"))?;
            if source.len() as u64 > MAX_FILE_BYTES {
                return Err(invalid("Presentation source exceeds the 4 MiB limit"));
            }
            if field == "layout" {
                w.layout_text = source.into();
            } else {
                w.theme_text = source.into();
            }
        }
        "watch" => {
            w.presentation_resources.watch = value
                .as_bool()
                .ok_or_else(|| invalid("Expected an automatic reload toggle"))?;
            crate::settings::save_live_application(w);
        }
        _ => {
            return Err(invalid(format!(
                "Unknown presentation resource field {field}"
            )));
        }
    }
    Ok(())
}

/// Resource tools use the same presentation commands exposed in the catalog.
pub fn show(ui: &mut egui::Ui, w: &mut Workspace) {
    egui::CollapsingHeader::new("Local files and portable packages").default_open(true).show(ui, |ui| {
        ui.horizontal_wrapped(|ui| {
            for (command, label) in [
                ("presentation.apply-source", "Apply source"), ("presentation.reload-files", "Reload external files"),
                ("presentation.save-project", "Save to project"), ("presentation.save-personal", "Save as my default"), ("presentation.reset", "Reset · F9"),
                ("presentation.import-layout", "Import layout"), ("presentation.import-theme", "Import theme"),
                ("presentation.import-package", "Import package"), ("presentation.export-package", "Export package"),
            ] {
                let enabled = command != "presentation.save-project" || w.project.is_some();
                if crate::presentation_controls::button(ui, &format!("workspace.{command}"), "presentation-resource-action", &["compact"], label, enabled, false).clicked() { action(w, command); }
            }
        });
        let props = crate::presentation_controls::properties("workspace.presentation.watch", "presentation-resource-field", &["toggle"], "ready");
        if props.get("display").is_none_or(|value| value != "none") {
            ui.scope(|ui| {
                crate::presentation_controls::apply_properties(ui, &props);
                if ui.checkbox(&mut w.presentation_resources.watch, "Reload external edits automatically").changed() {
                    crate::settings::save_live_application(w);
                }
            });
        }
        for (label, path) in [
            ("Layout", &w.presentation_resources.layout_path), ("Theme", &w.presentation_resources.theme_path),
            ("Package", &w.presentation_resources.package_path),
        ] {
            if let Some(path) = path { ui.small(format!("{label}: {}", path.display())); }
        }
        if let Some(message) = &w.presentation_resources.diagnostic {
            ui.label(message);
        }
        if let Some(notice) = crate::theme_library::fallback_notice(w) {
            crate::presentation_controls::label(ui, &notice.id, &notice.role, &[], &notice.label, &notice.state);
            ui.horizontal_wrapped(|ui| {
                for (command, label) in [
                    ("content-repositories.download-themes", "Download themes pack"),
                    ("content-repositories.show-themes", "View themes pack"),
                ] {
                    if crate::presentation_controls::button(ui, &format!("workspace.{command}"), "presentation-resource-action", &["compact"], label, true, false).clicked() {
                        crate::ui::action(w, command);
                    }
                }
            });
        }
        ui.small("JSON extends composes local resources; CSS @import accepts quoted local paths. Exports flatten inherited resources into a portable package. Invalid changes retain the last usable presentation.");
    });
}
