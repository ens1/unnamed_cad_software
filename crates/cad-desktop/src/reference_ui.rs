//! Native controls over semantic, application-owned datum definitions.
use crate::{Workspace, ui, viewport};
use bevy::math::DVec3;
use bevy_egui::egui;
use cad_core::{
    Document, Expr, Feature, NodeRef, Operation, commands::Command, reference_geometry::*,
};

fn expression(ui: &mut egui::Ui, label: &str, value: &mut Expr) -> bool {
    crate::presentation_controls::expression(
        ui,
        &format!("reference_ui/{}", label.to_lowercase().replace(' ', "-")),
        label,
        value,
    )
}
fn candidates<'a>(
    doc: &'a Document,
    definition: &'a str,
    current: &'a str,
) -> impl Iterator<Item = &'a Feature> {
    doc.parts.get(definition).into_iter().flat_map(move |part| {
        part.features
            .iter()
            .take_while(move |f| f.id != current)
            .filter(|f| !f.suppressed)
    })
}
fn name(doc: &Document, reference: &NodeRef) -> String {
    doc.parts
        .get(&reference.definition)
        .and_then(|p| p.features.iter().find(|f| f.id == reference.node))
        .map(|f| f.name.clone())
        .unwrap_or("Missing reference".into())
}
fn plane_label(doc: &Document, source: &PlaneSource) -> String {
    match source {
        PlaneSource::Origin { plane } => plane.label().into(),
        PlaneSource::Datum { feature } => name(doc, feature),
        PlaneSource::CoordinateSystem { feature, plane } => {
            format!("{} · {}", name(doc, feature), plane.label())
        }
        PlaneSource::Face { .. } => "Selected planar face".into(),
    }
}
fn axis_label(doc: &Document, source: &AxisSource) -> String {
    match source {
        AxisSource::Origin { axis } => axis.label().into(),
        AxisSource::Datum { feature } => name(doc, feature),
        AxisSource::CoordinateSystem { feature, axis } => {
            format!("{} · {}", name(doc, feature), axis.label())
        }
        AxisSource::Edge { .. } => "Selected straight edge".into(),
        AxisSource::FaceAxis { .. } => "Selected rotational face axis".into(),
        AxisSource::CircleEdge { .. } => "Selected circular edge axis".into(),
        AxisSource::SketchLine { profile, .. } => format!("{} · sketch line", name(doc, profile)),
        AxisSource::SketchCircle { profile, .. } => {
            format!("{} · sketch circle axis", name(doc, profile))
        }
    }
}
pub(crate) fn plane(
    ui: &mut egui::Ui,
    w: &Workspace,
    doc: &Document,
    current: &str,
    label: &str,
    source: &mut PlaneSource,
) -> bool {
    let mut changed = false;
    let definition = w.view.active_part.as_deref().unwrap_or_default();
    ui.push_id(label, |ui| {
        egui::ComboBox::from_label(label)
            .selected_text(plane_label(doc, source))
            .show_ui(ui, |ui| {
                for p in [OriginPlane::Front, OriginPlane::Top, OriginPlane::Right] {
                    changed |= ui
                        .selectable_value(source, PlaneSource::Origin { plane: p }, p.label())
                        .changed();
                }
                for f in candidates(doc, definition, current)
                    .filter(|f| matches!(f.operation, Operation::ReferencePlane { .. }))
                {
                    changed |= ui
                        .selectable_value(
                            source,
                            PlaneSource::Datum {
                                feature: NodeRef {
                                    definition: definition.into(),
                                    node: f.id.clone(),
                                },
                            },
                            &f.name,
                        )
                        .changed();
                }
                for f in candidates(doc, definition, current)
                    .filter(|f| matches!(f.operation, Operation::ReferenceCoordinateSystem { .. }))
                {
                    for p in [OriginPlane::Top, OriginPlane::Front, OriginPlane::Right] {
                        changed |= ui
                            .selectable_value(
                                source,
                                PlaneSource::CoordinateSystem {
                                    feature: NodeRef {
                                        definition: definition.into(),
                                        node: f.id.clone(),
                                    },
                                    plane: p,
                                },
                                format!("{} · {}", f.name, p.label()),
                            )
                            .changed();
                    }
                }
            });
        if let Some(selected) = w.selected.as_ref().filter(|s| s.definition == definition) {
            let picked = match selected.reference.topology.as_str() {
                "face" => Some(PlaneSource::Face {
                    reference: selected.reference.clone(),
                    body: selected_body(w, selected),
                }),
                "plane" if selected.reference.producer.node != current => {
                    PlaneSource::from_datum(&selected.reference).ok()
                }
                _ => None,
            };
            if let Some(picked) = picked
                && ui.button("Use selected plane / face").clicked()
            {
                *source = picked;
                changed = true;
            }
        }
    });
    changed
}
pub(crate) fn axis(
    ui: &mut egui::Ui,
    w: &Workspace,
    doc: &Document,
    current: &str,
    label: &str,
    source: &mut AxisSource,
) -> bool {
    let mut changed = false;
    let definition = w.view.active_part.as_deref().unwrap_or_default();
    ui.push_id(label, |ui| {
        egui::ComboBox::from_label(label)
            .selected_text(axis_label(doc, source))
            .show_ui(ui, |ui| {
                for a in [OriginAxis::X, OriginAxis::Y, OriginAxis::Z] {
                    changed |= ui
                        .selectable_value(source, AxisSource::Origin { axis: a }, a.label())
                        .changed();
                }
                for f in candidates(doc, definition, current)
                    .filter(|f| matches!(f.operation, Operation::ReferenceAxis { .. }))
                {
                    changed |= ui
                        .selectable_value(
                            source,
                            AxisSource::Datum {
                                feature: NodeRef {
                                    definition: definition.into(),
                                    node: f.id.clone(),
                                },
                            },
                            &f.name,
                        )
                        .changed();
                }
                for f in candidates(doc, definition, current)
                    .filter(|f| matches!(f.operation, Operation::ReferenceCoordinateSystem { .. }))
                {
                    for a in [OriginAxis::X, OriginAxis::Y, OriginAxis::Z] {
                        changed |= ui
                            .selectable_value(
                                source,
                                AxisSource::CoordinateSystem {
                                    feature: NodeRef {
                                        definition: definition.into(),
                                        node: f.id.clone(),
                                    },
                                    axis: a,
                                },
                                format!("{} · {}", f.name, a.label()),
                            )
                            .changed();
                    }
                }
                for f in candidates(doc, definition, current) {
                    if let Operation::Sketch3d { definition: sketch } = &f.operation {
                        for (index, (member, entity)) in sketch.members.iter().enumerate() {
                            let axis = match &entity.curve {
                                cad_core::sketch3d::Curve::Line { .. } => {
                                    Some(AxisSource::SketchLine {
                                        profile: NodeRef {
                                            definition: definition.into(),
                                            node: f.id.clone(),
                                        },
                                        member: member.clone(),
                                    })
                                }
                                cad_core::sketch3d::Curve::Arc { .. }
                                | cad_core::sketch3d::Curve::Circle { .. } => {
                                    Some(AxisSource::SketchCircle {
                                        profile: NodeRef {
                                            definition: definition.into(),
                                            node: f.id.clone(),
                                        },
                                        member: member.clone(),
                                    })
                                }
                                _ => None,
                            };
                            if let Some(axis) = axis {
                                changed |= ui
                                    .selectable_value(
                                        source,
                                        axis,
                                        format!("{} · 3D member {}", f.name, index + 1),
                                    )
                                    .changed();
                            }
                        }
                    }
                    if let Operation::Profile {
                        profile: cad_core::Profile::Sketch { sketch },
                        ..
                    } = &f.operation
                        && let Some(sketch) = doc.parts[definition].sketches.get(sketch)
                    {
                        for (index, (member, entity)) in
                            sketch.members.iter().enumerate().filter(|(_, (_, m))| {
                                matches!(
                                    m.curve,
                                    cad_core::SketchCurve::Line { .. }
                                        | cad_core::SketchCurve::Circle { .. }
                                        | cad_core::SketchCurve::Arc { .. }
                                )
                            })
                        {
                            changed |= ui
                                .selectable_value(
                                    source,
                                    if matches!(entity.curve, cad_core::SketchCurve::Line { .. }) {
                                        AxisSource::SketchLine {
                                            profile: NodeRef {
                                                definition: definition.into(),
                                                node: f.id.clone(),
                                            },
                                            member: member.clone(),
                                        }
                                    } else {
                                        AxisSource::SketchCircle {
                                            profile: NodeRef {
                                                definition: definition.into(),
                                                node: f.id.clone(),
                                            },
                                            member: member.clone(),
                                        }
                                    },
                                    format!(
                                        "{} · {} {}",
                                        f.name,
                                        if entity.construction {
                                            "Centerline"
                                        } else if matches!(
                                            entity.curve,
                                            cad_core::SketchCurve::Line { .. }
                                        ) {
                                            "Line"
                                        } else {
                                            "Circle / arc axis"
                                        },
                                        index + 1
                                    ),
                                )
                                .changed();
                        }
                        for (index, member) in sketch.circles.keys().enumerate() {
                            changed |= ui
                                .selectable_value(
                                    source,
                                    AxisSource::SketchCircle {
                                        profile: NodeRef {
                                            definition: definition.into(),
                                            node: f.id.clone(),
                                        },
                                        member: member.clone(),
                                    },
                                    format!("{} · Circle axis {}", f.name, index + 1),
                                )
                                .changed();
                        }
                    }
                }
            });
        if let Some(selected) = w.selected.as_ref().filter(|s| s.definition == definition) {
            let picked = match selected.reference.topology.as_str() {
                "edge" | "face" => Some(picked_axis(w, selected)),
                "axis" if selected.reference.producer.node != current => {
                    AxisSource::from_datum(&selected.reference).ok()
                }
                _ => None,
            };
            if let Some(picked) = picked
                && ui
                    .button("Use selected axis / edge / rotational face")
                    .clicked()
            {
                *source = picked;
                changed = true;
            }
        }
    });
    changed
}
pub(crate) fn point(
    ui: &mut egui::Ui,
    w: &Workspace,
    doc: &Document,
    current: &str,
    label: &str,
    source: &mut PointSource,
) -> bool {
    point_for_definition(
        ui,
        w,
        doc,
        current,
        label,
        w.view.active_part.as_deref().unwrap_or_default(),
        source,
    )
}
pub(crate) fn point_for_definition(
    ui: &mut egui::Ui,
    w: &Workspace,
    doc: &Document,
    current: &str,
    label: &str,
    definition: &str,
    source: &mut PointSource,
) -> bool {
    let mut changed = false;
    ui.push_id(label, |ui| {
        ui.strong(label);
        let selected_text = match source {
            PointSource::Coordinates { .. } => "Coordinates".into(),
            PointSource::Datum { feature } => name(doc, feature),
            PointSource::CoordinateSystemOrigin { feature } => {
                format!("{} · Origin", name(doc, feature))
            }
            PointSource::SketchPoint { profile, .. } => {
                format!("{} · sketch point", name(doc, profile))
            }
            PointSource::PlaneOrigin { plane } => format!("{} · origin", plane_label(doc, plane)),
            PointSource::AxisOrigin { axis } => format!("{} · origin", axis_label(doc, axis)),
            PointSource::EdgePoint { source } => format!("{} · model edge", name(doc, &source.body)),
        };
        egui::ComboBox::from_id_salt("point-reference")
            .selected_text(selected_text)
            .show_ui(ui, |ui| {
                if ui
                    .selectable_label(
                        matches!(source, PointSource::Coordinates { .. }),
                        "Coordinates",
                    )
                    .clicked()
                    && !matches!(source, PointSource::Coordinates { .. })
                {
                    *source = PointSource::default();
                    changed = true;
                }
                if let Some(selected) = w.selected.as_ref().filter(|s| {
                    s.definition == definition && s.reference.topology == "edge" && selected_body(w, s).node != current
                }) {
                    let picked = PointSource::EdgePoint { source: Box::new(EdgePointSource {
                        body: selected_body(w, selected), reference: selected.reference.clone(),
                        parameter: match source {PointSource::EdgePoint {source} => source.parameter.clone(), _ => "0.5".into()},
                    }) };
                    if ui.selectable_label(*source == picked, "Selected model edge · native parameter").clicked() && *source != picked {
                        *source = picked;
                        changed = true;
                    }
                }
                for p in [OriginPlane::Front, OriginPlane::Top, OriginPlane::Right] {
                    changed |= ui
                        .selectable_value(
                            source,
                            PointSource::PlaneOrigin {
                                plane: PlaneSource::Origin { plane: p },
                            },
                            format!("{} · origin", p.label()),
                        )
                        .changed();
                }
                for f in candidates(doc, definition, current) {
                    if matches!(f.operation, Operation::ReferenceCoordinateSystem { .. }) {
                        changed |= ui
                            .selectable_value(
                                source,
                                PointSource::CoordinateSystemOrigin {
                                    feature: NodeRef {
                                        definition: definition.into(),
                                        node: f.id.clone(),
                                    },
                                },
                                format!("{} · Origin", f.name),
                            )
                            .changed();
                    } else if matches!(f.operation, Operation::ReferencePoint { .. }) {
                        changed |= ui
                            .selectable_value(
                                source,
                                PointSource::Datum {
                                    feature: NodeRef {
                                        definition: definition.into(),
                                        node: f.id.clone(),
                                    },
                                },
                                &f.name,
                            )
                            .changed();
                    } else if matches!(f.operation, Operation::ReferencePlane { .. }) {
                        changed |= ui
                            .selectable_value(
                                source,
                                PointSource::PlaneOrigin {
                                    plane: PlaneSource::Datum {
                                        feature: NodeRef {
                                            definition: definition.into(),
                                            node: f.id.clone(),
                                        },
                                    },
                                },
                                format!("{} · origin", f.name),
                            )
                            .changed();
                    } else if matches!(f.operation, Operation::ReferenceAxis { .. }) {
                        changed |= ui
                            .selectable_value(
                                source,
                                PointSource::AxisOrigin {
                                    axis: AxisSource::Datum {
                                        feature: NodeRef {
                                            definition: definition.into(),
                                            node: f.id.clone(),
                                        },
                                    },
                                },
                                format!("{} · origin", f.name),
                            )
                            .changed();
                    } else if let Operation::Sketch3d { definition: sketch } = &f.operation {
                        for (index, key) in sketch.points.keys().enumerate() {
                            changed |= ui.selectable_value(source, PointSource::SketchPoint {profile:NodeRef {definition:definition.into(),node:f.id.clone()},point:key.clone()}, format!("{} · 3D point {}", f.name,index+1)).changed();
                        }
                    } else if let Operation::Profile {
                        profile: cad_core::Profile::Sketch { sketch },
                        ..
                    } = &f.operation
                        && let Some(sketch) = doc.parts[definition].sketches.get(sketch)
                    {
                        for (index, key) in sketch.points.keys().enumerate() {
                            changed |= ui
                                .selectable_value(
                                    source,
                                    PointSource::SketchPoint {
                                        profile: NodeRef {
                                            definition: definition.into(),
                                            node: f.id.clone(),
                                        },
                                        point: key.clone(),
                                    },
                                    format!("{} · Point {}", f.name, index + 1),
                                )
                                .changed();
                        }
                    }
                }
            });
        if let PointSource::Coordinates { coordinates } = source {
            for (label, e) in ["X", "Y", "Z"].into_iter().zip(coordinates) {
                changed |= expression(ui, label, e);
            }
        }
        if let PointSource::EdgePoint { source } = source {
            changed |= expression(ui, "Native edge parameter (0..1)", &mut source.parameter);
            ui.horizontal(|ui| {
                for (label, value) in [("Start", "0"), ("Parameter midpoint", "0.5"), ("End", "1")] {
                    if ui.small_button(label).clicked() {source.parameter = value.into(); changed = true;}
                }
            });
            ui.small("Uses the stored edge orientation and native curve parameter, not arc-length percentage.");
        }
        if let Some(selected) = w.selected.as_ref().filter(|s| {
            s.definition == definition && s.reference.topology == "edge" && selected_body(w, s).node != current
        }) && ui.button("Use selected model edge").clicked() {
            let parameter = match source {PointSource::EdgePoint {source} => source.parameter.clone(), _ => "0.5".into()};
            *source = PointSource::EdgePoint {source: Box::new(EdgePointSource {body:selected_body(w, selected),reference:selected.reference.clone(),parameter})};
            changed = true;
        }
        if let Some(selected) = w.selected.as_ref().filter(|s| {
            s.definition == definition
                && s.reference.topology == "point"
                && s.reference.producer.node != current
        }) && ui.button("Use selected reference point").clicked()
            && let Ok(p) = PointSource::from_datum(&selected.reference)
        {
            *source = p;
            changed = true;
        }
    });
    changed
}
fn default_plane() -> PlaneSource {
    PlaneSource::Origin {
        plane: OriginPlane::Top,
    }
}
fn default_axis() -> AxisSource {
    AxisSource::Origin {
        axis: OriginAxis::X,
    }
}
fn position(x: &str, y: &str, z: &str) -> PointSource {
    PointSource::Coordinates {
        coordinates: [x.into(), y.into(), z.into()],
    }
}
fn plane_mode(index: usize) -> PlaneDefinition {
    match index {
        1 => PlaneDefinition::Angle {
            plane: default_plane(),
            axis: default_axis(),
            angle: "45 deg".into(),
        },
        2 => PlaneDefinition::ThroughPoints {
            points: [
                PointSource::default(),
                position("10 mm", "0 mm", "0 mm"),
                position("0 mm", "10 mm", "0 mm"),
            ],
        },
        3 => PlaneDefinition::Midplane {
            planes: [
                default_plane(),
                PlaneSource::Origin {
                    plane: OriginPlane::Front,
                },
            ],
        },
        4 => PlaneDefinition::NormalToAxis {
            axis: default_axis(),
            point: PointSource::default(),
        },
        5 => PlaneDefinition::AngularBisector {
            planes: [
                default_plane(),
                PlaneSource::Origin {
                    plane: OriginPlane::Front,
                },
            ],
            solution: BisectorSolution::SameSignedDistance,
            reverse: false,
        },
        _ => PlaneDefinition::Offset {
            plane: default_plane(),
            distance: "10 mm".into(),
            flip: false,
        },
    }
}
fn selected_tangent(w: &Workspace, current: &str) -> Option<TangentPlaneSource> {
    let selected = w.selected.as_ref().filter(|selected| {
        Some(selected.definition.as_str()) == w.view.active_part.as_deref()
            && selected.reference.topology == "face"
            && selected.reference.producer.node != current
            && selected_body(w, selected).node != current
    })?;
    Some(TangentPlaneSource {
        body: selected_body(w, selected),
        reference: selected.reference.clone(),
        anchor: TangentAnchor::Parameters {
            parameters: ["0.5".into(), "0.5".into()],
        },
    })
}
fn axis_mode(index: usize) -> AxisDefinition {
    match index {
        1 => AxisDefinition::PlaneIntersection {
            planes: [
                default_plane(),
                PlaneSource::Origin {
                    plane: OriginPlane::Front,
                },
            ],
        },
        2 => AxisDefinition::NormalToPlane {
            plane: default_plane(),
            point: PointSource::default(),
        },
        3 => AxisDefinition::AlongEdge {
            axis: default_axis(),
        },
        4 => AxisDefinition::Direction {
            origin: PointSource::default(),
            direction: ["1".into(), "0".into(), "0".into()],
        },
        5 => AxisDefinition::Geometry {
            axis: default_axis(),
        },
        _ => AxisDefinition::ThroughPoints {
            points: [PointSource::default(), position("10 mm", "0 mm", "0 mm")],
        },
    }
}
fn point_mode(index: usize) -> PointDefinition {
    match index {
        1 => PointDefinition::OnAxis {
            axis: default_axis(),
            distance: "10 mm".into(),
        },
        2 => PointDefinition::PlaneCoordinates {
            plane: default_plane(),
            u: "0 mm".into(),
            v: "0 mm".into(),
            offset: "0 mm".into(),
        },
        3 => PointDefinition::AxisPlaneIntersection {
            axis: AxisSource::Origin {
                axis: OriginAxis::Z,
            },
            plane: default_plane(),
        },
        _ => PointDefinition::AtPoint {
            point: PointSource::default(),
        },
    }
}
fn mode(ui: &mut egui::Ui, label: &str, index: &mut usize, labels: &[&str]) -> bool {
    let mut changed = false;
    egui::ComboBox::from_label(label)
        .selected_text(labels[*index])
        .show_ui(ui, |ui| {
            for (n, label) in labels.iter().enumerate() {
                changed |= ui.selectable_value(index, n, *label).changed();
            }
        });
    changed
}
pub(crate) fn direction(
    ui: &mut egui::Ui,
    w: &Workspace,
    doc: &Document,
    current: &str,
    label: &str,
    source: &mut DirectionSource,
) -> bool {
    ui.push_id(label, |ui| {
        ui.strong(label);
        let mut changed = false;
        let mut index = match source {
            DirectionSource::Axis { .. } => 0,
            DirectionSource::PlaneNormal { .. } => 1,
            DirectionSource::TwoPoints { .. } => 2,
            DirectionSource::Explicit { .. } => 3,
        };
        if mode(
            ui,
            "Direction Definition",
            &mut index,
            &[
                "Axis / Edge",
                "Normal to Plane",
                "Two Points",
                "Explicit Direction",
            ],
        ) {
            *source = match index {
                1 => DirectionSource::PlaneNormal {
                    plane: default_plane(),
                },
                2 => DirectionSource::TwoPoints {
                    points: Box::new([PointSource::default(), position("10 mm", "0 mm", "0 mm")]),
                },
                3 => DirectionSource::Explicit {
                    direction: ["1".into(), "0".into(), "0".into()],
                },
                _ => DirectionSource::Axis {
                    axis: Box::new(default_axis()),
                },
            };
            changed = true;
        }
        match source {
            DirectionSource::Axis { axis: source } => {
                changed |= axis(ui, w, doc, current, "Direction Axis", source)
            }
            DirectionSource::PlaneNormal { plane: source } => {
                changed |= plane(ui, w, doc, current, "Normal Plane", source)
            }
            DirectionSource::TwoPoints { points } => {
                changed |= point(ui, w, doc, current, "From Point", &mut points[0]);
                changed |= point(ui, w, doc, current, "Toward Point", &mut points[1]);
            }
            DirectionSource::Explicit { direction } => {
                for (label, e) in ["Direction X", "Direction Y", "Direction Z"]
                    .into_iter()
                    .zip(direction)
                {
                    changed |= expression(ui, label, e);
                }
            }
        }
        changed
    })
    .inner
}
fn output_axis(ui: &mut egui::Ui, label: &str, value: &mut OriginAxis) -> bool {
    let mut changed = false;
    egui::ComboBox::from_label(label)
        .selected_text(value.label())
        .show_ui(ui, |ui| {
            for a in [OriginAxis::X, OriginAxis::Y, OriginAxis::Z] {
                changed |= ui.selectable_value(value, a, a.label()).changed();
            }
        });
    changed
}
pub fn fields(ui: &mut egui::Ui, w: &Workspace, feature: &mut Feature) -> bool {
    let doc = w.session.visible();
    let current = &feature.id;
    let mut changed = false;
    match &mut feature.operation {
        Operation::ReferenceCoordinateSystem { definition } => {
            changed |= point(
                ui,
                w,
                doc,
                current,
                "Coordinate Origin",
                &mut definition.origin,
            );
            changed |= output_axis(ui, "Primary Output Axis", &mut definition.primary_axis);
            changed |= direction(
                ui,
                w,
                doc,
                current,
                "Primary Direction",
                &mut definition.primary,
            );
            changed |= ui
                .checkbox(&mut definition.reverse_primary, "Reverse primary direction")
                .changed();
            changed |= output_axis(ui, "Secondary Output Axis", &mut definition.secondary_axis);
            changed |= direction(
                ui,
                w,
                doc,
                current,
                "Secondary Direction",
                &mut definition.secondary,
            );
            changed |= ui
                .checkbox(
                    &mut definition.reverse_secondary,
                    "Reverse secondary direction",
                )
                .changed();
            ui.small("The primary direction is preserved. The secondary direction is projected perpendicular to it; the third axis completes a right-handed frame.");
        }
        Operation::ReferencePlane { definition } => {
            let mut index = match definition {
                PlaneDefinition::Offset { .. } => 0,
                PlaneDefinition::Angle { .. } => 1,
                PlaneDefinition::ThroughPoints { .. } => 2,
                PlaneDefinition::Midplane { .. } => 3,
                PlaneDefinition::NormalToAxis { .. } => 4,
                PlaneDefinition::AngularBisector { .. } => 5,
                PlaneDefinition::Tangent { .. } => 6,
            };
            let labels = [
                "Offset / On Face",
                "At Angle",
                "Three Points",
                "Parallel Midplane",
                "Normal to Axis",
                "Angular Bisector",
                "Tangent to Face",
            ];
            let selected = selected_tangent(w, current).or_else(|| {
                if let PlaneDefinition::Offset {
                    plane: PlaneSource::Face { body, reference },
                    ..
                } = definition
                {
                    Some(TangentPlaneSource {
                        body: body.clone(),
                        reference: reference.clone(),
                        anchor: TangentAnchor::Parameters {
                            parameters: ["0.5".into(), "0.5".into()],
                        },
                    })
                } else {
                    None
                }
            });
            let old_index = index;
            egui::ComboBox::from_label("Plane Definition")
                .selected_text(labels[index])
                .show_ui(ui, |ui| {
                    for (value, label) in labels.into_iter().enumerate() {
                        ui.add_enabled_ui(
                            value != 6 || selected.is_some() || old_index == 6,
                            |ui| {
                                ui.selectable_value(&mut index, value, label);
                            },
                        );
                    }
                });
            if index != old_index {
                if index == 6 {
                    if let Some(source) = selected {
                        *definition = PlaneDefinition::Tangent {
                            source: Box::new(source),
                            reverse: false,
                        };
                        changed = true;
                    }
                } else {
                    *definition = plane_mode(index);
                    changed = true;
                }
            }
            if index != 6 && selected_tangent(w, current).is_none() {
                ui.small("Select a model face to enable Tangent to Face.");
            }
            match definition {
                PlaneDefinition::Offset {
                    plane: source,
                    distance,
                    flip,
                } => {
                    changed |= plane(ui, w, doc, current, "Reference Plane", source);
                    changed |= expression(ui, "Signed Offset", distance);
                    changed |= ui.checkbox(flip, "Reverse plane normal").changed();
                }
                PlaneDefinition::Angle {
                    plane: source,
                    axis: direction,
                    angle,
                } => {
                    changed |= plane(ui, w, doc, current, "Reference Plane", source);
                    changed |= axis(ui, w, doc, current, "Rotation Axis", direction);
                    changed |= expression(ui, "Angle", angle);
                }
                PlaneDefinition::ThroughPoints { points } => {
                    for (label, source) in ["First Point", "Second Point", "Third Point"]
                        .into_iter()
                        .zip(points)
                    {
                        changed |= point(ui, w, doc, current, label, source);
                    }
                }
                PlaneDefinition::Midplane { planes } => {
                    for (label, source) in ["First Plane", "Second Plane"].into_iter().zip(planes) {
                        changed |= plane(ui, w, doc, current, label, source);
                    }
                    ui.small("References must be parallel, distinct planes.");
                }
                PlaneDefinition::NormalToAxis {
                    axis: direction,
                    point: source,
                } => {
                    changed |= axis(ui, w, doc, current, "Normal Axis", direction);
                    changed |= point(ui, w, doc, current, "Point on Plane", source);
                }
                PlaneDefinition::AngularBisector {
                    planes,
                    solution,
                    reverse,
                } => {
                    for (label, source) in ["First Plane", "Second Plane"].into_iter().zip(planes) {
                        changed |= plane(ui, w, doc, current, label, source);
                    }
                    egui::ComboBox::from_label("Bisector Solution")
                        .selected_text(match solution {
                            BisectorSolution::SameSignedDistance => "Equal signed distance",
                            BisectorSolution::OppositeSignedDistance => "Opposite signed distance",
                        })
                        .show_ui(ui, |ui| {
                            changed |= ui
                                .selectable_value(
                                    solution,
                                    BisectorSolution::SameSignedDistance,
                                    "Equal signed distance",
                                )
                                .changed();
                            changed |= ui
                                .selectable_value(
                                    solution,
                                    BisectorSolution::OppositeSignedDistance,
                                    "Opposite signed distance",
                                )
                                .changed();
                        });
                    changed |= ui.checkbox(reverse, "Reverse plane normal").changed();
                    ui.small("Choose either of the two perpendicular bisectors. References must be intersecting planes.");
                }
                PlaneDefinition::Tangent { source, reverse } => {
                    ui.label(format!("Tangent Face · {}", name(doc, &source.body)));
                    if let Some(selected) = selected_tangent(w, current)
                        && ui.button("Use selected face").clicked()
                    {
                        source.body = selected.body;
                        source.reference = selected.reference;
                        changed = true;
                    }
                    let mut mode_index =
                        usize::from(matches!(source.anchor, TangentAnchor::Point { .. }));
                    if mode(
                        ui,
                        "Anchor",
                        &mut mode_index,
                        &["Normalized Face Parameters", "Associative Point"],
                    ) {
                        source.anchor = if mode_index == 0 {
                            TangentAnchor::Parameters {
                                parameters: ["0.5".into(), "0.5".into()],
                            }
                        } else {
                            TangentAnchor::Point {
                                point: Box::default(),
                            }
                        };
                        changed = true;
                    }
                    match &mut source.anchor {
                        TangentAnchor::Parameters { parameters } => {
                            changed |= expression(ui, "Face U (0–1)", &mut parameters[0]);
                            changed |= expression(ui, "Face V (0–1)", &mut parameters[1]);
                            ui.small("Coordinates span the proven selected face domain; they are not arc-length fractions.");
                        }
                        TangentAnchor::Point { point: anchor } => {
                            changed |= point(ui, w, doc, current, "Point on Face", anchor);
                            ui.small("The point must lie on the bounded face. General rational patches require parameter anchors until a unique inverse can be proven.");
                        }
                    }
                    changed |= ui.checkbox(reverse, "Reverse plane normal").changed();
                    ui.small("Supported: full four-sided rational patches, rectangular planar faces, and bounded cylindrical/conical revolution faces. Holes, general trims, and singular anchors require additional native proofs.");
                }
            }
        }
        Operation::ReferenceAxis { definition } => {
            let mut index = match definition {
                AxisDefinition::ThroughPoints { .. } => 0,
                AxisDefinition::PlaneIntersection { .. } => 1,
                AxisDefinition::NormalToPlane { .. } => 2,
                AxisDefinition::AlongEdge { .. } => 3,
                AxisDefinition::Direction { .. } => 4,
                AxisDefinition::Geometry { .. } => 5,
            };
            if mode(
                ui,
                "Axis Definition",
                &mut index,
                &[
                    "Two Points",
                    "Two Planes",
                    "Normal to Plane",
                    "Linear Edge / Axis",
                    "Point and Direction",
                    "Rotational Geometry",
                ],
            ) {
                *definition = axis_mode(index);
                changed = true;
            }
            match definition {
                AxisDefinition::ThroughPoints { points } => {
                    for (label, source) in ["First Point", "Second Point"].into_iter().zip(points) {
                        changed |= point(ui, w, doc, current, label, source);
                    }
                }
                AxisDefinition::PlaneIntersection { planes } => {
                    for (label, source) in ["First Plane", "Second Plane"].into_iter().zip(planes) {
                        changed |= plane(ui, w, doc, current, label, source);
                    }
                }
                AxisDefinition::NormalToPlane {
                    plane: source,
                    point: p,
                } => {
                    changed |= plane(ui, w, doc, current, "Normal Plane", source);
                    changed |= point(ui, w, doc, current, "Axis Origin", p);
                }
                AxisDefinition::AlongEdge { axis: direction }
                | AxisDefinition::Geometry { axis: direction } => {
                    changed |= axis(ui, w, doc, current, "Reference Axis / Edge", direction);
                    ui.small("Straight edges, circular edges and sketch circles/arcs, and proven cylindrical/conical or analytic revolved face axes.");
                }
                AxisDefinition::Direction { origin, direction } => {
                    changed |= point(ui, w, doc, current, "Axis Origin", origin);
                    for (label, e) in ["Direction X", "Direction Y", "Direction Z"]
                        .into_iter()
                        .zip(direction)
                    {
                        changed |= expression(ui, label, e);
                    }
                }
            }
        }
        Operation::ReferencePoint { definition } => {
            let mut index = match definition {
                PointDefinition::AtPoint { .. } => 0,
                PointDefinition::OnAxis { .. } => 1,
                PointDefinition::PlaneCoordinates { .. } => 2,
                PointDefinition::AxisPlaneIntersection { .. } => 3,
            };
            if mode(
                ui,
                "Point Definition",
                &mut index,
                &[
                    "Coordinates / Reference / Edge Point",
                    "Distance Along Axis",
                    "On Plane",
                    "Axis / Plane Intersection",
                ],
            ) {
                *definition = point_mode(index);
                changed = true;
            }
            match definition {
                PointDefinition::AtPoint { point: source } => {
                    changed |= point(ui, w, doc, current, "Point", source);
                }
                PointDefinition::OnAxis {
                    axis: direction,
                    distance,
                } => {
                    changed |= axis(ui, w, doc, current, "Reference Axis", direction);
                    changed |= expression(ui, "Distance from Axis Origin", distance);
                }
                PointDefinition::PlaneCoordinates {
                    plane: source,
                    u,
                    v,
                    offset,
                } => {
                    changed |= plane(ui, w, doc, current, "Reference Plane", source);
                    changed |= expression(ui, "Plane U", u);
                    changed |= expression(ui, "Plane V", v);
                    changed |= expression(ui, "Normal Offset", offset);
                }
                PointDefinition::AxisPlaneIntersection {
                    axis: direction,
                    plane: source,
                } => {
                    changed |= axis(ui, w, doc, current, "Reference Axis", direction);
                    changed |= plane(ui, w, doc, current, "Reference Plane", source);
                }
            }
        }
        _ => return false,
    }
    ui.small("Distances accept explicit units or named parameters. Angles accept deg / rad.");
    changed
}
pub fn preset(w: &mut Workspace, name: &str) -> bool {
    if !matches!(
        name,
        "Plane" | "Axis" | "Point" | "Face Axis" | "Coordinate System"
    ) {
        return false;
    }
    crate::history::latest(w);
    if w.sketch.is_some() || w.tool_checkpoint.is_some() {
        w.status =
            "Confirm or cancel the current operation before creating reference geometry".into();
        return true;
    }
    let Some(part) = w
        .view
        .active_part
        .as_ref()
        .and_then(|d| w.session.visible().parts.get(d))
        .cloned()
    else {
        w.status = "Create or select a part first".into();
        return true;
    };
    let mut operation = match name {
        "Coordinate System" => Operation::ReferenceCoordinateSystem {
            definition: Box::default(),
        },
        "Axis" => Operation::ReferenceAxis {
            definition: axis_mode(4),
        },
        "Face Axis" => Operation::ReferenceAxis {
            definition: axis_mode(5),
        },
        "Point" => Operation::ReferencePoint {
            definition: point_mode(0),
        },
        _ => Operation::ReferencePlane {
            definition: plane_mode(0),
        },
    };
    if let Operation::ReferencePlane {
        definition:
            PlaneDefinition::Offset {
                plane: source,
                distance,
                ..
            },
    } = &mut operation
    {
        if let Some(selected) = w.selected.as_ref().filter(|s| s.definition == part.id) {
            match selected.reference.topology.as_str() {
                "face" => {
                    *source = PlaneSource::Face {
                        reference: selected.reference.clone(),
                        body: selected_body(w, selected),
                    };
                    *distance = "0 mm".into();
                }
                "plane" => {
                    if let Ok(p) = PlaneSource::from_datum(&selected.reference) {
                        *source = p;
                    }
                }
                _ => {}
            }
        } else if let Some(node) = w
            .view
            .active_node
            .as_ref()
            .and_then(|id| part.features.iter().find(|f| &f.id == id))
            .filter(|f| matches!(f.operation, Operation::ReferencePlane { .. }))
        {
            *source = PlaneSource::Datum {
                feature: part.reference(&node.id),
            };
        } else if let Some(active) = &w.active_plane {
            *source = PlaneSource::Origin {
                plane: match active.as_str() {
                    "Front Plane" => OriginPlane::Front,
                    "Right Plane" => OriginPlane::Right,
                    _ => OriginPlane::Top,
                },
            };
        }
    }
    if matches!(operation, Operation::ReferencePlane { .. })
        && w.selected.as_ref().is_some_and(|selected| {
            selected.definition == part.id
                && selected.reference.topology == "face"
                && !selected_face_planar(w, selected)
        })
        && let Some(source) = selected_tangent(w, "")
    {
        operation = Operation::ReferencePlane {
            definition: PlaneDefinition::Tangent {
                source: Box::new(source),
                reverse: false,
            },
        };
    }
    if let Some(selected) = w.selected.as_ref().filter(|s| s.definition == part.id) {
        let feature = selected.reference.producer.clone();
        match (name, selected.reference.topology.as_str()) {
            ("Coordinate System", topology) => {
                if let Operation::ReferenceCoordinateSystem { definition } = &mut operation {
                    match topology {
                        "point" => {
                            if let Ok(p) = PointSource::from_datum(&selected.reference) {
                                definition.origin = p;
                            }
                        }
                        "plane" => {
                            if let Ok(p) = PlaneSource::from_datum(&selected.reference) {
                                definition.origin = PointSource::PlaneOrigin { plane: p.clone() };
                                definition.primary_axis = OriginAxis::Z;
                                definition.primary = DirectionSource::PlaneNormal { plane: p };
                                definition.secondary_axis = OriginAxis::X;
                                definition.secondary = DirectionSource::Axis {
                                    axis: Box::new(default_axis()),
                                };
                            }
                        }
                        "axis" => {
                            if let Ok(a) = AxisSource::from_datum(&selected.reference) {
                                definition.origin = PointSource::AxisOrigin { axis: a.clone() };
                                definition.primary = DirectionSource::Axis { axis: Box::new(a) };
                            }
                        }
                        "edge" | "face" => {
                            definition.primary = DirectionSource::Axis {
                                axis: Box::new(picked_axis(w, selected)),
                            }
                        }
                        _ => {}
                    }
                }
            }
            ("Plane", "axis") => {
                let axis =
                    AxisSource::from_datum(&selected.reference).unwrap_or(AxisSource::Datum {
                        feature: feature.clone(),
                    });
                operation = Operation::ReferencePlane {
                    definition: PlaneDefinition::NormalToAxis {
                        axis: axis.clone(),
                        point: PointSource::AxisOrigin { axis },
                    },
                }
            }
            ("Plane", "point") => {
                operation = Operation::ReferencePlane {
                    definition: PlaneDefinition::NormalToAxis {
                        axis: default_axis(),
                        point: PointSource::from_datum(&selected.reference)
                            .unwrap_or(PointSource::Datum { feature }),
                    },
                }
            }
            ("Axis", "edge") => {
                operation = Operation::ReferenceAxis {
                    definition: AxisDefinition::AlongEdge {
                        axis: picked_axis(w, selected),
                    },
                }
            }
            ("Axis", "axis") => {
                operation = Operation::ReferenceAxis {
                    definition: AxisDefinition::AlongEdge {
                        axis: AxisSource::from_datum(&selected.reference)
                            .unwrap_or(AxisSource::Datum { feature }),
                    },
                }
            }
            ("Axis", "plane") => {
                let plane = PlaneSource::from_datum(&selected.reference)
                    .unwrap_or(PlaneSource::Datum { feature });
                operation = Operation::ReferenceAxis {
                    definition: AxisDefinition::NormalToPlane {
                        plane: plane.clone(),
                        point: PointSource::PlaneOrigin { plane },
                    },
                };
            }
            ("Axis", "face") => {
                if !selected_face_planar(w, selected) {
                    operation = Operation::ReferenceAxis {
                        definition: AxisDefinition::Geometry {
                            axis: picked_axis(w, selected),
                        },
                    };
                } else {
                    let plane = PlaneSource::Face {
                        reference: selected.reference.clone(),
                        body: selected_body(w, selected),
                    };
                    operation = Operation::ReferenceAxis {
                        definition: AxisDefinition::NormalToPlane {
                            plane: plane.clone(),
                            point: PointSource::PlaneOrigin { plane },
                        },
                    };
                }
            }
            ("Face Axis", "face") => {
                operation = Operation::ReferenceAxis {
                    definition: AxisDefinition::Geometry {
                        axis: picked_axis(w, selected),
                    },
                };
            }
            ("Point", "plane") => {
                operation = Operation::ReferencePoint {
                    definition: PointDefinition::AtPoint {
                        point: PointSource::PlaneOrigin {
                            plane: PlaneSource::from_datum(&selected.reference)
                                .unwrap_or(PlaneSource::Datum { feature }),
                        },
                    },
                }
            }
            ("Point", "axis") => {
                operation = Operation::ReferencePoint {
                    definition: PointDefinition::AtPoint {
                        point: PointSource::AxisOrigin {
                            axis: AxisSource::from_datum(&selected.reference)
                                .unwrap_or(AxisSource::Datum { feature }),
                        },
                    },
                }
            }
            ("Point", "point") => {
                operation = Operation::ReferencePoint {
                    definition: PointDefinition::AtPoint {
                        point: PointSource::from_datum(&selected.reference)
                            .unwrap_or(PointSource::Datum { feature }),
                    },
                }
            }
            ("Point", "edge") => {
                operation = Operation::ReferencePoint {
                    definition: PointDefinition::AtPoint {
                        point: PointSource::EdgePoint {
                            source: Box::new(EdgePointSource {
                                body: selected_body(w, selected),
                                reference: selected.reference.clone(),
                                parameter: "0.5".into(),
                            }),
                        },
                    },
                };
            }
            _ => {}
        }
    }
    let number = part
        .features
        .iter()
        .filter(|f| f.name.starts_with(name))
        .count()
        + 1;
    let feature = Feature {
        id: cad_core::id(),
        name: format!("{name}{number}"),
        suppressed: false,
        operation,
    };
    w.start_tool();
    w.dispatch(Command::PutFeature {
        definition: part.id,
        feature: feature.clone().into(),
        output: false,
    });
    w.view.active_node = Some(feature.id.clone());
    ui::feature_dialog(w, Some(feature));
    true
}

