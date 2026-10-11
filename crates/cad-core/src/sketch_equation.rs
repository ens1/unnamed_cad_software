//! Exact editable polynomial equation curves over application-owned sketch intent.
use crate::{
    CadError, Dimension, Document, Expr, Id, NodeRef, Result, Sketch, SketchCurve, SketchMember,
    expression, presentation::SemanticElement,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const CONTRACT_VERSION: u32 = 1;
pub const MAX_DEGREE: usize = 15;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Definition {
    pub anchor: Id,
    /// Ascending power coefficients: X(t) = x[0] + x[1] t + ... .
    pub x: Vec<Expr>,
    /// Ascending power coefficients: Y(t) = y[0] + y[1] t + ... .
    pub y: Vec<Expr>,
    /// Finite increasing dimensionless parameter interval.
    pub range: [Expr; 2],
    pub angle: Expr,
    /// Exact derived Bernstein controls, including persistent endpoint identities.
    pub controls: Vec<Id>,
}

#[derive(Clone, Debug)]
pub struct Create {
    pub anchor: Id,
    pub x: Vec<Expr>,
    pub y: Vec<Expr>,
    pub range: [Expr; 2],
    pub angle: Expr,
    pub construction: bool,
}

impl Definition {
    pub fn degree(&self) -> usize {
        self.x.len().saturating_sub(1)
    }
    pub fn source_points(&self) -> Vec<Id> {
        vec![self.anchor.clone()]
    }
    pub fn driven_points(&self) -> Vec<Id> {
        self.controls.clone()
    }
    pub fn outputs(&self, key: &str) -> Vec<Id> {
        vec![key.into()]
    }
    pub fn expressions(&self) -> Vec<&Expr> {
        self.x
            .iter()
            .chain(&self.y)
            .chain(&self.range)
            .chain(std::iter::once(&self.angle))
            .collect()
    }
    pub fn request(&self, construction: bool) -> Create {
        Create {
            anchor: self.anchor.clone(),
            x: self.x.clone(),
            y: self.y.clone(),
            range: self.range.clone(),
            angle: self.angle.clone(),
            construction,
        }
    }
}

fn invalid(message: &str) -> CadError {
    CadError::Invalid(format!("Equation Curve: {message}"))
}

fn validate_coefficients(x: &[Expr], y: &[Expr]) -> Result<()> {
    if x.len() != y.len() || !(2..=MAX_DEGREE + 1).contains(&x.len()) {
        return Err(invalid(
            "X and Y require matching ascending coefficient lists for degree 1–15",
        ));
    }
    Ok(())
}

pub fn validate(sketch: &Sketch, key: &str, definition: &Definition) -> Result<()> {
    validate_coefficients(&definition.x, &definition.y)?;
    if key.trim().is_empty()
        || definition.anchor.trim().is_empty()
        || sketch.members.contains_key(&definition.anchor)
        || definition.controls.len() != definition.x.len()
        || definition.controls.iter().collect::<BTreeSet<_>>().len() != definition.controls.len()
        || definition.controls.iter().any(|control| {
            control.trim().is_empty()
                || control == &definition.anchor
                || sketch.members.contains_key(control)
                || !sketch.points.contains_key(control)
        })
        || sketch
            .points
            .get(&definition.anchor)
            .is_none_or(|point| point.iter().any(|v| !v.is_finite()))
        || !matches!(sketch.members.get(key).map(|member| &member.curve), Some(SketchCurve::Bezier { points }) if points == &definition.controls)
    {
        return Err(invalid(
            "The curve requires its source anchor and unique, exact derived Bézier controls",
        ));
    }
    Ok(())
}

/// Horner composition directly in Bernstein form. Multiplication by the exact
/// linear Bernstein polynomial [start, end] raises the degree algebraically;
/// adding the next coefficient adds it to every control. No point sampling,
/// curve fitting or renderer geometry contributes to this representation.
fn bernstein(coefficients: &[f64], range: [f64; 2]) -> Result<Vec<f64>> {
    let mut controls = vec![
        *coefficients
            .last()
            .ok_or_else(|| invalid("No coefficients"))?,
    ];
    for coefficient in coefficients[..coefficients.len() - 1].iter().rev() {
        let degree = controls.len();
        let mut next = Vec::with_capacity(degree + 1);
        for index in 0..=degree {
            let left = if index < degree {
                controls[index] * range[0] * ((degree - index) as f64 / degree as f64)
            } else {
                0.
            };
            let right = if index > 0 {
                controls[index - 1] * range[1] * (index as f64 / degree as f64)
            } else {
                0.
            };
            let value = left + right + coefficient;
            if !left.is_finite() || !right.is_finite() || !value.is_finite() {
                return Err(invalid(
                    "The complete polynomial Bernstein conversion exceeds finite f64 coordinates",
                ));
            }
            next.push(value);
        }
        controls = next;
    }
    Ok(controls)
}

pub fn geometry(
    doc: &Document,
    owner: &str,
    sketch: &Sketch,
    definition: &Definition,
) -> Result<Vec<[f64; 2]>> {
    validate_coefficients(&definition.x, &definition.y)?;
    if crate::sketch_transform::suppressed_points(sketch).contains(&definition.anchor) {
        return Err(invalid(
            "The source anchor is suppressed; restore it or select an active replacement",
        ));
    }
    let scalar = |expression: &Expr| expression::value(doc, owner, expression, Dimension::Scalar);
    let range = [scalar(&definition.range[0])?, scalar(&definition.range[1])?];
    let extent = range[1] - range[0];
    if range.iter().any(|v| !v.is_finite()) || !extent.is_finite() || extent <= 0. {
        return Err(invalid(
            "Parameter limits must be finite and increasing, with a finite positive difference",
        ));
    }
    let coefficients = |values: &[Expr]| {
        values
            .iter()
            .map(|value| {
                let value = expression::value(doc, owner, value, Dimension::Length)?;
                if !value.is_finite() {
                    return Err(invalid("Polynomial coefficients must be finite lengths"));
                }
                Ok(value)
            })
            .collect::<Result<Vec<_>>>()
    };
    let x = bernstein(&coefficients(&definition.x)?, range)?;
    let y = bernstein(&coefficients(&definition.y)?, range)?;
    let anchor = sketch
        .points
        .get(&definition.anchor)
        .filter(|point| point.iter().all(|v| v.is_finite()))
        .ok_or_else(|| invalid("The source anchor is missing or nonfinite"))?;
    let angle = expression::value(doc, owner, &definition.angle, Dimension::Angle)?;
    if !angle.is_finite() || angle.abs() > std::f64::consts::TAU * 1e6 {
        return Err(invalid(
            "Rotation must be finite and within the stable one-million-turn phase range",
        ));
    }
    let (sine, cosine) = angle.rem_euclid(std::f64::consts::TAU).sin_cos();
    let controls = x
        .into_iter()
        .zip(y)
        .map(|(x, y)| {
            let point = [
                anchor[0] + cosine * x - sine * y,
                anchor[1] + sine * x + cosine * y,
            ];
            if point.iter().any(|v| !v.is_finite()) {
                return Err(invalid(
                    "Rotated polynomial controls exceed finite sketch coordinates",
                ));
            }
            Ok(point)
        })
        .collect::<Result<Vec<_>>>()?;
    if controls
        .iter()
        .all(|point| (point[0] - controls[0][0]).hypot(point[1] - controls[0][1]) <= 1e-9)
    {
        return Err(invalid(
            "A constant or collapsed equation does not define a sketch curve",
        ));
    }
    Ok(controls)
}

pub fn derive(
    doc: &Document,
    owner: &str,
    sketch: &mut Sketch,
    key: &str,
    definition: &Definition,
) -> Result<()> {
    validate(sketch, key, definition)?;
    let controls = geometry(doc, owner, sketch, definition)?;
    for (id, point) in definition.controls.iter().zip(controls) {
        sketch.points.insert(id.clone(), point);
    }
    Ok(())
}

fn local_references(sketch: &Sketch, own: &str, retired: &BTreeSet<Id>) -> bool {
    sketch.constraints.iter().any(|constraint| {
        constraint
            .entities
            .iter()
            .any(|entity| retired.contains(entity))
    }) || sketch.members.iter().any(|(member, record)| {
        member != own
            && record
                .curve
                .points()
                .iter()
                .any(|point| retired.contains(*point))
    }) || crate::sketch_edit::modifier_ids(sketch)
        .iter()
        .filter(|key| key.as_str() != own)
        .any(|key| {
            crate::sketch_edit::modifier_reference_points(sketch, key)
                .iter()
                .any(|point| retired.contains(point))
        })
        || sketch
            .profile_order
            .iter()
            .any(|point| retired.contains(point))
        || sketch
            .segments
            .iter()
            .flatten()
            .any(|point| retired.contains(point))
        || sketch
            .circles
            .values()
            .any(|circle| retired.contains(&circle.center))
        || sketch.dimension_groups.values().any(|group| {
            retired.contains(&group.datum)
                || group
                    .entries
                    .iter()
                    .any(|entry| retired.contains(&entry.point))
        })
}

/// Check typed saved source references in other feature/assembly/drawing intent.
/// Context follows profile/path/position references, rather than matching free
/// text or treating a coordinate value as an identity.
fn external_references(
    doc: &Document,
    owner: &str,
    sketch: &Sketch,
    retired: &BTreeSet<Id>,
) -> Result<bool> {
    if retired.is_empty() {
        return Ok(false);
    }
    let profiles = doc.parts.get(owner).into_iter().flat_map(|part| &part.features)
        .filter(|feature| matches!(&feature.operation, crate::Operation::Profile { profile: crate::Profile::Sketch { sketch: id }, .. } if id == &sketch.id))
        .map(|feature| NodeRef { definition: owner.into(), node: feature.id.clone() })
        .collect::<BTreeSet<_>>();
    fn walk(
        value: &serde_json::Value,
        context: bool,
        profiles: &BTreeSet<NodeRef>,
        retired: &BTreeSet<Id>,
    ) -> bool {
        match value {
            serde_json::Value::Object(object) => {
                let refs = ["profile", "path_sketch", "positions", "sketch"]
                    .into_iter()
                    .filter_map(|key| object.get(key))
                    .filter_map(|value| serde_json::from_value::<NodeRef>(value.clone()).ok())
                    .collect::<Vec<_>>();
                let context = if refs.is_empty() {
                    context
                } else {
                    refs.iter().any(|reference| profiles.contains(reference))
                };
                object.iter().any(|(key, value)| {
                    if matches!(key.as_str(), "target_vertex" | "vertex")
                        && let Some(pair) = value.as_array()
                        && pair.len() == 2
                        && serde_json::from_value::<NodeRef>(pair[0].clone())
                            .is_ok_and(|reference| profiles.contains(&reference))
                        && pair[1]
                            .as_str()
                            .is_some_and(|point| retired.contains(point))
                    {
                        return true;
                    }
                    if context
                        && matches!(
                            key.as_str(),
                            "point"
                                | "points"
                                | "vertex"
                                | "start_vertex"
                                | "path_start_vertex"
                                | "anchor"
                                | "controls"
                                | "entities"
                        )
                    {
                        if value.as_str().is_some_and(|point| retired.contains(point)) {
                            return true;
                        }
                        if value.as_array().is_some_and(|points| {
                            points.iter().any(|point| {
                                point.as_str().is_some_and(|point| retired.contains(point))
                            })
                        }) {
                            return true;
                        }
                    }
                    walk(value, context, profiles, retired)
                })
            }
            serde_json::Value::Array(values) => values
                .iter()
                .any(|value| walk(value, context, profiles, retired)),
            _ => false,
        }
    }
    let mut source = serde_json::to_value(doc)?;
    if let Some(sketches) = source
        .get_mut("parts")
        .and_then(|parts| parts.get_mut(owner))
        .and_then(|part| part.get_mut("sketches"))
        .and_then(serde_json::Value::as_object_mut)
    {
        sketches.remove(&sketch.id);
    }
    Ok(walk(&source, false, &profiles, retired))
}

pub fn set(
    doc: &Document,
    owner: &str,
    sketch: &mut Sketch,
    key: &str,
    request: &Create,
) -> Result<()> {
    validate_coefficients(&request.x, &request.y)?;
    if sketch.members.contains_key(key) && !sketch.equations.contains_key(key) {
        return Err(invalid(
            "Select an existing equation curve or create a new member; independent geometry cannot be silently replaced",
        ));
    }
    let previous = sketch.equations.get(key);
    if let Some(previous) = previous {
        validate(sketch, key, previous)?;
    }
    let count = request.x.len();
    let controls = if let Some(previous) = previous {
        let mut controls = vec![previous.controls[0].clone()];
        controls.extend(
            previous.controls[1..previous.controls.len() - 1]
                .iter()
                .take(count - 2)
                .cloned(),
        );
        while controls.len() < count - 1 {
            controls.push(format!("{key}:equation-control:{}", controls.len()));
        }
        controls.push(
            previous
                .controls
                .last()
                .ok_or_else(|| invalid("The existing endpoint identity is missing"))?
                .clone(),
        );
        controls
    } else {
        std::iter::once(format!("{key}:equation-start"))
            .chain((1..count - 1).map(|index| format!("{key}:equation-control:{index}")))
            .chain(std::iter::once(format!("{key}:equation-end")))
            .collect()
    };
    if controls.iter().any(|point| {
        sketch.members.contains_key(point)
            || sketch.points.contains_key(point)
                && previous.is_none_or(|previous| !previous.controls.contains(point))
    }) {
        return Err(invalid(
            "A newly reserved control identity is already owned by other sketch geometry",
        ));
    }
    let retained = controls.iter().cloned().collect::<BTreeSet<_>>();
    let retired = previous
        .into_iter()
        .flat_map(|definition| &definition.controls)
        .filter(|point| !retained.contains(*point))
        .cloned()
        .collect::<BTreeSet<_>>();
    if local_references(sketch, key, &retired) || external_references(doc, owner, sketch, &retired)?
    {
        return Err(invalid(
            "This degree edit retires referenced controls; repair or explicitly detach their dependencies first",
        ));
    }
    let definition = Definition {
        anchor: request.anchor.clone(),
        x: request.x.clone(),
        y: request.y.clone(),
        range: request.range.clone(),
        angle: request.angle.clone(),
        controls,
    };
    let mut candidate = sketch.clone();
    for point in retired {
        candidate.points.remove(&point);
    }
    for point in &definition.controls {
        candidate.points.entry(point.clone()).or_insert([0.; 2]);
    }
    candidate.members.insert(
        key.into(),
        SketchMember {
            construction: request.construction,
            curve: SketchCurve::Bezier {
                points: definition.controls.clone(),
            },
        },
    );
    candidate.equations.insert(key.into(), definition.clone());
    derive(doc, owner, &mut candidate, key, &definition)?;
    crate::sketch_edit::validate(&candidate)?;
    *sketch = candidate;
    Ok(())
}

pub fn semantic_fields(
    doc: &Document,
    owner: &str,
    sketch: &Sketch,
    key: &str,
    definition: &Definition,
    state: &str,
) -> Vec<SemanticElement> {
    let target = serde_json::json!({"kind":"sketch-operation","definition":owner,"sketch":sketch.id,"operation":key,"member":key});
    let mut fields = vec![SemanticElement {
        id: format!("{owner}::{}::equation::{key}", sketch.id),
        role: "sketch-equation".into(),
        label: "Equation Curve".into(),
        value: Some(format!("Degree {} polynomial", definition.degree())),
        state: state.into(),
        commands: vec![
            "sketch.edit-equation".into(),
            "sketch.toggle-construction".into(),
            "sketch.detach-conversion".into(),
            "sketch.delete-selection".into(),
        ],
        relationships: vec![sketch.id.clone(), key.into(), definition.anchor.clone()],
        data: serde_json::json!({"contract":"tcad.sketch-equation","version":CONTRACT_VERSION,"target":target,"definition":owner,"sketch":sketch.id,"operation_id":key,"member":key,"source":definition,"parameter":"dimensionless_t","coefficients":"ascending_power_basis_length_expressions","derived":"exact_algebraic_bernstein_controls","geometry":geometry(doc,owner,sketch,definition).ok(),"editable":true}),
    }];
    let quantities = definition
        .x
        .iter()
        .enumerate()
        .map(|(index, value)| {
            (
                format!("x.{index}"),
                format!("X · t^{index}"),
                value,
                Dimension::Length,
            )
        })
        .chain(definition.y.iter().enumerate().map(|(index, value)| {
            (
                format!("y.{index}"),
                format!("Y · t^{index}"),
                value,
                Dimension::Length,
            )
        }))
        .chain([
            (
                String::from("range.0"),
                String::from("Parameter Start"),
                &definition.range[0],
                Dimension::Scalar,
            ),
            (
                String::from("range.1"),
                String::from("Parameter End"),
                &definition.range[1],
                Dimension::Scalar,
            ),
            (
                String::from("angle"),
                String::from("Rotation"),
                &definition.angle,
                Dimension::Angle,
            ),
        ]);
    for (field, label, expression, dimension) in quantities {
        fields.push(SemanticElement { id: format!("{owner}::{}::equation::{key}::{field}", sketch.id), role: "sketch-equation-field".into(), label, value: Some(expression.0.clone()), state: state.into(), commands: vec!["sketch.edit-equation".into()], relationships: vec![sketch.id.clone(), key.into()], data: serde_json::json!({"contract":"tcad.sketch-equation","version":CONTRACT_VERSION,"target":target,"definition":owner,"sketch":sketch.id,"operation_id":key,"member":key,"field":field,"dimension":dimension,"expression":expression,"evaluated":expression::value(doc,owner,expression,dimension).ok(),"editable":true}) });
    }
    fields
}
