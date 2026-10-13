//! Sketch selection, dimension intent and relation contracts.
use crate::{expression, sketch_geometry::arc_frame, *};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const ORIGIN: &str = "@origin";
pub const X_AXIS: &str = "@x-axis";
pub const Y_AXIS: &str = "@y-axis";
pub fn is_false(value: &bool) -> bool {
    !*value
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SketchEntity {
    Point { point: Id },
    Member { member: Id },
    Origin,
    XAxis,
    YAxis,
    Constraint { constraint: Id },
}
impl SketchEntity {
    pub fn key(&self) -> &str {
        match self {
            Self::Point { point } => point,
            Self::Member { member } => member,
            Self::Origin => ORIGIN,
            Self::XAxis => X_AXIS,
            Self::YAxis => Y_AXIS,
            Self::Constraint { constraint } => constraint,
        }
    }
}
pub const RELATIONS: &[(&str, &str)] = &[
    ("horizontal", "Horizontal"),
    ("vertical", "Vertical"),
    ("coincident", "Coincident"),
    ("point_on_arc", "Point on Bounded Arc"),
    ("parallel", "Parallel"),
    ("perpendicular", "Perpendicular"),
    ("equal", "Equal Length"),
    ("equal_radius", "Equal Radius"),
    ("concentric", "Concentric"),
    ("tangent", "Tangent"),
    ("curvature", "Curvature Continuous"),
    ("tangent_internal", "Internal Tangent"),
    ("arc_center", "Arc Center"),
    ("midpoint", "Midpoint"),
    ("symmetric_origin", "Symmetric about Origin"),
    ("symmetric_axis", "Symmetric about Selected Axis"),
    ("origin", "Coincident with Origin"),
    ("on_x_axis", "On X Axis"),
    ("on_y_axis", "On Y Axis"),
    ("fix", "Fix Position"),
];
#[derive(Clone, Debug)]
pub struct DimensionChoice {
    pub kind: String,
    pub label: String,
    pub entities: Vec<Id>,
    pub value: Expr,
    pub distance_side: Option<DistanceSide>,
}
/// Side of the exact saved oriented line, independent of its point location.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DistanceSide {
    Left,
    Right,
}
impl DistanceSide {
    pub fn reversed(self) -> Self {
        match self {
            Self::Left => Self::Right,
            Self::Right => Self::Left,
        }
    }
}
pub fn is_dimension(kind: &str) -> bool {
    matches!(
        kind,
        "fix_x"
            | "fix_y"
            | "distance"
            | "distance_x"
            | "distance_y"
            | "length"
            | "angle"
            | "line_distance"
            | "point_line_distance"
            | "radius"
            | "diameter"
            | "major_radius"
            | "minor_radius"
            | "rotation"
            | "arc_length"
            | "arc_angle"
    )
}
pub fn label(kind: &str) -> &str {
    match kind {
        "fix_x" => "X from Origin",
        "fix_y" => "Y from Origin",
        "distance_x" => "Horizontal Distance",
        "distance_y" => "Vertical Distance",
        "distance" | "line_distance" | "point_line_distance" => "Distance",
        "point_on_line" => "Coincident with Line / Axis",
        "point_on_circle" => "Coincident with Circular Geometry",
        "point_on_arc" => "Point on Bounded Arc",
        "point_on_curve" => "Point on Curve",
        "symmetric_member" => "Symmetric about Selected Axis",
        "length" => "Length",
        "angle" => "Angle",
        "radius" => "Radius",
        "diameter" => "Diameter",
        "major_radius" => "Major Radius",
        "minor_radius" => "Minor Radius",
        "rotation" => "Rotation",
        "arc_length" => "Arc Length",
        "arc_angle" => "Arc Included Angle",
        other => RELATIONS
            .iter()
            .find(|r| r.0 == other)
            .map(|r| r.1)
            .unwrap_or(other),
    }
}
pub fn dimension_unit(kind: &str) -> Dimension {
    if matches!(kind, "angle" | "rotation" | "arc_angle") {
        Dimension::Angle
    } else {
        Dimension::Length
    }
}

/// Measure the current exact source geometry without introducing solve equations.
pub fn measured_dimension(
    doc: &Document,
    owner: &str,
    sketch: &Sketch,
    c: &SketchConstraint,
) -> Result<f64> {
    let mut evaluated = sketch.clone();
    apply_curve_dimensions(doc, owner, &mut evaluated)?;
    sketch_edit::derive(doc, owner, &mut evaluated)?;
    measured_dimension_geometry(doc, owner, &evaluated, c)
}
pub(crate) fn measured_dimension_geometry(
    doc: &Document,
    owner: &str,
    sketch: &Sketch,
    c: &SketchConstraint,
) -> Result<f64> {
    let key = |i: usize| {
        c.entities
            .get(i)
            .map(String::as_str)
            .ok_or_else(|| CadError::Invalid("Reference dimension has a missing entity".into()))
    };
    let point = |i| entity_point(sketch, key(i)?);
    let endpoints = || {
        if c.entities.len() == 1 {
            entity_line(sketch, key(0)?)
        } else {
            Ok((point(0)?, point(1)?))
        }
    };
    let value = match c.kind.as_str() {
        "fix_x" => point(0)?[0],
        "fix_y" => point(0)?[1],
        "distance" | "distance_x" | "distance_y" | "length" => {
            let (a, b) = endpoints()?;
            let d = [b[0] - a[0], b[1] - a[1]];
            match c.kind.as_str() {
                "distance_x" => d[0],
                "distance_y" => d[1],
                _ => d[0].hypot(d[1]),
            }
        }
        "angle" | "line_distance" => {
            let ((a, b), (p, q)) = if c.entities.len() == 4 {
                ((point(0)?, point(1)?), (point(2)?, point(3)?))
            } else {
                (entity_line(sketch, key(0)?)?, entity_line(sketch, key(1)?)?)
            };
            let u = [b[0] - a[0], b[1] - a[1]];
            let v = [q[0] - p[0], q[1] - p[1]];
            if u[0].hypot(u[1]) < 1e-9 || v[0].hypot(v[1]) < 1e-9 {
                return Err(CadError::Invalid(
                    "Reference dimension needs nonzero members".into(),
                ));
            }
            if c.kind == "angle" {
                (u[0] * v[1] - u[1] * v[0]).atan2(u[0] * v[0] + u[1] * v[1])
            } else {
                ((p[0] - a[0]) * u[1] - (p[1] - a[1]) * u[0]).abs() / u[0].hypot(u[1])
            }
        }
        "point_line_distance" => {
            let p = point(0)?;
            let (a, b) = entity_line(sketch, key(1)?)?;
            let u = [b[0] - a[0], b[1] - a[1]];
            let l = u[0].hypot(u[1]);
            if l < 1e-9 {
                return Err(CadError::Invalid(
                    "Reference dimension needs a nonzero line".into(),
                ));
            }
            ((p[0] - a[0]) * u[1] - (p[1] - a[1]) * u[0]).abs() / l
        }
        "radius" | "diameter" => {
            circle_with(doc, owner, sketch, key(0)?, &|p| entity_point(sketch, p))?.1
                * if c.kind == "diameter" { 2. } else { 1. }
        }
        "arc_length" | "arc_angle" => {
            let Some(SketchCurve::Arc {
                start,
                through,
                end,
            }) = sketch.members.get(key(0)?).map(|m| &m.curve)
            else {
                return Err(CadError::Invalid("Reference dimension needs an arc".into()));
            };
            let (_, r, _, s) = arc_frame(
                entity_point(sketch, start)?,
                entity_point(sketch, through)?,
                entity_point(sketch, end)?,
            )?;
            if c.kind == "arc_length" {
                r * s.abs()
            } else {
                s.abs()
            }
        }
        "major_radius" | "minor_radius" | "rotation" => {
            let Some(SketchCurve::Ellipse {
                major,
                minor,
                angle,
                ..
            }) = sketch.members.get(key(0)?).map(|m| &m.curve)
            else {
                return Err(CadError::Invalid(
                    "Reference dimension needs an ellipse".into(),
                ));
            };
            expression::value(
                doc,
                owner,
                match c.kind.as_str() {
                    "major_radius" => major,
                    "minor_radius" => minor,
                    _ => angle,
                },
                dimension_unit(&c.kind),
            )?
        }
        _ => {
            return Err(CadError::Invalid(
                "Only dimensions may be driven measurements".into(),
            ));
        }
    };
    if value.is_finite() {
        Ok(value)
    } else {
        Err(CadError::Invalid(
            "Reference measurement must be finite".into(),
        ))
    }
}

