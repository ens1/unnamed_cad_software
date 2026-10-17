//! Native presentation of application-owned selection actions.
use crate::{Workspace, cad_ui, presentation_controls as controls, ui, viewport};
use bevy_egui::egui;
use cad_core::{
    Feature,
    commands::Command,
    presentation::Region,
    selection_actions::SelectionTarget,
    selection_sets::{self, Mode as SetMode},
};
use std::collections::BTreeMap;

pub struct ActionPopup {
    target: SelectionTarget,
    anchor: egui::Pos2,
    layer: egui::LayerId,
    opened_frame: u64,
    presentation_parent: Option<Region>,
}

/// The editor reads the shared draft, rather than retaining a second edge list.
pub(crate) fn edge_feature(w: &Workspace) -> Option<Feature> {
    if w.dialog.as_deref() != Some("feature")
        || w.tool_checkpoint.is_none()
        || w.fields.contains_key("feature-source-invalid")
    {
        return None;
    }
    let id = w.fields.get("feature-editor-id")?;
    w.session
        .visible()
        .parts
        .get(w.view.active_part.as_ref()?)?
        .features
        .iter()
        .find(|feature| {
            &feature.id == id
                && matches!(
                    feature.operation,
                    cad_core::Operation::Fillet { .. } | cad_core::Operation::Chamfer { .. }
                )
        })
        .cloned()
}

pub(crate) fn edge_selection(
    feature: &Feature,
) -> Option<(&cad_core::NodeRef, &[cad_core::TopologyRef])> {
    match &feature.operation {
        cad_core::Operation::Fillet { body, edges, .. }
        | cad_core::Operation::Chamfer { body, edges, .. } => Some((body, edges)),
        _ => None,
    }
}

pub(crate) fn same_edge(first: &cad_core::TopologyRef, second: &cad_core::TopologyRef) -> bool {
    selection_sets::same_reference(first, second)
}

fn individual_edges(w: &Workspace, feature: &str) -> bool {
    w.fields
        .get(&format!("edge-selection-mode/{feature}"))
        .is_some_and(|mode| mode == "individual")
}

/// Expand only a proven native circular carrier on one current occurrence.
/// Rectangular loops and branching tangent chains remain explicit set actions.
fn circular_rim(w: &Workspace, pick: &viewport::Selection) -> Option<Vec<viewport::Selection>> {
    use cad_core::protocol::{EDGE_CENTER_VERSION, EdgeCenter};
    let body = pick.body.as_ref()?;
    let evaluation = viewport::selection_evaluation(w)?;
    let instance = viewport::display_instances(w, evaluation)
        .into_iter()
        .find(|instance| instance.path == pick.path && instance.definition == pick.definition)?;
    let meshes = viewport::selection_meshes(w, evaluation, instance);
    let seed = meshes.iter().flat_map(|mesh| &mesh.edges).find(|edge| {
        edge.body.as_ref() == Some(body) && same_edge(&edge.reference, &pick.reference)
    })?;
    let Some(EdgeCenter::Circle {
        version,
        center,
        normal,
        radius_mm,
        ..
    }) = &seed.center
    else {
        return None;
    };
    if *version != EDGE_CENTER_VERSION || !radius_mm.is_finite() || *radius_mm <= 0. {
        return None;
    }
    let tolerance = (radius_mm.abs() * 1e-9).max(1e-7);
    let picks = expansion(w, SetMode::ClosedLoop).ok()?;
    let same_carrier = picks.iter().all(|candidate| {
        meshes.iter().flat_map(|mesh| &mesh.edges).any(|edge| {
            if edge.body.as_ref() != Some(body) || !same_edge(&edge.reference, &candidate.reference)
            {
                return false;
            }
            let Some(EdgeCenter::Circle {
                version,
                center: other,
                normal: axis,
                radius_mm: radius,
                ..
            }) = &edge.center
            else {
                return false;
            };
            *version == EDGE_CENTER_VERSION
                && (radius - radius_mm).abs() <= tolerance
                && center
                    .iter()
                    .zip(other)
                    .all(|(a, b)| (a - b).abs() <= tolerance)
                && normal
                    .iter()
                    .zip(axis)
                    .map(|(a, b)| a * b)
                    .sum::<f64>()
                    .abs()
                    >= 1. - 1e-10
        })
    });
    same_carrier.then_some(picks)
}

pub(crate) fn preferred_edges(
    w: &mut Workspace,
    body: &cad_core::NodeRef,
) -> Vec<cad_core::TopologyRef> {
    let edges = viewport::preselected_edges(w, body);
    if edges.len() != 1 {
        return edges;
    }
    if let Some(picks) = w.selected.as_ref().and_then(|pick| circular_rim(w, pick)) {
        let references = picks.iter().map(|pick| pick.reference.clone()).collect();
        w.geometry_selection = picks;
        return references;
    }
    edges
}

pub(crate) fn picked_edges(
    w: &Workspace,
    feature: &str,
    pick: &viewport::Selection,
) -> Vec<cad_core::TopologyRef> {
    if !individual_edges(w, feature)
        && let Some(picks) = circular_rim(w, pick)
    {
        return picks.into_iter().map(|pick| pick.reference).collect();
    }
    vec![pick.reference.clone()]
}

pub(crate) fn edge_key(feature: &str, edge: &cad_core::TopologyRef) -> String {
    // Use the full provenance identity, never the mutable list position or a
    // tessellated endpoint. Commands remain attached to their original edge.
    format!(
        "{feature}/{}/{}/{}",
        edge.producer.definition, edge.producer.node, edge.role
    )
}

pub(crate) fn accepts_edge(
    w: &Workspace,
    body: &cad_core::NodeRef,
    pick: &viewport::Selection,
) -> bool {
    pick.reference.topology == "edge"
        && pick.definition == body.definition
        && pick.body.as_ref() == Some(body)
        && edge_occurrence(w, &pick.path)
}

pub(crate) fn edge_occurrence(w: &Workspace, path: &[cad_core::Id]) -> bool {
    w.edge_selection_path.as_ref().map_or_else(
        || {
            w.view
                .active_instance
                .as_ref()
                .is_none_or(|id| path.contains(id))
        },
        |selected| selected == path,
    )
}

pub(crate) fn known_edge_occurrence(w: &Workspace, definition: &str) -> Option<Vec<cad_core::Id>> {
    let evaluation = viewport::selection_evaluation(w)?;
    let instances = viewport::display_instances(w, evaluation);
    let known = w
        .selected
        .as_ref()
        .filter(|pick| pick.definition == definition)
        .map(|pick| &pick.path)
        .or(w.selected_feature_path.as_ref())
        .or(w.edge_selection_path.as_ref());
    if let Some(path) = known.filter(|path| {
        instances
            .iter()
            .any(|instance| instance.definition == definition && &instance.path == *path)
    }) {
        return Some(path.clone());
    }
    let mut candidates = instances.iter().filter(|instance| {
        instance.definition == definition
            && w.view
                .active_instance
                .as_ref()
                .is_none_or(|id| instance.path.contains(id))
    });
    let first = candidates.next()?;
    candidates.next().is_none().then(|| first.path.clone())
}

pub(crate) fn clear_edge_pick(w: &mut Workspace) {
    for key in [
        "edge-pick-base",
        "edge-pick-last",
        "edge-pick-feature",
        "edge-pick-toggle",
    ] {
        w.fields.remove(key);
    }
}

struct ExpansionContext<'a> {
    pick: &'a viewport::Selection,
    body: &'a cad_core::NodeRef,
    evaluation: &'a cad_core::protocol::Evaluation,
    instance: &'a cad_core::PlacedPart,
}

