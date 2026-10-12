//! Compact semantic authoring of saved engineering design monitors.
use crate::{Workspace, presentation_controls as controls};
use bevy_egui::egui;
use cad_core::{
    CadError, Id, Result,
    commands::Command,
    presentation::SemanticElement,
    sensors::{self, Definition, Limits, Metric, Target},
};
use serde_json::{Value, json};
const TOOL: &str = "sensor-tool";
const SELECTED: &str = "sensor-selected";
const TOKEN: &str = "sensor-editor-token";
const INVALID: &str = "sensor-invalid:";
const DOMAIN: &str = "sensor-domain-error:";

#[derive(Clone, PartialEq, Eq)]
struct Stamp {
    document: Id,
    generation: u64,
    revision: u64,
    draft: Option<(Id, u64)>,
    evaluation: Option<(cad_core::protocol::RequestIdentity, bool)>,
}
#[derive(Default)]
pub struct Cache {
    stamp: Option<Stamp>,
    report: Option<sensors::Report>,
    worker: Option<Worker>,
}
struct Job {
    stamp: Stamp,
    document: cad_core::Document,
    properties: Option<sensors::PropertySnapshot>,
}
#[derive(Default)]
struct Queue {
    stop: bool,
    next: Option<Job>,
}
struct Worker {
    queue: std::sync::Arc<(std::sync::Mutex<Queue>, std::sync::Condvar)>,
    replies: std::sync::mpsc::Receiver<(Stamp, sensors::Report)>,
}
impl Worker {
    fn new() -> Self {
        let queue = std::sync::Arc::new((
            std::sync::Mutex::new(Queue::default()),
            std::sync::Condvar::new(),
        ));
        let worker_queue = queue.clone();
        let (sender, replies) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            loop {
                let (mutex, changed) = &*worker_queue;
                let Ok(mut queue) = mutex.lock() else {
                    return;
                };
                while !queue.stop && queue.next.is_none() {
                    let Ok(next) = changed.wait(queue) else {
                        return;
                    };
                    queue = next;
                }
                if queue.stop {
                    return;
                }
                let Some(job) = queue.next.take() else {
                    continue;
                };
                drop(queue);
                let report = sensors::evaluate_snapshot(&job.document, job.properties.as_ref());
                if sender.send((job.stamp, report)).is_err() {
                    return;
                }
            }
        });
        Self { queue, replies }
    }
    fn submit(&self, job: Job) {
        let (mutex, changed) = &*self.queue;
        if let Ok(mut queue) = mutex.lock() {
            queue.next = Some(job);
            changed.notify_one();
        }
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        let (mutex, changed) = &*self.queue;
        if let Ok(mut queue) = mutex.lock() {
            queue.stop = true;
            queue.next = None;
            changed.notify_one();
        }
    }
}
fn evaluation(w: &Workspace) -> Option<&cad_core::protocol::Evaluation> {
    w.session
        .draft
        .as_ref()
        .and_then(|draft| draft.preview_evaluation())
        .or(w.session.evaluation.as_ref())
}
fn pending(document: &cad_core::Document) -> sensors::Report {
    let readings = document
        .sensors
        .values()
        .map(|sensor| {
            let unit = match &sensor.target {
                Target::Part { metric, .. } | Target::Occurrence { metric, .. } => metric.unit(),
                Target::Parameter {
                    definition,
                    parameter,
                    ..
                } => document
                    .parts
                    .get(definition)
                    .map(|part| &part.parameters)
                    .or_else(|| {
                        document
                            .assemblies
                            .get(definition)
                            .map(|assembly| &assembly.parameters)
                    })
                    .and_then(|parameters| parameters.get(parameter))
                    .map(|parameter| match parameter.dimension {
                        cad_core::Dimension::Scalar => "",
                        cad_core::Dimension::Length => "mm",
                        cad_core::Dimension::Angle => "rad",
                    })
                    .unwrap_or(""),
            };
            (
                sensor.id.clone(),
                sensors::Reading {
                    sensor: sensor.id.clone(),
                    status: if sensor.enabled {
                        sensors::Status::Unavailable
                    } else {
                        sensors::Status::Disabled
                    },
                    value: None,
                    unit: unit.into(),
                    representation: String::new(),
                    diagnostic: sensor
                        .enabled
                        .then(|| "Waiting for the current source monitor calculation".into()),
                },
            )
        })
        .collect();
    sensors::Report {
        version: sensors::CONTRACT_VERSION,
        source: String::new(),
        readings,
    }
}
fn report(w: &Workspace) -> sensors::Report {
    let document = w.session.visible();
    let evaluation = evaluation(w);
    let stamp = Stamp {
        document: document.id.clone(),
        generation: w.session.generation,
        revision: document.revision,
        draft: w
            .session
            .draft
            .as_ref()
            .map(|draft| (draft.id.clone(), draft.edit)),
        evaluation: evaluation.map(|evaluation| (evaluation.identity.clone(), evaluation.valid)),
    };
    let Ok(mut cache) = w.sensor_cache.lock() else {
        return pending(document);
    };
    if cache.stamp.as_ref() != Some(&stamp) {
        cache.report = None;
        cache.stamp = Some(stamp.clone());
        let worker = cache.worker.get_or_insert_with(Worker::new);
        worker.submit(Job {
            stamp,
            document: document.clone(),
            properties: evaluation.map(sensors::PropertySnapshot::capture),
        });
    }
    let replies = cache
        .worker
        .as_ref()
        .map(|worker| worker.replies.try_iter().collect::<Vec<_>>())
        .unwrap_or_default();
    for (stamp, report) in replies {
        if cache.stamp.as_ref() == Some(&stamp) {
            cache.report = Some(report);
        }
    }
    cache.report.clone().unwrap_or_else(|| pending(document))
}
fn editing(w: &Workspace) -> bool {
    w.fields.contains_key(TOOL)
}
fn available(w: &Workspace) -> bool {
    w.settings.is_none()
        && w.sketch.is_none()
        && (editing(w) || w.tool_checkpoint.is_none() && w.session.draft.is_none())
}
fn selected(w: &Workspace) -> Option<Id> {
    w.fields.get(SELECTED).cloned()
}
fn source(w: &Workspace) -> Result<Definition> {
    let source: Definition = serde_json::from_str(&w.json)?;
    if selected(w).as_deref() != Some(&source.id) {
        return Err(CadError::Invalid(
            "The selected monitor identity cannot be changed".into(),
        ));
    }
    Ok(source)
}
pub fn current_source_is_valid(w: &Workspace) -> bool {
    if !editing(w) {
        return true;
    }
    !w.fields.keys().any(|key| key.starts_with(INVALID))
        && selected(w).is_none_or(|id| {
            source(w).is_ok_and(|source| w.session.visible().sensors.get(&id) == Some(&source))
        })
}
fn load(w: &mut Workspace, id: Option<Id>) {
    w.fields.retain(|key, _| !key.starts_with(INVALID));
    w.fields.retain(|key, _| !key.starts_with(DOMAIN));
    w.fields.insert(TOKEN.into(), cad_core::id());
    if let Some(id) = id {
        w.json = w
            .session
            .visible()
            .sensors
            .get(&id)
            .and_then(|source| serde_json::to_string_pretty(source).ok())
            .unwrap_or_default();
        w.fields.insert(SELECTED.into(), id);
    } else {
        w.fields.remove(SELECTED);
        w.json.clear();
    }
}
pub fn begin(w: &mut Workspace) {
    if !available(w) {
        w.status = "Apply or cancel the current operation before editing design monitors".into();
        return;
    }
    if editing(w) {
        w.open_editor("sensors");
        return;
    }
    w.start_tool();
    w.fields.insert(TOOL.into(), "true".into());
    let selected = selected(w)
        .filter(|id| w.session.visible().sensors.contains_key(id))
        .or_else(|| w.session.visible().sensors.keys().next().cloned());
    load(w, selected);
    w.open_editor("sensors");
}
pub fn cancel(w: &mut Workspace) {
    for key in [TOOL, SELECTED, TOKEN] {
        w.fields.remove(key);
    }
    w.fields.retain(|key, _| !key.starts_with(INVALID));
    w.fields.retain(|key, _| !key.starts_with(DOMAIN));
}
fn submit(w: &mut Workspace, source: Definition) -> Result<()> {
    w.json = serde_json::to_string_pretty(&source)?;
    w.dispatch(Command::PutSensor {
        sensor: Box::new(source.clone()),
    });
    if w.session.visible().sensors.get(&source.id) != Some(&source) {
        return Err(CadError::Invalid(w.status.clone()));
    }
    w.fields.retain(|key, _| !key.starts_with(DOMAIN));
    Ok(())
}
fn owners(w: &Workspace) -> Vec<(Id, String)> {
    w.session
        .visible()
        .parts
        .values()
        .map(|part| (part.id.clone(), part.name.clone()))
        .chain(
            w.session
                .visible()
                .assemblies
                .values()
                .map(|assembly| (assembly.id.clone(), assembly.name.clone())),
        )
        .collect()
}
fn default_target(w: &Workspace, kind: &str) -> Result<Target> {
    let doc = w.session.visible();
    match kind {
        "parameter" => {
            let values = doc
                .parts
                .values()
                .map(|part| (&part.id, &part.parameters))
                .chain(
                    doc.assemblies
                        .values()
                        .map(|assembly| (&assembly.id, &assembly.parameters)),
                );
            let (owner, parameter) = values
                .flat_map(|(owner, parameters)| {
                    parameters.keys().map(move |parameter| (owner, parameter))
                })
                .next()
                .ok_or_else(|| {
                    CadError::Invalid(
                        "Create a named parameter before adding a parameter monitor".into(),
                    )
                })?;
            Ok(Target::Parameter {
                definition: owner.clone(),
                parameter: parameter.clone(),
                configuration: None,
            })
        }
        "part" => {
            let part = w
                .view
                .active_part
                .as_ref()
                .and_then(|id| doc.parts.get(id))
                .or_else(|| doc.parts.values().next())
                .ok_or_else(|| {
                    CadError::Invalid("Create a part before adding a part monitor".into())
                })?;
            Ok(Target::Part {
                definition: part.id.clone(),
                configuration: None,
                metric: Metric::Mass,
            })
        }
        "occurrence" => Ok(Target::Occurrence {
            path: vec![],
            metric: Metric::Mass,
        }),
        _ => Err(CadError::Invalid("Unknown monitor target type".into())),
    }
}
fn new(w: &mut Workspace) -> Result<()> {
    let target = default_target(w, "part").or_else(|_| default_target(w, "occurrence"))?;
    let names = w
        .session
        .visible()
        .sensors
        .values()
        .map(|sensor| sensor.name.to_lowercase())
        .collect::<std::collections::BTreeSet<_>>();
    let name = (1..)
        .map(|index| format!("Design Monitor {index}"))
        .find(|name| !names.contains(&name.to_lowercase()))
        .unwrap();
    let source = Definition {
        version: sensors::CONTRACT_VERSION,
        id: cad_core::id(),
        name,
        target,
        limits: Limits {
            lower: None,
            upper: Some(1.),
        },
        enabled: true,
        notes: String::new(),
    };
    let id = source.id.clone();
    w.fields.insert(SELECTED.into(), id.clone());
    submit(w, source)?;
    load(w, Some(id));
    Ok(())
}
pub fn action(w: &mut Workspace, command: &str, args: Option<&Value>) -> bool {
    if command == "inspection.sensors" {
        begin(w);
        return true;
    }
    if !command.starts_with("sensor.") {
        return false;
    }
    let result = (|| -> Result<()> {
        if !available(w) {
            return Err(CadError::Invalid(
                "Apply or cancel the current operation first".into(),
            ));
        }
        if !editing(w) {
            begin(w);
        }
        if !current_source_is_valid(w) && !matches!(command, "sensor.export-report") {
            return Err(CadError::Invalid(
                "Correct the current monitor fields before selecting another monitor".into(),
            ));
        }
        match command {
            "sensor.new" => new(w)?,
            "sensor.edit" => {
                let id = args
                    .and_then(|args| args["sensor"].as_str())
                    .map(str::to_owned)
                    .or_else(|| selected(w))
                    .ok_or_else(|| CadError::Invalid("Choose a monitor".into()))?;
                if !w.session.visible().sensors.contains_key(&id) {
                    return Err(CadError::Invalid("The monitor no longer exists".into()));
                }
                load(w, Some(id));
            }
            "sensor.delete" => {
                let id = selected(w).ok_or_else(|| CadError::Invalid("Choose a monitor".into()))?;
                w.dispatch(Command::DeleteSensor { sensor: id.clone() });
                if w.session.visible().sensors.contains_key(&id) {
                    return Err(CadError::Invalid(w.status.clone()));
                }
                load(w, None);
            }
            "sensor.export-report" => {
                let report = report(w);
                if report.source.is_empty() {
                    return Err(CadError::Invalid(
                        "Wait for the current monitor calculation before exporting readings".into(),
                    ));
                }
                if let Some(path) = rfd::FileDialog::new()
                    .set_title("Export current design monitor readings")
                    .add_filter("JSON report", &["json"])
                    .set_file_name("design-monitors.json")
                    .save_file()
                {
                    cad_core::persistence::atomic_write(
                        &path,
                        &serde_json::to_vec_pretty(&report)?,
                    )?;
                    w.status = format!("Saved monitor report {}", path.display());
                }
            }
            _ => return Err(CadError::Invalid("Unknown design monitor command".into())),
        }
        Ok(())
    })();
    if let Err(error) = result {
        w.status = error.to_string();
    }
    true
}
fn field(
    w: &Workspace,
    source: &Definition,
    pointer: &str,
    label: &str,
    value: Value,
    choices: Value,
) -> SemanticElement {
    SemanticElement {
        id: format!("sensor-field:{}:{}", source.id, pointer.replace('/', ":")),
        role: "design-monitor-field".into(),
        label: label.into(),
        value: Some(
            value
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| value.to_string()),
        ),
        state: if current_source_is_valid(w) {
            "draft"
        } else {
            "invalid"
        }
        .into(),
        commands: vec![],
        relationships: vec![format!("sensor:{}", source.id)],
        data: json!({"contract":"tcad.design-monitor-field","version":1,"setter":"sensor-editor","sensor":source.id,"editor":w.fields.get(TOKEN),"pointer":pointer,"json":value,"editable":true,"choices":choices,"value_type":if choices.as_array().is_some_and(|choices|!choices.is_empty()){ "choice" }else if value.is_boolean(){"boolean"}else if value.is_number(){"number"}else{"text"}}),
    }
}
fn fields(w: &Workspace) -> Vec<SemanticElement> {
    let Ok(source) = source(w) else {
        return vec![];
    };
    let mut out = vec![
        field(w, &source, "/name", "Name", json!(source.name), json!([])),
        field(
            w,
            &source,
            "/enabled",
            "Enabled",
            json!(source.enabled),
            json!([]),
        ),
    ];
    let kind = match source.target {
        Target::Parameter { .. } => "parameter",
        Target::Part { .. } => "part",
        Target::Occurrence { .. } => "occurrence",
    };
    out.push(field(w,&source,"target-kind","Monitor",json!(kind),json!([{"value":"part","label":"Part properties"},{"value":"occurrence","label":"Assembly / occurrence properties"},{"value":"parameter","label":"Named parameter"}])));
    match &source.target {
        Target::Parameter {
            definition,
            parameter: _,
            configuration,
        }
        | Target::Part {
            definition,
            configuration,
            metric: _,
        } => {
            let choices = owners(w)
                .into_iter()
                .filter(|(id, _)| kind == "parameter" || w.session.visible().parts.contains_key(id))
                .map(|(id, name)| json!({"value":id,"label":name}))
                .collect::<Vec<_>>();
            out.push(field(
                w,
                &source,
                "/target/definition",
                "Definition",
                json!(definition),
                json!(choices),
            ));
            let mut configurations = vec![json!({"value":null,"label":"Active configuration"})];
            configurations.extend(
                cad_core::configuration::names(w.session.visible(), definition)
                    .into_iter()
                    .map(|(id, name)| json!({"value":id,"label":name})),
            );
            out.push(field(
                w,
                &source,
                "/target/configuration",
                "Configuration",
                json!(configuration),
                json!(configurations),
            ));
            if let Target::Parameter { parameter, .. } = &source.target {
                let parameters = w
                    .session
                    .visible()
                    .parts
                    .get(definition)
                    .map(|part| &part.parameters)
                    .or_else(|| {
                        w.session
                            .visible()
                            .assemblies
                            .get(definition)
                            .map(|assembly| &assembly.parameters)
                    });
                let choices = parameters
                    .into_iter()
                    .flat_map(|parameters| parameters.iter())
                    .map(|(id, parameter)| json!({"value":id,"label":parameter.name}))
                    .collect::<Vec<_>>();
                out.push(field(
                    w,
                    &source,
                    "/target/parameter",
                    "Parameter",
                    json!(parameter),
                    json!(choices),
                ));
            }
        }
        Target::Occurrence { path, .. } => {
            let mut choices = vec![json!({"value":[],"label":"Complete model assembly"})];
            let mut paths = std::collections::BTreeSet::new();
            if let Ok(parts) = w.session.visible().placed_parts() {
                for part in parts {
                    for count in 1..=part.path.len() {
                        let prefix = part.path[..count].to_vec();
                        if paths.insert(prefix.clone()) {
                            let label = if count == part.path.len() {
                                part.name.clone()
                            } else {
                                format!("Subassembly path · {} levels", count)
                            };
                            choices.push(json!({"value":prefix,"label":label}));
                        }
                    }
                }
            }
            out.push(field(
                w,
                &source,
                "/target/path",
                "Occurrence",
                json!(path),
                json!(choices),
            ));
        }
    }
    if let Target::Part { metric, .. } | Target::Occurrence { metric, .. } = source.target {
        out.push(field(w,&source,"/target/metric","Quantity",json!(metric),json!(Metric::ALL.map(|metric|json!({"value":metric,"label":format!("{} ({})",metric.label(),metric.unit())})))));
    }
    let unit = if w
        .session
        .visible()
        .sensors
        .get(&source.id)
        .is_some_and(|saved| saved.target == source.target)
    {
        report(w)
            .readings
            .get(&source.id)
            .map(|reading| reading.unit.clone())
            .unwrap_or_default()
    } else {
        sensors::unit(w.session.visible(), &source.target).unwrap_or_default()
    };
    for (name, value) in [
        ("lower", source.limits.lower),
        ("upper", source.limits.upper),
    ] {
        out.push(field(
            w,
            &source,
            &format!("limit-enabled:{name}"),
            &format!("Use {name} limit"),
            json!(value.is_some()),
            json!([]),
        ));
        if let Some(value) = value {
            out.push(field(
                w,
                &source,
                &format!("/limits/{name}"),
                &format!(
                    "{} limit ({unit})",
                    if name == "lower" { "Lower" } else { "Upper" }
                ),
                json!(value),
                json!([]),
            ));
        }
    }
    out.push(field(
        w,
        &source,
        "/notes",
        "Notes",
        json!(source.notes),
        json!([]),
    ));
    out
}
pub fn semantics(w: &Workspace) -> Vec<SemanticElement> {
    if !editing(w) && w.session.visible().sensors.is_empty() {
        return vec![];
    }
    let current = report(w);
    let mut out = sensors::semantic(w.session.visible(), &current);
    if current.source.is_empty() {
        for element in &mut out {
            element.state = "pending".into();
            element.data["source_fingerprint"] = Value::Null;
            element.data["report_current"] = json!(false);
        }
    }
    for element in &mut out {
        let enabled = available(w) && current_source_is_valid(w);
        let reason = if !available(w) {
            "Apply or cancel the current operation before editing a monitor"
        } else {
            "Correct retained monitor fields before selecting another monitor"
        };
        element.data["actions"] = json!([
            {"command":"inspection.sensors","label":"Open design monitors","enabled":available(w),
                "disabled_reason":(!available(w)).then_some(reason)},
            {"command":"sensor.edit","label":"Edit monitor","enabled":enabled,
                "disabled_reason":(!enabled).then_some(reason),"arguments":{"sensor":element.data["sensor"]}}
        ]);
    }
    if editing(w) {
        out.extend(fields(w));
        out.push(SemanticElement{id:"design-monitors:source".into(),role:"design-monitor-source".into(),label:"Advanced source".into(),value:Some(w.json.clone()),state:if current_source_is_valid(w){"draft"}else{"invalid"}.into(),commands:vec![],relationships:vec![],data:json!({"setter":"sensor-editor","pointer":"source","editor":w.fields.get(TOKEN),"json":w.json,"editable":selected(w).is_some(),"value_type":"text","multiline":true,"rows":12})});
    }
    for (command, label) in [
        ("sensor.new", "New monitor"),
        ("sensor.delete", "Delete monitor"),
        ("sensor.export-report", "Export readings…"),
    ] {
        let enabled = available(w)
            && (command != "sensor.delete" || selected(w).is_some())
            && (command != "sensor.export-report" || !current.source.is_empty())
            && (command == "sensor.export-report" || current_source_is_valid(w));
        let reason = if !available(w) {
            "Apply or cancel the current operation before using design monitors"
        } else if command == "sensor.delete" && selected(w).is_none() {
            "Select a monitor to delete"
        } else if command == "sensor.export-report" && current.source.is_empty() {
            "Wait for the current monitor report before exporting"
        } else {
            "Correct retained monitor fields before changing its definition"
        };
        out.push(SemanticElement{id:format!("command:{command}"),role:"design-monitor-action".into(),label:label.into(),value:(!enabled).then(||reason.into()),state:if enabled{"ready"}else{"disabled"}.into(),commands:vec![command.into()],relationships:vec![],data:json!({"enabled":enabled,"sensor":selected(w),"actions":[{"command":command,"label":label,"enabled":enabled,"disabled_reason":(!enabled).then_some(reason),"arguments":{"sensor":selected(w)}}]})});
    }
    out
}
pub fn current_value(w: &Workspace, element: &SemanticElement) -> Option<Value> {
    let pointer = element.data["pointer"].as_str()?;
    if pointer == "source" {
        return Some(json!(w.json));
    }
    if pointer == "target-kind" {
        return element.data.get("json").cloned();
    }
    if let Some(name) = pointer.strip_prefix("limit-enabled:") {
        let source = source(w).ok()?;
        return Some(json!(if name == "lower" {
            source.limits.lower.is_some()
        } else {
            source.limits.upper.is_some()
        }));
    }
    serde_json::to_value(source(w).ok()?)
        .ok()?
        .pointer(pointer)
        .cloned()
}
pub fn set_value(w: &mut Workspace, element: &SemanticElement, value: Value) -> Result<()> {
    if !editing(w) || element.data["editor"] != json!(w.fields.get(TOKEN)) {
        return Err(CadError::Invalid(
            "This monitor control belongs to an earlier editor".into(),
        ));
    }
    let pointer = element.data["pointer"]
        .as_str()
        .ok_or_else(|| CadError::Invalid("Missing monitor field pointer".into()))?;
    if pointer == "source" {
        w.json = value
            .as_str()
            .ok_or_else(|| CadError::Invalid("Monitor source requires text".into()))?
            .into();
        submit(w, source(w)?)?;
        w.fields.retain(|key, _| !key.starts_with(INVALID));
        w.fields.insert(TOKEN.into(), cad_core::id());
        return Ok(());
    }
    let mut source = source(w)?;
    if pointer == "target-kind" {
        source.target = default_target(w, value.as_str().unwrap_or_default())?;
    } else if let Some(name) = pointer.strip_prefix("limit-enabled:") {
        let enabled = value
            .as_bool()
            .ok_or_else(|| CadError::Invalid("A limit toggle requires a boolean".into()))?;
        let target = if name == "lower" {
            &mut source.limits.lower
        } else if name == "upper" {
            &mut source.limits.upper
        } else {
            return Err(CadError::Invalid("Unknown limit".into()));
        };
        *target = if enabled {
            Some(target.unwrap_or(0.))
        } else {
            None
        };
    } else {
        let mut json = serde_json::to_value(&source)?;
        *json
            .pointer_mut(pointer)
            .ok_or_else(|| CadError::Invalid("The monitor field no longer exists".into()))? = value;
        source = serde_json::from_value(json)?;
        if pointer == "/target/definition" {
            if let Target::Parameter {
                definition,
                parameter,
                configuration,
            } = &mut source.target
            {
                let parameters = w
                    .session
                    .visible()
                    .parts
                    .get(definition)
                    .map(|part| &part.parameters)
                    .or_else(|| {
                        w.session
                            .visible()
                            .assemblies
                            .get(definition)
                            .map(|assembly| &assembly.parameters)
                    });
                *parameter = parameters
                    .and_then(|parameters| parameters.keys().next())
                    .cloned()
                    .ok_or_else(|| {
                        CadError::Invalid("This definition has no named parameters".into())
                    })?;
                *configuration = None;
            } else if let Target::Part { configuration, .. } = &mut source.target {
                *configuration = None;
            }
        }
    }
    submit(w, source)
}
pub fn field_error(w: &mut Workspace, element: &SemanticElement, error: Option<String>) {
    if element.data["setter"] != "sensor-editor"
        || element.data["editor"] != json!(w.fields.get(TOKEN))
    {
        return;
    }
    let key = format!("{INVALID}{}", element.id);
    let domain = format!("{DOMAIN}{}", element.id);
    if let Some(error) = error {
        w.confirm_requested = false;
        let source_invalid = selected(w).is_some_and(|id| {
            !source(w).is_ok_and(|source| w.session.visible().sensors.get(&id) == Some(&source))
        });
        if source_invalid {
            w.fields.insert(domain, error);
        } else {
            w.fields.insert(key, error);
        }
    } else {
        w.fields.remove(&key);
        w.fields.remove(&domain);
    }
}
pub fn field_is_invalid(w: &Workspace, element: &SemanticElement) -> bool {
    element.data["setter"] == "sensor-editor"
        && (w.fields.contains_key(&format!("{INVALID}{}", element.id))
            || w.fields.contains_key(&format!("{DOMAIN}{}", element.id)))
}
pub fn show(ui: &mut egui::Ui, w: &mut Workspace) {
    ui.small("Current source · mechanical placements · visibility and exploded display do not alter readings.");
    let elements = semantics(w);
    for element in elements
        .iter()
        .filter(|element| element.role == "design-monitor-action")
    {
        if controls::button(
            ui,
            &element.id,
            &element.role,
            &["compact"],
            &element.label,
            element.data["enabled"].as_bool().unwrap_or(false),
            false,
        )
        .clicked()
        {
            action(w, &element.commands[0], Some(&element.data));
        }
    }
    let report = report(w);
    for sensor in w
        .session
        .visible()
        .sensors
        .values()
        .cloned()
        .collect::<Vec<_>>()
    {
        let reading = report.readings.get(&sensor.id);
        let value = reading
            .and_then(|reading| reading.value)
            .map(|value| {
                format!(
                    "{value:.4} {}",
                    reading.map(|reading| reading.unit.as_str()).unwrap_or("")
                )
            })
            .unwrap_or_else(|| "Unavailable".into());
        let label = format!("{} · {value}", sensor.name);
        if controls::button(
            ui,
            &format!("sensor:{}", sensor.id),
            "design-monitor-entry",
            &[],
            &label,
            current_source_is_valid(w),
            selected(w).as_deref() == Some(&sensor.id),
        )
        .clicked()
        {
            load(w, Some(sensor.id));
        }
    }
    if let Some(id) = selected(w)
        && let Some(reading) = report.readings.get(&id)
    {
        ui.label(reading.status.label());
        if let Some(diagnostic) = &reading.diagnostic {
            ui.label(diagnostic);
        }
        ui.small(&reading.representation);
    }
    for element in fields(w) {
        crate::ui::semantic_field(ui, w, &element);
    }
    ui.collapsing("Advanced source", |ui| {
        if let Some(element) = semantics(w)
            .into_iter()
            .find(|element| element.role == "design-monitor-source")
        {
            crate::ui::semantic_field(ui, w, &element);
        }
    });
    for error in w
        .fields
        .iter()
        .filter(|(key, _)| key.starts_with(INVALID) || key.starts_with(DOMAIN))
        .map(|(_, error)| error)
    {
        ui.colored_label(ui.visuals().error_fg_color, error);
    }
    ui.small("Mass, volume, area and bounds describe the current boundary tessellation. Limits are inclusive in the displayed canonical units; they are design alerts and do not block modeling.");
}
