//! Presentation-only sketch/reference visibility and bounded display occlusion.
use crate::{Workspace, presentation_controls as controls, viewport};
use bevy_egui::egui;
use cad_core::presentation::{OverlayVisibility, SemanticElement};
use std::collections::BTreeMap;

pub const CONTRACT_VERSION: u32 = 1;
pub const DEPTH_WORK: usize = 240_000;
const LAYERS: &[(&str, &str)] = &[
    ("sketch-lines", "Sketch lines and curves"),
    ("construction", "Construction geometry"),
    ("sketch-points", "Sketch points and handles"),
    ("sketch-midpoints", "Sketch line midpoints"),
    ("dimensions", "Sketch dimensions"),
    ("relations", "Sketch relations"),
    ("planes", "Planes"),
    ("axes", "Axes"),
    ("origins", "Origins"),
    ("reference-points", "Reference points"),
    ("sketch-grid", "Sketch grid"),
    ("centerlines", "Circular center guides on hover"),
    ("edge-midpoints", "Edge midpoint guides on hover"),
];
fn layer_mut<'a>(w: &'a mut Workspace, layer: &str) -> Option<&'a mut bool> {
    let visibility = &mut w.view.visibility;
    Some(match layer {
        "sketch-lines" => &mut visibility.sketch_lines,
        "construction" => &mut visibility.construction,
        "sketch-points" => &mut visibility.sketch_points,
        "sketch-midpoints" => &mut visibility.sketch_midpoints,
        "dimensions" => &mut w.view.sketch_annotation_visibility[0],
        "relations" => &mut w.view.sketch_annotation_visibility[1],
        "planes" => &mut visibility.planes,
        "axes" => &mut visibility.axes,
        "origins" => &mut visibility.origins,
        "reference-points" => &mut visibility.reference_points,
        "sketch-grid" => &mut visibility.sketch_grid,
        "centerlines" => &mut visibility.centerlines,
        "edge-midpoints" => &mut visibility.edge_midpoints,
        _ => return None,
    })
}
fn layer(w: &Workspace, layer: &str) -> bool {
    let visibility = &w.view.visibility;
    match layer {
        "sketch-lines" => visibility.sketch_lines,
        "construction" => visibility.construction,
        "sketch-points" => visibility.sketch_points,
        "sketch-midpoints" => visibility.sketch_midpoints,
        "dimensions" => w.view.sketch_annotation_visibility[0],
        "relations" => w.view.sketch_annotation_visibility[1],
        "planes" => visibility.planes,
        "axes" => visibility.axes,
        "origins" => visibility.origins,
        "reference-points" => visibility.reference_points,
        "sketch-grid" => visibility.sketch_grid,
        "centerlines" => visibility.centerlines,
        "edge-midpoints" => visibility.edge_midpoints,
        _ => false,
    }
}
pub fn member_visible(w: &Workspace, construction: bool) -> bool {
    if construction {
        w.view.visibility.construction
    } else {
        w.view.visibility.sketch_lines
    }
}
pub fn point_visible(w: &Workspace, source: &cad_core::Sketch, point: &str) -> bool {
    if w.view.visibility.sketch_lines && w.view.visibility.construction {
        return w.view.visibility.sketch_points;
    }
    w.view.visibility.sketch_points
        && (source
            .members
            .values()
            .all(|member| !member.curve.points().contains(&point))
            || source.members.values().any(|member| {
                member_visible(w, member.construction) && member.curve.points().contains(&point)
            }))
}
pub fn point3d_visible(
    w: &Workspace,
    source: &cad_core::sketch3d::Definition,
    point: &str,
) -> bool {
    if w.view.visibility.sketch_lines && w.view.visibility.construction {
        return w.view.visibility.sketch_points;
    }
    w.view.visibility.sketch_points
        && (source
            .members
            .values()
            .all(|member| !member.curve.points().iter().any(|id| id.as_str() == point))
            || source.members.values().any(|member| {
                member_visible(w, member.construction)
                    && member.curve.points().iter().any(|id| id.as_str() == point)
            }))
}
pub fn reference_visible(w: &Workspace, topology: &str) -> bool {
    match topology {
        "plane" => w.view.visibility.planes,
        "axis" => w.view.visibility.axes,
        "point" => w.view.visibility.reference_points,
        "coordinate_system" => {
            w.view.visibility.axes || w.view.visibility.origins || w.view.visibility.planes
        }
        _ => true,
    }
}
pub fn midpoint(w: &Workspace, ui: &egui::Ui, rect: egui::Rect, id: &str, point: [f64; 3]) {
    if !w.view.visibility.sketch_midpoints {
        return;
    }
    let props = properties(w, id, "sketch-midpoint", false);
    if hidden(&props) {
        return;
    }
    let Some(point) = viewport::project(w, rect, point) else {
        return;
    };
    let color = props
        .get("color")
        .and_then(|value| crate::ui::color(value))
        .unwrap_or_else(|| ui.visuals().weak_text_color());
    let size = controls::number(&props, "size", 3.).clamp(1., 12.);
    let points = [
        egui::vec2(0., -size),
        egui::vec2(size, 0.),
        egui::vec2(0., size),
        egui::vec2(-size, 0.),
        egui::vec2(0., -size),
    ]
    .map(|offset| point + offset);
    ui.painter().with_clip_rect(rect).add(egui::Shape::line(
        points.to_vec(),
        egui::Stroke::new(controls::number(&props, "stroke-width", 1.), color),
    ));
}
pub fn visibility_controls(ui: &mut egui::Ui, w: &mut Workspace) {
    controls::menu_button(ui, "viewport.visibility-menu", "Show / Hide", |ui| {
        if let Some(diagnostic) = &w.visibility_diagnostic {
            controls::label(
                ui,
                "viewport.visibility-storage-diagnostic",
                "diagnostic",
                &["warning"],
                diagnostic,
                "warning",
            );
            ui.separator();
        }
        for (group, label, keys) in [
            (
                "sketch",
                "Sketch",
                &[
                    "sketch-lines",
                    "construction",
                    "sketch-points",
                    "sketch-midpoints",
                    "dimensions",
                    "relations",
                    "sketch-grid",
                ][..],
            ),
            (
                "references",
                "Reference geometry",
                &["planes", "axes", "origins", "reference-points"][..],
            ),
            (
                "hover",
                "Hover guides",
                &["centerlines", "edge-midpoints"][..],
            ),
        ] {
            if group != "sketch" {
                ui.separator();
            }
            controls::label(
                ui,
                &format!("viewport.visibility-group.{group}"),
                "menu-group",
                &[],
                label,
                "ready",
            );
            for key in keys {
                let label = LAYERS
                    .iter()
                    .find(|(candidate, _)| candidate == key)
                    .map(|(_, label)| *label)
                    .unwrap_or(key);
                let command = format!("viewport.visibility.{key}");
                let visible = layer(w, key);
                if controls::button(
                    ui,
                    &command,
                    "viewport-visibility-toggle",
                    &["menu-action"],
                    &format!("{} {label}", if visible { "✓" } else { "  " }),
                    true,
                    visible,
                )
                .clicked()
                {
                    action(w, &command);
                }
            }
        }
        ui.separator();
        if controls::button(
            ui,
            "viewport.visibility.restore",
            "viewport-action",
            &["menu-action"],
            "Show all categories",
            true,
            false,
        )
        .clicked()
        {
            action(w, "viewport.visibility.restore");
        }
        ui.small("Saved with this project. Hidden items cannot be picked in the viewport. Drawing previews stay visible while placing new geometry.");
        ui.small("Sketch outlines and References control which saved features are shown. Hover guides also respect Settings and theme preferences.");
    });
}

