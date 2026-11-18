//! Model-first assembly mating. Partial picks belong to the editor and never
//! create a provisional constraint against an arbitrary component.
use super::*;
use crate::presentation_controls as controls;
use cad_core::{assembly_geometry::ReferenceClass, presentation::SemanticElement};
use serde_json::{Value, json};

type EndpointPick = (Id, Reference);

pub(crate) struct Picker {
    assembly: Id,
    token: Id,
    first: Option<EndpointPick>,
    context: Option<Vec<Id>>,
    concentric: Option<Mate>,
}
impl Picker {
    /// Keep the next pick on a different participant in this assembly occurrence.
    pub(crate) fn accepts_candidate(
        &self,
        doc: &cad_core::Document,
        s: &viewport::Selection,
    ) -> bool {
        let Some(assembly) = doc.assemblies.get(&self.assembly) else {
            return false;
        };
        let Ok((participant, tail)) = participant_path(doc, assembly, &s.path) else {
            return false;
        };
        if self
            .first
            .as_ref()
            .is_some_and(|(first, _)| *first == participant)
        {
            return false;
        }
        if let Some(mate) = &self.concentric
            && ((participant != mate.first && participant != mate.second)
                || !matches!(s.reference.topology.as_str(), "face" | "plane"))
        {
            return false;
        }
        let parent = &s.path[..s.path.len() - tail.len() - 1];
        self.context.as_deref().is_none_or(|p| p == parent)
    }
    pub(crate) fn prompt(&self) -> &'static str {
        match (self.concentric.is_some(), self.first.is_some()) {
            (true, false) => "Pick a seating face on either component",
            (true, true) => "Pick the mating face on the other component",
            (false, false) => "Pick the first face, edge or datum",
            (false, true) => "Pick geometry on the other component",
        }
    }
}
fn start(w: &mut Workspace, assembly: &str, concentric: Option<Mate>) {
    let token = cad_core::id();
    w.start_tool();
    if w.tool_checkpoint.is_none() {
        return;
    }
    w.fields.insert("assembly-id".into(), assembly.into());
    if concentric.is_none() {
        w.fields.insert("mate-editor".into(), token.clone());
        w.json.clear();
    }
    w.confirm_requested = false;
    w.mate_picker = Some(Picker {
        assembly: assembly.into(),
        token,
        first: None,
        context: None,
        concentric,
    });
    w.open_editor("assembly");
}

pub fn new(w: &mut Workspace, assembly: &str) {
    if w.tool_checkpoint.is_some() {
        w.status = "Finish the current operation before starting a mate".into();
        return;
    }
    if w.session
        .visible()
        .assemblies
        .get(assembly)
        .is_none_or(|a| a.instances.len() < 2)
    {
        w.status = "Add two components before creating a mate".into();
        return;
    }
    let picks = if current_evaluation(w).is_err() {
        Vec::new()
    } else if w.geometry_selection.is_empty() {
        w.selected.iter().cloned().collect()
    } else {
        w.geometry_selection.clone()
    };
    start(w, assembly, None);
    if picks.len() > 2 {
        w.status = "Pick two references for this mate".into();
        return;
    }
    for pick in &picks {
        if let Err(e) = accept(w, pick) {
            w.status = e.to_string();
            break;
        }
    }
}

