//! Direct profile inputs. Display samples are used for picking and feedback only.
//! The draft stores evaluated region members and associative topology references.
use super::{Change, action, command};
use crate::{Workspace, overlay_depth, sketch, viewport as model};
use bevy::math::DVec3;
use bevy_egui::egui::{self, Pos2, Rect};
use cad_core::{
    ExtrudeEnd, Feature, NodeRef, Operation, PlacedPart,
    protocol::{EvaluatedSketch, Evaluation},
    reference_geometry::{EdgePointSource, PointSource},
};
use std::collections::BTreeMap;

const TARGET_SLOT: &str = "extrude-viewport-target";

pub(crate) fn input_feature(w: &Workspace) -> Option<&Feature> {
    if w.dialog.as_deref() != Some("feature")
        || w.tool_checkpoint.is_none()
        || w.fields.contains_key("feature-source-invalid")
    {
        return None;
    }
    let id = w.fields.get("feature-editor-id")?;
    w.session
        .visible()
        .parts
        .get(w.view.active_part.as_ref()?)?
        .features
        .iter()
        .find(|f| &f.id == id && matches!(f.operation, Operation::Extrude { .. }))
}

// A frozen tool checkpoint serves many hit and occlusion queries per pointer
// move. Fingerprint each transaction epoch once, preserving the full worker
// identity check without serializing the entire project for each query.
thread_local! {
    static INPUT_IDENTITY: std::cell::RefCell<Option<(String,cad_core::protocol::RequestIdentity)>> = const {std::cell::RefCell::new(None)};
}
fn accepted(session: &cad_core::Session, e: &Evaluation) -> bool {
    INPUT_IDENTITY.with_borrow_mut(|cache| {
        if cache.as_ref().is_none_or(|(epoch, identity)| {
            epoch != session.source_epoch()
                || identity.backend != session.backend
                || identity.generation != session.generation
        }) {
            *cache = Some((session.source_epoch().into(), session.identity()));
        }
        cache.as_ref().is_some_and(|(_, id)| *id == e.identity)
    })
}

/// Use only an accepted source result whose upstream stage was built. Invalid
/// downstream features can be repaired using the valid preceding history stage.
pub(crate) fn input_evaluation(w: &Workspace) -> Option<&Evaluation> {
    let f = input_feature(w)?;
    let part = w
        .session
        .visible()
        .parts
        .get(w.view.active_part.as_ref()?)?;
    let previous = part
        .features
        .iter()
        .take_while(|prior| prior.id != f.id)
        .last()?;
    for session in w.tool_checkpoint.iter().chain(std::iter::once(&w.session)) {
        let evaluation = match &session.draft {
            Some(draft) => draft.evaluation.as_ref(),
            None => session.evaluation.as_ref(),
        };
        let Some(e) = evaluation else {
            continue;
        };
        if !accepted(session, e) {
            continue;
        }
        let Some(i) = active_instance(w, e) else {
            continue;
        };
        if cad_core::history::stage(e, cad_core::history::mesh_key(i), &previous.id)
            .is_some_and(|stage| stage.unavailable_reason.is_none())
        {
            return Some(e);
        }
    }
    None
}

pub(crate) fn cancel_replacement(w: &mut Workspace) -> bool {
    let Some(f) = input_feature(w) else {
        return false;
    };
    let Some((second, vertex)) = slot(w, f) else {
        return false;
    };
    let Operation::Extrude { options, .. } = &f.operation else {
        return false;
    };
    let complete = if second {
        options.direction2.as_ref().is_some_and(|e| {
            if vertex {
                e.target_vertex.is_some() || e.target_point.is_some()
            } else {
                e.target.is_some()
            }
        })
    } else if vertex {
        options.target_vertex.is_some() || options.target_point.is_some()
    } else {
        options.target.is_some()
    };
    complete && w.fields.remove(TARGET_SLOT).is_some()
}

pub(crate) fn arm_target(w: &mut Workspace, second: bool) {
    if let Some(id) = w.fields.get("feature-editor-generation") {
        w.fields
            .insert(TARGET_SLOT.into(), format!("{id}/{}", u8::from(second)));
        w.selection_popup = None;
        w.candidates.clear();
        w.status = "Pick the end reference on the model".into();
    }
}

