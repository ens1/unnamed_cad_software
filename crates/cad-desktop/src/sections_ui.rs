//! Docked section intent over the shared native feature draft.
use crate::{Workspace, cad_ui, reference_ui};
use bevy_egui::egui;
use cad_core::{
    Document, NodeRef, Operation, Profile, Sketch, ThinExtrude, ThinExtrudeMode,
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
fn thin(ui: &mut egui::Ui, thin: &mut Option<ThinExtrude>) -> bool {
    let mut changed = false;
    let mut enabled = thin.is_some();
    if ui.checkbox(&mut enabled, "Thin Feature").changed() {
        *thin = enabled.then(|| ThinExtrude {
            mode: ThinExtrudeMode::OneSide,
            thickness: "2 mm".into(),
            second_thickness: None,
            reverse: false,
        });
        changed = true;
    }
    if let Some(thin) = thin {
        egui::ComboBox::from_label("Thickness Mode")
            .selected_text(match thin.mode {
                ThinExtrudeMode::OneSide => "One Side",
                ThinExtrudeMode::TwoSide => "Two Sides",
                ThinExtrudeMode::MidPlane => "Mid Plane",
            })
            .show_ui(ui, |ui| {
                for (mode, label) in [
                    (ThinExtrudeMode::OneSide, "One Side"),
                    (ThinExtrudeMode::TwoSide, "Two Sides"),
                    (ThinExtrudeMode::MidPlane, "Mid Plane"),
                ] {
                    changed |= ui.selectable_value(&mut thin.mode, mode, label).changed();
                }
            });
        changed |= cad_ui::expression(ui, "Thickness", &mut thin.thickness);
        if thin.mode == ThinExtrudeMode::TwoSide {
            if thin.second_thickness.is_none() {
                thin.second_thickness = Some("2 mm".into());
                changed = true;
            }
            changed |=
                cad_ui::expression(ui, "Thickness 2", thin.second_thickness.as_mut().unwrap());
        }
        changed |= ui
            .checkbox(&mut thin.reverse, "Reverse Thickness Side")
            .changed();
        ui.small("Straight and circular sketch offsets; open chains receive flat ends.");
    }
    changed
}
fn scope(
    ui: &mut egui::Ui,
    doc: &Document,
    current: &str,
    body: Option<&NodeRef>,
    scope: &mut Vec<NodeRef>,
) -> bool {
    let mut changed = false;
    egui::CollapsingHeader::new("Additional Feature Scope").show(ui, |ui| {
        for part in doc.parts.values() {
            for feature in part
                .features
                .iter()
                .take_while(|feature| feature.id != current)
                .filter(|feature| !feature.suppressed && feature.operation.produces_solid())
            {
                let reference = part.reference(&feature.id);
                if Some(&reference) == body {
                    continue;
                }
                let mut included = scope.contains(&reference);
                if ui
                    .checkbox(&mut included, format!("{} / {}", part.name, feature.name))
                    .changed()
                {
                    if included {
                        scope.push(reference);
                    } else {
                        scope.retain(|old| old != &reference);
                    }
                    changed = true;
                }
            }
        }
        ui.small("Includes all current solid bodies produced by each selected feature.");
    });
    changed
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
        doc,
        profiles,
        &mut options.contours,
        options.thin.is_some(),
    );
    egui::CollapsingHeader::new("Profile Connectors").show(ui,|ui| {
        let mut remove=None;
        for (index,connector) in options.connectors.iter_mut().enumerate(){ui.push_id(index,|ui| {
            let profile_index=profiles.iter().position(|profile|profile==&connector.profile);
            egui::ComboBox::from_label("Section").selected_text(profile_index.map(|i|format!("Section {}",i+1)).unwrap_or("Missing Section".into())).show_ui(ui,|ui| {
                for (index,profile) in profiles.iter().enumerate(){changed|=ui.selectable_value(&mut connector.profile,profile.clone(),format!("Section {}",index+1)).changed();}
            });
            let members=crate::contours_ui::members(doc,&connector.profile);
            let region_label=connector.region_member.as_ref().and_then(|id|members.iter().find(|(member,_)|member==id).map(|(_,label)|format!("Contour through {label}"))).unwrap_or(if connector.region_member.is_some(){"Missing Region Member".into()}else{format!("Legacy Region {}",connector.region)});
            egui::ComboBox::from_label("Material Region").selected_text(region_label).show_ui(ui,|ui| {
                for (member,label) in &members {changed|=ui.selectable_value(&mut connector.region_member,Some(member.clone()),format!("Contour through {label}")).changed();}
            });
            if connector.region_member.is_none(){ui.horizontal(|ui| {ui.label("Legacy Region Index");changed|=ui.add(egui::DragValue::new(&mut connector.region).range(0..=1023)).changed();});ui.small("This older connector uses a region index. Choose an outer-boundary member to make its region association stable.");}
            ui.horizontal(|ui| {ui.label("Contour");changed|=ui.add(egui::DragValue::new(&mut connector.contour).range(0..=1023)).changed();});
            let points=source(doc,&connector.profile).map(|sketch|sketch.points.keys().cloned().collect::<Vec<_>>()).unwrap_or_default();
            let selected=points.iter().position(|point|point==&connector.vertex).map(|i|format!("Vertex {}",i+1)).unwrap_or("Missing Vertex".into());
            egui::ComboBox::from_label("Connector Vertex").selected_text(selected).show_ui(ui,|ui| {
                for (index,point) in points.iter().enumerate(){changed|=ui.selectable_value(&mut connector.vertex,point.clone(),format!("Vertex {}",index+1)).changed();}
            });
            if ui.small_button("Remove Connector").clicked(){remove=Some(index);}
        });}
        if let Some(index)=remove {options.connectors.remove(index);changed=true;}
        if ui.button("Add Connector").clicked()&&let Some((profile,member,vertex))=profiles.iter().find_map(|profile| {
            let sketch=source(doc,profile)?;
            crate::contours_ui::members(doc,profile).into_iter().find_map(|(member,_)| {
                let curve=&sketch.members[&member].curve;
                if matches!(curve,cad_core::SketchCurve::Circle {..}|cad_core::SketchCurve::Ellipse {..}){return None;}
                curve.points().first().map(|point|(profile.clone(),member.clone(),(*point).to_string()))
            })
        }) {
            options.connectors.push(Connector {profile,region:0,region_member:Some(member),contour:0,vertex});changed=true;
        }
        ui.small("Choose a region by an outer-boundary member and an actual boundary vertex to align its seam. Contour 0 is the outside; contained holes follow source-member order.");
    });
    changed |= end(ui, w, doc, current, "Start Constraint", &mut options.start);
    changed |= end(ui, w, doc, current, "End Constraint", &mut options.end);
    changed |= thin(ui, &mut options.thin);
    changed |= scope(ui, doc, current, body, &mut options.scope);
    ui.small("Use parallel sketch sections. Tangent influence controls the end shape; folded or incompatible sections report an error before Apply.");
    changed
}
pub(crate) fn sweep(
    ui: &mut egui::Ui,
    doc: &Document,
    current: &str,
    profile: &NodeRef,
    path: Option<&NodeRef>,
    body: Option<&NodeRef>,
    options: &mut SweepOptions,
) -> bool {
    let mut changed = false;
    changed |= crate::contours_ui::fields(
        ui,
        doc,
        std::slice::from_ref(profile),
        &mut options.contours,
        options.thin.is_some(),
    );
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
    if let Some(sketch) = path.and_then(|path| source(doc, path)) {
        let members = sketch.members.iter().collect::<Vec<_>>();
        let label = options
            .path_member
            .as_ref()
            .and_then(|id| members.iter().position(|(key, _)| *key == id))
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
                for (index, (id, member)) in members.iter().enumerate() {
                    let kind = match &member.curve {
                        cad_core::SketchCurve::Line { .. } => "Line",
                        cad_core::SketchCurve::Arc { .. } => "Arc",
                        cad_core::SketchCurve::Circle { .. } => "Circle",
                        cad_core::SketchCurve::Ellipse { .. } => "Ellipse",
                        _ => "Spline",
                    };
                    changed |= ui
                        .selectable_value(
                            &mut options.path_member,
                            Some((*id).clone()),
                            format!("{kind} {}", index + 1),
                        )
                        .changed();
                }
            });
        let points = sketch.points.keys().collect::<Vec<_>>();
        let label = options
            .path_start_vertex
            .as_ref()
            .and_then(|id| points.iter().position(|point| *point == id))
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
                            Some((*point).clone()),
                            format!("Vertex {}", index + 1),
                        )
                        .changed();
                }
            });
        ui.small("A chain needs a selected endpoint to keep its direction associated with the sketch. Single members use their declared start.");
    }
    changed |= cad_ui::expression(ui, "Start Parameter", &mut options.start);
    changed |= cad_ui::expression(ui, "End Parameter", &mut options.end);
    changed |= ui.checkbox(&mut options.reverse, "Reverse Path").changed();
    changed |= cad_ui::expression(ui, "Initial Roll", &mut options.roll);
    changed |= cad_ui::expression(ui, "Total Twist", &mut options.twist);
    ui.small("Range 0–1 gives each path member an equal share. Twist changes the angle nonlinearly within each path segment.");
    changed |= thin(ui, &mut options.thin);
    changed |= scope(ui, doc, current, body, &mut options.scope);
    ui.small("A fixed profile can follow lines or splines that keep moving across its sketch plane. Follow Tangent supports straight paths and sketch arcs/circles. Circular paths require zero twist and a section clear of the circle's center axis.");
    changed
}
