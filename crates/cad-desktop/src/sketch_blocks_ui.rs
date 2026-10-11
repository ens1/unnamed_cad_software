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
    Import,
}
pub struct Editor {
    pub id: Id,
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
    pub resource: Option<blocks::resources::Package>,
    pub resource_error: Option<String>,
    pub resource_message: Option<String>,
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
        let definition_id = if matches!(mode, Mode::Create | Mode::Import) {
            None
        } else {
            definition_id
        };
        let definition = definition_id
            .as_ref()
            .and_then(|key| source.blocks.get(key))
            .cloned();
        let instance_id = if matches!(mode, Mode::Create | Mode::Insert | Mode::Import) {
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
            id: id(),
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
            resource: None,
            resource_error: None,
            resource_message: None,
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
        if let Some(error) = &self.resource_error {
            return Err(CadError::Invalid(error.clone()));
        }
        match self.mode {
            Mode::Import if self.definition_id.is_none() => {
                let package = self.resource.as_ref().ok_or_else(|| {
                    CadError::Invalid("Choose a sketch block resource to insert".into())
                })?;
                let (definition, instance) =
                    blocks::resources::insert(doc, owner, source, package, self.anchor)?;
                self.definition_id = Some(definition.clone());
                self.instance_id = Some(instance.clone());
                self.definition = source.blocks.get(&definition).cloned();
                self.instance = source.block_instances.get(&instance).cloned();
            }
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
    if editor.mode == Mode::Import {
        ui.label(
            editor
                .resource
                .as_ref()
                .map(|resource| resource.definition.name.as_str())
                .unwrap_or("Choose a block from another document"),
        );
        if crate::presentation_controls::button(
            ui,
            "sketch.block-resource-choose",
            "command-action",
            &["toolbar-control"],
            "Choose block file…",
            editor.instance_id.is_none(),
            false,
        )
        .clicked()
            && let Some(path) = rfd::FileDialog::new()
                .add_filter("TCAD Sketch Block", &["tcadblock", "json"])
                .pick_file()
        {
            changed = true;
            match blocks::resources::read(&path) {
                Ok(package) => {
                    editor.resource = Some(package);
                    editor.resource_error = None;
                    editor.resource_message = Some("Self-contained source · external dimensions resolved at export · no live file link".into());
                }
                Err(error) => editor.resource_error = Some(error.to_string()),
            }
        }
        if editor.instance_id.is_none() {
            changed |= point_fields(ui, "Insertion anchor", &mut editor.anchor);
        } else {
            ui.small("Cancel this insertion to choose another resource.");
        }
    }
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
    if let Some(key) = &editor.definition_id {
        ui.horizontal_wrapped(|ui| {
            if crate::presentation_controls::button(ui, "sketch.block-export", "command-action", &["toolbar-control"], "Export block…", true, false).clicked()
                && let Some(path) = rfd::FileDialog::new().add_filter("TCAD Sketch Block", &["tcadblock"]).set_file_name("SketchBlock.tcadblock").save_file()
            {
                let mut candidate = source.clone();
                if editor.mode == Mode::Definition && let Some(definition) = &editor.definition {
                    candidate.blocks.insert(key.clone(), definition.clone());
                }
                if editor.mode == Mode::Create && let Some(definition) = candidate.blocks.get_mut(key) {
                    definition.name = editor.name.clone(); definition.origin = editor.origin;
                }
                let result = configuration::effective(doc).and_then(|effective| blocks::resources::snapshot(&effective, owner, &candidate, key))
                    .and_then(|package| blocks::resources::write(&package, &path));
                editor.resource_message = Some(match result {
                    Ok(()) => format!("Exported {}", path.display()),
                    Err(error) => error.to_string(),
                });
            }
            if editor.mode == Mode::Definition {
                let imported = editor.definition.as_ref().is_some_and(|definition| definition.resource.is_some());
                if crate::presentation_controls::button(ui, "sketch.block-resource-update", "command-action", &["toolbar-control"], "Update from library…", imported, false).on_hover_text("Choose an updated file with the same pinned block identity").clicked()
                    && let Some(path) = rfd::FileDialog::new().add_filter("TCAD Sketch Block", &["tcadblock", "json"]).pick_file()
                {
                    let mut candidate = source.clone();
                    let result = blocks::resources::read(&path).and_then(|package| blocks::resources::update(doc, owner, &mut candidate, key, &package));
                    match result {
                        Ok(()) => {
                            editor.definition = candidate.blocks.get(key).cloned();
                            editor.resource_error = None;
                            editor.resource_message = Some("Pinned source update pending · inspect the preview before accepting".into());
                            changed = true;
                        }
                        Err(error) => editor.resource_message = Some(error.to_string()),
                    }
                }
            }
        });
    }
    if let Some(message) = &editor.resource_message {
        ui.small(message);
    }
    if let Some(error) = &editor.resource_error {
        ui.label(error);
    }
    changed
}

pub fn export(w: &mut crate::Workspace, key: &str) {
    let Some(sketch) = &w.sketch else { return };
    if sketch.edit.is_some() {
        w.status = "Use Export block in the active block editor, or accept/cancel the sketch operation first".into();
        return;
    }
    let source = crate::sketch::sketch_definition(sketch);
    let definition = source
        .block_instances
        .get(key)
        .map(|instance| instance.definition.as_str())
        .unwrap_or(key);
    let result = configuration::effective(w.session.visible())
        .and_then(|doc| blocks::resources::snapshot(&doc, &sketch.definition, &source, definition));
    match result {
        Ok(package) => {
            if let Some(path) = rfd::FileDialog::new()
                .add_filter("TCAD Sketch Block", &["tcadblock"])
                .set_file_name("SketchBlock.tcadblock")
                .save_file()
            {
                w.status = match blocks::resources::write(&package, &path) {
                    Ok(()) => format!("Exported {}", path.display()),
                    Err(error) => error.to_string(),
                };
            }
        }
        Err(error) => w.status = error.to_string(),
    }
}

/// Typed transient placements share this reader/setter with native block fields.
pub fn semantics(w: &crate::Workspace) -> Vec<presentation::SemanticElement> {
    let Some(sketch) = &w.sketch else {
        return vec![];
    };
    let Some(editor) = sketch
        .edit
        .as_ref()
        .and_then(|editor| editor.block.as_ref())
    else {
        return vec![];
    };
    let mut fields = vec![];
    if editor.mode == Mode::Create {
        fields.push((
            "name",
            "Block name",
            serde_json::json!(editor.name),
            "string",
        ));
        fields.push((
            "origin-x",
            "Insertion origin X",
            serde_json::json!(editor.origin[0]),
            "number",
        ));
        fields.push((
            "origin-y",
            "Insertion origin Y",
            serde_json::json!(editor.origin[1]),
            "number",
        ));
    } else if editor.mode == Mode::Definition
        && let Some(definition) = &editor.definition
    {
        fields.push((
            "name",
            "Block name",
            serde_json::json!(definition.name),
            "string",
        ));
        fields.push((
            "origin-x",
            "Insertion origin X",
            serde_json::json!(definition.origin[0]),
            "number",
        ));
        fields.push((
            "origin-y",
            "Insertion origin Y",
            serde_json::json!(definition.origin[1]),
            "number",
        ));
    }
    if editor.mode != Mode::Definition {
        fields.push((
            "anchor-x",
            "Insertion anchor X",
            serde_json::json!(editor.anchor[0]),
            "number",
        ));
        fields.push((
            "anchor-y",
            "Insertion anchor Y",
            serde_json::json!(editor.anchor[1]),
            "number",
        ));
        if let Some(instance) = &editor.instance {
            fields.push((
                "translation-x",
                "Translation X",
                serde_json::json!(instance.translation[0].0),
                "expression",
            ));
            fields.push((
                "translation-y",
                "Translation Y",
                serde_json::json!(instance.translation[1].0),
                "expression",
            ));
            fields.push((
                "angle",
                "Rotation",
                serde_json::json!(instance.angle.0),
                "expression",
            ));
            fields.push((
                "scale",
                "Uniform scale",
                serde_json::json!(instance.scale.0),
                "expression",
            ));
            fields.push((
                "suppressed",
                "Suppress instance",
                serde_json::json!(instance.suppressed),
                "boolean",
            ));
        }
    }
    let mut elements = fields.into_iter().map(|(field,label,value,kind)| presentation::SemanticElement {
            id: format!("sketch-block:{}:{}:{field}", sketch.sketch_id, editor.id),
        role: "sketch-block-field".into(), label: label.into(), value: Some(value.as_str().map(str::to_string).unwrap_or_else(|| value.to_string())),
        state: "editing".into(), commands: vec!["field.set".into()], relationships: vec![sketch.sketch_id.clone()],
        data: serde_json::json!({"contract":"tcad.sketch.block","version":blocks::CONTRACT_VERSION,"sketch":sketch.sketch_id,"field":field,"setter":"sketch-block","kind":kind,"value":value,"editable":true,"unit":if matches!(field,"origin-x"|"origin-y"|"anchor-x"|"anchor-y"|"translation-x"|"translation-y"){Some("mm")}else if field=="angle"{Some("angle")}else{None},"accessibility_label":label}),
    }).collect::<Vec<_>>();
    for (command, label, enabled, reason) in [
        (
            "sketch.block-resource-choose",
            "Choose block file…",
            editor.mode == Mode::Import && editor.instance_id.is_none(),
            "Start an Insert Block From File operation before choosing a resource",
        ),
        (
            "sketch.block-export",
            "Export block…",
            editor.definition_id.is_some(),
            "Create or choose a block definition first",
        ),
        (
            "sketch.block-resource-update",
            "Update from library…",
            editor.mode == Mode::Definition
                && editor
                    .definition
                    .as_ref()
                    .is_some_and(|definition| definition.resource.is_some()),
            "Edit an imported block definition before updating its source",
        ),
    ] {
        elements.push(presentation::SemanticElement {
            id: format!("command:{command}"), role: "command".into(), label: label.into(), value: None,
            state: if enabled {"ready"}else{"disabled"}.into(), commands: vec![command.into()], relationships: vec![sketch.sketch_id.clone()],
            data: serde_json::json!({"contract":"tcad.sketch.block-resource","version":blocks::resources::CONTRACT_VERSION,"available":enabled,"disabled_reason":(!enabled).then_some(reason),"actions":[{"command":command,"label":label,"enabled":enabled,"disabled_reason":(!enabled).then_some(reason)}]}),
        });
    }
    elements
}

/// Resource actions are editing operations, independent of their native buttons.
pub fn resource_action(w: &mut crate::Workspace, command: &str) -> bool {
    if !matches!(
        command,
        "sketch.block-resource-choose" | "sketch.block-resource-update" | "sketch.block-export"
    ) {
        return false;
    }
    if command == "sketch.block-export"
        && w.sketch.as_ref().is_none_or(|sketch| {
            sketch
                .edit
                .as_ref()
                .is_none_or(|editor| editor.block.is_none())
        })
    {
        return false;
    }
    let Some(sketch) = &w.sketch else {
        w.status = "Open a sketch block editor first".into();
        return true;
    };
    let mut source = crate::sketch::sketch_definition(sketch);
    let owner = sketch.definition.clone();
    let Some(block) = sketch
        .edit
        .as_ref()
        .and_then(|editor| editor.block.as_ref())
    else {
        w.status = "Open a sketch block editor first".into();
        return true;
    };
    let key = block.definition_id.clone();
    let allowed = match command {
        "sketch.block-resource-choose" => block.mode == Mode::Import && block.instance_id.is_none(),
        "sketch.block-resource-update" => {
            block.mode == Mode::Definition
                && block
                    .definition
                    .as_ref()
                    .is_some_and(|definition| definition.resource.is_some())
        }
        _ => key.is_some(),
    };
    if !allowed {
        w.status = "This resource action is unavailable in the current block operation".into();
        return true;
    }
    let Some(path) = (if command == "sketch.block-export" {
        rfd::FileDialog::new()
            .add_filter("TCAD Sketch Block", &["tcadblock"])
            .set_file_name("SketchBlock.tcadblock")
            .save_file()
    } else {
        rfd::FileDialog::new()
            .add_filter("TCAD Sketch Block", &["tcadblock", "json"])
            .pick_file()
    }) else {
        return true;
    };
    if command == "sketch.block-export" {
        if block.mode == Mode::Definition
            && let (Some(key), Some(definition)) = (&key, &block.definition)
        {
            source.blocks.insert(key.clone(), definition.clone());
        }
        if block.mode == Mode::Create
            && let Some(definition) = source.blocks.get_mut(key.as_deref().unwrap())
        {
            definition.name = block.name.clone();
            definition.origin = block.origin;
        }
        let result = configuration::effective(w.session.visible())
            .and_then(|doc| {
                blocks::resources::snapshot(&doc, &owner, &source, key.as_deref().unwrap())
            })
            .and_then(|package| blocks::resources::write(&package, &path));
        w.status = match result {
            Ok(()) => format!("Exported {}", path.display()),
            Err(error) => error.to_string(),
        };
        return true;
    }
    let result = blocks::resources::read(&path);
    if command == "sketch.block-resource-choose" {
        if let Some(block) = w
            .sketch
            .as_mut()
            .and_then(|sketch| sketch.edit.as_mut())
            .and_then(|editor| editor.block.as_mut())
        {
            match result {
                Ok(package) => {
                    block.resource = Some(package);
                    block.resource_error = None;
                }
                Err(error) => {
                    block.resource_error = Some(error.to_string());
                }
            }
        }
        crate::sketch_edit_ui::rebuild(w, false);
    } else {
        let result = result.and_then(|package| {
            blocks::resources::update(
                w.session.visible(),
                &owner,
                &mut source,
                key.as_deref().unwrap(),
                &package,
            )
        });
        match result {
            Ok(()) => {
                if let Some(block) = w
                    .sketch
                    .as_mut()
                    .and_then(|sketch| sketch.edit.as_mut())
                    .and_then(|editor| editor.block.as_mut())
                {
                    block.definition = source.blocks.get(key.as_deref().unwrap()).cloned();
                    block.resource_error = None;
                }
                crate::sketch_edit_ui::rebuild(w, false);
            }
            Err(error) => w.status = error.to_string(),
        }
    }
    true
}
pub fn current_value(
    w: &crate::Workspace,
    element: &presentation::SemanticElement,
) -> Option<serde_json::Value> {
    semantics(w)
        .into_iter()
        .find(|current| current.id == element.id)
        .and_then(|current| current.data.get("value").cloned())
}
pub fn set_value(
    w: &mut crate::Workspace,
    element: &presentation::SemanticElement,
    value: serde_json::Value,
) -> Result<()> {
    let current = semantics(w)
        .into_iter()
        .find(|current| current.id == element.id)
        .ok_or_else(|| {
            CadError::Invalid("The block field belongs to a different sketch operation".into())
        })?;
    let field = current.data["field"].as_str().unwrap_or_default();
    let text = || {
        value
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| CadError::Invalid("Expected a block expression or name".into()))
    };
    let number = || {
        value
            .as_f64()
            .filter(|value| value.is_finite())
            .ok_or_else(|| CadError::Invalid("Expected a finite block coordinate".into()))
    };
    let editor = w
        .sketch
        .as_mut()
        .and_then(|sketch| sketch.edit.as_mut())
        .and_then(|editor| editor.block.as_mut())
        .ok_or_else(|| CadError::Invalid("Open a sketch block editor first".into()))?;
    match field {
        "name" if editor.mode == Mode::Create => editor.name = text()?,
        "name" => editor.definition.as_mut().unwrap().name = text()?,
        "origin-x" | "origin-y" => {
            let index = usize::from(field.ends_with('y'));
            if editor.mode == Mode::Create {
                editor.origin[index] = number()?;
            } else {
                editor.definition.as_mut().unwrap().origin[index] = number()?;
            }
        }
        "anchor-x" | "anchor-y" => editor.anchor[usize::from(field.ends_with('y'))] = number()?,
        "translation-x" | "translation-y" => {
            editor.instance.as_mut().unwrap().translation[usize::from(field.ends_with('y'))] =
                Expr(text()?)
        }
        "angle" => {
            let instance = editor.instance.as_mut().unwrap();
            instance.angle = Expr(text()?);
            if !blocks::rotation_literal(&instance.angle) {
                instance.free_rotation = false;
            }
        }
        "scale" => editor.instance.as_mut().unwrap().scale = Expr(text()?),
        "suppressed" => {
            editor.instance.as_mut().unwrap().suppressed = value
                .as_bool()
                .ok_or_else(|| CadError::Invalid("Expected a boolean suppression state".into()))?
        }
        _ => return Err(CadError::Invalid("Unknown block field".into())),
    }
    crate::sketch_edit_ui::rebuild(w, false);
    Ok(())
}
