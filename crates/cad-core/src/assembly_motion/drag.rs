//! Version 1 mechanical-space manipulation math, independent of pixels/renderers.
use crate::{CadError, Placement, Result};
use nalgebra::{Rotation3, Unit, Vector3, Vector4};
use serde::{Deserialize, Serialize};

pub const CONTRACT_VERSION: u32 = 1;
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Handle {
    ViewPlane,
    TranslationAxis { axis: usize },
    RotationAxis { axis: usize },
    ViewRotation,
}
impl Handle {
    pub fn axis(self, view_normal: [f64; 3]) -> Result<[f64; 3]> {
        match self {
            Self::ViewPlane | Self::ViewRotation => unit(view_normal).map(Into::into),
            Self::TranslationAxis { axis } | Self::RotationAxis { axis } if axis < 3 => Ok(
                std::array::from_fn(|index| if index == axis { 1. } else { 0. }),
            ),
            _ => Err(CadError::Invalid(
                "Unknown assembly manipulation axis".into(),
            )),
        }
    }
    pub fn rotation(self) -> bool {
        matches!(self, Self::RotationAxis { .. } | Self::ViewRotation)
    }
    pub fn key(self) -> &'static str {
        match self {
            Self::ViewPlane => "view-plane",
            Self::TranslationAxis { axis: 0 } => "move-x",
            Self::TranslationAxis { axis: 1 } => "move-y",
            Self::TranslationAxis { axis: 2 } => "move-z",
            Self::RotationAxis { axis: 0 } => "rotate-x",
            Self::RotationAxis { axis: 1 } => "rotate-y",
            Self::RotationAxis { axis: 2 } => "rotate-z",
            Self::ViewRotation => "view-rotation",
            _ => "invalid",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::ViewPlane => "Move in view plane",
            Self::TranslationAxis { axis: 0 } => "Move along assembly X",
            Self::TranslationAxis { axis: 1 } => "Move along assembly Y",
            Self::TranslationAxis { axis: 2 } => "Move along assembly Z",
            Self::RotationAxis { axis: 0 } => "Rotate around assembly X",
            Self::RotationAxis { axis: 1 } => "Rotate around assembly Y",
            Self::RotationAxis { axis: 2 } => "Rotate around assembly Z",
            Self::ViewRotation => "Rotate around view normal",
            _ => "Invalid handle",
        }
    }
}
fn unit(value: [f64; 3]) -> Result<Vector3<f64>> {
    if value.iter().any(|v| !v.is_finite()) {
        return Err(CadError::Invalid(
            "Manipulation direction must be finite".into(),
        ));
    }
    Vector3::from(value)
        .try_normalize(1e-12)
        .ok_or_else(|| CadError::Invalid("Manipulation direction is degenerate".into()))
}
pub fn anchor(placement: &Placement, local: [f64; 3]) -> [f64; 3] {
    let point = placement.matrix() * Vector4::new(local[0], local[1], local[2], 1.);
    [point.x, point.y, point.z]
}
pub fn plane_hit(
    ray: ([f64; 3], [f64; 3]),
    origin: [f64; 3],
    normal: [f64; 3],
) -> Result<[f64; 3]> {
    let normal = unit(normal)?;
    let direction = unit(ray.1)?;
    let denominator = normal.dot(&direction);
    if denominator.abs() < 1e-8 {
        return Err(CadError::Invalid(
            "The view is parallel to this manipulation plane; rotate the view".into(),
        ));
    }
    let distance = (Vector3::from(origin) - Vector3::from(ray.0)).dot(&normal) / denominator;
    let hit = Vector3::from(ray.0) + direction * distance;
    if !distance.is_finite() || distance <= 0. || hit.iter().any(|v| !v.is_finite()) {
        return Err(CadError::Invalid(
            "The manipulation plane lies behind the view".into(),
        ));
    }
    Ok(hit.into())
}
/// Closest approach between the pointer ray and the mechanical axis. Near
/// parallel projections fail explicitly instead of amplifying pointer noise.
pub fn axis_parameter(ray: ([f64; 3], [f64; 3]), origin: [f64; 3], axis: [f64; 3]) -> Result<f64> {
    let axis = unit(axis)?;
    let direction = unit(ray.1)?;
    let difference = Vector3::from(ray.0) - Vector3::from(origin);
    let alignment = direction.dot(&axis);
    let denominator = 1. - alignment * alignment;
    if denominator < 1e-6 {
        return Err(CadError::Invalid(
            "This axis aligns with the view; rotate the view before dragging it".into(),
        ));
    }
    let parameter = (difference.dot(&axis) - alignment * difference.dot(&direction)) / denominator;
    if !parameter.is_finite() {
        return Err(CadError::Invalid("The axis drag is non-finite".into()));
    }
    Ok(parameter)
}
pub fn radial(ray: ([f64; 3], [f64; 3]), origin: [f64; 3], axis: [f64; 3]) -> Result<[f64; 3]> {
    unit((Vector3::from(plane_hit(ray, origin, axis)?) - Vector3::from(origin)).into())
        .map(Into::into)
}
pub fn signed_angle(from: [f64; 3], to: [f64; 3], axis: [f64; 3]) -> Result<f64> {
    let a = unit(from)?;
    let b = unit(to)?;
    let axis = unit(axis)?;
    Ok(axis.dot(&a.cross(&b)).atan2(a.dot(&b)))
}
pub fn frame(axis: [f64; 3]) -> Result<[[f64; 3]; 2]> {
    let axis = unit(axis)?;
    let seed = if axis.x.abs() < 0.8 {
        Vector3::x()
    } else {
        Vector3::y()
    };
    let u = (seed - axis * seed.dot(&axis)).normalize();
    Ok([u.into(), axis.cross(&u).into()])
}
pub fn translation_offsets(
    baseline: &Placement,
    actual: &Placement,
    local: [f64; 3],
    delta: [f64; 3],
) -> [f64; 3] {
    (Vector3::from(anchor(actual, local)) - Vector3::from(anchor(baseline, local))
        + Vector3::from(delta))
    .into()
}
pub fn rotation_offsets(
    baseline: &Placement,
    actual: &Placement,
    axis: [f64; 3],
    angle: f64,
) -> Result<[f64; 3]> {
    let axis = unit(axis)?;
    if !angle.is_finite() {
        return Err(CadError::Invalid(
            "The requested rotation is non-finite".into(),
        ));
    }
    let delta = Rotation3::from_axis_angle(&Unit::new_unchecked(axis), angle);
    let baseline_matrix = baseline.matrix();
    let actual_matrix = actual.matrix();
    let rotation = delta.matrix()
        * actual_matrix.fixed_view::<3, 3>(0, 0)
        * baseline_matrix.fixed_view::<3, 3>(0, 0).transpose();
    let (x, y, z) = Rotation3::from_matrix_unchecked(rotation).euler_angles();
    Ok([x, y, z].map(f64::to_degrees))
}
