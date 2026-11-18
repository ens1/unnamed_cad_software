//! Resolve unsaved work before replacing the current project.
use crate::{Workspace, presentation_controls as controls};
use bevy_egui::egui;
use cad_core::{
    CadError, Document, Session,
    commands::Command,
    persistence::{FileStore, ProjectStore, copy_resources},
    presentation::SemanticElement,
};
use serde_json::json;
use std::path::{Path, PathBuf};

pub(crate) enum Destination {
    NewPart,
    NewAssembly,
    Open(PathBuf),
    Recover(PathBuf),
}

pub(crate) struct Pending {
    destination: Destination,
    focus_keep: bool,
}

impl Destination {
    fn replace(self, w: &mut Workspace) {
        let result = match self {
            Self::NewPart | Self::NewAssembly => {
                crate::settings::new_document(w, matches!(self, Self::NewAssembly))
                    .map(|doc| (doc, None))
            }
            Self::Open(path) => FileStore.load(&path).map(|doc| (doc, Some(path))),
            Self::Recover(path) => {
                if let Err(error) = recover(w, &path) {
                    w.status = format!("Could not recover the project. {error}");
                }
                return;
            }
        };
        match result {
            Ok((doc, path)) => w.replace(doc, path),
            Err(error) => w.status = format!("Could not switch projects. {error}"),
        }
    }

    fn discard_label(&self) -> &'static str {
        match self {
            Self::NewPart => "Discard and Create Part",
            Self::NewAssembly => "Discard and Create Assembly",
            Self::Open(_) => "Discard and Open",
            Self::Recover(_) => "Discard and Recover",
        }
    }
}

#[derive(serde::Deserialize)]
struct Recovery {
    committed: Document,
    draft: Option<Document>,
    resource_root: Option<PathBuf>,
}

fn recover(w: &mut Workspace, path: &Path) -> cad_core::Result<()> {
    let snapshot: Recovery = serde_json::from_slice(&std::fs::read(path)?)?;
    snapshot.committed.validate()?;
    let id = &snapshot.committed.id;
    if Path::new(id).file_name().and_then(|name| name.to_str()) != Some(id.as_str()) {
        return Err(CadError::Invalid("Invalid recovery document ID".into()));
    }
    let base = crate::recovery_directory().join("unsaved");
    let mut sources: Vec<_> = snapshot.resource_root.into_iter().collect();
    sources.push(base.join(id));
    if id == &w.session.committed.id {
        sources.push(w.root());
    }
    sources.extend(path.parent().map(Path::to_path_buf));
    let mut session = Session::new(snapshot.committed, crate::BACKEND);
    if let Some(document) = snapshot.draft {
        if document.id != session.committed.id {
            return Err(CadError::Invalid(
                "Recovery draft belongs to a different document".into(),
            ));
        }
        session.dispatch(Command::ReplaceDocument { document })?;
    }
    // A recovery may share an ID with the active project. Build a fresh cache
    // before swapping sessions so a truncated snapshot cannot change its files.
    std::fs::create_dir_all(&base)?;
    let root = base.join(cad_core::id());
    std::fs::create_dir(&root)?;
    let copy = copy_resources(&session.committed, &sources, &root).and_then(|()| {
        if let Some(draft) = &session.draft {
            copy_resources(&draft.document, &sources, &root)?;
            // A shared path must resolve to the expected bytes in both states.
            copy_resources(&session.committed, &[], &root)?;
        }
        Ok(())
    });
    if let Err(error) = copy {
        let _ = std::fs::remove_dir_all(&root);
        return Err(error);
    }
    if w.replace_session(session, None, Some(root.clone())) {
        w.status = "Recovered local work. Choose Save As to keep a project copy.".into();
    } else {
        let _ = std::fs::remove_dir_all(root);
    }
    Ok(())
}

fn editing(w: &Workspace) -> bool {
    w.tool_checkpoint.is_some() || w.session.draft.is_some() || w.sketch.is_some()
}

