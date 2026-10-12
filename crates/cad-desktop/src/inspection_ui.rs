//! Compact, independently semantic cached-property scopes, frames and reports.
use crate::Workspace;
use bevy_egui::egui;
use cad_core::{
    CadError, Result,
    inspection::report::{self, CacheState, Frame, Occurrence, Report, Request, Scope},
    presentation::SemanticElement,
    protocol::Evaluation,
};
use serde_json::{Value, json};
use std::{cell::RefCell, sync::Arc};

type CachedReport = (String, std::result::Result<Arc<Report>, String>);
thread_local! { static CACHE: RefCell<Option<CachedReport>> = const { RefCell::new(None) }; }
fn invalid(message: impl Into<String>) -> CadError {
    CadError::Invalid(format!("Mass Properties: {}", message.into()))
}
/// Engineering reports consume complete native session caches. Disposable
/// motion/repair viewport caches can omit dependent geometry and cannot prove
/// complete totals, even if their display identity follows the current draft.
fn source_evaluation(w: &Workspace) -> Option<&Evaluation> {
    let complete = |evaluation: &&Evaluation| evaluation.valid || evaluation.geometry_valid;
    if w.view.original && w.view.history_cursor.is_none() {
        return w.session.evaluation.as_ref().filter(complete);
    }
    let identity = w.session.identity();
    w.session
        .draft
        .as_ref()
        .and_then(|draft| {
            draft
                .evaluation
                .as_ref()
                .filter(|evaluation| {
                    evaluation.identity == identity
                        && (evaluation.valid || evaluation.geometry_valid)
                })
                .or_else(|| {
                    draft
                        .last_valid_evaluation
                        .as_ref()
                        .filter(|evaluation| evaluation.valid)
                })
        })
        .or_else(|| w.session.evaluation.as_ref().filter(complete))
}
fn scope(w: &Workspace) -> String {
    w.fields
        .get("inspection-scope")
        .cloned()
        .unwrap_or_else(|| {
            if w.selected.is_some() {
                "component".into()
            } else {
                "assembly".into()
            }
        })
}
fn frame(w: &Workspace) -> Result<Frame> {
    if let Some(error) = w.fields.get("inspection-frame-invalid") {
        return Err(invalid(error));
    }
    w.fields
        .get("inspection-frame")
        .map(|text| serde_json::from_str(text).map_err(CadError::from))
        .unwrap_or(Ok(Frame::World))
}
fn selected(w: &Workspace) -> Result<Occurrence> {
    let selection = w
        .selected
        .as_ref()
        .ok_or_else(|| invalid("Select a model component first"))?;
    let evaluation =
        source_evaluation(w).ok_or_else(|| invalid("No cached evaluation is available"))?;
    let mut matches = evaluation
        .instances
        .iter()
        .chain(
            evaluation
                .assembly_welds
                .values()
                .map(|weld| &weld.placement),
        )
        .filter(|instance| {
            instance.path == selection.path && instance.definition == selection.definition
        });
    let instance = matches
        .next()
        .ok_or_else(|| invalid("Selected component is absent from this cached evaluation"))?;
    if matches.next().is_some() {
        return Err(invalid("Selected component identity is ambiguous"));
    }
    Ok(instance.into())
}
fn request(w: &Workspace) -> Result<Request> {
    if let Some(error) = w.fields.get("inspection-scope-invalid") {
        return Err(invalid(error));
    }
    let scope = match scope(w).as_str() {
        "assembly" => Scope::Assembly,
        "component" => Scope::Component {
            occurrence: selected(w)?,
        },
        "body" => Scope::Body {
            occurrence: selected(w)?,
            body: w
                .selected
                .as_ref()
                .and_then(|selection| selection.body.clone())
                .ok_or_else(|| invalid("Select a face or edge belonging to a native body"))?,
        },
        _ => return Err(invalid("Unknown inspection scope")),
    };
    Ok(Request {
        scope,
        frame: frame(w)?,
        history: w.view.history_cursor.clone(),
    })
}
fn report(w: &Workspace) -> Result<Arc<Report>> {
    let evaluation = source_evaluation(w)
        .ok_or_else(|| invalid("Evaluate a model to inspect its properties"))?;
    let request = request(w)?;
    let state = if evaluation.identity == w.session.identity() {
        CacheState::Current
    } else {
        CacheState::LastValid
    };
    // The source class and completeness stamps also separate this cache from
    // any same-identity disposable viewport result previously displayed.
    let key = serde_json::to_string(&(
        "complete-native-session-properties-v2",
        &evaluation.identity,
        evaluation.valid,
        evaluation.geometry_valid,
        evaluation.elapsed_ms,
        evaluation.instances.len(),
        evaluation.assembly_welds.len(),
        evaluation.properties.len(),
        evaluation.references.len(),
        &request,
        state,
    ))?;
    CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if cache.as_ref().is_none_or(|(previous, _)| previous != &key) {
            *cache = Some((
                key,
                report::evaluate(evaluation, &request, state)
                    .map(Arc::new)
                    .map_err(|error| error.to_string()),
            ));
        }
        cache
            .as_ref()
            .expect("report cache installed")
            .1
            .clone()
            .map_err(CadError::Invalid)
    })
}
fn state(w: &Workspace) -> &'static str {
    if w.view.history_cursor.is_some() {
        "history"
    } else if source_evaluation(w)
        .is_some_and(|evaluation| evaluation.identity != w.session.identity())
    {
        "last-valid"
    } else {
        "current"
    }
}
fn description(value: &Report) -> String {
    format!(
        "{} · {}{}",
        if value.cache_state == CacheState::Current {
            "Current evaluation"
        } else {
            "Last valid evaluation"
        },
        value.frame_label,
        if value.request.history.is_some() {
            " · history station"
        } else {
            ""
        }
    )
}

