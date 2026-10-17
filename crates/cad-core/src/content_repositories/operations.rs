use super::*;
use super::{git::Git, storage::Snapshot};
use crate::{
    git_connections::{self, ConnectionProfile, RepositoryAddress},
    repository_models::{self, CloneRequest},
};
use std::{collections::BTreeMap, fs, io::Write};

fn find(id: &str) -> Result<InstalledRepository> {
    load_registry()?
        .repositories
        .into_iter()
        .find(|repository| repository.id == id)
        .ok_or_else(|| invalid("The content repository is no longer registered"))
}

fn mutate_registry(change: impl FnOnce(&mut Registry) -> Result<()>) -> Result<()> {
    let _guard = REGISTRY_LOCK
        .lock()
        .map_err(|_| invalid("Content library preferences are unavailable"))?;
    let mut registry = load_registry()?;
    change(&mut registry)?;
    storage::save_registry(&registry)?;
    refresh_catalog()?;
    Ok(())
}

fn replace(repository: &InstalledRepository) -> Result<()> {
    mutate_registry(|registry| {
        let slot = registry
            .repositories
            .iter_mut()
            .find(|entry| entry.id == repository.id)
            .ok_or_else(|| invalid("The content repository was removed during the operation"))?;
        if slot.root != repository.root {
            return Err(invalid(
                "The library checkout changed during the operation; refresh before continuing",
            ));
        }
        *slot = repository.clone();
        Ok(())
    })
}

fn installation(root: &Path, enabled: bool, cancel: &CancelToken) -> Result<InstalledRepository> {
    let snapshot = storage::snapshot(root, cancel)?;
    let git = Git::local(&snapshot.root);
    git.validate_root(cancel)?;
    let revision = git.head(cancel)?;
    tree_files(&git, revision.as_deref(), cancel, &mut |_| {})?;
    Ok(InstalledRepository {
        id: uuid::Uuid::new_v4().to_string(),
        root: snapshot.root.clone(),
        manifest: snapshot.manifest,
        revision,
        remote: git.remote(cancel)?,
        branch: git.branch(cancel)?,
        enabled,
    })
}

pub fn register_existing(root: &Path, enabled: bool) -> Result<InstalledRepository> {
    register_existing_with_cancel(root, enabled, &CancelToken::default())
}

pub fn register_existing_with_cancel(
    root: &Path,
    enabled: bool,
    cancel: &CancelToken,
) -> Result<InstalledRepository> {
    let mut repository = installation(root, enabled, cancel)?;
    cancel.check()?;
    mutate_registry(|registry| {
        if let Some(existing) = registry
            .repositories
            .iter_mut()
            .find(|existing| existing.root == repository.root)
        {
            repository.id = existing.id.clone();
            *existing = repository.clone();
        } else {
            registry.repositories.push(repository.clone());
        }
        Ok(())
    })?;
    Ok(repository)
}

pub fn clone_and_register(
    request: &CloneRequest,
    connection: Option<&ConnectionProfile>,
    cancel: &CancelToken,
    mut progress: impl FnMut(&str),
) -> Result<InstalledRepository> {
    let checkout = repository_models::clone_repository(request, connection, cancel, &mut progress)?;
    register_existing_with_cancel(&checkout.root, true, cancel).map_err(|error| {
        invalid(format!(
            "{error}. The checkout was retained at {}",
            checkout.root.display()
        ))
    })
}

/// Disable or remove libraries without deleting their checkouts or model resources.
pub fn set_enabled(id: &str, enabled: bool) -> Result<()> {
    mutate_registry(|registry| {
        registry
            .repositories
            .iter_mut()
            .find(|entry| entry.id == id)
            .ok_or_else(|| invalid("The content repository is no longer registered"))?
            .enabled = enabled;
        Ok(())
    })
}

pub fn remove(id: &str) -> Result<()> {
    mutate_registry(|registry| {
        registry
            .repositories
            .retain(|repository| repository.id != id);
        Ok(())
    })
}