/// Menu availability checks only the current source context. Traversing native
/// incidence and resolving legacy provenance belong to the explicit set action,
/// not every frame that displays the ordinary selection menu.
fn expansion_context(w: &Workspace, mode: SetMode) -> cad_core::Result<ExpansionContext<'_>> {
    if w.view.history_cursor.is_some() || w.sketch.is_some() {
        return Err(cad_core::CadError::Invalid(
            "Finish sketch editing and return to Latest before selecting model edge sets".into(),
        ));
    }
    let pick = w.selected.as_ref().ok_or_else(|| {
        cad_core::CadError::Invalid("Select a face or edge on the model first".into())
    })?;
    let topology = if mode == SetMode::FaceBoundary {
        "face"
    } else {
        "edge"
    };
    if pick.reference.topology != topology {
        return Err(cad_core::CadError::Invalid(format!(
            "Select a current native {topology} on this body"
        )));
    }
    let body = pick.body.as_ref().ok_or_else(|| {
        cad_core::CadError::Invalid("This selection has no evaluated body owner".into())
    })?;
    if let Some(feature) = edge_feature(w)
        && let Some((input, _)) = edge_selection(&feature)
        && (body != input || !edge_occurrence(w, &pick.path))
    {
        return Err(cad_core::CadError::Invalid(
            "Select geometry on this feature's input body and component".into(),
        ));
    }
    let evaluation = viewport::selection_evaluation(w)
        .ok_or_else(|| cad_core::CadError::Invalid("Wait for current model evaluation".into()))?;
    let session = if edge_feature(w).is_some() {
        w.tool_checkpoint.as_ref().unwrap_or(&w.session)
    } else {
        &w.session
    };
    if !evaluation.valid || evaluation.identity != session.identity() {
        return Err(cad_core::CadError::Invalid(
            "Wait for the current native geometry; an older display cache cannot expand selections"
                .into(),
        ));
    }
    let mut instances = viewport::display_instances(w, evaluation)
        .into_iter()
        .filter(|instance| instance.definition == pick.definition && instance.path == pick.path);
    let instance = instances.next().ok_or_else(|| {
        cad_core::CadError::Invalid(
            "The selected component occurrence is no longer displayed".into(),
        )
    })?;
    if instances.next().is_some() {
        return Err(cad_core::CadError::Invalid(
            "The selected component occurrence is ambiguous".into(),
        ));
    }
    Ok(ExpansionContext {
        pick,
        body,
        evaluation,
        instance,
    })
}

/// Resolve only the current native cache, on one output body and occurrence.
/// A last-valid display fallback is useful while rebuilding but cannot author a set.
fn expansion(w: &Workspace, mode: SetMode) -> cad_core::Result<Vec<viewport::Selection>> {
    let ExpansionContext {
        pick,
        body,
        evaluation,
        instance,
    } = expansion_context(w, mode)?;
    let candidates = viewport::selection_meshes(w, evaluation, instance)
        .into_iter()
        .filter(|mesh| {
            if pick.reference.topology == "face" {
                mesh.faces.iter().any(|face| {
                    face.body.as_ref() == Some(body)
                        && selection_sets::same_reference(&face.reference, &pick.reference)
                })
            } else {
                mesh.edges.iter().any(|edge| {
                    edge.body.as_ref() == Some(body) && same_edge(&edge.reference, &pick.reference)
                })
            }
        })
        .collect::<Vec<_>>();
    if candidates.len() != 1 {
        return Err(cad_core::CadError::Invalid(
            "The selected topology is missing or ambiguous; select it again".into(),
        ));
    }
    let mesh = candidates[0];
    let references = selection_sets::expand(mesh, body, &pick.reference, mode)?;
    let by_reference = mesh
        .edges
        .iter()
        .filter(|edge| edge.body.as_ref() == Some(body))
        .map(|edge| {
            (
                (
                    edge.reference.producer.definition.as_str(),
                    edge.reference.producer.node.as_str(),
                    edge.reference.role.as_str(),
                ),
                edge,
            )
        })
        .collect::<std::collections::BTreeMap<_, _>>();
    Ok(references
        .into_iter()
        .map(|reference| {
            let edge = by_reference
                .get(&(
                    reference.producer.definition.as_str(),
                    reference.producer.node.as_str(),
                    reference.role.as_str(),
                ))
                .unwrap();
            let native = edge.native.as_ref().unwrap();
            let center = native
                .endpoints
                .map(|point| viewport::placed(point, instance));
            viewport::Selection {
                reference,
                point: std::array::from_fn(|axis| center[0][axis] * 0.5 + center[1][axis] * 0.5),
                ..pick.clone()
            }
        })
        .collect())
}

fn set_descriptors(
    w: &Workspace,
    target: &SelectionTarget,
) -> Vec<cad_core::presentation::SemanticElement> {
    let SelectionTarget::Geometry {
        reference,
        body: Some(body),
        path,
        ..
    } = target
    else {
        return vec![];
    };
    let modes: &[SetMode] = match reference.topology.as_str() {
        "face" => &[SetMode::FaceBoundary],
        "edge" => &[SetMode::ClosedLoop, SetMode::TangentChain],
        _ => return vec![],
    };
    modes.iter().map(|mode| {
        let result = if current(w).as_ref() == Some(target) { expansion_context(w,*mode).map(|_| ()) } else { Err(cad_core::CadError::Invalid("Select this geometry again to expand its selection".into())) };
        let command = format!("selection.set-{}",mode.suffix());
        cad_core::presentation::SemanticElement {
            id:command.clone(), role:"selection-action".into(), label:mode.label().into(),
            value:Some(match &result { Ok(())=>"Resolve this set from current native topology; ambiguous or branching sets are rejected".into(),Err(error)=>error.to_string() }),
            state:if result.is_ok() {"ready"} else {"disabled"}.into(), commands:vec![command],
            relationships:vec![body.node.clone()],
            data:serde_json::json!({"contract":"tcad.presentation.selection-sets","version":selection_sets::CONTRACT_VERSION,"group":"Selection","target":target,"mode":mode,"input_body":body,"occurrence_path":path,"native_incidence":true,"current_evaluation_required":true,"classes":["menu-item","selection-action"]}),
        }
    }).collect()
}

/// A view-only set becomes authored input only through the shared feature draft.
pub(crate) fn set_action(w: &mut Workspace, command: &str) -> bool {
    let Some(mode) = command
        .strip_prefix("selection.set-")
        .and_then(SetMode::parse)
    else {
        return false;
    };
    match expansion(w, mode) {
        Ok(selected) => {
            w.geometry_selection = selected;
            w.selected = w.geometry_selection.first().cloned();
            if let Some(pick) = &w.selected {
                w.edge_selection_path = Some(pick.path.clone());
            }
            if let Some(mut feature) = edge_feature(w) {
                let edges = match &mut feature.operation {
                    cad_core::Operation::Fillet { edges, .. }
                    | cad_core::Operation::Chamfer { edges, .. } => edges,
                    _ => unreachable!(),
                };
                for pick in &w.geometry_selection {
                    if !edges.iter().any(|edge| same_edge(edge, &pick.reference)) {
                        edges.push(pick.reference.clone());
                    }
                }
                clear_edge_pick(w);
                w.json = serde_json::to_string_pretty(&feature).unwrap_or_default();
                ui::preview_feature(w, feature);
            }
            w.context_selection = None;
            w.selection_popup = None;
            w.candidates.clear();
            w.rendered_key.clear();
            w.status = format!("{} edges selected", w.geometry_selection.len());
        }
        Err(error) => w.status = error.to_string(),
    }
    true
}

fn focus_edge(
    w: &mut Workspace,
    body: &cad_core::NodeRef,
    reference: &cad_core::TopologyRef,
) -> cad_core::Result<()> {
    let path = known_edge_occurrence(w, &body.definition).ok_or_else(|| {
        cad_core::CadError::Invalid(
            "Select the component occurrence before focusing this edge".into(),
        )
    })?;
    let evaluation = viewport::selection_evaluation(w).ok_or_else(|| {
        cad_core::CadError::Invalid("Wait for the current model evaluation".into())
    })?;
    let session = w.tool_checkpoint.as_ref().unwrap_or(&w.session);
    if evaluation.identity != session.identity() {
        return Err(cad_core::CadError::Invalid(
            "Wait for current input geometry before focusing this edge".into(),
        ));
    }
    let instances = viewport::display_instances(w, evaluation)
        .into_iter()
        .filter(|instance| instance.definition == body.definition && instance.path == path)
        .collect::<Vec<_>>();
    if instances.len() != 1 {
        return Err(cad_core::CadError::Invalid(
            "The component occurrence is missing or ambiguous".into(),
        ));
    }
    let instance = instances[0];
    let edges = viewport::selection_meshes(w, evaluation, instance)
        .into_iter()
        .flat_map(|mesh| &mesh.edges)
        .filter(|edge| edge.body.as_ref() == Some(body) && same_edge(&edge.reference, reference))
        .collect::<Vec<_>>();
    if edges.len() != 1 {
        return Err(cad_core::CadError::Invalid(
            "This edge is missing or ambiguous; repair its reference first".into(),
        ));
    }
    let edge = edges[0];
    let mut low = [f64::INFINITY; 3];
    let mut high = [f64::NEG_INFINITY; 3];
    for point in &edge.points {
        let point = viewport::placed(*point, instance);
        for axis in 0..3 {
            low[axis] = low[axis].min(point[axis]);
            high[axis] = high[axis].max(point[axis]);
        }
    }
    if low.iter().chain(&high).any(|value| !value.is_finite()) {
        return Err(cad_core::CadError::Invalid(
            "This edge has no usable display extent".into(),
        ));
    }
    let center = std::array::from_fn(|axis| low[axis] * 0.5 + high[axis] * 0.5);
    let camera_target = viewport::render_point(center);
    if !camera_target.is_finite() {
        return Err(cad_core::CadError::Invalid(
            "This edge lies outside the viewport coordinate range".into(),
        ));
    }
    let pick = viewport::Selection {
        instance: path
            .last()
            .cloned()
            .unwrap_or_else(|| body.definition.clone()),
        path: path.clone(),
        name: instance.name.clone(),
        definition: body.definition.clone(),
        reference: edge.reference.clone(),
        body: Some(body.clone()),
        distance: 0.,
        point: center,
    };
    w.selected = Some(pick.clone());
    w.geometry_selection = vec![pick];
    w.edge_selection_path = Some(path);
    w.context_selection = None;
    w.view.target = camera_target.to_array();
    let extent = low
        .iter()
        .zip(high)
        .map(|(low, high)| (high - low).powi(2))
        .sum::<f64>()
        .sqrt();
    if extent.is_finite() && extent > 0. {
        w.view.distance = (extent as f32 * 0.01 * 2.0).clamp(0.05, 10000.);
    }
    crate::navigation::stop(w);
    w.rendered_key.clear();
    Ok(())
}

