//! Docked associative assembly component patterns with stable occurrence IDs.
use crate::Workspace;
use bevy_egui::egui;
use cad_core::{
    Document, NodeRef, Operation,
    assembly_geometry::{Reference, Source},
    body_editing::{PatternDirection, PatternOccurrence},
    commands::Command,
    component_pattern::{self, AxisReference, Definition, LinearDirection, Mode},
    reference_geometry::{AxisSource, OriginAxis},
};
fn parsed(w: &Workspace) -> cad_core::Result<Definition> {
    let p: Definition = serde_json::from_str(&w.json)?;
    if w.fields.get("component-pattern-editor") != Some(&p.id) {
        return Err(cad_core::CadError::Invalid(
            "Component pattern identity cannot be changed".into(),
        ));
    }
    Ok(p)
}
pub fn current_source_is_valid(w: &Workspace) -> bool {
    w.fields
        .get("assembly-id")
        .and_then(|id| w.session.visible().assemblies.get(id))
        .is_some_and(|a| parsed(w).is_ok_and(|p| a.component_patterns.get(&p.id) == Some(&p)))
}
fn guard(w: &mut Workspace) {
    if current_source_is_valid(w) {
        w.fields.remove("component-pattern-source-invalid");
    } else {
        w.fields
            .insert("component-pattern-source-invalid".into(), "true".into());
    }
}
fn submit(w: &mut Workspace, assembly: &str, p: Definition) {
    let mut doc = w.session.visible().clone();
    let id = p.id.clone();
    match (Command::PutComponentPattern {
        assembly: assembly.into(),
        pattern: p.into(),
    })
    .execute(&mut doc)
    {
        Ok(()) => {
            let accepted = doc.assemblies[assembly].component_patterns[&id].clone();
            w.dispatch(Command::ReplaceDocument { document: doc });
            if w.session
                .visible()
                .assemblies
                .get(assembly)
                .and_then(|a| a.component_patterns.get(&id))
                == Some(&accepted)
            {
                w.json = serde_json::to_string_pretty(&accepted).unwrap_or_default();
                w.fields
                    .retain(|key, _| !key.starts_with(&format!("component-pattern-input:{id}:")));
            }
        }
        Err(error) => w.status = error.to_string(),
    }
    guard(w);
}
pub fn begin(w: &mut Workspace, assembly: &str, p: Definition) {
    if w.fields.contains_key("exploded-view-editor")
        || w.fields.contains_key("assembly-inspection-editor")
        || w.fields.contains_key("assembly-weld-editor")
        || w.sketch.is_some()
        || w.fields.contains_key("mate-editor")
        || w.fields.contains_key("configuration-editor")
        || w.dialog.as_deref() == Some("feature")
    {
        w.status =
            "Finish or cancel the current operation before editing a component pattern".into();
        return;
    }
    w.start_tool();
    w.fields.insert("assembly-id".into(), assembly.into());
    w.fields
        .insert("component-pattern-editor".into(), p.id.clone());
    w.json = serde_json::to_string_pretty(&p).unwrap_or_default();
    w.open_editor("assembly");
    guard(w);
}
pub fn new(w: &mut Workspace, assembly: &str) {
    let Some(a) = w.session.visible().assemblies.get(assembly) else {
        return;
    };
    let generated = component_pattern::generated(a);
    let selected = w
        .selected_component
        .as_ref()
        .and_then(|path| path.first())
        .filter(|id| a.instances.contains_key(*id) && !generated.contains(*id))
        .cloned()
        .or_else(|| {
            w.view
                .active_instance
                .as_ref()
                .filter(|id| a.instances.contains_key(*id) && !generated.contains(*id))
                .cloned()
                .or_else(|| {
                    a.instances
                        .keys()
                        .find(|id| !generated.contains(*id))
                        .cloned()
                })
        });
    let Some(seed) = selected else {
        w.status = "Insert a component before creating a component pattern".into();
        return;
    };
    let name = (1..)
        .map(|n| format!("Component Pattern{n}"))
        .find(|name| {
            !a.component_patterns
                .values()
                .any(|p| p.name.eq_ignore_ascii_case(name))
        })
        .unwrap();
    let mut pattern = Definition::new(name);
    pattern.seeds.push(seed);
    begin(w, assembly, pattern.clone());
    if w.fields.get("component-pattern-editor") == Some(&pattern.id) {
        submit(w, assembly, pattern);
    }
}
pub fn show(ui: &mut egui::Ui, w: &mut Workspace, assembly: &str) {
    if !w.fields.contains_key("component-pattern-editor") {
        return;
    }
    let doc = w.session.visible().clone();
    let Some(a) = doc.assemblies.get(assembly) else {
        return;
    };
    ui.weak("Counts include the seed. Directions follow geometry and solved placements.");
    if let Ok(mut p) = parsed(w) {
        let mut changed = false;
        crate::presentation_controls::label(
            ui,
            &format!("component-pattern:{}/scope", p.id),
            "pattern-owner-context",
            &["task-context"],
            &scope_label(&doc, a, &p),
            "ready",
        );
        ui.label("Name");
        changed |= ui.text_edit_singleline(&mut p.name).changed();
        let mut kind = if matches!(p.mode, Mode::Linear { .. }) {
            "Linear"
        } else {
            "Circular"
        };
        let before = kind;
        egui::ComboBox::from_label("Pattern type")
            .selected_text(kind)
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut kind, "Linear", "Linear");
                ui.selectable_value(&mut kind, "Circular", "Circular");
            });
        if before != kind {
            p.mode = if kind == "Linear" {
                Mode::Linear {
                    step: ["30 mm".into(), "0 mm".into(), "0 mm".into()],
                    direction: Some(LinearDirection::default()),
                    second_direction: None,
                }
            } else {
                Mode::Circular {
                    origin: ["0 mm".into(), "0 mm".into(), "0 mm".into()],
                    axis: [0., 0., 1.],
                    angle: "30 deg".into(),
                    fixed_orientation: false,
                    axis_reference: None,
                }
            };
            changed = true;
        }
        ui.label("Count / scalar expression");
        changed |= ui.text_edit_singleline(&mut p.count.0).changed();
        match &mut p.mode {
            Mode::Linear {
                step,
                direction,
                second_direction,
            } => {
                let mut explicit = direction.is_none();
                if ui
                    .checkbox(&mut explicit, "Explicit translation vector")
                    .changed()
                {
                    *direction = (!explicit).then(LinearDirection::default);
                    changed = true;
                }
                if let Some(direction) = direction {
                    changed |=
                        assembly_direction_controls(ui, w, &doc, a, "Direction 1", direction);
                } else {
                    for (axis, e) in ["X", "Y", "Z"].into_iter().zip(step) {
                        changed |= crate::presentation_controls::expression(
                            ui,
                            &format!("component-pattern/step/{axis}"),
                            &format!("{axis} spacing"),
                            e,
                        );
                    }
                }
                let mut grid = second_direction.is_some();
                if ui.checkbox(&mut grid, "Second direction / grid").changed() {
                    *second_direction = grid.then(Box::default);
                    changed = true;
                }
                if let Some(second) = second_direction {
                    changed |= crate::presentation_controls::expression(
                        ui,
                        "component-pattern/second-count",
                        "Direction 2 count",
                        &mut second.count,
                    );
                    changed |= assembly_direction_controls(
                        ui,
                        w,
                        &doc,
                        a,
                        "Direction 2",
                        &mut second.direction,
                    );
                }
            }
            Mode::Circular {
                origin,
                axis,
                angle,
                fixed_orientation,
                axis_reference,
            } => {
                ui.label("Angle between occurrences / expression");
                changed |= ui.text_edit_singleline(&mut angle.0).changed();
                changed |= ui
                    .checkbox(fixed_orientation, "Keep each component's orientation fixed")
                    .changed();
                if let Some(reference) = axis_reference {
                    ui.label(format!(
                        "Associative axis: {}",
                        a.instances
                            .get(&reference.instance)
                            .map(|i| i.name.as_str())
                            .unwrap_or("Missing component")
                    ));
                    if ui.small_button("Use numeric axis").clicked() {
                        *axis_reference = None;
                        changed = true;
                    }
                } else {
                    ui.label("Axis origin");
                    for (label, e) in ["X", "Y", "Z"].into_iter().zip(origin) {
                        ui.horizontal(|ui| {
                            ui.label(label);
                            changed |= ui.text_edit_singleline(&mut e.0).changed();
                        });
                    }
                    ui.horizontal(|ui| {
                        ui.label("Axis direction");
                        for value in axis {
                            changed |= ui.add(egui::DragValue::new(value).speed(0.05)).changed();
                        }
                    });
                }
                if ui.button("Use selected axis / planar normal").clicked() {
                    match crate::mate_ui::selected_reference(w, a) {
                        Ok((instance, geometry)) => {
                            if matches!(
                                geometry.source,
                                cad_core::assembly_geometry::Source::Point { .. }
                            ) {
                                w.status =
                                    "Choose an axis, edge, or face normal for the circular pattern"
                                        .into();
                            } else {
                                *axis_reference =
                                    Some(Box::new(AxisReference { instance, geometry }));
                                changed = true;
                            }
                        }
                        Err(error) => w.status = error.to_string(),
                    }
                }
            }
        }
        ui.separator();
        ui.label(format!("Seed components · {} selected", p.seeds.len()));
        for seed in &p.seeds {
            if let Some(instance) = a.instances.get(seed) {
                ui.weak(&instance.name);
            }
        }
        let group = format!("component-pattern/{}/seeds", p.id);
        if let Some(query) = crate::workspace_controls::reference_filter(
            ui,
            w,
            &group,
            "seed components",
            p.seeds.len(),
            a.instances.len(),
        ) {
            let owned = p
                .occurrences
                .iter()
                .map(|o| &o.instance)
                .collect::<std::collections::BTreeSet<_>>();
            let rows = a
                .instances
                .values()
                .filter(|i| {
                    !owned.contains(&i.id)
                        && (query.is_empty() || i.name.to_lowercase().contains(&query))
                })
                .collect::<Vec<_>>();
            let max_height = crate::presentation_controls::properties(
                &group,
                "pattern-seed-list",
                &["reference-list"],
                "ready",
            );
            let max_height =
                crate::presentation_controls::number(&max_height, "max-height", 140.).max(24.);
            egui::ScrollArea::vertical()
                .id_salt("component-pattern-seeds")
                .max_height(max_height)
                .show_rows(
                    ui,
                    ui.text_style_height(&egui::TextStyle::Body) + 6.,
                    rows.len(),
                    |ui, range| {
                        for row in range {
                            let i = rows[row];
                            ui.push_id(&i.id, |ui| {
                                let mut selected = p.seeds.contains(&i.id);
                                if ui.checkbox(&mut selected, &i.name).changed() {
                                    changed = true;
                                    if selected {
                                        p.seeds.push(i.id.clone());
                                    } else {
                                        p.seeds.retain(|id| id != &i.id);
                                    }
                                }
                            });
                        }
                    },
                );
        }
        changed |= ui
            .checkbox(&mut p.copy_internal_mates, "Copy internal seed-group mates")
            .changed();
        ui.small("Copied mates preserve references below each copied component. Mate the seeds to position the group; external mates on generated copies are not supported yet.");
        if changed {
            w.json = serde_json::to_string_pretty(&p).unwrap_or_default();
            submit(w, assembly, p);
        }
    } else {
        ui.colored_label(
            egui::Color32::from_rgb(190, 55, 35),
            "Correct the current component pattern source.",
        );
    }
    let changed = egui::CollapsingHeader::new("Advanced Source")
        .show(ui, |ui| {
            ui.add(
                egui::TextEdit::multiline(&mut w.json)
                    .code_editor()
                    .desired_rows(12)
                    .desired_width(f32::INFINITY),
            )
            .changed()
        })
        .body_returned
        .unwrap_or(false);
    if changed {
        match parsed(w) {
            Ok(p) => submit(w, assembly, p),
            Err(error) => w.status = error.to_string(),
        }
    }
    guard(w);
    if w.fields.contains_key("component-pattern-source-invalid") {
        ui.colored_label(egui::Color32::from_rgb(190,55,35),"The current source has not been accepted; correct the seed/reference/count before applying.");
    }
    if let Some(d) = &w.session.draft {
        if let Some(error) = &d.error {
            ui.colored_label(egui::Color32::from_rgb(190, 55, 35), error);
        } else if d.pending {
            ui.weak("Updating component pattern…");
        }
    }
}