pub fn evaluated_plane(
    w: &Workspace,
    definition: &str,
    node: &str,
) -> Option<(cad_core::Plane, cad_core::TopologyRef)> {
    let evaluation = viewport::display_evaluation(w)?;
    let instance = evaluation.instances.iter().find(|i| {
        i.definition == definition
            && if let Some(selected) = w
                .selected
                .as_ref()
                .filter(|s| s.definition == definition && s.reference.producer.node == node)
            {
                i.path == selected.path
            } else {
                w.view
                    .active_instance
                    .as_ref()
                    .is_none_or(|id| i.path.contains(id))
            }
    });
    if instance.is_none()
        && (w
            .selected
            .as_ref()
            .is_some_and(|s| s.definition == definition && s.reference.producer.node == node)
            || w.view.active_instance.is_some())
    {
        return None;
    }
    let key = instance
        .map(|i| {
            if i.mesh_key.is_empty() {
                i.definition.as_str()
            } else {
                i.mesh_key.as_str()
            }
        })
        .unwrap_or(definition);
    let reference = evaluation.references.get(&format!("{key}::{node}"))?;
    if reference.version != CONTRACT_VERSION {
        return None;
    }
    let mut topology = reference.topology_reference();
    if matches!(reference.geometry, Geometry::CoordinateSystem { .. }) {
        topology.topology = "plane".into();
        topology.role = w
            .selected
            .as_ref()
            .filter(|s| {
                s.reference.producer == reference.producer
                    && coordinate_plane_element(&s.reference.role).is_some()
            })
            .map(|s| s.reference.role.clone())
            .unwrap_or_else(|| coordinate_plane_role(OriginPlane::Top).into());
    }
    reference
        .geometry
        .selected_plane(&topology.role)
        .ok()
        .map(|p| (p, topology))
}
pub fn normal_to(w: &mut Workspace) {
    let Some(definition) = w.view.active_part.as_deref() else {
        return;
    };
    let node = w
        .selected
        .as_ref()
        .filter(|s| s.reference.topology == "plane")
        .map(|s| s.reference.producer.node.as_str())
        .or(w.view.active_node.as_deref());
    let Some((plane, _)) = node.and_then(|n| evaluated_plane(w, definition, n)) else {
        return;
    };
    let n = DVec3::from_array(plane.u)
        .cross(DVec3::from_array(plane.v))
        .normalize();
    let instance = viewport::display_evaluation(w).and_then(|e| {
        e.instances.iter().find(|i| {
            i.definition == definition
                && if let Some(selected) = w
                    .selected
                    .as_ref()
                    .filter(|s| s.definition == definition && s.reference.topology == "plane")
                {
                    i.path == selected.path
                } else {
                    w.view
                        .active_instance
                        .as_ref()
                        .is_none_or(|id| i.path.contains(id))
                }
        })
    });
    let n = instance
        .map(|i| {
            DVec3::new(
                i.matrix[0][0] * n.x + i.matrix[0][1] * n.y + i.matrix[0][2] * n.z,
                i.matrix[1][0] * n.x + i.matrix[1][1] * n.y + i.matrix[1][2] * n.z,
                i.matrix[2][0] * n.x + i.matrix[2][1] * n.y + i.matrix[2][2] * n.z,
            )
        })
        .unwrap_or(n)
        .normalize();
    let origin = instance
        .map(|i| viewport::placed(plane.origin, i))
        .unwrap_or(plane.origin);
    w.view.orbit = [n.x.atan2(-n.y) as f32, n.z.clamp(-1., 1.).asin() as f32];
    w.view.roll = 0.;
    crate::navigation::stop(w);
    w.view.target = viewport::render_point(origin).to_array();
}

