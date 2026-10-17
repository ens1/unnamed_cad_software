use super::{
    interval::{I, J},
    unsupported,
};
use crate::{
    Result,
    drawing_geometry::{Carrier, Conic, Curve2, Curve3, FaceGeometry},
};

pub(super) fn affine(p: [J; 3], m: [[f64; 4]; 4]) -> [J; 3] {
    std::array::from_fn(|i| {
        p[0] * m[i][0].into() + p[1] * m[i][1].into() + p[2] * m[i][2].into() + m[i][3].into()
    })
}
fn deboor<const N: usize>(
    degree: usize,
    knots: &[f64],
    controls: &[[J; N]],
    t: J,
) -> Result<[J; N]> {
    if degree > 32
        || controls.len() <= degree
        || knots.len() != controls.len() + degree + 1
        || knots.iter().any(|x| !x.is_finite())
        || knots.windows(2).any(|x| x[0] > x[1])
    {
        return Err(unsupported("invalid native spline definition"));
    }
    let last = controls.len() - 1;
    let k = (degree..=last)
        .find(|k| t.x.mid() < knots[*k + 1])
        .unwrap_or(last);
    if t.x.lo < knots[k] || t.x.hi > knots[k + 1] {
        return Err(unsupported(
            "native spline interval crosses a knot; its parameter cells must be split",
        ));
    }
    let mut d = controls[k - degree..=k].to_vec();
    for r in 1..=degree {
        for j in (r..=degree).rev() {
            let i = j + k - degree;
            let denominator = knots[i + degree + 1 - r] - knots[i];
            if denominator <= 0. {
                return Err(unsupported("singular native spline knot interval"));
            }
            let alpha =
                (t - knots[i].into()) / (J::from(knots[i + degree + 1 - r]) - J::from(knots[i]));
            d[j] = std::array::from_fn(|c| d[j - 1][c] * (J::from(1.) - alpha) + d[j][c] * alpha);
        }
    }
    Ok(d[degree])
}
pub(super) fn curve3(curve: &Curve3, t: J) -> Result<[J; 3]> {
    match curve {
        Curve3::Line { start, end } => Ok(std::array::from_fn(|i| {
            J::from(start[i]) + t * (J::from(end[i]) - start[i].into())
        })),
        Curve3::Rational {
            degree,
            knots,
            control_points,
        } => {
            if control_points
                .iter()
                .any(|p| p[3] <= 0. || p.iter().any(|x| !x.is_finite()))
            {
                return Err(unsupported(
                    "native curve requires finite positive rational weights",
                ));
            }
            let mut p = deboor(
                *degree,
                knots,
                &control_points
                    .iter()
                    .map(|p| p.map(J::from))
                    .collect::<Vec<_>>(),
                t,
            )?;
            p[3].x = I::new(
                p[3].x.lo.max(
                    control_points
                        .iter()
                        .map(|p| p[3])
                        .fold(f64::INFINITY, f64::min),
                ),
                p[3].x
                    .hi
                    .min(control_points.iter().map(|p| p[3]).fold(0., f64::max)),
            );
            Ok(std::array::from_fn(|i| p[i] / p[3]))
        }
    }
}
pub(super) fn curve2(curve: &Curve2, t: J) -> Result<[J; 2]> {
    match curve {
        Curve2::Rational {
            degree,
            knots,
            control_points,
            ..
        } => {
            if control_points
                .iter()
                .any(|p| p[2] <= 0. || p.iter().any(|x| !x.is_finite()))
            {
                return Err(unsupported(
                    "native trim requires finite positive rational weights",
                ));
            }
            let mut p = deboor(
                *degree,
                knots,
                &control_points
                    .iter()
                    .map(|p| p.map(J::from))
                    .collect::<Vec<_>>(),
                t,
            )?;
            p[2].x = I::new(
                p[2].x.lo.max(
                    control_points
                        .iter()
                        .map(|p| p[2])
                        .fold(f64::INFINITY, f64::min),
                ),
                p[2].x
                    .hi
                    .min(control_points.iter().map(|p| p[2]).fold(0., f64::max)),
            );
            let mut result = [p[0] / p[2], p[1] / p[2]];
            for axis in 0..2 {
                if control_points
                    .iter()
                    .all(|v| v[axis] == control_points[0][axis] && v[2] == control_points[0][2])
                {
                    result[axis] = J::constant(
                        I::point(control_points[0][axis]) / control_points[0][2].into(),
                    );
                }
            }
            Ok(result)
        }
        Curve2::Conic {
            conic, transform, ..
        } => {
            let p = match conic {
                Conic::Ellipse => [t.cos(), t.sin()],
                Conic::Parabola => [t * t, t * 2.0.into()],
                Conic::Hyperbola => [
                    (t.exp() + (-t).exp()) / 2.0.into(),
                    (t.exp() - (-t).exp()) / 2.0.into(),
                ],
            };
            let h: [J; 3] = std::array::from_fn(|i| {
                p[0] * transform[i][0].into()
                    + p[1] * transform[i][1].into()
                    + transform[i][2].into()
            });
            Ok([h[0] / h[2], h[1] / h[2]])
        }
    }
}
pub(super) fn curve_domains(curve: &Curve2) -> Result<Vec<[f64; 2]>> {
    match curve {
        Curve2::Rational {
            degree,
            knots,
            control_points,
            ..
        } => {
            if knots.len() != control_points.len() + degree + 1 || control_points.len() <= *degree {
                return Err(unsupported("invalid native trim knot vector"));
            }
            Ok(knots[*degree..=control_points.len()]
                .windows(2)
                .filter(|p| p[1] > p[0])
                .map(|p| [p[0], p[1]])
                .collect())
        }
        Curve2::Conic { range, .. } => Ok(vec![*range]),
    }
}
fn surface_cell(
    face: &FaceGeometry,
    domain: [[f64; 2]; 2],
    placement: [[f64; 4]; 4],
) -> Result<[J; 3]> {
    let u = J::variable(I::new(domain[0][0], domain[0][1]), 0);
    let v = J::variable(I::new(domain[1][0], domain[1][1]), 1);
    let p = match &face.carrier {
        Carrier::Plane {
            origin,
            u_point: du,
            v_point: dv,
        } => std::array::from_fn(|i| {
            J::from(origin[i])
                + u * (J::from(du[i]) - origin[i].into())
                + v * (J::from(dv[i]) - origin[i].into())
        }),
        Carrier::Rational { surface: s } => {
            if s.control_points.is_empty()
                || s.control_points
                    .iter()
                    .any(|r| r.len() != s.control_points[0].len())
                || s.control_points
                    .iter()
                    .flatten()
                    .any(|p| p[3] <= 0. || p.iter().any(|x| !x.is_finite()))
            {
                return Err(unsupported("invalid native tensor surface"));
            }
            let columns = s
                .control_points
                .iter()
                .map(|row| {
                    deboor(
                        s.degree[1],
                        &s.knots[1],
                        &row.iter().map(|p| p.map(J::from)).collect::<Vec<_>>(),
                        v,
                    )
                })
                .collect::<Result<Vec<_>>>()?;
            let mut h = deboor(s.degree[0], &s.knots[0], &columns, u)?;
            h[3].x = I::new(
                h[3].x.lo.max(
                    s.control_points
                        .iter()
                        .flatten()
                        .map(|p| p[3])
                        .fold(f64::INFINITY, f64::min),
                ),
                h[3].x.hi.min(
                    s.control_points
                        .iter()
                        .flatten()
                        .map(|p| p[3])
                        .fold(0., f64::max),
                ),
            );
            std::array::from_fn(|i| h[i] / h[3])
        }
        Carrier::Sphere {
            center,
            radius,
            transform,
            swap_uv,
        } => {
            let (u, v) = if *swap_uv { (v, u) } else { (u, v) };
            affine(
                [
                    J::from(center[0]) + u.sin() * v.cos() * (*radius).into(),
                    J::from(center[1]) + u.sin() * v.sin() * (*radius).into(),
                    J::from(center[2]) + u.cos() * (*radius).into(),
                ],
                *transform,
            )
        }
        Carrier::Torus {
            center,
            major_radius,
            minor_radius,
            transform,
            swap_uv,
        } => {
            let (u, v) = if *swap_uv { (v, u) } else { (u, v) };
            let r = J::from(*major_radius) + v.cos() * (*minor_radius).into();
            affine(
                [
                    J::from(center[0]) + r * u.cos(),
                    J::from(center[1]) + r * u.sin(),
                    J::from(center[2]) + v.sin() * (*minor_radius).into(),
                ],
                *transform,
            )
        }
        Carrier::Revolution {
            profile,
            origin,
            axis,
            transform,
            swap_uv,
        } => {
            let (u, v) = if *swap_uv { (v, u) } else { (u, v) };
            let p = curve3(profile, u)?;
            let q: [J; 3] = std::array::from_fn(|i| p[i] - origin[i].into());
            let axial = q[0] * axis[0].into() + q[1] * axis[1].into() + q[2] * axis[2].into();
            let crossed = [
                q[2] * axis[1].into() - q[1] * axis[2].into(),
                q[0] * axis[2].into() - q[2] * axis[0].into(),
                q[1] * axis[0].into() - q[0] * axis[1].into(),
            ];
            affine(
                std::array::from_fn(|i| {
                    J::from(origin[i])
                        + q[i] * v.cos()
                        + crossed[i] * v.sin()
                        + axial * axis[i].into() * (J::from(1.) - v.cos())
                }),
                *transform,
            )
        }
    };
    let out = affine(p, placement);
    if out.iter().any(|p| {
        !p.x.finite()
            || p.d.iter().any(|d| !d.finite())
            || p.dd.iter().flatten().any(|d| !d.finite())
    }) {
        return Err(unsupported(
            "native surface interval is unbounded or singular",
        ));
    }
    Ok(out)
}
pub(super) fn domains(face: &FaceGeometry, max_cells: usize) -> Result<Vec<[[f64; 2]; 2]>> {
    if face.version != crate::drawing_geometry::VERSION
        || face
            .domain
            .iter()
            .any(|d| d.iter().any(|x| !x.is_finite()) || d[0] >= d[1])
    {
        return Err(unsupported("invalid native face parameter bounds"));
    }
    let mut cuts = face.domain.map(|d| vec![d[0], d[1]]);
    match &face.carrier {
        Carrier::Rational { surface } => {
            for (i, cut) in cuts.iter_mut().enumerate() {
                cut.extend(
                    surface.knots[i]
                        .iter()
                        .copied()
                        .filter(|x| *x > face.domain[i][0] && *x < face.domain[i][1]),
                );
            }
        }
        Carrier::Revolution {
            profile: Curve3::Rational { knots, .. },
            swap_uv,
            ..
        } => {
            let axis = usize::from(*swap_uv);
            cuts[axis].extend(
                knots
                    .iter()
                    .copied()
                    .filter(|x| *x > face.domain[axis][0] && *x < face.domain[axis][1]),
            );
        }
        _ => (),
    }
    if !matches!(
        face.carrier,
        Carrier::Plane { .. } | Carrier::Rational { .. }
    ) {
        for (i, c) in cuts.iter_mut().enumerate() {
            if face.domain[i][0] < 0. && face.domain[i][1] > 0. {
                c.push(0.);
            }
        }
    }
    for c in &mut cuts {
        c.sort_by(f64::total_cmp);
        c.dedup();
    }
    if cuts[0]
        .len()
        .saturating_sub(1)
        .saturating_mul(cuts[1].len().saturating_sub(1))
        > max_cells
    {
        return Err(unsupported(
            "native face knot cells exceed the remaining scene storage budget",
        ));
    }
    Ok(cuts[0]
        .windows(2)
        .flat_map(|u| {
            cuts[1]
                .windows(2)
                .map(move |v| [[u[0], u[1]], [v[0], v[1]]])
        })
        .collect())
}

