//! UV loop closure uses exact rational endpoint arithmetic. Native periodic
//! bands receive an explicit derived chart; source loops remain untouched.
use super::{Budget, Interval, out, unsupported};
use cad_core::Result;
use num_rational::BigRational as Q;
use num_traits::{One, Signed, Zero};

fn q(value: f64) -> Result<Q> {
    Q::from_float(value).ok_or_else(|| unsupported("A trim endpoint is not finite"))
}
fn charge(value: &Q, budget: &mut Budget) -> Result<()> {
    let bits = value.numer().bits().saturating_add(value.denom().bits());
    if bits > 65_536 {
        return Err(unsupported(
            "Exact trim endpoint arithmetic exceeded its bit bound",
        ));
    }
    budget.take((bits as usize / 512).max(1))
}

/// A positive rational curve lies in the convex hull of its dehomogenized
/// controls. Compare in homogeneous coordinates so a boundary exactly at a
/// native knot is admitted without a rounded division or tolerance expansion.
pub(super) fn certify_knot_frame(
    loops: &[Vec<out::Curve2>],
    axis: usize,
    frame: [f64; 2],
    budget: &mut Budget,
) -> Result<()> {
    let low = q(frame[0])?;
    let high = q(frame[1])?;
    if axis > 1 || low >= high {
        return Err(unsupported("The native carrier knot frame is invalid"));
    }
    for curve in loops.iter().flatten() {
        match curve {
            out::Curve2::Rational { control_points, .. } => {
                for point in control_points {
                    let weight = q(point[2])?;
                    let coordinate = q(point[axis])?;
                    let minimum = &low * &weight;
                    let maximum = &high * &weight;
                    charge(&minimum, budget)?;
                    charge(&maximum, budget)?;
                    if weight <= Q::zero() || coordinate < minimum || coordinate > maximum {
                        return Err(unsupported(
                            "The complete native UV trim is not certified inside the carrier knot frame",
                        ));
                    }
                }
            }
            out::Curve2::Conic { .. } => {
                budget.take(9)?;
                let bounds = super::curve_bounds(curve)?[axis];
                if bounds[0] < frame[0] || bounds[1] > frame[1] {
                    return Err(unsupported(
                        "The complete native conic trim is not certified inside the carrier knot frame",
                    ));
                }
            }
        }
    }
    Ok(())
}