/// Presentation metadata is resolved on the same current input and occurrence
/// as picking. It never substitutes a signature or sampled polyline for intent.
fn native_edge<'a>(
    w: &'a Workspace,
    body: &cad_core::NodeRef,
    reference: &cad_core::TopologyRef,
) -> Option<&'a cad_core::protocol::EdgeData> {
    let evaluation = viewport::selection_evaluation(w)?;
    let session = w.tool_checkpoint.as_ref().unwrap_or(&w.session);
    if !evaluation.valid || evaluation.identity != session.identity() {
        return None;
    }
    let path = known_edge_occurrence(w, &body.definition)?;
    let instances = viewport::display_instances(w, evaluation)
        .into_iter()
        .filter(|instance| instance.definition == body.definition && instance.path == path)
        .collect::<Vec<_>>();
    if instances.len() != 1 {
        return None;
    }
    let edges = viewport::selection_meshes(w, evaluation, instances[0])
        .into_iter()
        .flat_map(|mesh| &mesh.edges)
        .filter(|edge| edge.body.as_ref() == Some(body) && same_edge(&edge.reference, reference))
        .collect::<Vec<_>>();
    (edges.len() == 1).then(|| edges[0])
}

fn edge_summary(
    w: &Workspace,
    body: &cad_core::NodeRef,
    reference: &cad_core::TopologyRef,
) -> (String, serde_json::Value) {
    use cad_core::protocol::{EDGE_CENTER_VERSION, EdgeCenter};
    let Some(edge) = native_edge(w, body, reference) else {
        return (
            "Geometry unavailable".into(),
            serde_json::json!({"resolved":false,"summary":"Geometry unavailable"}),
        );
    };
    let settings = &w.session.visible().settings;
    let (kind, summary, length, radius) = match edge.center.as_ref() {
        Some(EdgeCenter::Line {
            version,
            start,
            end,
        }) if *version == EDGE_CENTER_VERSION => {
            let length = start
                .iter()
                .zip(end)
                .map(|(start, end)| (end - start).powi(2))
                .sum::<f64>()
                .sqrt();
            if length.is_finite() && length > 0. {
                (
                    "straight",
                    format!("Straight · {}", settings.format_length(length)),
                    Some(length),
                    None,
                )
            } else {
                ("edge", "Edge".into(), None, None)
            }
        }
        Some(EdgeCenter::Circle {
            version, radius_mm, ..
        }) if *version == EDGE_CENTER_VERSION && radius_mm.is_finite() && *radius_mm > 0. => (
            "circular",
            format!("Circular · R {}", settings.format_length(*radius_mm)),
            None,
            Some(*radius_mm),
        ),
        _ => ("edge", "Edge".into(), None, None),
    };
    let metadata = serde_json::json!({
        "resolved":true,"summary":summary,"kind":kind,"length_mm":length,"radius_mm":radius,
        "carrier":edge.center,"incidence":edge.native,"reference":edge.reference,
        "measurement_source":(length.is_some() || radius.is_some()).then_some("exact_native_carrier"),"display_only":true,
        "evaluation_identity":viewport::selection_evaluation(w).map(|evaluation| &evaluation.identity),
    });
    (summary, metadata)
}

pub(crate) fn edge_controls(w: &Workspace) -> Vec<cad_core::presentation::SemanticElement> {
    let Some(feature) = edge_feature(w) else {
        return vec![];
    };
    let Some((body, edges)) = edge_selection(&feature) else {
        return vec![];
    };
    let selection = w
        .selected
        .as_ref()
        .filter(|pick| accepts_edge(w, body, pick));
    let mut out = vec![cad_core::presentation::SemanticElement {
        id: format!("{}::edge-selection", feature.id),
        role: "edge-selection".into(),
        label: "Selected edges".into(),
        value: Some(format!("{} selected", edges.len())),
        state: "draft".into(),
        commands: vec![],
        relationships: vec![feature.id.clone(), body.node.clone()],
        data: serde_json::json!({"contract":"tcad.presentation.edge-selection","version":1,"definition":body.definition,"feature":feature.id,"input_body":body,"occurrence_path":w.edge_selection_path,"edges":edges,"shared_draft":true,"requires_current_validation":true,"hint":"Click edges to add. Command-click or Shift-click toggles an edge. Tab cycles the current pick without adding every candidate."}),
    }];
    for (suffix, label, enabled) in [
        (
            "add",
            "Add selected edge",
            selection
                .is_some_and(|pick| !edges.iter().any(|edge| same_edge(edge, &pick.reference))),
        ),
        ("clear", "Clear edges", !edges.is_empty()),
    ] {
        let command = format!("selection.edge-{suffix}/{}", feature.id);
        out.push(cad_core::presentation::SemanticElement {
            id: command.clone(), role:"edge-selection-action".into(), label:label.into(),
            value: (!enabled).then(|| "Select an edge on this feature's input body".into()),
            state: if enabled {"ready"} else {"disabled"}.into(), commands:vec![command],
            relationships:vec![feature.id.clone()],
            data:serde_json::json!({"presentation_control":true,"feature":feature.id,"input_body":body,"enabled":enabled,"classes":["property-control"]}),
        });
    }
    out.extend(edge_granularity_controls(w, &feature.id, body));
    out.extend(edge_set_controls(w, &feature.id, body));
    for (index, edge) in edges.iter().enumerate() {
        let key = edge_key(&feature.id, edge);
        let command = format!("selection.edge-remove/{key}");
        let focus = format!("selection.edge-focus/{key}");
        let row = format!("edge-selection/{key}");
        let label = format!("Edge {}", index + 1);
        let (summary, metadata) = edge_summary(w, body, edge);
        let resolved = metadata["resolved"].as_bool().unwrap_or(false);
        out.push(cad_core::presentation::SemanticElement {
            id: row.clone(), role:"edge-selection-item".into(), label:label.clone(),
            value:Some(summary.clone()), state:if resolved {"selected"} else {"unresolved"}.into(), commands:vec![focus.clone(),command.clone()],
            relationships:vec![feature.id.clone()],
            data:serde_json::json!({"feature":feature.id,"edge":edge,"input_body":body,"nativeEdge":metadata,"classes":["property-control","edge-selection-row"],
                "actions":[{"command":focus,"label":"Focus","enabled":resolved,"disabled_reason":(!resolved).then_some("Wait for current input geometry or repair this edge reference")},
                    {"command":command,"label":"Remove","enabled":true}]}),
        });
        out.push(cad_core::presentation::SemanticElement {
            id:focus.clone(),role:"edge-selection-action".into(),label:format!("Focus edge {}",index+1),
            value:Some(if resolved {format!("Focus and highlight {label} · {summary}")} else {"Wait for current input geometry or repair this edge reference".into()}),state:if resolved {"ready"} else {"disabled"}.into(),commands:vec![focus],relationships:vec![feature.id.clone()],
            data:serde_json::json!({"presentation_control":true,"feature":feature.id,"edge":edge,"input_body":body,"inline_parent":row,"classes":["property-control","edge-focus"]}),
        });
        out.push(cad_core::presentation::SemanticElement {
            id: command.clone(), role:"edge-selection-action".into(), label:format!("Remove edge {}", index + 1),
            value:Some(format!("Remove {label} from this feature")), state:"ready".into(), commands:vec![command],
            relationships:vec![feature.id.clone()],
            data:serde_json::json!({"presentation_control":true,"feature":feature.id,"edge":edge,"input_body":body,"inline_parent":row,"classes":["property-control","edge-remove"]}),
        });
    }
    out
}

