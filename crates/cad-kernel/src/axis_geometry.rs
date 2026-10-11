//! Axis recognition from exact carriers and rational polynomial identities.
//! Candidate values are verified against every Bernstein coefficient; no mesh
//! or sampled display curve supplies a geometric reference.
use cad_core::{CadError, Result, reference_geometry::Axis};
use monstertruck::{
    geometry::prelude::{TryIntoHomogeneousBsplineCurve, TryIntoHomogeneousBsplineSurface},
    modeling::*,
};

const DISTANCE: f64 = 1e-7;
const RELATIVE: f64 = 1e-10;
fn unsupported(message: &str) -> CadError {
    CadError::Unsupported(format!("Reference axis: {message}"))
}
fn finite(p: Point3) -> bool {
    [p.x, p.y, p.z].iter().all(|v| v.is_finite())
}
fn unit(v: Vector3) -> Result<Vector3> {
    let l = v.magnitude();
    if !l.is_finite() || l < 1e-12 {
        return Err(unsupported("carrier has a degenerate axis"));
    }
    Ok(v / l)
}
fn axis(origin: Point3, direction: Vector3) -> Result<Axis> {
    if !finite(origin) {
        return Err(unsupported("axis exceeds finite document coordinates"));
    }
    let d = unit(direction)?;
    Ok(Axis {
        origin: [origin.x, origin.y, origin.z],
        direction: [d.x, d.y, d.z],
    })
}
#[derive(Clone, Copy)]
struct Circle {
    center: Point3,
    normal: Vector3,
    radius: f64,
}
fn point(h: Vector4) -> Result<Point3> {
    if ![h.x, h.y, h.z, h.w].iter().all(|v| v.is_finite()) || h.w <= 0. {
        return Err(unsupported(
            "circle proof requires finite positive rational weights",
        ));
    }
    let p = Point3::new(h.x / h.w, h.y / h.w, h.z / h.w);
    if !finite(p) {
        return Err(unsupported("rational controls exceed finite coordinates"));
    }
    Ok(p)
}
fn bezier_spans(mut curve: BsplineCurve<Vector4>) -> Result<Vec<Vec<Vector4>>> {
    curve.clamp();
    let degree = curve.degree();
    if !(2..=32).contains(&degree) {
        return Err(unsupported(
            "circular-axis proof supports rational degrees 2–32",
        ));
    }
    let (a, b) = curve.range_tuple();
    let mut knots: Vec<_> = curve
        .knot_vector()
        .iter()
        .copied()
        .filter(|t| *t > a && *t < b)
        .collect();
    knots.sort_by(f64::total_cmp);
    knots.dedup();
    let mut out = Vec::new();
    for knot in knots {
        let right = curve.cut(knot);
        if curve.control_points().len() != degree + 1 {
            return Err(unsupported(
                "curve cannot be decomposed into exact Bernstein spans",
            ));
        }
        out.push(curve.control_points().clone());
        curve = right;
    }
    if curve.control_points().len() != degree + 1 {
        return Err(unsupported("curve has an unsupported knot structure"));
    }
    out.push(curve.control_points().clone());
    Ok(out)
}
fn binomial(n: usize, k: usize) -> f64 {
    (1..=k.min(n - k)).fold(1., |a, i| a * (n + 1 - i) as f64 / i as f64)
}
fn circle(curve: BsplineCurve<Vector4>) -> Result<Circle> {
    let spans = bezier_spans(curve)?;
    let first = &spans[0];
    let degree = first.len() - 1;
    let p0 = point(first[0])?;
    let p2 = point(first[degree])?;
    let mut midpoint = first.clone();
    for count in (1..midpoint.len()).rev() {
        for i in 0..count {
            midpoint[i] = (midpoint[i] + midpoint[i + 1]) * 0.5;
        }
    }
    let p1 = point(midpoint[0])?;
    let a = p1 - p0;
    let b = p2 - p0;
    let n = a.cross(b);
    let n2 = n.magnitude2();
    if !n2.is_finite() || n2 <= 1e-10 * a.magnitude2() * b.magnitude2() || n2 < 1e-36 {
        return Err(unsupported(
            "circular carrier is too short or ill-conditioned to prove its center",
        ));
    }
    let center = p0 + (n.cross(a) * b.magnitude2() + b.cross(n) * a.magnitude2()) / (2. * n2);
    let mut normal = unit(n)?;
    let radius = (p0 - center).magnitude();
    if !finite(center) || !radius.is_finite() || radius <= 1e-9 {
        return Err(unsupported("circle has a degenerate or non-finite radius"));
    }
    // Canonical orientation is independent of a topological edge's inversion.
    let dominant = if normal.x.abs() >= normal.y.abs() && normal.x.abs() >= normal.z.abs() {
        normal.x
    } else if normal.y.abs() >= normal.z.abs() {
        normal.y
    } else {
        normal.z
    };
    if dominant < 0. {
        normal = -normal;
    }
    for controls in spans {
        let p = controls.len() - 1;
        let local: Vec<_> = controls
            .iter()
            .map(|h| {
                Vector3::new(
                    h.x - center.x * h.w,
                    h.y - center.y * h.w,
                    h.z - center.z * h.w,
                )
            })
            .collect();
        for (h, v) in controls.iter().zip(&local) {
            point(*h)?;
            if v.dot(normal).abs() > DISTANCE * h.w + RELATIVE * v.magnitude() {
                return Err(unsupported("edge is not an exact planar circle"));
            }
        }
        for k in 0..=2 * p {
            let mut value = 0.;
            let mut weight = 0.;
            for i in k.saturating_sub(p)..=k.min(p) {
                let j = k - i;
                let coefficient = binomial(p, i) * binomial(p, j) / binomial(2 * p, k);
                value += coefficient
                    * (local[i].dot(local[j]) - radius * radius * controls[i].w * controls[j].w);
                weight += coefficient * controls[i].w * controls[j].w;
            }
            if !value.is_finite()
                || !weight.is_finite()
                || weight <= 0.
                || value.abs() > weight * (2. * radius * DISTANCE + RELATIVE * radius * radius)
            {
                return Err(unsupported(
                    "rational edge does not satisfy the exact circular polynomial identity",
                ));
            }
        }
    }
    Ok(Circle {
        center,
        normal,
        radius,
    })
}
pub(crate) fn circle_geometry(curve: &Curve) -> Result<(Point3, Vector3, f64)> {
    if matches!(
        curve,
        Curve::IntersectionCurve(_) | Curve::ParameterCurve(_)
    ) {
        return Err(unsupported(
            "implicit boundaries do not prove a circular carrier",
        ));
    }
    let native = curve
        .try_into_homogeneous_bspline_curve()
        .ok_or_else(|| unsupported("edge has no exact rational circular carrier"))?;
    let circle = circle(native)?;
    Ok((circle.center, circle.normal, circle.radius))
}
pub(crate) fn circle_axis(curve: &Curve) -> Result<Axis> {
    if matches!(curve, Curve::IntersectionCurve(_)) {
        return Err(unsupported(
            "intersection guides do not prove a circular carrier",
        ));
    }
    let native = curve
        .try_into_homogeneous_bspline_curve()
        .ok_or_else(|| unsupported("edge has no exact rational circular carrier"))?;
    let circle = circle(native)?;
    axis(circle.center, circle.normal)
}
pub(crate) fn line_axis(curve: &Curve) -> Result<Axis> {
    if matches!(curve, Curve::IntersectionCurve(_)) {
        return Err(unsupported(
            "intersection guides do not prove a straight carrier",
        ));
    }
    let native = curve
        .try_into_homogeneous_bspline_curve()
        .ok_or_else(|| unsupported("edge has no exact rational line representation"))?;
    let (a, b) = native.range_tuple();
    let rational = NurbsCurve::new(native.clone());
    let start = rational.subs(a);
    let end = rational.subs(b);
    let d = unit(end - start)?;
    for h in native.control_points() {
        if (point(*h)? - start).cross(d).magnitude() > DISTANCE {
            return Err(unsupported("edge controls do not prove a straight line"));
        }
    }
    axis(start, d)
}