pub fn create_repository(
    destination: &Path,
    name: &str,
    kind: ContentKind,
    author: &GitAuthor,
    cancel: &CancelToken,
    mut progress: impl FnMut(&str),
) -> Result<InstalledRepository> {
    git::validate_author(author)?;
    let mut manifest = LibraryManifest {
        version: CONTRACT_VERSION,
        id: uuid::Uuid::new_v4().to_string(),
        name: name.trim().to_owned(),
        description: format!("User-owned {} library", kind.label().to_ascii_lowercase()),
        entries: Vec::new(),
    };
    storage::validate_manifest(&manifest)?;
    let checkout =
        repository_models::initialize_local_repository(destination, "main", cancel, &mut progress)?;
    let root = &checkout.root;
    let result = (|| {
        if let Some((path, template)) = template(kind) {
            storage::atomic_write(&root.join(path), &serde_json::to_vec_pretty(&template)?)?;
            manifest.entries.push(LibraryEntry {
                id: "my-library".into(),
                name: name.trim().to_owned(),
                kind,
                path: path.into(),
            });
        }
        storage::atomic_write(
            &root.join(MANIFEST_FILE),
            &serde_json::to_vec_pretty(&manifest)?,
        )?;
        let readme = format!(
            "# {}\n\n{}\n\nOpen this folder from TCAD Settings → Content libraries. Edit the data files, then add their paths as entries in `tcad-library.json` or use the library manager.\n\nThis repository contains data only. Use relative paths inside this folder. Commit changes in the TCAD library manager. Publishing requires a separately reviewed Git URL. Keep new hosted repositories private unless you choose otherwise.\n\nEntry kinds: `weldment_profiles`, `textures`, `themes`, `materials`, `standard_parts`, `finishes`. Textures and finishes are separate from engineering materials.\n",
            manifest.name, manifest.description
        );
        storage::atomic_write(&root.join("README.md"), readme.as_bytes())?;
        let snapshot = storage::snapshot(root, cancel)?;
        let commit = commit_snapshot(
            &snapshot,
            "Create content library",
            author,
            ("main", None),
            cancel,
            &mut progress,
        )?;
        let repository = register_existing_with_cancel(root, true, cancel)?;
        if repository.revision.as_deref() != Some(&commit) {
            return Err(invalid(
                "The library changed while its first version was registered",
            ));
        }
        Ok(repository)
    })();
    result.map_err(|error: CadError| {
        invalid(format!(
            "{error}. The new library folder was retained at {}",
            root.display()
        ))
    })
}

fn template(kind: ContentKind) -> Option<(&'static str, serde_json::Value)> {
    use serde_json::json;
    match kind {
        ContentKind::Themes => Some((
            "presentation.json",
            json!({"schema_version":1,"inherit_defaults":true,"name":"My theme"}),
        )),
        // These catalog documents are intentionally empty. Entries are user data.
        ContentKind::WeldmentProfiles => Some((
            "profiles.json",
            json!({"version":1,"id":uuid::Uuid::new_v4().to_string(),"revision":"1","name":"My profiles","collections":[],"profiles":[]}),
        )),
        ContentKind::Materials => Some((
            "materials.json",
            json!({"schema_version":1,"name":"My materials","materials":[]}),
        )),
        ContentKind::StandardParts => Some((
            "standard-parts.json",
            json!({"schema_version":1,"catalog_version":"1","name":"My standard parts","sources":[],"entries":[]}),
        )),
        ContentKind::Finishes => Some((
            "finishes.tcadfinishes",
            json!({
                "format": "tcad.surface-finishes", "version": 1,
                "finishes": [crate::appearance::Finish {
                    id: uuid::Uuid::new_v4().to_string(), name: "My finish".into(),
                    builtin_bump: crate::appearance::BuiltinBump::None,
                    ..Default::default()
                }], "textures": []
            }),
        )),
        ContentKind::Textures => None,
    }
}

