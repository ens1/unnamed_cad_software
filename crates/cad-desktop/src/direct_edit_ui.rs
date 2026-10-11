use crate::{Workspace, ui};
use bevy_egui::egui;
use cad_core::{
    Document, Feature, Operation,
    commands::Command,
    direct_edit::{Definition, Method},
    reference_geometry::{AxisSource, OriginAxis, OriginPlane, PlaneSource},
};

pub(crate) fn is_preset(name: &str) -> bool {
    matches!(
        name,
        "Move Face" | "Offset Face" | "Delete Face" | "Replace Face"
    )
}
pub(crate) fn preset(w: &mut Workspace, name: &str) {
    if w.tool_checkpoint.is_some() || w.sketch.is_some() {
        w.status = "Confirm or cancel the current operation before editing faces".into();
        return;
    }
    let Some(selected) = w
        .selected
        .clone()
        .filter(|s| s.reference.topology == "face")
    else {
        w.status = "Select a model face to edit".into();
        return;
    };
    let body = crate::reference_ui::selected_body(w, &selected);
    if w.view.active_part.as_deref() != Some(body.definition.as_str()) {
        w.status = "Use Edit in Place on this component before editing its faces".into();
        return;
    }
    let Some(mut part) = w.session.visible().parts.get(&body.definition).cloned() else {
        return;
    };
    if w.session
        .visible()
        .libraries
        .values()
        .any(|link| link.pinned_definitions.contains_key(&part.id))
    {
        w.status = "Make the library component unique before editing its faces".into();
        return;
    }
    if !part.features.iter().any(|f| {
        f.id == body.node
            && f.operation.produces_solid()
            && !cad_core::configuration::feature_suppressed(w.session.visible(), &part.id, f)
    }) {
        w.status = "Direct face editing requires a solid body".into();
        return;
    }
    let method = match name {
        "Move Face" => Method::Move {
            translation: std::array::from_fn(|_| "0 mm".into()),
            axis: Box::new(AxisSource::Origin {
                axis: OriginAxis::Z,
            }),
            angle: "0 deg".into(),
        },
        "Delete Face" => Method::DeleteHeal,
        "Replace Face" => Method::Replace {
            plane: Box::new(PlaneSource::Origin {
                plane: OriginPlane::Top,
            }),
            reverse: false,
        },
        _ => Method::Offset {
            distance: "1 mm".into(),
        },
    };
    let definition = Definition {
        body,
        faces: vec![selected.reference],
        method,
    };
    let feature = Feature {
        id: cad_core::id(),
        name: format!(
            "{}{}",
            name,
            part.features
                .iter()
                .filter(|f| f.name.starts_with(name))
                .count()
                + 1
        ),
        operation: Operation::DirectEdit {
            definition: Box::new(definition),
        },
        suppressed: false,
    };
    for input in feature.operation.consumed_outputs() {
        part.outputs.retain(|id| id != &input.node);
    }
    part.outputs.push(feature.id.clone());
    part.features.push(feature.clone());
    crate::history::latest(w);
    w.start_tool();
    w.view.active_part = Some(part.id.clone());
    w.dispatch(Command::PutPart {
        part,
        assembly: None,
        placement: Default::default(),
    });
    if !w
        .session
        .visible()
        .parts
        .values()
        .any(|part| part.features.iter().any(|node| node.id == feature.id))
    {
        let error = w.status.clone();
        w.cancel_tool();
        w.status = error;
        return;
    }
    w.view.active_node = Some(feature.id.clone());
    ui::feature_dialog(w, Some(feature));
}
pub(crate) fn fields(
    ui: &mut egui::Ui,
    w: &Workspace,
    doc: &Document,
    current: &str,
    definition: &mut Definition,
) -> bool {
    let mut changed = false;
    let choices = doc
        .parts
        .get(&definition.body.definition)
        .into_iter()
        .flat_map(|part| {
            part.features
                .iter()
                .take_while(|f| f.id != current)
                .filter(move |f| {
                    f.operation.produces_solid()
                        && !cad_core::configuration::feature_suppressed(doc, &part.id, f)
                })
                .map(move |f| (part.reference(&f.id), f.name.clone()))
        })
        .collect::<Vec<_>>();
    let before = definition.body.clone();
    egui::ComboBox::from_label("Source Body")
        .selected_text(
            choices
                .iter()
                .find(|(body, _)| body == &definition.body)
                .map(|(_, name)| name.as_str())
                .unwrap_or("Missing source"),
        )
        .show_ui(ui, |ui| {
            for (body, name) in choices {
                changed |= ui
                    .selectable_value(&mut definition.body, body, name)
                    .changed();
            }
        });
    if before != definition.body {
        definition.faces.clear();
    }
    ui.label(format!("Faces: {}", definition.faces.len()));
    let picked = w.selected.as_ref().filter(|s| {
        s.reference.topology == "face"
            && crate::reference_ui::selected_body(w, s) == definition.body
    });
    if ui
        .add_enabled(picked.is_some(), egui::Button::new("Add Selected Face"))
        .clicked()
        && let Some(picked) = picked
        && !definition.faces.iter().any(|face| {
            face.producer == picked.reference.producer && face.role == picked.reference.role
        })
    {
        definition.faces.push(picked.reference.clone());
        changed = true;
    }
    let mut remove = None;
    for (i, face) in definition.faces.iter().enumerate() {
        ui.horizontal(|ui| {
            ui.label(format!(
                "Face {} · {}",
                i + 1,
                doc.parts
                    .get(&face.producer.definition)
                    .and_then(|p| p.features.iter().find(|f| f.id == face.producer.node))
                    .map(|f| f.name.as_str())
                    .unwrap_or("Source")
            ));
            if ui.small_button("Remove").clicked() {
                remove = Some(i);
            }
        });
    }
    if let Some(index) = remove {
        definition.faces.remove(index);
        changed = true;
    }
    match &mut definition.method {
        Method::Offset { distance } => {
            changed |= crate::cad_ui::expression(ui, "Normal Offset (+ outward)", distance)
        }
        Method::Move {
            translation,
            axis,
            angle,
        } => {
            for (i, value) in translation.iter_mut().enumerate() {
                changed |= crate::cad_ui::expression(
                    ui,
                    ["X Translation", "Y Translation", "Z Translation"][i],
                    value,
                );
            }
            changed |= crate::reference_ui::axis(ui, w, doc, current, "Rotation Axis", axis);
            changed |= crate::cad_ui::expression(ui, "Rotation", angle);
        }
        Method::DeleteHeal => {
            ui.label("Heal by extending the remaining supporting faces.");
        }
        Method::Replace { plane, reverse } => {
            changed |= crate::reference_ui::plane(ui, w, doc, current, "Replacement Plane", plane);
            changed |= ui.checkbox(reverse, "Reverse Material Side").changed();
        }
    }
    ui.small("Current support: convex solids with unsplit planar faces and straight edges. The preview must remain closed and bounded.");
    changed
}
