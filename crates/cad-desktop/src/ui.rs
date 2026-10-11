use crate::*;
use bevy_egui::{EguiContexts, egui};
use cad_core::{
    commands::Command,
    persistence::{FileStore, ProjectStore, atomic_write},
    presentation::{Region, SemanticElement},
    version_control::{Git, VersionControl},
    *,
};
use egui::{Color32, RichText, Stroke};
use serde_json::Value;

pub fn workspace_ui(
    mut contexts: EguiContexts,
    mut w: ResMut<Workspace>,
    image: Res<ViewImage>,
    // Native dialogs and accessibility must stay on the platform event thread.
    _main_thread: bevy::ecs::system::NonSendMarker,
) -> bevy::prelude::Result {
    let ctx = contexts.ctx_mut()?;
    ctx.set_zoom_factor(w.view.ui_scale.clamp(0.75, 1.5));
    let logical_size = ctx.content_rect().size();
    w.presentation_resources.logical_size = [logical_size.x, logical_size.y];
    crate::navigation::begin_ui(&mut w, ctx);
    shortcuts(ctx, &mut w);
    crate::presentation_resources::poll(&mut w);
    if !w.layout.layouts.contains_key(&w.view.active_layout) {
        w.view.active_layout = if w.layout.layouts.contains_key("instrument-rail") {
            "instrument-rail".into()
        } else {
            w.layout.layouts.keys().next().cloned().unwrap_or_default()
        };
    }
    let global_target = Region {
        id: "workspace".into(),
        role: "workspace".into(),
        component: None,
        rect: [0.; 4],
        classes: vec![],
        resizable: false,
    }
    .style_target(&w.view.active_layout, "ready");
    let globals = w.theme.properties_for(&global_target);
    let panel = globals
        .get("--panel")
        .and_then(|s| color(s))
        .unwrap_or(Color32::from_rgb(244, 241, 233));
    let mut visuals = if (panel.r() as u16 + panel.g() as u16 + panel.b() as u16) < 384 {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    visuals.panel_fill = panel;
    if let Some(ink) = globals.get("--ink").and_then(|s| color(s)) {
        visuals.override_text_color = Some(ink);
    }
    let accent = globals
        .get("--active")
        .and_then(|s| color(s))
        .unwrap_or(Color32::from_rgb(255, 90, 0));
    visuals.selection.bg_fill = accent.gamma_multiply(0.18);
    visuals.selection.stroke = Stroke::new(1., accent);
    if let Some(warning) = globals.get("--warning").and_then(|s| color(s)) {
        visuals.warn_fg_color = warning;
    }
    if let Some(error) = globals.get("--error").and_then(|s| color(s)) {
        visuals.error_fg_color = error;
    }
    ctx.set_visuals(visuals);

    let size = ctx.content_rect().size();
    crate::presentation_controls::begin_frame(&w.theme, &w.view.active_layout);
    let semantics = crate::presentation_composition::semantics(&w);
    let composition = w.layout.expand_semantics(&semantics)?;
    let regions = composition.regions_presented(
        &w.view.active_layout,
        size.x,
        size.y,
        w.editing_manager(),
        &w.theme,
    )?;
    let registry = presentation_registry();
    crate::presentation_composition::render(
        ctx,
        &mut w,
        &composition,
        &regions,
        &semantics,
        &registry,
        image.texture,
    );
    let resize_regions = regions.clone();
    let settings_open = w.settings.is_some();
    let viewport_center = regions
        .iter()
        .find(|region| region.role == "model-viewport")
        .map(|region| region.rect[0] + region.rect[2] * 0.5)
        .unwrap_or(size.x * 0.5);
    for region in resize_regions
        .into_iter()
        .filter(|r| r.resizable && !settings_open)
    {
        let right_dock = region.rect[0] + region.rect[2] * 0.5 > viewport_center;
        let edge = if right_dock {
            region.rect[0]
        } else {
            region.rect[0] + region.rect[2]
        };
        let r = egui::Rect::from_min_size(
            egui::pos2(edge - 4., region.rect[1]),
            egui::vec2(8., region.rect[3]),
        );
        egui::Area::new(egui::Id::new(format!("resize/{}", region.id)))
            .order(egui::Order::Foreground)
            .fixed_pos(r.min)
            .show(ctx, |ui| {
                let (_, response) = ui.allocate_exact_size(r.size(), egui::Sense::drag());
                response
                    .clone()
                    .on_hover_cursor(egui::CursorIcon::ResizeHorizontal);
                if response.dragged() {
                    let delta =
                        ui.input(|i| i.pointer.delta().x) * if right_dock { -1. } else { 1. };
                    fn resize(
                        node: &mut cad_core::presentation::LayoutNode,
                        id: &str,
                        delta: f32,
                        current_width: f32,
                        editing: bool,
                    ) -> bool {
                        if node.id.as_deref() == Some(id) {
                            let min = node.min_width_logical_px.unwrap_or(100.);
                            let max = node.max_width_logical_px.unwrap_or(f32::MAX).max(min);
                            let width = Some((current_width + delta).clamp(min, max));
                            if editing && let Some(sizing) = &mut node.editing {
                                sizing.width_logical_px = width;
                            } else {
                                node.width_logical_px = width;
                            }
                            return true;
                        }
                        node.children
                            .iter_mut()
                            .any(|n| resize(n, id, delta, current_width, editing))
                    }
                    let active_layout = w.view.active_layout.clone();
                    let editing = w.editing_manager();
                    if let Some(root) = w.layout.layouts.get_mut(&active_layout) {
                        resize(root, &region.id, delta, region.rect[2], editing);
                    }
                }
            });
    }

    crate::selection_ui::show(ctx, &mut w);
    crate::settings_ui::show(ctx, &mut w);
    crate::navigation::finish_ui(&mut w);
    ctx.request_repaint_after(std::time::Duration::from_millis(16));
    Ok(())
}
pub(crate) fn color(s: &str) -> Option<Color32> {
    let input = s.trim();
    if !input.starts_with('#') {
        return match input.to_ascii_lowercase().as_str() {
            "transparent" => Some(Color32::TRANSPARENT),
            "black" => Some(Color32::BLACK),
            "white" => Some(Color32::WHITE),
            "gray" | "grey" => Some(Color32::from_gray(128)),
            "red" => Some(Color32::RED),
            "green" => Some(Color32::from_rgb(0, 128, 0)),
            "blue" => Some(Color32::BLUE),
            "orange" => Some(Color32::from_rgb(255, 165, 0)),
            "yellow" => Some(Color32::YELLOW),
            "cyan" => Some(Color32::from_rgb(0, 255, 255)),
            "magenta" => Some(Color32::from_rgb(255, 0, 255)),
            _ => None,
        };
    }
    let s = input.strip_prefix('#')?;
    let v = u32::from_str_radix(s, 16).ok()?;
    match s.len() {
        3 => Some(Color32::from_rgb(
            ((v >> 8) & 15) as u8 * 17,
            ((v >> 4) & 15) as u8 * 17,
            (v & 15) as u8 * 17,
        )),
        4 => Some(Color32::from_rgba_unmultiplied(
            ((v >> 12) & 15) as u8 * 17,
            ((v >> 8) & 15) as u8 * 17,
            ((v >> 4) & 15) as u8 * 17,
            (v & 15) as u8 * 17,
        )),
        6 => Some(Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)),
        8 => Some(Color32::from_rgba_unmultiplied(
            (v >> 24) as u8,
            (v >> 16) as u8,
            (v >> 8) as u8,
            v as u8,
        )),
        _ => None,
    }
}
fn shortcuts(ctx: &egui::Context, w: &mut Workspace) {
    // Recovery stays reachable even when a user theme hides the Settings overlay.
    if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::F9)) {
        if w.settings.is_some() {
            crate::settings::recover_presentation(w);
        } else {
            crate::presentation_resources::reset(w);
            crate::navigation::action(w, "navigation.reset", None);
            w.view.roll = 0.;
            w.status = "Restored default presentation and navigation".into();
        }
        return;
    }
    if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::Comma)) {
        crate::settings::open(w, None);
    }
    if w.settings.is_some() {
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
            crate::settings::cancel(w);
        } else if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::Enter)) {
            crate::settings::apply(w, true);
        }
        return;
    }
    if ctx.input(|i| i.key_pressed(egui::Key::F5)) {
        action(w, "document.rebuild");
    }
    let shortcuts = ctx.input(|i| {
        (
            i.key_pressed(egui::Key::Escape),
            i.key_pressed(egui::Key::Enter) && i.modifiers.command,
            i.key_pressed(egui::Key::S) && i.modifiers.command,
            i.key_pressed(egui::Key::Z) && i.modifiers.command,
            i.modifiers.shift,
            i.key_pressed(egui::Key::K) && i.modifiers.command,
            i.key_pressed(egui::Key::F),
            i.modifiers.alt,
        )
    });
    if shortcuts.0 {
        crate::navigation::stop(w);
        let canceling_drag = w
            .sketch
            .as_ref()
            .is_some_and(|s| s.drag.is_some() || s.edit.is_some());
        if w.selection_popup.take().is_some() {
            ctx.input_mut(|i| {
                i.consume_key(egui::Modifiers::NONE, egui::Key::Escape);
            });
            if w.dialog.is_none() {
                crate::history::latest(w);
            }
        } else if crate::sketch::escape(w) {
            if !canceling_drag {
                w.status = "Sketch tool stopped. Select another tool or Exit Sketch.".into();
            }
        } else if w.view.history_cursor.is_some() && w.dialog.is_none() {
            crate::history::latest(w);
        } else if w.tool_checkpoint.is_some() {
            w.cancel_tool();
        } else if w.dialog.is_some() {
            w.dialog = None;
            w.manager_tree = true;
        } else {
            w.cancel();
        }
    }
    if shortcuts.1 {
        w.confirm_tool();
    }
    if shortcuts.2 {
        save(w, false);
    }
    if shortcuts.3 && !ctx.egui_wants_keyboard_input() {
        w.selection_popup = None;
        if w.sketch.is_some() {
            crate::sketch::undo_point(w);
        } else if w.tool_checkpoint.is_some() {
            w.cancel_tool();
        } else if shortcuts.4 {
            w.session.redo();
        } else {
            w.session.undo();
        }
        w.fields.retain(|key, _| {
            !key.starts_with("drawing-editor:") && key != "drawing-source-invalid"
        });
        crate::exploded_views_ui::clear_selection(w);
        w.requested = None;
        w.changed = Instant::now();
    }
    if ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::N)) {
        action(w, "file.new-part");
    }
    if ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::O)) {
        action(w, "file.open");
    }
    if !ctx.egui_wants_keyboard_input() && w.sketch.is_some() {
        if ctx.input(|i| i.key_pressed(egui::Key::D) && !i.modifiers.command) {
            action(w, "sketch.smart-dimension");
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace)) {
            action(w, "sketch.delete-selection");
        }
    }
    if shortcuts.5 {
        w.open_editor("commands");
    }
    if shortcuts.6 && !ctx.egui_wants_keyboard_input() {
        viewport::fit(w, shortcuts.4);
    }
    w.peek = shortcuts.7;
}
fn context(ui: &mut egui::Ui, w: &mut Workspace) {
    crate::cad_ui::menu(ui, w);
}
fn navigation(ui: &mut egui::Ui, w: &mut Workspace) {
    crate::cad_ui::navigation(ui, w);
}
fn dependencies(ui: &mut egui::Ui, w: &mut Workspace) {
    crate::cad_ui::tree(ui, w);
}
pub(crate) fn graph(ui: &mut egui::Ui, w: &mut Workspace, part: &Part) {
    crate::graph_ui::show(ui, w, Some(part));
}
pub(crate) fn parameters(ui: &mut egui::Ui, w: &mut Workspace) {
    let doc = w.session.visible().clone();
    let owner = w
        .view
        .active_part
        .clone()
        .unwrap_or_else(|| doc.root.clone());
    let params = doc
        .parts
        .get(&owner)
        .map(|p| &p.parameters)
        .or_else(|| doc.assemblies.get(&owner).map(|a| &a.parameters));
    ui.horizontal(|ui| {
        ui.label(RichText::new("PARAMETERS").strong().size(11.));
        if ui.small_button("+").clicked() {
            w.open_editor("parameter");
            w.fields.insert("parameter-name".into(), "dimension".into());
            w.fields.insert("parameter-value".into(), "10 mm".into());
        }
    });
    egui::ScrollArea::vertical()
        .id_salt("instruments")
        .show(ui, |ui| {
            if let Some(target) = w.fields.get("editing-parameter").cloned()
                && let Some((definition, key)) = target.rsplit_once("::")
                && definition != owner
                && let Some(value) = doc
                    .parts
                    .get(definition)
                    .map(|p| &p.parameters)
                    .or_else(|| doc.assemblies.get(definition).map(|a| &a.parameters))
                    .and_then(|parameters| parameters.get(key))
            {
                ui.small(format!(
                    "Referenced parameter · {}",
                    cad_core::presentation::definition_name(&doc, definition)
                ));
                parameter(ui, w, definition, key, value);
                ui.separator();
            }
            if let Some(assembly) = doc.assemblies.get(&owner) {
                for (alias, reference) in &assembly.promoted_parameters {
                    if let Some(p) = doc
                        .parts
                        .get(&reference.definition)
                        .and_then(|p| p.parameters.get(&reference.parameter))
                    {
                        ui.small(format!("Promoted: {alias}"));
                        parameter(ui, w, &reference.definition, &reference.parameter, p);
                    }
                }
            }
            if let Some(params) = params {
                for (id, p) in params {
                    parameter(ui, w, &owner, id, p);
                    ui.add_space(6.);
                }
            }
            if let Some(part) = doc.parts.get(&owner)
                && let Some(f) = w
                    .view
                    .active_node
                    .as_ref()
                    .and_then(|id| part.features.iter().find(|f| &f.id == id))
            {
                ui.separator();
                ui.label(RichText::new(&f.name).strong());
                let mut json = serde_json::to_value(&f.operation).unwrap_or(Value::Null);
                if edit_value(ui, &mut json, "operation") {
                    match serde_json::from_value(json) {
                        Ok(operation) => w.dispatch(Command::PutFeature {
                            definition: owner.clone(),
                            feature: Feature {
                                operation,
                                ..f.clone()
                            }
                            .into(),
                            output: false,
                        }),
                        Err(e) => w.status = e.to_string(),
                    }
                }
                let suppressed =
                    cad_core::configuration::feature_suppressed(w.session.visible(), &owner, f);
                ui.horizontal(|ui| {
                    if ui.small_button("Source").clicked() {
                        feature_dialog(w, Some(f.clone()));
                    }
                    if ui
                        .small_button(if suppressed { "Resume" } else { "Suppress" })
                        .clicked()
                    {
                        w.dispatch(Command::SuppressFeature {
                            definition: owner.clone(),
                            feature: f.id.clone(),
                            suppressed: !suppressed,
                        });
                    }
                    if ui.small_button("Delete").clicked() {
                        w.dispatch(Command::DeleteFeature {
                            definition: owner.clone(),
                            feature: f.id.clone(),
                        });
                    }
                });
            }
        });
}
fn parameter(ui: &mut egui::Ui, w: &mut Workspace, owner: &str, id: &str, p: &Parameter) {
    let key = format!("{owner}::{id}");
    let label = ui.add(egui::Label::new(&p.name).sense(egui::Sense::click()));
    if label.clicked() {
        crate::selection_ui::object_selected(
            w,
            cad_core::selection_actions::SelectionTarget::Object {
                definition: owner.into(),
                object: id.into(),
                role: "parameter-field".into(),
            },
        );
    }
    let focus = w
        .fields
        .get("focus-parameter")
        .is_some_and(|target| target == &key);
    if focus {
        w.fields.remove("focus-parameter");
    }
    let value = w
        .fields
        .entry(key.clone())
        .or_insert_with(|| p.expression.0.clone());
    let response = ui.add(
        egui::TextEdit::singleline(value)
            .id_salt(("parameter-value", &key))
            .font(egui::TextStyle::Monospace)
            .desired_width(ui.available_width()),
    );
    if focus {
        response.request_focus();
    }
    if !response.has_focus() && !response.changed() {
        *value = p.expression.0.clone();
    }
    if response.changed() {
        let expression = value.clone();
        w.dispatch(Command::SetParameter {
            definition: owner.into(),
            parameter: id.into(),
            value: Expr(expression),
        });
    }
    if ui.small_button("Promote to assembly control").clicked() {
        let mut document = w.session.visible().clone();
        if let Some(assembly) = document.assemblies.get_mut(&document.root) {
            assembly.promoted_parameters.insert(
                p.name.clone(),
                ParameterRef {
                    definition: owner.into(),
                    parameter: id.into(),
                },
            );
            w.dispatch(Command::ReplaceDocument { document });
        }
    }
    let result =
        cad_core::expression::value(w.session.visible(), owner, &p.expression, p.dimension);
    match result {
        Ok(v) => {
            let (display, unit) = match p.dimension {
                Dimension::Length => (v, "mm"),
                Dimension::Angle => (v.to_degrees(), "deg"),
                _ => (v, ""),
            };
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("{display:.3}")).size(23.));
                ui.small(unit);
            });
        }
        Err(e) => {
            ui.colored_label(Color32::from_rgb(180, 45, 30), e.to_string());
        }
    }
}
fn edit_value(ui: &mut egui::Ui, v: &mut Value, path: &str) -> bool {
    let mut changed = false;
    match v {
        Value::Object(o) => {
            for (k, value) in o {
                if k == "kind" {
                    ui.small(format!("Type: {value}"));
                    continue;
                }
                ui.push_id(format!("{path}/{k}"), |ui| {
                    if value.is_array() || value.is_object() {
                        egui::CollapsingHeader::new(k)
                            .default_open(false)
                            .show(ui, |ui| {
                                changed |= edit_value(ui, value, &format!("{path}/{k}"))
                            });
                    } else {
                        ui.horizontal(|ui| {
                            ui.small(k);
                            changed |= edit_value(ui, value, &format!("{path}/{k}"));
                        });
                    }
                });
            }
        }
        Value::Array(a) => {
            for (index, v) in a.iter_mut().enumerate() {
                ui.push_id(index, |ui| {
                    changed |= edit_value(ui, v, &format!("{path}/{index}"))
                });
            }
        }
        Value::String(s) => {
            changed |= ui
                .add(egui::TextEdit::singleline(s).desired_width(150.))
                .changed();
        }
        Value::Number(n) => {
            if n.is_u64() {
                let mut x = n.as_u64().unwrap_or(0);
                if ui.add(egui::DragValue::new(&mut x).speed(1)).changed() {
                    *n = x.into();
                    changed = true;
                }
            } else {
                let mut x = n.as_f64().unwrap_or(0.);
                if ui.add(egui::DragValue::new(&mut x).speed(0.1)).changed()
                    && let Some(num) = serde_json::Number::from_f64(x)
                {
                    *n = num;
                    changed = true;
                }
            }
        }
        Value::Bool(b) => changed = ui.checkbox(b, "").changed(),
        Value::Null => {
            ui.small("None");
        }
    }
    changed
}
fn preview_status(ui: &mut egui::Ui, w: &Workspace) {
    let (label, state) = crate::workspace_controls::validation_summary(w);
    crate::presentation_controls::label(
        ui,
        "workspace.validation-state",
        "validation-summary",
        &[],
        label,
        state,
    );
    crate::presentation_controls::label(
        ui,
        "workspace.status-message",
        "status-message",
        &[],
        &w.status,
        state,
    )
    .on_hover_text(&w.status);
    if let Some((state, label)) = crate::operation_preview::summary(w) {
        crate::presentation_controls::label(
            ui,
            "workspace.operation-preview",
            "operation-preview-status",
            &[],
            &label,
            &state,
        )
        .on_hover_text(crate::operation_preview::tooltip(w));
    }
}
fn presentation_registry() -> crate::presentation_composition::Registry {
    let mut registry = crate::presentation_composition::Registry::default();
    registry.register("document-context", |ui, w, _| context(ui, w));
    registry.register("command-manager", |ui, w, _| crate::cad_ui::commands(ui, w));
    registry.register("status-bar", |ui, w, _| crate::cad_ui::status(ui, w));
    registry.register("view-navigation", |ui, w, _| navigation(ui, w));
    registry.register("dependency-view", |ui, w, _| dependencies(ui, w));
    registry.register("parameter-instrument", |ui, w, _| property_manager(ui, w));
    registry.register("property-manager", |ui, w, _| property_manager(ui, w));
    registry.register("preview-status", |ui, w, _| preview_status(ui, w));
    registry.register("apply-action", |ui, w, context| {
        semantic_command(
            ui,
            w,
            context.region,
            "draft.apply",
            context
                .semantics
                .iter()
                .find(|e| e.id == "command:draft.apply"),
        )
    });
    registry.register("cancel-action", |ui, w, context| {
        semantic_command(
            ui,
            w,
            context.region,
            "draft.cancel",
            context
                .semantics
                .iter()
                .find(|e| e.id == "command:draft.cancel"),
        )
    });
    registry.register("model-viewport", |ui, w, context| {
        model_view(
            ui,
            w,
            context.texture,
            ui.max_rect(),
            context.viewport_index,
        )
    });
    registry.register("assembly-editor", |ui, w, _| assembly_editor(ui, w));
    registry.register("materials-editor", |ui, w, _| materials(ui, w));
    registry.register("appearances-editor", |ui, w, _| {
        crate::appearances_ui::show(ui, w)
    });
    registry.register("appearance-panel", |ui, w, _| {
        crate::appearances_ui::panel(ui, w)
    });
    registry.register("lighting-editor", |ui, w, _| {
        crate::appearances_ui::lighting_view(ui, w)
    });
    registry.register("drawings-editor", |ui, w, _| drawings(ui, w));
    registry.register("manufacturing-editor", |ui, w, _| manufacturing(ui, w));
    registry.register("versions-editor", |ui, w, _| versions(ui, w));
    registry.register("review-editor", |ui, w, _| comments(ui, w));
    registry.register("bom-view", |ui, w, _| bom(ui, w));
    registry.register("presentation-editor", |ui, w, _| presentation_editor(ui, w));
    registry.register("selection-actions", |ui, w, _| {
        if let Some(target) = crate::selection_ui::current(w) {
            crate::selection_ui::contents(ui, w, &target);
        } else {
            ui.weak("Select an edge, face or feature");
        }
    });
    registry.register("sketch-editor", |ui, w, _| crate::sketch::controls(ui, w));
    registry.register("viewport-toolbar", |ui, w, _| {
        crate::workspace_controls::toolbar(ui, w)
    });
    registry.register("viewport-start-hint", |ui, w, _| {
        crate::workspace_controls::start_hint(ui, w)
    });
    registry.register("isolation-controls", |ui, w, _| {
        crate::workspace_controls::isolation(ui, w)
    });
    registry.register("navigation-hint", |ui, w, _| {
        crate::workspace_controls::navigation_hint(ui, w)
    });
    registry.register("selection-candidates", |ui, w, _| {
        crate::workspace_controls::candidates(ui, w)
    });
    registry.register("view-cube", |ui, w, _| {
        viewport::view_cube_at(w, ui, ui.max_rect(), ui.clip_rect());
    });
    registry.register("reference-repair", |ui, w, _| {
        crate::reference_repair_ui::show(ui, w)
    });
    registry.register("sketch-feedback", |ui, w, _| {
        crate::sketch_relations_ui::panel(ui, w)
    });
    registry.register("library-editor", |ui, w, _| library_editor(ui, w));
    registry
}

