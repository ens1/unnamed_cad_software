//! Docked authoring for sketch-positioned holes through the shared feature draft.
use crate::{Workspace, ui, viewport};
use bevy_egui::egui;
use cad_core::{
    Document, Expr, Feature, Id, NodeRef, Operation,
    commands::Command,
    holes::{CosmeticThread, Definition, End, Handedness, Head},
};

const DEFERRED: &str = "tcad:hole-preset:";
#[derive(serde::Serialize, serde::Deserialize)]
struct PendingHole {
    name: String,
    document: Id,
    definition: Id,
    body: Option<NodeRef>,
    positions: Option<(NodeRef, Vec<Id>)>,
    #[serde(default)]
    owns_positions: bool,
}
impl PendingHole {
    fn new(w: &Workspace, name: &str) -> Self {
        let positions = selected_sketch_points(w);
        let definition = positions
            .as_ref()
            .map(|(source, _)| source.definition.clone())
            .or_else(|| w.view.active_part.clone())
            .unwrap_or_default();
        let body = w
            .selected
            .as_ref()
            .and_then(|selected| selected.body.clone())
            .or_else(|| {
                w.sketch
                    .as_ref()
                    .and_then(|sketch| sketch.reference_body.clone())
            })
            .filter(|body| valid_body(w.session.visible(), &definition, body));
        Self {
            name: name.into(),
            document: w.session.visible().id.clone(),
            definition,
            body,
            positions,
            owns_positions: false,
        }
    }
    fn encode(&self) -> serde_json::Result<String> {
        serde_json::to_string(self).map(|payload| format!("{DEFERRED}{payload}"))
    }
}
fn valid_body(doc: &Document, owner: &str, reference: &NodeRef) -> bool {
    reference.definition == owner
        && doc.parts.get(owner).is_some_and(|part| {
            part.features.iter().any(|feature| {
                feature.id == reference.node
                    && !cad_core::configuration::feature_suppressed(doc, owner, feature)
                    && feature.operation.produces_solid()
            })
        })
}
pub fn is_preset(name: &str) -> bool {
    name.starts_with(DEFERRED)
        || matches!(
            name,
            "Drilled Holes" | "Simple Hole" | "Counterbore" | "Countersink" | "Tapped Hole"
        )
}

fn positions(doc: &Document, owner: &str, current: &str) -> Vec<(NodeRef, String)> {
    doc.parts
        .get(owner)
        .into_iter()
        .flat_map(|part| {
            part.features
                .iter()
                .take_while(|feature| feature.id != current)
                .filter(move |feature| {
                    !cad_core::configuration::feature_suppressed(doc, owner, feature)
                        && matches!(
                            feature.operation,
                            Operation::Profile {
                                profile: cad_core::Profile::Sketch { .. },
                                ..
                            }
                        )
                })
                .map(move |feature| (part.reference(&feature.id), feature.name.clone()))
        })
        .collect()
}
fn bodies(doc: &Document, owner: &str, current: &str) -> Vec<(NodeRef, String)> {
    doc.parts
        .get(owner)
        .into_iter()
        .flat_map(|part| {
            part.features
                .iter()
                .take_while(|feature| feature.id != current)
                .filter(move |feature| {
                    !cad_core::configuration::feature_suppressed(doc, owner, feature)
                        && feature.operation.produces_solid()
                })
                .map(move |feature| (part.reference(&feature.id), feature.name.clone()))
        })
        .collect()
}
fn selector(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut NodeRef,
    choices: Vec<(NodeRef, String)>,
) -> bool {
    let mut changed = false;
    egui::ComboBox::from_label(label)
        .selected_text(
            choices
                .iter()
                .find(|(reference, _)| reference == value)
                .map(|(_, label)| label.as_str())
                .unwrap_or("Missing source"),
        )
        .show_ui(ui, |ui| {
            for (reference, label) in choices {
                changed |= ui.selectable_value(value, reference, label).changed();
            }
        });
    changed
}
fn expression(ui: &mut egui::Ui, label: &str, value: &mut Expr) -> bool {
    ui.label(label);
    ui.text_edit_singleline(&mut value.0).changed()
}
fn source_points(w: &Workspace, source: &NodeRef) -> Vec<(Id, [f64; 2])> {
    let doc = w.session.visible();
    let Some(part) = doc.parts.get(&source.definition) else {
        return vec![];
    };
    let Some(feature) = part
        .features
        .iter()
        .find(|feature| feature.id == source.node)
    else {
        return vec![];
    };
    let Operation::Profile {
        profile: cad_core::Profile::Sketch { sketch },
        ..
    } = &feature.operation
    else {
        return vec![];
    };
    let Some(sketch) = part.sketches.get(sketch) else {
        return vec![];
    };
    let evaluated = viewport::display_evaluation(w).and_then(|evaluation| {
        evaluation
            .sketches
            .get(&format!("{}::{}", source.definition, source.node))
    });
    let suppressed = cad_core::sketch_transform::suppressed_points(sketch);
    sketch
        .points
        .iter()
        .filter(|(point, _)| !suppressed.contains(*point))
        .map(|(id, point)| {
            (
                id.clone(),
                evaluated
                    .and_then(|sketch| sketch.points.get(id))
                    .copied()
                    .unwrap_or(*point),
            )
        })
        .collect()
}

