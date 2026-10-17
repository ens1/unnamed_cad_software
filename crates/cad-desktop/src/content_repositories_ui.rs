//! Personal data libraries. Git work never owns or replaces a modeling draft.
use crate::{Workspace, presentation_controls as controls};
use bevy_egui::egui;
use cad_core::{
    CadError, Result,
    content_repositories::{self as libraries, ContentKind},
    forge::CancelToken,
    git_connections::ConnectionProfile,
    presentation::SemanticElement,
    repository_models::{self as models, GitAuthor},
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{Mutex, mpsc},
};

#[derive(Default)]
pub struct State {
    initialized: bool,
    registry: libraries::Registry,
    notices: Vec<String>,
    selected: String,
    fields: BTreeMap<String, Value>,
    inspection: Option<libraries::RepositoryInspection>,
    update: Option<libraries::UpdateCandidate>,
    publish: Option<(libraries::PublishRequest, Option<ConnectionProfile>)>,
    pending: Option<CancelToken>,
    receiver: Mutex<Option<mpsc::Receiver<Event>>>,
    status: String,
    outcome_notice: Option<String>,
    context_revision: u64,
}
enum Event {
    Progress(String),
    Done(Box<std::result::Result<Outcome, String>>),
}
enum Outcome {
    Refresh(
        libraries::Registry,
        Vec<String>,
        Option<cad_core::git_connections::GitIdentity>,
    ),
    Changed(String, String),
    Inspected(libraries::RepositoryInspection),
    Update(libraries::UpdateCandidate),
    Publish(libraries::PublishRequest, Option<ConnectionProfile>),
}
const SOURCES: [(&str, &str); 5] = [
    ("Weldment profiles", "tcad-weldment-profiles"),
    ("Textures and finishes", "tcad-textures"),
    ("Themes", "tcad-themes"),
    ("Materials", "tcad-materials"),
    ("Standard parts", "tcad-standard-parts"),
];
const SECTIONS: [(&str, &str); 6] = [
    ("installed", "Installed libraries"),
    ("add", "Get libraries"),
    ("create", "Create or fork a library"),
    ("update", "Review updates"),
    ("edit", "Edit and save versions"),
    ("publish", "Publish library"),
];
fn invalid(message: impl Into<String>) -> CadError {
    CadError::Invalid(message.into())
}
fn idle(w: &Workspace) -> bool {
    w.content_repositories.pending.is_none()
}
fn value(w: &Workspace, key: &str) -> Value {
    w.content_repositories
        .fields
        .get(key)
        .cloned()
        .unwrap_or_else(|| match key {
            "kind" | "entry-kind" => json!("weldment_profiles"),
            "branch" => json!(""),
            "publish-branch" => json!("main"),
            "publish-confirmed" => json!(false),
            "message" => json!("Update content library"),
            _ => json!(""),
        })
}
fn text(w: &Workspace, key: &str) -> String {
    value(w, key).as_str().unwrap_or_default().to_owned()
}
fn set(w: &mut Workspace, key: &str, value: Value) {
    w.content_repositories.fields.insert(key.into(), value);
}
fn selected(w: &Workspace) -> Result<libraries::InstalledRepository> {
    w.content_repositories
        .registry
        .repositories
        .iter()
        .find(|r| r.id == w.content_repositories.selected)
        .cloned()
        .ok_or_else(|| invalid("Select an installed library first"))
}
fn context(w: &Workspace) -> Value {
    json!([
        w.content_repositories.selected,
        w.content_repositories.context_revision
    ])
}
fn advance_context(w: &mut Workspace) {
    w.content_repositories.context_revision =
        w.content_repositories.context_revision.wrapping_add(1);
}
fn clear_reviews(w: &mut Workspace) {
    advance_context(w);
    w.content_repositories.inspection = None;
    w.content_repositories.update = None;
    w.content_repositories.publish = None;
    set(w, "publish-confirmed", json!(false));
}
fn destination(w: &Workspace) -> Result<PathBuf> {
    let path = text(w, "destination");
    if path.trim().is_empty() {
        return Err(invalid("Choose a new local folder"));
    }
    Ok(PathBuf::from(path.trim()))
}
fn next_folder(name: &str) -> PathBuf {
    let base = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    base.join(".tcad/library-checkouts")
        .join(format!("{}-{}", name, &cad_core::id()[..8]))
}
/// Creating a checkout may create our managed parent, never a user-selected target.
fn prepare_destination(destination: &std::path::Path) -> Result<()> {
    let managed = next_folder("unused")
        .parent()
        .map(std::path::Path::to_path_buf)
        .ok_or_else(|| invalid("The managed library folder is unavailable"))?;
    if destination.parent() == Some(managed.as_path()) {
        std::fs::create_dir_all(managed)?;
    }
    Ok(())
}
fn author(w: &Workspace) -> GitAuthor {
    GitAuthor {
        name: text(w, "author-name"),
        email: text(w, "author-email"),
    }
}
fn kind(w: &Workspace, key: &str) -> Result<ContentKind> {
    Ok(serde_json::from_value(value(w, key))?)
}
fn status(w: &mut Workspace, message: impl Into<String>) {
    let message = message.into();
    if w.settings
        .as_ref()
        .is_some_and(|settings| settings.category == "application.libraries")
    {
        w.status = message.clone();
    }
    w.content_repositories.status = message;
}
fn submit(
    w: &mut Workspace,
    label: &str,
    work: impl FnOnce(CancelToken, mpsc::Sender<Event>) -> Result<Outcome> + Send + 'static,
) {
    let cancel = CancelToken::default();
    let worker = cancel.clone();
    let (tx, rx) = mpsc::channel();
    let connected = if let Ok(mut slot) = w.content_repositories.receiver.lock() {
        *slot = Some(rx);
        true
    } else {
        false
    };
    if !connected {
        status(
            w,
            "The library worker is unavailable; reopen TCAD before trying again",
        );
        return;
    }
    w.content_repositories.initialized = true;
    w.content_repositories.pending = Some(cancel);
    status(w, label);
    std::thread::spawn(move || {
        let result = work(worker, tx.clone()).map_err(|e| e.to_string());
        let _ = tx.send(Event::Done(Box::new(result)));
    });
}
fn refresh(w: &mut Workspace) {
    submit(w, "Reading installed libraries…", |cancel, _| {
        let snapshot = libraries::refresh_catalog_with_cancel(&cancel)?;
        if cancel.is_cancelled() {
            return Err(invalid("Library refresh cancelled"));
        }
        cad_core::finish_library::refresh_catalog();
        let mut notices = snapshot.notices;
        notices.extend(cad_core::finish_library::catalog_diagnostics());
        if let Err(e) = cad_core::weldment_profiles::installed_catalogs() {
            notices.push(e.to_string());
        }
        if let Err(e) = cad_core::material_library::installed_catalog() {
            notices.push(e.to_string());
        }
        if let Err(e) = cad_core::standard_parts::catalog() {
            notices.push(e.to_string());
        }
        if cancel.is_cancelled() {
            return Err(invalid("Library refresh cancelled"));
        }
        Ok(Outcome::Refresh(
            libraries::load_registry()?,
            notices,
            cad_core::git_connections::read_author(None).ok(),
        ))
    });
}
pub fn poll(w: &mut Workspace) {
    if !w.content_repositories.initialized {
        w.content_repositories.initialized = true;
        refresh(w);
    }
    let mut disconnected = false;
    let events = if let Ok(slot) = w.content_repositories.receiver.lock() {
        let mut events = Vec::new();
        if let Some(receiver) = slot.as_ref() {
            loop {
                match receiver.try_recv() {
                    Ok(event) => events.push(event),
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        disconnected = true;
                        break;
                    }
                }
            }
        }
        events
    } else {
        vec![]
    };
    let completed = events.iter().any(|event| matches!(event, Event::Done(_)));
    for event in events {
        match event {
            Event::Progress(message) => status(w, message),
            Event::Done(result) => {
                w.content_repositories.pending = None;
                if let Ok(mut slot) = w.content_repositories.receiver.lock() {
                    *slot = None;
                }
                match *result {
                    Err(error) => {
                        w.content_repositories.outcome_notice = Some(error.clone());
                        status(w, error);
                    }
                    Ok(Outcome::Refresh(registry, notices, author)) => {
                        w.content_repositories.registry = registry;
                        w.content_repositories.notices = notices;
                        advance_context(w);
                        if selected(w).is_err() {
                            w.content_repositories.selected.clear();
                            clear_reviews(w);
                        }
                        if !w.content_repositories.fields.contains_key("destination") {
                            set(w, "destination", json!(next_folder("library")));
                        }
                        if let Some(author) = author {
                            for (key, text) in
                                [("author-name", author.name), ("author-email", author.email)]
                            {
                                if !w.content_repositories.fields.contains_key(key) {
                                    set(w, key, json!(text));
                                }
                            }
                        }
                        crate::theme_library::refresh(w);
                        status(
                            w,
                            "Libraries refreshed. Saved models and the active theme keep their current data.",
                        );
                    }
                    Ok(Outcome::Changed(id, message)) => {
                        if w.content_repositories.selected != id {
                            set(w, "publish-url", json!(""));
                            set(w, "publish-branch", json!("main"));
                            for key in ["entry-id", "entry-name", "entry-path"] {
                                set(w, key, json!(""));
                            }
                            set(w, "destination", json!(next_folder("library")));
                        }
                        w.content_repositories.selected = id;
                        clear_reviews(w);
                        w.content_repositories.outcome_notice = Some(message);
                        refresh(w);
                    }
                    Ok(Outcome::Inspected(review)) => {
                        advance_context(w);
                        w.content_repositories.inspection = Some(review);
                        status(w, "Review the files before saving a version.");
                    }
                    Ok(Outcome::Update(review)) => {
                        advance_context(w);
                        w.content_repositories.update = Some(review);
                        status(
                            w,
                            "Update ready to review. Apply switches this library to the new checkout.",
                        );
                    }
                    Ok(Outcome::Publish(request, connection)) => {
                        advance_context(w);
                        w.content_repositories.publish = Some((request, connection));
                        status(
                            w,
                            "Review the destination and visibility before publishing.",
                        );
                    }
                }
            }
        }
    }
    if disconnected && !completed && w.content_repositories.pending.is_some() {
        w.content_repositories.pending = None;
        if let Ok(mut slot) = w.content_repositories.receiver.lock() {
            *slot = None;
        }
        let message = "The library worker stopped before reporting a result. Local files were retained; refresh the list before retrying.";
        w.content_repositories.outcome_notice = Some(message.into());
        status(w, message);
    }
}
pub fn action(w: &mut Workspace, command: &str, args: Option<&Value>) -> bool {
    if command == "content-repositories" {
        crate::settings::open(w, Some("application.libraries"));
        return true;
    }
    let Some(command) = command.strip_prefix("content-repositories.") else {
        return false;
    };
    if command == "cancel" {
        if let Some(cancel) = &w.content_repositories.pending {
            cancel.cancel();
        }
        return true;
    }
    if !idle(w) {
        status(w, "Wait for the library operation, or cancel it first");
        return true;
    }
    let result = (|| -> Result<()> {
        if let Some(bound) = args.and_then(|args| args.get("context"))
            && *bound != context(w)
        {
            return Err(invalid(
                "This library control is from an earlier selection or review. Use the current panel.",
            ));
        }
        match command {
            "refresh" => {
                clear_reviews(w);
                refresh(w);
            }
            "source" => {
                let index = args
                    .and_then(|a| a["index"].as_u64())
                    .ok_or_else(|| invalid("Select a library source"))?
                    as usize;
                let (_, name) = SOURCES
                    .get(index)
                    .ok_or_else(|| invalid("Unknown source"))?;
                set(
                    w,
                    "url",
                    json!(format!("https://github.com/ens1/{name}.git")),
                );
                set(w, "destination", json!(next_folder(name)));
                set(w, "branch", json!(""));
                clear_reviews(w);
            }
            "select" => {
                let id = args
                    .and_then(|a| a["id"].as_str())
                    .ok_or_else(|| invalid("Choose an installed library"))?;
                let repository = w
                    .content_repositories
                    .registry
                    .repositories
                    .iter()
                    .find(|repository| repository.id == id)
                    .cloned()
                    .ok_or_else(|| invalid("The selected library is no longer installed"))?;
                w.content_repositories.selected = repository.id;
                clear_reviews(w);
                set(w, "publish-url", json!(""));
                set(w, "publish-branch", json!(repository.branch));
                for key in ["entry-id", "entry-name", "entry-path"] {
                    set(w, key, json!(""));
                }
            }
            "toggle" | "remove" => {
                let repository = selected(w)?;
                let remove = command == "remove";
                submit(w, "Updating library list…", move |_, _| {
                    if remove {
                        libraries::remove(&repository.id)?;
                    } else {
                        libraries::set_enabled(&repository.id, !repository.enabled)?;
                    }
                    Ok(Outcome::Changed(
                        if remove { String::new() } else { repository.id },
                        "Library files are retained on disk.".into(),
                    ))
                });
            }
            "folder" => {
                if let Some(parent) = rfd::FileDialog::new()
                    .set_title("Choose parent folder for a new library checkout")
                    .pick_folder()
                {
                    set(
                        w,
                        "destination",
                        json!(parent.join(format!("library-{}", &cad_core::id()[..8]))),
                    );
                    clear_reviews(w);
                }
            }
            "register" => {
                if let Some(root) = rfd::FileDialog::new()
                    .set_title("Choose a Git library folder containing tcad-library.json")
                    .pick_folder()
                {
                    submit(w, "Registering library…", move |cancel, _| {
                        let r = libraries::register_existing_with_cancel(&root, true, &cancel)?;
                        Ok(Outcome::Changed(r.id, "Library added.".into()))
                    });
                }
            }
            "clone" => {
                let request = models::CloneRequest {
                    clone_url: text(w, "url"),
                    destination: destination(w)?,
                    branch: (!text(w, "branch").trim().is_empty()).then(|| text(w, "branch")),
                    expected_commit: None,
                };
                let connection = crate::git_connections_ui::resolve(w, &text(w, "connection"))?;
                submit(w, "Cloning library…", move |cancel, tx| {
                    prepare_destination(&request.destination)?;
                    let r = libraries::clone_and_register(
                        &request,
                        connection.as_ref(),
                        &cancel,
                        |s| {
                            let _ = tx.send(Event::Progress(s.into()));
                        },
                    )?;
                    Ok(Outcome::Changed(r.id, "Library cloned and enabled.".into()))
                });
            }
            "create" => {
                let destination = destination(w)?;
                let name = text(w, "name");
                let kind = kind(w, "kind")?;
                let author = author(w);
                submit(w, "Creating local library…", move |cancel, tx| {
                    prepare_destination(&destination)?;
                    let r = libraries::create_repository(
                        &destination,
                        &name,
                        kind,
                        &author,
                        &cancel,
                        |s| {
                            let _ = tx.send(Event::Progress(s.into()));
                        },
                    )?;
                    Ok(Outcome::Changed(
                        r.id,
                        "Local library created. It has no publishing destination.".into(),
                    ))
                });
            }
            "fork" => {
                let repository = selected(w)?;
                let destination = destination(w)?;
                let connection = crate::git_connections_ui::resolve(w, &text(w, "connection"))?;
                submit(w, "Forking library…", move |cancel, tx| {
                    prepare_destination(&destination)?;
                    let r = libraries::fork_repository(
                        &repository.id,
                        &destination,
                        connection.as_ref(),
                        &cancel,
                        |s| {
                            let _ = tx.send(Event::Progress(s.into()));
                        },
                    )?;
                    Ok(Outcome::Changed(r.id,"Fork created and disabled. Disable the original, then enable your copy to avoid duplicate catalog IDs.".into()))
                });
            }
            "inspect-update" => {
                let repository = selected(w)?;
                let destination = next_folder("update");
                let connection = crate::git_connections_ui::resolve(w, &text(w, "connection"))?;
                w.content_repositories.update = None;
                submit(
                    w,
                    "Fetching a separate update checkout…",
                    move |cancel, tx| {
                        prepare_destination(&destination)?;
                        Ok(Outcome::Update(libraries::inspect_update(
                            &repository.id,
                            &destination,
                            connection.as_ref(),
                            &cancel,
                            |s| {
                                let _ = tx.send(Event::Progress(s.into()));
                            },
                        )?))
                    },
                );
            }
            "apply-update" => {
                let review = w
                    .content_repositories
                    .update
                    .clone()
                    .ok_or_else(|| invalid("Inspect an update first"))?;
                submit(w, "Applying reviewed library update…", move |_, _| {
                    libraries::apply_update(&review)?;
                    Ok(Outcome::Changed(review.repository_id,"Update applied. Previous checkout retained; existing models are unchanged.".into()))
                });
            }
            "inspect" => {
                let id = selected(w)?.id;
                clear_reviews(w);
                submit(w, "Inspecting library files…", move |cancel, _| {
                    Ok(Outcome::Inspected(libraries::inspect_repository(
                        &id, &cancel,
                    )?))
                });
            }
            "commit" => {
                let review = w
                    .content_repositories
                    .inspection
                    .as_ref()
                    .ok_or_else(|| invalid("Inspect changes first"))?;
                let request = libraries::CommitRequest {
                    repository_id: review.repository.id.clone(),
                    message: text(w, "message"),
                    author: author(w),
                    expected_fingerprint: review.fingerprint.clone(),
                    expected_head: review.head.clone(),
                    expected_branch: review.branch.clone(),
                };
                submit(w, "Saving library version…", move |cancel, tx| {
                    let r = libraries::commit_changes(&request, &cancel, |s| {
                        let _ = tx.send(Event::Progress(s.into()));
                    })?;
                    Ok(Outcome::Changed(
                        r.repository.id,
                        format!("Saved version {}", r.commit),
                    ))
                });
            }
            "entry-file" => {
                let root = selected(w)?.root.canonicalize()?;
                if let Some(path) = rfd::FileDialog::new()
                    .set_directory(&root)
                    .set_title("Select a data file already inside the library")
                    .pick_file()
                {
                    let path = path.canonicalize()?;
                    let relative = path
                        .strip_prefix(&root)
                        .map_err(|_| invalid("Copy the file into this library folder first"))?;
                    set(
                        w,
                        "entry-path",
                        json!(relative.to_string_lossy().replace('\\', "/")),
                    );
                    if text(w, "entry-id").is_empty() {
                        set(
                            w,
                            "entry-id",
                            json!(path.file_stem().unwrap_or_default().to_string_lossy()),
                        );
                    }
                    if text(w, "entry-name").is_empty() {
                        set(
                            w,
                            "entry-name",
                            json!(path.file_stem().unwrap_or_default().to_string_lossy()),
                        );
                    }
                    clear_reviews(w);
                }
            }
            "edit-entry" => {
                let repository = selected(w)?;
                let id = args
                    .and_then(|args| args["entry"].as_str())
                    .ok_or_else(|| invalid("Choose an entry"))?;
                let entry = repository
                    .manifest
                    .entries
                    .iter()
                    .find(|entry| entry.id == id)
                    .ok_or_else(|| invalid("This library entry is no longer available"))?;
                set(w, "entry-id", json!(entry.id));
                set(w, "entry-name", json!(entry.name));
                set(w, "entry-kind", json!(entry.kind));
                set(w, "entry-path", json!(entry.path));
                clear_reviews(w);
                status(
                    w,
                    "Edit the fields, then choose Add or replace entry. Changing an ID adds a separate entry.",
                );
            }
            "use-source-destination" => {
                let remote = selected(w)?
                    .remote
                    .ok_or_else(|| invalid("This library has no source URL"))?;
                clear_reviews(w);
                set(w, "publish-url", json!(remote));
                status(
                    w,
                    "Source URL copied. Check that this is your intended destination before publishing a fork.",
                );
            }
            "add-entry" => {
                let id = selected(w)?.id;
                let entry = libraries::LibraryEntry {
                    id: text(w, "entry-id"),
                    name: text(w, "entry-name"),
                    kind: kind(w, "entry-kind")?,
                    path: text(w, "entry-path"),
                };
                submit(w, "Adding library entry…", move |cancel, _| {
                    let r = libraries::add_entry(&id, entry, &cancel)?;
                    Ok(Outcome::Changed(
                        r.id,
                        "Entry added. Inspect and save a version when ready.".into(),
                    ))
                });
            }
            "remove-entry" => {
                let id = selected(w)?.id;
                let entry = args
                    .and_then(|a| a["entry"].as_str())
                    .ok_or_else(|| invalid("Choose an entry"))?
                    .to_owned();
                submit(w, "Removing library entry…", move |cancel, _| {
                    let r = libraries::remove_entry(&id, &entry, &cancel)?;
                    Ok(Outcome::Changed(
                        r.id,
                        "Entry removed from the manifest; its file remains.".into(),
                    ))
                });
            }
            "files" => {
                let root = selected(w)?.root;
                #[cfg(target_os = "macos")]
                let program = "open";
                #[cfg(target_os = "windows")]
                let program = "explorer";
                #[cfg(not(any(target_os = "macos", target_os = "windows")))]
                let program = "xdg-open";
                std::process::Command::new(program).arg(root).spawn()?;
            }
            "review-publish" => {
                let id = selected(w)?.id;
                let url = text(w, "publish-url").trim().to_owned();
                let branch = text(w, "publish-branch").trim().to_owned();
                if branch.is_empty() {
                    return Err(invalid("Enter a destination branch"));
                }
                let connection = crate::git_connections_ui::resolve(w, &text(w, "connection"))?;
                w.content_repositories.publish = None;
                set(w, "publish-confirmed", json!(false));
                submit(
                    w,
                    "Reviewing publishing destination…",
                    move |cancel, tx| {
                        let local = libraries::inspect_repository(&id, &cancel)?;
                        if !local.status.trim().is_empty() {
                            return Err(invalid(
                                "Save a version of all library changes before publishing",
                            ));
                        }
                        let remote =
                            models::inspect_remote(&url, connection.as_ref(), &cancel, |s| {
                                let _ = tx.send(Event::Progress(s.into()));
                            })?;
                        let tip = remote
                            .branches
                            .iter()
                            .find(|b| b.name == branch)
                            .map(|b| b.commit.clone());
                        Ok(Outcome::Publish(
                            libraries::PublishRequest {
                                repository_id: id,
                                clone_url: url,
                                branch,
                                expected_remote_commit: tip,
                                remote_reviewed: true,
                                expected_local_commit: local
                                    .head
                                    .ok_or_else(|| invalid("Save a library version first"))?,
                            },
                            connection,
                        ))
                    },
                );
            }
            "publish" => {
                if value(w, "publish-confirmed") != true {
                    return Err(invalid("Confirm the destination access before publishing"));
                }
                let (request, connection) = w
                    .content_repositories
                    .publish
                    .clone()
                    .ok_or_else(|| invalid("Review the destination first"))?;
                submit(
                    w,
                    "Publishing library and its Git history…",
                    move |cancel, tx| {
                        let r = libraries::publish_repository(
                            &request,
                            connection.as_ref(),
                            &cancel,
                            |s| {
                                let _ = tx.send(Event::Progress(s.into()));
                            },
                        )?;
                        Ok(Outcome::Changed(
                            r.repository.id,
                            format!("Published {}. {}", r.commit, r.notices.join(" ")),
                        ))
                    },
                );
            }
            _ => return Err(invalid("Unknown content library action")),
        }
        Ok(())
    })();
    if let Err(error) = result {
        status(w, error.to_string());
    }
    true
}
fn field(
    w: &Workspace,
    group: &str,
    key: &str,
    label: &str,
    choices: Option<Vec<Value>>,
) -> SemanticElement {
    let current = value(w, key);
    SemanticElement {
        id: format!("content-repositories/{key}"),
        role: "content-library-field".into(),
        label: label.into(),
        value: Some(
            current
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| current.to_string()),
        ),
        state: if idle(w) { "editable" } else { "readonly" }.into(),
        commands: vec![],
        relationships: vec![],
        data: json!({"contract":"tcad.content-libraries-ui","version":1,"group":group,"setter":"content-repositories","key":key,"json":current,"value_type":if choices.is_some(){"choice"}else if current.is_boolean(){"boolean"}else{"text"},"choices":choices,"editable":idle(w),"classes":["modeling-field"],"accessibility_label":label}),
    }
}
fn button(
    group: &str,
    key: &str,
    command: &str,
    label: &str,
    args: Value,
    enabled: bool,
) -> SemanticElement {
    SemanticElement {
        id: format!("content-repositories/{group}/{key}"),
        role: "content-library-action".into(),
        label: label.into(),
        value: None,
        state: if enabled { "ready" } else { "disabled" }.into(),
        commands: vec![command.into()],
        relationships: vec![],
        data: json!({"group":group,"actions":[{"command":command,"label":label,"enabled":enabled,"arguments":args}],"classes":["modeling-action"]}),
    }
}
fn info(group: &str, key: &str, label: impl Into<String>) -> SemanticElement {
    SemanticElement {
        id: format!("content-repositories/{group}/{key}"),
        role: "content-library-information".into(),
        label: label.into(),
        value: None,
        state: "ready".into(),
        commands: vec![],
        relationships: vec![],
        data: json!({"group":group}),
    }
}
fn kinds() -> Vec<Value> {
    [
        ContentKind::WeldmentProfiles,
        ContentKind::Textures,
        ContentKind::Themes,
        ContentKind::Materials,
        ContentKind::StandardParts,
        ContentKind::Finishes,
    ]
    .into_iter()
    .map(|kind| json!({"value":kind,"label":kind.label()}))
    .collect()
}
pub fn semantics(w: &Workspace) -> Vec<SemanticElement> {
    let available = idle(w);
    let mut out = Vec::new();
    for (group, label) in SECTIONS {
        out.push(SemanticElement {
            id: format!("content-repositories/group/{group}"),
            role: "content-library-section".into(),
            label: label.into(),
            value: None,
            state: "ready".into(),
            commands: vec![],
            relationships: vec![],
            data: json!({"group":group}),
        });
    }
    out.push(info("installed","help","Libraries are personal. Changes save immediately; Settings Cancel does not undo them. Models keep copied definitions and textures."));
    if w.content_repositories.registry.repositories.is_empty() {
        out.push(info("installed","empty","No libraries installed. TCAD includes only the default theme and a plain aluminum appearance."));
    }
    for r in &w.content_repositories.registry.repositories {
        out.push(button(
            "installed",
            &r.id,
            "content-repositories.select",
            &format!(
                "{} · {}{}",
                r.manifest.name,
                if r.enabled { "enabled" } else { "disabled" },
                if r.id == w.content_repositories.selected {
                    " · selected"
                } else {
                    ""
                }
            ),
            json!({"id":r.id}),
            available,
        ));
    }
    out.push(button(
        "installed",
        "refresh",
        "content-repositories.refresh",
        "Refresh library list",
        json!({}),
        available,
    ));
    if let Ok(r) = selected(w) {
        out.push(info(
            "installed",
            "selection",
            format!(
                "{}\n{}\nBranch {} · {}\n{} entries",
                r.manifest.name,
                r.root.display(),
                r.branch,
                r.revision.as_deref().unwrap_or("uncommitted"),
                r.manifest.entries.len()
            ),
        ));
        out.push(button(
            "installed",
            "toggle",
            "content-repositories.toggle",
            if r.enabled {
                "Disable selected library"
            } else {
                "Enable selected library"
            },
            json!({}),
            available,
        ));
        out.push(button(
            "installed",
            "remove",
            "content-repositories.remove",
            "Remove from list (keep files)",
            json!({}),
            available,
        ));
        out.push(button(
            "installed",
            "files",
            "content-repositories.files",
            "Show library files…",
            json!({}),
            available,
        ));
    }
    for (index, (label, _)) in SOURCES.iter().enumerate() {
        out.push(button(
            "add",
            &format!("source-{index}"),
            "content-repositories.source",
            &format!("Use TCAD {label} source"),
            json!({"index":index}),
            available,
        ));
    }
    out.push(info("add","sources","These private repos need an account with access. You can use any Git host or local Git path. Nothing downloads until you click Clone."));
    for (key, label) in [
        ("url", "Git URL or local repository path"),
        ("branch", "Branch (blank uses default)"),
        ("destination", "New local working folder"),
    ] {
        out.push(field(w, "add", key, label, None));
    }
    out.push(field(
        w,
        "add",
        "connection",
        "Git connection",
        Some(crate::git_connections_ui::choices(w)),
    ));
    out.push(button(
        "add",
        "connection",
        "git-connections",
        "Manage connections…",
        json!({}),
        available,
    ));
    for (command, label) in [
        ("folder", "Choose parent folder…"),
        ("clone", "Clone library"),
        ("register", "Use an existing local library…"),
    ] {
        out.push(button(
            "add",
            command,
            &format!("content-repositories.{command}"),
            label,
            json!({}),
            available,
        ));
    }
    out.push(info("create","help","Create uses the new working folder above. A fork keeps history and source, starts disabled, and has no publishing destination. Disable the original before enabling its fork."));
    out.push(field(w, "create", "name", "Library name", None));
    out.push(field(w, "create", "kind", "Starter content", Some(kinds())));
    for (key, label) in [
        ("author-name", "Git author name"),
        ("author-email", "Git author email"),
    ] {
        out.push(field(w, "create", key, label, None));
    }
    out.push(button(
        "create",
        "create",
        "content-repositories.create",
        "Create local library",
        json!({}),
        available,
    ));
    out.push(button(
        "create",
        "fork",
        "content-repositories.fork",
        "Fork selected library to new folder",
        json!({}),
        available && selected(w).is_ok(),
    ));
    out.push(button(
        "update",
        "inspect",
        "content-repositories.inspect-update",
        "Inspect source update",
        json!({}),
        available && selected(w).is_ok(),
    ));
    if let Some(review) = &w.content_repositories.update {
        out.push(info("update","review",format!("{} → {}\nNew folder: {}\nAdded: {}\nChanged: {}\nRemoved: {}\nExisting models and the active theme remain unchanged.",review.previous_revision.as_deref().unwrap_or("none"),review.checkout.revision.as_deref().unwrap_or("none"),review.checkout.root.display(),review.added.join(", "),review.changed.join(", "),review.removed.join(", "))));
        out.push(button(
            "update",
            "apply",
            "content-repositories.apply-update",
            "Apply reviewed library update",
            json!({}),
            available,
        ));
    }
    out.push(info("edit","help","Edit the data files in your library folder. Export profiles, finishes, or themes from TCAD into that folder, then add an entry. Saved models do not change when library files change."));
    out.push(field(w, "edit", "entry-kind", "Entry type", Some(kinds())));
    for (key, label) in [
        ("entry-id", "Entry ID (unique within library)"),
        ("entry-name", "Entry name"),
        ("entry-path", "File path inside library"),
    ] {
        out.push(field(w, "edit", key, label, None));
    }
    for (command, label) in [
        ("entry-file", "Choose entry file…"),
        ("add-entry", "Add or replace entry"),
    ] {
        out.push(button(
            "edit",
            command,
            &format!("content-repositories.{command}"),
            label,
            json!({}),
            available && selected(w).is_ok(),
        ));
    }
    if let Ok(r) = selected(w) {
        for e in r.manifest.entries {
            out.push(button(
                "edit",
                &format!("edit-{}", e.id),
                "content-repositories.edit-entry",
                &format!("Edit entry: {} · {}", e.name, e.path),
                json!({"entry":e.id}),
                available,
            ));
            out.push(button(
                "edit",
                &format!("remove-{}", e.id),
                "content-repositories.remove-entry",
                &format!("Remove entry: {} · {}", e.name, e.path),
                json!({"entry":e.id}),
                available,
            ));
        }
    }
    out.push(field(w, "edit", "message", "Version message", None));
    out.push(button(
        "edit",
        "inspect",
        "content-repositories.inspect",
        "Inspect changes",
        json!({}),
        available && selected(w).is_ok(),
    ));
    if let Some(review) = &w.content_repositories.inspection {
        out.push(info(
            "edit",
            "review",
            format!(
                "Branch {} · {}\n{}\n{} files in version: {}",
                review.branch,
                review.head.as_deref().unwrap_or("none"),
                review.status,
                review.file_count,
                review.files.join(", ")
            ),
        ));
        out.push(button(
            "edit",
            "commit",
            "content-repositories.commit",
            "Save reviewed version",
            json!({}),
            available,
        ));
    }
    out.push(info("publish","help","Create an empty private repository on your chosen Git host, then paste its Git URL. Publishing sends all committed library files and Git history. TCAD cannot determine access permissions from a Git URL."));
    out.push(field(
        w,
        "publish",
        "publish-url",
        "Destination Git URL",
        None,
    ));
    out.push(field(
        w,
        "publish",
        "publish-branch",
        "Destination branch",
        None,
    ));
    out.push(button(
        "publish",
        "use-source",
        "content-repositories.use-source-destination",
        "Use library source URL",
        json!({}),
        available && selected(w).is_ok_and(|repository| repository.remote.is_some()),
    ));
    out.push(button(
        "publish",
        "review-destination",
        "content-repositories.review-publish",
        "Review destination",
        json!({}),
        available && selected(w).is_ok(),
    ));
    if let Some((request, _)) = &w.content_repositories.publish {
        out.push(info("publish","review",format!("{}\nBranch: {}\nRemote: {}\nVersion to publish: {}\nAccess: unknown — check on your Git host.",request.clone_url,request.branch,request.expected_remote_commit.as_deref().unwrap_or("new branch"),request.expected_local_commit)));
        out.push(field(
            w,
            "publish",
            "publish-confirmed",
            "I checked who can access this repository",
            None,
        ));
        out.push(button(
            "publish",
            "send",
            "content-repositories.publish",
            "Publish reviewed version and history",
            json!({}),
            available && value(w, "publish-confirmed") == true,
        ));
    }
    out.push(info("installed", "status", &w.content_repositories.status));
    for (index, notice) in w.content_repositories.notices.iter().enumerate() {
        out.push(info("installed", &format!("notice-{index}"), notice));
    }
    if !available {
        out.push(button(
            "installed",
            "cancel",
            "content-repositories.cancel",
            "Cancel operation",
            json!({}),
            true,
        ));
    }
    if let Some(notice) = &w.content_repositories.outcome_notice {
        out.push(info("installed", "last-result", notice));
    }
    out.insert(0, SemanticElement {
        id: "workspace.content-repositories".into(), role: "content-repositories-editor".into(),
        label: "Content libraries".into(), value: None,
        state: if w.settings.as_ref().is_some_and(|settings| settings.category == "application.libraries") {"open"} else {"closed"}.into(),
        commands: vec!["content-repositories".into()], relationships: vec!["settings:category:application.libraries".into()],
        data: json!({"contract":"tcad.content-libraries-ui","version":1,"renderer":"content-repositories-editor","personal":true,"immediate_actions":true}),
    });
    let current_context = context(w);
    for element in &mut out {
        if element.id != "workspace.content-repositories" {
            element
                .relationships
                .push("workspace.content-repositories".into());
        }
        element.data["context"] = current_context.clone();
        element.data["editor"] = json!(w.content_repositories.selected);
        if let Some(actions) = element
            .data
            .get_mut("actions")
            .and_then(Value::as_array_mut)
        {
            for action in actions {
                action["arguments"]["context"] = current_context.clone();
            }
        }
    }
    out
}
pub fn current_value(w: &Workspace, element: &SemanticElement) -> Option<Value> {
    (element.data["setter"] == "content-repositories")
        .then(|| value(w, element.data["key"].as_str().unwrap_or_default()))
}
pub fn set_value(w: &mut Workspace, element: &SemanticElement, incoming: Value) -> Result<()> {
    if !idle(w) {
        return Err(invalid("Wait for the library operation to finish"));
    }
    if element.data["setter"] != "content-repositories" || element.data["context"] != context(w) {
        return Err(invalid(
            "This library field is from an earlier selection or review. Use the current panel.",
        ));
    }
    let key = element.data["key"]
        .as_str()
        .ok_or_else(|| invalid("Unknown library field"))?;
    const FIELDS: &[&str] = &[
        "kind",
        "entry-kind",
        "branch",
        "publish-branch",
        "publish-confirmed",
        "message",
        "url",
        "destination",
        "connection",
        "name",
        "author-name",
        "author-email",
        "entry-id",
        "entry-name",
        "entry-path",
        "publish-url",
    ];
    if !FIELDS.contains(&key)
        || if key == "publish-confirmed" {
            !incoming.is_boolean()
        } else {
            !incoming.is_string() || incoming.as_str().is_some_and(|s| s.len() > 4096)
        }
    {
        return Err(invalid("Invalid library field value"));
    }
    if matches!(key, "kind" | "entry-kind") {
        let _: ContentKind = serde_json::from_value(incoming.clone())?;
    }
    if matches!(key, "publish-url" | "publish-branch" | "connection") {
        w.content_repositories.publish = None;
        set(w, "publish-confirmed", json!(false));
        advance_context(w);
    }
    if key == "publish-confirmed" && w.content_repositories.publish.is_none() {
        return Err(invalid(
            "Review the current destination before confirming access",
        ));
    }
    set(w, key, incoming);
    Ok(())
}
pub fn show(ui: &mut egui::Ui, w: &mut Workspace) {
    let snapshot = semantics(w);
    for (group, label) in SECTIONS {
        controls::inspector_section(
            ui,
            controls::InspectorSection {
                id: &format!("content-repositories/group/{group}"),
                role: "content-library-section",
                classes: &["repository-section"],
                label,
                state: "ready",
                collapse: Some(matches!(group, "installed" | "add")),
                open: None,
            },
            |ui| {
                for element in snapshot.iter().filter(|e| e.data["group"] == group) {
                    match element.role.as_str() {
                        "content-library-field" => {
                            crate::ui::semantic_field(ui, w, element);
                        }
                        "content-library-action" => {
                            if controls::button(
                                ui,
                                &element.id,
                                &element.role,
                                &["semantic-control", "modeling-action"],
                                &element.label,
                                element.state != "disabled",
                                false,
                            )
                            .clicked()
                            {
                                if element.commands[0] == "git-connections" {
                                    crate::ui::action(w, "git-connections");
                                } else {
                                    action(
                                        w,
                                        &element.commands[0],
                                        Some(&element.data["actions"][0]["arguments"]),
                                    );
                                }
                            }
                        }
                        "content-library-information" => {
                            controls::label(
                                ui,
                                &element.id,
                                &element.role,
                                &["repository-detail"],
                                &element.label,
                                &element.state,
                            );
                        }
                        _ => {}
                    }
                }
            },
        );
    }
}