pub(crate) fn semantic_command(
    ui: &mut egui::Ui,
    w: &mut Workspace,
    region: &Region,
    command: &str,
    descriptor: Option<&SemanticElement>,
) {
    let enabled = descriptor.is_none_or(|element| element.state != "disabled");
    let label = descriptor
        .map(|element| element.label.as_str())
        .unwrap_or(command);
    let role = match command {
        "draft.apply" => "apply-action",
        "draft.cancel" => "cancel-action",
        _ => "command-action",
    };
    let response = crate::presentation_controls::button(
        ui,
        &region.id,
        role,
        &["semantic-control"],
        label,
        enabled,
        false,
    )
    .on_hover_text(
        descriptor
            .and_then(|element| element.value.as_deref())
            .unwrap_or(label),
    )
    .on_disabled_hover_text(
        descriptor
            .and_then(|element| element.value.as_deref())
            .unwrap_or(label),
    );
    if response.clicked() {
        action(w, command);
    }
}

pub(crate) fn semantic_element(
    ui: &mut egui::Ui,
    w: &mut Workspace,
    _region: &Region,
    binding: &cad_core::presentation::BindingDescriptor,
    element: &SemanticElement,
) {
    let widget = binding.widget.as_deref().unwrap_or("auto");
    if element.role == "menu" {
        crate::cad_ui::semantic_menu(ui, w, element);
        return;
    }
    if widget == "label" || widget == "value" {
        ui.label(if widget == "value" {
            element.value.as_deref().unwrap_or(&element.label)
        } else {
            binding.label.as_deref().unwrap_or(&element.label)
        });
        return;
    }
    if element.role == "navigation-preference" {
        crate::navigation::semantic_control(ui, w, element);
        return;
    }
    if element.role == "settings-field" {
        crate::settings_ui::semantic_control(ui, w, element);
        return;
    }
    if element.role == "parameter-field"
        && let Some((owner, key)) = element.id.rsplit_once("::")
    {
        let value = w
            .session
            .visible()
            .parts
            .get(owner)
            .and_then(|part| part.parameters.get(key))
            .or_else(|| {
                w.session
                    .visible()
                    .assemblies
                    .get(owner)
                    .and_then(|assembly| assembly.parameters.get(key))
            })
            .cloned();
        if let Some(value) = value {
            parameter(ui, w, owner, key, &value);
            return;
        }
    }
    if element.data.get("pointer").is_some() || element.data.get("setter").is_some() {
        semantic_field(ui, w, element);
        return;
    }
    if widget != "action" {
        ui.label(binding.label.as_deref().unwrap_or(&element.label));
    }
    if widget != "action"
        && let Some(value) = &element.value
    {
        ui.small(value);
    }
    let mut classes = element
        .data
        .get("classes")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>();
    classes.push("semantic-control");
    for descriptor in element.action_descriptors() {
        let command = &descriptor.command;
        let label = cad_core::presentation::command_catalog()
            .into_iter()
            .find(|(id, _)| *id == command)
            .map(|(_, label)| label)
            .unwrap_or(command);
        let label = if widget == "action" || element.role == "selection-action" {
            binding.label.as_deref().unwrap_or(&element.label)
        } else {
            label
        };
        let response = crate::presentation_controls::button(
            ui,
            &element.id,
            &element.role,
            &classes,
            label,
            descriptor.enabled,
            element.state == "selected",
        )
        .on_disabled_hover_text(descriptor.disabled_reason.as_deref().unwrap_or(label));
        if response.clicked() {
            let mut target = element.clone();
            target.data = descriptor.arguments;
            semantic_action(w, &target, command);
        }
        if widget == "action" {
            break;
        }
    }
}
fn semantic_action(w: &mut Workspace, element: &SemanticElement, command: &str) {
    if crate::settings::action(w, command) {
        return;
    }
    if w.settings.is_some() {
        w.status = "Apply or cancel Settings before using another command".into();
        return;
    }
    if crate::reference_repair_ui::action(w, command, Some(&element.data))
        || crate::sketch_relations_ui::semantic_action(w, element, command)
    {
        return;
    }
    if reference_repair_blocks(w, command) {
        return;
    }
    if crate::appearances_ui::action(w, command, Some(&element.data)) {
        return;
    }
    if crate::navigation::action(w, command, Some(&element.data)) {
        return;
    }
    if command == "history.inspect" {
        if let Some(target) = element
            .data
            .get("target")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
        {
            crate::history::inspect(w, target);
        }
        return;
    }
    if element.role == "selection-action" {
        if let Some(target) = element
            .data
            .get("target")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
        {
            crate::selection_ui::execute(w, &target, command);
        }
        return;
    }
    if command.starts_with("feature.")
        && let Some(target) = element.data.get("target").and_then(|value| {
            serde_json::from_value::<cad_core::selection_actions::SelectionTarget>(value.clone())
                .ok()
        })
        && matches!(
            &target,
            cad_core::selection_actions::SelectionTarget::Feature { .. }
        )
    {
        crate::selection_ui::execute(w, &target, command);
        return;
    }
    if matches!(
        command,
        "sketch.edit-member"
            | "sketch.select-entity"
            | "sketch.edit-constraint"
            | "sketch.delete-constraint"
            | "sketch.select-handle"
    ) || (command.starts_with("sketch.")
        && matches!(
            element.role.as_str(),
            "sketch-member"
                | "sketch-modifier"
                | "sketch-instance"
                | "sketch-dimension-group"
                | "sketch-equation"
                | "sketch-equation-field"
        ))
    {
        crate::sketch_ui::semantic_action(w, element, command);
        return;
    }

    if command.starts_with("drawing.") && crate::drawing_ui::semantic_action(w, element, command) {
        return;
    }
    if command.starts_with("assembly.inspection-")
        && crate::assembly_inspection_ui::action(w, command, Some(&element.data))
    {
        return;
    }
    if matches!(element.role.as_str(), "exploded-view" | "explode-step")
        && command.starts_with("assembly.exploded-")
    {
        crate::exploded_views_ui::semantic_action(w, element, command);
        return;
    }
    if element.role == "component-pattern"
        && matches!(command, "assembly.pattern" | "assembly.edit")
    {
        let pattern = w.session.visible().assemblies.values().find_map(|a| {
            a.component_patterns
                .get(&element.id)
                .map(|p| (a.id.clone(), p.clone()))
        });
        if let Some((owner, pattern)) = pattern {
            crate::component_pattern_ui::begin(w, &owner, pattern);
        }
        return;
    }
    if matches!(
        element.role.as_str(),
        "configuration" | "configuration-table"
    ) && matches!(command, "configurations" | "equations")
    {
        if let Some(owner) = element.data.get("owner").and_then(|v| v.as_str()) {
            let selected = element
                .data
                .get("source")
                .and_then(|v| v.get("id"))
                .and_then(|v| v.as_str());
            crate::configuration_ui::open_owner(
                w,
                owner,
                selected,
                if command == "equations" {
                    "equations"
                } else {
                    "configuration"
                },
            );
        }
        return;
    }
    if command.starts_with("feature.")
        && let Ok(feature) = serde_json::from_value::<Feature>(element.data.clone())
    {
        if let Some(owner) = w
            .session
            .visible()
            .parts
            .values()
            .find(|p| p.features.iter().any(|f| f.id == feature.id))
            .map(|p| p.id.clone())
        {
            let target = cad_core::selection_actions::SelectionTarget::Feature {
                definition: owner,
                feature: feature.id,
            };
            crate::selection_ui::execute(w, &target, command);
        }
    } else {
        action(w, command);
    }
}
pub(crate) fn semantic_field(ui: &mut egui::Ui, w: &mut Workspace, element: &SemanticElement) {
    let descriptor = element.value_descriptor();
    let value = crate::presentation_composition::current_value(w, element);
    let editable = descriptor.editable;
    let role = if element
        .data
        .get("setter")
        .and_then(Value::as_str)
        .is_some_and(|setter| setter.starts_with("appearance-"))
    {
        element.role.as_str()
    } else {
        "field-control"
    };
    let props = crate::presentation_controls::properties(
        &element.id,
        role,
        &["semantic-control", "field-control"],
        if editable { "editable" } else { "readonly" },
    );
    if props.get("display").is_some_and(|value| value == "none") {
        return;
    }
    let mut appearance_error = None;
    let next = ui
        .scope(|ui| {
            crate::presentation_controls::apply_properties(ui, &props);
            ui.label(&element.label)
                .on_hover_text(&descriptor.accessibility_label);
            if !descriptor.choices.is_empty() {
                let mut next = value.clone();
                let selected_label = descriptor
                    .choices
                    .iter()
                    .find(|choice| choice.get("value").unwrap_or(choice) == &value)
                    .and_then(|choice| choice.get("label"))
                    .and_then(Value::as_str)
                    .map(str::to_owned)
                    .unwrap_or_else(|| {
                        value
                            .as_str()
                            .map(str::to_owned)
                            .unwrap_or_else(|| value.to_string())
                    });
                egui::ComboBox::from_id_salt(&element.id)
                    .selected_text(selected_label)
                    .show_ui(ui, |ui| {
                        ui.add_enabled_ui(editable, |ui| {
                            for choice in &descriptor.choices {
                                let candidate = choice.get("value").unwrap_or(choice);
                                let label = choice
                                    .get("label")
                                    .and_then(Value::as_str)
                                    .or_else(|| candidate.as_str())
                                    .map(str::to_string)
                                    .unwrap_or_else(|| candidate.to_string());
                                ui.selectable_value(&mut next, candidate.clone(), label);
                            }
                        });
                    });
                return (next != value).then_some(next);
            }
            if let Some(mut checked) = value.as_bool() {
                return ui
                    .add_enabled(editable, egui::Checkbox::new(&mut checked, "Enabled"))
                    .changed()
                    .then_some(Value::Bool(checked));
            }
            let current = value
                .as_str()
                .map(str::to_string)
                .unwrap_or_else(|| value.to_string());
            let key = format!("presentation-field:{}", element.id);
            let text = w.fields.entry(key).or_insert(current.clone());
            let editor = if element.data.get("multiline").and_then(Value::as_bool) == Some(true) {
                egui::TextEdit::multiline(text)
                    .code_editor()
                    .desired_rows(12)
            } else {
                egui::TextEdit::singleline(text)
            };
            let response = ui.add_enabled(
                editable,
                editor.id_salt(&element.id).desired_width(f32::INFINITY),
            );
            if !response.has_focus() && !response.changed() {
                *text = current;
                appearance_error = Some(None);
            }
            if response.changed() {
                if value.is_string() {
                    Some(Value::String(text.clone()))
                } else {
                    match serde_json::from_str(text) {
                        Ok(value) => Some(value),
                        Err(error) => {
                            w.status = format!("{}: {error}", element.label);
                            appearance_error = Some(Some(error.to_string()));
                            None
                        }
                    }
                }
            } else {
                None
            }
        })
        .inner;
    if let Some(error) = appearance_error {
        crate::appearances_ui::field_error(w, element, error);
    }
    if let Some(next) = next {
        match crate::presentation_composition::set_value(w, element, next) {
            Ok(()) => crate::appearances_ui::field_error(w, element, None),
            Err(error) => {
                w.status = error.to_string();
                crate::appearances_ui::field_error(w, element, Some(error.to_string()));
            }
        }
    }
}
pub(crate) fn pin_selected(w: &mut Workspace) {
    if let Some(selected) = &w.selected {
        if selected.instance.is_empty() {
            w.status = "Pin references from assembly components".into();
            return;
        }
        if w.view.pinned.contains(&selected.instance) {
            w.view.pinned.retain(|id| id != &selected.instance);
            w.status = "Reference unpinned".into();
        } else {
            w.view.pinned.push(selected.instance.clone());
            w.status = format!("Pinned reference: {}", selected.name);
        }
        w.rendered_key.clear();
    }
}
fn model_view(
    ui: &mut egui::Ui,
    w: &mut Workspace,
    texture: egui::TextureId,
    r: egui::Rect,
    index: usize,
) {
    if index == 0 {
        let scale = ui.ctx().pixels_per_point();
        w.viewport_size = [(r.width() * scale) as u32, (r.height() * scale) as u32];
    }
    let response = ui.put(
        r,
        egui::Image::new(egui::load::SizedTexture::new(texture, r.size()))
            .fit_to_exact_size(r.size())
            .sense(egui::Sense::click_and_drag()),
    );
    if crate::drawing_ui::preview(w, ui, r) {
        return;
    }
    if viewport::navigate(w, ui.ctx(), &response) {
        w.selection_popup = None;
    }
    let toolbar = crate::presentation_controls::overlay_rect_state(
        "viewport.toolbar",
        "viewport-toolbar",
        crate::workspace_controls::toolbar_state(w),
        r,
        egui::Rect::from_min_size(
            r.min + egui::vec2(12., 8.),
            egui::vec2((r.width() - 24.).max(0.), 84.),
        ),
    )
    .unwrap_or(egui::Rect::NOTHING);
    if toolbar.is_positive() {
        let mut tools = ui.new_child(egui::UiBuilder::new().max_rect(toolbar));
        crate::presentation_controls::apply_properties(
            &mut tools,
            &crate::presentation_controls::properties(
                "viewport.toolbar",
                "viewport-toolbar",
                &["viewport-overlay"],
                crate::workspace_controls::toolbar_state(w),
            ),
        );
        crate::workspace_controls::toolbar(&mut tools, w);
    }
    let empty = w.sketch.is_none()
        && w.view
            .active_part
            .as_ref()
            .and_then(|id| w.session.visible().parts.get(id))
            .is_some_and(|p| p.features.is_empty());
    if empty
        && w.active_plane.is_none()
        && let Some(hint_rect) = crate::presentation_controls::overlay_rect(
            "viewport.start-hint",
            "viewport-start-hint",
            r,
            egui::Rect::from_center_size(r.center(), egui::vec2(320., 132.)),
        )
    {
        let mut hint = ui.new_child(egui::UiBuilder::new().max_rect(hint_rect));
        crate::presentation_controls::apply_properties(
            &mut hint,
            &crate::presentation_controls::properties(
                "viewport.start-hint",
                "viewport-start-hint",
                &["viewport-overlay"],
                "ready",
            ),
        );
        crate::workspace_controls::start_hint(&mut hint, w);
    }
    if response.secondary_clicked() && w.sketch.is_none() && w.tool_checkpoint.is_none() {
        w.selection_popup = None;
        if let Some(pointer) = response.interact_pointer_pos() {
            if let Some(feature) = crate::sketch::pick(w, r, pointer) {
                crate::sketch::select_feature(w, feature, &response);
            } else if let Some(selected) = viewport::pick(w, r, pointer).into_iter().next() {
                viewport::select(w, selected);
            } else {
                w.selected = None;
                w.selected_feature_path = None;
                w.view.active_node = None;
                w.context_selection = None;
                w.candidates.clear();
            }
        }
    }
    if w.sketch.is_none() {
        response.context_menu(|ui| {
            if w.tool_checkpoint.is_some() {
                if ui
                    .button(if w.sketch.is_some() {
                        "Exit Sketch"
                    } else {
                        "Confirm Operation"
                    })
                    .clicked()
                {
                    w.confirm_tool();
                    ui.close();
                }
                if ui.button("Cancel Operation").clicked() {
                    w.cancel_tool();
                    ui.close();
                }
            } else if let Some(target) = crate::selection_ui::current(w) {
                crate::selection_ui::contents(ui, w, &target);
            }
            ui.separator();
            if ui.button("Zoom to Fit").clicked() {
                viewport::fit(w, false);
                ui.close();
            }
        });
    }
    viewport::overlay(w, ui, r);
    let cube = viewport::view_cube(w, ui, r);
    crate::sketch::overlay(w, ui, r, &response, toolbar);
    if w.sketch.is_none()
        && response.clicked()
        && response
            .interact_pointer_pos()
            .is_some_and(|p| !toolbar.contains(p) && !cube.contains(p))
    {
        let p = response.interact_pointer_pos().unwrap_or(r.center());
        w.selection_popup = None;
        let sketch_feature = if w.tool_checkpoint.is_none() {
            crate::sketch::pick(w, r, p)
        } else {
            None
        };
        if let Some(feature) = sketch_feature {
            if crate::sketch::select_feature(w, feature, &response) {
                if !response.double_clicked() {
                    crate::selection_ui::open(w, &response, p);
                }
                w.status =
                    "Sketch selected. Choose an action; double-click to edit on the model".into();
            }
        } else {
            w.candidates = viewport::pick(w, r, p);
            w.candidate_index = 0;
            let toggle = ui.input(|input| {
                input.modifiers.command || input.modifiers.ctrl || input.modifiers.shift
            });
            if let Some(s) = w.candidates.first().cloned() {
                if viewport::select_modified(w, s, toggle) {
                    if !crate::reference_repair_ui::capture_selection(w) {
                        capture_feature_selection(w, false, toggle);
                    }
                    if w.tool_checkpoint.is_none() {
                        crate::selection_ui::open(w, &response, p);
                    }
                }
            } else {
                w.selected = None;
                w.geometry_selection.clear();
                w.selected_feature_path = None;
                w.view.active_node = None;
                w.context_selection = None;
            }
        }
    }
    if w.sketch.is_none()
        && response.hovered()
        && ui.input(|i| i.key_pressed(egui::Key::Tab))
        && !w.candidates.is_empty()
    {
        w.candidate_index = (w.candidate_index + 1) % w.candidates.len();
        if viewport::select(w, w.candidates[w.candidate_index].clone()) {
            if !crate::reference_repair_ui::capture_selection(w) {
                capture_feature_selection(w, true, false);
            }
            if w.tool_checkpoint.is_none() {
                crate::selection_ui::open(w, &response, response.hover_pos().unwrap_or(r.center()));
            }
        }
    }
    if w.candidates.len() > 1
        && let Some(area) = crate::presentation_controls::overlay_rect(
            "viewport.candidates",
            "selection-candidates",
            r,
            egui::Rect::from_min_size(
                r.left_bottom() + egui::vec2(16., -54.),
                egui::vec2((r.width() - 32.).max(0.), 24.),
            ),
        )
    {
        let mut controls = ui.new_child(egui::UiBuilder::new().max_rect(area));
        crate::workspace_controls::candidates(&mut controls, w);
    }
    if w.view.isolation
        && w.sketch.is_none()
        && let Some(area) = crate::presentation_controls::overlay_rect(
            "viewport.isolation",
            "isolation-controls",
            r,
            egui::Rect::from_min_size(
                r.left_bottom() + egui::vec2(16., -70.),
                egui::vec2((r.width() - 128.).max(0.), 26.),
            ),
        )
    {
        let mut controls = ui.new_child(egui::UiBuilder::new().max_rect(area));
        crate::presentation_controls::apply_properties(
            &mut controls,
            &crate::presentation_controls::properties(
                "viewport.isolation",
                "isolation-controls",
                &["viewport-overlay"],
                "ready",
            ),
        );
        crate::workspace_controls::isolation(&mut controls, w);
    }
    if let Some(area) = crate::presentation_controls::overlay_rect(
        "viewport.navigation-hint",
        "navigation-hint",
        r,
        egui::Rect::from_min_size(
            r.left_bottom() + egui::vec2(16., -28.),
            egui::vec2((r.width() - 32.).max(0.), 20.),
        ),
    ) {
        let mut controls = ui.new_child(egui::UiBuilder::new().max_rect(area));
        crate::workspace_controls::navigation_hint(&mut controls, w);
    }
}
fn reference_repair_blocks(w: &mut Workspace, command: &str) -> bool {
    let starts_editor = command.starts_with("feature.")
        || command.starts_with("sketch.")
        || command.starts_with("assembly.")
        || command.starts_with("drawing.")
        || command.starts_with("appearance.")
        || command.starts_with("lighting.")
        || matches!(
            command,
            "part.new"
                | "parameter.new"
                | "source"
                | "configurations"
                | "equations"
                | "materials"
                | "appearances"
                | "lighting"
                | "drawings"
                | "manufacturing"
        );
    if w.reference_repair.is_some() && starts_editor {
        w.status = "Apply or cancel Reference Repair before starting another editor".into();
        return true;
    }
    false
}