pub(crate) fn request(w: &mut Workspace, destination: Destination) {
    if editing(w)
        || crate::document_identity_ui::has_unsaved_changes(w)
        || w.project.is_none() && w.recovered_resources.is_some()
    {
        w.file_switch = Some(Pending {
            destination,
            focus_keep: true,
        });
    } else {
        destination.replace(w);
    }
}

const KEEP: &str = "file-switch.keep";
const SAVE: &str = "file-switch.save";
const DISCARD: &str = "file-switch.discard";

pub(crate) fn semantics(w: &Workspace) -> Vec<SemanticElement> {
    let Some(pending) = &w.file_switch else {
        return vec![];
    };
    let name = &w.session.committed.name;
    let editing = editing(w);
    let message = if editing {
        format!(
            "Keep editing to finish and save {name}, or discard your current edit and any unsaved changes."
        )
    } else {
        format!("Save the changes to {name} before leaving this project?")
    };
    let mut elements = vec![SemanticElement {
        id: "file-switch".into(),
        role: "file-switch-dialog".into(),
        label: "Unsaved changes".into(),
        value: Some(message),
        state: "open".into(),
        commands: vec![],
        relationships: vec![],
        data: json!({"document": w.session.committed.id}),
    }];
    let save_label = if w.project.is_none() {
        "Save and Continue…"
    } else {
        "Save and Continue"
    };
    for (command, label) in [
        (KEEP, "Keep Editing"),
        (SAVE, save_label),
        (DISCARD, pending.destination.discard_label()),
    ] {
        if command == SAVE && editing {
            continue;
        }
        elements.push(SemanticElement {
            id: command.into(),
            role: "file-switch-action".into(),
            label: label.into(),
            value: None,
            state: "ready".into(),
            commands: vec![command.into()],
            relationships: vec!["file-switch".into()],
            data: json!({"actions":[{"command":command,"enabled":true}]}),
        });
    }
    elements
}

pub(crate) fn action(w: &mut Workspace, command: &str) {
    if command == SAVE && editing(w) {
        return;
    }
    if !matches!(command, KEEP | DISCARD | SAVE) {
        return;
    }
    let Some(pending) = w.file_switch.take() else {
        return;
    };
    // This choice saves the committed document under its current name. For a
    // first save, save_now prompts for a project folder and returns true only
    // after writing it. Dropping the intent here also cancels it on failure.
    if command == DISCARD || (command == SAVE && crate::ui::save_now(w, false)) {
        pending.destination.replace(w);
    }
}

pub(crate) fn show(ctx: &egui::Context, w: &mut Workspace) {
    let elements = semantics(w);
    let Some((dialog, actions)) = elements.split_first() else {
        return;
    };
    let focus_keep = w
        .file_switch
        .as_mut()
        .is_some_and(|p| std::mem::take(&mut p.focus_keep));
    let response = egui::Modal::new(egui::Id::new(&dialog.id)).show(ctx, |ui| {
        let props = controls::properties(&dialog.id, &dialog.role, &[], "open");
        controls::apply_properties(ui, &props);
        let available = (ctx.content_rect().width() - 40.).max(1.);
        ui.set_width(controls::length(&props, "width", available, 360.).min(available));
        ui.heading(&dialog.label);
        ui.label(dialog.value.as_deref().unwrap_or_default());
        ui.add_space(4.);
        ui.horizontal_wrapped(|ui| {
            for item in actions {
                let response =
                    controls::button(ui, &item.id, &item.role, &[], &item.label, true, false);
                if focus_keep && item.id == KEEP {
                    response.request_focus();
                }
                if response.clicked() {
                    return Some(item.id.as_str());
                }
            }
            None
        })
        .inner
    });
    if let Some(command) = response.inner {
        action(w, command);
    } else if response.should_close() {
        action(w, KEEP);
    }
}
