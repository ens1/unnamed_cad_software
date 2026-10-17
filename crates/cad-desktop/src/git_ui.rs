//! Forge-neutral history and controlled component updates over the shared draft.
use crate::{Workspace, presentation_controls as controls};
use bevy_egui::egui;
use cad_core::{
    Document, Id, library,
    persistence::{FileStore, ProjectStore},
    presentation::SemanticElement,
    version_control::{self as vc, GitOperation, GitReport},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    path::PathBuf,
    sync::{Mutex, mpsc},
};

#[derive(Default)]
pub struct State {
    receiver: Mutex<Option<mpsc::Receiver<std::result::Result<Outcome, String>>>>,
    pending: Option<Pending>,
    pub report: Option<GitReport>,
    impact: Option<library::ComponentUpdateImpact>,
    source: Option<Document>,
    source_request: Option<library::GitLinkRequest>,
    merge: Option<MergeCommit>,
    restored_merge: Option<(PathBuf, Id)>,
}
struct Pending {
    fingerprint: String,
    project: Option<PathBuf>,
    branch_switch: bool,
    generation: u64,
    document: Id,
}
#[derive(Serialize, Deserialize)]
struct MergeCommit {
    head: String,
    branch: String,
    other: String,
    candidate: String,
    other_files: Vec<String>,
    document: Id,
    #[serde(default)]
    accepted: bool,
}
enum Outcome {
    Git(GitReport),
    Compare(Document, PathBuf),
    Source(Document, library::GitLinkRequest),
    Linked(Document),
    Update(library::ComponentUpdatePlan),
    Checked(String),
    Merge {
        document: Document,
        head: String,
        branch: String,
        other: String,
        other_files: Vec<String>,
    },
}
fn manifest_name(w: &Workspace) -> String {
    w.project
        .as_ref()
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "json")
        })
        .and_then(|path| path.file_name())
        .and_then(|name| name.to_str())
        .unwrap_or("project.json")
        .to_string()
}
fn field(w: &Workspace, key: &str, default: &str) -> String {
    w.fields
        .get(&format!("git/{key}"))
        .cloned()
        .unwrap_or_else(|| default.into())
}
fn set_field(w: &mut Workspace, key: &str, value: impl Into<String>) {
    w.fields.insert(format!("git/{key}"), value.into());
}
pub fn busy(w: &Workspace) -> bool {
    w.git_ui
        .pending
        .as_ref()
        .is_some_and(|pending| pending.branch_switch)
}
pub fn operation_pending(w: &Workspace) -> bool {
    w.git_ui.pending.is_some()
}
pub fn recovery_pending(w: &Workspace) -> bool {
    w.git_ui
        .report
        .as_ref()
        .is_some_and(|report| report.recovery.is_some())
}
/// A new/opened project must never inherit another project's jobs, source picker or merge record.
/// Applied merge ancestry stays in that project's local cache and is restored on demand.
pub fn reset(w: &mut Workspace) {
    w.git_ui = State::default();
}
fn submit(
    w: &mut Workspace,
    branch_switch: bool,
    work: impl FnOnce() -> cad_core::Result<Outcome> + Send + 'static,
) {
    if w.git_ui.pending.is_some() {
        w.status = "Wait for the current Git operation".into();
        return;
    }
    let (sender, receiver) = mpsc::channel();
    if let Ok(mut slot) = w.git_ui.receiver.lock() {
        *slot = Some(receiver);
    }
    w.git_ui.pending = Some(Pending {
        fingerprint: w.session.visible().fingerprint(),
        project: w.project.clone(),
        branch_switch,
        generation: w.session.generation,
        document: w.session.visible().id.clone(),
    });
    std::thread::spawn(move || {
        let _ = sender.send(work().map_err(|error| error.to_string()));
    });
    w.status = "Git operation running".into();
}
fn exact_content(document: &Document) -> String {
    let mut document = document.clone();
    document.revision = 0;
    document.fingerprint()
}
fn merge_record(w: &Workspace) -> PathBuf {
    w.root().join(".tcad-cache/reviewed-merge.json")
}
fn persist_merge(w: &Workspace) {
    if let Some(merge) = &w.git_ui.merge {
        if let Ok(bytes) = serde_json::to_vec(merge) {
            let _ = cad_core::persistence::atomic_write(&merge_record(w), &bytes);
        }
    } else {
        let _ = std::fs::remove_file(merge_record(w));
    }
}
fn restore_merge(w: &mut Workspace) {
    let identity = w
        .project
        .as_ref()
        .map(|_| (w.root(), w.session.visible().id.clone()));
    if w.git_ui.restored_merge != identity {
        w.git_ui.merge = identity
            .as_ref()
            .and_then(|_| std::fs::read(merge_record(w)).ok())
            .and_then(|bytes| serde_json::from_slice::<MergeCommit>(&bytes).ok())
            .filter(|merge| merge.document == w.session.visible().id);
        w.git_ui.restored_merge = identity;
        if w.project.is_some()
            && w.git_ui.pending.is_none()
            && w.git_ui.report.is_none()
            && w.root()
                .ancestors()
                .any(|directory| directory.join(".git").exists())
        {
            run(w, GitOperation::Inspect);
        }
    }
    let accepted = w.git_ui.merge.as_ref().is_some_and(|merge| {
        !merge.accepted && merge.candidate == exact_content(&w.session.committed)
    });
    if accepted {
        if let Some(merge) = &mut w.git_ui.merge {
            merge.accepted = true;
        }
        persist_merge(w);
    }
}
pub fn poll(w: &mut Workspace) {
    restore_merge(w);
    let response = w.git_ui.receiver.lock().ok().and_then(|receiver| {
        receiver
            .as_ref()
            .and_then(|receiver| match receiver.try_recv() {
                Ok(reply) => Some(reply),
                Err(mpsc::TryRecvError::Empty) => None,
                Err(mpsc::TryRecvError::Disconnected) => Some(Err("The Git worker stopped before returning a result. Refresh to inspect repository state".into())),
            })
    });
    let Some(response) = response else {
        return;
    };
    if let Ok(mut slot) = w.git_ui.receiver.lock() {
        *slot = None;
    }
    let pending = w.git_ui.pending.take();
    let current = pending.as_ref().is_some_and(|pending| {
        pending.project == w.project
            && pending.fingerprint == w.session.visible().fingerprint()
            && pending.generation == w.session.generation
            && pending.document == w.session.visible().id
    });
    if !current {
        w.status = "The project changed while Git was running. The result was not applied; refresh the current project".into();
        return;
    }
    let result: cad_core::Result<()> = (|| {
        match response.map_err(cad_core::CadError::Invalid)? {
            Outcome::Git(report) => {
                if report.merge_committed {
                    w.git_ui.merge = None;
                    persist_merge(w);
                }
                if report.checkout_changed {
                    let path = w.project.clone().ok_or_else(|| {
                        cad_core::CadError::Invalid("The project path changed".into())
                    })?;
                    let document = FileStore.load(&path)?;
                    w.replace(document, Some(path));
                    w.git_ui.merge = None;
                    persist_merge(w);
                }
                w.git_text = if report.output.trim().is_empty() {
                    "Git state refreshed".into()
                } else {
                    report.output.clone()
                };
                w.status = w.git_text.clone();
                w.git_ui.report = Some(report);
            }
            Outcome::Compare(document, root) => crate::ui::compare(w, document, root),
            Outcome::Source(document, request) => {
                w.git_ui.source = Some(document);
                w.git_ui.source_request = Some(request);
                w.status = "Choose a component and configuration, then preview the link".into();
            }
            Outcome::Linked(document) => {
                let command = library::replace_command(w.session.visible(), document, &w.root())?;
                w.start_tool();
                w.dispatch(command);
                w.status = "Inspect the linked component. Apply accepts it; Cancel restores the prior model".into();
            }
            Outcome::Update(plan) => {
                if plan.base_fingerprint != w.session.visible().fingerprint() {
                    return Err(cad_core::CadError::Invalid(
                        "The component update is stale; prepare it again".into(),
                    ));
                }
                let command =
                    library::replace_command(w.session.visible(), plan.document, &w.root())?;
                w.start_tool();
                w.dispatch(command);
                w.git_ui.impact = Some(plan.impact);
                w.status = "Review affected items and the model preview. Apply waits for validation; Cancel keeps the old component version".into();
            }
            Outcome::Checked(message) => w.status = message,
            Outcome::Merge {
                document,
                head,
                branch,
                other,
                other_files,
            } => {
                let candidate = exact_content(&document);
                let command = library::replace_command(w.session.visible(), document, &w.root())?;
                w.start_tool();
                w.dispatch(command);
                w.git_ui.merge = Some(MergeCommit {
                    head,
                    branch,
                    other,
                    candidate,
                    other_files,
                    document: w.session.visible().id.clone(),
                    accepted: false,
                });
                persist_merge(w);
                w.status = "Review the merge preview, then Apply. Save and commit records both branch parents".into();
            }
        }
        Ok(())
    })();
    if let Err(error) = result {
        w.status = error.to_string();
    }
}
/// Cancel restores the shared document checkpoint. Keep an already applied merge's ancestry.
pub fn cancel_preview(w: &mut Workspace) {
    if w.git_ui.merge.as_ref().is_some_and(|merge| {
        !merge.accepted && merge.candidate != exact_content(&w.session.committed)
    }) {
        w.git_ui.merge = None;
        persist_merge(w);
    }
    w.git_ui.impact = None;
}
fn saved(w: &mut Workspace) -> bool {
    if w.project.is_none() {
        w.status = "Save this project before using Git".into();
        false
    } else {
        true
    }
}
fn no_draft(w: &mut Workspace) -> bool {
    if w.session.draft.is_some() || w.tool_checkpoint.is_some() || w.sketch.is_some() {
        w.status = "Apply or cancel the current operation first".into();
        false
    } else {
        true
    }
}
fn run(w: &mut Workspace, operation: GitOperation) {
    if !saved(w) {
        return;
    }
    let branch_switch = matches!(operation, GitOperation::SwitchBranch { .. });
    if branch_switch && (!no_draft(w) || w.saved_source != w.session.committed.fingerprint()) {
        w.status = "Save and commit changes before switching branches".into();
        return;
    }
    if matches!(operation, GitOperation::RecoverMerge)
        && (!no_draft(w) || w.saved_source != w.session.committed.fingerprint())
    {
        w.status =
            "Undo unsaved changes or reopen the saved project before recovering its merge checkout"
                .into();
        return;
    }
    let root = w.root();
    let changes_files = branch_switch
        || matches!(
            operation,
            GitOperation::Commit { .. }
                | GitOperation::CommitMerge { .. }
                | GitOperation::RecoverMerge
        );
    submit(w, changes_files, move || {
        match vc::execute(&root, &operation) {
            Ok(report) => Ok(Outcome::Git(report)),
            Err(error) => {
                if let Ok(mut report) = vc::execute(&root, &GitOperation::Inspect)
                    && report.recovery.is_some()
                {
                    report.output = error.to_string();
                    return Ok(Outcome::Git(report));
                }
                Err(error)
            }
        }
    });
}
pub fn commit_saved(w: &mut Workspace, message: String) {
    if !saved(w) || !no_draft(w) {
        return;
    }
    let manifest = w
        .project
        .as_ref()
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "json")
        })
        .and_then(|path| path.file_name())
        .and_then(|name| name.to_str())
        .unwrap_or("project.json")
        .to_string();
    let operation = if let Some(merge) = &w.git_ui.merge {
        if !merge.accepted && exact_content(&w.session.committed) != merge.candidate {
            w.status = "The merge preview changed or was canceled. Preview the branch merge again before committing it".into();
            return;
        }
        GitOperation::CommitMerge {
            message,
            manifest,
            expected_head: merge.head.clone(),
            expected_branch: merge.branch.clone(),
            other_commit: merge.other.clone(),
        }
    } else {
        GitOperation::Commit { message, manifest }
    };
    run(w, operation);
}
fn request(w: &Workspace) -> library::GitLinkRequest {
    library::GitLinkRequest {
        remote: field(w, "source-remote", ""),
        project_path: field(w, "source-project", ""),
        revision: field(w, "source-revision", "HEAD"),
        update_ref: field(w, "source-channel", "HEAD"),
        definition: Some(field(w, "source-definition", "")).filter(|value| !value.is_empty()),
        configuration: Some(field(w, "source-configuration", "")).filter(|value| !value.is_empty()),
    }
}
pub fn action(w: &mut Workspace, command: &str, arguments: Option<&Value>) -> bool {
    if !command.starts_with("git.") && !command.starts_with("component-source.") {
        return false;
    }
    if w.git_ui.pending.is_some() {
        w.status = "Wait for the current Git operation".into();
        return true;
    }
    if matches!(
        command,
        "git.merge"
            | "component-source.link-local"
            | "component-source.link-git"
            | "component-source.update"
    ) && cad_core::release::is_release_source(&w.root())
    {
        w.status =
            "This is a released source snapshot. Use Save As to create working source first".into();
        return true;
    }
    let remote = field(w, "remote", "origin");
    match command {
        "git.refresh" => run(w, GitOperation::Inspect),
        "git.recover-merge" => run(w, GitOperation::RecoverMerge),
        "git.initialize" => run(w, GitOperation::Initialize),
        "git.fetch" => run(w, GitOperation::Fetch { remote }),
        "git.push" => run(w, GitOperation::Push { remote }),
        "git.add-remote" => run(
            w,
            GitOperation::AddRemote {
                name: remote,
                url: field(w, "remote-url", ""),
            },
        ),
        "git.create-branch" => run(
            w,
            GitOperation::CreateBranch {
                name: field(w, "branch", ""),
            },
        ),
        "git.switch-branch" => run(
            w,
            GitOperation::SwitchBranch {
                name: field(w, "branch", ""),
                manifest: manifest_name(w),
            },
        ),
        "git.commit" => {
            if no_draft(w) && crate::ui::save(w, false) {
                let message = field(w, "message", "");
                if w.readme_preview.include_on_commit {
                    crate::readme_preview_ui::request_before_commit(w, message);
                } else {
                    commit_saved(w, message);
                }
            }
        }
        "git.compare" => {
            if saved(w) {
                let revision = field(w, "revision", "HEAD");
                let manifest = manifest_name(w);
                let root = w.root();
                let destination = crate::recovery_directory()
                    .join("comparisons")
                    .join(cad_core::id());
                submit(w, false, move || {
                    Ok(Outcome::Compare(
                        vc::snapshot_resources_manifest(&root, &revision, &manifest, &destination)?,
                        destination,
                    ))
                });
            }
        }
        "git.clear-comparison" => {
            w.compare = None;
            w.compare_document = None;
            w.rendered_key.clear();
        }
        "git.discard-merge" => {
            if no_draft(w) {
                w.git_ui.merge = None;
                persist_merge(w);
                w.status =
                    "Pending merge record removed. Current model changes are still present".into();
            }
        }
        "git.merge" => {
            if w.git_ui.merge.is_some() {
                w.status = "Commit the reviewed merge or discard its pending merge record before starting another branch merge".into();
                return true;
            }
            if saved(w) && no_draft(w) && crate::ui::save(w, false) {
                let root = w.root();
                let ours = w.session.committed.clone();
                let revision = field(w, "revision", "");
                let manifest = manifest_name(w);
                submit(w, false, move || {
                    let merge_info = vc::branch_merge_info(&root, &manifest, &revision)?;
                    let head = merge_info.head;
                    let branch = merge_info.branch;
                    let other = merge_info.other;
                    let base = merge_info.base;
                    let temporary = root.join(".tcad-cache/merge-snapshot").join(cad_core::id());
                    let result = (|| {
                        let theirs =
                            vc::snapshot_resources_manifest(&root, &other, &manifest, &temporary)?;
                        let base = vc::snapshot_manifest(&root, base.trim(), &manifest)?;
                        let mut document = vc::merge(&base, &ours, &theirs)?;
                        vc::materialize_merge_resources(
                            &mut document,
                            &[(&ours, root.as_path()), (&theirs, temporary.as_path())],
                            &root,
                            &manifest,
                        )?;
                        Ok(Outcome::Merge {
                            document,
                            head,
                            branch,
                            other,
                            other_files: merge_info.other_files,
                        })
                    })();
                    let _ = std::fs::remove_dir_all(temporary);
                    result
                });
            }
        }
        "component-source.inspect" => {
            if saved(w) && no_draft(w) {
                let root = w.root();
                let request = request(w);
                submit(w, false, move || {
                    let (document, commit) = library::inspect_git_source(&root, &request)?;
                    let mut request = request;
                    request.revision = commit;
                    Ok(Outcome::Source(document, request))
                });
            }
        }
        "component-source.link-git" => {
            if saved(w) && no_draft(w) {
                let mut document = w.session.visible().clone();
                let root = w.root();
                let Some(mut request) = w.git_ui.source_request.clone() else {
                    w.status = "Inspect the Git source before linking it".into();
                    return true;
                };
                let fields = request_from_choices(w);
                request.definition = fields.0;
                request.configuration = fields.1;
                submit(w, false, move || {
                    library::link_git(&mut document, &root, &request, None)?;
                    Ok(Outcome::Linked(document))
                });
            }
        }
        "component-source.link-local" => {
            if no_draft(w)
                && let Some(source) = rfd::FileDialog::new()
                    .add_filter("TCAD project", &["json"])
                    .pick_file()
            {
                let mut document = w.session.visible().clone();
                let root = w.root();
                submit(w, false, move || {
                    library::link(&mut document, &root, &source, None, None)?;
                    Ok(Outcome::Linked(document))
                });
            }
        }
        "component-source.locate" => {
            if no_draft(w) {
                let link = arguments
                    .and_then(|args| args.get("link"))
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                if !w.session.visible().libraries.contains_key(&link) {
                    w.status = "Choose a component source".into();
                    return true;
                }
                if let Some(source) = rfd::FileDialog::new()
                    .set_title("Locate the original component project")
                    .add_filter("TCAD project", &["json"])
                    .pick_file()
                {
                    let before = w.session.visible().clone();
                    let root = w.root();
                    submit(w, false, move || {
                        let mut document = before.clone();
                        library::link(&mut document, &root, &source, None, Some(&link))?;
                        let impact = library::update_impact(&before, &document, &link)?;
                        Ok(Outcome::Update(library::ComponentUpdatePlan {
                            base_fingerprint: before.fingerprint(),
                            link,
                            document,
                            impact,
                        }))
                    });
                }
            }
        }
        "component-source.check" | "component-source.update" => {
            if saved(w) && no_draft(w) {
                let link = arguments
                    .and_then(|args| args.get("link"))
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let Some(source) = w.session.visible().libraries.get(&link).cloned() else {
                    w.status = "Choose a component source".into();
                    return true;
                };
                let root = w.root();
                let document = w.session.visible().clone();
                if command == "component-source.check" {
                    submit(w, false, move || {
                        Ok(Outcome::Checked(
                            match library::check_update(&source, &root, true)? {
                                Some((revision, _)) => format!(
                                    "{} has an update (source revision {revision}). Preview it before applying",
                                    source.name
                                ),
                                None => format!("{} is current", source.name),
                            },
                        ))
                    });
                } else {
                    let revision = arguments
                        .and_then(|args| args.get("revision"))
                        .and_then(Value::as_str)
                        .map(str::to_string);
                    submit(w, false, move || {
                        Ok(Outcome::Update(library::prepare_update(
                            &document,
                            &root,
                            &link,
                            revision.as_deref(),
                        )?))
                    });
                }
            }
        }
        _ => return false,
    }
    true
}
fn request_from_choices(w: &Workspace) -> (Option<Id>, Option<Id>) {
    (
        Some(field(w, "source-definition", "")).filter(|v| !v.is_empty()),
        Some(field(w, "source-configuration", "")).filter(|v| !v.is_empty()),
    )
}
fn value_element(
    w: &Workspace,
    key: &str,
    label: &str,
    default: &str,
    choices: Option<Vec<Value>>,
) -> SemanticElement {
    let value = field(w, key, default);
    let enabled = w.git_ui.pending.is_none();
    SemanticElement {
        id: format!("git/{key}"),
        role: "git-field".into(),
        label: label.into(),
        value: Some(value.clone()),
        state: if enabled { "editable" } else { "readonly" }.into(),
        commands: vec![],
        relationships: vec![w.session.visible().id.clone()],
        data: json!({"contract":"tcad.presentation.git","version":1,"setter":"git-ui","document":w.session.visible().id,"key":key,"json":value,"value_type":if choices.is_some(){"choice"}else{"text"},"choices":choices,"editable":enabled,"classes":["modeling-field"],"accessibility_label":label}),
    }
}
fn action_element(w: &Workspace, command: &str, label: &str, args: Value) -> SemanticElement {
    let enabled = w.git_ui.pending.is_none();
    SemanticElement {
        id: format!(
            "{command}/{}",
            args.get("link")
                .and_then(Value::as_str)
                .unwrap_or("project")
        ),
        role: "git-action".into(),
        label: label.into(),
        value: None,
        state: if enabled { "ready" } else { "disabled" }.into(),
        commands: vec![command.into()],
        relationships: vec![w.session.visible().id.clone()],
        data: json!({"classes":["semantic-control","modeling-action"],"actions":[{"command":command,"label":label,"enabled":enabled,"arguments":args}]}),
    }
}
pub fn current_value(w: &Workspace, element: &SemanticElement) -> Option<Value> {
    (element.data["setter"] == "git-ui").then(|| {
        json!(field(
            w,
            element.data["key"].as_str().unwrap_or(""),
            element.value.as_deref().unwrap_or("")
        ))
    })
}
pub fn set_value(
    w: &mut Workspace,
    element: &SemanticElement,
    value: Value,
) -> cad_core::Result<()> {
    if element.data["document"] != w.session.visible().id || w.git_ui.pending.is_some() {
        return Err(cad_core::CadError::Invalid(
            "Use the current Git controls after the operation completes".into(),
        ));
    }
    let key = element.data["key"]
        .as_str()
        .ok_or_else(|| cad_core::CadError::Invalid("Unknown Git field".into()))?;
    let value = value
        .as_str()
        .ok_or_else(|| cad_core::CadError::Invalid("Enter text for this Git field".into()))?;
    if value.len() > 8192 || value.contains('\0') {
        return Err(cad_core::CadError::Invalid(
            "The Git field is too long or contains a null character".into(),
        ));
    }
    if matches!(
        key,
        "source-remote" | "source-project" | "source-revision" | "source-channel"
    ) {
        w.git_ui.source = None;
        w.git_ui.source_request = None;
    }
    if key == "source-definition" {
        set_field(w, "source-configuration", "");
    }
    set_field(w, key, value);
    Ok(())
}
fn version_elements(w: &Workspace) -> Vec<SemanticElement> {
    let mut elements = vec![
        action_element(w, "repositories", "Open or create a repository…", json!({})),
        action_element(
            w,
            "git-connections",
            "Git connections and credentials…",
            json!({}),
        ),
        action_element(w, "repositories.publish", "Publish this model…", json!({})),
        value_element(w, "message", "Commit message", "", None),
        action_element(w, "git.commit", "Save and commit", json!({})),
        value_element(w, "branch", "Branch name", "", None),
        action_element(w, "git.create-branch", "Create branch", json!({})),
        action_element(w, "git.switch-branch", "Switch branch", json!({})),
        value_element(w, "revision", "Compare or merge from", "HEAD", None),
        action_element(w, "git.compare", "Compare revision", json!({})),
        action_element(w, "git.merge", "Preview branch merge", json!({})),
        action_element(w, "git.clear-comparison", "Clear comparison", json!({})),
        value_element(w, "remote", "Remote name", "origin", None),
        value_element(w, "remote-url", "Remote URL", "", None),
        action_element(w, "git.add-remote", "Add remote", json!({})),
    ];
    for (command, label) in [
        ("git.refresh", "Refresh Git state"),
        ("git.initialize", "Initialize repository"),
        ("git.fetch", "Fetch remote"),
        ("git.push", "Push current branch"),
    ] {
        elements.push(action_element(w, command, label, json!({})));
    }
    if w.git_ui
        .report
        .as_ref()
        .is_some_and(|report| report.recovery.is_some())
    {
        elements.push(action_element(
            w,
            "git.recover-merge",
            "Recover interrupted merge",
            json!({}),
        ));
    }
    if w.git_ui.merge.is_some() {
        elements.push(action_element(
            w,
            "git.discard-merge",
            "Discard pending merge record",
            json!({}),
        ));
    }
    elements
}
fn library_elements(w: &Workspace) -> Vec<SemanticElement> {
    let mut elements = vec![
        action_element(
            w,
            "component-source.link-local",
            "Link local project…",
            json!({}),
        ),
        value_element(w, "source-remote", "Git repository URL", "", None),
        value_element(
            w,
            "source-project",
            "Project folder in repository",
            "",
            None,
        ),
        value_element(
            w,
            "source-revision",
            "Initial commit, tag or branch",
            "HEAD",
            None,
        ),
        value_element(w, "source-channel", "Check updates from", "HEAD", None),
        action_element(
            w,
            "component-source.inspect",
            "Fetch and inspect source",
            json!({}),
        ),
    ];
    if let Some(source) = &w.git_ui.source {
        let choices = std::iter::once(json!({"value":"","label":"Project root assembly"}))
            .chain(
                source
                    .parts
                    .values()
                    .map(|part| json!({"value":part.id,"label":part.name})),
            )
            .chain(
                source
                    .assemblies
                    .values()
                    .map(|assembly| json!({"value":assembly.id,"label":assembly.name})),
            )
            .collect();
        elements.push(value_element(
            w,
            "source-definition",
            "Component",
            "",
            Some(choices),
        ));
        let definition = field(w, "source-definition", &source.root);
        let definition = if definition.is_empty() {
            &source.root
        } else {
            &definition
        };
        let configurations = std::iter::once(json!({"value":"","label":"Default configuration"}))
            .chain(
                cad_core::configuration::names(source, definition)
                    .iter()
                    .map(|(id, name)| json!({"value":id,"label":name})),
            )
            .collect();
        elements.push(value_element(
            w,
            "source-configuration",
            "Configuration",
            "",
            Some(configurations),
        ));
        elements.push(action_element(
            w,
            "component-source.link-git",
            "Preview linked component",
            json!({}),
        ));
    }
    for link in w.session.visible().libraries.values() {
        elements.push(action_element(
            w,
            "component-source.check",
            &format!("Check {}", link.name),
            json!({"link":link.id}),
        ));
        if link.git_source.is_none() {
            elements.push(action_element(
                w,
                "component-source.locate",
                &format!("Locate {} source…", link.name),
                json!({"link":link.id}),
            ));
        }
        elements.push(action_element(
            w,
            "component-source.update",
            &format!("Preview {} update", link.name),
            json!({"link":link.id}),
        ));
    }
    elements
}
pub fn semantics(w: &Workspace) -> Vec<SemanticElement> {
    let mut elements = version_elements(w);
    elements.extend(library_elements(w));
    if let Some(report) = &w.git_ui.report {
        elements.push(SemanticElement{id:"git/repository-status".into(),role:"git-repository-status".into(),label:format!("Branch: {}",report.branch),value:Some(report.status.clone()),state:"ready".into(),commands:vec!["git.refresh".into()],relationships:vec![w.session.visible().id.clone()],data:json!({"contract":"tcad.git-status","version":1,"branch":report.branch,"status":report.status,"history":report.history,"branches":report.branches,"remotes":report.remotes,"recovery":report.recovery})});
    }
    for link in w.session.visible().libraries.values() {
        elements.push(SemanticElement{id:format!("component-source/{}",link.id),role:"component-source-pin".into(),label:link.name.clone(),value:Some(link.git_source.as_ref().map(|source|source.commit.clone()).unwrap_or_else(||link.source_sha256.clone())),state:"pinned".into(),commands:vec!["component-source.check".into(),"component-source.update".into()],relationships:vec![link.id.clone(),link.definition.clone()],data:json!({"contract":"tcad.component-source","version":1,"link":link.id,"source_document":link.source_document,"source_definition":link.source_definition,"source_revision":link.source_revision,"source_sha256":link.source_sha256,"git_source":link.git_source,"definition_closure":link.definition_closure,"offline_snapshot":link.snapshot_resource})});
    }
    if let Some(impact) = &w.git_ui.impact {
        elements.push(SemanticElement{id:"component-source/update-impact".into(),role:"component-update-impact".into(),label:"Component update review".into(),value:Some(format!("{} components · {} mates · {} drawings · {} BOM rows",impact.occurrences.len(),impact.mates.len(),impact.drawings.len(),impact.bom_rows.len())),state:"review".into(),commands:vec![],relationships:impact.changed_definitions.clone(),data:json!({"contract":"tcad.component-update-impact","version":1,"impact":impact,"requires_current_evaluation":true})});
    }
    elements
}
fn show_element(ui: &mut egui::Ui, w: &mut Workspace, element: SemanticElement) {
    if element.role == "git-field" {
        crate::ui::semantic_field(ui, w, &element);
    } else if controls::button(
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
        let args = element.data["actions"][0]["arguments"].clone();
        if !crate::git_connections_ui::action(w, &element.commands[0], Some(&args))
            && !crate::repositories_ui::action(w, &element.commands[0], Some(&args))
        {
            action(w, &element.commands[0], Some(&args));
        }
    }
}
pub fn versions(ui: &mut egui::Ui, w: &mut Workspace) {
    ui.label("Save stores your work. Commit records a version. Fetch retrieves history; it does not change the model.");
    if let Some(report) = &w.git_ui.report {
        ui.strong(format!("Branch: {}", report.branch));
        if let Some(recovery) = &report.recovery {
            ui.strong("Merge checkout needs recovery");
            ui.label(&recovery.message);
            ui.label(format!("Recorded branch: {}", recovery.branch));
        }
    }
    crate::readme_preview_ui::show(ui, w);
    if let Some(merge) = &w.git_ui.merge {
        ui.label("The merge commit will record both branches.");
        if !merge.other_files.is_empty() {
            ui.collapsing(
                format!("{} other files also change", merge.other_files.len()),
                |ui| {
                    for file in &merge.other_files {
                        ui.monospace(file);
                    }
                },
            );
        }
    }
    for element in version_elements(w) {
        show_element(ui, w, element);
    }
    if let Some(report) = &w.git_ui.report {
        for (label, text) in [
            ("Changed files", &report.status),
            ("History", &report.history),
            ("Branches", &report.branches),
            ("Remotes", &report.remotes),
        ] {
            ui.collapsing(label, |ui| {
                ui.monospace(if text.trim().is_empty() { "None" } else { text });
            });
        }
    }
    if let Some(document) = &w.compare_document {
        ui.label(format!(
            "Comparison: {} · revision {}",
            document.name, document.revision
        ));
        if let Ok(changes) = vc::diff(document, &w.session.committed) {
            ui.collapsing(format!("{} source changes", changes.len()), |ui| {
                for change in changes {
                    ui.small(change.path);
                    ui.monospace(format!(
                        "{} → {}",
                        change.before.unwrap_or(Value::Null),
                        change.after.unwrap_or(Value::Null)
                    ));
                }
            });
        }
    }
    ui.small(&w.git_text);
}
pub fn libraries(ui: &mut egui::Ui, w: &mut Workspace) {
    ui.label("Linked components keep exact source versions and work offline. Updates require preview and Apply.");
    for element in library_elements(w) {
        show_element(ui, w, element);
    }
    for link in w.session.visible().libraries.values() {
        ui.separator();
        ui.strong(&link.name);
        if let Some(git) = &link.git_source {
            ui.small(format!("{} · commit {}", git.remote, &git.commit[..12]));
            ui.small(format!("Update channel: {}", git.update_ref));
        } else {
            ui.small(format!(
                "{} · source revision {}",
                link.source_path, link.source_revision
            ));
        }
    }
    if let Some(impact) = &w.git_ui.impact {
        ui.separator();
        ui.strong("Update impact");
        ui.small(format!(
            "{} → {}",
            impact.source_before, impact.source_after
        ));
        ui.label(format!(
            "{} component occurrences · {} mates · {} drawings · {} changed BOM rows",
            impact.occurrences.len(),
            impact.mates.len(),
            impact.drawings.len(),
            impact.bom_rows.len()
        ));
        ui.collapsing("Affected components", |ui| {
            for occurrence in &impact.occurrences {
                ui.label(&occurrence.name);
            }
        });
        ui.collapsing("Source changes", |ui| {
            for change in &impact.changes {
                ui.small(&change.path);
                ui.monospace(format!(
                    "{} → {}",
                    change.before.as_ref().unwrap_or(&Value::Null),
                    change.after.as_ref().unwrap_or(&Value::Null)
                ));
            }
        });
    }
}
