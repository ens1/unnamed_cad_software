//! Part weld authoring. Every saved seam is source geometry, never a display path.
use crate::{Workspace, ui, viewport};
use cad_core::{
    Feature, NodeRef, Operation,
    commands::Command,
    weld_beads::{Definition, FaceSource, SeamSource},
};
use cad_core::{
    presentation::SemanticElement,
    protocol::Evaluation,
    weld_beads::{JointPreview, Profile},
};
use serde_json::json;

/// Invalid current-source evaluations carry diagnostic curves; the ordinary
/// model may still display its last valid station. Never read that fallback for
/// joint feedback, or a new selection/edit could inherit an obsolete verdict.
pub(crate) fn joint_evaluation(w: &Workspace) -> Option<&Evaluation> {
    let evaluation = if let Some(draft) = &w.session.draft {
        draft.evaluation.as_ref()
    } else {
        w.session.evaluation.as_ref()
    }?;
    (evaluation.identity == w.session.identity()).then_some(evaluation)
}

pub(crate) fn current_joint_previews(w: &Workspace) -> Vec<&JointPreview> {
    let target = if w.dialog.as_deref() == Some("feature") {
        if w.fields.contains_key("feature-source-invalid") {
            return vec![];
        }
        let Some(owner) = &w.view.active_part else {
            return vec![];
        };
        let Some(node) = w.fields.get("feature-editor-id") else {
            return vec![];
        };
        Some(NodeRef {
            definition: owner.clone(),
            node: node.clone(),
        })
    } else {
        crate::assembly_welds_ui::feedback_target(w)
    };
    let Some(target) = target else {
        return vec![];
    };
    let Some(evaluation) = joint_evaluation(w) else {
        return vec![];
    };
    let previews = evaluation
        .weld_joints
        .iter()
        .filter(|preview| preview.owner == target)
        .collect::<Vec<_>>();
    if w.session.visible().parts.contains_key(&target.definition) {
        let path = w.edge_selection_path.as_deref().unwrap_or_default();
        if previews.iter().any(|preview| preview.path == path) {
            return previews
                .into_iter()
                .filter(|preview| preview.path == path)
                .collect();
        }
        return previews
            .into_iter()
            .filter(|preview| preview.path.is_empty())
            .collect();
    }
    previews
}

pub(crate) fn joint_help(definition: &Definition) -> &'static str {
    if !matches!(definition.seam, SeamSource::ModelEdge { .. }) && definition.supports.is_empty() {
        return "This authored seam has no support faces. Direction and roll place a separate native bead in the owner's frame; add support faces to validate its joint contacts.";
    }
    match definition.profile {
        Profile::Fillet => {
            "Fillet beads need two joint faces with room for both toes. Choose an inside joint seam; a free outer plate edge usually places a toe outside its face. Select two support faces to make the joint explicit."
        }
        Profile::Butt => {
            "Butt beads need a planar support with room on both sides of the seam. Select that face explicitly when a model edge has more than one possible root support."
        }
    }
}