fn visible(
    w: &Workspace,
    instance: &cad_core::PlacedPart,
    reference: &EvaluatedReference,
    evaluation: &cad_core::protocol::Evaluation,
) -> bool {
    if reference.version != CONTRACT_VERSION || !viewport::visible(w, instance) {
        return false;
    }
    match crate::sketch_overlay::mode(w, false) {
        cad_core::presentation::OverlayVisibility::Hidden => return false,
        cad_core::presentation::OverlayVisibility::Auto
            if !(w
                .sketch
                .as_ref()
                .is_some_and(|sketch| sketch.choosing_plane)
                || (w.view.active_part.as_deref() == Some(&instance.definition)
                    && w.view.active_node.as_deref() == Some(&reference.producer.node))) =>
        {
            return false;
        }
        _ => {}
    }
    if w.view.isolation
        && !w.peek
        && w.view.active_part.as_deref() != Some(&instance.definition)
        && !w.view.ghost_others
        && !w.view.pinned.iter().any(|p| instance.path.contains(p))
    {
        return false;
    }
    if let Some(cursor) = w
        .view
        .history_cursor
        .as_ref()
        .filter(|c| c.definition == instance.definition)
    {
        let key = cad_core::history::mesh_key(instance);
        let stage = cad_core::history::stage(evaluation, key, &cursor.node);
        let reference_stage = cad_core::history::stage(evaluation, key, &reference.producer.node);
        if !stage
            .zip(reference_stage)
            .is_some_and(|(a, b)| b.index <= a.index)
        {
            return false;
        }
    }
    let region = cad_core::presentation::Region {
        id: format!("reference:{}", reference.producer.node),
        role: "reference-geometry".into(),
        component: None,
        rect: [0.; 4],
        classes: vec![reference.geometry.topology().into()],
        resizable: false,
    };
    !crate::sketch_overlay::hidden(&w.theme.properties(&region, &w.view.active_layout, "ready"))
}
fn extent(w: &Workspace) -> f64 {
    (w.view.distance as f64 * 18.).max(12.)
}
fn plane_corners(plane: &cad_core::Plane, extent: f64) -> [[f64; 3]; 4] {
    [
        [-extent, -extent],
        [extent, -extent],
        [extent, extent],
        [-extent, extent],
    ]
    .map(|p| std::array::from_fn(|i| plane.origin[i] + plane.u[i] * p[0] + plane.v[i] * p[1]))
}
fn segment_distance(pointer: egui::Pos2, a: egui::Pos2, b: egui::Pos2) -> f32 {
    let ab = b - a;
    let t = ((pointer - a).dot(ab) / ab.length_sq().max(1e-9)).clamp(0., 1.);
    (a + ab * t - pointer).length()
}
fn pick_elements(reference: &EvaluatedReference) -> Vec<(Geometry, String, String)> {
    reference
        .elements()
        .into_iter()
        .map(|(geometry, reference, label)| (geometry, reference.role, label))
        .collect()
}
pub fn picks(
    w: &Workspace,
    rect: egui::Rect,
    pointer: egui::Pos2,
    evaluation: &cad_core::protocol::Evaluation,
) -> Vec<viewport::Selection> {
    let mut hits = Vec::new();
    let (origin, ray_direction) = viewport::ray(w, rect, pointer);
    let mut remaining = crate::sketch_overlay::DEPTH_WORK;
    for instance in viewport::display_instances(w, evaluation) {
        if !crate::display_states_ui::pickable(w, &instance.path) {
            continue;
        }
        let key = cad_core::history::mesh_key(instance);
        for (_, reference) in evaluation
            .references
            .iter()
            .filter(|(id, _)| id.starts_with(&format!("{key}::")))
        {
            if !visible(w, instance, reference, evaluation) {
                continue;
            }
            for (geometry, role, label) in pick_elements(reference) {
                if w.sketch.as_ref().is_some_and(|s| s.choosing_plane)
                    && !matches!(geometry, Geometry::Plane { .. })
                {
                    continue;
                }
                let pick_extent = if matches!(reference.geometry, Geometry::CoordinateSystem { .. })
                {
                    extent(w) * 0.3
                } else {
                    extent(w)
                };
                let center = viewport::placed(geometry.origin(), instance);
                let project = |p| viewport::project(w, rect, viewport::placed(p, instance));
                let near = match &geometry {
                    Geometry::Point { point } => {
                        project(*point).is_some_and(|p| p.distance(pointer) < 8.)
                    }
                    Geometry::Axis { axis } => {
                        let points = [-pick_extent, pick_extent].map(|s| {
                            std::array::from_fn(|i| axis.origin[i] + axis.direction[i] * s)
                        });
                        project(points[0])
                            .zip(project(points[1]))
                            .is_some_and(|(a, b)| segment_distance(pointer, a, b) < 6.)
                    }
                    Geometry::Plane { plane } => {
                        let corners = plane_corners(plane, pick_extent);
                        let near_edge = (0..4).any(|i| {
                            project(corners[i])
                                .zip(project(corners[(i + 1) % 4]))
                                .is_some_and(|(a, b)| segment_distance(pointer, a, b) < 6.)
                        });
                        let near_center =
                            project(plane.origin).is_some_and(|p| p.distance(pointer) < 8.);
                        let interior = w.sketch.as_ref().is_some_and(|s| s.choosing_plane)
                            && crate::sketch::hit_plane(w, rect, pointer, plane, &instance.matrix)
                                .is_some_and(|p| p.into_iter().all(|a| a.abs() <= pick_extent));
                        near_edge || near_center || interior
                    }
                    Geometry::CoordinateSystem { .. } => false,
                };
                if near {
                    let props = crate::sketch_overlay::reference_properties(
                        w,
                        &format!("reference:{}", reference.producer.node),
                        geometry.topology(),
                        true,
                    );
                    let xray = crate::sketch_overlay::xray(w, false, &props)
                        || w.sketch
                            .as_ref()
                            .is_some_and(|sketch| sketch.choosing_plane);
                    let visibility_point = match &geometry {
                        Geometry::Plane { plane } => {
                            crate::sketch::hit_plane(w, rect, pointer, plane, &instance.matrix)
                                .map(|point| crate::sketch::world(plane, &instance.matrix, point))
                                .unwrap_or(center)
                        }
                        Geometry::Axis { axis } => {
                            let line_origin = DVec3::from_array(center);
                            let endpoint = viewport::placed(
                                std::array::from_fn(|index| {
                                    axis.origin[index] + axis.direction[index]
                                }),
                                instance,
                            );
                            let direction = (DVec3::from_array(endpoint) - line_origin).normalize();
                            let ray = DVec3::from_array(ray_direction);
                            let delta = line_origin - DVec3::from_array(origin);
                            let parallel = direction.dot(ray);
                            let denominator = 1. - parallel * parallel;
                            let parameter = if denominator > 1e-12 {
                                ((parallel * ray.dot(delta) - direction.dot(delta)) / denominator)
                                    .clamp(-pick_extent, pick_extent)
                            } else {
                                0.
                            };
                            (line_origin + direction * parameter).to_array()
                        }
                        _ => center,
                    };
                    if !xray && !viewport::visible_at(w, rect, visibility_point, &mut remaining) {
                        continue;
                    }
                    hits.push(viewport::Selection {
                        instance: instance
                            .path
                            .last()
                            .cloned()
                            .unwrap_or_else(|| instance.definition.clone()),
                        path: instance.path.clone(),
                        name: label,
                        definition: instance.definition.clone(),
                        reference: cad_core::TopologyRef {
                            producer: reference.producer.clone(),
                            topology: geometry.topology().into(),
                            role,
                            signature: None,
                        },
                        body: None,
                        distance: DVec3::from_array(center).distance(DVec3::from_array(origin)),
                        point: center,
                    });
                }
            }
        }
    }
    hits
}
pub fn overlay(w: &Workspace, ui: &egui::Ui, rect: egui::Rect) {
    let Some(evaluation) = viewport::display_evaluation(w) else {
        return;
    };
    let painter = ui.painter().with_clip_rect(rect);
    let mut remaining = crate::sketch_overlay::DEPTH_WORK;
    for instance in viewport::display_instances(w, evaluation) {
        let key = cad_core::history::mesh_key(instance);
        for (_, reference) in evaluation
            .references
            .iter()
            .filter(|(id, _)| id.starts_with(&format!("{key}::")))
        {
            if !visible(w, instance, reference, evaluation) {
                continue;
            }
            let selected = w.view.active_part.as_deref() == Some(&instance.definition)
                && w.view.active_node.as_deref() == Some(&reference.producer.node);
            let props = crate::sketch_overlay::reference_properties(
                w,
                &format!("reference:{}", reference.producer.node),
                reference.geometry.topology(),
                selected,
            );
            if crate::sketch_overlay::hidden(&props) {
                continue;
            }
            let color = props
                .get("color")
                .and_then(|s| ui::color(s))
                .or_else(|| {
                    w.theme
                        .variables
                        .get("--reference")
                        .and_then(|s| ui::color(s))
                })
                .unwrap_or_else(|| {
                    if selected {
                        ui.visuals().selection.stroke.color
                    } else {
                        ui.visuals().weak_text_color()
                    }
                });
            let stroke = egui::Stroke::new(
                crate::presentation_controls::number(
                    &props,
                    "stroke-width",
                    if selected { 1.8 } else { 1. },
                ),
                color,
            );
            let xray = crate::sketch_overlay::xray(w, false, &props)
                || w.sketch
                    .as_ref()
                    .is_some_and(|sketch| sketch.choosing_plane);
            let project = |point| viewport::project(w, rect, viewport::placed(point, instance));
            let visible_point = |point, remaining: &mut usize| {
                xray || viewport::visible_at(w, rect, viewport::placed(point, instance), remaining)
            };
            let draw = |endpoints: [[f64; 3]; 2], stroke, remaining: &mut usize| {
                crate::sketch_overlay::line(
                    w,
                    ui,
                    rect,
                    endpoints.map(|point| viewport::placed(point, instance)),
                    stroke,
                    xray,
                    remaining,
                );
            };
            let outline = |corners: [[f64; 3]; 4], stroke, remaining: &mut usize| {
                for index in 0..4 {
                    draw(
                        [corners[index], corners[(index + 1) % 4]],
                        stroke,
                        remaining,
                    );
                }
                if xray
                    && let Some(points) =
                        corners.into_iter().map(project).collect::<Option<Vec<_>>>()
                {
                    painter.add(egui::Shape::convex_polygon(
                        points,
                        color.gamma_multiply(0.05),
                        egui::Stroke::NONE,
                    ));
                }
            };
            match &reference.geometry {
                Geometry::CoordinateSystem { frame } => {
                    let length = extent(w) * 0.3;
                    for (axis, label, variable) in [
                        (OriginAxis::X, "X", "--coordinate-x"),
                        (OriginAxis::Y, "Y", "--coordinate-y"),
                        (OriginAxis::Z, "Z", "--coordinate-z"),
                    ] {
                        let axis = frame.axis(axis);
                        let endpoints = [-length, length].map(|distance| {
                            std::array::from_fn(|index| {
                                axis.origin[index] + axis.direction[index] * distance
                            })
                        });
                        let axis_color = w
                            .theme
                            .variables
                            .get(variable)
                            .and_then(|value| ui::color(value))
                            .unwrap_or(color);
                        draw(
                            endpoints,
                            egui::Stroke::new(stroke.width, axis_color),
                            &mut remaining,
                        );
                        if visible_point(endpoints[1], &mut remaining)
                            && let Some(point) = project(endpoints[1])
                        {
                            painter.text(
                                point,
                                egui::Align2::LEFT_BOTTOM,
                                label,
                                egui::FontId::proportional(11.),
                                axis_color,
                            );
                        }
                    }
                    for plane in [OriginPlane::Top, OriginPlane::Front, OriginPlane::Right] {
                        outline(
                            plane_corners(&frame.plane(plane), length),
                            egui::Stroke::new(stroke.width, color.gamma_multiply(0.3)),
                            &mut remaining,
                        );
                    }
                    if visible_point(frame.origin, &mut remaining)
                        && let Some(point) = project(frame.origin)
                    {
                        painter.circle_stroke(point, 4., stroke);
                        painter.text(
                            point + egui::vec2(7., -7.),
                            egui::Align2::LEFT_BOTTOM,
                            &reference.name,
                            egui::FontId::proportional(11.),
                            color,
                        );
                    }
                }
                Geometry::Plane { plane } => {
                    let corners = plane_corners(plane, extent(w));
                    outline(corners, stroke, &mut remaining);
                    if visible_point(corners[0], &mut remaining)
                        && let Some(point) = project(corners[0])
                    {
                        painter.text(
                            point,
                            egui::Align2::LEFT_BOTTOM,
                            &reference.name,
                            egui::FontId::proportional(11.),
                            color,
                        );
                    }
                    if visible_point(plane.origin, &mut remaining)
                        && let Some(point) = project(plane.origin)
                    {
                        painter.line_segment(
                            [point - egui::vec2(5., 0.), point + egui::vec2(5., 0.)],
                            stroke,
                        );
                        painter.line_segment(
                            [point - egui::vec2(0., 5.), point + egui::vec2(0., 5.)],
                            stroke,
                        );
                    }
                }
                Geometry::Axis { axis } => {
                    let endpoints = [-extent(w), extent(w)].map(|distance| {
                        std::array::from_fn(|index| {
                            axis.origin[index] + axis.direction[index] * distance
                        })
                    });
                    draw(endpoints, stroke, &mut remaining);
                    if visible_point(endpoints[1], &mut remaining)
                        && let Some(point) = project(endpoints[1])
                    {
                        painter.text(
                            point,
                            egui::Align2::LEFT_BOTTOM,
                            &reference.name,
                            egui::FontId::proportional(11.),
                            color,
                        );
                    }
                }
                Geometry::Point { point } => {
                    if visible_point(*point, &mut remaining)
                        && let Some(position) = project(*point)
                    {
                        painter.circle_stroke(position, 4., stroke);
                        painter.text(
                            position + egui::vec2(7., -7.),
                            egui::Align2::LEFT_BOTTOM,
                            &reference.name,
                            egui::FontId::proportional(11.),
                            color,
                        );
                    }
                }
            }
        }
    }
}

