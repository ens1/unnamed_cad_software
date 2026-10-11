//! Exact profile partitioning and Euclidean rational loft carriers.
//!
//! A contour's authored edge count and traversal seam are not correspondence.
//! Split native curves, keep their ancestry, and give both boundary carriers
//! the same rational denominator before constructing a strip.
use cad_core::{CadError, Result};
use monstertruck::modeling::*;
use std::cell::Cell;
use std::collections::HashMap;

#[path = "loft_separation.rs"]
mod separation;
pub(crate) use separation::prove_skin_separation;

const EPS: f64 = 1e-7;
const MAX_PIECES: usize = 4096;
const MAX_TOTAL: usize = 32768;
const MAX_DEGREE: usize = 16;
const MAX_WORK: usize = 16_000_000;
const PARTITION_EPS: f64 = 128. * f64::EPSILON;
fn charge(work: &mut usize, amount: usize) -> Result<()> {
    *work = work
        .checked_add(amount)
        .ok_or_else(|| unsupported("the native correspondence work budget overflowed"))?;
    if *work > MAX_WORK {
        return Err(unsupported(
            "the bounded exact correspondence proof budget was exceeded",
        ));
    }
    Ok(())
}

fn invalid(message: impl ToString) -> CadError {
    CadError::Invalid(format!("Loft: {}", message.to_string()))
}
fn unsupported(message: &str) -> CadError {
    CadError::Unsupported(format!("Loft correspondence: {message}"))
}
fn point(h: Vector4) -> Result<Point3> {
    if ![h.x, h.y, h.z, h.w].iter().all(|x| x.is_finite()) || h.w <= 0. {
        return Err(unsupported(
            "native rational weights must be finite and positive",
        ));
    }
    let p = Point3::new(h.x / h.w, h.y / h.w, h.z / h.w);
    if ![p.x, p.y, p.z].iter().all(|v| v.is_finite()) {
        return Err(invalid("A rational boundary point is not finite"));
    }
    Ok(p)
}
fn carrier(curve: &Curve) -> Result<BsplineCurve<Vector4>> {
    let count = match curve {
        Curve::Line(_) => 2,
        Curve::BsplineCurve(c) => c.control_points().len(),
        Curve::NurbsCurve(c) => c.control_points().len(),
        _ => {
            return Err(unsupported(
                "only native lines and positive-weight polynomial/rational curves have exact carriers",
            ));
        }
    };
    if count > MAX_PIECES {
        return Err(unsupported(
            "native curve control count exceeds the exact carrier budget",
        ));
    }
    crate::extrusion::curve_hull(curve)?;
    let mut curve = curve.lift_up();
    curve.clamp();
    curve.knot_normalize();
    if curve.degree() == 0
        || curve.degree() > MAX_DEGREE
        || curve.control_points().len() > MAX_PIECES
    {
        return Err(unsupported(
            "native curve degree or control count exceeds the bounded exact carrier budget",
        ));
    }
    let weight = curve
        .control_points()
        .iter()
        .map(|h| h.w)
        .fold(0f64, f64::max);
    for h in curve.control_points() {
        point(*h)?;
        if h.w / weight < 1e-12 {
            return Err(unsupported(
                "native rational weights are too ill-conditioned for a bounded exact carrier",
            ));
        }
    }
    for h in curve.control_points_mut() {
        *h /= weight;
        point(*h)?;
    }
    Ok(curve)
}

#[derive(Clone)]
pub(crate) struct Piece {
    pub source: EdgeId,
    /// Directed normalized parameter of the original stored Edge::curve()
    /// construction orientation, independent of selected wire traversal.
    pub interval: [f64; 2],
}
pub(crate) struct Prepared {
    pub wires: Vec<Wire>,
    pub pieces: Vec<HashMap<EdgeId, Piece>>,
    pub centers: Vec<Point3>,
    pub normals: Vec<Vector3>,
    pub radial: bool,
    proof_work: Cell<usize>,
    ruled: Vec<bool>,
}
#[derive(Clone)]
struct Segment {
    curve: BsplineCurve<Vector4>,
    source: EdgeId,
    interval: [f64; 2],
    front: Vertex,
    back: Vertex,
}
impl Segment {
    fn reverse(&mut self) {
        self.curve.invert();
        self.interval.swap(0, 1);
        std::mem::swap(&mut self.front, &mut self.back);
    }
    fn split(&self, t: f64) -> Result<(Self, Self)> {
        let mut left = self.curve.clone();
        let mut right = left.cut(t);
        let vertex = Vertex::new(point(right.subs(t))?);
        left.knot_normalize();
        right.knot_normalize();
        let middle = self.interval[0] + (self.interval[1] - self.interval[0]) * t;
        Ok((
            Self {
                curve: left,
                back: vertex.clone(),
                interval: [self.interval[0], middle],
                ..self.clone()
            },
            Self {
                curve: right,
                front: vertex,
                interval: [middle, self.interval[1]],
                ..self.clone()
            },
        ))
    }
}

fn segments(wire: &Wire) -> Result<Vec<Segment>> {
    if wire.is_empty() || wire.len() > MAX_PIECES || !wire.is_continuous() {
        return Err(invalid(
            "Each profile must be a nonempty connected native contour",
        ));
    }
    let mut result = Vec::new();
    for edge in wire {
        let curve = carrier(&edge.oriented_curve())?;
        let mut previous = edge.front().clone();
        let beziers = curve.bezier_decomposition();
        for (index, mut piece) in beziers.iter().cloned().enumerate() {
            let range = piece.range_tuple();
            let next = if index + 1 == beziers.len() {
                edge.back().clone()
            } else {
                Vertex::new(point(piece.subs(range.1))?)
            };
            piece.knot_normalize();
            let interval = if edge.orientation() {
                [range.0, range.1]
            } else {
                [1. - range.0, 1. - range.1]
            };
            result.push(Segment {
                curve: piece,
                source: edge.id(),
                interval,
                front: previous,
                back: next.clone(),
            });
            previous = next;
        }
    }
    if result.len() > MAX_PIECES {
        return Err(unsupported(
            "native profile knot partitions exceed the correspondence budget",
        ));
    }
    Ok(result)
}

