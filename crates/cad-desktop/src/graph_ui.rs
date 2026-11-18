//! Dependency cards over application-owned semantic elements. Graph layout is presentation data.
use crate::{
    Workspace, cad_ui, command_icons, presentation_controls as controls, selection_ui, ui,
};
use bevy_egui::egui::{self, FontId, Stroke};
use cad_core::{Document, Part, presentation::SemanticElement, selection_actions::SelectionTarget};
use std::collections::{BTreeMap, BTreeSet};

pub(crate) fn scope_reason(w: &Workspace, choice: &str) -> Option<&'static str> {
    if choice.eq_ignore_ascii_case("features") && w.view.active_part.is_none() {
        Some("Open a part to see its features")
    } else if choice.eq_ignore_ascii_case("assembly") && !w.has_assembly_context() {
        Some("Open an assembly to see its relationships")
    } else {
        None
    }
}

pub(crate) fn scope(w: &Workspace) -> &'static str {
    if w.view.active_part.is_none()
        || w.view.graph_scope == "Assembly" && scope_reason(w, "Assembly").is_none()
    {
        "Assembly"
    } else {
        "Features"
    }
}

fn toolbar(ui: &mut egui::Ui, w: &mut Workspace) {
    ui.horizontal_wrapped(|ui| {
        let actual = scope(w);
        controls::menu_button(ui, "graph.scope", actual, |ui| {
            for choice in ["Features", "Assembly"] {
                let command = format!("presentation.graph-scope.{}", choice.to_lowercase());
                let reason = cad_ui::command_disabled_reason(w, &command);
                if controls::menu_item(
                    ui,
                    &command,
                    choice,
                    None,
                    reason.is_none(),
                    actual == choice,
                )
                .on_disabled_hover_text(reason.unwrap_or_default())
                .clicked()
                {
                    ui.close();
                    ui::action(w, &command);
                }
            }
        });
        let arrange = "graph.arrange";
        let reason = cad_ui::command_disabled_reason(w, arrange);
        if controls::icon_button(
            ui,
            arrange,
            "graph-action",
            &["icon-action"],
            "Arrange graph",
            reason.is_none(),
            false,
            Some(&|p, c, s, color| command_icons::paint(p, c, s, color, arrange)),
        )
        .on_hover_text("Arrange cards and reset the graph view")
        .on_disabled_hover_text(reason.unwrap_or_default())
        .clicked()
        {
            ui::action(w, arrange);
        }
    });
}

