//! Native presentation of application-owned selection actions.
use crate::{Workspace, cad_ui, presentation_controls as controls, ui, viewport};
use bevy_egui::egui;
use cad_core::{
    Feature, commands::Command, presentation::Region, selection_actions::SelectionTarget,
};

pub struct ActionPopup {
    target: SelectionTarget,
    anchor: egui::Pos2,
    layer: egui::LayerId,
    opened_frame: u64,
    presentation_parent: Option<Region>,
}

pub fn current(w: &Workspace) -> Option<SelectionTarget> {
    if let Some(target) = &w.context_selection {
        return Some(target.clone());
    }

    if let Some(s) = &w.sketch
        && !s.choosing_plane
    {
        return Some(SelectionTarget::Sketch {
            definition: s.definition.clone(),
            feature: s.feature.clone(),
            sketch: s.sketch_id.clone(),
            entities: s.selection.clone(),
        });
    }
    if let Some(selected) = &w.selected {
        return Some(SelectionTarget::Geometry {
            definition: selected.definition.clone(),
            instance: selected.instance.clone(),
            path: selected.path.clone(),
            reference: selected.reference.clone(),
            body: selected.body.clone(),
            planar: crate::sketch::selected_plane(w, &selected.definition).is_some(),
        });
    }
    let definition = w.view.active_part.as_ref()?;
    if let Some(reference) = &w.active_plane {
        return Some(SelectionTarget::Reference {
            definition: definition.clone(),
            reference: reference.clone(),
        });
    }
    if let Some(feature) = &w.view.active_node
        && w.session
            .visible()
            .parts
            .get(definition)?
            .features
            .iter()
            .any(|f| &f.id == feature)
    {
        return Some(SelectionTarget::Feature {
            definition: definition.clone(),
            feature: feature.clone(),
        });
    }
    Some(SelectionTarget::Object {
        definition: definition.clone(),
        object: definition.clone(),
        role: "part".into(),
    })
}

pub fn semantics(w: &Workspace) -> Vec<cad_core::presentation::SemanticElement> {
    let Some(target) = current(w) else {
        return vec![];
    };
    let mut elements = descriptors(w, &target);
    for action in &mut elements {
        if let Some(data) = action.data.as_object_mut() {
            data.insert(
                "classes".into(),
                serde_json::json!(["menu-item", "selection-action"]),
            );
        }
    }
    let groups = elements
        .iter()
        .filter_map(|element| {
            element
                .data
                .get("group")
                .and_then(serde_json::Value::as_str)
        })
        .map(str::to_owned)
        .collect::<std::collections::BTreeSet<_>>();
    for group in groups {
        elements.push(cad_core::presentation::SemanticElement {
            id: format!("selection-group/{group}"),
            role: "selection-action-group".into(),
            label: group,
            value: None,
            state: "ready".into(),
            commands: vec![],
            relationships: vec!["selection-popup".into()],
            data: serde_json::json!({"presentation_control":true}),
        });
    }
    for (id, role, label, commands) in [
        (
            "selection-popup",
            "selection-actions",
            title(w, &target),
            vec![],
        ),
        (
            "selection-popup.title",
            "selection-title",
            title(w, &target),
            vec![],
        ),
        (
            "selection-popup.dismiss",
            "dismiss-action",
            "Dismiss actions".into(),
            vec!["presentation.selection-dismiss".into()],
        ),
    ] {
        elements.push(cad_core::presentation::SemanticElement {
            id: id.into(),
            role: role.into(),
            label,
            value: None,
            state: "ready".into(),
            commands,
            relationships: vec!["workspace.viewport".into()],
            data: serde_json::json!({"presentation_control":true,"target":target}),
        });
    }
    elements
}

