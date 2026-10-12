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
    /// Constant wall proved from one annular or rectangular material region.
    /// Absent when its analytic boundaries do not establish a uniform wall.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wall_mm: Option<f64>,
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
        let number = |value: f64| {
            let value = value / scale;
            let text = format!("{:.*}", precision.min(12), value);
            if value != 0. && text.parse::<f64>().is_ok_and(|rounded| rounded == 0.) {
                format!("{:.*e}", precision.min(12), value)
            } else {
                text
            }
        };
        let wall = self.wall_mm.filter(|wall| {
            wall.is_finite()
                && *wall > 0.
                && self.extents_mm.iter().all(|extent| 2. * wall < *extent)
        });
        Some(
            match self
                .diameter_mm
                .filter(|value| value.is_finite() && *value > 0.)
            {
                Some(diameter) => match wall.filter(|wall| 2. * wall < diameter) {
                    Some(wall) => format!("Ø{} × {} {unit} wall", number(diameter), number(wall)),
                    None => format!("Ø{} {unit}", number(diameter)),
                },
                None if wall.is_some() => format!(
                    "{} × {} × {} {unit} wall",
                    number(self.extents_mm[0]),
                    number(self.extents_mm[1]),
                    number(wall?)
                ),
                None => format!(
                    "{} × {} {unit} extents",
                    number(self.extents_mm[0]),
                    number(self.extents_mm[1])
                ),
            },
        )
    }
}

/// Error-free difference of two finite f64 coordinates. Comparing both parts
/// prevents rounded equal differences from inventing a uniform wall. This is
/// a dyadic-coordinate proof, not a geometric proximity tolerance.
fn difference(a: f64, b: f64) -> Option<(f64, f64)> {
    let high = a - b;
    let virtual_b = a - high;
    let virtual_a = high + virtual_b;
    let low = (a - virtual_a) + (virtual_b - b);
    [high, virtual_b, virtual_a, low]
        .into_iter()
        .all(f64::is_finite)
        .then_some((high, low))
}

fn rectangle(
    output: &EvaluatedSketch,
    members: &[crate::Id],
    suppressed: &BTreeSet<crate::Id>,
) -> Option<[[f64; 2]; 2]> {
    if members.len() != 4 || members.iter().collect::<BTreeSet<_>>().len() != 4 {
        return None;
    }
    let source = output.solved.as_ref()?;
    let mut edges = Vec::with_capacity(4);
    let mut bounds = [[f64::INFINITY; 2], [f64::NEG_INFINITY; 2]];
    for id in members {
        let member = source.members.get(id)?;
        if member.construction || suppressed.contains(id) {
            return None;
        }
        let SketchCurve::Line { start, end } =
            crate::sketch_edit::effective_curve(source, id).ok()?
        else {
            return None;
        };
        // Wall classification does not merge nearby independent vertices.
        let (a, b) = (*output.points.get(&start)?, *output.points.get(&end)?);
        if a.iter().chain(&b).any(|value| !value.is_finite()) || (a[0] == b[0]) == (a[1] == b[1]) {
            return None;
        }
        for point in [a, b] {
            for axis in 0..2 {
                bounds[0][axis] = bounds[0][axis].min(point[axis]);
                bounds[1][axis] = bounds[1][axis].max(point[axis]);
            }
        }
        edges.push([a, b]);
    }
    if (0..2).any(|axis| bounds[0][axis] >= bounds[1][axis]) {
        return None;
    }
    let mut sides = 0u8;
    for [a, b] in edges {
        let (axis, fixed) = if a[0] == b[0] { (1, 0) } else { (0, 1) };
        if a[axis].min(b[axis]) != bounds[0][axis] || a[axis].max(b[axis]) != bounds[1][axis] {
            return None;
        }
        let side = if a[fixed] == bounds[0][fixed] {
            fixed * 2
        } else if a[fixed] == bounds[1][fixed] {
            fixed * 2 + 1
        } else {
            return None;
        };
        let bit = 1 << side;
        if sides & bit != 0 {
            return None;
        }
        sides |= bit;
    }
    (sides == 15).then_some(bounds)
}

