//! Docked native sheet/weldment authoring through document commands and shared drafts.
use crate::{Workspace, ui, viewport};
use bevy_egui::egui;
use cad_core::{Document, Expr, Feature, NodeRef, Operation, commands::Command, sheet_metal::*};
const DEFERRED: &str = "tcad:fabrication-preset:";
pub fn is_preset(name: &str) -> bool {
    name.starts_with(DEFERRED) || native_preset(name)
}
fn native_preset(name: &str) -> bool {
    matches!(
        name,
        "Base Flange / Tab"
            | "Sheet Tab"
            | "Edge Flange"
            | "Flat Pattern"
            | "Formed Sheet"
            | "Structural Member Group"
            | "Trim / Extend Member"
            | "Weldment End Cap"
            | "Sketch Gusset"
    )
}
fn queue_after_confirm(w: &mut Workspace, name: &str) -> bool {
    let Some(definition) = w
        .sketch
        .as_ref()
        .map(|sketch| sketch.definition.as_str())
        .or(w.view.active_part.as_deref())
    else {
        w.status = "Create or select the intended fabrication part first".into();
        return false;
    };
    match serde_json::to_string(&(name, &w.session.visible().id, definition)) {
        Ok(payload) => {
            w.after_confirm = Some(format!("{DEFERRED}{payload}"));
            true
        }
        Err(error) => {
            w.status = error.to_string();
            false
        }
    }
}
fn sheet_choices(doc: &Document, current: &str) -> Vec<(NodeRef, String)> {
    doc.parts
        .values()
        .flat_map(|part| {
            part.features
                .iter()
                .take_while(|f| f.id != current)
                .filter(|f| !f.suppressed && matches!(f.operation, Operation::Sheet { .. }))
                .map(move |f| (part.reference(&f.id), format!("{} / {}", part.name, f.name)))
        })
        .collect()
}
fn profiles(doc: &Document, current: &str) -> Vec<(NodeRef, String)> {
    doc.parts
        .values()
        .flat_map(|part| {
            part.features
                .iter()
                .take_while(|f| f.id != current)
                .filter(|f| !f.suppressed && matches!(f.operation, Operation::Profile { .. }))
                .map(move |f| (part.reference(&f.id), format!("{} / {}", part.name, f.name)))
        })
        .collect()
}
fn selector(
    ui: &mut egui::Ui,
    label: &str,
    reference: &mut NodeRef,
    choices: Vec<(NodeRef, String)>,
) -> bool {
    let mut changed = false;
    egui::ComboBox::from_label(label)
        .selected_text(
            choices
                .iter()
                .find(|(r, _)| r == reference)
                .map(|(_, name)| name.as_str())
                .unwrap_or("Select source"),
        )
        .show_ui(ui, |ui| {
            for (source, name) in choices {
                changed |= ui.selectable_value(reference, source, name).changed();
            }
        });
    changed
}
fn expression(ui: &mut egui::Ui, label: &str, value: &mut Expr) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.label(label);
        changed = ui.text_edit_singleline(&mut value.0).changed();
    });
    changed
}
pub(crate) fn members(w: &Workspace, stock: &Stock) -> Vec<(String, String)> {
    let source = source_sketch(w, &stock.profile);
    path_members(w, &stock.profile)
        .into_iter()
        .filter(|(id, _)| {
            source
                .as_ref()
                .and_then(|sketch| sketch.members.get(id))
                .is_some_and(|member| !member.construction)
        })
        .collect()
}
pub fn preset(w: &mut Workspace, name: &str) {
    let decoded = name
        .strip_prefix(DEFERRED)
        .map(serde_json::from_str::<(String, String, String)>)
        .transpose();
    let name = match decoded {
        Ok(Some((name, document, definition))) => {
            if document != w.session.visible().id
                || w.view.active_part.as_deref() != Some(definition.as_str())
            {
                w.status = "The pending fabrication action belongs to another document or part; invoke it again in the intended part".into();
                return;
            }
            name
        }
        Ok(None) => name.to_string(),
        Err(error) => {
            w.status = format!("Deferred fabrication action is invalid: {error}");
            return;
        }
    };
    let name = name.as_str();
    if !native_preset(name) {
        w.status = "The pending fabrication preset is unavailable".into();
        return;
    }
    if matches!(
        name,
        "Structural Member Group" | "Trim / Extend Member" | "Weldment End Cap" | "Sketch Gusset"
    ) {
        weldment_preset(w, name);
        return;
    }
    crate::history::latest(w);
    if w.confirm_requested {
        queue_after_confirm(w, name);
        return;
    }
    if w.sketch.is_some() {
        if !queue_after_confirm(w, name) {
            return;
        }
        w.confirm_tool();
        if w.sketch.is_some() && !w.confirm_requested {
            w.after_confirm = None;
        }
        return;
    }
    if w.tool_checkpoint.is_some() {
        w.status = "Confirm or cancel the current feature first".into();
        return;
    }
    let Some(mut part) = w
        .view
        .active_part
        .as_ref()
        .and_then(|id| w.session.visible().parts.get(id))
        .cloned()
    else {
        w.status = "Create or select a part first".into();
        return;
    };
    let profile = part
        .features
        .iter()
        .find(|f| {
            w.view.active_node.as_ref() == Some(&f.id)
                && !f.suppressed
                && matches!(f.operation, Operation::Profile { .. })
        })
        .or_else(|| {
            part.features
                .iter()
                .rev()
                .find(|f| !f.suppressed && matches!(f.operation, Operation::Profile { .. }))
        })
        .map(|f| part.reference(&f.id));
    let body = w
        .selected
        .as_ref()
        .and_then(|s| s.body.as_ref())
        .filter(|r| {
            r.definition == part.id
                && part.features.iter().any(|f| {
                    f.id == r.node
                        && !f.suppressed
                        && matches!(f.operation, Operation::Sheet { .. })
                })
        })
        .cloned()
        .or_else(|| {
            part.features
                .iter()
                .find(|f| {
                    w.view.active_node.as_ref() == Some(&f.id)
                        && !f.suppressed
                        && matches!(f.operation, Operation::Sheet { .. })
                })
                .or_else(|| {
                    part.features.iter().rev().find(|f| {
                        !f.suppressed
                            && part.outputs.contains(&f.id)
                            && matches!(f.operation, Operation::Sheet { .. })
                    })
                })
                .map(|f| part.reference(&f.id))
        });
    let operation = match name {
        "Base Flange / Tab" => {
            let Some(profile) = profile else {
                crate::sketch::begin(w, None);
                w.status = "Draw a closed base sketch, finish it, then create sheet stock".into();
                return;
            };
            SheetOperation::Base {
                profile,
                thickness: "1 mm".into(),
                inside_radius: "1 mm".into(),
                k_factor: "0.4".into(),
                side: ThicknessSide::Positive,
            }
        }
        "Sheet Tab" => {
            let (Some(body), Some(profile)) = (body, profile) else {
                w.status = "Create sheet stock and a coplanar tab sketch first".into();
                return;
            };
            SheetOperation::Tab { body, profile }
        }
        "Edge Flange" => {
            let Some(body) = body else {
                w.status = "Select native sheet stock first".into();
                return;
            };
            let stock = match cad_core::sheet_metal::stock(w.session.visible(), &body) {
                Ok(stock) => stock,
                Err(error) => {
                    w.status = error.to_string();
                    return;
                }
            };
            let Some(member) = members(w, &stock)
                .into_iter()
                .find(|(id, _)| !stock.flanges.iter().any(|f| f.member == *id))
                .map(|(id, _)| id)
            else {
                w.status = "Base sketch needs an unused straight outer member for a flange".into();
                return;
            };
            SheetOperation::EdgeFlange {
                body,
                member,
                length: "20 mm".into(),
                angle: "90 deg".into(),
                inside_radius: None,
                k_factor: None,
            }
        }
        "Flat Pattern" | "Formed Sheet" => {
            let Some(body) = body else {
                w.status = "Select native sheet stock first".into();
                return;
            };
            SheetOperation::Flatten {
                body,
                state: if name == "Flat Pattern" {
                    State::Flat
                } else {
                    State::Formed
                },
            }
        }
        _ => return,
    };
    let feature = Feature {
        id: cad_core::id(),
        name: format!(
            "{}{}",
            name,
            part.features
                .iter()
                .filter(|f| f.name.starts_with(name))
                .count()
                + 1
        ),
        operation: Operation::Sheet {
            operation: Box::new(operation),
        },
        suppressed: false,
    };
    for source in feature
        .operation
        .consumed_outputs()
        .iter()
        .filter(|r| r.definition == part.id)
    {
        part.outputs.retain(|id| id != &source.node);
    }
    part.outputs.push(feature.id.clone());
    part.features.push(feature.clone());
    w.start_tool();
    w.dispatch(Command::PutPart {
        part,
        assembly: None,
        placement: Default::default(),
    });
    w.view.active_node = Some(feature.id.clone());
    ui::feature_dialog(w, Some(feature));
}
pub fn sheet_fields(
    ui: &mut egui::Ui,
    w: &Workspace,
    current: &str,
    operation: &mut SheetOperation,
) -> bool {
    let doc = w.session.visible();
    let mut changed = false;
    match operation {
        SheetOperation::Base {
            profile,
            thickness,
            inside_radius,
            k_factor,
            side,
        } => {
            changed |= selector(ui, "Base Sketch", profile, profiles(doc, current));
            changed |= expression(ui, "Thickness", thickness);
            changed |= expression(ui, "Default Inside Radius", inside_radius);
            changed |= expression(ui, "K Factor", k_factor);
            egui::ComboBox::from_label("Thickness Direction")
                .selected_text(format!("{side:?}"))
                .show_ui(ui, |ui| {
                    for (value, label) in [
                        (ThicknessSide::Positive, "Along sketch normal"),
                        (ThicknessSide::Negative, "Reverse"),
                        (ThicknessSide::MidPlane, "Mid-plane"),
                    ] {
                        changed |= ui.selectable_value(side, value, label).changed();
                    }
                });
            ui.small(
                "Closed sketch contours, inner holes and separate blank regions are supported.",
            );
        }
        SheetOperation::Tab { body, profile } => {
            changed |= selector(ui, "Sheet Stock", body, sheet_choices(doc, current));
            changed |= selector(ui, "Coplanar Tab Sketch", profile, profiles(doc, current));
            ui.small("Tabs join overlapping coplanar stock before bends. Tangent-only contacts require a joined source sketch.");
        }
        SheetOperation::EdgeFlange {
            body,
            member,
            length,
            angle,
            inside_radius,
            k_factor,
        } => {
            changed |= selector(ui, "Sheet Stock", body, sheet_choices(doc, current));
            if let Ok(stock) = cad_core::sheet_metal::stock(doc, body) {
                let choices = members(w, &stock);
                egui::ComboBox::from_label("Base Sketch Edge")
                    .selected_text(
                        choices
                            .iter()
                            .find(|(id, _)| id == member)
                            .map(|(_, name)| name.as_str())
                            .unwrap_or("Select straight outer edge"),
                    )
                    .show_ui(ui, |ui| {
                        for (id, name) in choices {
                            changed |= ui.selectable_value(member, id, name).changed();
                        }
                    });
            }
            changed |= expression(ui, "Straight Tangent Length", length);
            changed |= expression(ui, "Bend Angle", angle);
            let mut custom = inside_radius.is_some();
            if ui.checkbox(&mut custom, "Override Inside Radius").changed() {
                *inside_radius = custom.then(|| "1 mm".into());
                changed = true;
            }
            if let Some(radius) = inside_radius {
                changed |= expression(ui, "Inside Radius", radius);
            }
            let mut custom = k_factor.is_some();
            if ui.checkbox(&mut custom, "Override K Factor").changed() {
                *k_factor = custom.then(|| "0.4".into());
                changed = true;
            }
            if let Some(k) = k_factor {
                changed |= expression(ui, "K Factor", k);
            }
            ui.small("Full-width supporting straight edges; signed bends through 90°. Intersecting flange envelopes require corner relief. Length starts after the bend tangent.");
        }
        SheetOperation::Flatten { body, state } => {
            changed |= selector(ui, "Sheet Stock", body, sheet_choices(doc, current));
            egui::ComboBox::from_label("Display / Output State")
                .selected_text(format!("{state:?}"))
                .show_ui(ui, |ui| {
                    changed |= ui
                        .selectable_value(state, State::Flat, "Flat Pattern")
                        .changed();
                    changed |= ui
                        .selectable_value(state, State::Formed, "Formed")
                        .changed();
                });
            ui.small("The flat B-rep and bend lines derive from the same thickness, radius, signed angle and K factor as the formed stock.");
        }
    }
    changed
}

