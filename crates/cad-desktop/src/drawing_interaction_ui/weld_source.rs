use super::{Workspace, current_native, invalid, model_return::matches_occurrence};
use cad_core::{Drawing, Id, Operation, Result, drawing::model_items::PaperItem};

pub(super) fn open(w: &mut Workspace, sheet: &Drawing, item: &PaperItem) -> Result<()> {
    let doc = w.session.visible();
    if cad_core::library::is_definition_pinned(doc, &item.definition) {
        return Err(invalid(
            "Open the linked component’s source project to edit this weld",
        ));
    }
    let eval = current_native(w).ok_or_else(|| invalid("Wait for the current weld geometry"))?;
    let configuration = |config: Option<&Id>| {
        config
            .or(doc.active_configurations.get(&item.definition))
            .cloned()
    };
    let expected = configuration(item.configuration.as_ref());
    let occurrence_error =
        || invalid("The weld’s drawing occurrence is unavailable in the current configuration");
    if let Some(part) = doc.parts.get(&item.definition) {
        let feature = part
            .features
            .iter()
            .find(|f| f.id == item.source && matches!(&f.operation, Operation::WeldBead { .. }))
            .cloned()
            .ok_or_else(|| invalid("The weld feature is no longer in this part"))?;
        let path = eval
            .instances
            .iter()
            .filter(|i| {
                i.definition == part.id
                    && configuration(i.configuration.as_ref()) == expected
                    && matches_occurrence(doc, sheet, item, &i.path)
            })
            .min_by_key(|i| (Some(&i.path) != w.selected_component.as_ref(), &i.path))
            .map(|i| i.path.clone())
            .ok_or_else(occurrence_error)?;
        w.activate_context(Some(item.definition.clone()), Some(path.clone()));
        w.view.active_node = Some(item.source.clone());
        w.selected_feature_path = Some(path);
        crate::ui::feature_dialog(w, Some(feature));
        if w.fields.get("feature-editor-id") != Some(&item.source) {
            return Err(invalid(&w.status));
        }
    } else {
        let path = eval
            .assembly_welds
            .values()
            .filter(|weld| {
                weld.owner.definition == item.definition
                    && weld.owner.node == item.source
                    && configuration(weld.placement.configuration.as_ref()) == expected
            })
            .filter_map(|weld| weld.placement.path.split_last().map(|(_, owner)| owner))
            .filter(|owner| matches_occurrence(doc, sheet, item, owner))
            .min_by_key(|p| (Some(*p) != w.selected_component.as_deref(), *p))
            .ok_or_else(occurrence_error)?
            .to_vec();
        w.activate_context(None, None);
        w.selected_component = (!path.is_empty()).then_some(path);
        crate::assembly_welds_ui::open(w, &item.definition, &item.source);
        if w.fields.get("assembly-weld-editor") != Some(&item.source) {
            return Err(invalid(&w.status));
        }
    }
    Ok(())
}
