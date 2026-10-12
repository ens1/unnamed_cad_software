//! Assembly authoring controllers share the existing transaction and presentation adapters.
use crate::Workspace;
use bevy_egui::egui;
use cad_core::presentation::SemanticElement;
pub fn active(w: &Workspace) -> bool {
    w.fields.contains_key("assembly-group-editor") || w.assembly_copy.is_some()
}
pub fn valid(w: &Workspace) -> bool {
    crate::assembly_groups_ui::current_source_is_valid(w)
        && crate::assembly_copy_ui::current_source_is_valid(w)
}
pub fn cancel(w: &mut Workspace) {
    crate::assembly_groups_ui::cancel(w);
    crate::assembly_copy_ui::cancel(w);
}
pub fn action(w: &mut Workspace, command: &str, data: Option<&serde_json::Value>) -> bool {
    crate::assembly_groups_ui::action(w, command, data)
        || crate::assembly_copy_ui::action(w, command, data)
}
pub fn semantics(w: &Workspace) -> Vec<SemanticElement> {
    let mut out = crate::assembly_groups_ui::semantics(w);
    out.extend(crate::assembly_copy_ui::semantics(w));
    out
}
pub fn show(ui: &mut egui::Ui, w: &mut Workspace) {
    if w.fields.contains_key("assembly-group-editor") {
        crate::assembly_groups_ui::show(ui, w);
    } else if w.assembly_copy.is_some() {
        crate::assembly_copy_ui::show(ui, w);
    }
}
pub fn field_is_invalid(w: &Workspace, element: &SemanticElement) -> bool {
    crate::assembly_groups_ui::field_is_invalid(w, element)
        || crate::assembly_copy_ui::field_is_invalid(w, element)
}
pub fn field_error(w: &mut Workspace, element: &SemanticElement, error: Option<String>) {
    crate::assembly_groups_ui::field_error(w, element, error.clone());
    crate::assembly_copy_ui::field_error(w, element, error);
}
