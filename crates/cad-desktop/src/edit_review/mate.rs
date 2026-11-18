//! Repair a single explicit mate endpoint while the originating edit stays open.
use crate::{Workspace, viewport};
use bevy_egui::egui;
use cad_core::{
    Document, Id, Mate, NodeRef, PlacedPart, TopologyRef,
    assembly_geometry::{EndpointRole, FailedReference, Reference, ReferenceClass, Source},
    protocol::{Evaluation, MeshData, RequestIdentity},
    reference_geometry::{AxisSource, PlaneSource},
};
use std::collections::BTreeSet;

pub struct Picker {
    request: RequestIdentity,
    problem: FailedReference,
    paths: BTreeSet<Vec<Id>>,
}

fn endpoint_mut<'a>(mate: &'a mut Mate, failure: &FailedReference) -> Option<&'a mut Reference> {
    let (participant, reference) = match failure.endpoint {
        EndpointRole::First => (&mate.first, mate.first_reference.as_mut()?),
        EndpointRole::Second => (&mate.second, mate.second_reference.as_mut()?),
        EndpointRole::Additional(index) => {
            let endpoint = mate
                .relationship
                .as_mut()?
                .endpoints_mut()
                .into_iter()
                .nth(index)?;
            (&endpoint.instance, &mut endpoint.reference)
        }
    };
    (participant == &failure.participant && *reference == failure.reference).then_some(reference)
}

fn source_mate(doc: &Document, problem: &FailedReference) -> Option<Mate> {
    let mut mate = doc
        .assemblies
        .get(&problem.assembly)?
        .mates
        .iter()
        .find(|m| m.id == problem.mate)?
        .clone();
    endpoint_mut(&mut mate, problem)?;
    Some(mate)
}

pub(super) fn paths(
    doc: &Document,
    evaluation: &Evaluation,
    problem: &FailedReference,
) -> BTreeSet<Vec<Id>> {
    if source_mate(doc, problem).is_none() {
        return BTreeSet::new();
    }
    evaluation
        .instances
        .iter()
        .filter(|i| {
            evaluation
                .meshes
                .contains_key(cad_core::history::mesh_key(i))
                && problem.occurs_in(doc, &i.path)
        })
        .map(|i| i.path.clone())
        .collect()
}

pub(super) fn begin(w: &mut Workspace, problem: FailedReference, paths: BTreeSet<Vec<Id>>) {
    let Some(current) = w.session.draft.as_ref().and_then(|d| d.evaluation.as_ref()) else {
        return;
    };
    if current.failed_mate_reference.as_deref() != Some(&problem) || paths.is_empty() {
        return;
    }
    w.edit_repair = Some(Picker {
        request: current.identity.clone(),
        problem,
        paths,
    });
    w.selection_popup = None;
    w.confirm_requested = false;
    w.rendered_key.clear();
    w.status = prompt(w);
}

pub(crate) fn evaluation(w: &Workspace) -> Option<&Evaluation> {
    let picker = w.edit_repair.as_ref()?;
    let current = w.session.draft.as_ref()?.evaluation.as_ref()?;
    (current.identity == picker.request
        && current.identity == crate::source_cache::identity(w)
        && current.failed_mate_reference.as_deref() == Some(&picker.problem))
    .then_some(current)
}

pub(crate) fn eligible(w: &Workspace, path: &[Id]) -> bool {
    w.edit_repair
        .as_ref()
        .is_some_and(|p| p.paths.contains(path))
}

pub(crate) fn prompt(w: &Workspace) -> String {
    let Some(p) = &w.edit_repair else {
        return String::new();
    };
    let name = w
        .session
        .visible()
        .assemblies
        .get(&p.problem.assembly)
        .and_then(|a| {
            a.instances
                .get(&p.problem.participant)
                .map(|i| cad_core::occurrence_names::instance(w.session.visible(), &a.id, i))
        })
        .unwrap_or_else(|| "the component".into());
    let target = match p.problem.reference.source.class() {
        ReferenceClass::Plane => "a planar face or plane",
        ReferenceClass::Axis => "a rotational face, axis, or straight or circular edge",
        ReferenceClass::Point => "a datum point",
        ReferenceClass::Curve => "an edge",
        ReferenceClass::Profile => "a profile face",
    };
    format!("Pick {target} on {name}")
}

pub(crate) fn stop(w: &mut Workspace) {
    w.edit_repair = None;
    w.rendered_key.clear();
    w.status = "Returned to the edit".into();
}