fn endpoint(curve: &out::Curve2, end: bool, budget: &mut Budget) -> Result<[Q; 2]> {
    match curve {
        out::Curve2::Rational {
            degree,
            knots,
            control_points,
            reversed,
        } => {
            let end = end != *reversed;
            let n = control_points.len();
            if n <= *degree || knots.len() != n + degree + 1 {
                return Err(unsupported("A UV endpoint has invalid knots"));
            }
            let parameter = if end { knots[n] } else { knots[*degree] };
            let clamped = if end {
                knots[n..].iter().all(|k| *k == parameter)
            } else {
                knots[..=*degree].iter().all(|k| *k == parameter)
            };
            let h = if clamped {
                let p = control_points[if end { n - 1 } else { 0 }];
                [q(p[0])?, q(p[1])?, q(p[2])?]
            } else {
                let k = if end {
                    (0..n)
                        .rev()
                        .find(|i| knots[*i] < parameter)
                        .unwrap_or(n - 1)
                } else {
                    (*degree..n)
                        .find(|i| knots[*i + 1] > parameter)
                        .unwrap_or(n - 1)
                };
                if k < *degree {
                    return Err(unsupported(
                        "A UV endpoint lies outside the active knot frame",
                    ));
                }
                let mut work = control_points[k - degree..=k]
                    .iter()
                    .map(|p| Ok([q(p[0])?, q(p[1])?, q(p[2])?]))
                    .collect::<Result<Vec<_>>>()?;
                let parameter = q(parameter)?;
                for r in 1..=*degree {
                    for j in (r..=*degree).rev() {
                        let i = j + k - degree;
                        let denominator = q(knots[i + degree + 1 - r])? - q(knots[i])?;
                        if denominator.is_zero() {
                            return Err(unsupported(
                                "An exact trim endpoint has a singular knot denominator",
                            ));
                        }
                        let alpha = (&parameter - q(knots[i])?) / denominator;
                        let beta = Q::one() - &alpha;
                        let updated =
                            std::array::from_fn(|c| &work[j - 1][c] * &beta + &work[j][c] * &alpha);
                        for coordinate in &updated {
                            charge(coordinate, budget)?;
                        }
                        work[j] = updated;
                    }
                }
                work.pop()
                    .ok_or_else(|| unsupported("A UV endpoint has no control values"))?
            };
            if h[2] <= Q::zero() {
                return Err(unsupported("A UV endpoint has nonpositive weight"));
            }
            Ok([&h[0] / &h[2], &h[1] / &h[2]])
        }
        out::Curve2::Conic {
            conic,
            transform,
            range,
            reversed,
        } => {
            let t = range[usize::from(end != *reversed)];
            let p = match conic {
                out::Conic::Parabola => {
                    let t = q(t)?;
                    [&t * &t, t * Q::from_integer(2.into())]
                }
                out::Conic::Ellipse | out::Conic::Hyperbola if t == 0. => [Q::one(), Q::zero()],
                _ => {
                    return Err(unsupported(
                        "A transcendental UV endpoint has no exact closure certificate",
                    ));
                }
            };
            Ok([
                q(transform[0][0])? * &p[0] + q(transform[0][1])? * &p[1] + q(transform[0][2])?,
                q(transform[1][0])? * &p[0] + q(transform[1][1])? * &p[1] + q(transform[1][2])?,
            ])
        }
    }
}
fn ends(boundary: &[out::Curve2], budget: &mut Budget) -> Result<([Q; 2], [Q; 2])> {
    let first = boundary
        .first()
        .ok_or_else(|| unsupported("A native UV loop is empty"))?;
    let mut previous = endpoint(first, true, budget)?;
    let start = endpoint(first, false, budget)?;
    for curve in &boundary[1..] {
        if previous != endpoint(curve, false, budget)? {
            return Err(unsupported(
                "Consecutive native UV edges have no exact common endpoint",
            ));
        }
        previous = endpoint(curve, true, budget)?;
    }
    Ok((start, previous))
}
fn raw_endpoint(curve: &out::Curve2, end: bool) -> Result<[f64; 3]> {
    let out::Curve2::Rational {
        degree,
        knots,
        control_points,
        reversed,
    } = curve
    else {
        return Err(unsupported(
            "A virtual periodic seam needs a rational endpoint",
        ));
    };
    let end = end != *reversed;
    let n = control_points.len();
    let t = if end { knots[n] } else { knots[*degree] };
    let clamped = if end {
        knots[n..].iter().all(|k| *k == t)
    } else {
        knots[..=*degree].iter().all(|k| *k == t)
    };
    if !clamped {
        return Err(unsupported(
            "A virtual periodic seam needs a clamped native endpoint",
        ));
    }
    Ok(control_points[if end { n - 1 } else { 0 }])
}
fn connector(a: [f64; 3], b: [f64; 3]) -> out::Curve2 {
    out::Curve2::Rational {
        degree: 1,
        knots: vec![0., 0., 1., 1.],
        control_points: vec![a, b],
        reversed: false,
    }
}
fn periodic_axes(carrier: &out::Carrier) -> Vec<usize> {
    match carrier {
        out::Carrier::Revolution { swap_uv, .. } | out::Carrier::Sphere { swap_uv, .. } => {
            vec![usize::from(!*swap_uv)]
        }
        out::Carrier::Torus { .. } => vec![0, 1],
        _ => Vec::new(),
    }
}
fn axis_path(boundary: &[out::Curve2], axis: usize, start: &[Q; 2], end: &[Q; 2]) -> Result<bool> {
    if start[1 - axis] != end[1 - axis]
        || (&end[axis] - &start[axis]).abs() != q(std::f64::consts::TAU)?
    {
        return Ok(false);
    }
    let low = start[axis].clone().min(end[axis].clone());
    let high = start[axis].clone().max(end[axis].clone());
    let increasing = end[axis] > start[axis];
    let mut previous = start[axis].clone();
    for curve in boundary {
        let out::Curve2::Rational {
            degree,
            control_points,
            reversed,
            ..
        } = curve
        else {
            return Ok(false);
        };
        // A degree-one path has no unresolved reversals or hidden angular turns.
        if *degree != 1 {
            return Ok(false);
        }
        for index in 0..control_points.len() {
            let p = &control_points[if *reversed {
                control_points.len() - 1 - index
            } else {
                index
            }];
            let w = q(p[2])?;
            if q(p[1 - axis])? / &w != start[1 - axis] {
                return Ok(false);
            }
            let t = q(p[axis])? / &w;
            if t < low || t > high {
                return Ok(false);
            }
            if (increasing && t < previous) || (!increasing && t > previous) {
                return Ok(false);
            }
            previous = t;
        }
    }
    Ok(true)
}
fn interval_vector(p: [f64; 3]) -> Result<[Interval; 3]> {
    Ok([
        Interval::exact(p[0])?,
        Interval::exact(p[1])?,
        Interval::exact(p[2])?,
    ])
}
fn period_error(carrier: &out::Carrier, domain: [[f64; 2]; 2], angle_axis: usize) -> Result<f64> {
    let (speed, transform) = match carrier {
        out::Carrier::Sphere {
            radius, transform, ..
        } => (Interval::exact(*radius)?, transform),
        out::Carrier::Torus {
            major_radius,
            minor_radius,
            transform,
            swap_uv,
            ..
        } => {
            let entity_axis = if *swap_uv { 1 - angle_axis } else { angle_axis };
            (
                if entity_axis == 0 {
                    Interval::exact(*major_radius)?.add(Interval::exact(*minor_radius)?)?
                } else {
                    Interval::exact(*minor_radius)?
                },
                transform,
            )
        }
        out::Carrier::Revolution {
            profile,
            origin,
            axis,
            transform,
            ..
        } => {
            let profile_axis = 1 - angle_axis;
            let points = match profile {
                out::Curve3::Line { start, end } => {
                    let t = Interval::new(domain[profile_axis][0], domain[profile_axis][1])?;
                    let a = interval_vector(*start)?;
                    let b = interval_vector(*end)?;
                    vec![[
                        (a[0].add(b[0].sub(a[0])?.mul(t)?)?),
                        (a[1].add(b[1].sub(a[1])?.mul(t)?)?),
                        (a[2].add(b[2].sub(a[2])?.mul(t)?)?),
                    ]]
                }
                out::Curve3::Rational { control_points, .. } => control_points
                    .iter()
                    .map(|p| {
                        let w = Interval::exact(p[3])?;
                        Ok([
                            Interval::exact(p[0])?.div(w)?,
                            Interval::exact(p[1])?.div(w)?,
                            Interval::exact(p[2])?.div(w)?,
                        ])
                    })
                    .collect::<Result<Vec<_>>>()?,
            };
            let mut maximum = 0.0f64;
            for p in points {
                for i in 0..3 {
                    maximum = maximum.max(p[i].sub(Interval::exact(origin[i])?)?.magnitude());
                }
            }
            let mut axis_sum = Interval::exact(0.)?;
            for value in axis {
                axis_sum = axis_sum.add(Interval::exact(value.abs())?)?;
            }
            let speed = Interval::exact(maximum)?.mul(
                Interval::exact(1.)?
                    .add(axis_sum)?
                    .add(axis_sum.mul(axis_sum)?)?,
            )?;
            (speed, transform)
        }
        _ => {
            return Err(unsupported(
                "The carrier has no declared native angular period",
            ));
        }
    };
    let mut norm = 0.0f64;
    for row in transform.iter().take(3) {
        let mut sum = Interval::exact(0.)?;
        for value in row.iter().take(3) {
            sum = sum.add(Interval::exact(value.abs())?)?;
        }
        norm = norm.max(sum.high);
    }
    let tau = std::f64::consts::TAU;
    let gap = Interval::exact(tau)?
        .sub(Interval::new(tau.next_down(), tau.next_up())?)?
        .magnitude();
    // Twice the component bound encloses the Euclidean norm (sqrt(3)<2).
    Ok(speed
        .mul(Interval::exact(norm)?)?
        .mul(Interval::exact(2.)?)?
        .mul(Interval::exact(gap)?)?
        .high)
}