pub fn refresh_measurements(doc: &Document, owner: &str, sketch: &mut Sketch) -> Result<()> {
    for c in sketch.constraints.iter().filter(|c| c.driven) {
        measured_dimension_geometry(doc, owner, sketch, c)?;
    }
    Ok(())
}
pub fn entity_point(sketch: &Sketch, key: &str) -> Result<[f64; 2]> {
    point_with(sketch, key, &|p| {
        sketch
            .points
            .get(p)
            .copied()
            .ok_or_else(|| CadError::Invalid("Selected sketch point no longer exists".into()))
    })
}
fn point_with(
    sketch: &Sketch,
    key: &str,
    point: &impl Fn(&str) -> Result<[f64; 2]>,
) -> Result<[f64; 2]> {
    if key == ORIGIN {
        return Ok([0.; 2]);
    }
    if let Some(member) = sketch.members.get(key) {
        return match &member.curve {
            SketchCurve::Circle { center, .. } | SketchCurve::Ellipse { center, .. } => {
                point(center)
            }
            _ => Err(CadError::Invalid(
                "Select an endpoint or center for this relation".into(),
            )),
        };
    }
    point(key)
}
pub fn entity_line(sketch: &Sketch, key: &str) -> Result<([f64; 2], [f64; 2])> {
    line_with(sketch, key, &|p| entity_point(sketch, p))
}
fn line_with(
    sketch: &Sketch,
    key: &str,
    point: &impl Fn(&str) -> Result<[f64; 2]>,
) -> Result<([f64; 2], [f64; 2])> {
    match key {
        X_AXIS => Ok(([0.; 2], [1., 0.])),
        Y_AXIS => Ok(([0.; 2], [0., 1.])),
        _ => match sketch.members.get(key).map(|m| &m.curve) {
            Some(SketchCurve::Line { start, end }) => Ok((point(start)?, point(end)?)),
            _ => Err(CadError::Invalid(
                "Select straight lines or sketch axes".into(),
            )),
        },
    }
}
fn circle_with(
    doc: &Document,
    owner: &str,
    sketch: &Sketch,
    key: &str,
    point: &impl Fn(&str) -> Result<[f64; 2]>,
) -> Result<([f64; 2], f64)> {
    match sketch.members.get(key).map(|m| &m.curve) {
        Some(SketchCurve::Circle { center, radius }) => Ok((
            point(center)?,
            expression::value(doc, owner, radius, Dimension::Length)?,
        )),
        Some(SketchCurve::Arc {
            start,
            through,
            end,
        }) => {
            let (c, r, _, _) = arc_frame(point(start)?, point(through)?, point(end)?)?;
            Ok((c, r))
        }
        _ => Err(CadError::Invalid("Select a circle or arc".into())),
    }
}
fn constraint(kind: &str, entities: Vec<Id>, value: Expr) -> SketchConstraint {
    SketchConstraint {
        distance_side: None,
        driven: false,
        contact: None,
        id: id(),
        kind: kind.into(),
        entities,
        value,
    }
}

/// Exact source equivalence for suppressing duplicate authored relations.
///
/// Only ordinary symmetric two-entity equations may reorder their entities.
/// Radius equality retains its reference direction: `apply_curve_dimensions`
/// uses that direction to choose the source expression for an undimensioned
/// equality group. Contact parameters, oriented distances, dimensions, and
/// unknown relation kinds retain their exact saved order. This does not infer
/// transitive equality, compare evaluated values, or alter solver rank.
pub fn relation_equivalent(existing: &SketchConstraint, candidate: &SketchConstraint) -> bool {
    if existing.kind != candidate.kind
        || existing.driven != candidate.driven
        || existing.value != candidate.value
        || existing.contact != candidate.contact
        || existing.distance_side != candidate.distance_side
    {
        return false;
    }
    if existing.entities == candidate.entities {
        return true;
    }
    existing.contact.is_none()
        && existing.distance_side.is_none()
        && existing.entities.len() == 2
        && candidate.entities.len() == 2
        && matches!(
            existing.kind.as_str(),
            "parallel"
                | "perpendicular"
                | "equal"
                | "concentric"
                | "coincident"
                | "horizontal"
                | "vertical"
        )
        && existing.entities[0] == candidate.entities[1]
        && existing.entities[1] == candidate.entities[0]
}

/// Whether an authored relation would repeat existing source intent.
///
/// Ordinary relations use exact source equivalence, including symmetric pair
/// order. Circular equality additionally recognizes an existing shared-size
/// group. Re-selecting that group never reverses its radius-expression ancestry.
pub fn relation_already_present(sketch: &Sketch, candidate: &SketchConstraint) -> bool {
    if sketch
        .constraints
        .iter()
        .any(|existing| relation_equivalent(existing, candidate))
    {
        return true;
    }
    candidate.kind == "equal_radius"
        && !candidate.driven
        && candidate.contact.is_none()
        && candidate.distance_side.is_none()
        && candidate.entities.len() == 2
        && circular_equality_group(sketch, &candidate.entities[0])
            .is_ok_and(|group| group.members.contains(&candidate.entities[1]))
}

/// Describes the selection semantics independently of a widget or menu layout.
pub fn relation_description(kind: &str) -> &'static str {
    match kind {
        "horizontal" => {
            "Make every selected straight member horizontal, or give all selected points the same Y coordinate."
        }
        "vertical" => {
            "Make every selected straight member vertical, or give all selected points the same X coordinate."
        }
        "parallel" => "Make every other selected line parallel to the first selected line or axis.",
        "perpendicular" => {
            "Make every other selected line perpendicular to the first selected line or axis. The other lines need not be perpendicular to one another."
        }
        "equal" => "Give every selected straight member the length of the first selected member.",
        "equal_radius" => {
            "Join selected circle or arc size groups using the first selection's existing reference. Already-equal members retain their size owner; driving dimensions remain authoritative. Conflicting driving dimensions leave the pending sketch unsatisfied."
        }
        "concentric" => {
            "Give every selected circle or arc the center of the first selected member."
        }
        "coincident" => {
            "Join all selected points, or place multiple points on one selected line, axis, or curve. A pair keeps its existing point or center relation."
        }
        "point_on_arc" => "Place every selected point on one selected bounded circular arc.",
        "midpoint" => "Place every selected point at the midpoint of one selected straight member.",
        "arc_center" => "Place every selected point at the center of one selected circular arc.",
        "origin" => "Place every selected point or member anchor at the sketch origin.",
        "on_x_axis" => {
            "Place every selected point or member anchor on the X axis; straight members also become horizontal."
        }
        "on_y_axis" => {
            "Place every selected point or member anchor on the Y axis; straight members also become vertical."
        }
        "fix" => "Fix the current position of every point in the selected points and members.",
        "tangent" => {
            "Cmd-click two curves, unambiguous endpoints, or circular/elliptical centers. Selected endpoints remain the tangent contact; ambiguous shared points need the curves selected directly."
        }
        "tangent_internal" => {
            "Select exactly two circle or arc members for internal tangency. Select the curves directly rather than their endpoints or centers."
        }
        "curvature" => {
            "Select exactly two curve members for curvature-continuous contact. Select the curves directly rather than their control points."
        }
        "symmetric_origin" => "Select exactly two points to reflect them through the origin.",
        "symmetric_axis" => {
            "Select two points or compatible members and one straight member or axis for reflection."
        }
        _ => "Apply the relation to compatible selected sketch geometry.",
    }
}

