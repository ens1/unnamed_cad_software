//! Native grouped linear dimensions. Constraint values remain the sole dimensional authority.
use crate::{
    sketch_actions::{self as actions, SketchEntity},
    *,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const CONTRACT_VERSION: u32 = 1;
pub const MAX_ENTRIES: usize = 128;
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Ordinate,
    Baseline,
    Chain,
}
impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Ordinate => "Ordinate Dimensions",
            Self::Baseline => "Baseline Dimensions",
            Self::Chain => "Chain Dimensions",
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Axis {
    X,
    Y,
}
impl Axis {
    pub fn constraint(self) -> &'static str {
        match self {
            Self::X => "distance_x",
            Self::Y => "distance_y",
        }
    }
    pub fn index(self) -> usize {
        match self {
            Self::X => 0,
            Self::Y => 1,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    pub point: Id,
    pub constraint: Id,
    #[serde(default, skip_serializing_if = "zero_offset")]
    pub label_offset: [f64; 2],
}
fn zero_offset(offset: &[f64; 2]) -> bool {
    *offset == [0.; 2]
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Group {
    pub kind: Kind,
    pub datum: Id,
    pub axis: Axis,
    pub entries: Vec<Entry>,
    pub offset: f64,
    pub spacing: f64,
}
#[derive(Clone, Debug)]
pub struct Value {
    pub point: Id,
    pub constraint: Id,
    pub value: Expr,
    pub driven: bool,
    pub label_offset: [f64; 2],
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Annotation {
    pub constraint: Option<Id>,
    pub point: Id,
    pub label: [f64; 2],
    pub segments: Vec<[[f64; 2]; 2]>,
    pub arrows: Vec<[[f64; 2]; 2]>,
    pub measured: f64,
    pub driven: bool,
    pub zero: bool,
}
fn invalid(message: &str) -> CadError {
    CadError::Invalid(message.into())
}
fn point(sketch: &Sketch, key: &str) -> Result<[f64; 2]> {
    if key != actions::ORIGIN && !sketch.points.contains_key(key) {
        return Err(invalid(
            "Grouped dimensions need a current sketch point or the sketch origin",
        ));
    }
    let value = actions::entity_point(sketch, key)?;
    if value.iter().any(|v| !v.is_finite()) {
        return Err(invalid("Grouped dimension coordinates must be finite"));
    }
    Ok(value)
}
pub fn selected_points(sketch: &Sketch, selection: &[SketchEntity]) -> Vec<Id> {
    let inactive = crate::sketch_transform::inactive_entities(sketch);
    let mut points = vec![];
    for entity in selection {
        let keys = match entity {
            SketchEntity::Point { point } => vec![point.clone()],
            SketchEntity::Member { member } if !inactive.contains(member) => sketch
                .members
                .get(member)
                .map(|member| match &member.curve {
                    SketchCurve::Circle { center, .. } | SketchCurve::Ellipse { center, .. } => {
                        vec![center.clone()]
                    }
                    curve => {
                        let keys = curve.points();
                        keys.first()
                            .into_iter()
                            .chain(keys.last())
                            .map(|point| point.to_string())
                            .collect()
                    }
                })
                .unwrap_or_default(),
            _ => vec![],
        };
        for point in keys {
            if sketch.points.contains_key(&point)
                && !inactive.contains(&point)
                && !points.contains(&point)
            {
                points.push(point);
            }
        }
    }
    points
}
pub fn group_for_constraint<'a>(
    sketch: &'a Sketch,
    constraint: &str,
) -> Option<(&'a Id, &'a Group)> {
    sketch.dimension_groups.iter().find(|(_, group)| {
        group
            .entries
            .iter()
            .any(|entry| entry.constraint == constraint)
    })
}
pub fn values(sketch: &Sketch, group: &Group) -> Result<Vec<Value>> {
    group
        .entries
        .iter()
        .map(|entry| {
            let constraint = sketch
                .constraints
                .iter()
                .find(|constraint| constraint.id == entry.constraint)
                .ok_or_else(|| invalid("Grouped dimension constraint is missing"))?;
            Ok(Value {
                point: entry.point.clone(),
                constraint: entry.constraint.clone(),
                value: constraint.value.clone(),
                driven: constraint.driven,
                label_offset: entry.label_offset,
            })
        })
        .collect()
}
pub fn measured_values(
    sketch: &Sketch,
    kind: Kind,
    datum: &str,
    axis: Axis,
    points: &[Id],
    driven: bool,
) -> Result<Vec<Value>> {
    let inactive = crate::sketch_transform::inactive_entities(sketch);
    if inactive.contains(datum) || points.iter().any(|key| inactive.contains(key)) {
        return Err(invalid(
            "New grouped dimensions require active sketch points; suppressed entries remain saved in their existing groups",
        ));
    }
    point(sketch, datum)?;
    let mut previous = datum.to_string();
    points
        .iter()
        .map(|key| {
            let a = point(
                sketch,
                if kind == Kind::Chain {
                    &previous
                } else {
                    datum
                },
            )?;
            let b = point(sketch, key)?;
            let distance = b[axis.index()] - a[axis.index()];
            if !distance.is_finite() {
                return Err(invalid(
                    "Grouped dimension distance exceeds finite millimetre coordinates",
                ));
            }
            previous = key.clone();
            Ok(Value {
                point: key.clone(),
                constraint: id(),
                value: Expr(format!("{distance} mm")),
                driven,
                label_offset: [0.; 2],
            })
        })
        .collect()
}
pub fn set(sketch: &mut Sketch, key: &str, group: Group, values: &[Value]) -> Result<()> {
    if group.entries.len() != values.len()
        || group.entries.iter().zip(values).any(|(entry, value)| {
            entry.point != value.point || entry.constraint != value.constraint
        })
    {
        return Err(invalid(
            "Grouped dimension entries and native values must have matching stable identities",
        ));
    }
    let mut candidate = sketch.clone();
    let old = candidate
        .dimension_groups
        .get(key)
        .map(|group| {
            group
                .entries
                .iter()
                .map(|entry| entry.constraint.clone())
                .collect::<BTreeSet<_>>()
        })
        .unwrap_or_default();
    let reserved = candidate
        .dimension_groups
        .iter()
        .filter(|(id, _)| id.as_str() != key)
        .flat_map(|(_, group)| group.entries.iter().map(|entry| entry.constraint.clone()))
        .collect::<BTreeSet<_>>();
    if group.entries.iter().any(|entry| {
        reserved.contains(&entry.constraint)
            || candidate.constraints.iter().any(|constraint| {
                constraint.id == entry.constraint && !old.contains(&constraint.id)
            })
    }) {
        return Err(invalid(
            "A grouped dimension cannot replace another independent constraint",
        ));
    }
    candidate
        .constraints
        .retain(|constraint| !old.contains(&constraint.id));
    let mut previous = group.datum.clone();
    for value in values {
        candidate.constraints.push(SketchConstraint {
            id: value.constraint.clone(),
            kind: group.axis.constraint().into(),
            entities: vec![
                if group.kind == Kind::Chain {
                    previous.clone()
                } else {
                    group.datum.clone()
                },
                value.point.clone(),
            ],
            value: value.value.clone(),
            driven: value.driven,
            contact: None,
            distance_side: None,
        });
        previous = value.point.clone();
    }
    candidate.dimension_groups.insert(key.into(), group);
    validate(&candidate)?;
    *sketch = candidate;
    Ok(())
}
pub fn validate(sketch: &Sketch) -> Result<()> {
    let mut used = BTreeSet::new();
    for group in sketch.dimension_groups.values() {
        point(sketch, &group.datum)?;
        if group.entries.is_empty()
            || group.entries.len() > MAX_ENTRIES
            || !group.offset.is_finite()
            || !group.spacing.is_finite()
            || group.spacing <= 0.
        {
            return Err(invalid(
                "Grouped dimensions need 1–128 entries, finite annotation offset and positive spacing",
            ));
        }
        let mut previous = &group.datum;
        let mut points = BTreeSet::new();
        for entry in &group.entries {
            point(sketch, &entry.point)?;
            if entry.point == group.datum
                || !points.insert(&entry.point)
                || !used.insert(&entry.constraint)
                || entry.label_offset.iter().any(|v| !v.is_finite())
            {
                return Err(invalid(
                    "Grouped dimensions have a repeated datum/target/constraint or nonfinite annotation position",
                ));
            }
            let entities = vec![
                if group.kind == Kind::Chain {
                    previous.clone()
                } else {
                    group.datum.clone()
                },
                entry.point.clone(),
            ];
            let matches = sketch
                .constraints
                .iter()
                .filter(|constraint| constraint.id == entry.constraint)
                .collect::<Vec<_>>();
            if matches.len() != 1
                || matches[0].kind != group.axis.constraint()
                || matches[0].entities != entities
                || matches[0].contact.is_some()
            {
                return Err(invalid(
                    "Grouped dimension constraints must retain their saved datum, direction and ordered target identities",
                ));
            }
            previous = &entry.point;
        }
        all_annotations(sketch, group)?;
    }
    Ok(())
}
/// A destructive native edit explicitly releases invalid grouping metadata;
/// surviving constraints remain ordinary dimensions with their authored equations.
pub fn reconcile_removed(sketch: &mut Sketch) {
    let keys = sketch.dimension_groups.keys().cloned().collect::<Vec<_>>();
    for key in keys {
        let group = &sketch.dimension_groups[&key];
        let mut previous = &group.datum;
        let valid = point(sketch, &group.datum).is_ok()
            && group.entries.iter().all(|entry| {
                let entities = vec![
                    if group.kind == Kind::Chain {
                        previous.clone()
                    } else {
                        group.datum.clone()
                    },
                    entry.point.clone(),
                ];
                previous = &entry.point;
                point(sketch, &entry.point).is_ok()
                    && sketch.constraints.iter().any(|constraint| {
                        constraint.id == entry.constraint
                            && constraint.kind == group.axis.constraint()
                            && constraint.entities == entities
                    })
            });
        if !valid {
            sketch.dimension_groups.remove(&key);
        }
    }
}
pub fn remove(sketch: &mut Sketch, key: &str) -> Result<()> {
    let group = sketch
        .dimension_groups
        .remove(key)
        .ok_or_else(|| invalid("Dimension group is missing"))?;
    let ids = group
        .entries
        .into_iter()
        .map(|entry| entry.constraint)
        .collect::<BTreeSet<_>>();
    sketch
        .constraints
        .retain(|constraint| !ids.contains(&constraint.id));
    Ok(())
}
fn all_annotations(sketch: &Sketch, group: &Group) -> Result<Vec<Annotation>> {
    let datum = point(sketch, &group.datum)?;
    let axis = group.axis.index();
    let perp = 1 - axis;
    let sign = if group.offset < 0. { -1. } else { 1. };
    let mut previous = datum;
    let mut annotations = vec![];
    if group.kind == Kind::Ordinate {
        let mut position = datum;
        position[perp] += group.offset;
        if position.iter().any(|v| !v.is_finite()) {
            return Err(invalid(
                "Grouped dimension datum annotation exceeds the finite coordinate range",
            ));
        }
        annotations.push(Annotation {
            constraint: None,
            point: group.datum.clone(),
            label: position,
            segments: vec![[datum, position]],
            arrows: vec![],
            measured: 0.,
            driven: true,
            zero: true,
        });
    }
    for (index, entry) in group.entries.iter().enumerate() {
        let target = point(sketch, &entry.point)?;
        let origin = if group.kind == Kind::Chain {
            previous
        } else {
            datum
        };
        let level = datum[perp]
            + group.offset
            + if group.kind == Kind::Chain {
                0.
            } else {
                sign * group.spacing * index as f64
            };
        let mut a = origin;
        a[perp] = level;
        let mut b = target;
        b[perp] = level;
        let mut label = if group.kind == Kind::Ordinate {
            b
        } else {
            [a[0] / 2. + b[0] / 2., a[1] / 2. + b[1] / 2.]
        };
        label[perp] += sign * group.spacing / 4.;
        label[0] += entry.label_offset[0];
        label[1] += entry.label_offset[1];
        let constraint = sketch
            .constraints
            .iter()
            .find(|constraint| constraint.id == entry.constraint)
            .ok_or_else(|| invalid("Grouped annotation constraint is missing"))?;
        let (segments, arrows) = if group.kind == Kind::Ordinate {
            (vec![[target, b]], vec![[b, target]])
        } else {
            (vec![[origin, a], [target, b], [a, b]], vec![[a, b], [b, a]])
        };
        if label
            .iter()
            .chain(segments.iter().flatten().flatten())
            .any(|value| !value.is_finite())
        {
            return Err(invalid(
                "Grouped dimension annotation exceeds the finite coordinate range",
            ));
        }
        let measured = target[axis] - origin[axis];
        if !measured.is_finite() {
            return Err(invalid(
                "Grouped dimension measurement exceeds the finite coordinate range",
            ));
        }
        annotations.push(Annotation {
            constraint: Some(entry.constraint.clone()),
            point: entry.point.clone(),
            label,
            segments,
            arrows,
            measured,
            driven: constraint.driven,
            zero: false,
        });
        previous = target;
    }
    Ok(annotations)
}

/// Suppression changes visibility, never the saved chain station or label order.
pub fn annotations(sketch: &Sketch, group: &Group) -> Result<Vec<Annotation>> {
    let mut annotations = all_annotations(sketch, group)?;
    let inactive = crate::sketch_transform::inactive_entities(sketch);
    if inactive.contains(&group.datum) {
        return Ok(vec![]);
    }
    annotations.retain(|annotation| {
        annotation.constraint.as_ref().is_none_or(|key| {
            sketch.constraints.iter().any(|constraint| {
                &constraint.id == key
                    && !constraint.entities.iter().any(|key| inactive.contains(key))
            })
        })
    });
    if !annotations.iter().any(|annotation| !annotation.zero) {
        annotations.clear();
    }
    Ok(annotations)
}

/// A group selection must resolve to an active native equation, including both
/// saved chain stations. Fully inactive groups remain available in source editing.
pub fn selection_constraint<'a>(sketch: &Sketch, group: &'a Group) -> Option<&'a Id> {
    let inactive = crate::sketch_transform::inactive_entities(sketch);
    if inactive.contains(&group.datum) {
        return None;
    }
    group.entries.iter().find_map(|entry| {
        sketch
            .constraints
            .iter()
            .any(|constraint| {
                constraint.id == entry.constraint
                    && !constraint.entities.iter().any(|key| inactive.contains(key))
            })
            .then_some(&entry.constraint)
    })
}

