//! Native drawing geometry extraction. No sampled boundary or intersection
//! leader can enter this contract. Missing carriers remain explicit failures.
use crate::native_bounds::Interval;
use cad_core::{CadError, Result, drawing_geometry as out};
use monstertruck::{modeling::*, topology::trimmed::TrimmedFace};

type NativeFace = TrimmedFace<Point3, Curve, Surface, ParameterCurve<Curve2D, Box<Surface>>>;
const MAX_CONTROLS: usize = 262_144;
const MAX_DEGREE: usize = 32;
mod closure;

pub(crate) struct Budget(usize);
impl Default for Budget {
    fn default() -> Self {
        Self(MAX_CONTROLS)
    }
}
impl Budget {
    fn check(&self, count: usize) -> Result<()> {
        if count > self.0 {
            return Err(unsupported("Native drawing control budget exceeded"));
        }
        Ok(())
    }
    fn take(&mut self, count: usize) -> Result<()> {
        self.0 = self
            .0
            .checked_sub(count)
            .ok_or_else(|| unsupported("Native drawing control budget exceeded"))?;
        Ok(())
    }
}
fn unsupported(message: &str) -> CadError {
    CadError::Unsupported(format!("Native drawing geometry: {message}"))
}
fn p3(p: Point3) -> [f64; 3] {
    [p.x, p.y, p.z]
}
fn v3(p: Vector3) -> [f64; 3] {
    [p.x, p.y, p.z]
}
fn matrix4(m: &Matrix4) -> Result<[[f64; 4]; 4]> {
    let out = std::array::from_fn(|row| std::array::from_fn(|column| m[column][row]));
    if out.iter().flatten().any(|x| !x.is_finite()) || out[3] != [0., 0., 0., 1.] {
        return Err(unsupported(
            "An analytic carrier requires a finite affine transform",
        ));
    }
    Ok(out)
}
fn matrix3(m: &Matrix3) -> Result<[[f64; 3]; 3]> {
    let out = std::array::from_fn(|row| std::array::from_fn(|column| m[column][row]));
    if out.iter().flatten().any(|x| !x.is_finite()) || out[2] != [0., 0., 1.] {
        return Err(unsupported(
            "A parameter conic requires a finite affine transform",
        ));
    }
    Ok(out)
}
fn knots(degree: usize, knots: &[f64], controls: usize) -> Result<()> {
    if degree == 0
        || degree > MAX_DEGREE
        || controls <= degree
        || knots.len() != controls + degree + 1
        || knots.iter().any(|x| !x.is_finite())
        || knots.windows(2).any(|x| x[0] > x[1])
        || knots[degree] >= knots[controls]
    {
        return Err(unsupported(
            "The rational carrier has an invalid or unsupported knot domain",
        ));
    }
    Ok(())
}
fn controls<const N: usize>(values: &[[f64; N]], budget: &mut Budget) -> Result<()> {
    budget.take(values.len())?;
    if values.is_empty()
        || values
            .iter()
            .any(|p| p.iter().any(|x| !x.is_finite()) || p[N - 1] <= 0.)
    {
        return Err(unsupported(
            "Native rational controls require finite coordinates and positive weights",
        ));
    }
    Ok(())
}
pub(crate) fn curve3(curve: &Curve, budget: &mut Budget) -> Result<out::Curve3> {
    budget.check(match curve {
        Curve::Line(_) => 2,
        Curve::BsplineCurve(c) => c.control_points().len(),
        Curve::NurbsCurve(c) => c.control_points().len(),
        _ => 0,
    })?;
    let (degree, knots, points) = match curve {
        Curve::Line(Line(a, b)) => {
            budget.take(2)?;
            let (start, end) = (p3(*a), p3(*b));
            if start.iter().chain(&end).any(|x| !x.is_finite()) {
                return Err(unsupported("A profile line is not finite"));
            }
            return Ok(out::Curve3::Line { start, end });
        }
        Curve::BsplineCurve(c) => (
            c.degree(),
            c.knot_vector().to_vec(),
            c.control_points()
                .iter()
                .map(|p| [p.x, p.y, p.z, 1.])
                .collect::<Vec<_>>(),
        ),
        Curve::NurbsCurve(c) => (
            c.degree(),
            c.knot_vector().to_vec(),
            c.control_points()
                .iter()
                .map(|p| [p.x, p.y, p.z, p.w])
                .collect::<Vec<_>>(),
        ),
        _ => {
            return Err(unsupported(
                "The revolution profile has no explicit native rational carrier",
            ));
        }
    };
    controls(&points, budget)?;
    self::knots(degree, &knots, points.len())?;
    Ok(out::Curve3::Rational {
        degree,
        knots,
        control_points: points,
    })
}
fn rational_surface(surface: &Surface, budget: &mut Budget) -> Result<out::Carrier> {
    let count = match surface {
        Surface::BsplineSurface(s) => s
            .control_points()
            .iter()
            .map(Vec::len)
            .try_fold(0usize, usize::checked_add),
        Surface::NurbsSurface(s) => s
            .control_points()
            .iter()
            .map(Vec::len)
            .try_fold(0usize, usize::checked_add),
        _ => None,
    }
    .ok_or_else(|| unsupported("Native surface control count is unavailable"))?;
    budget.check(count)?;
    let (degree, knot_vectors, points) = match surface {
        Surface::BsplineSurface(s) => (
            s.degrees(),
            s.knot_vectors().clone(),
            s.control_points()
                .iter()
                .map(|row| row.iter().map(|p| [p.x, p.y, p.z, 1.]).collect::<Vec<_>>())
                .collect::<Vec<_>>(),
        ),
        Surface::NurbsSurface(s) => (
            s.degrees(),
            s.knot_vectors().clone(),
            s.control_points()
                .iter()
                .map(|row| row.iter().map(|p| [p.x, p.y, p.z, p.w]).collect::<Vec<_>>())
                .collect::<Vec<_>>(),
        ),
        _ => {
            return Err(unsupported(
                "This surface has no native rational tensor-product carrier",
            ));
        }
    };
    let width = points.first().map_or(0, Vec::len);
    if width == 0 || points.iter().any(|row| row.len() != width) {
        return Err(unsupported("Rational surface controls are not rectangular"));
    }
    for row in &points {
        controls(row, budget)?;
    }
    let knots = [knot_vectors.0.to_vec(), knot_vectors.1.to_vec()];
    self::knots(degree.0, &knots[0], points.len())?;
    self::knots(degree.1, &knots[1], width)?;
    Ok(out::Carrier::Rational {
        surface: out::RationalSurface {
            degree: [degree.0, degree.1],
            knots,
            control_points: points,
        },
    })
}
fn carrier(surface: &Surface, budget: &mut Budget) -> Result<out::Carrier> {
    // These are source values, not normalized or reconstructed approximations.
    match surface {
        Surface::SphericalSurface(p) => {
            budget.take(5)?;
            if p3(p.entity().center()).into_iter().any(|x| !x.is_finite())
                || !p.entity().radius().is_finite()
                || p.entity().radius() <= 0.
            {
                return Err(unsupported(
                    "The native sphere has invalid center or radius",
                ));
            }
        }
        Surface::ToroidalSurface(p) => {
            budget.take(6)?;
            if p3(p.entity().center()).into_iter().any(|x| !x.is_finite())
                || [p.entity().large_radius(), p.entity().small_radius()]
                    .into_iter()
                    .any(|r| !r.is_finite() || r <= 0.)
            {
                return Err(unsupported("The native torus has invalid center or radii"));
            }
        }
        Surface::RevolutionSurface(p) => {
            budget.take(6)?;
            let origin = p3(p.entity().origin());
            let axis = v3(p.entity().axis());
            if origin.iter().chain(&axis).any(|x| !x.is_finite()) || axis == [0.; 3] {
                return Err(unsupported(
                    "The native revolution has an invalid origin or axis",
                ));
            }
        }
        _ => {}
    }
    match surface {
        Surface::Plane(p) => {
            budget.take(3)?;
            let [origin, u_point, v_point] = p.defining_points().map(p3);
            if origin
                .iter()
                .chain(&u_point)
                .chain(&v_point)
                .any(|x| !x.is_finite())
            {
                return Err(unsupported("The native plane is not finite"));
            }
            Ok(out::Carrier::Plane {
                origin,
                u_point,
                v_point,
            })
        }
        Surface::BsplineSurface(_) | Surface::NurbsSurface(_) => rational_surface(surface, budget),
        Surface::SphericalSurface(p) => Ok(out::Carrier::Sphere {
            center: p3(p.entity().center()),
            radius: p.entity().radius(),
            transform: matrix4(p.transform())?,
            swap_uv: !p.orientation(),
        }),
        Surface::ToroidalSurface(p) => Ok(out::Carrier::Torus {
            center: p3(p.entity().center()),
            major_radius: p.entity().large_radius(),
            minor_radius: p.entity().small_radius(),
            transform: matrix4(p.transform())?,
            swap_uv: !p.orientation(),
        }),
        Surface::RevolutionSurface(p) => Ok(out::Carrier::Revolution {
            profile: curve3(p.entity().entity_curve(), budget)?,
            origin: p3(p.entity().origin()),
            axis: v3(p.entity().axis()),
            transform: matrix4(p.transform())?,
            swap_uv: !p.orientation(),
        }),
        Surface::TsplineSurface(_) => Err(unsupported(
            "T-spline drawing carriers are not exported yet",
        )),
    }
}
fn curve2(curve: &Curve2D, budget: &mut Budget) -> Result<out::Curve2> {
    budget.check(match curve {
        Curve2D::Line(_) => 2,
        Curve2D::Polyline(c) => c.len(),
        Curve2D::BsplineCurve(c) => c.control_points().len(),
        Curve2D::NurbsCurve(c) => c.control_points().len(),
        Curve2D::Conic(_) => 9,
    })?;
    let (degree, knots, points) = match curve {
        Curve2D::Line(Line(a, b)) => (
            1,
            vec![0., 0., 1., 1.],
            vec![[a.x, a.y, 1.], [b.x, b.y, 1.]],
        ),
        Curve2D::Polyline(c) => {
            if c.len() < 2 {
                return Err(unsupported(
                    "The native parameter polyline has fewer than two points",
                ));
            }
            let end = (c.len() - 1) as f64;
            (
                1,
                std::iter::once(0.)
                    .chain((0..c.len()).map(|i| i as f64))
                    .chain(std::iter::once(end))
                    .collect(),
                c.iter().map(|p| [p.x, p.y, 1.]).collect::<Vec<_>>(),
            )
        }
        Curve2D::BsplineCurve(c) => (
            c.degree(),
            c.knot_vector().to_vec(),
            c.control_points()
                .iter()
                .map(|p| [p.x, p.y, 1.])
                .collect::<Vec<_>>(),
        ),
        Curve2D::NurbsCurve(c) => (
            c.degree(),
            c.knot_vector().to_vec(),
            c.control_points()
                .iter()
                .map(|p| [p.x, p.y, p.z])
                .collect::<Vec<_>>(),
        ),
        Curve2D::Conic(c) => {
            budget.take(9)?;
            let (conic, transform, range, reversed) = match c {
                Conic2D::Ellipse(c) => (
                    out::Conic::Ellipse,
                    matrix3(c.transform())?,
                    c.range_tuple(),
                    !c.orientation(),
                ),
                Conic2D::Hyperbola(c) => (
                    out::Conic::Hyperbola,
                    matrix3(c.transform())?,
                    c.range_tuple(),
                    !c.orientation(),
                ),
                Conic2D::Parabola(c) => (
                    out::Conic::Parabola,
                    matrix3(c.transform())?,
                    c.range_tuple(),
                    !c.orientation(),
                ),
            };
            if !range.0.is_finite() || !range.1.is_finite() || range.0 >= range.1 {
                return Err(unsupported(
                    "The native conic trim has no finite increasing domain",
                ));
            }
            return Ok(out::Curve2::Conic {
                conic,
                transform,
                range: [range.0, range.1],
                reversed,
            });
        }
    };
    controls(&points, budget)?;
    self::knots(degree, &knots, points.len())?;
    Ok(out::Curve2::Rational {
        degree,
        knots,
        control_points: points,
        reversed: false,
    })
}
fn reverse(curve: &mut out::Curve2) {
    match curve {
        out::Curve2::Rational { reversed, .. } | out::Curve2::Conic { reversed, .. } => {
            *reversed = !*reversed
        }
    }
}
// Outward exponential enclosure from positive Taylor terms. The small base
// interval and geometric tail bound avoid relying on libm exp rounding.
fn exponential_upper(value: f64) -> Result<f64> {
    if !value.is_finite() || !(0. ..=700.).contains(&value) {
        return Err(unsupported(
            "The hyperbolic trim exceeds the finite parameter enclosure",
        ));
    }
    let mut x = Interval::exact(value)?;
    let mut squares = 0;
    while x.high > 0.5 {
        x = x.div(Interval::exact(2.)?)?;
        squares += 1;
    }
    let mut term = Interval::exact(1.)?;
    let mut sum = term;
    for n in 1..=48 {
        term = term.mul(x)?.div(Interval::exact(n as f64)?)?;
        sum = sum.add(term)?;
    }
    let next = term.mul(x)?.div(Interval::exact(49.)?)?;
    let tail = next.div(Interval::exact(1.)?.sub(x.div(Interval::exact(50.)?)?)?)?;
    sum = sum.add(Interval::new(0., tail.high)?)?;
    for _ in 0..squares {
        sum = sum.mul(sum)?;
    }
    Ok(sum.high)
}
fn curve_bounds(curve: &out::Curve2) -> Result<[[f64; 2]; 2]> {
    let mut result = [[f64::INFINITY, f64::NEG_INFINITY]; 2];
    match curve {
        out::Curve2::Rational { control_points, .. } => {
            for point in control_points {
                let weight = Interval::exact(point[2])?;
                for axis in 0..2 {
                    let value = Interval::exact(point[axis])?.div(weight)?;
                    result[axis][0] = result[axis][0].min(value.low);
                    result[axis][1] = result[axis][1].max(value.high);
                }
            }
        }
        out::Curve2::Conic {
            conic,
            transform,
            range,
            ..
        } => {
            let base = match conic {
                out::Conic::Ellipse => [Interval::new(-1., 1.)?; 2],
                out::Conic::Hyperbola => {
                    let e = exponential_upper(range[0].abs().max(range[1].abs()))?;
                    [Interval::new(1., e)?, Interval::new(-e, e)?]
                }
                out::Conic::Parabola => {
                    let t = Interval::new(range[0], range[1])?;
                    [t.mul(t)?, Interval::exact(2.)?.mul(t)?]
                }
            };
            for axis in 0..2 {
                let mut value = Interval::exact(transform[axis][2])?;
                for (column, part) in base.into_iter().enumerate() {
                    value = value.add(Interval::exact(transform[axis][column])?.mul(part)?)?;
                }
                result[axis] = [value.low, value.high];
            }
        }
    }
    Ok(result)
}
pub(crate) fn face(
    face: &NativeFace,
    budget: &mut Budget,
    tolerance_mm: f64,
) -> Result<out::FaceGeometry> {
    let carrier = carrier(&face.face().surface(), budget)?;
    let mut loops = Vec::new();
    let mut domain = [[f64::INFINITY, f64::NEG_INFINITY]; 2];
    for (wire, trims) in face.face().absolute_boundaries().iter().zip(face.trims()) {
        let mut boundary = Vec::new();
        for (edge, trim) in wire.iter().zip(trims) {
            let trim = trim.as_ref().ok_or_else(|| {
                unsupported("A face boundary has no authoritative native parameter curve")
            })?;
            let mut curve = curve2(trim.curve(), budget)?;
            let bounds = curve_bounds(&curve)?;
            for axis in 0..2 {
                domain[axis][0] = domain[axis][0].min(bounds[axis][0]);
                domain[axis][1] = domain[axis][1].max(bounds[axis][1]);
            }
            if !edge.orientation() {
                reverse(&mut curve);
            }
            boundary.push(curve);
        }
        if boundary.is_empty() {
            return Err(unsupported("A native trim loop is empty"));
        }
        loops.push(boundary);
    }
    if loops.is_empty()
        || domain
            .iter()
            .any(|d| !d[0].is_finite() || !d[1].is_finite() || d[0] >= d[1])
    {
        return Err(unsupported(
            "The face has no finite two-dimensional native trim domain",
        ));
    }
    // Rational surfaces have no continuation beyond their native knot frame.
    // First prove the complete trim locus is inside that frame. Only then may
    // its outward-rounded enclosure be intersected with the exact knot bounds;
    // this removes enclosure rounding, never part of an actual source curve.
    if let out::Carrier::Rational { surface } = &carrier {
        for (axis, d) in domain.iter_mut().enumerate() {
            let count = if axis == 0 {
                surface.control_points.len()
            } else {
                surface.control_points[0].len()
            };
            let frame = [
                surface.knots[axis][surface.degree[axis]],
                surface.knots[axis][count],
            ];
            closure::certify_knot_frame(&loops, axis, frame, budget)?;
            d[0] = d[0].max(frame[0]);
            d[1] = d[1].min(frame[1]);
            if d[0] >= d[1] {
                return Err(unsupported(
                    "The face trim lies outside its native rational knot domain",
                ));
            }
        }
    }
    if let out::Carrier::Revolution {
        profile:
            out::Curve3::Rational {
                degree,
                knots,
                control_points,
            },
        swap_uv,
        ..
    } = &carrier
    {
        let axis = usize::from(*swap_uv);
        let frame = [knots[*degree], knots[control_points.len()]];
        closure::certify_knot_frame(&loops, axis, frame, budget)?;
        domain[axis][0] = domain[axis][0].max(frame[0]);
        domain[axis][1] = domain[axis][1].min(frame[1]);
        if domain[axis][0] >= domain[axis][1] {
            return Err(unsupported(
                "The revolution trim lies outside its native profile knot domain",
            ));
        }
    }
    let (uv_chart_loops, uv_chart_error_mm) =
        closure::certify(&carrier, &loops, domain, budget, tolerance_mm)?;
    Ok(out::FaceGeometry {
        version: out::VERSION,
        carrier,
        domain,
        loops,
        uv_closed: true,
        uv_chart_loops,
        uv_chart_error_mm,
        forward: face.face().orientation(),
    })
}
pub(crate) fn edge(
    trim: &ParameterCurve<Curve2D, Box<Surface>>,
    face: usize,
    budget: &mut Budget,
) -> Result<out::EdgeGeometry> {
    let range = trim.range_tuple();
    Ok(out::EdgeGeometry {
        version: out::VERSION,
        face,
        curve: curve2(trim.curve(), budget)?,
        range: [range.0, range.1],
    })
}
