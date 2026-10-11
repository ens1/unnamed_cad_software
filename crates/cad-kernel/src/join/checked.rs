//! Checked analytic crossing completeness for layered native Booleans. The
//! legacy planar contact caller keeps its own predicate; uncertain cases here
//! return a diagnostic instead of silently producing an incomplete partition.
use super::{EPS, Kind, Point2, Segment};
use crate::native_bounds::Interval;
use cad_core::{CadError, Result};
use monstertruck::modeling::MetricSpace;

type Pair = [Interval; 2];
fn unsupported(message: &str) -> CadError {
    CadError::Unsupported(format!("Prismatic arrangement: {message}"))
}
fn scalar(value: f64) -> Result<Interval> {
    Interval::exact(value)
}
fn point(p: Point2) -> Result<Pair> {
    Ok([scalar(p.x)?, scalar(p.y)?])
}
fn sub(a: Pair, b: Pair) -> Result<Pair> {
    Ok([a[0].sub(b[0])?, a[1].sub(b[1])?])
}
fn add(a: Pair, b: Pair) -> Result<Pair> {
    Ok([a[0].add(b[0])?, a[1].add(b[1])?])
}
fn scale(a: Pair, b: Interval) -> Result<Pair> {
    Ok([a[0].mul(b)?, a[1].mul(b)?])
}
fn dot(a: Pair, b: Pair) -> Result<Interval> {
    a[0].mul(b[0])?.add(a[1].mul(b[1])?)
}
fn cross(a: Pair, b: Pair) -> Result<Interval> {
    a[0].mul(b[1])?.sub(a[1].mul(b[0])?)
}
fn square(a: Interval) -> Result<Interval> {
    if a.low >= 0. {
        a.mul(a)
    } else if a.high <= 0. {
        let b = scalar(-1.)?.mul(a)?;
        b.mul(b)
    } else {
        Interval::new(0., (a.magnitude() * a.magnitude()).next_up())
    }
}
fn norm2(a: Pair) -> Result<Interval> {
    let value = square(a[0])?.add(square(a[1])?)?;
    Interval::new(value.low.max(0.), value.high.max(0.))
}
fn resolved(p: Pair) -> Result<Point2> {
    if p.iter()
        .any(|a| !a.low.is_finite() || !a.high.is_finite() || a.high - a.low > EPS * 0.125)
    {
        return Err(unsupported(
            "a crossing has no coordinate enclosure within the native tolerance",
        ));
    }
    Ok(Point2::new(
        p[0].low + (p[0].high - p[0].low) * 0.5,
        p[1].low + (p[1].high - p[1].low) * 0.5,
    ))
}
fn validate(segment: &Segment) -> Result<()> {
    let valid = |p: Point2| {
        [p.x, p.y]
            .into_iter()
            .all(|v| v.is_finite() && v.abs() <= 1e12)
    };
    if !valid(segment.a) || !valid(segment.b) {
        return Err(unsupported(
            "intersection coordinates exceed the finite analytic bound",
        ));
    }
    match segment.kind {
        Kind::Line if segment.a.distance(segment.b) <= EPS => {
            Err(unsupported("a line interval collapses at native tolerance"))
        }
        Kind::Arc {
            center,
            radius,
            sweep,
        } if !valid(center)
            || !radius.is_finite()
            || radius <= EPS
            || radius > 1e12
            || !sweep.is_finite()
            || sweep.abs() <= 1e-14
            || sweep.abs() > std::f64::consts::TAU + 1e-9 =>
        {
            Err(unsupported(
                "a circular interval exceeds its finite analytic bound",
            ))
        }
        _ => Ok(()),
    }
}
fn orientation(a: Point2, b: Point2, p: Point2) -> f64 {
    let coord = |p: Point2| robust::Coord { x: p.x, y: p.y };
    robust::orient2d(coord(a), coord(b), coord(p))
}
fn same_sign(a: f64, b: f64) -> bool {
    a > 0. && b > 0. || a < 0. && b < 0.
}
fn endpoints(a: &Segment, b: &Segment) -> Vec<Point2> {
    vec![a.a, a.b, b.a, b.b]
}

