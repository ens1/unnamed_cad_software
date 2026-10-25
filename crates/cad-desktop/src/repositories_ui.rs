//! Git-first repository workflow with optional forge conveniences.
//! Network work runs away from the model thread. Credentials never enter UI state.
use crate::{Workspace, presentation_controls as controls};
use bevy_egui::egui;
use cad_core::{
    CadError, Document, Id, Result,
    forge::{self, CancelToken, GitHub},
    git_connections::{ConnectionProfile, RepositoryAddress},
    presentation::SemanticElement,
    repository_models::{self as models, GitAuthor, ModelEntry},
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{Mutex, mpsc},
};

#[derive(Default)]
pub struct State {
    receiver: Mutex<Option<mpsc::Receiver<Event>>>,
    pending: Option<Pending>,
    account: Option<forge::AccountStatus>,
    owners: Vec<forge::Owner>,
    owners_next: Option<u32>,
    repositories: Vec<forge::Repository>,
    repositories_next: Option<u32>,
    selected: Option<forge::Repository>,
    checkout: Option<Checkout>,
    review: Option<PublishReview>,
    create_review: Option<(forge::CreateRepository, String)>,
    focus_section: Option<String>,
    fields: BTreeMap<String, Value>,
    device_code: Option<String>,
    status: String,
    notices: Vec<String>,
}

struct Pending {
    cancel: CancelToken,
    context: Context,
    publishing: bool,
}
#[derive(Clone)]
struct Context {
    project: Option<PathBuf>,
    document: Id,
    generation: u64,
    fingerprint: String,
}
impl Context {
    fn capture(w: &Workspace) -> Self {
        Self {
            project: w.project.clone(),
            document: w.session.visible().id.clone(),
            generation: w.session.generation,
            fingerprint: w.session.visible().fingerprint(),
        }
    }
    fn current(&self, w: &Workspace) -> bool {
        self.project == w.project
            && self.document == w.session.visible().id
            && self.generation == w.session.generation
            && self.fingerprint == w.session.visible().fingerprint()
    }
}
struct Checkout {
    root: PathBuf,
    branch: String,
    projects: Vec<ModelEntry>,
}
#[derive(Clone)]
enum Destination {
    New(forge::CreateRepository),
    Existing(forge::Repository),
    Git(String),
    Local,
}
#[derive(Clone)]
struct PublishReview {
    context: Context,
    account: Option<String>,
    destination: Destination,
    connection: Option<ConnectionProfile>,
    remote_commit: Option<String>,
    local: PathBuf,
    branch: Option<String>,
    folder: String,
    message: String,
    author: GitAuthor,
}
impl PublishReview {
    fn source_request(&self) -> Result<models::PublishSourceRequest> {
        Ok(models::PublishSourceRequest {
            source_manifest: self
                .context
                .project
                .clone()
                .ok_or_else(|| invalid("The saved source is missing"))?,
            expected_fingerprint: self.context.fingerprint.clone(),
            destination: self.local.clone(),
            branch: self.branch.clone(),
            folder: self.folder.clone(),
            commit_message: self.message.clone(),
            author: self.author.clone(),
        })
    }
    fn private(&self) -> Option<bool> {
        match &self.destination {
            Destination::New(request) => Some(request.private),
            Destination::Existing(repository) => Some(repository.private),
            Destination::Git(_) | Destination::Local => None,
        }
    }
    fn name(&self) -> String {
        match &self.destination {
            Destination::New(request) => format!("{}/{}", request.owner, request.name),
            Destination::Existing(repository) => repository.full_name.clone(),
            Destination::Git(url) => url.clone(),
            Destination::Local => "Local repository".into(),
        }
    }
    fn disclosure_needed(&self) -> bool {
        !matches!(self.destination, Destination::Local) && self.private() != Some(true)
    }
}
enum Event {
    Progress(forge::Progress),
    Status(String),
    Created(forge::Repository),
    Finished(std::result::Result<Outcome, String>),
}
enum Outcome {
    Account(forge::AccountStatus),
    Owners(forge::OwnerPage, bool),
    Repositories(forge::RepositoryPage, bool),
    Repository(forge::Repository),
    Created(forge::Repository),
    Checkout(Checkout, Vec<String>),
    Opened(Box<Document>, PathBuf),
    Reviewed(Box<PublishReview>),
    Published(models::PublishResult),
    Author(GitAuthor, bool),
    Initialized(PathBuf, String),
}

fn invalid(message: impl Into<String>) -> CadError {
    CadError::Invalid(message.into())
}
fn value(w: &Workspace, key: &str) -> Value {
    w.repositories
        .fields
        .get(key)
        .cloned()
        .unwrap_or_else(|| match key {
            "new-private" => json!(true),
            "public-confirmed" => json!(false),
            "publish-mode" => json!("url"),
            "publish-message" => json!("Publish TCAD model"),
            "publish-folder" => json!(if matches!(
                text(w, "publish-mode").as_str(),
                "existing" | "url"
            ) {
                "model"
            } else {
                ""
            }),
            "author-name" | "author-email" => json!(""),
            "local-branch" => json!("main"),
            "account" => json!(
                login(w)
                    .or_else(|| w
                        .repositories
                        .account
                        .as_ref()
                        .and_then(|status| status
                            .accounts
                            .iter()
                            .find(|account| account.active)
                            .or_else(|| status.accounts.first()))
                        .map(|account| account.login.as_str()))
                    .unwrap_or_default()
            ),
            "new-owner" => json!(login(w).unwrap_or_default()),
            _ => json!(""),
        })
}
fn text(w: &Workspace, key: &str) -> String {
    value(w, key).as_str().unwrap_or_default().to_string()
}
fn flag(w: &Workspace, key: &str) -> bool {
    value(w, key).as_bool().unwrap_or(false)
}
fn login(w: &Workspace) -> Option<&str> {
    w.repositories
        .account
        .as_ref()
        .and_then(|account| account.login.as_deref())
}
fn select_repository(w: &mut Workspace, repository: forge::Repository) {
    w.repositories
        .fields
        .insert("clone-url".into(), json!(repository.clone_url));
    w.repositories
        .fields
        .insert("clone-connection".into(), json!("github"));
    w.repositories.fields.remove("clone-branch");
    w.repositories.selected = Some(repository);
    w.repositories.review = None;
    w.repositories
        .fields
        .insert("public-confirmed".into(), json!(false));
}
fn idle(w: &Workspace) -> bool {
    w.repositories.pending.is_none()
}
pub fn operation_pending(w: &Workspace) -> bool {
    !idle(w)
}
fn notice(w: &mut Workspace, message: String) {
    if w.repositories.notices.last() != Some(&message) {
        w.repositories.notices.push(message);
    }
    if w.repositories.notices.len() > 12 {
        w.repositories.notices.remove(0);
    }
}
fn set_status(w: &mut Workspace, message: impl Into<String>) {
    let message = message.into();
    w.status = message.clone();
    w.repositories.status = message;
}
fn submit(
    w: &mut Workspace,
    publishing: bool,
    label: &str,
    work: impl FnOnce(CancelToken, mpsc::Sender<Event>) -> Result<Outcome> + Send + 'static,
) {
    if !idle(w) {
        set_status(w, "Wait for the repository operation, or cancel it first");
        return;
    }
    let cancel = CancelToken::default();
    let worker_cancel = cancel.clone();
    let (sender, receiver) = mpsc::channel();
    if let Ok(mut slot) = w.repositories.receiver.lock() {
        *slot = Some(receiver);
    }
    w.repositories.pending = Some(Pending {
        cancel,
        context: Context::capture(w),
        publishing,
    });
    set_status(w, label);
    std::thread::spawn(move || {
        let result = work(worker_cancel, sender.clone()).map_err(|error| error.to_string());
        if sender.send(Event::Finished(result)).is_ok() {
            crate::redraw::wake();
        }
    });
}
fn safe_to_leave(w: &Workspace) -> Result<()> {
    if crate::git_ui::busy(w)
        || crate::drawing_interaction_ui::active(w)
        || w.settings.is_some()
        || w.tool_checkpoint.is_some()
        || w.sketch.is_some()
        || w.session.draft.is_some()
    {
        return Err(invalid(
            "Finish or cancel the current edit before opening or publishing a model",
        ));
    }
    if w.session.committed.fingerprint() != w.saved_source {
        return Err(invalid(
            "Save the current model before opening or publishing another model",
        ));
    }
    Ok(())
}
fn source_ready(w: &Workspace) -> Result<PathBuf> {
    safe_to_leave(w)?;
    w.project
        .clone()
        .ok_or_else(|| invalid("Save the current model to a local folder first"))
}
fn require_login(w: &Workspace) -> Result<()> {
    if login(w).is_none() {
        Err(invalid("Sign in to GitHub first"))
    } else {
        Ok(())
    }
}
fn path_field(w: &Workspace, key: &str) -> Result<PathBuf> {
    let path = PathBuf::from(text(w, key).trim());
    if path.as_os_str().is_empty() || !path.is_absolute() {
        return Err(invalid("Choose a full local folder path"));
    }
    if path.exists() {
        return Err(invalid(
            "Choose a new folder; existing folders are never replaced",
        ));
    }
    Ok(path)
}
fn open_sign_in_browser() -> std::result::Result<(), String> {
    const URL: &str = "https://github.com/login/device";
    #[cfg(target_os = "macos")]
    let result = std::process::Command::new("open").arg(URL).spawn();
    #[cfg(target_os = "windows")]
    let result = std::process::Command::new("rundll32")
        .args(["url.dll,FileProtocolHandler", URL])
        .spawn();
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let result = std::process::Command::new("xdg-open").arg(URL).spawn();
    result
        .map(|_| ())
        .map_err(|error| format!("Open {URL} in your browser: {error}"))
}

