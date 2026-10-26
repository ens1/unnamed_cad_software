//! User-composed command ribbons and history views over existing semantic actions.
use crate::presentation_composition::RenderContext;
use crate::{Workspace, command_icons, presentation_controls as controls, ui};
use bevy_egui::egui;
use cad_core::presentation::{
    LayoutDocument, LayoutNode, RibbonDefinition, RibbonPage, SemanticElement,
};
use serde_json::{Value, json};

fn chosen_page<'a>(
    w: &Workspace,
    id: &str,
    ribbon: &'a RibbonDefinition,
) -> Option<&'a RibbonPage> {
    let remembered = w.view.ribbon_pages.get(id);
    ribbon
        .pages
        .iter()
        .find(|page| Some(&page.id) == remembered && page.command_tab == w.command_tab)
        .or_else(|| {
            ribbon
                .pages
                .iter()
                .find(|page| page.command_tab == w.command_tab)
        })
        .or_else(|| ribbon.pages.first())
}

pub fn action(w: &mut Workspace, command: &str, data: &Value) -> bool {
    if command != "presentation.ribbon-page" {
        return false;
    }
    let Some(region) = data.get("region").and_then(Value::as_str) else {
        return true;
    };
    let Some(page_id) = data.get("page").and_then(Value::as_str) else {
        return true;
    };
    let page = w
        .layout
        .node_for_in(&w.view.active_layout, region)
        .and_then(|node| node.ribbon.as_ref())
        .and_then(|ribbon| ribbon.pages.iter().find(|page| page.id == page_id))
        .cloned();
    if let Some(page) = page {
        let previous_layout = w.view.active_layout.clone();
        ui::action(
            w,
            &format!(
                "presentation.command-tab.{}",
                page.command_tab.to_lowercase()
            ),
        );
        if w.command_tab == page.command_tab {
            w.view.ribbon_pages.insert(region.into(), page.id.clone());
            if previous_layout != w.view.active_layout {
                fn remember(
                    node: &LayoutNode,
                    page: &RibbonPage,
                    into: &mut std::collections::BTreeMap<String, String>,
                ) {
                    if let (Some(id), Some(ribbon)) = (&node.id, &node.ribbon)
                        && ribbon.pages.iter().any(|candidate| {
                            candidate.id == page.id && candidate.command_tab == page.command_tab
                        })
                    {
                        into.insert(id.clone(), page.id.clone());
                    }
                    for child in &node.children {
                        remember(child, page, into);
                    }
                }
                if let Some(root) = w.layout.layouts.get(&w.view.active_layout) {
                    remember(root, &page, &mut w.view.ribbon_pages);
                }
            }
        }
    }
    true
}

fn page_element(w: &Workspace, region: &str, page: &RibbonPage, selected: bool) -> SemanticElement {
    let command = format!(
        "presentation.command-tab.{}",
        page.command_tab.to_lowercase()
    );
    let reason = crate::cad_ui::command_disabled_reason(w, &command);
    SemanticElement {
        id: format!("ribbon:{region}:page:{}", page.id),
        role: "ribbon-tab".into(),
        label: page.label.clone(),
        value: None,
        state: if reason.is_some() {
            "disabled"
        } else if selected {
            "selected"
        } else {
            "ready"
        }
        .into(),
        commands: vec!["presentation.ribbon-page".into()],
        relationships: vec![region.into()],
        data: json!({"contract":"tcad.command-ribbon","version":1,"region":region,"page":page.id,"command_tab":page.command_tab,"enabled":reason.is_none(),"disabled_reason":reason,"presentation_only":true}),
    }
}

pub fn semantics(w: &Workspace, catalog: &[SemanticElement]) -> Vec<SemanticElement> {
    let mut out = Vec::new();
    if let Some(root) = w.layout.layouts.get(&w.view.active_layout) {
        project_ribbon_node(w, catalog, &w.view.active_layout, root, &mut out);
    }
    out
}

/// Candidate packages own their ribbon descriptors before they become active.
/// The composition tag permits validation of alternate layouts with reused IDs
/// without mixing those layouts' controls or style ancestry.
pub fn semantics_for_layout(
    w: &Workspace,
    layout: &LayoutDocument,
    catalog: &[SemanticElement],
) -> Vec<SemanticElement> {
    let mut out = Vec::new();
    for (name, root) in &layout.layouts {
        project_ribbon_node(w, catalog, name, root, &mut out);
    }
    out
}

