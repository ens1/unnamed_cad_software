//! Explicit drawing presentation scopes; mechanical instances and BOM remain unchanged.
use crate::{
    CadError, Document, Drawing, PlacedPart, Result,
    display_states::{Effect, ScopedState, Selection, Visibility},
};
use serde::{Deserialize, Serialize};
pub const CONTRACT_VERSION: u32 = 1;
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DisplayInstance {
    pub placement: PlacedPart,
    pub effect: Effect,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Info {
    pub version: u32,
    pub selection: Selection,
    pub configuration: Option<crate::Id>,
    pub hidden_components: usize,
    pub ghost_components: usize,
    pub finish_overrides: usize,
    pub ghost_style: String,
    pub annotations_preserved: bool,
    pub diagnostics: Vec<String>,
}
pub fn scope(doc: &Document, sheet: &Drawing) -> Result<Option<ScopedState>> {
    let Some(selection) = &sheet.display_state else {
        return Ok(None);
    };
    if !doc.assemblies.contains_key(&sheet.definition) {
        return Err(CadError::Invalid(
            "Drawing display states require an assembly model scope".into(),
        ));
    }
    let mut scoped = doc.clone();
    scoped.root = sheet.definition.clone();
    let state = crate::display_states::scope(&scoped, selection)?;
    if !state.applicable {
        return Err(CadError::Invalid("Drawing display state belongs to another active configuration; choose a compatible state or select its assembly configuration".into()));
    }
    Ok(Some(state))
}
pub fn apply(scoped: Option<&ScopedState>, instances: Vec<PlacedPart>) -> Vec<DisplayInstance> {
    instances
        .into_iter()
        .map(|placement| {
            let effect = scoped
                .map(|state| crate::display_states::effect(state, &placement.path))
                .unwrap_or_default();
            DisplayInstance { placement, effect }
        })
        .collect()
}
pub fn info(scoped: &ScopedState, instances: &[DisplayInstance]) -> Info {
    Info {
        version: CONTRACT_VERSION,
        selection: scoped.selection.clone(),
        configuration: scoped.configuration.clone(),
        hidden_components: instances
            .iter()
            .filter(|i| i.effect.visibility == Some(Visibility::Hidden))
            .count(),
        ghost_components: instances
            .iter()
            .filter(|i| i.effect.visibility == Some(Visibility::Ghost))
            .count(),
        finish_overrides: instances
            .iter()
            .filter(|i| i.effect.finish.is_some())
            .count(),
        ghost_style: "dashed-reference-curves".into(),
        annotations_preserved: true,
        diagnostics: scoped.diagnostics.clone(),
    }
}