fn descriptors(
    w: &Workspace,
    target: &SelectionTarget,
) -> Vec<cad_core::presentation::SemanticElement> {
    if let SelectionTarget::Sketch {
        definition,
        entities,
        ..
    } = target
        && let Some(s) = &w.sketch
    {
        return cad_core::selection_actions::sketch_project(
            w.session.visible(),
            definition,
            &crate::sketch::sketch_definition(s),
            entities,
            target,
        );
    }
    let inspecting_geometry =
        w.view.history_cursor.is_some() && matches!(target, SelectionTarget::Geometry { .. });
    let mut actions = cad_core::selection_actions::project(
        w.session.visible(),
        target,
        &w.view.pinned,
        w.tool_checkpoint.is_some() || w.sketch.is_some() || inspecting_geometry,
        w.view
            .isolation
            .then_some(w.view.active_part.as_deref())
            .flatten(),
    );
    if let Some(sketch) = &w.sketch
        && let SelectionTarget::Geometry {
            definition,
            reference,
            ..
        } = target
        && matches!(reference.topology.as_str(), "edge" | "face")
    {
        let reason = if definition != &sketch.definition {
            Some("Convert Entities currently uses geometry of the active part")
        } else if sketch.choosing_plane {
            Some("Choose a sketch plane first")
        } else if sketch.edit.is_some() {
            Some("Accept or cancel the current sketch operation first")
        } else {
            None
        };
        actions.insert(
            0,
            cad_core::selection_actions::convert_action(target, reason),
        );
    }
    if inspecting_geometry {
        for action in &mut actions {
            if action.state == "disabled" {
                let reason = "Return to Latest to model from the current geometry";
                action.value = Some(reason.into());
                if let Some(data) = action.data.as_object_mut() {
                    data.insert("disabled_reason".into(), reason.into());
                }
            }
        }
    }
    actions
}

pub fn open(w: &mut Workspace, response: &egui::Response, anchor: egui::Pos2) {
    w.selection_popup = None;
    if let Some(target) = current(w) {
        w.selection_popup = Some(ActionPopup {
            target,
            anchor,
            layer: response.layer_id,
            opened_frame: response.ctx.cumulative_frame_nr(),
            presentation_parent: controls::current_target().map(|target| Region {
                id: target.node.id,
                role: target.node.role,
                classes: target.node.classes,
                component: None,
                rect: [0.; 4],
                resizable: false,
            }),
        });
    }
}

/// Select a source feature without opening actions; the viewport opens its own popup.
pub fn feature_selected(
    w: &mut Workspace,
    definition: &str,
    feature: &Feature,
    response: &egui::Response,
) {
    feature_selected_at(w, definition, feature, response, None);
}
pub(crate) fn feature_selected_at(
    w: &mut Workspace,
    definition: &str,
    feature: &Feature,
    response: &egui::Response,
    path: Option<Vec<cad_core::Id>>,
) {
    if response.double_clicked() {
        crate::history::latest(w);
    }
    w.selection_popup = None;
    w.selected_feature_path = path;
    if w.sketch.as_ref().is_some_and(|s| s.choosing_plane)
        && matches!(
            feature.operation,
            cad_core::Operation::ReferenceCoordinateSystem { .. }
        )
    {
        w.selected = None;
    }
    if w.sketch.as_ref().is_some_and(|s| s.choosing_plane)
        && matches!(
            feature.operation,
            cad_core::Operation::ReferencePlane { .. }
                | cad_core::Operation::ReferenceCoordinateSystem { .. }
        )
        && let Some((plane, reference)) =
            crate::reference_ui::evaluated_plane(w, definition, &feature.id)
    {
        w.selected = None;
        crate::sketch::choose(w, plane, Some(reference));
        return;
    }
    if let Some(s) = &mut w.sketch
        && s.feature == feature.id
    {
        s.selection.clear();
        s.selected = None;
        s.selected_member = None;
        s.dimension_model = None;
        w.selected = None;
        w.candidates.clear();
        w.context_selection = None;
        return;
    }
    if w.tool_checkpoint.is_some() || w.sketch.is_some() {
        w.selected = None;
        w.candidates.clear();
        w.context_selection = Some(SelectionTarget::Feature {
            definition: definition.into(),
            feature: feature.id.clone(),
        });
        return;
    }
    if w.view.active_part.as_deref() != Some(definition) {
        w.view.active_instance = None;
    }
    w.context_selection = None;
    w.view.active_part = Some(definition.into());
    w.view.active_node = Some(feature.id.clone());
    w.selected = None;
    w.active_plane = None;
    w.candidates.clear();
    if response.double_clicked() {
        ui::feature_dialog(w, Some(feature.clone()));
    }
}

