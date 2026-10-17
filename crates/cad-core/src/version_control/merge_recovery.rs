//! Durable completion of a reviewed merge after its branch reference moves.
//! Only recorded paths are touched. Unrelated staged changes remain in the real index.
use crate::{CadError, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    process::Command,
};

const VERSION: u32 = 1;
const MAX_PATHS: usize = 100_000;
const MAX_JOURNAL: u64 = 64 * 1024 * 1024;
const JOURNAL: &str = "tcad-merge-recovery.json";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
struct IndexEntry {
    mode: String,
    object: String,
    stage: u8,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum WorkingFile {
    Missing,
    Regular {
        sha256: String,
        bytes: u64,
        executable: bool,
    },
    Symlink {
        sha256: String,
        bytes: u64,
    },
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
struct Entry {
    path: String,
    incoming: bool,
    baseline_file: WorkingFile,
    baseline_index: Vec<IndexEntry>,
    desired_index: Vec<IndexEntry>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
struct Record {
    version: u32,
    id: String,
    repository: PathBuf,
    branch: String,
    old_commit: String,
    new_commit: String,
    entries: Vec<Entry>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Status {
    pub branch: String,
    pub old_commit: String,
    pub new_commit: String,
    pub incoming: Vec<String>,
    pub reset: Vec<String>,
    pub phase: String,
    pub message: String,
}
pub struct Recovery {
    path: PathBuf,
    record: Record,
}
fn invalid(message: impl Into<String>) -> CadError {
    CadError::Invalid(format!("Merge checkout recovery: {}", message.into()))
}
fn output(repository: &Path, args: &[&str]) -> Result<std::process::Output> {
    Ok(Command::new("git")
        .arg("-C")
        .arg(repository)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GCM_INTERACTIVE", "never")
        .output()?)
}
fn git(repository: &Path, args: &[&str]) -> Result<String> {
    let result = output(repository, args)?;
    if !result.status.success() {
        return Err(invalid(
            String::from_utf8_lossy(&result.stderr).trim().to_owned(),
        ));
    }
    String::from_utf8(result.stdout).map_err(|_| invalid("Repository paths must use UTF-8"))
}
fn journal_path(repository: &Path) -> Result<PathBuf> {
    let path = PathBuf::from(git(repository, &["rev-parse", "--git-path", JOURNAL])?.trim());
    Ok(if path.is_absolute() {
        path
    } else {
        repository.join(path)
    })
}
fn is_object_id(value: &str) -> bool {
    matches!(value.len(), 40 | 64) && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}
fn exact_commit(repository: &Path, revision: &str) -> Result<String> {
    if !is_object_id(revision) {
        return Err(invalid("Recovery requires exact commit identities"));
    }
    let resolved = super::resolve_commit(repository, revision)?;
    if !resolved.eq_ignore_ascii_case(revision) {
        return Err(invalid("Commit identity changed"));
    }
    Ok(resolved)
}
fn validate_path(path: &str) -> Result<()> {
    if path.is_empty()
        || path.len() > 1024
        || path.chars().any(char::is_control)
        || Path::new(path)
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
        || path.split('/').any(|part| {
            matches!(
                part.to_ascii_lowercase().as_str(),
                ".git" | ".tcad-recovery" | ".tcad-cache"
            )
        })
    {
        return Err(invalid(
            "A checkout path is empty, outside the repository, or reserved metadata",
        ));
    }
    Ok(())
}
/// Do not follow a link in a parent directory. Git may replace the final link itself.
fn file_path(repository: &Path, relative: &str) -> Result<PathBuf> {
    validate_path(relative)?;
    let mut path = repository.to_path_buf();
    let components = Path::new(relative).components().collect::<Vec<_>>();
    for (index, component) in components.iter().enumerate() {
        path.push(component.as_os_str());
        if index + 1 == components.len() {
            break;
        }
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {}
            Ok(_) => {
                return Err(invalid(format!(
                    "{relative} crosses a file or symbolic link; finish this directory/file transition with Git"
                )));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(path)
}
fn working_file(repository: &Path, relative: &str) -> Result<WorkingFile> {
    let path = file_path(repository, relative)?;
    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(WorkingFile::Missing);
        }
        Err(error) => return Err(error.into()),
    };
    if metadata.file_type().is_symlink() {
        let target = fs::read_link(&path)?;
        #[cfg(unix)]
        let bytes = {
            use std::os::unix::ffi::OsStrExt;
            target.as_os_str().as_bytes().to_vec()
        };
        #[cfg(not(unix))]
        let bytes = target.to_string_lossy().as_bytes().to_vec();
        return Ok(WorkingFile::Symlink {
            sha256: format!("{:x}", Sha256::digest(&bytes)),
            bytes: bytes.len() as u64,
        });
    }
    if !metadata.is_file() {
        return Err(invalid(format!(
            "{relative} is a directory or special file; finish this file-type transition with Git"
        )));
    }
    let mut file = File::open(path)?;
    let mut digest = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    let mut bytes = 0u64;
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
        bytes = bytes
            .checked_add(count as u64)
            .ok_or_else(|| invalid("File length overflow"))?;
    }
    #[cfg(unix)]
    let executable = {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    };
    #[cfg(not(unix))]
    let executable = false;
    Ok(WorkingFile::Regular {
        sha256: format!("{:x}", digest.finalize()),
        bytes,
        executable,
    })
}
fn allowed_mode(mode: &str) -> bool {
    matches!(mode, "100644" | "100755" | "120000")
}
/// A disposable expected index recognizes files restored before a crash even
/// when Git had not yet published its real index. No working file is written.
struct ExpectedIndex {
    repository: PathBuf,
    path: PathBuf,
}
impl Drop for ExpectedIndex {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}
impl ExpectedIndex {
    fn command(&self, args: &[&str]) -> Result<String> {
        let result = Command::new("git")
            .arg("-C")
            .arg(&self.repository)
            .args(args)
            .env("GIT_INDEX_FILE", &self.path)
            .env("GIT_TERMINAL_PROMPT", "0")
            .output()?;
        if !result.status.success() {
            return Err(invalid(
                String::from_utf8_lossy(&result.stderr).trim().to_owned(),
            ));
        }
        String::from_utf8(result.stdout).map_err(|_| invalid("Repository paths must use UTF-8"))
    }
    fn at_commit(repository: &Path, journal: &Path, commit: &str) -> Result<Self> {
        let index = Self {
            repository: repository.to_path_buf(),
            path: journal.with_extension(format!("{}.index", crate::id())),
        };
        index.command(&["read-tree", commit])?;
        Ok(index)
    }
    fn new(recovery: &Recovery) -> Result<Self> {
        Self::at_commit(
            &recovery.record.repository,
            &recovery.path,
            &recovery.record.new_commit,
        )
    }
    fn changed(&self, entries: &[Entry]) -> Result<BTreeSet<String>> {
        // read-tree has no working-file stat cache. Refresh matching entries in
        // this disposable index so diff-files compares actual filtered content.
        let refreshed = Command::new("git")
            .arg("-C")
            .arg(&self.repository)
            .args([
                "-c",
                "core.fsmonitor=false",
                "update-index",
                "-q",
                "--refresh",
                "--ignore-missing",
            ])
            .env("GIT_INDEX_FILE", &self.path)
            .env("GIT_TERMINAL_PROMPT", "0")
            .output()?;
        if !matches!(refreshed.status.code(), Some(0 | 1)) {
            return Err(invalid(
                String::from_utf8_lossy(&refreshed.stderr).trim().to_owned(),
            ));
        }
        let paths = entries
            .iter()
            .filter(|entry| entry.incoming)
            .map(|entry| entry.path.as_str())
            .collect::<Vec<_>>();
        let mut changed = BTreeSet::new();
        for chunk in paths.chunks(128) {
            let mut args = vec![
                "--literal-pathspecs",
                "diff-files",
                "--no-ext-diff",
                "--name-only",
                "-z",
                "--",
            ];
            args.extend(chunk.iter().copied());
            changed.extend(
                self.command(&args)?
                    .split('\0')
                    .filter(|path| !path.is_empty())
                    .map(str::to_owned),
            );
        }
        Ok(changed)
    }
}
fn working_matches_index(
    path: &str,
    index: &[IndexEntry],
    working: &WorkingFile,
    changed: &BTreeSet<String>,
) -> bool {
    if index.is_empty() {
        matches!(working, WorkingFile::Missing)
    } else {
        !matches!(working, WorkingFile::Missing) && !changed.contains(path)
    }
}
fn working_is_expected(entry: &Entry, working: &WorkingFile, changed: &BTreeSet<String>) -> bool {
    working_matches_index(&entry.path, &entry.desired_index, working, changed)
}

fn index_batch(repository: &Path, paths: &[String]) -> Result<BTreeMap<String, Vec<IndexEntry>>> {
    let requested = paths.iter().cloned().collect::<BTreeSet<_>>();
    let mut result = BTreeMap::new();
    for chunk in paths.chunks(128) {
        let mut args = vec!["--literal-pathspecs", "ls-files", "--stage", "-z", "--"];
        args.extend(chunk.iter().map(String::as_str));
        for record in git(repository, &args)?
            .split('\0')
            .filter(|record| !record.is_empty())
        {
            let (fields, path) = record
                .split_once('\t')
                .ok_or_else(|| invalid("Malformed index entry"))?;
            if !requested.contains(path) {
                return Err(invalid("A checkout path identifies a directory"));
            }
            let fields = fields.split_whitespace().collect::<Vec<_>>();
            if fields.len() != 3 || !allowed_mode(fields[0]) || fields[2] != "0" {
                return Err(invalid(
                    "Resolve index conflicts, submodules or file-type transitions with Git",
                ));
            }
            result
                .entry(path.into())
                .or_insert_with(Vec::new)
                .push(IndexEntry {
                    mode: fields[0].into(),
                    object: fields[1].into(),
                    stage: 0,
                });
        }
    }
    Ok(result)
}
fn tree_batch(
    repository: &Path,
    commit: &str,
    paths: &[String],
) -> Result<BTreeMap<String, Vec<IndexEntry>>> {
    let requested = paths.iter().cloned().collect::<BTreeSet<_>>();
    let mut result = BTreeMap::new();
    for chunk in paths.chunks(128) {
        let mut args = vec!["--literal-pathspecs", "ls-tree", "-z", commit, "--"];
        args.extend(chunk.iter().map(String::as_str));
        for record in git(repository, &args)?
            .split('\0')
            .filter(|record| !record.is_empty())
        {
            let (fields, path) = record
                .split_once('\t')
                .ok_or_else(|| invalid("Malformed commit tree entry"))?;
            let fields = fields.split_whitespace().collect::<Vec<_>>();
            if !requested.contains(path)
                || fields.len() != 3
                || fields[1] != "blob"
                || !allowed_mode(fields[0])
            {
                return Err(invalid(
                    "Submodules and directory/file transitions require Git checkout",
                ));
            }
            result
                .entry(path.into())
                .or_insert_with(Vec::new)
                .push(IndexEntry {
                    mode: fields[0].into(),
                    object: fields[2].into(),
                    stage: 0,
                });
        }
    }
    Ok(result)
}
fn read_record(path: &Path) -> Result<Record> {
    let file = File::open(path)?;
    if file.metadata()?.len() > MAX_JOURNAL {
        return Err(invalid("Recovery journal exceeds its bounded size"));
    }
    let mut bytes = vec![];
    file.take(MAX_JOURNAL + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_JOURNAL {
        return Err(invalid("Recovery journal exceeds its bounded size"));
    }
    let record: Record = serde_json::from_slice(&bytes)?;
    if record.version != VERSION
        || record.entries.len() > MAX_PATHS
        || record.id.is_empty()
        || !is_object_id(&record.old_commit)
        || !is_object_id(&record.new_commit)
        || !record.branch.starts_with("refs/heads/")
        || record.branch.chars().any(char::is_control)
    {
        return Err(invalid("Unsupported recovery journal"));
    }
    let mut paths = BTreeSet::new();
    for entry in &record.entries {
        validate_path(&entry.path)?;
        if !paths.insert(&entry.path) {
            return Err(invalid("Duplicate recovery path"));
        }
        for index in [&entry.baseline_index, &entry.desired_index] {
            if index.len() > 1
                || index.iter().any(|item| {
                    item.stage != 0 || !allowed_mode(&item.mode) || !is_object_id(&item.object)
                })
            {
                return Err(invalid("Unsupported recovery index entry"));
            }
        }
    }
    Ok(record)
}
fn sync_parent(path: &Path) -> Result<()> {
    #[cfg(unix)]
    if let Some(parent) = path.parent() {
        File::open(parent)?.sync_all()?;
    }
    Ok(())
}
fn branch(repository: &Path) -> Result<String> {
    Ok(git(repository, &["symbolic-ref", "HEAD"])?.trim().into())
}
fn head(repository: &Path) -> Result<String> {
    super::resolve_commit(repository, "HEAD")
}

/// Persist before update-ref. Creating a second merge while recovery is pending is rejected.
pub fn prepare(
    repository: &Path,
    branch_ref: &str,
    old_commit: &str,
    new_commit: &str,
    incoming: Vec<String>,
    reset: Vec<String>,
) -> Result<Recovery> {
    let repository = super::repository_root(repository)?.canonicalize()?;
    if !branch_ref.starts_with("refs/heads/") {
        return Err(invalid("A merge must target an ordinary branch"));
    }
    git(&repository, &["check-ref-format", branch_ref])?;
    let old_commit = exact_commit(&repository, old_commit)?;
    let new_commit = exact_commit(&repository, new_commit)?;
    if branch(&repository)? != branch_ref || head(&repository)? != old_commit {
        return Err(invalid(
            "The checked-out branch changed before merge commit",
        ));
    }
    let path = journal_path(&repository)?;
    if path.exists() {
        return Err(invalid(
            "Finish the previous merge checkout before committing another merge",
        ));
    }
    let incoming = incoming.into_iter().collect::<BTreeSet<_>>();
    let paths = incoming
        .iter()
        .cloned()
        .chain(reset)
        .collect::<BTreeSet<_>>();
    if paths.len() > MAX_PATHS {
        return Err(invalid("The merge affects more than 100000 paths"));
    }
    let paths = paths.into_iter().collect::<Vec<_>>();
    for path in &paths {
        validate_path(path)?;
    }
    let baseline_index = index_batch(&repository, &paths)?;
    let desired_index = tree_batch(&repository, &new_commit, &paths)?;
    let mut entries = Vec::with_capacity(paths.len());
    for path in paths {
        entries.push(Entry {
            incoming: incoming.contains(&path),
            baseline_file: working_file(&repository, &path)?,
            baseline_index: baseline_index.get(&path).cloned().unwrap_or_default(),
            desired_index: desired_index.get(&path).cloned().unwrap_or_default(),
            path,
        });
    }
    // An incoming checkout must start from HEAD, not merely from whatever
    // happened to be present when this journal was captured. A second editor
    // may have changed a path since the merge planner's earlier clean check.
    let incoming_paths = incoming.iter().cloned().collect::<Vec<_>>();
    let old_tree = tree_batch(&repository, &old_commit, &incoming_paths)?;
    let old_expected = ExpectedIndex::at_commit(&repository, &path, &old_commit)?;
    let changed = old_expected.changed(&entries)?;
    let current_index = index_batch(&repository, &incoming_paths)?;
    for entry in entries.iter().filter(|entry| entry.incoming) {
        let old = old_tree
            .get(&entry.path)
            .map(Vec::as_slice)
            .unwrap_or_default();
        let current = current_index
            .get(&entry.path)
            .map(Vec::as_slice)
            .unwrap_or_default();
        if entry.baseline_index != old
            || current != old
            || !working_matches_index(&entry.path, old, &entry.baseline_file, &changed)
            || working_file(&repository, &entry.path)? != entry.baseline_file
        {
            return Err(invalid(format!(
                "{} has local or staged edits; preserve them before committing this merge",
                entry.path
            )));
        }
    }
    if branch(&repository)? != branch_ref || head(&repository)? != old_commit {
        return Err(invalid(
            "The checked-out branch changed while preparing recovery",
        ));
    }
    let record = Record {
        version: VERSION,
        id: crate::id(),
        repository,
        branch: branch_ref.into(),
        old_commit,
        new_commit,
        entries,
    };
    let bytes = serde_json::to_vec(&record)?;
    if bytes.len() as u64 > MAX_JOURNAL {
        return Err(invalid("The merge recovery journal exceeds 64 MiB"));
    }
    let temporary = path.with_extension(format!("{}.tmp", record.id));
    let result = (|| -> Result<()> {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        // Hard-link publication is atomic and cannot replace another pending journal.
        fs::hard_link(&temporary, &path)?;
        sync_parent(&path)?;
        Ok(())
    })();
    let _ = fs::remove_file(temporary);
    result?;
    Ok(Recovery { path, record })
}

impl Recovery {
    fn current(&self) -> Result<()> {
        let record = read_record(&self.path)?;
        if record != self.record {
            return Err(invalid("The recovery operation changed"));
        }
        if super::repository_root(&self.record.repository)?.canonicalize()?
            != self.record.repository
        {
            return Err(invalid("The recovery worktree moved; inspect it with Git"));
        }
        Ok(())
    }
    fn clear(&self) -> Result<()> {
        self.current()?;
        fs::remove_file(&self.path)?;
        sync_parent(&self.path)
    }
    /// Complete only unchanged captured files or files already restored to this exact commit.
    pub fn finish(&self) -> Result<String> {
        self.current()?;
        let repository = &self.record.repository;
        if branch(repository)? != self.record.branch || head(repository)? != self.record.new_commit
        {
            return Err(invalid(format!(
                "Checkout must be on {} at {}. The journal was retained; no files were overwritten",
                self.record.branch, self.record.new_commit
            )));
        }
        let paths = self
            .record
            .entries
            .iter()
            .map(|entry| entry.path.clone())
            .collect::<Vec<_>>();
        let indexes = index_batch(repository, &paths)?;
        let desired = tree_batch(repository, &self.record.new_commit, &paths)?;
        if self.record.entries.iter().any(|entry| {
            desired
                .get(&entry.path)
                .map(Vec::as_slice)
                .unwrap_or_default()
                != entry.desired_index
        }) {
            return Err(invalid(
                "The recovery journal does not match the committed tree",
            ));
        }
        let expected = ExpectedIndex::new(self)?;
        let changed = expected.changed(&self.record.entries)?;
        let mut restore = vec![];
        let mut reset = vec![];
        let mut remove = vec![];
        for entry in &self.record.entries {
            let index = indexes
                .get(&entry.path)
                .map(Vec::as_slice)
                .unwrap_or_default();
            if index != entry.baseline_index && index != entry.desired_index {
                return Err(invalid(format!(
                    "{} has new staged changes. Preserve them before recovering; the journal was retained",
                    entry.path
                )));
            }
            if entry.incoming {
                let working = working_file(repository, &entry.path)?;
                let done = working_is_expected(entry, &working, &changed);
                if working != entry.baseline_file && !done {
                    return Err(invalid(format!(
                        "{} changed after the merge started. Preserve those edits before recovering; no files were overwritten",
                        entry.path
                    )));
                }
                if !done && entry.desired_index.is_empty() {
                    // When the real index already removed a path, restore has
                    // no pathspec to match. Remove only its captured unchanged file.
                    remove.push(entry);
                    if index != entry.desired_index {
                        reset.push(entry.path.as_str());
                    }
                } else if !done {
                    restore.push(entry.path.as_str());
                } else if index != entry.desired_index {
                    // A previous restore may have written this file before a
                    // crash prevented publication of the real index.
                    reset.push(entry.path.as_str());
                }
            } else if index != entry.desired_index {
                reset.push(entry.path.as_str());
            }
        }
        // Recheck the exact symbolic branch after examining paths. Git's index
        // lock serializes each operation; a stale UI cannot choose another branch.
        if branch(repository)? != self.record.branch || head(repository)? != self.record.new_commit
        {
            return Err(invalid(
                "The checked-out branch changed during recovery; the journal was retained",
            ));
        }
        for entry in remove {
            if working_file(repository, &entry.path)? != entry.baseline_file {
                return Err(invalid(format!(
                    "{} changed while recovering; the journal was retained",
                    entry.path
                )));
            }
            let path = file_path(repository, &entry.path)?;
            fs::remove_file(&path)?;
            sync_parent(&path)?;
        }
        for chunk in restore.chunks(128) {
            let mut args = vec![
                "--literal-pathspecs",
                "restore",
                "--source",
                self.record.new_commit.as_str(),
                "--staged",
                "--worktree",
                "--",
            ];
            args.extend(chunk.iter().copied());
            git(repository, &args)?;
        }
        for chunk in reset.chunks(128) {
            let mut args = vec![
                "--literal-pathspecs",
                "reset",
                "--quiet",
                self.record.new_commit.as_str(),
                "--",
            ];
            args.extend(chunk.iter().copied());
            git(repository, &args)?;
        }
        if branch(repository)? != self.record.branch || head(repository)? != self.record.new_commit
        {
            return Err(invalid(
                "The branch changed while checkout was completing; the journal was retained",
            ));
        }
        let indexes = index_batch(repository, &paths)?;
        let changed = expected.changed(&self.record.entries)?;
        for entry in &self.record.entries {
            let index = indexes
                .get(&entry.path)
                .map(Vec::as_slice)
                .unwrap_or_default();
            if index != entry.desired_index
                || entry.incoming
                    && !working_is_expected(
                        entry,
                        &working_file(repository, &entry.path)?,
                        &changed,
                    )
            {
                return Err(invalid(format!(
                    "{} still needs checkout refresh. The recovery journal was retained",
                    entry.path
                )));
            }
        }
        self.clear()?;
        Ok(format!(
            "Merge {} is committed and its checkout is complete",
            self.record.new_commit
        ))
    }
    /// A failed reference update may discard preparation only while nothing moved.
    pub fn abort_if_uncommitted(&self) -> Result<()> {
        self.current()?;
        let repository = &self.record.repository;
        if branch(repository)? != self.record.branch || head(repository)? != self.record.old_commit
        {
            return Err(invalid(
                "The branch moved; recovery preparation cannot be discarded",
            ));
        }
        let paths = self
            .record
            .entries
            .iter()
            .map(|entry| entry.path.clone())
            .collect::<Vec<_>>();
        let indexes = index_batch(repository, &paths)?;
        for entry in &self.record.entries {
            if indexes
                .get(&entry.path)
                .map(Vec::as_slice)
                .unwrap_or_default()
                != entry.baseline_index
                || working_file(repository, &entry.path)? != entry.baseline_file
            {
                return Err(invalid(
                    "Files changed after preparation; retain the recovery journal for review",
                ));
            }
        }
        self.clear()
    }
}

pub fn inspect(repository: &Path) -> Result<Option<Status>> {
    let repository = super::repository_root(repository)?.canonicalize()?;
    let path = journal_path(&repository)?;
    if !path.exists() {
        return Ok(None);
    }
    let record = read_record(&path)?;
    if record.repository != repository {
        return Err(invalid("The journal belongs to another worktree"));
    }
    let current_branch = branch(&repository).ok();
    let current_head = head(&repository).ok();
    let (phase, message) = if current_branch.as_deref() != Some(record.branch.as_str()) {
        (
            "different_branch",
            "Return to the recorded branch before completing this checkout.",
        )
    } else if current_head.as_deref() == Some(record.new_commit.as_str()) {
        (
            "checkout_pending",
            "The merge commit exists. Finish its recorded file and index updates.",
        )
    } else if current_head.as_deref() == Some(record.old_commit.as_str()) {
        (
            "prepared",
            "The branch did not move. Discard unchanged preparation or complete the merge again.",
        )
    } else {
        (
            "different_commit",
            "The branch changed. Inspect the recorded commits before recovery.",
        )
    };
    Ok(Some(Status {
        branch: record.branch,
        old_commit: record.old_commit,
        new_commit: record.new_commit,
        incoming: record
            .entries
            .iter()
            .filter(|entry| entry.incoming)
            .map(|entry| entry.path.clone())
            .collect(),
        reset: record
            .entries
            .iter()
            .filter(|entry| !entry.incoming)
            .map(|entry| entry.path.clone())
            .collect(),
        phase: phase.into(),
        message: message.into(),
    }))
}
pub fn recover(repository: &Path) -> Result<String> {
    let repository = super::repository_root(repository)?.canonicalize()?;
    let path = journal_path(&repository)?;
    if !path.exists() {
        return Ok("No merge checkout is pending".into());
    }
    let record = read_record(&path)?;
    if record.repository != repository {
        return Err(invalid("The journal belongs to another worktree"));
    }
    let recovery = Recovery { path, record };
    if branch(&repository)? == recovery.record.branch
        && head(&repository)? == recovery.record.old_commit
    {
        recovery.abort_if_uncommitted()?;
        Ok("Uncommitted merge preparation removed. The branch and files are unchanged".into())
    } else {
        recovery.finish()
    }
}
