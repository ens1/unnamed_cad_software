//! Docked sketch editing operations over shared source intent and preview transactions.
use crate::{Workspace, sketch, viewport};
use bevy_egui::egui;
use cad_core::sketch_transform as transform;
use cad_core::{sketch_actions::SketchEntity, sketch_edit as edit, *};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Convert,
    Offset,
    Fillet,
    Chamfer,
    Mirror,
    LinearPattern,
    CircularPattern,
    MoveCopy,
    Rotate,
    Scale,
    Trim,
    Extend,
    Split,
    Spline,
    Conic,
    Repair,
    Stretch,
    Replace,
    DynamicMirror,
    Text,
    Equation,
    Block,
    DimensionGroup,
}
pub struct Editor {
    pub kind: Kind,
    pub block: Option<crate::sketch_blocks_ui::Editor>,
    pub dimension_group: Option<crate::sketch_dimensions_ui::Editor>,
    pub equation: Option<crate::sketch_equation_ui::Editor>,
    pub before: Sketch,
    pub operation: Option<Id>,
    pub value: Expr,
    pub second: Expr,
    pub angle: Expr,
    pub x: Expr,
    pub y: Expr,
    pub scale: Expr,
    pub count: usize,
    pub axis: transform::Axis,
    pub center: transform::Center,
    pub chamfer_mode: usize,
    pub equal_spacing: bool,
    pub copy: bool,
    pub reverse: bool,
    pub construction: bool,
    pub linked: bool,
    pub sources: Vec<viewport::Selection>,
    pub previewed: bool,
    pub error: Option<String>,
    pub curve_edit_id: Id,
    pub curve_target: Option<Id>,
    pub curve_parameter: f64,
    pub curve_to_intersection: bool,
    pub include_construction: bool,
    pub curve_report: Option<cad_core::sketch_trim::EditReport>,
    pub spline_definition: Option<cad_core::sketch_spline::Definition>,
    pub conic_definition: Option<cad_core::sketch_conic::Definition>,
    pub spline_points: std::collections::BTreeMap<Id, [f64; 2]>,
    pub history_len: usize,
    pub repair_report: Option<cad_core::sketch_relations::RepairReport>,
    pub dynamic: bool,
    pub instance_controls: Vec<transform::Instance>,
    pub instance_focus: usize,
    pub stretch_points: Vec<Id>,
    pub replace_target: Option<Id>,
    pub replace_source: Option<Id>,
    pub consume_replacement: bool,
    pub remove_incompatible: bool,
    pub replace_report: Option<edit::ReplaceReport>,
    pub text_font: Option<cad_core::sketch_text::FontSnapshot>,
    pub text_content: String,
    pub text_anchor: Id,
    pub text_alignment: cad_core::sketch_text::Alignment,
    pub text_face: u32,
    pub text_resources_before: std::collections::BTreeMap<Id, Resource>,
    pub text_construction_override: bool,
}
impl Kind {
    fn label(self) -> &'static str {
        match self {
            Self::Convert => "Convert Entities",
            Self::Offset => "Offset Entities",
            Self::Fillet => "Sketch Fillet",
            Self::Chamfer => "Sketch Chamfer",
            Self::Mirror => "Mirror Entities",
            Self::LinearPattern => "Linear Sketch Pattern",
            Self::CircularPattern => "Circular Sketch Pattern",
            Self::MoveCopy => "Move/Copy Entities",
            Self::Rotate => "Rotate Entities",
            Self::Scale => "Scale Entities",
            Self::Trim => "Trim Entities",
            Self::Extend => "Extend Entities",
            Self::Split => "Split Entities",
            Self::Spline => "Edit Spline",
            Self::Conic => "Edit Conic / Arc Slot",
            Self::Repair => "Repair Sketch",
            Self::Stretch => "Stretch Entities",
            Self::Replace => "Replace Entities",
            Self::DynamicMirror => "Dynamic Mirror Entities",
            Self::Text => "Sketch Text",
            Self::Equation => "Equation Curve",
            Self::Block => "Sketch Block",
            Self::DimensionGroup => "Grouped Sketch Dimensions",
        }
    }
    fn transform_command(self) -> Option<&'static str> {
        Some(match self {
            Self::Mirror | Self::DynamicMirror => "sketch.mirror",
            Self::LinearPattern => "sketch.linear-pattern",
            Self::CircularPattern => "sketch.circular-pattern",
            Self::MoveCopy | Self::Rotate | Self::Scale => "sketch.move-copy",
            _ => return None,
        })
    }
    fn curve_edit_kind(self) -> Option<cad_core::sketch_trim::Kind> {
        Some(match self {
            Self::Trim => cad_core::sketch_trim::Kind::Trim,
            Self::Extend => cad_core::sketch_trim::Kind::Extend,
            Self::Split => cad_core::sketch_trim::Kind::Split,
            _ => return None,
        })
    }
}
impl Editor {
    fn chamfer_size(&self) -> edit::ChamferSize {
        match self.chamfer_mode {
            1 => edit::ChamferSize::Distances {
                first: self.value.clone(),
                second: self.second.clone(),
            },
            2 => edit::ChamferSize::DistanceAngle {
                distance: self.value.clone(),
                angle: self.angle.clone(),
            },
            _ => edit::ChamferSize::Equal {
                distance: self.value.clone(),
            },
        }
    }
    fn transform(&self) -> transform::Transform {
        match self.kind {
            Kind::Mirror | Kind::DynamicMirror => transform::Transform::Mirror {
                axis: self.axis.clone(),
            },
            Kind::LinearPattern => transform::Transform::Linear {
                count: self.count,
                step: [self.x.clone(), self.y.clone()],
            },
            Kind::CircularPattern => transform::Transform::Circular {
                count: self.count,
                center: self.center.clone(),
                angle: self.angle.clone(),
                equal_spacing: self.equal_spacing,
            },
            _ => transform::Transform::Copy {
                center: self.center.clone(),
                translation: [self.x.clone(), self.y.clone()],
                angle: self.angle.clone(),
                scale: self.scale.clone(),
            },
        }
    }
}
fn selected(s: &sketch::EditingSketch) -> (Vec<Id>, Option<Id>) {
    (
        s.selection
            .iter()
            .filter_map(|e| {
                if let SketchEntity::Member { member } = e {
                    Some(member.clone())
                } else {
                    None
                }
            })
            .collect(),
        s.selection.iter().find_map(|e| {
            if let SketchEntity::Point { point } = e {
                Some(point.clone())
            } else {
                None
            }
        }),
    )
}
pub fn begin(w: &mut Workspace, kind: Kind) {
    begin_with_operation(w, kind, None, None, None);
}
pub fn begin_pattern(w: &mut Workspace, operation: Id) {
    let kind = w
        .sketch
        .as_ref()
        .and_then(|s| s.transforms.get(&operation))
        .map(|pattern| match pattern.transform {
            transform::Transform::Mirror { .. } => Kind::Mirror,
            transform::Transform::Linear { .. } => Kind::LinearPattern,
            transform::Transform::Circular { .. } => Kind::CircularPattern,
            _ => Kind::MoveCopy,
        });
    if let Some(kind) = kind {
        begin_with_operation(w, kind, Some(operation), None, None);
    }
}
pub fn begin_text(w: &mut Workspace, operation: Option<Id>) {
    begin_with_operation(w, Kind::Text, operation, None, None);
}
pub fn begin_equation(w: &mut Workspace, operation: Option<Id>) {
    begin_with_operation(w, Kind::Equation, operation, None, None);
}
pub fn begin_block(w: &mut Workspace, mode: crate::sketch_blocks_ui::Mode, key: Option<Id>) {
    begin_with_operation(w, Kind::Block, key.clone(), Some((mode, key)), None);
}
pub fn begin_dimension_group(
    w: &mut Workspace,
    kind: cad_core::sketch_dimensions::Kind,
    key: Option<Id>,
) {
    begin_with_operation(
        w,
        Kind::DimensionGroup,
        key.clone(),
        None,
        Some((kind, key)),
    );
}
fn begin_with_operation(
    w: &mut Workspace,
    kind: Kind,
    override_operation: Option<Id>,
    block_mode: Option<(crate::sketch_blocks_ui::Mode, Option<Id>)>,
    dimension_mode: Option<(cad_core::sketch_dimensions::Kind, Option<Id>)>,
) {
    crate::sketch_handles::end(w);
    sketch::cancel_pending_chain(w);
    let conversion_source = w.selected.clone();
    let text_resources_before = w.session.visible().resources.clone();
    let Some(s) = &mut w.sketch else { return };
    if s.choosing_plane {
        w.status = "Choose a sketch plane first".into();
        return;
    }
    if s.edit.is_some() {
        w.status = "Accept or cancel the current sketch operation first".into();
        return;
    }
    if matches!(kind, Kind::Text | Kind::Block | Kind::Equation) {
        s.tool = sketch::Tool::Select;
        s.chain.clear();
        s.clicks.clear();
    }
    let before = sketch::sketch_definition(s);
    let (members, _) = selected(s);
    let operation = override_operation.or_else(|| match kind {
        Kind::Offset => before
            .offsets
            .iter()
            .find(|(_, o)| o.members.iter().any(|m| members.contains(&m.member)))
            .map(|(k, _)| k.clone()),
        Kind::Fillet => before
            .fillets
            .iter()
            .find(|(_, f)| members.contains(&f.arc))
            .map(|(k, _)| k.clone()),
        Kind::Chamfer => before
            .chamfers
            .iter()
            .find(|(_, c)| members.contains(&c.member))
            .map(|(k, _)| k.clone()),
        Kind::Mirror
        | Kind::DynamicMirror
        | Kind::LinearPattern
        | Kind::CircularPattern
        | Kind::MoveCopy
        | Kind::Rotate
        | Kind::Scale => before
            .transforms
            .iter()
            .find(|(_, p)| {
                Some(p.transform.command()) == kind.transform_command()
                    && transform::outputs(p).iter().any(|m| members.contains(m))
            })
            .map(|(k, _)| k.clone()),
        Kind::Text => members.first().and_then(|member| {
            cad_core::sketch_text::operation_for(&before, member).map(|(key, _)| key.clone())
        }),
        Kind::Convert
        | Kind::Trim
        | Kind::Extend
        | Kind::Split
        | Kind::Spline
        | Kind::Conic
        | Kind::Repair
        | Kind::Stretch
        | Kind::Replace
        | Kind::Block
        | Kind::Equation
        | Kind::DimensionGroup => None,
    });
    let (value, reverse) = operation
        .as_ref()
        .map(|key| match kind {
            Kind::Offset => (
                before.offsets[key].distance.clone(),
                before.offsets[key].reverse,
            ),
            Kind::Fillet => (before.fillets[key].radius.clone(), false),
            _ => ("1 mm".into(), false),
        })
        .unwrap_or((
            Expr(
                if kind == Kind::Fillet {
                    "3 mm"
                } else if kind == Kind::Repair {
                    "0.01 mm"
                } else {
                    "1 mm"
                }
                .into(),
            ),
            false,
        ));
    let construction = operation
        .as_ref()
        .and_then(|key| match kind {
            Kind::Offset => before.offsets[key]
                .members
                .first()
                .and_then(|m| before.members.get(&m.member)),
            Kind::Fillet => before.members.get(&before.fillets[key].arc),
            Kind::Chamfer => before.members.get(&before.chamfers[key].member),
            _ => before
                .transforms
                .get(key)
                .and_then(|p| p.instances.first())
                .and_then(|i| i.members.values().next())
                .and_then(|m| before.members.get(m)),
        })
        .map(|m| m.construction)
        .or_else(|| {
            members
                .first()
                .and_then(|key| before.members.get(key))
                .map(|m| m.construction)
        })
        .unwrap_or(s.construction);
    s.tool = sketch::Tool::Select;
    s.dimension = None;
    s.dimension_pick = None;
    s.dimension_model = None;
    s.chain.clear();
    s.clicks.clear();
    let mut editor = Editor {
        kind,
        before,
        operation,
        value,
        second: "1 mm".into(),
        angle: match kind {
            Kind::CircularPattern => "360 deg",
            Kind::Chamfer => "45 deg",
            Kind::Rotate => "30 deg",
            _ => "0 deg",
        }
        .into(),
        x: if matches!(kind, Kind::LinearPattern | Kind::MoveCopy) {
            "10 mm"
        } else {
            "0 mm"
        }
        .into(),
        y: "0 mm".into(),
        scale: if kind == Kind::Scale { "2" } else { "1" }.into(),
        count: 3,
        axis: transform::Axis::X,
        center: transform::Center::Origin,
        chamfer_mode: 0,
        equal_spacing: true,
        copy: false,
        reverse,
        construction,
        linked: true,
        sources: if kind == Kind::Convert {
            conversion_source
                .filter(|hit| {
                    hit.definition == s.definition
                        && matches!(hit.reference.topology.as_str(), "edge" | "face")
                })
                .into_iter()
                .collect()
        } else {
            vec![]
        },
        previewed: false,
        error: None,
        curve_edit_id: id(),
        curve_target: members.first().cloned(),
        curve_parameter: 0.5,
        curve_to_intersection: true,
        include_construction: true,
        curve_report: None,
        spline_definition: (kind == Kind::Spline)
            .then(|| {
                members.first().and_then(|key| {
                    cad_core::sketch_spline::definition(&sketch::sketch_definition(s), key).ok()
                })
            })
            .flatten(),
        conic_definition: (kind == Kind::Conic)
            .then(|| {
                members.first().and_then(|key| {
                    cad_core::sketch_conic::operation_for(&sketch::sketch_definition(s), key)
                        .map(|(_, definition)| definition.clone())
                })
            })
            .flatten(),
        spline_points: s
            .keys
            .iter()
            .cloned()
            .zip(s.points.iter().copied())
            .collect(),
        history_len: s.history.len(),
        repair_report: None,
        dynamic: kind == Kind::DynamicMirror,
        instance_controls: vec![],
        instance_focus: members
            .first()
            .and_then(|member| {
                transform::instance_for_member(&sketch::sketch_definition(s), member)
                    .map(|(_, index)| index)
            })
            .unwrap_or(0),
        stretch_points: s
            .selection
            .iter()
            .flat_map(|entity| match entity {
                SketchEntity::Point { point } => vec![point.clone()],
                SketchEntity::Member { member } => s
                    .members
                    .get(member)
                    .map(|record| {
                        record
                            .curve
                            .points()
                            .into_iter()
                            .map(str::to_string)
                            .collect()
                    })
                    .unwrap_or_default(),
                _ => vec![],
            })
            .collect(),
        replace_target: members.first().cloned(),
        replace_source: members.get(1).cloned(),
        consume_replacement: true,
        remove_incompatible: false,
        replace_report: None,
        text_font: None,
        text_content: "Text".into(),
        text_anchor: s
            .selection
            .iter()
            .find_map(|entity| {
                if let SketchEntity::Point { point } = entity {
                    Some(point.clone())
                } else {
                    None
                }
            })
            .unwrap_or_else(id),
        text_alignment: cad_core::sketch_text::Alignment::Left,
        text_face: 0,
        text_resources_before,
        text_construction_override: false,
        block: None,
        dimension_group: None,
        equation: None,
    };
    if kind == Kind::Equation {
        match crate::sketch_equation_ui::Editor::new(
            &editor.before,
            &s.selection,
            editor.operation.clone(),
        ) {
            Ok(mut equation) => {
                if editor.operation.is_none() {
                    equation.request.construction = construction;
                }
                editor.equation = Some(equation);
            }
            Err(error) => {
                w.status = error.to_string();
                return;
            }
        }
    }
    editor.dimension_group = dimension_mode.map(|(kind, key)| {
        crate::sketch_dimensions_ui::Editor::new(&editor.before, &s.selection, kind, key)
    });
    editor.block = block_mode.map(|(mode, key)| {
        crate::sketch_blocks_ui::Editor::new(&editor.before, &s.selection, mode, key)
    });
    if kind == Kind::DynamicMirror
        && members.len() == 1
        && matches!(
            editor.before.members[&members[0]].curve,
            SketchCurve::Line { .. }
        )
    {
        editor.axis = transform::Axis::Member {
            member: members[0].clone(),
        };
    }
    if kind == Kind::Text {
        editor.value = "10 mm".into();
        editor.angle = "0 deg".into();
        editor.x = "0 mm".into();
        editor.y = "0 mm".into();
        editor.second = "0 mm".into();
        editor.scale = "1.2".into();
    }
    if let Some(key) = &editor.operation {
        if let Some(text) = editor.before.texts.get(key) {
            editor.text_font = Some(text.font.clone());
            editor.text_content = text.text.clone();
            editor.text_anchor = text.anchor.clone();
            editor.text_alignment = text.alignment;
            editor.text_face = text.font.face_index;
            editor.value = text.height.clone();
            editor.angle = text.angle.clone();
            editor.x = text.offset[0].clone();
            editor.y = text.offset[1].clone();
            editor.second = text.tracking.clone();
            editor.scale = text.line_spacing.clone();
            editor.construction = text
                .outputs()
                .first()
                .and_then(|member| editor.before.members.get(member))
                .is_some_and(|member| member.construction);
        }
        if let Some(c) = editor.before.chamfers.get(key) {
            match &c.size {
                edit::ChamferSize::Equal { distance } => editor.value = distance.clone(),
                edit::ChamferSize::Distances { first, second } => {
                    editor.chamfer_mode = 1;
                    editor.value = first.clone();
                    editor.second = second.clone();
                }
                edit::ChamferSize::DistanceAngle { distance, angle } => {
                    editor.chamfer_mode = 2;
                    editor.value = distance.clone();
                    editor.angle = angle.clone();
                }
            }
        }
        if let Some(p) = editor.before.transforms.get(key) {
            editor.copy = true;
            editor.dynamic = p.dynamic;
            editor.instance_controls = p.instances.clone();
            if p.instances
                .iter()
                .all(|instance| instance.members.is_empty())
            {
                editor.construction = p.dynamic_construction;
            }
            match &p.transform {
                transform::Transform::Mirror { axis } => editor.axis = axis.clone(),
                transform::Transform::Linear { count, step } => {
                    editor.count = *count;
                    editor.x = step[0].clone();
                    editor.y = step[1].clone();
                }
                transform::Transform::Circular {
                    count,
                    center,
                    angle,
                    equal_spacing,
                } => {
                    editor.count = *count;
                    editor.center = center.clone();
                    editor.angle = angle.clone();
                    editor.equal_spacing = *equal_spacing;
                }
                transform::Transform::Copy {
                    center,
                    translation,
                    angle,
                    scale,
                } => {
                    editor.center = center.clone();
                    editor.x = translation[0].clone();
                    editor.y = translation[1].clone();
                    editor.angle = angle.clone();
                    editor.scale = scale.clone();
                }
            }
        }
    }
    if kind == Kind::Text {
        editor
            .spline_points
            .entry(editor.text_anchor.clone())
            .or_insert([0.; 2]);
    }
    s.edit = Some(editor);
    w.fields.remove("smart-dimension-mode");
    w.manager_tree = false;
    w.selection_popup = None;
    if kind != Kind::Convert
        || w.sketch
            .as_ref()
            .and_then(|s| s.edit.as_ref())
            .is_some_and(|e| !e.sources.is_empty())
    {
        rebuild(w, false);
        if kind == Kind::Equation {
            let member = w.sketch.as_ref().and_then(|s| {
                s.edit
                    .as_ref()
                    .and_then(|editor| editor.operation.as_ref())
                    .filter(|member| s.members.contains_key(*member))
                    .cloned()
            });
            if let Some(member) = member {
                crate::sketch_ui::select(w, Some(SketchEntity::Member { member }), false);
            }
        }
    } else {
        w.status =
            "Convert Entities: click a model edge or face; Shift-click adds to the selection"
                .into();
    }
}
fn load(w: &mut Workspace, source: Sketch) {
    if let Some(s) = &mut w.sketch {
        let old = s.last_valid.clone();
        let selection = s.selection.clone();
        sketch::restore(s, source);
        s.last_valid = old;
        s.selection = selection;
    }
    sketch::solve(w);
}
pub fn rebuild(w: &mut Workspace, reselect: bool) {
    let Some(s) = &mut w.sketch else { return };
    let Some(mut editor) = s.edit.take() else {
        return;
    };
    editor.previewed = false;
    editor.error = None;
    editor.curve_report = None;
    editor.repair_report = None;
    editor.replace_report = None;
    let (selected, corner) = selected(s);
    let selected_entities = s.selection.clone();
    let owner = s.definition.clone();
    let plane = s.plane.clone();
    let mut source = if reselect || editor.operation.is_none() || editor.kind == Kind::Convert {
        editor.before.clone()
    } else {
        sketch::sketch_definition(s)
    };
    let result = (|| -> Result<()> {
        if reselect && let Some(key) = editor.operation.take() {
            edit::remove_modifier(&mut source, &key);
        }
        match editor.kind {
            Kind::Equation => {
                let equation = editor
                    .equation
                    .as_ref()
                    .ok_or_else(|| CadError::Invalid("Equation curve editor is missing".into()))?;
                equation.apply(w.session.visible(), &owner, &mut source)?;
                editor.operation = Some(equation.id.clone());
            }
            Kind::DimensionGroup => {
                let dimensions = editor
                    .dimension_group
                    .as_mut()
                    .ok_or_else(|| CadError::Invalid("Dimension group editor is missing".into()))?;
                dimensions.apply(&mut source)?;
                editor.operation = Some(dimensions.id.clone());
            }
            Kind::Block => {
                let block = editor
                    .block
                    .as_mut()
                    .ok_or_else(|| CadError::Invalid("Block editor is missing".into()))?;
                if reselect && block.mode == crate::sketch_blocks_ui::Mode::Create {
                    block.selected = selected.clone();
                    block.definition_id = None;
                    block.instance_id = None;
                }
                block.apply(w.session.visible(), &owner, &mut source)?;
                editor.operation = block
                    .instance_id
                    .clone()
                    .or_else(|| block.definition_id.clone());
            }
            Kind::Text => {
                let font = editor.text_font.clone().ok_or_else(|| {
                    CadError::Invalid("Choose a local TrueType/OpenType outline font".into())
                })?;
                let key = editor.operation.get_or_insert_with(id).clone();
                source.points = editor.spline_points.clone();
                cad_core::sketch_text::set(
                    w.session.visible(),
                    &owner,
                    &mut source,
                    &key,
                    &cad_core::sketch_text::Create {
                        font,
                        text: editor.text_content.clone(),
                        anchor: editor.text_anchor.clone(),
                        height: editor.value.clone(),
                        offset: [editor.x.clone(), editor.y.clone()],
                        angle: editor.angle.clone(),
                        tracking: editor.second.clone(),
                        line_spacing: editor.scale.clone(),
                        alignment: editor.text_alignment,
                        construction: editor.construction,
                        apply_construction: editor.text_construction_override,
                    },
                )?;
            }
            Kind::Stretch => {
                if reselect {
                    editor.stretch_points = selected_entities
                        .iter()
                        .flat_map(|entity| match entity {
                            SketchEntity::Point { point } => vec![point.clone()],
                            SketchEntity::Member { member } => source
                                .members
                                .get(member)
                                .map(|record| {
                                    record
                                        .curve
                                        .points()
                                        .into_iter()
                                        .map(str::to_string)
                                        .collect()
                                })
                                .unwrap_or_default(),
                            _ => vec![],
                        })
                        .collect();
                }
                transform::stretch(
                    w.session.visible(),
                    &owner,
                    &mut source,
                    &transform::StretchRequest {
                        points: editor.stretch_points.clone(),
                        translation: [editor.x.clone(), editor.y.clone()],
                    },
                )?;
            }
            Kind::Replace => {
                if reselect {
                    editor.replace_target = selected.first().cloned();
                    editor.replace_source = selected.get(1).cloned();
                }
                editor.replace_report = Some(edit::replace_entities(
                    w.session.visible(),
                    &owner,
                    &mut source,
                    &edit::ReplaceRequest {
                        edit_id: editor.curve_edit_id.clone(),
                        target: editor.replace_target.clone().ok_or_else(|| {
                            CadError::Invalid("Choose the member to replace".into())
                        })?,
                        replacement: editor.replace_source.clone().ok_or_else(|| {
                            CadError::Invalid("Choose the exact replacement member".into())
                        })?,
                        reverse: editor.reverse,
                        consume_replacement: editor.consume_replacement,
                        remove_incompatible_relations: editor.remove_incompatible,
                    },
                )?);
            }
            Kind::Repair => {
                let tolerance = expression::value(
                    w.session.visible(),
                    &owner,
                    &editor.value,
                    Dimension::Length,
                )?;
                editor.repair_report = Some(cad_core::sketch_relations::repair(
                    &mut source,
                    &cad_core::sketch_relations::RepairRequest {
                        edit_id: editor.curve_edit_id.clone(),
                        members: selected.clone(),
                        tolerance,
                        include_construction: editor.include_construction,
                        remove_duplicate_relations: editor.linked,
                    },
                )?);
            }
            Kind::Conic => {
                if reselect {
                    editor.curve_target = selected.first().cloned();
                    editor.conic_definition = editor.curve_target.as_ref().and_then(|member| {
                        cad_core::sketch_conic::operation_for(&source, member)
                            .map(|(_, definition)| definition.clone())
                    });
                    editor.spline_points = source.points.clone();
                }
                let member = editor
                    .curve_target
                    .as_ref()
                    .ok_or_else(|| CadError::Invalid("Select a conic or arc-slot member".into()))?;
                let definition = editor.conic_definition.clone().ok_or_else(|| {
                    CadError::Invalid("Select a member with conic or arc-slot source intent".into())
                })?;
                source.points = editor.spline_points.clone();
                let key = cad_core::sketch_conic::operation_for(&source, member)
                    .map(|(key, _)| key.clone())
                    .ok_or_else(|| CadError::Invalid("Conic source is missing".into()))?;
                cad_core::sketch_conic::set(
                    w.session.visible(),
                    &owner,
                    &mut source,
                    member,
                    definition,
                )?;
                for output in source.conics[&key].outputs(&key) {
                    source
                        .members
                        .get_mut(&output)
                        .ok_or_else(|| CadError::Invalid("Conic output is missing".into()))?
                        .construction = editor.construction;
                }
            }
            Kind::Spline => {
                if reselect {
                    editor.curve_target = selected.first().cloned();
                    editor.spline_definition = editor
                        .curve_target
                        .as_ref()
                        .map(|key| cad_core::sketch_spline::definition(&source, key))
                        .transpose()?;
                    editor.spline_points = source.points.clone();
                }
                let member = editor
                    .curve_target
                    .as_ref()
                    .ok_or_else(|| CadError::Invalid("Select one spline to edit".into()))?;
                let definition = editor.spline_definition.clone().ok_or_else(|| {
                    CadError::Invalid("Select an editable Bézier or rational spline".into())
                })?;
                source.points = editor.spline_points.clone();
                cad_core::sketch_spline::set(
                    w.session.visible(),
                    &owner,
                    &mut source,
                    member,
                    definition,
                )?;
                source
                    .members
                    .get_mut(member)
                    .ok_or_else(|| CadError::Invalid("Spline is missing".into()))?
                    .construction = editor.construction;
            }
            Kind::Trim | Kind::Extend | Kind::Split => {
                if reselect {
                    editor.curve_target = selected.first().cloned();
                }
                let target = editor.curve_target.clone().ok_or_else(|| {
                    CadError::Invalid(
                        "Click a sketch member to choose the portion or endpoint to edit".into(),
                    )
                })?;
                let extension = (editor.kind == Kind::Extend && !editor.curve_to_intersection)
                    .then(|| {
                        expression::value(
                            w.session.visible(),
                            &owner,
                            &editor.value,
                            Dimension::Length,
                        )
                    })
                    .transpose()?;
                let report = cad_core::sketch_trim::apply(
                    w.session.visible(),
                    &owner,
                    &mut source,
                    &cad_core::sketch_trim::Request {
                        edit_id: editor.curve_edit_id.clone(),
                        kind: editor.kind.curve_edit_kind().unwrap(),
                        member: target,
                        parameter: editor.curve_parameter,
                        extension,
                        include_construction: editor.include_construction,
                        tolerance: 1e-7,
                    },
                )?;
                for key in &report.members {
                    if let Some(member) = source.members.get_mut(key) {
                        member.construction = editor.construction;
                    }
                }
                editor.curve_report = Some(report);
            }
            Kind::Offset => {
                if let Some(key) = &editor.operation {
                    let o = source.offsets.get_mut(key).ok_or_else(|| {
                        CadError::Invalid("Offset operation no longer exists".into())
                    })?;
                    o.distance = editor.value.clone();
                    o.reverse = editor.reverse;
                    for m in &o.members {
                        if let Some(member) = source.members.get_mut(&m.member) {
                            member.construction = editor.construction;
                        }
                    }
                } else {
                    editor.operation = Some(edit::add_offset(
                        &mut source,
                        &selected,
                        editor.value.clone(),
                        editor.reverse,
                        editor.construction,
                    )?);
                }
            }
            Kind::Fillet => {
                if let Some(key) = &editor.operation {
                    let f = source.fillets.get_mut(key).ok_or_else(|| {
                        CadError::Invalid("Fillet operation no longer exists".into())
                    })?;
                    f.radius = editor.value.clone();
                    if let Some(member) = source.members.get_mut(&f.arc) {
                        member.construction = editor.construction;
                    }
                } else {
                    editor.operation = Some(edit::add_fillet(
                        &mut source,
                        &selected,
                        corner.as_deref(),
                        editor.value.clone(),
                        editor.construction,
                    )?);
                }
            }
            Kind::Chamfer => {
                let size = editor.chamfer_size();
                if let Some(key) = &editor.operation {
                    let c = source.chamfers.get_mut(key).ok_or_else(|| {
                        CadError::Invalid("Chamfer operation no longer exists".into())
                    })?;
                    c.size = size;
                    if let Some(member) = source.members.get_mut(&c.member) {
                        member.construction = editor.construction;
                    }
                } else {
                    editor.operation = Some(edit::add_chamfer(
                        &mut source,
                        &selected,
                        corner.as_deref(),
                        size,
                        editor.construction,
                    )?);
                }
            }
            Kind::Mirror
            | Kind::DynamicMirror
            | Kind::LinearPattern
            | Kind::CircularPattern
            | Kind::MoveCopy
            | Kind::Rotate
            | Kind::Scale => {
                let intent = editor.transform();
                if let Some(key) = &editor.operation {
                    let mut pattern = source
                        .transforms
                        .remove(key)
                        .or_else(|| editor.before.transforms.get(key).cloned())
                        .ok_or_else(|| {
                            CadError::Invalid("Sketch transformation no longer exists".into())
                        })?;
                    pattern.transform = intent;
                    transform::set_dynamic(&source, &mut pattern, editor.dynamic)?;
                    pattern.dynamic_construction = editor.dynamic && editor.construction;
                    if !pattern.sources.is_empty() || pattern.dynamic {
                        transform::resize(&mut source, &mut pattern, editor.construction)?;
                        for (instance, controls) in
                            pattern.instances.iter_mut().zip(&editor.instance_controls)
                        {
                            instance.suppressed = controls.suppressed;
                            instance.placement = controls.placement.clone();
                        }
                        editor.instance_controls = pattern.instances.clone();
                        source.transforms.insert(key.clone(), pattern);
                    }
                } else if matches!(editor.kind, Kind::MoveCopy | Kind::Rotate | Kind::Scale)
                    && !editor.copy
                {
                    transform::move_members(
                        w.session.visible(),
                        &owner,
                        &mut source,
                        &selected,
                        &intent,
                    )?;
                    for key in &selected {
                        if let Some(m) = source.members.get_mut(key) {
                            m.construction = editor.construction;
                        }
                    }
                } else {
                    editor.operation = Some(if editor.dynamic {
                        transform::add_dynamic(
                            &mut source,
                            &selected,
                            editor.axis.clone(),
                            editor.construction,
                        )?
                    } else {
                        transform::add(&mut source, &selected, intent, editor.construction)?
                    });
                    editor.instance_controls = source.transforms
                        [editor.operation.as_ref().unwrap()]
                    .instances
                    .clone();
                }
            }
            Kind::Convert => {
                if editor.sources.is_empty() {
                    return Err(CadError::Invalid(
                        "Select model edges or faces to convert".into(),
                    ));
                }
                let evaluated = viewport::display_evaluation(w).ok_or_else(|| {
                    CadError::Invalid("A current model evaluation is required".into())
                })?;
                let mut seen = std::collections::BTreeSet::new();
                for selected in &editor.sources {
                    if selected.definition != owner {
                        return Err(CadError::Unsupported(
                            "Convert Entities currently uses geometry of the active part".into(),
                        ));
                    }
                    let instance = evaluated
                        .instances
                        .iter()
                        .find(|i| i.path == selected.path && i.definition == selected.definition)
                        .ok_or_else(|| {
                            CadError::Invalid("Source component is no longer visible".into())
                        })?;
                    let key = if instance.mesh_key.is_empty() {
                        &instance.definition
                    } else {
                        &instance.mesh_key
                    };
                    let prefix = format!("{key}::");
                    let edges = evaluated
                        .meshes
                        .get(key)
                        .into_iter()
                        .chain(
                            evaluated
                                .reference_curve_meshes
                                .iter()
                                .filter(|(id, _)| id.starts_with(&prefix))
                                .map(|(_, mesh)| mesh),
                        )
                        .flat_map(|mesh| mesh.edges.iter())
                        .filter(|edge| {
                            if selected
                                .body
                                .as_ref()
                                .is_some_and(|body| edge.body.as_ref() != Some(body))
                            {
                                return false;
                            }
                            if selected.reference.topology == "edge" {
                                edge.reference.producer == selected.reference.producer
                                    && edge.reference.role == selected.reference.role
                            } else {
                                edge.faces.iter().any(|f| {
                                    f.producer == selected.reference.producer
                                        && f.role == selected.reference.role
                                })
                            }
                        })
                        .collect::<Vec<_>>();
                    if edges.is_empty() {
                        return Err(CadError::Invalid(
                            "Selected face/edge boundary is unavailable".into(),
                        ));
                    }
                    for edge in edges {
                        let provider = edge.body.as_ref().ok_or_else(|| {
                            CadError::Invalid(
                                "Source edge has no owning body; rebuild geometry".into(),
                            )
                        })?;
                        if !seen.insert((
                            provider.clone(),
                            edge.reference.producer.clone(),
                            edge.reference.role.clone(),
                        )) {
                            continue;
                        }
                        if source.conversions.values().any(|c| {
                            &c.body == provider
                                && c.reference.producer == edge.reference.producer
                                && c.reference.role == edge.reference.role
                        }) {
                            continue;
                        }
                        let curve = edge.curve.as_ref().ok_or_else(|| {
                            CadError::Unsupported(
                                "This edge has no exact convertible curve representation".into(),
                            )
                        })?;
                        let curve = edit::project(curve, &plane)?;
                        edit::insert_conversion(
                            &mut source,
                            curve,
                            editor
                                .linked
                                .then(|| (provider.clone(), edge.reference.clone())),
                            editor.construction,
                        )?;
                    }
                }
            }
        }
        Ok(())
    })();
    editor.error = result.as_ref().err().map(ToString::to_string);
    editor.previewed = result.is_ok();
    if let Some(s) = &mut w.sketch {
        s.edit = Some(editor);
    }
    match result {
        Ok(()) => {
            // Retain source line topology for fillets, but seed new output curves and
            // handles from their exact construction before the normal solve.
            let mut derived = source.clone();
            if edit::derive(w.session.visible(), &owner, &mut derived).is_ok() {
                source.points = derived.points;
                for key in edit::modifier_ids(&source)
                    .iter()
                    .flat_map(|id| edit::modifier_members(&source, id))
                    .collect::<Vec<_>>()
                {
                    if let Some(member) = derived.members.get(&key) {
                        source.members.insert(key, member.clone());
                    }
                }
            }
            load(w, source);
        }
        Err(e) => {
            w.status = e.to_string();
        }
    }
}
pub fn cancel(w: &mut Workspace) {
    crate::sketch_handles::end(w);
    let before = w.sketch.as_mut().and_then(|s| s.edit.take()).map(|e| {
        (
            e.before,
            e.history_len,
            (e.kind == Kind::Text).then_some(e.text_resources_before),
        )
    });
    if let Some((before, history_len, resources)) = before {
        if let Some(s) = &mut w.sketch {
            s.history.truncate(history_len);
        }
        load(w, before);
        if let Some(resources) = resources {
            let mut document = w.session.visible().clone();
            if document.resources != resources {
                document.resources = resources;
                w.dispatch(cad_core::commands::Command::ReplaceDocument { document });
            }
        }
        w.status = "Sketch operation cancelled".into();
    }
}
fn field(ui: &mut egui::Ui, label: &str, value: &mut Expr, hint: &str) -> bool {
    ui.label(label);
    ui.add(
        egui::TextEdit::singleline(&mut value.0)
            .id_salt(("sketch-edit", label))
            .hint_text(hint),
    )
    .changed()
}
pub fn parameterization(
    ui: &mut egui::Ui,
    value: &mut cad_core::sketch_spline::Parameterization,
) -> bool {
    use cad_core::sketch_spline::Parameterization;
    let old = *value;
    egui::ComboBox::from_id_salt("spline-parameterization")
        .selected_text(match value {
            Parameterization::Uniform => "Uniform",
            Parameterization::Chord => "Chord length",
            Parameterization::Centripetal => "Centripetal",
        })
        .show_ui(ui, |ui| {
            ui.selectable_value(value, Parameterization::Chord, "Chord length");
            ui.selectable_value(value, Parameterization::Centripetal, "Centripetal");
            ui.selectable_value(value, Parameterization::Uniform, "Uniform");
        });
    old != *value
}
fn spline_controls(
    ui: &mut egui::Ui,
    definition: &mut cad_core::sketch_spline::Definition,
    member: Option<&SketchMember>,
) -> bool {
    use cad_core::sketch_spline::Definition;
    let mut changed = false;
    ui.label(definition.label());
    let count = definition.weights().len();
    let old_degree = definition.degree();
    changed |= ui
        .add(
            egui::DragValue::new(definition.degree_mut())
                .range(1..=cad_core::sketch_spline::MAX_DEGREE.min(count.saturating_sub(1).max(1)))
                .prefix("Degree "),
        )
        .changed();
    if old_degree != definition.degree() {
        let knots =
            cad_core::sketch_spline::literal_knots(count, definition.degree()).unwrap_or_default();
        match definition {
            Definition::Control { knots: current, .. } => *current = knots,
            Definition::Fit { knots: current, .. } => *current = None,
        }
    }
    ui.small(format!(
        "{count} controls · clamped knots · positive weights"
    ));
    if let Definition::Fit {
        parameterization: mode,
        knots,
        ..
    } = definition
    {
        changed |= parameterization(ui, mode);
        let mut custom = knots.is_some();
        if ui
            .checkbox(&mut custom, "Custom interpolation knots")
            .changed()
        {
            *knots = if custom {
                Some(
                    if let Some(SketchMember {
                        curve: SketchCurve::Nurbs { knots, .. },
                        ..
                    }) = member
                    {
                        knots.iter().map(|value| Expr(value.to_string())).collect()
                    } else {
                        vec![]
                    },
                )
            } else {
                None
            };
            changed = true;
        }
        ui.small("The curve passes through every fit point. Automatic knots follow the selected parameterization. Repeated end points close the curve at a clamped seam.");
    }
    egui::CollapsingHeader::new("Control weights")
        .default_open(true)
        .show(ui, |ui| {
            if ui.small_button("Reset weights to 1").clicked() {
                definition.weights_mut().fill(Expr("1".into()));
                changed = true;
            }
            egui::ScrollArea::vertical()
                .id_salt("spline-weights")
                .max_height(180.)
                .show(ui, |ui| {
                    for (index, weight) in definition.weights_mut().iter_mut().enumerate() {
                        ui.push_id(index, |ui| {
                            changed |= field(
                                ui,
                                &format!("Weight {}", index + 1),
                                weight,
                                "positive scalar or parameter",
                            );
                        });
                    }
                });
        });
    let degree = definition.degree();
    let knots = match definition {
        Definition::Control { knots, .. } => Some(knots),
        Definition::Fit { knots, .. } => knots.as_mut(),
    };
    if let Some(knots) = knots {
        egui::CollapsingHeader::new("Knot vector").show(ui, |ui| {
            ui.small("Knots must be nondecreasing, clamped at both ends, and have a nonzero parameter domain. Interior multiplicity is limited to the degree.");
            if ui.small_button("Use uniform clamped knots").clicked() {
                *knots = cad_core::sketch_spline::literal_knots(count, degree).unwrap_or_default();
                changed = true;
            }
            egui::ScrollArea::vertical().id_salt("spline-knots").max_height(220.).show(ui, |ui| {
                for (index, knot) in knots.iter_mut().enumerate() {
                    ui.push_id(index, |ui| { changed |= field(ui, &format!("Knot {}", index + 1), knot, "scalar or parameter"); });
                }
            });
        });
    }
    changed
}
fn conic_controls(ui: &mut egui::Ui, definition: &mut cad_core::sketch_conic::Definition) -> bool {
    use cad_core::sketch_conic::Definition;
    let mut changed = false;
    ui.label(definition.label());
    match definition {
        Definition::Conic { rho, .. } => {
            changed |= field(ui, "Rho", rho, "0 < rho < 1, or a parameter");
            ui.small("Rho < 0.5: elliptical · 0.5: parabolic · rho > 0.5: hyperbolic. The middle handle is the tangent intersection.");
        }
        Definition::Parabola { .. } => {
            ui.small("Exact parabolic segment. Drag its endpoints or tangent intersection to reshape it.");
        }
        Definition::EllipseArc { start, sweep, .. } => {
            changed |= field(ui, "Start angle", start, "0 deg or a parameter");
            changed |= field(ui, "Sweep angle", sweep, "180 deg or a parameter");
            ui.small("Sweep may run in either direction up to 360°. The major handle sets its first axis; the minor handle sets the perpendicular second radius.");
        }
        Definition::ArcSlot {
            width, clockwise, ..
        } => {
            changed |= field(ui, "Slot width", width, "2 mm or a parameter");
            changed |= ui.checkbox(clockwise, "Clockwise centerline arc").changed();
            ui.small("Move the center, radius/start, and end-direction handles. The inner radius remains positive and end caps must not overlap.");
        }
    }
    changed
}
fn source_point_controls(
    ui: &mut egui::Ui,
    points: &[Id],
    positions: &mut std::collections::BTreeMap<Id, [f64; 2]>,
    changed: &mut bool,
) {
    egui::CollapsingHeader::new("Source point coordinates").show(ui, |ui| {
        ui.small("Positions follow the sketch dimensions and relations.");
        for (index, point) in points.iter().enumerate() {
            if let Some(position) = positions.get_mut(point) {
                ui.push_id(point, |ui| {
                    ui.horizontal(|ui| {
                        ui.label((index + 1).to_string());
                        *changed |= ui
                            .add(
                                egui::DragValue::new(&mut position[0])
                                    .speed(0.1)
                                    .prefix("X ")
                                    .suffix(" mm"),
                            )
                            .changed();
                        *changed |= ui
                            .add(
                                egui::DragValue::new(&mut position[1])
                                    .speed(0.1)
                                    .prefix("Y ")
                                    .suffix(" mm"),
                            )
                            .changed();
                    });
                });
            }
        }
    });
}
pub fn controls(ui: &mut egui::Ui, w: &mut Workspace) -> bool {
    if w.sketch.as_ref().is_none_or(|s| s.edit.is_none()) {
        return false;
    }
    let valid = w
        .session
        .draft
        .as_ref()
        .and_then(|d| d.evaluation.as_ref())
        .is_some_and(|e| e.valid && e.identity == w.session.identity())
        && w.sketch.as_ref().is_some_and(|s| s.error.is_none());
    let mut changed = false;
    let mut reselect = false;
    let mut accept = false;
    let mut discard = false;
    let mut font_import = None;
    let block_source = w.sketch.as_ref().map(sketch::sketch_definition);
    let block_owner = w.sketch.as_ref().map(|sketch| sketch.definition.clone());
    let block_doc = w.session.visible().clone();
    let dimensions_selection = w
        .sketch
        .as_ref()
        .map(|sketch| sketch.selection.clone())
        .unwrap_or_default();
    if let Some(s) = &mut w.sketch
        && let Some(e) = &mut s.edit
    {
        ui.strong(e.kind.label());
        ui.small(match e.kind {
            Kind::DimensionGroup=>"Dimension equations share a saved datum, axis and target order. Accept requires the current valid sketch preview.",
            Kind::Block=>"Local block source updates every instance. Move its insertion anchor or edit source controls while the preview remains pending.",
            Kind::Text=>"Local font outlines become exact sketch contours. Height is the font em-height. The first baseline uses the draggable anchor plus the saved X/Y offsets.",
            Kind::Equation=>"Save length-valued polynomial coefficients and dimensionless parameter limits. Exact Bézier geometry follows the source anchor and current dimensions/relations.",
            Kind::Conic => "Drag source handles in the viewport or edit the parameters below. Accept requires the current valid preview.",
            Kind::Spline => "Drag fit points or controls in the viewport. Degree, weights, and knots update the current preview; Cancel restores the starting curve.",
            Kind::Convert => "Click model edges or a face boundary. Shift-click adds another source.",
            Kind::Offset => "Select a line/arc chain, circle, exact straight/circular spline, or degree-one spline polyline. General ellipse/spline parallel curves are unsupported. Closed contours offset outward; Reverse offsets inward.",
            Kind::Fillet => "Select two joined lines/arcs or their corner. Exact adjacent curves are trimmed to a tangent circular fillet. One fillet per source arc is supported.",
            Kind::Chamfer => "Select two joined lines or their corner. Distances are measured along the adjacent members.",
            Kind::Mirror => "Select members to mirror, then choose an axis. Copies keep a parametric source relation.",
            Kind::LinearPattern => "Select members. Count includes the source; X/Y spacing is measured between successive instances.",
            Kind::CircularPattern => "Select members and a center. Equal spacing uses the total angle; 360° avoids a duplicate final instance.",
            Kind::Trim => "Click the portion to remove between its nearest intersections or ends. An unbounded selection removes the entire member.",
            Kind::Extend => "Click near the end to extend. The boundary is another bounded sketch member; an explicit length is also available.",
            Kind::Split => "Click an interior point to split. Closed curves split at the picked and opposite parameter; existing spline seams remain explicit.",
            Kind::Repair=>"Close unambiguous gaps with explicit coincidence relations. Select members to limit the search, or leave selection empty for the whole sketch. Curves and source identities are retained.",
            Kind::Stretch=>"Move only the chosen source controls. Unselected controls and all dimensions/relations remain active; generated controls must be edited through their source.",
            Kind::Replace=>"Use an existing exact curve as the replacement. The target member ID and compatible control references are retained. Incompatible relations are removed only when explicitly enabled.",
            Kind::DynamicMirror=>"Choose an axis and accept to mirror new entities as they are drawn. Existing mirrored entities keep their stable association when dynamic authoring is turned off.",
            _ => "Select members to transform. Existing constraints stay active; Copy makes associative geometry.",
        });
        if let Some(editor) = &mut e.dimension_group
            && let Some(source) = &block_source
        {
            changed |= crate::sketch_dimensions_ui::controls(
                ui,
                editor,
                source,
                &dimensions_selection,
                &block_doc.settings,
            );
        }
        if let Some(block) = &mut e.block
            && let (Some(source), Some(owner)) = (&block_source, &block_owner)
        {
            changed |= crate::sketch_blocks_ui::controls(ui, block, source, &block_doc, owner);
        }
        if e.kind == Kind::Equation {
            if let (Some(equation), Some(source)) = (&mut e.equation, &block_source) {
                changed |= crate::sketch_equation_ui::fields(ui, equation, source);
            }
        } else if e.kind == Kind::Text {
            ui.label(
                e.text_font
                    .as_ref()
                    .map(|font| format!("{} · face {}", font.name, font.face_index))
                    .unwrap_or_else(|| "Choose a local outline font".into()),
            );
            if ui.button("Choose font…").clicked() {
                font_import = rfd::FileDialog::new()
                    .add_filter("Outline fonts", &["ttf", "otf", "ttc", "otc"])
                    .pick_file();
            }
            changed |= ui
                .add(
                    egui::DragValue::new(&mut e.text_face)
                        .range(0..=255)
                        .prefix("Font face "),
                )
                .changed();
            changed |= ui
                .add(
                    egui::TextEdit::multiline(&mut e.text_content)
                        .desired_rows(3)
                        .hint_text("Sketch text"),
                )
                .changed();
            changed |= field(ui, "Em-height", &mut e.value, "10 mm or a parameter");
            changed |= field(ui, "Offset X", &mut e.x, "0 mm");
            changed |= field(ui, "Offset Y", &mut e.y, "0 mm");
            changed |= field(ui, "Rotation", &mut e.angle, "0 deg");
            changed |= field(ui, "Character spacing", &mut e.second, "0 mm");
            changed |= field(ui, "Line spacing / em", &mut e.scale, "1.2");
            egui::ComboBox::from_id_salt("sketch-text-alignment")
                .selected_text(format!("{:?}", e.text_alignment))
                .show_ui(ui, |ui| {
                    for alignment in [
                        cad_core::sketch_text::Alignment::Left,
                        cad_core::sketch_text::Alignment::Center,
                        cad_core::sketch_text::Alignment::Right,
                    ] {
                        changed |= ui
                            .selectable_value(
                                &mut e.text_alignment,
                                alignment,
                                format!("{alignment:?}"),
                            )
                            .changed();
                    }
                });
            if let Some(point) = e.spline_points.get_mut(&e.text_anchor) {
                ui.horizontal(|ui| {
                    ui.label("Anchor point");
                    changed |= ui
                        .add(
                            egui::DragValue::new(&mut point[0])
                                .speed(0.1)
                                .prefix("X ")
                                .suffix(" mm"),
                        )
                        .changed();
                    changed |= ui
                        .add(
                            egui::DragValue::new(&mut point[1])
                                .speed(0.1)
                                .prefix("Y ")
                                .suffix(" mm"),
                        )
                        .changed();
                });
            }
            ui.small("Unshaped left-to-right glyphs; no kerning, bidi, variable fonts or text-on-path. Font bytes are pinned into the project.");
        } else if e.kind == Kind::Repair {
            changed |= field(ui, "Endpoint gap tolerance", &mut e.value, "0.01 mm");
            changed |= ui
                .checkbox(&mut e.include_construction, "Include construction members")
                .changed();
            changed |= ui
                .checkbox(&mut e.linked, "Remove exact duplicate relations")
                .changed();
            if let Some(report) = &e.repair_report {
                ui.label(format!(
                    "{} endpoint pairs joined · {} duplicate relations removed",
                    report.joined_endpoints.len(),
                    report.removed_relations.len()
                ));
                ui.small(format!(
                    "{} eligible open endpoints remain in the repair scope",
                    report.remaining_open_endpoints
                ));
                if !report.ambiguous_endpoints.is_empty() {
                    ui.colored_label(ui.visuals().warn_fg_color,format!("{} endpoints have ambiguous neighbors; constrain those pairs explicitly",report.ambiguous_endpoints.len()));
                }
            }
        } else if e.kind == Kind::Stretch {
            changed |= field(ui, "Stretch X", &mut e.x, "0 mm");
            changed |= field(ui, "Stretch Y", &mut e.y, "0 mm");
            ui.label(format!(
                "{} source controls selected",
                e.stretch_points.len()
            ));
            let driven = edit::driven_points(&e.before);
            egui::ScrollArea::vertical()
                .id_salt("stretch-source-controls")
                .max_height(180.)
                .show(ui, |ui| {
                    for (index, (key, point)) in e.before.points.iter().enumerate() {
                        let mut selected = e.stretch_points.contains(key);
                        if ui
                            .add_enabled(
                                selected || !driven.contains(key),
                                egui::Checkbox::new(
                                    &mut selected,
                                    format!(
                                        "Point {} · {:.4}, {:.4} mm{}",
                                        index + 1,
                                        point[0],
                                        point[1],
                                        if driven.contains(key) {
                                            " · generated"
                                        } else {
                                            ""
                                        }
                                    ),
                                ),
                            )
                            .changed()
                        {
                            if selected {
                                e.stretch_points.push(key.clone());
                            } else {
                                e.stretch_points.retain(|point| point != key);
                            }
                            changed = true;
                        }
                    }
                });
        } else if e.kind == Kind::Replace {
            for (role, selection) in [
                ("Member to replace", &mut e.replace_target),
                ("Replacement curve", &mut e.replace_source),
            ] {
                egui::ComboBox::from_id_salt(role)
                    .selected_text(
                        selection
                            .as_ref()
                            .and_then(|key| {
                                e.before.members.keys().position(|member| member == key)
                            })
                            .map(|index| format!("Member {}", index + 1))
                            .unwrap_or_else(|| "Choose a member".into()),
                    )
                    .show_ui(ui, |ui| {
                        for (index, (key, record)) in e.before.members.iter().enumerate() {
                            let label = match record.curve {
                                SketchCurve::Line { .. } => "Line",
                                SketchCurve::Arc { .. } => "Arc",
                                SketchCurve::Circle { .. } => "Circle",
                                SketchCurve::Ellipse { .. } => "Ellipse",
                                SketchCurve::Bezier { .. } => "Bézier",
                                SketchCurve::Nurbs { .. } => "NURBS",
                            };
                            changed |= ui
                                .selectable_value(
                                    selection,
                                    Some(key.clone()),
                                    format!("Member {} · {label}", index + 1),
                                )
                                .changed();
                        }
                    });
            }
            changed |= ui
                .checkbox(&mut e.reverse, "Reverse replacement direction")
                .changed();
            changed |= ui
                .checkbox(
                    &mut e.consume_replacement,
                    "Consume replacement member and retain its point references",
                )
                .changed();
            changed |= ui
                .checkbox(
                    &mut e.remove_incompatible,
                    "Remove incompatible relations and dimensions",
                )
                .changed();
            if let Some(report) = &e.replace_report {
                ui.small(format!(
                    "{} original controls retained · {} incompatible relations removed",
                    report.preserved_points.len(),
                    report.removed_relations.len()
                ));
            }
        } else if e.kind == Kind::Conic {
            if let Some(definition) = &mut e.conic_definition {
                changed |= conic_controls(ui, definition);
                source_point_controls(
                    ui,
                    &definition.source_points(),
                    &mut e.spline_points,
                    &mut changed,
                );
            } else {
                ui.label("Select a conic, partial ellipse, parabola, or arc slot, then Use selected members.");
            }
        } else if e.kind == Kind::Spline {
            if let Some(definition) = &mut e.spline_definition {
                changed |= spline_controls(
                    ui,
                    definition,
                    e.curve_target.as_ref().and_then(|key| s.members.get(key)),
                );
                let points = match definition {
                    cad_core::sketch_spline::Definition::Fit { points, .. } => points.clone(),
                    _ => e
                        .curve_target
                        .as_ref()
                        .and_then(|key| s.members.get(key))
                        .map(|member| {
                            member
                                .curve
                                .points()
                                .iter()
                                .map(|key| (*key).to_owned())
                                .collect()
                        })
                        .unwrap_or_default(),
                };
                egui::CollapsingHeader::new("Point coordinates").show(ui, |ui| {
                    ui.small("Positions follow the sketch dimensions and relations.");
                    for (index, point) in points.iter().enumerate() {
                        if let Some(position) = e.spline_points.get_mut(point) {
                            ui.push_id(point, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(format!("{}", index + 1));
                                    changed |= ui
                                        .add(
                                            egui::DragValue::new(&mut position[0])
                                                .speed(0.1)
                                                .prefix("X ")
                                                .suffix(" mm"),
                                        )
                                        .changed();
                                    changed |= ui
                                        .add(
                                            egui::DragValue::new(&mut position[1])
                                                .speed(0.1)
                                                .prefix("Y ")
                                                .suffix(" mm"),
                                        )
                                        .changed();
                                });
                            });
                        }
                    }
                });
            } else {
                ui.label("Select a spline, then Use selected members.");
            }
        } else if e.kind.curve_edit_kind().is_some() {
            let target_name = e
                .curve_target
                .as_ref()
                .and_then(|key| e.before.members.get(key))
                .map(|member| match member.curve {
                    SketchCurve::Line { .. } => "Line",
                    SketchCurve::Arc { .. } => "Arc",
                    SketchCurve::Circle { .. } => "Circle",
                    SketchCurve::Ellipse { .. } => "Ellipse",
                    SketchCurve::Bezier { .. } => "Bézier spline",
                    SketchCurve::Nurbs { .. } => "Rational spline",
                })
                .unwrap_or("Click a member in the viewport");
            ui.label(target_name);
            if e.curve_target.is_some() {
                ui.horizontal(|ui| {
                    ui.label(if e.kind == Kind::Extend {
                        "Choose end"
                    } else {
                        "Position along member"
                    });
                    changed |= ui
                        .add(egui::Slider::new(&mut e.curve_parameter, 0. ..=1.).fixed_decimals(4))
                        .changed();
                });
            }
            if e.kind == Kind::Extend {
                changed |= ui
                    .checkbox(&mut e.curve_to_intersection, "To next intersection")
                    .changed();
                if !e.curve_to_intersection {
                    changed |= field(ui, "Extension length", &mut e.value, "10 mm or a parameter");
                }
            }
            if e.kind != Kind::Split {
                changed |= ui
                    .checkbox(
                        &mut e.include_construction,
                        "Use construction members as boundaries",
                    )
                    .changed();
            }
            if let Some(report) = &e.curve_report {
                ui.label(if report.deleted_member {
                    "Preview: entire member removed".into()
                } else {
                    format!(
                        "Preview: {} resulting member{}",
                        report.members.len(),
                        if report.members.len() == 1 { "" } else { "s" }
                    )
                });
                if !report.removed_constraints.is_empty() {
                    ui.small(format!(
                        "{} relations or dimensions of the changed span will be removed",
                        report.removed_constraints.len()
                    ));
                }
            }
        } else if e.kind == Kind::Convert {
            ui.label(format!("{} model selections", e.sources.len()));
            changed |= ui.checkbox(&mut e.linked, "Keep source relation").changed();
        } else if e.kind.transform_command().is_some() {
            if matches!(e.kind, Kind::Mirror | Kind::DynamicMirror) {
                changed |= ui
                    .checkbox(&mut e.dynamic, "Dynamic mirror newly drawn entities")
                    .changed();
                let current = match &e.axis {
                    transform::Axis::X => "Sketch X axis",
                    transform::Axis::Y => "Sketch Y axis",
                    _ => "Selected sketch line",
                };
                egui::ComboBox::from_id_salt("sketch-mirror-axis")
                    .selected_text(current)
                    .show_ui(ui, |ui| {
                        changed |= ui
                            .selectable_value(&mut e.axis, transform::Axis::X, "Sketch X axis")
                            .changed();
                        changed |= ui
                            .selectable_value(&mut e.axis, transform::Axis::Y, "Sketch Y axis")
                            .changed();
                        for (index, (key, member)) in s.members.iter().enumerate() {
                            if matches!(member.curve, SketchCurve::Line { .. }) {
                                changed |= ui
                                    .selectable_value(
                                        &mut e.axis,
                                        transform::Axis::Member {
                                            member: key.clone(),
                                        },
                                        format!(
                                            "Line {}{}",
                                            index + 1,
                                            if member.construction {
                                                " · construction"
                                            } else {
                                                ""
                                            }
                                        ),
                                    )
                                    .changed();
                            }
                        }
                    });
            }
            if matches!(e.kind, Kind::LinearPattern | Kind::CircularPattern) {
                ui.horizontal(|ui| {
                    ui.label("Instances (including source)");
                    changed |= ui
                        .add(egui::DragValue::new(&mut e.count).range(2..=256))
                        .changed();
                });
            }
            if matches!(e.kind, Kind::LinearPattern | Kind::MoveCopy)
                || (e.operation.is_some() && matches!(e.kind, Kind::Rotate | Kind::Scale))
            {
                changed |= field(ui, "X", &mut e.x, "10 mm");
                changed |= field(ui, "Y", &mut e.y, "0 mm");
            }
            if matches!(
                e.kind,
                Kind::CircularPattern | Kind::MoveCopy | Kind::Rotate
            ) || (e.operation.is_some() && e.kind == Kind::Scale)
            {
                changed |= field(ui, "Angle", &mut e.angle, "30 deg");
            }
            if matches!(e.kind, Kind::MoveCopy | Kind::Scale)
                || (e.operation.is_some() && e.kind == Kind::Rotate)
            {
                changed |= field(ui, "Scale factor", &mut e.scale, "1");
            }
            if e.kind == Kind::CircularPattern {
                changed |= ui
                    .checkbox(&mut e.equal_spacing, "Equal spacing over total angle")
                    .changed();
            }
            if matches!(
                e.kind,
                Kind::CircularPattern | Kind::MoveCopy | Kind::Rotate | Kind::Scale
            ) {
                let label = match e.center {
                    transform::Center::Origin => "Origin",
                    transform::Center::Point { .. } => "Sketch point",
                    _ => "Coordinates",
                };
                egui::ComboBox::from_id_salt("sketch-transform-center")
                    .selected_text(label)
                    .show_ui(ui, |ui| {
                        if ui
                            .selectable_label(
                                matches!(e.center, transform::Center::Origin),
                                "Origin",
                            )
                            .clicked()
                        {
                            e.center = transform::Center::Origin;
                            changed = true;
                        }
                        if ui
                            .selectable_label(
                                matches!(e.center, transform::Center::Coordinates { .. }),
                                "Coordinates",
                            )
                            .clicked()
                        {
                            if !matches!(e.center, transform::Center::Coordinates { .. }) {
                                e.center = transform::Center::Coordinates {
                                    x: "0 mm".into(),
                                    y: "0 mm".into(),
                                };
                            }
                            changed = true;
                        }
                        for (index, (key, position)) in s.keys.iter().zip(&s.points).enumerate() {
                            changed |= ui
                                .selectable_value(
                                    &mut e.center,
                                    transform::Center::Point { point: key.clone() },
                                    format!(
                                        "Point {} · {:.3}, {:.3} mm",
                                        index + 1,
                                        position[0],
                                        position[1]
                                    ),
                                )
                                .changed();
                        }
                    });
                if let transform::Center::Coordinates { x, y } = &mut e.center {
                    changed |= field(ui, "Center X", x, "0 mm");
                    changed |= field(ui, "Center Y", y, "0 mm");
                }
            }
            if matches!(e.kind, Kind::MoveCopy | Kind::Rotate | Kind::Scale) {
                changed |= ui
                    .add_enabled(
                        e.operation.is_none(),
                        egui::Checkbox::new(&mut e.copy, "Copy (keep source relation)"),
                    )
                    .changed();
            }
            if !e.instance_controls.is_empty() {
                ui.separator();
                ui.strong("Individual instances");
                egui::ComboBox::from_id_salt("sketch-pattern-instance")
                    .selected_text(format!(
                        "Instance {}",
                        e.instance_focus.min(e.instance_controls.len() - 1) + 2
                    ))
                    .show_ui(ui, |ui| {
                        for index in 0..e.instance_controls.len() {
                            ui.selectable_value(
                                &mut e.instance_focus,
                                index,
                                format!("Instance {}", index + 2),
                            );
                        }
                    });
                e.instance_focus = e.instance_focus.min(e.instance_controls.len() - 1);
                let instance = &mut e.instance_controls[e.instance_focus];
                changed |= ui
                    .checkbox(&mut instance.suppressed, "Suppress this instance")
                    .changed();
                let mut custom = instance.placement.is_some();
                if ui
                    .checkbox(&mut custom, "Additional placement for this instance")
                    .changed()
                {
                    instance.placement = custom.then(transform::Placement::default);
                    changed = true;
                }
                if let Some(placement) = &mut instance.placement {
                    changed |= field(ui, "Instance X", &mut placement.translation[0], "0 mm");
                    changed |= field(ui, "Instance Y", &mut placement.translation[1], "0 mm");
                    changed |= field(ui, "Instance angle", &mut placement.angle, "0 deg");
                    changed |= field(ui, "Instance scale", &mut placement.scale, "1");
                    let label = match &placement.center {
                        transform::Center::Origin => "Origin",
                        transform::Center::Point { .. } => "Source point",
                        transform::Center::Coordinates { .. } => "Coordinates",
                    };
                    let driven = edit::driven_points(&e.before);
                    egui::ComboBox::from_id_salt("sketch-instance-pivot")
                        .selected_text(label)
                        .show_ui(ui, |ui| {
                            changed |= ui
                                .selectable_value(
                                    &mut placement.center,
                                    transform::Center::Origin,
                                    "Origin",
                                )
                                .changed();
                            if ui
                                .selectable_label(
                                    matches!(
                                        placement.center,
                                        transform::Center::Coordinates { .. }
                                    ),
                                    "Coordinates",
                                )
                                .clicked()
                                && !matches!(
                                    placement.center,
                                    transform::Center::Coordinates { .. }
                                )
                            {
                                placement.center = transform::Center::Coordinates {
                                    x: "0 mm".into(),
                                    y: "0 mm".into(),
                                };
                                changed = true;
                            }
                            for (index, (key, point)) in e
                                .before
                                .points
                                .iter()
                                .enumerate()
                                .filter(|(_, (key, _))| !driven.contains(*key))
                            {
                                changed |= ui
                                    .selectable_value(
                                        &mut placement.center,
                                        transform::Center::Point { point: key.clone() },
                                        format!(
                                            "Point {} · {:.3}, {:.3} mm",
                                            index + 1,
                                            point[0],
                                            point[1]
                                        ),
                                    )
                                    .changed();
                            }
                        });
                    if let transform::Center::Coordinates { x, y } = &mut placement.center {
                        changed |= field(ui, "Instance pivot X", x, "0 mm");
                        changed |= field(ui, "Instance pivot Y", y, "0 mm");
                    }
                }
            }
        } else {
            if e.kind == Kind::Chamfer {
                egui::ComboBox::from_id_salt("sketch-chamfer-mode")
                    .selected_text(
                        ["Equal distances", "Two distances", "Distance and angle"]
                            [e.chamfer_mode.min(2)],
                    )
                    .show_ui(ui, |ui| {
                        for (mode, label) in
                            ["Equal distances", "Two distances", "Distance and angle"]
                                .iter()
                                .enumerate()
                        {
                            changed |= ui
                                .selectable_value(&mut e.chamfer_mode, mode, *label)
                                .changed();
                        }
                    });
            }
            changed |= field(
                ui,
                match e.kind {
                    Kind::Offset => "Offset distance",
                    Kind::Fillet => "Fillet radius",
                    _ => "Distance 1",
                },
                &mut e.value,
                "1 mm or a parameter",
            );
            if e.kind == Kind::Offset {
                changed |= ui.checkbox(&mut e.reverse, "Reverse direction").changed();
            }
            if e.kind == Kind::Chamfer {
                if e.chamfer_mode == 1 {
                    changed |= field(ui, "Distance 2", &mut e.second, "1 mm");
                }
                if e.chamfer_mode == 2 {
                    changed |= field(ui, "Angle", &mut e.angle, "45 deg");
                }
            }
        }
        if !matches!(
            e.kind,
            Kind::Repair
                | Kind::Stretch
                | Kind::Replace
                | Kind::Block
                | Kind::DimensionGroup
                | Kind::Equation
        ) {
            let construction_changed = ui
                .checkbox(&mut e.construction, "Construction geometry")
                .changed();
            if construction_changed && e.kind == Kind::Text {
                e.text_construction_override = true;
            }
            changed |= construction_changed;
        }
        if e.kind.curve_edit_kind().is_none()
            && e.kind != Kind::Text
            && e.kind != Kind::Block
            && e.kind != Kind::DimensionGroup
            && e.kind != Kind::Equation
            && (e.kind == Kind::Convert || e.operation.is_none())
            && ui
                .button(if e.kind == Kind::Convert {
                    "Preview selected entities"
                } else {
                    "Use selected members"
                })
                .clicked()
        {
            reselect = true;
        }
        ui.small("Live preview in the model viewport · pending until Apply Sketch");
        ui.horizontal(|ui| {
            accept = ui
                .add_enabled(
                    valid && e.previewed && !changed && !reselect,
                    egui::Button::new("Accept"),
                )
                .clicked();
            discard = ui.button("Cancel operation").clicked();
        });
        if let Some(error) = e.error.as_ref().or(s.error.as_ref()) {
            ui.colored_label(egui::Color32::from_rgb(180, 40, 30), error);
        }
    }
    if let Some(path) = font_import {
        match import_text_font(w, &path) {
            Ok(()) => {
                changed = true;
            }
            Err(error) => {
                if let Some(editor) = w.sketch.as_mut().and_then(|sketch| sketch.edit.as_mut()) {
                    editor.error = Some(error.to_string());
                    editor.previewed = false;
                }
                w.status = error.to_string();
            }
        }
    }
    if discard {
        cancel(w);
    } else if accept {
        accept_operation(w);
    } else if changed || reselect {
        match refresh_text_font(w) {
            Ok(()) => rebuild(w, reselect),
            Err(error) => {
                if let Some(editor) = w.sketch.as_mut().and_then(|sketch| sketch.edit.as_mut()) {
                    editor.error = Some(error.to_string());
                    editor.previewed = false;
                }
                w.status = error.to_string();
            }
        }
    }
    true
}
fn import_text_font(w: &mut Workspace, path: &std::path::Path) -> Result<()> {
    if std::fs::metadata(path)?.len() > cad_core::sketch_text::MAX_FONT_BYTES as u64 {
        return Err(CadError::Invalid("Font resource exceeds 32 MiB".into()));
    }
    let Some(editor) = w.sketch.as_ref().and_then(|sketch| sketch.edit.as_ref()) else {
        return Ok(());
    };
    let text = editor.text_content.clone();
    let face = editor.text_face;
    let root = w.root();
    let mut document = w.session.visible().clone();
    let key = cad_core::persistence::import_resource(&mut document, &root, path)?;
    let resource = &document.resources[&key];
    let bytes = std::fs::read(root.join(&resource.path))?;
    let font = cad_core::sketch_text::parse_font(&bytes, resource, face, &text)?;
    if let Some(editor) = w.sketch.as_mut().and_then(|sketch| sketch.edit.as_mut()) {
        editor.text_font = Some(font);
    }
    w.dispatch(cad_core::commands::Command::ReplaceDocument { document });
    Ok(())
}
fn refresh_text_font(w: &mut Workspace) -> Result<()> {
    let Some(editor) = w
        .sketch
        .as_ref()
        .and_then(|sketch| sketch.edit.as_ref())
        .filter(|editor| editor.kind == Kind::Text)
    else {
        return Ok(());
    };
    let Some(font) = editor.text_font.as_ref() else {
        return Ok(());
    };
    if font.face_index == editor.text_face
        && editor
            .text_content
            .chars()
            .filter(|character| *character != '\n')
            .all(|character| font.glyphs.contains_key(&character))
    {
        return Ok(());
    }
    let resource = w
        .session
        .visible()
        .resources
        .get(&font.resource)
        .ok_or_else(|| CadError::Invalid("Pinned text font resource is missing".into()))?;
    let path = w.root().join(&resource.path);
    if std::fs::metadata(&path)?.len() > cad_core::sketch_text::MAX_FONT_BYTES as u64 {
        return Err(CadError::Invalid("Font resource exceeds 32 MiB".into()));
    }
    let bytes = std::fs::read(path)?;
    let font = cad_core::sketch_text::parse_font(
        &bytes,
        resource,
        editor.text_face,
        &editor.text_content,
    )?;
    if let Some(editor) = w.sketch.as_mut().and_then(|sketch| sketch.edit.as_mut()) {
        editor.text_font = Some(font);
    }
    Ok(())
}
pub fn converting(w: &Workspace) -> bool {
    w.sketch
        .as_ref()
        .and_then(|s| s.edit.as_ref())
        .is_some_and(|e| e.kind == Kind::Convert)
}