pub fn optional_axis(
    ui: &mut egui::Ui,
    w: &Workspace,
    doc: &Document,
    current: &str,
    label: &str,
    source: &mut Option<AxisSource>,
) -> bool {
    let mut enabled = source.is_some();
    let mut changed = ui
        .checkbox(&mut enabled, "Use associative reference axis / sketch line")
        .changed();
    if changed {
        *source = enabled.then(default_axis);
    }
    if let Some(source) = source {
        changed |= axis(ui, w, doc, current, label, source);
    }
    changed
}
pub fn optional_plane(
    ui: &mut egui::Ui,
    w: &Workspace,
    doc: &Document,
    current: &str,
    label: &str,
    source: &mut Option<PlaneSource>,
) -> bool {
    let mut enabled = source.is_some();
    let mut changed = ui
        .checkbox(&mut enabled, "Use associative reference plane / face")
        .changed();
    if changed {
        *source = enabled.then(default_plane);
    }
    if let Some(source) = source {
        changed |= plane(ui, w, doc, current, label, source);
    }
    changed
}
pub fn profile_axis(doc: &Document, profile: &NodeRef) -> Option<AxisSource> {
    let part = doc.parts.get(&profile.definition)?;
    let feature = part
        .features
        .iter()
        .find(|f| f.id == profile.node && !f.suppressed)?;
    let Operation::Profile {
        profile: cad_core::Profile::Sketch { sketch },
        ..
    } = &feature.operation
    else {
        return None;
    };
    let sketch = part.sketches.get(sketch)?;
    let mut lines = sketch
        .members
        .iter()
        .filter(|(_, m)| m.construction && matches!(m.curve, cad_core::SketchCurve::Line { .. }));
    let (member, _) = lines.next()?;
    if lines.next().is_some() {
        return None;
    }
    Some(AxisSource::SketchLine {
        profile: profile.clone(),
        member: member.clone(),
    })
}

