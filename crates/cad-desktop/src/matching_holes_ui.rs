//! Pick a cover face and the holes it fits against in the assembly viewport.
use crate::{Workspace, presentation_controls as controls, viewport};
use bevy_egui::egui;
use cad_core::{
    CadError, Document, Expr, Feature, Id, NodeRef, Operation, Part, PlacedPart, Plane, Profile,
    Result, Sketch, TopologyRef,
    commands::Command,
    holes,
    matching_holes::{self, Center},
    presentation::SemanticElement,
    protocol::{EdgeCenter, Evaluation},
    reference_geometry::Axis,
};
use serde_json::{Value, json};

const TOOL: &str = "matching-holes";
const DIAMETER: &str = "match-holes.diameter";
type Matrix = [[f64; 4]; 4];

#[derive(Clone)]
struct Target {
    body: NodeRef,
    face: TopologyRef,
    plane: Plane,
    matrix: Matrix,
}
#[derive(Clone)]
struct Pick {
    point: Id,
    center: Center,
    uv: [f64; 2],
    nominal: f64,
    selection: viewport::Selection,
}
pub struct Editor {
    owner: Id,
    path: Vec<Id>,
    sketch: Id,
    profile: Id,
    feature: Id,
    target: Option<Target>,
    picks: Vec<Pick>,
    follow: bool,
    diameter: Expr,
    custom_diameter: bool,
    submitted: bool,
    error: Option<String>,
}
fn invalid(text: &str) -> CadError {
    CadError::Invalid(text.into())
}
pub fn active(w: &Workspace) -> bool {
    w.matching_holes.is_some()
}
pub fn ready(w: &Workspace) -> bool {
    w.matching_holes
        .as_ref()
        .is_none_or(|s| s.submitted && s.error.is_none())
}
pub fn preview_focus(w: &Workspace) -> Option<NodeRef> {
    let s = w.matching_holes.as_ref()?;
    s.submitted.then(|| NodeRef {
        definition: s.owner.clone(),
        node: s.feature.clone(),
    })
}
pub fn reset(w: &mut Workspace) {
    if w.matching_holes.take().is_some() {
        w.geometry_selection.clear();
        w.selected = None;
    }
}
pub fn start_reason(w: &Workspace) -> Option<&'static str> {
    let path = w.view.editing_occurrence.as_ref();
    if w.tool_checkpoint.is_some()
        || w.sketch.is_some()
        || w.settings.is_some()
        || w.session.draft.is_some()
    {
        return Some("Finish the current edit first.");
    }
    if path.is_none_or(Vec::is_empty) || !crate::workspace_controls::in_place(w) {
        return Some("Edit the cover in place in an assembly.");
    }
    if cad_core::library::is_definition_pinned(
        w.session.visible(),
        w.view.active_part.as_deref().unwrap_or_default(),
    ) {
        return Some("Fork the linked cover before editing it.");
    }
    if viewport::selection_evaluation(w).is_none() {
        return Some("Wait for the current assembly to rebuild.");
    }
    None
}
fn instance<'a>(eval: &'a Evaluation, path: &[Id]) -> Option<&'a PlacedPart> {
    eval.instances.iter().find(|i| i.path == path)
}
fn face_target(w: &Workspace, pick: &viewport::Selection) -> Option<Target> {
    let s = w.matching_holes.as_ref()?;
    if s.path != pick.path || s.owner != pick.definition || pick.reference.topology != "face" {
        return None;
    }
    let eval = viewport::selection_evaluation(w)?;
    let inst = instance(eval, &s.path)?;
    let face = eval
        .meshes
        .get(&inst.mesh_key)?
        .faces
        .iter()
        .find(|f| f.reference == pick.reference && f.body == pick.body)?;
    let body = face.body.clone()?;
    let part = w.tool_checkpoint.as_ref()?.visible().parts.get(&s.owner)?;
    if !part.outputs.contains(&body.node)
        || !part
            .features
            .iter()
            .any(|f| f.id == body.node && f.operation.produces_solid())
    {
        return None;
    }
    Some(Target {
        body,
        face: face.reference.clone(),
        plane: face.plane.clone()?,
        matrix: inst.matrix,
    })
}
pub fn start(w: &mut Workspace) {
    if let Some(reason) = start_reason(w) {
        w.status = reason.into();
        return;
    }
    let path = w.view.editing_occurrence.clone().unwrap_or_default();
    let owner = w.view.active_part.clone().unwrap_or_default();
    let Some(inst) = viewport::selection_evaluation(w).and_then(|e| instance(e, &path)) else {
        w.status = "Show and rebuild the cover occurrence first.".into();
        return;
    };
    if inst.configuration.as_ref() != w.session.visible().active_configurations.get(&owner) {
        w.status = "Activate the cover occurrence’s configuration before editing it.".into();
        return;
    }
    let first = w.selected.clone();
    w.start_tool();
    w.open_editor(TOOL);
    w.matching_holes = Some(Editor {
        owner,
        path,
        sketch: cad_core::id(),
        profile: cad_core::id(),
        feature: cad_core::id(),
        target: None,
        picks: vec![],
        follow: true,
        diameter: "6.6 mm".into(),
        custom_diameter: false,
        submitted: false,
        error: None,
    });
    if let Some(target) = first.as_ref().and_then(|s| face_target(w, s)) {
        w.matching_holes.as_mut().unwrap().target = Some(target);
    }
    w.selected = None;
    w.geometry_selection.clear();
    w.rendered_key.clear();
    w.status = prompt(w).into();
}
fn prompt(w: &Workspace) -> &'static str {
    match w.matching_holes.as_ref() {
        Some(s) if s.target.is_some() => {
            "Click the base’s hole rims or bore walls. Click again to remove."
        }
        _ => "Pick the face to drill on the cover.",
    }
}
fn bore(w: &Workspace, s: &Editor, pick: &viewport::Selection) -> Option<Result<Pick>> {
    if pick.definition == s.owner || s.path == pick.path {
        return None;
    }
    let eval = viewport::selection_evaluation(w)?;
    let source = instance(eval, &pick.path)?;
    let mesh = eval.meshes.get(&source.mesh_key)?;
    let body = pick.body.clone()?;
    let target = s.target.as_ref()?;
    let (axis, radius, source_face) = if pick.reference.topology == "face" {
        let face = mesh
            .faces
            .iter()
            .find(|f| f.body.as_ref() == Some(&body) && f.reference == pick.reference)?;
        let c = face
            .measurement
            .as_ref()?
            .cylinder
            .as_ref()
            .filter(|c| c.internal)?;
        (c.axis.clone(), c.radius_mm, &face.reference)
    } else if pick.reference.topology == "edge" {
        let e = mesh
            .edges
            .iter()
            .find(|e| e.body.as_ref() == Some(&body) && e.reference == pick.reference)?;
        let EdgeCenter::Circle {
            center,
            normal,
            radius_mm,
            ..
        } = e.center.as_ref()?
        else {
            return None;
        };
        // An outer boss rim also has a circle. Its native adjoining bore wall distinguishes a hole.
        let bore = mesh.faces.iter().find(|face| {
            face.body.as_ref() == Some(&body)
                && e.faces.iter().any(|adjacent| {
                    cad_core::provenance::equivalent(adjacent, &face.reference).unwrap_or(false)
                })
                && face
                    .measurement
                    .as_ref()
                    .and_then(|m| m.cylinder.as_ref())
                    .is_some_and(|c| c.internal)
        })?;
        (
            Axis {
                origin: *center,
                direction: *normal,
            },
            *radius_mm,
            &bore.reference,
        )
    } else {
        return None;
    };
    let doc = w.tool_checkpoint.as_ref()?.visible();
    let configured =
        cad_core::configuration::variant(doc, &source.definition, source.configuration.as_deref());
    let nominal = configured
        .as_ref()
        .ok()
        .and_then(|doc| {
            let producer = cad_core::provenance::pattern_seed_producer(source_face).ok()?;
            let feature = doc
                .parts
                .get(&producer.definition)?
                .features
                .iter()
                .find(|f| f.id == producer.node)?;
            let Operation::DrilledHoles { definition } = &feature.operation else {
                return None;
            };
            cad_core::expression::value(
                doc,
                &producer.definition,
                &definition.thread.as_ref()?.nominal_diameter,
                cad_core::Dimension::Length,
            )
            .ok()
        })
        .unwrap_or(2. * radius);
    Some(
        matching_holes::position(&target.plane, target.matrix, source.matrix, &axis).map(|uv| {
            Pick {
                point: cad_core::id(),
                center: Center {
                    path: pick.path.clone(),
                    body,
                    reference: pick.reference.clone(),
                },
                uv,
                nominal,
                selection: pick.clone(),
            }
        }),
    )
}
pub fn capture_click(w: &mut Workspace, rect: egui::Rect, p: egui::Pos2) -> bool {
    if !active(w) {
        return false;
    }
    let hits = viewport::pick(w, rect, p);
    if w.matching_holes
        .as_ref()
        .is_some_and(|s| s.target.is_none())
    {
        if let Some(target) = hits.iter().find_map(|h| face_target(w, h)) {
            w.matching_holes.as_mut().unwrap().target = Some(target);
            w.status = prompt(w).into();
        } else {
            w.status = "Pick a planar face on the cover being edited.".into();
        }
        return true;
    }
    let s = w.matching_holes.as_ref().unwrap();
    let Some(pick) = hits.iter().find_map(|h| bore(w, s, h)) else {
        w.status = "Pick a circular rim or a cylindrical hole wall on another component.".into();
        return true;
    };
    let pick = match pick {
        Ok(v) => v,
        Err(e) => {
            w.status = e.to_string();
            return true;
        }
    };
    let tolerance = w.session.visible().settings.modeling.linear_mm;
    let s = w.matching_holes.as_mut().unwrap();
    if let Some(index) = s.picks.iter().position(|v| {
        v.center.path == pick.center.path
            && ((v.uv[0] - pick.uv[0]).hypot(v.uv[1] - pick.uv[1]) <= tolerance
                || v.center == pick.center)
    }) {
        s.picks.remove(index);
    } else {
        s.picks.push(pick);
    }
    if !s.custom_diameter
        && let Some(largest) = s.picks.iter().map(|p| p.nominal).reduce(f64::max)
    {
        s.diameter = Expr(format!("{:.3} mm", largest * 1.10));
    }
    refresh(w);
    true
}
fn build(doc: &Document, s: &Editor) -> Result<Part> {
    let t = s
        .target
        .as_ref()
        .ok_or_else(|| invalid("Pick a cover face."))?;
    if s.picks.is_empty() {
        return Err(invalid("Pick at least one source hole."));
    }
    let mut part = doc
        .parts
        .get(&s.owner)
        .cloned()
        .ok_or_else(|| invalid("Cover is missing."))?;
    let diameter =
        cad_core::expression::value(doc, &s.owner, &s.diameter, cad_core::Dimension::Length)?;
    if !diameter.is_finite() || s.picks.iter().any(|p| diameter <= p.nominal) {
        return Err(invalid(
            "Enter a clearance diameter larger than the selected hole or thread.",
        ));
    }
    let number = part
        .features
        .iter()
        .filter(|f| f.name.starts_with("Clearance Holes"))
        .count()
        + 1;
    let sketch = Sketch {
        id: s.sketch.clone(),
        name: format!("Hole Positions{number}"),
        points: s.picks.iter().map(|p| (p.point.clone(), p.uv)).collect(),
        matched_centers: s.follow.then(|| matching_holes::Definition {
            assembly: doc.root.clone(),
            target_part: s.owner.clone(),
            target_path: s.path.clone(),
            centers: s
                .picks
                .iter()
                .map(|p| (p.point.clone(), p.center.clone()))
                .collect(),
        }),
        ..Default::default()
    };
    part.features.push(Feature {
        id: s.profile.clone(),
        name: sketch.name.clone(),
        suppressed: false,
        operation: Operation::Profile {
            profile: Profile::Sketch {
                sketch: sketch.id.clone(),
            },
            plane: t.plane.clone(),
            plane_reference: Some(t.face.clone()),
            reference_body: Some(t.body.clone()),
        },
    });
    part.sketches.insert(sketch.id.clone(), sketch);
    let def = holes::Definition {
        body: t.body.clone(),
        positions: part.reference(&s.profile),
        points: s.picks.iter().map(|p| p.point.clone()).collect(),
        diameter: s.diameter.clone(),
        depth: "1 mm".into(),
        end: holes::End::ThroughAll,
        head: holes::Head::Simple,
        thread: None,
        reverse: false,
        start_offset: "0 mm".into(),
    };
    part.outputs.retain(|id| id != &t.body.node);
    part.outputs.push(s.feature.clone());
    part.features.push(Feature {
        id: s.feature.clone(),
        name: format!("Clearance Holes{number}"),
        suppressed: false,
        operation: Operation::DrilledHoles {
            definition: Box::new(def),
        },
    });
    Ok(part)
}
fn refresh(w: &mut Workspace) {
    let Some(s) = w.matching_holes.as_ref() else {
        return;
    };
    w.confirm_requested = false;
    let Some(checkpoint) = w.tool_checkpoint.as_ref() else {
        return;
    };
    let result = build(checkpoint.visible(), s);
    let base = checkpoint.visible().parts[&s.owner].clone();
    let owner = s.owner.clone();
    let draft = result.as_ref().unwrap_or(&base).clone();
    let selected = s.picks.iter().map(|p| p.selection.clone()).collect();
    let complete = draft.sketches.contains_key(&s.sketch);
    let mut message = result.err().map(|e| e.to_string());
    if w.session.visible().parts.get(&owner) != Some(&draft) {
        w.dispatch(Command::PutPart {
            part: draft.clone(),
            assembly: None,
            placement: Default::default(),
        });
    }
    let s = w.matching_holes.as_mut().unwrap();
    if w.session.visible().parts.get(&owner) != Some(&draft) {
        message = Some(w.status.clone());
    }
    s.submitted = message.is_none() && complete;
    s.error = message;
    w.geometry_selection = selected;
    w.rendered_key.clear();
    if let Some(error) = &s.error {
        w.status = error.clone();
    }
}
pub fn action(w: &mut Workspace, command: &str, data: Option<&Value>) -> bool {
    let Some(s) = w.matching_holes.as_mut() else {
        return false;
    };
    if data
        .and_then(|v| v.get("generation"))
        .and_then(Value::as_str)
        .is_some_and(|g| g != s.feature)
    {
        return false;
    }
    match command {
        "match-holes.follow" => s.follow = true,
        "match-holes.copy" => s.follow = false,
        "match-holes.clear" => s.picks.clear(),
        "match-holes.face" => {
            s.picks.clear();
            s.target = None;
        }
        _ => return false,
    }
    refresh(w);
    true
}
fn actions(s: &Editor) -> [(&'static str, &'static str, bool, bool); 4] {
    [
        ("match-holes.follow", "Follow the base", true, s.follow),
        ("match-holes.copy", "Copy once", true, !s.follow),
        ("match-holes.face", "Change face", s.target.is_some(), false),
        (
            "match-holes.clear",
            "Clear holes",
            !s.picks.is_empty(),
            false,
        ),
    ]
}
pub fn semantics(w: &Workspace) -> Vec<SemanticElement> {
    let Some(s) = w.matching_holes.as_ref() else {
        return vec![];
    };
    let mut elements = vec![SemanticElement {
        id: DIAMETER.into(),
        role: "match-holes-field".into(),
        label: "Clearance diameter".into(),
        value: Some(s.diameter.0.clone()),
        state: "draft".into(),
        commands: vec![],
        relationships: vec![s.feature.clone()],
        data: json!({"setter":"match-holes","value_type":"quantity","dimension":"length","unit":"mm","editable":true,"json":s.diameter.0,"generation":s.feature}),
    }];
    elements.push(SemanticElement {
        id: "match-holes.sources".into(),
        role: "selection-summary".into(),
        label: prompt(w).into(),
        value: Some(source_summary(s)),
        state: if ready(w) { "ready" } else { "picking" }.into(),
        commands: vec![],
        relationships: s.picks.iter().map(|p| p.center.body.node.clone()).collect(),
        data: json!({
            "cover": s.path,
            "holes": s.picks.iter().map(|p| &p.center).collect::<Vec<_>>(),
            "error": s.error,
        }),
    });
    for (id, label, enabled, selected) in actions(s) {
        let state = if !enabled {
            "disabled"
        } else if selected {
            "selected"
        } else {
            "ready"
        };
        elements.push(SemanticElement {
            id: id.into(),
            role: "match-holes-action".into(),
            label: label.into(),
            value: None,
            state: state.into(),
            commands: vec![id.into()],
            relationships: vec![s.feature.clone()],
            data: json!({
                "generation": s.feature,
                "actions": [{"command":id, "label":label, "enabled":enabled, "arguments":{"generation":s.feature}}]
            }),
        });
    }
    elements
}
pub fn current_value(w: &Workspace, e: &SemanticElement) -> Option<Value> {
    let s = w.matching_holes.as_ref()?;
    (e.id == DIAMETER && e.data["generation"] == s.feature).then(|| json!(s.diameter.0))
}
pub fn set_value(w: &mut Workspace, e: &SemanticElement, v: Value) -> Result<()> {
    if current_value(w, e).is_none() {
        return Err(invalid("Reopen Match Holes before editing the diameter."));
    }
    let value = v
        .as_str()
        .ok_or_else(|| invalid("Enter a length or parameter."))?;
    let s = w.matching_holes.as_mut().unwrap();
    s.diameter = Expr(value.into());
    s.custom_diameter = true;
    refresh(w);
    Ok(())
}
fn source_summary(s: &Editor) -> String {
    let count = s.picks.len();
    let noun = if count == 1 { "hole" } else { "holes" };
    let names = s
        .picks
        .iter()
        .map(|p| p.selection.name.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    let mut names = names.into_iter();
    match (names.next(), names.next()) {
        (Some(base), None) => format!("{base} · {count} {noun}"),
        _ => format!("{count} source {noun}"),
    }
}
pub fn show(ui: &mut egui::Ui, w: &mut Workspace) {
    let Some(s) = w.matching_holes.as_ref() else {
        return;
    };
    controls::label(
        ui,
        "match-holes.prompt",
        "selection-prompt",
        &[],
        prompt(w),
        "ready",
    );
    controls::label(
        ui,
        "match-holes.sources",
        "selection-summary",
        &[],
        &source_summary(s),
        "ready",
    );
    let mut diameter = s.diameter.clone();
    let choices = actions(s);
    if !s.picks.is_empty()
        && controls::expression(ui, DIAMETER, "Clearance diameter", &mut diameter)
    {
        let s = w.matching_holes.as_mut().unwrap();
        s.diameter = diameter;
        s.custom_diameter = true;
        refresh(w);
    }
    ui.horizontal_wrapped(|ui| {
        for (id, label, enabled, selected) in choices {
            if !enabled {
                continue;
            }
            let response =
                controls::button(ui, id, "match-holes-action", &[], label, true, selected);
            let response = match id {
                "match-holes.follow" => response.on_hover_text(
                    "Keep the selected centers aligned after changes to the base or its placement.",
                ),
                "match-holes.copy" => response.on_hover_text(
                    "Keep these positions in the cover’s sketch when the base changes.",
                ),
                _ => response,
            };
            if response.clicked() {
                action(w, id, None);
            }
        }
    });
    if let Some(error) = w.matching_holes.as_ref().and_then(|s| s.error.as_deref()) {
        controls::label(ui, "match-holes.error", "diagnostic", &[], error, "invalid");
    }
}
