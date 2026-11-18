//! Docked section intent over the shared native feature draft.
use crate::{
    Workspace, cad_ui,
    cad_ui::profile_features::{options_section, scope_fields, thin_fields},
    reference_ui,
};
use bevy_egui::egui;
use cad_core::{
    Document, NodeRef, Operation, Profile, Sketch,
    reference_geometry::{AxisSource, OriginAxis},
    solid_sections::{Connector, End, LoftOptions, Orientation, Placement, SweepOptions},
};
fn source<'a>(doc: &'a Document, profile: &NodeRef) -> Option<&'a Sketch> {
    let part = doc.parts.get(&profile.definition)?;
    let feature = part
        .features
        .iter()
        .find(|feature| feature.id == profile.node)?;
    let Operation::Profile {
        profile: Profile::Sketch { sketch },
        ..
    } = &feature.operation
    else {
        return None;
    };
    part.sketches.get(sketch)
}
type PathEntities = (Vec<(cad_core::Id, String)>, Vec<cad_core::Id>);
fn path_entities(doc: &Document, path: &NodeRef) -> Option<PathEntities> {
    let part = doc.parts.get(&path.definition)?;
    let feature = part
        .features
        .iter()
        .find(|feature| feature.id == path.node)?;
    if let Operation::Sketch3d { definition } = &feature.operation {
        let members = definition
            .members
            .iter()
            .enumerate()
            .map(|(index, (id, member))| {
                let kind = match member.curve {
                    cad_core::sketch3d::Curve::Line { .. } => "Line",
                    cad_core::sketch3d::Curve::Arc { .. } => "Arc",
                    cad_core::sketch3d::Curve::Circle { .. } => "Circle",
                    _ => "Spline",
                };
                (id.clone(), format!("3D {kind} {}", index + 1))
            })
            .collect();
        return Some((members, definition.points.keys().cloned().collect()));
    }
    let sketch = source(doc, path)?;
    let members = sketch
        .members
        .iter()
        .enumerate()
        .map(|(index, (id, member))| {
            let kind = match member.curve {
                cad_core::SketchCurve::Line { .. } => "Line",
                cad_core::SketchCurve::Arc { .. } => "Arc",
                cad_core::SketchCurve::Circle { .. } => "Circle",
                cad_core::SketchCurve::Ellipse { .. } => "Ellipse",
                _ => "Spline",
            };
            (id.clone(), format!("{kind} {}", index + 1))
        })
        .collect();
    Some((members, sketch.points.keys().cloned().collect()))
}
fn end(
    ui: &mut egui::Ui,
    w: &Workspace,
    doc: &Document,
    current: &str,
    label: &str,
    end: &mut End,
) -> bool {
    let mut changed = false;
    ui.push_id(label, |ui| {
        let mode = match end {
            End::Default => 0,
            End::Normal { .. } => 1,
            End::Direction { .. } => 2,
        };
        let mut selected = mode;
        egui::ComboBox::from_label(label)
            .selected_text(["Default", "Normal to Profile", "Direction Vector"][mode])
            .show_ui(ui, |ui| {
                for (index, label) in ["Default", "Normal to Profile", "Direction Vector"]
                    .iter()
                    .enumerate()
                {
                    ui.selectable_value(&mut selected, index, *label);
                }
            });
        if selected != mode {
            *end = match selected {
                1 => End::Normal {
                    influence: "5 mm".into(),
                },
                2 => End::Direction {
                    axis: Box::new(AxisSource::Origin {
                        axis: OriginAxis::Z,
                    }),
                    influence: "5 mm".into(),
                    reverse: false,
                },
                _ => End::Default,
            };
            changed = true;
        }
        match end {
            End::Normal { influence } => {
                changed |= cad_ui::expression(ui, "Tangent Influence", influence)
            }
            End::Direction {
                axis,
                influence,
                reverse,
            } => {
                changed |= reference_ui::axis(ui, w, doc, current, "Tangent Direction", axis);
                changed |= ui.checkbox(reverse, "Reverse Tangent").changed();
                changed |= cad_ui::expression(ui, "Tangent Influence", influence);
            }
            End::Default => {}
        }
    });
    changed
}
pub(crate) fn loft(
    ui: &mut egui::Ui,
    w: &Workspace,
    doc: &Document,
    current: &str,
    profiles: &[NodeRef],
    body: Option<&NodeRef>,
    options: &mut LoftOptions,
) -> bool {
    let mut changed = false;
    changed |= crate::contours_ui::fields(
        ui,
        w,
        doc,
        current,
        profiles,
        &mut options.contours,
        options.thin.is_some(),
    );
    let connectors = if options.connectors.is_empty() {
        "Profile connectors".into()
    } else {
        format!("Profile connectors · {}", options.connectors.len())
    };
    options_section(ui, current, "connectors", &connectors, false, |ui| {
        let mut remove = None;
        for (index, connector) in options.connectors.iter_mut().enumerate() {
            ui.push_id(index, |ui| {
                let profile_index = profiles
                    .iter()
                    .position(|profile| profile == &connector.profile);
                egui::ComboBox::from_label("Section")
                    .selected_text(
                        profile_index
                            .map(|i| format!("Section {}", i + 1))
                            .unwrap_or("Missing Section".into()),
                    )
                    .show_ui(ui, |ui| {
                        for (index, profile) in profiles.iter().enumerate() {
                            changed |= ui
                                .selectable_value(
                                    &mut connector.profile,
                                    profile.clone(),
                                    format!("Section {}", index + 1),
                                )
                                .changed();
                        }
                    });
                let members = crate::contours_ui::members(doc, &connector.profile);
                let region_label = connector
                    .region_member
                    .as_ref()
                    .and_then(|id| {
                        members
                            .iter()
                            .find(|(member, _)| member == id)
                            .map(|(_, label)| format!("Contour through {label}"))
                    })
                    .unwrap_or(if connector.region_member.is_some() {
                        "Missing Region Member".into()
                    } else {
                        format!("Legacy Region {}", connector.region)
                    });
                egui::ComboBox::from_label("Material Region")
                    .selected_text(region_label)
                    .show_ui(ui, |ui| {
                        for (member, label) in &members {
                            changed |= ui
                                .selectable_value(
                                    &mut connector.region_member,
                                    Some(member.clone()),
                                    format!("Contour through {label}"),
                                )
                                .changed();
                        }
                    });
                if connector.region_member.is_none() {
                    ui.horizontal(|ui| {
                        ui.label("Region number");
                        changed |= ui
                            .add(egui::DragValue::new(&mut connector.region).range(0..=1023))
                            .changed();
                    });
                }
                ui.horizontal_wrapped(|ui| {
                    ui.label("Boundary");
                    let mut hole = connector.contour > 0;
                    if ui.selectable_value(&mut hole, false, "Outer").changed() {
                        connector.contour = 0;
                        changed = true;
                    }
                    if ui.selectable_value(&mut hole, true, "Hole").changed() {
                        connector.contour = 1;
                        changed = true;
                    }
                    if hole {
                        changed |= ui
                            .add(
                                egui::DragValue::new(&mut connector.contour)
                                    .range(1..=1023)
                                    .prefix("Hole "),
                            )
                            .changed();
                    }
                });
                let points = source(doc, &connector.profile)
                    .map(|sketch| sketch.points.keys().cloned().collect::<Vec<_>>())
                    .unwrap_or_default();
                let selected = points
                    .iter()
                    .position(|point| point == &connector.vertex)
                    .map(|i| format!("Vertex {}", i + 1))
                    .unwrap_or("Missing Vertex".into());
                egui::ComboBox::from_label("Connector Vertex")
                    .selected_text(selected)
                    .show_ui(ui, |ui| {
                        for (index, point) in points.iter().enumerate() {
                            changed |= ui
                                .selectable_value(
                                    &mut connector.vertex,
                                    point.clone(),
                                    format!("Vertex {}", index + 1),
                                )
                                .changed();
                        }
                    });
                if ui.small_button("Remove Connector").clicked() {
                    remove = Some(index);
                }
            });
        }
        if let Some(index) = remove {
            options.connectors.remove(index);
            changed = true;
        }
        let candidate = profiles.iter().find_map(|profile| {
            let sketch = source(doc, profile)?;
            crate::contours_ui::members(doc, profile)
                .into_iter()
                .find_map(|(member, _)| {
                    let curve = &sketch.members[&member].curve;
                    if matches!(
                        curve,
                        cad_core::SketchCurve::Circle { .. }
                            | cad_core::SketchCurve::Ellipse { .. }
                    ) {
                        return None;
                    }
                    curve
                        .points()
                        .first()
                        .map(|point| (profile.clone(), member.clone(), (*point).to_string()))
                })
        });
        if ui
            .add_enabled(candidate.is_some(), egui::Button::new("Add connector"))
            .on_disabled_hover_text("Choose a section with a boundary vertex.")
            .clicked()
            && let Some((profile, member, vertex)) = candidate
        {
            options.connectors.push(Connector {
                profile,
                region: 0,
                region_member: Some(member),
                contour: 0,
                vertex,
            });
            changed = true;
        }
    });
    options_section(
        ui,
        current,
        "ends",
        "End conditions",
        !matches!(options.start, End::Default) || !matches!(options.end, End::Default),
        |ui| {
            changed |= end(ui, w, doc, current, "Start", &mut options.start);
            changed |= end(ui, w, doc, current, "End", &mut options.end);
        },
    );
    options_section(
        ui,
        current,
        "wall",
        "Thin wall",
        options.thin.is_some(),
        |ui| {
            changed |= thin_fields(ui, &mut options.thin);
        },
    );
    changed |= scope_fields(ui, doc, current, body, &mut options.scope);
    changed
}
pub(crate) fn sweep(
    ui: &mut egui::Ui,
    w: &Workspace,
    current: &str,
    profile: &NodeRef,
    path: Option<&NodeRef>,
    body: Option<&NodeRef>,
    options: &mut SweepOptions,
) -> bool {
    let doc = w.session.visible();
    let mut changed = false;
    changed |= crate::contours_ui::fields(
        ui,
        w,
        doc,
        current,
        std::slice::from_ref(profile),
        &mut options.contours,
        options.thin.is_some(),
    );
    let path_options = options.path_member.is_some()
        || options.path_start_vertex.is_some()
        || options.start.0.trim() != "0"
        || options.end.0.trim() != "1"
        || options.reverse;
    options_section(ui, current, "path", "Path limits", path_options, |ui| {
        if let Some((members, points)) = path.and_then(|path| path_entities(doc, path)) {
            let label = options
                .path_member
                .as_ref()
                .and_then(|id| members.iter().position(|(key, _)| key == id))
                .map(|index| format!("Member {}", index + 1))
                .unwrap_or(
                    if options.path_member.is_some() {
                        "Missing Member"
                    } else {
                        "Whole Path"
                    }
                    .into(),
                );
            egui::ComboBox::from_label("Path Member")
                .selected_text(label)
                .show_ui(ui, |ui| {
                    changed |= ui
                        .selectable_value(&mut options.path_member, None, "Whole Path")
                        .changed();
                    for (id, label) in &members {
                        changed |= ui
                            .selectable_value(&mut options.path_member, Some(id.clone()), label)
                            .changed();
                    }
                });
            let label = options
                .path_start_vertex
                .as_ref()
                .and_then(|id| points.iter().position(|point| point == id))
                .map(|index| format!("Vertex {}", index + 1))
                .unwrap_or(
                    if options.path_start_vertex.is_some() {
                        "Missing Vertex"
                    } else {
                        "Use Member Start"
                    }
                    .into(),
                );
            egui::ComboBox::from_label("Path Start Vertex")
                .selected_text(label)
                .show_ui(ui, |ui| {
                    changed |= ui
                        .selectable_value(&mut options.path_start_vertex, None, "Use Member Start")
                        .changed();
                    for (index, point) in points.iter().enumerate() {
                        changed |= ui
                            .selectable_value(
                                &mut options.path_start_vertex,
                                Some(point.clone()),
                                format!("Vertex {}", index + 1),
                            )
                            .changed();
                    }
                });
        }
        changed |= cad_ui::expression(ui, "Start Parameter", &mut options.start);
        changed |= cad_ui::expression(ui, "End Parameter", &mut options.end);
        changed |= ui.checkbox(&mut options.reverse, "Reverse Path").changed();
    });
    let orientation_options = options.orientation != Orientation::Fixed
        || options.placement != Placement::KeepSource
        || options.roll.0.trim() != "0 deg"
        || options.twist.0.trim() != "0 deg";
    options_section(
        ui,
        current,
        "orientation",
        "Orientation and twist",
        orientation_options,
        |ui| {
            egui::ComboBox::from_label("Profile Orientation")
                .selected_text(match options.orientation {
                    Orientation::Fixed => "Keep Normal Constant",
                    Orientation::FollowTangent => "Follow Path Tangent",
                })
                .show_ui(ui, |ui| {
                    changed |= ui
                        .selectable_value(
                            &mut options.orientation,
                            Orientation::Fixed,
                            "Keep Normal Constant",
                        )
                        .changed();
                    changed |= ui
                        .selectable_value(
                            &mut options.orientation,
                            Orientation::FollowTangent,
                            "Follow Path Tangent",
                        )
                        .changed();
                });
            egui::ComboBox::from_label("Section Placement")
                .selected_text(match options.placement {
                    Placement::KeepSource => "Use Sketch Position",
                    Placement::AtPathStart => "Align at Path Start",
                })
                .show_ui(ui, |ui| {
                    changed |= ui
                        .selectable_value(
                            &mut options.placement,
                            Placement::KeepSource,
                            "Use Sketch Position",
                        )
                        .changed();
                    changed |= ui
                        .selectable_value(
                            &mut options.placement,
                            Placement::AtPathStart,
                            "Align at Path Start",
                        )
                        .changed();
                });
            changed |= cad_ui::expression(ui, "Initial Roll", &mut options.roll);
            changed |= cad_ui::expression(ui, "Total Twist", &mut options.twist);
        },
    );
    options_section(
        ui,
        current,
        "wall",
        "Thin wall",
        options.thin.is_some(),
        |ui| {
            changed |= thin_fields(ui, &mut options.thin);
        },
    );
    changed |= scope_fields(ui, doc, current, body, &mut options.scope);
    changed
}