fn frame(
    segments: &[Segment],
    previous: Option<Vector3>,
) -> Result<(Point3, Vector3, Vector3, Vector3)> {
    let points = segments
        .iter()
        .flat_map(|s| s.curve.control_points())
        .map(|p| point(*p))
        .collect::<Result<Vec<_>>>()?;
    let center =
        Point3::from_vec(points.iter().map(|p| p.to_vec()).sum::<Vector3>() / points.len() as f64);
    if ![center.x, center.y, center.z].iter().all(|v| v.is_finite()) {
        return Err(invalid(
            "A native section frame exceeds finite coordinate bounds",
        ));
    }
    let first = points
        .iter()
        .map(|p| *p - center)
        .max_by(|a, b| a.magnitude2().total_cmp(&b.magnitude2()))
        .ok_or_else(|| invalid("Empty contour"))?;
    let cross = points
        .iter()
        .map(|p| first.cross(*p - center))
        .max_by(|a, b| a.magnitude2().total_cmp(&b.magnitude2()))
        .unwrap();
    if !first.magnitude2().is_finite() || !cross.magnitude2().is_finite() {
        return Err(invalid(
            "A native section frame is numerically ill-conditioned",
        ));
    }
    let mut normal = if cross.magnitude() > EPS * EPS {
        cross.normalize()
    } else {
        if first.magnitude() <= EPS {
            return Err(invalid("Collapsed contour"));
        }
        let tangent = first.normalize();
        tangent
            .cross(if tangent.x.abs() < 0.8 {
                Vector3::unit_x()
            } else {
                Vector3::unit_y()
            })
            .normalize()
    };
    if let Some(previous) = previous {
        if normal.dot(previous) < 0. {
            normal = -normal;
        }
    } else {
        // The frame is independent of the contour's control polygon, whose
        // signed area can disagree with the actual polynomial/rational curve.
        let dominant = [normal.x, normal.y, normal.z]
            .into_iter()
            .max_by(|a, b| a.abs().total_cmp(&b.abs()))
            .unwrap();
        if dominant < 0. {
            normal = -normal;
        }
    }
    if points.iter().any(|p| (*p - center).dot(normal).abs() > EPS) {
        return Err(unsupported(
            "a loft section must have a planar native contour; spatial sections need an authored correspondence frame",
        ));
    }
    let axis = if normal.x.abs() < 0.9 {
        Vector3::unit_x()
    } else {
        Vector3::unit_y()
    };
    let u = (axis - normal * axis.dot(normal)).normalize();
    if ![normal.x, normal.y, normal.z, u.x, u.y, u.z]
        .iter()
        .all(|v| v.is_finite())
    {
        return Err(invalid("A native section frame is not finite"));
    }
    Ok((center, normal, u, normal.cross(u)))
}
fn choose(n: usize, k: usize) -> f64 {
    (0..k.min(n - k)).fold(1., |a, i| a * (n - i) as f64 / (i + 1) as f64)
}
fn product_factor(p: usize, i: usize, q: usize, j: usize) -> f64 {
    choose(p, i) * choose(q, j) / choose(p + q, i + j)
}

/// Enclose the exact signed-area integral of a positive-weight Bezier span.
/// The homogeneous numerator is X cross X'; the X cross X W' term vanishes.
/// Integrating each positive/negative Bernstein term with the complete weight
/// hull bounds supplies an enclosure, exact apart from arithmetic allowances
/// for a constant denominator. No polygon winding or sampled chord is used.
fn area_interval(
    curve: &BsplineCurve<Vector4>,
    base: Point3,
    normal: Vector3,
    work: &mut usize,
) -> Result<(f64, f64)> {
    let degree = curve.degree();
    let controls = curve
        .control_points()
        .iter()
        .map(|h| Vector3::new(h.x, h.y, h.z) - base.to_vec() * h.w)
        .collect::<Vec<_>>();
    let mut wmin = f64::INFINITY;
    let mut wmax = 0f64;
    let mut centered = 0f64;
    let mut centering_majorant = 0f64;
    for (h, x) in curve.control_points().iter().zip(&controls) {
        wmin = wmin.min(h.w);
        wmax = wmax.max(h.w);
        centered = centered.max(x.x.abs().max(x.y.abs()).max(x.z.abs()));
        centering_majorant = centering_majorant.max(
            (h.x.abs() + base.x.abs() * h.w)
                .max(h.y.abs() + base.y.abs() * h.w)
                .max(h.z.abs() + base.z.abs() * h.w),
        );
    }
    let input_error = 8. * f64::EPSILON * centering_majorant;
    let coefficient_error =
        (128. * f64::EPSILON * (degree + 1).pow(2) as f64 * (centered + input_error).powi(2)
            + 16. * centered * input_error
            + 16. * input_error.powi(2))
            * degree as f64
            * normal.magnitude();
    let mut positive = 0.;
    let mut negative = 0.;
    for k in 0..2 * degree {
        charge(work, degree + 1)?;
        let mut coefficient = 0.;
        for i in 0..=degree {
            let Some(j) = k.checked_sub(i).filter(|j| *j < degree) else {
                continue;
            };
            coefficient += controls[i]
                .cross((controls[j + 1] - controls[j]) * degree as f64)
                .dot(normal)
                * product_factor(degree, i, degree - 1, j);
        }
        if coefficient >= 0. {
            positive += coefficient / (2 * degree) as f64;
        } else {
            negative += coefficient / (2 * degree) as f64;
        }
    }
    let low = positive / wmax.powi(2) + negative / wmin.powi(2) - coefficient_error / wmin.powi(2);
    let high = positive / wmin.powi(2) + negative / wmax.powi(2) + coefficient_error / wmin.powi(2);
    let arithmetic =
        64. * f64::EPSILON * (positive.abs() + negative.abs() + coefficient_error) / wmin.powi(2);
    if ![low, high, arithmetic, coefficient_error]
        .iter()
        .all(|v| v.is_finite())
    {
        return Err(invalid("The native signed-area arithmetic is not finite"));
    }
    Ok((low - arithmetic, high + arithmetic))
}

fn winding_sign(
    segments: &[Segment],
    base: Point3,
    normal: Vector3,
    work: &mut usize,
) -> Result<f64> {
    let mut spans = segments
        .iter()
        .map(|s| {
            let interval = area_interval(&s.curve, base, normal, work)?;
            Ok((s.clone(), interval, 0usize))
        })
        .collect::<Result<Vec<_>>>()?;
    loop {
        charge(work, spans.len() * 4)?;
        let lower = spans.iter().map(|(_, bounds, _)| bounds.0).sum::<f64>();
        let upper = spans.iter().map(|(_, bounds, _)| bounds.1).sum::<f64>();
        let majorant = spans
            .iter()
            .map(|(_, bounds, _)| bounds.0.abs().max(bounds.1.abs()))
            .sum::<f64>();
        let margin = EPS * EPS + 128. * f64::EPSILON * spans.len() as f64 * majorant;
        if ![lower, upper, majorant, margin]
            .iter()
            .all(|v| v.is_finite())
        {
            return Err(invalid("The native contour signed area is not finite"));
        }
        if lower > margin {
            return Ok(1.);
        }
        if upper < -margin {
            return Ok(-1.);
        }
        let index = spans
            .iter()
            .enumerate()
            .filter(|(_, (_, _, depth))| *depth < 16)
            .max_by(|(_, a), (_, b)| (a.1.1 - a.1.0).total_cmp(&(b.1.1 - b.1.0)))
            .map(|(index, _)| index);
        let Some(index) = index.filter(|_| spans.len() < MAX_PIECES) else {
            return Err(unsupported(
                "the actual rational contour winding cannot be certified within its native interval budget",
            ));
        };
        let (span, _, depth) = spans.swap_remove(index);
        let (left, right) = span.split(0.5)?;
        let a = area_interval(&left.curve, base, normal, work)?;
        let b = area_interval(&right.curve, base, normal, work)?;
        spans.push((left, a, depth + 1));
        spans.push((right, b, depth + 1));
    }
}

