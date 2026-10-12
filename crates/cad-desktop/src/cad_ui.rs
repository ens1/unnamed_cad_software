//! Familiar CAD workspace widgets over the same document commands and user layout.
use crate::{Workspace, presentation_controls as controls, sketch, ui, viewport};
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

fn icon(p: &egui::Painter, center: egui::Pos2, kind: Icon, size: f32, color: Color32) {
    let scale = size / 32.;
    let at = |x, y| center + egui::vec2(x, y) * scale;
    let stroke = Stroke::new(1.6 * scale, color);
    let line = |a, b| {
        p.line_segment([a, b], stroke);
    };
    match kind {
        Icon::Rectangle => {
            p.rect_stroke(
                egui::Rect::from_center_size(center, egui::vec2(25., 18.) * scale),
                0.,
                stroke,
                egui::StrokeKind::Inside,
            );
        }
        Icon::Circle => {
            p.circle_stroke(center, 11. * scale, stroke);
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
                        egui::Rect::from_center_size(at(x, y), egui::vec2(9., 9.) * scale),
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
                p.circle_stroke(at(1., -5.), 4. * scale, Stroke::new(1.8 * scale, color));
            }
            if matches!(kind, Icon::Fillet) {
                p.add(egui::Shape::line(
                    vec![at(-12., 1.), at(-11., -2.), at(-8., -5.), at(-5., -5.)],
                    Stroke::new(3. * scale, color),
                ));
            }
            if matches!(kind, Icon::Chamfer) {
                p.line_segment([at(-12., 1.), at(-6., -5.)], Stroke::new(3. * scale, color));
            }
        }
    }
}
fn tool_button(
    ui: &mut egui::Ui,
    id: &str,
    label: &str,
    kind: Icon,
    enabled: bool,
    active: bool,
    hint: &str,
) -> bool {
    let draw = |p: &egui::Painter, center, size, color| icon(p, center, kind, size, color);
    controls::icon_button(
        ui,
        id,
        "command-action",
        &["toolbar-control"],
        label,
        enabled,
        active,
        Some(&draw),
    )
    .on_hover_text(hint)
    .on_disabled_hover_text(hint)
    .clicked()
}

fn command_button(ui: &mut egui::Ui, command: &str, label: &str, enabled: bool) -> egui::Response {
    controls::button(
        ui,
        &format!("command-toolbar/{command}"),
        "command-action",
        &["toolbar-control"],
        label,
        enabled,
        false,
    )
}
fn menu_action(ui: &mut egui::Ui, command: &str, label: &str, enabled: bool) -> egui::Response {
    controls::button(
        ui,
        &format!("command-menu/{command}"),
        "menu-action",
        &["menu-item"],
        label,
        enabled,
        false,
    )
}
fn menu_button(ui: &mut egui::Ui, label: &str, content: impl FnOnce(&mut egui::Ui)) {
    controls::menu_button(
        ui,
        &format!("menu/{}", label.to_lowercase().replace(' ', "-")),
        label,
        content,
    );
}

fn feature_groups() -> Vec<&'static str> {
    let mut groups = vec![];
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
    ]
    .into_iter()
    .chain(
        cad_core::modeling_tools::TOOLS
            .iter()
            .map(|tool| tool.group),
    ) {
        if !groups.contains(&group)
            && cad_core::modeling_tools::TOOLS
                .iter()
                .any(|tool| tool.group == group)
        {
            groups.push(group);
        }
    }
    groups
}

fn menu_items(name: &str, w: &Workspace) -> Vec<String> {
    let commands: Vec<&str> = match name {
        "File" => vec![
            "file.new-part",
            "file.new",
            "file.open",
            "file.save",
            "file.save-as",
            "file.import",
            "file.export",
            "file.recover",
            "file.example",
            "file.export-drawing-dxf",
            "file.export-flat-pattern",
            "file.export-cutlist",
            "file.export-bom",
        ],
        "Edit" => vec![
            "document.undo",
            "document.redo",
            "parameter.new",
            "settings",
        ],
        "Insert" => {
            return feature_groups()
                .into_iter()
                .map(|group| format!("menu/{}", group.to_lowercase().replace(' ', "-")))
                .collect();
        }
        "View" => vec![
            "viewport.orientation.front",
            "viewport.orientation.back",
            "viewport.orientation.top",
            "viewport.orientation.bottom",
            "viewport.orientation.right",
            "viewport.orientation.left",
            "viewport.orientation.isometric",
            "viewport.fit",
            "viewport.normal-sketch",
            "lighting",
        ],
        "Tools" => vec![
            "settings",
            "reference.repair",
            "assembly.edit",
            "assembly.move",
            "assembly.rotate",
            "assembly.pattern",
            "assembly.group-new",
            "assembly.copy-with-mates",
            "assembly.exploded-new",
            "assembly.inspection-new",
            "configurations",
            "equations",
            "materials",
            "appearances",
            "manufacturing",
            "drawings",
            "libraries",
            "bom",
            "controls",
        ],
        "Workspace" => vec![
            "presentation",
            "versions",
            "comments",
            "document.rebuild",
            "source",
            "file.release",
        ],
        "Size" => vec![
            "presentation.compact",
            "presentation.comfortable",
            "presentation",
        ],
        "Shapes" => {
            return [
                "rectangle",
                "center-rectangle",
                "three-point-rectangle",
                "three-point-center-rectangle",
                "parallelogram",
                "polygon",
                "slot",
                "center-slot",
                "arc-slot",
            ]
            .into_iter()
            .map(|tool| format!("command-toolbar/sketch.tool.{tool}"))
            .collect();
        }
        "Curves" => {
            return [
                "circle",
                "perimeter-circle",
                "arc",
                "center-arc",
                "tangent-arc",
                "ellipse",
                "ellipse-arc",
                "conic",
                "parabola",
                "spline",
                "interpolating-spline",
                "nurbs",
            ]
            .into_iter()
            .map(|tool| format!("command-toolbar/sketch.tool.{tool}"))
            .collect();
        }
        "Reference" if w.command_tab == "Sketch" => {
            return ["point", "centerline", "midpoint-line"]
                .into_iter()
                .map(|tool| format!("command-toolbar/sketch.tool.{tool}"))
                .collect();
        }
        _ => {
            let category = if name == "Edit Entities" {
                "Edit"
            } else if name == "Patterns" {
                "Pattern"
            } else if name == "Blocks" {
                "Block"
            } else {
                name
            };
            let sketch = cad_core::modeling_tools::SKETCH_TOOLS
                .iter()
                .filter(|tool| tool.2 == category)
                .map(|tool| tool.0)
                .collect::<Vec<_>>();
            if w.command_tab == "Sketch" && !sketch.is_empty() {
                sketch
            } else {
                cad_core::modeling_tools::TOOLS
                    .iter()
                    .filter(|tool| tool.group == name)
                    .map(|tool| tool.command)
                    .collect()
            }
        }
    };
    commands
        .into_iter()
        .map(|command| format!("command-menu/{command}"))
        .collect()
}

