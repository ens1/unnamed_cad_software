//! Docked, shared-draft assembly mating with exact persistent geometry references.
use crate::{Workspace, viewport};
use bevy::math::DVec3;
use bevy_egui::egui;
use cad_core::{
    Assembly, CadError, Document, Id, Mate, NodeRef, Operation, Result,
    assembly_geometry::{
        self, Alignment, CouplerKind, CurvePosition, Endpoint, Frame, FrameKind, PathOrientation,
        Range, Reference, Relationship, Source, WidthPosition,
    },
    commands::Command,
    reference_geometry::{
        AxisSource, EdgePointSource, OriginAxis, OriginPlane, PlaneSource, PointSource,
    },
};

fn origins() -> Vec<(String, Source)> {
    let mut choices = Vec::new();
    for plane in [OriginPlane::Front, OriginPlane::Top, OriginPlane::Right] {
        choices.push((
            plane.label().into(),
            Source::Plane {
                plane: PlaneSource::Origin { plane },
            },
        ));
    }
    for (axis, label) in [
        (OriginAxis::X, "X axis"),
        (OriginAxis::Y, "Y axis"),
        (OriginAxis::Z, "Z axis"),
    ] {
        choices.push((
            label.into(),
            Source::Axis {
                axis: AxisSource::Origin { axis },
            },
        ));
    }
    choices.push((
        "Origin / coordinates".into(),
        Source::Point {
            point: PointSource::default(),
        },
    ));
    choices
}
fn choices(doc: &Document, definition: &str) -> Vec<(String, Source)> {
    let mut choices = origins();
    if let Some(part) = doc.parts.get(definition) {
        for feature in part.features.iter().filter(|f| !f.suppressed) {
            let node = NodeRef {
                definition: definition.into(),
                node: feature.id.clone(),
            };
            match &feature.operation {
                Operation::ReferenceCoordinateSystem { .. } => {
                    choices.push((
                        format!("{} · Complete frame (Z / X)", feature.name),
                        Source::CoordinateSystem {
                            feature: node.clone(),
                        },
                    ));
                    choices.push((
                        format!("{} · Origin", feature.name),
                        Source::Point {
                            point: PointSource::CoordinateSystemOrigin {
                                feature: node.clone(),
                            },
                        },
                    ));
                    for axis in [OriginAxis::X, OriginAxis::Y, OriginAxis::Z] {
                        choices.push((
                            format!("{} · {}", feature.name, axis.label()),
                            Source::Axis {
                                axis: AxisSource::CoordinateSystem {
                                    feature: node.clone(),
                                    axis,
                                },
                            },
                        ));
                    }
                    for plane in [OriginPlane::Top, OriginPlane::Front, OriginPlane::Right] {
                        choices.push((
                            format!("{} · {}", feature.name, plane.label()),
                            Source::Plane {
                                plane: PlaneSource::CoordinateSystem {
                                    feature: node.clone(),
                                    plane,
                                },
                            },
                        ));
                    }
                }
                Operation::ReferencePlane { .. } => choices.push((
                    feature.name.clone(),
                    Source::Plane {
                        plane: PlaneSource::Datum { feature: node },
                    },
                )),
                Operation::ReferenceAxis { .. } => choices.push((
                    feature.name.clone(),
                    Source::Axis {
                        axis: AxisSource::Datum { feature: node },
                    },
                )),
                Operation::ReferencePoint { .. } => choices.push((
                    feature.name.clone(),
                    Source::Point {
                        point: PointSource::Datum { feature: node },
                    },
                )),
                Operation::Profile {
                    profile: cad_core::Profile::Sketch { sketch },
                    ..
                } => {
                    if let Some(sketch) = part.sketches.get(sketch) {
                        choices.push((
                            format!("{} · Profile center", feature.name),
                            Source::ProfileCenter {
                                profile: node.clone(),
                            },
                        ));
                        for (index, key) in sketch.points.keys().enumerate() {
                            choices.push((
                                format!("{} · Point {}", feature.name, index + 1),
                                Source::Point {
                                    point: PointSource::SketchPoint {
                                        profile: node.clone(),
                                        point: key.clone(),
                                    },
                                },
                            ));
                        }
                        for (index, (key, _)) in sketch
                            .members
                            .iter()
                            .filter(|(_, m)| {
                                matches!(
                                    m.curve,
                                    cad_core::SketchCurve::Line { .. }
                                        | cad_core::SketchCurve::Circle { .. }
                                        | cad_core::SketchCurve::Arc { .. }
                                )
                            })
                            .enumerate()
                        {
                            choices.push((
                                format!("{} · Sketch axis {}", feature.name, index + 1),
                                Source::Axis {
                                    axis: if matches!(
                                        sketch.members[key].curve,
                                        cad_core::SketchCurve::Line { .. }
                                    ) {
                                        AxisSource::SketchLine {
                                            profile: node.clone(),
                                            member: key.clone(),
                                        }
                                    } else {
                                        AxisSource::SketchCircle {
                                            profile: node.clone(),
                                            member: key.clone(),
                                        }
                                    },
                                },
                            ));
                        }
                        for (index, key) in sketch
                            .members
                            .keys()
                            .chain(sketch.circles.keys())
                            .enumerate()
                        {
                            choices.push((
                                format!("{} · Path member {}", feature.name, index + 1),
                                Source::SketchCurve {
                                    profile: node.clone(),
                                    member: key.clone(),
                                },
                            ));
                        }
                        for (index, key) in sketch.circles.keys().enumerate() {
                            choices.push((
                                format!("{} · Circle axis {}", feature.name, index + 1),
                                Source::Axis {
                                    axis: AxisSource::SketchCircle {
                                        profile: node.clone(),
                                        member: key.clone(),
                                    },
                                },
                            ));
                        }
                    }
                }
                _ => {}
            }
        }
    }
    choices
}
fn nested(doc: &Document, definition: &str) -> Vec<(String, Vec<Id>, Id)> {
    fn visit(
        doc: &Document,
        definition: &str,
        prefix: String,
        path: Vec<Id>,
        active: &mut Vec<Id>,
        result: &mut Vec<(String, Vec<Id>, Id)>,
    ) {
        result.push((
            if prefix.is_empty() {
                "Component origin / local geometry".into()
            } else {
                prefix.clone()
            },
            path.clone(),
            definition.into(),
        ));
        if active.iter().any(|k| k == definition) {
            return;
        }
        active.push(definition.into());
        if let Some(a) = doc.assemblies.get(definition) {
            for i in a.instances.values() {
                let mut p = path.clone();
                p.push(i.id.clone());
                visit(
                    doc,
                    &i.definition,
                    if prefix.is_empty() {
                        i.name.clone()
                    } else {
                        format!("{prefix} / {}", i.name)
                    },
                    p,
                    active,
                    result,
                );
            }
        }
        active.pop();
    }
    let mut result = Vec::new();
    visit(
        doc,
        definition,
        String::new(),
        Vec::new(),
        &mut Vec::new(),
        &mut result,
    );
    result
}
fn source_name(doc: &Document, source: &Source) -> String {
    let name = |node: &NodeRef| {
        doc.parts
            .get(&node.definition)
            .and_then(|p| p.features.iter().find(|f| f.id == node.node))
            .map(|f| f.name.clone())
            .unwrap_or_else(|| "Missing reference".into())
    };
    match source {
        Source::ProfileCenter { profile } => format!("{} · profile center", name(profile)),
        Source::FaceCenter { reference, .. } => {
            format!("{} · planar face centroid", name(&reference.producer))
        }
        Source::EdgeCurve { reference, .. } => {
            format!("{} · exact model path", name(&reference.producer))
        }
        Source::SketchCurve { profile, .. } => format!("{} · exact sketch path", name(profile)),
        Source::CoordinateSystem { feature } => {
            format!("{} · Complete frame (Z / X)", name(feature))
        }
        Source::Plane {
            plane: PlaneSource::CoordinateSystem { feature, plane },
        } => format!("{} · {}", name(feature), plane.label()),
        Source::Axis {
            axis: AxisSource::CoordinateSystem { feature, axis },
        } => format!("{} · {}", name(feature), axis.label()),
        Source::Plane {
            plane: PlaneSource::Origin { plane },
        } => plane.label().into(),
        Source::Plane {
            plane: PlaneSource::Datum { feature },
        } => name(feature),
        Source::Plane {
            plane: PlaneSource::Face { reference, .. },
        } => format!("{} · planar face", name(&reference.producer)),
        Source::Axis {
            axis: AxisSource::Origin { axis },
        } => format!("{axis:?} axis"),
        Source::Axis {
            axis: AxisSource::Datum { feature },
        } => name(feature),
        Source::Axis {
            axis: AxisSource::Edge { reference, .. },
        } => format!("{} · straight edge", name(&reference.producer)),
        Source::FaceAxis { reference, .. }
        | Source::Axis {
            axis: AxisSource::FaceAxis { reference, .. },
        } => format!("{} · rotational face axis", name(&reference.producer)),
        Source::Axis {
            axis: AxisSource::CircleEdge { reference, .. },
        } => format!("{} · circular edge axis", name(&reference.producer)),
        Source::Axis {
            axis: AxisSource::SketchLine { profile, .. },
        } => format!("{} · sketch line", name(profile)),
        Source::Axis {
            axis: AxisSource::SketchCircle { profile, .. },
        } => format!("{} · sketch circle axis", name(profile)),
        Source::Point {
            point: PointSource::Datum { feature },
        } => name(feature),
        Source::Point {
            point: PointSource::SketchPoint { profile, .. },
        } => format!("{} · sketch point", name(profile)),
        Source::Point {
            point: PointSource::Coordinates { .. },
        } => "Origin / coordinates".into(),
        Source::Point {
            point: PointSource::EdgePoint { source },
        } => format!(
            "{} · edge parameter {}",
            name(&source.body),
            source.parameter.0
        ),
        Source::Point { .. } => "Reference origin".into(),
    }
}
pub(crate) fn selected_reference(w: &Workspace, assembly: &Assembly) -> Result<(Id, Reference)> {
    let selected = w.selected.as_ref().ok_or_else(|| {
        CadError::Invalid(
            "Select a planar/rotational face, straight/circular edge, or datum on a component"
                .into(),
        )
    })?;
    let doc = w.session.visible();
    let mut definition = doc.root.clone();
    let mut direct = None;
    for (index, key) in selected.path.iter().enumerate() {
        let a = doc.assemblies.get(&definition).ok_or_else(|| {
            CadError::Invalid("Selected component path is not part of this assembly".into())
        })?;
        let instance = a
            .instances
            .get(key)
            .ok_or_else(|| CadError::Invalid("Selected component is missing".into()))?;
        if a.id == assembly.id {
            direct = Some((key.clone(), selected.path[index + 1..].to_vec()));
            break;
        }
        definition = instance.definition.clone();
    }
    let (participant, path) = direct.ok_or_else(|| {
        CadError::Invalid("Select geometry on a component of the assembly being edited".into())
    })?;
    let source = match selected.reference.topology.as_str() {
        "face" if crate::reference_ui::selected_face_planar(w, selected) => Source::Plane {
            plane: PlaneSource::Face {
                reference: selected.reference.clone(),
                body: crate::reference_ui::selected_body(w, selected),
            },
        },
        "face" => Source::FaceAxis {
            reference: selected.reference.clone(),
            body: crate::reference_ui::selected_body(w, selected),
        },
        "edge"
            if parsed(w)
                .ok()
                .is_some_and(|m| matches!(m.kind.as_str(), "path" | "slot" | "cam")) =>
        {
            Source::EdgeCurve {
                reference: selected.reference.clone(),
                body: crate::reference_ui::selected_body(w, selected),
            }
        }
        "edge" => Source::Axis {
            axis: crate::reference_ui::picked_axis(w, selected),
        },
        "plane" => Source::Plane {
            plane: PlaneSource::from_datum(&selected.reference)?,
        },
        "axis" => Source::Axis {
            axis: AxisSource::from_datum(&selected.reference)?,
        },
        "point" => Source::Point {
            point: PointSource::from_datum(&selected.reference)?,
        },
        "coordinate_system" => Source::CoordinateSystem {
            feature: selected.reference.producer.clone(),
        },
        _ => {
            return Err(CadError::Unsupported(
                "This geometry does not yet supply an exact mate reference".into(),
            ));
        }
    };
    let reference = Reference { path, source };
    assembly_geometry::validate_reference(
        doc,
        &assembly.instances[&participant].definition,
        &reference,
    )?;
    Ok((participant, reference))
}
fn current_evaluation(w: &Workspace) -> Result<&cad_core::protocol::Evaluation> {
    viewport::display_evaluation(w)
        .filter(|e| e.valid && e.identity == w.session.identity())
        .ok_or_else(|| {
            CadError::Invalid(
                "Wait for the current component placements to finish validation".into(),
            )
        })
}
pub fn preview_semantics(w: &Workspace) -> Vec<cad_core::presentation::SemanticElement> {
    let Some(assembly) = w.fields.get("assembly-id") else {
        return vec![];
    };
    let Ok(mate) = parsed(w) else {
        return vec![];
    };
    let doc = w.session.visible();
    let Some(source) = doc.assemblies.get(assembly) else {
        return vec![];
    };
    let evaluation = if synchronized(w, assembly, &mate) {
        current_evaluation(w).ok()
    } else {
        None
    };
    let solved = evaluation.and_then(|e| {
        e.assemblies.get(&cad_core::configuration::key(
            assembly,
            doc.active_configurations.get(assembly).map(String::as_str),
        ))
    });
    let world = solved.and_then(|a| {
        let frames = a.frames.get(&mate.id)?;
        Some([
            frames[0]
                .transformed(&a.placements.get(&mate.first)?.matrix())
                .ok()?,
            frames[1]
                .transformed(&a.placements.get(&mate.second)?.matrix())
                .ok()?,
        ])
    });
    let mut out = vec![];
    for (index, participant, reference) in [
        (0, &mate.first, &mate.first_reference),
        (1, &mate.second, &mate.second_reference),
    ] {
        let name = source
            .instances
            .get(participant)
            .map(|i| i.name.as_str())
            .unwrap_or("Missing component");
        out.push(cad_core::presentation::SemanticElement {
            id: format!("mate:{}:reference:{index}:preview", mate.id),
            role: "mate-reference-preview".into(),
            label: format!(
                "{} reference · {name}",
                if index == 0 { "First" } else { "Second" }
            ),
            value: Some(
                reference
                    .as_ref()
                    .map(|r| source_name(doc, &r.source))
                    .unwrap_or_else(|| "Legacy numeric frame".into()),
            ),
            state: if world.is_some() {
                "evaluated"
            } else {
                "pending"
            }
            .into(),
            commands: vec![],
            relationships: vec![assembly.clone(), participant.clone(), mate.id.clone()],
            data: serde_json::json!({
                "assembly": assembly, "mate": mate.id, "instance": participant,
                "reference": reference,
                "world_frame": world.as_ref().map(|frames| &frames[index]),
                "evaluation_identity": evaluation.map(|e| &e.identity),
                "editable": false,
            }),
        });
    }
    if matches!(mate.kind.as_str(), "coincident" | "planar" | "distance") {
        let measured = world.as_ref().map(|frames| {
            let [first, second] = frames;
            let delta = DVec3::from_array(second.origin) - DVec3::from_array(first.origin);
            if first.kind == FrameKind::Plane || second.kind == FrameKind::Plane {
                let (normal, reference) = if first.kind == FrameKind::Plane {
                    (first.axis, "first")
                } else {
                    (second.axis, "second")
                };
                format!(
                    "Solved signed gap {:.4} mm · second − first along {reference} normal",
                    delta.dot(DVec3::from_array(normal))
                )
            } else if first.kind == FrameKind::Axis || second.kind == FrameKind::Axis {
                let normal = if first.kind == FrameKind::Axis {
                    first.axis
                } else {
                    second.axis
                };
                format!(
                    "Solved radial distance {:.4} mm",
                    delta.cross(DVec3::from_array(normal)).length()
                )
            } else {
                format!("Solved point distance {:.4} mm", delta.length())
            }
        });
        out.push(cad_core::presentation::SemanticElement {
            id: format!("mate:{}:gap:preview", mate.id),
            role: "mate-gap-preview".into(),
            label: "Current reference separation".into(),
            value: Some(measured.unwrap_or_else(|| "Updating reference separation…".into())),
            state: if world.is_some() {
                "evaluated"
            } else {
                "pending"
            }
            .into(),
            commands: vec![],
            relationships: vec![
                assembly.clone(),
                mate.id.clone(),
                mate.first.clone(),
                mate.second.clone(),
            ],
            data: serde_json::json!({
                "assembly": assembly, "mate": mate.id, "requested_offset": mate.offset,
                "world_frames": world, "evaluation_identity": evaluation.map(|e| &e.identity),
                "editable": false,
            }),
        });
    }
    out
}
fn lock_placement(w: &Workspace, assembly: &str, mate: &Mate) -> Result<cad_core::Placement> {
    let evaluation = current_evaluation(w)?;
    let a = evaluation
        .assemblies
        .get(&cad_core::configuration::key(
            assembly,
            w.session
                .visible()
                .active_configurations
                .get(assembly)
                .map(String::as_str),
        ))
        .ok_or_else(|| CadError::Invalid("Assembly placements are not evaluated".into()))?;
    assembly_geometry::relative_placement(
        a.placements
            .get(&mate.first)
            .ok_or_else(|| CadError::Invalid("First component placement is missing".into()))?,
        a.placements
            .get(&mate.second)
            .ok_or_else(|| CadError::Invalid("Second component placement is missing".into()))?,
    )
}
fn rotation(w: &Workspace, assembly: &str, mate: &Mate) -> Result<cad_core::Expr> {
    let evaluation = current_evaluation(w)?;
    let a = evaluation
        .assemblies
        .get(&cad_core::configuration::key(
            assembly,
            w.session
                .visible()
                .active_configurations
                .get(assembly)
                .map(String::as_str),
        ))
        .ok_or_else(|| CadError::Invalid("Assembly placements are not evaluated".into()))?;
    let frames = a
        .frames
        .get(&mate.id)
        .ok_or_else(|| CadError::Invalid("Concentric references are not evaluated".into()))?;
    let first = frames[0].transformed(&a.placements[&mate.first].matrix())?;
    let second = frames[1].transformed(&a.placements[&mate.second].matrix())?;
    let axis = DVec3::from_array(first.axis);
    let u = DVec3::from_array(first.secondary);
    let v = DVec3::from_array(second.secondary);
    Ok(cad_core::Expr(format!(
        "{} deg",
        axis.dot(u.cross(v)).atan2(u.dot(v)).to_degrees()
    )))
}
fn parsed(w: &Workspace) -> Result<Mate> {
    let mate: Mate = serde_json::from_str(&w.json)?;
    if w.fields.get("mate-editor") != Some(&mate.id) {
        return Err(CadError::Invalid(
            "The mate identity must remain unchanged".into(),
        ));
    }
    Ok(mate)
}
pub fn current_source_is_valid(w: &Workspace) -> bool {
    w.fields
        .get("assembly-id")
        .and_then(|assembly| parsed(w).ok().map(|mate| synchronized(w, assembly, &mate)))
        .unwrap_or(false)
}
fn synchronized(w: &Workspace, assembly: &str, mate: &Mate) -> bool {
    w.session
        .visible()
        .assemblies
        .get(assembly)
        .and_then(|a| a.mates.iter().find(|m| m.id == mate.id))
        .is_some_and(|m| m == mate)
}
fn submit(w: &mut Workspace, assembly: &str, mate: Mate) {
    w.dispatch(Command::PutMate {
        assembly: assembly.into(),
        mate: mate.clone().into(),
    });
    if synchronized(w, assembly, &mate) {
        w.fields.remove("mate-source-invalid");
    } else {
        w.fields.insert("mate-source-invalid".into(), "true".into());
    }
}
pub fn begin(w: &mut Workspace, assembly: &str, mate: Mate) {
    if w.fields.contains_key("exploded-view-editor")
        || w.fields.contains_key("assembly-inspection-editor")
        || w.fields.contains_key("assembly-weld-editor")
    {
        w.status = "Confirm or cancel the current assembly operation before editing mates".into();
        return;
    }
    w.start_tool();
    w.fields.insert("assembly-id".into(), assembly.into());
    w.fields.insert("mate-editor".into(), mate.id.clone());
    w.fields.remove("mate-source-invalid");
    w.json = serde_json::to_string_pretty(&mate).unwrap_or_default();
    w.open_editor("assembly");
}
pub fn new(w: &mut Workspace, assembly: &str) {
    let doc = w.session.visible();
    let Some(a) = doc.assemblies.get(assembly).cloned() else {
        return;
    };
    let keys = a.instances.keys().take(2).cloned().collect::<Vec<_>>();
    if keys.len() != 2 {
        w.status = "Add two components before creating a mate".into();
        return;
    }
    let origin = Reference {
        path: Vec::new(),
        source: Source::Plane {
            plane: PlaneSource::Origin {
                plane: OriginPlane::Top,
            },
        },
    };
    let mut mate = Mate {
        id: cad_core::id(),
        kind: "coincident".into(),
        first: keys[0].clone(),
        second: keys[1].clone(),
        first_origin: [0.; 3],
        second_origin: [0.; 3],
        first_axis: [0., 0., 1.],
        second_axis: [0., 0., 1.],
        offset: "0 mm".into(),
        first_reference: Some(origin.clone()),
        second_reference: Some(origin),
        alignment: Alignment::Either,
        lock_rotation: false,
        rotation_offset: "0 deg".into(),
        lock_placement: None,
        relationship: None,
    };
    if let Ok((participant, reference)) = selected_reference(w, &a) {
        mate.first = participant;
        mate.second = a
            .instances
            .keys()
            .find(|key| *key != &mate.first)
            .cloned()
            .unwrap_or(keys[1].clone());
        if matches!(
            reference.source,
            Source::Axis { .. } | Source::FaceAxis { .. }
        ) {
            mate.kind = "concentric".into();
            mate.second_reference = Some(Reference {
                path: Vec::new(),
                source: Source::Axis {
                    axis: AxisSource::Origin {
                        axis: OriginAxis::Z,
                    },
                },
            });
        }
        mate.first_reference = Some(reference);
    }
    begin(w, assembly, mate.clone());
    submit(w, assembly, mate);
}
fn participant_editor(
    ui: &mut egui::Ui,
    w: &mut Workspace,
    doc: &Document,
    a: &Assembly,
    label: &str,
    mate: &mut Mate,
    first: bool,
) -> bool {
    let (participant, reference, origin, axis) = if first {
        (
            &mut mate.first,
            &mut mate.first_reference,
            &mut mate.first_origin,
            &mut mate.first_axis,
        )
    } else {
        (
            &mut mate.second,
            &mut mate.second_reference,
            &mut mate.second_origin,
            &mut mate.second_axis,
        )
    };
    reference_editor(
        ui,
        w,
        doc,
        a,
        label,
        ReferenceFields {
            participant,
            reference,
            origin,
            axis,
        },
    )
}
struct ReferenceFields<'a> {
    participant: &'a mut Id,
    reference: &'a mut Option<Reference>,
    origin: &'a mut [f64; 3],
    axis: &'a mut [f64; 3],
}
fn reference_editor(
    ui: &mut egui::Ui,
    w: &mut Workspace,
    doc: &Document,
    a: &Assembly,
    label: &str,
    fields: ReferenceFields<'_>,
) -> bool {
    let ReferenceFields {
        participant,
        reference,
        origin,
        axis,
    } = fields;
    let mut changed = false;
    let kind = parsed(w).ok().map(|m| m.kind).unwrap_or_default();
    let role = if label == "First reference" {
        "first"
    } else if label == "Second reference" {
        "second"
    } else {
        "auxiliary_plane"
    };
    let classes = assembly_geometry::reference_classes(&kind, role);
    ui.push_id(label, |ui| {
        ui.strong(label);
        egui::ComboBox::from_id_salt("component")
            .selected_text(
                a.instances
                    .get(participant)
                    .map(|i| i.name.as_str())
                    .unwrap_or("Missing component"),
            )
            .show_ui(ui, |ui| {
                for i in a.instances.values() {
                    if ui
                        .selectable_value(participant, i.id.clone(), &i.name)
                        .changed()
                    {
                        changed = true;
                        *reference = Some(Reference {
                            path: Vec::new(),
                            source: Source::Plane {
                                plane: PlaneSource::Origin {
                                    plane: OriginPlane::Top,
                                },
                            },
                        });
                    }
                }
            });
        if ui
            .add_enabled(
                w.selected.is_some(),
                egui::Button::new("Use selected model geometry"),
            )
            .clicked()
        {
            match selected_reference(w, a) {
                Ok((i, mut r)) => {
                    let kind = parsed(w).ok().map(|m| m.kind).unwrap_or_default();
                    if matches!(kind.as_str(), "path" | "slot" | "cam")
                        && label == "First reference"
                    {
                        r.source = match r.source {
                            Source::Axis {
                                axis:
                                    AxisSource::Edge { reference, body }
                                    | AxisSource::CircleEdge { reference, body },
                            } => Source::EdgeCurve { reference, body },
                            source => source,
                        };
                    }
                    if kind == "profile_center" {
                        r.source = match r.source {
                            Source::Plane {
                                plane: PlaneSource::Face { reference, body },
                            } => Source::FaceCenter { reference, body },
                            Source::SketchCurve { profile, .. }
                            | Source::Axis {
                                axis:
                                    AxisSource::SketchLine { profile, .. }
                                    | AxisSource::SketchCircle { profile, .. },
                            } => Source::ProfileCenter { profile },
                            source => source,
                        };
                    }
                    if !classes.contains(&r.source.class()) {
                        w.status = format!("{} requires {:?} geometry here", kind, classes);
                        return;
                    }
                    *participant = i;
                    *reference = Some(r);
                    changed = true;
                }
                Err(e) => w.status = e.to_string(),
            }
        }
        let Some(instance) = a.instances.get(participant) else {
            return;
        };
        let paths = nested(doc, &instance.definition);
        if let Some(reference) = reference {
            let selected = paths.iter().find(|(_, path, _)| path == &reference.path);
            egui::ComboBox::from_id_salt("nested-component")
                .selected_text(
                    selected
                        .map(|(name, _, _)| name.as_str())
                        .unwrap_or("Missing nested component"),
                )
                .show_ui(ui, |ui| {
                    for (label, path, _) in &paths {
                        if ui
                            .selectable_value(&mut reference.path, path.clone(), label)
                            .changed()
                        {
                            reference.source = Source::Plane {
                                plane: PlaneSource::Origin {
                                    plane: OriginPlane::Top,
                                },
                            };
                            changed = true;
                        }
                    }
                });
        }
        let leaf = reference
            .as_ref()
            .and_then(|r| paths.iter().find(|(_, path, _)| path == &r.path))
            .map(|(_, _, definition)| definition.as_str())
            .unwrap_or(&instance.definition);
        egui::ComboBox::from_id_salt("geometry-reference")
            .selected_text(
                reference
                    .as_ref()
                    .map(|r| source_name(doc, &r.source))
                    .unwrap_or_else(|| "Legacy numeric frame".into()),
            )
            .show_ui(ui, |ui| {
                if ui
                    .selectable_label(reference.is_none(), "Legacy numeric frame")
                    .clicked()
                    && reference.is_some()
                {
                    *reference = None;
                    changed = true;
                }
                for (label, source) in choices(doc, leaf)
                    .into_iter()
                    .filter(|(_, source)| classes.contains(&source.class()))
                {
                    if ui
                        .selectable_label(
                            reference.as_ref().is_some_and(|r| r.source == source),
                            label,
                        )
                        .clicked()
                        && reference.as_ref().is_none_or(|r| r.source != source)
                    {
                        let path = reference
                            .as_ref()
                            .map(|r| r.path.clone())
                            .unwrap_or_default();
                        *reference = Some(Reference { path, source });
                        changed = true;
                    }
                }
                if classes.contains(&assembly_geometry::ReferenceClass::Point)
                    && let Some(selected) = w.selected.as_ref().filter(|selected| {
                        selected.definition == leaf && selected.reference.topology == "edge"
                    })
                    && let Ok((selected_participant, picked)) = selected_reference(w, a)
                    && selected_participant == *participant
                    && reference
                        .as_ref()
                        .is_none_or(|current| current.path == picked.path)
                {
                    for (label, parameter) in [
                        ("Selected edge · start", "0"),
                        ("Selected edge · parameter midpoint", "0.5"),
                        ("Selected edge · end", "1"),
                    ] {
                        if ui.selectable_label(false, label).clicked() {
                            *reference = Some(Reference {
                                path: picked.path.clone(),
                                source: Source::Point {
                                    point: PointSource::EdgePoint {
                                        source: Box::new(EdgePointSource {
                                            body: crate::reference_ui::selected_body(w, selected),
                                            reference: selected.reference.clone(),
                                            parameter: parameter.into(),
                                        }),
                                    },
                                },
                            });
                            changed = true;
                        }
                    }
                }
            });
        if let Some(Reference {
            source: Source::Point { point },
            ..
        }) = reference
        {
            changed |= crate::reference_ui::point_for_definition(
                ui,
                w,
                doc,
                "",
                "Point source",
                leaf,
                point,
            );
        }
        if reference.is_none() {
            egui::CollapsingHeader::new("Local numeric frame").show(ui, |ui| {
                for (label, values) in [("Origin (mm)", origin), ("Axis", axis)] {
                    ui.label(label);
                    ui.horizontal(|ui| {
                        for (label, value) in ["X", "Y", "Z"].into_iter().zip(values) {
                            changed |= ui
                                .add(
                                    egui::DragValue::new(value)
                                        .prefix(format!("{label} "))
                                        .speed(0.1),
                                )
                                .changed();
                        }
                    });
                }
            });
        }
    });
    changed
}
fn endpoint_editor(
    ui: &mut egui::Ui,
    w: &mut Workspace,
    doc: &Document,
    a: &Assembly,
    label: &str,
    endpoint: &mut Endpoint,
) -> bool {
    let mut reference = Some(endpoint.reference.clone());
    let mut origin = [0.; 3];
    let mut axis = [0., 0., 1.];
    let changed = reference_editor(
        ui,
        w,
        doc,
        a,
        label,
        ReferenceFields {
            participant: &mut endpoint.instance,
            reference: &mut reference,
            origin: &mut origin,
            axis: &mut axis,
        },
    );
    if let Some(reference) = reference {
        endpoint.reference = reference;
    } else {
        ui.colored_label(
            egui::Color32::from_rgb(180, 45, 30),
            "Additional references require associative geometry.",
        );
    }
    changed
}
fn capture_zero(
    w: &Workspace,
    assembly: &str,
    mate: &Mate,
    support: Option<&Id>,
) -> Result<[Frame; 2]> {
    let stored = w
        .session
        .visible()
        .assemblies
        .get(assembly)
        .and_then(|a| a.mates.iter().find(|m| m.id == mate.id))
        .ok_or_else(|| {
            CadError::Invalid(
                "Validate both participant references before capturing motion zero".into(),
            )
        })?;
    if stored.first != mate.first
        || stored.second != mate.second
        || stored.first_reference != mate.first_reference
        || stored.second_reference != mate.second_reference
        || stored.first_origin != mate.first_origin
        || stored.second_origin != mate.second_origin
        || stored.first_axis != mate.first_axis
        || stored.second_axis != mate.second_axis
    {
        return Err(CadError::Invalid("Participant references must match the current validated source before motion-zero capture".into()));
    }
    let e = current_evaluation(w)?;
    let a = e
        .assemblies
        .get(&cad_core::configuration::key(
            assembly,
            w.session
                .visible()
                .active_configurations
                .get(assembly)
                .map(String::as_str),
        ))
        .ok_or_else(|| CadError::Invalid("Current assembly evaluation is missing".into()))?;
    let frames = a.frames.get(&mate.id).ok_or_else(|| {
        CadError::Invalid(
            "Select and validate both reference axes before capturing motion zero".into(),
        )
    })?;
    let inverse = if let Some(id) = support {
        a.placements
            .get(id)
            .ok_or_else(|| CadError::Invalid("Support occurrence is not evaluated".into()))?
            .matrix()
            .try_inverse()
            .ok_or_else(|| CadError::Invalid("Support transform is singular".into()))?
    } else {
        cad_core::Placement::default().matrix()
    };
    let first = frames[0].transformed(
        &(inverse
            * a.placements
                .get(&mate.first)
                .ok_or_else(|| CadError::Invalid("First participant is not evaluated".into()))?
                .matrix()),
    )?;
    let second = frames[1].transformed(
        &(inverse
            * a.placements
                .get(&mate.second)
                .ok_or_else(|| CadError::Invalid("Second participant is not evaluated".into()))?
                .matrix()),
    )?;
    Ok([
        Frame::new(
            first.origin,
            first.axis,
            Some(first.secondary),
            FrameKind::Axis,
        )?,
        Frame::new(
            second.origin,
            second.axis,
            Some(second.secondary),
            FrameKind::Axis,
        )?,
    ])
}
fn new_relationship(w: &Workspace, assembly: &str, m: &Mate) -> Result<Option<Box<Relationship>>> {
    let endpoint = |first: bool| Endpoint {
        instance: if first {
            m.first.clone()
        } else {
            m.second.clone()
        },
        reference: if first {
            m.first_reference.clone()
        } else {
            m.second_reference.clone()
        }
        .unwrap_or(Reference {
            path: vec![],
            source: Source::Plane {
                plane: PlaneSource::Origin {
                    plane: OriginPlane::Top,
                },
            },
        }),
    };
    let angle_range = || Range {
        minimum: "0 deg".into(),
        maximum: "90 deg".into(),
    };
    let relation = match m.kind.as_str() {
        "profile_center" => Relationship::ProfileCenter { rotation: None },
        "width" => Relationship::Width {
            first_wall: endpoint(true),
            second_wall: endpoint(false),
            position: WidthPosition::Centered,
        },
        "symmetry" => Relationship::Symmetry {
            plane: endpoint(true),
        },
        "limit_distance" => Relationship::LimitDistance {
            range: Range {
                minimum: "0 mm".into(),
                maximum: "10 mm".into(),
            },
        },
        "limit_angle" => Relationship::LimitAngle {
            range: angle_range(),
        },
        "hinge" => Relationship::Hinge {
            first_plane: endpoint(true),
            second_plane: endpoint(false),
            range: None,
        },
        "gear" | "rack_pinion" | "screw" | "linear_coupler" | "universal_joint" => {
            let motion = match m.kind.as_str() {
                "gear" => CouplerKind::Gear,
                "rack_pinion" => CouplerKind::RackPinion,
                "screw" => CouplerKind::Screw,
                "universal_joint" => CouplerKind::UniversalJoint,
                _ => CouplerKind::Linear,
            };
            Relationship::Coupler {
                motion,
                support: None,
                zero: capture_zero(w, assembly, m, None)?,
                ratio: "1".into(),
                pitch: "10 mm".into(),
                phase: if matches!(motion, CouplerKind::Gear | CouplerKind::UniversalJoint) {
                    "0 deg".into()
                } else {
                    "0 mm".into()
                },
                reverse: false,
            }
        }
        "slot" => Relationship::Slot {
            position: CurvePosition::Free { seed: 0.5 },
            orientation: PathOrientation::Free,
        },
        "path" => Relationship::Path {
            position: CurvePosition::Free { seed: 0.5 },
            orientation: PathOrientation::Free,
        },
        "cam" => Relationship::Cam {
            position: CurvePosition::Free { seed: 0.5 },
            follower_radius: "0 mm".into(),
            outward: true,
        },
        _ => return Ok(None),
    };
    Ok(Some(Box::new(relation)))
}
fn expression_field(ui: &mut egui::Ui, label: &str, e: &mut cad_core::Expr) -> bool {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.text_edit_singleline(&mut e.0).changed()
    })
    .inner
}
fn range_editor(ui: &mut egui::Ui, r: &mut Range) -> bool {
    expression_field(ui, "Minimum", &mut r.minimum)
        | expression_field(ui, "Maximum", &mut r.maximum)
}
fn curve_position_editor(ui: &mut egui::Ui, p: &mut CurvePosition) -> bool {
    let mut changed = false;
    let old = matches!(p, CurvePosition::Free { .. });
    let mut free = old;
    egui::ComboBox::from_label("Path position")
        .selected_text(if free {
            "Free"
        } else {
            "Fraction / expression"
        })
        .show_ui(ui, |ui| {
            changed |= ui
                .selectable_value(&mut free, true, "Free along carrier")
                .changed();
            changed |= ui
                .selectable_value(&mut free, false, "Fraction / expression")
                .changed();
        });
    if free != old {
        *p = if free {
            CurvePosition::Free { seed: 0.5 }
        } else {
            CurvePosition::Fraction {
                value: "0.5".into(),
            }
        };
    }
    match p {
        CurvePosition::Free { seed } => {
            ui.label("Initial branch / parameter");
            changed |= ui.add(egui::Slider::new(seed, 0. ..=1.)).changed();
        }
        CurvePosition::Fraction { value } => {
            changed |= expression_field(ui, "Fraction (0..1)", value)
        }
    }
    changed
}
fn orientation_editor(ui: &mut egui::Ui, o: &mut PathOrientation) -> bool {
    let mut changed = false;
    egui::ComboBox::from_label("Follower orientation")
        .selected_text(format!("{o:?}"))
        .show_ui(ui, |ui| {
            for (v, label) in [
                (PathOrientation::Free, "Free rotation"),
                (PathOrientation::Tangent, "Axis along tangent"),
                (PathOrientation::Normal, "Axis normal to path plane"),
            ] {
                changed |= ui.selectable_value(o, v, label).changed();
            }
        });
    changed
}
fn relationship_editor(
    ui: &mut egui::Ui,
    w: &mut Workspace,
    doc: &Document,
    a: &Assembly,
    m: &Mate,
    r: &mut Relationship,
) -> bool {
    let mut changed = false;
    match r {
        Relationship::ProfileCenter { rotation } => {
            ui.small("Choose closed sketch profiles or planar model faces. Centers are area centroids including inner boundaries.");
            let mut lock = rotation.is_some();
            if ui
                .checkbox(&mut lock, "Lock relative profile rotation")
                .changed()
            {
                *rotation = if lock { Some("0 deg".into()) } else { None };
                changed = true;
            }
            if let Some(angle) = rotation {
                changed |= expression_field(ui, "Relative rotation", angle);
            }
        }
        Relationship::Width {
            first_wall,
            second_wall,
            position,
        } => {
            ui.small("First/second references are one wall of the width and tab; select the other parallel wall on each component.");
            changed |= endpoint_editor(ui, w, doc, a, "Other width wall", first_wall);
            changed |= endpoint_editor(ui, w, doc, a, "Other tab wall", second_wall);
            let mut mode = match position {
                WidthPosition::Centered => 0,
                WidthPosition::Free => 1,
                WidthPosition::Distance { .. } => 2,
                WidthPosition::Fraction { .. } => 3,
            };
            let old = mode;
            egui::ComboBox::from_label("Width positioning")
                .selected_text(["Centered", "Free", "Distance", "Fraction"][mode])
                .show_ui(ui, |ui| {
                    for (i, label) in [
                        "Centered",
                        "Free within width",
                        "Distance from first wall",
                        "Fraction of available travel",
                    ]
                    .into_iter()
                    .enumerate()
                    {
                        changed |= ui.selectable_value(&mut mode, i, label).changed();
                    }
                });
            if mode != old {
                *position = match mode {
                    0 => WidthPosition::Centered,
                    1 => WidthPosition::Free,
                    2 => WidthPosition::Distance {
                        value: "0 mm".into(),
                    },
                    _ => WidthPosition::Fraction {
                        value: "0.5".into(),
                    },
                };
            }
            match position {
                WidthPosition::Distance { value } => {
                    changed |= expression_field(ui, "Distance", value)
                }
                WidthPosition::Fraction { value } => {
                    changed |= expression_field(ui, "Fraction (0..1)", value)
                }
                _ => {}
            }
        }
        Relationship::Symmetry { plane } => {
            ui.small(
                "Reflect the two selected points, origins, or directional frames about this plane.",
            );
            changed |= endpoint_editor(ui, w, doc, a, "Symmetry plane", plane);
        }
        Relationship::LimitDistance { range } | Relationship::LimitAngle { range } => {
            changed |= range_editor(ui, range)
        }
        Relationship::Hinge {
            first_plane,
            second_plane,
            range,
        } => {
            ui.small(
                "Primary references are hinge axes. Seating planes must be normal to those axes.",
            );
            changed |= endpoint_editor(ui, w, doc, a, "First seating plane", first_plane);
            changed |= endpoint_editor(ui, w, doc, a, "Second seating plane", second_plane);
            let mut limited = range.is_some();
            if ui.checkbox(&mut limited, "Limit hinge rotation").changed() {
                changed = true;
                *range = if limited {
                    Some(Range {
                        minimum: "-90 deg".into(),
                        maximum: "90 deg".into(),
                    })
                } else {
                    None
                };
            }
            if let Some(r) = range {
                changed |= range_editor(ui, r);
            }
        }
        Relationship::Coupler {
            motion,
            support,
            zero,
            ratio,
            pitch,
            phase,
            reverse,
        } => {
            ui.small("Motion is measured from captured axes in the support frame. Supporting mates locate the shafts and sliding components.");
            let previous = support.clone();
            egui::ComboBox::from_label("Motion support")
                .selected_text(
                    support
                        .as_ref()
                        .and_then(|id| a.instances.get(id))
                        .map(|i| i.name.as_str())
                        .unwrap_or("Assembly universe"),
                )
                .show_ui(ui, |ui| {
                    changed |= ui
                        .selectable_value(support, None, "Assembly universe")
                        .changed();
                    for i in a
                        .instances
                        .values()
                        .filter(|i| i.id != m.first && i.id != m.second)
                    {
                        changed |= ui
                            .selectable_value(support, Some(i.id.clone()), &i.name)
                            .changed();
                    }
                });
            if *support != previous {
                match capture_zero(w, &a.id, m, support.as_ref()) {
                    Ok(frames) => *zero = frames,
                    Err(e) => w.status = e.to_string(),
                }
            }
            if matches!(motion, CouplerKind::Gear | CouplerKind::Linear) {
                changed |= expression_field(ui, "Ratio", ratio);
            } else if *motion != CouplerKind::UniversalJoint {
                changed |= expression_field(
                    ui,
                    if *motion == CouplerKind::Screw {
                        "Lead per revolution"
                    } else {
                        "Rack travel per revolution"
                    },
                    pitch,
                );
            }
            changed |= expression_field(ui, "Phase / offset", phase);
            changed |= ui.checkbox(reverse, "Reverse relative motion").changed();
            if ui.button("Capture current motion zero").clicked() {
                match capture_zero(w, &a.id, m, support.as_ref()) {
                    Ok(frames) => {
                        *zero = frames;
                        changed = true;
                    }
                    Err(e) => w.status = e.to_string(),
                }
            }
            ui.small("Angular coordinates use the captured ±180° branch. Zero capture records the current validated placement.");
        }
        Relationship::Slot {
            position,
            orientation,
        }
        | Relationship::Path {
            position,
            orientation,
        } => {
            ui.small("First: planar sketch curve or model edge. Second: follower point, origin, or axis. Fraction is the exact carrier parameter.");
            changed |= curve_position_editor(ui, position);
            changed |= orientation_editor(ui, orientation);
        }
        Relationship::Cam {
            position,
            follower_radius,
            outward,
        } => {
            ui.small("First: planar sketch curve or model edge. Second: roller center or point follower. Left/right follows the profile direction.");
            changed |= curve_position_editor(ui, position);
            changed |=
                expression_field(ui, "Roller radius (0 for point follower)", follower_radius);
            changed |= ui
                .checkbox(outward, "Use right side of directed profile")
                .changed();
        }
    }
    changed
}

