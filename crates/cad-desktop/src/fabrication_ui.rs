//! Docked native sheet/weldment authoring through document commands and shared drafts.
use crate::{Workspace, ui, viewport};
use bevy_egui::egui;
use cad_core::{Document, Expr, Feature, NodeRef, Operation, commands::Command, sheet_metal::*};
const DEFERRED: &str = "tcad:fabrication-preset:";
fn state_occurrence(w: &Workspace, definition: &str) -> Option<cad_core::PlacedPart> {
    let evaluation = viewport::display_evaluation(w)?;
    if !evaluation.geometry_valid || evaluation.identity != w.session.identity() {
        return None;
    }
    let candidates = evaluation
        .instances
        .iter()
        .filter(|instance| instance.definition == definition)
        .collect::<Vec<_>>();
    if let Some(path) = &w.selected_component {
        let chosen = candidates
            .iter()
            .copied()
            .filter(|instance| instance.path.starts_with(path))
            .collect::<Vec<_>>();
        if chosen.len() == 1 {
            return Some(chosen[0].clone());
        }
    }
    if let Some(id) = &w.view.active_instance {
        let chosen = candidates
            .iter()
            .copied()
            .filter(|instance| instance.path.last() == Some(id))
            .collect::<Vec<_>>();
        if chosen.len() == 1 {
            return Some(chosen[0].clone());
        }
    }
    (candidates.len() == 1).then(|| candidates[0].clone())
}
/// Compact fabrication-state projection over current native sheet outputs.
pub fn state_semantics(w: &Workspace) -> Vec<cad_core::presentation::SemanticElement> {
    use cad_core::presentation::SemanticElement;
    let doc = w.session.visible();
    let Some(owner) = w.view.active_part.as_ref() else {
        return vec![];
    };
    if !doc.parts.get(owner).is_some_and(|part| {
        part.features
            .iter()
            .any(|feature| matches!(feature.operation, Operation::Sheet { .. }))
    }) {
        return vec![];
    }
    let Some(instance) = state_occurrence(w, owner) else {
        return vec![];
    };
    let Ok(part) =
        cad_core::configuration::effective_part(doc, owner, instance.configuration.as_deref())
    else {
        return vec![];
    };
    let Some(base) = doc.parts.get(owner) else {
        return vec![];
    };
    let Some(evaluation) = viewport::display_evaluation(w) else {
        return vec![];
    };
    let idle = w.tool_checkpoint.is_none()
        && w.sketch.is_none()
        && w.settings.is_none()
        && w.session.draft.is_none()
        && w.view.history_cursor.is_none()
        && !w.view.original;
    let mut out = Vec::new();
    for feature in part.features.iter().filter(|feature| {
        part.outputs.contains(&feature.id)
            && !feature.suppressed
            && matches!(feature.operation, Operation::Sheet { .. })
    }) {
        let source = part.reference(&feature.id);
        let Some(sheet) = evaluation.sheets.get(&format!(
            "{}::{}",
            cad_core::history::mesh_key(&instance),
            feature.id
        )) else {
            continue;
        };
        let id = format!("sheet-state:{}/{}", part.id, feature.id);
        out.push(SemanticElement { id:id.clone(), role:"sheet-state-controls".into(), label:format!("{} · fabrication", feature.name), value:Some(format!("{:?}", sheet.state)), state:"ready".into(), commands:vec![], relationships:vec![feature.id.clone(), "workspace.viewport".into()], data:serde_json::json!({"contract":"tcad.sheet-state-controls","version":1,"source":source,"state":sheet.state,"configuration":instance.configuration,"occurrence":instance.path,"export_state":"flat","requires_current_validation":true}) });
        for (command, label, selected) in [
            (
                "fabrication.state-formed",
                "Formed",
                sheet.state == State::Formed,
            ),
            ("fabrication.state-flat", "Flat", sheet.state == State::Flat),
            ("fabrication.export-flat", "Export flat DXF…", false),
        ] {
            let editable = base.outputs.contains(&feature.id)
                && !cad_core::library::is_definition_pinned(doc, owner);
            let enabled = idle && (command == "fabrication.export-flat" || editable);
            out.push(SemanticElement {id:format!("{id}/{command}"),role:"sheet-state-action".into(),label:label.into(),value:(!editable && command!="fabrication.export-flat").then(||"Make the linked definition unique, or restore its base output in Configurations, before changing fabrication state".into()),state:if !enabled {"disabled"} else if selected {"selected"} else {"ready"}.into(),commands:vec![command.into()],relationships:vec![id.clone()],data:serde_json::json!({"document":doc.id,"source":source,"occurrence":instance.path,"configuration":instance.configuration,"export_state":"flat","enabled":enabled,"classes":["compact","viewport-control"]})});
        }
    }
    out
}
pub fn state_action(w: &mut Workspace, command: &str, data: Option<&serde_json::Value>) -> bool {
    if !matches!(
        command,
        "fabrication.state-formed" | "fabrication.state-flat" | "fabrication.export-flat"
    ) {
        return false;
    }
    let Some(data) = data.filter(|data| {
        data.get("document").and_then(serde_json::Value::as_str)
            == Some(w.session.visible().id.as_str())
    }) else {
        return true;
    };
    if w.tool_checkpoint.is_some()
        || w.sketch.is_some()
        || w.settings.is_some()
        || w.session.draft.is_some()
        || w.view.history_cursor.is_some()
        || w.view.original
    {
        w.status = "Finish the current operation before changing fabrication state".into();
        return true;
    }
    let Some(source) = data
        .get("source")
        .and_then(|source| serde_json::from_value::<NodeRef>(source.clone()).ok())
    else {
        return true;
    };
    let Some(mut part) = w.session.visible().parts.get(&source.definition).cloned() else {
        return true;
    };
    if w.view.active_part.as_ref() != Some(&source.definition) {
        return true;
    }
    let Some(instance) = state_occurrence(w, &source.definition) else {
        w.status = "Select one current placed sheet occurrence".into();
        return true;
    };
    if data.get("occurrence") != Some(&serde_json::json!(instance.path)) {
        w.status = "The sheet occurrence changed; use its current fabrication controls".into();
        return true;
    }
    // Layouts can retain a descriptor across configuration/source changes.
    // Reconstruct availability from the current native output before invoking
    // either a shared-definition edit or a configured-sheet export.
    let current = state_semantics(w).into_iter().find(|control| {
        control.role == "sheet-state-action"
            && control
                .commands
                .iter()
                .any(|candidate| candidate == command)
            && ["document", "source", "occurrence", "configuration"]
                .iter()
                .all(|key| control.data.get(key) == data.get(key))
    });
    if !current.is_some_and(|control| {
        control.state != "disabled"
            && control
                .data
                .get("enabled")
                .and_then(serde_json::Value::as_bool)
                == Some(true)
    }) {
        w.status =
            "The sheet output or configuration changed; use its current fabrication controls"
                .into();
        return true;
    }
    if command == "fabrication.export-flat" {
        let selection = cad_core::sheet_metal::ExportSelection {
            source,
            occurrence: Some(instance.path),
        };
        if let Ok(payload) = serde_json::to_string(&selection) {
            ui::export_format(w, &format!("sheet-metal-dxf:{payload}"), "flat-pattern.dxf");
        }
        return true;
    }
    if cad_core::library::is_definition_pinned(w.session.visible(), &source.definition) {
        w.status =
            "Make the linked sheet definition unique before editing fabrication state".into();
        return true;
    }
    let Some(index) = part.features.iter().position(|f| {
        f.id == source.node
            && !f.suppressed
            && part.outputs.contains(&f.id)
            && matches!(f.operation, Operation::Sheet { .. })
    }) else {
        return true;
    };
    let state = if command == "fabrication.state-flat" {
        State::Flat
    } else {
        State::Formed
    };
    if cad_core::sheet_metal::stock(w.session.visible(), &source)
        .is_ok_and(|stock| stock.state == state)
    {
        return true;
    }
    let feature = if let Operation::Sheet { operation } = &mut part.features[index].operation
        && let SheetOperation::Flatten { state: current, .. } = operation.as_mut()
    {
        *current = state;
        part.features[index].clone()
    } else {
        let feature = Feature {
            id: cad_core::id(),
            name: "Fabrication State".into(),
            operation: Operation::Sheet {
                operation: Box::new(SheetOperation::Flatten {
                    body: source.clone(),
                    state,
                }),
            },
            suppressed: false,
        };
        for output in &mut part.outputs {
            if output == &source.node {
                *output = feature.id.clone();
            }
        }
        part.features.push(feature.clone());
        feature
    };
    w.start_tool();
    w.dispatch(Command::PutPart {
        part,
        assembly: None,
        placement: Default::default(),
    });
    ui::feature_dialog(w, Some(feature));
    true
}
pub fn state_controls(ui: &mut egui::Ui, w: &mut Workspace) {
    let controls = state_semantics(w);
    for group in controls
        .iter()
        .filter(|control| control.role == "sheet-state-controls")
    {
        ui.horizontal_wrapped(|ui| {
            crate::presentation_controls::label(ui,&group.id,&group.role,&[],&group.label,&group.state);
            for control in controls.iter().filter(|control|control.role=="sheet-state-action" && control.relationships.contains(&group.id)) {
                if crate::presentation_controls::button(ui,&control.id,&control.role,&["compact","viewport-control"],&control.label,control.state!="disabled",control.state=="selected").on_hover_text(if control.commands[0]=="fabrication.export-flat" {"Export this sheet's evaluated flat fabrication geometry; the viewport state stays unchanged"} else {"Preview the fabrication state, then confirm or cancel the shared operation"}).clicked() {
                    state_action(w,&control.commands[0],Some(&control.data));
                }
            }
        });
    }
}
pub fn is_preset(name: &str) -> bool {
    name.starts_with(DEFERRED) || native_preset(name)
}
fn native_preset(name: &str) -> bool {
    matches!(
        name,
        "Base Flange / Tab"
            | "Sheet Tab"
            | "Edge Flange"
            | "Open Hem"
            | "Jog / Offset Flange"
            | "Sheet Normal Cut / Relief"
            | "Flat Pattern"
            | "Formed Sheet"
            | "Structural Member Group"
            | "Custom Section Along Path"
            | "Trim / Extend Member"
            | "Weldment End Cap"
            | "Sketch Gusset"
    )
}
fn queue_after_confirm(w: &mut Workspace, name: &str) -> bool {
    let Some(definition) = w
        .sketch
        .as_ref()
        .map(|sketch| sketch.definition.as_str())
        .or(w.view.active_part.as_deref())
    else {
        w.status = "Create or select the intended fabrication part first".into();
        return false;
    };
    match serde_json::to_string(&(name, &w.session.visible().id, definition)) {
        Ok(payload) => {
            w.after_confirm = Some(format!("{DEFERRED}{payload}"));
            true
        }
        Err(error) => {
            w.status = error.to_string();
            false
        }
    }
}
fn sheet_choices(doc: &Document, current: &str) -> Vec<(NodeRef, String)> {
    doc.parts
        .values()
        .flat_map(|part| {
            part.features
                .iter()
                .take_while(|f| f.id != current)
                .filter(|f| !f.suppressed && matches!(f.operation, Operation::Sheet { .. }))
                .map(move |f| {
                    let source = part.reference(&f.id);
                    let name = cad_core::manufacturing::section_label(doc, &source)
                        .map(|label| label.display())
                        .unwrap_or_else(|| format!("{} / {}", part.name, f.name));
                    (source, name)
                })
        })
        .collect()
}
fn selected_profile(w: &Workspace, candidates: &[NodeRef]) -> Option<NodeRef> {
    w.selected
        .as_ref()
        .and_then(|selected| {
            selected
                .body
                .as_ref()
                .into_iter()
                .chain(std::iter::once(&selected.reference.producer))
                .find(|source| candidates.contains(source))
                .cloned()
        })
        .or_else(|| {
            candidates
                .iter()
                .find(|source| {
                    w.view.active_part.as_ref() == Some(&source.definition)
                        && w.view.active_node.as_ref() == Some(&source.node)
                })
                .cloned()
        })
}
fn preferred_profile(
    w: &Workspace,
    candidates: &[NodeRef],
    suitable: impl Fn(&NodeRef) -> bool,
) -> Option<NodeRef> {
    selected_profile(w, candidates)
        .filter(&suitable)
        .or_else(|| {
            candidates
                .iter()
                .rev()
                .find(|source| suitable(source))
                .cloned()
        })
}
fn compatible_profiles(
    w: &Workspace,
    current: &str,
    selected: &NodeRef,
    suitable: impl Fn(&NodeRef) -> bool,
) -> Vec<(NodeRef, String)> {
    profiles(w.session.visible(), current)
        .into_iter()
        .filter_map(|(source, name)| {
            if suitable(&source) {
                Some((source, name))
            } else if &source == selected {
                Some((source, format!("{name} · incompatible sketch")))
            } else {
                None
            }
        })
        .collect()
}
fn profiles(doc: &Document, current: &str) -> Vec<(NodeRef, String)> {
    doc.parts
        .values()
        .flat_map(|part| {
            part.features
                .iter()
                .take_while(|f| f.id != current)
                .filter(|f| !f.suppressed && matches!(f.operation, Operation::Profile { .. }))
                .map(move |f| {
                    let source = part.reference(&f.id);
                    let name = cad_core::manufacturing::section_label(doc, &source)
                        .map(|label| label.display())
                        .unwrap_or_else(|| format!("{} / {}", part.name, f.name));
                    (source, name)
                })
        })
        .collect()
}
pub(crate) fn section_choices(
    w: &Workspace,
    current: &str,
    selected: &NodeRef,
) -> Vec<(NodeRef, String)> {
    compatible_profiles(w, current, selected, |source| closed_profile(w, source))
}
pub(crate) fn structural_path_choices(
    w: &Workspace,
    current: &str,
    selected: &NodeRef,
    section: &NodeRef,
) -> Vec<(NodeRef, String)> {
    compatible_profiles(w, current, selected, |source| {
        source != section && !structural_path_members(w, source).is_empty()
    })
}
fn selector(
    ui: &mut egui::Ui,
    label: &str,
    reference: &mut NodeRef,
    choices: Vec<(NodeRef, String)>,
) -> bool {
    let mut changed = false;
    egui::ComboBox::from_label(label)
        .selected_text(
            choices
                .iter()
                .find(|(r, _)| r == reference)
                .map(|(_, name)| name.as_str())
                .unwrap_or("Select source"),
        )
        .show_ui(ui, |ui| {
            for (source, name) in choices {
                changed |= ui.selectable_value(reference, source, name).changed();
            }
        });
    changed
}
fn expression(ui: &mut egui::Ui, label: &str, value: &mut Expr) -> bool {
    crate::presentation_controls::expression(
        ui,
        &format!("fabrication_ui/{}", label.to_lowercase().replace(' ', "-")),
        label,
        value,
    )
}
pub(crate) fn members(w: &Workspace, stock: &Stock) -> Vec<(String, String)> {
    let source = source_sketch(w, &stock.profile);
    crate::fabrication_sources_ui::base_member_labels(w, stock)
        .into_iter()
        .filter(|(id, _)| {
            source
                .as_ref()
                .and_then(|sketch| sketch.members.get(id))
                .is_some_and(|member| !member.construction)
        })
        .collect()
}
pub(crate) fn sheet_base_members(w: &Workspace, stock: &Stock) -> Vec<(String, String)> {
    members(w, stock)
        .into_iter()
        .filter(|(member, _)| {
            !stock
                .flanges
                .iter()
                .any(|flange| flange.member == *member && flange.attachment.is_none())
        })
        .collect()
}
pub(crate) fn sheet_attachment_choices(
    w: &Workspace,
    stock: &Stock,
) -> Vec<(Option<NodeRef>, String)> {
    let mut choices = vec![(None, "Base sketch edge".into())];
    choices.extend(
        stock
            .flanges
            .iter()
            .filter(|flange| {
                !stock
                    .flanges
                    .iter()
                    .any(|child| child.attachment.as_ref() == Some(&flange.source))
            })
            .map(|flange| {
                let name = crate::fabrication_sources_ui::source_name(w, &flange.source)
                    .unwrap_or_else(|| "Missing bend".into());
                (
                    Some(flange.source.clone()),
                    format!("{name} · full-width terminal edge"),
                )
            }),
    );
    choices
}
pub(crate) fn sheet_target(w: &Workspace, stock: &Stock) -> Option<(String, Option<NodeRef>)> {
    if let Some(target) = crate::fabrication_sources_ui::picked_attachment(w, stock) {
        return Some(target);
    }
    let terminal = |source: &NodeRef| {
        stock.flanges.iter().find(|flange| {
            &flange.source == source
                && !stock
                    .flanges
                    .iter()
                    .any(|child| child.attachment.as_ref() == Some(source))
        })
    };
    let explicit = match w.context_selection.as_ref() {
        Some(cad_core::selection_actions::SelectionTarget::Feature {
            definition,
            feature,
        }) => Some(NodeRef {
            definition: definition.clone(),
            node: feature.clone(),
        }),
        _ => None,
    }
    .or_else(|| {
        w.selected_feature_path.as_ref().and_then(|_| {
            Some(NodeRef {
                definition: w.view.active_part.clone()?,
                node: w.view.active_node.clone()?,
            })
        })
    });
    if let Some(flange) = explicit.as_ref().and_then(terminal) {
        return Some((flange.member.clone(), Some(flange.source.clone())));
    }
    sheet_base_members(w, stock)
        .into_iter()
        .next()
        .map(|(member, _)| (member, None))
        .or_else(|| {
            stock
                .flanges
                .iter()
                .rev()
                .find_map(|flange| terminal(&flange.source))
                .map(|flange| (flange.member.clone(), Some(flange.source.clone())))
        })
}
pub fn preset(w: &mut Workspace, name: &str) {
    let decoded = name
        .strip_prefix(DEFERRED)
        .map(serde_json::from_str::<(String, String, String)>)
        .transpose();
    let name = match decoded {
        Ok(Some((name, document, definition))) => {
            if document != w.session.visible().id
                || w.view.active_part.as_deref() != Some(definition.as_str())
            {
                w.status = "The pending fabrication action belongs to another document or part; invoke it again in the intended part".into();
                return;
            }
            name
        }
        Ok(None) => name.to_string(),
        Err(error) => {
            w.status = format!("Deferred fabrication action is invalid: {error}");
            return;
        }
    };
    let name = name.as_str();
    if !native_preset(name) {
        w.status = "The pending fabrication preset is unavailable".into();
        return;
    }
    if matches!(
        name,
        "Structural Member Group"
            | "Custom Section Along Path"
            | "Trim / Extend Member"
            | "Weldment End Cap"
            | "Sketch Gusset"
    ) {
        weldment_preset(w, name);
        return;
    }
    crate::history::latest(w);
    if w.confirm_requested {
        queue_after_confirm(w, name);
        return;
    }
    if w.sketch.is_some() {
        if !queue_after_confirm(w, name) {
            return;
        }
        w.confirm_tool();
        if w.sketch.is_some() && !w.confirm_requested {
            w.after_confirm = None;
        }
        return;
    }
    if w.tool_checkpoint.is_some() {
        w.status = "Confirm or cancel the current feature first".into();
        return;
    }
    let Some(mut part) = w
        .view
        .active_part
        .as_ref()
        .and_then(|id| w.session.visible().parts.get(id))
        .cloned()
    else {
        w.status = "Create or select a part first".into();
        return;
    };
    let candidates = part
        .features
        .iter()
        .filter(|f| !f.suppressed && matches!(f.operation, Operation::Profile { .. }))
        .map(|f| part.reference(&f.id))
        .collect::<Vec<_>>();
    let profile = preferred_profile(w, &candidates, |source| closed_profile(w, source));
    let body = w
        .selected
        .as_ref()
        .and_then(|s| s.body.as_ref())
        .filter(|r| {
            r.definition == part.id
                && part.features.iter().any(|f| {
                    f.id == r.node
                        && !f.suppressed
                        && matches!(f.operation, Operation::Sheet { .. })
                })
        })
        .cloned()
        .or_else(|| {
            part.features
                .iter()
                .find(|f| {
                    w.view.active_node.as_ref() == Some(&f.id)
                        && !f.suppressed
                        && matches!(f.operation, Operation::Sheet { .. })
                })
                .or_else(|| {
                    part.features.iter().rev().find(|f| {
                        !f.suppressed
                            && part.outputs.contains(&f.id)
                            && matches!(f.operation, Operation::Sheet { .. })
                    })
                })
                .map(|f| part.reference(&f.id))
        });
    let operation = match name {
        "Base Flange / Tab" => {
            let Some(profile) = profile else {
                crate::sketch::begin(w, None);
                w.status = "Draw a closed base sketch, finish it, then create sheet stock".into();
                return;
            };
            SheetOperation::Base {
                profile,
                thickness: "1 mm".into(),
                inside_radius: "1 mm".into(),
                k_factor: "0.4".into(),
                side: ThicknessSide::Positive,
            }
        }
        "Sheet Tab" | "Sheet Normal Cut / Relief" => {
            let (Some(body), Some(profile)) = (body, profile) else {
                w.status =
                    "Create sheet stock and a closed sketch on the original base blank plane first"
                        .into();
                return;
            };
            if name == "Sheet Tab" {
                SheetOperation::Tab { body, profile }
            } else {
                SheetOperation::NormalCut { body, profile }
            }
        }
        "Edge Flange" | "Open Hem" | "Jog / Offset Flange" => {
            let Some(body) = body else {
                w.status = "Select native sheet stock first".into();
                return;
            };
            let stock = match cad_core::sheet_metal::stock(w.session.visible(), &body) {
                Ok(stock) => stock,
                Err(error) => {
                    w.status = error.to_string();
                    return;
                }
            };
            if w.selected.as_ref().is_some_and(|pick| {
                pick.reference.topology == "edge" && pick.body.as_ref() == Some(&body)
            }) && crate::fabrication_sources_ui::picked_attachment(w, &stock).is_none()
            {
                w.status = "The selected edge is not an unused base edge or a free full-width terminal. Select a suitable sheet edge, or clear the selection to choose a named source.".into();
                return;
            }
            let Some((member, attachment)) = sheet_target(w, &stock) else {
                w.status =
                    "Select an unused supporting base edge or an available terminal flange edge"
                        .into();
                return;
            };
            if name == "Open Hem" {
                SheetOperation::Hem {
                    body,
                    member,
                    length: "10 mm".into(),
                    gap: Expr(format!("{} mm", (stock.thickness_mm * 0.5).max(0.1))),
                    reverse: false,
                    attachment,
                    k_factor: None,
                }
            } else if name == "Jog / Offset Flange" {
                let angle = std::f64::consts::FRAC_PI_4;
                let offset = 2. * (stock.radius_mm + stock.thickness_mm * 0.5) * (1. - angle.cos())
                    + 10. * angle.sin();
                SheetOperation::Jog {
                    body,
                    member,
                    offset: Expr(format!("{offset} mm")),
                    length: "20 mm".into(),
                    angle: "45 deg".into(),
                    inside_radius: None,
                    k_factor: None,
                    attachment,
                }
            } else {
                SheetOperation::EdgeFlange {
                    body,
                    member,
                    length: "20 mm".into(),
                    angle: "90 deg".into(),
                    inside_radius: None,
                    k_factor: None,
                    attachment,
                }
            }
        }
        "Flat Pattern" | "Formed Sheet" => {
            let Some(body) = body else {
                w.status = "Select native sheet stock first".into();
                return;
            };
            SheetOperation::Flatten {
                body,
                state: if name == "Flat Pattern" {
                    State::Flat
                } else {
                    State::Formed
                },
            }
        }
        _ => return,
    };
    let feature = Feature {
        id: cad_core::id(),
        name: format!(
            "{}{}",
            name,
            part.features
                .iter()
                .filter(|f| f.name.starts_with(name))
                .count()
                + 1
        ),
        operation: Operation::Sheet {
            operation: Box::new(operation),
        },
        suppressed: false,
    };
    for source in feature
        .operation
        .consumed_outputs()
        .iter()
        .filter(|r| r.definition == part.id)
    {
        part.outputs.retain(|id| id != &source.node);
    }
    part.outputs.push(feature.id.clone());
    part.features.push(feature.clone());
    w.start_tool();
    w.dispatch(Command::PutPart {
        part,
        assembly: None,
        placement: Default::default(),
    });
    w.view.active_node = Some(feature.id.clone());
    ui::feature_dialog(w, Some(feature));
}
fn sheet_bend_source(
    ui: &mut egui::Ui,
    w: &Workspace,
    current: &str,
    body: &mut NodeRef,
    member: &mut String,
    attachment: &mut Option<NodeRef>,
) -> bool {
    let mut changed = selector(
        ui,
        "Sheet Stock",
        body,
        sheet_choices(w.session.visible(), current),
    );
    let Ok(stock) = cad_core::sheet_metal::stock(w.session.visible(), body) else {
        return changed;
    };
    if changed && let Some((next_member, next_attachment)) = sheet_target(w, &stock) {
        *member = next_member;
        *attachment = next_attachment;
    }
    let choices = sheet_attachment_choices(w, &stock);
    let before = attachment.clone();
    egui::ComboBox::from_label("Bend Attachment")
        .selected_text(
            choices
                .iter()
                .find(|(source, _)| source == attachment)
                .map(|(_, label)| label.as_str())
                .unwrap_or("Saved attachment"),
        )
        .show_ui(ui, |ui| {
            for (source, label) in choices {
                changed |= ui.selectable_value(attachment, source, label).changed();
            }
        });
    if before != *attachment {
        if let Some(parent) = attachment
            .as_ref()
            .and_then(|source| stock.flanges.iter().find(|flange| &flange.source == source))
        {
            *member = parent.member.clone();
        } else if let Some((next, _)) = sheet_base_members(w, &stock).into_iter().next() {
            *member = next;
        }
    }
    if attachment.is_none() {
        let choices = sheet_base_members(w, &stock);
        egui::ComboBox::from_label("Base Sketch Edge")
            .selected_text(
                choices
                    .iter()
                    .find(|(id, _)| id == member)
                    .map(|(_, label)| label.as_str())
                    .unwrap_or("Select unused straight outer edge"),
            )
            .show_ui(ui, |ui| {
                for (id, label) in choices {
                    changed |= ui.selectable_value(member, id, label).changed();
                }
            });
    } else {
        ui.small("Full-width terminal edge; source base member follows the attachment.");
    }
    changed
}
fn sheet_override(ui: &mut egui::Ui, label: &str, value: &mut Option<Expr>, default: &str) -> bool {
    let mut custom = value.is_some();
    let mut changed = ui
        .checkbox(&mut custom, format!("Override {label}"))
        .changed();
    if changed {
        *value = custom.then(|| default.into());
    }
    if let Some(value) = value {
        changed |= expression(ui, label, value);
    }
    changed
}
pub fn sheet_fields(
    ui: &mut egui::Ui,
    w: &Workspace,
    current: &str,
    operation: &mut SheetOperation,
) -> bool {
    let normal_cut = matches!(operation, SheetOperation::NormalCut { .. });
    let doc = w.session.visible();
    let mut changed = false;
    match operation {
        SheetOperation::Base {
            profile,
            thickness,
            inside_radius,
            k_factor,
            side,
        } => {
            changed |= selector(ui, "Base Sketch", profile, profiles(doc, current));
            changed |= expression(ui, "Thickness", thickness);
            changed |= expression(ui, "Default Inside Radius", inside_radius);
            changed |= expression(ui, "K Factor", k_factor);
            egui::ComboBox::from_label("Thickness Direction")
                .selected_text(format!("{side:?}"))
                .show_ui(ui, |ui| {
                    for (value, label) in [
                        (ThicknessSide::Positive, "Along sketch normal"),
                        (ThicknessSide::Negative, "Reverse"),
                        (ThicknessSide::MidPlane, "Mid-plane"),
                    ] {
                        changed |= ui.selectable_value(side, value, label).changed();
                    }
                });
            ui.small("Closed sketch regions, inner holes and separate blanks; exact constant-thickness stock.");
        }
        SheetOperation::Tab { body, profile } | SheetOperation::NormalCut { body, profile } => {
            changed |= selector(ui, "Sheet Stock", body, sheet_choices(doc, current));
            changed |= selector(
                ui,
                if normal_cut {
                    "Parallel Cut Sketch / Stock Face"
                } else {
                    "Base-Plane Closed Sketch"
                },
                profile,
                profiles(doc, current),
            );
            ui.small("Blank tabs, holes, notches and relief sketches propagate to formed and flat stock. Keep bend attachment sidewalls intact.");
        }
        SheetOperation::EdgeFlange {
            body,
            member,
            length,
            angle,
            inside_radius,
            k_factor,
            attachment,
        } => {
            changed |= sheet_bend_source(ui, w, current, body, member, attachment);
            changed |= expression(ui, "Straight Tangent Length", length);
            changed |= expression(ui, "Signed Bend Angle", angle);
            changed |= sheet_override(ui, "Inside Radius", inside_radius, "1 mm");
            changed |= sheet_override(ui, "K Factor", k_factor, "0.4");
            ui.small("Exact full-width bends through ±180°; length begins after the circular bend tangent. Chain onto an earlier terminal edge.");
        }
        SheetOperation::Hem {
            body,
            member,
            length,
            gap,
            reverse,
            attachment,
            k_factor,
        } => {
            changed |= sheet_bend_source(ui, w, current, body, member, attachment);
            changed |= expression(ui, "Return Tangent Length", length);
            changed |= expression(ui, "Clear Hem Gap", gap);
            changed |= ui.checkbox(reverse, "Reverse Bend Side").changed();
            changed |= sheet_override(ui, "K Factor", k_factor, "0.4");
            ui.small("Open 180° hem, with inside radius equal to half the clear gap. Closed crushed hems need a forming model.");
        }
        SheetOperation::Jog {
            body,
            member,
            offset,
            length,
            angle,
            inside_radius,
            k_factor,
            attachment,
        } => {
            changed |= sheet_bend_source(ui, w, current, body, member, attachment);
            changed |= expression(ui, "Signed Mid-Surface Offset", offset);
            changed |= expression(ui, "Terminal Tangent Length", length);
            changed |= expression(ui, "Bend Magnitude", angle);
            changed |= sheet_override(ui, "Inside Radius", inside_radius, "1 mm");
            changed |= sheet_override(ui, "K Factor", k_factor, "0.4");
            ui.small("Two opposite bends, each through 90°; signed offset controls bend side. Straight web length derives from the requested offset and stock radius.");
        }
        SheetOperation::Flatten { body, state } => {
            changed |= selector(ui, "Sheet Stock", body, sheet_choices(doc, current));
            egui::ComboBox::from_label("Output State")
                .selected_text(format!("{state:?}"))
                .show_ui(ui, |ui| {
                    changed |= ui
                        .selectable_value(state, State::Flat, "Flat Pattern")
                        .changed();
                    changed |= ui
                        .selectable_value(state, State::Formed, "Formed Sheet")
                        .changed();
                });
            ui.small("Exact formed and developed stock share thickness, signed bends, radius and K-factor definitions. Jog features publish two bend lines.");
        }
    }
    changed
}

