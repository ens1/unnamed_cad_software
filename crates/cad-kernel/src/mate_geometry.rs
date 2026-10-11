//! Boundary-derived planar mate carriers and centroids; no display tessellation.
use crate::adapter::{cad_plane, exact_curve};
use cad_core::{
    CadError, Result,
    assembly_geometry::{Curve as Carrier, CurveGeometry, Frame, FrameKind},
};
use monstertruck::modeling::*;
use nalgebra::Vector3 as Vec3;
fn unsupported(s: &str) -> CadError {
    CadError::Unsupported(format!("Mate geometry: {s}"))
}
fn project(curve: &Curve, plane: &cad_core::Plane) -> Result<Carrier> {
    let c = exact_curve(curve).ok_or_else(|| {
        unsupported("boundary must have an explicit positive-weight rational carrier")
    })?;
    let c = cad_core::sketch_edit::project(&c, plane)?;
    Ok(Carrier {
        plane: plane.clone(),
        geometry: CurveGeometry::Nurbs { curve: c },
        slot_carrier: None,
    })
}
pub(crate) fn edge_curve(edge: &Edge) -> Result<Carrier> {
    let curve = edge.oriented_curve();
    let source =
        exact_curve(&curve).ok_or_else(|| unsupported("implicit or nonrational curve carrier"))?;
    let p = Vec3::from(
        *source
            .control_points
            .first()
            .ok_or_else(|| unsupported("empty curve carrier"))?,
    );
    if let Ok(line) = crate::axis_geometry::line_axis(&curve) {
        let n = Vec3::from(line.direction);
        let helper = if n.x.abs() < 0.8 {
            Vec3::x()
        } else {
            Vec3::y()
        };
        let v = (helper - n * n.dot(&helper)).normalize();
        let plane = cad_core::Plane {
            origin: p.into(),
            u: n.into(),
            v: v.into(),
        };
        let mut carrier = project(&curve, &plane)?;
        carrier.slot_carrier = Some(cad_core::assembly_geometry::SlotCarrier::Line);
        return Ok(carrier);
    }
    if let Ok((center, normal, _)) = crate::axis_geometry::circle_geometry(&curve) {
        let u = (edge.front().point() - center).normalize();
        let v = normal.cross(u).normalize();
        let plane = cad_core::Plane {
            origin: [center.x, center.y, center.z],
            u: [u.x, u.y, u.z],
            v: [v.x, v.y, v.z],
        };
        let mut carrier = project(&curve, &plane)?;
        carrier.slot_carrier = Some(cad_core::assembly_geometry::SlotCarrier::Circle);
        return Ok(carrier);
    }
    let first = source
        .control_points
        .iter()
        .map(|q| Vec3::from(*q) - p)
        .find(|q| q.norm() > 1e-10)
        .ok_or_else(|| unsupported("degenerate curve carrier"))?;
    let normal = source
        .control_points
        .iter()
        .map(|q| first.cross(&(Vec3::from(*q) - p)))
        .find(|q| q.norm() > 1e-10)
        .and_then(|n| n.try_normalize(1e-12))
        .ok_or_else(|| unsupported("curve has no provable planar frame"))?;
    if source
        .control_points
        .iter()
        .any(|q| (Vec3::from(*q) - p).dot(&normal).abs() > 1e-7)
    {
        return Err(unsupported("a Path or Cam model carrier must be planar"));
    }
    let u = first.normalize();
    let v = normal.cross(&u);
    let plane = cad_core::Plane {
        origin: p.into(),
        u: u.into(),
        v: v.into(),
    };
    project(&curve, &plane)
}
fn integrand(curve: &Carrier, t: f64) -> Result<Vec3<f64>> {
    let p = Vec3::from(curve.point(t)?) - Vec3::from(curve.plane.origin);
    let d = Vec3::from(curve.derivative(t)?);
    let u = Vec3::from(curve.plane.u);
    let v = Vec3::from(curve.plane.v);
    let (x, y) = (p.dot(&u), p.dot(&v));
    let (dx, dy) = (d.dot(&u), d.dot(&v));
    Ok(Vec3::new(
        (x * dy - y * dx) * 0.5,
        x * x * dy * 0.5,
        -y * y * dx * 0.5,
    ))
}
fn gauss(curve: &Carrier, a: f64, b: f64, high: bool) -> Result<Vec3<f64>> {
    let mid = (a + b) * 0.5;
    let half = (b - a) * 0.5;
    let nodes: &[(f64, f64)] = if high {
        &[
            (0., 0.5688888888888889),
            (-0.5384693101056831, 0.4786286704993665),
            (0.5384693101056831, 0.4786286704993665),
            (-0.906179845938664, 0.2369268850561891),
            (0.906179845938664, 0.2369268850561891),
        ]
    } else {
        &[
            (0., 0.8888888888888888),
            (-0.7745966692414834, 0.5555555555555556),
            (0.7745966692414834, 0.5555555555555556),
        ]
    };
    let mut sum = Vec3::zeros();
    for (t, w) in nodes {
        sum += integrand(curve, mid + half * t)? * (*w * half);
    }
    Ok(sum)
}
fn integrate(
    curve: &Carrier,
    a: f64,
    b: f64,
    tol: Vec3<f64>,
    depth: usize,
    budget: &mut usize,
) -> Result<(Vec3<f64>, Vec3<f64>)> {
    if *budget == 0 {
        return Err(unsupported(
            "profile centroid quadrature exceeded its evaluation budget",
        ));
    }
    *budget -= 1;
    let hi = gauss(curve, a, b, true)?;
    let lo = gauss(curve, a, b, false)?;
    let error = (hi - lo).map(f64::abs);
    if error.iter().zip(tol.iter()).all(|(e, t)| e <= t) {
        return Ok((hi, error));
    }
    if depth == 0 {
        return Err(unsupported(
            "profile centroid integral did not reach its f64 error estimate",
        ));
    }
    let mid = (a + b) * 0.5;
    let (left, le) = integrate(curve, a, mid, tol * 0.5, depth - 1, budget)?;
    let (right, re) = integrate(curve, mid, b, tol * 0.5, depth - 1, budget)?;
    Ok((left + right, le + re))
}
pub(crate) fn profile_center(face: &Face) -> Result<Frame> {
    if !matches!(face.surface(), Surface::Plane(_)) {
        return Err(unsupported(
            "Profile Center requires a planar face or closed planar sketch",
        ));
    }
    let mut plane = cad_plane(face);
    plane.origin = face
        .boundaries()
        .first()
        .and_then(|w| w.front().map(|e| e.front()))
        .map(|v| {
            let p = v.point();
            [p.x, p.y, p.z]
        })
        .ok_or_else(|| unsupported("closed profile has no boundary"))?;
    let mut carriers = Vec::new();
    let mut scale: f64 = 1.;
    let mut spans = 0;
    for edge in face.boundaries().iter().flatten() {
        let curve = project(&edge.oriented_curve(), &plane)?;
        if let CurveGeometry::Nurbs { curve: c } = &curve.geometry {
            let lo = c.weights.iter().copied().fold(f64::INFINITY, f64::min);
            let hi = c.weights.iter().copied().fold(0., f64::max);
            if c.degree > 32 || hi / lo > 1e4 {
                return Err(unsupported(
                    "profile centroid requires rational degree at most 32 and weight ratio at most 10000",
                ));
            }
            for p in &c.control_points {
                scale = scale.max(p[0].abs()).max(p[1].abs());
            }
            spans += c.knots.windows(2).filter(|w| w[0] < w[1]).count();
        }
        carriers.push(curve);
    }
    let tolerance = Vec3::new(
        1e-12 * scale * scale,
        1e-12 * scale.powi(3),
        1e-12 * scale.powi(3),
    ) / spans.max(1) as f64;
    let mut sum = Vec3::zeros();
    let mut error = Vec3::zeros();
    let mut budget = 65536;
    for curve in carriers {
        let CurveGeometry::Nurbs { curve: c } = &curve.geometry else {
            unreachable!()
        };
        let start = c.knots[c.degree];
        let end = c.knots[c.control_points.len()];
        for w in c
            .knots
            .windows(2)
            .filter(|w| w[0] < w[1] && w[0] >= start && w[1] <= end)
        {
            let (q, e) = integrate(
                &curve,
                (w[0] - start) / (end - start),
                (w[1] - start) / (end - start),
                tolerance,
                20,
                &mut budget,
            )?;
            sum += q;
            error += e;
        }
    }
    if !sum.iter().chain(error.iter()).all(|v| v.is_finite())
        || sum.x.abs() <= 1e-12 * scale * scale
        || sum.x.abs() <= error.x * 100.
    {
        return Err(unsupported(
            "profile area is degenerate or its centroid is ill-conditioned",
        ));
    }
    let xy = [sum.y / sum.x, sum.z / sum.x];
    let bound = (error.y.max(error.z) + xy[0].abs().max(xy[1].abs()) * error.x) / sum.x.abs();
    if bound > 1e-7 {
        return Err(unsupported(
            "profile centroid exceeded 1e-7 mm integration error estimate",
        ));
    }
    let p = std::array::from_fn(|i| plane.origin[i] + plane.u[i] * xy[0] + plane.v[i] * xy[1]);
    let n = Vec3::from(plane.u).cross(&Vec3::from(plane.v));
    Frame::new(p, n.into(), Some(plane.u), FrameKind::Profile)
}
