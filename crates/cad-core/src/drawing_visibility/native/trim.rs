use super::{
    Budget, evaluate,
    interval::{I, J},
};
use crate::{
    Result,
    drawing_geometry::{Curve2, FaceGeometry},
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Region {
    Inside,
    Outside,
    Boundary,
}
pub(super) fn region(
    face: &FaceGeometry,
    uv: [[f64; 2]; 2],
    budget: &mut Budget,
) -> Result<Region> {
    if !face.uv_closed {
        return Err(super::unsupported(
            "native UV trim closure is not certified",
        ));
    }
    let loops = face.uv_chart_loops.as_ref().unwrap_or(&face.loops);
    if loops.is_empty() {
        return Err(super::unsupported(
            "native face has no closed UV trim loops",
        ));
    }
    let query = uv.map(|d| I::new(d[0], d[1]));
    for curve in loops.iter().flatten() {
        for domain in evaluate::curve_domains(curve)? {
            if touches(curve, domain, query, 0, budget)? {
                return Ok(Region::Boundary);
            }
        }
    }
    let point = query.map(I::mid);
    for slope in [0., 0.137, -0.319] {
        let p = [
            I::point(point[0]),
            I::point(point[1]) - I::point(slope) * I::point(point[0]),
        ];
        let mut crossings = Some(0);
        for curve in loops.iter().flatten() {
            for domain in evaluate::curve_domains(curve)? {
                crossings = crossings
                    .zip(winding(curve, domain, p, slope, 0, budget)?)
                    .map(|(a, b)| a + b);
            }
        }
        if let Some(n) = crossings {
            return Ok(if n % 2 == 1 {
                Region::Inside
            } else {
                Region::Outside
            });
        }
    }
    Ok(Region::Boundary)
}
fn touches(
    curve: &Curve2,
    t: [f64; 2],
    query: [I; 2],
    depth: usize,
    budget: &mut Budget,
) -> Result<bool> {
    budget.charge(super::curve2_units(curve))?;
    let p = evaluate::curve2(curve, J::variable(I::new(t[0], t[1]), 0))?;
    if !(0..2).all(|i| p[i].x.overlap(query[i])) {
        return Ok(false);
    }
    if depth >= 28 {
        return Ok(true);
    }
    let mid = t[0] * 0.5 + t[1] * 0.5;
    if mid == t[0] || mid == t[1] {
        return Ok(true);
    }
    Ok(touches(curve, [t[0], mid], query, depth + 1, budget)?
        || touches(curve, [mid, t[1]], query, depth + 1, budget)?)
}
fn winding(
    curve: &Curve2,
    t: [f64; 2],
    p: [I; 2],
    slope: f64,
    depth: usize,
    budget: &mut Budget,
) -> Result<Option<usize>> {
    budget.charge(super::curve2_units(curve))?;
    let mut q = evaluate::curve2(curve, J::variable(I::new(t[0], t[1]), 0))?;
    q[1] = q[1] - q[0] * slope.into();
    if q[1].x.lo > p[1].hi || q[1].x.hi < p[1].lo || q[0].x.hi < p[0].lo {
        return Ok(Some(0));
    }
    let mut a = evaluate::curve2(curve, J::constant(t[0].into()))?;
    a[1] = a[1] - a[0] * slope.into();
    let mut b = evaluate::curve2(curve, J::constant(t[1].into()))?;
    b[1] = b[1] - b[0] * slope.into();
    // Strict endpoint separation and a strict monotone derivative certify
    // one crossing. Endpoint equality is resolved by neighboring subdivision
    // or declared boundary, never rounded into a winding sign.
    if !q[1].d[0].contains_zero() {
        let across = (a[1].x.hi < p[1].lo && b[1].x.lo > p[1].hi)
            || (b[1].x.hi < p[1].lo && a[1].x.lo > p[1].hi);
        if across && q[0].x.lo > p[0].hi {
            return Ok(Some(1));
        }
        if (a[1].x.lo > p[1].hi && b[1].x.lo > p[1].hi)
            || (a[1].x.hi < p[1].lo && b[1].x.hi < p[1].lo)
        {
            return Ok(Some(0));
        }
    }
    if depth >= 40 {
        return Ok(None);
    }
    let mid = t[0] * 0.5 + t[1] * 0.5;
    if mid == t[0] || mid == t[1] {
        return Ok(None);
    }
    let a = winding(curve, [t[0], mid], p, slope, depth + 1, budget)?;
    let b = winding(curve, [mid, t[1]], p, slope, depth + 1, budget)?;
    Ok(a.zip(b).map(|(a, b)| a + b))
}