fn shared(a: &Segment, b: &Segment) -> Option<Point2> {
    [a.a, a.b].into_iter().find(|p| *p == b.a || *p == b.b)
}
fn line_line(a: &Segment, b: &Segment, result: &mut Vec<Point2>) -> Result<()> {
    let oa = orientation(a.a, a.b, b.a);
    let ob = orientation(a.a, a.b, b.b);
    if oa == 0. && ob == 0. {
        // Adaptive exact orientation distinguishes truly coincident lines from
        // merely small determinants. Endpoints partition their exact overlap.
        result.extend(endpoints(a, b));
        return Ok(());
    }
    if let Some(p) = shared(a, b) {
        // Distinct straight carriers have only one intersection. Exact input
        // endpoint equality supplies it even when their crossing is shallow.
        result.push(p);
        return Ok(());
    }
    let pa = point(a.a)?;
    let pb = point(b.a)?;
    let da = sub(point(a.b)?, pa)?;
    let db = sub(point(b.b)?, pb)?;
    let q = sub(pb, pa)?;
    if same_sign(oa, ob) {
        let c0 = cross(da, q)?;
        let c1 = cross(da, sub(point(b.b)?, pa)?)?;
        let margin = norm2(da)?.sqrt()?.high * EPS;
        if c0.low > margin && c1.low > margin || c0.high < -margin && c1.high < -margin {
            return Ok(());
        }
    }
    let other_a = orientation(b.a, b.b, a.a);
    let other_b = orientation(b.a, b.b, a.b);
    if same_sign(other_a, other_b) {
        let c0 = cross(db, sub(pa, pb)?)?;
        let c1 = cross(db, sub(point(a.b)?, pb)?)?;
        let margin = norm2(db)?.sqrt()?.high * EPS;
        if c0.low > margin && c1.low > margin || c0.high < -margin && c1.high < -margin {
            return Ok(());
        }
    }
    let determinant = cross(da, db)?;
    if determinant.low <= 0. && determinant.high >= 0. {
        return Err(unsupported(
            "near-parallel lines are neither certified coincident nor separated",
        ));
    }
    let ta = cross(q, db)?.div(determinant)?;
    let tb = cross(q, da)?.div(determinant)?;
    if ta.high < 0. || ta.low > 1. || tb.high < 0. || tb.low > 1. {
        return Ok(());
    }
    result.push(resolved(add(pa, scale(da, ta)?)?)?);
    Ok(())
}

