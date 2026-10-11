//! Transient center guides over evaluated part edges. Native curve metadata
//! supplies centers; tessellation is used only for screen proximity and visibility.
use crate::{Workspace, presentation_controls::number, viewport};
use bevy::math::DVec3;
use bevy_egui::egui::{self, Color32, Pos2, Rect, Stroke, Vec2};
use cad_core::{
    PlacedPart,
    presentation::{SemanticElement, StyleTarget},
    protocol::{EDGE_CENTER_VERSION, EdgeCenter, EdgeData},
};
use std::collections::BTreeMap;

/// Presentation feedback only: never persisted or used as modeling authority.
#[derive(Clone, Debug)]
pub struct Guide {
    pub element: SemanticElement,
    drawing: DrawingGuide,
}

/// Reuse both hits and misses while the presentation context is unchanged.
/// Stored geometry is limited to one analytic guide, never a display mesh.
#[derive(Default)]
pub struct Cache {
    key: Option<String>,
    guide: Option<Guide>,
}

impl Cache {
    fn clear(&mut self) {
        self.key = None;
        self.guide = None;
    }
}

#[derive(Clone, Debug)]
struct DrawingGuide {
    center: EdgeCenter,
    instance: PlacedPart,
    style: GuideStyle,
}

pub fn semantics(guide: Option<&Guide>) -> Vec<SemanticElement> {
    guide
        .into_iter()
        .map(|guide| guide.element.clone())
        .collect()
}

#[derive(Clone, Debug)]
struct GuideStyle {
    color: Color32,
    width: f32,
    hit_size: f32,
    extension: f32,
    mark_size: f32,
}

struct Candidate<'a> {
    edge: &'a EdgeData,
    instance: &'a PlacedPart,
    target: StyleTarget,
    style: GuideStyle,
    distance: f32,
    depth: f64,
    world_point: [f64; 3],
}

fn shortlist<'a>(candidates: &mut Vec<Candidate<'a>>, candidate: Candidate<'a>) {
    candidates.push(candidate);
    candidates.sort_by(|a, b| {
        a.distance
            .total_cmp(&b.distance)
            .then_with(|| a.depth.total_cmp(&b.depth))
            .then_with(|| a.target.node.id.cmp(&b.target.node.id))
    });
    candidates.truncate(8);
}

fn enabled(properties: &BTreeMap<String, String>, key: &str, fallback: bool) -> bool {
    properties
        .get(key)
        .map_or(fallback, |value| value == "true")
}

fn color(properties: &BTreeMap<String, String>, key: &str) -> Option<Color32> {
    properties
        .get(key)
        .and_then(|value| crate::ui::color(value))
}

fn style(
    w: &Workspace,
    parent: &StyleTarget,
    target: &StyleTarget,
    viewport: &BTreeMap<String, String>,
) -> Option<GuideStyle> {
    let properties = w.theme.properties_for(target);
    if properties
        .get("display")
        .is_some_and(|value| value == "none")
        || properties
            .get("visibility")
            .is_some_and(|value| value == "hidden")
    {
        return None;
    }
    // A generic text color is not an overlay color override. Semantic guide
    // selectors can still override every visual property, including color.
    let generic = w.theme.properties_for(&parent.child(
        "viewport.generic-guide",
        "viewport-overlay",
        &["viewport-overlay".into()],
        "hover visible transient",
    ));
    let fallback = color(viewport, "centerline-color").unwrap_or(Color32::from_rgb(82, 107, 135));
    let tint = if properties.get("color") != generic.get("color") {
        color(&properties, "color").unwrap_or(fallback)
    } else {
        fallback
    };
    let tint = tint.gamma_multiply(number(&properties, "opacity", 1.).clamp(0., 1.));
    let width = number(
        &properties,
        "stroke-width",
        number(viewport, "centerline-width", 1.25),
    )
    .clamp(0., 8.);
    if tint.a() == 0 || width <= 0. {
        return None;
    }
    Some(GuideStyle {
        color: tint,
        width,
        hit_size: number(
            &properties,
            "hit-size",
            number(viewport, "centerline-hit-size", 12.),
        )
        .clamp(0., 64.),
        extension: number(viewport, "centerline-extension", 24.).clamp(0., 200.),
        mark_size: number(
            &properties,
            "size",
            number(viewport, "centerline-mark-size", 7.),
        )
        .clamp(0., 40.),
    })
}

/// Escape separators too, so repeated names in nested occurrences cannot
/// collapse to the same style ID. Metadata and source IDs remain separately exposed.
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