pub fn axis_fields(
    ui: &mut egui::Ui,
    w: &Workspace,
    doc: &Document,
    current: &str,
    axis: &mut [f64; 3],
    origin: Option<&mut [f64; 3]>,
    reference: &mut Option<AxisSource>,
) -> bool {
    let mut changed = optional_axis(ui, w, doc, current, "Reference Axis", reference);
    ui.add_enabled_ui(reference.is_none(), |ui| {
        ui.horizontal(|ui| {
            for (label, value) in [
                ("X", [1., 0., 0.]),
                ("Y", [0., 1., 0.]),
                ("Z", [0., 0., 1.]),
            ] {
                changed |= ui.selectable_value(axis, value, label).changed();
            }
        });
        if let Some(origin) = origin {
            egui::CollapsingHeader::new("Axis Origin").show(ui, |ui| {
                for (label, value) in ["X", "Y", "Z"].into_iter().zip(origin) {
                    changed |= ui
                        .add(
                            egui::DragValue::new(value)
                                .prefix(format!("{label} "))
                                .suffix(" mm"),
                        )
                        .changed();
                }
            });
        }
    });
    changed
}
pub fn plane_fields(
    ui: &mut egui::Ui,
    w: &Workspace,
    doc: &Document,
    current: &str,
    plane: &mut cad_core::Plane,
    reference: &mut Option<PlaneSource>,
) -> bool {
    let mut changed = optional_plane(ui, w, doc, current, "Reference Plane", reference);
    ui.add_enabled_ui(reference.is_none(), |ui| {
        ui.horizontal_wrapped(|ui| {
            for name in ["Front Plane", "Top Plane", "Right Plane"] {
                if ui.button(name).clicked() {
                    *plane = crate::sketch::plane(name);
                    changed = true;
                }
            }
        });
        for (label, value) in ["Plane X", "Plane Y", "Plane Z"]
            .into_iter()
            .zip(&mut plane.origin)
        {
            changed |= ui
                .add(
                    egui::DragValue::new(value)
                        .speed(0.1)
                        .prefix(format!("{label} "))
                        .suffix(" mm"),
                )
                .changed();
        }
    });
    changed
}