/// Original source remains pickable as a faint overlay while its pending edit is
/// drawn in the same viewport. Clicks alter only the shared live draft.
pub fn curve_editing(w: &Workspace) -> bool {
    w.sketch
        .as_ref()
        .and_then(|s| s.edit.as_ref())
        .is_some_and(|e| e.kind.curve_edit_kind().is_some())
}
pub fn curve_edit_overlay(
    w: &mut Workspace,
    ui: &mut egui::Ui,
    rect: egui::Rect,
    response: &egui::Response,
) {
    let Some(s) = &w.sketch else { return };
    let Some(editor) = &s.edit else { return };
    let plane = s.plane.clone();
    let matrix = s.matrix;
    let owner = s.definition.clone();
    let before = editor.before.clone();
    let target = editor.curve_target.clone();
    let original = cad_core::sketch_geometry::polylines(w.session.visible(), &owner, &before)
        .unwrap_or_default();
    let preview = cad_core::sketch_geometry::polylines(
        w.session.visible(),
        &owner,
        &sketch::display_definition(s),
    )
    .unwrap_or_default();
    let pointer = response.hover_pos().or_else(|| {
        ui.input(|i| i.pointer.interact_pos())
            .filter(|p| rect.contains(*p))
    });
    let painter = ui.painter().with_clip_rect(rect);
    let color = ui.visuals().selection.stroke.color;
    let project = |p| viewport::project(w, rect, sketch::world(&plane, &matrix, p));
    let mut picked: Option<(f32, Id)> = None;
    for path in &original {
        let selected = target.as_ref() == Some(&path.member);
        for pair in path.points.windows(2) {
            if let (Some(a), Some(b)) = (project(pair[0]), project(pair[1])) {
                painter.line_segment(
                    [a, b],
                    egui::Stroke::new(
                        if selected { 2. } else { 1. },
                        color.gamma_multiply(if selected { 0.35 } else { 0.18 }),
                    ),
                );
                if let Some(pointer) = pointer {
                    let direction = b - a;
                    let parameter = ((pointer - a).dot(direction)
                        / direction.length_sq().max(1e-8))
                    .clamp(0., 1.);
                    let distance = pointer.distance(a + direction * parameter);
                    if distance < 8. && picked.as_ref().is_none_or(|(best, _)| distance < *best) {
                        picked = Some((distance, path.member.clone()));
                    }
                }
            }
        }
    }
    for path in preview {
        for pair in path.points.windows(2) {
            if let (Some(a), Some(b)) = (project(pair[0]), project(pair[1])) {
                painter.line_segment(
                    [a, b],
                    egui::Stroke::new(if path.construction { 1. } else { 2.5 }, color),
                );
            }
        }
    }
    if response.clicked()
        && let (Some((_, member)), Some(pointer)) = (picked, pointer)
        && let Some(position) = sketch::hit_plane(w, rect, pointer, &plane, &matrix)
    {
        match cad_core::sketch_trim::parameter_at(
            w.session.visible(),
            &owner,
            &before,
            &member,
            position,
        ) {
            Ok(parameter) => {
                if let Some(s) = &mut w.sketch
                    && let Some(editor) = &mut s.edit
                {
                    editor.curve_target = Some(member.clone());
                    editor.curve_parameter = parameter;
                    editor.construction = before.members[&member].construction;
                    s.selection = vec![SketchEntity::Member { member }];
                }
                rebuild(w, false);
            }
            Err(error) => {
                w.status = error.to_string();
                if let Some(editor) = w.sketch.as_mut().and_then(|s| s.edit.as_mut()) {
                    editor.error = Some(error.to_string());
                }
            }
        }
    }
}
pub fn conversion_click(w: &mut Workspace, response: &egui::Response, rect: egui::Rect) {
    if !(response.clicked() || response.secondary_clicked()) {
        return;
    }
    let Some(pointer) = response.interact_pointer_pos() else {
        return;
    };
    let definition = w
        .sketch
        .as_ref()
        .map(|s| s.definition.clone())
        .unwrap_or_default();
    let hit = viewport::pick(w, rect, pointer).into_iter().find(|s| {
        s.definition == definition && matches!(s.reference.topology.as_str(), "edge" | "face")
    });
    let additive = response
        .ctx
        .input(|i| i.modifiers.shift || i.modifiers.command);
    if let Some(s) = &mut w.sketch
        && let Some(editor) = &mut s.edit
    {
        if !additive {
            editor.sources.clear();
        }
        if let Some(hit) = hit {
            if let Some(index) = editor.sources.iter().position(|s| {
                s.reference.producer == hit.reference.producer
                    && s.reference.role == hit.reference.role
                    && s.body == hit.body
            }) {
                editor.sources.remove(index);
            } else {
                editor.sources.push(hit.clone());
            }
            w.selected = Some(hit);
        }
    }
    rebuild(w, false);
}