fn target(
    parent: &StyleTarget,
    instance: &PlacedPart,
    edge: &EdgeData,
    circular: bool,
) -> StyleTarget {
    let occurrence = instance
        .path
        .iter()
        .map(|id| token(id))
        .collect::<Vec<_>>()
        .join("/");
    let body = edge
        .body
        .as_ref()
        .map(|body| body.node.as_str())
        .unwrap_or("");
    let id = format!(
        "hover-guide:{}:{}:{}:{}:{}:{}",
        token(&instance.definition),
        occurrence,
        token(body),
        token(&edge.reference.producer.node),
        token(&edge.reference.role),
        token(&edge.reference.topology),
    );
    let role = if circular {
        "hover-centerline"
    } else {
        "hover-midpoint"
    };
    let mut target = parent.child(
        &id,
        role,
        &[
            "viewport-overlay".into(),
            "hover-guide".into(),
            if circular {
                "circular-edge"
            } else {
                "straight-edge"
            }
            .into(),
        ],
        "hover visible transient",
    );
    target.node.attributes.extend([
        ("definition".into(), instance.definition.clone()),
        (
            "occurrence".into(),
            serde_json::to_string(&instance.path).unwrap_or_default(),
        ),
        (
            "mesh-key".into(),
            cad_core::history::mesh_key(instance).into(),
        ),
        ("edge-role".into(), edge.reference.role.clone()),
        ("producer".into(), edge.reference.producer.node.clone()),
        ("body".into(), body.into()),
        (
            "curve-kind".into(),
            if circular { "circle" } else { "line" }.into(),
        ),
    ]);
    target
}

fn project(w: &Workspace, rect: Rect, point: [f64; 3]) -> Option<Pos2> {
    viewport::project(w, rect, point).filter(|point| point.x.is_finite() && point.y.is_finite())
}

fn depth(w: &Workspace, point: [f64; 3]) -> f32 {
    (viewport::render_point(point) - viewport::eye(w)).dot(viewport::basis(w).0)
}

fn nearest(
    w: &Workspace,
    rect: Rect,
    pointer: Pos2,
    instance: &PlacedPart,
    edge: &EdgeData,
    remaining_segments: &mut usize,
) -> Result<Option<(f32, [f64; 3])>, ()> {
    let mut nearest = None;
    for segment in edge.points.windows(2) {
        if *remaining_segments == 0 {
            return Err(());
        }
        *remaining_segments -= 1;
        let world_a = viewport::placed(segment[0], instance);
        let world_b = viewport::placed(segment[1], instance);
        let (Some(a), Some(b)) = (project(w, rect, world_a), project(w, rect, world_b)) else {
            continue;
        };
        let delta = b - a;
        let t = ((pointer - a).dot(delta) / delta.length_sq().max(1e-9)).clamp(0., 1.);
        let distance = pointer.distance(a + delta * t);
        if nearest.as_ref().is_some_and(|(best, _)| distance >= *best) {
            continue;
        }
        // The screen segment interpolation is perspective dependent. Recover
        // its actual point on the world segment before testing occlusion.
        let a_depth = depth(w, world_a);
        let b_depth = depth(w, world_b);
        let divisor = b_depth * (1. - t) + a_depth * t;
        if divisor <= 0. || !divisor.is_finite() {
            continue;
        }
        let world_t = (t * a_depth / divisor) as f64;
        let world =
            std::array::from_fn(|axis| world_a[axis] + (world_b[axis] - world_a[axis]) * world_t);
        nearest = Some((distance, world));
    }
    Ok(nearest)
}

fn center(center: &EdgeCenter) -> Option<([f64; 3], bool)> {
    let (point, circular) = match center {
        EdgeCenter::Line {
            version,
            start,
            end,
        } if *version == EDGE_CENTER_VERSION => {
            let start = DVec3::from_array(*start);
            let end = DVec3::from_array(*end);
            if !start.is_finite() || !end.is_finite() || start.distance_squared(end) <= 1e-20 {
                return None;
            }
            ((start + (end - start) * 0.5).to_array(), false)
        }
        EdgeCenter::Circle {
            version,
            center,
            normal,
            radius_mm,
            radial,
        } if *version == EDGE_CENTER_VERSION
            && radius_mm.is_finite()
            && *radius_mm > 0.
            && DVec3::from_array(*normal).is_finite()
            && DVec3::from_array(*normal).length_squared() > 1e-20
            && DVec3::from_array(*radial).is_finite()
            && DVec3::from_array(*radial).length_squared() > 1e-20 =>
        {
            (*center, true)
        }
        _ => return None,
    };
    point
        .into_iter()
        .all(f64::is_finite)
        .then_some((point, circular))
}

