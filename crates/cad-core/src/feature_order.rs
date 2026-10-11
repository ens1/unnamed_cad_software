//! Authored feature order, constrained by explicit local construction dependencies.
use crate::{CadError, Document, Id, Part, Result};
use std::collections::BTreeMap;

pub const CONTRACT_VERSION: u32 = 1;

pub fn destination(part: &Part, feature: &str, earlier: bool) -> Result<usize> {
    let index = part
        .features
        .iter()
        .position(|f| f.id == feature)
        .ok_or_else(|| CadError::Invalid("The feature to reorder no longer exists".into()))?;
    let position = if earlier {
        index.checked_sub(1)
    } else {
        index.checked_add(1).filter(|p| *p < part.features.len())
    }
    .ok_or_else(|| {
        CadError::Invalid(
            if earlier {
                "This is the first feature"
            } else {
                "This is the last feature"
            }
            .into(),
        )
    })?;
    validate_move(part, feature, position)?;
    Ok(position)
}

pub fn validate_move(part: &Part, feature: &str, position: usize) -> Result<()> {
    if position >= part.features.len() {
        return Err(CadError::Invalid(
            "The feature destination no longer exists".into(),
        ));
    }
    let from = part
        .features
        .iter()
        .position(|f| f.id == feature)
        .ok_or_else(|| CadError::Invalid("The feature to reorder no longer exists".into()))?;
    let mut order = part.features.iter().collect::<Vec<_>>();
    let moving = order.remove(from);
    order.insert(position, moving);
    let positions = order
        .iter()
        .enumerate()
        .map(|(i, f)| (&f.id, i))
        .collect::<BTreeMap<&Id, usize>>();
    for (index, consumer) in order.iter().enumerate() {
        for source in part
            .feature_dependencies(consumer)
            .into_iter()
            .filter(|r| r.definition == part.id)
        {
            let Some(source_index) = positions.get(&source.node) else {
                continue;
            };
            if *source_index >= index {
                return Err(CadError::Invalid(format!(
                    "{} must remain after its source feature {}",
                    consumer.name, order[*source_index].name
                )));
            }
        }
    }
    Ok(())
}

pub fn move_feature(
    document: &mut Document,
    definition: &str,
    feature: &str,
    position: usize,
) -> Result<()> {
    let part = document
        .parts
        .get_mut(definition)
        .ok_or_else(|| CadError::Invalid("The part to reorder no longer exists".into()))?;
    validate_move(part, feature, position)?;
    let from = part
        .features
        .iter()
        .position(|f| f.id == feature)
        .ok_or_else(|| CadError::Invalid("The feature to reorder no longer exists".into()))?;
    let moving = part.features.remove(from);
    part.features.insert(position, moving);
    Ok(())
}
