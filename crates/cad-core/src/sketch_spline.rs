//! Editable spline intent over exact, positive-weight rational curves.
use crate::{sketch_edit::Curve2, *};
use nalgebra::{DMatrix, DVector};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const CONTRACT_VERSION: u32 = 1;
pub const MAX_POINTS: usize = 64;
pub const MAX_DEGREE: usize = 15;
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Parameterization {
    Uniform,
    #[default]
    Chord,
    Centripetal,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Definition {
    Control {
        degree: usize,
        weights: Vec<Expr>,
        knots: Vec<Expr>,
    },
    Fit {
        points: Vec<Id>,
        controls: Vec<Id>,
        degree: usize,
        parameterization: Parameterization,
        weights: Vec<Expr>,
        /// None uses averaged interpolation knots; custom knots remain expressions.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        knots: Option<Vec<Expr>>,
    },
}
fn invalid(message: &str) -> CadError {
    CadError::Invalid(message.into())
}
impl Definition {
    pub fn degree(&self) -> usize {
        match self {
            Self::Control { degree, .. } | Self::Fit { degree, .. } => *degree,
        }
    }
    pub fn degree_mut(&mut self) -> &mut usize {
        match self {
            Self::Control { degree, .. } | Self::Fit { degree, .. } => degree,
        }
    }
    pub fn weights(&self) -> &[Expr] {
        match self {
            Self::Control { weights, .. } | Self::Fit { weights, .. } => weights,
        }
    }
    pub fn weights_mut(&mut self) -> &mut Vec<Expr> {
        match self {
            Self::Control { weights, .. } | Self::Fit { weights, .. } => weights,
        }
    }
    pub fn fit_points(&self) -> Vec<Id> {
        match self {
            Self::Fit { points, .. } => points.clone(),
            _ => vec![],
        }
    }
    pub fn driven_points(&self) -> Vec<Id> {
        match self {
            Self::Fit {
                points, controls, ..
            } => controls
                .iter()
                .filter(|id| !points.contains(id))
                .cloned()
                .collect(),
            _ => vec![],
        }
    }
    pub fn expressions(&self) -> Vec<&Expr> {
        self.weights()
            .iter()
            .chain(match self {
                Self::Control { knots, .. } => knots.iter(),
                Self::Fit { knots, .. } => knots.as_deref().unwrap_or_default().iter(),
            })
            .collect()
    }
    pub fn label(&self) -> &'static str {
        match self {
            Self::Control { .. } => "Control-point NURBS",
            Self::Fit { .. } => "Interpolating Spline",
        }
    }
}

