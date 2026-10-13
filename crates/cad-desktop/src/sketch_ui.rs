//! Selection and dimension presentation over the application-owned sketch contracts.
use crate::{Workspace, presentation_controls as controls, selection_ui, sketch, viewport};
use bevy_egui::egui::{self, Stroke};
use cad_core::{
    sketch_actions::{self as a, DimensionChoice, SketchEntity},
    *,
};

#[derive(Clone)]
pub struct DimensionEditor {
    pub choices: Vec<DimensionChoice>,
    pub choice: usize,
    pub constraint: Id,
    pub source: Sketch,
    pub document: Document,
    pub history_len: usize,
    pub focus_value: bool,
    pub focus_nearby: bool,
    /// A first pick offers its size immediately, while retaining the first
    /// reference for a deliberate second pick.
    pub extend_pick: bool,
}
#[derive(Clone)]
pub struct DimensionCheckpoint {
    pub source: Sketch,
    pub document: Document,
    pub history_len: usize,
}
fn ensure_dimension_checkpoint(w: &mut Workspace) {
    if w.sketch
        .as_ref()
        .is_some_and(|s| s.dimension.is_none() && s.dimension_pick.is_none())
    {
        let document = w.session.visible().clone();
        if let Some(s) = &mut w.sketch {
            s.dimension_pick = Some(DimensionCheckpoint {
                source: sketch::sketch_definition(s),
                document,
                history_len: s.history.len(),
            });
        }
    }
}

/// Read solved member sizes for presentation. Source expressions stay authored
/// in the sketch draft; applying a Diameter or Equal Radius relation changes
/// the evaluated curve, not the disabled source-size text field.
pub fn evaluated_member_values(w: &Workspace) -> std::collections::BTreeMap<(Id, String), String> {
    let Some(s) = &w.sketch else {
        return Default::default();
    };
    let Some(source) = &s.last_valid else {
        return Default::default();
    };
    let doc = w.session.visible();
    source
        .members
        .iter()
        .flat_map(|(key, member)| {
            let fields: Vec<(&str, &Expr, Dimension)> = match &member.curve {
                SketchCurve::Circle { radius, .. } => vec![("radius", radius, Dimension::Length)],
                SketchCurve::Ellipse {
                    major,
                    minor,
                    angle,
                    ..
                } => vec![
                    ("major_radius", major, Dimension::Length),
                    ("minor_radius", minor, Dimension::Length),
                    ("rotation", angle, Dimension::Angle),
                ],
                _ => vec![],
            };
            fields
                .into_iter()
                .map(move |(field, expression, dimension)| {
                    let value = expression::value(doc, &s.definition, expression, dimension)
                        .map(|value| {
                            if dimension == Dimension::Angle {
                                doc.settings.format_angle_radians(value)
                            } else {
                                doc.settings.format_length(value)
                            }
                        })
                        .unwrap_or_else(|error| format!("Unavailable: {error}"));
                    ((key.clone(), field.to_owned()), value)
                })
        })
        .collect()
}

/// Display coordinates in model-selected units without writing a rounded
/// display value back until the user actually edits the field.
pub fn coordinate(
    ui: &mut egui::Ui,
    label: &str,
    millimeters: &mut f64,
    settings: &settings::ModelSettings,
) -> bool {
    let scale = settings.display_length_unit.millimeters_per_unit();
    let mut displayed = *millimeters / scale;
    let response = ui.add(
        egui::DragValue::new(&mut displayed)
            .speed(0.1 / scale)
            .max_decimals(settings.length_decimal_places as usize)
            .prefix(format!("{label} "))
            .suffix(format!(" {}", settings.display_length_unit.symbol())),
    );
    if response.changed() {
        *millimeters = displayed * scale;
        true
    } else {
        false
    }
}

