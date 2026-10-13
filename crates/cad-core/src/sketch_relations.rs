//! Geometry-aware bounds and reflection semantics for native sketch relations.
use crate::*;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::f64::consts::{PI, TAU};

/// Version 1 exact contact intent. Parameters use each curve's normalized domain;
/// free coordinates belong to the sketch solve, never to a display polyline.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Contact {
    pub parameters: [f64; 2],
    pub free: [bool; 2],
}

fn invalid(message: &str) -> CadError {
    CadError::Invalid(message.into())
}
fn point(sketch: &Sketch, key: &str) -> Result<[f64; 2]> {
    sketch_actions::entity_point(sketch, key)
}
pub fn reflect(p: [f64; 2], a: [f64; 2], b: [f64; 2]) -> Result<[f64; 2]> {
    let d = [b[0] - a[0], b[1] - a[1]];
    let square = d[0] * d[0] + d[1] * d[1];
    if square < 1e-18 {
        return Err(invalid("Symmetry requires a nonzero straight axis"));
    }
    let t = ((p[0] - a[0]) * d[0] + (p[1] - a[1]) * d[1]) / square;
    Ok([2. * (a[0] + t * d[0]) - p[0], 2. * (a[1] + t * d[1]) - p[1]])
}
fn normalized_knots(knots: &[f64], reverse: bool) -> Vec<f64> {
    let a = knots[0];
    let width = knots.last().unwrap() - a;
    if reverse {
        knots.iter().rev().map(|v| 1. - (v - a) / width).collect()
    } else {
        knots.iter().map(|v| (v - a) / width).collect()
    }
}
pub fn compatible(sketch: &Sketch, first: &str, second: &str, reverse: bool) -> Result<()> {
    let a = &sketch
        .members
        .get(first)
        .ok_or_else(|| invalid("Missing symmetric member"))?
        .curve;
    let b = &sketch
        .members
        .get(second)
        .ok_or_else(|| invalid("Missing symmetric member"))?
        .curve;
    let valid = match (a, b) {
        (SketchCurve::Line { .. }, SketchCurve::Line { .. })
        | (SketchCurve::Arc { .. }, SketchCurve::Arc { .. })
        | (SketchCurve::Circle { .. }, SketchCurve::Circle { .. })
        | (SketchCurve::Ellipse { .. }, SketchCurve::Ellipse { .. }) => true,
        (SketchCurve::Bezier { points: a }, SketchCurve::Bezier { points: b }) => {
            a.len() == b.len()
        }
        (
            SketchCurve::Nurbs {
                points: a,
                weights: wa,
                knots: ka,
                degree: pa,
            },
            SketchCurve::Nurbs {
                points: b,
                weights: wb,
                knots: kb,
                degree: pb,
            },
        ) => {
            if a.len() != b.len()
                || pa != pb
                || wa.len() != wb.len()
                || ka.len() != kb.len()
                || wa.is_empty()
                || wb.is_empty()
                || ka.is_empty()
                || ka.last().unwrap() <= &ka[0]
                || kb.last().unwrap() <= &kb[0]
            {
                false
            } else {
                let weights = if reverse {
                    wb.iter().rev().copied().collect::<Vec<_>>()
                } else {
                    wb.clone()
                };
                let knots = normalized_knots(kb, reverse);
                wa.iter().zip(&weights).all(|(a, b)| {
                    (a / wa[0] - b / weights[0]).abs() <= 1e-12 * (a / wa[0]).abs().max(1.)
                }) && normalized_knots(ka, false)
                    .iter()
                    .zip(knots)
                    .all(|(a, b)| (a - b).abs() <= 1e-12)
            }
        }
        _ => false,
    };
    if valid {
        Ok(())
    } else {
        Err(invalid(
            "Symmetric members need matching curve types and control topology; rational weights and knot structure must also match",
        ))
    }
}
pub fn best_orientation(sketch: &Sketch, first: &str, second: &str, axis: &str) -> Result<bool> {
    let (a, b) = sketch_actions::entity_line(sketch, axis)?;
    let first_points = sketch
        .members
        .get(first)
        .ok_or_else(|| invalid("Missing symmetric member"))?
        .curve
        .points();
    let second_points = sketch
        .members
        .get(second)
        .ok_or_else(|| invalid("Missing symmetric member"))?
        .curve
        .points();
    let mut best: Option<(f64, bool)> = None;
    for reverse in [false, true] {
        if compatible(sketch, first, second, reverse).is_err() {
            continue;
        }
        let ordered = if reverse {
            second_points.iter().rev().copied().collect::<Vec<_>>()
        } else {
            second_points.clone()
        };
        let mut cost = 0.;
        for (first, second) in first_points.iter().zip(ordered) {
            let expected = reflect(point(sketch, first)?, a, b)?;
            let actual = point(sketch, second)?;
            cost += (expected[0] - actual[0]).powi(2) + (expected[1] - actual[1]).powi(2);
        }
        if best.is_none_or(|(value, _)| cost < value) {
            best = Some((cost, reverse));
        }
    }
    best.map(|(_, reverse)| reverse).ok_or_else(|| {
        invalid("Select compatible member pairs followed by a straight symmetry axis")
    })
}
pub fn member_residuals(
    doc: &Document,
    owner: &str,
    sketch: &Sketch,
    keys: &[Id],
    reverse: bool,
    get: &impl Fn(&str) -> Result<[f64; 2]>,
) -> Result<Vec<f64>> {
    if keys.len() != 3 {
        return Err(invalid("Member symmetry needs two members and an axis"));
    }
    compatible(sketch, &keys[0], &keys[1], reverse)?;
    let (axis_a, axis_b) = axis(sketch, &keys[2], get)?;
    let a = &sketch.members[&keys[0]].curve;
    let b = &sketch.members[&keys[1]].curve;
    let first = a.points();
    let second = b.points();
    let second = if reverse {
        second.into_iter().rev().collect::<Vec<_>>()
    } else {
        second
    };
    let mut out = vec![];
    for (p, q) in first.into_iter().zip(second) {
        let expected = reflect(get(p)?, axis_a, axis_b)?;
        let actual = get(q)?;
        out.extend([expected[0] - actual[0], expected[1] - actual[1]]);
    }
    match (a, b) {
        (SketchCurve::Circle { radius: a, .. }, SketchCurve::Circle { radius: b, .. }) => {
            out.push(
                expression::value(doc, owner, a, Dimension::Length)?
                    - expression::value(doc, owner, b, Dimension::Length)?,
            );
        }
        (
            SketchCurve::Ellipse {
                major: ma,
                minor: na,
                angle: aa,
                ..
            },
            SketchCurve::Ellipse {
                major: mb,
                minor: nb,
                angle: ab,
                ..
            },
        ) => {
            out.push(
                expression::value(doc, owner, ma, Dimension::Length)?
                    - expression::value(doc, owner, mb, Dimension::Length)?,
            );
            out.push(
                expression::value(doc, owner, na, Dimension::Length)?
                    - expression::value(doc, owner, nb, Dimension::Length)?,
            );
            let angle_a = expression::value(doc, owner, aa, Dimension::Angle)?;
            let angle_b = expression::value(doc, owner, ab, Dimension::Angle)?;
            let axis_angle = (axis_b[1] - axis_a[1]).atan2(axis_b[0] - axis_a[0]);
            out.push(
                100. * ((angle_b - 2. * axis_angle + angle_a + PI / 2.).rem_euclid(PI) - PI / 2.),
            );
        }
        _ => {}
    }
    Ok(out)
}
pub fn axis(
    sketch: &Sketch,
    key: &str,
    get: &impl Fn(&str) -> Result<[f64; 2]>,
) -> Result<([f64; 2], [f64; 2])> {
    match key {
        sketch_actions::X_AXIS => Ok(([0., 0.], [1., 0.])),
        sketch_actions::Y_AXIS => Ok(([0., 0.], [0., 1.])),
        _ => match sketch.members.get(key).map(|m| &m.curve) {
            Some(SketchCurve::Line { start, end }) => Ok((get(start)?, get(end)?)),
            _ => Err(invalid("Select a straight sketch member or a sketch axis")),
        },
    }
}

