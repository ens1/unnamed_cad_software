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
    for assembly in w.session.visible().assemblies.values() {
        for instance in assembly.instances.keys() {
            if let Some(element) = placement_element(w, &assembly.id, instance) {
                out.push(element);
            }
        }
    }
    out.extend(crate::mate_ui::preview_semantics(w));
    out
}

pub fn placement_element(w: &Workspace, assembly: &str, instance: &str) -> Option<SemanticElement> {
    let doc = w.session.visible();
    let source = doc.assemblies.get(assembly)?.instances.get(instance)?;
    let evaluation = crate::viewport::display_evaluation(w)
        .filter(|e| e.valid && e.identity == w.session.identity());
    let solved = evaluation.and_then(|e| {
        e.assemblies.get(&cad_core::configuration::key(
            assembly,
            doc.active_configurations.get(assembly).map(String::as_str),
        ))
    });
    let placement = solved.and_then(|a| a.placements.get(instance));
    let motion = solved.and_then(|a| a.component_motion.get(instance));
    let value = placement.map(|pose| {
        format!(
            "X {:.3} · Y {:.3} · Z {:.3} mm | Rx {:.2} · Ry {:.2} · Rz {:.2}°{}",
            pose.translation[0],
            pose.translation[1],
            pose.translation[2],
            pose.rotation_deg[0],
            pose.rotation_deg[1],
            pose.rotation_deg[2],
            motion
                .map(|motion| if motion.grounded {
                    " · grounded".into()
                } else {
                    format!(" · {} DOF", motion.degrees_of_freedom)
                })
                .unwrap_or_default()
        )
    });
    Some(SemanticElement {
        id: format!("assembly:{assembly}:instance:{instance}:solved-placement"),
        role: "assembly-solved-placement".into(),
        label: format!("{} · Solved position", source.name),
        value: Some(value.unwrap_or_else(|| "Waiting for current placement validation".into())),
        state: if placement.is_some() {
            "evaluated"
        } else {
            "pending"
        }
        .into(),
        commands: vec![],
        relationships: vec![assembly.into(), instance.into()],
        data: serde_json::json!({
            "assembly": assembly, "instance": instance,
            "configuration": doc.active_configurations.get(assembly),
            "input_scope": "base",
            "input_placement": source.placement,
            "solved_placement": placement,
            "motion": motion,
            "explicitly_fixed": source.fixed,
            "display_only_exploded": w.view.exploded_view.is_some(),
            "evaluation_identity": evaluation.map(|e| &e.identity),
            "editable": false,
        }),
    })
}

pub fn show_placement(ui: &mut egui::Ui, w: &Workspace, assembly: &str, instance: &str) {
    if let Some(element) = placement_element(w, assembly, instance) {
        crate::presentation_controls::label(
            ui,
            &element.id,
            &element.role,
            &[],
            &format!("Solved: {}", element.value.as_deref().unwrap_or_default()),
            &element.state,
        );
        if w.view.exploded_view.is_some() {
            crate::presentation_controls::label(
                ui,
                &format!("{}:display", element.id),
                "assembly-placement-display",
                &[],
                "Exploded display offsets are separate from solved mechanical placement.",
                "display-only",
            );
        }
    }
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