pub fn add_entry(
    id: &str,
    entry: LibraryEntry,
    cancel: &CancelToken,
) -> Result<InstalledRepository> {
    let mut repository = find(id)?;
    let (root, mut manifest) = editable_manifest(&repository.root, cancel)?;
    if let Some(existing) = manifest
        .entries
        .iter_mut()
        .find(|existing| existing.id == entry.id)
    {
        *existing = entry;
    } else {
        manifest.entries.push(entry);
    }
    storage::validate_manifest(&manifest)?;
    storage::validate_resources(&root, &manifest, cancel)?;
    cancel.check()?;
    storage::atomic_write(
        &root.join(MANIFEST_FILE),
        &serde_json::to_vec_pretty(&manifest)?,
    )?;
    repository.manifest = manifest;
    replace(&repository)?;
    Ok(repository)
}

pub fn remove_entry(id: &str, entry_id: &str, cancel: &CancelToken) -> Result<InstalledRepository> {
    let mut repository = find(id)?;
    let (root, mut manifest) = editable_manifest(&repository.root, cancel)?;
    manifest.entries.retain(|entry| entry.id != entry_id);
    storage::validate_manifest(&manifest)?;
    cancel.check()?;
    storage::atomic_write(
        &root.join(MANIFEST_FILE),
        &serde_json::to_vec_pretty(&manifest)?,
    )?;
    repository.manifest = manifest;
    replace(&repository)?;
    Ok(repository)
}

fn editable_manifest(root: &Path, cancel: &CancelToken) -> Result<(PathBuf, LibraryManifest)> {
    cancel.check()?;
    let root = storage::canonical_root(root)?;
    let bytes = storage::read_bounded(
        &storage::checked_file(&root, MANIFEST_FILE)?,
        4 * 1024 * 1024,
    )?;
    let manifest: LibraryManifest = serde_json::from_slice(&bytes)?;
    storage::validate_manifest(&manifest)?;
    Ok((root, manifest))
}

pub fn inspect_repository(id: &str, cancel: &CancelToken) -> Result<RepositoryInspection> {
    let mut repository = find(id)?;
    let snapshot = storage::snapshot(&repository.root, cancel)?;
    let git = Git::local(&snapshot.root);
    git.validate_root(cancel)?;
    let head = git.head(cancel)?;
    let branch = git.branch(cancel)?;
    let mut changes = changes(&snapshot, &git, head.as_deref(), cancel, &mut |_| {})?;
    if let Err(error) = check_index(&git, head.as_deref(), cancel, &mut |_| {}) {
        cancel.check()?;
        changes.push(error.to_string());
    }
    let status = changes.join("\n");
    repository.manifest = snapshot.manifest;
    Ok(RepositoryInspection {
        repository,
        fingerprint: snapshot.fingerprint,
        branch,
        head,
        status,
        file_count: snapshot.files.len(),
        files: snapshot.files.into_keys().collect(),
    })
}

fn file_objects(
    snapshot: &Snapshot,
    git: &Git<'_>,
    write: bool,
    cancel: &CancelToken,
    progress: &mut impl FnMut(&str),
) -> Result<BTreeMap<String, String>> {
    let names = snapshot.files.keys().cloned().collect::<Vec<_>>();
    let mut result = BTreeMap::new();
    for chunk in names.chunks(16) {
        let mut args = vec!["hash-object", "--no-filters"];
        if write {
            args.push("-w");
        }
        args.push("--");
        args.extend(chunk.iter().map(String::as_str));
        let output = git.run(&args, cancel, progress)?;
        let hashes = output.lines().collect::<Vec<_>>();
        if hashes.len() != chunk.len() {
            return Err(invalid("Git returned an incomplete library file snapshot"));
        }
        for (name, hash) in chunk.iter().zip(hashes) {
            git::validate_commit(hash)?;
            result.insert(name.clone(), hash.to_owned());
        }
    }
    Ok(result)
}

fn tree_files(
    git: &Git<'_>,
    head: Option<&str>,
    cancel: &CancelToken,
    progress: &mut impl FnMut(&str),
) -> Result<BTreeMap<String, String>> {
    let Some(head) = head else {
        return Ok(BTreeMap::new());
    };
    git::validate_commit(head)?;
    let output = git.run(&["ls-tree", "-r", "-z", head], cancel, progress)?;
    let mut files = BTreeMap::new();
    for record in output.split('\0').filter(|record| !record.is_empty()) {
        let (metadata, path) = record
            .split_once('\t')
            .ok_or_else(|| invalid("Invalid library Git tree"))?;
        let parts = metadata.split_whitespace().collect::<Vec<_>>();
        if parts.len() != 3 || parts[1] != "blob" || !matches!(parts[0], "100644" | "100755") {
            return Err(invalid(
                "Libraries cannot contain symbolic links or submodules",
            ));
        }
        storage::relative_path(path)?;
        git::validate_commit(parts[2])?;
        files.insert(path.to_owned(), parts[2].to_owned());
    }
    Ok(files)
}

