//! Default settings presentation over scoped, application-owned preferences.
//! The field descriptors and setters also serve user-defined compositions.
use crate::{Workspace, presentation_controls as controls, settings, settings_preferences};
use bevy_egui::egui;
use cad_core::{CadError, Result, presentation::SemanticElement, settings::ModelSettings};
use serde_json::{Value, json};
use std::collections::BTreeMap;

const CATEGORIES: &[(&str, &str, &str)] = &[
    ("model.units", "Units and precision", "Model"),
    ("model.geometry", "Geometry and parts", "Model"),
    ("model.lighting", "Lighting", "Model"),
    ("application.interface", "Interface", "Application"),
    ("application.viewport", "Model display", "Application"),
    ("application.themes", "Themes", "Application"),
    (
        "application.navigation",
        "Trackpad and navigation",
        "Application",
    ),
    ("application.recovery", "Recovery", "Application"),
    ("application.defaults", "New model defaults", "Application"),
    ("application.git", "Git connections", "Application"),
    ("application.libraries", "Content libraries", "Application"),
];

fn source(w: &Workspace, scope: &str) -> Result<Value> {
    match scope {
        "model" => {
            let settings = &w.session.visible().settings;
            let mut source = serde_json::to_value(settings)?;
            source["default_new_part_material"] = json!(settings.default_new_part_material);
            Ok(source)
        }
        "lighting" => Ok(serde_json::to_value(&w.session.visible().lighting)?),
        "application" => {
            let preferences = w
                .settings
                .as_ref()
                .map(|state| &state.application)
                .unwrap_or(&w.application_preferences);
            let mut source = serde_json::to_value(preferences)?;
            source["default_model"]["default_new_part_material"] =
                json!(preferences.default_model.default_new_part_material);
            source["default_model"]["default_material_selection"] = json!(format!(
                "{}:{}",
                if preferences.default_material.is_some() {
                    "snapshot"
                } else {
                    "builtin"
                },
                preferences.default_model.default_new_part_material
            ));
            source["default_model"]["default_finish_selection"] = json!(format!(
                "{}:{}",
                if preferences.default_finish.is_some() {
                    "snapshot"
                } else {
                    "builtin"
                },
                preferences.default_model.default_new_part_finish
            ));
            Ok(source)
        }
        "presentation" => Ok(json!({"theme":crate::theme_library::selected(w)})),
        "dialog" => Ok(json!({
            "search":w.settings.as_ref().map(|state|state.search.as_str()).unwrap_or("")
        })),
        _ => Err(CadError::Invalid("Unknown settings scope".into())),
    }
}

fn model_editable(w: &Workspace) -> bool {
    w.settings
        .as_ref()
        .map(|state| state.model_editable)
        .unwrap_or_else(|| {
            w.tool_checkpoint.is_none() && w.sketch.is_none() && w.session.draft.is_none()
        })
}

struct FieldSpec<'a> {
    category: &'a str,
    scope: &'a str,
    pointer: &'a str,
    label: &'a str,
    range: Option<[f64; 2]>,
    choices: Value,
    unit: Option<&'a str>,
}

fn field(w: &Workspace, spec: FieldSpec<'_>) -> SemanticElement {
    let value = source(w, spec.scope)
        .ok()
        .and_then(|source| source.pointer(spec.pointer).cloned())
        .unwrap_or_default();
    let pending = w.settings.as_ref().is_some_and(|state| state.pending);
    let editable =
        !pending && (matches!(spec.scope, "application" | "presentation") || model_editable(w));
    SemanticElement {
        id: format!(
            "settings:field:{}:{}",
            spec.scope,
            spec.pointer.trim_start_matches('/').replace('/', ":")
        ),
        role: "settings-field".into(),
        label: spec.label.into(),
        value: Some(
            value
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| value.to_string()),
        ),
        state: if pending {
            "pending"
        } else if editable {
            "editable"
        } else {
            "readonly"
        }
        .into(),
        commands: vec![format!("settings.open.{}", spec.category)],
        relationships: vec![format!("settings:category:{}", spec.category)],
        data: json!({
            "setter":"settings", "scope":spec.scope, "pointer":spec.pointer,
            "category":spec.category, "json":value, "editable":editable,
            "range":spec.range, "choices":spec.choices, "unit":spec.unit,
            "minimum":spec.range.map(|range|range[0]), "maximum":spec.range.map(|range|range[1]),
            "step":if spec.pointer.ends_with("/modeling/linear_mm") { Some(1e-7) } else if spec.pointer.ends_with("/modeling/boolean_search_mm") || spec.pointer.ends_with("/display_tolerance_mm") { Some(0.001) } else { None },
            "kind":if spec.choices.as_array().is_some_and(|choices|!choices.is_empty()) {"choice"} else if value.is_boolean() {"boolean"} else if value.is_number() {"number"} else {"text"},
            "accessibility_label":spec.label,
            "description":field_description(spec.scope, spec.pointer),
            "model_mutation":!matches!(spec.scope,"application"|"presentation"),
            "contract":match spec.scope {"application"=>"tcad.application-preferences", "lighting"=>"tcad.appearance", "presentation"=>"tcad.presentation.theme-library", _=>"tcad.model-settings"},
            "version":if spec.scope == "model" { cad_core::settings::CONTRACT_VERSION } else { 1 }
        }),
    }
}

