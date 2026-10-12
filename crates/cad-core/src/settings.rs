//! Versioned model preferences. Canonical definitions remain in millimeters and radians.
use crate::{CadError, Document, Id, Result};
use serde::{Deserialize, Serialize};

pub const CONTRACT_VERSION: u32 = 1;

/// Display units never rescale stored dimensions, coordinates or explicit expressions.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LengthUnit {
    #[default]
    Millimeters,
    Inches,
    Centimeters,
    Meters,
    Feet,
}
impl LengthUnit {
    pub const ALL: [Self; 5] = [
        Self::Millimeters,
        Self::Inches,
        Self::Centimeters,
        Self::Meters,
        Self::Feet,
    ];

    pub const fn symbol(self) -> &'static str {
        match self {
            Self::Millimeters => "mm",
            Self::Inches => "in",
            Self::Centimeters => "cm",
            Self::Meters => "m",
            Self::Feet => "ft",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Millimeters => "Millimeters",
            Self::Inches => "Inches",
            Self::Centimeters => "Centimeters",
            Self::Meters => "Meters",
            Self::Feet => "Feet",
        }
    }

    pub const fn millimeters_per_unit(self) -> f64 {
        match self {
            Self::Millimeters => 1.,
            Self::Inches => 25.4,
            Self::Centimeters => 10.,
            Self::Meters => 1000.,
            Self::Feet => 304.8,
        }
    }

    pub fn format_length(self, millimeters: f64, decimal_places: u8) -> String {
        format!(
            "{:.precision$} {}",
            millimeters / self.millimeters_per_unit(),
            self.symbol(),
            precision = decimal_places.min(8) as usize,
        )
    }
}

/// Saved with the model, edited through the shared document transaction and undo history.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct ModelSettings {
    pub version: u32,
    pub display_length_unit: LengthUnit,
    pub length_decimal_places: u8,
    pub angle_decimal_places: u8,
    /// Worker evaluation and tessellation tolerance in canonical millimeters.
    pub evaluation_tolerance_mm: f64,
    /// Applied to subsequently created part definitions; existing assignments are preserved.
    pub default_new_part_finish: Id,
    pub production: crate::production::Preferences,
}
impl Default for ModelSettings {
    fn default() -> Self {
        Self {
            version: CONTRACT_VERSION,
            display_length_unit: LengthUnit::default(),
            length_decimal_places: 3,
            angle_decimal_places: 2,
            evaluation_tolerance_mm: 0.15,
            default_new_part_finish: crate::appearance::default_finish(),
            production: Default::default(),
        }
    }
}
impl ModelSettings {
    /// Validate portable scalar preferences without requiring an owning document.
    pub fn validate_values(&self) -> Result<()> {
        self.production.validate()?;
        if self.version != CONTRACT_VERSION {
            return Err(CadError::Invalid(format!(
                "Unsupported model settings contract version {}",
                self.version
            )));
        }
        if self.length_decimal_places > 8 || self.angle_decimal_places > 8 {
            return Err(CadError::Invalid(
                "Model length and angle display precision must be 0–8 decimal places".into(),
            ));
        }
        if !self.evaluation_tolerance_mm.is_finite()
            || !(0.001..=1.).contains(&self.evaluation_tolerance_mm)
        {
            return Err(CadError::Invalid(
                "Model evaluation tolerance must be finite and between 0.001 and 1 mm".into(),
            ));
        }
        let finish = &self.default_new_part_finish;
        if finish.is_empty()
            || finish.len() > 256
            || matches!(finish.as_str(), "." | "..")
            || finish
                .chars()
                .any(|c| c.is_control() || matches!(c, '/' | '\\' | ':'))
        {
            return Err(CadError::Invalid(
                "The default new-part finish requires a portable stable identity".into(),
            ));
        }
        Ok(())
    }

    pub fn validate(&self, document: &Document) -> Result<()> {
        self.validate_values()?;
        if !document
            .finishes
            .contains_key(&self.default_new_part_finish)
        {
            return Err(CadError::Invalid(format!(
                "The default new-part finish {} does not exist in this model",
                self.default_new_part_finish
            )));
        }
        Ok(())
    }

    pub fn format_length(&self, millimeters: f64) -> String {
        self.display_length_unit
            .format_length(millimeters, self.length_decimal_places)
    }

    pub fn format_angle_radians(&self, radians: f64) -> String {
        format!(
            "{:.precision$} deg",
            radians.to_degrees(),
            precision = self.angle_decimal_places.min(8) as usize,
        )
    }
}

/// Omitting the unchanged default preserves existing native source fingerprints.
pub fn is_default_settings(settings: &ModelSettings) -> bool {
    *settings == ModelSettings::default()
}
