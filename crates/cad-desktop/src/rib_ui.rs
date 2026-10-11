//! Docked Rib authoring through the same source, validation and checkpoint path.
use crate::{Workspace, cad_ui, ui};
use bevy_egui::egui;
use cad_core::{
    Document, Feature, Id, NodeRef, Operation, Profile, Sketch,
    commands::Command,
    rib::{self, Definition, Direction, Placement, Termination},
};

const DEFERRED: &str = "tcad:rib-preset:";
#[derive(serde::Serialize, serde::Deserialize)]
struct Pending {
    document: Id,
    definition: Id,
    profile: Option<NodeRef>,
    members: Vec<Id>,
    body: Option<NodeRef>,
}
pub fn is_preset(name: &str) -> bool {
    name == "Rib" || name.starts_with(DEFERRED)
}
fn selected_contours(w: &Workspace) -> Option<(NodeRef, Vec<Id>)> {
    let sketch = w.sketch.as_ref()?;
    Some((
        NodeRef {
            definition: sketch.definition.clone(),
            node: sketch.feature.clone(),
        },
        sketch
            .selection
            .iter()
            .filter_map(|entity| {
                if let cad_core::sketch_actions::SketchEntity::Member { member } = entity {
                    Some(member.clone())
                } else {
                    None
                }
            })
            .collect(),
    ))
}
fn encode(w: &mut Workspace, pending: &Pending) -> bool {
    match serde_json::to_string(pending) {
        Ok(payload) => {
            w.after_confirm = Some(format!("{DEFERRED}{payload}"));
            true
        }
        Err(error) => {
            w.status = error.to_string();
            false
        }
    }
}
pub fn remember_contours(w: &mut Workspace) {
    let Some(payload) = w
        .after_confirm
        .as_deref()
        .and_then(|s| s.strip_prefix(DEFERRED))
    else {
        return;
    };
    let Ok(mut pending) = serde_json::from_str::<Pending>(payload) else {
        return;
    };
    let Some((profile, members)) = selected_contours(w) else {
        return;
    };
    if pending.document != w.session.visible().id || pending.definition != profile.definition {
        w.after_confirm = None;
        return;
    }
    pending.profile = Some(profile);
    pending.members = members;
    encode(w, &pending);
}
fn sketch<'a>(doc: &'a Document, profile: &NodeRef) -> Option<&'a Sketch> {
    let part = doc.parts.get(&profile.definition)?;
    let Operation::Profile {
        profile: Profile::Sketch { sketch },
        ..
    } = &part
        .features
        .iter()
        .find(|f| f.id == profile.node)?
        .operation
    else {
        return None;
    };
    part.sketches.get(sketch)
}
fn choices(doc: &Document, owner: &str, current: &str, profiles: bool) -> Vec<(NodeRef, String)> {
    doc.parts
        .get(owner)
        .into_iter()
        .flat_map(|part| {
            part.features
                .iter()
                .take_while(move |f| f.id != current)
                .filter(move |f| {
                    !cad_core::configuration::feature_suppressed(doc, &part.id, f)
                        && if profiles {
                            matches!(
                                f.operation,
                                Operation::Profile {
                                    profile: Profile::Sketch { .. },
                                    ..
                                }
                            )
                        } else {
                            f.operation.produces_solid()
                        }
                })
                .map(move |feature| (part.reference(&feature.id), feature.name.clone()))
        })
        .collect()
}
fn selector(
    ui: &mut egui::Ui,
    label: &str,
    reference: &mut NodeRef,
    choices: &[(NodeRef, String)],
) -> bool {
    let mut changed = false;
    egui::ComboBox::from_label(label)
        .selected_text(
            choices
                .iter()
                .find(|(r, _)| r == reference)
                .map(|(_, name)| name.as_str())
                .unwrap_or("Select source"),
        )
        .show_ui(ui, |ui| {
            for (value, label) in choices {
                changed |= ui
                    .selectable_value(reference, value.clone(), label)
                    .changed();
            }
        });
    changed
}
pub fn fields(
    ui: &mut egui::Ui,
    w: &Workspace,
    doc: &Document,
    current: &str,
    definition: &mut Definition,
) -> bool {
    let mut changed = false;
    let owner = definition.body.definition.clone();
    if selector(
        ui,
        "Rib Sketch",
        &mut definition.profile,
        &choices(doc, &owner, current, true),
    ) {
        definition.members.clear();
        changed = true;
    }
    changed |= selector(
        ui,
        "Supporting Body",
        &mut definition.body,
        &choices(doc, &owner, current, false),
    );
    egui::ComboBox::from_label("Material Direction")
        .selected_text(match definition.direction {
            Direction::ParallelToSketch => "Parallel to Sketch",
            Direction::NormalToSketch => "Normal to Sketch",
        })
        .show_ui(ui, |ui| {
            changed |= ui
                .selectable_value(
                    &mut definition.direction,
                    Direction::ParallelToSketch,
                    "Parallel to Sketch",
                )
                .changed();
            changed |= ui
                .selectable_value(
                    &mut definition.direction,
                    Direction::NormalToSketch,
                    "Normal to Sketch",
                )
                .changed();
        });
    if definition.direction == Direction::ParallelToSketch {
        changed |= cad_ui::expression(ui, "Direction U", &mut definition.in_plane_direction[0]);
        changed |= cad_ui::expression(ui, "Direction V", &mut definition.in_plane_direction[1]);
        ui.small("Fill along the sketch U/V direction; thickness runs across the sketch plane.");
    } else {
        ui.small("Offset the open contour in its plane, then fill along the sketch normal.");
    }
    changed |= ui
        .checkbox(&mut definition.reverse, "Reverse Material Direction")
        .changed();
    changed |= cad_ui::expression(ui, "Thickness", &mut definition.thickness);
    egui::ComboBox::from_label("Thickness Placement")
        .selected_text(match definition.placement {
            Placement::FirstSide => "First Side",
            Placement::Centered => "Both Sides",
            Placement::SecondSide => "Second Side",
        })
        .show_ui(ui, |ui| {
            for (value, label) in [
                (Placement::FirstSide, "First Side"),
                (Placement::Centered, "Both Sides"),
                (Placement::SecondSide, "Second Side"),
            ] {
                changed |= ui
                    .selectable_value(&mut definition.placement, value, label)
                    .changed();
            }
        });
    egui::CollapsingHeader::new("Selected Contours").show(ui,|ui| {
        if ui.selectable_label(definition.members.is_empty(),"All Open Contours").clicked() && !definition.members.is_empty() {definition.members.clear();changed=true;}
        let contours=sketch(doc,&definition.profile).and_then(|source|rib::open_contours(source).ok()).unwrap_or_default();
        for (index,members) in contours.iter().enumerate() {
            let mut included=definition.members.is_empty() || definition.members.iter().any(|member|members.contains(member));
            if ui.checkbox(&mut included,format!("Open Contour {} · {} members",index+1,members.len())).changed() {
                let mut selection=if definition.members.is_empty() {contours.iter().filter_map(|group|group.first().cloned()).collect()}else{definition.members.clone()};
                if included {if !selection.iter().any(|m|members.contains(m)) {selection.push(members[0].clone());}}
                else {selection.retain(|m|!members.contains(m));}
                if selection.is_empty() {ui.small("Keep at least one open contour selected.");}
                else {definition.members=selection;changed=true;}
            }
        }
        ui.small("Each chosen member identifies its complete connected open contour. Closed contours are excluded.");
    });
    let explicit = matches!(definition.termination, Termination::UpToFace { .. });
    if ui.selectable_label(!explicit, "Up to Body").clicked() && explicit {
        definition.termination = Termination::UpToBody;
        changed = true;
    }
    if let Some(selected) = w
        .selected
        .as_ref()
        .filter(|s| s.reference.topology == "face" && s.definition == owner)
        && crate::sketch::selected_plane(w, &owner).is_some()
        && let Some(body) = selected.body.as_ref()
        && definition.consumed_outputs().contains(body)
        && ui.button("Up to Selected Face").clicked()
    {
        definition.termination = Termination::UpToFace {
            face: Box::new(selected.reference.clone()),
            body: body.clone(),
        };
        changed = true;
    }
    if matches!(definition.termination, Termination::UpToFace { .. }) {
        ui.small("Termination: Selected Face");
    }
    changed |= cad_ui::expression(ui, "Attachment Overlap", &mut definition.attachment_overlap);
    ui.small("The full overlap must remain inside the support. Changes preview before OK.");
    egui::CollapsingHeader::new("Feature Scope").show(ui, |ui| {
        for (reference, name) in choices(doc, &owner, current, false) {
            if reference == definition.body {
                continue;
            }
            let mut included = definition.scope.contains(&reference);
            if ui.checkbox(&mut included, name).changed() {
                if included {
                    definition.scope.push(reference);
                } else {
                    definition.scope.retain(|r| r != &reference);
                }
                changed = true;
            }
        }
    });
    changed
}
pub fn preset(w: &mut Workspace, name: &str) {
    if !is_preset(name) {
        return;
    }
    if w.fields.contains_key("configuration-editor")
        || w.fields.contains_key("component-pattern-editor")
    {
        w.status = "Confirm or cancel the current configuration or component pattern first".into();
        return;
    }
    let pending = name
        .strip_prefix(DEFERRED)
        .map(serde_json::from_str::<Pending>)
        .transpose();
    let mut pending = match pending {
        Ok(Some(pending)) => {
            if pending.document != w.session.visible().id
                || w.view.active_part.as_deref() != Some(pending.definition.as_str())
            {
                w.status =
                    "The pending Rib action belongs to another document or part; invoke Rib again"
                        .into();
                return;
            }
            pending
        }
        Ok(None) => {
            let selected = selected_contours(w);
            Pending {
                document: w.session.visible().id.clone(),
                definition: w
                    .sketch
                    .as_ref()
                    .map(|s| s.definition.clone())
                    .or_else(|| w.view.active_part.clone())
                    .unwrap_or_default(),
                profile: selected.as_ref().map(|(source, _)| source.clone()),
                members: selected.map(|(_, members)| members).unwrap_or_default(),
                body: w
                    .selected
                    .as_ref()
                    .and_then(|s| s.body.clone())
                    .or_else(|| w.sketch.as_ref().and_then(|s| s.reference_body.clone())),
            }
        }
        Err(error) => {
            w.status = format!("Pending Rib action is invalid: {error}");
            return;
        }
    };
    if w.session
        .visible()
        .libraries
        .values()
        .any(|link| link.pinned_definitions.contains_key(&pending.definition))
    {
        w.status = "Make this linked component unique before adding a Rib".into();
        return;
    }
    if w.fields.contains_key("mate-editor") {
        w.status = "Confirm or cancel the current mate first".into();
        return;
    }
    crate::history::latest(w);
    if w.confirm_requested {
        encode(w, &pending);
        return;
    }
    if w.sketch.is_some() {
        if encode(w, &pending) {
            w.confirm_tool();
            if w.sketch.is_some() && !w.confirm_requested {
                w.after_confirm = None;
            }
        }
        return;
    }
    if w.tool_checkpoint.is_some() {
        w.status = "Confirm or cancel the current feature first".into();
        return;
    }
    let Some(mut part) = w.session.visible().parts.get(&pending.definition).cloned() else {
        w.status = "Create or select the intended Rib part first".into();
        return;
    };
    let profiles = choices(w.session.visible(), &part.id, "", true);
    let body = pending
        .body
        .take()
        .filter(|body| {
            choices(w.session.visible(), &part.id, "", false)
                .iter()
                .any(|(reference, _)| reference == body)
        })
        .or_else(|| {
            part.features
                .iter()
                .rev()
                .find(|f| {
                    !cad_core::configuration::feature_suppressed(w.session.visible(), &part.id, f)
                        && part.outputs.contains(&f.id)
                        && f.operation.produces_solid()
                })
                .map(|feature| part.reference(&feature.id))
        });
    let Some(body) = body else {
        w.status = "Create supporting solid stock from a sketch before adding a Rib".into();
        return;
    };
    if pending
        .profile
        .as_ref()
        .is_some_and(|profile| !profiles.iter().any(|(source, _)| source == profile))
    {
        w.status = "The pending Rib sketch is missing or suppressed; select its replacement".into();
        return;
    }
    let profile = pending
        .profile
        .clone()
        .or_else(|| {
            profiles
                .iter()
                .find(|(source, _)| {
                    w.view.active_node.as_ref() == Some(&source.node)
                        && sketch(w.session.visible(), source).is_some_and(|sketch| {
                            rib::open_contours(sketch).is_ok_and(|groups| !groups.is_empty())
                        })
                })
                .map(|(source, _)| source.clone())
        })
        .or_else(|| {
            profiles
                .iter()
                .rev()
                .find(|(source, _)| {
                    sketch(w.session.visible(), source).is_some_and(|sketch| {
                        rib::open_contours(sketch).is_ok_and(|groups| !groups.is_empty())
                    })
                })
                .map(|(source, _)| source.clone())
        });
    let Some(profile) = profile else {
        crate::sketch::begin(w, None);
        if w.sketch.is_some() {
            pending.body = Some(body);
            pending.profile = None;
            pending.members.clear();
            encode(w, &pending);
            w.status="Draw and constrain an open Rib contour, then Finish Sketch to open the Rib manager".into();
        }
        return;
    };
    let feature = Feature {
        id: cad_core::id(),
        name: format!(
            "Rib{}",
            part.features
                .iter()
                .filter(|feature| matches!(feature.operation, Operation::Rib { .. }))
                .count()
                + 1
        ),
        operation: Operation::Rib {
            definition: Box::new(Definition {
                profile,
                members: pending.members,
                body,
                scope: vec![],
                direction: Direction::ParallelToSketch,
                thickness: "2 mm".into(),
                placement: Placement::Centered,
                reverse: false,
                termination: Termination::UpToBody,
                in_plane_direction: rib::default_in_plane_direction(),
                attachment_overlap: rib::default_overlap(),
            }),
        },
        suppressed: false,
    };
    for reference in feature.operation.consumed_outputs() {
        part.outputs.retain(|output| output != &reference.node);
    }
    part.outputs.push(feature.id.clone());
    part.features.push(feature.clone());
    let definition = part.id.clone();
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
        .get(&definition)
        .is_some_and(|part| part.features.iter().any(|current| current == &feature))
    {
        let error = w.status.clone();
        w.cancel_tool();
        w.status = error;
        return;
    }
    w.view.active_node = Some(feature.id.clone());
    ui::feature_dialog(w, Some(feature));
}
