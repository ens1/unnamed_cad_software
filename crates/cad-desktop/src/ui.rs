use crate::*;
use bevy_egui::{EguiContexts, egui};
use cad_core::{
    commands::Command,
    persistence::{FileStore, ProjectStore},
    presentation::{Region, SemanticElement},
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
    if let Some(public_key) = crate::git_connections_ui::take_clipboard(&mut w) {
        ctx.copy_text(public_key);
    }
    if let Some(url) = crate::content_repositories_ui::take_clipboard(&mut w) {
        ctx.copy_text(url);
    }
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

    if w.view.drawing_workspace && w.dialog.is_none() {
        crate::drawing_ui::open_workspace(&mut w);
    }

    let size = ctx.content_rect().size();
    crate::presentation_controls::begin_frame(&w.theme, &w.view.active_layout);
    crate::quantity_display::begin_frame(&w.session.visible().settings);
    let semantics = crate::presentation_composition::semantics(&w);
    // Only a viewport rendered during this frame may retain transient hover feedback.
    w.hover_guide = None;
    w.hover_viewport_seen = false;
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
    crate::assembly_motion_ui::finish_ui(&mut w, ctx);
    if !w.hover_viewport_seen {
        w.hover_cache = crate::viewport_guides::Cache::default();
    }
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
    // UI edits happen after worker polling. Wake once at the validation deadline
    // instead of rendering the entire workspace continuously while it is idle.
    let validation_delay = std::time::Duration::from_millis(161);
    if w.changed.elapsed() < validation_delay {
        crate::redraw::request_after(validation_delay.saturating_sub(w.changed.elapsed()));
    }
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
    if crate::publish_ui::busy(w) {
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
            crate::publish_ui::action(w, "publish.cancel", None);
        }
        return;
    }
    if w.dialog.as_deref() == Some("publish") {
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
            crate::publish_ui::action(w, "publish.close", None);
        } else if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::Enter)) {
            crate::publish_ui::action(w, "publish.run", None);
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
        if w.dialog.as_deref() == Some("sketch-block-library") {
            crate::sketch_block_library_ui::close(w);
        } else if crate::command_palette::escape(w)
            || crate::drawing_interaction_ui::escape(w)
            || crate::assembly_motion_ui::cancel_drag(w)
            || crate::component_pattern_ui::picking_direction(w)
                && crate::component_pattern_ui::action(w, "pattern.direction-cancel", &Value::Null)
        {
            ctx.input_mut(|i| {
                i.consume_key(egui::Modifiers::NONE, egui::Key::Escape);
            });
        } else if w.selection_popup.take().is_some() {
            ctx.input_mut(|i| {
                i.consume_key(egui::Modifiers::NONE, egui::Key::Escape);
            });
            if w.dialog.is_none() {
                crate::history::latest(w);
            }
        } else if crate::sketch3d_ui::escape(w) {
            ctx.input_mut(|i| {
                i.consume_key(egui::Modifiers::NONE, egui::Key::Escape);
            });
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
    if shortcuts.1 && w.dialog.as_deref() != Some("commands") {
        if w.sketch
            .as_ref()
            .is_some_and(|sketch| sketch.dimension.is_some())
        {
            crate::sketch_ui::finish_dimension(w);
        } else {
            w.confirm_tool();
        }
    }
    if shortcuts.2 {
        save(w, false);
    }
    if shortcuts.3 && !ctx.egui_wants_keyboard_input() {
        history_command(w, shortcuts.4);
    }
    if ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::N)) {
        action(w, "file.new-part");
    }
    if ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::O)) {
        action(w, "file.open");
    }
    if !ctx.egui_wants_keyboard_input() && (w.sketch.is_some() || crate::sketch3d_ui::active(w)) {
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
        if w.dialog.as_deref() == Some("drawings") {
            crate::drawing_interaction_ui::action(w, "drawing.interact-fit", None);
        } else {
            viewport::fit(w, shortcuts.4);
        }
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
    if crate::drawing_ui::is_workspace(w) {
        crate::drawing_ui::tree(ui, w);
    } else {
        crate::cad_ui::tree(ui, w);
    }
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
    registry.register("structural-member-editor", |ui, w, _| {
        crate::structural_member_ui::show(ui, w);
    });
    registry.register("sketch3d-editor", |ui, w, _| {
        crate::sketch3d_ui::show(ui, w);
    });
    registry.register("structural-member-locator", |ui, w, _| {
        if let Some(element) = crate::structural_member_ui::semantics(w)
            .into_iter()
            .find(|element| element.role == "structural-member-locator")
        {
            crate::structural_member_ui::locator(ui, w, &element);
        }
    });
    registry.register("preview-status", |ui, w, _| preview_status(ui, w));
    registry.register("task-history-panel", |ui, w, _| {
        crate::cad_ui::task_history(ui, w)
    });
    registry.register("project-location", |ui, w, _| {
        crate::document_identity_ui::location_control(ui, w)
    });
    registry.register("document-identity-editor", |ui, w, _| {
        crate::document_identity_ui::controls(ui, w)
    });
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
            &context.region.id,
        )
    });
    registry.register("assembly-editor", |ui, w, _| assembly_editor(ui, w));
    registry.register("camera-views-editor", |ui, w, _| {
        crate::camera_views_ui::show(ui, w)
    });
    registry.register("standard-parts-editor", |ui, w, _| {
        crate::standard_parts_ui::show(ui, w)
    });
    registry.register("model-properties-editor", |ui, w, _| {
        crate::properties_ui::show(ui, w)
    });
    registry.register("sensors-editor", |ui, w, _| crate::sensors_ui::show(ui, w));
    registry.register("tolerance-stacks-editor", |ui, w, _| {
        crate::tolerance_stacks_ui::show(ui, w)
    });
    registry.register("command-palette", |ui, w, _| {
        crate::command_palette::show(ui, w)
    });
    registry.register("inspection-editor", |ui, w, _| {
        crate::inspection_ui::controls(ui, w)
    });
    registry.register("sketch-block-library-editor", |ui, w, _| {
        crate::sketch_block_library_ui::show(ui, w)
    });
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
    registry.register("drawing-tree", |ui, w, _| crate::drawing_ui::tree(ui, w));
    registry.register("drawing-completion", |ui, w, _| {
        crate::drawing_ui::footer(ui, w)
    });
    registry.register("manufacturing-editor", |ui, w, _| manufacturing(ui, w));
    registry.register("versions-editor", |ui, w, _| versions(ui, w));
    registry.register("repositories-editor", |ui, w, _| {
        crate::repositories_ui::show(ui, w)
    });
    registry.register("publish-editor", |ui, w, _| crate::publish_ui::show(ui, w));
    registry.register("git-connections-editor", |ui, w, _| {
        crate::git_connections_ui::show(ui, w)
    });
    registry.register("content-repositories-editor", |ui, w, _| {
        crate::content_repositories_ui::show(ui, w)
    });
    registry.register("git-connection-secret", |ui, w, context| {
        let binding = context
            .layout
            .binding_for_in(&w.view.active_layout, context.region);
        let bound = binding.as_ref().and_then(|binding| binding.bind.as_deref());
        for element in context.semantics.iter().filter(|element| {
            element.role == "git-connection-secret" && bound.is_none_or(|id| id == element.id)
        }) {
            if let Some(label) = binding.as_ref().and_then(|binding| binding.label.as_ref()) {
                let mut presented = element.clone();
                presented.label.clone_from(label);
                crate::git_connections_ui::secret_control(ui, w, &presented);
            } else {
                crate::git_connections_ui::secret_control(ui, w, element);
            }
        }
    });
    registry.register("feature-transfer-editor", |ui, w, _| {
        crate::feature_transfer_ui::show(ui, w)
    });
    registry.register("component-fork-editor", |ui, w, _| {
        crate::component_fork_ui::show(ui, w)
    });
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
    registry.register("sketch-issues", |ui, w, _| {
        crate::sketch_issues_ui::controls(ui, w)
    });
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
    if element.data["theme_preview"] == true
        && matches!(element.role.as_str(), "apply-action" | "cancel-action")
    {
        crate::presentation_controls::button(
            ui,
            &element.id,
            &element.role,
            &["confirmation-control", "theme-preview"],
            &element.label,
            true,
            false,
        )
        .on_hover_text("Appearance preview; no model changes");
        return;
    }
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
    if widget == "auto" && crate::selection_ui::render_edge_row(ui, w, element) {
        return;
    }
    if widget == "auto" && element.role.starts_with("sketch3d-") {
        crate::sketch3d_ui::render_control(ui, w, element);
        return;
    }
    if widget == "auto"
        && matches!(
            element.role.as_str(),
            "structural-member-field"
                | "structural-member-action"
                | "structural-member-note"
                | "structural-member-locator"
        )
    {
        crate::structural_member_ui::render_control(ui, w, element);
        return;
    }
    if matches!(
        element.role.as_str(),
        "production-field" | "production-action"
    ) {
        crate::production_ui::semantic_control(ui, w, element);
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
    if matches!(
        element.role.as_str(),
        "assembly-motion-field"
            | "assembly-motion-mode"
            | "assembly-motion-pivot"
            | "assembly-motion-scope"
    ) {
        crate::assembly_motion_ui::semantic_control(ui, w, element);
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
            .unwrap_or(&descriptor.label);
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
pub(crate) fn semantic_action(w: &mut Workspace, element: &SemanticElement, command: &str) {
    if crate::publish_ui::action(w, command, Some(&element.data)) {
        return;
    }
    if crate::publish_ui::busy(w) {
        w.status = "Wait for Publish to finish or cancel it before using another command".into();
        return;
    }
    if command == "content-repositories" || command.starts_with("content-repositories.") {
        crate::content_repositories_ui::action(w, command, Some(&element.data));
        return;
    }
    if command == "git-connections" || command.starts_with("git-connections.") {
        crate::git_connections_ui::action(w, command, Some(&element.data));
        return;
    }
    if matches!(command, "repositories.cancel" | "repositories.auth-browser") {
        crate::repositories_ui::action(w, command, Some(&element.data));
        return;
    }
    if crate::git_ui::busy(w) {
        w.status = "Wait for the Git operation to finish".into();
        return;
    }
    if crate::drawing_interaction_ui::active(w)
        && !matches!(command, "draft.cancel" | "operation.cancel" | "tool.cancel")
    {
        w.status = "Finish or cancel the paper gesture before using another command".into();
        return;
    }
    if crate::settings::action(w, command) {
        return;
    }
    if w.settings.is_some() {
        w.status = "Apply or cancel Settings before using another command".into();
        return;
    }
    if crate::repositories_ui::action(w, command, Some(&element.data))
        || crate::component_fork_ui::action(w, command, Some(&element.data))
        || crate::git_ui::action(w, command, Some(&element.data))
        || crate::feature_transfer_ui::action(w, command, Some(&element.data))
        || crate::readme_preview_ui::action(w, command)
    {
        return;
    }
    if crate::document_identity_ui::semantic_action(w, command, &element.data) {
        return;
    }
    if command == "drawings" {
        crate::drawing_ui::open_workspace(w);
        return;
    }
    if crate::drawing_ui::is_workspace(w)
        && (command.starts_with("feature.")
            || command.starts_with("sketch.")
            || command.starts_with("assembly.")
            || command == "part.new")
        && !crate::drawing_ui::leave_workspace(w)
    {
        return;
    }
    if element.role == "selection-action" && matches!(command, "assembly.move" | "assembly.rotate")
    {
        if let Some(target) = element
            .data
            .get("target")
            .and_then(|value| serde_json::from_value(value.clone()).ok())
        {
            crate::selection_ui::execute(w, &target, command);
        }
        return;
    }
    if command == "production.action" {
        if let Err(error) = crate::production_ui::dispatch(w, element) {
            w.status = error.to_string();
        }
        return;
    }
    if command == "camera-views" || command.starts_with("camera-view.") {
        crate::camera_views_ui::semantic_action(w, element, command);
        return;
    }
    if command.starts_with("assembly.display-state-") {
        crate::display_states_ui::semantic_action(w, element, command);
        return;
    }
    if crate::sketch_issues_ui::action(w, command, &element.data)
        || crate::structural_member_ui::action(w, command, &element.data)
        || crate::sketch3d_ui::action(w, command, Some(&element.data))
        || crate::fabrication_sources_ui::action(w, command, &element.data)
        || crate::weld_beads_ui::action(w, command, Some(&element.data))
        || crate::sketch_block_library_ui::action(w, command, Some(&element.data))
        || crate::command_palette::action(w, command, Some(&element.data))
        || crate::inspection_ui::action(w, command, Some(&element.data))
        || crate::reference_repair_ui::action(w, command, Some(&element.data))
        || crate::properties_ui::action(w, command, Some(&element.data))
        || crate::sensors_ui::action(w, command, Some(&element.data))
        || crate::standard_parts_ui::action(w, command, Some(&element.data))
        || crate::tolerance_stacks_ui::action(w, command, Some(&element.data))
        || crate::materials_ui::action(w, command, Some(&element.data))
        || crate::finish_library_ui::action(w, command, Some(&element.data))
        || crate::drawing_ui::semantic_action(w, element, command)
        || crate::sketch_relations_ui::semantic_action(w, element, command)
        || crate::feature_order_ui::action(w, command, Some(&element.data))
        || crate::operation_editor_ui::action(w, command, Some(&element.data))
        || crate::drawing_interaction_ui::action(w, command, Some(&element.data))
        || crate::drawing_editor_ui::action(w, command, Some(&element.data))
        || crate::component_pattern_ui::action(w, command, &element.data)
        || crate::viewport::picking_action(w, command, Some(&element.data))
        || crate::fabrication_ui::state_action(w, command, Some(&element.data))
        || crate::workspace_controls::task_action(w, command, &element.data)
        || crate::assembly_tree_ui::action(w, command, Some(&element.data))
        || crate::assembly_motion_ui::action(w, command, Some(&element.data))
        || crate::assembly_tools_ui::action(w, command, Some(&element.data))
        || crate::assembly_welds_ui::action(w, command, Some(&element.data))
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
                | "sketch-relation-action"
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
pub(crate) fn history_command(w: &mut Workspace, redo: bool) {
    if crate::publish_ui::busy(w) {
        w.status = "Wait for Publish to finish or cancel it before changing history".into();
        return;
    }
    w.selection_popup = None;
    if crate::drawing_interaction_ui::active(w) {
        crate::drawing_interaction_ui::escape(w);
    } else if w.sketch.is_some() {
        crate::sketch::undo_point(w);
    } else if crate::sketch3d_ui::active(w) {
        crate::sketch3d_ui::history_command(w, redo);
    } else if w.tool_checkpoint.is_some() {
        w.cancel_tool();
    } else if redo {
        w.session.redo();
    } else {
        w.session.undo();
    }
    w.fields
        .retain(|key, _| !key.starts_with("drawing-editor:") && key != "drawing-source-invalid");
    crate::exploded_views_ui::clear_selection(w);
    w.requested = None;
    w.changed = Instant::now();
}
pub(crate) fn semantic_field(ui: &mut egui::Ui, w: &mut Workspace, element: &SemanticElement) {
    let descriptor = element.value_descriptor();
    let value = crate::presentation_composition::current_value(w, element);
    let editable = descriptor.editable;
    let editor_generation = element
        .data
        .get("editor")
        .and_then(Value::as_str)
        .unwrap_or("");
    let buffer_error_key = format!(
        "presentation-field-error:{}:{editor_generation}",
        element.id
    );
    let role = if element
        .data
        .get("setter")
        .and_then(Value::as_str)
        .is_some_and(|setter| {
            setter.starts_with("appearance-")
                || matches!(
                    setter,
                    "operation-editor"
                        | "structural-member"
                        | "sketch3d"
                        | "component-placement"
                        | "component-insert"
                        | "material-ui"
                        | "finish-library"
                        | "display-state-field"
                        | "display-state-source"
                        | "display-state-selection"
                        | "sensor-editor"
                        | "sketch-block-library"
                        | "command-palette"
                        | "inspection-report"
                        | "camera-view"
                        | "standard-parts"
                        | "tolerance-stack-editor"
                        | "properties-ui"
                        | "model-property"
                        | "material-editor"
                        | "drawing-editor"
                        | "drawing-interaction"
                        | "production"
                        | "configuration-dimension"
                        | "assembly-weld"
                        | "assembly-copy"
                        | "assembly-group"
                        | "repositories"
                        | "git-connections"
                        | "content-repositories"
                        | "publish"
                )
        }) {
        element.role.as_str()
    } else {
        "field-control"
    };
    let mut field_classes = vec!["semantic-control", "field-control"];
    if let Some(classes) = element.data.get("classes").and_then(Value::as_array) {
        field_classes.extend(classes.iter().filter_map(Value::as_str));
    }
    let field_state = format!(
        "{} {}",
        if w.fields.contains_key(&buffer_error_key) {
            "invalid"
        } else {
            &element.state
        },
        if editable { "editable" } else { "readonly" }
    );
    let props =
        crate::presentation_controls::properties(&element.id, role, &field_classes, &field_state);
    if props.get("display").is_some_and(|value| value == "none") {
        return;
    }
    let mut appearance_error = None;
    let target = crate::presentation_controls::current_target().map(|mut parent| {
        if parent.node.id == element.id {
            parent.node.role = role.into();
            parent
                .node
                .states
                .extend(field_state.split_whitespace().map(str::to_owned));
            parent
        } else {
            parent.child(
                &element.id,
                role,
                &field_classes
                    .iter()
                    .map(|c| (*c).into())
                    .collect::<Vec<_>>(),
                &field_state,
            )
        }
    });
    let request_focus = element
        .data
        .get("focus_request")
        .and_then(Value::as_str)
        .is_some_and(|key| {
            key == format!("focus-request:{}", element.id) && w.fields.remove(key).is_some()
        });
    let mut draw = |ui: &mut egui::Ui| {
        ui.scope(|ui| {
            crate::presentation_controls::apply_properties(ui, &props);
            let available = if ui.available_width().is_finite() {
                ui.available_width().max(0.)
            } else {
                ui.clip_rect().width().max(0.)
            };
            let label_id = format!("{}:label", element.id);
            let unit_id = format!("{}:unit", element.id);
            let unit = descriptor
                .unit
                .as_deref()
                .filter(|unit| !unit.is_empty() && !value.is_boolean())
                .filter(|_| {
                    // Literal expressions already carry a visible unit. Their
                    // source unit is available in semantic data and the tooltip.
                    value.as_str().is_none_or(|source| {
                        crate::quantity_display::expression_with(
                            &w.session.visible().settings,
                            source,
                        )
                        .is_none_or(|display| display.split_whitespace().count() != 2)
                    })
                });
            let label_props = crate::presentation_controls::properties(
                &label_id,
                "field-label",
                &field_classes,
                &field_state,
            );
            let unit_props = crate::presentation_controls::properties(
                &unit_id,
                "field-unit",
                &field_classes,
                "ready",
            );
            let label_width = crate::presentation_controls::caption_width(
                ui,
                &label_props,
                &element.label,
                available,
            );
            let unit_width = unit
                .map(|unit| {
                    crate::presentation_controls::caption_width(ui, &unit_props, unit, available)
                })
                .unwrap_or(0.);
            let caption_gap = if label_width > 0. && unit_width > 0. {
                ui.spacing().item_spacing.x
            } else {
                0.
            };
            let caption_width = (label_width + caption_gap + unit_width).min(available);
            let minimum_control = crate::presentation_controls::length(
                &props,
                "min-width",
                available,
                crate::presentation_controls::number(
                    &props,
                    "width",
                    crate::presentation_controls::number(
                        &props,
                        "font-size",
                        egui::TextStyle::Body.resolve(ui.style()).size,
                    ) * 7.,
                ),
            )
            .max(crate::presentation_controls::number(
                &props,
                "min-target-size",
                0.,
            ));
            let row = props
                .get("flex-direction")
                .is_some_and(|value| value == "row")
                && element.data.get("multiline").and_then(Value::as_bool) != Some(true)
                && caption_width + ui.spacing().item_spacing.x + minimum_control <= available;
            let caption = |ui: &mut egui::Ui| {
                let width = ui.available_width().max(0.);
                let unit_width = unit_width.min(width);
                let label_width = label_width.min((width - unit_width - caption_gap).max(0.));
                if label_width > 0. {
                    crate::presentation_controls::bounded_label(
                        ui,
                        &label_id,
                        "field-label",
                        &field_classes,
                        &element.label,
                        &field_state,
                        label_width,
                    )
                    .on_hover_text(&descriptor.accessibility_label);
                }
                if let Some(unit) = unit.filter(|_| unit_width > 0.) {
                    crate::presentation_controls::bounded_label(
                        ui,
                        &unit_id,
                        "field-unit",
                        &field_classes,
                        unit,
                        "ready",
                        unit_width,
                    );
                }
            };
            let mut control = |ui: &mut egui::Ui| {
                let control_size = crate::presentation_controls::field_size(ui, &props);
                ui.spacing_mut().interact_size.y = control_size.y;
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
                    egui::ComboBox::from_id_salt((&element.id, editor_generation))
                        .selected_text(selected_label)
                        .width(control_size.x)
                        .wrap_mode(egui::TextWrapMode::Truncate)
                        .show_ui(ui, |ui| {
                            ui.add_enabled_ui(editable, |ui| {
                                let filter_key = format!(
                                    "presentation-choice-filter:{}:{editor_generation}",
                                    element.id
                                );
                                let query = w.fields.entry(filter_key).or_default();
                                ui.add(
                                    egui::TextEdit::singleline(query)
                                        .hint_text("Filter choices…")
                                        .desired_width(ui.available_width()),
                                );
                                let search = query.to_lowercase();
                                let indices = if search.is_empty() {
                                    None
                                } else {
                                    Some(
                                        descriptor
                                            .choices
                                            .iter()
                                            .enumerate()
                                            .filter_map(|(index, choice)| {
                                                let label = choice
                                                    .get("label")
                                                    .and_then(Value::as_str)
                                                    .or_else(|| {
                                                        choice
                                                            .get("value")
                                                            .unwrap_or(choice)
                                                            .as_str()
                                                    })
                                                    .map(str::to_owned)
                                                    .unwrap_or_else(|| choice.to_string());
                                                label
                                                    .to_lowercase()
                                                    .contains(&search)
                                                    .then_some(index)
                                            })
                                            .collect::<Vec<_>>(),
                                    )
                                };
                                let count =
                                    indices.as_ref().map_or(descriptor.choices.len(), Vec::len);
                                let menu_props = crate::presentation_controls::properties(
                                    &format!("{}:choices", element.id),
                                    "field-choices",
                                    &field_classes,
                                    &field_state,
                                );
                                egui::ScrollArea::vertical()
                                    .max_height(crate::presentation_controls::number(
                                        &menu_props,
                                        "max-height",
                                        240.,
                                    ))
                                    .show_rows(
                                        ui,
                                        ui.spacing().interact_size.y,
                                        count,
                                        |ui, rows| {
                                            for row in rows {
                                                let index = indices
                                                    .as_ref()
                                                    .map_or(row, |indices| indices[row]);
                                                let choice = &descriptor.choices[index];
                                                let candidate =
                                                    choice.get("value").unwrap_or(choice);
                                                let label = choice
                                                    .get("label")
                                                    .and_then(Value::as_str)
                                                    .or_else(|| candidate.as_str())
                                                    .map(str::to_string)
                                                    .unwrap_or_else(|| candidate.to_string());
                                                ui.selectable_value(
                                                    &mut next,
                                                    candidate.clone(),
                                                    label,
                                                );
                                            }
                                        },
                                    );
                                if count == 0 {
                                    ui.weak("No matching choices");
                                }
                            });
                        });
                    return (next != value).then_some(next);
                }
                if let Some(mut checked) = value.as_bool() {
                    return ui
                        .add_enabled_ui(editable, |ui| {
                            ui.add_sized(control_size, egui::Checkbox::new(&mut checked, ""))
                        })
                        .inner
                        .on_hover_text(&descriptor.accessibility_label)
                        .changed()
                        .then_some(Value::Bool(checked));
                }
                let current = value
                    .as_str()
                    .map(str::to_string)
                    .unwrap_or_else(|| value.to_string());
                let key = format!("presentation-field:{}:{editor_generation}", element.id);
                let keep_invalid_text = w.fields.contains_key(&buffer_error_key)
                    || crate::assembly_tools_ui::field_is_invalid(w, element)
                    || crate::component_fork_ui::field_is_invalid(w, element)
                    || crate::properties_ui::field_is_invalid(w, element)
                    || crate::sensors_ui::field_is_invalid(w, element)
                    || crate::appearances_ui::field_is_invalid(w, element)
                    || crate::sketch_block_library_ui::field_is_invalid(w, element)
                    || crate::camera_views_ui::field_is_invalid(w, element)
                    || crate::standard_parts_ui::field_is_invalid(w, element)
                    || crate::tolerance_stacks_ui::field_is_invalid(w, element)
                    || crate::materials_ui::field_is_invalid(w, element)
                    || crate::document_identity_ui::field_is_invalid(w, element)
                    || crate::components_ui::field_is_invalid(w, element)
                    || crate::assembly_welds_ui::field_is_invalid(w, element)
                    || crate::operation_editor_ui::field_is_invalid(w, element)
                    || crate::structural_member_ui::field_is_invalid(w, element)
                    || crate::sketch3d_ui::field_is_invalid(w, element)
                    || crate::drawing_editor_ui::field_is_invalid(w, element)
                    || crate::drawing_interaction_ui::field_is_invalid(w, element)
                    || crate::configuration_ui::field_is_invalid(w, element)
                    || crate::production_ui::field_is_invalid(w, element);
                let display =
                    crate::quantity_display::field(&w.session.visible().settings, element, &value);
                let edit_id = ui.make_persistent_id((&element.id, editor_generation));
                let focused = ui.memory(|memory| memory.has_focus(edit_id));
                let buffered = w
                    .fields
                    .entry(key.clone())
                    .or_insert(current.clone())
                    .clone();
                // A user layout may show several copies of this field. Compact
                // copies never overwrite the exact text of the focused copy.
                let mut text = if request_focus && !keep_invalid_text {
                    current.clone()
                } else if focused || keep_invalid_text {
                    buffered
                } else {
                    display.clone()
                };
                let editor = if element.data.get("multiline").and_then(Value::as_bool) == Some(true)
                {
                    egui::TextEdit::multiline(&mut text)
                        .code_editor()
                        .desired_rows(12)
                } else {
                    egui::TextEdit::singleline(&mut text)
                };
                let editor = editor.id(edit_id).desired_width(control_size.x);
                let response = ui
                    .add_enabled_ui(editable, |ui| {
                        if element.data.get("multiline").and_then(Value::as_bool) == Some(true)
                            && !props.contains_key("height")
                            && !props.contains_key("max-height")
                        {
                            ui.add(editor)
                        } else {
                            ui.add_sized(control_size, editor)
                        }
                    })
                    .inner
                    .on_hover_text(format!(
                        "{}\nSource: {current}",
                        descriptor.accessibility_label
                    ));
                if request_focus {
                    response.request_focus();
                }
                if (response.gained_focus() || request_focus)
                    && !response.changed()
                    && !keep_invalid_text
                {
                    w.fields.insert(key.clone(), current.clone());
                    if display != current {
                        crate::presentation_controls::select_exact_source(ui, edit_id, &current);
                    }
                }
                if !response.has_focus() && !response.changed() && !keep_invalid_text {
                    appearance_error = Some(None);
                }
                if response.changed() {
                    w.fields.insert(key, text.clone());
                    if value.is_string() {
                        Some(Value::String(text.clone()))
                    } else {
                        match serde_json::from_str(&text) {
                            Ok(value) => Some(value),
                            Err(_)
                                if element
                                    .data
                                    .get("accepts_quantity_text")
                                    .and_then(Value::as_bool)
                                    == Some(true) =>
                            {
                                Some(Value::String(text.clone()))
                            }
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
            };
            if row {
                ui.horizontal(|ui| {
                    ui.allocate_ui_with_layout(
                        egui::vec2(caption_width, ui.spacing().interact_size.y),
                        egui::Layout::left_to_right(egui::Align::Center),
                        |ui| {
                            ui.set_min_width(caption_width);
                            caption(ui);
                        },
                    );
                    control(ui)
                })
                .inner
            } else {
                ui.vertical(|ui| {
                    if caption_width > 0. {
                        ui.allocate_ui_with_layout(
                            egui::vec2(available, ui.spacing().interact_size.y),
                            egui::Layout::left_to_right(egui::Align::Center),
                            caption,
                        );
                    }
                    control(ui)
                })
                .inner
            }
        })
        .inner
    };
    let next = if let Some(target) = target {
        crate::presentation_controls::with_target(target, ui, &mut draw)
    } else {
        draw(ui)
    };
    if let Some(error) = appearance_error {
        if let Some(message) = &error {
            w.fields.insert(buffer_error_key.clone(), message.clone());
        } else {
            w.fields.remove(&buffer_error_key);
        }
        crate::document_identity_ui::field_error(w, element, error.clone());
        crate::properties_ui::field_error(w, element, error.clone());
        crate::sensors_ui::field_error(w, element, error.clone());
        crate::sketch_block_library_ui::field_error(w, element, error.clone());
        crate::camera_views_ui::field_error(w, element, error.clone());
        crate::standard_parts_ui::field_error(w, element, error.clone());
        crate::tolerance_stacks_ui::field_error(w, element, error.clone());
        crate::materials_ui::field_error(w, element, error.clone());
        crate::components_ui::field_error(w, element, error.clone());
        crate::assembly_tools_ui::field_error(w, element, error.clone());
        crate::component_fork_ui::field_error(w, element, error.clone());
        crate::assembly_welds_ui::field_error(w, element, error.clone());
        crate::operation_editor_ui::field_error(w, element, error.clone());
        crate::structural_member_ui::field_error(w, element, error.clone());
        crate::sketch3d_ui::field_error(w, element, error.clone());
        crate::drawing_editor_ui::field_error(w, element, error.clone());
        crate::drawing_interaction_ui::field_error(w, element, error.clone());
        crate::production_ui::record_field_error(w, element, error.clone());
        crate::configuration_ui::field_error(w, element, error.clone());
        crate::appearances_ui::field_error(w, element, error);
    }
    if let Some(next) = next {
        match crate::presentation_composition::set_value(w, element, next) {
            Ok(()) => {
                w.fields.remove(&buffer_error_key);
                crate::properties_ui::field_error(w, element, None);
                crate::document_identity_ui::field_error(w, element, None);
                crate::sensors_ui::field_error(w, element, None);
                crate::sketch_block_library_ui::field_error(w, element, None);
                crate::camera_views_ui::field_error(w, element, None);
                crate::standard_parts_ui::field_error(w, element, None);
                crate::tolerance_stacks_ui::field_error(w, element, None);
                crate::materials_ui::field_error(w, element, None);
                crate::components_ui::field_error(w, element, None);
                crate::assembly_tools_ui::field_error(w, element, None);
                crate::component_fork_ui::field_error(w, element, None);
                crate::assembly_welds_ui::field_error(w, element, None);
                crate::appearances_ui::field_error(w, element, None);
                crate::operation_editor_ui::field_error(w, element, None);
                crate::structural_member_ui::field_error(w, element, None);
                crate::sketch3d_ui::field_error(w, element, None);
                crate::drawing_editor_ui::field_error(w, element, None);
                crate::drawing_interaction_ui::field_error(w, element, None);
                crate::production_ui::record_field_error(w, element, None);
                crate::configuration_ui::field_error(w, element, None);
            }
            Err(error) => {
                w.fields.insert(buffer_error_key.clone(), error.to_string());
                crate::properties_ui::field_error(w, element, Some(error.to_string()));
                crate::document_identity_ui::field_error(w, element, Some(error.to_string()));
                crate::sensors_ui::field_error(w, element, Some(error.to_string()));
                crate::sketch_block_library_ui::field_error(w, element, Some(error.to_string()));
                crate::camera_views_ui::field_error(w, element, Some(error.to_string()));
                crate::standard_parts_ui::field_error(w, element, Some(error.to_string()));
                crate::tolerance_stacks_ui::field_error(w, element, Some(error.to_string()));
                crate::materials_ui::field_error(w, element, Some(error.to_string()));
                crate::components_ui::field_error(w, element, Some(error.to_string()));
                crate::assembly_tools_ui::field_error(w, element, Some(error.to_string()));
                crate::component_fork_ui::field_error(w, element, Some(error.to_string()));
                crate::assembly_welds_ui::field_error(w, element, Some(error.to_string()));
                w.status = error.to_string();
                crate::appearances_ui::field_error(w, element, Some(error.to_string()));
                crate::operation_editor_ui::field_error(w, element, Some(error.to_string()));
                crate::structural_member_ui::field_error(w, element, Some(error.to_string()));
                crate::sketch3d_ui::field_error(w, element, Some(error.to_string()));
                crate::drawing_editor_ui::field_error(w, element, Some(error.to_string()));
                crate::drawing_interaction_ui::field_error(w, element, Some(error.to_string()));
                crate::production_ui::record_field_error(w, element, Some(error.to_string()));
                crate::configuration_ui::field_error(w, element, Some(error.to_string()));
            }
        }
    }
    if let Some(message) = w.fields.get(&buffer_error_key) {
        crate::presentation_controls::label(
            ui,
            &format!("{}:buffer-diagnostic", element.id),
            "field-diagnostic",
            &field_classes,
            message,
            "error",
        );
    }
    for (index, message) in descriptor.diagnostics.iter().enumerate() {
        crate::presentation_controls::label(
            ui,
            &format!("{}:diagnostic:{index}", element.id),
            "field-diagnostic",
            &field_classes,
            message,
            "error",
        );
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
    viewport_id: &str,
) {
    if index == 0 {
        let scale = ui.ctx().pixels_per_point();
        w.viewport_size = [(r.width() * scale) as u32, (r.height() * scale) as u32];
        w.viewport_pixels_per_point = (w.viewport_size[0].clamp(32, 4096) as f32
            / r.width().max(1.))
        .min(w.viewport_size[1].clamp(32, 4096) as f32 / r.height().max(1.));
    }
    let response = ui.put(
        r,
        egui::Image::new(egui::load::SizedTexture::new(texture, r.size()))
            .fit_to_exact_size(r.size())
            .sense(egui::Sense::click_and_drag()),
    );
    if crate::drawing_ui::preview(w, ui, r, &response, viewport_id) {
        return;
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
    let cube = viewport::view_cube(w, ui, r);
    crate::operation_preview::viewport_status(ui, w, r);
    let motion_owned = crate::assembly_motion_ui::viewport(w, ui, r, &response, &[toolbar, cube]);
    if !motion_owned
        && !crate::sketch3d_ui::captured(w)
        && viewport::navigate(w, ui.ctx(), &response)
    {
        w.selection_popup = None;
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
    if !motion_owned
        && response.secondary_clicked()
        && w.sketch.is_none()
        && w.tool_checkpoint.is_none()
    {
        w.selection_popup = None;
        if let Some(pointer) = response.interact_pointer_pos() {
            if let Some(feature) = crate::sketch::pick(w, r, pointer)
                .or_else(|| crate::sketch3d_ui::pick_inactive(w, r, pointer))
            {
                crate::sketch::select_feature(w, feature, &response);
            } else if let Some(selected) = viewport::pick(w, r, pointer).into_iter().next() {
                viewport::select(w, selected);
            } else {
                w.selected = None;
                w.selected_component = None;
                w.selected_feature_path = None;
                w.view.active_node = None;
                w.context_selection = None;
                w.candidates.clear();
            }
        }
    }
    if !motion_owned && w.sketch.is_none() && !crate::sketch3d_ui::active(w) {
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
    crate::sketch3d_ui::overlay_inactive(w, ui, r);
    let blocked = !response.hovered()
        || motion_owned
        || ui.input(|input| {
            input
                .pointer
                .hover_pos()
                .is_some_and(|point| toolbar.contains(point) || cube.contains(point))
        });
    if response.hovered() {
        w.hover_viewport_seen = true;
        let mut hover_cache = std::mem::take(&mut w.hover_cache);
        if let Some(guide) = crate::viewport_guides::overlay(w, ui, r, blocked, &mut hover_cache) {
            w.hover_guide = Some(guide);
        }
        w.hover_cache = hover_cache;
    }
    if !motion_owned {
        crate::sketch::overlay(w, ui, r, &response, toolbar);
    }
    crate::sketch3d_ui::overlay(w, ui, r, &response, blocked || motion_owned);
    if w.sketch.is_none()
        && !crate::sketch3d_ui::active(w)
        && !motion_owned
        && response.clicked()
        && response
            .interact_pointer_pos()
            .is_some_and(|p| !toolbar.contains(p) && !cube.contains(p))
    {
        let p = response.interact_pointer_pos().unwrap_or(r.center());
        w.selection_popup = None;
        let direction_pick = crate::component_pattern_ui::capture_sketch_pick(w, r, p);
        let structural_pick = !direction_pick
            && crate::structural_member_ui::capture_click(
                w,
                r,
                p,
                ui.input(|input| input.modifiers),
            );
        let sketch_feature = if !direction_pick && !structural_pick && w.tool_checkpoint.is_none() {
            crate::sketch::pick(w, r, p).or_else(|| crate::sketch3d_ui::pick_inactive(w, r, p))
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
        } else if !direction_pick && !structural_pick {
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
                w.selected_component = None;
                w.geometry_selection.clear();
                w.selected_feature_path = None;
                w.view.active_node = None;
                w.context_selection = None;
            }
        }
    }
    if w.sketch.is_none()
        && !motion_owned
        && !crate::structural_member_ui::active(w)
        && !crate::sketch3d_ui::active(w)
        && response.hovered()
        && ui.input(|i| i.key_pressed(egui::Key::Tab))
        && !w.candidates.is_empty()
    {
        ui.input_mut(|input| {
            input.consume_key(egui::Modifiers::NONE, egui::Key::Tab);
        });
        ui.ctx()
            .memory_mut(|memory| memory.request_focus(response.id));
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
    if crate::publish_ui::action(w, cmd, None) {
        return;
    }
    if crate::publish_ui::busy(w) {
        w.status = "Wait for Publish to finish or cancel it before using another command".into();
        return;
    }
    if cmd == "content-repositories" || cmd.starts_with("content-repositories.") {
        crate::content_repositories_ui::action(w, cmd, None);
        return;
    }
    if cmd == "git-connections" || cmd.starts_with("git-connections.") {
        crate::git_connections_ui::action(w, cmd, None);
        return;
    }
    if matches!(cmd, "repositories.cancel" | "repositories.auth-browser") {
        crate::repositories_ui::action(w, cmd, None);
        return;
    }
    if crate::git_ui::busy(w) {
        w.status = "Wait for the Git operation to finish".into();
        return;
    }
    if w.settings.is_none() && !crate::drawing_interaction_ui::active(w) {
        if cmd == "drawings" {
            crate::drawing_ui::open_workspace(w);
            return;
        }
        if crate::drawing_ui::is_workspace(w)
            && (cmd.starts_with("feature.")
                || cmd.starts_with("sketch.")
                || cmd.starts_with("assembly.")
                || cmd == "part.new")
            && !crate::drawing_ui::leave_workspace(w)
        {
            return;
        }
    }
    if crate::drawing_interaction_ui::active(w)
        && !matches!(cmd, "draft.cancel" | "operation.cancel" | "tool.cancel")
    {
        w.status = "Finish or cancel the paper gesture before using another command".into();
        return;
    }
    if crate::settings::action(w, cmd) {
        return;
    }
    if w.settings.is_some() {
        w.status = "Apply or cancel Settings before using another command".into();
        return;
    }
    if crate::repositories_ui::action(w, cmd, None)
        || crate::workspace_controls::action(w, cmd)
        || crate::component_fork_ui::action(w, cmd, None)
        || crate::git_ui::action(w, cmd, None)
        || crate::feature_transfer_ui::action(w, cmd, None)
        || crate::readme_preview_ui::action(w, cmd)
        || crate::sketch3d_ui::action(w, cmd, None)
        || crate::document_identity_ui::action(w, cmd)
        || crate::sketch_overlay::action(w, cmd)
        || crate::properties_ui::action(w, cmd, None)
        || crate::sensors_ui::action(w, cmd, None)
        || crate::camera_views_ui::handle_command(w, cmd, None)
        || crate::standard_parts_ui::action(w, cmd, None)
        || crate::tolerance_stacks_ui::action(w, cmd, None)
        || crate::display_states_ui::handle_command(w, cmd, None)
        || crate::materials_ui::action(w, cmd, None)
        || crate::finish_library_ui::action(w, cmd, None)
        || crate::drawing_ui::action(w, cmd)
        || crate::components_ui::action(w, cmd)
        || crate::reference_repair_ui::action(w, cmd, None)
        || crate::sketch_relations_ui::action(w, cmd)
        || crate::selection_ui::set_action(w, cmd)
        || crate::feature_order_ui::action(w, cmd, None)
        || crate::operation_editor_ui::action(w, cmd, None)
        || crate::drawing_interaction_ui::action(w, cmd, None)
        || crate::drawing_editor_ui::action(w, cmd, None)
        || crate::assembly_motion_ui::action(w, cmd, None)
        || crate::assembly_tools_ui::action(w, cmd, None)
        || crate::assembly_welds_ui::action(w, cmd, None)
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
    if (crate::assembly_tools_ui::active(w)
        || w.assembly_motion.is_some()
        || w.fields.contains_key("appearance-tool")
        || w.fields.contains_key("material-tool")
        || w.fields.contains_key("display-state-editor")
        || w.fields.contains_key("sensor-tool")
        || crate::camera_views_ui::active(w)
        || w.fields.contains_key("standard-parts-tool")
        || w.fields.contains_key("tolerance-stack-tool")
        || w.fields.contains_key("properties-tool")
        || w.fields.contains_key("configuration-editor")
        || w.fields.contains_key("component-pattern-editor")
        || w.fields.contains_key("exploded-view-editor")
        || w.fields.contains_key("assembly-inspection-editor")
        || w.fields.contains_key("assembly-weld-editor"))
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
        "file.new-part" | "file.new" => match crate::settings::new_document(w, cmd == "file.new") {
            Ok(document) => w.replace(document, None),
            Err(error) => w.status = format!("New document defaults could not be applied: {error}"),
        },
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
        "file.export-bom" => export_format(w, "bom", "bom.csv"),
        "production.edit" => {
            if let Err(error) = crate::production_ui::begin(w) {
                w.status = error.to_string();
            }
        }
        "file.release" => export(w, true),
        "part.new" => new_part(w),
        "assembly.mate-new" => {
            let owner = w
                .fields
                .get("assembly-id")
                .filter(|id| w.session.visible().assemblies.contains_key(*id))
                .cloned()
                .unwrap_or_else(|| w.session.visible().root.clone());
            crate::mate_ui::new(w, &owner);
        }
        "sketch.new-3d" => {
            if w.view.active_part.is_none() {
                new_part(w);
            }
            crate::sketch3d_ui::begin(w, None);
        }
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
        "settings.themes" => crate::settings::open(w, Some("application.themes")),
        "presentation" => w.open_editor("presentation"),
        "versions" => {
            w.open_editor("versions");
            crate::git_ui::action(w, "git.refresh", None);
        }
        "bom" | "materials" | "drawings" | "manufacturing" | "comments" | "controls"
        | "commands" => w.open_editor(cmd),
        "document.rebuild" => {
            w.worker.cancel();
            w.session.restart_worker();
            w.requested = None;
            w.status = "Restarting geometry evaluation".into();
        }
        "viewport.fit" => {
            if w.dialog.as_deref() == Some("drawings") {
                crate::drawing_interaction_ui::action(w, "drawing.interact-fit", None);
            } else {
                viewport::fit(w, false);
            }
        }
        "selection.isolate" => w.isolate(),
        _ => w.status = format!("Unknown command: {cmd}"),
    }
}
pub(crate) fn save(w: &mut Workspace, choose: bool) -> bool {
    if crate::publish_ui::busy(w) {
        w.status = "Wait for Publish to finish or cancel it before saving".into();
        return false;
    }
    if crate::git_ui::busy(w) {
        w.status = "Wait for the Git operation to finish before saving".into();
        return false;
    }
    if (choose || w.project.is_none())
        && w.settings.is_none()
        && w.sketch.is_none()
        && (w.tool_checkpoint.is_none() && w.session.draft.is_none()
            || crate::document_identity_ui::active(w))
    {
        crate::document_identity_ui::begin_save_as(w);
        return false;
    }
    save_now(w, choose)
}
pub(crate) fn take_save_as_destination(w: &mut Workspace) -> Option<std::path::PathBuf> {
    w.fields
        .remove("document-identity-save-as-path")
        .map(Into::into)
}
/// Save the validated copy before promoting its name/history. A failed write
/// leaves the original session and retryable Save As editor intact.
pub(crate) fn save_identity_copy(w: &mut Workspace) {
    let Some(destination) = w
        .fields
        .get("document-identity-save-as-path")
        .map(std::path::PathBuf::from)
    else {
        w.status = "Choose a project destination before saving the copy".into();
        return;
    };
    if w.session.draft.as_ref().is_some_and(|draft| draft.pending) {
        w.confirm_requested = true;
        w.status = "Waiting for the current named copy to validate".into();
        return;
    }
    let mut staged = w.session.clone();
    if staged.draft.is_some()
        && let Err(error) = staged.apply()
    {
        w.confirm_requested = false;
        w.status = error.to_string();
        return;
    }
    let original = std::mem::replace(&mut w.session, staged);
    if !save_committed_to(w, destination) {
        w.session = original;
        w.confirm_requested = false;
        return;
    }
    w.tool_checkpoint = None;
    w.confirm_requested = false;
    w.dialog = None;
    w.manager_tree = true;
    w.fields.clear();
    w.requested = Some(w.session.identity());
    crate::drawing_ui::retain_workspace(w);
}
pub(crate) fn save_after_confirm(w: &mut Workspace, destination: Option<std::path::PathBuf>) {
    if let Some(destination) = destination {
        save_committed_to(w, destination);
    } else {
        save_now(w, true);
    }
}
pub(crate) fn choose_save_destination(w: &mut Workspace) -> Option<std::path::PathBuf> {
    let name = w
        .session
        .visible()
        .name
        .chars()
        .map(|c| if "/\\:*?\"<>|".contains(c) { '_' } else { c })
        .collect::<String>();
    let suggested_directory = w
        .fields
        .get("document-identity-save-as-path")
        .map(std::path::PathBuf::from)
        .or_else(|| w.project.clone())
        .map(|path| cad_core::persistence::project_root(&path))
        .and_then(|root| root.parent().map(std::path::Path::to_path_buf))
        .or_else(crate::document_identity_ui::default_save_directory);
    let mut dialog = rfd::FileDialog::new()
        .set_title("Save TCAD project folder")
        .set_file_name(format!("{name}.tcad"));
    // A new Projects directory may not exist until the first save. Start the
    // native picker at its nearest existing parent without creating empty folders.
    if let Some(directory) = suggested_directory
        .as_deref()
        .and_then(|path| path.ancestors().find(|ancestor| ancestor.is_dir()))
    {
        dialog = dialog.set_directory(directory);
    }
    let path = dialog.save_file()?;
    // Every copy owns its parts/resources directory. Two manifests in one
    // shared root would otherwise overwrite the original part files on Save As.
    let path = if path.file_name().and_then(|name| name.to_str()) == Some("project.json") {
        path
    } else if path.extension().and_then(|extension| extension.to_str()) == Some("json") {
        path.with_extension("tcad").join("project.json")
    } else {
        path.join("project.json")
    };
    if path
        .parent()
        .is_some_and(|parent| parent.exists() && !parent.is_dir())
    {
        w.status = "The Save As destination must be a project folder".into();
        return None;
    }
    Some(path)
}
pub(crate) fn save_now(w: &mut Workspace, choose: bool) -> bool {
    let path = if choose || w.project.is_none() {
        choose_save_destination(w)
    } else {
        w.project.clone()
    };
    if let Some(path) = path {
        return save_committed_to(w, path);
    }
    false
}
fn save_committed_to(w: &mut Workspace, path: std::path::PathBuf) -> bool {
    if crate::publish_ui::busy(w) {
        w.status = "Wait for Publish to finish or cancel it before saving".into();
        return false;
    }
    if crate::git_ui::busy(w) {
        w.status = "Wait for the Git operation to finish before saving".into();
        return false;
    }
    if crate::git_ui::recovery_pending(w) && cad_core::persistence::project_root(&path) == w.root()
    {
        w.status = "Finish merge recovery in Versions, or use Save As in a new folder".into();
        return false;
    }
    if cad_core::release::is_release_source(&path) {
        w.status = "Use Save As in a new folder to edit released source".into();
        return false;
    }
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
            let saved_path = path.display().to_string();
            let project_changed = w.project.as_ref() != Some(&path);
            w.project = Some(path);
            if project_changed {
                crate::git_ui::reset(w);
            }
            w.rendered_key.clear();
            w.saved_source = w.session.committed.fingerprint();
            w.status = if w.session.draft.is_some() {
                format!("Saved committed revision to {saved_path}. The draft still needs Apply.")
            } else {
                format!("Saved project to {saved_path}")
            };
            return true;
        }
        Err(e) => w.status = e.to_string(),
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
                display_tolerance_mm: w.display_tolerance(),
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
            display_tolerance_mm: w.display_tolerance(),
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
    let root = w
        .fields
        .get("assembly-id")
        .filter(|id| w.session.visible().assemblies.contains_key(*id))
        .cloned()
        .unwrap_or_else(|| w.session.visible().root.clone());
    w.dispatch(Command::PutPart {
        part,
        assembly: Some(root),
        placement: Default::default(),
    });
    w.activate_context(Some(def), None);
    crate::sketch::begin(w, None);
}
pub(crate) fn feature_dialog(w: &mut Workspace, f: Option<Feature>) {
    if crate::assembly_tools_ui::active(w)
        || w.assembly_motion.is_some()
        || w.fields.contains_key("appearance-tool")
        || w.fields.contains_key("material-tool")
        || w.fields.contains_key("display-state-editor")
        || w.fields.contains_key("sensor-tool")
        || crate::camera_views_ui::active(w)
        || w.fields.contains_key("standard-parts-tool")
        || w.fields.contains_key("tolerance-stack-tool")
        || w.fields.contains_key("properties-tool")
        || w.fields.contains_key("configuration-editor")
        || w.fields.contains_key("component-pattern-editor")
        || w.fields.contains_key("exploded-view-editor")
        || w.fields.contains_key("assembly-inspection-editor")
        || w.fields.contains_key("assembly-weld-editor")
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
    if matches!(f.operation, Operation::Sketch3d { .. }) {
        crate::sketch3d_ui::begin(w, Some(f));
        return;
    }
    let edge_path = crate::selection_ui::edge_selection(&f)
        .and_then(|(body, _)| crate::selection_ui::known_edge_occurrence(w, &body.definition));
    w.start_tool();
    if crate::selection_ui::edge_selection(&f).is_some() {
        w.edge_selection_path = edge_path;
    }
    w.fields.insert("feature-editor-id".into(), f.id.clone());
    w.fields
        .insert("feature-editor-generation".into(), cad_core::id());
    w.fields.remove("feature-source-invalid");
    w.fields.remove("feature-source-error");
    w.dialog_title = f.name.clone();
    w.json = serde_json::to_string_pretty(&f).unwrap_or_default();
    w.open_editor("feature");
}
pub(crate) fn feature_preset(w: &mut Workspace, name: &str) {
    if name == "Weld Bead" {
        if w.session
            .visible()
            .assemblies
            .contains_key(&w.session.visible().root)
        {
            let owner = w.session.visible().root.clone();
            crate::assembly_welds_ui::new(w, &owner);
        } else {
            crate::weld_beads_ui::preset(w);
        }
        return;
    }
    if crate::assembly_tools_ui::active(w)
        || w.fields.contains_key("appearance-tool")
        || w.fields.contains_key("material-tool")
        || w.fields.contains_key("display-state-editor")
        || w.fields.contains_key("sensor-tool")
        || crate::camera_views_ui::active(w)
        || w.fields.contains_key("standard-parts-tool")
        || w.fields.contains_key("tolerance-stack-tool")
        || w.fields.contains_key("properties-tool")
        || w.fields.contains_key("configuration-editor")
        || w.fields.contains_key("component-pattern-editor")
        || w.fields.contains_key("exploded-view-editor")
        || w.fields.contains_key("assembly-inspection-editor")
        || w.fields.contains_key("assembly-weld-editor")
    {
        w.status = "Confirm or cancel the current operation before editing another feature".into();
        return;
    }
    if crate::structural_member_ui::is_deferred(name) {
        crate::structural_member_ui::resume(w, name);
        return;
    }
    if matches!(name, "Structural Member Group" | "Structural Member") {
        crate::structural_member_ui::begin(w);
        return;
    }
    if crate::sketch3d_ui::active(w) && !crate::curve_pattern_ui::is_preset(name) {
        w.after_confirm = Some(name.into());
        w.confirm_tool();
        if crate::sketch3d_ui::active(w) && !w.confirm_requested {
            w.after_confirm = None;
        }
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
    let pending_contours = match crate::contours_ui::pending(w, name) {
        Ok(pending) => pending,
        Err(error) => {
            w.status = error;
            return;
        }
    };
    let requested_name = pending_contours
        .as_ref()
        .map(|pending| pending.requested_name.as_str())
        .unwrap_or(name);
    let name = match requested_name {
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
        w.after_confirm = Some(crate::contours_ui::deferred_preset(w, requested_name));
        return;
    }
    if w.sketch.is_some() {
        w.after_confirm = Some(crate::contours_ui::deferred_preset(w, requested_name));
        w.confirm_tool();
        if w.sketch.is_some() && !w.confirm_requested {
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
    let profile = pending_contours
        .as_ref()
        .map(|pending| pending.profile.clone())
        .or_else(|| {
            part.features
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
        })
        .unwrap_or(NodeRef {
            definition: part.id.clone(),
            node: "select-profile-node".into(),
        });
    let edges = if matches!(name, "Fillet" | "Chamfer") {
        crate::selection_ui::preferred_edges(w, &body)
    } else {
        viewport::preselected_edges(w, &body)
    };
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
            options: Box::new(cad_core::body_editing::PatternOptions {
                direction: Some(cad_core::body_editing::PatternDirection::default()),
                ..Default::default()
            }),
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
        "Sweep" => {
            let path_sketch = part
                .features
                .iter()
                .filter(|feature| !feature.suppressed && feature.id != profile.node)
                .find(|feature| {
                    w.view.active_node.as_ref() == Some(&feature.id)
                        && matches!(
                            feature.operation,
                            Operation::Sketch3d { .. }
                                | Operation::ReferenceCurve { .. }
                                | Operation::Profile { .. }
                        )
                })
                .or_else(|| {
                    part.features.iter().rev().find(|feature| {
                        !feature.suppressed
                            && matches!(feature.operation, Operation::Sketch3d { .. })
                    })
                })
                .map(|feature| part.reference(&feature.id));
            Operation::Sweep {
                options: Default::default(),
                profile,
                path: vec![
                    plane.origin,
                    std::array::from_fn(|axis| plane.origin[axis] + normal[axis] * 50.),
                ],
                path_sketch,
                body: solid_body.clone(),
                mode: "new".into(),
            }
        }
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
        Operation::Pattern {
            step,
            angle,
            options,
            ..
        } if requested_name == "Circular Pattern" => {
            options.direction = None;
            *step = ["0 mm".into(), "0 mm".into(), "0 mm".into()];
            *angle = "90 deg".into();
        }
        _ => {}
    }
    crate::contours_ui::seed_operation(w, &mut op, pending_contours.as_ref());
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
    if dialog == "document-identity" {
        crate::document_identity_ui::controls(ui, w);
        return;
    }
    if dialog == "drawings" {
        crate::drawing_ui::footer(ui, w);
        ui.separator();
    }
    let operation_title = crate::workspace_controls::task_title(w);
    ui.horizontal(|ui| {
        let title = match dialog.as_str() {
            "feature" | "sketch" | "assembly-motion" => operation_title.as_str(),
            "reference-repair" => "Repair References",
            "feature-order" => "Feature Order",
            "component-insert" => "Insert Existing Component",
            "selection" => "Selection Actions",
            "source" => "Source definitions",
            "presentation" => "Advanced layout and style",
            "assembly" => "Assembly",
            "configurations" => "Configurations and Equations",
            "versions" => "Design versions",
            "repositories" => "Git Repositories",
            "publish" => "Publish",
            "git-connections" => "Git Connections",
            "content-repositories" => "Content Libraries",
            "feature-transfer" => "Transfer Selected Features",
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
            "sensors" => "Design Monitors",
            "sketch-block-library" => "Sketch Block Library",
            "camera-views" => "Named Views",
            "standard-parts" => "Standard Hardware",
            "tolerance-stacks" => "Tolerance Stacks",
            "properties" => "Model Properties",
            _ => "Command search",
        };
        let title = if w.tool_checkpoint.is_some() && dialog != "publish" {
            operation_title.as_str()
        } else {
            title
        };
        crate::presentation_controls::label(
            ui,
            if w.tool_checkpoint.is_some() {
                "workspace.task-header"
            } else {
                "inspector/title"
            },
            if w.tool_checkpoint.is_some() {
                "task-header"
            } else {
                "inspector-title"
            },
            &[],
            title,
            "ready",
        );
        if !matches!(
            dialog.as_str(),
            "feature"
                | "sketch"
                | "configurations"
                | "appearances"
                | "materials"
                | "sensors"
                | "commands"
                | "standard-parts"
                | "tolerance-stacks"
                | "properties"
                | "reference-repair"
                | "feature-order"
                | "assembly-motion"
                | "publish"
        ) && !w.fields.contains_key("mate-editor")
            && !w.fields.contains_key("component-pattern-editor")
            && !w.fields.contains_key("assembly-weld-editor")
            && !crate::assembly_tools_ui::active(w)
            && ui.small_button("Close").clicked()
        {
            if dialog == "sketch-block-library" {
                crate::sketch_block_library_ui::close(w);
            } else if dialog == "publish" {
                crate::publish_ui::action(w, "publish.close", None);
            } else {
                w.dialog = None;
                w.manager_tree = true;
            }
        }
    });
    if let Some(part) = w
        .view
        .active_part
        .as_ref()
        .and_then(|id| w.session.visible().parts.get(id))
    {
        crate::presentation_controls::label(
            ui,
            "inspector/context",
            "inspector-context",
            &[],
            &part.name,
            "ready",
        );
    }
    ui.separator();
    if !crate::presentation_composition::confirmation_hosted(w)
        && (matches!(
            dialog.as_str(),
            "feature" | "sketch" | "reference-repair" | "feature-order"
        ) || (w.tool_checkpoint.is_some() || w.session.draft.is_some())
            && !matches!(
                dialog.as_str(),
                "drawings"
                    | "materials"
                    | "standard-parts"
                    | "camera-views"
                    | "commands"
                    | "publish"
            ))
    {
        crate::presentation_controls::inspector_section(
            ui,
            crate::presentation_controls::InspectorSection {
                id: "inspector/confirmation",
                role: "confirmation-group",
                classes: &["pending-changes"],
                label: "",
                state: "draft",
                collapse: None,
                open: None,
            },
            |ui| crate::cad_ui::confirmation(ui, w),
        );
        if w.dialog.is_none() {
            return;
        }
        ui.separator();
    }
    if dialog == "publish" {
        crate::publish_ui::show_actions(ui, w);
        if w.dialog.as_deref() != Some("publish") {
            return;
        }
        ui.separator();
    }
    egui::ScrollArea::vertical()
        .id_salt("workspace-editor")
        .show(ui, |ui| {
            match dialog.as_str() {
                "reference-repair" => crate::reference_repair_ui::show(ui, w),
                "feature-order" => crate::feature_order_ui::show(ui, w),
                "assembly-motion" => crate::assembly_motion_ui::show(ui, w),
                "selection" => {
                    if let Some(target) = crate::selection_ui::current(w) {
                        crate::selection_ui::contents(ui, w, &target);
                    }
                }
                "configurations" => crate::configuration_ui::show(ui, w),
                "inspection" => crate::inspection_ui::controls(ui, w),
                "sensors" => crate::sensors_ui::show(ui, w),
                "sketch-block-library" => crate::sketch_block_library_ui::show(ui, w),
                "camera-views" => crate::camera_views_ui::show(ui, w),
                "standard-parts" => crate::standard_parts_ui::show(ui, w),
                "tolerance-stacks" => crate::tolerance_stacks_ui::show(ui, w),
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
                "component-insert" => crate::components_ui::show(ui, w),
                "versions" => versions(ui, w),
                "repositories" => crate::repositories_ui::show(ui, w),
                "publish" => crate::publish_ui::show_contents(ui, w),
                "git-connections" => crate::git_connections_ui::show(ui, w),
                "content-repositories" => crate::content_repositories_ui::show(ui, w),
                "feature-transfer" => crate::feature_transfer_ui::show(ui, w),
                "bom" => bom(ui, w),
                "materials" => materials(ui, w),
                "properties" => crate::properties_ui::show(ui, w),
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
                        if !name.is_empty() && name.chars().all(|c| c.is_alphanumeric() || c == '_')
                        {
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
                                "Use letters, digits and underscores for the parameter identity"
                                    .into();
                        }
                    }
                }
                _ => crate::command_palette::show(ui, w),
            }
            crate::workspace_controls::diagnostics(ui, w);
            crate::cad_ui::task_history(ui, w);
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
    let changed = ui
        .add_enabled_ui(!w.fields.contains_key("feature-source-invalid"), |ui| {
            if crate::operation_editor_ui::show(ui, w) {
                false
            } else {
                crate::cad_ui::feature_fields(ui, w, &mut feature)
            }
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
    if let Some(selected) = w.selected.clone()
        && crate::component_pattern_ui::capture_selection(w, &selected)
    {
        return;
    }
    if let Some(selected) = w.selected.clone()
        && crate::fabrication_sources_ui::capture_selection(w, &selected)
    {
        return;
    }
    if crate::weld_beads_ui::capture_selection(w) {
        return;
    }
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
    let incoming = if is_edge_feature {
        crate::selection_ui::picked_edges(w, &feature_id, &selected)
    } else {
        vec![selected.reference.clone()]
    };
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
    let all_selected = incoming.iter().all(|pick| {
        selections
            .iter()
            .any(|reference| crate::selection_ui::same_edge(reference, pick))
    });
    if is_edge_feature && toggle && all_selected {
        selections.retain(|reference| {
            !incoming
                .iter()
                .any(|pick| crate::selection_ui::same_edge(reference, pick))
        });
    } else {
        for pick in incoming {
            if !selections
                .iter()
                .any(|reference| crate::selection_ui::same_edge(reference, &pick))
            {
                selections.push(pick);
            }
        }
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
    crate::workspace_controls::confirmation_preview(ui);
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
    if w.component_fork.is_some() {
        crate::component_fork_ui::show(ui, w);
        return;
    }
    if crate::assembly_tools_ui::active(w) {
        crate::assembly_tools_ui::show(ui, w);
        return;
    }
    if w.assembly_motion.is_some() {
        crate::assembly_motion_ui::show(ui, w);
        return;
    }
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
        || w.fields.contains_key("assembly-weld-editor")
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
    if w.fields.contains_key("display-state-editor") {
        crate::display_states_ui::show(ui, w, &assembly_id);
        return;
    }
    if w.fields.contains_key("assembly-inspection-editor") {
        crate::assembly_inspection_ui::show(ui, w, &assembly_id);
        return;
    }
    if w.fields.contains_key("assembly-weld-editor") {
        crate::assembly_welds_ui::show(ui, w);
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
    crate::display_states_ui::show(ui, w, &assembly_id);
    ui.horizontal_wrapped(|ui| {
        if ui.button("Insert Existing…").clicked() {
            crate::components_ui::begin_insert(w);
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
                    display_states: BTreeMap::new(),
                    inspections: BTreeMap::new(),
                    weld_beads: BTreeMap::new(),
                    rigid_groups: BTreeMap::new(),
                    parameters: BTreeMap::new(),
                    properties: BTreeMap::new(),
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
                        fixed: a.instances.is_empty(),
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
    if ui.button("Weld Bead…").clicked() {
        crate::assembly_welds_ui::new(w, &assembly_id);
    }
    crate::assembly_welds_ui::tree(ui, w);
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
    if a.instances.values().all(|instance| !instance.fixed) && !a.instances.is_empty() {
        ui.small(
            "No component is fixed. Ground a reference component to locate the assembly in space.",
        );
    }
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
                    cad_core::occurrence_names::instance(&doc, &a.id, i),
                    cad_core::presentation::definition_name(&doc, &i.definition)
                ))
                .id_salt(&i.id)
                .show(ui, |ui| {
                    if generated.contains(&i.id){ui.weak("Placement and shared definition are driven by the component pattern. Use configuration overrides to suppress this occurrence or select another configuration.");return;}
                    let mut instance = i.clone();
                    ui.horizontal(|ui| {
                        for (rotate, label) in [(false, "Move"), (true, "Rotate")] {
                            if crate::presentation_controls::button(ui,
                                &format!("assembly-motion:{}:{}:{rotate}", assembly_id, i.id),
                                "assembly-motion-action", &["semantic-control"], label,
                                !i.fixed && w.tool_checkpoint.is_none(), false)
                                .on_disabled_hover_text("Select a movable component and finish the current operation first").clicked() {
                                crate::assembly_motion_ui::begin_instance(w, &assembly_id, &i.id, rotate);
                            }
                        }
                    });
                    let mut changed = false;
                    changed |= ui.text_edit_singleline(&mut instance.name).changed();
                    changed |= ui
                        .checkbox(&mut instance.fixed, "Grounded / fixed")
                        .changed();
                    ui.small(format!(
                        "Definition: {}",
                        cad_core::presentation::definition_name(&doc, &instance.definition)
                    ));
                    crate::assembly_tools_ui::show_placement(ui, w, &assembly_id, &i.id);
                    ui.small("Mates solve from these starting coordinates. Move / Rotate uses the current solved pose.");
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
                        for (axis, e) in ["X", "Y", "Z"].into_iter().zip(expressions) {
                            ui.label(format!("{axis} position / expression"));
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
                    crate::components_ui::show_placement(ui,w,&assembly_id,&i.id);
                    ui.horizontal_wrapped(|ui| {
                        if ui.button("Edit in place").clicked() {
                            let path = doc.placed_parts().unwrap_or_default().into_iter()
                                .find(|placed| placed.path.last() == Some(&i.id)).map(|placed| placed.path);
                            w.activate_context(Some(i.definition.clone()), path);
                            if !w.view.isolation {
                                w.isolate();
                            }
                        }
                        if ui.button("Fork / Make independent").clicked() {
                            let paths = cad_core::component_fork::occurrence_paths(&doc, &assembly_id, &i.id);
                            match paths {
                                Ok(paths) if paths.len() == 1 => crate::component_fork_ui::begin(w, &i.definition, cad_core::component_fork::Target::Occurrence { path: paths[0].clone() }),
                                _ => w.status = "Select the specific occurrence in the assembly tree, then use Fork Part or Assembly".into(),
                            }
                        }
                        if ui.button("Duplicate shared").clicked() {
                            let mut clone = i.clone();
                            clone.id = id();
                            clone.name.push_str(" copy");
                            clone.fixed = false;
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
    crate::production_ui::show(ui, w);
}
fn materials(ui: &mut egui::Ui, w: &mut Workspace) {
    crate::materials_ui::show(ui, w);
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
            display_tolerance_mm: w.display_tolerance(),
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
    crate::git_ui::libraries(ui, w);
}
fn versions(ui: &mut egui::Ui, w: &mut Workspace) {
    crate::git_ui::versions(ui, w);
}
pub(crate) fn compare(w: &mut Workspace, d: Document, root: PathBuf) {
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
        display_tolerance_mm: w.display_tolerance(),
        operation_preview: None,
    });
    w.status = "Evaluating comparison revision".into();
}