pub fn pick_filter(w: &Workspace) -> &str {
    w.fields
        .get("sketch-pick-filter")
        .map(String::as_str)
        .filter(|value| matches!(*value, "points" | "members" | "construction"))
        .unwrap_or("all")
}
pub fn pick_point(w: &Workspace, source: &cad_core::Sketch, point: &str) -> bool {
    if !point_visible(w, source, point) {
        return false;
    }
    match pick_filter(w) {
        "members" => false,
        "construction" => source
            .members
            .values()
            .any(|member| member.construction && member.curve.points().contains(&point)),
        _ => true,
    }
}
pub fn pick_member(w: &Workspace, construction: bool) -> bool {
    if !member_visible(w, construction) {
        return false;
    }
    match pick_filter(w) {
        "points" => false,
        "construction" => construction,
        _ => true,
    }
}
pub fn pick_reference(w: &Workspace) -> bool {
    matches!(pick_filter(w), "all" | "points")
}
pub fn properties(w: &Workspace, id: &str, role: &str, selected: bool) -> BTreeMap<String, String> {
    w.theme.properties_for(&viewport::style_target(w).child(
        id,
        role,
        &["viewport-overlay".into()],
        if selected { "selected" } else { "ready" },
    ))
}
pub fn reference_properties(
    w: &Workspace,
    id: &str,
    topology: &str,
    selected: bool,
) -> BTreeMap<String, String> {
    w.theme.properties_for(&viewport::style_target(w).child(
        id,
        "reference-geometry",
        &["viewport-overlay".into(), topology.into()],
        if selected { "selected" } else { "ready" },
    ))
}
pub fn mode(w: &Workspace, sketches: bool) -> OverlayVisibility {
    let props = viewport::style(w);
    let key = if sketches {
        "renderer-sketch-display"
    } else {
        "renderer-reference-display"
    };
    props
        .get(key)
        .and_then(|value| OverlayVisibility::parse(value))
        .unwrap_or_else(|| {
            if sketches {
                w.view.sketch_overlay_mode.clone()
            } else {
                w.view.reference_overlay_mode.clone()
            }
        })
}
pub fn xray(w: &Workspace, sketches: bool, props: &BTreeMap<String, String>) -> bool {
    props
        .get("renderer-overlay-depth")
        .and_then(|value| match value.as_str() {
            "xray" => Some(true),
            "visible" => Some(false),
            _ => None,
        })
        .unwrap_or(if sketches {
            w.view.sketch_overlay_xray
        } else {
            w.view.reference_overlay_xray
        })
}
pub fn hidden(props: &BTreeMap<String, String>) -> bool {
    props.get("display").is_some_and(|value| value == "none")
        || props
            .get("visibility")
            .is_some_and(|value| value == "hidden")
}
fn clear_pick_cache(w: &mut Workspace) {
    w.hover_guide = None;
    w.hover_cache = Default::default();
    w.candidates.clear();
    w.candidate_index = 0;
    w.selection_popup = None;
    w.fields.remove("sketch-snap-target");
    crate::sketch3d_ui::visibility_changed(w);
    if let Some(sketch) = &mut w.sketch {
        sketch.pick_candidates.clear();
        sketch.pick_index = 0;
    }
}
pub fn action(w: &mut Workspace, command: &str) -> bool {
    if let Some(layer) = command.strip_prefix("viewport.visibility.") {
        if layer == "restore" {
            w.view.visibility = Default::default();
            w.view.sketch_annotation_visibility = [true; 2];
        } else if let Some(value) = layer_mut(w, layer) {
            *value = !*value;
        } else {
            return false;
        }
        clear_pick_cache(w);
        return true;
    }
    if crate::display_mesh::action(w, command) {
        return true;
    }
    match command {
        "viewport.sketches.auto" => w.view.sketch_overlay_mode = OverlayVisibility::Auto,
        "viewport.sketches.all" => w.view.sketch_overlay_mode = OverlayVisibility::All,
        "viewport.sketches.hidden" => w.view.sketch_overlay_mode = OverlayVisibility::Hidden,
        "viewport.references.auto" => w.view.reference_overlay_mode = OverlayVisibility::Auto,
        "viewport.references.all" => w.view.reference_overlay_mode = OverlayVisibility::All,
        "viewport.references.hidden" => w.view.reference_overlay_mode = OverlayVisibility::Hidden,
        "viewport.sketch-xray" => w.view.sketch_overlay_xray = !w.view.sketch_overlay_xray,
        "viewport.reference-xray" => w.view.reference_overlay_xray = !w.view.reference_overlay_xray,
        "sketch.snap-entities" => w.view.sketch_entity_snap = !w.view.sketch_entity_snap,
        "sketch.pick.all"
        | "sketch.pick.points"
        | "sketch.pick.members"
        | "sketch.pick.construction" => {
            w.fields.insert(
                "sketch-pick-filter".into(),
                command.trim_start_matches("sketch.pick.").into(),
            );
            if let Some(s) = &mut w.sketch {
                s.pick_candidates.clear();
                s.pick_index = 0;
            }
        }
        "viewport.fit-sketch" => crate::sketch::fit_active(w),
        "viewport.fit-sketch-selection" => crate::sketch::fit_selection(w),
        "viewport.fit-support" => crate::sketch::fit_support(w),
        "viewport.fit-part-context" => crate::sketch::fit_part_context(w),
        _ => return false,
    }
    w.hover_guide = None;
    w.hover_cache = Default::default();
    if command.starts_with("viewport.sketches.")
        || command.starts_with("viewport.references.")
        || matches!(command, "viewport.sketch-xray" | "viewport.reference-xray")
    {
        clear_pick_cache(w);
    }
    true
}
pub fn semantics(w: &Workspace) -> Vec<SemanticElement> {
    let mut elements = crate::display_mesh::semantics(w);
    if let Some(diagnostic) = &w.visibility_diagnostic {
        elements.push(SemanticElement { id: "viewport.visibility-storage-diagnostic".into(), role: "diagnostic".into(), label: diagnostic.clone(), value: None, state: "warning".into(), commands: vec![], relationships: vec!["viewport.visibility-menu".into()], data: serde_json::json!({"contract":"tcad.viewport-visibility","version":1,"classes":["warning"]}) });
    }
    if let Some(sketch) = &w.sketch {
        for (id, member) in &sketch.members {
            if let cad_core::SketchCurve::Line { start, end } = &member.curve {
                let visible =
                    w.view.visibility.sketch_midpoints && member_visible(w, member.construction);
                elements.push(SemanticElement { id: format!("{}::midpoint/{id}", sketch.sketch_id), role: "sketch-midpoint".into(), label: "Line midpoint".into(), value: None, state: if visible { "ready" } else { "hidden" }.into(), commands: vec![], relationships: vec![sketch.sketch_id.clone(), id.clone()], data: serde_json::json!({"contract":"tcad.viewport-visibility","version":1,"visible":visible,"anchor":{"kind":"member_midpoint","member":id,"endpoints":[start,end]},"selectable":false,"classes":["viewport-overlay"]}) });
            }
        }
    }
    elements.push(SemanticElement { id: "viewport.visibility-menu".into(), role: "menu".into(), label: "Show / Hide".into(), value: None, state: "ready".into(), commands: vec![], relationships: vec!["workspace.viewport".into()], data: serde_json::json!({"contract":"tcad.viewport-visibility","version":1,"children":LAYERS.iter().map(|(key, _)| format!("viewport.visibility.{key}")).chain(std::iter::once("viewport.visibility.restore".into())).collect::<Vec<_>>()}) });
    for (key, label) in [
        ("sketch", "Sketch"),
        ("references", "Reference geometry"),
        ("hover", "Hover guides"),
    ] {
        elements.push(SemanticElement {
            id: format!("viewport.visibility-group.{key}"),
            role: "menu-group".into(),
            label: label.into(),
            value: None,
            state: "ready".into(),
            commands: vec![],
            relationships: vec!["viewport.visibility-menu".into()],
            data: serde_json::json!({"geometry_changes":false}),
        });
    }
    for &(key, label) in LAYERS {
        let visible = layer(w, key);
        let command = format!("viewport.visibility.{key}");
        elements.push(SemanticElement { id: command.clone(), role: "viewport-visibility-toggle".into(), label: label.into(), value: Some(visible.to_string()), state: if visible { "selected" } else { "ready" }.into(), commands: vec![command], relationships: vec!["workspace.viewport".into()], data: serde_json::json!({"contract":"tcad.viewport-visibility","version":1,"layer":key,"visible":visible,"saved":"project_presentation","geometry_changes":false,"enabled":true,"classes":["menu-action"]}) });
    }
    elements.push(SemanticElement {
        id: "viewport.visibility.restore".into(),
        role: "viewport-action".into(),
        label: "Show all visibility categories".into(),
        value: None,
        state: "ready".into(),
        commands: vec!["viewport.visibility.restore".into()],
        relationships: vec!["workspace.viewport".into()],
        data: serde_json::json!({"enabled":true,"geometry_changes":false}),
    });
    for (sketches, label, role, prefix) in [
        (
            true,
            "Sketch outlines",
            "sketch-overlay-display",
            "sketches",
        ),
        (
            false,
            "Reference geometry",
            "reference-overlay-display",
            "references",
        ),
    ] {
        let mode = mode(w, sketches);
        let xray = xray(w, sketches, &viewport::style(w));
        elements.push(SemanticElement {
            id: format!("workspace.{prefix}-display"), role: role.into(), label: label.into(),
            value: Some(format!("{} · {}", mode.as_str(), if xray { "X-ray" } else { "Visible surfaces" })),
            state: mode.as_str().into(),
            commands: ["auto", "all", "hidden"].into_iter().map(|mode| format!("viewport.{prefix}.{mode}"))
                .chain(std::iter::once(if sketches { "viewport.sketch-xray" } else { "viewport.reference-xray" }.into())).collect(),
            relationships: vec!["workspace.viewport".into()],
            data: serde_json::json!({"contract":"tcad.sketch-overlay-display","version":CONTRACT_VERSION,
                "mode":mode.as_str(),"xray":xray,"geometry_changes":false,"depth_source":"disposable_display_mesh",
                "auto":"selected_or_active; unused sketches remain accessible through tree selection or Show all",
                "theme_properties":["renderer-sketch-display","renderer-reference-display","renderer-overlay-depth","display","visibility","color","stroke-width"]}),
        });
    }
    elements.push(SemanticElement {
        id: "workspace.sketch-entity-snap".into(), role: "sketch-snap-setting".into(), label: "Snap entities".into(),
        value: Some(w.view.sketch_entity_snap.to_string()), state: if w.view.sketch_entity_snap { "enabled" } else { "disabled" }.into(),
        commands: vec!["sketch.snap-entities".into()], relationships: vec!["workspace.viewport".into()],
        data: serde_json::json!({"contract":"tcad.sketch-snap","version":1,"entity_snap":w.view.sketch_entity_snap,
            "bypass":"Alt bypasses grid and entity snapping; drawing handles cannot force a join","snap_target":w.fields.get("sketch-snap-target"),"origin_inference":"persistent_coincident_relation","endpoint_inference":"shared_native_point_or_coincident_generated_point"}),
    });
    elements.push(SemanticElement { id: "workspace.sketch-pick-filter".into(), role: "sketch-selection-filter".into(), label: "Sketch selection filter".into(), value: Some(pick_filter(w).into()), state: "ready".into(),
        commands: ["all", "points", "members", "construction"].map(|mode| format!("sketch.pick.{mode}")).to_vec(), relationships: vec!["workspace.viewport".into()],
        data: serde_json::json!({"contract":"tcad.sketch-picking","version":1,"filter":pick_filter(w),"geometry_changes":false,"selection_metric":"screen_space_points","actions":[{"command":"sketch.pick.all","label":"All geometry"},{"command":"sketch.pick.points","label":"Points & centers"},{"command":"sketch.pick.members","label":"Members"},{"command":"sketch.pick.construction","label":"Construction"}]}),
    });
    for (filter, label) in [
        ("all", "All"),
        ("points", "Points"),
        ("members", "Members"),
        ("construction", "Construction"),
    ] {
        let command = format!("sketch.pick.{filter}");
        elements.push(SemanticElement { id: command.clone(), role: "sketch-selection-filter".into(), label: label.into(), value: None, state: if pick_filter(w) == filter { "selected" } else { "ready" }.into(), commands: vec![command], relationships: vec!["workspace.sketch-pick-filter".into()], data: serde_json::json!({"contract":"tcad.sketch-picking","version":1,"filter":filter,"enabled":true,"geometry_changes":false,"classes":["compact","selection-filter"]}) });
    }
    if let Some(target) = w.fields.get("sketch-snap-target")
        && let Some(s) = &w.sketch
    {
        elements.push(SemanticElement { id: "sketch.snap-target".into(), role: "sketch-snap-target".into(), label: if target == cad_core::sketch_actions::ORIGIN { "Coincident with Origin · persistent relation · Alt bypasses" } else { "Join endpoint · persistent shared point · Alt bypasses" }.into(), value: None, state: "selected".into(), commands: vec![], relationships: vec![s.sketch_id.clone(), target.clone()], data: serde_json::json!({"sketch":s.sketch_id,"target":target,"inference":"persistent","bypass":"Alt","classes":["viewport-overlay","inference"]}) });
    }
    if let Some(s) = &w.sketch {
        elements.push(SemanticElement { id: format!("{}::pick-chooser", s.sketch_id), role: "sketch-pick-chooser".into(), label: "Nearby sketch geometry".into(), value: s.pick_candidates.get(s.pick_index).map(|candidate| candidate.label.clone()), state: if s.pick_candidates.len() > 1 { "available" } else { "hidden" }.into(), commands: vec![], relationships: vec![s.sketch_id.clone()],
            data: serde_json::json!({"contract":"tcad.sketch-picking","version":1,"sketch":s.sketch_id,"filter":pick_filter(w),"candidate_index":s.pick_index,"candidates":s.pick_candidates.iter().map(|candidate|serde_json::json!({"entity":candidate.entity,"label":candidate.label,"screen_distance":candidate.distance})).collect::<Vec<_>>(),"keyboard":"Tab cycles nearby geometry while the viewport owns focus","geometry_changes":false}),
        });
        for candidate in &s.pick_candidates {
            elements.push(SemanticElement { id: format!("{}::pick/{}", s.sketch_id, candidate.entity.key()), role: "sketch-reference-action".into(), label: candidate.label.clone(), value: None, state: if s.selection.contains(&candidate.entity) { "selected" } else { "ready" }.into(), commands: vec!["sketch.select-entity".into()], relationships: vec![s.sketch_id.clone(), candidate.entity.key().into()],
                data: serde_json::json!({"sketch":s.sketch_id,"reference":candidate.entity.key(),"entity":candidate.entity,"classes":["selection-candidate"],"accessibility_label":format!("Select {}", candidate.label)}),
            });
        }
    }
    for (command, label, target) in [
        ("viewport.fit-sketch", "Fit active sketch", "active_sketch"),
        (
            "viewport.fit-sketch-selection",
            "Fit sketch selection",
            "selected_sketch_geometry",
        ),
        (
            "viewport.fit-support",
            "Fit sketch support face",
            "support_face",
        ),
        (
            "viewport.fit-part-context",
            "Fit editing part",
            "active_occurrence",
        ),
    ] {
        elements.push(SemanticElement { id: command.into(), role: "camera-framing-action".into(), label: label.into(),
            value: None, state: if w.sketch.is_some() { "available" } else { "disabled" }.into(), commands: vec![command.into()],
            relationships: vec!["workspace.viewport".into()],
            data: serde_json::json!({"contract":"tcad.sketch-framing","version":1,"enabled":w.sketch.is_some(),"target":target,"geometry_changes":false}),
        });
    }
    elements
}
pub fn view_controls(ui: &mut egui::Ui, w: &mut Workspace) {
    crate::display_mesh::controls(ui, w);
    for (sketches, label, prefix) in [
        (true, "Sketch outlines", "sketches"),
        (false, "References", "references"),
    ] {
        ui.menu_button(label, |ui| {
            let active = mode(w, sketches);
            for (value, label) in [("auto", "Auto · selected / active"), ("all", "Show all"), ("hidden", "Hide overlays")] {
                let command = format!("viewport.{prefix}.{value}");
                if controls::button(ui, &command, "viewport-action", &["menu-action"], label, true, active.as_str() == value).clicked() {
                    action(w, &command); ui.close();
                }
            }
            let command = if sketches { "viewport.sketch-xray" } else { "viewport.reference-xray" };
            if controls::button(ui, command, "viewport-action", &["menu-action"], "X-ray through solids", true,
                if sketches { w.view.sketch_overlay_xray } else { w.view.reference_overlay_xray }).clicked() {
                action(w, command); ui.close();
            }
            ui.small("Auto shows the selected sketch / datum. Select a hidden sketch in the tree to reveal it, or use Show all.");
        });
    }
    if w.sketch.is_some() {
        for (command, label) in [
            ("viewport.fit-sketch", "Fit active sketch"),
            ("viewport.fit-sketch-selection", "Fit sketch selection"),
            ("viewport.fit-support", "Fit support face"),
            ("viewport.fit-part-context", "Fit editing part"),
        ] {
            if controls::button(
                ui,
                command,
                "viewport-action",
                &["menu-action"],
                label,
                true,
                false,
            )
            .clicked()
            {
                action(w, command);
                ui.close();
            }
        }
    }
}
pub fn snap_controls(ui: &mut egui::Ui, w: &mut Workspace) {
    visibility_controls(ui, w);
    if controls::button(ui, "sketch.snap-entities", "sketch-snap-setting", &["sketch-control"], "Snap entities", true, w.view.sketch_entity_snap)
        .on_hover_text("Join the highlighted endpoint, or save Coincident with Origin when the origin is highlighted. Hold Alt for independent placement; Alt also bypasses grid snapping.").clicked() {
        action(w, "sketch.snap-entities");
    }
    selection_controls(ui, w);
}
pub fn selection_controls(ui: &mut egui::Ui, w: &mut Workspace) {
    ui.horizontal_wrapped(|ui| {
        for (mode, label) in [
            ("all", "All"),
            ("points", "Points"),
            ("members", "Members"),
            ("construction", "Construction"),
        ] {
            let command = format!("sketch.pick.{mode}");
            if controls::button(
                ui,
                &command,
                "sketch-selection-filter",
                &["compact", "selection-filter"],
                label,
                true,
                pick_filter(w) == mode,
            )
            .clicked()
            {
                action(w, &command);
            }
        }
    });
}