/// Compact bundled composition over the ordinary row and action descriptors.
/// Custom layouts can instead bind Focus and Remove independently.
pub(crate) fn render_edge_row(
    ui: &mut egui::Ui,
    w: &mut Workspace,
    element: &cad_core::presentation::SemanticElement,
) -> bool {
    if element.role != "edge-selection-item" {
        return false;
    }
    let classes = element
        .data
        .get("classes")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(serde_json::Value::as_str)
        .collect::<Vec<_>>();
    let props = controls::properties(&element.id, &element.role, &classes, &element.state);
    if props.get("display").is_some_and(|value| value == "none") {
        return true;
    }
    if props
        .get("visibility")
        .is_some_and(|value| value == "hidden")
    {
        ui.allocate_response(controls::field_size(ui, &props), egui::Sense::hover());
        return true;
    }
    let row_target = controls::current_target().map(|parent| {
        if parent.node.id == element.id {
            parent
        } else {
            parent.child(
                &element.id,
                &element.role,
                &classes
                    .iter()
                    .map(|class| (*class).into())
                    .collect::<Vec<_>>(),
                &element.state,
            )
        }
    });
    let mut clicked = None;
    let mut render = |ui: &mut egui::Ui| {
        ui.scope(|ui| {
            controls::apply_properties(ui, &props);
            let size = controls::field_size(ui, &props);
            if props.contains_key("width") || props.contains_key("max-width") {
                ui.set_max_width(size.x);
            }
            if props.contains_key("height") || props.contains_key("min-height") {
                ui.set_min_height(size.y);
            }
            controls::frame_from_properties(ui, &props).show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    let text = element
                        .value
                        .as_ref()
                        .map(|summary| format!("{} · {summary}", element.label))
                        .unwrap_or_else(|| element.label.clone());
                    controls::label(
                        ui,
                        &format!("{}:label", element.id),
                        "edge-selection-label",
                        &classes,
                        &text,
                        &element.state,
                    );
                    for action in element.action_descriptors() {
                        let focus = action.command.starts_with("selection.edge-focus/");
                        let action_classes = [
                            "property-control",
                            if focus { "edge-focus" } else { "edge-remove" },
                        ];
                        let response = controls::button(
                            ui,
                            &action.command,
                            "edge-selection-action",
                            &action_classes,
                            &action.label,
                            action.enabled,
                            false,
                        )
                        .on_hover_text(format!("{} {}", action.label, element.label))
                        .on_disabled_hover_text(
                            action.disabled_reason.as_deref().unwrap_or(&action.label),
                        );
                        if response.clicked() {
                            clicked = Some(action.command);
                        }
                    }
                });
            });
        });
    };
    if let Some(target) = row_target {
        controls::with_target(target, ui, render);
    } else {
        render(ui);
    }
    if let Some(command) = clicked {
        edge_action(w, &command);
    }
    true
}

pub(crate) fn edge_action(w: &mut Workspace, command: &str) -> bool {
    if !command.starts_with("selection.edge-") {
        return false;
    }
    let controls = edge_controls(w);
    let Some(control) = controls
        .iter()
        .find(|element| element.commands.iter().any(|id| id == command))
    else {
        w.status = "The edge selection changed; use its current controls".into();
        return true;
    };
    if let Some(action) = control
        .action_descriptors()
        .into_iter()
        .find(|action| action.command == command && !action.enabled)
    {
        w.status = action
            .disabled_reason
            .or_else(|| control.value.clone())
            .unwrap_or_else(|| "This edge action is unavailable".into());
        return true;
    }
    let Some(mut feature) = edge_feature(w) else {
        return true;
    };
    if let Some(granularity) = control
        .data
        .get("granularity")
        .and_then(serde_json::Value::as_str)
    {
        w.fields.insert(
            format!("edge-selection-mode/{}", feature.id),
            granularity.into(),
        );
        w.status = if granularity == "individual" {
            "Circular picks select individual segments".into()
        } else {
            "Circular picks select complete native rims".into()
        };
        return true;
    }
    if let Some(mode) = control
        .data
        .get("mode")
        .and_then(|value| serde_json::from_value::<SetMode>(value.clone()).ok())
    {
        return set_action(w, &format!("selection.set-{}", mode.suffix()));
    }
    if command.starts_with("selection.edge-focus/") {
        if let Some((body, _)) = edge_selection(&feature)
            && let Some(reference) = control.data.get("edge").and_then(|value| {
                serde_json::from_value::<cad_core::TopologyRef>(value.clone()).ok()
            })
            && let Err(error) = focus_edge(w, body, &reference)
        {
            w.status = error.to_string();
        }
        return true;
    }
    let edges = match &mut feature.operation {
        cad_core::Operation::Fillet { edges, .. } | cad_core::Operation::Chamfer { edges, .. } => {
            edges
        }
        _ => return true,
    };
    if command.starts_with("selection.edge-add/") {
        if let Some(pick) = &w.selected {
            edges.push(pick.reference.clone());
        }
    } else if command.starts_with("selection.edge-clear/") {
        edges.clear();
    } else if let Some(reference) = control
        .data
        .get("edge")
        .and_then(|value| serde_json::from_value::<cad_core::TopologyRef>(value.clone()).ok())
    {
        edges.retain(|edge| !same_edge(edge, &reference));
    }
    if w.selected.as_ref().is_some_and(|pick| {
        pick.reference.topology == "edge"
            && !edges.iter().any(|edge| same_edge(edge, &pick.reference))
    }) {
        w.selected = None;
        w.geometry_selection.clear();
    }
    clear_edge_pick(w);
    w.json = serde_json::to_string_pretty(&feature).unwrap_or_default();
    ui::preview_feature(w, feature);
    true
}

fn edge_granularity_controls(
    w: &Workspace,
    feature: &str,
    body: &cad_core::NodeRef,
) -> Vec<cad_core::presentation::SemanticElement> {
    [("rim", "Complete circular rims"), ("individual", "Individual edge segments")]
        .into_iter()
        .map(|(granularity, label)| {
            let selected = individual_edges(w, feature) == (granularity == "individual");
            let command = format!("selection.edge-mode-{granularity}/{feature}");
            cad_core::presentation::SemanticElement {
                id: command.clone(),
                role: "edge-selection-action".into(),
                label: label.into(),
                value: Some("Controls subsequent picks; existing authored selections are retained".into()),
                state: if selected { "selected" } else { "ready" }.into(),
                commands: vec![command],
                relationships: vec![feature.into(), body.node.clone()],
                data: serde_json::json!({"presentation_control":true,"contract":"tcad.presentation.edge-picking","version":1,"feature":feature,"input_body":body,"granularity":granularity,"selected":selected,"shared_draft":true,"classes":["property-control","selection-granularity"]}),
            }
        })
        .collect()
}

fn edge_set_controls(
    w: &Workspace,
    feature: &str,
    body: &cad_core::NodeRef,
) -> Vec<cad_core::presentation::SemanticElement> {
    [SetMode::FaceBoundary,SetMode::ClosedLoop,SetMode::TangentChain].into_iter().map(|mode| {
        let result = if w.selected.as_ref().is_some_and(|pick|pick.body.as_ref()==Some(body) && edge_occurrence(w,&pick.path)) { expansion_context(w,mode).map(|_| ()) } else { Err(cad_core::CadError::Invalid("Select a face or edge on this feature's input body".into())) };
        let command=format!("selection.edge-set-{}/{feature}",mode.suffix());
        cad_core::presentation::SemanticElement {
            id:command.clone(),role:"edge-selection-action".into(),label:mode.label().into(),value:Some(match &result { Ok(())=>"Resolve and add the current native edge set; ambiguous or branching sets are rejected".into(),Err(error)=>error.to_string() }),
            state:if result.is_ok() {"ready"} else {"disabled"}.into(),commands:vec![command],relationships:vec![feature.into(),body.node.clone()],
            data:serde_json::json!({"presentation_control":true,"contract":"tcad.presentation.selection-sets","version":selection_sets::CONTRACT_VERSION,"feature":feature,"input_body":body,"mode":mode,"shared_draft":true,"classes":["property-control","selection-set"]}),
        }
    }).collect()
}

