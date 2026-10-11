//! Familiar CAD workspace widgets over the same document commands and user layout.
use crate::{Workspace, sketch, ui, viewport};
use bevy_egui::egui::{self, Color32, RichText, Stroke};
use cad_core::{Feature, Operation, Part};

#[derive(Clone, Copy)]
enum Icon {
    Sketch,
    Extrude,
    Cut,
    Revolve,
    Fillet,
    Chamfer,
    Pattern,
    Shell,
    Line,
    Rectangle,
    Circle,
    Select,
}

fn icon(p: &egui::Painter, center: egui::Pos2, kind: Icon, color: Color32) {
    let at = |x, y| center + egui::vec2(x, y);
    let stroke = Stroke::new(1.6, color);
    let line = |a, b| {
        p.line_segment([a, b], stroke);
    };
    match kind {
        Icon::Rectangle => {
            p.rect_stroke(
                egui::Rect::from_center_size(center, egui::vec2(25., 18.)),
                0.,
                stroke,
                egui::StrokeKind::Inside,
            );
        }
        Icon::Circle => {
            p.circle_stroke(center, 11., stroke);
            line(at(-3., 0.), at(3., 0.));
            line(at(0., -3.), at(0., 3.));
        }
        Icon::Line | Icon::Sketch => {
            line(at(-11., 9.), at(9., -10.));
            line(at(9., -10.), at(12., -7.));
            line(at(12., -7.), at(-8., 12.));
            line(at(-11., 9.), at(-12., 13.));
            if matches!(kind, Icon::Sketch) {
                line(at(-14., 15.), at(13., 15.));
            }
        }
        Icon::Select => {
            p.add(egui::Shape::closed_line(
                vec![
                    at(-7., -13.),
                    at(-7., 9.),
                    at(-1., 4.),
                    at(4., 12.),
                    at(8., 10.),
                    at(3., 2.),
                    at(12., 1.),
                ],
                stroke,
            ));
        }
        Icon::Revolve => {
            line(at(-11., -10.), at(-11., 11.));
            let points = (0..=24)
                .map(|i| {
                    let a = i as f32 / 24. * 5.5;
                    at(a.cos() * 11., a.sin() * 8.)
                })
                .collect::<Vec<_>>();
            p.add(egui::Shape::line(points, stroke));
            line(at(8., -7.), at(12., -5.));
            line(at(12., -5.), at(11., -10.));
        }
        Icon::Pattern => {
            for x in [-8., 7.] {
                for y in [-7., 8.] {
                    p.rect_stroke(
                        egui::Rect::from_center_size(at(x, y), egui::vec2(9., 9.)),
                        0.,
                        stroke,
                        egui::StrokeKind::Inside,
                    );
                }
            }
        }
        _ => {
            let corners = [
                at(-12., -5.),
                at(2., -12.),
                at(13., -5.),
                at(-1., 2.),
                at(-12., 9.),
                at(-1., 16.),
                at(13., 9.),
            ];
            for (a, b) in [
                (0, 1),
                (1, 2),
                (2, 3),
                (3, 0),
                (0, 4),
                (4, 5),
                (5, 6),
                (6, 2),
                (3, 5),
            ] {
                line(corners[a], corners[b]);
            }
            if matches!(kind, Icon::Cut | Icon::Shell) {
                p.circle_stroke(at(1., -5.), 4., Stroke::new(1.8, color));
            }
            if matches!(kind, Icon::Fillet) {
                p.add(egui::Shape::line(
                    vec![at(-12., 1.), at(-11., -2.), at(-8., -5.), at(-5., -5.)],
                    Stroke::new(3., color),
                ));
            }
            if matches!(kind, Icon::Chamfer) {
                p.line_segment([at(-12., 1.), at(-6., -5.)], Stroke::new(3., color));
            }
        }
    }
}
fn tool_button(
    ui: &mut egui::Ui,
    label: &str,
    kind: Icon,
    enabled: bool,
    active: bool,
    hint: &str,
) -> bool {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(92., 32.), egui::Sense::click());
    let color = if !enabled {
        Color32::from_gray(160)
    } else if active {
        ui.visuals().selection.stroke.color
    } else {
        ui.visuals().text_color()
    };
    if enabled && (active || response.hovered()) {
        ui.painter().rect_filled(
            rect,
            3.,
            if active {
                ui.visuals().selection.stroke.color.gamma_multiply(0.12)
            } else {
                ui.visuals().widgets.hovered.bg_fill
            },
        );
    }
    icon(
        ui.painter(),
        rect.left_center() + egui::vec2(17., 0.),
        kind,
        color,
    );
    ui.painter().text(
        rect.left_center() + egui::vec2(35., 0.),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(10.5),
        color,
    );
    let clicked = enabled && response.clicked();
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, label));
    response.on_hover_text(hint);
    clicked
}