pub(crate) fn action(w: &mut Workspace, cmd: &str) {
    if crate::settings::action(w, cmd) {
        return;
    }
    if w.settings.is_some() {
        w.status = "Apply or cancel Settings before using another command".into();
        return;
    }
    if crate::workspace_controls::action(w, cmd)
        || crate::reference_repair_ui::action(w, cmd, None)
        || crate::sketch_relations_ui::action(w, cmd)
        || crate::selection_ui::set_action(w, cmd)
    {
        return;
    }
    if reference_repair_blocks(w, cmd) {
        return;
    }
    if crate::appearances_ui::action(w, cmd, None) {
        return;
    }
    if crate::selection_ui::edge_action(w, cmd) {
        return;
    }
    if crate::sketch_blocks_ui::resource_action(w, cmd) {
        return;
    }
    if crate::presentation_resources::action(w, cmd) {
        return;
    }
    if crate::cad_ui::presentation_action(w, cmd) {
        return;
    }
    if crate::navigation::action(w, cmd, None) {
        return;
    }
    if (w.fields.contains_key("appearance-tool")
        || w.fields.contains_key("configuration-editor")
        || w.fields.contains_key("component-pattern-editor")
        || w.fields.contains_key("exploded-view-editor")
        || w.fields.contains_key("assembly-inspection-editor"))
        && (cmd.starts_with("feature.")
            || cmd.starts_with("sketch.")
            || matches!(
                cmd,
                "part.new"
                    | "parameter.new"
                    | "assembly.edit"
                    | "source"
                    | "assembly.pattern"
                    | "assembly.exploded-new"
                    | "assembly.inspection-new"
                    | "materials"
            ))
    {
        w.status =
            "Confirm or cancel the current operation before starting another operation".into();
        return;
    }

    w.selection_popup = None;
    if cmd.starts_with("assembly.inspection-")
        && crate::assembly_inspection_ui::action(w, cmd, None)
    {
        return;
    }
    if cmd.starts_with("drawing.") && crate::drawing_ui::action(w, cmd) {
        return;
    }
    if cmd.starts_with("feature.")
        || cmd.starts_with("sketch.")
        || matches!(cmd, "assembly.edit" | "assembly.pattern")
    {
        crate::exploded_views_ui::collapse_for_modeling(w);
    }
    if cmd == "history.latest" {
        crate::history::latest(w);
        return;
    }
    if matches!(
        cmd,
        "sketch.smart-dimension"
            | "sketch.reference-dimension"
            | "sketch.toggle-dimension-mode"
            | "sketch.repair"
            | "sketch.stretch"
            | "sketch.replace-entities"
            | "sketch.dynamic-mirror"
            | "sketch.stop-dynamic-mirror"
            | "sketch.instance-controls"
            | "sketch.text"
            | "sketch.edit-text"
            | "sketch.block-create"
            | "sketch.block-insert"
            | "sketch.block-import"
            | "sketch.block-export"
            | "sketch.block-edit"
            | "sketch.block-instance"
            | "sketch.block-explode"
            | "sketch.ordinate-dimension"
            | "sketch.baseline-dimension"
            | "sketch.chain-dimension"
            | "sketch.edit-dimension-group"
            | "sketch.delete-dimension-group"
            | "sketch.relations"
            | "sketch.handles"
            | "sketch.finish"
            | "sketch.cancel"
            | "sketch.normal-to"
            | "sketch.finish-dimension"
            | "sketch.cancel-dimension"
            | "sketch.accept-edit"
            | "sketch.cancel-edit"
            | "sketch.convert-entities"
            | "sketch.offset-entities"
            | "sketch.fillet"
            | "sketch.detach-conversion"
            | "sketch.toggle-construction"
            | "sketch.delete-selection"
    ) || cmd.starts_with("sketch.relation.")
    {
        if w.sketch.is_none() && !matches!(cmd, "sketch.cancel" | "sketch.finish") {
            let existing = w
                .view
                .active_part
                .as_ref()
                .and_then(|id| w.session.visible().parts.get(id))
                .and_then(|p| {
                    p.features.iter().find(|f| {
                        w.view.active_node.as_ref() == Some(&f.id)
                            && matches!(f.operation, Operation::Profile { .. })
                    })
                })
                .cloned();
            crate::sketch::begin(w, existing);
        }
        crate::sketch_ui::action(w, cmd);
        return;
    }
    if cad_core::modeling_tools::SKETCH_TOOLS
        .iter()
        .any(|t| t.0 == cmd)
    {
        if w.sketch.is_none() {
            crate::sketch::begin(w, None);
        }
        if let Some(tool) = crate::sketch::tool_for_command(cmd) {
            crate::sketch::set_tool(w, tool);
        }
        return;
    }
    if let Some(tool) = cad_core::modeling_tools::TOOLS
        .iter()
        .find(|tool| tool.command == cmd)
    {
        feature_preset(w, tool.preset);
        return;
    }
    match cmd {
        "assembly.exploded-new" => crate::exploded_views_ui::new(w),
        "assembly.exploded-collapse" => {
            w.view.exploded_view = None;
            w.selected = None;
            w.rendered_key.clear();
        }
        "assembly.pattern" => {
            let assembly = w
                .fields
                .get("assembly-id")
                .cloned()
                .unwrap_or(w.session.visible().root.clone());
            crate::component_pattern_ui::new(w, &assembly);
        }
        "configurations" => crate::configuration_ui::begin(w, "configuration"),
        "equations" => crate::configuration_ui::begin(w, "equations"),
        "parameter.new" => w.open_editor("parameter"),
        "libraries" => w.open_editor("libraries"),
        "file.new-part" => w.replace(crate::settings::new_document(w, false), None),
        "file.new" => w.replace(crate::settings::new_document(w, true), None),
        "file.example" => {
            w.replace(Document::sample(), None);
            w.status = "Drive unit example — sample geometry".into();
        }
        "feature.extrude" => feature_preset(w, "Extrude"),
        "feature.cut" => feature_preset(w, "Cut"),
        "feature.revolve" => feature_preset(w, "Revolve"),
        "feature.fillet" => feature_preset(w, "Fillet"),
        "feature.chamfer" => feature_preset(w, "Chamfer"),
        "feature.pattern" => feature_preset(w, "Pattern"),
        "feature.shell" => feature_preset(w, "Shell"),
        "feature.loft" => feature_preset(w, "Loft"),
        "feature.sweep" => feature_preset(w, "Sweep"),
        "file.open" => {
            if let Some(path) = rfd::FileDialog::new()
                .add_filter("TCAD project", &["json"])
                .set_file_name("project.json")
                .pick_file()
            {
                match FileStore.load(&path) {
                    Ok(d) => w.replace(d, Some(path)),
                    Err(e) => w.status = e.to_string(),
                }
            }
        }
        "file.recover" => {
            if let Some(path) = rfd::FileDialog::new()
                .set_directory(crate::recovery_directory())
                .add_filter("Recovery", &["json"])
                .pick_file()
            {
                let result = (|| -> cad_core::Result<(Document, Option<Document>)> {
                    let v: Value = serde_json::from_slice(&std::fs::read(&path)?)?;
                    let document: Document = serde_json::from_value(
                        v.get("committed")
                            .cloned()
                            .ok_or_else(|| CadError::Invalid("Invalid recovery file".into()))?,
                    )?;
                    let draft: Option<Document> = v
                        .get("draft")
                        .filter(|v| !v.is_null())
                        .cloned()
                        .map(serde_json::from_value)
                        .transpose()?;
                    document.validate()?;
                    if let Some(draft) = &draft {
                        draft.validate()?;
                        if draft.id != document.id {
                            return Err(CadError::Invalid(
                                "Recovery draft belongs to a different document".into(),
                            ));
                        }
                    }
                    let destination = crate::recovery_directory()
                        .join("unsaved")
                        .join(&document.id);
                    let mut sources = vec![destination.clone()];
                    if let Some(root) = v.get("resource_root").and_then(Value::as_str) {
                        sources.insert(0, PathBuf::from(root));
                    }
                    if document.id == w.session.committed.id {
                        sources.push(w.root());
                    }
                    if let Some(parent) = path.parent() {
                        sources.push(parent.to_path_buf());
                    }
                    cad_core::persistence::copy_resources(&document, &sources, &destination)?;
                    if let Some(draft) = &draft {
                        cad_core::persistence::copy_resources(draft, &sources, &destination)?;
                    }
                    Ok((document, draft))
                })();
                match result {
                    Ok((d, draft)) => {
                        w.replace(d, None);
                        if let Some(document) = draft {
                            w.dispatch(Command::ReplaceDocument { document });
                        }
                        w.status = "Recovered local work. Choose Save as to preserve it.".into();
                    }
                    Err(e) => w.status = e.to_string(),
                }
            }
        }
        "file.save" => {
            save(w, false);
        }
        "file.save-as" => {
            save(w, true);
        }
        "file.import" => import_step(w),
        "file.export" => export(w, false),
        "file.export-flat-pattern" => export_format(w, "flat-pattern-dxf", "flat-pattern.dxf"),
        "file.export-cutlist" => export_format(w, "cutlist", "cutlist.csv"),
        "file.release" => export(w, true),
        "part.new" => new_part(w),
        "sketch.new" => {
            if w.view.active_part.is_none() {
                new_part(w);
            } else {
                crate::sketch::begin(w, None);
            }
        }
        "feature.new" => feature_dialog(w, None),
        "draft.apply" if w.tool_checkpoint.is_some() => w.confirm_tool(),
        "draft.cancel" if w.tool_checkpoint.is_some() => w.cancel_tool(),
        "draft.apply" => w.apply(),
        "draft.cancel" => w.cancel(),
        "assembly.edit" => w.open_editor("assembly"),
        "inspection.properties" => w.open_editor("inspection"),
        "source" => {
            w.source_text = serde_json::to_string_pretty(w.session.visible()).unwrap_or_default();
            w.open_editor("source");
        }
        "presentation" => w.open_editor("presentation"),
        "versions" => {
            w.open_editor("versions");
            git(w, &["status", "--short"]);
        }
        "bom" | "materials" | "drawings" | "manufacturing" | "comments" | "controls"
        | "commands" => w.open_editor(cmd),
        "document.rebuild" => {
            w.worker.cancel();
            w.session.restart_worker();
            w.requested = None;
            w.status = "Restarting geometry evaluation".into();
        }
        "viewport.fit" => viewport::fit(w, false),
        "selection.isolate" => w.isolate(),
        _ => w.status = format!("Unknown command: {cmd}"),
    }
}
fn save(w: &mut Workspace, choose: bool) -> bool {
    let path = if choose || w.project.is_none() {
        rfd::FileDialog::new()
            .set_title("Save TCAD project in a folder")
            .pick_folder()
            .map(|p| p.join("project.json"))
    } else {
        w.project.clone()
    };
    if let Some(path) = path {
        if path.exists() {
            match FileStore.load(&path) {
                Ok(on_disk) if on_disk.fingerprint() != w.saved_source => {
                    w.status="Project changed on disk. Open it for comparison or choose a new destination before saving.".into();
                    return false;
                }
                Err(error) => {
                    w.status = format!("Cannot safely read the existing project: {error}");
                    return false;
                }
                _ => {}
            }
        }
        let old_root = w.root();
        let new_root = cad_core::persistence::project_root(&path);
        let result = (|| {
            let sources = [old_root];
            cad_core::persistence::copy_resources(&w.session.committed, &sources, &new_root)?;
            cad_core::persistence::copy_resources(w.session.visible(), &sources, &new_root)?;
            if let Some(checkpoint) = &w.tool_checkpoint {
                cad_core::persistence::copy_resources(checkpoint.visible(), &sources, &new_root)?;
            }
            FileStore.save(&path, &w.session.committed)
        })();
        match result {
            Ok(()) => {
                w.project = Some(path);
                w.rendered_key.clear();
                w.saved_source = w.session.committed.fingerprint();
                w.status = if w.session.draft.is_some() {
                    "Saved committed revision. The draft still needs Apply.".into()
                } else {
                    "Project saved".into()
                };
                return true;
            }
            Err(e) => w.status = e.to_string(),
        }
    }
    false
}

