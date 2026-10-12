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
            "Butt beads span one planar face, or two same-facing coplanar faces meeting at a butt/miter seam. Pick both joint faces for a tube corner. The complete root footprint must remain on the selected stock; a free outer edge or a gap is not a supported joint."
        }
    }
}

const PICK: &str = "weld-joint-pick";
const ACTION: &str = "weld.joint-action";

fn authoring(w: &Workspace) -> Option<(NodeRef, Definition, String, bool)> {
    if w.tool_checkpoint.is_none() || w.settings.is_some() || w.confirm_requested {
        return None;
    }
    if let Some((owner, definition, generation)) = crate::assembly_welds_ui::joint_pick_source(w) {
        return Some((owner, definition, generation, false));
    }
    if w.dialog.as_deref() != Some("feature") || w.fields.contains_key("feature-source-invalid") {
        return None;
    }
    let owner = w.view.active_part.as_ref()?;
    let feature: Feature = serde_json::from_str(&w.json).ok()?;
    if w.fields.get("feature-editor-id") != Some(&feature.id)
        || !w
            .session
            .visible()
            .parts
            .get(owner)?
            .features
            .contains(&feature)
    {
        return None;
    }
    let Operation::WeldBead { definition } = feature.operation else {
        return None;
    };
    Some((
        NodeRef {
            definition: owner.clone(),
            node: feature.id,
        },
        *definition,
        w.fields.get("feature-editor-generation")?.clone(),
        true,
    ))
}

/// Selection intent is transient presentation state. Every source mutation
/// still enters the shared validated feature/assembly draft and one undo step.
pub(crate) fn pick_actions(w: &Workspace) -> Vec<SemanticElement> {
    let Some((owner, definition, generation, part)) = authoring(w) else {
        return vec![];
    };
    let prefix = if part {
        "operation-editor"
    } else {
        "assembly-weld-editor"
    };
    let current = w
        .fields
        .get(PICK)
        .and_then(|value| serde_json::from_str::<serde_json::Value>(value).ok());
    let selected = current
        .as_ref()
        .filter(|value| value["owner"] == json!(owner) && value["editor"] == generation)
        .and_then(|value| value["mode"].as_str());
    let preview = current_joint_previews(w)
        .into_iter()
        .find(|preview| preview.owner == owner);
    let mut out = vec![];
    for (mode, label, enabled) in [
        ("seam", "Pick seam on model", true),
        ("support-0", "Pick support face 1", true),
        (
            "support-1",
            "Pick support face 2",
            !definition.supports.is_empty() || preview.is_some_and(|p| !p.supports.is_empty()),
        ),
        ("stop", "Stop joint picking", selected.is_some()),
        (
            "keep",
            "Keep evaluated supports",
            definition.supports.is_empty()
                && preview.is_some_and(|p| p.complete && !p.supports.is_empty()),
        ),
        (
            "butt",
            "Use butt / miter bead",
            definition.profile == Profile::Fillet && preview.is_some_and(|p| !p.complete),
        ),
        (
            "trim",
            if definition.profile == Profile::Fillet {
                "Trim ends by current leg size"
            } else {
                "Trim ends by current half width"
            },
            preview.is_some_and(|preview| {
                preview
                    .guides
                    .iter()
                    .any(|guide| guide.valid == Some(false))
            }),
        ),
    ] {
        let enabled = enabled
            && (mode == "stop"
                || mode == "butt"
                || mode == "trim"
                || mode == "keep"
                || viewport::selection_evaluation(w).is_some());
        let id = format!("{prefix}:{}/action/joint-{mode}", owner.node);
        let data = json!({"contract":"tcad.presentation.weld-joint-picker","version":1,"element":id,"document":w.session.visible().id,"owner":owner,"editor":generation,"source":definition,"mode":mode,"group":if part {"sources"} else {"seam"},"classes":["modeling-action","weld-joint-picker"],"shared_draft":true,"requires_current_validation":true});
        let mut data = data;
        data["actions"] = json!([{"command":ACTION,"label":label,"enabled":enabled,"disabled_reason":(!enabled).then_some("Use current source geometry; pick the first face before the second"),"arguments":data.clone()}]);
        out.push(SemanticElement {
            id,
            role: if part {
                "operation-editor-action"
            } else {
                "assembly-weld-action"
            }
            .into(),
            label: label.into(),
            value: None,
            state: if !enabled {
                "disabled"
            } else if selected == Some(mode) {
                "selected"
            } else {
                "ready"
            }
            .into(),
            commands: vec![ACTION.into()],
            relationships: vec![owner.node.clone()],
            data,
        });
    }
    if let Some(preview) = preview {
        for (index, support) in preview.supports.iter().enumerate() {
            let id = format!("{prefix}:{}/action/joint-focus-{index}", owner.node);
            let label = format!("Highlight support face {}", index + 1);
            let enabled = viewport::selection_evaluation(w).is_some();
            let mut data = json!({"contract":"tcad.presentation.weld-joint-picker","version":1,"element":id,"document":w.session.visible().id,"owner":owner,"editor":generation,"source":definition,"mode":"focus","support":support,"joint_identity":joint_evaluation(w).map(|evaluation|&evaluation.identity),"path":preview.path,"group":if part {"sources"} else {"seam"},"classes":["modeling-action","weld-joint-picker"],"display_only":true});
            data["actions"] = json!([{"command":ACTION,"label":label,"enabled":enabled,"arguments":data.clone()}]);
            out.push(SemanticElement {
                id,
                role: if part {
                    "operation-editor-action"
                } else {
                    "assembly-weld-action"
                }
                .into(),
                label,
                value: None,
                state: if enabled { "ready" } else { "disabled" }.into(),
                commands: vec![ACTION.into()],
                relationships: vec![support.body.node.clone()],
                data,
            });
        }
    }
    out
}

