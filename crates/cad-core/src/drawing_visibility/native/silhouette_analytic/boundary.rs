//! A front native boundary can cover another whole boundary in projection.
//! Equality is proved in the original positive homogeneous spline basis, not
//! by comparing tessellation points or accepting a near-coincident projection.
use super::super::{Edge, ProjectedScene};
use super::{Q, q};
use crate::{
    Result,
    drawing_geometry::{self, Carrier, Curve2, Curve3},
};
use num_traits::{One, Zero};

struct ProjectedCurve {
    degree: usize,
    knots: Vec<Q>,
    controls: Vec<[Q; 4]>,
}

fn work(shape: (usize, usize, usize)) -> usize {
    shape
        .2
        .saturating_add(shape.1.saturating_mul(8))
        .saturating_mul(64)
}

fn shape(scene: &ProjectedScene, edge: &Edge) -> Option<(usize, usize, usize)> {
    if let Some(native) = &edge.on_surface {
        if native.version != drawing_geometry::VERSION
            || !matches!(
                scene.faces.get(native.face)?.geometry.carrier,
                Carrier::Plane { .. }
            )
        {
            return None;
        }
        return match &native.curve {
            Curve2::Rational {
                degree,
                knots,
                control_points,
                ..
            } => Some((*degree, control_points.len(), knots.len())),
            _ => None,
        };
    }
    Some(match edge.spatial.as_ref()? {
        Curve3::Line { .. } => (1, 2, 4),
        Curve3::Rational {
            degree,
            knots,
            control_points,
        } => (*degree, control_points.len(), knots.len()),
    })
}

fn homogeneous(point: [f64; 4]) -> Option<[Q; 4]> {
    Some([q(point[0])?, q(point[1])?, q(point[2])?, q(point[3])?])
}

fn project(scene: &ProjectedScene, edge: &Edge) -> Option<ProjectedCurve> {
    let (degree, knots, controls, placement, restriction) = if let Some(native) = &edge.on_surface {
        if native.version != drawing_geometry::VERSION {
            return None;
        }
        let face = scene.faces.get(native.face)?;
        let Carrier::Plane {
            origin,
            u_point,
            v_point,
        } = face.geometry.carrier
        else {
            return None;
        };
        let Curve2::Rational {
            degree,
            knots,
            control_points,
            ..
        } = &native.curve
        else {
            return None;
        };
        let origin = origin.into_iter().map(q).collect::<Option<Vec<_>>>()?;
        let u = u_point.into_iter().map(q).collect::<Option<Vec<_>>>()?;
        let v = v_point.into_iter().map(q).collect::<Option<Vec<_>>>()?;
        let controls = control_points
            .iter()
            .map(|p| {
                let (x, y, w) = (q(p[0])?, q(p[1])?, q(p[2])?);
                // Lift exactly the pcurve that split_native evaluates. Subtract
                // original plane defining points in rational arithmetic, with no
                // intermediate rounded axes or surrogate spatial-edge controls.
                let xyz: [Q; 3] = std::array::from_fn(|i| {
                    &origin[i] * &w + (&u[i] - &origin[i]) * &x + (&v[i] - &origin[i]) * &y
                });
                Some([xyz[0].clone(), xyz[1].clone(), xyz[2].clone(), w])
            })
            .collect::<Option<Vec<_>>>()?;
        (
            *degree,
            knots.clone(),
            controls,
            &face.matrix,
            Some(native.range),
        )
    } else {
        match edge.spatial.as_ref()? {
            Curve3::Line { start, end } => (
                1,
                vec![0., 0., 1., 1.],
                vec![
                    homogeneous([start[0], start[1], start[2], 1.])?,
                    homogeneous([end[0], end[1], end[2], 1.])?,
                ],
                &edge.matrix,
                None,
            ),
            Curve3::Rational {
                degree,
                knots,
                control_points,
            } => (
                *degree,
                knots.clone(),
                control_points
                    .iter()
                    .map(|p| homogeneous(*p))
                    .collect::<Option<Vec<_>>>()?,
                &edge.matrix,
                None,
            ),
        }
    };
    if degree == 0
        || degree > 32
        || controls.len() <= degree
        || knots.len() != controls.len().checked_add(degree)?.checked_add(1)?
        || knots.iter().any(|x| !x.is_finite())
        || knots.windows(2).any(|pair| pair[0] > pair[1])
        || placement[3] != [0., 0., 0., 1.]
    {
        return None;
    }
    // A clipped pcurve does not cover its complete basis domain. Proving a
    // restricted reparameterization needs exact knot insertion; decline here.
    if restriction.is_some_and(|range| range != [knots[degree], knots[controls.len()]]) {
        return None;
    }
    let first = q(knots[degree])?;
    let width = q(knots[controls.len()])? - &first;
    if width <= Q::zero() {
        return None;
    }
    let knots = knots
        .into_iter()
        .map(|k| Some((q(k)? - &first) / &width))
        .collect::<Option<Vec<_>>>()?;
    let matrix = placement
        .iter()
        .map(|row| row.iter().map(|x| q(*x)).collect())
        .collect::<Option<Vec<Vec<Q>>>>()?;
    let basis = scene
        .basis
        .iter()
        .map(|row| row.iter().map(|x| q(*x)).collect())
        .collect::<Option<Vec<Vec<Q>>>>()?;
    let controls = controls
        .into_iter()
        .map(|p| {
            if p[3] <= Q::zero() {
                return None;
            }
            let transformed: [Q; 3] =
                std::array::from_fn(|i| (0..4).map(|j| &matrix[i][j] * &p[j]).sum());
            let projected: [Q; 3] =
                std::array::from_fn(|i| (0..3).map(|j| &basis[i][j] * &transformed[j]).sum());
            Some([
                projected[0].clone(),
                projected[1].clone(),
                projected[2].clone(),
                p[3].clone(),
            ])
        })
        .collect::<Option<Vec<_>>>()?;
    Some(ProjectedCurve {
        degree,
        knots,
        controls,
    })
}

