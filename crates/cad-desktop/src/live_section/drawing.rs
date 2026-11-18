//! Transfer the cutting plane and effective occurrence visibility to paper.
use super::{Workspace, from_axes, plane, viewport};
use bevy::math::DVec3;
use cad_core::{
    CadError, Document, Drawing, Result,
    display_states::{self, Visibility},
    protocol::Evaluation,
};

/// Temporary isolation and ghosting are viewport preferences. Save their actual
/// effects with the sheet so the worker sees exactly the same sectioned bodies.
fn display_snapshot(
    w: &Workspace,
    e: &Evaluation,
    sheet: &mut Drawing,
) -> Result<Option<Document>> {
    if !w.view.isolation {
        return Ok(None);
    }
    let doc = w.session.visible();
    let Some(owner) = doc.assemblies.get(&sheet.definition) else {
        return Ok(None);
    };
    let mut differences = false;
    let mut rows = Vec::new();
    for i in viewport::display_instances(w, e) {
        let effect = crate::display_states_ui::effect(w, &i.path);
        let visibility = if !viewport::visible(w, i) {
            Visibility::Hidden
        } else if viewport::ghost_neighbor(w, i) {
            Visibility::Ghost
        } else {
            Visibility::Visible
        };
        differences |= visibility != effect.visibility.unwrap_or(Visibility::Visible);
        if visibility != Visibility::Visible || effect.finish.is_some() {
            rows.push(display_states::Override {
                id: cad_core::id(),
                path: i.path.clone(),
                visibility: Some(visibility),
                finish: effect.finish,
            });
        }
    }
    if !differences {
        return Ok(None);
    }
    let name = (1..)
        .map(|i| {
            if i == 1 {
                format!("{} visibility", sheet.name)
            } else {
                format!("{} visibility {i}", sheet.name)
            }
        })
        .find(|name| {
            owner
                .display_states
                .values()
                .all(|s| !s.name.eq_ignore_ascii_case(name))
        })
        .unwrap();
    let mut state = display_states::Definition::new(name);
    state.configuration = doc.active_configurations.get(&sheet.definition).cloned();
    state.overrides = rows;
    sheet.display_state = Some(display_states::Selection {
        assembly: sheet.definition.clone(),
        state: state.id.clone(),
        owner_path: vec![],
    });
    let mut updated = doc.clone();
    display_states::put(&mut updated, &sheet.definition, state)?;
    let effect = cad_core::drawing_display_states::scope(&updated, sheet)?;
    if effect.is_some_and(|s| !s.inactive_targets.is_empty()) {
        return Err(CadError::Invalid("A hidden or ghost item cannot be represented on this sheet. Show the full assembly to transfer the section.".into()));
    }
    Ok(Some(updated))
}
pub(super) fn create(w: &mut Workspace) -> Result<()> {
    let mut f = plane(w).ok_or_else(|| CadError::Invalid("Choose a section plane".into()))?;
    let doc = w.session.visible();
    let e = viewport::selection_evaluation(w)
        .ok_or_else(|| CadError::Invalid("Wait for the model to finish".into()))?;
    let mut shown = viewport::display_instances(w, e)
        .into_iter()
        .filter(|i| viewport::visible(w, i));
    let one = shown.next().filter(|_| shown.next().is_none());
    let single = one.filter(|i| {
        doc.parts.contains_key(&i.definition)
            && !viewport::ghost_neighbor(w, i)
            && i.configuration.as_ref() == doc.active_configurations.get(&i.definition)
    });
    let scope = single.map_or(&doc.root, |i| &i.definition);
    if let Some(i) = single {
        let origin = DVec3::from(viewport::placed([0.; 3], i));
        let inverse = viewport::normal_matrix(i).transpose();
        f = from_axes(
            (inverse * (DVec3::from(f.origin) - origin)).to_array(),
            inverse * DVec3::from(f.u),
            inverse * DVec3::from(f.v),
        )
        .ok_or_else(|| {
            CadError::Invalid("The component placement cannot define a section plane".into())
        })?;
    }
    let name = (1..)
        .map(|n| {
            if n == 1 {
                "Section".into()
            } else {
                format!("Section {n}")
            }
        })
        .find(|name| doc.drawings.iter().all(|s| &s.name != name))
        .unwrap();
    let mut sheet = cad_core::drawing::new_sheet(doc, scope, name)?;
    if single.is_none() {
        sheet.display_state = w.view.display_state.clone();
        if let Some(exploded) = &w.view.exploded_view {
            if doc
                .assemblies
                .get(scope)
                .and_then(|a| a.exploded_views.get(&exploded.view))
                .is_none_or(|v| v.steps.len() != exploded.stage)
            {
                return Err(CadError::Invalid(
                    "Use the final exploded stage or the assembled view for this drawing".into(),
                ));
            }
            sheet.exploded_view = Some(exploded.view.clone());
        }
    }
    let parent = ["front", "top", "right"].into_iter().find(|p| f.edge_on(p));
    let views = parent
        .into_iter()
        .chain([cad_core::drawing_sections::KEY])
        .map(str::to_owned)
        .collect();
    cad_core::drawing::set_views(&mut sheet, views)?;
    sheet.section = Some(cad_core::drawing_sections::Definition {
        id: cad_core::id(),
        parent: parent.unwrap_or("front").into(),
        at: [0.; 2],
        through: None,
        model_plane: Some(Box::new(f)),
        direction: [1., 0.],
        reversed: false,
    });
    let snapshot = if single.is_none() {
        display_snapshot(w, e, &mut sheet)?
    } else {
        None
    };
    if !crate::drawing_ui::open_workspace(w) {
        return Err(CadError::Invalid(w.status.clone()));
    }
    let id = sheet.id.clone();
    if let Some(mut updated) = snapshot {
        cad_core::drawing::put_sheet(&mut updated, sheet.clone())?;
        let source = serde_json::to_string(&sheet)?;
        w.start_tool();
        w.dispatch(cad_core::commands::Command::ReplaceDocument { document: updated });
        if !w.session.visible().drawings.contains(&sheet) {
            return Err(CadError::Invalid(w.status.clone()));
        }
        w.fields.insert(format!("drawing-editor:{id}"), source);
        crate::drawing_ui::select_sheet(w, &id);
    } else {
        crate::drawing_ui::select_sheet(w, &id);
        crate::drawing_ui::put(w, sheet);
    }
    Ok(())
}