/// Saved editing intents remain available after accepting their local preview.
pub fn modifiers(ui: &mut egui::Ui, w: &mut Workspace) {
    let Some(s) = &w.sketch else { return };
    let source = sketch::sketch_definition(s);
    if edit::modifier_ids(&source).is_empty()
        && source.conversions.is_empty()
        && source.blocks.is_empty()
        && source.dimension_groups.is_empty()
    {
        return;
    }
    let mut edit = None;
    let mut remove = None;
    let mut detach = None;
    let mut pattern_edit = None;
    let mut text_edit = None;
    let mut equation_edit = None;
    let mut block_edit = None;
    let mut block_explode = None;
    let mut dimension_edit = None;
    let mut dimension_remove = None;
    let mut remove_empty_pattern = None;
    egui::CollapsingHeader::new("Sketch Operations")
        .default_open(true)
        .show(ui, |ui| {
            for (key, group) in &source.dimension_groups {
                ui.push_id((key, "dimension-group"), |ui| ui.horizontal_wrapped(|ui| {
                    ui.label(format!("{} · {} · {} entries", group.kind.label(), if group.axis == cad_core::sketch_dimensions::Axis::X { "X" } else { "Y" }, group.entries.len()));
                    if ui.small_button("Edit").clicked() {
                        dimension_edit = Some((key.clone(), group.kind));
                    }
                    if ui.small_button("×").on_hover_text("Remove the group and its native dimensions").clicked() {
                        dimension_remove = Some(key.clone());
                    }
                }));
            }
            for(key,definition)in &source.blocks {ui.push_id((key,"block-definition"),|ui|ui.horizontal_wrapped(|ui|{ui.label(format!("Block · {}",definition.name));if ui.small_button("Edit source").clicked(){block_edit=Some((crate::sketch_blocks_ui::Mode::Definition,key.clone()));}
if ui.small_button("Insert").clicked(){block_edit=Some((crate::sketch_blocks_ui::Mode::Insert,key.clone()));}}));}
            for(key,instance)in &source.block_instances {ui.push_id((key,"block-instance"),|ui|ui.horizontal_wrapped(|ui|{ui.label(format!("Instance · {}{}",source.blocks[&instance.definition].name,if instance.suppressed{" · suppressed"}else{""}));if ui.small_button("Properties").clicked(){block_edit=Some((crate::sketch_blocks_ui::Mode::Instance,key.clone()));}
if ui.add_enabled(!instance.suppressed,egui::Button::new("Explode").small()).clicked(){block_explode=Some(key.clone());}
if ui.small_button("×").clicked(){remove=instance.outputs().first().cloned();}}));}
            for (key,text) in &source.texts {
                ui.push_id((key,"text-intent"),|ui|ui.horizontal_wrapped(|ui|{ui.label(format!("Text · {} · {}",text.text.lines().next().unwrap_or_default(),text.height.0));if ui.small_button("Edit").clicked(){text_edit=Some(key.clone());}let member=text.outputs().into_iter().next();if ui.small_button("Detach").on_hover_text("Keep exact contours and release editable text/font intent").clicked(){detach=member.clone();}
if ui.small_button("×").on_hover_text("Remove text and dependent sketch geometry").clicked(){remove=member;}}));
            }
            for (key, equation) in &source.equations {
                ui.push_id((key, "equation-intent"), |ui| ui.horizontal_wrapped(|ui| {
                    ui.label(format!("Equation Curve · degree {}", equation.degree()));
                    if ui.small_button("Edit").clicked() { equation_edit = Some(key.clone()); }
                    if ui.small_button("Detach").on_hover_text("Keep the exact Bézier member and release polynomial intent").clicked() { detach = Some(key.clone()); }
                    if ui.small_button("×").on_hover_text("Remove this equation and dependent sketch geometry").clicked() { remove = Some(key.clone()); }
                }));
            }
            for (index, (key, offset)) in source.offsets.iter().enumerate() {
                ui.push_id(key, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(format!("Offset {} · {}", index + 1, offset.distance.0));
                        if ui.small_button("Edit").clicked() {
                            edit = offset
                                .members
                                .first()
                                .map(|m| (Kind::Offset, m.member.clone()));
                        }
                        if ui
                            .small_button("×")
                            .on_hover_text("Remove offset and dependent geometry")
                            .clicked()
                        {
                            remove = offset.members.first().map(|m| m.member.clone());
                        }
                    });
                });
            }
            for (index, (key, fillet)) in source.fillets.iter().enumerate() {
                ui.push_id(key, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(format!("Fillet {} · {}", index + 1, fillet.radius.0));
                        if ui.small_button("Edit").clicked() {
                            edit = Some((Kind::Fillet, fillet.arc.clone()));
                        }
                        if ui
                            .small_button("×")
                            .on_hover_text("Remove fillet and restore corner")
                            .clicked()
                        {
                            remove = Some(fillet.arc.clone());
                        }
                    });
                });
            }
            for (index, (key, chamfer)) in source.chamfers.iter().enumerate() {
                ui.push_id(key, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(format!("Chamfer {}", index + 1));
                        if ui.small_button("Edit").clicked() {
                            edit = Some((Kind::Chamfer, chamfer.member.clone()));
                        }
                        if ui
                            .small_button("×")
                            .on_hover_text("Remove chamfer and restore corner")
                            .clicked()
                        {
                            remove = Some(chamfer.member.clone());
                        }
                    });
                });
            }
            for (key, pattern) in &source.transforms {
                ui.push_id(key, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(format!(
                            "{} · {} instances · {} suppressed{}",
                            pattern.transform.label(),
                            pattern.instances.len() + 1,
                            pattern.instances.iter().filter(|instance|instance.suppressed).count(),
                            if pattern.dynamic {" · dynamic"} else {""},
                        ));
                        let member = transform::outputs(pattern).into_iter().next();
                        if ui.small_button("Edit").clicked() {
                            pattern_edit=Some(key.clone());
                        }
                        if ui
                            .small_button("Detach")
                            .on_hover_text(
                                "Keep generated geometry and release its source relation",
                            )
                            .clicked()
                        {
                            detach = member.clone();
                            if member.is_none(){remove_empty_pattern=Some(key.clone());}
                        }
                        if ui
                            .small_button("×")
                            .on_hover_text("Remove operation and dependent geometry")
                            .clicked()
                        {
                            if member.is_none(){remove_empty_pattern=Some(key.clone());}
                            remove = member;
                        }
                    });
                });
            }
            for (member, definition) in &source.splines {
                ui.push_id((member, "spline-intent"), |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(format!(
                            "{} · degree {}",
                            definition.label(),
                            definition.degree()
                        ));
                        if ui.small_button("Edit").clicked() {
                            edit = Some((Kind::Spline, member.clone()));
                        }
                        if ui
                            .small_button("Detach")
                            .on_hover_text(
                                "Retain exact evaluated NURBS and release fit/parameter intent",
                            )
                            .clicked()
                        {
                            detach = Some(member.clone());
                        }
                    });
                });
            }
            for (key, definition) in &source.conics {
                ui.push_id((key,"conic-intent"),|ui| { ui.horizontal_wrapped(|ui| {
                    ui.label(definition.label());
                    if ui.small_button("Edit").clicked() { edit = Some((Kind::Conic,key.clone())); }
                    if ui.small_button("Detach").on_hover_text("Keep exact evaluated geometry and release parametric source intent").clicked() { detach = Some(key.clone()); }
                }); });
            }
            for (index, (member, _)) in source.conversions.iter().enumerate() {
                ui.push_id(member, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(format!("Converted entity {}", index + 1));
                        if ui
                            .small_button("Detach")
                            .on_hover_text("Keep geometry and release its source relation")
                            .clicked()
                        {
                            detach = Some(member.clone());
                        }
                    });
                });
            }
        });
    if let Some((key, kind)) = dimension_edit {
        begin_dimension_group(w, kind, Some(key));
    } else if let Some(key) = dimension_remove {
        crate::sketch_dimensions_ui::delete(w, &key);
    } else if let Some((mode, key)) = block_edit {
        begin_block(w, mode, Some(key));
    } else if let Some(key) = block_explode {
        crate::sketch_ui::explode_block(w, &key);
    } else if let Some(key) = text_edit {
        begin_text(w, Some(key));
    } else if let Some(key) = equation_edit {
        begin_equation(w, Some(key));
    } else if let Some(key) = pattern_edit {
        begin_pattern(w, key);
    } else if let Some(key) = remove_empty_pattern {
        if let Some(s) = &mut w.sketch {
            let before = sketch::sketch_definition(s);
            let mut source = before.clone();
            source.transforms.remove(&key);
            s.history.push(before);
            sketch::restore(s, source);
        }
        sketch::solve(w);
    } else if let Some((kind, member)) = edit {
        crate::sketch_ui::select(w, Some(SketchEntity::Member { member }), false);
        begin(w, kind);
    } else if let Some(member) = remove {
        crate::sketch_ui::select(w, Some(SketchEntity::Member { member }), false);
        crate::sketch_ui::action(w, "sketch.delete-selection");
    } else if let Some(member) = detach {
        crate::sketch_ui::select(w, Some(SketchEntity::Member { member }), false);
        detach_conversion(w);
    }
}
pub fn detach_conversion(w: &mut Workspace) {
    crate::sketch_handles::end(w);
    let Some(s) = &w.sketch else { return };
    let before = sketch::sketch_definition(s);
    let mut source = before.clone();
    if let Err(error) = edit::derive(w.session.visible(), &s.definition, &mut source) {
        w.status = error.to_string();
        return;
    }
    for entity in &s.selection {
        if let SketchEntity::Member { member } = entity {
            if cad_core::sketch_blocks::instance_for_member(&source, member).is_some() {
                w.status = "Use Explode Block to release its source association".into();
                return;
            }
            source.conversions.remove(member);
            source.splines.remove(member);
            source.equations.remove(member);
            if let Some((key, _)) = cad_core::sketch_conic::operation_for(&source, member) {
                let key = key.clone();
                source.conics.remove(&key);
            }
            if let Some((key, _)) = cad_core::sketch_text::operation_for(&source, member) {
                let key = key.clone();
                source.texts.remove(&key);
            }
            let operations = source
                .transforms
                .iter()
                .filter(|(_, pattern)| transform::outputs(pattern).contains(member))
                .map(|(key, _)| key.clone())
                .collect::<Vec<_>>();
            for key in operations {
                let deleted = source.transforms[&key]
                    .instances
                    .iter()
                    .filter(|instance| instance.suppressed)
                    .flat_map(|instance| instance.members.values().cloned())
                    .collect::<std::collections::BTreeSet<_>>();
                let removed_controls = source.transforms[&key]
                    .instances
                    .iter()
                    .filter(|instance| instance.suppressed)
                    .flat_map(|instance| instance.points.values().cloned())
                    .collect::<std::collections::BTreeSet<_>>();
                source.transforms.remove(&key);
                edit::delete_members(&mut source, &deleted);
                let removed_controls = removed_controls
                    .into_iter()
                    .filter(|point| {
                        !source
                            .members
                            .values()
                            .any(|member| member.curve.points().contains(&point.as_str()))
                            && !edit::modifier_ids(&source).iter().any(|operation| {
                                edit::modifier_reference_points(&source, operation).contains(point)
                            })
                    })
                    .collect::<std::collections::BTreeSet<_>>();
                for point in &removed_controls {
                    source.points.remove(point);
                }
                source.constraints.retain(|constraint| {
                    !constraint
                        .entities
                        .iter()
                        .any(|point| removed_controls.contains(point))
                });
            }
        }
    }
    cad_core::sketch_dimensions::reconcile_removed(&mut source);
    let selection = s.selection.clone();
    if let Some(s) = &mut w.sketch {
        s.history.push(before);
        sketch::restore(s, source);
        s.selection = selection;
    }
    sketch::solve(w);
    w.status = "Geometry retained; source relation released".into();
}

