//! Transient feedback over the source body of a fillet or chamfer.
use super::{accepts_edge, edge_action, edge_feature, edge_selection, pick_granularity, pick_plan};
use crate::{Workspace, presentation_controls as controls, viewport};
use bevy_egui::egui::{self, Pos2, Rect};
use cad_core::{presentation::SemanticElement, protocol::RequestIdentity};
use std::{
    collections::BTreeSet,
    hash::{DefaultHasher, Hash, Hasher},
};

const BADGE: &str = "viewport.edge-pick-preview";
const ROLE: &str = "edge-pick-preview";

#[derive(Default)]
pub(crate) struct Cache {
    viewport: String,
    context: String,
    pointer: Option<Pos2>,
    on_edge: bool,
    seed: Option<viewport::Selection>,
    plan: Option<Preview>,
    mode: String,
    frame: u64,
    badge: Option<Rect>,
    anchor: Option<Pos2>,
}

struct Preview {
    feature: String,
    epoch: String,
    source: RequestIdentity,
    element: SemanticElement,
    segments: Vec<[Pos2; 2]>,
    stops: Vec<(Pos2, bool)>,
}

impl Cache {
    pub(crate) fn finish_frame(&mut self, frame: u64) {
        if self.frame != frame && (self.pointer.is_some() || self.plan.is_some()) {
            *self = Self::default();
        }
    }
}

pub(super) fn semantic(w: &Workspace) -> Option<SemanticElement> {
    let plan = w.edge_hover.plan.as_ref()?;
    (plan.epoch == w.session.source_epoch()
        && w.fields.get("feature-editor-id") == Some(&plan.feature)
        && w.dialog.as_deref() == Some("feature")
        && pick_granularity(w, &plan.feature) == w.edge_hover.mode
        && viewport::selection_evaluation(w).is_some_and(|e| e.identity == plan.source))
    .then(|| plan.element.clone())
}

fn preview(
    w: &Workspace,
    rect: Rect,
    feature: &str,
    mode: &str,
    seed: &viewport::Selection,
) -> Option<Preview> {
    let evaluation = viewport::selection_evaluation(w)?;
    let instance = viewport::display_instances(w, evaluation)
        .into_iter()
        .find(|instance| instance.path == seed.path && instance.definition == seed.definition)?;
    let (picks, stops) = pick_plan(w, seed, mode);
    let references: Vec<_> = picks.iter().map(|pick| &pick.reference).collect();
    let keys: BTreeSet<_> = references.iter().map(|r| (&r.producer, &r.role)).collect();
    let project = viewport::projector(w, rect);
    let segments = viewport::selection_meshes(w, evaluation, instance)
        .into_iter()
        .flat_map(|mesh| &mesh.edges)
        .filter(|edge| {
            edge.body == seed.body
                && keys.contains(&(&edge.reference.producer, &edge.reference.role))
        })
        .flat_map(|edge| edge.points.windows(2))
        .filter_map(|pair| {
            Some([
                project(viewport::placed(pair[0], instance))?,
                project(viewport::placed(pair[1], instance))?,
            ])
        })
        .collect();
    let name = match mode {
        "individual" => "This edge",
        "rim" => "Circular rim",
        _ => "Tangent chain",
    };
    let label = if mode == "individual" {
        name.to_owned()
    } else {
        format!(
            "{name} · {} {}",
            references.len(),
            if references.len() == 1 {
                "edge"
            } else {
                "edges"
            }
        )
    };
    let next_mode = if mode == "individual" {
        "tangent"
    } else {
        "individual"
    };
    let command = format!("selection.edge-mode-{next_mode}/{feature}");
    let element = SemanticElement {
        id: BADGE.into(),
        role: ROLE.into(),
        label,
        value: Some(
            if mode == "individual" {
                "Switch to Tangent chain"
            } else {
                "Switch to This edge"
            }
            .into(),
        ),
        state: "hover visible transient".into(),
        commands: vec![command],
        relationships: vec![feature.into(), seed.definition.clone()],
        data: serde_json::json!({"contract":"tcad.presentation.edge-pick-preview","version":1,
            "presentation_control":true,"classes":["viewport-overlay","compact"],"mode":mode,"occurrence":seed.path,"body":seed.body,
            "seed":seed.reference,"edges":references,"stops_world_mm":stops,"input_evaluation":evaluation.identity}),
    };
    Some(Preview {
        feature: feature.into(),
        epoch: w.session.source_epoch().into(),
        source: evaluation.identity.clone(),
        element,
        segments,
        stops: stops
            .into_iter()
            .filter_map(|stop| Some((project(stop.point)?, stop.junction)))
            .collect(),
    })
}

fn paint(w: &Workspace, ui: &egui::Ui, rect: Rect, preview: &Preview) {
    let props = controls::properties(
        "viewport.edge-pick-outline",
        "edge-pick-outline",
        &["viewport-overlay"],
        "hover",
    );
    if props.get("display").is_some_and(|v| v == "none")
        || props.get("visibility").is_some_and(|v| v == "hidden")
    {
        return;
    }
    let color = props
        .get("selection-color")
        .and_then(|v| crate::ui::color(v))
        .unwrap_or_else(|| viewport::selection_color(w))
        .gamma_multiply(controls::number(&props, "opacity", 1.).clamp(0., 1.));
    let width = controls::number(&props, "stroke-width", 2.5).max(0.);
    let painter = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
    let stroke = egui::Stroke::new(width, color);
    painter.extend(
        preview
            .segments
            .iter()
            .map(|line| egui::Shape::line_segment(*line, stroke)),
    );
    let radius = controls::number(&props, "size", 3.5).clamp(0., 24.);
    for &(point, junction) in &preview.stops {
        painter.circle_stroke(point, radius, stroke);
        if junction {
            painter.circle_stroke(point, radius * 1.8, stroke);
        }
    }
}

