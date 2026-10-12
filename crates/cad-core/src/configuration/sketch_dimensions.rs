//! Stable driving sketch-dimension configuration bindings, independent of solver objects.
use crate::{CadError, Expr, Id, Part, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const CONTRACT_VERSION: u32 = 1;
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct DimensionField {
    pub sketch: Id,
    pub constraint: Id,
    pub label: String,
    pub kind: String,
    pub expression: Expr,
    pub entities: Vec<Id>,
    pub angular: bool,
}
pub fn fields(part: &Part) -> Vec<DimensionField> {
    part.sketches
        .values()
        .flat_map(|sketch| {
            sketch
                .constraints
                .iter()
                .enumerate()
                .filter(|(_, constraint)| {
                    !constraint.driven && crate::sketch_actions::is_dimension(&constraint.kind)
                })
                .map(|(index, constraint)| DimensionField {
                    sketch: sketch.id.clone(),
                    constraint: constraint.id.clone(),
                    label: format!(
                        "{} · {} {}",
                        sketch.name,
                        crate::sketch_actions::label(&constraint.kind),
                        index + 1
                    ),
                    kind: constraint.kind.clone(),
                    expression: constraint.value.clone(),
                    entities: constraint.entities.clone(),
                    angular: matches!(constraint.kind.as_str(), "angle" | "rotation" | "arc_angle"),
                })
        })
        .collect()
}
pub fn validate(part: &Part, overrides: &BTreeMap<Id, BTreeMap<Id, Expr>>) -> Result<()> {
    for (sketch_id, dimensions) in overrides {
        let sketch = part
            .sketches
            .get(sketch_id)
            .ok_or_else(|| CadError::Invalid("Configured dimension sketch is missing".into()))?;
        for (constraint_id, expression) in dimensions {
            let candidates = sketch
                .constraints
                .iter()
                .filter(|constraint| &constraint.id == constraint_id)
                .collect::<Vec<_>>();
            if candidates.len() != 1
                || candidates[0].driven
                || !crate::sketch_actions::is_dimension(&candidates[0].kind)
            {
                return Err(CadError::Invalid(format!(
                    "{} has no unique driving dimension {constraint_id}; reference dimensions cannot drive configurations",
                    sketch.name
                )));
            }
            if expression.0.trim().is_empty() || expression.0.len() > 4096 {
                return Err(CadError::Invalid(
                    "A configured sketch dimension requires bounded expression text".into(),
                ));
            }
            crate::expression::references(&expression.0)?;
        }
    }
    Ok(())
}
pub fn apply(part: &mut Part, overrides: &BTreeMap<Id, BTreeMap<Id, Expr>>) -> Result<()> {
    validate(part, overrides)?;
    for (sketch, dimensions) in overrides {
        for constraint in &mut part
            .sketches
            .get_mut(sketch)
            .ok_or_else(|| CadError::Invalid("Configured dimension sketch is missing".into()))?
            .constraints
        {
            if let Some(expression) = dimensions.get(&constraint.id) {
                constraint.value = expression.clone();
            }
        }
    }
    Ok(())
}
pub fn column(key: &str) -> Result<(&str, &str)> {
    let (sketch, dimension) = key.split_once(':').ok_or_else(|| {
        CadError::Invalid("Sketch dimension columns require sketch:constraint identities".into())
    })?;
    if sketch.is_empty() || dimension.is_empty() || dimension.contains(':') {
        return Err(CadError::Invalid(
            "Invalid sketch dimension column identity".into(),
        ));
    }
    Ok((sketch, dimension))
}