pub fn candidate_controls(w: &mut Workspace, ui: &mut egui::Ui, viewport: egui::Rect) -> bool {
    let Some(s) = &w.sketch else { return false };
    if s.tool != crate::sketch::Tool::Select || s.pick_candidates.len() < 2 {
        return false;
    }
    let candidates = s.pick_candidates.clone();
    let selected = s.pick_index.min(candidates.len() - 1);
    let id = format!("{}::pick-chooser", s.sketch_id);
    let default = egui::Rect::from_min_size(
        viewport.left_bottom() + egui::vec2(12., -40.),
        egui::vec2(260., 30.),
    );
    let Some(rect) =
        controls::overlay_rect_state(&id, "sketch-pick-chooser", "available", viewport, default)
    else {
        return false;
    };
    let props = controls::properties(
        &id,
        "sketch-pick-chooser",
        &["viewport-overlay", "selection-candidates"],
        "available",
    );
    let mut choice = None;
    let mut child = ui.new_child(egui::UiBuilder::new().id_salt(&id).max_rect(rect));
    controls::apply_properties(&mut child, &props);
    controls::frame_from_properties(&mut child, &props).show(&mut child, |ui| {
        egui::ComboBox::from_id_salt(&id).selected_text(format!("{} · {} choices", candidates[selected].label, candidates.len())).show_ui(ui, |ui| {
            for (index, candidate) in candidates.iter().enumerate() {
                if ui.selectable_label(index == selected, &candidate.label).clicked() { choice = Some(index); }
            }
        }).response.on_hover_text("Choose overlapping sketch geometry. Tab cycles the highlighted candidate while the viewport has focus.");
    });
    if let Some(index) = choice {
        let additive = ui.input(|input| {
            input.modifiers.command || input.modifiers.ctrl || input.modifiers.shift
        });
        crate::sketch_ui::select(w, Some(candidates[index].entity.clone()), additive);
        if let Some(s) = &mut w.sketch {
            s.pick_index = index;
        }
        crate::sketch_ui::selection_changed(w);
    }
    choice.is_some()
        || ui.input(|input| {
            input
                .pointer
                .hover_pos()
                .is_some_and(|pointer| rect.contains(pointer))
                && (input.pointer.any_pressed()
                    || input.pointer.any_down()
                    || input.pointer.any_released())
        })
}
/// Clip continuous projected lines against indexed disposable display triangles.
/// Work exhaustion hides optional decoration instead of drawing false geometry
/// over solids. Explicit X-ray skips occlusion; neither path changes CAD data.
pub fn line(
    w: &Workspace,
    ui: &egui::Ui,
    rect: egui::Rect,
    endpoints: [[f64; 3]; 2],
    stroke: egui::Stroke,
    xray: bool,
    remaining: &mut usize,
) {
    let Some((a, b)) =
        viewport::project(w, rect, endpoints[0]).zip(viewport::project(w, rect, endpoints[1]))
    else {
        return;
    };
    if xray {
        ui.painter()
            .with_clip_rect(rect)
            .line_segment([a, b], stroke);
        return;
    }
    for segment in crate::overlay_depth::segments(w, rect, endpoints, remaining) {
        ui.painter()
            .with_clip_rect(rect)
            .line_segment(segment, stroke);
    }
}