fn accept(w: &mut Workspace, selection: &viewport::Selection) -> Result<()> {
    let invalid = |s: &str| CadError::Invalid(s.into());
    let picker = w
        .mate_picker
        .as_ref()
        .ok_or_else(|| invalid("Start a mate first"))?;
    let assembly = w
        .session
        .visible()
        .assemblies
        .get(&picker.assembly)
        .ok_or_else(|| invalid("Assembly is missing"))?;
    let second = reference_from_pick(w, assembly, selection)?;
    let context_len = selection
        .path
        .len()
        .checked_sub(second.1.path.len() + 1)
        .ok_or_else(|| invalid("Pick geometry in this assembly"))?;
    let context = &selection.path[..context_len];
    if picker.context.as_deref().is_some_and(|old| old != context) {
        return Err(invalid(
            "Pick the other component in the same assembly occurrence",
        ));
    }
    if let Some(base) = &picker.concentric {
        if second.0 != base.first && second.0 != base.second {
            return Err(invalid(
                "Pick a face on one of the two concentric components",
            ));
        }
        if second.1.source.class() != ReferenceClass::Plane {
            return Err(invalid("Seating requires a planar face or plane"));
        }
    }
    let Some(first) = &picker.first else {
        let picker = w.mate_picker.as_mut().unwrap();
        picker.context = Some(context.to_vec());
        picker.first = Some(second);
        w.status = picker.prompt().into();
        return Ok(());
    };
    if first.0 == second.0 {
        return Err(invalid(
            "Pick the second reference on a different component",
        ));
    }
    let kind = if first.1.source.class() == ReferenceClass::Axis
        && second.1.source.class() == ReferenceClass::Axis
    {
        "concentric"
    } else {
        "coincident"
    };
    let mate = Mate {
        id: picker.token.clone(),
        kind: kind.into(),
        first: first.0.clone(),
        second: second.0,
        first_reference: Some(first.1.clone()),
        second_reference: Some(second.1),
        first_origin: [0.; 3],
        second_origin: [0.; 3],
        first_axis: [0., 0., 1.],
        second_axis: [0., 0., 1.],
        offset: "0 mm".into(),
        alignment: Alignment::Either,
        lock_rotation: false,
        rotation_offset: "0 deg".into(),
        relationship: None,
        lock_placement: None,
    };
    let owner = picker.assembly.clone();
    let anchor = picker.concentric.as_ref().map(|m| m.id.clone());
    begin(w, &owner, mate.clone());
    if let Some(anchor) = anchor {
        w.fields.insert("seat-guide-editor".into(), mate.id.clone());
        w.fields.insert("seat-guide-anchor".into(), anchor);
    }
    submit(w, &owner, mate);
    Ok(())
}
pub(crate) fn capture(w: &mut Workspace, selection: &viewport::Selection) -> bool {
    if let Err(error) = accept(w, selection) {
        w.status = error.to_string();
        return false;
    }
    true
}

fn actions(w: &Workspace) -> Vec<SemanticElement> {
    if w.dialog.as_deref() != Some("assembly") || w.settings.is_some() {
        return vec![];
    }
    let Some(owner) = w.fields.get("assembly-id") else {
        return vec![];
    };
    let Some(editor) = w.fields.get("mate-editor") else {
        return vec![];
    };
    let token = w.mate_picker.as_ref().map_or(editor, |p| &p.token);
    let mut result = vec![];
    let mut add = |key: &str, label: &str, command: Option<&str>| {
        let scope = json!({"contract":"tcad.presentation.mate-picker","version":1,"document":w.session.visible().id,"assembly":owner,"editor":editor,"token":token,"step": w.mate_picker.as_ref().and_then(|p|p.first.as_ref())});
        let mut data = scope.clone();
        if let Some(cmd) = command {
            data["actions"] =
                json!([{"command":cmd,"label":label,"enabled":true,"arguments":scope}]);
        }
        result.push(SemanticElement {
            id: format!("mate:{editor}/pick/{key}"),
            role: if command.is_some() {
                "mate-pick-action"
            } else {
                "mate-pick-prompt"
            }
            .into(),
            label: label.into(),
            value: None,
            state: "ready".into(),
            commands: command.into_iter().map(str::to_owned).collect(),
            relationships: vec![owner.clone(), editor.clone()],
            data,
        });
    };
    if let Some(picker) = &w.mate_picker {
        add("prompt", picker.prompt(), None);
        if let Some((id, reference)) = &picker.first
            && let Some(a) = w.session.visible().assemblies.get(owner)
            && let Some(i) = a.instances.get(id)
        {
            add(
                "first",
                &format!(
                    "{} · {}",
                    cad_core::occurrence_names::instance(w.session.visible(), owner, i),
                    source_name(w.session.visible(), &reference.source)
                ),
                None,
            );
            add("reset", "Change first reference", Some("mate.pick-first"));
        }
        if picker.concentric.is_some() {
            add("back", "Keep concentric mate", Some("mate.stop-seat"));
        } else {
            add(
                "manual",
                "Choose references manually…",
                Some("mate.manual-references"),
            );
        }
    } else if current_evaluation(w).is_ok()
        && parsed(w)
            .ok()
            .is_some_and(|m| m.kind == "concentric" && synchronized(w, owner, &m))
    {
        add("seat", "Seat against a face…", Some("mate.start-seat"));
    }
    result
}