// Exact subtraction is required before recognizing an axis-aligned tangent;
// an equality between two rounded differences is not a tangency certificate.
fn exact_difference(a: f64, b: f64) -> Option<f64> {
    let value = a - b;
    let tail = a - value;
    let error = (a - (value + tail)) + (tail - b);
    (value.is_finite() && error == 0.).then_some(value)
}
fn difference_expansion(a: f64, b: f64) -> [f64; 2] {
    let value = a - b;
    let tail = a - value;
    [(a - (value + tail)) + (tail - b), value]
}
fn grow(expansion: &mut Vec<f64>, mut value: f64) {
    let mut output = Vec::with_capacity(expansion.len() + 1);
    for component in expansion.iter().copied() {
        let sum = value + component;
        let virtual_component = sum - value;
        let error = (value - (sum - virtual_component)) + (component - virtual_component);
        if error != 0. {
            output.push(error);
        }
        value = sum;
    }
    if value != 0. {
        output.push(value);
    }
    *expansion = output;
}
fn circle_root(p: Point2, center: Point2, radius: f64) -> bool {
    let mut expansion = Vec::new();
    for (parts, sign) in [
        (difference_expansion(p.x, center.x), 1.),
        (difference_expansion(p.y, center.y), 1.),
        ([0., radius], -1.),
    ] {
        for a in parts {
            for b in parts {
                if a == 0. || b == 0. {
                    continue;
                }
                let product = a * b;
                if !product.is_finite() || product.abs() < 1e-250 {
                    return false;
                }
                grow(&mut expansion, a.mul_add(b, -product) * sign);
                grow(&mut expansion, product * sign);
            }
        }
    }
    expansion.is_empty()
}
fn tangent_at(line: &Segment, center: Point2, p: Point2) -> bool {
    let mut expansion = Vec::new();
    for (a, b) in [
        (
            difference_expansion(p.x, center.x),
            difference_expansion(line.b.x, line.a.x),
        ),
        (
            difference_expansion(p.y, center.y),
            difference_expansion(line.b.y, line.a.y),
        ),
    ] {
        for x in a {
            for y in b {
                if x == 0. || y == 0. {
                    continue;
                }
                let product = x * y;
                if !product.is_finite() || product.abs() < 1e-250 {
                    return false;
                }
                grow(&mut expansion, x.mul_add(y, -product));
                grow(&mut expansion, product);
            }
        }
    }
    expansion.is_empty()
}
fn line_circle(
    line: &Segment,
    arc: &Segment,
    center: Point2,
    radius: f64,
    result: &mut Vec<Point2>,
) -> Result<()> {
    let start = point(line.a)?;
    let d = sub(point(line.b)?, start)?;
    let length2 = norm2(d)?;
    if length2.low <= 0. {
        return Err(unsupported("a line has unresolved native length"));
    }
    let c = point(center)?;
    if let Some(p) = shared(line, arc).filter(|p| circle_root(*p, center, radius)) {
        // An exact dyadic residual proves this endpoint is a circle root. Factoring it
        // avoids subtracting nearly equal squared lengths at tangent joins.
        let offset = sub(point(p)?, c)?;
        let t = scalar(-2.)?.mul(dot(offset, d)?)?.div(length2)?;
        result.push(p);
        if tangent_at(line, center, p) {
            return Ok(());
        }
        if t.low <= 0. && t.high >= 0. {
            return Err(unsupported(
                "two line/circle roots cannot be distinguished from their shared endpoint",
            ));
        }
        let native = t.add(scalar(if p == line.a { 0. } else { 1. })?)?;
        if native.high >= 0. && native.low <= 1. {
            result.push(resolved(add(point(p)?, scale(d, t)?)?)?);
        }
        return Ok(());
    }
    if line.a.y == line.b.y
        && exact_difference(center.y, line.a.y).is_some_and(|d| d.abs() == radius)
    {
        result.push(Point2::new(center.x, line.a.y));
        return Ok(());
    }
    if line.a.x == line.b.x
        && exact_difference(center.x, line.a.x).is_some_and(|d| d.abs() == radius)
    {
        result.push(Point2::new(line.a.x, center.y));
        return Ok(());
    }
    let offset = sub(c, start)?;
    let along = dot(offset, d)?.div(length2)?;
    let height2 = square(scalar(radius)?)?.sub(square(cross(offset, d)?)?.div(length2)?)?;
    if height2.high < 0. {
        return Ok(());
    }
    if height2.low <= 0. {
        return Err(unsupported(
            "line/circle contact is near tangent without a certified common native endpoint or exact axis tangent",
        ));
    }
    let delta = height2.div(length2)?.sqrt()?;
    for t in [along.sub(delta)?, along.add(delta)?] {
        if t.high < 0. || t.low > 1. {
            continue;
        }
        result.push(resolved(add(start, scale(d, t)?)?)?);
    }
    Ok(())
}
fn circle_circle(
    a: &Segment,
    b: &Segment,
    c0: Point2,
    r0: f64,
    c1: Point2,
    r1: f64,
    result: &mut Vec<Point2>,
) -> Result<()> {
    let center_distance = norm2(sub(point(c1)?, point(c0)?)?)?.sqrt()?;
    let radial_difference = scalar(r0)?.sub(scalar(r1)?)?;
    // Independently recognized pieces of one native circle can differ by a
    // few ulps. The complete circles have Hausdorff distance no greater than
    // |center1-center0| + |radius1-radius0|. Enclose that bound explicitly and
    // reserve only one quarter of the native membership tolerance for it.
    if center_distance.high + radial_difference.magnitude() <= EPS * 0.25 {
        result.extend(endpoints(a, b));
        return Ok(());
    }
    if c0 == c1 {
        if r0 == r1 {
            result.extend(endpoints(a, b));
            return Ok(());
        }
        if (r0 - r1).abs() > EPS {
            return Ok(());
        }
        return Err(unsupported(
            "near-coincident circles have distinct unresolved radii",
        ));
    }
    let c = point(c0)?;
    let delta = sub(point(c1)?, c)?;
    let distance2 = norm2(delta)?;
    if distance2.low <= 0. {
        return Err(unsupported(
            "circle centers cannot be separated within finite arithmetic",
        ));
    }
    let distance = distance2.sqrt()?;
    let outer = scalar(r0)?.add(scalar(r1)?)?;
    let inner = scalar(r0)?.sub(scalar(r1)?)?;
    if distance.low > outer.high || distance2.high < square(inner)?.low {
        return Ok(());
    }
    if let Some(p) = shared(a, b).filter(|p| circle_root(*p, c0, r0) && circle_root(*p, c1, r1)) {
        if orientation(c0, c1, p) == 0. {
            result.push(p);
            return Ok(());
        }
        let t = dot(sub(point(p)?, c)?, delta)?.div(distance2)?;
        let foot = add(c, scale(delta, t)?)?;
        result.extend([p, resolved(sub(scale(foot, scalar(2.)?)?, point(p)?)?)?]);
        return Ok(());
    }
    let along = inner
        .mul(outer)?
        .add(distance2)?
        .div(scalar(2.)?.mul(distance)?)?;
    let height2 = square(scalar(r0)?)?.sub(square(along)?)?;
    if height2.high < 0. {
        return Ok(());
    }
    if height2.low <= 0. {
        return Err(unsupported(
            "circle/circle tangency cannot be resolved without a certified common native endpoint",
        ));
    }
    let unit = scale(delta, scalar(1.)?.div(distance)?)?;
    let foot = add(c, scale(unit, along)?)?;
    let side = scale([scalar(-1.)?.mul(unit[1])?, unit[0]], height2.sqrt()?)?;
    result.extend([resolved(add(foot, side)?)?, resolved(sub(foot, side)?)?]);
    Ok(())
}