/// Open an inspector once when a different entity is selected. Subsequent
/// frames respect the user's folding choice and retain focused field IDs.
pub fn selected_inspector_open(ui: &egui::Ui, id: &str, selected: Option<&str>) -> Option<bool> {
    let memory_id = ui.id().with((id, "last-selection"));
    let current = selected.unwrap_or_default().to_owned();
    let changed = ui.ctx().data_mut(|data| {
        let previous = data.get_temp::<String>(memory_id);
        data.insert_temp(memory_id, current.clone());
        previous.as_ref() != Some(&current)
    });
    (changed && selected.is_some()).then_some(true)
}
/// Custom layouts target the same sketch entities as the native viewport.
pub fn semantic_action(w: &mut Workspace, element: &presentation::SemanticElement, command: &str) {
    if matches!(
        element.role.as_str(),
        "sketch-dimension-editor" | "sketch-operation-prompt" | "sketch-relation-action"
    ) && element
        .data
        .get("sketch")
        .and_then(serde_json::Value::as_str)
        == w.sketch.as_ref().map(|s| s.sketch_id.as_str())
    {
        action(w, command);
        return;
    }
    let active = w.sketch.as_ref().and_then(|s| {
        entity_for(&sketch::sketch_definition(s), element).and_then(|entity| {
            w.session
                .visible()
                .parts
                .get(&s.definition)?
                .features
                .iter()
                .find(|f| f.id == s.feature)
                .map(|f| (s.definition.clone(), s.sketch_id.clone(), f.clone(), entity))
        })
    });
    let target = active.or_else(|| w.session.visible().parts.values().find_map(|p| {
        p.sketches.values().find_map(|s| {
            let entity = entity_for(s, element)?;
            p.features.iter().find(|f| matches!(&f.operation, Operation::Profile { profile: Profile::Sketch { sketch }, .. } if sketch == &s.id)).map(|f| (p.id.clone(), s.id.clone(), f.clone(), entity))
        })
    }));
    let Some((part, id, feature, entity)) = target else {
        return;
    };
    if w.sketch.as_ref().is_some_and(|s| s.sketch_id != id)
        || (w.sketch.is_none() && w.tool_checkpoint.is_some())
    {
        w.status = "Confirm or cancel the current operation before editing another sketch".into();
        return;
    }
    if w.sketch.is_none() {
        w.view.active_part = Some(part);
        sketch::begin(w, Some(feature));
    }
    let pattern = element
        .data
        .get("operation_id")
        .and_then(serde_json::Value::as_str)
        .filter(|key| {
            w.sketch
                .as_ref()
                .is_some_and(|s| s.transforms.contains_key(*key))
        })
        .map(str::to_string);
    let block_key = element
        .data
        .get("operation_id")
        .and_then(serde_json::Value::as_str)
        .filter(|key| {
            w.sketch.as_ref().is_some_and(|s| {
                s.blocks.contains_key(*key) || s.block_instances.contains_key(*key)
            })
        })
        .map(str::to_string);
    let block_definition = block_key.as_ref().is_some_and(|key| {
        w.sketch
            .as_ref()
            .is_some_and(|s| s.blocks.contains_key(key))
    });
    let dimension_group = element
        .data
        .get("group_id")
        .and_then(serde_json::Value::as_str)
        .filter(|key| {
            w.sketch
                .as_ref()
                .is_some_and(|s| s.dimension_groups.contains_key(*key))
        })
        .map(str::to_string);
    let empty_pattern = pattern.as_ref().is_some_and(|key| {
        w.sketch
            .as_ref()
            .is_some_and(|s| s.transforms[key].sources.is_empty())
    });
    let keep_pair = matches!(
        command,
        "sketch.smart-dimension" | "sketch.reference-dimension"
    ) && w
        .sketch
        .as_ref()
        .is_some_and(|s| s.selection.len() == 2 && s.selection.contains(&entity));
    if !keep_pair {
        select(
            w,
            (!empty_pattern && !block_definition).then_some(entity),
            false,
        );
    }
    match command {
        "sketch.edit-dimension-group" if dimension_group.is_some() => {
            if let Some(key) = dimension_group
                && let Some(kind) = w
                    .sketch
                    .as_ref()
                    .and_then(|s| s.dimension_groups.get(&key))
                    .map(|group| group.kind)
            {
                crate::sketch_edit_ui::begin_dimension_group(w, kind, Some(key));
            }
        }
        "sketch.delete-dimension-group" if dimension_group.is_some() => {
            if let Some(key) = dimension_group {
                crate::sketch_dimensions_ui::delete(w, &key);
            }
        }
        "sketch.block-edit" | "sketch.block-instance" | "sketch.block-insert"
            if block_key.is_some() =>
        {
            let mode = match command {
                "sketch.block-edit" => crate::sketch_blocks_ui::Mode::Definition,
                "sketch.block-instance" => crate::sketch_blocks_ui::Mode::Instance,
                _ => crate::sketch_blocks_ui::Mode::Insert,
            };
            crate::sketch_edit_ui::begin_block(w, mode, block_key);
        }
        "sketch.block-export" if block_key.is_some() => {
            if let Some(key) = block_key {
                crate::sketch_blocks_ui::export(w, &key);
            }
        }
        "sketch.block-explode" if block_key.is_some() => {
            if let Some(key) = block_key {
                explode_block(w, &key);
            }
        }
        "sketch.edit-text"
            if element
                .data
                .get("operation_id")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|key| {
                    w.sketch.as_ref().is_some_and(|s| s.texts.contains_key(key))
                }) =>
        {
            let operation = element.data["operation_id"].as_str().map(str::to_string);
            crate::sketch_edit_ui::begin_text(w, operation);
        }
        "sketch.edit-equation"
            if element
                .data
                .get("operation_id")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|key| {
                    w.sketch
                        .as_ref()
                        .is_some_and(|s| s.equations.contains_key(key))
                }) =>
        {
            crate::sketch_edit_ui::begin_equation(
                w,
                element.data["operation_id"].as_str().map(str::to_string),
            );
        }
        "sketch.detach-conversion" | "sketch.delete-selection" if empty_pattern => {
            if let Some(s) = &mut w.sketch {
                if s.edit.is_some() {
                    w.status = "Accept or cancel the current sketch operation first".into();
                    return;
                }
                let before = sketch::sketch_definition(s);
                if let Some(pattern) = pattern {
                    s.history.push(before);
                    s.transforms.remove(&pattern);
                }
            }
            sketch::solve(w);
        }
        "sketch.mirror"
        | "sketch.dynamic-mirror"
        | "sketch.linear-pattern"
        | "sketch.circular-pattern"
        | "sketch.move-copy"
        | "sketch.instance-controls"
            if pattern.is_some() =>
        {
            if let Some(pattern) = pattern {
                crate::sketch_edit_ui::begin_pattern(w, pattern);
            }
        }
        "sketch.select-handle" => action(w, "sketch.handles"),
        "sketch.edit-constraint" => {
            if w.sketch.as_ref().is_some_and(|s| {
                s.constraints
                    .iter()
                    .any(|c| c.id == element.id && a::is_dimension(&c.kind))
            }) {
                smart_dimension(w);
            }
        }
        "sketch.delete-constraint" => action(w, "sketch.delete-selection"),
        _ if command.starts_with("sketch.")
            && command != "sketch.edit-member"
            && command != "sketch.select-entity" =>
        {
            action(w, command)
        }
        _ => {}
    }
    w.manager_tree = false;
}
fn entity_for(source: &Sketch, element: &presentation::SemanticElement) -> Option<SketchEntity> {
    Some(match element.role.as_str() {
        "sketch-equation" | "sketch-equation-field"
            if element
                .data
                .get("sketch")
                .and_then(serde_json::Value::as_str)
                == Some(&source.id) =>
        {
            let member = element.data.get("member")?.as_str()?;
            if !source.equations.contains_key(member) {
                return None;
            }
            SketchEntity::Member {
                member: member.into(),
            }
        }
        "sketch-dimension-group"
            if element
                .data
                .get("sketch")
                .and_then(serde_json::Value::as_str)
                == Some(&source.id) =>
        {
            let group = source
                .dimension_groups
                .get(element.data.get("group_id")?.as_str()?)?;
            SketchEntity::Constraint {
                constraint: cad_core::sketch_dimensions::selection_constraint(source, group)?
                    .clone(),
            }
        }
        "sketch-handle"
            if element
                .data
                .get("sketch")
                .and_then(serde_json::Value::as_str)
                == Some(&source.id) =>
        {
            let handle: cad_core::sketch_handles::SketchHandle =
                serde_json::from_value(element.data.get("handle")?.clone()).ok()?;
            handle.target.entity()
        }
        "sketch-modifier" | "sketch-instance" => {
            if let Some(member) = element
                .data
                .get("member")
                .and_then(serde_json::Value::as_str)
                .filter(|member| source.members.contains_key(*member))
            {
                SketchEntity::Member {
                    member: member.to_string(),
                }
            } else if element.relationships.contains(&source.id)
                && element
                    .data
                    .get("operation_id")
                    .and_then(serde_json::Value::as_str)
                    .is_some_and(|key| {
                        source.blocks.contains_key(key)
                            || source.block_instances.contains_key(key)
                            || source.transforms.get(key).is_some_and(|pattern| {
                                pattern.dynamic && pattern.sources.is_empty()
                            })
                    })
            {
                // An empty dynamic mirror has a saved operation but no selectable
                // output yet. The caller opens its operation without selecting an axis.
                SketchEntity::XAxis
            } else {
                return None;
            }
        }
        "sketch-member" if source.members.contains_key(&element.id) => SketchEntity::Member {
            member: element.id.clone(),
        },
        "sketch-point" if source.points.contains_key(&element.id) => SketchEntity::Point {
            point: element.id.clone(),
        },
        "sketch-constraint" if source.constraints.iter().any(|c| c.id == element.id) => {
            SketchEntity::Constraint {
                constraint: element.id.clone(),
            }
        }
        "sketch-reference" | "sketch-reference-action"
            if element
                .data
                .get("sketch")
                .and_then(serde_json::Value::as_str)
                == Some(&source.id) =>
        {
            match element
                .data
                .get("reference")
                .and_then(serde_json::Value::as_str)
            {
                Some(a::ORIGIN) => SketchEntity::Origin,
                Some(a::X_AXIS) => SketchEntity::XAxis,
                Some(a::Y_AXIS) => SketchEntity::YAxis,
                _ => return None,
            }
        }
        _ => return None,
    })
}
pub fn select(w: &mut Workspace, entity: Option<SketchEntity>, additive: bool) {
    crate::sketch_relations_ui::end_edit(w);
    w.selection_popup = None;
    w.context_selection = None;
    w.selected = None;
    w.selected_feature_path = None;
    w.candidates.clear();
    crate::navigation::stop(w);
    let dimension_mode = w.fields.contains_key("smart-dimension-mode");
    if let Some(s) = &mut w.sketch {
        let repeat_pick = dimension_mode
            && s.dimension.is_some()
            && entity
                .as_ref()
                .is_some_and(|entity| s.selection.contains(entity));
        if repeat_pick {
            if let Some(editor) = &mut s.dimension {
                editor.focus_value = true;
                editor.focus_nearby = true;
            }
            return;
        }
        if dimension_mode
            && entity.as_ref().is_some_and(|entity| {
                s.dimension.as_ref().is_some_and(|editor| {
                    !matches!(entity,SketchEntity::Constraint {constraint} if constraint==&editor.constraint)
                })
            })
        {
            if s.dimension.as_ref().is_some_and(|editor| editor.extend_pick)
                && s.selection.len() == 1
                && !matches!(entity, Some(SketchEntity::Constraint { .. }))
            {
                discard_first_pick_dimension(s);
            } else {
                // A completed reference pair starts a fresh dimension.
                s.dimension = None;
                s.dimension_pick = None;
                s.selection.clear();
            }
        }
        let additive = if dimension_mode {
            entity.is_some() && !s.selection.is_empty() && s.selection.len() < 2
        } else {
            additive
        };
        if !additive {
            s.selection.clear();
            s.dimension_model = None;
        }
        if let Some(entity) = entity {
            if additive && s.selection.contains(&entity) {
                s.selection.retain(|e| e != &entity);
            } else if !s.selection.contains(&entity) {
                s.selection.push(entity.clone());
            }
        }
        s.selected_member = s.selection.iter().rev().find_map(|entity| match entity {
            SketchEntity::Member { member } => Some(member.clone()),
            _ => None,
        });
        s.selected = s.selection.iter().rev().find_map(|entity| match entity {
            SketchEntity::Point { point } => s.keys.iter().position(|key| key == point),
            _ => None,
        });
    }
}
fn discard_first_pick_dimension(s: &mut sketch::EditingSketch) {
    let Some(editor) = s.dimension.take() else {
        return;
    };
    // A type change may have replaced an existing constraint; restore the
    // complete pre-pick constraint set while retaining the current geometry.
    s.constraints = editor.source.constraints.clone();
    s.history.truncate(editor.history_len);
    s.dimension_pick = Some(DimensionCheckpoint {
        source: editor.source,
        document: editor.document,
        history_len: editor.history_len,
    });
}
fn attach_dimension(s: &mut sketch::EditingSketch, choice: &DimensionChoice, driven: bool) -> Id {
    if let Some(c) = s.constraints.iter_mut().find(|c| {
        c.kind == choice.kind
            && (c.entities == choice.entities
                || (c.entities.len() == 2 && c.entities.iter().rev().eq(choice.entities.iter())))
    }) {
        c.driven = driven;
        if c.distance_side.is_none() {
            c.distance_side = choice.distance_side;
        }
        return c.id.clone();
    }
    let id = id();
    s.constraints.push(SketchConstraint {
        driven,
        distance_side: choice.distance_side,
        contact: None,
        id: id.clone(),
        kind: choice.kind.clone(),
        entities: choice.entities.clone(),
        value: choice.value.clone(),
    });
    id
}
pub fn smart_dimension(w: &mut Workspace) {
    smart_dimension_mode(w, false);
}
fn smart_dimension_mode(w: &mut Workspace, driven: bool) {
    open_dimension(w, driven, false);
}
fn open_dimension(w: &mut Workspace, driven: bool, extend_pick: bool) {
    if let Some(editor) = w.sketch.as_mut().and_then(|s| s.dimension.as_mut()) {
        editor.focus_value = true;
        editor.focus_nearby = true;
        return;
    }
    sketch::cancel_pending_chain(w);
    ensure_dimension_checkpoint(w);
    if let Some(selected) = w.sketch.as_mut().and_then(|s| s.dimension_model.take()) {
        match model_edge_dimension_pick(w, &selected) {
            Ok(entity) => select(w, Some(entity), true),
            Err(error) => {
                w.status = error.to_string();
                return;
            }
        }
    }
    let Some(s) = &w.sketch else { return };
    if let Some(key) =
        cad_core::sketch_dimensions::selected_group(&sketch::sketch_definition(s), &s.selection)
    {
        let kind = s.dimension_groups[&key].kind;
        if let Some(s) = &mut w.sketch {
            s.dimension_pick = None;
        }
        crate::sketch_edit_ui::begin_dimension_group(w, kind, Some(key));
        return;
    }
    let driven = s
        .selection
        .iter()
        .find_map(|entity| {
            if let SketchEntity::Constraint { constraint } = entity {
                s.constraints
                    .iter()
                    .find(|c| &c.id == constraint)
                    .map(|c| c.driven)
            } else {
                None
            }
        })
        .unwrap_or(driven);
    w.selection_popup = None;
    w.context_selection = None;
    w.fields.insert(
        "smart-dimension-mode".into(),
        if driven { "reference" } else { "on" }.into(),
    );
    if s.selection.is_empty() {
        if let Some(s) = &mut w.sketch {
            s.tool = sketch::Tool::Select;
            s.chain.clear();
            s.clicks.clear();
        }
        w.status = "Smart Dimension: pick a member for its size, or a center / point then an edge for a distance. Enter a value, choose a second reference, or use Done Dimension.".into();
        return;
    }
    if s.selection.len() == 1
        && matches!(
            s.selection[0],
            SketchEntity::Origin | SketchEntity::XAxis | SketchEntity::YAxis
        )
    {
        if let Some(s) = &mut w.sketch {
            s.tool = sketch::Tool::Select;
            s.chain.clear();
            s.clicks.clear();
        }
        w.status = "Smart Dimension: reference selected. Pick the sketch point or member to dimension from it.".into();
        return;
    }
    let choices = a::dimensions(
        w.session.visible(),
        &s.definition,
        &sketch::sketch_definition(s),
        &s.selection,
    );
    match choices {
        Ok(choices) if !choices.is_empty() => {
            if let Some(s) = &mut w.sketch {
                let checkpoint = s
                    .dimension_pick
                    .take()
                    .expect("Dimension pick checkpoint exists");
                let source = checkpoint.source;
                let document = checkpoint.document;
                let history_len = checkpoint.history_len;
                s.history.push(source.clone());
                let constraint = attach_dimension(s, &choices[0], driven);
                s.dimension = Some(DimensionEditor {
                    choices,
                    choice: 0,
                    constraint,
                    source,
                    document,
                    history_len,
                    focus_value: true,
                    focus_nearby: true,
                    extend_pick: extend_pick
                        || (s.selection.len() == 1
                            && !matches!(s.selection[0], SketchEntity::Constraint { .. })),
                });
                s.tool = sketch::Tool::Select;
                s.chain.clear();
                s.clicks.clear();
            }
            w.manager_tree = false;
            w.status="Enter the dimension beside the geometry or in the docked field. Edits preview live; Done Dimension retains it, Cancel Dimension restores its start, Finish Sketch commits.".into();
            sketch::solve(w);
        }
        Err(e) => w.status = e.to_string(),
        _ => {}
    }
}
pub fn selection_changed(w: &mut Workspace) {
    if !w.fields.contains_key("smart-dimension-mode") {
        return;
    }
    let driven = w
        .fields
        .get("smart-dimension-mode")
        .is_some_and(|mode| mode == "reference");
    let first_pick = w.sketch.as_ref().is_some_and(|s| {
        s.selection.len() == 1 && !matches!(s.selection[0], SketchEntity::Constraint { .. })
    });
    open_dimension(w, driven, first_pick);
}
pub fn cancel_dimension(w: &mut Workspace) -> bool {
    let checkpoint = w.sketch.as_mut().and_then(|s| {
        s.dimension
            .take()
            .map(|editor| DimensionCheckpoint {
                source: editor.source,
                document: editor.document,
                history_len: editor.history_len,
            })
            .or_else(|| s.dimension_pick.take())
    });
    let Some(checkpoint) = checkpoint else {
        return false;
    };
    if let Some(s) = &mut w.sketch {
        sketch::restore(s, checkpoint.source);
        s.history.truncate(checkpoint.history_len);
    }
    w.dispatch(commands::Command::ReplaceDocument {
        document: checkpoint.document,
    });
    w.fields.remove("smart-dimension-mode");
    w.selection_popup = None;
    w.context_selection = None;
    sketch::solve(w);
    w.status = "Dimension canceled; earlier sketch edits retained".into();
    true
}
pub fn finish_dimension(w: &mut Workspace) {
    if w.sketch
        .as_ref()
        .is_some_and(|sketch| sketch.dimension.is_some())
    {
        sketch::solve(w);
    }
    if let Some(error) = w.sketch.as_ref().and_then(|sketch| sketch.error.as_ref()) {
        w.status = format!("Correct the dimension before finishing: {error}");
        return;
    }
    if let Some(s) = &mut w.sketch {
        s.dimension = None;
        s.dimension_pick = None;
        s.dimension_model = None;
        s.selection.clear();
    }
    w.selection_popup = None;
    w.context_selection = None;
    w.status = "Dimension retained in the sketch draft. Pick the next member or reference pair; Finish Sketch commits the validated sketch.".into();
}
/// Transient editor state is semantic data; layouts need not infer pick state
/// from viewport styling or docked widgets.
pub fn dimension_semantics(w: &Workspace) -> Vec<presentation::SemanticElement> {
    let Some(s) = &w.sketch else {
        return vec![];
    };
    let mut elements = [
        ("relations-section", "Dimensions & Relations"),
        ("tool-help", "Tool help"),
        ("member-list", "Sketch members"),
    ].into_iter().map(|(suffix, label)| presentation::SemanticElement {
        id: format!("{}::{suffix}", s.sketch_id),
        role: if suffix == "member-list" { "sketch-member-list" } else { "sketch-secondary-section" }.into(),
        label: label.into(), value: None, state: "ready".into(), commands: vec![],
        relationships: vec![s.sketch_id.clone()],
        data: serde_json::json!({"sketch":s.sketch_id,"presentation_only":true,"default_collapsed":suffix != "member-list","layout_order":"active-tool-before-secondary-editors"}),
    }).collect::<Vec<_>>();
    let source = sketch::sketch_definition(s);
    for (label, entity) in [
        ("Origin", SketchEntity::Origin),
        ("X Axis", SketchEntity::XAxis),
        ("Y Axis", SketchEntity::YAxis),
    ] {
        elements.push(presentation::SemanticElement {
            id: format!("{}::reference-action.{}", s.sketch_id, entity.key()),
            role: "sketch-reference-action".into(),
            label: label.into(),
            value: None,
            state: if s.selection.contains(&entity) { "selected" } else { "ready" }.into(),
            commands: vec!["sketch.select-entity".into()],
            relationships: vec![s.sketch_id.clone()],
            data: serde_json::json!({"sketch":s.sketch_id,"reference":entity.key(),"classes":["modeling-action","compact"],"accessibility_label":format!("Select sketch {label}")}),
        });
    }
    for (kind, label) in a::RELATIONS {
        let availability = a::relation(&source, &s.selection, kind);
        if availability.is_ok() || *kind == "tangent" {
            let reason = availability.as_ref().err().map(ToString::to_string);
            elements.push(presentation::SemanticElement {
                id: format!("{}::relation-action.{kind}", s.sketch_id),
                role: "sketch-relation-action".into(),
                label: (*label).into(),
                value: reason.clone(),
                state: if availability.is_ok() { "ready" } else { "disabled" }.into(),
                commands: vec![format!("sketch.relation.{kind}")],
                relationships: vec![s.sketch_id.clone()],
                data: serde_json::json!({"sketch":s.sketch_id,"selection_count":s.selection.len(),"description":a::relation_description(kind),"disabled_reason":reason,"classes":["modeling-action","compact"],"accessibility_label":label}),
            });
        }
    }
    if let Some(member) = &s.selected_member {
        for ((key, field), value) in evaluated_member_values(w)
            .into_iter()
            .filter(|((key, _), _)| key == member)
        {
            elements.push(presentation::SemanticElement {
                id: format!("{key}::{field}.evaluated"), role: "sketch-evaluated-size".into(),
                label: a::label(&field).into(), value: Some(value),
                state: if s.error.is_some() { "last-valid" } else { "evaluated" }.into(),
                commands: vec![], relationships: vec![s.sketch_id.clone(), key.clone()],
                data: serde_json::json!({"sketch":s.sketch_id,"member":key,"field":field,"editable":false,"geometry":"last-successful-solve","display_length_unit":w.session.visible().settings.display_length_unit,"display_length_precision":w.session.visible().settings.length_decimal_places,"display_angle_precision":w.session.visible().settings.angle_decimal_places,"authored_expressions":"advanced-source"}),
            });
        }
    }
    if !w.fields.contains_key("smart-dimension-mode") && s.dimension.is_none() {
        return elements;
    }
    let constraint = s
        .dimension
        .as_ref()
        .and_then(|editor| s.constraints.iter().find(|c| c.id == editor.constraint));
    elements.push(presentation::SemanticElement {
        id: format!("{}::smart-dimension", s.sketch_id),
        role: "sketch-dimension-editor".into(),
        label: "Smart Dimension".into(),
        value: constraint.map(|c| c.value.0.clone()),
        state: if constraint.is_some() {
            "value-entry"
        } else {
            "pick-entities"
        }
        .into(),
        commands: vec![
            "sketch.smart-dimension".into(),
            "sketch.reference-dimension".into(),
            "sketch.finish-dimension".into(),
            "sketch.cancel-dimension".into(),
        ],
        relationships: std::iter::once(s.sketch_id.clone())
            .chain(s.selection.iter().map(|entity| entity.key().to_string()))
            .collect(),
        data: serde_json::json!({"definition":s.definition,"sketch":s.sketch_id,"selection":s.selection,"pending_model_edge":s.dimension_model.as_ref().map(|selected|serde_json::json!({"body":selected.body,"reference":selected.reference,"path":selected.path})),"constraint":constraint,"mode":w.fields.get("smart-dimension-mode"),"unit":constraint.map(|c|a::dimension_unit(&c.kind)),"perpendicular_distance":constraint.is_some_and(|c|c.kind=="point_line_distance"),"current_error":s.error,"shared_draft":true,"requires_current_validation":true,"accepts_second_reference":s.dimension.as_ref().is_some_and(|editor|editor.extend_pick),"completion_label":"Done Dimension","completion_scope":"dimension","commit_command":"sketch.finish","actions":[{"command":"sketch.finish-dimension","label":"Done Dimension","enabled":constraint.is_some() && s.error.is_none(),"disabled_reason":"Enter a valid dimension before finishing"},{"command":"sketch.cancel-dimension","label":"Cancel Dimension","enabled":true}]}),
    });
    if let Some(constraint) = constraint {
        elements.push(dimension_value_element(w, s, constraint));
    }
    elements
}
fn dimension_value_element(
    w: &Workspace,
    sketch: &sketch::EditingSketch,
    constraint: &SketchConstraint,
) -> presentation::SemanticElement {
    presentation::SemanticElement {
        id: format!("{}::nearby-dimension-value", sketch.sketch_id),
        role: "sketch-dimension-value".into(),
        label: a::label(&constraint.kind).into(),
        value: Some(constraint.value.0.clone()),
        state: if sketch.error.is_some() {
            "error"
        } else {
            "draft"
        }
        .into(),
        commands: vec![],
        relationships: vec![sketch.sketch_id.clone(), constraint.id.clone()],
        data: serde_json::json!({"contract":"tcad.presentation.field","version":1,"document":w.session.visible().id,"definition":sketch.definition,"sketch":sketch.sketch_id,"setter":"sketch-dimension","constraint":constraint.id,"field":"value","json":constraint.value.0,"editable":!constraint.driven,"kind":"expression","unit":a::dimension_unit(&constraint.kind),"diagnostics":sketch.error.iter().collect::<Vec<_>>(),"accessibility_label":format!("{} expression", a::label(&constraint.kind)),"classes":["dimension-value","viewport-overlay"]}),
    }
}
/// Transient model-edge preselection has its own overlay and never replaces
/// global model selection. Source restoration clears this feedback with it.
pub fn model_dimension_highlight(w: &Workspace) -> Vec<[f64; 3]> {
    let Some(selected) = w.sketch.as_ref().and_then(|s| s.dimension_model.as_ref()) else {
        return vec![];
    };
    let Some(evaluated) =
        viewport::display_evaluation(w).filter(|e| e.valid && e.identity == w.session.identity())
    else {
        return vec![];
    };
    let Some(instance) = evaluated.instances.iter().find(|instance| {
        instance.path == selected.path && instance.definition == selected.definition
    }) else {
        return vec![];
    };
    let key = if instance.mesh_key.is_empty() {
        &instance.definition
    } else {
        &instance.mesh_key
    };
    let mut edges = evaluated
        .meshes
        .get(key)
        .into_iter()
        .flat_map(|mesh| &mesh.edges)
        .filter(|edge| {
            edge.body == selected.body
                && edge.reference.producer == selected.reference.producer
                && edge.reference.role == selected.reference.role
                && edge.reference.topology == "edge"
        });
    let Some(edge) = edges.next() else {
        return vec![];
    };
    if edges.next().is_some() {
        return vec![];
    }
    edge.points
        .iter()
        .map(|point| viewport::placed(*point, instance))
        .collect()
}
/// Save an exact associative construction line from a current native model edge.
pub fn model_edge_dimension_pick(
    w: &mut Workspace,
    selected: &viewport::Selection,
) -> cad_core::Result<SketchEntity> {
    let s = w.sketch.as_ref().ok_or_else(|| {
        CadError::Invalid("Open a sketch before dimensioning a model edge".into())
    })?;
    if selected.definition != s.definition || selected.reference.topology != "edge" {
        return Err(CadError::Invalid(
            "Pick a straight edge on the active sketch occurrence".into(),
        ));
    }
    let evaluated = viewport::display_evaluation(w)
        .filter(|e| e.valid && e.identity == w.session.identity())
        .ok_or_else(|| CadError::Invalid("Wait for the current validated model geometry".into()))?;
    let instance = evaluated
        .instances
        .iter()
        .find(|i| i.path == selected.path && i.definition == selected.definition)
        .ok_or_else(|| CadError::Invalid("The model occurrence is unavailable".into()))?;
    if instance.matrix != s.matrix
        || w.view
            .active_instance
            .as_ref()
            .is_some_and(|id| !selected.path.contains(id))
    {
        return Err(CadError::Invalid(
            "Pick the same part occurrence that owns the active sketch".into(),
        ));
    }
    let mesh_key = if instance.mesh_key.is_empty() {
        &instance.definition
    } else {
        &instance.mesh_key
    };
    let edges = evaluated
        .meshes
        .get(mesh_key)
        .into_iter()
        .flat_map(|mesh| &mesh.edges)
        .filter(|edge| {
            edge.body == selected.body
                && edge.reference.producer == selected.reference.producer
                && edge.reference.role == selected.reference.role
                && edge.reference.topology == "edge"
        })
        .collect::<Vec<_>>();
    if edges.len() != 1 {
        return Err(CadError::Invalid(
            "The selected edge has no unique current native owner".into(),
        ));
    }
    let edge = edges[0];
    let body = edge
        .body
        .clone()
        .ok_or_else(|| CadError::Invalid("The selected edge has no stable output owner".into()))?;
    let part = w
        .session
        .visible()
        .parts
        .get(&s.definition)
        .ok_or_else(|| CadError::Invalid("Active sketch part is missing".into()))?;
    let sketch_index = part
        .features
        .iter()
        .position(|feature| feature.id == s.feature)
        .unwrap_or(part.features.len());
    if body.definition != s.definition
        || part
            .features
            .iter()
            .position(|feature| feature.id == body.node)
            .is_none_or(|index| index >= sketch_index)
    {
        return Err(CadError::Invalid(
            "A dimension reference must come from a feature before this sketch".into(),
        ));
    }
    let curve = edge.curve.as_ref().ok_or_else(|| {
        CadError::Unsupported("This model edge has no exact convertible curve".into())
    })?;
    if curve.degree != 1 || curve.control_points.len() != 2 {
        return Err(CadError::Unsupported(
            "Center-to-edge dimensions currently require an exact straight model edge".into(),
        ));
    }
    let curve = cad_core::sketch_edit::project(curve, &s.plane)?;
    if (curve.control_points[1][0] - curve.control_points[0][0])
        .hypot(curve.control_points[1][1] - curve.control_points[0][1])
        < 1e-9
    {
        return Err(CadError::Invalid(
            "This edge projects to a point on the sketch plane".into(),
        ));
    }
    let owner = s.definition.clone();
    let sketch_id = s.sketch_id.clone();
    let reference = edge.reference.clone();
    let extending = s
        .dimension
        .as_ref()
        .is_some_and(|editor| editor.extend_pick);
    let mut source = sketch::sketch_definition(s);
    if extending && let Some(editor) = &s.dimension {
        source.constraints = editor.source.constraints.clone();
    }
    let existing = source
        .conversions
        .iter()
        .find(|(key, conversion)| {
            conversion.construction_orientation
                && conversion.body == body
                && conversion.reference.producer == reference.producer
                && conversion.reference.role == reference.role
                && matches!(
                    source.members.get(*key).map(|member| &member.curve),
                    Some(SketchCurve::Line { .. })
                )
        })
        .map(|(key, _)| key.clone());
    let member = if let Some(key) = existing {
        key
    } else {
        cad_core::sketch_edit::insert_conversion_independent(
            &mut source,
            curve,
            Some((body, reference)),
            true,
        )?
    };
    let mut document = w.session.visible().clone();
    document
        .parts
        .get_mut(&owner)
        .ok_or_else(|| CadError::Invalid("Active sketch part is missing".into()))?
        .sketches
        .insert(sketch_id, source.clone());
    // The normal source admission rejects downstream / cyclic model references.
    document.validate()?;
    if let Some(s) = &mut w.sketch {
        if extending {
            discard_first_pick_dimension(s);
        } else if s.dimension.take().is_some() {
            s.dimension_pick = None;
            s.selection.clear();
        }
    }
    ensure_dimension_checkpoint(w);
    if let Some(s) = &mut w.sketch {
        let checkpoint = s.dimension_pick.take();
        let selection = s.selection.clone();
        sketch::restore(s, source);
        s.dimension_pick = checkpoint;
        s.selection = selection;
    }
    Ok(SketchEntity::Member { member })
}
pub fn action(w: &mut Workspace, command: &str) {
    if w.sketch.as_ref().is_some_and(|s| s.edit.is_some())
        && !matches!(
            command,
            "sketch.cancel"
                | "sketch.finish"
                | "sketch.normal-to"
                | "sketch.accept-edit"
                | "sketch.cancel-edit"
        )
    {
        w.status = "Accept or cancel the current sketch operation first".into();
        return;
    }
    crate::sketch_handles::end(w);
    if !matches!(
        command,
        "sketch.smart-dimension"
            | "sketch.reference-dimension"
            | "sketch.finish-dimension"
            | "sketch.cancel-dimension"
            | "sketch.cancel"
            | "sketch.finish"
            | "sketch.normal-to"
    ) {
        if let Some(s) = &mut w.sketch {
            s.dimension = None;
            s.dimension_pick = None;
            s.dimension_model = None;
        }
        w.fields.remove("smart-dimension-mode");
    }
    match command {
        "sketch.finish-dimension" => finish_dimension(w),
        "sketch.cancel-dimension" => {
            if !cancel_dimension(w) {
                w.fields.remove("smart-dimension-mode");
                select(w, None, false);
                w.status =
                    "Smart Dimension pick mode canceled; earlier sketch previews retained".into();
            }
        }
        "sketch.equation" => crate::sketch_edit_ui::begin_equation(w, None),
        "sketch.edit-equation" => {
            let key = w.sketch.as_ref().and_then(|s| {
                s.selection.iter().find_map(|entity| match entity {
                    SketchEntity::Member { member } => {
                        s.equations.contains_key(member).then(|| member.clone())
                    }
                    SketchEntity::Point { point } => s
                        .equations
                        .iter()
                        .find(|(_, definition)| &definition.anchor == point)
                        .map(|(key, _)| key.clone()),
                    _ => None,
                })
            });
            if let Some(key) = key {
                crate::sketch_edit_ui::begin_equation(w, Some(key));
            } else {
                w.status = "Select an equation curve or its source anchor".into();
            }
        }
        "sketch.ordinate-dimension" | "sketch.baseline-dimension" | "sketch.chain-dimension" => {
            let kind = match command {
                "sketch.ordinate-dimension" => cad_core::sketch_dimensions::Kind::Ordinate,
                "sketch.baseline-dimension" => cad_core::sketch_dimensions::Kind::Baseline,
                _ => cad_core::sketch_dimensions::Kind::Chain,
            };
            crate::sketch_edit_ui::begin_dimension_group(w, kind, None);
        }
        "sketch.edit-dimension-group" | "sketch.delete-dimension-group" => {
            let key = w.sketch.as_ref().and_then(|s| {
                cad_core::sketch_dimensions::selected_group(
                    &sketch::sketch_definition(s),
                    &s.selection,
                )
            });
            if let Some(key) = key {
                if command == "sketch.delete-dimension-group" {
                    crate::sketch_dimensions_ui::delete(w, &key);
                } else if let Some(kind) = w
                    .sketch
                    .as_ref()
                    .and_then(|s| s.dimension_groups.get(&key))
                    .map(|group| group.kind)
                {
                    crate::sketch_edit_ui::begin_dimension_group(w, kind, Some(key));
                }
            } else {
                w.status = "Select an ordinate, baseline or chain dimension annotation".into();
            }
        }
        "sketch.block-create" => {
            crate::sketch_edit_ui::begin_block(w, crate::sketch_blocks_ui::Mode::Create, None)
        }
        "sketch.block-insert" => {
            crate::sketch_edit_ui::begin_block(w, crate::sketch_blocks_ui::Mode::Insert, None)
        }
        "sketch.block-import" => {
            crate::sketch_edit_ui::begin_block(w, crate::sketch_blocks_ui::Mode::Import, None)
        }
        "sketch.block-export" => {
            let key = w
                .sketch
                .as_ref()
                .and_then(|s| selected_block(&sketch::sketch_definition(s), &s.selection));
            if let Some(key) = key {
                crate::sketch_blocks_ui::export(w, &key);
            } else {
                w.status = "Select a block outline or its insertion anchor to export".into();
            }
        }
        "sketch.block-edit" | "sketch.block-instance" | "sketch.block-explode" => {
            let key = w
                .sketch
                .as_ref()
                .and_then(|s| selected_block(&sketch::sketch_definition(s), &s.selection));
            if let Some(key) = key {
                if command == "sketch.block-explode" {
                    explode_block(w, &key);
                } else {
                    let mode = if command == "sketch.block-edit" {
                        crate::sketch_blocks_ui::Mode::Definition
                    } else {
                        crate::sketch_blocks_ui::Mode::Instance
                    };
                    crate::sketch_edit_ui::begin_block(w, mode, Some(key));
                }
            } else {
                w.status = "Select a block outline or its insertion anchor".into();
            }
        }
        "sketch.text" => crate::sketch_edit_ui::begin_text(w, None),
        "sketch.edit-text" => {
            let key = w.sketch.as_ref().and_then(|s| {
                s.selection.iter().find_map(|entity| {
                    if let SketchEntity::Member { member } = entity {
                        cad_core::sketch_text::operation_for(&sketch::sketch_definition(s), member)
                            .map(|(key, _)| key.clone())
                    } else if let SketchEntity::Point { point } = entity {
                        s.texts
                            .iter()
                            .find(|(_, definition)| &definition.anchor == point)
                            .map(|(key, _)| key.clone())
                    } else {
                        None
                    }
                })
            });
            if let Some(key) = key {
                crate::sketch_edit_ui::begin_text(w, Some(key));
            } else {
                w.status = "Select a text outline or its insertion point to edit".into();
            }
        }
        "sketch.accept-edit" => crate::sketch_edit_ui::accept_operation(w),
        "sketch.cancel-edit" => crate::sketch_edit_ui::cancel(w),
        "sketch.handles" => {
            sketch::cancel_pending_chain(w);
            if let Some(s) = &mut w.sketch {
                s.tool = sketch::Tool::Select;
                s.dimension = None;
            }
            w.fields.remove("smart-dimension-mode");
            w.manager_tree = false;
            w.status = "Drag a point, center, control, size or move handle. Constraints remain active; Alt bypasses grid snap.".into();
        }
        "sketch.convert-entities" => {
            crate::sketch_edit_ui::begin(w, crate::sketch_edit_ui::Kind::Convert)
        }
        "sketch.offset-entities" => {
            crate::sketch_edit_ui::begin(w, crate::sketch_edit_ui::Kind::Offset)
        }
        "sketch.fillet" => crate::sketch_edit_ui::begin(w, crate::sketch_edit_ui::Kind::Fillet),
        "sketch.repair" => crate::sketch_edit_ui::begin(w, crate::sketch_edit_ui::Kind::Repair),
        "sketch.chamfer" => crate::sketch_edit_ui::begin(w, crate::sketch_edit_ui::Kind::Chamfer),
        "sketch.mirror" => crate::sketch_edit_ui::begin(w, crate::sketch_edit_ui::Kind::Mirror),
        "sketch.dynamic-mirror" => {
            crate::sketch_edit_ui::begin(w, crate::sketch_edit_ui::Kind::DynamicMirror)
        }
        "sketch.stop-dynamic-mirror" => {
            if let Some(s) = &mut w.sketch {
                let before = sketch::sketch_definition(s);
                let mut source = before.clone();
                if cad_core::sketch_transform::stop_dynamic(&mut source) {
                    s.history.push(before);
                    sketch::restore(s, source);
                }
            }
            sketch::solve(w);
        }
        "sketch.stretch" => crate::sketch_edit_ui::begin(w, crate::sketch_edit_ui::Kind::Stretch),
        "sketch.replace-entities" => {
            crate::sketch_edit_ui::begin(w, crate::sketch_edit_ui::Kind::Replace)
        }
        "sketch.instance-controls" => {
            let operation = w.sketch.as_ref().and_then(|s| {
                s.selection.iter().find_map(|entity| {
                    if let SketchEntity::Member { member } = entity {
                        cad_core::sketch_transform::instance_for_member(
                            &sketch::sketch_definition(s),
                            member,
                        )
                        .map(|(key, _)| key.clone())
                    } else {
                        None
                    }
                })
            });
            if let Some(operation) = operation {
                crate::sketch_edit_ui::begin_pattern(w, operation);
            } else {
                w.status = "Select a mirror or pattern instance to control".into();
            }
        }
        "sketch.linear-pattern" => {
            crate::sketch_edit_ui::begin(w, crate::sketch_edit_ui::Kind::LinearPattern)
        }
        "sketch.circular-pattern" => {
            crate::sketch_edit_ui::begin(w, crate::sketch_edit_ui::Kind::CircularPattern)
        }
        "sketch.move-copy" => {
            crate::sketch_edit_ui::begin(w, crate::sketch_edit_ui::Kind::MoveCopy)
        }
        "sketch.rotate" => crate::sketch_edit_ui::begin(w, crate::sketch_edit_ui::Kind::Rotate),
        "sketch.scale" => crate::sketch_edit_ui::begin(w, crate::sketch_edit_ui::Kind::Scale),
        "sketch.trim" => crate::sketch_edit_ui::begin(w, crate::sketch_edit_ui::Kind::Trim),
        "sketch.extend" => crate::sketch_edit_ui::begin(w, crate::sketch_edit_ui::Kind::Extend),
        "sketch.split" => crate::sketch_edit_ui::begin(w, crate::sketch_edit_ui::Kind::Split),
        "sketch.edit-spline" => {
            crate::sketch_edit_ui::begin(w, crate::sketch_edit_ui::Kind::Spline)
        }
        "sketch.edit-conic" => crate::sketch_edit_ui::begin(w, crate::sketch_edit_ui::Kind::Conic),
        "sketch.smart-dimension" => smart_dimension(w),
        "sketch.reference-dimension" => smart_dimension_mode(w, true),
        "sketch.toggle-dimension-mode" => {
            if let Some(s) = &mut w.sketch {
                s.history.push(sketch::sketch_definition(s));
                for c in &mut s.constraints {
                    if a::is_dimension(&c.kind) && s.selection.iter().any(|e| matches!(e, SketchEntity::Constraint{constraint} if constraint==&c.id)) {c.driven = !c.driven;}
                }
            }
            sketch::solve(w);
        }
        "sketch.relations" => {
            sketch::cancel_pending_chain(w);
            if let Some(s) = &mut w.sketch {
                s.tool = sketch::Tool::Select;
            }
            w.manager_tree = false;
            w.status =
                "Cmd-click members or points to add or remove selections, then choose a relation"
                    .into();
        }
        "sketch.finish" => w.confirm_tool(),
        "sketch.cancel" => w.cancel_tool(),
        "sketch.normal-to" => sketch::normal_to(w),
        "sketch.detach-conversion" => crate::sketch_edit_ui::detach_conversion(w),
        "sketch.toggle-construction" => {
            let mut block_message = None;
            if let Some(s) = &w.sketch {
                let before = sketch::sketch_definition(s);
                let mut source = before.clone();
                let owner = s.definition.clone();
                let selection = s.selection.clone();
                let members = s
                    .selection
                    .iter()
                    .filter_map(|e| {
                        if let SketchEntity::Member { member } = e {
                            Some(member.clone())
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<_>>();
                if members.is_empty() {
                    let s = w.sketch.as_mut().unwrap();
                    s.construction = !s.construction;
                    w.status = format!(
                        "New members: {}",
                        if s.construction {
                            "construction geometry"
                        } else {
                            "profile geometry"
                        }
                    );
                    return;
                }
                let construction = !members
                    .iter()
                    .all(|key| source.members.get(key).is_some_and(|m| m.construction));
                let mut source_changed = false;
                let mut instance_changed = false;
                for key in members {
                    let block = cad_core::sketch_blocks::instance_for_member(&source, &key)
                        .and_then(|(id, instance)| {
                            instance
                                .members
                                .iter()
                                .find(|(_, output)| **output == key)
                                .map(|(member, _)| {
                                    (
                                        id.clone(),
                                        instance.definition.clone(),
                                        member.clone(),
                                        instance.construction.is_some(),
                                    )
                                })
                        });
                    if let Some((instance, definition, member, overridden)) = block {
                        if overridden {
                            // A saved instance override applies to its complete source definition.
                            source
                                .block_instances
                                .get_mut(&instance)
                                .unwrap()
                                .construction = Some(construction);
                            instance_changed = true;
                        } else if let Some(member) = source
                            .blocks
                            .get_mut(&definition)
                            .and_then(|definition| definition.members.get_mut(&member))
                        {
                            member.construction = construction;
                            source_changed = true;
                        }
                    } else if let Some(m) = source.members.get_mut(&key) {
                        m.construction = construction;
                    }
                }
                if source_changed || instance_changed {
                    let derived =
                        cad_core::sketch_blocks::prepare(w.session.visible(), &owner, &source)
                            .and_then(|prepared| {
                                let instances =
                                    source.block_instances.keys().cloned().collect::<Vec<_>>();
                                for instance in instances {
                                    cad_core::sketch_blocks::derive_prepared(
                                        w.session.visible(),
                                        &owner,
                                        &mut source,
                                        &instance,
                                        &prepared,
                                    )?;
                                }
                                Ok(())
                            });
                    if let Err(error) = derived {
                        w.status = error.to_string();
                        return;
                    }
                    block_message = Some(match (source_changed, instance_changed) {
                        (true, true) => {
                            "Block source members and whole-instance construction overrides updated"
                        }
                        (true, false) => {
                            "Block source construction updated for every instance using source construction"
                        }
                        (false, true) => {
                            "Construction override updated for the whole selected block instance"
                        }
                        (false, false) => unreachable!(),
                    });
                }
                if let Some(s) = &mut w.sketch {
                    let last_valid = s.last_valid.clone();
                    s.history.push(before);
                    sketch::restore(s, source);
                    s.last_valid = last_valid;
                    s.selection = selection;
                }
            }
            sketch::solve(w);
            if let Some(message) = block_message
                && w.sketch.as_ref().is_some_and(|s| s.error.is_none())
            {
                w.status = message.into();
            }
        }
        "sketch.delete-selection" => {
            if let Some(s) = &mut w.sketch {
                let mut source = sketch::sketch_definition(s);
                s.history.push(source.clone());
                let mut deleted = std::collections::BTreeSet::new();
                let mut points = std::collections::BTreeSet::new();
                for e in &s.selection {
                    match e {
                        SketchEntity::Member { member } => {
                            deleted.insert(member.clone());
                        }
                        SketchEntity::Constraint { constraint } => {
                            source.constraints.retain(|c| &c.id != constraint)
                        }
                        SketchEntity::Point { point } => {
                            points.insert(point.clone());
                            for fillet in source.fillets.values() {
                                if &fillet.corner == point {
                                    deleted.extend(fillet.lines.iter().cloned());
                                    deleted.insert(fillet.arc.clone());
                                } else if fillet.trim.contains(point) || &fillet.through == point {
                                    deleted.insert(fillet.arc.clone());
                                }
                            }
                            for offset in source.offsets.values() {
                                deleted.extend(
                                    offset
                                        .members
                                        .iter()
                                        .filter(|m| m.points.contains(point))
                                        .map(|m| m.member.clone()),
                                );
                            }
                            for c in source.chamfers.values() {
                                if &c.corner == point {
                                    deleted.extend(c.lines.iter().cloned());
                                    deleted.insert(c.member.clone());
                                } else if c.trim.contains(point) {
                                    deleted.insert(c.member.clone());
                                }
                            }
                            for p in source.transforms.values() {
                                if cad_core::sketch_transform::generated_points(p).contains(point)
                                    || p.transform.reference_points().contains(point)
                                {
                                    deleted.extend(cad_core::sketch_transform::outputs(p));
                                }
                            }
                            for (member, spline) in &source.splines {
                                if spline.fit_points().contains(point)
                                    || spline.driven_points().contains(point)
                                {
                                    deleted.insert(member.clone());
                                }
                            }
                            for (key, definition) in &source.conics {
                                if definition.source_points().contains(point)
                                    || definition.driven_points().contains(point)
                                {
                                    deleted.extend(definition.outputs(key));
                                }
                            }
                            deleted.extend(
                                source
                                    .members
                                    .iter()
                                    .filter(|(_, m)| m.curve.points().contains(&point.as_str()))
                                    .map(|(id, _)| id.clone()),
                            );
                        }
                        _ => {}
                    }
                }
                cad_core::sketch_edit::delete_members(&mut source, &deleted);
                source
                    .constraints
                    .retain(|c| !c.entities.iter().any(|id| points.contains(id)));
                source.points.retain(|id, _| !points.contains(id));
                cad_core::sketch_dimensions::reconcile_removed(&mut source);
                sketch::restore(s, source);
                s.selected_member = None;
                s.selected = None;
            }
            sketch::solve(w);
        }
        _ if command.starts_with("sketch.relation.") => {
            sketch::cancel_pending_chain(w);
            let result = w.sketch.as_ref().map(|s| {
                a::relation_with_geometry(
                    w.session.visible(),
                    &s.definition,
                    &sketch::sketch_definition(s),
                    &s.selection,
                    command.trim_start_matches("sketch.relation."),
                )
            });
            if let Some(result) = result {
                match result {
                    Ok(relations) => {
                        let mut added = 0;
                        if let Some(s) = &mut w.sketch {
                            let before = sketch::sketch_definition(s);
                            let mut pending = Vec::new();
                            for relation in relations {
                                if !a::relation_already_present(&before, &relation)
                                    && !pending
                                        .iter()
                                        .any(|existing| a::relation_equivalent(existing, &relation))
                                {
                                    pending.push(relation);
                                }
                            }
                            if pending.is_empty() {
                                w.status = "The selected entities already have this relation; existing references retained".into();
                                return;
                            }
                            added = pending.len();
                            s.history.push(before);
                            s.constraints.extend(pending);
                        }
                        sketch::solve(w);
                        if w.sketch
                            .as_ref()
                            .is_some_and(|sketch| sketch.error.is_none())
                        {
                            w.status = format!(
                                "Added {added} relation{} in one sketch edit · Finish Sketch applies the validated result",
                                if added == 1 { "" } else { "s" }
                            );
                        }
                    }
                    Err(e) => w.status = e.to_string(),
                }
            }
        }
        _ => crate::ui::action(w, command),
    }
}
pub fn controls(ui: &mut egui::Ui, w: &mut Workspace) {
    let mut promote = None;
    let mut changed = false;
    let mut dimension_choice = None;
    let mut clicked_reference = None;
    let mut relation_command = None;
    let mut cancel_dimension_requested = false;
    let mut finish_dimension_requested = false;
    let dimension_mode = w.fields.contains_key("smart-dimension-mode");
    let measurements =
        w.sketch
            .as_ref()
            .map(|s| {
                let source = s
                    .last_valid
                    .clone()
                    .unwrap_or_else(|| sketch::sketch_definition(s));
                source
                    .constraints
                    .iter()
                    .filter(|c| c.driven)
                    .map(|c| {
                        (
                            c.id.clone(),
                            a::measured_dimension(w.session.visible(), &s.definition, &source, c)
                                .map(|value| {
                                    if a::dimension_unit(&c.kind) == Dimension::Angle {
                                        w.session.visible().settings.format_angle_radians(value)
                                    } else {
                                        w.session.visible().settings.format_length(value)
                                    }
                                }),
                        )
                    })
                    .collect::<std::collections::BTreeMap<_, _>>()
            })
            .unwrap_or_default();
    if let Some(s) = &mut w.sketch {
        let before = sketch::sketch_definition(s);
        let mut text_edit_started = false;
        ui.horizontal(|ui| {
            ui.strong(if dimension_mode || s.dimension.is_some() {
                "Smart Dimension"
            } else {
                "Selection"
            });
            ui.small(format!(
                "{} selected",
                s.selection.len() + usize::from(s.dimension_model.is_some())
            ));
        });
        if dimension_mode && s.dimension.is_none() {
            ui.small("Pick a member for size, or two references for distance / angle.");
        }
        if let Some(editor) = &mut s.dimension {
            let previous = editor.choice;
            egui::ComboBox::from_id_salt("smart-dimension-kind")
                .selected_text(&editor.choices[editor.choice].label)
                .show_ui(ui, |ui| {
                    for (i, choice) in editor.choices.iter().enumerate() {
                        ui.selectable_value(&mut editor.choice, i, &choice.label);
                    }
                });
            if editor.choice != previous {
                dimension_choice = Some((
                    editor.constraint.clone(),
                    editor.choices[editor.choice].clone(),
                ));
            }
            if let Some(c) = s.constraints.iter_mut().find(|c| c.id == editor.constraint) {
                ui.label(a::label(&c.kind));
                if c.driven {
                    ui.label(
                        measurements
                            .get(&c.id)
                            .map(|value| {
                                value.as_ref().cloned().unwrap_or_else(ToString::to_string)
                            })
                            .unwrap_or_else(|| "Awaiting current measurement".into()),
                    );
                } else {
                    let response = controls::expression_edit(
                        ui,
                        &format!("{}::smart-value", c.id),
                        a::label(&c.kind),
                        &mut c.value,
                        editor.focus_value,
                        "25.4 mm, 1 in, or a parameter expression",
                        None,
                    );
                    changed |= response.changed();
                    text_edit_started |= response.gained_focus();
                    if response.lost_focus()
                        && ui.input(|input| input.key_pressed(egui::Key::Enter))
                    {
                        finish_dimension_requested = true;
                    }
                }
                editor.focus_value = false;
                ui.horizontal_wrapped(|ui| {
                    if controls::button(
                        ui,
                        &format!("{}.done", c.id),
                        "sketch-dimension-action",
                        &["compact"],
                        "Done Dimension",
                        true,
                        false,
                    )
                    .clicked()
                    {
                        finish_dimension_requested = true;
                    }
                    if controls::button(
                        ui,
                        &format!("{}.cancel", c.id),
                        "sketch-dimension-action",
                        &["compact"],
                        "Cancel Dimension",
                        true,
                        false,
                    )
                    .clicked()
                    {
                        cancel_dimension_requested = true;
                    }
                });
                if editor.extend_pick {
                    ui.small("Pick a second reference for distance / angle, or finish this size.");
                }
                controls::collapsing(
                    ui,
                    &format!("{}::dimension-options", c.id),
                    "sketch-secondary-section",
                    "Dimension options",
                    false,
                    |ui| {
                        let mode = ui.checkbox(&mut c.driven, "Reference measurement")
                        .on_hover_text("Reference measures geometry. Driving uses the saved dimension expression.");
                        changed |= mode.changed();
                        if mode.changed() {
                            s.history.push(before.clone());
                        }
                        if ui
                            .add_enabled(!c.driven, egui::Button::new("Create Named Parameter"))
                            .clicked()
                        {
                            promote =
                                Some((c.id.clone(), c.value.clone(), a::dimension_unit(&c.kind)));
                        }
                        ui.small("Edits preview live. Done Dimension retains the draft; Finish Sketch commits it.");
                        if c.kind == "point_line_distance" {
                            ui.small("Perpendicular distance to the oriented line. The saved side stays fixed through zero.");
                        }
                    },
                );
            }
        }
        if dimension_mode || !s.selection.is_empty() || s.tool == sketch::Tool::Select {
            ui.horizontal_wrapped(|ui| {
                for (label, entity) in [
                    ("Origin", SketchEntity::Origin),
                    ("X Axis", SketchEntity::XAxis),
                    ("Y Axis", SketchEntity::YAxis),
                ] {
                    let response = controls::button(
                        ui,
                        &format!("{}::reference-action.{}", s.sketch_id, entity.key()),
                        "sketch-reference-action",
                        &["modeling-action", "compact"],
                        label,
                        true,
                        s.selection.contains(&entity),
                    );
                    if response.clicked() {
                        clicked_reference = Some(entity);
                    }
                }
            });
        }
        ui.horizontal_wrapped(|ui| {
            for (kind, label) in a::RELATIONS {
                let availability = a::relation(&before, &s.selection, kind);
                if availability.is_ok() || *kind == "tangent" {
                    let reason = availability.as_ref().err().map(ToString::to_string);
                    if controls::button(
                        ui,
                        &format!("{}::relation-action.{kind}", s.sketch_id),
                        "sketch-relation-action",
                        &["modeling-action", "compact"],
                        label,
                        availability.is_ok(),
                        false,
                    )
                    .on_hover_text(a::relation_description(kind))
                    .on_disabled_hover_text(reason.as_deref().unwrap_or_default())
                    .clicked()
                    {
                        relation_command = Some(format!("sketch.relation.{kind}"));
                    }
                }
            }
        });
        if text_edit_started {
            s.history.push(before);
        }
        let reconcile = dimension_choice.is_some();
        if let Some((old, choice)) = dimension_choice {
            s.history.push(sketch::sketch_definition(s));
            let driven = s
                .constraints
                .iter()
                .find(|c| c.id == old)
                .is_some_and(|c| c.driven);
            s.constraints.retain(|c| c.id != old);
            let constraint = attach_dimension(s, &choice, driven);
            if let Some(editor) = &mut s.dimension {
                editor.constraint = constraint;
            }
            changed = true;
        }
        if reconcile {
            let mut source = sketch::sketch_definition(s);
            cad_core::sketch_dimensions::reconcile_removed(&mut source);
            s.dimension_groups = source.dimension_groups;
        }
    }
    if cancel_dimension_requested {
        cancel_dimension(w);
        return;
    }
    if finish_dimension_requested {
        finish_dimension(w);
        changed = false;
    }
    if let Some(command) = relation_command {
        action(w, &command);
    }
    if let Some(entity) = clicked_reference {
        select(
            w,
            Some(entity),
            ui.input(|i| i.modifiers.shift || i.modifiers.command || i.modifiers.ctrl),
        );
        if w.fields.contains_key("smart-dimension-mode") {
            selection_changed(w);
        }
    }
    if let Some((constraint, value, dimension)) = promote
        && let Some(definition) = w.sketch.as_ref().map(|s| s.definition.clone())
    {
        let part = &w.session.visible().parts[&definition];
        let mut i = 1;
        while part.parameters.values().any(|p| p.name == format!("D{i}")) {
            i += 1;
        }
        let name = format!("D{i}");
        w.dispatch(commands::Command::PutParameter {
            definition,
            parameter: id(),
            value: Parameter {
                name: name.clone(),
                expression: value,
                dimension,
            },
        });
        if let Some(s) = &mut w.sketch
            && let Some(c) = s.constraints.iter_mut().find(|c| c.id == constraint)
        {
            c.value = Expr(name);
        }
        changed = true;
    }
    if changed {
        sketch::solve(w);
    }
}

/// Less frequent editors sit below active tool and selected-member inputs.
/// Custom layouts retain all of the underlying relation and parameter actions.
pub fn secondary_controls(ui: &mut egui::Ui, w: &mut Workspace) {
    let Some(s) = &w.sketch else { return };
    let id = format!("{}::relations-section", s.sketch_id);
    let attention = s.error.is_some() || !s.feedback.unsatisfied.is_empty();
    controls::collapsing(
        ui,
        &id,
        "sketch-secondary-section",
        "Dimensions & Relations",
        attention,
        |ui| {
            crate::sketch_relations_ui::panel(ui, w);
        },
    );
    crate::sketch_edit_ui::modifiers(ui, w);
    let open_parameters = w.fields.remove("graph-parameters").is_some();
    controls::inspector_section(
        ui,
        controls::InspectorSection {
            id: "sketch:named-parameters",
            role: "sketch-secondary-section",
            classes: &["secondary", "parameters"],
            label: "Named parameters",
            state: "ready",
            collapse: Some(false),
            open: open_parameters.then_some(true),
        },
        |ui| crate::ui::parameters(ui, w),
    );
}

fn anchor(source: &Sketch, c: &SketchConstraint) -> Option<[f64; 2]> {
    let key = c.entities.first()?;
    if c.kind == "point_line_distance" {
        let (p, foot) = perpendicular_witness(source, c)?;
        return Some([(p[0] + foot[0]) / 2., (p[1] + foot[1]) / 2.]);
    }
    if let Ok((a, b)) = a::entity_line(source, key) {
        return Some([(a[0] + b[0]) / 2., (a[1] + b[1]) / 2.]);
    }
    if let Ok(p) = a::entity_point(source, key) {
        if let Some(other) = c
            .entities
            .get(1)
            .and_then(|k| a::entity_point(source, k).ok())
        {
            return Some([(p[0] + other[0]) / 2., (p[1] + other[1]) / 2.]);
        }
        return Some(p);
    }
    source
        .members
        .get(key)?
        .curve
        .points()
        .first()
        .and_then(|p| source.points.get(*p))
        .copied()
}
fn perpendicular_witness(source: &Sketch, c: &SketchConstraint) -> Option<([f64; 2], [f64; 2])> {
    let p = a::entity_point(source, c.entities.first()?).ok()?;
    let (a, b) = a::entity_line(source, c.entities.get(1)?).ok()?;
    let d = [b[0] - a[0], b[1] - a[1]];
    let length = d[0].hypot(d[1]);
    if !length.is_finite() || length < 1e-9 {
        return None;
    }
    let u = [d[0] / length, d[1] / length];
    let t = (p[0] - a[0]) * u[0] + (p[1] - a[1]) * u[1];
    let foot = [a[0] + u[0] * t, a[1] + u[1] * t];
    (p.iter().chain(&foot).all(|v| v.is_finite())).then_some((p, foot))
}
/// Screen-facing dimensions and relation badges follow solved sketch geometry.
pub fn annotations(w: &mut Workspace, ui: &mut egui::Ui, r: egui::Rect) -> bool {
    let accent = viewport::selection_color(w);
    let mut consumed = crate::sketch_dimensions_ui::annotations(w, ui, r);
    let Some(s) = &w.sketch else { return consumed };
    let source = sketch::sketch_definition(s);
    let geometry = sketch::display_definition(s);
    let plane = s.plane.clone();
    let matrix = s.matrix;
    let mut clicked = None;
    let mut active_annotation = None;
    let inactive = cad_core::sketch_transform::inactive_entities(&source);
    for (index, c) in source.constraints.iter().enumerate() {
        if cad_core::sketch_dimensions::group_for_constraint(&source, &c.id).is_some() {
            continue;
        }
        if c.entities.iter().any(|key| inactive.contains(key)) {
            continue;
        }
        let Some(p) = anchor(&geometry, c)
            .and_then(|p| viewport::project(w, r, sketch::world(&plane, &matrix, p)))
        else {
            continue;
        };
        let dimension = a::is_dimension(&c.kind);
        if !w.view.sketch_annotation_visibility[if dimension { 0 } else { 1 }] {
            continue;
        }
        let active = crate::sketch_relations_ui::constraint_emphasized(s, c);
        let unsatisfied = s.feedback.unsatisfied.contains_key(&c.id);
        let region = presentation::Region {
            id: c.id.clone(),
            role: "sketch-annotation".into(),
            component: None,
            rect: [p.x, p.y, 0., 0.],
            classes: vec![if dimension { "dimension" } else { "relation" }.into()],
            resizable: false,
        };
        let props = w.theme.properties(
            &region,
            &w.view.active_layout,
            if active {
                "selected"
            } else if unsatisfied {
                "unsatisfied"
            } else {
                "ready"
            },
        );
        if props.get("display").is_some_and(|v| v == "none") {
            continue;
        }
        let number = |key: &str, default: f32| {
            props
                .get(key)
                .and_then(|v| v.trim_end_matches("px").parse::<f32>().ok())
                .filter(|v| v.is_finite())
                .unwrap_or(default)
        };
        let text = if dimension {
            let resolved = if c.driven {
                a::measured_dimension(w.session.visible(), &s.definition, &geometry, c)
            } else {
                expression::value(
                    w.session.visible(),
                    &s.definition,
                    &c.value,
                    a::dimension_unit(&c.kind),
                )
            }
            .ok()
            .map(|value| {
                if a::dimension_unit(&c.kind) == Dimension::Angle {
                    w.session.visible().settings.format_angle_radians(value)
                } else {
                    w.session.visible().settings.format_length(value)
                }
            });
            let prefix = match c.kind.as_str() {
                "diameter" => "Ø ",
                "radius" => "R ",
                "fix_x" => "X ",
                "fix_y" => "Y ",
                "distance_x" => "↔ ",
                "distance_y" => "↕ ",
                "major_radius" => "Major R ",
                "minor_radius" => "Minor R ",
                "arc_length" => "Arc L ",
                "arc_angle" => "Arc ∠ ",
                "rotation" => "Rotation ",
                _ => "",
            };
            let parameterized =
                expression::references(&c.value.0).is_ok_and(|references| !references.is_empty());
            let value = if c.driven {
                format!(
                    "{prefix}{}",
                    resolved.unwrap_or_else(|| "Awaiting current measurement".into())
                )
            } else {
                if parameterized {
                    format!(
                        "{prefix}{}{}",
                        c.value.0,
                        resolved
                            .map(|value| format!(" = {value}"))
                            .unwrap_or_default()
                    )
                } else {
                    format!("{prefix}{}", resolved.unwrap_or_else(|| c.value.0.clone()))
                }
            };
            if c.driven {
                format!("({value})")
            } else {
                value
            }
        } else {
            a::label(&c.kind).into()
        };
        let offset = w.view.sketch_annotations.get(&c.id).copied().unwrap_or([
            number("left", 24. + (index % 3) as f32 * 10.),
            number("top", -28. - (index % 4) as f32 * 21.),
        ]);
        let position = p + egui::vec2(offset[0], offset[1]);
        let font = egui::FontId::new(
            number("font-size", if dimension { 12. } else { 10. }).clamp(8., 40.),
            if props.get("font-family").is_some_and(|v| v == "monospace") {
                egui::FontFamily::Monospace
            } else {
                egui::FontFamily::Proportional
            },
        );
        let color = props
            .get("color")
            .and_then(|v| crate::ui::color(v))
            .unwrap_or(if active {
                accent
            } else if unsatisfied {
                ui.visuals().error_fg_color
            } else {
                ui.visuals().text_color()
            });
        let background = props
            .get("background")
            .and_then(|v| crate::ui::color(v))
            .unwrap_or(ui.visuals().panel_fill.gamma_multiply(0.95));
        let padding = number("padding", 4.).clamp(0., 20.);
        let radius = number("border-radius", 3.).clamp(0., 20.);
        let galley = ui.painter().layout_no_wrap(text, font, color);
        let rect = egui::Rect::from_center_size(
            position,
            galley.size() + egui::vec2(padding * 2., padding * 2.),
        );
        if s.dimension
            .as_ref()
            .is_some_and(|editor| editor.constraint == c.id)
        {
            active_annotation = Some(rect);
        }
        if !r.intersects(rect) {
            continue;
        }
        let hit = ui.interact(
            rect,
            egui::Id::new(("sketch-annotation", &c.id)),
            if w.fields.contains_key("smart-dimension-mode") {
                egui::Sense::click()
            } else {
                egui::Sense::click_and_drag()
            },
        );
        if c.kind == "point_line_distance"
            && let Some((a, b)) = perpendicular_witness(&geometry, c)
            && let (Some(a), Some(b)) = (
                viewport::project(w, r, sketch::world(&plane, &matrix, a)),
                viewport::project(w, r, sketch::world(&plane, &matrix, b)),
            )
        {
            ui.painter()
                .with_clip_rect(r)
                .line_segment([a, b], Stroke::new(1., color));
            ui.painter()
                .with_clip_rect(r)
                .circle_stroke(a, 2.5, Stroke::new(1., color));
            ui.painter()
                .with_clip_rect(r)
                .circle_stroke(b, 2.5, Stroke::new(1., color));
        }
        ui.painter()
            .with_clip_rect(r)
            .line_segment([p, rect.center()], Stroke::new(0.8, color));
        ui.painter()
            .with_clip_rect(r)
            .rect_filled(rect, radius, background);
        ui.painter().with_clip_rect(r).rect_stroke(
            rect,
            radius,
            Stroke::new(1., color),
            egui::StrokeKind::Inside,
        );
        ui.painter().with_clip_rect(r).galley(
            rect.min + egui::vec2(padding, padding),
            galley,
            color,
        );
        if hit.clicked() || hit.secondary_clicked() {
            consumed = true;
            clicked = Some((c.id.clone(), hit.clone()));
        }
        if hit.dragged() {
            consumed = true;
            w.selection_popup = None;
            let delta = ui.input(|i| i.pointer.delta());
            w.view
                .sketch_annotations
                .insert(c.id.clone(), [offset[0] + delta.x, offset[1] + delta.y]);
        }
        hit.on_hover_text(format!(
            "{} · Click to edit or remove; drag to position the annotation",
            a::label(&c.kind)
        ));
    }
    if let Some((constraint, response)) = clicked {
        select(w, Some(SketchEntity::Constraint { constraint }), false);
        if w.fields.contains_key("smart-dimension-mode") {
            selection_changed(w);
        } else if response.double_clicked() {
            smart_dimension(w);
        } else {
            selection_ui::open(w, &response, response.rect.right_bottom());
        }
    }
    if let Some(annotation) = active_annotation {
        consumed |= nearby_dimension(w, ui, r, annotation);
    }
    consumed
}

/// The in-model editor is another binding to the shared dimension expression.
/// Its placement, size, colors and controls remain ordinary semantic styles.
fn nearby_dimension(
    w: &mut Workspace,
    ui: &mut egui::Ui,
    viewport: egui::Rect,
    annotation: egui::Rect,
) -> bool {
    use crate::presentation_controls as controls;
    let elements = dimension_semantics(w);
    let Some(value) = elements
        .iter()
        .find(|element| element.role == "sketch-dimension-value")
    else {
        return false;
    };
    let Some(editor) = elements
        .iter()
        .find(|element| element.role == "sketch-dimension-editor")
    else {
        return false;
    };
    let id = format!("{}::nearby", editor.id);
    let size = egui::vec2(220., 102.).min(viewport.size());
    let min =
        (annotation.right_bottom() + egui::vec2(8., 4.)).clamp(viewport.min, viewport.max - size);
    let default = egui::Rect::from_min_size(min, size);
    let Some(rect) = controls::overlay_rect_state(
        &id,
        "sketch-dimension-input",
        &value.state,
        viewport,
        default,
    ) else {
        return false;
    };
    let props = controls::properties(
        &id,
        "sketch-dimension-input",
        &["viewport-overlay", "dimension-editor"],
        &value.state,
    );
    let field_props = controls::properties(
        &value.id,
        &value.role,
        &["semantic-control", "dimension-value"],
        &value.state,
    );
    let focus = w
        .sketch
        .as_ref()
        .and_then(|sketch| sketch.dimension.as_ref())
        .is_some_and(|editor| editor.focus_nearby);
    let mut expression = crate::presentation_composition::current_value(w, value)
        .as_str()
        .unwrap_or_default()
        .to_owned();
    let mut changed = false;
    let mut requested = None;
    let mut child = ui.new_child(egui::UiBuilder::new().id_salt(&id).max_rect(rect));
    child.set_clip_rect(viewport.intersect(rect));
    controls::apply_properties(&mut child, &props);
    let mut frame = controls::frame_from_properties(&mut child, &props);
    if !props.contains_key("background") {
        frame.fill = child.visuals().panel_fill;
    }
    frame.show(&mut child, |ui| {
        controls::label(
            ui,
            &format!("{}:label", value.id),
            "field-label",
            &["dimension-value"],
            &value.label,
            &value.state,
        );
        ui.scope(|ui| {
            controls::apply_properties(ui, &field_props);
            let size = controls::field_size(ui, &field_props);
            let response = ui
                .add_enabled_ui(value.data["editable"].as_bool().unwrap_or(false), |ui| {
                    let mut source = cad_core::Expr(expression.clone());
                    let response = controls::expression_edit(
                        ui,
                        &value.id,
                        &value.label,
                        &mut source,
                        focus,
                        "Value, units or parameter",
                        Some(size),
                    );
                    if response.changed() {
                        expression = source.0;
                    }
                    response
                })
                .inner;
            if focus && value.data["editable"].as_bool().unwrap_or(false) {
                response.request_focus();
            }
            changed = response.changed();
            if response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter)) {
                requested = Some("sketch.finish-dimension".into());
            }
        });
        ui.horizontal_wrapped(|ui| {
            for action in editor.action_descriptors().into_iter().filter(|action| {
                matches!(
                    action.command.as_str(),
                    "sketch.finish-dimension" | "sketch.cancel-dimension"
                )
            }) {
                if controls::button(
                    ui,
                    &action.id,
                    "sketch-dimension-action",
                    &["dimension-action"],
                    &action.label,
                    action.enabled,
                    false,
                )
                .clicked()
                {
                    requested = Some(action.command);
                }
            }
        });
    });
    if let Some(sketch) = &mut w.sketch
        && let Some(editor) = &mut sketch.dimension
    {
        editor.focus_nearby = false;
    }
    if changed
        && let Err(error) =
            crate::presentation_composition::set_value(w, value, serde_json::json!(expression))
    {
        w.status = error.to_string();
    }
    if let Some(command) = requested {
        action(w, &command);
    }
    ui.input(|input| {
        input
            .pointer
            .hover_pos()
            .is_some_and(|pointer| rect.contains(pointer))
            && (input.pointer.any_pressed()
                || input.pointer.any_released()
                || input.pointer.any_down())
    })
}

pub(crate) fn selected_block(source: &Sketch, selection: &[SketchEntity]) -> Option<Id> {
    selection.iter().find_map(|entity| match entity {
        SketchEntity::Member { member } => {
            cad_core::sketch_blocks::instance_for_member(source, member).map(|(key, _)| key.clone())
        }
        SketchEntity::Point { point } => source
            .block_instances
            .iter()
            .find(|(_, instance)| {
                &instance.anchor == point || instance.points.values().any(|output| output == point)
            })
            .map(|(key, _)| key.clone()),
        _ => None,
    })
}
pub(crate) fn explode_block(w: &mut Workspace, key: &str) {
    let Some(s) = &w.sketch else { return };
    if s.edit.is_some() {
        w.status = "Accept or cancel the current sketch operation first".into();
        return;
    }
    let before = sketch::sketch_definition(s);
    let mut source = before.clone();
    let owner = s.definition.clone();
    match cad_core::sketch_blocks::explode(w.session.visible(), &owner, &mut source, key) {
        Ok(()) => {
            if let Some(s) = &mut w.sketch {
                s.history.push(before);
                sketch::restore(s, source);
            }
            sketch::solve(w);
        }
        Err(error) => w.status = error.to_string(),
    }
}