/// Expand one selection action into a complete relation batch.
///
/// The first selected entity is the reference for equality and direction groups.
/// Independent line orientations and universe anchors create one relation per
/// member; point groups and equality groups create a reference fan with `n - 1`
/// relations. No all-pairs equations are introduced. Failure rejects the entire
/// batch before callers mutate their source sketch.
pub fn relation(
    sketch: &Sketch,
    selection: &[SketchEntity],
    kind: &str,
) -> Result<Vec<SketchConstraint>> {
    let mut seen = BTreeSet::new();
    let inactive = crate::sketch_transform::inactive_entities(sketch);
    for entity in selection {
        if !seen.insert(entity.key()) {
            return Err(CadError::Invalid(
                "Select distinct sketch entities for a relation".into(),
            ));
        }
        if matches!(entity, SketchEntity::Constraint { .. }) || inactive.contains(entity.key()) {
            return Err(CadError::Invalid(
                "Select active sketch points or members; Cmd-click to add to the selection".into(),
            ));
        }
        if matches!(entity, SketchEntity::XAxis | SketchEntity::YAxis) {
            if kind == "equal" {
                return Err(CadError::Invalid(
                    "Equal Length needs straight sketch members; a universe axis defines direction, not a physical length".into(),
                ));
            }
            if matches!(kind, "horizontal" | "vertical") {
                return Err(CadError::Invalid(
                    "Horizontal and Vertical constrain sketch members or points; use Parallel or Perpendicular to reference a universe axis".into(),
                ));
            }
        }
    }
    let points = || {
        selection
            .iter()
            .all(|entity| entity_point(sketch, entity.key()).is_ok())
    };
    let members = || {
        selection.iter().all(|entity| {
            matches!(entity, SketchEntity::Member { .. })
                && entity_line(sketch, entity.key()).is_ok()
        })
    };
    match kind {
        "horizontal" | "vertical" if !selection.is_empty() && members() => selection
            .iter()
            .map(|entity| relation_single_or_pair(sketch, std::slice::from_ref(entity), kind))
            .collect::<Result<Vec<_>>>()
            .map(|batches| batches.into_iter().flatten().collect()),
        "horizontal" | "vertical" if selection.len() >= 2 && points() => {
            reference_fan(sketch, selection, kind)
        }
        "equal_radius" if selection.len() >= 2 => circular_equality_fan(sketch, selection),
        "parallel" | "perpendicular" | "equal" | "concentric" if selection.len() >= 2 => {
            reference_fan(sketch, selection, kind)
        }
        "origin" | "on_x_axis" | "on_y_axis" if selection.len() >= 2 => selection
            .iter()
            .map(|entity| relation_single_or_pair(sketch, std::slice::from_ref(entity), kind))
            .collect::<Result<Vec<_>>>()
            .map(|batches| batches.into_iter().flatten().collect()),
        "coincident" if selection.len() > 2 => {
            // Explicit points plus one circular member mean points on its
            // circumference. Two-entity legacy center relations stay unchanged.
            let carriers = selection
                .iter()
                .filter(|entity| {
                    matches!(
                        entity,
                        SketchEntity::Member { .. } | SketchEntity::XAxis | SketchEntity::YAxis
                    )
                })
                .collect::<Vec<_>>();
            if carriers.len() == 1
                && selection.iter().all(|entity| {
                    entity == carriers[0]
                        || matches!(entity, SketchEntity::Point { .. } | SketchEntity::Origin)
                })
            {
                return point_carrier_batch(sketch, selection, carriers[0]);
            }
            if points() {
                reference_fan(sketch, selection, kind)
            } else {
                let carriers = selection
                    .iter()
                    .filter(|entity| entity_point(sketch, entity.key()).is_err())
                    .collect::<Vec<_>>();
                if carriers.len() != 1 {
                    return Err(CadError::Invalid(
                        "Select points and one carrier, or only points; Cmd-click to add entities"
                            .into(),
                    ));
                }
                point_carrier_batch(sketch, selection, carriers[0])
            }
        }
        "midpoint" | "point_on_arc" | "arc_center" if selection.len() > 2 => {
            let carriers = selection
                .iter()
                .filter(|entity| match kind {
                    "midpoint" => entity_line(sketch, entity.key()).is_ok(),
                    _ => matches!(
                        sketch.members.get(entity.key()).map(|member| &member.curve),
                        Some(SketchCurve::Arc { .. })
                    ),
                })
                .collect::<Vec<_>>();
            if carriers.len() != 1 {
                return Err(CadError::Invalid(format!(
                    "{} needs selected points and exactly one {}",
                    label(kind),
                    if kind == "midpoint" {
                        "straight member"
                    } else {
                        "circular arc"
                    }
                )));
            }
            let carrier = carriers[0];
            let mut batch = Vec::new();
            for point in selection.iter().filter(|entity| *entity != carrier) {
                batch.extend(relation_single_or_pair(
                    sketch,
                    &[point.clone(), carrier.clone()],
                    kind,
                )?);
            }
            Ok(batch)
        }
        "tangent" => tangent_relation(sketch, selection),
        "tangent_internal" | "curvature" if selection.len() != 2 => {
            Err(CadError::Invalid(format!(
                "{} needs exactly two selected members so the contact is unambiguous",
                label(kind)
            )))
        }
        _ => relation_single_or_pair(sketch, selection, kind),
    }
}

#[derive(Clone)]
struct TangentPick {
    entity: SketchEntity,
    endpoint: Option<u8>,
}

/// Promote a picked point using authored curve ownership, never proximity to a
/// displayed polyline. Interior controls are not a curve's contact endpoints.
fn tangent_picks(sketch: &Sketch, entity: &SketchEntity) -> Result<Vec<TangentPick>> {
    let SketchEntity::Point { point } = entity else {
        return match entity {
            SketchEntity::Member { member } if sketch.members.contains_key(member) => {
                Ok(vec![TangentPick {
                    entity: entity.clone(),
                    endpoint: None,
                }])
            }
            SketchEntity::XAxis | SketchEntity::YAxis => Ok(vec![TangentPick {
                entity: entity.clone(),
                endpoint: None,
            }]),
            _ => Err(CadError::Invalid(
                "Tangent needs two sketch curves, endpoints, or curve centers; select curves directly and Cmd-click to add the second curve".into(),
            )),
        };
    };
    let inactive = crate::sketch_transform::inactive_entities(sketch);
    let mut picks = Vec::new();
    for member in sketch
        .members
        .keys()
        .filter(|member| !inactive.contains(*member))
    {
        let curve = crate::sketch_edit::effective_curve(sketch, member)?;
        let owner = SketchEntity::Member {
            member: member.clone(),
        };
        match curve {
            SketchCurve::Circle { center, .. } | SketchCurve::Ellipse { center, .. }
                if &center == point =>
            {
                picks.push(TangentPick {
                    entity: owner,
                    endpoint: None,
                });
            }
            SketchCurve::Circle { .. } | SketchCurve::Ellipse { .. } => {}
            _ => {
                let controls = curve.points();
                if controls.len() >= 2 {
                    for (endpoint, control) in [controls[0], *controls.last().unwrap()]
                        .into_iter()
                        .enumerate()
                    {
                        if control == point {
                            picks.push(TangentPick {
                                entity: owner.clone(),
                                endpoint: Some(endpoint as u8),
                            });
                        }
                    }
                }
            }
        }
    }
    if picks.is_empty() {
        Err(CadError::Invalid(
            "Tangent needs a curve endpoint or circle/ellipse center; interior controls and standalone points do not identify a contact. Select curves directly and Cmd-click to add the second curve".into(),
        ))
    } else {
        Ok(picks)
    }
}

fn tangent_relation(sketch: &Sketch, selection: &[SketchEntity]) -> Result<Vec<SketchConstraint>> {
    if selection.len() != 2 {
        return Err(CadError::Invalid(
            "Tangent needs exactly two selected curves, endpoints, or curve centers; Cmd-click to add or remove selections".into(),
        ));
    }
    let first = tangent_picks(sketch, &selection[0])?;
    let second = tangent_picks(sketch, &selection[1])?;
    let mut pair: Option<[TangentPick; 2]> = None;
    for first in &first {
        for second in &second {
            if first.entity.key() == second.entity.key() {
                continue;
            }
            let same =
                |a: &TangentPick, b: &TangentPick| a.entity == b.entity && a.endpoint == b.endpoint;
            if pair.as_ref().is_some_and(|pair| {
                (same(&pair[0], first) && same(&pair[1], second))
                    || (same(&pair[0], second) && same(&pair[1], first))
            }) {
                continue;
            }
            if pair.is_some() {
                return Err(CadError::Invalid(
                    "Tangent contact is ambiguous because a selected point belongs to several curves. Select curves directly and Cmd-click to add the second curve".into(),
                ));
            }
            pair = Some([first.clone(), second.clone()]);
        }
    }
    let Some(pair) = pair else {
        return Err(CadError::Invalid(
            "Tangent needs two distinct curves; both selections belong to the same curve. Select curves directly and Cmd-click to add the second curve".into(),
        ));
    };
    let entities = [pair[0].entity.clone(), pair[1].entity.clone()];
    if pair.iter().any(|pick| pick.endpoint.is_some()) {
        if entities
            .iter()
            .any(|entity| !matches!(entity, SketchEntity::Member { .. }))
        {
            return Err(CadError::Invalid(
                "Endpoint tangency needs two sketch curves. Use a construction line instead of a universe axis, or select curves directly for whole-curve tangency".into(),
            ));
        }
        let keys = entities
            .iter()
            .map(|entity| entity.key().into())
            .collect::<Vec<_>>();
        // Explicit endpoint picks define contact independently of the planner's
        // whole-member nearest/joined endpoint estimate. The other curve remains
        // free unless its endpoint was explicitly picked as well.
        let mut contact = crate::sketch_relations::Contact {
            parameters: [0.; 2],
            free: [true; 2],
        };
        for (index, pick) in pair.iter().enumerate() {
            if let Some(endpoint) = pick.endpoint {
                contact.parameters[index] = endpoint as f64;
                contact.free[index] = false;
            }
        }
        let mut relation = constraint("tangent", keys, 1.0.into());
        relation.contact = Some(contact);
        return Ok(vec![relation]);
    }
    relation_single_or_pair(sketch, &entities, "tangent")
}

