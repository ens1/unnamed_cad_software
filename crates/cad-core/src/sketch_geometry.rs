//! Application-owned sketch geometry for presentation; polylines are derived display data.
use crate::*;
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SketchPolyline {
    pub member: Id,
    pub points: Vec<[f64; 2]>,
    pub closed: bool,
    pub construction: bool,
}
pub fn arc_frame(a: [f64; 2], m: [f64; 2], b: [f64; 2]) -> Result<([f64; 2], f64, f64, f64)> {
    if a.iter().chain(&m).chain(&b).any(|v| !v.is_finite()) {
        return Err(CadError::Invalid(
            "Arc source coordinates must be finite".into(),
        ));
    }
    let m_delta = [m[0] - a[0], m[1] - a[1]];
    let b_delta = [b[0] - a[0], b[1] - a[1]];
    let scale = m_delta
        .iter()
        .chain(&b_delta)
        .map(|v| v.abs())
        .fold(0., f64::max);
    if !scale.is_finite() || scale == 0. {
        return Err(CadError::Invalid(
            "Arc source separation exceeds the finite coordinate range or collapses".into(),
        ));
    }
    let m_local = m_delta.map(|v| v / scale);
    let b_local = b_delta.map(|v| v / scale);
    let d = 2. * (m_local[0] * b_local[1] - m_local[1] * b_local[0]);
    let q = |p: [f64; 2]| p[0] * p[0] + p[1] * p[1];
    if d.abs() < 1e-12 * q(m_local).max(q(b_local)).max(1e-24) {
        return Err(CadError::Invalid(
            "An arc needs three distinct, non-collinear points".into(),
        ));
    }
    let c = [
        a[0] + ((q(m_local) * b_local[1] - q(b_local) * m_local[1]) / d) * scale,
        a[1] + ((q(b_local) * m_local[0] - q(m_local) * b_local[0]) / d) * scale,
    ];
    let radius = (a[0] - c[0]).hypot(a[1] - c[1]);
    if c.iter().any(|v| !v.is_finite()) || !radius.is_finite() || radius == 0. {
        return Err(CadError::Invalid(
            "Arc carrier must have a finite center and positive finite radius".into(),
        ));
    }
    let ang = |p: [f64; 2]| (p[1] - c[1]).atan2(p[0] - c[0]);
    let start = ang(a);
    let end = (ang(b) - start).rem_euclid(std::f64::consts::TAU);
    let mid = (ang(m) - start).rem_euclid(std::f64::consts::TAU);
    Ok((
        c,
        radius,
        start,
        if mid < end {
            end
        } else {
            end - std::f64::consts::TAU
        },
    ))
}
pub fn polylines(doc: &Document, owner: &str, sketch: &Sketch) -> Result<Vec<SketchPolyline>> {
    let length = |e: &Expr| expression::value(doc, owner, e, Dimension::Length);
    let get = |id: &str| {
        sketch
            .points
            .get(id)
            .copied()
            .ok_or_else(|| CadError::Invalid(format!("Missing sketch point {id}")))
    };
    let mut out = Vec::new();
    if !sketch.profile_order.is_empty() {
        out.push(SketchPolyline {
            member: "legacy-profile".into(),
            points: sketch
                .profile_order
                .iter()
                .map(|id| get(id))
                .collect::<Result<_>>()?,
            closed: true,
            construction: false,
        });
    }
    if sketch.profile_order.is_empty() {
        for (index, segment) in sketch.segments.iter().enumerate() {
            out.push(SketchPolyline {
                member: format!("legacy-segment-{index}"),
                points: vec![get(&segment[0])?, get(&segment[1])?],
                closed: false,
                construction: false,
            });
        }
    }
    for (id, circle) in &sketch.circles {
        let c = get(&circle.center)?;
        let r = length(&circle.radius)?;
        if r <= 1e-9 {
            return Err(CadError::Invalid("Circle radius must be positive".into()));
        }
        out.push(SketchPolyline {
            member: id.clone(),
            points: (0..=96)
                .map(|i| {
                    let a = i as f64 / 96. * std::f64::consts::TAU;
                    [c[0] + r * a.cos(), c[1] + r * a.sin()]
                })
                .collect(),
            closed: true,
            construction: false,
        });
    }
    let suppressed = crate::sketch_transform::suppressed_members(sketch);
    for (id, member) in sketch
        .members
        .iter()
        .filter(|(id, _)| !suppressed.contains(*id))
    {
        let (points, closed) = match &member.curve {
            SketchCurve::Line { start, end } => (vec![get(start)?, get(end)?], false),
            SketchCurve::Arc {
                start,
                through,
                end,
            } => {
                let (c, r, a, sweep) = arc_frame(get(start)?, get(through)?, get(end)?)?;
                (
                    (0..=48)
                        .map(|i| {
                            let t = a + sweep * i as f64 / 48.;
                            [c[0] + r * t.cos(), c[1] + r * t.sin()]
                        })
                        .collect(),
                    false,
                )
            }
            SketchCurve::Bezier { points } => {
                if !(2..=16).contains(&points.len()) {
                    return Err(CadError::Invalid(
                        "A control-point spline needs 2–16 points".into(),
                    ));
                }
                let controls = points.iter().map(|p| get(p)).collect::<Result<Vec<_>>>()?;
                (
                    (0..=64)
                        .map(|i| {
                            let t = i as f64 / 64.;
                            let mut p = controls.clone();
                            for n in (1..p.len()).rev() {
                                for j in 0..n {
                                    p[j] = [
                                        p[j][0] * (1. - t) + p[j + 1][0] * t,
                                        p[j][1] * (1. - t) + p[j + 1][1] * t,
                                    ];
                                }
                            }
                            p[0]
                        })
                        .collect(),
                    false,
                )
            }
            SketchCurve::Nurbs {
                points,
                weights,
                knots,
                degree,
            } => {
                let curve = crate::sketch_edit::Curve2 {
                    control_points: points.iter().map(|p| get(p)).collect::<Result<_>>()?,
                    weights: weights.clone(),
                    knots: knots.clone(),
                    degree: *degree,
                };
                crate::sketch_edit::validate_nurbs(&curve)?;
                let count = (knots.windows(2).filter(|k| k[0] < k[1]).count() * 32).clamp(32, 4096);
                (
                    (0..=count)
                        .map(|i| crate::sketch_edit::nurbs_point(&curve, i as f64 / count as f64))
                        .collect::<Result<_>>()?,
                    points.first() == points.last(),
                )
            }
            SketchCurve::Ellipse {
                center,
                major,
                minor,
                angle,
            } => {
                let c = get(center)?;
                let a = length(major)?;
                let b = length(minor)?;
                if a <= 1e-9 || b <= 1e-9 {
                    return Err(CadError::Invalid("Ellipse radii must be positive".into()));
                }
                let rot = expression::value(doc, owner, angle, Dimension::Angle)?;
                (
                    (0..=96)
                        .map(|i| {
                            let t = i as f64 / 96. * std::f64::consts::TAU;
                            let x = a * t.cos();
                            let y = b * t.sin();
                            [
                                c[0] + x * rot.cos() - y * rot.sin(),
                                c[1] + x * rot.sin() + y * rot.cos(),
                            ]
                        })
                        .collect(),
                    true,
                )
            }
            SketchCurve::Circle { center, radius } => {
                let c = get(center)?;
                let r = length(radius)?;
                if r <= 1e-9 {
                    return Err(CadError::Invalid("Circle radius must be positive".into()));
                }
                (
                    (0..=96)
                        .map(|i| {
                            let a = i as f64 / 96. * std::f64::consts::TAU;
                            [c[0] + r * a.cos(), c[1] + r * a.sin()]
                        })
                        .collect(),
                    true,
                )
            }
        };
        if points.iter().flatten().any(|v| !v.is_finite()) {
            return Err(CadError::Invalid("Sketch geometry must be finite".into()));
        }
        out.push(SketchPolyline {
            member: id.clone(),
            points,
            closed,
            construction: member.construction,
        });
    }
    Ok(out)
}
