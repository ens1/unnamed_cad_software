use crate::{Workspace, ui};
use bevy_egui::egui;
use cad_core::{
    Document, Feature, NodeRef, Operation,
    commands::Command,
    reference_geometry::{OriginPlane, PlaneSource},
    split::{Definition, Keep, MAX_SCOPE},
};

pub(crate) fn is_preset(name: &str) -> bool {
    matches!(name, "Split" | "Cut With Plane")
}
fn choices(doc: &Document, owner: &str, current: &str) -> Vec<(NodeRef, String)> {
    doc.parts
        .get(owner)
        .into_iter()
        .flat_map(|part| {
            part.features
                .iter()
                .take_while(move |feature| feature.id != current)
                .filter(move |feature| {
                    feature.operation.produces_solid()
                        && !cad_core::configuration::feature_suppressed(doc, owner, feature)
                })
                .map(move |feature| (part.reference(&feature.id), feature.name.clone()))
        })
        .collect()
}
pub(crate) fn preset(w: &mut Workspace, name: &str) {
    if w.tool_checkpoint.is_some() || w.sketch.is_some() {
        w.status = "Confirm or cancel the current operation before splitting bodies".into();
        return;
    }
    let Some(owner) = w.view.active_part.clone() else {
        w.status = "Open a part or use Edit in Place before splitting bodies".into();
        return;
    };
    let doc = w.session.visible();
    let Some(mut part) = doc.parts.get(&owner).cloned() else {
        return;
    };
    if doc
        .libraries
        .values()
        .any(|link| link.pinned_definitions.contains_key(&owner))
    {
        w.status = "Make the library component unique before splitting bodies".into();
        return;
    }
    let available = choices(doc, &owner, "");
    let selected = w
        .selected
        .as_ref()
        .filter(|selected| matches!(selected.reference.topology.as_str(), "face" | "plane"));
    if selected.is_some_and(|selected| selected.definition != owner) {
        w.status = "Use Edit in Place on the selected component before splitting its bodies".into();
        return;
    }
    let picked_body = selected.map(|selected| crate::reference_ui::selected_body(w, selected));
    let body = picked_body
        .as_ref()
        .filter(|body| available.iter().any(|(value, _)| value == *body))
        .cloned()
        .or_else(|| {
            w.view
                .active_node
                .as_ref()
                .and_then(|node| available.iter().find(|(body, _)| &body.node == node))
                .map(|(body, _)| body.clone())
        })
        .or_else(|| {
            part.outputs.iter().rev().find_map(|output| {
                available
                    .iter()
                    .find(|(body, _)| &body.node == output)
                    .map(|(body, _)| body.clone())
            })
        });
    let Some(body) = body else {
        w.status = "Create or select a solid output before splitting bodies".into();
        return;
    };
    let plane = selected
        .and_then(|selected| match selected.reference.topology.as_str() {
            "plane" => PlaneSource::from_datum(&selected.reference).ok(),
            "face" => {
                let mut reference = selected.reference.clone();
                reference.signature = None;
                Some(PlaneSource::Face {
                    reference,
                    body: crate::reference_ui::selected_body(w, selected),
                })
            }
            _ => None,
        })
        .unwrap_or(PlaneSource::Origin {
            plane: match w.active_plane.as_deref() {
                Some("Front Plane") => OriginPlane::Front,
                Some("Right Plane") => OriginPlane::Right,
                _ => OriginPlane::Top,
            },
        });
    let definition = Definition {
        scope: vec![body],
        plane: Box::new(plane),
        offset: "0 mm".into(),
        reverse: false,
        keep: if name == "Cut With Plane" {
            Keep::Negative
        } else {
            Keep::Both
        },
    };
    if let Err(error) = definition.validate() {
        w.status = error.to_string();
        return;
    }
    let feature = Feature {
        id: cad_core::id(),
        name: format!(
            "Split{}",
            part.features
                .iter()
                .filter(|feature| feature.name.starts_with("Split"))
                .count()
                + 1
        ),
        operation: Operation::Split {
            definition: Box::new(definition),
        },
        suppressed: false,
    };
    for source in feature.operation.consumed_outputs() {
        part.outputs.retain(|output| output != &source.node);
    }
    part.outputs.push(feature.id.clone());
    part.features.push(feature.clone());
    crate::history::latest(w);
    w.start_tool();
    w.dispatch(Command::PutPart {
        part,
        assembly: None,
        placement: Default::default(),
    });
    if !w.session.visible().parts.get(&owner).is_some_and(|part| {
        part.features
            .iter()
            .any(|candidate| candidate.id == feature.id)
    }) {
        let error = w.status.clone();
        w.cancel_tool();
        w.status = error;
        return;
    }
    w.view.active_node = Some(feature.id.clone());
    ui::feature_dialog(w, Some(feature));
}
fn source_label(doc: &Document, body: &NodeRef) -> String {
    doc.parts
        .get(&body.definition)
        .and_then(|part| part.features.iter().find(|feature| feature.id == body.node))
        .map(|feature| {
            if cad_core::configuration::feature_suppressed(doc, &body.definition, feature) {
                format!("{} (suppressed in configuration)", feature.name)
            } else {
                feature.name.clone()
            }
        })
        .unwrap_or_else(|| "Missing source".into())
}
fn plane_fields(
    ui: &mut egui::Ui,
    w: &Workspace,
    doc: &Document,
    owner: &str,
    current: &str,
    source: &mut PlaneSource,
) -> bool {
    let mut changed = false;
    let label = match source {
        PlaneSource::Origin { plane } => plane.label().into(),
        PlaneSource::Datum { feature } => source_label(doc, feature),
        PlaneSource::CoordinateSystem { feature, plane } => {
            format!("{} · {}", source_label(doc, feature), plane.label())
        }
        PlaneSource::Face { body, .. } => format!("{} · planar face", source_label(doc, body)),
    };
    let preceding = doc
        .parts
        .get(owner)
        .into_iter()
        .flat_map(|part| {
            part.features
                .iter()
                .take_while(|feature| feature.id != current)
        })
        .filter(|feature| !cad_core::configuration::feature_suppressed(doc, owner, feature))
        .collect::<Vec<_>>();
    egui::ComboBox::from_label("Splitting Plane")
        .selected_text(label)
        .show_ui(ui, |ui| {
            for plane in [OriginPlane::Front, OriginPlane::Top, OriginPlane::Right] {
                changed |= ui
                    .selectable_value(source, PlaneSource::Origin { plane }, plane.label())
                    .changed();
            }
            for feature in &preceding {
                let reference = NodeRef {
                    definition: owner.into(),
                    node: feature.id.clone(),
                };
                match feature.operation {
                    Operation::ReferencePlane { .. } => {
                        changed |= ui
                            .selectable_value(
                                source,
                                PlaneSource::Datum { feature: reference },
                                &feature.name,
                            )
                            .changed()
                    }
                    Operation::ReferenceCoordinateSystem { .. } => {
                        for plane in [OriginPlane::Front, OriginPlane::Top, OriginPlane::Right] {
                            changed |= ui
                                .selectable_value(
                                    source,
                                    PlaneSource::CoordinateSystem {
                                        feature: reference.clone(),
                                        plane,
                                    },
                                    format!("{} · {}", feature.name, plane.label()),
                                )
                                .changed();
                        }
                    }
                    _ => {}
                }
            }
        });
    let selected = w
        .selected
        .as_ref()
        .filter(|selected| selected.definition == owner);
    let picked = selected.and_then(|selected| match selected.reference.topology.as_str() {
        "face" => {
            let body = crate::reference_ui::selected_body(w, selected);
            if preceding.iter().any(|feature| {
                feature.id == body.node
                    && (feature.operation.produces_solid() || feature.operation.is_surface())
            }) {
                let mut reference = selected.reference.clone();
                reference.signature = None;
                Some(PlaneSource::Face { body, reference })
            } else {
                None
            }
        }
        "plane"
            if preceding
                .iter()
                .any(|feature| feature.id == selected.reference.producer.node) =>
        {
            PlaneSource::from_datum(&selected.reference).ok()
        }
        _ => None,
    });
    if ui
        .add_enabled(
            picked.is_some(),
            egui::Button::new("Use Selected Plane or Planar Face"),
        )
        .clicked()
        && let Some(plane) = picked
    {
        *source = plane;
        changed = true;
    }
    changed
}
pub(crate) fn fields(
    ui: &mut egui::Ui,
    w: &Workspace,
    doc: &Document,
    current: &str,
    definition: &mut Definition,
) -> bool {
    let mut changed = false;
    let owner = doc
        .parts
        .values()
        .find(|part| part.features.iter().any(|feature| feature.id == current))
        .map(|part| part.id.as_str())
        .or(w.view.active_part.as_deref())
        .unwrap_or_default();
    changed |= plane_fields(ui, w, doc, owner, current, &mut definition.plane);
    changed |= crate::cad_ui::expression(ui, "Plane Offset", &mut definition.offset);
    changed |= ui
        .checkbox(&mut definition.reverse, "Reverse Plane Normal")
        .changed();
    ui.small("Offset follows the source plane normal. Reverse changes the positive and negative side labels.");
    egui::ComboBox::from_label("Keep")
        .selected_text(definition.keep.label())
        .show_ui(ui, |ui| {
            for keep in [Keep::Both, Keep::Positive, Keep::Negative] {
                changed |= ui
                    .selectable_value(&mut definition.keep, keep, keep.label())
                    .changed();
            }
        });
    let available = choices(doc, owner, current);
    ui.separator();
    ui.label(format!("Scoped Solid Outputs: {}", definition.scope.len()));
    let mut add = None;
    egui::ComboBox::from_id_salt((current, "split-add-scope"))
        .selected_text("Add Solid Output")
        .show_ui(ui, |ui| {
            for (body, label) in &available {
                if ui
                    .add_enabled(
                        definition.scope.len() < MAX_SCOPE && !definition.scope.contains(body),
                        egui::Button::new(label),
                    )
                    .clicked()
                {
                    add = Some(body.clone());
                }
            }
        });
    let picked = w
        .selected
        .as_ref()
        .filter(|selected| selected.reference.topology == "face")
        .map(|selected| crate::reference_ui::selected_body(w, selected));
    let eligible = picked.filter(|body| {
        body.definition == owner
            && available.iter().any(|(candidate, _)| candidate == body)
            && !definition.scope.contains(body)
    });
    if ui
        .add_enabled(
            eligible.is_some() && definition.scope.len() < MAX_SCOPE,
            egui::Button::new("Add Selected Solid Output"),
        )
        .clicked()
    {
        add = eligible;
    }
    if let Some(body) = add {
        definition.scope.push(body);
        changed = true;
    }
    let mut remove = None;
    for (index, body) in definition.scope.iter().enumerate() {
        ui.horizontal(|ui| {
            ui.label(format!("{} · {}", index + 1, source_label(doc, body)));
            if ui.small_button("Remove").clicked() {
                remove = Some(index);
            }
        });
    }
    if let Some(index) = remove {
        definition.scope.remove(index);
        changed = true;
    }
    ui.small("A selected planar face supplies its full supporting plane. All solids in each scoped output participate. Other outputs remain outside this scope.");
    ui.small("Both Sides retains each resulting side. An unsplit body remains once. Keeping an empty side is a valid empty result. Apply waits for the current validated preview.");
    changed
}
