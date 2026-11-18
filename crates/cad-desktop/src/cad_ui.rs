//! Familiar CAD workspace widgets over the same document commands and user layout.
use crate::{Workspace, presentation_controls as controls, sketch, ui};
use bevy_egui::egui::{self, RichText};
use cad_core::{Feature, Operation, Part, presentation::SemanticElement};
mod profile_features;

fn tool_button(
    ui: &mut egui::Ui,
    command: &str,
    label: &str,
    reason: Option<&str>,
    active: bool,
    hint: &str,
) -> bool {
    let draw = |p: &egui::Painter, center, size, color| {
        crate::command_icons::paint(p, center, size, color, command)
    };
    controls::icon_button(
        ui,
        &format!("command-toolbar/{command}"),
        "command-action",
        &["toolbar-control"],
        label,
        reason.is_none(),
        active,
        Some(&draw),
    )
    .on_hover_text(hint)
    .on_disabled_hover_text(reason.unwrap_or(hint))
    .clicked()
}

fn command_button(
    ui: &mut egui::Ui,
    w: &Workspace,
    command: &str,
    label: &str,
    enabled: bool,
) -> egui::Response {
    let reason = command_disabled_reason(w, command);
    controls::button(
        ui,
        &format!("command-toolbar/{command}"),
        "command-action",
        &["toolbar-control"],
        label,
        enabled && reason.is_none(),
        false,
    )
    .on_disabled_hover_text(
        reason.unwrap_or("Finish the current operation or select the required geometry first"),
    )
}
fn menu_action(
    ui: &mut egui::Ui,
    w: &Workspace,
    command: &str,
    label: &str,
    enabled: bool,
) -> egui::Response {
    let reason = command_disabled_reason(w, command);
    let shortcut = shortcut_text(ui.ctx(), command);
    controls::menu_item(
        ui,
        &format!("command-menu/{command}"),
        label,
        shortcut.as_deref(),
        enabled && reason.is_none(),
        false,
    )
    .on_disabled_hover_text(
        reason.unwrap_or("Finish the current operation or select the required geometry first"),
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

// The native menu and theme-authored menu placements share these groups.
const TOOL_GROUPS: &[(&str, &str, &[&str])] = &[
    (
        "menu/tools-model",
        "Model",
        &[
            "properties",
            "configurations",
            "equations",
            "reference.repair",
        ],
    ),
    (
        "menu/tools-assembly",
        "Assembly",
        &[
            "assembly.edit",
            "assembly.move",
            "assembly.rotate",
            "assembly.pattern",
            "assembly.group-new",
            "assembly.copy-with-mates",
            "assembly.exploded-new",
        ],
    ),
    (
        "menu/tools-inspect",
        "Inspect",
        &[
            "assembly.inspection-new",
            "inspection.sensors",
            "inspection.tolerance-stacks",
        ],
    ),
    (
        "menu/tools-appearance",
        "Materials and appearance",
        &["materials", "appearances", "lighting"],
    ),
    (
        "menu/tools-production",
        "Drawings and manufacturing",
        &["drawings", "bom", "manufacturing"],
    ),
    (
        "menu/tools-libraries",
        "Libraries and reuse",
        &[
            "libraries",
            "standard-parts",
            "sketch-block-library",
            "component.fork",
            "feature-transfer",
        ],
    ),
];

const EXPORT_MENU: (&str, &str, &[&str]) = (
    "menu/file-exports",
    "Export",
    &[
        "file.export",
        "file.export-drawing-dxf",
        "file.export-flat-pattern",
        "file.export-bom",
        "file.export-cutlist",
    ],
);
const ORIENTATION_MENU: (&str, &str, &[&str]) = (
    "menu/standard-views",
    "Standard views",
    &[
        "viewport.orientation.front",
        "viewport.orientation.back",
        "viewport.orientation.top",
        "viewport.orientation.bottom",
        "viewport.orientation.right",
        "viewport.orientation.left",
        "viewport.orientation.isometric",
    ],
);

fn shortcut_text(ctx: &egui::Context, command: &str) -> Option<String> {
    use egui::{Key, Modifiers};
    let (modifiers, key) = match command {
        "file.new-part" => (Modifiers::COMMAND, Key::N),
        "file.open" => (Modifiers::COMMAND, Key::O),
        "file.save" => (Modifiers::COMMAND, Key::S),
        "document.undo" => (Modifiers::COMMAND, Key::Z),
        "document.redo" => (Modifiers::COMMAND | Modifiers::SHIFT, Key::Z),
        "settings" => (Modifiers::COMMAND, Key::Comma),
        "commands" => (Modifiers::COMMAND, Key::K),
        "viewport.fit" => (Modifiers::NONE, Key::F),
        "draft.apply" => (Modifiers::COMMAND, Key::Enter),
        "document.rebuild" => (Modifiers::NONE, Key::F5),
        _ => return None,
    };
    Some(ctx.format_shortcut(&egui::KeyboardShortcut::new(modifiers, key)))
}

// Insert and the full feature menu always address modeling commands, including
// when the Sketch tab offers its own Reference and Pattern menus.
fn feature_menu_ids() -> Vec<String> {
    feature_groups()
        .into_iter()
        .map(|group| match group {
            "Reference" => "menu/feature-reference".into(),
            "Pattern" => "menu/feature-pattern".into(),
            _ => format!("menu/{}", group.to_lowercase().replace(' ', "-")),
        })
        .collect()
}

fn menu_items(name: &str, w: &Workspace) -> Vec<String> {
    let commands: Vec<&str> = match name {
        "File" => vec![
            "file.new-part",
            "file.new",
            "file.open",
            "repositories.open",
            "file.save",
            "file.save-as",
            "file.publish",
            "publish.settings",
            "document.rename",
            "file.import",
            EXPORT_MENU.0,
            "file.recover",
        ],
        "Edit" => vec![
            "document.undo",
            "document.redo",
            "parameter.new",
            "settings",
        ],
        "Insert" => {
            let mut items = vec![
                "command-menu/sketch.new".into(),
                "command-menu/sketch.new-3d".into(),
            ];
            items.extend(feature_menu_ids());
            return items;
        }
        "All features" => return feature_menu_ids(),
        "Draw geometry" => vec![
            "menu/shapes",
            "menu/curves",
            "menu/sketch-references",
            "sketch.text",
            "sketch.equation",
        ],
        "Edit geometry" => vec![
            "menu/edit-entities",
            "menu/transform",
            "menu/patterns",
            "menu/blocks",
        ],
        "Dimensions and relations" => vec![
            "sketch.smart-dimension",
            "sketch.reference-dimension",
            "sketch.relations",
            "sketch.toggle-dimension-mode",
            "menu/dimension-groups",
        ],
        "View" => vec![
            "menu/Themes",
            "viewport.visibility-menu",
            ORIENTATION_MENU.0,
            "viewport.fit",
            "viewport.normal-sketch",
            "viewport.fit-sketch",
            "viewport.fit-support",
            "viewport.fit-part-context",
            "viewport.sketches.auto",
            "viewport.sketches.all",
            "viewport.sketches.hidden",
            "viewport.sketch-xray",
            "viewport.references.auto",
            "viewport.references.all",
            "viewport.references.hidden",
            "viewport.reference-xray",
            "lighting",
            "camera-views",
            "camera-view.save",
            "camera-view.perspective",
            "camera-view.orthographic",
        ],
        "Tools" => {
            return ["command-menu/settings", "command-menu/controls"]
                .into_iter()
                .chain(TOOL_GROUPS.iter().map(|(id, _, _)| *id))
                .map(str::to_owned)
                .collect();
        }
        "Workspace" => vec![
            "menu/Themes",
            "presentation",
            "versions",
            "repositories",
            "git-connections",
            "content-repositories",
            "readme.preview",
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
        "Sketch references" | "Reference"
            if name == "Sketch references" || w.command_tab == "Sketch" =>
        {
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
            if !sketch.is_empty()
                && (w.command_tab == "Sketch"
                    || matches!(
                        name,
                        "Edit Entities" | "Transform" | "Patterns" | "Blocks" | "Dimension Groups"
                    ))
            {
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
        .map(|command| {
            if command.starts_with("menu/") || command == "viewport.visibility-menu" {
                command.into()
            } else {
                format!("command-menu/{command}")
            }
        })
        .collect()
}

/// A menu is also an ordinary semantic component, so user layouts can move or
/// duplicate individual headings without embedding a second command route.
pub fn semantic_menu_as(
    ui: &mut egui::Ui,
    w: &mut Workspace,
    element: &cad_core::presentation::SemanticElement,
    id: &str,
    role: &str,
    snapshot: &[cad_core::presentation::SemanticElement],
) {
    controls::menu_button_as(ui, id, role, &element.label, |ui| {
        menu_contents(ui, w, element, snapshot);
    });
}

fn bound_menu(ui: &mut egui::Ui, w: &mut Workspace, snapshot: &[SemanticElement], id: &str) {
    if let Some(menu) = snapshot.iter().find(|entry| entry.id == id) {
        let role = if menu
            .relationships
            .iter()
            .any(|parent| parent == "command-manager")
        {
            "ribbon-group-menu"
        } else {
            &menu.role
        };
        semantic_menu_as(ui, w, menu, &menu.id, role, snapshot);
    }
}

fn menu_contents(
    ui: &mut egui::Ui,
    w: &mut Workspace,
    element: &cad_core::presentation::SemanticElement,
    snapshot: &[cad_core::presentation::SemanticElement],
) {
    for item in element
        .data
        .get("items")
        .or_else(|| element.data.get("children"))
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(serde_json::Value::as_str)
    {
        let Some(descriptor) = snapshot.iter().find(|descriptor| descriptor.id == item) else {
            ui.add_enabled(false, egui::Button::new(format!("Unavailable: {item}")))
                .on_disabled_hover_text(
                    "This theme references a menu entry that is not available in this version",
                );
            continue;
        };
        if descriptor.role == "menu" {
            controls::menu_button(ui, &descriptor.id, &descriptor.label, |ui| {
                menu_contents(ui, w, descriptor, snapshot);
            });
            continue;
        }
        let action = descriptor.action_descriptors().into_iter().next();
        let reason = action.as_ref().and_then(|action| {
            action
                .disabled_reason
                .as_deref()
                .or_else(|| command_disabled_reason(w, &action.command))
        });
        let enabled = action.as_ref().is_some_and(|action| action.enabled) && reason.is_none();
        let shortcut = action
            .as_ref()
            .and_then(|a| shortcut_text(ui.ctx(), &a.command));
        let response = if descriptor.role == "viewport-visibility-toggle"
            || descriptor.role == "theme-action" && descriptor.data["management"] != true
        {
            controls::button(
                ui,
                &descriptor.id,
                "menu-action",
                &["menu-item", "menu-choice"],
                &descriptor.label,
                enabled,
                descriptor.state == "selected" || descriptor.data["selected"] == true,
            )
        } else {
            controls::menu_item(
                ui,
                &descriptor.id,
                &descriptor.label,
                shortcut.as_deref(),
                enabled,
                descriptor.state == "selected",
            )
        }
        .on_disabled_hover_text(
            reason
                .or(descriptor.value.as_deref())
                .unwrap_or("This menu item has no available action"),
        );
        if response.clicked()
            && let Some(action) = action
        {
            let mut target = descriptor.clone();
            target.data = action.arguments;
            ui::semantic_action(w, &target, &action.command);
            ui.close();
        }
    }
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
    if crate::sketch_overlay::action(w, command) {
        return true;
    }
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
    if command == "presentation.manager-edit" {
        if w.tool_checkpoint.is_some()
            || w.session.draft.is_some()
            || w.sketch.is_some()
            || w.dialog.is_some()
        {
            w.manager_tree = false;
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
        if let Some(reason) = command_disabled_reason(w, command) {
            w.status = reason.into();
            return true;
        }
        if *mode != "History" {
            crate::history::latest(w);
        }
        w.manager_tree = true;
        w.view.mode = (*mode).into();
        return true;
    }
    if let Some(name) = command.strip_prefix("presentation.graph-scope.")
        && let Some(scope) = ["Features", "Assembly"]
            .iter()
            .find(|scope| scope.eq_ignore_ascii_case(name))
    {
        if let Some(reason) = crate::graph_ui::scope_reason(w, scope) {
            w.status = reason.into();
        } else {
            w.view.graph_scope = (*scope).into();
        }
        return true;
    }
    match command {
        "presentation.selection-dismiss" => {
            w.selection_popup = None;
        }
        "document.undo" => {
            if w.tool_checkpoint.is_some() || w.session.can_undo() {
                ui::history_command(w, false);
            }
        }
        "document.redo" => {
            if w.tool_checkpoint.is_some() || w.session.can_redo() {
                ui::history_command(w, true);
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
        "assembly.isolate" => {
            if let Some(reason) = command_disabled_reason(w, command) {
                w.status = reason.into();
            } else if let Some(path) = w.selected_component.clone() {
                crate::assembly_tree_ui::edit_selected_part(w, path);
            }
        }
        "presentation.ghost-others" => {
            // Existing layouts use this spelling for the Ghost others choice.
            crate::workspace_controls::action(w, "workspace.ghost-others");
        }
        "viewport.normal-sketch" => sketch::normal_to(w),
        "selection.measure" => measure(w),
        "file.export-pdf" => ui::export_format(w, "pdf", "drawing.pdf"),
        "file.export-drawing-dxf" => ui::export_format(w, "drawing-dxf", "drawing.dxf"),
        _ => return false,
    }
    true
}

pub fn semantics(w: &Workspace) -> Vec<cad_core::presentation::SemanticElement> {
    let mut out = vec![];
    let sketching = w.sketch.is_some();
    let spatial_sketching = crate::sketch3d_ui::active(w);
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
        let reason = if command.is_empty() {
            None
        } else {
            command_disabled_reason(w, &command).or_else(|| {
                (spatial_sketching
                    && command.starts_with("sketch.")
                    && crate::sketch3d_ui::captured(w))
                .then_some("Release or cancel the active spatial handle first")
            })
        };
        let enabled = enabled && reason.is_none();
        out.push(cad_core::presentation::SemanticElement { id, role:role.into(), label:label.into(), value: (!enabled).then(|| reason.unwrap_or("Finish the active operation or create the required geometry first").into()), state:if !enabled {"disabled"} else if selected {"selected"} else {"ready"}.into(), commands:if command.is_empty() {vec![]} else {vec![command]}, relationships:vec![parent.into()], data:serde_json::json!({"presentation_control":true,"accessibility_label":label,"parent":parent,"classes":match role { "menu-action"=>vec!["menu-item"], "command-action"=>vec!["toolbar-control"], "menu"=>vec!["menu-heading"], "command-tab"|"navigation-action"=>vec!["selectable"], _=>vec![] }}) });
    };
    for (command, label) in cad_core::presentation::command_catalog() {
        let enabled = match command {
            "file.publish" => publish_available(w),
            "publish.settings" => true,
            "sketch.new-3d" => !blocked && !sketching,
            "draft.apply" => confirmation_available(w),
            "draft.cancel" => w.tool_checkpoint.is_some() || w.session.draft.is_some() || sketching,
            "viewport.fit-sketch"
            | "viewport.fit-support"
            | "viewport.fit-part-context"
            | "sketch.snap-entities" => sketching,
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
                spatial_sketching && crate::sketch3d_ui::tool_selected(w, command),
                parent,
            );
        }
    }
    for (name, label, tool) in SKETCH_DRAWING_TOOLS {
        let command = format!("sketch.tool.{name}");
        let enabled =
            !blocked || (spatial_sketching && crate::sketch3d_ui::tool_supported(&command));
        let selected = if spatial_sketching {
            crate::sketch3d_ui::tool_selected(w, &command)
        } else {
            w.sketch.as_ref().is_some_and(|s| s.tool == *tool)
        };
        publish(
            format!("command-toolbar/{command}"),
            "command-action",
            label,
            command,
            enabled,
            selected,
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
    publish(
        "command:presentation.manager-edit".into(),
        "command",
        "Active properties",
        "presentation.manager-edit".into(),
        w.tool_checkpoint.is_some()
            || w.session.draft.is_some()
            || w.sketch.is_some()
            || w.dialog.is_some(),
        w.editing_manager(),
        "view-navigation",
    );
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
    for scope in ["Features", "Assembly"] {
        let name = scope.to_lowercase();
        publish(
            format!("graph-scope/{name}"),
            "graph-scope",
            scope,
            format!("presentation.graph-scope.{name}"),
            true,
            crate::graph_ui::scope(w) == scope,
            "command-manager",
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
        "Draw geometry",
        "Edit geometry",
        "Dimensions and relations",
        "Sketch references",
        "All features",
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
        (
            "document.undo",
            "Undo",
            command_disabled_reason(w, "document.undo").is_none(),
            false,
        ),
        (
            "document.redo",
            "Redo",
            command_disabled_reason(w, "document.redo").is_none(),
            false,
        ),
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
        ("assembly.isolate", "Edit in Place", true, false),
        (
            "presentation.ghost-others",
            "Ghost Others",
            crate::workspace_controls::in_place(w),
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
            element.data["accessibility_label"] = serde_json::json!(element.label);
            element.data["completion_scope"] =
                serde_json::json!(if sketching || spatial_sketching {
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
        if (sketching || spatial_sketching)
            && w.command_tab == "Sketch"
            && (element.id == "command-toolbar/sketch.new"
                || (spatial_sketching && element.id == "command-toolbar/sketch.new-3d"))
        {
            element.label = confirmation_label(w);
            element.commands = vec!["draft.apply".into()];
            element.data["accessibility_label"] = serde_json::json!(element.label);
            element.data["completion_scope"] = serde_json::json!("sketch");
            element.state = if confirmation_available(w) {
                "selected"
            } else {
                "disabled"
            }
            .into();
            element.value = confirmation_disabled_reason(w).map(str::to_owned);
        }
    }
    for (group, id) in [
        ("Reference", "menu/feature-reference"),
        ("Pattern", "menu/feature-pattern"),
    ] {
        let items = cad_core::modeling_tools::TOOLS
            .iter()
            .filter(|tool| tool.group == group)
            .map(|tool| format!("command-menu/{}", tool.command))
            .collect::<Vec<_>>();
        out.push(SemanticElement {
            id: id.into(), role: "menu".into(), label: group.into(), value: None,
            state: "ready".into(), commands: vec![],
            relationships: vec!["menu/all-features".into(), "menu/insert".into()],
            data: serde_json::json!({"presentation_control":true,"classes":["menu-heading"],"items":items}),
        });
    }
    for ((id, label, commands), parent) in TOOL_GROUPS
        .iter()
        .copied()
        .map(|group| (group, "menu/tools"))
        .chain([(EXPORT_MENU, "menu/file"), (ORIENTATION_MENU, "menu/view")])
    {
        out.push(cad_core::presentation::SemanticElement {
            id: id.into(),
            role: "menu".into(),
            label: label.into(),
            value: None,
            state: "ready".into(),
            commands: vec![],
            relationships: vec![parent.into()],
            data: serde_json::json!({
                "presentation_control": true, "classes": ["menu-heading"], "parent": parent,
                "items": commands.iter().map(|command| format!("command-menu/{command}")).collect::<Vec<_>>()
            }),
        });
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
    if let Some(part) = w
        .view
        .active_part
        .as_ref()
        .and_then(|id| w.session.visible().parts.get(id))
    {
        for (key, label, items) in [
            ("bodies", "Bodies", part.outputs.clone()),
            (
                "appearance",
                "Material and finish",
                vec![part.material.clone(), part.finish.clone()],
            ),
            (
                "references",
                "Planes and origin",
                vec![
                    "Front Plane".into(),
                    "Top Plane".into(),
                    "Right Plane".into(),
                    "Origin".into(),
                ],
            ),
            (
                "features",
                "Features",
                part.features
                    .iter()
                    .map(|feature| feature.id.clone())
                    .collect(),
            ),
        ] {
            let id = format!("feature-tree:{}:{key}", part.id);
            for (id, role, section_label) in [
                (id.clone(), "feature-tree-group", label),
                (format!("{id}/heading"), "inspector-section-heading", label),
                (
                    format!("{id}/divider"),
                    "inspector-section-divider",
                    "Section divider",
                ),
            ] {
                out.push(cad_core::presentation::SemanticElement {
                    id, role: role.into(), label: section_label.into(), value: None,
                    state: "ready".into(), commands: vec![], relationships: vec![part.id.clone()],
                    data: serde_json::json!({"presentation_only":true,"classes":["inspector-section",key,match key { "bodies" => "body-outputs", "appearance" => "material-finish", "references" => "reference-geometry", _ => "model-features" }],"section":key,"items":items}),
                });
            }
        }
    }
    out.extend(crate::contours_ui::semantics(w));
    out.extend(crate::theme_library::semantics(w));
    out
}

pub fn confirmation_disabled_reason(w: &Workspace) -> Option<&'static str> {
    if crate::named_parameter_ui::active(w) {
        return Some("Add or cancel the named parameter first");
    }
    if !crate::component_fork_ui::current_source_is_valid(w)
        || !crate::feature_transfer_ui::current_source_is_valid(w)
    {
        return Some("Resolve the fork or feature transfer before applying");
    }
    if !crate::sketch3d_ui::current_source_is_valid(w) {
        return Some("Finish the current 3D sketch gesture and correct its fields before applying");
    }
    if !crate::quantity_display::source_fields_valid(w) {
        return Some("Correct the current field text before applying");
    }
    if !crate::document_identity_ui::current_source_is_valid(w) {
        return Some("Correct the document name before confirming");
    }
    if !crate::camera_views_ui::current_source_is_valid(w) {
        return Some("Correct the current named-view fields before applying");
    }
    if !crate::properties_ui::current_source_is_valid(w) {
        return Some("Correct the current property fields before applying");
    }
    if !crate::standard_parts_ui::current_source_is_valid(w) {
        return Some("Correct the current standard hardware fields before applying");
    }
    if !crate::tolerance_stacks_ui::current_source_is_valid(w) {
        return Some("Correct the current tolerance stack and wait for its report before applying");
    }
    if !crate::sensors_ui::current_source_is_valid(w) {
        return Some("Correct the current design monitor fields before applying");
    }
    if !crate::display_states_ui::current_source_is_valid(w) {
        return Some("Correct the current display state before applying");
    }
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
    if !crate::structural_member_ui::current_source_is_valid(w) {
        return Some(
            "Select sketch members and correct the current structural member fields before applying",
        );
    }
    if !crate::reference_repair_ui::current_source_is_valid(w) {
        return Some("Preview the current explicit reference repair before applying");
    }
    if w.confirm_requested {
        return Some("Waiting for the current operation to finish validation");
    }
    if let Some(sketch) = &w.sketch {
        if sketch.choosing_plane {
            return Some("Choose a plane or a flat face for this sketch");
        }
        if sketch.edit.is_some() {
            return Some("Finish or cancel the current sketch tool first");
        }
        if !sketch.clicks.is_empty() || sketch.tool.is_spline() && !sketch.chain.is_empty() {
            return Some("Finish placing the current curve or press Esc to end the tool");
        }
        if sketch.error.is_some() {
            return Some("Resolve the sketch error before finishing");
        }
        if !sketch.closed {
            return Some("Finish placing sketch geometry first");
        }
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
                evaluation.valid && evaluation.identity == crate::source_cache::identity(w)
            })
        {
            return Some("A current validated preview is required");
        }
    } else if !(w.fields.contains_key("exploded-view-editor")
        || crate::document_identity_ui::active(w)
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

/// One availability policy for menus, command search and user-composed controls.
/// These are prerequisites only; each operation still validates its actual input.
pub fn command_disabled_reason(w: &Workspace, command: &str) -> Option<&'static str> {
    if w.sketch
        .as_ref()
        .is_some_and(|sketch| sketch.edit.is_some())
        && (command.starts_with("sketch.tool.") || sketch::tool_for_command(command).is_some())
    {
        return Some("Finish or cancel the current sketch tool first");
    }
    if command == "presentation.navigation.history" && w.view.active_part.is_none() {
        return Some("Open a part to see its feature history");
    }
    if let Some(scope) = command.strip_prefix("presentation.graph-scope.")
        && let Some(reason) = crate::graph_ui::scope_reason(w, scope)
    {
        return Some(reason);
    }
    if crate::publish_ui::busy(w)
        && !matches!(command, "file.publish" | "publish.settings")
        && !command.starts_with("publish.")
    {
        return Some("Wait for Publish to finish or cancel it first");
    }
    if crate::git_ui::busy(w)
        && !command.starts_with("git-connections")
        && !command.starts_with("content-repositories")
        && !matches!(command, "repositories.cancel" | "repositories.auth-browser")
    {
        return Some("Wait for the Git operation to finish");
    }
    if crate::drawing_interaction_ui::active(w)
        && !matches!(command, "draft.cancel" | "operation.cancel" | "tool.cancel")
    {
        return Some("Finish or cancel the drawing gesture first");
    }
    if w.settings.is_some()
        && command != "settings"
        && !command.starts_with("settings.")
        && !command.starts_with("content-repositories")
        && !command.starts_with("git-connections")
    {
        return Some("Apply or cancel Settings first");
    }
    match command {
        "history.latest" => {
            return cad_core::history::latest_disabled_reason(
                &w.view,
                w.tool_checkpoint.is_some() || w.sketch.is_some(),
            );
        }
        "settings.apply" | "settings.ok" | "settings.cancel" if w.settings.is_none() => {
            return Some("Open Settings first");
        }
        "document.undo" => {
            if let Some(sketch) = &w.sketch {
                if sketch.history.is_empty()
                    && sketch.edit.is_none()
                    && sketch.drag.is_none()
                    && sketch.chain.is_empty()
                    && sketch.clicks.is_empty()
                {
                    return Some("No sketch edits to undo");
                }
            } else if crate::sketch3d_ui::active(w) {
                if !crate::sketch3d_ui::history_available(w, false) {
                    return Some("No 3D sketch edits to undo");
                }
            } else if w.tool_checkpoint.is_none() && !w.session.can_undo() {
                return Some("No changes to undo");
            }
        }
        "document.redo" => {
            if w.sketch.is_some() {
                return Some("Redo is unavailable while editing a planar sketch");
            }
            if crate::sketch3d_ui::active(w) {
                if !crate::sketch3d_ui::history_available(w, true) {
                    return Some("No 3D sketch edits to redo");
                }
            } else if w.tool_checkpoint.is_some() {
                return Some("Finish or cancel the current operation before using Redo");
            } else if !w.session.can_redo() {
                return Some("No changes to redo");
            }
        }
        "file.publish" if !publish_available(w) => {
            return Some(
                "Finish the current operation and wait for repository work before publishing",
            );
        }
        "draft.apply" => return confirmation_disabled_reason(w),
        "draft.cancel"
            if w.tool_checkpoint.is_none() && w.session.draft.is_none() && w.sketch.is_none() =>
        {
            return Some("No current operation to cancel");
        }
        "sketch.new" | "sketch.new-3d" if w.tool_checkpoint.is_some() || w.sketch.is_some() => {
            return Some("Finish or cancel the current operation before starting a new sketch");
        }
        "viewport.normal-sketch"
        | "viewport.fit-sketch"
        | "viewport.fit-support"
        | "viewport.fit-part-context"
        | "sketch.snap-entities"
            if w.sketch.is_none() =>
        {
            return Some("Open a planar sketch first");
        }
        "selection.measure" if w.selected.is_none() => {
            return Some("Select model geometry to measure");
        }
        "presentation.ghost-others" | "workspace.ghost-others" | "workspace.hide-others"
            if !crate::workspace_controls::in_place(w) || w.sketch.is_some() =>
        {
            return Some("Edit a component in place to change the surrounding components");
        }
        "workspace.finish-editing"
            if !crate::workspace_controls::in_place(w)
                || w.tool_checkpoint.is_some()
                || w.session.draft.is_some()
                || w.sketch.is_some() =>
        {
            return Some("Finish the current component edit first");
        }
        "assembly.isolate" if w.session.draft.is_some() => {
            return Some("Apply or cancel the current edit first");
        }
        "assembly.isolate" | "selection.isolate"
            if !w.has_assembly_context()
                || w.selected_component.as_ref().is_none_or(Vec::is_empty) =>
        {
            return Some("Select a component in the assembly to edit");
        }
        _ => {}
    }
    if crate::sketch3d_ui::active(w)
        && command.starts_with("sketch.")
        && !crate::sketch3d_ui::tool_supported(command)
    {
        return Some(
            "Use a supported 3D sketch tool, or finish this sketch before starting a planar tool",
        );
    }
    let assembly_command = matches!(
        command,
        "part.new"
            | "assembly.insert-existing"
            | "assembly.edit"
            | "assembly.mate-new"
            | "assembly.move"
            | "assembly.rotate"
            | "assembly.pattern"
            | "assembly.group-new"
            | "assembly.copy-with-mates"
            | "assembly.exploded-new"
            | "assembly.inspection-new"
            | "assembly.isolate"
            | "selection.isolate"
    );
    if assembly_command {
        let doc = w.session.visible();
        let assembly = w
            .fields
            .get("assembly-id")
            .and_then(|id| doc.assemblies.get(id))
            .or_else(|| doc.assemblies.get(&doc.root));
        let Some(assembly) = assembly else {
            return Some("Open or create an assembly first");
        };
        if w.tool_checkpoint.is_some() || w.sketch.is_some() {
            return Some("Finish or cancel the current operation before editing the assembly");
        }
        if !matches!(
            command,
            "part.new" | "assembly.insert-existing" | "assembly.edit"
        ) && assembly.instances.is_empty()
        {
            return Some("Insert a component into the assembly first");
        }
        if matches!(command, "assembly.group-new" | "assembly.inspection-new")
            && assembly.instances.len() < 2
        {
            return Some("Insert at least two components first");
        }
    }
    if let Some(tool) = cad_core::modeling_tools::TOOLS
        .iter()
        .find(|tool| tool.command == command)
    {
        if w.tool_checkpoint.is_some() && w.sketch.is_none() && !crate::sketch3d_ui::active(w) {
            return Some("Finish or cancel the current operation before starting another feature");
        }
        let part = w
            .view
            .active_part
            .as_ref()
            .and_then(|id| w.session.visible().parts.get(id));
        if part.is_none() && tool.preset != "Weld Bead" {
            return Some("Create or select a part first");
        }
        if tool.requires_body
            && !part.is_some_and(|part| {
                cad_core::configuration::has_solid_output(w.session.visible(), &part.id)
            })
        {
            return Some("Create a solid body in the active part first");
        }
    }
    None
}

fn publish_available(w: &Workspace) -> bool {
    crate::publish_ui::busy(w)
        || (!crate::git_ui::operation_pending(w)
            && !crate::repositories_ui::operation_pending(w)
            && !crate::readme_preview_ui::busy(w)
            && !crate::drawing_interaction_ui::active(w)
            && w.settings.is_none()
            && w.tool_checkpoint.is_none()
            && w.session.draft.is_none()
            && w.sketch.is_none())
}

pub fn menu(
    ui: &mut egui::Ui,
    w: &mut Workspace,
    snapshot: &[cad_core::presentation::SemanticElement],
) {
    let viewport_width = ui.available_width();
    egui::ScrollArea::horizontal()
        .id_salt("cad-main-menu-overflow")
        .auto_shrink([false, true])
        .show(ui, |ui| { ui.horizontal(|ui| {
        let row_start = ui.cursor().min.x;
        ui.label(RichText::new("TCAD").strong());
        for id in ["menu/file", "menu/edit", "menu/insert"] {
            bound_menu(ui, w, snapshot, id);
        }
        menu_button(ui, "View", |ui| {
            crate::theme_library::menu(ui, w, "view.themes");
            ui.separator();
            view_controls(ui, w);
            crate::camera_views_ui::view_controls(ui, w);
            crate::sketch_overlay::view_controls(ui, w);
        });
        if let Some(tools) = snapshot.iter().find(|entry| entry.id == "menu/tools") {
            semantic_menu_as(ui, w, tools, &tools.id, "menu", snapshot);
        }
        menu_button(ui, "Workspace", |ui| {
            crate::theme_library::menu(ui, w, "workspace.themes");
            ui.separator();
            for (label, command) in [
                ("Advanced Layout and Style…", "presentation"),
                ("Versions…", "versions"),
                ("Git Repositories…", "repositories"),
                ("Git Connections…", "git-connections"),
                ("Content Libraries…", "content-repositories"),
                ("Update README Preview", "readme.preview"),
                ("Review Comments…", "comments"),
                ("Rebuild", "document.rebuild"),
                ("Source Definitions…", "source"),
                ("Create Release…", "file.release"),
            ] {
                if menu_action(ui, w, command, label, true).clicked() {
                    ui::action(w, command);
                    ui.close();
                }
            }
        });
        ui.separator();
        let dirty = crate::document_identity_ui::has_unsaved_changes(w);
        let title_width = (viewport_width * 0.25).clamp(80., 240.);
        ui.add_sized(egui::vec2(title_width, ui.spacing().interact_size.y), egui::Label::new(
            RichText::new(format!(
                "{}{}",
                w.session.visible().name,
                if dirty { " *" } else { "" }
            ))
            .strong(),
        ).truncate()).on_hover_text(&w.session.visible().name);
        // Keep the actions at the far end on a wide workspace, while allowing
        // the complete native menu row to scroll on small/high-scale windows.
        let actions_id = ui.make_persistent_id("cad-main-menu-actions-width");
        let actions_width = ui.data(|data| data.get_temp::<f32>(actions_id)).unwrap_or(260.);
        ui.add_space((viewport_width - (ui.cursor().min.x - row_start) - actions_width).max(0.));
        let actions = ui.horizontal(|ui| {
            let search_hint = shortcut_text(ui.ctx(), "commands").map(|keys| format!("Find a command ({keys})")).unwrap_or_else(|| "Find a command".into());
            if tool_button(ui, "commands", "Find a command", command_disabled_reason(w,"commands"), false, &search_hint) {
                ui::action(w, "commands");
            }
            if command_button(
                ui,
                w,
                "file.publish",
                if crate::publish_ui::busy(w) { "Publishing…" } else { "Publish" },
                publish_available(w),
            )
            .on_hover_text("Generate the selected outputs, commit the project, and push to its saved repository and branch")
            .clicked()
            {
                ui::action(w, "file.publish");
            }
        });
        ui.data_mut(|data| data.insert_temp(actions_id, actions.response.rect.width()));
    });
    });
}
pub fn commands(ui: &mut egui::Ui, w: &mut Workspace, snapshot: &[SemanticElement]) {
    let viewport_width = ui.available_width();
    egui::ScrollArea::horizontal()
        .id_salt("cad-command-tabs-overflow")
        .auto_shrink([false, true])
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let row_start = ui.cursor().min.x;
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
                ui.add_space((viewport_width - (ui.cursor().min.x - row_start) - 70.).max(0.));
                ui.horizontal(|ui| {
                    menu_button(ui, "Size", |ui| {
                        if menu_action(ui, w, "presentation.compact", "Compact", true).clicked() {
                            w.view.ui_scale = 1.;
                            w.view.toolbar_height = 68.;
                            resize_toolbar(w);
                            crate::settings::save_live_application(w);
                            ui.close();
                        }
                        if menu_action(ui, w, "presentation.comfortable", "Comfortable", true)
                            .clicked()
                        {
                            w.view.ui_scale = 1.15;
                            w.view.toolbar_height = 80.;
                            resize_toolbar(w);
                            crate::settings::save_live_application(w);
                            ui.close();
                        }
                        if ui
                            .add(
                                egui::Slider::new(&mut w.view.ui_scale, 0.75..=1.5)
                                    .text("UI scale"),
                            )
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
        });
    let sketching = w.sketch.is_some();
    let tab = w.command_tab.clone();
    if tab == "Sketch" && crate::sketch3d_ui::active(w) {
        crate::sketch3d_ui::toolbar(ui, w);
        return;
    }
    egui::ScrollArea::horizontal()
        .id_salt("cad-tool-groups")
        .auto_shrink([false, true])
        .show(ui, |ui| {
            ui.horizontal(|ui| match tab.as_str() {
                "Sketch" => {
                    if !sketching {
                        for (command, label, hint) in [
                            (
                                "sketch.new",
                                "New sketch",
                                "Sketch on a plane or a flat face",
                            ),
                            (
                                "sketch.new-3d",
                                "3D sketch",
                                "Sketch points and curves in three dimensions",
                            ),
                        ] {
                            if tool_button(
                                ui,
                                command,
                                label,
                                command_disabled_reason(w, command),
                                false,
                                hint,
                            ) {
                                ui::action(w, command);
                            }
                        }
                        return;
                    }
                    if w.sketch
                        .as_ref()
                        .is_some_and(|sketch| sketch.choosing_plane)
                    {
                        controls::label(
                            ui,
                            "workspace.sketch-start-description",
                            "workspace-description",
                            &[],
                            "Sketch plane",
                            "required",
                        );
                        for plane in snapshot
                            .iter()
                            .filter(|entry| entry.role == "sketch-plane-action")
                        {
                            if crate::sketch_ui::plane_button(
                                ui,
                                &plane.id,
                                &plane.role,
                                &["toolbar-control", "plane-choice"],
                                &plane.label,
                            )
                            .clicked()
                            {
                                ui::semantic_action(w, plane, "sketch.select-entity");
                            }
                        }
                        return;
                    }
                    let dimensioning = w.fields.contains_key("smart-dimension-mode");
                    for (tool, label, hint) in [
                        (
                            sketch::Tool::Select,
                            "Select",
                            "Select or move sketch geometry",
                        ),
                        (
                            sketch::Tool::Polyline,
                            "Line",
                            "Pick connected line endpoints",
                        ),
                        (
                            sketch::Tool::Rectangle,
                            "Rectangle",
                            "Pick opposite corners",
                        ),
                        (
                            sketch::Tool::Circle,
                            "Circle",
                            "Pick the center and a point on the circle",
                        ),
                    ] {
                        let command = sketch_tool_command(tool);
                        if tool_button(
                            ui,
                            &command,
                            label,
                            command_disabled_reason(w, &command),
                            !dimensioning && w.sketch.as_ref().is_some_and(|s| s.tool == tool),
                            hint,
                        ) {
                            ui::action(w, &command);
                        }
                    }
                    if tool_button(
                        ui,
                        "sketch.smart-dimension",
                        "Smart Dimension",
                        command_disabled_reason(w, "sketch.smart-dimension"),
                        dimensioning,
                        "Pick geometry to dimension, or dimension your current selection (D)",
                    ) {
                        ui::action(w, "sketch.smart-dimension");
                    }
                    if tool_button(
                        ui,
                        "draft.apply",
                        "Finish sketch",
                        confirmation_disabled_reason(w),
                        false,
                        "Save this sketch and return to the model",
                    ) {
                        ui::action(w, "draft.apply");
                    }
                    ui.separator();
                    for id in [
                        "menu/draw-geometry",
                        "menu/edit-geometry",
                        "menu/dimensions-and-relations",
                    ] {
                        bound_menu(ui, w, snapshot, id);
                    }
                }
                "Graph" => {
                    for scope in ["Features", "Assembly"] {
                        let reason = crate::graph_ui::scope_reason(w, scope);
                        if controls::button(
                            ui,
                            &format!("graph-scope/{}", scope.to_lowercase()),
                            "graph-scope",
                            &["selectable"],
                            scope,
                            reason.is_none(),
                            crate::graph_ui::scope(w) == scope,
                        )
                        .on_disabled_hover_text(reason.unwrap_or_default())
                        .clicked()
                        {
                            ui::action(
                                w,
                                &format!("presentation.graph-scope.{}", scope.to_lowercase()),
                            );
                        }
                    }
                    if command_button(ui, w, "graph.arrange", "Arrange Graph", true).clicked() {
                        ui::action(w, "graph.arrange");
                    }
                    crate::workspace_controls::graph_gestures(ui);
                }
                "Assembly" => {
                    for (label, cmd) in [
                        ("New Part in Assembly", "part.new"),
                        ("Insert Existing", "assembly.insert-existing"),
                        ("Standard Hardware", "standard-parts"),
                        ("Move", "assembly.move"),
                        ("Mate", "assembly.mate-new"),
                        ("Components", "assembly.edit"),
                        ("Library", "libraries"),
                    ] {
                        if command_button(ui, w, cmd, label, true).clicked() {
                            ui::action(w, cmd);
                        }
                    }
                    if crate::workspace_controls::in_place(w) {
                        if command_button(
                            ui,
                            w,
                            "workspace.finish-editing",
                            "Finish editing",
                            w.tool_checkpoint.is_none(),
                        )
                        .clicked()
                        {
                            ui::action(w, "workspace.finish-editing");
                        }
                        crate::workspace_controls::component_visibility(ui, w);
                    } else if command_button(ui, w, "assembly.isolate", "Edit in place", true)
                        .clicked()
                    {
                        ui::action(w, "assembly.isolate");
                    }
                }
                "Evaluate" => {
                    if command_button(ui, w, "selection.measure", "Measure", w.selected.is_some())
                        .clicked()
                    {
                        ui::action(w, "selection.measure");
                    }
                    for (label, cmd) in [
                        ("Mass Properties", "inspection.properties"),
                        ("Zoom to Fit", "viewport.fit"),
                        ("Rebuild", "document.rebuild"),
                        ("BOM", "bom"),
                        ("Drawings", "drawings"),
                    ] {
                        if command_button(ui, w, cmd, label, true).clicked() {
                            ui::action(w, cmd);
                        }
                    }
                }
                "Drawing" => {
                    for (label, command) in [
                        ("New Sheet", "drawing.sheet-add"),
                        ("Model", "drawing.workspace-model"),
                    ] {
                        if command_button(ui, w, command, label, true).clicked() {
                            ui::action(w, command);
                        }
                    }
                }
                _ => {
                    for (command, label, hint) in [
                        (
                            "sketch.new",
                            "Sketch",
                            "Start a sketch on a plane or flat face",
                        ),
                        (
                            "feature.extrude",
                            "Extrude",
                            "Add material from a sketch profile",
                        ),
                        (
                            "feature.cut",
                            "Cut",
                            "Remove material with a sketch profile",
                        ),
                        ("feature.fillet", "Fillet", "Round selected edges"),
                    ] {
                        if tool_button(
                            ui,
                            command,
                            label,
                            command_disabled_reason(w, command),
                            false,
                            hint,
                        ) {
                            ui::action(w, command);
                        }
                    }
                    ui.separator();
                    bound_menu(ui, w, snapshot, "menu/all-features");
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
            let command = format!("presentation.navigation.{}", mode.to_lowercase());
            let reason = command_disabled_reason(w, &command);
            if controls::button(
                ui,
                &format!("navigation/{}", mode.to_lowercase()),
                "navigation-action",
                &["selectable"],
                label,
                reason.is_none(),
                w.manager_tree && w.view.mode == mode,
            )
            .on_hover_text(match mode {
                "History" => "Inspect the model after each feature",
                "Flow" => "View feature and component relationships",
                _ => "Show the current model tree",
            })
            .on_disabled_hover_text(reason.unwrap_or_default())
            .clicked()
            {
                presentation_action(w, &command);
            }
        }
        if controls::icon_button(
            ui,
            "navigation/presentation",
            "command-action",
            &["navigation-control", "icon-action"],
            "Edit layout and style",
            true,
            false,
            Some(&|p, center, size, color| {
                crate::command_icons::paint(p, center, size, color, "settings")
            }),
        )
        .on_hover_text("Layout, theme and sizing")
        .clicked()
        {
            ui::action(w, "presentation");
        }
    });
}
pub fn tree(ui: &mut egui::Ui, w: &mut Workspace, elements: &[SemanticElement]) {
    tree_contents(ui, w, elements, false);
}
/// Independently placed browsers keep the tree visible beside a task editor.
pub fn model_browser(ui: &mut egui::Ui, w: &mut Workspace, elements: &[SemanticElement]) {
    if crate::drawing_ui::is_workspace(w) {
        crate::drawing_ui::tree(ui, w);
    } else {
        tree_contents(ui, w, elements, true);
    }
}
fn tree_contents(
    ui: &mut egui::Ui,
    w: &mut Workspace,
    elements: &[SemanticElement],
    persistent: bool,
) {
    let task_active =
        w.tool_checkpoint.is_some() || w.sketch.is_some() || w.session.draft.is_some();
    if w.editing_manager() && !persistent {
        let open = crate::workspace_controls::history_open(w);
        if controls::button(
            ui,
            "workspace.task-history",
            "task-section-toggle",
            &["secondary-control"],
            &format!(
                "{} {}",
                if open { "▾" } else { "▸" },
                crate::history::browser_label(w)
            ),
            true,
            open,
        )
        .clicked()
        {
            crate::workspace_controls::action(w, "workspace.task-history");
        }
        return;
    }
    let doc = w.session.visible().clone();
    if doc.parts.len() > 1 || (w.view.active_part.is_none() && !doc.parts.is_empty()) {
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
    if w.view.mode == "Flow" && w.view.active_part.is_none() {
        crate::graph_ui::show(ui, w, None);
        return;
    }
    let Some(part) = w.view.active_part.as_ref().and_then(|id| doc.parts.get(id)) else {
        egui::ScrollArea::vertical().show(ui, |ui| {
            let response = ui.add(
                egui::Label::new(RichText::new(&doc.name).strong()).sense(egui::Sense::click()),
            );
            if response.clicked() && !task_active {
                crate::selection_ui::object_selected(
                    w,
                    cad_core::selection_actions::SelectionTarget::Object {
                        definition: doc.root.clone(),
                        object: doc.root.clone(),
                        role: "assembly".into(),
                    },
                );
            }
            controls::inspector_section(
                ui,
                controls::InspectorSection {
                    id: &format!("feature-tree:{}:components", doc.root),
                    role: "feature-tree-group",
                    classes: &["components"],
                    label: "Components",
                    state: "ready",
                    collapse: Some(true),
                    open: None,
                },
                |ui| {
                    crate::assembly_tree_ui::show(ui, w);
                    if command_button(
                        ui,
                        w,
                        "assembly.insert-existing",
                        "Insert component…",
                        !task_active,
                    )
                    .clicked()
                    {
                        ui::action(w, "assembly.insert-existing");
                    }
                },
            );
            if doc.assemblies.values().any(|assembly| {
                !assembly.rigid_groups.is_empty() || !assembly.weld_beads.is_empty()
            }) {
                controls::inspector_section(
                    ui,
                    controls::InspectorSection {
                        id: &format!("feature-tree:{}:connections", doc.root),
                        role: "feature-tree-group",
                        classes: &["assembly-connections"],
                        label: "Groups and welds",
                        state: "ready",
                        collapse: Some(true),
                        open: None,
                    },
                    |ui| {
                        crate::assembly_groups_ui::tree(ui, w);
                        crate::assembly_welds_ui::tree(ui, w);
                    },
                );
            }
        });
        return;
    };
    // A standalone part also has an internal assembly that places it once.
    // Only offer assembly navigation when there is an actual assembly to edit.
    if w.has_assembly_context()
        && controls::button(
            ui,
            "feature-tree/assembly-root",
            "assembly-breadcrumb",
            &["feature-tree-row"],
            &format!("← {}", doc.name),
            w.tool_checkpoint.is_none() && w.session.draft.is_none(),
            false,
        )
        .clicked()
    {
        w.leave_part_context();
        return;
    }
    if w.view.mode == "Flow" {
        crate::ui::graph(ui, w, part);
        return;
    }
    if w.view.mode == "History" {
        history(ui, w, part, elements);
        return;
    }
    egui::ScrollArea::vertical()
        .id_salt("cad-feature-tree")
        .show(ui, |ui| {
            crate::feature_order_ui::scroll(ui);
            let response = ui.add(
                egui::Label::new(RichText::new(&part.name).strong()).sense(egui::Sense::click()),
            );
            if response.clicked() && !task_active {
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
                crate::assembly_tree_ui::features::show(ui, w, part);
                controls::inspector_section(
                    ui,
                    controls::InspectorSection {
                        id: &format!("feature-tree:{}:references", part.id),
                        role: "feature-tree-group",
                        classes: &["reference-geometry"],
                        label: "Planes and origin",
                        state: "ready",
                        collapse: Some(part.features.is_empty()),
                        open: None,
                    },
                    |ui| {
                        for name in sketch::ORIGIN_PLANES {
                            let choosing_plane = w
                                .sketch
                                .as_ref()
                                .is_some_and(|sketch| sketch.choosing_plane);
                            let can_choose_plane = !task_active || choosing_plane;
                            let response = controls::icon_button(
                                ui,
                                &format!("reference:{}::{name}", part.id),
                                "reference-plane",
                                &["feature-tree-row"],
                                name,
                                can_choose_plane,
                                w.active_plane.as_deref() == Some(name),
                                Some(&|p, center, size, color| crate::command_icons::paint(p, center, size, color, crate::sketch_ui::plane_icon(name))),
                            );
                            if response.clicked() {
                                sketch::choose_origin(w, name);
                                if !choosing_plane && !response.double_clicked() {
                                    w.status = format!("{name} selected");
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
                            response.on_hover_text(if choosing_plane {
                                "Use this plane for the sketch"
                            } else {
                                "Click to select. Double-click to sketch on this plane."
                            }).context_menu(|ui| {
                                if ui
                                    .add_enabled(
                                        can_choose_plane,
                                        egui::Button::new("Sketch on this plane"),
                                    )
                                    .clicked()
                                {
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
                            !task_active,
                            matches!(&w.context_selection, Some(cad_core::selection_actions::SelectionTarget::Reference { definition, reference }) if definition == &part.id && reference == "Origin"),
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
                    },
                );
                let body_counts = crate::body_inventory::counts(w, &part.id);
                let has_bodies = body_counts
                    .is_some_and(|counts| counts.solids + counts.surfaces > 0)
                    || effective.outputs.iter().any(|id| {
                        part.features.iter().any(|feature| {
                            &feature.id == id
                                && (feature.operation.produces_solid()
                                    || feature.operation.is_surface())
                        })
                    });
                if has_bodies {
                    controls::inspector_section(
                        ui,
                        controls::InspectorSection {
                            id: &format!("feature-tree:{}:bodies", part.id),
                            role: "feature-tree-group",
                            classes: &["body-outputs"],
                            label: "Bodies",
                            state: "ready",
                            collapse: Some(false),
                            open: None,
                        },
                        |ui| {
                            for (label, is_surface) in
                                [("Solid Bodies", false), ("Surface Bodies", true)]
                            {
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
                                let count = body_counts.map(|counts| {
                                    if is_surface {
                                        counts.surfaces
                                    } else {
                                        counts.solids
                                    }
                                });
                                let heading = count
                                    .map(|count| format!("{label} ({count})"))
                                    .unwrap_or_else(|| {
                                        format!(
                                            "{} outputs ({})",
                                            if is_surface { "Surface" } else { "Solid" },
                                            outputs.len()
                                        )
                                    });
                                if count.is_some_and(|count| count > 0)
                                    || !outputs.is_empty()
                                {
                                    egui::CollapsingHeader::new(heading)
                                        .id_salt(label)
                                        .default_open(false)
                                        .show(ui, |ui| {
                                            ui.small("Grouped by output feature");
                                            for feature in outputs {
                                                feature_row(ui, w, part, feature);
                                            }
                                        });
                                }
                            }
                        },
                    );
                }
                controls::inspector_section(
                    ui,
                    controls::InspectorSection {
                        id: &format!("feature-tree:{}:appearance", part.id),
                        role: "feature-tree-group",
                        classes: &["material-finish"],
                        label: "Material and finish",
                        state: "ready",
                        collapse: Some(false),
                        open: None,
                    },
                    |ui| {
                        let material = doc
                            .materials
                            .get(&effective.material)
                            .map(|m| m.name.as_str())
                            .unwrap_or("Unassigned");
                        let response = controls::button(
                            ui,
                            &format!("{}::material", part.id),
                            "material-assignment",
                            &["feature-tree-row"],
                            &format!("Material: {material}"),
                            w.tool_checkpoint.is_none(),
                            false,
                        );
                        if response.clicked() {
                            crate::selection_ui::object_selected(
                                w,
                                cad_core::selection_actions::SelectionTarget::Object {
                                    definition: part.id.clone(),
                                    object: effective.material.clone(),
                                    role: "material".into(),
                                },
                            );
                            w.view.active_part = Some(part.id.clone());
                            crate::materials_ui::begin(w);
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
                    },
                );
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
fn history(ui: &mut egui::Ui, w: &mut Workspace, part: &Part, elements: &[SemanticElement]) {
    let picking = w.tool_checkpoint.is_some() || w.sketch.is_some();
    controls::inspector_section(
        ui,
        controls::InspectorSection {
            id: &format!("feature-tree:{}:history", part.id),
            role: "feature-tree-group",
            classes: &["feature-history"],
            label: if picking {
                "Model features"
            } else {
                "Feature history"
            },
            state: "ready",
            collapse: None,
            open: None,
        },
        |ui| {
            if picking {
                model_features(ui, w, part);
            } else {
                crate::history::return_control(ui, w, "history-view", "history-view");
                egui::ScrollArea::vertical()
                    .id_salt(("feature-history", &part.id))
                    .show(ui, |ui| {
                        crate::history::stations(
                            ui,
                            w,
                            elements,
                            crate::history::StationLayout::List,
                        );
                    });
            }
        },
    );
}

fn model_features(ui: &mut egui::Ui, w: &mut Workspace, part: &Part) {
    egui::ScrollArea::vertical()
        .id_salt(("source-features", &part.id))
        .show(ui, |ui| {
            if part.features.is_empty() {
                crate::history::empty(ui);
            }
            for feature in &part.features {
                feature_row(ui, w, part, feature);
            }
        });
}
pub(crate) fn feature_row(ui: &mut egui::Ui, w: &mut Workspace, part: &Part, feature: &Feature) {
    let suppressed =
        cad_core::configuration::feature_suppressed(w.session.visible(), &part.id, feature);
    let editing = w
        .sketch
        .as_ref()
        .is_some_and(|sketch| sketch.feature == feature.id);
    let selected = match &w.context_selection {
        Some(target) => target.definition() == part.id && target.feature() == feature.id,
        None => editing || w.view.active_node.as_ref() == Some(&feature.id),
    };
    let response = controls::button(
        ui,
        &feature.id,
        "feature-station",
        &["feature-tree-row"],
        &format!(
            "{}  {}{}",
            if suppressed {
                "○"
            } else if matches!(
                feature.operation,
                Operation::Profile { .. } | Operation::Sketch3d { .. }
            ) {
                "▱"
            } else {
                "◇"
            },
            feature.name,
            if editing { " (Editing)" } else { "" }
        ),
        true,
        selected,
    );
    let response = crate::feature_order_ui::row(ui, w, part, &feature.id, response);
    let response = response.on_hover_text(format!(
        "{} · {}{}{}",
        feature.name,
        feature.operation.name(),
        if suppressed { " (suppressed)" } else { "" },
        if w.tool_checkpoint.is_none() && w.sketch.is_none() {
            "\nDouble-click to edit. Right-click for more actions."
        } else {
            ""
        }
    ));
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
    controls::inspector_section(
        ui,
        controls::InspectorSection {
            id: "inspector/selection",
            role: "inspector-section",
            classes: &["selection-summary"],
            label: "Selected item",
            state: "ready",
            collapse: None,
            open: None,
        },
        |ui| {
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
                        egui::Button::new(
                            if matches!(
                                feature.operation,
                                Operation::Profile { .. } | Operation::Sketch3d { .. }
                            ) {
                                "Edit Sketch"
                            } else {
                                "Edit Feature"
                            },
                        ),
                    )
                    .on_disabled_hover_text(
                        "Apply or cancel appearance changes before editing a feature",
                    )
                    .clicked()
                {
                    ui::feature_dialog(w, Some(feature.clone()));
                }
            } else {
                ui.label("Select a plane or model feature.");
                ui.small("Double-click a sketch or feature in the tree to edit it.");
            }
        },
    );
    let open_parameters = w.fields.remove("graph-parameters").is_some();
    controls::inspector_section(
        ui,
        controls::InspectorSection {
            id: "inspector/parameters",
            role: "inspector-section",
            classes: &["parameters", "secondary"],
            label: "Named parameters",
            state: "ready",
            collapse: Some(false),
            open: open_parameters.then_some(true),
        },
        |ui| crate::ui::parameters(ui, w),
    );
}
pub fn confirmation_label(w: &Workspace) -> String {
    if crate::document_identity_ui::active(w) {
        return crate::document_identity_ui::confirmation_label(w).into();
    }
    if w.sketch.is_some() || crate::sketch3d_ui::active(w) {
        return "Finish sketch".into();
    }
    if w.dialog.as_deref() == Some("component-insert")
        || w.fields.contains_key("standard-parts-tool")
    {
        return "Insert".into();
    }
    if w.dialog.as_deref() == Some("feature")
        || w.assembly_motion.is_some()
        || w.fields.contains_key("mate-editor")
        || w.fields.contains_key("component-pattern-editor")
    {
        "OK".into()
    } else {
        "Apply".into()
    }
}

pub fn confirmation(ui: &mut egui::Ui, w: &mut Workspace) {
    crate::operation_preview::status_control(ui, w);
    ui.horizontal_wrapped(|ui| completion_buttons(ui, w));
}

fn completion_buttons(ui: &mut egui::Ui, w: &mut Workspace) {
    let label = confirmation_label(w);
    let reason = command_disabled_reason(w, "draft.apply");
    if controls::button(
        ui,
        "draft.apply",
        "apply-action",
        &["confirmation-control"],
        &label,
        reason.is_none(),
        false,
    )
    .on_hover_text("Finish this operation")
    .on_disabled_hover_text(reason.unwrap_or("Finish the required inputs first"))
    .clicked()
    {
        ui::action(w, "draft.apply");
    }
    let cancel_reason = command_disabled_reason(w, "draft.cancel");
    if controls::button(
        ui,
        "draft.cancel",
        "cancel-action",
        &["confirmation-control"],
        "Cancel",
        cancel_reason.is_none(),
        false,
    )
    .on_disabled_hover_text(cancel_reason.unwrap_or_default())
    .clicked()
    {
        ui::action(w, "draft.cancel");
    }
}

/// The viewport menu and the property manager commit the same validated draft.
pub(crate) fn viewport_menu(ui: &mut egui::Ui, w: &mut Workspace) {
    if w.tool_checkpoint.is_some() {
        for (command, label) in [
            ("draft.apply", confirmation_label(w)),
            ("draft.cancel", "Cancel".into()),
        ] {
            if menu_action(ui, w, command, &label, true).clicked() {
                ui.close();
                ui::action(w, command);
                return;
            }
        }
        ui.separator();
    } else if let Some(target) = crate::selection_ui::current(w) {
        if crate::selection_ui::contents(ui, w, &target) {
            return;
        }
        ui.separator();
    }
    if menu_action(ui, w, "viewport.fit", "Zoom to Fit", true).clicked() {
        ui.close();
        ui::action(w, "viewport.fit");
    }
}

pub(crate) fn task_history(ui: &mut egui::Ui, w: &mut Workspace) {
    if !crate::workspace_controls::history_open(w) {
        return;
    }
    controls::inspector_section(
        ui,
        controls::InspectorSection {
            id: "workspace.task-history-panel",
            role: "task-history-panel",
            classes: &["secondary-section"],
            label: crate::history::browser_label(w),
            state: "open",
            collapse: None,
            open: None,
        },
        |ui| {
            let part = w
                .view
                .active_part
                .as_ref()
                .and_then(|owner| w.session.visible().parts.get(owner))
                .cloned();
            if let Some(part) = part {
                model_features(ui, w, &part);
            } else {
                crate::assembly_tree_ui::show(ui, w);
            }
        },
    );
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
pub(crate) fn sweep_path_selector(
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
                                Operation::Profile { .. }
                                    | Operation::ReferenceCurve { .. }
                                    | Operation::Sketch3d { .. }
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
    let mut changed = false;
    let selected = match mode.as_str() {
        "" | "new" => "New body",
        "cut" | "difference" => "Cut",
        "join" | "union" => "Join",
        "intersect" | "intersection" => "Intersect",
        other => other,
    };
    let choice = egui::ComboBox::from_label("Result")
        .selected_text(selected)
        .show_ui(ui, |ui| {
            for (value, label) in [
                ("new", "New body"),
                ("join", "Join"),
                ("cut", "Cut"),
                ("intersect", "Intersect"),
            ] {
                let available = value == "new" || !candidates.is_empty();
                let response = ui
                    .add_enabled(
                        available,
                        egui::Button::selectable(selected == label, label),
                    )
                    .on_disabled_hover_text("Create a solid body first.");
                if response.clicked() && selected != label {
                    return Some(value);
                }
            }
            None
        })
        .inner
        .flatten();
    if let Some(value) = choice {
        *mode = value.into();
        changed = true;
    }
    if changed && body.is_none() && mode != "new" {
        // Prefer the current part when a user changes the result. Simply
        // inspecting a feature must never assign a different input body.
        let owner = doc
            .parts
            .values()
            .find(|part| part.features.iter().any(|f| f.id == current))
            .map(|part| &part.id);
        *body = candidates
            .iter()
            .rev()
            .find(|(reference, _)| Some(&reference.definition) == owner)
            .or_else(|| (candidates.len() == 1).then(|| &candidates[0]))
            .map(|(reference, _)| reference.clone());
    }
    if mode != "new" && !mode.is_empty() {
        let label = body
            .as_ref()
            .and_then(|r| candidates.iter().find(|(candidate, _)| candidate == r))
            .map(|(_, name)| name.as_str())
            .unwrap_or("Select target body");
        egui::ComboBox::from_label("Target body")
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
pub fn feature_fields(ui: &mut egui::Ui, w: &mut Workspace, feature: &mut Feature) -> bool {
    if matches!(feature.operation, Operation::ReferenceCurve { .. }) {
        return crate::reference_curves_ui::fields(ui, w, feature);
    }
    if feature.operation.is_reference() {
        return crate::reference_ui::fields(ui, w, feature);
    }
    if matches!(
        feature.operation,
        Operation::Extrude { .. } | Operation::Revolve { .. }
    ) {
        return profile_features::fields(ui, w, w.session.visible(), feature);
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
        Operation::Fillet {
            body,
            edges,
            radius,
        } => {
            ui.label(format!("Edges to fillet: {}", edges.len()));
            changed |= selected_edges(ui, w, &feature.id, body, edges);
            changed |= expression(ui, "Radius", radius);
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
            changed |= crate::component_pattern_ui::part_direction_controls(
                ui,
                w,
                &doc,
                &feature.id,
                "Direction 1",
                step,
                &mut options.direction,
            );
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
                changed |= crate::component_pattern_ui::part_direction_controls(
                    ui,
                    w,
                    &doc,
                    &feature.id,
                    "Direction 2",
                    &mut second.step,
                    &mut second.direction,
                );
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
    crate::sketch_overlay::visibility_controls(ui, w);
    crate::display_states_ui::view_controls(ui, w);
    crate::exploded_views_ui::view_controls(ui, w);
    controls::menu_button(ui, ORIENTATION_MENU.0, ORIENTATION_MENU.1, |ui| {
        for &command in ORIENTATION_MENU.2 {
            let name = command.trim_start_matches("viewport.orientation.");
            let mut label = name.to_string();
            label[..1].make_ascii_uppercase();
            if menu_action(ui, w, command, &label, true).clicked() {
                ui::action(w, command);
                ui.close();
            }
        }
    });
    ui.separator();
    if menu_action(ui, w, "viewport.fit", "Zoom to Fit", true).clicked() {
        ui::action(w, "viewport.fit");
        ui.close();
    }
    if menu_action(ui, w, "viewport.normal-sketch", "Normal To Sketch", true).clicked() {
        ui::action(w, "viewport.normal-sketch");
        ui.close();
    }
}
pub fn status(ui: &mut egui::Ui, w: &mut Workspace) {
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        let units = crate::workspace_controls::display_units(w);
        let reason = command_disabled_reason(w, "settings.model");
        if controls::button(
            ui,
            "workspace.model-units",
            "workspace-action",
            &["status-control"],
            &units,
            reason.is_none(),
            false,
        )
        .on_hover_text("Model units and precision")
        .on_disabled_hover_text(reason.unwrap_or_default())
        .clicked()
        {
            ui::action(w, "settings.model");
        }
        controls::label(
            ui,
            "workspace.status-mode",
            "status-mode",
            &["status-control"],
            crate::workspace_controls::status_mode(w),
            "ready",
        );
        ui.separator();
        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
            crate::document_identity_ui::location_control(ui, w);
            ui.separator();
            if w.session.draft.as_ref().is_some_and(|d| d.pending) {
                ui.spinner();
            }
            if w.tool_checkpoint.is_none()
                && let Some((state, label)) = crate::operation_preview::summary(w)
            {
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
                completion_buttons(ui, w);
            }
            controls::bounded_label(
                ui,
                "workspace.status-message",
                "status-message",
                &[],
                &w.status,
                "ready",
                ui.available_width(),
            )
            .on_hover_text(&w.status);
        });
    });
}
