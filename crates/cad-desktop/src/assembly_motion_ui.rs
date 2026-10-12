//! Live, mate-aware component manipulation over the shared modeling checkpoint.
use crate::{Workspace, presentation_controls as controls, worker::WorkerClient};
use bevy_egui::egui;
use cad_core::{
    CadError, Dimension, Document, Expr, Id, Placement, Result,
    assembly_geometry::EvaluatedAssembly,
    assembly_motion::drag::{self, Handle},
    assembly_motion::{self, Goal, Intent, Outcome},
    commands::Command,
    presentation::SemanticElement,
    protocol::{Evaluation, RequestIdentity, WorkerRequest, WorkerResponse},
};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, time::Instant};
mod viewport;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Mode {
    Translate,
    Rotate,
    Free,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Pivot {
    Picked,
    Origin,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
struct Values {
    mode: Mode,
    pivot: Pivot,
    translation: [Expr; 3],
    rotation: [Expr; 3],
    #[serde(default)]
    components: Vec<Id>,
}
struct Component {
    path: Vec<Id>,
    name: String,
    placement: Placement,
    reason: Option<String>,
}
pub struct State {
    token: Id,
    baseline: Document,
    expression_context: Document,
    assemblies: BTreeMap<Id, EvaluatedAssembly>,
    identity: RequestIdentity,
    expected_source: String,
    path: Vec<Id>,
    name: String,
    placement: Placement,
    components: BTreeMap<Id, Component>,
    picked_anchor: Option<[f64; 3]>,
    placement_scope: cad_core::configuration::PlacementScope,
    preferred_placement: Option<Placement>,
    selected_handle: Handle,
    capture: Option<viewport::Capture>,
    blocked_owner: Option<egui::Id>,
    blocked_seen: Option<u64>,
    placement_preview: Option<Evaluation>,
    values: Values,
    submitted: Values,
    accepted: Values,
    sequence: u64,
    needs_solve: bool,
    pending: Option<RequestIdentity>,
    changed: Instant,
    error: Option<String>,
    outcome: Option<Outcome>,
    confirm_after_solve: bool,
    worker: WorkerClient,
}

fn evaluation(w: &Workspace) -> Option<&Evaluation> {
    w.session
        .draft
        .as_ref()
        .and_then(|d| {
            d.evaluation
                .as_ref()
                .filter(|_| !d.pending && d.error.is_none())
        })
        .or_else(|| {
            w.session
                .draft
                .is_none()
                .then_some(w.session.evaluation.as_ref())
                .flatten()
        })
        .filter(|e| e.valid && e.identity == w.session.identity())
}
fn current_assembly(w: &Workspace) -> Option<&EvaluatedAssembly> {
    let doc = w.session.visible();
    let key = cad_core::configuration::key(
        &doc.root,
        doc.active_configurations.get(&doc.root).map(String::as_str),
    );
    evaluation(w)?.assemblies.get(&key)
}
fn component_reason(
    root: &cad_core::Assembly,
    current: Option<&EvaluatedAssembly>,
    generated: &std::collections::BTreeSet<Id>,
    component: &str,
) -> Option<String> {
    if generated.contains(component) {
        return Some("This occurrence follows a component pattern; move its seed".into());
    }
    let Some(instance) = root.instances.get(component) else {
        return Some("Select a component in the root assembly".into());
    };
    if instance.fixed {
        return Some("Release Grounded / fixed before moving this component".into());
    }
    if instance.translation_expressions.is_some() {
        return Some("Placement expressions drive this component; edit those expressions".into());
    }
    let Some(current) = current else {
        return Some("Wait for the current assembly evaluation".into());
    };
    if !current.placements.contains_key(component) {
        return Some("The component is suppressed in the active configuration".into());
    }
    let Some(motion) = current.component_motion.get(component) else {
        return Some("Current component motion information is unavailable".into());
    };
    if motion.grounded {
        return Some(
            "The assembly grounds this component; fix another reference component first".into(),
        );
    }
    if motion.degrees_of_freedom == 0 {
        return Some("This component is fully constrained; edit its driving mate".into());
    }
    None
}
fn movement_reason(w: &Workspace, component: &str) -> Option<String> {
    let doc = w.session.visible();
    let Some(root) = doc.assemblies.get(&doc.root) else {
        return Some("Open an assembly before moving a component".into());
    };
    component_reason(
        root,
        current_assembly(w),
        &cad_core::component_pattern::generated(root),
        component,
    )
}

pub fn begin_instance(w: &mut Workspace, assembly: &str, component: &str, rotate: bool) {
    if assembly != w.session.visible().root {
        w.status = "Open this assembly as the document root to manipulate its components".into();
        return;
    }
    let path = w
        .selected
        .as_ref()
        .filter(|s| s.path.first().is_some_and(|id| id == component))
        .map(|s| s.path.clone())
        .unwrap_or_else(|| vec![component.into()]);
    begin(
        w,
        path,
        if rotate {
            Mode::Rotate
        } else {
            Mode::Translate
        },
    );
}

fn begin(w: &mut Workspace, path: Vec<Id>, mode: Mode) {
    if w.settings.is_some() || w.sketch.is_some() || w.tool_checkpoint.is_some() {
        w.status = "Finish or cancel the current operation before moving a component".into();
        return;
    }
    if w.view.exploded_view.is_some() {
        w.status="Return to the assembled view before moving components; exploded step positions belong to the Exploded View editor".into();
        return;
    }
    let Some(component) = path.first() else {
        w.status = "Select an assembly component first".into();
        return;
    };
    if let Some(reason) = movement_reason(w, component) {
        w.status = reason;
        return;
    }
    let baseline = w.session.visible().clone();
    let expression_context = match cad_core::configuration::scope(
        &baseline,
        &baseline,
        &baseline.root,
        baseline
            .active_configurations
            .get(&baseline.root)
            .map(String::as_str),
    ) {
        Ok(context) => context,
        Err(error) => {
            w.status = error.to_string();
            return;
        }
    };
    let Some(evaluation) = evaluation(w) else {
        return;
    };
    let key = cad_core::configuration::key(
        &baseline.root,
        baseline
            .active_configurations
            .get(&baseline.root)
            .map(String::as_str),
    );
    let Some(placement) = evaluation
        .assemblies
        .get(&key)
        .and_then(|a| a.placements.get(component))
        .cloned()
    else {
        return;
    };
    let Some(instance) = baseline
        .assemblies
        .get(&baseline.root)
        .and_then(|a| a.instances.get(component))
    else {
        return;
    };
    let name = instance.name.clone();
    let current = &evaluation.assemblies[&key];
    let generated =
        cad_core::component_pattern::generated(&expression_context.assemblies[&baseline.root]);
    let mut components = BTreeMap::new();
    for instance in baseline.assemblies[&baseline.root].instances.values() {
        let Some(placement) = current.placements.get(&instance.id).cloned() else {
            continue;
        };
        let saved_path = w
            .geometry_selection
            .iter()
            .chain(w.selected.iter())
            .find(|selection| selection.path.first() == Some(&instance.id))
            .map(|selection| selection.path.clone())
            .unwrap_or_else(|| vec![instance.id.clone()]);
        components.insert(
            instance.id.clone(),
            Component {
                path: saved_path,
                name: instance.name.clone(),
                placement,
                reason: component_reason(
                    &expression_context.assemblies[&baseline.root],
                    Some(current),
                    &generated,
                    &instance.id,
                ),
            },
        );
    }
    let mut selected_components = vec![component.clone()];
    for selection in &w.geometry_selection {
        if let Some(id) = selection
            .path
            .first()
            .filter(|id| components.contains_key(*id))
            && !selected_components.contains(id)
        {
            if let Some(reason) = &components[id].reason {
                w.status = format!("{}: {reason}", components[id].name);
                return;
            }
            if selected_components.len() >= assembly_motion::MAX_COMPONENTS {
                w.status = "Select at most 128 direct components for one movement".into();
                return;
            }
            selected_components.push(id.clone());
        }
    }
    let assemblies = evaluation.assemblies.clone();
    let placement_scope =
        cad_core::configuration::PlacementScope::selected(&baseline, &baseline.root);
    let placement_preview = if expression_context
        .assemblies
        .get(&baseline.root)
        .is_some_and(|a| a.component_patterns.is_empty())
        && !baseline.parts.values().any(|part| {
            part.features.iter().any(|feature| {
                matches!(feature.operation, cad_core::Operation::ReferenceBody { .. })
            })
        }) {
        let mut preview = evaluation.clone();
        preview.valid = false;
        preview.operation_preview = None;
        preview.assembly_inspections.clear();
        preview.exploded_views.clear();
        Some(preview)
    } else {
        None
    };
    // Collapse exploded display first so the picked point denotes a mechanical
    // location, not the presentation transform of an exploded occurrence.
    w.start_tool();
    let picked_anchor = w
        .selected
        .as_ref()
        .filter(|s| s.path == path)
        .and_then(|s| assembly_motion::local_anchor(&placement, s.point).ok());
    let values = Values {
        mode,
        pivot: if picked_anchor.is_some() {
            Pivot::Picked
        } else {
            Pivot::Origin
        },
        translation: std::array::from_fn(|_| Expr("0 mm".into())),
        rotation: std::array::from_fn(|_| Expr("0 deg".into())),
        components: selected_components,
    };
    w.geometry_selection.clear();
    w.edge_selection_path = None;
    w.candidates.clear();
    w.assembly_motion = Some(State {
        token: cad_core::id(),
        identity: w.session.identity(),
        expected_source: baseline.fingerprint(),
        baseline,
        expression_context,
        assemblies,
        path,
        name,
        placement,
        components,
        picked_anchor,
        placement_scope,
        preferred_placement: None,
        selected_handle: if mode == Mode::Rotate {
            Handle::ViewRotation
        } else {
            Handle::ViewPlane
        },
        capture: None,
        blocked_owner: None,
        blocked_seen: None,
        placement_preview,
        values: values.clone(),
        submitted: values.clone(),
        accepted: values,
        sequence: 0,
        needs_solve: false,
        pending: None,
        changed: Instant::now(),
        error: None,
        outcome: None,
        confirm_after_solve: false,
        worker: WorkerClient::new(),
    });
    w.open_editor("assembly-motion");
    w.status = "Drag a movement dial or enter a dimension; mates remain enforced".into();
}

fn intent(state: &State) -> Result<Intent> {
    let length = |expression: &Expr| {
        cad_core::expression::value(
            &state.expression_context,
            &state.baseline.root,
            expression,
            Dimension::Length,
        )
    };
    let angle = |expression: &Expr| {
        cad_core::expression::value(
            &state.expression_context,
            &state.baseline.root,
            expression,
            Dimension::Angle,
        )
    };
    let translation = state
        .values
        .translation
        .iter()
        .map(length)
        .collect::<Result<Vec<_>>>()?;
    let rotation = if state.values.mode == Mode::Translate {
        vec![0.; 3]
    } else {
        state
            .values
            .rotation
            .iter()
            .map(angle)
            .collect::<Result<Vec<_>>>()?
    };
    let anchor = match state.values.pivot {
        Pivot::Origin => [0.; 3],
        Pivot::Picked => state
            .picked_anchor
            .ok_or_else(|| CadError::Invalid("No picked pivot is available".into()))?,
    };
    let translation = [translation[0], translation[1], translation[2]];
    let rotation =
        (state.values.mode != Mode::Translate).then_some([rotation[0], rotation[1], rotation[2]]);
    let mut goal = Goal::from_offsets(
        state.path[0].clone(),
        &state.placement,
        anchor,
        translation,
        rotation,
    );
    goal.preferred_placement = state.preferred_placement.clone();
    goal.validate()?;
    let pivot = drag::anchor(&state.placement, anchor);
    let mut additional_goals = vec![];
    let mut additional_paths = vec![];
    for id in state
        .values
        .components
        .iter()
        .filter(|id| *id != &state.path[0])
    {
        let component = state.components.get(id).ok_or_else(|| {
            CadError::Invalid("A selected movement component is no longer available".into())
        })?;
        if let Some(reason) = &component.reason {
            return Err(CadError::Invalid(format!("{}: {reason}", component.name)));
        }
        let local = assembly_motion::local_anchor(&component.placement, pivot)?;
        let mut target = Goal::from_offsets(
            id.clone(),
            &component.placement,
            local,
            translation,
            rotation,
        );
        target.preferred_placement = state
            .outcome
            .as_ref()
            .and_then(|outcome| outcome.placements.get(id))
            .cloned();
        additional_goals.push(target);
        additional_paths.push(component.path.clone());
    }
    Ok(Intent {
        version: assembly_motion::CONTRACT_VERSION,
        assembly: state.baseline.root.clone(),
        path: state.path.clone(),
        goal,
        additional_goals,
        additional_paths,
    })
}
fn changed(state: &mut State) {
    state.changed = Instant::now();
    state.error = None;
    state.confirm_after_solve = false;
    state.needs_solve = true;
    if state.capture.is_none() {
        state.preferred_placement = None;
    }
    if state.values.mode == Mode::Rotate && !state.selected_handle.rotation() {
        state.selected_handle = Handle::ViewRotation;
    }
    if state.values.mode == Mode::Translate && state.selected_handle.rotation() {
        state.selected_handle = Handle::ViewPlane;
    }
}
fn toggle_component(state: &mut State, component: &str) -> Result<()> {
    if state.capture.is_some() {
        return Err(CadError::Invalid(
            "Release the movement handle before changing its component selection".into(),
        ));
    }
    if component == state.path[0] {
        return Err(CadError::Invalid(
            "The primary movement component remains selected; begin another movement to change it"
                .into(),
        ));
    }
    let entry = state
        .components
        .get(component)
        .ok_or_else(|| CadError::Invalid("The movement component no longer exists".into()))?;
    if let Some(reason) = &entry.reason {
        return Err(CadError::Invalid(reason.clone()));
    }
    if state.values.components.iter().any(|id| id == component) {
        state.values.components.retain(|id| id != component);
    } else {
        if state.values.components.len() >= assembly_motion::MAX_COMPONENTS {
            return Err(CadError::Invalid(
                "Select at most 128 direct components for one movement".into(),
            ));
        }
        state.values.components.push(component.into());
    }
    changed(state);
    Ok(())
}
fn component_selection(ui: &mut egui::Ui, state: &mut State) {
    let identity = id(state, "components");
    let properties = controls::properties(
        &identity,
        "assembly-motion-components",
        &["selection-list"],
        "draft",
    );
    if properties
        .get("display")
        .is_some_and(|value| value == "none")
    {
        return;
    }
    controls::label(
        ui,
        &format!("{identity}.label"),
        "field-label",
        &[],
        &format!("Components · {} selected", state.values.components.len()),
        "ready",
    );
    let choices = state.components.keys().cloned().collect::<Vec<_>>();
    egui::ScrollArea::vertical()
        .id_salt(&identity)
        .max_height(controls::number(&properties, "max-height", 140.))
        .show_rows(
            ui,
            controls::number(&properties, "height", 24.),
            choices.len(),
            |ui, rows| {
                for row in rows {
                    let component = &choices[row];
                    let selected = state.values.components.contains(component);
                    let entry = &state.components[component];
                    let enabled = component != &state.path[0]
                        && entry.reason.is_none()
                        && state.capture.is_none()
                        && (selected
                            || state.values.components.len() < assembly_motion::MAX_COMPONENTS);
                    let response = controls::button(
                        ui,
                        &format!("{identity}.{component}"),
                        "assembly-motion-component-action",
                        &["selection-choice"],
                        &entry.name,
                        enabled,
                        selected,
                    );
                    if let Some(reason) = &entry.reason {
                        response.clone().on_hover_text(reason);
                    } else if component == &state.path[0] {
                        response.clone().on_hover_text(
                            "Primary component · shared pivot belongs to this occurrence",
                        );
                    }
                    if response.clicked()
                        && let Err(error) = toggle_component(state, component)
                    {
                        state.error = Some(error.to_string());
                    }
                }
            },
        );
}
fn title(state: &State) -> String {
    if state.values.components.len() > 1 {
        format!(
            "Move / Rotate · {} components",
            state.values.components.len()
        )
    } else {
        format!("Move Component · {}", state.name)
    }
}
fn rebase_values(state: &mut State) {
    let actual = state
        .outcome
        .as_ref()
        .and_then(|outcome| outcome.placements.get(&state.path[0]))
        .cloned()
        .unwrap_or_else(|| state.placement.clone());
    let anchor = if state.values.pivot == Pivot::Picked {
        state.picked_anchor.unwrap_or([0.; 3])
    } else {
        [0.; 3]
    };
    state.values.translation =
        drag::translation_offsets(&state.placement, &actual, anchor, [0.; 3])
            .map(|value| Expr(format!("{value:.12} mm")));
    if let Ok(rotation) = drag::rotation_offsets(&state.placement, &actual, [1., 0., 0.], 0.) {
        state.values.rotation = rotation.map(|value| Expr(format!("{value:.12} deg")));
    }
    changed(state);
    state.preferred_placement = Some(actual);
}
pub fn cancel(w: &mut Workspace) {
    if let Some(state) = w.assembly_motion.take() {
        state.worker.cancel();
    }
}
pub fn ready_to_confirm(w: &mut Workspace) -> bool {
    if dragging(w) {
        w.status = "Release the component drag before applying".into();
        w.confirm_requested = false;
        return false;
    }
    if w.assembly_motion.is_some() && w.settings.is_some() {
        w.status = "Close Settings before applying component movement".into();
        w.confirm_requested = false;
        return false;
    }
    if let Some(state) = &w.assembly_motion
        && let Some(outcome) = &state.outcome
        && let Some(current) = current_assembly(w)
        && let Err(error) = assembly_motion::validate_adoption(outcome, current)
    {
        w.status = error.to_string();
        w.confirm_requested = false;
        return false;
    }
    let Some(state) = w.assembly_motion.as_mut() else {
        return true;
    };
    if state.expected_source != w.session.visible().fingerprint() {
        w.status = "The movement baseline changed; cancel and begin movement again".into();
        w.confirm_requested = false;
        return false;
    }
    if let Some(error) = &state.error {
        w.status = error.clone();
        w.confirm_requested = false;
        return false;
    }
    if state.pending.is_some() || state.needs_solve || state.values != state.accepted {
        state.confirm_after_solve = true;
        w.confirm_requested = true;
        w.status =
            "Waiting for the current mate-constrained movement and geometry validation".into();
        return false;
    }
    true
}

/// A dedicated persistent worker makes movement responsive without competing
/// with document tessellation. Both source identity and goal generation reject
/// results after another edit, cancellation, or a newer movement request.
pub fn poll(w: &mut Workspace) {
    // Personal preferences may stay open while a modeling tool owns its
    // checkpoint. Leave completed replies queued until model writes resume.
    if w.settings.is_some() {
        return;
    }
    let Some(mut state) = w.assembly_motion.take() else {
        return;
    };
    if state.expected_source != w.session.visible().fingerprint() {
        state.worker.cancel();
        state.pending = None;
        state.error = Some("The movement source changed; cancel and begin again".into());
        state.confirm_after_solve = false;
    }
    if (state.values != state.submitted || state.needs_solve)
        && (state.capture.is_some() || state.changed.elapsed().as_millis() >= 60)
        && state.error.is_none()
    {
        match intent(&state) {
            Ok(intent) => {
                state.sequence += 1;
                let mut identity = state.identity.clone();
                identity.draft = Some(state.token.clone());
                identity.edit = state.sequence;
                state.worker.submit(WorkerRequest::ManipulateAssembly {
                    identity: identity.clone(),
                    document: state.baseline.clone(),
                    assemblies: state.assemblies.clone(),
                    intent: Box::new(intent),
                });
                state.submitted = state.values.clone();
                state.needs_solve = false;
                state.pending = Some(identity);
                state.error = None;
                w.status = "Solving component movement within its mates…".into();
            }
            Err(error) => {
                state.worker.cancel();
                state.needs_solve = false;
                state.pending = None;
                state.error = Some(error.to_string());
                state.confirm_after_solve = false;
                w.confirm_requested = false;
            }
        }
    }
    for response in state.worker.drain() {
        match response {
            WorkerResponse::AssemblyManipulated { identity, outcome }
                if state.pending.as_ref() == Some(&identity)
                    && state.submitted == state.values
                    && state.expected_source == w.session.visible().fingerprint() =>
            {
                state.pending = None;
                let mut document = state.baseline.clone();
                match assembly_motion::apply_in_scope(
                    &mut document,
                    &outcome,
                    &state.placement_scope,
                ) {
                    Ok(()) => {
                        let confirm = state.confirm_after_solve;
                        let expected = document.fingerprint();
                        w.dispatch(Command::ReplaceDocument { document });
                        if w.session.visible().fingerprint() != expected {
                            state.error = Some(
                                "The solved movement could not enter the shared document draft"
                                    .into(),
                            );
                            state.confirm_after_solve = false;
                            w.confirm_requested = false;
                            continue;
                        }
                        if let Some(selected) = w.selected.as_mut().filter(|s| s.path == state.path)
                            && let Some(anchor) = state.picked_anchor
                            && let Some(placement) = outcome.placements.get(&state.path[0])
                            && let Some(point) = Goal::from_offsets(
                                state.path[0].clone(),
                                placement,
                                anchor,
                                [0.; 3],
                                None,
                            )
                            .position
                        {
                            selected.point = point;
                        }
                        state.expected_source = w.session.visible().fingerprint();
                        state.accepted = state.submitted.clone();
                        viewport::preview(&mut state, w, &outcome);
                        state.outcome = Some(*outcome);
                        state.error = None;
                        w.confirm_requested = confirm;
                        w.status = "Movement solved; rebuilding the current model preview".into();
                    }
                    Err(error) => {
                        state.error = Some(error.to_string());
                        state.confirm_after_solve = false;
                        w.confirm_requested = false;
                    }
                }
            }
            WorkerResponse::Error {
                identity: Some(identity),
                message,
            } if state.pending.as_ref() == Some(&identity) && state.submitted == state.values => {
                state.pending = None;
                state.error = Some(message);
                state.confirm_after_solve = false;
                w.confirm_requested = false;
            }
            _ => {}
        }
    }
    w.assembly_motion = Some(state);
}

pub fn viewport(
    w: &mut Workspace,
    ui: &mut egui::Ui,
    rect: egui::Rect,
    response: &egui::Response,
    blocked: &[egui::Rect],
) -> bool {
    viewport::interact(w, ui, rect, response, blocked)
}
pub fn dragging(w: &Workspace) -> bool {
    w.assembly_motion
        .as_ref()
        .is_some_and(|state| state.capture.is_some() || state.blocked_owner.is_some())
}
pub(crate) fn component_selected(w: &Workspace, path: &[Id]) -> bool {
    w.assembly_motion.as_ref().is_some_and(|state| {
        path.first()
            .is_some_and(|component| state.values.components.contains(component))
    })
}
pub fn cancel_drag(w: &mut Workspace) -> bool {
    viewport::cancel(w)
}
pub fn finish_drag(w: &mut Workspace) {
    viewport::finish(w);
}
pub fn finish_ui(w: &mut Workspace, ctx: &egui::Context) {
    viewport::finish_ui(w, ctx);
}
pub fn display_evaluation(w: &Workspace) -> Option<&Evaluation> {
    let state = w.assembly_motion.as_ref()?;
    if evaluation(w).is_some() {
        return None;
    }
    state.placement_preview.as_ref().filter(|preview| {
        state.outcome.is_some()
            && preview.identity == w.session.identity()
            && state.expected_source == w.session.visible().fingerprint()
    })
}

fn id(state: &State, field: &str) -> String {
    format!("assembly.motion.{}.{}", state.path[0], field)
}
fn field(ui: &mut egui::Ui, state: &mut State, key: &str) {
    let (rotation, axis) = match key {
        "tx" => (false, 0),
        "ty" => (false, 1),
        "tz" => (false, 2),
        "rx" => (true, 0),
        "ry" => (true, 1),
        "rz" => (true, 2),
        _ => return,
    };
    let control_id = id(state, key);
    let label = format!(
        "{} {}",
        if rotation { "Rotate" } else { "Move" },
        ["X", "Y", "Z"][axis]
    );
    let properties = controls::properties(
        &control_id,
        "assembly-motion-field",
        &["parameter-field", "expression-field"],
        "draft",
    );
    if properties.get("display").is_some_and(|v| v == "none") {
        return;
    }
    let owner = state.baseline.root.clone();
    let baseline = &state.expression_context;
    let expression = if rotation {
        &mut state.values.rotation[axis]
    } else {
        &mut state.values.translation[axis]
    };
    let dimension = if rotation {
        Dimension::Angle
    } else {
        Dimension::Length
    };
    let mut value = cad_core::expression::value(baseline, &owner, expression, dimension)
        .ok()
        .map(|v| if rotation { v.to_degrees() } else { v });
    let edited = ui
        .scope(|ui| {
            controls::apply_properties(ui, &properties);
            ui.horizontal(|ui| {
                controls::label(
                    ui,
                    &format!("{control_id}.label"),
                    "field-label",
                    &[],
                    &label,
                    "ready",
                );
                let mut edited = false;
                if let Some(value) = &mut value {
                    let dial_id = format!("{control_id}.dial");
                    let dial_properties = controls::properties(
                        &dial_id,
                        "assembly-motion-dial",
                        &["number-field"],
                        "draft",
                    );
                    if !dial_properties.get("display").is_some_and(|v| v == "none") {
                        let response = ui
                            .scope(|ui| {
                                controls::apply_properties(ui, &dial_properties);
                                ui.add(
                                    egui::DragValue::new(value)
                                        .speed(if rotation { 0.5 } else { 0.2 })
                                        .suffix(if rotation { "°" } else { " mm" }),
                                )
                            })
                            .inner
                            .on_hover_text("Drag for live constrained movement.");
                        if response.changed() {
                            expression.0 =
                                format!("{value:.9} {}", if rotation { "deg" } else { "mm" });
                            edited = true;
                        }
                    }
                }
                edited |= ui
                    .add(
                        egui::TextEdit::singleline(&mut expression.0)
                            .id(ui.id().with(&control_id))
                            .desired_width(controls::number(&properties, "width", 100.)),
                    )
                    .changed();
                edited
            })
            .inner
        })
        .inner;
    if edited {
        changed(state);
    }
}
fn modes(ui: &mut egui::Ui, state: &mut State) {
    ui.horizontal_wrapped(|ui| {
        for (mode, label, key) in [
            (Mode::Translate, "Move", "translate"),
            (Mode::Rotate, "Rotate", "rotate"),
            (Mode::Free, "Move + Rotate", "free"),
        ] {
            if controls::button(
                ui,
                &id(state, key),
                "assembly-motion-mode-action",
                &["tool-mode"],
                label,
                true,
                state.values.mode == mode,
            )
            .clicked()
                && state.values.mode != mode
            {
                state.values.mode = mode;
                rebase_values(state);
            }
        }
    });
}
fn pivot(ui: &mut egui::Ui, state: &mut State) {
    ui.horizontal_wrapped(|ui| {
        for (pivot, label, key, enabled) in [
            (Pivot::Origin, "Component origin", "origin", true),
            (
                Pivot::Picked,
                "Picked point",
                "picked",
                state.picked_anchor.is_some(),
            ),
        ] {
            if controls::button(
                ui,
                &id(state, key),
                "assembly-motion-pivot-action",
                &["tool-mode"],
                label,
                enabled,
                state.values.pivot == pivot,
            )
            .clicked()
                && state.values.pivot != pivot
            {
                state.values.pivot = pivot;
                rebase_values(state);
            }
        }
    });
}
fn movement_status(state: &State) -> String {
    if let Some(error) = &state.error {
        error.clone()
    } else if state.pending.is_some() || state.needs_solve || state.values != state.accepted {
        "Solving the current movement…".into()
    } else if let Some(outcome) = &state.outcome {
        let mut message = format!(
            "{} {} DOF · mate residual {:.3e}",
            outcome.solution.report.degrees_of_freedom,
            if state.values.components.len() > 1 {
                "transient cohort"
            } else {
                "remaining"
            },
            outcome.solution.report.residual
        );
        let position_error = outcome
            .goal_errors
            .values()
            .filter_map(|error| error.position_error_mm)
            .max_by(f64::total_cmp)
            .or(outcome.position_error_mm);
        let rotation_error = outcome
            .goal_errors
            .values()
            .filter_map(|error| error.rotation_error_deg)
            .max_by(f64::total_cmp)
            .or(outcome.rotation_error_deg);
        if let Some(error) = position_error.filter(|e| *e > 1e-6) {
            message.push_str(&format!(" · requested point limited by {error:.4} mm"));
        }
        if let Some(error) = rotation_error.filter(|e| *e > 1e-6) {
            message.push_str(&format!(" · requested rotation limited by {error:.4}°"));
        }
        message
    } else {
        "Choose a movement; the current model remains visible until its new preview is validated."
            .into()
    }
}
fn placement_scope_text(state: &State) -> String {
    match &state.placement_scope {
        cad_core::configuration::PlacementScope::Base => "Placement scope: Base assembly".into(),
        cad_core::configuration::PlacementScope::Named(configuration) => {
            let name = state
                .baseline
                .assemblies
                .get(&state.baseline.root)
                .and_then(|a| a.configurations.get(configuration))
                .map(|c| c.name.as_str())
                .unwrap_or("Current configuration");
            format!(
                "Placement scope: {name}. Child configurations inherit these values unless overridden."
            )
        }
    }
}
fn placement_scope_control(ui: &mut egui::Ui, state: &State) {
    controls::label(
        ui,
        &id(state, "placement-scope"),
        "tool-guidance",
        &[],
        &placement_scope_text(state),
        "ready",
    );
    let base = matches!(
        state.placement_scope,
        cad_core::configuration::PlacementScope::Base
    );
    ui.horizontal_wrapped(|ui| {
        controls::button(
            ui,
            &id(state, "scope-base"),
            "assembly-motion-scope",
            &["tool-mode"],
            "Base",
            base,
            base,
        )
        .on_disabled_hover_text(
            "Select Base configuration before starting movement to edit its placements.",
        );
        if !base {
            controls::button(
                ui,
                &id(state, "scope-current"),
                "assembly-motion-scope",
                &["tool-mode"],
                "Current configuration",
                true,
                true,
            );
        }
    });
}

pub fn show(ui: &mut egui::Ui, w: &mut Workspace) {
    let Some(mut state) = w.assembly_motion.take() else {
        return;
    };
    let before = state.values.clone();
    controls::label(
        ui,
        &id(&state, "title"),
        "tool-title",
        &[],
        &title(&state),
        "ready",
    );
    modes(ui, &mut state);
    component_selection(ui, &mut state);
    controls::label(
        ui,
        &id(&state, "guidance"),
        "tool-guidance",
        &[],
        "Selected components move together about the primary component's origin or picked point. Offsets use assembly axes; mates and travel limits remain enforced.",
        "ready",
    );
    if state.path.len() > 1 {
        controls::label(
            ui,
            &id(&state, "scope"),
            "tool-guidance",
            &[],
            "The selected nested occurrence moves with its rigid top-level subassembly.",
            "ready",
        );
    }
    pivot(ui, &mut state);
    if state.values.mode != Mode::Rotate {
        for key in ["tx", "ty", "tz"] {
            field(ui, &mut state, key);
        }
    }
    if state.values.mode != Mode::Translate {
        for key in ["rx", "ry", "rz"] {
            field(ui, &mut state, key);
        }
    }
    let status = movement_status(&state);
    controls::label(
        ui,
        &id(&state, "status"),
        "assembly-motion-status",
        &["preview-status"],
        &status,
        if state.error.is_some() {
            "invalid"
        } else if state.pending.is_some() {
            "pending"
        } else {
            "ready"
        },
    );
    placement_scope_control(ui, &state);
    let mut reset = false;
    let mut retry = false;
    let mut confirm = false;
    let mut cancel = false;
    ui.horizontal_wrapped(|ui| {
        if state.error.is_some() {
            retry = controls::button(
                ui,
                &id(&state, "retry"),
                "command-action",
                &[],
                "Retry movement",
                true,
                false,
            )
            .clicked();
        }
        reset = controls::button(
            ui,
            &id(&state, "reset"),
            "command-action",
            &[],
            "Reset movement",
            true,
            false,
        )
        .clicked();
        confirm = controls::button(
            ui,
            &id(&state, "apply"),
            "apply-action",
            &[],
            "OK",
            state.error.is_none(),
            false,
        )
        .clicked();
        cancel = controls::button(
            ui,
            &id(&state, "cancel"),
            "cancel-action",
            &[],
            "Cancel",
            true,
            false,
        )
        .clicked();
    });
    if retry {
        changed(&mut state);
        w.confirm_requested = false;
    }
    if reset {
        state.values.translation = std::array::from_fn(|_| Expr("0 mm".into()));
        state.values.rotation = std::array::from_fn(|_| Expr("0 deg".into()));
        changed(&mut state);
        w.confirm_requested = false;
    }
    if state.values != before {
        w.confirm_requested = false;
        w.rendered_key.clear();
    }
    w.assembly_motion = Some(state);
    if cancel {
        w.cancel_tool();
    } else if confirm {
        w.confirm_tool();
    }
}

pub fn semantic_control(ui: &mut egui::Ui, w: &mut Workspace, element: &SemanticElement) {
    let Some(mut state) = w.assembly_motion.take() else {
        return;
    };
    let before = state.values.clone();
    if element.data.get("token").and_then(|v| v.as_str()) != Some(&state.token) {
        w.assembly_motion = Some(state);
        return;
    }
    if let Some(key) = element.data.get("field").and_then(|v| v.as_str()) {
        field(ui, &mut state, key);
    } else if element.role == "assembly-motion-mode" {
        modes(ui, &mut state);
    } else if element.role == "assembly-motion-pivot" {
        pivot(ui, &mut state);
    } else if element.role == "assembly-motion-scope" {
        placement_scope_control(ui, &state);
    }
    if state.values != before {
        w.confirm_requested = false;
    }
    w.assembly_motion = Some(state);
}
pub fn action(w: &mut Workspace, command: &str, data: Option<&serde_json::Value>) -> bool {
    if command.starts_with("assembly.motion-") {
        let active = w.assembly_motion.as_ref().is_some_and(|state| {
            data.and_then(|data| data.get("token"))
                .and_then(|v| v.as_str())
                .is_none_or(|token| token == state.token)
        });
        if !active {
            w.status = "That movement action is no longer current".into();
            return true;
        }
    }
    match command {
        "assembly.move" | "assembly.rotate" => {
            let doc = w.session.visible();
            if data.is_some_and(|data| {
                data.get("assembly")
                    .is_some_and(|owner| owner.as_str() != Some(&doc.root))
                    || data
                        .get("document")
                        .is_some_and(|id| id.as_str() != Some(&doc.id))
            }) {
                w.status =
                    "That component movement belongs to a different document or assembly".into();
                return true;
            }
            let path = data
                .and_then(|v| v.get("path"))
                .and_then(|v| serde_json::from_value::<Vec<Id>>(v.clone()).ok())
                .or_else(|| {
                    w.selected
                        .as_ref()
                        .map(|s| s.path.clone())
                        .filter(|p| !p.is_empty())
                })
                .or_else(|| {
                    w.view
                        .active_instance
                        .as_ref()
                        .filter(|i| {
                            doc.assemblies
                                .get(&doc.root)
                                .is_some_and(|a| a.instances.contains_key(*i))
                        })
                        .map(|i| vec![i.clone()])
                })
                .or_else(|| {
                    doc.assemblies.get(&doc.root).and_then(|a| {
                        a.instances
                            .keys()
                            .find(|id| movement_reason(w, id).is_none())
                            .map(|id| vec![id.clone()])
                    })
                });
            if let Some(path) = path {
                begin(
                    w,
                    path,
                    if command == "assembly.rotate" {
                        Mode::Rotate
                    } else {
                        Mode::Translate
                    },
                );
            } else {
                w.status = "Select a movable component in the root assembly".into();
            }
            true
        }
        "assembly.motion-reset" => {
            if let Some(state) = w.assembly_motion.as_mut() {
                state.values.translation = std::array::from_fn(|_| Expr("0 mm".into()));
                state.values.rotation = std::array::from_fn(|_| Expr("0 deg".into()));
                changed(state);
            }
            w.confirm_requested = false;
            true
        }
        "assembly.motion-component" => {
            let component = data
                .and_then(|data| data.get("component"))
                .and_then(serde_json::Value::as_str);
            if let Some(component) = component
                && let Some(state) = w.assembly_motion.as_mut()
            {
                if let Err(error) = toggle_component(state, component) {
                    w.status = error.to_string();
                } else {
                    w.confirm_requested = false;
                    w.rendered_key.clear();
                }
            }
            true
        }
        "assembly.motion-retry" => {
            if let Some(state) = w.assembly_motion.as_mut() {
                changed(state);
            }
            w.confirm_requested = false;
            true
        }
        "assembly.motion-mode" => {
            let mode = data
                .and_then(|data| data.get("mode"))
                .and_then(|value| serde_json::from_value::<Mode>(value.clone()).ok());
            if let Some(mode) = mode
                && let Some(state) = w.assembly_motion.as_mut()
                && state.values.mode != mode
            {
                state.values.mode = mode;
                rebase_values(state);
                w.confirm_requested = false;
            }
            true
        }
        "assembly.motion-handle" => {
            let handle = data
                .and_then(|data| data.get("handle"))
                .and_then(|value| serde_json::from_value::<Handle>(value.clone()).ok());
            if let Some(handle) = handle
                && handle.axis([0., 0., 1.]).is_ok()
                && let Some(state) = w.assembly_motion.as_mut()
            {
                state.selected_handle = handle;
                w.status = format!(
                    "{}: drag the handle or component in the model",
                    handle.label()
                );
            }
            true
        }
        "assembly.motion-pivot" => {
            let pivot = data
                .and_then(|data| data.get("pivot"))
                .and_then(|value| serde_json::from_value::<Pivot>(value.clone()).ok());
            if let Some(pivot) = pivot
                && let Some(state) = w.assembly_motion.as_mut()
                && state.values.pivot != pivot
                && (pivot != Pivot::Picked || state.picked_anchor.is_some())
            {
                state.values.pivot = pivot;
                rebase_values(state);
                w.confirm_requested = false;
            }
            true
        }
        "assembly.motion-apply" => {
            w.confirm_tool();
            true
        }
        "assembly.motion-cancel" => {
            w.cancel_tool();
            true
        }
        _ => false,
    }
}
pub fn semantics(w: &Workspace) -> Vec<SemanticElement> {
    let mut elements = vec![];
    let doc = w.session.visible();
    if let Some(root) = doc.assemblies.get(&doc.root) {
        // Resolve identity once for the semantic snapshot, not once per row.
        let current = current_assembly(w);
        let generated = cad_core::component_pattern::generated(root);
        for instance in root.instances.values() {
            let reason = if w.tool_checkpoint.is_some() {
                Some("Finish or cancel the current operation".into())
            } else {
                component_reason(root, current, &generated, &instance.id)
            };
            for (command, label) in [
                ("assembly.move", "Move component"),
                ("assembly.rotate", "Rotate component"),
            ] {
                elements.push(SemanticElement{id:format!("{command}:{}",instance.id),role:"assembly-motion-action".into(),label:format!("{label} · {}",instance.name),value:reason.clone(),state:if reason.is_some(){"disabled"}else{"ready"}.into(),commands:vec![command.into()],relationships:vec![root.id.clone(),instance.id.clone()],data:serde_json::json!({"contract":"tcad.assembly_motion","version":assembly_motion::CONTRACT_VERSION,"document":doc.id,"assembly":root.id,"path":[instance.id],"actions":[{"command":command,"label":label,"enabled":reason.is_none(),"disabled_reason":reason,"arguments":{"document":doc.id,"assembly":root.id,"path":[instance.id]}}]})});
            }
        }
    }
    let Some(state) = &w.assembly_motion else {
        return elements;
    };
    for (key, role, text) in [
        ("title", "tool-title", title(state)),
        (
            "guidance",
            "tool-guidance",
            "Offsets use assembly axes. Mates and travel limits constrain the requested movement."
                .into(),
        ),
        (
            "placement-scope",
            "tool-guidance",
            placement_scope_text(state),
        ),
    ] {
        elements.push(SemanticElement {
            id: id(state,key), role: role.into(), label: text, value: None, state: "ready".into(), commands: vec![],
            relationships: vec![state.path[0].clone()],
            data: serde_json::json!({"contract":"tcad.assembly_motion","version":assembly_motion::CONTRACT_VERSION,"token":state.token,"path":state.path}),
        });
    }
    elements.push(SemanticElement {
        id:id(state,"placement-scope-control"),role:"assembly-motion-scope".into(),label:"Placement scope".into(),value:Some(placement_scope_text(state)),state:"selected".into(),commands:vec![],relationships:vec![state.baseline.root.clone()],
        data:serde_json::json!({"contract":"tcad.assembly_motion","version":assembly_motion::CONTRACT_VERSION,"token":state.token,"scope":state.placement_scope,"editable":false,"base_requires_active_base_configuration":true}),
    });
    elements.extend(viewport::semantics(w, state));
    if let Some(outcome) = &state.outcome {
        for (component, error) in &outcome.goal_errors {
            elements.push(SemanticElement {id:format!("{}.{}",id(state,"goal-error"),component),role:"assembly-motion-goal-status".into(),label:state.components.get(component).map(|entry|entry.name.clone()).unwrap_or_else(||component.clone()),value:Some(format!("Point {:.6} mm · rotation {:.6}°",error.position_error_mm.unwrap_or(0.),error.rotation_error_deg.unwrap_or(0.))),state:if error.position_error_mm.is_some_and(|error|error>1e-6) || error.rotation_error_deg.is_some_and(|error|error>1e-6){"limited"}else{"reached"}.into(),commands:vec![],relationships:vec![component.clone()],data:serde_json::json!({"contract":"tcad.assembly_motion","version":assembly_motion::CONTRACT_VERSION,"goal_error":error,"selected":state.values.components.contains(component),"shared_pivot":true,"temporary_relative_locks":state.values.components.len()>1})});
        }
    }
    elements.push(SemanticElement {id:id(state,"components"),role:"assembly-motion-components".into(),label:format!("Components · {} selected",state.values.components.len()),value:None,state:"draft".into(),commands:vec![],relationships:state.values.components.clone(),data:serde_json::json!({"contract":"tcad.assembly_motion","version":assembly_motion::CONTRACT_VERSION,"token":state.token,"selection":state.values.components,"shared_pivot":true,"maximum":assembly_motion::MAX_COMPONENTS})});
    for (component, entry) in &state.components {
        let selected = state.values.components.contains(component);
        let reason = entry
            .reason
            .clone()
            .or_else(|| (component == &state.path[0]).then(|| "Primary movement component".into()))
            .or_else(|| {
                state
                    .capture
                    .is_some()
                    .then(|| "Release the movement handle first".into())
            })
            .or_else(|| {
                (!selected && state.values.components.len() >= assembly_motion::MAX_COMPONENTS)
                    .then(|| "Maximum movement selection reached".into())
            });
        elements.push(SemanticElement {id:format!("{}.{}",id(state,"components"),component),role:"assembly-motion-component-action".into(),label:entry.name.clone(),value:Some(selected.to_string()),state:if selected {"selected"}else if reason.is_some(){"disabled"}else{"ready"}.into(),commands:vec!["assembly.motion-component".into()],relationships:vec![state.baseline.root.clone(),component.clone()],data:serde_json::json!({"contract":"tcad.assembly_motion","version":assembly_motion::CONTRACT_VERSION,"token":state.token,"component":component,"path":entry.path,"selected":selected,"actions":[{"command":"assembly.motion-component","label":"Toggle movement component","enabled":reason.is_none(),"disabled_reason":reason,"arguments":{"token":state.token,"component":component}}]})});
    }
    if state.path.len() > 1 {
        elements.push(SemanticElement {
            id:id(state,"scope"), role:"tool-guidance".into(), label:"The selected nested occurrence moves with its rigid top-level subassembly.".into(), value:None, state:"ready".into(), commands:vec![], relationships:vec![state.path[0].clone()],
            data:serde_json::json!({"contract":"tcad.assembly_motion","version":assembly_motion::CONTRACT_VERSION,"token":state.token,"path":state.path}),
        });
    }
    for (key, label, rotation, axis) in [
        ("tx", "Move X", false, 0),
        ("ty", "Move Y", false, 1),
        ("tz", "Move Z", false, 2),
        ("rx", "Rotate X", true, 0),
        ("ry", "Rotate Y", true, 1),
        ("rz", "Rotate Z", true, 2),
    ] {
        let expression = if rotation {
            &state.values.rotation[axis]
        } else {
            &state.values.translation[axis]
        };
        elements.push(SemanticElement{id:id(state,key),role:"assembly-motion-field".into(),label:label.into(),value:Some(expression.0.clone()),state:"draft".into(),commands:vec![],relationships:vec![state.baseline.root.clone(),state.path[0].clone()],data:serde_json::json!({"contract":"tcad.assembly_motion","version":assembly_motion::CONTRACT_VERSION,"token":state.token,"field":key,"path":state.path,"type":"quantity","dimension":if rotation{Dimension::Angle}else{Dimension::Length},"unit":if rotation{"deg"}else{"mm"},"expression":expression,"editable":true,"shared_draft":true,"transient_goal":true})});
    }
    for (mode, label, key) in [
        (Mode::Translate, "Move", "translate"),
        (Mode::Rotate, "Rotate", "rotate"),
        (Mode::Free, "Move + Rotate", "free"),
    ] {
        elements.push(SemanticElement {
            id: id(state, key), role: "assembly-motion-mode-action".into(),
            label: label.into(), value: None,
            state: if state.values.mode == mode {"selected"} else {"ready"}.into(),
            commands: vec!["assembly.motion-mode".into()], relationships: vec![state.path[0].clone()],
            data: serde_json::json!({"contract":"tcad.assembly_motion","version":assembly_motion::CONTRACT_VERSION,"token":state.token,"mode":mode,"path":state.path,"classes":["tool-mode"]}),
        });
    }
    for (pivot, label, key) in [
        (Pivot::Origin, "Component origin", "origin"),
        (Pivot::Picked, "Picked point", "picked"),
    ] {
        let available = pivot != Pivot::Picked || state.picked_anchor.is_some();
        elements.push(SemanticElement {
            id: id(state, key), role: "assembly-motion-pivot-action".into(),
            label: label.into(), value: (!available).then(||"Pick a point on the component before beginning movement".into()),
            state: if !available {"disabled"} else if state.values.pivot == pivot {"selected"} else {"ready"}.into(),
            commands: vec!["assembly.motion-pivot".into()], relationships: vec![state.path[0].clone()],
            data: serde_json::json!({"contract":"tcad.assembly_motion","version":assembly_motion::CONTRACT_VERSION,"token":state.token,"pivot":pivot,"path":state.path,"classes":["tool-mode"]}),
        });
    }
    for (key, role, label) in [
        ("mode", "assembly-motion-mode", "Movement mode"),
        ("pivot", "assembly-motion-pivot", "Movement pivot"),
    ] {
        elements.push(SemanticElement{id:id(state,key),role:role.into(),label:label.into(),value:None,state:"draft".into(),commands:vec![],relationships:vec![state.path[0].clone()],data:serde_json::json!({"contract":"tcad.assembly_motion","version":assembly_motion::CONTRACT_VERSION,"token":state.token,"mode":state.values.mode,"pivot":state.values.pivot,"path":state.path,"editable":true})});
    }
    for (key, command, label, enabled) in [
        ("reset", "assembly.motion-reset", "Reset movement", true),
        (
            "retry",
            "assembly.motion-retry",
            "Retry movement",
            state.error.is_some(),
        ),
        (
            "apply",
            "assembly.motion-apply",
            "OK",
            state.error.is_none(),
        ),
        ("cancel", "assembly.motion-cancel", "Cancel", true),
    ] {
        elements.push(SemanticElement{id:id(state,key),role:if key=="apply"{"apply-action"}else if key=="cancel"{"cancel-action"}else{"command-action"}.into(),label:label.into(),value:state.error.clone(),state:if enabled{"ready"}else{"disabled"}.into(),commands:vec![command.into()],relationships:vec![state.path[0].clone()],data:serde_json::json!({"contract":"tcad.assembly_motion","version":assembly_motion::CONTRACT_VERSION,"token":state.token,"requires_current_validation":true})});
    }
    elements.push(SemanticElement{id:id(state,"status"),role:"assembly-motion-status".into(),label:"Movement preview".into(),value:Some(movement_status(state)),state:if state.error.is_some(){"invalid"}else if state.pending.is_some() || state.needs_solve || state.values!=state.accepted{"pending"}else{"ready"}.into(),commands:vec![],relationships:vec![state.path[0].clone()],data:serde_json::json!({"contract":"tcad.assembly_motion","version":assembly_motion::CONTRACT_VERSION,"path":state.path,"goal":intent(state).ok(),"result":state.outcome,"pending":state.pending.is_some() || state.needs_solve,"complete_occurrence_identity":true,"shared_checkpoint":true,"native_worker":true,"mate_constraints_preserved":true})});
    elements
}
