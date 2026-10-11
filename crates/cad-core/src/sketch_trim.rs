//! Exact source edits for Trim, Extend, and Split. Picking and intersection tolerances
//! never replace the authoritative line, circle, arc, Bézier, or rational spline.
use crate::*;
use nalgebra::Vector3;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::f64::consts::TAU;

const PARAMETER_TOLERANCE: f64 = 1e-10;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Trim,
    Extend,
    Split,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Request {
    /// Stable namespace for generated controls, pieces, and relations throughout
    /// this edit's live previews and eventual source commit.
    pub edit_id: Id,
    pub kind: Kind,
    pub member: Id,
    /// Normalized position on the original member. Extend chooses the nearest end.
    pub parameter: f64,
    /// Extend by a length along the supporting line or circle; None finds the next
    /// intersection with the other bounded sketch members.
    pub extension: Option<f64>,
    pub include_construction: bool,
    /// Explicit intersection/coincidence tolerance in millimetres.
    pub tolerance: f64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EditReport {
    pub members: Vec<Id>,
    pub removed_constraints: Vec<Id>,
    pub deleted_member: bool,
}

#[derive(Clone)]
enum Curve {
    Line([f64; 2], [f64; 2]),
    Circular {
        center: [f64; 2],
        radius: f64,
        start: f64,
        sweep: f64,
        closed: bool,
    },
    Spline {
        curve: sketch_edit::Curve2,
        bezier: bool,
        closed: bool,
    },
}
fn invalid(message: impl Into<String>) -> CadError {
    CadError::Invalid(message.into())
}
fn sub(a: [f64; 2], b: [f64; 2]) -> [f64; 2] {
    [a[0] - b[0], a[1] - b[1]]
}
fn dot(a: [f64; 2], b: [f64; 2]) -> f64 {
    a[0] * b[0] + a[1] * b[1]
}
fn cross(a: [f64; 2], b: [f64; 2]) -> f64 {
    a[0] * b[1] - a[1] * b[0]
}
fn distance(a: [f64; 2], b: [f64; 2]) -> f64 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}
fn curve(doc: &Document, owner: &str, sketch: &Sketch, key: &str) -> Result<Curve> {
    let get = |id: &str| {
        sketch
            .points
            .get(id)
            .copied()
            .ok_or_else(|| invalid("Missing curve control"))
    };
    let length = |e: &Expr| expression::value(doc, owner, e, Dimension::Length);
    match sketch_edit::effective_curve(sketch, key)? {
        SketchCurve::Line { start, end } => {
            let (a, b) = (get(&start)?, get(&end)?);
            if distance(a, b) <= 1e-9 {
                return Err(invalid("Cannot edit a zero-length line"));
            }
            Ok(Curve::Line(a, b))
        }
        SketchCurve::Arc {
            start,
            through,
            end,
        } => {
            let (center, radius, start, sweep) =
                sketch_geometry::arc_frame(get(&start)?, get(&through)?, get(&end)?)?;
            Ok(Curve::Circular {
                center,
                radius,
                start,
                sweep,
                closed: false,
            })
        }
        SketchCurve::Circle { center, radius } => {
            let radius = length(&radius)?;
            if radius <= 1e-9 {
                return Err(invalid("Circle radius must be positive"));
            }
            Ok(Curve::Circular {
                center: get(&center)?,
                radius,
                start: 0.,
                sweep: TAU,
                closed: true,
            })
        }
        SketchCurve::Bezier { points } => {
            if !(2..=16).contains(&points.len()) {
                return Err(invalid("A Bézier spline needs 2–16 controls"));
            }
            let degree = points.len() - 1;
            let control_points = points.iter().map(|p| get(p)).collect::<Result<Vec<_>>>()?;
            Ok(Curve::Spline {
                closed: points.first() == points.last(),
                bezier: true,
                curve: sketch_edit::Curve2 {
                    weights: vec![1.; points.len()],
                    knots: std::iter::repeat_n(0., degree + 1)
                        .chain(std::iter::repeat_n(1., degree + 1))
                        .collect(),
                    control_points,
                    degree,
                },
            })
        }
        SketchCurve::Nurbs {
            points,
            weights,
            knots,
            degree,
        } => {
            let spline = sketch_edit::Curve2 {
                control_points: points.iter().map(|p| get(p)).collect::<Result<_>>()?,
                weights,
                knots,
                degree,
            };
            sketch_edit::validate_nurbs(&spline)?;
            Ok(Curve::Spline {
                curve: spline,
                bezier: false,
                closed: points.first() == points.last(),
            })
        }
        SketchCurve::Ellipse {
            center,
            major,
            minor,
            angle,
        } => {
            let center = get(&center)?;
            let (major, minor) = (length(&major)?, length(&minor)?);
            let angle = expression::value(doc, owner, &angle, Dimension::Angle)?;
            if major <= 1e-9 || minor <= 1e-9 {
                return Err(invalid("Ellipse radii must be positive"));
            }
            let w = std::f64::consts::FRAC_1_SQRT_2;
            let local = [
                [1., 0.],
                [1., 1.],
                [0., 1.],
                [-1., 1.],
                [-1., 0.],
                [-1., -1.],
                [0., -1.],
                [1., -1.],
                [1., 0.],
            ];
            let control_points = local
                .into_iter()
                .map(|p| {
                    let (x, y) = (major * p[0], minor * p[1]);
                    [
                        center[0] + x * angle.cos() - y * angle.sin(),
                        center[1] + x * angle.sin() + y * angle.cos(),
                    ]
                })
                .collect();
            Ok(Curve::Spline {
                bezier: false,
                closed: true,
                curve: sketch_edit::Curve2 {
                    control_points,
                    weights: vec![1., w, 1., w, 1., w, 1., w, 1.],
                    knots: vec![0., 0., 0., 0.25, 0.25, 0.5, 0.5, 0.75, 0.75, 1., 1., 1.],
                    degree: 2,
                },
            })
        }
    }
}
impl Curve {
    fn closed(&self) -> bool {
        match self {
            Self::Line(..) => false,
            Self::Circular { closed, .. } | Self::Spline { closed, .. } => *closed,
        }
    }
    fn point(&self, t: f64) -> Result<[f64; 2]> {
        match self {
            Self::Line(a, b) => Ok([a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]),
            Self::Circular {
                center,
                radius,
                start,
                sweep,
                ..
            } => {
                let angle = start + sweep * t;
                Ok([
                    center[0] + radius * angle.cos(),
                    center[1] + radius * angle.sin(),
                ])
            }
            Self::Spline { curve, .. } => sketch_edit::nurbs_point(curve, t),
        }
    }
    fn parameter(&self, point: [f64; 2], unbounded: bool, tolerance: f64) -> Option<f64> {
        match self {
            Self::Line(a, b) => {
                let delta = sub(*b, *a);
                if cross(sub(point, *a), delta).abs() / dot(delta, delta).sqrt() > tolerance {
                    return None;
                }
                let t = dot(sub(point, *a), delta) / dot(delta, delta);
                (unbounded || (-PARAMETER_TOLERANCE..=1. + PARAMETER_TOLERANCE).contains(&t))
                    .then_some(t)
            }
            Self::Circular {
                center,
                radius,
                start,
                sweep,
                closed,
            } => {
                if (distance(point, *center) - radius).abs() > tolerance {
                    return None;
                }
                let angle = (point[1] - center[1]).atan2(point[0] - center[0]);
                let progress = if *sweep > 0. {
                    (angle - start).rem_euclid(TAU)
                } else {
                    (start - angle).rem_euclid(TAU)
                };
                let t = progress / sweep.abs();
                (*closed || unbounded || t <= 1. + PARAMETER_TOLERANCE).then_some(t)
            }
            _ => None,
        }
    }
    fn bounds(&self) -> ([f64; 2], [f64; 2]) {
        match self {
            Self::Circular { center, radius, .. } => (
                [center[0] - radius, center[1] - radius],
                [center[0] + radius, center[1] + radius],
            ),
            _ => {
                let points: Vec<[f64; 2]> = match self {
                    Self::Line(a, b) => vec![*a, *b],
                    Self::Spline { curve, .. } => curve.control_points.clone(),
                    _ => unreachable!(),
                };
                let mut low = [f64::INFINITY; 2];
                let mut high = [f64::NEG_INFINITY; 2];
                for p in points {
                    for i in 0..2 {
                        low[i] = low[i].min(p[i]);
                        high[i] = high[i].max(p[i]);
                    }
                }
                (low, high)
            }
        }
    }
}