/// Positive only outside the angular interval; it remains a fixed-size residual
/// as the point enters the arc and leaves tangential freedom within the interval.
pub fn arc_excess(
    sketch: &Sketch,
    key: &str,
    contact: [f64; 2],
    get: &impl Fn(&str) -> Result<[f64; 2]>,
) -> Result<Option<f64>> {
    let Some(SketchCurve::Arc {
        start,
        through,
        end,
    }) = sketch.members.get(key).map(|m| &m.curve)
    else {
        return Ok(None);
    };
    let (center, radius, start, sweep) =
        sketch_geometry::arc_frame(get(start)?, get(through)?, get(end)?)?;
    let angle = (contact[1] - center[1]).atan2(contact[0] - center[0]);
    let progress = if sweep > 0. {
        (angle - start).rem_euclid(TAU)
    } else {
        (start - angle).rem_euclid(TAU)
    };
    let angular = if progress <= sweep.abs() + 1e-12 {
        0.
    } else {
        (TAU - progress).min(progress - sweep.abs())
    };
    Ok(Some(radius * angular))
}
pub fn line_excess(
    sketch: &Sketch,
    key: &str,
    contact: [f64; 2],
    get: &impl Fn(&str) -> Result<[f64; 2]>,
) -> Result<Option<f64>> {
    if !matches!(
        sketch.members.get(key).map(|m| &m.curve),
        Some(SketchCurve::Line { .. })
    ) {
        return Ok(None);
    }
    let (a, b) = axis(sketch, key, get)?;
    let d = [b[0] - a[0], b[1] - a[1]];
    let length = d[0].hypot(d[1]);
    if length < 1e-9 {
        return Err(invalid("Tangent line has zero length"));
    }
    let along = ((contact[0] - a[0]) * d[0] + (contact[1] - a[1]) * d[1]) / length;
    Ok(Some(if along < 0. {
        -along
    } else {
        (along - length).max(0.)
    }))
}