fn field_description(scope: &str, pointer: &str) -> Option<&'static str> {
    match pointer.strip_prefix("/default_model").unwrap_or(pointer) {
        "/display_tolerance_mm" => {
            return Some(
                "Viewport and meshed export chord tolerance. Changing display quality never changes native modeling accuracy or Boolean search resolution.",
            );
        }
        "/modeling/linear_mm" => {
            return Some(
                "Accepted native boundary/carrier residual in millimeters. This is independent of display meshes; existing native constructors may enforce stricter checks and unsupported geometry remains rejected.",
            );
        }
        "/modeling/boolean_search_mm" => {
            return Some(
                "Native Boolean and intersection discovery resolution. This controls search subdivision, not final B-rep error; the boundary residual is validated independently.",
            );
        }
        _ => {}
    }
    if scope != "application" {
        return None;
    }
    match pointer {
        "/show_part_edges" => Some(
            "Draw visible topology edges over shaded parts. This changes only the viewport display.",
        ),
        "/hover_centerlines" => {
            Some("Reveal center marks and axes when the pointer is near a circular edge or hole.")
        }
        "/hover_midpoints" => {
            Some("Reveal midpoint marks and guides when the pointer is near a straight model edge.")
        }
        _ => None,
    }
}

fn length_choices() -> Value {
    Value::Array(
        cad_core::settings::LengthUnit::ALL
            .into_iter()
            .map(|unit| json!({"value":unit,"label":format!("{} ({})",unit.label(),unit.symbol())}))
            .collect(),
    )
}

fn model_fields(w: &Workspace, application_defaults: bool) -> Vec<SemanticElement> {
    let scope = if application_defaults {
        "application"
    } else {
        "model"
    };
    let prefix = if application_defaults {
        "/default_model"
    } else {
        ""
    };
    let units = if application_defaults {
        "application.defaults"
    } else {
        "model.units"
    };
    let geometry = if application_defaults {
        "application.defaults"
    } else {
        "model.geometry"
    };
    let finish_choices = if application_defaults {
        let mut choices = cad_core::appearance::new_document_finishes().into_values().map(|finish| json!({"value":format!("builtin:{}",finish.id),"label":format!("Built-in · {}",finish.name)})).collect::<Vec<_>>();
        choices.extend(cad_core::finish_library::catalog().iter().map(|entry| json!({"value":format!("catalog:{}",entry.finish.id),"label":format!("Catalog · {}",entry.finish.name)})));
        choices.extend(w.session.visible().finishes.values().map(|finish| json!({"value":format!("project:{}",finish.id),"label":format!("This project · {}",finish.name)})));
        let preferences = w
            .settings
            .as_ref()
            .map(|state| &state.application)
            .unwrap_or(&w.application_preferences);
        if let Some(snapshot) = &preferences.default_finish {
            choices.push(json!({"value":format!("snapshot:{}",snapshot.finish.id),"label":format!("Saved default · {}",snapshot.finish.name)}));
        }
        Value::Array(choices)
    } else {
        Value::Array(
            w.session
                .visible()
                .finishes
                .values()
                .map(|finish| json!({"value":finish.id,"label":finish.name}))
                .collect(),
        )
    };
    let material_choices = if application_defaults {
        let mut choices = cad_core::Document::blank_part("Part1").materials.into_values().map(|material| json!({"value":format!("builtin:{}",material.id),"label":format!("Built-in · {}",material.name)})).collect::<Vec<_>>();
        if let Ok(catalog) = cad_core::material_library::installed_catalog() {
            choices.extend(catalog.materials.iter().map(|material| json!({"value":format!("catalog:{}",material.id),"label":format!("Catalog · {}",material.name)})));
        }
        choices.extend(w.session.visible().materials.values().map(|material| json!({"value":format!("project:{}",material.id),"label":format!("This project · {}",material.name)})));
        let preferences = w
            .settings
            .as_ref()
            .map(|state| &state.application)
            .unwrap_or(&w.application_preferences);
        if let Some(material) = &preferences.default_material {
            choices.push(json!({"value":format!("snapshot:{}",material.id),"label":format!("Saved default · {}",material.name)}));
        }
        Value::Array(choices)
    } else {
        Value::Array(
            w.session
                .visible()
                .materials
                .values()
                .map(|material| json!({"value":material.id,"label":material.name}))
                .collect(),
        )
    };
    [
        (
            units,
            "/display_length_unit",
            "Length units",
            None,
            length_choices(),
            None,
        ),
        (
            units,
            "/length_decimal_places",
            "Length decimal places",
            Some([0., 8.]),
            json!([]),
            None,
        ),
        (
            units,
            "/angle_decimal_places",
            "Angle decimal places",
            Some([0., 8.]),
            json!([]),
            None,
        ),
        (
            geometry,
            "/display_tolerance_mm",
            "Display chord tolerance",
            Some([
                cad_core::tolerance::MIN_DISPLAY_MM,
                cad_core::tolerance::MAX_DISPLAY_MM,
            ]),
            json!([]),
            Some("mm"),
        ),
        (
            geometry,
            "/modeling/linear_mm",
            "Boundary residual tolerance",
            Some([
                cad_core::tolerance::MIN_LINEAR_MM,
                cad_core::tolerance::MAX_LINEAR_MM,
            ]),
            json!([]),
            Some("mm"),
        ),
        (
            geometry,
            "/modeling/boolean_search_mm",
            "Boolean search resolution",
            Some([
                cad_core::tolerance::MIN_SEARCH_MM,
                cad_core::tolerance::MAX_SEARCH_MM,
            ]),
            json!([]),
            Some("mm"),
        ),
        (
            geometry,
            if application_defaults {
                "/default_material_selection"
            } else {
                "/default_new_part_material"
            },
            "Default material for new parts",
            None,
            material_choices,
            None,
        ),
        (
            geometry,
            if application_defaults {
                "/default_finish_selection"
            } else {
                "/default_new_part_finish"
            },
            "Default finish for new parts",
            None,
            finish_choices,
            None,
        ),
    ]
    .into_iter()
    .map(|(category, pointer, label, range, choices, unit)| {
        field(
            w,
            FieldSpec {
                category,
                scope,
                pointer: &format!("{prefix}{pointer}"),
                label,
                range,
                choices,
                unit,
            },
        )
    })
    .collect()
}