fn candidate(w: &Workspace, hit: &viewport::Selection) -> Option<Reference> {
    let picker = w.edit_repair.as_ref()?;
    if !picker.paths.contains(&hit.path) {
        return None;
    }
    let a = w
        .session
        .visible()
        .assemblies
        .get(&picker.problem.assembly)?;
    let class = picker.problem.reference.source.class();
    let (participant, mut reference) =
        crate::mate_ui::typed_reference_from_pick(w, a, hit, Some(class)).ok()?;
    if class == ReferenceClass::Axis && matches!(hit.reference.topology.as_str(), "edge" | "face") {
        let evaluation = evaluation(w)?;
        let mesh = evaluation
            .instances
            .iter()
            .find(|i| i.path == hit.path)
            .and_then(|i| evaluation.meshes.get(cad_core::history::mesh_key(i)))?;
        if hit.reference.topology == "face" {
            if !mesh.faces.iter().any(|f| {
                f.body == hit.body
                    && f.repair_axis.is_some()
                    && crate::selection_ui::same_edge(&f.reference, &hit.reference)
            }) {
                return None;
            }
        } else {
            let edge = mesh.edges.iter().find(|e| {
                e.body == hit.body && crate::selection_ui::same_edge(&e.reference, &hit.reference)
            })?;
            let body = hit.body.clone()?;
            let axis = match edge.center.as_ref()? {
                cad_core::protocol::EdgeCenter::Line { .. } => AxisSource::Edge {
                    body,
                    reference: hit.reference.clone(),
                },
                cad_core::protocol::EdgeCenter::Circle { .. } => AxisSource::CircleEdge {
                    body,
                    reference: hit.reference.clone(),
                },
            };
            reference.source = Source::Axis { axis };
        }
    }
    (participant == picker.problem.participant
        && reference.path == picker.problem.reference.path
        && reference.source.class() == class
        && reference != picker.problem.reference)
        .then_some(reference)
}

fn replace(w: &mut Workspace, reference: Reference) {
    let Some(picker) = &w.edit_repair else { return };
    let problem = &picker.problem;
    let Some(mut mate) = source_mate(w.session.visible(), problem) else {
        stop(w);
        return;
    };
    let Some(slot) = endpoint_mut(&mut mate, problem) else {
        return;
    };
    *slot = reference;
    let owner = problem.assembly.clone();
    let before = picker.request.clone();
    w.dispatch(cad_core::commands::Command::PutMate {
        assembly: owner,
        mate: Box::new(mate),
    });
    crate::reference_repair_ui::after_downstream_mate(w, &before);
    w.rendered_key.clear();
}

fn topology(source: &Source) -> Option<(&NodeRef, &TopologyRef)> {
    match source {
        Source::FaceAxis { body, reference }
        | Source::FaceCenter { body, reference }
        | Source::EdgeCurve { body, reference }
        | Source::Plane {
            plane: PlaneSource::Face { body, reference },
        }
        | Source::Axis {
            axis:
                AxisSource::Edge { body, reference }
                | AxisSource::FaceAxis { body, reference }
                | AxisSource::CircleEdge { body, reference },
        } => Some((body, reference)),
        _ => None,
    }
}