fn styled_control(
    ui: &mut egui::Ui,
    id: &str,
    role: &str,
    classes: &[&str],
    content: impl FnOnce(&mut egui::Ui),
) {
    let properties = crate::presentation_controls::properties(id, role, classes, "current");
    if properties
        .get("display")
        .is_some_and(|value| value == "none")
    {
        return;
    }
    ui.scope(|ui| {
        crate::presentation_controls::apply_properties(ui, &properties);
        ui.spacing_mut().combo_width =
            crate::presentation_controls::number(&properties, "width", ui.spacing().combo_width)
                .max(24.);
        ui.spacing_mut().interact_size.y = crate::presentation_controls::number(
            &properties,
            "height",
            ui.spacing().interact_size.y,
        )
        .max(12.);
        if let Some(parent) = crate::presentation_controls::current_target() {
            let target = parent.child(
                id,
                role,
                &classes
                    .iter()
                    .map(|class| (*class).into())
                    .collect::<Vec<_>>(),
                "current",
            );
            crate::presentation_controls::with_target(target, ui, content);
        } else {
            content(ui);
        }
    });
}

pub fn controls(ui: &mut egui::Ui, w: &mut Workspace) {
    let mut scope = scope(w);
    let mut changed = false;
    ui.horizontal(|ui| {
        crate::presentation_controls::label(
            ui,
            "inspection:scope:label",
            "inspection-label",
            &[],
            "Scope",
            "current",
        );
        styled_control(
            ui,
            "inspection:scope",
            "inspection-field",
            &["choice"],
            |ui| {
                egui::ComboBox::from_id_salt("inspection-scope")
                    .selected_text(match scope.as_str() {
                        "assembly" => "Whole assembly",
                        "component" => "Selected component",
                        "body" => "Selected body",
                        _ => "Invalid scope",
                    })
                    .show_ui(ui, |ui| {
                        for (key, label) in [
                            ("assembly", "Whole assembly"),
                            ("component", "Selected component"),
                            ("body", "Selected body"),
                        ] {
                            changed |= ui.selectable_value(&mut scope, key.into(), label).clicked();
                        }
                    });
            },
        );
    });
    if changed {
        w.fields.insert("inspection-scope".into(), scope);
        w.fields.remove("inspection-scope-invalid");
    }
    let current_frame = frame(w);
    let choices = source_evaluation(w).map(report::frame_choices).transpose();
    let current_label = source_evaluation(w)
        .zip(current_frame.as_ref().ok())
        .and_then(|(evaluation, frame)| report::resolve_frame(evaluation, frame).ok())
        .map(|(_, label)| label)
        .unwrap_or_else(|| "Unavailable frame".into());
    ui.horizontal(|ui| {
        crate::presentation_controls::label(
            ui,
            "inspection:frame:label",
            "inspection-label",
            &[],
            "Coordinates",
            "current",
        );
        styled_control(
            ui,
            "inspection:frame",
            "inspection-field",
            &["choice"],
            |ui| {
                egui::ComboBox::from_id_salt("inspection-frame")
                    .selected_text(current_label)
                    .show_ui(ui, |ui| {
                        if let Ok(Some(choices)) = &choices {
                            for choice in choices {
                                if ui
                                    .selectable_label(
                                        current_frame
                                            .as_ref()
                                            .is_ok_and(|frame| frame == &choice.frame),
                                        &choice.label,
                                    )
                                    .clicked()
                                    && let Ok(text) = serde_json::to_string(&choice.frame)
                                {
                                    w.fields.insert("inspection-frame".into(), text);
                                    w.fields.remove("inspection-frame-invalid");
                                }
                            }
                        }
                    });
            },
        );
        if crate::presentation_controls::button(
            ui,
            "inspection:frame-selected",
            "inspection-action",
            &[],
            "Selected frame",
            w.selected.is_some(),
            false,
        )
        .clicked()
        {
            action(w, "inspection.frame-selected", None);
        }
    });
    if let Err(error) = choices {
        crate::presentation_controls::label(
            ui,
            "inspection:frame-error",
            "inspection-error",
            &["invalid"],
            &error.to_string(),
            "invalid",
        );
    }
    match report(w) {
        Ok(value) => {
            crate::presentation_controls::label(
                ui,
                "inspection:source",
                "inspection-source",
                &[state(w)],
                &description(&value),
                state(w),
            );
            crate::presentation_controls::label(
                ui,
                "inspection:basis",
                "inspection-disclosure",
                &[],
                "Boundary tessellation · conservative transformed body bounds",
                "approximate",
            );
            let elements = metrics(w, &value);
            metric_rows(ui, &elements, "principal");
            for (group, label) in [
                ("centroids", "Centroids"),
                ("bounds", "Bounds coordinates"),
                ("inertia", "Centroidal inertia"),
            ] {
                styled_control(
                    ui,
                    &format!("inspection:group:{group}"),
                    "inspection-group",
                    &[],
                    |ui| {
                        egui::CollapsingHeader::new(label)
                            .id_salt(format!("inspection:{group}"))
                            .show(ui, |ui| metric_rows(ui, &elements, group));
                    },
                );
            }
            if crate::presentation_controls::button(
                ui,
                "inspection:export",
                "inspection-action",
                &[],
                "Export JSON",
                true,
                false,
            )
            .clicked()
            {
                action(w, "inspection.export-json", None);
            }
            for (index, note) in value.notes.iter().enumerate() {
                crate::presentation_controls::label(
                    ui,
                    &format!("inspection:note:{index}"),
                    "inspection-disclosure",
                    &[],
                    note,
                    "approximate",
                );
            }
        }
        Err(error) => {
            crate::presentation_controls::label(
                ui,
                "inspection:error",
                "inspection-error",
                &["invalid"],
                &error.to_string(),
                "invalid",
            );
        }
    }
}