fn weldment_choices(doc: &Document, current: &str) -> Vec<(NodeRef, String)> {
    doc.parts
        .values()
        .flat_map(|part| {
            part.features
                .iter()
                .take_while(|feature| feature.id != current)
                .filter(|feature| {
                    !feature.suppressed && matches!(feature.operation, Operation::Weldment { .. })
                })
                .map(move |feature| {
                    (
                        part.reference(&feature.id),
                        format!("{} / {}", part.name, feature.name),
                    )
                })
        })
        .collect()
}

pub(crate) fn source_sketch(w: &Workspace, profile: &NodeRef) -> Option<cad_core::Sketch> {
    let mut sketch = viewport::display_evaluation(w)
        .and_then(|evaluation| {
            evaluation
                .sketches
                .get(&format!("{}::{}", profile.definition, profile.node))
        })
        .and_then(|output| output.solved.clone())
        .or_else(|| {
            let part = w.session.visible().parts.get(&profile.definition)?;
            let feature = part
                .features
                .iter()
                .find(|feature| feature.id == profile.node)?;
            let Operation::Profile {
                profile: definition,
                ..
            } = &feature.operation
            else {
                return None;
            };
            if let cad_core::Profile::Sketch { sketch } = definition {
                return part.sketches.get(sketch).cloned();
            }
            let mut sketch = cad_core::Sketch {
                id: profile.node.clone(),
                name: feature.name.clone(),
                points: Default::default(),
                segments: vec![],
                constraints: vec![],
                profile_order: vec![],
                circles: Default::default(),
                members: Default::default(),
                conversions: Default::default(),
                offsets: Default::default(),
                fillets: Default::default(),
                chamfers: Default::default(),
                transforms: Default::default(),
                splines: Default::default(),
                conics: Default::default(),
                texts: Default::default(),
                equations: Default::default(),
                blocks: Default::default(),
                block_instances: Default::default(),
                dimension_groups: Default::default(),
            };
            match definition {
                cad_core::Profile::Polygon { points } => {
                    for (index, point) in points.iter().enumerate() {
                        let id = format!("p{index}");
                        sketch.points.insert(id.clone(), *point);
                        sketch.profile_order.push(id);
                    }
                }
                cad_core::Profile::Circle { radius, center } => {
                    sketch.points.insert("center".into(), *center);
                    sketch.members.insert(
                        "circle".into(),
                        cad_core::SketchMember {
                            construction: false,
                            curve: cad_core::SketchCurve::Circle {
                                center: "center".into(),
                                radius: radius.clone(),
                            },
                        },
                    );
                }
                cad_core::Profile::Sketch { .. } => unreachable!(),
            }
            Some(sketch)
        })?;
    if !sketch.profile_order.is_empty() {
        for (index, (start, end)) in sketch
            .profile_order
            .iter()
            .zip(sketch.profile_order.iter().cycle().skip(1))
            .take(sketch.profile_order.len())
            .enumerate()
        {
            sketch.members.insert(
                format!("legacy-profile-edge-{index}"),
                cad_core::SketchMember {
                    construction: false,
                    curve: cad_core::SketchCurve::Line {
                        start: start.clone(),
                        end: end.clone(),
                    },
                },
            );
        }
    } else {
        for (index, [start, end]) in sketch.segments.iter().enumerate() {
            sketch.members.insert(
                format!("legacy-segment-{index}"),
                cad_core::SketchMember {
                    construction: false,
                    curve: cad_core::SketchCurve::Line {
                        start: start.clone(),
                        end: end.clone(),
                    },
                },
            );
        }
    }
    Some(sketch)
}

