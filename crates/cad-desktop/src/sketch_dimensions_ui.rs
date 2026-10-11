//! Docked grouped sketch dimensions over the shared sketch checkpoint.
use crate::{Workspace, sketch, viewport};
use bevy_egui::egui;
use cad_core::{sketch_actions::SketchEntity, sketch_dimensions as dimensions, *};

pub struct Editor {
    pub id: Id,
    pub group: dimensions::Group,
    pub values: Vec<dimensions::Value>,
    pub adding: Option<Id>,
    pub annotation_drag: Option<AnnotationDrag>,
}
impl Editor {
    pub fn new(
        source: &Sketch,
        selection: &[SketchEntity],
        kind: dimensions::Kind,
        key: Option<Id>,
    ) -> Self {
        if let Some(key) = key
            && let Some(group) = source.dimension_groups.get(&key)
        {
            return Self {
                id: key,
                group: group.clone(),
                values: dimensions::values(source, group).unwrap_or_default(),
                adding: None,
                annotation_drag: None,
            };
        }
        let points = dimensions::selected_points(source, selection);
        let datum = if selection.contains(&SketchEntity::Origin) {
            cad_core::sketch_actions::ORIGIN.into()
        } else {
            points
                .first()
                .cloned()
                .unwrap_or_else(|| cad_core::sketch_actions::ORIGIN.into())
        };
        let targets = points
            .into_iter()
            .filter(|point| point != &datum)
            .collect::<Vec<_>>();
        let values =
            dimensions::measured_values(source, kind, &datum, dimensions::Axis::X, &targets, false)
                .unwrap_or_default();
        let entries = values
            .iter()
            .map(|value| dimensions::Entry {
                point: value.point.clone(),
                constraint: value.constraint.clone(),
                label_offset: value.label_offset,
            })
            .collect();
        Self {
            id: id(),
            group: dimensions::Group {
                kind,
                datum,
                axis: dimensions::Axis::X,
                entries,
                offset: 10.,
                spacing: 5.,
            },
            values,
            adding: None,
            annotation_drag: None,
        }
    }
    pub fn refresh(&mut self, source: &Sketch) {
        if let Some(group) = source.dimension_groups.get(&self.id) {
            self.group = group.clone();
            if let Ok(values) = dimensions::values(source, group) {
                self.values = values;
            }
        }
    }
    fn update_entries(&mut self) {
        self.group.entries = self
            .values
            .iter()
            .map(|value| dimensions::Entry {
                point: value.point.clone(),
                constraint: value.constraint.clone(),
                label_offset: value.label_offset,
            })
            .collect();
    }
    pub fn apply(&mut self, source: &mut Sketch) -> Result<()> {
        self.update_entries();
        dimensions::set(source, &self.id, self.group.clone(), &self.values)
    }
    fn add_points(&mut self, source: &Sketch, points: &[Id]) -> Result<()> {
        let keys = points
            .iter()
            .filter(|point| {
                point.as_str() != self.group.datum
                    && !self.values.iter().any(|value| &value.point == *point)
            })
            .cloned()
            .collect::<Vec<_>>();
        let datum = if self.group.kind == dimensions::Kind::Chain {
            self.values
                .last()
                .map(|value| value.point.as_str())
                .unwrap_or(&self.group.datum)
        } else {
            &self.group.datum
        };
        let driven = self.values.iter().all(|value| value.driven) && !self.values.is_empty();
        let extra = dimensions::measured_values(
            source,
            self.group.kind,
            datum,
            self.group.axis,
            &keys,
            driven,
        )?;
        self.values.extend(extra);
        self.update_entries();
        Ok(())
    }
}
fn point_name(source: &Sketch, key: &str) -> String {
    if key == cad_core::sketch_actions::ORIGIN {
        return "Sketch Origin".into();
    }
    source
        .points
        .keys()
        .position(|point| point == key)
        .map(|index| format!("Point {}", index + 1))
        .unwrap_or_else(|| "Missing point".into())
}
pub fn controls(
    ui: &mut egui::Ui,
    editor: &mut Editor,
    source: &Sketch,
    selection: &[SketchEntity],
    settings: &cad_core::settings::ModelSettings,
) -> bool {
    let inactive = cad_core::sketch_transform::inactive_entities(source);
    let mut changed = false;
    egui::ComboBox::from_id_salt("dimension-group-kind")
        .selected_text(editor.group.kind.label())
        .show_ui(ui, |ui| {
            for kind in [
                dimensions::Kind::Ordinate,
                dimensions::Kind::Baseline,
                dimensions::Kind::Chain,
            ] {
                changed |= ui
                    .selectable_value(&mut editor.group.kind, kind, kind.label())
                    .changed();
            }
        });
    egui::ComboBox::from_id_salt("dimension-group-datum")
        .selected_text(point_name(source, &editor.group.datum))
        .show_ui(ui, |ui| {
            changed |= ui
                .selectable_value(
                    &mut editor.group.datum,
                    cad_core::sketch_actions::ORIGIN.into(),
                    "Sketch Origin",
                )
                .changed();
            for key in source.points.keys() {
                if !inactive.contains(key) && !editor.values.iter().any(|value| &value.point == key)
                {
                    changed |= ui
                        .selectable_value(
                            &mut editor.group.datum,
                            key.clone(),
                            point_name(source, key),
                        )
                        .changed();
                }
            }
        });
    ui.horizontal(|ui| {
        ui.label("Direction");
        changed |= ui
            .selectable_value(&mut editor.group.axis, dimensions::Axis::X, "Horizontal X")
            .changed();
        changed |= ui
            .selectable_value(&mut editor.group.axis, dimensions::Axis::Y, "Vertical Y")
            .changed();
    });
    ui.horizontal(|ui| {
        changed |= ui
            .add(
                egui::DragValue::new(&mut editor.group.offset)
                    .speed(0.25)
                    .prefix("Offset ")
                    .suffix(" mm"),
            )
            .changed();
        changed |= ui
            .add(
                egui::DragValue::new(&mut editor.group.spacing)
                    .speed(0.25)
                    .prefix("Spacing ")
                    .suffix(" mm"),
            )
            .changed();
    });
    ui.small("Signed values follow the chosen direction. Baseline and ordinate share the datum; chain follows the saved entry order. Direction/datum/order edits keep your authored values and update their equations.");
    ui.horizontal(|ui| {
        if ui.button("Make all driving").clicked() {
            for value in &mut editor.values {
                value.driven = false;
            }
            changed = true;
        }
        if ui.button("Make all reference").clicked() {
            for value in &mut editor.values {
                value.driven = true;
            }
            changed = true;
        }
    });
    let mut move_entry = None;
    let mut remove = None;
    egui::ScrollArea::vertical()
        .id_salt("dimension-group-entries")
        .max_height(280.)
        .show(ui, |ui| {
            let count = editor.values.len();
            for (index, value) in editor.values.iter_mut().enumerate() {
                ui.push_id(&value.constraint, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(format!(
                            "{} · {}",
                            index + 1,
                            point_name(source, &value.point)
                        ));
                        changed |= ui.checkbox(&mut value.driven, "Reference").changed();
                        if ui
                            .add_enabled(index > 0, egui::Button::new("↑").small())
                            .clicked()
                        {
                            move_entry = Some((index, index - 1));
                        }
                        if ui
                            .add_enabled(index + 1 < count, egui::Button::new("↓").small())
                            .clicked()
                        {
                            move_entry = Some((index, index + 1));
                        }
                        if ui.small_button("×").clicked() {
                            remove = Some(index);
                        }
                    });
                    if value.driven {
                        let a = if editor.group.kind == dimensions::Kind::Chain {
                            if index == 0 {
                                &editor.group.datum
                            } else {
                                &editor.group.entries[index - 1].point
                            }
                        } else {
                            &editor.group.datum
                        };
                        let measured =
                            cad_core::sketch_actions::entity_point(source, a).and_then(|a| {
                                cad_core::sketch_actions::entity_point(source, &value.point).map(
                                    |b| b[editor.group.axis.index()] - a[editor.group.axis.index()],
                                )
                            });
                        ui.label(
                            measured
                                .map(|value| format!("({})", settings.format_length(value)))
                                .unwrap_or_else(|error| error.to_string()),
                        );
                        ui.small(format!("Saved driving expression: {}", value.value.0));
                    } else {
                        changed |= ui.text_edit_singleline(&mut value.value.0).changed();
                    }
                    egui::CollapsingHeader::new("Label position").show(ui, |ui| {
                        changed |= ui
                            .add(
                                egui::DragValue::new(&mut value.label_offset[0])
                                    .speed(0.1)
                                    .prefix("X ")
                                    .suffix(" mm"),
                            )
                            .changed();
                        changed |= ui
                            .add(
                                egui::DragValue::new(&mut value.label_offset[1])
                                    .speed(0.1)
                                    .prefix("Y ")
                                    .suffix(" mm"),
                            )
                            .changed();
                    });
                });
            }
        });
    if let Some((from, to)) = move_entry {
        editor.values.swap(from, to);
        changed = true;
    }
    if let Some(index) = remove {
        editor.values.remove(index);
        changed = true;
    }
    let selected = dimensions::selected_points(source, selection);
    if ui
        .add_enabled(
            !selected.is_empty() && editor.values.len() < dimensions::MAX_ENTRIES,
            egui::Button::new("Add selected sketch points"),
        )
        .clicked()
    {
        match editor.add_points(source, &selected) {
            Ok(()) => changed = true,
            Err(error) => {
                ui.colored_label(ui.visuals().error_fg_color, error.to_string());
            }
        }
    }
    egui::ComboBox::from_id_salt("dimension-group-add-point")
        .selected_text(
            editor
                .adding
                .as_ref()
                .map(|key| point_name(source, key))
                .unwrap_or("Choose another point".into()),
        )
        .show_ui(ui, |ui| {
            for key in source.points.keys() {
                if key != &editor.group.datum
                    && !inactive.contains(key)
                    && !editor.values.iter().any(|value| &value.point == key)
                {
                    ui.selectable_value(
                        &mut editor.adding,
                        Some(key.clone()),
                        point_name(source, key),
                    );
                }
            }
        });
    if ui
        .add_enabled(
            editor
                .adding
                .as_ref()
                .is_some_and(|key| source.points.contains_key(key) && !inactive.contains(key))
                && editor.values.len() < dimensions::MAX_ENTRIES,
            egui::Button::new("Add point"),
        )
        .clicked()
        && let Some(key) = editor.adding.take()
    {
        match editor.add_points(source, &[key]) {
            Ok(()) => changed = true,
            Err(error) => {
                ui.colored_label(ui.visuals().error_fg_color, error.to_string());
            }
        }
    }
    editor.update_entries();
    changed
}
pub fn delete(w: &mut Workspace, key: &str) {
    let Some(s) = &w.sketch else { return };
    if s.edit.is_some() {
        w.status = "Accept or cancel the current sketch operation first".into();
        return;
    }
    let before = sketch::sketch_definition(s);
    let mut source = before.clone();
    match dimensions::remove(&mut source, key) {
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

#[derive(Clone)]
pub struct AnnotationDrag {
    constraint: Id,
    initial: [f64; 2],
    pointer: [f64; 2],
}
/// Layout lives in sketch-plane millimetres; rendering and styling remain replaceable.
pub fn annotations(w: &mut Workspace, ui: &mut egui::Ui, rect: egui::Rect) -> bool {
    if !w.view.sketch_annotation_visibility[0] {
        return false;
    }
    let Some(s) = &w.sketch else { return false };
    let source = sketch::sketch_definition(s);
    let geometry = sketch::display_definition(s);
    let owner = s.definition.clone();
    let plane = s.plane.clone();
    let matrix = s.matrix;
    let selected = s.selection.clone();
    let conflicts = if s.error.is_some() {
        cad_core::sketch_relations::conflict_residuals(w.session.visible(), &owner, &source)
            .unwrap_or_default()
    } else {
        std::collections::BTreeMap::new()
    };
    // Last-valid geometry may precede a current suppression change. Admission
    // and selection must use the current source even while its solve is pending.
    let inactive = cad_core::sketch_transform::inactive_entities(&source);
    let mut consumed = false;
    let mut click = None;
    let mut drag = None;
    let mut stop = false;
    for (key, group) in &geometry.dimension_groups {
        let Some(current_group) = source.dimension_groups.get(key) else {
            continue;
        };
        if dimensions::selection_constraint(&source, current_group).is_none() {
            continue;
        }
        let Ok(annotations) = dimensions::annotations(&geometry, group) else {
            continue;
        };
        for annotation in annotations {
            if inactive.contains(&annotation.point) {
                continue;
            }
            if let Some(key) = &annotation.constraint
                && (!current_group
                    .entries
                    .iter()
                    .any(|entry| &entry.constraint == key && entry.point == annotation.point)
                    || !source.constraints.iter().any(|constraint| {
                        &constraint.id == key
                            && !constraint.entities.iter().any(|key| inactive.contains(key))
                    }))
            {
                continue;
            }
            let Some(position) =
                viewport::project(w, rect, sketch::world(&plane, &matrix, annotation.label))
            else {
                continue;
            };
            let active = annotation.constraint.as_ref().is_some_and(|constraint| {
                selected.contains(&SketchEntity::Constraint {
                    constraint: constraint.clone(),
                })
            });
            let id = annotation
                .constraint
                .clone()
                .unwrap_or_else(|| format!("{key}::zero"));
            let region = presentation::Region {
                id: id.clone(),
                role: "sketch-annotation".into(),
                component: None,
                rect: [position.x, position.y, 0., 0.],
                classes: vec![
                    "dimension".into(),
                    "dimension-group".into(),
                    format!("{:?}", group.kind).to_lowercase(),
                ],
                resizable: false,
            };
            let props = w.theme.properties(
                &region,
                &w.view.active_layout,
                if active { "selected" } else { "ready" },
            );
            if props.get("display").is_some_and(|value| value == "none") {
                continue;
            }
            let number = |key: &str, default: f32| {
                props
                    .get(key)
                    .and_then(|value| value.trim_end_matches("px").parse::<f32>().ok())
                    .filter(|value| value.is_finite())
                    .unwrap_or(default)
            };
            let conflict = annotation
                .constraint
                .as_ref()
                .and_then(|constraint| conflicts.get(constraint));
            let color = if conflict.is_some() {
                ui.visuals().error_fg_color
            } else {
                props
                    .get("color")
                    .and_then(|value| crate::ui::color(value))
                    .unwrap_or(if active {
                        ui.visuals().selection.stroke.color
                    } else {
                        ui.visuals().text_color()
                    })
            };
            let painter = ui.painter().with_clip_rect(rect);
            let project = |point| viewport::project(w, rect, sketch::world(&plane, &matrix, point));
            for segment in &annotation.segments {
                if let (Some(a), Some(b)) = (project(segment[0]), project(segment[1])) {
                    painter.line_segment([a, b], egui::Stroke::new(0.9, color));
                }
            }
            for arrow in &annotation.arrows {
                if let (Some(tip), Some(toward)) = (project(arrow[0]), project(arrow[1])) {
                    let direction = toward - tip;
                    if direction.length() > 0.5 {
                        let direction = direction.normalized();
                        let normal = egui::vec2(-direction.y, direction.x);
                        for side in [-1., 1.] {
                            painter.line_segment(
                                [tip, tip + direction * 6. + normal * (side * 2.5)],
                                egui::Stroke::new(1., color),
                            );
                        }
                    }
                }
            }
            let constraint = annotation.constraint.as_ref().and_then(|key| {
                source
                    .constraints
                    .iter()
                    .find(|constraint| &constraint.id == key)
            });
            let text = if annotation.zero {
                format!(
                    "{} 0",
                    if group.axis == dimensions::Axis::X {
                        "X"
                    } else {
                        "Y"
                    }
                )
            } else if let Some(constraint) = constraint {
                if constraint.driven {
                    format!(
                        "({})",
                        w.session
                            .visible()
                            .settings
                            .format_length(annotation.measured)
                    )
                } else {
                    let mut text = constraint.value.0.clone();
                    let measured = w
                        .session
                        .visible()
                        .settings
                        .format_length(annotation.measured);
                    if expression::references(&text).is_ok_and(|references| !references.is_empty())
                    {
                        text.push_str(&format!(" = {measured}"));
                        text
                    } else {
                        measured
                    }
                }
            } else {
                w.session
                    .visible()
                    .settings
                    .format_length(annotation.measured)
            };
            let font = egui::FontId::new(
                number("font-size", 12.).clamp(8., 40.),
                if props
                    .get("font-family")
                    .is_some_and(|value| value == "monospace")
                {
                    egui::FontFamily::Monospace
                } else {
                    egui::FontFamily::Proportional
                },
            );
            let galley = painter.layout_no_wrap(text, font, color);
            let padding = number("padding", 3.).clamp(0., 20.);
            let label_rect = egui::Rect::from_center_size(
                position,
                galley.size() + egui::vec2(padding * 2., padding * 2.),
            );
            if !rect.intersects(label_rect) {
                continue;
            }
            let hit = ui.interact(
                label_rect,
                egui::Id::new(("sketch-group-annotation", key, &id)),
                egui::Sense::click_and_drag(),
            );
            painter.rect_filled(
                label_rect,
                number("border-radius", 2.).clamp(0., 20.),
                props
                    .get("background")
                    .and_then(|value| crate::ui::color(value))
                    .unwrap_or(ui.visuals().panel_fill.gamma_multiply(0.95)),
            );
            painter.galley(label_rect.min + egui::vec2(padding, padding), galley, color);
            if hit.clicked() || hit.secondary_clicked() {
                consumed = true;
                click = Some((key.clone(), annotation.constraint.clone(), hit.clone()));
            }
            if annotation.constraint.is_some() && (hit.dragged() || hit.drag_started()) {
                consumed = true;
                drag = Some((
                    key.clone(),
                    annotation.constraint.clone().unwrap(),
                    hit.clone(),
                ));
            }
            if hit.drag_stopped() {
                stop = true;
            }
            hit.on_hover_text(if let Some(residual)=conflict{format!("Unsatisfied equation residual {residual:.6} mm-equivalent. Edit this group or its relations; this is not a minimal conflict diagnosis.")}else{format!("{} · double-click to edit group · drag label to reposition in the sketch",group.kind.label())});
        }
    }
    if let Some((key, constraint, response)) = click {
        let constraint = constraint.or_else(|| {
            source
                .dimension_groups
                .get(&key)
                .and_then(|group| dimensions::selection_constraint(&source, group))
                .cloned()
        });
        if let Some(constraint) = constraint {
            crate::sketch_ui::select(w, Some(SketchEntity::Constraint { constraint }), false);
            if response.double_clicked() {
                if w.sketch.as_ref().is_some_and(|s| s.edit.is_none()) {
                    crate::sketch_edit_ui::begin_dimension_group(
                        w,
                        geometry.dimension_groups[&key].kind,
                        Some(key),
                    );
                }
            } else {
                crate::selection_ui::open(w, &response, response.rect.right_bottom());
            }
        }
    }
    if let Some((key, constraint, response)) = drag {
        if w.sketch.as_ref().is_some_and(|s| s.edit.is_none()) {
            crate::sketch_edit_ui::begin_dimension_group(
                w,
                geometry.dimension_groups[&key].kind,
                Some(key.clone()),
            );
        }
        w.selection_popup = None;
        let pointer = response.interact_pointer_pos();
        let start = pointer
            .zip(response.total_drag_delta())
            .map(|(pointer, delta)| pointer - delta);
        let current =
            pointer.and_then(|pointer| sketch::hit_plane(w, rect, pointer, &plane, &matrix));
        let start = start.and_then(|pointer| sketch::hit_plane(w, rect, pointer, &plane, &matrix));
        let mut changed = false;
        if let Some(editor) = w
            .sketch
            .as_mut()
            .and_then(|s| s.edit.as_mut())
            .and_then(|editor| editor.dimension_group.as_mut())
            .filter(|editor| editor.id == key)
            && let Some(current) = current
        {
            if editor
                .annotation_drag
                .as_ref()
                .is_none_or(|drag| drag.constraint != constraint)
                && let Some(pointer) = start
                && let Some(value) = editor
                    .values
                    .iter()
                    .find(|value| value.constraint == constraint)
            {
                editor.annotation_drag = Some(AnnotationDrag {
                    constraint: constraint.clone(),
                    initial: value.label_offset,
                    pointer,
                });
            }
            if let Some(drag) = &editor.annotation_drag
                && let Some(value) = editor
                    .values
                    .iter_mut()
                    .find(|value| value.constraint == constraint)
            {
                let offset = [
                    drag.initial[0] + current[0] - drag.pointer[0],
                    drag.initial[1] + current[1] - drag.pointer[1],
                ];
                if offset.iter().all(|value| value.is_finite()) && value.label_offset != offset {
                    value.label_offset = offset;
                    changed = true;
                }
            }
        }
        if changed {
            crate::sketch_edit_ui::rebuild(w, false);
        }
    }
    if stop
        && let Some(editor) = w
            .sketch
            .as_mut()
            .and_then(|s| s.edit.as_mut())
            .and_then(|editor| editor.dimension_group.as_mut())
    {
        editor.annotation_drag = None;
    }
    consumed
}