pub fn poll(w: &mut Workspace) {
    let mut events = Vec::new();
    if let Ok(slot) = w.repositories.receiver.lock()
        && let Some(receiver) = slot.as_ref()
    {
        loop {
            match receiver.try_recv() {
                Ok(event) => {
                    let complete = matches!(event, Event::Finished(_));
                    events.push(event);
                    if complete {
                        break;
                    }
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    events.push(Event::Finished(Err("The repository worker stopped. Any remote changes may already be complete; refresh before retrying".into())));
                    break;
                }
            }
        }
    }
    for event in events {
        match event {
            Event::Progress(forge::Progress::DeviceCode { code, .. }) => {
                w.repositories.device_code = Some(code);
                set_status(w, "Enter the code in your browser to finish signing in");
                if let Err(error) = open_sign_in_browser() { notice(w, error); }
            }
            Event::Progress(forge::Progress::Status(message)) | Event::Status(message) => set_status(w, message),
            Event::Progress(forge::Progress::CredentialStorageWarning) => notice(w, "GitHub could not use the system credential store. Its helper may store credentials in its local configuration. Use a working system credential store for protected storage.".into()),
            Event::Created(repository) => {
                notice(w, format!("Created {} ({}) on GitHub. This repository remains available even if publishing is cancelled or fails.", repository.full_name, privacy(repository.private)));
                w.repositories.fields.insert("publish-mode".into(), json!("existing"));
                select_repository(w, repository);
            }
            Event::Finished(result) => {
                if let Ok(mut slot) = w.repositories.receiver.lock() { *slot = None; }
                let pending = w.repositories.pending.take();
                let current = pending.as_ref().is_some_and(|pending| pending.context.current(w));
                let cancelled = pending.as_ref().is_some_and(|pending| pending.cancel.is_cancelled());
                w.repositories.device_code = None;
                match result {
                    Err(error) => { notice(w, error.clone()); set_status(w, error); }
                    Ok(Outcome::Account(account)) => {
                        if let Some(message) = &account.notice { notice(w, message.clone()); }
                        let signed_in = account.login.clone();
                        w.repositories.account = Some(account);
                        w.repositories.owners.clear();
                        w.repositories.repositories.clear();
                        w.repositories.repositories_next = None;
                        w.repositories.owners_next = None;
                        w.repositories.selected = None;
                        w.repositories.review = None;
                        w.repositories.create_review = None;
                        for key in ["account", "new-owner", "browse-owner"] { w.repositories.fields.remove(key); }
                        set_status(w, signed_in.map(|login| format!("GitHub: signed in as {login}")).unwrap_or_else(|| "GitHub is not connected. Git URLs and local repositories remain available.".into()));
                    }
                    Ok(Outcome::Owners(page, append)) => {
                        if !append { w.repositories.owners.clear(); }
                        for owner in page.owners { if !w.repositories.owners.iter().any(|entry| entry.login == owner.login) { w.repositories.owners.push(owner); } }
                        w.repositories.owners_next = page.next_page;
                        set_status(w, "Repository owners loaded");
                    }
                    Ok(Outcome::Repositories(page, append)) => {
                        if !append { w.repositories.repositories.clear(); }
                        for repository in page.repositories { if !w.repositories.repositories.iter().any(|entry| entry.id == repository.id) { w.repositories.repositories.push(repository); } }
                        w.repositories.repositories_next = page.next_page;
                        set_status(w, format!("{} repositories shown", w.repositories.repositories.len()));
                    }
                    Ok(Outcome::Repository(repository)) => {
                        set_status(w, format!("Selected {} · {}", repository.full_name, privacy(repository.private)));
                        select_repository(w, repository);
                    }
                    Ok(Outcome::Created(repository)) => {
                        let message = format!("Created {} · {}. Select Download repository to create a local working copy, or publish your saved model into it.", repository.full_name, privacy(repository.private));
                        notice(w, message.clone());
                        set_status(w, message);
                        select_repository(w, repository);
                        w.repositories.create_review = None;
                        w.repositories.review = None;
                        w.repositories.fields.insert("publish-mode".into(),json!("existing"));
                    }
                    Ok(Outcome::Checkout(checkout, notices)) => {
                        for message in notices { notice(w, message); }
                        set_status(w, format!("{} TCAD models found in {}. Choose a model to open.", checkout.projects.len(), checkout.root.display()));
                        w.repositories.checkout = Some(checkout);
                    }
                    Ok(Outcome::Opened(document, manifest)) => {
                        if cancelled { set_status(w,"Opening cancelled; the current model was kept."); }
                        else if !current || safe_to_leave(w).is_err() {
                            set_status(w, "The current model changed while opening. Your work was kept; choose Open again after saving.");
                        } else {
                            w.replace(*document, Some(manifest.clone()));
                            set_status(w, format!("Opened {}", manifest.display()));
                        }
                    }
                    Ok(Outcome::Reviewed(review)) => {
                        if !cancelled && current && source_ready(w).is_ok() {
                            w.repositories.review = Some(*review);
                            w.repositories.fields.insert("public-confirmed".into(), json!(false));
                            set_status(w, "Review the destination, visibility, branch and model folder, then click Publish");
                        } else { set_status(w, "The model changed. Save it and review the destination again."); }
                    }
                    Ok(Outcome::Published(published)) => {
                        let message = format!("{} commit {} on {}. Working copy: {}", if published.pushed { "Published" } else { "Saved local" }, published.commit, published.branch, published.manifest.display());
                        for note in published.notices { notice(w, note); }
                        notice(w, message.clone());
                        set_status(w, message);
                        w.repositories.review = None;
                        w.repositories.fields.insert("published-path".into(), json!(published.manifest.display().to_string()));
                        notice(w,"Open the published working copy to continue editing. Use Versions to commit and push later changes.".into());
                        if !current { notice(w, "Publishing completed for the saved source selected earlier. The current model was not replaced.".into()); }
                    }
                    Ok(Outcome::Author(author, replace)) => {
                        if current && !cancelled {
                            for (key, value) in [("author-name", author.name), ("author-email", author.email)] {
                                if replace || text(w, key).is_empty() { w.repositories.fields.insert(key.into(), json!(value)); }
                            }
                            set_status(w, if text(w, "author-name").trim().is_empty() || text(w, "author-email").trim().is_empty() {
                                "Enter a commit author name and email before saving a version. Git has no complete author setting."
                            } else {
                                "Git author loaded. You can edit the name and email before saving a version."
                            });
                        } else {
                            set_status(w, "The current model changed. Load its Git author settings again when ready.");
                        }
                    }
                    Ok(Outcome::Initialized(root, branch)) => {
                        set_status(w, format!("Created local repository at {} on {branch}. Save a model in this folder, then use Versions.", root.display()));
                        w.repositories.checkout = Some(Checkout { root, branch, projects: Vec::new() });
                    }
                }
            }
        }
    }
}

