use crate::Workspace;
use cad_core::commands::Command;

pub(crate) fn move_feature(w: &mut Workspace, definition: &str, feature: &str, earlier: bool) {
    if w.tool_checkpoint.is_some() || w.sketch.is_some() {
        w.status = "Confirm or cancel the current operation before reordering features".into();
        return;
    }
    let destination = w
        .session
        .visible()
        .parts
        .get(definition)
        .ok_or_else(|| cad_core::CadError::Invalid("The selected part no longer exists".into()))
        .and_then(|part| cad_core::feature_order::destination(part, feature, earlier));
    match destination {
        Ok(position) => {
            w.start_tool();
            w.view.active_part = Some(definition.into());
            w.view.active_node = Some(feature.into());
            w.dispatch(Command::ReorderFeature {
                definition: definition.into(),
                feature: feature.into(),
                position,
            });
        }
        Err(error) => w.status = error.to_string(),
    }
}