fn fields(w: &Workspace) -> Vec<SemanticElement> {
    let mut elements = model_fields(w, false);
    let choices = crate::theme_library::choices(w)
        .into_iter()
        .map(|choice| json!({"value":choice.id,"label":choice.name}))
        .collect::<Vec<_>>();
    elements.push(field(
        w,
        FieldSpec {
            category: "application.themes",
            scope: "presentation",
            pointer: "/theme",
            label: "Theme and layout",
            range: None,
            choices: json!(choices),
            unit: None,
        },
    ));
    elements.extend(model_fields(w, true));
    for (pointer, label, range, unit) in [
        (
            "/environment_rotation_deg",
            "Environment direction",
            Some([-1_000_000., 1_000_000.]),
            Some("deg"),
        ),
        ("/exposure", "Exposure", Some([-8., 8.]), Some("stops")),
        ("/intensity", "Environment intensity", Some([0., 10.]), None),
        (
            "/key_intensity",
            "Key light intensity",
            Some([0., 10.]),
            None,
        ),
        (
            "/fill_intensity",
            "Fill light intensity",
            Some([0., 10.]),
            None,
        ),
        ("/shadows", "Cast shadows", None, None),
    ] {
        elements.push(field(
            w,
            FieldSpec {
                category: "model.lighting",
                scope: "lighting",
                pointer,
                label,
                range,
                choices: json!([]),
                unit,
            },
        ));
    }
    for (category, pointer, label, range, unit) in [
        (
            "application.interface",
            "/ui_scale",
            "Interface scale",
            Some([
                f64::from(settings_preferences::MIN_UI_SCALE),
                f64::from(settings_preferences::MAX_UI_SCALE),
            ]),
            None,
        ),
        (
            "application.interface",
            "/watch_presentation_files",
            "Reload edited layout and style files automatically",
            None,
            None,
        ),
        (
            "application.viewport",
            "/show_part_edges",
            "Show part edges",
            None,
            None,
        ),
        (
            "application.viewport",
            "/hover_centerlines",
            "Show circular centers and axes on hover",
            None,
            None,
        ),
        (
            "application.viewport",
            "/hover_midpoints",
            "Show straight-edge midpoint guides on hover",
            None,
            None,
        ),
        (
            "application.recovery",
            "/recovery_enabled",
            "Save recovery checkpoints",
            None,
            None,
        ),
        (
            "application.recovery",
            "/recovery_interval_seconds",
            "Recovery checkpoint interval",
            Some([
                settings_preferences::MIN_RECOVERY_INTERVAL_SECONDS as f64,
                settings_preferences::MAX_RECOVERY_INTERVAL_SECONDS as f64,
            ]),
            Some("s"),
        ),
    ] {
        elements.push(field(
            w,
            FieldSpec {
                category,
                scope: "application",
                pointer,
                label,
                range,
                choices: json!([]),
                unit,
            },
        ));
    }
    for (key, label) in cad_core::navigation::FIELDS {
        let range = match *key {
            "orbit_sensitivity" => Some([0.0005, 0.012]),
            "pan_sensitivity" => Some([0.0002, 0.008]),
            "zoom_sensitivity" | "roll_sensitivity" => Some([0.1, 4.]),
            "precision_scale" => Some([0.02, 0.8]),
            _ => None,
        };
        let choices = if *key == "precision_modifier" {
            json!([
                {"value":"alt","label":"Alt / Option"},
                {"value":"alt_shift","label":"Alt / Option + Shift"},
                {"value":"none","label":"None"}
            ])
        } else {
            json!([])
        };
        elements.push(field(
            w,
            FieldSpec {
                category: "application.navigation",
                scope: "application",
                pointer: &format!("/navigation/{key}"),
                label,
                range,
                choices,
                unit: None,
            },
        ));
    }
    elements
}

pub fn field_value(w: &Workspace, element: &SemanticElement) -> Option<Value> {
    let scope = element.data.get("scope")?.as_str()?;
    let pointer = element.data.get("pointer")?.as_str()?;
    source(w, scope).ok()?.pointer(pointer).cloned()
}

