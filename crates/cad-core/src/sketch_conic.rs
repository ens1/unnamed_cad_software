//! Exact conics and curved slots with persistent source handles and parameter intent.
use crate::*;
use nalgebra::Vector2;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, f64::consts::TAU};
pub const CONTRACT_VERSION: u32 = 1;
const EPS: f64 = 1e-7;
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Definition {
    Conic {
        points: [Id; 3],
        rho: Expr,
    },
    Parabola {
        points: [Id; 3],
    },
    EllipseArc {
        center: Id,
        major: Id,
        minor: Id,
        start: Expr,
        sweep: Expr,
        controls: [Id; 9],
    },
    ArcSlot {
        center: Id,
        start: Id,
        end: Id,
        width: Expr,
        clockwise: bool,
        arcs: [Id; 4],
        controls: [Id; 8],
    },
}
impl Definition {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Conic { .. } => "Conic",
            Self::Parabola { .. } => "Parabola",
            Self::EllipseArc { .. } => "Partial Ellipse",
            Self::ArcSlot { .. } => "Arc Slot",
        }
    }
    pub fn source_points(&self) -> Vec<Id> {
        match self {
            Self::Conic { points, .. } | Self::Parabola { points } => points.to_vec(),
            Self::EllipseArc {
                center,
                major,
                minor,
                ..
            } => vec![center.clone(), major.clone(), minor.clone()],
            Self::ArcSlot {
                center, start, end, ..
            } => vec![center.clone(), start.clone(), end.clone()],
        }
    }
    pub fn outputs(&self, member: &str) -> Vec<Id> {
        match self {
            Self::ArcSlot { arcs, .. } => arcs.to_vec(),
            _ => vec![member.into()],
        }
    }
    pub fn driven_points(&self) -> Vec<Id> {
        match self {
            Self::EllipseArc { controls, .. } => controls.to_vec(),
            Self::ArcSlot { controls, .. } => controls.to_vec(),
            _ => vec![],
        }
    }
    pub fn expressions(&self) -> Vec<&Expr> {
        match self {
            Self::Conic { rho, .. } => vec![rho],
            Self::Parabola { .. } => vec![],
            Self::EllipseArc { start, sweep, .. } => vec![start, sweep],
            Self::ArcSlot { width, .. } => vec![width],
        }
    }
}
fn invalid(message: &str) -> CadError {
    CadError::Invalid(message.into())
}
fn point(sketch: &Sketch, key: &str) -> Result<Vector2<f64>> {
    sketch
        .points
        .get(key)
        .copied()
        .map(Vector2::from)
        .filter(|p| p.iter().all(|v| v.is_finite()))
        .ok_or_else(|| invalid("Conic source point is missing or non-finite"))
}
fn left(vector: Vector2<f64>) -> Vector2<f64> {
    Vector2::new(-vector.y, vector.x)
}
fn put(sketch: &mut Sketch, key: &str, p: Vector2<f64>) -> Result<()> {
    if p.iter().any(|v| !v.is_finite()) {
        return Err(invalid("Conic produced a non-finite coordinate"));
    }
    sketch.points.insert(key.into(), [p.x, p.y]);
    Ok(())
}
pub fn operation_for<'a>(sketch: &'a Sketch, member: &str) -> Option<(&'a Id, &'a Definition)> {
    sketch
        .conics
        .iter()
        .find(|(key, definition)| definition.outputs(key).iter().any(|id| id == member))
}
pub fn editable(sketch: &Sketch, member: &str) -> Result<()> {
    operation_for(sketch, member)
        .map(|_| ())
        .ok_or_else(|| invalid("Select a conic, parabola, partial ellipse, or arc slot member"))
}
pub fn validate(sketch: &Sketch, key: &str, definition: &Definition) -> Result<()> {
    let source = definition.source_points();
    let controls = definition.driven_points();
    let outputs = definition.outputs(key);
    if source.len() != source.iter().collect::<BTreeSet<_>>().len()
        || source.iter().any(|id| !sketch.points.contains_key(id))
        || controls
            .iter()
            .any(|id| !sketch.points.contains_key(id) || source.contains(id))
        || controls.len() != controls.iter().collect::<BTreeSet<_>>().len()
        || outputs.len() != outputs.iter().collect::<BTreeSet<_>>().len()
        || outputs.iter().any(|id| !sketch.members.contains_key(id))
    {
        return Err(invalid(
            "Conic has missing, duplicate, or inconsistent source/output references",
        ));
    }
    match definition {
        Definition::Conic { points, .. } | Definition::Parabola { points }
            if !matches!(&sketch.members[key].curve, SketchCurve::Nurbs { points: actual, degree:2, .. } if actual == points) => {
                return Err(invalid("Conic requires its three original quadratic controls"));
        }
        Definition::EllipseArc { controls, .. }
            if !matches!(&sketch.members[key].curve, SketchCurve::Nurbs { points, degree:2, .. } if points.len()==9 && points[..8] == controls[..8] && (points[8] == controls[8] || points[8] == controls[0])) => {
                return Err(invalid("Partial ellipse requires four exact rational quadratic spans"));
        }
        Definition::ArcSlot { arcs, controls, .. }
            if arcs[0] != key || arcs.iter().enumerate().any(|(i,member)| !matches!(&sketch.members[member].curve,
                SketchCurve::Arc { start,through,end } if start == &controls[i] && through == &controls[i+4] && end == &controls[(i+1)%4])) => {
                return Err(invalid("Arc slot requires its four connected circular arcs"));
        }
        _ => {}
    }
    Ok(())
}
pub fn derive(
    doc: &Document,
    owner: &str,
    sketch: &mut Sketch,
    key: &str,
    definition: &Definition,
) -> Result<()> {
    validate(sketch, key, definition)?;
    match definition {
        Definition::Conic { points, .. } | Definition::Parabola { points } => {
            let a = point(sketch, &points[0])?;
            let b = point(sketch, &points[1])?;
            let c = point(sketch, &points[2])?;
            let chord = c - a;
            let length = chord.x.hypot(chord.y);
            let tangent = b - a;
            let height = if length > EPS {
                (tangent.x * (chord.y / length) - tangent.y * (chord.x / length)).abs()
            } else {
                0.
            };
            if !length.is_finite() || !height.is_finite() || length <= EPS || height <= EPS {
                return Err(invalid(
                    "Conic endpoints and tangent intersection must form a nondegenerate triangle",
                ));
            }
            let rho = if let Definition::Conic { rho, .. } = definition {
                expression::value(doc, owner, rho, Dimension::Scalar)?
            } else {
                0.5
            };
            if !(0. ..1.).contains(&rho) || rho <= 0. {
                return Err(invalid(
                    "Conic rho must lie strictly between 0 and 1; 0.5 defines a parabola",
                ));
            }
            let weight = rho / (1. - rho);
            sketch
                .members
                .get_mut(key)
                .ok_or_else(|| invalid("Conic member missing"))?
                .curve = SketchCurve::Nurbs {
                points: points.to_vec(),
                weights: vec![1., weight, 1.],
                knots: vec![0., 0., 0., 1., 1., 1.],
                degree: 2,
            };
        }
        Definition::EllipseArc {
            center,
            major,
            minor,
            start,
            sweep,
            controls,
        } => {
            let c = point(sketch, center)?;
            let a = point(sketch, major)? - c;
            let length = a.x.hypot(a.y);
            let axis = if length.is_finite() && length > EPS {
                a / length
            } else {
                return Err(invalid("Ellipse major axis must have positive length"));
            };
            let b = (point(sketch, minor)? - c).dot(&left(axis)).abs();
            if !b.is_finite() || b <= EPS {
                return Err(invalid(
                    "Ellipse minor handle must have a nonzero perpendicular distance from its major axis",
                ));
            }
            let b = left(axis) * b;
            let start = expression::value(doc, owner, start, Dimension::Angle)?.rem_euclid(TAU);
            let sweep = expression::value(doc, owner, sweep, Dimension::Angle)?;
            if sweep.abs() <= 1e-9 || sweep.abs() > TAU + 1e-12 {
                return Err(invalid(
                    "Ellipse sweep must be nonzero and no greater than 360 degrees",
                ));
            }
            let sweep = sweep.clamp(-TAU, TAU);
            let step = sweep / 4.;
            let weight = (step / 2.).cos();
            for (i, control) in controls.iter().enumerate() {
                let angle = start + step * i as f64 / 2.;
                let radial = a * angle.cos() + b * angle.sin();
                put(
                    sketch,
                    control,
                    c + if i % 2 == 0 { radial } else { radial / weight },
                )?;
            }
            let mut points = controls.to_vec();
            if (sweep.abs() - TAU).abs() <= 1e-12 {
                points[8] = controls[0].clone();
            }
            sketch
                .members
                .get_mut(key)
                .ok_or_else(|| invalid("Ellipse member missing"))?
                .curve = SketchCurve::Nurbs {
                points,
                weights: (0..9)
                    .map(|i| if i % 2 == 0 { 1. } else { weight })
                    .collect(),
                knots: ellipse_knots(),
                degree: 2,
            };
        }
        Definition::ArcSlot {
            center,
            start,
            end,
            width,
            clockwise,
            controls,
            ..
        } => {
            let c = point(sketch, center)?;
            let u = point(sketch, start)? - c;
            let radius = u.x.hypot(u.y);
            let v = point(sketch, end)? - c;
            let end_radius = v.x.hypot(v.y);
            if !radius.is_finite() || !end_radius.is_finite() || radius <= EPS || end_radius <= EPS
            {
                return Err(invalid(
                    "Arc-slot center and direction handles must be distinct",
                ));
            }
            let start_angle = u.y.atan2(u.x);
            let end_angle = v.y.atan2(v.x);
            let sweep = if *clockwise {
                -(start_angle - end_angle).rem_euclid(TAU)
            } else {
                (end_angle - start_angle).rem_euclid(TAU)
            };
            if sweep.abs() <= 1e-8 || TAU - sweep.abs() <= 1e-8 {
                return Err(invalid(
                    "Arc slot needs distinct end directions and a sweep below 360 degrees",
                ));
            }
            let width = expression::value(doc, owner, width, Dimension::Length)?;
            if width <= EPS || width / 2. >= radius - EPS {
                return Err(invalid(
                    "Arc-slot width must be positive and below twice its centerline radius",
                ));
            }
            let h = width / 2.;
            let inner = radius - h;
            let outer = radius + h;
            let u = u / radius;
            let v = v / end_radius;
            let middle = Vector2::new(
                (start_angle + sweep / 2.).cos(),
                (start_angle + sweep / 2.).sin(),
            );
            let positions = [
                c + u * inner,
                c + v * inner,
                c + v * outer,
                c + u * outer,
                c + middle * inner,
                c + v * radius + left(v) * h * sweep.signum(),
                c + middle * outer,
                c + u * radius - left(u) * h * sweep.signum(),
            ];
            // Long slots can overlap their end caps even when both radii remain valid.
            // Their planar contour must be simple for downstream solid operations.
            if sweep.abs() > std::f64::consts::PI {
                let end_distance = (u - v).norm() * radius;
                if end_distance <= width + EPS {
                    return Err(invalid(
                        "Arc-slot end caps overlap; reduce its sweep or width",
                    ));
                }
            }
            for (control, position) in controls.iter().zip(positions) {
                put(sketch, control, position)?;
            }
        }
    }
    Ok(())
}
fn ellipse_knots() -> Vec<f64> {
    vec![0., 0., 0., 0.25, 0.25, 0.5, 0.5, 0.75, 0.75, 1., 1., 1.]
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Conic,
    Parabola,
    EllipseArc,
    ArcSlot,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Create {
    pub kind: Kind,
    pub source: [Id; 3],
    pub size: Expr,
    pub clockwise: bool,
    pub construction: bool,
}
pub fn add(doc: &Document, owner: &str, sketch: &mut Sketch, request: &Create) -> Result<Id> {
    let Create {
        kind,
        source,
        size,
        clockwise,
        construction,
    } = request.clone();
    let mut candidate = sketch.clone();
    let member = id();
    let definition = match kind {
        Kind::Conic => Definition::Conic {
            points: [source[0].clone(), source[2].clone(), source[1].clone()],
            rho: size,
        },
        Kind::Parabola => Definition::Parabola {
            points: [source[0].clone(), source[2].clone(), source[1].clone()],
        },
        Kind::EllipseArc => Definition::EllipseArc {
            center: source[0].clone(),
            major: source[1].clone(),
            minor: source[2].clone(),
            start: "0 deg".into(),
            sweep: "180 deg".into(),
            controls: std::array::from_fn(|_| id()),
        },
        Kind::ArcSlot => Definition::ArcSlot {
            center: source[0].clone(),
            start: source[1].clone(),
            end: source[2].clone(),
            width: size,
            clockwise,
            arcs: std::array::from_fn(|i| if i == 0 { member.clone() } else { id() }),
            controls: std::array::from_fn(|_| id()),
        },
    };
    for control in definition.driven_points() {
        candidate.points.insert(control, [0., 0.]);
    }
    match &definition {
        Definition::Conic { points, .. } | Definition::Parabola { points } => {
            candidate.members.insert(
                member.clone(),
                SketchMember {
                    construction,
                    curve: SketchCurve::Nurbs {
                        points: points.to_vec(),
                        weights: vec![1.; 3],
                        knots: vec![0., 0., 0., 1., 1., 1.],
                        degree: 2,
                    },
                },
            );
        }
        Definition::EllipseArc { controls, .. } => {
            candidate.members.insert(
                member.clone(),
                SketchMember {
                    construction,
                    curve: SketchCurve::Nurbs {
                        points: controls.to_vec(),
                        weights: vec![1.; 9],
                        knots: ellipse_knots(),
                        degree: 2,
                    },
                },
            );
        }
        Definition::ArcSlot { arcs, controls, .. } => {
            for (i, member) in arcs.iter().enumerate() {
                candidate.members.insert(
                    member.clone(),
                    SketchMember {
                        construction,
                        curve: SketchCurve::Arc {
                            start: controls[i].clone(),
                            through: controls[i + 4].clone(),
                            end: controls[(i + 1) % 4].clone(),
                        },
                    },
                );
            }
        }
    }
    candidate.conics.insert(member.clone(), definition);
    crate::sketch_edit::derive(doc, owner, &mut candidate)?;
    *sketch = candidate;
    Ok(member)
}
pub fn set(
    doc: &Document,
    owner: &str,
    sketch: &mut Sketch,
    member: &str,
    definition: Definition,
) -> Result<()> {
    let key = operation_for(sketch, member)
        .map(|(key, _)| key.clone())
        .ok_or_else(|| invalid("Conic source definition is missing"))?;
    let mut candidate = sketch.clone();
    candidate.conics.insert(key, definition);
    crate::sketch_edit::derive(doc, owner, &mut candidate)?;
    *sketch = candidate;
    Ok(())
}