/// Clamped, uniformly spaced knots; no approximation of the control polygon.
pub fn uniform_knots(count: usize, degree: usize) -> Result<Vec<f64>> {
    if !(2..=MAX_POINTS).contains(&count) || degree == 0 || degree >= count || degree > MAX_DEGREE {
        return Err(invalid(
            "Spline needs 2–64 points and degree 1–15 below its point count",
        ));
    }
    let mut knots = vec![0.; degree + 1];
    for i in 1..count - degree {
        knots.push(i as f64 / (count - degree) as f64);
    }
    knots.extend(std::iter::repeat_n(1., degree + 1));
    Ok(knots)
}
pub fn literal_knots(count: usize, degree: usize) -> Result<Vec<Expr>> {
    uniform_knots(count, degree)
        .map(|knots| knots.into_iter().map(|k| Expr(k.to_string())).collect())
}
pub fn editable(sketch: &Sketch, member: &str) -> Result<()> {
    if crate::sketch_conic::operation_for(sketch, member).is_some() {
        return Err(invalid(
            "Use Edit Conic to preserve its parametric source, or detach it before editing NURBS controls",
        ));
    }
    if crate::sketch_edit::driven_member(sketch, member) && !sketch.splines.contains_key(member) {
        return Err(invalid(
            "Detach the source edge or sketch operation before editing spline controls",
        ));
    }
    if !matches!(
        sketch.members.get(member).map(|m| &m.curve),
        Some(SketchCurve::Bezier { .. } | SketchCurve::Nurbs { .. })
    ) {
        return Err(invalid("Select one Bézier or rational spline"));
    }
    Ok(())
}
pub fn definition(sketch: &Sketch, member: &str) -> Result<Definition> {
    editable(sketch, member)?;
    if let Some(definition) = sketch.splines.get(member) {
        return Ok(definition.clone());
    }
    match &sketch.members[member].curve {
        SketchCurve::Bezier { points } => Ok(Definition::Control {
            degree: points.len() - 1,
            weights: vec![Expr("1".into()); points.len()],
            knots: literal_knots(points.len(), points.len() - 1)?,
        }),
        SketchCurve::Nurbs {
            degree,
            weights,
            knots,
            ..
        } => Ok(Definition::Control {
            degree: *degree,
            weights: weights.iter().map(|v| Expr(v.to_string())).collect(),
            knots: knots.iter().map(|v| Expr(v.to_string())).collect(),
        }),
        _ => Err(invalid("Select a spline")),
    }
}
pub fn validate(sketch: &Sketch, member: &str, definition: &Definition) -> Result<()> {
    let record = sketch
        .members
        .get(member)
        .ok_or_else(|| invalid("Spline member is missing"))?;
    let count = record.curve.points().len();
    if !matches!(
        record.curve,
        SketchCurve::Bezier { .. } | SketchCurve::Nurbs { .. }
    ) || !(2..=MAX_POINTS).contains(&count)
        || definition.degree() == 0
        || definition.degree() >= count
        || definition.degree() > MAX_DEGREE
        || definition.weights().len() != count
    {
        return Err(invalid(
            "Spline needs 2–64 controls, one weight per control, and degree 1–15 below its control count",
        ));
    }
    match definition {
        Definition::Control { knots, degree, .. } if knots.len() != count + degree + 1 => {
            return Err(invalid(
                "NURBS knot count must equal control count + degree + 1",
            ));
        }
        Definition::Fit {
            points,
            controls,
            degree,
            knots,
            ..
        } if points.len() != count
            || controls.len() != count
            || controls
                .iter()
                .map(String::as_str)
                .ne(record.curve.points())
            || points.iter().any(|id| !sketch.points.contains_key(id))
            || controls.iter().any(|id| !sketch.points.contains_key(id))
            || controls[0] != points[0]
            || controls[count - 1] != points[count - 1]
            || controls[1..count - 1].iter().any(|id| points.contains(id))
            || controls[1..count - 1].iter().collect::<BTreeSet<_>>().len() != count - 2
            || knots
                .as_ref()
                .is_some_and(|k| k.len() != count + degree + 1) =>
        {
            return Err(invalid(
                "Interpolating spline has missing, duplicate, or inconsistent fit/control references",
            ));
        }
        _ => {}
    }
    Ok(())
}
fn scalar(doc: &Document, owner: &str, expressions: &[Expr]) -> Result<Vec<f64>> {
    expressions
        .iter()
        .map(|expr| expression::value(doc, owner, expr, Dimension::Scalar))
        .collect()
}
fn parameters(points: &[[f64; 2]], parameterization: Parameterization) -> Result<Vec<f64>> {
    let mut result = vec![0.];
    for pair in points.windows(2) {
        let distance = (pair[1][0] - pair[0][0]).hypot(pair[1][1] - pair[0][1]);
        if !distance.is_finite() || distance <= 1e-9 {
            return Err(invalid(
                "Consecutive spline fit points must be distinct by more than 1e-9 mm",
            ));
        }
        let step = match parameterization {
            Parameterization::Uniform => 1.,
            Parameterization::Chord => distance,
            Parameterization::Centripetal => distance.sqrt(),
        };
        result.push(result.last().copied().unwrap_or(0.) + step);
    }
    let total = *result
        .last()
        .ok_or_else(|| invalid("No spline fit points"))?;
    if !total.is_finite() || total <= 0. {
        return Err(invalid("Spline fit point parameterization is degenerate"));
    }
    for value in &mut result {
        *value /= total;
    }
    if result.windows(2).any(|pair| pair[1] <= pair[0]) {
        return Err(invalid(
            "Spline fit point spacing exceeds f64 parameter resolution",
        ));
    }
    Ok(result)
}
fn averaged_knots(parameters: &[f64], degree: usize) -> Vec<f64> {
    let mut knots = vec![0.; degree + 1];
    for start in 1..parameters.len() - degree {
        knots.push(parameters[start..start + degree].iter().sum::<f64>() / degree as f64);
    }
    knots.extend(std::iter::repeat_n(1., degree + 1));
    knots
}
/// Cox–de Boor basis evaluated directly at fit parameters, including clamped ends.
fn basis(knots: &[f64], degree: usize, count: usize, parameter: f64) -> Vec<f64> {
    if parameter == knots[count] {
        let mut result = vec![0.; count];
        result[count - 1] = 1.;
        return result;
    }
    let mut values = (0..knots.len() - 1)
        .map(|i| {
            if knots[i] <= parameter && parameter < knots[i + 1] {
                1.
            } else {
                0.
            }
        })
        .collect::<Vec<_>>();
    for order in 1..=degree {
        for i in 0..knots.len() - order - 1 {
            let first = knots[i + order] - knots[i];
            let second = knots[i + order + 1] - knots[i + 1];
            values[i] = if first > 0. {
                (parameter - knots[i]) / first * values[i]
            } else {
                0.
            } + if second > 0. {
                (knots[i + order + 1] - parameter) / second * values[i + 1]
            } else {
                0.
            };
        }
    }
    values.truncate(count);
    values
}
pub fn derive(
    doc: &Document,
    owner: &str,
    sketch: &mut Sketch,
    member: &str,
    definition: &Definition,
) -> Result<()> {
    validate(sketch, member, definition)?;
    let weights = scalar(doc, owner, definition.weights())?;
    let degree = definition.degree();
    let control_ids = sketch.members[member]
        .curve
        .points()
        .iter()
        .map(|id| (*id).to_owned())
        .collect::<Vec<_>>();
    let mut control_points = control_ids
        .iter()
        .map(|id| sketch.points[id])
        .collect::<Vec<_>>();
    let knots = match definition {
        Definition::Control { knots, .. } => scalar(doc, owner, knots)?,
        Definition::Fit {
            points,
            parameterization,
            knots,
            ..
        } => {
            let fit = points
                .iter()
                .map(|id| sketch.points[id])
                .collect::<Vec<_>>();
            let parameters = parameters(&fit, *parameterization)?;
            let knots = if let Some(knots) = knots {
                scalar(doc, owner, knots)?
            } else {
                averaged_knots(&parameters, degree)
            };
            crate::sketch_edit::validate_nurbs(&Curve2 {
                control_points: control_points.clone(),
                weights: weights.clone(),
                knots: knots.clone(),
                degree,
            })?;
            let start = knots[degree];
            let end = knots[control_ids.len()];
            let rows = parameters
                .iter()
                .map(|parameter| {
                    let u = if *parameter == 1. {
                        end
                    } else {
                        start + parameter * (end - start)
                    };
                    let mut coefficients = basis(&knots, degree, control_ids.len(), u);
                    for (coefficient, weight) in coefficients.iter_mut().zip(&weights) {
                        *coefficient *= weight;
                    }
                    let total = coefficients.iter().sum::<f64>();
                    if !total.is_finite() || total <= 0. {
                        return Err(invalid(
                            "Spline interpolation has a nonpositive rational denominator",
                        ));
                    }
                    for coefficient in &mut coefficients {
                        *coefficient /= total;
                    }
                    Ok(coefficients)
                })
                .collect::<Result<Vec<_>>>()?;
            let matrix = DMatrix::from_fn(fit.len(), fit.len(), |row, column| rows[row][column]);
            let decomposition = matrix.clone().svd(true, true);
            let largest = decomposition.singular_values.max();
            let smallest = decomposition.singular_values.min();
            if !largest.is_finite() || smallest <= largest * 1e-12 {
                return Err(invalid(
                    "Spline interpolation is singular or ill-conditioned; adjust degree, knots, weights, or fit-point spacing",
                ));
            }
            let coordinates = (0..2)
                .map(|axis| {
                    decomposition
                        .solve(
                            &DVector::from_iterator(
                                fit.len(),
                                fit.iter().map(|point| point[axis] - fit[0][axis]),
                            ),
                            largest * 1e-13,
                        )
                        .map_err(|_| invalid("Spline interpolation could not be solved"))
                })
                .collect::<Result<Vec<_>>>()?;
            control_points = (0..fit.len())
                .map(|i| [coordinates[0][i] + fit[0][0], coordinates[1][i] + fit[0][1]])
                .collect();
            // The source endpoint identities remain ordinary constrainable points.
            control_points[0] = fit[0];
            let last = control_points.len() - 1;
            control_points[last] = fit[last];
            for (row, target) in rows.iter().zip(&fit) {
                let point = [
                    row.iter()
                        .zip(&control_points)
                        .map(|(c, p)| c * p[0])
                        .sum::<f64>(),
                    row.iter()
                        .zip(&control_points)
                        .map(|(c, p)| c * p[1])
                        .sum::<f64>(),
                ];
                if (point[0] - target[0]).hypot(point[1] - target[1]) > 1e-7 {
                    return Err(invalid(
                        "Spline interpolation exceeds the 1e-7 mm fit tolerance",
                    ));
                }
            }
            knots
        }
    };
    if knots[degree + 1..control_ids.len()]
        .iter()
        .any(|value| *value <= knots[degree] || *value >= knots[control_ids.len()])
    {
        return Err(invalid(
            "Interior spline knots must lie strictly inside the clamped domain",
        ));
    }
    let mut multiplicity = 0;
    let mut previous = None;
    for knot in &knots[degree + 1..control_ids.len()] {
        multiplicity = if previous == Some(*knot) {
            multiplicity + 1
        } else {
            1
        };
        previous = Some(*knot);
        if multiplicity > degree {
            return Err(invalid(
                "Interior spline knot multiplicity exceeds its degree; disconnected spans are unsupported",
            ));
        }
    }
    crate::sketch_edit::validate_nurbs(&Curve2 {
        control_points: control_points.clone(),
        weights: weights.clone(),
        knots: knots.clone(),
        degree,
    })?;
    for (key, point) in control_ids.iter().zip(control_points) {
        sketch.points.insert(key.clone(), point);
    }
    sketch
        .members
        .get_mut(member)
        .ok_or_else(|| invalid("Spline member is missing"))?
        .curve = SketchCurve::Nurbs {
        points: control_ids,
        weights,
        knots,
        degree,
    };
    Ok(())
}
/// Install parameters atomically. Failed previews leave the preceding definition intact.
pub fn set(
    doc: &Document,
    owner: &str,
    sketch: &mut Sketch,
    member: &str,
    definition: Definition,
) -> Result<()> {
    editable(sketch, member)?;
    let mut candidate = sketch.clone();
    candidate.splines.insert(member.into(), definition);
    crate::sketch_edit::derive(doc, owner, &mut candidate)?;
    *sketch = candidate;
    Ok(())
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Create {
    pub fit: bool,
    pub degree: usize,
    pub parameterization: Parameterization,
    pub construction: bool,
}
pub fn add(
    doc: &Document,
    owner: &str,
    sketch: &mut Sketch,
    points: &[Id],
    options: &Create,
) -> Result<Id> {
    let Create {
        fit,
        degree,
        parameterization,
        construction,
    } = *options;
    if points.iter().any(|id| !sketch.points.contains_key(id)) {
        return Err(invalid("Spline source point is missing"));
    }
    let knots = uniform_knots(points.len(), degree)?;
    let mut candidate = sketch.clone();
    let member = id();
    let controls = if fit {
        points
            .iter()
            .enumerate()
            .map(|(i, key)| {
                if i == 0 || i + 1 == points.len() {
                    key.clone()
                } else {
                    let control = id();
                    candidate
                        .points
                        .insert(control.clone(), candidate.points[key]);
                    control
                }
            })
            .collect::<Vec<_>>()
    } else {
        points.to_vec()
    };
    candidate.members.insert(
        member.clone(),
        SketchMember {
            construction,
            curve: SketchCurve::Nurbs {
                points: controls.clone(),
                weights: vec![1.; controls.len()],
                knots: knots.clone(),
                degree,
            },
        },
    );
    let weights = vec![Expr("1".into()); controls.len()];
    let definition = if fit {
        Definition::Fit {
            points: points.to_vec(),
            controls,
            degree,
            parameterization,
            weights,
            knots: None,
        }
    } else {
        Definition::Control {
            degree,
            weights,
            knots: knots
                .into_iter()
                .map(|value| Expr(value.to_string()))
                .collect(),
        }
    };
    candidate.splines.insert(member.clone(), definition);
    crate::sketch_edit::derive(doc, owner, &mut candidate)?;
    *sketch = candidate;
    Ok(member)
}

/// Remove fit intent while retaining the currently evaluated exact rational curve.
pub fn detach(doc: &Document, owner: &str, sketch: &mut Sketch, member: &str) -> Result<()> {
    if !sketch.splines.contains_key(member) {
        return Err(invalid("Spline has no editable source intent"));
    }
    let mut candidate = sketch.clone();
    crate::sketch_edit::derive(doc, owner, &mut candidate)?;
    candidate.splines.remove(member);
    *sketch = candidate;
    Ok(())
}