pub fn seed_inputs(w: &Workspace, operation: &mut Operation) {
    let Some(definition) = w.view.active_part.as_ref() else {
        return;
    };
    let Some(part) = w.session.visible().parts.get(definition) else {
        return;
    };
    let selected = w.selected.as_ref().filter(|s| &s.definition == definition);
    let node = selected
        .map(|s| s.reference.producer.clone())
        .or_else(|| w.view.active_node.as_ref().map(|n| part.reference(n)));
    let feature = node
        .as_ref()
        .and_then(|n| part.features.iter().find(|f| f.id == n.node));
    let plane = selected
        .filter(|s| s.reference.topology == "face")
        .map(|s| PlaneSource::Face {
            reference: s.reference.clone(),
            body: selected_body(w, s),
        })
        .or_else(|| {
            selected
                .filter(|s| s.reference.topology == "plane")
                .and_then(|s| PlaneSource::from_datum(&s.reference).ok())
        })
        .or_else(|| {
            feature
                .filter(|f| matches!(f.operation, Operation::ReferencePlane { .. }))
                .map(|f| PlaneSource::Datum {
                    feature: part.reference(&f.id),
                })
        });
    let axis = selected
        .filter(|s| {
            s.reference.topology == "edge"
                || s.reference.topology == "face" && !selected_face_planar(w, s)
        })
        .map(|s| picked_axis(w, s))
        .or_else(|| {
            selected
                .filter(|s| s.reference.topology == "axis")
                .and_then(|s| AxisSource::from_datum(&s.reference).ok())
        })
        .or_else(|| {
            feature
                .filter(|f| matches!(f.operation, Operation::ReferenceAxis { .. }))
                .map(|f| AxisSource::Datum {
                    feature: part.reference(&f.id),
                })
        });
    match operation {
        Operation::Revolve { axis_reference, .. } | Operation::Pattern { axis_reference, .. } => {
            if axis.is_some() {
                *axis_reference = axis;
            }
        }
        Operation::Mirror {
            plane_reference, ..
        }
        | Operation::Draft {
            plane_reference, ..
        } if plane.is_some() => *plane_reference = plane,
        _ => {}
    }
}

