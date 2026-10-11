//! Docked, explicit source-input repair using the same document draft and validation.
use crate::{Workspace, presentation_controls as controls};
use bevy_egui::egui;
use cad_core::{
    commands::Command,
    presentation::SemanticElement,
    protocol::{Evaluation, RequestIdentity},
    reference_repair::{self, Class, Failure, Input, Repair, Replacement},
    *,
};
use serde_json::Value;

pub struct State {
    failures: Vec<Failure>,
    owner: Option<NodeRef>,
    input: Option<Input>,
    candidate: Option<Replacement>,
    picking: bool,
    previewed: Option<RequestIdentity>,
    repairs: usize,
    unpreviewed: bool,
}
fn current(w: &Workspace) -> Option<&Evaluation> {
    w.session
        .draft
        .as_ref()
        .and_then(|d| d.evaluation.as_ref())
        .or(w.session.evaluation.as_ref())
        .filter(|e| e.identity == w.session.identity())
}
pub(crate) fn display_evaluation(w: &Workspace) -> Option<&Evaluation> {
    w.reference_repair.as_ref().and_then(|_| current(w))
}
pub(crate) fn reset(w: &mut Workspace) {
    if w.reference_repair.is_some() {
        w.view.history_cursor = None;
    }
    w.reference_repair = None;
}
fn stations(w: &Workspace, owner: &NodeRef) -> Vec<(NodeRef, String)> {
    let Some(evaluation) = current(w) else {
        return Vec::new();
    };
    let Some(part) = w.session.visible().parts.get(&owner.definition) else {
        return Vec::new();
    };
    let Some(consumer) = part.features.iter().position(|f| f.id == owner.node) else {
        return Vec::new();
    };
    let Some(history) = evaluation.history.get(&owner.definition) else {
        return Vec::new();
    };
    part.features
        .iter()
        .take(consumer)
        .filter(|feature| {
            history.stages.get(&feature.id).is_some_and(|stage| {
                stage.unavailable_reason.is_none()
                    && stage
                        .bodies
                        .iter()
                        .all(|body| history.meshes.contains_key(body))
            })
        })
        .map(|feature| (part.reference(&feature.id), feature.name.clone()))
        .collect()
}
fn choose_station(w: &mut Workspace, station: NodeRef) {
    let Some(owner) = w.reference_repair.as_ref().and_then(|s| s.owner.as_ref()) else {
        return;
    };
    if !stations(w, owner)
        .iter()
        .any(|(available, _)| available == &station)
    {
        w.status = "This history station is unavailable in the current upstream evaluation".into();
        return;
    }
    w.view.history_cursor = Some(station);
    w.selected = None;
    w.geometry_selection.clear();
    w.candidates.clear();
    w.rendered_key.clear();
    w.status =
        "Inspecting the selected cached upstream stage · pick an explicit replacement".into();
}
pub(crate) fn open(w: &mut Workspace) {
    if w.sketch.is_some()
        || w.settings.is_some()
        || w.tool_checkpoint.is_some() && current(w).is_none_or(|e| e.valid)
    {
        w.status = "Finish or cancel the current editor before repairing references".into();
        return;
    }
    let evaluation = current(w);
    let failures = reference_repair::failures(w.session.visible(), evaluation);
    let owner = failures.first().map(|f| f.owner.clone()).or_else(|| {
        w.view
            .active_part
            .clone()
            .zip(w.view.active_node.clone())
            .map(|(definition, node)| NodeRef { definition, node })
    });
    let input = owner
        .as_ref()
        .and_then(|owner| reference_repair::inputs(w.session.visible(), owner).ok())
        .and_then(|inputs| inputs.into_iter().next());
    w.start_tool();
    w.reference_repair = Some(State {
        failures,
        owner,
        input,
        candidate: None,
        picking: true,
        previewed: None,
        repairs: 0,
        unpreviewed: false,
    });
    w.open_editor("reference-repair");
    if let Some(owner) = w.reference_repair.as_ref().and_then(|s| s.owner.clone()) {
        w.view.active_part = Some(owner.definition.clone());
        w.view.active_node = Some(owner.node.clone());
        if let Some((station, _)) = stations(w, &owner).into_iter().last() {
            choose_station(w, station);
        }
    }
    w.status = "Select a failed feature and input, then pick an explicit replacement in its usable upstream model".into();
    w.rendered_key.clear();
}
fn select_input(w: &mut Workspace, input: Input) {
    let owner = input.owner.clone();
    if let Some(state) = w.reference_repair.as_mut() {
        state.owner = Some(input.owner.clone());
        state.input = Some(input);
        state.candidate = None;
        state.unpreviewed = false;
        state.picking = true;
        w.selected = None;
        w.geometry_selection.clear();
        w.candidates.clear();
        w.rendered_key.clear();
    }
    w.view.active_part = Some(owner.definition.clone());
    w.view.active_node = Some(owner.node.clone());
    w.view.history_cursor = None;
    if let Some((station, _)) = stations(w, &owner).into_iter().last() {
        choose_station(w, station);
    }
}
pub(crate) fn capture_selection(w: &mut Workspace) -> bool {
    let Some(state) = w.reference_repair.as_ref().filter(|s| s.picking) else {
        return false;
    };
    let Some(input) = state.input.clone() else {
        return true;
    };
    let Some(selected) = w.selected.clone() else {
        return true;
    };
    let Some(evaluation) = current(w) else {
        w.status = "Wait for the current upstream display before picking a replacement".into();
        return true;
    };
    let body = crate::reference_ui::selected_body(w, &selected);
    let candidate = if matches!(input.class, Class::Topology(_) | Class::Plane) {
        let displayed = evaluation
            .instances
            .iter()
            .filter(|instance| instance.path == selected.path)
            .flat_map(|instance| crate::viewport::display_meshes(w, evaluation, instance))
            .collect::<Vec<_>>();
        let available = match selected.reference.topology.as_str() {
            "face" => displayed.iter().flat_map(|m| &m.faces).any(|f| {
                f.reference == selected.reference
                    && f.body.as_ref().is_none_or(|b| b == &body)
                    && (input.class != Class::Plane || f.plane.is_some())
            }),
            "edge" => displayed.iter().flat_map(|m| &m.edges).any(|e| {
                e.reference == selected.reference && e.body.as_ref().is_none_or(|b| b == &body)
            }),
            "plane" => evaluation.references.values().any(|r| {
                r.elements()
                    .iter()
                    .any(|(_, reference, _)| reference == &selected.reference)
            }),
            _ => false,
        };
        if !available {
            w.status = "The picked reference is unavailable in the current upstream result".into();
            return true;
        }
        let body = if selected.reference.topology == "plane" {
            selected.reference.producer.clone()
        } else {
            body
        };
        Replacement::Topology {
            reference: selected.reference,
            body,
        }
    } else if input.class == Class::Body {
        let Some(body) = selected.body else {
            w.status = "Pick a face or edge belonging to the replacement body".into();
            return true;
        };
        // A retained face can originate in an older feature. A body replacement
        // targets the actual selected stage output, preserving intervening edits.
        Replacement::Node { node: body }
    } else {
        if input.element.is_some() {
            Replacement::SketchElement {
                profile: selected.reference.producer,
                elements: vec![],
            }
        } else {
            Replacement::Node {
                node: selected.reference.producer,
            }
        }
    };
    set_candidate(w, candidate);
    true
}
fn set_candidate(w: &mut Workspace, candidate: Replacement) {
    let Some(input) = w.reference_repair.as_ref().and_then(|s| s.input.clone()) else {
        return;
    };
    if let Some(state) = w.reference_repair.as_mut() {
        state.picking = true;
        state.unpreviewed = true;
    }
    if let Replacement::SketchElement { profile, elements } = &candidate
        && elements.is_empty()
        && reference_repair::node_compatible(w.session.visible(), &input, profile).is_ok()
    {
        if let Some(state) = w.reference_repair.as_mut() {
            state.candidate = Some(candidate);
        }
        w.status = "Choose the explicit sketch member or point used by the replacement".into();
        return;
    }
    let mut document = w.session.visible().clone();
    match reference_repair::repair(
        &mut document,
        &[Repair {
            input,
            replacement: candidate.clone(),
        }],
    ) {
        Ok(()) => {
            if let Some(state) = w.reference_repair.as_mut() {
                state.candidate = Some(candidate);
            }
            w.status = "Explicit replacement selected · Preview rebuilds the repaired draft".into();
        }
        Err(error) => {
            if let Some(state) = w.reference_repair.as_mut() {
                state.candidate = None;
            }
            w.status = error.to_string();
        }
    }
}
fn preview(w: &mut Workspace) {
    let Some((input, replacement)) = w
        .reference_repair
        .as_ref()
        .and_then(|s| s.input.clone().zip(s.candidate.clone()))
    else {
        w.status = "Select a compatible replacement before previewing the repair".into();
        return;
    };
    let before = w.session.identity();
    w.dispatch(Command::RepairReferences {
        repairs: vec![Repair {
            input: input.clone(),
            replacement,
        }],
    });
    let after = w.session.identity();
    if after == before {
        return;
    }
    if let Some(state) = w.reference_repair.as_mut() {
        state.previewed = Some(after);
        state.repairs += 1;
        state.unpreviewed = false;
        state.candidate = None;
        state.picking = false;
        state.input = reference_repair::inputs(w.session.visible(), &input.owner)
            .ok()
            .and_then(|inputs| {
                inputs
                    .into_iter()
                    .find(|i| i.storage == input.storage && i.path == input.path)
            });
    }
    w.selected = None;
    w.geometry_selection.clear();
    w.rendered_key.clear();
}
pub(crate) fn current_source_is_valid(w: &Workspace) -> bool {
    w.reference_repair.as_ref().is_none_or(|s| {
        s.repairs > 0
            && !s.unpreviewed
            && s.candidate.is_none()
            && s.previewed.as_ref() == Some(&w.session.identity())
    })
}
pub(crate) fn action(w: &mut Workspace, command: &str, arguments: Option<&Value>) -> bool {
    match command {
        "reference.repair" => {
            if w.reference_repair.is_none() {
                open(w);
            }
            if let Some(owner) = arguments
                .and_then(|args| args.get("owner"))
                .and_then(|value| serde_json::from_value::<NodeRef>(value.clone()).ok())
                && let Ok(inputs) = reference_repair::inputs(w.session.visible(), &owner)
                && let Some(input) = inputs.into_iter().next()
            {
                select_input(w, input);
            }
        }
        "reference.repair-feature" => {
            if let Some(input) = arguments
                .and_then(|args| args.get("owner").or(Some(args)))
                .and_then(|value| serde_json::from_value::<NodeRef>(value.clone()).ok())
                .and_then(|owner| reference_repair::inputs(w.session.visible(), &owner).ok())
                .and_then(|inputs| inputs.into_iter().next())
            {
                select_input(w, input);
            }
        }
        "reference.repair-replacement" => {
            if let Some(candidate) = arguments
                .and_then(|args| args.get("candidate").or(Some(args)))
                .and_then(|value| serde_json::from_value::<Replacement>(value.clone()).ok())
            {
                set_candidate(w, candidate);
            }
        }
        "reference.repair-station" => {
            if let Some(station) = arguments
                .and_then(|args| args.get("station").or(Some(args)))
                .and_then(|value| serde_json::from_value::<NodeRef>(value.clone()).ok())
            {
                choose_station(w, station);
            }
        }
        "reference.repair-input" => {
            if let Some(input) = arguments
                .and_then(|args| args.get("input").or(Some(args)))
                .and_then(|value| serde_json::from_value::<Input>(value.clone()).ok())
            {
                select_input(w, input);
            }
        }
        "reference.repair-pick" => {
            if let Some(state) = w.reference_repair.as_mut() {
                state.picking = true;
            }
            w.rendered_key.clear();
            w.status = "Pick a compatible reference in the current upstream model".into();
        }
        "reference.repair-use-selection" => {
            capture_selection(w);
        }
        "reference.repair-preview" => preview(w),
        "reference.repair-apply" => w.confirm_tool(),
        "reference.repair-cancel" => w.cancel_tool(),
        _ => return false,
    }
    true
}
fn source_label(document: &Document, replacement: &Replacement) -> String {
    match replacement {
        Replacement::Node { node }
        | Replacement::Topology { body: node, .. }
        | Replacement::SketchElement { profile: node, .. } => {
            let label = document
                .parts
                .get(&node.definition)
                .map(|p| {
                    format!(
                        "{} / {}",
                        p.name,
                        p.features
                            .iter()
                            .find(|f| f.id == node.node)
                            .map(|f| f.name.as_str())
                            .unwrap_or("Missing feature")
                    )
                })
                .unwrap_or_else(|| "Missing part".into());
            if let Replacement::Topology { reference, .. } = replacement {
                format!("{label} · {}", reference.topology)
            } else {
                label
            }
        }
        Replacement::Sketch { sketch } => document
            .parts
            .values()
            .find_map(|p| p.sketches.get(sketch))
            .map(|s| s.name.clone())
            .unwrap_or_else(|| "Missing sketch".into()),
        Replacement::OriginPlane { plane } => plane.label().into(),
    }
}
fn text(ui: &mut egui::Ui, id: &str, role: &str, label: &str, state: &str) {
    controls::label(ui, id, role, &["reference-repair-control"], label, state);
}
fn button(ui: &mut egui::Ui, id: &str, label: &str, enabled: bool) -> egui::Response {
    controls::button(
        ui,
        id,
        "repair-action",
        &["reference-repair-control"],
        label,
        enabled,
        false,
    )
}
fn node_candidate(input: &Input, node: NodeRef) -> Replacement {
    if input.element.is_some() {
        Replacement::SketchElement {
            profile: node,
            elements: vec![],
        }
    } else {
        Replacement::Node { node }
    }
}
fn candidate_ready(w: &Workspace) -> bool {
    w.reference_repair.as_ref().and_then(|s|s.candidate.as_ref()).is_some_and(|candidate| !matches!(candidate,Replacement::SketchElement{elements,..} if elements.is_empty()))
}
pub(crate) fn show(ui: &mut egui::Ui, w: &mut Workspace) {
    let Some(state) = w.reference_repair.as_ref() else {
        return;
    };
    let owner = state.owner.clone();
    let input = state.input.clone();
    let candidate = state.candidate.clone();
    let failures = reference_repair::failures(w.session.visible(), current(w));
    if let Some(state) = w.reference_repair.as_mut() {
        state.failures = failures.clone();
    }
    text(
        ui,
        "repair-title",
        "repair-summary",
        "Repair a source reference",
        "ready",
    );
    text(
        ui,
        "repair-instructions",
        "repair-summary",
        "Choose the input, pick a replacement, then preview. Apply commits one undoable repair; Cancel restores the starting document.",
        "ready",
    );
    if let Some(evaluation) = current(w).filter(|e| !e.valid) {
        text(
            ui,
            "repair-upstream-status",
            "repair-message",
            "Upstream display only · the current model has failed features",
            "warning",
        );
        for diagnostic in evaluation
            .diagnostics
            .iter()
            .filter(|d| d.severity == "blocked")
        {
            text(
                ui,
                &format!(
                    "repair-blocked:{}",
                    diagnostic.target.as_deref().unwrap_or("document")
                ),
                "repair-message",
                &diagnostic.message,
                "blocked",
            );
        }
    }
    let document = w.session.visible().clone();
    let mut selected_owner = owner.clone();
    let feature_label = owner
        .as_ref()
        .map(|o| source_label(&document, &Replacement::Node { node: o.clone() }))
        .unwrap_or_else(|| "Choose feature".into());
    controls::menu_button(ui, "repair-feature-picker", &feature_label, |ui| {
        for part in document.parts.values() {
            for feature in &part.features {
                let target = part.reference(&feature.id);
                if reference_repair::inputs(&document, &target)
                    .is_ok_and(|inputs| !inputs.is_empty())
                {
                    let failed = failures.iter().any(|f| f.owner == target);
                    let label = format!(
                        "{}{} / {}",
                        if failed { "! " } else { "" },
                        part.name,
                        feature.name
                    );
                    if controls::selectable(
                        ui,
                        &format!("repair-feature:{}::{}", part.id, feature.id),
                        "repair-feature-choice",
                        &label,
                        owner.as_ref() == Some(&target),
                    )
                    .clicked()
                    {
                        selected_owner = Some(target);
                        ui.close();
                    }
                }
            }
        }
    });
    if selected_owner != owner {
        if let Some(input) = selected_owner
            .as_ref()
            .and_then(|owner| reference_repair::inputs(&document, owner).ok())
            .and_then(|inputs| inputs.into_iter().next())
        {
            select_input(w, input);
        }
        return;
    }
    if let Some(failure) = failures.iter().find(|f| Some(&f.owner) == owner.as_ref()) {
        for (index, message) in failure.messages.iter().enumerate() {
            text(
                ui,
                &format!("repair-error:{}:{index}", failure.owner.node),
                "repair-message",
                message,
                "error",
            );
        }
    }
    let Some(owner) = owner else {
        text(
            ui,
            "repair-empty",
            "repair-summary",
            "Select a feature with an associative input to repair.",
            "empty",
        );
        return;
    };
    let history_label = w
        .view
        .history_cursor
        .as_ref()
        .and_then(|cursor| {
            document
                .parts
                .get(&cursor.definition)?
                .features
                .iter()
                .find(|feature| feature.id == cursor.node)
        })
        .map(|feature| format!("Upstream: {}", feature.name))
        .unwrap_or_else(|| "Choose upstream stage".into());
    controls::menu_button(ui, "repair-history-picker", &history_label, |ui| {
        for (station, label) in stations(w, &owner) {
            if controls::selectable(
                ui,
                &format!("repair-station:{}::{}", station.definition, station.node),
                "repair-history-choice",
                &label,
                w.view.history_cursor.as_ref() == Some(&station),
            )
            .clicked()
            {
                choose_station(w, station);
                ui.close();
            }
        }
    });
    let inputs = reference_repair::inputs(&document, &owner).unwrap_or_default();
    for option in inputs {
        let selected = input
            .as_ref()
            .is_some_and(|i| i.storage == option.storage && i.path == option.path);
        if controls::selectable(
            ui,
            &option.semantic_id(),
            "repair-input",
            &format!("{} · {}", option.label, option.class.label()),
            selected,
        )
        .clicked()
        {
            select_input(w, option);
        }
    }
    let Some(input) = input else {
        return;
    };
    text(
        ui,
        "repair-replacement-summary",
        "repair-summary",
        &format!("Replacement for {}", input.label),
        "ready",
    );
    if matches!(input.class, Class::Topology(_) | Class::Plane) {
        if button(
            ui,
            "reference.repair-pick",
            "Pick in upstream model",
            current(w).is_some(),
        )
        .clicked()
        {
            action(w, "reference.repair-pick", None);
        }
        if button(
            ui,
            "reference.repair-use-selection",
            "Use selected reference",
            w.selected.is_some() && current(w).is_some(),
        )
        .clicked()
        {
            capture_selection(w);
        }
        if input.class == Class::Plane && input.path == "/plane_reference" {
            ui.horizontal_wrapped(|ui| {
                for plane in [
                    cad_core::reference_geometry::OriginPlane::Front,
                    cad_core::reference_geometry::OriginPlane::Top,
                    cad_core::reference_geometry::OriginPlane::Right,
                ] {
                    if button(
                        ui,
                        &format!("repair-origin:{}", plane.label()),
                        plane.label(),
                        true,
                    )
                    .clicked()
                    {
                        set_candidate(w, Replacement::OriginPlane { plane });
                    }
                }
            });
        }
    } else if input.class == Class::Sketch {
        if let Some(part) = document.parts.get(&input.owner.definition) {
            for sketch in part.sketches.values() {
                if controls::selectable(
                    ui,
                    &format!("repair-sketch:{}", sketch.id),
                    "repair-candidate-choice",
                    &sketch.name,
                    false,
                )
                .clicked()
                {
                    set_candidate(
                        w,
                        Replacement::Sketch {
                            sketch: sketch.id.clone(),
                        },
                    );
                }
            }
        }
    } else {
        let label = candidate
            .as_ref()
            .map(|c| source_label(&document, c))
            .unwrap_or_else(|| "Choose upstream feature".into());
        controls::menu_button(ui, "repair-candidate-picker", &label, |ui| {
            for part in document.parts.values() {
                for feature in &part.features {
                    let node = part.reference(&feature.id);
                    if reference_repair::node_compatible(&document, &input, &node).is_ok()
                        && controls::selectable(
                            ui,
                            &format!("repair-candidate:{}::{}", part.id, feature.id),
                            "repair-candidate-choice",
                            &format!("{} / {}", part.name, feature.name),
                            false,
                        )
                        .clicked()
                    {
                        set_candidate(w, node_candidate(&input, node));
                        ui.close();
                    }
                }
            }
        });
    }
    if let Some(Replacement::SketchElement { profile, elements }) = candidate.as_ref() {
        text(
            ui,
            "repair-element-summary",
            "repair-summary",
            "Choose the source sketch point or member explicitly",
            "ready",
        );
        for (element, label) in reference_repair::element_choices(&document, &input, profile) {
            let selected = elements.contains(&element);
            if controls::selectable(
                ui,
                &format!("repair-element:{}", element),
                "repair-element-choice",
                &label,
                selected,
            )
            .clicked()
            {
                let mut selected_elements = elements.clone();
                if input
                    .element
                    .as_ref()
                    .is_some_and(|binding| binding.class == reference_repair::ElementClass::Members)
                {
                    if selected {
                        selected_elements.retain(|id| id != &element);
                    } else {
                        selected_elements.push(element);
                    }
                } else {
                    selected_elements = vec![element];
                }
                set_candidate(
                    w,
                    Replacement::SketchElement {
                        profile: profile.clone(),
                        elements: selected_elements,
                    },
                );
            }
        }
    }
    if let Some(candidate) = candidate {
        text(
            ui,
            "repair-selected-replacement",
            "repair-candidate",
            &format!("Selected: {}", source_label(&document, &candidate)),
            "selected",
        );
    }
    if button(
        ui,
        "reference.repair-preview",
        "Preview repair",
        candidate_ready(w),
    )
    .clicked()
    {
        preview(w);
    }
    if w.reference_repair.as_ref().is_some_and(|s| s.repairs > 0) {
        text(
            ui,
            "repair-preview-status",
            "repair-summary",
            "The repair is in the shared draft. Current geometry validation must pass before Apply.",
            "previewed",
        );
        ui.horizontal(|ui| {
            if button(
                ui,
                "reference.repair-apply",
                "Apply",
                current_source_is_valid(w),
            )
            .clicked()
            {
                w.confirm_tool();
            }
            if button(ui, "reference.repair-cancel", "Cancel", true).clicked() {
                w.cancel_tool();
            }
        });
    } else if button(ui, "reference.repair-cancel", "Cancel", true).clicked() {
        w.cancel_tool();
    }
}
fn semantic(
    id: impl Into<String>,
    role: &str,
    label: impl Into<String>,
    state: &str,
    commands: Vec<String>,
    data: Value,
) -> SemanticElement {
    SemanticElement {
        id: id.into(),
        role: role.into(),
        label: label.into(),
        value: None,
        state: state.into(),
        commands,
        relationships: vec!["reference-repair-manager".into()],
        data,
    }
}
pub(crate) fn project(w: &Workspace) -> Vec<SemanticElement> {
    let document = w.session.visible();
    let failures = reference_repair::failures(document, current(w));
    let mut out = reference_repair::project(&failures);
    let Some(state) = &w.reference_repair else {
        return out;
    };
    out.push(SemanticElement {id:"reference-repair-manager".into(),role:"reference-repair".into(),label:"Repair References".into(),value:state.input.as_ref().map(|i|i.label.clone()),state:if state.picking{"picking"}else if current_source_is_valid(w){"previewed"}else{"editing"}.into(),commands:vec!["reference.repair-pick".into(),"reference.repair-use-selection".into(),"reference.repair-preview".into(),"reference.repair-apply".into(),"reference.repair-cancel".into()],relationships:state.owner.as_ref().map(|o|vec![o.definition.clone(),o.node.clone()]).unwrap_or_default(),data:serde_json::json!({"renderer":"reference-repair","version":reference_repair::CONTRACT_VERSION,"owner":state.owner,"input":state.input,"candidate":state.candidate,"repairs":state.repairs,"picking":state.picking,"upstream_current":current(w).is_some(),"current_validation_required":true,"shared_draft":true})});
    for (id, label) in [
        ("repair-title", "Repair a source reference"),
        (
            "repair-instructions",
            "Choose the input, pick a replacement, then preview. Apply commits one undoable repair; Cancel restores the starting document.",
        ),
    ] {
        out.push(semantic(
            id,
            "repair-summary",
            label,
            "ready",
            vec![],
            serde_json::json!({"version":reference_repair::CONTRACT_VERSION}),
        ));
    }
    if let Some(evaluation) = current(w).filter(|e| !e.valid) {
        out.push(semantic(
            "repair-upstream-status",
            "repair-message",
            "Upstream display only · the current model has failed features",
            "warning",
            vec![],
            serde_json::json!({"identity":evaluation.identity,"valid":false,"display_only":true}),
        ));
        for (index, diagnostic) in evaluation.diagnostics.iter().enumerate() {
            out.push(semantic(format!("repair-diagnostic:{index}"),"repair-message",&diagnostic.message,&diagnostic.severity,vec![],serde_json::json!({"target":diagnostic.target,"message":diagnostic.message,"severity":diagnostic.severity})));
        }
    }
    for part in document.parts.values() {
        for feature in &part.features {
            let owner = part.reference(&feature.id);
            if reference_repair::inputs(document, &owner).is_ok_and(|inputs| !inputs.is_empty()) {
                let selected = state.owner.as_ref() == Some(&owner);
                out.push(semantic(format!("repair-feature:{}::{}",part.id,feature.id),"repair-feature-choice",format!("{} / {}",part.name,feature.name),if selected{"selected"}else{"available"},vec!["reference.repair-feature".into()],serde_json::json!({"owner":owner,"failed":failures.iter().any(|f|f.owner==owner)})));
            }
        }
    }
    if let Some(owner) = &state.owner {
        for (station, label) in stations(w, owner) {
            let selected = w.view.history_cursor.as_ref() == Some(&station);
            out.push(semantic(
                format!("repair-station:{}::{}", station.definition, station.node),
                "repair-history-choice",
                label,
                if selected { "selected" } else { "available" },
                vec!["reference.repair-station".into()],
                serde_json::json!({"station":station,"current_source_cache":true,"view_only":true}),
            ));
        }
        for input in reference_repair::inputs(document, owner).unwrap_or_default() {
            let id = input.semantic_id();
            let selected = state.input.as_ref().is_some_and(|selected| {
                selected.storage == input.storage && selected.path == input.path
            });
            let element = semantic(
                id.clone(),
                "repair-input",
                format!("{} · {}", input.label, input.class.label()),
                if selected { "selected" } else { "available" },
                vec!["reference.repair-input".into()],
                serde_json::json!({"input":input}),
            );
            if let Some(existing) = out.iter_mut().find(|existing| existing.id == id) {
                *existing = element;
            } else {
                out.push(element);
            }
        }
    }
    if let Some(input) = &state.input {
        for part in document.parts.values() {
            for feature in &part.features {
                let node = part.reference(&feature.id);
                if reference_repair::node_compatible(document, input, &node).is_ok()
                    && !matches!(
                        input.class,
                        Class::Topology(_) | Class::Plane | Class::Sketch
                    )
                {
                    let candidate = node_candidate(input, node);
                    out.push(semantic(
                        format!("repair-candidate:{}::{}", part.id, feature.id),
                        "repair-candidate-choice",
                        format!("{} / {}", part.name, feature.name),
                        if state.candidate.as_ref() == Some(&candidate) {
                            "selected"
                        } else {
                            "available"
                        },
                        vec!["reference.repair-replacement".into()],
                        serde_json::json!({"candidate":candidate}),
                    ));
                }
            }
        }
        if input.class == Class::Sketch
            && let Some(part) = document.parts.get(&input.owner.definition)
        {
            for sketch in part.sketches.values() {
                let candidate = Replacement::Sketch {
                    sketch: sketch.id.clone(),
                };
                out.push(semantic(
                    format!("repair-sketch:{}", sketch.id),
                    "repair-candidate-choice",
                    &sketch.name,
                    "available",
                    vec!["reference.repair-replacement".into()],
                    serde_json::json!({"candidate":candidate}),
                ));
            }
        }
        if input.class == Class::Plane && input.path == "/plane_reference" {
            for plane in [
                cad_core::reference_geometry::OriginPlane::Front,
                cad_core::reference_geometry::OriginPlane::Top,
                cad_core::reference_geometry::OriginPlane::Right,
            ] {
                out.push(semantic(
                    format!("repair-origin:{}", plane.label()),
                    "repair-action",
                    plane.label(),
                    "available",
                    vec!["reference.repair-replacement".into()],
                    serde_json::json!({"candidate":Replacement::OriginPlane{plane}}),
                ));
            }
        }
        if let Some(Replacement::SketchElement { profile, elements }) = &state.candidate {
            for (element, label) in reference_repair::element_choices(document, input, profile) {
                let selected = elements.contains(&element);
                let mut choices = elements.clone();
                if input
                    .element
                    .as_ref()
                    .is_some_and(|binding| binding.class == reference_repair::ElementClass::Members)
                {
                    if selected {
                        choices.retain(|id| id != &element);
                    } else {
                        choices.push(element.clone());
                    }
                } else {
                    choices = vec![element.clone()];
                }
                let candidate = Replacement::SketchElement {
                    profile: profile.clone(),
                    elements: choices,
                };
                out.push(semantic(
                    format!("repair-element:{element}"),
                    "repair-element-choice",
                    label,
                    if selected { "selected" } else { "available" },
                    vec!["reference.repair-replacement".into()],
                    serde_json::json!({"candidate":candidate}),
                ));
            }
        }
    }
    for (command, label, enabled) in [
        (
            "reference.repair-pick",
            "Pick in upstream model",
            current(w).is_some(),
        ),
        (
            "reference.repair-use-selection",
            "Use selected reference",
            current(w).is_some() && w.selected.is_some(),
        ),
        (
            "reference.repair-preview",
            "Preview repair",
            candidate_ready(w),
        ),
        (
            "reference.repair-apply",
            "Apply",
            current_source_is_valid(w),
        ),
        ("reference.repair-cancel", "Cancel", true),
    ] {
        out.push(semantic(command,"repair-action",label,if enabled{"available"}else{"disabled"},vec![command.into()],serde_json::json!({"enabled":enabled,"shared_draft":true,"current_validation_required":command=="reference.repair-apply"})));
    }
    out
}