type Chart = Option<Vec<Vec<out::Curve2>>>;
pub(super) fn certify(
    carrier: &out::Carrier,
    loops: &[Vec<out::Curve2>],
    domain: [[f64; 2]; 2],
    budget: &mut Budget,
    tolerance_mm: f64,
) -> Result<(Chart, f64)> {
    let endpoints = loops
        .iter()
        .map(|boundary| ends(boundary, budget))
        .collect::<Result<Vec<_>>>()?;
    let open = endpoints
        .iter()
        .enumerate()
        .filter_map(|(i, (a, b))| (a != b).then_some(i))
        .collect::<Vec<_>>();
    if open.is_empty() {
        return Ok((None, 0.));
    }
    if open.len() != 2 {
        return Err(unsupported(
            "The face has an unresolved native UV seam or open trim",
        ));
    }
    for axis in periodic_axes(carrier) {
        let (a, b) = (open[0], open[1]);
        if !axis_path(&loops[a], axis, &endpoints[a].0, &endpoints[a].1)?
            || !axis_path(&loops[b], axis, &endpoints[b].0, &endpoints[b].1)?
        {
            continue;
        }
        if endpoints[a].0[axis] != endpoints[b].1[axis]
            || endpoints[a].1[axis] != endpoints[b].0[axis]
            || endpoints[a].0[1 - axis] == endpoints[b].0[1 - axis]
        {
            continue;
        }
        let error = period_error(carrier, domain, axis)?;
        if !error.is_finite() || error > tolerance_mm / 16. {
            return Err(unsupported(
                "The native periodic chart seam error exceeds the modeling allowance",
            ));
        }
        let copied_controls = loops
            .iter()
            .flatten()
            .try_fold(4usize, |count, curve| {
                count.checked_add(match curve {
                    out::Curve2::Rational { control_points, .. } => control_points.len(),
                    out::Curve2::Conic { .. } => 9,
                })
            })
            .ok_or_else(|| unsupported("The derived periodic chart exceeds its control count"))?;
        budget.take(copied_controls)?;
        let mut chart = loops
            .iter()
            .enumerate()
            .filter_map(|(i, l)| (!open.contains(&i)).then_some(l.clone()))
            .collect::<Vec<_>>();
        let mut band = loops[a].clone();
        band.push(connector(
            raw_endpoint(loops[a].last().unwrap(), true)?,
            raw_endpoint(&loops[b][0], false)?,
        ));
        band.extend(loops[b].clone());
        band.push(connector(
            raw_endpoint(loops[b].last().unwrap(), true)?,
            raw_endpoint(&loops[a][0], false)?,
        ));
        let (start, end) = ends(&band, budget)?;
        if start != end {
            return Err(unsupported(
                "The derived native periodic chart is not closed",
            ));
        }
        chart.push(band);
        return Ok((Some(chart), error));
    }
    Err(unsupported(
        "The native UV seams have no proven closed chart",
    ))
}
