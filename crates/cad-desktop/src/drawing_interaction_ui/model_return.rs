use super::{PaperView, State, Target, Workspace, page, select, view_key};
use cad_core::{Document, Drawing, Id, drawing::model_items::PaperItem};
use std::time::{Duration, Instant};

/// Callouts store paths from the sheet's model. The viewport uses paths from
/// the project root, including for repeated subassemblies.
pub(crate) fn matches_occurrence(
    doc: &Document,
    sheet: &Drawing,
    item: &PaperItem,
    path: &[Id],
) -> bool {
    // Both callers have verified the leaf definition and configuration against
    // evaluated geometry. A sheet of that definition has no relative path.
    if item.path.is_empty() && item.definition == sheet.definition {
        return true;
    }
    let Some(prefix) = path.strip_suffix(item.path.as_slice()) else {
        return false;
    };
    let owner = prefix.iter().try_fold(&doc.root, |definition, id| {
        Some(
            &doc.assemblies
                .get(definition)?
                .instances
                .get(id)?
                .definition,
        )
    });
    owner == Some(&sheet.definition)
}

/// Kept outside the model transaction and transient editor fields. A draft
/// can take several worker revisions to finish before the sheet is reopened.
pub(crate) struct ModelReturn {
    document: Id,
    sheet: Id,
    page: u32,
    target: Target,
    view: PaperView,
}

impl ModelReturn {
    pub(super) fn capture(w: &Workspace, state: &State, sheet: &Drawing, target: Target) -> Self {
        let page = page(w);
        Self {
            document: w.session.visible().id.clone(),
            sheet: sheet.id.clone(),
            page,
            target,
            view: state
                .views
                .get(&view_key(sheet, page))
                .cloned()
                .unwrap_or_default(),
        }
    }
}

pub(crate) fn restore(w: &mut Workspace, changed: bool) {
    let Some(origin) = w.drawing_model_return.take() else {
        return;
    };
    let doc = w.session.visible();
    if doc.id != origin.document {
        return;
    }
    let Some(sheet) = doc.drawings.iter().find(|sheet| sheet.id == origin.sheet) else {
        w.status = "The original drawing sheet is no longer available".into();
        return;
    };
    let key = view_key(sheet, origin.page);
    w.view.drawing_sheet = Some(origin.sheet.clone());
    w.view.drawing_page = origin.page;
    w.view.drawing_selection = Some(origin.target.clone());
    if crate::drawing_ui::open_workspace(w) {
        w.drawing_interaction.views.insert(key, origin.view);
        w.drawing_interaction.returned_callout = Some(ReturnedCallout {
            sheet: origin.sheet,
            page: origin.page,
            target: origin.target,
            changed,
            visible_until: None,
        });
    }
}

pub(crate) fn unchanged_feature(w: &Workspace) -> bool {
    w.drawing_model_return.is_some()
        && w.dialog.as_deref() == Some("feature")
        && w.session.draft.is_none()
        && serde_json::from_str::<cad_core::Feature>(&w.json).is_ok_and(|source| {
            w.fields.get("feature-editor-id") == Some(&source.id)
                && w.view
                    .active_part
                    .as_ref()
                    .and_then(|id| w.session.visible().parts.get(id))
                    .is_some_and(|part| part.features.contains(&source))
        })
}

pub(super) struct ReturnedCallout {
    sheet: Id,
    page: u32,
    target: Target,
    changed: bool,
    visible_until: Option<Instant>,
}

/// Start the emphasis only once the regenerated paper can actually be seen.
/// Keep the scroll position even when a deleted or repaginated callout is gone.
pub(super) fn reveal(w: &mut Workspace, state: &mut State, sheet: &Drawing) -> Option<Duration> {
    let mut returned = state.returned_callout.take()?;
    if returned.sheet != sheet.id {
        return None;
    }
    let now = Instant::now();
    if let Some(deadline) = returned.visible_until {
        let remaining = deadline.checked_duration_since(now)?;
        if super::selected(state) != Some(&returned.target) || returned.page != page(w) {
            return None;
        }
        state.returned_callout = Some(returned);
        return Some(remaining);
    }
    if page(w) != returned.page
        || !state
            .cache
            .as_ref()?
            .scene
            .targets
            .iter()
            .any(|item| item.target == returned.target)
    {
        w.status = "Returned to the drawing. The callout is no longer on this page.".into();
        return None;
    }
    select(w, state, sheet, returned.page, returned.target.clone());
    if returned.changed {
        let duration = Duration::from_millis(1500);
        returned.visible_until = Some(now + duration);
        state.returned_callout = Some(returned);
        Some(duration)
    } else {
        None
    }
}
