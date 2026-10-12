//! Display dimensions from validated analytic sketch carriers. Polylines and
//! body tessellation never establish these values or manufacturing identity.
use crate::{SketchCurve, protocol::EvaluatedSketch};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const VERSION: u32 = 1;
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SectionSize {
    pub version: u32,
    pub extents_mm: [f64; 2],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diameter_mm: Option<f64>,
}
impl SectionSize {
    pub fn display(&self, units: crate::drawing::Units, precision: usize) -> Option<String> {
        if self.version != VERSION
            || self
                .extents_mm
                .iter()
                .any(|value| !value.is_finite() || *value <= 0.)
        {
            return None;
        }
        let (scale, unit) = if units == crate::drawing::Units::Inches {
            (25.4, "in")
        } else {
            (1., "mm")
        };
        let number = |value: f64| format!("{:.*}", precision.min(12), value / scale);
        Some(
            match self
                .diameter_mm
                .filter(|value| value.is_finite() && *value > 0.)
            {
                Some(diameter) => format!("Ø{} {unit}", number(diameter)),
                None => format!(
                    "{} × {} {unit} extents",
                    number(self.extents_mm[0]),
                    number(self.extents_mm[1])
                ),
            },
        )
    }
}

pub fn from_sketch(output: &EvaluatedSketch) -> Option<SectionSize> {
    let source = output.solved.as_ref()?;
    if output.material_regions.len() > 1
        || source
            .members
            .len()
            .saturating_add(source.circles.len())
            .saturating_add(source.profile_order.len())
            .saturating_add(source.segments.len())
            > 65_536
    {
        return None;
    }
    if output.material_regions.iter().any(|region| {
        region
            .hole_members
            .iter()
            .fold(region.outer_members.len(), |count, members| {
                count.saturating_add(members.len())
            })
            > 65_536
    }) {
        return None;
    }
    let aliases =
        crate::sketch_topology::point_aliases(source, crate::sketch_topology::DEFAULT_TOLERANCE_MM)
            .ok()?;
    let point = |id: &str| {
        output
            .points
            .get(aliases.get(id).map(String::as_str).unwrap_or(id))
            .copied()
    };
    // Section coordinates are millimetres only in an orthonormal plane frame.
    let dot = |a: [f64; 3], b: [f64; 3]| a.iter().zip(b).map(|(a, b)| a * b).sum::<f64>();
    let (u, v) = (output.plane.u, output.plane.v);
    if u.iter().chain(&v).any(|value| !value.is_finite())
        || (dot(u, u) - 1.).abs() > 1e-9
        || (dot(v, v) - 1.).abs() > 1e-9
        || dot(u, v).abs() > 1e-9
    {
        return None;
    }
    let selected = output.material_regions.first().map(|region| {
        region
            .outer_members
            .iter()
            .chain(region.hole_members.iter().flatten())
            .cloned()
            .collect::<BTreeSet<_>>()
    });
    let suppressed = crate::sketch_transform::suppressed_members(source);
    let material =
        |id: &str| !suppressed.contains(id) && selected.as_ref().is_none_or(|set| set.contains(id));
    let mut bounds = [[f64::INFINITY; 2], [f64::NEG_INFINITY; 2]];
    let mut points = Vec::new();
    if source.members.is_empty() {
        if !output.profile_order.is_empty() {
            for id in &output.profile_order {
                points.push(point(id)?);
            }
        } else if !source.segments.is_empty() {
            for segment in &source.segments {
                for id in segment {
                    points.push(point(id)?);
                }
            }
        }
    }
    let mut circle_ids = BTreeSet::new();
    let mut line_count = 0usize;
    for (id, member) in &source.members {
        if member.construction || !material(id) {
            continue;
        }
        match crate::sketch_edit::effective_curve(source, id).ok()? {
            SketchCurve::Line { start, end } => {
                points.extend([point(&start)?, point(&end)?]);
                line_count += 1;
            }
            SketchCurve::Circle { .. } => {
                circle_ids.insert(id.clone());
            }
            _ => return None,
        }
    }
    for id in source.circles.keys().filter(|id| material(id)) {
        circle_ids.insert(id.clone());
    }
    for id in &circle_ids {
        let circle = output.circles.get(id)?;
        let center = point(&circle.center)?;
        let radius = circle.radius_mm;
        if !radius.is_finite() || radius <= 0. {
            return None;
        }
        points.extend([
            [center[0] - radius, center[1] - radius],
            [center[0] + radius, center[1] + radius],
        ]);
    }
    for point in &points {
        for (axis, value) in point.iter().enumerate() {
            if !value.is_finite() {
                return None;
            }
            bounds[0][axis] = bounds[0][axis].min(*value);
            bounds[1][axis] = bounds[1][axis].max(*value);
        }
    }
    let extents_mm = std::array::from_fn(|axis| bounds[1][axis] - bounds[0][axis]);
    if extents_mm
        .iter()
        .any(|value| !value.is_finite() || *value <= 0.)
    {
        return None;
    }
    let diameter_mm = if line_count == 0
        && source.segments.is_empty()
        && output.profile_order.is_empty()
        && circle_ids.len() == 1
    {
        Some(2. * output.circles.get(circle_ids.first()?)?.radius_mm)
    } else {
        None
    };
    Some(SectionSize {
        version: VERSION,
        extents_mm,
        diameter_mm,
    })
}
