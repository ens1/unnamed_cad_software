use crate::{CadError, Document, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    process::Command,
};
pub mod merge_recovery;

pub trait VersionControl {
    fn command(&self, root: &Path, args: &[&str]) -> Result<String>;
}
pub struct Git;
impl VersionControl for Git {
    fn command(&self, root: &Path, args: &[&str]) -> Result<String> {
        let mut command = Command::new("git");
        // Accounts are optional. Only network operations need the saved connection;
        // local inspection stays available even when an account needs repair.
        if args
            .first()
            .is_some_and(|arg| matches!(*arg, "fetch" | "push" | "ls-remote" | "clone"))
        {
            crate::git_connections::configure_git_for_operation(
                &mut command,
                root,
                &args
                    .iter()
                    .map(|arg| (*arg).to_string())
                    .collect::<Vec<_>>(),
            )?;
        }
        let output = command
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GCM_INTERACTIVE", "never")
            .arg("-C")
            .arg(root)
            .args(args)
            .output()?;
        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).into())
        } else {
            Err(CadError::Invalid(crate::forge::sanitize_diagnostic(
                &String::from_utf8_lossy(&output.stderr),
            )))
        }
    }
}

/// Forge-neutral operations. Call on a background executor; fetching never edits a CAD document.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case")]
pub enum GitOperation {
    Inspect,
    Initialize,
    RecoverMerge,
    CreateBranch {
        name: String,
    },
    SwitchBranch {
        name: String,
        manifest: String,
    },
    Fetch {
        remote: String,
    },
    Push {
        remote: String,
    },
    AddRemote {
        name: String,
        url: String,
    },
    Commit {
        message: String,
        manifest: String,
    },
    CommitMerge {
        message: String,
        manifest: String,
        expected_head: String,
        expected_branch: String,
        other_commit: String,
    },
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct GitReport {
    pub output: String,
    pub branch: String,
    pub status: String,
    pub history: String,
    pub branches: String,
    pub remotes: String,
    pub checkout_changed: bool,
    pub merge_committed: bool,
    pub recovery: Option<merge_recovery::Status>,
}

fn non_option(value: &str, label: &str) -> Result<()> {
    if value.trim().is_empty() || value.starts_with('-') || value.chars().any(char::is_control) {
        return Err(CadError::Invalid(format!("Choose a valid {label}")));
    }
    Ok(())
}
pub fn validate_remote(url: &str) -> Result<()> {
    crate::git_connections::RepositoryAddress::parse(url).map(|_| ())
}
pub fn repository_root(root: &Path) -> Result<PathBuf> {
    Ok(PathBuf::from(
        Git.command(root, &["rev-parse", "--show-toplevel"])?.trim(),
    ))
}
pub fn resolve_commit(root: &Path, revision: &str) -> Result<String> {
    if revision.trim().is_empty() || revision.chars().any(char::is_control) {
        return Err(CadError::Invalid("Choose a commit, tag or branch".into()));
    }
    Ok(Git
        .command(
            root,
            &[
                "rev-parse",
                "--verify",
                "--end-of-options",
                &format!("{revision}^{{commit}}"),
            ],
        )?
        .trim()
        .to_string())
}
fn project_prefix(root: &Path) -> Result<String> {
    Ok(Git
        .command(root, &["rev-parse", "--show-prefix"])?
        .trim()
        .to_string())
}
fn manifest_files(value: &Value, files: &mut BTreeSet<String>) -> Result<()> {
    for group in ["parts", "assemblies"] {
        let entries = value
            .get(group)
            .and_then(Value::as_object)
            .ok_or_else(|| CadError::Invalid("Invalid native project manifest".into()))?;
        for path in entries.values() {
            let path = path
                .as_str()
                .ok_or_else(|| CadError::Invalid("Invalid definition path".into()))?;
            crate::persistence::safe_join(Path::new("."), path)?;
            files.insert(path.into());
        }
    }
    if let Some(resources) = value
        .pointer("/document/resources")
        .and_then(Value::as_object)
    {
        for resource in resources.values() {
            let path = resource
                .get("path")
                .and_then(Value::as_str)
                .ok_or_else(|| CadError::Invalid("Invalid resource path".into()))?;
            crate::persistence::safe_join(Path::new("."), path)?;
            files.insert(path.into());
        }
    }
    Ok(())
}
/// Commit only this project's manifest, referenced definitions/assets and known presentation files.
/// `--only` keeps unrelated files already in the user's index out of the commit.
pub fn commit_project(root: &Path, manifest: &str, message: &str) -> Result<String> {
    if message.trim().is_empty() || message.len() > 65_536 || message.contains('\0') {
        return Err(CadError::Invalid("Enter a commit message".into()));
    }
    if merge_recovery::inspect(root)?.is_some() {
        return Err(CadError::Invalid(
            "Finish the interrupted merge checkout before committing".into(),
        ));
    }
    verify_saved_project(root, manifest)?;
    let files = project_files(root, manifest)?;
    let paths = files.iter().map(String::as_str).collect::<Vec<_>>();
    let mut add = vec!["--literal-pathspecs", "add", "--all", "--"];
    add.extend(paths.iter().copied());
    Git.command(root, &add)?;
    let mut commit = vec![
        "--literal-pathspecs",
        "commit",
        "--only",
        "-m",
        message,
        "--",
    ];
    commit.extend(paths);
    Git.command(root, &commit)
}
fn verify_saved_project(root: &Path, manifest: &str) -> Result<()> {
    use crate::persistence::ProjectStore;
    let document =
        crate::persistence::FileStore.load(&crate::persistence::safe_join(root, manifest)?)?;
    crate::persistence::copy_resources(&document, &[root.to_path_buf()], root)
}
/// Repository-local ignores keep private recovery snapshots and rebuildable caches off the forge.
pub fn exclude_project_caches(root: &Path) -> Result<()> {
    let exclude = Git.command(root, &["rev-parse", "--git-path", "info/exclude"])?;
    let path = PathBuf::from(exclude.trim());
    let path = if path.is_absolute() {
        path
    } else {
        root.join(path)
    };
    let mut text = std::fs::read_to_string(&path).unwrap_or_default();
    let old = text.clone();
    for line in [".tcad-recovery/", ".tcad-cache/"] {
        if !text.lines().any(|existing| existing == line) {
            text.push_str(&format!("\n{line}\n"));
        }
    }
    if old != text {
        crate::persistence::atomic_write(&path, text.as_bytes())?;
    }
    Ok(())
}
fn project_files(root: &Path, manifest: &str) -> Result<BTreeSet<String>> {
    crate::persistence::safe_join(root, manifest)?;
    let current: Value = serde_json::from_slice(&std::fs::read(root.join(manifest))?)?;
    let prefix = project_prefix(root)?;
    let mut files = BTreeSet::from([manifest.to_string()]);
    manifest_files(&current, &mut files)?;
    if let Ok(old) = Git.command(root, &["show", &format!("HEAD:{prefix}{manifest}")]) {
        manifest_files(&serde_json::from_str(&old)?, &mut files)?;
    }
    for path in [
        "ui/workspace.json",
        "ui/visibility.json",
        "README.md",
        "previews/model.stl",
        "previews/model.svg",
        "previews/model.png",
        "previews/model-preview.json",
    ] {
        if root.join(path).is_file() {
            // README is included only when TCAD's generated preview manifest exists.
            if path != "README.md" || root.join("previews/model-preview.json").is_file() {
                files.insert(path.into());
            }
        }
    }
    // Exclude unavailable paths that were never tracked; deleted tracked resources remain eligible.
    let tracked = Git.command(root, &["ls-files", "-z"])?;
    let tracked = tracked.split('\0').collect::<BTreeSet<_>>();
    files.retain(|path| root.join(path).exists() || tracked.contains(path.as_str()));
    Ok(files)
}
pub fn execute(root: &Path, operation: &GitOperation) -> Result<GitReport> {
    if crate::release::is_release_source(root)
        && matches!(
            operation,
            GitOperation::SwitchBranch { .. }
                | GitOperation::Commit { .. }
                | GitOperation::CommitMerge { .. }
                | GitOperation::RecoverMerge
        )
    {
        return Err(CadError::Invalid("This is a released source snapshot. Use Save As to create working source before changing its Git version".into()));
    }
    if !matches!(operation, GitOperation::Initialize) {
        exclude_project_caches(root)?;
    }
    let recovery_before = if matches!(operation, GitOperation::Initialize) {
        None
    } else {
        merge_recovery::inspect(root)?
    };
    if recovery_before.is_some()
        && matches!(
            operation,
            GitOperation::SwitchBranch { .. }
                | GitOperation::Commit { .. }
                | GitOperation::CommitMerge { .. }
                | GitOperation::Push { .. }
        )
    {
        return Err(CadError::Invalid(
            "Finish the interrupted merge checkout before switching, committing or pushing".into(),
        ));
    }
    let mut checkout_changed = false;
    let mut merge_committed = false;
    let output = match operation {
        GitOperation::Inspect => String::new(),
        GitOperation::RecoverMerge => {
            let result = merge_recovery::recover(root)?;
            merge_committed = recovery_before
                .as_ref()
                .is_some_and(|state| state.phase == "checkout_pending");
            checkout_changed = merge_committed;
            result
        }
        GitOperation::Initialize => {
            let result = Git.command(root, &["init"])?;
            exclude_project_caches(root)?;
            result
        }
        GitOperation::CreateBranch { name } => {
            non_option(name, "branch name")?;
            Git.command(root, &["check-ref-format", "--branch", name])?;
            Git.command(root, &["branch", "--", name])?
        }
        GitOperation::SwitchBranch { name, manifest } => {
            non_option(name, "branch name")?;
            Git.command(root, &["check-ref-format", "--branch", name])?;
            if !Git
                .command(root, &["status", "--porcelain", "--untracked-files=normal"])?
                .trim()
                .is_empty()
            {
                return Err(CadError::Invalid("Commit or move all repository changes before switching branches; no changes were overwritten".into()));
            }
            // Validate a complete source and its resources before changing the checkout.
            let revision = resolve_commit(root, &format!("refs/heads/{name}"))?;
            let candidate = snapshot_manifest(root, &revision, manifest)?;
            verify_snapshot_resources(root, &revision, &candidate)?;
            let output = Git.command(root, &["switch", "--no-overwrite-ignore", "--", name])?;
            checkout_changed = true;
            output
        }
        GitOperation::Fetch { remote } => {
            non_option(remote, "remote name")?;
            Git.command(root, &["fetch", "--", remote])?
        }
        GitOperation::Push { remote } => {
            non_option(remote, "remote name")?;
            let branch = Git.command(root, &["symbolic-ref", "--short", "HEAD"])?;
            Git.command(
                root,
                &["push", "--set-upstream", "--", remote, branch.trim()],
            )?
        }
        GitOperation::AddRemote { name, url } => {
            non_option(name, "remote name")?;
            let address = crate::git_connections::RepositoryAddress::parse(url)?;
            Git.command(root, &["remote", "add", "--", name, &address.git_url])?
        }
        GitOperation::Commit { message, manifest } => commit_project(root, manifest, message)?,
        GitOperation::CommitMerge {
            message,
            manifest,
            expected_head,
            expected_branch,
            other_commit,
        } => commit_semantic_merge(
            root,
            manifest,
            message,
            expected_branch,
            expected_head,
            other_commit,
        )?,
    };
    Ok(GitReport {
        output: crate::forge::sanitize_diagnostic(&output),
        branch: Git
            .command(root, &["symbolic-ref", "--short", "HEAD"])
            .unwrap_or_else(|_| "Detached revision".into())
            .trim()
            .into(),
        status: Git.command(root, &["status", "--short"])?,
        history: Git
            .command(
                root,
                &[
                    "log",
                    "-30",
                    "--date=short",
                    "--format=%h  %ad  %an  %s",
                    "--",
                    ".",
                ],
            )
            .unwrap_or_default(),
        branches: Git.command(root, &["branch", "--list"])?,
        remotes: crate::forge::sanitize_diagnostic(&Git.command(root, &["remote", "-v"])?),
        checkout_changed,
        merge_committed: merge_committed || matches!(operation, GitOperation::CommitMerge { .. }),
        recovery: merge_recovery::inspect(root)?,
    })
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SemanticChange {
    pub path: String,
    pub before: Option<Value>,
    pub after: Option<Value>,
}
fn entities(value: &Value) -> Option<BTreeMap<&str, &Value>> {
    let Value::Array(array) = value else {
        return None;
    };
    let mut map = BTreeMap::new();
    for item in array {
        let id = item.get("id")?.as_str()?;
        if map.insert(id, item).is_some() {
            return None;
        }
    }
    Some(map)
}
pub fn diff(before: &Document, after: &Document) -> Result<Vec<SemanticChange>> {
    fn walk(path: String, a: Option<&Value>, b: Option<&Value>, out: &mut Vec<SemanticChange>) {
        if a == b {
            return;
        }
        if let (Some(a), Some(b)) = (a, b) {
            if let (Value::Object(a), Value::Object(b)) = (a, b) {
                let keys: BTreeSet<_> = a.keys().chain(b.keys()).collect();
                for k in keys {
                    walk(format!("{path}/{k}"), a.get(k), b.get(k), out);
                }
                return;
            }
            if let (Some(a), Some(b)) = (entities(a), entities(b)) {
                let keys: BTreeSet<_> = a.keys().chain(b.keys()).collect();
                for k in keys {
                    walk(
                        format!("{path}/{k}"),
                        a.get(k).copied(),
                        b.get(k).copied(),
                        out,
                    );
                }
                return;
            }
        }
        out.push(SemanticChange {
            path,
            before: a.cloned(),
            after: b.cloned(),
        });
    }
    let mut out = vec![];
    walk(
        String::new(),
        Some(&serde_json::to_value(before)?),
        Some(&serde_json::to_value(after)?),
        &mut out,
    );
    Ok(out)
}
pub fn merge(base: &Document, ours: &Document, theirs: &Document) -> Result<Document> {
    fn order(
        path: &str,
        b: &Value,
        o: &Value,
        t: &Value,
        merged: &BTreeMap<String, Value>,
    ) -> Result<Vec<String>> {
        let ids = |value: &Value| {
            value
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|item| item.get("id").and_then(Value::as_str))
                .filter(|id| merged.contains_key(*id))
                .map(str::to_string)
                .collect::<Vec<_>>()
        };
        let base = ids(b);
        let ours = ids(o);
        let theirs = ids(t);
        let common = base
            .iter()
            .filter(|id| ours.contains(id) && theirs.contains(id))
            .cloned()
            .collect::<BTreeSet<_>>();
        let shared = |values: &[String]| {
            values
                .iter()
                .filter(|id| common.contains(*id))
                .cloned()
                .collect::<Vec<_>>()
        };
        let base_order = shared(&base);
        let our_order = shared(&ours);
        let their_order = shared(&theirs);
        let ours_reordered = our_order != base_order;
        let theirs_reordered = their_order != base_order;
        if ours_reordered && theirs_reordered && our_order != their_order {
            return Err(CadError::Invalid(format!(
                "Semantic merge conflict at {path}: both branches changed feature order"
            )));
        }
        let mut edges = merged
            .keys()
            .map(|id| (id.clone(), BTreeSet::new()))
            .collect::<BTreeMap<_, _>>();
        for (sequence, preserve_order) in [
            (&ours, !theirs_reordered || ours_reordered),
            (&theirs, !ours_reordered || theirs_reordered),
        ] {
            for pair in sequence.windows(2) {
                if preserve_order || !common.contains(&pair[0]) || !common.contains(&pair[1]) {
                    edges.get_mut(&pair[0]).unwrap().insert(pair[1].clone());
                }
            }
        }
        let preferred = ours.iter().chain(&theirs).cloned().collect::<Vec<_>>();
        let mut result = Vec::new();
        while !edges.is_empty() {
            let incoming = edges.values().flatten().cloned().collect::<BTreeSet<_>>();
            let next = preferred
                .iter()
                .find(|id| edges.contains_key(*id) && !incoming.contains(*id))
                .cloned()
                .ok_or_else(|| {
                    CadError::Invalid(format!(
                        "Semantic merge conflict at {path}: feature insertion order is ambiguous"
                    ))
                })?;
            edges.remove(&next);
            result.push(next);
        }
        Ok(result)
    }
    fn combine(path: &str, b: &Value, o: &Value, t: &Value) -> Result<Value> {
        if o == t || t == b {
            return Ok(o.clone());
        }
        if o == b {
            return Ok(t.clone());
        }
        if path == "/revision" {
            return Ok(serde_json::json!(
                o.as_u64().unwrap_or(0).max(t.as_u64().unwrap_or(0))
            ));
        }
        if let (Value::Object(b), Value::Object(o), Value::Object(t)) = (b, o, t) {
            let keys: BTreeSet<_> = b.keys().chain(o.keys()).chain(t.keys()).collect();
            let mut result = serde_json::Map::new();
            for k in keys {
                let value = combine(
                    &format!("{path}/{k}"),
                    b.get(k).unwrap_or(&Value::Null),
                    o.get(k).unwrap_or(&Value::Null),
                    t.get(k).unwrap_or(&Value::Null),
                )?;
                if !value.is_null() {
                    result.insert(k.clone(), value);
                }
            }
            return Ok(Value::Object(result));
        }
        if let (Some(bm), Some(om), Some(tm)) = (entities(b), entities(o), entities(t)) {
            let keys: BTreeSet<_> = bm.keys().chain(om.keys()).chain(tm.keys()).collect();
            let mut merged = BTreeMap::new();
            for key in keys {
                let value = combine(
                    &format!("{path}/{key}"),
                    bm.get(key).copied().unwrap_or(&Value::Null),
                    om.get(key).copied().unwrap_or(&Value::Null),
                    tm.get(key).copied().unwrap_or(&Value::Null),
                )?;
                if !value.is_null() {
                    merged.insert(key.to_string(), value);
                }
            }
            let order = order(path, b, o, t, &merged)?;
            return Ok(Value::Array(
                order
                    .into_iter()
                    .filter_map(|id| merged.remove(&id))
                    .collect(),
            ));
        }
        Err(CadError::Invalid(format!(
            "Semantic merge conflict at {path}"
        )))
    }
    if base.id != ours.id || base.id != theirs.id {
        return Err(CadError::Invalid(
            "Merge inputs must share one document identity".into(),
        ));
    }
    let value = combine(
        "",
        &serde_json::to_value(base)?,
        &serde_json::to_value(ours)?,
        &serde_json::to_value(theirs)?,
    )?;
    let doc: Document = serde_json::from_value(value)?;
    doc.validate()?;
    Ok(doc)
}
/// Read a complete native document at a Git revision without changing the checkout.
pub fn snapshot(root: &Path, revision: &str) -> Result<Document> {
    snapshot_manifest(root, revision, "project.json")
}
pub fn snapshot_manifest(root: &Path, revision: &str, manifest: &str) -> Result<Document> {
    snapshot_at_manifest(root, revision, &project_prefix(root)?, manifest)
}
/// `project_path` is repository-relative and can address a project inside a monorepo.
pub fn snapshot_at(root: &Path, revision: &str, project_path: &str) -> Result<Document> {
    snapshot_at_manifest(root, revision, project_path, "project.json")
}
fn snapshot_at_manifest(
    root: &Path,
    revision: &str,
    project_path: &str,
    manifest_name: &str,
) -> Result<Document> {
    repository_project_prefix(manifest_name)?;
    let revision = resolve_commit(root, revision)?;
    let prefix = repository_project_prefix(project_path)?;
    let manifest: Value =
        serde_json::from_slice(&blob(root, &revision, &format!("{prefix}{manifest_name}"))?)?;
    if manifest.get("schema_version").and_then(Value::as_u64) != Some(1) {
        return Err(CadError::Invalid(
            "Unsupported project revision schema".into(),
        ));
    }
    let mut doc: Document = serde_json::from_value(
        manifest
            .get("document")
            .cloned()
            .ok_or_else(|| CadError::Invalid("Revision has no native document".into()))?,
    )?;
    for (kind, part) in [("parts", true), ("assemblies", false)] {
        let map = manifest
            .get(kind)
            .and_then(Value::as_object)
            .ok_or_else(|| CadError::Invalid("Invalid revision manifest".into()))?;
        for (id, path) in map {
            let path = path
                .as_str()
                .ok_or_else(|| CadError::Invalid("Invalid revision path".into()))?;
            repository_project_prefix(path)?;
            let source = blob(root, &revision, &format!("{prefix}{path}"))?;
            if part {
                doc.parts
                    .insert(id.clone(), serde_json::from_slice(&source)?);
            } else {
                doc.assemblies
                    .insert(id.clone(), serde_json::from_slice(&source)?);
            }
        }
    }
    doc.validate()?;
    Ok(doc)
}
fn repository_project_prefix(path: &str) -> Result<String> {
    crate::persistence::safe_join(Path::new("."), path)?;
    if path
        .split('/')
        .any(|part| part == ".git" || part == ".tcad-recovery")
        || path.chars().any(char::is_control)
    {
        return Err(CadError::Invalid(
            "Use a portable project path outside Git metadata".into(),
        ));
    }
    let path = path.trim_matches('/');
    Ok(if path.is_empty() || path == "." {
        String::new()
    } else {
        format!("{path}/")
    })
}
fn blob(root: &Path, revision: &str, path: &str) -> Result<Vec<u8>> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["show", &format!("{revision}:{path}")])
        .output()?;
    if !output.status.success() {
        return Err(CadError::Invalid(format!(
            "Revision lacks {path}: {}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    Ok(output.stdout)
}
fn resource_bytes(
    root: &Path,
    revision: &str,
    prefix: &str,
    resource: &crate::Resource,
) -> Result<Vec<u8>> {
    use sha2::{Digest, Sha256};
    repository_project_prefix(&resource.path)?;
    let path = format!("{prefix}{}", resource.path);
    let entry = Git.command(
        root,
        &[
            "--literal-pathspecs",
            "ls-tree",
            "--full-tree",
            "-z",
            revision,
            "--",
            &path,
        ],
    )?;
    if !entry.starts_with("100644 blob ") && !entry.starts_with("100755 blob ") {
        return Err(CadError::Invalid(format!(
            "Revision resource {} must be a regular tracked file",
            resource.path
        )));
    }
    let bytes = blob(root, revision, &path)?;
    if format!("{:x}", Sha256::digest(&bytes)) != resource.sha256 {
        return Err(CadError::Invalid(format!(
            "Revision resource {} does not match its recorded SHA-256",
            resource.path
        )));
    }
    Ok(bytes)
}
fn verify_snapshot_resources(root: &Path, revision: &str, document: &Document) -> Result<()> {
    let prefix = project_prefix(root)?;
    for resource in document.resources.values() {
        resource_bytes(root, revision, &prefix, resource)?;
    }
    Ok(())
}
/// Materialize pinned bytes in a disposable root. The source checkout is never changed.
pub fn snapshot_resources(root: &Path, revision: &str, destination: &Path) -> Result<Document> {
    snapshot_resources_manifest(root, revision, "project.json", destination)
}
pub fn snapshot_resources_manifest(
    root: &Path,
    revision: &str,
    manifest: &str,
    destination: &Path,
) -> Result<Document> {
    snapshot_resources_at_manifest(
        root,
        revision,
        &project_prefix(root)?,
        manifest,
        destination,
    )
}
pub fn snapshot_resources_at(
    root: &Path,
    revision: &str,
    project_path: &str,
    destination: &Path,
) -> Result<Document> {
    snapshot_resources_at_manifest(root, revision, project_path, "project.json", destination)
}
fn snapshot_resources_at_manifest(
    root: &Path,
    revision: &str,
    project_path: &str,
    manifest: &str,
    destination: &Path,
) -> Result<Document> {
    let revision = resolve_commit(root, revision)?;
    let prefix = repository_project_prefix(project_path)?;
    let mut document = snapshot_at_manifest(root, &revision, project_path, manifest)?;
    for resource in document.resources.values() {
        let bytes = resource_bytes(root, &revision, &prefix, resource)?;
        crate::persistence::atomic_write(
            &crate::persistence::safe_join(destination, &resource.path)?,
            &bytes,
        )?;
    }
    crate::library::hydrate_pins(&mut document, destination);
    document.validate()?;
    Ok(document)
}

/// Copy one selected immutable resource from a revision without reading unrelated resources.
pub fn snapshot_resource(
    root: &Path,
    revision: &str,
    resource: &crate::Resource,
    destination: &Path,
) -> Result<()> {
    let revision = resolve_commit(root, revision)?;
    let bytes = resource_bytes(root, &revision, &project_prefix(root)?, resource)?;
    crate::persistence::atomic_write(
        &crate::persistence::safe_join(destination, &resource.path)?,
        &bytes,
    )
}

/// A complete branch merge must include the non-CAD files too. CAD conflicts
/// are replaced only by the reviewed native document. Other conflicts stop the merge.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BranchMergeInfo {
    pub branch: String,
    pub head: String,
    pub other: String,
    pub base: String,
    pub other_files: Vec<String>,
}
pub fn merge_base(root: &Path, head: &str, other: &str) -> Result<String> {
    let bases = Git.command(root, &["merge-base", "--all", head, other])?;
    let bases = bases
        .lines()
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();
    if bases.len() != 1 {
        return Err(CadError::Invalid("This branch merge has no single common base. Resolve its ancestry with Git, or transfer selected features".into()));
    }
    Ok(bases[0].into())
}
struct MergeIndex {
    index: PathBuf,
    repository: PathBuf,
    project: BTreeSet<String>,
    canonical: BTreeSet<String>,
    info: BranchMergeInfo,
}
impl Drop for MergeIndex {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.index);
    }
}
impl MergeIndex {
    fn command(&self, args: &[&str]) -> Result<String> {
        let output = Command::new("git")
            .arg("-C")
            .arg(&self.repository)
            .args(args)
            .env("GIT_INDEX_FILE", &self.index)
            .env("GIT_TERMINAL_PROMPT", "0")
            .output()?;
        if !output.status.success() {
            return Err(CadError::Invalid(
                String::from_utf8_lossy(&output.stderr).into(),
            ));
        }
        String::from_utf8(output.stdout)
            .map_err(|_| CadError::Invalid("Git paths must use UTF-8 for a CAD merge".into()))
    }
    fn add(&self, files: &BTreeSet<String>) -> Result<()> {
        if files.is_empty() {
            return Ok(());
        }
        let mut add = vec!["--literal-pathspecs", "add", "--all", "--"];
        add.extend(files.iter().map(String::as_str));
        self.command(&add)?;
        Ok(())
    }
}
fn changed_paths(root: &Path, args: &[&str]) -> Result<BTreeSet<String>> {
    Ok(Git
        .command(root, args)?
        .split('\0')
        .filter(|path| !path.is_empty())
        .map(str::to_string)
        .collect())
}
fn merge_index(
    root: &Path,
    manifest: &str,
    expected_head: &str,
    other_commit: &str,
) -> Result<MergeIndex> {
    let head = resolve_commit(root, "HEAD")?;
    if head != expected_head {
        return Err(CadError::Invalid(
            "The branch changed after merge review. Preview the merge again".into(),
        ));
    }
    let other = resolve_commit(root, other_commit)?;
    let base = merge_base(root, &head, &other)?;
    let repository = repository_root(root)?;
    let branch = Git
        .command(root, &["symbolic-ref", "HEAD"])?
        .trim()
        .to_string();
    let prefix = project_prefix(root)?;
    let mut canonical = BTreeSet::from([manifest.to_string()]);
    let current: Value = serde_json::from_slice(&std::fs::read(root.join(manifest))?)?;
    manifest_files(&current, &mut canonical)?;
    for revision in [&head, &other] {
        let manifest: Value =
            serde_json::from_slice(&blob(root, revision, &format!("{prefix}{manifest}"))?)?;
        manifest_files(&manifest, &mut canonical)?;
    }
    let canonical = canonical
        .into_iter()
        .map(|file| format!("{prefix}{file}"))
        .collect::<BTreeSet<_>>();
    let project = project_files(root, manifest)?
        .into_iter()
        .map(|file| format!("{prefix}{file}"))
        .collect();
    let index = root.join(format!(".tcad-cache/merge-index-{}", crate::id()));
    std::fs::create_dir_all(index.parent().unwrap())?;
    let mut merged = MergeIndex {
        index,
        repository,
        project,
        canonical,
        info: BranchMergeInfo {
            branch,
            head,
            other,
            base,
            other_files: vec![],
        },
    };
    merged.command(&[
        "read-tree",
        "-m",
        &merged.info.base,
        &merged.info.head,
        &merged.info.other,
    ])?;
    let unresolved = merged.command(&["ls-files", "--unmerged", "-z"])?;
    let outside_conflicts = unresolved
        .split('\0')
        .filter_map(|entry| entry.split_once('\t').map(|(_, path)| path))
        .filter(|path| !merged.canonical.contains(*path))
        .collect::<BTreeSet<_>>();
    if !outside_conflicts.is_empty() {
        return Err(CadError::Invalid(format!(
            "Branch merge has file conflicts outside the CAD model: {}. Resolve these with Git, or transfer selected features",
            outside_conflicts.into_iter().collect::<Vec<_>>().join(", ")
        )));
    }
    // A canonical path absent from both the checkout and index is not an add pathspec.
    let tracked = merged.command(&["ls-files", "-z"])?;
    let tracked = tracked.split('\0').collect::<BTreeSet<_>>();
    let actual = merged
        .canonical
        .iter()
        .filter(|path| merged.repository.join(path).exists() || tracked.contains(path.as_str()))
        .cloned()
        .collect();
    merged.add(&actual)?;
    let tree = merged.command(&["write-tree"])?;
    let changed = changed_paths(
        &merged.repository,
        &["diff", "--name-only", "-z", &merged.info.head, tree.trim()],
    )?;
    merged.info.other_files = changed.difference(&merged.canonical).cloned().collect();
    let mut dirty = changed_paths(&merged.repository, &["diff", "--name-only", "-z", "HEAD"])?;
    dirty.extend(changed_paths(
        &merged.repository,
        &["diff", "--cached", "--name-only", "-z", "HEAD"],
    )?);
    let untracked = changed_paths(&merged.repository, &["ls-files", "--others", "-z"])?;
    let collisions = merged
        .info
        .other_files
        .iter()
        .filter(|path| {
            dirty.contains(*path)
                || untracked.iter().any(|local| {
                    local == *path
                        || local.starts_with(&format!("{path}/"))
                        || path.starts_with(&format!("{local}/"))
                })
        })
        .cloned()
        .collect::<Vec<_>>();
    if !collisions.is_empty() {
        return Err(CadError::Invalid(format!(
            "Commit or move local changes before merging these incoming files: {}",
            collisions.join(", ")
        )));
    }
    Ok(merged)
}
/// Inspect the full-tree merge before previewing geometry. No checkout/index is changed.
pub fn branch_merge_info(root: &Path, manifest: &str, revision: &str) -> Result<BranchMergeInfo> {
    let head = resolve_commit(root, "HEAD")?;
    let merged = merge_index(root, manifest, &head, revision)?;
    Ok(merged.info.clone())
}
/// Commit a reviewed semantic merge with two standard Git parents. Preserve
/// clean incoming non-CAD changes and reject their conflicts instead of dropping them.
pub fn commit_semantic_merge(
    root: &Path,
    manifest: &str,
    message: &str,
    expected_branch: &str,
    expected_head: &str,
    other_commit: &str,
) -> Result<String> {
    if message.trim().is_empty() || message.contains('\0') || message.len() > 65_536 {
        return Err(CadError::Invalid("Enter a merge commit message".into()));
    }
    if merge_recovery::inspect(root)?.is_some() {
        return Err(CadError::Invalid(
            "Finish the interrupted merge checkout before committing".into(),
        ));
    }
    verify_saved_project(root, manifest)?;
    let merged = merge_index(root, manifest, expected_head, other_commit)?;
    let branch = Git.command(root, &["symbolic-ref", "HEAD"])?;
    if branch.trim() != expected_branch {
        return Err(CadError::Invalid(
            "The selected branch changed after merge review. Preview the merge again".into(),
        ));
    }
    if Git
        .command(root, &["rev-parse", "--verify", "MERGE_HEAD"])
        .is_ok()
    {
        return Err(CadError::Invalid(
            "Finish the existing Git merge before committing a CAD merge".into(),
        ));
    }
    if !Git
        .command(root, &["diff", "--name-only", "--diff-filter=U"])?
        .trim()
        .is_empty()
    {
        return Err(CadError::Invalid(
            "Resolve existing Git index conflicts before committing".into(),
        ));
    }
    // Extra project files (README preview/layout) may have local changes. Add them
    // only when the full-tree merge does not contain an incoming version of that path.
    let extras = merged
        .project
        .iter()
        .filter(|path| {
            !merged.canonical.contains(*path) && !merged.info.other_files.contains(*path)
        })
        .cloned()
        .collect();
    merged.add(&extras)?;
    let tree = merged.command(&["write-tree"])?;
    let commit = merged.command(&[
        "commit-tree",
        tree.trim(),
        "-p",
        &merged.info.head,
        "-p",
        &merged.info.other,
        "-m",
        message,
    ])?;
    let commit = commit.trim();
    let reset = merged
        .project
        .iter()
        .chain(&merged.canonical)
        .cloned()
        .collect::<BTreeSet<_>>();
    let recovery = merge_recovery::prepare(
        &merged.repository,
        expected_branch,
        &merged.info.head,
        commit,
        merged.info.other_files.clone(),
        reset.into_iter().collect(),
    )?;
    if let Err(error) = Git.command(
        &merged.repository,
        &[
            "update-ref",
            "-m",
            "TCAD reviewed semantic merge",
            expected_branch,
            commit,
            &merged.info.head,
        ],
    ) {
        return match recovery.abort_if_uncommitted() {
            Ok(()) => Err(error),
            Err(recovery_error) => Err(CadError::Invalid(format!("{error}. {recovery_error}"))),
        };
    }
    recovery.finish()
}

