mod appearances_ui;
mod assembly_inspection_ui;
mod assembly_motion_ui;
mod background;
mod body_filter_ui;
mod cad_ui;
mod component_pattern_ui;
mod configuration_ui;
mod contours_ui;
mod curve_pattern_ui;
mod direct_edit_ui;
mod drawing_table_ui;
mod drawing_ui;
mod exploded_views_ui;
mod fabrication_ui;
mod feature_order_ui;
mod graph_ui;
mod history;
mod holes_ui;
mod inspection_ui;
mod mate_ui;
mod mold_ui;
mod navigation;
mod operation_editor_ui;
mod operation_preview;
mod placement_pattern_ui;
mod presentation_composition;
mod presentation_controls;
mod presentation_resources;
mod reference_curves_ui;
mod reference_repair_ui;
mod reference_ui;
mod render_appearance;
mod rib_ui;
mod sections_ui;
mod selection_ui;
mod settings;
mod settings_preferences;
mod settings_ui;
mod sketch;
mod sketch_analysis_ui;
mod sketch_blocks_ui;
mod sketch_dimensions_ui;
mod sketch_edit_ui;
mod sketch_equation_ui;
mod sketch_handles;
mod sketch_relations_ui;
mod sketch_ui;
mod split_ui;
mod surface_ui;
mod texture_assets;
mod theme_library;
mod ui;
mod viewport;
mod worker;
mod workspace_controls;
use bevy::{
    asset::RenderAssetUsages,
    camera::{RenderTarget, visibility::RenderLayers},
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat, TextureUsages},
};
use bevy_egui::{
    EguiGlobalSettings, EguiPlugin, EguiPrimaryContextPass, EguiTextureHandle, EguiUserTextures,
    PrimaryEguiContext,
};
use cad_core::{
    Document, Session,
    persistence::{FileStore, ProjectStore},
    presentation::{LayoutDocument, PresentationState, Theme, default_layout, default_theme},
    protocol::*,
};
use std::{collections::BTreeMap, path::PathBuf, time::Instant};
const BACKEND: &str = "monstertruck/0.4.1+fc2306852a46;adapter/29;solver/tcad-native-lm/13";
#[derive(Resource)]
pub struct Workspace {
    pub session: Session,
    pub view: PresentationState,
    pub project: Option<PathBuf>,
    pub layout: LayoutDocument,
    pub theme: Theme,
    pub presentation_resources: crate::presentation_resources::State,
    pub application_preferences: settings_preferences::Preferences,
    pub settings: Option<settings::State>,
    pub reference_repair: Option<reference_repair_ui::State>,
    pub assembly_motion: Option<assembly_motion_ui::State>,
    pub sketch_analysis: sketch_analysis_ui::State,
    pub jobs: background::Jobs,
    pub worker: worker::WorkerClient,
    pub exporter: worker::WorkerClient,
    pub requested: Option<RequestIdentity>,
    pub requested_preview: Option<operation_preview::RequestKey>,
    pub preview_error: Option<(operation_preview::RequestKey, String)>,
    pub changed: Instant,
    pub status: String,
    pub appearance_diagnostics: Vec<String>,
    pub appearance_validation_key: String,
    pub dialog: Option<String>,
    pub json: String,
    pub dialog_title: String,
    pub fields: BTreeMap<String, String>,
    pub layout_text: String,
    pub theme_text: String,
    pub source_text: String,
    pub selected: Option<viewport::Selection>,
    /// View-only preselection; authored edge sets belong to their feature draft.
    pub geometry_selection: Vec<viewport::Selection>,
    /// Full occurrence anchor for edge selection; a leaf can repeat in subassemblies.
    pub edge_selection_path: Option<Vec<cad_core::Id>>,
    pub selected_feature_path: Option<Vec<String>>,
    pub selection_popup: Option<selection_ui::ActionPopup>,
    pub context_selection: Option<cad_core::selection_actions::SelectionTarget>,
    pub candidates: Vec<viewport::Selection>,
    pub candidate_index: usize,
    pub rendered_key: String,
    pub viewport_size: [u32; 2],
    pub git_text: String,
    pub commit_message: String,
    pub branch_name: String,
    pub before_isolation: Option<PresentationState>,
    pub sketch: Option<sketch::EditingSketch>,
    pub active_plane: Option<String>,
    pub manager_tree: bool,
    pub command_tab: String,
    pub tool_checkpoint: Option<Session>,
    pub confirm_requested: bool,
    pub after_confirm: Option<String>,
    pub recovery_path: PathBuf,
    pub last_recovery: String,
    pub last_recovery_write: Instant,
    pub recovery_draft: Option<Document>,
    pub saved_source: String,
    pub gesture: Option<navigation::Capture>,
    pub navigation_input: navigation::Runtime,
    pub peek: bool,
    pub review_text: String,
    pub compare: Option<Evaluation>,
    pub compare_document: Option<Document>,
    pub compare_worker: worker::WorkerClient,
}
impl Workspace {
    fn new() -> Self {
        let mut project = None;
        let mut status = "Select a plane, then Sketch to begin".to_string();
        let mut doc = Document::blank_part("Part1");
        let reset = std::env::args().any(|a| a == "--reset-ui");
        let loaded_preferences = settings_preferences::load(reset);
        if let Some(diagnostic) = loaded_preferences.diagnostic {
            status = diagnostic;
        } else if loaded_preferences.migrated_navigation {
            status = "Existing navigation preferences imported into application settings".into();
        }
        let application_preferences = loaded_preferences.preferences;
        doc.settings = application_preferences.default_model.clone();
        for part in doc.parts.values_mut() {
            part.finish = doc.settings.default_new_part_finish.clone();
        }
        for arg in std::env::args().skip(1).filter(|a| !a.starts_with("--")) {
            let path = PathBuf::from(arg);
            match FileStore.load(&path) {
                Ok(d) => {
                    doc = d;
                    project = Some(path)
                }
                Err(e) => status = e.to_string(),
            }
        }
        let mut view = PresentationState {
            active_part: doc.parts.keys().next().cloned(),
            ..Default::default()
        };
        let root = project
            .as_ref()
            .map(|p| cad_core::persistence::project_root(p));
        if !reset
            && let Some(saved) = root
                .as_ref()
                .and_then(|r| std::fs::read(r.join("ui/workspace.json")).ok())
                .and_then(|data| serde_json::from_slice::<PresentationState>(&data).ok())
        {
            view = saved;
        }
        view.navigation = application_preferences.navigation.clone();
        view.ui_scale = application_preferences.ui_scale;
        if reset {
            view.roll = 0.;
        }
        navigation::sanitize(&mut view);
        let layout = default_layout();
        let theme = default_theme();
        let saved_source = doc.fingerprint();
        let recovery_path = recovery_directory().join(format!("{}.json", doc.id));
        let recovery_draft = std::fs::read(&recovery_path)
            .ok()
            .and_then(|data| serde_json::from_slice::<serde_json::Value>(&data).ok())
            .and_then(|v| {
                let saved = v.get("source")?.as_str()?;
                if saved != doc.fingerprint() {
                    return None;
                }
                let source = v
                    .get("draft")
                    .filter(|source| !source.is_null())
                    .or_else(|| v.get("committed"))?;
                let recovered: Document = serde_json::from_value(source.clone()).ok()?;
                recovered.validate().ok()?;
                (recovered.fingerprint() != doc.fingerprint()).then_some(recovered)
            });
        let layout_text = serde_json::to_string_pretty(&layout).unwrap_or_default();
        let theme_text = serde_json::to_string_pretty(&theme).unwrap_or_default();
        let mut workspace = Self {
            session: Session::new(doc, BACKEND),
            view,
            project,
            layout,
            theme,
            presentation_resources: crate::presentation_resources::State::default(),
            application_preferences,
            settings: None,
            reference_repair: None,
            assembly_motion: None,
            sketch_analysis: sketch_analysis_ui::State::default(),
            jobs: background::Jobs::new(),
            worker: worker::WorkerClient::new(),
            exporter: worker::WorkerClient::new(),
            requested: None,
            requested_preview: None,
            preview_error: None,
            changed: Instant::now(),
            status,
            appearance_diagnostics: vec![],
            appearance_validation_key: String::new(),
            dialog: None,
            json: String::new(),
            dialog_title: String::new(),
            fields: BTreeMap::new(),
            layout_text,
            theme_text,
            source_text: String::new(),
            selected: None,
            geometry_selection: vec![],
            edge_selection_path: None,
            selected_feature_path: None,
            selection_popup: None,
            context_selection: None,
            candidates: vec![],
            candidate_index: 0,
            rendered_key: String::new(),
            viewport_size: [1000, 800],
            git_text: String::new(),
            commit_message: String::new(),
            branch_name: String::new(),
            before_isolation: None,
            sketch: None,
            active_plane: None,
            manager_tree: true,
            command_tab: "Features".into(),
            tool_checkpoint: None,
            confirm_requested: false,
            after_confirm: None,
            recovery_path,
            last_recovery: String::new(),
            last_recovery_write: Instant::now(),
            recovery_draft,
            saved_source,
            gesture: None,
            navigation_input: navigation::Runtime::default(),
            peek: false,
            review_text: String::new(),
            compare: None,
            compare_document: None,
            compare_worker: worker::WorkerClient::new(),
        };
        crate::presentation_resources::initialize(&mut workspace, reset);
        if !reset {
            workspace.presentation_resources.watch =
                workspace.application_preferences.watch_presentation_files;
        }
        workspace
    }
    pub fn root(&self) -> PathBuf {
        self.project
            .as_ref()
            .map(|p| cad_core::persistence::project_root(p))
            .unwrap_or_else(|| {
                recovery_directory()
                    .join("unsaved")
                    .join(&self.session.committed.id)
            })
    }
    pub fn tolerance(&self) -> f64 {
        self.session.visible().settings.evaluation_tolerance_mm
    }
    pub fn dispatch(&mut self, c: cad_core::commands::Command) {
        if self.settings.is_some() {
            self.status = "Close Settings before editing the model".into();
            return;
        }
        history::latest(self);
        self.selection_popup = None;
        self.confirm_requested = false;
        let before = self.sketch.as_ref().map(|_| self.session.visible().clone());
        match self.session.dispatch(c) {
            Ok(()) => {
                self.changed = Instant::now();
                self.status = "Draft changed — evaluating preview".into();
                if let Some(before) = before {
                    sketch::reconcile(self, &before);
                }
            }
            Err(e) => self.status = e.to_string(),
        }
    }
    pub fn apply(&mut self) {
        if !assembly_motion_ui::ready_to_confirm(self) {
            return;
        }
        if !operation_editor_ui::current_source_is_valid(self) {
            self.status = "Correct the current operation fields before applying".into();
            self.confirm_requested = false;
            return;
        }
        if !reference_repair_ui::current_source_is_valid(self) {
            self.status = "Preview the current explicit reference repair before applying".into();
            self.confirm_requested = false;
            return;
        }

        if self.dialog.as_deref() == Some("feature")
            && !serde_json::from_str::<cad_core::Feature>(&self.json).is_ok_and(|source| {
                self.fields.get("feature-editor-id") == Some(&source.id)
                    && self
                        .view
                        .active_part
                        .as_ref()
                        .and_then(|owner| self.session.visible().parts.get(owner))
                        .and_then(|part| part.features.iter().find(|f| f.id == source.id))
                        .is_some_and(|current| current == &source)
            })
        {
            self.status = "The current feature source must be validated before applying".into();
            self.confirm_requested = false;
            return;
        }
        if self.fields.contains_key("appearance-tool") && render_appearance::resources_pending(self)
        {
            self.confirm_requested = true;
            self.status = "Waiting for the current texture resources to finish loading".into();
            return;
        }
        if self.fields.contains_key("appearance-tool")
            && !appearances_ui::current_source_is_valid(self)
        {
            self.status =
                "Correct the current visual finish or lighting source before applying".into();
            self.confirm_requested = false;
            return;
        }
        if self.fields.contains_key("drawing-source-invalid")
            || self.dialog.as_deref() == Some("drawings")
                && !drawing_ui::current_source_is_valid(self)
            || self.fields.contains_key("exploded-view-source-invalid")
            || self
                .fields
                .contains_key("assembly-inspection-source-invalid")
            || self.fields.contains_key("assembly-inspection-editor")
                && !assembly_inspection_ui::current_source_is_valid(self)
            || self.fields.contains_key("exploded-view-editor")
                && !exploded_views_ui::current_source_is_valid(self)
            || self.fields.contains_key("feature-source-invalid")
            || self.fields.contains_key("mate-source-invalid")
            || self.fields.contains_key("component-pattern-source-invalid")
            || self.fields.contains_key("component-pattern-editor")
                && !component_pattern_ui::current_source_is_valid(self)
            || self.fields.contains_key("configuration-source-invalid")
            || self.fields.contains_key("configuration-editor")
                && !configuration_ui::current_source_is_valid(self)
        {
            self.status = "Correct the current source definition before applying".into();
            self.confirm_requested = false;
            return;
        }
        if self.sketch.is_some() && !sketch::finish(self) {
            return;
        }
        let editing_edges = selection_ui::edge_feature(self).is_some();
        match self.session.apply() {
            Ok(()) => {
                assembly_motion_ui::cancel(self);
                if editing_edges {
                    self.selected = None;
                    self.geometry_selection.clear();
                    self.candidates.clear();
                }
                reference_repair_ui::reset(self);
                self.edge_selection_path = None;
                self.tool_checkpoint = None;
                self.dialog = None;
                self.manager_tree = true;
                self.confirm_requested = false;
                self.status = "Applied as one undoable change".into();
                self.requested = Some(self.session.identity());
                self.fields.clear();
                if let Some(feature) = self.after_confirm.take() {
                    ui::feature_preset(self, &feature);
                }
            }
            Err(e) => self.status = e.to_string(),
        }
    }
    pub fn cancel(&mut self) {
        assembly_motion_ui::cancel(self);
        reference_repair_ui::reset(self);
        navigation::stop(self);
        assembly_inspection_ui::cancel(self);
        exploded_views_ui::cancel(self);
        history::latest(self);
        self.selection_popup = None;
        self.context_selection = None;
        self.selected_feature_path = None;
        self.geometry_selection.clear();
        self.edge_selection_path = None;
        self.tool_checkpoint = None;
        self.confirm_requested = false;
        self.after_confirm = None;
        self.sketch = None;
        if matches!(
            self.dialog.as_deref(),
            Some("sketch" | "feature" | "reference-repair" | "feature-order" | "assembly-motion")
        ) {
            self.dialog = None;
        }
        self.session.cancel();
        self.worker.cancel();
        self.requested = self.session.evaluation.as_ref().map(|e| e.identity.clone());
        self.fields.clear();
        self.manager_tree = true;
        self.status = "Draft discarded".into();
    }
    pub fn replace(&mut self, d: Document, path: Option<PathBuf>) {
        settings::cancel(self);
        self.cancel();
        self.session = Session::new(d, BACKEND);
        self.project = path;
        let navigation = self.view.navigation.clone();
        self.view = PresentationState {
            navigation,
            ui_scale: self.application_preferences.ui_scale,
            ..Default::default()
        };
        self.view.active_part = self.session.committed.parts.keys().next().cloned();
        self.requested = None;
        self.changed = Instant::now();
        self.selected = None;
        self.selection_popup = None;
        self.dialog = None;
        self.context_selection = None;
        self.selected_feature_path = None;
        self.active_plane = None;
        self.command_tab = "Features".into();
        self.manager_tree = true;
        self.compare = None;
        self.compare_document = None;
        self.appearance_diagnostics.clear();
        self.appearance_validation_key.clear();
        self.saved_source = self.session.committed.fingerprint();
        self.recovery_path =
            recovery_directory().join(format!("{}.json", self.session.committed.id));
        self.last_recovery.clear();
        self.last_recovery_write = Instant::now();
    }
    pub fn isolate(&mut self) {
        navigation::stop(self);
        if self.view.isolation {
            if let Some(v) = self.before_isolation.take() {
                let navigation = self.view.navigation.clone();
                let ui_scale = self.view.ui_scale;
                self.view = v;
                self.view.navigation = navigation;
                self.view.ui_scale = ui_scale;
            }
        } else {
            self.before_isolation = Some(self.view.clone());
            self.view.isolation = true;
        }
        self.rendered_key.clear();
    }
    pub fn recover(&mut self) {
        if let Some(document) = self.recovery_draft.take() {
            self.dispatch(cad_core::commands::Command::ReplaceDocument { document });
        }
    }
    pub fn open_editor(&mut self, name: &str) {
        navigation::stop(self);
        self.selection_popup = None;
        self.dialog = Some(name.into());
        self.manager_tree = false;
    }
    pub fn editing_manager(&self) -> bool {
        if self.dialog.as_deref() == Some("appearances")
            && presentation_composition::appearance_panel_hosted(self)
        {
            return false;
        }
        (self.dialog.is_some() || self.recovery_draft.is_some()) && !self.manager_tree
    }
    pub fn start_tool(&mut self) {
        navigation::stop(self);
        exploded_views_ui::collapse_for_modeling(self);
        history::latest(self);
        self.selection_popup = None;
        self.context_selection = None;
        self.selected_feature_path = None;
        if self.tool_checkpoint.is_none() {
            self.tool_checkpoint = Some(self.session.clone());
        }
        self.confirm_requested = false;
    }
    pub fn cancel_tool(&mut self) {
        assembly_motion_ui::cancel(self);
        reference_repair_ui::reset(self);
        navigation::stop(self);
        assembly_inspection_ui::cancel(self);
        exploded_views_ui::cancel(self);
        self.selection_popup = None;
        self.context_selection = None;
        self.selected_feature_path = None;
        if let Some(checkpoint) = self.tool_checkpoint.take() {
            self.worker.cancel();
            self.session = checkpoint;
            self.requested = None;
        }
        self.sketch = None;
        self.dialog = None;
        self.manager_tree = true;
        self.confirm_requested = false;
        self.after_confirm = None;
        self.gesture = None;
        self.selected = None;
        self.geometry_selection.clear();
        self.edge_selection_path = None;
        self.candidates.clear();
        self.fields.retain(|key, _| {
            !key.starts_with("appearance-")
                && !key.starts_with("lighting-")
                && !key.starts_with("drawing-editor:")
                && key != "drawing-source-invalid"
        });
        self.fields.remove("smart-dimension-mode");
        self.fields.remove("component-pattern-editor");
        self.fields.remove("component-pattern-source-invalid");
        self.fields.remove("configuration-owner");
        self.fields.remove("configuration-editor");
        self.fields.remove("configuration-source-invalid");
        self.fields.remove("configuration-equations");
        self.fields.remove("mate-editor");
        self.fields.remove("mate-source-invalid");
        self.fields.remove("feature-source-invalid");
        self.fields.remove("feature-source-error");
        self.fields.remove("feature-editor-id");
        self.fields.remove("feature-editor-generation");
        self.fields.remove("feature-order-owner");
        self.fields.remove("feature-order-feature");
        if self.view.active_node.as_ref().is_some_and(|id| {
            !self
                .session
                .visible()
                .parts
                .values()
                .any(|p| p.features.iter().any(|f| &f.id == id))
        }) {
            self.view.active_node = None;
        }
        self.rendered_key.clear();
        self.status = "Operation cancelled".into();
    }
    pub fn confirm_tool(&mut self) {
        if !assembly_motion_ui::ready_to_confirm(self) {
            return;
        }
        if !operation_editor_ui::current_source_is_valid(self) {
            self.status = "Correct the current operation fields before applying".into();
            self.confirm_requested = false;
            return;
        }
        if !reference_repair_ui::current_source_is_valid(self) {
            self.status = "Preview the current explicit reference repair before applying".into();
            self.confirm_requested = false;
            return;
        }

        if self.fields.contains_key("appearance-tool") && render_appearance::resources_pending(self)
        {
            self.confirm_requested = true;
            self.status = "Waiting for the current texture resources to finish loading".into();
            return;
        }
        if self.fields.contains_key("appearance-tool")
            && !appearances_ui::current_source_is_valid(self)
        {
            self.status =
                "Correct the current visual finish or lighting source before applying".into();
            return;
        }
        if self
            .fields
            .contains_key("assembly-inspection-source-invalid")
            || self.fields.contains_key("assembly-inspection-editor")
                && !assembly_inspection_ui::current_source_is_valid(self)
        {
            self.status = "Correct the current assembly inspection source before applying".into();
            return;
        }
        if self.fields.contains_key("drawing-source-invalid")
            || self.dialog.as_deref() == Some("drawings")
                && !drawing_ui::current_source_is_valid(self)
        {
            self.status = "Correct the current drawing source before applying".into();
            return;
        }
        if self.fields.contains_key("exploded-view-source-invalid")
            || self.fields.contains_key("exploded-view-editor")
                && !exploded_views_ui::current_source_is_valid(self)
        {
            self.status = "Correct the current exploded view source before applying".into();
            return;
        }
        holes_ui::remember_positions(self);
        rib_ui::remember_contours(self);
        contours_ui::remember_contours(self);
        curve_pattern_ui::remember_path(self);
        placement_pattern_ui::remember_points(self);
        if self.fields.contains_key("component-pattern-source-invalid")
            || self.fields.contains_key("component-pattern-editor")
                && !component_pattern_ui::current_source_is_valid(self)
        {
            self.status = "Correct the current component pattern source before applying".into();
            return;
        }
        if self.fields.contains_key("configuration-source-invalid")
            || self.fields.contains_key("configuration-editor")
                && !configuration_ui::current_source_is_valid(self)
        {
            self.status = "Correct the current configuration source before applying".into();
            return;
        }
        if self.fields.contains_key("feature-source-invalid") {
            self.status = "Correct the current feature source before applying".into();
            return;
        }
        if self.fields.contains_key("mate-source-invalid")
            || self.fields.contains_key("mate-editor") && !mate_ui::current_source_is_valid(self)
        {
            self.status = "Correct the current mate source before applying".into();
            return;
        }

        if (self.assembly_motion.is_some()
            || self.fields.contains_key("exploded-view-editor")
            || self.fields.contains_key("assembly-inspection-editor")
            || self.fields.contains_key("appearance-tool"))
            && self.session.draft.is_none()
        {
            assembly_motion_ui::cancel(self);
            self.tool_checkpoint = None;
            self.dialog = None;
            self.manager_tree = true;
            self.fields.clear();
            self.confirm_requested = false;
            self.rendered_key.clear();
            self.status = "Editor closed; source unchanged".into();
            return;
        }
        if self.sketch.is_none() && self.session.draft.is_none() {
            self.status = "No operation to confirm".into();
            return;
        }
        if self.sketch.is_some() && !sketch::finish(self) {
            return;
        }
        let valid = self.session.draft.as_ref().is_some_and(|d| {
            !d.pending
                && d.error.is_none()
                && d.evaluation
                    .as_ref()
                    .is_some_and(|e| e.valid && e.identity == self.session.identity())
        });
        if valid {
            self.apply();
        } else if self
            .session
            .draft
            .as_ref()
            .is_some_and(|d| d.error.is_some())
        {
            self.status = "Resolve the feature error before confirming".into();
        } else {
            self.confirm_requested = true;
            self.status = "Confirming after geometry finishes rebuilding".into();
        }
    }
}
fn recovery_directory() -> PathBuf {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    if cfg!(target_os = "macos") {
        home.join("Library/Application Support/TCAD/recovery")
    } else if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or(home)
            .join("TCAD/recovery")
    } else {
        home.join(".local/share/tcad/recovery")
    }
}
#[derive(Resource)]
pub struct ViewImage {
    pub image: Handle<Image>,
    pub texture: bevy_egui::egui::TextureId,
}
#[derive(Component)]
pub struct CadCamera;
pub enum CadMaterial {
    Standard(Handle<StandardMaterial>),
    Finish(Handle<render_appearance::FinishMaterial>),
}
#[derive(Component)]
pub struct CadBody {
    pub mesh: Handle<Mesh>,
    pub material: CadMaterial,
}
fn main() {
    App::new()
        .insert_resource(Workspace::new())
        .insert_resource(EguiGlobalSettings {
            auto_create_primary_context: false,
            ..default()
        })
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "TCAD · Mechanical design".into(),
                resolution: (1440, 960).into(),
                ..default()
            }),
            ..default()
        }))
        .add_plugins(render_appearance::FinishRenderPlugin)
        .add_plugins(EguiPlugin::default())
        .add_systems(Startup, setup)
        .add_systems(
            Update,
            (
                navigation::collect_input,
                poll_workers,
                render_appearance::poll_assets,
                render_appearance::lighting,
                viewport::sync_scene,
            )
                .chain(),
        )
        .add_systems(EguiPrimaryContextPass, ui::workspace_ui)
        .add_systems(
            PostUpdate,
            viewport::camera
                .after(bevy_egui::EguiPostUpdateSet::EndPass)
                .before(bevy::transform::TransformSystems::Propagate)
                .before(bevy::camera::CameraUpdateSystems),
        )
        .run();
}
fn setup(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut textures: ResMut<EguiUserTextures>,
    mut appearance: ResMut<render_appearance::Runtime>,
) {
    let size = Extent3d {
        width: 1000,
        height: 800,
        depth_or_array_layers: 1,
    };
    let mut image = Image::new_fill(
        size,
        TextureDimension::D2,
        &[233, 234, 236, 255],
        TextureFormat::Bgra8UnormSrgb,
        RenderAssetUsages::default(),
    );
    image.texture_descriptor.usage =
        TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST | TextureUsages::RENDER_ATTACHMENT;
    let image = images.add(image);
    let texture = textures.add_image(EguiTextureHandle::Strong(image.clone()));
    commands.insert_resource(ViewImage {
        image: image.clone(),
        texture,
    });
    commands.spawn((
        Camera3d::default(),
        Camera {
            order: -1,
            clear_color: ClearColorConfig::Custom(Color::srgb(0.82, 0.84, 0.86)),
            ..default()
        },
        RenderTarget::Image(image.into()),
        Transform::from_xyz(3., 2., 4.).looking_at(Vec3::new(0., 0.35, 0.), Vec3::Y),
        RenderLayers::layer(1),
        CadCamera,
        render_appearance::camera_components(),
    ));
    commands.spawn((Camera2d, PrimaryEguiContext));
    render_appearance::setup(&mut commands, &mut images, &mut appearance);
}
fn poll_workers(mut w: ResMut<Workspace>) {
    sketch_analysis_ui::poll(&mut w);
    assembly_motion_ui::poll(&mut w);
    for (ok, message) in w.jobs.drain() {
        if ok {
            w.git_text = message;
            w.status = "Git operation completed".into();
        } else {
            w.status = message;
        }
    }
    let replies = w.worker.drain();
    for response in replies {
        match response {
            WorkerResponse::Evaluation { result } => {
                let ms = result.elapsed_ms;
                let valid = result.valid;
                if operation_preview::accepts(&w, &result) && w.session.accept(result) {
                    w.preview_error = None;
                    if w.view.exploded_view.is_some() {
                        exploded_views_ui::clear_selection(&mut w);
                    }
                    sketch::accept_evaluation(&mut w);
                    w.status = if let Some(error) = w.sketch.as_ref().and_then(|s| s.error.as_ref())
                    {
                        format!("{error} · last valid body preview retained")
                    } else if valid {
                        format!("Geometry valid · {ms} ms")
                    } else {
                        w.session
                            .draft
                            .as_ref()
                            .and_then(|d| d.error.clone())
                            .unwrap_or_else(|| "Geometry validation failed".into())
                    };
                    if w.confirm_requested {
                        if w.session.draft.as_ref().is_some_and(|d| d.error.is_none()) {
                            w.apply();
                        } else {
                            w.confirm_requested = false;
                        }
                    }
                }
            }
            WorkerResponse::Error { identity, message } => {
                if let Some(i) = identity {
                    if i == w.session.identity() {
                        w.preview_error =
                            operation_preview::request_key(&w).map(|key| (key, message.clone()));
                        w.session.fail(&i, message.clone());
                        w.confirm_requested = false;
                        w.status = message;
                    }
                } else {
                    w.status = message;
                }
            }
            _ => {}
        }
    }
    // Keep completed imports queued while Settings owns a checkpoint; never merge unrelated source into it.
    let exporter_replies = if w.settings.is_none() {
        w.exporter.drain()
    } else {
        vec![]
    };
    for response in exporter_replies {
        match response {
            WorkerResponse::Exported { path, .. } => w.status = format!("Wrote {path}"),
            WorkerResponse::Imported { identity, document } => {
                if identity == w.session.identity() {
                    w.dispatch(cad_core::commands::Command::ReplaceDocument { document });
                    w.status = "Imported STEP structure; inspect and Apply".into();
                } else {
                    w.status =
                        "STEP import result is stale; import again against the current draft"
                            .into();
                }
            }
            WorkerResponse::Error { message, .. } => w.status = message,
            _ => {}
        }
    }
    for response in w.compare_worker.drain() {
        match response {
            WorkerResponse::Evaluation { result } => {
                if w.compare_document
                    .as_ref()
                    .is_some_and(|d| d.fingerprint() == result.identity.source)
                {
                    w.compare = Some(result);
                    w.rendered_key.clear();
                }
            }
            WorkerResponse::Error { message, .. } => w.status = message,
            _ => {}
        }
    }
    let identity = w.session.identity();
    let preview_key = operation_preview::request_key(&w);
    history::reconcile(&mut w);
    if (w.requested.as_ref() != Some(&identity) || w.requested_preview != preview_key)
        && w.changed.elapsed().as_millis() > 160
    {
        w.worker.submit(WorkerRequest::Evaluate {
            identity: identity.clone(),
            document: w.session.visible().clone(),
            project_root: w.root().to_string_lossy().into(),
            tolerance: w.tolerance(),
            operation_preview: preview_key
                .as_ref()
                .map(|key| Box::new(operation_preview::request(&w, key))),
        });
        w.requested = Some(identity);
        w.requested_preview = preview_key;
        w.preview_error = None;
    }
    settings::poll(&mut w);
    if w.application_preferences.recovery_enabled
        && w.changed.elapsed().as_millis() > 250
        && w.last_recovery_write.elapsed().as_secs()
            >= w.application_preferences.recovery_interval_seconds
    {
        w.last_recovery_write = Instant::now();
        let recovery = serde_json::json!({"source":w.saved_source,"resource_root":w.root(),"committed":w.session.committed,"draft":w.session.draft.as_ref().map(|d|&d.document)});
        let data = serde_json::to_string(&recovery).unwrap_or_default();
        if data != w.last_recovery {
            let path = w.recovery_path.clone();
            let bytes = data.as_bytes().to_vec();
            w.jobs
                .submit(background::Job::Recovery { path, data: bytes });
            w.last_recovery = data;
        }
    }
}