/// Intersection of exact Bernstein angular-derivative halfplanes. A nonempty
/// result supplies a certified kernel point, including for concave line/arc
/// profiles whose bounding-box center is outside the profile's visibility kernel.
fn radial_center(
    segments: &[Segment],
    base: Point3,
    u: Vector3,
    v: Vector3,
    work: &mut usize,
) -> Result<Option<Point3>> {
    let controls = segments
        .iter()
        .flat_map(|s| s.curve.control_points())
        .map(|h| point(*h))
        .collect::<Result<Vec<_>>>()?;
    let xy = |p: Point3| Vector2::new((p - base).dot(u), (p - base).dot(v));
    let mut low = Vector2::new(f64::INFINITY, f64::INFINITY);
    let mut high = Vector2::new(f64::NEG_INFINITY, f64::NEG_INFINITY);
    for p in controls {
        let p = xy(p);
        low.x = low.x.min(p.x);
        low.y = low.y.min(p.y);
        high.x = high.x.max(p.x);
        high.y = high.y.max(p.y);
    }
    let mut polygon = vec![
        low,
        Vector2::new(high.x, low.y),
        high,
        Vector2::new(low.x, high.y),
    ];
    for segment in segments {
        let p = segment.curve.degree();
        let h = segment
            .curve
            .control_points()
            .iter()
            .map(|h| {
                let relative = Vector3::new(h.x, h.y, h.z) - base.to_vec() * h.w;
                (Vector2::new(relative.dot(u), relative.dot(v)), h.w)
            })
            .collect::<Vec<_>>();
        for k in 0..2 * p {
            charge(work, p + 1 + polygon.len())?;
            let mut constant = 0.;
            let mut coefficient = Vector2::zero();
            for i in 0..=p {
                let Some(j) = k.checked_sub(i).filter(|j| *j < p) else {
                    continue;
                };
                let factor = product_factor(p, i, p - 1, j);
                let d = (h[j + 1].0 - h[j].0) * p as f64;
                let dw = (h[j + 1].1 - h[j].1) * p as f64;
                constant += factor * (h[i].0.x * d.y - h[i].0.y * d.x);
                let l = (d * h[i].1 - h[i].0 * dw) * factor;
                coefficient += Vector2::new(-l.y, l.x);
            }
            if ![constant, coefficient.x, coefficient.y]
                .iter()
                .all(|v| v.is_finite())
            {
                return Err(invalid(
                    "The exact radial-center coefficients exceed finite arithmetic bounds",
                ));
            }
            let threshold = EPS * EPS;
            let value = |p: Vector2| constant + coefficient.dot(p) - threshold;
            let mut clipped = Vec::new();
            for i in 0..polygon.len() {
                let a = polygon[i];
                let b = polygon[(i + 1) % polygon.len()];
                let va = value(a);
                let vb = value(b);
                if !va.is_finite() || !vb.is_finite() {
                    return Err(invalid(
                        "The exact radial-center arrangement is numerically ill-conditioned",
                    ));
                }
                if va >= 0. {
                    clipped.push(a);
                }
                if (va >= 0.) != (vb >= 0.) {
                    clipped.push(a + (b - a) * (va / (va - vb)));
                }
            }
            if clipped.len() > 1024 {
                return Err(unsupported(
                    "the native radial-center halfplane arrangement exceeds its vertex budget",
                ));
            }
            polygon = clipped;
            if polygon.is_empty() {
                return Ok(None);
            }
        }
    }
    let center = polygon.iter().copied().sum::<Vector2>() / polygon.len() as f64;
    if ![center.x, center.y].iter().all(|v| v.is_finite()) {
        return Err(invalid("The native radial center is not finite"));
    }
    Ok(Some(base + u * center.x + v * center.y))
}
fn angle(point: Point3, center: Point3, u: Vector3, v: Vector3) -> f64 {
    let d = point - center;
    d.dot(v).atan2(d.dot(u)).rem_euclid(std::f64::consts::TAU)
}
fn angle_parameter(
    segment: &Segment,
    center: Point3,
    u: Vector3,
    v: Vector3,
    target: f64,
) -> Result<f64> {
    let start = angle(segment.front.point(), center, u, v);
    let span =
        (angle(segment.back.point(), center, u, v) - start).rem_euclid(std::f64::consts::TAU);
    let wanted = (target - start).rem_euclid(std::f64::consts::TAU);
    if wanted < PARTITION_EPS {
        return Ok(0.);
    }
    if (span - wanted).abs() < PARTITION_EPS {
        return Ok(1.);
    }
    let mut lo = 0.;
    let mut hi = 1.;
    for _ in 0..64 {
        let t = (lo + hi) * 0.5;
        let progress = (angle(point(segment.curve.subs(t))?, center, u, v) - start)
            .rem_euclid(std::f64::consts::TAU);
        if progress < wanted {
            lo = t;
        } else {
            hi = t;
        }
    }
    Ok((lo + hi) * 0.5)
}
fn radial_seam(
    segments: &mut Vec<Segment>,
    center: Point3,
    u: Vector3,
    v: Vector3,
    target: f64,
) -> Result<()> {
    let index = segments
        .iter()
        .position(|s| {
            let a = angle(s.front.point(), center, u, v);
            let span = (angle(s.back.point(), center, u, v) - a).rem_euclid(std::f64::consts::TAU);
            (target - a).rem_euclid(std::f64::consts::TAU) <= span + PARTITION_EPS
        })
        .ok_or_else(|| invalid("Native angular seam cannot be located"))?;
    let t = angle_parameter(&segments[index], center, u, v, target)?;
    if t <= PARTITION_EPS {
        segments.rotate_left(index);
    } else if t >= 1. - PARTITION_EPS {
        let next = (index + 1) % segments.len();
        segments.rotate_left(next);
    } else {
        let (left, right) = segments[index].split(t)?;
        segments[index] = left;
        segments.insert(index + 1, right);
        segments.rotate_left(index + 1);
    }
    Ok(())
}
fn reverse(segments: &mut [Segment]) {
    segments.reverse();
    for segment in segments {
        segment.reverse();
    }
}

fn split_scalar(values: &[f64]) -> (Vec<f64>, Vec<f64>) {
    let mut row = values.to_vec();
    let mut left = vec![row[0]];
    let mut right = vec![*row.last().unwrap()];
    while row.len() > 1 {
        row = row.windows(2).map(|v| v[0] * 0.5 + v[1] * 0.5).collect();
        left.push(row[0]);
        right.push(*row.last().unwrap());
    }
    right.reverse();
    (left, right)
}

/// Pick a geometric support seam rather than an unrelated authored vertex.
/// Bernstein derivative hulls isolate native projection extrema; the chosen
/// parameter cuts the original exact curve, even when the extremum parameter
/// itself requires numerical root isolation.
fn support_seam(
    segments: &mut Vec<Segment>,
    base: Point3,
    u: Vector3,
    v: Vector3,
    work: &mut usize,
) -> Result<()> {
    let mut best: Option<(usize, f64, Vector2)> = None;
    for (index, segment) in segments.iter().enumerate() {
        let bounds = derivative_numerator(&segment.curve, work)?;
        let coefficients = bounds
            .coefficients
            .iter()
            .map(|d| d.dot(u))
            .collect::<Vec<_>>();
        if coefficients.iter().any(|v| !v.is_finite()) {
            return Err(invalid("A native support derivative is not finite"));
        }
        let mut parameters = vec![0., 1.];
        let mut pending = vec![(coefficients, 0., 1., 0usize)];
        while let Some((coefficients, a, b, depth)) = pending.pop() {
            charge(work, coefficients.len().pow(2))?;
            let low = coefficients.iter().copied().fold(f64::INFINITY, f64::min);
            let high = coefficients
                .iter()
                .copied()
                .fold(f64::NEG_INFINITY, f64::max);
            // A nonnegative/nonpositive Bernstein polynomial is monotone.
            // Its interval ends also retain extrema exactly on dyadic splits.
            if low >= 0. || high <= 0. {
                parameters.extend([a, b]);
                continue;
            }
            if low.abs().max(high.abs()) <= bounds.uncertainty {
                parameters.push((a + b) * 0.5);
                continue;
            }
            if depth >= 44 || b - a <= PARTITION_EPS {
                parameters.push((a + b) * 0.5);
                continue;
            }
            let (left, right) = split_scalar(&coefficients);
            let middle = (a + b) * 0.5;
            pending.push((right, middle, b, depth + 1));
            pending.push((left, a, middle, depth + 1));
        }
        for t in parameters {
            charge(work, segment.curve.control_points().len().pow(2))?;
            let p = point(segment.curve.subs(t))? - base;
            let projected = Vector2::new(p.dot(u), p.dot(v));
            if ![projected.x, projected.y].iter().all(|v| v.is_finite()) {
                return Err(invalid(
                    "A native support seam exceeds finite projection bounds",
                ));
            }
            let replace = best.as_ref().is_none_or(|(_, _, previous)| {
                projected.x.total_cmp(&previous.x).is_gt()
                    || (projected.x == previous.x && projected.y.total_cmp(&previous.y).is_gt())
            });
            if replace {
                best = Some((index, t, projected));
            }
        }
    }
    let (index, t, _) =
        best.ok_or_else(|| invalid("A native contour has no finite support seam"))?;
    if t <= PARTITION_EPS {
        segments.rotate_left(index);
    } else if t >= 1. - PARTITION_EPS {
        let next = (index + 1) % segments.len();
        segments.rotate_left(next);
    } else {
        let (left, right) = segments[index].split(t)?;
        segments[index] = left;
        segments.insert(index + 1, right);
        segments.rotate_left(index + 1);
    }
    Ok(())
}

