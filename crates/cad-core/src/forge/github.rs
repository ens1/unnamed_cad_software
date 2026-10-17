use super::{
    Account, AccountStatus, CONTRACT_VERSION, CancelToken, CreateRepository, Forge, Owner,
    OwnerPage, Progress, Repository, RepositoryPage, RepositoryQuery, github_helper, process,
};
use crate::{CadError, Result};
use serde_json::Value;
use std::{
    path::PathBuf,
    process::Command,
    time::{Duration, Instant},
};

const HOST: &str = "github.com";
const VERIFY_URI: &str = "https://github.com/login/device";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(90);
const REPOSITORY_FIELDS: &str = "id name nameWithOwner owner{login} url isPrivate isArchived isEmpty viewerPermission defaultBranchRef{name}";

#[derive(Clone, Debug)]
pub struct GitHub {
    helper: PathBuf,
}

impl GitHub {
    pub fn discover() -> Result<Self> {
        Ok(Self {
            helper: github_helper()?,
        })
    }

    /// Use the adapter-owned credential helper only for GitHub HTTPS requests. No token enters TCAD.
    pub fn configure_git(&self, command: &mut Command) -> Result<()> {
        let helper = self.helper.to_str().ok_or_else(|| {
            CadError::Invalid("The GitHub helper path is not valid Unicode".into())
        })?;
        if helper.chars().any(char::is_control) {
            return Err(CadError::Invalid("Invalid GitHub helper path".into()));
        }
        let quoted = format!("'{}'", helper.replace('\'', "'\\''"));
        command.args([
            "-c",
            "credential.https://github.com.helper=",
            "-c",
            &format!("credential.https://github.com.helper=!{quoted} auth git-credential"),
        ]);
        command
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GCM_INTERACTIVE", "never")
            .env("GH_PROMPT_DISABLED", "1")
            .env("GH_HOST", HOST)
            .env("LC_ALL", "C")
            .env("NO_COLOR", "1");
        // Debug tracing can expose credentials even when the application only reads status.
        for key in [
            "GH_DEBUG",
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
        Ok(())
    }

    pub fn git_command(&self) -> Result<Command> {
        let mut command = Command::new("git");
        self.configure_git(&mut command)?;
        Ok(command)
    }

    fn command(&self) -> Command {
        let mut command = Command::new(&self.helper);
        command
            .env("GH_HOST", HOST)
            .env("GH_PROMPT_DISABLED", "1")
            .env("GH_PAGER", "cat")
            .env("PAGER", "cat")
            .env("NO_COLOR", "1")
            .env("CLICOLOR", "0")
            .env("LC_ALL", "C");
        for key in [
            "GH_DEBUG",
            "GH_REPO",
            "GH_FORCE_TTY",
            "GH_ACCESSIBLE_PROMPTER",
        ] {
            command.env_remove(key);
        }
        command
    }

    fn api(&self, endpoint: &str, args: &[&str], cancel: &CancelToken) -> Result<Value> {
        let mut command = self.command();
        command
            .args([
                "api",
                "--hostname",
                HOST,
                "--header",
                "Accept: application/vnd.github+json",
                endpoint,
            ])
            .args(args);
        let output = process::run(&mut command, cancel, REQUEST_TIMEOUT, |_| {})?;
        if !output.status.success() {
            return Err(process::failure(&output, "GitHub request"));
        }
        serde_json::from_slice(&output.stdout).map_err(|_| CadError::Invalid("GitHub returned an unreadable response. Update the GitHub helper if this continues.".into()))
    }

    // Inherent entry points keep composition code independent of trait import details.
    pub fn account(&self, cancel: &CancelToken) -> Result<AccountStatus> {
        <Self as Forge>::account(self, cancel)
    }
    pub fn login(
        &self,
        cancel: &CancelToken,
        progress: &mut dyn FnMut(Progress),
    ) -> Result<AccountStatus> {
        <Self as Forge>::login(self, cancel, progress)
    }
    pub fn logout(&self, login: &str, cancel: &CancelToken) -> Result<AccountStatus> {
        <Self as Forge>::logout(self, login, cancel)
    }
    pub fn switch_account(&self, login: &str, cancel: &CancelToken) -> Result<AccountStatus> {
        <Self as Forge>::switch_account(self, login, cancel)
    }
    pub fn owners(&self, page: u32, cancel: &CancelToken) -> Result<OwnerPage> {
        <Self as Forge>::owners(self, page, cancel)
    }
    pub fn repositories(
        &self,
        query: &RepositoryQuery,
        cancel: &CancelToken,
    ) -> Result<RepositoryPage> {
        <Self as Forge>::repositories(self, query, cancel)
    }
    pub fn repository(&self, full_name: &str, cancel: &CancelToken) -> Result<Repository> {
        <Self as Forge>::repository(self, full_name, cancel)
    }
    pub fn create(&self, request: &CreateRepository, cancel: &CancelToken) -> Result<Repository> {
        <Self as Forge>::create(self, request, cancel)
    }

    fn repository_batch(&self, names: &[String], cancel: &CancelToken) -> Result<Vec<Repository>> {
        if names.is_empty() {
            return Ok(Vec::new());
        }
        let mut fields = Vec::new();
        for (index, name) in names.iter().enumerate() {
            let (owner, repository) = full_name(name)?;
            fields.push(format!(
                "r{index}:repository(owner:{},name:{}){{{REPOSITORY_FIELDS}}}",
                serde_json::to_string(owner)?,
                serde_json::to_string(repository)?
            ));
        }
        let query = format!("query={{{}}}", fields.join(" "));
        let value = self.api("graphql", &["--raw-field", &query], cancel)?;
        if value.get("errors").is_some() {
            return Err(CadError::Invalid("GitHub could not read one or more repositories. Refresh the list and check access.".into()));
        }
        names
            .iter()
            .enumerate()
            .map(|(index, _)| parse_repository(&value["data"][format!("r{index}")]))
            .collect()
    }
}

impl Forge for GitHub {
    fn account(&self, cancel: &CancelToken) -> Result<AccountStatus> {
        let mut command = self.command();
        // The allowlist deliberately strips error text, scopes, token values and credential paths.
        command.args([
            "auth",
            "status",
            "--hostname",
            HOST,
            "--json",
            "hosts",
            "--jq",
            "[.hosts[\"github.com\"][]? | {login,active,state,tokenSource}]",
        ]);
        let output = process::run(&mut command, cancel, REQUEST_TIMEOUT, |_| {})?;
        if !output.status.success() {
            return Err(process::failure(&output, "Read GitHub sign-in"));
        }
        let entries: Vec<Value> = serde_json::from_slice(&output.stdout).map_err(|_| {
            CadError::Invalid("Could not read GitHub sign-in. Update the GitHub helper.".into())
        })?;
        let mut status = AccountStatus {
            version: CONTRACT_VERSION,
            host: HOST.into(),
            login: None,
            user_id: None,
            display_name: None,
            accounts: Vec::new(),
            helper_available: true,
            notice: None,
        };
        for entry in &entries {
            let login = entry["login"].as_str().unwrap_or_default();
            let active = entry["active"].as_bool().unwrap_or(false);
            if valid_owner(login) {
                status.accounts.push(Account {
                    login: login.into(),
                    active,
                });
                if active && entry["state"].as_str() == Some("success") {
                    status.login = Some(login.into());
                }
            }
            if active {
                let source = entry["tokenSource"].as_str().unwrap_or_default();
                let mut notices = Vec::new();
                if source.ends_with("_TOKEN") {
                    notices.push("This account is supplied by the computer environment. Remove that override before switching accounts or signing out.");
                } else if source.ends_with("hosts.yml") || source == "oauth_token" {
                    notices.push("GitHub CLI is storing this credential in its local configuration file because secure credential storage is unavailable. TCAD does not copy it into model files.");
                }
                if entry["state"].as_str() != Some("success") {
                    notices.push("The account could not be verified. Check your connection or sign in again.");
                }
                status.notice = (!notices.is_empty()).then(|| notices.join(" "));
            }
        }
        if status.login.is_some() {
            let user = self.api("user", &["--method", "GET"], cancel)?;
            status.user_id = user["id"].as_u64();
            status.display_name = user["name"].as_str().map(|name| {
                name.chars()
                    .filter(|character| !character.is_control())
                    .take(200)
                    .collect()
            });
            if let Some(login) = user["login"].as_str().filter(|login| valid_owner(login)) {
                status.login = Some(login.into());
                for account in &mut status.accounts {
                    account.active = account.login.eq_ignore_ascii_case(login);
                }
                if !status.accounts.iter().any(|account| account.active) {
                    status.accounts.push(Account {
                        login: login.into(),
                        active: true,
                    });
                }
            }
        }
        Ok(status)
    }

