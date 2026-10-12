//! Presentation-only sketch/reference visibility and bounded display occlusion.
use crate::{Workspace, presentation_controls as controls, viewport};
use bevy::math::DVec3;
use bevy_egui::egui;
use cad_core::presentation::{OverlayVisibility, SemanticElement};
use std::collections::BTreeMap;

pub const CONTRACT_VERSION: u32 = 1;
pub const DEPTH_WORK: usize = 240_000;
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
pub fn action(w: &mut Workspace, command: &str) -> bool {
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
        "viewport.fit-sketch" => crate::sketch::fit_active(w),
        "viewport.fit-support" => crate::sketch::fit_support(w),
        "viewport.fit-part-context" => crate::sketch::fit_part_context(w),
        _ => return false,
    }
    w.hover_guide = None;
    w.hover_cache = Default::default();
    true
}
pub fn semantics(w: &Workspace) -> Vec<SemanticElement> {
    let mut elements = Vec::new();
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
            "bypass":"Alt bypasses grid and entity snapping; drawing handles cannot force a join","snap_target":w.fields.get("sketch-snap-target")}),
    });
    for (command, label, target) in [
        ("viewport.fit-sketch", "Fit active sketch", "active_sketch"),
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
    if controls::button(ui, "sketch.snap-entities", "sketch-snap-setting", &["sketch-control"], "Snap entities", true, w.view.sketch_entity_snap)
        .on_hover_text("Join the highlighted endpoint. Hold Alt for independent placement; Alt also bypasses grid snapping.").clicked() {
        action(w, "sketch.snap-entities");
    }
}
/// Short display fragments are culled by the same occluders as viewport guides.
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
    let steps = ((b - a).length() / 10.).ceil().clamp(1., 192.) as usize;
    let start = DVec3::from_array(endpoints[0]);
    let delta = DVec3::from_array(endpoints[1]) - start;
    for step in 0..steps {
        let lo = step as f64 / steps as f64;
        let hi = (step + 1) as f64 / steps as f64;
        let point = (start + delta * ((lo + hi) * 0.5)).to_array();
        if !viewport::visible_at(w, rect, point, remaining) {
            continue;
        }
        if let Some((a, b)) = viewport::project(w, rect, (start + delta * lo).to_array())
            .zip(viewport::project(w, rect, (start + delta * hi).to_array()))
        {
            ui.painter()
                .with_clip_rect(rect)
                .line_segment([a, b], stroke);
        }
    }
}
