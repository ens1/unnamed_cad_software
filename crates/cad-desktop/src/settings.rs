//! Settings transactions share model validation while personal preferences stay outside CAD source.
use crate::{Workspace, navigation, settings_preferences};
use cad_core::{CadError, Document, Result, Session, commands::Command};

pub struct State {
    pub category: String,
    pub search: String,
    pub application: settings_preferences::Preferences,
    pub model_editable: bool,
    pub pending: bool,
    pub diagnostic: Option<String>,
    checkpoint: Option<Session>,
    applied_application: settings_preferences::Preferences,
    close_after_apply: bool,
    open_presentation_after_apply: bool,
    theme_checkpoint: crate::theme_library::Snapshot,
    theme_changed: bool,
}

pub fn open(w: &mut Workspace, category: Option<&str>) {
    if crate::drawing_interaction_ui::active(w) {
        crate::drawing_interaction_ui::escape(w);
    }
    crate::assembly_motion_ui::cancel_drag(w);
    navigation::stop(w);
    w.selection_popup = None;
    if let Some(state) = &mut w.settings {
        if let Some(category) = category {
            state.category = category.into();
        }
        return;
    }
    // Opening personal preferences during a feature operation never takes ownership of its draft.
    let model_editable =
        w.session.draft.is_none() && w.tool_checkpoint.is_none() && w.sketch.is_none();
    let mut application = w.application_preferences.clone();
    application.ui_scale = w.view.ui_scale;
    application.navigation = w.view.navigation.clone();
    application.watch_presentation_files = w.presentation_resources.watch;
    w.settings = Some(State {
        category: category
            .unwrap_or(if model_editable {
                "model.units"
            } else {
                "application.interface"
            })
            .into(),
        search: String::new(),
        applied_application: application.clone(),
        application,
        model_editable,
        pending: false,
        diagnostic: None,
        checkpoint: model_editable.then(|| w.session.clone()),
        close_after_apply: false,
        open_presentation_after_apply: false,
        theme_checkpoint: crate::theme_library::capture(w),
        theme_changed: false,
    });
}

fn require_model(w: &Workspace) -> Result<()> {
    if !w
        .settings
        .as_ref()
        .is_some_and(|state| state.model_editable)
    {
        return Err(CadError::Invalid(
            "Finish or cancel the active modeling operation before changing model settings".into(),
        ));
    }
    Ok(())
}

pub fn set_model(w: &mut Workspace, settings: cad_core::settings::ModelSettings) -> Result<()> {
    require_model(w)?;
    settings.validate(w.session.visible())?;
    if w.session.visible().settings != settings {
        w.session.dispatch(Command::SetModelSettings { settings })?;
        model_changed(w);
    }
    Ok(())
}

pub fn set_lighting(w: &mut Workspace, lighting: cad_core::appearance::Lighting) -> Result<()> {
    require_model(w)?;
    cad_core::appearance::validate_lighting(&lighting)?;
    if w.session.visible().lighting != lighting {
        w.session.dispatch(Command::SetLighting { lighting })?;
        model_changed(w);
    }
    Ok(())
}

fn model_changed(w: &mut Workspace) {
    if let Some(state) = &mut w.settings {
        state.pending = false;
        state.diagnostic = None;
    }
    w.changed = std::time::Instant::now();
    w.status = "Settings changed — evaluating model preview".into();
}

pub fn set_application(
    w: &mut Workspace,
    preferences: settings_preferences::Preferences,
) -> Result<()> {
    preferences.validate()?;
    let state = w
        .settings
        .as_mut()
        .ok_or_else(|| CadError::Invalid("Open Settings to edit preferences".into()))?;
    state.application = preferences.clone();
    state.pending = false;
    state.diagnostic = None;
    preview_application(w, &preferences);
    Ok(())
}

fn preview_application(w: &mut Workspace, preferences: &settings_preferences::Preferences) {
    navigation::stop(w);
    w.view.ui_scale = preferences.ui_scale;
    w.view.navigation = preferences.navigation.clone();
    w.presentation_resources.watch = preferences.watch_presentation_files;
}

pub fn set_theme(w: &mut Workspace, identity: &str) -> Result<()> {
    if w.settings.is_none() {
        open(w, Some("application.themes"));
    }
    if w.settings.as_ref().is_some_and(|state| state.pending) {
        return Err(CadError::Invalid(
            "Wait for settings validation before changing themes".into(),
        ));
    }
    crate::theme_library::select(w, identity)?;
    mark_theme_changed(w);
    Ok(())
}