fn weldment_choices(doc: &Document, current: &str) -> Vec<(NodeRef, String)> {
    doc.parts
        .values()
        .flat_map(|part| {
            part.features
                .iter()
                .take_while(|feature| feature.id != current)
                .filter(|feature| {
                    !feature.suppressed && matches!(feature.operation, Operation::Weldment { .. })
                })
                .map(move |feature| {
                    (
                        part.reference(&feature.id),
                        format!("{} / {}", part.name, feature.name),
                    )
                })
        })
        .collect()
}

pub(crate) fn source_sketch(w: &Workspace, profile: &NodeRef) -> Option<cad_core::Sketch> {
    let mut sketch = viewport::display_evaluation(w)
        .and_then(|evaluation| {
            evaluation
                .sketches
                .get(&format!("{}::{}", profile.definition, profile.node))
        })
        .and_then(|output| output.solved.clone())
        .or_else(|| {
            let part = w.session.visible().parts.get(&profile.definition)?;
            let feature = part
                .features
                .iter()
                .find(|feature| feature.id == profile.node)?;
            let Operation::Profile {
                profile: definition,
                ..
            } = &feature.operation
            else {
                return None;
            };
            if let cad_core::Profile::Sketch { sketch } = definition {
                return part.sketches.get(sketch).cloned();
            }
            let mut sketch = cad_core::Sketch {
                id: profile.node.clone(),
                name: feature.name.clone(),
                points: Default::default(),
                segments: vec![],
                constraints: vec![],
                profile_order: vec![],
                circles: Default::default(),
                members: Default::default(),
                conversions: Default::default(),
                offsets: Default::default(),
                fillets: Default::default(),
                chamfers: Default::default(),
                transforms: Default::default(),
                splines: Default::default(),
                conics: Default::default(),
                texts: Default::default(),
                equations: Default::default(),
                blocks: Default::default(),
                block_instances: Default::default(),
                dimension_groups: Default::default(),
            };
            match definition {
                cad_core::Profile::Polygon { points } => {
                    for (index, point) in points.iter().enumerate() {
                        let id = format!("p{index}");
                        sketch.points.insert(id.clone(), *point);
                        sketch.profile_order.push(id);
                    }
                }
                cad_core::Profile::Circle { radius, center } => {
                    sketch.points.insert("center".into(), *center);
                    sketch.members.insert(
                        "circle".into(),
                        cad_core::SketchMember {
                            construction: false,
                            curve: cad_core::SketchCurve::Circle {
                                center: "center".into(),
                                radius: radius.clone(),
                            },
                        },
                    );
                }
                cad_core::Profile::Sketch { .. } => unreachable!(),
            }
            Some(sketch)
        })?;
    if sketch.members.is_empty() && !sketch.profile_order.is_empty() {
        for (index, (start, end)) in sketch
            .profile_order
            .iter()
            .zip(sketch.profile_order.iter().cycle().skip(1))
            .take(sketch.profile_order.len())
            .enumerate()
        {
            sketch.members.insert(
                format!("legacy-profile-edge-{index}"),
                cad_core::SketchMember {
                    construction: false,
                    curve: cad_core::SketchCurve::Line {
                        start: start.clone(),
                        end: end.clone(),
                    },
                },
            );
        }
    } else if sketch.members.is_empty() {
        for (index, [start, end]) in sketch.segments.iter().enumerate() {
            sketch.members.insert(
                format!("legacy-segment-{index}"),
                cad_core::SketchMember {
                    construction: false,
                    curve: cad_core::SketchCurve::Line {
                        start: start.clone(),
                        end: end.clone(),
                    },
                },
            );
        }
    }
    Some(sketch)
}