/// A menu is also an ordinary semantic component, so user layouts can move or
/// duplicate individual headings without embedding a second command route.
pub fn semantic_menu(
    ui: &mut egui::Ui,
    w: &mut Workspace,
    element: &cad_core::presentation::SemanticElement,
) {
    let snapshot = semantics(w);
    controls::menu_button(ui, &element.id, &element.label, |ui| {
        for item in element
            .data
            .get("items")
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(serde_json::Value::as_str)
        {
            let Some(descriptor) = snapshot.iter().find(|descriptor| descriptor.id == item) else {
                ui.weak(format!("Unavailable menu item: {item}"));
                continue;
            };
            if descriptor.role == "menu" {
                semantic_menu(ui, w, descriptor);
                continue;
            }
            let response = controls::button(
                ui,
                &descriptor.id,
                "menu-action",
                &["menu-item"],
                &descriptor.label,
                descriptor.state != "disabled",
                descriptor.state == "selected",
            )
            .on_disabled_hover_text(descriptor.value.as_deref().unwrap_or(&descriptor.label));
            if response.clicked()
                && let Some(command) = descriptor.commands.first()
            {
                ui::action(w, command);
                ui.close();
            }
        }
    });
}

const SKETCH_DRAWING_TOOLS: &[(&str, &str, sketch::Tool)] = &[
    ("line", "Line", sketch::Tool::Polyline),
    ("rectangle", "Corner Rectangle", sketch::Tool::Rectangle),
    ("circle", "Circle", sketch::Tool::Circle),
    ("select", "Select", sketch::Tool::Select),
    (
        "center-rectangle",
        "Center Rectangle",
        sketch::Tool::CenterRectangle,
    ),
    (
        "three-point-rectangle",
        "3-point Corner Rectangle",
        sketch::Tool::ThreePointRectangle,
    ),
    (
        "three-point-center-rectangle",
        "3-point Center Rectangle",
        sketch::Tool::ThreePointCenterRectangle,
    ),
    (
        "parallelogram",
        "Parallelogram",
        sketch::Tool::Parallelogram,
    ),
    ("polygon", "Regular Polygon", sketch::Tool::Polygon),
    ("slot", "Straight Slot", sketch::Tool::Slot),
    (
        "center-slot",
        "Centerpoint Straight Slot",
        sketch::Tool::CenterSlot,
    ),
    ("arc-slot", "Arc Slot", sketch::Tool::ArcSlot),
    (
        "perimeter-circle",
        "Perimeter Circle",
        sketch::Tool::PerimeterCircle,
    ),
    ("arc", "3-point Arc", sketch::Tool::Arc),
    ("center-arc", "Centerpoint Arc", sketch::Tool::CenterArc),
    ("tangent-arc", "Tangent Arc", sketch::Tool::TangentArc),
    ("ellipse", "Ellipse", sketch::Tool::Ellipse),
    ("ellipse-arc", "Partial Ellipse", sketch::Tool::EllipseArc),
    ("conic", "Conic", sketch::Tool::Conic),
    ("parabola", "Parabola", sketch::Tool::Parabola),
    ("spline", "Control-point Spline", sketch::Tool::Spline),
    (
        "interpolating-spline",
        "Interpolating Spline",
        sketch::Tool::InterpolatingSpline,
    ),
    ("nurbs", "Control-point NURBS", sketch::Tool::Nurbs),
    ("point", "Point", sketch::Tool::Point),
    ("centerline", "Centerline", sketch::Tool::Centerline),
    ("midpoint-line", "Midpoint Line", sketch::Tool::MidpointLine),
];

fn sketch_tool_command(tool: sketch::Tool) -> String {
    SKETCH_DRAWING_TOOLS
        .iter()
        .find(|entry| entry.2 == tool)
        .map(|entry| format!("sketch.tool.{}", entry.0))
        .unwrap_or_else(|| "sketch.tool.unspecified".into())
}