/// Ellipse sizes and reflected orientation are derived from the equality group,
/// preserving source expressions and honoring explicit dimensions on any peer.
pub fn derive_ellipses(doc: &Document, owner: &str, sketch: &mut Sketch) -> Result<()> {
    if !sketch.constraints.iter().any(|c| {
        c.kind == "symmetric_member"
            && c.entities.len() == 3
            && c.entities[..2].iter().all(|key| {
                matches!(
                    sketch.members.get(key).map(|m| &m.curve),
                    Some(SketchCurve::Ellipse { .. })
                )
            })
    }) {
        return Ok(());
    }
    let mut source = sketch.clone();
    let inactive = crate::sketch_transform::inactive_entities(sketch);
    source
        .constraints
        .retain(|constraint| !constraint.entities.iter().any(|key| inactive.contains(key)));
    // Reflection may use an offset/pattern/corner-trimmed axis. Resolve that
    // geometry from this candidate's current free points before deriving angles.
    sketch_edit::derive(doc, owner, &mut source)?;
    let ellipse = |key: &Id| {
        matches!(
            source.members.get(key).map(|m| &m.curve),
            Some(SketchCurve::Ellipse { .. })
        )
    };
    let relations = source
        .constraints
        .iter()
        .filter(|c| {
            c.kind == "symmetric_member"
                && c.entities.len() == 3
                && ellipse(&c.entities[0])
                && ellipse(&c.entities[1])
        })
        .collect::<Vec<_>>();
    let mut visited = BTreeSet::new();
    for key in source.members.keys().filter(|key| ellipse(key)) {
        if visited.contains(key) {
            continue;
        }
        let mut group = BTreeSet::from([key.clone()]);
        let mut pending = vec![key.clone()];
        while let Some(key) = pending.pop() {
            for c in &relations {
                let peer = if c.entities[0] == key {
                    &c.entities[1]
                } else if c.entities[1] == key {
                    &c.entities[0]
                } else {
                    continue;
                };
                if group.insert(peer.clone()) {
                    pending.push(peer.clone());
                }
            }
        }
        visited.extend(group.iter().cloned());
        if group.len() == 1 {
            continue;
        }
        let driver = source
            .constraints
            .iter()
            .find(|c| {
                !c.driven
                    && c.kind == "rotation"
                    && c.entities.first().is_some_and(|id| group.contains(id))
            })
            .map(|c| c.entities[0].clone())
            .unwrap_or_else(|| {
                group
                    .iter()
                    .find(|key| !relations.iter().any(|c| c.entities[1] == **key))
                    .unwrap_or(group.first().unwrap())
                    .clone()
            });
        let SketchCurve::Ellipse {
            major,
            minor,
            angle,
            ..
        } = &source.members[&driver].curve
        else {
            unreachable!()
        };
        let size = |kind: &str, fallback: &Expr| {
            source
                .constraints
                .iter()
                .find(|c| {
                    !c.driven
                        && c.kind == kind
                        && c.entities.first().is_some_and(|id| group.contains(id))
                })
                .map(|c| c.value.clone())
                .unwrap_or_else(|| fallback.clone())
        };
        let major = size("major_radius", major);
        let minor = size("minor_radius", minor);
        let angle = size("rotation", angle);
        let mut angles = BTreeMap::from([(
            driver.clone(),
            expression::value(doc, owner, &angle, Dimension::Angle)?,
        )]);
        let mut pending = vec![driver];
        while let Some(key) = pending.pop() {
            for c in &relations {
                let peer = if c.entities[0] == key {
                    &c.entities[1]
                } else if c.entities[1] == key {
                    &c.entities[0]
                } else {
                    continue;
                };
                if angles.contains_key(peer) {
                    continue;
                }
                let (a, b) = sketch_actions::entity_line(&source, &c.entities[2])?;
                if (b[0] - a[0]).hypot(b[1] - a[1]) < 1e-9 {
                    return Err(invalid("Ellipse symmetry requires a nonzero axis"));
                }
                let reflected = 2. * (b[1] - a[1]).atan2(b[0] - a[0]) - angles[&key];
                angles.insert(peer.clone(), reflected);
                pending.push(peer.clone());
            }
        }
        for (key, value) in angles {
            if sketch_edit::driven_member(&source, &key) {
                continue;
            }
            if let SketchCurve::Ellipse {
                major: m,
                minor: n,
                angle: a,
                ..
            } = &mut sketch.members.get_mut(&key).unwrap().curve
            {
                *m = major.clone();
                *n = minor.clone();
                *a = Expr(format!("{value} rad"));
            }
        }
    }
    Ok(())
}