fn axis_choices(doc: &Document, owner: &str, current: Option<&str>) -> Vec<(String, AxisSource)> {
    let mut choices = [OriginAxis::X, OriginAxis::Y, OriginAxis::Z]
        .into_iter()
        .map(|axis| (axis.label().into(), AxisSource::Origin { axis }))
        .collect::<Vec<_>>();
    let Some(part) = doc.parts.get(owner) else {
        return choices;
    };
    for feature in part
        .features
        .iter()
        .take_while(|f| current != Some(f.id.as_str()))
        .filter(|f| !f.suppressed)
    {
        let node = part.reference(&feature.id);
        match &feature.operation {
            Operation::ReferenceAxis { .. } => {
                choices.push((feature.name.clone(), AxisSource::Datum { feature: node }))
            }
            Operation::ReferenceCoordinateSystem { .. } => {
                for axis in [OriginAxis::X, OriginAxis::Y, OriginAxis::Z] {
                    choices.push((
                        format!("{} · {}", feature.name, axis.label()),
                        AxisSource::CoordinateSystem {
                            feature: node.clone(),
                            axis,
                        },
                    ));
                }
            }
            Operation::Profile {
                profile: cad_core::Profile::Sketch { sketch },
                ..
            } => {
                if let Some(sketch) = part.sketches.get(sketch) {
                    for (index, (member, entity)) in sketch.members.iter().enumerate() {
                        if matches!(entity.curve, cad_core::SketchCurve::Line { .. }) {
                            choices.push((
                                format!(
                                    "{} · {} {}",
                                    feature.name,
                                    if entity.construction {
                                        "Centerline"
                                    } else {
                                        "Line"
                                    },
                                    index + 1
                                ),
                                AxisSource::SketchLine {
                                    profile: node.clone(),
                                    member: member.clone(),
                                },
                            ));
                        }
                    }
                }
            }
            Operation::Sketch3d { definition } => {
                for (index, (member, entity)) in definition.members.iter().enumerate() {
                    if matches!(entity.curve, cad_core::sketch3d::Curve::Line { .. }) {
                        choices.push((
                            format!("{} · 3D line {}", feature.name, index + 1),
                            AxisSource::SketchLine {
                                profile: node.clone(),
                                member: member.clone(),
                            },
                        ));
                    }
                }
            }
            _ => {}
        }
    }
    choices
}
fn feature_name(doc: &Document, node: &NodeRef) -> String {
    doc.parts
        .get(&node.definition)
        .and_then(|p| p.features.iter().find(|f| f.id == node.node))
        .map(|f| f.name.clone())
        .unwrap_or_else(|| "Missing geometry".into())
}
fn axis_name(doc: &Document, axis: &AxisSource) -> String {
    match axis {
        AxisSource::Origin { axis } => axis.label().into(),
        AxisSource::Datum { feature } => feature_name(doc, feature),
        AxisSource::CoordinateSystem { feature, axis } => {
            format!("{} · {}", feature_name(doc, feature), axis.label())
        }
        AxisSource::SketchLine { profile, member } => {
            let index = doc
                .parts
                .get(&profile.definition)
                .and_then(|p| p.features.iter().find(|f| f.id == profile.node))
                .and_then(|f| match &f.operation {
                    Operation::Profile {
                        profile: cad_core::Profile::Sketch { sketch },
                        ..
                    } => doc.parts[&profile.definition]
                        .sketches
                        .get(sketch)
                        .and_then(|s| s.members.keys().position(|id| id == member)),
                    Operation::Sketch3d { definition } => {
                        definition.members.keys().position(|id| id == member)
                    }
                    _ => None,
                });
            format!(
                "{} · Line {}",
                feature_name(doc, profile),
                index
                    .map(|i| (i + 1).to_string())
                    .unwrap_or_else(|| "derived".into())
            )
        }
        AxisSource::Edge { reference, .. } => format!(
            "{} · selected straight edge",
            feature_name(doc, &reference.producer)
        ),
        _ => "Unsupported curved direction".into(),
    }
}
fn path_name(doc: &Document, path: &[cad_core::Id]) -> String {
    let mut definition = doc.root.as_str();
    let mut names = vec![];
    for key in path {
        let Some(i) = doc
            .assemblies
            .get(definition)
            .and_then(|a| a.instances.get(key))
        else {
            names.push("Missing component".into());
            break;
        };
        names.push(i.name.clone());
        definition = &i.definition;
    }
    names.join(" / ")
}
fn arm_pick(w: &mut Workspace, key: &str, slot: &str) {
    if !w.fields.contains_key("pattern-pick-context-previous") {
        w.fields.insert(
            "pattern-pick-context-previous".into(),
            w.fields.contains_key("pick-context-references").to_string(),
        );
    }
    w.fields
        .insert("pick-context-references".into(), "true".into());
    w.fields.remove("part-pattern-pick-direction");
    w.fields.remove("component-pattern-pick-direction");
    w.fields.insert(key.into(), slot.into());
    w.status =
        "Select a straight sketch line, model edge, or reference axis for the pattern direction"
            .into();
}
fn end_pick(w: &mut Workspace) {
    w.fields.remove("part-pattern-pick-direction");
    w.fields.remove("component-pattern-pick-direction");
    if w.fields.remove("pattern-pick-context-previous").as_deref() == Some("false") {
        w.fields.remove("pick-context-references");
    }
}
fn selected_axis(
    w: &Workspace,
    selected: &crate::viewport::Selection,
) -> cad_core::Result<AxisSource> {
    let source = &selected.reference.producer;
    if let Some(feature) = w
        .session
        .visible()
        .parts
        .get(&source.definition)
        .and_then(|p| p.features.iter().find(|f| f.id == source.node))
        && let Operation::Sketch3d { definition } = &feature.operation
        && let Some(evaluation) = crate::viewport::display_evaluation(w)
        && let Some(instance) = evaluation
            .instances
            .iter()
            .find(|i| i.path == selected.path)
        && let Some(curve) = evaluation.reference_curves.get(&format!(
            "{}::{}",
            cad_core::history::mesh_key(instance),
            source.node
        ))
        && let Some(piece) = curve.pieces.iter().find(|piece| {
            piece.reference.producer == *source && piece.reference.role == selected.reference.role
        })
        && let Some((member, _)) = piece.id.rsplit_once("::")
    {
        if definition
            .members
            .get(member)
            .is_some_and(|m| matches!(m.curve, cad_core::sketch3d::Curve::Line { .. }))
        {
            return Ok(AxisSource::SketchLine {
                profile: source.clone(),
                member: member.into(),
            });
        }
        return Err(cad_core::CadError::Unsupported("Linear Pattern: select a straight spatial sketch member; a curved sketch member has no single direction".into()));
    }
    let axis = match selected.reference.topology.as_str() {
        "axis" => AxisSource::from_datum(&selected.reference)?,
        "edge" => crate::reference_ui::picked_axis(w, selected),
        _ => {
            return Err(cad_core::CadError::Invalid(
                "Select a straight line, edge, or reference axis".into(),
            ));
        }
    };
    PatternDirection {
        axis: axis.clone(),
        ..Default::default()
    }
    .validate()?;
    Ok(axis)
}
fn picked_part_direction(
    w: &Workspace,
    selected: &crate::viewport::Selection,
    previous: Option<&PatternDirection>,
) -> cad_core::Result<PatternDirection> {
    direction_for_axis(
        w,
        selected_axis(w, selected)?,
        &selected.definition,
        &selected.path,
        previous,
    )
}
/// A feature direction must remain upstream of its operation, including references
/// through another definition. No output/current-node alias is silently substituted.
fn source_depends_on(doc: &Document, source: &NodeRef, target: &NodeRef) -> bool {
    let mut pending = vec![source.clone()];
    let mut seen = std::collections::BTreeSet::new();
    while let Some(node) = pending.pop() {
        if &node == target {
            return true;
        }
        if !seen.insert(node.clone()) {
            continue;
        }
        let Some(part) = doc.parts.get(&node.definition) else {
            return true;
        };
        let Some(feature) = part.features.iter().find(|f| f.id == node.node) else {
            return true;
        };
        pending.extend(part.feature_dependencies(feature));
    }
    false
}
fn direction_is_upstream(
    doc: &Document,
    owner: &str,
    pattern: &str,
    axis: &AxisSource,
) -> cad_core::Result<()> {
    let target = NodeRef {
        definition: owner.into(),
        node: pattern.into(),
    };
    let own_producer = match axis {
        AxisSource::Edge { reference, .. } => reference.producer == target,
        _ => false,
    };
    if own_producer
        || axis
            .dependencies()
            .iter()
            .any(|source| source_depends_on(doc, source, &target))
    {
        return Err(cad_core::CadError::Invalid("Select a straight edge on the pattern's input body, an upstream sketch line, or an independent reference axis. The current pattern and features depending on it cannot drive its direction".into()));
    }
    Ok(())
}
fn direction_for_axis(
    w: &Workspace,
    axis: AxisSource,
    definition: &str,
    path: &[cad_core::Id],
    previous: Option<&PatternDirection>,
) -> cad_core::Result<PatternDirection> {
    let owner =
        w.view.active_part.as_ref().ok_or_else(|| {
            cad_core::CadError::Invalid("No pattern owner is being edited".into())
        })?;
    if let Some(pattern) = w.view.active_node.as_deref() {
        direction_is_upstream(w.session.visible(), owner, pattern, &axis)?;
    }
    if axis
        .dependencies()
        .iter()
        .any(|source| source.definition != definition)
    {
        return Err(cad_core::CadError::Invalid(
            "Pattern direction geometry and selected occurrence definition disagree".into(),
        ));
    }
    let owner_path = w
        .edge_selection_path
        .clone()
        .filter(|path| !path.is_empty())
        .or_else(|| {
            let evaluation = crate::viewport::display_evaluation(w)?;
            let mut paths = evaluation.instances.iter().filter(|i| {
                &i.definition == owner && i.path.last() == w.view.active_instance.as_ref()
            });
            let path = paths.next()?.path.clone();
            if paths.next().is_some() {
                None
            } else {
                Some(path)
            }
        });
    let occurrence = if definition != owner
        || owner_path.as_ref().is_some_and(|p| p.as_slice() != path)
    {
        Some(PatternOccurrence { path:path.to_vec(),owner_path:owner_path.ok_or_else(||cad_core::CadError::Invalid("Choose the complete editing occurrence before using a direction on another component".into()))? })
    } else {
        None
    };
    Ok(PatternDirection {
        axis,
        occurrence,
        ..previous.cloned().unwrap_or_default()
    })
}
/// Source-bound native adapter over renderer-independent pattern directions.
pub fn part_direction_controls(
    ui: &mut egui::Ui,
    w: &mut Workspace,
    doc: &Document,
    current: &str,
    label: &str,
    step: &mut [cad_core::Expr; 3],
    direction: &mut Option<PatternDirection>,
) -> bool {
    let mut changed = false;
    ui.push_id(label, |ui| {
        let mut explicit = direction.is_none();
        if ui
            .checkbox(&mut explicit, format!("{label} · explicit vector"))
            .changed()
        {
            *direction = (!explicit).then(|| PatternDirection {
                axis: AxisSource::Origin {
                    axis: if label.ends_with('2') {
                        OriginAxis::Y
                    } else {
                        OriginAxis::X
                    },
                },
                ..Default::default()
            });
            changed = true;
        }
        if let Some(source) = direction {
            let mut choice = source.axis.clone();
            egui::ComboBox::from_label(label)
                .selected_text(axis_name(doc, &source.axis))
                .show_ui(ui, |ui| {
                    if let Some(owner) = w.view.active_part.as_deref() {
                        for (name, axis) in axis_choices(doc, owner, Some(current)) {
                            changed |= ui.selectable_value(&mut choice, axis, name).changed();
                        }
                    }
                });
            if choice != source.axis {
                source.axis = choice;
                source.occurrence = None;
            }
            if let Some(path) = &source.occurrence {
                ui.weak(path_name(doc, &path.path));
            }
            changed |= crate::presentation_controls::expression(
                ui,
                &format!("part-pattern/{label}/spacing"),
                "Spacing",
                &mut source.spacing,
            );
            changed |= ui
                .checkbox(&mut source.reverse, "Reverse direction")
                .changed();
        } else {
            for (axis, value) in ["X", "Y", "Z"].into_iter().zip(step) {
                changed |= crate::presentation_controls::expression(
                    ui,
                    &format!("part-pattern/{label}/{axis}"),
                    &format!("{axis} spacing"),
                    value,
                );
            }
        }
        if crate::presentation_controls::button(
            ui,
            &format!("part-pattern/{label}/pick"),
            "pattern-direction-action",
            &["operation-input", "direction-pick"],
            "Pick direction on model",
            true,
            false,
        )
        .clicked()
        {
            if let Some(selected) = w.selected.as_ref() {
                match picked_part_direction(w, selected, direction.as_ref()) {
                    Ok(source) => {
                        *direction = Some(source);
                        changed = true;
                        end_pick(w);
                    }
                    Err(error) => {
                        w.status = error.to_string();
                        arm_pick(w, "part-pattern-pick-direction", label);
                    }
                }
            } else {
                arm_pick(w, "part-pattern-pick-direction", label);
            }
        }
        if w.fields
            .get("part-pattern-pick-direction")
            .is_some_and(|s| s == label)
        {
            ui.weak("Select direction geometry in the viewport");
            if ui.small_button("Cancel direction selection").clicked() {
                end_pick(w);
            }
        }
    });
    changed
}
fn assembly_reference_name(
    doc: &Document,
    a: &cad_core::Assembly,
    reference: &AxisReference,
) -> String {
    let direct = a.instances.get(&reference.instance);
    let mut names = vec![
        direct
            .map(|i| i.name.clone())
            .unwrap_or_else(|| "Missing component".into()),
    ];
    let mut definition = direct.map(|i| i.definition.as_str()).unwrap_or_default();
    for key in &reference.geometry.path {
        let Some(i) = doc
            .assemblies
            .get(definition)
            .and_then(|a| a.instances.get(key))
        else {
            names.push("Missing child".into());
            break;
        };
        names.push(i.name.clone());
        definition = &i.definition;
    }
    let component = names.join(" / ");
    let name = if let Source::Axis { axis } = &reference.geometry.source {
        axis_name(doc, axis)
    } else {
        "Unsupported direction".into()
    };
    format!("{component} / {name}")
}
fn assembly_direction_controls(
    ui: &mut egui::Ui,
    w: &mut Workspace,
    doc: &Document,
    a: &cad_core::Assembly,
    label: &str,
    direction: &mut LinearDirection,
) -> bool {
    let mut changed = false;
    ui.push_id(label, |ui| {
        ui.label(label);
        if let Some(reference) = &direction.reference {
            ui.weak(assembly_reference_name(doc, a, reference));
        }
        changed |= crate::presentation_controls::expression(
            ui,
            &format!("component-pattern/{label}/spacing"),
            "Spacing",
            &mut direction.spacing,
        );
        changed |= ui
            .checkbox(&mut direction.reverse, "Reverse direction")
            .changed();
        egui::ComboBox::from_label("Direction reference")
            .selected_text(
                direction
                    .reference
                    .as_deref()
                    .map(|r| assembly_reference_name(doc, a, r))
                    .unwrap_or_else(|| "Explicit vector".into()),
            )
            .show_ui(ui, |ui| {
                if ui
                    .selectable_label(direction.reference.is_none(), "Explicit vector")
                    .clicked()
                {
                    direction.reference = None;
                    changed = true;
                }
                fn visit(
                    ui: &mut egui::Ui,
                    doc: &Document,
                    source: (&str, &cad_core::Id, Vec<cad_core::Id>, String, usize),
                    direction: &mut LinearDirection,
                ) -> bool {
                    let (definition, direct, path, prefix, depth) = source;
                    if depth > 64 {
                        return false;
                    }
                    let mut changed = false;
                    for (name, axis) in axis_choices(doc, definition, None) {
                        let reference = AxisReference {
                            instance: direct.clone(),
                            geometry: Reference {
                                path: path.clone(),
                                source: Source::Axis { axis },
                            },
                        };
                        if ui
                            .selectable_label(
                                direction.reference.as_deref() == Some(&reference),
                                format!("{prefix} / {name}"),
                            )
                            .clicked()
                        {
                            direction.reference = Some(Box::new(reference));
                            changed = true;
                        }
                    }
                    if let Some(a) = doc.assemblies.get(definition) {
                        for i in a.instances.values() {
                            let mut child = path.clone();
                            child.push(i.id.clone());
                            changed |= visit(
                                ui,
                                doc,
                                (
                                    &i.definition,
                                    direct,
                                    child,
                                    format!("{prefix} / {}", i.name),
                                    depth + 1,
                                ),
                                direction,
                            );
                        }
                    }
                    changed
                }
                for i in a.instances.values() {
                    changed |= visit(
                        ui,
                        doc,
                        (&i.definition, &i.id, vec![], i.name.clone(), 0),
                        direction,
                    );
                }
            });
        if direction.reference.is_none() {
            ui.horizontal(|ui| {
                for (name, vector) in [
                    ("X", [1., 0., 0.]),
                    ("Y", [0., 1., 0.]),
                    ("Z", [0., 0., 1.]),
                ] {
                    changed |= ui
                        .selectable_value(&mut direction.vector, vector, name)
                        .changed();
                }
            });
            egui::CollapsingHeader::new("Custom direction vector").show(ui, |ui| {
                ui.horizontal(|ui| {
                    for value in &mut direction.vector {
                        changed |= ui.add(egui::DragValue::new(value).speed(0.05)).changed();
                    }
                });
            });
        }
        if crate::presentation_controls::button(
            ui,
            &format!("component-pattern/{label}/pick"),
            "pattern-direction-action",
            &["operation-input", "direction-pick"],
            "Pick direction on model",
            true,
            false,
        )
        .clicked()
        {
            arm_pick(w, "component-pattern-pick-direction", label);
        }
        if w.fields
            .get("component-pattern-pick-direction")
            .is_some_and(|s| s == label)
        {
            ui.weak("Select direction geometry in the viewport");
            if ui.small_button("Cancel direction selection").clicked() {
                end_pick(w);
            }
        }
    });
    changed
}
/// Consume an explicitly armed pick before ordinary feature/body selection changes scope.
fn reference_for_path(
    w: &Workspace,
    a: &cad_core::Assembly,
    axis: AxisSource,
    path: &[cad_core::Id],
) -> cad_core::Result<AxisReference> {
    let mut definition = w.session.visible().root.as_str();
    for (index, key) in path.iter().enumerate() {
        let owner = w
            .session
            .visible()
            .assemblies
            .get(definition)
            .ok_or_else(|| {
                cad_core::CadError::Invalid(
                    "Pattern direction path is not in the edited assembly".into(),
                )
            })?;
        let i = owner.instances.get(key).ok_or_else(|| {
            cad_core::CadError::Invalid("Pattern direction component is missing".into())
        })?;
        if owner.id == a.id {
            let reference = Reference {
                path: path[index + 1..].to_vec(),
                source: Source::Axis { axis },
            };
            cad_core::assembly_geometry::validate_reference(
                w.session.visible(),
                &i.definition,
                &reference,
            )?;
            component_pattern::straight_reference(&reference)?;
            return Ok(AxisReference {
                instance: key.clone(),
                geometry: reference,
            });
        }
        definition = &i.definition;
    }
    Err(cad_core::CadError::Invalid(
        "Select a direction on a component of the edited assembly".into(),
    ))
}
pub fn capture_selection(w: &mut Workspace, selected: &crate::viewport::Selection) -> bool {
    if !picking_direction(w) {
        return false;
    }
    match selected_axis(w, selected) {
        Ok(axis) => capture_axis(w, axis, &selected.definition, &selected.path),
        Err(error) => {
            w.status = error.to_string();
            true
        }
    }
}
fn capture_axis(
    w: &mut Workspace,
    axis: AxisSource,
    definition: &str,
    path: &[cad_core::Id],
) -> bool {
    if let Some(slot) = w.fields.get("component-pattern-pick-direction").cloned() {
        let Some(owner) = w.fields.get("assembly-id").cloned() else {
            end_pick(w);
            return false;
        };
        let Some(a) = w.session.visible().assemblies.get(&owner).cloned() else {
            end_pick(w);
            return false;
        };
        let reference = reference_for_path(w, &a, axis, path);
        match (parsed(w), reference) {
            (Ok(mut p), Ok(reference)) => {
                if let Mode::Linear {
                    direction,
                    second_direction,
                    ..
                } = &mut p.mode
                {
                    let target = if slot.ends_with('2') {
                        second_direction.as_mut().map(|d| &mut d.direction)
                    } else {
                        Some(direction.get_or_insert_with(Default::default))
                    };
                    if let Some(target) = target {
                        target.reference = Some(Box::new(reference));
                        submit(w, &owner, p);
                        end_pick(w);
                    }
                }
            }
            (_, Err(error)) => w.status = error.to_string(),
            (Err(error), _) => {
                w.status = error.to_string();
                end_pick(w);
            }
        }
        return true;
    }
    if let Some(slot) = w.fields.get("part-pattern-pick-direction").cloned() {
        let feature = serde_json::from_str::<cad_core::Feature>(&w.json);
        match feature {
            Ok(mut feature) => {
                if let Operation::Pattern { options, .. } = &mut feature.operation {
                    let target = if slot.ends_with('2') {
                        options.second_direction.as_mut().map(|d| &mut d.direction)
                    } else {
                        Some(&mut options.direction)
                    };
                    if let Some(target) = target {
                        match direction_for_axis(w, axis, definition, path, target.as_ref()) {
                            Ok(source) => {
                                *target = Some(source);
                                crate::ui::preview_feature(w, feature);
                                end_pick(w);
                            }
                            Err(error) => w.status = error.to_string(),
                        }
                    }
                } else {
                    end_pick(w);
                }
            }
            Err(error) => {
                w.status = error.to_string();
                end_pick(w);
            }
        }
        return true;
    }
    false
}
pub fn picking_direction(w: &Workspace) -> bool {
    w.fields.contains_key("part-pattern-pick-direction")
        || w.fields.contains_key("component-pattern-pick-direction")
}