/// Source-independent view context used by both cache invalidation and the
/// semantic snapshot. History is explicitly included because it is transient
/// and deliberately skipped in PresentationState's project serialization.
pub fn view_context(w: &Workspace) -> String {
    let application = w
        .settings
        .as_ref()
        .map(|settings| &settings.application)
        .unwrap_or(&w.application_preferences);
    serde_json::json!({
        "view":w.view,
        "history_cursor":w.view.history_cursor,
        "viewport_size":w.viewport_size,
        "logical_size":w.presentation_resources.logical_size,
        "hover_centerlines":application.hover_centerlines,
        "hover_midpoints":application.hover_midpoints,
        // An input-edge editor keeps its checkpoint identity while the user
        // changes the target body or occurrence. Those picking filters are
        // presentation context too, so they must invalidate cached guides.
        "selection_dialog":w.dialog,
        "selection_feature":crate::selection_ui::edge_feature(w),
        "edge_selection_path":w.edge_selection_path,
    })
    .to_string()
}

fn rect_key(rect: Rect) -> [f32; 4] {
    [rect.min.x, rect.min.y, rect.max.x, rect.max.y]
}

fn ancestry_key(target: &StyleTarget) -> Vec<serde_json::Value> {
    target
        .ancestors
        .iter()
        .chain(std::iter::once(&target.node))
        .map(|node| {
            serde_json::json!({
                "id":node.id, "role":node.role, "classes":node.classes,
                "attributes":node.attributes, "states":node.states,
            })
        })
        .collect()
}

/// `blocked` is viewport event ownership, supplied by the workspace so docked
/// controls, view cubes and duplicated viewport regions never produce stray guides.
pub fn overlay(
    w: &Workspace,
    ui: &egui::Ui,
    rect: Rect,
    blocked: bool,
    cache: &mut Cache,
) -> Option<Guide> {
    if blocked
        || w.gesture.is_some()
        || w.selection_popup.is_some()
        || crate::assembly_motion_ui::dragging(w)
        || w.sketch.as_ref().is_some_and(|sketch| {
            sketch.drag.is_some()
                || (sketch.tool != crate::sketch::Tool::Select
                    && (sketch.anchor.is_some()
                        || !sketch.clicks.is_empty()
                        || !sketch.chain.is_empty()))
        })
        || ui.input(|input| !input.focused || input.pointer.any_down())
    {
        cache.clear();
        return None;
    }
    let Some(pointer) = ui.input(|input| input.pointer.hover_pos()) else {
        cache.clear();
        return None;
    };
    if !rect.contains(pointer) || !ui.clip_rect().contains(pointer) {
        cache.clear();
        return None;
    }
    let Some(evaluation) = viewport::selection_evaluation(w) else {
        cache.clear();
        return None;
    };
    let parent = viewport::style_target(w);
    let view_context = view_context(w);
    let key = serde_json::json!({
        "pointer":[pointer.x,pointer.y],
        "rect":rect_key(rect), "clip":rect_key(ui.clip_rect()),
        "source":evaluation.identity,
        "display_source":viewport::display_evaluation(w).map(|evaluation| &evaluation.identity),
        "view_context":view_context,
        "theme":w.theme, "theme_text":w.theme_text,
        "ancestry":ancestry_key(&parent),
    })
    .to_string();
    if cache.key.as_ref() != Some(&key) {
        cache.guide = query(w, rect, pointer, &parent, &view_context);
        cache.key = Some(key);
    }
    let guide = cache.guide.as_ref()?;
    draw(w, ui, rect, &guide.drawing)?;
    Some(guide.clone())
}

