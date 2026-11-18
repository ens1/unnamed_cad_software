//! Default property manager for sketch-based extrusions and revolutions.
use super::{body_selector, controls, expression, profile_selector, result_fields};
use crate::Workspace;
use bevy_egui::egui;
use cad_core::{Document, Feature, NodeRef, Operation};

pub(crate) fn options_section(
    ui: &mut egui::Ui,
    feature: &str,
    key: &str,
    label: &str,
    active: bool,
    content: impl FnOnce(&mut egui::Ui),
) {
    controls::inspector_section(
        ui,
        controls::InspectorSection {
            id: &format!("operation-editor:{feature}/group/{key}"),
            role: "operation-editor-group",
            classes: &["modeling-group", "secondary", key],
            label,
            state: if active { "draft active" } else { "draft" },
            collapse: Some(active),
            open: None,
        },
        content,
    );
}

pub(crate) fn scope_fields(
    ui: &mut egui::Ui,
    doc: &Document,
    feature: &str,
    body: Option<&NodeRef>,
    scope: &mut Vec<NodeRef>,
) -> bool {
    let candidates = doc
        .parts
        .values()
        .flat_map(|part| {
            part.features
                .iter()
                .take_while(|f| f.id != feature)
                .filter(|f| !f.suppressed && f.operation.produces_solid())
                .map(|f| (part.reference(&f.id), format!("{} / {}", part.name, f.name)))
        })
        .filter(|(reference, _)| Some(reference) != body)
        .collect::<Vec<_>>();
    if candidates.is_empty() && scope.is_empty() {
        return false;
    }
    let mut changed = false;
    let title = if scope.is_empty() {
        "Additional bodies".into()
    } else {
        format!("Additional bodies ({})", scope.len())
    };
    options_section(ui, feature, "scope", &title, !scope.is_empty(), |ui| {
        for (reference, name) in &candidates {
            let mut included = scope.contains(reference);
            if ui.checkbox(&mut included, name).changed() {
                if included {
                    scope.push(reference.clone());
                } else {
                    scope.retain(|r| r != reference);
                }
                changed = true;
            }
        }
        scope.retain(|reference| {
            if candidates
                .iter()
                .any(|(candidate, _)| candidate == reference)
            {
                return true;
            }
            let remove = ui
                .button("Remove unavailable body")
                .on_hover_text(format!("{} / {}", reference.definition, reference.node))
                .clicked();
            changed |= remove;
            !remove
        });
    });
    changed
}

