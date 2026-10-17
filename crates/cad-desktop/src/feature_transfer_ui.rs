//! Docked selective transfer over the ordinary shared draft and geometry worker.
use crate::{Workspace, presentation_controls as controls};
use bevy_egui::egui;
use cad_core::{
    Document, NodeRef, Result,
    commands::Command,
    feature_transfer::{self, NodeBinding, ParameterBinding, Plan, Request, Resolution},
    persistence::{FileStore, ProjectStore},
    presentation::SemanticElement,
    version_control,
};
use serde_json::{Value, json};
use std::{
    path::PathBuf,
    sync::{
        Mutex,
        mpsc::{self, Receiver},
    },
};

struct Loaded {
    document: Document,
    base: Option<Document>,
    root: PathBuf,
    commit: Option<String>,
}
enum Reply {
    Loaded(Box<Loaded>),
    Preview(Box<Document>),
}
pub struct State {
    baseline: Document,
    root: PathBuf,
    source_path: String,
    revision: String,
    loaded: Option<Loaded>,
    request: Request,
    plan: Option<Plan>,
    pending: Option<Mutex<Receiver<std::result::Result<Reply, String>>>>,
    preview_fingerprint: Option<String>,
    error: Option<String>,
    generation: String,
    session_generation: u64,
}

fn invalid(message: impl Into<String>) -> cad_core::CadError {
    cad_core::CadError::Invalid(message.into())
}
pub fn open(w: &mut Workspace) {
    if w.settings.is_some()
        || w.sketch.is_some()
        || w.tool_checkpoint.is_some()
        || w.recovery_draft.is_some()
        || w.session.draft.is_some()
    {
        w.status = "Finish the current editor before transferring features".into();
        return;
    }
    let baseline = w.session.visible().clone();
    let target_definition = w
        .view
        .active_part
        .clone()
        .filter(|id| baseline.parts.contains_key(id))
        .or_else(|| baseline.parts.keys().next().cloned())
        .unwrap_or_default();
    let root = w.root();
    w.start_tool();
    w.feature_transfer = Some(State {
        baseline,
        source_path: w.project.as_ref().unwrap_or(&root).to_string_lossy().into(),
        root,
        revision: "HEAD".into(),
        loaded: None,
        request: Request {
            target_definition,
            reconnect_outputs: true,
            ..Default::default()
        },
        plan: None,
        pending: None,
        preview_fingerprint: None,
        error: None,
        generation: cad_core::id(),
        session_generation: w.session.generation,
    });
    w.open_editor("feature-transfer");
    w.status =
        "Choose a source branch or the current project, then select the features to transfer"
            .into();
}
pub fn cancel(w: &mut Workspace) {
    w.feature_transfer = None;
}

