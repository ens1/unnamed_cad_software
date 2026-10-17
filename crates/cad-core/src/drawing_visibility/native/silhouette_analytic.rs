//! Analytic cases whose entire projected image is already carried by native
//! boundary curves. Exact binary64 rational algebra proves redundancy; a zero
//! sampled or rounded normal is never sufficient.
use super::{Face, ProjectedScene};
use crate::{
    Result,
    drawing_geometry::{Carrier, Curve2, Curve3},
};
use num_rational::BigRational as Q;
use num_traits::Zero;
mod boundary;
mod continuity;
pub(super) use boundary::boundary_occludes;
pub(super) use continuity::continuous_normals;

fn q(value: f64) -> Option<Q> {
    Q::from_float(value)
}
fn vector(value: [f64; 3]) -> Option<[Q; 3]> {
    Some([q(value[0])?, q(value[1])?, q(value[2])?])
}
fn rational_point(point: [f64; 3]) -> Option<[Q; 2]> {
    let w = q(point[2])?;
    if w <= Q::zero() {
        return None;
    }
    Some([q(point[0])? / &w, q(point[1])? / w])
}
fn apply(matrix: &[[f64; 4]; 4], point: &[Q; 3]) -> Option<[Q; 3]> {
    let mut result = [Q::zero(), Q::zero(), Q::zero()];
    for row in 0..3 {
        for column in 0..3 {
            result[row] += q(matrix[row][column])? * &point[column];
        }
    }
    Some(result)
}
fn projected_zero(
    scene: &ProjectedScene,
    face: &Face,
    carrier_transform: &[[f64; 4]; 4],
    vector: [Q; 3],
) -> Option<bool> {
    let transformed = apply(&face.matrix, &apply(carrier_transform, &vector)?)?;
    for row in 0..2 {
        let mut value = Q::zero();
        for (column, p) in transformed.iter().enumerate() {
            value += q(scene.basis[row][column])? * p;
        }
        if !value.is_zero() {
            return Some(false);
        }
    }
    Some(true)
}
fn proof(scene: &ProjectedScene, face: &Face) -> Option<bool> {
    let Carrier::Revolution {
        profile: Curve3::Line { start, end },
        axis,
        transform,
        swap_uv,
        ..
    } = &face.geometry.carrier
    else {
        return Some(false);
    };
    if !face.geometry.uv_closed {
        return Some(false);
    }
    let (a, start, end) = (vector(*axis)?, vector(*start)?, vector(*end)?);
    let delta: [Q; 3] = std::array::from_fn(|i| &end[i] - &start[i]);
    let dot = &a[0] * &delta[0] + &a[1] * &delta[1] + &a[2] * &delta[2];
    let axial: [Q; 3] = std::array::from_fn(|i| &a[i] * &dot);
    let cosine = std::array::from_fn(|i| &delta[i] - &axial[i]);
    let sine = [
        &a[1] * &delta[2] - &a[2] * &delta[1],
        &a[2] * &delta[0] - &a[0] * &delta[2],
        &a[0] * &delta[1] - &a[1] * &delta[0],
    ];
    // Rodrigues gives P(R(v)D) = P(a(a·D)) + cos(v)P(D-a(a·D))
    // + sin(v)P(a×D). All three coefficients must vanish exactly, including
    // the original stored axis and every affine placement coefficient.
    if !projected_zero(scene, face, transform, axial)?
        || !projected_zero(scene, face, transform, cosine)?
        || !projected_zero(scene, face, transform, sine)?
    {
        return Some(false);
    }
    let angle = usize::from(!*swap_uv);
    let mut angular_hull: Option<(Q, Q)> = None;
    for boundary in face.geometry.loops.iter().flatten() {
        let Curve2::Rational { control_points, .. } = boundary else {
            return Some(false);
        };
        for point in control_points {
            let value = rational_point(*point)?[angle].clone();
            angular_hull = Some(match angular_hull {
                None => (value.clone(), value),
                Some((low, high)) => (low.min(value.clone()), high.max(value)),
            });
        }
    }
    let (hull_low, hull_high) = angular_hull?;
    for candidate in face.geometry.loops.iter().flatten() {
        let Curve2::Rational {
            degree,
            knots,
            control_points,
            ..
        } = candidate
        else {
            continue;
        };
        if *degree != 1 || control_points.len() < 2 || knots.len() != control_points.len() + 2 {
            continue;
        }
        if knots[0] != knots[1] || knots[knots.len() - 1] != knots[knots.len() - 2] {
            continue;
        }
        let endpoints = [
            rational_point(control_points[0])?,
            rational_point(*control_points.last()?)?,
        ];
        if endpoints[0][1 - angle] != endpoints[1][1 - angle] {
            continue;
        }
        let low = endpoints[0][angle].clone().min(endpoints[1][angle].clone());
        let high = endpoints[0][angle].clone().max(endpoints[1][angle].clone());
        if high == low {
            continue;
        }
        // Every point on a continuous degree-one path between these endpoints
        // reaches each intervening angle. Its fixed profile coordinate makes
        // this a boundary carrier for the whole projected angular image.
        if control_points
            .iter()
            .any(|p| rational_point(*p).is_none_or(|p| p[1 - angle] != endpoints[0][1 - angle]))
        {
            continue;
        }
        // The complete closed UV trim lies in the convex hull of its positive
        // rational boundary controls. Every admitted angular parameter is
        // therefore represented by the native boundary path above. Holes do
        // not add an image outside that already emitted path.
        if hull_low >= low && hull_high <= high {
            return Some(true);
        }
    }
    Some(false)
}

pub(super) fn redundant(scene: &mut ProjectedScene, face: &Face) -> Result<bool> {
    scene.budget.charge(
        2048 + face
            .geometry
            .loops
            .iter()
            .flatten()
            .map(|curve| match curve {
                Curve2::Rational { control_points, .. } => control_points.len(),
                _ => 1,
            })
            .sum::<usize>(),
    )?;
    Ok(proof(scene, face).unwrap_or(false))
}