fn closed_profile(w: &Workspace, profile: &NodeRef) -> bool {
    let Some(sketch) = source_sketch(w, profile) else {
        return viewport::display_evaluation(w)
            .and_then(|evaluation| {
                evaluation
                    .sketches
                    .get(&format!("{}::{}", profile.definition, profile.node))
            })
            .is_some_and(|output| !output.profile_order.is_empty() || !output.circles.is_empty());
    };
    if !sketch.profile_order.is_empty() || !sketch.circles.is_empty() {
        return true;
    }
    let mut degree = std::collections::BTreeMap::new();
    for (id, _) in sketch
        .members
        .iter()
        .filter(|(_, member)| !member.construction)
    {
        let Ok(curve) = cad_core::sketch_edit::effective_curve(&sketch, id) else {
            return false;
        };
        let (start, end) = match &curve {
            cad_core::SketchCurve::Circle { .. } | cad_core::SketchCurve::Ellipse { .. } => {
                return true;
            }
            cad_core::SketchCurve::Line { start, end }
            | cad_core::SketchCurve::Arc { start, end, .. } => (start, end),
            cad_core::SketchCurve::Bezier { points }
            | cad_core::SketchCurve::Nurbs { points, .. } => {
                let (Some(start), Some(end)) = (points.first(), points.last()) else {
                    return false;
                };
                (start, end)
            }
        };
        *degree.entry(start.clone()).or_insert(0usize) += 1;
        *degree.entry(end.clone()).or_insert(0usize) += 1;
    }
    !degree.is_empty() && degree.values().all(|degree| *degree == 2)
}