/// Refine the *native* carrier before allocating a nonradial perimeter
/// coordinate. Long distant control arms are poor perimeter measures. The
/// chord/control-polygon contrast supplies a bounded geometric refinement
/// criterion; it chooses correspondence and never replaces the curve by lines.
fn perimeter_spans(segments: &[Segment], work: &mut usize) -> Result<Vec<Segment>> {
    let mut pending = segments
        .iter()
        .cloned()
        .map(|s| (s, 0usize))
        .rev()
        .collect::<Vec<_>>();
    let mut result = Vec::new();
    while let Some((segment, depth)) = pending.pop() {
        charge(work, segment.curve.control_points().len().pow(2))?;
        let points = segment
            .curve
            .control_points()
            .iter()
            .map(|h| point(*h))
            .collect::<Result<Vec<_>>>()?;
        let polygon = points.windows(2).map(|p| p[0].distance(p[1])).sum::<f64>();
        let chord = points[0].distance(*points.last().unwrap());
        if !polygon.is_finite() || !chord.is_finite() {
            return Err(invalid(
                "A native perimeter span exceeds finite length bounds",
            ));
        }
        if polygon - chord <= EPS.max(polygon * 0.005) {
            result.push(segment);
            continue;
        }
        if depth >= 16 || result.len() + pending.len() + 2 > MAX_PIECES {
            return Err(unsupported(
                "the native perimeter refinement exceeds its bounded section budget",
            ));
        }
        let (a, b) = segment.split(0.5)?;
        pending.push((b, depth + 1));
        pending.push((a, depth + 1));
    }
    Ok(result)
}

fn radial_spans(
    segments: &[Segment],
    center: Point3,
    u: Vector3,
    v: Vector3,
) -> Result<Vec<Segment>> {
    let mut pending = segments
        .iter()
        .cloned()
        .map(|s| (s, 0usize))
        .rev()
        .collect::<Vec<_>>();
    let mut result = Vec::new();
    while let Some((s, depth)) = pending.pop() {
        let a = angle(s.front.point(), center, u, v);
        let span = (angle(s.back.point(), center, u, v) - a).rem_euclid(std::f64::consts::TAU);
        let mid = a + span / 2.;
        let axis = u * mid.cos() + v * mid.sin();
        let bounded = span > PARTITION_EPS
            && span < std::f64::consts::PI - 1e-8
            && s.curve
                .control_points()
                .iter()
                .map(|h| point(*h))
                .collect::<Result<Vec<_>>>()?
                .iter()
                .all(|p| (*p - center).dot(axis) > EPS);
        if bounded {
            result.push(s);
        } else {
            if depth >= 16 || result.len() + pending.len() + 2 > MAX_PIECES {
                return Err(unsupported(
                    "a native radial contour needs a finer exact angular partition than the bounded carrier supports",
                ));
            }
            let (a, b) = s.split(0.5)?;
            pending.push((b, depth + 1));
            pending.push((a, depth + 1));
        }
    }
    Ok(result)
}

