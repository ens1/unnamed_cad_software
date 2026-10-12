//! Saved optical/visibility arrangements, independent of camera and CAD geometry.
use crate::{CadError, Document, Id, Result, presentation::SemanticElement};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
pub const CONTRACT_VERSION: u32 = 1;
pub const MAX_STATES: usize = 128;
pub const MAX_OVERRIDES: usize = 4096;
const MAX_DOCUMENT_OVERRIDES: usize = 65_536;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Visibility {
    Visible,
    Hidden,
    Ghost,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Override {
    pub id: Id,
    /// Full path relative to the owning assembly. A subassembly path affects
    /// its descendants; more specific paths override each channel separately.
    pub path: Vec<Id>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visibility: Option<Visibility>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finish: Option<Id>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Definition {
    pub version: u32,
    pub id: Id,
    pub name: String,
    /// None is reusable across configurations; Some restricts activation and
    /// editing to that configuration without selecting it or changing suppression.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub configuration: Option<Id>,
    #[serde(default)]
    pub overrides: Vec<Override>,
}
impl Definition {
    pub fn new(name: String) -> Self {
        Self {
            version: CONTRACT_VERSION,
            id: crate::id(),
            name,
            configuration: None,
            overrides: vec![],
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Selection {
    pub assembly: Id,
    pub state: Id,
    /// The owning assembly's full occurrence path from the document root.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub owner_path: Vec<Id>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Effect {
    pub visibility: Option<Visibility>,
    pub finish: Option<Id>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ScopedState {
    pub selection: Selection,
    pub definition: Definition,
    pub configuration: Option<Id>,
    pub applicable: bool,
    pub diagnostics: Vec<String>,
    pub inactive_targets: Vec<Id>,
    /// Sorted disposable path index; source row ordering never controls precedence.
    pub path_effects: Vec<(Vec<Id>, Effect)>,
}
fn invalid(message: impl Into<String>) -> CadError {
    CadError::Invalid(format!("Display State: {}", message.into()))
}
fn identity(id: &str) -> bool {
    !id.is_empty() && id.len() <= 256 && !id.chars().any(char::is_control)
}
pub fn validate(doc: &Document) -> Result<()> {
    let mut budget = 0_usize;
    for assembly in doc.assemblies.values() {
        if assembly.display_states.len() > MAX_STATES {
            return Err(invalid("At most 128 states per assembly"));
        }
        let mut names = BTreeSet::new();
        for (id, state) in &assembly.display_states {
            if state.id != *id
                || !identity(id)
                || state.version != CONTRACT_VERSION
                || state.name.trim().is_empty()
                || state.name.len() > 256
                || !names.insert(state.name.to_lowercase())
                || state.overrides.len() > MAX_OVERRIDES
            {
                return Err(invalid(
                    "A saved state needs a unique identity/name, supported version and at most 4096 component overrides",
                ));
            }
            if state
                .configuration
                .as_ref()
                .is_some_and(|id| !crate::configuration::exists(doc, &assembly.id, id))
            {
                return Err(invalid(
                    "The display state's linked assembly configuration is missing",
                ));
            }
            let mut ids = BTreeSet::new();
            let mut paths = BTreeSet::new();
            budget = budget.saturating_add(state.overrides.len());
            if budget > MAX_DOCUMENT_OVERRIDES {
                return Err(invalid("At most 65536 display overrides per document"));
            }
            for target in &state.overrides {
                if !identity(&target.id)
                    || !ids.insert(&target.id)
                    || !paths.insert(&target.path)
                    || target.path.is_empty()
                    || target.path.len() > 64
                    || target.path.iter().any(|id| !identity(id))
                    || target
                        .finish
                        .as_ref()
                        .is_some_and(|id| !doc.finishes.contains_key(id))
                {
                    return Err(invalid(
                        "An override needs a unique source path and valid visibility/finish choices",
                    ));
                }
            }
        }
    }
    Ok(())
}
pub fn put(doc: &mut Document, assembly: &str, state: Definition) -> Result<()> {
    let mut staged = doc.clone();
    staged
        .assemblies
        .get_mut(assembly)
        .ok_or_else(|| invalid("Owning assembly is missing"))?
        .display_states
        .insert(state.id.clone(), state);
    validate(&staged)?;
    *doc = staged;
    Ok(())
}
pub fn remove(doc: &mut Document, assembly: &str, state: &str) -> Result<()> {
    if doc.drawings.iter().any(|drawing| {
        drawing
            .display_state
            .as_ref()
            .is_some_and(|selection| selection.assembly == assembly && selection.state == state)
    }) {
        return Err(invalid(
            "A drawing explicitly uses this display state; change its display selection before deletion",
        ));
    }
    if doc
        .assemblies
        .get_mut(assembly)
        .ok_or_else(|| invalid("Owning assembly is missing"))?
        .display_states
        .remove(state)
        .is_none()
    {
        return Err(invalid("Saved state is missing"));
    }
    Ok(())
}
/// Resolve explicit owner occurrence/configuration scope from authoritative
/// source. Missing occurrences remain diagnostics instead of geometry repair guesses.
pub fn scope(doc: &Document, selection: &Selection) -> Result<ScopedState> {
    if selection.owner_path.len() > 64 {
        return Err(invalid("Owning occurrence path exceeds 64 levels"));
    }
    let state = doc
        .assemblies
        .get(&selection.assembly)
        .and_then(|a| a.display_states.get(&selection.state))
        .ok_or_else(|| invalid("Selected saved state no longer exists"))?
        .clone();
    let mut context = crate::configuration::effective(doc)?;
    let mut definition = doc.root.clone();
    let mut configuration = context.active_configurations.get(&definition).cloned();
    for id in &selection.owner_path {
        let instance = context
            .assemblies
            .get(&definition)
            .and_then(|a| a.instances.get(id))
            .ok_or_else(|| {
                invalid("The state's owning assembly occurrence is absent from this configuration")
            })?
            .clone();
        configuration = crate::configuration::selected(
            &context,
            &instance.definition,
            instance.configuration.as_ref(),
        )
        .cloned();
        context = crate::configuration::scope(
            doc,
            &context,
            &instance.definition,
            configuration.as_deref(),
        )?;
        definition = instance.definition;
    }
    if definition != selection.assembly {
        return Err(invalid(
            "The owning occurrence path resolves to a different definition",
        ));
    }
    let applicable = state.configuration.is_none() || state.configuration == configuration;
    let mut diagnostics = vec![];
    if !applicable {
        diagnostics.push("Select the linked configuration to use this state; the current configuration is unchanged and normal visibility/finishes are shown.".into());
    }
    let mut inactive_targets = vec![];
    if applicable && !state.overrides.is_empty() {
        let mut paths = BTreeSet::new();
        for placed in doc.placed_parts()? {
            if !placed.path.starts_with(&selection.owner_path) {
                continue;
            }
            let relative = &placed.path[selection.owner_path.len()..];
            for length in 1..=relative.len() {
                paths.insert(relative[..length].to_vec());
            }
            if paths.len() > 1_000_000 {
                return Err(invalid(
                    "Configured display path cache exceeds one million occurrence prefixes",
                ));
            }
        }
        for (index, target) in state.overrides.iter().enumerate() {
            if !paths.contains(&target.path) {
                inactive_targets.push(target.id.clone());
                diagnostics.push(format!("Override {} targets an absent/suppressed occurrence; it is retained for repair and has no effect.", index + 1));
            }
        }
    }
    let inactive = inactive_targets.iter().collect::<BTreeSet<_>>();
    let mut path_effects = state
        .overrides
        .iter()
        .filter(|target| !inactive.contains(&target.id))
        .map(|target| {
            (
                target.path.clone(),
                Effect {
                    visibility: target.visibility,
                    finish: target.finish.clone(),
                },
            )
        })
        .collect::<Vec<_>>();
    path_effects.sort_by(|(a, _), (b, _)| a.cmp(b));
    Ok(ScopedState {
        selection: selection.clone(),
        definition: state,
        configuration,
        applicable,
        diagnostics,
        inactive_targets,
        path_effects,
    })
}
pub fn effect(scoped: &ScopedState, full_path: &[Id]) -> Effect {
    if !scoped.applicable || !full_path.starts_with(&scoped.selection.owner_path) {
        return Effect::default();
    }
    let relative = &full_path[scoped.selection.owner_path.len()..];
    let mut effect = Effect::default();
    for length in 1..=relative.len() {
        if let Ok(index) = scoped
            .path_effects
            .binary_search_by(|(path, _)| path.as_slice().cmp(&relative[..length]))
        {
            let target = &scoped.path_effects[index].1;
            if target.visibility.is_some() {
                effect.visibility = target.visibility;
            }
            if target.finish.is_some() {
                effect.finish = target.finish.clone();
            }
        }
    }
    effect
}
pub fn finish_is_used(doc: &Document, finish: &str) -> bool {
    doc.assemblies
        .values()
        .flat_map(|a| a.display_states.values())
        .flat_map(|s| &s.overrides)
        .any(|target| target.finish.as_deref() == Some(finish))
}
pub fn semantic(doc: &Document, active: Option<&Selection>, state: &str) -> Vec<SemanticElement> {
    let mut elements = vec![];
    for assembly in doc.assemblies.values() {
        for definition in assembly.display_states.values() {
            let selected = active.is_some_and(|selection| {
                selection.assembly == assembly.id && selection.state == definition.id
            });
            let identity = format!("{}::display-state:{}", assembly.id, definition.id);
            elements.push(SemanticElement {
                id: identity.clone(), role: "assembly-display-state".into(), label: definition.name.clone(),
                value: definition.configuration.clone(), state: if selected { "selected" } else { state }.into(),
                commands: vec!["assembly.display-state-show".into(),"assembly.display-state-edit".into(),"assembly.display-state-duplicate".into(),"assembly.display-state-delete".into()],
                relationships: vec![assembly.id.clone()],
                data: serde_json::json!({"contract":"tcad.display-states","version":CONTRACT_VERSION,"assembly":assembly.id,"state":definition.id,"source":definition,"configuration":definition.configuration,"camera_independent":true,"geometry_changes":false,"scope":"owning-assembly-occurrence","path_match":"longest-prefix-per-channel","face_finish_precedence":true,"ghost_pickable":false}),
            });
            for target in &definition.overrides {
                elements.push(SemanticElement {
                    id: format!("{identity}::{}", target.id), role: "display-state-component".into(),
                    label: "Component Display Override".into(), value: Some(format!("{:?}", target.visibility)), state: state.into(),
                    commands: vec!["assembly.display-state-select-target".into(),"assembly.display-state-remove-target".into()],
                    relationships: vec![identity.clone()],
                    data: serde_json::json!({"contract":"tcad.display-states","version":CONTRACT_VERSION,"assembly":assembly.id,"state":definition.id,"override":target.id,"source":target}),
                });
            }
        }
    }
    elements
}
