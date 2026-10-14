//! Disposable, versioned viewport tessellation. Modeling accuracy is separate.
use crate::{CadError, Document, Id, Result, protocol::*};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
pub const VERSION: u32 = 2;
pub const MAX_TARGETS: usize = 256;
pub const MAX_TRIANGLES: usize = 500_000;
pub const MAX_VERTICES: usize = 1_500_000;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Quality {
    pub automatic: bool,
    pub chord_pixels: f64,
    pub minimum_mm: f64,
    pub maximum_mm: f64,
}
impl Default for Quality {
    fn default() -> Self {
        Self {
            automatic: true,
            chord_pixels: 0.5,
            minimum_mm: 0.0001,
            maximum_mm: 0.1,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Target {
    pub key: String,
    pub definition: Id,
    pub configuration: Option<String>,
    pub outputs: Vec<Id>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Request {
    pub version: u32,
    pub identity: RequestIdentity,
    pub generation: u64,
    pub document: Document,
    pub project_root: String,
    pub modeling: crate::tolerance::Modeling,
    pub display_tolerance_mm: f64,
    pub targets: Vec<Target>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Output {
    pub version: u32,
    pub identity: RequestIdentity,
    pub generation: u64,
    pub tolerance_mm: f64,
    pub meshes: BTreeMap<String, MeshData>,
    pub diagnostics: Vec<Diagnostic>,
}
impl Request {
    pub fn validate(&self, backend: &str) -> Result<()> {
        self.modeling.validate()?;
        crate::tolerance::validate_display(self.display_tolerance_mm)?;
        if self.version != VERSION
            || self.identity.protocol != PROTOCOL_VERSION
            || self.identity.backend != backend
            || self.identity.source != self.document.fingerprint()
            || self.targets.len() > MAX_TARGETS
            || self.targets.iter().any(|t| t.outputs.len() > MAX_TARGETS)
            || self.modeling != self.document.settings.modeling
        {
            return Err(CadError::Invalid(
                "Display tessellation source, version, tolerance or work bounds mismatch".into(),
            ));
        }
        Ok(())
    }
}