    fn login(
        &self,
        cancel: &CancelToken,
        progress: &mut dyn FnMut(Progress),
    ) -> Result<AccountStatus> {
        mutable_account()?;
        progress(Progress::Status("Starting GitHub sign-in…".into()));
        let mut command = self.command();
        command.args([
            "auth",
            "login",
            "--hostname",
            HOST,
            "--web",
            "--git-protocol",
            "https",
            "--skip-ssh-key",
            "--clipboard=false",
        ]);
        let mut code_sent = false;
        let mut storage_warning = false;
        let output = process::run(&mut command, cancel, Duration::from_secs(15 * 60), |line| {
            if !code_sent
                && line.contains("one-time code")
                && let Some(code) = device_code(line)
            {
                code_sent = true;
                progress(Progress::DeviceCode {
                    code,
                    verification_uri: VERIFY_URI.into(),
                });
            }
            if line.contains("Authentication complete") {
                progress(Progress::Status(
                    "Sign-in approved. Finishing account setup…".into(),
                ));
            }
            if line.contains("credentials saved in plain text") {
                storage_warning = true;
                progress(Progress::CredentialStorageWarning);
            }
        })?;
        if !output.status.success() {
            return Err(process::failure(&output, "GitHub sign-in"));
        }
        let mut account = self.account(cancel)?;
        if storage_warning {
            account.notice = Some("GitHub CLI saved the credential in its local configuration because secure credential storage was unavailable. TCAD does not store credentials in model files.".into());
        }
        if account.login.is_none() {
            return Err(CadError::Invalid(
                "GitHub sign-in finished without an active account. Refresh the account status."
                    .into(),
            ));
        }
        Ok(account)
    }