/// Availability is source-based. Editing a derived member or a source with live
/// dependent operations requires explicit detachment/removal, never silent loss.
pub fn editable(sketch: &Sketch, member: &str, kind: Kind) -> Result<()> {
    if !sketch.profile_order.is_empty() || !sketch.segments.is_empty() || !sketch.circles.is_empty()
    {
        return Err(CadError::Unsupported("Open this legacy sketch in the sketch editor to migrate its geometry to named members before trimming, extending, or splitting".into()));
    }
    let member_curve = &sketch
        .members
        .get(member)
        .ok_or_else(|| invalid("Select a sketch member"))?
        .curve;
    if sketch_edit::driven_member(sketch, member)
        || sketch.splines.contains_key(member)
        || crate::sketch_conic::operation_for(sketch, member).is_some()
    {
        return Err(invalid(
            "Detach the member's source relation before trimming, extending, or splitting",
        ));
    }
    if sketch
        .fillets
        .values()
        .any(|f| f.lines.iter().any(|m| m == member))
        || sketch
            .chamfers
            .values()
            .any(|c| c.lines.iter().any(|m| m == member))
        || sketch
            .offsets
            .values()
            .any(|o| o.members.iter().any(|m| m.source == member))
        || sketch
            .transforms
            .values()
            .any(|p| sketch_transform::sources(p).iter().any(|m| m == member))
    {
        return Err(invalid(
            "Remove dependent corner, offset, or pattern operations before changing this member's topology",
        ));
    }
    if kind == Kind::Extend
        && !matches!(
            member_curve,
            SketchCurve::Line { .. } | SketchCurve::Arc { .. }
        )
    {
        return Err(CadError::Unsupported(
            "Extend supports open lines and circular arcs; a closed curve has no end to extend"
                .into(),
        ));
    }
    Ok(())
}

/// Project a viewport pick onto the exact member. Display tessellation is not used
/// to choose the resulting source geometry.
pub fn parameter_at(
    doc: &Document,
    owner: &str,
    sketch: &Sketch,
    member: &str,
    pick: [f64; 2],
) -> Result<f64> {
    let curve = curve(doc, owner, sketch, member)?;
    match &curve {
        Curve::Line(a, b) => {
            let direction = sub(*b, *a);
            Ok((dot(sub(pick, *a), direction) / dot(direction, direction)).clamp(0., 1.))
        }
        Curve::Circular {
            center,
            start,
            sweep,
            closed,
            ..
        } => {
            let angle = (pick[1] - center[1]).atan2(pick[0] - center[0]);
            let progress = if *sweep > 0. {
                (angle - start).rem_euclid(TAU)
            } else {
                (start - angle).rem_euclid(TAU)
            };
            let t = progress / sweep.abs();
            if *closed || t <= 1. {
                Ok(t)
            } else {
                Ok(
                    if distance(pick, curve.point(0.)?) < distance(pick, curve.point(1.)?) {
                        0.
                    } else {
                        1.
                    },
                )
            }
        }
        Curve::Spline { curve: spline, .. } => {
            // Find every sampled local minimum before refining each basin. This is
            // only a picking decision; exact knot insertion constructs the edit.
            let samples = (spline.control_points.len() * 16).clamp(64, 2048);
            let values = (0..=samples)
                .map(|i| {
                    curve
                        .point(i as f64 / samples as f64)
                        .map(|p| distance(p, pick))
                })
                .collect::<Result<Vec<_>>>()?;
            let mut best = if values[0] < values[samples] {
                (values[0], 0.)
            } else {
                (values[samples], 1.)
            };
            for i in 1..samples {
                if values[i] <= values[i - 1] && values[i] <= values[i + 1] {
                    let (mut lo, mut hi) = (
                        (i - 1) as f64 / samples as f64,
                        (i + 1) as f64 / samples as f64,
                    );
                    for _ in 0..48 {
                        let a = lo + (hi - lo) / 3.;
                        let b = hi - (hi - lo) / 3.;
                        if distance(curve.point(a)?, pick) < distance(curve.point(b)?, pick) {
                            hi = b;
                        } else {
                            lo = a;
                        }
                    }
                    let t = (lo + hi) / 2.;
                    let value = distance(curve.point(t)?, pick);
                    if value < best.0 {
                        best = (value, t);
                    }
                }
            }
            Ok(best.1)
        }
    }
}