fn changes(
    snapshot: &Snapshot,
    git: &Git<'_>,
    head: Option<&str>,
    cancel: &CancelToken,
    progress: &mut impl FnMut(&str),
) -> Result<Vec<String>> {
    let before = tree_files(git, head, cancel, progress)?;
    let after = file_objects(snapshot, git, false, cancel, progress)?;
    let mut changes = Vec::new();
    for (name, hash) in &after {
        match before.get(name) {
            None => changes.push(format!("Added {name}")),
            Some(prior) if prior != hash => changes.push(format!("Changed {name}")),
            _ => {}
        }
    }
    for name in before.keys().filter(|name| !after.contains_key(*name)) {
        changes.push(format!("Removed {name}"));
    }
    Ok(changes)
}

fn commit_snapshot(
    snapshot: &Snapshot,
    message: &str,
    author: &GitAuthor,
    expected: (&str, Option<&str>),
    cancel: &CancelToken,
    progress: &mut impl FnMut(&str),
) -> Result<String> {
    git::validate_author(author)?;
    if message.trim().is_empty() || message.len() > 16_384 || message.contains('\0') {
        return Err(invalid("Enter a commit message"));
    }
    let git = Git::local(&snapshot.root);
    git.validate_root(cancel)?;
    let branch = git.branch(cancel)?;
    let head = git.head(cancel)?;
    if branch != expected.0 || head.as_deref() != expected.1 {
        return Err(invalid(
            "The library branch or committed version changed after review; refresh before committing",
        ));
    }
    let index_lock = snapshot.root.join(".git/index.lock");
    let mut locked_index = Some(fs::OpenOptions::new().write(true).create_new(true).open(&index_lock)
        .map_err(|_| invalid("The library Git index is busy or locked. Finish the other Git operation before committing"))?);
    let index = snapshot
        .root
        .join(".git")
        .join(format!("tcad-library-index-{}", uuid::Uuid::new_v4()));
    let staged = Git {
        root: &snapshot.root,
        index: Some(&index),
        address: None,
        connection: None,
    };
    let mut committed = false;
    let result = (|| {
        check_index(&git, head.as_deref(), cancel, progress)?;
        staged.run(&["read-tree", "--empty"], cancel, progress)?;
        let objects = file_objects(snapshot, &git, true, cancel, progress)?;
        for (name, hash) in &objects {
            let blob = git.blob(hash, cancel, progress)?;
            let expected = snapshot
                .files
                .get(name)
                .ok_or_else(|| invalid("The reviewed library file is missing"))?;
            if blob.len() as u64 != expected.size || storage::digest(&blob) != expected.sha256 {
                return Err(invalid(
                    "A library file changed while Git read it; refresh the review before committing",
                ));
            }
            let record = format!("100644,{hash},{name}");
            staged.run(
                &["update-index", "--add", "--cacheinfo", &record],
                cancel,
                progress,
            )?;
        }
        // Recheck bytes after Git read them. A changing checkout cannot become a
        // different commit than the explicitly reviewed content snapshot.
        if storage::snapshot(&snapshot.root, cancel)?.fingerprint != snapshot.fingerprint {
            return Err(invalid(
                "Library files changed while the commit was being prepared; review again",
            ));
        }
        let tree = staged
            .run(&["write-tree"], cancel, progress)?
            .trim()
            .to_owned();
        let mut args = vec!["commit-tree", &tree, "-m", message];
        if let Some(head) = &head {
            args.extend(["-p", head]);
        }
        let commit = staged
            .run_author(&args, Some(author), cancel, progress)?
            .trim()
            .to_owned();
        git::validate_commit(&commit)?;
        if git.branch(cancel)? != branch || git.head(cancel)? != head {
            return Err(invalid(
                "The library branch changed while the commit was being prepared",
            ));
        }
        let reference = format!("refs/heads/{branch}");
        let zero = "0".repeat(commit.len());
        let prepared_index = storage::read_bounded(&index, 32 * 1024 * 1024)?;
        let mut locked = locked_index
            .take()
            .ok_or_else(|| invalid("The Git index lock was lost"))?;
        locked.write_all(&prepared_index)?;
        locked.sync_all()?;
        drop(locked);
        cancel.check()?;
        // Once the reference moves, finish the paired index replacement even if
        // cancellation arrives. No worktree files are changed.
        git.run(
            &[
                "update-ref",
                "-m",
                "TCAD content library commit",
                &reference,
                &commit,
                head.as_deref().unwrap_or(&zero),
            ],
            &CancelToken::default(),
            progress,
        )?;
        committed = true;
        fs::rename(&index_lock, snapshot.root.join(".git/index")).map_err(|error| invalid(format!("Commit {commit} was saved, but the prepared index could not be installed: {error}. The recovery index is at {}", index_lock.display())))?;
        Ok(commit)
    })();
    drop(locked_index);
    if !committed {
        let _ = fs::remove_file(&index_lock);
    }
    let _ = fs::remove_file(&index);
    let _ = fs::remove_file(index.with_extension("lock"));
    result
}

