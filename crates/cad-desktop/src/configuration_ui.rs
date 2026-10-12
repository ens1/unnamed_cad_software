//! Docked named/derived configurations, equations and local design tables.
use crate::Workspace;
use bevy_egui::egui;
use cad_core::{
    Document, Expr, Id, Parameter,
    commands::Command,
    configuration::{self, BomName, Definition},
};
use std::collections::BTreeMap;
#[path = "configuration_poses_ui.rs"]
mod poses;
#[path = "configuration_sketch_dimensions_ui.rs"]
mod sketch_dimensions;

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
    let error_prefix = format!(
        "configuration-pose-error:{}:",
        w.fields
            .get("configuration-editor-generation")
            .map(String::as_str)
            .unwrap_or("")
    );
    if w.fields.keys().any(|key| key.starts_with(&error_prefix)) {
        return false;
    }
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
    w.confirm_requested = false;
    w.after_confirm = None;
    w.fields
        .retain(|key, _| !key.starts_with("configuration-pose-error:"));
    w.fields
        .insert("configuration-editor-generation".into(), cad_core::id());
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
    if (w.tool_checkpoint.is_some() && w.dialog.as_deref() != Some("configurations"))
        || w.fields.contains_key("component-pattern-editor")
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
    w.confirm_requested = false;
    w.after_confirm = None;
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
fn dimension_cell(
    ui: &mut egui::Ui,
    doc: &Document,
    owner: &str,
    c: &mut Definition,
    field: &configuration::DimensionField,
    resolved: Option<&configuration::Resolved>,
) -> bool {
    let count_context = if field.kind == configuration::dimensions::FieldKind::Count {
        Some(configuration::counts::preview_context(doc, owner, c))
    } else {
        None
    };
    let count_resolved = count_context
        .as_ref()
        .and_then(|context| context.as_ref().ok())
        .and_then(|context| configuration::resolve(context, owner, &c.id).ok());
    let resolved = count_resolved.as_ref().or(resolved);
    let inherited = resolved
        .and_then(|r| r.overrides.feature_dimensions.get(&field.feature))
        .and_then(|fields| fields.get(&field.path))
        .unwrap_or(&field.expression);
    let mut explicit = c
        .overrides
        .feature_dimensions
        .get(&field.feature)
        .is_some_and(|fields| fields.contains_key(&field.path));
    let mut changed = false;
    ui.horizontal(|ui| {
        if ui
            .checkbox(&mut explicit, "")
            .on_hover_text("Override this feature dimension in this configuration")
            .changed()
        {
            changed = true;
            if explicit {
                c.overrides
                    .feature_dimensions
                    .entry(field.feature.clone())
                    .or_default()
                    .insert(field.path.clone(), inherited.clone());
            } else if let Some(fields) = c.overrides.feature_dimensions.get_mut(&field.feature) {
                fields.remove(&field.path);
                if fields.is_empty() {
                    c.overrides.feature_dimensions.remove(&field.feature);
                }
            }
        }
        if let Some(expression) = c
            .overrides
            .feature_dimensions
            .get_mut(&field.feature)
            .and_then(|fields| fields.get_mut(&field.path))
        {
            let id = dimension_id(owner, &c.id, field, "expression");
            changed |= crate::presentation_controls::expression(ui, &id, &field.label, expression);
        } else {
            crate::presentation_controls::label(
                ui,
                &dimension_id(owner, &c.id, field, "expression"),
                "configuration-dimension-field",
                &["inherited"],
                &format!("{}: {}", field.label, inherited.0),
                "inherited",
            );
        }
    });
    if field.kind == configuration::dimensions::FieldKind::Count {
        let count = configuration::counts::preview_count(doc, owner, c, field);
        let (text, state) = match &count {
            Ok(value) => (format!("→ {value}"), "resolved"),
            Err(_) => ("Unresolved count".into(), "invalid"),
        };
        crate::presentation_controls::label(
            ui,
            &dimension_id(owner, &c.id, field, "resolved"),
            "configuration-count-result",
            &[state],
            &text,
            state,
        )
        .on_hover_text(match count {
            Ok(_) => "Counts accept scalar parameters and equations, such as @copies * 2. The result must be a nonnegative integer within the operation's bounds.".into(),
            Err(error) => error.to_string(),
        });
    }
    if changed {
        c.version = configuration::CONTRACT_VERSION;
    }
    changed
}
fn dimension_id(
    owner: &str,
    configuration: &str,
    field: &configuration::DimensionField,
    kind: &str,
) -> String {
    format!(
        "configuration-dimension:{owner}/{configuration}/{}{}:{kind}",
        field.feature, field.path
    )
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
        egui::CollapsingHeader::new("Sketch driving dimensions").default_open(true).show(ui, |ui| {
            let resolved = configuration::resolve(doc, owner, &c.id).ok();
            for dimension in configuration::sketch_dimensions::fields(part) {
                changed |= sketch_dimensions::cell(ui, owner, c, &dimension, resolved.as_ref());
            }
            ui.weak("Unchecked dimensions inherit. Saved driving dimensions accept unit expressions and @parameters; reference dimensions are measurements.");
        });
        egui::CollapsingHeader::new("Feature dimensions")
            .default_open(true)
            .show(ui, |ui| {
                let resolved = configuration::resolve(doc, owner, &c.id).ok();
                for feature in &part.features {
                    if let Ok(dimensions) = configuration::dimensions::fields(feature)
                        && !dimensions.is_empty()
                    {
                        egui::CollapsingHeader::new(&feature.name)
                            .id_salt(("configuration-dimensions", &feature.id))
                            .show(ui, |ui| {
                                for dimension in dimensions {
                                    changed |= dimension_cell(ui, doc, owner, c, &dimension, resolved.as_ref());
                                }
                            });
                    }
                }
                ui.weak("Unchecked fields inherit. Expressions use the feature's normal units and may reference @parameters; the current worker preview validates the result.");
            });
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
        egui::CollapsingHeader::new("Weld beads").show(ui, |ui| {
            for bead in assembly.weld_beads.values() {
                changed |= suppress(ui, &bead.name, &bead.id, &mut c.overrides.assembly_welds);
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
    ui.small("Select a row to preview it. Parameters and driving dimensions accept unit expressions; pattern counts require integer literals. Unchecked cells inherit. CSV columns may use unique owner names or stable IDs; sketch constraints retain stable IDs.");
    let params = parameters(doc, owner);
    let dimensions = configuration::dimension_fields(doc, owner).unwrap_or_default();
    let sketch_fields = doc
        .parts
        .get(owner)
        .map(configuration::sketch_dimensions::fields)
        .unwrap_or_default();
    let mut filter = w
        .fields
        .get("configuration-dimension-filter")
        .cloned()
        .unwrap_or_default();
    let mut show_dimensions = w
        .fields
        .get("configuration-show-dimensions")
        .is_none_or(|value| value != "false");
    ui.horizontal(|ui| {
        ui.checkbox(&mut show_dimensions, "Dimensions and counts");
        ui.add_enabled(
            show_dimensions,
            egui::TextEdit::singleline(&mut filter).hint_text("Filter feature or field…"),
        );
    });
    w.fields
        .insert("configuration-dimension-filter".into(), filter.clone());
    w.fields.insert(
        "configuration-show-dimensions".into(),
        show_dimensions.to_string(),
    );
    let dimensions = dimensions
        .into_iter()
        .filter(|field| {
            show_dimensions && field.label.to_lowercase().contains(&filter.to_lowercase())
        })
        .collect::<Vec<_>>();
    let sketch_fields = sketch_fields
        .into_iter()
        .filter(|field| {
            show_dimensions && field.label.to_lowercase().contains(&filter.to_lowercase())
        })
        .collect::<Vec<_>>();
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
                    for field in &dimensions {
                        ui.strong(&field.label)
                            .on_hover_text(format!("dimension:{}:{}", field.feature, field.path));
                    }
                    for field in &sketch_fields {
                        ui.strong(&field.label).on_hover_text(format!(
                            "sketch-dimension:{}:{}",
                            field.sketch, field.constraint
                        ));
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
                        for field in &dimensions {
                            ui.push_id((c.id.clone(), &field.feature, &field.path), |ui| {
                                changed |= dimension_cell(
                                    ui,
                                    doc,
                                    owner,
                                    &mut c,
                                    field,
                                    resolved.as_ref(),
                                );
                            });
                        }
                        for field in &sketch_fields {
                            ui.push_id((c.id.clone(), &field.sketch, &field.constraint), |ui| {
                                changed |= sketch_dimensions::cell(
                                    ui,
                                    owner,
                                    &mut c,
                                    field,
                                    resolved.as_ref(),
                                );
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
                                        .chain(r.overrides.assembly_welds.values())
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
    poses::show(ui, w, true);
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
                    poses::show(ui, w, false);
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
                poses::show(ui, w, false);
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

/// Typed configuration controls are independent of the docked table arrangement.
pub fn semantics(w: &Workspace) -> Vec<cad_core::presentation::SemanticElement> {
    if w.dialog.as_deref() != Some("configurations") {
        return Vec::new();
    }
    let Some(owner) = owner(w) else {
        return Vec::new();
    };
    let doc = w.session.visible();
    let Ok(fields) = configuration::dimension_fields(doc, &owner) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    out.extend(poses::semantics(w));
    out.extend(sketch_dimensions::semantics(w));
    for mut c in configuration::definitions(doc, &owner).into_values() {
        if let Ok(local) = source(w)
            && local.id == c.id
        {
            c = local;
        }
        let resolved = configuration::resolve(doc, &owner, &c.id).ok();
        if let Some(assembly) = doc.assemblies.get(&owner) {
            for bead in assembly.weld_beads.values() {
                let explicit = c.overrides.assembly_welds.get(&bead.id).copied();
                let effective = explicit
                    .or_else(|| {
                        resolved
                            .as_ref()
                            .and_then(|resolved| resolved.overrides.assembly_welds.get(&bead.id))
                            .copied()
                    })
                    .unwrap_or(bead.suppressed);
                out.push(cad_core::presentation::SemanticElement {
                    id:format!("configuration-weld:{owner}@{}:{}",c.id,bead.id),role:"configuration-weld-field".into(),label:format!("{} · {}",c.name,bead.name),value:Some(explicit.map(|value|value.to_string()).unwrap_or("Inherited".into())),state:if explicit.is_some(){"overridden"}else{"inherited"}.into(),commands:vec![],relationships:vec![owner.clone(),format!("{owner}@{}",c.id),bead.id.clone()],
                    data:serde_json::json!({"contract":"tcad.configuration.weld-suppression","version":1,"setter":"configuration-dimension","owner":owner,"configuration":c.id,"weld":bead.id,"json":explicit,"value_type":"choice","editable":true,"effective_suppressed":effective,"choices":[{"value":null,"label":"Inherited"},{"value":false,"label":"Present"},{"value":true,"label":"Suppressed"}]}),
                });
            }
        }
        let count_context = if fields
            .iter()
            .any(|field| field.kind == configuration::dimensions::FieldKind::Count)
        {
            Some(configuration::counts::preview_context(doc, &owner, &c))
        } else {
            None
        };
        let count_resolved = count_context
            .as_ref()
            .and_then(|context| context.as_ref().ok())
            .and_then(|context| configuration::resolve(context, &owner, &c.id).ok());
        for dimension in &fields {
            let explicit = c
                .overrides
                .feature_dimensions
                .get(&dimension.feature)
                .and_then(|fields| fields.get(&dimension.path));
            let dimension_resolved =
                if dimension.kind == configuration::dimensions::FieldKind::Count {
                    count_resolved.as_ref().or(resolved.as_ref())
                } else {
                    resolved.as_ref()
                };
            let expression = explicit
                .or_else(|| {
                    dimension_resolved
                        .and_then(|r| r.overrides.feature_dimensions.get(&dimension.feature))
                        .and_then(|fields| fields.get(&dimension.path))
                })
                .unwrap_or(&dimension.expression);
            let count = if dimension.kind == configuration::dimensions::FieldKind::Count {
                Some(match count_context.as_ref() {
                    Some(Ok(context)) => configuration::dimensions::count_value(
                        context, &owner, dimension, expression,
                    ),
                    Some(Err(error)) => Err(cad_core::CadError::Invalid(error.to_string())),
                    None => Err(cad_core::CadError::Invalid(
                        "Count parameter context is missing".into(),
                    )),
                })
            } else {
                None
            };
            for (field, value, editable) in [
                ("override", serde_json::json!(explicit.is_some()), true),
                (
                    "expression",
                    serde_json::json!(expression.0),
                    explicit.is_some(),
                ),
            ] {
                out.push(cad_core::presentation::SemanticElement {
                    id: dimension_id(&owner, &c.id, dimension, field),
                    role: "configuration-dimension-field".into(),
                    label: format!("{} · {}{}", c.name, dimension.label, if field == "override" { " override" } else { "" }),
                    value: Some(if field == "override" { explicit.is_some().to_string() } else { expression.0.clone() }),
                    state: if field == "expression" && count.as_ref().is_some_and(Result::is_err) { "invalid" } else if explicit.is_some() { "overridden" } else { "inherited" }.into(),
                    commands: Vec::new(),
                    relationships: vec![owner.clone(), format!("{owner}@{}", c.id), dimension.feature.clone()],
                    data: serde_json::json!({"contract":"tcad.configuration.dimension-field","version":configuration::FIELD_CONTRACT_VERSION,
                        "setter":"configuration-dimension","owner":owner,"configuration":c.id,"feature":dimension.feature,
                        "path":dimension.path,"field":field,"kind":if field=="override" {"boolean"}else if dimension.kind==configuration::dimensions::FieldKind::Count {"count"}else{"expression"},
                        "json":value,"editable":editable,"expression":true,"base_expression":dimension.expression.0,
                        "current_pointer":dimension.pointer,"positional":dimension.positional,"value_kind":dimension.kind,"maximum":dimension.maximum,"integer_literal_required":false,
                        "scalar_count_expression":dimension.kind==configuration::dimensions::FieldKind::Count,
                        "maximum_expression_count":configuration::dimensions::MAX_EXPRESSION_COUNT,
                        "resolved_count":count.as_ref().and_then(|result|result.as_ref().ok()),
                        "count_error":count.as_ref().and_then(|result|result.as_ref().err()).map(ToString::to_string),
                        "accessibility_label":format!("{} {} {}",c.name,dimension.label,field)}),
                });
            }
        }
    }
    out
}

fn dimension_identity(
    element: &cad_core::presentation::SemanticElement,
) -> Option<(&str, &str, &str, &str, &str)> {
    Some((
        element.data.get("owner")?.as_str()?,
        element.data.get("configuration")?.as_str()?,
        element.data.get("feature")?.as_str()?,
        element.data.get("path")?.as_str()?,
        element.data.get("field")?.as_str()?,
    ))
}
pub fn current_value(
    w: &Workspace,
    element: &cad_core::presentation::SemanticElement,
) -> Option<serde_json::Value> {
    if element.role == "configuration-sketch-dimension-field" {
        return sketch_dimensions::current_value(w, element);
    }
    if element.role == "configuration-placement-field" {
        return poses::current_value(w, element);
    }
    if element.role == "configuration-weld-field" {
        let owner = element.data["owner"].as_str()?;
        let id = element.data["configuration"].as_str()?;
        let weld = element.data["weld"].as_str()?;
        let configuration = source(w)
            .ok()
            .filter(|configuration| {
                configuration.id == id && self::owner(w).as_deref() == Some(owner)
            })
            .or_else(|| configuration::definitions(w.session.visible(), owner).remove(id))?;
        return Some(serde_json::json!(
            configuration.overrides.assembly_welds.get(weld)
        ));
    }
    let (owner, id, feature, path, field) = dimension_identity(element)?;
    let c = source(w)
        .ok()
        .filter(|c| c.id == id && self::owner(w).as_deref() == Some(owner))
        .or_else(|| configuration::definitions(w.session.visible(), owner).remove(id))?;
    let explicit = c
        .overrides
        .feature_dimensions
        .get(feature)
        .and_then(|fields| fields.get(path));
    match field {
        "override" => Some(serde_json::json!(explicit.is_some())),
        "expression" => {
            let fields = configuration::dimension_fields(w.session.visible(), owner).ok()?;
            let base = fields
                .iter()
                .find(|field| field.feature == feature && field.path == path)?;
            let resolved = if base.kind == configuration::dimensions::FieldKind::Count {
                configuration::counts::preview_context(w.session.visible(), owner, &c)
                    .and_then(|context| configuration::resolve(&context, owner, id))
                    .ok()
            } else {
                configuration::resolve(w.session.visible(), owner, id).ok()
            };
            Some(serde_json::json!(
                explicit
                    .or_else(|| resolved
                        .as_ref()
                        .and_then(|r| r.overrides.feature_dimensions.get(feature))
                        .and_then(|fields| fields.get(path)))
                    .unwrap_or(&base.expression)
                    .0
            ))
        }
        _ => None,
    }
}
pub fn set_value(
    w: &mut Workspace,
    element: &cad_core::presentation::SemanticElement,
    value: serde_json::Value,
) -> cad_core::Result<()> {
    if element.role == "configuration-sketch-dimension-field" {
        return sketch_dimensions::set_value(w, element, value);
    }
    if element.role == "configuration-placement-field" {
        return poses::set_value(w, element, value);
    }
    if element.role == "configuration-weld-field" {
        let invalid = |message: &str| cad_core::CadError::Invalid(message.into());
        let owner = element.data["owner"]
            .as_str()
            .ok_or_else(|| invalid("Missing weld configuration owner"))?;
        let id = element.data["configuration"]
            .as_str()
            .ok_or_else(|| invalid("Missing weld configuration identity"))?;
        let weld = element.data["weld"]
            .as_str()
            .ok_or_else(|| invalid("Missing weld identity"))?;
        if w.dialog.as_deref() != Some("configurations")
            || self::owner(w).as_deref() != Some(owner)
            || w.tool_checkpoint.is_none()
        {
            return Err(invalid(
                "Open the owning assembly's Configurations editor before changing weld suppression",
            ));
        }
        if !w
            .session
            .visible()
            .assemblies
            .get(owner)
            .is_some_and(|assembly| assembly.weld_beads.contains_key(weld))
        {
            return Err(invalid("The assembly weld is no longer present"));
        }
        let mut configuration =
            if w.fields.get("configuration-editor").map(String::as_str) == Some(id) {
                source(w)?
            } else {
                if !current_source_is_valid(w) {
                    return Err(invalid(
                        "Correct the current configuration source before editing another row",
                    ));
                }
                configuration::definitions(w.session.visible(), owner)
                    .remove(id)
                    .ok_or_else(|| invalid("This configuration no longer exists"))?
            };
        if value.is_null() {
            configuration.overrides.assembly_welds.remove(weld);
        } else {
            configuration.overrides.assembly_welds.insert(
                weld.into(),
                value.as_bool().ok_or_else(|| {
                    invalid("Weld suppression requires an inherited, present or suppressed choice")
                })?,
            );
        }
        configuration.version = configuration::CONTRACT_VERSION;
        w.fields.insert("configuration-editor".into(), id.into());
        w.json = serde_json::to_string_pretty(&configuration)?;
        submit(w, owner, configuration.clone());
        if configuration::definitions(w.session.visible(), owner).get(id) != Some(&configuration) {
            return Err(invalid(&w.status));
        }
        return Ok(());
    }
    let (owner, id, feature, path, field) = dimension_identity(element).ok_or_else(|| {
        cad_core::CadError::Invalid("Missing configuration dimension identity".into())
    })?;
    if w.dialog.as_deref() != Some("configurations")
        || self::owner(w).as_deref() != Some(owner)
        || w.tool_checkpoint.is_none()
    {
        return Err(cad_core::CadError::Invalid(
            "Open this definition's Configurations editor before editing dimensions".into(),
        ));
    }
    let fields = configuration::dimension_fields(w.session.visible(), owner)?;
    let base = fields
        .iter()
        .find(|candidate| candidate.feature == feature && candidate.path == path)
        .ok_or_else(|| {
            cad_core::CadError::Invalid("This feature dimension no longer exists".into())
        })?;
    let mut c = if w.fields.get("configuration-editor").map(String::as_str) == Some(id) {
        source(w)?
    } else {
        if !current_source_is_valid(w) {
            return Err(cad_core::CadError::Invalid(
                "Correct the current configuration source before editing another row".into(),
            ));
        }
        configuration::definitions(w.session.visible(), owner)
            .remove(id)
            .ok_or_else(|| {
                cad_core::CadError::Invalid("This configuration no longer exists".into())
            })?
    };
    match field {
        "override" => {
            let enabled = value.as_bool().ok_or_else(|| {
                cad_core::CadError::Invalid("A dimension override requires a boolean".into())
            })?;
            if enabled {
                let resolved = configuration::resolve(w.session.visible(), owner, id)?;
                let expression = resolved
                    .overrides
                    .feature_dimensions
                    .get(feature)
                    .and_then(|fields| fields.get(path))
                    .unwrap_or(&base.expression)
                    .clone();
                c.overrides
                    .feature_dimensions
                    .entry(feature.into())
                    .or_default()
                    .entry(path.into())
                    .or_insert(expression);
            } else if let Some(fields) = c.overrides.feature_dimensions.get_mut(feature) {
                fields.remove(path);
                if fields.is_empty() {
                    c.overrides.feature_dimensions.remove(feature);
                }
            }
        }
        "expression" => {
            let expression = value.as_str().ok_or_else(|| {
                cad_core::CadError::Invalid("A dimension requires expression text".into())
            })?;
            let target = c
                .overrides
                .feature_dimensions
                .get_mut(feature)
                .and_then(|fields| fields.get_mut(path))
                .ok_or_else(|| {
                    cad_core::CadError::Invalid(
                        "Enable this dimension override before editing it".into(),
                    )
                })?;
            target.0 = expression.into();
        }
        _ => {
            return Err(cad_core::CadError::Invalid(
                "Unknown configuration dimension field".into(),
            ));
        }
    }
    c.version = configuration::CONTRACT_VERSION;
    w.fields.insert("configuration-editor".into(), c.id.clone());
    w.json = serde_json::to_string_pretty(&c)?;
    submit(w, owner, c.clone());
    if configuration::definitions(w.session.visible(), owner).get(id) != Some(&c) {
        return Err(cad_core::CadError::Invalid(w.status.clone()));
    }
    Ok(())
}
pub fn field_error(
    w: &mut Workspace,
    element: &cad_core::presentation::SemanticElement,
    error: Option<String>,
) {
    poses::field_error(w, element, error);
}
pub fn field_is_invalid(w: &Workspace, element: &cad_core::presentation::SemanticElement) -> bool {
    poses::field_is_invalid(w, element)
}