    fn logout(&self, login: &str, cancel: &CancelToken) -> Result<AccountStatus> {
        mutable_account()?;
        owner(login)?;
        let mut command = self.command();
        command.args(["auth", "logout", "--hostname", HOST, "--user", login]);
        let output = process::run(&mut command, cancel, REQUEST_TIMEOUT, |_| {})?;
        if !output.status.success() {
            return Err(process::failure(&output, "Sign out of GitHub"));
        }
        self.account(cancel)
    }

    fn switch_account(&self, login: &str, cancel: &CancelToken) -> Result<AccountStatus> {
        mutable_account()?;
        owner(login)?;
        let mut command = self.command();
        command.args(["auth", "switch", "--hostname", HOST, "--user", login]);
        let output = process::run(&mut command, cancel, REQUEST_TIMEOUT, |_| {})?;
        if !output.status.success() {
            return Err(process::failure(&output, "Switch GitHub account"));
        }
        self.account(cancel)
    }

    fn owners(&self, page: u32, cancel: &CancelToken) -> Result<OwnerPage> {
        if !(1..=1000).contains(&page) {
            return Err(CadError::Invalid("Invalid owner page".into()));
        }
        let mut owners = Vec::new();
        if page == 1 {
            let user = self.api("user", &["--method", "GET"], cancel)?;
            let login = user["login"]
                .as_str()
                .ok_or_else(|| CadError::Invalid("GitHub did not return an account name".into()))?;
            owner(login)?;
            owners.push(Owner {
                login: login.into(),
                organization: false,
            });
        }
        let value = self.api(
            &format!("user/orgs?per_page=100&page={page}"),
            &["--method", "GET"],
            cancel,
        )?;
        let organizations = value.as_array().ok_or_else(|| {
            CadError::Invalid("GitHub did not return an organization list".into())
        })?;
        for organization in organizations {
            if let Some(login) = organization["login"]
                .as_str()
                .filter(|login| valid_owner(login))
            {
                owners.push(Owner {
                    login: login.into(),
                    organization: true,
                });
            }
        }
        Ok(OwnerPage {
            owners,
            next_page: (organizations.len() == 100).then_some(page + 1),
        })
    }

