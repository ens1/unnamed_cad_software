//! Application-owned accuracy policy in canonical millimeters. Numerical
//! boundary acceptance, intersection discovery, and display subdivision are
//! independent budgets; a search mesh never establishes B-rep accuracy.
use crate::{CadError, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const VERSION: u32 = 1;
pub const MIN_LINEAR_MM: f64 = 1e-6;
pub const MAX_LINEAR_MM: f64 = 1e-3;
pub const MIN_SEARCH_MM: f64 = 1e-4;
pub const MAX_SEARCH_MM: f64 = 1.;
pub const MIN_DISPLAY_MM: f64 = 1e-6;
pub const MAX_DISPLAY_MM: f64 = 1.;

/// Saved modeling policy. These values never derive from viewport quality.
/// Existing native constructors may enforce stricter geometric restrictions.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Modeling {
    pub version: u32,
    /// Maximum accepted boundary/carrier residual where the adapter supports
    /// tolerance-controlled admission; not permission to accept invalid topology.
    pub linear_mm: f64,
    /// Coarse discovery/subdivision resolution for native Boolean/intersection
    /// searches. Subsequent native boundary validation uses linear_mm.
    pub boolean_search_mm: f64,
}
impl Default for Modeling {
    fn default() -> Self {
        Self {
            version: VERSION,
            linear_mm: MIN_LINEAR_MM,
            boolean_search_mm: 0.05,
        }
    }
}
impl Modeling {
    pub fn validate(&self) -> Result<()> {
        if self.version != VERSION
            || !self.linear_mm.is_finite()
            || !(MIN_LINEAR_MM..=MAX_LINEAR_MM).contains(&self.linear_mm)
            || !self.boolean_search_mm.is_finite()
            || !(MIN_SEARCH_MM..=MAX_SEARCH_MM).contains(&self.boolean_search_mm)
            || self.boolean_search_mm < self.linear_mm
        {
            return Err(CadError::Invalid(
                "Modeling tolerance requires contract 1, a boundary residual of 0.000001–0.001 mm, and Boolean search resolution of 0.0001–1 mm no smaller than the boundary residual".into(),
            ));
        }
        Ok(())
    }

    /// Includes all numerical policy values, independent of display quality.
    pub fn fingerprint(&self) -> String {
        let mut digest = Sha256::new();
        digest.update(self.version.to_le_bytes());
        digest.update(self.linear_mm.to_bits().to_le_bytes());
        digest.update(self.boolean_search_mm.to_bits().to_le_bytes());
        format!("{:x}", digest.finalize())
    }
}

/// Report the actual policy used by an evaluation/export. Display refinements
/// may replace meshes without changing the modeling portion of this record.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
pub struct Accuracy {
    pub version: u32,
    pub modeling: Modeling,
    pub display_mm: f64,
}
impl Accuracy {
    pub fn new(modeling: Modeling, display_mm: f64) -> Result<Self> {
        modeling.validate()?;
        validate_display(display_mm)?;
        Ok(Self {
            version: VERSION,
            modeling,
            display_mm,
        })
    }
}

pub fn validate_display(display_mm: f64) -> Result<()> {
    if !display_mm.is_finite() || !(MIN_DISPLAY_MM..=MAX_DISPLAY_MM).contains(&display_mm) {
        return Err(CadError::Invalid(
            "Display/export chord tolerance must be finite and between 0.000001 and 1 mm".into(),
        ));
    }
    Ok(())
}
