//! Derived native edge display. Intersection chord error is estimated by
//! bounded refinement; an analytic parameter carrier uses a proven bound.
use cad_core::{CadError, Result};
use monstertruck::modeling::*;
use std::cell::{Cell, RefCell};

const MAX_POINTS: usize = 1_000_000;
const MAX_EVALUATIONS: usize = 999_000;
const MAX_DEPTH: usize = 40;
const MAX_CYCLES: usize = MAX_POINTS * 3;

fn invalid(message: &str) -> CadError {
    CadError::Invalid(format!("Curve display: {message}"))
}
fn unsupported(message: &str) -> CadError {
    CadError::Unsupported(format!("Curve display: {message}"))
}
fn finite(point: Point3) -> bool {
    [point.x, point.y, point.z].into_iter().all(f64::is_finite)
}
/// Resolve an actual carrier point. An intersection leader is a search guide,
/// and never substitutes for a point on both supporting surfaces.
pub(crate) fn evaluate_point(curve: &Curve, parameter: f64) -> Result<Point3> {
    if !parameter.is_finite() {
        return Err(invalid("The native parameter is nonfinite"));
    }
    let point = if let Curve::IntersectionCurve(intersection) = curve {
        let (point, uv0, uv1) = intersection.search_triple(parameter, 100).ok_or_else(|| {
            invalid("The intersection could not be resolved on both supporting surfaces")
        })?;
        if ![uv0.x, uv0.y, uv1.x, uv1.y].into_iter().all(f64::is_finite) {
            return Err(invalid(
                "Intersection supporting-surface parameters are nonfinite",
            ));
        }
        let first = intersection.surface0().subs(uv0.x, uv0.y);
        let second = intersection.surface1().subs(uv1.x, uv1.y);
        if !finite(point)
            || !finite(first)
            || !finite(second)
            || point.distance(first) > 1e-6
            || point.distance(second) > 1e-6
            || first.distance(second) > 1e-6
        {
            return Err(invalid(
                "Intersection point/supporting-surface residual exceeds 1e-6 mm",
            ));
        }
        point
    } else {
        curve.subs(parameter)
    };
    if !finite(point) {
        return Err(invalid("The native curve point is nonfinite"));
    }
    Ok(point)
}

#[derive(Clone)]
struct BudgetCurve<'a> {
    curve: &'a Curve,
    range: (f64, f64),
    evaluations: &'a Cell<usize>,
    failure: &'a RefCell<Option<CadError>>,
}
impl ParametricCurve for BudgetCurve<'_> {
    type Point = Point3;
    type Vector = Vector3;
    fn evaluate(&self, t: f64) -> Point3 {
        let point = Point3::new(f64::NAN, f64::NAN, f64::NAN);
        if self.failure.borrow().is_some() {
            return point;
        }
        if self.evaluations.get() >= MAX_EVALUATIONS {
            *self.failure.borrow_mut() = Some(unsupported(
                "Native subdivision exceeds its bounded evaluation budget",
            ));
            return point;
        }
        self.evaluations.set(self.evaluations.get() + 1);
        match evaluate_point(self.curve, self.range.0 * (1. - t) + self.range.1 * t) {
            Ok(point) => point,
            Err(error) => {
                *self.failure.borrow_mut() = Some(error);
                point
            }
        }
    }
    fn derivative(&self, t: f64) -> Vector3 {
        self.curve
            .derivative(self.range.0 * (1. - t) + self.range.1 * t)
            * (self.range.1 - self.range.0)
    }
    fn derivative_2(&self, t: f64) -> Vector3 {
        self.curve
            .derivative_2(self.range.0 * (1. - t) + self.range.1 * t)
            * (self.range.1 - self.range.0).powi(2)
    }
    fn derivative_n(&self, order: usize, t: f64) -> Vector3 {
        self.curve
            .derivative_n(order, self.range.0 * (1. - t) + self.range.1 * t)
            * (self.range.1 - self.range.0).powi(order as i32)
    }
}