fn ruled_axis(surface: BsplineSurface<Vector4>) -> Result<Axis> {
    let mut surface = surface;
    if surface.vdegree() != 1 || surface.control_points()[0].len() != 2 {
        if surface.udegree() == 1 && surface.control_points().len() == 2 {
            surface.invert();
        } else {
            return Err(unsupported(
                "rational face must have an exact linear ruling direction",
            ));
        }
    }
    if surface.vdegree() != 1 || surface.control_points()[0].len() != 2 {
        return Err(unsupported(
            "rational face has unsupported ruled parameterization",
        ));
    }
    let first = BsplineCurve::try_new(
        surface.knot_vector_u().clone(),
        surface.control_points().iter().map(|row| row[0]).collect(),
    )
    .map_err(|e| unsupported(&e.to_string()))?;
    let second = BsplineCurve::try_new(
        surface.knot_vector_u().clone(),
        surface.control_points().iter().map(|row| row[1]).collect(),
    )
    .map_err(|e| unsupported(&e.to_string()))?;
    let a = circle(first)?;
    let b = circle(second)?;
    let direction = unit(b.center - a.center)?;
    if a.normal.dot(direction).abs() < 1. - RELATIVE
        || b.normal.dot(direction).abs() < 1. - RELATIVE
    {
        return Err(unsupported(
            "circular rulings do not have a common normal axis",
        ));
    }
    let ratio = surface.control_points()[0][1].w / surface.control_points()[0][0].w;
    let scale = b.radius / a.radius;
    if !ratio.is_finite() || ratio <= 0. || !scale.is_finite() || scale <= 0. {
        return Err(unsupported(
            "circular ruling scale or weight ratio exceeds finite positive bounds",
        ));
    }
    for row in surface.control_points() {
        let p = point(row[0])? - a.center;
        let q = point(row[1])? - b.center;
        let weight_ratio = row[1].w / row[0].w;
        if !weight_ratio.is_finite()
            || (weight_ratio - ratio).abs() > RELATIVE * ratio
            || (q - p * scale).magnitude() > DISTANCE + RELATIVE * b.radius
        {
            return Err(unsupported(
                "circular ruling controls or weights do not prove a cylindrical/conical carrier",
            ));
        }
    }
    axis(a.center, direction)
}
pub(crate) fn face_axis(face: &Face) -> Result<Axis> {
    let surface = face.surface();
    if let Surface::RevolutionSurface(processor) = &surface {
        let source = processor.entity();
        let n = unit(source.axis())?;
        let transverse = unit(n.cross(if n.x.abs() < 0.8 {
            Vector3::unit_x()
        } else {
            Vector3::unit_y()
        }))?;
        let other = n.cross(transverse);
        let m = processor.transform();
        let matrix: [[f64; 4]; 4] = (*m).into();
        if matrix.iter().flatten().any(|v| !v.is_finite())
            || m.x.w.abs() > RELATIVE
            || m.y.w.abs() > RELATIVE
            || m.z.w.abs() > RELATIVE
            || (m.w.w - 1.).abs() > RELATIVE
            || m.invert().is_none()
        {
            return Err(unsupported(
                "revolution axis needs a finite reversible affine processor",
            ));
        }
        let a = m.transform_vector(transverse);
        let b = m.transform_vector(other);
        let d = m.transform_vector(n);
        let l = a.magnitude2();
        let r = b.magnitude2();
        if ![l, r, d.magnitude2()]
            .iter()
            .all(|v| v.is_finite() && *v > 1e-24)
        {
            return Err(unsupported(
                "processor has degenerate or non-finite rotational directions",
            ));
        }
        // Normalize before taking dot products. Products of finite squared
        // lengths can overflow and must not silently loosen the proof bound.
        let an = unit(a)?;
        let bn = unit(b)?;
        let dn = unit(d)?;
        if an.dot(bn).abs() > RELATIVE
            || (l - r).abs() > RELATIVE * l.max(r)
            || dn.dot(an).abs() > RELATIVE
            || dn.dot(bn).abs() > RELATIVE
        {
            return Err(unsupported(
                "processor shear or unequal radial scaling destroys rotational form",
            ));
        }
        return axis(m.transform_point(source.origin()), d);
    }
    if !matches!(
        surface,
        Surface::NurbsSurface(_) | Surface::BsplineSurface(_)
    ) {
        return Err(unsupported(
            "select a proven cylindrical/conical rational face or analytic revolution surface",
        ));
    }
    let rational = surface
        .try_into_homogeneous_bspline_surface()
        .ok_or_else(|| unsupported("face carrier has no exact rational form"))?;
    ruled_axis(rational)
}