/// All profiles are partitioned together so interior sections retain the same
/// native edge and vertex IDs in both adjoining strips.
pub(crate) fn prepare(wires: &[Wire], anchors: &[Option<Point3>]) -> Result<Prepared> {
    if wires.len() < 2 || wires.len() > 256 || anchors.len() != wires.len() {
        return Err(invalid(
            "Correspondence needs 2–256 ordered profiles and their connector selections",
        ));
    }
    let closed = wires[0].is_closed();
    if wires.iter().any(|w| w.is_closed() != closed) {
        return Err(invalid("Profiles must all be open or all closed"));
    }
    let mut source_controls = 0usize;
    for (wire, anchor) in wires.iter().zip(anchors) {
        if let Some(anchor) = anchor {
            let vertices = wire
                .vertex_iter()
                .filter(|v| v.point().distance(*anchor) <= EPS)
                .map(|v| v.id())
                .collect::<std::collections::HashSet<_>>();
            if vertices.len() != 1 {
                return Err(unsupported(
                    "a selected connector must identify exactly one original native contour vertex",
                ));
            }
        }
        for edge in wire {
            let n = match edge.curve() {
                Curve::Line(_) => 2,
                Curve::BsplineCurve(c) => c.control_points().len(),
                Curve::NurbsCurve(c) => c.control_points().len(),
                _ => {
                    return Err(unsupported(
                        "only native lines and rational/polynomial section boundaries are supported",
                    ));
                }
            };
            source_controls = source_controls
                .checked_add(n)
                .ok_or_else(|| unsupported("the native source control budget overflowed"))?;
            if source_controls > MAX_TOTAL {
                return Err(unsupported(
                    "the complete native section input exceeds the loft control budget",
                ));
            }
        }
    }
    let mut all = wires.iter().map(segments).collect::<Result<Vec<_>>>()?;
    let mut centers = Vec::new();
    let mut normals = Vec::new();
    let mut bases = Vec::new();
    let mut radial = closed;
    let mut previous_ends: Option<(Vector3, Vector3)> = None;
    let mut work = 0;
    for (index, segments) in all.iter_mut().enumerate() {
        let (base, mut normal, u, mut v) = frame(segments, normals.last().copied())?;
        if closed {
            if winding_sign(segments, base, normal, &mut work)? < 0. {
                if index == 0 {
                    // Preserve the first contour's native topological sense,
                    // including holes and negative-axis solid loft sections.
                    normal = -normal;
                    v = -v;
                } else {
                    reverse(segments);
                }
            }
        } else if let Some((a, b)) = previous_ends {
            let c = segments[0].front.point() - base;
            let d = segments.last().unwrap().back.point() - base;
            if (a - d).magnitude2() + (b - c).magnitude2()
                < (a - c).magnitude2() + (b - d).magnitude2()
            {
                reverse(segments);
            }
        }
        previous_ends = Some((
            segments[0].front.point() - base,
            segments.last().unwrap().back.point() - base,
        ));
        let center = if closed {
            radial_center(segments, base, u, v, &mut work)?
        } else {
            None
        };
        radial &= center.is_some();
        centers.push(center.unwrap_or(base));
        normals.push(normal);
        bases.push((u, v));
    }
    if radial {
        for (index, segments) in all.iter_mut().enumerate() {
            let (u, v) = bases[index];
            let center = centers[index];
            *segments = radial_spans(segments, center, u, v)?;
            let target = if let Some(anchor) = anchors[index] {
                let vertices = segments
                    .iter()
                    .filter(|s| s.front.point().distance(anchor) <= EPS)
                    .count();
                if vertices != 1 {
                    return Err(unsupported(
                        "a selected connector must resolve to one actual contour vertex",
                    ));
                }
                angle(anchor, center, u, v)
            } else {
                0.
            };
            radial_seam(segments, center, u, v, target)?;
        }
    } else {
        // An actual geometric support point fixes the common seam; native
        // perimeter refinement reduces control-layout-dependent phase drift.
        for (index, segments) in all.iter_mut().enumerate() {
            if !closed {
                *segments = perimeter_spans(segments, &mut work)?;
                continue;
            }
            let (u, v) = bases[index];
            let center = centers[index];
            if let Some(anchor) = anchors[index] {
                let starts = segments
                    .iter()
                    .enumerate()
                    .filter(|(_, s)| s.front.point().distance(anchor) <= EPS)
                    .map(|(i, _)| i)
                    .collect::<Vec<_>>();
                if starts.len() != 1 {
                    return Err(unsupported(
                        "a selected connector must resolve to one actual contour vertex",
                    ));
                }
                segments.rotate_left(starts[0]);
            } else {
                support_seam(segments, center, u, v, &mut work)?;
            }
            *segments = perimeter_spans(segments, &mut work)?;
        }
    }
    let mut partitions = Vec::new();
    for (index, segments) in all.iter().enumerate() {
        let mut p = vec![0.];
        if radial {
            let (u, v) = bases[index];
            let origin = angle(segments[0].front.point(), centers[index], u, v);
            let mut progress = 0.;
            for s in segments {
                progress += (angle(s.back.point(), centers[index], u, v)
                    - angle(s.front.point(), centers[index], u, v))
                .rem_euclid(std::f64::consts::TAU);
                p.push(progress / std::f64::consts::TAU);
            }
            if (progress - std::f64::consts::TAU).abs() > 1e-6 || !origin.is_finite() {
                return Err(invalid(
                    "The native contour winds more than once around its correspondence center",
                ));
            }
        } else {
            for s in segments {
                let h = s
                    .curve
                    .control_points()
                    .iter()
                    .map(|p| point(*p))
                    .collect::<Result<Vec<_>>>()?;
                let polygon = h.windows(2).map(|p| p[0].distance(p[1])).sum::<f64>();
                let length = polygon * 0.5 + h[0].distance(*h.last().unwrap()) * 0.5;
                let total = p.last().unwrap() + length;
                if !length.is_finite() || length <= EPS || !total.is_finite() {
                    return Err(invalid(
                        "A native profile segment has no finite geometric extent",
                    ));
                }
                p.push(total);
            }
            let total = *p.last().unwrap();
            for value in &mut p {
                *value /= total;
            }
        }
        *p.last_mut().unwrap() = 1.;
        partitions.push(p);
    }
    for p in &partitions {
        if p.windows(2).any(|p| p[1] - p[0] <= PARTITION_EPS) {
            return Err(unsupported(
                "distinct native corners have an ill-conditioned correspondence interval",
            ));
        }
    }
    let mut candidates = partitions
        .iter()
        .enumerate()
        .flat_map(|(i, p)| p.iter().copied().map(move |v| (v, i)))
        .collect::<Vec<_>>();
    candidates.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut common: Vec<f64> = Vec::new();
    let mut last_profile = None;
    for (value, profile) in candidates {
        if let Some(last) = common.last() {
            let gap = value - *last;
            if gap <= PARTITION_EPS {
                if last_profile == Some(profile) && gap > 0. {
                    return Err(unsupported(
                        "distinct source corners cannot be merged into one correspondence partition",
                    ));
                }
                continue;
            }
            if gap < 1e-10 {
                return Err(unsupported(
                    "nearly coincident profile partitions are ill-conditioned; adjust the section or its connector",
                ));
            }
        }
        common.push(value);
        last_profile = Some(profile);
    }
    if common.len() > MAX_PIECES || (common.len() - 1) * wires.len() > MAX_TOTAL {
        return Err(unsupported(
            "the exact common profile partition exceeds the bounded loft budget",
        ));
    }
    let mut result = Vec::new();
    let mut provenance = Vec::new();
    for (index, segments) in all.iter().enumerate() {
        let p = &partitions[index];
        let mut output = Vec::new();
        let mut origins = HashMap::new();
        let mut previous: Option<Vertex> = None;
        let mut first: Option<Vertex> = None;
        for span in common.windows(2) {
            charge(&mut work, segments.len())?;
            let member = (0..segments.len())
                .find(|i| span[0] >= p[*i] - PARTITION_EPS && span[1] <= p[*i + 1] + PARTITION_EPS)
                .ok_or_else(|| invalid("A common partition crossed an unresolved native corner"))?;
            let source = &segments[member];
            if radial {
                charge(&mut work, (source.curve.degree() + 1).pow(2) * 128)?;
            }
            let parameter = |value: f64| -> Result<f64> {
                if (value - p[member]).abs() <= PARTITION_EPS {
                    return Ok(0.);
                }
                if (value - p[member + 1]).abs() <= PARTITION_EPS {
                    return Ok(1.);
                }
                if radial {
                    let (u, v) = bases[index];
                    let seam = angle(segments[0].front.point(), centers[index], u, v);
                    angle_parameter(
                        source,
                        centers[index],
                        u,
                        v,
                        (seam + value * std::f64::consts::TAU).rem_euclid(std::f64::consts::TAU),
                    )
                } else {
                    Ok((value - p[member]) / (p[member + 1] - p[member]))
                }
            };
            let a = parameter(span[0])?;
            let b = parameter(span[1])?;
            if b - a <= 1e-12 {
                return Err(invalid(
                    "A common loft partition collapses on a native boundary",
                ));
            }
            let mut curve = source.curve.clone();
            if b < 1. - PARTITION_EPS {
                curve.cut(b);
            }
            if a > PARTITION_EPS {
                curve = curve.cut(a);
            }
            curve.knot_normalize();
            let front = if let Some(previous) = &previous {
                previous.clone()
            } else if a <= PARTITION_EPS {
                source.front.clone()
            } else {
                Vertex::new(point(source.curve.subs(a))?)
            };
            let back = if span[1] >= 1. - PARTITION_EPS && closed {
                first.clone().unwrap_or_else(|| front.clone())
            } else if b >= 1. - PARTITION_EPS {
                source.back.clone()
            } else {
                Vertex::new(point(source.curve.subs(b))?)
            };
            if first.is_none() {
                first = Some(front.clone());
            }
            let edge = Edge::try_new(&front, &back, Curve::NurbsCurve(NurbsCurve::new(curve)))
                .map_err(invalid)?;
            origins.insert(
                edge.id(),
                Piece {
                    source: source.source,
                    interval: [
                        source.interval[0] + (source.interval[1] - source.interval[0]) * a,
                        source.interval[0] + (source.interval[1] - source.interval[0]) * b,
                    ],
                },
            );
            previous = Some(back);
            output.push(edge);
        }
        result.push(Wire::from(output));
        provenance.push(origins);
    }
    let prepared = Prepared {
        wires: result,
        pieces: provenance,
        centers,
        normals,
        radial,
        proof_work: Cell::new(work),
        ruled: vec![false; wires.len() - 1],
    };
    Ok(prepared)
}