fn export(w: &mut Workspace, release: bool) {
    let doc = w.session.committed.clone();
    let identity = RequestIdentity {
        protocol: PROTOCOL_VERSION,
        generation: w.session.generation,
        base_revision: doc.revision,
        draft: None,
        edit: 0,
        source: doc.fingerprint(),
        backend: BACKEND.into(),
    };
    let root = w.root().to_string_lossy().into();
    if release {
        if let Some(path) = rfd::FileDialog::new()
            .set_title("New immutable release folder")
            .set_file_name(format!("release-r{}", doc.revision))
            .save_file()
        {
            w.exporter.submit(WorkerRequest::Release {
                identity,
                document: doc,
                project_root: root,
                path: path.to_string_lossy().into(),
                tolerance: w.tolerance(),
            });
            w.status = "Building release from committed revision".into();
        }
    } else if let Some(path) = rfd::FileDialog::new()
        .set_title("Export committed revision")
        .add_filter("STEP", &["step"])
        .add_filter("STL", &["stl"])
        .add_filter("Drawing PDF", &["pdf"])
        .add_filter("Profiles DXF", &["dxf"])
        .add_filter("BOM CSV", &["csv"])
        .set_file_name("assembly.step")
        .save_file()
    {
        let format = path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("step")
            .to_string();
        w.exporter.submit(WorkerRequest::Export {
            identity,
            document: doc,
            project_root: root,
            path: path.to_string_lossy().into(),
            format,
            tolerance: w.tolerance(),
        });
        w.status = "Exporting committed geometry".into();
    }
}
fn import_step(w: &mut Workspace) {
    if let Some(path) = rfd::FileDialog::new()
        .add_filter("STEP", &["step", "stp"])
        .pick_file()
    {
        let mut document = w.session.visible().clone();
        match cad_core::persistence::import_resource(&mut document, &w.root(), &path) {
            Ok(resource) => {
                w.dispatch(Command::ReplaceDocument { document });
                w.exporter.submit(WorkerRequest::Import {
                    identity: w.session.identity(),
                    document: w.session.visible().clone(),
                    project_root: w.root().to_string_lossy().into(),
                    resource,
                });
                w.status = "Reading STEP geometry and product structure".into();
            }
            Err(e) => w.status = e.to_string(),
        }
    }
}

