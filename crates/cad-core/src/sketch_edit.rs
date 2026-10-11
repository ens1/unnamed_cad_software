//! Version 4 sketch editing intent and exact planar constructions.
use crate::*;
use nalgebra::{Vector2, Vector3};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
const EPS: f64 = 1e-7;
pub const CONTRACT_VERSION: u32 = 4;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Curve3 {
    pub control_points: Vec<[f64; 3]>,
    pub weights: Vec<f64>,
    pub knots: Vec<f64>,
    pub degree: usize,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Curve2 {
    pub control_points: Vec<[f64; 2]>,
    pub weights: Vec<f64>,
    pub knots: Vec<f64>,
    pub degree: usize,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Conversion {
    pub body: NodeRef,
    pub reference: TopologyRef,
    pub curve: Curve2,
    pub points: Vec<Id>,
    /// New references follow the native carrier orientation, independent of
    /// incidental face-boundary ordering. Older sources retain their ordering.
    #[serde(default, skip_serializing_if = "crate::sketch_actions::is_false")]
    pub construction_orientation: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct OffsetMember {
    pub source: Id,
    pub member: Id,
    pub points: Vec<Id>,
    pub reversed: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Offset {
    pub distance: Expr,
    pub reverse: bool,
    pub closed: bool,
    pub members: Vec<OffsetMember>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Fillet {
    /// The original wire key is retained for native format compatibility.
    pub lines: [Id; 2],
    pub corner: Id,
    pub at_start: [bool; 2],
    pub radius: Expr,
    pub trim: [Id; 2],
    pub through: Id,
    pub arc: Id,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub source_curves: BTreeMap<Id, SketchCurve>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub arc_through: BTreeMap<Id, Id>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ChamferSize {
    Equal { distance: Expr },
    Distances { first: Expr, second: Expr },
    DistanceAngle { distance: Expr, angle: Expr },
}
impl ChamferSize {
    pub fn expressions(&self) -> Vec<&Expr> {
        match self {
            Self::Equal { distance } => vec![distance],
            Self::Distances { first, second } => vec![first, second],
            Self::DistanceAngle { distance, angle } => vec![distance, angle],
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Chamfer {
    pub lines: [Id; 2],
    pub corner: Id,
    pub at_start: [bool; 2],
    pub size: ChamferSize,
    pub trim: [Id; 2],
    pub member: Id,
}
/// Curve topology after saved corner operations, independent of evaluated coordinates.
pub fn effective_curve(sketch: &Sketch, key: &str) -> Result<SketchCurve> {
    let mut curve = sketch
        .members
        .get(key)
        .ok_or_else(|| invalid("Sketch member missing"))?
        .curve
        .clone();
    if let SketchCurve::Line { start, end } = &mut curve {
        for (lines, at_start, trim) in sketch
            .fillets
            .values()
            .map(|f| (&f.lines, &f.at_start, &f.trim))
            .chain(
                sketch
                    .chamfers
                    .values()
                    .map(|c| (&c.lines, &c.at_start, &c.trim)),
            )
        {
            for i in 0..2 {
                if lines[i] == key {
                    if at_start[i] {
                        *start = trim[i].clone();
                    } else {
                        *end = trim[i].clone();
                    }
                }
            }
        }
    }
    if let SketchCurve::Arc {
        start,
        through,
        end,
    } = &mut curve
    {
        for fillet in sketch.fillets.values() {
            if let Some(i) = fillet.lines.iter().position(|source| source == key) {
                if fillet.at_start[i] {
                    *start = fillet.trim[i].clone();
                } else {
                    *end = fillet.trim[i].clone();
                }
                if let Some(point) = fillet.arc_through.get(key) {
                    *through = point.clone();
                }
            }
        }
    }
    Ok(curve)
}
fn invalid(message: &str) -> CadError {
    CadError::Invalid(message.into())
}
fn p(sketch: &Sketch, key: &str) -> Result<Vector2<f64>> {
    sketch
        .points
        .get(key)
        .copied()
        .map(Vector2::from)
        .ok_or_else(|| invalid("Sketch operation has a missing point"))
}
fn put(sketch: &mut Sketch, key: &str, point: Vector2<f64>) -> Result<()> {
    if point.iter().any(|v| !v.is_finite()) {
        return Err(invalid("Sketch operation produced a non-finite point"));
    }
    sketch.points.insert(key.into(), [point.x, point.y]);
    Ok(())
}
fn cross(a: Vector2<f64>, b: Vector2<f64>) -> f64 {
    a.x * b.y - a.y * b.x
}
fn left(a: Vector2<f64>) -> Vector2<f64> {
    Vector2::new(-a.y, a.x)
}

pub fn project(curve: &Curve3, plane: &Plane) -> Result<Curve2> {
    let u = Vector3::from(plane.u);
    let v = Vector3::from(plane.v);
    let determinant = u.norm_squared() * v.norm_squared() - u.dot(&v).powi(2);
    if !determinant.is_finite() || determinant <= 1e-14 {
        return Err(invalid("Invalid projection plane"));
    }
    let control_points = curve
        .control_points
        .iter()
        .map(|point| {
            let d = Vector3::from(*point) - Vector3::from(plane.origin);
            [
                (d.dot(&u) * v.norm_squared() - d.dot(&v) * u.dot(&v)) / determinant,
                (d.dot(&v) * u.norm_squared() - d.dot(&u) * u.dot(&v)) / determinant,
            ]
        })
        .collect();
    let result = Curve2 {
        control_points,
        weights: curve.weights.clone(),
        knots: curve.knots.clone(),
        degree: curve.degree,
    };
    validate_nurbs(&result)?;
    if result
        .control_points
        .iter()
        .all(|p| Vector2::from(*p).metric_distance(&Vector2::from(result.control_points[0])) <= EPS)
    {
        return Err(invalid(
            "Selected edge projects to a point; choose another edge or sketch plane",
        ));
    }
    Ok(result)
}
pub fn validate_nurbs(curve: &Curve2) -> Result<()> {
    let n = curve.control_points.len();
    if n < 2
        || curve.degree == 0
        || curve.degree >= n
        || curve.weights.len() != n
        || curve.knots.len() != n + curve.degree + 1
        || curve
            .control_points
            .iter()
            .flatten()
            .any(|v| !v.is_finite())
        || curve.weights.iter().any(|v| !v.is_finite() || *v <= 0.)
        || curve.knots.iter().any(|v| !v.is_finite())
        || curve.knots.windows(2).any(|k| k[0] > k[1])
        || curve.knots[curve.degree] >= curve.knots[n]
        || curve.knots[..=curve.degree]
            .iter()
            .any(|t| *t != curve.knots[0])
        || curve.knots[n..].iter().any(|t| *t != curve.knots[n])
    {
        return Err(invalid(
            "Rational spline requires finite, clamped knots and positive weights",
        ));
    }
    Ok(())
}
pub fn nurbs_point(curve: &Curve2, t: f64) -> Result<[f64; 2]> {
    validate_nurbs(curve)?;
    let n = curve.control_points.len();
    let degree = curve.degree;
    let start = curve.knots[degree];
    let end = curve.knots[n];
    let t = start + (end - start) * t.clamp(0., 1.);
    let span = if t >= end {
        n - 1
    } else {
        (degree..n)
            .find(|i| t >= curve.knots[*i] && t < curve.knots[*i + 1])
            .ok_or_else(|| invalid("Invalid spline span"))?
    };
    let mut points = (span - degree..=span)
        .map(|i| {
            Vector3::new(
                curve.control_points[i][0] * curve.weights[i],
                curve.control_points[i][1] * curve.weights[i],
                curve.weights[i],
            )
        })
        .collect::<Vec<_>>();
    for level in 1..=degree {
        for j in (level..=degree).rev() {
            let i = span - degree + j;
            let denominator = curve.knots[i + degree + 1 - level] - curve.knots[i];
            let alpha = if denominator == 0. {
                0.
            } else {
                (t - curve.knots[i]) / denominator
            };
            points[j] = points[j - 1] * (1. - alpha) + points[j] * alpha;
        }
    }
    let p = points[degree];
    Ok([p.x / p.z, p.y / p.z])
}
pub fn insert_conversion(
    sketch: &mut Sketch,
    curve: Curve2,
    binding: Option<(NodeRef, TopologyRef)>,
    construction: bool,
) -> Result<Id> {
    insert_conversion_with_join(sketch, curve, binding, construction, true)
}
/// Linked dimension references have independent endpoints: a coincident free
/// center must not become a driven conversion control as a picking side effect.
pub fn insert_conversion_independent(
    sketch: &mut Sketch,
    curve: Curve2,
    binding: Option<(NodeRef, TopologyRef)>,
    construction: bool,
) -> Result<Id> {
    insert_conversion_with_join(sketch, curve, binding, construction, false)
}
fn insert_conversion_with_join(
    sketch: &mut Sketch,
    curve: Curve2,
    binding: Option<(NodeRef, TopologyRef)>,
    construction: bool,
    join_endpoints: bool,
) -> Result<Id> {
    validate_nurbs(&curve)?;
    let mut points = Vec::new();
    for (i, point) in curve.control_points.iter().enumerate() {
        // Only endpoints join existing sketch topology; interior controls stay independent.
        let key = if join_endpoints && (i == 0 || i + 1 == curve.control_points.len()) {
            sketch
                .points
                .iter()
                .find(|(_, p)| Vector2::from(**p).metric_distance(&Vector2::from(*point)) <= EPS)
                .map(|(key, _)| key.clone())
                .unwrap_or_else(id)
        } else {
            id()
        };
        sketch.points.insert(key.clone(), *point);
        points.push(key);
    }
    let member = id();
    let shape = if curve.degree == 1 && points.len() == 2 {
        SketchCurve::Line {
            start: points[0].clone(),
            end: points[1].clone(),
        }
    } else {
        SketchCurve::Nurbs {
            points: points.clone(),
            weights: curve.weights.clone(),
            knots: curve.knots.clone(),
            degree: curve.degree,
        }
    };
    sketch.members.insert(
        member.clone(),
        SketchMember {
            construction,
            curve: shape,
        },
    );
    if let Some((body, reference)) = binding {
        sketch.conversions.insert(
            member.clone(),
            Conversion {
                body,
                reference,
                curve,
                points,
                construction_orientation: true,
            },
        );
    }
    Ok(member)
}
pub fn driven_points(sketch: &Sketch) -> BTreeSet<Id> {
    sketch
        .conversions
        .values()
        .flat_map(|c| c.points.clone())
        .chain(
            sketch
                .offsets
                .values()
                .flat_map(|o| o.members.iter().flat_map(|m| m.points.clone())),
        )
        .chain(sketch.fillets.values().flat_map(|f| {
            f.trim
                .iter()
                .cloned()
                .chain(std::iter::once(f.through.clone()))
                .chain(f.arc_through.values().cloned())
        }))
        .chain(
            sketch
                .chamfers
                .values()
                .flat_map(|c| c.trim.iter().cloned()),
        )
        .chain(
            sketch
                .transforms
                .values()
                .flat_map(crate::sketch_transform::generated_points),
        )
        .chain(
            sketch
                .splines
                .values()
                .flat_map(crate::sketch_spline::Definition::driven_points),
        )
        .chain(
            sketch
                .conics
                .values()
                .flat_map(crate::sketch_conic::Definition::driven_points),
        )
        .chain(
            sketch
                .texts
                .values()
                .flat_map(crate::sketch_text::Definition::driven_points),
        )
        .chain(
            sketch
                .equations
                .values()
                .flat_map(crate::sketch_equation::Definition::driven_points),
        )
        .chain(
            sketch
                .block_instances
                .values()
                .flat_map(crate::sketch_blocks::Instance::driven_points),
        )
        .collect()
}
pub fn driven_member(sketch: &Sketch, member: &str) -> bool {
    sketch.conversions.contains_key(member)
        || sketch.equations.contains_key(member)
        || crate::sketch_text::operation_for(sketch, member).is_some()
        || crate::sketch_blocks::instance_for_member(sketch, member).is_some()
        || crate::sketch_conic::operation_for(sketch, member)
            .is_some_and(|(_, definition)| !definition.driven_points().is_empty())
        || matches!(
            sketch.splines.get(member),
            Some(crate::sketch_spline::Definition::Fit { .. })
        )
        || sketch
            .offsets
            .values()
            .any(|o| o.members.iter().any(|m| m.member == member))
        || sketch.fillets.values().any(|f| f.arc == member)
        || sketch.chamfers.values().any(|c| c.member == member)
        || sketch.transforms.values().any(|p| {
            crate::sketch_transform::outputs(p)
                .iter()
                .any(|id| id == member)
        })
}
fn endpoints(curve: &SketchCurve) -> Result<(&str, &str)> {
    match curve {
        SketchCurve::Line { start, end } | SketchCurve::Arc { start, end, .. } => Ok((start, end)),
        _ => Err(CadError::Unsupported(
            "Offset chains support lines and circular arcs; circles can be offset independently"
                .into(),
        )),
    }
}
pub fn offset_selection(sketch: &Sketch, selected: &[Id]) -> Result<(Vec<(Id, bool)>, bool)> {
    // Connectivity uses the trimmed topology even when the saved source retains sharp corners.
    let mut connected = sketch.clone();
    for key in sketch.members.keys() {
        connected.members.get_mut(key).unwrap().curve = effective_curve(sketch, key)?;
    }
    let sketch = &connected;
    if selected.is_empty() {
        return Err(invalid("Select sketch members to offset"));
    }
    if selected.len() == 1
        && matches!(
            sketch.members.get(&selected[0]).map(|m| &m.curve),
            Some(SketchCurve::Circle { .. })
        )
    {
        return Ok((vec![(selected[0].clone(), false)], true));
    }
    if selected.len() == 1
        && let Some(SketchCurve::Bezier { points } | SketchCurve::Nurbs { points, .. }) =
            sketch.members.get(&selected[0]).map(|m| &m.curve)
    {
        if points.len() < 2 {
            return Err(invalid("Offset spline needs at least two controls"));
        }
        let closed = (p(sketch, &points[0])? - p(sketch, points.last().unwrap())?).norm() <= EPS;
        return Ok((vec![(selected[0].clone(), false)], closed));
    }
    let mut pending = selected.iter().cloned().collect::<BTreeSet<_>>();
    let mut degree = BTreeMap::<Id, usize>::new();
    for key in &pending {
        let curve = &sketch
            .members
            .get(key)
            .ok_or_else(|| invalid("Offset source no longer exists"))?
            .curve;
        let (a, b) = endpoints(curve)?;
        *degree.entry(a.into()).or_default() += 1;
        *degree.entry(b.into()).or_default() += 1;
    }
    if degree.values().any(|n| *n > 2) {
        return Err(invalid(
            "Offset selection branches; select one contour or chain",
        ));
    }
    let first = pending.first().unwrap();
    let mut current = degree
        .iter()
        .find(|(_, n)| **n == 1)
        .map(|(p, _)| p.clone())
        .unwrap_or_else(|| endpoints(&sketch.members[first].curve).unwrap().0.into());
    let start = current.clone();
    let mut ordered = Vec::new();
    while !pending.is_empty() {
        let next = pending
            .iter()
            .find_map(|key| {
                let (a, b) = endpoints(&sketch.members[key].curve).ok()?;
                if a == current {
                    Some((key.clone(), false, b.to_string()))
                } else if b == current {
                    Some((key.clone(), true, a.to_string()))
                } else {
                    None
                }
            })
            .ok_or_else(|| {
                invalid("Select one connected offset chain; offset separate contours separately")
            })?;
        pending.remove(&next.0);
        ordered.push((next.0, next.1));
        current = next.2;
    }
    Ok((ordered, current == start))
}
pub fn add_offset(
    sketch: &mut Sketch,
    selected: &[Id],
    distance: Expr,
    reverse: bool,
    construction: bool,
) -> Result<Id> {
    let (ordered, closed) = offset_selection(sketch, selected)?;
    let mut members = Vec::new();
    let mut last = id();
    let first = last.clone();
    for (i, (source, reversed)) in ordered.iter().enumerate() {
        let member = id();
        let end = if closed && i + 1 == ordered.len() {
            first.clone()
        } else {
            id()
        };
        let points = match &sketch.members[source].curve {
            SketchCurve::Line { .. } => vec![last.clone(), end.clone()],
            SketchCurve::Arc { .. } => vec![last.clone(), id(), end.clone()],
            SketchCurve::Circle { .. } => vec![id()],
            SketchCurve::Bezier { points } | SketchCurve::Nurbs { points, .. } => {
                let mut mapped = BTreeMap::new();
                points
                    .iter()
                    .map(|key| mapped.entry(key.clone()).or_insert_with(id).clone())
                    .collect()
            }
            _ => {
                return Err(CadError::Unsupported(
                    "Select a line/arc chain or one exact supported spline carrier".into(),
                ));
            }
        };
        for point in &points {
            sketch.points.insert(point.clone(), [0.; 2]);
        }
        sketch.members.insert(
            member.clone(),
            SketchMember {
                construction,
                curve: SketchCurve::Line {
                    start: points[0].clone(),
                    end: points.last().unwrap().clone(),
                },
            },
        );
        members.push(OffsetMember {
            source: source.clone(),
            member,
            points,
            reversed: *reversed,
        });
        last = end;
    }
    let key = id();
    sketch.offsets.insert(
        key.clone(),
        Offset {
            distance,
            reverse,
            closed,
            members,
        },
    );
    Ok(key)
}
pub fn fillet_selection(
    sketch: &Sketch,
    selected: &[Id],
    corner: Option<&str>,
) -> Result<([Id; 2], Id, [bool; 2])> {
    let lines = if let Some(corner) = corner {
        sketch.members.iter().filter(|(_, m)| matches!(&m.curve, SketchCurve::Line { start, end } | SketchCurve::Arc { start, end, .. } if start == corner || end == corner)).map(|(id, _)| id.clone()).collect::<Vec<_>>()
    } else {
        selected.to_vec()
    };
    if lines.len() != 2 {
        return Err(invalid(
            "Select two joined lines/arcs or their shared corner for Sketch Fillet",
        ));
    }
    let (a, b) = endpoints(
        &sketch
            .members
            .get(&lines[0])
            .ok_or_else(|| invalid("Fillet source missing"))?
            .curve,
    )?;
    let (c, d) = endpoints(
        &sketch
            .members
            .get(&lines[1])
            .ok_or_else(|| invalid("Fillet source missing"))?
            .curve,
    )?;
    if !lines.iter().all(|k| {
        matches!(
            sketch.members[k].curve,
            SketchCurve::Line { .. } | SketchCurve::Arc { .. }
        )
    }) {
        return Err(CadError::Unsupported(
            "Sketch Fillet requires straight members or bounded circular arcs".into(),
        ));
    }
    let common = [a, b]
        .into_iter()
        .filter(|p| *p == c || *p == d)
        .collect::<Vec<_>>();
    if common.len() != 1 {
        return Err(invalid("Sketch Fillet needs one shared endpoint"));
    }
    if sketch
        .fillets
        .values()
        .any(|f| f.corner == common[0] && f.lines.iter().any(|k| lines.contains(k)))
        || sketch
            .chamfers
            .values()
            .any(|f| f.corner == common[0] && f.lines.iter().any(|k| lines.contains(k)))
    {
        return Err(invalid(
            "This corner already has a fillet or chamfer; edit that operation",
        ));
    }
    if lines.iter().any(|key| {
        matches!(sketch.members[key].curve, SketchCurve::Arc { .. })
            && sketch.fillets.values().any(|f| f.lines.contains(key))
    }) {
        return Err(invalid(
            "An arc can participate in one parametric corner fillet; edit or remove its existing fillet first",
        ));
    }
    Ok((
        [lines[0].clone(), lines[1].clone()],
        common[0].into(),
        [a == common[0], c == common[0]],
    ))
}
pub fn add_fillet(
    sketch: &mut Sketch,
    selected: &[Id],
    corner: Option<&str>,
    radius: Expr,
    construction: bool,
) -> Result<Id> {
    let (lines, corner, at_start) = fillet_selection(sketch, selected, corner)?;
    let arc = id();
    let trim = [id(), id()];
    let through = id();
    let source_curves = lines
        .iter()
        .map(|key| (key.clone(), sketch.members[key].curve.clone()))
        .collect();
    let arc_through: BTreeMap<Id, Id> = lines
        .iter()
        .filter(|key| matches!(sketch.members[*key].curve, SketchCurve::Arc { .. }))
        .map(|key| (key.clone(), id()))
        .collect();
    for point in trim.iter().chain(std::iter::once(&through)) {
        sketch.points.insert(point.clone(), [0.; 2]);
    }
    for point in arc_through.values() {
        sketch.points.insert(point.clone(), [0.; 2]);
    }
    sketch.members.insert(
        arc.clone(),
        SketchMember {
            construction,
            curve: SketchCurve::Arc {
                start: trim[0].clone(),
                through: through.clone(),
                end: trim[1].clone(),
            },
        },
    );
    let key = id();
    sketch.fillets.insert(
        key.clone(),
        Fillet {
            lines,
            corner,
            at_start,
            radius,
            trim,
            through,
            arc,
            source_curves,
            arc_through,
        },
    );
    Ok(key)
}

pub fn add_chamfer(
    sketch: &mut Sketch,
    selected: &[Id],
    corner: Option<&str>,
    size: ChamferSize,
    construction: bool,
) -> Result<Id> {
    let (lines, corner, at_start) = fillet_selection(sketch, selected, corner)?;
    if !lines
        .iter()
        .all(|key| matches!(sketch.members[key].curve, SketchCurve::Line { .. }))
    {
        return Err(CadError::Unsupported(
            "Sketch Chamfer currently requires two joined straight members".into(),
        ));
    }
    let member = id();
    let trim = [id(), id()];
    for key in &trim {
        sketch.points.insert(key.clone(), [0.; 2]);
    }
    sketch.members.insert(
        member.clone(),
        SketchMember {
            construction,
            curve: SketchCurve::Line {
                start: trim[0].clone(),
                end: trim[1].clone(),
            },
        },
    );
    let key = id();
    sketch.chamfers.insert(
        key.clone(),
        Chamfer {
            lines,
            corner,
            at_start,
            size,
            trim,
            member,
        },
    );
    Ok(key)
}
fn chamfer_geometry(doc: &Document, owner: &str, sketch: &mut Sketch, c: &Chamfer) -> Result<()> {
    let corner = p(sketch, &c.corner)?;
    let mut directions = Vec::new();
    let mut lengths = Vec::new();
    for i in 0..2 {
        let Some(SketchMember {
            curve: SketchCurve::Line { start, end },
            ..
        }) = sketch.members.get(&c.lines[i])
        else {
            return Err(invalid("Sketch chamfer source must be a straight member"));
        };
        let delta = p(sketch, if c.at_start[i] { end } else { start })? - corner;
        lengths.push(delta.norm());
        directions.push(
            delta
                .try_normalize(EPS)
                .ok_or_else(|| invalid("Chamfer source collapsed"))?,
        );
    }
    let theta = directions[0].dot(&directions[1]).clamp(-1., 1.).acos();
    if theta <= 1e-6 || std::f64::consts::PI - theta <= 1e-6 {
        return Err(invalid("Sketch chamfer requires a non-collinear corner"));
    }
    let length = |e| expression::value(doc, owner, e, Dimension::Length);
    let distances = match &c.size {
        ChamferSize::Equal { distance } => [length(distance)?; 2],
        ChamferSize::Distances { first, second } => [length(first)?, length(second)?],
        ChamferSize::DistanceAngle { distance, angle } => {
            let alpha = expression::value(doc, owner, angle, Dimension::Angle)?;
            if alpha <= 1e-6 || alpha + theta >= std::f64::consts::PI - 1e-6 {
                return Err(invalid("Chamfer angle must lie inside the selected corner"));
            }
            let first = length(distance)?;
            [first, first * alpha.sin() / (theta + alpha).sin()]
        }
    };
    for i in 0..2 {
        if !distances[i].is_finite() || distances[i] <= EPS || distances[i] >= lengths[i] - EPS {
            return Err(invalid(
                "Chamfer distance must be positive and shorter than its adjacent member",
            ));
        }
        put(sketch, &c.trim[i], corner + directions[i] * distances[i])?;
        let SketchCurve::Line { start, end } =
            &mut sketch.members.get_mut(&c.lines[i]).unwrap().curve
        else {
            unreachable!()
        };
        if c.at_start[i] {
            *start = c.trim[i].clone();
        } else {
            *end = c.trim[i].clone();
        }
    }
    sketch
        .members
        .get_mut(&c.member)
        .ok_or_else(|| invalid("Chamfer output missing"))?
        .curve = SketchCurve::Line {
        start: c.trim[0].clone(),
        end: c.trim[1].clone(),
    };
    Ok(())
}

#[derive(Clone)]
enum Segment {
    Line {
        a: Vector2<f64>,
        b: Vector2<f64>,
    },
    Arc {
        a: Vector2<f64>,
        b: Vector2<f64>,
        center: Vector2<f64>,
        radius: f64,
        sweep: f64,
    },
}
impl Segment {
    fn ends(&self) -> (Vector2<f64>, Vector2<f64>) {
        match self {
            Self::Line { a, b } | Self::Arc { a, b, .. } => (*a, *b),
        }
    }
    fn intersects(&self, other: &Self) -> Vec<Vector2<f64>> {
        let line_circle = |a: Vector2<f64>, b: Vector2<f64>, c: Vector2<f64>, r: f64| {
            let d = b - a;
            let q = a - c;
            let aa = d.norm_squared();
            let bb = 2. * q.dot(&d);
            let cc = q.norm_squared() - r * r;
            let disc = bb * bb - 4. * aa * cc;
            if aa <= EPS * EPS || disc < -EPS * EPS {
                vec![]
            } else {
                [
                    (-bb + disc.max(0.).sqrt()) / (2. * aa),
                    (-bb - disc.max(0.).sqrt()) / (2. * aa),
                ]
                .map(|t| a + d * t)
                .to_vec()
            }
        };
        match (self, other) {
            (Self::Line { a, b }, Self::Line { a: c, b: d }) => {
                let u = *b - *a;
                let v = *d - *c;
                let denominator = cross(u, v);
                if denominator.abs() <= 1e-12 * u.norm() * v.norm() {
                    if (b - c).norm() <= EPS {
                        vec![(*b + *c) * 0.5]
                    } else {
                        vec![]
                    }
                } else {
                    vec![*a + u * (cross(*c - *a, v) / denominator)]
                }
            }
            (Self::Line { a, b }, Self::Arc { center, radius, .. })
            | (Self::Arc { center, radius, .. }, Self::Line { a, b }) => {
                line_circle(*a, *b, *center, *radius)
            }
            (
                Self::Arc {
                    center: a,
                    radius: ra,
                    b,
                    ..
                },
                Self::Arc {
                    center: c,
                    radius: rb,
                    a: d,
                    ..
                },
            ) => {
                let delta = *c - *a;
                let distance = delta.norm();
                if distance <= EPS {
                    return if (b - d).norm() <= EPS {
                        vec![(*b + *d) * 0.5]
                    } else {
                        vec![]
                    };
                }
                let x = (ra * ra - rb * rb + distance * distance) / (2. * distance);
                let h = ra * ra - x * x;
                if h < -EPS * EPS {
                    vec![]
                } else {
                    let u = delta / distance;
                    let base = *a + u * x;
                    vec![
                        base + left(u) * h.max(0.).sqrt(),
                        base - left(u) * h.max(0.).sqrt(),
                    ]
                }
            }
        }
    }
}
fn offset_geometry(
    doc: &Document,
    owner: &str,
    sketch: &mut Sketch,
    operation: &Offset,
) -> Result<()> {
    if independent_spline_offset(doc, owner, sketch, operation)? {
        return Ok(());
    }
    let distance = expression::value(doc, owner, &operation.distance, Dimension::Length)?;
    if !distance.is_finite() || distance <= EPS {
        return Err(invalid("Offset distance must be positive"));
    }
    let mut segments = Vec::new();
    for member in &operation.members {
        let source = &sketch
            .members
            .get(&member.source)
            .ok_or_else(|| invalid("Offset source missing"))?
            .curve;
        if let SketchCurve::Circle { center, radius } = source {
            let radius = expression::value(doc, owner, radius, Dimension::Length)?
                + if operation.reverse {
                    -distance
                } else {
                    distance
                };
            if radius <= EPS {
                return Err(invalid("Offset distance collapses the circle"));
            }
            let center = p(sketch, center)?;
            put(sketch, &member.points[0], center)?;
            sketch
                .members
                .get_mut(&member.member)
                .ok_or_else(|| invalid("Offset output missing"))?
                .curve = SketchCurve::Circle {
                center: member.points[0].clone(),
                radius: Expr(format!("{radius} mm")),
            };
            return Ok(());
        }
        let (a, b) = endpoints(source)?;
        let (a, b) = if member.reversed { (b, a) } else { (a, b) };
        let (a, b) = (p(sketch, a)?, p(sketch, b)?);
        segments.push(match source {
            SketchCurve::Line { .. } => Segment::Line { a, b },
            SketchCurve::Arc { through, .. } => {
                let middle = p(sketch, through)?;
                let (center, radius, _, sweep) =
                    sketch_geometry::arc_frame([a.x, a.y], [middle.x, middle.y], [b.x, b.y])?;
                Segment::Arc {
                    a,
                    b,
                    center: Vector2::from(center),
                    radius,
                    sweep,
                }
            }
            _ => {
                return Err(CadError::Unsupported(
                    "Offset supports straight and circular members".into(),
                ));
            }
        });
    }
    let area = segments
        .iter()
        .map(|s| {
            let (a, b) = s.ends();
            match s {
                Segment::Line { .. } => cross(a, b),
                Segment::Arc {
                    center,
                    radius,
                    sweep,
                    ..
                } => cross(*center, b - a) + radius * radius * sweep,
            }
        })
        .sum::<f64>();
    let signed = distance
        * if operation.closed && area > 0. {
            -1.
        } else {
            1.
        }
        * if operation.reverse { -1. } else { 1. };
    for segment in &mut segments {
        match segment {
            Segment::Line { a, b } => {
                let direction = (*b - *a)
                    .try_normalize(EPS)
                    .ok_or_else(|| invalid("Cannot offset a collapsed line"))?;
                let delta = left(direction) * signed;
                *a += delta;
                *b += delta;
            }
            Segment::Arc {
                a,
                b,
                center,
                radius,
                sweep,
            } => {
                let new_radius = *radius - signed * sweep.signum();
                if new_radius <= EPS {
                    return Err(invalid("Offset distance collapses an arc"));
                }
                *a = *center + (*a - *center) * (new_radius / *radius);
                *b = *center + (*b - *center) * (new_radius / *radius);
                *radius = new_radius;
            }
        }
    }
    let mut starts = segments.iter().map(|s| s.ends().0).collect::<Vec<_>>();
    let mut ends = segments.iter().map(|s| s.ends().1).collect::<Vec<_>>();
    let joins = if operation.closed {
        segments.len()
    } else {
        segments.len().saturating_sub(1)
    };
    for i in 0..joins {
        let j = (i + 1) % segments.len();
        let target = (ends[i] + starts[j]) * 0.5;
        let joint=segments[i].intersects(&segments[j]).into_iter().min_by(|a,b| (a-target).norm_squared().total_cmp(&(b-target).norm_squared())).ok_or_else(|| invalid("Offset members do not intersect at a corner; reduce the distance or offset them separately"))?;
        ends[i] = joint;
        starts[j] = joint;
    }
    for (i, member) in operation.members.iter().enumerate() {
        put(sketch, &member.points[0], starts[i])?;
        put(sketch, member.points.last().unwrap(), ends[i])?;
        let curve = match &segments[i] {
            Segment::Line { a, b } => {
                if (ends[i] - starts[i]).dot(&(b - a)) <= EPS * EPS {
                    return Err(invalid(
                        "Offset reverses or collapses a member; reduce distance",
                    ));
                }
                SketchCurve::Line {
                    start: member.points[0].clone(),
                    end: member.points[1].clone(),
                }
            }
            Segment::Arc {
                center,
                radius,
                sweep,
                ..
            } => {
                let start = (starts[i].y - center.y).atan2(starts[i].x - center.x);
                let end = (ends[i].y - center.y).atan2(ends[i].x - center.x);
                let angle = if *sweep > 0. {
                    (end - start).rem_euclid(std::f64::consts::TAU)
                } else {
                    -(start - end).rem_euclid(std::f64::consts::TAU)
                };
                if angle.abs() <= EPS || (angle - sweep).abs() > std::f64::consts::PI {
                    return Err(invalid("Offset changes an arc's branch; reduce distance"));
                }
                let middle = *center
                    + Vector2::new((start + angle * 0.5).cos(), (start + angle * 0.5).sin())
                        * *radius;
                put(sketch, &member.points[1], middle)?;
                SketchCurve::Arc {
                    start: member.points[0].clone(),
                    through: member.points[1].clone(),
                    end: member.points[2].clone(),
                }
            }
        };
        sketch
            .members
            .get_mut(&member.member)
            .ok_or_else(|| invalid("Offset output missing"))?
            .curve = curve;
    }
    Ok(())
}
#[derive(Clone)]
enum FilletCarrier {
    Line {
        a: Vector2<f64>,
        b: Vector2<f64>,
    },
    Arc {
        center: Vector2<f64>,
        radius: f64,
        start: f64,
        sweep: f64,
    },
}
impl FilletCarrier {
    fn loci(&self, radius: f64) -> Vec<Segment> {
        match self {
            Self::Line { a, b } => {
                let direction = (*b - *a).normalize();
                [-1., 1.]
                    .into_iter()
                    .map(|side| Segment::Line {
                        a: *a + left(direction) * radius * side,
                        b: *b + left(direction) * radius * side,
                    })
                    .collect()
            }
            Self::Arc {
                center,
                radius: source,
                ..
            } => [source + radius, (source - radius).abs()]
                .into_iter()
                .filter(|r| *r > EPS)
                .map(|r| Segment::Arc {
                    a: *center,
                    b: *center,
                    center: *center,
                    radius: r,
                    sweep: 0.,
                })
                .collect(),
        }
    }
    fn contacts(&self, center: Vector2<f64>, radius: f64) -> Vec<Vector2<f64>> {
        match self {
            Self::Line { a, b } => {
                let d = *b - *a;
                vec![*a + d * ((center - *a).dot(&d) / d.norm_squared())]
            }
            Self::Arc {
                center: source,
                radius: r,
                ..
            } => {
                let Some(d) = (center - *source).try_normalize(EPS) else {
                    return vec![];
                };
                [-1., 1.]
                    .into_iter()
                    .map(|side| *source + d * (*r * side))
                    .filter(|p| ((center - *p).norm() - radius).abs() <= EPS)
                    .collect()
            }
        }
    }
    fn progress(&self, point: Vector2<f64>) -> f64 {
        match self {
            Self::Line { a, b } => (point - *a).dot(&(*b - *a)) / (*b - *a).norm_squared(),
            Self::Arc {
                center,
                start,
                sweep,
                ..
            } => {
                let angle = (point.y - center.y).atan2(point.x - center.x);
                let delta = if *sweep > 0. {
                    (angle - start).rem_euclid(std::f64::consts::TAU)
                } else {
                    (start - angle).rem_euclid(std::f64::consts::TAU)
                };
                delta / sweep.abs()
            }
        }
    }
    fn at(&self, t: f64) -> Vector2<f64> {
        match self {
            Self::Line { a, b } => *a + (*b - *a) * t,
            Self::Arc {
                center,
                radius,
                start,
                sweep,
            } => {
                *center
                    + Vector2::new((start + sweep * t).cos(), (start + sweep * t).sin()) * (*radius)
            }
        }
    }
    fn tangent(&self, point: Vector2<f64>) -> Vector2<f64> {
        match self {
            Self::Line { a, b } => (*b - *a).normalize(),
            Self::Arc { center, sweep, .. } => left((point - *center).normalize()) * sweep.signum(),
        }
    }
    fn length(&self) -> f64 {
        match self {
            Self::Line { a, b } => (*b - *a).norm(),
            Self::Arc { radius, sweep, .. } => radius * sweep.abs(),
        }
    }
}
fn fillet_geometry(
    doc: &Document,
    owner: &str,
    sketch: &mut Sketch,
    operation: &Fillet,
) -> Result<()> {
    let radius = expression::value(doc, owner, &operation.radius, Dimension::Length)?;
    if !radius.is_finite() || radius <= EPS {
        return Err(invalid("Fillet radius must be finite and positive"));
    }
    let mut carriers = Vec::new();
    for key in &operation.lines {
        let curve = &sketch
            .members
            .get(key)
            .ok_or_else(|| invalid("Fillet source missing"))?
            .curve;
        let carrier = match curve {
            SketchCurve::Line { start, end } => {
                let (a, b) = (p(sketch, start)?, p(sketch, end)?);
                let length = (b - a).norm();
                if !length.is_finite() || length <= EPS {
                    return Err(invalid(
                        "Fillet source line must have a finite, nonzero length",
                    ));
                }
                FilletCarrier::Line { a, b }
            }
            SketchCurve::Arc {
                start,
                through,
                end,
            } => {
                let (center, radius, start, sweep) = sketch_geometry::arc_frame(
                    p(sketch, start)?.into(),
                    p(sketch, through)?.into(),
                    p(sketch, end)?.into(),
                )?;
                FilletCarrier::Arc {
                    center: Vector2::from(center),
                    radius,
                    start,
                    sweep,
                }
            }
            _ => {
                return Err(invalid(
                    "Fillet requires straight members or bounded circular arcs",
                ));
            }
        };
        carriers.push(carrier);
    }
    let mut candidates = Vec::new();
    for a in carriers[0].loci(radius) {
        for b in carriers[1].loci(radius) {
            if matches!((&a,&b),(Segment::Arc{center:a,..},Segment::Arc{center:b,..}) if (*a-*b).norm()<=EPS)
            {
                continue;
            }
            for center in a.intersects(&b) {
                if center.iter().any(|v| !v.is_finite()) {
                    continue;
                }
                for first in carriers[0].contacts(center, radius) {
                    for second in carriers[1].contacts(center, radius) {
                        let contacts = [first, second];
                        let mut progress = [0.; 2];
                        let mut cost = 0.;
                        let mut valid = true;
                        for i in 0..2 {
                            progress[i] = carriers[i].progress(contacts[i]);
                            let cut = if operation.at_start[i] {
                                progress[i]
                            } else {
                                1. - progress[i]
                            };
                            if progress[i] <= EPS / carriers[i].length()
                                || progress[i] >= 1. - EPS / carriers[i].length()
                                || cut <= 0.
                            {
                                valid = false;
                                break;
                            }
                            cost += cut * carriers[i].length();
                        }
                        if !valid || (first - second).norm() <= EPS {
                            continue;
                        }
                        let r0 = (first - center) / radius;
                        let r1 = (second - center) / radius;
                        let sweep = cross(r0, r1).atan2(r0.dot(&r1));
                        if sweep.abs() <= 1e-8 || sweep.abs() >= std::f64::consts::PI - 1e-8 {
                            continue;
                        }
                        let away = std::array::from_fn::<_, 2, _>(|i| {
                            carriers[i].tangent(contacts[i])
                                * if operation.at_start[i] { 1. } else { -1. }
                        });
                        if left(r0).dot(&(-away[0])) * sweep.signum() < 1. - 1e-8
                            || left(r1).dot(&away[1]) * sweep.signum() < 1. - 1e-8
                        {
                            continue;
                        }
                        let start = r0.y.atan2(r0.x);
                        let middle = center
                            + Vector2::new((start + sweep / 2.).cos(), (start + sweep / 2.).sin())
                                * radius;
                        candidates.push((cost, contacts, progress, middle));
                    }
                }
            }
        }
    }
    let Some((_, contacts, progress, middle)) =
        candidates.into_iter().min_by(|a, b| a.0.total_cmp(&b.0))
    else {
        return Err(invalid(
            "No bounded tangent corner fillet fits this radius; reduce the radius or repair the selected corner",
        ));
    };
    for i in 0..2 {
        put(sketch, &operation.trim[i], contacts[i])?;
        let old = sketch.members[&operation.lines[i]].curve.clone();
        let curve = match old {
            SketchCurve::Line { mut start, mut end } => {
                if operation.at_start[i] {
                    start = operation.trim[i].clone();
                } else {
                    end = operation.trim[i].clone();
                }
                SketchCurve::Line { start, end }
            }
            SketchCurve::Arc {
                mut start, mut end, ..
            } => {
                let through = operation
                    .arc_through
                    .get(&operation.lines[i])
                    .ok_or_else(|| invalid("Arc fillet is missing its stable trimmed-arc handle"))?
                    .clone();
                let t = if operation.at_start[i] {
                    (1. + progress[i]) / 2.
                } else {
                    progress[i] / 2.
                };
                put(sketch, &through, carriers[i].at(t))?;
                if operation.at_start[i] {
                    start = operation.trim[i].clone();
                } else {
                    end = operation.trim[i].clone();
                }
                SketchCurve::Arc {
                    start,
                    through,
                    end,
                }
            }
            _ => unreachable!(),
        };
        sketch.members.get_mut(&operation.lines[i]).unwrap().curve = curve;
    }
    put(sketch, &operation.through, middle)?;
    sketch
        .members
        .get_mut(&operation.arc)
        .ok_or_else(|| invalid("Fillet arc missing"))?
        .curve = SketchCurve::Arc {
        start: operation.trim[0].clone(),
        through: operation.through.clone(),
        end: operation.trim[1].clone(),
    };
    Ok(())
}
pub fn derive(doc: &Document, owner: &str, sketch: &mut Sketch) -> Result<()> {
    let prepared = crate::sketch_blocks::prepare(doc, owner, sketch)?;
    derive_prepared(doc, owner, sketch, &prepared)
}
pub fn derive_prepared(
    doc: &Document,
    owner: &str,
    sketch: &mut Sketch,
    prepared: &crate::sketch_blocks::Prepared,
) -> Result<()> {
    validate(sketch)?;
    validate_active_sources(sketch)?;
    for operation in sketch.fillets.values() {
        for (key, curve) in &operation.source_curves {
            if matches!(curve, SketchCurve::Arc { .. }) {
                sketch
                    .members
                    .get_mut(key)
                    .ok_or_else(|| invalid("Fillet arc source missing"))?
                    .curve = curve.clone();
            }
        }
    }
    // Rebuild trimmed endpoints from the sharp-corner definitions on every pass.
    for (lines, at_start, corner) in sketch
        .fillets
        .values()
        .map(|f| (&f.lines, &f.at_start, &f.corner))
        .chain(
            sketch
                .chamfers
                .values()
                .map(|c| (&c.lines, &c.at_start, &c.corner)),
        )
    {
        for (i, key) in lines.iter().enumerate() {
            if let Some(SketchMember {
                curve: SketchCurve::Line { start, end },
                ..
            }) = sketch.members.get_mut(key)
            {
                if at_start[i] {
                    *start = corner.clone();
                } else {
                    *end = corner.clone();
                }
            }
        }
    }
    let mut converted_points = BTreeMap::<Id, Vector2<f64>>::new();
    for (member, conversion) in sketch.conversions.clone() {
        validate_nurbs(&conversion.curve)?;
        if conversion.points.len() != conversion.curve.control_points.len() {
            return Err(invalid(
                "Converted edge changed its control topology; replace the source reference",
            ));
        }
        for (key, point) in conversion
            .points
            .iter()
            .zip(&conversion.curve.control_points)
        {
            let point = Vector2::from(*point);
            if converted_points
                .get(key)
                .is_some_and(|old| (old - point).norm() > EPS)
            {
                return Err(invalid(
                    "Converted edge endpoints no longer coincide; replace the source references",
                ));
            }
            converted_points.insert(key.clone(), point);
            put(sketch, key, point)?;
        }
        let record = sketch
            .members
            .get_mut(&member)
            .ok_or_else(|| invalid("Converted member missing"))?;
        record.curve = if conversion.curve.degree == 1 && conversion.points.len() == 2 {
            SketchCurve::Line {
                start: conversion.points[0].clone(),
                end: conversion.points[1].clone(),
            }
        } else {
            SketchCurve::Nurbs {
                points: conversion.points,
                weights: conversion.curve.weights,
                knots: conversion.curve.knots,
                degree: conversion.curve.degree,
            }
        };
    }
    let operations = modifier_ids(sketch);
    let mut done = BTreeSet::new();
    let mut visiting = BTreeSet::new();
    fn visit(
        doc: &Document,
        owner: &str,
        sketch: &mut Sketch,
        key: &str,
        done: &mut BTreeSet<Id>,
        visiting: &mut BTreeSet<Id>,
        prepared: &crate::sketch_blocks::Prepared,
    ) -> Result<()> {
        if done.contains(key) {
            return Ok(());
        }
        if !visiting.insert(key.into()) {
            return Err(invalid("Cyclic sketch modifier dependency"));
        }
        let sources = modifier_sources(sketch, key);
        let points = modifier_reference_points(sketch, key);
        let dependencies = modifier_ids(sketch)
            .into_iter()
            .filter(|other| {
                other != key
                    && (modifier_members(sketch, other)
                        .iter()
                        .any(|id| sources.contains(id))
                        || modifier_points(sketch, other)
                            .iter()
                            .any(|id| points.contains(id))
                        || ((sketch.offsets.contains_key(key)
                            || sketch.transforms.contains_key(key))
                            && modified_lines(sketch, other)
                                .iter()
                                .any(|id| sources.contains(id))))
            })
            .collect::<Vec<_>>();
        for dependency in dependencies {
            visit(doc, owner, sketch, &dependency, done, visiting, prepared)?;
        }
        if let Some(operation) = sketch.offsets.get(key).cloned() {
            offset_geometry(doc, owner, sketch, &operation)?;
        } else if let Some(operation) = sketch.fillets.get(key).cloned() {
            fillet_geometry(doc, owner, sketch, &operation)?;
        } else if let Some(operation) = sketch.chamfers.get(key).cloned() {
            chamfer_geometry(doc, owner, sketch, &operation)?;
        } else if let Some(operation) = sketch.transforms.get(key).cloned() {
            crate::sketch_transform::derive(doc, owner, sketch, &operation)?;
        } else if let Some(definition) = sketch.splines.get(key).cloned() {
            crate::sketch_spline::derive(doc, owner, sketch, key, &definition)?;
        } else if let Some(definition) = sketch.conics.get(key).cloned() {
            crate::sketch_conic::derive(doc, owner, sketch, key, &definition)?;
        } else if let Some(definition) = sketch.texts.get(key).cloned() {
            crate::sketch_text::derive(doc, owner, sketch, &definition)?;
        } else if let Some(definition) = sketch.equations.get(key).cloned() {
            crate::sketch_equation::derive(doc, owner, sketch, key, &definition)?;
        } else if sketch.block_instances.contains_key(key) {
            crate::sketch_blocks::derive_prepared(doc, owner, sketch, key, prepared)?;
        } else {
            return Err(invalid("Sketch modifier is missing"));
        }
        visiting.remove(key);
        done.insert(key.into());
        Ok(())
    }
    for key in operations {
        visit(doc, owner, sketch, &key, &mut done, &mut visiting, prepared)?;
    }
    Ok(())
}

pub fn modifier_ids(sketch: &Sketch) -> Vec<Id> {
    sketch
        .offsets
        .keys()
        .chain(sketch.fillets.keys())
        .chain(sketch.chamfers.keys())
        .chain(sketch.transforms.keys())
        .chain(sketch.splines.keys())
        .chain(sketch.conics.keys())
        .chain(sketch.texts.keys())
        .chain(sketch.equations.keys())
        .chain(sketch.block_instances.keys())
        .cloned()
        .collect()
}
pub fn modifier_members(sketch: &Sketch, key: &str) -> Vec<Id> {
    if let Some(o) = sketch.offsets.get(key) {
        o.members.iter().map(|m| m.member.clone()).collect()
    } else if let Some(f) = sketch.fillets.get(key) {
        vec![f.arc.clone()]
    } else if let Some(c) = sketch.chamfers.get(key) {
        vec![c.member.clone()]
    } else if let Some(p) = sketch.transforms.get(key) {
        crate::sketch_transform::outputs(p)
    } else if sketch.splines.contains_key(key) {
        vec![key.into()]
    } else if let Some(definition) = sketch.conics.get(key) {
        definition.outputs(key)
    } else if let Some(definition) = sketch.texts.get(key) {
        definition.outputs()
    } else if let Some(definition) = sketch.equations.get(key) {
        definition.outputs(key)
    } else if let Some(instance) = sketch.block_instances.get(key) {
        instance.outputs()
    } else {
        vec![]
    }
}
pub fn modifier_points(sketch: &Sketch, key: &str) -> Vec<Id> {
    if let Some(o) = sketch.offsets.get(key) {
        o.members.iter().flat_map(|m| m.points.clone()).collect()
    } else if let Some(f) = sketch.fillets.get(key) {
        f.trim
            .iter()
            .cloned()
            .chain(std::iter::once(f.through.clone()))
            .chain(f.arc_through.values().cloned())
            .collect()
    } else if let Some(c) = sketch.chamfers.get(key) {
        c.trim.to_vec()
    } else if let Some(p) = sketch.transforms.get(key) {
        crate::sketch_transform::generated_points(p)
    } else if let Some(s) = sketch.splines.get(key) {
        s.driven_points()
    } else if let Some(definition) = sketch.conics.get(key) {
        definition.driven_points()
    } else if let Some(definition) = sketch.texts.get(key) {
        definition.driven_points()
    } else if let Some(definition) = sketch.equations.get(key) {
        definition.driven_points()
    } else if let Some(instance) = sketch.block_instances.get(key) {
        instance.driven_points()
    } else {
        vec![]
    }
}
pub(crate) fn modifier_sources(sketch: &Sketch, key: &str) -> Vec<Id> {
    if let Some(o) = sketch.offsets.get(key) {
        o.members.iter().map(|m| m.source.clone()).collect()
    } else if let Some(f) = sketch.fillets.get(key) {
        f.lines.to_vec()
    } else if let Some(c) = sketch.chamfers.get(key) {
        c.lines.to_vec()
    } else if let Some(p) = sketch.transforms.get(key) {
        crate::sketch_transform::sources(p)
    } else {
        vec![]
    }
}
pub fn modifier_reference_points(sketch: &Sketch, key: &str) -> Vec<Id> {
    if let Some(f) = sketch.fillets.get(key) {
        f.source_curves
            .values()
            .flat_map(|curve| curve.points().into_iter().map(str::to_string))
            .chain(std::iter::once(f.corner.clone()))
            .collect()
    } else if let Some(chamfer) = sketch.chamfers.get(key) {
        vec![chamfer.corner.clone()]
    } else if let Some(p) = sketch.transforms.get(key) {
        crate::sketch_transform::reference_points(p)
    } else if let Some(s) = sketch.splines.get(key) {
        s.fit_points()
    } else if let Some(definition) = sketch.conics.get(key) {
        definition.source_points()
    } else if let Some(definition) = sketch.texts.get(key) {
        vec![definition.anchor.clone()]
    } else if let Some(definition) = sketch.equations.get(key) {
        definition.source_points()
    } else if let Some(instance) = sketch.block_instances.get(key) {
        vec![instance.anchor.clone()]
    } else {
        vec![]
    }
}

/// Current activity is derived only from saved suppression flags and native
/// output identities. An empty dynamic mirror still has an active authoring axis.
pub(crate) fn modifier_active(sketch: &Sketch, key: &str, suppressed: &BTreeSet<Id>) -> bool {
    if let Some(instance) = sketch.block_instances.get(key) {
        !instance.suppressed
    } else if let Some(pattern) = sketch.transforms.get(key) {
        pattern
            .instances
            .iter()
            .any(|instance| !instance.suppressed)
            && (pattern.dynamic
                || modifier_members(sketch, key)
                    .iter()
                    .any(|member| !suppressed.contains(member)))
    } else {
        modifier_members(sketch, key)
            .iter()
            .any(|member| !suppressed.contains(member))
    }
}

/// Inputs used by current consumers. Suppressed instance-specific placements do
/// not make unrelated centers active or invalidate an active sibling instance.
pub(crate) fn active_modifier_reference_points(
    sketch: &Sketch,
    key: &str,
    suppressed: &BTreeSet<Id>,
) -> Vec<Id> {
    if !modifier_active(sketch, key, suppressed) {
        return vec![];
    }
    if let Some(pattern) = sketch.transforms.get(key) {
        pattern
            .transform
            .reference_points()
            .into_iter()
            .chain(
                pattern
                    .instances
                    .iter()
                    .filter(|instance| !instance.suppressed)
                    .flat_map(|instance| {
                        instance.points.keys().cloned().chain(
                            instance
                                .placement
                                .as_ref()
                                .into_iter()
                                .filter_map(|placement| match &placement.center {
                                    crate::sketch_transform::Center::Point { point } => {
                                        Some(point.clone())
                                    }
                                    _ => None,
                                }),
                        )
                    }),
            )
            .collect()
    } else {
        modifier_reference_points(sketch, key)
    }
}

/// Runtime admission, after configuration materialization, keeps saved latent
/// definitions intact while preventing active output from using suppressed sources.
fn validate_active_sources(sketch: &Sketch) -> Result<()> {
    let suppressed_members = crate::sketch_transform::suppressed_members(sketch);
    let suppressed_points = crate::sketch_transform::suppressed_points(sketch);
    for key in modifier_ids(sketch) {
        if !modifier_active(sketch, &key, &suppressed_members) {
            continue;
        }
        let own_members = modifier_members(sketch, &key)
            .into_iter()
            .collect::<BTreeSet<_>>();
        let own_points = modifier_points(sketch, &key)
            .into_iter()
            .collect::<BTreeSet<_>>();
        if let Some(source) = modifier_sources(sketch, &key)
            .into_iter()
            .find(|source| suppressed_members.contains(source) && !own_members.contains(source))
        {
            return Err(invalid(&format!(
                "Active sketch operation {key} references suppressed member {source}; restore or replace that source"
            )));
        }
        if let Some(point) = active_modifier_reference_points(sketch, &key, &suppressed_members)
            .into_iter()
            .find(|point| suppressed_points.contains(point) && !own_points.contains(point))
        {
            return Err(invalid(&format!(
                "Active sketch operation {key} references suppressed point {point}; restore or replace that source"
            )));
        }
    }
    Ok(())
}
fn modified_lines(sketch: &Sketch, key: &str) -> Vec<Id> {
    if let Some(f) = sketch.fillets.get(key) {
        f.lines.to_vec()
    } else if let Some(c) = sketch.chamfers.get(key) {
        c.lines.to_vec()
    } else {
        vec![]
    }
}

/// Remove an editing operation and its generated geometry, restoring sharp endpoints.
pub fn remove_modifier(sketch: &mut Sketch, key: &str) {
    let mut members = BTreeSet::new();
    let mut points = BTreeSet::new();
    let mut block_anchor = None;
    if let Some(offset) = sketch.offsets.remove(key) {
        for m in offset.members {
            members.insert(m.member);
            points.extend(m.points);
        }
    }
    if let Some(fillet) = sketch.fillets.remove(key) {
        for (key, curve) in &fillet.source_curves {
            if matches!(curve, SketchCurve::Arc { .. })
                && let Some(member) = sketch.members.get_mut(key)
            {
                member.curve = curve.clone();
            }
        }
        for (i, key) in fillet.lines.iter().enumerate() {
            if let Some(SketchMember {
                curve: SketchCurve::Line { start, end },
                ..
            }) = sketch.members.get_mut(key)
            {
                if fillet.at_start[i] {
                    *start = fillet.corner.clone();
                } else {
                    *end = fillet.corner.clone();
                }
            }
        }
        members.insert(fillet.arc);
        points.extend(fillet.trim);
        points.insert(fillet.through);
        points.extend(fillet.arc_through.into_values());
    }
    if let Some(c) = sketch.chamfers.remove(key) {
        for i in 0..2 {
            if let Some(SketchMember {
                curve: SketchCurve::Line { start, end },
                ..
            }) = sketch.members.get_mut(&c.lines[i])
            {
                if c.at_start[i] {
                    *start = c.corner.clone();
                } else {
                    *end = c.corner.clone();
                }
            }
        }
        members.insert(c.member);
        points.extend(c.trim);
    }
    if let Some(pattern) = sketch.transforms.remove(key) {
        members.extend(crate::sketch_transform::outputs(&pattern));
        points.extend(crate::sketch_transform::generated_points(&pattern));
    }
    if let Some(spline) = sketch.splines.remove(key) {
        members.insert(key.into());
        points.extend(spline.driven_points());
    }
    if let Some(definition) = sketch.conics.remove(key) {
        members.extend(definition.outputs(key));
        points.extend(definition.driven_points());
    }
    if let Some(definition) = sketch.texts.remove(key) {
        members.extend(definition.outputs());
        points.extend(definition.driven_points());
    }
    if let Some(definition) = sketch.equations.remove(key) {
        members.extend(definition.outputs(key));
        points.extend(definition.driven_points());
    }
    if let Some(instance) = sketch.block_instances.remove(key) {
        members.extend(instance.outputs());
        points.extend(instance.driven_points());
        block_anchor = Some(instance.anchor);
    }
    for member in &members {
        sketch.members.remove(member);
    }
    sketch.constraints.retain(|c| {
        !c.entities
            .iter()
            .any(|id| members.contains(id) || points.contains(id))
    });
    for point in points {
        if !sketch
            .members
            .values()
            .any(|m| m.curve.points().contains(&point.as_str()))
        {
            sketch.points.remove(&point);
        }
    }
    if let Some(anchor) = block_anchor {
        crate::sketch_blocks::remove_unused_anchor(sketch, &anchor);
    }
    crate::sketch_dimensions::reconcile_removed(sketch);
}
/// Delete selected geometry and dependent operations as one source edit.
pub fn delete_members(sketch: &mut Sketch, deleted: &BTreeSet<Id>) {
    let mut deleted = deleted.clone();
    let mut generated_points = BTreeSet::new();
    let dynamic = sketch
        .transforms
        .iter()
        .filter(|(_, pattern)| {
            pattern.dynamic
                && pattern
                    .sources
                    .iter()
                    .any(|source| deleted.contains(source))
        })
        .map(|(key, _)| key.clone())
        .collect::<Vec<_>>();
    for key in dynamic {
        if let Some(mut pattern) = sketch.transforms.remove(&key) {
            pattern.sources.retain(|source| !deleted.contains(source));
            let controls = pattern
                .sources
                .iter()
                .filter_map(|source| effective_curve(sketch, source).ok())
                .flat_map(|curve| {
                    curve
                        .points()
                        .into_iter()
                        .map(str::to_string)
                        .collect::<Vec<_>>()
                })
                .collect::<BTreeSet<_>>();
            for instance in &mut pattern.instances {
                instance.members.retain(|source, target| {
                    if deleted.contains(source) {
                        deleted.insert(target.clone());
                        false
                    } else {
                        true
                    }
                });
                instance.points.retain(|source, target| {
                    if controls.contains(source) {
                        true
                    } else {
                        generated_points.insert(target.clone());
                        false
                    }
                });
            }
            sketch.transforms.insert(key, pattern);
        }
    }
    loop {
        let operations = modifier_ids(sketch)
            .into_iter()
            .filter(|key| {
                modifier_sources(sketch, key)
                    .iter()
                    .chain(modifier_members(sketch, key).iter())
                    .any(|id| deleted.contains(id))
                    || modifier_reference_points(sketch, key)
                        .iter()
                        .any(|id| generated_points.contains(id))
            })
            .collect::<Vec<_>>();
        if operations.is_empty() {
            break;
        }
        for key in operations {
            generated_points.extend(modifier_points(sketch, &key));
            deleted.extend(modifier_members(sketch, &key));
            remove_modifier(sketch, &key);
        }
    }
    sketch.members.retain(|key, _| !deleted.contains(key));
    sketch.conversions.retain(|key, _| !deleted.contains(key));
    let mut removed_points = BTreeSet::new();
    for point in generated_points {
        if !sketch
            .members
            .values()
            .any(|m| m.curve.points().contains(&point.as_str()))
            && !sketch.fillets.values().any(|f| f.corner == point)
            && !sketch.chamfers.values().any(|f| f.corner == point)
        {
            sketch.points.remove(&point);
            removed_points.insert(point);
        }
    }

    sketch.constraints.retain(|c| {
        !c.entities
            .iter()
            .any(|id| deleted.contains(id) || removed_points.contains(id))
    });
    crate::sketch_dimensions::reconcile_removed(sketch);
}

/// Validate saved references and generated topology before any indexing or kernel call.
pub fn validate(sketch: &Sketch) -> Result<()> {
    crate::sketch_dimensions::validate(sketch)?;
    let exists = |key: &Id| sketch.points.contains_key(key);
    for c in &sketch.constraints {
        if c.distance_side.is_some() && (c.kind != "point_line_distance" || c.entities.len() != 2) {
            return Err(invalid(
                "Saved distance side requires a point-to-oriented-line dimension",
            ));
        }
        if c.driven && !sketch_actions::is_dimension(&c.kind) {
            return Err(invalid("Driven measurements apply only to dimensions"));
        }
        if let Some(contact) = &c.contact {
            if c.driven
                || !matches!(c.kind.as_str(), "tangent" | "curvature" | "point_on_curve")
                || c.entities.len() != 2
                || c.entities[0] == c.entities[1]
                || contact.parameters.iter().any(|v| !v.is_finite())
            {
                return Err(invalid("Invalid saved curve contact intent"));
            }
            for i in 0..2 {
                if c.kind == "point_on_curve" && i == 0 {
                    if contact.free[0]
                        || sketch_actions::entity_point(sketch, &c.entities[0]).is_err()
                    {
                        return Err(invalid(
                            "Point on Curve needs a point and one curve parameter",
                        ));
                    }
                } else if !sketch.members.contains_key(&c.entities[i])
                    || (!contact.free[i]
                        && !sketch_relations::closed_contact(sketch, &c.entities[i])
                        && !(0.0..=1.0).contains(&contact.parameters[i]))
                {
                    return Err(invalid(
                        "Curve contact needs existing members and bounded fixed parameters",
                    ));
                }
            }
        }
    }
    for member in sketch.members.values() {
        if let SketchCurve::Nurbs {
            points,
            weights,
            knots,
            degree,
        } = &member.curve
        {
            let control_points = points
                .iter()
                .map(|id| {
                    sketch
                        .points
                        .get(id)
                        .copied()
                        .ok_or_else(|| invalid("Missing spline control"))
                })
                .collect::<Result<_>>()?;
            validate_nurbs(&Curve2 {
                control_points,
                weights: weights.clone(),
                knots: knots.clone(),
                degree: *degree,
            })?;
        }
    }
    let ids = modifier_ids(sketch);
    if ids.iter().collect::<BTreeSet<_>>().len() != ids.len() {
        return Err(invalid("Duplicate sketch modifier identity"));
    }
    let mut generated = BTreeSet::new();
    for (member, conversion) in &sketch.conversions {
        validate_nurbs(&conversion.curve)?;
        if !sketch.members.contains_key(member)
            || conversion.points.len() != conversion.curve.control_points.len()
            || !conversion.points.iter().all(exists)
            || conversion.reference.topology != "edge"
        {
            return Err(invalid("Invalid converted sketch member or source edge"));
        }
    }
    for (key, offset) in &sketch.offsets {
        if sketch.fillets.contains_key(key) || offset.members.is_empty() {
            return Err(invalid("Invalid or duplicate sketch operation identity"));
        }
        for member in &offset.members {
            if member.source == member.member
                || !sketch.members.contains_key(&member.source)
                || !sketch.members.contains_key(&member.member)
                || !member.points.iter().all(exists)
                || !generated.insert(member.member.clone())
            {
                return Err(invalid(
                    "Offset has a missing, cyclic or multiply driven member",
                ));
            }
            let expected = match &sketch.members[&member.source].curve {
                SketchCurve::Line { .. } => 2,
                SketchCurve::Arc { .. } => 3,
                SketchCurve::Circle { .. } => 1,
                SketchCurve::Bezier { points } | SketchCurve::Nurbs { points, .. }
                    if offset.members.len() == 1 =>
                {
                    points.len()
                }
                _ => {
                    return Err(CadError::Unsupported(
                        "Offsets support lines, circular arcs and circles".into(),
                    ));
                }
            };
            if member.points.len() != expected {
                return Err(invalid(
                    "Offset output control topology does not match its source",
                ));
            }
        }
    }
    for fillet in sketch.fillets.values() {
        let points = fillet
            .trim
            .iter()
            .chain(std::iter::once(&fillet.through))
            .chain(fillet.arc_through.values())
            .collect::<Vec<_>>();
        if points.iter().collect::<BTreeSet<_>>().len() != points.len()
            || points.contains(&&fillet.corner)
            || fillet.source_curves.values().any(|curve| {
                curve
                    .points()
                    .iter()
                    .any(|id| points.iter().any(|point| point.as_str() == *id))
            })
            || fillet.arc_through.keys().any(|key| {
                !fillet.lines.contains(key)
                    || !matches!(fillet.source_curves.get(key), Some(SketchCurve::Arc { .. }))
            })
        {
            return Err(invalid(
                "Fillet generated handles must be distinct from its original source points",
            ));
        }
        if fillet.lines[0] == fillet.lines[1]
            || !fillet.lines.iter().all(|key| {
                matches!(
                    sketch.members.get(key).map(|m| &m.curve),
                    Some(SketchCurve::Line { .. } | SketchCurve::Arc { .. })
                )
            })
            || !exists(&fillet.corner)
            || !fillet.trim.iter().all(exists)
            || !exists(&fillet.through)
            || !sketch.members.contains_key(&fillet.arc)
            || !generated.insert(fillet.arc.clone())
            || fillet.arc_through.values().any(|key| !exists(key))
            || fillet.source_curves.iter().any(|(key, curve)| {
                !fillet.lines.contains(key)
                    || curve
                        .points()
                        .iter()
                        .any(|point| !sketch.points.contains_key(*point))
            })
        {
            return Err(invalid(
                "Fillet has a missing or multiply driven member/corner",
            ));
        }
        for key in &fillet.lines {
            if matches!(sketch.members[key].curve, SketchCurve::Arc { .. })
                && (!matches!(fillet.source_curves.get(key), Some(SketchCurve::Arc { .. }))
                    || !fillet.arc_through.contains_key(key))
            {
                return Err(invalid(
                    "Arc fillets require original circular source intent and stable trim handles",
                ));
            }
            if matches!(sketch.members[key].curve, SketchCurve::Arc { .. })
                && sketch
                    .fillets
                    .values()
                    .filter(|other| other.lines.contains(key))
                    .count()
                    != 1
            {
                return Err(invalid("An arc supports one corner fillet operation"));
            }
        }
    }
    for c in sketch.chamfers.values() {
        if c.lines[0] == c.lines[1]
            || !c.lines.iter().all(|k| {
                matches!(
                    sketch.members.get(k).map(|m| &m.curve),
                    Some(SketchCurve::Line { .. })
                )
            })
            || !exists(&c.corner)
            || !c.trim.iter().all(exists)
            || !sketch.members.contains_key(&c.member)
            || !generated.insert(c.member.clone())
        {
            return Err(invalid(
                "Chamfer has missing or multiply driven members/corner",
            ));
        }
    }
    for pattern in sketch.transforms.values() {
        crate::sketch_transform::validate(sketch, pattern)?;
        if crate::sketch_transform::outputs(pattern)
            .iter()
            .any(|key| !generated.insert(key.clone()))
        {
            return Err(invalid("Sketch pattern output has multiple drivers"));
        }
    }
    for (member, spline) in &sketch.splines {
        crate::sketch_spline::validate(sketch, member, spline)?;
        if !generated.insert(member.clone()) {
            return Err(invalid(
                "Spline parameters cannot also be driven by another sketch operation",
            ));
        }
        let controls = spline.driven_points();
        if modifier_ids(sketch)
            .iter()
            .filter(|id| id.as_str() != member)
            .any(|other| {
                modifier_points(sketch, other)
                    .iter()
                    .any(|id| controls.contains(id))
            })
            || sketch
                .conversions
                .values()
                .any(|conversion| conversion.points.iter().any(|id| controls.contains(id)))
        {
            return Err(invalid(
                "Spline interpolation controls have multiple source drivers",
            ));
        }
    }
    for (key, definition) in &sketch.conics {
        crate::sketch_conic::validate(sketch, key, definition)?;
        if definition
            .outputs(key)
            .iter()
            .any(|member| !generated.insert(member.clone()))
        {
            return Err(invalid("Conic outputs have multiple source drivers"));
        }
        let controls = definition.driven_points();
        if modifier_ids(sketch)
            .iter()
            .filter(|other| other.as_str() != key)
            .any(|other| {
                modifier_points(sketch, other)
                    .iter()
                    .any(|id| controls.contains(id))
            })
            || sketch
                .conversions
                .values()
                .any(|conversion| conversion.points.iter().any(|id| controls.contains(id)))
        {
            return Err(invalid("Conic controls have multiple source drivers"));
        }
    }
    for definition in sketch.texts.values() {
        crate::sketch_text::validate(sketch, definition)?;
        if definition
            .outputs()
            .iter()
            .any(|member| !generated.insert(member.clone()))
        {
            return Err(invalid("Sketch Text outlines have multiple source drivers"));
        }
        let controls = definition.driven_points();
        if modifier_ids(sketch)
            .iter()
            .filter(|key| sketch.texts.get(*key) != Some(definition))
            .any(|key| {
                modifier_points(sketch, key)
                    .iter()
                    .any(|point| controls.contains(point))
            })
            || sketch.conversions.values().any(|conversion| {
                conversion
                    .points
                    .iter()
                    .any(|point| controls.contains(point))
            })
        {
            return Err(invalid("Sketch Text controls have multiple source drivers"));
        }
    }
    for (key, definition) in &sketch.equations {
        crate::sketch_equation::validate(sketch, key, definition)?;
        if !generated.insert(key.clone()) {
            return Err(invalid(
                "Equation curve outputs have multiple source drivers",
            ));
        }
        let controls = definition.driven_points();
        if modifier_ids(sketch)
            .iter()
            .filter(|other| other.as_str() != key)
            .any(|other| {
                modifier_points(sketch, other)
                    .iter()
                    .any(|point| controls.contains(point))
            })
            || sketch.conversions.values().any(|conversion| {
                conversion
                    .points
                    .iter()
                    .any(|point| controls.contains(point))
            })
        {
            return Err(invalid(
                "Equation curve controls have multiple source drivers",
            ));
        }
    }
    crate::sketch_blocks::validate(sketch)?;
    for instance in sketch.block_instances.values() {
        if instance
            .outputs()
            .iter()
            .any(|member| !generated.insert(member.clone()))
        {
            return Err(invalid("Sketch Block output members have multiple drivers"));
        }
        let controls = instance.driven_points();
        if sketch.conversions.values().any(|conversion| {
            conversion
                .points
                .iter()
                .any(|point| controls.contains(point))
        }) || modifier_ids(sketch)
            .iter()
            .filter(|key| sketch.block_instances.get(*key) != Some(instance))
            .any(|key| {
                modifier_points(sketch, key)
                    .iter()
                    .any(|point| controls.contains(point))
            })
        {
            return Err(invalid(
                "Sketch Block output controls have multiple drivers",
            ));
        }
    }
    if generated
        .iter()
        .any(|id| sketch.conversions.contains_key(id))
    {
        return Err(invalid(
            "Generated geometry cannot also have a converted source relation",
        ));
    }
    Ok(())
}

fn bernstein_product(a: &[f64], b: &[f64]) -> Vec<f64> {
    let choose =
        |n: usize, k: usize| (0..k.min(n - k)).fold(1., |v, i| v * (n - i) as f64 / (i + 1) as f64);
    let (m, n) = (a.len() - 1, b.len() - 1);
    (0..=m + n)
        .map(|k| {
            (k.saturating_sub(n)..=k.min(m))
                .map(|i| a[i] * b[k - i] * choose(m, i) * choose(n, k - i) / choose(m + n, k))
                .sum()
        })
        .collect()
}
fn rational_circle(curve: &Curve2) -> Result<Option<(Vector2<f64>, f64, f64)>> {
    if !(2..=16).contains(&curve.degree) {
        return Ok(None);
    }
    let spans = crate::sketch_trim::bezier_spans(curve)?;
    let Some(first) = spans.first() else {
        return Ok(None);
    };
    let points = [
        nurbs_point(first, 0.)?,
        nurbs_point(first, 0.5)?,
        nurbs_point(first, 1.)?,
    ];
    let Ok((center, radius, _, sweep)) =
        sketch_geometry::arc_frame(points[0], points[1], points[2])
    else {
        return Ok(None);
    };
    if !radius.is_finite() || radius <= EPS {
        return Ok(None);
    }
    let direction = sweep.signum();
    for span in spans {
        let max = span.weights.iter().copied().fold(0., f64::max);
        let homogeneous = span
            .control_points
            .iter()
            .zip(&span.weights)
            .map(|(p, w)| {
                [
                    (p[0] - center[0]) / radius * (w / max),
                    (p[1] - center[1]) / radius * (w / max),
                    w / max,
                ]
            })
            .collect::<Vec<_>>();
        if homogeneous.iter().flatten().any(|v| !v.is_finite()) {
            return Err(invalid("Exact circular offset proof overflow"));
        }
        let x = homogeneous.iter().map(|p| p[0]).collect::<Vec<_>>();
        let y = homogeneous.iter().map(|p| p[1]).collect::<Vec<_>>();
        let w = homogeneous.iter().map(|p| p[2]).collect::<Vec<_>>();
        let xx = bernstein_product(&x, &x);
        let yy = bernstein_product(&y, &y);
        let ww = bernstein_product(&w, &w);
        if xx
            .iter()
            .zip(&yy)
            .zip(&ww)
            .any(|((x, y), w)| (x + y - w).abs() > 1e-10 * (x.abs() + y.abs() + w.abs()).max(1e-30))
        {
            return Ok(None);
        }
        let dx = x
            .windows(2)
            .map(|p| (p[1] - p[0]) * span.degree as f64)
            .collect::<Vec<_>>();
        let dy = y
            .windows(2)
            .map(|p| (p[1] - p[0]) * span.degree as f64)
            .collect::<Vec<_>>();
        let xy = bernstein_product(&x, &dy);
        let yx = bernstein_product(&y, &dx);
        let angular = xy
            .iter()
            .zip(&yx)
            .map(|(a, b)| (a - b) * direction)
            .collect::<Vec<_>>();
        if angular.iter().any(|v| !v.is_finite() || *v < -1e-12)
            || angular.iter().all(|v| *v <= 1e-12)
        {
            return Ok(None);
        }
    }
    Ok(Some((Vector2::from(center), radius, direction)))
}
fn independent_spline_offset(
    doc: &Document,
    owner: &str,
    sketch: &mut Sketch,
    operation: &Offset,
) -> Result<bool> {
    if operation.members.len() != 1 {
        return Ok(false);
    }
    let member = &operation.members[0];
    let source = sketch
        .members
        .get(&member.source)
        .ok_or_else(|| invalid("Offset source missing"))?
        .curve
        .clone();
    let (points, weights, knots, degree, bezier) = match source {
        SketchCurve::Bezier { points } => {
            let degree = points
                .len()
                .checked_sub(1)
                .ok_or_else(|| invalid("Empty offset spline"))?;
            let count = points.len();
            (
                points,
                vec![1.; count],
                std::iter::repeat_n(0., degree + 1)
                    .chain(std::iter::repeat_n(1., degree + 1))
                    .collect(),
                degree,
                true,
            )
        }
        SketchCurve::Nurbs {
            points,
            weights,
            knots,
            degree,
        } => (points, weights, knots, degree, false),
        _ => return Ok(false),
    };
    let curve = Curve2 {
        control_points: points
            .iter()
            .map(|key| p(sketch, key).map(Into::into))
            .collect::<Result<_>>()?,
        weights: weights.clone(),
        knots: knots.clone(),
        degree,
    };
    validate_nurbs(&curve)?;
    if curve.control_points.windows(2).any(|p| {
        !Vector2::from(p[1])
            .metric_distance(&Vector2::from(p[0]))
            .is_finite()
    }) || !Vector2::from(curve.control_points[0])
        .metric_distance(&Vector2::from(*curve.control_points.last().unwrap()))
        .is_finite()
    {
        return Err(invalid(
            "Offset source separation exceeds the finite coordinate range",
        ));
    }
    let distance = expression::value(doc, owner, &operation.distance, Dimension::Length)?;
    if !distance.is_finite() || distance <= EPS {
        return Err(invalid("Offset distance must be finite and positive"));
    }
    let mut output = curve
        .control_points
        .iter()
        .copied()
        .map(Vector2::from)
        .collect::<Vec<_>>();
    if let Some((center, radius, direction)) = rational_circle(&curve)? {
        let delta = distance
            * if operation.reverse { -1. } else { 1. }
            * if operation.closed { 1. } else { -direction };
        let next = radius + delta;
        if next <= EPS {
            return Err(invalid("Offset collapses the exact circular carrier"));
        }
        for point in &mut output {
            *point = center + (*point - center) * (next / radius);
        }
    } else {
        let a = output[0];
        let b = *output.last().unwrap();
        let direction = (b - a).try_normalize(EPS);
        let straight = direction.is_some_and(|d| {
            output.iter().all(|p| cross(*p - a, d).abs() <= EPS)
                && output.windows(2).all(|p| (p[1] - p[0]).dot(&d) >= -EPS)
        });
        if straight {
            let shift =
                left(direction.unwrap()) * distance * if operation.reverse { -1. } else { 1. };
            for point in &mut output {
                *point += shift;
            }
        } else if degree == 1 {
            let closed = operation.closed;
            if closed && (output[0] - *output.last().unwrap()).norm() > EPS {
                return Err(invalid(
                    "Closed polyline offset must preserve its closing endpoint",
                ));
            }
            let area = output.windows(2).map(|p| cross(p[0], p[1])).sum::<f64>();
            let signed = distance
                * if closed && area > 0. { -1. } else { 1. }
                * if operation.reverse { -1. } else { 1. };
            let mut segments = Vec::new();
            for pair in output.windows(2) {
                let d = (pair[1] - pair[0])
                    .try_normalize(EPS)
                    .ok_or_else(|| invalid("Offset polyline has a collapsed span"))?;
                let shift = left(d) * signed;
                segments.push(Segment::Line {
                    a: pair[0] + shift,
                    b: pair[1] + shift,
                });
            }
            let original = output.clone();
            output[0] = segments[0].ends().0;
            let last = output.len() - 1;
            output[last] = segments.last().unwrap().ends().1;
            for i in 1..last {
                output[i] = segments[i - 1]
                    .intersects(&segments[i])
                    .into_iter()
                    .min_by(|a, b| {
                        (*a - original[i])
                            .norm_squared()
                            .total_cmp(&(*b - original[i]).norm_squared())
                    })
                    .ok_or_else(|| {
                        invalid("Offset polyline contains parallel or reversing adjacent spans")
                    })?;
            }
            if closed {
                let join = segments
                    .last()
                    .unwrap()
                    .intersects(&segments[0])
                    .into_iter()
                    .next()
                    .ok_or_else(|| {
                        invalid("Offset polyline closing spans are parallel or reversing")
                    })?;
                output[0] = join;
                output[last] = join;
            }
            if output
                .windows(2)
                .zip(original.windows(2))
                .any(|(p, q)| (p[1] - p[0]).dot(&(q[1] - q[0])) <= EPS * EPS)
            {
                return Err(invalid("Offset collapses or reverses a polyline span"));
            }
            for i in 0..last {
                for j in i + 2..last {
                    if closed && i == 0 && j == last - 1 {
                        continue;
                    }
                    let a = Segment::Line {
                        a: output[i],
                        b: output[i + 1],
                    };
                    let b = Segment::Line {
                        a: output[j],
                        b: output[j + 1],
                    };
                    let d = output[i + 1] - output[i];
                    let e = output[j + 1] - output[j];
                    if cross(d, e).abs() <= 1e-12 * d.norm() * e.norm()
                        && cross(output[j] - output[i], d).abs() <= EPS * d.norm()
                    {
                        let first = (output[j] - output[i]).dot(&d) / d.norm_squared();
                        let second = (output[j + 1] - output[i]).dot(&d) / d.norm_squared();
                        if first.min(second) <= 1. + 1e-10 && first.max(second) >= -1e-10 {
                            return Err(invalid("Offset polyline has overlapping collinear spans"));
                        }
                    }
                    for intersection in a.intersects(&b) {
                        let inside = |start: Vector2<f64>, end: Vector2<f64>| {
                            let d = end - start;
                            let t = (intersection - start).dot(&d) / d.norm_squared();
                            (-1e-10..=1. + 1e-10).contains(&t)
                        };
                        if inside(output[i], output[i + 1]) && inside(output[j], output[j + 1]) {
                            return Err(invalid(
                                "Offset polyline self-intersects; reduce the distance or split the contour",
                            ));
                        }
                    }
                }
            }
        } else {
            return Err(CadError::Unsupported("Exact spline offset requires a proven straight/circular carrier or a degree-one polyline; general spline and ellipse offsets are not rational curves".into()));
        }
    }
    if output.len() != member.points.len() {
        return Err(invalid("Offset spline changed its source control topology"));
    }
    for (id, point) in member.points.iter().zip(output) {
        put(sketch, id, point)?;
    }
    sketch
        .members
        .get_mut(&member.member)
        .ok_or_else(|| invalid("Offset output missing"))?
        .curve = if bezier {
        SketchCurve::Bezier {
            points: member.points.clone(),
        }
    } else {
        SketchCurve::Nurbs {
            points: member.points.clone(),
            weights,
            knots,
            degree,
        }
    };
    Ok(true)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReplaceRequest {
    pub edit_id: Id,
    pub target: Id,
    pub replacement: Id,
    pub reverse: bool,
    pub consume_replacement: bool,
    pub remove_incompatible_relations: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReplaceReport {
    pub target: Id,
    pub preserved_points: Vec<Id>,
    pub removed_relations: Vec<Id>,
    pub consumed: Option<Id>,
}
fn reverse_curve(curve: SketchCurve) -> SketchCurve {
    match curve {
        SketchCurve::Line { start, end } => SketchCurve::Line {
            start: end,
            end: start,
        },
        SketchCurve::Arc {
            start,
            through,
            end,
        } => SketchCurve::Arc {
            start: end,
            through,
            end: start,
        },
        SketchCurve::Bezier { mut points } => {
            points.reverse();
            SketchCurve::Bezier { points }
        }
        SketchCurve::Nurbs {
            mut points,
            mut weights,
            knots,
            degree,
        } => {
            points.reverse();
            weights.reverse();
            let sum = knots[degree] + knots[points.len()];
            let knots = knots.into_iter().rev().map(|value| sum - value).collect();
            SketchCurve::Nurbs {
                points,
                weights,
                knots,
                degree,
            }
        }
        other => other,
    }
}
/// Replace the exact carrier while retaining the target member identity and its
/// endpoint/control identities where the source topology permits a transfer.
pub fn replace_entities(
    doc: &Document,
    owner: &str,
    sketch: &mut Sketch,
    request: &ReplaceRequest,
) -> Result<ReplaceReport> {
    if request.target == request.replacement
        || request.edit_id.is_empty()
        || request.edit_id.len() > 128
        || !request
            .edit_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(invalid(
            "Replace Entities needs two distinct members and a stable edit identity",
        ));
    }
    let mut candidate = sketch.clone();
    let old = candidate
        .members
        .get(&request.target)
        .ok_or_else(|| invalid("Replacement target is missing"))?
        .curve
        .clone();
    let mut replacement = candidate
        .members
        .get(&request.replacement)
        .ok_or_else(|| invalid("Replacement source is missing"))?
        .curve
        .clone();
    let driven = driven_points(&candidate);
    for member in [&request.target, &request.replacement] {
        if driven_member(&candidate, member)
            || effective_curve(&candidate, member)?
                .points()
                .iter()
                .any(|key| driven.contains(*key))
        {
            return Err(invalid(
                "Replace independent members; edit or detach their generating operations first",
            ));
        }
    }
    for key in modifier_ids(&candidate) {
        let sources = modifier_sources(&candidate, &key);
        if (!candidate.transforms.contains_key(&key) && sources.contains(&request.target))
            || (request.consume_replacement && sources.contains(&request.replacement))
        {
            return Err(invalid(
                "A replacement member is used by another sketch operation; detach that operation or keep the replacement source",
            ));
        }
    }
    if request.reverse {
        replacement = reverse_curve(replacement);
    }
    let old_points = old
        .points()
        .into_iter()
        .map(str::to_string)
        .collect::<Vec<_>>();
    let replacement_points = replacement
        .points()
        .into_iter()
        .map(str::to_string)
        .collect::<Vec<_>>();
    let analytic_closed = |curve: &SketchCurve| {
        matches!(
            curve,
            SketchCurve::Circle { .. } | SketchCurve::Ellipse { .. }
        )
    };
    if analytic_closed(&old) != analytic_closed(&replacement) {
        return Err(CadError::Unsupported("Replace Entities preserves either an open/control curve or a centered closed curve; circle/ellipse replacement cannot consume open endpoints".into()));
    }
    if old_points.len() != replacement_points.len()
        && candidate
            .transforms
            .values()
            .any(|pattern| pattern.sources.contains(&request.target))
    {
        return Err(CadError::Unsupported("Changing control topology of a patterned source requires detaching or reselecting its pattern first".into()));
    }
    let mut mapping = BTreeMap::<Id, Id>::new();
    let mut positions = BTreeMap::<Id, [f64; 2]>::new();
    let mut preserved = BTreeSet::new();
    for (index, source) in replacement_points.iter().enumerate() {
        let target = if old_points.len() == replacement_points.len() {
            old_points[index].clone()
        } else if index == 0 {
            old_points[0].clone()
        } else if index + 1 == replacement_points.len() {
            old_points.last().unwrap().clone()
        } else {
            format!("{}-control-{index}", request.edit_id)
        };
        if old_points.contains(source) && source != &target {
            return Err(invalid(
                "Shared replacement controls need a matching orientation; reverse the replacement or detach the shared controls first",
            ));
        }
        if let Some(existing) = mapping.insert(source.clone(), target.clone())
            && existing != target
        {
            return Err(invalid(
                "Replacement curve shares controls differently from its target; split or detach those controls first",
            ));
        }
        let position = *candidate
            .points
            .get(source)
            .ok_or_else(|| invalid("Replacement control is missing"))?;
        if position.iter().any(|value| !value.is_finite()) {
            return Err(invalid("Replacement controls must be finite"));
        }
        if let Some(existing) = positions.insert(target.clone(), position)
            && existing != position
        {
            return Err(invalid(
                "Replacement would assign different coordinates to a shared target control",
            ));
        }
        if old_points.contains(&target) {
            preserved.insert(target);
        } else if candidate.points.contains_key(&target) {
            return Err(invalid(
                "Replacement edit namespace already owns a source control",
            ));
        }
    }
    let curve = crate::sketch_transform::remap(&replacement, &mapping)?;
    candidate.points.extend(positions);
    candidate.members.get_mut(&request.target).unwrap().curve = curve.clone();
    if request.consume_replacement {
        candidate.members.remove(&request.replacement);
        for constraint in &mut candidate.constraints {
            for entity in &mut constraint.entities {
                if entity == &request.replacement {
                    *entity = request.target.clone();
                }
            }
        }
        for (index, (source, target)) in mapping
            .iter()
            .filter(|(source, target)| source != target)
            .enumerate()
        {
            let key = format!("{}-retained-{index}", request.edit_id);
            if candidate
                .constraints
                .iter()
                .any(|constraint| constraint.id == key)
            {
                return Err(invalid(
                    "Replacement namespace already owns a retained-point relation",
                ));
            }
            candidate.constraints.push(SketchConstraint {
                id: key,
                kind: "coincident".into(),
                entities: vec![source.clone(), target.clone()],
                value: "0 mm".into(),
                driven: false,
                contact: None,
                distance_side: None,
            });
        }
    }
    for pattern in candidate.transforms.values() {
        if pattern.sources.contains(&request.target) {
            for instance in &pattern.instances {
                let target = &instance.members[&request.target];
                candidate
                    .members
                    .get_mut(target)
                    .ok_or_else(|| invalid("Pattern replacement target is missing"))?
                    .curve = crate::sketch_transform::remap(&curve, &instance.points)?;
            }
        }
    }
    let obsolete = old_points
        .iter()
        .filter(|point| !preserved.contains(*point))
        .cloned()
        .collect::<BTreeSet<_>>();
    let mut removed = Vec::new();
    let mut retained = Vec::new();
    for constraint in &candidate.constraints {
        if !constraint
            .entities
            .iter()
            .any(|key| key == &request.target || obsolete.contains(key))
        {
            retained.push(constraint.clone());
            continue;
        }
        let mut probe = candidate.clone();
        probe.constraints = vec![constraint.clone()];
        let compatible = if constraint
            .entities
            .iter()
            .any(|point| obsolete.contains(point))
        {
            false
        } else {
            let evaluate = (|| -> Result<()> {
                if constraint.driven {
                    crate::sketch_actions::measured_dimension_geometry(
                        doc, owner, &probe, constraint,
                    )?;
                } else {
                    crate::sketch_actions::residual_blocks(doc, owner, &probe, |key| {
                        crate::sketch_actions::entity_point(&probe, key)
                    })?;
                }
                Ok(())
            })();
            evaluate.is_ok()
        };
        if compatible {
            retained.push(constraint.clone());
        } else if request.remove_incompatible_relations {
            removed.push(constraint.id.clone());
        } else {
            return Err(invalid(
                "Replacement has an incompatible relation or lost control reference; explicitly allow removal or edit that relation first",
            ));
        }
    }
    candidate.constraints = retained;
    crate::sketch_dimensions::reconcile_removed(&mut candidate);
    let mut orphan_candidates = old_points;
    if request.consume_replacement {
        orphan_candidates.extend(replacement_points);
    }
    for point in orphan_candidates {
        if !candidate
            .members
            .values()
            .any(|member| member.curve.points().contains(&point.as_str()))
            && !candidate
                .constraints
                .iter()
                .any(|constraint| constraint.entities.contains(&point))
            && !modifier_ids(&candidate)
                .iter()
                .any(|key| modifier_reference_points(&candidate, key).contains(&point))
        {
            candidate.points.remove(&point);
        }
    }
    validate(&candidate)?;
    *sketch = candidate;
    Ok(ReplaceReport {
        target: request.target.clone(),
        preserved_points: preserved.into_iter().collect(),
        removed_relations: removed,
        consumed: request
            .consume_replacement
            .then(|| request.replacement.clone()),
    })
}
