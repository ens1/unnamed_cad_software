//! Docked authoring over shared feature drafts, without independent windows or state.
use crate::{Workspace, reference_ui, ui};
use bevy_egui::egui;
use cad_core::{commands::Command, reference_curves::*, reference_geometry::*, *};
fn expression(ui: &mut egui::Ui, label: &str, value: &mut Expr) -> bool {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.text_edit_singleline(&mut value.0).changed()
    })
    .inner
}
fn feature_name(doc: &Document, node: &NodeRef) -> String {
    doc.parts
        .get(&node.definition)
        .and_then(|part| part.features.iter().find(|f| f.id == node.node))
        .map(|f| f.name.clone())
        .unwrap_or_else(|| "Missing source".into())
}
pub(crate) fn source_label(doc: &Document, source: &CurveSource) -> String {
    match source {
        CurveSource::SketchMember { profile, member } => {
            let Some(part) = doc.parts.get(&profile.definition) else {
                return "Missing sketch source".into();
            };
            let feature = part.features.iter().find(|f| f.id == profile.node);
            let ordinal = feature.and_then(|f| {
                if let Operation::Profile {
                    profile: Profile::Sketch { sketch },
                    ..
                } = &f.operation
                {
                    part.sketches
                        .get(sketch)?
                        .members
                        .keys()
                        .position(|id| id == member)
                } else {
                    None
                }
            });
            format!(
                "{} · member {}",
                feature_name(doc, profile),
                ordinal
                    .map(|n| (n + 1).to_string())
                    .unwrap_or_else(|| "missing".into())
            )
        }
        CurveSource::ModelEdge { body, .. } => {
            format!("{} · selected edge", feature_name(doc, body))
        }
        CurveSource::Curve { feature } => feature_name(doc, feature),
    }
}
pub(crate) fn sources(
    doc: &Document,
    definition: &str,
    current: &str,
) -> Vec<(String, CurveSource)> {
    let mut out = Vec::new();
    let Some(part) = doc.parts.get(definition) else {
        return out;
    };
    for feature in part
        .features
        .iter()
        .take_while(|f| f.id != current)
        .filter(|f| !f.suppressed)
    {
        match &feature.operation {
            Operation::ReferenceCurve { .. } => out.push((
                feature.name.clone(),
                CurveSource::Curve {
                    feature: part.reference(&feature.id),
                },
            )),
            Operation::Profile {
                profile: Profile::Sketch { sketch },
                ..
            } => {
                if let Some(sketch) = part.sketches.get(sketch) {
                    let suppressed = sketch_transform::suppressed_members(sketch);
                    for (index, (member, record)) in sketch
                        .members
                        .iter()
                        .enumerate()
                        .filter(|(_, (id, _))| !suppressed.contains(*id))
                    {
                        let kind = match record.curve {
                            SketchCurve::Line { .. } => "Line",
                            SketchCurve::Circle { .. } => "Circle",
                            SketchCurve::Arc { .. } => "Arc",
                            SketchCurve::Ellipse { .. } => "Ellipse",
                            SketchCurve::Bezier { .. } => "Spline",
                            SketchCurve::Nurbs { .. } => "Rational spline",
                        };
                        out.push((
                            format!("{} · {} {}", feature.name, kind, index + 1),
                            CurveSource::SketchMember {
                                profile: part.reference(&feature.id),
                                member: member.clone(),
                            },
                        ));
                    }
                }
            }
            _ => {}
        }
    }
    out
}
pub(crate) fn selected_edge(w: &Workspace, current: &str) -> Option<CurveSource> {
    let selected = w.selected.as_ref()?;
    if selected.reference.topology != "edge"
        || selected.definition != *w.view.active_part.as_ref()?
    {
        return None;
    }
    let body = reference_ui::selected_body(w, selected);
    if body.node == current {
        return None;
    }
    Some(CurveSource::ModelEdge {
        body,
        reference: Box::new(selected.reference.clone()),
    })
}
fn source_closed(w: &Workspace, source: &CurveSource) -> bool {
    let doc = w.session.visible();
    match source {
        CurveSource::SketchMember { profile, member } => doc
            .parts
            .get(&profile.definition)
            .and_then(|part| {
                part.features
                    .iter()
                    .find(|feature| feature.id == profile.node)
                    .and_then(|feature| match &feature.operation {
                        Operation::Profile {
                            profile: Profile::Sketch { sketch },
                            ..
                        } => part.sketches.get(sketch),
                        _ => None,
                    })
            })
            .and_then(|sketch| sketch.members.get(member))
            .is_some_and(|member| match &member.curve {
                SketchCurve::Circle { .. } | SketchCurve::Ellipse { .. } => true,
                SketchCurve::Bezier { points } | SketchCurve::Nurbs { points, .. } => {
                    points.first() == points.last()
                }
                _ => false,
            }),
        CurveSource::Curve { feature } => doc
            .parts
            .get(&feature.definition)
            .and_then(|part| {
                part.features
                    .iter()
                    .find(|record| record.id == feature.node)
            })
            .is_some_and(|record| match &record.operation {
                Operation::ReferenceCurve { definition } => match definition.as_ref() {
                    Definition::Composite { closed, .. }
                    | Definition::ThroughPoints { closed, .. } => *closed,
                    Definition::Helix { .. } => crate::history::current(w)
                        .and_then(|evaluation| {
                            evaluation
                                .reference_curves
                                .get(&format!("{}::{}", feature.definition, feature.node))
                        })
                        .is_some_and(|curve| curve.closed),
                },
                _ => false,
            }),
        CurveSource::ModelEdge { .. } => false,
    }
}
pub fn fields(ui: &mut egui::Ui, w: &Workspace, feature: &mut Feature) -> bool {
    let Operation::ReferenceCurve { definition } = &mut feature.operation else {
        return false;
    };
    let doc = w.session.visible();
    let owner = w.view.active_part.as_deref().unwrap_or_default();
    let current = &feature.id;
    let mut changed = false;
    match definition.as_mut() {
        Definition::Composite {
            segments,
            closed,
            join_tolerance,
        } => {
            ui.small("Ordered exact source carriers. Reverse individual sources to connect their endpoints; no gap repair or fitted substitute geometry.");
            let candidates = sources(doc, owner, current);
            let mut remove = None;
            let mut movement = None;
            for (index, segment) in segments.iter_mut().enumerate() {
                ui.push_id(&segment.id, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(format!("Source {}", index + 1));
                        if ui.small_button("↑").clicked() && index > 0 {
                            movement = Some((index, index - 1));
                        }
                        if ui.small_button("↓").clicked() {
                            movement = Some((index, index + 1));
                        }
                        if ui.small_button("Remove").clicked() {
                            remove = Some(index);
                        }
                    });
                    egui::ComboBox::from_id_salt("curve-source")
                        .selected_text(source_label(doc, &segment.source))
                        .show_ui(ui, |ui| {
                            for (label, source) in &candidates {
                                changed |= ui
                                    .selectable_value(&mut segment.source, source.clone(), label)
                                    .changed();
                            }
                        });
                    if let Some(picked) = selected_edge(w, current)
                        && ui.button("Use selected model edge").clicked()
                    {
                        segment.source = picked;
                        changed = true;
                    }
                    changed |= ui
                        .checkbox(&mut segment.reverse, "Reverse source direction")
                        .changed();
                });
            }
            if let Some(index) = remove {
                segments.remove(index);
                changed = true;
            } else if let Some((a, b)) = movement
                && b < segments.len()
            {
                segments.swap(a, b);
                changed = true;
            }
            ui.horizontal_wrapped(|ui| {
                if let Some(picked) = selected_edge(w, current)
                    && ui
                        .add_enabled(
                            segments.len() < MAX_SOURCES,
                            egui::Button::new("Add selected edge"),
                        )
                        .clicked()
                {
                    segments.push(Segment {
                        id: id(),
                        source: picked,
                        reverse: false,
                    });
                    changed = true;
                }
                egui::ComboBox::from_id_salt("add-composite-source")
                    .selected_text("Add sketch / reference curve")
                    .show_ui(ui, |ui| {
                        for (label, source) in candidates {
                            if ui
                                .add_enabled(segments.len() < MAX_SOURCES, egui::Button::new(label))
                                .clicked()
                            {
                                segments.push(Segment {
                                    id: id(),
                                    source,
                                    reverse: false,
                                });
                                changed = true;
                            }
                        }
                    });
            });
            changed |= ui.checkbox(closed, "Closed curve").changed();
            changed |= expression(ui, "Endpoint join tolerance", join_tolerance);
            ui.small("Join tolerance: 1e-10–1e-6 mm. Source order and identities remain saved when edited.");
        }
        Definition::ThroughPoints {
            points,
            interpolation,
            closed,
        } => {
            egui::ComboBox::from_label("Curve type")
                .selected_text(match interpolation {
                    Interpolation::Linear => "Straight segments",
                    Interpolation::NaturalCubic => "Interpolating cubic spline",
                })
                .show_ui(ui, |ui| {
                    changed |= ui
                        .selectable_value(interpolation, Interpolation::Linear, "Straight segments")
                        .changed();
                    changed |= ui
                        .selectable_value(
                            interpolation,
                            Interpolation::NaturalCubic,
                            "Interpolating cubic spline",
                        )
                        .changed();
                });
            ui.small("Associative 3D points in authored order. Open splines use natural end curvature; closed splines use periodic curvature.");
            let mut remove = None;
            let mut movement = None;
            for (index, record) in points.iter_mut().enumerate() {
                ui.push_id(&record.id, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.strong(format!("Point {}", index + 1));
                        if ui.small_button("↑").clicked() && index > 0 {
                            movement = Some((index, index - 1));
                        }
                        if ui.small_button("↓").clicked() {
                            movement = Some((index, index + 1));
                        }
                        if ui.small_button("Remove").clicked() {
                            remove = Some(index);
                        }
                    });
                    changed |=
                        reference_ui::point(ui, w, doc, current, "Point source", &mut record.point);
                });
            }
            if let Some(index) = remove {
                points.remove(index);
                changed = true;
            } else if let Some((a, b)) = movement
                && b < points.len()
            {
                points.swap(a, b);
                changed = true;
            }
            if ui
                .add_enabled(points.len() < MAX_SOURCES, egui::Button::new("Add point"))
                .clicked()
            {
                points.push(PointRecord {
                    id: id(),
                    point: PointSource::Coordinates {
                        coordinates: [
                            Expr(format!("{} mm", points.len() * 10)),
                            "0 mm".into(),
                            "0 mm".into(),
                        ],
                    },
                });
                changed = true;
            }
            changed |= ui.checkbox(closed, "Closed curve").changed();
        }
        Definition::Helix {
            axis,
            radial,
            radius,
            end_radius,
            pitch,
            turns,
            start_angle,
            reverse,
        } => {
            changed |= reference_ui::axis(ui, w, doc, current, "Helix axis", axis);
            changed |=
                reference_ui::direction(ui, w, doc, current, "Start radial direction", radial);
            changed |= expression(ui, "Start radius", radius);
            let mut tapered = end_radius.is_some();
            if ui
                .checkbox(&mut tapered, "Change radius along the curve")
                .changed()
            {
                *end_radius = tapered.then(|| radius.clone());
                changed = true;
            }
            if let Some(end) = end_radius {
                changed |= expression(ui, "End radius", end);
            }
            changed |= expression(ui, "Pitch per revolution", pitch);
            changed |= expression(ui, "Revolutions", turns);
            changed |= expression(ui, "Start angle", start_angle);
            changed |= ui.checkbox(reverse, "Reverse winding direction").changed();
            ui.small("Exact constant-pitch analytic helix; pitch 0 produces a planar spiral/circle. Height follows signed pitch, winding follows Reverse. Positive start/end radii; up to 256 turns. A constant-radius, zero-pitch circle supports one turn maximum.");
            ui.small("The analytic carrier remains exact. Rational-only sweep and projection modes reject this carrier until they support the analytic definition.");
        }
    }
    changed
}
pub fn preset(w: &mut Workspace, name: &str) -> bool {
    if !matches!(
        name,
        "Composite Curve" | "Curve Through Points" | "Helix / Spiral"
    ) {
        return false;
    }
    if w.confirm_requested {
        w.after_confirm = Some(name.into());
        return true;
    }
    if w.sketch.is_some()
        || w.tool_checkpoint.is_some()
        || w.fields.contains_key("configuration-editor")
        || w.fields.contains_key("component-pattern-editor")
        || w.fields.contains_key("mate-editor")
    {
        w.status =
            "Confirm or cancel the current operation before creating a reference curve".into();
        return true;
    }
    crate::history::latest(w);
    let Some(part) = w
        .view
        .active_part
        .as_ref()
        .and_then(|id| w.session.visible().parts.get(id))
        .cloned()
    else {
        w.status = "Create or select a part first".into();
        return true;
    };
    if w.session
        .visible()
        .libraries
        .values()
        .any(|link| link.pinned_definitions.contains_key(&part.id))
    {
        w.status = "Make this linked component unique before adding a reference curve".into();
        return true;
    }
    let definition = match name {
        "Composite Curve" => {
            let candidates = sources(w.session.visible(), &part.id, "");
            let source = selected_edge(w, "").or_else(|| {
                candidates
                    .iter()
                    .find(|(_, source)| {
                        w.view.active_node.as_ref() == Some(&source.dependency().node)
                    })
                    .or_else(|| candidates.first())
                    .map(|(_, source)| source.clone())
            });
            let Some(source) = source else {
                w.status =
                    "Draw a sketch or select a model edge before creating a Composite Curve".into();
                return true;
            };
            let closed = source_closed(w, &source);
            Definition::Composite {
                segments: vec![Segment {
                    id: id(),
                    source,
                    reverse: false,
                }],
                closed,
                join_tolerance: "0.000001 mm".into(),
            }
        }
        "Curve Through Points" => Definition::ThroughPoints {
            points: (0..3)
                .map(|index| PointRecord {
                    id: id(),
                    point: PointSource::Coordinates {
                        coordinates: [
                            Expr(format!("{} mm", index * 10)),
                            Expr(format!("{} mm", if index == 1 { 10 } else { 0 })),
                            "0 mm".into(),
                        ],
                    },
                })
                .collect(),
            interpolation: Interpolation::NaturalCubic,
            closed: false,
        },
        _ => Definition::Helix {
            axis: Box::new(AxisSource::Origin {
                axis: OriginAxis::Z,
            }),
            radial: Box::new(DirectionSource::Explicit {
                direction: ["1".into(), "0".into(), "0".into()],
            }),
            radius: "10 mm".into(),
            end_radius: None,
            pitch: "5 mm".into(),
            turns: "3".into(),
            start_angle: "0 deg".into(),
            reverse: false,
        },
    };
    let number = part
        .features
        .iter()
        .filter(|f| f.name.starts_with(name))
        .count()
        + 1;
    let feature = Feature {
        id: id(),
        name: format!("{name}{number}"),
        suppressed: false,
        operation: Operation::ReferenceCurve {
            definition: Box::new(definition),
        },
    };
    w.start_tool();
    w.dispatch(Command::PutFeature {
        definition: part.id.clone(),
        feature: feature.clone().into(),
        output: false,
    });
    if !w
        .session
        .visible()
        .parts
        .get(&part.id)
        .is_some_and(|part| part.features.iter().any(|current| current == &feature))
    {
        let error = w.status.clone();
        w.cancel_tool();
        w.status = error;
        return true;
    }
    w.view.active_node = Some(feature.id.clone());
    ui::feature_dialog(w, Some(feature));
    true
}