/// Called by shared Apply. A previously valid model is insufficient after the transfer selection changes.
pub fn current_source_is_valid(w: &Workspace) -> bool {
    w.feature_transfer.as_ref().is_none_or(|state| {
        current(state, w)
            && state.pending.is_none()
            && state.error.is_none()
            && state.preview_fingerprint.as_deref()
                == Some(w.session.visible().fingerprint().as_str())
            && state
                .plan
                .as_ref()
                .is_some_and(|plan| plan.candidate.is_some())
    })
}
pub fn ready_to_confirm(w: &mut Workspace) -> bool {
    if current_source_is_valid(w) {
        true
    } else {
        w.status =
            "Preview the current feature selection and resolve its conflicts before Apply".into();
        false
    }
}
fn current(state: &State, w: &Workspace) -> bool {
    w.session.generation == state.session_generation
        && w.root() == state.root
        && w.session.visible().id == state.baseline.id
        && (w.session.visible().fingerprint() == state.baseline.fingerprint()
            || state.preview_fingerprint.as_deref()
                == Some(w.session.visible().fingerprint().as_str()))
}
fn rebuild(state: &mut State) {
    state.plan = None;
    state.error = None;
    let Some(loaded) = &state.loaded else {
        return;
    };
    match feature_transfer::prepare(
        &loaded.document,
        loaded.base.as_ref(),
        &state.baseline,
        &state.request,
    ) {
        Ok(plan) => state.plan = Some(plan),
        Err(error) => state.error = Some(error.to_string()),
    }
}
fn invalidate(w: &mut Workspace, state: &mut State) {
    if state.preview_fingerprint.take().is_some() {
        w.dispatch(Command::ReplaceDocument {
            document: state.baseline.clone(),
        });
    }
    w.confirm_requested = false;
}
fn load(w: &mut Workspace, local: bool) {
    let Some(mut state) = w.feature_transfer.take() else {
        return;
    };
    if state.pending.is_some() || !current(&state, w) {
        w.status =
            "Wait for the source load, or reopen the transfer after the document changes".into();
        w.feature_transfer = Some(state);
        return;
    }
    invalidate(w, &mut state);
    state.plan = None;
    state.error = None;
    state.request.selected_features.clear();
    state.request.include_dependencies.clear();
    state.request.bindings.clear();
    state.request.parameter_bindings.clear();
    state.request.repairs.clear();
    state.request.resolutions.clear();
    if local {
        state.loaded = Some(Loaded {
            document: state.baseline.clone(),
            base: None,
            root: state.root.clone(),
            commit: None,
        });
        state.request.source_definition = state.request.target_definition.clone();
        rebuild(&mut state);
    } else {
        let source_path = PathBuf::from(&state.source_path);
        let source_root = cad_core::persistence::project_root(&source_path);
        let manifest = if source_path
            .extension()
            .is_some_and(|extension| extension == "json")
        {
            source_path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("project.json")
                .to_owned()
        } else {
            "project.json".into()
        };
        let target_root = state.root.clone();
        let revision = state.revision.clone();
        let (sender, receiver) = mpsc::channel();
        state.pending = Some(Mutex::new(receiver));
        std::thread::spawn(move || {
            let result = (|| -> Result<Reply> {
                if revision.trim().is_empty() {
                    if source_root.join(".tcad-recovery/pending.json").exists() {
                        return Err(invalid("Open and recover the source project before transferring its saved features."));
                    }
                    return Ok(Reply::Loaded(Box::new(Loaded { document: FileStore.load(&source_path)?, base: None, root: source_root, commit: None })));
                }
                let commit = version_control::resolve_commit(&source_root, &revision)?;
                let document = version_control::snapshot_manifest(&source_root, &commit, &manifest)?;
                let source_repository = version_control::repository_root(&source_root).ok();
                let base = if source_repository.is_some() && source_repository == version_control::repository_root(&target_root).ok() {
                    version_control::resolve_commit(&target_root, "HEAD").ok()
                        .and_then(|head| version_control::merge_base(&source_root, &head, &commit).ok())
                        .and_then(|rev| version_control::snapshot_manifest(&source_root, &rev, &manifest).ok())
                } else {
                    None
                };
                Ok(Reply::Loaded(Box::new(Loaded {
                    document,
                    base,
                    root: source_root,
                    commit: Some(commit),
                })))
            })()
            .map_err(|error| error.to_string());
            let _ = sender.send(result);
        });
        w.status = "Reading the source revision without changing the working branch".into();
    }
    w.feature_transfer = Some(state);
}
fn preview(w: &mut Workspace) {
    let Some(mut state) = w.feature_transfer.take() else {
        return;
    };
    if state.pending.is_some() || !current(&state, w) {
        state.error = Some("The target changed. Close and reopen the transfer.".into());
    } else {
        rebuild(&mut state);
        if let (Some(plan), Some(loaded)) = (&state.plan, &state.loaded)
            && plan.candidate.is_some()
        {
            let plan = plan.clone();
            let source_root = loaded.root.clone();
            let commit = loaded.commit.clone();
            let target_root = state.root.clone();
            let target = state.baseline.clone();
            let (sender, receiver) = mpsc::channel();
            state.pending = Some(Mutex::new(receiver));
            std::thread::spawn(move || {
                let result = (|| -> Result<Reply> {
                    let candidate = plan.document(&target)?;
                    if let Some(commit) = commit {
                        let stage = std::env::temp_dir()
                            .join(format!("tcad-feature-transfer-{}", cad_core::id()));
                        let result = (|| -> Result<()> {
                            for resource in &plan.resources {
                                version_control::snapshot_resource(
                                    &source_root,
                                    &commit,
                                    &resource.source,
                                    &stage,
                                )?;
                            }
                            feature_transfer::materialize(&plan, &stage, &target_root)
                        })();
                        let _ = std::fs::remove_dir_all(&stage);
                        result?;
                    } else {
                        feature_transfer::materialize(&plan, &source_root, &target_root)?;
                    }
                    Ok(Reply::Preview(Box::new(candidate)))
                })()
                .map_err(|error| error.to_string());
                let _ = sender.send(result);
            });
            w.status = "Preparing selected features and their required files".into();
        }
    }
    w.feature_transfer = Some(state);
}
pub fn poll(w: &mut Workspace) {
    let Some(mut state) = w.feature_transfer.take() else {
        return;
    };
    let reply =
        state
            .pending
            .as_ref()
            .and_then(|receiver| match receiver.lock().ok()?.try_recv() {
                Ok(reply) => Some(reply),
                Err(mpsc::TryRecvError::Disconnected) => Some(Err(
                    "The feature transfer worker stopped before it returned a result".into(),
                )),
                Err(mpsc::TryRecvError::Empty) => None,
            });
    if let Some(reply) = reply {
        state.pending = None;
        if !current(&state, w) {
            state.error = Some("The target changed. Close and reopen the transfer.".into());
        } else {
            match reply {
                Ok(Reply::Loaded(loaded)) => {
                    state.request.source_definition = loaded
                        .document
                        .parts
                        .keys()
                        .find(|id| *id == &state.request.target_definition)
                        .or_else(|| loaded.document.parts.keys().next())
                        .cloned()
                        .unwrap_or_default();
                    state.loaded = Some(*loaded);
                    rebuild(&mut state);
                    w.status = "Source loaded. Select the features to transfer.".into();
                }
                Ok(Reply::Preview(document)) => {
                    let fingerprint = document.fingerprint();
                    w.dispatch(Command::ReplaceDocument {
                        document: *document,
                    });
                    if w.session.visible().fingerprint() == fingerprint {
                        state.preview_fingerprint = Some(fingerprint);
                        state.error = None;
                        w.view.active_part = Some(state.request.target_definition.clone());
                        w.rendered_key.clear();
                        w.status = "Evaluating the transfer preview. Apply waits for the current model to be valid.".into();
                    } else {
                        state.error = Some(w.status.clone());
                    }
                }
                Err(error) => {
                    w.status = error.clone();
                    state.error = Some(error);
                }
            }
        }
    }
    w.feature_transfer = Some(state);
}

