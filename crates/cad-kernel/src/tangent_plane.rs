//! Tangent frames from proven bounded native face domains. Display meshes and
//! intersection guides never supply anchors, containment, or derivatives.
use cad_core::{CadError, Plane as DatumPlane, Result, reference_geometry::TangentAnchorValue};
use monstertruck::{geometry::prelude::TryIntoHomogeneousBsplineSurface, modeling::*};

const EPS: f64 = 1e-7;
const ANGLE_EPS: f64 = 1e-9;
fn unsupported(message: &str) -> CadError {
    CadError::Unsupported(format!("Tangent plane: {message}"))
}
fn invalid(message: &str) -> CadError {
    CadError::Invalid(format!("Tangent plane: {message}"))
}
fn finite(point: Point3) -> bool {
    [point.x, point.y, point.z].into_iter().all(f64::is_finite)
}
fn unit(vector: Vector3) -> Result<Vector3> {
    let length = vector.magnitude();
    if ![vector.x, vector.y, vector.z, length]
        .into_iter()
        .all(f64::is_finite)
        || length <= 1e-12
    {
        return Err(invalid(
            "The native surface derivative is collapsed or nonfinite",
        ));
    }
    Ok(vector / length)
}
fn frame(point: Point3, du: Vector3, dv: Vector3, orientation: bool) -> Result<DatumPlane> {
    if !finite(point) {
        return Err(invalid("The native surface anchor is nonfinite"));
    }
    let u = unit(du)?;
    let v = unit(dv)?;
    let mut normal = u.cross(v);
    if normal.magnitude() <= 1e-10 {
        return Err(invalid(
            "The native surface anchor has a singular or ill-conditioned tangent",
        ));
    }
    normal = unit(normal)?;
    if !orientation {
        normal = -normal;
    }
    let v = normal.cross(u);
    Ok(DatumPlane {
        origin: [point.x, point.y, point.z],
        u: [u.x, u.y, u.z],
        v: [v.x, v.y, v.z],
    })
}
fn parameters(anchor: TangentAnchorValue) -> Result<[f64; 2]> {
    let TangentAnchorValue::Parameters(values) = anchor else {
        return Err(unsupported(
            "An associative point anchor requires a proven one-to-one face inverse; use normalized face parameters on this rational patch",
        ));
    };
    if values
        .into_iter()
        .any(|value| !value.is_finite() || !(0. ..=1.).contains(&value))
    {
        return Err(invalid(
            "Normalized face parameters must be finite and in 0..1",
        ));
    }
    Ok(values)
}
fn same_boundary(a: &Curve, b: &Curve) -> bool {
    if !matches!(
        a,
        Curve::Line(_) | Curve::BsplineCurve(_) | Curve::NurbsCurve(_)
    ) {
        return false;
    }
    let compare = |b: &Curve| {
        let (a0, a1) = a.range_tuple();
        let (b0, b1) = b.range_tuple();
        if ![a0, a1, b0, b1].into_iter().all(f64::is_finite)
            || a1 <= a0
            || b1 <= b0
            || ![a.subs(a0), a.subs(a1), b.subs(b0), b.subs(b1)]
                .into_iter()
                .all(finite)
            || a.subs(a0).distance(b.subs(b0)) > EPS
            || a.subs(a1).distance(b.subs(b1)) > EPS
        {
            return false;
        }
        if crate::bounded_carriers::line(a).is_ok() && crate::bounded_carriers::line(b).is_ok() {
            return true;
        }
        crate::surfaces::same_curve(a, b, EPS)
    };
    compare(b) || compare(&b.inverse())
}
/// Prove that retained native edges are exactly the four natural boundaries.
/// This geometric-domain proof is also used by strict STEP surface import; it
/// does not infer UV trims from tessellation or a nearest-point fit.
pub(crate) fn full_rational_patch(face: &Face) -> Result<NurbsSurface<Vector4>> {
    let surface = face.surface();
    if !matches!(
        surface,
        Surface::BsplineSurface(_) | Surface::NurbsSurface(_)
    ) {
        return Err(unsupported(
            "Select a full rational patch, a rectangular planar face, or a bounded cylindrical/conical revolution face",
        ));
    }
    let rational = surface
        .try_into_homogeneous_bspline_surface()
        .ok_or_else(|| unsupported("The selected carrier has no exact rational surface form"))?;
    let patch = NurbsSurface::new(rational);
    let rows = patch.control_points();
    if !patch.is_clamped()
        || rows.len() < 2
        || rows[0].len() < 2
        || rows
            .len()
            .checked_mul(rows[0].len())
            .is_none_or(|size| size > 1024)
        || rows.iter().any(|row| row.len() != rows[0].len())
    {
        return Err(unsupported(
            "A clamped rectangular rational patch within 1024 controls is required",
        ));
    }
    for point in rows.iter().flatten() {
        if ![point.x, point.y, point.z, point.w]
            .into_iter()
            .all(f64::is_finite)
            || point.w <= 0.
            || !finite(Point3::new(
                point.x / point.w,
                point.y / point.w,
                point.z / point.w,
            ))
        {
            return Err(invalid(
                "The native patch needs finite positive homogeneous controls",
            ));
        }
    }
    if patch
        .knot_vector_u()
        .iter()
        .chain(patch.knot_vector_v().iter())
        .any(|value| !value.is_finite())
    {
        return Err(invalid("The native patch has nonfinite interior knots"));
    }
    let wires = face.boundaries();
    if wires.len() != 1 || wires[0].len() != 4 {
        return Err(unsupported(
            "Rational tangent faces must retain their full four-sided carrier domain, without holes or existing trims",
        ));
    }
    let expected = natural_boundaries(&patch);
    let mut matched = [false; 4];
    for edge in &wires[0] {
        let matches = expected
            .iter()
            .enumerate()
            .filter(|(index, curve)| !matched[*index] && same_boundary(&edge.curve(), curve))
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        if matches.len() != 1 {
            return Err(unsupported(
                "The face boundary does not uniquely prove the complete rational parameter domain",
            ));
        }
        matched[matches[0]] = true;
    }
    let ((u0, u1), (v0, v1)) = patch.range_tuple();
    if ![u0, u1, v0, v1, u1 - u0, v1 - v0]
        .into_iter()
        .all(f64::is_finite)
        || u1 <= u0
        || v1 <= v0
    {
        return Err(invalid(
            "The native patch has no finite increasing parameter domain",
        ));
    }
    Ok(patch)
}
fn natural_boundaries(patch: &NurbsSurface<Vector4>) -> [Curve; 4] {
    let rows = patch.control_points();
    [
        Curve::NurbsCurve(patch.curve_u(0)),
        Curve::NurbsCurve(patch.curve_u(rows[0].len() - 1)),
        Curve::NurbsCurve(patch.curve_v(0)),
        Curve::NurbsCurve(patch.curve_v(rows.len() - 1)),
    ]
}
/// Only a very small inverse roundoff band can snap to a proven boundary.
/// Callers must also check the snapped native point against the authored point.
fn bounded_parameter(value: f64, range: [f64; 2]) -> Result<f64> {
    let span = range[1] - range[0];
    if ![value, range[0], range[1], span]
        .into_iter()
        .all(f64::is_finite)
        || span <= 0.
    {
        return Err(invalid(
            "The native inverse parameter is nonfinite or unbounded",
        ));
    }
    let fraction = (value - range[0]) / span;
    if !fraction.is_finite() || !(-ANGLE_EPS..=1. + ANGLE_EPS).contains(&fraction) {
        return Err(invalid(
            "The associative point lies outside the bounded native parameter domain",
        ));
    }
    Ok(if fraction.abs() <= ANGLE_EPS {
        range[0]
    } else if (fraction - 1.).abs() <= ANGLE_EPS {
        range[1]
    } else {
        value
    })
}
fn rational(face: &Face, anchor: TangentAnchorValue) -> Result<DatumPlane> {
    let patch = full_rational_patch(face)?;
    let ((u0, u1), (v0, v1)) = patch.range_tuple();
    let (u, v) = match anchor {
        TangentAnchorValue::Parameters(values) => {
            let values = parameters(TangentAnchorValue::Parameters(values))?;
            (u0 + (u1 - u0) * values[0], v0 + (v1 - v0) * values[1])
        }
        TangentAnchorValue::Point(values) => {
            // The axis recognizer proves positive proportional radial controls
            // and a genuinely linear axial ruling. Strict circle traversal
            // then proves an injective angular interval, so the native inverse
            // cannot choose a different material location on a folded patch.
            crate::axis_geometry::face_axis(face).map_err(|_| unsupported("Associative point anchors on rational patches require a proven cylindrical/conical one-to-one carrier; use normalized parameters for general patches"))?;
            let mut lines = 0;
            let mut circles = 0;
            for curve in &natural_boundaries(&patch) {
                if crate::bounded_carriers::line(curve).is_ok() {
                    lines += 1;
                } else {
                    crate::bounded_carriers::circle(curve)?;
                    circles += 1;
                }
            }
            if lines != 2 || circles != 2 {
                return Err(unsupported(
                    "A rational point anchor requires two monotonic straight rulings and two bounded circular sections",
                ));
            }
            let point = Point3::from(values);
            if !finite(point) {
                return Err(invalid("The associative point is nonfinite"));
            }
            let (u, v) = patch
                .search_parameter(point, Some((u0 + (u1 - u0) / 2., v0 + (v1 - v0) / 2.)), 100)
                .ok_or_else(|| {
                    invalid("The associative point has no converged native surface inverse")
                })?;
            let u = bounded_parameter(u, [u0, u1])?;
            let v = bounded_parameter(v, [v0, v1])?;
            let evaluated = patch.subs(u, v);
            if !finite(evaluated) || evaluated.distance(point) > EPS {
                return Err(invalid(
                    "The associative point lies outside the bounded rational face",
                ));
            }
            (u, v)
        }
    };
    frame(
        patch.subs(u, v),
        patch.uder(u, v),
        patch.vder(u, v),
        face.orientation(),
    )
}
fn planar(face: &Face, anchor: TangentAnchorValue) -> Result<DatumPlane> {
    let wires = face.boundaries();
    if wires.len() != 1 || wires[0].len() != 4 {
        return Err(unsupported(
            "Planar tangent anchors currently require one exact rectangular boundary",
        ));
    }
    for edge in &wires[0] {
        crate::bounded_carriers::line(&edge.curve())?;
    }
    let points = wires[0]
        .iter()
        .map(|edge| edge.front().point())
        .collect::<Vec<_>>();
    if points.iter().any(|point| !finite(*point)) {
        return Err(invalid("The planar boundary has nonfinite vertices"));
    }
    let a = points[1] - points[0];
    let b = points[3] - points[0];
    let u = unit(a)?;
    let v = unit(b)?;
    let width = a.magnitude();
    let height = b.magnitude();
    if width <= EPS
        || height <= EPS
        || u.dot(v).abs() > 1e-10
        || points[2].distance(points[0] + a + b) > EPS
    {
        return Err(unsupported(
            "The planar boundary must prove a nondegenerate rectangle",
        ));
    }
    let plane = face.oriented_surface();
    let plane_origin = plane.subs(0., 0.);
    let normal = unit(unit(plane.uder(0., 0.))?.cross(unit(plane.vder(0., 0.))?))?;
    if !finite(plane_origin)
        || points
            .iter()
            .any(|point| (*point - plane_origin).dot(normal).abs() > EPS)
        || u.dot(normal).abs() > 1e-10
        || v.dot(normal).abs() > 1e-10
    {
        return Err(invalid(
            "The rectangular boundary is inconsistent with its native plane",
        ));
    }
    let point = match anchor {
        TangentAnchorValue::Parameters(values) => {
            let values = parameters(TangentAnchorValue::Parameters(values))?;
            points[0] + a * values[0] + b * values[1]
        }
        TangentAnchorValue::Point(value) => {
            let point = Point3::from(value);
            let delta = point - points[0];
            let x = delta.dot(u) / width;
            let y = delta.dot(v) / height;
            let x = bounded_parameter(x, [0., 1.])?;
            let y = bounded_parameter(y, [0., 1.])?;
            let bounded = points[0] + a * x + b * y;
            let evaluated = bounded - normal * (bounded - plane_origin).dot(normal);
            if !finite(point) || !finite(evaluated) || evaluated.distance(point) > EPS {
                return Err(invalid(
                    "The associative anchor is outside the bounded planar face",
                ));
            }
            evaluated
        }
    };
    // Evaluate the actual native plane anchor and derivative directions after
    // bounding it with the exact rectangular trim; no boundary mesh supplies it.
    let point = point - normal * (point - plane_origin).dot(normal);
    let du = a - normal * a.dot(normal);
    let dv = b - normal * b.dot(normal);
    frame(point, du, dv, unit(du)?.cross(unit(dv)?).dot(normal) >= 0.)
}