fn project_ribbon_node(
    w: &Workspace,
    catalog: &[SemanticElement],
    composition: &str,
    node: &LayoutNode,
    out: &mut Vec<SemanticElement>,
) {
    let first = out.len();
    if let (Some(region), Some(ribbon)) = (&node.id, &node.ribbon) {
        let selected = chosen_page(w, region, ribbon);
        for page in &ribbon.pages {
            out.push(page_element(
                w,
                region,
                page,
                selected.is_some_and(|selected| selected.id == page.id),
            ));
            for group in &page.groups {
                let group_id = format!("ribbon:{region}:{}:{}", page.id, group.id);
                let state = if selected.is_some_and(|selected| selected.id == page.id) {
                    "ready"
                } else {
                    "hidden"
                };
                let mut children = Vec::new();
                for item in &group.commands {
                    let Some(source) = catalog
                        .iter()
                        .find(|element| element.id == format!("command-toolbar/{}", item.command))
                        .or_else(|| {
                            catalog
                                .iter()
                                .find(|element| element.id == format!("command:{}", item.command))
                        })
                    else {
                        continue;
                    };
                    let mut actions = source.action_descriptors();
                    let Some(first_action) = actions.first() else {
                        continue;
                    };
                    let label = if first_action.command == item.command {
                        item.label.clone().unwrap_or_else(|| source.label.clone())
                    } else {
                        source.label.clone()
                    };
                    for action in &mut actions {
                        if let Some(reason) =
                            crate::cad_ui::command_disabled_reason(w, &action.command)
                        {
                            action.enabled = false;
                            action.disabled_reason = Some(reason.into());
                        }
                    }
                    let mut element = source.clone();
                    element.id = format!(
                        "{group_id}:command:{}",
                        item.id.as_deref().unwrap_or(&item.command)
                    );
                    element.role = "ribbon-action".into();
                    element.label = label;
                    element.relationships = vec![group_id.clone(), source.id.clone()];
                    let mut classes = vec!["ribbon-tool".to_owned()];
                    classes.extend(item.classes.iter().cloned());
                    element.data["contract"] = json!("tcad.command-ribbon");
                    element.data["version"] = json!(1);
                    element.data["parent"] = json!(group_id);
                    element.data["classes"] = json!(classes);
                    element.data["accessibility_label"] = json!(source.label);
                    // Save the original action arguments before adding presentation metadata.
                    element.data["actions"] = json!(actions);
                    if let Some(action) = actions.first().filter(|action| !action.enabled) {
                        element.state = "disabled".into();
                        element.value = action.disabled_reason.clone().or(element.value);
                    }
                    children.push(element.id.clone());
                    out.push(element);
                }
                if let Some(menu) = group.menu.as_ref().and_then(|menu| {
                    catalog
                        .iter()
                        .find(|element| element.id == *menu && element.role == "menu")
                }) {
                    let mut element = menu.clone();
                    element.id = format!("{group_id}:menu");
                    element.role = "ribbon-group-menu".into();
                    element.label = format!("{} ▾", group.label);
                    element.relationships = vec![group_id.clone(), menu.id.clone()];
                    element.data["contract"] = json!("tcad.command-ribbon");
                    element.data["version"] = json!(1);
                    element.data["parent"] = json!(group_id);
                    element.data["classes"] = json!(["menu-heading"]);
                    children.push(element.id.clone());
                    out.push(element);
                } else {
                    let id = format!("{group_id}:label");
                    children.push(id.clone());
                    out.push(SemanticElement {
                            id, role: "ribbon-group-label".into(), label: group.label.clone(),
                            value: None, state: "ready".into(), commands: vec![],
                            relationships: vec![group_id.clone()],
                            data: json!({"contract":"tcad.command-ribbon","version":1,"parent":group_id,"presentation_only":true}),
                        });
                }
                out.push(SemanticElement {
                        id:group_id,role:"ribbon-group".into(),label:group.label.clone(),value:None,
                        state:state.into(),commands:vec![],relationships:vec![region.clone(),format!("ribbon:{region}:page:{}",page.id)],
                        data:json!({"contract":"tcad.command-ribbon","version":1,"parent":region,"commands":group.commands,"menu":group.menu,"children":children,"presentation_only":true}),
                    });
            }
        }
    }
    for element in &mut out[first..] {
        element.data["composition"] = json!(composition);
    }
    for child in &node.children {
        project_ribbon_node(w, catalog, composition, child, out);
    }
    if let Some(template) = &node.template {
        project_ribbon_node(w, catalog, composition, template, out);
    }
}