fn closed_profile(w: &Workspace, profile: &NodeRef) -> bool {
    let Some(sketch) = source_sketch(w, profile) else {
        return viewport::display_evaluation(w)
            .and_then(|evaluation| {
                evaluation
                    .sketches
                    .get(&format!("{}::{}", profile.definition, profile.node))
            })
            .is_some_and(|output| !output.profile_order.is_empty() || !output.circles.is_empty());
    };
    cad_core::sketch_topology::analyze(&sketch, cad_core::sketch_topology::DEFAULT_TOLERANCE_MM)
        .is_ok_and(|topology| {
            (!topology.contours.is_empty() || !sketch.circles.is_empty())
                && topology.contours.iter().all(|contour| {
                    contour.status == cad_core::sketch_topology::ContourStatus::Closed
                })
        })
}

/// Structural stock uses exact line/circular paths; sheet-edge selection stays linear.
pub(crate) fn structural_path_members(w: &Workspace, profile: &NodeRef) -> Vec<(String, String)> {
    let settings = &w.session.visible().settings;
    if let Some(Operation::Sketch3d { definition }) = w
        .session
        .visible()
        .parts
        .get(&profile.definition)
        .and_then(|part| {
            part.features
                .iter()
                .find(|feature| feature.id == profile.node && !feature.suppressed)
        })
        .map(|feature| &feature.operation)
    {
        let identity = w.session.identity();
        let key = format!(
            "{}::{}",
            cad_core::configuration::key(
                &profile.definition,
                w.session
                    .visible()
                    .active_configurations
                    .get(&profile.definition)
                    .map(String::as_str)
            ),
            profile.node
        );
        let solved = w
            .session
            .draft
            .as_ref()
            .and_then(|draft| draft.evaluation.as_ref())
            .or(w.session.evaluation.as_ref())
            .filter(|evaluation| evaluation.identity == identity)
            .and_then(|evaluation| evaluation.spatial_sketches.get(&key))
            .map(|evaluated| &evaluated.solved)
            .unwrap_or(definition);
        return solved
            .members
            .iter()
            .enumerate()
            .filter_map(|(index, (id, member))| {
                let cad_core::sketch3d::Curve::Line { start, end } = &member.curve else {
                    return None;
                };
                let a = solved.points.get(start)?;
                let b = solved.points.get(end)?;
                let length = (0..3)
                    .map(|axis| (b[axis] - a[axis]).powi(2))
                    .sum::<f64>()
                    .sqrt();
                Some((
                    id.clone(),
                    format!("3D path {} · {}", index + 1, settings.format_length(length)),
                ))
            })
            .collect();
    }
    let evaluated = viewport::display_evaluation(w).and_then(|evaluation| {
        evaluation
            .sketches
            .get(&format!("{}::{}", profile.definition, profile.node))
    });
    source_sketch(w, profile)
        .map(|sketch| {
            sketch
                .members
                .keys()
                .enumerate()
                .filter_map(|(ordinal, id)| {
                    let label = match cad_core::sketch_edit::effective_curve(&sketch, id).ok()? {
                        cad_core::SketchCurve::Line { start, end } => {
                            let a = sketch.points.get(&start)?;
                            let b = sketch.points.get(&end)?;
                            let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
                            let length = dx.hypot(dy);
                            let location = if dy.abs() <= length * 1e-9 {
                                format!(
                                    "horizontal · Y {}",
                                    settings.format_length((a[1] + b[1]) * 0.5)
                                )
                            } else if dx.abs() <= length * 1e-9 {
                                format!(
                                    "vertical · X {}",
                                    settings.format_length((a[0] + b[0]) * 0.5)
                                )
                            } else {
                                "diagonal".into()
                            };
                            format!(
                                "Path {} · {} · {location}",
                                ordinal + 1,
                                settings.format_length(length)
                            )
                        }
                        cad_core::SketchCurve::Arc {
                            start,
                            through,
                            end,
                        } => {
                            let (_, radius, _, angle) = cad_core::sketch_geometry::arc_frame(
                                *sketch.points.get(&start)?,
                                *sketch.points.get(&through)?,
                                *sketch.points.get(&end)?,
                            )
                            .ok()?;
                            format!(
                                "Path {} · arc {} · R {} · {}",
                                ordinal + 1,
                                settings.format_length(radius * angle.abs()),
                                settings.format_length(radius),
                                settings.format_angle_radians(angle.abs())
                            )
                        }
                        cad_core::SketchCurve::Circle { radius, .. } => {
                            if let Some(radius) = evaluated
                                .and_then(|output| output.circles.get(id))
                                .map(|circle| circle.radius_mm)
                                .filter(|radius| radius.is_finite() && *radius > 0.)
                            {
                                format!(
                                    "Path {} · circular {} · R {}",
                                    ordinal + 1,
                                    settings.format_length(2. * std::f64::consts::PI * radius),
                                    settings.format_length(radius)
                                )
                            } else {
                                format!(
                                    "Path {} · circular · radius {} (source)",
                                    ordinal + 1,
                                    crate::quantity_display::expression(&radius.0)
                                )
                            }
                        }
                        _ => return None,
                    };
                    Some((id.clone(), label))
                })
                .collect()
        })
        .unwrap_or_default()
}
pub(crate) fn member_choices(
    w: &Workspace,
    stock: &cad_core::weldment::Stock,
) -> Vec<(cad_core::weldment::MemberRef, String)> {
    stock
        .groups
        .iter()
        .flat_map(|group| {
            let group_name = w
                .session
                .visible()
                .parts
                .get(&group.source.definition)
                .and_then(|part| {
                    part.features
                        .iter()
                        .find(|feature| feature.id == group.source.node)
                })
                .map(|feature| feature.name.as_str())
                .unwrap_or("Missing group");
            structural_path_members(w, &group.path)
                .into_iter()
                .filter(|(id, _)| group.members.is_empty() || group.members.contains(id))
                .map(move |(member, label)| {
                    (
                        cad_core::weldment::MemberRef {
                            group: group.source.clone(),
                            member,
                        },
                        format!("{group_name} · {label}"),
                    )
                })
        })
        .collect()
}