struct RevolutionDomain {
    surface: Surface,
    inverse: Matrix4,
    origin: Point3,
    axis: Vector3,
    generator: Line<Point3>,
    axial_start: f64,
    axial_change: f64,
    orientation: bool,
    u: [f64; 2],
    v: [f64; 2],
}
impl RevolutionDomain {
    fn coordinates(&self, point: Point3) -> Result<[f64; 2]> {
        let point = self.inverse.transform_point(point);
        if !finite(point) {
            return Err(invalid(
                "The native revolution inverse exceeds finite coordinates",
            ));
        }
        let delta = point - self.origin;
        let u = (delta.dot(self.axis) - self.axial_start) / self.axial_change;
        let base = self.generator.subs(u) - self.origin;
        let base = base - self.axis * base.dot(self.axis);
        let radial = delta - self.axis * delta.dot(self.axis);
        let length = base.magnitude();
        let radius = radial.magnitude();
        if !u.is_finite()
            || !length.is_finite()
            || !radius.is_finite()
            || length <= EPS
            || radius <= EPS
        {
            return Err(invalid(
                "The anchor reaches a collapsed or nonfinite rotational section",
            ));
        }
        let base = base / length;
        let radial = radial / radius;
        let angle = self.axis.dot(base.cross(radial)).atan2(base.dot(radial));
        if !angle.is_finite() {
            return Err(invalid("The native revolution angle is nonfinite"));
        }
        Ok([u, angle])
    }
    fn point(&self, u: f64, v: f64) -> Point3 {
        if self.orientation {
            self.surface.subs(u, v)
        } else {
            self.surface.subs(v, u)
        }
    }
    fn anchor(&self, anchor: TangentAnchorValue) -> Result<[f64; 2]> {
        let result = match anchor {
            TangentAnchorValue::Parameters(values) => {
                let values = parameters(TangentAnchorValue::Parameters(values))?;
                let values = if self.orientation {
                    values
                } else {
                    [values[1], values[0]]
                };
                [
                    self.u[0] + (self.u[1] - self.u[0]) * values[0],
                    self.v[0] + (self.v[1] - self.v[0]) * values[1],
                ]
            }
            TangentAnchorValue::Point(value) => {
                let point = Point3::from(value);
                if !finite(point) {
                    return Err(invalid("The associative anchor is nonfinite"));
                }
                let [u, v] = self.coordinates(point)?;
                let u = bounded_parameter(u, self.u)?;
                let v = unwrapped(v, self.v)?;
                let evaluated = self.point(u, v);
                if !finite(evaluated) || evaluated.distance(point) > EPS {
                    return Err(invalid(
                        "The associative anchor is outside the bounded native revolution face",
                    ));
                }
                [u, v]
            }
        };
        Ok(if self.orientation {
            result
        } else {
            [result[1], result[0]]
        })
    }
}
fn unwrapped(angle: f64, range: [f64; 2]) -> Result<f64> {
    // Round to the nearest occurrence of the lower boundary first. Ceil would
    // send a one-ULP-low boundary point to the next complete turn.
    let candidate =
        angle + ((range[0] - angle) / std::f64::consts::TAU).round() * std::f64::consts::TAU;
    let mut result = None;
    for value in [
        candidate,
        candidate + std::f64::consts::TAU,
        candidate - std::f64::consts::TAU,
    ] {
        if let Ok(value) = bounded_parameter(value, range) {
            if result.is_some_and(|old| old != value) {
                return Err(invalid(
                    "The native angular inverse is ambiguous inside the selected face",
                ));
            }
            result = Some(value);
        }
    }
    result.ok_or_else(|| invalid("The angular anchor is outside the bounded face"))
}
fn boundary_angle(angle: f64, range: [f64; 2]) -> Result<f64> {
    let candidate =
        angle + ((range[0] - angle) / std::f64::consts::TAU).round() * std::f64::consts::TAU;
    for value in [
        candidate,
        candidate + std::f64::consts::TAU,
        candidate - std::f64::consts::TAU,
    ] {
        if (value - range[0]).abs() <= ANGLE_EPS {
            return Ok(range[0]);
        }
        if (value - range[1]).abs() <= ANGLE_EPS {
            return Ok(range[1]);
        }
    }
    Err(unsupported(
        "The rotational trim boundary does not prove one angular rectangle",
    ))
}
fn revolution_domain(face: &Face) -> Result<RevolutionDomain> {
    let surface = face.surface();
    let Surface::RevolutionSurface(processor) = &surface else {
        return Err(unsupported("Select an analytic revolution carrier"));
    };
    let Curve::Line(generator) = processor.entity().entity_curve() else {
        return Err(unsupported(
            "Tangent revolution trims currently require a cylindrical/conical straight generatrix",
        ));
    };
    crate::axis_geometry::face_axis(face)?;
    let matrix = processor.transform();
    // Exact affine coordinates are necessary: a nearly projective transform
    // cannot use affine inverse parameters or affine derivative identities.
    if matrix.x.w != 0. || matrix.y.w != 0. || matrix.z.w != 0. || matrix.w.w != 1. {
        return Err(unsupported(
            "A genuinely affine native revolution processor is required",
        ));
    }
    let inverse = matrix
        .invert()
        .ok_or_else(|| invalid("The native revolution transform is singular"))?;
    let inverse_values: [[f64; 4]; 4] = inverse.into();
    if inverse_values
        .iter()
        .flatten()
        .any(|value| !value.is_finite())
    {
        return Err(invalid("The native revolution inverse is nonfinite"));
    }
    let axis = unit(processor.entity().axis())?;
    let origin = processor.entity().origin();
    if !finite(origin) || !finite(generator.0) || !finite(generator.1) {
        return Err(invalid("The native revolution source is nonfinite"));
    }
    let a = generator.0 - origin;
    let b = generator.1 - origin;
    let radial_a = a - axis * a.dot(axis);
    let radial_b = b - axis * b.dot(axis);
    let radius_a = radial_a.magnitude();
    let radius_b = radial_b.magnitude();
    let axial_change = (generator.1 - generator.0).dot(axis);
    if ![radius_a, radius_b, axial_change]
        .into_iter()
        .all(f64::is_finite)
        || radius_a <= EPS
        || radius_b <= EPS
        || axial_change.abs() <= EPS
        || unit(radial_a)?.cross(unit(radial_b)?).magnitude() > 1e-10
        || radial_a.dot(radial_b) <= 0.
    {
        return Err(unsupported(
            "The generatrix must prove a regular cylinder/conical frustum, without an apex or radial reversal",
        ));
    }
    let wires = face.boundaries();
    if wires.len() != 1 || wires[0].len() != 4 {
        return Err(unsupported(
            "Rotational tangent faces need two straight generators and two bounded circular trim arcs, without holes",
        ));
    }
    let mut domain = RevolutionDomain {
        surface: surface.clone(),
        inverse,
        origin,
        axis,
        generator: *generator,
        axial_start: a.dot(axis),
        axial_change,
        orientation: processor.orientation(),
        u: [0.; 2],
        v: [0.; 2],
    };
    let points = wires[0]
        .iter()
        .map(|edge| edge.front().point())
        .collect::<Vec<_>>();
    let corners = points
        .iter()
        .map(|point| domain.coordinates(*point))
        .collect::<Result<Vec<_>>>()?;
    domain.u = [
        corners.iter().map(|uv| uv[0]).fold(f64::INFINITY, f64::min),
        corners
            .iter()
            .map(|uv| uv[0])
            .fold(f64::NEG_INFINITY, f64::max),
    ];
    if domain.u[0] < -ANGLE_EPS
        || domain.u[1] > 1. + ANGLE_EPS
        || domain.u[1] - domain.u[0] <= ANGLE_EPS
    {
        return Err(unsupported(
            "Rotational trim vertices must lie in one bounded generatrix interval",
        ));
    }
    domain.u[0] = domain.u[0].max(0.);
    domain.u[1] = domain.u[1].min(1.);
    let mut circles = Vec::new();
    let mut lines = Vec::new();
    for edge in &wires[0] {
        let curve = edge.oriented_curve();
        if crate::bounded_carriers::line(&curve).is_ok() {
            lines.push(curve);
        } else {
            circles.push((curve.clone(), crate::bounded_carriers::circle(&curve)?));
        }
    }
    if circles.len() != 2 || lines.len() != 2 {
        return Err(unsupported(
            "The face must prove exactly two straight and two circular trim carriers",
        ));
    }
    let world_axis = unit(matrix.transform_vector(axis))?;
    let mut circular_domains = Vec::new();
    for (curve, ((center, normal, radius), sweep)) in &circles {
        let (a, b) = curve.range_tuple();
        let p = curve.subs(a);
        let q = curve.subs(b);
        let [u, v] = domain.coordinates(p)?;
        let [ub, _] = domain.coordinates(q)?;
        let local_point = inverse.transform_point(p);
        let local_tangent = inverse.transform_vector(curve.der(a));
        let radial = local_point - origin - axis * (local_point - origin).dot(axis);
        let direction = unit(radial)?.cross(unit(local_tangent)?).dot(axis);
        let generator_point = generator.subs(u);
        let expected_center =
            matrix.transform_point(origin + axis * (generator_point - origin).dot(axis));
        let expected_radius = matrix
            .transform_vector(
                generator_point - origin - axis * (generator_point - origin).dot(axis),
            )
            .magnitude();
        if (u - ub).abs() > ANGLE_EPS
            || normal.cross(world_axis).magnitude() > 1e-10
            || center.distance(expected_center) > EPS
            || (radius - expected_radius).abs() > EPS
            || !direction.is_finite()
            || direction.abs() < 1e-9
        {
            return Err(unsupported(
                "The circular trim carrier is inconsistent with the native rotational section",
            ));
        }
        let end = v + sweep.abs() * direction.signum();
        circular_domains.push((u, [v.min(end), v.max(end)]));
    }
    domain.v = circular_domains[0].1;
    let span = domain.v[1] - domain.v[0];
    if !span.is_finite() || span <= ANGLE_EPS || span >= std::f64::consts::TAU - ANGLE_EPS {
        return Err(unsupported(
            "The selected rotational face must have a unique nonclosed angular interval",
        ));
    }
    let mut axial_sides = [false; 2];
    for (u, range) in circular_domains {
        let side = if (u - domain.u[0]).abs() <= ANGLE_EPS {
            0
        } else if (u - domain.u[1]).abs() <= ANGLE_EPS {
            1
        } else {
            return Err(unsupported(
                "Circular trim arcs do not bound the selected generatrix interval",
            ));
        };
        if axial_sides[side]
            || (range[1] - range[0] - span).abs() > ANGLE_EPS
            || boundary_angle(range[0], domain.v)? != domain.v[0]
            || boundary_angle(range[1], domain.v)? != domain.v[1]
        {
            return Err(unsupported(
                "The circular trim arcs do not prove one common angular interval",
            ));
        }
        axial_sides[side] = true;
    }
    let mut angular_sides = [false; 2];
    for curve in lines {
        let (a, b) = curve.range_tuple();
        let [u, v] = domain.coordinates(curve.subs(a))?;
        let [ub, vb] = domain.coordinates(curve.subs(b))?;
        let v = boundary_angle(v, domain.v)?;
        let vb = boundary_angle(vb, domain.v)?;
        let side = usize::from(v == domain.v[1]);
        if angular_sides[side]
            || v != vb
            || (u.min(ub) - domain.u[0]).abs() > ANGLE_EPS
            || (u.max(ub) - domain.u[1]).abs() > ANGLE_EPS
        {
            return Err(unsupported(
                "Straight trim carriers do not prove both angular sides",
            ));
        }
        angular_sides[side] = true;
    }
    for (point, uv) in points.iter().zip(corners) {
        let v = boundary_angle(uv[1], domain.v)?;
        if !finite(domain.point(uv[0], v)) || domain.point(uv[0], v).distance(*point) > EPS {
            return Err(invalid(
                "The native face vertices disagree with their proven surface parameters",
            ));
        }
    }
    Ok(domain)
}
pub(crate) fn at(face: &Face, anchor: TangentAnchorValue) -> Result<DatumPlane> {
    match face.surface() {
        Surface::Plane(_) => planar(face, anchor),
        Surface::RevolutionSurface(_) => {
            let domain = revolution_domain(face)?;
            let [u, v] = domain.anchor(anchor)?;
            frame(
                domain.surface.subs(u, v),
                domain.surface.uder(u, v),
                domain.surface.vder(u, v),
                face.orientation(),
            )
        }
        _ => rational(face, anchor),
    }
}