fn slot(w: &Workspace, f: &Feature) -> Option<(bool, bool)> {
    let Operation::Extrude { options, .. } = &f.operation else {
        return None;
    };
    let mut available = vec![(
        false,
        &options.end,
        options.target.is_some(),
        options.target_vertex.is_some() || options.target_point.is_some(),
    )];
    if let Some(end) = &options.direction2 {
        available.push((
            true,
            &end.end,
            end.target.is_some(),
            end.target_vertex.is_some() || end.target_point.is_some(),
        ));
    }
    let mut missing = None;
    for (second, end, face, vertex) in available {
        if !matches!(end, ExtrudeEnd::UpToFace | ExtrudeEnd::UpToVertex) {
            continue;
        }
        let vertex_mode = end == &ExtrudeEnd::UpToVertex;
        if w.fields.get("feature-editor-generation").is_some_and(|id| {
            w.fields
                .get(TARGET_SLOT)
                .is_some_and(|s| s == &format!("{id}/{}", u8::from(second)))
        }) {
            return Some((second, vertex_mode));
        }
        if !(if vertex_mode { vertex } else { face }) {
            missing.get_or_insert((second, vertex_mode));
        }
    }
    missing
}
pub(crate) fn target_direction(w: &Workspace) -> Option<bool> {
    input_feature(w)
        .and_then(|f| slot(w, f))
        .map(|(second, _)| second)
}
pub(crate) fn picking_target(w: &Workspace) -> bool {
    target_direction(w).is_some()
}

fn active_instance<'a>(w: &Workspace, e: &'a Evaluation) -> Option<&'a PlacedPart> {
    model::display_instances(w, e).into_iter().find(|i| {
        w.view.active_part.as_ref() == Some(&i.definition)
            && w.view
                .active_instance
                .as_ref()
                .is_none_or(|id| i.path.contains(id) || i.path.is_empty() && id == &i.definition)
            && model::visible(w, i)
            && crate::display_states_ui::pickable(w, &i.path)
    })
}

#[derive(Clone)]
enum Target {
    Face(model::Selection),
    Vertex(PointSource, [f64; 3]),
}