fn end_fields(ui: &mut egui::Ui, end: &mut cad_core::weldment::End) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.label("Member End");
        changed |= ui
            .selectable_value(end, cad_core::weldment::End::Start, "Start")
            .changed();
        changed |= ui
            .selectable_value(end, cad_core::weldment::End::Finish, "Finish")
            .changed();
    });
    changed
}

fn inherited_stock(
    ui: &mut egui::Ui,
    doc: &Document,
    current: &str,
    body: &mut Option<NodeRef>,
) -> bool {
    let choices = weldment_choices(doc, current);
    let mut append = body.is_some();
    let mut changed = false;
    if ui
        .add_enabled(
            !choices.is_empty() || append,
            egui::Checkbox::new(&mut append, "Append to Existing Weldment"),
        )
        .changed()
    {
        *body = if append {
            choices.last().map(|(reference, _)| reference.clone())
        } else {
            None
        };
        changed = true;
    }
    if let Some(reference) = body {
        changed |= selector(ui, "Weldment", reference, choices);
    }
    changed
}

fn weldment_preset(w: &mut Workspace, name: &str) {
    use cad_core::weldment::{Corner, End, WeldmentOperation};
    crate::history::latest(w);
    if w.confirm_requested {
        queue_after_confirm(w, name);
        return;
    }
    if w.sketch.is_some() {
        if !queue_after_confirm(w, name) {
            return;
        }
        w.confirm_tool();
        if w.sketch.is_some() && !w.confirm_requested {
            w.after_confirm = None;
        }
        return;
    }
    if w.tool_checkpoint.is_some() {
        w.status = "Confirm or cancel the current feature first".into();
        return;
    }
    let Some(mut part) = w
        .view
        .active_part
        .as_ref()
        .and_then(|id| w.session.visible().parts.get(id))
        .cloned()
    else {
        w.status = "Create or select a part first".into();
        return;
    };
    let body = w
        .selected
        .as_ref()
        .and_then(|selected| selected.body.as_ref())
        .filter(|reference| {
            reference.definition == part.id
                && part.features.iter().any(|feature| {
                    feature.id == reference.node
                        && !feature.suppressed
                        && matches!(feature.operation, Operation::Weldment { .. })
                })
        })
        .cloned()
        .or_else(|| {
            part.features
                .iter()
                .find(|feature| {
                    w.view.active_node.as_ref() == Some(&feature.id)
                        && !feature.suppressed
                        && matches!(feature.operation, Operation::Weldment { .. })
                })
                .or_else(|| {
                    part.features.iter().rev().find(|feature| {
                        !feature.suppressed
                            && part.outputs.contains(&feature.id)
                            && matches!(feature.operation, Operation::Weldment { .. })
                    })
                })
                .map(|feature| part.reference(&feature.id))
        });
    let profiles = part
        .features
        .iter()
        .filter(|feature| {
            !feature.suppressed && matches!(feature.operation, Operation::Profile { .. })
        })
        .map(|feature| part.reference(&feature.id))
        .collect::<Vec<_>>();
    let closed = |profile: &NodeRef| closed_profile(w, profile);
    let profile = preferred_profile(w, &profiles, closed);
    let operation = match name {
        "Structural Member Group" | "Custom Section Along Path" => {
            // A selected open path (or an already-authored path sketch) keeps
            // its path intent. A selected closed section otherwise has first
            // priority; new groups never silently return to the oldest sketch.
            let selected_path = selected_profile(w, &profiles).filter(|source| {
                !structural_path_members(w, source).is_empty()
                    && (!closed(source)
                        || body.is_none()
                            && profiles.last() == Some(source)
                            && profiles
                                .iter()
                                .take_while(|earlier| *earlier != source)
                                .any(closed)
                        || part.features.iter().any(|feature| {
                            !feature.suppressed
                                && matches!(&feature.operation,
                            Operation::Weldment {operation} if matches!(operation.as_ref(),
                                WeldmentOperation::Members {path,..} if path == source))
                        }))
            });
            let Some(section) = preferred_profile(w, &profiles, |source| {
                closed(source) && selected_path.as_ref() != Some(source)
            }) else {
                w.status =
                    "Create a closed section sketch and a separate line/arc path sketch first"
                        .into();
                return;
            };
            let Some(path) = selected_path.or_else(|| {
                preferred_profile(w, &profiles, |source| {
                    *source != section && !structural_path_members(w, source).is_empty()
                })
            }) else {
                w.status = "Create a separate line/arc path sketch for the member section".into();
                return;
            };
            WeldmentOperation::Members {
                body,
                section,
                path,
                members: vec![],
                member_mode: cad_core::weldment::MemberMode::All,
                roll: "0 deg".into(),
                anchor: None,
                profile_offset: ["0 mm".into(), "0 mm".into()],
                library_profile: None,
                start_offset: "0 mm".into(),
                end_offset: "0 mm".into(),
                corner: Corner::Miter,
                joint_gap: "0 mm".into(),
            }
        }
        "Trim / Extend Member" => {
            let Some(body) = body else {
                w.status = "Select native weldment stock first".into();
                return;
            };
            let stock = match cad_core::weldment::stock(w.session.visible(), &body) {
                Ok(stock) => stock,
                Err(error) => {
                    w.status = error.to_string();
                    return;
                }
            };
            let Some((member, _)) = member_choices(w, &stock).into_iter().next() else {
                w.status = "Weldment needs an open line or arc member".into();
                return;
            };
            let plane = w
                .selected
                .as_ref()
                .filter(|selected| selected.reference.topology == "face")
                .and_then(|selected| {
                    selected.body.clone().map(|body| {
                        cad_core::reference_geometry::PlaneSource::Face {
                            reference: selected.reference.clone(),
                            body,
                        }
                    })
                })
                .unwrap_or(cad_core::reference_geometry::PlaneSource::Origin {
                    plane: cad_core::reference_geometry::OriginPlane::Right,
                });
            WeldmentOperation::Trim {
                body,
                members: vec![member],
                plane,
                offset: "0 mm".into(),
                end: End::Finish,
            }
        }
        "Weldment End Cap" => {
            let Some(body) = body else {
                w.status = "Select native weldment stock first".into();
                return;
            };
            let stock = match cad_core::weldment::stock(w.session.visible(), &body) {
                Ok(stock) => stock,
                Err(error) => {
                    w.status = error.to_string();
                    return;
                }
            };
            let Some((member, _)) = member_choices(w, &stock).into_iter().next() else {
                w.status = "Weldment needs a member end".into();
                return;
            };
            WeldmentOperation::EndCap {
                body,
                member,
                end: End::Finish,
                thickness: "2 mm".into(),
                offset: "0 mm".into(),
                contour: cad_core::weldment::CapContour::Outer,
                edge_offset: "0 mm".into(),
            }
        }
        "Sketch Gusset" => {
            let Some(profile) = profile else {
                crate::sketch::begin(w, None);
                w.status = "Draw a closed gusset sketch, then add its thickness".into();
                return;
            };
            WeldmentOperation::Gusset {
                body,
                profile,
                thickness: "2 mm".into(),
                side: ThicknessSide::MidPlane,
            }
        }
        _ => return,
    };
    let feature = Feature {
        id: cad_core::id(),
        name: format!(
            "{}{}",
            name,
            part.features
                .iter()
                .filter(|feature| feature.name.starts_with(name))
                .count()
                + 1
        ),
        operation: Operation::Weldment {
            operation: Box::new(operation),
        },
        suppressed: false,
    };
    for source in feature
        .operation
        .consumed_outputs()
        .iter()
        .filter(|reference| reference.definition == part.id)
    {
        part.outputs.retain(|id| id != &source.node);
    }
    part.outputs.push(feature.id.clone());
    part.features.push(feature.clone());
    w.start_tool();
    w.dispatch(Command::PutPart {
        part,
        assembly: None,
        placement: Default::default(),
    });
    w.view.active_node = Some(feature.id.clone());
    ui::feature_dialog(w, Some(feature));
}

