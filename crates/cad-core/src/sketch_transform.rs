//! Application-owned, associative sketch transformations over exact curve controls.
use crate::*;
use nalgebra::{Matrix2, Rotation2, Vector2};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const CONTRACT_VERSION: u32 = 2;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Axis {
    X,
    Y,
    Member { member: Id },
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Center {
    Origin,
    Point { point: Id },
    Coordinates { x: Expr, y: Expr },
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Transform {
    Mirror {
        axis: Axis,
    },
    Linear {
        count: usize,
        step: [Expr; 2],
    },
    Circular {
        count: usize,
        center: Center,
        angle: Expr,
        equal_spacing: bool,
    },
    Copy {
        center: Center,
        translation: [Expr; 2],
        angle: Expr,
        scale: Expr,
    },
}
impl Transform {
    pub fn expressions(&self) -> Vec<&Expr> {
        let (mut out, center) = match self {
            Self::Mirror { .. } => (vec![], None),
            Self::Linear { step, .. } => (step.iter().collect(), None),
            Self::Circular { center, angle, .. } => (vec![angle], Some(center)),
            Self::Copy {
                center,
                translation,
                angle,
                scale,
            } => (
                vec![&translation[0], &translation[1], angle, scale],
                Some(center),
            ),
        };
        if let Some(Center::Coordinates { x, y }) = center {
            out.extend([x, y]);
        }
        out
    }
    pub fn label(&self) -> &'static str {
        match self {
            Self::Mirror { .. } => "Mirror Entities",
            Self::Linear { .. } => "Linear Sketch Pattern",
            Self::Circular { .. } => "Circular Sketch Pattern",
            Self::Copy { .. } => "Copy Entities",
        }
    }
    pub fn command(&self) -> &'static str {
        match self {
            Self::Mirror { .. } => "sketch.mirror",
            Self::Linear { .. } => "sketch.linear-pattern",
            Self::Circular { .. } => "sketch.circular-pattern",
            Self::Copy { .. } => "sketch.move-copy",
        }
    }
    fn count(&self) -> usize {
        match self {
            Self::Linear { count, .. } | Self::Circular { count, .. } => *count,
            _ => 2,
        }
    }
    pub fn reference_points(&self) -> Vec<Id> {
        match self {
            Self::Circular { center, .. } | Self::Copy { center, .. } => match center {
                Center::Point { point } => vec![point.clone()],
                _ => vec![],
            },
            _ => vec![],
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Instance {
    pub points: BTreeMap<Id, Id>,
    pub members: BTreeMap<Id, Id>,
    #[serde(default, skip_serializing_if = "crate::sketch_actions::is_false")]
    pub suppressed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub placement: Option<Placement>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Placement {
    pub center: Center,
    pub translation: [Expr; 2],
    pub angle: Expr,
    pub scale: Expr,
}
impl Default for Placement {
    fn default() -> Self {
        Self {
            center: Center::Origin,
            translation: ["0 mm".into(), "0 mm".into()],
            angle: "0 deg".into(),
            scale: "1".into(),
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Pattern {
    pub transform: Transform,
    pub sources: Vec<Id>,
    pub instances: Vec<Instance>,
    #[serde(default, skip_serializing_if = "crate::sketch_actions::is_false")]
    pub dynamic: bool,
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub dynamic_excluded: BTreeSet<Id>,
    #[serde(default, skip_serializing_if = "crate::sketch_actions::is_false")]
    pub dynamic_construction: bool,
}
impl Pattern {
    pub fn expressions(&self) -> Vec<&Expr> {
        let mut out = self.transform.expressions();
        for placement in self
            .instances
            .iter()
            .filter_map(|instance| instance.placement.as_ref())
        {
            out.extend([
                &placement.translation[0],
                &placement.translation[1],
                &placement.angle,
                &placement.scale,
            ]);
            if let Center::Coordinates { x, y } = &placement.center {
                out.extend([x, y]);
            }
        }
        out
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
        .ok_or_else(|| invalid("Sketch transformation reference point is missing"))
}
pub fn sources(pattern: &Pattern) -> Vec<Id> {
    let mut sources = pattern.sources.clone();
    if let Transform::Mirror {
        axis: Axis::Member { member },
    } = &pattern.transform
    {
        sources.push(member.clone());
    }
    sources
}
pub fn reference_points(pattern: &Pattern) -> Vec<Id> {
    pattern
        .transform
        .reference_points()
        .into_iter()
        .chain(
            pattern
                .instances
                .iter()
                .filter_map(|instance| match &instance.placement {
                    Some(Placement {
                        center: Center::Point { point },
                        ..
                    }) => Some(point.clone()),
                    _ => None,
                }),
        )
        .chain(
            pattern
                .instances
                .iter()
                .flat_map(|instance| instance.points.keys().cloned()),
        )
        .collect()
}
pub fn outputs(pattern: &Pattern) -> Vec<Id> {
    pattern
        .instances
        .iter()
        .flat_map(|i| i.members.values().cloned())
        .collect()
}
pub fn generated_points(pattern: &Pattern) -> Vec<Id> {
    pattern
        .instances
        .iter()
        .flat_map(|i| i.points.values().cloned())
        .collect()
}
pub fn suppressed_members(sketch: &Sketch) -> BTreeSet<Id> {
    sketch
        .transforms
        .values()
        .flat_map(|pattern| {
            pattern
                .instances
                .iter()
                .filter(|instance| instance.suppressed)
                .flat_map(|instance| instance.members.values().cloned())
        })
        .chain(
            sketch
                .block_instances
                .values()
                .filter(|instance| instance.suppressed)
                .flat_map(crate::sketch_blocks::Instance::outputs),
        )
        .collect()
}
pub fn suppressed_points(sketch: &Sketch) -> BTreeSet<Id> {
    let members = suppressed_members(sketch);
    let driven = crate::sketch_edit::driven_points(sketch);
    let active = sketch
        .members
        .iter()
        .filter(|(key, _)| !members.contains(*key))
        .flat_map(|(_, member)| member.curve.points().into_iter().map(str::to_string))
        .chain(
            crate::sketch_edit::modifier_ids(sketch)
                .into_iter()
                .flat_map(|key| {
                    crate::sketch_edit::active_modifier_reference_points(sketch, &key, &members)
                })
                .filter(|point| !driven.contains(point)),
        )
        .collect::<BTreeSet<_>>();
    sketch
        .transforms
        .values()
        .flat_map(|pattern| {
            pattern
                .instances
                .iter()
                .filter(|instance| instance.suppressed)
                .flat_map(|instance| instance.points.values().cloned())
        })
        .chain(
            sketch
                .block_instances
                .values()
                .filter(|instance| instance.suppressed)
                .flat_map(|instance| {
                    instance
                        .driven_points()
                        .into_iter()
                        .chain(std::iter::once(instance.anchor.clone()))
                }),
        )
        .filter(|point| !active.contains(point))
        .collect()
}
pub fn constraint_active(sketch: &Sketch, constraint: &SketchConstraint) -> bool {
    let inactive = inactive_entities(sketch);
    !constraint.entities.iter().any(|key| inactive.contains(key))
}
pub fn inactive_entities(sketch: &Sketch) -> BTreeSet<Id> {
    suppressed_members(sketch)
        .into_iter()
        .chain(suppressed_points(sketch))
        .collect()
}
pub fn instance_for_member<'a>(sketch: &'a Sketch, member: &str) -> Option<(&'a Id, usize)> {
    sketch.transforms.iter().find_map(|(key, pattern)| {
        pattern
            .instances
            .iter()
            .position(|instance| instance.members.values().any(|target| target == member))
            .map(|index| (key, index))
    })
}
pub fn set_dynamic(sketch: &Sketch, pattern: &mut Pattern, enabled: bool) -> Result<()> {
    if enabled && !matches!(pattern.transform, Transform::Mirror { .. }) {
        return Err(invalid("Dynamic authoring requires a mirror operation"));
    }
    if enabled && !pattern.dynamic {
        pattern.dynamic_excluded = sketch
            .members
            .keys()
            .filter(|key| !pattern.sources.contains(key))
            .cloned()
            .collect();
    }
    if !enabled {
        pattern.dynamic_excluded.clear();
        pattern.dynamic_construction = false;
    }
    pattern.dynamic = enabled;
    Ok(())
}
pub fn add_dynamic(
    sketch: &mut Sketch,
    selected: &[Id],
    axis: Axis,
    construction: bool,
) -> Result<Id> {
    let mut sources = selected
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    if let Axis::Member { member } = &axis {
        sources.retain(|key| key != member);
    }
    if sources.iter().any(|key| !sketch.members.contains_key(key)) {
        return Err(invalid("Dynamic mirror source is missing"));
    }
    let mut pattern = Pattern {
        transform: Transform::Mirror { axis },
        sources,
        instances: vec![],
        dynamic: false,
        dynamic_excluded: BTreeSet::new(),
        dynamic_construction: construction,
    };
    set_dynamic(sketch, &mut pattern, true)?;
    resize(sketch, &mut pattern, construction)?;
    let key = id();
    sketch.transforms.insert(key.clone(), pattern);
    Ok(key)
}
/// Enrol newly authored members once; retained mappings keep existing instance IDs.
pub fn synchronize_dynamic(sketch: &mut Sketch) -> Result<bool> {
    let mut candidate = sketch.clone();
    let mut generated = candidate
        .transforms
        .values()
        .flat_map(outputs)
        .collect::<BTreeSet<_>>();
    let keys = candidate.transforms.keys().cloned().collect::<Vec<_>>();
    let mut changed = false;
    for key in keys {
        let mut pattern = candidate
            .transforms
            .remove(&key)
            .ok_or_else(|| invalid("Dynamic mirror disappeared"))?;
        let axis = if let Transform::Mirror {
            axis: Axis::Member { member },
        } = &pattern.transform
        {
            Some(member.as_str())
        } else {
            None
        };
        let additions = if pattern.dynamic {
            candidate
                .members
                .keys()
                .filter(|member| {
                    !generated.contains(*member)
                        && !pattern.dynamic_excluded.contains(*member)
                        && !pattern.sources.contains(*member)
                        && axis != Some(member.as_str())
                })
                .cloned()
                .collect::<Vec<_>>()
        } else {
            vec![]
        };
        let topology_changed = pattern.instances.iter().any(|instance| {
            instance
                .points
                .keys()
                .any(|point| !candidate.points.contains_key(point))
                || pattern.sources.iter().any(|source| {
                    crate::sketch_edit::effective_curve(&candidate, source).is_ok_and(|curve| {
                        match remap(&curve, &instance.points) {
                            Err(_) => true,
                            Ok(mapped) => instance
                                .members
                                .get(source)
                                .and_then(|target| candidate.members.get(target))
                                .is_none_or(|member| {
                                    std::mem::discriminant(&member.curve)
                                        != std::mem::discriminant(&mapped)
                                        || member.curve.points() != mapped.points()
                                }),
                        }
                    })
                })
        });
        if !additions.is_empty() || topology_changed {
            let construction = if pattern.dynamic {
                pattern.dynamic_construction
            } else {
                outputs(&pattern)
                    .first()
                    .and_then(|member| candidate.members.get(member))
                    .is_some_and(|member| member.construction)
            };
            pattern.sources.extend(additions);
            resize(&mut candidate, &mut pattern, construction)?;
            changed = true;
        }
        generated.extend(outputs(&pattern));
        candidate.transforms.insert(key, pattern);
    }
    if changed {
        *sketch = candidate;
    }
    Ok(changed)
}
pub fn stop_dynamic(sketch: &mut Sketch) -> bool {
    let mut changed = false;
    sketch.transforms.retain(|_, pattern| {
        if !pattern.dynamic {
            return true;
        }
        changed = true;
        pattern.dynamic = false;
        pattern.dynamic_excluded.clear();
        pattern.dynamic_construction = false;
        !pattern.sources.is_empty()
    });
    changed
}
pub fn dynamic_preview(
    doc: &Document,
    owner: &str,
    sketch: &Sketch,
    points: &[[f64; 2]],
) -> Result<Vec<Vec<[f64; 2]>>> {
    sketch
        .transforms
        .values()
        .filter(|pattern| pattern.dynamic)
        .filter(|pattern| {
            pattern
                .instances
                .first()
                .is_some_and(|instance| !instance.suppressed)
        })
        .map(|pattern| {
            let (mut matrix, mut offset) = affine(doc, owner, sketch, &pattern.transform, 1)?;
            if let Some(placement) = &pattern.instances[0].placement {
                let (extra, translation) = affine(
                    doc,
                    owner,
                    sketch,
                    &Transform::Copy {
                        center: placement.center.clone(),
                        translation: placement.translation.clone(),
                        angle: placement.angle.clone(),
                        scale: placement.scale.clone(),
                    },
                    1,
                )?;
                matrix = extra * matrix;
                offset = extra * offset + translation;
            }
            points
                .iter()
                .map(|point| {
                    let p = matrix * Vector2::from(*point) + offset;
                    if p.iter().any(|v| !v.is_finite()) {
                        Err(invalid("Dynamic mirror preview coordinate overflow"))
                    } else {
                        Ok([p.x, p.y])
                    }
                })
                .collect()
        })
        .collect()
}
pub fn add(
    sketch: &mut Sketch,
    selected: &[Id],
    transform: Transform,
    construction: bool,
) -> Result<Id> {
    let sources = selected
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    if sources.is_empty() || sources.iter().any(|k| !sketch.members.contains_key(k)) {
        return Err(invalid("Select sketch members to transform"));
    }
    let count = transform.count();
    if !(2..=256).contains(&count) || sources.len().saturating_mul(count) > 4096 {
        return Err(invalid(
            "Sketch pattern requires 2–256 instances and at most 4096 generated members",
        ));
    }
    let mut pattern = Pattern {
        transform,
        sources,
        instances: vec![],
        dynamic: false,
        dynamic_excluded: BTreeSet::new(),
        dynamic_construction: false,
    };
    resize(sketch, &mut pattern, construction)?;
    let key = id();
    sketch.transforms.insert(key.clone(), pattern);
    Ok(key)
}
pub fn resize(sketch: &mut Sketch, pattern: &mut Pattern, construction: bool) -> Result<()> {
    let count = pattern.transform.count();
    if !(2..=256).contains(&count) || pattern.sources.len().saturating_mul(count) > 4096 {
        return Err(invalid(
            "Sketch pattern requires 2–256 instances and at most 4096 generated members",
        ));
    }
    if pattern.instances.len() >= count {
        let removed = pattern.instances.split_off(count - 1);
        let deleted = removed
            .iter()
            .flat_map(|i| i.members.values().cloned())
            .collect();
        crate::sketch_edit::delete_members(sketch, &deleted);
        for key in removed.iter().flat_map(|i| i.points.values()) {
            if !sketch
                .members
                .values()
                .any(|m| m.curve.points().contains(&key.as_str()))
            {
                sketch.points.remove(key);
            }
        }
    }
    let keys = pattern
        .sources
        .iter()
        .map(|key| crate::sketch_edit::effective_curve(sketch, key))
        .collect::<Result<Vec<_>>>()?
        .iter()
        .flat_map(|m| m.points().into_iter().map(String::from))
        .collect::<BTreeSet<_>>();
    while pattern.instances.len() < count - 1 {
        let mut instance = Instance {
            points: BTreeMap::new(),
            members: BTreeMap::new(),
            suppressed: false,
            placement: None,
        };
        for source in &keys {
            let output = id();
            sketch.points.insert(output.clone(), sketch.points[source]);
            instance.points.insert(source.clone(), output);
        }
        for source in &pattern.sources {
            let output = id();
            let mut member = sketch.members[source].clone();
            member.curve = remap(
                &crate::sketch_edit::effective_curve(sketch, source)?,
                &instance.points,
            )?;
            member.construction = construction;
            sketch.members.insert(output.clone(), member);
            instance.members.insert(source.clone(), output);
        }
        pattern.instances.push(instance);
    }
    // Dynamic authoring adds controls and members without replacing prior instance IDs.
    let mut retired = BTreeSet::new();
    for instance in &mut pattern.instances {
        instance.points.retain(|source, target| {
            if sketch.points.contains_key(source) {
                true
            } else {
                retired.insert(target.clone());
                false
            }
        });
        for source in &keys {
            if !instance.points.contains_key(source) {
                let target = id();
                sketch.points.insert(target.clone(), sketch.points[source]);
                instance.points.insert(source.clone(), target);
            }
        }
        for source in &pattern.sources {
            if !instance.members.contains_key(source) {
                let target = id();
                let mut member = sketch.members[source].clone();
                member.curve = remap(
                    &crate::sketch_edit::effective_curve(sketch, source)?,
                    &instance.points,
                )?;
                member.construction = construction;
                sketch.members.insert(target.clone(), member);
                instance.members.insert(source.clone(), target);
            }
            let target = instance.members[source].clone();
            let curve = remap(
                &crate::sketch_edit::effective_curve(sketch, source)?,
                &instance.points,
            )?;
            sketch
                .members
                .get_mut(&target)
                .ok_or_else(|| invalid("Sketch pattern output disappeared"))?
                .curve = curve;
        }
    }
    for key in outputs(pattern) {
        if let Some(member) = sketch.members.get_mut(&key) {
            member.construction = construction;
        }
    }
    for point in retired {
        if sketch
            .members
            .values()
            .any(|member| member.curve.points().contains(&point.as_str()))
            || sketch
                .constraints
                .iter()
                .any(|constraint| constraint.entities.contains(&point))
            || crate::sketch_edit::modifier_ids(sketch).iter().any(|key| {
                crate::sketch_edit::modifier_reference_points(sketch, key).contains(&point)
            })
        {
            return Err(invalid(
                "Source topology removed a referenced instance control; repair or detach that reference first",
            ));
        }
        sketch.points.remove(&point);
    }
    Ok(())
}
pub(crate) fn remap(curve: &SketchCurve, points: &BTreeMap<Id, Id>) -> Result<SketchCurve> {
    let map = |key: &Id| {
        points
            .get(key)
            .cloned()
            .ok_or_else(|| invalid("Sketch pattern control topology changed; reselect its sources"))
    };
    Ok(match curve {
        SketchCurve::Line { start, end } => SketchCurve::Line {
            start: map(start)?,
            end: map(end)?,
        },
        SketchCurve::Arc {
            start,
            through,
            end,
        } => SketchCurve::Arc {
            start: map(start)?,
            through: map(through)?,
            end: map(end)?,
        },
        SketchCurve::Bezier { points: controls } => SketchCurve::Bezier {
            points: controls.iter().map(map).collect::<Result<_>>()?,
        },
        SketchCurve::Nurbs {
            points: controls,
            weights,
            knots,
            degree,
        } => SketchCurve::Nurbs {
            points: controls.iter().map(map).collect::<Result<_>>()?,
            weights: weights.clone(),
            knots: knots.clone(),
            degree: *degree,
        },
        SketchCurve::Circle { center, radius } => SketchCurve::Circle {
            center: map(center)?,
            radius: radius.clone(),
        },
        SketchCurve::Ellipse {
            center,
            major,
            minor,
            angle,
        } => SketchCurve::Ellipse {
            center: map(center)?,
            major: major.clone(),
            minor: minor.clone(),
            angle: angle.clone(),
        },
    })
}
fn center(doc: &Document, owner: &str, sketch: &Sketch, value: &Center) -> Result<Vector2<f64>> {
    Ok(match value {
        Center::Origin => Vector2::zeros(),
        Center::Point { point: key } => point(sketch, key)?,
        Center::Coordinates { x, y } => Vector2::new(
            expression::value(doc, owner, x, Dimension::Length)?,
            expression::value(doc, owner, y, Dimension::Length)?,
        ),
    })
}
fn affine(
    doc: &Document,
    owner: &str,
    sketch: &Sketch,
    transform: &Transform,
    index: usize,
) -> Result<(Matrix2<f64>, Vector2<f64>)> {
    let length = |e| expression::value(doc, owner, e, Dimension::Length);
    let angle = |e| expression::value(doc, owner, e, Dimension::Angle);
    let result = match transform {
        Transform::Mirror { axis } => {
            let (origin, direction) = match axis {
                Axis::X => (Vector2::zeros(), Vector2::x()),
                Axis::Y => (Vector2::zeros(), Vector2::y()),
                Axis::Member { member } => {
                    let Some(SketchMember {
                        curve: SketchCurve::Line { start, end },
                        ..
                    }) = sketch.members.get(member)
                    else {
                        return Err(invalid("Mirror axis must be a straight sketch member"));
                    };
                    let a = point(sketch, start)?;
                    let delta = point(sketch, end)? - a;
                    if !delta.norm().is_finite() {
                        return Err(invalid(
                            "Mirror axis length exceeds the finite coordinate range",
                        ));
                    }
                    (
                        a,
                        delta
                            .try_normalize(1e-9)
                            .ok_or_else(|| invalid("Mirror axis collapsed"))?,
                    )
                }
            };
            let matrix = direction * direction.transpose() * 2. - Matrix2::identity();
            (matrix, origin - matrix * origin)
        }
        Transform::Linear { step, .. } => {
            let offset = Vector2::new(length(&step[0])?, length(&step[1])?);
            if offset.norm() <= 1e-9 {
                return Err(invalid("Pattern spacing must be nonzero"));
            }
            (Matrix2::identity(), offset * index as f64)
        }
        Transform::Circular {
            count,
            center: c,
            angle: value,
            equal_spacing,
        } => {
            let total = angle(value)?;
            if total.abs() <= 1e-10 || total.abs() > std::f64::consts::TAU + 1e-8 {
                return Err(invalid(
                    "Circular pattern angle must be nonzero and at most 360 degrees",
                ));
            }
            let full = (total.abs() - std::f64::consts::TAU).abs() <= 1e-8;
            let step = if *equal_spacing {
                total / if full { *count } else { count - 1 } as f64
            } else {
                total
            };
            if !equal_spacing && step.abs() * (*count - 1) as f64 >= std::f64::consts::TAU - 1e-8 {
                return Err(invalid(
                    "Circular pattern instances overlap after a complete turn; reduce count or angle",
                ));
            }
            let matrix = Rotation2::new(step * index as f64).into_inner();
            let origin = center(doc, owner, sketch, c)?;
            (matrix, origin - matrix * origin)
        }
        Transform::Copy {
            center: c,
            translation,
            angle: value,
            scale,
        } => {
            let scale = expression::value(doc, owner, scale, Dimension::Scalar)?;
            if scale <= 1e-9 {
                return Err(invalid("Sketch scale must be positive"));
            }
            let matrix = Rotation2::new(angle(value)?).into_inner() * scale;
            let origin = center(doc, owner, sketch, c)?;
            (
                matrix,
                origin - matrix * origin
                    + Vector2::new(length(&translation[0])?, length(&translation[1])?),
            )
        }
    };
    if result
        .0
        .iter()
        .chain(result.1.iter())
        .any(|v| !v.is_finite())
    {
        return Err(invalid(
            "Sketch transformation produced non-finite coordinates",
        ));
    }
    Ok(result)
}
pub(crate) fn curve_geometry(
    doc: &Document,
    owner: &str,
    curve: SketchCurve,
    matrix: &Matrix2<f64>,
) -> Result<SketchCurve> {
    let scale = matrix.column(0).norm();
    let size = |e: &Expr| {
        expression::value(doc, owner, e, Dimension::Length)
            .map(|v| Expr(format!("{} mm", v * scale)))
    };
    Ok(match curve {
        SketchCurve::Circle { center, radius } => SketchCurve::Circle {
            center,
            radius: size(&radius)?,
        },
        SketchCurve::Ellipse {
            center,
            major,
            minor,
            angle,
        } => {
            let a = expression::value(doc, owner, &angle, Dimension::Angle)?;
            let u = matrix * Vector2::new(a.cos(), a.sin());
            SketchCurve::Ellipse {
                center,
                major: size(&major)?,
                minor: size(&minor)?,
                angle: Expr(format!("{} deg", u.y.atan2(u.x).to_degrees())),
            }
        }
        other => other,
    })
}
pub fn derive(doc: &Document, owner: &str, sketch: &mut Sketch, pattern: &Pattern) -> Result<()> {
    for (index, instance) in pattern.instances.iter().enumerate() {
        let (mut matrix, mut offset) = affine(doc, owner, sketch, &pattern.transform, index + 1)?;
        if let Some(placement) = &instance.placement {
            let (extra, translation) = affine(
                doc,
                owner,
                sketch,
                &Transform::Copy {
                    center: placement.center.clone(),
                    translation: placement.translation.clone(),
                    angle: placement.angle.clone(),
                    scale: placement.scale.clone(),
                },
                1,
            )?;
            offset = extra * offset + translation;
            matrix = extra * matrix;
        }
        for (source, target) in &instance.points {
            let p = matrix * point(sketch, source)? + offset;
            if p.iter().any(|v| !v.is_finite()) {
                return Err(invalid("Sketch pattern coordinate overflow"));
            }
            sketch.points.insert(target.clone(), [p.x, p.y]);
        }
        for (source, target) in &instance.members {
            let source = sketch
                .members
                .get(source)
                .ok_or_else(|| invalid("Sketch pattern source is missing"))?
                .curve
                .clone();
            let curve = curve_geometry(doc, owner, remap(&source, &instance.points)?, &matrix)?;
            sketch
                .members
                .get_mut(target)
                .ok_or_else(|| invalid("Sketch pattern output is missing"))?
                .curve = curve;
        }
    }
    Ok(())
}
/// Transform existing free members as a pending source edit, preserving constraints.
pub fn move_members(
    doc: &Document,
    owner: &str,
    sketch: &mut Sketch,
    selected: &[Id],
    transform: &Transform,
) -> Result<()> {
    if selected.is_empty() {
        return Err(invalid("Select sketch members to transform"));
    }
    let driven = crate::sketch_edit::driven_points(sketch);
    let mut controls = BTreeSet::new();
    for key in selected {
        let member = sketch
            .members
            .get(key)
            .ok_or_else(|| invalid("Sketch member is missing"))?;
        if crate::sketch_edit::driven_member(sketch, key)
            || member.curve.points().iter().any(|k| driven.contains(*k))
        {
            return Err(invalid(
                "Driven geometry cannot be moved directly; edit or detach its operation",
            ));
        }
        controls.extend(member.curve.points().into_iter().map(String::from));
    }
    let (matrix, offset) = affine(doc, owner, sketch, transform, 1)?;
    if matrix.determinant() < 0. {
        let moved_lines = sketch
            .members
            .iter()
            .filter_map(|(key, member)| {
                if let SketchCurve::Line { start, end } = &member.curve {
                    (controls.contains(start) && controls.contains(end)).then_some(key.clone())
                } else {
                    None
                }
            })
            .collect::<BTreeSet<_>>();
        let moved_centers = sketch
            .members
            .iter()
            .filter_map(|(key, member)| match &member.curve {
                SketchCurve::Circle { center, .. } | SketchCurve::Ellipse { center, .. }
                    if controls.contains(center) =>
                {
                    Some(key.clone())
                }
                _ => None,
            })
            .collect::<BTreeSet<_>>();
        for constraint in &mut sketch.constraints {
            if constraint.kind == "point_line_distance"
                && constraint.entities.len() == 2
                && (controls.contains(&constraint.entities[0])
                    || moved_centers.contains(&constraint.entities[0]))
                && moved_lines.contains(&constraint.entities[1])
            {
                constraint.distance_side = constraint.distance_side.map(|side| side.reversed());
            }
        }
    }
    for key in &controls {
        let p = matrix * point(sketch, key)? + offset;
        if p.iter().any(|v| !v.is_finite()) {
            return Err(invalid("Sketch transformation coordinate overflow"));
        }
        sketch.points.insert(key.clone(), [p.x, p.y]);
    }
    for key in selected {
        let curve = curve_geometry(doc, owner, sketch.members[key].curve.clone(), &matrix)?;
        sketch.members.get_mut(key).unwrap().curve = curve;
    }
    Ok(())
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StretchRequest {
    pub points: Vec<Id>,
    pub translation: [Expr; 2],
}
/// Stretch an explicit set of source controls; unselected controls and all
/// authored relations are retained for the shared constraint solve.
pub fn stretch(
    doc: &Document,
    owner: &str,
    sketch: &mut Sketch,
    request: &StretchRequest,
) -> Result<()> {
    let controls = request.points.iter().cloned().collect::<BTreeSet<_>>();
    if controls.is_empty() {
        return Err(invalid(
            "Select the source endpoints, centers or controls to stretch",
        ));
    }
    let driven = crate::sketch_edit::driven_points(sketch);
    if controls
        .iter()
        .any(|key| !sketch.points.contains_key(key) || driven.contains(key))
    {
        return Err(invalid(
            "Stretch needs independent source controls; edit or detach generated geometry first",
        ));
    }
    let offset = Vector2::new(
        expression::value(doc, owner, &request.translation[0], Dimension::Length)?,
        expression::value(doc, owner, &request.translation[1], Dimension::Length)?,
    );
    let mut candidate = sketch.clone();
    for key in controls {
        let p = point(&candidate, &key)? + offset;
        if p.iter().any(|v| !v.is_finite()) {
            return Err(invalid("Stretch coordinate overflow"));
        }
        candidate.points.insert(key, [p.x, p.y]);
    }
    *sketch = candidate;
    Ok(())
}
pub fn validate(sketch: &Sketch, pattern: &Pattern) -> Result<()> {
    if !(2..=256).contains(&pattern.transform.count())
        || pattern.instances.len() + 1 != pattern.transform.count()
        || (pattern.sources.is_empty() && !pattern.dynamic)
        || (pattern.dynamic && !matches!(pattern.transform, Transform::Mirror { .. }))
        || pattern
            .sources
            .len()
            .saturating_mul(pattern.transform.count())
            > 4096
        || pattern.sources.iter().collect::<BTreeSet<_>>().len() != pattern.sources.len()
    {
        return Err(invalid(
            "Invalid sketch pattern instance count or source selection",
        ));
    }
    for source in sources(pattern) {
        if !sketch.members.contains_key(&source) {
            return Err(invalid("Sketch pattern source or axis is missing"));
        }
    }
    for p in reference_points(pattern) {
        if !sketch.points.contains_key(&p) {
            return Err(invalid("Sketch pattern center is missing"));
        }
    }
    let curves = pattern
        .sources
        .iter()
        .map(|k| crate::sketch_edit::effective_curve(sketch, k))
        .collect::<Result<Vec<_>>>()?;
    let keys = curves
        .iter()
        .flat_map(|c| c.points().into_iter().map(String::from))
        .collect::<BTreeSet<_>>();
    let mut seen = BTreeSet::new();
    let mut points_seen = BTreeSet::new();
    for instance in &pattern.instances {
        if let Some(Placement {
            center: Center::Point { point },
            ..
        }) = &instance.placement
            && (!sketch.points.contains_key(point) || generated_points(pattern).contains(point))
        {
            return Err(invalid(
                "Instance placement center must be an independent source point",
            ));
        }
        if !keys.is_subset(&instance.points.keys().cloned().collect::<BTreeSet<_>>())
            || instance.points.keys().any(|point| {
                !sketch.points.contains_key(point) || generated_points(pattern).contains(point)
            })
            || instance.members.keys().cloned().collect::<BTreeSet<_>>()
                != pattern.sources.iter().cloned().collect()
            || instance.points.values().any(|p| {
                !sketch.points.contains_key(p) || keys.contains(p) || !points_seen.insert(p)
            })
            || instance.members.values().any(|m| {
                !sketch.members.contains_key(m) || pattern.sources.contains(m) || !seen.insert(m)
            })
        {
            return Err(invalid(
                "Sketch pattern has missing, cyclic or multiply driven output topology",
            ));
        }
        for (source, target) in &instance.members {
            let curve = remap(
                &crate::sketch_edit::effective_curve(sketch, source)?,
                &instance.points,
            )?;
            if std::mem::discriminant(&curve)
                != std::mem::discriminant(&sketch.members[target].curve)
                || curve.points() != sketch.members[target].curve.points()
            {
                return Err(invalid(
                    "Sketch pattern output topology does not match its source",
                ));
            }
        }
    }
    Ok(())
}
