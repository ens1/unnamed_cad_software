//! Docked associative assembly component patterns with stable occurrence IDs.
use crate::Workspace;
use bevy_egui::egui;
use cad_core::{
    commands::Command,
    component_pattern::{self, AxisReference, Definition, Mode},
};
fn parsed(w: &Workspace) -> cad_core::Result<Definition> {
    let p: Definition = serde_json::from_str(&w.json)?;
    if w.fields.get("component-pattern-editor") != Some(&p.id) {
        return Err(cad_core::CadError::Invalid(
            "Component pattern identity cannot be changed".into(),
        ));
    }
    Ok(p)
}
pub fn current_source_is_valid(w: &Workspace) -> bool {
    w.fields
        .get("assembly-id")
        .and_then(|id| w.session.visible().assemblies.get(id))
        .is_some_and(|a| parsed(w).is_ok_and(|p| a.component_patterns.get(&p.id) == Some(&p)))
}
fn guard(w: &mut Workspace) {
    if current_source_is_valid(w) {
        w.fields.remove("component-pattern-source-invalid");
    } else {
        w.fields
            .insert("component-pattern-source-invalid".into(), "true".into());
    }
}
fn submit(w: &mut Workspace, assembly: &str, p: Definition) {
    let mut doc = w.session.visible().clone();
    let id = p.id.clone();
    match (Command::PutComponentPattern {
        assembly: assembly.into(),
        pattern: p.into(),
    })
    .execute(&mut doc)
    {
        Ok(()) => {
            let accepted = doc.assemblies[assembly].component_patterns[&id].clone();
            w.dispatch(Command::ReplaceDocument { document: doc });
            if w.session
                .visible()
                .assemblies
                .get(assembly)
                .and_then(|a| a.component_patterns.get(&id))
                == Some(&accepted)
            {
                w.json = serde_json::to_string_pretty(&accepted).unwrap_or_default();
            }
        }
        Err(error) => w.status = error.to_string(),
    }
    guard(w);
}
pub fn begin(w: &mut Workspace, assembly: &str, p: Definition) {
    if w.fields.contains_key("exploded-view-editor")
        || w.fields.contains_key("assembly-inspection-editor")
        || w.fields.contains_key("assembly-weld-editor")
        || w.sketch.is_some()
        || w.fields.contains_key("mate-editor")
        || w.fields.contains_key("configuration-editor")
        || w.dialog.as_deref() == Some("feature")
    {
        w.status =
            "Finish or cancel the current operation before editing a component pattern".into();
        return;
    }
    w.start_tool();
    w.fields.insert("assembly-id".into(), assembly.into());
    w.fields
        .insert("component-pattern-editor".into(), p.id.clone());
    w.json = serde_json::to_string_pretty(&p).unwrap_or_default();
    w.open_editor("assembly");
    guard(w);
}
pub fn new(w: &mut Workspace, assembly: &str) {
    let Some(a) = w.session.visible().assemblies.get(assembly) else {
        return;
    };
    let generated = component_pattern::generated(a);
    let selected = w
        .selected_component
        .as_ref()
        .and_then(|path| path.first())
        .filter(|id| a.instances.contains_key(*id) && !generated.contains(*id))
        .cloned()
        .or_else(|| {
            w.view
                .active_instance
                .as_ref()
                .filter(|id| a.instances.contains_key(*id) && !generated.contains(*id))
                .cloned()
                .or_else(|| {
                    a.instances
                        .keys()
                        .find(|id| !generated.contains(*id))
                        .cloned()
                })
        });
    let Some(seed) = selected else {
        w.status = "Insert a component before creating a component pattern".into();
        return;
    };
    let name = (1..)
        .map(|n| format!("Component Pattern{n}"))
        .find(|name| {
            !a.component_patterns
                .values()
                .any(|p| p.name.eq_ignore_ascii_case(name))
        })
        .unwrap();
    let mut pattern = Definition::new(name);
    pattern.seeds.push(seed);
    begin(w, assembly, pattern.clone());
    if w.fields.get("component-pattern-editor") == Some(&pattern.id) {
        submit(w, assembly, pattern);
    }
}
pub fn show(ui: &mut egui::Ui, w: &mut Workspace, assembly: &str) {
    if !w.fields.contains_key("component-pattern-editor") {
        return;
    }
    let doc = w.session.visible().clone();
    let Some(a) = doc.assemblies.get(assembly) else {
        return;
    };
    ui.strong("Component Pattern");
    ui.small("Patterns follow solved seed placements. Counts include the seed. Pattern copies retain their native IDs when counts shrink or configurations suppress them.");
    if let Ok(mut p) = parsed(w) {
        let mut changed = false;
        ui.label("Name");
        changed |= ui.text_edit_singleline(&mut p.name).changed();
        let mut kind = if matches!(p.mode, Mode::Linear { .. }) {
            "Linear"
        } else {
            "Circular"
        };
        let before = kind;
        egui::ComboBox::from_label("Pattern type")
            .selected_text(kind)
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut kind, "Linear", "Linear");
                ui.selectable_value(&mut kind, "Circular", "Circular");
            });
        if before != kind {
            p.mode = if kind == "Linear" {
                Mode::Linear {
                    step: ["30 mm".into(), "0 mm".into(), "0 mm".into()],
                }
            } else {
                Mode::Circular {
                    origin: ["0 mm".into(), "0 mm".into(), "0 mm".into()],
                    axis: [0., 0., 1.],
                    angle: "30 deg".into(),
                    fixed_orientation: false,
                    axis_reference: None,
                }
            };
            changed = true;
        }
        ui.label("Count / scalar expression");
        changed |= ui.text_edit_singleline(&mut p.count.0).changed();
        ui.strong("Seed components");
        let owned = p
            .occurrences
            .iter()
            .map(|o| o.instance.clone())
            .collect::<std::collections::BTreeSet<_>>();
        for i in a.instances.values().filter(|i| !owned.contains(&i.id)) {
            let mut selected = p.seeds.contains(&i.id);
            if ui.checkbox(&mut selected, &i.name).changed() {
                changed = true;
                if selected {
                    p.seeds.push(i.id.clone());
                } else {
                    p.seeds.retain(|id| id != &i.id);
                }
            }
        }
        match &mut p.mode {
            Mode::Linear { step } => {
                ui.label("Translation per occurrence");
                for (axis, e) in ["X", "Y", "Z"].into_iter().zip(step) {
                    ui.horizontal(|ui| {
                        ui.label(axis);
                        changed |= ui.text_edit_singleline(&mut e.0).changed();
                    });
                }
            }
            Mode::Circular {
                origin,
                axis,
                angle,
                fixed_orientation,
                axis_reference,
            } => {
                ui.label("Angle between occurrences / expression");
                changed |= ui.text_edit_singleline(&mut angle.0).changed();
                changed |= ui
                    .checkbox(fixed_orientation, "Keep each component's orientation fixed")
                    .changed();
                if let Some(reference) = axis_reference {
                    ui.label(format!(
                        "Associative axis: {}",
                        a.instances
                            .get(&reference.instance)
                            .map(|i| i.name.as_str())
                            .unwrap_or("Missing component")
                    ));
                    if ui.small_button("Use numeric axis").clicked() {
                        *axis_reference = None;
                        changed = true;
                    }
                } else {
                    ui.label("Axis origin");
                    for (label, e) in ["X", "Y", "Z"].into_iter().zip(origin) {
                        ui.horizontal(|ui| {
                            ui.label(label);
                            changed |= ui.text_edit_singleline(&mut e.0).changed();
                        });
                    }
                    ui.horizontal(|ui| {
                        ui.label("Axis direction");
                        for value in axis {
                            changed |= ui.add(egui::DragValue::new(value).speed(0.05)).changed();
                        }
                    });
                }
                if ui.button("Use selected axis / planar normal").clicked() {
                    match crate::mate_ui::selected_reference(w, a) {
                        Ok((instance, geometry)) => {
                            if matches!(
                                geometry.source,
                                cad_core::assembly_geometry::Source::Point { .. }
                            ) {
                                w.status =
                                    "Choose an axis, edge, or face normal for the circular pattern"
                                        .into();
                            } else {
                                *axis_reference =
                                    Some(Box::new(AxisReference { instance, geometry }));
                                changed = true;
                            }
                        }
                        Err(error) => w.status = error.to_string(),
                    }
                }
            }
        }
        changed |= ui
            .checkbox(&mut p.copy_internal_mates, "Copy internal seed-group mates")
            .changed();
        ui.small("Copied mates preserve references below each copied component. Mate the seeds to position the group; external mates on generated copies are not supported yet.");
        if changed {
            w.json = serde_json::to_string_pretty(&p).unwrap_or_default();
            submit(w, assembly, p);
        }
    } else {
        ui.colored_label(
            egui::Color32::from_rgb(190, 55, 35),
            "Correct the current component pattern source.",
        );
    }
    let changed = egui::CollapsingHeader::new("Advanced Source")
        .show(ui, |ui| {
            ui.add(
                egui::TextEdit::multiline(&mut w.json)
                    .code_editor()
                    .desired_rows(12)
                    .desired_width(f32::INFINITY),
            )
            .changed()
        })
        .body_returned
        .unwrap_or(false);
    if changed {
        match parsed(w) {
            Ok(p) => submit(w, assembly, p),
            Err(error) => w.status = error.to_string(),
        }
    }
    guard(w);
    if w.fields.contains_key("component-pattern-source-invalid") {
        ui.colored_label(egui::Color32::from_rgb(190,55,35),"The current source has not been accepted; correct the seed/reference/count before applying.");
    }
    if let Some(d) = &w.session.draft {
        if let Some(error) = &d.error {
            ui.colored_label(egui::Color32::from_rgb(190, 55, 35), error);
        } else if d.pending {
            ui.weak("Updating component pattern…");
        }
    }
    ui.horizontal(|ui| {
        if ui
            .add_enabled(current_source_is_valid(w), egui::Button::new("OK"))
            .clicked()
        {
            if w.session.draft.is_none() {
                w.cancel_tool();
                w.status = "Editor closed".into();
            } else {
                w.confirm_tool();
            }
        }
        if ui.button("Cancel").clicked() {
            w.cancel_tool();
        }
    });
}