pub(super) fn fields(
    ui: &mut egui::Ui,
    w: &Workspace,
    doc: &Document,
    feature: &mut Feature,
) -> bool {
    let mut changed = false;
    match &mut feature.operation {
        Operation::Extrude {
            profile,
            distance,
            direction,
            body,
            mode,
            options,
        } => {
            if profile_selector(ui, doc, &feature.id, "Sketch", profile) {
                // Selected contours belong to their sketch. A newly chosen
                // profile starts with all of its own contours.
                options.contours.clear();
                changed = true;
            }
            let prior_end = options.end.clone();
            let mut first = cad_core::extrusion::DirectionEnd {
                end: options.end.clone(),
                distance: distance.clone(),
                target: options.target.clone(),
                target_body: options.target_body.clone(),
                target_vertex: options.target_vertex.clone(),
                end_offset: options.end_offset.clone(),
            };
            changed |= extrusion_end_fields(ui, w, doc, &feature.id, &mut first, true);
            options.end = first.end;
            *distance = first.distance;
            options.target = first.target;
            options.target_body = first.target_body;
            options.target_vertex = first.target_vertex;
            options.end_offset = first.end_offset;

            if prior_end != options.end
                && matches!(
                    options.end,
                    cad_core::ExtrudeEnd::MidPlane | cad_core::ExtrudeEnd::ThroughAllBoth
                )
            {
                options.direction2 = None;
                options.reverse_distance = None;
            }
            if controls::button(
                ui,
                "feature.extrude/reverse-direction",
                "operation-editor-action",
                &["modeling-action"],
                "Reverse direction",
                true,
                options.profile_normal && options.reverse_direction,
            )
            .clicked()
            {
                if options.profile_normal {
                    options.reverse_direction = !options.reverse_direction;
                } else {
                    *direction = direction.map(|v| -v);
                }
                changed = true;
            }
            changed |= result_fields(ui, doc, &feature.id, body, mode);
            changed |= crate::contours_ui::fields(
                ui,
                w,
                doc,
                &feature.id,
                std::slice::from_ref(profile),
                &mut options.contours,
                options.thin.is_some(),
            );
            options_section(
                ui,
                &feature.id,
                "direction",
                "Start and direction",
                !options.profile_normal
                    || !options.start.is_sketch_plane()
                    || options.start_offset.0.trim() != "0 mm"
                    || options.direction2.is_some()
                    || options.reverse_distance.is_some(),
                |ui| {
                    let previous_normal = options.profile_normal;
                    egui::ComboBox::from_label("Extrusion Direction")
                        .selected_text(if options.profile_normal {
                            "Normal to Sketch"
                        } else {
                            "Specified Vector"
                        })
                        .show_ui(ui, |ui| {
                            changed |= ui
                                .selectable_value(
                                    &mut options.profile_normal,
                                    true,
                                    "Normal to Sketch",
                                )
                                .changed();
                            changed |= ui
                                .selectable_value(
                                    &mut options.profile_normal,
                                    false,
                                    "Specified Vector",
                                )
                                .changed();
                        });
                    if previous_normal && !options.profile_normal {
                        let plane = w
                            .session
                            .draft
                            .as_ref()
                            .and_then(|d| d.evaluation.as_ref())
                            .or(w.session.evaluation.as_ref())
                            .and_then(|e| {
                                e.sketches
                                    .get(&format!("{}::{}", profile.definition, profile.node))
                            })
                            .map(|s| &s.plane)
                            .or_else(|| {
                                doc.parts
                                    .get(&profile.definition)
                                    .and_then(|p| p.features.iter().find(|f| f.id == profile.node))
                                    .and_then(|f| {
                                        if let Operation::Profile { plane, .. } = &f.operation {
                                            Some(plane)
                                        } else {
                                            None
                                        }
                                    })
                            });
                        if let Some(plane) = plane {
                            *direction = [
                                plane.u[1] * plane.v[2] - plane.u[2] * plane.v[1],
                                plane.u[2] * plane.v[0] - plane.u[0] * plane.v[2],
                                plane.u[0] * plane.v[1] - plane.u[1] * plane.v[0],
                            ]
                            .map(|v| v * if options.reverse_direction { -1. } else { 1. });
                        }
                    }
                    if !options.profile_normal {
                        ui.horizontal(|ui| {
                            for (label, value) in
                                ["X", "Y", "Z"].into_iter().zip(direction.iter_mut())
                            {
                                changed |= ui
                                    .add(
                                        egui::DragValue::new(value)
                                            .prefix(format!("{label} "))
                                            .speed(0.01),
                                    )
                                    .changed();
                            }
                        });
                    }
                    changed |= extrusion_start_fields(ui, w, doc, &feature.id, &mut options.start);
                    changed |= expression(ui, "Start Offset", &mut options.start_offset);
                    let combined = matches!(
                        options.end,
                        cad_core::ExtrudeEnd::MidPlane | cad_core::ExtrudeEnd::ThroughAllBoth
                    );
                    let mut second = options.direction2.is_some()
                        || (options.end == cad_core::ExtrudeEnd::Blind
                            && options.reverse_distance.is_some());
                    if ui
                        .add_enabled(!combined, egui::Checkbox::new(&mut second, "Direction 2"))
                        .on_disabled_hover_text(
                            "The chosen end condition already extends in both directions.",
                        )
                        .changed()
                    {
                        options.direction2 =
                            second.then(cad_core::extrusion::DirectionEnd::default);
                        options.reverse_distance = None;
                        changed = true;
                    }
                    if second
                        && let Some(legacy) = &mut options.reverse_distance
                        && options.direction2.is_none()
                    {
                        let mut end = cad_core::extrusion::DirectionEnd {
                            distance: legacy.clone(),
                            ..Default::default()
                        };
                        if ui
                            .push_id("direction-two", |ui| {
                                extrusion_end_fields(ui, w, doc, &feature.id, &mut end, false)
                            })
                            .inner
                        {
                            options.reverse_distance = None;
                            options.direction2 = Some(end);
                            changed = true;
                        }
                    } else if let Some(end) = &mut options.direction2 {
                        ui.push_id("direction-two", |ui| {
                            changed |= extrusion_end_fields(ui, w, doc, &feature.id, end, false);
                        });
                    }
                },
            );
            options_section(
                ui,
                &feature.id,
                "wall",
                "Thin wall and draft",
                options.thin.is_some() || options.draft.is_some(),
                |ui| {
                    changed |= thin_fields(ui, &mut options.thin);
                    let mut draft = options.draft.is_some();
                    if ui
                        .checkbox(&mut draft, "Draft")
                        .on_hover_text("Available for straight-sided sketch profiles.")
                        .changed()
                    {
                        options.draft = draft.then(|| cad_core::ExtrudeDraft {
                            angle: "3 deg".into(),
                            outward: false,
                        });
                        changed = true;
                    }
                    if let Some(draft) = &mut options.draft {
                        changed |= expression(ui, "Draft Angle", &mut draft.angle);
                        changed |= ui.checkbox(&mut draft.outward, "Draft Outward").changed();
                    }
                },
            );
            changed |= scope_fields(ui, doc, &feature.id, body.as_ref(), &mut options.scope);
        }
        Operation::Revolve {
            profile,
            origin,
            axis,
            axis_reference,
            angle,
            body,
            mode,
            options,
        } => {
            if profile_selector(ui, doc, &feature.id, "Sketch", profile) {
                // Selected contours belong to their sketch. A newly chosen
                // profile starts with all of its own contours.
                options.contours.clear();
                changed = true;
            }
            changed |= expression(ui, "Angle", angle);
            changed |= ui
                .checkbox(&mut options.reverse, "Reverse direction")
                .changed();
            let axis_name = axis_reference.as_ref().map_or_else(
                || {
                    let name = match *axis {
                        [1., 0., 0.] => "X axis",
                        [0., 1., 0.] => "Y axis",
                        [0., 0., 1.] => "Z axis",
                        _ => "Custom direction",
                    };
                    if *origin == [0.; 3] {
                        name.into()
                    } else {
                        format!("{name}, offset origin")
                    }
                },
                |source| crate::reference_ui::axis_label(doc, source),
            );
            options_section(
                ui,
                &feature.id,
                "axis",
                &format!("Revolution axis · {axis_name}"),
                false,
                |ui| {
                    changed |= crate::reference_ui::axis_fields(
                        ui,
                        w,
                        doc,
                        &feature.id,
                        axis,
                        Some(origin),
                        axis_reference,
                    );
                },
            );
            changed |= result_fields(ui, doc, &feature.id, body, mode);
            changed |= crate::contours_ui::fields(
                ui,
                w,
                doc,
                &feature.id,
                std::slice::from_ref(profile),
                &mut options.contours,
                options.thin.is_some(),
            );
            options_section(
                ui,
                &feature.id,
                "direction",
                "Start angle and directions",
                options.start_angle.is_some() || options.midplane || options.second_angle.is_some(),
                |ui| {
                    if ui.checkbox(&mut options.midplane, "Mid Plane").changed() {
                        if options.midplane {
                            options.second_angle = None;
                        }
                        changed = true;
                    }
                    let mut second = options.second_angle.is_some();
                    if ui
                        .add_enabled(
                            !options.midplane,
                            egui::Checkbox::new(&mut second, "Direction 2"),
                        )
                        .on_disabled_hover_text("Mid-plane already revolves in both directions.")
                        .changed()
                    {
                        options.second_angle = second.then(|| "90 deg".into());
                        changed = true;
                    }
                    if let Some(angle) = &mut options.second_angle {
                        changed |= expression(ui, "Direction 2 Angle", angle);
                    }
                    let mut enabled = options.start_angle.is_some();
                    if ui.checkbox(&mut enabled, "Start at an angle").changed() {
                        options.start_angle = enabled.then(|| "0 deg".into());
                        changed = true;
                    }
                    if let Some(angle) = &mut options.start_angle {
                        changed |= expression(ui, "Start angle", angle);
                    }
                },
            );
            options_section(
                ui,
                &feature.id,
                "wall",
                "Thin wall",
                options.thin.is_some(),
                |ui| {
                    changed |= thin_fields(ui, &mut options.thin);
                },
            );
            changed |= scope_fields(ui, doc, &feature.id, body.as_ref(), &mut options.scope);
        }
        _ => {}
    }
    changed
}