pub fn show(ui: &mut egui::Ui, w: &mut Workspace, assembly: &str) {
    if !w.fields.contains_key("mate-editor") {
        return;
    }
    let doc = w.session.visible().clone();
    let Some(a) = doc.assemblies.get(assembly) else {
        return;
    };
    let mut changed = false;
    if let Ok(mut mate) = parsed(w) {
        if synchronized(w, assembly, &mate) {
            w.fields.remove("mate-source-invalid");
        } else {
            w.fields.insert("mate-source-invalid".into(), "true".into());
        }
        let old_mate = mate.clone();
        let old_kind = mate.kind.clone();
        egui::ComboBox::from_label("Mate type")
            .selected_text(&mate.kind)
            .show_ui(ui, |ui| {
                let mut group = "";
                for mode in assembly_geometry::MATE_MODES {
                    if mode.group != group {
                        if !group.is_empty() {
                            ui.separator();
                        }
                        ui.strong(mode.group);
                        group = mode.group;
                    }
                    changed |= ui
                        .selectable_value(&mut mate.kind, mode.kind.into(), mode.label)
                        .on_hover_text(mode.domain)
                        .changed();
                }
            });
        if mate.kind != old_kind {
            mate.offset = if mate.kind == "angle" {
                "0 deg".into()
            } else {
                "0 mm".into()
            };
            mate.lock_rotation = false;
            match new_relationship(w, assembly, &mate) {
                Ok(relationship) => mate.relationship = relationship,
                Err(e) => {
                    mate = old_mate;
                    w.status = e.to_string();
                }
            }
            if mate.kind == "lock" {
                match lock_placement(w, assembly, &mate) {
                    Ok(p) => mate.lock_placement = Some(p),
                    Err(e) => {
                        mate.lock_placement = None;
                        w.status = e.to_string();
                    }
                }
            }
        }
        changed |= participant_editor(ui, w, &doc, a, "First reference", &mut mate, true);
        changed |= participant_editor(ui, w, &doc, a, "Second reference", &mut mate, false);
        if matches!(
            mate.kind.as_str(),
            "distance" | "angle" | "planar" | "coincident"
        ) {
            ui.label(if mate.kind == "angle" {
                "Angle / expression"
            } else {
                "Gap / distance / expression"
            });
            changed |= ui.text_edit_singleline(&mut mate.offset.0).changed();
            if mate.kind != "angle" {
                for element in preview_semantics(w)
                    .into_iter()
                    .filter(|e| e.role == "mate-gap-preview")
                {
                    crate::presentation_controls::label(
                        ui,
                        &element.id,
                        &element.role,
                        &[],
                        element.value.as_deref().unwrap_or_default(),
                        &element.state,
                    );
                }
                ui.small("For planar references, the gap is signed from first to second. Reverse the sign or swap references to place the other side.");
            }
        }
        if !matches!(
            mate.kind.as_str(),
            "lock" | "gear" | "rack_pinion" | "screw" | "linear_coupler" | "universal_joint"
        ) {
            egui::ComboBox::from_label("Alignment")
                .selected_text(format!("{:?}", mate.alignment))
                .show_ui(ui, |ui| {
                    for (value, label) in [
                        (Alignment::Either, "Either direction"),
                        (Alignment::Aligned, "Aligned"),
                        (Alignment::Opposed, "Opposed"),
                    ] {
                        changed |= ui
                            .selectable_value(&mut mate.alignment, value, label)
                            .changed();
                    }
                });
        }
        if mate.kind == "concentric" {
            changed |= ui
                .checkbox(&mut mate.lock_rotation, "Lock relative rotation")
                .changed();
            if mate.lock_rotation {
                changed |= ui
                    .text_edit_singleline(&mut mate.rotation_offset.0)
                    .changed();
                if ui.button("Capture current rotation").clicked() {
                    match rotation(w, assembly, &mate) {
                        Ok(e) => {
                            mate.rotation_offset = e;
                            changed = true;
                        }
                        Err(e) => w.status = e.to_string(),
                    }
                }
            }
        }
        if mate.kind == "lock" && ui.button("Capture current relative placement").clicked() {
            match lock_placement(w, assembly, &mate) {
                Ok(p) => {
                    mate.lock_placement = Some(p);
                    changed = true;
                }
                Err(e) => w.status = e.to_string(),
            }
        }
        let source = mate.clone();
        if let Some(relationship) = mate.relationship.as_deref_mut() {
            changed |= relationship_editor(ui, w, &doc, a, &source, relationship);
        }
        if changed {
            w.json = serde_json::to_string_pretty(&mate).unwrap_or_default();
            submit(w, assembly, mate);
        }
    } else {
        w.fields.insert("mate-source-invalid".into(), "true".into());
    }
    let advanced = egui::CollapsingHeader::new("Advanced Source")
        .show(ui, |ui| {
            ui.add(
                egui::TextEdit::multiline(&mut w.json)
                    .code_editor()
                    .desired_width(f32::INFINITY)
                    .desired_rows(8),
            )
            .changed()
        })
        .body_returned
        .unwrap_or(false);
    if advanced {
        match parsed(w) {
            Ok(mate) => {
                submit(w, assembly, mate);
            }
            Err(e) => {
                w.fields.insert("mate-source-invalid".into(), "true".into());
                w.status = e.to_string();
            }
        }
    }
    if let Some(draft) = &w.session.draft {
        if draft.pending {
            ui.small("Updating component placements…");
        } else if draft.error.is_none()
            && let Some(report) = draft
                .preview_evaluation()
                .and_then(|e| {
                    e.assemblies.get(&cad_core::configuration::key(
                        assembly,
                        w.session
                            .visible()
                            .active_configurations
                            .get(assembly)
                            .map(String::as_str),
                    ))
                })
                .and_then(|a| a.report.as_ref())
        {
            ui.small(format!(
                "{} remaining degrees of freedom",
                report.degrees_of_freedom
            ));
        }
    }
    if let Ok(e) = current_evaluation(w)
        && let Some(evaluated) = e.assemblies.get(&cad_core::configuration::key(
            assembly,
            w.session
                .visible()
                .active_configurations
                .get(assembly)
                .map(String::as_str),
        ))
    {
        if let Some(id) = w.fields.get("mate-editor")
            && let Some(diagnostic) = evaluated.diagnostics.get(id)
        {
            ui.small(format!(
                "{} independent equations · residual {:.3e}",
                diagnostic.independent_equations, diagnostic.maximum_residual
            ));
            if let Some(t) = diagnostic.curve_parameter {
                ui.small(format!("Solved carrier parameter {:.6}", t));
            }
        }
        egui::CollapsingHeader::new("Solved placements / component motion").show(ui,|ui|{for(id,motion)in &evaluated.component_motion{crate::assembly_tools_ui::show_placement(ui,w,assembly,id);ui.small(format!("{} translational / {} rotational motion directions",motion.translation_directions.len(),motion.rotation_directions.len()));}ui.small("Directions describe local infinitesimal motion; coupled components may share a degree of freedom. Input coordinates are solver seeds; solved placements are the current model positions.");});
    }
}
