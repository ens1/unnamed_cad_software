//! Forge-neutral Git transport, account preferences and operating-system credentials.
//! Secrets never enter CAD definitions, serializable preferences or process arguments.
use crate::{CadError, Result};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

mod address;
mod process;
mod ssh;
use crate::forge::CancelToken;
pub use address::{RepositoryAddress, Transport};
pub use ssh::{
    HostKey, HostKeyReview, SshKeyInfo, askpass_main, generate_ssh_key, inspect_ssh_key,
    known_hosts_path, scan_host_key, trust_host_key, trust_host_key_cancellable, unlock_ssh_key,
};

pub const CONTRACT_VERSION: u32 = 1;
static PREFERENCES_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// An ephemeral input. Deliberately neither Clone nor Serialize; Debug hides its contents.
pub struct Secret(Vec<u8>);
impl Secret {
    pub fn new(value: String) -> Result<Self> {
        if value.is_empty() || value.len() > 65_536 || value.chars().any(char::is_control) {
            return Err(invalid(
                "Enter a nonempty secret without control characters",
            ));
        }
        Ok(Self(value.into_bytes()))
    }
}
impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Secret([redacted])")
    }
}
impl Drop for Secret {
    fn drop(&mut self) {
        self.0.fill(0);
        std::hint::black_box(&mut self.0);
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConnectionScope {
    pub transport: Transport,
    pub host: Option<String>,
    pub port: Option<u16>,
    /// Optional repository path restriction, relative to the host root.
    pub path_prefix: Option<String>,
}
impl ConnectionScope {
    pub fn for_address(address: &RepositoryAddress) -> Self {
        Self {
            transport: address.transport.clone(),
            host: address.host.clone(),
            port: address.effective_port(),
            path_prefix: None,
        }
    }
    pub fn matches(&self, address: &RepositoryAddress) -> bool {
        self.transport == address.transport
            && self.host.as_deref().map(str::to_ascii_lowercase)
                == address.host.as_deref().map(str::to_ascii_lowercase)
            && self.port.or(match self.transport {
                Transport::Https => Some(443),
                Transport::Ssh => Some(22),
                Transport::Local => None,
            }) == address.effective_port()
            && self.path_prefix.as_ref().is_none_or(|prefix| {
                let path = address.path.trim_matches('/');
                let prefix = prefix.trim_matches('/');
                path == prefix
                    || path
                        .strip_prefix(prefix)
                        .is_some_and(|tail| tail.starts_with('/'))
            })
    }
    fn address(&self) -> Result<RepositoryAddress> {
        let host = self
            .host
            .as_deref()
            .ok_or_else(|| invalid("A connection needs a host"))?;
        if self.path_prefix.as_ref().is_some_and(|path| {
            path.is_empty()
                || path.starts_with('/')
                || path.ends_with('/')
                || path.contains('%')
                || path
                    .split('/')
                    .any(|part| part.is_empty() || matches!(part, "." | ".."))
        }) {
            return Err(invalid(
                "A Git connection path scope must be a plain repository path without escapes or dot segments",
            ));
        }
        let path = self.path_prefix.as_deref().unwrap_or("tcad-connection");
        let scheme = match self.transport {
            Transport::Https => "https",
            Transport::Ssh => "ssh",
            Transport::Local => return Err(invalid("Local repositories do not need an account")),
        };
        let address = RepositoryAddress::parse(&format!(
            "{scheme}://{host}{}/{path}",
            self.port.map(|port| format!(":{port}")).unwrap_or_default(),
        ))?;
        if address.host.as_deref() != Some(host.to_ascii_lowercase().as_str())
            || address.user.is_some()
        {
            return Err(invalid("Invalid Git connection host scope"));
        }
        Ok(address)
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Authentication {
    #[default]
    System,
    GitHub,
    HttpsKeychain {
        username: String,
    },
    SshKey {
        private_key: PathBuf,
    },
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConnectionProfile {
    pub version: u32,
    pub id: String,
    pub name: String,
    pub scope: ConnectionScope,
    pub authentication: Authentication,
}
impl ConnectionProfile {
    pub fn validate(&self) -> Result<()> {
        if self.version != CONTRACT_VERSION
            || self.id.is_empty()
            || self.id.len() > 200
            || self.name.trim().is_empty()
            || self.name.len() > 500
            || self
                .id
                .chars()
                .chain(self.name.chars())
                .any(char::is_control)
        {
            return Err(invalid("Invalid or unsupported Git connection profile"));
        }
        let address = self.scope.address()?;
        validate_authentication(&address, &self.authentication)
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RepositoryBinding {
    pub root: PathBuf,
    pub profile_id: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Preferences {
    pub version: u32,
    pub profiles: Vec<ConnectionProfile>,
    pub bindings: Vec<RepositoryBinding>,
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            version: CONTRACT_VERSION,
            profiles: vec![],
            bindings: vec![],
        }
    }
}

pub fn preferences_path() -> Result<PathBuf> {
    Ok(preferences_root()?.join("git-connections.json"))
}
pub fn builtin_github_profile() -> ConnectionProfile {
    ConnectionProfile {
        version: CONTRACT_VERSION,
        id: "github".into(),
        name: "GitHub account".into(),
        scope: ConnectionScope {
            transport: Transport::Https,
            host: Some("github.com".into()),
            port: Some(443),
            path_prefix: None,
        },
        authentication: Authentication::GitHub,
    }
}
pub fn save_profiles(profiles: &[ConnectionProfile]) -> Result<()> {
    let _guard = PREFERENCES_LOCK
        .lock()
        .map_err(|_| invalid("Git preferences are unavailable"))?;
    let mut preferences = load_preferences()?;
    preferences.profiles = profiles.to_vec();
    save_preferences(&preferences)
}
fn preferences_root() -> Result<PathBuf> {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .ok_or_else(|| invalid("The user home folder is not available"))?;
    Ok(PathBuf::from(home).join(".tcad"))
}
pub fn load_preferences() -> Result<Preferences> {
    let path = preferences_path()?;
    if !path.exists() {
        return Ok(Preferences::default());
    }
    if fs::metadata(&path)?.len() > 2 * 1024 * 1024 {
        return Err(invalid("Git connection preferences are too large"));
    }
    let preferences: Preferences = serde_json::from_slice(&fs::read(path)?)?;
    validate_preferences(&preferences)?;
    Ok(preferences)
}
fn validate_preferences(preferences: &Preferences) -> Result<()> {
    if preferences.version != CONTRACT_VERSION
        || preferences.profiles.len() > 500
        || preferences.bindings.len() > 10_000
    {
        return Err(invalid("Unsupported Git connection preferences"));
    }
    let mut ids = std::collections::BTreeSet::new();
    for profile in &preferences.profiles {
        profile.validate()?;
        if !ids.insert(&profile.id) {
            return Err(invalid(
                "Git connection names must have distinct identities",
            ));
        }
    }
    Ok(())
}
pub fn save_preferences(preferences: &Preferences) -> Result<()> {
    validate_preferences(preferences)?;
    let path = preferences_path()?;
    fs::create_dir_all(
        path.parent()
            .ok_or_else(|| invalid("Invalid preference path"))?,
    )?;
    let temporary = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
    let bytes = serde_json::to_vec_pretty(preferences)?;
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    use std::io::Write;
    let mut file = options.open(&temporary)?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    if let Err(error) = fs::rename(&temporary, &path) {
        let _ = fs::remove_file(temporary);
        return Err(error.into());
    }
    Ok(())
}
pub fn bind_repository(root: &Path, profile_id: Option<&str>) -> Result<()> {
    let _guard = PREFERENCES_LOCK
        .lock()
        .map_err(|_| invalid("Git preferences are unavailable"))?;
    let root = fs::canonicalize(root)?;
    let mut preferences = load_preferences()?;
    preferences.bindings.retain(|binding| binding.root != root);
    if let Some(id) = profile_id {
        if id != "github" && !preferences.profiles.iter().any(|profile| profile.id == id) {
            return Err(invalid("The selected Git connection was removed"));
        }
        preferences.bindings.push(RepositoryBinding {
            root,
            profile_id: id.to_owned(),
        });
    } else {
        // Explicit System selection must stop an ancestor repository's account binding.
        preferences.bindings.push(RepositoryBinding {
            root,
            profile_id: String::new(),
        });
    }
    save_preferences(&preferences)
}
/// Bind the exact credential choice reviewed before a long-running clone or publication.
pub fn bind_reviewed_connection(root: &Path, reviewed: Option<&ConnectionProfile>) -> Result<()> {
    let _guard = PREFERENCES_LOCK
        .lock()
        .map_err(|_| invalid("Git preferences are unavailable"))?;
    let root = fs::canonicalize(root)?;
    let mut preferences = load_preferences()?;
    if let Some(reviewed) = reviewed {
        let current = if reviewed.id == "github" {
            Some(builtin_github_profile())
        } else {
            preferences
                .profiles
                .iter()
                .find(|profile| profile.id == reviewed.id)
                .cloned()
        };
        if current.as_ref() != Some(reviewed) {
            return Err(invalid(
                "The reviewed Git connection changed during this operation. Choose a connection for the saved checkout before fetching or pushing again.",
            ));
        }
    }
    preferences.bindings.retain(|binding| binding.root != root);
    preferences.bindings.push(RepositoryBinding {
        root,
        profile_id: reviewed
            .map(|profile| profile.id.clone())
            .unwrap_or_default(),
    });
    save_preferences(&preferences)
}
pub fn profile_for_repository(root: &Path) -> Result<Option<ConnectionProfile>> {
    let root = fs::canonicalize(root)?;
    let preferences = load_preferences()?;
    let binding = preferences
        .bindings
        .iter()
        .filter(|binding| root.starts_with(&binding.root))
        .max_by_key(|binding| binding.root.components().count());
    let Some(binding) = binding else {
        return Ok(None);
    };
    if binding.profile_id.is_empty() {
        return Ok(None);
    }
    if binding.profile_id == "github" {
        return Ok(Some(builtin_github_profile()));
    }
    preferences.profiles.iter().find(|profile| profile.id == binding.profile_id).cloned().map(Some).ok_or_else(|| invalid("The Git connection for this repository was removed. Choose System credentials or another connection."))
}

/// Configures only the selected host. The default preserves the user's normal credential helpers.
pub fn configure_git(
    command: &mut Command,
    address: &RepositoryAddress,
    profile: Option<&ConnectionProfile>,
) -> Result<()> {
    if let Some(profile) = profile {
        profile.validate()?;
        if !profile.scope.matches(address) {
            return Err(invalid(
                "This connection belongs to a different Git host, port or repository path",
            ));
        }
        configure_authentication(
            command,
            &profile.scope.address()?,
            &profile.authentication,
            profile.scope.path_prefix.is_some(),
        )?;
    } else if address.transport == Transport::Ssh {
        ssh::configure(command, None)?;
    }
    noninteractive(command);
    command.args([
        "-c",
        "protocol.ext.allow=never",
        "-c",
        "protocol.git.allow=never",
        "-c",
        "protocol.http.allow=never",
    ]);
    Ok(())
}
/// Used by ordinary version actions. Bindings live in user preferences, not in untrusted clones.
pub fn configure_git_for_repository(command: &mut Command, root: &Path) -> Result<()> {
    configure_git_for_operation(command, root, &["fetch".into(), "origin".into()])
}
/// Resolve the actual destination, including push URLs and user URL rewrites, before choosing a key.
pub fn configure_git_for_operation(
    command: &mut Command,
    root: &Path,
    arguments: &[String],
) -> Result<()> {
    let operation = arguments.first().map(String::as_str).unwrap_or("");
    if !matches!(operation, "fetch" | "push" | "ls-remote" | "clone") {
        noninteractive(command);
        return Ok(());
    }
    let remote = if let Some(index) = arguments.iter().position(|value| value == "--") {
        arguments.get(index + 1).map(String::as_str)
    } else {
        arguments
            .iter()
            .skip(1)
            .find(|value| !value.starts_with('-'))
            .map(String::as_str)
    }
    .unwrap_or("origin");
    if remote.is_empty() || remote.starts_with('-') || remote.chars().any(char::is_control) {
        return Err(invalid("Invalid Git remote destination"));
    }
    let mut lookup = Command::new("git");
    lookup.arg("-C").arg(root).args(["remote", "get-url"]);
    if operation == "push" {
        lookup.arg("--push");
    }
    lookup.args(["--all", "--", remote]);
    noninteractive(&mut lookup);
    let output = process::run(
        &mut lookup,
        None,
        &CancelToken::default(),
        std::time::Duration::from_secs(5),
    )?;
    let urls = if output.status.success() {
        String::from_utf8(output.stdout)
            .map_err(|_| invalid("The repository remote is not valid Unicode"))?
            .lines()
            .map(str::to_owned)
            .collect::<Vec<_>>()
    } else {
        // --get-url expands insteadOf without contacting a server.
        let mut lookup = Command::new("git");
        lookup
            .arg("-C")
            .arg(root)
            .args(["ls-remote", "--get-url", "--", remote]);
        noninteractive(&mut lookup);
        let output = process::run(
            &mut lookup,
            None,
            &CancelToken::default(),
            std::time::Duration::from_secs(5),
        )?;
        if !output.status.success() {
            return Err(invalid("Could not resolve the selected Git remote"));
        }
        vec![
            String::from_utf8(output.stdout)
                .map_err(|_| invalid("The repository remote is not valid Unicode"))?
                .trim()
                .to_owned(),
        ]
    };
    if urls.is_empty() {
        return Err(invalid("The selected Git remote has no repository address"));
    }
    let addresses = urls
        .iter()
        .map(|url| RepositoryAddress::parse(url))
        .collect::<Result<Vec<_>>>()?;
    let profile = profile_for_repository(root)?;
    if let Some(profile) = &profile {
        profile.validate()?;
        if addresses
            .iter()
            .any(|address| !profile.scope.matches(address))
        {
            return Err(invalid(
                "The selected remote uses a different host, port or repository path than this connection. Choose a matching connection or System credentials.",
            ));
        }
    }
    configure_git(command, &addresses[0], profile.as_ref())
}
fn validate_authentication(
    address: &RepositoryAddress,
    authentication: &Authentication,
) -> Result<()> {
    match authentication {
        Authentication::System => Ok(()),
        Authentication::GitHub
            if address.transport == Transport::Https
                && address.host.as_deref() == Some("github.com")
                && address.effective_port() == Some(443) =>
        {
            Ok(())
        }
        Authentication::HttpsKeychain { username } if address.transport == Transport::Https => {
            validate_username(username)
        }
        Authentication::SshKey { private_key } if address.transport == Transport::Ssh => {
            validate_key_path(private_key)
        }
        _ => Err(invalid(
            "This credential type does not match the repository address",
        )),
    }
}
fn configure_authentication(
    command: &mut Command,
    address: &RepositoryAddress,
    authentication: &Authentication,
    restrict_path: bool,
) -> Result<()> {
    validate_authentication(address, authentication)?;
    match authentication {
        Authentication::System => {
            if address.transport == Transport::Ssh {
                ssh::configure(command, None)?;
            }
        }
        Authentication::GitHub => crate::forge::GitHub::discover()?.configure_git(command)?,
        Authentication::HttpsKeychain { username } => {
            let helper = keychain_helper()?;
            // Keychain treats an omitted port and an explicit 443 differently. Normalize
            // the standard HTTPS port for this operation only, without changing Git config.
            if address.effective_port() == Some(443) {
                let host = address
                    .host
                    .as_deref()
                    .ok_or_else(|| invalid("Missing HTTPS host"))?;
                command.args([
                    "-c",
                    &format!("url.https://{host}/.insteadOf=https://{host}:443/"),
                ]);
            }
            let scope = if restrict_path {
                format!(
                    "{}/{}",
                    address.credential_scope(),
                    address.path.trim_start_matches('/')
                )
            } else {
                address.credential_scope()
            };
            command.args([
                "-c",
                &format!("credential.{scope}.helper="),
                "-c",
                &format!("credential.{scope}.helper={helper}"),
                "-c",
                &format!("credential.{scope}.username={username}"),
                "-c",
                &format!("credential.{scope}.useHttpPath=false"),
            ]);
        }
        Authentication::SshKey { private_key } => ssh::configure(command, Some(private_key))?,
    }
    Ok(())
}
fn noninteractive(command: &mut Command) {
    command
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GCM_INTERACTIVE", "never")
        .env("LC_ALL", "C");
    for key in [
        "GIT_TRACE",
        "GIT_TRACE_CURL",
        "GIT_CURL_VERBOSE",
        "GIT_TRACE_PACKET",
        "GIT_TRACE_SETUP",
        "GIT_TRACE_PERFORMANCE",
        "GIT_TRACE2",
        "GIT_TRACE2_EVENT",
        "GIT_TRACE2_PERF",
    ] {
        command.env_remove(key);
    }
}
fn validate_username(username: &str) -> Result<()> {
    if username.trim().is_empty() || username.len() > 500 || username.chars().any(char::is_control)
    {
        return Err(invalid("Enter a valid Git account name"));
    }
    Ok(())
}
fn validate_key_path(path: &Path) -> Result<()> {
    if !path.is_absolute()
        || path
            .to_str()
            .is_none_or(|value| value.chars().any(char::is_control))
    {
        return Err(invalid("Choose an absolute SSH private-key file path"));
    }
    Ok(())
}
fn keychain_helper() -> Result<&'static str> {
    if cfg!(target_os = "macos") {
        Ok("osxkeychain")
    } else {
        Err(invalid(
            "TCAD-managed HTTPS credentials currently use macOS Keychain. On this platform choose System credentials and your installed Git credential manager.",
        ))
    }
}
pub fn store_https_credential(
    address: &RepositoryAddress,
    username: &str,
    secret: Secret,
    cancel: &CancelToken,
) -> Result<()> {
    credential_operation("store", address, username, Some(secret), cancel)
}
pub fn remove_https_credential(
    address: &RepositoryAddress,
    username: &str,
    cancel: &CancelToken,
) -> Result<()> {
    credential_operation("erase", address, username, None, cancel)
}
fn credential_operation(
    operation: &str,
    address: &RepositoryAddress,
    username: &str,
    secret: Option<Secret>,
    cancel: &CancelToken,
) -> Result<()> {
    if address.transport != Transport::Https {
        return Err(invalid(
            "Keychain credentials require an HTTPS repository address",
        ));
    }
    validate_username(username)?;
    let helper = keychain_helper()?;
    let mut input = format!(
        "protocol=https\nhost={}\nusername={username}\n",
        address.authority()
    )
    .into_bytes();
    if let Some(secret) = &secret {
        input.extend_from_slice(b"password=");
        input.extend_from_slice(&secret.0);
        input.push(b'\n');
    }
    input.push(b'\n');
    let mut command = Command::new("git");
    command.arg(format!("credential-{helper}")).arg(operation);
    noninteractive(&mut command);
    let output = process::run(
        &mut command,
        Some(Secret(input)),
        cancel,
        std::time::Duration::from_secs(60),
    )?;
    if !output.status.success() {
        return Err(invalid(
            "The system credential store did not complete the request. Check Keychain access and retry.",
        ));
    }
    Ok(())
}
#[derive(Clone, Debug)]
pub struct GitIdentity {
    pub name: String,
    pub email: String,
}
pub fn system_author() -> Result<GitIdentity> {
    read_author(None)
}
pub fn read_author(root: Option<&Path>) -> Result<GitIdentity> {
    fn value(root: Option<&Path>, key: &str) -> Result<String> {
        let mut command = Command::new("git");
        if let Some(root) = root {
            command.arg("-C").arg(root);
        }
        command.args(["config", "--get", key]);
        let output = process::run(
            &mut command,
            None,
            &CancelToken::default(),
            std::time::Duration::from_secs(5),
        )?;
        Ok(if output.status.success() {
            String::from_utf8_lossy(&output.stdout).trim().to_owned()
        } else {
            String::new()
        })
    }
    Ok(GitIdentity {
        name: value(root, "user.name")?,
        email: value(root, "user.email")?,
    })
}
fn invalid(message: &str) -> CadError {
    CadError::Invalid(message.to_owned())
}