pub fn menu(ui: &mut egui::Ui, w: &mut Workspace) {
    ui.horizontal(|ui| {
        ui.label(RichText::new("TCAD").strong());
        ui.menu_button("File", |ui| {
            for (label, command) in [
                ("New Part  ⌘N", "file.new-part"),
                ("New Assembly", "file.new"),
                ("Open…  ⌘O", "file.open"),
                ("Save  ⌘S", "file.save"),
                ("Save As…", "file.save-as"),
                ("Import STEP…", "file.import"),
                ("Export…", "file.export"),
                ("Recover Local Work…", "file.recover"),
            ] {
                if ui.button(label).clicked() {
                    ui::action(w, command);
                    ui.close();
                }
            }
            ui.separator();
            if ui.button("Open Example: Drive Unit").clicked() {
                ui::action(w, "file.example");
                ui.close();
            }
            ui.separator();
            if ui.button("Export Drawing DXF…").clicked() {
                ui::export_format(w, "drawing-dxf", "drawing.dxf");
                ui.close();
            }
            if ui.button("Export Flat Pattern DXF…").clicked() {
                ui::action(w, "file.export-flat-pattern");
                ui.close();
            }
            if ui.button("Export Cut List CSV…").clicked() {
                ui::action(w, "file.export-cutlist");
                ui.close();
            }
        });
        ui.menu_button("Edit", |ui| {
            if ui
                .add_enabled(w.session.can_undo(), egui::Button::new("Undo  ⌘Z"))
                .clicked()
            {
                w.session.undo();
                w.requested = None;
                ui.close();
            }
            if ui
                .add_enabled(w.session.can_redo(), egui::Button::new("Redo  ⇧⌘Z"))
                .clicked()
            {
                w.session.redo();
                w.requested = None;
                ui.close();
            }
            if ui.button("Parameters…").clicked() {
                w.open_editor("parameter");
                ui.close();
            }
        });
        ui.menu_button("Insert", |ui| {
            for group in [
                "Create",
                "Remove",
                "Modify",
                "Pattern",
                "Surfaces",
                "Mold",
                "Reference",
                "Sheet Metal",
                "Weldments",
            ] {
                ui.menu_button(group, |ui| {
                    for tool in cad_core::modeling_tools::TOOLS
                        .iter()
                        .filter(|t| t.group == group)
                    {
                        if ui.button(tool.label).clicked() {
                            ui::action(w, tool.command);
                            ui.close();
                        }
                    }
                });
            }
        });
        ui.menu_button("View", |ui| {
            view_controls(ui, w);
        });
        ui.menu_button("Tools", |ui| {
            for (label, command) in [
                ("Assembly and Mates…", "assembly.edit"),
                ("Component Pattern…", "assembly.pattern"),
                ("Exploded View…", "assembly.exploded-new"),
                ("Interference / Clearance…", "assembly.inspection-new"),
                ("Configurations…", "configurations"),
                ("Equations…", "equations"),
                ("Materials…", "materials"),
                ("Manufacturing…", "manufacturing"),
                ("Drawings…", "drawings"),
                ("Component Libraries…", "libraries"),
                ("Bill of Materials…", "bom"),
                ("Navigation Settings…", "controls"),
            ] {
                if ui.button(label).clicked() {
                    ui::action(w, command);
                    ui.close();
                }
            }
        });
        ui.menu_button("Workspace", |ui| {
            for (label, command) in [
                ("Layout and Theme…", "presentation"),
                ("Versions…", "versions"),
                ("Review Comments…", "comments"),
                ("Rebuild  F5", "document.rebuild"),
                ("Source Definitions…", "source"),
                ("Create Release…", "file.release"),
            ] {
                if ui.button(label).clicked() {
                    ui::action(w, command);
                    ui.close();
                }
            }
        });
        ui.separator();
        let dirty = w.session.committed.fingerprint() != w.saved_source
            || w.session.draft.is_some()
            || w.sketch.is_some();
        ui.label(
            RichText::new(format!(
                "{}{}",
                w.session.visible().name,
                if dirty { " *" } else { "" }
            ))
            .strong(),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("Find a command  ⌘K").clicked() {
                w.open_editor("commands");
            }
        });
    });
}
pub fn commands(ui: &mut egui::Ui, w: &mut Workspace) {
    ui.horizontal(|ui| {
        for tab in ["Features", "Sketch", "Assembly", "Evaluate", "Graph"] {
            if ui.selectable_label(w.command_tab == tab, tab).clicked() {
                w.command_tab = tab.into();
                if tab == "Graph" {
                    w.view.active_layout = "graph-workspace".into();
                    w.view.mode = "Flow".into();
                } else if w.view.active_layout == "graph-workspace" {
                    w.view.active_layout = "instrument-rail".into();
                    w.view.mode = "Model".into();
                }
            }
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.menu_button("Size", |ui| {
                if ui.button("Compact").clicked() {
                    w.view.ui_scale = 1.;
                    w.view.toolbar_height = 68.;
                    resize_toolbar(w);
                    ui.close();
                }
                if ui.button("Comfortable").clicked() {
                    w.view.ui_scale = 1.15;
                    w.view.toolbar_height = 80.;
                    resize_toolbar(w);
                    ui.close();
                }
                ui.add(egui::Slider::new(&mut w.view.ui_scale, 0.75..=1.5).text("UI scale"));
                if ui
                    .add(
                        egui::Slider::new(&mut w.view.toolbar_height, 60.0..=130.0)
                            .text("Toolbar height"),
                    )
                    .changed()
                {
                    resize_toolbar(w);
                }
            });
        });
    });
    let sketching = w.sketch.is_some();
    let blocked = w.tool_checkpoint.is_some() && !sketching;
    let tab = w.command_tab.clone();
    egui::ScrollArea::horizontal()
        .id_salt("cad-tool-groups")
        .auto_shrink([false, true])
        .show(ui, |ui| {
            ui.horizontal(|ui| match tab.as_str() {
                "Sketch" => {
                    if tool_button(
                        ui,
                        if sketching { "Finish" } else { "Sketch" },
                        Icon::Sketch,
                        !blocked,
                        sketching,
                        "Finish the sketch or select a plane to begin",
                    ) {
                        if sketching {
                            w.confirm_tool();
                        } else {
                            ui::action(w, "sketch.new");
                        }
                    }
                    for (label, kind, tool) in [
                        ("Line", Icon::Line, sketch::Tool::Polyline),
                        ("Rectangle", Icon::Rectangle, sketch::Tool::Rectangle),
                        ("Circle", Icon::Circle, sketch::Tool::Circle),
                        ("Select", Icon::Select, sketch::Tool::Select),
                    ] {
                        if tool_button(
                            ui,
                            label,
                            kind,
                            !blocked,
                            w.sketch.as_ref().is_some_and(|s| s.tool == tool),
                            "Draw on the active sketch plane",
                        ) {
                            if !sketching {
                                ui::action(w, "sketch.new");
                            }
                            sketch::set_tool(w, tool);
                        }
                    }
                    for (group, tools) in [
                        (
                            "Shapes",
                            vec![
                                ("Corner Rectangle", sketch::Tool::Rectangle),
                                ("Center Rectangle", sketch::Tool::CenterRectangle),
                                (
                                    "3-point Corner Rectangle",
                                    sketch::Tool::ThreePointRectangle,
                                ),
                                (
                                    "3-point Center Rectangle",
                                    sketch::Tool::ThreePointCenterRectangle,
                                ),
                                ("Parallelogram", sketch::Tool::Parallelogram),
                                ("Regular Polygon", sketch::Tool::Polygon),
                                ("Straight Slot", sketch::Tool::Slot),
                                ("Centerpoint Straight Slot", sketch::Tool::CenterSlot),
                                ("Arc Slot", sketch::Tool::ArcSlot),
                            ],
                        ),
                        (
                            "Curves",
                            vec![
                                ("Circle", sketch::Tool::Circle),
                                ("Perimeter Circle", sketch::Tool::PerimeterCircle),
                                ("3-point Arc", sketch::Tool::Arc),
                                ("Centerpoint Arc", sketch::Tool::CenterArc),
                                ("Tangent Arc", sketch::Tool::TangentArc),
                                ("Ellipse", sketch::Tool::Ellipse),
                                ("Partial Ellipse", sketch::Tool::EllipseArc),
                                ("Conic", sketch::Tool::Conic),
                                ("Parabola", sketch::Tool::Parabola),
                                ("Control-point Spline", sketch::Tool::Spline),
                                ("Interpolating Spline", sketch::Tool::InterpolatingSpline),
                                ("Control-point NURBS", sketch::Tool::Nurbs),
                            ],
                        ),
                        (
                            "Reference",
                            vec![
                                ("Point", sketch::Tool::Point),
                                ("Centerline", sketch::Tool::Centerline),
                                ("Midpoint Line", sketch::Tool::MidpointLine),
                            ],
                        ),
                    ] {
                        ui.menu_button(group, |ui| {
                            for (label, tool) in &tools {
                                if ui
                                    .add_enabled(!blocked, egui::Button::new(*label))
                                    .clicked()
                                {
                                    if w.sketch.is_none() {
                                        ui::action(w, "sketch.new");
                                    }
                                    sketch::set_tool(w, *tool);
                                    ui.close();
                                }
                            }
                        });
                    }
                    for (menu, category) in [
                        ("Edit Entities", "Edit"),
                        ("Transform", "Transform"),
                        ("Patterns", "Pattern"),
                        ("Blocks", "Block"),
                        ("Dimension Groups", "Dimension Groups"),
                    ] {
                        ui.menu_button(menu, |ui| {
                            for (command, label, group) in cad_core::modeling_tools::SKETCH_TOOLS {
                                if *group == category
                                    && ui
                                        .add_enabled(sketching, egui::Button::new(*label))
                                        .clicked()
                                {
                                    ui::action(w, command);
                                    ui.close();
                                }
                            }
                        });
                    }
                    if ui
                        .add_enabled(sketching, egui::Button::new("Smart Dimension  D"))
                        .clicked()
                    {
                        ui::action(w, "sketch.smart-dimension");
                    }
                    if ui
                        .add_enabled(sketching, egui::Button::new("Text"))
                        .clicked()
                    {
                        ui::action(w, "sketch.text");
                    }
                    if ui
                        .add_enabled(sketching, egui::Button::new("Equation Curve"))
                        .clicked()
                    {
                        ui::action(w, "sketch.equation");
                    }
                    if ui
                        .add_enabled(sketching, egui::Button::new("Reference Dimension"))
                        .clicked()
                    {
                        ui::action(w, "sketch.reference-dimension");
                    }
                    if ui
                        .add_enabled(sketching, egui::Button::new("Relations"))
                        .clicked()
                    {
                        ui::action(w, "sketch.relations");
                    }
                    if ui.button("Normal To").clicked() {
                        sketch::normal_to(w);
                    }
                }
                "Graph" => {
                    for scope in ["Features", "Assembly"] {
                        if ui
                            .selectable_label(w.view.graph_scope == scope, scope)
                            .clicked()
                        {
                            w.view.graph_scope = scope.into();
                        }
                    }
                    if ui.button("Arrange Graph").clicked() {
                        w.view.graph_positions.clear();
                        w.view.graph_pan = [0.; 2];
                        w.view.graph_zoom = 1.;
                    }
                    ui.small("Drag cards · scroll to pan · pinch to zoom · double-click to edit");
                }
                "Assembly" => {
                    for (label, cmd) in [
                        ("Components", "part.new"),
                        ("Mates", "assembly.edit"),
                        ("Library", "libraries"),
                    ] {
                        if ui.button(label).clicked() {
                            ui::action(w, cmd);
                        }
                    }
                    if ui
                        .button(if w.view.isolation {
                            "Finish Editing"
                        } else {
                            "Edit in Place"
                        })
                        .clicked()
                    {
                        w.isolate();
                    }
                    ui.checkbox(&mut w.view.ghost_others, "Ghost others");
                }
                "Evaluate" => {
                    if ui
                        .add_enabled(w.selected.is_some(), egui::Button::new("Measure"))
                        .clicked()
                    {
                        measure(w);
                    }
                    for (label, cmd) in [
                        ("Mass Properties", "inspection.properties"),
                        ("Fit", "viewport.fit"),
                        ("Rebuild", "document.rebuild"),
                        ("BOM", "bom"),
                        ("Drawings", "drawings"),
                    ] {
                        if ui.button(label).clicked() {
                            ui::action(w, cmd);
                        }
                    }
                }
                _ => {
                    if tool_button(
                        ui,
                        "Sketch",
                        Icon::Sketch,
                        !blocked,
                        false,
                        "Create a sketch on an origin plane or model face",
                    ) {
                        ui::action(w, "sketch.new");
                    }
                    for group in [
                        "Create",
                        "Remove",
                        "Modify",
                        "Pattern",
                        "Surfaces",
                        "Mold",
                        "Reference",
                        "Sheet Metal",
                        "Weldments",
                    ] {
                        ui.menu_button(group, |ui| {
                            for tool in cad_core::modeling_tools::TOOLS
                                .iter()
                                .filter(|t| t.group == group)
                            {
                                let has_body = w
                                    .view
                                    .active_part
                                    .as_ref()
                                    .and_then(|id| w.session.visible().parts.get(id))
                                    .is_some_and(|p| {
                                        cad_core::configuration::has_solid_output(
                                            w.session.visible(),
                                            &p.id,
                                        )
                                    });
                                let enabled = !blocked && (!tool.requires_body || has_body);
                                let kind = match tool.preset {
                                    "Extrude" => Icon::Extrude,
                                    "Cut" => Icon::Cut,
                                    "Revolve" => Icon::Revolve,
                                    "Fillet" => Icon::Fillet,
                                    "Chamfer" => Icon::Chamfer,
                                    "Pattern" => Icon::Pattern,
                                    "Shell" => Icon::Shell,
                                    _ => Icon::Sketch,
                                };
                                ui.horizontal(|ui| {
                                    let (r, _) = ui.allocate_exact_size(
                                        egui::vec2(30., 30.),
                                        egui::Sense::hover(),
                                    );
                                    icon(ui.painter(), r.center(), kind, ui.visuals().text_color());
                                    if ui
                                        .add_enabled(enabled, egui::Button::new(tool.label))
                                        .clicked()
                                    {
                                        ui::action(w, tool.command);
                                        ui.close();
                                    }
                                });
                            }
                        });
                    }
                }
            });
        });
}
fn resize_toolbar(w: &mut Workspace) {
    fn resize(node: &mut cad_core::presentation::LayoutNode, height: f32) {
        if node.component.as_deref() == Some("commands") {
            node.height_logical_px = Some(height);
        }
        for child in &mut node.children {
            resize(child, height);
        }
    }
    if let Some(layout) = w.layout.layouts.get_mut(&w.view.active_layout) {
        resize(layout, w.view.toolbar_height);
    }
    w.layout_text = serde_json::to_string_pretty(&w.layout).unwrap_or_default();
}
pub fn navigation(ui: &mut egui::Ui, w: &mut Workspace) {
    ui.horizontal(|ui| {
        for (label, mode) in [
            ("Model", "Model"),
            ("History", "History"),
            ("Graph", "Flow"),
        ] {
            if ui
                .selectable_label(w.manager_tree && w.view.mode == mode, label)
                .clicked()
            {
                w.manager_tree = true;
                w.view.mode = mode.into();
            }
        }
        if ui
            .small_button("⋯")
            .on_hover_text("Layout, theme and sizing")
            .clicked()
        {
            w.open_editor("presentation");
        }
    });
}
pub fn tree(ui: &mut egui::Ui, w: &mut Workspace) {
    let doc = w.session.visible().clone();
    if doc.parts.len() > 1 || w.view.active_part.is_none() {
        let mut active = w.view.active_part.clone();
        egui::ComboBox::from_id_salt("editing-component")
            .selected_text(
                active
                    .as_ref()
                    .map(|id| cad_core::presentation::definition_name(&doc, id))
                    .unwrap_or_else(|| doc.name.clone()),
            )
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut active, None, "Assembly");
                for p in doc.parts.values() {
                    ui.selectable_value(&mut active, Some(p.id.clone()), &p.name);
                }
            });
        if active != w.view.active_part && w.sketch.is_none() {
            w.view.active_part = active;
            w.view.active_instance = None;
            w.view.active_node = None;
            w.active_plane = None;
            w.selected = None;
            w.selected_feature_path = None;
            w.candidates.clear();
            w.context_selection = None;
            w.selection_popup = None;
        }
    }
    if w.view.active_layout == "graph-workspace"
        || (w.view.mode == "Flow" && w.view.active_part.is_none())
    {
        crate::graph_ui::show(
            ui,
            w,
            w.view.active_part.as_ref().and_then(|id| doc.parts.get(id)),
        );
        return;
    }
    let Some(part) = w.view.active_part.as_ref().and_then(|id| doc.parts.get(id)) else {
        egui::ScrollArea::vertical().show(ui, |ui| {
            let response = ui.add(
                egui::Label::new(RichText::new(&doc.name).strong()).sense(egui::Sense::click()),
            );
            if response.clicked() {
                crate::selection_ui::object_selected(
                    w,
                    cad_core::selection_actions::SelectionTarget::Object {
                        definition: doc.root.clone(),
                        object: doc.root.clone(),
                        role: "assembly".into(),
                    },
                );
            }
            for placed in doc.placed_parts().unwrap_or_default() {
                let response = ui.selectable_label(false, &placed.name);
                if response.clicked() {
                    if w.tool_checkpoint.is_none() {
                        w.view.active_part = Some(placed.definition.clone());
                        w.view.active_instance = placed.path.last().cloned();
                    }
                    crate::selection_ui::object_selected(
                        w,
                        cad_core::selection_actions::SelectionTarget::Object {
                            definition: placed.definition,
                            object: placed.path.last().cloned().unwrap_or_default(),
                            role: "component-instance".into(),
                        },
                    );
                }
            }
            if ui.button("Insert Component").clicked() {
                ui::action(w, "part.new");
            }
        });
        return;
    };
    if w.view.mode == "Flow" {
        crate::ui::graph(ui, w, part);
        return;
    }
    if w.view.mode == "History" || w.editing_manager() {
        history(ui, w, part);
        return;
    }
    egui::ScrollArea::vertical()
        .id_salt("cad-feature-tree")
        .show(ui, |ui| {
            let response = ui.add(
                egui::Label::new(RichText::new(&part.name).strong()).sense(egui::Sense::click()),
            );
            if response.clicked() {
                crate::selection_ui::object_selected(
                    w,
                    cad_core::selection_actions::SelectionTarget::Object {
                        definition: part.id.clone(),
                        object: part.id.clone(),
                        role: "part".into(),
                    },
                );
            }
            ui.indent("part-tree", |ui| {
                let effective = cad_core::configuration::effective_part(
                    &doc,
                    &part.id,
                    doc.active_configurations.get(&part.id).map(String::as_str),
                )
                .unwrap_or_else(|_| part.clone());
                for (label, is_surface) in [("Solid Bodies", false), ("Surface Bodies", true)] {
                    let outputs = effective
                        .outputs
                        .iter()
                        .filter_map(|id| part.features.iter().find(|f| &f.id == id))
                        .filter(|f| {
                            if is_surface {
                                f.operation.is_surface()
                            } else {
                                f.operation.produces_solid()
                            }
                        })
                        .collect::<Vec<_>>();
                    if !is_surface || !outputs.is_empty() {
                        egui::CollapsingHeader::new(format!("{label} ({})", outputs.len()))
                            .id_salt(label)
                            .default_open(false)
                            .show(ui, |ui| {
                                for feature in outputs {
                                    feature_row(ui, w, part, feature);
                                }
                            });
                    }
                }
                let material = doc
                    .materials
                    .get(&effective.material)
                    .map(|m| m.name.as_str())
                    .unwrap_or("Unassigned");
                let response = ui.small_button(format!("Material: {material}"));
                if response.clicked() {
                    crate::selection_ui::object_selected(
                        w,
                        cad_core::selection_actions::SelectionTarget::Object {
                            definition: part.id.clone(),
                            object: effective.material.clone(),
                            role: "material".into(),
                        },
                    );
                }
                ui.separator();
                for name in ["Front Plane", "Top Plane", "Right Plane"] {
                    let response = ui.selectable_label(
                        w.active_plane.as_deref() == Some(name),
                        format!("▱  {name}"),
                    );
                    if response.clicked() {
                        w.selected = None;
                        w.view.active_node = None;
                        sketch::choose_origin(w, name);
                        w.status = format!("{name} selected. Choose a sketch or viewing action.");
                        if !response.double_clicked() {
                            crate::selection_ui::object_selected(
                                w,
                                cad_core::selection_actions::SelectionTarget::Reference {
                                    definition: part.id.clone(),
                                    reference: name.into(),
                                },
                            );
                        }
                    }
                    if response.double_clicked() && w.sketch.is_none() {
                        ui::action(w, "sketch.new");
                    }
                    response.context_menu(|ui| {
                        if ui.button("Sketch on this plane").clicked() {
                            sketch::choose_origin(w, name);
                            if w.sketch.is_none() {
                                ui::action(w, "sketch.new");
                            }
                            ui.close();
                        }
                    });
                }
                let response = ui.selectable_label(false, "+  Origin");
                if response.clicked() {
                    w.active_plane = None;
                    crate::selection_ui::object_selected(
                        w,
                        cad_core::selection_actions::SelectionTarget::Reference {
                            definition: part.id.clone(),
                            reference: "Origin".into(),
                        },
                    );
                }
                ui.separator();
                for feature in &part.features {
                    let consumed = matches!(feature.operation, Operation::Profile { .. })
                        && part.features.iter().any(|f| {
                            part.feature_dependencies(f)
                                .iter()
                                .any(|n| n.definition == part.id && n.node == feature.id)
                        });
                    if consumed {
                        continue;
                    }
                    feature_row(ui, w, part, feature);
                    if !matches!(feature.operation, Operation::Profile { .. }) {
                        for input in part.feature_dependencies(feature) {
                            if let Some(sketch) = part.features.iter().find(|f| {
                                f.id == input.node
                                    && matches!(f.operation, Operation::Profile { .. })
                            }) {
                                ui.indent((&feature.id, &sketch.id), |ui| {
                                    feature_row(ui, w, part, sketch);
                                });
                            }
                        }
                    }
                }
                if let Some(s) = &w.sketch {
                    ui.colored_label(
                        ui.visuals().selection.stroke.color,
                        format!("{} (Editing)", s.name),
                    );
                }
            });
        });
}
pub fn operation_summary(operation: &Operation) -> String {
    match operation {
        Operation::ReferencePlane { definition } => definition.label().into(),
        Operation::ReferenceAxis { definition } => definition.label().into(),
        Operation::ReferencePoint { definition } => definition.label().into(),
        Operation::ReferenceCoordinateSystem { definition } => format!(
            "{:?} / {:?} · Right-handed",
            definition.primary_axis, definition.secondary_axis
        ),
        Operation::Extrude {
            distance,
            options,
            mode,
            ..
        } => format!(
            "{} · {}",
            if mode == "cut" { "Cut" } else { "Extrude" },
            match options.end {
                cad_core::ExtrudeEnd::Blind | cad_core::ExtrudeEnd::MidPlane => distance.0.as_str(),
                cad_core::ExtrudeEnd::ThroughAll => "Through All",
                cad_core::ExtrudeEnd::ThroughAllBoth => "Through All Both",
                cad_core::ExtrudeEnd::UpToFace => "Up to Face",
                cad_core::ExtrudeEnd::UpToVertex => "Up to Vertex",
                cad_core::ExtrudeEnd::UpToNext => "Up to Next",
                cad_core::ExtrudeEnd::UpToBody => "Up to Body",
            }
        ),
        Operation::Fillet { radius, .. } => format!("Radius {}", radius.0),
        Operation::Chamfer { distance, .. } => format!("Distance {}", distance.0),
        Operation::Revolve { angle, .. } => format!("Angle {}", angle.0),
        Operation::Shell { thickness, .. } => format!("Thickness {}", thickness.0),
        Operation::Draft { angle, .. } => format!("Draft {}", angle.0),
        Operation::Loft { profiles, .. } => format!("{} sections", profiles.len()),
        Operation::Sweep {
            path, path_sketch, ..
        } => {
            if path_sketch.is_some() {
                "Sketch path".into()
            } else {
                format!("{} stations", path.len())
            }
        }
        Operation::Pattern { count, .. } => format!("{count} instances"),
        Operation::Mirror { keep_original, .. } => {
            if *keep_original {
                "Copy and mirror".into()
            } else {
                "Mirror body".into()
            }
        }
        _ => operation.name().into(),
    }
}
fn history(ui: &mut egui::Ui, w: &mut Workspace, part: &Part) {
    let accent = ui.visuals().selection.stroke.color;
    if ui
        .selectable_label(w.view.history_cursor.is_none(), "Latest model")
        .on_hover_text("Show the current model without rebuilding geometry")
        .clicked()
    {
        crate::history::latest(w);
    }
    if w.view.history_cursor.is_some() {
        ui.small("History inspection · Esc returns to Latest");
    }
    egui::ScrollArea::vertical()
        .id_salt("feature-history")
        .show(ui, |ui| {
            if part.features.is_empty() {
                ui.small("Sketches and features appear here.");
            }
            let mut last = None;
            for (index, feature) in part.features.iter().enumerate() {
                let active = w
                    .view
                    .history_cursor
                    .as_ref()
                    .map(|c| c.definition == part.id && c.node == feature.id)
                    .unwrap_or_else(|| w.view.active_node.as_ref() == Some(&feature.id));
                let future = w
                    .view
                    .history_cursor
                    .as_ref()
                    .filter(|c| c.definition == part.id)
                    .and_then(|c| part.features.iter().position(|f| f.id == c.node))
                    .is_some_and(|i| index > i);
                let (rect, response) = ui.allocate_exact_size(
                    egui::vec2(ui.available_width(), if active { 48. } else { 31. }),
                    egui::Sense::click(),
                );
                let p = rect.left_top() + egui::vec2(35., 15.);
                if let Some(last) = last {
                    ui.painter()
                        .line_segment([last, p], Stroke::new(1.2, ui.visuals().weak_text_color()));
                }
                last = Some(p);
                ui.painter().text(
                    p - egui::vec2(19., 0.),
                    egui::Align2::RIGHT_CENTER,
                    format!("{:02}", index + 1),
                    egui::FontId::proportional(10.),
                    ui.visuals().weak_text_color(),
                );
                ui.painter().circle(
                    p,
                    if active { 6. } else { 4. },
                    if active {
                        accent
                    } else if future {
                        ui.visuals().weak_text_color()
                    } else {
                        ui.visuals().text_color()
                    },
                    Stroke::NONE,
                );
                if active
                    && w.fields
                        .get("history-focus")
                        .is_none_or(|id| id != &feature.id)
                {
                    ui.scroll_to_rect(rect, Some(egui::Align::Center));
                    w.fields.insert("history-focus".into(), feature.id.clone());
                }
                if active {
                    ui.painter().circle_stroke(p, 9., Stroke::new(1.5, accent));
                }
                ui.painter().text(
                    p + egui::vec2(16., 0.),
                    egui::Align2::LEFT_CENTER,
                    &feature.name,
                    egui::FontId::proportional(12.),
                    if active {
                        accent
                    } else if future {
                        ui.visuals().weak_text_color()
                    } else {
                        ui.visuals().text_color()
                    },
                );
                if active {
                    ui.painter().text(
                        p + egui::vec2(16., 16.),
                        egui::Align2::LEFT_CENTER,
                        operation_summary(&feature.operation),
                        egui::FontId::proportional(10.),
                        ui.visuals().weak_text_color(),
                    );
                }
                if response.clicked()
                    && (w.tool_checkpoint.is_some()
                        || response.double_clicked()
                        || crate::history::inspect(w, part.reference(&feature.id)))
                {
                    crate::selection_ui::feature_selected(w, &part.id, feature, &response);
                }
                response.context_menu(|ui| {
                    let target = cad_core::selection_actions::SelectionTarget::Feature {
                        definition: part.id.clone(),
                        feature: feature.id.clone(),
                    };
                    crate::selection_ui::contents(ui, w, &target);
                });
            }
        });
}
fn feature_row(ui: &mut egui::Ui, w: &mut Workspace, part: &Part, feature: &Feature) {
    let response = ui.selectable_label(
        w.view.active_node.as_ref() == Some(&feature.id),
        format!(
            "{}  {}",
            if cad_core::configuration::feature_suppressed(w.session.visible(), &part.id, feature) {
                "○"
            } else if matches!(feature.operation, Operation::Profile { .. }) {
                "▱"
            } else {
                "◇"
            },
            feature.name
        ),
    );
    if response.clicked() {
        crate::selection_ui::feature_selected(w, &part.id, feature, &response);
    }
    response.context_menu(|ui| {
        let target = cad_core::selection_actions::SelectionTarget::Feature {
            definition: part.id.clone(),
            feature: feature.id.clone(),
        };
        crate::selection_ui::contents(ui, w, &target);
    });
}