pub(super) fn surface(
    face: &FaceGeometry,
    domain: [[f64; 2]; 2],
    placement: [[f64; 4]; 4],
) -> Result<[J; 3]> {
    // Split only intersected knot spans. A local ray refinement must not
    // enumerate every tensor cell of a large trimmed spline on each call.
    let clipped: [[f64; 2]; 2] = std::array::from_fn(|i| {
        [
            domain[i][0].max(face.domain[i][0]),
            domain[i][1].min(face.domain[i][1]),
        ]
    });
    if clipped.iter().any(|d| d[0] > d[1]) {
        return Err(unsupported(
            "native curve leaves its carrier parameter domain",
        ));
    }
    let mut cuts = clipped.map(|d| vec![d[0], d[1]]);
    match &face.carrier {
        Carrier::Rational { surface } => {
            for i in 0..2 {
                let knots = &surface.knots[i];
                let a = knots.partition_point(|v| *v <= clipped[i][0]);
                let b = knots.partition_point(|v| *v < clipped[i][1]);
                if a < b {
                    cuts[i].extend_from_slice(&knots[a..b]);
                }
            }
        }
        Carrier::Revolution {
            profile: Curve3::Rational { knots, .. },
            swap_uv,
            ..
        } => {
            let i = usize::from(*swap_uv);
            let a = knots.partition_point(|v| *v <= clipped[i][0]);
            let b = knots.partition_point(|v| *v < clipped[i][1]);
            if a < b {
                cuts[i].extend_from_slice(&knots[a..b]);
            }
        }
        _ => (),
    }
    for cut in &mut cuts {
        if cut[0] != cut[1] {
            cut.sort_by(f64::total_cmp);
            cut.dedup();
        }
    }
    let mut output: Option<[J; 3]> = None;
    for u in cuts[0].windows(2) {
        for v in cuts[1].windows(2) {
            let cell = [[u[0], u[1]], [v[0], v[1]]];
            let p = surface_cell(face, cell, placement)?;
            output = Some(match output {
                None => p,
                Some(q) => std::array::from_fn(|i| J {
                    x: q[i].x.hull(p[i].x),
                    d: std::array::from_fn(|j| q[i].d[j].hull(p[i].d[j])),
                    dd: std::array::from_fn(|j| {
                        std::array::from_fn(|k| q[i].dd[j][k].hull(p[i].dd[j][k]))
                    }),
                }),
            });
        }
    }
    output.ok_or_else(|| unsupported("native curve leaves its carrier parameter domain"))
}