/// Default compact presentation; the same commands are available to custom layouts.
pub(crate) fn edge_picker(
    ui: &mut egui::Ui,
    w: &mut Workspace,
    feature: &str,
    body: &cad_core::NodeRef,
    edges: &mut Vec<cad_core::TopologyRef>,
) -> bool {
    let mut changed = false;
    ui.horizontal_wrapped(|ui| {
        for control in edge_granularity_controls(w, feature, body) {
            if controls::button(
                ui,
                &control.id,
                &control.role,
                &["property-control", "selection-granularity"],
                &control.label,
                true,
                control.state == "selected",
            )
            .on_hover_text(control.value.as_deref().unwrap_or_default())
            .clicked()
                && let Some(command) = control.commands.first()
            {
                edge_action(w, command);
            }
        }
    });
    ui.horizontal(|ui| {
        let selected = w
            .selected
            .as_ref()
            .filter(|pick| accepts_edge(w, body, pick))
            .cloned();
        if controls::button(
            ui,
            &format!("selection.edge-add/{feature}"),
            "edge-selection-action",
            &["property-control"],
            "Add edge",
            selected
                .as_ref()
                .is_some_and(|pick| !edges.iter().any(|edge| same_edge(edge, &pick.reference))),
            false,
        )
        .clicked()
            && let Some(pick) = selected
        {
            edges.push(pick.reference);
            changed = true;
        }
        if controls::button(
            ui,
            &format!("selection.edge-clear/{feature}"),
            "edge-selection-action",
            &["property-control"],
            "Clear",
            !edges.is_empty(),
            false,
        )
        .clicked()
        {
            edges.clear();
            changed = true;
        }
    });
    for control in edge_set_controls(w, feature, body) {
        if controls::button(
            ui,
            &control.id,
            &control.role,
            &["property-control", "selection-set"],
            &control.label,
            control.state != "disabled",
            false,
        )
        .on_hover_text(control.value.as_deref().unwrap_or_default())
        .on_disabled_hover_text(control.value.as_deref().unwrap_or_default())
        .clicked()
            && let Some(mode) = control
                .data
                .get("mode")
                .and_then(|value| serde_json::from_value::<SetMode>(value.clone()).ok())
        {
            match expansion(w, mode) {
                Ok(picks) => {
                    for pick in &picks {
                        if !edges.iter().any(|edge| same_edge(edge, &pick.reference)) {
                            edges.push(pick.reference.clone());
                            changed = true;
                        }
                    }
                    w.geometry_selection = picks;
                    w.selected = w.geometry_selection.first().cloned();
                    if let Some(pick) = &w.selected {
                        w.edge_selection_path = Some(pick.path.clone());
                    }
                }
                Err(error) => w.status = error.to_string(),
            }
        }
    }
    let mut remove = None;
    for (index, edge) in edges.iter().enumerate() {
        let key = edge_key(feature, edge);
        let (summary, metadata) = edge_summary(w, body, edge);
        let resolved = metadata["resolved"].as_bool().unwrap_or(false);
        ui.push_id(&key, |ui| {
            ui.horizontal_wrapped(|ui| {
                controls::label(
                    ui,
                    &format!("edge-selection/{key}:label"),
                    "edge-selection-label",
                    &["property-control", "edge-selection-row"],
                    &format!("Edge {} · {summary}", index + 1),
                    if resolved { "selected" } else { "unresolved" },
                );
                let active = w
                    .selected
                    .as_ref()
                    .is_some_and(|pick| same_edge(&pick.reference, edge));
                if controls::button(
                    ui,
                    &format!("selection.edge-focus/{key}"),
                    "edge-selection-action",
                    &["property-control", "edge-focus"],
                    "Focus",
                    resolved,
                    active,
                )
                .on_hover_text(format!(
                    "Focus and highlight Edge {} · {summary}",
                    index + 1
                ))
                .on_disabled_hover_text(
                    "Wait for current input geometry or repair this edge reference",
                )
                .clicked()
                    && let Err(error) = focus_edge(w, body, edge)
                {
                    w.status = error.to_string();
                }
                if controls::button(
                    ui,
                    &format!("selection.edge-remove/{key}"),
                    "edge-selection-action",
                    &["property-control", "edge-remove"],
                    "Remove",
                    true,
                    false,
                )
                .on_hover_text("Remove edge from this draft")
                .clicked()
                {
                    remove = Some(index);
                }
            })
        });
    }
    if let Some(index) = remove {
        edges.remove(index);
        changed = true;
    }
    if changed {
        clear_edge_pick(w);
        w.geometry_selection
            .retain(|pick| edges.iter().any(|edge| same_edge(edge, &pick.reference)));
        if w.selected.as_ref().is_some_and(|pick| {
            pick.reference.topology == "edge"
                && !edges.iter().any(|edge| same_edge(edge, &pick.reference))
        }) {
            w.selected = w.geometry_selection.last().cloned();
        }
        w.rendered_key.clear();
    }
    ui.small("Click edges to add · ⌘/Shift-click to toggle · click a listed edge to focus");
    changed
}

pub fn current(w: &Workspace) -> Option<SelectionTarget> {
    if let Some(target) = &w.context_selection {
        return Some(target.clone());
    }

    if let Some(s) = &w.sketch
        && !s.choosing_plane
    {
        return Some(SelectionTarget::Sketch {
            definition: s.definition.clone(),
            feature: s.feature.clone(),
            sketch: s.sketch_id.clone(),
            entities: s.selection.clone(),
        });
    }
    if let Some(selected) = &w.selected {
        return Some(SelectionTarget::Geometry {
            definition: selected.definition.clone(),
            instance: selected.instance.clone(),
            path: selected.path.clone(),
            reference: selected.reference.clone(),
            body: selected.body.clone(),
            planar: crate::sketch::selected_plane(w, &selected.definition).is_some(),
        });
    }
    let definition = w.view.active_part.as_ref()?;
    if let Some(reference) = &w.active_plane {
        return Some(SelectionTarget::Reference {
            definition: definition.clone(),
            reference: reference.clone(),
        });
    }
    if let Some(feature) = &w.view.active_node
        && w.session
            .visible()
            .parts
            .get(definition)?
            .features
            .iter()
            .any(|f| &f.id == feature)
    {
        return Some(SelectionTarget::Feature {
            definition: definition.clone(),
            feature: feature.clone(),
        });
    }
    Some(SelectionTarget::Object {
        definition: definition.clone(),
        object: definition.clone(),
        role: "part".into(),
    })
}

pub fn semantics(w: &Workspace) -> Vec<cad_core::presentation::SemanticElement> {
    let mut edge_elements = edge_controls(w);
    let Some(target) = current(w) else {
        return edge_elements;
    };
    let mut elements = descriptors(w, &target);
    for action in &mut elements {
        let primary = action.data["menu"]["primary"].as_bool() == Some(true);
        if let Some(data) = action.data.as_object_mut() {
            data.insert(
                "classes".into(),
                serde_json::json!([
                    "menu-item",
                    "selection-action",
                    if primary { "primary" } else { "secondary" }
                ]),
            );
        }
    }
    let groups = elements
        .iter()
        .filter_map(|element| {
            element
                .data
                .get("menu")
                .and_then(|menu| menu.get("group"))
                .and_then(serde_json::Value::as_str)
        })
        .map(str::to_owned)
        .collect::<std::collections::BTreeSet<_>>();
    for group in groups {
        elements.push(cad_core::presentation::SemanticElement {
            id: format!("selection-group/{group}"),
            role: "selection-action-group".into(),
            label: group.clone(),
            value: None,
            state: "ready".into(),
            commands: vec![],
            relationships: vec!["selection-popup".into()],
            data: serde_json::json!({"presentation_control":true,"members":elements.iter().filter(|element|element.data["menu"]["group"].as_str()==Some(&group)).map(|element|&element.id).collect::<Vec<_>>(),"classes":["selection-menu-group"]}),
        });
    }
    for (id, role, label, commands) in [
        (
            "selection-popup",
            "selection-actions",
            title(w, &target),
            vec![],
        ),
        (
            "selection-popup.title",
            "selection-title",
            title(w, &target),
            vec![],
        ),
        (
            "selection-popup.dismiss",
            "dismiss-action",
            "Dismiss actions".into(),
            vec!["presentation.selection-dismiss".into()],
        ),
    ] {
        elements.push(cad_core::presentation::SemanticElement {
            id: id.into(),
            role: role.into(),
            label,
            value: None,
            state: "ready".into(),
            commands,
            relationships: vec!["workspace.viewport".into()],
            data: serde_json::json!({"presentation_control":true,"target":target}),
        });
    }
    edge_elements.extend(elements);
    edge_elements
}

