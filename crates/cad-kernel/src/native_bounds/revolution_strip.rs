//! Native line-revolution strip enclosures, independent of analytic-to-NURBS
//! conversion. Stored Rodrigues axes need not be exactly unit length, and every
//! affine/inverse operation retains its outward arithmetic error interval.
use super::*;

fn subtract(a: IntervalPoint, b: IntervalPoint) -> Result<IntervalPoint> {
    Ok([a[0].sub(b[0])?, a[1].sub(b[1])?, a[2].sub(b[2])?])
}
fn add(a: IntervalPoint, b: IntervalPoint) -> Result<IntervalPoint> {
    Ok([a[0].add(b[0])?, a[1].add(b[1])?, a[2].add(b[2])?])
}
fn scale(a: IntervalPoint, value: Interval) -> Result<IntervalPoint> {
    Ok([a[0].mul(value)?, a[1].mul(value)?, a[2].mul(value)?])
}
fn cross(a: IntervalPoint, b: IntervalPoint) -> Result<IntervalPoint> {
    Ok([
        a[1].mul(b[2])?.sub(a[2].mul(b[1])?)?,
        a[2].mul(b[0])?.sub(a[0].mul(b[2])?)?,
        a[0].mul(b[1])?.sub(a[1].mul(b[0])?)?,
    ])
}
fn inverse_box(inverse: &InverseAffine, point: IntervalPoint) -> Result<IntervalPoint> {
    let mut local = [Interval::exact(0.)?; 3];
    for (row, coordinate) in local.iter_mut().enumerate() {
        for (column, value) in point.iter().enumerate() {
            *coordinate = coordinate.add(
                Interval::exact(inverse.matrix[column][row])?
                    .mul(value.sub(inverse.translation[column])?)?,
            )?;
        }
    }
    // The parent's Neumann certificate encloses the exact inverse for every
    // point in this box, including the explicitly declared trim residual.
    let magnitude = local.iter().fold(0.0f64, |a, b| a.max(b.magnitude()));
    let error = Interval::exact(magnitude)?
        .mul(Interval::exact(inverse.residual_factor)?)?
        .high;
    for coordinate in &mut local {
        *coordinate = coordinate.widen(error)?;
    }
    Ok(local)
}

struct Orbit {
    center: IntervalPoint,
    cosine: IntervalPoint,
    sine: IntervalPoint,
}
impl Orbit {
    fn new(origin: IntervalPoint, axis: IntervalPoint, point: IntervalPoint) -> Result<Self> {
        let delta = subtract(point, origin)?;
        let axial = scale(axis, dot_intervals(&axis, &delta)?)?;
        Ok(Self {
            center: add(origin, axial)?,
            cosine: subtract(delta, axial)?,
            sine: cross(axis, delta)?,
        })
    }
    fn quadrant(&self, transform: &Matrix4, quarter: usize) -> Result<CurveBounds> {
        // Rational quadratic Bezier data for (cos(theta), sin(theta)) with
        // theta=2 atan(t), 0<=t<=1. Integer coefficients avoid a rounded sqrt(2)
        // or trigonometric endpoint becoming an unaccounted construction error.
        let controls = [[1., 0., 1.], [1., 1., 1.], [0., 2., 2.]]
            .into_iter()
            .map(|[x, y, w]| {
                let (x, y) = match quarter {
                    0 => (x, y),
                    1 => (-y, x),
                    2 => (-x, -y),
                    3 => (y, -x),
                    _ => unreachable!(),
                };
                let (x, y, w) = (
                    Interval::exact(x)?,
                    Interval::exact(y)?,
                    Interval::exact(w)?,
                );
                let local = add(
                    add(scale(self.center, w)?, scale(self.cosine, x)?)?,
                    scale(self.sine, y)?,
                )?;
                let mut world = [Interval::exact(0.)?; 4];
                for row in 0..3 {
                    world[row] = Interval::exact(transform.w[row])?.mul(w)?;
                    for column in 0..3 {
                        world[row] = world[row]
                            .add(Interval::exact(transform[column][row])?.mul(local[column])?)?;
                    }
                }
                world[3] = w;
                Ok(world)
            })
            .collect::<Result<Vec<_>>>()?;
        CurveBounds::from_bezier_controls(controls)
    }
}