pub(crate) fn path_members(w: &Workspace, profile: &NodeRef) -> Vec<(String, String)> {
    source_sketch(w, profile)
        .map(|sketch| {
            sketch
                .members
                .keys()
                .filter_map(|id| {
                    let cad_core::SketchCurve::Line { start, end } =
                        cad_core::sketch_edit::effective_curve(&sketch, id).ok()?
                    else {
                        return None;
                    };
                    let a = sketch.points.get(&start)?;
                    let b = sketch.points.get(&end)?;
                    Some((
                        id.clone(),
                        format!("({:.2}, {:.2}) → ({:.2}, {:.2}) mm", a[0], a[1], b[0], b[1]),
                    ))
                })
                .collect()
        })
        .unwrap_or_default()
}

pub(crate) fn member_choices(
    w: &Workspace,
    stock: &cad_core::weldment::Stock,
) -> Vec<(cad_core::weldment::MemberRef, String)> {
    stock
        .groups
        .iter()
        .flat_map(|group| {
            let group_name = w
                .session
                .visible()
                .parts
                .get(&group.source.definition)
                .and_then(|part| {
                    part.features
                        .iter()
                        .find(|feature| feature.id == group.source.node)
                })
                .map(|feature| feature.name.as_str())
                .unwrap_or("Missing group");
            path_members(w, &group.path)
                .into_iter()
                .filter(|(id, _)| group.members.is_empty() || group.members.contains(id))
                .map(move |(member, label)| {
                    (
                        cad_core::weldment::MemberRef {
                            group: group.source.clone(),
                            member,
                        },
                        format!("{group_name} · {label}"),
                    )
                })
        })
        .collect()
}

