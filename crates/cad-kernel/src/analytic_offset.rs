//! Exact normal offsets of proven analytic carriers. Recognition uses complete
//! rational control nets/identities; display meshes never supply geometry.
use cad_core::{CadError, Result};
use monstertruck::{
    geometry::prelude::{TryIntoHomogeneousBsplineCurve, TryIntoHomogeneousBsplineSurface},
    modeling::*,
};
use std::collections::HashMap;

pub(crate) const EPS: f64 = 1e-7;
const REL: f64 = 1e-10;
const MAX_CONTROLS: usize = 65_536;
fn invalid(message: &str) -> CadError {
    CadError::Invalid(format!("Normal offset: {message}"))
}
fn unsupported(message: &str) -> CadError {
    CadError::Unsupported(format!("Normal offset: {message}"))
}
fn finite(p: Point3) -> bool {
    [p.x, p.y, p.z].iter().all(|x| x.is_finite())
}
fn unit(v: Vector3) -> Result<Vector3> {
    let length = v.magnitude();
    if !length.is_finite() || length <= 1e-12 {
        return Err(invalid("Degenerate carrier direction"));
    }
    Ok(v / length)
}
fn point(p: Vector4) -> Result<Point3> {
    if ![p.x, p.y, p.z, p.w].iter().all(|x| x.is_finite()) || p.w <= 0. {
        return Err(unsupported(
            "Exact carrier needs finite positive rational weights",
        ));
    }
    let p = Point3::from_vec(p.truncate() / p.w);
    if !finite(p) {
        return Err(invalid("Carrier exceeds finite document coordinates"));
    }
    Ok(p)
}
fn similarity(m: Matrix4) -> Option<f64> {
    if !Into::<[[f64; 4]; 4]>::into(m)
        .iter()
        .flatten()
        .all(|x| x.is_finite())
        || m.x.w.abs() > REL
        || m.y.w.abs() > REL
        || m.z.w.abs() > REL
        || (m.w.w - 1.).abs() > REL
    {
        return None;
    }
    let axes = [m.x.truncate(), m.y.truncate(), m.z.truncate()];
    let lengths = axes.map(|axis| axis.magnitude());
    let scale = lengths[0];
    if !scale.is_finite()
        || scale <= 1e-12
        || lengths
            .iter()
            .any(|l| !l.is_finite() || (*l - scale).abs() > REL * scale)
    {
        return None;
    }
    if (0..3).any(|i| (i + 1..3).any(|j| (axes[i] / scale).dot(axes[j] / scale).abs() > REL)) {
        return None;
    }
    Some(scale)
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum Carrier {
    Plane {
        origin: Point3,
        normal: Vector3,
    },
    Sphere {
        center: Point3,
        radius: f64,
    },
    Torus {
        origin: Point3,
        axis: Vector3,
        major: f64,
        minor: f64,
    },
    /// r(z) = radius + slope*z, with z measured from origin along axis.
    Cone {
        origin: Point3,
        axis: Vector3,
        radius: f64,
        slope: f64,
    },
}
impl Carrier {
    pub(crate) fn normal(self, p: Point3) -> Result<Vector3> {
        match self {
            Self::Plane { normal, .. } => Ok(normal),
            Self::Sphere { center, .. } => unit(p - center),
            Self::Torus {
                origin,
                axis,
                major,
                ..
            } => {
                let local = p - origin;
                let radial = unit(local - axis * local.dot(axis))?;
                unit(local - radial * major)
            }
            Self::Cone {
                origin,
                axis,
                slope,
                ..
            } => {
                let local = p - origin;
                Ok((unit(local - axis * local.dot(axis))? - axis * slope)
                    / (1. + slope * slope).sqrt())
            }
        }
    }
    pub(crate) fn same(self, other: Self) -> bool {
        match (self, other) {
            (
                Self::Plane {
                    origin: a,
                    normal: n,
                },
                Self::Plane {
                    origin: b,
                    normal: m,
                },
            ) => n.dot(m).abs() > 1. - REL && (a - b).dot(n).abs() <= EPS,
            (
                Self::Sphere {
                    center: a,
                    radius: r,
                },
                Self::Sphere {
                    center: b,
                    radius: s,
                },
            ) => a.distance(b) <= EPS && (r - s).abs() <= EPS,
            (
                Self::Torus {
                    origin: a,
                    axis: n,
                    major: r,
                    minor: t,
                },
                Self::Torus {
                    origin: b,
                    axis: m,
                    major: s,
                    minor: u,
                },
            ) => {
                a.distance(b) <= EPS
                    && n.dot(m).abs() > 1. - REL
                    && (r - s).abs() <= EPS
                    && (t - u).abs() <= EPS
            }
            (
                Self::Cone {
                    origin: a,
                    axis: n,
                    radius: r,
                    slope: k,
                },
                Self::Cone {
                    origin: b,
                    axis: m,
                    radius: s,
                    slope: l,
                },
            ) => {
                let d = b - a;
                n.dot(m).abs() > 1. - REL
                    && d.cross(n).magnitude() <= EPS
                    && (k - l * n.dot(m).signum()).abs() <= REL
                    && (r + k * d.dot(n) - s).abs() <= EPS
            }
            _ => false,
        }
    }
    pub(crate) fn offset_point(self, p: Point3, distance: f64) -> Result<Point3> {
        let q = p + self.normal(p)? * distance;
        if !finite(q) {
            return Err(invalid("Offset exceeds finite document coordinates"));
        }
        match self {
            Self::Sphere { radius, .. } if radius + distance <= EPS => {
                return Err(invalid(
                    "Thickness collapses the spherical offset at its center",
                ));
            }
            Self::Torus { major, minor, .. }
                if minor + distance <= EPS || minor + distance >= major - EPS =>
            {
                return Err(invalid(
                    "Thickness collapses the toroidal tube or closes its central opening",
                ));
            }
            Self::Cone {
                origin,
                axis,
                radius,
                slope,
            } => {
                let r = radius + slope * (p - origin).dot(axis);
                if !r.is_finite() || r <= EPS || r + distance / (1. + slope * slope).sqrt() <= EPS {
                    return Err(invalid("Thickness reaches or crosses the rotational axis"));
                }
            }
            _ => {}
        }
        Ok(q)
    }
    pub(crate) fn affine(self, distance: f64) -> Result<Option<Matrix4>> {
        match self {
            Self::Plane { normal, .. } => Ok(Some(Matrix4::from_translation(normal * distance))),
            Self::Sphere { center, radius } => {
                if radius + distance <= EPS {
                    return Err(invalid(
                        "Thickness collapses the spherical offset at its center",
                    ));
                }
                Ok(Some(
                    Matrix4::from_translation(center.to_vec())
                        * Matrix4::from_scale((radius + distance) / radius)
                        * Matrix4::from_translation(-center.to_vec()),
                ))
            }
            Self::Cone {
                origin,
                axis,
                radius,
                slope: 0.,
            } => {
                if radius + distance <= EPS {
                    return Err(invalid(
                        "Thickness collapses the cylindrical offset at its axis",
                    ));
                }
                let factor = (radius + distance) / radius;
                let column =
                    |v: Vector3| (v * factor + axis * v.dot(axis) * (1. - factor)).extend(0.);
                let linear = Matrix4::from_cols(
                    column(Vector3::unit_x()),
                    column(Vector3::unit_y()),
                    column(Vector3::unit_z()),
                    Vector4::unit_w(),
                );
                Ok(Some(
                    Matrix4::from_translation(origin.to_vec())
                        * linear
                        * Matrix4::from_translation(-origin.to_vec()),
                ))
            }
            _ => Ok(None),
        }
    }
    pub(crate) fn offset_curve(self, curve: &Curve, distance: f64) -> Result<Curve> {
        if let Some(transform) = self.affine(distance)? {
            return Ok(curve.transformed(transform));
        }
        if let Self::Torus {
            origin,
            axis,
            major,
            minor,
        } = self
        {
            if minor + distance <= EPS || minor + distance >= major - EPS {
                return Err(invalid("Thickness reaches a toroidal focal surface"));
            }
            let ((center, normal, radius), _) = crate::bounded_carriers::circle(curve)?;
            if normal.dot(axis).abs() > 1. - REL && (center - origin).cross(axis).magnitude() <= EPS
            {
                let scale = (minor + distance) / minor;
                let next_radius = major + (radius - major) * scale;
                let next_center = origin + (center - origin) * scale;
                let transform = Matrix4::from_translation(next_center.to_vec())
                    * Matrix4::from_scale(next_radius / radius)
                    * Matrix4::from_translation(-center.to_vec());
                return Ok(curve.transformed(transform));
            }
            let delta = center - origin;
            if normal.dot(axis).abs() <= REL
                && delta.dot(axis).abs() <= EPS
                && (delta.magnitude() - major).abs() <= EPS
                && (radius - minor).abs() <= EPS
            {
                return Ok(curve.transformed(
                    Matrix4::from_translation(center.to_vec())
                        * Matrix4::from_scale((minor + distance) / minor)
                        * Matrix4::from_translation(-center.to_vec()),
                ));
            }
            return Err(unsupported(
                "Toroidal offset requires exact meridian or latitude circular trims",
            ));
        }
        let Self::Cone {
            origin,
            axis,
            radius,
            slope,
        } = self
        else {
            unreachable!()
        };
        if !matches!(
            curve,
            Curve::Line(_) | Curve::BsplineCurve(_) | Curve::NurbsCurve(_)
        ) {
            return Err(unsupported(
                "Conical offsets of implicit trims require an exact parameter-space trim; intersection guides cannot replace them",
            ));
        }
        let native = curve
            .try_into_homogeneous_bspline_curve()
            .ok_or_else(|| unsupported("Boundary has no exact rational representation"))?;
        if native.control_points().len() > MAX_CONTROLS || native.degree() > 32 {
            return Err(unsupported(
                "Conical offset boundary exceeds the rational control budget",
            ));
        }
        let spans = native.bezier_decomposition();
        let degree = native.degree() * 2;
        let mut controls = Vec::new();
        let mut knots = Vec::new();
        for (index, span) in spans.iter().enumerate() {
            let source = span.control_points();
            let mut result = vec![Vector4::zero(); degree + 1];
            let rho=source.iter().map(|p| {
                let q=point(*p)?;
                let r=radius+slope*(q-origin).dot(axis);
                if r<=EPS || r+distance/(1.+slope*slope).sqrt()<=EPS { return Err(invalid("Thickness crosses the axis or the exact rational bound cannot exclude that collapse")); }
                Ok(r*p.w)
            }).collect::<Result<Vec<_>>>()?;
            let d = distance / (1. + slope * slope).sqrt();
            for (i, p) in source.iter().enumerate() {
                let local = p.truncate() - origin.to_vec() * p.w;
                let radial = local - axis * local.dot(axis);
                for (j, q) in source.iter().enumerate() {
                    let f = binomial(source.len() - 1, i) * binomial(source.len() - 1, j)
                        / binomial(degree, i + j);
                    let xyz = p.truncate() * rho[j] + radial * (d * q.w)
                        - axis * (d * slope * p.w * rho[j]);
                    result[i + j] += Vector4::new(xyz.x, xyz.y, xyz.z, p.w * rho[j]) * f;
                }
            }
            for p in &result {
                point(*p)?;
            }
            let (a, b) = span.range_tuple();
            if index == 0 {
                knots.extend(std::iter::repeat_n(a, degree + 1));
            } else {
                knots.extend(std::iter::repeat_n(a, degree));
                result.remove(0);
            }
            controls.extend(result);
            if index + 1 == spans.len() {
                knots.extend(std::iter::repeat_n(b, degree + 1));
            }
            if controls.len() > MAX_CONTROLS {
                return Err(unsupported(
                    "Conical offset exceeds the rational control budget",
                ));
            }
        }
        Ok(Curve::NurbsCurve(NurbsCurve::new(
            BsplineCurve::try_new(KnotVector::from(knots), controls)
                .map_err(|e| invalid(&e.to_string()))?,
        )))
    }
    pub(crate) fn offset_surface(self, surface: &Surface, distance: f64) -> Result<Surface> {
        if let Some(transform) = self.affine(distance)? {
            return Ok(surface.transformed(transform));
        }
        if let Surface::RevolutionSurface(processor) = surface {
            let inverse = processor
                .transform()
                .invert()
                .ok_or_else(|| invalid("Singular revolution transform"))?;
            let world = processor
                .entity()
                .entity_curve()
                .transformed(*processor.transform());
            let changed = self.offset_curve(&world, distance)?.transformed(inverse);
            return Ok(Surface::RevolutionSurface(processor.map_ref(|source| {
                RevolutionSurface::by_revolution(changed, source.origin(), source.axis())
            })));
        }
        if let Self::Torus {
            origin,
            axis,
            major,
            minor,
        } = self
        {
            if minor + distance <= EPS || minor + distance >= major - EPS {
                return Err(invalid("Thickness reaches a toroidal focal surface"));
            }
            let mut net = surface
                .try_into_homogeneous_bspline_surface()
                .ok_or_else(|| unsupported("Toroidal carrier has no exact rational net"))?;
            let Some((_, transposed)) = toroidal(&net)? else {
                return Err(unsupported(
                    "Toroidal carrier lost its exact circular product",
                ));
            };
            if transposed {
                net.invert();
            }
            let scale = (minor + distance) / minor;
            let mut controls = net.control_points().clone();
            for j in 0..controls[0].len() {
                let c = Curve::NurbsCurve(NurbsCurve::new(BsplineCurve::new(
                    net.knot_vector_u().clone(),
                    controls.iter().map(|row| row[j]).collect(),
                )));
                let (center, _, radius) = crate::axis_geometry::circle_geometry(&c)?;
                let next_radius = major + (radius - major) * scale;
                let next_center = origin + axis * ((center - origin).dot(axis) * scale);
                for row in &mut controls {
                    let h = &mut row[j];
                    let p = point(*h)?;
                    *h = (next_center + (p - center) * (next_radius / radius)).to_homogeneous()
                        * h.w;
                    point(*h)?;
                }
            }
            net = BsplineSurface::new(net.knot_vectors().clone(), controls);
            if transposed {
                net.invert();
            }
            return Ok(Surface::NurbsSurface(NurbsSurface::new(net)));
        }
        let mut net = surface
            .try_into_homogeneous_bspline_surface()
            .ok_or_else(|| unsupported("Conical carrier has no exact rational net"))?;
        let Some((_, transposed)) = ruled(&net)? else {
            return Err(unsupported(
                "Conical carrier lost its proven straight meridian",
            ));
        };
        if transposed {
            net.invert();
        }
        let Self::Cone {
            origin,
            axis,
            radius,
            slope,
        } = self
        else {
            unreachable!()
        };
        let d = distance / (1. + slope * slope).sqrt();
        for control in net.control_points_mut() {
            let p = point(*control)?;
            let z = (p - origin).dot(axis);
            let r = radius + slope * z;
            if r <= EPS || r + d <= EPS {
                return Err(invalid("Offset conical carrier collapses at the axis"));
            }
            let radial = (p - origin) - axis * z;
            let p = p + radial * (d / r) - axis * (d * slope);
            *control = p.to_homogeneous() * control.w;
            point(*control)?;
        }
        if transposed {
            net.invert();
        }
        Ok(Surface::NurbsSurface(NurbsSurface::new(net)))
    }
}
fn binomial(n: usize, k: usize) -> f64 {
    (1..=k.min(n - k)).fold(1., |a, i| a * (n + 1 - i) as f64 / i as f64)
}
fn ruled(surface: &BsplineSurface<Vector4>) -> Result<Option<(Carrier, bool)>> {
    if surface
        .control_points()
        .len()
        .saturating_mul(surface.control_points().first().map_or(0, Vec::len))
        > MAX_CONTROLS
    {
        return Err(unsupported("Carrier exceeds the rational control budget"));
    }
    let mut net = surface.clone();
    let transposed = net.vdegree() != 1 || net.control_points()[0].len() != 2;
    if transposed {
        net.invert();
    }
    if net.vdegree() != 1 || net.control_points()[0].len() != 2 {
        return Ok(None);
    }
    let curve = |i| {
        Curve::NurbsCurve(NurbsCurve::new(BsplineCurve::new(
            net.knot_vector_u().clone(),
            net.control_points().iter().map(|row| row[i]).collect(),
        )))
    };
    let (Ok((a, na, ra)), Ok((b, nb, rb))) = (
        crate::axis_geometry::circle_geometry(&curve(0)),
        crate::axis_geometry::circle_geometry(&curve(1)),
    ) else {
        return Ok(None);
    };
    let delta = b - a;
    if delta.magnitude() <= EPS {
        return Ok(None);
    }
    let axis = unit(delta)?;
    if na.dot(axis).abs() < 1. - REL || nb.dot(axis).abs() < 1. - REL {
        return Ok(None);
    }
    let ratio = net.control_points()[0][1].w / net.control_points()[0][0].w;
    let scale = rb / ra;
    if !ratio.is_finite() || ratio <= 0. || !scale.is_finite() || scale <= 0. {
        return Ok(None);
    }
    for row in net.control_points() {
        let p = point(row[0])?;
        let q = point(row[1])?;
        if (row[1].w / row[0].w - ratio).abs() > REL * ratio
            || ((q - b) - (p - a) * scale).magnitude() > EPS
        {
            return Ok(None);
        }
    }
    Ok(Some((
        Carrier::Cone {
            origin: a,
            axis,
            radius: ra,
            slope: (rb - ra) / delta.magnitude(),
        },
        transposed,
    )))
}

/// Verify a circular meridian multiplied by a common circular angular basis.
fn toroidal(surface: &BsplineSurface<Vector4>) -> Result<Option<(Carrier, bool)>> {
    if surface
        .control_points()
        .len()
        .saturating_mul(surface.control_points().first().map_or(0, Vec::len))
        > MAX_CONTROLS
    {
        return Err(unsupported(
            "Toroidal carrier exceeds the rational control budget",
        ));
    }
    for transposed in [false, true] {
        let mut net = surface.clone();
        if transposed {
            net.invert();
        }
        if net.control_points().len() < 3 || net.control_points()[0].len() < 3 {
            continue;
        }
        let columns = net.control_points()[0].len();
        let angular = |j| {
            Curve::NurbsCurve(NurbsCurve::new(BsplineCurve::new(
                net.knot_vector_u().clone(),
                net.control_points().iter().map(|row| row[j]).collect(),
            )))
        };
        let Ok((origin, axis, radius)) = crate::axis_geometry::circle_geometry(&angular(0)) else {
            continue;
        };
        let meridian = Curve::NurbsCurve(NurbsCurve::new(BsplineCurve::new(
            net.knot_vector_v().clone(),
            net.control_points()[0].clone(),
        )));
        let Ok((center, normal, minor)) = crate::axis_geometry::circle_geometry(&meridian) else {
            continue;
        };
        let delta = center - origin;
        let major_vector = delta - axis * delta.dot(axis);
        let major = major_vector.magnitude();
        if normal.dot(axis).abs() > REL || major <= minor + EPS {
            continue;
        }
        let mut valid = true;
        for j in 0..columns {
            let Ok((c, n, r)) = crate::axis_geometry::circle_geometry(&angular(j)) else {
                valid = false;
                break;
            };
            if (c - origin).cross(axis).magnitude() > EPS || n.dot(axis).abs() < 1. - REL {
                valid = false;
                break;
            }
            let ratio = net.control_points()[0][j].w / net.control_points()[0][0].w;
            if !ratio.is_finite() || ratio <= 0. || !(r / radius).is_finite() {
                valid = false;
                break;
            }
            for row in net.control_points() {
                if (row[j].w / row[0].w - ratio).abs() > REL * ratio
                    || ((point(row[j])? - c) - (point(row[0])? - origin) * (r / radius)).magnitude()
                        > EPS
                {
                    valid = false;
                    break;
                }
            }
            if !valid {
                break;
            }
        }
        if valid {
            return Ok(Some((
                Carrier::Torus {
                    origin: center - major_vector,
                    axis,
                    major,
                    minor,
                },
                transposed,
            )));
        }
    }
    Ok(None)
}

pub(crate) fn recognize(face: &Face) -> Result<Option<Carrier>> {
    let surface = face.surface();
    match &surface {
        Surface::Plane(plane) => {
            return Ok(Some(Carrier::Plane {
                origin: plane.subs(0., 0.),
                normal: unit(plane.normal())?,
            }));
        }
        Surface::SphericalSurface(processor) => {
            let Some(scale) = similarity(*processor.transform()) else {
                return Ok(None);
            };
            return Ok(Some(Carrier::Sphere {
                center: processor
                    .transform()
                    .transform_point(processor.entity().center()),
                radius: scale * processor.entity().radius(),
            }));
        }
        Surface::RevolutionSurface(processor) => {
            let Ok(axis) = crate::axis_geometry::face_axis(face) else {
                return Ok(None);
            };
            let origin = Point3::from(axis.origin);
            let axis = Vector3::from(axis.direction);
            let profile = processor
                .entity()
                .entity_curve()
                .transformed(*processor.transform());
            if let Ok((center, normal, radius)) = crate::axis_geometry::circle_geometry(&profile)
                && (center - origin).cross(axis).magnitude() <= EPS
                && normal.dot(axis).abs() <= REL
            {
                return Ok(Some(Carrier::Sphere { center, radius }));
            }
            if let Ok((center, normal, minor)) = crate::axis_geometry::circle_geometry(&profile) {
                let delta = center - origin;
                let major = delta.cross(axis).magnitude();
                if normal.dot(axis).abs() <= REL
                    && delta.dot(normal).abs() <= EPS
                    && major > minor + EPS
                {
                    return Ok(Some(Carrier::Torus {
                        origin: origin + axis * delta.dot(axis),
                        axis,
                        major,
                        minor,
                    }));
                }
            }
            if crate::axis_geometry::line_axis(&profile).is_ok() {
                let (a, b) = profile.range_tuple();
                let p = profile.subs(a);
                let q = profile.subs(b);
                let za = (p - origin).dot(axis);
                let zb = (q - origin).dot(axis);
                let pa = p - origin - axis * za;
                let pb = q - origin - axis * zb;
                if (za - zb).abs() <= EPS {
                    return Ok(Some(Carrier::Plane {
                        origin: origin + axis * za,
                        normal: axis,
                    }));
                }
                let ra = pa.magnitude();
                let rb = pb.magnitude();
                if ra > EPS && rb > EPS && (pa / ra).dot(pb / rb) > 1. - REL {
                    return Ok(Some(Carrier::Cone {
                        origin: origin + axis * za,
                        axis,
                        radius: ra,
                        slope: (rb - ra) / (zb - za),
                    }));
                }
            }
        }
        Surface::NurbsSurface(_) | Surface::BsplineSurface(_) => {
            let Some(net) = surface.try_into_homogeneous_bspline_surface() else {
                return Ok(None);
            };
            if let Some((carrier, _)) = ruled(&net)? {
                return Ok(Some(carrier));
            }
            if let Some((carrier, _)) = toroidal(&net)? {
                return Ok(Some(carrier));
            }
        }
        _ => {}
    }
    Ok(None)
}
pub(crate) fn side(face: &Face, carrier: Carrier) -> Result<f64> {
    let surface = face.oriented_surface();
    let (ur, vr) = surface.try_range_tuple();
    let (u, v) = (
        ur.map_or(0., |(a, b)| (a + b) * 0.5),
        vr.map_or(0., |(a, b)| (a + b) * 0.5),
    );
    let p = surface.subs(u, v);
    let n = unit(surface.normal(u, v))?;
    let alignment = n.dot(carrier.normal(p)?);
    if alignment.abs() < 1. - 1e-8 {
        return Err(unsupported("Carrier orientation cannot be established"));
    }
    Ok(alignment.signum())
}

pub(crate) struct Offset {
    pub(crate) shell: Shell,
    /// Source ID -> generated absolute edge.
    pub(crate) edges: HashMap<EdgeId, Edge>,
    pub(crate) faces: HashMap<FaceId, FaceId>,
}
/// A common analytic normal field is globally injective until its focal axis or
/// center is reached. Adjacent patches retain one shared edge and vertex.
pub(crate) fn offset(shell: &Shell, distance: f64) -> Result<Offset> {
    if !distance.is_finite() || shell.is_empty() {
        return Err(invalid("Offset needs a finite distance and nonempty shell"));
    }
    let carrier = recognize(&shell[0])?.ok_or_else(|| {
        unsupported(
            "This surface has no proven exact plane, cylinder, cone or sphere normal offset",
        )
    })?;
    let sign = side(&shell[0], carrier)?;
    for face in shell {
        let Some(other) = recognize(face)? else {
            return Err(unsupported(
                "All joined surface patches need a proven analytic normal offset",
            ));
        };
        if !carrier.same(other) || side(face, carrier)? != sign {
            return Err(unsupported(
                "Joined surfaces on different carriers require offset intersections and corner reconstruction",
            ));
        }
    }
    if distance == 0. {
        return Ok(Offset {
            shell: shell.clone(),
            edges: shell
                .edge_iter()
                .map(|edge| {
                    (
                        edge.id(),
                        if edge.orientation() {
                            edge
                        } else {
                            edge.inverse()
                        },
                    )
                })
                .collect(),
            faces: shell.iter().map(|face| (face.id(), face.id())).collect(),
        });
    }
    let d = distance * sign;
    let mut vertices: HashMap<VertexId, Vertex> = HashMap::new();
    let mut edges = HashMap::new();
    for edge in shell.edge_iter() {
        if edges.contains_key(&edge.id()) {
            continue;
        }
        for vertex in [edge.absolute_front(), edge.absolute_back()] {
            if let std::collections::hash_map::Entry::Vacant(slot) = vertices.entry(vertex.id()) {
                slot.insert(builder::vertex(carrier.offset_point(vertex.point(), d)?));
            }
        }
        let curve = carrier.offset_curve(&edge.curve(), d)?;
        edges.insert(
            edge.id(),
            Edge::try_new(
                &vertices[&edge.absolute_front().id()],
                &vertices[&edge.absolute_back().id()],
                curve,
            )
            .map_err(|e| invalid(&e.to_string()))?,
        );
    }
    let mut result = Shell::new();
    let mut faces = HashMap::new();
    for face in shell {
        let wires = face
            .boundaries()
            .into_iter()
            .map(|wire| {
                wire.iter()
                    .map(|edge| {
                        if edge.orientation() {
                            edges[&edge.id()].clone()
                        } else {
                            edges[&edge.id()].inverse()
                        }
                    })
                    .collect()
            })
            .collect();
        let surface = carrier.offset_surface(&face.oriented_surface(), d)?;
        let mapped = Face::try_new(wires, surface).map_err(|e| invalid(&e.to_string()))?;
        faces.insert(face.id(), mapped.id());
        result.push(mapped);
    }
    crate::validation::surface(&result)?;
    Ok(Offset {
        shell: result,
        edges,
        faces,
    })
}

pub(crate) fn thicken(
    shell: &Shell,
    thickness: f64,
    mid_plane: bool,
    reverse: bool,
) -> Result<Vec<Solid>> {
    if shell.is_empty() {
        return Err(invalid("Thicken needs a nonempty surface shell"));
    }
    if !thickness.is_finite() || thickness <= EPS {
        return Err(invalid("Thickness must be finite and positive"));
    }
    let direction = if reverse { -1. } else { 1. };
    if let Some(Carrier::Plane { normal, .. }) = recognize(&shell[0])? {
        let sign = side(
            &shell[0],
            recognize(&shell[0])?.ok_or_else(|| invalid("Missing planar carrier"))?,
        )?;
        let start = offset(
            shell,
            if mid_plane {
                -thickness * direction * 0.5
            } else {
                0.
            },
        )?;
        return crate::surface_edit::thicken_planar(
            &start.shell,
            normal * sign * direction * thickness,
        );
    }
    let lower = if mid_plane { -thickness * 0.5 } else { 0. };
    let upper = if mid_plane {
        thickness * 0.5
    } else {
        thickness
    };
    let a = offset(shell, lower * direction)?;
    let b = offset(shell, upper * direction)?;
    let mut result = if reverse {
        a.shell.clone()
    } else {
        a.shell.inverse().into()
    };
    result.extend(if reverse {
        b.shell.inverse()
    } else {
        b.shell.iter().cloned().collect()
    });
    // Extract original boundary loops, then preserve their exact edge pairing.
    for wire in shell.extract_boundaries() {
        let mapped = |offset: &Offset| {
            wire.iter()
                .map(|edge| {
                    if edge.orientation() {
                        offset.edges[&edge.id()].clone()
                    } else {
                        offset.edges[&edge.id()].inverse()
                    }
                })
                .collect::<Wire>()
        };
        let lower = mapped(&a);
        let upper = mapped(&b);
        crate::surface_edit::extrudable_edges(lower.iter().cloned())?;
        crate::surface_edit::extrudable_edges(upper.iter().cloned())?;
        let rim = crate::loft_correspondence::skin(&lower, &upper, None, None)?;
        result.extend(if reverse {
            rim.inverse()
        } else {
            rim.iter().cloned().collect()
        });
    }
    let components = result.connected_components();
    let solids = if shell.extract_boundaries().is_empty() {
        vec![Solid::try_new(components).map_err(|e| invalid(&e.to_string()))?]
    } else {
        components
            .into_iter()
            .map(|shell| Solid::try_new(vec![shell]).map_err(|e| invalid(&e.to_string())))
            .collect::<Result<Vec<_>>>()?
    };
    for solid in &solids {
        crate::validation::solid(solid)?;
    }
    Ok(solids)
}