#[derive(Clone, Copy)]
pub struct CurveJet {
    pub point: [f64; 2],
    pub first: [f64; 2],
    pub second: [f64; 2],
}
fn deboor(
    controls: &[nalgebra::Vector3<f64>],
    knots: &[f64],
    degree: usize,
    u: f64,
) -> Result<nalgebra::Vector3<f64>> {
    if controls.is_empty() || knots.len() != controls.len() + degree + 1 {
        return Err(invalid("Invalid derivative spline topology"));
    }
    let n = controls.len() - 1;
    let span = if u >= knots[n + 1] {
        n
    } else {
        (degree..=n)
            .find(|i| u >= knots[*i] && u < knots[*i + 1])
            .ok_or_else(|| invalid("Contact parameter lies outside the spline domain"))?
    };
    let mut work = (0..=degree)
        .map(|j| controls[span - degree + j])
        .collect::<Vec<_>>();
    for r in 1..=degree {
        for j in (r..=degree).rev() {
            let i = span - degree + j;
            let denominator = knots[i + degree + 1 - r] - knots[i];
            let alpha = if denominator > 0. {
                (u - knots[i]) / denominator
            } else {
                0.
            };
            work[j] = work[j - 1] * (1. - alpha) + work[j] * alpha;
        }
    }
    Ok(work[degree])
}
fn derivative_controls(
    controls: &[nalgebra::Vector3<f64>],
    knots: &[f64],
    degree: usize,
) -> Vec<nalgebra::Vector3<f64>> {
    controls
        .windows(2)
        .enumerate()
        .map(|(i, p)| {
            let width = knots[i + degree + 1] - knots[i + 1];
            if width > 0. {
                (p[1] - p[0]) * (degree as f64 / width)
            } else {
                nalgebra::Vector3::zeros()
            }
        })
        .collect()
}
fn rational_jet(curve: &sketch_edit::Curve2, t: f64) -> Result<CurveJet> {
    sketch_edit::validate_nurbs(curve)?;
    let degree = curve.degree;
    let count = curve.control_points.len();
    let start = curve.knots[degree];
    let width = curve.knots[count] - start;
    let u = start + t * width;
    let controls = curve
        .control_points
        .iter()
        .zip(&curve.weights)
        .map(|(p, w)| nalgebra::Vector3::new(p[0] * w, p[1] * w, *w))
        .collect::<Vec<_>>();
    let h = deboor(&controls, &curve.knots, degree, u)?;
    let d = derivative_controls(&controls, &curve.knots, degree);
    let first = deboor(&d, &curve.knots[1..curve.knots.len() - 1], degree - 1, u)? * width;
    let second = if degree > 1 {
        let dd = derivative_controls(&d, &curve.knots[1..curve.knots.len() - 1], degree - 1);
        deboor(&dd, &curve.knots[2..curve.knots.len() - 2], degree - 2, u)? * (width * width)
    } else {
        nalgebra::Vector3::zeros()
    };
    if h.z <= 0. || !h.z.is_finite() {
        return Err(invalid("Contact curve has an invalid rational denominator"));
    }
    let p = [h.x / h.z, h.y / h.z];
    let d = [
        (first.x - first.z * p[0]) / h.z,
        (first.y - first.z * p[1]) / h.z,
    ];
    let dd = [
        (second.x - second.z * p[0] - 2. * first.z * d[0]) / h.z,
        (second.y - second.z * p[1] - 2. * first.z * d[1]) / h.z,
    ];
    Ok(CurveJet {
        point: p,
        first: d,
        second: dd,
    })
}
pub fn closed_contact(sketch: &Sketch, key: &str) -> bool {
    matches!(
        sketch.members.get(key).map(|m| &m.curve),
        Some(SketchCurve::Circle { .. } | SketchCurve::Ellipse { .. })
    )
}
/// Analytic derivatives of authoritative application-owned source curves.
pub fn curve_jet(
    doc: &Document,
    owner: &str,
    sketch: &Sketch,
    key: &str,
    parameter: f64,
    get: &impl Fn(&str) -> Result<[f64; 2]>,
) -> Result<CurveJet> {
    if !parameter.is_finite() {
        return Err(invalid("Contact parameter must be finite"));
    }
    let curve = &sketch
        .members
        .get(key)
        .ok_or_else(|| invalid("Contact member no longer exists"))?
        .curve;
    let t = if closed_contact(sketch, key) {
        parameter.rem_euclid(1.)
    } else {
        parameter.clamp(0., 1.)
    };
    let circular = |center: [f64; 2], radius: f64, start: f64, sweep: f64| {
        let a = start + sweep * t;
        let radial = [a.cos(), a.sin()];
        CurveJet {
            point: [
                center[0] + radius * radial[0],
                center[1] + radius * radial[1],
            ],
            first: [-radius * sweep * radial[1], radius * sweep * radial[0]],
            second: [
                -radius * sweep * sweep * radial[0],
                -radius * sweep * sweep * radial[1],
            ],
        }
    };
    let jet = match curve {
        SketchCurve::Line { start, end } => {
            let (a, b) = (get(start)?, get(end)?);
            let d = [b[0] - a[0], b[1] - a[1]];
            CurveJet {
                point: [a[0] + d[0] * t, a[1] + d[1] * t],
                first: d,
                second: [0.; 2],
            }
        }
        SketchCurve::Arc {
            start,
            through,
            end,
        } => {
            let (c, r, a, s) = sketch_geometry::arc_frame(get(start)?, get(through)?, get(end)?)?;
            circular(c, r, a, s)
        }
        SketchCurve::Circle { center, radius } => {
            let radius = expression::value(doc, owner, radius, Dimension::Length)?;
            if radius <= 1e-9 {
                return Err(invalid("Circular contacts need a positive radius"));
            }
            circular(get(center)?, radius, 0., TAU)
        }
        SketchCurve::Ellipse {
            center,
            major,
            minor,
            angle,
        } => {
            let c = get(center)?;
            let a = expression::value(doc, owner, major, Dimension::Length)?;
            let b = expression::value(doc, owner, minor, Dimension::Length)?;
            if a <= 1e-9 || b <= 1e-9 {
                return Err(invalid("Ellipse contacts need positive radii"));
            }
            let angle = expression::value(doc, owner, angle, Dimension::Angle)?;
            let q = t * TAU;
            let rotate = |p: [f64; 2]| {
                [
                    p[0] * angle.cos() - p[1] * angle.sin(),
                    p[0] * angle.sin() + p[1] * angle.cos(),
                ]
            };
            let p = rotate([a * q.cos(), b * q.sin()]);
            let d = rotate([-a * q.sin() * TAU, b * q.cos() * TAU]);
            let dd = rotate([-a * q.cos() * TAU * TAU, -b * q.sin() * TAU * TAU]);
            CurveJet {
                point: [c[0] + p[0], c[1] + p[1]],
                first: d,
                second: dd,
            }
        }
        SketchCurve::Bezier { points } => {
            let degree = points
                .len()
                .checked_sub(1)
                .ok_or_else(|| invalid("Empty contact spline"))?;
            let curve = sketch_edit::Curve2 {
                control_points: points.iter().map(|p| get(p)).collect::<Result<_>>()?,
                weights: vec![1.; points.len()],
                knots: std::iter::repeat_n(0., degree + 1)
                    .chain(std::iter::repeat_n(1., degree + 1))
                    .collect(),
                degree,
            };
            rational_jet(&curve, t)?
        }
        SketchCurve::Nurbs {
            points,
            weights,
            knots,
            degree,
        } => rational_jet(
            &sketch_edit::Curve2 {
                control_points: points.iter().map(|p| get(p)).collect::<Result<_>>()?,
                weights: weights.clone(),
                knots: knots.clone(),
                degree: *degree,
            },
            t,
        )?,
    };
    let speed = jet.first[0].hypot(jet.first[1]);
    if jet
        .point
        .iter()
        .chain(&jet.first)
        .chain(&jet.second)
        .any(|v| !v.is_finite())
        || !speed.is_finite()
    {
        return Err(invalid("Contact curve derivatives must be finite"));
    }
    Ok(jet)
}
pub fn contact_residuals(
    doc: &Document,
    owner: &str,
    sketch: &Sketch,
    c: &SketchConstraint,
    get: &impl Fn(&str) -> Result<[f64; 2]>,
) -> Result<Vec<f64>> {
    let contact = c
        .contact
        .as_ref()
        .ok_or_else(|| invalid("Missing saved curve contact intent"))?;
    if c.entities.len() != 2 || contact.parameters.iter().any(|v| !v.is_finite()) {
        return Err(invalid(
            "Curve contact needs two members and finite parameters",
        ));
    }
    if c.kind == "point_on_curve" {
        let p = get(&c.entities[0])?;
        let q = curve_jet(
            doc,
            owner,
            sketch,
            &c.entities[1],
            contact.parameters[1],
            get,
        )?;
        let t = contact.parameters[1];
        return Ok(vec![
            p[0] - q.point[0],
            p[1] - q.point[1],
            if closed_contact(sketch, &c.entities[1]) {
                0.
            } else {
                100. * ((-t).max(0.) + (t - 1.).max(0.))
            },
        ]);
    }
    let first = curve_jet(
        doc,
        owner,
        sketch,
        &c.entities[0],
        contact.parameters[0],
        get,
    )?;
    let second = curve_jet(
        doc,
        owner,
        sketch,
        &c.entities[1],
        contact.parameters[1],
        get,
    )?;
    let sign = expression::value(doc, owner, &c.value, Dimension::Scalar)?;
    if sign != 1. && sign != -1. {
        return Err(invalid("Curve contact tangent orientation must be 1 or -1"));
    }
    let speed_a = first.first[0].hypot(first.first[1]);
    let speed_b = second.first[0].hypot(second.first[1]);
    if speed_a <= 1e-9 || speed_b <= 1e-9 {
        return Err(invalid(
            "Tangency and curvature contact require regular nonzero curve tangents",
        ));
    }
    let a = first.first.map(|v| v / speed_a);
    let b = second.first.map(|v| v / speed_b * sign);
    let angle = (a[0] * b[1] - a[1] * b[0]).atan2(a[0] * b[0] + a[1] * b[1]);
    let mut residuals = vec![
        first.point[0] - second.point[0],
        first.point[1] - second.point[1],
        angle * 100.,
    ];
    for i in 0..2 {
        let t = contact.parameters[i];
        residuals.push(if closed_contact(sketch, &c.entities[i]) {
            0.
        } else {
            100. * ((-t).max(0.) + (t - 1.).max(0.))
        });
    }
    if c.kind == "curvature" {
        for i in 0..2 {
            if let SketchCurve::Nurbs {
                knots,
                degree,
                points,
                ..
            } = &sketch.members[&c.entities[i]].curve
            {
                let u =
                    knots[*degree] + contact.parameters[i] * (knots[points.len()] - knots[*degree]);
                if u > knots[*degree] + 1e-12
                    && u < knots[points.len()] - 1e-12
                    && knots.iter().filter(|k| (**k - u).abs() <= 1e-12).count()
                        > degree.saturating_sub(2)
                {
                    return Err(invalid(
                        "Curvature contact needs a C2 regular spline location; select an endpoint or a smooth knot span",
                    ));
                }
            }
        }
        let curvature = |j: CurveJet| {
            let speed = j.first[0].hypot(j.first[1]);
            ((j.first[0] / speed) * (j.second[1] / speed)
                - (j.first[1] / speed) * (j.second[0] / speed))
                / speed
        };
        residuals.push((curvature(first) - sign * curvature(second)) * 100.);
    }
    Ok(residuals)
}