fn line_line(
    a: [f64; 2],
    b: [f64; 2],
    c: [f64; 2],
    d: [f64; 2],
    tolerance: f64,
    unbounded: bool,
) -> Result<Vec<[f64; 2]>> {
    let (u, v, w) = (sub(b, a), sub(d, c), sub(c, a));
    let denominator = cross(u, v);
    if denominator.abs() <= 1e-12 * dot(u, u).sqrt() * dot(v, v).sqrt() {
        if cross(w, u).abs() <= tolerance * dot(u, u).sqrt() {
            if !unbounded {
                let (first, second) =
                    (dot(sub(c, a), u) / dot(u, u), dot(sub(d, a), u) / dot(u, u));
                let (lo, hi) = (first.min(second).max(0.), first.max(second).min(1.));
                if hi < lo - PARAMETER_TOLERANCE {
                    return Ok(vec![]);
                }
                if (hi - lo).abs() <= PARAMETER_TOLERANCE {
                    return Ok(vec![[a[0] + u[0] * lo, a[1] + u[1] * lo]]);
                }
            }
            return Err(CadError::Unsupported(
                "Overlapping collinear members do not define an unambiguous trim boundary".into(),
            ));
        }
        return Ok(vec![]);
    }
    let t = cross(w, v) / denominator;
    Ok(vec![[a[0] + t * u[0], a[1] + t * u[1]]])
}
fn line_circle(
    a: [f64; 2],
    b: [f64; 2],
    center: [f64; 2],
    radius: f64,
    tolerance: f64,
) -> Vec<[f64; 2]> {
    let (u, w) = (sub(b, a), sub(a, center));
    let aa = dot(u, u);
    let middle = -dot(u, w) / aa;
    let nearest = [a[0] + middle * u[0], a[1] + middle * u[1]];
    let delta = radius * radius - dot(sub(nearest, center), sub(nearest, center));
    if delta < -tolerance * (2. * radius + tolerance) {
        return vec![];
    }
    let spread = (delta.max(0.) / aa).sqrt();
    let mut out = vec![[
        a[0] + (middle - spread) * u[0],
        a[1] + (middle - spread) * u[1],
    ]];
    if spread * aa.sqrt() > tolerance {
        out.push([
            a[0] + (middle + spread) * u[0],
            a[1] + (middle + spread) * u[1],
        ]);
    }
    out
}
fn circle_circle(
    a: [f64; 2],
    ra: f64,
    b: [f64; 2],
    rb: f64,
    tolerance: f64,
) -> Result<Vec<[f64; 2]>> {
    let delta = sub(b, a);
    let d = distance(a, b);
    if d <= tolerance {
        if (ra - rb).abs() <= tolerance {
            return Err(CadError::Unsupported(
                "Coincident circular members do not define an unambiguous trim boundary".into(),
            ));
        }
        return Ok(vec![]);
    }
    if d > ra + rb + tolerance || d < (ra - rb).abs() - tolerance {
        return Ok(vec![]);
    }
    let along = (ra * ra - rb * rb + d * d) / (2. * d);
    let height_squared = ra * ra - along * along;
    if height_squared < -tolerance * (2. * ra + tolerance) {
        return Ok(vec![]);
    }
    let height = height_squared.max(0.).sqrt();
    let midpoint = [a[0] + delta[0] * along / d, a[1] + delta[1] * along / d];
    let normal = [-delta[1] / d, delta[0] / d];
    let mut out = vec![[
        midpoint[0] + normal[0] * height,
        midpoint[1] + normal[1] * height,
    ]];
    if height > tolerance {
        out.push([
            midpoint[0] - normal[0] * height,
            midpoint[1] - normal[1] * height,
        ]);
    }
    Ok(out)
}