pub(crate) fn feedback(w: &Workspace, owner: &str, node: &str) -> Vec<SemanticElement> {
    let Some(evaluation) = joint_evaluation(w) else {
        return vec![];
    };
    let Some(preview) = current_joint_previews(w)
        .into_iter()
        .find(|preview| preview.owner.definition == owner && preview.owner.node == node)
    else {
        return vec![];
    };
    let mut out = vec![];
    let identity = format!("weld-joint:{owner}/{node}");
    let mut push = |key: &str, label: String, state: &str, data: serde_json::Value| {
        out.push(SemanticElement {
            id: format!("{identity}/{key}"), role: "weld-joint-feedback".into(),
            label, value: None, state: state.into(), commands: vec![],
            relationships: vec![owner.into(), node.into()],
            data: json!({"contract":"tcad.weld-joint-preview","version":cad_core::weld_beads::JOINT_PREVIEW_VERSION,"identity":evaluation.identity,"owner":preview.owner,"path":preview.path,"group":"sources","classes":["modeling-status","weld-feedback"],"diagnostic":data,"display_only":true}),
        });
    };
    push(
        "status",
        preview.message.clone().unwrap_or_else(|| {
            if preview.complete {
                "Native bead and all support contacts validated".into()
            } else {
                "Joint contact checks incomplete; bead is not accepted".into()
            }
        }),
        if preview.complete { "ready" } else { "invalid" },
        json!({"complete":preview.complete}),
    );
    push(
        "frame",
        if !preview.supports.is_empty() {
            format!(
                "{} · {}",
                preview.frame_label,
                if preview.orientation.is_some() {
                    "orientation follows the joint faces"
                } else {
                    "joint orientation is unresolved"
                }
            )
        } else {
            format!(
                "{} · authored direction and roll orient the section",
                preview.frame_label
            )
        },
        "ready",
        json!({"matrix":preview.matrix,"orientation":preview.orientation}),
    );
    for (index, support) in preview.supports.iter().enumerate() {
        let part = w.session.visible().parts.get(&support.body.definition);
        let feature = part.and_then(|part| {
            part.features
                .iter()
                .find(|feature| feature.id == support.body.node)
        });
        let failed = preview
            .guides
            .iter()
            .find(|guide| guide.support == index && guide.valid == Some(false));
        let label = format!(
            "Support {} · {} / {}{}",
            index + 1,
            part.map(|part| part.name.as_str()).unwrap_or("Component"),
            feature
                .map(|feature| feature.name.as_str())
                .unwrap_or("Body"),
            failed
                .map(|guide| format!(" · {} failed contact", guide.label))
                .unwrap_or_default()
        );
        push(
            &format!("support/{index}"),
            label,
            if failed.is_some() {
                "invalid"
            } else if preview.complete {
                "ready"
            } else {
                "pending"
            },
            json!({"support":support,"index":index,"failed_guide":failed}),
        );
    }
    if preview
        .guides
        .iter()
        .any(|guide| guide.valid == Some(false))
    {
        push("remedy", "For an outside toe, choose the inside joint or replace the support. Reduce leg size for a narrow face; increase start/end trim only when the toe crosses an end boundary. Direction and roll do not change face-supported joints.".into(), "invalid", json!({"supports":preview.supports,"guides":preview.guides}));
    }
    out
}

pub(crate) fn semantics(w: &Workspace) -> Vec<SemanticElement> {
    let Some(evaluation) = joint_evaluation(w) else {
        return vec![];
    };
    let mut out = vec![];
    for preview in current_joint_previews(w) {
        let prefix = format!(
            "weld-joint:{}/{}/{}",
            preview.owner.definition,
            preview.owner.node,
            preview.path.join("/")
        );
        let metadata = json!({"contract":"tcad.weld-joint-preview","version":cad_core::weld_beads::JOINT_PREVIEW_VERSION,"identity":evaluation.identity,"owner":preview.owner,"path":preview.path,"matrix":preview.matrix,"frame_label":preview.frame_label,"display_only":true});
        for (index, guide) in preview.guides.iter().enumerate() {
            let mut data = metadata.clone();
            data["points"] = json!(guide.points);
            data["support"] = json!(preview.supports.get(guide.support));
            data["support_index"] = json!(guide.support);
            data["segment"] = json!(guide.segment);
            data["message"] = json!(guide.message);
            data["classes"] = json!([
                "viewport-guide",
                "weld-guide",
                if guide.label == "Seam root" {
                    "weld-root"
                } else {
                    "weld-toe"
                }
            ]);
            out.push(SemanticElement {
                id: format!("{prefix}/guide/{index}"),
                role: "weld-joint-guide".into(),
                label: format!(
                    "Support {} · {} · Stitch {}",
                    guide.support + 1,
                    guide.label,
                    guide.segment + 1
                ),
                value: None,
                state: match guide.valid {
                    Some(true) => "ready",
                    Some(false) => "invalid",
                    None => "pending",
                }
                .into(),
                commands: vec![],
                relationships: std::iter::once(preview.owner.node.clone())
                    .chain(
                        preview
                            .supports
                            .get(guide.support)
                            .map(|support| support.body.node.clone()),
                    )
                    .collect(),
                data,
            });
        }
        let mut data = metadata;
        data["points"] = json!(preview.seam_points);
        data["orientation"] = json!(preview.orientation);
        data["classes"] = json!(["viewport-guide", "weld-guide", "weld-seam"]);
        out.push(SemanticElement {
            id: format!("{prefix}/seam"),
            role: "weld-joint-guide".into(),
            label: "Weld seam and section orientation".into(),
            value: None,
            state: if preview.complete { "ready" } else { "invalid" }.into(),
            commands: vec![],
            relationships: vec![preview.owner.node.clone()],
            data,
        });
    }
    out
}

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