pub(crate) fn selected_face_planar(w: &Workspace, selected: &viewport::Selection) -> bool {
    viewport::display_evaluation(w).is_some_and(|evaluation| {
        evaluation.meshes.values().flat_map(|m| &m.faces).any(|f| {
            f.body == selected.body
                && f.reference.producer == selected.reference.producer
                && f.reference.role == selected.reference.role
                && f.plane.is_some()
        })
    })
}
pub(crate) fn picked_axis(w: &Workspace, selected: &viewport::Selection) -> AxisSource {
    let body = selected_body(w, selected);
    if selected.reference.topology == "face" {
        return AxisSource::FaceAxis {
            reference: selected.reference.clone(),
            body,
        };
    }
    let straight = viewport::display_evaluation(w).is_some_and(|evaluation| {
        evaluation.meshes.values().flat_map(|m| &m.edges).any(|e| {
            e.body == selected.body
                && e.reference.producer == selected.reference.producer
                && e.reference.role == selected.reference.role
                && e.curve.as_ref().is_some_and(|curve| {
                    let Some(first) = curve.control_points.first() else {
                        return false;
                    };
                    let Some(last) = curve.control_points.last() else {
                        return false;
                    };
                    let a = DVec3::from_array(*first);
                    let d = DVec3::from_array(*last) - a;
                    d.length() > 1e-9
                        && curve.weights.iter().all(|w| w.is_finite() && *w > 0.)
                        && curve.control_points.iter().all(|p| {
                            (DVec3::from_array(*p) - a).cross(d).length() <= 1e-7 * d.length()
                        })
                })
        })
    });
    if straight {
        AxisSource::Edge {
            reference: selected.reference.clone(),
            body,
        }
    } else {
        AxisSource::CircleEdge {
            reference: selected.reference.clone(),
            body,
        }
    }
}
pub(crate) fn selected_body(w: &Workspace, selected: &viewport::Selection) -> NodeRef {
    if let Some(body) = &selected.body {
        return body.clone();
    }
    let part = w.session.visible().parts.get(&selected.definition);
    let evaluation = viewport::display_evaluation(w);
    let key = evaluation
        .and_then(|e| e.instances.iter().find(|i| i.path == selected.path))
        .map(cad_core::history::mesh_key)
        .unwrap_or(&selected.definition);
    let mut candidates = part
        .into_iter()
        .flat_map(|p| p.outputs.iter())
        .filter(|id| {
            evaluation
                .and_then(|e| e.history.get(key))
                .and_then(|h| h.meshes.get(*id))
                .is_some_and(|mesh| match selected.reference.topology.as_str() {
                    "face" => mesh.faces.iter().any(|f| {
                        f.reference.producer == selected.reference.producer
                            && f.reference.role == selected.reference.role
                    }),
                    "edge" => mesh.edges.iter().any(|e| {
                        e.reference.producer == selected.reference.producer
                            && e.reference.role == selected.reference.role
                    }),
                    _ => false,
                })
        });
    if let Some(node) = candidates.next()
        && candidates.next().is_none()
    {
        return NodeRef {
            definition: selected.definition.clone(),
            node: node.clone(),
        };
    }
    part.and_then(|p| p.outputs.last().map(|n| p.reference(n)))
        .unwrap_or_else(|| selected.reference.producer.clone())
}
