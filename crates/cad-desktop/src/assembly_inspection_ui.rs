//! Docked saved assembly inspection over mechanical occurrence placements.
use crate::{Workspace, viewport};
use bevy_egui::egui;
use cad_core::{
    assembly_inspection::{self, Definition, InterferenceStatus, Mode, Selection, Target},
    commands::Command,
};
pub struct Highlight {
    pub first_path: Vec<String>,
    pub second_path: Vec<String>,
    pub witness_mm: Option<[[f64; 3]; 2]>,
}
fn evaluation(w: &Workspace) -> Option<&cad_core::protocol::Evaluation> {
    w.session
        .draft
        .as_ref()
        .and_then(|draft| draft.evaluation.as_ref())
        .or(w.session.evaluation.as_ref())
        .filter(|evaluation| evaluation.valid && evaluation.identity == w.session.identity())
}
fn editing(w: &Workspace) -> bool {
    w.sketch.is_some()
        || w.tool_checkpoint.is_some()
        || w.session.draft.is_some()
        || w.dialog.as_deref() == Some("feature")
        || [
            "mate-editor",
            "component-pattern-editor",
            "configuration-editor",
            "exploded-view-editor",
            "assembly-inspection-editor",
        ]
        .into_iter()
        .any(|key| w.fields.contains_key(key))
}
fn parsed(w: &Workspace) -> cad_core::Result<Definition> {
    let source: Definition = serde_json::from_str(&w.json)?;
    if w.fields.get("assembly-inspection-editor") != Some(&source.id) {
        return Err(cad_core::CadError::Invalid(
            "Inspection identity cannot change".into(),
        ));
    }
    Ok(source)
}
pub fn current_source_is_valid(w: &Workspace) -> bool {
    let doc = w.session.visible();
    let configuration =
        serde_json::to_string(&doc.active_configurations.get(&doc.root)).unwrap_or_default();
    w.fields.get("assembly-id") == Some(&doc.root)
        && w.fields.get("assembly-inspection-configuration") == Some(&configuration)
        && parsed(w).is_ok_and(|source| {
            doc.assemblies
                .get(&doc.root)
                .is_some_and(|a| a.inspections.get(&source.id) == Some(&source))
                && (source.configuration.is_none()
                    || source.configuration.as_ref() == doc.active_configurations.get(&doc.root))
        })
}
fn guard(w: &mut Workspace) {
    if current_source_is_valid(w) {
        w.fields.remove("assembly-inspection-source-invalid");
    } else {
        w.fields
            .insert("assembly-inspection-source-invalid".into(), "true".into());
        w.fields.remove("assembly-inspection-highlight");
    }
}
pub fn cancel(w: &mut Workspace) {
    for key in [
        "assembly-inspection-editor",
        "assembly-inspection-source-invalid",
        "assembly-inspection-highlight",
        "assembly-inspection-configuration",
    ] {
        w.fields.remove(key);
    }
}
fn submit(w: &mut Workspace, source: Definition) {
    w.json = serde_json::to_string_pretty(&source).unwrap_or_default();
    let owner = w.session.visible().root.clone();
    let mut doc = w.session.visible().clone();
    match (Command::PutAssemblyInspection {
        assembly: owner,
        inspection: Box::new(source),
    })
    .execute(&mut doc)
    {
        Ok(()) => {
            w.fields.remove("assembly-inspection-highlight");
            w.dispatch(Command::ReplaceDocument { document: doc });
        }
        Err(error) => w.status = error.to_string(),
    }
    guard(w);
}
fn begin(w: &mut Workspace, source: Definition) -> bool {
    if editing(w) {
        w.status = "Confirm or cancel the current operation before editing an inspection".into();
        return false;
    }
    let doc = w.session.visible();
    if !doc.assemblies.contains_key(&doc.root) {
        w.status = "Open an assembly to inspect component occurrences".into();
        return false;
    }
    if source
        .configuration
        .as_ref()
        .is_some_and(|c| doc.active_configurations.get(&doc.root) != Some(c))
    {
        w.status = "Select the inspection's linked assembly configuration first".into();
        return false;
    }
    let owner = doc.root.clone();
    let configuration =
        serde_json::to_string(&doc.active_configurations.get(&owner)).unwrap_or_default();
    w.start_tool();
    w.fields.insert("assembly-id".into(), owner);
    w.fields
        .insert("assembly-inspection-configuration".into(), configuration);
    w.fields
        .insert("assembly-inspection-editor".into(), source.id.clone());
    w.json = serde_json::to_string_pretty(&source).unwrap_or_default();
    w.open_editor("assembly");
    guard(w);
    true
}
pub fn new(w: &mut Workspace) {
    let doc = w.session.visible();
    let Some(assembly) = doc.assemblies.get(&doc.root) else {
        w.status = "Open an assembly before inspecting interference or clearance".into();
        return;
    };
    let Some(evaluation) = evaluation(w) else {
        w.status = "Wait for the current assembled geometry evaluation".into();
        return;
    };
    if evaluation.instances.len() < 2 {
        w.status = "Insert at least two components before comparing them".into();
        return;
    }
    let name = (1..)
        .map(|n| format!("Interference and Clearance{n}"))
        .find(|name| {
            !assembly
                .inspections
                .values()
                .any(|d| d.name.eq_ignore_ascii_case(name))
        })
        .unwrap();
    let mut source = Definition::new(name);
    source.configuration = doc.active_configurations.get(&doc.root).cloned();
    source.boolean_tolerance = cad_core::Expr(format!("{} mm", w.tolerance()));
    source.targets = evaluation
        .instances
        .iter()
        .take(assembly_inspection::MAX_TARGETS)
        .map(|i| Target {
            path: i.path.clone(),
            configuration: i.configuration.clone(),
        })
        .collect();
    let partial = evaluation.instances.len() > assembly_inspection::MAX_TARGETS;
    if begin(w, source.clone()) {
        submit(w, source);
        if partial {
            w.status="Inspection scope starts with the first 64 occurrences; edit the selected scope explicitly".into();
        }
    }
}
pub fn open(w: &mut Workspace, id: &str) {
    let source = w
        .session
        .visible()
        .assemblies
        .get(&w.session.visible().root)
        .and_then(|a| a.inspections.get(id))
        .cloned();
    if let Some(source) = source {
        begin(w, source);
    } else {
        w.status = "This inspection does not belong to the current root assembly".into();
    }
}
pub fn highlight(w: &Workspace, evaluation: &cad_core::protocol::Evaluation) -> Option<Highlight> {
    if !evaluation.valid
        || evaluation.identity != w.session.identity()
        || w.view.history_cursor.is_some()
        || w.view.exploded_view.is_some()
        || w.fields.contains_key("assembly-inspection-editor") && !current_source_is_valid(w)
    {
        return None;
    }
    let selection: Selection =
        serde_json::from_str(w.fields.get("assembly-inspection-highlight")?).ok()?;
    let report =
        assembly_inspection::current(w.session.visible(), evaluation, &selection.inspection)?;
    let pair = report.pairs.iter().find(|p| p.id == selection.pair)?;
    Some(Highlight {
        first_path: pair.first.path.clone(),
        second_path: pair.second.path.clone(),
        witness_mm: selection
            .show_witness
            .then(|| pair.boundary_distance.as_ref().map(|d| d.witness_mm))
            .flatten(),
    })
}
fn select_pair(w: &mut Workspace, id: &str, pair_id: &str, side: Option<usize>) {
    if w.fields.contains_key("assembly-inspection-editor") && !current_source_is_valid(w) {
        w.status = "Preview the valid inspection source before using result actions".into();
        return;
    }
    let choice = (|| {
        let evaluation = evaluation(w)?;
        let report = assembly_inspection::current(w.session.visible(), evaluation, id)?;
        let pair = report.pairs.iter().find(|p| p.id == pair_id)?;
        let selection = side.and_then(|side| {
            let target = if side == 0 { &pair.first } else { &pair.second };
            let instance = evaluation
                .instances
                .iter()
                .find(|i| i.path == target.path && i.configuration == target.configuration)?;
            let distance = pair.boundary_distance.as_ref();
            let mesh = evaluation.meshes.get(&instance.mesh_key)?;
            let reference = distance.map(|d| {
                if side == 0 {
                    &d.first_face
                } else {
                    &d.second_face
                }
            });
            let face = reference
                .and_then(|source| {
                    mesh.faces.iter().find(|face| {
                        face.body.as_ref() == Some(&source.body)
                            && face.reference.producer == source.reference.producer
                            && face.reference.role == source.reference.role
                            && face.reference.topology == source.reference.topology
                    })
                })
                .or_else(|| mesh.faces.first())?;
            let end = face.start.checked_add(face.count)?;
            if face.count == 0 || end > mesh.indices.len() {
                return None;
            }
            let vertex = *mesh.indices.get(face.start)? as usize;
            let point = viewport::placed(*mesh.positions.get(vertex)?, instance);
            if point.iter().any(|x| !x.is_finite()) {
                return None;
            }
            Some(viewport::Selection {
                instance: instance.path.last()?.clone(),
                path: instance.path.clone(),
                name: instance.name.clone(),
                definition: instance.definition.clone(),
                reference: face.reference.clone(),
                body: Some(face.body.clone()?),
                distance: 0.,
                point,
            })
        });
        Some((
            selection,
            Selection {
                inspection: id.into(),
                pair: pair_id.into(),
                show_witness: true,
            },
        ))
    })();
    let Some((selection, highlight)) = choice else {
        w.status =
            "A current validated inspection report is required for this result action".into();
        return;
    };
    crate::history::latest(w);
    crate::exploded_views_ui::collapse_for_modeling(w);
    crate::exploded_views_ui::clear_selection(w);
    w.view.original = false;
    w.view.isolation = false;
    w.view.ghost_others = false;
    if let Some(selection) = selection {
        w.view.active_part = Some(selection.definition.clone());
        w.view.active_instance = Some(selection.instance.clone());
        w.selected = Some(selection);
    } else if side.is_some() {
        w.status =
            "The result has no selectable evaluated face; both occurrence bounds are highlighted"
                .into();
    }
    w.fields.insert(
        "assembly-inspection-highlight".into(),
        serde_json::to_string(&highlight).unwrap_or_default(),
    );
    w.rendered_key.clear();
}
pub fn action(w: &mut Workspace, command: &str, data: Option<&serde_json::Value>) -> bool {
    let target = data.and_then(|d| d.get("target")).or(data);
    if target
        .and_then(|t| t.get("assembly"))
        .and_then(|id| id.as_str())
        .is_some_and(|owner| owner != w.session.visible().root)
    {
        w.status =
            "Open the owning assembly as the document root for this inspection action".into();
        return true;
    }
    let id = target
        .and_then(|t| t.get("inspection"))
        .and_then(|id| id.as_str())
        .map(str::to_owned)
        .or_else(|| w.fields.get("assembly-inspection-editor").cloned());
    match command {
        "assembly.inspection-new" => new(w),
        "assembly.inspection-edit" => {
            if let Some(id) = id {
                open(w, &id);
            } else {
                w.status = "Select a saved inspection first".into();
            }
        }
        "assembly.inspection-delete" => {
            if let Some(id) = id {
                if editing(w) {
                    w.status =
                        "Confirm or cancel the current operation before deleting an inspection"
                            .into();
                } else {
                    let owner = w.session.visible().root.clone();
                    w.start_tool();
                    w.dispatch(Command::DeleteAssemblyInspection {
                        assembly: owner,
                        inspection: id,
                    });
                    w.open_editor("assembly");
                }
            }
        }
        "assembly.inspection-highlight"
        | "assembly.inspection-select-first"
        | "assembly.inspection-select-second" => {
            if let (Some(id), Some(pair)) = (
                id,
                target
                    .and_then(|t| t.get("pair"))
                    .and_then(|id| id.as_str()),
            ) {
                select_pair(
                    w,
                    &id,
                    pair,
                    match command {
                        "assembly.inspection-select-first" => Some(0),
                        "assembly.inspection-select-second" => Some(1),
                        _ => None,
                    },
                );
            } else {
                w.status = "Select a component pair in the current report".into();
            }
        }
        "assembly.inspection-clear-highlight" => {
            w.fields.remove("assembly-inspection-highlight");
            w.rendered_key.clear();
        }
        _ => return false,
    }
    true
}
fn expression(ui: &mut egui::Ui, label: &str, value: &mut cad_core::Expr) -> bool {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.text_edit_singleline(&mut value.0).changed()
    })
    .inner
}
fn advanced_source(ui: &mut egui::Ui, w: &mut Workspace, open: bool) {
    egui::CollapsingHeader::new("Advanced Source")
        .default_open(open)
        .show(ui, |ui| {
            if ui
                .add(
                    egui::TextEdit::multiline(&mut w.json)
                        .code_editor()
                        .desired_rows(8),
                )
                .changed()
            {
                guard(w);
            }
            if ui.button("Preview edited source").clicked() {
                match parsed(w) {
                    Ok(source) => submit(w, source),
                    Err(error) => w.status = error.to_string(),
                }
            }
        });
}
pub fn show(ui: &mut egui::Ui, w: &mut Workspace, assembly: &str) {
    ui.strong("Interference and Clearance");
    let mut source = match parsed(w) {
        Ok(source) => source,
        Err(error) => {
            ui.label(error.to_string());
            advanced_source(ui, w, true);
            guard(w);
            return;
        }
    };
    let mut changed = false;
    changed |= ui.text_edit_singleline(&mut source.name).changed();
    let current_configuration = w
        .session
        .visible()
        .active_configurations
        .get(assembly)
        .cloned();
    let before_configuration = source.configuration.clone();
    egui::ComboBox::from_id_salt("inspection-configuration")
        .selected_text(if source.configuration.is_some() {
            "Current named assembly configuration"
        } else {
            "All owning assembly configurations"
        })
        .show_ui(ui, |ui| {
            ui.selectable_value(
                &mut source.configuration,
                None,
                "All owning assembly configurations",
            );
            if let Some(configuration) = &current_configuration {
                ui.selectable_value(
                    &mut source.configuration,
                    Some(configuration.clone()),
                    "Current named assembly configuration",
                );
            }
        });
    changed |= before_configuration != source.configuration;
    egui::ComboBox::from_id_salt("inspection-mode")
        .selected_text(match source.mode {
            Mode::Interference => "Native interference",
            Mode::BoundaryDistance => "Approximate boundary distance",
            Mode::Both => "Interference and approximate boundary distance",
        })
        .show_ui(ui, |ui| {
            for (value, label) in [
                (Mode::Both, "Both checks"),
                (Mode::Interference, "Native interference"),
                (Mode::BoundaryDistance, "Approximate boundary distance"),
            ] {
                changed |= ui
                    .selectable_value(&mut source.mode, value, label)
                    .changed();
            }
        });
    changed |= expression(ui, "Requested minimum clearance", &mut source.clearance);
    changed |= expression(
        ui,
        "Native Boolean tolerance",
        &mut source.boolean_tolerance,
    );
    changed |= ui
        .checkbox(&mut source.suppressed, "Suppress inspection")
        .changed();
    ui.small("Assembled, mate-solved placements. Boundary distance is approximate and has no certified error bound. Surface bodies participate only in boundary distance.");
    let current_evaluation = evaluation(w);
    let scope_is_current = current_evaluation.is_some();
    let instances = current_evaluation
        .map(|e| e.instances.as_slice())
        .unwrap_or_default();
    if !scope_is_current {
        ui.small("Current assembled occurrence data is pending or unavailable; saved targets are retained.");
    } else if instances.is_empty() {
        ui.small("The current assembly has no active leaf occurrences.");
    }
    ui.separator();
    ui.strong(format!(
        "Selected occurrences ({}/{})",
        source.targets.len(),
        assembly_inspection::MAX_TARGETS
    ));
    let mut remove = None;
    for (index, target) in source.targets.iter_mut().enumerate() {
        ui.push_id(index, |ui| {
            let instance = instances.iter().find(|i| i.path == target.path);
            ui.label(
                instance
                    .map(|i| i.name.as_str())
                    .unwrap_or(if scope_is_current {
                        "Missing or inactive occurrence"
                    } else {
                        "Waiting for current assembled occurrence data"
                    }),
            );
            ui.small(format!(
                "Configuration: {}",
                target.configuration.as_deref().unwrap_or("Base")
            ));
            if let Some(instance) = instance
                && instance.configuration != target.configuration
                && ui.small_button("Use current leaf configuration").clicked()
            {
                target.configuration = instance.configuration.clone();
                changed = true;
            }
            if ui.small_button("Remove").clicked() {
                remove = Some(index);
            }
        });
    }
    if let Some(index) = remove {
        source.targets.remove(index);
        changed = true;
    }
    let selected = w
        .selected
        .as_ref()
        .and_then(|s| instances.iter().find(|i| i.path == s.path));
    if ui
        .add_enabled(
            source.targets.len() < assembly_inspection::MAX_TARGETS && selected.is_some(),
            egui::Button::new("Add selected component"),
        )
        .clicked()
        && let Some(i) = selected
        && !source.targets.iter().any(|t| t.path == i.path)
    {
        source.targets.push(Target {
            path: i.path.clone(),
            configuration: i.configuration.clone(),
        });
        changed = true;
    }
    egui::ComboBox::from_id_salt("inspection-add-occurrence")
        .selected_text("Add an active occurrence")
        .show_ui(ui, |ui| {
            for instance in instances.iter().take(4096) {
                if source.targets.len() < assembly_inspection::MAX_TARGETS
                    && !source.targets.iter().any(|t| t.path == instance.path)
                    && ui.selectable_label(false, &instance.name).clicked()
                {
                    source.targets.push(Target {
                        path: instance.path.clone(),
                        configuration: instance.configuration.clone(),
                    });
                    changed = true;
                }
            }
        });
    if instances.len() > 4096 {
        ui.small("The dropdown shows the first 4,096 occurrences. Select any other component in the model, then choose Add selected component.");
    }
    if ui
        .add_enabled(
            scope_is_current && instances.len() <= assembly_inspection::MAX_TARGETS,
            egui::Button::new("Use all active occurrences"),
        )
        .clicked()
    {
        source.targets = instances
            .iter()
            .map(|i| Target {
                path: i.path.clone(),
                configuration: i.configuration.clone(),
            })
            .collect();
        changed = true;
    }
    if changed {
        submit(w, source.clone());
    }
    let mut result_action = None;
    ui.separator();
    if current_source_is_valid(w)
        && let Some(evaluation) = evaluation(w)
        && let Some(report) =
            assembly_inspection::current(w.session.visible(), evaluation, &source.id)
    {
        ui.strong(format!(
            "{} of {} active pairs reported",
            report.pairs.len(),
            report.expected_pairs
        ));
        for inactive in &report.inactive_targets {
            ui.colored_label(ui.visuals().warn_fg_color, &inactive.message);
        }
        for diagnostic in &report.diagnostics {
            ui.colored_label(ui.visuals().warn_fg_color, diagnostic);
        }
        for pair in &report.pairs {
            let name = |target: &Target| {
                report
                    .occurrences
                    .iter()
                    .find(|i| &i.target == target)
                    .map(|i| i.name.as_str())
                    .unwrap_or("Missing occurrence")
            };
            ui.push_id(&pair.id,|ui|{ui.collapsing(format!("{} / {}",name(&pair.first),name(&pair.second)),|ui|{
                ui.label(match pair.interference{InterferenceStatus::ValidatedCommonSolid=>"Native common solid detected: engineering clearance is zero.",InterferenceStatus::SeparatedNativeHulls=>"Positive native carrier hulls are separated.",InterferenceStatus::NoCommonSolidReturned=>"No common solid returned; contact/separation remains unresolved.",InterferenceStatus::Unavailable=>"Native interference check unavailable.",InterferenceStatus::NotRequested=>"Native interference was not requested."});
                if let Some(volume)=pair.approximate_common_volume_mm3{ui.label(format!("Common-result planar boundary volume: {volume:.6} mm³ (f64 estimate)"));}
                if let Some(distance)=&pair.boundary_distance{ui.label(format!("Minimum boundary distance: {:.6} mm (approximate)",distance.distance_mm));ui.small(if distance.below_requested{"Boundary estimate is below the requested clearance; this is not a certified native distance comparison."}else{"Boundary estimate meets the requested value; this is not a clearance certificate."});}
                for diagnostic in &pair.diagnostics{ui.colored_label(ui.visuals().warn_fg_color,diagnostic);}
                ui.horizontal(|ui|{for(label,command)in [("Highlight pair","assembly.inspection-highlight"),("Select A","assembly.inspection-select-first"),("Select B","assembly.inspection-select-second")]{if ui.small_button(label).clicked(){result_action=Some((command,serde_json::json!({"target":{"kind":"assembly_inspection","assembly":assembly,"inspection":source.id,"pair":pair.id}})));}}});
            });});
        }
    } else if source.suppressed {
        ui.label("This inspection is suppressed.");
    } else {
        ui.label("Waiting for the current validated inspection report.");
    }
    if let Some((command, data)) = result_action {
        action(w, command, Some(&data));
    }
    advanced_source(ui, w, false);
    guard(w);
}
