//! Compact drawing controls and the same vector sheet used by PDF/DXF export.
use crate::Workspace;
use bevy_egui::egui::{self, Color32, Stroke};
use cad_core::{Drawing, DrawingDimension, Expr, commands::Command, drawing::*};

fn evaluation(w: &Workspace) -> Option<&cad_core::protocol::Evaluation> {
    w.session
        .draft
        .as_ref()
        .and_then(|d| d.preview_evaluation())
        .or(w.session.evaluation.as_ref())
}
fn selected_path(w: &Workspace) -> Vec<String> {
    let Some(selected) = &w.selected else {
        return vec![];
    };
    let paths = evaluation(w)
        .map(|e| {
            e.instances
                .iter()
                .filter(|i| {
                    i.definition == selected.definition
                        && i.path.last().is_some_and(|id| id == &selected.instance)
                })
                .map(|i| i.path.clone())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if paths.len() == 1 {
        paths[0].clone()
    } else {
        vec![]
    }
}
fn selected_curve(w: &Workspace) -> Option<CurveReference> {
    w.selected
        .as_ref()
        .filter(|s| s.reference.topology == "edge")
        .map(|s| CurveReference::ModelEdge {
            edge: s.reference.clone(),
            body: s.body.clone(),
            instance_path: selected_path(w),
        })
}
pub(crate) fn selected_anchor(w: &Workspace) -> Anchor {
    let reference =
        w.selected
            .as_ref()
            .and_then(|selected| match selected.reference.topology.as_str() {
                "edge" => Some(PointReference::EdgePoint {
                    edge: selected.reference.clone(),
                    body: selected.body.clone(),
                    parameter: 0.,
                    instance_path: selected_path(w),
                }),
                "face" => Some(PointReference::FaceOrigin {
                    face: selected.reference.clone(),
                    body: selected.body.clone(),
                    instance_path: selected_path(w),
                }),
                _ => None,
            });
    Anchor {
        point: w
            .selected
            .as_ref()
            .map(|selected| selected.point)
            .unwrap_or([0.; 3]),
        reference,
    }
}
fn reference_choices(w: &Workspace) -> Vec<(PointReference, String)> {
    let mut out = Vec::new();
    let Some(e) = evaluation(w) else {
        return out;
    };
    let doc = w.session.visible();
    for instance in &e.instances {
        let Some(part) = doc.parts.get(&instance.definition) else {
            continue;
        };
        let key = if instance.mesh_key.is_empty() {
            &instance.definition
        } else {
            &instance.mesh_key
        };
        for feature in &part.features {
            let Some(geometry) = e.sketches.get(&format!("{key}::{}", feature.id)) else {
                continue;
            };
            for (point, p) in &geometry.points {
                out.push((
                    PointReference::SketchPoint {
                        sketch: part.reference(&feature.id),
                        point: point.clone(),
                        instance_path: instance.path.clone(),
                    },
                    format!(
                        "{} / {} ({:.2}, {:.2})",
                        instance.name, feature.name, p[0], p[1]
                    ),
                ));
            }
        }
    }
    out
}
fn point_fields(
    ui: &mut egui::Ui,
    w: &Workspace,
    label: &str,
    p: &mut [f64; 3],
    reference: &mut Option<PointReference>,
) -> bool {
    let mut changed = false;
    ui.push_id(label, |ui| {
        ui.label(label);
        let choices = reference_choices(w);
        let value = reference
            .as_ref()
            .and_then(|r| choices.iter().find(|(choice, _)| choice == r))
            .map(|(_, name)| name.as_str())
            .unwrap_or(if reference.is_some() {
                "Model reference"
            } else {
                "Explicit coordinates"
            });
        egui::ComboBox::from_id_salt("point-ref")
            .selected_text(value)
            .show_ui(ui, |ui| {
                changed |= ui
                    .selectable_value(reference, None, "Explicit coordinates")
                    .changed();
                for (source, name) in choices {
                    changed |= ui.selectable_value(reference, Some(source), name).changed();
                }
            });
        if let Some(selected) = &w.selected {
            if selected.reference.topology == "edge" {
                ui.horizontal(|ui| {
                    for (parameter, label) in
                        [(0., "Selected edge start"), (1., "Selected edge end")]
                    {
                        if ui.small_button(label).clicked() {
                            *reference = Some(PointReference::EdgePoint {
                                edge: selected.reference.clone(),
                                body: selected.body.clone(),
                                parameter,
                                instance_path: selected_path(w),
                            });
                            changed = true;
                        }
                    }
                });
            }
            if selected.reference.topology == "face"
                && ui.small_button("Selected face plane origin").clicked()
            {
                *reference = Some(PointReference::FaceOrigin {
                    face: selected.reference.clone(),
                    body: selected.body.clone(),
                    instance_path: selected_path(w),
                });
                changed = true;
            }
        }
        if reference.is_none() {
            ui.horizontal(|ui| {
                for (label, value) in ["X", "Y", "Z"].into_iter().zip(p) {
                    changed |= ui
                        .add(
                            egui::DragValue::new(value)
                                .prefix(format!("{label} "))
                                .suffix(" mm")
                                .speed(0.1),
                        )
                        .changed();
                }
            });
        }
        if let Some(PointReference::EdgePoint { parameter, .. }) = reference {
            changed |= ui
                .add(
                    egui::DragValue::new(parameter)
                        .range(0.0..=1.0)
                        .prefix("Edge parameter ")
                        .speed(0.01),
                )
                .changed();
        }
        if let Some(reference) = reference {
            let (definition, path) = match reference {
                PointReference::SketchPoint {
                    sketch,
                    instance_path,
                    ..
                } => (&sketch.definition, instance_path),
                PointReference::EdgePoint {
                    edge,
                    instance_path,
                    ..
                } => (&edge.producer.definition, instance_path),
                PointReference::FaceOrigin {
                    face,
                    instance_path,
                    ..
                } => (&face.producer.definition, instance_path),
            };
            changed |= instance_fields(ui, w, definition, path);
        }
    });
    changed
}
fn instance_fields(
    ui: &mut egui::Ui,
    w: &Workspace,
    definition: &str,
    path: &mut Vec<String>,
) -> bool {
    let mut changed = false;
    if let Some(e) = evaluation(w) {
        let instances = e
            .instances
            .iter()
            .filter(|i| i.definition == definition)
            .collect::<Vec<_>>();
        if instances.len() > 1 {
            egui::ComboBox::from_id_salt(("annotation-instance", definition))
                .selected_text(
                    instances
                        .iter()
                        .find(|i| i.path == *path)
                        .map(|i| i.name.as_str())
                        .unwrap_or("Select component instance"),
                )
                .show_ui(ui, |ui| {
                    for instance in instances {
                        changed |= ui
                            .selectable_value(
                                path,
                                instance.path.clone(),
                                format!("{} ({})", instance.name, instance.path.join(" / ")),
                            )
                            .changed();
                    }
                });
        }
    }
    changed
}
fn curve_fields(ui: &mut egui::Ui, w: &Workspace, reference: &mut Option<CurveReference>) -> bool {
    let mut changed = false;
    let mut choices = Vec::new();
    if let Some(e) = evaluation(w) {
        for instance in &e.instances {
            if let Some(part) = w.session.visible().parts.get(&instance.definition) {
                let key = if instance.mesh_key.is_empty() {
                    &instance.definition
                } else {
                    &instance.mesh_key
                };
                for feature in &part.features {
                    if let Some(sketch) = e
                        .sketches
                        .get(&format!("{key}::{}", feature.id))
                        .and_then(|s| s.solved.as_ref())
                    {
                        for (id, m) in &sketch.members {
                            if matches!(
                                m.curve,
                                cad_core::SketchCurve::Circle { .. }
                                    | cad_core::SketchCurve::Arc { .. }
                            ) {
                                choices.push((
                                    CurveReference::SketchMember {
                                        sketch: part.reference(&feature.id),
                                        member: id.clone(),
                                        instance_path: instance.path.clone(),
                                    },
                                    format!(
                                        "{} / {} — {}",
                                        instance.name,
                                        feature.name,
                                        if matches!(m.curve, cad_core::SketchCurve::Circle { .. }) {
                                            "Circle"
                                        } else {
                                            "Arc"
                                        }
                                    ),
                                ));
                            }
                        }
                    }
                }
            }
        }
    }
    let label = reference
        .as_ref()
        .and_then(|r| choices.iter().find(|(c, _)| c == r))
        .map(|(_, name)| name.as_str())
        .unwrap_or(if reference.is_some() {
            "Selected circular model edge"
        } else {
            "Select circle or arc"
        });
    egui::ComboBox::from_id_salt("circular-source")
        .selected_text(label)
        .show_ui(ui, |ui| {
            for (r, label) in choices {
                changed |= ui.selectable_value(reference, Some(r), label).changed();
            }
        });
    if let Some(curve) = selected_curve(w)
        && ui.small_button("Use selected circular edge").clicked()
    {
        *reference = Some(curve);
        changed = true;
    }
    if let Some(reference) = reference {
        let (definition, path) = match reference {
            CurveReference::SketchMember {
                sketch,
                instance_path,
                ..
            } => (&sketch.definition, instance_path),
            CurveReference::ModelEdge {
                edge,
                instance_path,
                ..
            } => (&edge.producer.definition, instance_path),
        };
        changed |= instance_fields(ui, w, definition, path);
    }
    changed
}
fn expression(ui: &mut egui::Ui, label: &str, e: &mut Expr) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.label(label);
        changed = ui.text_edit_singleline(&mut e.0).changed();
    });
    changed
}
fn tolerance_fields(ui: &mut egui::Ui, d: &mut DrawingDimension) -> bool {
    let mut changed = false;
    let mut mode = match d.options.tolerance {
        Tolerance::None => 0,
        Tolerance::Symmetric { .. } => 1,
        Tolerance::Deviation { .. } => 2,
        Tolerance::Limits { .. } => 3,
        Tolerance::Basic => 4,
    };
    let old = mode;
    egui::ComboBox::from_label("Tolerance")
        .selected_text(
            [
                "None",
                "Symmetric ±",
                "Upper / Lower",
                "Limits",
                "Basic (boxed)",
            ][mode],
        )
        .show_ui(ui, |ui| {
            for (i, label) in [
                "None",
                "Symmetric ±",
                "Upper / Lower",
                "Limits",
                "Basic (boxed)",
            ]
            .into_iter()
            .enumerate()
            {
                changed |= ui.selectable_value(&mut mode, i, label).changed();
            }
        });
    if mode != old {
        let unit = if d.options.kind == DimensionKind::Angular {
            "deg"
        } else {
            "mm"
        };
        d.options.tolerance = match mode {
            1 => Tolerance::Symmetric {
                value: Expr(format!("0.1 {unit}")),
            },
            2 => Tolerance::Deviation {
                lower: Expr(format!("-0.1 {unit}")),
                upper: Expr(format!("0.1 {unit}")),
            },
            3 => Tolerance::Limits {
                lower: Expr(format!("9.9 {unit}")),
                upper: Expr(format!("10.1 {unit}")),
            },
            4 => Tolerance::Basic,
            _ => Tolerance::None,
        };
    }
    match &mut d.options.tolerance {
        Tolerance::Symmetric { value } => changed |= expression(ui, "± Tolerance", value),
        Tolerance::Deviation { lower, upper } => {
            changed |= expression(ui, "Upper Deviation", upper);
            changed |= expression(ui, "Lower Deviation", lower);
        }
        Tolerance::Limits { lower, upper } => {
            changed |= expression(ui, "Upper Limit", upper);
            changed |= expression(ui, "Lower Limit", lower);
        }
        _ => {}
    }
    changed
}
fn view_fields(ui: &mut egui::Ui, views: &[String], value: &mut String) -> bool {
    let mut changed = false;
    egui::ComboBox::from_label("View")
        .selected_text(value.as_str())
        .show_ui(ui, |ui| {
            for view in views {
                changed |= ui.selectable_value(value, view.clone(), view).changed();
            }
        });
    changed
}
fn dimension_fields(
    ui: &mut egui::Ui,
    w: &Workspace,
    views: &[String],
    d: &mut DrawingDimension,
) -> bool {
    let mut changed = view_fields(ui, views, &mut d.view);
    egui::ComboBox::from_label("Dimension Type")
        .selected_text(format!("{:?}", d.options.kind))
        .show_ui(ui, |ui| {
            for (kind, label) in [
                (DimensionKind::Aligned, "Aligned"),
                (DimensionKind::Horizontal, "Horizontal"),
                (DimensionKind::Vertical, "Vertical"),
                (DimensionKind::Angular, "Angular"),
                (DimensionKind::Diameter, "Diameter"),
                (DimensionKind::Radius, "Radius"),
                (DimensionKind::HoleCallout, "Hole Callout"),
            ] {
                changed |= ui
                    .selectable_value(&mut d.options.kind, kind, label)
                    .changed();
            }
        });
    if matches!(
        d.options.kind,
        DimensionKind::Diameter | DimensionKind::Radius | DimensionKind::HoleCallout
    ) {
        changed |= curve_fields(ui, w, &mut d.options.curve);
    } else {
        changed |= point_fields(
            ui,
            w,
            if d.options.kind == DimensionKind::Angular {
                "Angle Vertex"
            } else {
                "First Point"
            },
            &mut d.first,
            &mut d.options.first,
        );
        changed |= point_fields(
            ui,
            w,
            if d.options.kind == DimensionKind::Angular {
                "First Ray"
            } else {
                "Second Point"
            },
            &mut d.second,
            &mut d.options.second,
        );
    }
    if d.options.kind == DimensionKind::Angular {
        changed |= point_fields(
            ui,
            w,
            "Second Ray",
            &mut d.options.third_point,
            &mut d.options.third,
        );
    }
    if d.options.kind == DimensionKind::HoleCallout {
        ui.small("Select an exact circular rim from its designed hole. Callouts include configured count, bore/head dimensions and explicit cosmetic-thread diameter, pitch, hand and depth; threaded callouts draw a nominal thread arc.");
        let holes = w
            .session
            .visible()
            .parts
            .values()
            .flat_map(|p| {
                p.features
                    .iter()
                    .filter(move |f| {
                        !cad_core::configuration::feature_suppressed(w.session.visible(), &p.id, f)
                            && matches!(
                                f.operation,
                                cad_core::Operation::Hole { .. }
                                    | cad_core::Operation::DrilledHoles { .. }
                            )
                    })
                    .map(move |f| (p.reference(&f.id), format!("{} / {}", p.name, f.name)))
            })
            .collect::<Vec<_>>();
        egui::ComboBox::from_label("Hole Feature")
            .selected_text(
                d.options
                    .hole_feature
                    .as_ref()
                    .and_then(|r| holes.iter().find(|(h, _)| h == r))
                    .map(|(_, name)| name.as_str())
                    .unwrap_or("Select Hole feature"),
            )
            .show_ui(ui, |ui| {
                for (r, label) in holes {
                    changed |= ui
                        .selectable_value(&mut d.options.hole_feature, Some(r), label)
                        .changed();
                }
            });
    }
    ui.horizontal(|ui| {
        changed |= ui
            .add(
                egui::DragValue::new(&mut d.offset_mm)
                    .speed(0.2)
                    .suffix(" mm")
                    .prefix("Paper offset "),
            )
            .changed();
        changed |= ui
            .add(
                egui::DragValue::new(&mut d.options.precision)
                    .range(0..=8)
                    .prefix("Decimals "),
            )
            .changed();
    });
    egui::ComboBox::from_label("Units")
        .selected_text(if d.options.units == Units::Millimeters {
            "Millimeters"
        } else {
            "Inches"
        })
        .show_ui(ui, |ui| {
            changed |= ui
                .selectable_value(&mut d.options.units, Units::Millimeters, "Millimeters")
                .changed();
            changed |= ui
                .selectable_value(&mut d.options.units, Units::Inches, "Inches")
                .changed();
        });
    changed |= tolerance_fields(ui, d);
    changed |= ui
        .checkbox(
            &mut d.options.reference,
            "Reference dimension (parentheses)",
        )
        .changed();
    egui::CollapsingHeader::new("Text and Parameter").show(ui, |ui| {
        changed |= ui.text_edit_singleline(&mut d.options.prefix).changed();
        changed |= ui.text_edit_singleline(&mut d.options.suffix).changed();
        ui.label("Additional tolerance note");
        changed |= ui.text_edit_singleline(&mut d.tolerance).changed();
        let parameters = w
            .session
            .visible()
            .parts
            .values()
            .flat_map(|p| {
                p.parameters.iter().map(|(id, param)| {
                    (
                        cad_core::ParameterRef {
                            definition: p.id.clone(),
                            parameter: id.clone(),
                        },
                        format!("{} / {}", p.name, param.name),
                    )
                })
            })
            .collect::<Vec<_>>();
        egui::ComboBox::from_label("Named Parameter")
            .selected_text(
                d.parameter
                    .as_ref()
                    .and_then(|r| parameters.iter().find(|(p, _)| p == r))
                    .map(|(_, name)| name.as_str())
                    .unwrap_or("Measure referenced geometry"),
            )
            .show_ui(ui, |ui| {
                changed |= ui
                    .selectable_value(&mut d.parameter, None, "Measure referenced geometry")
                    .changed();
                for (r, label) in parameters {
                    changed |= ui
                        .selectable_value(&mut d.parameter, Some(r), label)
                        .changed();
                }
            });
    });
    changed
}
fn anchor_fields(ui: &mut egui::Ui, w: &Workspace, label: &str, a: &mut Anchor) -> bool {
    point_fields(ui, w, label, &mut a.point, &mut a.reference)
}
fn annotation_fields(
    ui: &mut egui::Ui,
    w: &Workspace,
    views: &[String],
    tables: &[cad_core::drawing_tables::Table],
    scope: &str,
    a: &mut Annotation,
) -> bool {
    let mut changed = view_fields(ui, views, &mut a.view);
    match &mut a.kind {
        AnnotationKind::Balloon {
            target,
            table,
            definition,
            configuration,
            show_quantity,
        } => {
            changed |= anchor_fields(ui, w, "Balloon Target", target);
            changed |= crate::drawing_table_ui::balloon_fields(
                ui,
                w,
                tables,
                scope,
                crate::drawing_table_ui::BalloonFields {
                    table_id: table,
                    definition,
                    configuration,
                    show_quantity,
                },
            );
        }
        AnnotationKind::Leader { target, text } => {
            changed |= anchor_fields(ui, w, "Leader Target", target);
            changed |= ui.text_edit_multiline(text).changed();
        }
        AnnotationKind::Datum { target, identifier } => {
            changed |= anchor_fields(ui, w, "Datum Target", target);
            ui.label("Datum Identifier");
            changed |= ui.text_edit_singleline(identifier).changed();
        }
        AnnotationKind::GeometricTolerance {
            target,
            characteristic,
            value,
            diameter,
            material,
            datums,
        } => {
            changed |= anchor_fields(ui, w, "Tolerance Target", target);
            egui::ComboBox::from_label("Characteristic")
                .selected_text(format!("{characteristic:?}"))
                .show_ui(ui, |ui| {
                    for (kind, label) in [
                        (GeometricCharacteristic::Position, "Position"),
                        (GeometricCharacteristic::Flatness, "Flatness"),
                        (GeometricCharacteristic::Straightness, "Straightness"),
                        (GeometricCharacteristic::Circularity, "Circularity"),
                        (GeometricCharacteristic::Cylindricity, "Cylindricity"),
                        (GeometricCharacteristic::Parallelism, "Parallelism"),
                        (
                            GeometricCharacteristic::Perpendicularity,
                            "Perpendicularity",
                        ),
                        (GeometricCharacteristic::Angularity, "Angularity"),
                        (GeometricCharacteristic::ProfileLine, "Profile of Line"),
                        (
                            GeometricCharacteristic::ProfileSurface,
                            "Profile of Surface",
                        ),
                        (GeometricCharacteristic::CircularRunout, "Circular Runout"),
                        (GeometricCharacteristic::TotalRunout, "Total Runout"),
                    ] {
                        changed |= ui.selectable_value(characteristic, kind, label).changed();
                    }
                });
            changed |= expression(ui, "Tolerance Zone", value);
            changed |= ui.checkbox(diameter, "Diametrical Zone").changed();
            egui::ComboBox::from_label("Material Condition")
                .selected_text(format!("{material:?}"))
                .show_ui(ui, |ui| {
                    for (value, label) in [
                        (MaterialCondition::Regardless, "RFS"),
                        (MaterialCondition::Maximum, "MMC"),
                        (MaterialCondition::Least, "LMC"),
                    ] {
                        changed |= ui.selectable_value(material, value, label).changed();
                    }
                });
            let mut text = datums.join(" ");
            ui.label("Datum sequence (letters separated by spaces)");
            if ui.text_edit_singleline(&mut text).changed() {
                *datums = text.split_whitespace().map(str::to_string).collect();
                changed = true;
            }
        }
        AnnotationKind::CenterMark {
            curve,
            extension_mm,
        } => {
            let mut reference = Some(curve.clone());
            if curve_fields(ui, w, &mut reference) {
                if let Some(reference) = reference {
                    *curve = reference;
                }
                changed = true;
            }
            changed |= ui
                .add(
                    egui::DragValue::new(extension_mm)
                        .range(0.0..=1000.0)
                        .suffix(" mm")
                        .prefix("Paper extension "),
                )
                .changed();
        }
        AnnotationKind::CenterLine {
            first,
            second,
            extension_mm,
        } => {
            changed |= anchor_fields(ui, w, "First Point", first);
            changed |= anchor_fields(ui, w, "Second Point", second);
            changed |= ui
                .add(
                    egui::DragValue::new(extension_mm)
                        .range(0.0..=1000.0)
                        .suffix(" mm")
                        .prefix("Paper extension "),
                )
                .changed();
        }
    }
    if !matches!(
        a.kind,
        AnnotationKind::CenterMark { .. } | AnnotationKind::CenterLine { .. }
    ) {
        ui.horizontal(|ui| {
            for value in &mut a.offset_mm {
                changed |= ui
                    .add(
                        egui::DragValue::new(value)
                            .suffix(" mm")
                            .prefix("Label offset ")
                            .speed(0.2),
                    )
                    .changed();
            }
        });
    }
    changed
}
fn sheet_source(w: &Workspace, key: &str) -> Option<Drawing> {
    w.fields
        .get(&format!("drawing-editor:{key}"))
        .and_then(|s| serde_json::from_str(s).ok())
        .or_else(|| {
            w.session
                .visible()
                .drawings
                .iter()
                .find(|s| s.id == key)
                .cloned()
        })
}
pub fn current_source_is_valid(w: &Workspace) -> bool {
    w.fields
        .iter()
        .filter(|(key, _)| key.starts_with("drawing-editor:"))
        .all(|(_, value)| {
            serde_json::from_str::<Drawing>(value)
                .is_ok_and(|source| w.session.visible().drawings.iter().any(|s| s == &source))
        })
}
fn sync_source_validity(w: &mut Workspace) {
    if current_source_is_valid(w) {
        w.fields.remove("drawing-source-invalid");
    } else {
        w.fields
            .insert("drawing-source-invalid".into(), "true".into());
    }
}
fn put(w: &mut Workspace, sheet: Drawing) {
    match serde_json::to_string(&sheet) {
        Ok(source) => {
            w.start_tool();
            w.fields
                .insert(format!("drawing-editor:{}", sheet.id), source);
            let requested = sheet.clone();
            w.dispatch(Command::PutDrawing { drawing: sheet });
            // The command may append native source-row reservations. Accept that
            // normalization only if every explicitly edited field was preserved.
            if let Some(actual) = w
                .session
                .visible()
                .drawings
                .iter()
                .find(|s| s.id == requested.id)
            {
                let mut normalized = requested.clone();
                for table in &mut normalized.tables {
                    if let Some(current) = actual.tables.iter().find(|t| t.id == table.id)
                        && table
                            .item_numbers
                            .iter()
                            .all(|(key, number)| current.item_numbers.get(key) == Some(number))
                        && current.row_order.starts_with(&table.row_order)
                    {
                        table.item_numbers = current.item_numbers.clone();
                        table.row_order = current.row_order.clone();
                    }
                }
                if &normalized == actual
                    && let Ok(source) = serde_json::to_string(actual)
                {
                    w.fields
                        .insert(format!("drawing-editor:{}", actual.id), source);
                }
            }
            sync_source_validity(w);
        }
        Err(error) => {
            w.status = error.to_string();
            w.fields
                .insert("drawing-source-invalid".into(), "true".into());
        }
    }
}
fn unique_name(doc: &cad_core::Document, base: &str) -> String {
    let mut name = base.to_string();
    let mut index = 2;
    while doc
        .drawings
        .iter()
        .any(|s| s.name.trim().to_lowercase() == name.trim().to_lowercase())
    {
        name = format!("{base} {index}");
        index += 1;
    }
    name
}
fn drawing_context(w: &mut Workspace) -> bool {
    if w.sketch.is_some()
        || w.dialog.as_deref().is_some_and(|d| d != "drawings") && w.tool_checkpoint.is_some()
    {
        w.status = "Accept or cancel the current operation before editing drawing sheets".into();
        return false;
    }
    w.dialog = Some("drawings".into());
    w.manager_tree = false;
    true
}
fn select_sheet(w: &mut Workspace, key: &str) {
    w.fields.insert("drawing-preview".into(), key.into());
    w.fields.insert("drawing-active".into(), key.into());
}
fn read_template() -> cad_core::Result<Option<SheetTemplate>> {
    let Some(path) = rfd::FileDialog::new()
        .add_filter("TCAD sheet template", &["json"])
        .pick_file()
    else {
        return Ok(None);
    };
    if std::fs::metadata(&path)?.len() > 1024 * 1024 {
        return Err(cad_core::CadError::Invalid(
            "A sheet template may not exceed 1 MiB".into(),
        ));
    }
    let template: SheetTemplate = serde_json::from_str(&std::fs::read_to_string(path)?)?;
    template.validate()?;
    Ok(Some(template))
}
fn load_template(w: &mut Workspace, key: Option<&str>) {
    let result = (|| -> cad_core::Result<Option<Drawing>> {
        let Some(template) = read_template()? else {
            return Ok(None);
        };
        if let Some(key) = key {
            let mut sheet = sheet_source(w, key)
                .ok_or_else(|| cad_core::CadError::Invalid("Drawing sheet is missing".into()))?;
            template.apply_format(&mut sheet)?;
            Ok(Some(sheet))
        } else {
            let doc = w.session.visible();
            let definition = w.view.active_part.as_deref().unwrap_or(&doc.root);
            Ok(Some(template.instantiate(
                doc,
                definition,
                unique_name(doc, &template.name),
            )?))
        }
    })();
    match result {
        Ok(Some(sheet)) => {
            select_sheet(w, &sheet.id);
            put(w, sheet);
        }
        Ok(None) => {}
        Err(error) => w.status = error.to_string(),
    }
}
fn save_template(w: &mut Workspace, key: &str) {
    let Some(sheet) = sheet_source(w, key) else {
        return;
    };
    let template = SheetTemplate::from_sheet(&sheet);
    let result = (|| -> cad_core::Result<()> {
        template.validate()?;
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("TCAD sheet template", &["json"])
            .set_file_name("sheet-template.json")
            .save_file()
        {
            cad_core::persistence::atomic_write(
                &path,
                serde_json::to_string_pretty(&template)?.as_bytes(),
            )?;
        }
        Ok(())
    })();
    if let Err(error) = result {
        w.status = error.to_string();
    }
}
pub fn action(w: &mut Workspace, command: &str) -> bool {
    if !matches!(command, "drawing.sheet-add" | "drawing.sheet-template-load") {
        return false;
    }
    if !drawing_context(w) {
        return true;
    }
    if command == "drawing.sheet-template-load" {
        load_template(w, None);
        return true;
    }
    let doc = w.session.visible();
    let definition = w.view.active_part.as_deref().unwrap_or(&doc.root);
    match new_sheet(doc, definition, unique_name(doc, "Sheet")) {
        Ok(sheet) => {
            select_sheet(w, &sheet.id);
            put(w, sheet);
        }
        Err(error) => w.status = error.to_string(),
    }
    true
}
pub fn semantic_action(
    w: &mut Workspace,
    element: &cad_core::presentation::SemanticElement,
    command: &str,
) -> bool {
    if action(w, command) {
        return true;
    }
    if !command.starts_with("drawing.") {
        return false;
    }
    if !drawing_context(w) {
        return true;
    }
    let key = element
        .data
        .get("sheet_id")
        .and_then(serde_json::Value::as_str)
        .or_else(|| {
            element
                .data
                .get("sheet")
                .and_then(|s| s.get("id"))
                .and_then(serde_json::Value::as_str)
        });
    let Some(key) = key
        .filter(|key| w.session.visible().drawings.iter().any(|s| &s.id == key))
        .map(str::to_string)
    else {
        return true;
    };
    select_sheet(w, &key);
    if command == "drawing.sheet-edit" {
        let section = match element.role.as_str() {
            "drawing-sheet-format" | "drawing-title-field" => "format",
            "drawing-view" => "views",
            _ => "items",
        };
        w.fields.insert("drawing-section".into(), section.into());
        w.fields.insert("drawing-item".into(), element.id.clone());
        if let Some(table) = element
            .data
            .get("table_id")
            .and_then(serde_json::Value::as_str)
        {
            w.fields.insert("drawing-table".into(), table.into());
        }
        if let Some(view) = element
            .data
            .get("view_key")
            .and_then(serde_json::Value::as_str)
        {
            w.fields.insert("drawing-view".into(), view.into());
        }
    }
    match command {
        "drawing.sheet-edit" | "drawing.sheet-preview" => {}
        "drawing.sheet-template-save" => save_template(w, &key),
        "drawing.sheet-format-load" => load_template(w, Some(&key)),
        "drawing.sheet-delete"
        | "drawing.sheet-duplicate"
        | "drawing.sheet-up"
        | "drawing.sheet-down" => {
            if !current_source_is_valid(w) {
                w.status =
                    "Correct or cancel the current drawing source before managing sheets".into();
                return true;
            }
            let doc = w.session.visible();
            let index = doc.drawings.iter().position(|s| s.id == key).unwrap();
            let before = doc
                .drawings
                .iter()
                .map(|s| s.id.clone())
                .collect::<std::collections::BTreeSet<_>>();
            let command = match command {
                "drawing.sheet-delete" => Command::DeleteDrawing {
                    drawing: key.clone(),
                },
                "drawing.sheet-duplicate" => Command::DuplicateDrawing {
                    drawing: key.clone(),
                    name: unique_name(doc, &format!("{} Copy", doc.drawings[index].name)),
                },
                "drawing.sheet-up" if index > 0 => Command::MoveDrawing {
                    drawing: key.clone(),
                    index: index - 1,
                },
                "drawing.sheet-down" if index + 1 < doc.drawings.len() => Command::MoveDrawing {
                    drawing: key.clone(),
                    index: index + 1,
                },
                _ => return true,
            };
            w.start_tool();
            w.dispatch(command);
            if let Some(next) = w
                .session
                .visible()
                .drawings
                .iter()
                .find(|s| !before.contains(&s.id))
                .map(|s| s.id.clone())
            {
                select_sheet(w, &next);
            }
            if !w.session.visible().drawings.iter().any(|s| s.id == key) {
                w.fields.remove(&format!("drawing-editor:{key}"));
                if let Some(next) = w.session.visible().drawings.first().map(|s| s.id.clone()) {
                    select_sheet(w, &next);
                } else {
                    w.fields.remove("drawing-preview");
                    w.fields.remove("drawing-active");
                }
            }
            sync_source_validity(w);
        }
        _ => {}
    }
    true
}
fn sheet_command(w: &mut Workspace, key: &str, command: &str) {
    let element = cad_core::presentation::SemanticElement {
        id: key.into(),
        role: "drawing-sheet".into(),
        label: "Drawing sheet".into(),
        value: None,
        state: "ready".into(),
        commands: vec![],
        relationships: vec![],
        data: serde_json::json!({"sheet_id":key}),
    };
    semantic_action(w, &element, command);
}
fn linked_fields(ui: &mut egui::Ui, value: &mut LinkedText) -> bool {
    let mut changed = false;
    egui::ComboBox::from_id_salt("title-field-source")
        .selected_text(value.label())
        .show_ui(ui, |ui| {
            for choice in [
                LinkedText::Literal {
                    text: String::new(),
                },
                LinkedText::DocumentName,
                LinkedText::DocumentProperty {
                    key: "revision".into(),
                },
                LinkedText::ModelName,
                LinkedText::ModelProperty {
                    key: "part_number".into(),
                },
                LinkedText::ConfigurationName,
                LinkedText::MaterialName,
                LinkedText::SheetName,
                LinkedText::SheetNumber,
                LinkedText::SourceRevision,
                LinkedText::SourceFingerprint,
            ] {
                let selected = choice.label() == value.label();
                if ui.selectable_label(selected, choice.label()).clicked() && !selected {
                    *value = choice;
                    changed = true;
                }
            }
        });
    match value {
        LinkedText::Literal { text } => {
            changed |= ui.text_edit_multiline(text).changed();
        }
        LinkedText::DocumentProperty { key } | LinkedText::ModelProperty { key } => {
            ui.label("Native property key");
            changed |= ui.text_edit_singleline(key).changed();
        }
        _ => {}
    }
    changed
}
fn format_fields(
    ui: &mut egui::Ui,
    doc: &cad_core::Document,
    sheet: &mut Drawing,
    active: Option<&str>,
) -> bool {
    let context = sheet.clone();
    let mut changed = false;
    let mut enabled = sheet.format.is_some();
    if ui
        .checkbox(&mut enabled, "Use editable sheet format")
        .changed()
    {
        sheet.format = enabled.then(SheetFormat::default);
        changed = true;
    }
    let Some(format) = &mut sheet.format else {
        return changed;
    };
    if ui.checkbox(&mut format.border, "Border").changed() {
        if !format.border {
            format.zones = None;
        }
        changed = true;
    }
    ui.horizontal_wrapped(|ui| {
        for (value, label) in format
            .margins_mm
            .iter_mut()
            .zip(["Left", "Bottom", "Right", "Top"])
        {
            changed |= ui
                .add(
                    egui::DragValue::new(value)
                        .range(2. ..=200.)
                        .prefix(format!("{label} "))
                        .suffix(" mm")
                        .speed(0.25),
                )
                .changed();
        }
    });
    let mut zones = format.zones.is_some();
    if ui
        .add_enabled(
            format.border,
            egui::Checkbox::new(&mut zones, "Border zones"),
        )
        .changed()
    {
        format.zones = zones.then_some(Zones {
            columns: 4,
            rows: 4,
            band_mm: 5.,
        });
        changed = true;
    }
    if let Some(zones) = &mut format.zones {
        ui.horizontal_wrapped(|ui| {
            changed |= ui
                .add(
                    egui::DragValue::new(&mut zones.columns)
                        .range(1..=100)
                        .prefix("Columns "),
                )
                .changed();
            changed |= ui
                .add(
                    egui::DragValue::new(&mut zones.rows)
                        .range(1..=26)
                        .prefix("Rows "),
                )
                .changed();
            changed |= ui
                .add(
                    egui::DragValue::new(&mut zones.band_mm)
                        .range(3. ..=50.)
                        .suffix(" mm")
                        .prefix("Band "),
                )
                .changed();
        });
    }
    let mut title = format.title_block.is_some();
    if ui.checkbox(&mut title, "Title block").changed() {
        format.title_block = if title {
            SheetFormat::default().title_block
        } else {
            None
        };
        changed = true;
    }
    if let Some(block) = &mut format.title_block {
        ui.horizontal_wrapped(|ui| {
            for (value, label) in block.size_mm.iter_mut().zip(["Width", "Height"]) {
                changed |= ui
                    .add(
                        egui::DragValue::new(value)
                            .range(8. ..=2000.)
                            .prefix(format!("{label} "))
                            .suffix(" mm"),
                    )
                    .changed();
            }
            changed |= ui
                .add(
                    egui::DragValue::new(&mut block.text_size_mm)
                        .range(0.5..=10.)
                        .prefix("Text ")
                        .suffix(" mm")
                        .speed(0.1),
                )
                .changed();
        });
        let mut remove = None;
        for (index, cell) in block.cells.iter_mut().enumerate() {
            ui.push_id(&cell.id, |ui| {
                egui::CollapsingHeader::new(format!("{} · {}", index + 1, cell.label))
                    .default_open(active == Some(&cell.id))
                    .show(ui, |ui| {
                        changed |= ui.text_edit_singleline(&mut cell.label).changed();
                        changed |= linked_fields(ui, &mut cell.value);
                        match linked_text(doc, &context, &cell.value) {
                            Ok(text) => {
                                ui.small(format!("Current value: {text}"));
                            }
                            Err(error) => {
                                ui.colored_label(ui.visuals().error_fg_color, error.to_string());
                            }
                        }
                        ui.horizontal_wrapped(|ui| {
                            for (value, label) in
                                cell.rect_mm.iter_mut().zip(["X", "Y", "Width", "Height"])
                            {
                                changed |= ui
                                    .add(
                                        egui::DragValue::new(value)
                                            .prefix(format!("{label} "))
                                            .suffix(" mm")
                                            .speed(0.25),
                                    )
                                    .changed();
                            }
                        });
                        if ui.small_button("Delete title field").clicked() {
                            remove = Some(index);
                        }
                    });
            });
        }
        if let Some(index) = remove {
            block.cells.remove(index);
            changed = true;
        }
        if ui
            .add_enabled(
                block.cells.len() < 128,
                egui::Button::new("Add title field row"),
            )
            .clicked()
        {
            block.cells.push(TitleCell {
                id: cad_core::id(),
                label: "FIELD".into(),
                value: LinkedText::Literal {
                    text: String::new(),
                },
                rect_mm: [0., block.size_mm[1], block.size_mm[0], 12.],
            });
            block.size_mm[1] += 12.;
            changed = true;
        }
    }
    ui.small("Linked fields update from native source and the current configuration. Missing custom property values are blank. Cell rectangles cannot overlap; text overflow is reported.");
    changed
}
fn view_layout_fields(ui: &mut egui::Ui, w: &Workspace, sheet: &mut Drawing) -> bool {
    let mut changed = false;
    let resolved = evaluation(w)
        .and_then(|e| build_sheet(w.session.visible(), e, sheet).ok())
        .map(|s| s.views)
        .unwrap_or_default();
    let bounds = paper_bounds(sheet);
    let height = ((bounds[1][1] - bounds[0][1]) / 4.).max(8.);
    let fallback = ViewLayout {
        rect_mm: [
            bounds[0][0] + 4.,
            bounds[1][1] - height - 7.,
            (bounds[1][0] - bounds[0][0] - 8.).max(8.),
            height,
        ],
        scale: None,
    };
    for key in view_keys(sheet) {
        ui.push_id(&key, |ui| {
            egui::CollapsingHeader::new(format!("{key} placement and scale"))
                .default_open(w.fields.get("drawing-view") == Some(&key))
                .show(ui, |ui| {
                    let mut manual = sheet.view_layouts.contains_key(&key);
                    if ui.checkbox(&mut manual, "Saved paper placement").changed() {
                        if manual {
                            let mut layout = resolved.get(&key).unwrap_or(&fallback).clone();
                            layout.scale = None;
                            sheet.view_layouts.insert(key.clone(), layout);
                            changed = true;
                        } else {
                            sheet.view_layouts.remove(&key);
                            changed = true;
                        }
                    }
                    if let Some(layout) = sheet.view_layouts.get_mut(&key) {
                        ui.horizontal_wrapped(|ui| {
                            for (value, label) in
                                layout.rect_mm.iter_mut().zip(["X", "Y", "Width", "Height"])
                            {
                                changed |= ui
                                    .add(
                                        egui::DragValue::new(value)
                                            .prefix(format!("{label} "))
                                            .suffix(" mm")
                                            .speed(0.25),
                                    )
                                    .changed();
                            }
                        });
                        let mut fixed = layout.scale.is_some();
                        if ui
                            .checkbox(&mut fixed, "Fixed model-to-paper scale")
                            .changed()
                        {
                            layout.scale = if fixed {
                                Some(resolved.get(&key).and_then(|v| v.scale).unwrap_or(1.))
                            } else {
                                None
                            };
                            changed = true;
                        }
                        if let Some(scale) = &mut layout.scale {
                            changed |= ui
                                .add(
                                    egui::DragValue::new(scale)
                                        .speed(0.01)
                                        .range(1e-6..=1000.)
                                        .prefix("Scale ")
                                        .suffix(":1"),
                                )
                                .changed();
                        }
                    } else {
                        ui.small("Automatic fitting and grid placement");
                    }
                });
        });
    }
    changed
}
pub fn editor(ui: &mut egui::Ui, w: &mut Workspace) {
    let doc = w.session.visible().clone();
    ui.small("Drawing annotations use current solved geometry and exact curve data. Paper preview, PDF, and DXF share the same vectors.");
    let mut preview = w.fields.contains_key("drawing-preview");
    if ui
        .checkbox(&mut preview, "Show sheet in main viewport")
        .changed()
    {
        if preview {
            if let Some(sheet) = doc.drawings.first() {
                w.fields.insert("drawing-preview".into(), sheet.id.clone());
            }
        } else {
            w.fields.remove("drawing-preview");
        }
    }
    ui.horizontal_wrapped(|ui| {
        if ui.button("Add Drawing Sheet").clicked() {
            action(w, "drawing.sheet-add");
        }
        if ui.button("Add Sheet from Template").clicked() {
            action(w, "drawing.sheet-template-load");
        }
    });
    egui::ScrollArea::vertical().id_salt("production-drawings").show(ui,|ui| {for sheet in &doc.drawings {ui.push_id(&sheet.id,|ui| {egui::CollapsingHeader::new(&sheet.name).default_open(w.fields.get("drawing-active")==Some(&sheet.id)).show(ui,|ui| {
        let mut sheet=sheet_source(w,&sheet.id).unwrap_or_else(||sheet.clone());let mut changed=ui.text_edit_singleline(&mut sheet.name).changed();
        let mut manage=None;
        ui.horizontal_wrapped(|ui| {for (command,label) in [("drawing.sheet-preview","Preview"),("drawing.sheet-up","↑ Sheet"),("drawing.sheet-down","↓ Sheet"),("drawing.sheet-duplicate","Duplicate"),("drawing.sheet-delete","Delete")] {if ui.small_button(label).clicked() {manage=Some(command);}}});
        if let Some(command)=manage {if changed {put(w,sheet.clone());}sheet_command(w,&sheet.id,command);return;}
        let mut template_action=None;
        ui.horizontal_wrapped(|ui| {
            if ui.small_button("Save Template").clicked() {template_action=Some(false);}
            if ui.small_button("Load Format from Template").clicked() {template_action=Some(true);}
        });
        if let Some(load)=template_action {if changed {put(w,sheet.clone());}
            if load{load_template(w,Some(&sheet.id));}else{save_template(w,&sheet.id);}return;}
        let names=doc.parts.values().map(|p|(p.id.clone(),p.name.clone())).chain(doc.assemblies.values().map(|a|(a.id.clone(),a.name.clone()))).collect::<Vec<_>>();egui::ComboBox::from_label("Model Scope").selected_text(names.iter().find(|(id,_)|id==&sheet.definition).map(|(_,name)|name.as_str()).unwrap_or("Missing definition")).show_ui(ui,|ui| {for (id,name) in names {changed|=ui.selectable_value(&mut sheet.definition,id,name).changed();}});
        ui.horizontal(|ui| {for (label,value) in ["Width","Height"].into_iter().zip(&mut sheet.paper_mm) {changed|=ui.add(egui::DragValue::new(value).range(50.0..=2000.0).prefix(format!("{label} ")).suffix(" mm")).changed();}});
        ui.horizontal_wrapped(|ui| {for (label,paper) in [("A4 Landscape",[297.,210.]),("A3 Landscape",[420.,297.]),("A2 Landscape",[594.,420.]),("Letter",[279.4,215.9]),("Ledger",[431.8,279.4])] {if ui.small_button(label).clicked() {sheet.paper_mm=paper;changed=true;}}if ui.small_button("Swap Orientation").clicked() {sheet.paper_mm.swap(0,1);changed=true;}});
        egui::ComboBox::from_id_salt("drawing-exploded-view").selected_text(sheet.exploded_view.as_ref().and_then(|key|doc.assemblies.get(&sheet.definition).and_then(|a|a.exploded_views.get(key))).map(|v|v.name.as_str()).unwrap_or(if sheet.exploded_view.is_some(){"Missing/incompatible exploded view"}else{"Assembled"})).show_ui(ui,|ui| {
            changed|=ui.selectable_value(&mut sheet.exploded_view,None,"Assembled").changed();
            if sheet.definition==doc.root && let Some(assembly)=doc.assemblies.get(&sheet.definition) {for view in assembly.exploded_views.values().filter(|v|v.configuration.is_none() || v.configuration==doc.active_configurations.get(&sheet.definition).cloned()) {changed|=ui.selectable_value(&mut sheet.exploded_view,Some(view.id.clone()),&view.name).changed();}}
        });
        ui.horizontal_wrapped(|ui| {for name in VIEW_KEYS {let mut views=view_keys(&sheet);let mut enabled=views.iter().any(|v|v==name);if ui.checkbox(&mut enabled,name).changed() {if enabled {views.push(name.into());} else {views.retain(|v|v!=name);}match set_views(&mut sheet,views) {Ok(())=>changed=true,Err(error)=>w.status=error.to_string()}}}});
        egui::CollapsingHeader::new("View order").show(ui,|ui| {let mut order=view_keys(&sheet);let mut reorder=None;for (index,key) in order.iter().enumerate() {ui.horizontal(|ui| {ui.label(key);if ui.add_enabled(index>0,egui::Button::new("↑").small()).clicked() {reorder=Some((index,index-1));}
            if ui.add_enabled(index+1<order.len(),egui::Button::new("↓").small()).clicked() {reorder=Some((index,index+1));}});}
            if let Some((a,b))=reorder {order.swap(a,b);sheet.views=order;changed=true;}});
        let active_item=w.fields.get("drawing-item").cloned();
        egui::CollapsingHeader::new("Sheet format and linked title fields").default_open(w.fields.get("drawing-section").is_some_and(|s|s=="format")).show(ui,|ui| {changed|=format_fields(ui,&doc,&mut sheet,active_item.as_deref());});
        egui::CollapsingHeader::new("View placement and scale").default_open(w.fields.get("drawing-section").is_some_and(|s|s=="views")).show(ui,|ui| {changed|=view_layout_fields(ui,w,&mut sheet);});
        ui.menu_button("Add Dimension",|ui| {for (kind,label) in [(DimensionKind::Aligned,"Aligned"),(DimensionKind::Horizontal,"Horizontal"),(DimensionKind::Vertical,"Vertical"),(DimensionKind::Angular,"Angular"),(DimensionKind::Diameter,"Diameter"),(DimensionKind::Radius,"Radius"),(DimensionKind::HoleCallout,"Hole Callout")] {if ui.button(label).clicked() {let curve=selected_curve(w);let (first,second)=if let Some(CurveReference::ModelEdge {edge,body,instance_path})=&curve {(Some(PointReference::EdgePoint {edge:edge.clone(),body:body.clone(),instance_path:instance_path.clone(),parameter:0.}),Some(PointReference::EdgePoint {edge:edge.clone(),body:body.clone(),instance_path:instance_path.clone(),parameter:1.}))} else {(None,None)};let hole_feature=w.selected.as_ref().filter(|s|doc.parts.get(&s.reference.producer.definition).is_some_and(|p|p.features.iter().any(|f|f.id==s.reference.producer.node && matches!(f.operation,cad_core::Operation::Hole {..}|cad_core::Operation::DrilledHoles {..})))).map(|s|s.reference.producer.clone());sheet.dimensions.push(DrawingDimension {id:cad_core::id(),view:sheet.views.first().cloned().unwrap_or("front".into()),first:[0.;3],second:[50.,0.,0.],offset_mm:10.,parameter:None,tolerance:String::new(),options:DimensionOptions {kind,curve,hole_feature,first,second,third_point:[0.,0.,50.],..Default::default()}});changed=true;ui.close();}}});
        let mut remove=None;for (index,d) in sheet.dimensions.iter_mut().enumerate() {ui.push_id(d.id.clone(),|ui| {egui::CollapsingHeader::new(format!("{:?} Dimension {}",d.options.kind,index+1)).show(ui,|ui| {changed|=dimension_fields(ui,w,&sheet.views,d);if ui.small_button("Delete Dimension").clicked() {remove=Some(index);}});});}if let Some(index)=remove {sheet.dimensions.remove(index);changed=true;}
        ui.menu_button("Add Annotation",|ui| {for (kind,label) in [(0,"Leader Note"),(1,"Datum Identifier"),(2,"Geometric Tolerance"),(3,"Center Mark"),(4,"Centerline")] {if ui.button(label).clicked() {let target=selected_anchor(w);let annotation=match kind {1=>AnnotationKind::Datum {target,identifier:"A".into()},2=>AnnotationKind::GeometricTolerance {target,characteristic:GeometricCharacteristic::Position,value:"0.1 mm".into(),diameter:false,material:MaterialCondition::Regardless,datums:vec![]},3=>{let Some(curve)=selected_curve(w) else {w.status="Select a circular edge before adding a center mark".into();ui.close();continue;};AnnotationKind::CenterMark {curve,extension_mm:3.}},4=>AnnotationKind::CenterLine {first:target,second:Box::new(Anchor {point:[50.,0.,0.],reference:None}),extension_mm:3.},_=>AnnotationKind::Leader {target,text:"Note".into()}};sheet.entities.push(Annotation {id:cad_core::id(),view:sheet.views.first().cloned().unwrap_or("front".into()),offset_mm:[15.,15.],kind:annotation});changed=true;ui.close();}}});
        changed |= crate::drawing_table_ui::editor(ui,w,&mut sheet);
        if ui.button("Add BOM Balloon").clicked() {
            match crate::drawing_table_ui::default_balloon(w,&sheet,selected_anchor(w)) {
                Ok(kind) => { sheet.entities.push(Annotation {id:cad_core::id(),view:sheet.views.first().cloned().unwrap_or("front".into()),offset_mm:[15.,15.],kind}); changed=true; }
                Err(error) => { w.status=error.to_string(); }
            }
        }
        let mut remove=None;for (index,a) in sheet.entities.iter_mut().enumerate() {ui.push_id(a.id.clone(),|ui| {egui::CollapsingHeader::new(format!("Annotation {}",index+1)).show(ui,|ui| {changed|=annotation_fields(ui,w,&sheet.views,&sheet.tables,&sheet.definition,a);if ui.small_button("Delete Annotation").clicked() {remove=Some(index);}});});}if let Some(index)=remove {sheet.entities.remove(index);changed=true;}
        for note in &mut sheet.annotations {changed|=ui.text_edit_singleline(note).changed();}if ui.small_button("Add Sheet Note").clicked() {sheet.annotations.push(String::new());changed=true;}
        if changed {put(w,sheet);}
    });});}});
    if let Some(error) = w.session.draft.as_ref().and_then(|d| d.error.as_ref()) {
        ui.colored_label(Color32::from_rgb(210, 75, 45), error);
    }
    if !current_source_is_valid(w) {
        ui.colored_label(ui.visuals().error_fg_color,"Correct the current sheet fields before applying. Its last validated geometry remains available.");
    }
    ui.horizontal(|ui| {
        if ui
            .add_enabled(
                w.session.draft.is_some() && current_source_is_valid(w),
                egui::Button::new("Apply Drawing Changes"),
            )
            .clicked()
        {
            w.confirm_tool();
        }
        if ui
            .add_enabled(w.tool_checkpoint.is_some(), egui::Button::new("Cancel"))
            .clicked()
        {
            w.cancel_tool();
        }
    });
    ui.separator();
    ui.horizontal(|ui| {
        if ui
            .add_enabled(current_source_is_valid(w), egui::Button::new("Export PDF"))
            .clicked()
        {
            crate::ui::export_format(w, "pdf", "drawing.pdf");
        }
        if ui
            .add_enabled(
                current_source_is_valid(w),
                egui::Button::new("Export Drawing DXF"),
            )
            .clicked()
        {
            crate::ui::export_format(w, "drawing-dxf", "drawing.dxf");
        }
    });
}

