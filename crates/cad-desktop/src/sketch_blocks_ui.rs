//! Docked block source/instance editing, sharing the ordinary sketch transaction.
use bevy_egui::egui;
use cad_core::{
    sketch_actions::{self, SketchEntity},
    sketch_blocks as blocks, *,
};
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Create,
    Insert,
    Definition,
    Instance,
}
pub struct Editor {
    pub mode: Mode,
    pub definition_id: Option<Id>,
    pub instance_id: Option<Id>,
    pub definition: Option<blocks::Definition>,
    pub instance: Option<blocks::Instance>,
    pub selected: Vec<Id>,
    pub name: String,
    pub origin: [f64; 2],
    pub anchor: [f64; 2],
    pub selection: Vec<SketchEntity>,
    pub relation: usize,
    pub local_dof: Option<usize>,
}
impl Editor {
    pub fn new(source: &Sketch, selected: &[SketchEntity], mode: Mode, key: Option<Id>) -> Self {
        let selected_members = selected
            .iter()
            .filter_map(|entity| {
                if let SketchEntity::Member { member } = entity {
                    Some(member.clone())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        let instance_id = key
            .as_ref()
            .filter(|key| source.block_instances.contains_key(*key))
            .cloned()
            .or_else(|| {
                selected_members.iter().find_map(|member| {
                    blocks::instance_for_member(source, member).map(|(key, _)| key.clone())
                })
            })
            .or_else(|| {
                selected.iter().find_map(|entity| {
                    if let SketchEntity::Point { point } = entity {
                        source
                            .block_instances
                            .iter()
                            .find(|(_, instance)| &instance.anchor == point)
                            .map(|(key, _)| key.clone())
                    } else {
                        None
                    }
                })
            });
        let definition_id = key
            .filter(|key| source.blocks.contains_key(key))
            .or_else(|| {
                instance_id
                    .as_ref()
                    .map(|key| source.block_instances[key].definition.clone())
            })
            .or_else(|| {
                (mode == Mode::Insert)
                    .then(|| source.blocks.keys().next().cloned())
                    .flatten()
            });
        let definition_id = if mode == Mode::Create {
            None
        } else {
            definition_id
        };
        let definition = definition_id
            .as_ref()
            .and_then(|key| source.blocks.get(key))
            .cloned();
        let instance_id = if matches!(mode, Mode::Create | Mode::Insert) {
            None
        } else {
            instance_id.or_else(|| {
                definition_id.as_ref().and_then(|definition| {
                    source
                        .block_instances
                        .iter()
                        .find(|(_, instance)| &instance.definition == definition)
                        .map(|(key, _)| key.clone())
                })
            })
        };
        let instance = instance_id
            .as_ref()
            .and_then(|key| source.block_instances.get(key))
            .cloned();
        let origin = selected
            .iter()
            .find_map(|entity| {
                if let SketchEntity::Point { point } = entity {
                    source.points.get(point).copied()
                } else {
                    None
                }
            })
            .unwrap_or([0.; 2]);
        let anchor = instance
            .as_ref()
            .and_then(|instance| source.points.get(&instance.anchor))
            .copied()
            .unwrap_or([origin[0] + 25., origin[1]]);
        Self {
            mode,
            definition_id,
            instance_id,
            definition,
            instance,
            selected: selected_members,
            name: format!("Block{}", source.blocks.len() + 1),
            origin,
            anchor,
            selection: vec![],
            relation: 0,
            local_dof: None,
        }
    }
    pub fn refresh(&mut self, source: &Sketch) {
        if let Some(key) = &self.definition_id {
            self.definition = source.blocks.get(key).cloned();
        }
        if let Some(key) = &self.instance_id {
            self.instance = source.block_instances.get(key).cloned();
            if let Some(instance) = &self.instance
                && let Some(anchor) = source.points.get(&instance.anchor)
            {
                self.anchor = *anchor;
            }
        }
    }
    pub fn apply(&mut self, doc: &Document, owner: &str, source: &mut Sketch) -> Result<()> {
        match self.mode {
            Mode::Create if self.definition_id.is_none() => {
                let (definition, instance) =
                    blocks::create(doc, owner, source, &self.selected, &self.name, self.origin)?;
                self.definition_id = Some(definition.clone());
                self.instance_id = Some(instance.clone());
                self.definition = source.blocks.get(&definition).cloned();
                self.instance = source.block_instances.get(&instance).cloned();
                self.anchor = source.points[&self.instance.as_ref().unwrap().anchor];
            }
            Mode::Insert if self.instance_id.is_none() => {
                let definition = self.definition_id.as_ref().ok_or_else(|| {
                    CadError::Invalid(
                        "Create a local block definition before inserting an instance".into(),
                    )
                })?;
                let key = blocks::insert(doc, owner, source, definition, self.anchor)?;
                self.instance_id = Some(key.clone());
                self.instance = source.block_instances.get(&key).cloned();
                self.definition = source.blocks.get(definition).cloned();
            }
            Mode::Definition | Mode::Create => {
                let key = self.definition_id.as_ref().ok_or_else(|| {
                    CadError::Invalid("Select a block to edit its local source".into())
                })?;
                let mut definition = self
                    .definition
                    .clone()
                    .ok_or_else(|| CadError::Invalid("Block source is missing".into()))?;
                if self.mode == Mode::Create {
                    definition.name = self.name.clone();
                    definition.origin = self.origin;
                }
                blocks::set_definition(doc, owner, source, key, definition.clone())?;
                self.definition = Some(definition);
            }
            _ => {}
        }
        if self.mode != Mode::Definition {
            let key = self
                .instance_id
                .as_ref()
                .ok_or_else(|| CadError::Invalid("Select a block instance".into()))?;
            let instance = self
                .instance
                .clone()
                .ok_or_else(|| CadError::Invalid("Block instance is missing".into()))?;
            source.points.insert(instance.anchor.clone(), self.anchor);
            source.block_instances.insert(key.clone(), instance);
            blocks::derive(doc, owner, source, key)?;
        }
        if let (Some(key), Some(definition)) = (&self.definition_id, &self.definition) {
            use cad_core::solver::ConstraintSolver;
            let mut local = blocks::source(key, definition);
            self.local_dof = Some(
                cad_core::solver::NativeSolver
                    .solve_sketch(doc, owner, &mut local)?
                    .degrees_of_freedom,
            );
        }
        Ok(())
    }
}
fn expression(ui: &mut egui::Ui, label: &str, value: &mut Expr) -> bool {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.add(egui::TextEdit::singleline(&mut value.0).desired_width(115.))
            .changed()
    })
    .inner
}
fn point_fields(ui: &mut egui::Ui, label: &str, point: &mut [f64; 2]) -> bool {
    ui.horizontal(|ui| {
        ui.label(label);
        let x = ui
            .add(
                egui::DragValue::new(&mut point[0])
                    .speed(0.1)
                    .prefix("X ")
                    .suffix(" mm"),
            )
            .changed();
        let y = ui
            .add(
                egui::DragValue::new(&mut point[1])
                    .speed(0.1)
                    .prefix("Y ")
                    .suffix(" mm"),
            )
            .changed();
        x || y
    })
    .inner
}
pub fn controls(
    ui: &mut egui::Ui,
    editor: &mut Editor,
    source: &Sketch,
    doc: &Document,
    owner: &str,
) -> bool {
    let mut changed = false;
    if editor.mode == Mode::Create {
        ui.label(format!(
            "{} independent source members",
            editor.selected.len()
        ));
        changed |= ui.text_edit_singleline(&mut editor.name).changed();
        changed |= point_fields(ui, "Insertion origin", &mut editor.origin);
    }
    if editor.mode == Mode::Insert {
        let label = editor
            .definition_id
            .as_ref()
            .and_then(|key| source.blocks.get(key))
            .map(|block| block.name.as_str())
            .unwrap_or("Choose a block");
        ui.add_enabled_ui(editor.instance_id.is_none(), |ui| {
            egui::ComboBox::from_id_salt("block-insert-definition")
                .selected_text(label)
                .show_ui(ui, |ui| {
                    for (key, definition) in &source.blocks {
                        changed |= ui
                            .selectable_value(
                                &mut editor.definition_id,
                                Some(key.clone()),
                                &definition.name,
                            )
                            .changed();
                    }
                })
        });
        if editor.instance_id.is_some() {
            ui.small("Cancel this insertion to choose another definition.");
        }
    }
    if matches!(editor.mode, Mode::Definition) {
        if let Some(definition) = &mut editor.definition {
            changed |= ui.text_edit_singleline(&mut definition.name).changed();
            changed |= point_fields(ui, "Insertion origin", &mut definition.origin);
            if let Some(dof) = editor.local_dof {
                ui.small(format!("Local source: {dof} internal degrees of freedom. Instances preserve this solved shape rigidly."));
            }
            egui::ScrollArea::vertical()
                .id_salt("block-source-controls")
                .max_height(180.)
                .show(ui, |ui| {
                    for (index, (key, point)) in definition.points.iter_mut().enumerate() {
                        ui.push_id(key, |ui| {
                            let entity = SketchEntity::Point { point: key.clone() };
                            let mut selected = editor.selection.contains(&entity);
                            ui.horizontal(|ui| {
                                if ui
                                    .checkbox(&mut selected, format!("Control {}", index + 1))
                                    .changed()
                                {
                                    if selected {
                                        editor.selection.push(entity.clone());
                                    } else {
                                        editor.selection.retain(|item| item != &entity);
                                    }
                                }
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
                        });
                    }
                });
            let mut remove_member = None;
            egui::CollapsingHeader::new("Local members").show(ui, |ui| {
                for (index, (key, member)) in definition.members.iter_mut().enumerate() {
                    ui.push_id(key, |ui| {
                        let entity = SketchEntity::Member {
                            member: key.clone(),
                        };
                        let mut selected = editor.selection.contains(&entity);
                        ui.horizontal(|ui| {
                            if ui
                                .checkbox(&mut selected, format!("Member {}", index + 1))
                                .changed()
                            {
                                if selected {
                                    editor.selection.push(entity.clone());
                                } else {
                                    editor.selection.retain(|item| item != &entity);
                                }
                            }
                            changed |= ui
                                .checkbox(&mut member.construction, "Construction")
                                .changed();
                            if ui.small_button("Delete").clicked() {
                                remove_member = Some(key.clone());
                            }
                        });
                        match &mut member.curve {
                            SketchCurve::Circle { radius, .. } => {
                                changed |= expression(ui, "Radius", radius)
                            }
                            SketchCurve::Ellipse {
                                major,
                                minor,
                                angle,
                                ..
                            } => {
                                changed |= expression(ui, "Major", major);
                                changed |= expression(ui, "Minor", minor);
                                changed |= expression(ui, "Rotation", angle);
                            }
                            SketchCurve::Nurbs { weights, .. } => {
                                ui.horizontal_wrapped(|ui| {
                                    for (index, weight) in weights.iter_mut().enumerate() {
                                        changed |= ui
                                            .add(
                                                egui::DragValue::new(weight)
                                                    .speed(0.05)
                                                    .prefix(format!("Weight {} ", index + 1)),
                                            )
                                            .changed();
                                    }
                                });
                            }
                            _ => {}
                        }
                    });
                }
            });
            if let Some(key) = remove_member {
                definition.members.remove(&key);
                definition.points.retain(|point, _| {
                    definition
                        .members
                        .values()
                        .any(|member| member.curve.points().contains(&point.as_str()))
                        || definition
                            .constraints
                            .iter()
                            .any(|constraint| constraint.entities.contains(point))
                });
                editor.selection.retain(|entity| entity.key() != key);
                changed = true;
            }
            let selected_points = editor
                .selection
                .iter()
                .filter_map(|entity| {
                    if let SketchEntity::Point { point } = entity {
                        Some(point.clone())
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>();
            if ui
                .add_enabled(
                    selected_points.len() == 2,
                    egui::Button::new("Add line between selected controls"),
                )
                .clicked()
            {
                definition.members.insert(
                    id(),
                    SketchMember {
                        construction: false,
                        curve: SketchCurve::Line {
                            start: selected_points[0].clone(),
                            end: selected_points[1].clone(),
                        },
                    },
                );
                changed = true;
            }
            ui.horizontal_wrapped(|ui| {
                for (label, entity) in [
                    ("Local origin", SketchEntity::Origin),
                    ("Local X axis", SketchEntity::XAxis),
                    ("Local Y axis", SketchEntity::YAxis),
                ] {
                    let mut selected = editor.selection.contains(&entity);
                    if ui.checkbox(&mut selected, label).changed() {
                        if selected {
                            editor.selection.push(entity.clone());
                        } else {
                            editor.selection.retain(|item| item != &entity);
                        }
                    }
                }
            });
            let local = blocks::source(
                editor.definition_id.as_deref().unwrap_or("block"),
                definition,
            );
            let applicable = sketch_actions::RELATIONS
                .iter()
                .filter(|(kind, _)| {
                    sketch_actions::relation(&local, &editor.selection, kind).is_ok()
                })
                .collect::<Vec<_>>();
            editor.relation = editor.relation.min(applicable.len().saturating_sub(1));
            egui::ComboBox::from_id_salt("block-local-relations")
                .selected_text(
                    applicable
                        .get(editor.relation)
                        .map(|(_, label)| *label)
                        .unwrap_or("Select controls/members"),
                )
                .show_ui(ui, |ui| {
                    for (index, (_, label)) in applicable.iter().enumerate() {
                        ui.selectable_value(&mut editor.relation, index, *label);
                    }
                });
            if ui
                .add_enabled(
                    !applicable.is_empty(),
                    egui::Button::new("Add local relation"),
                )
                .clicked()
                && let Some((kind, _)) = applicable.get(editor.relation)
                && let Ok(constraint) = sketch_actions::relation(&local, &editor.selection, kind)
            {
                definition.constraints.extend(constraint);
                changed = true;
            }
            if ui
                .add_enabled(
                    !editor.selection.is_empty(),
                    egui::Button::new("Add local Smart Dimension"),
                )
                .clicked()
                && let Ok(dimensions) =
                    sketch_actions::dimensions(doc, owner, &local, &editor.selection)
                && let Some(dimension) = dimensions.first()
            {
                definition.constraints.push(SketchConstraint {
                    id: id(),
                    kind: dimension.kind.clone(),
                    entities: dimension.entities.clone(),
                    value: dimension.value.clone(),
                    driven: false,
                    contact: None,
                    distance_side: dimension.distance_side,
                });
                changed = true;
            }
            let mut remove_relation = None;
            egui::CollapsingHeader::new("Internal relations / dimensions").show(ui, |ui| {
                for (index, constraint) in definition.constraints.iter_mut().enumerate() {
                    ui.push_id(&constraint.id, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(sketch_actions::label(&constraint.kind));
                            if sketch_actions::is_dimension(&constraint.kind) {
                                changed |=
                                    ui.text_edit_singleline(&mut constraint.value.0).changed();
                                changed |=
                                    ui.checkbox(&mut constraint.driven, "Reference").changed();
                            }
                            if ui.small_button("×").clicked() {
                                remove_relation = Some(index);
                            }
                        })
                    });
                }
            });
            if let Some(index) = remove_relation {
                definition.constraints.remove(index);
                changed = true;
            }
        }
    } else if let Some(instance) = &mut editor.instance {
        changed |= point_fields(ui, "Insertion anchor", &mut editor.anchor);
        changed |= expression(ui, "Translation X", &mut instance.translation[0]);
        changed |= expression(ui, "Translation Y", &mut instance.translation[1]);
        changed |= expression(ui, "Rotation", &mut instance.angle);
        changed |= expression(ui, "Uniform scale", &mut instance.scale);
        if !blocks::rotation_literal(&instance.angle) && instance.free_rotation {
            instance.free_rotation = false;
            changed = true;
        }
        changed |= ui
            .add_enabled(
                blocks::rotation_literal(&instance.angle),
                egui::Checkbox::new(
                    &mut instance.free_rotation,
                    "Allow rotation in constraint solve",
                ),
            )
            .changed();
        changed |= ui
            .checkbox(&mut instance.suppressed, "Suppress instance")
            .changed();
        let mut override_construction = instance.construction.is_some();
        if ui
            .checkbox(&mut override_construction, "Override source construction")
            .changed()
        {
            instance.construction = override_construction.then_some(false);
            changed = true;
        }
        if let Some(construction) = &mut instance.construction {
            changed |= ui.checkbox(construction, "Construction geometry").changed();
        }
        ui.small("Anchor supplies translation freedom; free literal rotation supplies one angle DOF. Scale and expression fields retain their authored intent.");
    }
    changed
}
