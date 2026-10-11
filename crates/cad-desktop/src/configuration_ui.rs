//! Docked named/derived configurations, equations and local design tables.
use crate::Workspace;
use bevy_egui::egui;
use cad_core::{
    Document, Expr, Id, Parameter,
    commands::Command,
    configuration::{self, BomName, Definition},
};
use std::collections::BTreeMap;

fn owner(w: &Workspace) -> Option<Id> {
    w.fields.get("configuration-owner").cloned()
}
fn parameters(doc: &Document, owner: &str) -> BTreeMap<Id, Parameter> {
    doc.parts
        .get(owner)
        .map(|p| p.parameters.clone())
        .or_else(|| doc.assemblies.get(owner).map(|a| a.parameters.clone()))
        .unwrap_or_default()
}
fn set_parameters(doc: &mut Document, owner: &str, values: BTreeMap<Id, Parameter>) {
    if let Some(p) = doc.parts.get_mut(owner) {
        p.parameters = values;
    } else if let Some(a) = doc.assemblies.get_mut(owner) {
        a.parameters = values;
    }
}
fn source(w: &Workspace) -> cad_core::Result<Definition> {
    let c: Definition = serde_json::from_str(&w.json)?;
    if w.fields.get("configuration-editor") != Some(&c.id) {
        return Err(cad_core::CadError::Invalid(
            "Configuration identity cannot be changed in its editor".into(),
        ));
    }
    Ok(c)
}
pub fn current_source_is_valid(w: &Workspace) -> bool {
    let Some(owner) = owner(w) else {
        return false;
    };
    let equations = w.fields.get("configuration-equations").is_none_or(|text| {
        serde_json::from_str::<BTreeMap<Id, Parameter>>(text)
            .is_ok_and(|values| values == parameters(w.session.visible(), &owner))
    });
    if !equations {
        return false;
    }
    if w.fields.get("configuration-editor").map(String::as_str) == Some("base") {
        return !w
            .session
            .visible()
            .active_configurations
            .contains_key(&owner);
    }
    source(w).is_ok_and(|c| {
        w.session.visible().active_configurations.get(&owner) == Some(&c.id)
            && configuration::definitions(w.session.visible(), &owner).get(&c.id) == Some(&c)
    })
}
fn synchronized(w: &mut Workspace) {
    if current_source_is_valid(w) {
        w.fields.remove("configuration-source-invalid");
    } else {
        w.fields
            .insert("configuration-source-invalid".into(), "true".into());
    }
}
fn load(w: &mut Workspace, owner: &str, selected: Option<&str>) {
    w.fields.insert("configuration-owner".into(), owner.into());
    w.fields.insert(
        "configuration-editor".into(),
        selected.unwrap_or("base").into(),
    );
    w.fields.remove("configuration-equations");
    w.json = selected
        .and_then(|id| {
            configuration::definitions(w.session.visible(), owner)
                .get(id)
                .cloned()
        })
        .and_then(|c| serde_json::to_string_pretty(&c).ok())
        .unwrap_or_default();
    synchronized(w);
}
pub fn begin(w: &mut Workspace, tab: &str) {
    if w.fields.contains_key("component-pattern-editor")
        || w.sketch.is_some()
        || w.dialog.as_deref().is_some_and(|dialog| {
            matches!(dialog, "feature" | "assembly") && w.tool_checkpoint.is_some()
        })
    {
        w.status =
            "Finish or cancel the current modeling operation before editing configurations".into();
        return;
    }
    w.start_tool();
    w.fields.remove("mate-editor");
    w.fields.insert("configuration-tab".into(), tab.into());
    let owner = w
        .view
        .active_part
        .clone()
        .filter(|id| w.session.visible().parts.contains_key(id))
        .unwrap_or_else(|| w.session.visible().root.clone());
    let selected = w
        .session
        .visible()
        .active_configurations
        .get(&owner)
        .cloned();
    load(w, &owner, selected.as_deref());
    w.open_editor("configurations");
}
fn select(w: &mut Workspace, owner: &str, id: Option<Id>) {
    w.dispatch(Command::SelectConfiguration {
        definition: owner.into(),
        configuration: id.clone(),
    });
    if w.session.visible().active_configurations.get(owner) == id.as_ref() {
        load(w, owner, id.as_deref());
    }
}
fn submit(w: &mut Workspace, owner: &str, c: Definition) {
    let mut doc = w.session.visible().clone();
    let id = c.id.clone();
    match configuration::put(&mut doc, owner, c) {
        Ok(()) => {
            doc.active_configurations.insert(owner.into(), id);
            w.dispatch(Command::ReplaceDocument { document: doc });
        }
        Err(error) => w.status = error.to_string(),
    }
    synchronized(w);
}
fn unique_name(doc: &Document, owner: &str, stem: &str) -> String {
    let names = configuration::names(doc, owner);
    for index in 1.. {
        let name = format!("{stem}{index}");
        if !names.values().any(|n| n.eq_ignore_ascii_case(&name)) {
            return name;
        }
    }
    unreachable!()
}
fn add(w: &mut Workspace, owner: &str, mode: &str) {
    let current = source(w).ok();
    let doc = w.session.visible();
    let mut c = if mode == "Duplicate" {
        current
            .clone()
            .unwrap_or_else(|| Definition::new("Configuration"))
    } else {
        Definition::new("Configuration")
    };
    c.id = cad_core::id();
    c.name = unique_name(
        doc,
        owner,
        if mode == "Derived" {
            "Derived"
        } else {
            "Configuration"
        },
    );
    if mode == "Derived" {
        c.parent = current.map(|c| c.id);
    }
    if mode == "Duplicate" {
        c.parent = None;
        if let Some(id) = w.fields.get("configuration-editor")
            && id != "base"
            && let Ok(resolved) = configuration::resolve(doc, owner, id)
        {
            c.overrides = resolved.overrides;
        }
    }
    w.fields
        .insert("configuration-tab".into(), "configuration".into());
    w.fields.insert("configuration-editor".into(), c.id.clone());
    w.json = serde_json::to_string_pretty(&c).unwrap_or_default();
    submit(w, owner, c);
}
fn suppress(ui: &mut egui::Ui, label: &str, key: &str, values: &mut BTreeMap<Id, bool>) -> bool {
    let mut value = values.get(key).copied();
    let before = value;
    egui::ComboBox::from_id_salt(("suppression", key))
        .selected_text(match value {
            None => "Inherit",
            Some(false) => "Resolved",
            Some(true) => "Suppressed",
        })
        .show_ui(ui, |ui| {
            ui.selectable_value(&mut value, None, "Inherit");
            ui.selectable_value(&mut value, Some(false), "Resolved");
            ui.selectable_value(&mut value, Some(true), "Suppressed");
        });
    ui.label(label);
    if before != value {
        if let Some(value) = value {
            values.insert(key.into(), value);
        } else {
            values.remove(key);
        }
        true
    } else {
        false
    }
}
fn optional_text(ui: &mut egui::Ui, label: &str, value: &mut Option<String>, seed: &str) -> bool {
    let mut changed = false;
    let mut explicit = value.is_some();
    ui.horizontal(|ui| {
        changed |= ui.checkbox(&mut explicit, label).changed();
        if explicit {
            if value.is_none() {
                *value = Some(seed.into());
            }
            changed |= ui.text_edit_singleline(value.as_mut().unwrap()).changed();
        } else if value.is_some() {
            *value = None;
        }
    });
    changed
}
fn properties(ui: &mut egui::Ui, w: &mut Workspace, c: &mut Definition) -> bool {
    let mut changed = false;
    let mut remove = None;
    for (key, value) in &mut c.overrides.properties {
        ui.horizontal(|ui| {
            ui.label(key);
            changed |= ui.text_edit_singleline(value).changed();
            if ui.small_button("×").clicked() {
                remove = Some(key.clone());
            }
        });
    }
    if let Some(key) = remove {
        c.overrides.properties.remove(&key);
        changed = true;
    }
    let mut key = w
        .fields
        .get("configuration-property-name")
        .cloned()
        .unwrap_or_default();
    ui.horizontal(|ui| {
        ui.text_edit_singleline(&mut key);
        if ui
            .add_enabled(!key.trim().is_empty(), egui::Button::new("Add property"))
            .clicked()
        {
            c.overrides.properties.entry(key.trim().into()).or_default();
            changed = true;
            key.clear();
        }
    });
    w.fields.insert("configuration-property-name".into(), key);
    changed
}
fn fields(
    ui: &mut egui::Ui,
    w: &mut Workspace,
    doc: &Document,
    owner: &str,
    c: &mut Definition,
) -> bool {
    let mut changed = false;
    ui.label("Name");
    changed |= ui.text_edit_singleline(&mut c.name).changed();
    ui.label("Description");
    changed |= ui.text_edit_singleline(&mut c.description).changed();
    let names = configuration::names(doc, owner);
    egui::ComboBox::from_label("Derived from")
        .selected_text(
            c.parent
                .as_ref()
                .and_then(|id| names.get(id))
                .map(String::as_str)
                .unwrap_or("Base definition"),
        )
        .show_ui(ui, |ui| {
            changed |= ui
                .selectable_value(&mut c.parent, None, "Base definition")
                .changed();
            for (id, name) in &names {
                if id != &c.id
                    && !configuration::resolve(doc, owner, id)
                        .is_ok_and(|r| r.ancestors.contains(&c.id))
                {
                    changed |= ui
                        .selectable_value(&mut c.parent, Some(id.clone()), name)
                        .changed();
                }
            }
        });
    egui::CollapsingHeader::new("Parameter equations")
        .default_open(true)
        .show(ui, |ui| {
            for (id, parameter) in parameters(doc, owner) {
                ui.push_id(&id, |ui| {
                    let mut overridden = c.overrides.parameters.contains_key(&id);
                    ui.horizontal(|ui| {
                        if ui.checkbox(&mut overridden, &parameter.name).changed() {
                            changed = true;
                            if overridden {
                                c.overrides
                                    .parameters
                                    .insert(id.clone(), parameter.expression.clone());
                            } else {
                                c.overrides.parameters.remove(&id);
                            }
                        }
                        if let Some(expr) = c.overrides.parameters.get_mut(&id) {
                            changed |= ui.text_edit_singleline(&mut expr.0).changed();
                        } else {
                            ui.weak("Inherited");
                        }
                    });
                });
            }
        });
    if let Some(part) = doc.parts.get(owner) {
        egui::CollapsingHeader::new("Features")
            .default_open(true)
            .show(ui, |ui| {
                for f in &part.features {
                    ui.horizontal(|ui| {
                        changed |= suppress(ui, &f.name, &f.id, &mut c.overrides.features);
                    });
                }
            });
        egui::CollapsingHeader::new("Sketches").show(ui, |ui| {
            for sketch in part.sketches.values() {
                ui.horizontal(|ui| {
                    changed |= suppress(ui, &sketch.name, &sketch.id, &mut c.overrides.sketches);
                });
            }
        });
        egui::ComboBox::from_label("Material")
            .selected_text(
                c.overrides
                    .material
                    .as_ref()
                    .and_then(|id| doc.materials.get(id))
                    .map(|m| m.name.as_str())
                    .unwrap_or("Inherited material"),
            )
            .show_ui(ui, |ui| {
                changed |= ui
                    .selectable_value(&mut c.overrides.material, None, "Inherited material")
                    .changed();
                for material in doc.materials.values() {
                    changed |= ui
                        .selectable_value(
                            &mut c.overrides.material,
                            Some(material.id.clone()),
                            &material.name,
                        )
                        .changed();
                }
            });
    }
    if let Some(assembly) = doc.assemblies.get(owner) {
        egui::CollapsingHeader::new("Components")
            .default_open(true)
            .show(ui, |ui| {
                for component in assembly.instances.values() {
                    ui.push_id(&component.id, |ui| {
                        ui.horizontal(|ui| {
                            changed |= suppress(
                                ui,
                                &component.name,
                                &component.id,
                                &mut c.overrides.components,
                            );
                        });
                        let names = configuration::names(doc, &component.definition);
                        let before = c
                            .overrides
                            .component_configurations
                            .get(&component.id)
                            .cloned();
                        let mut choice = before.clone();
                        egui::ComboBox::from_label("Component configuration")
                            .selected_text(match &choice {
                                None => "Inherit component selection".into(),
                                Some(None) => "Active definition configuration".into(),
                                Some(Some(id)) => names
                                    .get(id)
                                    .cloned()
                                    .unwrap_or("Missing configuration".into()),
                            })
                            .show_ui(ui, |ui| {
                                ui.selectable_value(
                                    &mut choice,
                                    None,
                                    "Inherit component selection",
                                );
                                ui.selectable_value(
                                    &mut choice,
                                    Some(None),
                                    "Active definition configuration",
                                );
                                for (id, name) in names {
                                    ui.selectable_value(&mut choice, Some(Some(id)), name);
                                }
                            });
                        if choice != before {
                            changed = true;
                            if let Some(choice) = choice {
                                c.overrides
                                    .component_configurations
                                    .insert(component.id.clone(), choice);
                            } else {
                                c.overrides.component_configurations.remove(&component.id);
                            }
                        }
                    });
                }
            });
        egui::CollapsingHeader::new("Component patterns").show(ui, |ui| {
            for pattern in assembly.component_patterns.values() {
                ui.horizontal(|ui| {
                    changed |= suppress(
                        ui,
                        &pattern.name,
                        &pattern.id,
                        &mut c.overrides.component_patterns,
                    );
                });
            }
        });
        egui::CollapsingHeader::new("Mates").show(ui, |ui| {
            for (index, mate) in assembly.mates.iter().enumerate() {
                ui.horizontal(|ui| {
                    changed |= suppress(
                        ui,
                        &format!("{} {}", mate.kind, index + 1),
                        &mate.id,
                        &mut c.overrides.mates,
                    );
                });
            }
        });
    }
    egui::CollapsingHeader::new("Custom properties").show(ui, |ui| {
        changed |= properties(ui, w, c);
    });
    egui::CollapsingHeader::new("Bill of materials").show(ui, |ui| {
        let before = c.overrides.bom.exclude;
        egui::ComboBox::from_label("Include in BOM")
            .selected_text(match before {
                None => "Inherited",
                Some(false) => "Included",
                Some(true) => "Excluded",
            })
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut c.overrides.bom.exclude, None, "Inherited");
                ui.selectable_value(&mut c.overrides.bom.exclude, Some(false), "Included");
                ui.selectable_value(&mut c.overrides.bom.exclude, Some(true), "Excluded");
            });
        changed |= before != c.overrides.bom.exclude;
        egui::ComboBox::from_label("Part number")
            .selected_text(format!("{:?}", c.overrides.bom.name))
            .show_ui(ui, |ui| {
                for (mode, label) in [
                    (None, "Inherited"),
                    (Some(BomName::Document), "Document part number"),
                    (Some(BomName::Configuration), "Configuration name"),
                    (Some(BomName::Custom), "Custom number"),
                ] {
                    changed |= ui
                        .selectable_value(&mut c.overrides.bom.name, mode, label)
                        .changed();
                }
            });
        if c.overrides.bom.name == Some(BomName::Custom) {
            changed |= optional_text(ui, "Custom number", &mut c.overrides.bom.part_number, "");
        }
        changed |= optional_text(
            ui,
            "Override description",
            &mut c.overrides.bom.description,
            "",
        );
    });
    changed
}
fn equations(ui: &mut egui::Ui, w: &mut Workspace, doc: &Document, owner: &str) {
    ui.small("Named equations use mm/in, deg/rad, and scalar units. Reference a parameter with @name or @PartName::name. Cycles and incorrect units are reported by the worker.");
    let mut values = w
        .fields
        .get("configuration-equations")
        .and_then(|text| serde_json::from_str::<BTreeMap<Id, Parameter>>(text).ok())
        .unwrap_or_else(|| parameters(doc, owner));
    let mut changed = false;
    let effective = configuration::effective(doc).ok();
    for (id, parameter) in &mut values {
        ui.push_id(id, |ui| {
            ui.strong(&parameter.name);
            ui.horizontal(|ui| {
                changed |= ui
                    .text_edit_singleline(&mut parameter.expression.0)
                    .changed();
                egui::ComboBox::from_id_salt("units")
                    .selected_text(format!("{:?}", parameter.dimension))
                    .show_ui(ui, |ui| {
                        for (dimension, label) in [
                            (cad_core::Dimension::Length, "Length"),
                            (cad_core::Dimension::Angle, "Angle"),
                            (cad_core::Dimension::Scalar, "Scalar"),
                        ] {
                            changed |= ui
                                .selectable_value(&mut parameter.dimension, dimension, label)
                                .changed();
                        }
                    });
            });
            if let Some(effective) = &effective
                && let Some(current) = parameters(effective, owner).get(id)
            {
                match cad_core::expression::value(
                    effective,
                    owner,
                    &current.expression,
                    current.dimension,
                ) {
                    Ok(value) => {
                        ui.weak(match current.dimension {
                            cad_core::Dimension::Angle => format!("{} deg", value.to_degrees()),
                            cad_core::Dimension::Length => format!("{value} mm"),
                            cad_core::Dimension::Scalar => value.to_string(),
                        });
                    }
                    Err(error) => {
                        ui.colored_label(egui::Color32::from_rgb(190, 55, 35), error.to_string());
                    }
                }
            }
            ui.separator();
        });
    }
    let mut name = w
        .fields
        .get("configuration-parameter-name")
        .cloned()
        .unwrap_or_default();
    ui.horizontal(|ui| {
        ui.text_edit_singleline(&mut name);
        if ui
            .add_enabled(!name.trim().is_empty(), egui::Button::new("Add equation"))
            .clicked()
        {
            values.insert(
                cad_core::id(),
                Parameter {
                    name: name.trim().into(),
                    expression: Expr("10 mm".into()),
                    dimension: cad_core::Dimension::Length,
                },
            );
            changed = true;
            name.clear();
        }
    });
    w.fields.insert("configuration-parameter-name".into(), name);
    if changed {
        w.fields.insert(
            "configuration-equations".into(),
            serde_json::to_string(&values).unwrap_or_default(),
        );
        let mut doc = w.session.visible().clone();
        set_parameters(&mut doc, owner, values);
        w.dispatch(Command::ReplaceDocument { document: doc });
        synchronized(w);
    }
}
fn table(ui: &mut egui::Ui, w: &mut Workspace, doc: &Document, owner: &str) {
    let mut replace = w
        .fields
        .get("configuration-import-replace")
        .is_some_and(|v| v == "true");
    ui.checkbox(&mut replace, "Replace existing configurations on import");
    w.fields
        .insert("configuration-import-replace".into(), replace.to_string());
    ui.horizontal_wrapped(|ui| {
        if ui.button("Import JSON / CSV…").clicked()
            && let Some(path) = rfd::FileDialog::new()
                .add_filter("TCAD design table", &["json", "csv"])
                .pick_file()
        {
            match configuration::read(&path, owner) {
                Ok(table) => {
                    w.dispatch(Command::ImportConfigurationTable {
                        table: table.into(),
                        replace,
                    });
                    let active = w
                        .session
                        .visible()
                        .active_configurations
                        .get(owner)
                        .cloned();
                    load(w, owner, active.as_deref());
                }
                Err(error) => w.status = error.to_string(),
            }
        }
        if ui.button("Export table…").clicked()
            && let Some(path) = rfd::FileDialog::new()
                .add_filter("CSV design table", &["csv"])
                .add_filter("TCAD configuration table", &["json"])
                .set_file_name("configurations.csv")
                .save_file()
        {
            match configuration::write(doc, owner, &path) {
                Ok(()) => w.status = format!("Saved design table {}", path.display()),
                Err(error) => w.status = error.to_string(),
            }
        }
    });
    ui.small("Select a row to preview and edit its inherited equations, suppression, material and BOM values. Edit a parameter cell directly or select a row for all other values. CSV columns may reference parameter/feature/component names or stable IDs; blank cells inherit and suppression uses true/false. JSON preserves the complete native table.");
    let params = parameters(doc, owner);
    egui::ScrollArea::horizontal()
        .id_salt("configuration-grid-scroll")
        .show(ui, |ui| {
            egui::Grid::new("configuration-grid")
                .striped(true)
                .show(ui, |ui| {
                    ui.strong("Configuration");
                    ui.strong("Parent");
                    for p in params.values() {
                        ui.strong(&p.name);
                    }
                    ui.strong("Suppressed");
                    ui.end_row();
                    for mut c in configuration::definitions(doc, owner).into_values() {
                        if let Ok(local) = source(w)
                            && local.id == c.id
                        {
                            c = local;
                        }
                        let mut changed = false;
                        if ui
                            .selectable_label(
                                doc.active_configurations.get(owner) == Some(&c.id),
                                &c.name,
                            )
                            .clicked()
                        {
                            select(w, owner, Some(c.id.clone()));
                        }
                        ui.label(
                            c.parent
                                .as_ref()
                                .and_then(|id| configuration::names(doc, owner).get(id).cloned())
                                .unwrap_or("Base".into()),
                        );
                        let resolved = configuration::resolve(doc, owner, &c.id).ok();
                        for (id, p) in &params {
                            ui.push_id((&c.id, id), |ui| {
                                ui.horizontal(|ui| {
                                    let mut explicit = c.overrides.parameters.contains_key(id);
                                    if ui
                                        .checkbox(&mut explicit, "")
                                        .on_hover_text(
                                            "Override this configuration's parameter equation",
                                        )
                                        .changed()
                                    {
                                        changed = true;
                                        if explicit {
                                            c.overrides.parameters.insert(
                                                id.clone(),
                                                resolved
                                                    .as_ref()
                                                    .and_then(|r| r.overrides.parameters.get(id))
                                                    .cloned()
                                                    .unwrap_or_else(|| p.expression.clone()),
                                            );
                                        } else {
                                            c.overrides.parameters.remove(id);
                                        }
                                    }
                                    if let Some(expression) = c.overrides.parameters.get_mut(id) {
                                        changed |= ui
                                            .add(
                                                egui::TextEdit::singleline(&mut expression.0)
                                                    .desired_width(105.),
                                            )
                                            .changed();
                                    } else {
                                        ui.weak(
                                            resolved
                                                .as_ref()
                                                .and_then(|r| r.overrides.parameters.get(id))
                                                .map(|e| e.0.as_str())
                                                .unwrap_or(&p.expression.0),
                                        );
                                    }
                                });
                            });
                        }
                        ui.label(
                            resolved
                                .map(|r| {
                                    r.overrides
                                        .features
                                        .values()
                                        .chain(r.overrides.sketches.values())
                                        .chain(r.overrides.components.values())
                                        .chain(r.overrides.component_patterns.values())
                                        .chain(r.overrides.mates.values())
                                        .filter(|s| **s)
                                        .count()
                                        .to_string()
                                })
                                .unwrap_or("Invalid".into()),
                        );
                        ui.end_row();
                        if changed {
                            w.fields.insert("configuration-editor".into(), c.id.clone());
                            w.json = serde_json::to_string_pretty(&c).unwrap_or_default();
                            submit(w, owner, c);
                        }
                    }
                });
        });
}
pub fn show(ui: &mut egui::Ui, w: &mut Workspace) {
    let doc = w.session.visible().clone();
    let Some(mut owner) = owner(w) else {
        begin(w, "configuration");
        return;
    };
    let before = owner.clone();
    egui::ComboBox::from_label("Definition")
        .selected_text(
            doc.parts
                .get(&owner)
                .map(|p| p.name.as_str())
                .or_else(|| doc.assemblies.get(&owner).map(|a| a.name.as_str()))
                .unwrap_or("Missing definition"),
        )
        .show_ui(ui, |ui| {
            for p in doc.parts.values() {
                ui.selectable_value(&mut owner, p.id.clone(), &p.name);
            }
            for a in doc.assemblies.values() {
                ui.selectable_value(&mut owner, a.id.clone(), &a.name);
            }
        });
    if before != owner {
        let selected = doc.active_configurations.get(&owner).cloned();
        load(w, &owner, selected.as_deref());
        return;
    }
    let names = configuration::names(&doc, &owner);
    let mut selected = doc.active_configurations.get(&owner).cloned();
    let before = selected.clone();
    egui::ComboBox::from_label("Configuration")
        .selected_text(
            selected
                .as_ref()
                .and_then(|id| names.get(id))
                .map(String::as_str)
                .unwrap_or("Base definition"),
        )
        .show_ui(ui, |ui| {
            ui.selectable_value(&mut selected, None, "Base definition");
            for (id, name) in &names {
                ui.selectable_value(&mut selected, Some(id.clone()), name);
            }
        });
    if selected != before {
        select(w, &owner, selected);
        return;
    }
    ui.horizontal_wrapped(|ui| {
        for mode in ["New", "Derived", "Duplicate"] {
            if ui
                .add_enabled(
                    mode == "New"
                        || w.fields
                            .get("configuration-editor")
                            .is_some_and(|id| id != "base"),
                    egui::Button::new(mode),
                )
                .clicked()
            {
                add(w, &owner, mode);
            }
        }
        if ui
            .add_enabled(selected.is_some(), egui::Button::new("Delete"))
            .clicked()
            && let Some(id) = selected.clone()
        {
            let mut d = doc.clone();
            d.active_configurations.remove(&owner);
            match configuration::remove(&mut d, &owner, &id) {
                Ok(()) => {
                    w.dispatch(Command::ReplaceDocument { document: d });
                    let active = w
                        .session
                        .visible()
                        .active_configurations
                        .get(&owner)
                        .cloned();
                    load(w, &owner, active.as_deref());
                }
                Err(error) => w.status = error.to_string(),
            }
        }
    });
    let mut tab = w
        .fields
        .get("configuration-tab")
        .cloned()
        .unwrap_or("configuration".into());
    ui.horizontal(|ui| {
        for (key, label) in [
            ("configuration", "Configuration"),
            ("equations", "Equations"),
            ("table", "Design table"),
        ] {
            ui.selectable_value(&mut tab, key.into(), label);
        }
    });
    w.fields.insert("configuration-tab".into(), tab.clone());
    ui.separator();
    match tab.as_str() {
        "equations" => equations(ui, w, &doc, &owner),
        "table" => table(ui, w, &doc, &owner),
        _ => {
            if w.fields.get("configuration-editor").map(String::as_str) != Some("base") {
                if let Ok(mut c) = source(w) {
                    if fields(ui, w, &doc, &owner, &mut c) {
                        w.json = serde_json::to_string_pretty(&c).unwrap_or_default();
                        submit(w, &owner, c);
                    }
                } else {
                    ui.colored_label(
                        egui::Color32::from_rgb(190, 55, 35),
                        "Correct the configuration source before applying.",
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
                    match source(w) {
                        Ok(c) => submit(w, &owner, c),
                        Err(error) => {
                            w.status = error.to_string();
                            synchronized(w);
                        }
                    }
                }
            } else {
                ui.label("The base definition is selected. Create a named configuration to override equations, feature/sketch suppression, components, mates, material, properties and BOM choices.");
            }
        }
    }
    synchronized(w);
    if w.fields.contains_key("configuration-source-invalid") {
        ui.colored_label(egui::Color32::from_rgb(190,55,35),"Current edits have not been accepted. Correct the dependency or source before applying.");
    }
    if let Some(draft) = &w.session.draft {
        if let Some(error) = &draft.error {
            ui.colored_label(egui::Color32::from_rgb(190, 55, 35), error);
        } else if draft.pending {
            ui.weak("Updating configuration preview…");
        }
    }
    ui.separator();
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

pub fn open_owner(w: &mut Workspace, owner: &str, configuration: Option<&str>, tab: &str) {
    begin(w, tab);
    if w.dialog.as_deref() != Some("configurations") {
        return;
    }
    let selected = configuration.map(str::to_string).or_else(|| {
        w.session
            .visible()
            .active_configurations
            .get(owner)
            .cloned()
    });
    w.fields.insert("configuration-owner".into(), owner.into());
    select(w, owner, selected);
}