fn check_index(
    git: &Git<'_>,
    head: Option<&str>,
    cancel: &CancelToken,
    progress: &mut impl FnMut(&str),
) -> Result<()> {
    let indexed = git.run(&["ls-files", "--stage", "-z"], cancel, progress)?;
    let mut index = BTreeMap::new();
    for record in indexed.split('\0').filter(|record| !record.is_empty()) {
        let (metadata, path) = record
            .split_once('\t')
            .ok_or_else(|| invalid("Invalid library Git index"))?;
        let fields = metadata.split_whitespace().collect::<Vec<_>>();
        if fields.len() != 3 || fields[2] != "0" {
            return Err(invalid("Resolve the library's Git merge before committing"));
        }
        index.insert(
            path.to_owned(),
            (fields[0].to_owned(), fields[1].to_owned()),
        );
    }
    let mut committed = BTreeMap::new();
    if let Some(head) = head {
        let tree = git.run(&["ls-tree", "-r", "-z", head], cancel, progress)?;
        for record in tree.split('\0').filter(|record| !record.is_empty()) {
            let (metadata, path) = record
                .split_once('\t')
                .ok_or_else(|| invalid("Invalid library Git tree"))?;
            let fields = metadata.split_whitespace().collect::<Vec<_>>();
            if fields.len() != 3 {
                return Err(invalid("Invalid library Git tree"));
            }
            committed.insert(
                path.to_owned(),
                (fields[0].to_owned(), fields[2].to_owned()),
            );
        }
    }
    if index != committed {
        return Err(invalid(
            "This library has independently staged Git changes. Commit or unstage them in your Git client before committing the complete library in TCAD",
        ));
    }
    Ok(())
}

pub fn commit_changes(
    request: &CommitRequest,
    cancel: &CancelToken,
    mut progress: impl FnMut(&str),
) -> Result<CommitResult> {
    let mut repository = find(&request.repository_id)?;
    let snapshot = storage::snapshot(&repository.root, cancel)?;
    if snapshot.fingerprint != request.expected_fingerprint {
        return Err(invalid(
            "Library files changed after review; refresh before committing",
        ));
    }
    if Git::local(&snapshot.root).head(cancel)? != request.expected_head {
        return Err(invalid(
            "The library's committed version changed after review; refresh before committing",
        ));
    }
    let commit = commit_snapshot(
        &snapshot,
        &request.message,
        &request.author,
        (&request.expected_branch, request.expected_head.as_deref()),
        cancel,
        &mut progress,
    )?;
    repository.manifest = snapshot.manifest;
    repository.revision = Some(commit.clone());
    replace(&repository)?;
    Ok(CommitResult { repository, commit })
}