/// Whitelist descriptors before deserialization, then use the shared staged setters.
pub fn set_field(w: &mut Workspace, element: &SemanticElement, value: Value) -> Result<()> {
    let scope = element
        .data
        .get("scope")
        .and_then(Value::as_str)
        .ok_or_else(|| CadError::Invalid("Settings field has no scope".into()))?;
    let pointer = element
        .data
        .get("pointer")
        .and_then(Value::as_str)
        .ok_or_else(|| CadError::Invalid("Settings field has no pointer".into()))?;
    if scope == "dialog" && pointer == "/search" {
        let search = value
            .as_str()
            .filter(|search| search.chars().count() <= 256)
            .ok_or_else(|| {
                CadError::Invalid("Settings search requires at most 256 characters".into())
            })?;
        if w.settings.is_none() {
            settings::open(w, None);
        }
        if let Some(state) = &mut w.settings {
            state.search = search.into();
        }
        return Ok(());
    }
    let descriptor = fields(w)
        .into_iter()
        .find(|candidate| {
            candidate.data.get("scope").and_then(Value::as_str) == Some(scope)
                && candidate.data.get("pointer").and_then(Value::as_str) == Some(pointer)
        })
        .ok_or_else(|| CadError::Invalid("Unknown settings field".into()))?;
    if w.settings.is_none() {
        settings::open(w, descriptor.data.get("category").and_then(Value::as_str));
    }
    if w.settings.as_ref().is_some_and(|state| state.pending) {
        return Err(CadError::Invalid(
            "Wait for the current settings validation".into(),
        ));
    }
    if !matches!(scope, "application" | "presentation") && !model_editable(w) {
        return Err(CadError::Invalid(
            "Apply or cancel the active modeling operation before changing model settings".into(),
        ));
    }
    let mut candidate = source(w, scope)?;
    if scope == "application" && pointer == "/default_model/default_finish_selection" {
        let selection = value
            .as_str()
            .ok_or_else(|| CadError::Invalid("Choose a visual finish default".into()))?;
        let (kind, identity) = selection
            .split_once(':')
            .ok_or_else(|| CadError::Invalid("Invalid default finish selection".into()))?;
        let mut preferences = w
            .settings
            .as_ref()
            .map(|state| state.application.clone())
            .unwrap_or_else(|| w.application_preferences.clone());
        match kind {
            "builtin" => {
                if !cad_core::appearance::new_document_finishes().contains_key(identity) {
                    return Err(CadError::Invalid(
                        "The built-in finish no longer exists".into(),
                    ));
                }
                preferences.default_finish = None;
                preferences.default_model.default_new_part_finish = identity.into();
            }
            "snapshot" => {
                if !preferences
                    .default_finish
                    .as_ref()
                    .is_some_and(|snapshot| snapshot.finish.id == identity)
                {
                    return Err(CadError::Invalid(
                        "The saved finish default no longer exists".into(),
                    ));
                }
            }
            "catalog" | "project" => {
                let snapshot = if kind == "catalog" {
                    let package = cad_core::finish_library::package_for_catalog(identity)?;
                    settings_preferences::capture_default_package(package)?
                } else {
                    settings_preferences::capture_default_finish(
                        w.session.visible(),
                        &w.root(),
                        identity,
                    )?
                };
                preferences.default_model.default_new_part_finish = snapshot.finish.id.clone();
                preferences.default_finish = Some(snapshot);
            }
            _ => return Err(CadError::Invalid("Unknown finish default source".into())),
        }
        return settings::set_application(w, preferences);
    }
    if scope == "application" && pointer == "/default_model/default_material_selection" {
        let selection = value
            .as_str()
            .ok_or_else(|| CadError::Invalid("Choose an engineering material default".into()))?;
        let mut preferences = w
            .settings
            .as_ref()
            .map(|state| state.application.clone())
            .unwrap_or_else(|| w.application_preferences.clone());
        let (kind, identity) = selection
            .split_once(':')
            .ok_or_else(|| CadError::Invalid("Invalid default material selection".into()))?;
        if kind == "builtin" {
            if !cad_core::Document::blank_part("Part1")
                .materials
                .contains_key(identity)
            {
                return Err(CadError::Invalid(
                    "The built-in material no longer exists".into(),
                ));
            }
            preferences.default_material = None;
            preferences.default_model.default_new_part_material = identity.into();
        } else if kind == "snapshot" {
            if !preferences
                .default_material
                .as_ref()
                .is_some_and(|material| material.id == identity)
            {
                return Err(CadError::Invalid(
                    "The saved material default no longer exists".into(),
                ));
            }
        } else {
            let material = match kind {
                "catalog" => cad_core::material_library::instantiate(identity)?,
                "project" => {
                    let source = w.session.visible().materials.get(identity).ok_or_else(|| {
                        CadError::Invalid("The project material no longer exists".into())
                    })?;
                    let mut material = source.clone();
                    material.id = cad_core::id();
                    material
                }
                _ => return Err(CadError::Invalid("Unknown material default source".into())),
            };
            preferences.default_model.default_new_part_material = material.id.clone();
            preferences.default_material = Some(material);
        }
        return settings::set_application(w, preferences);
    }
    *candidate
        .pointer_mut(pointer)
        .ok_or_else(|| CadError::Invalid("Settings field is missing".into()))? = value;
    match scope {
        "model" => settings::set_model(w, serde_json::from_value::<ModelSettings>(candidate)?),
        "lighting" => settings::set_lighting(w, serde_json::from_value(candidate)?),
        "application" => settings::set_application(w, serde_json::from_value(candidate)?),
        "presentation" => settings::set_theme(
            w,
            candidate
                .get("theme")
                .and_then(Value::as_str)
                .ok_or_else(|| CadError::Invalid("Expected a theme identity".into()))?,
        ),
        _ => Err(CadError::Invalid("Unknown settings scope".into())),
    }
}

