//! Docked controls over saved drawing tables and associative BOM row references.
use crate::Workspace;
use bevy_egui::egui;
use cad_core::{
    Drawing,
    drawing::{Anchor, AnnotationKind},
    drawing_tables::*,
};

fn evaluation(w: &Workspace) -> Option<&cad_core::protocol::Evaluation> {
    w.session
        .draft
        .as_ref()
        .and_then(|d| d.preview_evaluation())
        .or(w.session.evaluation.as_ref())
}
fn scoped(w: &Workspace, sheet: &Drawing) -> Option<cad_core::Document> {
    evaluation(w).map(|e| cad_core::drawing::scoped_document(w.session.visible(), e, sheet))
}
pub fn default_balloon(
    w: &Workspace,
    sheet: &Drawing,
    target: Anchor,
) -> cad_core::Result<AnnotationKind> {
    let table = sheet
        .tables
        .iter()
        .find(|t| matches!(t.kind, TableKind::BillOfMaterials))
        .ok_or_else(|| cad_core::CadError::Invalid("Add a BOM table to this sheet first".into()))?;
    let evaluation = evaluation(w).ok_or_else(|| {
        cad_core::CadError::Invalid("Wait for the current model evaluation".into())
    })?;
    let doc = cad_core::drawing::scoped_document(w.session.visible(), evaluation, sheet);
    let rows = content(&doc, evaluation, table)?.items;
    let row = w
        .selected
        .as_ref()
        .and_then(|s| rows.iter().find(|r| r.definition == s.definition))
        .or_else(|| rows.first())
        .ok_or_else(|| {
            cad_core::CadError::Invalid("This drawing scope has no included BOM items".into())
        })?;
    Ok(AnnotationKind::Balloon {
        target,
        table: table.id.clone(),
        definition: row.definition.clone(),
        configuration: row.configuration.clone(),
        show_quantity: false,
    })
}
pub struct BalloonFields<'a> {
    pub table_id: &'a mut String,
    pub definition: &'a mut String,
    pub configuration: &'a mut Option<String>,
    pub show_quantity: &'a mut bool,
}
fn represented_parts(w: &Workspace, sheet: &Drawing) -> Vec<String> {
    let Some(doc) = scoped(w, sheet) else {
        return vec![];
    };
    doc.placed_parts()
        .unwrap_or_default()
        .into_iter()
        .map(|i| i.definition)
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect()
}
fn hole_candidates(w: &Workspace, sheet: &Drawing) -> Vec<(cad_core::NodeRef, String)> {
    let represented = represented_parts(w, sheet);
    w.session
        .visible()
        .parts
        .values()
        .filter(|p| represented.contains(&p.id))
        .flat_map(|p| {
            p.features
                .iter()
                .filter(|f| {
                    matches!(
                        f.operation,
                        cad_core::Operation::Hole { .. } | cad_core::Operation::DrilledHoles { .. }
                    )
                })
                .map(|f| (p.reference(&f.id), format!("{} / {}", p.name, f.name)))
        })
        .collect()
}
fn configuration_fields(
    ui: &mut egui::Ui,
    w: &Workspace,
    scope: &str,
    configuration: &mut Option<String>,
    label: &str,
) -> bool {
    let mut changed = false;
    egui::ComboBox::from_label(label)
        .selected_text(
            configuration
                .as_ref()
                .and_then(|id| {
                    cad_core::configuration::names(w.session.visible(), scope)
                        .get(id)
                        .cloned()
                })
                .unwrap_or("Represented / current".into()),
        )
        .show_ui(ui, |ui| {
            changed |= ui
                .selectable_value(configuration, None, "Represented / current")
                .changed();
            for (id, c) in cad_core::configuration::definitions(w.session.visible(), scope) {
                changed |= ui
                    .selectable_value(configuration, Some(id), c.name)
                    .changed();
            }
        });
    changed
}
fn quantity_fields(
    ui: &mut egui::Ui,
    units: &mut cad_core::drawing::Units,
    precision: &mut usize,
) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        changed |= ui
            .selectable_value(units, cad_core::drawing::Units::Millimeters, "mm")
            .changed();
        changed |= ui
            .selectable_value(units, cad_core::drawing::Units::Inches, "in")
            .changed();
        changed |= ui
            .add(
                egui::DragValue::new(precision)
                    .range(0..=8)
                    .prefix("Precision "),
            )
            .changed();
    });
    changed
}
fn hole_fields(ui: &mut egui::Ui, w: &Workspace, sheet: &Drawing, kind: &mut TableKind) -> bool {
    let TableKind::Holes {
        features,
        configuration,
        instance_path,
        view,
        datum,
        units,
        precision,
        tag_prefix,
    } = kind
    else {
        return false;
    };
    let mut changed = false;
    let candidates = hole_candidates(w, sheet);
    ui.horizontal(|ui| {
        ui.label("Tag prefix");
        changed |= ui.text_edit_singleline(tag_prefix).changed();
    });
    let mut owner = features
        .first()
        .map(|f| f.definition.clone())
        .unwrap_or_default();
    let previous = owner.clone();
    egui::ComboBox::from_label("Hole model")
        .selected_text(
            w.session
                .visible()
                .parts
                .get(&owner)
                .map(|p| p.name.as_str())
                .unwrap_or("Select part"),
        )
        .show_ui(ui, |ui| {
            for id in represented_parts(w, sheet)
                .into_iter()
                .filter(|id| candidates.iter().any(|(f, _)| &f.definition == id))
            {
                if let Some(part) = w.session.visible().parts.get(&id) {
                    changed |= ui.selectable_value(&mut owner, id, &part.name).changed();
                }
            }
        });
    if previous != owner {
        features.clear();
        features.extend(
            candidates
                .iter()
                .filter(|(f, _)| f.definition == owner)
                .take(1)
                .map(|(f, _)| f.clone()),
        );
        instance_path.clear();
        *configuration = None;
        *datum = HoleDatum::ModelOrigin;
    }
    for (source, label) in candidates.iter().filter(|(f, _)| f.definition == owner) {
        let mut included = features.contains(source);
        if ui
            .add_enabled(
                !included || features.len() > 1,
                egui::Checkbox::new(&mut included, label),
            )
            .changed()
        {
            if included {
                features.push(source.clone());
            } else {
                features.retain(|f| f != source);
            }
            changed = true;
        }
    }
    if let Some(doc) = scoped(w, sheet) {
        let occurrences = doc
            .placed_parts()
            .unwrap_or_default()
            .into_iter()
            .filter(|i| i.definition == owner)
            .collect::<Vec<_>>();
        egui::ComboBox::from_label("Hole occurrence")
            .selected_text(
                occurrences
                    .iter()
                    .find(|i| &i.path == instance_path)
                    .map(|i| i.name.as_str())
                    .unwrap_or("Resolve unique occurrence"),
            )
            .show_ui(ui, |ui| {
                changed |= ui
                    .selectable_value(instance_path, vec![], "Resolve unique occurrence")
                    .changed();
                for occurrence in occurrences {
                    changed |= ui
                        .selectable_value(instance_path, occurrence.path, occurrence.name)
                        .changed();
                }
            });
    }
    changed |= configuration_fields(ui, w, &owner, configuration, "Hole configuration");
    egui::ComboBox::from_label("Coordinate view")
        .selected_text(view.as_str())
        .show_ui(ui, |ui| {
            for key in cad_core::drawing::view_keys(sheet)
                .into_iter()
                .filter(|k| matches!(k.as_str(), "front" | "top" | "right"))
            {
                changed |= ui.selectable_value(view, key.clone(), key).changed();
            }
        });
    if ui
        .selectable_label(
            matches!(datum, HoleDatum::ModelOrigin),
            "Datum: part origin",
        )
        .clicked()
        && !matches!(datum, HoleDatum::ModelOrigin)
    {
        *datum = HoleDatum::ModelOrigin;
        changed = true;
    }
    if let Some(reference) = crate::drawing_ui::selected_anchor(w).reference
        && ui
            .button("Use selected model point / face origin as datum")
            .clicked()
    {
        *datum = HoleDatum::Reference {
            point: reference.into(),
        };
        changed = true;
    }
    egui::ComboBox::from_id_salt("hole-table-sketch-datum")
        .selected_text(match datum {
            HoleDatum::ModelOrigin => "Select sketch datum point".into(),
            HoleDatum::Reference {point} => match point.as_ref() {
                cad_core::drawing::PointReference::SketchPoint { sketch, point, .. } => format!("{} / {point}", sketch.node),
                _ => "Selected model datum".into(),
            },
        })
        .show_ui(ui, |ui| {
            if let Some(part) = w.session.visible().parts.get(&owner) {
                for sketch in part.sketches.values() {
                    let inactive = cad_core::sketch_transform::suppressed_points(sketch);
                    for point in sketch.points.keys().filter(|id| !inactive.contains(*id)) {
                        let reference = cad_core::drawing::PointReference::SketchPoint {
                            sketch: part.reference(&sketch.id),
                            point: point.clone(),
                            instance_path: instance_path.clone(),
                        };
                        let selected =
                            matches!(datum,HoleDatum::Reference {point:p} if p.as_ref()==&reference);
                        if ui
                            .selectable_label(selected, format!("{} / {point}", sketch.name))
                            .clicked()
                            && !selected
                        {
                            *datum = HoleDatum::Reference { point: reference.into() };
                            changed = true;
                        }
                    }
                }
            }
        });
    changed |= quantity_fields(ui, units, precision);
    ui.small("Designed hole centers; axes must be normal to the coordinate view. X/Y are model distances from the native datum, independent of paper scale. H tags share saved row numbers.");
    changed
}
fn cut_list_fields(
    ui: &mut egui::Ui,
    w: &Workspace,
    sheet: &Drawing,
    kind: &mut TableKind,
) -> bool {
    let TableKind::WeldmentCutList {
        scope,
        configuration,
        sources,
        units,
        precision,
    } = kind
    else {
        return false;
    };
    let mut changed = false;
    let previous = scope.clone();
    let mut scopes = represented_parts(w, sheet);
    scopes.push(sheet.definition.clone());
    scopes.sort();
    scopes.dedup();
    egui::ComboBox::from_label("Cut-list model scope")
        .selected_text(
            w.session
                .visible()
                .parts
                .get(scope)
                .map(|p| p.name.as_str())
                .or_else(|| {
                    w.session
                        .visible()
                        .assemblies
                        .get(scope)
                        .map(|a| a.name.as_str())
                })
                .unwrap_or("Select model"),
        )
        .show_ui(ui, |ui| {
            for id in scopes {
                let label = w
                    .session
                    .visible()
                    .parts
                    .get(&id)
                    .map(|p| p.name.clone())
                    .or_else(|| {
                        w.session
                            .visible()
                            .assemblies
                            .get(&id)
                            .map(|a| a.name.clone())
                    })
                    .unwrap_or_default();
                changed |= ui.selectable_value(scope, id, label).changed();
            }
        });
    if previous != *scope {
        *configuration = None;
        sources.clear();
    }
    changed |= configuration_fields(ui, w, scope, configuration, "Cut-list configuration");
    let all = sources.is_empty();
    let mut include_all = all;
    if ui
        .checkbox(&mut include_all, "All native weldment outputs in scope")
        .changed()
    {
        if include_all {
            sources.clear();
        } else {
            let represented = represented_parts(w, sheet);
            sources.extend(
                w.session
                    .visible()
                    .parts
                    .values()
                    .filter(|p| {
                        represented.contains(&p.id)
                            && (p.id == *scope || scope == &sheet.definition)
                    })
                    .flat_map(|p| {
                        p.features
                            .iter()
                            .filter(|f| {
                                p.outputs.contains(&f.id)
                                    && matches!(f.operation, cad_core::Operation::Weldment { .. })
                            })
                            .take(1)
                            .map(|f| p.reference(&f.id))
                    }),
            );
        }
        changed = true;
    }
    if !sources.is_empty() {
        let represented = represented_parts(w, sheet);
        for part in w.session.visible().parts.values().filter(|p| {
            represented.contains(&p.id) && (p.id == *scope || scope == &sheet.definition)
        }) {
            for feature in part
                .features
                .iter()
                .filter(|f| matches!(f.operation, cad_core::Operation::Weldment { .. }))
            {
                let reference = part.reference(&feature.id);
                let mut included = sources.contains(&reference);
                if ui
                    .add_enabled(
                        !included || sources.len() > 1,
                        egui::Checkbox::new(
                            &mut included,
                            format!("{} / {}", part.name, feature.name),
                        ),
                    )
                    .changed()
                {
                    if included {
                        sources.push(reference);
                    } else {
                        sources.retain(|s| s != &reference);
                    }
                    changed = true;
                }
            }
        }
    }
    changed |= quantity_fields(ui, units, precision);
    ui.small("Native member centerline length, resolved start/finish cut angles and configured material. Quantities follow included assembled occurrences. Caps, gussets, longest blank dimensions and equivalent-stock grouping are not listed.");
    changed
}
pub fn balloon_fields(
    ui: &mut egui::Ui,
    w: &Workspace,
    tables: &[Table],
    scope: &str,
    fields: BalloonFields<'_>,
) -> bool {
    let BalloonFields {
        table_id,
        definition,
        configuration,
        show_quantity,
    } = fields;
    let mut changed = false;
    egui::ComboBox::from_label("BOM Table")
        .selected_text(
            tables
                .iter()
                .find(|t| &t.id == table_id)
                .map(|t| t.title.as_str())
                .unwrap_or("Select BOM table"),
        )
        .show_ui(ui, |ui| {
            for table in tables
                .iter()
                .filter(|t| matches!(t.kind, TableKind::BillOfMaterials))
            {
                changed |= ui
                    .selectable_value(table_id, table.id.clone(), &table.title)
                    .changed();
            }
        });
    if let Some(table) = tables.iter().find(|t| &t.id == table_id)
        && let Some(e) = evaluation(w)
    {
        let mut doc = w.session.visible().clone();
        doc.root = scope.into();
        match content(&doc, e, table) {
            Ok(content) => {
                let selected = content
                    .items
                    .iter()
                    .find(|r| &r.definition == definition && &r.configuration == configuration)
                    .map(|r| format!("{} · {}", r.number, r.cells[1]))
                    .unwrap_or("Select BOM item".into());
                egui::ComboBox::from_label("Component / Configuration")
                    .selected_text(selected)
                    .show_ui(ui, |ui| {
                        for row in content.items {
                            let current = &row.definition == definition
                                && &row.configuration == configuration;
                            if ui
                                .selectable_label(
                                    current,
                                    format!(
                                        "{} · {} ({})",
                                        row.number,
                                        row.cells[1],
                                        row.configuration.as_deref().unwrap_or("Default")
                                    ),
                                )
                                .clicked()
                                && !current
                            {
                                *definition = row.definition;
                                *configuration = row.configuration;
                                changed = true;
                            }
                        }
                    });
            }
            Err(error) => {
                ui.colored_label(ui.visuals().error_fg_color, error.to_string());
            }
        }
    }
    changed |= ui.checkbox(show_quantity, "Show Quantity").changed();
    changed
}
pub fn editor(ui: &mut egui::Ui, w: &Workspace, sheet: &mut Drawing) -> bool {
    let mut changed = false;
    let doc = scoped(w, sheet);
    ui.menu_button("Add Table", |ui| {
        for (index, label) in [
            (0, "Bill of Materials"),
            (1, "Revision Table"),
            (2, "Bend Table"),
            (3, "General Table"),
            (4, "Hole Table"),
            (5, "Weldment Cut List"),
        ] {
            if ui.button(label).clicked() {
                let kind = match index {
                    1 => TableKind::Revisions { rows: vec![] },
                    2 => {
                        let source = w
                            .session
                            .visible()
                            .parts
                            .values()
                            .flat_map(|p| {
                                p.outputs
                                    .iter()
                                    .filter_map(|id| {
                                        p.features.iter().find(|f| {
                                            &f.id == id
                                                && !f.suppressed
                                                && matches!(
                                                    f.operation,
                                                    cad_core::Operation::Sheet { .. }
                                                )
                                        })
                                    })
                                    .map(|f| p.reference(&f.id))
                            })
                            .next();
                        let Some(source) = source else {
                            ui.label("Create sketch-based sheet stock first");
                            continue;
                        };
                        TableKind::Bends {
                            source,
                            configuration: None,
                        }
                    }
                    3 => TableKind::General {
                        headers: vec!["Property".into(), "Value".into()],
                        rows: vec![],
                    },
                    4 => {
                        let candidates=hole_candidates(w,sheet);
                        let selected=w.selected.as_ref().and_then(|s|candidates.iter().find(|(f,_)|f==&s.reference.producer));
                        let Some((feature,_))=selected.or_else(||candidates.first()) else {ui.label("Create a designed Hole or DrilledHoles feature in this sheet scope first");continue;};
                        let view=cad_core::drawing::view_keys(sheet).into_iter().find(|v|matches!(v.as_str(),"front"|"top"|"right")).unwrap_or("front".into());
                        let mut prefix="A".to_string();let mut index=1usize;
                        while sheet.tables.iter().any(|t|matches!(&t.kind,TableKind::Holes {tag_prefix,..} if tag_prefix==&prefix)) {index+=1;prefix=format!("H{index}_");}
                        TableKind::Holes {tag_prefix:prefix,features:vec![feature.clone()],configuration:None,instance_path:vec![],view,datum:HoleDatum::ModelOrigin,units:cad_core::drawing::Units::Millimeters,precision:3}
                    }
                    5 => TableKind::WeldmentCutList {scope:sheet.definition.clone(),configuration:None,sources:vec![],units:cad_core::drawing::Units::Millimeters,precision:3},
                    _ => TableKind::BillOfMaterials,
                };
                let mut table = Table::new(kind);
                table.position_mm = cad_core::drawing::default_table_position(sheet);
                if matches!(table.kind,TableKind::Holes {..}) && let Some(e)=evaluation(w) {
                    for view in cad_core::drawing::view_keys(sheet).into_iter().filter(|v|matches!(v.as_str(),"front"|"top"|"right")) {
                        if let TableKind::Holes {view:key,..}=&mut table.kind {*key=view;}
                        if content_for_sheet(w.session.visible(),e,sheet,&table).is_ok(){break;}
                    }
                }
                if let Some(e) = evaluation(w)
                    && let Err(error) = pin_source_rows(w.session.visible(), e, sheet, &mut table)
                {
                    ui.colored_label(ui.visuals().error_fg_color, error.to_string());
                }
                sheet.tables.push(table);
                changed = true;
                ui.close();
            }
        }
    });
    let context = sheet.clone();
    let mut remove = None;
    for (index, table) in sheet.tables.iter_mut().enumerate() {
        ui.push_id(table.id.clone(), |ui| {
            egui::CollapsingHeader::new(&table.title).default_open(w.fields.get("drawing-table")==Some(&table.id) || w.fields.get("drawing-item")==Some(&table.id)).show(ui, |ui| {
                ui.label(table.kind.name());
                changed |= ui.text_edit_singleline(&mut table.title).changed();
                ui.horizontal(|ui| {
                    for (label, value) in ["Paper X", "Paper Y"]
                        .into_iter()
                        .zip(&mut table.position_mm)
                    {
                        changed |= ui
                            .add(
                                egui::DragValue::new(value)
                                    .prefix(format!("{label} "))
                                    .suffix(" mm")
                                    .speed(0.2),
                            )
                            .changed();
                    }
                });
                changed |= ui
                    .checkbox(&mut table.reserve_space, "Reserve Space Below Views")
                    .changed();
                changed |= ui
                    .add(
                        egui::DragValue::new(&mut table.row_height_mm)
                            .prefix("Row height ")
                            .suffix(" mm")
                            .range(3.0..=100.0)
                            .speed(0.2),
                    )
                    .changed();
                changed |= ui
                    .add(
                        egui::DragValue::new(&mut table.text_size_mm)
                            .prefix("Text size ")
                            .suffix(" mm")
                            .range(0.5..=20.0)
                            .speed(0.1),
                    )
                    .changed();
                egui::CollapsingHeader::new("Column Widths").show(ui, |ui| {
                    for (index, value) in table.column_widths_mm.iter_mut().enumerate() {
                        changed |= ui
                            .add(
                                egui::DragValue::new(value)
                                    .prefix(format!("Column {} ", index + 1))
                                    .suffix(" mm")
                                    .range(4.0..=1000.0)
                                    .speed(0.5),
                            )
                            .changed();
                    }
                });
                match &mut table.kind {
                    TableKind::BillOfMaterials => {}
                    TableKind::Revisions { rows } => {
                        let mut row_remove = None;
                        for (index, row) in rows.iter_mut().enumerate() {
                            ui.push_id(row.id.clone(), |ui| {
                                ui.horizontal(|ui| {
                                    ui.label("Revision");
                                    changed |= ui.text_edit_singleline(&mut row.revision).changed();
                                    if ui.small_button("Remove Row").clicked() {
                                        row_remove = Some(index);
                                    }
                                });
                                ui.label("Description");
                                changed |= ui.text_edit_multiline(&mut row.description).changed();
                                ui.horizontal(|ui| {
                                    ui.label("Date");
                                    changed |= ui.text_edit_singleline(&mut row.date).changed();
                                    ui.label("Approved by");
                                    changed |=
                                        ui.text_edit_singleline(&mut row.approved_by).changed();
                                });
                            });
                        }
                        if let Some(index) = row_remove {
                            rows.remove(index);
                            changed = true;
                        }
                        if ui.button("Add Revision Row").clicked() {
                            let mut next = rows.len() + 1;
                            while rows.iter().any(|r| r.revision == next.to_string()) {
                                next += 1;
                            }
                            rows.push(RevisionRow {
                                id: cad_core::id(),
                                revision: next.to_string(),
                                description: String::new(),
                                date: String::new(),
                                approved_by: String::new(),
                            });
                            changed = true;
                        }
                    }
                    TableKind::Bends {
                        source,
                        configuration,
                    } => {
                        let candidates = w
                            .session
                            .visible()
                            .parts
                            .values()
                            .flat_map(|p| {
                                p.features
                                    .iter()
                                    .filter(|f| {
                                        !f.suppressed
                                            && matches!(
                                                f.operation,
                                                cad_core::Operation::Sheet { .. }
                                            )
                                    })
                                    .map(|f| {
                                        (p.reference(&f.id), format!("{} / {}", p.name, f.name))
                                    })
                            })
                            .collect::<Vec<_>>();
                        egui::ComboBox::from_label("Sheet Feature")
                            .selected_text(
                                candidates
                                    .iter()
                                    .find(|(r, _)| r == source)
                                    .map(|(_, label)| label.as_str())
                                    .unwrap_or("Select sheet feature"),
                            )
                            .show_ui(ui, |ui| {
                                for (reference, label) in candidates {
                                    changed |=
                                        ui.selectable_value(source, reference, label).changed();
                                }
                            });
                        egui::ComboBox::from_label("Sheet Configuration")
                            .selected_text(configuration.as_deref().unwrap_or("Active / Default"))
                            .show_ui(ui, |ui| {
                                changed |= ui
                                    .selectable_value(configuration, None, "Active / Default")
                                    .changed();
                                for (id, c) in cad_core::configuration::definitions(
                                    w.session.visible(),
                                    &source.definition,
                                ) {
                                    changed |= ui
                                        .selectable_value(configuration, Some(id), c.name)
                                        .changed();
                                }
                            });
                    }
                    TableKind::General { headers, rows } => {
                        for header in headers.iter_mut() {
                            changed |= ui.text_edit_singleline(header).changed();
                        }
                        let mut row_remove = None;
                        for (index, row) in rows.iter_mut().enumerate() {
                            ui.push_id(index, |ui| {
                                ui.horizontal(|ui| {
                                    for cell in row {
                                        changed |= ui.text_edit_singleline(cell).changed();
                                    }
                                    if ui.small_button("×").clicked() {
                                        row_remove = Some(index);
                                    }
                                });
                            });
                        }
                        if let Some(index) = row_remove {
                            rows.remove(index);
                            changed = true;
                        }
                        ui.horizontal(|ui| {
                            if ui.button("Add Row").clicked() {
                                rows.push(vec![String::new(); headers.len()]);
                                changed = true;
                            }
                            if ui.button("Add Column").clicked() && headers.len() < 32 {
                                headers.push(format!("Column {}", headers.len() + 1));
                                table.column_widths_mm.push(35.);
                                for row in rows.iter_mut() {
                                    row.push(String::new());
                                }
                                changed = true;
                            }
                            if ui.button("Remove Last Column").clicked() && headers.len() > 1 {
                                headers.pop();
                                table.column_widths_mm.pop();
                                for row in rows.iter_mut() {
                                    row.pop();
                                }
                                changed = true;
                            }
                        });
                    }
                    TableKind::Holes {..}=> {changed|=hole_fields(ui,w,&context,&mut table.kind);}
                    TableKind::WeldmentCutList {..}=> {changed|=cut_list_fields(ui,w,&context,&mut table.kind);}
                }
                if matches!(table.kind, TableKind::BillOfMaterials)
                    && let (Some(doc), Some(e)) = (&doc, evaluation(w))
                {
                    let before = table.item_numbers.clone();
                    if pin_item_numbers(doc, e, table).is_ok() && before != table.item_numbers {
                        changed = true;
                    }
                    match content(doc, e, table) {
                        Ok(content) => {
                            for row in content.items {
                                ui.horizontal(|ui| {
                                    ui.label(&row.cells[1]);
                                    let key =
                                        item_key(&row.definition, row.configuration.as_deref());
                                    if let Some(number) = table.item_numbers.get_mut(&key) {
                                        changed |= ui
                                            .add(
                                                egui::DragValue::new(number)
                                                    .range(1..=u32::MAX)
                                                    .prefix("Item "),
                                            )
                                            .changed();
                                    }
                                    ui.label(format!("Qty {}", row.quantity));
                                });
                            }
                        }
                        Err(error) => {
                            ui.colored_label(ui.visuals().error_fg_color, error.to_string());
                        }
                    }
                }
                if matches!(table.kind,TableKind::Holes {..}|TableKind::WeldmentCutList {..})
                    && let Some(e)=evaluation(w)
                {
                    let before=(table.item_numbers.clone(),table.row_order.clone());
                    match pin_source_rows(w.session.visible(),e,&context,table) {
                        Ok(())=> {if before!=(table.item_numbers.clone(),table.row_order.clone()){changed=true;}},
                        Err(error)=> {ui.colored_label(ui.visuals().error_fg_color,error.to_string());},
                    }
                    match content_for_sheet(w.session.visible(),e,&context,table) {
                        Ok(content)=> {
                            for (index,row) in content.entries.iter().enumerate() {
                                ui.push_id(&row.key,|ui| {ui.horizontal_wrapped(|ui| {
                                    ui.label(row.cells.get(1).map(String::as_str).unwrap_or("Row"));
                                    if let Some(number)=table.item_numbers.get_mut(&row.key) {changed|=ui.add(egui::DragValue::new(number).range(1..=u32::MAX).prefix("Item ")).changed();}
                                    for (label,neighbor) in [("↑",index.checked_sub(1)),("↓",(index+1<content.entries.len()).then_some(index+1))] {
                                        if ui.add_enabled(neighbor.is_some(),egui::Button::new(label).small()).clicked() && let Some(neighbor)=neighbor {
                                            let first=table.row_order.iter().position(|key|key==&row.key);let second=table.row_order.iter().position(|key|key==&content.entries[neighbor].key);
                                            if let (Some(first),Some(second))=(first,second){table.row_order.swap(first,second);changed=true;}
                                        }
                                    }
                                });ui.small(row.cells.join(" · "));});
                            }
                            ui.small("Absent source rows keep their reserved numbers and order. Changing the model or configuration never recycles a saved row number.");
                        }
                        Err(error)=> {ui.colored_label(ui.visuals().error_fg_color,error.to_string());},
                    }
                }
                if ui.small_button("Delete Table").clicked() {
                    remove = Some(index);
                }
            });
        });
    }
    if let Some(index) = remove {
        sheet.tables.remove(index);
        changed = true;
    }
    changed
}