pub fn fork_repository(
    id: &str,
    destination: &Path,
    _connection: Option<&ConnectionProfile>,
    cancel: &CancelToken,
    mut progress: impl FnMut(&str),
) -> Result<InstalledRepository> {
    let source = inspect_repository(id, cancel)?;
    if !source.status.is_empty() {
        return Err(invalid("Commit the library's changes before forking it"));
    }
    let revision = source
        .head
        .ok_or_else(|| invalid("Commit the library before forking it"))?;
    let root = source.repository.root.to_string_lossy().into_owned();
    let checkout = repository_models::clone_repository(
        &CloneRequest {
            clone_url: root.clone(),
            destination: destination.to_path_buf(),
            branch: Some(source.branch),
            expected_commit: Some(revision),
        },
        None,
        cancel,
        &mut progress,
    )?;
    let git = Git::local(&checkout.root);
    git.run(
        &["remote", "rename", "origin", "upstream"],
        cancel,
        &mut progress,
    )?;
    if let Some(remote) = source.repository.remote {
        git.run(
            &["remote", "set-url", "upstream", &remote],
            cancel,
            &mut progress,
        )?;
    }
    // A fork has no publish origin. Its first upload always needs URL review.
    register_existing_with_cancel(&checkout.root, false, cancel)
}

pub fn inspect_update(
    id: &str,
    destination: &Path,
    connection: Option<&ConnectionProfile>,
    cancel: &CancelToken,
    mut progress: impl FnMut(&str),
) -> Result<UpdateCandidate> {
    let inspection = inspect_repository(id, cancel)?;
    if !inspection.status.is_empty() {
        return Err(invalid(
            "Commit or resolve the library's local and staged changes before reviewing an update",
        ));
    }
    let current_head = inspection
        .head
        .clone()
        .ok_or_else(|| invalid("Commit this library before reviewing a remote update"))?;
    let repository = inspection.repository;
    let before = storage::snapshot(&repository.root, cancel)?;
    if before.fingerprint != inspection.fingerprint
        || Git::local(&repository.root).head(cancel)? != inspection.head
    {
        return Err(invalid(
            "The library changed while its update was being prepared; review it again",
        ));
    }
    let remote = repository.remote.as_ref().ok_or_else(|| {
        invalid("This library has no update source. Publish it or open a remote library first")
    })?;
    let advertised = repository_models::inspect_remote(remote, connection, cancel, &mut progress)?;
    let branch = advertised
        .branches
        .iter()
        .find(|branch| branch.name == repository.branch)
        .ok_or_else(|| invalid("The library's source branch is no longer available"))?;
    let checkout = repository_models::clone_repository(
        &CloneRequest {
            clone_url: remote.clone(),
            destination: destination.to_path_buf(),
            branch: Some(branch.name.clone()),
            expected_commit: Some(branch.commit.clone()),
        },
        connection,
        cancel,
        &mut progress,
    )?;
    let candidate = installation(&checkout.root, repository.enabled, cancel)?;
    prove_update_ancestry(&candidate, &current_head, cancel, &mut progress)?;
    if candidate.manifest.id != repository.manifest.id {
        return Err(invalid(
            "The remote is a different library. Register it separately instead of replacing this library",
        ));
    }
    let after = storage::snapshot(&checkout.root, cancel)?;
    require_clean_snapshot(&after, candidate.revision.as_deref(), cancel, &mut progress)?;
    let mut added = Vec::new();
    let mut removed = Vec::new();
    let mut changed = Vec::new();
    for (name, file) in &after.files {
        match before.files.get(name) {
            None => added.push(name.clone()),
            Some(previous) if previous.sha256 != file.sha256 => changed.push(name.clone()),
            _ => {}
        }
    }
    for name in before
        .files
        .keys()
        .filter(|name| !after.files.contains_key(*name))
    {
        removed.push(name.clone());
    }
    Ok(UpdateCandidate {
        repository_id: repository.id,
        previous_root: repository.root,
        previous_revision: inspection.head,
        previous_branch: inspection.branch,
        previous_fingerprint: before.fingerprint,
        checkout: candidate,
        fingerprint: after.fingerprint,
        added,
        removed,
        changed,
    })
}