/// Multiply homogeneous Bezier carriers by the other's positive denominator.
/// Unlike projective interpolation, this preserves Euclidean boundary blending
/// when one section is rational (a circle) and the other is polynomial (a line).
pub(crate) fn compatible(
    a: &Curve,
    b: &Curve,
) -> Result<(BsplineCurve<Vector4>, BsplineCurve<Vector4>)> {
    let mut a = carrier(a)?;
    let mut b = carrier(b)?;
    let mut knots = a
        .knot_vector()
        .iter()
        .chain(b.knot_vector())
        .copied()
        .collect::<Vec<_>>();
    knots.sort_by(f64::total_cmp);
    knots.dedup();
    for knot in &knots {
        let ca = a.knot_vector().iter().filter(|k| **k == *knot).count();
        let cb = b.knot_vector().iter().filter(|k| **k == *knot).count();
        for _ in ca..a.degree() {
            a.add_knot(*knot);
        }
        for _ in cb..b.degree() {
            b.add_knot(*knot);
        }
    }
    let aa = a.bezier_decomposition();
    let bb = b.bezier_decomposition();
    if aa.len() != bb.len() {
        return Err(invalid(
            "Native rational spans could not be exactly synchronized",
        ));
    }
    let degree = a.degree() + b.degree();
    let mut ac = Vec::new();
    let mut bc = Vec::new();
    let mut out_knots = vec![0.; degree + 1];
    for (index, (a, b)) in aa.iter().zip(&bb).enumerate() {
        let mut left = vec![Vector4::zero(); degree + 1];
        let mut right = left.clone();
        for (i, pa) in a.control_points().iter().enumerate() {
            for (j, pb) in b.control_points().iter().enumerate() {
                let f = product_factor(a.degree(), i, b.degree(), j);
                let w = pa.w * pb.w * f;
                left[i + j] += Vector4::new(pa.x * pb.w * f, pa.y * pb.w * f, pa.z * pb.w * f, w);
                right[i + j] += Vector4::new(pb.x * pa.w * f, pb.y * pa.w * f, pb.z * pa.w * f, w);
            }
        }
        if index > 0 {
            left.remove(0);
            right.remove(0);
            out_knots.extend(std::iter::repeat_n(a.range_tuple().0, degree));
        }
        ac.extend(left);
        bc.extend(right);
    }
    out_knots.extend(std::iter::repeat_n(1., degree + 1));
    if ac.len() > MAX_PIECES {
        return Err(unsupported(
            "the exact rational common denominator exceeds the carrier budget",
        ));
    }
    for (a, b) in ac.iter().zip(&bc) {
        point(*a)?;
        point(*b)?;
        if a.w != b.w {
            return Err(invalid("The exact common rational denominator was lost"));
        }
    }
    Ok((
        BsplineCurve::new(KnotVector::from(out_knots.clone()), ac),
        BsplineCurve::new(KnotVector::from(out_knots), bc),
    ))
}

pub(crate) fn skin(
    a: &Wire,
    b: &Wire,
    start: Option<Vector3>,
    end: Option<Vector3>,
) -> Result<Shell> {
    if a.len() != b.len() {
        return Err(invalid(
            "Native loft profiles were not partitioned together",
        ));
    }
    let mut connectors: HashMap<(VertexId, VertexId), Edge> = HashMap::new();
    let mut shell = Shell::new();
    for (lower, upper) in a.iter().zip(b) {
        let (a, b) = compatible(&lower.oriented_curve(), &upper.oriented_curve())?;
        let ruled = start.is_none() && end.is_none();
        let net = a
            .control_points()
            .iter()
            .zip(b.control_points())
            .map(|(a, b)| {
                if ruled {
                    return Ok(vec![*a, *b]);
                }
                let p = point(*a)?;
                let q = point(*b)?;
                let first = start.unwrap_or(q - p);
                let last = end.unwrap_or(q - p);
                Ok(vec![
                    *a,
                    (p + first / 3.).to_homogeneous() * a.w,
                    (q - last / 3.).to_homogeneous() * b.w,
                    *b,
                ])
            })
            .collect::<Result<Vec<_>>>()?;
        let surface = NurbsSurface::new(BsplineSurface::new(
            (
                a.knot_vector().clone(),
                KnotVector::bezier_knot(if ruled { 1 } else { 3 }),
            ),
            net,
        ));
        let mut connector = |a: &Vertex, b: &Vertex, index: usize| -> Result<Edge> {
            if let Some(edge) = connectors.get(&(a.id(), b.id())) {
                return Ok(edge.clone());
            }
            let edge =
                Edge::try_new(a, b, Curve::NurbsCurve(surface.curve_v(index))).map_err(invalid)?;
            connectors.insert((a.id(), b.id()), edge.clone());
            Ok(edge)
        };
        let left = connector(lower.front(), upper.front(), 0)?;
        let right = connector(
            lower.back(),
            upper.back(),
            surface.control_points().len() - 1,
        )?;
        shell.push(
            Face::try_new(
                vec![Wire::from(vec![
                    lower.clone(),
                    right,
                    upper.inverse(),
                    left.inverse(),
                ])],
                Surface::NurbsSurface(surface),
            )
            .map_err(invalid)?,
        );
    }
    Ok(shell)
}

/// A positive native Bernstein angular derivative throughout the complete
/// Euclidean section blend proves all intermediate closed contours simple.
pub(crate) fn prove_radial_pair(prepared: &Prepared, index: usize) -> Result<bool> {
    if !prepared.radial
        || index + 1 >= prepared.wires.len()
        || !prepared.wires[index].is_closed()
        || prepared.normals[index]
            .cross(prepared.normals[index + 1])
            .magnitude()
            > 1e-9
    {
        return Ok(false);
    }
    let normal = prepared.normals[index];
    let ca = prepared.centers[index];
    let cb = prepared.centers[index + 1];
    if (cb - ca).dot(normal).abs() <= EPS {
        return Ok(false);
    }
    let mut work = prepared.proof_work.get();
    for (a, b) in prepared.wires[index].iter().zip(&prepared.wires[index + 1]) {
        let (a, b) = compatible(&a.oriented_curve(), &b.oriented_curve())?;
        for (a, b) in a
            .bezier_decomposition()
            .iter()
            .zip(b.bezier_decomposition())
        {
            let p = a.degree();
            let aa = a
                .control_points()
                .iter()
                .map(|h| Vector3::new(h.x, h.y, h.z) - ca.to_vec() * h.w)
                .collect::<Vec<_>>();
            let bb = b
                .control_points()
                .iter()
                .map(|h| Vector3::new(h.x, h.y, h.z) - cb.to_vec() * h.w)
                .collect::<Vec<_>>();
            for k in 0..2 * p {
                charge(&mut work, p + 1)?;
                prepared.proof_work.set(work);
                let mut coefficients = [0.; 3];
                for i in 0..=p {
                    let Some(j) = k.checked_sub(i).filter(|j| *j < p) else {
                        continue;
                    };
                    let f = product_factor(p, i, p - 1, j) * p as f64;
                    let da = aa[j + 1] - aa[j];
                    let db = bb[j + 1] - bb[j];
                    coefficients[0] += f * aa[i].cross(da).dot(normal);
                    coefficients[1] += f * (aa[i].cross(db) + bb[i].cross(da)).dot(normal) / 2.;
                    coefficients[2] += f * bb[i].cross(db).dot(normal);
                }
                if coefficients
                    .iter()
                    .any(|v| !v.is_finite() || *v <= EPS * EPS)
                {
                    return Ok(false);
                }
            }
        }
    }
    Ok(true)
}