fn rational_points(curve: &Curve, range: (f64, f64), tolerance: f64) -> Result<Vec<[f64; 3]>> {
    if tolerance < TOLERANCE {
        return Err(unsupported(
            "Native rational display subdivision requires tolerance at least 1e-6 mm",
        ));
    }
    let knots = match curve {
        Curve::BsplineCurve(curve) => curve.knot_vector(),
        Curve::NurbsCurve(curve) => curve.knot_vector(),
        _ => return Err(invalid("Select a rational native display carrier")),
    };
    if knots.len() > MAX_POINTS
        || knots.iter().any(|knot| !knot.is_finite())
        || knots.windows(2).any(|pair| pair[0] > pair[1])
    {
        return Err(invalid(
            "Native knot intervals are nonfinite, unordered or exceed the display resource bound",
        ));
    }
    let mut stations = vec![0.];
    for knot in knots
        .iter()
        .filter(|knot| **knot > range.0 && **knot < range.1)
    {
        let t = (*knot - range.0) / (range.1 - range.0);
        if stations.last().copied() != Some(t) {
            stations.push(t);
        }
    }
    stations.push(1.);
    let evaluations = Cell::new(0);
    let failure = RefCell::new(None);
    let bounded = BudgetCurve {
        curve,
        range,
        evaluations: &evaluations,
        failure: &failure,
    };
    let mut out = Vec::new();
    for station in stations.windows(2) {
        // This is the same native adaptive implementation used by the B-spline
        // and NURBS ParameterDivision1D implementations. The wrapper terminates
        // evaluation before unbounded native vector growth; failure discards
        // the derived result. Normalized intervals also bound float halving.
        let (parameters, points) =
            algo::curve::parameter_division(&bounded, (station[0], station[1]), tolerance);
        if let Some(error) = failure.borrow_mut().take() {
            return Err(error);
        }
        if parameters.len() != points.len()
            || points.len() < 2
            || parameters
                .windows(2)
                .any(|pair| !pair[0].is_finite() || pair[0] >= pair[1])
            || parameters.last().is_none_or(|value| !value.is_finite())
            || points.iter().any(|point| !finite(*point))
        {
            return Err(invalid(
                "Native subdivision returned invalid points or parameter progression",
            ));
        }
        if out
            .len()
            .saturating_add(points.len() - usize::from(!out.is_empty()))
            > MAX_POINTS
        {
            return Err(unsupported(
                "Native curve subdivision exceeds one million display points",
            ));
        }
        out.extend(
            points
                .into_iter()
                .skip(usize::from(!out.is_empty()))
                .map(|point| [point.x, point.y, point.z]),
        );
    }
    Ok(out)
}

