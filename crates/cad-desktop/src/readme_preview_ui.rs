//! Repository preview commands over the persistent geometry worker.
use crate::{Workspace, worker::WorkerClient};
use bevy_egui::egui;
use cad_core::{
    Document,
    presentation::SemanticElement,
    protocol::{RequestIdentity, WorkerRequest, WorkerResponse},
};
use std::path::PathBuf;

pub struct State {
    worker: WorkerClient,
    pending: Option<Pending>,
    pub include_on_commit: bool,
}
struct Pending {
    identity: RequestIdentity,
    document: Document,
    project: PathBuf,
    commit: Option<String>,
}
impl Default for State {
    fn default() -> Self {
        Self {
            worker: WorkerClient::new(),
            pending: None,
            include_on_commit: false,
        }
    }
}
pub fn busy(w: &Workspace) -> bool {
    w.readme_preview.pending.is_some()
}
pub fn request(w: &mut Workspace) {
    start(w, None);
}
pub fn request_before_commit(w: &mut Workspace, message: String) {
    start(w, Some(message));
}
fn start(w: &mut Workspace, commit: Option<String>) {
    if crate::git_ui::busy(w) || crate::git_ui::recovery_pending(w) {
        w.status = "Finish the Git operation or merge recovery before updating the README".into();
        return;
    }
    if busy(w) {
        w.status = "The README preview is already being built".into();
        return;
    }
    if w.session.draft.is_some()
        || w.tool_checkpoint.is_some()
        || w.saved_source != w.session.committed.fingerprint()
    {
        w.status = "Apply or cancel the current operation, then save the model before making its README preview".into();
        return;
    }
    let Some(project) = w.project.clone() else {
        w.status = "Save the project before making its README preview".into();
        return;
    };
    if cad_core::release::is_release_source(&project) {
        w.status = "Save a working copy before updating a released model's README".into();
        return;
    }
    let identity = w.session.identity();
    let document = w.session.committed.clone();
    w.readme_preview
        .worker
        .submit(WorkerRequest::ReadmePreview {
            identity: identity.clone(),
            document: document.clone(),
            project_root: w.root().to_string_lossy().into(),
            display_tolerance_mm: w.display_tolerance(),
        });
    w.readme_preview.pending = Some(Pending {
        identity,
        document,
        project,
        commit,
    });
    w.status = "Building the README model preview from the saved model".into();
}
pub fn poll(w: &mut Workspace) {
    for reply in w.readme_preview.worker.drain() {
        let Some(pending) = w.readme_preview.pending.take() else {
            continue;
        };
        if w.project.as_ref() != Some(&pending.project)
            || w.session.identity() != pending.identity
            || w.saved_source != pending.document.fingerprint()
        {
            w.status = "Model changed while the README preview was being built. Generate the preview again".into();
            continue;
        }
        match reply {
            WorkerResponse::ReadmePreview { identity, preview } if identity == pending.identity => {
                match cad_core::readme_preview::write(&pending.project, &pending.document, &preview)
                {
                    Ok(()) => {
                        w.status = if preview.metadata.inline {
                            "README updated with an interactive model preview"
                        } else {
                            "README updated with a thumbnail linked to the interactive STL viewer"
                        }
                        .into();
                        if let Some(message) = pending.commit {
                            crate::git_ui::commit_saved(w, message);
                        }
                    }
                    Err(error) => w.status = error.to_string(),
                }
            }
            WorkerResponse::Error { message, .. } => w.status = message,
            _ => {
                w.status =
                    "README worker returned an unexpected response; the files were not changed"
                        .into();
            }
        }
    }
}
pub fn show(ui: &mut egui::Ui, w: &mut Workspace) {
    ui.collapsing("README model preview", |ui| {
        let elements = semantics(w);
        for element in &elements {
            if element.data.get("setter").is_some() { crate::ui::semantic_field(ui, w, element); }
            else {
                for descriptor in element.action_descriptors() {
                    if crate::presentation_controls::button(ui, &element.id, &element.role, &["semantic-control"], &descriptor.label, descriptor.enabled, false).clicked() {
                        action(w, &descriptor.command);
                    }
                }
            }
        }
        ui.small("Small models rotate inside GitHub's README. Larger models use a thumbnail linked to the STL viewer. The preview shows the saved project.");
    });
}
pub fn semantics(w: &Workspace) -> Vec<SemanticElement> {
    let enabled = w.project.is_some()
        && !busy(w)
        && !crate::git_ui::busy(w)
        && !crate::git_ui::recovery_pending(w)
        && w.session.draft.is_none()
        && w.tool_checkpoint.is_none();
    vec![
        SemanticElement {
            id: "readme.include-on-commit".into(),
            role: "readme-preview-field".into(),
            label: "Update preview before each commit".into(),
            value: Some(w.readme_preview.include_on_commit.to_string()),
            state: "ready".into(),
            commands: vec![],
            relationships: vec![],
            data: serde_json::json!({"setter":"readme-preview","field":"include_on_commit","kind":"boolean","json":w.readme_preview.include_on_commit,"editable":!busy(w)}),
        },
        SemanticElement {
            id: "readme.preview".into(),
            role: "readme-preview-action".into(),
            label: "Update README preview".into(),
            value: None,
            state: if busy(w) { "building" } else { "ready" }.into(),
            commands: vec!["readme.preview".into()],
            relationships: vec![w.session.committed.id.clone()],
            data: serde_json::json!({"contract":"tcad.readme-preview","version":1,"enabled":enabled,"disabled_reason":"Save the project and finish the current operation first","actions":[{"command":"readme.preview","label":"Update README preview","enabled":enabled}]}),
        },
    ]
}
pub fn action(w: &mut Workspace, command: &str) -> bool {
    if command != "readme.preview" {
        return false;
    }
    request(w);
    true
}
pub fn set_value(
    w: &mut Workspace,
    _element: &SemanticElement,
    value: serde_json::Value,
) -> cad_core::Result<()> {
    if busy(w) || w.settings.is_some() {
        return Err(cad_core::CadError::Invalid(
            "Finish the preview or settings dialog before changing this preference".into(),
        ));
    }
    let enabled = value.as_bool().ok_or_else(|| {
        cad_core::CadError::Invalid("Preview preference must be true or false".into())
    })?;
    let mut preferences = w.application_preferences.clone();
    preferences.readme_preview_on_commit = enabled;
    crate::settings_preferences::save(&preferences)?;
    w.application_preferences = preferences;
    w.readme_preview.include_on_commit = enabled;
    Ok(())
}
pub fn current_value(w: &Workspace, _element: &SemanticElement) -> Option<serde_json::Value> {
    Some(serde_json::Value::Bool(w.readme_preview.include_on_commit))
}