pub fn apply_update(candidate: &UpdateCandidate) -> Result<InstalledRepository> {
    let cancel = CancelToken::default();
    let inspection = inspect_repository(&candidate.repository_id, &cancel)?;
    if !inspection.status.is_empty() {
        return Err(invalid(
            "The installed library has local or staged changes. Commit or resolve them, then inspect the update again",
        ));
    }
    let previous = inspection.repository;
    if previous.root != candidate.previous_root
        || inspection.head != candidate.previous_revision
        || inspection.branch != candidate.previous_branch
        || inspection.fingerprint != candidate.previous_fingerprint
    {
        return Err(invalid(
            "The installed library changed after update review; inspect the update again",
        ));
    }
    let mut replacement = installation(&candidate.checkout.root, previous.enabled, &cancel)?;
    prove_update_ancestry(
        &replacement,
        candidate
            .previous_revision
            .as_deref()
            .ok_or_else(|| invalid("The reviewed update has no previous commit"))?,
        &cancel,
        &mut |_| {},
    )?;
    let snapshot = storage::snapshot(&replacement.root, &cancel)?;
    require_clean_snapshot(
        &snapshot,
        replacement.revision.as_deref(),
        &cancel,
        &mut |_| {},
    )?;
    if replacement.revision != candidate.checkout.revision
        || snapshot.fingerprint != candidate.fingerprint
        || replacement.manifest.id != previous.manifest.id
        || replacement.root == previous.root
    {
        return Err(invalid(
            "The update checkout changed after review; inspect it again",
        ));
    }
    replacement.id = previous.id.clone();
    replacement.remote = previous.remote.clone();
    mutate_registry(|registry| {
        let slot = registry
            .repositories
            .iter_mut()
            .find(|entry| entry.id == previous.id)
            .ok_or_else(|| invalid("The library was removed during update"))?;
        if slot.root != previous.root || slot.revision != previous.revision {
            return Err(invalid("The library changed during update"));
        }
        let actual = Git::local(&previous.root);
        if actual.head(&cancel)? != candidate.previous_revision
            || actual.branch(&cancel)? != candidate.previous_branch
            || storage::snapshot(&previous.root, &cancel)?.fingerprint
                != candidate.previous_fingerprint
        {
            return Err(invalid(
                "The installed library's branch changed while the update was being applied",
            ));
        }
        check_index(
            &actual,
            candidate.previous_revision.as_deref(),
            &cancel,
            &mut |_| {},
        )?;
        *slot = replacement.clone();
        Ok(())
    })?;
    Ok(replacement)
}

fn prove_update_ancestry(
    candidate: &InstalledRepository,
    previous: &str,
    cancel: &CancelToken,
    progress: &mut impl FnMut(&str),
) -> Result<()> {
    git::validate_commit(previous)?;
    let next = candidate
        .revision
        .as_deref()
        .ok_or_else(|| invalid("The downloaded library has no committed version"))?;
    if Git::local(&candidate.root)
        .run(
            &["merge-base", "--is-ancestor", previous, next],
            cancel,
            progress,
        )
        .is_err()
    {
        cancel.check()?;
        return Err(invalid(
            "The downloaded library could not be shown to contain your current commits. Merge the histories in Git, or register the downloaded source separately. The installed library was retained",
        ));
    }
    Ok(())
}

fn require_clean_snapshot(
    snapshot: &Snapshot,
    head: Option<&str>,
    cancel: &CancelToken,
    progress: &mut impl FnMut(&str),
) -> Result<()> {
    let git = Git::local(&snapshot.root);
    if !changes(snapshot, &git, head, cancel, progress)?.is_empty() {
        return Err(invalid(
            "The downloaded update has local changes. Inspect the update again in a fresh folder",
        ));
    }
    check_index(&git, head, cancel, progress)
}