fn target_at(
    w: &Workspace,
    e: &Evaluation,
    i: &PlacedPart,
    rect: Rect,
    p: Pos2,
    vertex: bool,
    click: bool,
) -> Option<Target> {
    let f = input_feature(w)?;
    let mut work = crate::sketch_overlay::DEPTH_WORK;
    let mut visible = |world| {
        if click {
            overlay_depth::click_visible(w, e, rect, world)
        } else {
            model::visible_at(w, rect, world, &mut work)
        }
    };
    let preceding = |id: &str| {
        w.session.visible().parts[&i.definition]
            .features
            .iter()
            .take_while(|prior| prior.id != f.id)
            .any(|prior| prior.id == id)
    };
    if !vertex {
        let mut faces = model::pick(w, rect, p);
        faces.sort_by(|a, b| a.distance.total_cmp(&b.distance));
        return faces
            .into_iter()
            .filter(|s| {
                s.path == i.path
                    && matches!(s.reference.topology.as_str(), "face" | "plane")
                    && preceding(&s.reference.producer.node)
                    && s.body.as_ref().is_none_or(|body| preceding(&body.node))
            })
            .find(|s| visible(s.point))
            .map(Target::Face);
    }
    let eye = DVec3::from_array(model::ray(w, rect, p).0);
    let mut near: Vec<(f32, f64, PointSource, [f64; 3])> = Vec::new();
    let mut offer = |world: [f64; 3], make: &dyn Fn() -> PointSource| {
        if let Some(pixel) = model::project(w, rect, world) {
            let distance = pixel.distance(p);
            if distance <= 9. {
                near.push((
                    distance,
                    eye.distance(DVec3::from_array(world)),
                    make(),
                    world,
                ));
            }
        }
    };
    for mesh in model::selection_meshes(w, e, i) {
        for edge in &mesh.edges {
            let Some(native) = edge.native.as_ref().filter(|native| {
                native.version == cad_core::protocol::NATIVE_SELECTION_VERSION
                    && native.vertices[0] != native.vertices[1]
            }) else {
                continue;
            };
            for (idx, point) in native.endpoints.into_iter().enumerate() {
                offer(model::placed(point, i), &|| PointSource::EdgePoint {
                    source: Box::new(EdgePointSource {
                        body: edge
                            .body
                            .clone()
                            .unwrap_or_else(|| edge.reference.producer.clone()),
                        reference: edge.reference.clone(),
                        parameter: cad_core::Expr(idx.to_string()),
                    }),
                });
            }
        }
    }
    for prior in w.session.visible().parts[&i.definition]
        .features
        .iter()
        .take_while(|prior| prior.id != f.id)
    {
        let driving =
            matches!(&f.operation, Operation::Extrude {profile,..} if profile.node==prior.id);
        if !driving && !sketch::visible_sketch(w, i, prior) {
            continue;
        }
        let Some(geometry) =
            e.sketches
                .get(&format!("{}::{}", cad_core::history::mesh_key(i), prior.id))
        else {
            continue;
        };
        for (point, uv) in &geometry.points {
            offer(sketch::world(&geometry.plane, &i.matrix, *uv), &|| {
                PointSource::SketchPoint {
                    profile: NodeRef {
                        definition: i.definition.clone(),
                        node: prior.id.clone(),
                    },
                    point: point.clone(),
                }
            });
        }
    }
    near.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
    let mut examined = std::collections::BTreeSet::new();
    near.into_iter()
        .find(|(_, _, _, world)| examined.insert(world.map(f64::to_bits)) && visible(*world))
        .map(|(_, _, source, world)| Target::Vertex(source, world))
}

fn assign(w: &mut Workspace, mut f: Feature, second: bool, target: Target) {
    let Operation::Extrude { options, .. } = &mut f.operation else {
        return;
    };
    let (face, body, vertex, point) = if second {
        let Some(end) = options.direction2.as_mut() else {
            return;
        };
        (
            &mut end.target,
            &mut end.target_body,
            &mut end.target_vertex,
            &mut end.target_point,
        )
    } else {
        (
            &mut options.target,
            &mut options.target_body,
            &mut options.target_vertex,
            &mut options.target_point,
        )
    };
    match target {
        Target::Face(s) => {
            *face = Some(s.reference);
            *body = s.body;
            *vertex = None;
            *point = None;
        }
        Target::Vertex(source, _) => {
            *face = None;
            *body = None;
            match source {
                PointSource::SketchPoint { profile, point: id } => {
                    *vertex = Some((profile, id));
                    *point = None;
                }
                source => {
                    *vertex = None;
                    *point = Some(Box::new(source));
                }
            }
        }
    }
    w.fields.remove(TARGET_SLOT);
    w.candidates.clear();
    w.selection_popup = None;
    w.confirm_requested = false;
    w.json = serde_json::to_string_pretty(&f).unwrap_or_default();
    crate::ui::preview_feature(w, f);
}

#[derive(Clone)]
struct Region {
    id: String,
    anchor: String,
    edges: Vec<[Pos2; 2]>,
    bounds: Rect,
    selected: bool,
    fill: Vec<[Pos2; 2]>,
    boundary: Vec<[Pos2; 2]>,
}

impl Region {
    // The same even-odd rule drives both hit testing and fill, including islands and holes.
    fn crossings(&self, y: f32, xs: &mut Vec<f32>) {
        xs.clear();
        for [a, b] in &self.edges {
            if (a.y > y) != (b.y > y) {
                xs.push(a.x + (y - a.y) * (b.x - a.x) / (b.y - a.y));
            }
        }
        xs.sort_by(f32::total_cmp);
    }
    fn contains(&self, p: Pos2) -> bool {
        if !self.bounds.contains(p) {
            return false;
        }
        let mut xs = Vec::new();
        self.crossings(p.y, &mut xs);
        xs.as_chunks::<2>()
            .0
            .iter()
            .any(|x| p.x >= x[0] && p.x < x[1])
    }
}

