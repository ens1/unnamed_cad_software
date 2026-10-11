//! Docked mold controls use the shared live feature draft and validated Apply.
use crate::{Workspace, cad_ui, ui};
use bevy_egui::egui;
use cad_core::{
    Feature, NodeRef, Operation, TopologyRef,
    commands::Command,
    mold::{MoldOperation, ToolingSide},
    reference_geometry::{AxisSource, OriginAxis, OriginPlane, PlaneSource, PointSource},
};

pub fn is_preset(name: &str) -> bool {
    matches!(
        name,
        "Draft Analysis" | "Parting Surface" | "Shut-off Surface" | "Mold Shrink" | "Tooling Split"
    )
}
fn current_evaluation(w: &Workspace) -> Option<&cad_core::protocol::Evaluation> {
    w.session
        .draft
        .as_ref()
        .map_or(w.session.evaluation.as_ref(), |d| d.evaluation.as_ref())
        .filter(|e| e.valid && e.identity == w.session.identity())
}
fn report_for_feature<'a>(
    w: &'a Workspace,
    definition: &str,
    feature: &str,
) -> Option<&'a cad_core::mold::DraftReport> {
    let evaluation = current_evaluation(w)?;
    let key = cad_core::configuration::key(
        definition,
        w.session
            .visible()
            .active_configurations
            .get(definition)
            .map(String::as_str),
    );
    evaluation
        .mold_analyses
        .get(&format!("{key}::{feature}"))
        .or_else(|| {
            evaluation
                .mold_analyses
                .get(&format!("{definition}::{feature}"))
        })
}
fn source_assessment<'a>(
    w: &'a Workspace,
    body: &NodeRef,
) -> Option<(&'a cad_core::mold::DraftReport, &'a AxisSource)> {
    // A configured component can share source IDs with another configuration.
    // Select the source definition's current configuration and feature order,
    // rather than taking an arbitrary report with the same unqualified IDs.
    let part = w.session.visible().parts.get(&body.definition)?;
    part.features.iter().rev().find_map(|f| {
        let Operation::Mold {
            operation:
                MoldOperation::DraftAnalysis {
                    body: source, pull, ..
                },
        } = &f.operation
        else {
            return None;
        };
        if f.suppressed || source != body {
            return None;
        }
        report_for_feature(w, &part.id, &f.id)
            .filter(|r| &r.source == body)
            .map(|r| (r, pull))
    })
}
fn perpendicular(pull: [f64; 3]) -> [f64; 3] {
    if pull[0].abs() < 0.8 {
        [0., pull[2], -pull[1]]
    } else {
        [-pull[2], 0., pull[0]]
    }
}
pub fn preset(w: &mut Workspace, name: &str) {
    crate::history::latest(w);
    if w.confirm_requested {
        w.after_confirm = Some(name.into());
        return;
    }
    if w.sketch.is_some() {
        w.after_confirm = Some(name.into());
        w.confirm_tool();
        if w.sketch.is_some() {
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
        .and_then(|s| s.body.as_ref())
        .filter(|r| {
            r.definition == part.id
                && part
                    .features
                    .iter()
                    .any(|f| f.id == r.node && !f.suppressed && f.operation.produces_solid())
        })
        .cloned()
        .or_else(|| {
            part.outputs
                .iter()
                .rev()
                .filter_map(|id| {
                    part.features
                        .iter()
                        .find(|f| &f.id == id && !f.suppressed && f.operation.produces_solid())
                })
                .next()
                .map(|f| part.reference(&f.id))
        });
    let Some(body) = body else {
        w.status = "Create or select a solid part first".into();
        return;
    };
    let edges = w
        .selected
        .as_ref()
        .filter(|s| s.reference.topology == "edge" && s.body.as_ref() == Some(&body))
        .map(|s| vec![s.reference.clone()])
        .unwrap_or_default();
    let pull = AxisSource::Origin {
        axis: OriginAxis::Z,
    };
    let operation = match name {
        "Draft Analysis" => MoldOperation::DraftAnalysis {
            body,
            pull,
            minimum: "1 deg".into(),
            reverse: false,
        },
        "Parting Surface" => {
            let analysis = source_assessment(w, &body);
            let edges = if edges.is_empty() {
                analysis
                    .map(|(r, _)| r.parting_edges.clone())
                    .unwrap_or_default()
            } else {
                edges
            };
            let direction = analysis
                .map(|(r, _)| perpendicular(r.pull))
                .unwrap_or([1., 0., 0.]);
            let pull = analysis.map(|(_, p)| p.clone()).unwrap_or(pull);
            MoldOperation::PartingSurface {
                body,
                edges,
                pull,
                direction,
                width: "20 mm".into(),
                reverse: false,
            }
        }
        "Shut-off Surface" => MoldOperation::ShutoffSurface {
            body,
            edges,
            reverse: false,
        },
        "Mold Shrink" => MoldOperation::Shrink {
            body,
            factors: std::array::from_fn(|_| "1.02".into()),
            center: PointSource::default(),
        },
        "Tooling Split" => {
            let Some(stock) = part
                .features
                .iter()
                .rev()
                .find(|f| !f.suppressed && matches!(f.operation, Operation::Profile { .. }))
                .map(|f| part.reference(&f.id))
            else {
                w.status = "Sketch the mold stock boundary first, then choose Tooling Split".into();
                return;
            };
            MoldOperation::ToolingSplit {
                body,
                stock,
                depth: "100 mm".into(),
                parting: PlaneSource::Origin {
                    plane: OriginPlane::Top,
                },
                side: ToolingSide::Both,
            }
        }
        _ => return,
    };
    let count = part
        .features
        .iter()
        .filter(|f| f.name.starts_with(name))
        .count()
        + 1;
    let feature = Feature {
        id: cad_core::id(),
        name: format!("{name}{count}"),
        operation: Operation::Mold {
            operation: operation.clone(),
        },
        suppressed: false,
    };
    for source in operation
        .consumed_outputs()
        .iter()
        .filter(|r| r.definition == part.id)
    {
        part.outputs.retain(|id| id != &source.node);
    }
    if !operation.is_analysis() {
        part.outputs.push(feature.id.clone());
    }
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
fn body_selector(ui: &mut egui::Ui, w: &Workspace, current: &str, body: &mut NodeRef) -> bool {
    let doc = w.session.visible();
    let name = doc
        .parts
        .get(&body.definition)
        .and_then(|p| p.features.iter().find(|f| f.id == body.node))
        .map(|f| f.name.as_str())
        .unwrap_or("Select source body");
    let mut changed = false;
    egui::ComboBox::from_id_salt("mold-source-body")
        .selected_text(name)
        .show_ui(ui, |ui| {
            for part in doc.parts.values() {
                for feature in part
                    .features
                    .iter()
                    .take_while(|f| f.id != current)
                    .filter(|f| !f.suppressed && f.operation.produces_solid())
                {
                    changed |= ui
                        .selectable_value(
                            body,
                            part.reference(&feature.id),
                            format!("{} / {}", part.name, feature.name),
                        )
                        .changed();
                }
            }
        });
    changed
}
pub(crate) fn edge_fields(
    ui: &mut egui::Ui,
    w: &Workspace,
    body: &NodeRef,
    edges: &mut Vec<TopologyRef>,
) -> bool {
    let mut changed = false;
    let mut remove = None;
    for (i, _) in edges.iter().enumerate() {
        ui.horizontal(|ui| {
            ui.label(format!("Edge {}", i + 1));
            if ui.small_button("Remove").clicked() {
                remove = Some(i);
            }
        });
    }
    if let Some(i) = remove {
        edges.remove(i);
        changed = true;
    }
    let selected = w
        .selected
        .as_ref()
        .filter(|s| s.reference.topology == "edge" && s.body.as_ref() == Some(body));
    if ui
        .add_enabled(selected.is_some(), egui::Button::new("Add selected edge"))
        .clicked()
        && let Some(selected) = selected
        && !edges
            .iter()
            .any(|e| e.producer == selected.reference.producer && e.role == selected.reference.role)
    {
        edges.push(selected.reference.clone());
        changed = true;
    }
    if !edges.is_empty() && ui.small_button("Clear edges").clicked() {
        edges.clear();
        changed = true;
    }
    changed
}
pub fn fields(
    ui: &mut egui::Ui,
    w: &Workspace,
    current: &str,
    operation: &mut MoldOperation,
) -> bool {
    let doc = w.session.visible();
    let mut changed = false;
    match operation {
        MoldOperation::DraftAnalysis {
            body,
            pull,
            minimum,
            reverse,
        } => {
            changed |= body_selector(ui, w, current, body);
            changed |= crate::reference_ui::axis(ui, w, doc, current, "Pull direction", pull);
            changed |= cad_ui::expression(ui, "Minimum draft", minimum);
            changed |= ui.checkbox(reverse, "Reverse pull direction").changed();
            let report = w
                .view
                .active_part
                .as_deref()
                .and_then(|definition| report_for_feature(w, definition, current));
            if let Some(report) = report {
                ui.label(if report.complete {
                    "Every face has an exact draft classification"
                } else {
                    "Partial assessment: curved faces remain unassessed"
                });
                for face in &report.faces {
                    ui.horizontal(|ui| {
                        ui.label(format!("{:?}", face.class));
                        if let Some(angle) = face.angle_radians {
                            ui.label(format!("{:.3}°", angle.to_degrees()));
                        } else {
                            ui.label("Curved face: unresolved exact extrema");
                        }
                    });
                }
                ui.small(format!(
                    "{} exact planar positive/negative parting edges",
                    report.parting_edges.len()
                ));
            } else {
                ui.small("Waiting for the current source and pull-direction evaluation.");
            }
            ui.small("Draft angles assess local normals. Global undercut visibility and release checks require further geometry analysis.");
        }
        MoldOperation::PartingSurface {
            body,
            edges,
            pull,
            direction,
            width,
            reverse,
        } => {
            let before = body.clone();
            changed |= body_selector(ui, w, current, body);
            if before != *body {
                edges.clear();
            }
            changed |= crate::reference_ui::axis(ui, w, doc, current, "Pull direction", pull);
            changed |= edge_fields(ui, w, body, edges);
            if ui.button("Use analyzed parting edges").clicked()
                && let Some((report, source_pull)) = source_assessment(w, body)
            {
                edges.clone_from(&report.parting_edges);
                pull.clone_from(source_pull);
                *direction = perpendicular(report.pull);
                changed = true;
            }
            ui.label("Extension direction (perpendicular to pull)");
            ui.horizontal(|ui| {
                for (label, value) in ["X", "Y", "Z"].into_iter().zip(direction) {
                    changed |= ui
                        .add(
                            egui::DragValue::new(value)
                                .speed(0.05)
                                .prefix(format!("{label} ")),
                        )
                        .changed();
                }
            });
            changed |= cad_ui::expression(ui, "Width", width);
            changed |= ui.checkbox(reverse, "Reverse face orientation").changed();
            ui.small("Exact ruled strips extend the selected parting edges.");
        }
        MoldOperation::ShutoffSurface {
            body,
            edges,
            reverse,
        } => {
            let before = body.clone();
            changed |= body_selector(ui, w, current, body);
            if before != *body {
                edges.clear();
            }
            changed |= edge_fields(ui, w, body, edges);
            changed |= ui.checkbox(reverse, "Reverse face orientation").changed();
            ui.small("Select every edge of each closed planar opening. Exact planar caps retain the original curves.");
        }
        MoldOperation::Shrink {
            body,
            factors,
            center,
        } => {
            changed |= body_selector(ui, w, current, body);
            for (label, factor) in ["X factor", "Y factor", "Z factor"]
                .into_iter()
                .zip(factors)
            {
                changed |= cad_ui::expression(ui, label, factor);
            }
            changed |= crate::reference_ui::point(ui, w, doc, current, "Shrink center", center);
            ui.small("A factor of 1.02 applies 2% linear shrink allowance. Exact affine scaling follows document X/Y/Z axes.");
        }
        MoldOperation::ToolingSplit {
            body,
            stock,
            depth,
            parting,
            side,
        } => {
            changed |= body_selector(ui, w, current, body);
            changed |=
                cad_ui::profile_selector(ui, doc, current, "Closed mold stock sketch", stock);
            changed |= cad_ui::expression(ui, "Stock depth", depth);
            changed |= crate::reference_ui::plane(ui, w, doc, current, "Parting plane", parting);
            egui::ComboBox::from_label("Tool bodies")
                .selected_text(format!("{side:?}"))
                .show_ui(ui, |ui| {
                    for (value, label) in [
                        (ToolingSide::Both, "Core and cavity"),
                        (ToolingSide::Core, "Core"),
                        (ToolingSide::Cavity, "Cavity"),
                    ] {
                        changed |= ui.selectable_value(side, value, label).changed();
                    }
                });
            ui.small("Stock extrudes equally about its sketch. A parallel parting plane splits it; exact subtraction forms the core/cavity from the selected part or shrink feature.");
        }
    }
    changed
}