pub fn publish_repository(
    request: &PublishRequest,
    connection: Option<&ConnectionProfile>,
    cancel: &CancelToken,
    mut progress: impl FnMut(&str),
) -> Result<PublishResult> {
    if !request.remote_reviewed {
        return Err(invalid(
            "Review this exact repository address and its access before publishing. Generic Git cannot determine whether it is private",
        ));
    }
    git::validate_commit(&request.expected_local_commit)?;
    if let Some(commit) = &request.expected_remote_commit {
        git::validate_commit(commit)?;
    }
    let mut inspection = inspect_repository(&request.repository_id, cancel)?;
    if inspection.head.as_ref() != Some(&request.expected_local_commit)
        || !inspection.status.is_empty()
    {
        return Err(invalid(
            "Commit the current library and review its latest version before publishing",
        ));
    }
    let address = RepositoryAddress::parse(&request.clone_url)?;
    let git = Git {
        root: &inspection.repository.root,
        index: None,
        address: Some(&address),
        connection,
    };
    git.check_branch(&request.branch, cancel)?;
    git.check_reviewed_address(true, cancel)?;
    git.check_reviewed_address(false, cancel)?;
    let remote =
        repository_models::inspect_remote(&address.git_url, connection, cancel, &mut progress)?;
    let actual = remote
        .branches
        .iter()
        .find(|branch| branch.name == request.branch)
        .map(|branch| branch.commit.clone());
    if actual != request.expected_remote_commit {
        return Err(invalid(
            "The remote branch changed after review; inspect it again before publishing",
        ));
    }
    let reference = format!("refs/heads/{}", request.branch);
    if let Some(expected) = &actual {
        git.run(
            &[
                "fetch",
                "--no-tags",
                "--no-recurse-submodules",
                &address.git_url,
                &reference,
            ],
            cancel,
            &mut progress,
        )?;
        let downloaded = git.run(&["rev-parse", "FETCH_HEAD^{commit}"], cancel, &mut progress)?;
        if downloaded.trim() != expected {
            return Err(invalid(
                "The remote branch changed during publication; inspect it again",
            ));
        }
        git.run(&["merge-base","--is-ancestor",expected,&request.expected_local_commit],cancel,&mut progress)
            .map_err(|_| invalid("The remote library has changes missing from this branch. Merge them in Git or publish a new branch; TCAD will not overwrite them"))?;
    }
    let snapshot = storage::snapshot(&inspection.repository.root, cancel)?;
    if snapshot.fingerprint != inspection.fingerprint
        || git.head(cancel)?.as_ref() != Some(&request.expected_local_commit)
    {
        return Err(invalid(
            "The library changed before upload; review it again",
        ));
    }
    let refspec = format!("{}:{reference}", request.expected_local_commit);
    progress("Publishing the reviewed library version");
    // No force option: a concurrent incompatible remote update is rejected by Git.
    git.run(
        &[
            "push",
            "--porcelain",
            "--no-follow-tags",
            "--",
            &address.git_url,
            &refspec,
        ],
        cancel,
        &mut progress,
    ).map_err(|error| invalid(format!("{error}. The remote may already contain the library version. Inspect the destination branch before retrying")))?;
    // Persisting a reviewed source is local bookkeeping after the successful upload.
    inspection.repository.remote = Some(address.git_url.clone());
    inspection.repository.branch = request.branch.clone();
    let mut notices = Vec::new();
    let bookkeeping = (|| -> Result<()> {
        let complete = CancelToken::default();
        if Git::local(&inspection.repository.root)
            .optional(&["remote", "get-url", "origin"], &complete)?
            .is_some()
        {
            Git::local(&inspection.repository.root).run(
                &["remote", "set-url", "origin", &address.git_url],
                &complete,
                &mut progress,
            )?;
        } else {
            Git::local(&inspection.repository.root).run(
                &["remote", "add", "origin", &address.git_url],
                &complete,
                &mut progress,
            )?;
        }
        git_connections::bind_reviewed_connection(&inspection.repository.root, connection)?;
        replace(&inspection.repository)
    })();
    if let Err(error) = bookkeeping {
        notices.push(format!("The library was published successfully. Local connection preferences need repair: {error}. Register {} again to refresh them", inspection.repository.root.display()));
    }
    Ok(PublishResult {
        repository: inspection.repository,
        commit: request.expected_local_commit.clone(),
        branch: request.branch.clone(),
        notices,
    })
}
