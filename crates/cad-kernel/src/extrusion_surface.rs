//! Exact extrusion caps cut from separable positive rational graph carriers.
//! Numerical inverse coordinates choose parameters; knot insertion preserves
//! the carrier itself. Acceptance uses every control, domain boundary and
//! rational weight, rather than sampled geometry or a tessellated substitute.
use cad_core::{CadError, Result};
use monstertruck::{geometry::prelude::TryIntoHomogeneousBsplineSurface, modeling::*};

const EPS: f64 = 1e-7;
fn unsupported(message: &str) -> CadError {
    CadError::Unsupported(format!("Extrusion surface termination: {message}"))
}
fn point(p: Vector4) -> Result<Point3> {
    if ![p.x, p.y, p.z, p.w].iter().all(|v| v.is_finite()) || p.w <= 0. {
        return Err(unsupported("finite positive rational weights are required"));
    }
    Ok(Point3::new(p.x / p.w, p.y / p.w, p.z / p.w))
}
fn straight(curve: &Curve) -> bool {
    let Ok(points) = crate::extrusion::curve_hull(curve) else {
        return false;
    };
    let (a, b) = curve.range_tuple();
    let origin = curve.subs(a);
    let delta = curve.subs(b) - origin;
    let l = delta.magnitude2();
    l > EPS * EPS
        && points.iter().all(|p| {
            let t = (*p - origin).dot(delta) / l;
            (-1e-9..=1. + 1e-9).contains(&t) && (*p - origin - delta * t).magnitude() <= EPS
        })
}
fn same_curve(a: &Curve, b: &Curve) -> bool {
    let compare = |a: &Curve, b: &Curve| {
        let (lo, hi) = a.range_tuple();
        let (bl, bh) = b.range_tuple();
        if a.subs(lo).distance(b.subs(bl)) > EPS || a.subs(hi).distance(b.subs(bh)) > EPS {
            return false;
        }
        if straight(a) && straight(b) {
            return true;
        }
        let mut a = a.lift_up();
        let mut b = b.lift_up();
        a.clamp();
        b.clamp();
        a.knot_normalize();
        b.knot_normalize();
        if a.degree() != b.degree()
            || a.knot_vector().len() != b.knot_vector().len()
            || a.control_points().len() != b.control_points().len()
            || a.knot_vector()
                .iter()
                .zip(b.knot_vector())
                .any(|(a, b)| (a - b).abs() > 1e-10)
        {
            return false;
        }
        let aw = a.control_points()[0].w;
        let bw = b.control_points()[0].w;
        if aw <= 0. || bw <= 0. {
            return false;
        }
        a.control_points()
            .iter()
            .zip(b.control_points())
            .all(|(a, b)| {
                point(*a)
                    .ok()
                    .zip(point(*b).ok())
                    .is_some_and(|(a, b)| a.distance(b) <= EPS)
                    && (a.w / aw - b.w / bw).abs()
                        <= 1e-10 * (a.w / aw).abs().max((b.w / bw).abs()).max(1.)
            })
    };
    compare(a, b) || compare(a, &b.inverse())
}
fn full_patch(face: &Face) -> Result<NurbsSurface<Vector4>> {
    let rational = face
        .surface()
        .try_into_homogeneous_bspline_surface()
        .ok_or_else(|| unsupported("the selected carrier has no exact rational surface form"))?;
    let surface = NurbsSurface::new(rational);
    if !surface.is_clamped()
        || surface.control_points().len() < 2
        || surface.control_points()[0].len() < 2
        || surface.control_points().len() * surface.control_points()[0].len() > 1024
    {
        return Err(unsupported(
            "a finite clamped surface patch within the control budget is required",
        ));
    }
    for p in surface.control_points().iter().flatten() {
        point(*p)?;
    }
    let wires = face.boundaries();
    if wires.len() != 1 || wires[0].len() != 4 {
        return Err(unsupported(
            "the selected face must retain its full four-sided carrier domain without holes or existing trims",
        ));
    }
    let expected = [
        Curve::NurbsCurve(surface.curve_u(0)),
        Curve::NurbsCurve(surface.curve_u(surface.control_points()[0].len() - 1)),
        Curve::NurbsCurve(surface.curve_v(0)),
        Curve::NurbsCurve(surface.curve_v(surface.control_points().len() - 1)),
    ];
    let mut matched = [false; 4];
    for edge in &wires[0] {
        let Some(index) = expected
            .iter()
            .enumerate()
            .position(|(i, curve)| !matched[i] && same_curve(&edge.curve(), curve))
        else {
            return Err(unsupported(
                "face boundaries do not prove the complete rational carrier domain",
            ));
        };
        matched[index] = true;
    }
    Ok(surface)
}
struct Rectangle {
    origin: Point3,
    u: Vector3,
    v: Vector3,
    width: f64,
    height: f64,
}
fn rectangle(face: &Face, axis: Vector3) -> Result<Rectangle> {
    let wires = face.boundaries();
    if !matches!(face.surface(), Surface::Plane(_))
        || wires.len() != 1
        || wires[0].len() != 4
        || wires[0].iter().any(|e| !straight(&e.curve()))
        || face
            .oriented_surface()
            .normal(0., 0.)
            .normalize()
            .cross(axis)
            .magnitude()
            > 1e-9
    {
        return Err(unsupported(
            "curved ends currently need a rectangular straight-edge section normal to the extrusion direction",
        ));
    }
    let points = wires[0]
        .iter()
        .map(|e| e.front().point())
        .collect::<Vec<_>>();
    let origin = points[0];
    let a = points[1] - origin;
    let b = points[3] - origin;
    let width = a.magnitude();
    let height = b.magnitude();
    if width <= EPS
        || height <= EPS
        || a.normalize().dot(b.normalize()).abs() > 1e-10
        || (points[2] - (origin + a + b)).magnitude() > EPS
    {
        return Err(unsupported(
            "the projected contour must be one nondegenerate rectangle",
        ));
    }
    Ok(Rectangle {
        origin,
        u: a / width,
        v: b / height,
        width,
        height,
    })
}
fn trim(surface: &NurbsSurface<Vector4>, u: [f64; 2], v: [f64; 2]) -> NurbsSurface<Vector4> {
    let mut patch = surface.clone();
    let ur = [
        patch.knot_vector_u()[0],
        *patch.knot_vector_u().last().unwrap(),
    ];
    let vr = [
        patch.knot_vector_v()[0],
        *patch.knot_vector_v().last().unwrap(),
    ];
    if u[0] > ur[0] {
        patch = patch.cut_u(u[0]);
    }
    if u[1] < ur[1] {
        patch.cut_u(u[1]);
    }
    if v[0] > vr[0] {
        patch = patch.cut_v(v[0]);
    }
    if v[1] < vr[1] {
        patch.cut_v(v[1]);
    }
    patch
}
fn monotone(values: &[f64]) -> Option<f64> {
    let change = values.last()? - values.first()?;
    if change.abs() <= EPS
        || values
            .windows(2)
            .any(|p| (p[1] - p[0]) * change < -EPS * change.abs())
    {
        None
    } else {
        Some(change.signum())
    }
}
fn graph(surface: &NurbsSurface<Vector4>, rect: &Rectangle) -> Result<bool> {
    let net = surface.control_points();
    let base = net[0][0].w;
    let mut x = Vec::new();
    let mut y = Vec::new();
    for row in net {
        x.push((point(row[0])? - rect.origin).dot(rect.u));
    }
    for p in &net[0] {
        y.push((point(*p)? - rect.origin).dot(rect.v));
    }
    if monotone(&x).is_none() || monotone(&y).is_none() {
        return Ok(false);
    }
    for (i, row) in net.iter().enumerate() {
        for (j, h) in row.iter().enumerate() {
            let p = point(*h)? - rect.origin;
            let ratio = h.w / net[i][0].w;
            let expected = net[0][j].w / base;
            if (p.dot(rect.u) - x[i]).abs() > EPS
                || (p.dot(rect.v) - y[j]).abs() > EPS
                || (ratio - expected).abs() > 1e-10 * ratio.abs().max(expected.abs()).max(1.)
            {
                return Ok(false);
            }
        }
    }
    // A monotone control sequence can still contain a constant whole knot
    // span. Endpoint differences on each proven monotone span exclude those
    // collapsed chart intervals without a sampled invertibility decision.
    let mut us = surface.knot_vector_u().iter().copied().collect::<Vec<_>>();
    us.dedup();
    let mut vs = surface.knot_vector_v().iter().copied().collect::<Vec<_>>();
    vs.dedup();
    if us.windows(2).any(|p| {
        (surface.subs(p[1], vs[0]) - surface.subs(p[0], vs[0]))
            .dot(rect.u)
            .abs()
            <= 1e-12
    }) || vs.windows(2).any(|p| {
        (surface.subs(us[0], p[1]) - surface.subs(us[0], p[0]))
            .dot(rect.v)
            .abs()
            <= 1e-12
    }) {
        return Ok(false);
    }
    Ok(true)
}
fn inverse(
    surface: &NurbsSurface<Vector4>,
    rect: &Rectangle,
    axis: usize,
    value: f64,
) -> Result<f64> {
    let u = [
        surface.knot_vector_u()[0],
        *surface.knot_vector_u().last().unwrap(),
    ];
    let v = [
        surface.knot_vector_v()[0],
        *surface.knot_vector_v().last().unwrap(),
    ];
    let range = if axis == 0 { u } else { v };
    let direction = if axis == 0 { rect.u } else { rect.v };
    let coordinate = |t| {
        let p = if axis == 0 {
            surface.subs(t, (v[0] + v[1]) * 0.5)
        } else {
            surface.subs((u[0] + u[1]) * 0.5, t)
        };
        (p - rect.origin).dot(direction)
    };
    let a = coordinate(range[0]);
    let b = coordinate(range[1]);
    if !a.is_finite() || !b.is_finite() || value < a.min(b) - EPS || value > a.max(b) + EPS {
        return Err(unsupported(
            "the section lies outside this proven monotone patch",
        ));
    }
    if (value - a).abs() <= 1e-10 {
        return Ok(range[0]);
    }
    if (value - b).abs() <= 1e-10 {
        return Ok(range[1]);
    }
    let mut lo = range[0];
    let mut hi = range[1];
    let increasing = b > a;
    for _ in 0..96 {
        let mid = (lo + hi) * 0.5;
        let at = coordinate(mid);
        if !at.is_finite() {
            return Err(unsupported("the inverse surface coordinate is non-finite"));
        }
        if (at < value) == increasing {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    let result = (lo + hi) * 0.5;
    if (coordinate(result) - value).abs() > 1e-9 {
        return Err(unsupported(
            "the exact carrier's inverse coordinate did not converge within geometric precision",
        ));
    }
    Ok(result)
}
fn patch_face(mut surface: NurbsSurface<Vector4>, axis: Vector3) -> Result<Face> {
    surface.knot_normalize();
    let rows = surface.control_points().len();
    let cols = surface.control_points()[0].len();
    let vertices = [
        builder::vertex(point(surface.control_points()[0][0])?),
        builder::vertex(point(surface.control_points()[rows - 1][0])?),
        builder::vertex(point(surface.control_points()[rows - 1][cols - 1])?),
        builder::vertex(point(surface.control_points()[0][cols - 1])?),
    ];
    let wire = Wire::from(vec![
        Edge::try_new(
            &vertices[0],
            &vertices[1],
            Curve::NurbsCurve(surface.curve_u(0)),
        )
        .map_err(|e| unsupported(&e.to_string()))?,
        Edge::try_new(
            &vertices[1],
            &vertices[2],
            Curve::NurbsCurve(surface.curve_v(rows - 1)),
        )
        .map_err(|e| unsupported(&e.to_string()))?,
        Edge::try_new(
            &vertices[2],
            &vertices[3],
            Curve::NurbsCurve(surface.curve_u(cols - 1).inverse()),
        )
        .map_err(|e| unsupported(&e.to_string()))?,
        Edge::try_new(
            &vertices[3],
            &vertices[0],
            Curve::NurbsCurve(surface.curve_v(0).inverse()),
        )
        .map_err(|e| unsupported(&e.to_string()))?,
    ]);
    let mut face = Face::try_new(vec![wire], Surface::NurbsSurface(surface))
        .map_err(|e| unsupported(&e.to_string()))?;
    if face.oriented_surface().normal(0.5, 0.5).dot(axis) < 0. {
        face.invert();
    }
    crate::validation::surface(&Shell::from(vec![face.clone()]))?;
    Ok(face)
}
fn separated_cap(mut patch: NurbsSurface<Vector4>, start: &Face, axis: Vector3) -> Result<Face> {
    for _ in 0..5 {
        let cap = patch_face(patch.clone(), axis)?;
        match crate::extrusion::between_caps(start, &cap, axis) {
            Ok(_) => return Ok(cap),
            Err(CadError::Unsupported(_)) => {}
            Err(error) => return Err(error),
        }
        // Exact knot insertion tightens a conservative envelope while retaining
        // the same rational carrier. It never changes the authored geometry.
        let mut us = patch.knot_vector_u().iter().copied().collect::<Vec<_>>();
        us.dedup();
        let mut vs = patch.knot_vector_v().iter().copied().collect::<Vec<_>>();
        vs.dedup();
        if (patch.control_points().len() + us.len() - 1)
            * (patch.control_points()[0].len() + vs.len() - 1)
            > 1024
        {
            break;
        }
        for span in us.windows(2) {
            patch.add_knot_u((span[0] + span[1]) * 0.5);
        }
        for span in vs.windows(2) {
            patch.add_knot_v((span[0] + span[1]) * 0.5);
        }
    }
    Err(unsupported(
        "the complete positive surface envelope does not prove a forward cap separated from the start",
    ))
}
pub(crate) fn cap(section: &Face, start: &Face, axis: Vector3, target: &Face) -> Result<Face> {
    let rect = rectangle(section, axis)?;
    let mut surface = full_patch(target)?;
    // A surface may use either parameter order; try the transposed exact net.
    let mut candidates: Vec<(Vec<u8>, Face)> = Vec::new();
    for _ in 0..2 {
        let mut us = surface.knot_vector_u().iter().copied().collect::<Vec<_>>();
        us.dedup();
        let mut vs = surface.knot_vector_v().iter().copied().collect::<Vec<_>>();
        vs.dedup();
        if us.len() * us.len() * vs.len() * vs.len() > 4096 {
            return Err(unsupported(
                "carrier branch partition exceeds the exact termination budget",
            ));
        }
        for a in 0..us.len() - 1 {
            for b in a + 1..us.len() {
                for c in 0..vs.len() - 1 {
                    for d in c + 1..vs.len() {
                        let patch = trim(&surface, [us[a], us[b]], [vs[c], vs[d]]);
                        if !graph(&patch, &rect)? {
                            continue;
                        }
                        let (Ok(x0), Ok(x1), Ok(y0), Ok(y1)) = (
                            inverse(&patch, &rect, 0, 0.),
                            inverse(&patch, &rect, 0, rect.width),
                            inverse(&patch, &rect, 1, 0.),
                            inverse(&patch, &rect, 1, rect.height),
                        ) else {
                            continue;
                        };
                        let patch =
                            trim(&patch, [x0.min(x1), x0.max(x1)], [y0.min(y1), y0.max(y1)]);
                        if !graph(&patch, &rect)? {
                            continue;
                        }
                        let cap = match separated_cap(patch, start, axis) {
                            Ok(cap) => cap,
                            Err(CadError::Unsupported(_) | CadError::Invalid(_)) => continue,
                            Err(error) => return Err(error),
                        };
                        let key = serde_json::to_vec(&cap.surface())?;
                        if !candidates.iter().any(|(existing, _)| *existing == key) {
                            candidates.push((key, cap));
                        }
                    }
                }
            }
        }
        surface.swap_axes();
    }
    if candidates.is_empty() {
        return Err(unsupported(
            "no full forward monotone surface patch covers this rectangular section",
        ));
    }
    for index in 0..candidates.len() {
        if candidates.iter().enumerate().all(|(other, (_, face))| {
            other == index
                || crate::extrusion::cap_precedes(&candidates[index].1, face, axis).unwrap_or(false)
        }) {
            return Ok(candidates.remove(index).1);
        }
    }
    Err(unsupported(
        "carrier branches overlap; select a separately bounded surface patch",
    ))
}

/// Complete containment on a convex planar face, using exact positive hulls or
/// the rational polynomial circle recognizer. Inner trims must be disjoint.
pub(crate) fn covered(target: &Face, cap: &Face) -> Result<bool> {
    let boundaries = target.boundaries();
    let Some(outer) = boundaries.first() else {
        return Ok(false);
    };
    let Some(origin) = target.vertex_iter().next().map(|v| v.point()) else {
        return Ok(false);
    };
    let normal = target.oriented_surface().normal(0., 0.).normalize();
    if !crate::extrusion::face_hull(target)?
        .iter()
        .all(|p| (*p - origin).dot(normal).abs() <= EPS)
    {
        return Ok(false);
    }
    let hull = crate::extrusion::face_hull(cap)?;
    let normal = target.oriented_surface().normal(0., 0.).normalize();
    let polygon = outer.iter().all(|edge| straight(&edge.curve()));
    let contained = if polygon {
        let points = outer.iter().map(|e| e.front().point()).collect::<Vec<_>>();
        points.len() >= 3
            && outer.iter().all(|edge| {
                let inward = normal
                    .cross(edge.back().point() - edge.front().point())
                    .normalize();
                points
                    .iter()
                    .all(|p| (*p - edge.front().point()).dot(inward) >= -EPS)
                    && hull
                        .iter()
                        .all(|p| (*p - edge.front().point()).dot(inward) >= -EPS)
            })
    } else if outer.len() == 1 && outer[0].front().point().distance(outer[0].back().point()) <= EPS
    {
        let Ok((center, _, radius)) = crate::axis_geometry::circle_geometry(&outer[0].curve())
        else {
            return Ok(false);
        };
        cap.edge_iter().all(|edge| {
            if let Ok((c, _, r)) = crate::axis_geometry::circle_geometry(&edge.curve()) {
                (c - center).magnitude() + r <= radius + EPS
            } else {
                crate::extrusion::curve_hull(&edge.curve()).is_ok_and(|points| {
                    points
                        .iter()
                        .all(|p| (*p - center).magnitude() <= radius + EPS)
                })
            }
        })
    } else {
        false
    };
    if !contained {
        return Ok(false);
    }
    for hole in boundaries.iter().skip(1) {
        let controls = hole
            .iter()
            .map(|edge| crate::extrusion::curve_hull(&edge.curve()))
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();
        if !bounds_disjoint(&controls, &hull, EPS) {
            return Ok(false);
        }
    }
    Ok(true)
}
pub(crate) fn bounds_disjoint(a: &[Point3], b: &[Point3], margin: f64) -> bool {
    !a.is_empty()
        && !b.is_empty()
        && (0..3).any(|i| {
            a.iter().map(|p| p[i]).fold(f64::NEG_INFINITY, f64::max)
                < b.iter().map(|p| p[i]).fold(f64::INFINITY, f64::min) - margin
                || b.iter().map(|p| p[i]).fold(f64::NEG_INFINITY, f64::max)
                    < a.iter().map(|p| p[i]).fold(f64::INFINITY, f64::min) - margin
        })
}
