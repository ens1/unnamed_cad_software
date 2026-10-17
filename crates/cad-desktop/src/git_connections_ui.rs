//! Personal Git connections. Secret input stays outside document and presentation data.
use crate::{Workspace, presentation_controls as controls};
use bevy_egui::egui;
use cad_core::{
    CadError, Result, forge::CancelToken, git_connections as connections,
    presentation::SemanticElement,
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{Mutex, mpsc},
};

pub struct State {
    preferences: connections::Preferences,
    fields: BTreeMap<String, Value>,
    secret: String,
    confirmation: String,
    key: Option<connections::SshKeyInfo>,
    host_review: Option<connections::HostKeyReview>,
    pending: Option<CancelToken>,
    receiver: Mutex<Option<mpsc::Receiver<Result<Outcome>>>>,
    status: String,
    clipboard: Option<String>,
    repository_context: Option<PathBuf>,
}
impl Default for State {
    fn default() -> Self {
        let (preferences, status) = match connections::load_preferences() {
            Ok(preferences) => (preferences, String::new()),
            Err(error) => (Default::default(), error.to_string()),
        };
        Self {
            preferences,
            fields: BTreeMap::new(),
            secret: String::new(),
            confirmation: String::new(),
            key: None,
            host_review: None,
            pending: None,
            receiver: Mutex::new(None),
            status,
            clipboard: None,
            repository_context: None,
        }
    }
}
enum Outcome {
    Preferences(connections::Preferences, String),
    Key(connections::SshKeyInfo),
    Host(connections::HostKeyReview),
    Message(String),
    Saved(connections::Preferences, String),
    Removed(connections::Preferences),
    Repository {
        context: PathBuf,
        root: PathBuf,
        connection: String,
        author: connections::GitIdentity,
    },
}
pub fn take_clipboard(w: &mut Workspace) -> Option<String> {
    w.git_connections.clipboard.take()
}
fn invalid(message: impl Into<String>) -> CadError {
    CadError::Invalid(message.into())
}
fn idle(w: &Workspace) -> bool {
    w.git_connections.pending.is_none()
}
fn value(w: &Workspace, key: &str) -> Value {
    if matches!(
        key,
        "repository-connection" | "author-name" | "author-email"
    ) && w.git_connections.repository_context.as_ref() != Some(&w.root())
    {
        return json!("");
    }
    w.git_connections
        .fields
        .get(key)
        .cloned()
        .unwrap_or_else(|| match key {
            "kind" => json!("system"),
            "host-confirmed" => json!(false),
            _ => json!(""),
        })
}
fn text(w: &Workspace, key: &str) -> String {
    value(w, key).as_str().unwrap_or_default().to_string()
}
fn status(w: &mut Workspace, message: impl Into<String>) {
    let message = message.into();
    w.status = message.clone();
    w.git_connections.status = message;
}
pub fn choices(w: &Workspace) -> Vec<Value> {
    let mut result = vec![
        json!({"value":"","label":"Use existing Git credentials"}),
        json!({"value":"github","label":"GitHub account (optional)"}),
    ];
    result.extend(
        w.git_connections
            .preferences
            .profiles
            .iter()
            .map(|profile| json!({"value":profile.id,"label":profile.name})),
    );
    result
}
pub fn resolve(w: &Workspace, id: &str) -> Result<Option<connections::ConnectionProfile>> {
    if id.is_empty() {
        return Ok(None);
    }
    if id == "github" {
        return Ok(Some(connections::builtin_github_profile()));
    }
    w.git_connections
        .preferences
        .profiles
        .iter()
        .find(|profile| profile.id == id)
        .cloned()
        .map(Some)
        .ok_or_else(|| invalid("Refresh Git connections and choose an available connection"))
}
fn address(w: &Workspace) -> Result<connections::RepositoryAddress> {
    connections::RepositoryAddress::parse(text(w, "address").trim())
}
fn profile(w: &Workspace) -> Result<connections::ConnectionProfile> {
    let address = address(w)?;
    let name = text(w, "name");
    if name.trim().is_empty() {
        return Err(invalid("Enter a connection name"));
    }
    let authentication = match text(w, "kind").as_str() {
        "system" => connections::Authentication::System,
        "https" => connections::Authentication::HttpsKeychain {
            username: text(w, "username"),
        },
        "ssh" => connections::Authentication::SshKey {
            private_key: PathBuf::from(text(w, "key-path")),
        },
        _ => return Err(invalid("Choose an authentication method")),
    };
    let selected = text(w, "selected");
    let mut scope = connections::ConnectionScope::for_address(&address);
    let prefix = text(w, "path-prefix");
    scope.path_prefix =
        (!prefix.trim_matches('/').is_empty()).then(|| prefix.trim_matches('/').to_owned());
    let profile = connections::ConnectionProfile {
        version: connections::CONTRACT_VERSION,
        id: if selected.is_empty() {
            cad_core::id()
        } else {
            selected
        },
        name: name.trim().into(),
        scope,
        authentication,
    };
    profile.validate()?;
    Ok(profile)
}
fn submit(
    w: &mut Workspace,
    message: &str,
    work: impl FnOnce(CancelToken) -> Result<Outcome> + Send + 'static,
) {
    if !idle(w) {
        status(w, "Wait for the connection operation or cancel it first");
        return;
    }
    let cancel = CancelToken::default();
    let worker = cancel.clone();
    let (sender, receiver) = mpsc::channel();
    if let Ok(mut slot) = w.git_connections.receiver.lock() {
        *slot = Some(receiver);
    }
    w.git_connections.pending = Some(cancel);
    status(w, message);
    std::thread::spawn(move || {
        let _ = sender.send(work(worker));
    });
}
pub fn poll(w: &mut Workspace) {
    let result = if let Ok(slot) = w.git_connections.receiver.lock()
        && let Some(receiver) = slot.as_ref()
    {
        match receiver.try_recv() {
            Ok(result) => Some(result),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => Some(Err(invalid(
                "The connection operation stopped. Refresh before retrying.",
            ))),
        }
    } else {
        None
    };
    let Some(result) = result else {
        return;
    };
    if let Ok(mut slot) = w.git_connections.receiver.lock() {
        *slot = None;
    }
    w.git_connections.pending = None;
    match result {
        Err(error) => status(w, error.to_string()),
        Ok(Outcome::Message(message)) => status(w, message),
        Ok(Outcome::Preferences(preferences, message)) => {
            w.git_connections.preferences = preferences;
            status(w, message);
        }
        Ok(Outcome::Saved(preferences, id)) => {
            w.git_connections.preferences = preferences;
            w.git_connections
                .fields
                .insert("selected".into(), json!(id));
            status(
                w,
                "Connection saved for this user. Choose it in Repositories or assign it below.",
            );
        }
        Ok(Outcome::Removed(preferences)) => {
            w.git_connections.preferences = preferences;
            w.git_connections.fields.remove("selected");
            status(
                w,
                "Connection removed. Key files and stored credentials were retained. Assign another connection to any repository that used it.",
            );
        }
        Ok(Outcome::Repository {
            context,
            root,
            connection,
            author,
        }) => {
            if context == w.root() {
                w.git_connections.repository_context = Some(context);
                for (key, value) in [
                    ("repository-connection", connection),
                    ("author-name", author.name),
                    ("author-email", author.email),
                ] {
                    w.git_connections.fields.insert(key.into(), json!(value));
                }
                status(w, format!("Current repository: {}", root.display()));
            }
        }
        Ok(Outcome::Key(key)) => {
            w.git_connections.fields.insert(
                "key-path".into(),
                json!(key.private_key.display().to_string()),
            );
            status(
                w,
                "Public key ready. Copy it to your Git host account's SSH key settings.",
            );
            w.git_connections.key = Some(key);
        }
        Ok(Outcome::Host(review)) => {
            w.git_connections
                .fields
                .insert("host-confirmed".into(), json!(false));
            status(
                w,
                "Check these fingerprints against the Git host's published keys or with its administrator before trusting them.",
            );
            w.git_connections.host_review = Some(review);
        }
    }
}
fn secret(w: &mut Workspace) -> Result<connections::Secret> {
    connections::Secret::new(std::mem::take(&mut w.git_connections.secret))
}
fn perform(w: &mut Workspace, command: &str, args: Option<&Value>) -> Result<()> {
    if command == "git-connections" {
        if w.settings.is_some() {
            crate::settings::open(w, Some("application.git"));
            return Ok(());
        }
        if crate::drawing_interaction_ui::active(w) {
            return Err(invalid(
                "Finish or cancel the drawing gesture before opening Git connections",
            ));
        }
        if w.session.draft.is_some() || w.sketch.is_some() || w.tool_checkpoint.is_some() {
            return Err(invalid(
                "Finish the current edit before opening Git connections",
            ));
        }
        w.open_editor("git-connections");
        return Ok(());
    }
    if command == "git-connections.cancel" {
        if let Some(cancel) = &w.git_connections.pending {
            cancel.cancel();
        }
        w.git_connections.secret.clear();
        w.git_connections.confirmation.clear();
        status(
            w,
            "Cancellation requested. Completed credential changes are retained.",
        );
        return Ok(());
    }
    if !idle(w) {
        return Err(invalid(
            "Wait for the connection operation or cancel it first",
        ));
    }
    match command {
        "git-connections.refresh" => submit(w, "Loading saved connections…", |_| {
            Ok(Outcome::Preferences(
                connections::load_preferences()?,
                "Connections refreshed".into(),
            ))
        }),
        "git-connections.new" => {
            w.git_connections.fields.clear();
            w.git_connections.secret.clear();
            w.git_connections.confirmation.clear();
            w.git_connections.key = None;
            w.git_connections.host_review = None;
            status(
                w,
                "New connection. Choose its address and authentication method.",
            );
        }
        "git-connections.select" => {
            let id = args
                .and_then(|args| args["profile"].as_str())
                .unwrap_or_default();
            let selected = w
                .git_connections
                .preferences
                .profiles
                .iter()
                .find(|profile| profile.id == id)
                .cloned()
                .ok_or_else(|| invalid("Refresh and select a saved connection"))?;
            w.git_connections.secret.clear();
            w.git_connections.confirmation.clear();
            w.git_connections.host_review = None;
            w.git_connections.key = None;
            w.git_connections.fields.clear();
            w.git_connections
                .fields
                .insert("selected".into(), json!(selected.id));
            w.git_connections
                .fields
                .insert("name".into(), json!(selected.name));
            // A safe representative address is used only to edit the scope, not to contact a repo.
            let host = selected.scope.host.unwrap_or_default();
            let port = selected
                .scope
                .port
                .map(|port| format!(":{port}"))
                .unwrap_or_default();
            let scheme = match selected.scope.transport {
                connections::Transport::Https => "https",
                connections::Transport::Ssh => "ssh",
                connections::Transport::Local => {
                    return Err(invalid(
                        "Local repositories use existing credentials without a saved connection",
                    ));
                }
            };
            let kind = match selected.authentication {
                connections::Authentication::System | connections::Authentication::GitHub => {
                    "system"
                }
                connections::Authentication::HttpsKeychain { username } => {
                    w.git_connections
                        .fields
                        .insert("username".into(), json!(username));
                    "https"
                }
                connections::Authentication::SshKey { private_key } => {
                    w.git_connections
                        .fields
                        .insert("key-path".into(), json!(private_key.display().to_string()));
                    "ssh"
                }
            };
            w.git_connections.fields.insert("kind".into(), json!(kind));
            w.git_connections.fields.insert(
                "path-prefix".into(),
                json!(selected.scope.path_prefix.as_deref().unwrap_or_default()),
            );
            w.git_connections.fields.insert(
                "address".into(),
                json!(format!("{scheme}://{host}{port}/repository.git")),
            );
        }
        "git-connections.save" => {
            let profile = profile(w)?;
            submit(w, "Saving connection…", move |_| {
                let id = profile.id.clone();
                let mut preferences = connections::load_preferences()?;
                preferences.profiles.retain(|old| old.id != profile.id);
                preferences.profiles.push(profile);
                connections::save_profiles(&preferences.profiles)?;
                Ok(Outcome::Saved(connections::load_preferences()?, id))
            });
        }
        "git-connections.remove" => {
            let id = text(w, "selected");
            if id.is_empty() {
                return Err(invalid("Select a saved connection first"));
            }
            submit(w, "Removing connection…", move |_| {
                let mut preferences = connections::load_preferences()?;
                preferences.profiles.retain(|profile| profile.id != id);
                connections::save_profiles(&preferences.profiles)?;
                Ok(Outcome::Removed(connections::load_preferences()?))
            });
        }
        "git-connections.read-repository" => {
            let context = w.root();
            submit(w, "Reading the current repository settings…", move |_| {
                let root = cad_core::version_control::repository_root(&context)?;
                let preferences = connections::load_preferences()?;
                let canonical = std::fs::canonicalize(&root)?;
                let connection = preferences
                    .bindings
                    .iter()
                    .filter(|binding| canonical.starts_with(&binding.root))
                    .max_by_key(|binding| binding.root.components().count())
                    .map(|binding| binding.profile_id.clone())
                    .unwrap_or_default();
                let author = connections::read_author(Some(&root))?;
                Ok(Outcome::Repository {
                    context,
                    root,
                    connection,
                    author,
                })
            });
        }
        "git-connections.bind-repository" => {
            let connection = text(w, "repository-connection");
            resolve(w, &connection)?;
            let context = w.root();
            submit(w, "Assigning the repository connection…", move |_| {
                let root = cad_core::version_control::repository_root(&context)?;
                connections::bind_repository(
                    &root,
                    (!connection.is_empty()).then_some(connection.as_str()),
                )?;
                Ok(Outcome::Preferences(
                    connections::load_preferences()?,
                    format!("Connection assigned to {}", root.display()),
                ))
            });
        }
        "git-connections.save-author" => {
            let name = text(w, "author-name").trim().to_owned();
            let email = text(w, "author-email").trim().to_owned();
            if name.is_empty()
                || !email.contains('@')
                || name
                    .chars()
                    .chain(email.chars())
                    .any(|c| c.is_control() || matches!(c, '<' | '>'))
            {
                return Err(invalid(
                    "Enter a commit author name and email address without angle brackets or control characters",
                ));
            }
            let context = w.root();
            submit(w, "Saving the author for this repository…", move |_| {
                use cad_core::version_control::VersionControl;
                let root = cad_core::version_control::repository_root(&context)?;
                cad_core::version_control::Git.command(
                    &root,
                    &["config", "--local", "--replace-all", "user.name", &name],
                )?;
                cad_core::version_control::Git.command(
                    &root,
                    &["config", "--local", "--replace-all", "user.email", &email],
                )?;
                Ok(Outcome::Message(format!(
                    "Commit author saved for {}",
                    root.display()
                )))
            });
        }
        "git-connections.copy-key" => {
            let key = w
                .git_connections
                .key
                .as_ref()
                .ok_or_else(|| invalid("Read or create a public key first"))?;
            w.git_connections.clipboard = Some(key.public_key.clone());
        }
        "git-connections.store-token" => {
            let address = address(w)?;
            let username = text(w, "username");
            let secret = secret(w)?;
            submit(
                w,
                "Saving credential in the system credential store…",
                move |cancel| {
                    connections::store_https_credential(&address, &username, secret, &cancel)?;
                    Ok(Outcome::Message("HTTPS credential saved. Save the connection to use it in repository operations.".into()))
                },
            );
        }
        "git-connections.remove-token" => {
            let address = address(w)?;
            let username = text(w, "username");
            submit(
                w,
                "Removing the selected stored credential…",
                move |cancel| {
                    connections::remove_https_credential(&address, &username, &cancel)?;
                    Ok(Outcome::Message("The selected HTTPS credential was removed from the system credential store.".into()))
                },
            );
        }
        "git-connections.choose-key" => {
            if let Some(path) = rfd::FileDialog::new()
                .set_title("Choose an SSH private key (kept on this computer)")
                .pick_file()
            {
                w.git_connections
                    .fields
                    .insert("key-path".into(), json!(path.display().to_string()));
                w.git_connections.key = None;
                w.git_connections.secret.clear();
                w.git_connections.confirmation.clear();
            }
        }
        "git-connections.inspect-key" => {
            let path = PathBuf::from(text(w, "key-path"));
            let passphrase = if w.git_connections.secret.is_empty() {
                None
            } else {
                Some(secret(w)?)
            };
            submit(w, "Reading the SSH public key…", move |cancel| {
                Ok(Outcome::Key(connections::inspect_ssh_key(
                    &path, passphrase, &cancel,
                )?))
            });
        }
        "git-connections.generate-key" => {
            if w.git_connections.secret.is_empty()
                || w.git_connections.secret != w.git_connections.confirmation
            {
                return Err(invalid(
                    "Enter the same non-empty passphrase twice to create an encrypted key",
                ));
            }
            let Some(path) = rfd::FileDialog::new()
                .set_title("Save a new SSH private key")
                .set_file_name("tcad_ed25519")
                .save_file()
            else {
                return Ok(());
            };
            let passphrase = secret(w)?;
            w.git_connections.confirmation.clear();
            let comment = text(w, "name");
            submit(w, "Creating an encrypted SSH key…", move |cancel| {
                Ok(Outcome::Key(connections::generate_ssh_key(
                    &path, &comment, passphrase, &cancel,
                )?))
            });
        }
        "git-connections.unlock-key" => {
            let path = PathBuf::from(text(w, "key-path"));
            let passphrase = secret(w)?;
            submit(w, "Unlocking the key for the SSH agent…", move |cancel| {
                connections::unlock_ssh_key(&path, passphrase, &cancel)?;
                Ok(Outcome::Message(
                    "SSH key loaded into the agent for Git operations.".into(),
                ))
            });
        }
        "git-connections.scan-host" => {
            let address = address(w)?;
            submit(
                w,
                "Reading server key fingerprints for review…",
                move |cancel| {
                    Ok(Outcome::Host(connections::scan_host_key(
                        &address, &cancel,
                    )?))
                },
            );
        }
        "git-connections.trust-host" => {
            if value(w, "host-confirmed") != true {
                return Err(invalid(
                    "Confirm that you checked the displayed fingerprints with the Git host before trusting them",
                ));
            }
            let review = w
                .git_connections
                .host_review
                .clone()
                .ok_or_else(|| invalid("Read the server fingerprints first"))?;
            let fingerprints = review
                .keys
                .iter()
                .map(|key| key.fingerprint.clone())
                .collect::<Vec<_>>();
            submit(w, "Saving the reviewed server keys…", move |cancel| {
                connections::trust_host_key_cancellable(&review, &fingerprints, &cancel)?;
                Ok(Outcome::Message(
                    "Reviewed server keys saved. Changed keys will still block connection.".into(),
                ))
            });
        }
        _ => return Err(invalid("Unknown Git connection action")),
    }
    Ok(())
}
pub fn action(w: &mut Workspace, command: &str, args: Option<&Value>) -> bool {
    if command != "git-connections" && !command.starts_with("git-connections.") {
        return false;
    }
    if let Err(error) = perform(w, command, args) {
        status(w, error.to_string());
    }
    true
}
fn field(w: &Workspace, key: &str, label: &str, choices: Option<Vec<Value>>) -> SemanticElement {
    let current = value(w, key);
    SemanticElement {
        id: format!("git-connections/{key}"),
        role: "git-connection-field".into(),
        label: label.into(),
        value: Some(
            current
                .as_str()
                .map(str::to_string)
                .unwrap_or_else(|| current.to_string()),
        ),
        state: if idle(w) { "editable" } else { "readonly" }.into(),
        commands: vec![],
        relationships: vec![],
        data: json!({"contract":"tcad.git-connections-ui","version":1,"setter":"git-connections","key":key,"json":current,"value_type":if choices.is_some(){"choice"}else if current.is_boolean(){"boolean"}else{"text"},"choices":choices,"editable":idle(w),"classes":["modeling-field"]}),
    }
}
fn button(w: &Workspace, command: &str, label: &str, args: Value) -> SemanticElement {
    let enabled = idle(w) || command == "git-connections.cancel";
    SemanticElement {
        id: format!("{command}/{}", args["profile"].as_str().unwrap_or("main")),
        role: "git-connection-action".into(),
        label: label.into(),
        value: None,
        state: if enabled { "ready" } else { "disabled" }.into(),
        commands: vec![command.into()],
        relationships: vec![],
        data: json!({"classes":["semantic-control","modeling-action"],"actions":[{"command":command,"label":label,"enabled":enabled,"arguments":args}]}),
    }
}
fn info(id: &str, label: &str) -> SemanticElement {
    SemanticElement {
        id: format!("git-connections/{id}"),
        role: "git-connection-information".into(),
        label: label.into(),
        value: None,
        state: "ready".into(),
        commands: vec![],
        relationships: vec![],
        data: json!({"contract":"tcad.git-connections-ui","version":1}),
    }
}
fn section(id: &str, label: &str, open: bool) -> SemanticElement {
    let mut element = info(id, label);
    element.role = "git-connection-section".into();
    element.data["default_open"] = json!(open);
    element
}
pub fn semantics(w: &Workspace) -> Vec<SemanticElement> {
    let mut result = vec![
        info(
            "privacy",
            "Connections are personal preferences. Passwords and tokens go to the system credential store. SSH keys stay on your computer. Changes take effect immediately, independently of Settings Apply or Cancel.",
        ),
        section("saved", "Saved connections", true),
        button(
            w,
            "git-connections.refresh",
            "Refresh connections",
            json!({}),
        ),
        button(w, "git-connections.new", "New connection", json!({})),
    ];
    for profile in &w.git_connections.preferences.profiles {
        result.push(button(
            w,
            "git-connections.select",
            &profile.name,
            json!({"profile":profile.id}),
        ));
    }
    result.push(section("editor", "Connection details", true));
    result.push(field(w, "name", "Connection name", None));
    result.push(field(
        w,
        "address",
        "Repository address (sets server scope)",
        None,
    ));
    result.push(field(
        w,
        "path-prefix",
        "Restrict to repository path (optional)",
        None,
    ));
    result.push(field(
        w,
        "kind",
        "Authentication",
        Some(vec![
            json!({"value":"system","label":"Existing Git credentials"}),
            json!({"value":"https","label":"HTTPS credential"}),
            json!({"value":"ssh","label":"SSH key"}),
        ]),
    ));
    match text(w, "kind").as_str() {
        "https" => {
            result.push(field(w, "username", "HTTPS username", None));
            result.push(secret_element(
                "secret",
                "Password or access token",
                idle(w),
            ));
            result.push(button(
                w,
                "git-connections.store-token",
                "Store credential securely",
                json!({}),
            ));
            result.push(button(
                w,
                "git-connections.remove-token",
                "Remove stored credential",
                json!({}),
            ));
        }
        "ssh" => {
            result.push(field(w, "key-path", "Private key file", None));
            result.push(button(
                w,
                "git-connections.choose-key",
                "Choose existing key…",
                json!({}),
            ));
            result.push(secret_element("secret", "Key passphrase", idle(w)));
            result.push(button(
                w,
                "git-connections.inspect-key",
                "Read public key",
                json!({}),
            ));
            result.push(button(
                w,
                "git-connections.unlock-key",
                "Unlock key for this session",
                json!({}),
            ));
            result.push(secret_element(
                "confirmation",
                "Repeat passphrase for a new key",
                idle(w),
            ));
            result.push(button(
                w,
                "git-connections.generate-key",
                "Create encrypted key…",
                json!({}),
            ));
            if let Some(key) = &w.git_connections.key {
                result.push(info("key-fingerprint", &key.fingerprint));
                result.push(info("public-key", &key.public_key));
                result.push(button(
                    w,
                    "git-connections.copy-key",
                    "Copy public key",
                    json!({}),
                ));
            }
        }
        _ => {}
    }
    if address(w).is_ok_and(|address| address.transport == connections::Transport::Ssh) {
        result.push(button(
            w,
            "git-connections.scan-host",
            "Review server fingerprints",
            json!({}),
        ));
        if let Some(review) = &w.git_connections.host_review {
            result.push(info(
                "host-review",
                &format!(
                    "{}:{} — {}",
                    review.host,
                    review.port,
                    if review.known {
                        "already known"
                    } else {
                        "not yet trusted"
                    }
                ),
            ));
            for (index, key) in review.keys.iter().enumerate() {
                result.push(info(
                    &format!("host-key-{index}"),
                    &format!("{} · {}", key.algorithm, key.fingerprint),
                ));
            }
            result.push(field(
                w,
                "host-confirmed",
                "I checked these fingerprints with the server owner",
                None,
            ));
            result.push(button(
                w,
                "git-connections.trust-host",
                "Trust the reviewed server keys",
                json!({}),
            ));
        }
    }
    result.push(button(
        w,
        "git-connections.save",
        "Save connection",
        json!({}),
    ));
    if !text(w, "selected").is_empty() {
        result.push(button(
            w,
            "git-connections.remove",
            "Remove connection",
            json!({}),
        ));
    }
    result.push(section("repository", "Current repository", false));
    result.push(info("repository-context", &w.root().display().to_string()));
    result.push(button(
        w,
        "git-connections.read-repository",
        "Read current repository settings",
        json!({}),
    ));
    result.push(field(
        w,
        "repository-connection",
        "Connection for fetch and push",
        Some(choices(w)),
    ));
    result.push(button(
        w,
        "git-connections.bind-repository",
        "Use connection for this repository",
        json!({}),
    ));
    result.push(field(w, "author-name", "Commit author name", None));
    result.push(field(w, "author-email", "Commit author email", None));
    result.push(button(
        w,
        "git-connections.save-author",
        "Save author for this repository",
        json!({}),
    ));
    result.push(section("operation", "Connection status", true));
    if !w.git_connections.status.is_empty() {
        result.push(info("status", &w.git_connections.status));
    }
    if !idle(w) {
        result.push(button(
            w,
            "git-connections.cancel",
            "Cancel operation",
            json!({}),
        ));
    }
    let mut group = None;
    for element in &mut result {
        if element.role == "git-connection-section" {
            group = Some(element.id.clone());
        } else if let Some(group) = &group {
            element.data["section"] = json!(group);
            element.relationships.push(group.clone());
        }
    }
    result
}
fn secret_element(key: &str, label: &str, enabled: bool) -> SemanticElement {
    SemanticElement {
        id: format!("git-connections/{key}"),
        role: "git-connection-secret".into(),
        label: label.into(),
        value: None,
        state: if enabled { "editable" } else { "readonly" }.into(),
        commands: vec![],
        relationships: vec![],
        data: json!({"contract":"tcad.git-secret-input","version":1,"key":key,"sensitive":true,"editable":enabled,"renderer":"git-connection-secret"}),
    }
}
pub fn current_value(w: &Workspace, element: &SemanticElement) -> Option<Value> {
    (element.data["setter"] == "git-connections")
        .then(|| value(w, element.data["key"].as_str().unwrap_or_default()))
}
pub fn set_value(w: &mut Workspace, element: &SemanticElement, incoming: Value) -> Result<()> {
    if !idle(w) {
        return Err(invalid(
            "Wait for the connection operation or cancel it first",
        ));
    }
    let key = element.data["key"]
        .as_str()
        .ok_or_else(|| invalid("Unknown connection field"))?;
    if matches!(
        key,
        "repository-connection" | "author-name" | "author-email"
    ) && w.git_connections.repository_context.as_ref() != Some(&w.root())
    {
        for field in ["repository-connection", "author-name", "author-email"] {
            w.git_connections.fields.remove(field);
        }
        w.git_connections.repository_context = Some(w.root());
    }
    if ![
        "name",
        "address",
        "path-prefix",
        "kind",
        "username",
        "key-path",
        "host-confirmed",
        "repository-connection",
        "author-name",
        "author-email",
    ]
    .contains(&key)
    {
        return Err(invalid("Unknown connection field"));
    }
    if key == "host-confirmed" {
        if !incoming.is_boolean() {
            return Err(invalid("Choose checked or unchecked"));
        }
    } else if incoming
        .as_str()
        .is_none_or(|text| text.len() > 8192 || text.chars().any(char::is_control))
    {
        return Err(invalid("Enter a single line up to 8192 characters"));
    }
    if matches!(key, "address" | "kind") {
        w.git_connections.host_review = None;
        w.git_connections
            .fields
            .insert("host-confirmed".into(), json!(false));
        w.git_connections.secret.clear();
        w.git_connections.confirmation.clear();
    }
    if key == "key-path" {
        w.git_connections.key = None;
        w.git_connections.secret.clear();
        w.git_connections.confirmation.clear();
    }
    w.git_connections.fields.insert(key.into(), incoming);
    Ok(())
}
pub fn secret_control(ui: &mut egui::Ui, w: &mut Workspace, element: &SemanticElement) {
    let key = element.data["key"].as_str().unwrap_or_default();
    if !matches!(key, "secret" | "confirmation") {
        return;
    }
    let props = controls::properties(
        &element.id,
        &element.role,
        &["modeling-field"],
        &element.state,
    );
    if props.get("display").is_some_and(|value| value == "none") {
        return;
    }
    controls::label(
        ui,
        &format!("{}/label", element.id),
        "field-label",
        &[],
        &element.label,
        &element.state,
    );
    let editable = idle(w);
    let value = if key == "secret" {
        &mut w.git_connections.secret
    } else {
        &mut w.git_connections.confirmation
    };
    ui.scope(|ui| {
        controls::apply_properties(ui, &props);
        ui.add_enabled(
            editable,
            egui::TextEdit::singleline(value)
                .id_salt(&element.id)
                .password(true)
                .char_limit(8192)
                .desired_width(f32::INFINITY),
        );
    });
}
fn show_element(ui: &mut egui::Ui, w: &mut Workspace, element: &SemanticElement) {
    match element.role.as_str() {
        "git-connection-field" => crate::ui::semantic_field(ui, w, element),
        "git-connection-secret" => secret_control(ui, w, element),
        "git-connection-action" => {
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
                action(
                    w,
                    &element.commands[0],
                    Some(&element.data["actions"][0]["arguments"]),
                );
            }
        }
        _ => {
            controls::label(
                ui,
                &element.id,
                &element.role,
                &[],
                &element.label,
                &element.state,
            );
        }
    }
}
pub fn show(ui: &mut egui::Ui, w: &mut Workspace) {
    let elements = semantics(w);
    let mut index = 0;
    while index < elements.len() {
        let element = &elements[index];
        if element.role == "git-connection-section" {
            let start = index + 1;
            let end = elements[start..]
                .iter()
                .position(|item| item.role == "git-connection-section")
                .map(|offset| start + offset)
                .unwrap_or(elements.len());
            controls::collapsing(
                ui,
                &element.id,
                &element.role,
                &element.label,
                element.data["default_open"].as_bool().unwrap_or(true),
                |ui| {
                    for child in &elements[start..end] {
                        show_element(ui, w, child);
                    }
                },
            );
            index = end;
        } else {
            show_element(ui, w, element);
            index += 1;
        }
    }
}