fn reference_fan(
    sketch: &Sketch,
    selection: &[SketchEntity],
    kind: &str,
) -> Result<Vec<SketchConstraint>> {
    let mut batch = Vec::new();
    let reference = &selection[0];
    for entity in &selection[1..] {
        batch.extend(relation_single_or_pair(
            sketch,
            &[reference.clone(), entity.clone()],
            kind,
        )?);
    }
    Ok(batch)
}

struct CircularEqualityGroup {
    members: BTreeSet<Id>,
    reference: Id,
    roots: usize,
    dimension: Option<SketchConstraint>,
}

fn circular_equality_group(sketch: &Sketch, key: &str) -> Result<CircularEqualityGroup> {
    let circular = |key: &str| {
        matches!(
            sketch.members.get(key).map(|member| &member.curve),
            Some(SketchCurve::Circle { .. } | SketchCurve::Arc { .. })
        )
    };
    if !circular(key) {
        return Err(CadError::Invalid(
            "Equal Radius needs circles or circular arcs".into(),
        ));
    }
    let inactive = crate::sketch_transform::inactive_entities(sketch);
    let links = sketch
        .constraints
        .iter()
        .filter(|constraint| {
            ((constraint.kind == "equal_radius" && constraint.entities.len() == 2)
                || (constraint.kind == "symmetric_member" && constraint.entities.len() == 3))
                && !constraint
                    .entities
                    .iter()
                    .any(|entity| inactive.contains(entity))
                && circular(&constraint.entities[0])
                && circular(&constraint.entities[1])
        })
        .collect::<Vec<_>>();
    let mut members = BTreeSet::from([key.to_string()]);
    let mut pending = vec![key.to_string()];
    while let Some(key) = pending.pop() {
        for constraint in &links {
            let (first, second) = (&constraint.entities[0], &constraint.entities[1]);
            let peer = if first == &key {
                second
            } else if second == &key {
                first
            } else {
                continue;
            };
            if members.insert(peer.clone()) {
                pending.push(peer.clone());
            }
        }
    }
    let roots = members
        .iter()
        .filter(|member| {
            !links.iter().any(|constraint| {
                members.contains(&constraint.entities[0]) && constraint.entities[1] == **member
            })
        })
        .collect::<Vec<_>>();
    let reference = roots
        .first()
        .copied()
        .unwrap_or_else(|| members.first().unwrap())
        .clone();
    let dimension = sketch
        .constraints
        .iter()
        .find(|constraint| {
            !constraint.driven
                && matches!(constraint.kind.as_str(), "radius" | "diameter")
                && !constraint
                    .entities
                    .iter()
                    .any(|entity| inactive.contains(entity))
                && constraint
                    .entities
                    .first()
                    .is_some_and(|entity| members.contains(entity))
        })
        .cloned();
    Ok(CircularEqualityGroup {
        reference,
        roots: roots.len(),
        members,
        dimension,
    })
}

fn circular_equality_fan(
    sketch: &Sketch,
    selection: &[SketchEntity],
) -> Result<Vec<SketchConstraint>> {
    // Preserve the existing dimension precedence rule: the first stored
    // driving Radius/Diameter governs derived circular sizes, and residuals
    // validate every other dimension. The planner never deletes or converts a
    // driving dimension; conflicting existing dimensions may leave this draft
    // unsatisfied until the user edits them or makes a dimension driven.
    let reference = circular_equality_group(sketch, selection[0].key())?;
    let mut visited = reference.members.clone();
    let mut batch = Vec::new();
    for entity in &selection[1..] {
        let target = circular_equality_group(sketch, entity.key())?;
        if visited.contains(entity.key()) {
            continue;
        }
        // Old files can contain circular equality cycles or multiple directed
        // roots with a lexical source fallback. Adding one edge cannot preserve
        // that fallback reliably, so keep the source intact and report it.
        if reference.roots != 1 || target.roots != 1 {
            return Err(CadError::Invalid(
                "Equal Radius cannot merge a group with cyclic or competing size references; keep one reference member in each existing group".into(),
            ));
        }
        batch.push(constraint(
            "equal_radius",
            vec![reference.reference.clone(), target.reference.clone()],
            Expr("0 mm".into()),
        ));
        visited.extend(target.members);
    }
    Ok(batch)
}

fn point_carrier_batch(
    sketch: &Sketch,
    selection: &[SketchEntity],
    carrier: &SketchEntity,
) -> Result<Vec<SketchConstraint>> {
    let curve = sketch
        .members
        .get(carrier.key())
        .map(|member| &member.curve);
    let kind = if entity_line(sketch, carrier.key()).is_ok() {
        "point_on_line"
    } else {
        match curve {
            Some(SketchCurve::Circle { .. }) => "point_on_circle",
            Some(SketchCurve::Arc { .. }) => "point_on_arc",
            Some(_) => "point_on_curve",
            None => {
                return Err(CadError::Invalid(
                    "Coincident needs one selected line, axis, or curve".into(),
                ));
            }
        }
    };
    let mut batch = Vec::new();
    for point in selection.iter().filter(|entity| *entity != carrier) {
        entity_point(sketch, point.key())?;
        let mut relation = constraint(
            kind,
            vec![point.key().into(), carrier.key().into()],
            0.0.into(),
        );
        if kind == "point_on_curve" {
            relation.contact = Some(crate::sketch_relations::Contact {
                parameters: [0., 0.5],
                free: [false, true],
            });
        }
        batch.push(relation);
    }
    Ok(batch)
}