pub fn semantic_control(ui: &mut egui::Ui, w: &mut Workspace, element: &SemanticElement) {
    let Some(current) = field_value(w, element) else {
        return;
    };
    let editable = element
        .data
        .get("editable")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let mut value = current.clone();
    let properties = controls::properties(
        &element.id,
        "settings-field",
        &["semantic-control"],
        if editable { "editable" } else { "readonly" },
    );
    if properties
        .get("display")
        .is_some_and(|value| value == "none")
    {
        return;
    }
    let target = controls::current_target().map(|parent| {
        parent.child(
            &element.id,
            "settings-field",
            &["semantic-control".into()],
            if editable { "editable" } else { "readonly" },
        )
    });
    let mut draw = |ui: &mut egui::Ui| {
        ui.push_id(&element.id, |ui| {
            ui.scope(|ui| {
                controls::apply_properties(ui, &properties);
                ui.add_enabled_ui(editable, |ui| {
                    if let Some(mut candidate) = value.as_bool() {
                        let mut response = ui.checkbox(&mut candidate, &element.label);
                        if let Some(description) =
                            element.data.get("description").and_then(Value::as_str)
                        {
                            response = response.on_hover_text(description);
                        }
                        let changed = response.changed();
                        value = json!(candidate);
                        return changed;
                    }
                    ui.horizontal_wrapped(|ui| {
                        controls::label(
                            ui,
                            &format!("{}.label", element.id),
                            "settings-field-label",
                            &[],
                            &element.label,
                            "ready",
                        );
                        if let Some(choices) = element
                            .data
                            .get("choices")
                            .and_then(Value::as_array)
                            .filter(|choices| !choices.is_empty())
                        {
                            let label = choices
                                .iter()
                                .find(|choice| choice.get("value") == Some(&value))
                                .and_then(|choice| choice.get("label"))
                                .and_then(Value::as_str)
                                .unwrap_or_else(|| value.as_str().unwrap_or("Choose…"));
                            egui::ComboBox::from_id_salt(&element.id)
                                .selected_text(label)
                                .show_ui(ui, |ui| {
                                    for choice in choices {
                                        if let (Some(candidate), Some(label)) = (
                                            choice.get("value"),
                                            choice.get("label").and_then(Value::as_str),
                                        ) {
                                            ui.selectable_value(
                                                &mut value,
                                                candidate.clone(),
                                                label,
                                            );
                                        }
                                    }
                                });
                            return value != current;
                        }
                        if let Some(mut candidate) = value.as_f64() {
                            let range = element
                                .data
                                .get("range")
                                .and_then(Value::as_array)
                                .and_then(|range| {
                                    Some(range.first()?.as_f64()?..=range.get(1)?.as_f64()?)
                                });
                            let integral = value.is_u64() || value.is_i64();
                            let speed = element
                                .data
                                .get("step")
                                .and_then(Value::as_f64)
                                .unwrap_or_else(|| {
                                    if integral {
                                        1.
                                    } else if current
                                        .as_f64()
                                        .is_some_and(|number| number.abs() < 0.1)
                                    {
                                        0.0001
                                    } else {
                                        0.01
                                    }
                                });
                            let mut widget = egui::DragValue::new(&mut candidate)
                                .speed(speed)
                                .max_decimals(9);
                            if let Some(range) = range {
                                widget = widget.range(range);
                            }
                            if integral {
                                widget = widget.max_decimals(0);
                            }
                            if let Some(unit) = element.data.get("unit").and_then(Value::as_str) {
                                widget = widget.suffix(format!(" {unit}"));
                            }
                            let changed = ui.add(widget).changed();
                            value = if value.is_u64() {
                                json!(candidate.round().max(0.) as u64)
                            } else if value.is_i64() {
                                json!(candidate.round() as i64)
                            } else {
                                json!(candidate)
                            };
                            return changed;
                        }
                        false
                    })
                    .inner
                })
                .inner
            })
            .inner
        })
        .inner
    };
    let changed = if let Some(target) = target {
        controls::with_target(target, ui, draw)
    } else {
        draw(ui)
    };
    if changed && let Err(error) = set_field(w, element, value) {
        let message = format!("{}: {error}", element.label);
        w.status = message.clone();
        if let Some(state) = &mut w.settings {
            state.diagnostic = Some(message);
        }
    }
}