pub fn properties(ui: &mut egui::Ui, w: &mut Workspace) {
    let doc = w.session.visible().clone();
    let part = w.view.active_part.as_ref().and_then(|id| doc.parts.get(id));
    ui.strong(part.map(|p| p.name.as_str()).unwrap_or(&doc.name));
    ui.separator();
    if let Some(feature) = part.and_then(|p| {
        w.view
            .active_node
            .as_ref()
            .and_then(|id| p.features.iter().find(|f| &f.id == id))
    }) {
        ui.label(&feature.name);
        ui.small(feature.operation.name());
        if ui
            .button(if matches!(feature.operation, Operation::Profile { .. }) {
                "Edit Sketch"
            } else {
                "Edit Feature"
            })
            .clicked()
        {
            ui::feature_dialog(w, Some(feature.clone()));
        }
    } else {
        ui.label("Select a plane or model feature.");
        ui.small("Double-click a sketch or feature in the tree to edit it.");
    }
    let open_parameters = w.fields.remove("graph-parameters").is_some();
    egui::CollapsingHeader::new("Named Parameters")
        .open(open_parameters.then_some(true))
        .show(ui, |ui| {
            crate::ui::parameters(ui, w);
        });
}
pub fn confirmation(ui: &mut egui::Ui, w: &mut Workspace) {
    ui.horizontal(|ui| {
        let closed = w.sketch.as_ref().is_none_or(|s| {
            s.closed
                && s.error.is_none()
                && !s.choosing_plane
                && s.edit.is_none()
                && s.clicks.is_empty()
                && (!s.tool.is_spline() || s.chain.is_empty())
        });
        let enabled = closed
            && !w.confirm_requested
            && !w.fields.contains_key("feature-source-invalid")
            && !w.fields.contains_key("mate-source-invalid")
            && !w.fields.contains_key("drawing-source-invalid")
            && !w.fields.contains_key("component-pattern-source-invalid")
            && !w.fields.contains_key("configuration-source-invalid")
            && !w.fields.contains_key("exploded-view-source-invalid")
            && !w.fields.contains_key("assembly-inspection-source-invalid");
        let (rect, response) = ui.allocate_exact_size(egui::vec2(44., 44.), egui::Sense::click());
        ui.painter().circle(
            rect.center(),
            20.,
            if enabled {
                ui.visuals().selection.stroke.color
            } else {
                ui.visuals().widgets.inactive.bg_fill
            },
            Stroke::new(1.3, ui.visuals().text_color()),
        );
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "Apply",
            egui::FontId::proportional(11.),
            if enabled {
                Color32::BLACK
            } else {
                ui.visuals().weak_text_color()
            },
        );
        response
            .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, "Apply"));
        if enabled && response.clicked() {
            w.confirm_tool();
        }
        response.on_hover_text("Confirm this operation · Command + Enter");
        if ui
            .button(RichText::new("×  Cancel").color(Color32::from_rgb(165, 45, 40)))
            .clicked()
        {
            w.cancel_tool();
        }
    });
    if w.confirm_requested {
        ui.small("Rebuilding before confirmation…");
    }
    if let Some(error) = w.session.draft.as_ref().and_then(|d| d.error.as_ref()) {
        ui.colored_label(Color32::from_rgb(170, 50, 35), error);
    }
}
pub(crate) fn expression(ui: &mut egui::Ui, label: &str, expression: &mut cad_core::Expr) -> bool {
    ui.label(label);
    ui.add(
        egui::TextEdit::singleline(&mut expression.0)
            .id(ui.id().with(("feature-expression", label)))
            .desired_width(f32::INFINITY),
    )
    .changed()
}