fn query(
    w: &Workspace,
    rect: Rect,
    pointer: Pos2,
    parent: &StyleTarget,
    view_context: &str,
) -> Option<Guide> {
    let application = w
        .settings
        .as_ref()
        .map(|settings| &settings.application)
        .unwrap_or(&w.application_preferences);
    let viewport_style = viewport::style(w);
    let circles = enabled(
        &viewport_style,
        "hover-centerlines",
        application.hover_centerlines,
    );
    let midpoints = enabled(
        &viewport_style,
        "hover-midpoints",
        application.hover_midpoints,
    );
    if !circles && !midpoints {
        return None;
    }
    let evaluation = viewport::selection_evaluation(w)?;
    let mut candidates = Vec::new();
    let mut remaining_segments = 200_000;
    for instance in viewport::display_instances(w, evaluation) {
        if !viewport::visible(w, instance) {
            continue;
        }
        for mesh in viewport::selection_meshes(w, evaluation, instance) {
            for edge in &mesh.edges {
                let Some(metadata) = &edge.center else {
                    continue;
                };
                let Some((local_center, circular)) = center(metadata) else {
                    continue;
                };
                if (circular && !circles) || (!circular && !midpoints) {
                    continue;
                }
                let mut nearby = nearest(w, rect, pointer, instance, edge, &mut remaining_segments)
                    .ok()?
                    .into_iter()
                    .collect::<Vec<_>>();
                // Circular centers are useful even when the rim is further
                // away. A center hidden behind nearer geometry never wins.
                let world_center = viewport::placed(local_center, instance);
                if circular && let Some(position) = project(w, rect, world_center) {
                    nearby.push((pointer.distance(position), world_center));
                }
                // All hit targets are limited to 64 logical pixels. Avoid
                // building style targets for the rest of a dense assembly.
                if nearby.iter().all(|(distance, _)| *distance > 64.) {
                    continue;
                }
                let target = target(parent, instance, edge, circular);
                let Some(style) = style(w, parent, &target, &viewport_style) else {
                    continue;
                };
                for (distance, world_point) in nearby {
                    if distance > style.hit_size {
                        continue;
                    }
                    shortlist(
                        &mut candidates,
                        Candidate {
                            edge,
                            instance,
                            target: target.clone(),
                            style: style.clone(),
                            distance,
                            depth: depth(w, world_point) as f64,
                            world_point,
                        },
                    );
                }
            }
        }
    }
    // Ray occlusion is bounded independently of model complexity. A hidden
    // center candidate can fall back to the visible rim of the same edge.
    let mut remaining_triangles = 200_000;
    let candidate = candidates.into_iter().find(|candidate| {
        viewport::visible_at(w, rect, candidate.world_point, &mut remaining_triangles)
    })?;
    let metadata = candidate.edge.center.as_ref()?;
    let (local_center, circular) = center(metadata)?;
    Some(Guide {
        drawing: DrawingGuide {
            center: metadata.clone(),
            instance: candidate.instance.clone(),
            style: candidate.style,
        },
        element: SemanticElement {
            id: candidate.target.node.id,
            role: candidate.target.node.role,
            label: if circular {
                "Circular Edge Center"
            } else {
                "Straight Edge Midpoint"
            }
            .into(),
            value: None,
            state: "hover visible transient".into(),
            commands: vec![],
            relationships: vec![
                candidate.instance.definition.clone(),
                candidate.edge.reference.producer.node.clone(),
            ],
            data: serde_json::json!({
                "contract":"tcad.viewport-guides", "version":1,
                "evaluation_identity":evaluation.identity,
                "history_cursor":w.view.history_cursor,
                "view_context":view_context,
                "transient":true, "interactive":false, "model_mutation":false,
                "definition":candidate.instance.definition,
                "occurrence":candidate.instance.path,
                "mesh_key":cad_core::history::mesh_key(candidate.instance),
                "edge":candidate.edge.reference, "body":candidate.edge.body,
                "geometry":metadata,
                "center_mm":viewport::placed(local_center,candidate.instance),
                "display_geometry_is_authority":false,
                "style_classes":candidate.target.node.classes,
                "style_attributes":candidate.target.node.attributes,
            }),
        },
    })
}

fn dashed(painter: &egui::Painter, start: Pos2, end: Pos2, stroke: Stroke) {
    let Some([start, end]) = clip_line(painter.clip_rect().expand(stroke.width), start, end) else {
        return;
    };
    let distance = start.distance(end);
    if !distance.is_finite() || distance < 0.1 {
        return;
    }
    // Centerline dash pattern: long dash, short dash. Screen-space lengths
    // stay legible as users zoom, without claiming physical axis extents.
    let direction = (end - start) / distance;
    let mut offset = 0.;
    for _ in 0..2048 {
        if offset >= distance {
            break;
        }
        for (draw, span) in [(true, 8.), (false, 3.), (true, 2.), (false, 3.)] {
            let next = (offset + span).min(distance);
            if draw && offset < distance {
                painter.line_segment(
                    [start + direction * offset, start + direction * next],
                    stroke,
                );
            }
            offset = next;
        }
    }
}