pub fn show(ctx: &egui::Context, w: &mut Workspace) {
    let Some(popup) = w.selection_popup.take() else {
        return;
    };
    if current(w).as_ref() != Some(&popup.target) {
        return;
    }
    let region = Region {
        id: "selection-popup".into(),
        role: "selection-actions".into(),
        component: None,
        rect: [popup.anchor.x, popup.anchor.y, 232., 0.],
        classes: vec![],
        resizable: false,
    };
    let target = popup
        .presentation_parent
        .as_ref()
        .map(|parent| {
            w.layout
                .style_target(parent, &w.view.active_layout, "ready")
                .child(&region.id, &region.role, &region.classes, "ready")
        })
        .unwrap_or_else(|| {
            w.layout
                .style_target(&region, &w.view.active_layout, "ready")
        });
    let props = w.theme.properties_for(&target);
    if props.get("display").is_some_and(|v| v == "none") {
        return;
    }
    let number = |key: &str, default: f32| {
        props
            .get(key)
            .and_then(|v| v.trim_end_matches("px").parse::<f32>().ok())
            .filter(|v| v.is_finite())
            .unwrap_or(default)
    };
    let width = number("width", 232.)
        .max(number("min-width", 0.))
        .min(number("max-width", ctx.content_rect().width()))
        .max(0.);
    let mut frame =
        egui::Frame::popup(&ctx.global_style()).inner_margin(number("padding", 8.).max(0.));
    if let Some(color) = props.get("background").and_then(|v| ui::color(v)) {
        frame.fill = color;
    }
    frame.corner_radius =
        egui::CornerRadius::same(number("border-radius", 6.).clamp(0., 255.) as u8);
    if let Some(border) = props.get("border") {
        let mut words = border.split_whitespace();
        let width = words
            .next()
            .and_then(|v| v.trim_end_matches("px").parse::<f32>().ok())
            .filter(|v| v.is_finite())
            .unwrap_or(1.)
            .max(0.);
        if let Some(color) = words.next_back().and_then(ui::color) {
            frame.stroke = egui::Stroke::new(width, color);
        }
    }
    let mut open = true;
    egui::Popup::new(
        egui::Id::new("selection-popup"),
        ctx.clone(),
        popup.anchor,
        popup.layer,
    )
    .at_position(egui::pos2(
        number("left", popup.anchor.x),
        number("top", popup.anchor.y),
    ))
    .gap(8.)
    .width(width)
    .frame(frame)
    .open_bool(&mut open)
    .close_behavior(if popup.opened_frame == ctx.cumulative_frame_nr() {
        egui::PopupCloseBehavior::IgnoreClicks
    } else {
        egui::PopupCloseBehavior::CloseOnClickOutside
    })
    .show(|ui| {
        controls::with_target(target, ui, |ui| {
            ui.set_width(width);
            if let Some(color) = props.get("color").and_then(|v| ui::color(v)) {
                ui.visuals_mut().override_text_color = Some(color);
            }
            ui.style_mut().override_font_id =
                Some(egui::FontId::proportional(number("font-size", 12.).max(1.)));
            ui.spacing_mut().item_spacing.y = number("gap", 3.).max(0.);
            egui::ScrollArea::vertical()
                .max_height(
                    number("height", ctx.content_rect().height() - 32.)
                        .max(number("min-height", 0.))
                        .min(number("max-height", ctx.content_rect().height()))
                        .max(0.),
                )
                .show(ui, |ui| contents(ui, w, &popup.target));
        })
    });
    if open {
        w.selection_popup = Some(popup);
    }
}

fn title(w: &Workspace, target: &SelectionTarget) -> String {
    let name = w
        .session
        .visible()
        .parts
        .get(target.definition())
        .and_then(|p| p.features.iter().find(|f| f.id == target.feature()))
        .map(|f| f.name.as_str())
        .unwrap_or("Selection");
    match target {
        SelectionTarget::Geometry { reference, .. } => format!(
            "{} · {name}",
            match reference.topology.as_str() {
                "edge" => "Edge",
                "plane" => "Plane",
                "axis" => "Axis",
                "point" => "Point",
                _ => "Face",
            }
        ),
        SelectionTarget::Feature { .. } => name.into(),
        SelectionTarget::Sketch { entities, .. } => {
            if entities.is_empty() {
                "Sketch".into()
            } else {
                format!("Sketch · {} selected", entities.len())
            }
        }
        SelectionTarget::Reference { reference, .. } => reference.clone(),
        SelectionTarget::Object {
            role,
            object,
            definition,
        } => {
            let doc = w.session.visible();
            match role.as_str() {
                "part" | "assembly" => cad_core::presentation::definition_name(doc, definition),
                "component-instance" => doc
                    .assemblies
                    .values()
                    .find_map(|a| a.instances.get(object))
                    .map(|i| i.name.clone())
                    .unwrap_or("Component".into()),
                "assembly-mate" => "Mate".into(),
                "parameter-field" => "Parameter".into(),
                "material" => "Material".into(),
                _ => "Selection".into(),
            }
        }
    }
}

