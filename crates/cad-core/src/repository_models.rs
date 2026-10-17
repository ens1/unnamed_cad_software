//! Bounded, forge-neutral model transfer with a GitHub transport adapter.
//! Remote checkouts are new directories. The source model is never changed.
use crate::{
    CadError, Document, Id, Result,
    forge::{CancelToken, GitHub},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    process::{Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

const MAX_JSON: u64 = 64 * 1024 * 1024;
const MAX_RESOURCE: u64 = 100 * 1024 * 1024;
const MAX_MODEL: u64 = 1024 * 1024 * 1024;
const MAX_SCAN_JSON: u64 = 256 * 1024 * 1024;
const MAX_SCAN_ENTRIES: usize = 25_000;
const MAX_DEPTH: usize = 16;
const MAX_MODELS: usize = 256;
const MAX_OUTPUT: usize = 2 * 1024 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CloneRequest {
    pub clone_url: String,
    pub destination: PathBuf,
    pub branch: Option<String>,
    pub expected_commit: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ModelEntry {
    pub manifest: PathBuf,
    pub relative_path: String,
    pub name: String,
    pub document_id: Id,
    pub parts: usize,
    pub assemblies: usize,
    pub fingerprint: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Discovery {
    pub projects: Vec<ModelEntry>,
    pub notices: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CloneResult {
    pub root: PathBuf,
    pub branch: String,
    pub commit: String,
    pub projects: Vec<ModelEntry>,
    pub notices: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GitAuthor {
    pub name: String,
    pub email: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PublishRequest {
    pub source_manifest: PathBuf,
    pub expected_fingerprint: String,
    pub clone_url: String,
    pub expected_private: bool,
    pub expected_repository_id: String,
    pub expected_login: String,
    pub destination: PathBuf,
    pub branch: Option<String>,
    /// Empty only when publishing the first model into an empty repository.
    pub folder: String,
    pub commit_message: String,
    pub author: GitAuthor,
    pub expected_remote_commit: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PublishSourceRequest {
    pub source_manifest: PathBuf,
    pub expected_fingerprint: String,
    pub destination: PathBuf,
    pub branch: Option<String>,
    pub folder: String,
    pub commit_message: String,
    pub author: GitAuthor,
}

impl From<&PublishRequest> for PublishSourceRequest {
    fn from(request: &PublishRequest) -> Self {
        Self {
            source_manifest: request.source_manifest.clone(),
            expected_fingerprint: request.expected_fingerprint.clone(),
            destination: request.destination.clone(),
            branch: request.branch.clone(),
            folder: request.folder.clone(),
            commit_message: request.commit_message.clone(),
            author: request.author.clone(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PublishResult {
    pub root: PathBuf,
    pub manifest: PathBuf,
    pub branch: String,
    pub commit: String,
    pub notices: Vec<String>,
}

fn invalid(message: impl Into<String>) -> CadError {
    CadError::Invalid(message.into())
}

fn check_cancel(cancel: &CancelToken) -> Result<()> {
    if cancel.is_cancelled() {
        Err(invalid("Repository operation cancelled"))
    } else {
        Ok(())
    }
}

fn relative_path(value: &str) -> Result<PathBuf> {
    if value.is_empty()
        || value.len() > 4096
        || value.contains('\\')
        || value.chars().any(char::is_control)
    {
        return Err(invalid("Use a portable path inside the model folder"));
    }
    let path = Path::new(value);
    for component in path.components() {
        let Component::Normal(name) = component else {
            return Err(invalid("Model paths must stay inside their folder"));
        };
        let name = name
            .to_str()
            .ok_or_else(|| invalid("Model paths must be UTF-8"))?;
        if name.contains(':')
            || name.ends_with(['.', ' '])
            || matches!(
                name.to_ascii_lowercase().as_str(),
                ".git" | ".tcad-cache" | ".tcad-recovery"
            )
        {
            return Err(invalid(
                "Model paths cannot use Git, recovery or cache folders",
            ));
        }
    }
    Ok(path.to_path_buf())
}

/// Reject every symlink component, including the supplied root. Repository data
/// never decides which local files outside that root may be read.
fn checked_path(root: &Path, relative: &str) -> Result<PathBuf> {
    let relative = relative_path(relative)?;
    let mut path = root.to_path_buf();
    for component in relative.components() {
        path.push(component);
        let metadata = fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() {
            return Err(invalid(format!(
                "Symlink model paths are not supported: {relative:?}"
            )));
        }
    }
    if !path.is_file() {
        return Err(invalid(format!(
            "Model file is missing: {}",
            relative.display()
        )));
    }
    Ok(path)
}

fn canonical_directory(path: &Path) -> Result<PathBuf> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    let mut examined = PathBuf::new();
    for component in absolute.components() {
        examined.push(component);
        if matches!(component, Component::ParentDir) {
            return Err(invalid(
                "Choose a folder without parent-directory path components",
            ));
        }
        if fs::symlink_metadata(&examined)?.file_type().is_symlink() {
            return Err(invalid("Choose a real folder, not a symlink"));
        }
    }
    if !absolute.is_dir() {
        return Err(invalid("Choose an existing parent folder"));
    }
    Ok(absolute.canonicalize()?)
}

fn read_bounded(path: &Path, limit: u64) -> Result<Vec<u8>> {
    let file = File::open(path)?;
    if !file.metadata()?.is_file() || file.metadata()?.len() > limit {
        return Err(invalid(format!(
            "{} exceeds the supported file size",
            path.display()
        )));
    }
    let mut bytes = Vec::new();
    file.take(limit + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(invalid(
            "Model file grew beyond the supported size while reading",
        ));
    }
    Ok(bytes)
}

#[derive(Deserialize)]
struct Manifest {
    schema_version: u32,
    document: Document,
    parts: BTreeMap<Id, String>,
    assemblies: BTreeMap<Id, String>,
}

struct Snapshot {
    root: PathBuf,
    document: Document,
    files: BTreeSet<String>,
}

/// Read without FileStore's recovery replay. An untrusted repository must not
/// supply a journal that moves files as a side effect of browsing its models.
fn snapshot(path: &Path, cancel: &CancelToken) -> Result<Snapshot> {
    check_cancel(cancel)?;
    let manifest = if path
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("json"))
    {
        path.to_path_buf()
    } else {
        path.join("project.json")
    };
    let root = canonical_directory(manifest.parent().unwrap_or(Path::new(".")))?;
    let name = manifest
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| invalid("Choose a JSON model manifest"))?;
    let manifest_path = checked_path(&root, name)?;
    if root.join(".tcad-recovery/pending.json").exists() {
        return Err(invalid(
            "This model has an unfinished local save. Open and save its original local copy before publishing or selecting it",
        ));
    }
    let bytes = read_bounded(&manifest_path, MAX_JSON)?;
    let mut size = bytes.len() as u64;
    let mut manifest: Manifest = serde_json::from_slice(&bytes)?;
    if manifest.schema_version != 1 {
        return Err(invalid("Unsupported TCAD project schema"));
    }
    if manifest.parts.len() + manifest.assemblies.len() > MAX_SCAN_ENTRIES {
        return Err(invalid("Model has too many definition files"));
    }
    let mut files = BTreeSet::from([name.to_string()]);
    for (id, relative) in manifest.parts {
        check_cancel(cancel)?;
        let bytes = read_bounded(&checked_path(&root, &relative)?, MAX_JSON)?;
        size += bytes.len() as u64;
        if size > MAX_MODEL {
            return Err(invalid("Model exceeds the 1 GiB publishing limit"));
        }
        manifest
            .document
            .parts
            .insert(id, serde_json::from_slice(&bytes)?);
        files.insert(relative);
    }
    for (id, relative) in manifest.assemblies {
        check_cancel(cancel)?;
        let bytes = read_bounded(&checked_path(&root, &relative)?, MAX_JSON)?;
        size += bytes.len() as u64;
        if size > MAX_MODEL {
            return Err(invalid("Model exceeds the 1 GiB publishing limit"));
        }
        manifest
            .document
            .assemblies
            .insert(id, serde_json::from_slice(&bytes)?);
        files.insert(relative);
    }
    if manifest.document.resources.len() > MAX_SCAN_ENTRIES {
        return Err(invalid("Model has too many resource files"));
    }
    for resource in manifest.document.resources.values() {
        check_cancel(cancel)?;
        let path = checked_path(&root, &resource.path)?;
        let metadata = fs::metadata(&path)?;
        if metadata.len() > MAX_RESOURCE {
            return Err(invalid(format!(
                "{} is larger than GitHub's 100 MiB file limit",
                resource.path
            )));
        }
        size += metadata.len();
        if size > MAX_MODEL {
            return Err(invalid("Model exceeds the 1 GiB publishing limit"));
        }
        let hash = hash_file(&path, MAX_RESOURCE, cancel)?;
        if hash != resource.sha256 {
            return Err(invalid(format!(
                "Resource bytes do not match their saved hash: {}",
                resource.path
            )));
        }
        files.insert(resource.path.clone());
    }
    crate::library::hydrate_pins(&mut manifest.document, &root);
    manifest.document.validate()?;
    for link in manifest.document.libraries.keys() {
        check_cancel(cancel)?;
        crate::library::verified_source(&manifest.document, &root, link)?;
    }
    Ok(Snapshot {
        root,
        document: manifest.document,
        files,
    })
}

fn hash_file(path: &Path, limit: u64, cancel: &CancelToken) -> Result<String> {
    let mut file = File::open(path)?;
    let mut hash = Sha256::new();
    let mut size = 0;
    let mut buffer = [0; 64 * 1024];
    loop {
        check_cancel(cancel)?;
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        size += count as u64;
        if size > limit {
            return Err(invalid(
                "Resource grew beyond the supported file size while reading",
            ));
        }
        hash.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hash.finalize()))
}

/// Find valid native manifests, including named JSON manifests in nested folders.
/// Invalid projects remain visible as notices rather than disappearing silently.
pub fn discover(root: &Path, cancel: &CancelToken) -> Result<Discovery> {
    let root = canonical_directory(root)?;
    let mut discovery = Discovery::default();
    let mut queue = vec![(root.clone(), 0)];
    let mut entries = 0;
    let mut scanned_bytes = 0;
    while let Some((directory, depth)) = queue.pop() {
        check_cancel(cancel)?;
        let mut children = fs::read_dir(&directory)?.collect::<std::io::Result<Vec<_>>>()?;
        children.sort_by_key(|entry| entry.file_name());
        for entry in children {
            check_cancel(cancel)?;
            entries += 1;
            if entries > MAX_SCAN_ENTRIES {
                discovery.notices.push("Stopped after 25,000 repository entries. Open a model manifest directly for deeper repositories.".into());
                return Ok(discovery);
            }
            let name = entry.file_name();
            let text = name.to_string_lossy();
            if text.starts_with('.') || matches!(text.as_ref(), "node_modules" | "target" | "dist")
            {
                continue;
            }
            let kind = entry.file_type()?;
            if kind.is_symlink() {
                continue;
            }
            let path = entry.path();
            if kind.is_dir() {
                if depth < MAX_DEPTH {
                    queue.push((path, depth + 1));
                } else if discovery.notices.len() < 64 {
                    discovery.notices.push(format!(
                        "Skipped folders deeper than {MAX_DEPTH}: {}",
                        path.display()
                    ));
                }
                continue;
            }
            if !kind.is_file()
                || !path
                    .extension()
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("json"))
            {
                continue;
            }
            let length = entry.metadata()?.len();
            if length > MAX_JSON {
                continue;
            }
            scanned_bytes += length;
            if scanned_bytes > MAX_SCAN_JSON {
                discovery.notices.push("Stopped after reading 256 MiB of JSON. Open a model manifest directly for larger repositories.".into());
                return Ok(discovery);
            }
            let bytes = read_bounded(&path, MAX_JSON)?;
            let Ok(value) = serde_json::from_slice::<Value>(&bytes) else {
                continue;
            };
            if value.get("document").is_none()
                || value.get("parts").is_none()
                || value.get("assemblies").is_none()
            {
                continue;
            }
            let relative = path
                .strip_prefix(&root)
                .map_err(|_| invalid("Model escaped the repository"))?
                .to_string_lossy()
                .replace('\\', "/");
            match snapshot(&path, cancel) {
                Ok(snapshot) => discovery.projects.push(ModelEntry {
                    manifest: path,
                    relative_path: relative,
                    name: snapshot.document.name.clone(),
                    document_id: snapshot.document.id.clone(),
                    parts: snapshot.document.parts.len(),
                    assemblies: snapshot.document.assemblies.len(),
                    fingerprint: snapshot.document.fingerprint(),
                }),
                Err(error) => {
                    if discovery.notices.len() < 64 {
                        discovery.notices.push(format!("{relative}: {error}"));
                    }
                }
            }
            if discovery.projects.len() >= MAX_MODELS {
                discovery.notices.push(
                    "Showing the first 256 models. Open another manifest directly if needed."
                        .into(),
                );
                return Ok(discovery);
            }
        }
    }
    discovery
        .projects
        .sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
    Ok(discovery)
}

pub fn open_model(entry: &ModelEntry) -> Result<Document> {
    let snapshot = snapshot(&entry.manifest, &CancelToken::default())?;
    if snapshot.document.id != entry.document_id
        || snapshot.document.fingerprint() != entry.fingerprint
    {
        return Err(invalid(
            "The selected model changed. Refresh the model list before opening it",
        ));
    }
    Ok(snapshot.document)
}

/// Validate a saved model and every pinned resource without replaying recovery
/// journals or changing the source. Suitable for a publish review.
pub fn inspect_model(manifest: &Path, cancel: &CancelToken) -> Result<ModelEntry> {
    let source = snapshot(manifest, cancel)?;
    Ok(model_entry(manifest, &source))
}

fn model_entry(manifest: &Path, source: &Snapshot) -> ModelEntry {
    let name = if manifest
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("json"))
    {
        manifest
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned()
    } else {
        "project.json".into()
    };
    ModelEntry {
        manifest: source.root.join(&name),
        relative_path: name,
        name: source.document.name.clone(),
        document_id: source.document.id.clone(),
        parts: source.document.parts.len(),
        assemblies: source.document.assemblies.len(),
        fingerprint: source.document.fingerprint(),
    }
}

fn preflight_source(request: &PublishSourceRequest, cancel: &CancelToken) -> Result<Snapshot> {
    check_cancel(cancel)?;
    author_valid(&request.author)?;
    if request.commit_message.trim().is_empty()
        || request.commit_message.len() > 65_536
        || request.commit_message.contains('\0')
    {
        return Err(invalid("Enter a commit message"));
    }
    if let Some(branch) = &request.branch {
        branch_name(branch)?;
    }
    if !request.folder.is_empty() {
        relative_path(&request.folder)?;
    }
    destination_path(&request.destination)?;
    let mut source = snapshot(&request.source_manifest, cancel)?;
    if source.document.fingerprint() != request.expected_fingerprint {
        return Err(invalid(
            "The saved model changed. Save and review it before publishing",
        ));
    }
    add_presentation_files(&mut source)?;
    let preview = readme_artifacts(&source, cancel)?;
    let mut bytes = 0;
    for path in source.files.union(&preview.files) {
        check_cancel(cancel)?;
        let metadata = fs::metadata(checked_path(&source.root, path)?)?;
        if metadata.len() > MAX_RESOURCE {
            return Err(invalid("Published files must be at most 100 MiB"));
        }
        bytes += metadata.len();
        if bytes > MAX_MODEL {
            return Err(invalid(
                "Model and presentation files exceed the 1 GiB publishing limit",
            ));
        }
    }
    Ok(source)
}

/// Read-only preparation before a user approves remote repository creation.
/// No repository or destination directory is created by this operation.
pub fn preflight_publish(
    request: &PublishSourceRequest,
    cancel: &CancelToken,
) -> Result<ModelEntry> {
    let source = preflight_source(request, cancel)?;
    Ok(model_entry(&request.source_manifest, &source))
}

fn github_name(url: &str) -> Result<String> {
    let name = url
        .strip_prefix("https://github.com/")
        .ok_or_else(|| invalid("Use the HTTPS clone address of a GitHub repository"))?
        .strip_suffix(".git")
        .unwrap_or_else(|| url.trim_start_matches("https://github.com/"));
    let pieces = name.split('/').collect::<Vec<_>>();
    if pieces.len() != 2
        || pieces.iter().any(|piece| {
            piece.is_empty()
                || piece.len() > 100
                || matches!(*piece, "." | "..")
                || !piece
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
        })
        || pieces[0].starts_with('.')
    {
        return Err(invalid(
            "Use a GitHub owner/repository HTTPS clone address without credentials",
        ));
    }
    Ok(name.into())
}

fn branch_name(branch: &str) -> Result<()> {
    if branch.is_empty()
        || branch.len() > 1024
        || branch.starts_with('-')
        || branch.ends_with('.')
        || branch.contains("..")
        || branch.contains("@{")
        || branch
            .split('/')
            .any(|part| part.is_empty() || part.starts_with('.') || part.ends_with(".lock"))
        || branch
            .chars()
            .any(|c| c.is_control() || c.is_whitespace() || "~^:?*[\\".contains(c))
    {
        return Err(invalid("Choose a valid branch name"));
    }
    Ok(())
}

fn commit_hash(hash: &str) -> Result<()> {
    if !matches!(hash.len(), 40 | 64) || !hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(invalid("Expected an exact Git commit hash"));
    }
    Ok(())
}

fn destination_path(destination: &Path) -> Result<PathBuf> {
    let name = destination
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| invalid("Choose a new repository folder"))?;
    if relative_path(name)?.components().count() != 1 {
        return Err(invalid("Choose a new repository folder"));
    }
    let parent = canonical_directory(destination.parent().unwrap_or(Path::new(".")))?;
    let destination = parent.join(name);
    match fs::symlink_metadata(&destination) {
        Ok(_) => {
            return Err(invalid(
                "The local repository folder already exists. Choose a new folder name",
            ));
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    Ok(destination)
}

fn reserve_destination(destination: &Path) -> Result<PathBuf> {
    let destination = destination_path(destination)?;
    // create_dir is atomic and must fail even for an existing empty directory.
    fs::create_dir(&destination).map_err(|error| {
        invalid(format!(
            "Cannot create {}: {error}. Choose a folder name that does not already exist",
            destination.display()
        ))
    })?;
    Ok(destination)
}

fn secure_git(forge: &GitHub, root: Option<&Path>) -> Result<Command> {
    let mut command = forge.git_command()?;
    command.env("GIT_CONFIG_NOSYSTEM", "1").env(
        "GIT_CONFIG_GLOBAL",
        if cfg!(windows) { "NUL" } else { "/dev/null" },
    );
    for name in [
        "GIT_CONFIG",
        "GIT_CONFIG_COUNT",
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_INDEX_FILE",
        "GIT_OBJECT_DIRECTORY",
        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
        "GIT_TEMPLATE_DIR",
        "GIT_SSH",
        "GIT_SSH_COMMAND",
        "GIT_ASKPASS",
        "SSH_ASKPASS",
        "GIT_CONFIG_PARAMETERS",
    ] {
        command.env_remove(name);
    }
    command
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GCM_INTERACTIVE", "never")
        .env("GIT_LFS_SKIP_SMUDGE", "1");
    let hooks = root
        .map(|root| root.join(".git/tcad-empty-hooks"))
        .unwrap_or_else(|| PathBuf::from(if cfg!(windows) { "NUL" } else { "/dev/null" }));
    command
        .arg("-c")
        .arg(format!("core.hooksPath={}", hooks.display()));
    command.args([
        "-c",
        "init.templateDir=",
        "-c",
        "core.fsmonitor=false",
        "-c",
        "core.symlinks=false",
        "-c",
        "credential.interactive=false",
        "-c",
        "protocol.allow=never",
        "-c",
        "protocol.https.allow=always",
        "-c",
        "http.followRedirects=false",
        "-c",
        "submodule.recurse=false",
        "-c",
        "fetch.recurseSubmodules=false",
        "-c",
        "commit.gpgSign=false",
        "-c",
        "tag.gpgSign=false",
    ]);
    if let Some(root) = root {
        command.arg("-C").arg(root);
    }
    Ok(command)
}

struct ProcessOutput {
    stdout: String,
    stderr: String,
    success: bool,
}

fn redact(text: &str) -> String {
    crate::forge::sanitize_diagnostic(text)
}

fn process(
    mut command: Command,
    cancel: &CancelToken,
    timeout: Duration,
    progress: &mut impl FnMut(&str),
) -> Result<ProcessOutput> {
    check_cancel(cancel)?;
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = command
        .spawn()
        .map_err(|error| invalid(format!("Could not start Git: {error}")))?;
    let (sender, receiver) = mpsc::channel();
    fn reader(
        mut stream: impl Read + Send + 'static,
        sender: mpsc::Sender<(bool, Vec<u8>)>,
        stdout: bool,
    ) {
        thread::spawn(move || {
            let mut bytes = Vec::new();
            let mut buffer = [0; 8192];
            while let Ok(count) = stream.read(&mut buffer) {
                if count == 0 {
                    break;
                }
                if bytes.len() < MAX_OUTPUT {
                    bytes.extend_from_slice(&buffer[..count.min(MAX_OUTPUT - bytes.len())]);
                }
            }
            let _ = sender.send((stdout, bytes));
        });
    }
    reader(
        child
            .stdout
            .take()
            .ok_or_else(|| invalid("Git output pipe is unavailable"))?,
        sender.clone(),
        true,
    );
    reader(
        child
            .stderr
            .take()
            .ok_or_else(|| invalid("Git error pipe is unavailable"))?,
        sender,
        false,
    );
    let started = Instant::now();
    let mut announced = Instant::now();
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let mut received = 0;
    let mut status = None;
    loop {
        if cancel.is_cancelled() || started.elapsed() > timeout {
            #[cfg(unix)]
            {
                let _ = Command::new("/bin/kill")
                    .args(["-TERM", &format!("-{}", child.id())])
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status();
            }
            #[cfg(windows)]
            {
                let _ = Command::new("taskkill")
                    .args(["/PID", &child.id().to_string(), "/T", "/F"])
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status();
            }
            let _ = child.kill();
            let _ = child.wait();
            return Err(invalid(if cancel.is_cancelled() {
                "Repository operation cancelled"
            } else {
                "Git timed out; the local repository was retained for recovery"
            }));
        }
        while let Ok((is_stdout, bytes)) = receiver.try_recv() {
            if is_stdout {
                stdout = bytes;
            } else {
                stderr = bytes;
            }
            received += 1;
        }
        if status.is_none() {
            status = child.try_wait()?;
        }
        if let Some(status) = status
            && received == 2
        {
            return Ok(ProcessOutput {
                stdout: String::from_utf8_lossy(&stdout).into(),
                stderr: String::from_utf8_lossy(&stderr).into(),
                success: status.success(),
            });
        }
        if announced.elapsed() >= Duration::from_secs(3) {
            progress("Git is working. You can cancel this operation.");
            announced = Instant::now();
        }
        thread::sleep(Duration::from_millis(40));
    }
}

fn git(
    forge: &GitHub,
    root: Option<&Path>,
    args: &[&str],
    cancel: &CancelToken,
    progress: &mut impl FnMut(&str),
) -> Result<String> {
    let mut command = secure_git(forge, root)?;
    command.args(args);
    let output = process(command, cancel, Duration::from_secs(300), progress)?;
    if output.success {
        Ok(output.stdout)
    } else {
        Err(invalid(format!(
            "Git could not complete the operation: {}",
            redact(&output.stderr)
        )))
    }
}

struct RemoteTip {
    branch: String,
    commit: Option<String>,
}

fn remote_tip(
    forge: &GitHub,
    url: &str,
    requested: Option<&str>,
    cancel: &CancelToken,
    progress: &mut impl FnMut(&str),
) -> Result<RemoteTip> {
    github_name(url)?;
    if let Some(branch) = requested {
        branch_name(branch)?;
    }
    let output = git(
        forge,
        None,
        &["ls-remote", "--symref", "--", url, "HEAD", "refs/heads/*"],
        cancel,
        progress,
    )?;
    let mut heads = BTreeMap::new();
    let mut default = None;
    for line in output.lines() {
        if let Some(branch) = line
            .strip_prefix("ref: refs/heads/")
            .and_then(|text| text.strip_suffix("\tHEAD"))
        {
            default = Some(branch.to_owned());
            continue;
        }
        if let Some((hash, branch)) = line.split_once('\t')
            && let Some(branch) = branch.strip_prefix("refs/heads/")
        {
            commit_hash(hash)?;
            branch_name(branch)?;
            heads.insert(branch.to_owned(), hash.to_owned());
        }
    }
    let branch = requested
        .map(str::to_owned)
        .or(default)
        .or_else(|| heads.keys().next().cloned())
        .unwrap_or_else(|| "main".into());
    if !heads.is_empty() && !heads.contains_key(&branch) {
        return Err(invalid(
            "The selected remote branch does not exist. Refresh the repository and choose an existing branch",
        ));
    }
    Ok(RemoteTip {
        commit: heads.remove(&branch),
        branch,
    })
}

fn checkout(
    forge: &GitHub,
    destination: &Path,
    url: &str,
    tip: &RemoteTip,
    cancel: &CancelToken,
    progress: &mut impl FnMut(&str),
) -> Result<()> {
    progress("Creating a new local repository");
    git(
        forge,
        Some(destination),
        &["init", "--initial-branch", &tip.branch],
        cancel,
        progress,
    )?;
    fs::create_dir(destination.join(".git/tcad-empty-hooks"))?;
    git(
        forge,
        Some(destination),
        &["remote", "add", "origin", url],
        cancel,
        progress,
    )?;
    if let Some(commit) = &tip.commit {
        progress("Downloading the selected branch");
        let reference = format!("refs/heads/{}", tip.branch);
        git(
            forge,
            Some(destination),
            &[
                "fetch",
                "--no-tags",
                "--no-recurse-submodules",
                "--progress",
                "origin",
                &reference,
            ],
            cancel,
            progress,
        )?;
        let actual = git(
            forge,
            Some(destination),
            &["rev-parse", "FETCH_HEAD^{commit}"],
            cancel,
            progress,
        )?;
        if actual.trim() != commit {
            return Err(invalid(
                "The remote branch changed during download. Refresh and try again with a new destination folder",
            ));
        }
        git(
            forge,
            Some(destination),
            &["update-ref", &reference, commit],
            cancel,
            progress,
        )?;
        git(
            forge,
            Some(destination),
            &["read-tree", "--reset", "-u", "HEAD"],
            cancel,
            progress,
        )?;
        git(
            forge,
            Some(destination),
            &[
                "update-ref",
                &format!("refs/remotes/origin/{}", tip.branch),
                commit,
            ],
            cancel,
            progress,
        )?;
        git(
            forge,
            Some(destination),
            &["config", &format!("branch.{}.remote", tip.branch), "origin"],
            cancel,
            progress,
        )?;
        git(
            forge,
            Some(destination),
            &[
                "config",
                &format!("branch.{}.merge", tip.branch),
                &reference,
            ],
            cancel,
            progress,
        )?;
    }
    // Persist the security settings for later normal TCAD Git operations. Hooks
    // and submodules received from a remote are never activated by opening it.
    git(
        forge,
        Some(destination),
        &[
            "config",
            "core.hooksPath",
            &destination.join(".git/tcad-empty-hooks").to_string_lossy(),
        ],
        cancel,
        progress,
    )?;
    for (key, value) in [
        ("core.fsmonitor", "false"),
        ("core.symlinks", "false"),
        ("submodule.recurse", "false"),
        ("fetch.recurseSubmodules", "false"),
    ] {
        git(
            forge,
            Some(destination),
            &["config", key, value],
            cancel,
            progress,
        )?;
    }
    fs::create_dir_all(destination.join(".git/info"))?;
    fs::write(
        destination.join(".git/info/exclude"),
        b".tcad-cache/\n.tcad-recovery/\n",
    )?;
    Ok(())
}

pub fn clone_repository(
    request: &CloneRequest,
    forge: &GitHub,
    cancel: &CancelToken,
    mut progress: impl FnMut(&str),
) -> Result<CloneResult> {
    let tip = remote_tip(
        forge,
        &request.clone_url,
        request.branch.as_deref(),
        cancel,
        &mut progress,
    )?;
    if let Some(expected) = &request.expected_commit {
        commit_hash(expected)?;
        if tip.commit.as_ref() != Some(expected) {
            return Err(invalid(
                "The remote branch changed. Refresh before downloading",
            ));
        }
    }
    check_cancel(cancel)?;
    let root = reserve_destination(&request.destination)?;
    let result = (|| {
        checkout(
            forge,
            &root,
            &request.clone_url,
            &tip,
            cancel,
            &mut progress,
        )?;
        if let Ok(account) = forge.account(cancel)
            && let Some(login) = account.login
        {
            let email = account
                .user_id
                .map(|id| format!("{id}+{login}@users.noreply.github.com"))
                .unwrap_or_else(|| format!("{login}@users.noreply.github.com"));
            configure_author(
                forge,
                &root,
                &GitAuthor { name: login, email },
                cancel,
                &mut progress,
            )?;
        }
        progress("Finding TCAD models and verifying their resources");
        let discovery = discover(&root, cancel)?;
        Ok(CloneResult {
            root: root.clone(),
            branch: tip.branch,
            commit: tip.commit.unwrap_or_default(),
            projects: discovery.projects,
            notices: discovery.notices,
        })
    })();
    result.map_err(|error: CadError| {
        invalid(format!(
            "{error}. Download folder retained at {}. The current model was not changed",
            root.display()
        ))
    })
}

fn add_presentation_files(snapshot: &mut Snapshot) -> Result<()> {
    if snapshot.root.join("ui/workspace.json").is_file() {
        snapshot.files.insert("ui/workspace.json".into());
    }
    let package = "ui/presentation.json";
    if snapshot.root.join(package).is_file() {
        let mut queue = vec![package.to_string()];
        let mut seen = BTreeSet::new();
        while let Some(name) = queue.pop() {
            if !seen.insert(name.clone()) {
                continue;
            }
            if seen.len() > 64 {
                return Err(invalid("Presentation inheritance exceeds 64 files"));
            }
            let path = checked_path(&snapshot.root, &name)?;
            let package: Value = serde_json::from_slice(&read_bounded(&path, 4 * 1024 * 1024)?)?;
            snapshot.files.insert(name.clone());
            let parent = Path::new(&name)
                .parent()
                .ok_or_else(|| invalid("Invalid presentation package path"))?;
            for field in ["layout", "theme"] {
                if let Some(value) = package.get(field).and_then(Value::as_str) {
                    let relative = parent
                        .join(relative_path(value)?)
                        .to_string_lossy()
                        .replace('\\', "/");
                    if !relative.starts_with("ui/") {
                        return Err(invalid(
                            "Share presentation files inside the model's ui folder before publishing",
                        ));
                    }
                    checked_path(&snapshot.root, &relative)?;
                    snapshot.files.insert(relative);
                }
            }
            if let Some(extends) = package.get("extends").and_then(Value::as_array) {
                for value in extends {
                    let value = value
                        .as_str()
                        .ok_or_else(|| invalid("Invalid inherited presentation path"))?;
                    let relative = parent
                        .join(relative_path(value)?)
                        .to_string_lossy()
                        .replace('\\', "/");
                    if !relative.starts_with("ui/") {
                        return Err(invalid(
                            "Shared presentation packages must remain inside the ui folder",
                        ));
                    }
                    queue.push(relative);
                }
            }
        }
    } else {
        for name in ["ui/layout.json", "ui/theme.css", "ui/theme.json"] {
            if snapshot.root.join(name).is_file() {
                snapshot.files.insert(name.into());
            }
        }
    }
    Ok(())
}

fn copy_new_file(source: &Path, target: &Path, cancel: &CancelToken) -> Result<()> {
    let mut input = File::open(source)?;
    if input.metadata()?.len() > MAX_RESOURCE {
        return Err(invalid("Published files must be at most 100 MiB"));
    }
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(target)?;
    let mut size = 0;
    let mut buffer = [0; 64 * 1024];
    loop {
        check_cancel(cancel)?;
        let count = input.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        size += count as u64;
        if size > MAX_RESOURCE {
            return Err(invalid("Published file grew beyond 100 MiB"));
        }
        output.write_all(&buffer[..count])?;
    }
    output.sync_all()?;
    Ok(())
}

struct ReadmeArtifacts {
    markdown: String,
    files: BTreeSet<String>,
    notices: Vec<String>,
}

fn readme_artifacts(source: &Snapshot, cancel: &CancelToken) -> Result<ReadmeArtifacts> {
    let mut result = ReadmeArtifacts {
        markdown: format!(
            "# {}\n\nTCAD model. Open the JSON project manifest in TCAD to edit it.\n",
            source.document.name.replace(['\r', '\n'], " ")
        ),
        files: BTreeSet::new(),
        notices: Vec::new(),
    };
    let metadata_path = "previews/model-preview.json";
    if !source.root.join(metadata_path).is_file() {
        return Ok(result);
    }
    let metadata: crate::readme_preview::Metadata = serde_json::from_slice(&read_bounded(
        &checked_path(&source.root, metadata_path)?,
        1024 * 1024,
    )?)?;
    if metadata.version != crate::readme_preview::CONTRACT_VERSION {
        return Err(invalid(
            "Unsupported README preview metadata. Update the preview before publishing",
        ));
    }
    if metadata.source_sha256 != source.document.fingerprint() {
        result.notices.push("The README preview was out of date and was not published. Update it from Workspace before publishing another copy.".into());
        return Ok(result);
    }
    let stl = checked_path(&source.root, "previews/model.stl")?;
    if metadata.stl_sha256 != hash_file(&stl, MAX_RESOURCE, cancel)? {
        return Err(invalid(
            "README preview STL no longer matches its saved hash. Update the preview before publishing",
        ));
    }
    // Build this section from verified preview assets. Never copy unrelated
    // prose, absolute links or host paths from the source repository's README.
    result.markdown.push_str(&format!("\n{}\n\n### Model preview\n\nGenerated from saved model `{}`. Coordinates are in millimeters.\n\n", crate::readme_preview::START, &metadata.source_sha256[..16]));
    if metadata.inline {
        let ascii = String::from_utf8(read_bounded(&stl, 180_000)?)
            .map_err(|_| invalid("The inline STL preview is not UTF-8"))?;
        if ascii.contains("```") {
            return Err(invalid(
                "The inline STL preview contains invalid markup. Regenerate it before publishing",
            ));
        }
        result
            .markdown
            .push_str("Drag to rotate. Scroll to zoom on GitHub.\n\n```stl\n");
        result.markdown.push_str(&ascii);
        result.markdown.push_str("```\n\n");
    } else {
        result.markdown.push_str("[![Model preview](previews/model.png)](previews/model.stl)\n\nOpen the preview to rotate and zoom in GitHub's STL viewer.\n\n");
    }
    result.markdown.push_str("[Download model preview](previews/model.stl) · [Preview source record](previews/model-preview.json)\n\nThis is a generated display mesh. Use the native TCAD files for editing and manufacturing.\n\n");
    result.markdown.push_str(crate::readme_preview::END);
    result.markdown.push('\n');
    for name in [metadata_path, "previews/model.stl", "previews/model.png"] {
        let path = checked_path(&source.root, name)?;
        if fs::metadata(path)?.len() > MAX_RESOURCE {
            return Err(invalid(
                "README preview files exceed the 100 MiB file limit",
            ));
        }
        result.files.insert(name.into());
    }
    Ok(result)
}

fn preview_readme(
    source: &Snapshot,
    destination: &Path,
    cancel: &CancelToken,
    notices: &mut Vec<String>,
) -> Result<BTreeSet<String>> {
    let generated = readme_artifacts(source, cancel)?;
    for name in &generated.files {
        if !source.files.contains(name) {
            copy_new_file(
                &checked_path(&source.root, name)?,
                &destination.join(name),
                cancel,
            )?;
        }
    }
    let mut files = generated.files;
    let verified = readme_artifacts(
        &Snapshot {
            root: destination.to_path_buf(),
            document: source.document.clone(),
            files: BTreeSet::new(),
        },
        cancel,
    )?;
    if !files.is_empty() && verified.files != files {
        return Err(invalid(
            "The README preview changed while copying. Update it and review publishing again",
        ));
    }
    let path = destination.join("README.md");
    if !path.exists() {
        fs::write(path, verified.markdown)?;
        files.insert("README.md".into());
    }
    notices.extend(generated.notices);
    Ok(files)
}

fn author_valid(author: &GitAuthor) -> Result<()> {
    if author.name.trim().is_empty()
        || author.name.len() > 256
        || author.email.len() > 320
        || !author.email.contains('@')
        || author.email.chars().any(char::is_whitespace)
        || [&author.name, &author.email].iter().any(|value| {
            value
                .chars()
                .any(|c| c.is_control() || matches!(c, '<' | '>'))
        })
    {
        return Err(invalid("Enter a commit author name and email address"));
    }
    Ok(())
}

fn configure_author(
    forge: &GitHub,
    root: &Path,
    author: &GitAuthor,
    cancel: &CancelToken,
    progress: &mut impl FnMut(&str),
) -> Result<()> {
    author_valid(author)?;
    for (key, value) in [("user.name", &author.name), ("user.email", &author.email)] {
        git(
            forge,
            Some(root),
            &["config", "--local", key, value],
            cancel,
            progress,
        )?;
    }
    Ok(())
}

fn verify_remote(request: &PublishRequest, forge: &GitHub, cancel: &CancelToken) -> Result<()> {
    if request.expected_login.is_empty()
        || forge.account(cancel)?.login.as_deref() != Some(request.expected_login.as_str())
    {
        return Err(invalid(
            "The signed-in account changed. Review publishing again before uploading the saved local copy",
        ));
    }
    let repository = forge.repository(&github_name(&request.clone_url)?, cancel)?;
    if repository.id != request.expected_repository_id
        || repository.private != request.expected_private
    {
        return Err(invalid(
            "The repository identity or visibility changed. Review the publish destination again",
        ));
    }
    if repository.archived || !repository.can_push {
        return Err(invalid(
            "This repository is archived or the signed-in account cannot publish to it",
        ));
    }
    Ok(())
}

fn verify_staged_bytes(
    forge: &GitHub,
    root: &Path,
    paths: &BTreeSet<String>,
    cancel: &CancelToken,
    progress: &mut impl FnMut(&str),
) -> Result<()> {
    for batch in paths.iter().collect::<Vec<_>>().chunks(128) {
        let mut args = vec!["--literal-pathspecs", "ls-files", "--stage", "-z", "--"];
        args.extend(batch.iter().map(|path| path.as_str()));
        let staged = git(forge, Some(root), &args, cancel, progress)?;
        let mut objects = BTreeMap::new();
        for entry in staged.split('\0').filter(|entry| !entry.is_empty()) {
            let (metadata, path) = entry
                .split_once('\t')
                .ok_or_else(|| invalid("Git returned an invalid staged path"))?;
            let fields = metadata.split_whitespace().collect::<Vec<_>>();
            if fields.len() != 3 || fields[0] != "100644" || fields[2] != "0" {
                return Err(invalid(
                    "Published model files must be regular files without merge conflicts",
                ));
            }
            objects.insert(path.to_string(), fields[1].to_string());
        }
        let mut args = vec!["hash-object", "--no-filters", "--"];
        args.extend(batch.iter().map(|path| path.as_str()));
        let raw = git(forge, Some(root), &args, cancel, progress)?;
        let hashes = raw.lines().collect::<Vec<_>>();
        if hashes.len() != batch.len() {
            return Err(invalid("Git could not verify every staged model file"));
        }
        for (path, hash) in batch.iter().zip(hashes) {
            if objects.get(path.as_str()).map(String::as_str) != Some(hash) {
                return Err(invalid(format!(
                    "Git attributes changed the saved bytes of {path}. No upload was made. Adjust the repository's text/filter attributes before publishing this model"
                )));
            }
        }
    }
    Ok(())
}

/// Publish one saved model, never its containing source repository. Existing
/// repositories receive a new model folder and keep every existing file/history.
pub fn publish_model(
    request: &PublishRequest,
    forge: &GitHub,
    cancel: &CancelToken,
    mut progress: impl FnMut(&str),
) -> Result<PublishResult> {
    let folder = if request.folder.is_empty() {
        PathBuf::new()
    } else {
        relative_path(&request.folder)?
    };
    progress("Verifying the saved model and its resource files");
    let source = preflight_source(&PublishSourceRequest::from(request), cancel)?;
    verify_remote(request, forge, cancel)?;
    let tip = remote_tip(
        forge,
        &request.clone_url,
        request.branch.as_deref(),
        cancel,
        &mut progress,
    )?;
    if let Some(expected) = &request.expected_remote_commit {
        commit_hash(expected)?;
        if tip.commit.as_ref() != Some(expected) {
            return Err(invalid(
                "The remote branch changed. Refresh before publishing",
            ));
        }
    }
    if tip.commit.is_some() && folder.as_os_str().is_empty() {
        return Err(invalid(
            "Choose a new model folder when publishing into an existing repository",
        ));
    }
    check_cancel(cancel)?;
    let root = reserve_destination(&request.destination)?;
    let result = (|| {
        checkout(
            forge,
            &root,
            &request.clone_url,
            &tip,
            cancel,
            &mut progress,
        )?;
        let destination = root.join(&folder);
        if !folder.as_os_str().is_empty() {
            let mut current = root.clone();
            let components = folder.components().collect::<Vec<_>>();
            for (index, component) in components.iter().enumerate() {
                current.push(component);
                match fs::symlink_metadata(&current) {
                    Ok(metadata)
                        if index + 1 < components.len()
                            && metadata.is_dir()
                            && !metadata.file_type().is_symlink() => {}
                    Ok(_) => {
                        return Err(invalid(
                            "The chosen model folder already exists or an enclosing path is not a real folder. Choose a new model folder so no remote files are overwritten",
                        ));
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                        fs::create_dir(&current)?
                    }
                    Err(error) => return Err(error.into()),
                }
            }
        }
        progress("Copying only this model, its resources and shared presentation");
        for name in &source.files {
            let source_path = checked_path(&source.root, name)?;
            copy_new_file(
                &source_path,
                &destination.join(relative_path(name)?),
                cancel,
            )?;
        }
        let manifest_name = if request
            .source_manifest
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("json"))
        {
            request
                .source_manifest
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or_else(|| invalid("Invalid source manifest name"))?
        } else {
            "project.json"
        };
        let manifest = destination.join(manifest_name);
        let copied = snapshot(&manifest, cancel)?;
        if copied.document.fingerprint() != request.expected_fingerprint {
            return Err(invalid(
                "Source definitions changed while copying. The incomplete copy was retained; review the saved source before publishing",
            ));
        }
        let mut notices = Vec::new();
        let mut published_files = source.files.clone();
        published_files.extend(preview_readme(&source, &destination, cancel, &mut notices)?);
        let published_paths = published_files
            .iter()
            .map(|path| folder.join(path).to_string_lossy().replace('\\', "/"))
            .collect::<BTreeSet<_>>();
        progress("Creating a version of the copied model");
        for paths in published_paths.iter().collect::<Vec<_>>().chunks(128) {
            let mut args = vec!["--literal-pathspecs", "add", "--force", "--"];
            args.extend(paths.iter().map(|path| path.as_str()));
            git(forge, Some(&root), &args, cancel, &mut progress)?;
        }
        let staged = git(
            forge,
            Some(&root),
            &["diff", "--cached", "--name-only", "-z"],
            cancel,
            &mut progress,
        )?;
        let staged = staged
            .split('\0')
            .filter(|name| !name.is_empty())
            .map(str::to_string)
            .collect::<BTreeSet<_>>();
        if staged != published_paths {
            return Err(invalid(
                "The publish checkout has unexpected staged files. No commit or upload was made; inspect the retained copy",
            ));
        }
        verify_staged_bytes(forge, &root, &published_paths, cancel, &mut progress)?;
        configure_author(forge, &root, &request.author, cancel, &mut progress)?;
        let mut command = secure_git(forge, Some(&root))?;
        command
            .env_remove("GIT_AUTHOR_DATE")
            .env_remove("GIT_COMMITTER_DATE")
            .env("GIT_AUTHOR_NAME", &request.author.name)
            .env("GIT_AUTHOR_EMAIL", &request.author.email)
            .env("GIT_COMMITTER_NAME", &request.author.name)
            .env("GIT_COMMITTER_EMAIL", &request.author.email)
            .args(["commit", "-m", &request.commit_message]);
        let output = process(command, cancel, Duration::from_secs(60), &mut progress)?;
        if !output.success {
            return Err(invalid(format!(
                "Could not create the local version: {}",
                redact(&output.stderr)
            )));
        }
        let commit = git(
            forge,
            Some(&root),
            &["rev-parse", "HEAD"],
            cancel,
            &mut progress,
        )?
        .trim()
        .to_string();
        verify_remote(request, forge, cancel)?;
        if snapshot(&request.source_manifest, cancel)?
            .document
            .fingerprint()
            != request.expected_fingerprint
        {
            return Err(invalid(
                "The source model changed before upload. The copied version is kept locally; review before pushing it",
            ));
        }
        progress("Publishing the model to GitHub");
        let reference = format!("HEAD:refs/heads/{}", tip.branch);
        git(
            forge,
            Some(&root),
            &[
                "push",
                "--porcelain",
                "--set-upstream",
                "origin",
                &reference,
            ],
            cancel,
            &mut progress,
        )?;
        Ok(PublishResult {
            root: root.clone(),
            manifest,
            branch: tip.branch,
            commit,
            notices,
        })
    })();
    result.map_err(|error: CadError| invalid(format!("{error}. Recovery copy retained at {}. Your original model is unchanged. If upload was interrupted, refresh GitHub before retrying because the remote may already contain the commit", root.display())))
}