fn round(
    output: &EvaluatedSketch,
    id: &str,
    suppressed: &BTreeSet<crate::Id>,
) -> Option<([f64; 2], f64)> {
    let source = output.solved.as_ref()?;
    if suppressed.contains(id)
        || source
            .members
            .get(id)
            .is_some_and(|member| member.construction)
    {
        return None;
    }
    let is_circle = if source.members.contains_key(id) {
        matches!(
            crate::sketch_edit::effective_curve(source, id).ok()?,
            SketchCurve::Circle { .. }
        )
    } else {
        source.circles.contains_key(id)
    };
    if !is_circle {
        return None;
    }
    let circle = output.circles.get(id)?;
    let center = *output.points.get(&circle.center)?;
    (center.iter().all(|value| value.is_finite())
        && circle.radius_mm.is_finite()
        && circle.radius_mm > 0.)
        .then_some((center, circle.radius_mm))
}

fn physical(
    output: &EvaluatedSketch,
    suppressed: &BTreeSet<crate::Id>,
) -> (Option<f64>, Option<f64>) {
    let Some(region) = output
        .material_regions
        .first()
        .filter(|_| output.material_regions.len() == 1)
    else {
        return (None, None);
    };
    if let [outer] = region.outer_members.as_slice()
        && let Some((center, radius)) = round(output, outer, suppressed)
    {
        let diameter = (2. * radius).is_finite().then_some(2. * radius);
        let wall = if let [hole] = region.hole_members.as_slice()
            && let [inner] = hole.as_slice()
            && let Some((inner_center, inner_radius)) = round(output, inner, suppressed)
            && center == inner_center
            && inner_radius < radius
        {
            difference(radius, inner_radius)
                .map(|(high, _)| high)
                .filter(|wall| *wall > 0.)
        } else {
            None
        };
        return (diameter, wall);
    }
    let wall = (|| {
        let [hole] = region.hole_members.as_slice() else {
            return None;
        };
        let outer = rectangle(output, &region.outer_members, suppressed)?;
        let inner = rectangle(output, hole, suppressed)?;
        let mut setbacks = Vec::with_capacity(4);
        for axis in 0..2 {
            if !(outer[0][axis] < inner[0][axis] && inner[1][axis] < outer[1][axis]) {
                return None;
            }
            setbacks.push(difference(inner[0][axis], outer[0][axis])?);
            setbacks.push(difference(outer[1][axis], inner[1][axis])?);
        }
        let wall = setbacks[0];
        (wall.0 > 0. && setbacks.iter().all(|setback| *setback == wall)).then_some(wall.0)
    })();
    (None, wall)
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
        || (dot(u, u) - 1.).abs() > 32. * f64::EPSILON
        || (dot(v, v) - 1.).abs() > 32. * f64::EPSILON
        || dot(u, v).abs() > 32. * f64::EPSILON
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
    for id in source.circles.keys().filter(|id| {
        material(id)
            && source.members.get(*id).is_none_or(|member| {
                !member.construction
                    && matches!(
                        crate::sketch_edit::effective_curve(source, id).ok(),
                        Some(SketchCurve::Circle { .. })
                    )
            })
    }) {
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
    let (physical_diameter, wall_mm) = physical(output, &suppressed);
    let diameter_mm = physical_diameter.or_else(|| {
        if line_count == 0
            && source.segments.is_empty()
            && output.profile_order.is_empty()
            && circle_ids.len() == 1
        {
            Some(2. * output.circles.get(circle_ids.first()?)?.radius_mm)
        } else {
            None
        }
    });
    Some(SectionSize {
        version: VERSION,
        extents_mm,
        diameter_mm,
        wall_mm,
    })
}
