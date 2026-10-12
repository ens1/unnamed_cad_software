//! Drafted linear tolerance-stack authoring and coalesced source arithmetic.
use crate::{Workspace, presentation_controls as controls};
use bevy_egui::egui;
use cad_core::{
    CadError, Dimension, Id, Result,
    commands::Command,
    presentation::SemanticElement,
    tolerance_stacks::{
        self as stacks, Definition, Distribution, Nominal, Report, Scope, Status, Term,
    },
};
use serde_json::{Value, json};
const TOOL: &str = "tolerance-stack-tool";
const SELECTED: &str = "tolerance-stack-selected";
const TERM: &str = "tolerance-stack-term";
const TOKEN: &str = "tolerance-stack-editor-token";
const INVALID: &str = "tolerance-stack-invalid:";
const DOMAIN: &str = "tolerance-stack-domain:";

#[derive(Clone, PartialEq, Eq)]
struct Stamp {
    document: Id,
    generation: u64,
    revision: u64,
    draft: Option<(Id, u64)>,
}
#[derive(Default)]
pub struct Cache {
    stamp: Option<Stamp>,
    report: Option<Report>,
    worker: Option<Worker>,
    error: Option<String>,
}
struct Job {
    stamp: Stamp,
    document: cad_core::Document,
}
#[derive(Default)]
struct Queue {
    stop: bool,
    next: Option<Job>,
    reply: Option<(Stamp, Report)>,
}
struct Worker {
    queue: std::sync::Arc<(std::sync::Mutex<Queue>, std::sync::Condvar)>,
}
impl Worker {
    fn new() -> std::result::Result<Self, String> {
        let queue = std::sync::Arc::new((
            std::sync::Mutex::new(Queue::default()),
            std::sync::Condvar::new(),
        ));
        let work = queue.clone();
        std::thread::Builder::new()
            .name("tcad-tolerance-stacks".into())
            .spawn(move || {
                loop {
                    let (mutex, changed) = &*work;
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
                    let report = std::panic::catch_unwind(|| stacks::evaluate(&job.document))
                        .unwrap_or_else(|_| {
                            unavailable(
                                &job.document,
                                "Tolerance calculation stopped unexpectedly",
                                true,
                            )
                        });
                    let Ok(mut queue) = mutex.lock() else {
                        return;
                    };
                    if queue.stop {
                        return;
                    }
                    queue.reply = Some((job.stamp, report));
                }
            })
            .map_err(|error| format!("Could not start tolerance calculation: {error}"))?;
        Ok(Self { queue })
    }
    fn submit(&self, job: Job) {
        let (mutex, changed) = &*self.queue;
        if let Ok(mut queue) = mutex.lock() {
            queue.next = Some(job);
            changed.notify_one();
        }
    }
    fn reply(&self) -> Option<(Stamp, Report)> {
        self.queue.0.lock().ok()?.reply.take()
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        let (mutex, changed) = &*self.queue;
        if let Ok(mut queue) = mutex.lock() {
            queue.stop = true;
            queue.next = None;
            queue.reply = None;
            changed.notify_one();
        }
    }
}
fn unavailable(document: &cad_core::Document, message: &str, complete: bool) -> Report {
    Report {
        version: stacks::CONTRACT_VERSION,
        source: if complete {
            document.fingerprint()
        } else {
            String::new()
        },
        stacks: document
            .tolerance_stacks
            .values()
            .map(|stack| {
                (
                    stack.id.clone(),
                    stacks::Reading {
                        stack: stack.id.clone(),
                        status: Status::Unavailable,
                        nominal_mm: None,
                        worst_case_mm: None,
                        requirements_mm: [None, None],
                        statistics: None,
                        contributions: vec![],
                        diagnostics: vec![message.into()],
                    },
                )
            })
            .collect(),
    }
}
fn report(w: &Workspace) -> Report {
    let document = w.session.visible();
    let stamp = Stamp {
        document: document.id.clone(),
        generation: w.session.generation,
        revision: document.revision,
        draft: w
            .session
            .draft
            .as_ref()
            .map(|draft| (draft.id.clone(), draft.edit)),
    };
    let Ok(mut cache) = w.tolerance_stack_cache.lock() else {
        return unavailable(document, "Tolerance report cache is unavailable", false);
    };
    if cache.stamp.as_ref() != Some(&stamp) {
        cache.stamp = Some(stamp.clone());
        cache.report = None;
        if cache.worker.is_none() && cache.error.is_none() {
            match Worker::new() {
                Ok(worker) => cache.worker = Some(worker),
                Err(error) => cache.error = Some(error),
            }
        }
        if let Some(worker) = &cache.worker {
            worker.submit(Job {
                stamp,
                document: document.clone(),
            });
        }
    }
    if let Some((stamp, report)) = cache.worker.as_ref().and_then(Worker::reply)
        && cache.stamp.as_ref() == Some(&stamp)
    {
        cache.report = Some(report);
    }
    cache.report.clone().unwrap_or_else(|| {
        unavailable(
            document,
            cache
                .error
                .as_deref()
                .unwrap_or("Waiting for the current source tolerance calculation"),
            false,
        )
    })
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
        return Err(invalid("The selected stack identity cannot be changed"));
    }
    Ok(source)
}
fn invalid(message: &str) -> CadError {
    CadError::Invalid(format!("Tolerance stack: {message}"))
}
fn fields_are_valid(w: &Workspace) -> bool {
    !w.fields.keys().any(|key| key.starts_with(INVALID))
        && selected(w).is_none_or(|id| {
            source(w)
                .is_ok_and(|source| w.session.visible().tolerance_stacks.get(&id) == Some(&source))
        })
}
pub fn current_source_is_valid(w: &Workspace) -> bool {
    if !editing(w) {
        return true;
    }
    if !fields_are_valid(w) {
        return false;
    }
    let Some(id) = selected(w) else {
        return true;
    };
    let current = report(w);
    !current.source.is_empty()
        && current
            .stacks
            .get(&id)
            .is_some_and(|reading| reading.status != Status::Unavailable)
}
fn load(w: &mut Workspace, id: Option<Id>) {
    w.fields
        .retain(|key, _| !key.starts_with(INVALID) && !key.starts_with(DOMAIN));
    w.fields.insert(TOKEN.into(), cad_core::id());
    if let Some(id) = id {
        w.json = w
            .session
            .visible()
            .tolerance_stacks
            .get(&id)
            .and_then(|source| serde_json::to_string_pretty(source).ok())
            .unwrap_or_default();
        let term = w
            .session
            .visible()
            .tolerance_stacks
            .get(&id)
            .and_then(|stack| stack.terms.first())
            .map(|term| term.id.clone());
        w.fields.insert(SELECTED.into(), id);
        if let Some(term) = term {
            w.fields.insert(TERM.into(), term);
        } else {
            w.fields.remove(TERM);
        }
    } else {
        w.fields.remove(SELECTED);
        w.fields.remove(TERM);
        w.json.clear();
    }
}
pub fn begin(w: &mut Workspace) {
    if !available(w) {
        w.status = "Apply or cancel the current operation before editing tolerance stacks".into();
        return;
    }
    if editing(w) {
        w.open_editor("tolerance-stacks");
        return;
    }
    w.start_tool();
    w.fields.insert(TOOL.into(), "true".into());
    let id = selected(w)
        .filter(|id| w.session.visible().tolerance_stacks.contains_key(id))
        .or_else(|| w.session.visible().tolerance_stacks.keys().next().cloned());
    load(w, id);
    w.open_editor("tolerance-stacks");
}
pub fn cancel(w: &mut Workspace) {
    for key in [TOOL, SELECTED, TERM, TOKEN] {
        w.fields.remove(key);
    }
    w.fields
        .retain(|key, _| !key.starts_with(INVALID) && !key.starts_with(DOMAIN));
}
fn submit(w: &mut Workspace, source: Definition) -> Result<()> {
    w.json = serde_json::to_string_pretty(&source)?;
    w.dispatch(Command::PutToleranceStack {
        stack: Box::new(source.clone()),
    });
    if w.session.visible().tolerance_stacks.get(&source.id) != Some(&source) {
        return Err(CadError::Invalid(w.status.clone()));
    }
    w.fields.retain(|key, _| !key.starts_with(DOMAIN));
    Ok(())
}
fn owners(w: &Workspace) -> Vec<Value> {
    w.session
        .visible()
        .parts
        .values()
        .map(|part| json!({"value":part.id,"label":part.name}))
        .chain(
            w.session
                .visible()
                .assemblies
                .values()
                .map(|assembly| json!({"value":assembly.id,"label":assembly.name})),
        )
        .collect()
}
fn configurations(w: &Workspace, definition: &str) -> Vec<Value> {
    std::iter::once(json!({"value":null,"label":"Active configuration"}))
        .chain(
            cad_core::configuration::names(w.session.visible(), definition)
                .into_iter()
                .map(|(id, name)| json!({"value":id,"label":name})),
        )
        .collect()
}
fn default_scope(w: &Workspace) -> Result<Scope> {
    let doc = w.session.visible();
    let definition = w
        .view
        .active_part
        .as_ref()
        .filter(|id| doc.parts.contains_key(*id))
        .cloned()
        .or_else(|| {
            (doc.parts.contains_key(&doc.root) || doc.assemblies.contains_key(&doc.root))
                .then(|| doc.root.clone())
        })
        .or_else(|| doc.parts.keys().next().cloned())
        .or_else(|| doc.assemblies.keys().next().cloned())
        .ok_or_else(|| invalid("Create a part or assembly before adding a tolerance stack"))?;
    Ok(Scope {
        definition,
        configuration: None,
    })
}
fn new_term(scope: Scope) -> Term {
    Term {
        id: cad_core::id(),
        name: "Dimension".into(),
        nominal: Nominal::Expression {
            scope,
            value: "10 mm".into(),
        },
        coefficient: 1.,
        lower_deviation: "-0.1 mm".into(),
        upper_deviation: "0.1 mm".into(),
        distribution: Distribution::Unspecified,
        variation_group: None,
        enabled: true,
    }
}
fn unique_name(w: &Workspace, prefix: &str) -> String {
    let prefix = if prefix.len() > 220 {
        "Tolerance Stack Copy"
    } else {
        prefix
    };
    let names = w
        .session
        .visible()
        .tolerance_stacks
        .values()
        .map(|stack| stack.name.to_lowercase())
        .collect::<std::collections::BTreeSet<_>>();
    (1..)
        .map(|index| format!("{prefix} {index}"))
        .find(|name| !names.contains(&name.to_lowercase()))
        .unwrap()
}
fn new(w: &mut Workspace) -> Result<()> {
    if w.session.visible().tolerance_stacks.len() >= stacks::MAX_STACKS {
        return Err(invalid("A document supports at most 128 tolerance stacks"));
    }
    let scope = default_scope(w)?;
    let source = Definition {
        version: stacks::CONTRACT_VERSION,
        id: cad_core::id(),
        name: unique_name(w, "Tolerance Stack"),
        scope: scope.clone(),
        terms: vec![new_term(scope)],
        lower_requirement: None,
        upper_requirement: None,
        assume_independent_groups: false,
        sigma_multiplier: 3.,
        notes: String::new(),
    };
    let id = source.id.clone();
    w.fields.insert(SELECTED.into(), id.clone());
    submit(w, source)?;
    load(w, Some(id));
    Ok(())
}
fn command(w: &Workspace, key: &str, label: &str, enabled: bool, args: Value) -> SemanticElement {
    let enabled = enabled && available(w);
    let mut data = args;
    data["enabled"] = json!(enabled);
    data["actions"] =
        json!([{"command":key,"label":label,"enabled":enabled,"arguments":data.clone()}]);
    SemanticElement {
        id: format!(
            "command:{key}:{}:{}",
            data["stack"].as_str().unwrap_or(""),
            data["term"].as_str().unwrap_or("")
        ),
        role: "tolerance-stack-action".into(),
        label: label.into(),
        value: None,
        state: if enabled { "available" } else { "disabled" }.into(),
        commands: vec![key.into()],
        relationships: vec![],
        data,
    }
}
pub fn action(w: &mut Workspace, key: &str, args: Option<&Value>) -> bool {
    if key == "inspection.tolerance-stacks" {
        begin(w);
        return true;
    }
    if !key.starts_with("tolerance.") {
        return false;
    }
    let result = (|| -> Result<()> {
        if !available(w) {
            return Err(invalid("Apply or cancel the current operation first"));
        }
        if !editing(w) {
            begin(w);
        }
        if !fields_are_valid(w) {
            return Err(invalid(
                "Correct the retained field text before selecting another stack or term",
            ));
        }
        match key {
            "tolerance.new" => new(w)?,
            "tolerance.edit" => {
                let id = args
                    .and_then(|args| args["stack"].as_str())
                    .map(str::to_owned)
                    .or_else(|| selected(w))
                    .ok_or_else(|| invalid("Choose a stack"))?;
                if !w.session.visible().tolerance_stacks.contains_key(&id) {
                    return Err(invalid("The stack no longer exists"));
                }
                load(w, Some(id));
            }
            "tolerance.delete" => {
                let id = selected(w).ok_or_else(|| invalid("Choose a stack"))?;
                w.dispatch(Command::DeleteToleranceStack { stack: id.clone() });
                if w.session.visible().tolerance_stacks.contains_key(&id) {
                    return Err(CadError::Invalid(w.status.clone()));
                }
                load(w, None);
            }
            "tolerance.duplicate" => {
                if w.session.visible().tolerance_stacks.len() >= stacks::MAX_STACKS {
                    return Err(invalid("A document supports at most 128 tolerance stacks"));
                }
                let mut source = source(w)?;
                source.id = cad_core::id();
                source.name = unique_name(w, &format!("{} Copy", source.name));
                for term in &mut source.terms {
                    term.id = cad_core::id();
                }
                let id = source.id.clone();
                w.fields.insert(SELECTED.into(), id.clone());
                submit(w, source)?;
                load(w, Some(id));
            }
            "tolerance.export-report" => {
                let report = report(w);
                if report.source.is_empty() {
                    return Err(invalid(
                        "Wait for the current source calculation before exporting",
                    ));
                }
                if let Some(path) = rfd::FileDialog::new()
                    .set_title("Export current tolerance stack report")
                    .add_filter("JSON report", &["json"])
                    .set_file_name("tolerance-stacks.json")
                    .save_file()
                {
                    cad_core::persistence::atomic_write(
                        &path,
                        &serde_json::to_vec_pretty(&report)?,
                    )?;
                    w.status = format!("Saved tolerance report {}", path.display());
                }
            }
            "tolerance.term-edit" => {
                let id = args
                    .and_then(|args| args["term"].as_str())
                    .ok_or_else(|| invalid("Choose a term"))?;
                if !source(w)?.terms.iter().any(|term| term.id == id) {
                    return Err(invalid("The term no longer exists"));
                }
                w.fields.insert(TERM.into(), id.into());
            }
            "tolerance.term-add" => {
                let mut source = source(w)?;
                if source.terms.len() >= stacks::MAX_TERMS {
                    return Err(invalid("A stack supports at most 128 terms"));
                }
                let mut term = new_term(source.scope.clone());
                term.name = format!("Dimension {}", source.terms.len() + 1);
                w.fields.insert(TERM.into(), term.id.clone());
                source.terms.push(term);
                submit(w, source)?;
            }
            "tolerance.term-remove" | "tolerance.term-earlier" | "tolerance.term-later" => {
                let mut source = source(w)?;
                let id = args
                    .and_then(|args| args["term"].as_str())
                    .map(str::to_owned)
                    .or_else(|| w.fields.get(TERM).cloned())
                    .ok_or_else(|| invalid("Choose a term"))?;
                let index = source
                    .terms
                    .iter()
                    .position(|term| term.id == id)
                    .ok_or_else(|| invalid("The term no longer exists"))?;
                if key == "tolerance.term-remove" {
                    if source.terms.len() <= 1 {
                        return Err(invalid("Keep at least one term"));
                    }
                    source.terms.remove(index);
                    w.fields.insert(
                        TERM.into(),
                        source.terms[index.min(source.terms.len() - 1)].id.clone(),
                    );
                } else {
                    let next = if key == "tolerance.term-earlier" {
                        index.checked_sub(1)
                    } else {
                        index
                            .checked_add(1)
                            .filter(|index| *index < source.terms.len())
                    }
                    .ok_or_else(|| invalid("The term is already at the end of the sequence"))?;
                    source.terms.swap(index, next);
                }
                submit(w, source)?;
            }
            _ => return Err(invalid("Unknown tolerance-stack command")),
        }
        Ok(())
    })();
    if let Err(error) = result {
        w.status = error.to_string();
        w.confirm_requested = false;
    }
    true
}
fn field(
    w: &Workspace,
    source: &Definition,
    term: Option<&Term>,
    pointer: &str,
    label: &str,
    value: Value,
    choices: Vec<Value>,
) -> SemanticElement {
    let stable = term
        .map(|term| {
            format!(
                "term:{}:{}",
                term.id,
                pointer.splitn(4, '/').nth(3).unwrap_or(pointer)
            )
        })
        .unwrap_or_else(|| pointer.replace('/', ":"));
    SemanticElement {
        id: format!("tolerance-stack-field:{}:{stable}", source.id),
        role: "tolerance-stack-field".into(),
        label: label.into(),
        value: Some(
            value
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| value.to_string()),
        ),
        state: if fields_are_valid(w) {
            "draft"
        } else {
            "invalid"
        }
        .into(),
        commands: vec![],
        relationships: vec![source.id.clone()]
            .into_iter()
            .chain(term.map(|term| term.id.clone()))
            .collect(),
        data: json!({"contract":"tcad.tolerance-stack-field","version":1,"setter":"tolerance-stack-editor","stack":source.id,"term":term.map(|term|&term.id),"editor":w.fields.get(TOKEN),"pointer":pointer,"json":value,"editable":w.settings.is_none(),"choices":choices,"value_type":if !choices.is_empty(){"choice"}else if value.is_boolean(){"boolean"}else if value.is_number(){"number"}else{"text"}}),
    }
}
fn length_field(mut field: SemanticElement) -> SemanticElement {
    field.data["unit"] = json!("mm");
    field.data["dimension"] = json!("length");
    field
}
fn binding_field(mut field: SemanticElement) -> SemanticElement {
    let value = field.data["json"].clone();
    let choices = field.data["choices"].as_array_mut().unwrap();
    let available = !choices.is_empty();
    if !choices.iter().any(|choice| choice["value"] == value) {
        choices.push(json!({"value":value,"label":"Missing source · choose a current binding"}));
    }
    field.data["value_type"] = json!("choice");
    field.data["editable"] = json!(available && field.data["editable"] == Value::Bool(true));
    field.data["disabled_reason"] = json!(
        "This scope has no current binding of this type; choose another nominal source or definition"
    );
    field
}
fn parameters(w: &Workspace, scope: &Scope) -> Vec<Value> {
    let doc = w.session.visible();
    doc.parts
        .get(&scope.definition)
        .map(|part| &part.parameters)
        .or_else(|| {
            doc.assemblies
                .get(&scope.definition)
                .map(|assembly| &assembly.parameters)
        })
        .into_iter()
        .flat_map(|parameters| parameters.iter())
        .filter(|(_, parameter)| parameter.dimension == Dimension::Length)
        .map(|(id, parameter)| json!({"value":id,"label":parameter.name}))
        .collect()
}
fn sketch_dimensions(w: &Workspace, scope: &Scope) -> Vec<Value> {
    w.session.visible().parts.get(&scope.definition).into_iter().flat_map(|part|part.sketches.iter()).flat_map(|(id,sketch)|sketch.constraints.iter().filter(|constraint|!constraint.driven&&cad_core::sketch_actions::is_dimension(&constraint.kind)&&!matches!(constraint.kind.as_str(),"angle"|"arc_angle"|"rotation")).map(move|constraint|json!({"value":{"sketch":id,"constraint":constraint.id},"label":format!("{} · {} = {}",sketch.name,constraint.kind,constraint.value.0)}))).collect()
}
fn fields(w: &Workspace) -> Vec<SemanticElement> {
    let Ok(source) = source(w) else {
        return vec![];
    };
    let mut out = vec![field(
        w,
        &source,
        None,
        "/name",
        "Name",
        json!(source.name),
        vec![],
    )];
    for (pointer, label, value, choices) in [
        (
            "/scope/definition",
            "Requirements definition",
            json!(source.scope.definition),
            owners(w),
        ),
        (
            "/scope/configuration",
            "Requirements configuration",
            json!(source.scope.configuration),
            configurations(w, &source.scope.definition),
        ),
    ] {
        out.push(binding_field(field(
            w, &source, None, pointer, label, value, choices,
        )));
    }
    for (name, label, value) in [
        (
            "lower_requirement",
            "Minimum result",
            &source.lower_requirement,
        ),
        (
            "upper_requirement",
            "Maximum result",
            &source.upper_requirement,
        ),
    ] {
        out.push(field(
            w,
            &source,
            None,
            &format!("requirement-enabled:{name}"),
            &format!("Use {label}"),
            json!(value.is_some()),
            vec![],
        ));
        if let Some(value) = value {
            out.push(length_field(field(
                w,
                &source,
                None,
                &format!("/{name}"),
                label,
                json!(value),
                vec![],
            )));
        }
    }
    out.push(field(
        w,
        &source,
        None,
        "/assume_independent_groups",
        "Assume variation groups are independent",
        json!(source.assume_independent_groups),
        vec![],
    ));
    if source.assume_independent_groups {
        let mut f = field(
            w,
            &source,
            None,
            "/sigma_multiplier",
            "Statistical k-sigma multiplier",
            json!(source.sigma_multiplier),
            vec![],
        );
        f.data["minimum"] = json!(0.);
        f.data["maximum"] = json!(10.);
        out.push(f);
    }
    if let Some((index, term)) = source
        .terms
        .iter()
        .enumerate()
        .find(|(_, term)| w.fields.get(TERM) == Some(&term.id))
        .or_else(|| source.terms.iter().enumerate().next())
    {
        let prefix = format!("/terms/{index}");
        let scope = term.nominal.scope();
        for (key, label, value) in [
            ("name", "Term name", json!(term.name)),
            ("enabled", "Include term", json!(term.enabled)),
            ("coefficient", "Signed coefficient", json!(term.coefficient)),
        ] {
            out.push(field(
                w,
                &source,
                Some(term),
                &format!("{prefix}/{key}"),
                label,
                value,
                vec![],
            ));
        }
        out.push(binding_field(field(
            w,
            &source,
            Some(term),
            &format!("{prefix}/nominal/scope/definition"),
            "Term definition",
            json!(scope.definition),
            owners(w),
        )));
        out.push(binding_field(field(
            w,
            &source,
            Some(term),
            &format!("{prefix}/nominal/scope/configuration"),
            "Term configuration",
            json!(scope.configuration),
            configurations(w, &scope.definition),
        )));
        let params = parameters(w, scope);
        let dims = sketch_dimensions(w, scope);
        let kind = match &term.nominal {
            Nominal::Expression { .. } => "expression",
            Nominal::Parameter { .. } => "parameter",
            Nominal::SketchDimension { .. } => "sketch_dimension",
        };
        let mut kinds = vec![json!({"value":"expression","label":"Unit-aware expression"})];
        if !params.is_empty() {
            kinds.push(json!({"value":"parameter","label":"Linked length parameter"}));
        }
        if !dims.is_empty() {
            kinds.push(
                json!({"value":"sketch_dimension","label":"Linked driving sketch dimension"}),
            );
        }
        let mut f = field(
            w,
            &source,
            Some(term),
            &format!("{prefix}/nominal-kind"),
            "Nominal source",
            json!(kind),
            kinds,
        );
        f.data["intent"] = json!("nominal-kind");
        out.push(f);
        match &term.nominal {
            Nominal::Expression { value, .. } => out.push(length_field(field(
                w,
                &source,
                Some(term),
                &format!("{prefix}/nominal/value"),
                "Nominal length",
                json!(value),
                vec![],
            ))),
            Nominal::Parameter { parameter, .. } => out.push(binding_field(field(
                w,
                &source,
                Some(term),
                &format!("{prefix}/nominal/parameter"),
                "Length parameter",
                json!(parameter),
                params,
            ))),
            Nominal::SketchDimension {
                sketch, constraint, ..
            } => {
                let mut f = field(
                    w,
                    &source,
                    Some(term),
                    &format!("{prefix}/sketch-binding"),
                    "Driving dimension",
                    json!({"sketch":sketch,"constraint":constraint}),
                    dims,
                );
                f = binding_field(f);
                f.data["intent"] = json!("sketch-binding");
                out.push(f);
            }
        }
        for (key, label, value) in [
            ("lower_deviation", "Lower deviation", &term.lower_deviation),
            ("upper_deviation", "Upper deviation", &term.upper_deviation),
        ] {
            out.push(length_field(field(
                w,
                &source,
                Some(term),
                &format!("{prefix}/{key}"),
                label,
                json!(value),
                vec![],
            )));
        }
        let distribution = match term.distribution {
            Distribution::Unspecified => "unspecified",
            Distribution::Uniform => "uniform",
            Distribution::Normal { .. } => "normal",
        };
        let mut f = field(
            w,
            &source,
            Some(term),
            &format!("{prefix}/distribution-kind"),
            "Variation assumption",
            json!(distribution),
            vec![
                json!({"value":"unspecified","label":"Unspecified · worst case only"}),
                json!({"value":"uniform","label":"Uniform within declared deviations"}),
                json!({"value":"normal","label":"Normal · explicit sigma and mean"}),
            ],
        );
        f.data["intent"] = json!("distribution-kind");
        out.push(f);
        if let Distribution::Normal { sigma, mean_offset } = &term.distribution {
            for (key, label, value) in [
                ("sigma", "Process sigma", sigma),
                ("mean_offset", "Process mean offset", mean_offset),
            ] {
                out.push(length_field(field(
                    w,
                    &source,
                    Some(term),
                    &format!("{prefix}/distribution/{key}"),
                    label,
                    json!(value),
                    vec![],
                )));
            }
        }
        let mut f = field(
            w,
            &source,
            Some(term),
            &format!("{prefix}/group-enabled"),
            "Share a variation variable",
            json!(term.variation_group.is_some()),
            vec![],
        );
        f.data["intent"] = json!("group-enabled");
        out.push(f);
        if let Some(group) = &term.variation_group {
            out.push(field(
                w,
                &source,
                Some(term),
                &format!("{prefix}/variation_group"),
                "Shared variation group",
                json!(group),
                vec![],
            ));
        }
        for (key, label, enabled) in [
            ("tolerance.term-earlier", "Earlier term", index > 0),
            (
                "tolerance.term-later",
                "Later term",
                index + 1 < source.terms.len(),
            ),
            (
                "tolerance.term-remove",
                "Remove term",
                source.terms.len() > 1,
            ),
        ] {
            out.push(command(
                w,
                key,
                label,
                enabled && fields_are_valid(w),
                json!({"stack":source.id,"term":term.id}),
            ));
        }
    }
    out.push(field(
        w,
        &source,
        None,
        "/notes",
        "Notes",
        json!(source.notes),
        vec![],
    ));
    if let Some(last) = out.last_mut() {
        last.data["multiline"] = json!(true);
        last.data["rows"] = json!(3);
    }
    out
}
fn status_label(status: Status) -> &'static str {
    match status {
        Status::Calculated => "Calculated",
        Status::WithinWorstCaseLimits => "Within worst-case requirements",
        Status::ExceedsWorstCaseLimits => "Exceeds worst-case requirements",
        Status::Unavailable => "Unavailable",
    }
}
pub fn semantics(w: &Workspace) -> Vec<SemanticElement> {
    if !editing(w) && w.session.visible().tolerance_stacks.is_empty() {
        return vec![];
    }
    let current = report(w);
    let mut out = Vec::new();
    for source in w.session.visible().tolerance_stacks.values() {
        let reading = current.stacks.get(&source.id);
        out.push(SemanticElement{id:format!("tolerance-stack:{}",source.id),role:"tolerance-stack".into(),label:source.name.clone(),value:reading.map(|reading|status_label(reading.status).into()),state:if current.source.is_empty(){"pending"}else if reading.is_some_and(|reading|reading.status==Status::Unavailable){"invalid"}else{"ready"}.into(),commands:vec!["tolerance.edit".into()],relationships:vec![source.scope.definition.clone()],data:json!({"contract":"tcad.tolerance-stack","version":stacks::CONTRACT_VERSION,"stack":source.id,"source":source,"reading":reading,"report_current":!current.source.is_empty(),"source_fingerprint":current.source,"actions":[{"command":"tolerance.edit","label":"Edit tolerance stack","enabled":available(w)&&fields_are_valid(w),"arguments":{"stack":source.id}}]})});
    }
    if editing(w) {
        out.extend(fields(w));
        if let Ok(source) = source(w) {
            for term in &source.terms {
                let contribution = current.stacks.get(&source.id).and_then(|reading| {
                    reading
                        .contributions
                        .iter()
                        .find(|value| value.term == term.id)
                });
                out.push(SemanticElement{id:format!("tolerance-term:{}:{}",source.id,term.id),role:"tolerance-stack-term".into(),label:term.name.clone(),value:contribution.map(|value|format!("{:.4} × {:+.4} mm",value.nominal_mm,value.coefficient)),state:if term.enabled{"enabled"}else{"disabled"}.into(),commands:vec!["tolerance.term-edit".into()],relationships:vec![source.id.clone(),term.nominal.scope().definition.clone()],data:json!({"stack":source.id,"term":term.id,"source":term,"contribution":contribution,"actions":[{"command":"tolerance.term-edit","label":"Edit term","enabled":fields_are_valid(w),"arguments":{"stack":source.id,"term":term.id}}]})});
            }
        }
        out.push(SemanticElement{id:"tolerance-stacks:source".into(),role:"tolerance-stack-source".into(),label:"Advanced source".into(),value:Some(w.json.clone()),state:if fields_are_valid(w){"draft"}else{"invalid"}.into(),commands:vec![],relationships:vec![],data:json!({"setter":"tolerance-stack-editor","pointer":"source","stack":selected(w),"editor":w.fields.get(TOKEN),"json":w.json,"editable":selected(w).is_some(),"value_type":"text","multiline":true,"rows":12})});
    }
    let count = w.session.visible().tolerance_stacks.len();
    for (key, label, enabled) in [
        ("tolerance.new", "New stack", count < stacks::MAX_STACKS),
        (
            "tolerance.duplicate",
            "Duplicate stack",
            selected(w).is_some() && count < stacks::MAX_STACKS,
        ),
        ("tolerance.delete", "Delete stack", selected(w).is_some()),
        (
            "tolerance.term-add",
            "Add term",
            source(w).is_ok_and(|source| source.terms.len() < stacks::MAX_TERMS),
        ),
        (
            "tolerance.export-report",
            "Export report…",
            !current.source.is_empty(),
        ),
    ] {
        out.push(command(
            w,
            key,
            label,
            enabled && fields_are_valid(w),
            json!({"stack":selected(w)}),
        ));
    }
    out
}
fn live(w: &Workspace, element: &SemanticElement) -> Result<SemanticElement> {
    if !editing(w)
        || element.data["editor"] != json!(w.fields.get(TOKEN))
        || element.data["stack"] != json!(selected(w))
    {
        return Err(invalid("This control belongs to an earlier stack editor"));
    }
    if element.data["pointer"] == "source" {
        return Ok(element.clone());
    }
    fields(w)
        .into_iter()
        .find(|live| live.id == element.id)
        .ok_or_else(|| invalid("The stack field or term no longer exists"))
}
pub fn current_value(w: &Workspace, element: &SemanticElement) -> Option<Value> {
    let element = live(w, element).ok()?;
    if element.data["pointer"] == "source" {
        Some(json!(w.json))
    } else {
        element.data.get("json").cloned()
    }
}
fn set_field(w: &mut Workspace, element: &SemanticElement, value: Value) -> Result<()> {
    let element = live(w, element)?;
    if !element.value_descriptor().editable {
        return Err(invalid("This stack field is read-only"));
    }
    if element.data["pointer"] == "source" {
        w.json = value
            .as_str()
            .ok_or_else(|| invalid("Stack source requires text"))?
            .into();
        submit(w, source(w)?)?;
        w.fields.retain(|key, _| !key.starts_with(INVALID));
        w.fields.insert(TOKEN.into(), cad_core::id());
        return Ok(());
    }
    let choices = element.data["choices"].as_array();
    if choices.is_some_and(|choices| {
        !choices.is_empty()
            && !choices
                .iter()
                .any(|choice| choice.get("value").unwrap_or(choice) == &value)
    }) {
        return Err(invalid("Choose a current source binding or option"));
    }
    let mut source = source(w)?;
    let pointer = element.data["pointer"]
        .as_str()
        .ok_or_else(|| invalid("Missing stack field"))?;
    if let Some(name) = pointer.strip_prefix("requirement-enabled:") {
        let enabled = value
            .as_bool()
            .ok_or_else(|| invalid("Requirement switch requires a boolean"))?;
        let target = match name {
            "lower_requirement" => &mut source.lower_requirement,
            "upper_requirement" => &mut source.upper_requirement,
            _ => return Err(invalid("Unknown requirement")),
        };
        *target = if enabled {
            Some(target.clone().unwrap_or_else(|| "0 mm".into()))
        } else {
            None
        };
    } else if let Some(term) = element.data["term"].as_str() {
        let term = source
            .terms
            .iter_mut()
            .find(|row| row.id == term)
            .ok_or_else(|| invalid("The term no longer exists"))?;
        match element.data["intent"].as_str() {
            Some("nominal-kind") => {
                let scope = term.nominal.scope().clone();
                term.nominal = match value.as_str() {
                    Some("expression") => Nominal::Expression {
                        scope,
                        value: "10 mm".into(),
                    },
                    Some("parameter") => Nominal::Parameter {
                        parameter: parameters(w, &scope)
                            .first()
                            .and_then(|value| value["value"].as_str())
                            .ok_or_else(|| invalid("This scope has no length parameters"))?
                            .into(),
                        scope,
                    },
                    Some("sketch_dimension") => {
                        let first =
                            sketch_dimensions(w, &scope)
                                .into_iter()
                                .next()
                                .ok_or_else(|| {
                                    invalid("This scope has no linear driving sketch dimensions")
                                })?;
                        Nominal::SketchDimension {
                            scope,
                            sketch: first["value"]["sketch"].as_str().unwrap().into(),
                            constraint: first["value"]["constraint"].as_str().unwrap().into(),
                        }
                    }
                    _ => return Err(invalid("Unknown nominal source")),
                };
            }
            Some("sketch-binding") => {
                let scope = term.nominal.scope().clone();
                term.nominal = Nominal::SketchDimension {
                    scope,
                    sketch: value["sketch"]
                        .as_str()
                        .ok_or_else(|| invalid("Choose a sketch dimension"))?
                        .into(),
                    constraint: value["constraint"]
                        .as_str()
                        .ok_or_else(|| invalid("Choose a driving dimension"))?
                        .into(),
                };
            }
            Some("distribution-kind") => {
                term.distribution = match value.as_str() {
                    Some("unspecified") => Distribution::Unspecified,
                    Some("uniform") => Distribution::Uniform,
                    Some("normal") => Distribution::Normal {
                        sigma: "0.03 mm".into(),
                        mean_offset: "0 mm".into(),
                    },
                    _ => return Err(invalid("Unknown variation assumption")),
                }
            }
            Some("group-enabled") => {
                term.variation_group = if value
                    .as_bool()
                    .ok_or_else(|| invalid("Group switch requires a boolean"))?
                {
                    Some(
                        term.variation_group
                            .clone()
                            .unwrap_or_else(|| "Group 1".into()),
                    )
                } else {
                    None
                }
            }
            _ => {
                let suffix = pointer
                    .splitn(4, '/')
                    .nth(3)
                    .ok_or_else(|| invalid("Missing term field"))?;
                let mut json = serde_json::to_value(&*term)?;
                *json
                    .pointer_mut(&format!("/{suffix}"))
                    .ok_or_else(|| invalid("The term field no longer exists"))? = value;
                *term = serde_json::from_value(json)?;
                if suffix == "nominal/scope/definition" {
                    let mut scope = term.nominal.scope().clone();
                    scope.configuration = None;
                    term.nominal = Nominal::Expression {
                        scope,
                        value: "10 mm".into(),
                    };
                }
            }
        }
    } else {
        let mut json = serde_json::to_value(&source)?;
        *json
            .pointer_mut(pointer)
            .ok_or_else(|| invalid("The stack field no longer exists"))? = value;
        source = serde_json::from_value(json)?;
        if pointer == "/scope/definition" {
            source.scope.configuration = None;
        }
    }
    submit(w, source)
}
pub fn set_value(w: &mut Workspace, element: &SemanticElement, value: Value) -> Result<()> {
    let result = set_field(w, element, value);
    field_error(w, element, result.as_ref().err().map(ToString::to_string));
    result
}
pub fn field_error(w: &mut Workspace, element: &SemanticElement, error: Option<String>) {
    if element.data["setter"] != "tolerance-stack-editor"
        || element.data["editor"] != json!(w.fields.get(TOKEN))
    {
        return;
    }
    let key = format!("{INVALID}{}", element.id);
    let domain = format!("{DOMAIN}{}", element.id);
    if let Some(error) = error {
        w.confirm_requested = false;
        if !source(w).is_ok_and(|source| {
            w.session.visible().tolerance_stacks.get(&source.id) == Some(&source)
        }) {
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
    element.data["setter"] == "tolerance-stack-editor"
        && (w.fields.contains_key(&format!("{INVALID}{}", element.id))
            || w.fields.contains_key(&format!("{DOMAIN}{}", element.id)))
}
fn result_ui(ui: &mut egui::Ui, reading: &stacks::Reading, pending: bool) {
    ui.label(if pending {
        "Calculating current source"
    } else {
        status_label(reading.status)
    });
    if let Some(nominal) = reading.nominal_mm {
        ui.label(format!("Nominal {nominal:.4} mm"));
    }
    if let Some([lower, upper]) = reading.worst_case_mm {
        ui.label(format!("Worst case {lower:.4} … {upper:.4} mm"));
    }
    if let Some(statistics) = &reading.statistics {
        ui.label(format!(
            "Mean {:.4} mm · σ {:.4} mm",
            statistics.mean_mm, statistics.sigma_mm
        ));
        ui.label(format!(
            "{}σ {:.4} … {:.4} mm",
            statistics.multiplier, statistics.spread_mm[0], statistics.spread_mm[1]
        ));
        ui.small(&statistics.assumption);
    }
    for diagnostic in &reading.diagnostics {
        ui.small(diagnostic);
    }
}
pub fn show(ui: &mut egui::Ui, w: &mut Workspace) {
    ui.small("Linear source dimensions · f64 arithmetic · mm report units");
    let elements = semantics(w);
    ui.horizontal_wrapped(|ui| {
        for element in elements.iter().filter(|element| {
            element.role == "tolerance-stack-action"
                && !element.commands[0].starts_with("tolerance.term-")
        }) {
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
    });
    for element in elements
        .iter()
        .filter(|element| element.role == "tolerance-stack")
    {
        if controls::button(
            ui,
            &element.id,
            &element.role,
            &[],
            &element.label,
            fields_are_valid(w),
            selected(w).as_deref() == element.data["stack"].as_str(),
        )
        .clicked()
        {
            action(w, "tolerance.edit", Some(&element.data));
        }
    }
    let current = report(w);
    if let Some(id) = selected(w)
        && let Some(reading) = current.stacks.get(&id)
    {
        result_ui(ui, reading, current.source.is_empty());
    }
    if let Ok(source) = source(w) {
        ui.separator();
        ui.label("Ordered dimensional terms");
        egui::Grid::new("tolerance-term-table")
            .num_columns(4)
            .striped(true)
            .show(ui, |ui| {
                ui.small("Term");
                ui.small("Nominal mm");
                ui.small("×");
                ui.small("Deviation mm");
                ui.end_row();
                let reading = current.stacks.get(&source.id);
                for term in &source.terms {
                    let contribution = reading.and_then(|reading| {
                        reading
                            .contributions
                            .iter()
                            .find(|value| value.term == term.id)
                    });
                    if controls::button(
                        ui,
                        &format!("tolerance-term:{}:{}", source.id, term.id),
                        "tolerance-stack-term",
                        &["compact"],
                        &format!("{}{}", if term.enabled { "" } else { "○ " }, term.name),
                        fields_are_valid(w),
                        w.fields.get(TERM) == Some(&term.id),
                    )
                    .clicked()
                    {
                        action(
                            w,
                            "tolerance.term-edit",
                            Some(&json!({"stack":source.id,"term":term.id})),
                        );
                    }
                    ui.small(
                        contribution
                            .map(|value| format!("{:.4}", value.nominal_mm))
                            .unwrap_or_else(|| "—".into()),
                    );
                    ui.small(format!("{:+.3}", term.coefficient));
                    ui.small(
                        contribution
                            .map(|value| {
                                format!(
                                    "{:+.4}…{:+.4}",
                                    value.lower_deviation_mm, value.upper_deviation_mm
                                )
                            })
                            .unwrap_or_else(|| "—".into()),
                    );
                    ui.end_row();
                }
            });
    }
    for element in fields(w) {
        if element.role == "tolerance-stack-action" {
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
        } else {
            crate::ui::semantic_field(ui, w, &element);
        }
    }
    if let Some(element) = semantics(w)
        .into_iter()
        .find(|element| element.commands.first().map(String::as_str) == Some("tolerance.term-add"))
        && controls::button(
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
        action(w, "tolerance.term-add", Some(&element.data));
    }
    ui.collapsing("Advanced source", |ui| {
        if let Some(element) = semantics(w)
            .into_iter()
            .find(|element| element.role == "tolerance-stack-source")
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
    ui.small("Signed terms form the result. Shared groups represent one repeated variation variable and must have identical assumptions. Statistical spread requires explicit distributions and group independence; it does not infer yield. Exceeded requirements remain valid design results; unavailable calculations block this editor's Apply.");
}