pub fn show(ui: &mut egui::Ui, w: &mut Workspace, part: Option<&Part>) {
    let doc = w.session.visible().clone();
    toolbar(ui, w);
    let all = cad_core::presentation::project(&w.session, None);
    let assembly_scope = scope(w) == "Assembly" || part.is_none();
    let mut nodes: Vec<SemanticElement>;
    if assembly_scope {
        let assembly = w
            .fields
            .get("assembly-id")
            .and_then(|id| doc.assemblies.get(id))
            .or_else(|| doc.assemblies.get(&doc.root));
        let Some(assembly) = assembly else {
            ui.weak("Open an assembly to see its relationships.");
            return;
        };
        let ids = assembly
            .instances
            .keys()
            .chain(assembly.mates.iter().map(|m| &m.id))
            .cloned()
            .collect::<BTreeSet<_>>();
        nodes = all
            .iter()
            .filter(|e| {
                ids.contains(&e.id)
                    && matches!(e.role.as_str(), "component-instance" | "assembly-mate")
            })
            .cloned()
            .collect();
        // Containment points from the completed assembly to its members in the
        // document. This view draws the modeling direction into the result.
        for node in &mut nodes {
            node.relationships.retain(|id| id != &assembly.id);
        }
        if let Some(mut result) = all
            .iter()
            .find(|e| e.id == assembly.id && e.role == "assembly")
            .cloned()
        {
            result.relationships = assembly
                .instances
                .keys()
                .chain(assembly.mates.iter().map(|m| &m.id))
                .cloned()
                .collect();
            nodes.push(result);
        }
    } else {
        let part = part.unwrap();
        nodes = part
            .features
            .iter()
            .filter_map(|f| {
                all.iter()
                    .find(|e| e.id == f.id && e.role == "feature-station")
                    .cloned()
            })
            .collect();
        nodes.extend(
            all.iter()
                .filter(|e| {
                    e.role == "parameter-field" && e.id.starts_with(&format!("{}::", part.id))
                })
                .cloned(),
        );
        let external = nodes
            .iter()
            .flat_map(|n| &n.relationships)
            .filter(|id| !nodes.iter().any(|n| &n.id == *id))
            .cloned()
            .collect::<BTreeSet<_>>();
        nodes.extend(
            all.iter()
                .filter(|e| {
                    external.contains(&e.id)
                        && matches!(e.role.as_str(), "feature-station" | "parameter-field")
                })
                .cloned(),
        );
    }
    if nodes.is_empty() {
        ui.weak("No features yet");
        return;
    }
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), ui.available_height().max(80.)),
        egui::Sense::click_and_drag(),
    );
    let painter = ui.painter().with_clip_rect(rect);
    let accent = ui.visuals().selection.stroke.color;
    let wide = rect.width() > 450.;
    if response.hovered() {
        let delta = ui.input(|i| i.smooth_scroll_delta);
        w.view.graph_pan[0] += delta.x;
        w.view.graph_pan[1] += delta.y;
        let zoom = ui.input(|i| i.zoom_delta());
        w.view.graph_zoom = (w.view.graph_zoom * zoom).clamp(0.4, 2.);
    }
    if response.dragged() {
        let delta = ui.input(|i| i.pointer.delta());
        w.view.graph_pan[0] += delta.x;
        w.view.graph_pan[1] += delta.y;
    }
    let zoom = w.view.graph_zoom;
    let mut next = if wide { [14., 18.] } else { [12., 14.] };
    let positions = nodes
        .iter()
        .map(|node| {
            let props = controls::properties(&node.id, "graph-card", &[&node.role], "ready");
            let card = egui::vec2(
                controls::length(
                    &props,
                    "width",
                    rect.width(),
                    if wide {
                        146.
                    } else {
                        (rect.width() - 24.).clamp(72., 280.)
                    },
                )
                .max(32.),
                controls::length(&props, "height", rect.height(), 50.).max(22.),
            );
            let default = next;
            if wide {
                next[0] += card.x + 30.;
            } else {
                next[1] += card.y + 20.;
            }
            let p = w
                .view
                .graph_positions
                .get(&node.id)
                .copied()
                .unwrap_or(default);
            (
                node.id.clone(),
                (
                    p,
                    egui::Rect::from_min_size(
                        rect.min
                            + egui::vec2(
                                p[0] * zoom + w.view.graph_pan[0],
                                p[1] * zoom + w.view.graph_pan[1],
                            ),
                        card * zoom,
                    ),
                ),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let mut inputs = BTreeSet::new();
    let mut outputs = BTreeSet::new();
    for node in &nodes {
        let end = positions[&node.id].1;
        for input in &node.relationships {
            if let Some((_, start)) = positions.get(input) {
                outputs.insert(input.as_str());
                inputs.insert(node.id.as_str());
                let (a, b) = if wide {
                    (start.right_center(), end.left_center())
                } else {
                    (start.center_bottom(), end.center_top())
                };
                let middle = if wide {
                    vec![
                        a,
                        a + egui::vec2(18. * zoom, 0.),
                        b - egui::vec2(18. * zoom, 0.),
                        b,
                    ]
                } else {
                    vec![
                        a,
                        a + egui::vec2(0., 12. * zoom),
                        b - egui::vec2(0., 12. * zoom),
                        b,
                    ]
                };
                painter.add(egui::Shape::line(
                    middle,
                    Stroke::new(1.3, accent.gamma_multiply(0.7)),
                ));
                let direction = (b - a).normalized();
                let side = egui::vec2(-direction.y, direction.x);
                painter.add(egui::Shape::convex_polygon(
                    vec![
                        b,
                        b - direction * 7. + side * 3.,
                        b - direction * 7. - side * 3.,
                    ],
                    accent,
                    Stroke::NONE,
                ));
            }
        }
    }
    for node in &nodes {
        let (old, card_rect) = positions[&node.id];
        let Some(target) = object_target(&doc, node) else {
            continue;
        };
        let active = w.context_selection.as_ref().map_or_else(
            || {
                w.view.active_node.as_ref() == Some(&node.id)
                    || w.view.active_instance.as_ref() == Some(&node.id)
            },
            |selected| selected == &target,
        );
        let hit = ui.interact(
            card_rect.intersect(rect),
            ui.id().with((&node.id, "graph-card")),
            egui::Sense::click_and_drag(),
        );
        let (icon, detail) = caption(&doc, node, &target);
        let state = if active {
            "selected"
        } else if hit.has_focus() {
            "focus"
        } else if hit.hovered() {
            "hover"
        } else {
            "ready"
        };
        let props = controls::properties(&node.id, "graph-card", &[&node.role], state);
        draw_card(
            ui,
            &painter,
            card_rect,
            node,
            &props,
            &icon,
            &detail,
            zoom,
            active,
            hit.has_focus() || hit.hovered(),
        );
        let ports = if wide {
            [card_rect.left_center(), card_rect.right_center()]
        } else {
            [card_rect.center_top(), card_rect.center_bottom()]
        };
        for (port, connected) in ports.into_iter().zip([
            inputs.contains(node.id.as_str()),
            outputs.contains(node.id.as_str()),
        ]) {
            if connected {
                painter.circle(
                    port,
                    3. * zoom,
                    ui.visuals().panel_fill,
                    Stroke::new(1.2, accent),
                );
            }
        }
        let hit = hit.on_hover_ui(|ui| {
            ui.strong(&node.label);
            if !detail.is_empty() {
                ui.label(&detail);
            }
            ui.small("Double-click or press Enter to edit");
        });
        hit.widget_info(|| {
            egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, active, &node.label)
        });
        let keyboard_edit = hit.has_focus()
            && ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
        if hit.clicked() || keyboard_edit {
            activate(
                w,
                &doc,
                &target,
                &hit,
                hit.double_clicked() || keyboard_edit,
            );
        }
        let anchor = hit.id.with("context-occurrence");
        if hit.secondary_clicked() {
            if w.tool_checkpoint.is_none() && w.sketch.is_none() && w.session.draft.is_none() {
                activate(w, &doc, &target, &hit, false);
            }
            ui.data_mut(|data| data.insert_temp(anchor, w.selected_component.clone()));
        }
        hit.context_menu(|ui| {
            let same_occurrence = !matches!(&target, SelectionTarget::Object { role, .. } if role == "component-instance")
                || ui.data(|data| data.get_temp::<Option<Vec<String>>>(anchor)).flatten().is_some_and(|opened| Some(&opened) == w.selected_component.as_ref());
            if selection_ui::current(w).as_ref() == Some(&target) && same_occurrence {
                selection_ui::contents(ui, w, &target);
            } else {
                ui.close();
            }
        });
        if hit.dragged() {
            let delta = ui.input(|i| i.pointer.delta()) / zoom;
            w.view
                .graph_positions
                .insert(node.id.clone(), [old[0] + delta.x, old[1] + delta.y]);
        }
    }
}

fn activate(
    w: &mut Workspace,
    doc: &Document,
    target: &SelectionTarget,
    hit: &egui::Response,
    edit: bool,
) {
    let editor_context =
        (!edit && w.tool_checkpoint.is_some() && matches!(target, SelectionTarget::Object { .. }))
            .then(|| (w.view.active_node.clone(), w.view.active_instance.clone()));
    match target {
        SelectionTarget::Feature { .. } if edit => selection_ui::execute(w, target, "feature.edit"),
        SelectionTarget::Feature {
            definition,
            feature,
        } => {
            if let Some(feature) = doc
                .parts
                .get(definition)
                .and_then(|part| part.features.iter().find(|f| f.id == *feature))
            {
                selection_ui::feature_selected(w, definition, feature, hit);
            }
        }
        SelectionTarget::Object { role, .. } if role == "component-instance" => {
            match selection_ui::component_path(w, target) {
                Ok(path) if edit => crate::assembly_tree_ui::edit_selected_part(w, path),
                Ok(path) => crate::assembly_tree_ui::select(w, path),
                Err(error) => w.status = error.to_string(),
            }
        }
        _ if edit => {
            let command = if matches!(target, SelectionTarget::Object {role, ..} if role == "assembly")
            {
                "assembly.edit"
            } else {
                "object.edit"
            };
            selection_ui::execute(w, target, command);
        }
        _ => selection_ui::object_selected(w, target.clone()),
    }
    if let Some((node, instance)) = editor_context {
        w.view.active_node = node;
        w.view.active_instance = instance;
    }
}

fn caption(doc: &Document, node: &SemanticElement, target: &SelectionTarget) -> (String, String) {
    match node.role.as_str() {
        "feature-station" => {
            if let Some(feature) = doc
                .parts
                .get(target.definition())
                .and_then(|p| p.features.iter().find(|f| f.id == node.id))
            {
                let kind = feature.operation.name();
                let summary = cad_ui::operation_summary(&feature.operation);
                let detail = if summary == kind
                    && node.label.to_lowercase().contains(&kind.to_lowercase())
                {
                    String::new()
                } else {
                    summary
                };
                let icon = match &feature.operation {
                    cad_core::Operation::Extrude { mode, .. } if mode == "cut" => {
                        "feature.cut".into()
                    }
                    cad_core::Operation::Sketch3d { .. } => "sketch3d.new".into(),
                    _ => format!("feature.{}", kind.to_lowercase().replace(' ', "-")),
                };
                return (icon, detail);
            }
            ("feature.edit".into(), String::new())
        }
        "parameter-field" => (
            "sketch.smart-dimension".into(),
            node.value.clone().unwrap_or_default(),
        ),
        "assembly-mate" => (
            "assembly.mate".into(),
            node.value.clone().unwrap_or_default(),
        ),
        "component-instance" => {
            let detail = node
                .value
                .as_ref()
                .filter(|value| !node.label.starts_with(value.as_str()))
                .cloned()
                .unwrap_or_default();
            ("assembly.component".into(), detail)
        }
        _ => ("assembly".into(), String::new()),
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "The native graph painter uses the positioned semantic card and its current interaction state"
)]
fn draw_card(
    ui: &mut egui::Ui,
    painter: &egui::Painter,
    rect: egui::Rect,
    node: &SemanticElement,
    props: &BTreeMap<String, String>,
    icon: &str,
    detail: &str,
    zoom: f32,
    active: bool,
    emphasized: bool,
) {
    let frame = controls::frame_from_properties(ui, props);
    let visuals = ui.visuals();
    let fill = if props.contains_key("background") {
        frame.fill
    } else if active {
        visuals.selection.bg_fill
    } else {
        visuals.panel_fill
    };
    let accent = visuals.selection.stroke.color;
    let stroke = if props
        .keys()
        .any(|k| k == "border" || k.starts_with("border-") && k != "border-radius")
    {
        frame.stroke
    } else {
        Stroke::new(
            if active || emphasized { 2. } else { 1. },
            if active || emphasized {
                accent
            } else {
                visuals.widgets.noninteractive.bg_stroke.color
            },
        )
    };
    let radius = controls::number(props, "border-radius", 4.) * zoom;
    painter.rect(rect, radius, fill, stroke, egui::StrokeKind::Inside);
    let ink = props
        .get("color")
        .and_then(|s| ui::color(s))
        .unwrap_or_else(|| visuals.text_color());
    let inside = painter.with_clip_rect(rect.intersect(painter.clip_rect()).shrink(zoom));
    let padding = controls::number(props, "padding", 8.).max(0.) * zoom;
    let icon_size = controls::number(props, "icon-size", 16.).max(0.) * zoom;
    command_icons::paint(
        &inside,
        egui::pos2(rect.left() + padding + icon_size / 2., rect.center().y),
        icon_size,
        ink,
        icon,
    );
    let x = rect.left() + padding + icon_size + controls::number(props, "gap", 5.).max(0.) * zoom;
    let width = (rect.right() - padding - x).max(0.);
    let font = controls::number(props, "font-size", 12.).max(1.) * zoom;
    let family = if props.get("font-family").is_some_and(|f| f.contains("mono")) {
        egui::FontFamily::Monospace
    } else {
        egui::TextStyle::Body.resolve(ui.style()).family
    };
    let line = |text: &str, size: f32, color| {
        let mut job = egui::text::LayoutJob::single_section(
            text.into(),
            egui::TextFormat {
                font_id: FontId::new(size, family.clone()),
                color,
                ..Default::default()
            },
        );
        job.wrap.max_width = width;
        job.wrap.max_rows = 1;
        job.wrap.break_anywhere = true;
        job.break_on_newline = false;
        inside.layout_job(job)
    };
    let title = line(&node.label, font, ink);
    let detail_color = if active {
        ink
    } else {
        visuals.weak_text_color()
    };
    let secondary = (!detail.is_empty())
        .then(|| line(detail, font * 0.9, detail_color))
        .filter(|g| title.size().y + g.size().y < rect.height());
    let y = rect.center().y - (title.size().y + secondary.as_ref().map_or(0., |g| g.size().y)) / 2.;
    let next_y = y + title.size().y;
    inside.galley(egui::pos2(x, y), title, ink);
    if let Some(secondary) = secondary {
        inside.galley(egui::pos2(x, next_y), secondary, detail_color);
    }
}

fn object_target(
    doc: &cad_core::Document,
    node: &SemanticElement,
) -> Option<cad_core::selection_actions::SelectionTarget> {
    if node.role == "feature-station" {
        return serde_json::from_value(node.data.get("target")?.clone()).ok();
    }
    let (definition, object) = match node.role.as_str() {
        "parameter-field" => {
            let (definition, parameter) = node.id.rsplit_once("::")?;
            (definition.to_string(), parameter.to_string())
        }
        "assembly-mate" => (
            doc.assemblies
                .values()
                .find(|a| a.mates.iter().any(|m| m.id == node.id))?
                .id
                .clone(),
            node.id.clone(),
        ),
        "component-instance" => (
            doc.assemblies
                .values()
                .find_map(|a| a.instances.get(&node.id))?
                .definition
                .clone(),
            node.id.clone(),
        ),
        "assembly" => (node.id.clone(), node.id.clone()),
        _ => return None,
    };
    Some(cad_core::selection_actions::SelectionTarget::Object {
        definition,
        object,
        role: node.role.clone(),
    })
}