pub(crate) fn action(w: &mut Workspace, command: &str, data: Option<&serde_json::Value>) -> bool {
    if command != ACTION {
        return false;
    }
    let result = (|| -> cad_core::Result<()> {
        let data = data.ok_or_else(|| {
            cad_core::CadError::Invalid("Choose a current weld joint action".into())
        })?;
        let fresh = pick_actions(w)
            .into_iter()
            .find(|element| data["element"] == element.id)
            .ok_or_else(|| {
                cad_core::CadError::Invalid("The weld editor or joint action changed".into())
            })?;
        for key in [
            "document",
            "owner",
            "editor",
            "source",
            "mode",
            "support",
            "joint_identity",
            "path",
        ] {
            if fresh.data[key] != data[key] {
                return Err(cad_core::CadError::Invalid(
                    "Use the current weld joint selection action".into(),
                ));
            }
        }
        if !fresh
            .action_descriptors()
            .iter()
            .any(|action| action.enabled)
        {
            return Err(cad_core::CadError::Invalid(
                "This weld joint action is unavailable".into(),
            ));
        }
        let (owner, mut definition, generation, part) = authoring(w)
            .ok_or_else(|| cad_core::CadError::Invalid("The weld editor closed".into()))?;
        match data["mode"].as_str() {
            Some("trim") => {
                let offset = if definition.profile == Profile::Fillet {
                    definition.size.clone()
                } else {
                    cad_core::Expr(format!("({}) / 2", definition.size.0))
                };
                definition.start_offset = offset.clone();
                definition.end_offset = offset;
                submit_joint(w, definition, part)?;
                w.status="Both end trims updated from the current bead size; native contact is being reevaluated".into();
            }
            Some("focus") => {
                let support: FaceSource = serde_json::from_value(data["support"].clone())?;
                let mut path: Vec<String> = serde_json::from_value(data["path"].clone())?;
                path.extend(support.path.iter().cloned());
                let selection=viewport::selection_evaluation(w).and_then(|evaluation| {
                    for instance in viewport::display_instances(w,evaluation).into_iter().filter(|instance|instance.path==path) {
                        for mesh in viewport::selection_meshes(w,evaluation,instance) {
                            if let Some(face)=mesh.faces.iter().find(|face|face.body.as_ref()==Some(&support.body) && cad_core::selection_sets::same_reference(&face.reference,&support.reference)) {
                                let local=face.reference.signature.as_ref().map(|signature|signature.center).or_else(||mesh.indices.get(face.start).and_then(|index|mesh.positions.get(*index as usize)).copied())?;
                                return Some(viewport::Selection{instance:instance.path.last().cloned().unwrap_or_else(||instance.definition.clone()),path:path.clone(),name:instance.name.clone(),definition:instance.definition.clone(),reference:face.reference.clone(),body:face.body.clone(),distance:0.,point:viewport::placed(local,instance)});
                            }
                        }
                    }
                    None
                }).ok_or_else(||cad_core::CadError::Invalid("This support is no longer in the current selectable source; evaluate or pick its replacement".into()))?;
                if !viewport::select(w, selection) {
                    return Err(cad_core::CadError::Invalid(w.status.clone()));
                }
                w.fields.remove(PICK);
                w.status="Support face highlighted; seam and toe guides show the evaluated contact extent".into();
            }
            Some("stop") => {
                w.fields.remove(PICK);
                w.status = "Joint picking stopped; the current weld draft is unchanged".into();
            }
            Some("keep") => {
                definition.supports = current_joint_previews(w)
                    .into_iter()
                    .find(|preview| preview.owner == owner && preview.complete)
                    .ok_or_else(|| {
                        cad_core::CadError::Invalid(
                            "Current native joint contact has not passed validation".into(),
                        )
                    })?
                    .supports
                    .clone();
                submit_joint(w, definition, part)?;
            }
            Some("butt") => {
                definition.profile = Profile::Butt;
                definition.supports.clear();
                submit_joint(w, definition, part)?;
                w.status =
                    "Butt / miter profile selected; native contact is being evaluated".into();
            }
            Some(mode @ ("seam" | "support-0" | "support-1")) => {
                w.fields.insert(
                    PICK.into(),
                    json!({"owner":owner,"editor":generation,"mode":mode}).to_string(),
                );
                w.status=if mode=="seam" {"Click a native joint edge; Tab cycles overlapping geometry"} else {"Click the intended support face; Tab cycles overlapping faces. Its root and complete footprint must contact the seam"}.into();
            }
            _ => {
                return Err(cad_core::CadError::Invalid(
                    "Unknown weld joint action".into(),
                ));
            }
        }
        Ok(())
    })();
    if let Err(error) = result {
        w.status = error.to_string();
    }
    true
}