fn descriptors(
    w: &Workspace,
    target: &SelectionTarget,
) -> Vec<cad_core::presentation::SemanticElement> {
    if let SelectionTarget::Sketch {
        definition,
        entities,
        ..
    } = target
        && let Some(s) = &w.sketch
    {
        return cad_core::selection_actions::sketch_project(
            w.session.visible(),
            definition,
            &crate::sketch::sketch_definition(s),
            entities,
            target,
        );
    }
    let inspecting_geometry =
        w.view.history_cursor.is_some() && matches!(target, SelectionTarget::Geometry { .. });
    let mut actions = cad_core::selection_actions::project(
        w.session.visible(),
        target,
        &w.view.pinned,
        w.tool_checkpoint.is_some() || w.sketch.is_some() || inspecting_geometry,
        w.view
            .isolation
            .then_some(w.view.active_part.as_deref())
            .flatten(),
    );
    if w.sketch.is_none() && w.view.history_cursor.is_none() {
        actions.extend(set_descriptors(w, target));
    }
    if let Some(sketch) = &w.sketch
        && let SelectionTarget::Geometry {
            definition,
            reference,
            ..
        } = target
        && matches!(reference.topology.as_str(), "edge" | "face")
    {
        let reason = if definition != &sketch.definition {
            Some("Convert Entities currently uses geometry of the active part")
        } else if sketch.choosing_plane {
            Some("Choose a sketch plane first")
        } else if sketch.edit.is_some() {
            Some("Accept or cancel the current sketch operation first")
        } else {
            None
        };
        actions.insert(
            0,
            cad_core::selection_actions::convert_action(target, reason),
        );
    }
    if inspecting_geometry {
        for action in &mut actions {
            if action.state == "disabled" {
                let reason = "Return to Latest to model from the current geometry";
                action.value = Some(reason.into());
                if let Some(data) = action.data.as_object_mut() {
                    data.insert("disabled_reason".into(), reason.into());
                }
            }
        }
    }
    for action in &mut actions {
        cad_core::selection_actions::menu_metadata(action);
    }
    actions
}

pub fn open(w: &mut Workspace, response: &egui::Response, anchor: egui::Pos2) {
    w.selection_popup = None;
    if let Some(target) = current(w) {
        w.selection_popup = Some(ActionPopup {
            target,
            anchor,
            layer: response.layer_id,
            opened_frame: response.ctx.cumulative_frame_nr(),
            presentation_parent: controls::current_target().map(|target| Region {
                id: target.node.id,
                role: target.node.role,
                classes: target.node.classes,
                component: None,
                rect: [0.; 4],
                resizable: false,
            }),
        });
    }
}

/// Select a source feature without opening actions; the viewport opens its own popup.
pub fn feature_selected(
    w: &mut Workspace,
    definition: &str,
    feature: &Feature,
    response: &egui::Response,
) {
    feature_selected_at(w, definition, feature, response, None);
}
pub(crate) fn feature_selected_at(
    w: &mut Workspace,
    definition: &str,
    feature: &Feature,
    response: &egui::Response,
    path: Option<Vec<cad_core::Id>>,
) {
    w.geometry_selection.clear();
    if response.double_clicked() {
        crate::history::latest(w);
    }
    w.selection_popup = None;
    w.selected_feature_path = path;
    if w.sketch.as_ref().is_some_and(|s| s.choosing_plane)
        && matches!(
            feature.operation,
            cad_core::Operation::ReferenceCoordinateSystem { .. }
        )
    {
        w.selected = None;
    }
    if w.sketch.as_ref().is_some_and(|s| s.choosing_plane)
        && matches!(
            feature.operation,
            cad_core::Operation::ReferencePlane { .. }
                | cad_core::Operation::ReferenceCoordinateSystem { .. }
        )
        && let Some((plane, reference)) =
            crate::reference_ui::evaluated_plane(w, definition, &feature.id)
    {
        w.selected = None;
        crate::sketch::choose(w, plane, Some(reference));
        return;
    }
    if let Some(s) = &mut w.sketch
        && s.feature == feature.id
    {
        s.selection.clear();
        s.selected = None;
        s.selected_member = None;
        s.dimension_model = None;
        w.selected = None;
        w.candidates.clear();
        w.context_selection = None;
        return;
    }
    if w.tool_checkpoint.is_some() || w.sketch.is_some() {
        w.selected = None;
        w.candidates.clear();
        w.context_selection = Some(SelectionTarget::Feature {
            definition: definition.into(),
            feature: feature.id.clone(),
        });
        return;
    }
    if w.view.active_part.as_deref() != Some(definition) {
        w.view.active_instance = None;
        w.edge_selection_path = None;
    }
    w.context_selection = Some(SelectionTarget::Feature {
        definition: definition.into(),
        feature: feature.id.clone(),
    });
    w.view.active_part = Some(definition.into());
    w.view.active_node = Some(feature.id.clone());
    w.selected = None;
    w.active_plane = None;
    w.candidates.clear();
    if response.double_clicked() {
        ui::feature_dialog(w, Some(feature.clone()));
    }
}

pub fn show(ctx: &egui::Context, w: &mut Workspace) {
    let Some(popup) = w.selection_popup.take() else {
        return;
    };
    if current(w).as_ref() != Some(&popup.target) {
        return;
    }
    let region = Region {
        id: "selection-popup".into(),
        role: "selection-actions".into(),
        component: None,
        rect: [popup.anchor.x, popup.anchor.y, 232., 0.],
        classes: vec![],
        resizable: false,
    };
    let target = popup
        .presentation_parent
        .as_ref()
        .map(|parent| {
            w.layout
                .style_target(parent, &w.view.active_layout, "ready")
                .child(&region.id, &region.role, &region.classes, "ready")
        })
        .unwrap_or_else(|| {
            w.layout
                .style_target(&region, &w.view.active_layout, "ready")
        });
    let props = w.theme.properties_for(&target);
    if props.get("display").is_some_and(|v| v == "none") {
        return;
    }
    let number = |key: &str, default: f32| {
        props
            .get(key)
            .and_then(|v| v.trim_end_matches("px").parse::<f32>().ok())
            .filter(|v| v.is_finite())
            .unwrap_or(default)
    };
    let width = number("width", 232.)
        .max(number("min-width", 0.))
        .min(number("max-width", ctx.content_rect().width()))
        .max(0.);
    let mut frame =
        egui::Frame::popup(&ctx.global_style()).inner_margin(number("padding", 8.).max(0.));
    if let Some(color) = props.get("background").and_then(|v| ui::color(v)) {
        frame.fill = color;
    }
    frame.corner_radius =
        egui::CornerRadius::same(number("border-radius", 6.).clamp(0., 255.) as u8);
    if let Some(border) = props.get("border") {
        let mut words = border.split_whitespace();
        let width = words
            .next()
            .and_then(|v| v.trim_end_matches("px").parse::<f32>().ok())
            .filter(|v| v.is_finite())
            .unwrap_or(1.)
            .max(0.);
        if let Some(color) = words.next_back().and_then(ui::color) {
            frame.stroke = egui::Stroke::new(width, color);
        }
    }
    let mut open = true;
    let mut executed = false;
    egui::Popup::new(
        egui::Id::new("selection-popup"),
        ctx.clone(),
        popup.anchor,
        popup.layer,
    )
    .kind(egui::PopupKind::Menu)
    .at_position(egui::pos2(
        number("left", popup.anchor.x),
        number("top", popup.anchor.y),
    ))
    .gap(8.)
    .width(width)
    .frame(frame)
    .open_bool(&mut open)
    .close_behavior(if popup.opened_frame == ctx.cumulative_frame_nr() {
        egui::PopupCloseBehavior::IgnoreClicks
    } else {
        egui::PopupCloseBehavior::CloseOnClickOutside
    })
    .show(|ui| {
        controls::with_target(target, ui, |ui| {
            ui.set_width(width);
            if let Some(color) = props.get("color").and_then(|v| ui::color(v)) {
                ui.visuals_mut().override_text_color = Some(color);
            }
            ui.style_mut().override_font_id =
                Some(egui::FontId::proportional(number("font-size", 12.).max(1.)));
            ui.spacing_mut().item_spacing.y = number("gap", 3.).max(0.);
            egui::ScrollArea::vertical()
                .max_height(
                    number("height", ctx.content_rect().height() - 32.)
                        .max(number("min-height", 0.))
                        .min(number("max-height", ctx.content_rect().height()))
                        .max(0.),
                )
                .show(ui, |ui| executed = contents(ui, w, &popup.target));
        })
    });
    if open && !executed {
        w.selection_popup = Some(popup);
    }
}

