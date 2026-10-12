//! Part weld authoring. Every saved seam is source geometry, never a display path.
use crate::{Workspace, ui, viewport};
use cad_core::{
    Feature, NodeRef, Operation,
    commands::Command,
    weld_beads::{Definition, FaceSource, SeamSource},
};

pub(crate) fn selected_seam(w: &Workspace, part_local: bool) -> Option<SeamSource> {
    if let Some(selected) = w
        .selected
        .as_ref()
        .filter(|selected| selected.reference.topology == "edge")
    {
        return Some(SeamSource::ModelEdge {
            body: crate::reference_ui::selected_body(w, selected),
            reference: Box::new(selected.reference.clone()),
            path: if part_local {
                vec![]
            } else {
                selected.path.clone()
            },
        });
    }
    if let Some(cad_core::selection_actions::SelectionTarget::Sketch {
        definition,
        feature,
        entities,
        ..
    }) = &w.context_selection
    {
        let member = entities.iter().find_map(|entity| match entity {
            cad_core::sketch_actions::SketchEntity::Member { member } => Some(member.clone()),
            _ => None,
        })?;
        return Some(SeamSource::SketchMember {
            profile: NodeRef {
                definition: definition.clone(),
                node: feature.clone(),
            },
            member,
            path: if part_local {
                vec![]
            } else {
                w.selected_feature_path.clone().unwrap_or_default()
            },
        });
    }
    let owner = w.view.active_part.as_ref()?;
    let feature = w
        .session
        .visible()
        .parts
        .get(owner)?
        .features
        .iter()
        .find(|feature| Some(&feature.id) == w.view.active_node.as_ref())?;
    match &feature.operation {
        Operation::ReferenceCurve { .. } => Some(SeamSource::ReferenceCurve {
            feature: NodeRef {
                definition: owner.clone(),
                node: feature.id.clone(),
            },
            piece: None,
            path: if part_local {
                vec![]
            } else {
                w.selected_feature_path.clone().unwrap_or_default()
            },
        }),
        _ => None,
    }
}
pub(crate) fn selected_supports(w: &Workspace, part_local: bool) -> Vec<FaceSource> {
    let mut result = vec![];
    for selected in w
        .geometry_selection
        .iter()
        .chain(w.selected.iter())
        .filter(|selected| selected.reference.topology == "face")
    {
        let face = FaceSource {
            body: crate::reference_ui::selected_body(w, selected),
            reference: Box::new(selected.reference.clone()),
            path: if part_local {
                vec![]
            } else {
                selected.path.clone()
            },
        };
        if !result.contains(&face) {
            result.push(face);
        }
    }
    result
}
pub(crate) fn seam_choices(w: &Workspace, owner: &str, current: &str) -> Vec<(SeamSource, String)> {
    let doc = w.session.visible();
    let Some(part) = doc.parts.get(owner) else {
        return vec![];
    };
    let prior = part
        .features
        .iter()
        .take_while(|feature| feature.id != current)
        .filter(|feature| !feature.suppressed)
        .collect::<Vec<_>>();
    let mut result = vec![];
    if let Some(mesh) =
        viewport::display_evaluation(w).and_then(|evaluation| evaluation.meshes.get(owner))
    {
        for (index, edge) in mesh.edges.iter().enumerate() {
            let Some(body) = &edge.body else {
                continue;
            };
            if body.definition != owner || !prior.iter().any(|feature| feature.id == body.node) {
                continue;
            }
            let seam = SeamSource::ModelEdge {
                body: body.clone(),
                reference: Box::new(edge.reference.clone()),
                path: vec![],
            };
            if !result.iter().any(|(source, _)| *source == seam) {
                let name = prior
                    .iter()
                    .find(|feature| feature.id == body.node)
                    .map(|feature| feature.name.as_str())
                    .unwrap_or("Body");
                result.push((seam, format!("{name} / Edge {}", index + 1)));
            }
        }
    }
    for feature in prior {
        let source = NodeRef {
            definition: owner.into(),
            node: feature.id.clone(),
        };
        match &feature.operation {
            Operation::Profile { .. } => {
                if let Some(sketch) = crate::fabrication_ui::source_sketch(w, &source) {
                    for (index, member) in sketch.members.keys().enumerate() {
                        result.push((
                            SeamSource::SketchMember {
                                profile: source.clone(),
                                member: member.clone(),
                                path: vec![],
                            },
                            format!("{} / Member {}", feature.name, index + 1),
                        ));
                    }
                }
            }
            Operation::ReferenceCurve { .. } => result.push((
                SeamSource::ReferenceCurve {
                    feature: source,
                    piece: None,
                    path: vec![],
                },
                feature.name.clone(),
            )),
            _ => {}
        }
    }
    result
}
pub(crate) fn support_choices(
    w: &Workspace,
    owner: &str,
    current: &str,
) -> Vec<(FaceSource, String)> {
    let mut result = vec![];
    if let Some(mesh) =
        viewport::display_evaluation(w).and_then(|evaluation| evaluation.meshes.get(owner))
    {
        let Some(part) = w.session.visible().parts.get(owner) else {
            return result;
        };
        let prior = part
            .features
            .iter()
            .take_while(|feature| feature.id != current)
            .collect::<Vec<_>>();
        for (index, face) in mesh.faces.iter().enumerate() {
            let Some(body) = &face.body else {
                continue;
            };
            if body.definition != owner || !prior.iter().any(|feature| feature.id == body.node) {
                continue;
            }
            let source = FaceSource {
                body: body.clone(),
                reference: Box::new(face.reference.clone()),
                path: vec![],
            };
            if !result.iter().any(|(old, _)| *old == source) {
                result.push((source, format!("Face {}", index + 1)));
            }
        }
    }
    result
}
pub(crate) fn preset(w: &mut Workspace) {
    crate::history::latest(w);
    if w.tool_checkpoint.is_some() || w.sketch.is_some() || w.confirm_requested {
        w.status = "Confirm or cancel the current operation before adding weld metal".into();
        return;
    }
    let Some(mut part) = w
        .view
        .active_part
        .as_ref()
        .and_then(|owner| w.session.visible().parts.get(owner))
        .cloned()
    else {
        w.status =
            "Select the intended part; assembly welds use the assembly's own Weld Bead command"
                .into();
        return;
    };
    let Some(seam) = selected_seam(w, true).filter(|seam| seam.dependency().definition == part.id)
    else {
        w.status =
            "Select a joint edge, sketch member, or reusable seam curve in the intended part"
                .into();
        return;
    };
    let id = cad_core::id();
    let name = format!(
        "Weld Bead{}",
        part.features
            .iter()
            .filter(|feature| matches!(feature.operation, Operation::WeldBead { .. }))
            .count()
            + 1
    );
    let mut definition = Definition::new(id.clone(), name.clone(), seam);
    let supports = selected_supports(w, true)
        .into_iter()
        .filter(|face| face.body.definition == part.id)
        .collect::<Vec<_>>();
    if supports.len() == 2 {
        definition.supports = supports;
    }
    let feature = Feature {
        id,
        name,
        operation: Operation::WeldBead {
            definition: Box::new(definition),
        },
        suppressed: false,
    };
    part.outputs.push(feature.id.clone());
    part.features.push(feature.clone());
    w.start_tool();
    w.dispatch(Command::PutPart {
        part,
        assembly: None,
        placement: Default::default(),
    });
    w.view.active_node = Some(feature.id.clone());
    ui::feature_dialog(w, Some(feature));
}