fn regions(
    w: &Workspace,
    geometry: &EvaluatedSketch,
    i: &PlacedPart,
    rect: Rect,
    profile: &NodeRef,
    selections: &[cad_core::contours::Selection],
) -> Vec<Region> {
    let mut paths = BTreeMap::new();
    for poly in geometry
        .polylines
        .iter()
        .filter(|poly| !poly.construction && poly.points.len() >= 2)
    {
        let Some(points) = poly
            .points
            .iter()
            .map(|p| model::project(w, rect, sketch::world(&geometry.plane, &i.matrix, *p)))
            .collect::<Option<Vec<_>>>()
        else {
            continue;
        };
        let mut edges: Vec<_> = points.windows(2).map(|p| [p[0], p[1]]).collect();
        if poly.closed && points.first() != points.last() {
            edges.push([points[points.len() - 1], points[0]]);
        }
        paths.insert(&poly.member, edges);
    }
    geometry
        .material_regions
        .iter()
        .filter_map(|r| {
            // An untraced legacy hole cannot safely be filled or hit-tested.
            if r.hole_members.iter().any(Vec::is_empty) {
                return None;
            }
            let anchor = r.outer_members.first()?.clone();
            let mut edges = vec![];
            let mut selected = false;
            for member in r
                .outer_members
                .iter()
                .chain(r.hole_members.iter().flatten())
            {
                edges.extend_from_slice(paths.get(member)?);
                selected |= selections
                    .iter()
                    .any(|s| &s.profile == profile && s.outer_members.contains(member));
            }
            let bounds = edges.iter().flatten().fold(Rect::NOTHING, |mut bounds, p| {
                bounds.extend_with(*p);
                bounds
            });
            (bounds.is_positive() && bounds.intersects(rect)).then_some(Region {
                id: format!("extrude.region.{}.{}", profile.node, anchor),
                anchor,
                edges,
                bounds,
                selected,
                fill: vec![],
                boundary: vec![],
            })
        })
        .collect()
}

fn clip_region(
    w: &Workspace,
    rect: Rect,
    region: &mut Region,
    world: &impl Fn(Pos2) -> Option<[f64; 3]>,
    work: &mut usize,
) {
    let bounds = region.bounds.intersect(rect);
    let spacing = 3.;
    let mut y = bounds.top().ceil() + 0.5;
    let mut spans = vec![];
    let mut xs = vec![];
    while y <= bounds.bottom() {
        region.crossings(y, &mut xs);
        for x in xs.as_chunks::<2>().0.iter() {
            let left = x[0].max(rect.left());
            let right = x[1].min(rect.right());
            if right > left
                && let (Some(a), Some(b)) =
                    (world(egui::pos2(left, y)), world(egui::pos2(right, y)))
            {
                spans.push([a, b]);
            }
        }
        y += spacing;
    }
    region.fill = overlay_depth::segments_batch(w, rect, spans, work);
    let lines = region
        .edges
        .iter()
        .filter_map(|[a, b]| Some([world(*a)?, world(*b)?]));
    region.boundary = overlay_depth::segments_batch(w, rect, lines, work);
}

fn region_style(w: &Workspace, region: &Region, hovered: bool) -> Option<(egui::Color32, f32)> {
    let state = if hovered {
        "hovered"
    } else if region.selected {
        "selected"
    } else {
        "available"
    };
    let props = w.theme.properties_for(&model::style_target(w).child(
        &region.id,
        "profile-region",
        &[],
        state,
    ));
    if crate::sketch_overlay::hidden(&props) {
        return None;
    }
    let color = props
        .get("color")
        .and_then(|s| crate::ui::color(s))
        .unwrap_or_else(|| model::selection_color(w));
    let opacity = crate::presentation_controls::number(
        &props,
        "opacity",
        if hovered {
            0.42
        } else if region.selected {
            0.30
        } else {
            0.09
        },
    )
    .clamp(0., 1.);
    if opacity <= 0. {
        return None;
    }
    Some((color, opacity))
}