fn bounds(segment: &Segment) -> Result<Pair> {
    match segment.kind {
        Kind::Line => Ok([
            Interval::new(segment.a.x.min(segment.b.x), segment.a.x.max(segment.b.x))?,
            Interval::new(segment.a.y.min(segment.b.y), segment.a.y.max(segment.b.y))?,
        ]),
        Kind::Arc { center, radius, .. } => Ok([
            scalar(center.x)?.widen(radius)?,
            scalar(center.y)?.widen(radius)?,
        ]),
    }
}
fn membership(p: Point2, segment: &Segment) -> Result<bool> {
    if p == segment.a || p == segment.b {
        return Ok(true);
    }
    let enclosure = [
        Interval::rounded(p.x)?.widen(EPS * 0.125)?,
        Interval::rounded(p.y)?.widen(EPS * 0.125)?,
    ];
    match segment.kind {
        Kind::Line => {
            let d = sub(point(segment.b)?, point(segment.a)?)?;
            let t = dot(sub(enclosure, point(segment.a)?)?, d)?.div(norm2(d)?)?;
            if t.high < 0. || t.low > 1. {
                return Ok(false);
            }
            if t.low > 0. && t.high < 1. {
                return Ok(true);
            }
        }
        Kind::Arc { center, sweep, .. } => {
            if sweep.abs() == std::f64::consts::TAU {
                return Ok(true);
            }
            let (start, end) = if sweep > 0. {
                (segment.a, segment.b)
            } else {
                (segment.b, segment.a)
            };
            let q = sub(enclosure, point(center)?)?;
            let first = cross(sub(point(start)?, point(center)?)?, q)?;
            let last = cross(q, sub(point(end)?, point(center)?)?)?;
            if sweep.abs() <= std::f64::consts::PI {
                if first.high < 0. || last.high < 0. {
                    return Ok(false);
                }
                if first.low > 0. && last.low > 0. {
                    return Ok(true);
                }
            } else {
                if first.low > 0. || last.low > 0. {
                    return Ok(true);
                }
                if first.high < 0. && last.high < 0. {
                    return Ok(false);
                }
            }
        }
    }
    Err(unsupported(
        "a crossing cannot be certified inside or outside its bounded native interval near an endpoint",
    ))
}

pub(crate) fn checked_intersections(a: &Segment, b: &Segment) -> Result<Vec<Point2>> {
    validate(a)?;
    validate(b)?;
    let ba = bounds(a)?;
    let bb = bounds(b)?;
    if (0..2).any(|i| ba[i].high < bb[i].low || bb[i].high < ba[i].low) {
        return Ok(Vec::new());
    }
    let mut result = Vec::new();
    match (&a.kind, &b.kind) {
        (Kind::Line, Kind::Line) => line_line(a, b, &mut result)?,
        (Kind::Line, Kind::Arc { center, radius, .. }) => {
            line_circle(a, b, *center, *radius, &mut result)?
        }
        (Kind::Arc { center, radius, .. }, Kind::Line) => {
            line_circle(b, a, *center, *radius, &mut result)?
        }
        (
            Kind::Arc {
                center: c0,
                radius: r0,
                ..
            },
            Kind::Arc {
                center: c1,
                radius: r1,
                ..
            },
        ) => circle_circle(a, b, *c0, *r0, *c1, *r1, &mut result)?,
    }
    let mut unique: Vec<Point2> = Vec::new();
    for p in result {
        if !membership(p, a)? || !membership(p, b)? {
            continue;
        }
        if [a.a, a.b, b.a, b.b]
            .into_iter()
            .any(|endpoint| endpoint != p && endpoint.distance(p) <= EPS)
        {
            return Err(unsupported(
                "an intersection and a distinct authored endpoint are unresolved at native tolerance",
            ));
        }
        if let Some(other) = unique.iter().find(|other| other.distance(p) <= EPS) {
            if *other != p || ![a.a, a.b, b.a, b.b].contains(&p) {
                return Err(unsupported(
                    "distinct crossings cannot be separated at native tolerance",
                ));
            }
        } else {
            unique.push(p);
        }
    }
    Ok(unique)
}