/// Accept only the operation represented by the current validated shared draft.
pub fn accept_operation(w: &mut Workspace) {
    crate::sketch_handles::end(w);
    let ready = w.session.draft.as_ref().is_some_and(|d| {
        !d.pending
            && d.error.is_none()
            && d.evaluation
                .as_ref()
                .is_some_and(|e| e.valid && e.identity == w.session.identity())
    }) && w.sketch.as_ref().is_some_and(|s| {
        s.error.is_none()
            && s.edit
                .as_ref()
                .is_some_and(|e| e.previewed && e.error.is_none())
    });
    if !ready {
        w.status = "Wait for the current valid sketch preview before accepting".into();
        return;
    }
    if let Some(s) = &mut w.sketch
        && let Some(editor) = s.edit.take()
    {
        s.history.truncate(editor.history_len);
        s.history.push(editor.before);
        s.selection.clear();
        s.selected_member = None;
        s.selected = None;
        w.status = "Sketch operation accepted; Apply Sketch commits the shared draft".into();
    }
}
pub fn semantics(w: &Workspace) -> Vec<presentation::SemanticElement> {
    let Some(s) = &w.sketch else { return vec![] };
    let Some(editor) = &s.edit else { return vec![] };
    let residuals = if s.error.is_some() {
        cad_core::sketch_relations::conflict_residuals(
            w.session.visible(),
            &s.definition,
            &sketch::sketch_definition(s),
        )
        .ok()
    } else {
        None
    };
    vec![presentation::SemanticElement {
        id: format!("{}::edit", s.sketch_id),
        role: "sketch-edit".into(),
        label: editor.kind.label().into(),
        value: Some(
            editor
                .dimension_group
                .as_ref()
                .map(|editor| {
                    format!(
                        "{} {} dimensions",
                        editor.values.len(),
                        if editor.group.axis == cad_core::sketch_dimensions::Axis::X {
                            "X"
                        } else {
                            "Y"
                        }
                    )
                })
                .unwrap_or_else(|| editor.value.0.clone()),
        ),
        state: if editor.error.is_some() || s.error.is_some() {
            "invalid"
        } else if editor.previewed {
            "preview"
        } else {
            "selecting"
        }
        .into(),
        commands: vec!["sketch.accept-edit".into(), "sketch.cancel-edit".into()],
        relationships: vec![s.sketch_id.clone()],
        data: serde_json::json!({"version":edit::CONTRACT_VERSION,"constraint_residuals_mm_equivalent":residuals,"degrees_of_freedom":s.error.is_none().then_some(s.degrees_of_freedom).flatten(),"operation":editor.operation,"expression":editor.value,"second_expression":editor.second,"angle":editor.angle,"spline":editor.spline_definition,"conic":editor.conic_definition,"source_points":editor.spline_points,"equation":editor.equation.as_ref().map(|equation|serde_json::json!({"version":cad_core::sketch_equation::CONTRACT_VERSION,"id":equation.id,"anchor":equation.request.anchor,"anchor_position":equation.anchor_position,"x":equation.request.x,"y":equation.request.y,"range":equation.request.range,"angle":equation.request.angle,"construction":equation.request.construction})),"dimension_group":editor.dimension_group.as_ref().map(|editor|serde_json::json!({"id":editor.id,"group":editor.group,"constraints":editor.values.iter().map(|value|serde_json::json!({"id":value.constraint,"value":value.value,"driven":value.driven})).collect::<Vec<_>>() })),"block":editor.block.as_ref().map(|block|serde_json::json!({"version":cad_core::sketch_blocks::CONTRACT_VERSION,"definition_id":block.definition_id,"instance_id":block.instance_id,"definition":block.definition,"instance":block.instance,"anchor":block.anchor,"internal_degrees_of_freedom":block.local_dof})),"transform":editor.kind.transform_command().map(|_| editor.transform()),"chamfer_size":(editor.kind==Kind::Chamfer).then(||editor.chamfer_size()),"curve_edit":{"kind":editor.kind.curve_edit_kind(),"edit_id":editor.curve_edit_id,"target":editor.curve_target,"parameter":editor.curve_parameter,"to_intersection":editor.curve_to_intersection,"include_construction":editor.include_construction,"tolerance_mm":1e-7,"report":editor.curve_report},"copy":editor.copy,"reverse":editor.reverse,"construction":editor.construction,"linked":editor.linked,"sources":editor.sources.iter().map(|hit| &hit.reference).collect::<Vec<_>>(),"error":editor.error.as_ref().or(s.error.as_ref())}),
    }]
}