pub fn semantics(w: &Workspace) -> Vec<SemanticElement> {
    let pending = w.settings.as_ref().is_some_and(|state| state.pending);
    let mut elements = vec![SemanticElement {
        id: "settings:dialog".into(),
        role: "settings-dialog".into(),
        label: "Settings".into(),
        value: None,
        state: if w.settings.is_some() {
            "open"
        } else {
            "closed"
        }
        .into(),
        commands: vec![
            "settings".into(),
            "settings.apply".into(),
            "settings.ok".into(),
            "settings.cancel".into(),
        ],
        relationships: vec![],
        data: json!({"contract":"tcad.settings-presentation","version":1,"modal":true,"model_editable":model_editable(w),"pending":pending,"category":w.settings.as_ref().map(|state|&state.category),"search":w.settings.as_ref().map(|state|&state.search),"diagnostic":w.settings.as_ref().and_then(|state|state.diagnostic.as_ref()),"scopes":["model","application","presentation"]}),
    }];
    let search = w
        .settings
        .as_ref()
        .map(|state| state.search.as_str())
        .unwrap_or("");
    elements.push(SemanticElement {
        id:"settings:search".into(), role:"settings-search".into(), label:"Find a preference".into(), value:Some(search.into()),
        state:if w.settings.is_some() {"editable"} else {"closed"}.into(), commands:vec!["settings".into()], relationships:vec!["settings:dialog".into()],
        data:json!({"setter":"settings","scope":"dialog","pointer":"/search","json":search,"editable":w.settings.is_some(),"accessibility_label":"Find a preference","model_mutation":false}),
    });
    for (id, label, scope) in CATEGORIES {
        elements.push(SemanticElement {
            id:format!("settings:category:{id}"), role:"settings-category".into(), label:(*label).into(), value:Some((*scope).into()),
            state:if w.settings.as_ref().is_some_and(|state|state.category == *id) {"selected"} else {"ready"}.into(),
            commands:vec![format!("settings.open.{id}")], relationships:vec!["settings:dialog".into()],
            data:json!({"category":id,"scope":scope.to_lowercase(),"accessibility_label":format!("{scope}: {label}")}),
        });
        elements.push(SemanticElement {
            id: format!("settings:description:{id}"),
            role: "settings-description".into(),
            label: category_description(id).into(),
            value: None,
            state: "ready".into(),
            commands: vec![],
            relationships: vec![format!("settings:category:{id}")],
            data: json!({"category":id,"scope":scope.to_lowercase(),"model_mutation":false}),
        });
    }
    for (command, label) in [
        ("settings.apply", "Apply"),
        ("settings.ok", "OK"),
        ("settings.cancel", "Cancel"),
        ("settings.presentation", "Layout and theme…"),
        ("settings.theme-load", "Load theme…"),
        ("settings.theme-export", "Export theme…"),
        ("content-repositories", "Get libraries…"),
    ] {
        let available = w.settings.is_some() && (command == "settings.cancel" || !pending);
        elements.push(SemanticElement {
            id:format!("command:{command}"), role:"settings-action".into(), label:label.into(), value:None,
            state:if available {"available"} else {"disabled"}.into(), commands:vec![command.into()], relationships:vec!["settings:dialog".into()],
            data:json!({"actions":[{"command":command,"enabled":available,"disabled_reason":if available {None} else {Some("Open settings and wait for any current validation")}}],"accessibility_label":label}),
        });
    }
    elements.extend(fields(w));
    elements
}

fn category_description(category: &str) -> &'static str {
    match category {
        "model.units" => {
            "Saved with this model. Display units and precision do not resize geometry or change explicit unit expressions."
        }
        "model.geometry" => {
            "Saved with this model. Boundary accuracy, Boolean search resolution, and display chord tolerance are independent. Default material and finish apply to newly created parts."
        }
        "model.lighting" => {
            "Saved with this model. Lighting updates the viewport while you edit. Theme overrides may alter the displayed lighting."
        }
        "application.interface" => {
            "Saved for this user across projects. Layout and theme files give independent control over composition, density, and styling."
        }
        "application.viewport" => {
            "Saved for this user across projects. Edges, circular centers, and straight-edge midpoint guides preview immediately. Apply saves them; Cancel restores the starting display. Theme styles can override display and guide styling."
        }
        "application.themes" => {
            "Choose a complete layout and theme, or load a CSS / JSON style or presentation package. Changes preview immediately. Apply saves your default; Cancel restores the previous presentation."
        }
        "application.navigation" => {
            "Saved for this user across projects. Sensitivity and gesture directions preview immediately. Cancel restores the starting preferences."
        }
        "application.recovery" => {
            "Saved for this user across projects. Recovery checkpoints preserve unsaved committed definitions; they do not replace saving a project."
        }
        "application.defaults" => {
            "Applied only when creating a new model. Existing models keep their saved preferences."
        }
        "application.libraries" => {
            "Optional profiles, textures, materials, standard parts and themes in your own Git repositories. Library actions save immediately; Settings Apply and Cancel do not undo them. Existing model definitions keep their saved copies."
        }
        "application.git" => {
            "Personal Git accounts, keys and server trust. Connection actions save immediately; Settings Apply and Cancel affect model and interface preferences only."
        }
        _ => "Preferences use separate model and application scopes.",
    }
}

fn styled_length(
    properties: &BTreeMap<String, String>,
    key: &str,
    extent: f32,
    fallback: f32,
) -> f32 {
    properties
        .get(key)
        .and_then(|value| value.trim().strip_suffix('%'))
        .and_then(|value| value.parse::<f32>().ok())
        .filter(|value| value.is_finite())
        .map(|value| extent * value / 100.)
        .unwrap_or_else(|| controls::number(properties, key, fallback))
}

