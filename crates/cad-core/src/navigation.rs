//! View-only navigation intent, independent of platform events and widgets.
use crate::{CadError, Result, presentation::SemanticElement};
use serde::{Deserialize, Serialize};

pub const CONTRACT_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrecisionModifier {
    #[default]
    Alt,
    AltShift,
    None,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Preferences {
    pub version: u32,
    pub alternate_pan: bool,
    pub invert_orbit: [bool; 2],
    pub invert_pan: [bool; 2],
    pub invert_zoom: bool,
    pub invert_roll: bool,
    /// Radians per logical point, applied to both orbit axes.
    pub orbit_sensitivity: f32,
    /// View-distance multiples per logical point.
    pub pan_sensitivity: f32,
    pub zoom_sensitivity: f32,
    pub roll_sensitivity: f32,
    pub precision_modifier: PrecisionModifier,
    pub precision_scale: f32,
    pub native_rotation: bool,
    pub smart_zoom: bool,
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            version: CONTRACT_VERSION,
            alternate_pan: false,
            invert_orbit: [false; 2],
            invert_pan: [false; 2],
            invert_zoom: false,
            invert_roll: false,
            orbit_sensitivity: 0.0035,
            pan_sensitivity: 0.002,
            zoom_sensitivity: 1.,
            roll_sensitivity: 1.,
            precision_modifier: PrecisionModifier::Alt,
            precision_scale: 0.2,
            native_rotation: true,
            smart_zoom: true,
        }
    }
}
impl Preferences {
    pub fn validate(&self) -> Result<()> {
        if self.version != CONTRACT_VERSION
            || ![
                (self.orbit_sensitivity, 0.0005, 0.012),
                (self.pan_sensitivity, 0.0002, 0.008),
                (self.zoom_sensitivity, 0.1, 4.),
                (self.roll_sensitivity, 0.1, 4.),
                (self.precision_scale, 0.02, 0.8),
            ]
            .into_iter()
            .all(|(value, low, high)| value.is_finite() && (low..=high).contains(&value))
        {
            return Err(CadError::Invalid(
                "Invalid navigation preference version or sensitivity range".into(),
            ));
        }
        Ok(())
    }
    pub fn set(&mut self, pointer: &str, value: serde_json::Value) -> Result<()> {
        if !FIELDS
            .iter()
            .any(|(field, _)| format!("/{field}") == pointer)
        {
            return Err(CadError::Invalid("Unknown navigation preference".into()));
        }
        let mut source = serde_json::to_value(&*self)?;
        *source
            .pointer_mut(pointer)
            .ok_or_else(|| CadError::Invalid("Navigation preference is missing".into()))? = value;
        let candidate: Self = serde_json::from_value(source)?;
        candidate.validate()?;
        *self = candidate;
        Ok(())
    }
}

pub const FIELDS: &[(&str, &str)] = &[
    (
        "alternate_pan",
        "Two fingers pan; Shift orbits outside sketches",
    ),
    ("invert_orbit/0", "Invert horizontal orbit"),
    ("invert_orbit/1", "Invert vertical orbit"),
    ("invert_pan/0", "Invert horizontal pan"),
    ("invert_pan/1", "Invert vertical pan"),
    ("invert_zoom", "Invert zoom"),
    ("invert_roll", "Invert rotation gesture"),
    ("orbit_sensitivity", "Orbit sensitivity"),
    ("pan_sensitivity", "Pan sensitivity"),
    ("zoom_sensitivity", "Zoom sensitivity"),
    ("roll_sensitivity", "Rotation sensitivity"),
    ("precision_modifier", "Navigation precision modifier"),
    ("precision_scale", "Precision speed multiplier"),
    ("native_rotation", "Two-finger rotation rolls the view"),
    ("smart_zoom", "Native smart zoom fits selection or assembly"),
];
pub fn semantic_fields(preferences: &Preferences) -> Vec<SemanticElement> {
    let source = serde_json::to_value(preferences).unwrap_or_default();
    let mut fields = vec![SemanticElement {
        id: "view:navigation".into(),
        role: "navigation-preferences".into(),
        label: "Trackpad Navigation".into(),
        value: None,
        state: "view-only".into(),
        commands: vec![
            "navigation.preferences".into(),
            "navigation.reset".into(),
            "navigation.upright".into(),
            "navigation.stop".into(),
        ],
        relationships: vec![],
        data: serde_json::json!({"contract":"tcad.navigation","version":CONTRACT_VERSION,"preferences":preferences,"model_mutation":false,"orbit":"unlimited_pole_crossing","precision":"modifier","inertia":"no_application_generated_inertia","touchscreen_navigation":"unsupported","event_ownership":"primary_window_phase","native_gesture_availability":"platform_dependent"}),
    }];
    for (field, label) in FIELDS {
        let pointer = format!("/{field}");
        let value = source.pointer(&pointer).cloned().unwrap_or_default();
        let range = match *field {
            "orbit_sensitivity" => Some([0.0005, 0.012]),
            "pan_sensitivity" => Some([0.0002, 0.008]),
            "zoom_sensitivity" | "roll_sensitivity" => Some([0.1, 4.]),
            "precision_scale" => Some([0.02, 0.8]),
            _ => None,
        };
        fields.push(SemanticElement {
            id: format!("view:navigation:{field}"), role: "navigation-preference".into(), label: (*label).into(),
            value: Some(value.as_str().map(str::to_owned).unwrap_or_else(||value.to_string())), state: "view-only".into(),
            commands: vec!["navigation.set".into(), "navigation.preferences".into()], relationships: vec!["view:navigation".into()],
            data: serde_json::json!({"pointer":pointer,"json":value,"editable":true,"accessibility_label":label,"target":{"kind":"navigation_preference","pointer":pointer},"model_mutation":false,"range":range,"options":if *field=="precision_modifier" {Some(["alt","alt_shift","none"])}else{None}}),
        });
    }
    fields
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Start,
    Move,
    End,
    Cancel,
}
#[derive(Clone, Copy, Debug)]
pub enum Motion {
    Scroll {
        delta: [f32; 2],
        lines: bool,
    },
    Pinch {
        logarithm: f32,
    },
    /// Clockwise image rotation in radians.
    Rotate {
        radians: f32,
    },
    Pan {
        delta: [f32; 2],
    },
    SmartZoom,
    Cancel,
}
#[derive(Clone, Copy, Debug)]
pub struct Event {
    pub phase: Option<Phase>,
    pub motion: Motion,
}