struct Interval {
    start: f64,
    end: f64,
    first: Point3,
    last: Point3,
    depth: usize,
}
fn estimated_points(curve: &Curve, range: (f64, f64), tolerance: f64) -> Result<Vec<[f64; 3]>> {
    // These seed points and every refinement point use actual native carriers.
    // Implicit intersections resolve on both supports; leaders never substitute. The deviation estimate is
    // local; it does not certify an unsampled global geometric error bound.
    let mut seeds = Vec::with_capacity(17);
    for index in 0..=16 {
        let t = range.0 * (1. - index as f64 / 16.) + range.1 * (index as f64 / 16.);
        if seeds.last().is_some_and(|(previous, _)| *previous >= t) {
            return Err(unsupported(
                "The native curve interval cannot be finitely subdivided",
            ));
        }
        seeds.push((t, evaluate_point(curve, t)?));
    }
    let mut pending = seeds
        .windows(2)
        .rev()
        .map(|pair| Interval {
            start: pair[0].0,
            end: pair[1].0,
            first: pair[0].1,
            last: pair[1].1,
            depth: 0,
        })
        .collect::<Vec<_>>();
    let mut out = vec![seeds[0].1.into()];
    let mut cycles = 0usize;
    while let Some(interval) = pending.pop() {
        cycles += 1;
        if cycles > MAX_CYCLES {
            return Err(unsupported(
                "Estimated native chord refinement exceeds its cycle budget",
            ));
        }
        let midpoint = interval.start * 0.5 + interval.end * 0.5;
        if !midpoint.is_finite() || midpoint <= interval.start || midpoint >= interval.end {
            return Err(unsupported(
                "Estimated native chord refinement exhausted native parameter precision",
            ));
        }
        let middle = evaluate_point(curve, midpoint)?;
        let quarter = evaluate_point(curve, interval.start * 0.75 + interval.end * 0.25)?;
        let three_quarter = evaluate_point(curve, interval.start * 0.25 + interval.end * 0.75)?;
        let deviation = middle
            .distance(interval.first * 0.5 + interval.last.to_vec() * 0.5)
            .max(quarter.distance(interval.first * 0.75 + interval.last.to_vec() * 0.25))
            .max(three_quarter.distance(interval.first * 0.25 + interval.last.to_vec() * 0.75));
        if !deviation.is_finite() {
            return Err(invalid("Estimated native chord deviation overflow"));
        }
        if deviation > tolerance {
            if interval.depth >= MAX_DEPTH {
                return Err(unsupported(
                    "Estimated native chord error remains above tolerance after 40 refinements",
                ));
            }
            if out.len().saturating_add(pending.len()).saturating_add(3) > MAX_POINTS {
                return Err(unsupported(
                    "Native curve subdivision exceeds one million display points",
                ));
            }
            let depth = interval.depth + 1;
            pending.push(Interval {
                start: midpoint,
                end: interval.end,
                first: middle,
                last: interval.last,
                depth,
            });
            pending.push(Interval {
                start: interval.start,
                end: midpoint,
                first: interval.first,
                last: middle,
                depth,
            });
        } else {
            if out.len() >= MAX_POINTS {
                return Err(unsupported(
                    "Native curve subdivision exceeds one million display points",
                ));
            }
            out.push(interval.last.into());
        }
    }
    Ok(out)
}

pub(crate) fn display_points(curve: &Curve, tolerance: f64) -> Result<Vec<[f64; 3]>> {
    if !tolerance.is_finite() || tolerance <= 0. {
        return Err(invalid("Display tolerance must be finite and positive"));
    }
    let range = curve
        .try_range_tuple()
        .ok_or_else(|| unsupported("Display requires a bounded native curve interval"))?;
    if !range.0.is_finite()
        || !range.1.is_finite()
        || !(range.1 - range.0).is_finite()
        || range.0 >= range.1
    {
        return Err(invalid(
            "Native curve interval must be finite and increasing",
        ));
    }
    let first = evaluate_point(curve, range.0)?;
    let last = evaluate_point(curve, range.1)?;
    match curve {
        Curve::Line(_) => Ok(vec![first.into(), last.into()]),
        Curve::BsplineCurve(_) | Curve::NurbsCurve(_) => rational_points(curve, range, tolerance),
        Curve::IntersectionCurve(_) => estimated_points(curve, range, tolerance),
        Curve::ParameterCurve(_) => {
            let bound = match crate::reference_curves::display_bound(curve) {
                Ok(Some(bound)) => bound,
                Ok(None) | Err(CadError::Unsupported(_)) => {
                    return estimated_points(curve, range, tolerance);
                }
                Err(error) => return Err(error),
            };
            let divisions = (bound / (8. * tolerance)).sqrt().ceil().max(1.);
            if !divisions.is_finite() || divisions >= MAX_POINTS as f64 {
                return Err(unsupported(
                    "Analytic subdivision exceeds one million display points",
                ));
            }
            let divisions = divisions as usize;
            (0..=divisions)
                .map(|index| {
                    let fraction = index as f64 / divisions as f64;
                    evaluate_point(curve, range.0 * (1. - fraction) + range.1 * fraction)
                        .map(Into::into)
                })
                .collect()
        }
    }
}
