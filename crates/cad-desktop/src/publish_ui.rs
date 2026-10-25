//! One publication transaction over a saved source snapshot. Geometry and Git
//! remain separate workers; neither worker owns the live modeling session.
use crate::{Workspace, presentation_controls as controls, worker::WorkerClient};
use bevy_egui::egui;
use cad_core::{
    CadError, Result,
    forge::CancelToken,
    git_connections::{self, ConnectionProfile},
    persistence::atomic_write,
    presentation::SemanticElement,
    protocol::{RequestIdentity, WorkerRequest, WorkerResponse},
    publication::{self, Preferences},
    repository_models::{GitAuthor, publication as publisher},
    version_control::{self, Git, VersionControl},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    path::PathBuf,
    sync::{Mutex, mpsc},
};

pub struct State {
    worker: WorkerClient,
    sender: mpsc::Sender<Event>,
    receiver: Mutex<mpsc::Receiver<Event>>,
    context: Option<(Option<PathBuf>, String)>,
    preferences: Preferences,
    configured: bool,
    dirty: bool,
    metadata_pending: bool,
    publish_when_loaded: bool,
    personal_loaded: bool,
    touched: BTreeSet<String>,
    connection: String,
    author_name: String,
    author_email: String,
    message: String,
    status: String,
    pending: Option<Pending>,
    sequence: u64,
    last: Option<Receipt>,
    previous_dialog: Option<String>,
    previous_manager_tree: bool,
}
impl Default for State {
    fn default() -> Self {
        let (sender, receiver) = mpsc::channel();
        Self {
            worker: WorkerClient::new(),
            sender,
            receiver: Mutex::new(receiver),
            context: None,
            preferences: Default::default(),
            configured: false,
            dirty: false,
            metadata_pending: false,
            publish_when_loaded: false,
            personal_loaded: false,
            touched: BTreeSet::new(),
            connection: String::new(),
            author_name: String::new(),
            author_email: String::new(),
            message: String::new(),
            status: String::new(),
            pending: None,
            sequence: 0,
            last: None,
            previous_dialog: None,
            previous_manager_tree: true,
        }
    }
}
#[derive(Clone)]
struct Context {
    project: PathBuf,
    document: String,
    generation: u64,
    source: String,
}
impl Context {
    fn capture(w: &Workspace) -> Result<Self> {
        Ok(Self {
            project: w
                .project
                .clone()
                .ok_or_else(|| invalid("Save this project before publishing"))?,
            document: w.session.committed.id.clone(),
            generation: w.session.generation,
            source: w.session.committed.fingerprint(),
        })
    }
    fn current(&self, w: &Workspace) -> bool {
        w.project.as_ref() == Some(&self.project)
            && w.session.committed.id == self.document
            && w.session.generation == self.generation
            && w.session.committed.fingerprint() == self.source
            && w.session.draft.is_none()
            && w.tool_checkpoint.is_none()
    }
}
#[derive(Clone)]
struct Job {
    context: Context,
    preferences: Preferences,
    connection: Option<ConnectionProfile>,
    author: GitAuthor,
    message: String,
    directory: PathBuf,
    source_repository: Option<PathBuf>,
    base_commit: Option<String>,
}
struct Pending {
    sequence: u64,
    identity: Option<RequestIdentity>,
    job: Job,
    cancel: CancelToken,
}
struct Metadata {
    repository_url: String,
    branch: String,
    folder: String,
    connection: String,
    author: git_connections::GitIdentity,
}
enum Event {
    Metadata(
        (Option<PathBuf>, String),
        std::result::Result<Metadata, String>,
    ),
    Progress(u64, String),
    Published(
        u64,
        Box<std::result::Result<publisher::Publication, String>>,
    ),
}
#[derive(Clone, Serialize, Deserialize)]
struct Receipt {
    version: u32,
    document: String,
    preferences: Preferences,
    author: GitAuthor,
    result: publisher::Publication,
}
#[derive(Serialize, Deserialize)]
struct PersonalChoices {
    version: u32,
    connection: String,
    author: GitAuthor,
}
fn invalid(message: impl Into<String>) -> CadError {
    CadError::Invalid(message.into())
}
pub fn busy(w: &Workspace) -> bool {
    w.publish.pending.is_some()
}
fn status(w: &mut Workspace, message: impl Into<String>) {
    let message = message.into();
    w.publish.status = message.clone();
    w.status = message;
}
fn personal_root() -> Result<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(|home| PathBuf::from(home).join(".tcad/publishing"))
        .ok_or_else(|| invalid("The personal Publish folder is unavailable"))
}
fn receipt_path(project: &std::path::Path, document: &str) -> Result<PathBuf> {
    let root = cad_core::persistence::project_root(project);
    let root = root.canonicalize().unwrap_or(root);
    let key = serde_json::to_vec(&(root, document))?;
    Ok(personal_root()?.join(format!("{:x}.json", Sha256::digest(key))))
}
fn same_destination(a: &Preferences, b: &Preferences) -> bool {
    a.repository_url == b.repository_url
        && a.branch == b.branch
        && a.project_folder == b.project_folder
}
fn load_context(w: &mut Workspace) {
    let key = (w.project.clone(), w.session.committed.id.clone());
    if w.publish.context.as_ref() == Some(&key) || busy(w) {
        return;
    }
    w.publish = State::default();
    w.publish.context = Some(key.clone());
    if let Some(project) = &key.0 {
        let root = cad_core::persistence::project_root(project);
        match publication::read_preferences(&root) {
            Ok(preferences) => {
                w.publish.configured = !preferences.repository_url.is_empty();
                w.publish.preferences = preferences;
            }
            Err(error) => {
                w.publish.status = format!(
                    "Cannot read Publish choices: {error}. Correct the choices and save them again."
                )
            }
        }
        if let Ok(path) = receipt_path(project, &key.1)
            && let Ok(metadata) = std::fs::symlink_metadata(&path)
            && metadata.is_file()
            && !metadata.file_type().is_symlink()
            && metadata.len() <= 4 * 1024 * 1024
            && let Ok(bytes) = std::fs::read(path)
            && let Ok(receipt) = serde_json::from_slice::<Receipt>(&bytes)
            && receipt.version == 1
            && receipt.document == key.1
        {
            w.publish.last = Some(receipt);
        }
        if let Ok(path) =
            receipt_path(project, &key.1).map(|path| path.with_extension("settings.json"))
            && let Ok(metadata) = std::fs::symlink_metadata(&path)
            && metadata.is_file()
            && !metadata.file_type().is_symlink()
            && metadata.len() <= 64 * 1024
            && let Ok(bytes) = std::fs::read(path)
            && let Ok(personal) = serde_json::from_slice::<PersonalChoices>(&bytes)
            && personal.version == 1
        {
            w.publish.connection = personal.connection;
            w.publish.author_name = personal.author.name;
            w.publish.author_email = personal.author.email;
            w.publish.personal_loaded = true;
        }
    }
    w.publish.metadata_pending = true;
    let sender = w.publish.sender.clone();
    let root = w.root();
    std::thread::spawn(move || {
        let result = (|| -> Result<Metadata> {
            let source_repository = version_control::repository_root(&root).ok();
            let repository_url = source_repository
                .as_ref()
                .and_then(|repo| Git.command(repo, &["remote", "get-url", "origin"]).ok())
                .and_then(|url| git_connections::RepositoryAddress::parse(url.trim()).ok())
                .map(|address| address.git_url)
                .unwrap_or_default();
            let branch = source_repository
                .as_ref()
                .and_then(|repo| Git.command(repo, &["symbolic-ref", "--short", "HEAD"]).ok())
                .unwrap_or_default()
                .trim()
                .to_owned();
            let folder = source_repository
                .as_ref()
                .and_then(|repo| root.strip_prefix(repo).ok())
                .map(|relative| relative.to_string_lossy().replace('\\', "/"))
                .unwrap_or_else(|| "model".into());
            let connection = if root.exists() {
                git_connections::profile_for_repository(&root)?
            } else {
                None
            }
            .map(|profile| profile.id)
            .unwrap_or_default();
            let author = git_connections::read_author(root.exists().then_some(root.as_path()))?;
            Ok(Metadata {
                repository_url,
                branch,
                folder,
                connection,
                author,
            })
        })()
        .map_err(|error| error.to_string());
        if sender.send(Event::Metadata(key, result)).is_ok() {
            crate::redraw::wake();
        }
    });
}
fn open(w: &mut Workspace) {
    load_context(w);
    if w.dialog.as_deref() != Some("publish") {
        w.publish.previous_dialog = w.dialog.clone();
        w.publish.previous_manager_tree = w.manager_tree;
    }
    w.dialog = Some("publish".into());
    w.dialog_title = "Publish".into();
    w.manager_tree = false;
}
fn close(w: &mut Workspace) {
    w.publish.publish_when_loaded = false;
    if w.dialog.as_deref() == Some("publish") {
        w.dialog = w.publish.previous_dialog.take();
        w.manager_tree = w.publish.previous_manager_tree;
    }
}
fn ready(w: &Workspace) -> Result<()> {
    if busy(w) {
        return Err(invalid("A publication is already in progress"));
    }
    if w.session.draft.is_some()
        || w.tool_checkpoint.is_some()
        || w.sketch.is_some()
        || w.sketch3d.is_some()
        || w.settings.is_some()
        || crate::drawing_interaction_ui::active(w)
    {
        return Err(invalid("Apply or cancel the active edit before publishing"));
    }
    if crate::git_ui::operation_pending(w)
        || crate::git_ui::recovery_pending(w)
        || crate::repositories_ui::operation_pending(w)
        || crate::readme_preview_ui::busy(w)
    {
        return Err(invalid(
            "Finish the current Git operation or merge recovery before publishing",
        ));
    }
    if w.publish.metadata_pending {
        return Err(invalid(
            "Reading this project's Git choices; Publish will be available in a moment",
        ));
    }
    Ok(())
}
fn save_choices(w: &mut Workspace) -> Result<()> {
    if busy(w) {
        return Err(invalid(
            "Wait for publication to finish before changing its choices",
        ));
    }
    w.publish.preferences.validate()?;
    if w.project.is_none() {
        if w.session.draft.is_some() || w.tool_checkpoint.is_some() {
            return Err(invalid(
                "Finish the active edit and save the project before saving Publish choices",
            ));
        }
        if !crate::ui::save_now(w, false) {
            return Err(invalid(w.status.clone()));
        }
    }
    let root = w.root();
    crate::git_connections_ui::resolve(w, &w.publish.connection)?;
    publication::save_preferences(&root, &w.publish.preferences)?;
    save_personal_choices(w)?;
    let context = (w.project.clone(), w.session.committed.id.clone());
    if w.publish.context.as_ref() != Some(&context) {
        w.publish.metadata_pending = false;
    }
    w.publish.context = Some(context);
    w.publish.configured = !w.publish.preferences.repository_url.is_empty();
    w.publish.dirty = false;
    Ok(())
}
fn save_personal_choices(w: &mut Workspace) -> Result<()> {
    let project = w
        .project
        .as_ref()
        .ok_or_else(|| invalid("Save the project before saving Publish choices"))?;
    let personal = PersonalChoices {
        version: 1,
        connection: w.publish.connection.clone(),
        author: GitAuthor {
            name: w.publish.author_name.clone(),
            email: w.publish.author_email.clone(),
        },
    };
    atomic_write(
        &receipt_path(project, &w.session.committed.id)?.with_extension("settings.json"),
        &serde_json::to_vec_pretty(&personal)?,
    )?;
    w.publish.personal_loaded = true;
    Ok(())
}
fn run(w: &mut Workspace) -> Result<()> {
    ready(w)?;
    w.publish.preferences.validate_publish()?;
    if w.publish.last.as_ref().is_some_and(|receipt| {
        !receipt.result.pushed
            && receipt.preferences == w.publish.preferences
            && receipt.result.source_fingerprint == w.session.committed.fingerprint()
    }) {
        return retry(w);
    }
    let connection = crate::git_connections_ui::resolve(w, &w.publish.connection)?;
    let author = GitAuthor {
        name: w.publish.author_name.trim().into(),
        email: w.publish.author_email.trim().into(),
    };
    if author.name.is_empty() || author.email.is_empty() {
        return Err(invalid(
            "Enter your Git author name and email in Publish settings",
        ));
    }
    if !crate::ui::save_now(w, false) {
        return Err(invalid(w.status.clone()));
    }
    save_choices(w)?;
    let context = Context::capture(w)?;
    let directory = personal_root()?.join("jobs").join(cad_core::id());
    std::fs::create_dir_all(&directory)?;
    let preferences = w.publish.preferences.clone();
    let base_commit = w
        .publish
        .last
        .as_ref()
        .filter(|receipt| same_destination(&receipt.preferences, &preferences))
        .and_then(|receipt| {
            if receipt.result.pushed {
                Some(receipt.result.commit.clone())
            } else {
                receipt.result.expected_remote_commit.clone()
            }
        });
    let message = if w.publish.message.trim().is_empty() {
        format!(
            "Publish {} · revision {}",
            w.session.committed.name, w.session.committed.revision
        )
    } else {
        w.publish.message.trim().to_owned()
    };
    let job = Job {
        context,
        preferences: preferences.clone(),
        connection,
        author,
        message,
        directory: directory.clone(),
        source_repository: Some(w.root()),
        base_commit,
    };
    w.publish.sequence = w.publish.sequence.wrapping_add(1);
    let mut identity = w.session.identity();
    identity.generation = w.publish.sequence;
    w.publish.worker.submit(WorkerRequest::PublishGenerate {
        identity: identity.clone(),
        document: w.session.committed.clone(),
        project_root: w.root().to_string_lossy().into(),
        staging_directory: directory.join("package").to_string_lossy().into(),
        preferences: Box::new(preferences),
        display_tolerance_mm: w.display_tolerance(),
    });
    w.publish.pending = Some(Pending {
        sequence: w.publish.sequence,
        identity: Some(identity),
        job,
        cancel: CancelToken::default(),
    });
    status(
        w,
        "Generating drawings, BOM and exports from the saved model…",
    );
    Ok(())
}
fn start_git(w: &mut Workspace, package: publication::GeneratedPackage) -> Result<()> {
    let pending = w
        .publish
        .pending
        .as_mut()
        .ok_or_else(|| invalid("The publication was cancelled"))?;
    let job = pending.job.clone();
    if publication::read_preferences(&cad_core::persistence::project_root(&job.context.project))?
        != job.preferences
    {
        return Err(invalid(
            "Publish choices changed on disk. Publish again with the current choices",
        ));
    }
    let request = publisher::Request {
        source_manifest: package.source_manifest,
        expected_fingerprint: package.source_sha256,
        artifact_root: package.root,
        artifacts: package.files,
        clone_url: job.preferences.repository_url.clone(),
        branch: job.preferences.branch.clone(),
        folder: job.preferences.project_folder.clone(),
        destination: job.directory.join("repository"),
        commit_message: job.message.clone(),
        author: job.author.clone(),
        base_commit: job.base_commit,
        source_repository: job.source_repository,
    };
    pending.identity = None;
    let sequence = pending.sequence;
    let cancel = pending.cancel.clone();
    let sender = w.publish.sender.clone();
    std::thread::spawn(move || {
        let result = publisher::publish(&request, job.connection.as_ref(), &cancel, |message| {
            if sender
                .send(Event::Progress(sequence, message.into()))
                .is_ok()
            {
                crate::redraw::wake();
            }
        })
        .map_err(|error| error.to_string());
        if sender
            .send(Event::Published(sequence, Box::new(result)))
            .is_ok()
        {
            crate::redraw::wake();
        }
    });
    status(w, "Committing the model and generated files, then pushing…");
    Ok(())
}
fn retry(w: &mut Workspace) -> Result<()> {
    ready(w)?;
    let receipt = w
        .publish
        .last
        .clone()
        .filter(|receipt| !receipt.result.pushed)
        .ok_or_else(|| invalid("There is no saved publication waiting for upload"))?;
    if !same_destination(&receipt.preferences, &w.publish.preferences) {
        return Err(invalid(
            "This saved commit belongs to the previous Publish destination. Restore that repository, branch and project folder to retry its upload",
        ));
    }
    let connection = crate::git_connections_ui::resolve(w, &w.publish.connection)?;
    let context = Context::capture(w)?;
    save_personal_choices(w)?;
    let job = Job {
        context,
        preferences: receipt.preferences,
        connection: connection.clone(),
        author: receipt.author,
        message: String::new(),
        directory: receipt.result.root.clone(),
        source_repository: None,
        base_commit: None,
    };
    w.publish.sequence = w.publish.sequence.wrapping_add(1);
    let sequence = w.publish.sequence;
    let cancel = CancelToken::default();
    let worker_cancel = cancel.clone();
    let sender = w.publish.sender.clone();
    w.publish.pending = Some(Pending {
        sequence,
        identity: None,
        job,
        cancel,
    });
    std::thread::spawn(move || {
        let result = publisher::retry_push(
            &receipt.result,
            connection.as_ref(),
            &worker_cancel,
            |message| {
                if sender
                    .send(Event::Progress(sequence, message.into()))
                    .is_ok()
                {
                    crate::redraw::wake();
                }
            },
        )
        .map_err(|error| error.to_string());
        if sender
            .send(Event::Published(sequence, Box::new(result)))
            .is_ok()
        {
            crate::redraw::wake();
        }
    });
    status(w, "Uploading the saved publication commit…");
    Ok(())
}
pub fn poll(w: &mut Workspace) {
    load_context(w);
    let events = w
        .publish
        .receiver
        .lock()
        .map(|receiver| receiver.try_iter().collect::<Vec<_>>())
        .unwrap_or_default();
    for event in events {
        match event {
            Event::Metadata(context, result) if w.publish.context.as_ref() == Some(&context) => {
                w.publish.metadata_pending = false;
                match result {
                    Ok(metadata) => {
                        if !w.publish.personal_loaded {
                            if !w.publish.touched.contains("connection")
                                && (!w.publish.configured
                                    || w.publish.preferences.repository_url
                                        == metadata.repository_url)
                            {
                                w.publish.connection = metadata.connection;
                            }
                            if !w.publish.touched.contains("author-name") {
                                w.publish.author_name = metadata.author.name;
                            }
                            if !w.publish.touched.contains("author-email") {
                                w.publish.author_email = metadata.author.email;
                            }
                            if let Some(last) = &w.publish.last {
                                if w.publish.author_name.is_empty()
                                    && !w.publish.touched.contains("author-name")
                                {
                                    w.publish.author_name = last.author.name.clone();
                                }
                                if w.publish.author_email.is_empty()
                                    && !w.publish.touched.contains("author-email")
                                {
                                    w.publish.author_email = last.author.email.clone();
                                }
                            }
                        }
                        if !w.publish.dirty && !w.publish.configured {
                            w.publish.preferences.repository_url = metadata.repository_url;
                            if !metadata.branch.is_empty() {
                                w.publish.preferences.branch = metadata.branch;
                            }
                            w.publish.preferences.project_folder = metadata.folder;
                        }
                    }
                    Err(error) => {
                        w.publish.status =
                            format!("Enter your Publish connection and author details. {error}")
                    }
                }
                if w.publish.publish_when_loaded {
                    w.publish.publish_when_loaded = false;
                    if let Err(error) = run(w) {
                        open(w);
                        status(w, error.to_string());
                    }
                }
            }
            Event::Progress(sequence, message)
                if w.publish
                    .pending
                    .as_ref()
                    .is_some_and(|pending| pending.sequence == sequence) =>
            {
                status(w, message)
            }
            Event::Published(sequence, result)
                if w.publish
                    .pending
                    .as_ref()
                    .is_some_and(|pending| pending.sequence == sequence) =>
            {
                let pending = w
                    .publish
                    .pending
                    .take()
                    .expect("matched pending publication");
                match *result {
                    Ok(result) => {
                        let receipt = Receipt {
                            version: 1,
                            document: pending.job.context.document.clone(),
                            preferences: pending.job.preferences,
                            author: pending.job.author,
                            result,
                        };
                        let mut message = if receipt.result.pushed {
                            format!(
                                "Published {} to {}. Files and commit: {}",
                                &receipt.result.commit[..receipt.result.commit.len().min(12)],
                                receipt.result.branch,
                                receipt.result.root.display()
                            )
                        } else {
                            format!(
                                "The publication is committed locally, but upload was not confirmed. Use Retry saved upload to check and finish. Files: {}",
                                receipt.result.root.display()
                            )
                        };
                        if let Some(error) = &receipt.result.push_error {
                            message.push_str(&format!(" {error}"));
                        }
                        for notice in &receipt.result.notices {
                            message.push_str(&format!(" {notice}"));
                        }
                        let saved = receipt_path(&pending.job.context.project, &receipt.document)
                            .and_then(|path| {
                                atomic_write(&path, &serde_json::to_vec_pretty(&receipt)?)
                            });
                        if let Err(error) = saved {
                            message.push_str(&format!(
                                " Could not save the local publication receipt: {error}"
                            ));
                        }
                        w.publish.last = Some(receipt);
                        status(w, message);
                    }
                    Err(error) => status(
                        w,
                        format!(
                            "Publication stopped: {error}. Saved model and generated files were retained at {}",
                            pending.job.directory.display()
                        ),
                    ),
                }
            }
            _ => {}
        }
    }
    for reply in w.publish.worker.drain() {
        let Some(pending) = &w.publish.pending else {
            continue;
        };
        let Some(expected) = &pending.identity else {
            continue;
        };
        let result = match reply {
            WorkerResponse::PublishGenerated { identity, package } if &identity == expected => {
                if !pending.job.context.current(w) {
                    Err(invalid(
                        "The model changed during generation; no commit or push was made",
                    ))
                } else {
                    start_git(w, *package)
                }
            }
            WorkerResponse::Error { identity, message }
                if identity
                    .as_ref()
                    .is_none_or(|identity| identity == expected) =>
            {
                Err(invalid(message))
            }
            _ => continue,
        };
        if let Err(error) = result {
            w.publish.pending = None;
            status(
                w,
                format!("Publication stopped before commit or push: {error}"),
            );
        }
    }
}
pub fn action(w: &mut Workspace, command: &str, _: Option<&Value>) -> bool {
    if command != "file.publish" && !command.starts_with("publish.") {
        return false;
    }
    load_context(w);
    let result = match command {
        "file.publish" => {
            if busy(w) || !w.publish.configured || w.publish.dirty {
                open(w);
                Ok(())
            } else if w.publish.metadata_pending {
                w.publish.publish_when_loaded = true;
                status(w, "Reading this project's Git choices before publishing…");
                Ok(())
            } else {
                run(w)
            }
        }
        "publish.settings" => {
            open(w);
            Ok(())
        }
        "publish.run" => run(w),
        "publish.save-settings" => {
            save_choices(w).map(|()| status(w, "Publish choices saved for this project"))
        }
        "publish.retry" => retry(w),
        "publish.close" => {
            close(w);
            Ok(())
        }
        "publish.cancel" => {
            w.publish.publish_when_loaded = false;
            if let Some(pending) = &w.publish.pending {
                pending.cancel.cancel();
            }
            if w.publish
                .pending
                .as_ref()
                .is_some_and(|pending| pending.identity.is_some())
            {
                w.publish.worker.cancel();
                w.publish.pending = None;
                status(
                    w,
                    "Generation cancelled. The saved model is unchanged and no upload was started",
                );
            } else if busy(w) {
                status(
                    w,
                    "Stopping publication. If upload already finished, its commit remains on the remote branch",
                );
            }
            Ok(())
        }
        "publish.show-files" => {
            if let Some(last) = &w.publish.last {
                #[cfg(target_os = "macos")]
                let command = "open";
                #[cfg(target_os = "windows")]
                let command = "explorer";
                #[cfg(not(any(target_os = "macos", target_os = "windows")))]
                let command = "xdg-open";
                std::process::Command::new(command)
                    .arg(&last.result.root)
                    .spawn()
                    .map(|_| ())
                    .map_err(CadError::from)
            } else {
                Err(invalid("Publish once to create the output folder"))
            }
        }
        _ => Err(invalid("Unknown Publish action")),
    };
    if let Err(error) = result {
        open(w);
        status(w, error.to_string());
    }
    true
}
fn field(
    w: &Workspace,
    key: &str,
    label: &str,
    value: Value,
    choices: Option<Vec<Value>>,
) -> SemanticElement {
    let group = match key {
        "repository-url" | "branch" | "project-folder" | "connection" => "destination",
        "message" | "author-name" | "author-email" => "commit",
        _ => "outputs",
    };
    let mut element = SemanticElement {
        id: format!("publish/{key}"),
        role: "publish-field".into(),
        label: label.into(),
        value: Some(value.to_string()),
        state: if busy(w) { "disabled" } else { "ready" }.into(),
        commands: vec![],
        relationships: vec!["workspace.publish".into()],
        data: json!({"setter":"publish","key":key,"group":group,"json":value,"editable":!busy(w),"document":w.session.committed.id,"project":w.project,"accessibility_label":label}),
    };
    if let Some(choices) = choices {
        element.data["choices"] = json!(choices);
    }
    element
}
fn note(key: &str, label: impl Into<String>) -> SemanticElement {
    SemanticElement {
        id: format!("publish/{key}"),
        role: "publish-information".into(),
        label: label.into(),
        value: None,
        state: "ready".into(),
        commands: vec![],
        relationships: vec!["workspace.publish".into()],
        data: json!({"group":match key {"destination-help"=>"destination","drawing-source"=>"outputs",_=>""}}),
    }
}
pub fn semantics(w: &Workspace) -> Vec<SemanticElement> {
    let preferences = &w.publish.preferences;
    let ready = ready(w);
    let mut out = vec![SemanticElement {
        id: "workspace.publish".into(),
        role: "publish-editor".into(),
        label: "Publish".into(),
        value: None,
        state: if busy(w) { "busy" } else { "ready" }.into(),
        commands: vec!["file.publish".into(), "publish.settings".into()],
        relationships: vec![],
        data: json!({"renderer":"publish-editor","contract":"tcad.publication","version":1,"project":w.project,"actions":[{"command":"file.publish","label":if busy(w){"Show publication progress"}else{"Publish"},"enabled":busy(w)||!w.publish.configured||ready.is_ok(),"disabled_reason":ready.as_ref().err().map(ToString::to_string)},{"command":"publish.settings","label":"Publish settings","enabled":true}]}),
    }];
    out.push(note("help", "Save the model, generate the selected files, commit them together, and push. These destination and output choices are saved with the project."));
    for (group, label) in [
        ("destination", "Destination"),
        ("outputs", "Generated files"),
        ("commit", "Commit details"),
    ] {
        out.push(SemanticElement {
            id: format!("publish/group/{group}"),
            role: "publish-section".into(),
            label: label.into(),
            value: None,
            state: "ready".into(),
            commands: vec![],
            relationships: vec!["workspace.publish".into()],
            data: json!({"group":group}),
        });
    }
    out.push(field(
        w,
        "repository-url",
        "Repository",
        json!(preferences.repository_url),
        None,
    ));
    out.push(field(
        w,
        "branch",
        "Branch",
        json!(preferences.branch),
        None,
    ));
    out.push(field(
        w,
        "project-folder",
        "Folder in repository",
        json!(preferences.project_folder),
        None,
    ));
    out.push(note("destination-help","Leave the project folder empty for the repository root. The branch is updated without force-pushing. Repository access follows the chosen Git connection."));
    out.push(field(
        w,
        "connection",
        "Connection",
        json!(w.publish.connection),
        Some(crate::git_connections_ui::choices(w)),
    ));
    for (key, label) in [
        ("drawing_pdf", "Drawing PDF"),
        ("drawing_dxf", "Drawing DXF"),
        ("bom_csv", "BOM CSV"),
        ("step", "STEP model"),
        ("stl", "STL mesh"),
        ("sketch_dxf", "Sketch profiles DXF"),
        ("flat_pattern_dxf", "Sheet-metal flat pattern DXF"),
        ("cut_list_csv", "Weldment cut list CSV"),
    ] {
        let values = serde_json::to_value(&preferences.outputs).unwrap_or_default();
        out.push(field(w, key, label, values[key].clone(), None));
    }
    if w.session.committed.drawings.is_empty()
        && (preferences.outputs.drawing_pdf || preferences.outputs.drawing_dxf)
    {
        out.push(note("drawing-source","No drawing sheets are saved. Publish will generate the standard projection sheet; dimensions and tolerances must be authored in Drawings."));
    } else {
        out.push(note(
            "drawing-source",
            format!(
                "Drawing outputs use all {} saved sheets and the current model configuration.",
                w.session.committed.drawings.len()
            ),
        ));
    }
    out.push(field(
        w,
        "message",
        "Commit message",
        json!(w.publish.message),
        None,
    ));
    out.push(field(
        w,
        "author-name",
        "Git author name",
        json!(w.publish.author_name),
        None,
    ));
    out.push(field(
        w,
        "author-email",
        "Git author email",
        json!(w.publish.author_email),
        None,
    ));
    out.push(note(
        "message-help",
        "Leave the message blank to use the model revision.",
    ));
    out.push(note("status", &w.publish.status));
    if let Some(last) = &w.publish.last {
        out.push(note(
            "last",
            format!(
                "Last publication: {} · {} · {}",
                last.preferences.repository_url,
                last.result.branch,
                if last.result.pushed {
                    "uploaded"
                } else {
                    "upload not confirmed"
                }
            ),
        ));
    }
    for (command, label, enabled, reason) in [
        (
            "publish.run",
            "Publish",
            ready.is_ok(),
            ready
                .as_ref()
                .err()
                .map(ToString::to_string)
                .unwrap_or_default(),
        ),
        (
            "publish.save-settings",
            "Save choices",
            !busy(w),
            "Wait for publication to finish".into(),
        ),
        (
            "publish.retry",
            "Retry saved upload",
            ready.is_ok()
                && w.publish.last.as_ref().is_some_and(|last| {
                    !last.result.pushed && same_destination(&last.preferences, preferences)
                }),
            "No completed local commit is waiting for upload".into(),
        ),
        (
            "publish.show-files",
            "Show published files…",
            w.publish.last.is_some(),
            "Publish a model first".into(),
        ),
        (
            "git-connections",
            "Manage connections…",
            !busy(w),
            "Wait for publication to finish".into(),
        ),
        (
            "publish.cancel",
            "Cancel publication",
            busy(w),
            "No publication is in progress".into(),
        ),
        ("publish.close", "Close", true, String::new()),
    ] {
        out.push(SemanticElement { id:format!("publish/action/{command}"),role:"publish-action".into(),label:label.into(),value:None,state:if enabled {"ready"}else{"disabled"}.into(),commands:vec![command.into()],relationships:vec!["workspace.publish".into()],data:json!({"enabled":enabled,"disabled_reason":if enabled {String::new()}else{reason},"accessibility_label":label}) });
    }
    out
}
pub fn current_value(w: &Workspace, element: &SemanticElement) -> Option<Value> {
    if element.data["setter"] != "publish" {
        return None;
    }
    Some(match element.data["key"].as_str()? {
        "repository-url" => json!(w.publish.preferences.repository_url),
        "branch" => json!(w.publish.preferences.branch),
        "project-folder" => json!(w.publish.preferences.project_folder),
        "connection" => json!(w.publish.connection),
        "message" => json!(w.publish.message),
        "author-name" => json!(w.publish.author_name),
        "author-email" => json!(w.publish.author_email),
        key => serde_json::to_value(&w.publish.preferences.outputs)
            .ok()?
            .get(key)?
            .clone(),
    })
}
pub fn set_value(w: &mut Workspace, element: &SemanticElement, value: Value) -> Result<()> {
    if busy(w)
        || element.data["setter"] != "publish"
        || element.data["document"] != w.session.committed.id
        || element.data["project"] != json!(w.project)
    {
        return Err(invalid(
            "Use the current Publish settings after the running operation finishes",
        ));
    }
    let key = element.data["key"]
        .as_str()
        .ok_or_else(|| invalid("Unknown Publish field"))?;
    if matches!(
        key,
        "drawing_pdf"
            | "drawing_dxf"
            | "bom_csv"
            | "step"
            | "stl"
            | "sketch_dxf"
            | "flat_pattern_dxf"
            | "cut_list_csv"
    ) {
        if !value.is_boolean() {
            return Err(invalid("Choose whether to include this output"));
        }
        let mut outputs = serde_json::to_value(&w.publish.preferences.outputs)?;
        outputs[key] = value;
        w.publish.preferences.outputs = serde_json::from_value(outputs)?;
    } else {
        let value = value
            .as_str()
            .filter(|value| value.len() <= 4096 && !value.chars().any(|c| c == '\0'))
            .ok_or_else(|| invalid("Enter a short text value"))?
            .to_owned();
        match key {
            "repository-url" => w.publish.preferences.repository_url = value,
            "branch" => w.publish.preferences.branch = value,
            "project-folder" => w.publish.preferences.project_folder = value,
            "connection" => w.publish.connection = value,
            "message" => w.publish.message = value,
            "author-name" => w.publish.author_name = value,
            "author-email" => w.publish.author_email = value,
            _ => return Err(invalid("Unknown Publish field")),
        }
    }
    w.publish.dirty = true;
    w.publish.touched.insert(key.into());
    w.publish.publish_when_loaded = false;
    Ok(())
}
pub fn show(ui: &mut egui::Ui, w: &mut Workspace) {
    show_actions(ui, w);
    show_contents(ui, w);
}
pub fn show_actions(ui: &mut egui::Ui, w: &mut Workspace) {
    let elements = semantics(w);
    let publishing = busy(w);
    ui.horizontal_wrapped(|ui| {
        for element in elements.iter().filter(|element| {
            element.role == "publish-action"
                && matches!(
                    element.commands.first().map(String::as_str),
                    Some("publish.run" | "publish.save-settings" | "publish.close")
                )
                || publishing
                    && element.commands.first().map(String::as_str) == Some("publish.cancel")
        }) {
            render_element(ui, w, element);
        }
    });
}
pub fn show_contents(ui: &mut egui::Ui, w: &mut Workspace) {
    let elements = semantics(w);
    for element in elements
        .iter()
        .filter(|element| element.role == "publish-information" && element.data["group"] == "")
    {
        render_element(ui, w, element);
    }
    for (group, label) in [
        ("destination", "Destination"),
        ("outputs", "Generated files"),
        ("commit", "Commit details"),
    ] {
        controls::inspector_section(
            ui,
            controls::InspectorSection {
                id: &format!("publish/group/{group}"),
                role: "publish-section",
                classes: &["compact"],
                label,
                state: "ready",
                collapse: Some(
                    group != "commit"
                        || w.publish.author_name.is_empty()
                        || w.publish.author_email.is_empty(),
                ),
                open: None,
            },
            |ui| {
                for element in elements
                    .iter()
                    .filter(|element| element.data["group"] == group)
                {
                    render_element(ui, w, element);
                }
            },
        );
    }
    ui.horizontal_wrapped(|ui| {
        for element in elements.iter().filter(|element| {
            element.role == "publish-action"
                && !matches!(
                    element.commands.first().map(String::as_str),
                    Some(
                        "publish.run"
                            | "publish.save-settings"
                            | "publish.close"
                            | "publish.cancel"
                    )
                )
        }) {
            render_element(ui, w, element);
        }
    });
}
fn render_element(ui: &mut egui::Ui, w: &mut Workspace, element: &SemanticElement) {
    match element.role.as_str() {
        "publish-field" => crate::ui::semantic_field(ui, w, element),
        "publish-information" => {
            controls::bounded_label(
                ui,
                &element.id,
                &element.role,
                &[],
                &element.label,
                &element.state,
                ui.available_width(),
            );
        }
        "publish-action"
            if controls::button(
                ui,
                &element.id,
                &element.role,
                &["semantic-control"],
                &element.label,
                element.data["enabled"].as_bool().unwrap_or(false),
                false,
            )
            .on_hover_text(element.data["disabled_reason"].as_str().unwrap_or_default())
            .clicked() =>
        {
            crate::ui::semantic_action(w, element, &element.commands[0]);
        }
        _ => {}
    }
}
