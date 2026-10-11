//! Docked saved exploded assembly presentation over mate-solved geometry.
use crate::Workspace;
use bevy_egui::egui;
use cad_core::{
    commands::Command,
    exploded_views::{Definition, Selection, Step, Target},
};

fn parsed(w: &Workspace) -> cad_core::Result<Definition> {
    let view: Definition = serde_json::from_str(&w.json)?;
    if w.fields.get("exploded-view-editor") != Some(&view.id) {
        return Err(cad_core::CadError::Invalid(
            "Exploded view identity cannot change".into(),
        ));
    }
    Ok(view)
}
pub fn current_source_is_valid(w: &Workspace) -> bool {
    w.fields
        .get("assembly-id")
        .and_then(|id| w.session.visible().assemblies.get(id))
        .is_some_and(|a| {
            parsed(w).is_ok_and(|v| {
                a.exploded_views.get(&v.id) == Some(&v)
                    && (v.configuration.is_none()
                        || v.configuration.as_ref()
                            == w.session.visible().active_configurations.get(&a.id))
            })
        })
}
pub fn clear_selection(w: &mut Workspace) {
    w.selected = None;
    w.context_selection = None;
    w.selection_popup = None;
    w.candidates.clear();
}
fn guard(w: &mut Workspace) {
    if current_source_is_valid(w) {
        w.fields.remove("exploded-view-source-invalid");
    } else {
        w.fields
            .insert("exploded-view-source-invalid".into(), "true".into());
    }
}
fn submit(w: &mut Workspace, assembly: &str, view: Definition) {
    w.json = serde_json::to_string_pretty(&view).unwrap_or_default();
    let mut doc = w.session.visible().clone();
    match (Command::PutExplodedView {
        assembly: assembly.into(),
        view: Box::new(view.clone()),
    })
    .execute(&mut doc)
    {
        Ok(()) => {
            w.dispatch(Command::ReplaceDocument { document: doc });
            clear_selection(w);
            w.view.exploded_view = Some(Selection {
                view: view.id,
                stage: view.steps.len(),
                show_lines: true,
            });
            w.rendered_key.clear();
        }
        Err(error) => w.status = error.to_string(),
    }
    guard(w);
}
pub fn begin(w: &mut Workspace, assembly: &str, view: Definition) {
    if assembly != w.session.visible().root {
        w.status = "Open the owning assembly as the document root to edit its exploded view".into();
        return;
    }
    if view
        .configuration
        .as_ref()
        .is_some_and(|c| w.session.visible().active_configurations.get(assembly) != Some(c))
    {
        w.status = "Select this view's linked assembly configuration before editing it".into();
        return;
    }
    if w.sketch.is_some()
        || w.tool_checkpoint.is_some()
        || w.dialog.as_deref() == Some("feature")
        || w.fields.contains_key("mate-editor")
        || w.fields.contains_key("component-pattern-editor")
        || w.fields.contains_key("configuration-editor")
    {
        w.status = "Confirm or cancel the current operation before editing an exploded view".into();
        return;
    }
    let before = serde_json::to_string(&w.view.exploded_view).unwrap_or_default();
    w.start_tool();
    w.fields.insert("explode-before-view".into(), before);
    w.fields.insert("assembly-id".into(), assembly.into());
    w.fields
        .insert("exploded-view-editor".into(), view.id.clone());
    w.json = serde_json::to_string_pretty(&view).unwrap_or_default();
    clear_selection(w);
    w.view.exploded_view = Some(Selection {
        view: view.id,
        stage: view.steps.len(),
        show_lines: true,
    });
    w.open_editor("assembly");
    w.rendered_key.clear();
    guard(w);
}
pub fn new(w: &mut Workspace) {
    let doc = w.session.visible();
    let Some(a) = doc.assemblies.get(&doc.root) else {
        w.status = "Exploded views belong to assemblies".into();
        return;
    };
    let path = w
        .selected
        .as_ref()
        .map(|s| s.path.clone())
        .filter(|p| !p.is_empty())
        .or_else(|| a.instances.keys().next().map(|id| vec![id.clone()]));
    let Some(path) = path else {
        w.status = "Insert a component before creating an exploded view".into();
        return;
    };
    let name = (1..)
        .map(|n| format!("Exploded View{n}"))
        .find(|name| {
            !a.exploded_views
                .values()
                .any(|v| v.name.eq_ignore_ascii_case(name))
        })
        .unwrap();
    let mut v = Definition::new(name);
    let mut step = Step::new("Explode Step1".into());
    step.targets.push(Target {
        path,
        anchor: ["0 mm".into(), "0 mm".into(), "0 mm".into()],
        show_line: true,
    });
    v.steps.push(step);
    let owner = a.id.clone();
    begin(w, &owner, v.clone());
    if w.fields.get("exploded-view-editor") == Some(&v.id) {
        submit(w, &owner, v);
    }
}
pub fn cancel(w: &mut Workspace) {
    if let Some(before) = w.fields.remove("explode-before-view") {
        clear_selection(w);
        w.view.exploded_view = serde_json::from_str(&before).ok().flatten();
    }
    w.fields.remove("exploded-view-editor");
    w.fields.remove("exploded-view-source-invalid");
}
/// Modeling tools consume mechanical coordinates. Rebase an existing picked
/// point back to the assembled occurrence before leaving the exploded display.
pub fn collapse_for_modeling(w: &mut Workspace) {
    if w.view.exploded_view.is_none() {
        return;
    }
    let rebased = w.selected.as_ref().and_then(|s| {
        let e = crate::viewport::display_evaluation(w)?;
        let display = crate::viewport::display_instances(w, e)
            .iter()
            .find(|i| i.path == s.path)?;
        let original = e.instances.iter().find(|i| i.path == s.path)?;
        let shifted: [f64; 3] = std::array::from_fn(|r| s.point[r] - display.matrix[r][3]);
        let local: [f64; 3] =
            std::array::from_fn(|r| (0..3).map(|c| display.matrix[c][r] * shifted[c]).sum());
        let point = crate::viewport::placed(local, original);
        point.iter().all(|v| v.is_finite()).then_some(point)
    });
    if let Some(point) = rebased {
        if let Some(s) = &mut w.selected {
            s.point = point;
        }
    } else {
        w.selected = None;
    }
    w.view.exploded_view = None;
    w.rendered_key.clear();
}
fn quantity(ui: &mut egui::Ui, label: &str, e: &mut cad_core::Expr) -> bool {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.text_edit_singleline(&mut e.0).changed()
    })
    .inner
}
fn quantities(ui: &mut egui::Ui, label: &str, expressions: &mut [cad_core::Expr; 3]) -> bool {
    ui.label(label);
    let mut changed = false;
    for (label, e) in ["X", "Y", "Z"].into_iter().zip(expressions) {
        changed |= quantity(ui, label, e);
    }
    changed
}
fn candidates(w: &Workspace) -> Vec<(Vec<String>, String)> {
    let doc = w.session.visible();
    let mut values = Vec::new();
    fn walk(
        doc: &cad_core::Document,
        owner: &str,
        path: &[String],
        label: &str,
        active: &mut std::collections::BTreeSet<String>,
        values: &mut Vec<(Vec<String>, String)>,
    ) {
        if path.len() >= 64 || values.len() >= 4096 || !active.insert(owner.into()) {
            return;
        }
        if let Some(a) = doc.assemblies.get(owner) {
            for i in a.instances.values() {
                if values.len() >= 4096 {
                    break;
                }
                let mut child = path.to_vec();
                child.push(i.id.clone());
                let name = if label.is_empty() {
                    i.name.clone()
                } else {
                    format!("{label} / {}", i.name)
                };
                values.push((child.clone(), name.clone()));
                walk(doc, &i.definition, &child, &name, active, values);
            }
        }
        active.remove(owner);
    }
    walk(
        doc,
        &doc.root,
        &[],
        "",
        &mut std::collections::BTreeSet::new(),
        &mut values,
    );
    values
}
pub fn show(ui: &mut egui::Ui, w: &mut Workspace, assembly: &str) {
    ui.strong("Exploded View");
    ui.small("Saved display steps follow current configured mate placements. Distances and rotations use assembly axes. Connector anchors use each leaf component's local coordinates.");
    let doc = w.session.visible().clone();
    let candidates = candidates(w);
    if let Ok(mut view) = parsed(w) {
        let mut changed = false;
        ui.label("Name");
        changed |= ui.text_edit_singleline(&mut view.name).changed();
        let before = view.configuration.clone();
        egui::ComboBox::from_id_salt("explode-configuration")
            .selected_text(
                view.configuration
                    .as_ref()
                    .and_then(|id| doc.assemblies[assembly].configurations.get(id))
                    .map(|c| c.name.as_str())
                    .unwrap_or("All configurations"),
            )
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut view.configuration, None, "All configurations");
                if let Some(c) = doc
                    .active_configurations
                    .get(assembly)
                    .and_then(|id| doc.assemblies[assembly].configurations.get(id))
                {
                    ui.selectable_value(&mut view.configuration, Some(c.id.clone()), &c.name);
                }
            });
        changed |= before != view.configuration;
        let mut remove = None;
        let mut reorder = None;
        let count = view.steps.len();
        for (index, step) in view.steps.iter_mut().enumerate() {
            egui::CollapsingHeader::new(format!("{:02}  {}", index + 1, step.name))
                .id_salt(&step.id)
                .default_open(index + 1 == count)
                .show(ui, |ui| {
                    changed |= ui.text_edit_singleline(&mut step.name).changed();
                    changed |= ui.checkbox(&mut step.suppressed, "Suppress step").changed();
                    changed |=
                        quantities(ui, "Direction / scalar expressions", &mut step.direction);
                    changed |= quantity(ui, "Distance", &mut step.distance);
                    changed |= quantity(ui, "Rotation about direction", &mut step.angle);
                    changed |= quantities(ui, "Rotation pivot / lengths", &mut step.pivot);
                    ui.strong("Components / subassemblies");
                    for (path, name) in &candidates {
                        let mut selected = step.targets.iter().any(|t| &t.path == path);
                        if ui.checkbox(&mut selected, name).changed() {
                            changed = true;
                            if selected {
                                step.targets.push(Target {
                                    path: path.clone(),
                                    anchor: ["0 mm".into(), "0 mm".into(), "0 mm".into()],
                                    show_line: true,
                                });
                            } else {
                                step.targets.retain(|t| &t.path != path);
                            }
                        }
                    }
                    if ui.small_button("Add selected model occurrence").clicked() {
                        if let Some(s) = &w.selected {
                            if !step.targets.iter().any(|t| t.path == s.path) {
                                step.targets.push(Target {
                                    path: s.path.clone(),
                                    anchor: ["0 mm".into(), "0 mm".into(), "0 mm".into()],
                                    show_line: true,
                                });
                                changed = true;
                            }
                        } else {
                            w.status = "Select a component on the model first".into();
                        }
                    }
                    let mut remove_target = None;
                    for (index, target) in step.targets.iter_mut().enumerate() {
                        egui::CollapsingHeader::new(format!(
                            "Connector · {}",
                            candidates
                                .iter()
                                .find(|(p, _)| p == &target.path)
                                .map(|(_, n)| n.as_str())
                                .unwrap_or("Missing occurrence")
                        ))
                        .id_salt((&step.id, index))
                        .show(ui, |ui| {
                            changed |= ui
                                .checkbox(&mut target.show_line, "Show displacement connector")
                                .changed();
                            changed |= quantities(ui, "Local anchor / lengths", &mut target.anchor);
                            if ui.small_button("Remove target").clicked() {
                                remove_target = Some(index);
                            }
                        });
                    }
                    if let Some(index) = remove_target {
                        step.targets.remove(index);
                        changed = true;
                    }
                    ui.horizontal(|ui| {
                        if ui
                            .add_enabled(index > 0, egui::Button::new("Move up"))
                            .clicked()
                        {
                            reorder = Some((index, index - 1));
                        }
                        if ui
                            .add_enabled(index + 1 < count, egui::Button::new("Move down"))
                            .clicked()
                        {
                            reorder = Some((index, index + 1));
                        }
                        if ui.small_button("Delete step").clicked() {
                            remove = Some(index);
                        }
                        if ui.small_button("Show this stage").clicked() {
                            clear_selection(w);
                            w.view.exploded_view = Some(Selection {
                                view: view.id.clone(),
                                stage: index + 1,
                                show_lines: true,
                            });
                            w.rendered_key.clear();
                        }
                    });
                });
        }
        if let Some(index) = remove {
            view.steps.remove(index);
            changed = true;
        }
        if let Some((a, b)) = reorder {
            view.steps.swap(a, b);
            changed = true;
        }
        if ui
            .add_enabled(
                view.steps.len() < 128,
                egui::Button::new("Add explode step"),
            )
            .clicked()
        {
            let mut step = Step::new(format!("Explode Step{}", view.steps.len() + 1));
            if let Some((path, _)) = candidates.first() {
                step.targets.push(Target {
                    path: path.clone(),
                    anchor: ["0 mm".into(), "0 mm".into(), "0 mm".into()],
                    show_line: true,
                });
            }
            view.steps.push(step);
            changed = true;
        }
        ui.horizontal(|ui| {
            if ui.button("Assembled").clicked() {
                clear_selection(w);
                w.view.exploded_view = None;
                w.rendered_key.clear();
            }
            if ui.button("Fully exploded").clicked() {
                clear_selection(w);
                w.view.exploded_view = Some(Selection {
                    view: view.id.clone(),
                    stage: view.steps.len(),
                    show_lines: true,
                });
                w.rendered_key.clear();
            }
        });
        if let Some(e) =
            crate::viewport::display_evaluation(w).and_then(|e| e.exploded_views.get(&view.id))
        {
            let inactive = e.stages.iter().flat_map(|s| &s.inactive_targets).count();
            if inactive > 0 {
                ui.small(format!(
                    "{inactive} step targets have no active leaves in this configuration."
                ));
            }
        }
        if changed {
            submit(w, assembly, view);
        }
    } else {
        ui.colored_label(
            egui::Color32::from_rgb(185, 60, 30),
            "Correct the exploded view source.",
        );
    }
    egui::CollapsingHeader::new("Advanced Source").show(ui, |ui| {
        ui.add(
            egui::TextEdit::multiline(&mut w.json)
                .font(egui::TextStyle::Monospace)
                .desired_rows(8),
        );
        if ui.button("Validate source and preview").clicked() {
            match parsed(w) {
                Ok(v) => submit(w, assembly, v),
                Err(e) => w.status = e.to_string(),
            };
        }
    });
    guard(w);
    if !current_source_is_valid(w) {
        ui.small("The current source must validate before OK.");
    }
}
pub fn view_controls(ui: &mut egui::Ui, w: &mut Workspace) {
    let doc = w.session.visible().clone();
    let Some(a) = doc.assemblies.get(&doc.root) else {
        return;
    };
    if a.exploded_views.is_empty() {
        return;
    }
    ui.separator();
    ui.strong("Exploded views");
    if ui.button("Assembled").clicked() {
        clear_selection(w);
        w.view.exploded_view = None;
        w.selected = None;
        w.rendered_key.clear();
    }
    for v in a.exploded_views.values() {
        let available = crate::viewport::display_evaluation(w)
            .is_some_and(|e| e.exploded_views.contains_key(&v.id));
        if ui
            .add_enabled(available, egui::Button::new(&v.name))
            .clicked()
        {
            clear_selection(w);
            w.view.exploded_view = Some(Selection {
                view: v.id.clone(),
                stage: v.steps.len(),
                show_lines: true,
            });
            w.selected = None;
            w.rendered_key.clear();
        }
    }
    if let Some(s) = &mut w.view.exploded_view
        && ui
            .checkbox(&mut s.show_lines, "Show explode lines")
            .changed()
    {
        w.rendered_key.clear();
    }
}
pub fn semantic_action(
    w: &mut Workspace,
    element: &cad_core::presentation::SemanticElement,
    command: &str,
) {
    if command == "assembly.exploded-collapse" {
        clear_selection(w);
        w.view.exploded_view = None;
        w.selected = None;
        w.rendered_key.clear();
        return;
    }
    let view = element
        .data
        .get("view")
        .and_then(|v| v.as_str())
        .unwrap_or(&element.id);
    let target = w.session.visible().assemblies.values().find_map(|a| {
        a.exploded_views
            .get(view)
            .map(|v| (a.id.clone(), v.clone()))
    });
    if let Some((owner, v)) = target {
        match command {
            "assembly.exploded-edit" => begin(w, &owner, v),
            "assembly.exploded-show" | "assembly.exploded-step" => {
                if owner != w.session.visible().root {
                    w.status = "Open the owning assembly to show its exploded view".into();
                    return;
                }
                let stage = if command == "assembly.exploded-step" {
                    v.steps
                        .iter()
                        .position(|s| s.id == element.id)
                        .map(|n| n + 1)
                        .unwrap_or(v.steps.len())
                } else {
                    v.steps.len()
                };
                clear_selection(w);
                w.view.exploded_view = Some(Selection {
                    view: v.id,
                    stage,
                    show_lines: true,
                });
                w.selected = None;
                w.rendered_key.clear();
            }
            _ => {}
        }
    }
}