fn element(
    state: &State,
    key: &str,
    label: &str,
    value: Option<Value>,
    choices: Vec<Value>,
    command: Option<&str>,
    enabled: bool,
) -> SemanticElement {
    let field = value.is_some();
    let mut data = json!({"contract":"tcad.feature-transfer","version":1,"editor":state.generation,"key":key,"enabled":enabled});
    if let Some(value) = &value {
        data["setter"] = json!("feature-transfer");
        data["json"] = value.clone();
        data["editable"] = json!(enabled);
        data["choices"] = json!(choices);
        data["value_type"] = json!(if !choices.is_empty() {
            "choice"
        } else if value.is_boolean() {
            "boolean"
        } else {
            "text"
        });
    }
    if let Some(command) = command {
        data["actions"] = json!([{"command":command,"label":label,"enabled":enabled}]);
    }
    SemanticElement {
        id: format!("feature-transfer:{key}"),
        role: if field {
            "feature-transfer-field"
        } else {
            "feature-transfer-action"
        }
        .into(),
        label: label.into(),
        value: value.map(|value| {
            value
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| value.to_string())
        }),
        state: if enabled { "ready" } else { "disabled" }.into(),
        commands: command.into_iter().map(str::to_owned).collect(),
        relationships: vec![
            state.request.source_definition.clone(),
            state.request.target_definition.clone(),
        ],
        data,
    }
}