/// Bernstein coefficients of the rational derivative numerator X'W-XW'.
/// Translation of X by an exact constant times W does not change this
/// numerator; centering its arithmetic avoids unrelated world-coordinate size.
struct DerivativeBounds {
    coefficients: Vec<Vector3>,
    uncertainty: f64,
}
fn derivative_numerator(
    curve: &BsplineCurve<Vector4>,
    work: &mut usize,
) -> Result<DerivativeBounds> {
    let degree = curve.degree();
    if curve.control_points().len() != degree + 1 {
        return Err(unsupported(
            "ruled chart certification requires an exact native Bezier span",
        ));
    }
    let origin = point(curve.control_points()[0])?.to_vec();
    let controls = curve
        .control_points()
        .iter()
        .map(|h| (Vector3::new(h.x, h.y, h.z) - origin * h.w, h.w))
        .collect::<Vec<_>>();
    let input_majorant = curve
        .control_points()
        .iter()
        .map(|h| {
            [
                h.x.abs() + origin.x.abs() * h.w,
                h.y.abs() + origin.y.abs() * h.w,
                h.z.abs() + origin.z.abs() * h.w,
            ]
            .into_iter()
            .fold(0f64, f64::max)
        })
        .fold(0f64, f64::max);
    let weight_majorant = curve
        .control_points()
        .iter()
        .map(|h| h.w)
        .fold(0f64, f64::max);
    // Covers centering, both differentiated products, binomial factors and
    // summation before cancellation; a small resulting derivative is not used
    // to estimate its own arithmetic uncertainty.
    let uncertainty =
        256. * f64::EPSILON * (degree + 1).pow(2) as f64 * input_majorant * weight_majorant;
    if !uncertainty.is_finite() {
        return Err(invalid(
            "The ruled derivative arithmetic majorant is not finite",
        ));
    }
    let mut result = vec![Vector3::zero(); 2 * degree];
    for (k, value) in result.iter_mut().enumerate() {
        charge(work, degree + 1)?;
        for i in 0..degree {
            let Some(j) = k.checked_sub(i).filter(|j| *j <= degree) else {
                continue;
            };
            let dx = (controls[i + 1].0 - controls[i].0) * degree as f64;
            let dw = (controls[i + 1].1 - controls[i].1) * degree as f64;
            let factor = product_factor(degree - 1, i, degree, j);
            *value += (dx * controls[j].1 - controls[j].0 * dw) * factor;
        }
        if ![value.x, value.y, value.z].iter().all(|v| v.is_finite()) {
            return Err(invalid(
                "A ruled chart derivative exceeds finite coefficient bounds",
            ));
        }
    }
    Ok(DerivativeBounds {
        coefficients: result,
        uncertainty,
    })
}

/// Find one fixed contour coordinate whose complete Bernstein derivative hull
/// is positive. Angular bounds choose the coordinate; projection of every
/// original coefficient, rather than an angular sample, establishes the proof.
fn contour_coordinate(derivatives: &[Vector3], normal: Vector3) -> Option<Vector3> {
    let basis = if normal.x.abs() < 0.9 {
        Vector3::unit_x()
    } else {
        Vector3::unit_y()
    };
    let u = (basis - normal * basis.dot(normal)).normalize();
    let v = normal.cross(u);
    let mut angles = Vec::with_capacity(derivatives.len());
    for d in derivatives {
        let x = d.dot(u);
        let y = d.dot(v);
        if !x.is_finite() || !y.is_finite() || x.hypot(y) <= 0. {
            return None;
        }
        angles.push(y.atan2(x).rem_euclid(std::f64::consts::TAU));
    }
    if angles.is_empty() {
        return None;
    }
    angles.sort_by(f64::total_cmp);
    let (index, gap) = (0..angles.len())
        .map(|i| {
            let next = if i + 1 == angles.len() {
                angles[0] + std::f64::consts::TAU
            } else {
                angles[i + 1]
            };
            (i, next - angles[i])
        })
        .max_by(|a, b| a.1.total_cmp(&b.1))?;
    let span = std::f64::consts::TAU - gap;
    if span >= std::f64::consts::PI - 1e-10 {
        return None;
    }
    let start = angles[(index + 1) % angles.len()];
    let middle = start + span * 0.5;
    Some(u * middle.cos() + v * middle.sin())
}

/// A fixed projected Jacobian has positive principal minors throughout one
/// complete ruled chart. This P-matrix certificate proves global injection on
/// its rectangular parameter domain without requiring the projection's
/// symmetric part positive: large legitimate loft shear is permitted.
///
/// With the common denominator W, S_u has numerator (1-t)D_A+tD_B and
/// S_t has numerator X_B-X_A. Bernstein hulls therefore bound all four
/// entries, including small native deviations from parallel planes. The
/// determinant minorant a_min*d_min-|b|_max*|c|_max accounts for those deviations;
/// no tolerance comparison is used to assume an entry exactly zero.
fn ruled_chart(a: &Curve, b: &Curve, normal: Option<Vector3>, work: &mut usize) -> Result<bool> {
    let (a, b) = compatible(a, b)?;
    let aa = a.bezier_decomposition();
    let bb = b.bezier_decomposition();
    if aa.len() != 1 || bb.len() != 1 {
        return Ok(false);
    }
    let da = derivative_numerator(&aa[0], work)?;
    let db = derivative_numerator(&bb[0], work)?;
    let derivative_error = da.uncertainty.max(db.uncertainty) * 3.;
    let mut derivatives = da.coefficients;
    derivatives.extend(db.coefficients);
    let mut frames = Vec::new();
    if let Some(normal) = normal
        && let Some(contour) = contour_coordinate(&derivatives, normal)
    {
        frames.push((contour, normal));
    }
    // A chart does not need parallel section planes. With a common positive
    // denominator its two derivative numerators are convex combinations of
    // these native coefficient vectors. Candidate dual frames come from
    // actual rational derivatives; every candidate is checked against the
    // complete coefficient hull, not just the points choosing the frame.
    for parameter in [0.5, 0.25, 0.75] {
        let ca = NurbsCurve::new(a.clone());
        let cb = NurbsCurve::new(b.clone());
        let tangent = (ca.der(parameter) + cb.der(parameter)) * 0.5;
        let ruling = cb.subs(parameter) - ca.subs(parameter);
        let ta = tangent.magnitude();
        let tb = ruling.magnitude();
        if !ta.is_finite() || !tb.is_finite() || ta <= 1e-12 || tb <= 1e-12 {
            continue;
        }
        let x = tangent / ta;
        let y = ruling / tb;
        let cosine = x.dot(y);
        let determinant = 1. - cosine * cosine;
        if determinant.is_finite() && determinant > 1e-10 {
            frames.push((
                (x - y * cosine) / determinant,
                (y - x * cosine) / determinant,
            ));
        }
    }
    for (contour, normal) in frames {
        charge(
            work,
            (derivatives.len() + aa[0].control_points().len()) * 32,
        )?;
        if ruled_minors(
            &aa[0],
            &bb[0],
            &derivatives,
            derivative_error,
            contour,
            normal,
        )? {
            return Ok(true);
        }
    }
    Ok(false)
}