/// The badge stays at the picked edge while the pointer crosses onto its
/// button. Only the button's actual bounds intercept a modeling click.
pub(crate) fn overlay(
    w: &mut Workspace,
    ui: &mut egui::Ui,
    rect: Rect,
    response: &egui::Response,
    viewport_id: &str,
    blocked: bool,
) -> bool {
    let Some(pointer) = ui.input(|i| i.pointer.hover_pos()) else {
        return false;
    };
    let over_button = w.edge_hover.viewport == viewport_id
        && w.edge_hover.badge.is_some_and(|r| r.contains(pointer));
    if blocked
        || !rect.intersect(ui.clip_rect()).contains(pointer)
        || (!response.hovered() && !over_button)
        || ui.ctx().layer_id_at(pointer) != Some(ui.layer_id())
        || w.gesture.is_some()
        || w.selection_popup.is_some()
        || w.reference_repair.is_some()
        || crate::operation_editor_ui::recovery::replacement_index(w).is_some()
        || !ui.input(|i| i.focused)
    {
        return false;
    }
    let Some(feature) = edge_feature(w) else {
        return false;
    };
    let Some((body, _)) = edge_selection(&feature) else {
        return false;
    };
    let Some(evaluation) = viewport::selection_evaluation(w) else {
        return false;
    };
    let mode = pick_granularity(w, &feature.id).to_owned();
    // Source, visibility and occurrence transforms can change at the same
    // pointer location. Large scene keys are already maintained by the viewport.
    let mut scene = DefaultHasher::new();
    w.rendered_key.hash(&mut scene);
    w.selection_source.hash(&mut scene);
    let context = serde_json::json!([
        w.session.source_epoch(),
        evaluation.identity,
        w.display_mesh.epoch,
        crate::camera_views_ui::current(w),
        w.viewport_size,
        w.edge_selection_path,
        feature.id,
        w.fields.get("model-pick-filter"),
        scene.finish(),
        [rect.min.x, rect.min.y, rect.max.x, rect.max.y]
    ])
    .to_string();
    let mut cache = std::mem::take(&mut w.edge_hover);
    let new_context = cache.context != context || cache.viewport != viewport_id;
    if new_context {
        cache = Cache {
            context,
            viewport: viewport_id.into(),
            ..Default::default()
        };
    }
    cache.frame = ui.ctx().cumulative_frame_nr();
    let on_badge = cache.badge.is_some_and(|r| r.contains(pointer));
    if !on_badge && cache.pointer != Some(pointer) {
        let seed =
            viewport::first_edge(w, rect, pointer).filter(|pick| accepts_edge(w, body, pick));
        let same_seed = matches!((&seed, &cache.seed), (Some(a),Some(b)) if a.path == b.path && a.body == b.body && a.reference == b.reference);
        // Preserve the badge across the short gap from a thin edge to the control.
        cache.on_edge = seed.is_some();
        let in_gap = seed.is_none()
            && cache
                .anchor
                .is_some_and(|anchor| anchor.distance(pointer) <= 12.);
        if !in_gap {
            if !same_seed {
                cache.plan = None;
                cache.badge = None;
            }
            cache.seed = seed;
            if !same_seed
                || cache
                    .anchor
                    .is_none_or(|anchor| anchor.distance(pointer) > 26.)
            {
                cache.anchor = Some(pointer);
            }
        }
        cache.pointer = Some(pointer);
    }
    if cache.plan.is_none() || cache.mode != mode {
        cache.plan = cache
            .seed
            .as_ref()
            .and_then(|seed| preview(w, rect, &feature.id, &mode, seed));
        cache.mode = mode;
    }
    let mut captured = false;
    if let Some(plan) = &cache.plan {
        if on_badge || cache.on_edge {
            paint(w, ui, rect, plan);
        }
        let anchor = cache.anchor.unwrap_or(pointer);
        let size = egui::vec2(168f32.min(rect.width()), 26f32.min(rect.height()));
        let x = (anchor.x - 4.).clamp(rect.left(), (rect.right() - size.x).max(rect.left()));
        let y = if anchor.y + 8. + size.y <= rect.bottom() {
            anchor.y + 8.
        } else {
            (anchor.y - 8. - size.y).max(rect.top())
        };
        let default = Rect::from_min_size(egui::pos2(x, y), size);
        cache.badge = None;
        if let Some(area) = controls::overlay_rect(BADGE, ROLE, rect, default) {
            let mut child = ui.new_child(egui::UiBuilder::new().id_salt(BADGE).max_rect(area));
            let button = controls::button(
                &mut child,
                BADGE,
                ROLE,
                &["viewport-overlay", "compact"],
                &format!("{}  ⇄", plan.element.label),
                true,
                false,
            )
            .on_hover_text(plan.element.value.as_deref().unwrap_or_default());
            cache.badge = Some(button.rect);
            captured = button.rect.contains(pointer);
            if button.clicked() {
                edge_action(w, &plan.element.commands[0]);
                ui.ctx().request_repaint();
            }
        }
    }
    w.edge_hover = cache;
    captured
}
