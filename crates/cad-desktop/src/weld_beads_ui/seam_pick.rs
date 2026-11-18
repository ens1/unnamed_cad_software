//! Select the first real seam before creating a part feature or an assembly record.
use super::{
    SeamSource, SemanticElement, Workspace, authored_seams, create_part, edge_seam, viewport,
};
use crate::{assembly_welds_ui, presentation_controls as controls};
use bevy_egui::egui;
use serde_json::{Value, json};
pub(crate) const PANEL: &str = "weld-seam";
pub(crate) const PROMPT: &str = "Pick a joint edge or sketch member.";
const ACTION: &str = "weld.choose-seam";

pub(crate) fn owner(w: &Workspace) -> Option<&str> {
    (w.dialog.as_deref() == Some(PANEL) && w.tool_checkpoint.is_some())
        .then(|| w.fields.get("weld-start-owner").map(String::as_str))
        .flatten()
}
pub(crate) fn open(w: &mut Workspace, owner: &str) {
    if w.tool_checkpoint.is_some()
        || w.session.draft.is_some()
        || w.sketch.is_some()
        || w.assembly_motion.is_some()
        || w.confirm_requested
    {
        w.status = "Finish the current operation before starting a weld bead.".into();
        return;
    }
    w.start_tool();
    if w.tool_checkpoint.is_none() {
        return;
    }
    w.fields.insert("weld-start-owner".into(), owner.into());
    w.fields
        .insert("weld-start-generation".into(), cad_core::id());
    w.open_editor(PANEL);
    w.status = PROMPT.into();
}
fn scope(w: &Workspace, owner: &str, part: &str, path: &[String]) -> Option<Vec<String>> {
    if w.session.visible().parts.contains_key(owner) {
        (owner == part).then(Vec::new)
    } else {
        assembly_welds_ui::relative_path(w, owner, path)
    }
}
fn choose(w: &mut Workspace, owner: &str, seam: SeamSource) {
    if w.session.visible().parts.contains_key(owner) {
        create_part(w, owner, seam);
    } else {
        assembly_welds_ui::create(w, owner, seam);
    }
}
fn replacement(w: &Workspace) -> Option<(cad_core::NodeRef, super::Definition, bool)> {
    let pick: Value = serde_json::from_str(w.fields.get(super::PICK)?).ok()?;
    if pick["mode"] != "seam" {
        return None;
    }
    let (node, definition, generation, part) = super::authoring(w)?;
    (pick["owner"] == json!(node) && pick["editor"] == generation)
        .then_some((node, definition, part))
}
pub(crate) fn active(w: &Workspace) -> bool {
    owner(w).is_some() || replacement(w).is_some()
}
pub(crate) fn capture_click(w: &mut Workspace, rect: egui::Rect, point: egui::Pos2) -> bool {
    let replacing = replacement(w);
    let Some(owner) = owner(w).map(str::to_owned).or_else(|| {
        replacing
            .as_ref()
            .map(|(node, _, _)| node.definition.clone())
    }) else {
        return false;
    };
    let picked = viewport::pick(w, rect, point)
        .into_iter()
        .filter(|hit| hit.reference.topology == "edge")
        .find_map(|hit| edge_seam(w, &hit, scope(w, &owner, &hit.definition, &hit.path)?));
    let seam = picked.or_else(|| {
        let (hit, member) = crate::sketch::pick_with_member(w, rect, point)?;
        let path = scope(w, &owner, &hit.definition, &hit.path)?;
        Some(SeamSource::SketchMember {
            profile: cad_core::NodeRef {
                definition: hit.definition,
                node: hit.feature,
            },
            member: member?,
            path,
        })
    });
    let Some(seam) = seam else {
        w.status = PROMPT.into();
        return true;
    };
    if let Some((node, mut definition, part)) = replacing {
        if part && !super::upstream_part_source(w, &node, seam.dependency()) {
            w.status = "Pick geometry made before this weld in the owning part.".into();
            return true;
        }
        definition.seam = seam;
        definition.supports.clear();
        match super::submit_joint(w, definition, part) {
            Ok(()) => {
                w.fields.remove(super::PICK);
                w.status = "Seam updated.".into();
            }
            Err(error) => w.status = error.to_string(),
        }
    } else {
        choose(w, &owner, seam);
    }
    true
}
pub(super) fn elements(w: &Workspace) -> Vec<SemanticElement> {
    let Some(owner) = owner(w) else {
        return vec![];
    };
    let doc = w.session.visible();
    let mut result = vec![];
    let mut add = |part: &cad_core::Part, path: &[String], prefix: &str| {
        for (source, label) in authored_seams(
            w,
            &part.id,
            part.features.iter().filter(|f| !f.suppressed),
            path,
        ) {
            let id = format!("weld-start:{}", json!(source));
            result.push(SemanticElement {
                id, role:"weld-seam-choice".into(), label:format!("{prefix}{label}"), value:None, state:"ready".into(),
                commands:vec![ACTION.into()], relationships:vec![part.id.clone(), source.dependency().node.clone()],
                data:json!({"document":doc.id, "owner":owner, "generation":w.fields.get("weld-start-generation"), "source":source}),
            });
        }
    };
    if let Some(part) = doc.parts.get(owner) {
        add(part, &[], "");
    } else if let Some(evaluation) = viewport::selection_evaluation(w) {
        let mut used = std::collections::BTreeSet::new();
        for instance in &evaluation.instances {
            if let Some(part) = doc.parts.get(&instance.definition)
                && let Some(path) = scope(w, owner, &part.id, &instance.path)
                && used.insert(path.clone())
            {
                add(part, &path, &format!("{} / ", instance.name));
            }
        }
    }
    result
}
pub(super) fn action(w: &mut Workspace, command: &str, data: Option<&Value>) -> bool {
    if command != ACTION {
        return false;
    }
    if let Some(choice) = elements(w).into_iter().find(|e| Some(&e.data) == data)
        && let (Some(owner), Ok(source)) = (
            owner(w).map(str::to_owned),
            serde_json::from_value(choice.data["source"].clone()),
        )
    {
        choose(w, &owner, source);
    } else {
        w.status = "The seam changed. Select it again.".into();
    }
    true
}
pub(crate) fn show(ui: &mut egui::Ui, w: &mut Workspace) {
    controls::menu_button(
        ui,
        "weld-start:curves",
        "Choose a sketch or curve…",
        |ui| {
            let entries = elements(w);
            if entries.is_empty() {
                ui.weak("No sketch members or reference curves available.");
            }
            egui::ScrollArea::vertical()
                .max_height(300.)
                .show(ui, |ui| {
                    for item in entries {
                        if controls::menu_item(ui, &item.id, &item.label, None, true, false)
                            .clicked()
                        {
                            action(w, ACTION, Some(&item.data));
                            ui.close();
                        }
                    }
                });
        },
    );
}