pub fn contents(ui: &mut egui::Ui, w: &mut Workspace, target: &SelectionTarget) {
    let actions = descriptors(w, target);
    if actions.is_empty() {
        ui.weak("Selection is no longer available");
        return;
    }
    let title = title(w, target);
    ui.horizontal(|ui| {
        controls::label(
            ui,
            "selection-popup.title",
            "selection-title",
            &[],
            &title,
            "ready",
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if controls::button(
                ui,
                "selection-popup.dismiss",
                "dismiss-action",
                &["menu-item"],
                "×",
                true,
                false,
            )
            .on_hover_text("Dismiss actions · Esc")
            .clicked()
            {
                ui.close();
            }
        });
    });
    let mut group = "";
    for element in &actions {
        let next = element
            .data
            .get("group")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        if next != group {
            ui.separator();
            controls::label(
                ui,
                &format!("selection-group/{next}"),
                "selection-action-group",
                &[],
                next,
                "ready",
            );
            group = next;
        }
        if controls::button(
            ui,
            &element.id,
            &element.role,
            &["menu-item", "selection-action"],
            &element.label,
            element.state != "disabled",
            element.state == "selected",
        )
        .on_hover_text(element.value.as_deref().unwrap_or(&element.label))
        .on_disabled_hover_text(element.value.as_deref().unwrap_or(&element.label))
        .clicked()
        {
            execute(w, target, &element.commands[0]);
            ui.close();
            break;
        }
    }
}