fn scope_label(doc: &Document, a: &cad_core::Assembly, p: &Definition) -> String {
    let names = p
        .seeds
        .iter()
        .filter_map(|id| a.instances.get(id))
        .take(3)
        .map(|i| {
            format!(
                "{}{}",
                i.name,
                if doc.assemblies.contains_key(&i.definition) {
                    " (complete subassembly)"
                } else {
                    ""
                }
            )
        })
        .collect::<Vec<_>>();
    format!(
        "Owner: {} · Seeds: {}{}",
        a.name,
        names.join(", "),
        if p.seeds.len() > 3 {
            format!(" +{} more", p.seeds.len() - 3)
        } else {
            String::new()
        }
    )
}
fn active_component(w: &Workspace) -> Option<(&str, &Definition)> {
    let owner = w.fields.get("assembly-id")?;
    let id = w.fields.get("component-pattern-editor")?;
    Some((
        owner,
        w.session
            .visible()
            .assemblies
            .get(owner)?
            .component_patterns
            .get(id)?,
    ))
}
fn field_json(pattern: &Definition, field: &str) -> Option<serde_json::Value> {
    match field {
        "/name" => Some(pattern.name.clone().into()),
        "/count" => Some(pattern.count.0.clone().into()),
        "/copy_internal_mates" => Some(pattern.copy_internal_mates.into()),
        _ => {
            let Mode::Linear {
                direction,
                second_direction,
                step,
            } = &pattern.mode
            else {
                return None;
            };
            if let Some(index) = field
                .strip_prefix("/mode/step/")
                .and_then(|s| s.parse::<usize>().ok())
                .filter(|i| *i < 3)
            {
                return Some(step[index].0.clone().into());
            }
            if field == "/mode/second_direction/count" {
                return Some(second_direction.as_ref()?.count.0.clone().into());
            }
            let (source, tail) = if let Some(tail) = field.strip_prefix("/mode/direction/") {
                (direction.as_ref()?, tail)
            } else {
                let tail = field.strip_prefix("/mode/second_direction/direction/")?;
                (&second_direction.as_ref()?.direction, tail)
            };
            match tail {
                "spacing" => Some(source.spacing.0.clone().into()),
                "reverse" => Some(source.reverse.into()),
                "reference" => serde_json::to_value(&source.reference).ok(),
                _ => tail
                    .strip_prefix("vector/")
                    .and_then(|s| s.parse::<usize>().ok())
                    .filter(|i| *i < 3)
                    .map(|i| source.vector[i].into()),
            }
        }
    }
}
pub fn value(
    w: &Workspace,
    element: &cad_core::presentation::SemanticElement,
) -> Option<serde_json::Value> {
    let (owner, pattern) = active_component(w)?;
    if element.data["document"].as_str() != Some(w.session.visible().id.as_str())
        || element.data["owner"].as_str() != Some(owner)
        || element.data["pattern"].as_str() != Some(pattern.id.as_str())
    {
        return None;
    }
    if let Some(seed) = element.data["seed"].as_str() {
        return Some(pattern.seeds.iter().any(|id| id == seed).into());
    }
    let field = element.data["field"].as_str()?;
    w.fields
        .get(&format!("component-pattern-input:{}:{field}", pattern.id))
        .map(|text| text.clone().into())
        .or_else(|| field_json(pattern, field))
}
pub fn set_value(
    w: &mut Workspace,
    element: &cad_core::presentation::SemanticElement,
    new_value: serde_json::Value,
) -> cad_core::Result<()> {
    let (owner, current) = active_component(w).ok_or_else(|| {
        cad_core::CadError::Invalid("No component pattern is being edited".into())
    })?;
    if element.data["document"].as_str() != Some(w.session.visible().id.as_str())
        || element.data["owner"].as_str() != Some(owner)
        || element.data["pattern"].as_str() != Some(current.id.as_str())
    {
        return Err(cad_core::CadError::Invalid(
            "Component pattern field belongs to another editor".into(),
        ));
    }
    let owner = owner.to_string();
    let mut pattern = parsed(w)?;
    if let Some(seed) = element.data["seed"].as_str() {
        let selected = new_value.as_bool().ok_or_else(|| {
            cad_core::CadError::Invalid("Seed selection needs a boolean value".into())
        })?;
        let a = &w.session.visible().assemblies[&owner];
        if !a.instances.contains_key(seed) || pattern.occurrences.iter().any(|e| e.instance == seed)
        {
            return Err(cad_core::CadError::Invalid(
                "This component is not eligible as a seed".into(),
            ));
        }
        pattern.seeds.retain(|id| id != seed);
        if selected {
            pattern.seeds.push(seed.into());
        }
    } else {
        let field = element.data["field"].as_str().ok_or_else(|| {
            cad_core::CadError::Invalid("Component pattern field has no source binding".into())
        })?;
        let old = field_json(&pattern, field).ok_or_else(|| {
            cad_core::CadError::Invalid("This pattern field is unavailable".into())
        })?;
        if old.is_string() && !new_value.is_string()
            || old.is_boolean() && !new_value.is_boolean()
            || old.is_number() && !new_value.is_number()
        {
            return Err(cad_core::CadError::Invalid(
                "Component pattern field value has the wrong type".into(),
            ));
        }
        let mut source = serde_json::to_value(&pattern)?;
        *source.pointer_mut(field).ok_or_else(|| {
            cad_core::CadError::Invalid("Component pattern binding changed".into())
        })? = new_value.clone();
        pattern = serde_json::from_value(source)?;
        let key = format!("component-pattern-input:{}:{field}", pattern.id);
        if let Some(text) = new_value.as_str() {
            w.fields.insert(key.clone(), text.into());
        }
        w.json = serde_json::to_string_pretty(&pattern)?;
        submit(w, &owner, pattern.clone());
        if current_source_is_valid(w) {
            w.fields.remove(&key);
            return Ok(());
        } else {
            return Err(cad_core::CadError::Invalid(w.status.clone()));
        }
    }
    w.json = serde_json::to_string_pretty(&pattern)?;
    submit(w, &owner, pattern);
    if current_source_is_valid(w) {
        Ok(())
    } else {
        Err(cad_core::CadError::Invalid(w.status.clone()))
    }
}
pub fn semantics(w: &Workspace) -> Vec<cad_core::presentation::SemanticElement> {
    use cad_core::presentation::SemanticElement;
    let mut out = vec![];
    let doc = w.session.visible();
    if let Some((owner, pattern)) = active_component(w) {
        let valid = current_source_is_valid(w);
        out.push(SemanticElement {id:format!("component-pattern:{}/scope",pattern.id),role:"pattern-owner-context".into(),label:scope_label(doc,&doc.assemblies[owner],pattern),value:None,state:"ready".into(),commands:vec![],relationships:std::iter::once(owner.into()).chain(pattern.seeds.iter().cloned()).collect(),data:serde_json::json!({"contract":"tcad.component-pattern","version":component_pattern::CONTRACT_VERSION,"document":doc.id,"owner":owner,"pattern":pattern.id,"seeds":pattern.seeds,"scope":"direct_owner_occurrences","nested_seed_behavior":"complete_subassembly"})});
        let mut fields = vec![
            ("/name".to_string(), "Name".to_string()),
            ("/count".into(), "Direction 1 count".into()),
            (
                "/copy_internal_mates".into(),
                "Copy internal seed-group mates".into(),
            ),
        ];
        if let Mode::Linear {
            direction,
            second_direction,
            ..
        } = &pattern.mode
        {
            for (prefix, label, source) in [
                ("/mode/direction", "Direction 1", direction.as_ref()),
                (
                    "/mode/second_direction/direction",
                    "Direction 2",
                    second_direction.as_ref().map(|s| &s.direction),
                ),
            ] {
                if let Some(source) = source {
                    for (suffix, name) in [("spacing", "Spacing"), ("reverse", "Reverse direction")]
                    {
                        fields.push((format!("{prefix}/{suffix}"), format!("{label} · {name}")));
                    }
                    if source.reference.is_none() {
                        for i in 0..3 {
                            fields.push((
                                format!("{prefix}/vector/{i}"),
                                format!("{label} · {} vector", ["X", "Y", "Z"][i]),
                            ));
                        }
                    }
                }
            }
            if second_direction.is_some() {
                fields.push((
                    "/mode/second_direction/count".into(),
                    "Direction 2 count".into(),
                ));
            }
            if direction.is_none() {
                for i in 0..3 {
                    fields.push((
                        format!("/mode/step/{i}"),
                        format!("{} spacing", ["X", "Y", "Z"][i]),
                    ));
                }
            }
        }
        for (field, label) in fields {
            if let Some(json) = field_json(pattern, &field) {
                out.push(SemanticElement {id:format!("component-pattern:{}/field{field}",pattern.id),role:"component-pattern-field".into(),label,value:Some(json.as_str().map(str::to_string).unwrap_or_else(||json.to_string())),state:if valid{"draft"}else{"invalid"}.into(),commands:vec![],relationships:vec![pattern.id.clone(),owner.into()],data:serde_json::json!({"contract":"tcad.component-pattern","version":component_pattern::CONTRACT_VERSION,"setter":"component-pattern-editor","document":doc.id,"owner":owner,"pattern":pattern.id,"field":field,"json":json,"editable":true,"value_type":if json.is_boolean(){"boolean"}else if json.is_number(){"number"}else{"text"},"unit":if field.ends_with("spacing")||field.contains("/step/"){Some(doc.settings.display_length_unit.symbol())}else{None},"dimension":if field.ends_with("spacing")||field.contains("/step/"){Some(cad_core::Dimension::Length)}else if field.ends_with("count"){Some(cad_core::Dimension::Scalar)}else{None}})});
            }
        }
        let a = &doc.assemblies[owner];
        let generated = pattern
            .occurrences
            .iter()
            .map(|e| &e.instance)
            .collect::<std::collections::BTreeSet<_>>();
        for i in a.instances.values().filter(|i| !generated.contains(&i.id)) {
            out.push(SemanticElement {id:format!("component-pattern:{}/seed/{}",pattern.id,i.id),role:"component-pattern-seed".into(),label:i.name.clone(),value:Some(pattern.seeds.contains(&i.id).to_string()),state:if pattern.seeds.contains(&i.id){"selected"}else{"available"}.into(),commands:vec![],relationships:vec![pattern.id.clone(),i.id.clone()],data:serde_json::json!({"contract":"tcad.component-pattern","version":component_pattern::CONTRACT_VERSION,"setter":"component-pattern-editor","document":doc.id,"owner":owner,"pattern":pattern.id,"seed":i.id,"json":pattern.seeds.contains(&i.id),"value_type":"boolean","editable":true})});
        }
        out.extend(crate::workspace_controls::reference_controls(
            w,
            &format!("component-pattern/{}/seeds", pattern.id),
            "seed components",
        ));
        if let Mode::Linear {
            second_direction, ..
        } = &pattern.mode
        {
            for slot in 0..if second_direction.is_some() { 2 } else { 1 } {
                out.push(pick_element(w, owner, &pattern.id, true, slot));
            }
        }
    }
    if w.dialog.as_deref() == Some("feature")
        && let Some(owner) = w.view.active_part.as_deref()
        && let Some(feature) = doc.parts.get(owner).and_then(|p| {
            p.features
                .iter()
                .find(|f| Some(&f.id) == w.view.active_node.as_ref())
        })
        && let Operation::Pattern { options, .. } = &feature.operation
    {
        for slot in 0..if options.second_direction.is_some() {
            2
        } else {
            1
        } {
            out.push(pick_element(w, owner, &feature.id, false, slot));
        }
    }
    out.extend(viewport_guides(w));
    out.extend(sketch_direction_candidates(w));
    out
}
fn pick_element(
    w: &Workspace,
    owner: &str,
    pattern: &str,
    component: bool,
    slot: usize,
) -> cad_core::presentation::SemanticElement {
    cad_core::presentation::SemanticElement {
        id: format!("pattern:{pattern}/direction-{slot}/pick"),
        role: "pattern-direction-action".into(),
        label: format!("Pick Direction {} on model", slot + 1),
        value: None,
        state: if picking_direction(w) {
            "selected"
        } else {
            "available"
        }
        .into(),
        commands: vec![
            "pattern.direction-pick".into(),
            "pattern.direction-vector".into(),
            "pattern.direction-reverse".into(),
            "pattern.grid-toggle".into(),
        ],
        relationships: vec![pattern.into(), owner.into()],
        data: serde_json::json!({"contract":"tcad.presentation.pattern-direction","version":1,"document":w.session.visible().id,"owner":owner,"pattern":pattern,"component":component,"slot":slot,"accepted_sources":["straight_sketch_line","reference_axis","straight_component_edge"],"complete_occurrence_path":true,"actions":[{"command":"pattern.direction-pick","label":format!("Pick Direction {} on model",slot+1)},{"command":"pattern.direction-vector","label":format!("Toggle Direction {} vector/reference",slot+1)},{"command":"pattern.direction-reverse","label":format!("Reverse Direction {}",slot+1)},{"command":"pattern.grid-toggle","label":"Toggle second direction / grid"}]}),
    }
}
pub fn action(w: &mut Workspace, command: &str, data: &serde_json::Value) -> bool {
    if !matches!(
        command,
        "pattern.direction-pick"
            | "pattern.direction-cancel"
            | "pattern.direction-reverse"
            | "pattern.direction-source"
            | "pattern.grid-toggle"
            | "pattern.direction-vector"
    ) {
        return false;
    }
    if command == "pattern.direction-cancel" {
        end_pick(w);
        return true;
    }
    if data["document"].as_str() != Some(w.session.visible().id.as_str()) {
        w.status = "Pattern direction action belongs to another document".into();
        return true;
    }
    let slot = data["slot"].as_u64().unwrap_or(0);
    if slot > 1 {
        w.status = "Pattern direction is unavailable".into();
        return true;
    }
    let component = data["component"].as_bool().unwrap_or(false);
    let live = if component {
        active_component(w).is_some_and(|(owner, p)| {
            data["owner"].as_str() == Some(owner) && data["pattern"].as_str() == Some(p.id.as_str())
        })
    } else {
        w.dialog.as_deref() == Some("feature")
            && data["owner"].as_str() == w.view.active_part.as_deref()
            && data["pattern"].as_str() == w.view.active_node.as_deref()
    };
    if !live {
        w.status = "Pattern direction editor changed".into();
        return true;
    }
    if matches!(command, "pattern.grid-toggle" | "pattern.direction-vector") {
        if component {
            if let Ok(mut p) = parsed(w)
                && let Mode::Linear {
                    direction,
                    second_direction,
                    ..
                } = &mut p.mode
            {
                if command == "pattern.grid-toggle" {
                    *second_direction = if second_direction.is_some() {
                        None
                    } else {
                        Some(Box::default())
                    };
                } else if slot == 0 {
                    *direction = if direction.is_some() {
                        None
                    } else {
                        Some(LinearDirection::default())
                    };
                } else if let Some(second) = second_direction {
                    second.direction.reference = None;
                }
                let owner = w.fields["assembly-id"].clone();
                submit(w, &owner, p);
            }
        } else if let Ok(mut feature) = serde_json::from_str::<cad_core::Feature>(&w.json)
            && let Operation::Pattern { options, .. } = &mut feature.operation
        {
            if command == "pattern.grid-toggle" {
                options.second_direction = if options.second_direction.is_some() {
                    None
                } else {
                    Some(Default::default())
                };
            } else {
                let target = if slot == 0 {
                    Some(&mut options.direction)
                } else {
                    options.second_direction.as_mut().map(|d| &mut d.direction)
                };
                if let Some(target) = target {
                    *target = if target.is_some() {
                        None
                    } else {
                        Some(PatternDirection {
                            axis: AxisSource::Origin {
                                axis: if slot == 0 {
                                    OriginAxis::X
                                } else {
                                    OriginAxis::Y
                                },
                            },
                            ..Default::default()
                        })
                    };
                }
            }
            crate::ui::preview_feature(w, feature);
        }
        end_pick(w);
        return true;
    }
    if command == "pattern.direction-source" {
        if !picking_direction(w) {
            w.status = "Choose Pick direction before selecting a sketch source".into();
            return true;
        }
        let Some(e) = current_evaluation(w) else {
            w.status = "Wait for the current sketch evaluation before choosing a direction".into();
            return true;
        };
        if data["evaluation_identity"] != serde_json::to_value(&e.identity).unwrap_or_default() {
            w.status = "Sketch direction changed during evaluation; choose the member again".into();
            return true;
        }
        let axis = serde_json::from_value::<AxisSource>(data["source_axis"].clone());
        let path = serde_json::from_value::<Vec<cad_core::Id>>(data["occurrence_path"].clone());
        if let (Ok(axis), Ok(path), Some(definition)) =
            (axis, path, data["source_definition"].as_str())
        {
            capture_axis(w, axis, definition, &path);
        } else {
            w.status = "Sketch direction source is unavailable".into();
        }
        return true;
    }
    if command == "pattern.direction-pick" {
        arm_pick(
            w,
            if component {
                "component-pattern-pick-direction"
            } else {
                "part-pattern-pick-direction"
            },
            if slot == 0 {
                "Direction 1"
            } else {
                "Direction 2"
            },
        );
        return true;
    }
    if component {
        if let Ok(mut p) = parsed(w)
            && let Mode::Linear {
                direction,
                second_direction,
                step,
            } = &mut p.mode
        {
            let target = if slot == 0 {
                direction.as_mut()
            } else {
                second_direction.as_mut().map(|d| &mut d.direction)
            };
            if let Some(target) = target {
                target.reverse = !target.reverse;
            } else if slot == 0 {
                for value in step {
                    value.0 = format!("-({})", value.0);
                }
            }
            let owner = w.fields["assembly-id"].clone();
            submit(w, &owner, p);
        }
    } else if let Ok(mut feature) = serde_json::from_str::<cad_core::Feature>(&w.json)
        && let Operation::Pattern { step, options, .. } = &mut feature.operation
    {
        let target = if slot == 0 {
            options.direction.as_mut()
        } else {
            options
                .second_direction
                .as_mut()
                .and_then(|d| d.direction.as_mut())
        };
        if let Some(target) = target {
            target.reverse = !target.reverse;
        } else {
            let values = if slot == 0 {
                Some(step)
            } else {
                options.second_direction.as_mut().map(|d| &mut d.step)
            };
            if let Some(values) = values {
                for value in values {
                    value.0 = format!("-({})", value.0);
                }
            }
        }
        crate::ui::preview_feature(w, feature);
    }
    true
}