pub fn semantics(w: &Workspace) -> Vec<SemanticElement> {
    let Some(state) = &w.feature_transfer else {
        return vec![];
    };
    if w.dialog.as_deref() != Some("feature-transfer") {
        return vec![];
    }
    let enabled = state.pending.is_none() && current(state, w);
    let mut out = vec![
        element(
            state,
            "source-path",
            "Source project file or folder",
            Some(json!(state.source_path)),
            vec![],
            None,
            enabled,
        ),
        element(
            state,
            "revision",
            "Branch or commit (blank uses saved files)",
            Some(json!(state.revision)),
            vec![],
            None,
            enabled,
        ),
        element(
            state,
            "load",
            "Load source revision",
            None,
            vec![],
            Some("feature-transfer.load"),
            enabled,
        ),
        element(
            state,
            "local",
            "Use current project",
            None,
            vec![],
            Some("feature-transfer.local"),
            enabled,
        ),
    ];
    let Some(loaded) = &state.loaded else {
        return out;
    };
    let parts = |document: &Document| {
        document
            .parts
            .values()
            .map(|part| json!({"value":part.id,"label":part.name}))
            .collect::<Vec<_>>()
    };
    out.push(element(
        state,
        "source-part",
        "Take features from",
        Some(json!(state.request.source_definition)),
        parts(&loaded.document),
        None,
        enabled,
    ));
    out.push(element(
        state,
        "target-part",
        "Add features to",
        Some(json!(state.request.target_definition)),
        parts(&state.baseline),
        None,
        enabled,
    ));
    if let Some(part) = loaded.document.parts.get(&state.request.source_definition) {
        for feature in &part.features {
            out.push(element(
                state,
                &format!("select:{}", feature.id),
                &feature.name,
                Some(json!(state.request.selected_features.contains(&feature.id))),
                vec![],
                None,
                enabled,
            ));
        }
        // Explicit node bindings are available for local and external inputs, including a later target body.
        let sources = part
            .features
            .iter()
            .filter(|f| {
                state.request.selected_features.contains(&f.id)
                    || state.request.include_dependencies.contains(&f.id)
                    || state.plan.as_ref().is_some_and(|plan| {
                        plan.items.iter().any(|item| {
                            item.kind == "feature" && item.source == f.id && item.state != "reuse"
                        })
                    })
            })
            .flat_map(|feature| part.feature_dependencies(feature))
            .collect::<std::collections::BTreeSet<_>>();
        for source in sources {
            let name = loaded
                .document
                .parts
                .get(&source.definition)
                .and_then(|p| p.features.iter().find(|f| f.id == source.node))
                .map(|f| f.name.clone())
                .unwrap_or_else(|| "External input".into());
            let mut choices = vec![
                json!({"value":Value::Null,"label":"Use corresponding input / include required dependency"}),
            ];
            for part in state.baseline.parts.values() {
                for feature in part.features.iter().filter(|f| !f.suppressed) {
                    choices.push(json!({"value":part.reference(&feature.id),"label":format!("{} / {}",part.name,feature.name)}));
                }
            }
            let value = state
                .request
                .bindings
                .iter()
                .find(|binding| binding.source == source)
                .map(|binding| json!(binding.target))
                .unwrap_or(Value::Null);
            let mut row = element(
                state,
                &format!("binding:{}:{}", source.definition, source.node),
                &format!("Input: {name}"),
                Some(value),
                choices,
                None,
                enabled,
            );
            row.data["source_node"] = json!(source);
            out.push(row);
        }
    }
    if let Some(plan) = &state.plan {
        for input in &plan.parameter_inputs {
            let mut choices =
                vec![json!({"value":Value::Null,"label":"Use corresponding target parameter"})];
            for (definition, name, parameters) in state
                .baseline
                .parts
                .values()
                .map(|part| (&part.id, &part.name, &part.parameters))
                .chain(
                    state
                        .baseline
                        .assemblies
                        .values()
                        .map(|assembly| (&assembly.id, &assembly.name, &assembly.parameters)),
                )
            {
                for (id, parameter) in parameters
                    .iter()
                    .filter(|(_, parameter)| parameter.dimension == input.dimension)
                {
                    choices.push(json!({"value":cad_core::ParameterRef {definition:definition.clone(),parameter:id.clone()},"label":format!("{name} / {}",parameter.name)}));
                }
            }
            let value = state
                .request
                .parameter_bindings
                .iter()
                .find(|binding| binding.source == input.source)
                .map(|binding| json!(binding.target))
                .unwrap_or(Value::Null);
            let mut row = element(
                state,
                &format!(
                    "parameter:{}:{}",
                    input.source.definition, input.source.parameter
                ),
                &format!("External parameter: {}", input.label),
                Some(value),
                choices,
                None,
                enabled,
            );
            row.data["source_parameter"] = json!(input.source);
            out.push(row);
        }
        for item in plan.items.iter().filter(|item| {
            item.kind == "feature"
                && !item.selected
                && (item.state == "reuse"
                    || state.request.include_dependencies.contains(&item.source))
        }) {
            out.push(element(
                state,
                &format!("dependency:{}", item.source),
                &format!("Also take changes to {}", item.label),
                Some(json!(
                    state.request.include_dependencies.contains(&item.source)
                )),
                vec![],
                None,
                enabled,
            ));
        }
        for conflict in &plan.conflicts {
            let value = state
                .request
                .resolutions
                .get(&conflict.key)
                .map(|r| json!(r))
                .unwrap_or(Value::Null);
            out.push(element(
                state,
                &format!("resolution:{}", conflict.key),
                &format!("Conflict: {}", conflict.label),
                Some(value),
                vec![
                    json!({"value":Value::Null,"label":"Choose a resolution"}),
                    json!({"value":"keep_target","label":"Keep target"}),
                    json!({"value":"use_source","label":"Use source"}),
                ],
                None,
                enabled,
            ));
        }
        for input in &plan.reference_inputs {
            if matches!(
                input.class,
                cad_core::reference_repair::Class::Topology(_)
                    | cad_core::reference_repair::Class::Plane
            ) {
                let owner = plan
                    .candidate
                    .as_ref()
                    .and_then(|d| d.parts.get(&input.owner.definition))
                    .and_then(|p| p.features.iter().find(|f| f.id == input.owner.node))
                    .map(|f| f.name.as_str())
                    .unwrap_or("Transferred feature");
                let mut row = element(
                    state,
                    &format!("pick:{}", input.semantic_id()),
                    &format!("{owner} / {}: use picked geometry", input.label),
                    None,
                    vec![],
                    Some("feature-transfer.bind-picked"),
                    enabled && w.selected.is_some(),
                );
                row.data["input"] = json!(input);
                out.push(row);
            }
        }
    }
    out.push(element(
        state,
        "outputs",
        "Show transferred results as part outputs",
        Some(json!(state.request.reconnect_outputs)),
        vec![],
        None,
        enabled,
    ));
    out.push(element(
        state,
        "parallel-outputs",
        "Allow separate results beside later target features",
        Some(json!(state.request.allow_parallel_outputs)),
        vec![],
        None,
        enabled,
    ));
    out.push(element(
        state,
        "preview",
        "Preview selected features",
        None,
        vec![],
        Some("feature-transfer.preview"),
        enabled && state.plan.as_ref().is_some_and(|p| p.candidate.is_some()),
    ));
    out
}