fn draw_region(w: &Workspace, ui: &egui::Ui, rect: Rect, region: &Region, hovered: bool) {
    let Some((color, opacity)) = region_style(w, region, hovered) else {
        return;
    };
    let painter = ui.painter().with_clip_rect(rect);
    let stroke = egui::Stroke::new(3., color.gamma_multiply(opacity));
    painter.extend(
        region
            .fill
            .iter()
            .map(|line| egui::Shape::line_segment(*line, stroke)),
    );
    if hovered || region.selected {
        painter.extend(
            region
                .boundary
                .iter()
                .map(|line| egui::Shape::line_segment(*line, egui::Stroke::new(1.5, color))),
        );
    }
}

fn broken_endpoints(geometry: &EvaluatedSketch) -> Vec<[f64; 2]> {
    let Some(source) = &geometry.solved else {
        return vec![];
    };
    let Ok(topology) =
        cad_core::sketch_topology::analyze(source, cad_core::sketch_topology::DEFAULT_TOLERANCE_MM)
    else {
        return vec![];
    };
    let mut degree = BTreeMap::new();
    for contour in topology
        .contours
        .iter()
        .filter(|c| c.status != cad_core::sketch_topology::ContourStatus::Closed)
    {
        for member in &contour.members {
            if let Ok(curve) = cad_core::sketch_edit::effective_curve(source, member) {
                let controls = curve.points();
                for point in [controls.first(), controls.last()].into_iter().flatten() {
                    if let Some(point) = topology.point_aliases.get(*point) {
                        *degree.entry(point).or_insert(0) += 1;
                    }
                }
            }
        }
    }
    degree
        .into_iter()
        .filter(|(_, count)| *count != 2)
        .filter_map(|(point, _)| geometry.points.get(point).copied())
        .collect()
}

#[derive(Clone)]
struct RegionScene {
    regions: Vec<Region>,
    issues: Vec<Pos2>,
}