pub fn selected_group(sketch: &Sketch, selection: &[SketchEntity]) -> Option<Id> {
    let inactive = crate::sketch_transform::inactive_entities(sketch);
    selection.iter().find_map(|entity| match entity {
        SketchEntity::Constraint { constraint } => group_for_constraint(sketch, constraint)
            .filter(|(_, group)| !inactive.contains(&group.datum))
            .filter(|_| {
                sketch.constraints.iter().any(|native| {
                    native.id == *constraint
                        && !native.entities.iter().any(|key| inactive.contains(key))
                })
            })
            .map(|(key, _)| key.clone()),
        _ => None,
    })
}
pub fn semantics(sketch: &Sketch) -> Vec<crate::presentation::SemanticElement> {
    let inactive = crate::sketch_transform::inactive_entities(sketch);
    sketch
        .dimension_groups
        .iter()
        .map(|(id, group)| {
            let active = group
                .entries
                .iter()
                .filter_map(|entry| {
                    sketch.constraints.iter().find(|constraint| {
                        constraint.id == entry.constraint
                            && !inactive.contains(&group.datum)
                            && !constraint.entities.iter().any(|key| inactive.contains(key))
                    })
                })
                .collect::<Vec<_>>();
            crate::presentation::SemanticElement {
                id: id.clone(),
                role: "sketch-dimension-group".into(),
                label: group.kind.label().into(),
                value: Some(format!(
                    "{} {} dimensions",
                    group.entries.len(),
                    if group.axis == Axis::X { "X" } else { "Y" }
                )),
                state: if active.is_empty() {
                    "suppressed"
                } else if active.iter().all(|constraint| constraint.driven) {
                    "reference-dimensions"
                } else {
                    "driving-dimensions"
                }
                .into(),
                commands: vec![
                    "sketch.edit-dimension-group".into(),
                    "sketch.delete-dimension-group".into(),
                ],
                relationships: vec![sketch.id.clone(), group.datum.clone()]
                    .into_iter()
                    .chain(group.entries.iter().flat_map(|entry| {
                        [entry.point.clone(), entry.constraint.clone()]
                    }))
                    .collect(),
                data: serde_json::json!({
                    "contract":"tcad.sketch-dimensions", "version":CONTRACT_VERSION,
                    "sketch":sketch.id, "group_id":id, "group":group,
                    "active_constraints":active.iter().map(|constraint|&constraint.id).collect::<Vec<_>>(),
                    "constraints":values(sketch,group).ok().map(|values|values.into_iter().map(|value|serde_json::json!({"id":value.constraint,"value":value.value,"driven":value.driven})).collect::<Vec<_>>()),
                    "annotations":annotations(sketch,group).ok()
                }),
            }
        })
        .collect()
}