pub fn initial_contact(sketch: &Sketch, keys: &[Id], curvature: bool) -> Result<Contact> {
    if keys.len() != 2 {
        return Err(invalid("Contact requires two curve members"));
    }
    let ends = |key: &str| -> Result<Option<[Id; 2]>> {
        // Planning can receive raw source or an already derived sketch. The
        // modifier-owned endpoint identities must be the same in either case;
        // seed_contact and solver residuals evaluate the derived member curve.
        let curve = sketch_edit::effective_curve(sketch, key)?;
        if matches!(
            curve,
            SketchCurve::Circle { .. } | SketchCurve::Ellipse { .. }
        ) {
            return Ok(None);
        }
        let points = curve.points();
        if points.len() < 2 {
            return Err(invalid("Contact spline needs at least two controls"));
        }
        Ok(Some([points[0].into(), points.last().unwrap().to_string()]))
    };
    let a = ends(&keys[0])?;
    let b = ends(&keys[1])?;
    let mut contact = Contact {
        parameters: [0., 0.],
        // Whole-curve tangency does not imply an endpoint pick. In particular,
        // an open curve paired with a closed curve must be free to meet it
        // within its bounded domain. Curvature keeps its endpoint intent.
        free: if curvature {
            [a.is_none(), b.is_none()]
        } else {
            [true; 2]
        },
    };
    if let (Some(a), Some(b)) = (&a, &b) {
        let inactive = crate::sketch_transform::inactive_entities(sketch);
        let point_key = |key: &str| -> Option<Id> {
            if sketch.points.contains_key(key) || key == sketch_actions::ORIGIN {
                Some(key.into())
            } else {
                match sketch.members.get(key).map(|member| &member.curve) {
                    Some(
                        SketchCurve::Circle { center, .. } | SketchCurve::Ellipse { center, .. },
                    ) => Some(center.clone()),
                    _ => None,
                }
            }
        };
        // Coincidence is source intent even while an edited draft is not yet
        // solved. Avoid the geometry-validating topology helper here: a near
        // endpoint is only a starting estimate, never an authored connection.
        let mut joins = BTreeMap::<Id, Vec<Id>>::new();
        for relation in sketch.constraints.iter().filter(|relation| {
            !relation.driven
                && relation.entities.len() == 2
                && !relation.entities.iter().any(|key| inactive.contains(key))
        }) {
            let endpoint = |key: &str, parameter: f64, free: bool| -> Option<Id> {
                if free || (parameter != 0. && parameter != 1.) {
                    return None;
                }
                ends(key)
                    .ok()
                    .flatten()
                    .map(|ends| ends[parameter as usize].clone())
            };
            let pair = if relation.kind == "coincident" {
                Some((
                    point_key(&relation.entities[0]),
                    point_key(&relation.entities[1]),
                ))
            } else if let Some(contact) = &relation.contact {
                match relation.kind.as_str() {
                    "point_on_curve" => Some((
                        point_key(&relation.entities[0]),
                        endpoint(
                            &relation.entities[1],
                            contact.parameters[1],
                            contact.free[1],
                        ),
                    )),
                    "tangent" | "curvature" => Some((
                        endpoint(
                            &relation.entities[0],
                            contact.parameters[0],
                            contact.free[0],
                        ),
                        endpoint(
                            &relation.entities[1],
                            contact.parameters[1],
                            contact.free[1],
                        ),
                    )),
                    _ => None,
                }
            } else {
                None
            };
            if let Some((Some(first), Some(second))) = pair {
                joins.entry(first.clone()).or_default().push(second.clone());
                joins.entry(second).or_default().push(first);
            }
        }
        let joined = |first: &str, second: &str| {
            let mut seen = BTreeSet::new();
            let mut pending = vec![first.to_string()];
            while let Some(key) = pending.pop() {
                if key == second {
                    return true;
                }
                if seen.insert(key.clone()) {
                    pending.extend(joins.get(&key).into_iter().flatten().cloned());
                }
            }
            false
        };
        let mut best = None;
        let mut authored = Vec::new();
        for (i, a) in a.iter().enumerate() {
            for (j, b) in b.iter().enumerate() {
                if joined(a, b) {
                    authored.push((i, j));
                }
                let p = point(sketch, a)?;
                let q = point(sketch, b)?;
                let distance = (p[0] - q[0]).hypot(p[1] - q[1]);
                if best.is_none_or(|(d, _, _)| distance < d) {
                    best = Some((distance, i, j));
                }
            }
        }
        let (_, i, j) = best.unwrap();
        contact.parameters = [i as f64, j as f64];
        if curvature {
            contact.free = [false; 2];
        } else if let [(i, j)] = authored.as_slice() {
            contact.parameters = [*i as f64, *j as f64];
            contact.free = [false; 2];
        } else if !authored.is_empty() {
            return Err(invalid(
                "Tangent contact is ambiguous because the curves share several joined endpoints; select their contact endpoints directly",
            ));
        }
    }
    Ok(contact)
}
/// Exact curve evaluation supplies a starting contact; final coincidence, tangent,
/// curvature and domain equations are validated by the constraint solver.
pub fn seed_contact(
    doc: &Document,
    owner: &str,
    sketch: &Sketch,
    c: &mut SketchConstraint,
) -> Result<()> {
    let Some(mut contact) = c.contact.clone() else {
        return Ok(());
    };
    let get = |key: &str| point(sketch, key);
    if c.kind == "point_on_curve" {
        let p = get(&c.entities[0])?;
        let mut best = (f64::INFINITY, 0.);
        for i in 0..=64 {
            let t = i as f64 / 64.;
            let q = curve_jet(doc, owner, sketch, &c.entities[1], t, &get)?.point;
            let d = (p[0] - q[0]).hypot(p[1] - q[1]);
            if d < best.0 {
                best = (d, t);
            }
        }
        if !best.0.is_finite() {
            return Err(invalid(
                "Point-on-curve contact exceeds the finite coordinate range",
            ));
        }
        contact.parameters[1] = best.1;
        c.contact = Some(contact);
        return Ok(());
    }
    let parameters = |i: usize| {
        if contact.free[i] {
            (0..=32).map(|j| j as f64 / 32.).collect::<Vec<_>>()
        } else {
            vec![contact.parameters[i]]
        }
    };
    let mut best = None;
    for a in parameters(0) {
        for b in parameters(1) {
            let Ok(first) = curve_jet(doc, owner, sketch, &c.entities[0], a, &get) else {
                continue;
            };
            let Ok(second) = curve_jet(doc, owner, sketch, &c.entities[1], b, &get) else {
                continue;
            };
            let distance =
                (first.point[0] - second.point[0]).hypot(first.point[1] - second.point[1]);
            let speed_a = first.first[0].hypot(first.first[1]);
            let speed_b = second.first[0].hypot(second.first[1]);
            if speed_a <= 1e-9 || speed_b <= 1e-9 {
                continue;
            }
            let dot = (first.first[0] / speed_a) * (second.first[0] / speed_b)
                + (first.first[1] / speed_a) * (second.first[1] / speed_b);
            if best.is_none_or(|(d, _, _, _)| distance < d) {
                best = Some((distance, a, b, dot));
            }
        }
    }
    let (_, a, b, dot) = best.ok_or_else(|| invalid("No regular starting curve contact"))?;
    contact.parameters = [a, b];
    c.value = if dot < 0. { (-1.).into() } else { 1.0.into() };
    c.contact = Some(contact);
    Ok(())
}