fn submit_joint(w: &mut Workspace, definition: Definition, part: bool) -> cad_core::Result<()> {
    definition.validate()?;
    if !part {
        return crate::assembly_welds_ui::submit_joint_pick(w, definition);
    }
    let mut feature: Feature = serde_json::from_str(&w.json)?;
    feature.operation = Operation::WeldBead {
        definition: Box::new(definition),
    };
    ui::preview_feature(w, feature.clone());
    if w.fields.contains_key("feature-source-invalid") {
        return Err(cad_core::CadError::Invalid(w.status.clone()));
    }
    w.json = serde_json::to_string_pretty(&feature)?;
    Ok(())
}

fn upstream_part_source(w: &Workspace, owner: &NodeRef, source: &NodeRef) -> bool {
    source.definition == owner.definition
        && w.session
            .visible()
            .parts
            .get(&owner.definition)
            .is_some_and(|part| {
                part.features
                    .iter()
                    .take_while(|feature| feature.id != owner.node)
                    .any(|feature| feature.id == source.node && !feature.suppressed)
            })
}

pub(crate) fn capture_selection(w: &mut Workspace) -> bool {
    let Some(pick) = w
        .fields
        .get(PICK)
        .and_then(|value| serde_json::from_str::<serde_json::Value>(value).ok())
    else {
        return false;
    };
    let Some((owner, mut definition, generation, part)) = authoring(w) else {
        w.fields.remove(PICK);
        return false;
    };
    if pick["owner"] != json!(owner) || pick["editor"] != generation {
        w.fields.remove(PICK);
        return false;
    }
    let Some(mode) = pick["mode"].as_str() else {
        return false;
    };
    let result = (|| -> cad_core::Result<()> {
        if mode == "seam" {
            let source = if part {
                selected_seam(w, true)
            } else {
                crate::assembly_welds_ui::picked_joint_seam(w, &owner.definition)
            }
            .ok_or_else(|| {
                cad_core::CadError::Invalid("Pick a native edge or authored seam source".into())
            })?;
            if part && !upstream_part_source(w, &owner, source.dependency()) {
                return Err(cad_core::CadError::Invalid(
                    "Pick upstream seam geometry in this part".into(),
                ));
            }
            definition.seam = source;
            definition.supports.clear();
        } else {
            let index = if mode == "support-0" { 0 } else { 1 };
            let selected = w
                .selected
                .as_ref()
                .filter(|selected| selected.reference.topology == "face")
                .ok_or_else(|| {
                    cad_core::CadError::Invalid(
                        "Pick a support face; Tab cycles the nearby candidates".into(),
                    )
                })?;
            let source = if part {
                Some(FaceSource {
                    body: crate::reference_ui::selected_body(w, selected),
                    reference: Box::new(selected.reference.clone()),
                    path: vec![],
                })
            } else {
                crate::assembly_welds_ui::picked_joint_support(w, &owner.definition)
            }
            .ok_or_else(|| {
                cad_core::CadError::Invalid("Pick a face within the owning assembly".into())
            })?;
            if part && !upstream_part_source(w, &owner, &source.body) {
                return Err(cad_core::CadError::Invalid(
                    "Pick upstream support geometry in this part".into(),
                ));
            }
            if definition.supports.is_empty()
                && let Some(preview) = current_joint_previews(w)
                    .into_iter()
                    .find(|preview| preview.owner == owner)
            {
                definition.supports = preview.supports.clone();
            }
            if index > definition.supports.len() {
                return Err(cad_core::CadError::Invalid(
                    "Pick support face 1 before support face 2".into(),
                ));
            }
            if definition.supports.iter().enumerate().any(|(old, face)| {
                old != index
                    && face.body == source.body
                    && face.path == source.path
                    && cad_core::selection_sets::same_reference(&face.reference, &source.reference)
            }) {
                return Err(cad_core::CadError::Invalid(
                    "The two support slots must refer to distinct joint faces".into(),
                ));
            }
            if index == definition.supports.len() {
                definition.supports.push(source);
            } else {
                definition.supports[index] = source;
            }
        }
        let next = definition.profile == Profile::Fillet && definition.supports.len() == 1;
        submit_joint(w, definition, part)?;
        if next {
            w.fields.insert(
                PICK.into(),
                json!({"owner":owner,"editor":generation,"mode":"support-1"}).to_string(),
            );
            w.status = "First support set; pick support face 2 to complete the fillet joint".into();
        } else {
            w.fields.remove(PICK);
            w.status =
                "Weld joint source updated; inspecting current native contact and bead preview"
                    .into();
        }
        Ok(())
    })();
    if let Err(error) = result {
        w.status = error.to_string();
    }
    true
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
                result.push((source, face_caption(w, face, index)));
            }
        }
    }
    result
}
pub(crate) fn face_caption(
    w: &Workspace,
    face: &cad_core::protocol::FaceRange,
    index: usize,
) -> String {
    let part = w
        .session
        .visible()
        .parts
        .get(&face.reference.producer.definition);
    let name = part
        .and_then(|part| {
            part.features
                .iter()
                .find(|feature| feature.id == face.reference.producer.node)
        })
        .map(|feature| feature.name.as_str())
        .unwrap_or("Body");
    let geometry = face
        .plane
        .as_ref()
        .map(|plane| {
            let u = plane.u;
            let v = plane.v;
            let n = [
                u[1] * v[2] - u[2] * v[1],
                u[2] * v[0] - u[0] * v[2],
                u[0] * v[1] - u[1] * v[0],
            ];
            let axis = (0..3)
                .max_by(|a, b| n[*a].abs().total_cmp(&n[*b].abs()))
                .unwrap_or(0);
            if n[axis].abs() > 1. - 1e-8 {
                format!(
                    "plane {}{} · {}",
                    if n[axis] < 0. { "−" } else { "+" },
                    ["X", "Y", "Z"][axis],
                    w.session
                        .visible()
                        .settings
                        .format_length(plane.origin[axis])
                )
            } else {
                "planar face".into()
            }
        })
        .unwrap_or_else(|| "curved face".into());
    format!("{name} · face {} · {geometry}", index + 1)
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
