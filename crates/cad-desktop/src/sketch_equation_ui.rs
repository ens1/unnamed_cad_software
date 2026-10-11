//! Polynomial curve fields consumed by the shared docked sketch operation editor.
use bevy_egui::egui;
use cad_core::{
    CadError, Document, Expr, Id, Result, Sketch,
    sketch_actions::SketchEntity,
    sketch_equation::{self as equation, Create},
};

pub struct Editor {
    pub id: Id,
    pub request: Create,
    pub anchor_position: [f64; 2],
    pub create_anchor: bool,
}

impl Editor {
    pub fn new(source: &Sketch, selection: &[SketchEntity], key: Option<Id>) -> Result<Self> {
        if let Some(key) = key {
            let definition = source.equations.get(&key).ok_or_else(|| {
                CadError::Invalid("Select a member with saved polynomial equation intent".into())
            })?;
            equation::validate(source, &key, definition)?;
            return Ok(Self {
                id: key.clone(),
                request: definition.request(source.members[&key].construction),
                anchor_position: source.points[&definition.anchor],
                create_anchor: false,
            });
        }
        let anchor = selection.iter().find_map(|entity| {
            if let SketchEntity::Point { point } = entity {
                source.points.contains_key(point).then(|| point.clone())
            } else {
                None
            }
        });
        let create_anchor = anchor.is_none();
        let anchor = anchor.unwrap_or_else(cad_core::id);
        let anchor_position = source.points.get(&anchor).copied().unwrap_or([0.; 2]);
        Ok(Self {
            id: cad_core::id(),
            request: Create {
                anchor,
                x: vec!["0 mm".into(), "30 mm".into(), "0 mm".into()],
                y: vec!["0 mm".into(), "0 mm".into(), "15 mm".into()],
                range: ["0".into(), "1".into()],
                angle: "0 deg".into(),
                construction: false,
            },
            anchor_position,
            create_anchor,
        })
    }
    pub fn refresh(&mut self, source: &Sketch) {
        if let Some(definition) = source.equations.get(&self.id) {
            self.request = definition.request(
                source
                    .members
                    .get(&self.id)
                    .is_some_and(|member| member.construction),
            );
            if let Some(position) = source.points.get(&definition.anchor) {
                self.anchor_position = *position;
            }
        }
    }
    pub fn apply(&self, doc: &Document, owner: &str, source: &mut Sketch) -> Result<()> {
        let mut candidate = source.clone();
        if !self.create_anchor && !candidate.points.contains_key(&self.request.anchor) {
            return Err(CadError::Invalid(
                "The equation source anchor is missing; select an explicit replacement".into(),
            ));
        }
        if self.anchor_position.iter().any(|value| !value.is_finite()) {
            return Err(CadError::Invalid(
                "Equation anchor coordinates must remain finite".into(),
            ));
        }
        candidate
            .points
            .insert(self.request.anchor.clone(), self.anchor_position);
        equation::set(doc, owner, &mut candidate, &self.id, &self.request)?;
        *source = candidate;
        Ok(())
    }
}

fn point_name(source: &Sketch, key: &str) -> String {
    source
        .points
        .keys()
        .position(|point| point == key)
        .map(|index| format!("Point {}", index + 1))
        .unwrap_or_else(|| "New Source Anchor".into())
}

pub fn fields(ui: &mut egui::Ui, editor: &mut Editor, source: &Sketch) -> bool {
    let mut changed = false;
    ui.small("X(t) = Σ Xᵢ tⁱ    Y(t) = Σ Yᵢ tⁱ. Coefficients are lengths; t is dimensionless.");
    let mut degree = editor.request.x.len().saturating_sub(1);
    if ui
        .add(
            egui::DragValue::new(&mut degree)
                .range(1..=equation::MAX_DEGREE)
                .prefix("Degree "),
        )
        .changed()
    {
        editor.request.x.resize(degree + 1, Expr("0 mm".into()));
        editor.request.y.resize(degree + 1, Expr("0 mm".into()));
        changed = true;
    }
    ui.small("Increasing degree appends zero coefficients and retains the curve. Lowering degree removes high powers; referenced retired controls must be repaired first.");
    egui::Grid::new(("equation-coefficients", &editor.id))
        .num_columns(3)
        .striped(true)
        .show(ui, |ui| {
            ui.label("Power");
            ui.label("X coefficient");
            ui.label("Y coefficient");
            ui.end_row();
            for index in 0..editor.request.x.len() {
                ui.label(format!("t^{index}"));
                changed |= ui
                    .text_edit_singleline(&mut editor.request.x[index].0)
                    .changed();
                changed |= ui
                    .text_edit_singleline(&mut editor.request.y[index].0)
                    .changed();
                ui.end_row();
            }
        });
    changed |= crate::cad_ui::expression(ui, "Parameter Start", &mut editor.request.range[0]);
    changed |= crate::cad_ui::expression(ui, "Parameter End", &mut editor.request.range[1]);
    changed |= crate::cad_ui::expression(ui, "Rotation", &mut editor.request.angle);
    changed |= ui
        .checkbox(&mut editor.request.construction, "Construction Geometry")
        .changed();
    let before = editor.request.anchor.clone();
    let driven = cad_core::sketch_edit::driven_points(source);
    let own = source
        .equations
        .get(&editor.id)
        .map(|definition| definition.controls.as_slice())
        .unwrap_or_default();
    egui::ComboBox::from_id_salt(("equation-anchor", &editor.id))
        .selected_text(point_name(source, &editor.request.anchor))
        .show_ui(ui, |ui| {
            for point in source.points.keys().filter(|point| !own.contains(point)) {
                changed |= ui
                    .selectable_value(
                        &mut editor.request.anchor,
                        point.clone(),
                        point_name(source, point),
                    )
                    .changed();
            }
        });
    if editor.request.anchor != before {
        editor.anchor_position = source.points[&editor.request.anchor];
        editor.create_anchor = false;
    }
    ui.add_enabled_ui(!driven.contains(&editor.request.anchor), |ui| {
        ui.horizontal(|ui| {
            ui.label("Anchor");
            changed |= ui
                .add(
                    egui::DragValue::new(&mut editor.anchor_position[0])
                        .speed(0.1)
                        .prefix("X ")
                        .suffix(" mm"),
                )
                .changed();
            changed |= ui
                .add(
                    egui::DragValue::new(&mut editor.anchor_position[1])
                        .speed(0.1)
                        .prefix("Y ")
                        .suffix(" mm"),
                )
                .changed();
        });
    });
    ui.small("Drag the source anchor on the sketch to translate the equation curve. Relations constrain that anchor; the derived controls follow the saved polynomial and are not independently movable.");
    ui.small("Algebraic polynomial curves only. Trigonometric, piecewise, implicit and other nonpolynomial equations are unsupported.");
    changed
}