fn matrix_product(a: [[f64; 4]; 4], b: [[f64; 4]; 4]) -> [[f64; 4]; 4] {
    std::array::from_fn(|i| std::array::from_fn(|j| (0..4).map(|k| a[i][k] * b[k][j]).sum()))
}
fn matrix_point(a: [[f64; 4]; 4], p: [f64; 3]) -> [f64; 3] {
    std::array::from_fn(|i| a[i][0] * p[0] + a[i][1] * p[1] + a[i][2] * p[2] + a[i][3])
}
fn assembly_contexts(
    w: &Workspace,
    e: &cad_core::protocol::Evaluation,
    owner: &str,
) -> Vec<(Vec<cad_core::Id>, [[f64; 4]; 4], String)> {
    let doc = w.session.visible();
    let identity = [
        [1., 0., 0., 0.],
        [0., 1., 0., 0.],
        [0., 0., 1., 0.],
        [0., 0., 0., 1.],
    ];
    if doc.root == owner {
        return vec![(
            vec![],
            identity,
            cad_core::configuration::key(
                owner,
                doc.active_configurations.get(owner).map(String::as_str),
            ),
        )];
    }
    let mut out = std::collections::BTreeMap::new();
    for leaf in &e.instances {
        let mut definition = doc.root.as_str();
        let mut configuration = doc
            .active_configurations
            .get(definition)
            .map(String::as_str);
        let mut matrix = identity;
        let mut path = vec![];
        for key in &leaf.path {
            let Some(a) = doc.assemblies.get(definition) else {
                break;
            };
            let Some(i) = a.instances.get(key) else {
                break;
            };
            let evaluated = e
                .assemblies
                .get(&cad_core::configuration::key(definition, configuration));
            let Some(placement) = evaluated.and_then(|a| a.placements.get(key)) else {
                break;
            };
            let m = placement.matrix();
            matrix = matrix_product(
                matrix,
                std::array::from_fn(|r| std::array::from_fn(|c| m[(r, c)])),
            );
            path.push(key.clone());
            definition = &i.definition;
            configuration = i.configuration.as_deref().or_else(|| {
                doc.active_configurations
                    .get(definition)
                    .map(String::as_str)
            });
            if definition == owner {
                out.entry(path.clone())
                    .or_insert((matrix, cad_core::configuration::key(owner, configuration)));
                break;
            }
        }
    }
    if let Some(selected) = w
        .selected_component
        .as_ref()
        .or(w.edge_selection_path.as_ref())
    {
        let matches = out
            .keys()
            .filter(|path| selected.starts_with(path))
            .cloned()
            .collect::<Vec<_>>();
        if matches.len() == 1 {
            out.retain(|path, _| path == &matches[0]);
        }
    }
    out.into_iter()
        .map(|(path, (matrix, key))| (path, matrix, key))
        .collect()
}
/// Semantic viewport arrows contain model coordinates, never pixels or styling.
pub fn viewport_guides(w: &Workspace) -> Vec<cad_core::presentation::SemanticElement> {
    let Some(e) = crate::viewport::display_evaluation(w) else {
        return vec![];
    };
    let current = e.identity.backend == w.session.backend
        && e.identity.generation == w.session.generation
        && e.identity.base_revision == w.session.committed.revision
        && e.identity.draft.as_ref() == w.session.draft.as_ref().map(|d| &d.id)
        && e.identity.edit == w.session.draft.as_ref().map_or(0, |d| d.edit);
    if !current || !e.geometry_valid {
        return vec![];
    }
    let doc = w.session.visible();
    let mut out = vec![];
    let mut add = |owner: &str,
                   pattern: &str,
                   name: &str,
                   component: bool,
                   path: &[cad_core::Id],
                   matrix: [[f64; 4]; 4],
                   guide: &cad_core::body_editing::PatternGuide| {
        if guide.document != doc.id {
            return;
        }
        for slot in 0..2 {
            let step = guide.steps_mm[slot];
            if guide.counts[slot] < 2 || step.iter().all(|v| v.abs() < 1e-12) {
                continue;
            }
            let origin = matrix_point(matrix, guide.origin_mm);
            let end = matrix_point(
                matrix,
                std::array::from_fn(|i| guide.origin_mm[i] + step[i]),
            );
            out.push(cad_core::presentation::SemanticElement {id:format!("pattern:{pattern}/direction-{slot}/guide/{}",path.join("/")),role:"pattern-direction-guide".into(),label:format!("{name} · Direction {}",slot+1),value:Some(guide.counts[slot].to_string()),state:"preview".into(),commands:vec!["pattern.direction-reverse".into(),"pattern.direction-pick".into()],relationships:vec![pattern.into(),owner.into()],data:serde_json::json!({"contract":"tcad.presentation.pattern-direction","version":1,"document":doc.id,"owner":owner,"pattern":pattern,"component":component,"slot":slot,"occurrence_path":path,"space":"root","origin_mm":origin,"end_mm":end,"count":guide.counts[slot],"unit":"mm","evaluation_identity":e.identity,"disposable_display":true})});
        }
    };
    if let Some((owner, pattern)) = active_component(w) {
        for (path, matrix, key) in assembly_contexts(w, e, owner) {
            if let Some(guide) = e
                .assemblies
                .get(&key)
                .and_then(|a| a.pattern_guides.get(&pattern.id))
            {
                add(
                    owner,
                    &pattern.id,
                    &pattern.name,
                    true,
                    &path,
                    matrix,
                    guide,
                );
            }
        }
    } else if w.dialog.as_deref() == Some("feature")
        && let Some(owner) = w.view.active_part.as_deref()
        && let Some(feature) = doc.parts.get(owner).and_then(|p| {
            p.features
                .iter()
                .find(|f| Some(&f.id) == w.view.active_node.as_ref())
        })
        && matches!(feature.operation, Operation::Pattern { .. })
    {
        for instance in e.instances.iter().filter(|i| {
            i.definition == owner
                && w.edge_selection_path
                    .as_ref()
                    .is_none_or(|path| path == &i.path)
        }) {
            let key = format!("{}::{}", cad_core::history::mesh_key(instance), feature.id);
            if let Some(guide) = e.pattern_guides.get(&key) {
                add(
                    owner,
                    &feature.id,
                    &feature.name,
                    false,
                    &instance.path,
                    instance.matrix,
                    guide,
                );
            }
        }
    }
    out
}