/// Validate authored command references at the same boundary as layout/CSS.
pub fn validate(layout: &LayoutDocument, semantics: &[SemanticElement]) -> cad_core::Result<()> {
    fn walk(node: &LayoutNode, semantics: &[SemanticElement]) -> cad_core::Result<()> {
        if let Some(ribbon) = &node.ribbon {
            if node.id.as_ref().is_none_or(|id| id.is_empty()) {
                return Err(cad_core::CadError::Invalid(
                    "Command ribbons need a stable placement id".into(),
                ));
            }
            for page in &ribbon.pages {
                for group in &page.groups {
                    for item in &group.commands {
                        if !semantics.iter().any(|element| {
                            element.id == format!("command-toolbar/{}", item.command)
                                || element.id == format!("command:{}", item.command)
                        }) {
                            return Err(cad_core::CadError::Invalid(format!(
                                "Ribbon {} references unavailable command {}",
                                page.label, item.command
                            )));
                        }
                    }
                    if let Some(menu) = &group.menu
                        && !semantics
                            .iter()
                            .any(|element| element.id == *menu && element.role == "menu")
                    {
                        return Err(cad_core::CadError::Invalid(format!(
                            "Ribbon {} references unavailable menu {menu}",
                            page.label
                        )));
                    }
                }
            }
        }
        for child in &node.children {
            walk(child, semantics)?;
        }
        if let Some(template) = &node.template {
            walk(template, semantics)?;
        }
        Ok(())
    }
    for root in layout.layouts.values() {
        walk(root, semantics)?;
    }
    Ok(())
}

fn tabs(ui: &mut egui::Ui, w: &mut Workspace, id: &str, ribbon: &RibbonDefinition) {
    let selected = chosen_page(w, id, ribbon).map(|page| page.id.clone());
    egui::ScrollArea::horizontal()
        .id_salt((id, "tabs"))
        .auto_shrink([false, true])
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                for page in &ribbon.pages {
                    let element = page_element(w, id, page, selected.as_deref() == Some(&page.id));
                    if controls::button(
                        ui,
                        &element.id,
                        &element.role,
                        &[],
                        &element.label,
                        element.state != "disabled",
                        element.state == "selected",
                    )
                    .on_disabled_hover_text(
                        element.data["disabled_reason"]
                            .as_str()
                            .unwrap_or("Finish the current action first"),
                    )
                    .clicked()
                    {
                        ui::semantic_action(w, &element, "presentation.ribbon-page");
                        ui.ctx().request_repaint();
                    }
                }
            });
        });
}

pub fn ribbon(ui: &mut egui::Ui, w: &mut Workspace, context: &RenderContext<'_>) {
    let id = &context.region.id;
    let Some(ribbon) = context
        .layout
        .node_for_in(&w.view.active_layout, id)
        .and_then(|node| node.ribbon.as_ref())
    else {
        controls::label(
            ui,
            id,
            "presentation-diagnostic",
            &[],
            "This ribbon needs pages and command groups in its layout file.",
            "invalid",
        );
        return;
    };
    if ribbon.tabs_position == "top" {
        tabs(ui, w, id, ribbon);
    }
    let page = chosen_page(w, id, ribbon);
    egui::ScrollArea::horizontal()
        .id_salt((id, "groups"))
        .auto_shrink([false, true])
        .show(ui, |ui| {
            ui.horizontal_top(|ui| {
                if let Some(page) = page {
                    for group in &page.groups {
                        let group_id = format!("ribbon:{id}:{}:{}", page.id, group.id);
                        let target = controls::current_target()
                            .unwrap_or_else(|| {
                                context.region.style_target(&w.view.active_layout, "ready")
                            })
                            .child(&group_id, "ribbon-group", &[], "ready");
                        let props = controls::properties(&group_id, "ribbon-group", &[], "ready");
                        if props.get("display").is_some_and(|value| value == "none") {
                            continue;
                        }
                        controls::with_target(target, ui, |ui| {
                            controls::frame_from_properties(ui, &props).show(ui, |ui| {
                                controls::apply_properties(ui, &props);
                                ui.set_min_width(controls::number(&props, "min-width", 0.));
                                ui.set_min_height(controls::number(&props, "min-height", 0.));
                                if props.contains_key("width") {
                                    ui.set_width(controls::number(
                                        &props,
                                        "width",
                                        ui.available_width(),
                                    ));
                                }
                                if props.contains_key("height") {
                                    ui.set_height(controls::number(
                                        &props,
                                        "height",
                                        ui.available_height(),
                                    ));
                                }
                                ui.vertical(|ui| {
                                    ui.horizontal(|ui| {
                                        for item in &group.commands {
                                            let descriptor = context
                                                .semantics
                                                .iter()
                                                .find(|element| {
                                                    element.id
                                                        == format!(
                                                            "command-toolbar/{}",
                                                            item.command
                                                        )
                                                })
                                                .or_else(|| {
                                                    context.semantics.iter().find(|element| {
                                                        element.id
                                                            == format!("command:{}", item.command)
                                                    })
                                                });
                                            let Some(descriptor) = descriptor else {
                                                continue;
                                            };
                                            let Some(action) =
                                                descriptor.action_descriptors().into_iter().next()
                                            else {
                                                continue;
                                            };
                                            // Context can change a tool into its completion action. Keep that
                                            // semantic label instead of an authored creation-time alias.
                                            let label = if action.command != item.command {
                                                descriptor.label.as_str()
                                            } else {
                                                item.label.as_deref().unwrap_or(&descriptor.label)
                                            };
                                            let reason = crate::cad_ui::command_disabled_reason(
                                                w,
                                                &action.command,
                                            )
                                            .or(action.disabled_reason.as_deref());
                                            let painter = |p: &egui::Painter, c, s, col| {
                                                command_icons::paint(p, c, s, col, &action.command)
                                            };
                                            let mut classes = vec!["ribbon-tool"];
                                            classes.extend(item.classes.iter().map(String::as_str));
                                            let response =
                                                controls::icon_button(
                                                    ui,
                                                    &format!(
                                                        "{group_id}:command:{}",
                                                        item.id.as_deref().unwrap_or(&item.command)
                                                    ),
                                                    "ribbon-action",
                                                    &classes,
                                                    label,
                                                    action.enabled && reason.is_none(),
                                                    descriptor.state == "selected",
                                                    Some(&painter),
                                                )
                                                .on_hover_text(&descriptor.label)
                                                .on_disabled_hover_text(reason.unwrap_or(
                                                    "Select the required geometry first",
                                                ));
                                            if response.clicked() {
                                                let mut element = descriptor.clone();
                                                element.data = action.arguments;
                                                ui::semantic_action(w, &element, &action.command);
                                            }
                                        }
                                    });
                                    if let Some(menu) = group.menu.as_ref().and_then(|id| {
                                        context.semantics.iter().find(|element| &element.id == id)
                                    }) {
                                        let mut menu = menu.clone();
                                        menu.label = format!("{} ▾", group.label);
                                        crate::cad_ui::semantic_menu_as(
                                            ui,
                                            w,
                                            &menu,
                                            &format!("{group_id}:menu"),
                                            "ribbon-group-menu",
                                        );
                                    } else {
                                        controls::label(
                                            ui,
                                            &format!("{group_id}:label"),
                                            "ribbon-group-label",
                                            &[],
                                            &group.label,
                                            "ready",
                                        );
                                    }
                                });
                            });
                        });
                    }
                }
            });
        });
    if ribbon.tabs_position == "bottom" {
        tabs(ui, w, id, ribbon);
    }
}