fn sketch_vertices(doc: &Document, current: &str) -> Vec<((NodeRef, cad_core::Id), String)> {
    doc.parts
        .values()
        .flat_map(|part| {
            part.features
                .iter()
                .take_while(|f| f.id != current)
                .filter(|f| !f.suppressed)
                .filter_map(move |feature| {
                    if let Operation::Profile {
                        profile: cad_core::Profile::Sketch { sketch },
                        ..
                    } = &feature.operation
                    {
                        part.sketches
                            .get(sketch)
                            .map(|sketch| (part, feature, sketch))
                    } else {
                        None
                    }
                })
                .flat_map(|(part, feature, sketch)| {
                    sketch.points.iter().map(move |(id, p)| {
                        (
                            (part.reference(&feature.id), id.clone()),
                            format!(
                                "{} / {} ({}, {})",
                                part.name,
                                feature.name,
                                doc.settings.format_length(p[0]),
                                doc.settings.format_length(p[1])
                            ),
                        )
                    })
                })
        })
        .collect()
}

fn extrusion_vertex_field(
    ui: &mut egui::Ui,
    doc: &cad_core::Document,
    current: &str,
    label: &str,
    selected: &mut Option<(cad_core::NodeRef, cad_core::Id)>,
) -> bool {
    let vertices = sketch_vertices(doc, current);
    let name = selected
        .as_ref()
        .and_then(|selected| vertices.iter().find(|(value, _)| value == selected))
        .map(|(_, name)| name.as_str())
        .unwrap_or("Select sketch vertex");
    let mut changed = false;
    egui::ComboBox::from_label(label)
        .selected_text(name)
        .show_ui(ui, |ui| {
            for (value, name) in vertices {
                changed |= ui.selectable_value(selected, Some(value), name).changed();
            }
        });
    changed
}
fn extrusion_end_fields(
    ui: &mut egui::Ui,
    w: &Workspace,
    doc: &cad_core::Document,
    current: &str,
    end: &mut cad_core::extrusion::DirectionEnd,
    combined: bool,
) -> bool {
    use cad_core::ExtrudeEnd;
    let mut changed = false;
    let before = end.end.clone();
    let name = match end.end {
        ExtrudeEnd::Blind => "Blind",
        ExtrudeEnd::MidPlane => "Mid-plane",
        ExtrudeEnd::ThroughAll => "Through all",
        ExtrudeEnd::ThroughAllBoth => "Through all (both)",
        ExtrudeEnd::UpToFace => "Up to face",
        ExtrudeEnd::UpToVertex => "Up to vertex",
        ExtrudeEnd::UpToNext => "Up to next face",
        ExtrudeEnd::UpToBody => "Up to body",
    };
    egui::ComboBox::from_label("End Condition")
        .selected_text(name)
        .show_ui(ui, |ui| {
            for (value, name) in [
                (ExtrudeEnd::Blind, "Blind"),
                (ExtrudeEnd::MidPlane, "Mid-plane"),
                (ExtrudeEnd::ThroughAll, "Through all"),
                (ExtrudeEnd::ThroughAllBoth, "Through all (both)"),
                (ExtrudeEnd::UpToFace, "Up to face"),
                (ExtrudeEnd::UpToVertex, "Up to vertex"),
                (ExtrudeEnd::UpToNext, "Up to next face"),
                (ExtrudeEnd::UpToBody, "Up to body"),
            ] {
                if combined || !matches!(value, ExtrudeEnd::MidPlane | ExtrudeEnd::ThroughAllBoth) {
                    changed |= ui.selectable_value(&mut end.end, value, name).changed();
                }
            }
        });
    if before != end.end {
        if end.end != ExtrudeEnd::UpToFace {
            end.target = None;
        }
        if !matches!(end.end, ExtrudeEnd::UpToFace | ExtrudeEnd::UpToBody) {
            end.target_body = None;
        }
        if end.end != ExtrudeEnd::UpToVertex {
            end.target_vertex = None;
        }
    }
    match end.end {
        ExtrudeEnd::Blind | ExtrudeEnd::MidPlane => {
            let label = if end.end == ExtrudeEnd::MidPlane {
                "Total depth"
            } else {
                "Depth"
            };
            changed |= controls::expression(ui, "feature-field/depth", label, &mut end.distance);
        }
        ExtrudeEnd::UpToFace => {
            if let Some(reference) = &end.target {
                let name = doc
                    .parts
                    .get(&reference.producer.definition)
                    .and_then(|part| {
                        part.features
                            .iter()
                            .find(|feature| feature.id == reference.producer.node)
                    })
                    .map(|feature| feature.name.as_str());
                ui.label(format!(
                    "End {} on {}",
                    reference.topology,
                    name.unwrap_or("saved reference")
                ));
            } else {
                ui.label("Select an end face or plane.");
            }
            if let Some(selected) = w.selected.as_ref().filter(|s| {
                matches!(s.reference.topology.as_str(), "face" | "plane")
                    && s.reference.producer.node != current
            }) && ui.button("Use selected face or plane").clicked()
            {
                end.target = Some(selected.reference.clone());
                end.target_body = Some(crate::reference_ui::selected_body(w, selected));
                changed = true;
            }
            changed |= expression(ui, "Offset from End Face", &mut end.end_offset);
        }
        ExtrudeEnd::UpToVertex => {
            changed |=
                extrusion_vertex_field(ui, doc, current, "End Vertex", &mut end.target_vertex);
            changed |= expression(ui, "Offset from End Vertex", &mut end.end_offset);
        }
        ExtrudeEnd::UpToBody => {
            changed |= body_selector(ui, doc, current, "Termination Body", &mut end.target_body);
            changed |= expression(ui, "Offset from Body", &mut end.end_offset);
        }
        ExtrudeEnd::UpToNext => {
            changed |= expression(ui, "Offset from Next Face", &mut end.end_offset);
        }
        ExtrudeEnd::ThroughAll | ExtrudeEnd::ThroughAllBoth => {}
    };
    changed
}
fn start_vertex_choices(
    ui: &mut egui::Ui,
    doc: &Document,
    current: &str,
    start: &cad_core::extrusion::Start,
) -> Option<(NodeRef, cad_core::Id)> {
    let vertices = sketch_vertices(doc, current);
    if vertices.is_empty() {
        ui.weak("This model has no sketch vertices.");
        return None;
    }
    let mut picked = None;
    egui::ScrollArea::vertical().max_height(230.).show_rows(
        ui,
        ui.spacing().interact_size.y,
        vertices.len(),
        |ui, rows| {
            for (reference, label) in &vertices[rows] {
                let selected = match start {
                    cad_core::extrusion::Start::Vertex { profile, point } => {
                        profile == &reference.0 && point == &reference.1
                    }
                    _ => false,
                };
                if ui.selectable_label(selected, label).clicked() {
                    picked = Some(reference.clone());
                }
            }
        },
    );
    picked
}
fn extrusion_start_fields(
    ui: &mut egui::Ui,
    w: &Workspace,
    doc: &cad_core::Document,
    current: &str,
    start: &mut cad_core::extrusion::Start,
) -> bool {
    use cad_core::extrusion::Start;
    let mut changed = false;
    let picked = w.selected.as_ref().filter(|s| {
        matches!(s.reference.topology.as_str(), "face" | "plane")
            && s.reference.producer.node != current
    });
    egui::ComboBox::from_label("From")
        .selected_text(match start {
            Start::SketchPlane => "Sketch Plane",
            Start::Face { .. } => "Face / Plane",
            Start::Vertex { .. } => "Vertex",
            Start::Point { .. } => "Reference Point",
        })
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show_ui(ui, |ui| {
            if ui
                .selectable_label(matches!(start, Start::SketchPlane), "Sketch Plane")
                .clicked()
            {
                changed |= !matches!(start, Start::SketchPlane);
                *start = Start::SketchPlane;
                ui.close();
            }
            if ui
                .selectable_label(matches!(start, Start::Point { .. }), "Reference Point")
                .clicked()
            {
                if !matches!(start, Start::Point { .. }) {
                    *start = Start::Point {
                        point: Box::default(),
                    };
                    changed = true;
                }
                ui.close();
            }
            if ui
                .add_enabled(
                    picked.is_some(),
                    egui::Button::new("Selected face or plane"),
                )
                .on_disabled_hover_text("Select a face or plane on the model first.")
                .clicked()
                && let Some(selected) = picked
            {
                *start = Start::Face {
                    face: Box::new(selected.reference.clone()),
                    body: crate::reference_ui::selected_body(w, selected),
                };
                changed = true;
                ui.close();
            }
            let vertices = egui::CollapsingHeader::new("Sketch vertices")
                .id_salt("extrusion/start-vertex")
                .show(ui, |ui| start_vertex_choices(ui, doc, current, start));
            if let Some((profile, point)) = vertices.body_returned.flatten() {
                *start = Start::Vertex { profile, point };
                changed = true;
                ui.close();
            }
        });

    if let Start::Point { point } = start {
        changed |= crate::reference_ui::point(ui, w, doc, current, "Start Point", point);
    }
    if let Start::Vertex { profile, point } = start
        && let Some(part) = doc.parts.get(&profile.definition)
        && let Some(source) = part
            .features
            .iter()
            .find(|feature| feature.id == profile.node)
        && let Operation::Profile {
            profile: cad_core::Profile::Sketch { sketch },
            ..
        } = &source.operation
        && let Some(coords) = part
            .sketches
            .get(sketch)
            .and_then(|sketch| sketch.points.get(point))
    {
        ui.label(format!(
            "{} ({}, {})",
            source.name,
            doc.settings.format_length(coords[0]),
            doc.settings.format_length(coords[1])
        ));
    }
    changed
}
pub(crate) fn thin_fields(ui: &mut egui::Ui, thin: &mut Option<cad_core::ThinExtrude>) -> bool {
    let mut changed = false;
    let mut enabled = thin.is_some();
    if ui.checkbox(&mut enabled, "Thin Feature").changed() {
        *thin = enabled.then(|| cad_core::ThinExtrude {
            mode: cad_core::ThinExtrudeMode::OneSide,
            thickness: "2 mm".into(),
            second_thickness: None,
            reverse: false,
        });
        changed = true;
    }
    if let Some(thin) = thin {
        let prior_mode = thin.mode.clone();
        egui::ComboBox::from_label("Thickness Mode")
            .selected_text(match thin.mode {
                cad_core::ThinExtrudeMode::OneSide => "One Side",
                cad_core::ThinExtrudeMode::TwoSide => "Two Sides",
                cad_core::ThinExtrudeMode::MidPlane => "Mid Plane",
            })
            .show_ui(ui, |ui| {
                for (mode, label) in [
                    (cad_core::ThinExtrudeMode::OneSide, "One Side"),
                    (cad_core::ThinExtrudeMode::TwoSide, "Two Sides"),
                    (cad_core::ThinExtrudeMode::MidPlane, "Mid Plane"),
                ] {
                    changed |= ui.selectable_value(&mut thin.mode, mode, label).changed();
                }
            });
        let label = match thin.mode {
            cad_core::ThinExtrudeMode::MidPlane => "Total thickness",
            cad_core::ThinExtrudeMode::TwoSide => "Side 1 thickness",
            _ => "Thickness",
        };
        changed |= expression(ui, label, &mut thin.thickness);
        if thin.mode == cad_core::ThinExtrudeMode::TwoSide {
            if thin.second_thickness.is_none() && thin.mode != prior_mode {
                thin.second_thickness = Some("2 mm".into());
            }
            let mut second = thin.second_thickness.clone().unwrap_or_else(|| "".into());
            if expression(ui, "Thickness 2", &mut second) {
                thin.second_thickness = Some(second);
                changed = true;
            }
        }
        if thin.mode != cad_core::ThinExtrudeMode::MidPlane {
            changed |= ui
                .checkbox(&mut thin.reverse, "Reverse thickness side")
                .changed();
        }
    }
    changed
}