/// Enclose the complete native source strip between two actual trim curves.
/// The caller must establish a line-revolution wall bounded by its two rim
/// curves and generatrix seams. The returned four groups cover every angle,
/// so they also enclose any bounded angular subpatch without a trig fit.
///
/// `modeling_tolerance` is the declared, validated world-space trim/carrier
/// residual. It is propagated through the certified affine inverse, never used
/// to assert exact equality of converted curves or to alter the model.
pub(crate) fn source_strip(
    surface: &Surface,
    old_boundary: &Curve,
    new_boundary: &Curve,
    modeling_tolerance: f64,
) -> Result<Vec<Vec<CurveBounds>>> {
    if !modeling_tolerance.is_finite() || modeling_tolerance < 0. {
        return Err(unsupported(
            "A revolved strip needs a finite nonnegative trim residual",
        ));
    }
    let Surface::RevolutionSurface(processor) = surface else {
        return Err(unsupported(
            "A native revolved strip requires a revolution surface",
        ));
    };
    let Curve::Line(Line(start, end)) = processor.entity().entity_curve() else {
        return Err(unsupported(
            "A native revolved strip currently requires a line generator",
        ));
    };
    if !affine(processor.transform()) {
        return Err(unsupported(
            "A native revolved strip requires an affine transform",
        ));
    }
    let inverse = InverseAffine::new(processor.transform())?;
    let origin = exact_point(processor.entity().origin())?;
    let raw_axis = exact_point(Point3::from_vec(processor.entity().axis()))?;
    let squared_length = dot_intervals(&raw_axis, &raw_axis)?;
    let length = squared_length.sqrt()?;
    let axis = [
        raw_axis[0].div(length)?,
        raw_axis[1].div(length)?,
        raw_axis[2].div(length)?,
    ];
    // For stored axis k*n, native Rodrigues evaluation scales the axial
    // coordinate by k²+(1-k²)cos(theta). Keep that entire factor when recovering
    // profile stations rather than treating near-unit normalization as exact.
    let axial_factor = squared_length.add(
        Interval::exact(1.)?
            .sub(squared_length)?
            .mul(Interval::new(-1., 1.)?)?,
    )?;
    if length.low <= 0. || axial_factor.low <= 0. {
        return Err(unsupported(
            "The stored revolution axis cannot certify an invertible axial station",
        ));
    }
    let start = exact_point(*start)?;
    let delta = subtract(exact_point(*end)?, start)?;
    let initial = dot_intervals(&subtract(start, origin)?, &axis)?;
    let slope = dot_intervals(&delta, &axis)?;
    if slope.low <= 0. && slope.high >= 0. {
        return Err(unsupported(
            "The line generator has no certified axial progression",
        ));
    }
    let mut low = f64::INFINITY;
    let mut high = f64::NEG_INFINITY;
    for boundary in [old_boundary, new_boundary] {
        if !matches!(
            boundary,
            Curve::Line(_) | Curve::BsplineCurve(_) | Curve::NurbsCurve(_)
        ) {
            return Err(unsupported(
                "A revolved strip requires explicit native trim controls",
            ));
        }
        let enclosure = curve(boundary)?;
        point_budget(enclosure.len(), 1)?;
        for point in enclosure.points() {
            let mut point = exact_point(*point)?;
            for coordinate in &mut point {
                *coordinate = coordinate.widen(modeling_tolerance)?;
            }
            let local = inverse_box(&inverse, point)?;
            let axial = dot_intervals(&subtract(local, origin)?, &axis)?.div(axial_factor)?;
            let parameter = axial.sub(initial)?.div(slope)?;
            low = low.min(parameter.low);
            high = high.max(parameter.high);
        }
    }
    let parameters = Interval::new(low, high)?;
    let first = Orbit::new(
        origin,
        raw_axis,
        add(start, scale(delta, Interval::exact(parameters.low)?)?)?,
    )?;
    let last = Orbit::new(
        origin,
        raw_axis,
        add(start, scale(delta, Interval::exact(parameters.high)?)?)?,
    )?;
    // Rodrigues evaluation is affine in the line parameter. Convex combinations
    // of these two interval orbit curves therefore enclose every intermediate
    // station, including source trims extending beyond the generator's seed span.
    (0..4)
        .map(|quarter| {
            Ok(vec![
                first.quadrant(processor.transform(), quarter)?,
                last.quadrant(processor.transform(), quarter)?,
            ])
        })
        .collect()
}