fn homogeneous(curve: &sketch_edit::Curve2) -> Vec<Vector3<f64>> {
    curve
        .control_points
        .iter()
        .zip(&curve.weights)
        .map(|(p, w)| Vector3::new(p[0] * w, p[1] * w, *w))
        .collect()
}
fn from_homogeneous(
    points: Vec<Vector3<f64>>,
    knots: Vec<f64>,
    degree: usize,
) -> sketch_edit::Curve2 {
    sketch_edit::Curve2 {
        control_points: points.iter().map(|p| [p.x / p.z, p.y / p.z]).collect(),
        weights: points.iter().map(|p| p.z).collect(),
        knots,
        degree,
    }
}
fn insert_knot(curve: &sketch_edit::Curve2, u: f64) -> Result<sketch_edit::Curve2> {
    let p = curve.degree;
    let controls = homogeneous(curve);
    let n = controls.len() - 1;
    let k = (p..=n)
        .find(|i| u >= curve.knots[*i] && u < curve.knots[*i + 1])
        .ok_or_else(|| invalid("Split position must lie inside the spline domain"))?;
    let multiplicity = curve.knots.iter().filter(|knot| **knot == u).count();
    if multiplicity >= p {
        return Err(invalid("Spline split knot already has full multiplicity"));
    }
    let mut output = vec![Vector3::zeros(); controls.len() + 1];
    output[..=k - p].copy_from_slice(&controls[..=k - p]);
    output[k - multiplicity + 1..].copy_from_slice(&controls[k - multiplicity..]);
    for i in k - p + 1..=k - multiplicity {
        let denominator = curve.knots[i + p] - curve.knots[i];
        if denominator <= 0. {
            return Err(invalid("Degenerate spline split span"));
        }
        let alpha = (u - curve.knots[i]) / denominator;
        output[i] = controls[i - 1] * (1. - alpha) + controls[i] * alpha;
    }
    let mut knots = curve.knots.clone();
    knots.insert(k + 1, u);
    Ok(from_homogeneous(output, knots, p))
}
fn split_spline(
    curve: &sketch_edit::Curve2,
    t: f64,
) -> Result<(sketch_edit::Curve2, sketch_edit::Curve2)> {
    if !(PARAMETER_TOLERANCE..1. - PARAMETER_TOLERANCE).contains(&t) {
        return Err(invalid("Split position is too close to a spline endpoint"));
    }
    let mut expanded = curve.clone();
    let mut u = curve.knots[curve.degree]
        + (curve.knots[curve.control_points.len()] - curve.knots[curve.degree]) * t;
    if let Some(existing) = curve.knots.iter().find(|k| {
        (**k - u).abs()
            <= 1e-12 * (curve.knots[curve.control_points.len()] - curve.knots[curve.degree])
    }) {
        u = *existing;
    }
    if expanded.knots.iter().filter(|k| **k == u).count() > curve.degree {
        return Err(CadError::Unsupported(
            "Splitting a discontinuous spline knot requires separate source members".into(),
        ));
    }
    while expanded.knots.iter().filter(|k| **k == u).count() < curve.degree {
        expanded = insert_knot(&expanded, u)?;
    }
    let k = expanded
        .knots
        .iter()
        .rposition(|k| *k == u)
        .ok_or_else(|| invalid("Missing spline split knot"))?;
    let split = k - curve.degree;
    let controls = homogeneous(&expanded);
    let mut first_knots = expanded.knots[..=k].to_vec();
    first_knots.push(u);
    let mut second_knots = vec![u];
    second_knots.extend_from_slice(&expanded.knots[split + 1..]);
    let first = from_homogeneous(controls[..=split].to_vec(), first_knots, curve.degree);
    let second = from_homogeneous(controls[split..].to_vec(), second_knots, curve.degree);
    sketch_edit::validate_nurbs(&first)?;
    sketch_edit::validate_nurbs(&second)?;
    Ok((first, second))
}
fn sub_spline(curve: &sketch_edit::Curve2, start: f64, end: f64) -> Result<sketch_edit::Curve2> {
    let mut curve = curve.clone();
    if end < 1. - PARAMETER_TOLERANCE {
        curve = split_spline(&curve, end)?.0;
    }
    if start > PARAMETER_TOLERANCE {
        curve = split_spline(&curve, start / end)?.1;
    }
    Ok(curve)
}
pub(crate) fn bezier_spans(curve: &sketch_edit::Curve2) -> Result<Vec<sketch_edit::Curve2>> {
    sketch_edit::validate_nurbs(curve)?;
    let start = curve.knots[curve.degree];
    let width = curve.knots[curve.control_points.len()] - start;
    let mut spans = Vec::new();
    for pair in curve
        .knots
        .windows(2)
        .filter(|pair| pair[1] > pair[0] && pair[0] >= start && pair[1] <= start + width)
    {
        let span = sub_spline(curve, (pair[0] - start) / width, (pair[1] - start) / width)?;
        if span.control_points.len() != span.degree + 1 {
            return Err(invalid(
                "Exact carrier proof requires finite Bezier knot spans",
            ));
        }
        spans.push(span);
    }
    Ok(spans)
}
fn binomial(n: usize, k: usize) -> f64 {
    (0..k.min(n - k)).fold(1., |v, i| v * (n - i) as f64 / (i + 1) as f64)
}
fn product(a: &[f64], b: &[f64]) -> Vec<f64> {
    let (m, n) = (a.len() - 1, b.len() - 1);
    (0..=m + n)
        .map(|k| {
            (k.saturating_sub(n)..=k.min(m))
                .map(|i| binomial(m, i) * binomial(n, k - i) / binomial(m + n, k) * a[i] * b[k - i])
                .sum()
        })
        .collect()
}
fn polynomial_value(coefficients: &[f64], t: f64) -> f64 {
    let mut values = coefficients.to_vec();
    for width in (1..values.len()).rev() {
        for i in 0..width {
            values[i] = values[i] * (1. - t) + values[i + 1] * t;
        }
    }
    values[0]
}
fn polynomial_roots(coefficients: &[f64]) -> Result<Vec<f64>> {
    let scale = coefficients.iter().map(|v| v.abs()).fold(0., f64::max);
    if !scale.is_finite() {
        return Err(invalid("Spline intersection arithmetic overflow"));
    }
    if scale <= 1e-14 {
        return Err(CadError::Unsupported(
            "A spline overlaps the trim boundary; select a different boundary".into(),
        ));
    }
    let original = coefficients.iter().map(|v| v / scale).collect::<Vec<_>>();
    let mut stack = vec![(original.clone(), 0., 1., 0usize)];
    let mut roots = vec![];
    let mut visited = 0;
    while let Some((coefficients, lo, hi, depth)) = stack.pop() {
        visited += 1;
        if visited > 65536 {
            return Err(CadError::Unsupported(
                "Spline intersection isolation exceeded its complexity limit".into(),
            ));
        }
        let min = coefficients.iter().copied().fold(f64::INFINITY, f64::min);
        let max = coefficients
            .iter()
            .copied()
            .fold(f64::NEG_INFINITY, f64::max);
        if min > 1e-14 || max < -1e-14 {
            continue;
        }
        if hi - lo <= PARAMETER_TOLERANCE || depth >= 40 {
            let candidate = (lo + hi) / 2.;
            if polynomial_value(&original, candidate).abs() <= 1e-10 {
                roots.push(candidate);
            }
            continue;
        }
        let mut values = coefficients;
        let mut left = vec![values[0]];
        let mut right = vec![*values.last().unwrap()];
        for width in (1..values.len()).rev() {
            for i in 0..width {
                values[i] = (values[i] + values[i + 1]) / 2.;
            }
            left.push(values[0]);
            right.push(values[width - 1]);
        }
        right.reverse();
        let mid = (lo + hi) / 2.;
        stack.push((right, mid, hi, depth + 1));
        stack.push((left, lo, mid, depth + 1));
    }
    roots.sort_by(f64::total_cmp);
    roots.dedup_by(|a, b| (*a - *b).abs() < 1e-7);
    Ok(roots)
}
fn spline_intersections(
    spline: &sketch_edit::Curve2,
    boundary: &Curve,
    unbounded: bool,
    tolerance: f64,
) -> Result<Vec<f64>> {
    if matches!(boundary, Curve::Spline { .. }) {
        return Err(CadError::Unsupported("Trim/Extend intersections between two general splines or ellipses are not yet supported; Split preserves each curve exactly".into()));
    }
    let mut expanded = spline.clone();
    let start = spline.knots[spline.degree];
    let end = spline.knots[spline.control_points.len()];
    let mut interior = spline
        .knots
        .iter()
        .copied()
        .filter(|k| *k > start && *k < end)
        .collect::<Vec<_>>();
    interior.dedup();
    for u in interior {
        while expanded.knots.iter().filter(|k| **k == u).count() < expanded.degree {
            expanded = insert_knot(&expanded, u)?;
        }
    }
    let controls = homogeneous(&expanded);
    let mut intersections = vec![];
    for span in expanded.degree..expanded.control_points.len() {
        let (lo, hi) = (expanded.knots[span], expanded.knots[span + 1]);
        if lo >= hi {
            continue;
        }
        let points = &controls[span - expanded.degree..=span];
        let coefficients: Vec<f64> = match boundary {
            Curve::Line(a, b) => {
                let direction = sub(*b, *a);
                points
                    .iter()
                    .map(|p| (p.x - a[0] * p.z) * direction[1] - (p.y - a[1] * p.z) * direction[0])
                    .collect()
            }
            Curve::Circular { center, radius, .. } => {
                let x = points
                    .iter()
                    .map(|p| p.x - center[0] * p.z)
                    .collect::<Vec<_>>();
                let y = points
                    .iter()
                    .map(|p| p.y - center[1] * p.z)
                    .collect::<Vec<_>>();
                let w = points.iter().map(|p| p.z).collect::<Vec<_>>();
                let (xx, yy, ww) = (product(&x, &x), product(&y, &y), product(&w, &w));
                xx.into_iter()
                    .zip(yy)
                    .zip(ww)
                    .map(|((x, y), w)| x + y - radius * radius * w)
                    .collect()
            }
            _ => unreachable!(),
        };
        for local in polynomial_roots(&coefficients)? {
            let t = ((lo + (hi - lo) * local) - start) / (end - start);
            let point = sketch_edit::nurbs_point(spline, t)?;
            if boundary.parameter(point, unbounded, tolerance).is_some() {
                intersections.push(t);
            }
        }
    }
    intersections.sort_by(f64::total_cmp);
    intersections.dedup_by(|a, b| (*a - *b).abs() < 1e-7);
    Ok(intersections)
}
#[derive(Clone)]
struct Intersection {
    parameter: f64,
    boundary: Id,
    point: [f64; 2],
}
fn intersections(
    doc: &Document,
    owner: &str,
    sketch: &Sketch,
    target: &Curve,
    request: &Request,
) -> Result<Vec<Intersection>> {
    let mut output = vec![];
    for (key, member) in &sketch.members {
        if key == &request.member || (!request.include_construction && member.construction) {
            continue;
        }
        let boundary = curve(doc, owner, sketch, key)?;
        let unbounded = request.kind == Kind::Extend;
        if !unbounded {
            let (a, b) = target.bounds();
            let (c, d) = boundary.bounds();
            if (0..2).any(|i| b[i] + request.tolerance < c[i] || d[i] + request.tolerance < a[i]) {
                continue;
            }
        }
        let points = match (target, &boundary) {
            (Curve::Line(a, b), Curve::Line(c, d)) => {
                line_line(*a, *b, *c, *d, request.tolerance, unbounded)?
            }
            (Curve::Line(a, b), Curve::Circular { center, radius, .. })
            | (Curve::Circular { center, radius, .. }, Curve::Line(a, b)) => {
                line_circle(*a, *b, *center, *radius, request.tolerance)
            }
            (
                Curve::Circular {
                    center: a,
                    radius: ra,
                    ..
                },
                Curve::Circular {
                    center: b,
                    radius: rb,
                    ..
                },
            ) => circle_circle(*a, *ra, *b, *rb, request.tolerance)?,
            (Curve::Spline { curve: spline, .. }, _) => {
                for t in spline_intersections(spline, &boundary, false, request.tolerance)? {
                    output.push(Intersection {
                        parameter: t,
                        boundary: key.clone(),
                        point: target.point(t)?,
                    });
                }
                continue;
            }
            (_, Curve::Spline { curve: spline, .. }) => {
                spline_intersections(spline, target, unbounded, request.tolerance)?
                    .into_iter()
                    .map(|t| sketch_edit::nurbs_point(spline, t))
                    .collect::<Result<Vec<_>>>()?
            }
        };
        for point in points {
            if boundary
                .parameter(point, false, request.tolerance)
                .is_none()
                && !matches!(boundary, Curve::Spline { .. })
            {
                continue;
            }
            if let Some(parameter) = target.parameter(point, unbounded, request.tolerance) {
                output.push(Intersection {
                    parameter,
                    boundary: key.clone(),
                    point,
                });
            }
        }
    }
    output.sort_by(|a, b| a.parameter.total_cmp(&b.parameter));
    output.dedup_by(|a, b| (a.parameter - b.parameter).abs() < 1e-7);
    Ok(output)
}