pub(crate) fn viewport(
    w: &mut Workspace,
    ui: &egui::Ui,
    rect: Rect,
    response: &egui::Response,
    blocked: bool,
) -> bool {
    let Some(f) = input_feature(w).filter(|_| w.settings.is_none()) else {
        return false;
    };
    let pointer = (!blocked).then(|| response.hover_pos()).flatten();
    let click = pointer.is_some() && response.clicked();
    let target = slot(w, f);
    let Some(e) = model::selection_evaluation(w) else {
        return target.is_some() && click;
    };
    let Some(i) = active_instance(w, e) else {
        return target.is_some() && click;
    };
    let (epoch, complete, exhausted) = overlay_depth::token(w, rect);
    if !complete {
        if !exhausted {
            ui.ctx().request_repaint();
        }
        if !click {
            if exhausted && pointer.is_some() {
                response.clone().on_hover_text(if target.is_some() {
                    "Click the end reference on the model."
                } else {
                    "Click inside a region or choose it under Contours."
                });
            }
            return false;
        }
    }
    if let Some((second, vertex)) = target {
        let Some(p) = pointer else {
            return false;
        };
        let key = ui.id().with((
            "extrude-target-at",
            rect.min.x.to_bits(),
            rect.min.y.to_bits(),
        ));
        let stamp = (
            epoch,
            p,
            vertex,
            i.path.clone(),
            f.id.clone(),
            w.session.source_epoch().to_owned(),
        );
        let saved = ui.ctx().data(|d| {
            d.get_temp::<(
                (u64, Pos2, bool, Vec<String>, String, String),
                Option<Target>,
            )>(key)
        });
        let aim = if click {
            target_at(w, e, i, rect, p, vertex, true)
        } else if let Some((_, target)) = saved.filter(|(s, _)| s == &stamp) {
            target
        } else {
            let target = target_at(w, e, i, rect, p, vertex, false);
            ui.ctx()
                .data_mut(|d| d.insert_temp(key, (stamp, target.clone())));
            target
        };
        if let Some(target) = aim {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
            match &target {
                Target::Vertex(_, point) => {
                    if let Some(pixel) = model::project(w, rect, *point) {
                        ui.painter().with_clip_rect(rect).circle_stroke(
                            pixel,
                            5.,
                            egui::Stroke::new(2., model::selection_color(w)),
                        );
                    }
                }
                Target::Face(hit) => model::candidate_feedback(w, ui, rect, hit, false),
            }
            if click {
                assign(w, f.clone(), second, target);
            }
        } else if click {
            w.status = if vertex {
                "Pick a visible model or sketch vertex"
            } else {
                "Pick a face or datum plane on the active part"
            }
            .into();
        }
        return click;
    }
    let Operation::Extrude {
        profile, options, ..
    } = &f.operation
    else {
        return false;
    };
    if options.thin.is_some() {
        return false;
    }
    let Some(geometry) = e.sketches.get(&format!(
        "{}::{}",
        cad_core::history::mesh_key(i),
        profile.node
    )) else {
        return false;
    };
    let world = |p: Pos2| {
        sketch::hit_plane(w, rect, p, &geometry.plane, &i.matrix)
            .map(|uv| sketch::world(&geometry.plane, &i.matrix, uv))
    };
    let mut work = crate::sketch_overlay::DEPTH_WORK;
    let stamp = (
        epoch,
        complete,
        w.session.source_epoch().to_owned(),
        profile.clone(),
        i.path.clone(),
    );
    let key = ui.id().with((
        "extrude-region-scene",
        rect.min.x.to_bits(),
        rect.min.y.to_bits(),
    ));
    let saved = ui.ctx().data(|d| {
        d.get_temp::<(
            (u64, bool, String, NodeRef, Vec<String>),
            std::sync::Arc<RegionScene>,
        )>(key)
    });
    let scene = if let Some((_, scene)) = saved.filter(|(s, _)| s == &stamp) {
        scene
    } else {
        let mut regions = regions(w, geometry, i, rect, profile, &options.contours);
        let mut depth_work = crate::sketch_overlay::DEPTH_WORK;
        let issues = complete
            .then(|| broken_endpoints(geometry))
            .into_iter()
            .flatten()
            .filter_map(|uv| {
                let world = sketch::world(&geometry.plane, &i.matrix, uv);
                model::visible_at(w, rect, world, &mut depth_work)
                    .then(|| model::project(w, rect, world))
                    .flatten()
            })
            .collect();
        if complete {
            for region in &mut regions {
                clip_region(w, rect, region, &world, &mut depth_work);
            }
        }
        let scene = std::sync::Arc::new(RegionScene { regions, issues });
        ui.ctx()
            .data_mut(|d| d.insert_temp(key, (stamp, scene.clone())));
        scene
    };
    let regions = &scene.regions;
    let props = w.theme.properties_for(&model::style_target(w).child(
        "extrude.open-end",
        "profile-open-end",
        &[],
        "error",
    ));
    if !crate::sketch_overlay::hidden(&props) {
        let color = props
            .get("color")
            .and_then(|s| crate::ui::color(s))
            .unwrap_or_else(|| ui.visuals().error_fg_color);
        for p in &scene.issues {
            ui.painter()
                .with_clip_rect(rect)
                .circle_stroke(*p, 4., egui::Stroke::new(2., color));
        }
    }
    let hit = pointer.and_then(|p| {
        let index = regions.iter().position(|region| {
            region.contains(p)
                && region_style(w, region, false).is_some()
                && region_style(w, region, true).is_some()
        })?;
        let world = world(p)?;
        let visible = if click {
            overlay_depth::click_visible(w, e, rect, world)
        } else {
            model::visible_at(w, rect, world, &mut work)
        };
        visible.then_some(index)
    });
    for (index, region) in regions.iter().enumerate() {
        draw_region(w, ui, rect, region, Some(index) == hit);
    }
    if let Some(index) = hit {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        if click {
            let cmd = command(
                w,
                f,
                Change::Pick {
                    profile: profile.clone(),
                    member: regions[index].anchor.clone(),
                },
            );
            return action(w, &cmd);
        }
    }
    false
}