pub fn preview(w: &Workspace, ui: &mut egui::Ui, rect: egui::Rect) -> bool {
    if w.dialog.as_deref() != Some("drawings") {
        return false;
    }
    let Some(id) = w.fields.get("drawing-preview") else {
        return false;
    };
    let Some(sheet) = w.session.visible().drawings.iter().find(|d| &d.id == id) else {
        return false;
    };
    let region = cad_core::presentation::Region {
        id: sheet.id.clone(),
        role: "drawing-sheet".into(),
        component: None,
        rect: [rect.left(), rect.top(), rect.width(), rect.height()],
        classes: vec!["paper".into(), "preview".into()],
        resizable: false,
    };
    let style = w.theme.properties(
        &region,
        &w.view.active_layout,
        if w.session.draft.as_ref().is_some_and(|draft| draft.pending) {
            "pending"
        } else {
            "ready"
        },
    );
    if style.get("display").is_some_and(|value| value == "none") {
        return true;
    }
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0., ui.visuals().faint_bg_color);
    let Some(e) = evaluation(w) else {
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "Preparing drawing geometry…",
            egui::FontId::proportional(14.),
            ui.visuals().text_color(),
        );
        return true;
    };
    match build_sheet(w.session.visible(), e, sheet) {
        Ok(sheet) => {
            let scale = ((rect.width() - 24.) / sheet.paper_mm[0] as f32)
                .min((rect.height() - 24.) / sheet.paper_mm[1] as f32);
            let size = egui::vec2(
                sheet.paper_mm[0] as f32 * scale,
                sheet.paper_mm[1] as f32 * scale,
            );
            let paper = egui::Rect::from_center_size(rect.center(), size);
            painter.rect_filled(
                paper,
                0.,
                style
                    .get("background")
                    .and_then(|value| crate::ui::color(value))
                    .unwrap_or(Color32::from_rgb(250, 249, 243)),
            );
            let at = |p: [f64; 2]| {
                egui::pos2(
                    paper.left() + p[0] as f32 * scale,
                    paper.bottom() - p[1] as f32 * scale,
                )
            };
            let stroke = Stroke::new(
                style
                    .get("stroke-width")
                    .and_then(|value| value.trim_end_matches("px").parse::<f32>().ok())
                    .filter(|value| value.is_finite() && *value > 0.)
                    .unwrap_or((0.16 * scale).max(0.7)),
                style
                    .get("color")
                    .and_then(|value| crate::ui::color(value))
                    .unwrap_or(Color32::from_rgb(25, 27, 28)),
            );
            for primitive in sheet.primitives {
                match primitive {
                    Primitive::Polyline { points, dashed } => {
                        for pair in points.windows(2) {
                            let a = at(pair[0]);
                            let b = at(pair[1]);
                            if dashed {
                                let delta = b - a;
                                let length = delta.length();
                                let mut start = 0.;
                                let mut index = 0;
                                while start < length {
                                    let end = (start
                                        + if index % 2 == 0 { 4. * scale } else { scale })
                                    .min(length);
                                    painter.line_segment(
                                        [a + delta * start / length, a + delta * end / length],
                                        stroke,
                                    );
                                    start = end + scale;
                                    index += 1;
                                }
                            } else {
                                painter.line_segment([a, b], stroke);
                            }
                        }
                    }
                    Primitive::Text { at: p, size, text } => {
                        painter.text(
                            at(p),
                            egui::Align2::LEFT_BOTTOM,
                            text,
                            egui::FontId::proportional((size as f32 * scale).max(1.)),
                            stroke.color,
                        );
                    }
                    Primitive::Spline { curve } => {
                        if let Ok(points) = display_curve(&curve) {
                            for pair in points.windows(2) {
                                painter.line_segment([at(pair[0]), at(pair[1])], stroke);
                            }
                        }
                    }
                    Primitive::Circle { center, radius } => {
                        painter.circle_stroke(at(center), radius as f32 * scale, stroke);
                    }
                }
            }
            if w.session.draft.as_ref().is_some_and(|d| d.pending) {
                painter.text(
                    rect.left_top() + egui::vec2(10., 10.),
                    egui::Align2::LEFT_TOP,
                    "Updating drawing preview…",
                    egui::FontId::proportional(12.),
                    ui.visuals().text_color(),
                );
            }
        }
        Err(error) => {
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                error.to_string(),
                egui::FontId::proportional(13.),
                ui.visuals().text_color(),
            );
        }
    }
    true
}