    fn repositories(
        &self,
        query: &RepositoryQuery,
        cancel: &CancelToken,
    ) -> Result<RepositoryPage> {
        if !(1..=1000).contains(&query.page) || !(1..=100).contains(&query.per_page) {
            return Err(CadError::Invalid("Invalid repository page".into()));
        }
        if let Some(name) = &query.owner {
            owner(name)?;
        }
        if query.search.len() > 200 || query.search.chars().any(char::is_control) {
            return Err(CadError::Invalid(
                "Enter a shorter repository search".into(),
            ));
        }
        let search = query.search.trim().to_ascii_lowercase();
        let offset = ((query.page - 1) * query.per_page) as usize;
        let limit = offset + query.per_page as usize;
        let filtered = query.owner.is_some() || !search.is_empty();
        let mut matches = Vec::new();
        let started = Instant::now();
        // Filtering applies across accessible repositories, not merely the currently displayed page.
        // REST affiliation includes private repositories shared by another person or organization.
        let first_page = if filtered { 1 } else { offset / 100 + 1 };
        let mut skipped = if filtered { 0 } else { (first_page - 1) * 100 };
        let mut exhausted = false;
        for page in first_page..=1000 {
            cancel.check()?;
            if started.elapsed() > Duration::from_secs(120) {
                return Err(CadError::Invalid("Repository search took too long. Choose an owner or enter the exact owner/name.".into()));
            }
            let endpoint = format!(
                "user/repos?affiliation=owner,collaborator,organization_member&sort=updated&direction=desc&per_page=100&page={page}"
            );
            let value = self.api(&endpoint, &["--method", "GET"], cancel)?;
            let repositories = value.as_array().ok_or_else(|| {
                CadError::Invalid("GitHub did not return a repository list".into())
            })?;
            for repository in repositories {
                let Some(name) = repository["full_name"].as_str() else {
                    continue;
                };
                let Ok((repo_owner, _)) = full_name(name) else {
                    continue;
                };
                if query
                    .owner
                    .as_ref()
                    .is_some_and(|owner| !repo_owner.eq_ignore_ascii_case(owner))
                    || !name.to_ascii_lowercase().contains(&search)
                {
                    continue;
                }
                if skipped < offset {
                    skipped += 1;
                    continue;
                }
                matches.push(name.into());
                if matches.len() > query.per_page as usize {
                    break;
                }
            }
            if matches.len() > query.per_page as usize || repositories.len() < 100 {
                exhausted = true;
                break;
            }
            if !filtered && (page * 100) > limit + 100 {
                break;
            }
        }
        if !exhausted {
            return Err(CadError::Invalid("Too many repositories to finish this search. Choose an owner or enter the exact owner/name.".into()));
        }
        let next_page = (matches.len() > query.per_page as usize).then_some(query.page + 1);
        matches.truncate(query.per_page as usize);
        Ok(RepositoryPage {
            repositories: self.repository_batch(&matches, cancel)?,
            next_page,
        })
    }

    fn repository(&self, name: &str, cancel: &CancelToken) -> Result<Repository> {
        full_name(name)?;
        self.repository_batch(&[name.to_string()], cancel)?
            .into_iter()
            .next()
            .ok_or_else(|| {
                CadError::Invalid(
                    "The repository was not found or this account cannot access it".into(),
                )
            })
    }