/// Stage merge resources without changing any existing project bytes. A changed
/// path is moved to an immutable content-addressed file before draft validation.
/// Cancel therefore leaves all committed assets intact.
pub fn materialize_merge_resources(
    document: &mut Document,
    sources: &[(&Document, &Path)],
    destination: &Path,
    manifest: &str,
) -> Result<()> {
    use sha2::{Digest, Sha256};
    use std::io::Write;
    fn checked(root: &Path, relative: &str) -> Result<PathBuf> {
        let path = crate::persistence::safe_join(root, relative)?;
        let mut current = root.to_path_buf();
        for component in Path::new(relative).components() {
            current.push(component);
            if std::fs::symlink_metadata(&current)
                .ok()
                .is_some_and(|metadata| metadata.file_type().is_symlink())
            {
                return Err(CadError::Invalid(
                    "Merge resources cannot pass through symbolic links".into(),
                ));
            }
        }
        Ok(path)
    }
    let mut candidate = document.clone();
    let mut copies = Vec::new();
    for resource in candidate.resources.values_mut() {
        repository_project_prefix(&resource.path)?;
        if resource.path.split('/').any(|part| {
            matches!(
                part.to_ascii_lowercase().as_str(),
                ".git" | ".tcad-cache" | ".tcad-recovery"
            )
        }) {
            return Err(CadError::Invalid(
                "A merge resource cannot name Git or recovery files".into(),
            ));
        }
        let normalized = Path::new(&resource.path)
            .components()
            .filter_map(|part| match part {
                std::path::Component::Normal(name) => name.to_str(),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("/");
        let first = normalized.split('/').next().unwrap_or_default();
        if normalized.eq_ignore_ascii_case(manifest)
            || normalized.eq_ignore_ascii_case("project.json")
            || normalized.eq_ignore_ascii_case("README.md")
            || matches!(
                first.to_ascii_lowercase().as_str(),
                "parts" | "assemblies" | "ui" | "previews"
            )
        {
            return Err(CadError::Invalid("A merge resource cannot use a project definition, presentation or generated preview path".into()));
        }
        let matches = |bytes: &[u8]| format!("{:x}", Sha256::digest(bytes)) == resource.sha256;
        let original = checked(destination, &resource.path)?;
        if std::fs::read(&original)
            .ok()
            .is_some_and(|bytes| matches(&bytes))
        {
            continue;
        }
        let bytes = sources
            .iter()
            .flat_map(|(document, source)| {
                document
                    .resources
                    .values()
                    .filter(|saved| saved.sha256 == resource.sha256)
                    .filter_map(|saved| checked(source, &saved.path).ok())
            })
            .filter_map(|path| std::fs::read(path).ok())
            .find(|bytes| matches(bytes))
            .ok_or_else(|| {
                CadError::Invalid(format!(
                    "The exact merge resource {} is unavailable",
                    resource.path
                ))
            })?;
        let extension = Path::new(&resource.path)
            .extension()
            .and_then(|value| value.to_str())
            .filter(|extension| {
                extension.len() <= 16 && extension.bytes().all(|byte| byte.is_ascii_alphanumeric())
            })
            .unwrap_or("bin");
        resource.path = format!("imports/{}.{}", resource.sha256, extension);
        let target = checked(destination, &resource.path)?;
        if let Ok(existing) = std::fs::read(&target) {
            if !matches(&existing) {
                return Err(CadError::Invalid("An immutable merge resource path contains different bytes; no existing file was replaced".into()));
            }
        } else {
            copies.push((target, bytes, resource.sha256.clone()));
        }
    }
    candidate.validate()?;
    for (target, bytes, hash) in copies {
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let temp = target.with_file_name(format!(".merge-resource-{}", crate::id()));
        let result = (|| {
            let mut file = std::fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&temp)?;
            file.write_all(&bytes)?;
            file.sync_all()?;
            match std::fs::hard_link(&temp, &target) {
                Ok(()) => {
                    #[cfg(unix)]
                    if let Some(parent) = target.parent() {
                        std::fs::File::open(parent)?.sync_all()?;
                    }
                    Ok(())
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    if format!("{:x}", Sha256::digest(std::fs::read(&target)?)) == hash {
                        Ok(())
                    } else {
                        Err(CadError::Invalid(
                            "A merge resource path changed while staging; no file was replaced"
                                .into(),
                        ))
                    }
                }
                Err(error) => Err(error.into()),
            }
        })();
        let _ = std::fs::remove_file(temp);
        result?;
    }
    *document = candidate;
    Ok(())
}
