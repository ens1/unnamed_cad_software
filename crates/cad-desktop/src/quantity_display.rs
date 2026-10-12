//! Compact quantity presentation; authored text remains the editing authority.
use cad_core::{presentation::SemanticElement, settings::ModelSettings};
use serde_json::{Value, json};
use std::cell::RefCell;

pub const CONTRACT_VERSION: u32 = 1;
pub fn source_fields_valid(w: &crate::Workspace) -> bool {
    !w.fields
        .keys()
        .any(|key| key.starts_with("presentation-field-error:"))
}
pub fn clear_errors(w: &mut crate::Workspace) {
    w.fields.retain(|key, _| {
        !key.starts_with("presentation-field-error:") && !key.starts_with("presentation-field:")
    });
}
thread_local! {
    static SETTINGS: RefCell<ModelSettings> = RefCell::new(ModelSettings::default());
}
pub fn begin_frame(settings: &ModelSettings) {
    SETTINGS.with(|current| *current.borrow_mut() = settings.clone());
}
fn decimal(value: f64, places: u8) -> String {
    let mut text = format!(
        "{value:.precision$}",
        precision = usize::from(places.min(8))
    );
    if text.parse::<f64>().is_ok_and(|rounded| rounded == 0.) && value != 0. {
        // Do not make a small nonzero length or angle appear to be exactly zero.
        return format!(
            "{value:.precision$e}",
            precision = usize::from(places.min(8))
        );
    }
    if text.contains('.') {
        while text.ends_with('0') {
            text.pop();
        }
        if text.ends_with('.') {
            text.pop();
        }
    }
    if text == "-0" { "0".into() } else { text }
}
/// Only a single finite literal is eligible. Parameters, equations, functions,
/// tolerances and unsupported units retain their exact authored expression.
pub fn expression_with(settings: &ModelSettings, source: &str) -> Option<String> {
    let source = source.trim();
    if let Ok(value) = source.parse::<f64>() {
        return value
            .is_finite()
            .then(|| decimal(value, settings.length_decimal_places));
    }
    for (unit, factor, angle) in [
        ("micrometers", 0.001, false),
        ("micrometer", 0.001, false),
        ("microinches", 0.0000254, false),
        ("microinch", 0.0000254, false),
        ("uin", 0.0000254, false),
        ("µm", 0.001, false),
        ("um", 0.001, false),
        ("mm", 1., false),
        ("cm", 10., false),
        ("ft", 304.8, false),
        ("in", 25.4, false),
        ("deg", 1., true),
        ("rad", 180. / std::f64::consts::PI, true),
        ("m", 1000., false),
    ] {
        let Some(number) = source.strip_suffix(unit) else {
            continue;
        };
        let Ok(value) = number.trim().parse::<f64>() else {
            continue;
        };
        let value = value * factor;
        if !value.is_finite() {
            return None;
        }
        return Some(if angle {
            format!("{} deg", decimal(value, settings.angle_decimal_places))
        } else {
            format!(
                "{} {}",
                decimal(
                    value / settings.display_length_unit.millimeters_per_unit(),
                    settings.length_decimal_places
                ),
                settings.display_length_unit.symbol()
            )
        });
    }
    None
}
pub fn expression(source: &str) -> String {
    SETTINGS.with(|settings| {
        expression_with(&settings.borrow(), source).unwrap_or_else(|| source.into())
    })
}
/// JSON numbers keep their published canonical unit, avoiding any ambiguity in
/// numeric setters. Explicit-unit expression strings use model display units.
pub fn field(settings: &ModelSettings, element: &SemanticElement, value: &Value) -> String {
    let source = value
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| value.to_string());
    let descriptor = element.value_descriptor();
    if element.data.get("multiline").and_then(Value::as_bool) == Some(true)
        || matches!(descriptor.kind, cad_core::presentation::ValueKind::Json)
    {
        return source;
    }
    if let Some(number) = value.as_f64().filter(|number| number.is_finite()) {
        if value.is_i64() || value.is_u64() {
            return source;
        }
        let places = if matches!(descriptor.unit.as_deref(), Some("deg" | "rad")) {
            settings.angle_decimal_places
        } else {
            settings.length_decimal_places
        };
        return decimal(number, places);
    }
    if matches!(descriptor.kind, cad_core::presentation::ValueKind::Quantity)
        || descriptor
            .unit
            .as_deref()
            .is_some_and(|unit| !unit.is_empty())
    {
        return expression_with(settings, &source).unwrap_or(source);
    }
    source
}
pub fn publish(settings: &ModelSettings, element: &mut SemanticElement) {
    let descriptor = element.value_descriptor();
    if !matches!(
        descriptor.kind,
        cad_core::presentation::ValueKind::Number | cad_core::presentation::ValueKind::Quantity
    ) && descriptor.unit.as_deref().is_none_or(str::is_empty)
    {
        return;
    }
    let source = descriptor
        .value
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| descriptor.value.to_string());
    let display = field(settings, element, &descriptor.value);
    element.data["quantity_presentation"] = json!({"contract":"tcad.quantity-presentation","version":CONTRACT_VERSION,
        "display_text":display,"source_text":source,"source_value":descriptor.value,"source_unit":descriptor.unit,
        "model_length_unit":settings.display_length_unit.symbol(),"length_decimal_places":settings.length_decimal_places,
        "angle_decimal_places":settings.angle_decimal_places,"edit_text":"exact-source","changes_source_on_focus":false});
}