fn relation_single_or_pair(
    sketch: &Sketch,
    selection: &[SketchEntity],
    kind: &str,
) -> Result<Vec<SketchConstraint>> {
    if !selection
        .iter()
        .any(|e| matches!(e, SketchEntity::Point { .. } | SketchEntity::Member { .. }))
    {
        return Err(CadError::Invalid(
            "Select a sketch member or point with the reference".into(),
        ));
    }
    let keys = selection
        .iter()
        .map(|e| e.key().to_string())
        .collect::<Vec<_>>();
    let invalid = || {
        CadError::Invalid(
            "Select compatible members or points; Cmd-click to add to the selection".into(),
        )
    };
    let first = keys.first().ok_or_else(invalid)?;
    let mut out = vec![];
    let mut add = |kind: &str, entities: Vec<Id>, value: f64| {
        out.push(constraint(kind, entities, Expr(format!("{value} mm"))))
    };
    match kind {
        "fix" => {
            let mut points = BTreeSet::new();
            for key in &keys {
                if let Some(member) = sketch.members.get(key) {
                    points.extend(member.curve.points().into_iter().map(str::to_string));
                } else if sketch.points.contains_key(key) {
                    points.insert(key.clone());
                } else {
                    return Err(invalid());
                }
            }
            for key in points {
                let p = sketch.points[&key];
                add("fix_x", vec![key.clone()], p[0]);
                add("fix_y", vec![key], p[1]);
            }
        }
        "origin" | "on_x_axis" | "on_y_axis" => {
            if keys.len() != 1 {
                return Err(invalid());
            }
            let point = if let Some(member) = sketch.members.get(first) {
                member.curve.points()[0].to_string()
            } else {
                entity_point(sketch, first)?;
                first.clone()
            };
            if kind == "origin" {
                add("coincident", vec![point, ORIGIN.into()], 0.);
            } else {
                if matches!(
                    sketch.members.get(first).map(|m| &m.curve),
                    Some(SketchCurve::Line { .. })
                ) {
                    add(
                        if kind == "on_x_axis" {
                            "horizontal"
                        } else {
                            "vertical"
                        },
                        vec![first.clone()],
                        0.,
                    );
                }
                add(
                    if kind == "on_x_axis" {
                        "fix_y"
                    } else {
                        "fix_x"
                    },
                    vec![point],
                    0.,
                );
            }
        }
        "horizontal" | "vertical" => {
            if keys.len() == 1 {
                entity_line(sketch, first)?;
                add(kind, keys, 0.);
            } else if keys.len() == 2 {
                for key in &keys {
                    entity_point(sketch, key)?;
                }
                add(kind, keys, 0.);
            } else {
                return Err(invalid());
            }
        }
        "parallel" | "perpendicular" | "equal" => {
            if keys.len() != 2 {
                return Err(invalid());
            }
            for key in &keys {
                entity_line(sketch, key)?;
            }
            add(kind, keys, 0.);
        }
        "coincident" => {
            if keys.len() != 2 {
                return Err(invalid());
            }
            if keys.iter().all(|k| entity_point(sketch, k).is_ok()) {
                add(kind, keys, 0.);
            } else {
                let (p, l) = if entity_point(sketch, first).is_ok() {
                    (&keys[0], &keys[1])
                } else {
                    (&keys[1], &keys[0])
                };
                entity_point(sketch, p)?;
                if entity_line(sketch, l).is_ok() {
                    add("point_on_line", vec![p.clone(), l.clone()], 0.);
                } else if matches!(
                    sketch.members.get(l).map(|m| &m.curve),
                    Some(SketchCurve::Circle { .. } | SketchCurve::Arc { .. })
                ) {
                    add(
                        if matches!(sketch.members[l].curve, SketchCurve::Arc { .. }) {
                            "point_on_arc"
                        } else {
                            "point_on_circle"
                        },
                        vec![p.clone(), l.clone()],
                        0.,
                    );
                } else if sketch.members.contains_key(l) {
                    let mut c =
                        constraint("point_on_curve", vec![p.clone(), l.clone()], 0.0.into());
                    c.contact = Some(crate::sketch_relations::Contact {
                        parameters: [0., 0.5],
                        free: [false, true],
                    });
                    out.push(c);
                } else {
                    return Err(invalid());
                }
            }
        }
        "midpoint" => {
            if keys.len() != 2 {
                return Err(invalid());
            }
            let keys = if entity_line(sketch, first).is_ok() {
                vec![keys[1].clone(), first.clone()]
            } else {
                keys
            };
            entity_point(sketch, &keys[0])?;
            entity_line(sketch, &keys[1])?;
            add(kind, keys, 0.);
        }
        "symmetric_origin" => {
            if keys.len() != 2 {
                return Err(invalid());
            }
            for key in &keys {
                entity_point(sketch, key)?;
            }
            add(kind, keys, 0.);
        }
        "symmetric_axis" => {
            if keys.len() != 3 || keys.iter().collect::<BTreeSet<_>>().len() != 3 {
                return Err(CadError::Invalid("Select two points or compatible members, then a straight sketch member or sketch axis".into()));
            }
            let axes = keys
                .iter()
                .enumerate()
                .filter(|(_, key)| entity_line(sketch, key).is_ok())
                .map(|(i, _)| i)
                .collect::<Vec<_>>();
            let axis = keys
                .iter()
                .position(|key| matches!(key.as_str(), X_AXIS | Y_AXIS))
                .or_else(|| (axes.len() == 1).then(|| axes[0]))
                .or_else(|| {
                    let construction = axes
                        .iter()
                        .copied()
                        .filter(|i| {
                            sketch
                                .members
                                .get(&keys[*i])
                                .is_some_and(|m| m.construction)
                        })
                        .collect::<Vec<_>>();
                    (construction.len() == 1).then(|| construction[0])
                })
                .unwrap_or(2);
            let mut keys = keys;
            let axis = keys.remove(axis);
            keys.push(axis);
            entity_line(sketch, &keys[2])?;
            if sketch.members.contains_key(&keys[0]) && sketch.members.contains_key(&keys[1]) {
                let reverse = crate::sketch_relations::best_orientation(
                    sketch, &keys[0], &keys[1], &keys[2],
                )?;
                out.push(constraint(
                    "symmetric_member",
                    keys,
                    if reverse { (-1.).into() } else { 1.0.into() },
                ));
            } else {
                entity_point(sketch, &keys[0])?;
                entity_point(sketch, &keys[1])?;
                add(kind, keys, 0.);
            }
        }
        "point_on_arc" => {
            if keys.len() != 2 {
                return Err(invalid());
            }
            let (p, arc) = if matches!(
                sketch.members.get(first).map(|m| &m.curve),
                Some(SketchCurve::Arc { .. })
            ) {
                (&keys[1], first)
            } else {
                (first, &keys[1])
            };
            entity_point(sketch, p)?;
            if !matches!(
                sketch.members.get(arc).map(|m| &m.curve),
                Some(SketchCurve::Arc { .. })
            ) {
                return Err(invalid());
            }
            add(kind, vec![p.clone(), arc.clone()], 0.);
        }
        "concentric" | "equal_radius" => {
            if keys.len() != 2 {
                return Err(invalid());
            }
            for key in &keys {
                if !matches!(
                    sketch.members.get(key).map(|m| &m.curve),
                    Some(SketchCurve::Circle { .. } | SketchCurve::Arc { .. })
                ) {
                    return Err(CadError::Invalid(
                        "Select two circles or circular arcs".into(),
                    ));
                }
            }
            add(kind, keys, 0.);
        }
        "tangent" | "tangent_internal" => {
            if keys.len() != 2 {
                return Err(invalid());
            }
            let circle = |k: &str| {
                matches!(
                    sketch.members.get(k).map(|m| &m.curve),
                    Some(SketchCurve::Circle { .. } | SketchCurve::Arc { .. })
                )
            };
            if !((circle(first) && circle(&keys[1]))
                || (circle(first) && entity_line(sketch, &keys[1]).is_ok())
                || (circle(&keys[1]) && entity_line(sketch, first).is_ok()))
            {
                if kind == "tangent" && keys.iter().all(|key| sketch.members.contains_key(key)) {
                    let contact = crate::sketch_relations::initial_contact(sketch, &keys, false)?;
                    let mut c = constraint(kind, keys, 1.0.into());
                    c.contact = Some(contact);
                    out.push(c);
                    return Ok(out);
                }
                if kind == "tangent"
                    && selection
                        .iter()
                        .any(|entity| matches!(entity, SketchEntity::XAxis | SketchEntity::YAxis))
                {
                    return Err(CadError::Invalid(
                        "Tangent with a universe axis needs a circle or arc. Use a straight construction member for tangency with another curve type".into(),
                    ));
                }
                return Err(CadError::Invalid(
                    if kind == "tangent_internal" {
                        "Internal Tangent needs two circle or arc members; select curves directly rather than endpoints or centers"
                    } else {
                        "Tangent needs two sketch curves; select curves directly and Cmd-click to add the second curve"
                    }.into(),
                ));
            }
            if kind == "tangent_internal" && !(circle(first) && circle(&keys[1])) {
                return Err(CadError::Invalid(
                    "Internal tangency requires two circles or arcs".into(),
                ));
            }
            if !(circle(first) && circle(&keys[1])) {
                let (circular, line) = if circle(first) {
                    (first, &keys[1])
                } else {
                    (&keys[1], first)
                };
                let center = match &sketch.members[circular].curve {
                    SketchCurve::Circle { center, .. } => entity_point(sketch, center)?,
                    SketchCurve::Arc {
                        start,
                        through,
                        end,
                    } => {
                        arc_frame(
                            entity_point(sketch, start)?,
                            entity_point(sketch, through)?,
                            entity_point(sketch, end)?,
                        )?
                        .0
                    }
                    _ => unreachable!(),
                };
                let (a, b) = entity_line(sketch, line)?;
                let side = (center[0] - a[0]) * (b[1] - a[1]) - (center[1] - a[1]) * (b[0] - a[0]);
                out.push(constraint(
                    kind,
                    keys,
                    if side < 0. { (-1.).into() } else { 1.0.into() },
                ));
            } else {
                add(kind, keys, 0.);
            }
        }
        "curvature" => {
            if keys.len() != 2 || !keys.iter().all(|key| sketch.members.contains_key(key)) {
                return Err(CadError::Invalid(
                    "Curvature Continuous needs two curve members; select curves directly rather than endpoints or control points".into(),
                ));
            }
            let contact = crate::sketch_relations::initial_contact(sketch, &keys, true)?;
            let mut c = constraint(kind, keys, 1.0.into());
            c.contact = Some(contact);
            out.push(c);
        }
        "arc_center" => {
            if keys.len() != 2 {
                return Err(invalid());
            }
            let (point, arc) = if matches!(
                sketch.members.get(first).map(|m| &m.curve),
                Some(SketchCurve::Arc { .. })
            ) {
                (&keys[1], first)
            } else {
                (first, &keys[1])
            };
            entity_point(sketch, point)?;
            if !matches!(
                sketch.members.get(arc).map(|m| &m.curve),
                Some(SketchCurve::Arc { .. })
            ) {
                return Err(invalid());
            }
            add(kind, vec![point.clone(), arc.clone()], 0.);
        }
        _ => return Err(CadError::Unsupported(format!("Sketch relation {kind}"))),
    }
    if out.is_empty() {
        return Err(invalid());
    }
    Ok(out)
}
pub fn relation_with_geometry(
    doc: &Document,
    owner: &str,
    sketch: &Sketch,
    selection: &[SketchEntity],
    kind: &str,
) -> Result<Vec<SketchConstraint>> {
    let mut source = sketch.clone();
    apply_curve_dimensions(doc, owner, &mut source)?;
    sketch_edit::derive(doc, owner, &mut source)?;
    let mut relations = relation(&source, selection, kind)?;
    for c in &mut relations {
        crate::sketch_relations::seed_contact(doc, owner, &source, c)?;
    }
    Ok(relations)
}
pub fn dimensions(
    doc: &Document,
    owner: &str,
    sketch: &Sketch,
    selection: &[SketchEntity],
) -> Result<Vec<DimensionChoice>> {
    if selection.len() == 1
        && let SketchEntity::Constraint { constraint } = &selection[0]
    {
        let c = sketch
            .constraints
            .iter()
            .find(|c| &c.id == constraint)
            .ok_or_else(|| CadError::Invalid("Dimension no longer exists".into()))?;
        if is_dimension(&c.kind) {
            return Ok(vec![DimensionChoice {
                kind: c.kind.clone(),
                label: label(&c.kind).into(),
                entities: c.entities.clone(),
                value: c.value.clone(),
                distance_side: c.distance_side,
            }]);
        }
    }
    if !selection
        .iter()
        .any(|e| matches!(e, SketchEntity::Point { .. } | SketchEntity::Member { .. }))
    {
        return Err(CadError::Invalid(
            "Select a sketch member or point with the reference to dimension".into(),
        ));
    }
    let mut evaluated = sketch.clone();
    apply_curve_dimensions(doc, owner, &mut evaluated)?;
    sketch_edit::derive(doc, owner, &mut evaluated)?;
    let sketch = &evaluated;
    let inactive = crate::sketch_transform::inactive_entities(sketch);
    if selection
        .iter()
        .any(|entity| inactive.contains(entity.key()))
    {
        return Err(CadError::Invalid(
            "Select active sketch geometry to dimension".into(),
        ));
    }
    let keys = selection
        .iter()
        .map(|e| e.key().to_string())
        .collect::<Vec<_>>();
    let first = keys
        .first()
        .ok_or_else(|| CadError::Invalid("Select a line, curve, or points to dimension".into()))?;
    let mut out = vec![];
    let mut add = |kind: &str, entities: Vec<Id>, value: f64| {
        out.push(DimensionChoice {
            kind: kind.into(),
            label: label(kind).into(),
            entities,
            value: Expr(if dimension_unit(kind) == Dimension::Angle {
                format!("{} deg", value.to_degrees())
            } else {
                format!("{value} mm")
            }),
            distance_side: None,
        })
    };
    if keys.len() == 1 {
        if let Ok((a, b)) = entity_line(sketch, first) {
            if !sketch.members.contains_key(first) {
                return Err(CadError::Invalid(
                    "Select a sketch member with the axis to dimension it".into(),
                ));
            }
            let delta = [b[0] - a[0], b[1] - a[1]];
            let mut entities = sketch.members[first]
                .curve
                .points()
                .into_iter()
                .map(str::to_string)
                .collect::<Vec<_>>();
            if delta[1].abs() < 1e-6 {
                if delta[0] < 0. {
                    entities.reverse();
                }
                add("distance_x", entities, delta[0].abs());
            } else if delta[0].abs() < 1e-6 {
                if delta[1] < 0. {
                    entities.reverse();
                }
                add("distance_y", entities, delta[1].abs());
            }
            add("length", keys.clone(), delta[0].hypot(delta[1]));
        } else if let Ok((_, radius)) =
            circle_with(doc, owner, sketch, first, &|k| entity_point(sketch, k))
        {
            if matches!(sketch.members[first].curve, SketchCurve::Circle { .. }) {
                add("diameter", keys.clone(), radius * 2.);
            }
            add("radius", keys.clone(), radius);
            if let SketchCurve::Arc {
                start,
                through,
                end,
            } = &sketch.members[first].curve
            {
                let (_, _, _, sweep) = arc_frame(
                    entity_point(sketch, start)?,
                    entity_point(sketch, through)?,
                    entity_point(sketch, end)?,
                )?;
                add("arc_length", keys.clone(), radius * sweep.abs());
                add("arc_angle", keys.clone(), sweep.abs());
            }
        } else if let Some(SketchMember {
            curve:
                SketchCurve::Ellipse {
                    major,
                    minor,
                    angle,
                    ..
                },
            ..
        }) = sketch.members.get(first)
        {
            for (kind, expr) in [
                ("major_radius", major),
                ("minor_radius", minor),
                ("rotation", angle),
            ] {
                add(
                    kind,
                    keys.clone(),
                    expression::value(doc, owner, expr, dimension_unit(kind))?,
                );
            }
        } else {
            let p = entity_point(sketch, first)?;
            add("fix_x", keys.clone(), p[0]);
            add("fix_y", keys.clone(), p[1]);
        }
    } else if keys.len() == 2 {
        match (entity_line(sketch, first), entity_line(sketch, &keys[1])) {
            (Ok((a, b)), Ok((c, d))) => {
                let u = [b[0] - a[0], b[1] - a[1]];
                let v = [d[0] - c[0], d[1] - c[1]];
                let cross = u[0] * v[1] - u[1] * v[0];
                let dot = u[0] * v[0] + u[1] * v[1];
                if u[0].hypot(u[1]) < 1e-9 || v[0].hypot(v[1]) < 1e-9 {
                    return Err(CadError::Invalid(
                        "Cannot dimension a zero-length line".into(),
                    ));
                }
                if cross.abs() < 1e-6 * u[0].hypot(u[1]) * v[0].hypot(v[1]) {
                    add(
                        "line_distance",
                        keys.clone(),
                        ((c[0] - a[0]) * u[1] - (c[1] - a[1]) * u[0]).abs() / u[0].hypot(u[1]),
                    );
                }
                add("angle", keys, cross.atan2(dot));
            }
            (Ok((a, b)), Err(_)) | (Err(_), Ok((a, b))) => {
                let mut keys = if entity_line(sketch, first).is_ok() {
                    vec![keys[1].clone(), first.clone()]
                } else {
                    keys
                };
                if let Some(SketchMember {
                    curve: SketchCurve::Circle { center, .. } | SketchCurve::Ellipse { center, .. },
                    ..
                }) = sketch.members.get(&keys[0])
                {
                    keys[0] = center.clone();
                }
                let p = entity_point(sketch, &keys[0])?;
                let u = [b[0] - a[0], b[1] - a[1]];
                if u[0].hypot(u[1]) < 1e-9 {
                    return Err(CadError::Invalid(
                        "Cannot dimension a zero-length line".into(),
                    ));
                }
                let signed = ((p[0] - a[0]) * u[1] - (p[1] - a[1]) * u[0]) / u[0].hypot(u[1]);
                if !signed.is_finite() {
                    return Err(CadError::Invalid(
                        "Perpendicular dimension is not finite".into(),
                    ));
                }
                add("point_line_distance", keys, signed.abs());
                out.last_mut().unwrap().distance_side = Some(if signed < 0. {
                    DistanceSide::Left
                } else {
                    DistanceSide::Right
                });
            }
            _ => {
                let a = entity_point(sketch, first)?;
                let b = entity_point(sketch, &keys[1])?;
                add("distance", keys.clone(), (b[0] - a[0]).hypot(b[1] - a[1]));
                add("distance_x", keys.clone(), b[0] - a[0]);
                add("distance_y", keys, b[1] - a[1]);
            }
        }
    } else {
        return Err(CadError::Invalid(
            "Smart Dimension accepts one member or two entities".into(),
        ));
    }
    Ok(out)
}