fn live(w: &Workspace, element: &SemanticElement) -> Result<SemanticElement> {
    semantics(w)
        .into_iter()
        .find(|current| {
            current.id == element.id && current.data["editor"] == element.data["editor"]
        })
        .ok_or_else(|| invalid("This transfer control is no longer current"))
}
pub fn current_value(w: &Workspace, element: &SemanticElement) -> Option<Value> {
    live(w, element).ok()?.data.get("json").cloned()
}
pub fn set_value(w: &mut Workspace, element: &SemanticElement, value: Value) -> Result<()> {
    let element = live(w, element)?;
    if element.data["editable"] != true {
        return Err(invalid("Wait for the transfer operation to finish"));
    }
    let key = element.data["key"]
        .as_str()
        .ok_or_else(|| invalid("Transfer field has no key"))?;
    let Some(mut state) = w.feature_transfer.take() else {
        return Err(invalid("The transfer editor is closed"));
    };
    let result = (|| -> Result<()> {
        invalidate(w, &mut state);
        match key {
            "source-path" | "revision" => {
                let text = value.as_str().ok_or_else(|| invalid("Enter text"))?;
                if text.len() > 4096 {
                    return Err(invalid("Source value is too long"));
                }
                if key == "source-path" {
                    state.source_path = text.into();
                } else {
                    state.revision = text.into();
                }
                state.loaded = None;
            }
            "source-part" | "target-part" => {
                let part = value.as_str().ok_or_else(|| invalid("Choose a part"))?;
                if key == "source-part" {
                    if !state
                        .loaded
                        .as_ref()
                        .is_some_and(|loaded| loaded.document.parts.contains_key(part))
                    {
                        return Err(invalid("Source part is missing"));
                    }
                    state.request.source_definition = part.into();
                } else {
                    if !state.baseline.parts.contains_key(part) {
                        return Err(invalid("Target part is missing"));
                    }
                    state.request.target_definition = part.into();
                }
                state.request.selected_features.clear();
                state.request.include_dependencies.clear();
                state.request.bindings.clear();
                state.request.parameter_bindings.clear();
                state.request.repairs.clear();
                state.request.resolutions.clear();
            }
            "outputs" => {
                state.request.reconnect_outputs = value
                    .as_bool()
                    .ok_or_else(|| invalid("Choose true or false"))?
            }
            "parallel-outputs" => {
                state.request.allow_parallel_outputs = value
                    .as_bool()
                    .ok_or_else(|| invalid("Choose true or false"))?
            }
            _ if key.starts_with("select:") || key.starts_with("dependency:") => {
                let (kind, id) = key
                    .split_once(':')
                    .ok_or_else(|| invalid("Feature identity is missing"))?;
                let selected = value
                    .as_bool()
                    .ok_or_else(|| invalid("Choose true or false"))?;
                let set = if kind == "select" {
                    &mut state.request.selected_features
                } else {
                    &mut state.request.include_dependencies
                };
                if selected {
                    set.insert(id.into());
                } else {
                    set.remove(id);
                }
            }
            _ if key.starts_with("resolution:") => {
                let key = key.trim_start_matches("resolution:");
                if value.is_null() {
                    state.request.resolutions.remove(key);
                } else {
                    state
                        .request
                        .resolutions
                        .insert(key.into(), serde_json::from_value::<Resolution>(value)?);
                }
            }
            _ if key.starts_with("binding:") => {
                let source: NodeRef = serde_json::from_value(element.data["source_node"].clone())?;
                state
                    .request
                    .bindings
                    .retain(|binding| binding.source != source);
                if !value.is_null() {
                    state.request.bindings.push(NodeBinding {
                        source,
                        target: serde_json::from_value(value)?,
                    });
                }
                state.request.repairs.clear();
            }
            _ if key.starts_with("parameter:") => {
                let source: cad_core::ParameterRef =
                    serde_json::from_value(element.data["source_parameter"].clone())?;
                state
                    .request
                    .parameter_bindings
                    .retain(|binding| binding.source != source);
                if !value.is_null() {
                    state.request.parameter_bindings.push(ParameterBinding {
                        source,
                        target: serde_json::from_value(value)?,
                    });
                }
            }
            _ => return Err(invalid("Unknown feature transfer field")),
        }
        rebuild(&mut state);
        Ok(())
    })();
    w.feature_transfer = Some(state);
    result
}
pub fn action(w: &mut Workspace, command: &str, data: Option<&Value>) -> bool {
    match command {
        "feature-transfer" | "feature-transfer.open" | "git.transfer-features" => open(w),
        "feature-transfer.load" => load(w, false),
        "feature-transfer.local" => load(w, true),
        "feature-transfer.preview" => preview(w),
        "feature-transfer.bind-picked" => {
            let result = (|| -> Result<()> {
                let data = data.ok_or_else(|| invalid("Choose the input to replace"))?;
                let row = semantics(w)
                    .into_iter()
                    .find(|row| row.data == *data)
                    .ok_or_else(|| {
                        invalid("The transfer inputs changed. Choose the input again.")
                    })?;
                if row.data["enabled"] != true {
                    return Err(invalid("Pick the required face or edge in the model first"));
                }
                let input: cad_core::reference_repair::Input =
                    serde_json::from_value(row.data["input"].clone())?;
                let selected = w
                    .selected
                    .as_ref()
                    .ok_or_else(|| invalid("Pick a face or edge in the model first"))?;
                let body = crate::reference_ui::selected_body(w, selected);
                let repair = cad_core::reference_repair::Repair {
                    input: input.clone(),
                    replacement: cad_core::reference_repair::Replacement::Topology {
                        reference: selected.reference.clone(),
                        body,
                    },
                };
                let Some(mut state) = w.feature_transfer.take() else {
                    return Err(invalid("The transfer editor is closed"));
                };
                invalidate(w, &mut state);
                state
                    .request
                    .repairs
                    .retain(|repair| repair.input.semantic_id() != input.semantic_id());
                state.request.repairs.push(repair);
                rebuild(&mut state);
                w.feature_transfer = Some(state);
                Ok(())
            })();
            if let Err(error) = result {
                w.status = error.to_string();
            }
        }
        _ => return false,
    }
    true
}
pub fn show(ui: &mut egui::Ui, w: &mut Workspace) {
    controls::label(
        ui,
        "feature-transfer.hint",
        "editor-hint",
        &[],
        "Take selected features from a branch or a related part. Review required inputs. Preview before Apply.",
        "ready",
    );
    for element in semantics(w) {
        if element.data.get("setter").is_some() {
            crate::ui::semantic_field(ui, w, &element);
        } else if let Some(command) = element.commands.first()
            && controls::button(
                ui,
                &element.id,
                &element.role,
                &["semantic-control"],
                &element.label,
                element.data["enabled"] == true,
                false,
            )
            .clicked()
        {
            action(w, command, Some(&element.data));
        }
    }
    if let Some(state) = &w.feature_transfer {
        if state.pending.is_some() {
            controls::label(
                ui,
                "feature-transfer.pending",
                "editor-hint",
                &[],
                "Reading source files…",
                "pending",
            );
            ui.ctx().request_repaint();
        }
        if let Some(error) = &state.error {
            controls::label(
                ui,
                "feature-transfer.error",
                "validation-message",
                &[],
                error,
                "error",
            );
        }
        if let Some(plan) = &state.plan {
            for item in &plan.items {
                controls::label(
                    ui,
                    &format!("feature-transfer.review:{}", item.key),
                    "feature-transfer-review",
                    &[],
                    &format!("{} · {} — {}", item.kind, item.label, item.message),
                    &item.state,
                );
            }
            for (index, diagnostic) in plan.diagnostics.iter().enumerate() {
                controls::label(
                    ui,
                    &format!("feature-transfer.diagnostic:{index}"),
                    "validation-message",
                    &[],
                    diagnostic,
                    "error",
                );
            }
            for conflict in &plan.conflicts {
                controls::label(
                    ui,
                    &format!("feature-transfer.conflict:{}", conflict.key),
                    "validation-message",
                    &[],
                    &format!("{}: {}", conflict.label, conflict.message),
                    if conflict.resolution.is_some() {
                        "resolved"
                    } else {
                        "error"
                    },
                );
            }
        }
    }
}