    fn create(&self, request: &CreateRepository, cancel: &CancelToken) -> Result<Repository> {
        owner(&request.owner)?;
        repository_name(&request.name)?;
        if request.description.len() > 350 || request.description.chars().any(char::is_control) {
            return Err(CadError::Invalid(
                "Repository descriptions must use at most 350 characters on one line".into(),
            ));
        }
        let user = self.api("user", &["--method", "GET"], cancel)?;
        let personal = user["login"]
            .as_str()
            .is_some_and(|login| login.eq_ignore_ascii_case(&request.owner));
        let endpoint = if personal {
            "user/repos".into()
        } else {
            format!("orgs/{}/repos", request.owner)
        };
        let name = format!("name={}", request.name);
        let description = format!("description={}", request.description);
        let private = format!("private={}", request.private);
        // Empty repositories avoid unrelated README/license commits and never publish source here.
        self.api(
            &endpoint,
            &[
                "--method",
                "POST",
                "--raw-field",
                &name,
                "--raw-field",
                &description,
                "--field",
                &private,
                "--field",
                "auto_init=false",
            ],
            cancel,
        )?;
        let result = self.repository(&format!("{}/{}", request.owner, request.name), cancel);
        let repository = result.map_err(|_| CadError::Invalid("The repository was created, but its details could not be read. Refresh the repository list before trying to create it again.".into()))?;
        if request.private && !repository.private {
            return Err(CadError::Invalid("GitHub did not confirm the new repository is private. No model was uploaded. Check the repository visibility before publishing.".into()));
        }
        Ok(repository)
    }
}

fn parse_repository(value: &Value) -> Result<Repository> {
    let text = |key: &str| {
        value[key].as_str().ok_or_else(|| {
            CadError::Invalid("GitHub returned incomplete repository details".into())
        })
    };
    let name = text("name")?.to_string();
    let full = text("nameWithOwner")?.to_string();
    let (owner, actual_name) = full_name(&full)?;
    if name != actual_name {
        return Err(CadError::Invalid(
            "GitHub repository identity is inconsistent".into(),
        ));
    }
    let boolean = |key: &str| {
        value[key].as_bool().ok_or_else(|| {
            CadError::Invalid("GitHub returned incomplete repository access details".into())
        })
    };
    let default_branch = value
        .pointer("/defaultBranchRef/name")
        .and_then(Value::as_str)
        .map(str::to_string);
    Ok(Repository {
        id: text("id")?.into(),
        name,
        owner: owner.into(),
        web_url: format!("https://github.com/{full}"),
        clone_url: format!("https://github.com/{full}.git"),
        full_name: full,
        private: boolean("isPrivate")?,
        archived: boolean("isArchived")?,
        empty: boolean("isEmpty")?,
        can_push: value["viewerPermission"]
            .as_str()
            .is_some_and(|permission| matches!(permission, "ADMIN" | "MAINTAIN" | "WRITE")),
        default_branch,
    })
}

fn valid_owner(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 100
        && !value.starts_with('-')
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}
fn mutable_account() -> Result<()> {
    if ["GH_TOKEN", "GITHUB_TOKEN"]
        .iter()
        .any(|key| std::env::var_os(key).is_some_and(|value| !value.is_empty()))
    {
        Err(CadError::Invalid("A GitHub token supplied by the computer environment controls this account. Remove that override before signing in, switching accounts or signing out.".into()))
    } else {
        Ok(())
    }
}
fn owner(value: &str) -> Result<()> {
    if valid_owner(value) {
        Ok(())
    } else {
        Err(CadError::Invalid(
            "Choose a valid GitHub account or organization name".into(),
        ))
    }
}
fn repository_name(value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 100
        || matches!(value, "." | "..")
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
    {
        Err(CadError::Invalid("Repository names can use letters, numbers, periods, underscores and hyphens (up to 100 characters)".into()))
    } else {
        Ok(())
    }
}
fn full_name(value: &str) -> Result<(&str, &str)> {
    let (owner_name, repository) = value
        .split_once('/')
        .ok_or_else(|| CadError::Invalid("Enter a repository as owner/name".into()))?;
    owner(owner_name)?;
    repository_name(repository)?;
    Ok((owner_name, repository))
}
fn device_code(line: &str) -> Option<String> {
    line.split_whitespace()
        .map(|word| {
            word.trim_matches(|character: char| {
                !character.is_ascii_alphanumeric() && character != '-'
            })
        })
        .find(|word| {
            word.len() == 9
                && word.as_bytes()[4] == b'-'
                && word.bytes().enumerate().all(|(index, byte)| {
                    index == 4 || byte.is_ascii_uppercase() || byte.is_ascii_digit()
                })
        })
        .map(str::to_string)
}