fn title(w: &Workspace, target: &SelectionTarget) -> String {
    let name = w
        .session
        .visible()
        .parts
        .get(target.definition())
        .and_then(|p| p.features.iter().find(|f| f.id == target.feature()))
        .map(|f| f.name.as_str())
        .unwrap_or("Selection");
    match target {
        SelectionTarget::Geometry { reference, .. } => format!(
            "{} · {name}",
            match reference.topology.as_str() {
                "edge" => "Edge",
                "plane" => "Plane",
                "axis" => "Axis",
                "point" => "Point",
                _ => "Face",
            }
        ),
        SelectionTarget::Feature { .. } => name.into(),
        SelectionTarget::Sketch { entities, .. } => {
            if entities.is_empty() {
                "Sketch".into()
            } else {
                format!("Sketch · {} selected", entities.len())
            }
        }
        SelectionTarget::Reference { reference, .. } => reference.clone(),
        SelectionTarget::Object {
            role,
            object,
            definition,
        } => {
            let doc = w.session.visible();
            match role.as_str() {
                "part" | "assembly" => cad_core::presentation::definition_name(doc, definition),
                "component-instance" => doc
                    .assemblies
                    .values()
                    .find_map(|a| a.instances.get(object))
                    .map(|i| i.name.clone())
                    .unwrap_or("Component".into()),
                "assembly-mate" => "Mate".into(),
                "parameter-field" => "Parameter".into(),
                "material" => "Material".into(),
                _ => "Selection".into(),
            }
        }
    }
}

pub fn contents(ui: &mut egui::Ui, w: &mut Workspace, target: &SelectionTarget) -> bool {
    let actions = descriptors(w, target);
    if actions.is_empty() {
        ui.weak("Selection is no longer available");
        return false;
    }
    let title = title(w, target);
    let dismissed = ui
        .horizontal(|ui| {
            let dismiss = controls::properties(
                "selection-popup.dismiss",
                "dismiss-action",
                &["menu-item"],
                "ready",
            );
            let dismiss_width = controls::length(&dismiss, "width", ui.available_width(), 24.)
                .max(controls::number(&dismiss, "min-width", 0.))
                .min(ui.available_width());
            let available =
                (ui.available_width() - dismiss_width - ui.spacing().item_spacing.x).max(0.);
            let title_props =
                controls::properties("selection-popup.title", "selection-title", &[], "ready");
            let title_width =
                controls::length(&title_props, "width", available, available).min(available);
            controls::bounded_label(
                ui,
                "selection-popup.title",
                "selection-title",
                &[],
                &title,
                "ready",
                title_width,
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if controls::button(
                    ui,
                    "selection-popup.dismiss",
                    "dismiss-action",
                    &["menu-item"],
                    "×",
                    true,
                    false,
                )
                .on_hover_text("Dismiss actions · Esc")
                .clicked()
                {
                    ui.close();
                    true
                } else {
                    false
                }
            })
            .inner
        })
        .inner;
    if dismissed {
        return true;
    }
    let mut direct = vec![];
    let mut groups: BTreeMap<String, Vec<&cad_core::presentation::SemanticElement>> =
        BTreeMap::new();
    for element in &actions {
        let primary = element.data["menu"]["primary"].as_bool() == Some(true);
        let props = controls::properties(
            &element.id,
            &element.role,
            &[
                "menu-item",
                "selection-action",
                if primary { "primary" } else { "secondary" },
            ],
            &element.state,
        );
        if element.commands.is_empty() || props.get("display").is_some_and(|value| value == "none")
        {
            continue;
        }
        let primary = props
            .get("renderer-menu-presentation")
            .map_or(primary, |mode| {
                matches!(mode.as_str(), "primary" | "inline")
            });
        if primary {
            direct.push(element);
        } else {
            let group = element.data["menu"]["group"].as_str().unwrap_or("More");
            groups.entry(group.into()).or_default().push(element);
        }
    }
    let action_order = |element: &&cad_core::presentation::SemanticElement| {
        let primary = element.data["menu"]["primary"].as_bool() == Some(true);
        let props = controls::properties(
            &element.id,
            &element.role,
            &[
                "menu-item",
                "selection-action",
                if primary { "primary" } else { "secondary" },
            ],
            &element.state,
        );
        controls::number(
            &props,
            "renderer-menu-order",
            element.data["menu"]["order"].as_f64().unwrap_or(50.) as f32,
        )
    };
    let mut visible_groups = vec![];
    for (group, members) in groups {
        let props = controls::properties(
            &format!("selection-group/{group}"),
            "selection-action-group",
            &["selection-menu-group"],
            "ready",
        );
        if props.get("display").is_some_and(|value| value == "none") {
            continue;
        }
        if props
            .get("renderer-menu-presentation")
            .is_some_and(|value| value == "primary")
        {
            direct.extend(members);
        } else {
            visible_groups.push((group, members));
        }
    }
    direct.sort_by(|a, b| action_order(a).total_cmp(&action_order(b)));
    for element in direct {
        if menu_action(ui, w, target, element) {
            return true;
        }
    }
    let mut groups = visible_groups;
    let group_order = |group: &str, members: &[&cad_core::presentation::SemanticElement]| {
        let props = controls::properties(
            &format!("selection-group/{group}"),
            "selection-action-group",
            &["selection-menu-group"],
            "ready",
        );
        controls::number(
            &props,
            "renderer-menu-order",
            members
                .first()
                .and_then(|e| e.data["menu"]["group_order"].as_f64())
                .unwrap_or(120.) as f32,
        )
    };
    groups.sort_by(|a, b| group_order(&a.0, &a.1).total_cmp(&group_order(&b.0, &b.1)));
    if !groups.is_empty() {
        ui.separator();
    }
    for (group, mut members) in groups {
        members.sort_by(|a, b| action_order(a).total_cmp(&action_order(b)));
        let id = format!("selection-group/{group}");
        let props = controls::properties(
            &id,
            "selection-action-group",
            &["selection-menu-group"],
            "ready",
        );
        if props.get("display").is_some_and(|v| v == "none") {
            continue;
        }
        if props
            .get("renderer-menu-presentation")
            .is_some_and(|v| v == "inline")
        {
            controls::label(
                ui,
                &id,
                "selection-action-group",
                &["selection-menu-group"],
                &group,
                "inline",
            );
            for element in members {
                if menu_action(ui, w, target, element) {
                    return true;
                }
            }
            continue;
        }
        let response = controls::button(
            ui,
            &id,
            "selection-action-group",
            &["selection-menu-group"],
            &format!("{group}  ›"),
            true,
            false,
        );
        let parent = controls::current_target();
        let parent_width = ui.available_width();
        let mut executed = false;
        egui::containers::menu::SubMenu::new().show(ui, &response, |ui| {
            let render = |ui: &mut egui::Ui| {
                controls::apply_properties(ui, &props);
                let available = (ui.ctx().content_rect().width() - 16.).max(0.);
                let width = controls::length(&props, "width", parent_width, 232.)
                    .min(available)
                    .max(0.);
                ui.set_width(width);
                egui::ScrollArea::vertical()
                    .max_height(
                        controls::number(&props, "max-height", 400.)
                            .min(ui.ctx().content_rect().height() - 32.)
                            .max(0.),
                    )
                    .show(ui, |ui| {
                        for element in members {
                            if menu_action(ui, w, target, element) {
                                executed = true;
                                break;
                            }
                        }
                    });
            };
            if let Some(parent) = parent {
                controls::with_target(
                    parent.child(
                        &id,
                        "selection-action-group",
                        &["selection-menu-group".into()],
                        "flyout",
                    ),
                    ui,
                    render,
                );
            } else {
                render(ui);
            }
        });
        if executed {
            return true;
        }
    }
    false
}

fn menu_action(
    ui: &mut egui::Ui,
    w: &mut Workspace,
    target: &SelectionTarget,
    element: &cad_core::presentation::SemanticElement,
) -> bool {
    let primary = element.data["menu"]["primary"].as_bool() == Some(true);
    if controls::button(
        ui,
        &element.id,
        &element.role,
        &[
            "menu-item",
            "selection-action",
            if primary { "primary" } else { "secondary" },
        ],
        &element.label,
        element.state != "disabled",
        element.state == "selected",
    )
    .on_hover_text(
        element
            .value
            .as_deref()
            .or_else(|| element.data["description"].as_str())
            .unwrap_or(&element.label),
    )
    .on_disabled_hover_text(element.value.as_deref().unwrap_or(&element.label))
    .clicked()
        && let Some(command) = element.commands.first()
    {
        execute(w, target, command);
        ui.close();
        true
    } else {
        false
    }
}