fn metric_rows(ui: &mut egui::Ui, elements: &[SemanticElement], group: &str) {
    for element in elements
        .iter()
        .filter(|element| element.data["group"] == group)
    {
        ui.horizontal(|ui| {
            crate::presentation_controls::label(
                ui,
                &format!("{}:label", element.id),
                "inspection-label",
                &[],
                &element.label,
                &element.state,
            );
            crate::presentation_controls::label(
                ui,
                &element.id,
                &element.role,
                &["readonly"],
                element.value.as_deref().unwrap_or("Unavailable"),
                &element.state,
            );
        });
    }
}

fn metrics(w: &Workspace, report: &Report) -> Vec<SemanticElement> {
    let settings = &w.session.visible().settings;
    let unit = settings.display_length_unit;
    let scale = unit.millimeters_per_unit();
    let precision = usize::from(settings.length_decimal_places);
    let mut out = Vec::new();
    let mut add = |key: &str, label: String, value: Value, display: String, unit: &str| {
        out.push(SemanticElement {
        id:format!("inspection:metric:{key}"),role:"inspection-metric".into(),label,value:Some(display),state:state(w).into(),commands:vec![],relationships:vec![],
            data:json!({"contract":"tcad.inspection.report-metric","version":report::CONTRACT_VERSION,"field":key,"group":if key.starts_with("inertia:"){"inertia"}else if key.contains("center:"){"centroids"}else if key.starts_with("bounds:")&&!key.starts_with("bounds:size:"){"bounds"}else{"principal"},"json":value,"unit":unit,"readonly":true,"source":report.source,"frame":report.request.frame,"history":report.request.history,"cache_state":report.cache_state,"bounds_basis":report.bounds_basis}),
    })
    };
    let v = &report.metrics;
    add(
        "area",
        "Surface area".into(),
        json!(v.area_mm2),
        format!(
            "{:.precision$} {}²",
            v.area_mm2 / scale.powi(2),
            unit.symbol()
        ),
        "mm2",
    );
    add(
        "volume",
        "Solid volume".into(),
        json!(v.volume_mm3),
        format!(
            "{:.precision$} {}³",
            v.volume_mm3 / scale.powi(3),
            unit.symbol()
        ),
        "mm3",
    );
    add(
        "mass",
        "Mass".into(),
        json!(v.mass_kg),
        v.mass_kg
            .map(|value| format!("{value:.9} kg"))
            .unwrap_or_else(|| "Unavailable (no complete solid density)".into()),
        "kg",
    );
    for axis in 0..3 {
        let axis_name = ["X", "Y", "Z"][axis];
        for (group, label, center) in [
            ("mass-center", "Center of mass", v.mass_center_mm),
            ("volume-center", "Volume centroid", v.volume_center_mm),
            ("area-center", "Area centroid", Some(v.area_center_mm)),
        ] {
            let value = center.map(|center| center[axis]);
            add(
                &format!("{group}:{axis}"),
                format!("{label} {axis_name}"),
                json!(value),
                value
                    .map(|value| settings.format_length(value))
                    .unwrap_or_else(|| "Unavailable".into()),
                "mm",
            );
        }
        for (key, label, value) in [
            ("minimum", "Bounds minimum", v.bounds_mm[0][axis]),
            ("maximum", "Bounds maximum", v.bounds_mm[1][axis]),
            (
                "size",
                "Bounding size",
                v.bounds_mm[1][axis] - v.bounds_mm[0][axis],
            ),
        ] {
            add(
                &format!("bounds:{key}:{axis}"),
                format!("{label} {axis_name}"),
                json!(value),
                settings.format_length(value),
                "mm",
            );
        }
    }
    for row in 0..3 {
        for column in row..3 {
            let value = v.inertia_kg_m2.map(|tensor| tensor[row][column]);
            add(
                &format!("inertia:{row}:{column}"),
                format!(
                    "I{}{} about mass center",
                    ["x", "y", "z"][row],
                    ["x", "y", "z"][column]
                ),
                json!(value),
                value
                    .map(|value| format!("{value:.7e} kg·m²"))
                    .unwrap_or_else(|| "Unavailable".into()),
                "kg_m2",
            );
        }
    }
    out
}