pub(crate) fn action(w: &mut Workspace, command: &str, data: Option<&Value>) -> bool {
    if !matches!(
        command,
        "mate.pick-first" | "mate.stop-seat" | "mate.manual-references" | "mate.start-seat"
    ) {
        return false;
    }
    if w.confirm_requested
        || !actions(w).iter().any(|e| {
            e.action_descriptors()
                .iter()
                .any(|a| a.command == command && data.is_none_or(|d| *d == a.arguments))
        })
    {
        return true;
    }
    let Some(owner) = w.fields.get("assembly-id").cloned() else {
        return true;
    };
    match command {
        "mate.pick-first" => {
            if let Some(picker) = &mut w.mate_picker {
                picker.first = None;
                picker.context = None;
            }
        }
        "mate.stop-seat" => w.mate_picker = None,
        "mate.manual-references" => manual(w, &owner),
        "mate.start-seat" => {
            if let Ok(base) = parsed(w) {
                start(w, &owner, Some(base));
            }
        }
        _ => {}
    }
    true
}
pub(crate) fn show(ui: &mut egui::Ui, w: &mut Workspace) -> bool {
    let was_picking = w.mate_picker.is_some();
    for element in actions(w) {
        if let Some(a) = element.action_descriptors().into_iter().next() {
            if controls::button(
                ui,
                &element.id,
                &element.role,
                &[],
                &element.label,
                true,
                false,
            )
            .clicked()
            {
                action(w, &a.command, Some(&a.arguments));
            }
        } else {
            controls::label(
                ui,
                &element.id,
                &element.role,
                &[],
                &element.label,
                &element.state,
            );
        }
    }
    was_picking || w.mate_picker.is_some()
}

fn motion_anchor<'a>(
    w: &Workspace,
    assembly: &'a Assembly,
    solution: &assembly_geometry::EvaluatedAssembly,
    active: &'a Mate,
) -> &'a Mate {
    if active.kind != "coincident" {
        return active;
    }
    let Some(seat) = solution.frames.get(&active.id) else {
        return active;
    };
    if !seat.iter().all(|f| f.kind == FrameKind::Plane) {
        return active;
    }
    let axes = assembly.mates.iter().filter(|m| {
        m.kind == "concentric"
            && ((m.first == active.first && m.second == active.second)
                || (m.second == active.first && m.first == active.second))
    });
    if w.fields.get("seat-guide-editor") == Some(&active.id)
        && let Some(id) = w.fields.get("seat-guide-anchor")
        && let Some(axis) = axes
            .clone()
            .find(|m| &m.id == id && solution.frames.contains_key(id))
    {
        return axis;
    }
    // When a saved seat is reopened, a unique shared axis normal to its seating
    // plane identifies the shaft. Match directions in the same component frame.
    let mut aligned = axes.filter(|m| {
        solution.frames.get(&m.id).is_some_and(|frames| {
            let frame = &frames[usize::from(m.second == active.first)];
            DVec3::from_array(frame.axis)
                .dot(DVec3::from_array(seat[0].axis))
                .abs()
                > 1. - 1e-8
        })
    });
    match (aligned.next(), aligned.next()) {
        (Some(axis), None) => axis,
        _ => active,
    }
}