pub fn timeline(ui: &mut egui::Ui, w: &mut Workspace, context: &RenderContext<'_>) {
    egui::ScrollArea::horizontal()
        .id_salt((&context.region.id, "history"))
        .auto_shrink([false, true])
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let painter = |p: &egui::Painter, c, s, col| {
                    command_icons::paint(p, c, s, col, "history.latest")
                };
                if controls::icon_button(
                    ui,
                    "timeline:latest",
                    "timeline-action",
                    &[],
                    "Latest model",
                    true,
                    w.view.history_cursor.is_none(),
                    Some(&painter),
                )
                .on_hover_text("Return to the latest model")
                .clicked()
                {
                    crate::history::latest(w);
                }
                let mut any = false;
                for element in context
                    .semantics
                    .iter()
                    .filter(|element| element.role == "history-station")
                {
                    any = true;
                    let Some(action) = element.action_descriptors().into_iter().next() else {
                        continue;
                    };
                    let icon = element
                        .data
                        .get("icon_command")
                        .and_then(Value::as_str)
                        .unwrap_or("feature.extrude");
                    let painter =
                        |p: &egui::Painter, c, s, col| command_icons::paint(p, c, s, col, icon);
                    let label = format!(
                        "{} · {}",
                        element.data["index"].as_u64().unwrap_or(0) + 1,
                        element.label
                    );
                    if controls::icon_button(
                        ui,
                        &element.id,
                        "history-station",
                        &["timeline-station"],
                        &label,
                        action.enabled,
                        element.state == "selected",
                        Some(&painter),
                    )
                    .on_hover_text(format!(
                        "{label}\n{}",
                        element
                            .value
                            .as_deref()
                            .unwrap_or("Inspect this point in history")
                    ))
                    .on_disabled_hover_text(
                        action
                            .disabled_reason
                            .as_deref()
                            .unwrap_or("History unavailable"),
                    )
                    .clicked()
                    {
                        let mut element = element.clone();
                        element.data = action.arguments;
                        ui::semantic_action(w, &element, &action.command);
                    }
                }
                if !any {
                    controls::label(
                        ui,
                        "timeline:empty",
                        "timeline-empty",
                        &[],
                        "Your feature history appears here",
                        "ready",
                    );
                }
            });
        });
}