/// The same presentation command route is used by native controls and arbitrary
/// user-composed copies of them. Modeling continues through existing commands.
pub fn presentation_action(w: &mut Workspace, command: &str) -> bool {
    if crate::contours_ui::action(w, command) {
        return true;
    }
    if let Some(name) = command.strip_prefix("viewport.orientation.") {
        let orbit = match name {
            "front" => [0., 0.],
            "back" => [std::f32::consts::PI, 0.],
            "top" => [0., std::f32::consts::FRAC_PI_2],
            "bottom" => [0., -std::f32::consts::FRAC_PI_2],
            "right" => [std::f32::consts::FRAC_PI_2, 0.],
            "left" => [-std::f32::consts::FRAC_PI_2, 0.],
            "isometric" => [0.65, 0.4],
            _ => return false,
        };
        w.view.orbit = orbit;
        w.view.roll = 0.;
        crate::navigation::stop(w);
        return true;
    }
    if let Some(name) = command.strip_prefix("sketch.tool.")
        && let Some((_, _, tool)) = SKETCH_DRAWING_TOOLS.iter().find(|entry| entry.0 == name)
    {
        if w.tool_checkpoint.is_none() || w.sketch.is_some() {
            if w.sketch.is_none() {
                ui::action(w, "sketch.new");
            }
            sketch::set_tool(w, *tool);
        }
        return true;
    }
    if let Some(name) = command.strip_prefix("presentation.command-tab.")
        && let Some(tab) = [
            "Features", "Sketch", "Assembly", "Drawing", "Evaluate", "Graph",
        ]
        .iter()
        .find(|tab| tab.eq_ignore_ascii_case(name))
    {
        if *tab == "Drawing" {
            crate::drawing_ui::open_workspace(w);
            return true;
        }
        if crate::drawing_ui::is_workspace(w) && !crate::drawing_ui::leave_workspace(w) {
            return true;
        }
        w.command_tab = (*tab).into();
        if *tab == "Graph" {
            w.view.active_layout = "graph-workspace".into();
            w.view.mode = "Flow".into();
        } else if w.view.active_layout == "graph-workspace" {
            w.view.active_layout = "instrument-rail".into();
            w.view.mode = "Model".into();
        }
        return true;
    }
    if let Some(name) = command.strip_prefix("presentation.navigation.")
        && let Some(mode) = ["Model", "History", "Flow"]
            .iter()
            .find(|mode| mode.eq_ignore_ascii_case(name))
    {
        w.manager_tree = true;
        w.view.mode = (*mode).into();
        return true;
    }
    if let Some(name) = command.strip_prefix("presentation.graph-scope.")
        && let Some(scope) = ["Features", "Assembly"]
            .iter()
            .find(|scope| scope.eq_ignore_ascii_case(name))
    {
        w.view.graph_scope = (*scope).into();
        return true;
    }
    match command {
        "presentation.selection-dismiss" => {
            w.selection_popup = None;
        }
        "document.undo" => {
            if w.session.can_undo() {
                w.session.undo();
                w.requested = None;
            }
        }
        "document.redo" => {
            if w.session.can_redo() {
                w.session.redo();
                w.requested = None;
            }
        }
        "presentation.compact" | "presentation.comfortable" => {
            let compact = command == "presentation.compact";
            w.view.ui_scale = if compact { 1. } else { 1.15 };
            w.view.toolbar_height = if compact { 68. } else { 80. };
            resize_toolbar(w);
            crate::settings::save_live_application(w);
        }
        "graph.arrange" => {
            w.view.graph_positions.clear();
            w.view.graph_pan = [0.; 2];
            w.view.graph_zoom = 1.;
        }
        "assembly.isolate" => w.isolate(),
        "presentation.ghost-others" => {
            w.view.ghost_others = !w.view.ghost_others;
            w.rendered_key.clear();
        }
        "viewport.normal-sketch" => sketch::normal_to(w),
        "selection.measure" => measure(w),
        "file.export-drawing-dxf" => ui::export_format(w, "drawing-dxf", "drawing.dxf"),
        _ => return false,
    }
    true
}