/// Canonical point identities for explicitly authored endpoint coincidence.
/// Geometry proximity alone never establishes a topological connection.
pub fn topological_point_aliases(sketch: &Sketch, tolerance: f64) -> Result<BTreeMap<Id, Id>> {
    if !tolerance.is_finite() || tolerance <= 0. {
        return Err(invalid(
            "Endpoint topology needs a positive finite millimetre tolerance",
        ));
    }
    if sketch.points.values().flatten().any(|v| !v.is_finite()) {
        return Err(invalid(
            "Endpoint topology requires finite authoritative point coordinates",
        ));
    }
    let mut aliases = sketch
        .points
        .keys()
        .map(|key| (key.clone(), key.clone()))
        .collect::<BTreeMap<_, _>>();
    let mut union = |a: &str, b: &str| -> Result<()> {
        if !sketch.points.contains_key(a) || !sketch.points.contains_key(b) {
            return Ok(());
        }
        let first = aliases[a].clone();
        let second = aliases[b].clone();
        if first == second {
            return Ok(());
        }
        let (p, q) = (sketch.points[&first], sketch.points[&second]);
        if (p[0] - q[0]).hypot(p[1] - q[1]) > tolerance {
            return Err(invalid(
                "Explicitly joined sketch endpoints do not satisfy the topology tolerance",
            ));
        }
        let canonical = first.min(second.clone());
        let other = if canonical == second {
            aliases[a].clone()
        } else {
            second
        };
        for value in aliases.values_mut() {
            if *value == other {
                *value = canonical.clone();
            }
        }
        Ok(())
    };
    let endpoint = |key: &str, t: f64| -> Option<Id> {
        let curve = &sketch.members.get(key)?.curve;
        if matches!(
            curve,
            SketchCurve::Circle { .. } | SketchCurve::Ellipse { .. }
        ) {
            return None;
        }
        let controls = curve.points();
        if t == 0. {
            controls.first().map(|p| p.to_string())
        } else if t == 1. {
            controls.last().map(|p| p.to_string())
        } else {
            None
        }
    };
    let inactive = crate::sketch_transform::inactive_entities(sketch);
    for c in sketch
        .constraints
        .iter()
        .filter(|c| !c.driven && !c.entities.iter().any(|key| inactive.contains(key)))
    {
        if c.kind == "coincident" && c.entities.len() == 2 {
            union(&c.entities[0], &c.entities[1])?;
        }
        if let Some(contact) = &c.contact
            && c.entities.len() == 2
        {
            if c.kind == "point_on_curve" {
                if let Some(end) = endpoint(&c.entities[1], contact.parameters[1]) {
                    union(&c.entities[0], &end)?;
                }
            } else if matches!(c.kind.as_str(), "tangent" | "curvature")
                && let (Some(a), Some(b)) = (
                    endpoint(&c.entities[0], contact.parameters[0]),
                    endpoint(&c.entities[1], contact.parameters[1]),
                )
            {
                union(&a, &b)?;
            }
        }
    }
    for (key, canonical) in &aliases {
        let (p, q) = (sketch.points[key], sketch.points[canonical]);
        if (p[0] - q[0]).hypot(p[1] - q[1]) > tolerance {
            return Err(invalid(
                "Transitive endpoint coincidences exceed the topology tolerance",
            ));
        }
    }
    Ok(aliases)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RepairRequest {
    pub edit_id: Id,
    pub members: Vec<Id>,
    pub tolerance: f64,
    pub include_construction: bool,
    pub remove_duplicate_relations: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RepairReport {
    pub joined_endpoints: Vec<[Id; 2]>,
    pub removed_relations: Vec<Id>,
    pub ambiguous_endpoints: Vec<Id>,
    pub remaining_open_endpoints: usize,
}
/// Add explicit, persistent endpoint coincidence only for isolated unambiguous
/// near pairs. Source curves and all point/member identities remain intact.
pub fn repair(sketch: &mut Sketch, request: &RepairRequest) -> Result<RepairReport> {
    let mut candidate = sketch.clone();
    let report = repair_geometry(&mut candidate, request)?;
    *sketch = candidate;
    Ok(report)
}
fn repair_geometry(sketch: &mut Sketch, request: &RepairRequest) -> Result<RepairReport> {
    if request.edit_id.is_empty()
        || request.edit_id.len() > 128
        || !request
            .edit_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        || !request.tolerance.is_finite()
        || request.tolerance <= 0.
        || request.tolerance > 1.
    {
        return Err(invalid(
            "Repair requires a stable edit identity and an explicit positive tolerance up to 1 mm",
        ));
    }
    let driven = sketch_edit::driven_points(sketch);
    let selected = request.members.iter().collect::<BTreeSet<_>>();
    let aliases = topological_point_aliases(sketch, 1e-6)?;
    let suppressed = crate::sketch_transform::suppressed_members(sketch);
    let mut eligible = BTreeSet::new();
    let mut degree = BTreeMap::<Id, usize>::new();
    let mut owners = BTreeMap::<Id, BTreeSet<Id>>::new();
    for (key, member) in &sketch.members {
        if suppressed.contains(key) {
            continue;
        }
        if !request.include_construction && member.construction {
            continue;
        }
        let curve = sketch_edit::effective_curve(sketch, key)?;
        if matches!(
            curve,
            SketchCurve::Circle { .. } | SketchCurve::Ellipse { .. }
        ) {
            continue;
        }
        let points = curve.points();
        if points.len() < 2 {
            return Err(invalid(
                "Repair encountered a member without two endpoint controls",
            ));
        }
        for point in [points[0], *points.last().unwrap()] {
            let canonical = aliases
                .get(point)
                .ok_or_else(|| invalid("Repair encountered a missing endpoint"))?
                .clone();
            *degree.entry(canonical.clone()).or_default() += 1;
            owners
                .entry(canonical.clone())
                .or_default()
                .insert(key.clone());
            if selected.is_empty() || selected.contains(key) {
                eligible.insert(canonical);
            }
        }
    }
    let endpoints = degree
        .iter()
        .filter(|(key, n)| **n == 1 && eligible.contains(*key) && !driven.contains(*key))
        .map(|(key, _)| key.clone())
        .collect::<Vec<_>>();
    let mut neighbors = BTreeMap::<Id, Vec<Id>>::new();
    for (i, a) in endpoints.iter().enumerate() {
        for b in endpoints.iter().skip(i + 1) {
            if owners[a].intersection(&owners[b]).next().is_some() {
                continue;
            }
            if sketch.constraints.iter().any(|c| {
                !c.driven
                    && c.kind == "coincident"
                    && c.entities.len() == 2
                    && ((c.entities[0] == *a && c.entities[1] == *b)
                        || (c.entities[0] == *b && c.entities[1] == *a))
            }) {
                continue;
            }
            let (p, q) = (point(sketch, a)?, point(sketch, b)?);
            let distance = (p[0] - q[0]).hypot(p[1] - q[1]);
            if distance <= request.tolerance {
                neighbors.entry(a.clone()).or_default().push(b.clone());
                neighbors.entry(b.clone()).or_default().push(a.clone());
            }
        }
    }
    let mut report = RepairReport {
        joined_endpoints: vec![],
        removed_relations: vec![],
        ambiguous_endpoints: vec![],
        remaining_open_endpoints: endpoints.len(),
    };
    for a in &endpoints {
        let Some(neighbors_a) = neighbors.get(a) else {
            continue;
        };
        if neighbors_a.len() != 1 {
            report.ambiguous_endpoints.push(a.clone());
            continue;
        }
        let b = &neighbors_a[0];
        if neighbors.get(b).is_none_or(|n| n.len() != 1) {
            report.ambiguous_endpoints.push(a.clone());
            continue;
        }
        if a >= b {
            continue;
        }
        let id = format!("{}-join-{}", request.edit_id, report.joined_endpoints.len());
        if sketch.constraints.iter().any(|c| c.id == id) {
            return Err(invalid(
                "Repair namespace already owns a saved relation; begin a new repair",
            ));
        }
        sketch.constraints.push(SketchConstraint {
            id,
            kind: "coincident".into(),
            entities: vec![a.clone(), b.clone()],
            value: "0 mm".into(),
            driven: false,
            distance_side: None,
            contact: None,
        });
        report.joined_endpoints.push([a.clone(), b.clone()]);
    }
    report.remaining_open_endpoints = report
        .remaining_open_endpoints
        .saturating_sub(2 * report.joined_endpoints.len());
    if request.remove_duplicate_relations {
        let mut retained = Vec::<SketchConstraint>::new();
        for c in sketch.constraints.drain(..) {
            if retained.iter().any(|a| {
                a.kind == c.kind
                    && a.entities == c.entities
                    && a.value == c.value
                    && a.driven == c.driven
                    && a.contact == c.contact
                    && a.distance_side == c.distance_side
            }) {
                report.removed_relations.push(c.id);
            } else {
                retained.push(c);
            }
        }
        sketch.constraints = retained;
        crate::sketch_dimensions::reconcile_removed(sketch);
    }
    Ok(report)
}

/// Current equation residuals for honest conflict emphasis, not a minimal conflict claim.
pub fn conflict_residuals(
    doc: &Document,
    owner: &str,
    source: &Sketch,
) -> Result<BTreeMap<Id, f64>> {
    let mut geometry = source.clone();
    crate::sketch_actions::apply_curve_dimensions(doc, owner, &mut geometry)?;
    crate::sketch_edit::derive(doc, owner, &mut geometry)?;
    Ok(
        crate::sketch_actions::residual_blocks(doc, owner, &geometry, |key| {
            crate::sketch_actions::entity_point(&geometry, key)
        })?
        .into_iter()
        .filter_map(|block| {
            let residual = block.values.into_iter().map(f64::abs).fold(0., f64::max);
            (residual.is_finite() && residual > 1e-6).then_some((block.constraint, residual))
        })
        .collect(),
    )
}
