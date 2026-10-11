//! Sufficient Bernstein proofs for bounded, non-retracing line/circle edges.
use cad_core::{CadError, Result};
use monstertruck::{geometry::prelude::TryIntoHomogeneousBsplineCurve, modeling::*};
fn unsupported(message: &str) -> CadError {
    CadError::Unsupported(format!("Boundary traversal: {message}"))
}
pub(crate) fn spans(curve: &Curve) -> Result<Vec<Vec<Vector4>>> {
    if !matches!(
        curve,
        Curve::Line(_) | Curve::BsplineCurve(_) | Curve::NurbsCurve(_)
    ) {
        return Err(unsupported(
            "Implicit intersection/parameter curves need an exact bounded carrier",
        ));
    }
    let mut curve = curve
        .try_into_homogeneous_bspline_curve()
        .ok_or_else(|| unsupported("No exact rational boundary"))?;
    curve.clamp();
    let degree = curve.degree();
    if !(1..=32).contains(&degree) {
        return Err(unsupported("Carrier proof supports degrees 1 through 32"));
    }
    let (a, b) = curve.range_tuple();
    let mut knots = curve
        .knot_vector()
        .iter()
        .copied()
        .filter(|value| *value > a && *value < b)
        .collect::<Vec<_>>();
    knots.sort_by(f64::total_cmp);
    knots.dedup();
    let mut result = Vec::new();
    for knot in knots {
        let right = curve.cut(knot);
        if curve.control_points().len() != degree + 1 {
            return Err(unsupported("Carrier has an unsupported knot structure"));
        }
        result.push(curve.control_points().clone());
        curve = right;
    }
    if curve.control_points().len() != degree + 1 {
        return Err(unsupported("Carrier has an unsupported final knot span"));
    }
    result.push(curve.control_points().clone());
    for span in &result {
        if span
            .iter()
            .any(|p| ![p.x, p.y, p.z, p.w].iter().all(|v| v.is_finite()) || p.w <= 0.)
        {
            return Err(unsupported(
                "Carrier requires finite positive homogeneous controls",
            ));
        }
    }
    Ok(result)
}
fn point(h: Vector4) -> Result<Point3> {
    let p = Point3::new(h.x / h.w, h.y / h.w, h.z / h.w);
    if ![p.x, p.y, p.z].iter().all(|value| value.is_finite()) {
        return Err(unsupported("Carrier point exceeds finite coordinates"));
    }
    Ok(p)
}
fn binomial(n: usize, k: usize) -> f64 {
    (1..=k.min(n - k)).fold(1., |value, i| value * (n + 1 - i) as f64 / i as f64)
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
fn derivative(a: &[f64]) -> Vec<f64> {
    a.windows(2)
        .map(|pair| (pair[1] - pair[0]) * (a.len() - 1) as f64)
        .collect()
}
fn sign(coefficients: &[f64]) -> Result<f64> {
    let scale = coefficients
        .iter()
        .map(|value| value.abs())
        .fold(0., f64::max);
    if !scale.is_finite() || scale == 0. || coefficients.iter().any(|value| !value.is_finite()) {
        return Err(unsupported("Carrier derivative is collapsed or nonfinite"));
    }
    let sign = coefficients[0].signum();
    if coefficients
        .iter()
        .any(|value| value * sign <= scale * 1e-13)
    {
        return Err(unsupported(
            "Carrier monotonicity cannot be proven across every Bernstein coefficient",
        ));
    }
    Ok(sign)
}
pub(crate) fn line(curve: &Curve) -> Result<()> {
    let axis = crate::axis_geometry::line_axis(curve)?;
    let origin = Point3::from(axis.origin);
    let direction = Vector3::from(axis.direction);
    let (a, b) = curve.range_tuple();
    let length = (curve.subs(b) - curve.subs(a)).magnitude();
    if !length.is_finite() || length <= 1e-7 {
        return Err(unsupported(
            "Straight edge has an unbounded or collapsed extent",
        ));
    }
    for span in spans(curve)? {
        let scale = span.iter().map(|p| p.w).fold(0., f64::max);
        let weights = span.iter().map(|p| p.w / scale).collect::<Vec<_>>();
        let positions = span
            .iter()
            .map(|p| Ok((point(*p)? - origin).dot(direction) / length * (p.w / scale)))
            .collect::<Result<Vec<_>>>()?;
        let first = product(&derivative(&positions), &weights);
        let second = product(&positions, &derivative(&weights));
        if sign(
            &first
                .iter()
                .zip(second)
                .map(|(a, b)| a - b)
                .collect::<Vec<_>>(),
        )? < 0.
        {
            return Err(unsupported("Straight edge reverses its endpoint direction"));
        }
    }
    Ok(())
}
pub(crate) fn circle(curve: &Curve) -> Result<((Point3, Vector3, f64), f64)> {
    let (center, normal, radius) = crate::axis_geometry::circle_geometry(curve)?;
    let all = spans(curve)?;
    let u = (point(all[0][0])? - center) / radius;
    let v = normal.cross(u);
    let mut orientation = 0.;
    let mut sweep = 0.;
    for span in all {
        let weight_scale = span.iter().map(|p| p.w).fold(0., f64::max);
        let controls = span
            .iter()
            .map(|p| {
                let relative = (point(*p)? - center) / radius;
                let weight = p.w / weight_scale;
                Ok(Vector2::new(
                    relative.dot(u) * weight,
                    relative.dot(v) * weight,
                ))
            })
            .collect::<Result<Vec<_>>>()?;
        let x = controls.iter().map(|p| p.x).collect::<Vec<_>>();
        let y = controls.iter().map(|p| p.y).collect::<Vec<_>>();
        let a = product(&x, &derivative(&y));
        let b = product(&y, &derivative(&x));
        let direction = sign(&a.iter().zip(b).map(|(a, b)| a - b).collect::<Vec<_>>())?;
        if orientation != 0. && orientation != direction {
            return Err(unsupported("Circular edge reverses angular direction"));
        }
        orientation = direction;
        let first = controls[0].normalize();
        let last = controls.last().unwrap().normalize();
        let bisector = first + last;
        let length = bisector.magnitude();
        if !length.is_finite()
            || length < 1e-9
            || controls
                .iter()
                .any(|p| p.dot(bisector / length) <= p.magnitude() * 1e-10)
        {
            return Err(unsupported(
                "A circular span must have a proven open half-plane control hull",
            ));
        }
        let angle = (first.x * last.y - first.y * last.x).atan2(first.dot(last));
        if !angle.is_finite() || angle * direction <= 0. {
            return Err(unsupported("Circular span has inconsistent angular extent"));
        }
        sweep += angle;
    }
    if !sweep.is_finite() || sweep.abs() >= std::f64::consts::TAU - 1e-10 {
        return Err(unsupported(
            "One circular edge loops or closes its endpoint; split it into bounded arcs",
        ));
    }
    Ok(((center, normal, radius), sweep))
}