pub fn semantics(w: &Workspace) -> Vec<cad_core::presentation::SemanticElement> {
    let mut out = vec![];
    let sketching = w.sketch.is_some();
    let blocked = w.tool_checkpoint.is_some() && !sketching;
    let has_body = w
        .view
        .active_part
        .as_ref()
        .is_some_and(|id| cad_core::configuration::has_solid_output(w.session.visible(), id));
    let mut publish = |id: String,
                       role: &str,
                       label: &str,
                       command: String,
                       enabled: bool,
                       selected: bool,
                       parent: &str| {
        out.push(cad_core::presentation::SemanticElement { id, role:role.into(), label:label.into(), value: (!enabled).then(||"Finish the active operation or create the required geometry first".into()), state:if !enabled {"disabled"} else if selected {"selected"} else {"ready"}.into(), commands:if command.is_empty() {vec![]} else {vec![command]}, relationships:vec![parent.into()], data:serde_json::json!({"presentation_control":true,"accessibility_label":label,"parent":parent,"classes":match role { "menu-action"=>vec!["menu-item"], "command-action"=>vec!["toolbar-control"], "menu"=>vec!["menu-heading"], "command-tab"|"navigation-action"=>vec!["selectable"], _=>vec![] }}) });
    };
    for (command, label) in cad_core::presentation::command_catalog() {
        let enabled = match command {
            "draft.apply" => confirmation_available(w),
            "draft.cancel" => w.tool_checkpoint.is_some() || w.session.draft.is_some() || sketching,
            _ => cad_core::modeling_tools::TOOLS
                .iter()
                .find(|tool| tool.command == command)
                .is_none_or(|tool| !blocked && (!tool.requires_body || has_body)),
        };
        for (prefix, role, parent) in [
            ("command-toolbar", "command-action", "command-manager"),
            ("command-menu", "menu-action", "document-context"),
        ] {
            publish(
                format!("{prefix}/{command}"),
                role,
                label,
                command.into(),
                enabled,
                false,
                parent,
            );
        }
    }
    for (name, label, tool) in SKETCH_DRAWING_TOOLS {
        let command = format!("sketch.tool.{name}");
        publish(
            format!("command-toolbar/{command}"),
            "command-action",
            label,
            command,
            !blocked,
            w.sketch.as_ref().is_some_and(|s| s.tool == *tool),
            "command-manager",
        );
    }
    for tab in [
        "Features", "Sketch", "Assembly", "Drawing", "Evaluate", "Graph",
    ] {
        publish(
            format!("command-tab/{}", tab.to_lowercase()),
            "command-tab",
            tab,
            format!("presentation.command-tab.{}", tab.to_lowercase()),
            true,
            w.command_tab == tab,
            "command-manager",
        );
    }
    for (label, mode) in [
        ("Model", "Model"),
        ("History", "History"),
        ("Graph", "Flow"),
    ] {
        publish(
            format!("navigation/{}", mode.to_lowercase()),
            "navigation-action",
            label,
            format!("presentation.navigation.{}", mode.to_lowercase()),
            true,
            w.manager_tree && w.view.mode == mode,
            "view-navigation",
        );
    }
    for menu in [
        "File",
        "Edit",
        "Insert",
        "View",
        "Tools",
        "Workspace",
        "Size",
        "Create",
        "Remove",
        "Modify",
        "Pattern",
        "Surfaces",
        "Mold",
        "Reference",
        "Sheet Metal",
        "Weldments",
        "Shapes",
        "Curves",
        "Edit Entities",
        "Transform",
        "Patterns",
        "Blocks",
        "Dimension Groups",
    ] {
        publish(
            format!("menu/{}", menu.to_lowercase().replace(' ', "-")),
            "menu",
            menu,
            String::new(),
            true,
            false,
            if matches!(
                menu,
                "File" | "Edit" | "Insert" | "View" | "Tools" | "Workspace"
            ) {
                "document-context"
            } else {
                "command-manager"
            },
        );
    }
    for (command, label, enabled, selected) in [
        ("document.undo", "Undo", w.session.can_undo(), false),
        ("document.redo", "Redo", w.session.can_redo(), false),
        (
            "presentation.compact",
            "Compact",
            true,
            w.view.ui_scale == 1.,
        ),
        (
            "presentation.comfortable",
            "Comfortable",
            true,
            w.view.ui_scale == 1.15,
        ),
        ("graph.arrange", "Arrange Graph", true, false),
        ("assembly.isolate", "Edit in Place", true, w.view.isolation),
        (
            "presentation.ghost-others",
            "Ghost Others",
            true,
            w.view.ghost_others,
        ),
        (
            "viewport.normal-sketch",
            "Normal To Sketch",
            sketching,
            false,
        ),
        ("selection.measure", "Measure", w.selected.is_some(), false),
        (
            "file.export-drawing-dxf",
            "Export Drawing DXF…",
            true,
            false,
        ),
    ] {
        publish(
            format!("command:{command}"),
            "command",
            label,
            command.into(),
            enabled,
            selected,
            "workspace",
        );
        for (prefix, role, parent) in [
            ("command-toolbar", "command-action", "command-manager"),
            ("command-menu", "menu-action", "document-context"),
        ] {
            publish(
                format!("{prefix}/{command}"),
                role,
                label,
                command.into(),
                enabled,
                selected,
                parent,
            );
        }
    }
    for (id, role, label, command, enabled) in [
        (
            "command:draft.apply",
            "command",
            "Apply",
            "draft.apply",
            confirmation_available(w),
        ),
        (
            "draft.apply",
            "apply-action",
            "Apply",
            "draft.apply",
            confirmation_available(w),
        ),
        (
            "command:draft.cancel",
            "command",
            "Cancel",
            "draft.cancel",
            w.tool_checkpoint.is_some() || w.session.draft.is_some() || sketching,
        ),
        (
            "draft.cancel",
            "cancel-action",
            "Cancel",
            "draft.cancel",
            w.tool_checkpoint.is_some() || w.session.draft.is_some() || sketching,
        ),
    ] {
        publish(
            id.into(),
            role,
            label,
            command.into(),
            enabled,
            false,
            "property-manager",
        );
    }
    for orientation in [
        "Front",
        "Back",
        "Top",
        "Bottom",
        "Right",
        "Left",
        "Isometric",
    ] {
        let command = format!("viewport.orientation.{}", orientation.to_lowercase());
        publish(
            format!("command:{command}"),
            "command",
            orientation,
            command.clone(),
            true,
            false,
            "workspace",
        );
        publish(
            format!("command-menu/{command}"),
            "menu-action",
            orientation,
            command,
            true,
            false,
            "menu/view",
        );
    }
    for group in feature_groups() {
        let id = format!("menu/{}", group.to_lowercase().replace(' ', "-"));
        if !out.iter().any(|element| element.id == id) {
            out.push(cad_core::presentation::SemanticElement {
                id,
                role: "menu".into(),
                label: group.into(),
                value: None,
                state: "ready".into(),
                commands: vec![],
                relationships: vec!["command-manager".into()],
                data: serde_json::json!({"classes":["menu-heading"],"presentation_control":true}),
            });
        }
    }
    for element in &mut out {
        if element
            .commands
            .iter()
            .any(|command| command == "draft.apply")
        {
            element.label = confirmation_label(w);
            element.data["completion_scope"] = serde_json::json!(if w.sketch.is_some() {
                "sketch"
            } else if w.dialog.as_deref() == Some("component-insert") {
                "component"
            } else if crate::drawing_ui::is_workspace(w) {
                "drawing"
            } else {
                "feature"
            });
        }
        if element.role == "menu" {
            let items = menu_items(&element.label, w);
            element.data["items"] = serde_json::json!(items);
        }
        if sketching && w.command_tab == "Sketch" && element.id == "command-toolbar/sketch.new" {
            element.label = confirmation_label(w);
            element.commands = vec!["draft.apply".into()];
            element.state = if confirmation_available(w) {
                "selected"
            } else {
                "disabled"
            }
            .into();
            element.value = confirmation_disabled_reason(w).map(str::to_owned);
        }
    }
    if let Some(reason) = confirmation_disabled_reason(w) {
        for element in &mut out {
            if element
                .commands
                .first()
                .is_some_and(|command| command == "draft.apply")
            {
                element.value = Some(reason.into());
                if let Some(data) = element.data.as_object_mut() {
                    data.insert("disabled_reason".into(), reason.into());
                }
            }
        }
    }
    out.extend(crate::contours_ui::semantics(w));
    out
}