pub fn execute(w: &mut Workspace, target: &SelectionTarget, command: &str) {
    let actions = descriptors(w, target);
    let Some(action) = actions
        .iter()
        .find(|a| a.commands.iter().any(|c| c == command))
    else {
        return;
    };
    if action.state == "disabled" {
        w.status = action.value.clone().unwrap_or_default();
        return;
    }
    // A bound action may outlive the frame that published it. Never apply an old
    // topology command to the geometry that is selected now.
    if matches!(target, SelectionTarget::Geometry { .. }) && current(w).as_ref() != Some(target) {
        w.status = "Selection changed. Select the geometry again to show its actions".into();
        return;
    }
    if command == "sketch.convert-entities" && matches!(target, SelectionTarget::Geometry { .. }) {
        w.context_selection = None;
        crate::sketch_edit_ui::begin(w, crate::sketch_edit_ui::Kind::Convert);
        return;
    }
    match target {
        SelectionTarget::Sketch { .. } => {
            if current(w).as_ref() != Some(target) {
                w.status = "Sketch selection changed; select again".into();
                return;
            }
            w.selection_popup = None;
            crate::sketch_ui::action(w, command);
            return;
        }
        SelectionTarget::Reference {
            definition,
            reference,
        } => {
            w.selection_popup = None;
            match command {
                "sketch.new" => {
                    w.view.active_part = Some(definition.clone());
                    w.context_selection = None;
                    w.active_plane = (reference != "Origin").then(|| reference.clone());
                    ui::action(w, command);
                }
                "feature.split" | "feature.cut-with-plane" => {
                    w.view.active_part = Some(definition.clone());
                    w.context_selection = None;
                    w.selected = None;
                    w.active_plane = Some(reference.clone());
                    ui::action(w, command);
                }
                "viewport.center-origin" => {
                    w.view.target = [0.; 3];
                    crate::navigation::stop(w);
                }
                "viewport.normal-reference" => {
                    w.view.roll = 0.;
                    crate::navigation::stop(w);
                    w.view.orbit = match reference.as_str() {
                        "Top Plane" => [0., std::f32::consts::FRAC_PI_2],
                        "Right Plane" => [std::f32::consts::FRAC_PI_2, 0.],
                        _ => [0., 0.],
                    };
                }
                _ => {}
            }
            return;
        }
        SelectionTarget::Object {
            definition,
            object,
            role,
        } => {
            w.selection_popup = None;
            if w.tool_checkpoint.is_none() && w.session.visible().parts.contains_key(definition) {
                w.view.active_part = Some(definition.clone());
            }
            match command {
                "selection.fit" => {
                    let active = w.view.active_part.replace(target.definition().into());
                    viewport::fit(w, true);
                    w.view.active_part = active;
                }
                "selection.pin" => {
                    if w.view.pinned.contains(object) {
                        w.view.pinned.retain(|id| id != object);
                    } else {
                        w.view.pinned.push(object.clone());
                    }
                    w.rendered_key.clear();
                }
                "selection.isolate" | "selection.edit-in-place" => {
                    w.view.active_instance = Some(object.clone());
                    if command == "selection.isolate" || !w.view.isolation {
                        w.isolate();
                    }
                    w.rendered_key.clear();
                }
                "object.edit" if role == "component-pattern" => {
                    if let Some(pattern) = w
                        .session
                        .visible()
                        .assemblies
                        .get(definition)
                        .and_then(|a| a.component_patterns.get(object))
                        .cloned()
                    {
                        crate::component_pattern_ui::begin(w, definition, pattern);
                    }
                }
                "object.delete" if role == "component-pattern" => {
                    w.start_tool();
                    w.dispatch(Command::DeleteComponentPattern {
                        assembly: definition.clone(),
                        pattern: object.clone(),
                    });
                }
                "object.edit" => {
                    if role == "assembly-mate" {
                        if let Some(mate) = w
                            .session
                            .visible()
                            .assemblies
                            .get(definition)
                            .and_then(|a| a.mates.iter().find(|m| &m.id == object))
                        {
                            let mate = mate.clone();
                            crate::mate_ui::begin(w, definition, mate);
                        }
                    } else {
                        w.fields.insert("graph-parameters".into(), "open".into());
                        w.fields
                            .insert("focus-parameter".into(), format!("{definition}::{object}"));
                        w.fields.insert(
                            "editing-parameter".into(),
                            format!("{definition}::{object}"),
                        );
                        if w.sketch.is_some() {
                            w.manager_tree = false;
                        } else {
                            w.open_editor("properties");
                        }
                    }
                }
                "object.delete" if role == "assembly-mate" => {
                    w.start_tool();
                    w.dispatch(Command::DeleteMate {
                        assembly: definition.clone(),
                        mate: object.clone(),
                    });
                }
                "assembly.edit" | "selection.mate" => {
                    if let Some(assembly) = w
                        .session
                        .visible()
                        .assemblies
                        .values()
                        .find(|a| a.instances.contains_key(object))
                        .map(|a| a.id.clone())
                    {
                        w.fields.insert("assembly-id".into(), assembly);
                    }
                    w.open_editor("assembly");
                }
                _ => {
                    w.context_selection = None;
                    ui::action(w, command);
                }
            }
            return;
        }
        _ => {}
    }
    let Some(feature) = w
        .session
        .visible()
        .parts
        .get(target.definition())
        .and_then(|p| p.features.iter().find(|f| f.id == target.feature()))
        .cloned()
    else {
        return;
    };
    w.selection_popup = None;
    if matches!(target, SelectionTarget::Feature { .. }) {
        w.selected = None;
        w.active_plane = None;
        w.candidates.clear();
    }
    if (!w.view.isolation && w.tool_checkpoint.is_none()) || command == "selection.edit-in-place" {
        w.view.active_part = Some(target.definition().into());
        w.view.active_node = Some(target.feature().into());
    }
    match command {
        "viewport.normal-reference" => crate::reference_ui::normal_to(w),
        "feature.edit" => ui::feature_dialog(w, Some(feature)),
        "feature.move-up" | "feature.move-down" => crate::feature_order_ui::move_feature(
            w,
            target.definition(),
            &feature.id,
            command == "feature.move-up",
        ),
        "feature.suppress" | "feature.delete" => {
            let suppressed = !cad_core::configuration::feature_suppressed(
                w.session.visible(),
                target.definition(),
                &feature,
            );
            w.start_tool();
            w.dispatch(if command == "feature.suppress" {
                Command::SuppressFeature {
                    definition: target.definition().into(),
                    feature: feature.id,
                    suppressed,
                }
            } else {
                Command::DeleteFeature {
                    definition: target.definition().into(),
                    feature: feature.id,
                }
            });
        }
        "selection.measure" => cad_ui::measure(w),
        "selection.fit" => {
            let active = w.view.active_part.replace(target.definition().into());
            viewport::fit(w, true);
            w.view.active_part = active;
        }
        "selection.pin" => ui::pin_selected(w),
        "selection.edit-in-place" => {
            if let SelectionTarget::Geometry { instance, .. } = target {
                w.view.active_instance = Some(instance.clone());
            }
            if !w.view.isolation {
                w.isolate();
            }
            w.rendered_key.clear();
        }
        "selection.isolate" => w.isolate(),
        "selection.mate" => {
            let assembly = w
                .fields
                .get("assembly-id")
                .filter(|id| w.session.visible().assemblies.contains_key(*id))
                .cloned()
                .unwrap_or_else(|| w.session.visible().root.clone());
            crate::mate_ui::new(w, &assembly);
        }
        _ => ui::action(w, command),
    }
}

/// Sidebar and graph selections still publish actions without opening a popup.
pub fn object_selected(w: &mut Workspace, target: SelectionTarget) {
    w.selection_popup = None;
    w.selected_feature_path = None;
    w.context_selection = Some(target);
    w.selected = None;
    w.view.active_node = None;
}