fn end_fields(ui: &mut egui::Ui, end: &mut cad_core::weldment::End) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.label("Member End");
        changed |= ui
            .selectable_value(end, cad_core::weldment::End::Start, "Start")
            .changed();
        changed |= ui
            .selectable_value(end, cad_core::weldment::End::Finish, "Finish")
            .changed();
    });
    changed
}

fn inherited_stock(
    ui: &mut egui::Ui,
    doc: &Document,
    current: &str,
    body: &mut Option<NodeRef>,
) -> bool {
    let choices = weldment_choices(doc, current);
    let mut append = body.is_some();
    let mut changed = false;
    if ui
        .add_enabled(
            !choices.is_empty() || append,
            egui::Checkbox::new(&mut append, "Append to Existing Weldment"),
        )
        .changed()
    {
        *body = if append {
            choices.last().map(|(reference, _)| reference.clone())
        } else {
            None
        };
        changed = true;
    }
    if let Some(reference) = body {
        changed |= selector(ui, "Weldment", reference, choices);
    }
    changed
}

fn weldment_preset(w: &mut Workspace, name: &str) {
    use cad_core::weldment::{Corner, End, WeldmentOperation};
    crate::history::latest(w);
    if w.confirm_requested {
        queue_after_confirm(w, name);
        return;
    }
    if w.sketch.is_some() {
        if !queue_after_confirm(w, name) {
            return;
        }
        w.confirm_tool();
        if w.sketch.is_some() && !w.confirm_requested {
            w.after_confirm = None;
        }
        return;
    }
    if w.tool_checkpoint.is_some() {
        w.status = "Confirm or cancel the current feature first".into();
        return;
    }
    let Some(mut part) = w
        .view
        .active_part
        .as_ref()
        .and_then(|id| w.session.visible().parts.get(id))
        .cloned()
    else {
        w.status = "Create or select a part first".into();
        return;
    };
    let body = w
        .selected
        .as_ref()
        .and_then(|selected| selected.body.as_ref())
        .filter(|reference| {
            reference.definition == part.id
                && part.features.iter().any(|feature| {
                    feature.id == reference.node
                        && !feature.suppressed
                        && matches!(feature.operation, Operation::Weldment { .. })
                })
        })
        .cloned()
        .or_else(|| {
            part.features
                .iter()
                .find(|feature| {
                    w.view.active_node.as_ref() == Some(&feature.id)
                        && !feature.suppressed
                        && matches!(feature.operation, Operation::Weldment { .. })
                })
                .or_else(|| {
                    part.features.iter().rev().find(|feature| {
                        !feature.suppressed
                            && part.outputs.contains(&feature.id)
                            && matches!(feature.operation, Operation::Weldment { .. })
                    })
                })
                .map(|feature| part.reference(&feature.id))
        });
    let profiles = part
        .features
        .iter()
        .filter(|feature| {
            !feature.suppressed && matches!(feature.operation, Operation::Profile { .. })
        })
        .map(|feature| part.reference(&feature.id))
        .collect::<Vec<_>>();
    let closed = |profile: &NodeRef| closed_profile(w, profile);
    let profile = profiles
        .iter()
        .find(|profile| w.view.active_node.as_ref() == Some(&profile.node))
        .or_else(|| profiles.iter().rev().find(|profile| closed(profile)))
        .cloned();
    let operation = match name {
        "Structural Member Group" => {
            let Some(section) = profiles.iter().find(|profile| closed(profile)).cloned() else {
                w.status =
                    "Create a closed section sketch and a separate straight path sketch first"
                        .into();
                return;
            };
            let Some(path) = profiles
                .iter()
                .rev()
                .find(|profile| **profile != section && !path_members(w, profile).is_empty())
                .cloned()
            else {
                w.status = "Create a separate straight path sketch for the member section".into();
                return;
            };
            WeldmentOperation::Members {
                body,
                section,
                path,
                members: vec![],
                roll: "0 deg".into(),
                anchor: None,
                start_offset: "0 mm".into(),
                end_offset: "0 mm".into(),
                corner: Corner::Miter,
            }
        }
        "Trim / Extend Member" => {
            let Some(body) = body else {
                w.status = "Select native weldment stock first".into();
                return;
            };
            let stock = match cad_core::weldment::stock(w.session.visible(), &body) {
                Ok(stock) => stock,
                Err(error) => {
                    w.status = error.to_string();
                    return;
                }
            };
            let Some((member, _)) = member_choices(w, &stock).into_iter().next() else {
                w.status = "Weldment needs a straight sketch member".into();
                return;
            };
            let plane = w
                .selected
                .as_ref()
                .filter(|selected| selected.reference.topology == "face")
                .and_then(|selected| {
                    selected.body.clone().map(|body| {
                        cad_core::reference_geometry::PlaneSource::Face {
                            reference: selected.reference.clone(),
                            body,
                        }
                    })
                })
                .unwrap_or(cad_core::reference_geometry::PlaneSource::Origin {
                    plane: cad_core::reference_geometry::OriginPlane::Right,
                });
            WeldmentOperation::Trim {
                body,
                members: vec![member],
                plane,
                offset: "0 mm".into(),
                end: End::Finish,
            }
        }
        "Weldment End Cap" => {
            let Some(body) = body else {
                w.status = "Select native weldment stock first".into();
                return;
            };
            let stock = match cad_core::weldment::stock(w.session.visible(), &body) {
                Ok(stock) => stock,
                Err(error) => {
                    w.status = error.to_string();
                    return;
                }
            };
            let Some((member, _)) = member_choices(w, &stock).into_iter().next() else {
                w.status = "Weldment needs a member end".into();
                return;
            };
            WeldmentOperation::EndCap {
                body,
                member,
                end: End::Finish,
                thickness: "2 mm".into(),
                offset: "0 mm".into(),
            }
        }
        "Sketch Gusset" => {
            let Some(profile) = profile else {
                crate::sketch::begin(w, None);
                w.status = "Draw a closed gusset sketch, then add its thickness".into();
                return;
            };
            WeldmentOperation::Gusset {
                body,
                profile,
                thickness: "2 mm".into(),
                side: ThicknessSide::MidPlane,
            }
        }
        _ => return,
    };
    let feature = Feature {
        id: cad_core::id(),
        name: format!(
            "{}{}",
            name,
            part.features
                .iter()
                .filter(|feature| feature.name.starts_with(name))
                .count()
                + 1
        ),
        operation: Operation::Weldment {
            operation: Box::new(operation),
        },
        suppressed: false,
    };
    for source in feature
        .operation
        .consumed_outputs()
        .iter()
        .filter(|reference| reference.definition == part.id)
    {
        part.outputs.retain(|id| id != &source.node);
    }
    part.outputs.push(feature.id.clone());
    part.features.push(feature.clone());
    w.start_tool();
    w.dispatch(Command::PutPart {
        part,
        assembly: None,
        placement: Default::default(),
    });
    w.view.active_node = Some(feature.id.clone());
    ui::feature_dialog(w, Some(feature));
}