pub fn weldment_fields(
    ui: &mut egui::Ui,
    w: &Workspace,
    current: &str,
    operation: &mut cad_core::weldment::WeldmentOperation,
) -> bool {
    use cad_core::weldment::{CapContour, Corner, WeldmentOperation};
    let doc = w.session.visible();
    let mut changed = false;
    match operation {
        WeldmentOperation::Members {
            body,
            section,
            path,
            members,
            roll,
            anchor,
            start_offset,
            end_offset,
            corner,
            joint_gap,
            profile_offset,
            member_mode: _,
            library_profile: _,
        } => {
            changed |= inherited_stock(ui, doc, current, body);
            let sections = section_choices(w, current, section);
            changed |= selector(ui, "Closed Section Sketch", section, sections);
            let paths = structural_path_choices(w, current, path, section);
            changed |= selector(ui, "Path Sketch", path, paths);
            let mut all = members.is_empty();
            if ui.checkbox(&mut all, "All Path Members").changed() {
                *members = if all {
                    vec![]
                } else {
                    structural_path_members(w, path)
                        .into_iter()
                        .next()
                        .map(|(id, _)| vec![id])
                        .unwrap_or_default()
                };
                changed = true;
            }
            if !all {
                egui::CollapsingHeader::new("Path Members")
                    .default_open(true)
                    .show(ui, |ui| {
                        for (id, label) in structural_path_members(w, path) {
                            let mut selected = members.contains(&id);
                            if ui.checkbox(&mut selected, label).changed() {
                                if selected {
                                    members.push(id);
                                } else {
                                    members.retain(|member| member != &id);
                                }
                                changed = true;
                            }
                        }
                    });
                ui.small("Removing the last selected member restores All Path Members.");
            }
            let points = source_sketch(w, section)
                .map(|sketch| sketch.points)
                .unwrap_or_default();
            egui::ComboBox::from_label("Section Locating Point")
                .selected_text(
                    anchor
                        .as_ref()
                        .and_then(|id| points.get(id))
                        .map(|point| format!("({:.2}, {:.2}) mm", point[0], point[1]))
                        .unwrap_or_else(|| {
                            if anchor.is_some() {
                                "Missing locating point".into()
                            } else {
                                "Sketch Origin".into()
                            }
                        }),
                )
                .show_ui(ui, |ui| {
                    changed |= ui.selectable_value(anchor, None, "Sketch Origin").changed();
                    for (id, point) in points {
                        changed |= ui
                            .selectable_value(
                                anchor,
                                Some(id),
                                format!("({:.2}, {:.2}) mm", point[0], point[1]),
                            )
                            .changed();
                    }
                });
            changed |= expression(ui, "Section Rotation", roll);
            changed |= expression(ui, "Start Shortening (+) / Extension (−)", start_offset);
            changed |= expression(ui, "Finish Shortening (+) / Extension (−)", end_offset);
            egui::ComboBox::from_label("Corner Treatment")
                .selected_text(format!("{corner:?}"))
                .show_ui(ui, |ui| {
                    changed |= ui
                        .selectable_value(corner, Corner::Miter, "Miter")
                        .changed();
                    changed |= ui.selectable_value(corner, Corner::Butt, "Butt").changed();
                    changed |= ui
                        .selectable_value(corner, Corner::ButtTrim, "Trim to primary member")
                        .changed();
                });
            changed |= expression(ui, "Joint Gap", joint_gap);
            changed |= expression(ui, "Profile Offset X", &mut profile_offset[0]);
            changed |= expression(ui, "Profile Offset Y", &mut profile_offset[1]);
            if *corner == Corner::ButtTrim {
                let choices = structural_path_members(w, path);
                let selected = members
                    .first()
                    .or_else(|| choices.first().map(|(id, _)| id))
                    .cloned();
                egui::ComboBox::from_label("Primary Member")
                    .selected_text(
                        choices
                            .iter()
                            .find(|(id, _)| Some(id) == selected.as_ref())
                            .map(|(_, label)| label.as_str())
                            .unwrap_or("Select a member"),
                    )
                    .show_ui(ui, |ui| {
                        for (id, label) in &choices {
                            if ui
                                .selectable_label(Some(id) == selected.as_ref(), label)
                                .clicked()
                            {
                                if members.is_empty() {
                                    *members = choices.iter().map(|(id, _)| id.clone()).collect();
                                }
                                if members.contains(id) {
                                    members.retain(|member| member != id);
                                    members.insert(0, id.clone());
                                    changed = true;
                                }
                            }
                        }
                    });
            }
            ui.small("Straight, circular-arc and closed circular stock retain separate solids. Miter needs two incident members; primary butt trim supports convex polygon sections and branch junctions. Curved joints require matching tangents.");
        }
        WeldmentOperation::Trim {
            body,
            members,
            plane,
            offset,
            end,
        } => {
            changed |= selector(ui, "Weldment", body, weldment_choices(doc, current));
            if let Ok(stock) = cad_core::weldment::stock(doc, body) {
                let choices = member_choices(w, &stock);
                let mut all = members.is_empty();
                if ui.checkbox(&mut all, "All Members").changed() {
                    *members = if all {
                        vec![]
                    } else {
                        choices
                            .first()
                            .map(|(reference, _)| vec![reference.clone()])
                            .unwrap_or_default()
                    };
                    changed = true;
                }
                if !all {
                    for (reference, label) in choices {
                        let mut selected = members.contains(&reference);
                        if ui.checkbox(&mut selected, label).changed() {
                            if selected {
                                members.push(reference);
                            } else {
                                members.retain(|member| member != &reference);
                            }
                            changed = true;
                        }
                    }
                }
            }
            changed |= end_fields(ui, end);
            changed |= crate::reference_ui::plane(ui, w, doc, current, "End Trim Plane", plane);
            changed |= expression(ui, "Plane Offset", offset);
            ui.small("Straight stock projects to an associative plane; circular stock supports planes normal to the local tangent. Positive developed-length validation prevents collapsed members.");
        }
        WeldmentOperation::EndCap {
            body,
            member,
            end,
            thickness,
            offset,
            contour,
            edge_offset,
        } => {
            changed |= selector(ui, "Weldment", body, weldment_choices(doc, current));
            if let Ok(stock) = cad_core::weldment::stock(doc, body) {
                let choices = member_choices(w, &stock);
                egui::ComboBox::from_label("Member")
                    .selected_text(
                        choices
                            .iter()
                            .find(|(reference, _)| reference == member)
                            .map(|(_, label)| label.as_str())
                            .unwrap_or("Missing member"),
                    )
                    .show_ui(ui, |ui| {
                        for (reference, label) in choices {
                            changed |= ui.selectable_value(member, reference, label).changed();
                        }
                    });
            }
            changed |= end_fields(ui, end);
            changed |= expression(ui, "Cap Thickness", thickness);
            egui::ComboBox::from_label("Cap Contour")
                .selected_text(format!("{contour:?}"))
                .show_ui(ui, |ui| {
                    changed |= ui
                        .selectable_value(contour, CapContour::Outer, "Outer contour")
                        .changed();
                    changed |= ui
                        .selectable_value(contour, CapContour::Inner, "Inner contour (inset)")
                        .changed();
                });
            changed |= expression(ui, "Outward Gap (+) / Recess (−)", offset);
            changed |= expression(ui, "Contour Oversize (+) / Clearance (−)", edge_offset);
            ui.small("Outer or inset caps stay separate manufacturing solids and receive cut-list rows. Inner caps need one hollow contour; recessed placement needs straight stock. Clearance supports line/arc contours.");
        }
        WeldmentOperation::Gusset {
            body,
            profile,
            thickness,
            side,
        } => {
            changed |= inherited_stock(ui, doc, current, body);
            changed |= selector(ui, "Gusset Sketch", profile, profiles(doc, current));
            changed |= expression(ui, "Thickness", thickness);
            egui::ComboBox::from_label("Thickness Direction")
                .selected_text(format!("{side:?}"))
                .show_ui(ui, |ui| {
                    for (value, label) in [
                        (ThicknessSide::Positive, "Along sketch normal"),
                        (ThicknessSide::Negative, "Reverse"),
                        (ThicknessSide::MidPlane, "Mid-plane"),
                    ] {
                        changed |= ui.selectable_value(side, value, label).changed();
                    }
                });
        }
    }
    changed
}