pub fn execute(w: &mut Workspace, target: &SelectionTarget, command: &str) {
    let actions = descriptors(w, target);
    let Some(action) = actions
        .iter()
        .find(|a| a.commands.iter().any(|c| c == command))
    else {
        return;
    };
    if action.state == "disabled" {
        w.status = action.value.clone().unwrap_or_default();
        return;
    }
    // A bound action may outlive the frame that published it. Never apply an old
    // topology command to the geometry that is selected now.
    if matches!(target, SelectionTarget::Geometry { .. }) && current(w).as_ref() != Some(target) {
        w.status = "Selection changed. Select the geometry again to show its actions".into();
        return;
    }
    if command.starts_with("assembly.weld-") {
        crate::assembly_welds_ui::action(w, command, Some(&serde_json::json!(target)));
        return;
    }
    if set_action(w, command) {
        return;
    }
    if command == "sketch.convert-entities" && matches!(target, SelectionTarget::Geometry { .. }) {
        w.context_selection = None;
        crate::sketch_edit_ui::begin(w, crate::sketch_edit_ui::Kind::Convert);
        return;
    }
    match target {
        SelectionTarget::Sketch { .. } => {
            if current(w).as_ref() != Some(target) {
                w.status = "Sketch selection changed; select again".into();
                return;
            }
            w.selection_popup = None;
            crate::sketch_ui::action(w, command);
            return;
        }
        SelectionTarget::Reference {
            definition,
            reference,
        } => {
            w.selection_popup = None;
            match command {
                "sketch.new" => {
                    w.view.active_part = Some(definition.clone());
                    w.context_selection = None;
                    w.active_plane = (reference != "Origin").then(|| reference.clone());
                    ui::action(w, command);
                }
                "feature.split" | "feature.cut-with-plane" => {
                    w.view.active_part = Some(definition.clone());
                    w.context_selection = None;
                    w.selected = None;
                    w.active_plane = Some(reference.clone());
                    ui::action(w, command);
                }
                "viewport.center-origin" => {
                    w.view.target = [0.; 3];
                    crate::navigation::stop(w);
                }
                "viewport.normal-reference" => {
                    w.view.roll = 0.;
                    crate::navigation::stop(w);
                    w.view.orbit = match reference.as_str() {
                        "Top Plane" => [0., std::f32::consts::FRAC_PI_2],
                        "Right Plane" => [std::f32::consts::FRAC_PI_2, 0.],
                        _ => [0., 0.],
                    };
                }
                _ => {}
            }
            return;
        }
        SelectionTarget::Object {
            definition,
            object,
            role,
        } => {
            w.selection_popup = None;
            if w.tool_checkpoint.is_none() && w.session.visible().parts.contains_key(definition) {
                w.view.active_part = Some(definition.clone());
            }
            if role == "assembly" && w.session.visible().assemblies.contains_key(definition) {
                w.fields.insert("assembly-id".into(), definition.clone());
            }
            match command {
                "selection.fit" => {
                    let active = w.view.active_part.replace(target.definition().into());
                    viewport::fit(w, true);
                    w.view.active_part = active;
                }
                "selection.pin" => {
                    if w.view.pinned.contains(object) {
                        w.view.pinned.retain(|id| id != object);
                    } else {
                        w.view.pinned.push(object.clone());
                    }
                    w.rendered_key.clear();
                }
                "selection.isolate" | "selection.edit-in-place" => {
                    if command == "selection.edit-in-place" {
                        let path = w
                            .session
                            .visible()
                            .placed_parts()
                            .unwrap_or_default()
                            .into_iter()
                            .find(|placed| placed.path.last() == Some(object))
                            .map(|placed| placed.path);
                        w.activate_context(Some(definition.clone()), path);
                    } else {
                        w.view.active_instance = Some(object.clone());
                        w.isolate();
                    }
                    w.rendered_key.clear();
                }
                "object.edit" if role == "component-pattern" => {
                    if let Some(pattern) = w
                        .session
                        .visible()
                        .assemblies
                        .get(definition)
                        .and_then(|a| a.component_patterns.get(object))
                        .cloned()
                    {
                        crate::component_pattern_ui::begin(w, definition, pattern);
                    }
                }
                "object.delete" if role == "component-pattern" => {
                    w.start_tool();
                    w.dispatch(Command::DeleteComponentPattern {
                        assembly: definition.clone(),
                        pattern: object.clone(),
                    });
                }
                "object.edit" => {
                    if role == "assembly-mate" {
                        if let Some(mate) = w
                            .session
                            .visible()
                            .assemblies
                            .get(definition)
                            .and_then(|a| a.mates.iter().find(|m| &m.id == object))
                        {
                            let mate = mate.clone();
                            crate::mate_ui::begin(w, definition, mate);
                        }
                    } else {
                        w.fields.insert("graph-parameters".into(), "open".into());
                        w.fields
                            .insert("focus-parameter".into(), format!("{definition}::{object}"));
                        w.fields.insert(
                            "editing-parameter".into(),
                            format!("{definition}::{object}"),
                        );
                        if w.sketch.is_some() {
                            w.manager_tree = false;
                        } else {
                            w.open_editor("properties");
                        }
                    }
                }
                "object.delete" if role == "assembly-mate" => {
                    w.start_tool();
                    w.dispatch(Command::DeleteMate {
                        assembly: definition.clone(),
                        mate: object.clone(),
                    });
                }
                "assembly.edit" | "selection.mate" => {
                    if let Some(assembly) = w
                        .session
                        .visible()
                        .assemblies
                        .values()
                        .find(|a| a.instances.contains_key(object))
                        .map(|a| a.id.clone())
                    {
                        w.fields.insert("assembly-id".into(), assembly);
                    }
                    w.open_editor("assembly");
                }
                _ => {
                    w.context_selection = None;
                    ui::action(w, command);
                }
            }
            return;
        }
        _ => {}
    }
    let Some(feature) = w
        .session
        .visible()
        .parts
        .get(target.definition())
        .and_then(|p| p.features.iter().find(|f| f.id == target.feature()))
        .cloned()
    else {
        return;
    };
    w.selection_popup = None;
    if matches!(target, SelectionTarget::Feature { .. }) {
        w.selected = None;
        w.active_plane = None;
        w.candidates.clear();
    }
    if (!w.view.isolation && w.tool_checkpoint.is_none()) || command == "selection.edit-in-place" {
        w.view.active_part = Some(target.definition().into());
        w.view.active_node = Some(target.feature().into());
    }
    match command {
        "viewport.normal-reference" => crate::reference_ui::normal_to(w),
        "feature.edit" => ui::feature_dialog(w, Some(feature)),
        "feature.move-up" | "feature.move-down" => crate::feature_order_ui::move_feature(
            w,
            target.definition(),
            &feature.id,
            command == "feature.move-up",
        ),
        "feature.suppress" | "feature.delete" => {
            let suppressed = !cad_core::configuration::feature_suppressed(
                w.session.visible(),
                target.definition(),
                &feature,
            );
            w.start_tool();
            w.dispatch(if command == "feature.suppress" {
                Command::SuppressFeature {
                    definition: target.definition().into(),
                    feature: feature.id,
                    suppressed,
                }
            } else {
                Command::DeleteFeature {
                    definition: target.definition().into(),
                    feature: feature.id,
                }
            });
        }
        "selection.measure" => cad_ui::measure(w),
        "selection.fit" => {
            let active = w.view.active_part.replace(target.definition().into());
            viewport::fit(w, true);
            w.view.active_part = active;
        }
        "selection.pin" => ui::pin_selected(w),
        "selection.edit-in-place" => {
            if let SelectionTarget::Geometry { instance, .. } = target {
                w.view.active_instance = Some(instance.clone());
            }
            if !w.view.isolation {
                w.isolate();
            }
            w.rendered_key.clear();
        }
        "selection.isolate" => w.isolate(),
        "selection.mate" => {
            let assembly = w
                .fields
                .get("assembly-id")
                .filter(|id| w.session.visible().assemblies.contains_key(*id))
                .cloned()
                .unwrap_or_else(|| w.session.visible().root.clone());
            crate::mate_ui::new(w, &assembly);
        }
        _ => ui::action(w, command),
    }
}

/// Sidebar and graph selections still publish actions without opening a popup.
pub fn object_selected(w: &mut Workspace, target: SelectionTarget) {
    w.geometry_selection.clear();
    w.selection_popup = None;
    w.selected_feature_path = None;
    w.context_selection = Some(target);
    w.selected = None;
    w.view.active_node = None;
}
