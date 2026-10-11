use crate::{Workspace, viewport};
use bevy_egui::egui;
use cad_core::inspection::{self, Metrics};

pub fn controls(ui: &mut egui::Ui, w: &mut Workspace) {
    let mut scope = w
        .fields
        .get("inspection-scope")
        .cloned()
        .unwrap_or_else(|| {
            if w.selected.is_some() {
                "component".into()
            } else {
                "assembly".into()
            }
        });
    egui::ComboBox::from_id_salt("inspection-scope")
        .selected_text(match scope.as_str() {
            "body" => "Selected Body",
            "component" => "Selected Component",
            _ => "Whole Assembly",
        })
        .show_ui(ui, |ui| {
            ui.selectable_value(&mut scope, "assembly".into(), "Whole Assembly");
            ui.selectable_value(&mut scope, "component".into(), "Selected Component");
            ui.selectable_value(&mut scope, "body".into(), "Selected Body");
        });
    w.fields.insert("inspection-scope".into(), scope.clone());
    let Some(evaluation) = viewport::display_evaluation(w) else {
        ui.label("Evaluate a model to inspect its properties.");
        return;
    };
    ui.small("Assembly coordinates · boundary tessellation");
    ui.small(format!("Rebuild tolerance: {} mm", w.tolerance()));
    if w.session
        .draft
        .as_ref()
        .is_some_and(|d| d.pending || d.error.is_some())
    {
        ui.small("Showing the last valid evaluation.");
    }
    let result = (|| {
        let selected = if scope == "assembly" {
            None
        } else {
            Some(w.selected.as_ref().ok_or_else(|| {
                cad_core::CadError::Invalid("Select geometry on a component first".into())
            })?)
        };
        let mut values = vec![];
        for instance in &evaluation.instances {
            if selected.is_some_and(|s| s.path != instance.path) {
                continue;
            }
            let key = cad_core::history::mesh_key(instance);
            let Some(properties) = evaluation.properties.get(key) else {
                continue;
            };
            let value = if scope == "body" {
                let selected = selected.unwrap();
                let body = selected.body.as_ref().ok_or_else(|| {
                    cad_core::CadError::Invalid(
                        "Select a model face or edge belonging to a body".into(),
                    )
                })?;
                properties.bodies.get(&body.node).ok_or_else(|| {
                    cad_core::CadError::Invalid(
                        "Selected body properties are unavailable; rebuild geometry".into(),
                    )
                })?
            } else {
                &properties.total
            };
            values.push(inspection::placed(value, instance.matrix)?);
        }
        inspection::combine(&values)
    })();
    match result {
        Ok(metrics) => display(ui, &metrics, &w.session.visible().settings),
        Err(error) => {
            ui.label(error.to_string());
        }
    }
}
fn display(ui: &mut egui::Ui, value: &Metrics, settings: &cad_core::settings::ModelSettings) {
    let unit = settings.display_length_unit;
    let scale = unit.millimeters_per_unit();
    let precision = usize::from(settings.length_decimal_places);
    egui::Grid::new("engineering-properties")
        .num_columns(2)
        .spacing([8., 6.])
        .show(ui, |ui| {
            for (label, text) in [
                (
                    "Surface Area",
                    format!(
                        "{:.precision$} {}²",
                        value.area_mm2 / scale.powi(2),
                        unit.symbol()
                    ),
                ),
                (
                    "Solid Volume",
                    format!(
                        "{:.precision$} {}³",
                        value.volume_mm3 / scale.powi(3),
                        unit.symbol()
                    ),
                ),
                (
                    "Mass",
                    value
                        .mass_kg
                        .map(|m| format!("{m:.9} kg"))
                        .unwrap_or_else(|| "No solid volume or assigned density".into()),
                ),
                (
                    "Bounding Size",
                    format!(
                        "{} × {} × {}",
                        settings.format_length(value.bounds_mm[1][0] - value.bounds_mm[0][0]),
                        settings.format_length(value.bounds_mm[1][1] - value.bounds_mm[0][1]),
                        settings.format_length(value.bounds_mm[1][2] - value.bounds_mm[0][2])
                    ),
                ),
            ] {
                ui.label(label);
                ui.label(text);
                ui.end_row();
            }
        });
    for (label, center) in [
        ("Center of Mass", value.mass_center_mm),
        ("Volume Centroid", value.volume_center_mm),
        ("Area Centroid", Some(value.area_center_mm)),
    ] {
        if let Some(c) = center {
            ui.separator();
            ui.strong(label);
            ui.label(format!(
                "X {} · Y {} · Z {}",
                settings.format_length(c[0]),
                settings.format_length(c[1]),
                settings.format_length(c[2])
            ));
        }
    }
    if let Some(inertia) = value.inertia_kg_m2 {
        ui.separator();
        ui.strong("Inertia about Center of Mass (kg·m²)");
        for row in inertia {
            ui.monospace(format!(
                "{:>14.7e} {:>14.7e} {:>14.7e}",
                row[0], row[1], row[2]
            ));
        }
    }
}
