//! Versioned document-owned camera bookmarks. No geometry or renderer identities.
use crate::{CadError, Document, Id, Result, presentation::SemanticElement};
use nalgebra::Vector3;
use serde::{Deserialize, Serialize};
use serde_json::json;
pub const CONTRACT_VERSION: u32 = 1;
pub const MAX_VIEWS: usize = 256;
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Projection {
    Perspective { vertical_fov_deg: f64 },
    Orthographic { vertical_span_mm: f64 },
}
impl Default for Projection {
    fn default() -> Self {
        Self::Perspective {
            vertical_fov_deg: 45.,
        }
    }
}
pub fn is_default_projection(value: &Projection) -> bool {
    *value == Projection::default()
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Pose {
    pub target_mm: [f64; 3],
    /// Matches unlimited yaw/pitch orbit including pole crossings.
    pub orbit_rad: [f64; 2],
    pub roll_rad: f64,
    pub distance_mm: f64,
    pub projection: Projection,
}
impl Default for Pose {
    fn default() -> Self {
        Self {
            target_mm: [0.; 3],
            orbit_rad: [0.65, 0.4],
            roll_rad: 0.,
            distance_mm: 350.,
            projection: Projection::default(),
        }
    }
}
pub struct Frame {
    pub eye_mm: [f64; 3],
    pub forward: [f64; 3],
    pub right: [f64; 3],
    pub up: [f64; 3],
}
impl Pose {
    pub fn validate(&self) -> Result<()> {
        if self
            .target_mm
            .iter()
            .any(|v| !v.is_finite() || v.abs() > 1e9)
            || self
                .orbit_rad
                .iter()
                .chain(std::iter::once(&self.roll_rad))
                .any(|v| !v.is_finite() || v.abs() > 1e6)
            || !self.distance_mm.is_finite()
            || !(0.5..=1e6).contains(&self.distance_mm)
            || match self.projection {
                Projection::Perspective { vertical_fov_deg } => {
                    !vertical_fov_deg.is_finite() || !(5.0..=120.).contains(&vertical_fov_deg)
                }
                Projection::Orthographic { vertical_span_mm } => {
                    !vertical_span_mm.is_finite() || !(0.1..=1e8).contains(&vertical_span_mm)
                }
            }
        {
            return Err(CadError::Invalid("Camera pose requires finite target coordinates within ±1e9 mm, distance 0.5–1000000 mm, bounded angles, and perspective FOV 5–120° or orthographic span 0.1–1e8 mm".into()));
        }
        Ok(())
    }
    pub fn frame(&self) -> Frame {
        let [yaw, pitch] = self.orbit_rad;
        let offset = Vector3::new(
            yaw.sin() * pitch.cos(),
            -yaw.cos() * pitch.cos(),
            pitch.sin(),
        );
        let forward = -offset;
        let right = Vector3::new(yaw.cos(), yaw.sin(), 0.);
        let up = right.cross(&forward);
        let (sin, cos) = self.roll_rad.sin_cos();
        Frame {
            eye_mm: (Vector3::from(self.target_mm) + offset * self.distance_mm).into(),
            forward: forward.into(),
            right: (right * cos + up * sin).into(),
            up: (up * cos - right * sin).into(),
        }
    }
    pub fn vertical_span_mm(&self) -> f64 {
        match self.projection {
            Projection::Perspective { vertical_fov_deg } => {
                2. * self.distance_mm * (vertical_fov_deg.to_radians() * 0.5).tan()
            }
            Projection::Orthographic { vertical_span_mm } => vertical_span_mm,
        }
    }
    pub fn switch_projection(&mut self, orthographic: bool) {
        let span = self.vertical_span_mm();
        if orthographic {
            if !matches!(self.projection, Projection::Orthographic { .. }) {
                self.projection = Projection::Orthographic {
                    vertical_span_mm: span.clamp(0.1, 1e8),
                };
            }
        } else if !matches!(self.projection, Projection::Perspective { .. }) {
            self.projection = Projection::default();
            self.distance_mm = (span / (2. * (std::f64::consts::PI / 8.).tan())).clamp(0.5, 1e6);
        }
    }
    pub fn normalize(&mut self) {
        self.orbit_rad = self.orbit_rad.map(|v| v.rem_euclid(std::f64::consts::TAU));
        self.roll_rad = self.roll_rad.rem_euclid(std::f64::consts::TAU);
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Definition {
    pub version: u32,
    pub id: Id,
    pub name: String,
    pub pose: Pose,
}
impl Definition {
    pub fn new(name: String, pose: Pose) -> Self {
        Self {
            version: CONTRACT_VERSION,
            id: crate::id(),
            name,
            pose,
        }
    }
    pub fn validate(&self) -> Result<()> {
        if self.version != CONTRACT_VERSION
            || !valid_id(&self.id)
            || self.name.trim().is_empty()
            || self.name.len() > 128
            || self.name.chars().any(char::is_control)
        {
            return Err(CadError::Invalid("Named camera views require version 1, a portable stable identity and a nonempty name up to 128 bytes".into()));
        }
        self.pose.validate()
    }
}
fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 256
        && !matches!(id, "." | "..")
        && !id
            .chars()
            .any(|c| c.is_control() || matches!(c, '/' | '\\' | ':'))
}
pub fn validate(doc: &Document) -> Result<()> {
    if doc.camera_views.len() > MAX_VIEWS {
        return Err(CadError::Invalid(format!(
            "A document supports at most {MAX_VIEWS} named camera views"
        )));
    }
    let mut names = std::collections::BTreeSet::new();
    for (key, view) in &doc.camera_views {
        view.validate()?;
        if key != &view.id || !names.insert(view.name.trim().to_lowercase()) {
            return Err(CadError::Invalid("Named camera view keys must match their identities and names must be unique ignoring case".into()));
        }
    }
    Ok(())
}
pub fn put(doc: &mut Document, view: Definition) -> Result<()> {
    view.validate()?;
    if !doc.camera_views.contains_key(&view.id) && doc.camera_views.len() >= MAX_VIEWS {
        return Err(CadError::Invalid("Named camera view limit reached".into()));
    }
    if doc
        .camera_views
        .values()
        .any(|old| old.id != view.id && old.name.trim().eq_ignore_ascii_case(view.name.trim()))
    {
        return Err(CadError::Invalid(
            "Choose a unique named camera view name".into(),
        ));
    }
    doc.camera_views.insert(view.id.clone(), view);
    Ok(())
}
pub fn delete(doc: &mut Document, id: &str) -> Result<()> {
    doc.camera_views
        .remove(id)
        .ok_or_else(|| CadError::Invalid("The named camera view no longer exists".into()))?;
    Ok(())
}
pub fn semantic(doc: &Document) -> Vec<SemanticElement> {
    let mut out = vec![SemanticElement {
        id: "camera-views".into(),
        role: "camera-view-library".into(),
        label: "Named views".into(),
        value: Some(doc.camera_views.len().to_string()),
        state: "ready".into(),
        commands: vec!["camera-views".into(), "camera-view.save".into()],
        relationships: vec![doc.id.clone()],
        data: json!({"contract":"tcad.camera-views","version":CONTRACT_VERSION,"maximum":MAX_VIEWS,"geometry_mutation":false,"recall_changes_configuration":false,"recall_changes_display_state":false,"length_unit":"mm","angle_unit":"rad"}),
    }];
    for view in doc.camera_views.values() {
        out.push(SemanticElement{id:format!("camera-view:{}",view.id),role:"camera-view".into(),label:view.name.clone(),value:None,state:"ready".into(),commands:vec!["camera-view.recall".into(),"camera-view.edit".into(),"camera-view.delete".into()],relationships:vec!["camera-views".into()],data:json!({"view":view.id,"document":doc.id,"source":view,"projection":view.pose.projection,"recall_requires_evaluation":false})});
    }
    out
}