fn new_part(w: &mut Workspace) {
    let part = w
        .session
        .visible()
        .new_part(&format!("Part{}", w.session.visible().parts.len() + 1));
    let def = part.id.clone();
    let root = w.session.visible().root.clone();
    w.dispatch(Command::PutPart {
        part,
        assembly: Some(root),
        placement: Default::default(),
    });
    w.view.active_part = Some(def);
    w.view.active_instance = None;
    w.selected = None;
    w.active_plane = None;
    crate::sketch::begin(w, None);
}
pub(crate) fn feature_dialog(w: &mut Workspace, f: Option<Feature>) {
    if w.fields.contains_key("appearance-tool")
        || w.fields.contains_key("configuration-editor")
        || w.fields.contains_key("component-pattern-editor")
        || w.fields.contains_key("exploded-view-editor")
        || w.fields.contains_key("assembly-inspection-editor")
    {
        w.status = "Confirm or cancel the current operation before editing another feature".into();
        return;
    }
    crate::history::latest(w);
    if w.sketch.is_some() {
        w.status = "Exit or cancel the current sketch before editing another feature".into();
        return;
    }
    if w.tool_checkpoint.is_some()
        && w.dialog.as_deref() == Some("feature")
        && f.as_ref().is_some_and(|feature| {
            serde_json::from_str::<Feature>(&w.json).is_ok_and(|current| current.id != feature.id)
        })
    {
        w.status = "Confirm or cancel the current feature before editing another".into();
        return;
    }
    if w.view.active_part.is_none() {
        w.status = "Create or select a part first".into();
        return;
    }
    let Some(f) = f else {
        w.json.clear();
        w.open_editor("feature");
        return;
    };
    if matches!(f.operation, Operation::Profile { .. }) {
        crate::sketch::begin(w, Some(f));
        return;
    }
    let edge_path = crate::selection_ui::edge_selection(&f)
        .and_then(|(body, _)| crate::selection_ui::known_edge_occurrence(w, &body.definition));
    w.start_tool();
    if crate::selection_ui::edge_selection(&f).is_some() {
        w.edge_selection_path = edge_path;
    }
    w.fields.insert("feature-editor-id".into(), f.id.clone());
    w.fields.remove("feature-source-invalid");
    w.fields.remove("feature-source-error");
    w.dialog_title = f.name.clone();
    w.json = serde_json::to_string_pretty(&f).unwrap_or_default();
    w.open_editor("feature");
}
pub(crate) fn feature_preset(w: &mut Workspace, name: &str) {
    if w.fields.contains_key("appearance-tool")
        || w.fields.contains_key("configuration-editor")
        || w.fields.contains_key("component-pattern-editor")
        || w.fields.contains_key("exploded-view-editor")
        || w.fields.contains_key("assembly-inspection-editor")
    {
        w.status = "Confirm or cancel the current operation before editing another feature".into();
        return;
    }
    if crate::rib_ui::is_preset(name) {
        crate::rib_ui::preset(w, name);
        return;
    }
    if crate::direct_edit_ui::is_preset(name) {
        crate::direct_edit_ui::preset(w, name);
        return;
    }
    if crate::body_filter_ui::is_preset(name) {
        crate::body_filter_ui::preset(w, name);
        return;
    }
    if crate::split_ui::is_preset(name) {
        crate::split_ui::preset(w, name);
        return;
    }
    if crate::curve_pattern_ui::is_preset(name) {
        crate::curve_pattern_ui::preset(w, name);
        return;
    }
    if crate::placement_pattern_ui::is_preset(name) {
        crate::placement_pattern_ui::preset(w, name);
        return;
    }
    if crate::reference_curves_ui::preset(w, name) {
        return;
    }
    if crate::holes_ui::is_preset(name) {
        crate::holes_ui::preset(w, name);
        return;
    }
    if crate::reference_ui::preset(w, name) {
        return;
    }
    if crate::surface_ui::is_preset(name) {
        crate::surface_ui::preset(w, name);
        return;
    }
    if crate::mold_ui::is_preset(name) {
        crate::mold_ui::preset(w, name);
        return;
    }
    if crate::fabrication_ui::is_preset(name) {
        crate::fabrication_ui::preset(w, name);
        return;
    }
    crate::history::latest(w);
    let requested_name = name;
    let name = match name {
        "Mid-plane Extrude" | "Two-direction Extrude" | "Thin Extrude" => "Extrude",
        "Through All Cut" | "Through Both Cut" | "Up to Face Cut" | "Thin Cut" => "Cut",
        "Revolved Cut" => "Revolve",
        "Lofted Cut" => "Loft",
        "Swept Cut" => "Sweep",
        "Circular Pattern" => "Pattern",
        "Offset Sketch" => "Sketch",
        other => other,
    };

    if w.confirm_requested {
        w.after_confirm = Some(requested_name.into());
        return;
    }
    if w.sketch.is_some() {
        w.after_confirm = Some(requested_name.into());
        w.confirm_tool();
        if w.sketch.is_some() {
            w.after_confirm = None;
        }
        return;
    }
    if w.tool_checkpoint.is_some() {
        w.status = "Confirm or cancel the current feature before starting another".into();
        return;
    }
    let Some(part) = w
        .view
        .active_part
        .as_ref()
        .and_then(|id| w.session.visible().parts.get(id))
        .cloned()
    else {
        w.status = "Create or select a part first".into();
        return;
    };
    let selected_body = w
        .selected
        .as_ref()
        .and_then(|s| s.body.as_ref())
        .filter(|r| {
            r.definition == part.id
                && part
                    .features
                    .iter()
                    .any(|f| f.id == r.node && !f.suppressed && f.operation.produces_solid())
        })
        .cloned();
    let solid_body = selected_body.or_else(|| {
        part.outputs
            .iter()
            .rev()
            .find(|id| {
                part.features
                    .iter()
                    .any(|f| &f.id == *id && !f.suppressed && f.operation.produces_solid())
            })
            .map(|id| part.reference(id))
    });
    let body = solid_body.clone().unwrap_or(NodeRef {
        definition: part.id.clone(),
        node: "select-body-node".into(),
    });
    let profile = part
        .features
        .iter()
        .find(|f| {
            w.view.active_node.as_ref() == Some(&f.id)
                && matches!(f.operation, Operation::Profile { .. })
        })
        .or_else(|| {
            part.features
                .iter()
                .rev()
                .find(|f| matches!(f.operation, Operation::Profile { .. }))
        })
        .map(|f| part.reference(&f.id))
        .unwrap_or(NodeRef {
            definition: part.id.clone(),
            node: "select-profile-node".into(),
        });
    let edges = viewport::preselected_edges(w, &body);
    let faces = w
        .selected
        .as_ref()
        .filter(|s| s.reference.topology == "face")
        .map(|s| vec![s.reference.clone()])
        .unwrap_or_default();
    if name == "Sketch" {
        crate::sketch::begin(w, None);
        return;
    }
    if matches!(
        name,
        "Extrude" | "Cut" | "Revolve" | "Sweep" | "Loft" | "Structural member"
    ) && !part.features.iter().any(|f| f.id == profile.node)
    {
        crate::sketch::begin(w, None);
        w.status = "Start with a sketch, finish its contour, then create the feature".into();
        return;
    }
    if (name == "Cut" || requested_name.ends_with("Cut")) && solid_body.is_none() {
        w.status = "Extrude a sketch into a body before adding a cut".into();
        return;
    }
    if matches!(
        name,
        "Fillet"
            | "Chamfer"
            | "Shell"
            | "Draft"
            | "Transform"
            | "Scale"
            | "Pattern"
            | "Boolean"
            | "Mirror"
    ) && solid_body.is_none()
    {
        w.status = "Create a body from a sketch before using this feature".into();
        return;
    }
    let plane = part
        .features
        .iter()
        .find(|f| f.id == profile.node)
        .and_then(|f| {
            if let Operation::Profile { plane, .. } = &f.operation {
                Some(plane.clone())
            } else {
                None
            }
        })
        .unwrap_or_default();
    let normal = [
        plane.u[1] * plane.v[2] - plane.u[2] * plane.v[1],
        plane.u[2] * plane.v[0] - plane.u[0] * plane.v[2],
        plane.u[0] * plane.v[1] - plane.u[1] * plane.v[0],
    ];
    let mut op = match name {
        "Circle" | "Rectangle" => {
            crate::sketch::begin(w, None);
            if let Some(s) = &mut w.sketch {
                s.tool = if name == "Circle" {
                    crate::sketch::Tool::Circle
                } else {
                    crate::sketch::Tool::Rectangle
                };
            }
            return;
        }
        "Extrude" | "Cut" => {
            let evaluated_plane = w
                .session
                .draft
                .as_ref()
                .and_then(|d| d.evaluation.as_ref())
                .or(w.session.evaluation.as_ref())
                .and_then(|e| {
                    let key = e
                        .instances
                        .iter()
                        .find(|i| {
                            i.definition == part.id
                                && w.view
                                    .active_instance
                                    .as_ref()
                                    .is_none_or(|id| i.path.contains(id))
                        })
                        .map(|i| {
                            if i.mesh_key.is_empty() {
                                &i.definition
                            } else {
                                &i.mesh_key
                            }
                        })
                        .unwrap_or(&part.id);
                    e.sketches.get(&format!("{key}::{}", profile.node))
                })
                .map(|s| s.plane.clone());
            let sketch_plane = evaluated_plane
                .or_else(|| {
                    part.features
                        .iter()
                        .find(|f| f.id == profile.node)
                        .and_then(|f| {
                            if let Operation::Profile { plane, .. } = &f.operation {
                                Some(plane.clone())
                            } else {
                                None
                            }
                        })
                })
                .unwrap_or_default();
            let direction = [
                sketch_plane.u[1] * sketch_plane.v[2] - sketch_plane.u[2] * sketch_plane.v[1],
                sketch_plane.u[2] * sketch_plane.v[0] - sketch_plane.u[0] * sketch_plane.v[2],
                sketch_plane.u[0] * sketch_plane.v[1] - sketch_plane.u[1] * sketch_plane.v[0],
            ];
            Operation::Extrude {
                profile,
                distance: "10 mm".into(),
                direction: if name == "Cut" {
                    direction.map(|v| -v)
                } else {
                    direction
                },
                body: if solid_body.is_none() {
                    None
                } else {
                    Some(body)
                },
                mode: if name == "Cut" {
                    "cut"
                } else if solid_body.is_none() {
                    "new"
                } else {
                    "join"
                }
                .into(),
                options: Box::new(cad_core::ExtrudeOptions {
                    profile_normal: true,
                    reverse_direction: name == "Cut",
                    ..Default::default()
                }),
            }
        }
        "Revolve" => Operation::Revolve {
            profile: profile.clone(),
            origin: plane.origin,
            axis: plane.v,
            axis_reference: crate::reference_ui::profile_axis(w.session.visible(), &profile),
            angle: "360 deg".into(),
            body: solid_body.clone(),
            mode: "new".into(),
            options: Box::default(),
        },
        "Fillet" => Operation::Fillet {
            body,
            edges,
            radius: "3 mm".into(),
        },
        "Chamfer" => Operation::Chamfer {
            body,
            edges,
            distance: "2 mm".into(),
            options: Box::default(),
        },
        "Pattern" => Operation::Pattern {
            body,
            count: 4,
            step: ["30 mm".into(), "0 mm".into(), "0 mm".into()],
            angle: "0 deg".into(),
            axis: [0., 0., 1.],
            axis_reference: None,
            options: Default::default(),
        },
        "Associative reference" => {
            let Some(selected) = w.selected.as_ref() else {
                w.status = "Select a component to reference".into();
                return;
            };
            let source = w.session.visible().parts.get(&selected.definition);
            let Some(source) = source else {
                return;
            };
            let Some(output) = source.outputs.last() else {
                return;
            };
            Operation::ReferenceBody {
                body: source.reference(output),
                instance: selected.instance.clone(),
                relative_to: w.view.active_instance.clone(),
            }
        }
        "Transform" => Operation::Transform {
            body,
            placement: Default::default(),
        },
        "Scale" => Operation::Scale {
            body,
            options: Default::default(),
        },
        "Boolean" => {
            let right = w
                .session
                .visible()
                .parts
                .values()
                .filter(|p| p.id != part.id)
                .find_map(|p| {
                    p.outputs
                        .iter()
                        .rev()
                        .find(|id| {
                            p.features.iter().any(|f| {
                                &f.id == *id && !f.suppressed && f.operation.produces_solid()
                            })
                        })
                        .map(|id| p.reference(id))
                })
                .unwrap_or(body.clone());
            Operation::Boolean {
                left: body,
                right,
                mode: "union".into(),
            }
        }
        "Loft" => {
            let profiles = part
                .features
                .iter()
                .filter(|f| matches!(f.operation, Operation::Profile { .. }))
                .map(|f| part.reference(&f.id))
                .collect();
            Operation::Loft {
                options: Default::default(),
                profiles,
                body: solid_body.clone(),
                mode: "new".into(),
            }
        }
        "Sweep" => Operation::Sweep {
            options: Default::default(),
            profile,
            path: vec![
                plane.origin,
                std::array::from_fn(|axis| plane.origin[axis] + normal[axis] * 50.),
            ],
            path_sketch: None,
            body: solid_body.clone(),
            mode: "new".into(),
        },
        "Shell" => Operation::Shell {
            body,
            thickness: "2 mm".into(),
            removed_faces: faces,
            options: Default::default(),
        },
        "Structural member" => Operation::StructuralMember {
            section: match &part
                .features
                .iter()
                .find(|f| f.id == profile.node)
                .unwrap()
                .operation
            {
                Operation::Profile { profile, .. } => profile.clone(),
                _ => return,
            },
            start: [0., 0., 0.],
            end: [0., 0., 200.],
            roll: "0 deg".into(),
        },
        "Mirror" => Operation::Mirror {
            body,
            plane: Plane::default(),
            plane_reference: None,
            keep_original: true,
        },
        "Draft" => Operation::Draft {
            body,
            faces,
            angle: "3 deg".into(),
            plane: Plane::default(),
            plane_reference: None,
        },
        _ => return,
    };
    crate::reference_ui::seed_inputs(w, &mut op);
    let target_body = solid_body.clone();
    match &mut op {
        Operation::Extrude { options, .. } => match requested_name {
            "Mid-plane Extrude" => options.end = ExtrudeEnd::MidPlane,
            "Two-direction Extrude" => options.reverse_distance = Some("10 mm".into()),
            "Through All Cut" => options.end = ExtrudeEnd::ThroughAll,
            "Through Both Cut" => options.end = ExtrudeEnd::ThroughAllBoth,
            "Up to Face Cut" => options.end = ExtrudeEnd::UpToFace,
            "Thin Extrude" | "Thin Cut" => {
                options.thin = Some(cad_core::ThinExtrude {
                    mode: cad_core::ThinExtrudeMode::OneSide,
                    thickness: "2 mm".into(),
                    second_thickness: None,
                    reverse: false,
                })
            }
            _ => {}
        },
        Operation::Revolve { body, mode, .. }
        | Operation::Loft { body, mode, .. }
        | Operation::Sweep { body, mode, .. }
            if requested_name.ends_with("Cut") =>
        {
            *body = target_body;
            *mode = "cut".into();
        }
        Operation::Pattern { step, angle, .. } if requested_name == "Circular Pattern" => {
            *step = ["0 mm".into(), "0 mm".into(), "0 mm".into()];
            *angle = "90 deg".into();
        }
        _ => {}
    }
    w.start_tool();
    let base_name = match requested_name {
        "Revolved Cut" => "Cut-Revolve",
        "Lofted Cut" => "Cut-Loft",
        "Swept Cut" => "Cut-Sweep",
        "Mid-plane Extrude" | "Two-direction Extrude" | "Thin Extrude" => "Boss-Extrude",
        "Through All Cut" | "Through Both Cut" | "Up to Face Cut" | "Thin Cut" => "Cut-Extrude",
        "Extrude" => "Boss-Extrude",
        "Cut" => "Cut-Extrude",
        "Revolve" => "Revolve",
        other => other,
    };
    let number = part
        .features
        .iter()
        .filter(|f| f.name.starts_with(base_name))
        .count()
        + 1;
    let feature = Feature {
        id: id(),
        name: format!("{base_name}{number}"),
        operation: op,
        suppressed: false,
    };
    w.dispatch(Command::PutFeature {
        definition: part.id,
        feature: feature.clone().into(),
        output: !matches!(feature.operation, Operation::ReferenceBody { .. }),
    });
    w.view.active_node = Some(feature.id.clone());
    feature_dialog(w, Some(feature));
}
fn git(w: &mut Workspace, args: &[&str]) {
    if w.project.is_none() {
        w.status = "Save the project before using Git".into();
        return;
    }
    w.jobs.submit(crate::background::Job::Git {
        root: w.root(),
        args: args.iter().map(|s| s.to_string()).collect(),
    });
    w.status = "Git operation running".into();
}