fn paint_reference(
    ui: &egui::Ui,
    rect: egui::Rect,
    w: &Workspace,
    instance: &PlacedPart,
    mesh: &MeshData,
    target: (&NodeRef, &TopologyRef),
    old: bool,
) {
    let (body, reference) = target;
    let props = crate::presentation_controls::properties(
        if old {
            "edit-review/previous-seat"
        } else {
            "edit-review/replacement"
        },
        "mate-repair-highlight",
        &["viewport-guide"],
        if old { "previous" } else { "hovered" },
    );
    if props.get("display").is_some_and(|p| p == "none") {
        return;
    }
    let color = props
        .get("color")
        .and_then(|s| crate::ui::color(s))
        .unwrap_or_else(|| {
            if old {
                ui.visuals().warn_fg_color
            } else {
                viewport::selection_color(w)
            }
        });
    let project = viewport::projector(w, rect);
    let screen = |point| project(viewport::placed(point, instance));
    let painter = ui.painter_at(rect);
    let matches = |other: &TopologyRef, owner: Option<&NodeRef>| {
        owner.is_none_or(|o| o == body) && crate::selection_ui::same_edge(reference, other)
    };
    let label_position = if reference.topology == "face" {
        let mut faces = mesh
            .faces
            .iter()
            .filter(|f| matches(&f.reference, f.body.as_ref()));
        let Some(face) = faces.next() else { return };
        if faces.next().is_some() {
            return;
        }
        let mut painted = egui::Mesh::default();
        let mut bounds = egui::Rect::NOTHING;
        for triangle in mesh.indices[face.start..face.start + face.count]
            .as_chunks::<3>()
            .0
        {
            let [Some(a), Some(b), Some(c)] =
                [triangle[0], triangle[1], triangle[2]].map(|i| screen(mesh.positions[i as usize]))
            else {
                continue;
            };
            let index = painted.vertices.len() as u32;
            for p in [a, b, c] {
                bounds.extend_with(p);
                painted.colored_vertex(p, color.gamma_multiply(0.35));
            }
            painted.add_triangle(index, index + 1, index + 2);
        }
        if !bounds.is_positive() {
            return;
        }
        painter.add(painted);
        bounds.center()
    } else {
        let mut edges = mesh
            .edges
            .iter()
            .filter(|e| matches(&e.reference, e.body.as_ref()));
        let Some(edge) = edges.next() else { return };
        if edges.next().is_some() {
            return;
        }
        for pair in edge.points.windows(2) {
            if let (Some(a), Some(b)) = (screen(pair[0]), screen(pair[1])) {
                painter.line_segment([a, b], egui::Stroke::new(2., color));
            }
        }
        let Some(p) = edge
            .points
            .get(edge.points.len() / 2)
            .copied()
            .and_then(screen)
        else {
            return;
        };
        p
    };
    if old && rect.contains(label_position) {
        painter.text(
            label_position,
            egui::Align2::CENTER_BOTTOM,
            "Previous reference",
            egui::TextStyle::Small.resolve(ui.style()),
            color,
        );
    }
}

pub(crate) fn viewport(
    w: &mut Workspace,
    ui: &egui::Ui,
    rect: egui::Rect,
    response: &egui::Response,
    exclusions: &[egui::Rect],
) -> bool {
    let Some(current) = evaluation(w) else {
        return false;
    };
    let picker = w.edit_repair.as_ref().unwrap();
    crate::reference_ui::overlay(w, ui, rect);
    let (baseline, baseline_doc) = match &w.tool_checkpoint {
        Some(session) => (super::evaluated(session), session.visible()),
        None => (w.session.evaluation.as_ref(), &w.session.committed),
    };
    let prior = baseline_doc
        .assemblies
        .get(&picker.problem.assembly)
        .and_then(|a| a.mates.iter().find(|m| m.id == picker.problem.mate))
        .and_then(|m| match picker.problem.endpoint {
            EndpointRole::First if m.first == picker.problem.participant => {
                m.first_reference.as_ref()
            }
            EndpointRole::Second if m.second == picker.problem.participant => {
                m.second_reference.as_ref()
            }
            EndpointRole::Additional(index) => m
                .relationship
                .as_ref()?
                .endpoints()
                .into_iter()
                .nth(index)
                .filter(|e| e.instance == picker.problem.participant)
                .map(|e| &e.reference),
            _ => None,
        })
        .filter(|r| r.path == picker.problem.reference.path);
    if let Some((old, (body, reference))) = baseline.zip(prior.and_then(|r| topology(&r.source))) {
        for instance in current
            .instances
            .iter()
            .filter(|i| picker.paths.contains(&i.path))
        {
            let key = cad_core::history::mesh_key(instance);
            let Some(prior) = old
                .instances
                .iter()
                .find(|i| i.path == instance.path && cad_core::history::mesh_key(i) == key)
            else {
                continue;
            };
            let mesh = old
                .history
                .get(cad_core::history::mesh_key(prior))
                .and_then(|h| h.meshes.get(&body.node).map(|m| m.as_ref()))
                .or_else(|| old.meshes.get(key));
            if let Some(mesh) = mesh {
                paint_reference(ui, rect, w, instance, mesh, (body, reference), true);
            }
        }
    }
    if response.hovered()
        && let Some(p) = response
            .hover_pos()
            .filter(|p| !exclusions.iter().any(|r| r.contains(*p)))
        && let Some((hit, reference)) = viewport::pick(w, rect, p)
            .into_iter()
            .find_map(|hit| candidate(w, &hit).map(|reference| (hit, reference)))
    {
        if let Some(target) = topology(&reference.source)
            && let Some(instance) = current.instances.iter().find(|i| i.path == hit.path)
            && let Some(mesh) = current.meshes.get(cad_core::history::mesh_key(instance))
        {
            paint_reference(ui, rect, w, instance, mesh, target, false);
        }
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        if response.clicked() {
            replace(w, reference);
        }
    }
    viewport::navigate(w, ui.ctx(), response);
    true
}