fn clip_line(rect: Rect, start: Pos2, end: Pos2) -> Option<[Pos2; 2]> {
    if !start.x.is_finite() || !start.y.is_finite() || !end.x.is_finite() || !end.y.is_finite() {
        return None;
    }
    let delta = end - start;
    let mut first = 0_f32;
    let mut last = 1_f32;
    for (base, delta, low, high) in [
        (start.x, delta.x, rect.left(), rect.right()),
        (start.y, delta.y, rect.top(), rect.bottom()),
    ] {
        if delta.abs() <= 1e-8 {
            if base < low || base > high {
                return None;
            }
            continue;
        }
        let low = (low - base) / delta;
        let high = (high - base) / delta;
        first = first.max(low.min(high));
        last = last.min(low.max(high));
        if first > last {
            return None;
        }
    }
    Some([start + delta * first, start + delta * last])
}

fn mark(
    painter: &egui::Painter,
    center: Pos2,
    first: Vec2,
    second: Vec2,
    size: f32,
    stroke: Stroke,
) {
    for direction in [first, second] {
        if direction.length_sq() > 1e-8 {
            painter.line_segment(
                [center - direction * size, center + direction * size],
                stroke,
            );
        }
    }
    painter.circle_filled(center, (stroke.width * 0.8).clamp(1., 3.), stroke.color);
}

fn draw(w: &Workspace, ui: &egui::Ui, rect: Rect, drawing: &DrawingGuide) -> Option<()> {
    let painter = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
    let stroke = Stroke::new(drawing.style.width, drawing.style.color);
    let metadata = &drawing.center;
    match metadata {
        EdgeCenter::Line { start, end, .. } => {
            let world_start = viewport::placed(*start, &drawing.instance);
            let world_end = viewport::placed(*end, &drawing.instance);
            let (a, b) = (project(w, rect, world_start)?, project(w, rect, world_end)?);
            let midpoint = DVec3::from_array(*start).lerp(DVec3::from_array(*end), 0.5);
            let center = project(
                w,
                rect,
                viewport::placed(midpoint.to_array(), &drawing.instance),
            )?;
            let delta = b - a;
            if delta.length_sq() <= 1e-8 {
                // An end-on line has no reliable screen perpendicular.
                painter.circle_stroke(center, drawing.style.mark_size.max(2.), stroke);
                return Some(());
            }
            let along = delta.normalized();
            let perpendicular = Vec2::new(-along.y, along.x);
            let span = drawing.style.extension + drawing.style.mark_size;
            dashed(
                &painter,
                center - perpendicular * span,
                center + perpendicular * span,
                stroke,
            );
            mark(
                &painter,
                center,
                along,
                perpendicular,
                drawing.style.mark_size,
                stroke,
            );
        }
        EdgeCenter::Circle {
            center,
            normal,
            radius_mm,
            radial,
            ..
        } => {
            let world_center = viewport::placed(*center, &drawing.instance);
            let position = project(w, rect, world_center)?;
            let normal = DVec3::from_array(*normal).normalize();
            let radial = DVec3::from_array(*radial);
            let first = (radial - normal * radial.dot(normal)).try_normalize()?;
            let second = normal.cross(first).try_normalize()?;
            let center = DVec3::from_array(*center);
            let mut mark_directions = vec![];
            for direction in [first, second] {
                let low = viewport::placed(
                    (center - direction * *radius_mm).to_array(),
                    &drawing.instance,
                );
                let high = viewport::placed(
                    (center + direction * *radius_mm).to_array(),
                    &drawing.instance,
                );
                let (Some(a), Some(b)) = (project(w, rect, low), project(w, rect, high)) else {
                    continue;
                };
                let delta = b - a;
                if delta.length_sq() <= 1e-8 {
                    continue;
                }
                let screen = delta.normalized();
                mark_directions.push(screen);
                dashed(
                    &painter,
                    a - screen * drawing.style.extension,
                    b + screen * drawing.style.extension,
                    stroke,
                );
            }
            // The normal is the true circular axis, transformed with the same
            // occurrence as its edge. End-on axes reduce to the center mark.
            let axis_point = viewport::placed(
                (center + normal * radius_mm.max(1.)).to_array(),
                &drawing.instance,
            );
            if let Some(axis_end) = project(w, rect, axis_point) {
                let delta = axis_end - position;
                if delta.length_sq() > 1e-4 {
                    let direction = delta.normalized();
                    let length = drawing.style.extension + drawing.style.mark_size;
                    dashed(
                        &painter,
                        position - direction * length,
                        position + direction * length,
                        stroke,
                    );
                }
            }
            let (first, second) = match mark_directions.as_slice() {
                [first, second] => (*first, *second),
                [first] => (*first, Vec2::new(-first.y, first.x)),
                _ => (Vec2::X, Vec2::Y),
            };
            mark(
                &painter,
                position,
                first,
                second,
                drawing.style.mark_size,
                stroke,
            );
        }
    }
    Some(())
}