fn covers(source: &ProjectedCurve, front: &ProjectedCurve, tolerance: &Q, reverse: bool) -> bool {
    if source.degree != front.degree
        || source.controls.len() != front.controls.len()
        || source.knots.len() != front.knots.len()
    {
        return false;
    }
    let knots_equal = source.knots.iter().enumerate().all(|(i, knot)| {
        if reverse {
            knot == &(Q::one() - &front.knots[front.knots.len() - 1 - i])
        } else {
            knot == &front.knots[i]
        }
    });
    if !knots_equal {
        return false;
    }
    let index = |i| {
        if reverse {
            front.controls.len() - 1 - i
        } else {
            i
        }
    };
    let scale = &front.controls[index(0)][3] / &source.controls[0][3];
    // Positive weights and a common knot vector make the basis nonnegative
    // over its active domain. Identical projected homogeneous coefficients
    // then prove the same entire XY locus (possibly traversed backwards).
    // The strict depth coefficient inequality survives the same weighted sum
    // and division, proving coverage beyond the modeling depth tolerance.
    source.controls.iter().enumerate().all(|(i, source)| {
        let front = &front.controls[index(i)];
        (0..=1).all(|j| front[j] == &scale * &source[j])
            && front[3] == &scale * &source[3]
            && &front[2] - &scale * &source[2] > tolerance * &front[3]
    })
}

pub(in crate::drawing_visibility::native) fn boundary_occludes(
    scene: &mut ProjectedScene,
    source_index: usize,
) -> Result<bool> {
    let Some(source) = scene.edges.get(source_index) else {
        return Ok(false);
    };
    if source.ghost {
        return Ok(false);
    }
    let Some(source_shape) = shape(scene, source) else {
        return Ok(false);
    };
    scene.budget.charge(work(source_shape))?;
    let Some(source) = project(scene, &scene.edges[source_index]) else {
        return Ok(false);
    };
    let Some(tolerance) = q(scene.accuracy.modeling.linear_mm) else {
        return Ok(false);
    };
    for candidate in 0..scene.edges.len() {
        scene.budget.charge(1)?;
        let front = &scene.edges[candidate];
        if candidate == source_index || front.ghost || front.on_surface.is_none() {
            continue;
        }
        let Some(front_shape) = shape(scene, front) else {
            continue;
        };
        if front_shape != source_shape {
            continue;
        }
        scene.budget.charge(work(front_shape))?;
        let Some(front) = project(scene, &scene.edges[candidate]) else {
            continue;
        };
        if covers(&source, &front, &tolerance, false) || covers(&source, &front, &tolerance, true) {
            return Ok(true);
        }
    }
    Ok(false)
}
