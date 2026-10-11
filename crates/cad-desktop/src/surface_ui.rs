//! Default docked presentation for semantic surface commands.
use crate::{Workspace, cad_ui, sketch, ui, viewport};
use bevy_egui::egui;
use cad_core::{
    Feature, NodeRef, Operation,
    commands::Command,
    reference_curves::Segment,
    reference_geometry::{AxisSource, DirectionSource, OriginAxis},
    surface::{FaceSource, PatchCorrespondence, SurfaceOperation},
};

pub fn is_preset(name: &str) -> bool {
    matches!(
        name,
        "Planar Surface"
            | "Ruled Surface"
            | "Mid-Surface"
            | "Extruded Surface"
            | "Revolved Surface"
            | "Lofted Surface"
            | "Swept Surface"
            | "Offset Surface"
            | "Thicken"
            | "Knit Surface"
            | "Reverse Surface"
            | "Filled Surface"
            | "Trim Surface"
            | "Parameter Trim Surface"
            | "Untrim Surface"
            | "Extend Surface"
    )
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
    let Some(part) = w
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
                && matches!(f.operation, Operation::Profile { .. })
        })
        .or_else(|| {
            part.features
                .iter()
                .rev()
                .find(|f| !f.suppressed && matches!(f.operation, Operation::Profile { .. }))
        })
        .map(|f| part.reference(&f.id));
    if name == "Ruled Surface" {
        let source = crate::reference_curves_ui::selected_edge(w, "").or_else(|| {
            crate::reference_curves_ui::sources(w.session.visible(), &part.id, "")
                .into_iter()
                .last()
                .map(|(_, source)| source)
        });
        let Some(source) = source else {
            sketch::begin(w, None);
            w.status = "Draw a curve or select a model edge, then create a Ruled Surface".into();
            return;
        };
        insert(
            w,
            part,
            name,
            SurfaceOperation::Ruled {
                sources: vec![Segment {
                    id: cad_core::id(),
                    source,
                    reverse: false,
                }],
                direction: Box::new(DirectionSource::Axis {
                    axis: Box::new(AxisSource::Origin {
                        axis: OriginAxis::Z,
                    }),
                }),
                distance: "10 mm".into(),
                reverse: false,
            },
        );
        return;
    }
    if name == "Mid-Surface" {
        let choices = face_choices(w, "");
        let first =
            selected_face(w, "").or_else(|| choices.first().map(|(_, source)| source.clone()));
        let Some(first) = first else {
            w.status = "Select the first face of a solid or surface body".into();
            return;
        };
        let second = choices
            .iter()
            .find(|(_, source)| source != &first)
            .map(|(_, source)| source.clone())
            .unwrap_or_else(|| first.clone());
        insert(
            w,
            part,
            name,
            SurfaceOperation::MidSurface {
                faces: Box::new([first, second]),
                fraction: "0.5".into(),
                correspondence: PatchCorrespondence::default(),
                reverse: false,
            },
        );
        return;
    }
    let surface = w
        .selected
        .as_ref()
        .and_then(|s| s.body.as_ref())
        .filter(|r| {
            r.definition == part.id
                && part
                    .features
                    .iter()
                    .any(|f| f.id == r.node && !f.suppressed && f.operation.is_surface())
        })
        .cloned()
        .or_else(|| {
            part.features
                .iter()
                .find(|f| w.view.active_node.as_ref() == Some(&f.id) && f.operation.is_surface())
                .or_else(|| {
                    part.features
                        .iter()
                        .rev()
                        .find(|f| !f.suppressed && f.operation.is_surface())
                })
                .map(|f| part.reference(&f.id))
        });
    let needs_surface = matches!(
        name,
        "Offset Surface"
            | "Thicken"
            | "Knit Surface"
            | "Reverse Surface"
            | "Trim Surface"
            | "Parameter Trim Surface"
            | "Untrim Surface"
            | "Extend Surface"
    );
    let reference = if needs_surface {
        surface
    } else {
        profile.clone()
    };
    let Some(reference) = reference else {
        if needs_surface {
            w.status = "Create or select a surface body first".into();
        } else {
            sketch::begin(w, None);
            w.status = "Draw a section sketch, finish it, then create the surface".into();
        }
        return;
    };
    let plane = viewport::display_evaluation(w)
        .and_then(|e| {
            e.sketches
                .get(&format!("{}::{}", reference.definition, reference.node))
        })
        .map(|s| s.plane.clone())
        .or_else(|| {
            part.features
                .iter()
                .find(|f| f.id == reference.node)
                .and_then(|f| {
                    if let Operation::Profile { plane, .. } = &f.operation {
                        Some(plane.clone())
                    } else {
                        None
                    }
                })
        })
        .unwrap_or_default();
    let normal = [
        plane.u[1] * plane.v[2] - plane.u[2] * plane.v[1],
        plane.u[2] * plane.v[0] - plane.u[0] * plane.v[2],
        plane.u[0] * plane.v[1] - plane.u[1] * plane.v[0],
    ];
    let operation = match name {
        "Planar Surface" => SurfaceOperation::Planar { profile: reference },
        "Extruded Surface" => SurfaceOperation::Extrude {
            profile: reference,
            distance: "10 mm".into(),
            direction: normal,
            mid_plane: false,
            profile_normal: true,
            reverse: false,
        },
        "Revolved Surface" => SurfaceOperation::Revolve {
            axis_reference: crate::reference_ui::profile_axis(w.session.visible(), &reference),
            profile: reference,
            origin: plane.origin,
            axis: plane.v,
            angle: "360 deg".into(),
        },
        "Lofted Surface" => SurfaceOperation::Loft {
            profiles: part
                .features
                .iter()
                .filter(|f| !f.suppressed && matches!(f.operation, Operation::Profile { .. }))
                .map(|f| part.reference(&f.id))
                .collect(),
        },
        "Swept Surface" => SurfaceOperation::Sweep {
            profile: reference,
            path: vec![
                plane.origin,
                std::array::from_fn(|i| plane.origin[i] + normal[i] * 50.),
            ],
            path_sketch: None,
        },
        "Filled Surface" => {
            let sections: Vec<_> = part
                .features
                .iter()
                .filter(|f| !f.suppressed && matches!(f.operation, Operation::Profile { .. }))
                .map(|f| part.reference(&f.id))
                .collect();
            if sections.len() < 4 {
                w.status = "Fill needs four ordered, end-connected open sketch curves".into();
                return;
            }
            SurfaceOperation::Fill {
                boundaries: std::array::from_fn(|i| sections[sections.len() - 4 + i].clone()),
                reverse: false,
            }
        }
        "Trim Surface" => {
            let Some(profile) = profile else {
                w.status = "Sketch a closed trim boundary on the surface first".into();
                return;
            };
            SurfaceOperation::Trim {
                surface: reference,
                profile,
                keep_inside: false,
            }
        }
        "Parameter Trim Surface" => SurfaceOperation::TrimDomain {
            surface: reference,
            u: ["0.1".into(), "0.9".into()],
            v: ["0.1".into(), "0.9".into()],
        },
        "Untrim Surface" => SurfaceOperation::Untrim { surface: reference },
        "Extend Surface" => SurfaceOperation::Extend {
            surface: reference.clone(),
            distance: "5 mm".into(),
            edges: w
                .selected
                .as_ref()
                .filter(|s| s.reference.topology == "edge" && s.body.as_ref() == Some(&reference))
                .map(|s| vec![s.reference.clone()])
                .unwrap_or_default(),
        },
        "Offset Surface" => SurfaceOperation::Offset {
            surface: reference,
            distance: "2 mm".into(),
        },
        "Thicken" => SurfaceOperation::Thicken {
            surface: reference,
            thickness: "2 mm".into(),
            mid_plane: false,
            reverse: false,
        },
        "Reverse Surface" => SurfaceOperation::Reverse { surface: reference },
        "Knit Surface" => SurfaceOperation::Knit {
            surfaces: part
                .outputs
                .iter()
                .filter_map(|id| {
                    part.features
                        .iter()
                        .find(|f| &f.id == id && !f.suppressed && f.operation.is_surface())
                })
                .map(|f| part.reference(&f.id))
                .collect(),
            tolerance: "0.0000001 mm".into(),
            make_solid: false,
            reverse: false,
        },
        _ => return,
    };
    insert(w, part, name, operation);
}
fn insert(w: &mut Workspace, mut part: cad_core::Part, name: &str, operation: SurfaceOperation) {
    let count = part
        .features
        .iter()
        .filter(|f| f.name.starts_with(name))
        .count()
        + 1;
    let feature = Feature {
        id: cad_core::id(),
        name: format!("{name}{count}"),
        operation: Operation::Surface {
            operation: operation.clone(),
        },
        suppressed: false,
    };
    for source in operation
        .consumed_surfaces()
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
fn selected_face(w: &Workspace, current: &str) -> Option<FaceSource> {
    let selected = w.selected.as_ref()?;
    if selected.reference.topology != "face"
        || selected.definition != *w.view.active_part.as_ref()?
    {
        return None;
    }
    let body = crate::reference_ui::selected_body(w, selected);
    if body.node == current {
        return None;
    }
    Some(FaceSource {
        body,
        reference: selected.reference.clone(),
    })
}
fn face_choices(w: &Workspace, current: &str) -> Vec<(String, FaceSource)> {
    let doc = w.session.visible();
    let mut result = Vec::new();
    let Some(part) = w.view.active_part.as_ref().and_then(|id| doc.parts.get(id)) else {
        return result;
    };
    let Some(evaluation) = viewport::display_evaluation(w) else {
        return result;
    };
    let mut meshes = evaluation.meshes.values().collect::<Vec<_>>();
    if let Some(history) = evaluation.history.get(&part.id) {
        meshes.extend(history.meshes.values().map(std::sync::Arc::as_ref));
    }
    for feature in part
        .features
        .iter()
        .take_while(|f| f.id != current)
        .filter(|f| !f.suppressed && (f.operation.produces_solid() || f.operation.is_surface()))
    {
        let body = part.reference(&feature.id);
        let mut ordinal = 0;
        for face in meshes
            .iter()
            .flat_map(|mesh| &mesh.faces)
            .filter(|face| face.body.as_ref() == Some(&body))
        {
            let source = FaceSource {
                body: body.clone(),
                reference: face.reference.clone(),
            };
            if result.iter().any(|(_, existing)| existing == &source) {
                continue;
            }
            ordinal += 1;
            result.push((format!("{} · face {}", feature.name, ordinal), source));
        }
    }
    result
}
fn face_selector(
    ui: &mut egui::Ui,
    w: &Workspace,
    current: &str,
    label: &str,
    source: &mut FaceSource,
) -> bool {
    ui.push_id(label, |ui| {
        ui.strong(label);
        let choices = face_choices(w, current);
        let mut changed = false;
        let caption = choices
            .iter()
            .find(|(_, choice)| choice == source)
            .map(|(label, _)| label.as_str())
            .unwrap_or("Saved face reference");
        egui::ComboBox::from_id_salt("source-face")
            .selected_text(caption)
            .show_ui(ui, |ui| {
                for (label, choice) in choices {
                    changed |= ui.selectable_value(source, choice, label).changed();
                }
            });
        if ui
            .add_enabled(
                selected_face(w, current).is_some(),
                egui::Button::new("Use selected face"),
            )
            .clicked()
            && let Some(selected) = selected_face(w, current)
        {
            *source = selected;
            changed = true;
        }
        changed
    })
    .inner
}
fn coordinates(ui: &mut egui::Ui, label: &str, values: &mut [f64; 3], units: bool) -> bool {
    let mut changed = false;
    ui.label(label);
    ui.horizontal(|ui| {
        for (label, value) in ["X", "Y", "Z"].into_iter().zip(values) {
            changed |= ui
                .add(
                    egui::DragValue::new(value)
                        .speed(0.1)
                        .prefix(format!("{label} "))
                        .suffix(if units { " mm" } else { "" }),
                )
                .changed();
        }
    });
    changed
}
pub(crate) fn surface_selector(
    ui: &mut egui::Ui,
    doc: &cad_core::Document,
    current: &str,
    reference: &mut NodeRef,
) -> bool {
    let name = doc
        .parts
        .get(&reference.definition)
        .and_then(|p| p.features.iter().find(|f| f.id == reference.node))
        .map(|f| f.name.as_str())
        .unwrap_or("Select surface");
    let mut changed = false;
    egui::ComboBox::from_id_salt("surface-source")
        .selected_text(name)
        .show_ui(ui, |ui| {
            for part in doc.parts.values() {
                for feature in part
                    .features
                    .iter()
                    .take_while(|f| f.id != current)
                    .filter(|f| !f.suppressed && f.operation.is_surface())
                {
                    changed |= ui
                        .selectable_value(
                            reference,
                            part.reference(&feature.id),
                            format!("{} / {}", part.name, feature.name),
                        )
                        .changed();
                }
            }
        });
    changed
}
pub fn fields(
    ui: &mut egui::Ui,
    w: &Workspace,
    current: &str,
    operation: &mut SurfaceOperation,
) -> bool {
    let doc = w.session.visible();
    let mut changed = false;
    match operation {
        SurfaceOperation::Ruled {
            sources,
            direction,
            distance,
            reverse,
        } => {
            let definition = w.view.active_part.as_deref().unwrap_or("");
            let available = crate::reference_curves_ui::sources(doc, definition, current);
            ui.strong("Curve sources");
            let mut remove = None;
            for (index, source) in sources.iter_mut().enumerate() {
                ui.push_id(&source.id, |ui| {
                    egui::ComboBox::from_id_salt("ruled-source")
                        .selected_text(crate::reference_curves_ui::source_label(
                            doc,
                            &source.source,
                        ))
                        .show_ui(ui, |ui| {
                            for (label, choice) in &available {
                                changed |= ui
                                    .selectable_value(&mut source.source, choice.clone(), label)
                                    .changed();
                            }
                        });
                    ui.horizontal(|ui| {
                        if ui
                            .add_enabled(
                                crate::reference_curves_ui::selected_edge(w, current).is_some(),
                                egui::Button::new("Use selected edge"),
                            )
                            .clicked()
                            && let Some(selected) =
                                crate::reference_curves_ui::selected_edge(w, current)
                        {
                            source.source = selected;
                            changed = true;
                        }
                        changed |= ui.checkbox(&mut source.reverse, "Reverse curve").changed();
                        if ui.small_button("Remove").clicked() {
                            remove = Some(index);
                        }
                    });
                });
            }
            if let Some(index) = remove {
                sources.remove(index);
                changed = true;
            }
            if ui
                .add_enabled(sources.len() < 256, egui::Button::new("Add curve"))
                .clicked()
                && let Some(source) = crate::reference_curves_ui::selected_edge(w, current)
                    .or_else(|| available.first().map(|(_, source)| source.clone()))
            {
                sources.push(Segment {
                    id: cad_core::id(),
                    source,
                    reverse: false,
                });
                changed = true;
            }
            changed |= crate::reference_ui::direction(
                ui,
                w,
                doc,
                current,
                "Extension direction",
                direction,
            );
            changed |= cad_ui::expression(ui, "Signed extension", distance);
            changed |= ui.checkbox(reverse, "Reverse face orientation").changed();
            ui.small("Exact translation of native rational curves. Each proven curve span creates a surface strip; use Knit to join adjacent strips. Analytic helix and implicit intersection extrusion are not supported.");
        }
        SurfaceOperation::MidSurface {
            faces,
            fraction,
            correspondence,
            reverse,
        } => {
            changed |= face_selector(ui, w, current, "First face", &mut faces[0]);
            changed |= face_selector(ui, w, current, "Second face", &mut faces[1]);
            changed |= cad_ui::expression(ui, "Position between faces (0–1)", fraction);
            ui.horizontal(|ui| {
                for (label, value) in [("First", 0.), ("Midpoint", 0.5), ("Second", 1.)] {
                    if ui.small_button(label).clicked() {
                        *fraction = cad_core::Expr(value.to_string());
                        changed = true;
                    }
                }
            });
            ui.collapsing("Rational patch correspondence", |ui| {
                changed |= ui
                    .checkbox(&mut correspondence.swap_uv, "Swap second-face U and V")
                    .changed();
                changed |= ui
                    .checkbox(&mut correspondence.reverse_u, "Reverse second-face U")
                    .changed();
                changed |= ui
                    .checkbox(&mut correspondence.reverse_v, "Reverse second-face V")
                    .changed();
            });
            changed |= ui.checkbox(reverse, "Reverse face orientation").changed();
            ui.small("Parallel planar faces preserve matching translated trims and holes. Compatible full rational patches blend corresponding U/V positions; this is a normal-distance midpoint only for parallel planar faces.");
        }
        SurfaceOperation::Planar { profile } => {
            changed |= cad_ui::profile_selector(ui, doc, current, "Closed sketch", profile);
        }
        SurfaceOperation::Extrude {
            profile,
            distance,
            direction,
            mid_plane,
            profile_normal,
            reverse,
        } => {
            changed |= cad_ui::profile_selector(ui, doc, current, "Section sketch", profile);
            changed |= cad_ui::expression(ui, "Depth", distance);
            changed |= ui.checkbox(mid_plane, "Mid-plane").changed();
            changed |= ui
                .checkbox(profile_normal, "Normal to sketch plane")
                .changed();
            changed |= ui.checkbox(reverse, "Reverse direction").changed();
            if !*profile_normal {
                changed |= coordinates(ui, "Direction", direction, false);
            }
        }
        SurfaceOperation::Revolve {
            profile,
            origin,
            axis,
            angle,
            axis_reference,
        } => {
            changed |= cad_ui::profile_selector(ui, doc, current, "Section sketch", profile);
            changed |= cad_ui::expression(ui, "Angle", angle);
            changed |= crate::reference_ui::optional_axis(
                ui,
                w,
                doc,
                current,
                "Revolve axis",
                axis_reference,
            );
            if axis_reference.is_none() {
                changed |= coordinates(ui, "Axis origin", origin, true);
                changed |= coordinates(ui, "Axis direction", axis, false);
            }
        }
        SurfaceOperation::Loft { profiles } => {
            ui.label("Ordered sections");
            let mut remove = None;
            let mut up = None;
            for (i, reference) in profiles.iter_mut().enumerate() {
                ui.push_id(i, |ui| {
                    changed |= cad_ui::profile_selector(
                        ui,
                        doc,
                        current,
                        &format!("Section {}", i + 1),
                        reference,
                    );
                    ui.horizontal(|ui| {
                        if ui.small_button("Remove").clicked() {
                            remove = Some(i);
                        }
                        if i > 0 && ui.small_button("Move Up").clicked() {
                            up = Some(i);
                        }
                    });
                });
            }
            if let Some(i) = remove {
                profiles.remove(i);
                changed = true;
            } else if let Some(i) = up {
                profiles.swap(i, i - 1);
                changed = true;
            }
            if ui.button("Add section").clicked()
                && let Some(reference) = doc
                    .parts
                    .values()
                    .flat_map(|p| {
                        p.features
                            .iter()
                            .take_while(|f| f.id != current)
                            .filter(|f| {
                                !f.suppressed && matches!(f.operation, Operation::Profile { .. })
                            })
                            .map(|f| p.reference(&f.id))
                    })
                    .find(|r| !profiles.contains(r))
            {
                profiles.push(reference);
                changed = true;
            }
            ui.small("Ruled surface between compatible open or closed contours.");
        }
        SurfaceOperation::Sweep {
            profile,
            path,
            path_sketch,
        } => {
            changed |= cad_ui::profile_selector(ui, doc, current, "Section sketch", profile);
            let mut use_sketch = path_sketch.is_some();
            if ui.checkbox(&mut use_sketch, "Use path sketch").changed() {
                *path_sketch = use_sketch.then(|| profile.clone());
                changed = true;
            }
            if let Some(reference) = path_sketch {
                changed |=
                    cad_ui::profile_selector(ui, doc, current, "Open path sketch", reference);
            } else {
                let mut remove = None;
                for (i, p) in path.iter_mut().enumerate() {
                    ui.push_id(i, |ui| {
                        changed |= coordinates(ui, &format!("Station {}", i + 1), p, true);
                        if path_sketch.is_none() && ui.small_button("Remove station").clicked() {
                            remove = Some(i);
                        }
                    });
                }
                if let Some(i) = remove {
                    path.remove(i);
                    changed = true;
                }
                if ui.button("Add station").clicked() {
                    let mut next = path.last().copied().unwrap_or([0.; 3]);
                    next[2] += 10.;
                    path.push(next);
                    changed = true;
                }
            }
            ui.small("Fixed section orientation along a polyline path.");
        }
        SurfaceOperation::Fill {
            boundaries,
            reverse,
        } => {
            for (i, reference) in boundaries.iter_mut().enumerate() {
                ui.push_id(i, |ui| {
                    changed |= cad_ui::profile_selector(
                        ui,
                        doc,
                        current,
                        &format!("Boundary {}", i + 1),
                        reference,
                    );
                });
            }
            changed |= ui.checkbox(reverse, "Reverse face orientation").changed();
            ui.small("Four ordered, end-connected open polynomial curves. Exact Coons fill retains the boundaries.");
        }
        SurfaceOperation::Trim {
            surface,
            profile,
            keep_inside,
        } => {
            changed |= surface_selector(ui, doc, current, surface);
            changed |= cad_ui::profile_selector(ui, doc, current, "Closed trim sketch", profile);
            changed |= ui.checkbox(keep_inside, "Keep inside sketch").changed();
            ui.small("The trim sketch must lie on the planar source face. Remove-inside supports holes and multiple regions.");
        }
        SurfaceOperation::TrimDomain { surface, u, v } => {
            changed |= surface_selector(ui, doc, current, surface);
            changed |= cad_ui::expression(ui, "U start", &mut u[0]);
            changed |= cad_ui::expression(ui, "U end", &mut u[1]);
            changed |= cad_ui::expression(ui, "V start", &mut v[0]);
            changed |= cad_ui::expression(ui, "V end", &mut v[1]);
            ui.small(
                "Exact rectangular rational patch trim. Normalized U/V parameters run from 0 to 1.",
            );
        }
        SurfaceOperation::Untrim { surface } => {
            changed |= surface_selector(ui, doc, current, surface);
            ui.small("Restore a finite rational carrier's natural boundary and remove its trim loops. Infinite planes and collapsed periodic patch sides require other editing modes.");
        }
        SurfaceOperation::Extend {
            surface,
            distance,
            edges,
        } => {
            let previous = surface.clone();
            changed |= surface_selector(ui, doc, current, surface);
            if *surface != previous {
                edges.clear();
            }
            changed |= cad_ui::expression(ui, "Extension distance", distance);
            changed |= crate::mold_ui::edge_fields(ui, w, surface, edges);
            ui.small("No selected edges extends every boundary. Supports an independent convex planar face with straight edges.");
        }
        SurfaceOperation::Offset { surface, distance } => {
            changed |= surface_selector(ui, doc, current, surface);
            changed |= cad_ui::expression(ui, "Offset distance", distance);
            ui.small("Signed normal offset of planar surface bodies.");
        }
        SurfaceOperation::Reverse { surface } => {
            changed |= surface_selector(ui, doc, current, surface);
        }
        SurfaceOperation::Knit {
            surfaces,
            tolerance,
            make_solid,
            reverse,
        } => {
            let mut remove = None;
            for (i, reference) in surfaces.iter_mut().enumerate() {
                ui.push_id(i, |ui| {
                    changed |= surface_selector(ui, doc, current, reference);
                    if ui.small_button("Remove input").clicked() {
                        remove = Some(i);
                    }
                });
            }
            if let Some(i) = remove {
                surfaces.remove(i);
                changed = true;
            }
            if ui.button("Add surface input").clicked()
                && let Some(reference) = doc
                    .parts
                    .values()
                    .flat_map(|p| {
                        p.features
                            .iter()
                            .take_while(|f| f.id != current)
                            .filter(|f| !f.suppressed && f.operation.is_surface())
                            .map(|f| p.reference(&f.id))
                    })
                    .find(|r| !surfaces.contains(r))
            {
                surfaces.push(reference);
                changed = true;
            }
            changed |= cad_ui::expression(ui, "Knit tolerance", tolerance);
            changed |= ui.checkbox(make_solid, "Try to form solid").changed();
            changed |= ui.checkbox(reverse, "Reverse result orientation").changed();
            ui.small("Matching boundaries are sewn. Form Solid requires one closed shell.");
        }
        SurfaceOperation::Thicken {
            surface,
            thickness,
            mid_plane,
            reverse,
        } => {
            changed |= surface_selector(ui, doc, current, surface);
            changed |= cad_ui::expression(ui, "Thickness", thickness);
            changed |= ui.checkbox(mid_plane, "Both sides equally").changed();
            changed |= ui.checkbox(reverse, "Reverse direction").changed();
            ui.small("Planar surface bodies become validated solid bodies.");
        }
    }
    changed
}