pub fn semantics(w: &Workspace) -> Vec<SemanticElement> {
    if w.dialog.as_deref() != Some("inspection") {
        return Vec::new();
    }
    let mut out = vec![SemanticElement {
        id: "inspection:scope".into(),
        role: "inspection-field".into(),
        label: "Inspection scope".into(),
        value: Some(scope(w)),
        state: if w.fields.contains_key("inspection-scope-invalid") {
            "invalid"
        } else {
            state(w)
        }
        .into(),
        commands: vec![],
        relationships: vec![],
        data: json!({"contract":"tcad.inspection.report-controls","version":1,"setter":"inspection-report","field":"scope","value_type":"choice","json":scope(w),"editable":true,"choices":[{"value":"assembly","label":"Whole assembly"},{"value":"component","label":"Selected component"},{"value":"body","label":"Selected body"}]}),
    }];
    let frame = frame(w);
    let choices_result = source_evaluation(w).map(report::frame_choices).transpose();
    let choices_error = choices_result.as_ref().err().map(ToString::to_string);
    let choices = choices_result.unwrap_or_default().unwrap_or_default();
    let resolved_frame = source_evaluation(w)
        .zip(frame.as_ref().ok())
        .and_then(|(evaluation, frame)| report::resolve_frame(evaluation, frame).ok());
    out.push(SemanticElement {id:"inspection:frame".into(),role:"inspection-field".into(),label:"Reporting coordinate frame".into(),value:Some(resolved_frame.as_ref().map(|(_,label)|label.clone()).unwrap_or_else(||"Unavailable frame".into())),state:if resolved_frame.is_some(){state(w)}else{"invalid"}.into(),commands:vec!["inspection.frame-selected".into()],relationships:vec![],data:json!({"contract":"tcad.inspection.report-controls","version":1,"setter":"inspection-report","field":"frame","value_type":"choice","json":frame.ok(),"editable":true,"choices":choices.iter().map(|choice|json!({"value":choice.frame,"label":choice.label})).collect::<Vec<_>>(),"choices_error":choices_error,"source_qualified":true})});
    out.push(SemanticElement {id:"inspection:frame-selected".into(),role:"inspection-action".into(),label:"Selected component frame".into(),value:None,state:if selected(w).is_ok(){"enabled"}else{"disabled"}.into(),commands:vec!["inspection.frame-selected".into()],relationships:vec![],data:json!({"contract":"tcad.inspection.report-controls","version":1,"enabled":selected(w).is_ok(),"source_qualified":true})});
    match report(w) {
        Ok(report)=>{
            out.push(SemanticElement {id:"inspection:source".into(),role:"inspection-source".into(),label:"Cached properties source".into(),value:Some(description(&report)),state:state(w).into(),commands:vec!["inspection.export-json".into()],relationships:vec![],data:json!({"contract":"tcad.inspection.report","version":report.version,"source":report.source,"request":report.request,"frame_to_world":report.frame_to_world,"bounds_basis":report.bounds_basis,"inertia_basis":report.inertia_basis,"item_count":report.items.len(),"notes":report.notes,"exact_brep_integration":false,"export_limit_bytes":report::MAX_EXPORT_BYTES})});
            out.extend(metrics(w,&report));
            out.push(SemanticElement {id:"inspection:export".into(),role:"inspection-action".into(),label:"Export JSON".into(),value:None,state:"enabled".into(),commands:vec!["inspection.export-json".into()],relationships:vec![],data:json!({"contract":"tcad.inspection.report","version":report.version,"enabled":true,"source":report.source,"cache_state":report.cache_state,"bounded":true})});
        }
        Err(error)=>out.push(SemanticElement {id:"inspection:error".into(),role:"inspection-error".into(),label:"Inspection unavailable".into(),value:Some(error.to_string()),state:"invalid".into(),commands:vec![],relationships:vec![],data:json!({"contract":"tcad.inspection.report","version":1,"scope":scope(w),"history":w.view.history_cursor,"evaluation_source":source_evaluation(w).map(|evaluation|&evaluation.identity),"partial_totals":false})}),
    }
    out
}
pub fn current_value(w: &Workspace, element: &SemanticElement) -> Option<Value> {
    if element.role != "inspection-field" {
        return None;
    }
    match element.data["field"].as_str()? {
        "scope" => Some(json!(scope(w))),
        "frame" => match w.fields.get("inspection-frame") {
            Some(text) => serde_json::from_str(text).ok(),
            None => Some(json!(Frame::World)),
        },
        _ => None,
    }
}
pub fn set_value(w: &mut Workspace, element: &SemanticElement, value: Value) -> Result<()> {
    if w.dialog.as_deref() != Some("inspection") || element.role != "inspection-field" {
        return Err(invalid(
            "Open Mass Properties before editing its source controls",
        ));
    }
    match element.data["field"].as_str() {
        Some("scope") => {
            let Some(scope) = value
                .as_str()
                .filter(|value| matches!(*value, "assembly" | "component" | "body"))
            else {
                w.fields.insert(
                    "inspection-scope-invalid".into(),
                    "Unknown inspection scope".into(),
                );
                return Err(invalid("Unknown inspection scope"));
            };
            w.fields.insert("inspection-scope".into(), scope.into());
            w.fields.remove("inspection-scope-invalid");
        }
        Some("frame") => {
            let text = serde_json::to_string(&value)?;
            if text.len() > 64 * 1024 {
                w.fields.insert(
                    "inspection-frame-invalid".into(),
                    "Frame source exceeds 64 KiB".into(),
                );
                return Err(invalid("Frame source exceeds 64 KiB"));
            }
            w.fields.insert("inspection-frame".into(), text);
            match serde_json::from_value::<Frame>(value) {
                Ok(frame) => {
                    w.fields.remove("inspection-frame-invalid");
                    if let Some(evaluation) = source_evaluation(w) {
                        report::resolve_frame(evaluation, &frame)?;
                    }
                }
                Err(error) => {
                    w.fields
                        .insert("inspection-frame-invalid".into(), error.to_string());
                    return Err(error.into());
                }
            }
        }
        _ => return Err(invalid("Unknown inspection control")),
    }
    Ok(())
}
pub fn action(w: &mut Workspace, command: &str, _args: Option<&Value>) -> bool {
    let result = match command {
        "inspection.frame-selected" => selected(w)
            .and_then(|occurrence| {
                serde_json::to_string(&Frame::Component { occurrence }).map_err(CadError::from)
            })
            .map(|text| {
                w.fields.insert("inspection-frame".into(), text);
                w.fields.remove("inspection-frame-invalid");
            }),
        "inspection.export-json" => report(w)
            .and_then(|report| report::export_json(&report))
            .and_then(|source| {
                if let Some(path) = rfd::FileDialog::new()
                    .set_title("Export cached mass properties")
                    .set_file_name("MassProperties.tcadproperties.json")
                    .add_filter("Source-qualified properties JSON", &["json"])
                    .save_file()
                {
                    cad_core::persistence::atomic_write(&path, source.as_bytes())?;
                    w.status = format!("Exported cached properties to {}", path.display());
                }
                Ok(())
            }),
        _ => return false,
    };
    if let Err(error) = result {
        w.status = error.to_string();
    }
    true
}
