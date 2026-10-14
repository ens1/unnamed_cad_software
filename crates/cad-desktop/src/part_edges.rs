//! Retained, depth-tested native boundary lines over shaded CAD bodies.
//! The line assets are disposable renderer caches, never modeling geometry.
use crate::{Workspace, viewport};
use bevy::{camera::visibility::RenderLayers, prelude::*};
use cad_core::presentation::{SemanticElement, StyleTarget};
use std::collections::BTreeMap;

#[derive(Resource, Default)]
pub struct Cache {
    scene: String,
    theme: String,
    viewport_target: String,
    shown: bool,
    pixels_per_point: f32,
}

#[derive(Component)]
pub struct PartEdges(Handle<GizmoAsset>);

fn hidden(props: &BTreeMap<String, String>) -> bool {
    props.get("display").is_some_and(|value| value == "none")
        || props
            .get("visibility")
            .is_some_and(|value| value == "hidden")
}

fn token(value: &str) -> String {
    use std::fmt::Write;
    let mut result = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.') {
            result.push(byte as char);
        } else {
            let _ = write!(result, "%{byte:02X}");
        }
    }
    result
}

pub fn semantics(w: &Workspace) -> Vec<SemanticElement> {
    let preferences = w
        .settings
        .as_ref()
        .map(|state| &state.application)
        .unwrap_or(&w.application_preferences);
    let props = viewport::style(w);
    let enabled = props
        .get("show-part-edges")
        .map(|value| value == "true")
        .unwrap_or(preferences.show_part_edges)
        && !hidden(&props);
    vec![SemanticElement {
        id: "workspace.edge-display".into(),
        role: "model-edge-display".into(),
        label: "Model Display".into(),
        value: Some(
            if enabled {
                "Shaded with edges"
            } else {
                "Shaded"
            }
            .into(),
        ),
        state: if enabled {
            "edges-visible"
        } else {
            "edges-hidden"
        }
        .into(),
        commands: vec!["settings.open.application.viewport".into()],
        relationships: vec!["workspace.viewport".into()],
        data: serde_json::json!({"contract":"tcad.viewport-edge-display","version":1,
            "show_part_edges":enabled,
            "hover_centerlines":props.get("hover-centerlines").map(|value|value=="true").unwrap_or(preferences.hover_centerlines),
            "hover_midpoints":props.get("hover-midpoints").map(|value|value=="true").unwrap_or(preferences.hover_midpoints),
            "geometry_changes":false,
            "source":"native_boundary_display","hidden_edges":"depth_tested"}),
    }]
}

fn edge_target(
    parent: &StyleTarget,
    instance: &cad_core::PlacedPart,
    edge: &cad_core::protocol::EdgeData,
    ghost: bool,
) -> StyleTarget {
    let mut target = parent.child(
        &format!(
            "part-edge:{}:{}:{}:{}:{}:{}",
            instance
                .path
                .iter()
                .map(|id| token(id))
                .collect::<Vec<_>>()
                .join("/"),
            token(&instance.definition),
            edge.body
                .as_ref()
                .map(|body| token(&body.node))
                .unwrap_or_default(),
            token(&edge.reference.producer.definition),
            token(&edge.reference.producer.node),
            token(&edge.reference.role)
        ),
        "part-edge",
        &[
            "viewport-overlay".into(),
            if ghost {
                "ghost-context"
            } else {
                "active-geometry"
            }
            .into(),
        ],
        if ghost { "ghost" } else { "ready" },
    );
    for (name, value) in [
        ("definition", instance.definition.clone()),
        (
            "occurrence",
            serde_json::to_string(&instance.path).unwrap_or_default(),
        ),
        ("producer", edge.reference.producer.node.clone()),
        ("construction-role", edge.reference.role.clone()),
    ] {
        target.node.attributes.insert(name.into(), value);
    }
    if let Some(body) = &edge.body {
        target
            .node
            .attributes
            .insert("body".into(), body.node.clone());
    }
    target
}