/// Resolve driven curve sizes in the derived sketch, retaining expressions and relation intent.
pub fn apply_curve_dimensions(_doc: &Document, _owner: &str, sketch: &mut Sketch) -> Result<()> {
    fn radius(source: &Sketch, key: &str) -> Result<Expr> {
        let group = circular_equality_group(source, key)?;
        // A dimension on any circle drives its whole equality group. Redundant
        // cycles are valid; all remaining dimensions are checked by residuals.
        if let Some(c) = group.dimension {
            return Ok(if c.kind == "diameter" {
                Expr(format!("({}) / 2", c.value.0))
            } else {
                c.value.clone()
            });
        }
        match &source.members[&group.reference].curve {
            SketchCurve::Circle { radius, .. } => Ok(radius.clone()),
            SketchCurve::Arc {
                start,
                through,
                end,
            } => {
                let (_, radius, _, _) = arc_frame(
                    entity_point(source, start)?,
                    entity_point(source, through)?,
                    entity_point(source, end)?,
                )?;
                Ok(Expr(format!("{radius} mm")))
            }
            _ => Err(CadError::Invalid(
                "Radius relation requires a circle".into(),
            )),
        }
    }
    let mut source = sketch.clone();
    let inactive = crate::sketch_transform::inactive_entities(sketch);
    source
        .constraints
        .retain(|constraint| !constraint.entities.iter().any(|key| inactive.contains(key)));
    for (key, member) in &mut sketch.members {
        if crate::sketch_edit::driven_member(&source, key) {
            continue;
        }
        match &mut member.curve {
            SketchCurve::Circle { radius: r, .. } => *r = radius(&source, key)?,
            SketchCurve::Ellipse {
                major,
                minor,
                angle,
                ..
            } => {
                for (kind, value) in [
                    ("major_radius", major),
                    ("minor_radius", minor),
                    ("rotation", angle),
                ] {
                    if let Some(c) = source
                        .constraints
                        .iter()
                        .find(|c| !c.driven && c.kind == kind && c.entities.first() == Some(key))
                    {
                        *value = c.value.clone();
                    }
                }
            }
            _ => {}
        }
    }
    crate::sketch_relations::derive_ellipses(_doc, _owner, sketch)
}
#[derive(Clone, Debug)]
pub struct ConstraintResidual {
    pub constraint: Id,
    pub kind: String,
    /// Length residuals in mm; angular residuals multiplied by 100 mm/radian.
    pub values: Vec<f64>,
}