/// Recover visible controls without committing or discarding a model settings draft.
pub fn recover_presentation(w: &mut Workspace) {
    if let Some(state) = &mut w.settings {
        state.pending = false;
        state.category = "application.themes".into();
        state.search.clear();
    }
    match set_theme(w, "classic") {
        Ok(()) => {
            navigation::action(w, "navigation.reset", None);
            w.view.roll = 0.;
            w.status = "Classic presentation restored as a Settings preview".into();
        }
        Err(error) => fail(
            w,
            format!("Could not restore Classic presentation: {error}"),
        ),
    }
}

fn mark_theme_changed(w: &mut Workspace) {
    if let Some(state) = &mut w.settings {
        state.theme_changed = true;
        state.diagnostic = Some(
            "Theme preview active. Apply remembers it; Cancel restores the previous presentation."
                .into(),
        );
        // Loading a presentation never takes ownership of the personal reload preference.
        w.presentation_resources.watch = state.application.watch_presentation_files;
    }
}

pub fn load_theme(w: &mut Workspace, path: std::path::PathBuf) -> Result<()> {
    if w.settings.is_none() {
        open(w, Some("application.themes"));
    }
    if w.settings.as_ref().is_some_and(|state| state.pending) {
        return Err(CadError::Invalid(
            "Wait for settings validation before loading a theme".into(),
        ));
    }
    crate::theme_library::load(w, path)?;
    mark_theme_changed(w);
    Ok(())
}

pub fn apply(w: &mut Workspace, close: bool) {
    let Some(state) = w.settings.as_ref() else {
        return;
    };
    let preferences = state.application.clone();
    if let Err(error) = preferences.validate() {
        fail(w, error.to_string());
        return;
    }
    let model_changed = state.model_editable && w.session.draft.is_some();
    if model_changed {
        let draft = w.session.draft.as_ref().unwrap();
        if let Some(error) = &draft.error {
            fail(w, format!("Model settings could not be validated: {error}"));
            return;
        }
        if draft.pending
            || !draft.evaluation.as_ref().is_some_and(|evaluation| {
                evaluation.valid && evaluation.identity == w.session.identity()
            })
        {
            let state = w.settings.as_mut().unwrap();
            state.pending = true;
            state.close_after_apply = close;
            state.diagnostic = Some("Waiting for the current model preview to validate…".into());
            return;
        }
    }
    let theme_save = if w.settings.as_ref().is_some_and(|state| state.theme_changed) {
        match crate::theme_library::prepare_selected(w) {
            Ok(save) => Some(save),
            Err(error) => {
                fail(w, format!("Could not prepare the selected theme: {error}"));
                return;
            }
        }
    } else {
        None
    };
    // Persist before committing model source. A filesystem failure keeps both settings drafts open.
    if let Err(error) = settings_preferences::save(&preferences) {
        fail(
            w,
            format!("Could not save application preferences: {error}"),
        );
        return;
    }
    if let Some(save) = &theme_save
        && let Err(error) = crate::theme_library::commit_selected(save)
    {
        let theme_rollback = crate::theme_library::rollback_selected(save);
        let preference_rollback = settings_preferences::save(&w.application_preferences);
        fail(
            w,
            format!(
                "Could not save the selected theme: {error}{}{}",
                rollback_message(theme_rollback),
                rollback_message(preference_rollback)
            ),
        );
        return;
    }
    if model_changed && let Err(error) = w.session.apply() {
        let rollback = settings_preferences::save(&w.application_preferences);
        let theme_rollback = theme_save
            .as_ref()
            .map(crate::theme_library::rollback_selected)
            .transpose();
        fail(
            w,
            format!(
                "Could not apply model settings: {error}{}",
                rollback
                    .err()
                    .map(|error| format!("; preference rollback failed: {error}"))
                    .unwrap_or_default()
            ),
        );
        if let Err(error) = theme_rollback {
            fail(w, format!("{}; theme rollback failed: {error}", w.status));
        }
        return;
    }
    if let Some(save) = &theme_save {
        crate::theme_library::finish_selected(w, save);
    }
    w.application_preferences = preferences.clone();
    preview_application(w, &preferences);
    if model_changed {
        w.requested = Some(w.session.identity());
        w.changed = std::time::Instant::now();
    }
    let theme_checkpoint = crate::theme_library::capture(w);
    let state = w.settings.as_mut().unwrap();
    state.applied_application = preferences;
    state.theme_changed = false;
    state.theme_checkpoint = theme_checkpoint;
    if state.model_editable {
        state.checkpoint = Some(w.session.clone());
    }
    state.pending = false;
    state.diagnostic = Some(
        "Settings applied. Model preferences are saved with the model when you save it.".into(),
    );
    w.status = if model_changed {
        "Settings applied as one undoable model change"
    } else {
        "Application preferences saved"
    }
    .into();
    if close {
        let presentation = state.open_presentation_after_apply;
        w.settings = None;
        if presentation {
            crate::ui::action(w, "presentation");
        }
    }
}