pub fn cancel(w: &mut Workspace) {
    end_pick(w);
    w.fields
        .retain(|key, _| !key.starts_with("component-pattern-input:"));
}

fn current_evaluation(w: &Workspace) -> Option<&cad_core::protocol::Evaluation> {
    let e = crate::viewport::display_evaluation(w)?;
    (e.geometry_valid
        && e.identity.backend == w.session.backend
        && e.identity.generation == w.session.generation
        && e.identity.base_revision == w.session.committed.revision
        && e.identity.draft.as_ref() == w.session.draft.as_ref().map(|d| &d.id)
        && e.identity.edit == w.session.draft.as_ref().map_or(0, |d| d.edit))
    .then_some(e)
}
/// Current solved sketch-member hit/display candidates; native member IDs remain the inputs.
pub fn sketch_direction_candidates(w: &Workspace) -> Vec<cad_core::presentation::SemanticElement> {
    if !picking_direction(w) {
        return vec![];
    }
    let Some(e) = current_evaluation(w) else {
        return vec![];
    };
    let doc = w.session.visible();
    let component = w.fields.contains_key("component-pattern-pick-direction");
    let (owner, pattern, slot) = if component {
        let Some((owner, p)) = active_component(w) else {
            return vec![];
        };
        (
            owner,
            p.id.as_str(),
            usize::from(
                w.fields
                    .get("component-pattern-pick-direction")
                    .is_some_and(|s| s.ends_with('2')),
            ),
        )
    } else {
        let (Some(owner), Some(pattern)) =
            (w.view.active_part.as_deref(), w.view.active_node.as_deref())
        else {
            return vec![];
        };
        (
            owner,
            pattern,
            usize::from(
                w.fields
                    .get("part-pattern-pick-direction")
                    .is_some_and(|s| s.ends_with('2')),
            ),
        )
    };
    let mut out = vec![];
    let mut instances = e
        .instances
        .iter()
        .filter(|i| crate::viewport::visible(w, i))
        .collect::<Vec<_>>();
    let focus = w
        .edge_selection_path
        .as_ref()
        .or(w.selected_component.as_ref());
    instances.sort_by_key(|i| {
        (
            !focus.is_some_and(|path| i.path.starts_with(path)),
            w.view
                .active_part
                .as_ref()
                .is_some_and(|owner| owner != &i.definition),
        )
    });
    for instance in instances {
        let Some(part) = doc.parts.get(&instance.definition) else {
            continue;
        };
        for feature in part
            .features
            .iter()
            .filter(|f| !f.suppressed && f.id != pattern)
        {
            if !component
                && source_depends_on(
                    doc,
                    &part.reference(&feature.id),
                    &NodeRef {
                        definition: owner.into(),
                        node: pattern.into(),
                    },
                )
            {
                continue;
            }
            let key = format!("{}::{}", cad_core::history::mesh_key(instance), feature.id);
            let mut add = |member: &str, index: usize, start: [f64; 3], end: [f64; 3]| {
                out.push(cad_core::presentation::SemanticElement {id:format!("pattern:{pattern}/sketch-direction/{}/{}/{}",instance.path.join("/"),feature.id,member),role:"pattern-direction-candidate".into(),label:format!("{} / {} · Line {}",instance.name,feature.name,index+1),value:None,state:"available".into(),commands:vec!["pattern.direction-source".into()],relationships:vec![pattern.into(),feature.id.clone(),member.into()],data:serde_json::json!({"contract":"tcad.presentation.pattern-direction","version":1,"document":doc.id,"owner":owner,"pattern":pattern,"component":component,"slot":slot,"source_definition":instance.definition,"source_axis":AxisSource::SketchLine {profile:part.reference(&feature.id),member:member.into()},"occurrence_path":instance.path,"space":"root","origin_mm":start,"end_mm":end,"unit":"mm","evaluation_identity":e.identity,"disposable_display":true})});
            };
            if let Some(sketch) = e.sketches.get(&key)
                && let Some(solved) = &sketch.solved
            {
                for (index, line) in sketch.polylines.iter().enumerate() {
                    if !solved
                        .members
                        .get(&line.member)
                        .is_some_and(|m| matches!(m.curve, cad_core::SketchCurve::Line { .. }))
                    {
                        continue;
                    }
                    if let (Some(start), Some(end)) = (line.points.first(), line.points.last()) {
                        add(
                            &line.member,
                            index,
                            crate::sketch::world(&sketch.plane, &instance.matrix, *start),
                            crate::sketch::world(&sketch.plane, &instance.matrix, *end),
                        );
                    }
                }
            }
            if let Some(sketch) = e.spatial_sketches.get(&key) {
                for (index, (member, line)) in sketch.solved.members.iter().enumerate() {
                    if let cad_core::sketch3d::Curve::Line { start, end } = &line.curve
                        && let (Some(start), Some(end)) = (
                            sketch.solved.points.get(start),
                            sketch.solved.points.get(end),
                        )
                    {
                        add(
                            member,
                            index,
                            matrix_point(instance.matrix, *start),
                            matrix_point(instance.matrix, *end),
                        );
                    }
                }
            }
            if out.len() > 4096 {
                return out;
            }
        }
    }
    out
}
/// Only explicit pattern picks use this projected hit adapter. It stores no display coordinates.
pub fn capture_sketch_pick(w: &mut Workspace, rect: egui::Rect, pointer: egui::Pos2) -> bool {
    let mut candidates = sketch_direction_candidates(w)
        .into_iter()
        .filter_map(|candidate| {
            if !crate::viewport::pattern_candidate_visible(w, &candidate) {
                return None;
            }
            let props = w
                .theme
                .properties_for(&crate::viewport::style_target(w).child(
                    &candidate.id,
                    &candidate.role,
                    &[
                        "viewport-guide".into(),
                        "pattern-direction-candidate".into(),
                    ],
                    &candidate.state,
                ));
            if props.get("display").is_some_and(|s| s == "none")
                || props.get("visibility").is_some_and(|s| s == "hidden")
            {
                return None;
            }
            let radius = crate::presentation_controls::number(&props, "renderer-pick-radius", 8.)
                .clamp(0., 128.);
            let start =
                serde_json::from_value::<[f64; 3]>(candidate.data["origin_mm"].clone()).ok()?;
            let end = serde_json::from_value::<[f64; 3]>(candidate.data["end_mm"].clone()).ok()?;
            let (start, end) = (
                crate::viewport::project(w, rect, start)?,
                crate::viewport::project(w, rect, end)?,
            );
            let delta = end - start;
            let t = if delta.length_sq() > 1e-12 {
                ((pointer - start).dot(delta) / delta.length_sq()).clamp(0., 1.)
            } else {
                0.
            };
            let distance = pointer.distance(start + delta * t);
            (distance <= radius).then_some((distance, candidate))
        })
        .collect::<Vec<_>>();
    candidates.sort_by(|a, b| a.0.total_cmp(&b.0));
    if candidates.len() > 1 && (candidates[0].0 - candidates[1].0).abs() < 0.5 {
        w.status="Several sketch lines overlap here. Choose a named line in Direction reference, or rotate/isolate the source before picking".into();
        return true;
    }
    if let Some((_, candidate)) = candidates.into_iter().next() {
        action(w, "pattern.direction-source", &candidate.data)
    } else {
        false
    }
}