fn selected_sketch_points(w: &Workspace) -> Option<(NodeRef, Vec<Id>)> {
    let sketch = w.sketch.as_ref()?;
    let selected = sketch
        .selection
        .iter()
        .filter_map(|entity| {
            if let cad_core::sketch_actions::SketchEntity::Point { point } = entity {
                Some(point.clone())
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    let source = NodeRef {
        definition: sketch.definition.clone(),
        node: sketch.feature.clone(),
    };
    Some((source, selected))
}

/// Keep deliberate hole-center placements and explicit existing point picks.
/// Sketch corners or unrelated members are never inferred as hole centers.
pub fn remember_positions(w: &mut Workspace) {
    let Some(pending) = w
        .after_confirm
        .as_deref()
        .filter(|pending| is_preset(pending))
    else {
        return;
    };
    let mut pending = if let Some(payload) = pending.strip_prefix(DEFERRED) {
        let Ok(pending) = serde_json::from_str::<PendingHole>(payload) else {
            return;
        };
        pending
    } else {
        PendingHole::new(w, pending)
    };
    let Some(mut selected) = selected_sketch_points(w) else {
        return;
    };
    if pending.document != w.session.visible().id || pending.definition != selected.0.definition {
        w.after_confirm = None;
        return;
    }
    if pending.owns_positions
        && let Some((source, points)) = &pending.positions
        && source == &selected.0
        && let Some(sketch) = &w.sketch
    {
        selected.1.extend(
            points
                .iter()
                .filter(|point| sketch.keys.contains(point))
                .cloned(),
        );
        selected.1.sort();
        selected.1.dedup();
    }
    pending.positions = Some(selected);
    if let Ok(payload) = pending.encode() {
        w.after_confirm = Some(payload);
    }
}

pub fn remember_placed_center(w: &mut Workspace, point: Id) {
    let Some(payload) = w
        .after_confirm
        .as_deref()
        .and_then(|pending| pending.strip_prefix(DEFERRED))
    else {
        return;
    };
    let Ok(mut pending) = serde_json::from_str::<PendingHole>(payload) else {
        return;
    };
    let Some(sketch) = &w.sketch else { return };
    if !pending.owns_positions
        || pending.definition != sketch.definition
        || pending.document != w.session.visible().id
    {
        return;
    }
    let source = NodeRef {
        definition: sketch.definition.clone(),
        node: sketch.feature.clone(),
    };
    let Some((positions, points)) = &mut pending.positions else {
        return;
    };
    if positions != &source || !sketch.keys.contains(&point) {
        return;
    }
    if !points.contains(&point) {
        points.push(point);
    }
    if let Ok(payload) = pending.encode() {
        w.after_confirm = Some(payload);
    }
}

/// Finish validated positions by opening the feature in the same transaction.
/// The shared checkpoint survives until the hole's OK or Cancel.
pub fn complete_positions(w: &mut Workspace) -> bool {
    if w.sketch.is_some() {
        return false;
    }
    let Some(pending) = w
        .after_confirm
        .as_deref()
        .filter(|pending| is_preset(pending))
        .map(str::to_owned)
    else {
        return false;
    };
    w.after_confirm = None;
    w.confirm_requested = false;
    preset(w, &pending);
    true
}

pub fn positions_are_ready(w: &mut Workspace) -> bool {
    let prompt = sketch_semantics(w).into_iter().next();
    if prompt.is_some_and(|prompt| prompt.data["point_count"].as_u64() == Some(0)) {
        w.confirm_requested = false;
        w.status = "Place at least one hole center before finishing positions".into();
        return false;
    }
    true
}

pub fn sketch_semantics(w: &Workspace) -> Vec<cad_core::presentation::SemanticElement> {
    let Some(sketch) = &w.sketch else {
        return vec![];
    };
    let Some(payload) = w
        .after_confirm
        .as_deref()
        .and_then(|pending| pending.strip_prefix(DEFERRED))
    else {
        return vec![];
    };
    let Ok(pending) = serde_json::from_str::<PendingHole>(payload) else {
        return vec![];
    };
    if pending.document != w.session.visible().id || pending.definition != sketch.definition {
        return vec![];
    }
    let mut points = pending
        .positions
        .as_ref()
        .filter(|(source, _)| source.node == sketch.feature)
        .map(|(_, points)| {
            points
                .iter()
                .filter(|point| sketch.keys.contains(point))
                .cloned()
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if let Some((_, selected)) = selected_sketch_points(w) {
        points.extend(selected);
        points.sort();
        points.dedup();
    }
    let prompt = if sketch.choosing_plane {
        format!(
            "{} — choose a plane or planar face for the centers",
            pending.name
        )
    } else {
        format!("{} — place centers, then Finish Positions", pending.name)
    };
    let can_finish = !points.is_empty()
        && !sketch.choosing_plane
        && sketch.error.is_none()
        && sketch.edit.is_none();
    vec![cad_core::presentation::SemanticElement {
        id: format!("{}::hole-positions", sketch.sketch_id),
        role: "sketch-operation-prompt".into(),
        label: prompt,
        value: Some(format!(
            "{} centers placed · dimensions and relations stay editable",
            points.len()
        )),
        state: if sketch.choosing_plane {
            "choose-plane"
        } else if sketch.error.is_some() {
            "error"
        } else if points.is_empty() {
            "place-centers"
        } else {
            "ready-to-finish"
        }
        .into(),
        commands: vec![
            "sketch.tool.point".into(),
            "sketch.finish".into(),
            "sketch.cancel".into(),
        ],
        relationships: std::iter::once(sketch.sketch_id.clone())
            .chain(points.iter().cloned())
            .collect(),
        data: serde_json::json!({"contract":"tcad.sketch-operation-workflow","version":1,"document":pending.document,"definition":pending.definition,"sketch":sketch.sketch_id,"operation":pending.name,"body":pending.body,"positions":pending.positions,"completed_points":points,"point_count":points.len(),"completion_label":"Finish Positions","completion_scope":"hole-position-sketch","next_operation":pending.name,"requires_current_validation":true,"shared_checkpoint":true,"classes":["operation-prompt","hole-position-prompt"],"actions":[{"command":"sketch.tool.point","label":"Place Centers","enabled":!sketch.choosing_plane},{"command":"sketch.finish","label":"Finish Positions","enabled":can_finish,"disabled_reason":"Place at least one center and resolve sketch errors"},{"command":"sketch.cancel","label":format!("Cancel {}",pending.name),"enabled":true}]}),
    }]
}

pub fn sketch_prompt(ui: &mut egui::Ui, w: &mut Workspace) {
    for element in sketch_semantics(w) {
        crate::presentation_controls::label(
            ui,
            &element.id,
            &element.role,
            &["operation-prompt", "hole-position-prompt"],
            &element.label,
            &element.state,
        );
        if let Some(value) = &element.value {
            crate::presentation_controls::label(
                ui,
                &format!("{}:status", element.id),
                "sketch-operation-status",
                &["operation-prompt"],
                value,
                &element.state,
            );
        }
        ui.horizontal_wrapped(|ui| {
            for descriptor in element.action_descriptors() {
                if crate::presentation_controls::button(
                    ui,
                    &descriptor.id,
                    "sketch-operation-action",
                    &["operation-action"],
                    &descriptor.label,
                    descriptor.enabled,
                    false,
                )
                .clicked()
                {
                    crate::sketch_ui::action(w, &descriptor.command);
                }
            }
        });
    }
}

fn begin_positions(w: &mut Workspace, mut pending: PendingHole, body: NodeRef) {
    crate::sketch::begin(w, None);
    if w.sketch.is_some() {
        crate::sketch::set_tool(w, crate::sketch::Tool::Point);
        pending.body = Some(body);
        pending.positions = selected_sketch_points(w);
        pending.owns_positions = true;
        match pending.encode() {
            Ok(payload) => w.after_confirm = Some(payload),
            Err(error) => {
                w.status = error.to_string();
                return;
            }
        }
        w.status = format!(
            "{} — place and constrain centers, then Finish Positions to set the hole size",
            pending.name
        );
    }
}

pub fn preset(w: &mut Workspace, name: &str) {
    if w.fields.contains_key("configuration-editor")
        || w.fields.contains_key("component-pattern-editor")
    {
        w.status = "Confirm or cancel the current configuration or component pattern first".into();
        return;
    }
    let handoff = name.starts_with(DEFERRED);
    let decoded = name
        .strip_prefix(DEFERRED)
        .map(serde_json::from_str::<PendingHole>)
        .transpose();
    let mut pending = match decoded {
        Ok(Some(pending)) => {
            if pending.document != w.session.visible().id
                || w.view.active_part.as_deref() != Some(pending.definition.as_str())
            {
                w.status = "The pending hole action belongs to another document or part; invoke it again in the intended part".into();
                return;
            }
            pending
        }
        Ok(None) => {
            let mut pending = PendingHole::new(w, name);
            if let Some(previous) = w
                .after_confirm
                .as_deref()
                .and_then(|value| value.strip_prefix(DEFERRED))
                .and_then(|value| serde_json::from_str::<PendingHole>(value).ok())
                .filter(|previous| {
                    previous.owns_positions
                        && previous.document == pending.document
                        && previous.definition == pending.definition
                })
            {
                pending.body = previous.body;
                pending.positions = previous.positions;
                pending.owns_positions = true;
            }
            pending
        }
        Err(error) => {
            w.status = format!("Deferred hole selection is invalid: {error}");
            return;
        }
    };
    if !matches!(
        pending.name.as_str(),
        "Drilled Holes" | "Simple Hole" | "Counterbore" | "Countersink" | "Tapped Hole"
    ) {
        w.status = "The pending hole preset is unavailable; choose a hole command again".into();
        return;
    }
    crate::history::latest(w);
    if w.confirm_requested {
        match pending.encode() {
            Ok(value) => {
                w.after_confirm = Some(value);
                remember_positions(w);
            }
            Err(error) => w.status = error.to_string(),
        }
        return;
    }
    if w.sketch.is_some() {
        if !pending.owns_positions {
            pending.positions = selected_sketch_points(w);
        }
        pending.owns_positions = true;
        let deferred = match pending.encode() {
            Ok(value) => value,
            Err(error) => {
                w.status = error.to_string();
                return;
            }
        };
        w.after_confirm = Some(deferred);
        remember_positions(w);
        if !positions_are_ready(w) {
            crate::sketch::set_tool(w, crate::sketch::Tool::Point);
            w.status = format!(
                "{} — place and constrain centers, then Finish Positions",
                pending.name
            );
            return;
        }
        w.confirm_tool();
        return;
    }
    if w.tool_checkpoint.is_some() && !handoff {
        w.status = "Confirm or cancel the current feature first".into();
        return;
    }
    let Some(mut part) = w
        .view
        .active_part
        .as_ref()
        .and_then(|id| w.session.visible().parts.get(id))
        .cloned()
    else {
        w.status = "Create or select a part first".into();
        return;
    };
    if w.session
        .visible()
        .libraries
        .values()
        .any(|link| link.pinned_definitions.contains_key(&part.id))
    {
        w.status = "Make the library component unique before creating holes".into();
        return;
    }
    if pending
        .body
        .as_ref()
        .is_some_and(|body| !valid_body(w.session.visible(), &part.id, body))
    {
        w.status = "The pending hole target body is missing or suppressed; select its replacement and invoke the hole command again".into();
        return;
    }
    let body = pending.body.clone().or_else(|| {
        w.selected
            .as_ref()
            .and_then(|selected| selected.body.as_ref())
            .filter(|reference| {
                reference.definition == part.id
                    && part.features.iter().any(|feature| {
                        feature.id == reference.node
                            && !cad_core::configuration::feature_suppressed(
                                w.session.visible(),
                                &part.id,
                                feature,
                            )
                            && feature.operation.produces_solid()
                    })
            })
            .cloned()
            .or_else(|| {
                part.features
                    .iter()
                    .find(|feature| {
                        w.view.active_node.as_ref() == Some(&feature.id)
                            && !cad_core::configuration::feature_suppressed(
                                w.session.visible(),
                                &part.id,
                                feature,
                            )
                            && feature.operation.produces_solid()
                    })
                    .or_else(|| {
                        part.features.iter().rev().find(|feature| {
                            !cad_core::configuration::feature_suppressed(
                                w.session.visible(),
                                &part.id,
                                feature,
                            ) && part.outputs.contains(&feature.id)
                                && feature.operation.produces_solid()
                        })
                    })
                    .map(|feature| part.reference(&feature.id))
            })
    });
    let Some(body) = body else {
        w.status = "Create a solid body from a sketch before drilling holes".into();
        return;
    };
    let choices = positions(w.session.visible(), &part.id, "");
    if pending
        .positions
        .as_ref()
        .is_some_and(|(source, _)| !choices.iter().any(|(reference, _)| reference == source))
    {
        w.status = "The pending hole-position sketch is missing or suppressed; invoke the hole command again with its replacement".into();
        return;
    }
    let remembered = pending.positions.clone();
    let active_position_sketch = choices
        .iter()
        .any(|(reference, _)| w.view.active_node.as_ref() == Some(&reference.node));
    if remembered.is_none()
        && !active_position_sketch
        && w.selected.as_ref().is_some_and(|selected| {
            selected.definition == part.id && selected.reference.topology == "face"
        })
        && crate::sketch::selected_plane(w, &part.id).is_some()
    {
        begin_positions(w, pending, body);
        return;
    }
    let source = remembered
        .as_ref()
        .map(|(source, _)| source.clone())
        .or_else(|| {
            choices
                .iter()
                .find(|(reference, _)| w.view.active_node.as_ref() == Some(&reference.node))
                .or_else(|| choices.last())
                .map(|(source, _)| source.clone())
        });
    let Some(positions) = source else {
        begin_positions(w, pending, body);
        return;
    };
    let available = source_points(w, &positions);
    let points = remembered
        .filter(|(source, _)| *source == positions)
        .map(|(_, points)| {
            points
                .into_iter()
                .filter(|point| available.iter().any(|(id, _)| id == point))
                .collect()
        })
        .unwrap_or_default();
    let name = pending.name.as_str();
    let head = match name {
        "Counterbore" => Head::Counterbore {
            diameter: "10 mm".into(),
            depth: "3 mm".into(),
        },
        "Countersink" => Head::Countersink {
            diameter: "10 mm".into(),
            angle: "90 deg".into(),
        },
        _ => Head::Simple,
    };
    let definition = Definition {
        body,
        positions,
        points,
        diameter: "5 mm".into(),
        depth: "10 mm".into(),
        end: End::Blind,
        head,
        thread: (name == "Tapped Hole").then(|| {
            Box::new(CosmeticThread {
                nominal_diameter: "6 mm".into(),
                pitch: "1 mm".into(),
                tap_drill_diameter: "5 mm".into(),
                handedness: Handedness::Right,
                depth: "8 mm".into(),
                end: End::Blind,
            })
        }),
        reverse: false,
        start_offset: "0 mm".into(),
    };
    let feature = Feature {
        id: cad_core::id(),
        name: format!(
            "{}{}",
            name,
            part.features
                .iter()
                .filter(|feature| feature.name.starts_with(name))
                .count()
                + 1
        ),
        operation: Operation::DrilledHoles {
            definition: Box::new(definition),
        },
        suppressed: false,
    };
    for source in feature
        .operation
        .consumed_outputs()
        .iter()
        .filter(|reference| reference.definition == part.id)
    {
        part.outputs.retain(|id| id != &source.node);
    }
    part.outputs.push(feature.id.clone());
    part.features.push(feature.clone());
    w.start_tool();
    w.dispatch(Command::PutPart {
        part,
        assembly: None,
        placement: Default::default(),
    });
    if !w
        .session
        .visible()
        .parts
        .values()
        .any(|part| part.features.iter().any(|node| node.id == feature.id))
    {
        let error = w.status.clone();
        w.cancel_tool();
        w.status = error;
        return;
    }
    w.view.active_node = Some(feature.id.clone());
    w.status = format!(
        "{} · {} positions selected · changes preview before OK Feature",
        feature.name,
        match &feature.operation {
            Operation::DrilledHoles { definition } => definition.points.len(),
            _ => 0,
        }
    );
    ui::feature_dialog(w, Some(feature));
}

pub fn fields(
    ui: &mut egui::Ui,
    w: &Workspace,
    current: &str,
    definition: &mut Definition,
) -> bool {
    let doc = w.session.visible();
    let owner = w
        .view
        .active_part
        .clone()
        .unwrap_or_else(|| definition.body.definition.clone());
    let mut changed = selector(
        ui,
        "Target Body",
        &mut definition.body,
        bodies(doc, &owner, current),
    );
    if selector(
        ui,
        "Position Sketch",
        &mut definition.positions,
        positions(doc, &owner, current),
    ) {
        definition.points.clear();
        changed = true;
    }
    let points = source_points(w, &definition.positions);
    egui::CollapsingHeader::new(format!("Positions · {} selected", definition.points.len())).default_open(true).show(ui, |ui| {
        if points.is_empty() { ui.weak("Add native sketch points for the hole centers."); }
        egui::ScrollArea::vertical().id_salt("hole-position-points").max_height(170.).show(ui, |ui| {
            for (index, (id, point)) in points.iter().enumerate() {
                let mut selected = definition.points.contains(id);
                if ui.checkbox(&mut selected, format!("Point {} · ({}, {})", index + 1, doc.settings.format_length(point[0]), doc.settings.format_length(point[1]))).changed() { if selected { definition.points.push(id.clone()); } else { definition.points.retain(|point| point != id); } changed = true; }
            }
        });
        if definition.points.iter().any(|point| !points.iter().any(|(id, _)| id == point)) {
            ui.weak("A selected point was removed. Choose its replacement or clear missing points.");
            if ui.small_button("Clear Missing Points").clicked() { definition.points.retain(|point| points.iter().any(|(id, _)| id == point)); changed = true; }
        }
        if ui.small_button("Clear Position Selection").clicked() { definition.points.clear(); changed = true; }
        ui.small("Centers placed by the hole-position workflow are preselected. Choose additional centers explicitly; sketch corners are never selected automatically.");
    });
    let mut threaded = definition.thread.is_some();
    if ui
        .checkbox(&mut threaded, "Cosmetic Thread / Tapped Hole")
        .changed()
    {
        definition.thread = threaded.then(|| {
            Box::new(CosmeticThread {
                nominal_diameter: Expr(format!("({}) + 1 mm", definition.diameter.0)),
                pitch: "1 mm".into(),
                tap_drill_diameter: definition.diameter.clone(),
                handedness: Handedness::Right,
                depth: Expr(format!("({}) * 0.5", definition.depth.0)),
                end: if definition.end == End::ThroughAll {
                    End::ThroughAll
                } else {
                    End::Blind
                },
            })
        });
        changed = true;
    }
    if definition.thread.is_none() {
        changed |= expression(ui, "Hole Diameter", &mut definition.diameter);
    }
    egui::ComboBox::from_label("End Condition")
        .selected_text(if definition.end == End::Blind {
            "Blind"
        } else {
            "Through All"
        })
        .show_ui(ui, |ui| {
            changed |= ui
                .selectable_value(&mut definition.end, End::Blind, "Blind")
                .changed();
            changed |= ui
                .selectable_value(&mut definition.end, End::ThroughAll, "Through All")
                .changed();
        });
    if definition.end == End::Blind {
        changed |= expression(ui, "Hole Depth", &mut definition.depth);
    }
    let mut style = match definition.head {
        Head::Simple => 0,
        Head::Counterbore { .. } => 1,
        Head::Countersink { .. } => 2,
    };
    let previous = style;
    egui::ComboBox::from_label("Hole Style")
        .selected_text(definition.head.name())
        .show_ui(ui, |ui| {
            for (value, label) in [(0, "Simple Hole"), (1, "Counterbore"), (2, "Countersink")] {
                ui.selectable_value(&mut style, value, label);
            }
        });
    if style != previous {
        definition.head = match style {
            1 => Head::Counterbore {
                diameter: "10 mm".into(),
                depth: "3 mm".into(),
            },
            2 => Head::Countersink {
                diameter: "10 mm".into(),
                angle: "90 deg".into(),
            },
            _ => Head::Simple,
        };
        changed = true;
    }
    match &mut definition.head {
        Head::Simple => {}
        Head::Counterbore { diameter, depth } => {
            changed |= expression(ui, "Counterbore Diameter", diameter);
            changed |= expression(ui, "Counterbore Depth", depth);
        }
        Head::Countersink { diameter, angle } => {
            changed |= expression(ui, "Countersink Diameter", diameter);
            changed |= expression(ui, "Included Angle", angle);
        }
    }
    if let Some(thread) = &mut definition.thread {
        egui::CollapsingHeader::new("Thread · Cosmetic").default_open(true).show(ui, |ui| {
            changed |= expression(ui, "Nominal Diameter", &mut thread.nominal_diameter);
            changed |= expression(ui, "Pitch", &mut thread.pitch);
            changed |= expression(ui, "Tap-Drill Diameter", &mut thread.tap_drill_diameter);
            egui::ComboBox::from_label("Handedness")
                .selected_text(thread.handedness.abbreviation())
                .show_ui(ui, |ui| {
                    changed |= ui.selectable_value(&mut thread.handedness, Handedness::Right, "Right Hand").changed();
                    changed |= ui.selectable_value(&mut thread.handedness, Handedness::Left, "Left Hand").changed();
                });
            egui::ComboBox::from_label("Thread End Condition")
                .selected_text(if thread.end == End::Blind {"Blind"} else {"Through All"})
                .show_ui(ui, |ui| {
                    changed |= ui.selectable_value(&mut thread.end, End::Blind, "Blind").changed();
                    changed |= ui.selectable_value(&mut thread.end, End::ThroughAll, "Through All").changed();
                });
            if thread.end == End::Blind {
                changed |= expression(ui, "Thread Depth Below Entry Head", &mut thread.depth);
            }
            ui.small("Preview and solid export contain the exact tap-drill bore. The thread is cosmetic; drawings retain its nominal diameter, pitch, hand and depth. Dimensions are explicit and have no automatic thread-standard designation.");
        });
    }
    changed |= ui
        .checkbox(&mut definition.reverse, "Reverse Entry Direction")
        .changed();
    changed |= expression(ui, "Start Offset Along Entry", &mut definition.start_offset);
    ui.small("Entry follows the current sketch normal into the face. Blind bottoms are flat; drill-tip and standards-table intent are not inferred.");
    changed
}
