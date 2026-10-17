//! Exact project publication through an isolated Git checkout. The caller's
//! working tree and index are never staged, reset, or switched by this adapter.
use super::*;
use crate::publication::ArtifactFile;

const OWNERSHIP_FILE: &str = ".tcad-publication-files.json";
const RECOVERY_FILE: &str = "tcad-publication-recovery.json";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Request {
    pub source_manifest: PathBuf,
    pub expected_fingerprint: String,
    pub artifact_root: PathBuf,
    pub artifacts: Vec<ArtifactFile>,
    pub clone_url: String,
    pub branch: String,
    pub folder: String,
    /// A new, absent directory for the recoverable publication checkout.
    pub destination: PathBuf,
    pub commit_message: String,
    pub author: GitAuthor,
    /// The last successful publication to this exact destination, if known.
    pub base_commit: Option<String>,
    /// The original model folder, before output generation copied its snapshot.
    /// A matching containing Git repository contributes its committed history.
    pub source_repository: Option<PathBuf>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Publication {
    pub version: u32,
    pub pushed: bool,
    pub root: PathBuf,
    pub manifest: PathBuf,
    pub clone_url: String,
    pub folder: String,
    pub branch: String,
    pub commit: String,
    pub source_fingerprint: String,
    pub expected_remote_commit: Option<String>,
    pub notices: Vec<String>,
    pub push_error: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Ownership {
    version: u32,
    document_id: Id,
    source_fingerprint: String,
    files: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Recovery {
    version: u32,
    publication: Publication,
    files: BTreeMap<String, String>,
}

struct SourceFile {
    path: PathBuf,
    hash: String,
}

fn readme_block(markdown: &str) -> Result<&str> {
    let starts = markdown
        .match_indices(crate::readme_preview::START)
        .collect::<Vec<_>>();
    let ends = markdown
        .match_indices(crate::readme_preview::END)
        .collect::<Vec<_>>();
    match (starts.as_slice(), ends.as_slice()) {
        ([(start, _)], [(end, _)]) if end > start => {
            Ok(&markdown[*start..end + crate::readme_preview::END.len()])
        }
        _ => Err(invalid(
            "Generated README preview markers are incomplete or repeated",
        )),
    }
}

fn portable(value: &str) -> Result<PathBuf> {
    let path = relative_path(value)?;
    let normalized = path
        .components()
        .map(|part| part.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/");
    if normalized != value {
        return Err(invalid("Use a normalized relative publication path"));
    }
    Ok(path)
}

fn project_path(folder: &str, path: &str) -> String {
    if folder.is_empty() {
        path.into()
    } else {
        format!("{folder}/{path}")
    }
}

fn ensure_parents(root: &Path, relative: &Path) -> Result<()> {
    let mut current = root.to_path_buf();
    if let Some(parent) = relative.parent() {
        for component in parent.components() {
            current.push(component);
            match fs::symlink_metadata(&current) {
                Ok(meta) if meta.is_dir() && !meta.file_type().is_symlink() => {}
                Ok(_) => return Err(invalid("A publication path is not a real directory")),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    fs::create_dir(&current)?;
                }
                Err(error) => return Err(error.into()),
            }
        }
    }
    Ok(())
}

fn regular_existing(root: &Path, relative: &str) -> Result<Option<PathBuf>> {
    let relative_path = portable(relative)?;
    let mut current = root.to_path_buf();
    let count = relative_path.components().count();
    for (index, component) in relative_path.components().enumerate() {
        current.push(component);
        match fs::symlink_metadata(&current) {
            Ok(meta) if meta.file_type().is_symlink() => {
                return Err(invalid("Publication cannot replace a symlink"));
            }
            Ok(meta) if index + 1 == count && meta.is_file() => return Ok(Some(current)),
            Ok(meta) if index + 1 < count && meta.is_dir() => {}
            Ok(_) => {
                return Err(invalid(
                    "A publication file conflicts with a remote directory",
                ));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        }
    }
    Ok(None)
}

fn load_ownership(root: &Path, folder: &str) -> Result<Option<Ownership>> {
    let Some(path) = regular_existing(root, &project_path(folder, OWNERSHIP_FILE))? else {
        return Ok(None);
    };
    let ownership: Ownership = serde_json::from_slice(&read_bounded(&path, MAX_JSON)?)?;
    if ownership.version != 1 || ownership.files.len() > MAX_SCAN_ENTRIES {
        return Err(invalid("Unsupported project publication file record"));
    }
    for (path, hash) in &ownership.files {
        portable(path)?;
        if path == OWNERSHIP_FILE || !sha256_valid(hash) {
            return Err(invalid("Invalid project publication file record"));
        }
    }
    Ok(Some(ownership))
}

fn sha256_valid(hash: &str) -> bool {
    hash.len() == 64 && hash.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn generated_path(path: &str) -> bool {
    matches!(
        path,
        "outputs/drawing.pdf"
            | "outputs/drawing.dxf"
            | "outputs/bom.csv"
            | "outputs/model.step"
            | "outputs/model.stl"
            | "outputs/sketches.dxf"
            | "outputs/flat-pattern.dxf"
            | "outputs/cutlist.csv"
            | "publication.json"
            | "tcad-publish.json"
            | "README.md"
            | "previews/model-preview.json"
            | "previews/model.stl"
            | "previews/model.png"
    )
}

fn ancestor(
    transport: &GitTransport<'_>,
    root: &Path,
    older: &str,
    newer: &str,
    cancel: &CancelToken,
    progress: &mut impl FnMut(&str),
) -> Result<bool> {
    let mut command = secure_git(transport, Some(root))?;
    command.args(["merge-base", "--is-ancestor", older, newer]);
    let output = process(command, cancel, Duration::from_secs(60), progress)?;
    if !output.success && !output.stderr.trim().is_empty() {
        return Err(invalid(format!(
            "Cannot verify publication history: {}",
            redact(&output.stderr)
        )));
    }
    Ok(output.success)
}

fn current_target(
    transport: &GitTransport<'_>,
    url: &str,
    branch: &str,
    cancel: &CancelToken,
    progress: &mut impl FnMut(&str),
) -> Result<Option<String>> {
    Ok(inspect_remote_with(transport, url, cancel, progress)?
        .branches
        .into_iter()
        .find(|head| head.name == branch)
        .map(|head| head.commit))
}

fn local_source_head(
    request: &Request,
    transport: &GitTransport<'_>,
    root: &Path,
    cancel: &CancelToken,
    progress: &mut impl FnMut(&str),
) -> Result<Option<String>> {
    let Some(source) = &request.source_repository else {
        return Ok(None);
    };
    let source = canonical_directory(source)?;
    let mut command = secure_git(transport, Some(&source))?;
    command.args(["rev-parse", "--show-toplevel"]);
    let output = process(command, cancel, Duration::from_secs(30), progress)?;
    if !output.success {
        return Ok(None);
    }
    let repository = canonical_directory(Path::new(output.stdout.trim()))?;
    let mut command = secure_git(transport, Some(&repository))?;
    command.args(["remote", "get-url", "origin"]);
    let output = process(command, cancel, Duration::from_secs(30), progress)?;
    if !output.success {
        return Ok(None);
    }
    let remote = RepositoryAddress::parse(output.stdout.trim())?;
    if Some(&remote.git_url) != transport.address.map(|address| &address.git_url) {
        return Ok(None);
    }
    if !git(
        transport,
        Some(&repository),
        &["ls-files", "--unmerged", "-z"],
        cancel,
        progress,
    )?
    .is_empty()
    {
        return Err(invalid(
            "Finish resolving the model repository's merge before publishing",
        ));
    }
    for state in [
        "MERGE_HEAD",
        "rebase-merge",
        "rebase-apply",
        "CHERRY_PICK_HEAD",
    ] {
        let path = git(
            transport,
            Some(&repository),
            &["rev-parse", "--git-path", state],
            cancel,
            progress,
        )?;
        if repository.join(path.trim()).exists() {
            return Err(invalid(
                "Finish the model repository's current merge or rebase before publishing",
            ));
        }
    }
    let mut command = secure_git(transport, Some(&repository))?;
    command.args(["rev-parse", "--verify", "HEAD^{commit}"]);
    let output = process(command, cancel, Duration::from_secs(30), progress)?;
    if !output.success {
        return Ok(None);
    }
    let head = output.stdout.trim().to_string();
    commit_hash(&head)?;
    progress("Preserving the model repository's committed history");
    git(
        transport,
        Some(root),
        &[
            "fetch",
            "--no-tags",
            "--no-recurse-submodules",
            "--",
            &repository.to_string_lossy(),
            &head,
        ],
        cancel,
        progress,
    )?;
    Ok(Some(head))
}

fn reject_remote_project_changes(
    transport: &GitTransport<'_>,
    root: &Path,
    base: &str,
    remote: &str,
    paths: &BTreeSet<String>,
    cancel: &CancelToken,
    progress: &mut impl FnMut(&str),
) -> Result<()> {
    if !ancestor(transport, root, base, remote, cancel, progress)? {
        return Err(invalid(
            "The publication branch has diverged from the saved baseline. Merge the branch before publishing",
        ));
    }
    for batch in paths.iter().collect::<Vec<_>>().chunks(128) {
        let mut args = vec![
            "--literal-pathspecs",
            "diff",
            "--name-only",
            "-z",
            base,
            remote,
            "--",
        ];
        args.extend(batch.iter().map(|path| path.as_str()));
        if !git(transport, Some(root), &args, cancel, progress)?.is_empty() {
            return Err(invalid(
                "The repository has newer changes to this project. Open or merge that version before publishing; no remote model was overwritten",
            ));
        }
    }
    Ok(())
}

/// Merge committed histories without invoking text merge drivers, hooks or any
/// repository-provided executable. CAD conflicts are replaced later by the
/// already-validated snapshot. Other conflicts remain an explicit user task.
fn merge_histories(
    transport: &GitTransport<'_>,
    root: &Path,
    remote: &str,
    local: &str,
    project_paths: &BTreeSet<String>,
    cancel: &CancelToken,
    progress: &mut impl FnMut(&str),
) -> Result<BTreeSet<String>> {
    progress("Combining local and published history without changing the working model");
    let base = git(
        transport,
        Some(root),
        &["merge-base", "--all", remote, local],
        cancel,
        progress,
    )?;
    let base = base.trim();
    if base.lines().count() != 1 {
        return Err(invalid(
            "The local and remote branches have multiple merge bases. Merge their history explicitly before publishing",
        ));
    }
    commit_hash(base)?;
    git(
        transport,
        Some(root),
        &["read-tree", "-m", base, remote, local],
        cancel,
        progress,
    )?;
    let unmerged = git(
        transport,
        Some(root),
        &["ls-files", "--unmerged", "-z"],
        cancel,
        progress,
    )?;
    let mut conflicts = BTreeSet::new();
    for entry in unmerged.split('\0').filter(|entry| !entry.is_empty()) {
        let (_, path) = entry
            .split_once('\t')
            .ok_or_else(|| invalid("Git returned an invalid merge conflict path"))?;
        portable(path)?;
        if !project_paths.contains(path) {
            return Err(invalid(format!(
                "Local and remote history both changed {path}. Merge that file before publishing; no model or remote branch was changed"
            )));
        }
        conflicts.insert(path.to_string());
    }
    for batch in conflicts.iter().collect::<Vec<_>>().chunks(128) {
        let mut args = vec![
            "--literal-pathspecs",
            "update-index",
            "--force-remove",
            "--",
        ];
        args.extend(batch.iter().map(|path| path.as_str()));
        git(transport, Some(root), &args, cancel, progress)?;
    }
    let merged_tree = git(transport, Some(root), &["write-tree"], cancel, progress)?;
    let merged_tree = merged_tree.trim();
    commit_hash(merged_tree)?;
    // Restore the remote index before updating its worktree to the merged tree;
    // this makes Git remove deleted paths and write every changed file.
    git(
        transport,
        Some(root),
        &["read-tree", "--reset", remote],
        cancel,
        progress,
    )?;
    git(
        transport,
        Some(root),
        &["read-tree", "--reset", "-u", merged_tree],
        cancel,
        progress,
    )?;
    fs::write(root.join(".git/MERGE_HEAD"), format!("{local}\n"))?;
    let changed = git(
        transport,
        Some(root),
        &["diff", "--cached", "--name-only", "-z"],
        cancel,
        progress,
    )?;
    Ok(changed
        .split('\0')
        .filter(|path| !path.is_empty())
        .map(str::to_string)
        .collect())
}

fn recovery_path(root: &Path) -> PathBuf {
    root.join(".git").join(RECOVERY_FILE)
}

fn save_recovery(record: &Recovery) -> Result<()> {
    let path = recovery_path(&record.publication.root);
    let temporary = path.with_extension("json.tmp");
    let mut file = File::create(&temporary)?;
    file.write_all(&serde_json::to_vec_pretty(record)?)?;
    file.sync_all()?;
    fs::rename(temporary, path)?;
    Ok(())
}

fn save_recovery_notice(record: &mut Recovery) {
    if let Err(error) = save_recovery(record) {
        record.publication.notices.push(format!(
            "Could not save the local retry record: {error}. Commit {} remains in {}.",
            record.publication.commit,
            record.publication.root.display()
        ));
    }
}

fn push_record(
    record: &mut Recovery,
    connection: Option<&ConnectionProfile>,
    cancel: &CancelToken,
    progress: &mut impl FnMut(&str),
) -> Result<()> {
    let publication = &record.publication;
    let address = RepositoryAddress::parse(&publication.clone_url)?;
    let transport = GitTransport {
        address: Some(&address),
        connection,
    };
    verify_transport_address(&transport, Some(&publication.root), true, cancel, progress)?;
    let remote = current_target(
        &transport,
        &address.git_url,
        &publication.branch,
        cancel,
        progress,
    )?;
    if remote.as_deref() == Some(&publication.commit) {
        record.publication.pushed = true;
        record.publication.push_error = None;
        save_recovery_notice(record);
        return Ok(());
    }
    if remote != publication.expected_remote_commit {
        return Err(invalid(
            "The remote branch changed before upload. The complete local publication is retained. Merge or refresh that branch before publishing again",
        ));
    }
    progress("Pushing the model, drawings, BOM and exports together");
    let reference = format!("{}:refs/heads/{}", publication.commit, publication.branch);
    git(
        &transport,
        Some(&publication.root),
        &["push", "--porcelain", "origin", &reference],
        cancel,
        progress,
    )?;
    record.publication.pushed = true;
    record.publication.push_error = None;
    save_recovery_notice(record);
    Ok(())
}

/// Generate no geometry here: publish one already-generated, hash-bound package.
/// A failed upload returns the complete committed publication with `pushed=false`
/// so callers can offer Retry Push without regenerating or committing it again.
pub fn publish(
    request: &Request,
    connection: Option<&ConnectionProfile>,
    cancel: &CancelToken,
    mut progress: impl FnMut(&str),
) -> Result<Publication> {
    branch_name(&request.branch)?;
    if !request.folder.is_empty() {
        portable(&request.folder)?;
    }
    let address = RepositoryAddress::parse(&request.clone_url)?;
    let transport = GitTransport {
        address: Some(&address),
        connection,
    };
    let source_request = PublishSourceRequest {
        source_manifest: request.source_manifest.clone(),
        expected_fingerprint: request.expected_fingerprint.clone(),
        destination: request.destination.clone(),
        branch: Some(request.branch.clone()),
        folder: request.folder.clone(),
        commit_message: request.commit_message.clone(),
        author: request.author.clone(),
    };
    progress("Verifying the saved project and generated output snapshot");
    let mut source = preflight_source(&source_request, cancel)?;
    if source.root.join("tcad-publish.json").exists() {
        source.files.insert("tcad-publish.json".into());
    }
    let manifest_name = model_entry(&request.source_manifest, &source).relative_path;
    let artifact_root = canonical_directory(&request.artifact_root)?;
    if request.artifacts.len() > MAX_SCAN_ENTRIES {
        return Err(invalid("Too many publication files"));
    }
    let mut files = BTreeMap::<String, SourceFile>::new();
    let mut total_bytes = 0;
    for path in &source.files {
        portable(path)?;
        let file = checked_path(&source.root, path)?;
        total_bytes += fs::metadata(&file)?.len();
        files.insert(
            path.clone(),
            SourceFile {
                hash: hash_file(&file, MAX_RESOURCE, cancel)?,
                path: file,
            },
        );
    }
    for artifact in &request.artifacts {
        portable(&artifact.path)?;
        if !sha256_valid(&artifact.sha256)
            || artifact.path == OWNERSHIP_FILE
            || files.contains_key(&artifact.path)
        {
            return Err(invalid(
                "Generated outputs overlap project files or have an invalid hash",
            ));
        }
        let file = checked_path(&artifact_root, &artifact.path)?;
        let hash = hash_file(&file, MAX_RESOURCE, cancel)?;
        if hash != artifact.sha256 {
            return Err(invalid(
                "Generated output changed after generation; generate the publication again",
            ));
        }
        total_bytes += fs::metadata(&file)?.len();
        files.insert(artifact.path.clone(), SourceFile { path: file, hash });
    }
    if total_bytes > MAX_MODEL {
        return Err(invalid("Publication exceeds the 1 GiB supported size"));
    }
    if files.contains_key(OWNERSHIP_FILE) {
        return Err(invalid(
            "The source uses the reserved publication record path",
        ));
    }
    let package_manifest_file = files
        .get(crate::publication::MANIFEST_FILE)
        .ok_or_else(|| invalid("Publication has no generated output manifest"))?;
    let mut package_manifest: crate::publication::Manifest =
        serde_json::from_slice(&read_bounded(&package_manifest_file.path, MAX_JSON)?)?;
    let declared = package_manifest
        .files
        .iter()
        .map(|file| (file.path.clone(), file.sha256.clone()))
        .collect::<BTreeMap<_, _>>();
    let provided = request
        .artifacts
        .iter()
        .filter(|file| file.path != crate::publication::MANIFEST_FILE)
        .map(|file| (file.path.clone(), file.sha256.clone()))
        .collect::<BTreeMap<_, _>>();
    if package_manifest.version != crate::publication::VERSION
        || package_manifest.document_id != source.document.id
        || package_manifest.source_sha256 != request.expected_fingerprint
        || declared != provided
        || declared.len() != package_manifest.files.len()
    {
        return Err(invalid(
            "Generated outputs and their manifest do not describe this exact source snapshot",
        ));
    }
    let generated_readme = files
        .get("README.md")
        .map(|file| {
            String::from_utf8(read_bounded(&file.path, MAX_RESOURCE)?)
                .map_err(|_| invalid("Generated README must be UTF-8"))
        })
        .transpose()?;
    if let Some(readme) = &generated_readme {
        readme_block(readme)?;
    }
    let mut case_paths = BTreeSet::new();
    for path in files.keys() {
        if !case_paths.insert(path.to_lowercase()) {
            return Err(invalid(
                "Publication filenames collide on case-insensitive filesystems",
            ));
        }
    }
    let remote = inspect_remote_with(&transport, &address.git_url, cancel, &mut progress)?;
    let target_commit = remote
        .branches
        .iter()
        .find(|head| head.name == request.branch)
        .map(|head| head.commit.clone());
    let checkout_head = target_commit
        .as_ref()
        .map(|commit| RemoteTip {
            branch: request.branch.clone(),
            commit: Some(commit.clone()),
        })
        .or_else(|| {
            remote
                .default_branch
                .as_ref()
                .and_then(|name| remote.branches.iter().find(|head| &head.name == name))
                .or(remote.branches.first())
                .map(|head| RemoteTip {
                    branch: head.name.clone(),
                    commit: Some(head.commit.clone()),
                })
        })
        .unwrap_or_else(|| RemoteTip {
            branch: request.branch.clone(),
            commit: None,
        });
    if let Some(base) = &request.base_commit {
        commit_hash(base)?;
    }
    let root = reserve_destination(&request.destination)?;
    let result = (|| {
        checkout(
            &transport,
            &root,
            Some(&address.git_url),
            &checkout_head,
            cancel,
            &mut progress,
        )?;
        let local_head = local_source_head(request, &transport, &root, cancel, &mut progress)?;
        let mut parent_commit = checkout_head.commit.clone();
        let mut pending_merge = None;
        if let Some(local) = &local_head {
            if target_commit.is_none() {
                // A new destination branch is a fork of the source branch, not
                // a request to merge every later change on the default branch.
                parent_commit = Some(local.clone());
            } else if let Some(remote) = &parent_commit {
                if ancestor(&transport, &root, remote, local, cancel, &mut progress)? {
                    parent_commit = Some(local.clone());
                } else if !ancestor(&transport, &root, local, remote, cancel, &mut progress)? {
                    if request.base_commit.is_some() {
                        pending_merge = Some((remote.clone(), local.clone()));
                    } else {
                        return Err(invalid(
                            "The local model history and publication branch have diverged without a prior publication baseline. Merge them before publishing",
                        ));
                    }
                }
            } else {
                parent_commit = Some(local.clone());
            }
        }
        let reference = format!("refs/heads/{}", request.branch);
        git(
            &transport,
            Some(&root),
            &["symbolic-ref", "HEAD", &reference],
            cancel,
            &mut progress,
        )?;
        // onbranch configuration can become active after selecting the target.
        // Disable its filters before materializing any branch contents.
        disable_transfer_filters(&transport, &root, cancel, &mut progress)?;
        if let Some(parent) = &parent_commit {
            git(
                &transport,
                Some(&root),
                &["update-ref", &reference, parent],
                cancel,
                &mut progress,
            )?;
            git(
                &transport,
                Some(&root),
                &["read-tree", "--reset", "-u", "HEAD"],
                cancel,
                &mut progress,
            )?;
        }
        let ownership = load_ownership(&root, &request.folder)?;
        if ownership
            .as_ref()
            .is_some_and(|owned| owned.document_id != source.document.id)
        {
            return Err(invalid(
                "The destination folder belongs to another TCAD project",
            ));
        }
        let remote_manifest_path = project_path(&request.folder, &manifest_name);
        let mut existing = regular_existing(&root, &remote_manifest_path)?
            .map(|manifest| snapshot(&manifest, cancel))
            .transpose()?;
        if existing
            .as_ref()
            .is_some_and(|existing| existing.document.id != source.document.id)
        {
            return Err(invalid(
                "The destination folder contains a different TCAD project. Choose a different project folder",
            ));
        }
        if let Some(existing) = &mut existing {
            add_presentation_files(existing)?;
            if existing.root.join("tcad-publish.json").exists() {
                existing.files.insert("tcad-publish.json".into());
            }
        }
        // A downloaded ownership file alone cannot grant permission to remove
        // unrelated files. Only shared Git history makes earlier ownership a
        // deletion/update baseline; first adoption claims current model files.
        let trusted_ownership = request.base_commit.is_some() || local_head.is_some();
        let mut owned = ownership
            .as_ref()
            .filter(|_| trusted_ownership)
            .map(|owned| owned.files.keys().cloned().collect::<BTreeSet<_>>())
            .unwrap_or_default();
        // Repository metadata cannot claim an arbitrary filename for deletion.
        // Source ownership comes from the actual CAD manifest and referenced
        // presentation; generated ownership uses this version's fixed outputs.
        owned.retain(|path| {
            generated_path(path)
                || existing
                    .as_ref()
                    .is_some_and(|existing| existing.files.contains(path))
        });
        if let Some(existing) = &existing {
            owned.extend(existing.files.iter().cloned());
        }
        // An old publication from before this ownership contract can be adopted,
        // but arbitrary remote files are never claimed simply by sharing a folder.
        let project_paths = owned
            .iter()
            .chain(files.keys())
            .map(|path| project_path(&request.folder, path))
            .chain([project_path(&request.folder, OWNERSHIP_FILE)])
            .collect::<BTreeSet<_>>();
        let mut conflict_paths = project_paths.clone();
        if generated_readme.is_some() {
            conflict_paths.remove(&project_path(&request.folder, "README.md"));
        }
        if let Some(remote) = &target_commit {
            if let Some(base) = request.base_commit.as_ref().or(local_head.as_ref()) {
                if !local_head
                    .as_ref()
                    .is_some_and(|local| parent_commit.as_ref() == Some(local))
                {
                    reject_remote_project_changes(
                        &transport,
                        &root,
                        base,
                        remote,
                        &conflict_paths,
                        cancel,
                        &mut progress,
                    )?;
                } else if let Some(base) = &request.base_commit {
                    // A last successful publication may be newer than the source
                    // working tree, but it must remain in the retained history.
                    if !ancestor(
                        &transport,
                        &root,
                        base,
                        parent_commit.as_deref().unwrap_or(remote),
                        cancel,
                        &mut progress,
                    )? {
                        return Err(invalid(
                            "The last published version is not in this branch's history. Merge it before publishing",
                        ));
                    }
                }
            } else if existing.as_ref().is_some_and(|existing| {
                existing.document.fingerprint() != request.expected_fingerprint
            }) {
                return Err(invalid(
                    "This remote project differs from the local model and has no shared publication baseline. Open the repository version or choose a new project folder",
                ));
            }
        }
        let merged_changes = if let Some((remote, local)) = &pending_merge {
            let mut merge_project_paths = project_paths.clone();
            merge_project_paths.remove(&project_path(&request.folder, "README.md"));
            merge_histories(
                &transport,
                &root,
                remote,
                local,
                &merge_project_paths,
                cancel,
                &mut progress,
            )?
        } else {
            BTreeSet::new()
        };
        if let Some(ownership) = ownership.as_ref().filter(|_| trusted_ownership) {
            for (path, expected_hash) in &ownership.files {
                if !owned.contains(path) {
                    continue;
                }
                if path == "README.md" && generated_readme.is_some() {
                    continue;
                }
                if let Some(file) = regular_existing(&root, &project_path(&request.folder, path))?
                    && hash_file(&file, MAX_RESOURCE, cancel)? != *expected_hash
                    && !existing
                        .as_ref()
                        .is_some_and(|existing| existing.files.contains(path))
                {
                    return Err(invalid(format!(
                        "Published output {path} was changed in Git. Preserve or resolve that change before publishing again"
                    )));
                }
            }
        }
        let preview_metadata = project_path(&request.folder, "previews/model-preview.json");
        if let Some(metadata) = regular_existing(&root, &preview_metadata)? {
            let metadata: crate::readme_preview::Metadata =
                serde_json::from_slice(&read_bounded(&metadata, MAX_JSON)?)?;
            if metadata.version != crate::readme_preview::CONTRACT_VERSION {
                return Err(invalid(
                    "The repository's README preview format is unsupported",
                ));
            }
            if generated_readme.is_none() {
                return Err(invalid(
                    "This project already has a README preview. Open the repository project with its preview files before publishing so the preview can be refreshed with the model",
                ));
            }
            let stl =
                regular_existing(&root, &project_path(&request.folder, "previews/model.stl"))?
                    .ok_or_else(|| {
                        invalid("The repository's existing README preview STL is missing")
                    })?;
            if hash_file(&stl, MAX_RESOURCE, cancel)? != metadata.stl_sha256 {
                return Err(invalid(
                    "The repository's README preview was edited outside TCAD. Preserve or regenerate that preview before publishing",
                ));
            }
            if existing.is_some() && trusted_ownership {
                owned.extend(
                    [
                        "previews/model-preview.json",
                        "previews/model.stl",
                        "previews/model.png",
                    ]
                    .map(str::to_string),
                );
            }
        }
        let mut replacements = BTreeMap::<String, Vec<u8>>::new();
        if generated_readme.is_none()
            && let Some(readme) =
                regular_existing(&root, &project_path(&request.folder, "README.md"))?
        {
            let text = String::from_utf8(read_bounded(&readme, MAX_RESOURCE)?)
                .map_err(|_| invalid("The repository README is not UTF-8"))?;
            if text.contains(crate::readme_preview::START)
                || text.contains(crate::readme_preview::END)
            {
                return Err(invalid(
                    "The repository README contains a model preview. Generate a local README preview before publishing so this view can be updated with the model",
                ));
            }
        }
        if let Some(generated_readme) = &generated_readme
            && let Some(remote_readme) =
                regular_existing(&root, &project_path(&request.folder, "README.md"))?
        {
            let remote_readme = String::from_utf8(read_bounded(&remote_readme, MAX_RESOURCE)?)
                .map_err(|_| invalid("The repository README is not UTF-8"))?;
            let merged = crate::readme_preview::update_markdown_block(
                &remote_readme,
                readme_block(generated_readme)?,
            )?;
            let hash = format!("{:x}", Sha256::digest(merged.as_bytes()));
            files
                .get_mut("README.md")
                .expect("generated readme exists")
                .hash = hash.clone();
            replacements.insert("README.md".into(), merged.into_bytes());
            if let Some(file) = package_manifest
                .files
                .iter_mut()
                .find(|file| file.path == "README.md")
            {
                file.sha256 = hash;
            }
            let manifest_bytes = serde_json::to_vec_pretty(&package_manifest)?;
            files
                .get_mut(crate::publication::MANIFEST_FILE)
                .expect("manifest verified")
                .hash = format!("{:x}", Sha256::digest(&manifest_bytes));
            replacements.insert(crate::publication::MANIFEST_FILE.into(), manifest_bytes);
            owned.insert("README.md".into());
        }
        progress("Copying the exact model and generated outputs into the publication");
        for (path, file) in &files {
            check_cancel(cancel)?;
            let relative = project_path(&request.folder, path);
            if let Some(existing) = regular_existing(&root, &relative)? {
                if !owned.contains(path) && hash_file(&existing, MAX_RESOURCE, cancel)? != file.hash
                {
                    return Err(invalid(format!(
                        "Publication would overwrite unrelated repository file {relative}. Choose another project folder or preserve that file first"
                    )));
                }
                fs::remove_file(existing)?;
            }
            let relative_path = portable(&relative)?;
            ensure_parents(&root, &relative_path)?;
            let destination = root.join(relative_path);
            if let Some(bytes) = replacements.get(path) {
                if bytes.len() as u64 > MAX_RESOURCE {
                    return Err(invalid(
                        "Merged publication file exceeds the supported file size",
                    ));
                }
                let mut output = OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&destination)?;
                output.write_all(bytes)?;
                output.sync_all()?;
            } else {
                copy_new_file(&file.path, &destination, cancel)?;
            }
            if hash_file(&destination, MAX_RESOURCE, cancel)? != file.hash {
                return Err(invalid(
                    "A source or generated file changed while publishing",
                ));
            }
        }
        let mut removed = BTreeSet::new();
        for path in owned.difference(&files.keys().cloned().collect()) {
            // README prose is user-authored even when its generated preview
            // block was part of a previous publication.
            if path == "README.md" {
                continue;
            }
            let relative = project_path(&request.folder, path);
            if let Some(existing) = regular_existing(&root, &relative)? {
                fs::remove_file(existing)?;
                removed.insert(relative);
            }
        }
        let manifest = root.join(&remote_manifest_path);
        if snapshot(&manifest, cancel)?.document.fingerprint() != request.expected_fingerprint {
            return Err(invalid(
                "The copied model no longer matches the generated outputs",
            ));
        }
        let ownership = Ownership {
            version: 1,
            document_id: source.document.id.clone(),
            source_fingerprint: request.expected_fingerprint.clone(),
            files: files
                .iter()
                .map(|(path, file)| (path.clone(), file.hash.clone()))
                .collect(),
        };
        let ownership_relative = project_path(&request.folder, OWNERSHIP_FILE);
        ensure_parents(&root, &portable(&ownership_relative)?)?;
        fs::write(
            root.join(&ownership_relative),
            serde_json::to_vec_pretty(&ownership)?,
        )?;
        let mut written = ownership
            .files
            .iter()
            .map(|(path, hash)| (project_path(&request.folder, path), hash.clone()))
            .collect::<BTreeMap<_, _>>();
        written.insert(
            ownership_relative.clone(),
            hash_file(&root.join(&ownership_relative), MAX_JSON, cancel)?,
        );
        let allowed = project_paths
            .union(&BTreeSet::from([ownership_relative]))
            .cloned()
            .chain(merged_changes)
            .collect::<BTreeSet<_>>();
        let stage_paths = written
            .keys()
            .cloned()
            .chain(removed)
            .collect::<BTreeSet<_>>();
        for batch in stage_paths.iter().collect::<Vec<_>>().chunks(128) {
            let mut args = vec!["--literal-pathspecs", "add", "--all", "--force", "--"];
            // Deleted and present paths are both explicit. No user index is read.
            args.extend(batch.iter().map(|path| path.as_str()));
            git(&transport, Some(&root), &args, cancel, &mut progress)?;
        }
        let staged = git(
            &transport,
            Some(&root),
            &["diff", "--cached", "--name-only", "-z"],
            cancel,
            &mut progress,
        )?;
        let changed = staged
            .split('\0')
            .filter(|path| !path.is_empty())
            .map(str::to_owned)
            .collect::<BTreeSet<_>>();
        if !changed.is_subset(&allowed) {
            return Err(invalid(
                "Publication staged unexpected repository files; no commit was made",
            ));
        }
        verify_staged_bytes(
            &transport,
            &root,
            &written.keys().cloned().collect(),
            cancel,
            &mut progress,
        )?;
        configure_author(&transport, &root, &request.author, cancel, &mut progress)?;
        git_connections::bind_reviewed_connection(&root, connection)?;
        let mut commit_failure = None;
        if !changed.is_empty() || parent_commit.is_none() || pending_merge.is_some() {
            progress("Committing the model, drawings, BOM and exports together");
            let mut command = secure_git(&transport, Some(&root))?;
            command
                .env_remove("GIT_AUTHOR_DATE")
                .env_remove("GIT_COMMITTER_DATE")
                .env("GIT_AUTHOR_NAME", &request.author.name)
                .env("GIT_AUTHOR_EMAIL", &request.author.email)
                .env("GIT_COMMITTER_NAME", &request.author.name)
                .env("GIT_COMMITTER_EMAIL", &request.author.email)
                .args(["commit", "-m", &request.commit_message]);
            match process(command, cancel, Duration::from_secs(60), &mut progress) {
                Ok(output) if output.success => {}
                Ok(output) => {
                    commit_failure = Some(format!(
                        "Could not commit the publication: {}",
                        redact(&output.stderr)
                    ))
                }
                Err(error) => commit_failure = Some(error.to_string()),
            }
        }
        // Cancellation can arrive just after Git updates HEAD. Capture that
        // committed state without credentials or a cancelled process token so
        // the caller can retry its exact upload rather than lose the receipt.
        let local_transport = GitTransport {
            address: None,
            connection: None,
        };
        let commit = git(
            &local_transport,
            Some(&root),
            &["rev-parse", "HEAD"],
            &CancelToken::default(),
            &mut progress,
        )
        .map_err(|error| invalid(commit_failure.clone().unwrap_or_else(|| error.to_string())))?
        .trim()
        .to_string();
        if let Some(error) = &commit_failure
            && parent_commit.as_deref() == Some(&commit)
        {
            return Err(invalid(error));
        }
        let mut record = Recovery {
            version: 1,
            publication: Publication {
                version: 1, pushed: false, root: root.clone(), manifest,
                clone_url: address.git_url.clone(), folder: request.folder.clone(), branch: request.branch.clone(), commit,
                source_fingerprint: request.expected_fingerprint.clone(), expected_remote_commit: target_commit,
                notices: vec!["Publication uses a separate checkout. The original model folder, branch and staged files remain unchanged.".into()], push_error: None,
            },
            files: written,
        };
        if commit_failure.is_some() {
            record.publication.notices.push("Git completed the commit before cancellation or interruption was reported. Its exact commit is retained for Retry Push.".into());
        }
        if let Err(error) = save_recovery(&record) {
            record.publication.push_error = Some(format!(
                "The publication is committed, but its local retry record could not be saved: {error}. No push was attempted. The complete commit remains at {}.",
                root.display()
            ));
            return Ok(record.publication);
        }
        if let Err(error) = push_record(&mut record, connection, cancel, &mut progress) {
            record.publication.push_error = Some(format!(
                "{error}. The complete commit is saved at {}. Retry Push sends this same commit without generating another version.",
                root.display()
            ));
            save_recovery_notice(&mut record);
        }
        Ok(record.publication)
    })();
    result.map_err(|error: CadError| invalid(format!("{error}. Publication recovery folder retained at {}. The original model and its Git index are unchanged.", root.display())))
}

/// Retry only the saved commit. This checks the retained worktree, index,
/// ownership hashes, target address, and remote head before a normal Git push.
pub fn retry_push(
    previous: &Publication,
    connection: Option<&ConnectionProfile>,
    cancel: &CancelToken,
    mut progress: impl FnMut(&str),
) -> Result<Publication> {
    let root = canonical_directory(&previous.root)?;
    let path = recovery_path(&root);
    let mut record: Recovery = match fs::symlink_metadata(&path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return Err(invalid("Publication recovery record cannot be a symlink"));
        }
        Ok(_) => serde_json::from_slice(&read_bounded(&path, MAX_JSON)?)?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            // The UI's saved receipt still identifies the exact commit if the
            // first local receipt write failed. Reconstruct only its file hashes
            // from the committed ownership file, then verify everything below.
            let ownership = load_ownership(&root, &previous.folder)?
                .ok_or_else(|| invalid("The committed publication ownership record is missing"))?;
            if ownership.source_fingerprint != previous.source_fingerprint {
                return Err(invalid(
                    "The retained source does not match this publication receipt",
                ));
            }
            let ownership_relative = project_path(&previous.folder, OWNERSHIP_FILE);
            let mut files = ownership
                .files
                .into_iter()
                .map(|(path, hash)| (project_path(&previous.folder, &path), hash))
                .collect::<BTreeMap<_, _>>();
            files.insert(
                ownership_relative.clone(),
                hash_file(&checked_path(&root, &ownership_relative)?, MAX_JSON, cancel)?,
            );
            Recovery {
                version: 1,
                publication: previous.clone(),
                files,
            }
        }
        Err(error) => return Err(error.into()),
    };
    let saved = &record.publication;
    if record.version != 1
        || saved.version != 1
        || saved.root != root
        || saved.commit != previous.commit
        || saved.clone_url != previous.clone_url
        || saved.branch != previous.branch
        || saved.folder != previous.folder
        || saved.source_fingerprint != previous.source_fingerprint
        || saved.expected_remote_commit != previous.expected_remote_commit
        || record.files.len() > MAX_SCAN_ENTRIES
    {
        return Err(invalid(
            "The retained publication no longer matches the selected retry",
        ));
    }
    branch_name(&saved.branch)?;
    commit_hash(&saved.commit)?;
    let address = RepositoryAddress::parse(&saved.clone_url)?;
    let transport = GitTransport {
        address: Some(&address),
        connection,
    };
    if git(
        &transport,
        Some(&root),
        &["rev-parse", "HEAD"],
        cancel,
        &mut progress,
    )?
    .trim()
        != saved.commit
        || !git(
            &transport,
            Some(&root),
            &["status", "--porcelain=v1", "-z", "--untracked-files=normal"],
            cancel,
            &mut progress,
        )?
        .is_empty()
    {
        return Err(invalid(
            "The retained publication checkout changed. Preserve or commit those edits before retrying",
        ));
    }
    for (path, hash) in &record.files {
        portable(path)?;
        if !sha256_valid(hash)
            || hash_file(&checked_path(&root, path)?, MAX_RESOURCE, cancel)? != *hash
        {
            return Err(invalid(
                "The retained publication files no longer match their committed snapshot",
            ));
        }
    }
    verify_staged_bytes(
        &transport,
        &root,
        &record.files.keys().cloned().collect(),
        cancel,
        &mut progress,
    )?;
    if let Err(error) = save_recovery(&record) {
        record.publication.pushed = false;
        record.publication.push_error = Some(format!(
            "Could not save the local retry record: {error}. No push was attempted; the complete commit remains at {}.",
            root.display()
        ));
        return Ok(record.publication);
    }
    if let Err(error) = push_record(&mut record, connection, cancel, &mut progress) {
        record.publication.pushed = false;
        record.publication.push_error = Some(format!(
            "{error}. The committed publication remains at {}.",
            root.display()
        ));
        save_recovery_notice(&mut record);
    }
    Ok(record.publication)
}