fn wrapped_angle(angle: f64) -> f64 {
    (angle + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU) - std::f64::consts::PI
}

fn unoriented_angle(angle: f64) -> f64 {
    (angle + std::f64::consts::FRAC_PI_2).rem_euclid(std::f64::consts::PI)
        - std::f64::consts::FRAC_PI_2
}

pub fn residuals(
    doc: &Document,
    owner: &str,
    sketch: &Sketch,
    point: impl Fn(&str) -> Result<[f64; 2]>,
) -> Result<Vec<f64>> {
    Ok(residual_blocks(doc, owner, sketch, point)?
        .into_iter()
        .flat_map(|b| b.values)
        .collect())
}

pub fn residual_blocks(
    doc: &Document,
    owner: &str,
    sketch: &Sketch,
    point: impl Fn(&str) -> Result<[f64; 2]>,
) -> Result<Vec<ConstraintResidual>> {
    let get_point = |key: &str| point_with(sketch, key, &point);
    let get_line = |key: &str| line_with(sketch, key, &point);
    let circle = |key: &str| circle_with(doc, owner, sketch, key, &point);
    let mut blocks = vec![];
    let inactive = crate::sketch_transform::inactive_entities(sketch);
    for c in sketch
        .constraints
        .iter()
        .filter(|c| !c.driven && !c.entities.iter().any(|key| inactive.contains(key)))
    {
        let mut r = Vec::new();
        let key = |i: usize| {
            c.entities
                .get(i)
                .map(String::as_str)
                .ok_or_else(|| CadError::Invalid(format!("Missing entity for {}", label(&c.kind))))
        };
        let q = if is_dimension(&c.kind) {
            expression::value(doc, owner, &c.value, dimension_unit(&c.kind))?
        } else {
            0.
        };
        if matches!(c.kind.as_str(), "line_distance" | "point_line_distance") && q < 0. {
            return Err(CadError::Invalid("Distance cannot be negative".into()));
        }
        let line_pair = || -> Result<_> {
            if c.entities.len() == 4 {
                Ok((
                    (get_point(key(0)?)?, get_point(key(1)?)?),
                    (get_point(key(2)?)?, get_point(key(3)?)?),
                ))
            } else {
                Ok((get_line(key(0)?)?, get_line(key(1)?)?))
            }
        };
        let endpoints = || -> Result<_> {
            if c.entities.len() == 1 {
                get_line(key(0)?)
            } else {
                Ok((get_point(key(0)?)?, get_point(key(1)?)?))
            }
        };
        if c.contact.is_some() {
            r.extend(crate::sketch_relations::contact_residuals(
                doc, owner, sketch, c, &get_point,
            )?);
            blocks.push(ConstraintResidual {
                constraint: c.id.clone(),
                kind: c.kind.clone(),
                values: r,
            });
            continue;
        }
        match c.kind.as_str() {
            "fix_x" => r.push(get_point(key(0)?)?[0] - q),
            "fix_y" => r.push(get_point(key(0)?)?[1] - q),
            "coincident" => {
                let a = get_point(key(0)?)?;
                let b = get_point(key(1)?)?;
                r.extend([a[0] - b[0], a[1] - b[1]]);
            }
            "horizontal" | "vertical" | "distance" | "distance_x" | "distance_y" | "length" => {
                let (a, b) = endpoints()?;
                let u = [b[0] - a[0], b[1] - a[1]];
                r.push(match c.kind.as_str() {
                    "horizontal" => u[1],
                    "vertical" => u[0],
                    "distance_x" => u[0] - q,
                    "distance_y" => u[1] - q,
                    _ => {
                        if q <= 0. {
                            return Err(CadError::Invalid(
                                "Length and distance must be positive".into(),
                            ));
                        }
                        u[0].hypot(u[1]) - q
                    }
                });
            }
            "parallel" | "perpendicular" | "equal" | "angle" | "line_distance" => {
                let ((a, b), (c1, d)) = line_pair()?;
                let u = [b[0] - a[0], b[1] - a[1]];
                let v = [d[0] - c1[0], d[1] - c1[1]];
                let lu = u[0].hypot(u[1]);
                let lv = v[0].hypot(v[1]);
                if lu < 1e-9 || lv < 1e-9 {
                    return Err(CadError::Invalid("Zero-length constrained line".into()));
                }
                let cross = (u[0] * v[1] - u[1] * v[0]) / (lu * lv);
                let dot = (u[0] * v[0] + u[1] * v[1]) / (lu * lv);
                match c.kind.as_str() {
                    "parallel" => r.push(100. * unoriented_angle(cross.atan2(dot))),
                    "perpendicular" => r.push(
                        100. * unoriented_angle(cross.atan2(dot) - std::f64::consts::FRAC_PI_2),
                    ),
                    "equal" => r.push(lu - lv),
                    "angle" => r.push(100. * wrapped_angle(cross.atan2(dot) - q)),
                    _ => r.extend([
                        100. * unoriented_angle(cross.atan2(dot)),
                        ((c1[0] - a[0]) * u[1] - (c1[1] - a[1]) * u[0]).abs() / lu - q,
                    ]),
                };
            }
            "midpoint" => {
                let p = get_point(key(0)?)?;
                let (a, b) = get_line(key(1)?)?;
                r.extend([p[0] - (a[0] + b[0]) / 2., p[1] - (a[1] + b[1]) / 2.]);
            }
            "symmetric_origin" => {
                let a = get_point(key(0)?)?;
                let b = get_point(key(1)?)?;
                r.extend([a[0] + b[0], a[1] + b[1]]);
            }
            "symmetric_axis" => {
                let a = get_point(key(0)?)?;
                let b = get_point(key(1)?)?;
                let (start, end) = get_line(key(2)?)?;
                let reflected = crate::sketch_relations::reflect(a, start, end)?;
                r.extend([reflected[0] - b[0], reflected[1] - b[1]]);
            }
            "symmetric_member" => {
                let direction = expression::value(doc, owner, &c.value, Dimension::Scalar)?;
                if direction != 1. && direction != -1. {
                    return Err(CadError::Invalid(
                        "Member symmetry orientation must be 1 or -1".into(),
                    ));
                }
                r.extend(crate::sketch_relations::member_residuals(
                    doc,
                    owner,
                    sketch,
                    &c.entities,
                    direction < 0.,
                    &get_point,
                )?);
            }
            "point_on_line" | "point_line_distance" => {
                let p = get_point(key(0)?)?;
                let (a, b) = get_line(key(1)?)?;
                let u = [b[0] - a[0], b[1] - a[1]];
                let length = u[0].hypot(u[1]);
                if length < 1e-9 {
                    return Err(CadError::Invalid("Zero-length constrained line".into()));
                }
                let d = ((p[0] - a[0]) * u[1] - (p[1] - a[1]) * u[0]) / length;
                r.push(if c.kind == "point_on_line" {
                    d
                } else {
                    match c.distance_side {
                        Some(DistanceSide::Left) => d + q,
                        Some(DistanceSide::Right) => d - q,
                        None => d.abs() - q,
                    }
                });
            }
            "point_on_circle" | "point_on_arc" => {
                let p = get_point(key(0)?)?;
                let (center, radius) = circle(key(1)?)?;
                r.push((p[0] - center[0]).hypot(p[1] - center[1]) - radius);
                if let Some(excess) =
                    crate::sketch_relations::arc_excess(sketch, key(1)?, p, &get_point)?
                {
                    r.push(excess);
                } else if c.kind == "point_on_arc" {
                    return Err(CadError::Invalid(
                        "Point on Arc requires a bounded circular arc".into(),
                    ));
                }
            }
            "arc_length" | "arc_angle" => {
                let Some(SketchCurve::Arc {
                    start,
                    through,
                    end,
                }) = sketch.members.get(key(0)?).map(|m| &m.curve)
                else {
                    return Err(CadError::Invalid(
                        "Arc dimensions require a circular arc".into(),
                    ));
                };
                if q <= 0. || (c.kind == "arc_angle" && q >= std::f64::consts::TAU) {
                    return Err(CadError::Invalid("Arc length must be positive; an included angle must be between 0 and 360 degrees".into()));
                }
                let (_, radius, _, sweep) =
                    arc_frame(get_point(start)?, get_point(through)?, get_point(end)?)?;
                r.push(if c.kind == "arc_length" {
                    radius * sweep.abs() - q
                } else {
                    100. * (sweep.abs() - q)
                });
            }
            "radius" | "diameter" => {
                if q <= 0. {
                    return Err(CadError::Invalid(
                        "Radius and diameter must be positive".into(),
                    ));
                }
                let (_, radius) = circle(key(0)?)?;
                r.push(radius * if c.kind == "diameter" { 2. } else { 1. } - q);
            }
            "major_radius" | "minor_radius" | "rotation" => {
                let Some(SketchMember {
                    curve:
                        SketchCurve::Ellipse {
                            major,
                            minor,
                            angle,
                            ..
                        },
                    ..
                }) = sketch.members.get(key(0)?)
                else {
                    return Err(CadError::Invalid("Select an ellipse".into()));
                };
                let expr = match c.kind.as_str() {
                    "major_radius" => major,
                    "minor_radius" => minor,
                    _ => angle,
                };
                let residual = expression::value(doc, owner, expr, dimension_unit(&c.kind))? - q;
                r.push(if c.kind == "rotation" {
                    100. * wrapped_angle(residual)
                } else {
                    residual
                });
            }
            "equal_radius" | "concentric" => {
                let (a, ra) = circle(key(0)?)?;
                let (b, rb) = circle(key(1)?)?;
                if c.kind == "concentric" {
                    r.extend([a[0] - b[0], a[1] - b[1]]);
                } else {
                    r.push(ra - rb);
                }
            }
            "arc_center" => {
                let p = get_point(key(0)?)?;
                let (c, _) = circle(key(1)?)?;
                r.extend([p[0] - c[0], p[1] - c[1]]);
            }
            "tangent" | "tangent_internal" => match (circle(key(0)?), circle(key(1)?)) {
                (Ok((a, ra)), Ok((b, rb))) => {
                    let separation = if c.kind == "tangent_internal" {
                        (ra - rb).abs()
                    } else {
                        ra + rb
                    };
                    if separation < 1e-9 {
                        return Err(CadError::Invalid(
                            "Internal tangent circles/arcs need different radii".into(),
                        ));
                    }
                    let distance = (a[0] - b[0]).hypot(a[1] - b[1]);
                    if distance < 1e-9 {
                        return Err(CadError::Invalid(
                            "Tangent circular members need distinct initial centers".into(),
                        ));
                    }
                    r.push(distance - separation);
                    let direction = [(b[0] - a[0]) / distance, (b[1] - a[1]) / distance];
                    let sign = if c.kind == "tangent_internal" && ra < rb {
                        -1.
                    } else {
                        1.
                    };
                    let contact_a = [
                        a[0] + direction[0] * ra * sign,
                        a[1] + direction[1] * ra * sign,
                    ];
                    let other_sign = if c.kind == "tangent_internal" {
                        sign
                    } else {
                        -1.
                    };
                    let contact_b = [
                        b[0] + direction[0] * rb * other_sign,
                        b[1] + direction[1] * rb * other_sign,
                    ];
                    for (key, contact) in [(key(0)?, contact_a), (key(1)?, contact_b)] {
                        if let Some(excess) =
                            crate::sketch_relations::arc_excess(sketch, key, contact, &get_point)?
                        {
                            r.push(excess);
                        }
                    }
                }
                (Ok((p, radius)), _) | (_, Ok((p, radius))) => {
                    let line = if get_line(key(0)?).is_ok() {
                        key(0)?
                    } else {
                        key(1)?
                    };
                    let (a, b) = get_line(line)?;
                    let u = [b[0] - a[0], b[1] - a[1]];
                    if u[0].hypot(u[1]) < 1e-9 {
                        return Err(CadError::Invalid("Zero-length tangent line".into()));
                    }
                    let length = u[0].hypot(u[1]);
                    let signed = ((p[0] - a[0]) * u[1] - (p[1] - a[1]) * u[0]) / length;
                    let branch = match c.value.0.as_str() {
                        "1" => Some(1.),
                        "-1" => Some(-1.),
                        _ => None,
                    };
                    r.push(if let Some(branch) = branch {
                        signed - branch * radius
                    } else {
                        signed.abs() - radius
                    });
                    let along = ((p[0] - a[0]) * u[0] + (p[1] - a[1]) * u[1]) / (length * length);
                    let contact = [a[0] + u[0] * along, a[1] + u[1] * along];
                    let circular = if circle(key(0)?).is_ok() {
                        key(0)?
                    } else {
                        key(1)?
                    };
                    if let Some(excess) =
                        crate::sketch_relations::arc_excess(sketch, circular, contact, &get_point)?
                    {
                        r.push(excess);
                    }
                    if let Some(excess) =
                        crate::sketch_relations::line_excess(sketch, line, contact, &get_point)?
                    {
                        r.push(excess);
                    }
                }
                _ => {
                    return Err(CadError::Unsupported(
                        "Tangent relation requires a circle/arc and line, or two circles/arcs"
                            .into(),
                    ));
                }
            },
            _ => {
                return Err(CadError::Unsupported(format!(
                    "Sketch constraint {}",
                    c.kind
                )));
            }
        }
        blocks.push(ConstraintResidual {
            constraint: c.id.clone(),
            kind: c.kind.clone(),
            values: r,
        });
    }
    Ok(blocks)
}
