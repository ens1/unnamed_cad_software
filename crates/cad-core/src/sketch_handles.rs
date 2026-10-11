//! Version 1 sketch handle intent, independent of viewport pixels and solver implementations.
use crate::{
    sketch_actions,
    solver::{ConstraintSolver, SketchDragTarget, SolveReport},
    *,
};
use serde::{Deserialize, Serialize};

pub const CONTRACT_VERSION: u32 = 2;
pub const MIN_HANDLE_SIZE_MM: f64 = 1e-7;
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HandleTarget {
    Point { point: Id },
    BlockControl { instance: Id, point: Id, output: Id },
    MoveMember { member: Id },
    CircleRadius { member: Id },
    EllipseMajor { member: Id },
    EllipseMinor { member: Id },
    EllipseRotation { member: Id },
}
impl HandleTarget {
    pub fn entity(&self) -> sketch_actions::SketchEntity {
        match self {
            Self::BlockControl { output: point, .. } | Self::Point { point } => {
                sketch_actions::SketchEntity::Point {
                    point: point.clone(),
                }
            }
            Self::MoveMember { member }
            | Self::CircleRadius { member }
            | Self::EllipseMajor { member }
            | Self::EllipseMinor { member }
            | Self::EllipseRotation { member } => sketch_actions::SketchEntity::Member {
                member: member.clone(),
            },
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SketchHandle {
    pub id: Id,
    pub target: HandleTarget,
    /// Sketch-plane coordinates, in canonical millimeters.
    pub position: [f64; 2],
    pub label: String,
    pub role: String,
    pub disabled_reason: Option<String>,
}
fn literal(expr: &Expr) -> bool {
    let text = expr.0.trim();
    let number = ["mm", "cm", "m", "in", "ft", "deg", "rad"]
        .into_iter()
        .find_map(|unit| text.strip_suffix(unit))
        .unwrap_or(text)
        .trim();
    number.parse::<f64>().is_ok_and(f64::is_finite)
}
fn driven(source: &Sketch, member: &str, kind: &str, expression: &Expr) -> Option<String> {
    if crate::sketch_edit::driven_member(source, member) {
        return Some("Driven by a sketch operation or source edge. Edit its operation parameters or detach the source relation.".into());
    }
    let dimension = source.constraints.iter().find(|c| {
        !c.driven
            && c.entities.first().is_some_and(|e| e == member)
            && (c.kind == kind || (kind == "radius" && c.kind == "diameter"))
    });
    if let Some(c) = dimension {
        Some(format!(
            "Driven by {}. Edit its dimension to change this handle.",
            sketch_actions::label(&c.kind)
        ))
    } else if kind == "radius"
        && source
            .constraints
            .iter()
            .any(|c| c.kind == "equal_radius" && c.entities.get(1).is_some_and(|e| e == member))
    {
        Some("Driven by Equal Radius. Resize the driving circle or edit the relation.".into())
    } else if !literal(expression) {
        Some("Driven by a parameter or expression. Edit that value to change this handle.".into())
    } else {
        None
    }
}
pub fn handles(doc: &Document, owner: &str, source: &Sketch) -> Result<Vec<SketchHandle>> {
    let mut evaluated = source.clone();
    sketch_actions::apply_curve_dimensions(doc, owner, &mut evaluated)?;
    crate::sketch_edit::derive(doc, owner, &mut evaluated)?;
    let derived = crate::sketch_edit::driven_points(source);
    let suppressed_members = crate::sketch_transform::suppressed_members(source);
    let suppressed_points = crate::sketch_transform::suppressed_points(source);
    let text_controls = source
        .texts
        .values()
        .flat_map(crate::sketch_text::Definition::driven_points)
        .collect::<std::collections::BTreeSet<_>>();
    let mut out = vec![];
    for (point, position) in &evaluated.points {
        if suppressed_points.contains(point) || text_controls.contains(point) {
            continue;
        }
        let mut role = "point";
        if source
            .splines
            .values()
            .any(|definition| definition.fit_points().contains(point))
        {
            role = "fit";
        }
        for member in source.members.values() {
            match &member.curve {
                SketchCurve::Circle { center, .. } | SketchCurve::Ellipse { center, .. }
                    if center == point =>
                {
                    role = "center"
                }
                SketchCurve::Bezier { points } | SketchCurve::Nurbs { points, .. }
                    if role != "fit"
                        && points.len() > 2
                        && points[1..points.len() - 1].contains(point) =>
                {
                    role = "control"
                }
                _ => {}
            }
        }
        for definition in source.conics.values() {
            use crate::sketch_conic::Definition;
            match definition {
                Definition::Conic { points, .. } | Definition::Parabola { points }
                    if &points[1] == point =>
                {
                    role = "tangent";
                }
                Definition::EllipseArc {
                    center,
                    major,
                    minor,
                    ..
                } => {
                    if center == point {
                        role = "center";
                    } else if major == point || minor == point {
                        role = "axis";
                    }
                }
                Definition::ArcSlot {
                    center, start, end, ..
                } => {
                    if center == point {
                        role = "center";
                    } else if start == point {
                        role = "radius-start";
                    } else if end == point {
                        role = "direction";
                    }
                }
                _ => {}
            }
        }
        if source
            .constraints
            .iter()
            .any(|c| c.kind == "midpoint" && c.entities.first() == Some(point))
        {
            role = "center";
        }
        if source
            .texts
            .values()
            .any(|definition| &definition.anchor == point)
        {
            role = "text-anchor";
        }
        if source
            .equations
            .values()
            .any(|definition| &definition.anchor == point)
        {
            role = "equation-anchor";
        }
        if source
            .block_instances
            .values()
            .any(|instance| &instance.anchor == point)
        {
            role = "block-anchor";
        }
        let x = source.constraints.iter().any(|c| {
            !c.driven
                && matches!(c.kind.as_str(), "origin" | "fix_x")
                && c.entities.first() == Some(point)
        });
        let y = source.constraints.iter().any(|c| {
            !c.driven
                && matches!(c.kind.as_str(), "origin" | "fix_y")
                && c.entities.first() == Some(point)
        });
        out.push(SketchHandle {
            id: format!("{}::handle::{point}", source.id),
            target: HandleTarget::Point {
                point: point.clone(),
            },
            position: *position,
            label: match role {
                "center" => "Move center",
                "control" => "Move spline control",
                "fit" => "Move spline fit point",
                "tangent" => "Move tangent intersection",
                "axis" => "Move ellipse axis handle",
                "radius-start" => "Move slot radius/start handle",
                "direction" => "Move slot end direction",
                "text-anchor"=>"Move text anchor",
                "equation-anchor"=>"Move equation curve anchor",
                "block-anchor"=>"Move block insertion anchor",
                _ => "Move point",
            }
            .into(),
            role: role.into(),
            disabled_reason: if derived.contains(point) { Some("Driven by a source edge or sketch operation. Edit its source points or parameters to move this point.".into()) } else { (x && y).then(|| "Position is fixed. Edit its dimensions or relations to move it.".into()) },
        });
    }
    for (member, record) in &evaluated.members {
        if suppressed_members.contains(member) {
            continue;
        }
        let original = &source.members[member].curve;
        let mut add = |suffix: &str, target, position, label: &str, role: &str, disabled_reason| {
            out.push(SketchHandle {
                id: format!("{}::handle::{member}::{suffix}", source.id),
                target,
                position,
                label: label.into(),
                role: role.into(),
                disabled_reason,
            });
        };
        match &record.curve {
            SketchCurve::Circle { center, radius } => {
                let c = evaluated.points[center];
                let r = expression::value(doc, owner, radius, Dimension::Length)?;
                let source_radius = if let SketchCurve::Circle { radius, .. } = original {
                    radius
                } else {
                    radius
                };
                for (i, axis) in [[1., 0.], [0., 1.], [-1., 0.], [0., -1.]]
                    .into_iter()
                    .enumerate()
                {
                    add(
                        &format!("radius-{i}"),
                        HandleTarget::CircleRadius {
                            member: member.clone(),
                        },
                        [c[0] + r * axis[0], c[1] + r * axis[1]],
                        "Resize radius",
                        "size",
                        driven(source, member, "radius", source_radius),
                    );
                }
            }
            SketchCurve::Ellipse {
                center,
                major,
                minor,
                angle,
            } => {
                let c = evaluated.points[center];
                let a = expression::value(doc, owner, major, Dimension::Length)?;
                let b = expression::value(doc, owner, minor, Dimension::Length)?;
                let t = expression::value(doc, owner, angle, Dimension::Angle)?;
                let SketchCurve::Ellipse {
                    major: source_major,
                    minor: source_minor,
                    angle: source_angle,
                    ..
                } = original
                else {
                    unreachable!()
                };
                for sign in [-1., 1.] {
                    add(
                        &format!("major-{sign}"),
                        HandleTarget::EllipseMajor {
                            member: member.clone(),
                        },
                        [c[0] + sign * a * t.cos(), c[1] + sign * a * t.sin()],
                        "Resize major radius",
                        "size",
                        driven(source, member, "major_radius", source_major),
                    );
                    add(
                        &format!("minor-{sign}"),
                        HandleTarget::EllipseMinor {
                            member: member.clone(),
                        },
                        [c[0] - sign * b * t.sin(), c[1] + sign * b * t.cos()],
                        "Resize minor radius",
                        "size",
                        driven(source, member, "minor_radius", source_minor),
                    );
                }
                let rotation = t + std::f64::consts::FRAC_PI_4;
                let radius = a.max(b) * 1.35;
                add(
                    "rotation",
                    HandleTarget::EllipseRotation {
                        member: member.clone(),
                    },
                    [
                        c[0] + radius * rotation.cos(),
                        c[1] + radius * rotation.sin(),
                    ],
                    "Rotate ellipse",
                    "rotation",
                    driven(source, member, "rotation", source_angle),
                );
            }
            _ => {
                let points = record.curve.points();
                let mut c = [0.; 2];
                for key in &points {
                    for (i, value) in c.iter_mut().enumerate() {
                        *value += evaluated.points[*key][i] / points.len() as f64;
                    }
                }
                if let SketchCurve::Arc {
                    start,
                    through,
                    end,
                } = &record.curve
                {
                    c = sketch_geometry::arc_frame(
                        evaluated.points[start],
                        evaluated.points[through],
                        evaluated.points[end],
                    )?
                    .0;
                }
                add(
                    "move",
                    HandleTarget::MoveMember {
                        member: member.clone(),
                    },
                    c,
                    "Move member",
                    "move",
                    (crate::sketch_edit::driven_member(source, member) || points.iter().any(|key| derived.contains(*key))).then(|| "Driven by a sketch operation or source edge. Edit that operation to move this member.".into()),
                );
            }
        }
    }
    Ok(out)
}
/// Source controls projected through one proven positive-uniform block instance.
pub fn block_controls(
    doc: &Document,
    owner: &str,
    sketch: &Sketch,
    key: &str,
) -> Result<Vec<SketchHandle>> {
    let instance = sketch
        .block_instances
        .get(key)
        .ok_or_else(|| CadError::Invalid("Block instance is missing".into()))?;
    if instance.suppressed {
        return Ok(vec![]);
    }
    let definition = sketch
        .blocks
        .get(&instance.definition)
        .ok_or_else(|| CadError::Invalid("Block definition is missing".into()))?;
    let mut local = crate::sketch_blocks::source(&instance.definition, definition);
    crate::solver::NativeSolver.solve_sketch(doc, owner, &mut local)?;
    let (matrix, offset) = crate::sketch_blocks::affine(doc, owner, sketch, instance)?;
    handles(doc, owner, &local)?
        .into_iter()
        .filter(|handle| matches!(handle.target, HandleTarget::Point { .. }))
        .map(|handle| {
            let HandleTarget::Point { point } = handle.target else {
                unreachable!()
            };
            let output = instance.points.get(&point).ok_or_else(|| {
                CadError::Invalid("Block source control has no output mapping".into())
            })?;
            let position = matrix * nalgebra::Vector2::from(handle.position) + offset;
            if position.iter().any(|value| !value.is_finite()) {
                return Err(CadError::Invalid(
                    "Block projected source control coordinate overflow".into(),
                ));
            }
            Ok(SketchHandle {
                id: format!("{}::block-control::{key}::{point}", sketch.id),
                target: HandleTarget::BlockControl {
                    instance: key.into(),
                    point,
                    output: output.clone(),
                },
                position: [position.x, position.y],
                label: "Edit local block source control".into(),
                role: "block-control".into(),
                disabled_reason: handle.disabled_reason,
            })
        })
        .collect()
}
/// Apply a handle goal to a source clone; dimensional definitions remain authoritative.
pub fn drag(
    solver: &dyn ConstraintSolver,
    doc: &Document,
    owner: &str,
    sketch: &mut Sketch,
    handle: &SketchHandle,
    position: [f64; 2],
) -> Result<SolveReport> {
    if position.iter().any(|v| !v.is_finite()) {
        return Err(CadError::Invalid("Invalid handle position".into()));
    }
    if let HandleTarget::BlockControl {
        instance: key,
        point,
        output,
    } = &handle.target
    {
        let current = block_controls(doc, owner, sketch, key)?
            .into_iter()
            .find(|current| current.id == handle.id && current.target == handle.target)
            .ok_or_else(|| CadError::Invalid("Block source handle no longer exists".into()))?;
        if let Some(reason) = current.disabled_reason {
            return Err(CadError::Invalid(reason));
        }
        let instance = sketch.block_instances[key].clone();
        if instance.points.get(point) != Some(output) {
            return Err(CadError::Invalid("Block control mapping changed".into()));
        }
        let (_, offset) = crate::sketch_blocks::affine(doc, owner, sketch, &instance)?;
        let angle = expression::value(doc, owner, &instance.angle, Dimension::Angle)?;
        let scale = expression::value(doc, owner, &instance.scale, Dimension::Scalar)?;
        let goal =
            nalgebra::Rotation2::new(-angle) * (nalgebra::Vector2::from(position) - offset) / scale;
        if goal.iter().any(|value| !value.is_finite()) {
            return Err(CadError::Invalid(
                "Block local control coordinate overflow".into(),
            ));
        }
        let mut definition = sketch.blocks[&instance.definition].clone();
        let mut local = crate::sketch_blocks::source(&instance.definition, &definition);
        solver.drag_sketch(
            doc,
            owner,
            &mut local,
            &[SketchDragTarget {
                point: point.clone(),
                position: [goal.x, goal.y],
            }],
        )?;
        definition.points = local.points;
        definition.constraints = local.constraints;
        let mut candidate = sketch.clone();
        crate::sketch_blocks::set_definition(
            doc,
            owner,
            &mut candidate,
            &instance.definition,
            definition,
        )?;
        let report = solver.solve_sketch(doc, owner, &mut candidate)?;
        *sketch = candidate;
        return Ok(report);
    }
    let current = handles(doc, owner, sketch)?
        .into_iter()
        .find(|h| h.id == handle.id)
        .ok_or_else(|| CadError::Invalid("Sketch handle no longer exists".into()))?;
    if current.target != handle.target {
        return Err(CadError::Invalid("Sketch handle target changed".into()));
    }
    if let Some(reason) = current.disabled_reason {
        return Err(CadError::Invalid(reason));
    }
    let mut targets = vec![];
    match &handle.target {
        HandleTarget::Point { point } => targets.push(SketchDragTarget {
            point: point.clone(),
            position,
        }),
        HandleTarget::MoveMember { member } => {
            let delta = [
                position[0] - current.position[0],
                position[1] - current.position[1],
            ];
            let keys: std::collections::BTreeSet<_> = sketch
                .members
                .get(member)
                .ok_or_else(|| CadError::Invalid("Sketch member no longer exists".into()))?
                .curve
                .points()
                .into_iter()
                .map(str::to_owned)
                .collect();
            for key in keys {
                let p = sketch.points[&key];
                targets.push(SketchDragTarget {
                    point: key,
                    position: [p[0] + delta[0], p[1] + delta[1]],
                });
            }
        }
        target => {
            let entity = target.entity();
            let record = sketch
                .members
                .get_mut(entity.key())
                .ok_or_else(|| CadError::Invalid("Sketch member no longer exists".into()))?;
            match (&mut record.curve, target) {
                (SketchCurve::Circle { center, radius }, HandleTarget::CircleRadius { .. }) => {
                    let c = sketch.points[center];
                    let value = (position[0] - c[0]).hypot(position[1] - c[1]);
                    if value <= MIN_HANDLE_SIZE_MM {
                        return Err(CadError::Invalid(
                            "Radius must exceed the handle tolerance (0.0000001 mm)".into(),
                        ));
                    }
                    *radius = Expr(format!("{value} mm"));
                }
                (
                    SketchCurve::Ellipse {
                        center,
                        major,
                        minor,
                        angle,
                    },
                    target,
                ) => {
                    let c = sketch.points[center];
                    let delta = [position[0] - c[0], position[1] - c[1]];
                    let t = expression::value(doc, owner, angle, Dimension::Angle)?;
                    match target {
                        HandleTarget::EllipseRotation { .. } => {
                            *angle = Expr(format!(
                                "{} rad",
                                delta[1].atan2(delta[0]) - std::f64::consts::FRAC_PI_4
                            ))
                        }
                        HandleTarget::EllipseMajor { .. } | HandleTarget::EllipseMinor { .. } => {
                            let value = if matches!(target, HandleTarget::EllipseMajor { .. }) {
                                delta[0] * t.cos() + delta[1] * t.sin()
                            } else {
                                -delta[0] * t.sin() + delta[1] * t.cos()
                            }
                            .abs();
                            if value <= MIN_HANDLE_SIZE_MM {
                                return Err(CadError::Invalid(
                                    "Radius must exceed the handle tolerance (0.0000001 mm)".into(),
                                ));
                            }
                            *if matches!(target, HandleTarget::EllipseMajor { .. }) {
                                major
                            } else {
                                minor
                            } = Expr(format!("{value} mm"));
                        }
                        _ => {
                            return Err(CadError::Invalid(
                                "Handle does not match the curve".into(),
                            ));
                        }
                    }
                }
                _ => return Err(CadError::Invalid("Handle does not match the curve".into())),
            }
        }
    }
    let members = sketch.members.clone();
    let result = if targets.is_empty() {
        solver.solve_sketch(doc, owner, sketch)
    } else {
        solver.drag_sketch(doc, owner, sketch, &targets)
    };
    sketch.members = members;
    result
}