fn property_manager(ui: &mut egui::Ui, w: &mut Workspace) {
    if w.dialog.is_none() && w.sketch.is_some() {
        w.open_editor("sketch");
    }
    if w.recovery_draft.is_some() {
        ui.strong("Recover pending work");
        ui.label("A recoverable draft exists for this saved source revision.");
        ui.horizontal(|ui| {
            if ui.button("Restore draft").clicked() {
                w.recover();
            }
            if ui.button("Discard recovery").clicked() {
                w.recovery_draft = None;
            }
        });
        ui.separator();
    }
    if w.dialog.is_none()
        || w.dialog.as_deref() == Some("appearances")
            && crate::presentation_composition::appearance_panel_hosted(w)
    {
        crate::cad_ui::properties(ui, w);
        return;
    }
    let dialog = w.dialog.clone().unwrap_or_default();
    if dialog == "properties" {
        crate::cad_ui::properties(ui, w);
        return;
    }
    if matches!(dialog.as_str(), "feature" | "sketch" | "reference-repair") {
        crate::cad_ui::confirmation(ui, w);
        if w.dialog.is_none() {
            return;
        }
        ui.separator();
    }
    ui.horizontal(|ui| {
        ui.strong(match dialog.as_str() {
            "reference-repair" => "Repair References",
            "selection" => "Selection Actions",
            "feature" => "Feature",
            "sketch" => "Sketch on model",
            "source" => "Source definitions",
            "presentation" => "Layout and theme",
            "assembly" => "Assembly",
            "configurations" => "Configurations and Equations",
            "versions" => "Design versions",
            "materials" => "Engineering Materials",
            "appearances" => "Appearance · Colors and Textures",
            "drawings" => "Drawings",
            "manufacturing" => "Manufacturing",
            "libraries" => "Component libraries",
            "controls" => "Controls",
            "comments" => "Review comments",
            "parameter" => "Named parameter",
            "bom" => "Bill of materials",
            "inspection" => "Mass Properties",
            _ => "Command search",
        });
        if !matches!(
            dialog.as_str(),
            "feature" | "sketch" | "configurations" | "appearances" | "reference-repair"
        ) && !w.fields.contains_key("mate-editor")
            && !w.fields.contains_key("component-pattern-editor")
            && ui.small_button("Close").clicked()
        {
            w.dialog = None;
            w.manager_tree = true;
        }
    });
    ui.separator();
    egui::ScrollArea::vertical()
        .id_salt("workspace-editor")
        .show(ui, |ui| match dialog.as_str() {
            "reference-repair" => crate::reference_repair_ui::show(ui, w),
            "selection" => {
                if let Some(target) = crate::selection_ui::current(w) {
                    crate::selection_ui::contents(ui, w, &target);
                }
            }
            "configurations" => crate::configuration_ui::show(ui, w),
            "inspection" => crate::inspection_ui::controls(ui, w),
            "feature" => feature_editor(ui, w),
            "sketch" => crate::sketch::controls(ui, w),
            "source" => {
                ui.label("Source changes use the same preview and Apply path.");
                ui.add(
                    egui::TextEdit::multiline(&mut w.source_text)
                        .code_editor()
                        .desired_width(f32::INFINITY)
                        .desired_rows(24),
                );
                if ui.button("Preview source changes").clicked() {
                    match serde_json::from_str::<Document>(&w.source_text) {
                        Ok(document) => w.dispatch(Command::ReplaceDocument { document }),
                        Err(e) => w.status = e.to_string(),
                    }
                }
            }
            "presentation" => presentation_editor(ui, w),
            "assembly" => assembly_editor(ui, w),
            "versions" => versions(ui, w),
            "bom" => bom(ui, w),
            "materials" => materials(ui, w),
            "appearances" => crate::appearances_ui::show(ui, w),
            "libraries" => library_editor(ui, w),
            "drawings" => drawings(ui, w),
            "manufacturing" => manufacturing(ui, w),
            "comments" => comments(ui, w),
            "controls" => {
                if ui.button("Open Settings…").clicked() {
                    crate::settings::open(w, Some("application.navigation"));
                }
            }
            "parameter" => {
                let mut name = w
                    .fields
                    .get("parameter-name")
                    .cloned()
                    .unwrap_or("dimension".into());
                let mut value = w
                    .fields
                    .get("parameter-value")
                    .cloned()
                    .unwrap_or("10 mm".into());
                ui.label("Identity / name");
                ui.text_edit_singleline(&mut name);
                ui.label("Expression");
                ui.text_edit_singleline(&mut value);
                w.fields.insert("parameter-name".into(), name.clone());
                w.fields.insert("parameter-value".into(), value.clone());
                let mut dimension = w
                    .fields
                    .get("parameter-dimension")
                    .cloned()
                    .unwrap_or("length".into());
                egui::ComboBox::from_id_salt("param-dimension")
                    .selected_text(&dimension)
                    .show_ui(ui, |ui| {
                        for d in ["length", "angle", "scalar"] {
                            ui.selectable_value(&mut dimension, d.into(), d);
                        }
                    });
                w.fields
                    .insert("parameter-dimension".into(), dimension.clone());
                if ui.button("Add to draft").clicked() {
                    if !name.is_empty() && name.chars().all(|c| c.is_alphanumeric() || c == '_') {
                        let definition = w
                            .view
                            .active_part
                            .clone()
                            .unwrap_or(w.session.visible().root.clone());
                        w.dispatch(Command::PutParameter {
                            definition,
                            parameter: name.clone(),
                            value: Parameter {
                                name,
                                expression: Expr(value),
                                dimension: match dimension.as_str() {
                                    "angle" => Dimension::Angle,
                                    "scalar" => Dimension::Scalar,
                                    _ => Dimension::Length,
                                },
                            },
                        });
                        w.dialog = None;
                    } else {
                        w.status =
                            "Use letters, digits and underscores for the parameter identity".into();
                    }
                }
            }
            _ => {
                let query = w.fields.entry("command-query".into()).or_default();
                ui.text_edit_singleline(query);
                let query = query.to_lowercase();
                for (command, label) in cad_core::presentation::command_catalog() {
                    if label.to_lowercase().contains(&query) && ui.button(label).clicked() {
                        action(w, command);
                    }
                }
            }
        });
}
fn feature_editor(ui: &mut egui::Ui, w: &mut Workspace) {
    let current = w
        .fields
        .get("feature-editor-id")
        .and_then(|id| {
            w.view
                .active_part
                .as_ref()
                .and_then(|owner| w.session.visible().parts.get(owner))
                .and_then(|p| p.features.iter().find(|f| &f.id == id))
        })
        .cloned();
    let Some(mut feature) = current else {
        ui.label("Choose a modeling command from Features or select a feature to edit.");
        return;
    };
    ui.strong(&feature.name);
    ui.separator();
    let changed = ui
        .add_enabled_ui(!w.fields.contains_key("feature-source-invalid"), |ui| {
            crate::cad_ui::feature_fields(ui, w, &mut feature)
        })
        .inner;
    if changed {
        w.json = serde_json::to_string_pretty(&feature).unwrap_or_default();
        preview_feature(w, feature.clone());
    }
    egui::CollapsingHeader::new("Advanced Source").show(ui, |ui| {
        ui.small("Source edits update the shared preview when the definition is valid.");
        let source_changed = ui
            .add(
                egui::TextEdit::multiline(&mut w.json)
                    .code_editor()
                    .desired_rows(14)
                    .desired_width(f32::INFINITY),
            )
            .changed();
        if source_changed || ui.button("Rebuild from Source").clicked() {
            match serde_json::from_str::<Feature>(&w.json) {
                Ok(parsed) if parsed.id == feature.id => {
                    w.fields.remove("feature-source-invalid");
                    w.fields.remove("feature-source-error");
                    if parsed != feature {
                        preview_feature(w, parsed);
                    }
                }
                result => {
                    let error = match result {
                        Ok(_) => "The feature identity must remain unchanged".into(),
                        Err(error) => error.to_string(),
                    };
                    w.fields
                        .insert("feature-source-invalid".into(), "true".into());
                    w.fields
                        .insert("feature-source-error".into(), error.clone());
                    w.confirm_requested = false;
                    w.status = error;
                }
            }
        }
        if let Some(error) = w.fields.get("feature-source-error") {
            ui.colored_label(egui::Color32::from_rgb(180, 45, 30), error);
        }
    });
}
pub(crate) fn preview_feature(w: &mut Workspace, feature: Feature) {
    if let Some(definition) = w.view.active_part.clone() {
        let expected = feature.clone();
        w.dispatch(Command::PutFeature {
            definition: definition.clone(),
            feature: feature.into(),
            output: false,
        });
        let synced = w
            .session
            .visible()
            .parts
            .get(&definition)
            .and_then(|p| p.features.iter().find(|f| f.id == expected.id))
            .is_some_and(|f| f == &expected);
        if synced {
            w.fields.remove("feature-source-invalid");
            w.fields.remove("feature-source-error");
        } else {
            w.fields
                .insert("feature-source-invalid".into(), "true".into());
            w.fields
                .insert("feature-source-error".into(), w.status.clone());
            w.confirm_requested = false;
        }
    }
}
fn capture_feature_selection(w: &mut Workspace, cycle: bool, toggle: bool) {
    if w.dialog.as_deref() != Some("feature") || w.fields.contains_key("feature-source-invalid") {
        return;
    }
    let Ok(mut feature) = serde_json::from_str::<Feature>(&w.json) else {
        return;
    };
    let Some(selected) = w.selected.clone() else {
        return;
    };
    if let Some((body, _)) = crate::selection_ui::edge_selection(&feature)
        && !crate::selection_ui::accepts_edge(w, body, &selected)
    {
        w.status = "Select edges on this feature's input body and component".into();
        return;
    }
    let is_edge_feature = matches!(
        feature.operation,
        Operation::Fillet { .. } | Operation::Chamfer { .. }
    );
    let feature_id = feature.id.clone();
    let selections = match &mut feature.operation {
        Operation::Fillet { edges, .. } | Operation::Chamfer { edges, .. }
            if selected.reference.topology == "edge" =>
        {
            edges
        }
        Operation::Shell { removed_faces, .. } if selected.reference.topology == "face" => {
            removed_faces
        }
        Operation::Draft { faces, .. } if selected.reference.topology == "face" => faces,
        _ => return,
    };
    let before = selections.clone();
    let mut toggle = toggle;
    if is_edge_feature {
        let last_matches = w.fields.get("edge-pick-feature") == Some(&feature_id)
            && w.fields.get("edge-pick-last").is_some_and(|json| {
                serde_json::from_str::<Vec<cad_core::TopologyRef>>(json)
                    .is_ok_and(|last| last == *selections)
            });
        if cycle && last_matches {
            if let Some(base) = w
                .fields
                .get("edge-pick-base")
                .and_then(|json| serde_json::from_str::<Vec<cad_core::TopologyRef>>(json).ok())
            {
                *selections = base;
                toggle = w
                    .fields
                    .get("edge-pick-toggle")
                    .is_some_and(|value| value == "true");
            }
        } else {
            w.fields.insert(
                "edge-pick-base".into(),
                serde_json::to_string(selections).unwrap_or_default(),
            );
            w.fields.insert("edge-pick-feature".into(), feature_id);
            w.fields
                .insert("edge-pick-toggle".into(), toggle.to_string());
        }
    }
    let existing = selections
        .iter()
        .position(|reference| crate::selection_ui::same_edge(reference, &selected.reference));
    if is_edge_feature
        && toggle
        && let Some(index) = existing
    {
        selections.remove(index);
    } else if existing.is_none() {
        selections.push(selected.reference.clone());
    }
    if is_edge_feature {
        w.fields.insert(
            "edge-pick-last".into(),
            serde_json::to_string(selections).unwrap_or_default(),
        );
        if !selections
            .iter()
            .any(|edge| crate::selection_ui::same_edge(edge, &selected.reference))
        {
            w.selected = None;
            w.geometry_selection.clear();
        }
    }
    if *selections != before {
        w.json = serde_json::to_string_pretty(&feature).unwrap_or_default();
        preview_feature(w, feature);
    }
}
fn presentation_editor(ui: &mut egui::Ui, w: &mut Workspace) {
    ui.horizontal(|ui| {
        ui.label("Composition");
        let mut active = w.view.active_layout.clone();
        egui::ComboBox::from_id_salt("layout-name")
            .selected_text(&active)
            .show_ui(ui, |ui| {
                for name in w.layout.layouts.keys() {
                    ui.selectable_value(&mut active, name.clone(), name);
                }
            });
        w.view.active_layout = active;
        if ui.button("Reset default").clicked() {
            crate::presentation_resources::reset(w);
        }
    });
    ui.label("Reorder, group, hide, resize, duplicate or absolutely position semantic components. Styles accept variables, IDs, classes, role/layout/state selectors and CSS declarations. F9 is always available for recovery.");
    egui::CollapsingHeader::new("Semantic binding catalog").show(ui, |ui| {
        let query = w.fields.entry("binding-query".into()).or_default();
        ui.add(egui::TextEdit::singleline(query).hint_text("Search names, roles or identities"));
        let query = query.to_lowercase();
        let elements = crate::presentation_composition::semantics(w);
        egui::ScrollArea::vertical()
            .id_salt("binding-catalog")
            .max_height(180.)
            .show(ui, |ui| {
                for element in elements
                    .iter()
                    .filter(|e| {
                        format!("{} {} {}", e.id, e.label, e.role)
                            .to_lowercase()
                            .contains(&query)
                    })
                    .take(250)
                {
                    ui.horizontal(|ui| {
                        if ui.small_button("Copy binding").clicked() {
                            ui.ctx().copy_text(
                                serde_json::json!({"role":element.role,"bind":element.id})
                                    .to_string(),
                            );
                        }
                        ui.small(&element.label).on_hover_text(&element.id);
                        ui.small(&element.role);
                    });
                }
            });
    });
    ui.columns(2, |columns| {
        columns[0].label("Layout JSON");
        egui::ScrollArea::vertical()
            .id_salt("layout-source")
            .max_height(410.)
            .show(&mut columns[0], |ui| {
                ui.add(
                    egui::TextEdit::multiline(&mut w.layout_text)
                        .code_editor()
                        .desired_width(f32::INFINITY)
                        .desired_rows(24),
                );
            });
        columns[1].label("Theme JSON or CSS");
        egui::ScrollArea::vertical()
            .id_salt("theme-source")
            .max_height(410.)
            .show(&mut columns[1], |ui| {
                ui.add(
                    egui::TextEdit::multiline(&mut w.theme_text)
                        .code_editor()
                        .desired_width(f32::INFINITY)
                        .desired_rows(24),
                );
            });
    });
    crate::presentation_resources::show(ui, w);
}
fn assembly_editor(ui: &mut egui::Ui, w: &mut Workspace) {
    let doc = w.session.visible().clone();
    let mut assembly_id = w
        .fields
        .get("assembly-id")
        .cloned()
        .unwrap_or(doc.root.clone());
    if w.fields.contains_key("mate-editor")
        || w.fields.contains_key("component-pattern-editor")
        || w.fields.contains_key("exploded-view-editor")
        || w.fields.contains_key("assembly-inspection-editor")
    {
        ui.label(cad_core::presentation::definition_name(&doc, &assembly_id));
    } else {
        egui::ComboBox::from_id_salt("assembly-selector")
            .selected_text(cad_core::presentation::definition_name(&doc, &assembly_id))
            .show_ui(ui, |ui| {
                for a in doc.assemblies.values() {
                    ui.selectable_value(&mut assembly_id, a.id.clone(), &a.name);
                }
            });
    }
    w.fields.insert("assembly-id".into(), assembly_id.clone());
    let Some(a) = doc.assemblies.get(&assembly_id) else {
        return;
    };
    if w.fields.contains_key("exploded-view-editor") {
        crate::exploded_views_ui::show(ui, w, &assembly_id);
        return;
    }
    if w.fields.contains_key("assembly-inspection-editor") {
        crate::assembly_inspection_ui::show(ui, w, &assembly_id);
        return;
    }
    if w.fields.contains_key("component-pattern-editor") {
        crate::component_pattern_ui::show(ui, w, &assembly_id);
        return;
    }
    if w.fields.contains_key("mate-editor") {
        crate::mate_ui::show(ui, w, &assembly_id);
        return;
    }
    ui.horizontal(|ui| {
        if ui.button("Add instance").clicked()
            && let Some(definition) = w.view.active_part.clone()
        {
            let key = id();
            w.dispatch(Command::PutInstance {
                assembly: assembly_id.clone(),
                instance: Instance {
                    id: key,
                    name: cad_core::presentation::definition_name(&doc, &definition),
                    definition,
                    placement: Default::default(),
                    configuration: None,
                    finish: None,
                    fixed: false,
                    translation_expressions: None,
                },
            });
        }
        if ui.button("New subassembly").clicked() {
            let mut document = doc.clone();
            let key = id();
            document.assemblies.insert(
                key.clone(),
                Assembly {
                    id: key.clone(),
                    name: "Subassembly".into(),
                    instances: BTreeMap::new(),
                    mates: vec![],
                    promoted_parameters: BTreeMap::new(),
                    configurations: BTreeMap::new(),
                    component_patterns: BTreeMap::new(),
                    exploded_views: BTreeMap::new(),
                    inspections: BTreeMap::new(),
                    parameters: BTreeMap::new(),
                },
            );
            let instance = id();
            document
                .assemblies
                .get_mut(&assembly_id)
                .unwrap()
                .instances
                .insert(
                    instance.clone(),
                    Instance {
                        id: instance,
                        name: "Subassembly".into(),
                        definition: key,
                        placement: Default::default(),
                        configuration: None,
                        finish: None,
                        fixed: false,
                        translation_expressions: None,
                    },
                );
            w.dispatch(Command::ReplaceDocument { document });
        }
        if ui.button("New mate").clicked() {
            crate::mate_ui::new(w, &assembly_id);
        }
    });
    if ui.button("Component Pattern…").clicked() {
        crate::component_pattern_ui::new(w, &assembly_id);
    }
    if assembly_id == doc.root {
        if ui.button("Interference / Clearance…").clicked() {
            crate::assembly_inspection_ui::new(w);
        }
        for inspection in a.inspections.values() {
            ui.horizontal(|ui| {
                ui.label(&inspection.name);
                if ui.small_button("Inspect / Edit").clicked() {
                    crate::assembly_inspection_ui::open(w, &inspection.id);
                }
                if ui.small_button("Delete").clicked() {
                    w.dispatch(Command::DeleteAssemblyInspection {
                        assembly: assembly_id.clone(),
                        inspection: inspection.id.clone(),
                    });
                    crate::assembly_inspection_ui::cancel(w);
                }
            });
        }
        if ui.button("New Exploded View…").clicked() {
            crate::exploded_views_ui::new(w);
        }
        for view in a.exploded_views.values() {
            ui.horizontal(|ui| {
                ui.label(&view.name);
                if ui.small_button("Edit").clicked() {
                    crate::exploded_views_ui::begin(w, &assembly_id, view.clone());
                }
                if ui.small_button("Delete").clicked() {
                    w.dispatch(Command::DeleteExplodedView {
                        assembly: assembly_id.clone(),
                        view: view.id.clone(),
                    });
                    if w.view
                        .exploded_view
                        .as_ref()
                        .is_some_and(|s| s.view == view.id)
                    {
                        w.view.exploded_view = None;
                    }
                }
            });
        }
    }
    for pattern in a.component_patterns.values() {
        ui.horizontal(|ui| {
            ui.label(&pattern.name);
            if ui.small_button("Edit").clicked() {
                crate::component_pattern_ui::begin(w, &assembly_id, pattern.clone());
            }
            if ui.small_button("Delete").clicked() {
                w.dispatch(Command::DeleteComponentPattern {
                    assembly: assembly_id.clone(),
                    pattern: pattern.id.clone(),
                });
            }
        });
    }
    let generated = cad_core::component_pattern::generated(a);
    let present = cad_core::configuration::effective(&doc)
        .ok()
        .and_then(|d| d.assemblies.get(&assembly_id).cloned())
        .map(|a| {
            a.instances
                .keys()
                .cloned()
                .collect::<std::collections::BTreeSet<_>>()
        })
        .unwrap_or_default();
    egui::ScrollArea::vertical()
        .max_height(360.)
        .show(ui, |ui| {
            for i in a.instances.values().filter(|i|present.contains(&i.id)) {
                egui::CollapsingHeader::new(format!(
                    "{} · {}",
                    i.name,
                    cad_core::presentation::definition_name(&doc, &i.definition)
                ))
                .id_salt(&i.id)
                .show(ui, |ui| {
                    if generated.contains(&i.id){ui.weak("Placement and shared definition are driven by the component pattern. Use configuration overrides to suppress this occurrence or select another configuration.");return;}
                    let mut instance = i.clone();
                    let mut changed = false;
                    changed |= ui.text_edit_singleline(&mut instance.name).changed();
                    changed |= ui
                        .checkbox(&mut instance.fixed, "Grounded / fixed")
                        .changed();
                    ui.small(format!(
                        "Definition: {}",
                        cad_core::presentation::definition_name(&doc, &instance.definition)
                    ));
                    ui.horizontal(|ui| {
                        ui.label("Position");
                        for value in &mut instance.placement.translation {
                            changed |= ui
                                .add(egui::DragValue::new(value).speed(0.2).suffix(" mm"))
                                .changed();
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.label("Rotation");
                        for value in &mut instance.placement.rotation_deg {
                            changed |= ui
                                .add(egui::DragValue::new(value).speed(0.2).suffix("°"))
                                .changed();
                        }
                    });
                    let mut expr = instance.translation_expressions.is_some();
                    if ui
                        .checkbox(&mut expr, "Drive position with assembly expressions")
                        .changed()
                    {
                        instance.translation_expressions = if expr {
                            Some(
                                instance
                                    .placement
                                    .translation
                                    .map(|v| Expr(format!("{v} mm"))),
                            )
                        } else {
                            None
                        };
                        changed = true;
                    }
                    if let Some(expressions) = &mut instance.translation_expressions {
                        for e in expressions {
                            changed |= ui.text_edit_singleline(&mut e.0).changed();
                        }
                    }
                    {
                        let names=cad_core::configuration::names(&doc,&instance.definition);
                        let mut configuration=instance.configuration.clone();
                        egui::ComboBox::from_id_salt(format!("config/{}",i.id)).selected_text(configuration.as_ref().and_then(|id|names.get(id)).map(String::as_str).unwrap_or("Active definition configuration")).show_ui(ui,|ui|{
                            ui.selectable_value(&mut configuration,None,"Active definition configuration");
                            for (id,name) in &names {ui.selectable_value(&mut configuration,Some(id.clone()),name);}
                        });
                        if configuration!=instance.configuration {instance.configuration=configuration;changed=true;}
                    }
                    if changed {
                        w.dispatch(Command::PutInstance {
                            assembly: assembly_id.clone(),
                            instance,
                        });
                    }
                    ui.horizontal(|ui| {
                        if ui.button("Edit in place").clicked() {
                            w.view.active_instance = Some(i.id.clone());
                            w.view.active_part = Some(i.definition.clone());
                            if !w.view.isolation {
                                w.isolate();
                            }
                        }
                        if ui.button("Make unique").clicked() {
                            w.dispatch(Command::MakeUnique {
                                assembly: assembly_id.clone(),
                                instance: i.id.clone(),
                            });
                        }
                        if ui.button("Duplicate shared").clicked() {
                            let mut clone = i.clone();
                            clone.id = id();
                            clone.name.push_str(" copy");
                            clone.placement.translation[0] += 30.;
                            w.dispatch(Command::PutInstance {
                                assembly: assembly_id.clone(),
                                instance: clone,
                            });
                        }
                        if ui.button("Remove").clicked() {
                            w.dispatch(Command::DeleteInstance {
                                assembly: assembly_id.clone(),
                                instance: i.id.clone(),
                            });
                        }
                    });
                });
            }
            for mate in &a.mates {
                ui.horizontal(|ui| {
                    ui.small(format!(
                        "{}: {} ↔ {}",
                        mate.kind,
                        a.instances
                            .get(&mate.first)
                            .map(|i| i.name.as_str())
                            .unwrap_or("Missing component"),
                        a.instances
                            .get(&mate.second)
                            .map(|i| i.name.as_str())
                            .unwrap_or("Missing component")
                    ));
                    if ui.small_button("Edit").clicked() {
                        crate::mate_ui::begin(w, &assembly_id, mate.clone());
                    }
                    if ui.small_button("Remove").clicked() {
                        w.dispatch(Command::DeleteMate {
                            assembly: assembly_id.clone(),
                            mate: mate.id.clone(),
                        });
                    }
                });
            }
        });
    crate::mate_ui::show(ui, w, &assembly_id);
}
fn bom(ui: &mut egui::Ui, w: &mut Workspace) {
    let doc = w.session.visible();
    if let Ok(rows) = cad_core::manufacturing::bom(doc) {
        egui::ScrollArea::both().show(ui, |ui| {
            egui::Grid::new("bom").striped(true).show(ui, |ui| {
                for heading in [
                    "Part number",
                    "Description",
                    "Material",
                    "Quantity",
                    "Configuration",
                ] {
                    ui.strong(heading);
                }
                ui.end_row();
                for r in rows {
                    ui.label(r.part_number);
                    ui.label(r.description);
                    ui.label(r.material);
                    ui.label(r.quantity.to_string());
                    ui.label(r.configuration.unwrap_or_default());
                    ui.end_row();
                }
            });
        });
    }
    if ui.button("Export committed BOM").clicked()
        && let Some(path) = rfd::FileDialog::new().set_file_name("bom.csv").save_file()
    {
        match cad_core::manufacturing::csv(&w.session.committed)
            .and_then(|s| atomic_write(&path, s.as_bytes()))
        {
            Ok(()) => w.status = format!("Wrote {}", path.display()),
            Err(e) => w.status = e.to_string(),
        }
    }
}
fn materials(ui: &mut egui::Ui, w: &mut Workspace) {
    let doc = w.session.visible().clone();
    ui.small("Engineering materials control density and manufacturing data. Edit visual finishes separately.");
    if ui.button("Visual finishes…").clicked() {
        crate::appearances_ui::begin(w, false);
        return;
    }
    egui::ScrollArea::vertical().show(ui, |ui| {
        for m in doc.materials.values() {
            egui::CollapsingHeader::new(&m.name)
                .id_salt(&m.id)
                .show(ui, |ui| {
                    let mut m = m.clone();
                    let mut changed = ui.text_edit_singleline(&mut m.name).changed();
                    changed |= ui
                        .add(
                            egui::DragValue::new(&mut m.density_kg_m3)
                                .range(0.001..=100000.)
                                .speed(10.)
                                .prefix("Density ")
                                .suffix(" kg/m³"),
                        )
                        .changed();
                    if changed {
                        w.dispatch(Command::PutMaterial {
                            material: m.clone(),
                        });
                    }
                    if ui.button("Assign to active part").clicked()
                        && let Some(part) =
                            w.view.active_part.as_ref().and_then(|i| doc.parts.get(i))
                    {
                        let mut part = part.clone();
                        part.material = m.id;
                        w.dispatch(Command::PutPart {
                            part,
                            assembly: None,
                            placement: Default::default(),
                        });
                    }
                });
        }
        if ui.button("New material").clicked() {
            w.dispatch(Command::PutMaterial {
                material: cad_core::Material {
                    id: id(),
                    name: "Custom material".into(),
                    density_kg_m3: 1000.,
                    color: [0.5; 3],
                    metallic: 0.,
                    roughness: 0.4,
                },
            });
        }
    });
}
fn drawings(ui: &mut egui::Ui, w: &mut Workspace) {
    crate::drawing_ui::editor(ui, w);
}

pub(crate) fn export_format(w: &mut Workspace, format: &str, name: &str) {
    if let Some(path) = rfd::FileDialog::new().set_file_name(name).save_file() {
        let d = w.session.committed.clone();
        w.exporter.submit(WorkerRequest::Export {
            identity: RequestIdentity {
                protocol: PROTOCOL_VERSION,
                generation: w.session.generation,
                base_revision: d.revision,
                draft: None,
                edit: 0,
                source: d.fingerprint(),
                backend: BACKEND.into(),
            },
            document: d,
            project_root: w.root().to_string_lossy().into(),
            path: path.to_string_lossy().into(),
            format: format.into(),
            tolerance: w.tolerance(),
        });
        w.status = "Exporting committed revision".into();
    }
}
fn manufacturing(ui: &mut egui::Ui, w: &mut Workspace) {
    let doc = w.session.visible().clone();
    if let Some(part) = w.view.active_part.as_ref().and_then(|i| doc.parts.get(i)) {
        let mut part = part.clone();
        ui.label(RichText::new(&part.name).strong());
        let mut changed = false;
        for key in [
            "part_number",
            "description",
            "finish",
            "dimensional_tolerance",
            "geometric_tolerance",
            "engineering_standard",
        ] {
            let value = part.properties.entry(key.into()).or_default();
            ui.horizontal(|ui| {
                ui.small(key);
                changed |= ui.text_edit_singleline(value).changed();
            });
        }
        if changed {
            w.dispatch(Command::PutPart {
                part,
                assembly: None,
                placement: Default::default(),
            });
        }
    }
    ui.horizontal_wrapped(|ui| {
        if ui.button("Structural member").clicked() {
            feature_preset(w, "Structural Member Group");
        }
        if ui.button("Export cut list").clicked() {
            export_format(w, "cutlist", "cutlist.csv");
        }
    });
    let cut_list = viewport::display_evaluation(w)
        .filter(|evaluation| evaluation.valid && evaluation.identity == w.session.identity())
        .map(|evaluation| cad_core::manufacturing::cut_list_evaluated(&doc, evaluation));
    if let Some(Err(error)) = &cut_list {
        ui.small(error.to_string());
    }
    if let Some(Ok(rows)) = cut_list {
        for row in rows {
            ui.small(format!(
                "{} · {:.3} mm · qty {}",
                row.description, row.length_mm, row.quantity
            ));
        }
    }
    if let Ok(patterns) = cad_core::manufacturing::flat_patterns(&doc) {
        for (id, length, width, _) in patterns {
            ui.small(format!("Flat {id}: {length:.3} × {width:.3} mm"));
        }
    }
    ui.separator();
    ui.label("Bend allowance calculator");
    let mut values = [90., 2., 1., 0.42, 100., 50.];
    for (index, (name, v)) in [
        "Bend angle (deg)",
        "Inside radius (mm)",
        "Thickness (mm)",
        "K factor",
        "First straight (mm)",
        "Second straight (mm)",
    ]
    .into_iter()
    .zip(values.iter_mut())
    .enumerate()
    {
        let key = format!("bend/{index}");
        let mut s = w.fields.get(&key).cloned().unwrap_or(v.to_string());
        ui.horizontal(|ui| {
            ui.label(name);
            ui.text_edit_singleline(&mut s);
        });
        *v = s.parse().unwrap_or(*v);
        w.fields.insert(key, s);
    }
    let b = cad_core::manufacturing::Bend {
        angle_deg: values[0],
        inside_radius_mm: values[1],
        thickness_mm: values[2],
        k_factor: values[3],
    };
    if let Ok(allowance) = b.allowance() {
        ui.label(format!("Bend allowance {allowance:.4} mm"));
        if let Ok(length) = cad_core::manufacturing::flat_length(&values[4..], &[b]) {
            ui.label(format!("Developed neutral length {length:.4} mm"));
        }
    }
    ui.horizontal(|ui| {
        if ui.button("Export profiles DXF").clicked() {
            export_format(w, "dxf", "profiles.dxf");
        }
        if ui.button("Export STL").clicked() {
            export_format(w, "stl", "assembly.stl");
        }
    });
}
fn comments(ui: &mut egui::Ui, w: &mut Workspace) {
    let doc = w.session.visible().clone();
    let mut author = w
        .fields
        .get("review-author")
        .cloned()
        .unwrap_or_else(|| std::env::var("USER").unwrap_or("Engineer".into()));
    ui.horizontal(|ui| {
        ui.label("Author");
        ui.text_edit_singleline(&mut author);
    });
    w.fields.insert("review-author".into(), author.clone());
    ui.add(
        egui::TextEdit::multiline(&mut w.review_text)
            .desired_width(f32::INFINITY)
            .desired_rows(3),
    );
    if ui.button("Add local comment to active entity").clicked() && !w.review_text.trim().is_empty()
    {
        let target = w
            .view
            .active_node
            .clone()
            .or(w.view.active_part.clone())
            .unwrap_or(doc.root.clone());
        w.dispatch(Command::PutComment {
            comment: ReviewComment {
                id: id(),
                target,
                text: w.review_text.clone(),
                author,
                resolved: false,
            },
        });
        w.review_text.clear();
    }
    egui::ScrollArea::vertical().show(ui, |ui| {
        for c in &doc.comments {
            ui.separator();
            ui.strong(format!("{} · {}", c.author, c.target));
            ui.label(&c.text);
            let mut resolved = c.resolved;
            if ui.checkbox(&mut resolved, "Resolved").changed() {
                w.dispatch(Command::PutComment {
                    comment: ReviewComment {
                        resolved,
                        ..c.clone()
                    },
                });
            }
        }
    });
}
fn library_editor(ui: &mut egui::Ui, w: &mut Workspace) {
    ui.label("Component references retain the exact source revision and resource bytes. Updates enter a preview; linked definitions stay immutable. Make an instance unique to edit its geometry independently.");
    if ui.button("Link local component project…").clicked()
        && let Some(source) = rfd::FileDialog::new()
            .add_filter("TCAD project", &["json"])
            .pick_file()
    {
        let mut document = w.session.visible().clone();
        match cad_core::library::link(&mut document, &w.root(), &source, None, None) {
            Ok(_) => w.dispatch(Command::ReplaceDocument { document }),
            Err(e) => w.status = e.to_string(),
        }
    }
    let links = w
        .session
        .visible()
        .libraries
        .values()
        .cloned()
        .collect::<Vec<_>>();
    egui::ScrollArea::vertical().show(ui, |ui| {
        for link in links {
            ui.push_id(&link.id, |ui| {
                ui.separator();
                ui.strong(&link.name);
                ui.small(format!(
                    "Source revision {} · {}",
                    link.source_revision,
                    &link.source_sha256[..link.source_sha256.len().min(16)]
                ));
                ui.small(&link.source_path);
                ui.horizontal(|ui| {
                    if ui.button("Check update").clicked() {
                        w.status = match cad_core::library::available_update(&link, &w.root()) {
                            Ok(Some((revision, _))) => {
                                format!("{} has source revision {revision} available", link.name)
                            }
                            Ok(None) => "The exact source snapshot is current".into(),
                            Err(e) => e.to_string(),
                        };
                    }
                    if ui.button("Preview update").clicked() {
                        let mut document = w.session.visible().clone();
                        match cad_core::library::link(
                            &mut document,
                            &w.root(),
                            std::path::Path::new(&link.source_path),
                            None,
                            Some(&link.id),
                        ) {
                            Ok(_) => w.dispatch(Command::ReplaceDocument { document }),
                            Err(e) => w.status = e.to_string(),
                        }
                    }
                    if ui.button("Locate source…").clicked()
                        && let Some(source) = rfd::FileDialog::new()
                            .add_filter("TCAD project", &["json"])
                            .pick_file()
                    {
                        let mut document = w.session.visible().clone();
                        match cad_core::library::link(
                            &mut document,
                            &w.root(),
                            &source,
                            None,
                            Some(&link.id),
                        ) {
                            Ok(_) => w.dispatch(Command::ReplaceDocument { document }),
                            Err(e) => w.status = e.to_string(),
                        }
                    }
                });
            });
        }
    });
}
fn versions(ui: &mut egui::Ui, w: &mut Workspace) {
    ui.label("Local Git repositories work without an account. Commit the saved, committed design; use branches for alternatives.");
    ui.horizontal_wrapped(|ui| {
        if ui.button("Initialize repository").clicked() {
            git(w, &["init"]);
        }
        if ui.button("Status").clicked() {
            git(w, &["status", "--short"]);
        }
        if ui.button("History").clicked() {
            git(
                w,
                &["log", "-20", "--date=short", "--format=%h  %ad  %an  %s"],
            );
        }
        if ui.button("Branches").clicked() {
            git(w, &["branch", "--list"]);
        }
        if ui.button("Fetch remotes").clicked() {
            git(w, &["fetch", "--all"]);
        }
        if ui.button("Push current branch").clicked() {
            git(w, &["push"]);
        }
    });
    ui.horizontal(|ui| {
        ui.text_edit_singleline(&mut w.commit_message);
        if ui.button("Save and commit").clicked() {
            let saved = save(w, false);
            if saved && !w.commit_message.trim().is_empty() && w.project.is_some() {
                let message = w.commit_message.clone();
                let manifest = w
                    .project
                    .as_ref()
                    .filter(|p| p.extension().is_some_and(|e| e == "json"))
                    .and_then(|p| p.file_name())
                    .and_then(|p| p.to_str())
                    .unwrap_or("project.json")
                    .to_string();
                w.jobs.submit(crate::background::Job::Commit {
                    root: w.root(),
                    manifest,
                    message,
                });
                w.status = "Saving design commit".into();
            }
        }
    });
    ui.horizontal(|ui|{ui.text_edit_singleline(&mut w.branch_name);if ui.button("Create branch").clicked(){let branch=w.branch_name.clone();git(w,&["branch","--",&branch]);}
if ui.button("Switch branch / revision").clicked(){if w.session.draft.is_some()||w.saved_source!=w.session.committed.fingerprint(){w.status="Save committed changes and apply or cancel the draft before switching revisions".into();}else{let branch=w.branch_name.clone();match Git.command(&w.root(),&["switch","--",&branch]){Ok(text)=>{w.git_text=text;if let Some(path)=w.project.clone(){match FileStore.load(&path){Ok(d)=>w.replace(d,Some(path)),Err(e)=>w.status=e.to_string()}}},Err(e)=>w.status=e.to_string()}}}
if ui.button("Compare revision").clicked(){let revision=w.branch_name.clone();let root=crate::recovery_directory().join("comparisons").join(cad_core::id());match cad_core::version_control::snapshot_resources(&w.root(),&revision,&root){Ok(d)=>compare(w,d,root),Err(e)=>w.status=e.to_string()}}});
    ui.horizontal(|ui| {
        if ui.button("Compare project…").clicked()
            && let Some(path) = rfd::FileDialog::new()
                .add_filter("TCAD project", &["json"])
                .pick_file()
        {
            match FileStore.load(&path) {
                Ok(d) => compare(w, d, cad_core::persistence::project_root(&path)),
                Err(e) => w.status = e.to_string(),
            }
        }
        if ui.button("Clear comparison").clicked() {
            w.compare = None;
            w.compare_document = None;
            w.rendered_key.clear();
        }
    });
    if let Some(d) = &w.compare_document {
        ui.label(format!("Blue geometry: {} revision {}", d.name, d.revision));
        if let Ok(changes) = cad_core::version_control::diff(d, &w.session.committed) {
            egui::ScrollArea::vertical()
                .id_salt("semantic-diff")
                .max_height(180.)
                .show(ui, |ui| {
                    for c in changes {
                        ui.small(&c.path);
                        ui.monospace(format!(
                            "{} → {}",
                            c.before.map(|v| v.to_string()).unwrap_or("∅".into()),
                            c.after.map(|v| v.to_string()).unwrap_or("∅".into())
                        ));
                    }
                });
        }
    }
    egui::ScrollArea::both()
        .id_salt("git-output")
        .max_height(220.)
        .show(ui, |ui| {
            ui.monospace(&w.git_text);
        });
}
fn compare(w: &mut Workspace, d: Document, root: PathBuf) {
    let identity = RequestIdentity {
        protocol: PROTOCOL_VERSION,
        generation: w.session.generation,
        base_revision: d.revision,
        draft: None,
        edit: 0,
        source: d.fingerprint(),
        backend: BACKEND.into(),
    };
    w.compare_document = Some(d.clone());
    w.compare_worker.submit(WorkerRequest::Evaluate {
        identity,
        document: d,
        project_root: root.to_string_lossy().into(),
        tolerance: w.tolerance(),
        operation_preview: None,
    });
    w.status = "Evaluating comparison revision".into();
}