fn face_selection_fields(
    ui: &mut egui::Ui,
    faces: &mut Vec<cad_core::TopologyRef>,
    selected: Option<&cad_core::TopologyRef>,
) -> bool {
    let mut changed = false;
    if let Some(selected) = selected.filter(|r| r.topology == "face") {
        let present = faces
            .iter()
            .any(|r| r.producer == selected.producer && r.role == selected.role);
        if ui
            .add_enabled(!present, egui::Button::new("Add selected face"))
            .clicked()
        {
            faces.push(selected.clone());
            changed = true;
        }
    }
    let mut remove = None;
    for (index, face) in faces.iter().enumerate() {
        ui.horizontal(|ui| {
            ui.label(format!("Face {}", index + 1));
            if let Some(signature) = &face.signature {
                ui.small(format!(
                    "({:.1}, {:.1}, {:.1}) mm",
                    signature.center[0], signature.center[1], signature.center[2]
                ));
            }
            if ui.small_button("Remove").clicked() {
                remove = Some(index);
            }
        });
    }
    if let Some(index) = remove {
        faces.remove(index);
        changed = true;
    }
    changed
}
fn selected_edges(ui: &mut egui::Ui, edges: &mut Vec<cad_core::TopologyRef>) -> bool {
    let mut remove = None;
    for index in 0..edges.len() {
        ui.push_id(("selected-edge", index), |ui| {
            ui.horizontal(|ui| {
                ui.label(format!("Edge {}", index + 1));
                if ui.small_button("Remove").clicked() {
                    remove = Some(index);
                }
            });
        });
    }
    if let Some(index) = remove {
        edges.remove(index);
        true
    } else {
        false
    }
}
pub(crate) fn profile_selector(
    ui: &mut egui::Ui,
    doc: &cad_core::Document,
    current: &str,
    label: &str,
    reference: &mut cad_core::NodeRef,
) -> bool {
    let name = doc
        .parts
        .get(&reference.definition)
        .and_then(|p| p.features.iter().find(|f| f.id == reference.node))
        .map(|f| f.name.as_str())
        .unwrap_or("Select sketch");
    let mut changed = false;
    egui::ComboBox::from_id_salt(("profile", label))
        .selected_text(format!("{label}: {name}"))
        .show_ui(ui, |ui| {
            for part in doc.parts.values() {
                for feature in part
                    .features
                    .iter()
                    .take_while(|f| f.id != current)
                    .filter(|f| !f.suppressed && matches!(f.operation, Operation::Profile { .. }))
                {
                    changed |= ui
                        .selectable_value(
                            reference,
                            part.reference(&feature.id),
                            format!("{} / {}", part.name, feature.name),
                        )
                        .changed();
                }
            }
        });
    changed
}
fn sweep_path_selector(
    ui: &mut egui::Ui,
    doc: &cad_core::Document,
    current: &str,
    reference: &mut cad_core::NodeRef,
) -> bool {
    let name = doc
        .parts
        .get(&reference.definition)
        .and_then(|part| {
            part.features
                .iter()
                .find(|feature| feature.id == reference.node)
        })
        .map(|feature| feature.name.as_str())
        .unwrap_or("Select path");
    let mut changed = false;
    egui::ComboBox::from_id_salt("sweep-path-source")
        .selected_text(format!("Path: {name}"))
        .show_ui(ui, |ui| {
            for part in doc.parts.values() {
                for feature in part
                    .features
                    .iter()
                    .take_while(|feature| feature.id != current)
                    .filter(|feature| {
                        !feature.suppressed
                            && matches!(
                                feature.operation,
                                Operation::Profile { .. } | Operation::ReferenceCurve { .. }
                            )
                    })
                {
                    changed |= ui
                        .selectable_value(
                            reference,
                            part.reference(&feature.id),
                            format!("{} / {}", part.name, feature.name),
                        )
                        .changed();
                }
            }
        });
    changed
}
fn result_fields(
    ui: &mut egui::Ui,
    doc: &cad_core::Document,
    current: &str,
    body: &mut Option<cad_core::NodeRef>,
    mode: &mut String,
) -> bool {
    let mut changed = false;
    egui::ComboBox::from_label("Result")
        .selected_text(match mode.as_str() {
            "cut" => "Remove Material",
            "intersect" => "Common Material",
            "join" => "Merge Result",
            _ => "New Body",
        })
        .show_ui(ui, |ui| {
            for (value, label) in [
                ("new", "New Body"),
                ("join", "Merge Result"),
                ("cut", "Remove Material"),
                ("intersect", "Common Material"),
            ] {
                changed |= ui.selectable_value(mode, value.into(), label).changed();
            }
        });
    let candidates = doc
        .parts
        .values()
        .flat_map(|p| {
            p.features
                .iter()
                .take_while(|f| f.id != current)
                .filter(|f| !f.suppressed && f.operation.produces_solid())
                .map(|f| (p.reference(&f.id), format!("{} / {}", p.name, f.name)))
        })
        .collect::<Vec<_>>();
    if body.is_none() && mode != "new" && !mode.is_empty() {
        *body = candidates.last().map(|(r, _)| r.clone());
        changed = true;
    }
    if mode != "new" {
        let label = body
            .as_ref()
            .and_then(|r| candidates.iter().find(|(candidate, _)| candidate == r))
            .map(|(_, name)| name.as_str())
            .unwrap_or("Select target body");
        egui::ComboBox::from_id_salt("result-body")
            .selected_text(label)
            .show_ui(ui, |ui| {
                for (reference, name) in &candidates {
                    changed |= ui
                        .selectable_value(body, Some(reference.clone()), name)
                        .changed();
                }
            });
    }
    changed
}
fn body_selector(
    ui: &mut egui::Ui,
    doc: &cad_core::Document,
    current: &str,
    label: &str,
    body: &mut Option<cad_core::NodeRef>,
) -> bool {
    let candidates = doc
        .parts
        .values()
        .flat_map(|p| {
            p.features
                .iter()
                .take_while(|f| f.id != current)
                .filter(|f| !f.suppressed && f.operation.produces_solid())
                .map(|f| (p.reference(&f.id), format!("{} / {}", p.name, f.name)))
        })
        .collect::<Vec<_>>();
    let selected = body
        .as_ref()
        .and_then(|source| candidates.iter().find(|(candidate, _)| candidate == source))
        .map(|(_, name)| name.as_str())
        .unwrap_or("Select body");
    let mut changed = false;
    egui::ComboBox::from_label(label)
        .selected_text(selected)
        .show_ui(ui, |ui| {
            for (reference, name) in candidates {
                changed |= ui.selectable_value(body, Some(reference), name).changed();
            }
        });
    changed
}
fn extrusion_vertex_field(
    ui: &mut egui::Ui,
    doc: &cad_core::Document,
    current: &str,
    label: &str,
    selected: &mut Option<(cad_core::NodeRef, cad_core::Id)>,
) -> bool {
    let vertices = doc
        .parts
        .values()
        .flat_map(|part| {
            part.features
                .iter()
                .take_while(|f| f.id != current)
                .filter(|f| !f.suppressed)
                .filter_map(move |feature| {
                    if let Operation::Profile {
                        profile: cad_core::Profile::Sketch { sketch },
                        ..
                    } = &feature.operation
                    {
                        part.sketches
                            .get(sketch)
                            .map(|sketch| (part, feature, sketch))
                    } else {
                        None
                    }
                })
                .flat_map(|(part, feature, sketch)| {
                    sketch.points.iter().map(move |(id, p)| {
                        (
                            (part.reference(&feature.id), id.clone()),
                            format!(
                                "{} / {}: ({:.3}, {:.3})",
                                part.name, feature.name, p[0], p[1]
                            ),
                        )
                    })
                })
        })
        .collect::<Vec<_>>();
    let name = selected
        .as_ref()
        .and_then(|selected| vertices.iter().find(|(value, _)| value == selected))
        .map(|(_, name)| name.as_str())
        .unwrap_or("Select sketch vertex");
    let mut changed = false;
    egui::ComboBox::from_label(label)
        .selected_text(name)
        .show_ui(ui, |ui| {
            for (value, name) in vertices {
                changed |= ui.selectable_value(selected, Some(value), name).changed();
            }
        });
    changed
}
fn extrusion_end_fields(
    ui: &mut egui::Ui,
    w: &Workspace,
    doc: &cad_core::Document,
    current: &str,
    end: &mut cad_core::extrusion::DirectionEnd,
    combined: bool,
) -> bool {
    use cad_core::ExtrudeEnd;
    let mut changed = false;
    let before = end.end.clone();
    let name = match end.end {
        ExtrudeEnd::Blind => "Blind",
        ExtrudeEnd::MidPlane => "Mid-plane",
        ExtrudeEnd::ThroughAll => "Through All",
        ExtrudeEnd::ThroughAllBoth => "Through All Both",
        ExtrudeEnd::UpToFace => "Up to Face / Offset from Face",
        ExtrudeEnd::UpToVertex => "Up to Vertex",
        ExtrudeEnd::UpToNext => "Up to Next",
        ExtrudeEnd::UpToBody => "Up to Body",
    };
    egui::ComboBox::from_label("End Condition")
        .selected_text(name)
        .show_ui(ui, |ui| {
            for (value, name) in [
                (ExtrudeEnd::Blind, "Blind"),
                (ExtrudeEnd::MidPlane, "Mid-plane"),
                (ExtrudeEnd::ThroughAll, "Through All"),
                (ExtrudeEnd::ThroughAllBoth, "Through All Both"),
                (ExtrudeEnd::UpToFace, "Up to Face / Offset from Face"),
                (ExtrudeEnd::UpToVertex, "Up to Vertex"),
                (ExtrudeEnd::UpToNext, "Up to Next"),
                (ExtrudeEnd::UpToBody, "Up to Body"),
            ] {
                if combined || !matches!(value, ExtrudeEnd::MidPlane | ExtrudeEnd::ThroughAllBoth) {
                    changed |= ui.selectable_value(&mut end.end, value, name).changed();
                }
            }
        });
    if before != end.end {
        if end.end != ExtrudeEnd::UpToFace {
            end.target = None;
        }
        if !matches!(end.end, ExtrudeEnd::UpToFace | ExtrudeEnd::UpToBody) {
            end.target_body = None;
        }
        if end.end != ExtrudeEnd::UpToVertex {
            end.target_vertex = None;
        }
    }
    match end.end {
        ExtrudeEnd::Blind | ExtrudeEnd::MidPlane => {
            changed |= expression(ui, "Depth", &mut end.distance)
        }
        ExtrudeEnd::UpToFace => {
            ui.label(if end.target.is_some() {
                "End reference selected"
            } else {
                "Select an end face or datum plane"
            });
            if let Some(selected) = w.selected.as_ref().filter(|s| {
                matches!(s.reference.topology.as_str(), "face" | "plane")
                    && s.reference.producer.node != current
            }) && ui.button("Use Selected End Face / Plane").clicked()
            {
                end.target = Some(selected.reference.clone());
                end.target_body = Some(crate::reference_ui::selected_body(w, selected));
                changed = true;
            }
            changed |= expression(ui, "Offset from End Face", &mut end.end_offset);
            ui.small("Planar carriers, or complete bounded positive rational graph patches over rectangular sections. Curved end offsets require an exact offset surface.");
        }
        ExtrudeEnd::UpToVertex => {
            changed |=
                extrusion_vertex_field(ui, doc, current, "End Vertex", &mut end.target_vertex);
            changed |= expression(ui, "Offset from End Vertex", &mut end.end_offset);
        }
        ExtrudeEnd::UpToBody => {
            changed |= body_selector(ui, doc, current, "Termination Body", &mut end.target_body);
            changed |= expression(ui, "Offset from Body", &mut end.end_offset);
        }
        ExtrudeEnd::UpToNext => {
            ui.small("Finds the nearest complete forward face in Feature Scope. Piecewise termination across several faces needs exact segmented construction.");
            changed |= expression(ui, "Offset from Next Face", &mut end.end_offset);
        }
        ExtrudeEnd::ThroughAll | ExtrudeEnd::ThroughAllBoth => {
            ui.small("Through All follows the finite exact bounds of Feature Scope.");
        }
    };
    changed
}
fn extrusion_start_fields(
    ui: &mut egui::Ui,
    w: &Workspace,
    doc: &cad_core::Document,
    current: &str,
    start: &mut cad_core::extrusion::Start,
) -> bool {
    use cad_core::extrusion::Start;
    let mut changed = false;
    let picked = w.selected.as_ref().filter(|s| {
        matches!(s.reference.topology.as_str(), "face" | "plane")
            && s.reference.producer.node != current
    });
    egui::ComboBox::from_label("From")
        .selected_text(match start {
            Start::SketchPlane => "Sketch Plane",
            Start::Face { .. } => "Face / Plane",
            Start::Vertex { .. } => "Vertex",
            Start::Point { .. } => "Reference Point",
        })
        .show_ui(ui, |ui| {
            if ui
                .selectable_label(matches!(start, Start::SketchPlane), "Sketch Plane")
                .clicked()
            {
                *start = Start::SketchPlane;
                changed = true;
            }
            if ui
                .selectable_label(matches!(start, Start::Point { .. }), "Reference Point")
                .clicked()
                && !matches!(start, Start::Point { .. })
            {
                *start = Start::Point {
                    point: Box::default(),
                };
                changed = true;
            }
            if ui
                .add_enabled(picked.is_some(), egui::Button::new("Selected Face / Plane"))
                .clicked()
                && let Some(selected) = picked
            {
                *start = Start::Face {
                    face: Box::new(selected.reference.clone()),
                    body: crate::reference_ui::selected_body(w, selected),
                };
                changed = true;
            }
        });
    if picked.is_none() {
        ui.small("Select a model face or datum plane to use it as the start.");
    }
    if let Start::Point { point } = start {
        changed |= crate::reference_ui::point(ui, w, doc, current, "Start Point", point);
        ui.small("The start plane passes through this point. Start Offset moves it along the extrusion direction.");
    }
    let mut vertex = if let Start::Vertex { profile, point } = start {
        Some((profile.clone(), point.clone()))
    } else {
        None
    };
    if extrusion_vertex_field(ui, doc, current, "From Vertex", &mut vertex)
        && let Some((profile, point)) = vertex
    {
        *start = Start::Vertex { profile, point };
        changed = true;
    }
    changed
}
fn revolution_thin_fields(ui: &mut egui::Ui, thin: &mut Option<cad_core::ThinExtrude>) -> bool {
    let mut changed = false;
    let mut enabled = thin.is_some();
    if ui.checkbox(&mut enabled, "Thin Feature").changed() {
        *thin = enabled.then(|| cad_core::ThinExtrude {
            mode: cad_core::ThinExtrudeMode::OneSide,
            thickness: "2 mm".into(),
            second_thickness: None,
            reverse: false,
        });
        changed = true;
    }
    if let Some(thin) = thin {
        egui::ComboBox::from_label("Thickness Mode")
            .selected_text(match thin.mode {
                cad_core::ThinExtrudeMode::OneSide => "One Side",
                cad_core::ThinExtrudeMode::TwoSide => "Two Sides",
                cad_core::ThinExtrudeMode::MidPlane => "Mid Plane",
            })
            .show_ui(ui, |ui| {
                for (mode, label) in [
                    (cad_core::ThinExtrudeMode::OneSide, "One Side"),
                    (cad_core::ThinExtrudeMode::TwoSide, "Two Sides"),
                    (cad_core::ThinExtrudeMode::MidPlane, "Mid Plane"),
                ] {
                    changed |= ui.selectable_value(&mut thin.mode, mode, label).changed();
                }
            });
        changed |= expression(ui, "Thickness", &mut thin.thickness);
        if thin.mode == cad_core::ThinExtrudeMode::TwoSide {
            if thin.second_thickness.is_none() {
                thin.second_thickness = Some("2 mm".into());
                changed = true;
            }
            changed |= expression(ui, "Thickness 2", thin.second_thickness.as_mut().unwrap());
        }
        changed |= ui
            .checkbox(&mut thin.reverse, "Reverse Thickness Side")
            .changed();
        ui.small("Exact straight and circular offsets; open ends receive flat caps.");
    }
    changed
}

