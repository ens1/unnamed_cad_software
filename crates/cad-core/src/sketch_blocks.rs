//! Local associative sketch blocks, rigid instances and exact source controls.
use crate::solver::ConstraintSolver;
use crate::*;
use nalgebra::{Matrix2, Rotation2, Vector2};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
pub const CONTRACT_VERSION: u32 = 1;
pub const MAX_MEMBERS: usize = 4096;
pub const MAX_INSTANCES: usize = 256;
pub type Prepared = BTreeMap<Id, Sketch>;
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Definition {
    pub name: String,
    pub origin: [f64; 2],
    pub points: BTreeMap<Id, [f64; 2]>,
    pub members: BTreeMap<Id, SketchMember>,
    pub constraints: Vec<SketchConstraint>,
}
impl Definition {
    pub fn expressions(&self) -> Vec<&Expr> {
        self.members
            .values()
            .flat_map(|member| match &member.curve {
                SketchCurve::Circle { radius, .. } => vec![radius],
                SketchCurve::Ellipse {
                    major,
                    minor,
                    angle,
                    ..
                } => vec![major, minor, angle],
                _ => vec![],
            })
            .chain(self.constraints.iter().map(|constraint| &constraint.value))
            .collect()
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Instance {
    pub definition: Id,
    pub anchor: Id,
    pub translation: [Expr; 2],
    pub angle: Expr,
    pub scale: Expr,
    #[serde(default, skip_serializing_if = "crate::sketch_actions::is_false")]
    pub free_rotation: bool,
    #[serde(default, skip_serializing_if = "crate::sketch_actions::is_false")]
    pub suppressed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub construction: Option<bool>,
    pub points: BTreeMap<Id, Id>,
    pub members: BTreeMap<Id, Id>,
}
impl Instance {
    pub fn outputs(&self) -> Vec<Id> {
        self.members.values().cloned().collect()
    }
    pub fn driven_points(&self) -> Vec<Id> {
        self.points.values().cloned().collect()
    }
    pub fn expressions(&self) -> Vec<&Expr> {
        vec![
            &self.translation[0],
            &self.translation[1],
            &self.angle,
            &self.scale,
        ]
    }
}
fn invalid(message: &str) -> CadError {
    CadError::Invalid(message.into())
}
pub fn source(key: &str, definition: &Definition) -> Sketch {
    Sketch {
        id: key.into(),
        name: definition.name.clone(),
        points: definition.points.clone(),
        members: definition.members.clone(),
        constraints: definition.constraints.clone(),
        segments: vec![],
        profile_order: vec![],
        circles: BTreeMap::new(),
        conversions: BTreeMap::new(),
        offsets: BTreeMap::new(),
        fillets: BTreeMap::new(),
        chamfers: BTreeMap::new(),
        transforms: BTreeMap::new(),
        splines: BTreeMap::new(),
        conics: BTreeMap::new(),
        texts: BTreeMap::new(),
        equations: BTreeMap::new(),
        blocks: BTreeMap::new(),
        block_instances: BTreeMap::new(),
        dimension_groups: BTreeMap::new(),
    }
}
pub fn validate_definition(key: &str, definition: &Definition) -> Result<()> {
    if definition.name.trim().is_empty()
        || definition.members.is_empty()
        || definition.members.len() > MAX_MEMBERS
        || definition.points.len() > MAX_MEMBERS * 16
        || definition
            .origin
            .iter()
            .chain(definition.points.values().flatten())
            .any(|value| !value.is_finite())
    {
        return Err(invalid(
            "Block needs a name, finite source controls and 1–4096 exact native members",
        ));
    }
    let local = source(key, definition);
    crate::sketch_edit::validate(&local)?;
    if definition.constraints.iter().any(|constraint| {
        constraint.entities.iter().any(|entity| {
            !definition.points.contains_key(entity)
                && !definition.members.contains_key(entity)
                && !matches!(
                    entity.as_str(),
                    crate::sketch_actions::ORIGIN
                        | crate::sketch_actions::X_AXIS
                        | crate::sketch_actions::Y_AXIS
                )
        })
    }) {
        return Err(invalid(
            "Block internal relations cannot refer to entities outside their local definition",
        ));
    }
    if definition.points.keys().any(|point| {
        !definition
            .members
            .values()
            .any(|member| member.curve.points().contains(&point.as_str()))
            && !definition
                .constraints
                .iter()
                .any(|constraint| constraint.entities.contains(point))
    }) {
        return Err(invalid(
            "Block contains an unreferenced hidden source control",
        ));
    }
    Ok(())
}
pub fn rotation_literal(expr: &Expr) -> bool {
    let value = expr.0.trim();
    let number = value
        .strip_suffix("deg")
        .or_else(|| value.strip_suffix("rad"))
        .unwrap_or(value)
        .trim();
    number.parse::<f64>().is_ok_and(f64::is_finite)
}
pub fn validate(sketch: &Sketch) -> Result<()> {
    if sketch.block_instances.len() > MAX_INSTANCES {
        return Err(invalid("A sketch supports at most 256 block instances"));
    }
    for (key, definition) in &sketch.blocks {
        validate_definition(key, definition)?;
    }
    let mut members = BTreeSet::new();
    let mut points = BTreeSet::new();
    for instance in sketch.block_instances.values() {
        let definition = sketch
            .blocks
            .get(&instance.definition)
            .ok_or_else(|| invalid("Block instance has no local source definition"))?;
        if !sketch.points.contains_key(&instance.anchor)
            || instance
                .points
                .values()
                .any(|point| point == &instance.anchor)
            || (instance.free_rotation && !rotation_literal(&instance.angle))
            || instance.points.keys().ne(definition.points.keys())
            || instance.members.keys().ne(definition.members.keys())
        {
            return Err(invalid(
                "Block instance has an invalid anchor, literal free rotation, or source mapping",
            ));
        }
        for (source, target) in &instance.points {
            if !sketch.points.contains_key(target)
                || !points.insert(target)
                || !definition.points.contains_key(source)
            {
                return Err(invalid(
                    "Block output points have missing or multiple source drivers",
                ));
            }
        }
        for (source, target) in &instance.members {
            let curve = crate::sketch_transform::remap(
                &definition.members[source].curve,
                &instance.points,
            )?;
            if !members.insert(target)
                || sketch.members.get(target).is_none_or(|member| {
                    member.curve.points() != curve.points()
                        || std::mem::discriminant(&member.curve) != std::mem::discriminant(&curve)
                })
            {
                return Err(invalid(
                    "Block output member has inconsistent or multiply driven topology",
                ));
            }
        }
    }
    Ok(())
}
pub fn instance_for_member<'a>(sketch: &'a Sketch, member: &str) -> Option<(&'a Id, &'a Instance)> {
    sketch
        .block_instances
        .iter()
        .find(|(_, instance)| instance.members.values().any(|output| output == member))
}
pub fn affine(
    doc: &Document,
    owner: &str,
    sketch: &Sketch,
    instance: &Instance,
) -> Result<(Matrix2<f64>, Vector2<f64>)> {
    let definition = sketch
        .blocks
        .get(&instance.definition)
        .ok_or_else(|| invalid("Block definition is missing"))?;
    let angle = expression::value(doc, owner, &instance.angle, Dimension::Angle)?;
    let scale = expression::value(doc, owner, &instance.scale, Dimension::Scalar)?;
    if scale <= 0. {
        return Err(invalid("Block uniform scale must be positive"));
    }
    let matrix = Rotation2::new(angle).matrix() * scale;
    let anchor = sketch
        .points
        .get(&instance.anchor)
        .ok_or_else(|| invalid("Block insertion anchor is missing"))?;
    let position = Vector2::from(*anchor)
        + Vector2::new(
            expression::value(doc, owner, &instance.translation[0], Dimension::Length)?,
            expression::value(doc, owner, &instance.translation[1], Dimension::Length)?,
        );
    let offset = position - matrix * Vector2::from(definition.origin);
    if matrix
        .iter()
        .chain(offset.iter())
        .any(|value| !value.is_finite())
        || scale.recip().is_infinite()
    {
        return Err(invalid(
            "Block affine placement exceeds the finite coordinate range",
        ));
    }
    Ok((matrix, offset))
}
pub fn prepare(doc: &Document, owner: &str, sketch: &Sketch) -> Result<Prepared> {
    let mut prepared = BTreeMap::new();
    for key in sketch
        .block_instances
        .values()
        .map(|instance| instance.definition.clone())
        .collect::<BTreeSet<_>>()
    {
        let definition = sketch
            .blocks
            .get(&key)
            .ok_or_else(|| invalid("Block source definition is missing"))?;
        let mut local = source(&key, definition);
        crate::solver::NativeSolver
            .solve_sketch(doc, owner, &mut local)
            .map_err(|error| CadError::Invalid(format!("Block {}: {error}", definition.name)))?;
        prepared.insert(key, local);
    }
    Ok(prepared)
}
pub fn derive(doc: &Document, owner: &str, sketch: &mut Sketch, key: &str) -> Result<()> {
    let prepared = prepare(doc, owner, sketch)?;
    derive_prepared(doc, owner, sketch, key, &prepared)
}
pub fn derive_prepared(
    doc: &Document,
    owner: &str,
    sketch: &mut Sketch,
    key: &str,
    prepared: &Prepared,
) -> Result<()> {
    let instance = sketch
        .block_instances
        .get(key)
        .cloned()
        .ok_or_else(|| invalid("Block instance is missing"))?;
    let local = prepared
        .get(&instance.definition)
        .ok_or_else(|| invalid("Block has no current prepared local source geometry"))?;
    let (matrix, offset) = affine(doc, owner, sketch, &instance)?;
    for (source, target) in &instance.points {
        let point = matrix * Vector2::from(local.points[source]) + offset;
        if point.iter().any(|value| !value.is_finite()) {
            return Err(invalid("Block output coordinate overflow"));
        }
        sketch.points.insert(target.clone(), [point.x, point.y]);
    }
    for (source, target) in &instance.members {
        let original = &local.members[source];
        let curve = crate::sketch_transform::remap(&original.curve, &instance.points)?;
        let curve = crate::sketch_transform::curve_geometry(doc, owner, curve, &matrix)?;
        let construction = instance.construction.unwrap_or(original.construction);
        sketch.members.insert(
            target.clone(),
            SketchMember {
                construction,
                curve,
            },
        );
    }
    Ok(())
}
fn output_referenced(
    sketch: &Sketch,
    members: &BTreeSet<Id>,
    points: &BTreeSet<Id>,
    own: &str,
) -> bool {
    sketch.constraints.iter().any(|constraint| {
        constraint
            .entities
            .iter()
            .any(|entity| members.contains(entity) || points.contains(entity))
    }) || sketch.members.iter().any(|(key, member)| {
        !members.contains(key)
            && member
                .curve
                .points()
                .iter()
                .any(|point| points.contains(*point))
    }) || crate::sketch_edit::modifier_ids(sketch)
        .iter()
        .filter(|key| key.as_str() != own)
        .any(|key| {
            crate::sketch_edit::modifier_sources(sketch, key)
                .iter()
                .any(|member| members.contains(member))
                || crate::sketch_edit::modifier_reference_points(sketch, key)
                    .iter()
                    .any(|point| points.contains(point))
        })
}
pub fn set_definition(
    doc: &Document,
    owner: &str,
    sketch: &mut Sketch,
    key: &str,
    definition: Definition,
) -> Result<()> {
    validate_definition(key, &definition)?;
    let mut solved = source(key, &definition);
    crate::solver::NativeSolver.solve_sketch(doc, owner, &mut solved)?;
    let mut candidate = sketch.clone();
    if !candidate.blocks.contains_key(key) {
        return Err(invalid("Block source definition is missing"));
    }
    candidate.blocks.insert(key.into(), definition.clone());
    let instances = candidate
        .block_instances
        .iter()
        .filter(|(_, instance)| instance.definition == key)
        .map(|(id, _)| id.clone())
        .collect::<Vec<_>>();
    for id in &instances {
        let mut instance = candidate.block_instances.remove(id).unwrap();
        let retired_members = instance
            .members
            .iter()
            .filter(|(source, _)| !definition.members.contains_key(*source))
            .map(|(_, target)| target.clone())
            .collect::<BTreeSet<_>>();
        let retired_points = instance
            .points
            .iter()
            .filter(|(source, _)| !definition.points.contains_key(*source))
            .map(|(_, target)| target.clone())
            .collect::<BTreeSet<_>>();
        if output_referenced(&candidate, &retired_members, &retired_points, id) {
            return Err(invalid(
                "Block source edit retires referenced output members/controls; repair or explode those references first",
            ));
        }
        for member in &retired_members {
            candidate.members.remove(member);
        }
        for point in &retired_points {
            candidate.points.remove(point);
        }
        instance
            .members
            .retain(|source, _| definition.members.contains_key(source));
        instance
            .points
            .retain(|source, _| definition.points.contains_key(source));
        for source in definition.points.keys() {
            let target = instance
                .points
                .entry(source.clone())
                .or_insert_with(crate::id);
            candidate.points.entry(target.clone()).or_insert([0.; 2]);
        }
        for (source, member) in &definition.members {
            let target = instance
                .members
                .entry(source.clone())
                .or_insert_with(crate::id);
            candidate.members.insert(
                target.clone(),
                SketchMember {
                    construction: instance.construction.unwrap_or(member.construction),
                    curve: crate::sketch_transform::remap(&member.curve, &instance.points)?,
                },
            );
        }
        candidate.block_instances.insert(id.clone(), instance);
    }
    let mut remainder = candidate.clone();
    remainder
        .block_instances
        .retain(|_, instance| instance.definition != key);
    let mut prepared = prepare(doc, owner, &remainder)?;
    prepared.insert(key.into(), solved);
    for id in instances {
        derive_prepared(doc, owner, &mut candidate, &id, &prepared)?;
    }
    crate::sketch_edit::validate(&candidate)?;
    *sketch = candidate;
    Ok(())
}
pub fn create(
    doc: &Document,
    owner: &str,
    sketch: &mut Sketch,
    members: &[Id],
    name: &str,
    origin: [f64; 2],
) -> Result<(Id, Id)> {
    let selected = members.iter().cloned().collect::<BTreeSet<_>>();
    if selected.is_empty()
        || selected.iter().any(|member| {
            !sketch.members.contains_key(member)
                || crate::sketch_edit::driven_member(sketch, member)
        })
    {
        return Err(invalid(
            "Create Block requires independent native members; explicitly detach generated or linked geometry first",
        ));
    }
    let controls = selected
        .iter()
        .flat_map(|member| {
            sketch.members[member]
                .curve
                .points()
                .into_iter()
                .map(str::to_string)
        })
        .collect::<BTreeSet<_>>();
    let driven = crate::sketch_edit::driven_points(sketch);
    if controls.iter().any(|point| driven.contains(point)) {
        return Err(invalid(
            "Detach driven corner/offset controls before creating a block",
        ));
    }
    let internal = sketch
        .constraints
        .iter()
        .filter(|constraint| {
            constraint
                .entities
                .iter()
                .any(|entity| selected.contains(entity) || controls.contains(entity))
                && constraint
                    .entities
                    .iter()
                    .all(|entity| selected.contains(entity) || controls.contains(entity))
                && !matches!(
                    constraint.kind.as_str(),
                    "origin"
                        | "fix_x"
                        | "fix_y"
                        | "horizontal"
                        | "vertical"
                        | "rotation"
                        | "distance_x"
                        | "distance_y"
                )
                && !(constraint.kind == "angle" && constraint.entities.len() == 1)
        })
        .cloned()
        .collect::<Vec<_>>();
    let key = id();
    let definition = Definition {
        name: name.into(),
        origin,
        points: controls
            .iter()
            .map(|point| (point.clone(), sketch.points[point]))
            .collect(),
        members: selected
            .iter()
            .map(|member| (member.clone(), sketch.members[member].clone()))
            .collect(),
        constraints: internal.clone(),
    };
    validate_definition(&key, &definition)?;
    let mut candidate = sketch.clone();
    candidate
        .constraints
        .retain(|constraint| !internal.iter().any(|source| source.id == constraint.id));
    candidate.blocks.insert(key.clone(), definition);
    let anchor = id();
    candidate.points.insert(anchor.clone(), origin);
    let instance = id();
    candidate.block_instances.insert(
        instance.clone(),
        Instance {
            definition: key.clone(),
            anchor,
            translation: ["0 mm".into(), "0 mm".into()],
            angle: "0 deg".into(),
            scale: "1".into(),
            free_rotation: true,
            suppressed: false,
            construction: None,
            points: controls
                .into_iter()
                .map(|point| (point.clone(), point))
                .collect(),
            members: selected
                .into_iter()
                .map(|member| (member.clone(), member))
                .collect(),
        },
    );
    derive(doc, owner, &mut candidate, &instance)?;
    crate::sketch_edit::validate(&candidate)?;
    *sketch = candidate;
    Ok((key, instance))
}
pub fn insert(
    doc: &Document,
    owner: &str,
    sketch: &mut Sketch,
    definition: &str,
    position: [f64; 2],
) -> Result<Id> {
    let source = sketch
        .blocks
        .get(definition)
        .cloned()
        .ok_or_else(|| invalid("Choose a saved local block definition"))?;
    let mut candidate = sketch.clone();
    let anchor = id();
    candidate.points.insert(anchor.clone(), position);
    let key = id();
    let points = source
        .points
        .keys()
        .map(|source| (source.clone(), id()))
        .collect::<BTreeMap<_, _>>();
    for target in points.values() {
        candidate.points.insert(target.clone(), [0.; 2]);
    }
    let members = source
        .members
        .keys()
        .map(|source| (source.clone(), id()))
        .collect::<BTreeMap<_, _>>();
    for (source_id, target) in &members {
        candidate.members.insert(
            target.clone(),
            SketchMember {
                construction: source.members[source_id].construction,
                curve: crate::sketch_transform::remap(&source.members[source_id].curve, &points)?,
            },
        );
    }
    candidate.block_instances.insert(
        key.clone(),
        Instance {
            definition: definition.into(),
            anchor,
            translation: ["0 mm".into(), "0 mm".into()],
            angle: "0 deg".into(),
            scale: "1".into(),
            free_rotation: true,
            suppressed: false,
            construction: None,
            points,
            members,
        },
    );
    derive(doc, owner, &mut candidate, &key)?;
    crate::sketch_edit::validate(&candidate)?;
    *sketch = candidate;
    Ok(key)
}
pub fn explode(doc: &Document, owner: &str, sketch: &mut Sketch, key: &str) -> Result<()> {
    let mut candidate = sketch.clone();
    derive(doc, owner, &mut candidate, key)?;
    let instance = candidate
        .block_instances
        .remove(key)
        .ok_or_else(|| invalid("Block instance is missing"))?;
    if instance.suppressed {
        return Err(invalid(
            "Unsuppress the block before exploding its geometry",
        ));
    }
    remove_unused_anchor(&mut candidate, &instance.anchor);
    crate::sketch_edit::validate(&candidate)?;
    *sketch = candidate;
    Ok(())
}
pub fn rotation_variables(sketch: &Sketch) -> Vec<Id> {
    sketch
        .block_instances
        .iter()
        .filter(|(_, instance)| !instance.suppressed && instance.free_rotation)
        .map(|(key, _)| key.clone())
        .collect()
}
pub fn rotation_values(
    doc: &Document,
    owner: &str,
    sketch: &Sketch,
    keys: &[Id],
) -> Result<Vec<f64>> {
    keys.iter()
        .map(|key| {
            expression::value(
                doc,
                owner,
                &sketch.block_instances[key].angle,
                Dimension::Angle,
            )
        })
        .collect()
}
pub fn put_rotations(sketch: &mut Sketch, keys: &[Id], values: &[f64]) -> Result<()> {
    if keys.len() != values.len() || values.iter().any(|value| !value.is_finite()) {
        return Err(invalid("Invalid block rotation solver values"));
    }
    for (key, value) in keys.iter().zip(values) {
        sketch
            .block_instances
            .get_mut(key)
            .ok_or_else(|| invalid("Block solver instance is missing"))?
            .angle = Expr(format!("{value} rad"));
    }
    Ok(())
}

pub fn remove_unused_anchor(sketch: &mut Sketch, anchor: &str) {
    if !sketch
        .members
        .values()
        .any(|member| member.curve.points().contains(&anchor))
        && !sketch
            .constraints
            .iter()
            .any(|constraint| constraint.entities.iter().any(|entity| entity == anchor))
        && !crate::sketch_edit::modifier_ids(sketch).iter().any(|key| {
            crate::sketch_edit::modifier_reference_points(sketch, key)
                .iter()
                .any(|point| point == anchor)
        })
    {
        sketch.points.remove(anchor);
    }
}