fn browse(w: &mut Workspace, append: bool) -> Result<()> {
    require_login(w)?;
    let query = forge::RepositoryQuery {
        owner: Some(text(w, "browse-owner")).filter(|owner| !owner.is_empty()),
        search: text(w, "browse-search"),
        page: if append {
            w.repositories
                .repositories_next
                .ok_or_else(|| invalid("No more repositories"))?
        } else {
            1
        },
        per_page: 30,
    };
    submit(w, false, "Loading repositories…", move |cancel, _| {
        Ok(Outcome::Repositories(
            GitHub::discover()?.repositories(&query, &cancel)?,
            append,
        ))
    });
    Ok(())
}
fn reviewed_branch(review: &mut PublishReview, remote: models::RemoteRepository) -> Result<()> {
    if remote.empty {
        review.branch = Some(review.branch.clone().unwrap_or_else(|| "main".into()));
        review.remote_commit = None;
        return Ok(());
    }
    let branch = review
        .branch
        .clone()
        .or(remote.default_branch)
        .ok_or_else(|| {
            invalid("This repository has no default branch. Enter an existing branch.")
        })?;
    let reference = remote.branches.iter().find(|entry| entry.name == branch)
        .ok_or_else(|| invalid("Choose an existing remote branch. Create a branch in Versions after opening the repository."))?;
    if review.folder.is_empty() {
        return Err(invalid(
            "Choose a new model folder inside this existing repository",
        ));
    }
    review.branch = Some(branch);
    review.remote_commit = Some(reference.commit.clone());
    Ok(())
}
fn review(w: &mut Workspace) -> Result<()> {
    source_ready(w)?;
    w.repositories.create_review = None;
    let destination = match text(w, "publish-mode").as_str() {
        "existing" => {
            require_login(w)?;
            Destination::Existing(
                w.repositories
                    .selected
                    .clone()
                    .ok_or_else(|| invalid("Select a repository under GitHub connection first"))?,
            )
        }
        "new" => {
            require_login(w)?;
            let request = forge::CreateRepository {
                owner: text(w, "new-owner"),
                name: text(w, "new-name"),
                description: text(w, "new-description"),
                private: flag(w, "new-private"),
            };
            if request.owner.trim().is_empty() || request.name.trim().is_empty() {
                return Err(invalid("Enter the repository owner and name"));
            }
            Destination::New(request)
        }
        "local" => Destination::Local,
        _ => Destination::Git(RepositoryAddress::parse(text(w, "publish-url").trim())?.git_url),
    };
    let github = matches!(destination, Destination::New(_) | Destination::Existing(_));
    let selected_connection = text(w, "publish-connection");
    let connection = if matches!(destination, Destination::Local) {
        None
    } else {
        crate::git_connections_ui::resolve(
            w,
            if github {
                "github"
            } else {
                &selected_connection
            },
        )?
    };
    let review = PublishReview {
        context: Context::capture(w),
        account: github.then(|| login(w).unwrap_or_default().to_owned()),
        destination,
        connection,
        remote_commit: None,
        local: path_field(w, "publish-local")?,
        branch: Some(text(w, "publish-branch").trim().into())
            .filter(|branch: &String| !branch.is_empty()),
        folder: text(w, "publish-folder").trim().into(),
        message: text(w, "publish-message"),
        author: GitAuthor {
            name: text(w, "author-name"),
            email: text(w, "author-email"),
        },
    };
    if review.message.trim().is_empty()
        || review.author.name.trim().is_empty()
        || review.author.email.trim().is_empty()
    {
        return Err(invalid(
            "Enter a version message, author name and author email",
        ));
    }
    submit(
        w,
        false,
        "Checking publish destination…",
        move |cancel, sender| {
            let mut review = review;
            models::preflight_publish(&review.source_request()?, &cancel)?;
            let url = match &review.destination {
                Destination::Existing(repository) => {
                    let actual = GitHub::discover()?.repository(&repository.full_name, &cancel)?;
                    if !actual.can_push || actual.archived {
                        return Err(invalid(
                            "This repository is archived or your account cannot push to it",
                        ));
                    }
                    let url = actual.clone_url.clone();
                    review.destination = Destination::Existing(actual);
                    Some(url)
                }
                Destination::Git(url) => Some(url.clone()),
                Destination::New(_) | Destination::Local => None,
            };
            if let Some(url) = url {
                let remote =
                    models::inspect_remote(&url, review.connection.as_ref(), &cancel, |message| {
                        if sender.send(Event::Status(message.into())).is_ok() {
                            crate::redraw::wake();
                        }
                    })?;
                reviewed_branch(&mut review, remote)?;
            } else if review.branch.is_none() {
                review.branch = Some("main".into());
            }
            Ok(Outcome::Reviewed(Box::new(review)))
        },
    );
    Ok(())
}
fn publish(w: &mut Workspace) -> Result<()> {
    source_ready(w)?;
    let review = w
        .repositories
        .review
        .clone()
        .ok_or_else(|| invalid("Review the destination first"))?;
    if !review.context.current(w) {
        return Err(invalid(
            "The model changed; save it and review publishing again",
        ));
    }
    if review.disclosure_needed() && !flag(w, "public-confirmed") {
        return Err(invalid(
            "Confirm the destination access before publishing the model and its resources",
        ));
    }
    submit(
        w,
        true,
        "Saving the reviewed model…",
        move |cancel, sender| {
            models::preflight_publish(&review.source_request()?, &cancel)?;
            if matches!(review.destination, Destination::Local) {
                let result = models::create_local_model_repository(
                    &review.source_request()?,
                    &cancel,
                    |message| {
                        if sender.send(Event::Status(message.into())).is_ok() {
                            crate::redraw::wake();
                        }
                    },
                )?;
                return Ok(Outcome::Published(result));
            }
            let (url, github_guard) = match &review.destination {
                Destination::New(_) | Destination::Existing(_) => {
                    // A forge destination keeps its checked account, identity and visibility.
                    let github = GitHub::discover()?;
                    if github.account(&cancel)?.login != review.account {
                        return Err(invalid(
                            "The signed-in account changed. Review publishing again.",
                        ));
                    }
                    let repository = match &review.destination {
                        Destination::New(request) => {
                            let repository = github.create(request, &cancel)?;
                            if sender.send(Event::Created(repository.clone())).is_ok() {
                                crate::redraw::wake();
                            }
                            repository
                        }
                        Destination::Existing(repository) => {
                            let actual = github.repository(&repository.full_name, &cancel)?;
                            if actual.id != repository.id
                                || actual.private != repository.private
                                || actual.clone_url != repository.clone_url
                            {
                                return Err(invalid(
                                    "Repository identity or visibility changed. Review the destination again.",
                                ));
                            }
                            actual
                        }
                        _ => unreachable!(),
                    };
                    if Some(repository.private) != review.private() {
                        return Err(invalid(
                            "Repository visibility changed. Review the destination again.",
                        ));
                    }
                    let guard = models::GitHubPublishGuard {
                        expected_private: repository.private,
                        expected_repository_id: repository.id,
                        expected_login: review.account.clone().unwrap_or_default(),
                    };
                    (repository.clone_url, Some(guard))
                }
                Destination::Git(url) => (url.clone(), None),
                Destination::Local => unreachable!(),
            };
            let request = models::PublishRequest {
                source_manifest: review
                    .context
                    .project
                    .clone()
                    .ok_or_else(|| invalid("The saved source is missing"))?,
                expected_fingerprint: review.context.fingerprint,
                clone_url: url,
                destination: review.local,
                branch: review.branch,
                folder: review.folder,
                commit_message: review.message,
                author: review.author,
                expected_remote_commit: review.remote_commit,
                github_guard,
                remote_reviewed: true,
            };
            let published =
                models::publish_model(&request, review.connection.as_ref(), &cancel, |message| {
                    if sender.send(Event::Status(message.into())).is_ok() {
                        crate::redraw::wake();
                    }
                })?;
            Ok(Outcome::Published(published))
        },
    );
    Ok(())
}
fn choose_destination(w: &mut Workspace, key: &str) {
    let source = if key == "clone-local" {
        text(w, "clone-url")
    } else if key == "publish-local" && text(w, "publish-mode") == "url" {
        text(w, "publish-url")
    } else {
        text(w, "new-name")
    };
    let name = source
        .trim_end_matches('/')
        .rsplit(['/', ':'])
        .next()
        .unwrap_or_default()
        .trim_end_matches(".git")
        .to_owned();
    let name = if name.trim().is_empty() {
        "tcad-model".into()
    } else {
        name
    };
    if let Some(parent) = rfd::FileDialog::new()
        .set_title("Choose a parent folder for the new repository checkout")
        .pick_folder()
    {
        w.repositories
            .fields
            .insert(key.into(), json!(parent.join(name).display().to_string()));
        w.repositories.review = None;
    }
}
fn load_author(w: &mut Workspace, replace: bool) {
    if replace {
        w.repositories.review = None;
        w.repositories.create_review = None;
        w.repositories
            .fields
            .insert("public-confirmed".into(), json!(false));
    }
    let root = w
        .project
        .as_ref()
        .and_then(|path| path.parent())
        .map(|path| path.to_path_buf());
    submit(w, false, "Loading Git author settings…", move |_, _| {
        let identity = cad_core::git_connections::read_author(root.as_deref())?;
        Ok(Outcome::Author(
            GitAuthor {
                name: identity.name,
                email: identity.email,
            },
            replace,
        ))
    });
}
fn discover(w: &mut Workspace, root: PathBuf) {
    submit(w, false, "Finding TCAD models…", move |cancel, _| {
        let discovery = models::discover(&root, &cancel)?;
        Ok(Outcome::Checkout(
            Checkout {
                root,
                branch: "Local working copy".into(),
                projects: discovery.projects,
            },
            discovery.notices,
        ))
    });
}
fn perform(w: &mut Workspace, command: &str, args: Option<&Value>) -> Result<()> {
    if matches!(
        command,
        "repositories" | "repositories.open" | "repositories.publish"
    ) {
        if w.settings.is_some()
            || crate::drawing_interaction_ui::active(w)
            || w.tool_checkpoint.is_some()
            || w.sketch.is_some()
            || w.session.draft.is_some()
        {
            return Err(invalid(
                "Finish or cancel the current edit before opening Repositories",
            ));
        }
        w.dialog = Some("repositories".into());
        w.manager_tree = false;
        w.repositories.focus_section = Some(
            if command == "repositories.publish" {
                "publish"
            } else {
                "browse"
            }
            .into(),
        );
        if text(w, "author-name").is_empty() && text(w, "author-email").is_empty() && idle(w) {
            load_author(w, false);
        }
        return Ok(());
    }
    if command == "repositories.cancel" {
        if let Some(pending) = &w.repositories.pending {
            pending.cancel.cancel();
            set_status(
                w,
                "Cancellation requested. Completed remote changes are kept and reported.",
            );
        }
        return Ok(());
    }
    if command == "repositories.auth-browser" {
        open_sign_in_browser().map_err(invalid)?;
        return Ok(());
    }
    if !idle(w) {
        return Err(invalid(
            "Wait for the repository operation, or cancel it first",
        ));
    }
    match command {
        "repositories.account" => submit(w, false, "Checking GitHub sign-in…", |cancel, _| {
            Ok(Outcome::Account(GitHub::discover()?.account(&cancel)?))
        }),
        "repositories.login" => {
            submit(w, false, "Starting browser sign-in…", |cancel, sender| {
                let account = GitHub::discover()?.login(&cancel, &mut |progress| {
                    if sender.send(Event::Progress(progress)).is_ok() {
                        crate::redraw::wake();
                    }
                })?;
                Ok(Outcome::Account(account))
            })
        }
        "repositories.logout" | "repositories.switch-account" => {
            let account = text(w, "account");
            let account = if account.is_empty() {
                login(w).unwrap_or_default().into()
            } else {
                account
            };
            if account.is_empty() {
                return Err(invalid("Select an account first"));
            }
            let logout = command == "repositories.logout";
            submit(
                w,
                false,
                if logout {
                    "Signing out…"
                } else {
                    "Switching account…"
                },
                move |cancel, _| {
                    let github = GitHub::discover()?;
                    Ok(Outcome::Account(if logout {
                        github.logout(&account, &cancel)?
                    } else {
                        github.switch_account(&account, &cancel)?
                    }))
                },
            );
        }
        "repositories.owners" | "repositories.more-owners" => {
            require_login(w)?;
            let append = command.ends_with("more-owners");
            let page = if append {
                w.repositories
                    .owners_next
                    .ok_or_else(|| invalid("No more owners"))?
            } else {
                1
            };
            submit(
                w,
                false,
                "Loading repository owners…",
                move |cancel, _| {
                    Ok(Outcome::Owners(
                        GitHub::discover()?.owners(page, &cancel)?,
                        append,
                    ))
                },
            );
        }
        "repositories.browse" => browse(w, false)?,
        "repositories.more" => browse(w, true)?,
        "repositories.select" => {
            let name = args
                .and_then(|args| args["repository"].as_str())
                .unwrap_or_default();
            let repository = w
                .repositories
                .repositories
                .iter()
                .find(|repository| repository.full_name == name)
                .cloned()
                .ok_or_else(|| invalid("Refresh and select a current repository"))?;
            select_repository(w, repository);
            w.repositories.focus_section = Some("browse".into());
        }
        "repositories.lookup" => {
            require_login(w)?;
            let name = text(w, "repository-name").trim().to_string();
            submit(w, false, "Finding repository…", move |cancel, _| {
                Ok(Outcome::Repository(
                    GitHub::discover()?.repository(&name, &cancel)?,
                ))
            });
        }
        "repositories.clone-folder" => choose_destination(w, "clone-local"),
        "repositories.publish-folder" => choose_destination(w, "publish-local"),
        "repositories.clone" => {
            let request = models::CloneRequest {
                clone_url: RepositoryAddress::parse(text(w, "clone-url").trim())?.git_url,
                destination: path_field(w, "clone-local")?,
                branch: Some(text(w, "clone-branch").trim().to_string())
                    .filter(|branch| !branch.is_empty()),
                expected_commit: None,
            };
            let connection = crate::git_connections_ui::resolve(w, &text(w, "clone-connection"))?;
            submit(
                w,
                false,
                "Downloading a local working copy…",
                move |cancel, sender| {
                    let result = models::clone_repository(
                        &request,
                        connection.as_ref(),
                        &cancel,
                        |message| {
                            if sender.send(Event::Status(message.into())).is_ok() {
                                crate::redraw::wake();
                            }
                        },
                    )?;
                    Ok(Outcome::Checkout(
                        Checkout {
                            root: result.root,
                            branch: result.branch,
                            projects: result.projects,
                        },
                        result.notices,
                    ))
                },
            );
        }
        "repositories.author" => load_author(w, true),
        "repositories.local-folder" => choose_destination(w, "local-folder"),
        "repositories.initialize" => {
            let root = path_field(w, "local-folder")?;
            let branch = text(w, "local-branch");
            submit(
                w,
                false,
                "Creating a local repository…",
                move |cancel, sender| {
                    models::initialize_local_repository(&root, &branch, &cancel, |message| {
                        if sender.send(Event::Status(message.into())).is_ok() {
                            crate::redraw::wake();
                        }
                    })?;
                    Ok(Outcome::Initialized(root, branch))
                },
            );
        }
        "repositories.local" => {
            if let Some(root) = rfd::FileDialog::new()
                .set_title("Choose a local repository or model folder")
                .pick_folder()
            {
                discover(w, root);
            }
        }
        "repositories.rescan" => {
            let root = w
                .repositories
                .checkout
                .as_ref()
                .map(|checkout| checkout.root.clone())
                .ok_or_else(|| invalid("Choose a local folder first"))?;
            discover(w, root);
        }
        "repositories.open-model" => {
            safe_to_leave(w)?;
            let path = args
                .and_then(|args| args["manifest"].as_str())
                .unwrap_or_default();
            let entry = w
                .repositories
                .checkout
                .as_ref()
                .and_then(|checkout| {
                    checkout
                        .projects
                        .iter()
                        .find(|entry| entry.manifest.to_str() == Some(path))
                })
                .cloned()
                .ok_or_else(|| invalid("Rescan and choose a current model"))?;
            submit(
                w,
                false,
                "Opening the selected model…",
                move |cancel, _| {
                    Ok(Outcome::Opened(
                        Box::new(models::open_model_with_cancel(&entry, &cancel)?),
                        entry.manifest,
                    ))
                },
            );
        }
        "repositories.open-published" => {
            safe_to_leave(w)?;
            let manifest = PathBuf::from(text(w, "published-path"));
            let root = manifest
                .parent()
                .ok_or_else(|| invalid("No published model is available"))?
                .to_path_buf();
            submit(
                w,
                false,
                "Opening the published working copy…",
                move |cancel, _| {
                    let discovery = models::discover(&root, &cancel)?;
                    let entry = discovery
                        .projects
                        .into_iter()
                        .find(|entry| entry.manifest == manifest)
                        .ok_or_else(|| {
                            invalid("The published model is no longer in that folder")
                        })?;
                    Ok(Outcome::Opened(
                        Box::new(models::open_model_with_cancel(&entry, &cancel)?),
                        entry.manifest,
                    ))
                },
            );
        }
        "repositories.save" => {
            if w.tool_checkpoint.is_some() || w.session.draft.is_some() || w.sketch.is_some() {
                return Err(invalid(
                    "Finish or cancel the model edit before saving for publication",
                ));
            }
            crate::ui::save(w, false);
        }
        "repositories.use-selected" => {
            w.repositories
                .fields
                .insert("publish-mode".into(), json!("existing"));
            w.repositories.review = None;
            w.repositories.focus_section = Some("publish".into());
        }
        "repositories.new" => {
            w.repositories.focus_section = Some("publish".into());
            for key in [
                "new-name",
                "new-description",
                "publish-folder",
                "publish-local",
            ] {
                w.repositories.fields.remove(key);
            }
            w.repositories
                .fields
                .insert("publish-mode".into(), json!("new"));
            w.repositories
                .fields
                .insert("new-private".into(), json!(true));
            w.repositories
                .fields
                .insert("public-confirmed".into(), json!(false));
            w.repositories.review = None;
            w.repositories.create_review = None;
        }
        "repositories.review" => review(w)?,
        "repositories.publish-confirmed" => publish(w)?,
        "repositories.review-empty" => {
            require_login(w)?;
            let request = forge::CreateRepository {
                owner: text(w, "new-owner"),
                name: text(w, "new-name"),
                description: text(w, "new-description"),
                private: flag(w, "new-private"),
            };
            if request.owner.trim().is_empty() || request.name.trim().is_empty() {
                return Err(invalid("Enter a repository owner and name"));
            }
            w.repositories.create_review = Some((request, login(w).unwrap_or_default().into()));
            w.repositories.review = None;
            w.repositories
                .fields
                .insert("public-confirmed".into(), json!(false));
        }
        "repositories.create-empty" => {
            require_login(w)?;
            let (request, account) = w
                .repositories
                .create_review
                .clone()
                .ok_or_else(|| invalid("Review the empty repository first"))?;
            if !request.private && !flag(w, "public-confirmed") {
                return Err(invalid("Confirm that the new repository can be public"));
            }
            submit(
                w,
                false,
                "Creating the reviewed empty repository…",
                move |cancel, _| {
                    let github = GitHub::discover()?;
                    if github.account(&cancel)?.login.as_deref() != Some(account.as_str()) {
                        return Err(invalid(
                            "The signed-in account changed. Review the repository again.",
                        ));
                    }
                    Ok(Outcome::Created(github.create(&request, &cancel)?))
                },
            );
        }
        "repositories.versions" => {
            w.dialog = Some("versions".into());
            w.manager_tree = false;
            crate::git_ui::action(w, "git.refresh", None);
        }
        "repositories.clear-notices" => w.repositories.notices.clear(),
        _ => return Err(invalid("Unknown repository action")),
    }
    Ok(())
}
pub fn action(w: &mut Workspace, command: &str, args: Option<&Value>) -> bool {
    if command != "repositories" && !command.starts_with("repositories.") {
        return false;
    }
    if let Err(error) = perform(w, command, args) {
        set_status(w, error.to_string());
    }
    true
}
fn privacy(private: bool) -> &'static str {
    if private { "Private" } else { "Public" }
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
        id: format!("repositories/{key}"),
        role: "repository-field".into(),
        label: label.into(),
        value: Some(
            current
                .as_str()
                .map(str::to_string)
                .unwrap_or_else(|| current.to_string()),
        ),
        state: if idle(w) { "editable" } else { "readonly" }.into(),
        commands: vec![],
        relationships: vec![w.session.visible().id.clone()],
        data: json!({"contract":"tcad.repositories-ui","version":1,"group":group,"setter":"repositories","key":key,"document":w.session.visible().id,"json":current,"value_type":if choices.is_some(){"choice"}else if current.is_boolean(){"boolean"}else{"text"},"choices":choices,"editable":idle(w),"classes":["modeling-field"],"accessibility_label":label}),
    }
}
fn button(
    w: &Workspace,
    group: &str,
    command: &str,
    label: &str,
    args: Value,
    enabled: bool,
) -> SemanticElement {
    let suffix = args["repository"]
        .as_str()
        .or_else(|| args["manifest"].as_str())
        .unwrap_or("main");
    let id =
        if command == "git-connections" || (command == "repositories.save" && group == "browse") {
            format!("{command}/{group}/{suffix}")
        } else {
            format!("{command}/{suffix}")
        };
    SemanticElement {
        id,
        role: "repository-action".into(),
        label: label.into(),
        value: None,
        state: if enabled { "ready" } else { "disabled" }.into(),
        commands: vec![command.into()],
        relationships: vec![w.session.visible().id.clone()],
        data: json!({"group":group,"classes":["semantic-control","modeling-action"],"actions":[{"command":command,"label":label,"enabled":enabled,"arguments":args}]}),
    }
}
fn information(group: &str, key: &str, label: &str, state: &str, data: Value) -> SemanticElement {
    SemanticElement {
        id: format!("repositories/{key}"),
        role: "repository-information".into(),
        label: label.into(),
        value: None,
        state: state.into(),
        commands: vec![],
        relationships: vec![],
        data: json!({"group":group,"contract":"tcad.repositories-ui","version":1,"detail":data}),
    }
}
pub fn semantics(w: &Workspace) -> Vec<SemanticElement> {
    let available = idle(w);
    let signed_in = login(w).is_some();
    let ready_source = source_ready(w).is_ok();
    let mut elements = Vec::new();
    for (group, label) in sections() {
        elements.push(SemanticElement {
            id: format!("repositories/group/{group}"),
            role: "repository-section".into(),
            label: label.into(),
            value: None,
            state: "ready".into(),
            commands: vec![],
            relationships: vec![],
            data: json!({"group":group,"children_role":"repository-field"}),
        });
    }
    elements.push(information(
        "browse",
        "git-first",
        "Use any Git host or a local repository. A forge account is optional.",
        "ready",
        Value::Null,
    ));
    elements.push(field(
        w,
        "browse",
        "clone-url",
        "Git URL or repository path",
        None,
    ));
    elements.push(field(
        w,
        "browse",
        "clone-connection",
        "Connection",
        Some(crate::git_connections_ui::choices(w)),
    ));
    elements.push(button(
        w,
        "browse",
        "git-connections",
        "Manage connections…",
        json!({}),
        available,
    ));
    elements.push(field(
        w,
        "browse",
        "clone-branch",
        "Branch (blank uses default)",
        None,
    ));
    elements.push(field(
        w,
        "browse",
        "clone-local",
        "New local working folder",
        None,
    ));
    elements.push(button(
        w,
        "browse",
        "repositories.clone-folder",
        "Choose parent folder…",
        json!({}),
        available,
    ));
    elements.push(button(
        w,
        "browse",
        "repositories.clone",
        "Clone repository",
        json!({}),
        available && !text(w, "clone-url").trim().is_empty(),
    ));
    elements.push(button(
        w,
        "browse",
        "repositories.local",
        "Open a local folder…",
        json!({}),
        available,
    ));
    elements.push(button(
        w,
        "browse",
        "repositories.versions",
        "Versions and branches",
        json!({}),
        available && w.project.is_some(),
    ));
    if let Some(checkout) = &w.repositories.checkout {
        elements.push(information(
            "browse",
            "checkout",
            &format!("{} · {}", checkout.root.display(), checkout.branch),
            "ready",
            json!({"root":checkout.root,"branch":checkout.branch}),
        ));
        elements.push(button(
            w,
            "browse",
            "repositories.rescan",
            "Refresh local models",
            json!({}),
            available,
        ));
        for entry in &checkout.projects {
            elements.push(information("browse", &format!("model/{}", entry.relative_path), &format!("{} · {} parts · {} assemblies\n{}", entry.name, entry.parts, entry.assemblies, entry.relative_path), "ready", json!({"document":entry.document_id,"manifest":entry.manifest,"fingerprint":entry.fingerprint})));
            elements.push(button(
                w,
                "browse",
                "repositories.open-model",
                &format!("Open {}", entry.name),
                json!({"manifest":entry.manifest}),
                available && safe_to_leave(w).is_ok(),
            ));
        }
        if checkout.projects.is_empty() {
            elements.push(information(
                "browse",
                "no-models",
                "No TCAD models found. Save a model in this folder to start working locally.",
                "empty",
                Value::Null,
            ));
        }
        if safe_to_leave(w).is_err() {
            elements.push(information(
                "browse",
                "open-blocked",
                "Finish the current edit and save your model before opening another model.",
                "warning",
                Value::Null,
            ));
            elements.push(button(
                w,
                "browse",
                "repositories.save",
                "Save current model",
                json!({}),
                available
                    && w.tool_checkpoint.is_none()
                    && w.session.draft.is_none()
                    && w.sketch.is_none(),
            ));
        }
    }

    if let Err(error) = source_ready(w) {
        elements.push(information(
            "publish",
            "source-blocked",
            &error.to_string(),
            "warning",
            Value::Null,
        ));
    }
    elements.push(button(
        w,
        "publish",
        "repositories.save",
        "Save current model",
        json!({}),
        available && w.tool_checkpoint.is_none() && w.session.draft.is_none() && w.sketch.is_none(),
    ));
    elements.push(field(
        w,
        "publish",
        "publish-mode",
        "Destination",
        Some(vec![
            json!({"value":"url","label":"Git URL"}),
            json!({"value":"local","label":"New local repository"}),
            json!({"value":"existing","label":"Selected GitHub repository"}),
            json!({"value":"new","label":"New GitHub repository"}),
        ]),
    ));
    let mode = text(w, "publish-mode");
    if mode == "url" {
        elements.push(field(
            w,
            "publish",
            "publish-url",
            "Destination Git URL",
            None,
        ));
        elements.push(field(
            w,
            "publish",
            "publish-connection",
            "Connection",
            Some(crate::git_connections_ui::choices(w)),
        ));
        elements.push(button(
            w,
            "publish",
            "git-connections",
            "Manage connections…",
            json!({}),
            available,
        ));
        elements.push(information("publish", "unknown-access", "Git does not report who can access a repository. Check access to this destination before publishing.", "notice", Value::Null));
    } else if mode == "local" {
        elements.push(information("publish", "local-only", "Create a separate local repository with this model and its resources. Nothing is uploaded.", "ready", Value::Null));
    } else if mode == "existing" {
        elements.push(information(
            "publish",
            "selected-destination",
            &w.repositories
                .selected
                .as_ref()
                .map(|r| format!("{} · {}", r.full_name, privacy(r.private)))
                .unwrap_or_else(|| "Choose a repository in the GitHub connection section.".into()),
            "ready",
            Value::Null,
        ));
    } else if mode == "new" {
        let owners = std::iter::once(login(w).unwrap_or_default().to_string())
            .chain(
                w.repositories
                    .owners
                    .iter()
                    .map(|owner| owner.login.clone()),
            )
            .filter(|owner| !owner.is_empty())
            .collect::<std::collections::BTreeSet<_>>();
        elements.push(field(
            w,
            "publish",
            "new-owner",
            "Repository owner",
            if owners.is_empty() {
                None
            } else {
                Some(
                    owners
                        .into_iter()
                        .map(|owner| json!({"value":owner,"label":owner}))
                        .collect(),
                )
            },
        ));
        elements.push(field(w, "publish", "new-name", "Repository name", None));
        elements.push(field(w, "publish", "new-description", "Description", None));
        elements.push(field(
            w,
            "publish",
            "new-private",
            "Private repository",
            None,
        ));
        elements.push(button(
            w,
            "publish",
            "repositories.review-empty",
            "Review empty repository",
            json!({}),
            available && signed_in,
        ));
        if let Some((request, _)) = &w.repositories.create_review {
            elements.push(information(
                "publish",
                "create-review",
                &format!(
                    "Create {}/{} · {}. No model files will be uploaded.",
                    request.owner,
                    request.name,
                    privacy(request.private)
                ),
                "review",
                json!({"owner":request.owner,"name":request.name,"private":request.private}),
            ));
            if !request.private {
                elements.push(field(
                    w,
                    "publish",
                    "public-confirmed",
                    "I intend to create a public repository",
                    None,
                ));
            }
            elements.push(button(
                w,
                "publish",
                "repositories.create-empty",
                "Create reviewed empty repository",
                json!({}),
                available && (request.private || flag(w, "public-confirmed")),
            ));
        }
    }
    if matches!(mode.as_str(), "new" | "existing") && !signed_in {
        elements.push(information("publish", "forge-sign-in", "Connect GitHub below to use this destination. Git URL and local destinations do not need GitHub.", "warning", Value::Null));
    }
    elements.push(field(
        w,
        "publish",
        "publish-local",
        "New local working folder",
        None,
    ));
    elements.push(button(
        w,
        "publish",
        "repositories.publish-folder",
        "Choose parent folder…",
        json!({}),
        available,
    ));
    elements.push(field(
        w,
        "publish",
        "publish-branch",
        "Branch (blank uses default)",
        None,
    ));
    elements.push(field(
        w,
        "publish",
        "publish-folder",
        "Model folder in repository",
        None,
    ));
    elements.push(field(
        w,
        "publish",
        "publish-message",
        "Version message",
        None,
    ));
    elements.push(field(w, "publish", "author-name", "Commit author", None));
    elements.push(field(w, "publish", "author-email", "Commit email", None));
    elements.push(button(
        w,
        "publish",
        "repositories.author",
        "Use Git author settings",
        json!({}),
        available,
    ));
    let destination_ready = !matches!(mode.as_str(), "new" | "existing") || signed_in;
    elements.push(button(
        w,
        "publish",
        "repositories.review",
        "Review destination",
        json!({}),
        available && ready_source && destination_ready,
    ));
    if let Some(review) = &w.repositories.review {
        let visibility = if matches!(review.destination, Destination::Local) {
            "Local only"
        } else {
            review
                .private()
                .map(privacy)
                .unwrap_or("Access unknown — check the destination")
        };
        elements.push(information("publish", "publish-review", &format!("Model: {}\nSource: {}\nDestination: {} · {}\nConnection: {}\nBranch: {}\nRemote version: {}\nModel folder: {}\nLocal copy: {}\nAuthor: {} <{}>\nMessage: {}", w.session.committed.name, review.context.project.as_ref().map(|p| p.display().to_string()).unwrap_or_default(), review.name(), visibility, review.connection.as_ref().map(|profile| profile.name.as_str()).unwrap_or(if matches!(review.destination, Destination::Local) { "Local only" } else { "Existing Git credentials" }), review.branch.as_deref().unwrap_or("main / repository default"), review.remote_commit.as_deref().unwrap_or("New repository"), if review.folder.is_empty() { "repository root" } else { &review.folder }, review.local.display(), review.author.name, review.author.email, review.message), "review", json!({"repository":review.name(),"private":review.private(),"local_only":matches!(review.destination,Destination::Local),"branch":review.branch,"remote_commit":review.remote_commit,"folder":review.folder,"local":review.local,"source":review.context.project,"fingerprint":review.context.fingerprint,"connection":review.connection.as_ref().map(|profile| &profile.name)})));
        if review.disclosure_needed() {
            let label = if review.private() == Some(false) {
                "I intend to publish this model and its resources publicly"
            } else {
                "I checked who can access this destination and intend to upload this model and its resources"
            };
            elements.push(field(w, "publish", "public-confirmed", label, None));
        }
        elements.push(button(
            w,
            "publish",
            "repositories.publish-confirmed",
            if matches!(review.destination, Destination::Local) {
                "Create local model repository"
            } else {
                "Publish reviewed model"
            },
            json!({}),
            available
                && ready_source
                && review.context.current(w)
                && (!review.disclosure_needed() || flag(w, "public-confirmed")),
        ));
    }
    if !text(w, "published-path").is_empty() {
        elements.push(button(
            w,
            "publish",
            "repositories.open-published",
            "Open saved working copy",
            json!({}),
            available && safe_to_leave(w).is_ok(),
        ));
    }

    elements.push(information(
        "local",
        "local-create",
        "Start an empty repository on this computer. You can add a remote later in Versions.",
        "ready",
        Value::Null,
    ));
    elements.push(field(
        w,
        "local",
        "local-folder",
        "New repository folder",
        None,
    ));
    elements.push(button(
        w,
        "local",
        "repositories.local-folder",
        "Choose parent folder…",
        json!({}),
        available,
    ));
    elements.push(field(w, "local", "local-branch", "First branch", None));
    elements.push(button(
        w,
        "local",
        "repositories.initialize",
        "Create local repository",
        json!({}),
        available,
    ));

    elements.push(information("forge", "account-status", &login(w).map(|login| format!("GitHub · {login}")).unwrap_or_else(|| "Optional GitHub connection".into()), "ready", w.repositories.account.as_ref().map(|account| json!({"host":account.host,"login":account.login,"helper_available":account.helper_available,"notice":account.notice})).unwrap_or(Value::Null)));
    if let Some(account) = &w.repositories.account {
        if let Some(message) = &account.notice {
            elements.push(information(
                "forge",
                "account-notice",
                message,
                "warning",
                Value::Null,
            ));
        }
        if account.accounts.len() > 1 {
            elements.push(field(
                w,
                "forge",
                "account",
                "Account",
                Some(
                    account
                        .accounts
                        .iter()
                        .map(|account| json!({"value":account.login,"label":account.login}))
                        .collect(),
                ),
            ));
            elements.push(button(
                w,
                "forge",
                "repositories.switch-account",
                "Use selected account",
                json!({}),
                available,
            ));
        }
    }
    elements.push(button(
        w,
        "forge",
        "repositories.login",
        if signed_in {
            "Sign in another GitHub account"
        } else {
            "Connect GitHub with browser"
        },
        json!({}),
        available,
    ));
    elements.push(button(
        w,
        "forge",
        "repositories.account",
        "Check GitHub connection",
        json!({}),
        available,
    ));
    elements.push(button(
        w,
        "forge",
        "repositories.logout",
        "Disconnect GitHub account",
        json!({}),
        available
            && (signed_in
                || w.repositories
                    .account
                    .as_ref()
                    .is_some_and(|status| !status.accounts.is_empty())),
    ));
    elements.push(information("forge", "credential-store", "GitHub uses the GitHub CLI account store. Account changes also affect GitHub CLI on this computer.", "ready", Value::Null));
    if let Some(code) = &w.repositories.device_code {
        elements.push(information(
            "forge",
            "device-code",
            &format!("Your sign-in code: {code}"),
            "pending",
            json!({"user_code":code,"verification_uri":"https://github.com/login/device"}),
        ));
        elements.push(button(
            w,
            "forge",
            "repositories.auth-browser",
            "Open sign-in page",
            json!({}),
            true,
        ));
    }
    if signed_in {
        let owners = std::iter::once(json!({"value":"","label":"All accessible repositories"}))
            .chain(login(w).map(|login| json!({"value":login,"label":login})))
            .chain(
                w.repositories
                    .owners
                    .iter()
                    .filter(|owner| Some(owner.login.as_str()) != login(w))
                    .map(|owner| json!({"value":owner.login,"label":owner.login})),
            )
            .collect();
        elements.push(field(w, "forge", "browse-owner", "Owner", Some(owners)));
        elements.push(field(
            w,
            "forge",
            "browse-search",
            "Find GitHub repositories",
            None,
        ));
        elements.push(button(
            w,
            "forge",
            "repositories.browse",
            "Browse GitHub repositories",
            json!({}),
            available,
        ));
        elements.push(button(
            w,
            "forge",
            "repositories.owners",
            "Load organizations",
            json!({}),
            available,
        ));
        if w.repositories.owners_next.is_some() {
            elements.push(button(
                w,
                "forge",
                "repositories.more-owners",
                "More organizations",
                json!({}),
                available,
            ));
        }
        for repository in &w.repositories.repositories {
            let mut element = button(
                w,
                "forge",
                "repositories.select",
                &format!(
                    "{} · {}{}",
                    repository.full_name,
                    privacy(repository.private),
                    if repository.archived {
                        " · Archived"
                    } else {
                        ""
                    }
                ),
                json!({"repository":repository.full_name}),
                available,
            );
            if w.repositories
                .selected
                .as_ref()
                .is_some_and(|selected| selected.id == repository.id)
            {
                element.state = "selected".into();
            }
            element.data["repository"] = json!({"id":repository.id,"name":repository.full_name,"private":repository.private,"can_push":repository.can_push,"archived":repository.archived,"default_branch":repository.default_branch});
            elements.push(element);
        }
        if w.repositories.repositories_next.is_some() {
            elements.push(button(
                w,
                "forge",
                "repositories.more",
                "More repositories",
                json!({}),
                available,
            ));
        }
        elements.push(field(
            w,
            "forge",
            "repository-name",
            "Or enter owner/repository",
            None,
        ));
        elements.push(button(
            w,
            "forge",
            "repositories.lookup",
            "Find exact repository",
            json!({}),
            available,
        ));
        if let Some(repository) = &w.repositories.selected {
            elements.push(information("forge", "selected-repository", &format!("Selected: {} · {}. Its URL is ready in Open models.", repository.full_name, privacy(repository.private)), "selected", json!({"repository":repository.full_name,"url":repository.web_url,"private":repository.private})));
            elements.push(button(
                w,
                "forge",
                "repositories.use-selected",
                "Publish to selected repository…",
                json!({}),
                available,
            ));
        }
        elements.push(button(
            w,
            "forge",
            "repositories.new",
            "New private GitHub repository…",
            json!({}),
            available,
        ));
    }
    if !w.repositories.status.is_empty() {
        elements.push(information("progress", "status", &w.repositories.status, if available { "ready" } else { "pending" }, json!({"publishing":w.repositories.pending.as_ref().is_some_and(|pending|pending.publishing),"cancellable":!available})));
    }
    if !available {
        elements.push(button(
            w,
            "progress",
            "repositories.cancel",
            "Cancel operation",
            json!({}),
            true,
        ));
    }
    for (index, message) in w.repositories.notices.iter().enumerate() {
        elements.push(information(
            "progress",
            &format!("notice/{index}"),
            message,
            "notice",
            Value::Null,
        ));
    }
    if !w.repositories.notices.is_empty() {
        elements.push(button(
            w,
            "progress",
            "repositories.clear-notices",
            "Clear notices",
            json!({}),
            available,
        ));
    }
    elements
}
fn sections() -> [(&'static str, &'static str); 5] {
    [
        ("browse", "Open models"),
        ("publish", "Save or publish current model"),
        ("local", "Create a local repository"),
        ("forge", "Forge connections · GitHub"),
        ("progress", "Operation status"),
    ]
}
pub fn current_value(w: &Workspace, element: &SemanticElement) -> Option<Value> {
    (element.data["setter"] == "repositories")
        .then(|| value(w, element.data["key"].as_str().unwrap_or_default()))
}
pub fn set_value(w: &mut Workspace, element: &SemanticElement, incoming: Value) -> Result<()> {
    if element.data["document"] != w.session.visible().id || !idle(w) {
        return Err(invalid(
            "Use the current repository controls after the operation completes",
        ));
    }
    let key = element.data["key"]
        .as_str()
        .ok_or_else(|| invalid("Unknown repository field"))?;
    const FIELDS: &[&str] = &[
        "account",
        "browse-owner",
        "browse-search",
        "repository-name",
        "clone-local",
        "clone-branch",
        "clone-url",
        "clone-connection",
        "publish-url",
        "publish-connection",
        "local-folder",
        "local-branch",
        "publish-mode",
        "new-owner",
        "new-name",
        "new-description",
        "new-private",
        "publish-local",
        "publish-branch",
        "publish-folder",
        "publish-message",
        "author-name",
        "author-email",
        "public-confirmed",
    ];
    if !FIELDS.contains(&key) {
        return Err(invalid("Unknown repository field"));
    }
    if matches!(key, "new-private" | "public-confirmed") {
        if !incoming.is_boolean() {
            return Err(invalid("Choose checked or unchecked"));
        }
    } else if incoming
        .as_str()
        .is_none_or(|text| text.len() > 8192 || text.contains('\0'))
    {
        return Err(invalid(
            "Enter text without null characters, up to 8192 characters",
        ));
    }
    if key == "publish-mode"
        && !matches!(
            incoming.as_str(),
            Some("new" | "existing" | "url" | "local")
        )
    {
        return Err(invalid(
            "Choose a Git URL, local repository, or GitHub destination",
        ));
    }
    if key != "public-confirmed" {
        w.repositories.review = None;
        w.repositories.create_review = None;
        w.repositories
            .fields
            .insert("public-confirmed".into(), json!(false));
    }
    if key == "publish-mode" && incoming == "new" {
        w.repositories
            .fields
            .insert("new-private".into(), json!(true));
    }
    if matches!(key, "browse-owner" | "browse-search") {
        w.repositories.repositories_next = None;
    }
    w.repositories.fields.insert(key.into(), incoming);
    Ok(())
}
fn render(ui: &mut egui::Ui, w: &mut Workspace, element: SemanticElement) {
    match element.role.as_str() {
        "repository-field" => {
            crate::ui::semantic_field(ui, w, &element);
        }
        "repository-action" => {
            if controls::button(
                ui,
                &element.id,
                &element.role,
                &["semantic-control", "modeling-action"],
                &element.label,
                element.state != "disabled",
                element.state == "selected",
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
        "repository-information" => {
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
pub fn show(ui: &mut egui::Ui, w: &mut Workspace) {
    let focus = w.repositories.focus_section.take();
    let snapshot = semantics(w);
    for (group, label) in sections() {
        let elements = snapshot
            .iter()
            .filter(|element| {
                element.data["group"] == group && element.role != "repository-section"
            })
            .cloned()
            .collect::<Vec<_>>();
        if elements.is_empty() {
            continue;
        }
        controls::inspector_section(
            ui,
            controls::InspectorSection {
                id: &format!("repositories/group/{group}"),
                role: "repository-section",
                classes: &["repository-section"],
                label,
                state: "ready",
                collapse: Some(group == "browse" || (group == "progress" && !idle(w))),
                open: focus
                    .as_ref()
                    .filter(|focus| focus.as_str() == group)
                    .map(|_| true),
            },
            |ui| {
                for element in elements {
                    render(ui, w, element);
                }
            },
        );
    }
}