pub fn feature_fields(ui: &mut egui::Ui, w: &mut Workspace, feature: &mut Feature) -> bool {
    if matches!(feature.operation, Operation::ReferenceCurve { .. }) {
        return crate::reference_curves_ui::fields(ui, w, feature);
    }
    if feature.operation.is_reference() {
        return crate::reference_ui::fields(ui, w, feature);
    }
    let doc = w.session.visible().clone();
    let part = w.view.active_part.as_ref().and_then(|id| doc.parts.get(id));
    let name = |reference: &cad_core::NodeRef| {
        doc.parts
            .get(&reference.definition)
            .and_then(|p| p.features.iter().find(|f| f.id == reference.node))
            .map(|f| f.name.clone())
            .unwrap_or_else(|| "Missing selection".into())
    };
    let mut changed = false;
    match &mut feature.operation {
        Operation::Rib { definition } => {
            changed |= crate::rib_ui::fields(ui, w, &doc, &feature.id, definition)
        }
        Operation::DirectEdit { definition } => {
            changed |= crate::direct_edit_ui::fields(ui, w, &doc, &feature.id, definition)
        }
        Operation::BodyFilter { definition } => {
            changed |= crate::body_filter_ui::fields(ui, w, &doc, &feature.id, definition)
        }
        Operation::Split { definition } => {
            changed |= crate::split_ui::fields(ui, w, &doc, &feature.id, definition)
        }
        Operation::CurvePattern { definition } => {
            changed |= crate::curve_pattern_ui::fields(ui, w, &doc, &feature.id, definition)
        }
        Operation::PlacementPattern { definition } => {
            changed |= crate::placement_pattern_ui::fields(ui, w, &doc, &feature.id, definition)
        }
        Operation::DrilledHoles { definition } => {
            changed |= crate::holes_ui::fields(ui, w, &feature.id, definition);
        }
        Operation::Sheet { operation } => {
            changed |= crate::fabrication_ui::sheet_fields(ui, w, &feature.id, operation);
        }
        Operation::Weldment { operation } => {
            changed |= crate::fabrication_ui::weldment_fields(ui, w, &feature.id, operation);
        }
        Operation::Surface { operation } => {
            changed |= crate::surface_ui::fields(ui, w, &feature.id, operation);
        }
        Operation::Mold { operation } => {
            changed |= crate::mold_ui::fields(ui, w, &feature.id, operation);
        }
        Operation::Extrude {
            profile,
            distance,
            direction,
            body,
            mode,
            options,
        } => {
            changed |= profile_selector(ui, &doc, &feature.id, "Profile", profile);
            changed |= crate::contours_ui::fields(
                ui,
                &doc,
                std::slice::from_ref(profile),
                &mut options.contours,
                options.thin.is_some(),
            );
            ui.separator();
            let previous_normal = options.profile_normal;
            egui::ComboBox::from_label("Extrusion Direction")
                .selected_text(if options.profile_normal {
                    "Normal to Sketch"
                } else {
                    "Specified Vector"
                })
                .show_ui(ui, |ui| {
                    changed |= ui
                        .selectable_value(&mut options.profile_normal, true, "Normal to Sketch")
                        .changed();
                    changed |= ui
                        .selectable_value(&mut options.profile_normal, false, "Specified Vector")
                        .changed();
                });
            if previous_normal && !options.profile_normal {
                let plane = w
                    .session
                    .draft
                    .as_ref()
                    .and_then(|d| d.evaluation.as_ref())
                    .or(w.session.evaluation.as_ref())
                    .and_then(|e| {
                        e.sketches
                            .get(&format!("{}::{}", profile.definition, profile.node))
                    })
                    .map(|s| &s.plane)
                    .or_else(|| {
                        doc.parts
                            .get(&profile.definition)
                            .and_then(|p| p.features.iter().find(|f| f.id == profile.node))
                            .and_then(|f| {
                                if let Operation::Profile { plane, .. } = &f.operation {
                                    Some(plane)
                                } else {
                                    None
                                }
                            })
                    });
                if let Some(plane) = plane {
                    *direction = [
                        plane.u[1] * plane.v[2] - plane.u[2] * plane.v[1],
                        plane.u[2] * plane.v[0] - plane.u[0] * plane.v[2],
                        plane.u[0] * plane.v[1] - plane.u[1] * plane.v[0],
                    ]
                    .map(|v| v * if options.reverse_direction { -1. } else { 1. });
                }
            }
            if !options.profile_normal {
                ui.horizontal(|ui| {
                    for (label, value) in ["X", "Y", "Z"].into_iter().zip(direction.iter_mut()) {
                        changed |= ui
                            .add(
                                egui::DragValue::new(value)
                                    .prefix(format!("{label} "))
                                    .speed(0.01),
                            )
                            .changed();
                    }
                });
            } else {
                ui.small("Direction follows changes to the sketch plane.");
            }
            ui.separator();
            ui.strong("Direction 1");
            let mut first = cad_core::extrusion::DirectionEnd {
                end: options.end.clone(),
                distance: distance.clone(),
                target: options.target.clone(),
                target_body: options.target_body.clone(),
                target_vertex: options.target_vertex.clone(),
                end_offset: options.end_offset.clone(),
            };
            changed |= extrusion_end_fields(ui, w, &doc, &feature.id, &mut first, true);
            options.end = first.end;
            *distance = first.distance;
            options.target = first.target;
            options.target_body = first.target_body;
            options.target_vertex = first.target_vertex;
            options.end_offset = first.end_offset;
            egui::CollapsingHeader::new("From and Direction 2").show(ui, |ui| {
                changed |= extrusion_start_fields(ui, w, &doc, &feature.id, &mut options.start);
                changed |= expression(ui, "Start Offset", &mut options.start_offset);
                let combined = matches!(
                    options.end,
                    cad_core::ExtrudeEnd::MidPlane | cad_core::ExtrudeEnd::ThroughAllBoth
                );
                if combined && (options.direction2.is_some() || options.reverse_distance.is_some())
                {
                    options.direction2 = None;
                    options.reverse_distance = None;
                    changed = true;
                }
                let mut second = options.direction2.is_some()
                    || (options.end == cad_core::ExtrudeEnd::Blind
                        && options.reverse_distance.is_some());
                if ui
                    .add_enabled(!combined, egui::Checkbox::new(&mut second, "Direction 2"))
                    .changed()
                {
                    options.direction2 = second.then(cad_core::extrusion::DirectionEnd::default);
                    options.reverse_distance = None;
                    changed = true;
                }
                if second && options.direction2.is_none() {
                    options.direction2 = Some(cad_core::extrusion::DirectionEnd {
                        distance: options
                            .reverse_distance
                            .take()
                            .unwrap_or_else(|| "10 mm".into()),
                        ..Default::default()
                    });
                    changed = true;
                }
                if let Some(end) = &mut options.direction2 {
                    ui.push_id("direction-two", |ui| {
                        changed |= extrusion_end_fields(ui, w, &doc, &feature.id, end, false);
                    });
                }
            });
            ui.small("Lengths accept units or expressions: 20 mm, thickness / 2.");
            if ui.button("Reverse Direction").clicked() {
                if options.profile_normal {
                    options.reverse_direction = !options.reverse_direction;
                } else {
                    *direction = direction.map(|v| -v);
                }
                changed = true;
            }
            let mut thin = options.thin.is_some();
            if ui.checkbox(&mut thin, "Thin Feature").changed() {
                options.thin = thin.then(|| cad_core::ThinExtrude {
                    mode: cad_core::ThinExtrudeMode::OneSide,
                    thickness: "2 mm".into(),
                    second_thickness: None,
                    reverse: false,
                });
                changed = true;
            }
            if let Some(thin) = &mut options.thin {
                egui::ComboBox::from_label("Thickness Mode")
                    .selected_text(match thin.mode {
                        cad_core::ThinExtrudeMode::OneSide => "One Side",
                        cad_core::ThinExtrudeMode::TwoSide => "Two Sides",
                        cad_core::ThinExtrudeMode::MidPlane => "Mid-plane",
                    })
                    .show_ui(ui, |ui| {
                        for (mode, label) in [
                            (cad_core::ThinExtrudeMode::OneSide, "One Side"),
                            (cad_core::ThinExtrudeMode::TwoSide, "Two Sides"),
                            (cad_core::ThinExtrudeMode::MidPlane, "Mid-plane"),
                        ] {
                            changed |= ui.selectable_value(&mut thin.mode, mode, label).changed();
                        }
                    });
                changed |= expression(
                    ui,
                    if thin.mode == cad_core::ThinExtrudeMode::MidPlane {
                        "Total Thickness"
                    } else {
                        "Side 1 Thickness"
                    },
                    &mut thin.thickness,
                );
                if thin.mode == cad_core::ThinExtrudeMode::TwoSide {
                    if thin.second_thickness.is_none() {
                        thin.second_thickness = Some("2 mm".into());
                        changed = true;
                    }
                    changed |= expression(
                        ui,
                        "Side 2 Thickness",
                        thin.second_thickness.as_mut().unwrap(),
                    );
                }
                if thin.mode != cad_core::ThinExtrudeMode::MidPlane {
                    changed |= ui
                        .checkbox(&mut thin.reverse, "Reverse Thickness Side")
                        .changed();
                }
                ui.small("Straight lines, circular arcs, and circles. Open ends receive flat caps. Closed contours use the outward side by default.");
            }
            let mut draft = options.draft.is_some();
            if ui.checkbox(&mut draft, "Draft While Extruding").changed() {
                options.draft = draft.then(|| cad_core::ExtrudeDraft {
                    angle: "3 deg".into(),
                    outward: false,
                });
                changed = true;
            }
            if let Some(draft) = &mut options.draft {
                changed |= expression(ui, "Draft Angle", &mut draft.angle);
                changed |= ui.checkbox(&mut draft.outward, "Draft Outward").changed();
                ui.small("Straight-edge profiles; the sketch plane stays neutral. Curved tapering is unavailable.");
            }
            changed |= result_fields(ui, &doc, &feature.id, body, mode);
            egui::CollapsingHeader::new("Feature Scope").show(ui,|ui| {
                ui.small("The target and these additional feature outputs are edited together. Each selected output may contain several bodies.");
                for part in doc.parts.values() {
                    for candidate in part.features.iter().take_while(|f|f.id!=feature.id).filter(|f|!f.suppressed&&f.operation.produces_solid()) {
                        let reference=part.reference(&candidate.id);
                        if body.as_ref()==Some(&reference) {continue;}
                        let mut included=options.scope.contains(&reference);
                        if ui.checkbox(&mut included,format!("{} / {}",part.name,candidate.name)).changed() {
                            if included {options.scope.push(reference);} else {options.scope.retain(|r|r!=&reference);}
                            changed=true;
                        }
                    }
                }
            });
        }
        Operation::Revolve {
            profile,
            origin,
            axis,
            axis_reference,
            angle,
            body,
            mode,
            options,
        } => {
            changed |= profile_selector(ui, &doc, &feature.id, "Profile", profile);
            changed |= crate::contours_ui::fields(
                ui,
                &doc,
                std::slice::from_ref(profile),
                &mut options.contours,
                options.thin.is_some(),
            );
            changed |= result_fields(ui, &doc, &feature.id, body, mode);
            changed |= expression(ui, "Angle", angle);
            changed |= ui
                .checkbox(&mut options.reverse, "Reverse Direction")
                .changed();
            if ui.checkbox(&mut options.midplane, "Mid Plane").changed() {
                if options.midplane {
                    options.second_angle = None;
                }
                changed = true;
            }
            let mut second = options.second_angle.is_some();
            if ui
                .add_enabled(
                    !options.midplane,
                    egui::Checkbox::new(&mut second, "Direction 2"),
                )
                .changed()
            {
                options.second_angle = second.then(|| "90 deg".into());
                changed = true;
            }
            if let Some(angle) = &mut options.second_angle {
                changed |= expression(ui, "Direction 2 Angle", angle);
            }
            egui::CollapsingHeader::new("Start Angle").show(ui, |ui| {
                let mut enabled = options.start_angle.is_some();
                if ui.checkbox(&mut enabled, "Angular Offset").changed() {
                    options.start_angle = enabled.then(|| "0 deg".into());
                    changed = true;
                }
                if let Some(angle) = &mut options.start_angle {
                    changed |= expression(ui, "Offset", angle);
                }
            });
            changed |= revolution_thin_fields(ui, &mut options.thin);
            ui.label("Axis of revolution");
            changed |= crate::reference_ui::axis_fields(
                ui,
                w,
                &doc,
                &feature.id,
                axis,
                Some(origin),
                axis_reference,
            );
            egui::CollapsingHeader::new("Feature Scope").show(ui, |ui| {
                for part in doc.parts.values() {
                    for candidate in part
                        .features
                        .iter()
                        .take_while(|candidate| candidate.id != feature.id)
                        .filter(|candidate| {
                            !candidate.suppressed && candidate.operation.produces_solid()
                        })
                    {
                        let reference = part.reference(&candidate.id);
                        if body.as_ref() == Some(&reference) {
                            continue;
                        }
                        let mut included = options.scope.contains(&reference);
                        if ui
                            .checkbox(&mut included, format!("{} / {}", part.name, candidate.name))
                            .changed()
                        {
                            if included {
                                options.scope.push(reference);
                            } else {
                                options.scope.retain(|candidate| candidate != &reference);
                            }
                            changed = true;
                        }
                    }
                }
            });
            ui.small("Selected material regions are revolved. The section and its thin offsets must remain on one side of the axis.");
        }
        Operation::Fillet { edges, radius, .. } => {
            ui.label(format!("Edges to fillet: {}", edges.len()));
            if let Some(selected) = w
                .selected
                .as_ref()
                .filter(|s| s.reference.topology == "edge")
                && ui.button("Use selected edge").clicked()
            {
                *edges = vec![selected.reference.clone()];
                changed = true;
            }
            changed |= selected_edges(ui, edges);
            changed |= expression(ui, "Radius", radius);
            if edges.is_empty() {
                ui.small("Select an edge on the model, then Use selected edge.");
            }
        }
        Operation::Chamfer {
            edges,
            distance,
            options,
            ..
        } => {
            ui.label(format!("Edges to chamfer: {}", edges.len()));
            if let Some(selected) = w
                .selected
                .as_ref()
                .filter(|s| s.reference.topology == "edge")
                && ui.button("Use selected edge").clicked()
            {
                *edges = vec![selected.reference.clone()];
                changed = true;
            }
            changed |= selected_edges(ui, edges);
            use cad_core::edge_features::ChamferMethod;
            egui::ComboBox::from_id_salt("chamfer-method")
                .selected_text(match options.method {
                    ChamferMethod::EqualDistance => "Equal Distance",
                    ChamferMethod::TwoDistances => "Two Distances",
                    ChamferMethod::DistanceAngle => "Distance and Angle",
                })
                .show_ui(ui, |ui| {
                    changed |= ui
                        .selectable_value(
                            &mut options.method,
                            ChamferMethod::EqualDistance,
                            "Equal Distance",
                        )
                        .changed();
                    changed |= ui
                        .selectable_value(
                            &mut options.method,
                            ChamferMethod::TwoDistances,
                            "Two Distances",
                        )
                        .changed();
                    changed |= ui
                        .selectable_value(
                            &mut options.method,
                            ChamferMethod::DistanceAngle,
                            "Distance and Angle",
                        )
                        .changed();
                });
            changed |= expression(ui, "Distance 1", distance);
            match options.method {
                ChamferMethod::EqualDistance => {}
                ChamferMethod::TwoDistances => {
                    changed |= expression(ui, "Distance 2", &mut options.second_distance);
                }
                ChamferMethod::DistanceAngle => {
                    changed |= expression(ui, "Angle", &mut options.angle);
                }
            }
            if options.method != ChamferMethod::EqualDistance {
                changed |= ui
                    .checkbox(&mut options.flip, "Flip Distance 1 Side")
                    .changed();
                if let Some(selected) = w
                    .selected
                    .as_ref()
                    .filter(|selection| selection.reference.topology == "face")
                    && ui.button("Use selected face for Distance 1").clicked()
                {
                    options.first_face = Some(selected.reference.clone());
                    changed = true;
                }
                if let Some(reference) = &options.first_face {
                    ui.small(format!("Distance 1 face: {}", reference.role));
                    if ui.button("Clear Distance 1 Face").clicked() {
                        options.first_face = None;
                        changed = true;
                    }
                } else {
                    ui.small("Distance 1 uses the first adjacent face in normal order; pick a face to keep its side associative.");
                }
            }
        }
        Operation::Shell {
            thickness,
            removed_faces,
            options,
            ..
        } => {
            changed |= expression(ui, "Wall Thickness", thickness);
            changed |= ui.checkbox(&mut options.outward, "Shell Outward").changed();
            ui.label("Faces to remove");
            changed |=
                face_selection_fields(ui, removed_faces, w.selected.as_ref().map(|s| &s.reference));
            if removed_faces.is_empty() {
                ui.small("No opening faces: create an enclosed cavity.");
            }
            ui.separator();
            ui.strong("Face Thicknesses");
            if let Some(selection) = w
                .selected
                .as_ref()
                .filter(|selection| selection.reference.topology == "face")
                && ui.button("Use Selected Face Thickness").clicked()
                && !options.face_thicknesses.iter().any(|wall| {
                    wall.face.producer == selection.reference.producer
                        && wall.face.role == selection.reference.role
                })
            {
                options
                    .face_thicknesses
                    .push(cad_core::wall::FaceThickness {
                        face: selection.reference.clone(),
                        thickness: thickness.clone(),
                    });
                changed = true;
            }
            let mut remove = None;
            for (index, wall) in options.face_thicknesses.iter_mut().enumerate() {
                ui.horizontal(|ui| {
                    changed |= expression(
                        ui,
                        &format!("Face {} Thickness", index + 1),
                        &mut wall.thickness,
                    );
                    if ui.small_button("Remove").clicked() {
                        remove = Some(index);
                    }
                });
            }
            if let Some(index) = remove {
                options.face_thicknesses.remove(index);
                changed = true;
            }
        }
        Operation::Pattern {
            body,
            count,
            step,
            angle,
            axis,
            axis_reference,
            options,
            ..
        } => {
            let mut source = Some(body.clone());
            changed |= body_selector(ui, &doc, &feature.id, "Bodies to Pattern", &mut source);
            if let Some(source) = source {
                *body = source;
            }
            ui.label("Instances");
            changed |= ui
                .add(egui::DragValue::new(count).range(1..=10000))
                .changed();
            for (label, value) in ["X Spacing", "Y Spacing", "Z Spacing"]
                .into_iter()
                .zip(step)
            {
                changed |= expression(ui, label, value);
            }
            changed |= ui
                .checkbox(
                    &mut options.angle_is_total,
                    "Equal Spacing over Total Angle",
                )
                .changed();
            changed |= expression(
                ui,
                if options.angle_is_total {
                    "Total Angle"
                } else {
                    "Rotation per Instance"
                },
                angle,
            );
            changed |= ui
                .checkbox(&mut options.rotate_instances, "Rotate Instances")
                .changed();
            if !options.rotate_instances {
                changed |= crate::reference_ui::point(
                    ui,
                    w,
                    &doc,
                    &feature.id,
                    "Instance Reference Point",
                    &mut options.seed,
                );
            }
            ui.label("Rotation Axis");
            changed |= crate::reference_ui::axis_fields(
                ui,
                w,
                &doc,
                &feature.id,
                axis,
                None,
                axis_reference,
            );
            let mut second = options.second_direction.is_some();
            if ui.checkbox(&mut second, "Second Direction").changed() {
                options.second_direction = second.then(Default::default);
                changed = true;
            }
            if let Some(second) = &mut options.second_direction {
                changed |= ui
                    .add(
                        egui::DragValue::new(&mut second.count)
                            .range(1..=10000)
                            .prefix("Direction 2 instances "),
                    )
                    .changed();
                for (label, value) in ["Direction 2 · X", "Direction 2 · Y", "Direction 2 · Z"]
                    .into_iter()
                    .zip(&mut second.step)
                {
                    changed |= expression(ui, label, value);
                }
            }
            egui::CollapsingHeader::new("Instances to Skip").show(ui, |ui| {
                let secondary = options
                    .second_direction
                    .as_ref()
                    .map(|d| d.count)
                    .unwrap_or(1);
                let mut remove = None;
                for (index, pair) in options.skipped.iter_mut().enumerate() {
                    ui.push_id(index, |ui| {
                        ui.horizontal(|ui| {
                            for (coordinate, maximum) in pair.iter_mut().zip([*count, secondary]) {
                                let mut value = coordinate.saturating_add(1);
                                if ui
                                    .add(
                                        egui::DragValue::new(&mut value)
                                            .range(1..=maximum.max(1))
                                            .prefix("Instance "),
                                    )
                                    .changed()
                                {
                                    *coordinate = value.saturating_sub(1);
                                    changed = true;
                                }
                            }
                            if ui.small_button("Remove").clicked() {
                                remove = Some(index);
                            }
                        });
                    });
                }
                if let Some(index) = remove {
                    options.skipped.remove(index);
                    changed = true;
                }
                if ui.button("Skip an Instance").clicked()
                    && let Some(pair) = (0..secondary)
                        .flat_map(|j| (0..*count).map(move |i| [i, j]))
                        .find(|pair| *pair != [0, 0] && !options.skipped.contains(pair))
                {
                    options.skipped.push(pair);
                    changed = true;
                }
            });
        }
        Operation::Transform { placement, .. } => {
            ui.strong("Move");
            for (label, value) in ["X", "Y", "Z"].into_iter().zip(&mut placement.translation) {
                changed |= ui
                    .add(
                        egui::DragValue::new(value)
                            .prefix(format!("{label} "))
                            .suffix(" mm"),
                    )
                    .changed();
            }
            ui.strong("Rotate");
            for (label, value) in ["X", "Y", "Z"].into_iter().zip(&mut placement.rotation_deg) {
                changed |= ui
                    .add(
                        egui::DragValue::new(value)
                            .prefix(format!("{label} "))
                            .suffix(" deg"),
                    )
                    .changed();
            }
        }
        Operation::Scale { body, options } => {
            let mut source = Some(body.clone());
            changed |= body_selector(ui, &doc, &feature.id, "Body to Scale", &mut source);
            if let Some(source) = source {
                *body = source;
            }
            let was_uniform = options.uniform;
            changed |= ui
                .checkbox(&mut options.uniform, "Uniform Scaling")
                .changed();
            if was_uniform && !options.uniform {
                options.factors[1] = options.factors[0].clone();
                options.factors[2] = options.factors[0].clone();
            }
            if options.uniform {
                changed |= expression(ui, "Scale Factor", &mut options.factors[0]);
            } else {
                for (label, factor) in ["X Factor", "Y Factor", "Z Factor"]
                    .into_iter()
                    .zip(&mut options.factors)
                {
                    changed |= expression(ui, label, factor);
                }
            }
            changed |= crate::reference_ui::point(
                ui,
                w,
                &doc,
                &feature.id,
                "Scale Center",
                &mut options.center,
            );
            changed |= ui
                .checkbox(&mut options.keep_original, "Copy Body")
                .changed();
            ui.small("Nonuniform factors follow the document X, Y and Z axes.");
        }
        Operation::Draft {
            angle,
            faces,
            plane,
            plane_reference,
            ..
        } => {
            changed |= expression(ui, "Draft Angle", angle);
            ui.label("Faces to draft");
            changed |= face_selection_fields(ui, faces, w.selected.as_ref().map(|s| &s.reference));
            ui.separator();
            ui.label("Neutral plane");
            changed |=
                crate::reference_ui::plane_fields(ui, w, &doc, &feature.id, plane, plane_reference);
        }
        Operation::Boolean { left, right, mode } => {
            egui::ComboBox::from_label("Combine")
                .selected_text(mode.as_str())
                .show_ui(ui, |ui| {
                    for value in ["union", "difference", "intersection"] {
                        changed |= ui.selectable_value(mode, value.into(), value).changed();
                    }
                });
            for (label, reference) in [("Main Body", left), ("Tool Body", right)] {
                egui::ComboBox::from_label(label)
                    .selected_text(name(reference))
                    .show_ui(ui, |ui| {
                        for part in doc.parts.values() {
                            for node in &part.outputs {
                                if node != &feature.id {
                                    let candidate = part.reference(node);
                                    changed |= ui
                                        .selectable_value(
                                            reference,
                                            candidate.clone(),
                                            format!("{} / {}", part.name, name(&candidate)),
                                        )
                                        .changed();
                                }
                            }
                        }
                    });
            }
        }
        Operation::Loft {
            profiles,
            body,
            mode,
            options,
        } => {
            ui.strong("Ordered Cross-sections");
            ui.small("Ordered sketch sections with explicit connectors and endpoint influences.");
            let mut remove = None;
            let mut move_up = None;
            for (index, reference) in profiles.iter_mut().enumerate() {
                ui.push_id(index, |ui| {
                    changed |= profile_selector(
                        ui,
                        &doc,
                        &feature.id,
                        &format!("Section {}", index + 1),
                        reference,
                    );
                    ui.horizontal(|ui| {
                        if ui.small_button("Remove").clicked() {
                            remove = Some(index);
                        }
                        if index > 0 && ui.small_button("Move Up").clicked() {
                            move_up = Some(index);
                        }
                    });
                });
            }
            if let Some(index) = remove {
                profiles.remove(index);
                changed = true;
            }
            if let Some(index) = move_up {
                profiles.swap(index, index - 1);
                changed = true;
            }
            if ui.button("Add Section").clicked()
                && let Some(reference) = doc
                    .parts
                    .values()
                    .flat_map(|p| {
                        p.features
                            .iter()
                            .filter(|f| {
                                f.id != feature.id
                                    && matches!(f.operation, Operation::Profile { .. })
                            })
                            .map(|f| p.reference(&f.id))
                    })
                    .find(|r| !profiles.contains(r))
            {
                profiles.push(reference);
                changed = true;
            }
            changed |= result_fields(ui, &doc, &feature.id, body, mode);
            changed |= crate::sections_ui::loft(
                ui,
                w,
                &doc,
                &feature.id,
                profiles,
                body.as_ref(),
                options,
            );
        }
        Operation::Sweep {
            profile,
            path,
            path_sketch,
            body,
            mode,
            options,
        } => {
            changed |= profile_selector(ui, &doc, &feature.id, "Section", profile);
            let mut use_sketch = path_sketch.is_some();
            if ui
                .checkbox(
                    &mut use_sketch,
                    "Use a sketch or reference curve as the path",
                )
                .changed()
            {
                *path_sketch = if use_sketch {
                    Some(profile.clone())
                } else {
                    options.path_member = None;
                    options.path_start_vertex = None;
                    None
                };
                changed = true;
            }
            if let Some(reference) = path_sketch {
                changed |= sweep_path_selector(ui, &doc, &feature.id, reference);
                if matches!(
                    doc.parts
                        .get(&reference.definition)
                        .and_then(|part| part
                            .features
                            .iter()
                            .find(|feature| feature.id == reference.node))
                        .map(|feature| &feature.operation),
                    Some(Operation::ReferenceCurve { .. })
                ) {
                    if options.path_member.take().is_some()
                        || options.path_start_vertex.take().is_some()
                    {
                        changed = true;
                    }
                    ui.small("Reference curves use their saved direction. The kernel reports unsupported path carriers before Apply.");
                } else {
                    ui.small(
                    "Use one connected open chain or select a native member. A circular member can be a closed tangent-transport path.",
                );
                }
            } else {
                ui.strong("Path Stations");
                let mut remove = None;
                for (index, point) in path.iter_mut().enumerate() {
                    ui.push_id(index, |ui| {
                        ui.label(format!("Station {}", index + 1));
                        for (label, value) in ["X", "Y", "Z"].into_iter().zip(point) {
                            changed |= ui
                                .add(
                                    egui::DragValue::new(value)
                                        .speed(0.5)
                                        .prefix(format!("{label} "))
                                        .suffix(" mm"),
                                )
                                .changed();
                        }
                        if ui.small_button("Remove").clicked() {
                            remove = Some(index);
                        }
                    });
                }
                if let Some(index) = remove {
                    path.remove(index);
                    changed = true;
                }
                if ui.button("Add Station").clicked() {
                    let mut next = *path.last().unwrap_or(&[0.; 3]);
                    next[2] += 10.;
                    path.push(next);
                    changed = true;
                }
            }
            changed |= result_fields(ui, &doc, &feature.id, body, mode);
            changed |= crate::sections_ui::sweep(
                ui,
                &doc,
                &feature.id,
                profile,
                path_sketch.as_ref(),
                body.as_ref(),
                options,
            );
        }
        Operation::Mirror {
            plane,
            plane_reference,
            keep_original,
            ..
        } => {
            changed |= ui.checkbox(keep_original, "Keep original body").changed();
            changed |=
                crate::reference_ui::plane_fields(ui, w, &doc, &feature.id, plane, plane_reference);
        }
        Operation::StructuralMember {
            start, end, roll, ..
        } => {
            for (label, point) in [("Start", start), ("End", end)] {
                ui.label(label);
                for value in point {
                    changed |= ui.add(egui::DragValue::new(value).suffix(" mm")).changed();
                }
            }
            changed |= expression(ui, "Section Rotation", roll);
        }
        _ => {
            ui.small("Additional settings are available in Advanced Source.");
        }
    }
    if let Some(part) = part
        && !part.parameters.is_empty()
    {
        ui.separator();
        ui.small("You can use named parameters in dimension fields.");
    }
    changed
}
pub fn measure(w: &mut Workspace) {
    if let Some(selected) = &w.selected {
        if matches!(
            selected.reference.topology.as_str(),
            "plane" | "axis" | "point"
        ) {
            w.status = format!(
                "{} · Origin / Position: {:.4}, {:.4}, {:.4} mm (assembly coordinates)",
                selected.name, selected.point[0], selected.point[1], selected.point[2]
            );
            return;
        }
        w.status = selected
            .reference
            .signature
            .as_ref()
            .map(|sig| {
                format!(
                    "{}: {:.4} {} · Center: {:.3}, {:.3}, {:.3} mm",
                    selected.reference.topology,
                    sig.measure,
                    if selected.reference.topology == "edge" {
                        "mm"
                    } else {
                        "mm²"
                    },
                    sig.center[0],
                    sig.center[1],
                    sig.center[2]
                )
            })
            .unwrap_or_else(|| "Measurement unavailable".into());
    }
}
pub fn view_controls(ui: &mut egui::Ui, w: &mut Workspace) {
    crate::exploded_views_ui::view_controls(ui, w);
    for (label, orbit) in [
        ("Front", [0., 0.]),
        ("Back", [std::f32::consts::PI, 0.]),
        ("Top", [0., std::f32::consts::FRAC_PI_2]),
        ("Bottom", [0., -std::f32::consts::FRAC_PI_2]),
        ("Right", [std::f32::consts::FRAC_PI_2, 0.]),
        ("Left", [-std::f32::consts::FRAC_PI_2, 0.]),
        ("Isometric", [0.65, 0.4]),
    ] {
        if ui.button(label).clicked() {
            w.view.orbit = orbit;
            w.view.roll = 0.;
            crate::navigation::stop(w);
            ui.close();
        }
    }
    ui.separator();
    if ui.button("Zoom to Fit  F").clicked() {
        viewport::fit(w, false);
        ui.close();
    }
    if ui.button("Normal To Sketch").clicked() {
        sketch::normal_to(w);
        ui.close();
    }
}
pub fn status(ui: &mut egui::Ui, w: &mut Workspace) {
    ui.horizontal(|ui| {
        if w.session.draft.as_ref().is_some_and(|d| d.pending) {
            ui.spinner();
        }
        ui.label(&w.status);
        if w.session.draft.is_some() && w.tool_checkpoint.is_none() {
            if ui.small_button("Confirm Changes").clicked() {
                w.confirm_tool();
            }
            if ui.small_button("Cancel").clicked() {
                w.cancel();
            }
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label("mm · deg");
            ui.separator();
            ui.label(if w.sketch.is_some() {
                "Editing Sketch"
            } else if w.view.history_cursor.is_some() {
                "History inspection"
            } else {
                "Model"
            });
        });
    });
}
