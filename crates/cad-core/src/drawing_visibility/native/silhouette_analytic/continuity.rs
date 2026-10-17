//! Exact positive projective-tangent continuity at native full-degree knot
//! joins. Shared spline basis products suffice without fitting or sampling.
use super::{Q, q};
use crate::{
    Result,
    drawing_geometry::{Carrier, Curve3, FaceGeometry},
};
use num_traits::Zero;

fn h(point: [f64; 4]) -> Option<[Q; 4]> {
    Some([q(point[0])?, q(point[1])?, q(point[2])?, q(point[3])?])
}
fn junctions(degree: usize, knots: &[f64], count: usize) -> Option<Vec<usize>> {
    if degree == 0
        || degree > 32
        || count <= degree
        || knots.len() != count.checked_add(degree)?.checked_add(1)?
        || knots.iter().any(|k| !k.is_finite())
        || knots.windows(2).any(|pair| pair[0] > pair[1])
        || knots[degree] >= knots[count]
    {
        return None;
    }
    let mut result = Vec::new();
    let mut i = 0;
    while i < knots.len() {
        let first = i;
        let value = knots[i];
        while i < knots.len() && knots[i] == value {
            i += 1
        }
        if value <= knots[degree] || value >= knots[count] {
            continue;
        }
        let multiplicity = i - first;
        if multiplicity > degree {
            return None;
        }
        if multiplicity == degree {
            let index = first.checked_sub(1)?;
            if index == 0 || index + 1 >= count {
                return None;
            }
            result.push(index);
        }
    }
    Some(result)
}
fn compare(left: &Q, right: &Q, ratio: &mut Option<Q>) -> bool {
    if left.is_zero() && right.is_zero() {
        return true;
    }
    if left.is_zero() || right.is_zero() {
        return false;
    }
    match ratio {
        Some(ratio) => left == &(right * ratio.clone()),
        None => {
            let candidate = left / right;
            if candidate <= Q::zero() {
                false
            } else {
                *ratio = Some(candidate);
                true
            }
        }
    }
}
fn tangent_join(
    previous: &[[f64; 4]],
    boundary: &[[f64; 4]],
    next: &[[f64; 4]],
    work: &mut usize,
) -> Option<bool> {
    let n = boundary.len();
    if n == 0 || previous.len() != n || next.len() != n {
        return Some(false);
    }
    *work = work.checked_add(n.checked_mul(n + 1)?.checked_div(2)?)?;
    if *work > 32768 {
        return None;
    }
    let boundary = boundary.iter().map(|p| h(*p)).collect::<Option<Vec<_>>>()?;
    let previous = previous.iter().map(|p| h(*p)).collect::<Option<Vec<_>>>()?;
    let next = next.iter().map(|p| h(*p)).collect::<Option<Vec<_>>>()?;
    let left = boundary
        .iter()
        .zip(previous)
        .map(|(b, a)| std::array::from_fn::<_, 4, _>(|i| &b[i] - &a[i]))
        .collect::<Vec<_>>();
    let right = next
        .iter()
        .zip(&boundary)
        .map(|(c, b)| std::array::from_fn::<_, 4, _>(|i| &c[i] - &b[i]))
        .collect::<Vec<_>>();
    let mut ratio = None;
    // Euclidean tangent numerator is H_w D_xyz - H_xyz D_w.
    // The products N_j(v)N_k(v) are shared by both sides. Equal symmetric
    // coefficients prove identity on every knot span without assuming that
    // the basis products are linearly independent.
    for j in 0..n {
        for k in j..n {
            for coordinate in 0..3 {
                let coefficient = |d: &[[Q; 4]]| {
                    let mut value =
                        &d[j][coordinate] * &boundary[k][3] - &boundary[j][coordinate] * &d[k][3];
                    if j != k {
                        value += &d[k][coordinate] * &boundary[j][3]
                            - &boundary[k][coordinate] * &d[j][3];
                    }
                    value
                };
                if !compare(&coefficient(&left), &coefficient(&right), &mut ratio) {
                    return Some(false);
                }
            }
        }
    }
    // Positive one-sided knot-span scale factors were omitted. They preserve
    // the positive proportionality. An identically zero tangent is singular.
    Some(ratio.is_some())
}
pub(in crate::drawing_visibility::native) fn continuous_normals(
    face: &FaceGeometry,
) -> Result<bool> {
    let mut work = 0;
    let proven = (|| -> Option<bool> {
        match &face.carrier {
            Carrier::Rational { surface } => {
                let rows = surface.control_points.len();
                let columns = surface.control_points.first()?.len();
                if columns == 0
                    || surface
                        .control_points
                        .iter()
                        .any(|row| row.len() != columns)
                {
                    return Some(false);
                }
                for axis in 0..2 {
                    let count = if axis == 0 { rows } else { columns };
                    for join in junctions(surface.degree[axis], &surface.knots[axis], count)? {
                        let line = |index: usize| -> Vec<[f64; 4]> {
                            if axis == 0 {
                                surface.control_points[index].clone()
                            } else {
                                surface
                                    .control_points
                                    .iter()
                                    .map(|row| row[index])
                                    .collect()
                            }
                        };
                        if !tangent_join(&line(join - 1), &line(join), &line(join + 1), &mut work)?
                        {
                            return Some(false);
                        }
                    }
                }
                Some(true)
            }
            Carrier::Revolution {
                profile:
                    Curve3::Rational {
                        degree,
                        knots,
                        control_points,
                    },
                ..
            } => {
                for join in junctions(*degree, knots, control_points.len())? {
                    if !tangent_join(
                        &control_points[join - 1..join],
                        &control_points[join..join + 1],
                        &control_points[join + 1..join + 2],
                        &mut work,
                    )? {
                        return Some(false);
                    }
                }
                Some(true)
            }
            _ => Some(true),
        }
    })();
    proven.ok_or_else(|| {
        super::super::unsupported("native knot continuity has no bounded exact certificate")
    })
}
