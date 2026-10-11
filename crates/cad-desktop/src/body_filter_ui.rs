use crate::{Workspace, ui};
use bevy_egui::egui;
use cad_core::{
    Document, Feature, NodeRef, Operation,
    body_filter::{Definition, MAX_SCOPE, MAX_SELECTIONS, Mode, Selection},
    commands::Command,
};

pub(crate) fn is_preset(name: &str) -> bool {
    matches!(name, "Keep Bodies" | "Delete Bodies")
}

pub(crate) fn preset(w: &mut Workspace, name: &str) {
    if w.tool_checkpoint.is_some() || w.sketch.is_some() {
        w.status = "Confirm or cancel the current operation before filtering bodies".into();
        return;
    }
    let Some(selected) = w
        .selected
        .clone()
        .filter(|selected| selected.reference.topology == "face")
    else {
        w.status = "Select a face on the solid to keep or delete".into();
        return;
    };
    let body = crate::reference_ui::selected_body(w, &selected);
    if w.view.active_part.as_deref() != Some(body.definition.as_str()) {
        w.status = "Use Edit in Place on this component before filtering its bodies".into();
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
        w.status = "Make the library component unique before filtering its bodies".into();
        return;
    }
    if !part
        .features
        .iter()
        .any(|feature| feature.id == body.node && feature.operation.produces_solid())
    {
        w.status = "Keep/Delete Bodies requires a solid output; select a face on a solid".into();
        return;
    }
    let mut face = selected.reference;
    face.signature = None;
    let definition = Definition {
        scope: vec![body.clone()],
        selected: vec![Selection { body, face }],
        mode: if name == "Keep Bodies" {
            Mode::KeepSelected
        } else {
            Mode::DeleteSelected
        },
    };
    let feature = Feature {
        id: cad_core::id(),
        name: format!(
            "{}{}",
            name,
            part.features
                .iter()
                .filter(|feature| feature.name.starts_with(name))
                .count()
                + 1
        ),
        operation: Operation::BodyFilter {
            definition: Box::new(definition),
        },
        suppressed: false,
    };
    for input in feature.operation.consumed_outputs() {
        part.outputs.retain(|output| output != &input.node);
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

fn source_name(doc: &Document, body: &NodeRef) -> String {
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

pub(crate) fn fields(
    ui: &mut egui::Ui,
    w: &Workspace,
    doc: &Document,
    current: &str,
    definition: &mut Definition,
) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        changed |= ui
            .selectable_value(&mut definition.mode, Mode::KeepSelected, "Keep Selected")
            .changed();
        changed |= ui
            .selectable_value(
                &mut definition.mode,
                Mode::DeleteSelected,
                "Delete Selected",
            )
            .changed();
    });
    let owner = doc
        .parts
        .values()
        .find(|part| part.features.iter().any(|feature| feature.id == current))
        .or_else(|| {
            w.view
                .active_part
                .as_ref()
                .and_then(|part| doc.parts.get(part))
        });
    let choices = owner
        .into_iter()
        .flat_map(|part| {
            part.features
                .iter()
                .take_while(|feature| feature.id != current)
                .filter(|feature| feature.operation.produces_solid())
                .map(move |feature| {
                    (
                        part.reference(&feature.id),
                        if cad_core::configuration::feature_suppressed(doc, &part.id, feature) {
                            format!("{} (suppressed in configuration)", feature.name)
                        } else {
                            feature.name.clone()
                        },
                    )
                })
        })
        .collect::<Vec<_>>();
    let picked = w
        .selected
        .as_ref()
        .filter(|selected| selected.reference.topology == "face")
        .map(|selected| (crate::reference_ui::selected_body(w, selected), selected));
    let eligible = picked.as_ref().filter(|(body, _)| {
        owner.is_some_and(|part| part.id == body.definition)
            && choices.iter().any(|(choice, _)| choice == body)
    });
    ui.separator();
    ui.label(format!("Scoped Solid Outputs: {}", definition.scope.len()));
    let mut add_scope = None;
    egui::ComboBox::from_id_salt((current, "body-filter-add-scope"))
        .selected_text("Add Source Output")
        .show_ui(ui, |ui| {
            for (body, name) in &choices {
                if ui
                    .add_enabled(
                        definition.scope.len() < MAX_SCOPE && !definition.scope.contains(body),
                        egui::Button::new(name),
                    )
                    .clicked()
                {
                    add_scope = Some(body.clone());
                }
            }
        });
    if ui
        .add_enabled(
            eligible.is_some_and(|(body, _)| !definition.scope.contains(body))
                && definition.scope.len() < MAX_SCOPE,
            egui::Button::new("Add Selected Output to Scope"),
        )
        .clicked()
        && let Some((body, _)) = eligible
    {
        add_scope = Some(body.clone());
    }
    if let Some(body) = add_scope {
        definition.scope.push(body);
        changed = true;
    }
    let mut remove_scope = None;
    for (index, body) in definition.scope.iter().enumerate() {
        ui.horizontal(|ui| {
            ui.label(format!("{} · {}", index + 1, source_name(doc, body)));
            if ui.small_button("Remove").clicked() {
                remove_scope = Some(index);
            }
        });
    }
    if let Some(index) = remove_scope {
        let body = definition.scope.remove(index);
        definition.selected.retain(|selected| selected.body != body);
        changed = true;
    }
    ui.separator();
    ui.label(format!(
        "Selected Face Anchors: {}",
        definition.selected.len()
    ));
    let selectable = eligible.filter(|(body, _)| definition.scope.contains(body));
    if ui
        .add_enabled(
            selectable.is_some() && definition.selected.len() < MAX_SELECTIONS,
            egui::Button::new("Add Selected Body by Face"),
        )
        .clicked()
        && let Some((body, selected)) = selectable
        && !definition.selected.iter().any(|anchor| {
            anchor.body == *body
                && anchor.face.producer == selected.reference.producer
                && anchor.face.role == selected.reference.role
        })
    {
        let mut face = selected.reference.clone();
        face.signature = None;
        definition.selected.push(Selection {
            body: body.clone(),
            face,
        });
        changed = true;
    }
    let mut remove_anchor = None;
    for (index, selected) in definition.selected.iter().enumerate() {
        ui.horizontal(|ui| {
            ui.label(format!(
                "Anchor {} · {} · {}",
                index + 1,
                source_name(doc, &selected.body),
                source_name(doc, &selected.face.producer)
            ));
            if ui.small_button("Remove").clicked() {
                remove_anchor = Some(index);
            }
        });
    }
    if let Some(index) = remove_anchor {
        definition.selected.remove(index);
        changed = true;
    }
    ui.small("Pick faces on the source solids. Multiple anchors on one solid count as one body. Removing a scope also removes its anchors.");
    ui.small(match definition.mode {
        Mode::KeepSelected => "Only selected whole solids in the scoped outputs survive. Other document outputs remain outside this feature's scope.",
        Mode::DeleteSelected => "Selected whole solids are removed from the scoped outputs. Deleting every scoped solid is a valid empty result.",
    });
    ui.small("An ambiguous or missing construction anchor requires explicit reselection. Apply waits for the current validated preview.");
    changed
}
