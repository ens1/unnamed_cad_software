//! Exact mechanical and advanced mate residuals. Lengths are mm and angles radians.
use crate::{CadError, Dimension, Document, Mate, Result, assembly_geometry::*, expression};
use nalgebra::{Matrix4, Vector3};
const ANGULAR_SCALE: f64 = 100.;
pub(crate) struct Context<'a> {
    pub doc: &'a Document,
    pub owner: &'a str,
    pub mate: &'a Mate,
    pub first: &'a Frame,
    pub second: &'a Frame,
    pub auxiliary: &'a [Frame],
    pub support: Option<&'a Matrix4<f64>>,
    pub parameter: Option<f64>,
}
fn value(c: &Context<'_>, e: &crate::Expr, d: Dimension) -> Result<f64> {
    expression::value(c.doc, c.owner, e, d)
}
fn range(c: &Context<'_>, r: &Range, d: Dimension, q: f64) -> Result<f64> {
    let lo = value(c, &r.minimum, d)?;
    let hi = value(c, &r.maximum, d)?;
    if lo > hi {
        return Err(CadError::Invalid("Mate minimum exceeds maximum".into()));
    }
    Ok(q.clamp(lo, hi) - q)
}
fn axis(f: &Frame) -> Vector3<f64> {
    Vector3::from(f.axis)
}
fn origin(f: &Frame) -> Vector3<f64> {
    Vector3::from(f.origin)
}
fn align(a: Vector3<f64>, b: Vector3<f64>, u: Vector3<f64>, how: Alignment) -> Vector3<f64> {
    crate::solver::aligned_axes(a, b, u, how)
}
fn directional(f: &Frame) -> bool {
    matches!(
        f.kind,
        FrameKind::Axis | FrameKind::Legacy | FrameKind::Plane
    )
}
fn require_planes(frames: &[&Frame]) -> Result<()> {
    if frames.iter().any(|f| f.kind != FrameKind::Plane) {
        return Err(CadError::Invalid(
            "This mate requires planar faces or datum planes".into(),
        ));
    }
    Ok(())
}
fn angular(a: &Frame, b: &Frame) -> f64 {
    let n = axis(a);
    let u = Vector3::from(a.secondary);
    let v = Vector3::from(b.secondary);
    n.dot(&u.cross(&v)).atan2(u.dot(&v))
}
fn distance(a: &Frame, b: &Frame) -> f64 {
    let d = origin(b) - origin(a);
    if a.kind == FrameKind::Plane {
        d.dot(&axis(a))
    } else if b.kind == FrameKind::Plane {
        d.dot(&axis(b))
    } else if a.kind == FrameKind::Axis {
        d.cross(&axis(a)).norm()
    } else if b.kind == FrameKind::Axis {
        d.cross(&axis(b)).norm()
    } else {
        d.norm()
    }
}
pub(crate) fn equations(c: Context<'_>) -> Result<Vec<f64>> {
    let relation = c.mate.relationship.as_deref().ok_or_else(|| {
        CadError::Invalid("Mechanical mate needs its relationship parameters".into())
    })?;
    let a = c.first;
    let b = c.second;
    if a.kind == FrameKind::Curve
        && !matches!(
            relation,
            Relationship::Path { .. } | Relationship::Slot { .. } | Relationship::Cam { .. }
        )
        || b.kind == FrameKind::Curve
    {
        return Err(CadError::Invalid(
            "Curve geometry is available only as the first Path, Slot, or Cam reference".into(),
        ));
    }
    if matches!(
        relation,
        Relationship::Path { .. } | Relationship::Slot { .. } | Relationship::Cam { .. }
    ) && !matches!(
        b.kind,
        FrameKind::Axis | FrameKind::Point | FrameKind::Legacy
    ) {
        return Err(CadError::Invalid(
            "Path / slot / cam followers require a point or axis reference".into(),
        ));
    }
    if (a.kind == FrameKind::Profile || b.kind == FrameKind::Profile)
        && !matches!(relation, Relationship::ProfileCenter { .. })
    {
        return Err(CadError::Invalid(
            "Profile centroid references require Profile Center mates".into(),
        ));
    }
    let av = axis(a);
    let bv = axis(b);
    let au = Vector3::from(a.secondary);
    let delta = origin(b) - origin(a);
    let mut out = vec![];
    match relation {
        Relationship::ProfileCenter { rotation } => {
            if a.kind != FrameKind::Profile || b.kind != FrameKind::Profile {
                return Err(CadError::Invalid(
                    "Profile Center requires two closed planar profile or face centroid references"
                        .into(),
                ));
            }
            out.extend(delta.iter().copied());
            out.extend(align(av, bv, au, c.mate.alignment).iter().copied());
            if let Some(rotation) = rotation {
                let theta = value(&c, rotation, Dimension::Angle)?;
                let q = angular(a, b) - theta;
                out.push(
                    (q + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU) * ANGULAR_SCALE
                        - std::f64::consts::PI * ANGULAR_SCALE,
                );
            }
        }
        Relationship::Width { position, .. } => {
            let aa = c
                .auxiliary
                .first()
                .ok_or_else(|| CadError::Invalid("First width wall is unresolved".into()))?;
            let bb = c
                .auxiliary
                .get(1)
                .ok_or_else(|| CadError::Invalid("Second tab wall is unresolved".into()))?;
            require_planes(&[a, b, aa, bb])?;
            out.extend(align(av, axis(aa), au, Alignment::Either).iter().copied());
            out.extend(
                align(bv, axis(bb), Vector3::from(b.secondary), Alignment::Either)
                    .iter()
                    .copied(),
            );
            out.extend(align(av, bv, au, c.mate.alignment).iter().copied());
            let width = (origin(aa) - origin(a)).dot(&av);
            let tab = (origin(bb) - origin(b)).dot(&av);
            if width.abs() < 1e-8 || tab.abs() < 1e-8 {
                return Err(CadError::Invalid(
                    "Width and tab each need two distinct parallel walls".into(),
                ));
            }
            let (lo, hi) = (0f64.min(width), 0f64.max(width));
            let q = delta.dot(&av);
            let tab_lo = q + 0f64.min(tab);
            let thickness = tab.abs();
            let travel = hi - lo - thickness;
            // The interior is a genuine remaining sliding DOF in Free mode.
            out.push(match position {
                WidthPosition::Centered => {
                    (origin(b) + origin(bb) - origin(a) - origin(aa)).dot(&av) * 0.5
                }
                WidthPosition::Free => 0.,
                WidthPosition::Distance { value: e } => {
                    tab_lo - lo - value(&c, e, Dimension::Length)?
                }
                WidthPosition::Fraction { value: e } => {
                    let f = value(&c, e, Dimension::Scalar)?;
                    if !(0. ..=1.).contains(&f) {
                        return Err(CadError::Invalid("Width fraction must be 0..1".into()));
                    }
                    tab_lo - lo - travel * f
                }
            });
            out.push(tab_lo.clamp(lo, (hi - thickness).max(lo)) - tab_lo);
            out.push((-travel).max(0.));
        }
        Relationship::Symmetry { .. } => {
            let p = c
                .auxiliary
                .first()
                .ok_or_else(|| CadError::Invalid("Symmetry plane is unresolved".into()))?;
            require_planes(&[p])?;
            let n = axis(p);
            let reflected = origin(a) - n * 2. * (origin(a) - origin(p)).dot(&n);
            out.extend((origin(b) - reflected).iter().copied());
            if directional(a) && directional(b) {
                let reflected = av - n * 2. * av.dot(&n);
                out.extend(align(reflected, bv, au, c.mate.alignment).iter().copied());
            }
        }
        Relationship::LimitDistance { range: r } => {
            out.push(range(&c, r, Dimension::Length, distance(a, b))?);
            if a.kind == FrameKind::Plane && b.kind == FrameKind::Plane {
                out.extend(align(av, bv, au, c.mate.alignment).iter().copied());
            }
        }
        Relationship::LimitAngle { range: r } => {
            if !directional(a) || !directional(b) {
                return Err(CadError::Invalid(
                    "Limit Angle requires directional geometry".into(),
                ));
            }
            let baxis = if c.mate.alignment == Alignment::Opposed {
                -bv
            } else {
                bv
            };
            let q = av.cross(&baxis).norm().atan2(av.dot(&baxis));
            let lo = value(&c, &r.minimum, Dimension::Angle)?;
            let hi = value(&c, &r.maximum, Dimension::Angle)?;
            if lo < 0. || hi > std::f64::consts::PI {
                return Err(CadError::Invalid(
                    "Limit Angle bounds must lie in 0..180 degrees".into(),
                ));
            }
            out.push(range(&c, r, Dimension::Angle, q)? * ANGULAR_SCALE);
        }
        Relationship::Hinge { range: r, .. } => {
            if !matches!(a.kind, FrameKind::Axis | FrameKind::Legacy)
                || !matches!(b.kind, FrameKind::Axis | FrameKind::Legacy)
            {
                return Err(CadError::Invalid("Hinge requires two axes".into()));
            }
            let aa = c.auxiliary.first().ok_or_else(|| {
                CadError::Invalid("First hinge seating plane is unresolved".into())
            })?;
            let bb = c.auxiliary.get(1).ok_or_else(|| {
                CadError::Invalid("Second hinge seating plane is unresolved".into())
            })?;
            require_planes(&[aa, bb])?;
            out.extend(delta.cross(&av).iter().copied());
            out.extend(align(av, bv, au, c.mate.alignment).iter().copied());
            out.extend(align(av, axis(aa), au, Alignment::Either).iter().copied());
            out.extend(
                align(bv, axis(bb), Vector3::from(b.secondary), Alignment::Either)
                    .iter()
                    .copied(),
            );
            out.push((origin(bb) - origin(aa)).dot(&axis(aa)));
            if let Some(r) = r {
                let lo = value(&c, &r.minimum, Dimension::Angle)?;
                let hi = value(&c, &r.maximum, Dimension::Angle)?;
                if lo < -std::f64::consts::PI || hi > std::f64::consts::PI {
                    return Err(CadError::Invalid(
                        "Hinge limits require -180..180 degrees in the captured frame".into(),
                    ));
                }
                out.push(range(&c, r, Dimension::Angle, angular(a, b))? * ANGULAR_SCALE);
            }
        }
        Relationship::Coupler {
            motion,
            zero,
            ratio,
            pitch,
            phase,
            reverse,
            ..
        } => {
            if !matches!(a.kind, FrameKind::Axis | FrameKind::Legacy)
                || !matches!(b.kind, FrameKind::Axis | FrameKind::Legacy)
            {
                return Err(CadError::Invalid(
                    "Mechanical coupling requires two axes".into(),
                ));
            }
            let inverse = c
                .support
                .map(|m| {
                    m.try_inverse().ok_or_else(|| {
                        CadError::Invalid("Mechanical support transform is singular".into())
                    })
                })
                .transpose()?
                .unwrap_or_else(Matrix4::identity);
            let aa = a.transformed(&inverse)?;
            let bb = b.transformed(&inverse)?;
            // Motion axes retain their captured direction in the explicitly chosen support.
            out.extend(
                align(
                    axis(&zero[0]),
                    axis(&aa),
                    Vector3::from(zero[0].secondary),
                    Alignment::Aligned,
                )
                .iter()
                .copied(),
            );
            out.extend(
                align(
                    axis(&zero[1]),
                    axis(&bb),
                    Vector3::from(zero[1].secondary),
                    Alignment::Aligned,
                )
                .iter()
                .copied(),
            );
            let angle1 = angular(&zero[0], &aa);
            let angle2 = angular(&zero[1], &bb);
            let linear1 = (origin(&aa) - origin(&zero[0])).dot(&axis(&zero[0]));
            let linear2 = (origin(&bb) - origin(&zero[1])).dot(&axis(&zero[1]));
            let sign = if *reverse { -1. } else { 1. };
            match motion {
                CouplerKind::Gear => {
                    let r = value(&c, ratio, Dimension::Scalar)?;
                    if r <= 0. {
                        return Err(CadError::Invalid("Gear ratio must be positive".into()));
                    }
                    out.push(
                        (angle2 + sign * r * angle1 - value(&c, phase, Dimension::Angle)?)
                            * ANGULAR_SCALE,
                    );
                }
                CouplerKind::RackPinion => {
                    let lead = value(&c, pitch, Dimension::Length)?;
                    if lead <= 0. {
                        return Err(CadError::Invalid(
                            "Pitch / lead per revolution must be positive".into(),
                        ));
                    }
                    out.push(
                        linear2
                            - sign * lead * angle1 / std::f64::consts::TAU
                            - value(&c, phase, Dimension::Length)?,
                    );
                }
                CouplerKind::Screw => {
                    let lead = value(&c, pitch, Dimension::Length)?;
                    if lead <= 0. {
                        return Err(CadError::Invalid(
                            "Screw lead per revolution must be positive".into(),
                        ));
                    }
                    let a0 = axis(&zero[0]);
                    let b0 = axis(&zero[1]);
                    if a0.cross(&b0).norm() > 1e-8 {
                        return Err(CadError::Invalid(
                            "Screw shafts require parallel captured directions".into(),
                        ));
                    }
                    let direction = if a0.dot(&b0) >= 0. { 1. } else { -1. };
                    out.extend(delta.cross(&av).iter().copied());
                    let axial = direction * linear2 - linear1;
                    let twist = angle1 - direction * angle2;
                    out.push(
                        axial
                            - sign * lead * twist / std::f64::consts::TAU
                            - value(&c, phase, Dimension::Length)?,
                    );
                }
                CouplerKind::UniversalJoint => {
                    let cosine = axis(&zero[0]).dot(&axis(&zero[1]));
                    if cosine.abs() < 1e-6 {
                        return Err(CadError::Invalid("Universal-joint shafts cannot be perpendicular; the angular transmission is singular".into()));
                    }
                    out.extend(delta.iter().copied());
                    let target = (sign * cosine * angle1.sin()).atan2(angle1.cos())
                        + value(&c, phase, Dimension::Angle)?;
                    let q = angle2 - target;
                    out.push(
                        ((q + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU)
                            - std::f64::consts::PI)
                            * ANGULAR_SCALE,
                    );
                }
                CouplerKind::Linear => {
                    let r = value(&c, ratio, Dimension::Scalar)?;
                    if r <= 0. {
                        return Err(CadError::Invalid(
                            "Linear coupling ratio must be positive".into(),
                        ));
                    }
                    out.push(linear2 - sign * r * linear1 - value(&c, phase, Dimension::Length)?);
                }
            }
        }
        Relationship::Slot {
            position,
            orientation,
        }
        | Relationship::Path {
            position,
            orientation,
        } => {
            let curve = a.curve.as_deref().ok_or_else(|| {
                CadError::Invalid("First reference must be an exact planar curve".into())
            })?;
            if matches!(relation, Relationship::Slot { .. })
                && curve.slot_carrier.is_none()
                && !matches!(
                    curve.geometry,
                    CurveGeometry::Line { .. } | CurveGeometry::Circle { .. }
                )
            {
                return Err(CadError::Invalid(
                    "Slot center paths require a line, circle, or circular arc".into(),
                ));
            }
            let t = parameter(&c, position, curve, &mut out)?;
            out.extend((origin(b) - Vector3::from(curve.point(t)?)).iter().copied());
            if *orientation != PathOrientation::Free {
                if !directional(b) {
                    return Err(CadError::Invalid(
                        "Path orientation requires a follower axis or plane normal".into(),
                    ));
                }
                let desired = if *orientation == PathOrientation::Normal {
                    axis(a)
                } else {
                    Vector3::from(curve.tangent(t)?)
                };
                out.extend(
                    align(desired, bv, Vector3::from(a.secondary), c.mate.alignment)
                        .iter()
                        .copied(),
                );
            }
        }
        Relationship::Cam {
            position,
            follower_radius,
            outward,
        } => {
            let curve = a
                .curve
                .as_deref()
                .ok_or_else(|| CadError::Invalid("Cam requires an exact planar curve".into()))?;
            let radius = value(&c, follower_radius, Dimension::Length)?;
            if radius < 0. {
                return Err(CadError::Invalid(
                    "Cam follower radius must be nonnegative".into(),
                ));
            }
            let t = parameter(&c, position, curve, &mut out)?;
            let tangent = Vector3::from(curve.tangent(t)?);
            let normal = tangent.cross(&axis(a)).normalize() * if *outward { 1. } else { -1. };
            let p = Vector3::from(curve.point(t)?) + normal * radius;
            out.extend((origin(b) - p).iter().copied());
            if directional(b) {
                out.extend(
                    align(axis(a), bv, Vector3::from(a.secondary), c.mate.alignment)
                        .iter()
                        .copied(),
                );
            }
        }
    }
    Ok(out)
}
fn parameter(
    c: &Context<'_>,
    position: &CurvePosition,
    curve: &Curve,
    out: &mut Vec<f64>,
) -> Result<f64> {
    let t = match position {
        CurvePosition::Free { .. } => c
            .parameter
            .ok_or_else(|| CadError::Invalid("Free path parameter is missing".into()))?,
        CurvePosition::Fraction { value: e } => {
            let t = value(c, e, Dimension::Scalar)?;
            if !(0. ..=1.).contains(&t) {
                return Err(CadError::Invalid("Path fraction must be 0..1".into()));
            }
            t
        }
    };
    if curve.closed() {
        out.push(0.);
        Ok(t.rem_euclid(1.))
    } else {
        out.push((t.clamp(0., 1.) - t) * 100.);
        Ok(t.clamp(0., 1.))
    }
}

/// Inequalities bound travel but do not remove its geometric degree of freedom.
pub(crate) fn inequality_rows(
    doc: &Document,
    owner: &str,
    relation: &Relationship,
    count: usize,
) -> Result<Vec<usize>> {
    let bounded = |range: &Range, dimension| -> Result<bool> {
        Ok(expression::value(doc, owner, &range.minimum, dimension)?
            != expression::value(doc, owner, &range.maximum, dimension)?)
    };
    Ok(match relation {
        Relationship::LimitDistance { range } if bounded(range, Dimension::Length)? => vec![0],
        Relationship::LimitAngle { range } if bounded(range, Dimension::Angle)? => vec![0],
        Relationship::Hinge {
            range: Some(range), ..
        } if bounded(range, Dimension::Angle)? => vec![count - 1],
        Relationship::Width {
            position: WidthPosition::Free,
            ..
        } => vec![count - 3, count - 2, count - 1],
        Relationship::Width { .. } => vec![count - 2, count - 1],
        Relationship::Path { .. } | Relationship::Slot { .. } | Relationship::Cam { .. } => vec![0],
        _ => vec![],
    })
}