struct Ids {
    namespace: Id,
    point: usize,
    constraint: usize,
}
impl Ids {
    fn point(&mut self) -> Id {
        let key = format!("{}-control-{}", self.namespace, self.point);
        self.point += 1;
        key
    }
    fn constraint(&mut self) -> Id {
        let key = format!("{}-relation-{}", self.namespace, self.constraint);
        self.constraint += 1;
        key
    }
}
fn point_key(
    sketch: &mut Sketch,
    ids: &mut Ids,
    point: [f64; 2],
    tolerance: f64,
    candidates: &[Id],
) -> Id {
    if let Some(key) = candidates.iter().find(|id| {
        sketch
            .points
            .get(*id)
            .is_some_and(|p| distance(*p, point) <= tolerance)
    }) {
        return key.clone();
    }
    let key = ids.point();
    sketch.points.insert(key.clone(), point);
    key
}
fn constraint(sketch: &mut Sketch, ids: &mut Ids, kind: &str, entities: Vec<Id>, value: Expr) {
    if !sketch
        .constraints
        .iter()
        .any(|c| c.kind == kind && c.entities == entities)
    {
        sketch.constraints.push(SketchConstraint {
            driven: false,
            contact: None,
            distance_side: None,
            id: ids.constraint(),
            kind: kind.into(),
            entities,
            value,
        });
    }
}
struct Construction<'a> {
    endpoints: &'a [Id],
    support: Option<&'a (Id, Expr)>,
    tolerance: f64,
    ids: &'a mut Ids,
}
fn write_piece(
    sketch: &mut Sketch,
    key: Id,
    original: &SketchMember,
    exact: &Curve,
    range: (f64, f64),
    construction: &mut Construction<'_>,
) -> Result<()> {
    let (start, end) = range;
    if end - start <= PARAMETER_TOLERANCE {
        return Err(invalid("Trim would produce a zero-length member"));
    }
    let a = point_key(
        sketch,
        construction.ids,
        exact.point(start)?,
        construction.tolerance,
        construction.endpoints,
    );
    let b = point_key(
        sketch,
        construction.ids,
        exact.point(end)?,
        construction.tolerance,
        construction.endpoints,
    );
    let curve = match exact {
        Curve::Line(..) => SketchCurve::Line { start: a, end: b },
        Curve::Circular { .. } => {
            let through = point_key(
                sketch,
                construction.ids,
                exact.point((start + end) / 2.)?,
                construction.tolerance,
                &[],
            );
            SketchCurve::Arc {
                start: a,
                through,
                end: b,
            }
        }
        Curve::Spline { curve, bezier, .. } => {
            let segment = sub_spline(curve, start, end)?;
            let last = segment.control_points.len() - 1;
            let points = segment
                .control_points
                .iter()
                .enumerate()
                .map(|(i, p)| {
                    if i == 0 {
                        a.clone()
                    } else if i == last {
                        b.clone()
                    } else {
                        point_key(sketch, construction.ids, *p, construction.tolerance, &[])
                    }
                })
                .collect();
            if *bezier {
                SketchCurve::Bezier { points }
            } else {
                SketchCurve::Nurbs {
                    points,
                    weights: segment.weights,
                    knots: segment.knots,
                    degree: segment.degree,
                }
            }
        }
    };
    sketch.members.insert(
        key.clone(),
        SketchMember {
            construction: original.construction,
            curve,
        },
    );
    if let Some((center, radius)) = construction.support {
        constraint(
            sketch,
            construction.ids,
            "arc_center",
            vec![center.clone(), key.clone()],
            "0 mm".into(),
        );
        constraint(
            sketch,
            construction.ids,
            "radius",
            vec![key],
            radius.clone(),
        );
    }
    Ok(())
}