pub fn confirmation_disabled_reason(w: &Workspace) -> Option<&'static str> {
    if !crate::materials_ui::current_source_is_valid(w) {
        return Some("Correct the current material library fields before applying");
    }
    if !crate::components_ui::current_source_is_valid(w) {
        return Some("Correct or select the component before applying");
    }
    if !crate::production_ui::current_source_is_valid(w) {
        return Some("Correct the current production fields before applying");
    }
    if !crate::drawing_interaction_ui::current_source_is_valid(w) {
        return Some("Finish or cancel the drawing gesture and correct its fields before applying");
    }
    if !crate::drawing_editor_ui::current_source_is_valid(w) {
        return Some("Correct the current drawing fields before applying");
    }
    if !crate::assembly_tools_ui::valid(w) {
        return Some("Validate the current assembly group or copy before applying");
    }
    if !crate::assembly_welds_ui::current_source_is_valid(w) {
        return Some("Correct the current assembly weld source before applying");
    }
    if !crate::operation_editor_ui::current_source_is_valid(w) {
        return Some("Correct the current operation fields before applying");
    }
    if !crate::reference_repair_ui::current_source_is_valid(w) {
        return Some("Preview the current explicit reference repair before applying");
    }
    if w.confirm_requested {
        return Some("Waiting for the current operation to finish validation");
    }
    if w.sketch.as_ref().is_some_and(|s| {
        !s.closed
            || s.error.is_some()
            || s.choosing_plane
            || s.edit.is_some()
            || !s.clicks.is_empty()
            || s.tool.is_spline() && !s.chain.is_empty()
    }) {
        return Some("Finish drawing and resolve sketch errors before applying");
    }
    if [
        "feature-source-invalid",
        "mate-source-invalid",
        "drawing-source-invalid",
        "component-pattern-source-invalid",
        "configuration-source-invalid",
        "exploded-view-source-invalid",
        "assembly-inspection-source-invalid",
        "assembly-weld-source-invalid",
        "appearance-source-invalid",
    ]
    .iter()
    .any(|key| w.fields.contains_key(*key))
    {
        return Some("Correct the active operation's source before applying");
    }
    if w.fields.contains_key("appearance-tool")
        && !crate::appearances_ui::current_source_is_valid(w)
    {
        return Some("Correct the visual finish or lighting fields before applying");
    }
    if let Some(draft) = &w.session.draft {
        if draft.error.is_some() {
            return Some("Resolve the current preview error before applying");
        }
        if draft.pending
            || !draft.evaluation.as_ref().is_some_and(|evaluation| {
                evaluation.valid && evaluation.identity == w.session.identity()
            })
        {
            return Some("A current validated preview is required");
        }
    } else if !(w.fields.contains_key("exploded-view-editor")
        || w.fields.contains_key("assembly-inspection-editor")
        || w.fields.contains_key("assembly-weld-editor"))
    {
        return Some("No current operation to apply");
    }
    None
}
pub fn confirmation_available(w: &Workspace) -> bool {
    confirmation_disabled_reason(w).is_none()
}