pub fn weldment_fields(
    ui: &mut egui::Ui,
    w: &Workspace,
    current: &str,
    operation: &mut cad_core::weldment::WeldmentOperation,
) -> bool {
    use cad_core::weldment::{Corner, WeldmentOperation};
    let doc = w.session.visible();
    let mut changed = false;
    match operation {
        WeldmentOperation::Members {
            body,
            section,
            path,
            members,
            roll,
            anchor,
            start_offset,
            end_offset,
            corner,
        } => {
            changed |= inherited_stock(ui, doc, current, body);
            changed |= selector(ui, "Closed Section Sketch", section, profiles(doc, current));
            changed |= selector(ui, "Path Sketch", path, profiles(doc, current));
            let mut all = members.is_empty();
            if ui.checkbox(&mut all, "All Path Members").changed() {
                *members = if all {
                    vec![]
                } else {
                    path_members(w, path)
                        .into_iter()
                        .next()
                        .map(|(id, _)| vec![id])
                        .unwrap_or_default()
                };
                changed = true;
            }
            if !all {
                egui::CollapsingHeader::new("Path Members")
                    .default_open(true)
                    .show(ui, |ui| {
                        for (id, label) in path_members(w, path) {
                            let mut selected = members.contains(&id);
                            if ui.checkbox(&mut selected, label).changed() {
                                if selected {
                                    members.push(id);
                                } else {
                                    members.retain(|member| member != &id);
                                }
                                changed = true;
                            }
                        }
                    });
                ui.small("Removing the last selected member restores All Path Members.");
            }
            let points = source_sketch(w, section)
                .map(|sketch| sketch.points)
                .unwrap_or_default();
            egui::ComboBox::from_label("Section Locating Point")
                .selected_text(
                    anchor
                        .as_ref()
                        .and_then(|id| points.get(id))
                        .map(|point| format!("({:.2}, {:.2}) mm", point[0], point[1]))
                        .unwrap_or_else(|| {
                            if anchor.is_some() {
                                "Missing locating point".into()
                            } else {
                                "Sketch Origin".into()
                            }
                        }),
                )
                .show_ui(ui, |ui| {
                    changed |= ui.selectable_value(anchor, None, "Sketch Origin").changed();
                    for (id, point) in points {
                        changed |= ui
                            .selectable_value(
                                anchor,
                                Some(id),
                                format!("({:.2}, {:.2}) mm", point[0], point[1]),
                            )
                            .changed();
                    }
                });
            changed |= expression(ui, "Section Rotation", roll);
            changed |= expression(ui, "Start Shortening (+) / Extension (−)", start_offset);
            changed |= expression(ui, "Finish Shortening (+) / Extension (−)", end_offset);
            egui::ComboBox::from_label("Corner Treatment")
                .selected_text(format!("{corner:?}"))
                .show_ui(ui, |ui| {
                    changed |= ui
                        .selectable_value(corner, Corner::Miter, "Miter")
                        .changed();
                    changed |= ui.selectable_value(corner, Corner::Butt, "Butt").changed();
                });
            ui.small("Closed solid or hollow sections on planar straight sketch paths. Miter requires two incident segments; butt leaves independently cut member solids.");
        }
        WeldmentOperation::Trim {
            body,
            members,
            plane,
            offset,
            end,
        } => {
            changed |= selector(ui, "Weldment", body, weldment_choices(doc, current));
            if let Ok(stock) = cad_core::weldment::stock(doc, body) {
                let choices = member_choices(w, &stock);
                let mut all = members.is_empty();
                if ui.checkbox(&mut all, "All Members").changed() {
                    *members = if all {
                        vec![]
                    } else {
                        choices
                            .first()
                            .map(|(reference, _)| vec![reference.clone()])
                            .unwrap_or_default()
                    };
                    changed = true;
                }
                if !all {
                    for (reference, label) in choices {
                        let mut selected = members.contains(&reference);
                        if ui.checkbox(&mut selected, label).changed() {
                            if selected {
                                members.push(reference);
                            } else {
                                members.retain(|member| member != &reference);
                            }
                            changed = true;
                        }
                    }
                }
            }
            changed |= end_fields(ui, end);
            changed |= crate::reference_ui::plane(ui, w, doc, current, "End Trim Plane", plane);
            changed |= expression(ui, "Plane Offset", offset);
            ui.small("Projects the true section onto an associative planar face, datum or origin plane. Positive-length validation prevents collapsed or reversed members.");
        }
        WeldmentOperation::EndCap {
            body,
            member,
            end,
            thickness,
            offset,
        } => {
            changed |= selector(ui, "Weldment", body, weldment_choices(doc, current));
            if let Ok(stock) = cad_core::weldment::stock(doc, body) {
                let choices = member_choices(w, &stock);
                egui::ComboBox::from_label("Member")
                    .selected_text(
                        choices
                            .iter()
                            .find(|(reference, _)| reference == member)
                            .map(|(_, label)| label.as_str())
                            .unwrap_or("Missing member"),
                    )
                    .show_ui(ui, |ui| {
                        for (reference, label) in choices {
                            changed |= ui.selectable_value(member, reference, label).changed();
                        }
                    });
            }
            changed |= end_fields(ui, end);
            changed |= expression(ui, "Cap Thickness", thickness);
            changed |= expression(ui, "Outward Gap", offset);
            ui.small("Fills the outer section at a terminal or explicitly trimmed end. Caps remain separate manufacturing solids.");
        }
        WeldmentOperation::Gusset {
            body,
            profile,
            thickness,
            side,
        } => {
            changed |= inherited_stock(ui, doc, current, body);
            changed |= selector(ui, "Gusset Sketch", profile, profiles(doc, current));
            changed |= expression(ui, "Thickness", thickness);
            egui::ComboBox::from_label("Thickness Direction")
                .selected_text(format!("{side:?}"))
                .show_ui(ui, |ui| {
                    for (value, label) in [
                        (ThicknessSide::Positive, "Along sketch normal"),
                        (ThicknessSide::Negative, "Reverse"),
                        (ThicknessSide::MidPlane, "Mid-plane"),
                    ] {
                        changed |= ui.selectable_value(side, value, label).changed();
                    }
                });
        }
    }
    changed
}