fn ruled_minors(
    a: &BsplineCurve<Vector4>,
    b: &BsplineCurve<Vector4>,
    derivatives: &[Vector3],
    derivative_error: f64,
    contour: Vector3,
    normal: Vector3,
) -> Result<bool> {
    let mut amin = f64::INFINITY;
    let mut amax = 0f64;
    let mut cmax = 0f64;
    for d in derivatives {
        let a = d.dot(contour);
        let c = d.dot(normal);
        amin = amin.min(a);
        amax = amax.max(a.abs());
        cmax = cmax.max(c.abs());
    }
    let mut dmin = f64::INFINITY;
    let mut dmax = 0f64;
    let mut bmax = 0f64;
    for (a, b) in a.control_points().iter().zip(b.control_points()) {
        let difference = Vector3::new(b.x - a.x, b.y - a.y, b.z - a.z);
        let d = difference.dot(normal);
        let b = difference.dot(contour);
        dmin = dmin.min(d);
        dmax = dmax.max(d.abs());
        bmax = bmax.max(b.abs());
    }
    let difference_error = a
        .control_points()
        .iter()
        .chain(b.control_points())
        .map(|h| {
            [h.x.abs(), h.y.abs(), h.z.abs()]
                .into_iter()
                .fold(0f64, f64::max)
        })
        .fold(0f64, f64::max)
        * 256.
        * f64::EPSILON;
    amin -= derivative_error * contour.magnitude();
    amax += derivative_error * contour.magnitude();
    cmax += derivative_error * normal.magnitude();
    dmin -= difference_error * normal.magnitude();
    dmax += difference_error * normal.magnitude();
    bmax += difference_error * contour.magnitude();
    let positive = amin * dmin;
    let off = bmax * cmax;
    let majorant = amax * dmax + off;
    if ![amin, amax, cmax, dmin, dmax, bmax, positive, off, majorant]
        .iter()
        .all(|v| v.is_finite())
    {
        return Err(invalid(
            "The native ruled projection exceeds finite arithmetic bounds",
        ));
    }
    const MARGIN: f64 = 1e-10;
    Ok(amin > MARGIN * amax && dmin > MARGIN * dmax && positive - off > MARGIN * majorant)
}

fn ruled_normal(prepared: &Prepared, index: usize) -> Option<Vector3> {
    let normal = *prepared.normals.get(index)?;
    let next = *prepared.normals.get(index + 1)?;
    // This only selects the specialized proof frame. The actual carrier's
    // nonzero off-axis derivatives are included in ruled_chart's determinant.
    if normal.cross(next).magnitude() > 1e-8 {
        return None;
    }
    let height = (prepared.centers[index + 1] - prepared.centers[index]).dot(normal);
    (height.is_finite() && height.abs() > EPS).then_some(normal * height.signum())
}

/// Long concave boundaries can need multiple injective coordinate charts even
/// though the complete contour is simple. Split the *native section wires*
/// and actual loft faces, not merely the proof rectangles. All section rows
/// split the same column so adjacent loft pairs retain identical shared edges;
/// directed original member intervals follow every split.
pub(crate) fn refine_ruled_charts(prepared: &mut Prepared) -> Result<()> {
    let normals = (0..prepared.wires.len() - 1)
        .map(|i| ruled_normal(prepared, i))
        .collect::<Vec<_>>();
    let mut columns = Vec::new();
    for column in 0..prepared.wires[0].len() {
        let row = prepared
            .wires
            .iter()
            .enumerate()
            .map(|(index, wire)| {
                let edge = &wire[column];
                let piece = &prepared.pieces[index][&edge.id()];
                Ok(Segment {
                    curve: carrier(&edge.oriented_curve())?,
                    source: piece.source,
                    interval: piece.interval,
                    front: edge.front().clone(),
                    back: edge.back().clone(),
                })
            })
            .collect::<Result<Vec<_>>>()?;
        columns.push((row, 0usize));
    }
    columns.reverse();
    let mut accepted = Vec::new();
    let mut work = prepared.proof_work.get();
    let mut certified = vec![true; normals.len()];
    while let Some((column, depth)) = columns.pop() {
        let mut regular = true;
        let mut failures = Vec::new();
        for (index, normal) in normals.iter().enumerate() {
            let a = Curve::NurbsCurve(NurbsCurve::new(column[index].curve.clone()));
            let b = Curve::NurbsCurve(NurbsCurve::new(column[index + 1].curve.clone()));
            if !ruled_chart(&a, &b, *normal, &mut work)? {
                regular = false;
                failures.push(index);
            }
        }
        if regular {
            accepted.push(column);
            continue;
        }
        if depth >= 12 {
            for index in failures {
                certified[index] = false;
            }
            accepted.push(column);
            continue;
        }
        if accepted.len() + columns.len() + 2 > MAX_PIECES
            || (accepted.len() + columns.len() + 2) * prepared.wires.len() > MAX_TOTAL
        {
            return Err(unsupported(
                "the native ruled chart refinement exceeds the shared section-piece budget",
            ));
        }
        let mut left = Vec::new();
        let mut right = Vec::new();
        for segment in column {
            let (a, b) = segment.split(0.5)?;
            left.push(a);
            right.push(b);
        }
        columns.push((right, depth + 1));
        columns.push((left, depth + 1));
    }
    let mut wires = Vec::new();
    let mut pieces = Vec::new();
    for row in 0..prepared.wires.len() {
        let mut edges = Vec::new();
        let mut provenance = HashMap::new();
        for column in &accepted {
            let s = &column[row];
            let edge = Edge::try_new(
                &s.front,
                &s.back,
                Curve::NurbsCurve(NurbsCurve::new(s.curve.clone())),
            )
            .map_err(invalid)?;
            provenance.insert(
                edge.id(),
                Piece {
                    source: s.source,
                    interval: s.interval,
                },
            );
            edges.push(edge);
        }
        wires.push(Wire::from(edges));
        pieces.push(provenance);
    }
    prepared.wires = wires;
    prepared.pieces = pieces;
    prepared.proof_work.set(work);
    prepared.ruled = certified;
    Ok(())
}

/// Certificate for every complete native face in an uninfluenced ruled pair.
/// This does not assert separation between different faces or section pairs.
pub(crate) fn prove_ruled_pair(prepared: &Prepared, index: usize) -> Result<bool> {
    if prepared.ruled.get(index) == Some(&true) {
        return Ok(true);
    }
    let normal = ruled_normal(prepared, index);
    let mut work = prepared.proof_work.get();
    for (a, b) in prepared.wires[index].iter().zip(&prepared.wires[index + 1]) {
        let regular = ruled_chart(&a.oriented_curve(), &b.oriented_curve(), normal, &mut work)?;
        prepared.proof_work.set(work);
        if !regular {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Until general inter-skin intersection is available, multiple ruled pairs
/// require genuinely disjoint slabs. A tolerance-flat control net is not an
/// exact separating plane: its two neighboring skins can overlap in that band.
/// Here exact projective coordinate equality proves a common native coordinate
/// plane, and outward station intervals establish strict consistent ordering.
pub(crate) fn ordered_ruled_sections(sections: &[Vec<Vector4>]) -> Result<()> {
    use crate::native_bounds::Interval;
    if sections.len() <= 2 {
        return Ok(());
    }
    for axis in 0..3 {
        if sections.iter().any(|controls| {
            controls.is_empty()
                || controls
                    .iter()
                    .any(|h| !separation::same_projective_coordinate(*h, controls[0], axis))
        }) {
            continue;
        }
        let levels = sections
            .iter()
            .map(|controls| {
                Interval::exact(controls[0][axis])?.div(Interval::exact(controls[0].w)?)
            })
            .collect::<Result<Vec<_>>>()?;
        let increasing = levels
            .windows(2)
            .all(|pair| pair[0].high + EPS < pair[1].low);
        let decreasing = levels
            .windows(2)
            .all(|pair| pair[1].high + EPS < pair[0].low);
        if increasing || decreasing {
            return Ok(());
        }
    }
    Err(unsupported(
        "the multi-pair native loft certificate requires exact ordered Front/Top/Right section planes; angled, tolerance-planar, overlapping or reversing skins need a complete inter-skin intersection proof",
    ))
}