pub fn menu(ui: &mut egui::Ui, w: &mut Workspace) {
    ui.horizontal(|ui| {
        ui.label(RichText::new("TCAD").strong());
        menu_button(ui, "File", |ui| {
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
                if menu_action(ui, command, label, true).clicked() {
                    ui::action(w, command);
                    ui.close();
                }
            }
            ui.separator();
            if menu_action(ui, "file.example", "Open Example: Drive Unit", true).clicked() {
                ui::action(w, "file.example");
                ui.close();
            }
            ui.separator();
            if menu_action(ui, "file.export-drawing-dxf", "Export Drawing DXF…", true).clicked() {
                ui::export_format(w, "drawing-dxf", "drawing.dxf");
                ui.close();
            }
            if menu_action(
                ui,
                "file.export-flat-pattern",
                "Export Flat Pattern DXF…",
                true,
            )
            .clicked()
            {
                ui::action(w, "file.export-flat-pattern");
                ui.close();
            }
            if menu_action(ui, "file.export-bom", "Export BOM CSV…", true).clicked() {
                ui::action(w, "file.export-bom");
            }
            if menu_action(ui, "file.export-cutlist", "Export Cut List CSV…", true).clicked() {
                ui::action(w, "file.export-cutlist");
                ui.close();
            }
        });
        menu_button(ui, "Edit", |ui| {
            if menu_action(ui, "document.undo", "Undo  ⌘Z", w.session.can_undo()).clicked() {
                w.session.undo();
                w.requested = None;
                ui.close();
            }
            if menu_action(ui, "document.redo", "Redo  ⇧⌘Z", w.session.can_redo()).clicked() {
                w.session.redo();
                w.requested = None;
                ui.close();
            }
            if menu_action(ui, "parameter.new", "Parameters…", true).clicked() {
                w.open_editor("parameter");
                ui.close();
            }
            ui.separator();
            if menu_action(ui, "settings", "Settings…  ⌘,", true).clicked() {
                crate::settings::open(w, None);
                ui.close();
            }
        });
        menu_button(ui, "Insert", |ui| {
            for group in feature_groups() {
                menu_button(ui, group, |ui| {
                    for tool in cad_core::modeling_tools::TOOLS
                        .iter()
                        .filter(|t| t.group == group)
                    {
                        if menu_action(ui, tool.command, tool.label, true).clicked() {
                            ui::action(w, tool.command);
                            ui.close();
                        }
                    }
                });
            }
        });
        menu_button(ui, "View", |ui| {
            view_controls(ui, w);
        });
        menu_button(ui, "Tools", |ui| {
            for (label, command) in [
                ("Settings…  ⌘,", "settings"),
                ("Repair References…", "reference.repair"),
                ("Assembly and Mates…", "assembly.edit"),
                ("Move Component", "assembly.move"),
                ("Rotate Component", "assembly.rotate"),
                ("Component Pattern…", "assembly.pattern"),
                ("Rigid Group…", "assembly.group-new"),
                ("Copy With Mates…", "assembly.copy-with-mates"),
                ("Exploded View…", "assembly.exploded-new"),
                ("Interference / Clearance…", "assembly.inspection-new"),
                ("Configurations…", "configurations"),
                ("Equations…", "equations"),
                ("Engineering Materials…", "materials"),
                ("Appearance · Colors and Textures…", "appearances"),
                ("Studio Lighting…", "lighting"),
                ("Manufacturing…", "manufacturing"),
                ("Drawings…", "drawings"),
                ("Component Libraries…", "libraries"),
                ("Bill of Materials…", "bom"),
                ("Navigation Settings…", "controls"),
            ] {
                if menu_action(ui, command, label, true).clicked() {
                    ui::action(w, command);
                    ui.close();
                }
            }
        });
        menu_button(ui, "Workspace", |ui| {
            for (label, command) in [
                ("Layout and Theme…", "presentation"),
                ("Versions…", "versions"),
                ("Review Comments…", "comments"),
                ("Rebuild  F5", "document.rebuild"),
                ("Source Definitions…", "source"),
                ("Create Release…", "file.release"),
            ] {
                if menu_action(ui, command, label, true).clicked() {
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
            if command_button(ui, "commands", "Find a command  ⌘K", true).clicked() {
                w.open_editor("commands");
            }
        });
    });
}
pub fn commands(ui: &mut egui::Ui, w: &mut Workspace) {
    ui.horizontal(|ui| {
        for tab in [
            "Features", "Sketch", "Assembly", "Drawing", "Evaluate", "Graph",
        ] {
            if controls::selectable(
                ui,
                &format!("command-tab/{}", tab.to_lowercase()),
                "command-tab",
                tab,
                w.command_tab == tab,
            )
            .clicked()
            {
                presentation_action(
                    w,
                    &format!("presentation.command-tab.{}", tab.to_lowercase()),
                );
            }
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            menu_button(ui, "Size", |ui| {
                if menu_action(ui, "presentation.compact", "Compact", true).clicked() {
                    w.view.ui_scale = 1.;
                    w.view.toolbar_height = 68.;
                    resize_toolbar(w);
                    crate::settings::save_live_application(w);
                    ui.close();
                }
                if menu_action(ui, "presentation.comfortable", "Comfortable", true).clicked() {
                    w.view.ui_scale = 1.15;
                    w.view.toolbar_height = 80.;
                    resize_toolbar(w);
                    crate::settings::save_live_application(w);
                    ui.close();
                }
                if ui
                    .add(egui::Slider::new(&mut w.view.ui_scale, 0.75..=1.5).text("UI scale"))
                    .changed()
                {
                    crate::settings::save_live_application(w);
                }
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
                        "command-toolbar/sketch.new",
                        if sketching { "Finish" } else { "Sketch" },
                        Icon::Sketch,
                        if sketching {
                            confirmation_available(w)
                        } else {
                            !blocked
                        },
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
                            &format!("command-toolbar/{}", sketch_tool_command(tool)),
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
                        menu_button(ui, group, |ui| {
                            for (label, tool) in &tools {
                                if command_button(ui, &sketch_tool_command(*tool), label, !blocked)
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
                        menu_button(ui, menu, |ui| {
                            for (command, label, group) in cad_core::modeling_tools::SKETCH_TOOLS {
                                if *group == category
                                    && command_button(ui, command, label, sketching).clicked()
                                {
                                    ui::action(w, command);
                                    ui.close();
                                }
                            }
                        });
                    }
                    if command_button(
                        ui,
                        "sketch.smart-dimension",
                        "Smart Dimension  D",
                        sketching,
                    )
                    .clicked()
                    {
                        ui::action(w, "sketch.smart-dimension");
                    }
                    if command_button(ui, "sketch.text", "Text", sketching).clicked() {
                        ui::action(w, "sketch.text");
                    }
                    if command_button(ui, "sketch.equation", "Equation Curve", sketching).clicked()
                    {
                        ui::action(w, "sketch.equation");
                    }
                    if command_button(
                        ui,
                        "sketch.reference-dimension",
                        "Reference Dimension",
                        sketching,
                    )
                    .clicked()
                    {
                        ui::action(w, "sketch.reference-dimension");
                    }
                    if command_button(ui, "sketch.relations", "Relations", sketching).clicked() {
                        ui::action(w, "sketch.relations");
                    }
                    if command_button(ui, "viewport.normal-sketch", "Normal To", true).clicked() {
                        sketch::normal_to(w);
                    }
                }
                "Graph" => {
                    for scope in ["Features", "Assembly"] {
                        if controls::selectable(
                            ui,
                            &format!("graph-scope/{}", scope.to_lowercase()),
                            "graph-scope",
                            scope,
                            w.view.graph_scope == scope,
                        )
                        .clicked()
                        {
                            w.view.graph_scope = scope.into();
                        }
                    }
                    if command_button(ui, "graph.arrange", "Arrange Graph", true).clicked() {
                        w.view.graph_positions.clear();
                        w.view.graph_pan = [0.; 2];
                        w.view.graph_zoom = 1.;
                    }
                    ui.small("Drag cards · scroll to pan · pinch to zoom · double-click to edit");
                }
                "Assembly" => {
                    for (label, cmd) in [
                        ("New Part in Assembly", "part.new"),
                        ("Insert Existing", "assembly.insert-existing"),
                        ("Move", "assembly.move"),
                        ("Mate", "assembly.mate-new"),
                        ("Components", "assembly.edit"),
                        ("Library", "libraries"),
                    ] {
                        if command_button(ui, cmd, label, true).clicked() {
                            ui::action(w, cmd);
                        }
                    }
                    if command_button(
                        ui,
                        "assembly.isolate",
                        if w.view.isolation {
                            "Finish Editing"
                        } else {
                            "Edit in Place"
                        },
                        true,
                    )
                    .clicked()
                    {
                        w.isolate();
                    }
                    if controls::button(
                        ui,
                        "command-toolbar/presentation.ghost-others",
                        "command-action",
                        &["toolbar-control"],
                        "Ghost others",
                        true,
                        w.view.ghost_others,
                    )
                    .clicked()
                    {
                        w.view.ghost_others = !w.view.ghost_others;
                        w.rendered_key.clear();
                    }
                }
                "Evaluate" => {
                    if command_button(ui, "selection.measure", "Measure", w.selected.is_some())
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
                        if command_button(ui, cmd, label, true).clicked() {
                            ui::action(w, cmd);
                        }
                    }
                }
                "Drawing" => {
                    for (label, command) in [
                        ("New Sheet", "drawing.sheet-add"),
                        ("Model", "drawing.workspace-model"),
                    ] {
                        if command_button(ui, command, label, true).clicked() {
                            ui::action(w, command);
                        }
                    }
                }
                _ => {
                    if tool_button(
                        ui,
                        "command-toolbar/sketch.new",
                        "Sketch",
                        Icon::Sketch,
                        !blocked,
                        false,
                        "Create a sketch on an origin plane or model face",
                    ) {
                        ui::action(w, "sketch.new");
                    }
                    for group in feature_groups() {
                        menu_button(ui, group, |ui| {
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
                                if tool_button(
                                    ui,
                                    &format!("command-toolbar/{}", tool.command),
                                    tool.label,
                                    kind,
                                    enabled,
                                    false,
                                    tool.label,
                                ) {
                                    ui::action(w, tool.command);
                                    ui.close();
                                }
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
            if controls::selectable(
                ui,
                &format!("navigation/{}", mode.to_lowercase()),
                "navigation-action",
                label,
                w.manager_tree && w.view.mode == mode,
            )
            .clicked()
            {
                w.manager_tree = true;
                w.view.mode = mode.into();
            }
        }
        if controls::button(
            ui,
            "navigation/presentation",
            "command-action",
            &["navigation-control"],
            "⋯",
            true,
            false,
        )
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
        ui.add_enabled_ui(w.tool_checkpoint.is_none(), |ui| {
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
        });
        if active != w.view.active_part && w.tool_checkpoint.is_none() && w.sketch.is_none() {
            w.activate_context(active, None);
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
                let response = controls::button(
                    ui,
                    &format!("component:{}", placed.path.join("/")),
                    "component-instance",
                    &["feature-tree-row"],
                    &placed.name,
                    true,
                    w.view.active_instance.as_ref() == placed.path.last(),
                );
                if response.clicked() {
                    if w.tool_checkpoint.is_none() {
                        w.activate_context(
                            Some(placed.definition.clone()),
                            Some(placed.path.clone()),
                        );
                    }
                    crate::selection_ui::object_selected(
                        w,
                        cad_core::selection_actions::SelectionTarget::Object {
                            definition: placed.definition.clone(),
                            object: placed.path.last().cloned().unwrap_or_default(),
                            role: "component-instance".into(),
                        },
                    );
                    if w.tool_checkpoint.is_none() {
                        w.edge_selection_path = Some(placed.path.clone());
                    }
                }
            }
            crate::assembly_groups_ui::tree(ui, w);
            crate::assembly_welds_ui::tree(ui, w);
            if ui.button("Insert Component").clicked() {
                ui::action(w, "assembly.insert-existing");
            }
        });
        return;
    };
    if doc.assemblies.contains_key(&doc.root)
        && controls::button(
            ui,
            "feature-tree/assembly-root",
            "assembly-breadcrumb",
            &["feature-tree-row"],
            &format!("← {}", doc.name),
            w.tool_checkpoint.is_none(),
            false,
        )
        .clicked()
    {
        w.activate_context(None, None);
        return;
    }
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
            crate::feature_order_ui::scroll(ui);
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
                let finish = doc
                    .finishes
                    .get(&effective.finish)
                    .map(|finish| finish.name.as_str())
                    .unwrap_or("Missing finish");
                if controls::button(
                    ui,
                    &format!("{}::finish", part.id),
                    "appearance-assignment",
                    &["feature-tree-row"],
                    &format!("Finish: {finish}"),
                    w.tool_checkpoint.is_none(),
                    false,
                )
                .clicked()
                {
                    w.view.active_part = Some(part.id.clone());
                    crate::appearances_ui::begin(w, false);
                }
                ui.separator();
                for name in ["Front Plane", "Top Plane", "Right Plane"] {
                    let response = controls::button(
                        ui,
                        &format!("reference:{}::{name}", part.id),
                        "reference-plane",
                        &["feature-tree-row"],
                        &format!("▱  {name}"),
                        true,
                        w.active_plane.as_deref() == Some(name),
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
                let response = controls::button(
                    ui,
                    &format!("reference:{}::Origin", part.id),
                    "reference-origin",
                    &["feature-tree-row"],
                    "+  Origin",
                    true,
                    false,
                );
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
    if controls::selectable(
        ui,
        "history-view",
        "history-view",
        "Latest model",
        w.view.history_cursor.is_none(),
    )
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
                    .map(|cursor| cursor.definition == part.id && cursor.node == feature.id)
                    .unwrap_or_else(|| w.view.active_node.as_ref() == Some(&feature.id));
                let future = w
                    .view
                    .history_cursor
                    .as_ref()
                    .filter(|cursor| cursor.definition == part.id)
                    .and_then(|cursor| {
                        part.features
                            .iter()
                            .position(|feature| feature.id == cursor.node)
                    })
                    .is_some_and(|cursor| index > cursor);
                let id = format!("history:{}::{}", part.id, feature.id);
                let classes = if future {
                    vec!["history-row", "future"]
                } else {
                    vec!["history-row"]
                };
                let props = controls::properties(
                    &id,
                    "history-station",
                    &classes,
                    if active { "selected" } else { "ready" },
                );
                let marker = std::cell::Cell::new(None);
                let draw = |painter: &egui::Painter, center: egui::Pos2, size: f32, color| {
                    marker.set(Some(center));
                    painter.circle_filled(center, size * 0.23, color);
                    if active {
                        painter.circle_stroke(
                            center,
                            size * 0.42,
                            Stroke::new(controls::number(&props, "stroke-width", 1.2), color),
                        );
                    }
                };
                let label = format!("{:02}  {}", index + 1, feature.name);
                let response = controls::icon_button(
                    ui,
                    &id,
                    "history-station",
                    &classes,
                    &label,
                    true,
                    active,
                    Some(&draw),
                )
                .on_hover_text(format!(
                    "{}\n{}",
                    feature.name,
                    operation_summary(&feature.operation)
                ));
                if let Some(marker) = marker.get() {
                    let path = controls::properties(
                        &format!("history-path:{}", part.id),
                        "history-path",
                        &[],
                        "ready",
                    );
                    if let Some(last) = last {
                        let color = path
                            .get("color")
                            .and_then(|color| crate::ui::color(color))
                            .unwrap_or_else(|| ui.visuals().weak_text_color());
                        ui.painter().line_segment(
                            [last, marker],
                            Stroke::new(controls::number(&path, "stroke-width", 1.), color),
                        );
                    }
                    last = Some(marker);
                }
                if active
                    && w.fields
                        .get("history-focus")
                        .is_none_or(|id| id != &feature.id)
                {
                    ui.scroll_to_rect(response.rect, Some(egui::Align::Center));
                    w.fields.insert("history-focus".into(), feature.id.clone());
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
    let response = controls::button(
        ui,
        &feature.id,
        "feature-station",
        &["feature-tree-row"],
        &format!(
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
        true,
        w.view.active_node.as_ref() == Some(&feature.id),
    );
    let response = crate::feature_order_ui::row(ui, w, part, &feature.id, response);
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
            .add_enabled(
                !w.fields.contains_key("appearance-tool"),
                egui::Button::new(if matches!(feature.operation, Operation::Profile { .. }) {
                    "Edit Sketch"
                } else {
                    "Edit Feature"
                }),
            )
            .on_disabled_hover_text("Apply or cancel appearance changes before editing a feature")
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
pub fn confirmation_label(w: &Workspace) -> String {
    if w.sketch.is_some() {
        return crate::holes_ui::sketch_semantics(w)
            .iter()
            .find_map(|element| element.data["completion_label"].as_str().map(str::to_owned))
            .unwrap_or_else(|| "Finish Sketch".into());
    }
    if w.dialog.as_deref() == Some("component-insert") {
        "OK Component".into()
    } else if w.fields.contains_key("mate-editor") {
        "OK Mate".into()
    } else if w.dialog.as_deref() == Some("assembly") {
        "Apply Assembly".into()
    } else if crate::drawing_ui::is_workspace(w) {
        "Apply Drawing".into()
    } else if w.fields.contains_key("material-tool") {
        "Apply Materials".into()
    } else if w.fields.contains_key("appearance-tool") {
        "Apply Appearance".into()
    } else {
        "OK Feature".into()
    }
}
pub fn confirmation(ui: &mut egui::Ui, w: &mut Workspace) {
    let label = confirmation_label(w);
    ui.horizontal(|ui| {
        let enabled = confirmation_available(w);
        if controls::button(
            ui,
            "draft.apply",
            "apply-action",
            &["confirmation-control"],
            &label,
            enabled,
            false,
        )
        .on_hover_text("Confirm this operation · Command + Enter")
        .clicked()
        {
            w.confirm_tool();
        }
        if controls::button(
            ui,
            "draft.cancel",
            "cancel-action",
            &["confirmation-control"],
            "Cancel",
            true,
            false,
        )
        .clicked()
        {
            if w.tool_checkpoint.is_some() {
                w.cancel_tool();
            } else {
                w.cancel();
            }
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
    controls::expression(
        ui,
        &format!("feature-field/{}", label.to_lowercase().replace(' ', "-")),
        label,
        expression,
    )
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
fn selected_edges(
    ui: &mut egui::Ui,
    w: &mut Workspace,
    feature: &str,
    body: &cad_core::NodeRef,
    edges: &mut Vec<cad_core::TopologyRef>,
) -> bool {
    crate::selection_ui::edge_picker(ui, w, feature, body, edges)
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
        Operation::WeldBead { .. } => {
            ui.label("Weld Bead parameters are exposed through the modeling property manager.");
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
                w,
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
                w,
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
        Operation::Fillet {
            body,
            edges,
            radius,
        } => {
            ui.label(format!("Edges to fillet: {}", edges.len()));
            changed |= selected_edges(ui, w, &feature.id, body, edges);
            changed |= expression(ui, "Radius", radius);
            if edges.is_empty() {
                ui.small("Select edges on the model to round.");
            }
        }
        Operation::Chamfer {
            edges,
            distance,
            options,
            body,
        } => {
            ui.label(format!("Edges to chamfer: {}", edges.len()));
            changed |= selected_edges(ui, w, &feature.id, body, edges);
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
                if let Some(selected) = w.selected.as_ref().filter(|selection| {
                    selection.reference.topology == "face"
                        && selection.body.as_ref() == Some(body)
                        && crate::selection_ui::edge_occurrence(w, &selection.path)
                }) && ui.button("Use selected face for Distance 1").clicked()
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
                w,
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
        let settings = &w.session.visible().settings;
        if matches!(
            selected.reference.topology.as_str(),
            "plane" | "axis" | "point"
        ) {
            w.status = format!(
                "{} · Origin / Position: {}, {}, {} (assembly coordinates)",
                selected.name,
                settings.format_length(selected.point[0]),
                settings.format_length(selected.point[1]),
                settings.format_length(selected.point[2])
            );
            return;
        }
        w.status = selected
            .reference
            .signature
            .as_ref()
            .map(|sig| {
                let measure = if selected.reference.topology == "edge" {
                    settings.format_length(sig.measure)
                } else {
                    format!(
                        "{:.precision$} {}²",
                        sig.measure / settings.display_length_unit.millimeters_per_unit().powi(2),
                        settings.display_length_unit.symbol(),
                        precision = usize::from(settings.length_decimal_places)
                    )
                };
                format!(
                    "{}: {} · Center: {}, {}, {}",
                    selected.reference.topology,
                    measure,
                    settings.format_length(sig.center[0]),
                    settings.format_length(sig.center[1]),
                    settings.format_length(sig.center[2])
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
        if menu_action(
            ui,
            &format!("viewport.orientation.{}", label.to_lowercase()),
            label,
            true,
        )
        .clicked()
        {
            w.view.orbit = orbit;
            w.view.roll = 0.;
            crate::navigation::stop(w);
            ui.close();
        }
    }
    ui.separator();
    if menu_action(ui, "viewport.fit", "Zoom to Fit  F", true).clicked() {
        viewport::fit(w, false);
        ui.close();
    }
    if menu_action(ui, "viewport.normal-sketch", "Normal To Sketch", true).clicked() {
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
        if let Some((state, label)) = crate::operation_preview::summary(w) {
            ui.separator();
            controls::label(
                ui,
                "workspace.operation-preview",
                "operation-preview-status",
                &[],
                &label,
                &state,
            )
            .on_hover_text(crate::operation_preview::tooltip(w));
        }
        if w.session.draft.is_some() && w.tool_checkpoint.is_none() {
            if controls::button(
                ui,
                "draft.apply",
                "apply-action",
                &["status-control"],
                "Confirm Changes",
                confirmation_available(w),
                false,
            )
            .clicked()
            {
                w.confirm_tool();
            }
            if controls::button(
                ui,
                "draft.cancel",
                "cancel-action",
                &["status-control"],
                "Cancel",
                true,
                false,
            )
            .clicked()
            {
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
