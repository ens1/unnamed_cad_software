//! Dependency cards over application-owned semantic elements. Graph layout is presentation data.
use crate::{Workspace, cad_ui, ui};
use bevy_egui::egui::{self, FontId, Stroke};
use cad_core::{Feature, Part, presentation::SemanticElement};
use std::collections::{BTreeMap, BTreeSet};

pub fn show(ui: &mut egui::Ui, w: &mut Workspace, part: Option<&Part>) {
    let doc = w.session.visible().clone();
    ui.horizontal(|ui| {
        ui.strong("Relationships");
        egui::ComboBox::from_id_salt("graph-scope")
            .selected_text(if w.view.graph_scope == "Assembly" {
                "Assembly"
            } else {
                "Features"
            })
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut w.view.graph_scope, "Features".into(), "Features");
                ui.selectable_value(&mut w.view.graph_scope, "Assembly".into(), "Assembly");
            });
        if ui.small_button("Arrange").clicked() {
            w.view.graph_positions.clear();
            w.view.graph_pan = [0.; 2];
            w.view.graph_zoom = 1.;
        }
    });
    let all = cad_core::presentation::project(&w.session, None);
    let assembly_scope = w.view.graph_scope == "Assembly" || part.is_none();
    let mut nodes: Vec<SemanticElement>;
    if assembly_scope {
        let assembly = w
            .fields
            .get("assembly-id")
            .and_then(|id| doc.assemblies.get(id))
            .or_else(|| doc.assemblies.get(&doc.root));
        let Some(assembly) = assembly else {
            ui.small("This is a part document. Add components in an assembly to show mate relationships.");
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
        ui.small("Create sketches and features to build the dependency graph.");
        return;
    }
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), ui.available_height().max(80.)),
        egui::Sense::click_and_drag(),
    );
    let painter = ui.painter().with_clip_rect(rect);
    let accent = ui.visuals().selection.stroke.color;
    let wide = rect.width() > 450.;
    let card = if wide {
        egui::vec2(146., 92.)
    } else {
        egui::vec2((rect.width() - 30.).max(100.), 72.)
    };
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
    let positions = nodes
        .iter()
        .enumerate()
        .map(|(index, node)| {
            let default = if wide {
                [18. + index as f32 * (card.x + 38.), 24.]
            } else {
                [14., 18. + index as f32 * (card.y + 26.)]
            };
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
    for node in &nodes {
        let end = positions[&node.id].1;
        for input in &node.relationships {
            if let Some((_, start)) = positions.get(input) {
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
    for (index, node) in nodes.iter().enumerate() {
        let (old, card_rect) = positions[&node.id];
        let active = w.view.active_node.as_ref() == Some(&node.id)
            || w.view.active_instance.as_ref() == Some(&node.id);
        let hit = ui.interact(
            card_rect,
            ui.id().with((&node.id, "graph-card")),
            egui::Sense::click_and_drag(),
        );
        painter.rect_filled(card_rect, 4., ui.visuals().panel_fill);
        painter.rect_stroke(
            card_rect,
            4.,
            Stroke::new(
                if active { 2. } else { 1. },
                if active {
                    accent
                } else {
                    ui.visuals().widgets.noninteractive.bg_stroke.color
                },
            ),
            egui::StrokeKind::Inside,
        );
        let header =
            egui::Rect::from_min_size(card_rect.min, egui::vec2(card_rect.width(), 26. * zoom));
        painter.rect_filled(
            header,
            4.,
            if active {
                accent.gamma_multiply(0.18)
            } else {
                ui.visuals().faint_bg_color
            },
        );
        painter.text(
            card_rect.min + egui::vec2(8., 12.) * zoom,
            egui::Align2::LEFT_CENTER,
            format!("{:02}  {}", index + 1, node.label),
            FontId::proportional(12. * zoom),
            ui.visuals().text_color(),
        );
        let subtitle = if node.role == "feature-station" {
            serde_json::from_value::<Feature>(node.data.clone())
                .map(|f| cad_ui::operation_summary(&f.operation))
                .unwrap_or_default()
        } else {
            node.value
                .clone()
                .unwrap_or_else(|| node.role.replace('-', " "))
        };
        painter.text(
            card_rect.min + egui::vec2(8., 38.) * zoom,
            egui::Align2::LEFT_TOP,
            subtitle,
            FontId::proportional(10. * zoom),
            ui.visuals().weak_text_color(),
        );
        for port in if wide {
            [card_rect.left_center(), card_rect.right_center()]
        } else {
            [card_rect.center_top(), card_rect.center_bottom()]
        } {
            painter.circle(
                port,
                3.5 * zoom,
                ui.visuals().panel_fill,
                Stroke::new(1.4, accent),
            );
        }
        if hit.clicked() {
            if node.role == "feature-station" {
                if let Some(part) = doc
                    .parts
                    .values()
                    .find(|p| p.features.iter().any(|f| f.id == node.id))
                    && let Some(feature) = part.features.iter().find(|f| f.id == node.id)
                {
                    crate::selection_ui::feature_selected(w, &part.id, feature, &hit);
                }
            } else if node.role == "component-instance"
                && let Some(instance) = doc
                    .assemblies
                    .values()
                    .find_map(|a| a.instances.get(&node.id))
            {
                if w.tool_checkpoint.is_none() {
                    let path = doc
                        .placed_parts()
                        .unwrap_or_default()
                        .into_iter()
                        .find(|placed| placed.path.last() == Some(&instance.id))
                        .map(|placed| placed.path);
                    w.activate_context(Some(instance.definition.clone()), path);
                }
                crate::selection_ui::object_selected(
                    w,
                    cad_core::selection_actions::SelectionTarget::Object {
                        definition: instance.definition.clone(),
                        object: instance.id.clone(),
                        role: node.role.clone(),
                    },
                );
            } else if let Some(target) = object_target(&doc, node) {
                crate::selection_ui::object_selected(w, target);
            }
        }
        if hit.double_clicked() && node.role != "feature-station" {
            edit(w, node);
        }
        hit.context_menu(|ui| {
            if node.role == "feature-station"
                && let Some(part) = doc
                    .parts
                    .values()
                    .find(|p| p.features.iter().any(|f| f.id == node.id))
            {
                crate::selection_ui::contents(
                    ui,
                    w,
                    &cad_core::selection_actions::SelectionTarget::Feature {
                        definition: part.id.clone(),
                        feature: node.id.clone(),
                    },
                );
                return;
            }
            if let Some(target) = object_target(&doc, node) {
                crate::selection_ui::contents(ui, w, &target);
                return;
            }
            if ui.button("Edit").clicked() {
                edit(w, node);
                ui.close();
            }
            if node.role == "component-instance" && ui.button("Pin reference").clicked() {
                if !w.view.pinned.contains(&node.id) {
                    w.view.pinned.push(node.id.clone());
                }
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
fn edit(w: &mut Workspace, node: &SemanticElement) {
    match node.role.as_str() {
        "feature-station" => {
            if let Ok(feature) = serde_json::from_value::<Feature>(node.data.clone()) {
                ui::feature_dialog(w, Some(feature));
            }
        }
        "assembly-mate" => {
            w.json = serde_json::to_string_pretty(&node.data).unwrap_or_default();
            w.fields.insert("mate-editor".into(), "open".into());
            w.open_editor("assembly");
        }
        "component-instance" | "assembly" => w.open_editor("assembly"),
        _ => {
            w.fields.insert("graph-parameters".into(), "open".into());
            w.open_editor("properties");
        }
    }
}

fn object_target(
    doc: &cad_core::Document,
    node: &SemanticElement,
) -> Option<cad_core::selection_actions::SelectionTarget> {
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