fn fail(w: &mut Workspace, message: String) {
    w.status = message.clone();
    if let Some(state) = &mut w.settings {
        state.pending = false;
        state.diagnostic = Some(message);
    }
}

fn rollback_message(result: Result<()>) -> String {
    result
        .err()
        .map(|error| format!("; rollback failed: {error}"))
        .unwrap_or_default()
}

pub fn poll(w: &mut Workspace) {
    if let Some(state) = &w.settings
        && state.pending
        && w.session.draft.as_ref().is_none_or(|draft| !draft.pending)
    {
        apply(w, state.close_after_apply);
    }
}

pub fn cancel(w: &mut Workspace) {
    if let Some(state) = w.settings.take() {
        if state.theme_changed {
            crate::theme_library::restore(w, state.theme_checkpoint);
        }
        preview_application(w, &state.applied_application);
        if let Some(checkpoint) = state.checkpoint
            && w.session.draft.is_some()
        {
            w.worker.cancel();
            w.session = checkpoint;
            w.requested = None;
            w.rendered_key.clear();
            w.changed = std::time::Instant::now();
        }
        w.status = "Unapplied settings discarded".into();
    }
}

/// Apply personal defaults only at document creation. Existing files retain their own preferences.
pub fn new_document(w: &Workspace, assembly: bool) -> Document {
    let mut document = if assembly {
        Document::empty("Assembly1")
    } else {
        Document::blank_part("Part1")
    };
    document.settings = w.application_preferences.default_model.clone();
    for part in document.parts.values_mut() {
        part.finish = document.settings.default_new_part_finish.clone();
    }
    document
}

/// Existing navigation and size controls persist through the same personal preference contract.
pub fn save_live_application(w: &mut Workspace) {
    if let Some(state) = &mut w.settings {
        state.application.ui_scale = w.view.ui_scale;
        state.application.navigation = w.view.navigation.clone();
        state.application.watch_presentation_files = w.presentation_resources.watch;
        return;
    }
    let mut preferences = w.application_preferences.clone();
    preferences.ui_scale = w.view.ui_scale;
    preferences.navigation = w.view.navigation.clone();
    preferences.watch_presentation_files = w.presentation_resources.watch;
    match settings_preferences::save(&preferences) {
        Ok(()) => w.application_preferences = preferences,
        Err(error) => w.status = format!("Could not save application preferences: {error}"),
    }
}

pub fn action(w: &mut Workspace, command: &str) -> bool {
    if let Some(category) = command.strip_prefix("settings.open.") {
        open(w, Some(category));
        return true;
    }
    match command {
        "settings" => open(w, None),
        "settings.model" => open(w, Some("model.units")),
        "settings.application" => open(w, Some("application.interface")),
        "controls" | "navigation.preferences" => open(w, Some("application.navigation")),
        "settings.apply" => apply(w, false),
        "settings.ok" => apply(w, true),
        "settings.cancel" => cancel(w),
        "settings.theme-load" => {
            if let Some(path) = rfd::FileDialog::new()
                .set_title("Load theme or presentation package")
                .add_filter("Theme or package", &["css", "json"])
                .pick_file()
                && let Err(error) = load_theme(w, path)
            {
                fail(w, format!("Could not load theme: {error}"));
            }
        }
        "settings.theme-export" => {
            crate::presentation_resources::action(w, "presentation.export-package");
            if let Some(message) = w.presentation_resources.diagnostic.clone() {
                fail(w, message);
            }
        }
        "settings.presentation" => {
            if let Some(state) = &mut w.settings {
                state.open_presentation_after_apply = true;
            }
            apply(w, true);
            if w.settings.is_none() && w.dialog.as_deref() != Some("presentation") {
                crate::ui::action(w, "presentation");
            }
        }
        _ => return false,
    }
    true
}