pub fn sync(
    mut commands: Commands,
    w: Res<Workspace>,
    mut cache: ResMut<Cache>,
    existing: Query<(Entity, &PartEdges)>,
    mut assets: ResMut<Assets<GizmoAsset>>,
) {
    let preferences = w
        .settings
        .as_ref()
        .map(|state| &state.application)
        .unwrap_or(&w.application_preferences);
    let parent = viewport::style_target(&w);
    let viewport_target = format!("{parent:?}");
    let viewport_props = w.theme.properties_for(&parent);
    let shown = viewport_props
        .get("show-part-edges")
        .map(|value| value == "true")
        .unwrap_or(preferences.show_part_edges);
    let pixels_per_point = w.viewport_pixels_per_point;
    if cache.scene == w.rendered_key
        && cache.theme == w.theme_text
        && cache.viewport_target == viewport_target
        && cache.shown == shown
        && cache.pixels_per_point == pixels_per_point
    {
        return;
    }
    cache.scene.clone_from(&w.rendered_key);
    cache.theme.clone_from(&w.theme_text);
    cache.viewport_target = viewport_target;
    cache.shown = shown;
    cache.pixels_per_point = pixels_per_point;
    for (entity, edges) in &existing {
        commands.entity(entity).despawn();
        assets.remove(&edges.0);
    }
    if !shown || hidden(&viewport_props) {
        return;
    }
    let Some(evaluation) = viewport::display_evaluation(&w) else {
        return;
    };
    let width =
        crate::presentation_controls::number(&viewport_props, "part-edge-width", 1.).clamp(0., 8.);
    let default_color = viewport_props
        .get("part-edge-color")
        .and_then(|value| crate::ui::color(value))
        .unwrap_or(bevy_egui::egui::Color32::from_rgb(51, 58, 68));
    let generic = w.theme.properties_for(&parent.child(
        "viewport.generic-overlay",
        "viewport-overlay",
        &["viewport-overlay".into()],
        "ready",
    ));
    let mut groups = BTreeMap::<u32, GizmoAsset>::new();
    for instance in viewport::display_instances(&w, evaluation) {
        if !viewport::visible(&w, instance) {
            continue;
        }
        for mesh in viewport::display_meshes(&w, evaluation, instance) {
            // Reusable reference curves have their own colored overlay.
            if mesh.indices.is_empty() {
                continue;
            }
            for edge in &mesh.edges {
                let ghost = viewport::ghost_neighbor(&w, instance);
                let props = w
                    .theme
                    .properties_for(&edge_target(&parent, instance, edge, ghost));
                if hidden(&props) {
                    continue;
                }
                let edge_width =
                    crate::presentation_controls::number(&props, "stroke-width", width)
                        .clamp(0., 8.);
                let mut color = if props.get("color") != generic.get("color") {
                    props
                        .get("color")
                        .and_then(|value| crate::ui::color(value))
                        .unwrap_or(default_color)
                } else {
                    default_color
                }
                .to_srgba_unmultiplied()
                .map(|channel| f32::from(channel) / 255.);
                color[3] *= crate::presentation_controls::number(
                    &props,
                    "opacity",
                    if ghost { 0.4 } else { 1. },
                )
                .clamp(0., 1.);
                if ghost {
                    color[3] *= crate::presentation_controls::number(
                        &viewport_props,
                        "ghost-opacity",
                        0.13,
                    )
                    .clamp(0., 1.);
                }
                if edge_width <= 0. || color[3] <= 0. {
                    continue;
                }
                let line_color = Color::srgba(color[0], color[1], color[2], color[3]);
                let asset = groups.entry(edge_width.to_bits()).or_default();
                asset.linestrip(
                    edge.points
                        .iter()
                        .map(|point| viewport::render_point(viewport::placed(*point, instance))),
                    line_color,
                );
            }
        }
    }
    for (width, asset) in groups {
        let handle = assets.add(asset);
        commands.spawn((
            Gizmo {
                handle: handle.clone(),
                line_config: GizmoLineConfig {
                    width: f32::from_bits(width) * pixels_per_point,
                    perspective: false,
                    joints: GizmoLineJoint::Bevel,
                    ..default()
                },
                // Keep the line on its front surface while retaining depth occlusion.
                depth_bias: -0.00001,
            },
            RenderLayers::layer(1),
            PartEdges(handle),
        ));
    }
}