fn motion(w: &Workspace) -> Vec<SemanticElement> {
    if w.dialog.as_deref() != Some("assembly")
        || w.mate_picker
            .as_ref()
            .is_some_and(|p| p.concentric.is_none())
    {
        return vec![];
    }
    let Some(owner) = w.fields.get("assembly-id") else {
        return vec![];
    };
    let Ok(mate) = parsed(w) else { return vec![] };
    if !synchronized(w, owner, &mate) {
        return vec![];
    }
    let Ok(e) = current_evaluation(w) else {
        return vec![];
    };
    let Some(assembly) = w.session.visible().assemblies.get(owner) else {
        return vec![];
    };
    let mut result = vec![];
    let mut anchors = std::collections::BTreeMap::new();
    for (path, matrix, key) in crate::component_pattern_ui::assembly_contexts(w, e, owner) {
        let Some(solution) = e.assemblies.get(&key) else {
            continue;
        };
        let anchor = *anchors
            .entry(key)
            .or_insert_with(|| motion_anchor(w, assembly, solution, &mate));
        let Some(frames) = solution.frames.get(&anchor.id) else {
            continue;
        };
        for (id, frame) in [(&anchor.first, &frames[0]), (&anchor.second, &frames[1])] {
            let (Some(motion), Some(placement)) = (
                solution
                    .component_motion
                    .get(id)
                    .and_then(|m| m.joints.get(&anchor.id)),
                solution.placements.get(id),
            ) else {
                continue;
            };
            let Ok(frame) = frame.transformed(&placement.matrix()) else {
                continue;
            };
            let origin = crate::component_pattern_ui::matrix_point(matrix, frame.origin);
            let map = |axis: &[f64; 3]| -> [f64; 3] {
                std::array::from_fn(|r| (0..3).map(|c| matrix[r][c] * axis[c]).sum())
            };
            let moves = motion
                .translation_directions
                .iter()
                .map(map)
                .collect::<Vec<_>>();
            let turns = motion
                .rotation_directions
                .iter()
                .map(map)
                .collect::<Vec<_>>();
            let name = w
                .session
                .visible()
                .assemblies
                .get(owner)
                .and_then(|a| a.instances.get(id))
                .map(|i| cad_core::occurrence_names::instance(w.session.visible(), owner, i))
                .unwrap_or_else(|| "Component".into());
            let coupled = moves.len() + turns.len() > motion.degrees_of_freedom;
            let value = match (moves.len(), turns.len(), coupled) {
                (0, 0, _) => "Fixed to mate".into(),
                (0, 1, _) => "Can rotate".into(),
                (1, 0, _) => "Can slide".into(),
                (1, 1, true) => "Slide and rotation linked".into(),
                (1, 1, false) => "Can slide and rotate".into(),
                (_, _, true) => format!(
                    "{} degrees of freedom · linked motion",
                    motion.degrees_of_freedom
                ),
                _ => format!("{} degrees of freedom", motion.degrees_of_freedom),
            };
            result.push(SemanticElement {
                id: format!("mate:{}/motion/{}/{id}", mate.id, path.join("/")),
                role: "mate-motion-guide".into(),
                label: name,
                value: Some(value),
                commands: vec![],
                state: "evaluated".into(),
                relationships: vec![mate.id.clone(), id.clone()],
                data: json!({"contract":"tcad.presentation.mate-motion","version":2,"identity":e.identity,"origin":origin,"anchor_mate":anchor.id,"relative_to":if id == &anchor.first {&anchor.second} else {&anchor.first},"translation":moves,"rotation":turns,"coupled":coupled,"degrees_of_freedom":motion.degrees_of_freedom}),
            });
        }
    }
    result
}
pub(crate) fn semantics(w: &Workspace) -> Vec<SemanticElement> {
    let mut out = actions(w);
    out.extend(motion(w));
    out
}
pub(crate) fn overlay(w: &Workspace, ui: &egui::Ui, rect: egui::Rect) {
    let painter = ui.painter_at(rect);
    let mut labels: Vec<egui::Pos2> = Vec::new();
    for item in motion(w) {
        let props = controls::properties(&item.id, &item.role, &[], &item.state);
        if props.get("display").is_some_and(|v| v == "none")
            || props.get("visibility").is_some_and(|v| v == "hidden")
        {
            continue;
        }
        let Ok(origin) = serde_json::from_value::<[f64; 3]>(item.data["origin"].clone()) else {
            continue;
        };
        let Some(center) = viewport::project(w, rect, origin).filter(|p| rect.contains(*p)) else {
            continue;
        };
        let color = props
            .get("color")
            .and_then(|v| crate::ui::color(v))
            .unwrap_or(ui.visuals().selection.stroke.color);
        let stroke = egui::Stroke::new(controls::number(&props, "stroke-width", 2.), color);
        let length = crate::camera_views_ui::current(w).distance_mm * 0.04;
        for kind in ["translation", "rotation"] {
            let Ok(axes) = serde_json::from_value::<Vec<[f64; 3]>>(item.data[kind].clone()) else {
                continue;
            };
            for axis in axes {
                let n = DVec3::from_array(axis).normalize_or_zero();
                let pos = |v: DVec3| {
                    viewport::project(w, rect, (DVec3::from_array(origin) + v * length).to_array())
                };
                if kind == "translation" {
                    if let Some(end) = pos(n) {
                        painter.arrow(center, end - center, stroke);
                    }
                } else if n.length_squared() > 0. {
                    let a = n.any_orthonormal_vector() * 0.55;
                    let b = n.cross(a);
                    let mut prev = None;
                    for i in 0..=24 {
                        let theta = f64::from(i) * std::f64::consts::PI / 14.;
                        let next = pos(a * theta.cos() + b * theta.sin());
                        if let (Some(p), Some(q)) = (prev, next) {
                            if i == 24 {
                                painter.arrow(p, q - p, stroke);
                            } else {
                                painter.line_segment([p, q], stroke);
                            }
                        }
                        prev = next;
                    }
                }
            }
        }
        let mut label = center + egui::vec2(6., 9.);
        while labels
            .iter()
            .any(|p| (p.y - label.y).abs() < 14. && (p.x - label.x).abs() < 90.)
        {
            label.y += 15.;
        }
        labels.push(label);
        painter.text(
            label,
            egui::Align2::LEFT_TOP,
            format!("{} · {}", item.label, item.value.as_deref().unwrap_or("")),
            egui::FontId::proportional(controls::number(&props, "font-size", 11.)),
            color,
        );
    }
}