/// Apply atomically to a source clone. The surviving first piece retains the
/// original member identity; shared unmodified endpoints keep their identities.
pub fn apply(
    doc: &Document,
    owner: &str,
    sketch: &mut Sketch,
    request: &Request,
) -> Result<EditReport> {
    if request.edit_id.is_empty()
        || request.edit_id.len() > 128
        || request
            .edit_id
            .chars()
            .any(|c| !c.is_ascii_alphanumeric() && c != '-' && c != '_')
        || !request.parameter.is_finite()
        || !(0. ..=1.).contains(&request.parameter)
        || !request.tolerance.is_finite()
        || request.tolerance <= 0.
        || request.tolerance > 1.
    {
        return Err(invalid(
            "Sketch edit needs a normalized position and a positive millimetre tolerance",
        ));
    }
    editable(sketch, &request.member, request.kind)?;
    let prefix = format!("{}-", request.edit_id);
    if sketch
        .points
        .keys()
        .chain(sketch.members.keys())
        .any(|key| key.starts_with(&prefix))
        || sketch.constraints.iter().any(|c| c.id.starts_with(&prefix))
    {
        return Err(invalid(
            "The sketch edit identity already owns committed geometry; start a new edit namespace",
        ));
    }
    let exact = curve(doc, owner, sketch, &request.member)?;
    let original = sketch.members[&request.member].clone();
    let mut source = sketch.clone();
    let mut ids = Ids {
        namespace: request.edit_id.clone(),
        point: 0,
        constraint: 0,
    };
    let boundaries = if request.kind == Kind::Trim
        || (request.kind == Kind::Extend && request.extension.is_none())
    {
        intersections(doc, owner, sketch, &exact, request)?
    } else {
        vec![]
    };
    if request.kind == Kind::Trim
        && boundaries
            .iter()
            .any(|b| (b.parameter - request.parameter).abs() <= 1e-7)
    {
        return Err(invalid(
            "Click inside the portion to trim, away from an intersection",
        ));
    }
    let mut ranges = vec![];
    match request.kind {
        Kind::Split if exact.closed() => {
            let a = request.parameter.rem_euclid(1.);
            let b = (a + 0.5).rem_euclid(1.);
            let (lo, hi) = if a < b { (a, b) } else { (b, a) };
            if matches!(exact, Curve::Circular { .. }) {
                ranges.extend([(lo, hi), (hi, lo + 1.)]);
            } else {
                ranges.extend(
                    [(0., lo), (lo, hi), (hi, 1.)]
                        .into_iter()
                        .filter(|(a, b)| b - a > PARAMETER_TOLERANCE),
                );
            }
        }
        Kind::Split => {
            if !(PARAMETER_TOLERANCE..1. - PARAMETER_TOLERANCE).contains(&request.parameter) {
                return Err(invalid("Pick an interior point to split the member"));
            }
            ranges.extend([(0., request.parameter), (request.parameter, 1.)]);
        }
        Kind::Trim => {
            if exact.closed() {
                if boundaries.len() == 1 {
                    return Err(invalid(
                        "A closed member needs two distinct intersections to trim a portion",
                    ));
                }
                if boundaries.len() >= 2 {
                    let lo = boundaries
                        .iter()
                        .rev()
                        .find(|b| b.parameter < request.parameter - PARAMETER_TOLERANCE)
                        .map(|b| b.parameter)
                        .unwrap_or(boundaries.last().unwrap().parameter - 1.);
                    let hi = boundaries
                        .iter()
                        .find(|b| b.parameter > request.parameter + PARAMETER_TOLERANCE)
                        .map(|b| b.parameter)
                        .unwrap_or(boundaries[0].parameter + 1.);
                    if lo < 0. {
                        ranges.push((hi, lo + 1.));
                    } else if hi > 1. {
                        ranges.push((hi - 1., lo));
                    } else if matches!(exact, Curve::Circular { .. }) {
                        ranges.push((hi, lo + 1.));
                    } else {
                        ranges.extend(
                            [(0., lo), (hi, 1.)]
                                .into_iter()
                                .filter(|(a, b)| b - a > PARAMETER_TOLERANCE),
                        );
                    }
                }
            } else {
                let lo = boundaries
                    .iter()
                    .rev()
                    .find(|b| b.parameter < request.parameter - PARAMETER_TOLERANCE)
                    .map(|b| b.parameter.clamp(0., 1.))
                    .unwrap_or(0.);
                let hi = boundaries
                    .iter()
                    .find(|b| b.parameter > request.parameter + PARAMETER_TOLERANCE)
                    .map(|b| b.parameter.clamp(0., 1.))
                    .unwrap_or(1.);
                ranges.extend(
                    [(0., lo), (hi, 1.)]
                        .into_iter()
                        .filter(|(a, b)| b - a > PARAMETER_TOLERANCE),
                );
            }
        }
        Kind::Extend => {
            let first = request.parameter < 0.5;
            let t = if let Some(extension) = request.extension {
                if !extension.is_finite() || extension <= request.tolerance {
                    return Err(invalid("Extension length must be positive"));
                }
                let length = match &exact {
                    Curve::Line(a, b) => distance(*a, *b),
                    Curve::Circular { radius, sweep, .. } => radius * sweep.abs(),
                    _ => unreachable!(),
                };
                if first {
                    -extension / length
                } else {
                    1. + extension / length
                }
            } else {
                let mut candidates = boundaries.iter().map(|b| b.parameter).collect::<Vec<_>>();
                if let Curve::Circular { sweep, .. } = &exact {
                    let period = TAU / sweep.abs();
                    if first {
                        for t in &mut candidates {
                            *t -= period;
                        }
                    }
                }
                candidates.into_iter().filter(|t| if first { *t < -PARAMETER_TOLERANCE } else { *t > 1.+PARAMETER_TOLERANCE })
                    .min_by(|a,b| (if first { -a } else { a-1. }).total_cmp(&(if first { -b } else { b-1. })))
                    .ok_or_else(|| invalid("No bounded sketch member intersects beyond that endpoint; choose an extension length"))?
            };
            let range = if first { (t, 1.) } else { (0., t) };
            if let Curve::Circular { sweep, .. } = &exact
                && (range.1 - range.0) * sweep.abs() >= TAU - PARAMETER_TOLERANCE
            {
                return Err(invalid(
                    "An extended arc must remain shorter than a complete circle",
                ));
            }
            ranges.push(range);
        }
    }
    let old_controls = original
        .curve
        .points()
        .into_iter()
        .map(str::to_string)
        .collect::<BTreeSet<_>>();
    let endpoints = match &original.curve {
        SketchCurve::Line { start, end } | SketchCurve::Arc { start, end, .. } => {
            vec![start.clone(), end.clone()]
        }
        SketchCurve::Bezier { points } | SketchCurve::Nurbs { points, .. } => {
            vec![points[0].clone(), points.last().unwrap().clone()]
        }
        _ => vec![],
    };
    let support = match (&exact, &original.curve) {
        (Curve::Circular { center, radius, .. }, original_curve) if !ranges.is_empty() => {
            let center_key = if let SketchCurve::Circle { center, .. } = original_curve {
                center.clone()
            } else {
                point_key(&mut source, &mut ids, *center, request.tolerance, &[])
            };
            let radius = if let SketchCurve::Circle { radius, .. } = original_curve {
                radius.clone()
            } else {
                sketch
                    .constraints
                    .iter()
                    .find(|c| {
                        !c.driven && c.kind == "radius" && c.entities == [request.member.clone()]
                    })
                    .map(|c| c.value.clone())
                    .unwrap_or_else(|| Expr(format!("{radius} mm")))
            };
            Some((center_key, radius))
        }
        _ => None,
    };
    source.members.remove(&request.member);
    let mut removed_constraints = vec![];
    source.constraints.retain(|c| {
        let compatible_line_relation = matches!(original.curve, SketchCurve::Line { .. })
            && ((matches!(c.kind.as_str(), "horizontal" | "vertical") && c.entities.len() == 1)
                || (matches!(c.kind.as_str(), "parallel" | "perpendicular" | "angle")
                    && c.entities.len() == 2))
            && !ranges.is_empty();
        let keep = (!c.entities.contains(&request.member) || compatible_line_relation)
            && !c.entities.iter().any(|p| {
                old_controls.contains(p)
                    && !endpoints.contains(p)
                    && !source
                        .members
                        .values()
                        .any(|m| m.curve.points().contains(&p.as_str()))
            });
        if !keep {
            removed_constraints.push(c.id.clone());
        }
        keep
    });
    let mut members = vec![];
    let mut endpoint_candidates = endpoints;
    for boundary in &boundaries {
        if let Some(member) = source.members.get(&boundary.boundary) {
            endpoint_candidates.extend(member.curve.points().into_iter().map(str::to_string));
        }
    }
    for (i, range) in ranges.into_iter().enumerate() {
        let key = if i == 0 {
            request.member.clone()
        } else {
            format!("{}-piece-{i}", request.edit_id)
        };
        write_piece(
            &mut source,
            key.clone(),
            &original,
            &exact,
            range,
            &mut Construction {
                endpoints: &endpoint_candidates,
                support: support.as_ref(),
                tolerance: request.tolerance,
                ids: &mut ids,
            },
        )?;
        endpoint_candidates.extend(
            source.members[&key]
                .curve
                .points()
                .into_iter()
                .map(str::to_string),
        );
        members.push(key);
    }
    // Preserve only relations compatible with each shortened line; dimensions of
    // removed spans are intentionally removed and reported to the editor.
    if matches!(original.curve, SketchCurve::Line { .. }) {
        for c in &sketch.constraints {
            if matches!(c.kind.as_str(), "horizontal" | "vertical")
                && c.entities == [request.member.clone()]
            {
                for key in members.iter().skip(1) {
                    constraint(
                        &mut source,
                        &mut ids,
                        &c.kind,
                        vec![key.clone()],
                        c.value.clone(),
                    );
                }
            }
            if matches!(c.kind.as_str(), "parallel" | "perpendicular" | "angle")
                && c.entities.len() == 2
                && c.entities.contains(&request.member)
            {
                for key in members.iter().skip(1) {
                    let entities = c
                        .entities
                        .iter()
                        .map(|entity| {
                            if entity == &request.member {
                                key.clone()
                            } else {
                                entity.clone()
                            }
                        })
                        .collect();
                    constraint(&mut source, &mut ids, &c.kind, entities, c.value.clone());
                }
            }
        }
        if members.len() == 2 {
            constraint(
                &mut source,
                &mut ids,
                "parallel",
                members.clone(),
                "0 mm".into(),
            );
        }
    }
    for key in &members {
        let end_points = match &source.members[key].curve {
            SketchCurve::Line { start, end } | SketchCurve::Arc { start, end, .. } => {
                vec![start.clone(), end.clone()]
            }
            SketchCurve::Bezier { points } | SketchCurve::Nurbs { points, .. } => {
                vec![points[0].clone(), points.last().unwrap().clone()]
            }
            _ => vec![],
        };
        for point in end_points {
            for boundary in &boundaries {
                if distance(source.points[&point], boundary.point) <= request.tolerance {
                    let kind = match &source.members[&boundary.boundary].curve {
                        SketchCurve::Line { .. } => Some("point_on_line"),
                        SketchCurve::Arc { .. } => Some("point_on_arc"),
                        SketchCurve::Circle { .. } => Some("point_on_circle"),
                        _ => None,
                    };
                    if let Some(kind) = kind {
                        constraint(
                            &mut source,
                            &mut ids,
                            kind,
                            vec![point.clone(), boundary.boundary.clone()],
                            "0 mm".into(),
                        );
                    }
                }
            }
        }
    }
    let unused = old_controls
        .iter()
        .filter(|point| {
            !source
                .members
                .values()
                .any(|m| m.curve.points().contains(&point.as_str()))
                && support.as_ref().is_none_or(|s| &s.0 != *point)
                && !source
                    .transforms
                    .values()
                    .any(|p| p.transform.reference_points().contains(point))
        })
        .cloned()
        .collect::<BTreeSet<_>>();
    source.constraints.retain(|c| {
        let keep = !c.entities.iter().any(|p| unused.contains(p));
        if !keep {
            removed_constraints.push(c.id.clone());
        }
        keep
    });
    for point in old_controls {
        if !source
            .members
            .values()
            .any(|m| m.curve.points().contains(&point.as_str()))
            && !source
                .constraints
                .iter()
                .any(|c| c.entities.contains(&point))
            && !source
                .transforms
                .values()
                .any(|p| p.transform.reference_points().contains(&point))
        {
            source.points.remove(&point);
        }
    }
    if source.points.values().flatten().any(|v| !v.is_finite()) {
        return Err(invalid(
            "Sketch edit arithmetic exceeded the finite coordinate range",
        ));
    }
    sketch_edit::validate(&source)?;
    let deleted_member = members.is_empty();
    *sketch = source;
    Ok(EditReport {
        members,
        removed_constraints,
        deleted_member,
    })
}