pub fn show(ctx: &egui::Context, w: &mut Workspace) {
    if w.settings.is_none() {
        return;
    }
    let parent = controls::current_target().unwrap_or_else(|| {
        cad_core::presentation::Region {
            id: "workspace".into(),
            role: "workspace".into(),
            component: None,
            rect: [0.; 4],
            classes: vec![],
            resizable: false,
        }
        .style_target(&w.view.active_layout, "ready")
    });
    let target = parent.child(
        "settings:dialog",
        "settings-dialog",
        &["settings-surface".into()],
        "open",
    );
    let id = egui::Id::new("tcad:settings-modal");
    let properties = controls::properties(
        "settings:dialog",
        "settings-dialog",
        &["settings-surface"],
        "open",
    );
    // A user composition can replace the default surface while keeping the same transaction and fields.
    if properties
        .get("display")
        .is_some_and(|value| value == "none")
        || properties
            .get("visibility")
            .is_some_and(|value| value == "hidden")
    {
        return;
    }
    let bounds = ctx.content_rect();
    let maximum = egui::vec2(
        (bounds.width() - 32.).max(240.),
        (bounds.height() - 32.).max(240.),
    );
    let width = styled_length(&properties, "width", bounds.width(), 700.).clamp(240., maximum.x);
    let height = styled_length(&properties, "height", bounds.height(), 460.).clamp(240., maximum.y);
    let area = if ["left", "right", "top", "bottom"]
        .iter()
        .any(|key| properties.contains_key(*key))
    {
        let coordinate = |start: &str, end: &str, minimum: f32, extent: f32, size: f32| {
            if properties.contains_key(start) {
                minimum + styled_length(&properties, start, extent, 0.)
            } else if properties.contains_key(end) {
                minimum + extent - size - styled_length(&properties, end, extent, 0.)
            } else {
                minimum + (extent - size) * 0.5
            }
        };
        egui::Area::new(id)
            .kind(egui::UiKind::Modal)
            .sense(egui::Sense::hover())
            .order(egui::Order::Foreground)
            .interactable(true)
            .fixed_pos(egui::pos2(
                coordinate("left", "right", bounds.left(), bounds.width(), width),
                coordinate("top", "bottom", bounds.top(), bounds.height(), height),
            ))
    } else {
        egui::Modal::default_area(id)
    };
    let response = egui::Modal::new(id).area(area).frame(egui::Frame::NONE).show(ctx, |ui| {
        controls::with_target(target, ui, |ui| {
            let properties = controls::properties("settings:dialog", "settings-dialog", &["settings-surface"], "open");
            let mut frame = controls::frame_from_properties(ui, &properties);
            if !properties.contains_key("background") { frame.fill = ui.visuals().window_fill; }
            if !properties.contains_key("border") && !properties.contains_key("border-color") { frame.stroke = ui.visuals().window_stroke; }
            frame.show(ui, |ui| {
                controls::apply_properties(ui, &properties);
                let available = ctx.content_rect().size();
                let maximum = egui::vec2((available.x - 32.).max(240.), (available.y - 32.).max(240.));
                let width = styled_length(&properties, "width",available.x,700.).clamp(240., maximum.x);
                let height = styled_length(&properties, "height",available.y,460.).clamp(240., maximum.y);
                let override_width = properties.contains_key("width");
                let override_height = properties.contains_key("height");
                egui::Resize::default().id_salt("settings:resize").default_size(egui::vec2(width,height))
                    .min_size(egui::vec2(if override_width {width} else {240.},if override_height {height} else {240.}))
                    .max_size(egui::vec2(if override_width {width} else {maximum.x},if override_height {height} else {maximum.y}))
                    .resizable([!override_width,!override_height]).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        controls::label(ui,"settings:title","settings-title",&[],"Settings","ready");
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if controls::button(ui,"settings:close","settings-action",&[],"×",true,false).on_hover_text("Cancel settings").clicked() {settings::cancel(w);}
                        });
                    });
                    let Some(state) = &mut w.settings else { return; };
                    let search_properties = controls::properties("settings:search", "settings-search", &[], "ready");
                    ui.scope(|ui| {
                        controls::apply_properties(ui,&search_properties);
                        ui.add(egui::TextEdit::singleline(&mut state.search).id_salt("settings:search").hint_text("Find a preference…").char_limit(256).desired_width(f32::INFINITY));
                    });
                    let category = state.category.clone();
                    let search = state.search.to_lowercase();
                    let pending = state.pending;
                    let content_height = (ui.available_height() - 65.).max(90.);
                    ui.horizontal_top(|ui| {
                        let sidebar_properties = controls::properties("settings:categories", "settings-categories", &[], "ready");
                        let sidebar_width = styled_length(&sidebar_properties,"width",ui.available_width(),175.).clamp(90.,(ui.available_width()*0.45).max(90.));
                        ui.allocate_ui_with_layout(egui::vec2(sidebar_width,content_height),egui::Layout::top_down(egui::Align::Min), |ui| {
                            controls::apply_properties(ui,&sidebar_properties);
                            egui::ScrollArea::vertical().id_salt("settings:category-scroll").max_height(content_height).show(ui, |ui| {
                                for scope in ["Model", "Application"] {
                                    controls::label(ui,&format!("settings:scope:{scope}"),"settings-scope",&[],scope,"ready");
                                    for (id,label,group) in CATEGORIES.iter().filter(|(_,_,group)|*group == scope) {
                                        if controls::button(ui,&format!("settings:category:{id}"),"settings-category",&[],label,!pending,category == *id && search.is_empty()).on_hover_text(format!("{group}: {label}")).clicked()
                                            && let Some(state) = &mut w.settings {state.category = (*id).into();state.search.clear();
                                        }
                                    }
                                    ui.add_space(8.);
                                }
                            });
                        });
                        ui.separator();
                        let panel_properties = controls::properties("settings:content", "settings-content", &[], "ready");
                        ui.allocate_ui_with_layout(egui::vec2(ui.available_width(),content_height),egui::Layout::top_down(egui::Align::Min), |ui| {
                            controls::apply_properties(ui,&panel_properties);
                            egui::ScrollArea::vertical().id_salt("settings:field-scroll").max_height(content_height).show(ui, |ui| {
                                let all_fields = fields(w);
                                let mut shown = 0;
                                for (id,label,scope) in CATEGORIES {
                                    if *id == "application.libraries" && (category == *id && search.is_empty() || !search.is_empty() && "content libraries repositories profiles materials hardware standard parts textures finishes themes".contains(&search)) {
                                        controls::label(ui,"settings:heading:application.libraries","settings-heading",&[],"Application · Content libraries","ready");
                                        controls::label(ui,"settings:description:application.libraries","settings-description",&[],category_description(id),"ready");
                                        crate::content_repositories_ui::show(ui,w);
                                        shown += 1;
                                        continue;
                                    }
                                    if *id == "application.git" && (category == *id && search.is_empty() || !search.is_empty() && "git credentials ssh keys connections accounts".contains(&search)) {
                                        controls::label(ui,"settings:heading:application.git","settings-heading",&[],"Application · Git connections","ready");
                                        crate::git_connections_ui::show(ui,w);
                                        shown += 1;
                                        continue;
                                    }
                                    let selected_fields = all_fields.iter().filter(|element|element.data.get("category").and_then(Value::as_str) == Some(*id))
                                        .filter(|element| if search.is_empty() {*id == category} else {format!("{scope} {label} {}",element.label).to_lowercase().contains(&search)}).collect::<Vec<_>>();
                                    if selected_fields.is_empty() {continue;}
                                    controls::label(ui,&format!("settings:heading:{id}"),"settings-heading",&[],&format!("{scope} · {label}"),"ready");
                                    controls::label(ui,&format!("settings:description:{id}"),"settings-description",&[],category_description(id),"ready");
                                    if scope == &"Model" && !model_editable(w) {
                                        controls::label(ui,"settings:model-readonly","settings-diagnostic",&[],"Apply or cancel the active modeling operation to edit model preferences.","readonly");
                                    }
                                    for element in selected_fields {semantic_control(ui,w,element);shown += 1;}
                                    if *id == "model.units" {
                                        let model = &w.session.visible().settings;
                                        controls::label(ui,"settings:units-example","settings-description",&[],&format!("Example: {} · {}",model.format_length(25.4),model.format_angle_radians(std::f64::consts::FRAC_PI_4)),"ready");
                                    }
                                    if *id == "application.interface" && controls::button(ui,"command:settings.presentation","settings-action",&[],"Layout and theme…",!pending,false).clicked() {
                                        settings::action(w,"settings.presentation");
                                    }
                                    if *id == "application.themes" {
                                        if controls::button(ui,"command:content-repositories","settings-action",&[],"Get libraries…",!pending,false).clicked() {
                                            crate::content_repositories_ui::action(w,"content-repositories",None);
                                        }
                                        crate::workspace_controls::confirmation_preview(ui);
                                        let selected = crate::theme_library::selected(w);
                                        if let Some(choice) = crate::theme_library::choices(w).into_iter().find(|choice|choice.id == selected) {
                                            controls::label(ui,"settings:theme-description","settings-description",&[],&choice.description,"ready");
                                        }
                                        ui.horizontal_wrapped(|ui| {
                                            for (command,label) in [("settings.theme-load","Load theme…"),("settings.theme-export","Export theme…"),("settings.presentation","Edit layout and style…")] {
                                                if controls::button(ui,&format!("command:{command}"),"settings-action",&[],label,!pending,false).clicked() {settings::action(w,command);}
                                            }
                                        });
                                    }
                                    ui.add_space(8.);
                                }
                                if shown == 0 {controls::label(ui,"settings:empty-search","settings-description",&[],"No matching preferences.","ready");}
                            });
                        });
                    });
                    let Some(state) = &w.settings else { return; };
                    let diagnostic = state.diagnostic.clone();
                    let pending = state.pending;
                    if let Some(diagnostic) = diagnostic {controls::label(ui,"settings:diagnostic","settings-diagnostic",&[],&diagnostic,"invalid");}
                    else if pending {controls::label(ui,"settings:pending","settings-status",&[],"Validating model preferences…","pending");}
                    ui.separator();
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if controls::button(ui,"command:settings.ok","settings-action",&["primary"],"OK",!pending,false).clicked() {settings::apply(w,true);}
                        if controls::button(ui,"command:settings.cancel","settings-action",&[],"Cancel",true,false).clicked() {settings::cancel(w);}
                        if controls::button(ui,"command:settings.apply","settings-action",&[],"Apply",!pending,false).clicked() {settings::apply(w,false);}
                    });
                });
            });
        });
    });
    if response.should_close() && w.settings.is_some() {
        settings::cancel(w);
    }
}
