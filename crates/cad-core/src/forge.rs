//! Application-owned forge contract. Credentials belong to the selected adapter, never a document.
use crate::{CadError, Result};
use serde::{Deserialize, Serialize};
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

mod github;
mod process;
pub use github::GitHub;

pub const CONTRACT_VERSION: u32 = 1;

#[derive(Clone, Default, Debug)]
pub struct CancelToken(Arc<AtomicBool>);
impl CancelToken {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
    pub(crate) fn check(&self) -> Result<()> {
        if self.is_cancelled() {
            Err(CadError::Invalid("Repository operation cancelled".into()))
        } else {
            Ok(())
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum Progress {
    Status(String),
    DeviceCode {
        code: String,
        verification_uri: String,
    },
    CredentialStorageWarning,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Account {
    pub login: String,
    pub active: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AccountStatus {
    pub version: u32,
    pub host: String,
    pub login: Option<String>,
    pub user_id: Option<u64>,
    pub display_name: Option<String>,
    pub accounts: Vec<Account>,
    pub helper_available: bool,
    pub notice: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Repository {
    pub id: String,
    pub name: String,
    pub full_name: String,
    pub owner: String,
    pub web_url: String,
    pub clone_url: String,
    pub private: bool,
    pub archived: bool,
    pub can_push: bool,
    pub empty: bool,
    pub default_branch: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RepositoryPage {
    pub repositories: Vec<Repository>,
    pub next_page: Option<u32>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Owner {
    pub login: String,
    pub organization: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OwnerPage {
    pub owners: Vec<Owner>,
    pub next_page: Option<u32>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct RepositoryQuery {
    pub owner: Option<String>,
    pub search: String,
    pub page: u32,
    pub per_page: u32,
}
impl Default for RepositoryQuery {
    fn default() -> Self {
        Self {
            owner: None,
            search: String::new(),
            page: 1,
            per_page: 30,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct CreateRepository {
    pub owner: String,
    pub name: String,
    pub description: String,
    pub private: bool,
}
impl Default for CreateRepository {
    fn default() -> Self {
        Self {
            owner: String::new(),
            name: String::new(),
            description: String::new(),
            private: true,
        }
    }
}

/// Replaceable, blocking forge adapter. Call from a background worker. Every result is token-free.
pub trait Forge {
    fn account(&self, cancel: &CancelToken) -> Result<AccountStatus>;
    fn login(
        &self,
        cancel: &CancelToken,
        progress: &mut dyn FnMut(Progress),
    ) -> Result<AccountStatus>;
    fn logout(&self, login: &str, cancel: &CancelToken) -> Result<AccountStatus>;
    fn switch_account(&self, login: &str, cancel: &CancelToken) -> Result<AccountStatus>;
    fn owners(&self, page: u32, cancel: &CancelToken) -> Result<OwnerPage>;
    fn repositories(&self, query: &RepositoryQuery, cancel: &CancelToken)
    -> Result<RepositoryPage>;
    fn repository(&self, full_name: &str, cancel: &CancelToken) -> Result<Repository>;
    fn create(&self, request: &CreateRepository, cancel: &CancelToken) -> Result<Repository>;
}

/// Resolve a helper without invoking it. This is also suitable for a package's launch checks.
pub fn github_helper() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("TCAD_GH") {
        let path = PathBuf::from(path);
        return executable(&path)
            .then(|| std::fs::canonicalize(&path))
            .transpose()?
            .ok_or_else(|| {
                CadError::Invalid("TCAD_GH does not point to an executable GitHub helper".into())
            });
    }
    let binary = if cfg!(windows) {
        "tcad-gh.exe"
    } else {
        "tcad-gh"
    };
    let mut paths = std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(|parent| vec![parent.join(binary)]))
        .unwrap_or_default();
    if let Some(value) = std::env::var_os("PATH") {
        paths.extend(
            std::env::split_paths(&value)
                .filter(|path| path.is_absolute())
                .map(|path| path.join(if cfg!(windows) { "gh.exe" } else { "gh" })),
        );
    }
    paths.extend([
        PathBuf::from("/opt/homebrew/bin/gh"),
        PathBuf::from("/usr/local/bin/gh"),
    ]);
    for path in paths {
        if executable(&path) {
            return Ok(std::fs::canonicalize(path)?);
        }
    }
    Err(CadError::Invalid("The GitHub helper is missing. Install the full TCAD application or GitHub CLI from https://cli.github.com, then reopen Repositories. Advanced installations can set TCAD_GH to the helper executable.".into()))
}

/// Redact credential-shaped data before displaying external diagnostics. Do not use on source blobs.
pub fn sanitize_diagnostic(text: &str) -> String {
    let mut clean = String::with_capacity(text.len());
    let mut escaping = false;
    for character in text.chars() {
        if escaping {
            if character.is_ascii_alphabetic() {
                escaping = false;
            }
        } else if character == '\u{1b}' {
            escaping = true;
        } else if !character.is_control() || matches!(character, '\n' | '\t' | '\r') {
            clean.push(character);
        }
    }
    let mut output = String::new();
    for line in clean.split_inclusive('\n') {
        let lower = line.to_ascii_lowercase();
        if lower.contains("authorization:") || lower.contains("cookie:") {
            output.push_str("[authentication details omitted]\n");
            continue;
        }
        let mut remaining = line;
        while !remaining.is_empty() {
            let prefix = ["github_pat_", "ghp_", "gho_", "ghu_", "ghs_", "ghr_"]
                .into_iter()
                .find(|prefix| remaining.starts_with(prefix));
            if prefix.is_some() {
                let end = remaining
                    .find(|character: char| !character.is_ascii_alphanumeric() && character != '_')
                    .unwrap_or(remaining.len());
                output.push_str("[credential omitted]");
                remaining = &remaining[end..];
                continue;
            }
            if let Some(scheme) = ["https://", "http://", "ssh://", "git://"]
                .into_iter()
                .find(|scheme| {
                    remaining
                        .get(..scheme.len())
                        .is_some_and(|prefix| prefix.eq_ignore_ascii_case(scheme))
                })
            {
                let authority = &remaining[scheme.len()..];
                let end = authority
                    .find(|character: char| {
                        matches!(character, '/' | '?' | '#') || character.is_whitespace()
                    })
                    .unwrap_or(authority.len());
                if let Some(at) = authority[..end].rfind('@') {
                    output.push_str(scheme);
                    output.push_str("[credential omitted]@");
                    remaining = &authority[at + 1..];
                    continue;
                }
            }
            if let Some(key) = [
                "access_token=",
                "oauth_token=",
                "token=",
                "password=",
                "client_secret=",
            ]
            .into_iter()
            .find(|key| {
                remaining
                    .get(..key.len())
                    .is_some_and(|prefix| prefix.eq_ignore_ascii_case(key))
            }) {
                output.push_str(&remaining[..key.len()]);
                output.push_str("[credential omitted]");
                let value = &remaining[key.len()..];
                let end = if value.starts_with(['"', '\'']) {
                    let quote = value.chars().next().unwrap_or('"');
                    value[1..].find(quote).map_or(value.len(), |end| end + 2)
                } else {
                    value
                        .find(|character: char| {
                            character.is_whitespace() || matches!(character, '&' | '#' | '"' | '\'')
                        })
                        .unwrap_or(value.len())
                };
                remaining = &value[end..];
                continue;
            }
            let character = remaining.chars().next().unwrap_or_default();
            output.push(character);
            remaining = &remaining[character.len_utf8()..];
        }
    }
    output
}
fn executable(path: &std::path::Path) -> bool {
    let Ok(metadata) = std::fs::metadata(path) else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}
