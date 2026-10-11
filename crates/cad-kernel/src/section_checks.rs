//! Analytic bounded line/arc contour checks for native wall offsets.
use cad_core::{CadError, Result, Sketch, SketchCurve};
use monstertruck::modeling::*;
const EPS: f64 = 1e-7;
#[derive(Clone)]
enum Segment {
    Line {
        a: Vector2,
        b: Vector2,
    },
    Arc {
        a: Vector2,
        b: Vector2,
        c: Vector2,
        r: f64,
        start: f64,
        sweep: f64,
    },
}
fn invalid(message: &str) -> CadError {
    CadError::Invalid(format!("Shell section: {message}"))
}
fn cross(a: Vector2, b: Vector2) -> f64 {
    a.x * b.y - a.y * b.x
}
fn conditioning(scale: f64) -> Result<f64> {
    let error = 32. * f64::EPSILON * scale;
    if !error.is_finite() || error > EPS * 0.25 {
        return Err(CadError::Unsupported("Shell section intersections exceed the millimetre tolerance at this coordinate/length scale".into()));
    }
    Ok(error)
}
fn cross_exact_zero(a: Vector2, b: Vector2) -> bool {
    let p = a.x * b.y;
    let q = a.y * b.x;
    p.is_finite() && q.is_finite() && p == q && a.x.mul_add(b.y, -p) == a.y.mul_add(b.x, -q)
}
impl Segment {
    fn ends(&self) -> (Vector2, Vector2) {
        match self {
            Self::Line { a, b } | Self::Arc { a, b, .. } => (*a, *b),
        }
    }
    fn at(&self, t: f64) -> Vector2 {
        match self {
            Self::Line { a, b } => a + (*b - *a) * t,
            Self::Arc {
                c, r, start, sweep, ..
            } => *c + Vector2::new((start + sweep * t).cos(), (start + sweep * t).sin()) * *r,
        }
    }
    fn parameter(&self, p: Vector2) -> Option<f64> {
        match self {
            Self::Line { a, b } => {
                let d = *b - *a;
                let length = d.magnitude();
                if !length.is_finite() || length <= EPS {
                    return None;
                }
                let unit = d / length;
                let t = (p - *a).dot(unit) / length;
                ((p - *a - unit * (t * length)).magnitude() <= EPS
                    && t >= -EPS / length
                    && t <= 1. + EPS / length)
                    .then_some(t.clamp(0., 1.))
            }
            Self::Arc {
                c, r, start, sweep, ..
            } => {
                let v = p - *c;
                if (v.magnitude() - r).abs() > EPS {
                    return None;
                }
                let angle = v.y.atan2(v.x);
                let mut progress = if *sweep > 0. {
                    (angle - start).rem_euclid(std::f64::consts::TAU)
                } else {
                    (start - angle).rem_euclid(std::f64::consts::TAU)
                };
                if (progress - std::f64::consts::TAU) * r >= -EPS {
                    progress = 0.;
                }
                (progress <= sweep.abs() + EPS / r)
                    .then_some((progress / sweep.abs()).clamp(0., 1.))
            }
        }
    }
    fn crossings(&self, p: Vector2) -> i32 {
        match self {
            Self::Line { a, b } => {
                if (a.y > p.y) == (b.y > p.y) {
                    return 0;
                }
                let t = (p.y - a.y) / (b.y - a.y);
                let x = a.x + (b.x - a.x) * t;
                if x > p.x {
                    if b.y > a.y { 1 } else { -1 }
                } else {
                    0
                }
            }
            Self::Arc {
                c, r, start, sweep, ..
            } => {
                let dy = (p.y - c.y) / r;
                if !(-1. ..=1.).contains(&dy) {
                    return 0;
                }
                let a = dy.asin();
                let mut winding = 0;
                for angle in [a, std::f64::consts::PI - a] {
                    let x = c.x + r * angle.cos();
                    let q = Vector2::new(x, p.y);
                    let Some(t) = self.parameter(q) else {
                        continue;
                    };
                    let derivative = (start + sweep * t).cos() * sweep;
                    if x <= p.x
                        || derivative.abs() < 1e-12
                        || t <= 1e-12 && derivative < 0.
                        || t >= 1. - 1e-12 && derivative > 0.
                    {
                        continue;
                    }
                    winding += if derivative > 0. { 1 } else { -1 };
                }
                winding
            }
        }
    }
}
fn intersections(a: &Segment, b: &Segment) -> Result<Vec<Vector2>> {
    let (a0, a1) = a.ends();
    let (b0, b1) = b.ends();
    conditioning(a0.magnitude() + a1.magnitude() + b0.magnitude() + b1.magnitude())?;
    let mut candidates = Vec::new();
    match (a, b) {
        (Segment::Line { a: a0, b: a1 }, Segment::Line { a: b0, b: b1 }) => {
            let d = *a1 - *a0;
            let e = *b1 - *b0;
            let (ld, le) = (d.magnitude(), e.magnitude());
            if !ld.is_finite() || !le.is_finite() || ld.min(le) <= EPS {
                return Err(invalid("A contour member is collapsed or nonfinite"));
            }
            let du = d / ld;
            let eu = e / le;
            let q = *b0 - *a0;
            let determinant = cross(du, eu);
            if determinant.abs() < 1e-12 {
                if !cross_exact_zero(d, e) {
                    return Err(CadError::Unsupported("Shell section has nonparallel lines whose intersection is too ill-conditioned to prove".into()));
                }
                conditioning(ld + le + q.magnitude())?;
                for point in [*a0, *a1, *b0, *b1] {
                    if a.parameter(point).is_some() && b.parameter(point).is_some() {
                        candidates.push(point);
                    }
                }
                if candidates
                    .iter()
                    .any(|a| candidates.iter().any(|b| (*a - *b).magnitude() > EPS))
                {
                    return Err(invalid("Straight section boundaries overlap"));
                }
            } else {
                conditioning((ld + le + q.magnitude()) / determinant.abs())?;
                candidates.push(*a0 + du * (cross(q, eu) / determinant));
            }
        }
        (Segment::Line { a: a0, b: a1 }, Segment::Arc { c, r, .. })
        | (Segment::Arc { c, r, .. }, Segment::Line { a: a0, b: a1 }) => {
            let d = *a1 - *a0;
            let f = *c - *a0;
            let length = d.magnitude();
            if !length.is_finite() || length <= EPS {
                return Err(invalid(
                    "Circular intersection exceeds the finite numeric range",
                ));
            }
            let error = conditioning(length + f.magnitude() + r)?;
            let unit = d / length;
            let distance = cross(f, unit).abs();
            let arc = if matches!(a, Segment::Arc { .. }) {
                a
            } else {
                b
            };
            let (arc_start, arc_end) = arc.ends();
            let shared = [*a0, *a1]
                .into_iter()
                .find(|point| *point == arc_start || *point == arc_end);
            if let Some(shared) = shared {
                // The arc is authored through this exact endpoint. Factor the
                // known root instead of subtracting near-equal squared terms.
                let other = -2. * unit.dot(shared - *c);
                candidates.extend([shared, shared + unit * other]);
            } else if distance <= r + error {
                let radicand = (r - distance) * (r + distance);
                let radial_error = error * (2. * r + error);
                if !radicand.is_finite()
                    || radial_error.sqrt() > EPS && radicand.abs() <= radial_error
                {
                    return Err(CadError::Unsupported("Shell section has a near-tangent line/circle intersection whose bounded contact is uncertain at the document tolerance".into()));
                }
                let root = radicand.max(0.).sqrt();
                let along = f.dot(unit);
                for offset in [-root, root] {
                    candidates.push(*a0 + unit * (along + offset));
                }
            }
        }
        (Segment::Arc { c: c0, r: r0, .. }, Segment::Arc { c: c1, r: r1, .. }) => {
            let delta = *c1 - *c0;
            let distance = delta.magnitude();
            if distance <= EPS && (r0 - r1).abs() <= EPS {
                let (a0, a1) = a.ends();
                let (b0, b1) = b.ends();
                for point in [a0, a1, b0, b1, a.at(0.5), b.at(0.5)] {
                    if a.parameter(point).is_some() && b.parameter(point).is_some() {
                        candidates.push(point);
                    }
                }
            } else if distance > EPS {
                let scale = distance.max(*r0).max(*r1);
                if !scale.is_finite() {
                    return Err(invalid(
                        "Circular intersection exceeds the finite numeric range",
                    ));
                }
                let error = conditioning((r0 + r1 + distance) * ((r0 + r1) / distance).max(1.))?;
                let (a0, a1) = a.ends();
                let (b0, b1) = b.ends();
                let shared = [a0, a1]
                    .into_iter()
                    .find(|point| *point == b0 || *point == b1);
                let unit = delta / distance;
                if let Some(shared) = shared {
                    let foot = *c0 + unit * (shared - *c0).dot(unit);
                    candidates.extend([shared, foot + (foot - shared)]);
                } else if distance <= r0 + r1 + error && distance >= (r0 - r1).abs() - error {
                    let d = distance / scale;
                    let ra = r0 / scale;
                    let rb = r1 / scale;
                    let x = (d + (ra - rb) * (ra + rb) / d) * 0.5;
                    let h2 = (ra - x) * (ra + x);
                    let physical_error = error * (2. * r0 + error);
                    if !h2.is_finite()
                        || physical_error.sqrt() > EPS && (h2 * scale) * scale <= physical_error
                    {
                        return Err(CadError::Unsupported("Shell section has a near-tangent circle pair whose bounded intersection cannot be proven at the document tolerance".into()));
                    }
                    let foot = *c0 + unit * (x * scale);
                    let side = Vector2::new(-unit.y, unit.x) * (h2.max(0.).sqrt() * scale);
                    candidates.extend([foot + side, foot - side]);
                }
            }
        }
    }
    candidates.retain(|point| {
        [point.x, point.y].iter().all(|value| value.is_finite())
            && a.parameter(*point).is_some()
            && b.parameter(*point).is_some()
    });
    let mut output = Vec::new();
    for point in candidates {
        if output
            .iter()
            .all(|existing: &Vector2| (*existing - point).magnitude() > EPS)
        {
            output.push(point);
        }
    }
    Ok(output)
}
fn segments(sketch: &Sketch, ids: &[String]) -> Result<Vec<Segment>> {
    let point = |id: &str| {
        sketch
            .points
            .get(id)
            .copied()
            .map(Vector2::from)
            .ok_or_else(|| invalid("A contour point is missing"))
    };
    ids.iter()
        .map(|id| match &sketch.members[id].curve {
            SketchCurve::Line { start, end } => Ok(Segment::Line {
                a: point(start)?,
                b: point(end)?,
            }),
            SketchCurve::Arc {
                start,
                through,
                end,
            } => {
                let a = point(start)?;
                let b = point(end)?;
                let m = point(through)?;
                let (center, radius, start, sweep) =
                    cad_core::sketch_geometry::arc_frame([a.x, a.y], [m.x, m.y], [b.x, b.y])?;
                Ok(Segment::Arc {
                    a,
                    b,
                    c: Vector2::from(center),
                    r: radius,
                    start,
                    sweep,
                })
            }
            _ => Err(CadError::Unsupported(
                "Shell sections require exact straight/circular boundaries".into(),
            )),
        })
        .collect()
}
fn simple(contours: &[Vec<Segment>]) -> Result<()> {
    let count: usize = contours.iter().map(Vec::len).sum();
    if count > 2048 {
        return Err(CadError::Unsupported(
            "Shell section exceeds the analytic intersection complexity limit".into(),
        ));
    }
    for (ci, contour) in contours.iter().enumerate() {
        for (i, a) in contour.iter().enumerate() {
            for (cj, other) in contours.iter().enumerate().skip(ci) {
                for (j, b) in other.iter().enumerate() {
                    if ci == cj && j <= i {
                        continue;
                    }
                    let adjacent = ci == cj && (j == i + 1 || i == 0 && j + 1 == contour.len());
                    for p in intersections(a, b)? {
                        let (a0, a1) = a.ends();
                        let (b0, b1) = b.ends();
                        if !adjacent
                            || !(p.distance(a0) <= EPS || p.distance(a1) <= EPS)
                            || !(p.distance(b0) <= EPS || p.distance(b1) <= EPS)
                        {
                            return Err(invalid(
                                "Offset boundaries intersect, overlap or close a material passage; reduce thickness",
                            ));
                        }
                    }
                }
            }
        }
    }
    Ok(())
}
pub(crate) fn nesting(
    sketch: &Sketch,
    source: &[Vec<String>],
    derived: &[Vec<String>],
    inward: bool,
) -> Result<()> {
    let source = source
        .iter()
        .map(|ids| segments(sketch, ids))
        .collect::<Result<Vec<_>>>()?;
    let derived = derived
        .iter()
        .map(|ids| segments(sketch, ids))
        .collect::<Result<Vec<_>>>()?;
    simple(&source)?;
    simple(&derived)?;
    for a in source.iter().flatten() {
        for b in derived.iter().flatten() {
            if !intersections(a, b)?.is_empty() {
                return Err(invalid(
                    "Offset crosses the source boundary or collapses a wall",
                ));
            }
        }
    }
    let (inside, outside) = if inward {
        (&derived, &source)
    } else {
        (&source, &derived)
    };
    for segment in inside.iter().flatten() {
        let p = segment.at(0.37);
        let winding: i32 = outside
            .iter()
            .flatten()
            .map(|segment| segment.crossings(p))
            .sum();
        if winding == 0 {
            return Err(invalid(
                "Offset leaves the source material region or changes its nesting",
            ));
        }
    }
    Ok(())
}
