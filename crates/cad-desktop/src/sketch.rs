//! Sketch tools operate in the model viewport over the shared pending document.
use crate::{Workspace, viewport};
use bevy::math::DVec3;
use bevy_egui::egui::{self, Color32, Stroke};
use cad_core::{commands::Command, protocol::EvaluatedSketch, *};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Polyline,
    Rectangle,
    Circle,
    CenterRectangle,
    ThreePointRectangle,
    ThreePointCenterRectangle,
    Parallelogram,
    PerimeterCircle,
    CenterArc,
    TangentArc,
    CenterSlot,
    MidpointLine,
    Centerline,
    Polygon,
    Arc,
    Ellipse,
    Slot,
    Spline,
    InterpolatingSpline,
    Nurbs,
    Conic,
    Parabola,
    EllipseArc,
    ArcSlot,
    Point,
    Select,
}
impl Tool {
    pub fn is_spline(self) -> bool {
        matches!(self, Self::Spline | Self::InterpolatingSpline | Self::Nurbs)
    }
    pub fn conic_kind(self) -> Option<cad_core::sketch_conic::Kind> {
        Some(match self {
            Self::Conic => cad_core::sketch_conic::Kind::Conic,
            Self::Parabola => cad_core::sketch_conic::Kind::Parabola,
            Self::EllipseArc => cad_core::sketch_conic::Kind::EllipseArc,
            Self::ArcSlot => cad_core::sketch_conic::Kind::ArcSlot,
            _ => return None,
        })
    }
}
pub struct EditingSketch {
    pub definition: Id,
    pub feature: Id,
    pub sketch_id: Id,
    pub name: String,
    pub plane: Plane,
    pub reference: Option<TopologyRef>,
    pub reference_body: Option<NodeRef>,
    pub matrix: [[f64; 4]; 4],
    pub choosing_plane: bool,
    pub tool: Tool,
    pub points: Vec<[f64; 2]>,
    pub keys: Vec<Id>,
    pub circle: Option<Expr>,
    pub closed: bool,
    pub anchor: Option<[f64; 2]>,
    pub hover: Option<[f64; 2]>,
    pub selected: Option<usize>,
    pub snap: bool,
    pub grid: f64,
    pub constraints: Vec<SketchConstraint>,
    pub error: Option<String>,
    pub placement_error: Option<String>,
    pub degrees_of_freedom: Option<usize>,
    pub feedback: crate::sketch_relations_ui::Feedback,
    pub members: BTreeMap<Id, SketchMember>,
    pub chain: Vec<Id>,
    pub clicks: Vec<[f64; 2]>,
    /// A multi-click tool only merges the endpoints explicitly snapped on that click.
    pub click_join: Vec<bool>,
    pub construction: bool,
    pub conversions: BTreeMap<Id, cad_core::sketch_edit::Conversion>,
    pub offsets: BTreeMap<Id, cad_core::sketch_edit::Offset>,
    pub fillets: BTreeMap<Id, cad_core::sketch_edit::Fillet>,
    pub chamfers: BTreeMap<Id, cad_core::sketch_edit::Chamfer>,
    pub transforms: BTreeMap<Id, cad_core::sketch_transform::Pattern>,
    pub splines: BTreeMap<Id, cad_core::sketch_spline::Definition>,
    pub conics: BTreeMap<Id, cad_core::sketch_conic::Definition>,
    pub texts: BTreeMap<Id, cad_core::sketch_text::Definition>,
    pub equations: BTreeMap<Id, cad_core::sketch_equation::Definition>,
    pub blocks: BTreeMap<Id, cad_core::sketch_blocks::Definition>,
    pub block_instances: BTreeMap<Id, cad_core::sketch_blocks::Instance>,
    pub dimension_groups: BTreeMap<Id, cad_core::sketch_dimensions::Group>,
    pub conic_rho: Expr,
    pub spline_degree: usize,
    pub spline_parameterization: cad_core::sketch_spline::Parameterization,
    pub edit: Option<crate::sketch_edit_ui::Editor>,
    pub polygon_sides: usize,
    pub clockwise: bool,
    pub selected_member: Option<Id>,
    pub history: Vec<Sketch>,
    pub selection: Vec<cad_core::sketch_actions::SketchEntity>,
    pub dimension: Option<crate::sketch_ui::DimensionEditor>,
    pub dimension_pick: Option<crate::sketch_ui::DimensionCheckpoint>,
    pub dimension_model: Option<viewport::Selection>,
    pub last_valid: Option<Sketch>,
    pub drag: Option<crate::sketch_handles::DragGesture>,
}
fn identity() -> [[f64; 4]; 4] {
    std::array::from_fn(|i| std::array::from_fn(|j| if i == j { 1. } else { 0. }))
}
fn transform(matrix: &[[f64; 4]; 4], p: [f64; 3], vector: bool) -> [f64; 3] {
    std::array::from_fn(|i| {
        matrix[i][0] * p[0]
            + matrix[i][1] * p[1]
            + matrix[i][2] * p[2]
            + if vector { 0. } else { matrix[i][3] }
    })
}
fn frame(w: &Workspace, definition: &str) -> [[f64; 4]; 4] {
    let evaluated = w
        .session
        .draft
        .as_ref()
        .and_then(|d| d.preview_evaluation())
        .or(w.session.evaluation.as_ref());
    let selected_feature = match &w.context_selection {
        Some(cad_core::selection_actions::SelectionTarget::Feature {
            definition,
            feature,
        }) => Some((definition.as_str(), feature.as_str())),
        None => w
            .view
            .active_part
            .as_deref()
            .zip(w.view.active_node.as_deref()),
        _ => None,
    };
    let feature_path = selected_feature
        .filter(|(owner, feature)| {
            *owner == definition
                && w.session
                    .visible()
                    .parts
                    .get(definition)
                    .is_some_and(|part| {
                        part.features.iter().any(|candidate| {
                            candidate.id == *feature
                                && matches!(candidate.operation, Operation::Profile { .. })
                        })
                    })
        })
        .and(w.selected_feature_path.as_ref());
    let selected_path = w
        .selected
        .as_ref()
        .filter(|selection| selection.definition == definition)
        .map(|selection| &selection.path)
        .or(feature_path);
    let matches = |i: &&PlacedPart| {
        i.definition == definition
            && selected_path.is_none_or(|path| path == &i.path)
            && w.view
                .active_instance
                .as_ref()
                .is_none_or(|id| i.path.contains(id) || (i.path.is_empty() && id == definition))
    };
    evaluated
        .and_then(|e| {
            e.instances.iter().find(matches).or_else(|| {
                e.instances
                    .iter()
                    .find(|i| selected_path.is_none() && i.definition == definition)
            })
        })
        .map(|i| i.matrix)
        .or_else(|| {
            w.session
                .visible()
                .placed_parts()
                .ok()
                .and_then(|instances| {
                    instances
                        .iter()
                        .find(matches)
                        .or_else(|| {
                            instances
                                .iter()
                                .find(|i| selected_path.is_none() && i.definition == definition)
                        })
                        .map(|i| i.matrix)
                })
        })
        .unwrap_or_else(identity)
}
pub(crate) fn world(plane: &Plane, matrix: &[[f64; 4]; 4], p: [f64; 2]) -> [f64; 3] {
    transform(
        matrix,
        std::array::from_fn(|i| plane.origin[i] + plane.u[i] * p[0] + plane.v[i] * p[1]),
        false,
    )
}
pub(crate) fn hit_plane(
    w: &Workspace,
    rect: egui::Rect,
    p: egui::Pos2,
    plane: &Plane,
    matrix: &[[f64; 4]; 4],
) -> Option<[f64; 2]> {
    let (origin, dir) = viewport::ray(w, rect, p);
    let origin = DVec3::from_array(origin);
    let dir = DVec3::from_array(dir);
    let anchor = DVec3::from_array(transform(matrix, plane.origin, false));
    let u = DVec3::from_array(transform(matrix, plane.u, true));
    let v = DVec3::from_array(transform(matrix, plane.v, true));
    let normal = u.cross(v);
    let den = normal.dot(dir);
    if den.abs() < 1e-10 {
        return None;
    }
    let t = normal.dot(anchor - origin) / den;
    if !t.is_finite() || t <= 0. {
        return None;
    }
    let point = origin + dir * t - anchor;
    let uu = u.dot(u);
    let vv = v.dot(v);
    let uv = u.dot(v);
    let determinant = uu * vv - uv * uv;
    if determinant.abs() < 1e-12 {
        return None;
    }
    Some([
        (point.dot(u) * vv - point.dot(v) * uv) / determinant,
        (point.dot(v) * uu - point.dot(u) * uv) / determinant,
    ])
}
pub fn normal_to(w: &mut Workspace) {
    let Some(sketch) = &w.sketch else {
        return;
    };
    let normal = DVec3::from_array(transform(&sketch.matrix, sketch.plane.u, true))
        .cross(DVec3::from_array(transform(
            &sketch.matrix,
            sketch.plane.v,
            true,
        )))
        .normalize();
    if !normal.is_finite() {
        return;
    }
    let origin = DVec3::from_array(world(&sketch.plane, &sketch.matrix, [0.; 2]));
    let mut pose = crate::camera_views_ui::current(w);
    // Alignment preserves the useful point under the center of the viewport.
    // The plane's modeling origin does not define the display's framing center.
    let target = DVec3::from_array(pose.target_mm);
    let center = target - normal * (target - origin).dot(normal);
    pose.orbit_rad = [normal.x.atan2(-normal.y), normal.z.clamp(-1., 1.).asin()];
    pose.target_mm = center.to_array();
    pose.roll_rad = 0.;
    let mut points = active_framing_points(w);
    if points.is_empty() {
        points = support_framing_points(w);
    }
    crate::camera_views_ui::set_current(w, pose);
    viewport::frame_points(w, &points, Some(center.to_array()), true);
    w.pending_context_focus = false;
    crate::navigation::stop(w);
}
fn active_framing_points(w: &Workspace) -> Vec<[f64; 3]> {
    let Some(sketch) = &w.sketch else {
        return vec![];
    };
    cad_core::sketch_geometry::polylines(
        w.session.visible(),
        &sketch.definition,
        &display_definition(sketch),
    )
    .unwrap_or_default()
    .into_iter()
    .flat_map(|path| path.points)
    .chain(sketch.points.iter().copied())
    .map(|point| world(&sketch.plane, &sketch.matrix, point))
    .collect()
}
fn support_framing_points(w: &Workspace) -> Vec<[f64; 3]> {
    let Some(sketch) = &w.sketch else {
        return vec![];
    };
    let Some(reference) = sketch
        .reference
        .as_ref()
        .filter(|reference| reference.topology == "face")
    else {
        return vec![];
    };
    viewport::occurrence_framing_points(
        w,
        &sketch.definition,
        &sketch.matrix,
        Some((reference, sketch.reference_body.as_ref())),
    )
}
pub fn fit_active(w: &mut Workspace) {
    let points = active_framing_points(w);
    if points.is_empty() {
        w.status = "The active sketch has no geometry to frame".into();
        return;
    }
    viewport::frame_points(w, &points, None, false);
    w.pending_context_focus = false;
}
pub fn fit_support(w: &mut Workspace) {
    let points = support_framing_points(w);
    if points.is_empty() {
        w.status = "Select a face-supported sketch to frame its support".into();
        return;
    }
    let target = sketch_plane_center(w, &points);
    viewport::frame_points(w, &points, target, false);
    w.pending_context_focus = false;
}
fn sketch_plane_center(w: &Workspace, points: &[[f64; 3]]) -> Option<[f64; 3]> {
    let sketch = w.sketch.as_ref()?;
    let min = points
        .iter()
        .map(|p| DVec3::from_array(*p))
        .fold(DVec3::splat(f64::INFINITY), DVec3::min);
    let max = points
        .iter()
        .map(|p| DVec3::from_array(*p))
        .fold(DVec3::splat(f64::NEG_INFINITY), DVec3::max);
    let normal = DVec3::from_array(transform(&sketch.matrix, sketch.plane.u, true))
        .cross(DVec3::from_array(transform(
            &sketch.matrix,
            sketch.plane.v,
            true,
        )))
        .normalize();
    let origin = DVec3::from_array(world(&sketch.plane, &sketch.matrix, [0.; 2]));
    let center = (min + max) * 0.5;
    Some((center - normal * (center - origin).dot(normal)).to_array())
}
pub fn fit_part_context(w: &mut Workspace) {
    let Some(sketch) = &w.sketch else {
        return;
    };
    let mut points =
        viewport::occurrence_framing_points(w, &sketch.definition, &sketch.matrix, None);
    points.extend(active_framing_points(w));
    if points.is_empty() {
        return;
    }
    let target = sketch_plane_center(w, &points);
    viewport::frame_points(w, &points, target, false);
    w.pending_context_focus = false;
}
pub(crate) fn selected_plane(w: &Workspace, definition: &str) -> Option<(Plane, TopologyRef)> {
    let selected = w.selected.as_ref().filter(|s| s.definition == definition);
    if let Some(node) = selected
        .filter(|s| s.reference.topology == "plane")
        .map(|s| s.reference.producer.node.as_str())
        .or_else(|| {
            selected
                .is_none()
                .then_some(w.view.active_node.as_deref())
                .flatten()
        })
        && let Some(plane) = crate::reference_ui::evaluated_plane(w, definition, node)
    {
        return Some(plane);
    }
    let selected = selected.filter(|s| s.reference.topology == "face")?;
    let evaluated = viewport::selection_evaluation(w)?;
    let instance = viewport::display_instances(w, evaluated)
        .into_iter()
        .find(|i| i.path == selected.path && i.definition == selected.definition)?;
    let faces = viewport::display_meshes(w, evaluated, instance)
        .into_iter()
        .flat_map(|mesh| &mesh.faces)
        .filter(|f| {
            f.reference.producer == selected.reference.producer
                && f.reference.role == selected.reference.role
                && f.reference.topology == selected.reference.topology
                && f.body == selected.body
        })
        .collect::<Vec<_>>();
    let [face] = faces.as_slice() else {
        return None;
    };
    face.plane
        .clone()
        .map(|plane| (plane, selected.reference.clone()))
}
pub fn begin(w: &mut Workspace, existing: Option<Feature>) {
    if w.fields.contains_key("exploded-view-editor")
        || w.fields.contains_key("assembly-inspection-editor")
        || w.fields.contains_key("assembly-weld-editor")
    {
        w.status = "Confirm or cancel the current assembly operation before sketching".into();
        return;
    }
    crate::exploded_views_ui::collapse_for_modeling(w);
    crate::history::latest(w);
    if w.sketch.is_some() {
        w.status = "Finish or discard the current sketch before starting another".into();
        return;
    }
    if w.tool_checkpoint.is_some() {
        w.status = "Confirm or cancel the current feature before starting a sketch".into();
        return;
    }
    let Some(definition) = w.view.active_part.clone() else {
        w.status = "Create a part to start sketching".into();
        return;
    };
    if w.session
        .visible()
        .libraries
        .values()
        .any(|l| l.pinned_definitions.contains_key(&definition))
    {
        w.status = "Make this linked instance unique before editing its sketches".into();
        return;
    }
    let matrix = frame(w, &definition);
    let mut scratch = EditingSketch {
        definition: definition.clone(),
        feature: id(),
        sketch_id: id(),
        name: format!(
            "Sketch{}",
            w.session.visible().parts[&definition]
                .features
                .iter()
                .filter(|f| matches!(f.operation, Operation::Profile { .. }))
                .count()
                + 1
        ),
        plane: Plane::default(),
        reference: None,
        reference_body: None,
        matrix,
        choosing_plane: true,
        tool: Tool::Select,
        points: vec![],
        keys: vec![],
        circle: None,
        closed: false,
        anchor: None,
        hover: None,
        selected: None,
        snap: true,
        grid: 1.,
        constraints: vec![],
        error: None,
        placement_error: None,
        degrees_of_freedom: None,
        feedback: crate::sketch_relations_ui::Feedback::default(),
        members: BTreeMap::new(),
        chain: vec![],
        clicks: vec![],
        click_join: vec![],
        construction: false,
        conversions: BTreeMap::new(),
        offsets: BTreeMap::new(),
        fillets: BTreeMap::new(),
        chamfers: BTreeMap::new(),
        transforms: BTreeMap::new(),
        splines: BTreeMap::new(),
        conics: BTreeMap::new(),
        texts: BTreeMap::new(),
        equations: BTreeMap::new(),
        blocks: BTreeMap::new(),
        block_instances: BTreeMap::new(),
        dimension_groups: BTreeMap::new(),
        conic_rho: "0.5".into(),
        spline_degree: 3,
        spline_parameterization: cad_core::sketch_spline::Parameterization::Chord,
        edit: None,
        polygon_sides: 6,
        clockwise: false,
        selected_member: None,
        history: vec![],
        selection: vec![],
        dimension: None,
        dimension_pick: None,
        dimension_model: None,
        last_valid: None,
        drag: None,
    };
    if let Some(feature) = existing {
        if let Operation::Profile {
            profile,
            plane,
            plane_reference,
            reference_body,
        } = &feature.operation
        {
            scratch.feature = feature.id.clone();
            scratch.name = feature.name.clone();
            scratch.plane = plane.clone();
            scratch.reference = plane_reference.clone();
            scratch.reference_body = reference_body.clone();
            scratch.choosing_plane = false;
            scratch.closed = true;
            scratch.tool = Tool::Select;
            let part = &w.session.visible().parts[&definition];
            match profile {
                Profile::Sketch { sketch } => {
                    let source = &part.sketches[sketch];
                    scratch.sketch_id = sketch.clone();
                    scratch.constraints = source.constraints.clone();
                    scratch.keys = source.points.keys().cloned().collect();
                    scratch.points = scratch.keys.iter().map(|id| source.points[id]).collect();
                    scratch.members = source.members.clone();
                    scratch.conversions = source.conversions.clone();
                    scratch.offsets = source.offsets.clone();
                    scratch.fillets = source.fillets.clone();
                    scratch.chamfers = source.chamfers.clone();
                    scratch.transforms = source.transforms.clone();
                    scratch.splines = source.splines.clone();
                    scratch.conics = source.conics.clone();
                    scratch.texts = source.texts.clone();
                    scratch.equations = source.equations.clone();
                    scratch.blocks = source.blocks.clone();
                    scratch.block_instances = source.block_instances.clone();
                    scratch.dimension_groups = source.dimension_groups.clone();
                    for pair in source
                        .profile_order
                        .iter()
                        .zip(source.profile_order.iter().cycle().skip(1))
                        .take(source.profile_order.len())
                    {
                        scratch.members.insert(
                            id(),
                            SketchMember {
                                construction: false,
                                curve: SketchCurve::Line {
                                    start: pair.0.clone(),
                                    end: pair.1.clone(),
                                },
                            },
                        );
                    }
                    if source.profile_order.is_empty() {
                        for segment in &source.segments {
                            scratch.members.insert(
                                id(),
                                SketchMember {
                                    construction: false,
                                    curve: SketchCurve::Line {
                                        start: segment[0].clone(),
                                        end: segment[1].clone(),
                                    },
                                },
                            );
                        }
                    }
                    for (member_id, circle) in &source.circles {
                        scratch.members.insert(
                            member_id.clone(),
                            SketchMember {
                                construction: false,
                                curve: SketchCurve::Circle {
                                    center: circle.center.clone(),
                                    radius: circle.radius.clone(),
                                },
                            },
                        );
                    }
                }
                Profile::Polygon { points } => scratch.points = points.clone(),
                Profile::Circle { center, radius } => {
                    scratch.points = vec![*center];
                    scratch.circle = Some(radius.clone());
                }
            }
            if let Some(geometry) = w
                .session
                .draft
                .as_ref()
                .and_then(|d| d.preview_evaluation())
                .or(w.session.evaluation.as_ref())
                .and_then(|e| {
                    let key = e
                        .instances
                        .iter()
                        .find(|i| {
                            i.definition == definition
                                && w.view
                                    .active_instance
                                    .as_ref()
                                    .is_none_or(|id| i.path.contains(id))
                        })
                        .map(|i| {
                            if i.mesh_key.is_empty() {
                                &i.definition
                            } else {
                                &i.mesh_key
                            }
                        })
                        .unwrap_or(&definition);
                    e.sketches.get(&format!("{key}::{}", feature.id))
                })
            {
                scratch.plane = geometry.plane.clone();
                if !scratch.keys.is_empty() {
                    scratch.points = scratch
                        .keys
                        .iter()
                        .map(|id| geometry.points.get(id).copied().unwrap_or([0.; 2]))
                        .collect();
                }
            }
        }
    } else if let Some((plane, reference)) = selected_plane(w, &definition) {
        scratch.plane = plane;
        scratch.reference = Some(reference.clone());
        scratch.reference_body = if reference.topology == "plane" {
            Some(reference.producer.clone())
        } else {
            w.selected
                .as_ref()
                .map(|selected| crate::reference_ui::selected_body(w, selected))
        };
        scratch.choosing_plane = false;
    }
    while scratch.keys.len() < scratch.points.len() {
        scratch.keys.push(format!("p{}", scratch.keys.len()));
    }
    if scratch.members.is_empty() && !scratch.points.is_empty() {
        if let Some(radius) = scratch.circle.take() {
            scratch.members.insert(
                id(),
                SketchMember {
                    construction: false,
                    curve: SketchCurve::Circle {
                        center: scratch.keys[0].clone(),
                        radius,
                    },
                },
            );
        } else {
            for i in 0..scratch.keys.len() {
                scratch.members.insert(
                    id(),
                    SketchMember {
                        construction: false,
                        curve: SketchCurve::Line {
                            start: scratch.keys[i].clone(),
                            end: scratch.keys[(i + 1) % scratch.keys.len()].clone(),
                        },
                    },
                );
            }
        }
    }
    w.start_tool();
    w.sketch = Some(scratch);
    w.view.original = false;
    w.open_editor("sketch");
    w.command_tab = "Sketch".into();
    w.gesture = None;
    if !w.sketch.as_ref().is_some_and(|s| s.choosing_plane) {
        normal_to(w);
    } else if let Some(name) = w.active_plane.clone() {
        choose_origin(w, &name);
    }
    if w.sketch.as_ref().is_some_and(|s| !s.choosing_plane) {
        solve(w);
    }
    w.status = if w.sketch.as_ref().is_some_and(|s| s.choosing_plane) {
        "Choose a plane or planar face for the sketch"
    } else {
        "Sketch active. Choose Line, Rectangle, or Circle"
    }
    .into();
}
pub(crate) fn choose(w: &mut Workspace, plane: Plane, reference: Option<TopologyRef>) {
    if reference.is_some()
        && let Some(selected) = &w.selected
    {
        w.view.active_instance = selected.path.last().cloned();
    }
    let matrix = w.sketch.as_ref().map(|s| frame(w, &s.definition));
    let context = w
        .selected
        .as_ref()
        .map(|selected| crate::reference_ui::selected_body(w, selected))
        .or_else(|| {
            w.sketch
                .as_ref()
                .and_then(|s| w.session.visible().parts.get(&s.definition))
                .and_then(|p| p.outputs.last().map(|node| p.reference(node)))
        });
    if let Some(s) = &mut w.sketch {
        s.plane = plane;
        if let Some(matrix) = matrix {
            s.matrix = matrix;
        }
        s.reference_body = reference.as_ref().and_then(|r| {
            if r.topology == "plane" {
                Some(r.producer.clone())
            } else {
                context.clone()
            }
        });
        s.reference = reference;
        s.choosing_plane = false;
        s.anchor = None;
    }
    normal_to(w);
    solve(w);
    w.status = "Sketch plane selected. Draw in the model viewport".into();
}
pub fn choose_origin(w: &mut Workspace, name: &str) {
    w.active_plane = Some(name.into());
    if w.sketch.as_ref().is_some_and(|s| s.choosing_plane) {
        choose(w, plane(name), None);
    }
}
pub fn tool_for_command(command: &str) -> Option<Tool> {
    Some(match command {
        "sketch.line" => Tool::Polyline,
        "sketch.rectangle" => Tool::Rectangle,
        "sketch.center-rectangle" => Tool::CenterRectangle,
        "sketch.three-point-rectangle" => Tool::ThreePointRectangle,
        "sketch.three-point-center-rectangle" => Tool::ThreePointCenterRectangle,
        "sketch.parallelogram" => Tool::Parallelogram,
        "sketch.perimeter-circle" => Tool::PerimeterCircle,
        "sketch.center-arc" => Tool::CenterArc,
        "sketch.tangent-arc" => Tool::TangentArc,
        "sketch.center-slot" => Tool::CenterSlot,
        "sketch.midpoint-line" => Tool::MidpointLine,
        "sketch.centerline" => Tool::Centerline,
        "sketch.polygon" => Tool::Polygon,
        "sketch.slot" => Tool::Slot,
        "sketch.circle" => Tool::Circle,
        "sketch.arc" => Tool::Arc,
        "sketch.ellipse" => Tool::Ellipse,
        "sketch.spline" => Tool::Spline,
        "sketch.interpolating-spline" => Tool::InterpolatingSpline,
        "sketch.nurbs" => Tool::Nurbs,
        "sketch.conic" => Tool::Conic,
        "sketch.parabola" => Tool::Parabola,
        "sketch.ellipse-arc" => Tool::EllipseArc,
        "sketch.arc-slot" => Tool::ArcSlot,
        "sketch.point" => Tool::Point,
        "sketch.select" => Tool::Select,
        _ => return None,
    })
}
pub fn set_tool(w: &mut Workspace, tool: Tool) {
    if w.sketch.as_ref().is_some_and(|s| s.edit.is_some()) {
        w.status = "Accept or cancel the current sketch operation first".into();
        return;
    }
    crate::sketch_handles::end(w);
    w.context_selection = None;
    w.fields.remove("smart-dimension-mode");
    if let Some(s) = &mut w.sketch {
        s.tool = tool;
        s.selection.clear();
        s.dimension = None;
        s.dimension_pick = None;
        s.dimension_model = None;
        w.selection_popup = None;
        s.chain.clear();
        s.clicks.clear();
        s.click_join.clear();
        s.anchor = None;
        w.manager_tree = false;
    }
}
pub(crate) fn restore(s: &mut EditingSketch, source: Sketch) {
    if let Some(editor) = s.edit.as_mut().and_then(|editor| editor.equation.as_mut()) {
        editor.refresh(&source);
    }
    if let Some(editor) = s
        .edit
        .as_mut()
        .and_then(|editor| editor.dimension_group.as_mut())
    {
        editor.refresh(&source);
    }
    if let Some(block) = s.edit.as_mut().and_then(|editor| editor.block.as_mut()) {
        block.refresh(&source);
    }
    if let Some(editor) = &mut s.edit
        && matches!(
            editor.kind,
            crate::sketch_edit_ui::Kind::Spline
                | crate::sketch_edit_ui::Kind::Conic
                | crate::sketch_edit_ui::Kind::Text
        )
    {
        editor.spline_points = source.points.clone();
    }
    s.keys = source.points.keys().cloned().collect();
    s.points = s.keys.iter().map(|id| source.points[id]).collect();
    s.members = source.members;
    s.conversions = source.conversions;
    s.offsets = source.offsets;
    s.fillets = source.fillets;
    s.chamfers = source.chamfers;
    s.transforms = source.transforms;
    s.splines = source.splines;
    s.conics = source.conics;
    s.texts = source.texts;
    s.equations = source.equations;
    s.blocks = source.blocks;
    s.block_instances = source.block_instances;
    s.dimension_groups = source.dimension_groups;
    s.constraints = source.constraints;
    s.closed = !s.members.is_empty();
    s.chain.clear();
    s.clicks.clear();
    s.click_join.clear();
    s.error = None;
    s.placement_error = None;
    s.degrees_of_freedom = None;
    s.dimension = None;
    s.dimension_pick = None;
    s.dimension_model = None;
    s.selection.clear();
    s.last_valid = None;
    s.feedback = crate::sketch_relations_ui::Feedback::default();
    s.drag = None;
}
pub fn undo_point(w: &mut Workspace) {
    if crate::sketch_handles::cancel(w) {
        return;
    }
    if w.sketch.as_ref().is_some_and(|s| s.edit.is_some()) {
        crate::sketch_edit_ui::cancel(w);
        return;
    }
    if let Some(s) = &mut w.sketch
        && let Some(previous) = s.history.pop()
    {
        restore(s, previous);
    }
    solve(w);
}
pub fn escape(w: &mut Workspace) -> bool {
    if crate::sketch_handles::cancel(w) {
        return true;
    }
    if w.sketch.as_ref().is_some_and(|s| s.edit.is_some()) {
        crate::sketch_edit_ui::cancel(w);
        return true;
    }
    if crate::sketch_ui::cancel_dimension(w) {
        return true;
    }
    if w.fields.remove("smart-dimension-mode").is_some() {
        crate::sketch_ui::select(w, None, false);
        w.status = "Smart Dimension pick mode canceled; earlier sketch previews retained".into();
        return true;
    }
    if let Some(s) = &mut w.sketch
        && s.dimension.take().is_some()
    {
        return true;
    }
    if let Some(s) = &mut w.sketch
        && (!s.chain.is_empty() || !s.clicks.is_empty() || s.tool != Tool::Select)
    {
        s.chain.clear();
        s.clicks.clear();
        s.click_join.clear();
        s.tool = Tool::Select;
        return true;
    }
    false
}
pub fn plane(name: &str) -> Plane {
    match name {
        "XZ" | "Front Plane" => Plane {
            origin: [0.; 3],
            u: [1., 0., 0.],
            v: [0., 0., 1.],
        },
        "YZ" | "Right Plane" => Plane {
            origin: [0.; 3],
            u: [0., 1., 0.],
            v: [0., 0., 1.],
        },
        _ => Plane::default(),
    }
}
/// Raw source stays independent of the native editor. Only a buffer still
/// identical to its recorded baseline follows native drawing/solver changes.
fn synchronize_source_buffer(
    fields: &mut BTreeMap<String, String>,
    source: &Sketch,
    initialize: bool,
) {
    let key = format!("sketch-source:{}", source.id);
    let baseline = format!("sketch-source-baseline:{}", source.id);
    if !initialize && !fields.contains_key(&key) {
        return;
    }
    let Ok(current) = serde_json::to_string_pretty(source) else {
        return;
    };
    if !fields.contains_key(&key)
        || fields.get(&key) == fields.get(&baseline)
        || (!fields.contains_key(&baseline) && fields.get(&key) == Some(&current))
    {
        fields.insert(key, current.clone());
        fields.insert(baseline, current);
    }
}
/// Unknown legacy buffers are treated conservatively. Valid text already equal
/// to the current source is not an unapplied edit, but its formatting is retained.
pub(crate) fn source_buffer_is_pending(w: &Workspace, source: &Sketch) -> bool {
    let key = format!("sketch-source:{}", source.id);
    let Some(raw) = w.fields.get(&key) else {
        return false;
    };
    w.fields
        .get(&format!("sketch-source-baseline:{}", source.id))
        != Some(raw)
        && serde_json::from_str::<Sketch>(raw).map_or(true, |parsed| parsed != *source)
}
fn synchronize_active_source(w: &mut Workspace) {
    if let Some(sketch) = &w.sketch {
        synchronize_source_buffer(&mut w.fields, &sketch_definition(sketch), false);
    }
}

pub(crate) fn sketch_definition(s: &EditingSketch) -> Sketch {
    Sketch {
        id: s.sketch_id.clone(),
        name: s.name.clone(),
        points: s
            .keys
            .iter()
            .cloned()
            .zip(s.points.iter().copied())
            .collect(),
        segments: vec![],
        constraints: s.constraints.clone(),
        profile_order: vec![],
        circles: BTreeMap::new(),
        members: s.members.clone(),
        conversions: s.conversions.clone(),
        offsets: s.offsets.clone(),
        fillets: s.fillets.clone(),
        chamfers: s.chamfers.clone(),
        transforms: s.transforms.clone(),
        splines: s.splines.clone(),
        conics: s.conics.clone(),
        texts: s.texts.clone(),
        equations: s.equations.clone(),
        blocks: s.blocks.clone(),
        block_instances: s.block_instances.clone(),
        dimension_groups: s.dimension_groups.clone(),
    }
}
pub(crate) fn display_definition(s: &EditingSketch) -> Sketch {
    s.last_valid.clone().unwrap_or_else(|| sketch_definition(s))
}
fn add_point(s: &mut EditingSketch, p: [f64; 2], snap_existing: bool) -> Id {
    let source = sketch_definition(s);
    let suppressed = cad_core::sketch_transform::suppressed_points(&source);
    let generated = dynamic_generated_points(&source);
    let snapped = snap_existing
        .then(|| {
            s.points
                .iter()
                .enumerate()
                .filter(|(index, v)| {
                    !suppressed.contains(&s.keys[*index]) && (v[0] - p[0]).hypot(v[1] - p[1]) < 1e-7
                })
                .min_by_key(|(index, _)| generated.contains(&s.keys[*index]))
                .map(|(index, _)| s.keys[index].clone())
        })
        .flatten();
    if let Some(target) = snapped
        .as_ref()
        .filter(|target| !generated.contains(*target))
    {
        return target.clone();
    }
    let key = id();
    s.keys.push(key.clone());
    s.points.push(p);
    if let Some(target) = snapped {
        s.constraints.push(SketchConstraint {
            id: id(),
            kind: "coincident".into(),
            entities: vec![key.clone(), target],
            value: "0 mm".into(),
            driven: false,
            distance_side: None,
            contact: None,
        });
    }
    key
}
fn dynamic_generated_points(source: &Sketch) -> BTreeSet<Id> {
    if source.transforms.values().any(|pattern| pattern.dynamic) {
        source
            .transforms
            .values()
            .flat_map(cad_core::sketch_transform::generated_points)
            .collect()
    } else {
        BTreeSet::new()
    }
}
fn add_member(s: &mut EditingSketch, curve: SketchCurve) {
    let key = id();
    s.members.insert(
        key.clone(),
        SketchMember {
            construction: s.construction,
            curve,
        },
    );
    s.selected_member = Some(key);
    s.closed = true;
}
fn line(s: &mut EditingSketch, a: Id, b: Id) {
    if a != b {
        add_member(s, SketchCurve::Line { start: a, end: b });
    }
}
fn finish_spline(w: &mut Workspace) {
    let Some(s) = &w.sketch else { return };
    let mut source = sketch_definition(s);
    let owner = s.definition.clone();
    let before = source.clone();
    let selected = if s.tool == Tool::Spline {
        let member = id();
        source.members.insert(
            member.clone(),
            SketchMember {
                construction: s.construction,
                curve: SketchCurve::Bezier {
                    points: s.chain.clone(),
                },
            },
        );
        Ok(member)
    } else {
        cad_core::sketch_spline::add(
            w.session.visible(),
            &owner,
            &mut source,
            &s.chain,
            &cad_core::sketch_spline::Create {
                fit: s.tool == Tool::InterpolatingSpline,
                degree: s.spline_degree.min(s.chain.len().saturating_sub(1)),
                parameterization: s.spline_parameterization,
                construction: s.construction,
            },
        )
    };
    match selected {
        Ok(member) => {
            if let Some(s) = &mut w.sketch {
                s.history.push(before);
                restore(s, source);
                s.tool = Tool::Select;
                s.selected_member = Some(member.clone());
                s.selection = vec![cad_core::sketch_actions::SketchEntity::Member { member }];
            }
            solve(w);
            if w.sketch.as_ref().is_some_and(|s| {
                s.selected_member
                    .as_ref()
                    .is_some_and(|key| s.splines.contains_key(key))
            }) {
                crate::sketch_edit_ui::begin(w, crate::sketch_edit_ui::Kind::Spline);
            }
        }
        Err(error) => {
            if let Some(s) = &mut w.sketch {
                s.placement_error = Some(error.to_string());
            }
            w.status = error.to_string();
        }
    }
}
fn conic_request(
    source: &mut Sketch,
    tool: Tool,
    positions: &[[f64; 2]],
    rho: &Expr,
    clockwise: bool,
    construction: bool,
) -> Option<cad_core::sketch_conic::Create> {
    let kind = tool.conic_kind()?;
    if positions.len() < 3 {
        return None;
    }
    let suppressed = cad_core::sketch_transform::suppressed_points(source);
    let generated = dynamic_generated_points(source);
    let keys = std::array::from_fn(|i| {
        let snapped = source
            .points
            .iter()
            .filter(|(key, point)| {
                !suppressed.contains(*key)
                    && (point[0] - positions[i][0]).hypot(point[1] - positions[i][1]) < 1e-7
            })
            .min_by_key(|(key, _)| generated.contains(*key))
            .map(|(key, _)| key.clone());
        if let Some(target) = snapped
            .as_ref()
            .filter(|target| !generated.contains(*target))
        {
            return target.clone();
        }
        let key = id();
        source.points.insert(key.clone(), positions[i]);
        if let Some(target) = snapped {
            source.constraints.push(SketchConstraint {
                id: id(),
                kind: "coincident".into(),
                entities: vec![key.clone(), target],
                value: "0 mm".into(),
                driven: false,
                distance_side: None,
                contact: None,
            });
        }
        key
    });
    let size = if tool == Tool::Conic {
        rho.clone()
    } else if tool == Tool::ArcSlot {
        let radius = (positions[1][0] - positions[0][0]).hypot(positions[1][1] - positions[0][1]);
        let width = positions
            .get(3)
            .map(|point| {
                2. * ((point[0] - positions[0][0]).hypot(point[1] - positions[0][1]) - radius).abs()
            })
            .unwrap_or(radius * 0.2);
        Expr(format!("{width} mm"))
    } else {
        "0.5".into()
    };
    Some(cad_core::sketch_conic::Create {
        kind,
        source: keys,
        size,
        clockwise,
        construction,
    })
}
fn finish_conic(w: &mut Workspace) {
    let Some(s) = &w.sketch else { return };
    let mut source = sketch_definition(s);
    let before = source.clone();
    let Some(request) = conic_request(
        &mut source,
        s.tool,
        &s.clicks,
        &s.conic_rho,
        s.clockwise,
        s.construction,
    ) else {
        return;
    };
    match cad_core::sketch_conic::add(w.session.visible(), &s.definition, &mut source, &request) {
        Ok(member) => {
            if let Some(s) = &mut w.sketch {
                s.history.push(before);
                restore(s, source);
                s.tool = Tool::Select;
                s.selected_member = Some(member.clone());
                s.selection = vec![cad_core::sketch_actions::SketchEntity::Member { member }];
            }
            solve(w);
            crate::sketch_edit_ui::begin(w, crate::sketch_edit_ui::Kind::Conic);
        }
        Err(error) => {
            if let Some(s) = &mut w.sketch {
                s.clicks.pop();
                s.placement_error = Some(error.to_string());
            }
            w.status = error.to_string();
        }
    }
}
fn preview_document(w: &Workspace, s: &EditingSketch) -> cad_core::Result<Document> {
    let mut document = w.session.visible().clone();
    let part = document
        .parts
        .get_mut(&s.definition)
        .ok_or_else(|| CadError::Invalid("The editing part no longer exists".into()))?;
    part.sketches
        .insert(s.sketch_id.clone(), sketch_definition(s));
    let feature = Feature {
        id: s.feature.clone(),
        name: s.name.clone(),
        suppressed: false,
        operation: Operation::Profile {
            profile: Profile::Sketch {
                sketch: s.sketch_id.clone(),
            },
            plane: s.plane.clone(),
            plane_reference: s.reference.clone(),
            reference_body: s.reference_body.clone(),
        },
    };
    if let Some(existing) = part.features.iter_mut().find(|f| f.id == feature.id) {
        *existing = feature;
    } else {
        part.features.push(feature);
    }
    document.validate()?;
    Ok(document)
}
pub(crate) fn solve(w: &mut Workspace) {
    use cad_core::solver::ConstraintSolver;
    let Some(s) = &w.sketch else { return };
    if s.choosing_plane {
        return;
    }
    let mut source = sketch_definition(s);
    let owner = s.definition.clone();
    let synchronized = cad_core::sketch_transform::synchronize_dynamic(&mut source);
    if matches!(synchronized, Ok(true))
        && let Some(s) = &mut w.sketch
    {
        s.keys = source.points.keys().cloned().collect();
        s.points = s.keys.iter().map(|key| source.points[key]).collect();
        s.members = source.members.clone();
        s.transforms = source.transforms.clone();
    }
    let result = synchronized.and_then(|_| {
        cad_core::solver::NativeSolver.solve_sketch(w.session.visible(), &owner, &mut source)
    });
    match result {
        Ok(report) => {
            if let Some(s) = &mut w.sketch {
                s.degrees_of_freedom = Some(report.degrees_of_freedom);
                s.block_instances = source.block_instances.clone();
                if let Some(editor) = s.edit.as_mut().and_then(|editor| editor.equation.as_mut()) {
                    editor.refresh(&source);
                }
                if let Some(editor) = s
                    .edit
                    .as_mut()
                    .and_then(|editor| editor.dimension_group.as_mut())
                {
                    editor.refresh(&source);
                }
                if let Some(block) = s.edit.as_mut().and_then(|editor| editor.block.as_mut()) {
                    block.refresh(&source);
                }
                s.points = s.keys.iter().map(|key| source.points[key]).collect();
                for c in &mut s.constraints {
                    if let Some(solved) = source.constraints.iter().find(|other| other.id == c.id) {
                        if c.driven {
                            c.value = solved.value.clone();
                        }
                        c.contact = solved.contact.clone();
                    }
                }
                s.last_valid = Some(source);
                s.error = None;
            }
            crate::sketch_relations_ui::refresh(w, Some(report.clone()));
            if let Some(s) = &w.sketch {
                match preview_document(w, s) {
                    Ok(document) if document.fingerprint() != w.session.visible().fingerprint() => {
                        w.dispatch(Command::ReplaceDocument { document });
                    }
                    Err(e) => {
                        w.status = e.to_string();
                        if let Some(s) = &mut w.sketch {
                            s.error = Some(e.to_string());
                        }
                        return;
                    }
                    _ => {}
                }
            }
            w.status = format!(
                "Sketch · {} degrees of freedom · live preview",
                report.degrees_of_freedom
            );
        }
        Err(e) => {
            if let Some(s) = &mut w.sketch {
                s.degrees_of_freedom = None;
                s.error = Some(e.to_string());
            }
            crate::sketch_relations_ui::refresh(w, None);
            if let Some(s) = &w.sketch
                && let Ok(document) = preview_document(w, s)
                && document.fingerprint() != w.session.visible().fingerprint()
            {
                w.dispatch(Command::ReplaceDocument { document });
            }
            let identity = w.session.identity();
            w.session.fail(&identity, e.to_string());
            w.status = format!("{e} · last valid body preview retained");
        }
    }
    synchronize_active_source(w);
}
/// Reconcile only the current worker result, including freshly resolved conversion curves.
pub(crate) fn accept_evaluation(w: &mut Workspace) {
    let solved = w.sketch.as_ref().and_then(|s| {
        let evaluated = viewport::display_evaluation(w)?;
        if !evaluated.valid || evaluated.identity != w.session.identity() {
            return None;
        }
        let key = evaluated
            .instances
            .iter()
            .find(|i| {
                i.definition == s.definition
                    && w.view
                        .active_instance
                        .as_ref()
                        .is_none_or(|id| i.path.contains(id))
            })
            .map(|i| {
                if i.mesh_key.is_empty() {
                    &i.definition
                } else {
                    &i.mesh_key
                }
            })
            .unwrap_or(&s.definition);
        evaluated
            .sketches
            .get(&format!("{key}::{}", s.feature))
            .map(|record| (record.plane.clone(), record.solved.clone()))
    });
    if let Some((plane, Some(solved))) = solved
        && let Some(s) = &mut w.sketch
        && s.error.is_none()
    {
        s.plane = plane;
        s.block_instances = solved.block_instances.clone();
        if let Some(editor) = s.edit.as_mut().and_then(|editor| editor.equation.as_mut()) {
            editor.refresh(&solved);
        }
        if let Some(editor) = s
            .edit
            .as_mut()
            .and_then(|editor| editor.dimension_group.as_mut())
        {
            editor.refresh(&solved);
        }
        if let Some(block) = s.edit.as_mut().and_then(|editor| editor.block.as_mut()) {
            block.refresh(&solved);
        }
        for (key, conversion) in &mut s.conversions {
            if let Some(resolved) = solved.conversions.get(key) {
                conversion.curve = resolved.curve.clone();
            }
        }
        for (index, key) in s.keys.iter().enumerate() {
            if let Some(point) = solved.points.get(key) {
                s.points[index] = *point;
            }
        }
        for c in &mut s.constraints {
            if let Some(record) = solved.constraints.iter().find(|other| other.id == c.id) {
                if c.driven {
                    c.value = record.value.clone();
                }
                c.contact = record.contact.clone();
            }
        }
        s.last_valid = Some(solved);
        let report = crate::sketch_relations_ui::retain_report(s);
        if report.is_none() {
            s.degrees_of_freedom = None;
        }
        crate::sketch_relations_ui::refresh(w, report);
    }
    synchronize_active_source(w);
}
/// Edits from custom bindings and parameter panels reconcile into the active editor.
pub(crate) fn reconcile(w: &mut Workspace, before: &Document) {
    let Some(s) = &w.sketch else { return };
    let after = w.session.visible();
    let parameters_changed = before
        .parts
        .iter()
        .map(|(id, p)| (id, &p.parameters))
        .collect::<Vec<_>>()
        != after
            .parts
            .iter()
            .map(|(id, p)| (id, &p.parameters))
            .collect::<Vec<_>>()
        || before
            .assemblies
            .iter()
            .map(|(id, a)| (id, &a.parameters))
            .collect::<Vec<_>>()
            != after
                .assemblies
                .iter()
                .map(|(id, a)| (id, &a.parameters))
                .collect::<Vec<_>>();
    let source = after
        .parts
        .get(&s.definition)
        .and_then(|p| p.sketches.get(&s.sketch_id));
    let source_changed = source
        != before
            .parts
            .get(&s.definition)
            .and_then(|p| p.sketches.get(&s.sketch_id));
    let replacement = source
        .filter(|source| source_changed && **source != sketch_definition(s))
        .cloned();
    let needs_reconcile = replacement.is_some();
    if parameters_changed || needs_reconcile {
        crate::sketch_handles::end(w);
    }
    if let Some(source) = replacement
        && let Some(s) = &mut w.sketch
    {
        let selection = s.selection.clone();
        let dimension = s.dimension.clone();
        s.history.push(sketch_definition(s));
        restore(s, source);
        s.selection = selection
            .into_iter()
            .filter(|entity| match entity {
                cad_core::sketch_actions::SketchEntity::Point { point } => s.keys.contains(point),
                cad_core::sketch_actions::SketchEntity::Member { member } => {
                    s.members.contains_key(member)
                }
                cad_core::sketch_actions::SketchEntity::Constraint { constraint } => {
                    s.constraints.iter().any(|c| &c.id == constraint)
                }
                _ => true,
            })
            .collect();
        s.dimension = dimension.filter(|d| s.constraints.iter().any(|c| c.id == d.constraint));
    }
    if parameters_changed || needs_reconcile {
        solve(w);
    }
}
pub fn finish(w: &mut Workspace) -> bool {
    if !crate::holes_ui::positions_are_ready(w) {
        return false;
    }
    crate::sketch_handles::end(w);
    solve(w);
    let Some(s) = &w.sketch else { return false };
    if !s.closed
        || s.edit.is_some()
        || s.error.is_some()
        || s.choosing_plane
        || !s.clicks.is_empty()
        || (s.tool.is_spline() && !s.chain.is_empty())
    {
        w.status = "Finish drawing and resolve sketch errors before applying".into();
        return false;
    }
    let feature = s.feature.clone();
    if w.session.draft.is_none() {
        match preview_document(w, s) {
            Ok(document) => w.dispatch(Command::ReplaceDocument { document }),
            Err(e) => {
                w.status = e.to_string();
                return false;
            }
        }
    }
    if let Some(error) = w.session.draft.as_ref().and_then(|d| d.error.as_ref()) {
        w.status = format!("{error} · correct the sketch or dependent features before applying");
        return false;
    }
    if !w.session.draft.as_ref().is_some_and(|d| {
        !d.pending
            && d.evaluation
                .as_ref()
                .is_some_and(|e| e.valid && e.identity == w.session.identity())
    }) {
        w.confirm_requested = true;
        w.status = "Confirming sketch after the current geometry preview validates".into();
        return false;
    }
    let Some(s) = &w.sketch else { return false };
    match preview_document(w, s) {
        Ok(document) => {
            if w.session.draft.is_none()
                || document.fingerprint() != w.session.visible().fingerprint()
            {
                w.dispatch(Command::ReplaceDocument { document });
            }
            w.view.active_node = Some(feature);
            w.sketch = None;
            w.selection_popup = None;
            w.context_selection = None;
            w.fields.remove("smart-dimension-mode");
            w.dialog = None;
            w.command_tab = "Features".into();
            w.manager_tree = true;
            w.status = "Sketch finished; awaiting current validated preview".into();
            true
        }
        Err(e) => {
            w.status = e.to_string();
            false
        }
    }
}
pub fn controls(ui: &mut egui::Ui, w: &mut Workspace) {
    crate::holes_ui::sketch_prompt(ui, w);
    if w.sketch.as_ref().is_some_and(|s| s.choosing_plane) {
        ui.label("Choose an origin plane or a planar model face.");
        for name in ["Front Plane", "Top Plane", "Right Plane"] {
            if ui.button(name).clicked() {
                choose_origin(w, name);
            }
        }
        return;
    }
    if crate::sketch_edit_ui::controls(ui, w) {
        return;
    }
    crate::sketch_ui::controls(ui, w);
    crate::sketch_overlay::snap_controls(ui, w);
    let evaluated_sizes = crate::sketch_ui::evaluated_member_values(w);
    let settings = w.session.visible().settings.clone();
    let mut changed = false;
    let mut undo = false;
    let mut clicked_entity = None;
    let mut delete_selected = false;
    let mut finish_curve = false;
    if let Some(s) = &mut w.sketch {
        ui.strong(&s.name);
        ui.small(format!(
            "{} members · {}",
            s.members.len(),
            s.degrees_of_freedom
                .map(|dof| format!("{dof} DOF"))
                .unwrap_or_else(|| "DOF unavailable".into())
        ));
        ui.checkbox(&mut s.construction, "Construction for new geometry");
        if s.tool == Tool::Polygon {
            ui.add(
                egui::DragValue::new(&mut s.polygon_sides)
                    .range(3..=32)
                    .prefix("Sides "),
            );
        }
        if matches!(s.tool, Tool::CenterArc | Tool::ArcSlot) {
            ui.checkbox(&mut s.clockwise, "Clockwise arc");
        }
        if s.tool == Tool::Conic {
            crate::presentation_controls::expression(
                ui,
                &format!("{}::conic-rho", s.sketch_id),
                "Rho (0–1; 0.5 is parabolic)",
                &mut s.conic_rho,
            );
        }
        crate::presentation_controls::collapsing(
            ui,
            &format!("{}::tool-help", s.sketch_id),
            "sketch-secondary-section",
            "Tool help",
            false,
            |ui| {
                ui.small(match s.tool {
            Tool::Arc => "Arc: start, end, then a point on the arc.",
            Tool::ThreePointRectangle => "Rectangle: first corner, next corner along an edge, then height.",
            Tool::ThreePointCenterRectangle => "Rectangle: center, half-width axis point, then half-height.",
            Tool::Parallelogram => "Parallelogram: three consecutive corners.",
            Tool::PerimeterCircle => "Circle: three points on the perimeter.",
            Tool::CenterArc => "Arc: center, start point, then end direction. Radius follows the start point.",
            Tool::TangentArc => "Arc: an existing straight member endpoint, then the arc endpoint.",
            Tool::CenterSlot => "Slot: overall center, an end center, then half-width.",
            Tool::MidpointLine => "Line: midpoint, then one endpoint. The midpoint remains constrained.",
            Tool::Centerline => "Construction centerline: first point, then second point.",
            Tool::Ellipse => "Ellipse: center, major axis point, then minor radius.",
            Tool::Slot => "Slot: first center, second center, then half-width.",
            Tool::Spline => "Bézier spline: click 2–16 controls, then Finish Spline.",
            Tool::InterpolatingSpline => "Interpolating spline: click 2–64 fit points, then Finish Spline. Fit points remain draggable and constrainable.",
            Tool::Nurbs => "NURBS: click 2–64 controls, then Finish Spline. Edit weights and knots in the docked manager.",
            Tool::Conic => "Conic: start, end, then the intersection of their tangent lines. Rho controls the shape.",
            Tool::Parabola => "Parabola: start, end, then the intersection of their tangent lines.",
            Tool::EllipseArc => "Partial ellipse: center, major-axis handle, then minor-axis handle. Edit start and sweep after placement.",
            Tool::ArcSlot => "Arc slot: center, start direction/radius, end direction, then a radial point defining half-width.",
            Tool::CenterRectangle => "Rectangle: center, then a corner.",
            Tool::Polygon => "Polygon: center, then a vertex.",
            Tool::Point => "Click to add a reference point.",
            Tool::Select => {
                "Drag point handles to reshape. Select a curve for size/move handles. Alt bypasses snapping; Esc cancels a drag. Shift-click adds a selection."
            }
            _ => "Click to draw. Snap to existing points joins members. Esc stops the tool.",
        });
            },
        );
        if matches!(s.tool, Tool::InterpolatingSpline | Tool::Nurbs) {
            ui.add(
                egui::DragValue::new(&mut s.spline_degree)
                    .range(1..=cad_core::sketch_spline::MAX_DEGREE)
                    .prefix("Degree "),
            );
            ui.small("Degree is limited to one less than the number of placed points.");
            if s.tool == Tool::InterpolatingSpline {
                crate::sketch_edit_ui::parameterization(ui, &mut s.spline_parameterization);
            }
        }
        if s.tool.is_spline()
            && ui
                .add_enabled(s.chain.len() >= 2, egui::Button::new("Finish Spline"))
                .clicked()
        {
            finish_curve = true;
        }
        if !s.selection.is_empty() && ui.button("Delete Selection").clicked() {
            delete_selected = true;
        }
        ui.separator();
        let mut remove = None;
        let member_checkpoint = sketch_definition(s);
        let driven_points = cad_core::sketch_edit::driven_points(&member_checkpoint);
        let mut member_order = s
            .members
            .keys()
            .enumerate()
            .map(|(index, key)| (index, key.clone()))
            .collect::<Vec<_>>();
        member_order.sort_by_key(|(_, key)| s.selected_member.as_ref() != Some(key));
        let member_inspector = crate::sketch_ui::selected_inspector_open(
            ui,
            "sketch-member-inspector",
            s.selected_member.as_deref(),
        );
        egui::CollapsingHeader::new("Sketch Members")
            .open(member_inspector)
            .default_open(true)
            .show(ui, |ui| {
                let props = crate::presentation_controls::properties(&format!("{}::member-list", s.sketch_id), "sketch-member-list", &["compact"], "ready");
                if props.get("display").is_some_and(|value| value == "none") { return; }
                let height = crate::presentation_controls::number(&props, "max-height", 180.).max(24.);
                egui::ScrollArea::vertical().id_salt(("sketch-member-list", &s.sketch_id)).max_height(height).show(ui, |ui| {
                crate::presentation_controls::apply_properties(ui, &props);
                for (index, member_key) in member_order {
                    let key = &member_key;
                    let Some(member) = s.members.get_mut(key) else { continue };
                    ui.push_id(key, |ui| {
                        let name = match member.curve {
                            SketchCurve::Line { .. } => "Line",
                            SketchCurve::Arc { .. } => "3-point arc",
                            SketchCurve::Bezier { .. } => "Control-point spline",
                            SketchCurve::Nurbs { .. } => "Rational spline",
                            SketchCurve::Circle { .. } => "Circle",
                            SketchCurve::Ellipse { .. } => "Ellipse",
                        };
                        ui.horizontal(|ui| {
                            let response = ui.selectable_label(s.selection.iter().any(|e| matches!(e, cad_core::sketch_actions::SketchEntity::Member { member } if member == key)), format!("{} · {name}", index+1));
                            if response.clicked() {
                                clicked_entity = Some(cad_core::sketch_actions::SketchEntity::Member { member: key.clone() });
                            }
                            if ui
                                .small_button("×")
                                .on_hover_text("Delete member")
                                .clicked()
                            {
                                remove = Some(key.clone());
                            }
                        });
                        if s.selected_member.as_ref() == Some(key) {
                            if ui.checkbox(&mut member.construction, "Construction").changed() { s.history.push(member_checkpoint.clone()); changed = true; }
                            let operation_driven = cad_core::sketch_edit::driven_member(&member_checkpoint, key);
                            match &mut member.curve {
                                SketchCurve::Circle { radius, .. } => {
                                    ui.label("Radius");
                                    let driver = s.constraints.iter().find(|c| !c.driven && matches!(c.kind.as_str(), "radius" | "diameter") && c.entities.first() == Some(key));
                                    let equal = s.constraints.iter().any(|c| c.kind == "equal_radius" && c.entities.get(1) == Some(key));
                                    if driver.is_some() || equal || operation_driven {
                                        let value = evaluated_sizes.get(&(key.clone(), "radius".into())).map(String::as_str).unwrap_or("Awaiting solved size");
                                        crate::presentation_controls::label(ui, &format!("{key}::radius.evaluated"), "sketch-evaluated-size", &[], value, if s.error.is_some() { "last-valid" } else { "evaluated" });
                                        ui.small(if s.error.is_some() { "Last valid radius · correct the sketch to update" } else { "Controlled by dimension / relation" });
                                    } else {
                                        changed |= crate::presentation_controls::expression_edit(ui, &format!("{key}::radius"), "Radius", radius, false, "Radius, units or parameter", None).changed();
                                    }
                                }
                                SketchCurve::Ellipse { major, minor, angle, .. } => {
                                    for (label, kind, value) in [
                                        ("Major radius", "major_radius", major),
                                        ("Minor radius", "minor_radius", minor),
                                        ("Rotation", "rotation", angle),
                                    ] {
                                        ui.label(label);
                                        let driven = s.constraints.iter().any(|c| !c.driven && c.kind == kind && c.entities.first() == Some(key));
                                        if driven || operation_driven {
                                            let value = evaluated_sizes.get(&(key.clone(), kind.into())).map(String::as_str).unwrap_or("Awaiting solved size");
                                            crate::presentation_controls::label(ui, &format!("{key}::{kind}.evaluated"), "sketch-evaluated-size", &[], value, if s.error.is_some() { "last-valid" } else { "evaluated" });
                                            ui.small("Controlled by dimension / relation");
                                        } else {
                                            changed |= crate::presentation_controls::expression_edit(ui, &format!("{key}::{kind}"), label, value, false, "Value, units or parameter", None).changed();
                                        }
                                    }
                                }
                                _ => {}
                            }
                        }
                    });
                }
                });
            });
        if let Some(key) = remove {
            s.history.push(sketch_definition(s));
            let mut source = sketch_definition(s);
            cad_core::sketch_edit::delete_members(
                &mut source,
                &std::collections::BTreeSet::from([key]),
            );
            restore(s, source);
            changed = true;
        }
        let selected_point = s
            .selected
            .and_then(|index| s.keys.get(index))
            .map(String::as_str);
        let point_inspector =
            crate::sketch_ui::selected_inspector_open(ui, "sketch-point-inspector", selected_point);
        let mut point_order = (0..s.points.len()).collect::<Vec<_>>();
        point_order.sort_by_key(|index| s.selected != Some(*index));
        egui::CollapsingHeader::new("Point Coordinates")
            .open(point_inspector)
            .default_open(s.selected.is_some())
            .show(ui, |ui| {
                for i in point_order {
                    let p = &mut s.points[i];
                    ui.push_id(&s.keys[i], |ui| {
                        let response =
                            ui.selectable_label(s.selection.iter().any(|e| matches!(e, cad_core::sketch_actions::SketchEntity::Point { point } if point == &s.keys[i])), format!("Point {}", i + 1));
                        if response.clicked() {
                            clicked_entity = Some(
                                cad_core::sketch_actions::SketchEntity::Point {
                                    point: s.keys[i].clone(),
                                },
                            );
                        }
                        ui.add_enabled_ui(!driven_points.contains(&s.keys[i]), |ui| {
                            ui.horizontal(|ui| {
                                changed |= crate::sketch_ui::coordinate(ui, "X", &mut p[0], &settings);
                                changed |= crate::sketch_ui::coordinate(ui, "Y", &mut p[1], &settings);
                            });
                        });
                    });
                }
            });
        egui::CollapsingHeader::new("Advanced Point Relations").show(ui, |ui| {
            let kind = w
                .fields
                .entry("sketch-relation-kind".into())
                .or_insert("distance".into());
            egui::ComboBox::from_id_salt("sketch-relation-kind")
                .selected_text(kind.as_str())
                .show_ui(ui, |ui| {
                    for name in [
                        "fix_x",
                        "fix_y",
                        "distance",
                        "distance_x",
                        "distance_y",
                        "horizontal",
                        "vertical",
                        "coincident",
                        "parallel",
                        "perpendicular",
                        "equal",
                        "angle",
                    ] {
                        ui.selectable_value(kind, name.into(), name);
                    }
                });
            let kind = kind.clone();
            let value = w
                .fields
                .entry("sketch-relation-value".into())
                .or_insert("10 mm".into());
            ui.text_edit_singleline(value);
            let value = value.clone();
            let mut first = w
                .fields
                .get("sketch-relation-first")
                .and_then(|v| v.parse::<usize>().ok())
                .unwrap_or(1);
            let mut second = w
                .fields
                .get("sketch-relation-second")
                .and_then(|v| v.parse::<usize>().ok())
                .unwrap_or(2);
            ui.add(
                egui::DragValue::new(&mut first)
                    .range(1..=s.keys.len().max(1))
                    .prefix("First point "),
            );
            ui.add(
                egui::DragValue::new(&mut second)
                    .range(1..=s.keys.len().max(1))
                    .prefix("Second point "),
            );
            w.fields
                .insert("sketch-relation-first".into(), first.to_string());
            w.fields
                .insert("sketch-relation-second".into(), second.to_string());
            let four = matches!(
                kind.as_str(),
                "parallel" | "perpendicular" | "equal" | "angle"
            );
            let mut third = w
                .fields
                .get("sketch-relation-third")
                .and_then(|v| v.parse::<usize>().ok())
                .unwrap_or(3)
                .clamp(1, s.keys.len().max(1));
            let mut fourth = w
                .fields
                .get("sketch-relation-fourth")
                .and_then(|v| v.parse::<usize>().ok())
                .unwrap_or(4)
                .clamp(1, s.keys.len().max(1));
            if four {
                ui.small("Segment 1: points 1 and 2. Segment 2: points 3 and 4.");
                ui.add(
                    egui::DragValue::new(&mut third)
                        .range(1..=s.keys.len().max(1))
                        .prefix("Third point "),
                );
                ui.add(
                    egui::DragValue::new(&mut fourth)
                        .range(1..=s.keys.len().max(1))
                        .prefix("Fourth point "),
                );
                w.fields
                    .insert("sketch-relation-third".into(), third.to_string());
                w.fields
                    .insert("sketch-relation-fourth".into(), fourth.to_string());
            }
            first = first.clamp(1, s.keys.len().max(1));
            second = second.clamp(1, s.keys.len().max(1));
            if ui
                .add_enabled(!s.keys.is_empty(), egui::Button::new("Add Relation"))
                .clicked()
            {
                s.history.push(sketch_definition(s));
                s.constraints.push(SketchConstraint {
                    driven: false,
                    distance_side: None,
                    contact: None,
                    id: id(),
                    kind: kind.clone(),
                    entities: if kind.starts_with("fix_") {
                        vec![s.keys[first - 1].clone()]
                    } else if four {
                        vec![
                            s.keys[first - 1].clone(),
                            s.keys[second - 1].clone(),
                            s.keys[third - 1].clone(),
                            s.keys[fourth - 1].clone(),
                        ]
                    } else {
                        vec![s.keys[first - 1].clone(), s.keys[second - 1].clone()]
                    },
                    value: Expr(value),
                });
                changed = true;
            }
            let mut remove = None;
            for (i, c) in s.constraints.iter().enumerate() {
                ui.horizontal(|ui| {
                    ui.small(&c.kind);
                    if ui.small_button("Remove").clicked() {
                        remove = Some(i);
                    }
                });
            }
            if let Some(i) = remove {
                s.constraints.remove(i);
                changed = true;
            }
        });
        egui::CollapsingHeader::new("Sketch Plane").show(ui, |ui| {
            if s.reference.is_some() {
                ui.small("Plane follows its selected model face.");
            } else {
                for (label, value) in ["Offset X", "Offset Y", "Offset Z"]
                    .into_iter()
                    .zip(&mut s.plane.origin)
                {
                    changed |= ui
                        .add(
                            egui::DragValue::new(value)
                                .speed(0.1)
                                .prefix(format!("{label} "))
                                .suffix(" mm"),
                        )
                        .changed();
                }
            }
        });
        egui::CollapsingHeader::new("Grid and Snap").show(ui, |ui| {
            ui.checkbox(&mut s.snap, "Snap to grid");
            ui.add(
                egui::DragValue::new(&mut s.grid)
                    .range(0.001..=1000.)
                    .speed(0.1)
                    .suffix(" mm"),
            );
        });
        egui::CollapsingHeader::new("Advanced Source").show(ui, |ui| {
            let source = sketch_definition(s);
            let key = format!("sketch-source:{}", s.sketch_id);
            let baseline = format!("sketch-source-baseline:{}", s.sketch_id);
            synchronize_source_buffer(&mut w.fields, &source, true);
            let mut accepted = false;
            let Some(text) = w.fields.get_mut(&key) else {
                ui.colored_label(
                    Color32::from_rgb(180, 45, 30),
                    "Current sketch source could not be serialized",
                );
                return;
            };
            ui.add(
                egui::TextEdit::multiline(&mut *text)
                    .code_editor()
                    .desired_width(f32::INFINITY)
                    .desired_rows(8),
            );
            if ui.button("Refresh Source").clicked() {
                *text = serde_json::to_string_pretty(&source).unwrap_or_default();
                accepted = true;
            }
            if ui.button("Load Sketch Source").clicked() {
                match serde_json::from_str::<Sketch>(text) {
                    Ok(source)
                        if source.id == s.sketch_id
                            && source.profile_order.is_empty()
                            && source.circles.is_empty()
                            && source.segments.is_empty() =>
                    {
                        s.history.push(sketch_definition(s));
                        s.name = source.name.clone();
                        restore(s, source);
                        changed = true;
                        accepted = true;
                    }
                    Ok(_) => {
                        s.error = Some(
                            "Keep this sketch identity and use the members representation".into(),
                        )
                    }
                    Err(error) => s.error = Some(error.to_string()),
                }
            }
            if accepted {
                let current = text.clone();
                w.fields.insert(baseline, current);
            }
        });
        if let Some(error) = &s.error {
            ui.colored_label(Color32::from_rgb(180, 45, 30), error);
        }
        if let Some(error) = &s.placement_error {
            ui.colored_label(Color32::from_rgb(180, 45, 30), error);
        }
        ui.separator();
        if ui.button("Undo Sketch Change").clicked() {
            undo = true;
        }
        ui.small("Open members may define a sweep path. Closed contours define solids; nested contours define holes. Construction members never generate solid geometry.");
    }
    crate::sketch_issues_ui::controls(ui, w);
    crate::sketch_ui::secondary_controls(ui, w);
    if let Some(entity) = clicked_entity {
        let continue_dimension = w.fields.contains_key("smart-dimension-mode")
            && w.sketch.as_ref().is_some_and(|s| s.selection.len() == 1);
        crate::sketch_ui::select(
            w,
            Some(entity),
            continue_dimension || ui.input(|i| i.modifiers.shift || i.modifiers.command),
        );
        if w.fields.contains_key("smart-dimension-mode") {
            crate::sketch_ui::selection_changed(w);
        }
    }
    if delete_selected {
        crate::sketch_ui::action(w, "sketch.delete-selection");
    }
    if changed {
        solve(w);
    }
    if finish_curve {
        finish_spline(w);
    }
    if undo {
        undo_point(w);
    }
}
fn draw_polyline(
    w: &Workspace,
    ui: &egui::Ui,
    r: egui::Rect,
    frame: (&Plane, &[[f64; 4]; 4]),
    points: &[[f64; 2]],
    closed: bool,
    stroke: Stroke,
) {
    let (plane, matrix) = frame;
    let painter = ui.painter().with_clip_rect(r);
    for (a, b) in polyline_segments(points, closed) {
        if let (Some(a), Some(b)) = (
            viewport::project(w, r, world(plane, matrix, a)),
            viewport::project(w, r, world(plane, matrix, b)),
        ) {
            painter.line_segment([a, b], stroke);
        }
    }
}
fn polyline_segments(
    points: &[[f64; 2]],
    closed: bool,
) -> impl Iterator<Item = ([f64; 2], [f64; 2])> + '_ {
    points
        .windows(2)
        .map(|pair| (pair[0], pair[1]))
        .chain((closed && points.len() > 2).then(|| (points[points.len() - 1], points[0])))
}
fn polyline_distance(
    w: &Workspace,
    rect: egui::Rect,
    frame: (&Plane, &[[f64; 4]; 4]),
    points: &[[f64; 2]],
    closed: bool,
    pointer: egui::Pos2,
    mut depth: Option<&mut usize>,
) -> f32 {
    polyline_segments(points, closed)
        .filter_map(|(a, b)| {
            let a = viewport::project(w, rect, world(frame.0, frame.1, a))?;
            let b = viewport::project(w, rect, world(frame.0, frame.1, b))?;
            let edge = b - a;
            let t = ((pointer - a).dot(edge) / edge.length_sq().max(1e-9)).clamp(0., 1.);
            if let Some(remaining) = depth.as_deref_mut() {
                let uv = hit_plane(w, rect, a + edge * t, frame.0, frame.1)?;
                if !viewport::visible_at(w, rect, world(frame.0, frame.1, uv), remaining) {
                    return None;
                }
            }
            Some(pointer.distance(a + edge * t))
        })
        .fold(f32::INFINITY, f32::min)
}
fn circle_points(center: [f64; 2], radius: f64) -> Vec<[f64; 2]> {
    (0..=96)
        .map(|i| {
            let angle = i as f64 / 96. * std::f64::consts::TAU;
            [
                center[0] + radius * angle.cos(),
                center[1] + radius * angle.sin(),
            ]
        })
        .collect()
}
fn draw_geometry(
    w: &Workspace,
    ui: &egui::Ui,
    r: egui::Rect,
    geometry: &EvaluatedSketch,
    matrix: &[[f64; 4]; 4],
    display: (Color32, bool, bool, &mut usize),
) {
    let (color, selected, xray, remaining) = display;
    let mut draw = |points: &[[f64; 2]], closed, stroke| {
        for (a, b) in polyline_segments(points, closed) {
            crate::sketch_overlay::line(
                w,
                ui,
                r,
                [
                    world(&geometry.plane, matrix, a),
                    world(&geometry.plane, matrix, b),
                ],
                stroke,
                xray,
                remaining,
            );
        }
    };
    if !geometry.polylines.is_empty() {
        for path in &geometry.polylines {
            draw(
                &path.points,
                path.closed,
                Stroke::new(
                    if selected {
                        3.5
                    } else if path.construction {
                        1.
                    } else {
                        2.
                    },
                    if path.construction && !selected {
                        color.gamma_multiply(0.4)
                    } else {
                        color
                    },
                ),
            );
        }
        return;
    }
    let points = geometry
        .profile_order
        .iter()
        .filter_map(|id| geometry.points.get(id).copied())
        .collect::<Vec<_>>();
    draw(
        &points,
        true,
        Stroke::new(if selected { 3.5 } else { 2. }, color),
    );
    for circle in geometry.circles.values() {
        if let Some(center) = geometry.points.get(&circle.center) {
            draw(
                &circle_points(*center, circle.radius_mm),
                false,
                Stroke::new(if selected { 3.5 } else { 2. }, color),
            );
        }
    }
}
fn draw_origin_plane(
    w: &Workspace,
    ui: &egui::Ui,
    rect: egui::Rect,
    name: &str,
    matrix: &[[f64; 4]; 4],
    color: Color32,
) {
    if w.sketch.is_none()
        && matches!(
            crate::sketch_overlay::mode(w, false),
            cad_core::presentation::OverlayVisibility::Hidden
        )
    {
        return;
    }
    let props = crate::sketch_overlay::reference_properties(
        w,
        &format!("origin-plane:{name}"),
        "plane",
        true,
    );
    if crate::sketch_overlay::hidden(&props) {
        return;
    }
    let color = props
        .get("color")
        .and_then(|value| crate::ui::color(value))
        .unwrap_or(color);
    let xray = w.sketch.is_some() || crate::sketch_overlay::xray(w, false, &props);
    let plane = plane(name);
    let size = (w.view.distance as f64 * 30.).max(20.);
    let points = [[-size, -size], [size, -size], [size, size], [-size, size]];
    let screen = points
        .iter()
        .filter_map(|p| viewport::project(w, rect, world(&plane, matrix, *p)))
        .collect::<Vec<_>>();
    if screen.len() == 4 && w.sketch.is_some() {
        ui.painter()
            .with_clip_rect(rect)
            .add(egui::Shape::convex_polygon(
                screen,
                color.gamma_multiply(0.07),
                Stroke::NONE,
            ));
    }
    let mut remaining = crate::sketch_overlay::DEPTH_WORK;
    for (a, b) in polyline_segments(&points, true) {
        crate::sketch_overlay::line(
            w,
            ui,
            rect,
            [world(&plane, matrix, a), world(&plane, matrix, b)],
            Stroke::new(
                crate::presentation_controls::number(&props, "stroke-width", 1.2),
                color,
            ),
            xray,
            &mut remaining,
        );
    }
    if (xray || viewport::visible_at(w, rect, world(&plane, matrix, [size, size]), &mut remaining))
        && let Some(p) = viewport::project(w, rect, world(&plane, matrix, [size, size]))
    {
        ui.painter().with_clip_rect(rect).text(
            p,
            egui::Align2::CENTER_CENTER,
            name,
            egui::FontId::proportional(13.),
            color,
        );
    }
}
fn selected_sketch(w: &Workspace, instance: &PlacedPart, feature: &Feature) -> bool {
    if w.selected.is_some()
        || w.selected_feature_path
            .as_ref()
            .is_some_and(|path| path != &instance.path)
    {
        return false;
    }
    if let Some(cad_core::selection_actions::SelectionTarget::Feature {
        definition,
        feature: selected,
    }) = &w.context_selection
    {
        return definition == &instance.definition && selected == &feature.id;
    }
    w.context_selection.is_none()
        && w.view.active_part.as_ref() == Some(&instance.definition)
        && w.view.active_node.as_ref() == Some(&feature.id)
}
fn visible_sketch(w: &Workspace, instance: &PlacedPart, feature: &Feature) -> bool {
    if matches!(
        crate::sketch_overlay::mode(w, true),
        cad_core::presentation::OverlayVisibility::Hidden
    ) {
        return false;
    }
    if w.view.active_part.as_ref() != Some(&instance.definition)
        || w.view.active_instance.as_ref().is_some_and(|id| {
            !instance.path.contains(id) && !(instance.path.is_empty() && id == &instance.definition)
        })
        || feature.suppressed
        || !matches!(feature.operation, Operation::Profile { .. })
    {
        return false;
    }
    if let Some(cursor) = w
        .view
        .history_cursor
        .as_ref()
        .filter(|c| c.definition == instance.definition)
    {
        return viewport::display_evaluation(w)
            .and_then(|e| {
                cad_core::history::stage(e, cad_core::history::mesh_key(instance), &cursor.node)
            })
            .is_some_and(|s| s.sketches.contains(&feature.id));
    }
    match crate::sketch_overlay::mode(w, true) {
        cad_core::presentation::OverlayVisibility::Hidden => false,
        cad_core::presentation::OverlayVisibility::All => true,
        cad_core::presentation::OverlayVisibility::Auto => selected_sketch(w, instance, feature),
    }
}
pub struct SketchHit {
    pub(crate) definition: Id,
    pub(crate) feature: Id,
    pub(crate) path: Vec<Id>,
    pub(crate) instance: Option<Id>,
}
pub fn select_feature(w: &mut Workspace, hit: SketchHit, response: &egui::Response) -> bool {
    let Some(feature) = w
        .session
        .visible()
        .parts
        .get(&hit.definition)
        .and_then(|part| {
            part.features
                .iter()
                .find(|feature| feature.id == hit.feature)
        })
        .cloned()
    else {
        return false;
    };
    if w.tool_checkpoint.is_none() && w.sketch.is_none() {
        w.view.active_instance = hit.instance;
    }
    crate::selection_ui::feature_selected_at(
        w,
        &hit.definition,
        &feature,
        response,
        Some(hit.path),
    );
    true
}
pub fn pick(w: &Workspace, rect: egui::Rect, pointer: egui::Pos2) -> Option<SketchHit> {
    let evaluation = viewport::display_evaluation(w)?;
    let mut nearest = 7.;
    let mut hit = None;
    let mut remaining = crate::sketch_overlay::DEPTH_WORK;
    for instance in viewport::display_instances(w, evaluation) {
        let Some(part) = w.session.visible().parts.get(&instance.definition) else {
            continue;
        };
        let key = if instance.mesh_key.is_empty() {
            &instance.definition
        } else {
            &instance.mesh_key
        };
        for feature in &part.features {
            if !visible_sketch(w, instance, feature) {
                continue;
            }
            let Some(geometry) = evaluation.sketches.get(&format!("{key}::{}", feature.id)) else {
                continue;
            };
            let props = crate::sketch_overlay::properties(
                w,
                &format!(
                    "sketch-overlay:{}:{}:{}",
                    instance.definition,
                    feature.id,
                    instance.path.join("/")
                ),
                "sketch-overlay",
                selected_sketch(w, instance, feature),
            );
            if crate::sketch_overlay::hidden(&props) {
                continue;
            }
            let xray = crate::sketch_overlay::xray(w, true, &props);
            let contours = if geometry.polylines.is_empty() {
                std::iter::once((
                    geometry
                        .profile_order
                        .iter()
                        .filter_map(|id| geometry.points.get(id).copied())
                        .collect::<Vec<_>>(),
                    true,
                ))
                .chain(geometry.circles.values().filter_map(|circle| {
                    geometry
                        .points
                        .get(&circle.center)
                        .map(|center| (circle_points(*center, circle.radius_mm), true))
                }))
                .collect::<Vec<_>>()
            } else {
                geometry
                    .polylines
                    .iter()
                    .map(|path| (path.points.clone(), path.closed))
                    .collect()
            };
            for (points, closed) in contours {
                let distance = polyline_distance(
                    w,
                    rect,
                    (&geometry.plane, &instance.matrix),
                    &points,
                    closed,
                    pointer,
                    (!xray).then_some(&mut remaining),
                );
                if distance < nearest {
                    nearest = distance;
                    hit = Some(SketchHit {
                        definition: instance.definition.clone(),
                        feature: feature.id.clone(),
                        path: instance.path.clone(),
                        instance: instance.path.last().cloned(),
                    });
                }
            }
        }
    }
    hit
}
pub fn overlay(
    w: &mut Workspace,
    ui: &mut egui::Ui,
    r: egui::Rect,
    response: &egui::Response,
    toolbar: egui::Rect,
) {
    let accent = viewport::selection_color(w);
    let blue = w
        .theme
        .variables
        .get("--sketch")
        .and_then(|v| crate::ui::color(v))
        .unwrap_or(Color32::from_rgb(45, 110, 210));
    if w.sketch.is_none() {
        let mut remaining = crate::sketch_overlay::DEPTH_WORK;
        if let Some(name) = &w.active_plane
            && let Some(definition) = &w.view.active_part
        {
            draw_origin_plane(w, ui, r, name, &frame(w, definition), blue);
        }
        if let Some(e) = viewport::display_evaluation(w) {
            for instance in viewport::display_instances(w, e) {
                if let Some(part) = w.session.visible().parts.get(&instance.definition) {
                    let key = if instance.mesh_key.is_empty() {
                        &instance.definition
                    } else {
                        &instance.mesh_key
                    };
                    for feature in &part.features {
                        if visible_sketch(w, instance, feature)
                            && let Some(geometry) =
                                e.sketches.get(&format!("{key}::{}", feature.id))
                        {
                            let selected = selected_sketch(w, instance, feature);
                            let props = crate::sketch_overlay::properties(
                                w,
                                &format!(
                                    "sketch-overlay:{}:{}:{}",
                                    instance.definition,
                                    feature.id,
                                    instance.path.join("/")
                                ),
                                "sketch-overlay",
                                selected,
                            );
                            if crate::sketch_overlay::hidden(&props) {
                                continue;
                            }
                            let color = props
                                .get("color")
                                .and_then(|value| crate::ui::color(value))
                                .unwrap_or(if selected {
                                    accent
                                } else {
                                    blue.gamma_multiply(0.6)
                                });
                            draw_geometry(
                                w,
                                ui,
                                r,
                                geometry,
                                &instance.matrix,
                                (
                                    color,
                                    selected,
                                    crate::sketch_overlay::xray(w, true, &props),
                                    &mut remaining,
                                ),
                            );
                        }
                    }
                }
            }
        }
        return;
    }
    let choosing = w.sketch.as_ref().is_some_and(|s| s.choosing_plane);
    if choosing {
        if let Some(s) = &w.sketch {
            for (name, c) in [
                ("Top Plane", Color32::from_rgb(70, 120, 210)),
                ("Front Plane", Color32::from_rgb(80, 160, 100)),
                ("Right Plane", Color32::from_rgb(190, 100, 80)),
            ] {
                draw_origin_plane(w, ui, r, name, &s.matrix, c);
            }
        }
        if response.clicked()
            && let Some(p) = response
                .interact_pointer_pos()
                .filter(|p| !toolbar.contains(*p))
        {
            let definition = w
                .sketch
                .as_ref()
                .map(|s| s.definition.clone())
                .unwrap_or_default();
            if let Some(hit) = viewport::pick(w, r, p).into_iter().find(|s| {
                s.definition == definition
                    && matches!(s.reference.topology.as_str(), "face" | "plane")
            }) {
                w.selected = Some(hit);
                if let Some((plane, reference)) = selected_plane(w, &definition) {
                    choose(w, plane, Some(reference));
                } else {
                    w.status = "Choose a planar face or an origin plane".into();
                }
            } else if let Some(s) = &w.sketch {
                let matrix = s.matrix;
                let size = (w.view.distance as f64 * 30.).max(20.);
                let chosen = ["Top Plane", "Front Plane", "Right Plane"]
                    .into_iter()
                    .find(|name| {
                        hit_plane(w, r, p, &plane(name), &matrix)
                            .is_some_and(|uv| uv.iter().all(|v| v.abs() <= size))
                    });
                if let Some(name) = chosen {
                    choose(w, plane(name), None);
                }
            }
        }
        return;
    }
    let (plane, matrix) = w
        .sketch
        .as_ref()
        .map(|s| (s.plane.clone(), s.matrix))
        .unwrap_or((Plane::default(), identity()));
    if crate::sketch_edit_ui::converting(w) {
        if let Some(s) = &w.sketch {
            for path in cad_core::sketch_geometry::polylines(
                w.session.visible(),
                &s.definition,
                &display_definition(s),
            )
            .unwrap_or_default()
            {
                draw_polyline(
                    w,
                    ui,
                    r,
                    (&plane, &matrix),
                    &path.points,
                    path.closed,
                    Stroke::new(if path.construction { 1. } else { 2. }, accent),
                );
            }
        }
        crate::sketch_edit_ui::conversion_click(w, response, r);
        return;
    }
    if crate::sketch_edit_ui::curve_editing(w) {
        crate::sketch_edit_ui::curve_edit_overlay(w, ui, r, response);
        return;
    }
    let pointer = response.hover_pos().or_else(|| {
        ui.input(|i| i.pointer.interact_pos())
            .filter(|p| r.contains(*p))
    });
    let mut cursor = pointer.and_then(|p| hit_plane(w, r, p, &plane, &matrix));
    let bypass_snap = ui.input(|i| i.modifiers.alt);
    let entity_snap = w.view.sketch_entity_snap && !bypass_snap;
    let snapped = if entity_snap {
        w.sketch
            .as_ref()
            .filter(|s| s.tool != Tool::Select)
            .and_then(|s| {
                let suppressed =
                    cad_core::sketch_transform::suppressed_points(&sketch_definition(s));
                let pointer = pointer?;
                s.points
                    .iter()
                    .enumerate()
                    .filter(|(index, _)| !suppressed.contains(&s.keys[*index]))
                    .filter_map(|(index, point)| {
                        let pos = viewport::project(w, r, world(&plane, &matrix, *point))?;
                        let distance = pointer.distance(pos);
                        (distance < 9.).then_some((distance, index))
                    })
                    .min_by(|a, b| a.0.total_cmp(&b.0))
                    .map(|(_, index)| index)
            })
    } else {
        None
    };
    if let Some(s) = &w.sketch
        && s.snap
        && !bypass_snap
    {
        cursor = cursor.map(|p| p.map(|v| (v / s.grid).round() * s.grid));
    }
    if let Some((point, key)) = w
        .sketch
        .as_ref()
        .and_then(|s| snapped.map(|index| (s.points[index], s.keys[index].clone())))
    {
        cursor = Some(point);
        w.fields.insert("sketch-snap-target".into(), key);
        let props =
            crate::sketch_overlay::properties(w, "sketch.snap-target", "sketch-snap-target", true);
        if !crate::sketch_overlay::hidden(&props)
            && let Some(pos) = viewport::project(w, r, world(&plane, &matrix, point))
        {
            let color = props
                .get("color")
                .and_then(|value| crate::ui::color(value))
                .unwrap_or(accent);
            ui.painter().with_clip_rect(r).circle_stroke(
                pos,
                crate::presentation_controls::number(&props, "size", 8.).clamp(3., 30.),
                Stroke::new(
                    crate::presentation_controls::number(&props, "stroke-width", 2.).clamp(0.5, 6.),
                    color,
                ),
            );
            ui.painter().with_clip_rect(r).text(
                pos + egui::vec2(12., -12.),
                egui::Align2::LEFT_BOTTOM,
                "Join endpoint · Alt for independent point",
                egui::FontId::proportional(11.),
                color,
            );
        }
    } else {
        w.fields.remove("sketch-snap-target");
    }
    if let Some(s) = &mut w.sketch {
        s.hover = cursor;
    }
    let extent = (w.view.distance as f64 * 70.).clamp(20., 100000.);
    let spacing = 10f64.powf((extent / 12.).log10().floor());
    for i in -12..=12 {
        let d = i as f64 * spacing;
        draw_polyline(
            w,
            ui,
            r,
            (&plane, &matrix),
            &[[d, -extent], [d, extent]],
            false,
            Stroke::new(0.5, Color32::from_black_alpha(26)),
        );
        draw_polyline(
            w,
            ui,
            r,
            (&plane, &matrix),
            &[[-extent, d], [extent, d]],
            false,
            Stroke::new(0.5, Color32::from_black_alpha(26)),
        );
    }
    let related = w
        .sketch
        .as_ref()
        .map(crate::sketch_relations_ui::related_entities)
        .unwrap_or_default();
    let problems = crate::sketch_relations_ui::problem_entities(w);
    let selected_axis = |axis: cad_core::sketch_actions::SketchEntity| {
        related.contains(axis.key())
            || w.sketch
                .as_ref()
                .is_some_and(|sketch| sketch.selection.contains(&axis))
    };
    let x_selected = selected_axis(cad_core::sketch_actions::SketchEntity::XAxis);
    let y_selected = selected_axis(cad_core::sketch_actions::SketchEntity::YAxis);
    let x_problem = problems.contains(cad_core::sketch_actions::X_AXIS);
    let y_problem = problems.contains(cad_core::sketch_actions::Y_AXIS);
    draw_polyline(
        w,
        ui,
        r,
        (&plane, &matrix),
        &[[-extent, 0.], [extent, 0.]],
        false,
        Stroke::new(
            if x_selected || x_problem { 3.5 } else { 1. },
            if x_selected {
                accent
            } else if x_problem {
                ui.visuals().error_fg_color
            } else {
                Color32::from_rgba_unmultiplied(190, 65, 65, 120)
            },
        ),
    );
    draw_polyline(
        w,
        ui,
        r,
        (&plane, &matrix),
        &[[0., -extent], [0., extent]],
        false,
        Stroke::new(
            if y_selected || y_problem { 3.5 } else { 1. },
            if y_selected {
                accent
            } else if y_problem {
                ui.visuals().error_fg_color
            } else {
                Color32::from_rgba_unmultiplied(65, 140, 65, 120)
            },
        ),
    );
    let origin_selected = selected_axis(cad_core::sketch_actions::SketchEntity::Origin);
    if (origin_selected || problems.contains(cad_core::sketch_actions::ORIGIN))
        && let Some(origin) = viewport::project(w, r, world(&plane, &matrix, [0., 0.]))
    {
        ui.painter().with_clip_rect(r).circle_stroke(
            origin,
            7.,
            Stroke::new(
                2.5,
                if origin_selected {
                    accent
                } else {
                    ui.visuals().error_fg_color
                },
            ),
        );
    }
    let mut edited = false;
    let mut placed_conic = false;
    if let Some(s) = &w.sketch {
        if let Ok(paths) = cad_core::sketch_geometry::polylines(
            w.session.visible(),
            &s.definition,
            &display_definition(s),
        ) {
            for path in paths {
                let selected = related.contains(&path.member) || s.selection.iter().any(|e| matches!(e, cad_core::sketch_actions::SketchEntity::Member { member } if member == &path.member));
                let unsatisfied = problems.contains(&path.member);
                draw_polyline(
                    w,
                    ui,
                    r,
                    (&plane, &matrix),
                    &path.points,
                    path.closed,
                    Stroke::new(
                        if selected || unsatisfied {
                            3.5
                        } else if path.construction {
                            1.
                        } else {
                            2.5
                        },
                        if selected {
                            accent
                        } else if unsatisfied {
                            ui.visuals().error_fg_color
                        } else if path.construction {
                            blue.gamma_multiply(0.45)
                        } else {
                            blue
                        },
                    ),
                );
            }
        }
        let pending = s
            .chain
            .iter()
            .filter_map(|id| s.keys.iter().position(|key| key == id).map(|i| s.points[i]))
            .chain(s.clicks.iter().copied())
            .collect::<Vec<_>>();
        let mut preview = pending.clone();
        if let Some(hover) = cursor {
            preview.push(hover);
        }
        draw_polyline(
            w,
            ui,
            r,
            (&plane, &matrix),
            &preview,
            false,
            Stroke::new(1., blue),
        );
        if let Ok(paths) = cad_core::sketch_transform::dynamic_preview(
            w.session.visible(),
            &s.definition,
            &sketch_definition(s),
            &preview,
        ) {
            for points in paths {
                draw_polyline(
                    w,
                    ui,
                    r,
                    (&plane, &matrix),
                    &points,
                    false,
                    Stroke::new(1., blue.gamma_multiply(0.65)),
                );
            }
        }
        if s.tool.is_spline() && preview.len() >= 2 {
            // Temporary exact curve definitions produce only a display tessellation.
            // The committed source remains the clicked fit/control points until Finish.
            let mut candidate = sketch_definition(s);
            let controls = preview
                .iter()
                .map(|point| {
                    let key = id();
                    candidate.points.insert(key.clone(), *point);
                    key
                })
                .collect::<Vec<_>>();
            let member = if s.tool == Tool::Spline {
                let key = id();
                candidate.members.insert(
                    key.clone(),
                    SketchMember {
                        construction: s.construction,
                        curve: SketchCurve::Bezier { points: controls },
                    },
                );
                Ok(key)
            } else {
                cad_core::sketch_spline::add(
                    w.session.visible(),
                    &s.definition,
                    &mut candidate,
                    &controls,
                    &cad_core::sketch_spline::Create {
                        fit: s.tool == Tool::InterpolatingSpline,
                        degree: s.spline_degree.min(preview.len() - 1),
                        parameterization: s.spline_parameterization,
                        construction: s.construction,
                    },
                )
            };
            if let Ok(member) = member
                && let Ok(paths) = cad_core::sketch_geometry::polylines(
                    w.session.visible(),
                    &s.definition,
                    &candidate,
                )
            {
                for path in paths.iter().filter(|path| path.member == member) {
                    draw_polyline(
                        w,
                        ui,
                        r,
                        (&plane, &matrix),
                        &path.points,
                        path.closed,
                        Stroke::new(2.5, blue),
                    );
                    if let Ok(mirrors) = cad_core::sketch_transform::dynamic_preview(
                        w.session.visible(),
                        &s.definition,
                        &sketch_definition(s),
                        &path.points,
                    ) {
                        for points in mirrors {
                            draw_polyline(
                                w,
                                ui,
                                r,
                                (&plane, &matrix),
                                &points,
                                path.closed,
                                Stroke::new(2.5, blue.gamma_multiply(0.65)),
                            );
                        }
                    }
                }
            }
        }
        if s.tool.conic_kind().is_some() && preview.len() >= 3 {
            let mut candidate = sketch_definition(s);
            if let Some(request) = conic_request(
                &mut candidate,
                s.tool,
                &preview,
                &s.conic_rho,
                s.clockwise,
                s.construction,
            ) && let Ok(member) = cad_core::sketch_conic::add(
                w.session.visible(),
                &s.definition,
                &mut candidate,
                &request,
            ) && let Ok(paths) =
                cad_core::sketch_geometry::polylines(w.session.visible(), &s.definition, &candidate)
            {
                let outputs = candidate.conics[&member].outputs(&member);
                for path in paths.iter().filter(|path| outputs.contains(&path.member)) {
                    draw_polyline(
                        w,
                        ui,
                        r,
                        (&plane, &matrix),
                        &path.points,
                        path.closed,
                        Stroke::new(2.5, blue),
                    );
                    if let Ok(mirrors) = cad_core::sketch_transform::dynamic_preview(
                        w.session.visible(),
                        &s.definition,
                        &sketch_definition(s),
                        &path.points,
                    ) {
                        for points in mirrors {
                            draw_polyline(
                                w,
                                ui,
                                r,
                                (&plane, &matrix),
                                &points,
                                path.closed,
                                Stroke::new(2.5, blue.gamma_multiply(0.65)),
                            );
                        }
                    }
                }
            }
        }
    }
    let points = w
        .sketch
        .as_ref()
        .map(|s| s.points.clone())
        .unwrap_or_default();
    let picked_member = w
        .sketch
        .as_ref()
        .and_then(|s| {
            cad_core::sketch_geometry::polylines(
                w.session.visible(),
                &s.definition,
                &display_definition(s),
            )
            .ok()
        })
        .and_then(|paths| {
            pointer.and_then(|pointer| {
                paths
                    .into_iter()
                    .filter_map(|path| {
                        let distance = polyline_distance(
                            w,
                            r,
                            (&plane, &matrix),
                            &path.points,
                            path.closed,
                            pointer,
                            None,
                        );
                        (distance < 7.).then_some((distance, path.member))
                    })
                    .min_by(|a, b| a.0.total_cmp(&b.0))
                    .map(|(_, member)| member)
            })
        });
    for pair in crate::sketch_ui::model_dimension_highlight(w).windows(2) {
        if let (Some(a), Some(b)) = (
            viewport::project(w, r, pair[0]),
            viewport::project(w, r, pair[1]),
        ) {
            ui.painter()
                .with_clip_rect(r)
                .line_segment([a, b], Stroke::new(3., accent));
        }
    }
    let mut handle_clicked = crate::sketch_ui::annotations(w, ui, r);
    handle_clicked |= crate::sketch_handles::overlay(w, ui, r);
    let mut selection_click = None;
    let mut placed_hole_center = None;
    let suppressed_points = w
        .sketch
        .as_ref()
        .map(|s| cad_core::sketch_transform::suppressed_points(&sketch_definition(s)))
        .unwrap_or_default();
    for (index, p) in points.iter().enumerate() {
        if w.sketch
            .as_ref()
            .is_some_and(|s| suppressed_points.contains(&s.keys[index]))
        {
            continue;
        }
        if let Some(pos) = viewport::project(w, r, world(&plane, &matrix, *p)) {
            if w.sketch.as_ref().is_some_and(|s| s.tool == Tool::Select) {
                continue;
            }
            let key = &w.sketch.as_ref().expect("active sketch").keys[index];
            let active = related.contains(key) || w.sketch.as_ref().is_some_and(|s| s.selection.iter().any(|e| matches!(e, cad_core::sketch_actions::SketchEntity::Point { point } if point == key)));
            ui.painter().with_clip_rect(r).circle_filled(
                pos,
                if active { 5. } else { 3. },
                if active {
                    accent
                } else if problems.contains(key) {
                    ui.visuals().error_fg_color
                } else {
                    blue
                },
            );
        }
    }
    if response.clicked()
        && !handle_clicked
        && pointer.is_some_and(|p| !toolbar.contains(p))
        && let Some(cursor) = cursor
        && let Some(s) = &mut w.sketch
    {
        let selecting = s.tool == Tool::Select;
        if !selecting {
            s.placement_error = None;
            s.history.push(sketch_definition(s));
        }
        let cursor = snapped.map(|i| s.points[i]).unwrap_or(cursor);
        match s.tool {
            Tool::Select => {
                let entity = picked_member
                    .clone()
                    .map(|member| cad_core::sketch_actions::SketchEntity::Member { member });
                selection_click = Some((entity, response.clone()));
            }
            Tool::Point => {
                placed_hole_center = Some(
                    snapped
                        .map(|index| s.keys[index].clone())
                        .unwrap_or_else(|| add_point(s, cursor, false)),
                );
                s.closed = true;
            }
            Tool::Polyline => {
                let key = snapped
                    .map(|i| s.keys[i].clone())
                    .unwrap_or_else(|| add_point(s, cursor, false));
                if let Some(last) = s.chain.last().cloned() {
                    line(s, last, key.clone());
                }
                if s.chain.first() == Some(&key) && s.chain.len() >= 3 {
                    s.chain.clear();
                } else {
                    s.chain.push(key);
                }
            }
            Tool::Spline | Tool::InterpolatingSpline | Tool::Nurbs => {
                if s.chain.len()
                    < if s.tool == Tool::Spline {
                        16
                    } else {
                        cad_core::sketch_spline::MAX_POINTS
                    }
                {
                    let key = snapped
                        .map(|i| s.keys[i].clone())
                        .unwrap_or_else(|| add_point(s, cursor, false));
                    if s.chain.last() != Some(&key) {
                        s.chain.push(key);
                    }
                }
            }
            Tool::Conic | Tool::Parabola | Tool::EllipseArc | Tool::ArcSlot => {
                s.click_join.resize(s.clicks.len(), false);
                s.clicks.push(cursor);
                s.click_join.push(snapped.is_some());
                placed_conic = s.clicks.len() == if s.tool == Tool::ArcSlot { 4 } else { 3 };
            }
            Tool::Rectangle
            | Tool::CenterRectangle
            | Tool::Circle
            | Tool::Polygon
            | Tool::Arc
            | Tool::Ellipse
            | Tool::Slot
            | Tool::ThreePointRectangle
            | Tool::ThreePointCenterRectangle
            | Tool::Parallelogram
            | Tool::PerimeterCircle
            | Tool::CenterArc
            | Tool::TangentArc
            | Tool::CenterSlot
            | Tool::MidpointLine
            | Tool::Centerline => {
                s.click_join.resize(s.clicks.len(), false);
                s.clicks.push(cursor);
                s.click_join.push(snapped.is_some());
                let required = if matches!(
                    s.tool,
                    Tool::Arc
                        | Tool::Ellipse
                        | Tool::Slot
                        | Tool::CenterSlot
                        | Tool::ThreePointRectangle
                        | Tool::ThreePointCenterRectangle
                        | Tool::Parallelogram
                        | Tool::PerimeterCircle
                        | Tool::CenterArc
                ) {
                    3
                } else {
                    2
                };
                if s.clicks.len() == required {
                    let joins = s
                        .clicks
                        .iter()
                        .zip(&s.click_join)
                        .filter(|(_, join)| **join)
                        .map(|(point, _)| *point)
                        .collect::<Vec<_>>();
                    let join_existing = |point: [f64; 2]| {
                        joins
                            .iter()
                            .any(|target| (point[0] - target[0]).hypot(point[1] - target[1]) < 1e-7)
                    };
                    let a = s.clicks[0];
                    let b = s.clicks[1];
                    match s.tool {
                        Tool::Rectangle | Tool::CenterRectangle => {
                            let (lo, hi) = if s.tool == Tool::CenterRectangle {
                                let half = [(b[0] - a[0]).abs(), (b[1] - a[1]).abs()];
                                (
                                    [a[0] - half[0], a[1] - half[1]],
                                    [a[0] + half[0], a[1] + half[1]],
                                )
                            } else {
                                (
                                    [a[0].min(b[0]), a[1].min(b[1])],
                                    [a[0].max(b[0]), a[1].max(b[1])],
                                )
                            };
                            let keys = [lo, [hi[0], lo[1]], hi, [lo[0], hi[1]]]
                                .map(|p| add_point(s, p, join_existing(p)));
                            for i in 0..4 {
                                line(s, keys[i].clone(), keys[(i + 1) % 4].clone());
                            }
                            for (kind, a, b, value) in [
                                ("horizontal", 0, 1, "0 mm".into()),
                                ("vertical", 1, 2, "0 mm".into()),
                                ("horizontal", 2, 3, "0 mm".into()),
                                ("vertical", 3, 0, "0 mm".into()),
                            ] {
                                s.constraints.push(SketchConstraint {
                                    driven: false,
                                    distance_side: None,
                                    contact: None,
                                    id: id(),
                                    kind: kind.into(),
                                    entities: vec![keys[a].clone(), keys[b].clone()],
                                    value: Expr(value),
                                });
                            }
                            if s.tool == Tool::CenterRectangle {
                                let center = add_point(s, a, join_existing(a));
                                let diagonal = id();
                                s.members.insert(
                                    diagonal.clone(),
                                    SketchMember {
                                        construction: true,
                                        curve: SketchCurve::Line {
                                            start: keys[0].clone(),
                                            end: keys[2].clone(),
                                        },
                                    },
                                );
                                s.constraints.push(SketchConstraint {
                                    driven: false,
                                    distance_side: None,
                                    contact: None,
                                    id: id(),
                                    kind: "midpoint".into(),
                                    entities: vec![center, diagonal],
                                    value: Expr::from("0 mm"),
                                });
                            }
                        }
                        Tool::ThreePointRectangle
                        | Tool::ThreePointCenterRectangle
                        | Tool::Parallelogram => {
                            match cad_core::sketch_construction::rectangle(
                                a,
                                b,
                                s.clicks[2],
                                s.tool == Tool::ThreePointCenterRectangle,
                                s.tool == Tool::Parallelogram,
                            ) {
                                Ok(points) => {
                                    let keys = points.map(|p| add_point(s, p, join_existing(p)));
                                    let mut members = Vec::new();
                                    for i in 0..4 {
                                        line(s, keys[i].clone(), keys[(i + 1) % 4].clone());
                                        if let Some(key) = &s.selected_member {
                                            members.push(key.clone());
                                        }
                                    }
                                    if members.len() == 4 {
                                        for (kind, first, second) in
                                            [("parallel", 0, 2), ("parallel", 1, 3)]
                                        {
                                            s.constraints.push(SketchConstraint {
                                                driven: false,
                                                distance_side: None,
                                                contact: None,
                                                id: id(),
                                                kind: kind.into(),
                                                entities: vec![
                                                    members[first].clone(),
                                                    members[second].clone(),
                                                ],
                                                value: "0 mm".into(),
                                            });
                                        }
                                        if s.tool != Tool::Parallelogram {
                                            s.constraints.push(SketchConstraint {
                                                driven: false,
                                                distance_side: None,
                                                contact: None,
                                                id: id(),
                                                kind: "perpendicular".into(),
                                                entities: vec![
                                                    members[0].clone(),
                                                    members[1].clone(),
                                                ],
                                                value: "0 mm".into(),
                                            });
                                        }
                                        if s.tool == Tool::ThreePointCenterRectangle {
                                            let center = add_point(s, a, join_existing(a));
                                            let diagonal = id();
                                            s.members.insert(
                                                diagonal.clone(),
                                                SketchMember {
                                                    construction: true,
                                                    curve: SketchCurve::Line {
                                                        start: keys[0].clone(),
                                                        end: keys[2].clone(),
                                                    },
                                                },
                                            );
                                            s.constraints.push(SketchConstraint {
                                                driven: false,
                                                distance_side: None,
                                                contact: None,
                                                id: id(),
                                                kind: "midpoint".into(),
                                                entities: vec![center, diagonal],
                                                value: "0 mm".into(),
                                            });
                                        }
                                    }
                                }
                                Err(error) => s.placement_error = Some(error.to_string()),
                            }
                        }
                        Tool::Centerline | Tool::MidpointLine => {
                            let (start, end) = if s.tool == Tool::MidpointLine {
                                ([2. * a[0] - b[0], 2. * a[1] - b[1]], b)
                            } else {
                                (a, b)
                            };
                            let start = add_point(s, start, join_existing(start));
                            let end = add_point(s, end, join_existing(end));
                            if start != end {
                                let member = id();
                                s.members.insert(
                                    member.clone(),
                                    SketchMember {
                                        construction: s.construction || s.tool == Tool::Centerline,
                                        curve: SketchCurve::Line { start, end },
                                    },
                                );
                                s.selected_member = Some(member.clone());
                                s.closed = true;
                                if s.tool == Tool::MidpointLine {
                                    let midpoint = add_point(s, a, join_existing(a));
                                    s.constraints.push(SketchConstraint {
                                        driven: false,
                                        distance_side: None,
                                        contact: None,
                                        id: id(),
                                        kind: "midpoint".into(),
                                        entities: vec![midpoint, member],
                                        value: "0 mm".into(),
                                    });
                                }
                            }
                        }
                        Tool::PerimeterCircle => {
                            match cad_core::sketch_geometry::arc_frame(a, s.clicks[2], b) {
                                Ok((center, radius, _, _)) => {
                                    let center = add_point(s, center, false);
                                    add_member(
                                        s,
                                        SketchCurve::Circle {
                                            center,
                                            radius: Expr(format!("{radius} mm")),
                                        },
                                    );
                                }
                                Err(error) => s.placement_error = Some(error.to_string()),
                            }
                        }
                        Tool::CenterArc => {
                            match cad_core::sketch_construction::center_arc(
                                a,
                                b,
                                s.clicks[2],
                                s.clockwise,
                            ) {
                                Ok(points) => {
                                    let center = add_point(s, a, join_existing(a));
                                    let start = add_point(s, points[0], join_existing(points[0]));
                                    let through = add_point(s, points[1], false);
                                    let end = add_point(s, points[2], join_existing(points[2]));
                                    add_member(
                                        s,
                                        SketchCurve::Arc {
                                            start,
                                            through,
                                            end,
                                        },
                                    );
                                    if let Some(arc) = &s.selected_member {
                                        s.constraints.push(SketchConstraint {
                                            driven: false,
                                            distance_side: None,
                                            contact: None,
                                            id: id(),
                                            kind: "arc_center".into(),
                                            entities: vec![center, arc.clone()],
                                            value: "0 mm".into(),
                                        });
                                    }
                                }
                                Err(error) => s.placement_error = Some(error.to_string()),
                            }
                        }
                        Tool::TangentArc => {
                            let source = sketch_definition(s);
                            let incident = source
                                .members
                                .iter()
                                .filter_map(|(key, m)| {
                                    if let SketchCurve::Line { start, end } = &m.curve {
                                        let p = source.points[start];
                                        let q = source.points[end];
                                        if (a[0] - q[0]).hypot(a[1] - q[1]) < 1e-7 {
                                            Some((key.clone(), [q[0] - p[0], q[1] - p[1]]))
                                        } else if (a[0] - p[0]).hypot(a[1] - p[1]) < 1e-7 {
                                            Some((key.clone(), [p[0] - q[0], p[1] - q[1]]))
                                        } else {
                                            None
                                        }
                                    } else {
                                        None
                                    }
                                })
                                .collect::<Vec<_>>();
                            if incident.len() == 1 {
                                match cad_core::sketch_construction::tangent_arc(
                                    a,
                                    b,
                                    incident[0].1,
                                ) {
                                    Ok(points) => {
                                        let start =
                                            add_point(s, points[0], join_existing(points[0]));
                                        let through = add_point(s, points[1], false);
                                        let end = add_point(s, points[2], join_existing(points[2]));
                                        add_member(
                                            s,
                                            SketchCurve::Arc {
                                                start,
                                                through,
                                                end,
                                            },
                                        );
                                        if let Some(arc) = &s.selected_member {
                                            s.constraints.push(SketchConstraint {
                                                driven: false,
                                                distance_side: None,
                                                contact: None,
                                                id: id(),
                                                kind: "tangent".into(),
                                                entities: vec![incident[0].0.clone(), arc.clone()],
                                                value: "0 mm".into(),
                                            });
                                        }
                                    }
                                    Err(error) => s.placement_error = Some(error.to_string()),
                                }
                            } else {
                                s.placement_error=Some("Choose an endpoint belonging to exactly one straight member for Tangent Arc".into());
                            }
                        }
                        Tool::Circle => {
                            let center = add_point(s, a, join_existing(a));
                            add_member(
                                s,
                                SketchCurve::Circle {
                                    center,
                                    radius: Expr(format!(
                                        "{} mm",
                                        (b[0] - a[0]).hypot(b[1] - a[1])
                                    )),
                                },
                            );
                        }
                        Tool::Polygon => {
                            let radius = (b[0] - a[0]).hypot(b[1] - a[1]);
                            let angle = (b[1] - a[1]).atan2(b[0] - a[0]);
                            let keys = (0..s.polygon_sides)
                                .map(|i| {
                                    let t = angle
                                        + i as f64 / s.polygon_sides as f64 * std::f64::consts::TAU;
                                    add_point(
                                        s,
                                        [a[0] + radius * t.cos(), a[1] + radius * t.sin()],
                                        join_existing([
                                            a[0] + radius * t.cos(),
                                            a[1] + radius * t.sin(),
                                        ]),
                                    )
                                })
                                .collect::<Vec<_>>();
                            for i in 0..keys.len() {
                                line(s, keys[i].clone(), keys[(i + 1) % keys.len()].clone());
                            }
                        }
                        Tool::Arc => {
                            let start = add_point(s, a, join_existing(a));
                            let end = add_point(s, b, join_existing(b));
                            let through = add_point(s, s.clicks[2], false);
                            add_member(
                                s,
                                SketchCurve::Arc {
                                    start,
                                    through,
                                    end,
                                },
                            );
                        }
                        Tool::Ellipse => {
                            let major = (b[0] - a[0]).hypot(b[1] - a[1]);
                            let rotation = (b[1] - a[1]).atan2(b[0] - a[0]);
                            let c = s.clicks[2];
                            let minor = (-(c[0] - a[0]) * rotation.sin()
                                + (c[1] - a[1]) * rotation.cos())
                            .abs();
                            let center = add_point(s, a, join_existing(a));
                            add_member(
                                s,
                                SketchCurve::Ellipse {
                                    center,
                                    major: Expr(format!("{major} mm")),
                                    minor: Expr(format!("{minor} mm")),
                                    angle: Expr(format!("{} deg", rotation.to_degrees())),
                                },
                            );
                        }
                        Tool::Slot | Tool::CenterSlot => {
                            let a = if s.tool == Tool::CenterSlot {
                                [2. * a[0] - b[0], 2. * a[1] - b[1]]
                            } else {
                                a
                            };
                            let len = (b[0] - a[0]).hypot(b[1] - a[1]);
                            if len > 1e-9 {
                                let tangent = [(b[0] - a[0]) / len, (b[1] - a[1]) / len];
                                let n = [-tangent[1], tangent[0]];
                                let c = s.clicks[2];
                                let radius = ((c[0] - b[0]) * n[0] + (c[1] - b[1]) * n[1]).abs();
                                let k = [
                                    add_point(
                                        s,
                                        [a[0] + n[0] * radius, a[1] + n[1] * radius],
                                        join_existing([a[0] + n[0] * radius, a[1] + n[1] * radius]),
                                    ),
                                    add_point(
                                        s,
                                        [b[0] + n[0] * radius, b[1] + n[1] * radius],
                                        join_existing([b[0] + n[0] * radius, b[1] + n[1] * radius]),
                                    ),
                                    add_point(
                                        s,
                                        [b[0] - n[0] * radius, b[1] - n[1] * radius],
                                        join_existing([b[0] - n[0] * radius, b[1] - n[1] * radius]),
                                    ),
                                    add_point(
                                        s,
                                        [a[0] - n[0] * radius, a[1] - n[1] * radius],
                                        join_existing([a[0] - n[0] * radius, a[1] - n[1] * radius]),
                                    ),
                                ];
                                line(s, k[0].clone(), k[1].clone());
                                let m = add_point(
                                    s,
                                    [b[0] + tangent[0] * radius, b[1] + tangent[1] * radius],
                                    false,
                                );
                                add_member(
                                    s,
                                    SketchCurve::Arc {
                                        start: k[1].clone(),
                                        through: m,
                                        end: k[2].clone(),
                                    },
                                );
                                line(s, k[2].clone(), k[3].clone());
                                let m = add_point(
                                    s,
                                    [a[0] - tangent[0] * radius, a[1] - tangent[1] * radius],
                                    false,
                                );
                                add_member(
                                    s,
                                    SketchCurve::Arc {
                                        start: k[3].clone(),
                                        through: m,
                                        end: k[0].clone(),
                                    },
                                );
                            }
                        }
                        _ => {}
                    }
                    s.clicks.clear();
                    s.click_join.clear();
                }
            }
        }
        edited |= !selecting;
    }
    if response.secondary_clicked() && !handle_clicked {
        selection_click = Some((
            picked_member
                .clone()
                .map(|member| cad_core::sketch_actions::SketchEntity::Member { member }),
            response.clone(),
        ));
    }
    if let Some((mut entity, hit)) = selection_click {
        let multiselect = ui.input(|i| i.modifiers.shift || i.modifiers.command);
        let dimension_mode = w.fields.contains_key("smart-dimension-mode");
        let mut converted_model = false;
        if entity.is_none()
            && (dimension_mode || multiselect)
            && w.sketch.as_ref().is_some_and(|s| s.edit.is_none())
            && let Some(pointer) = hit.interact_pointer_pos()
        {
            let definition = w.sketch.as_ref().unwrap().definition.clone();
            if let Some(model_hit) = viewport::pick(w, r, pointer)
                .into_iter()
                .find(|s| s.definition == definition && s.reference.topology == "edge")
            {
                if dimension_mode {
                    match crate::sketch_ui::model_edge_dimension_pick(w, &model_hit) {
                        Ok(selected) => {
                            entity = Some(selected);
                            converted_model = true;
                        }
                        Err(error) => {
                            w.status = error.to_string();
                            return;
                        }
                    }
                } else {
                    if let Some(s) = &mut w.sketch {
                        s.dimension_model = Some(model_hit);
                    }
                    w.selection_popup = None;
                    w.context_selection = None;
                    w.status = "Model edge selected. Command-click a sketch center / member, then Smart Dimension to enter its value.".into();
                    return;
                }
            }
        }
        if entity.is_none()
            && let Some(pointer) = hit.interact_pointer_pos()
        {
            let project = |p| viewport::project(w, r, world(&plane, &matrix, p));
            if project([0., 0.]).is_some_and(|p| p.distance(pointer) < 9.) {
                entity = Some(cad_core::sketch_actions::SketchEntity::Origin);
            } else {
                let distance = |a: [f64; 2], b: [f64; 2]| -> Option<f32> {
                    let a = project(a)?;
                    let b = project(b)?;
                    let edge = b - a;
                    let t = ((pointer - a).dot(edge) / edge.length_sq().max(1e-8)).clamp(0., 1.);
                    Some(pointer.distance(a + edge * t))
                };
                if distance([-extent, 0.], [extent, 0.]).is_some_and(|d| d < 5.) {
                    entity = Some(cad_core::sketch_actions::SketchEntity::XAxis);
                } else if distance([0., -extent], [0., extent]).is_some_and(|d| d < 5.) {
                    entity = Some(cad_core::sketch_actions::SketchEntity::YAxis);
                }
            }
        }
        if entity.is_none()
            && !w.fields.contains_key("smart-dimension-mode")
            && w.sketch.as_ref().is_some_and(|s| s.edit.is_none())
            && let Some(pointer) = hit.interact_pointer_pos()
        {
            let definition = w.sketch.as_ref().unwrap().definition.clone();
            if let Some(model_hit) = viewport::pick(w, r, pointer).into_iter().find(|s| {
                s.definition == definition
                    && matches!(s.reference.topology.as_str(), "edge" | "face")
            }) {
                let target = cad_core::selection_actions::SelectionTarget::Geometry {
                    definition: model_hit.definition.clone(),
                    body: model_hit.body.clone(),
                    instance: model_hit.instance.clone(),
                    path: model_hit.path.clone(),
                    reference: model_hit.reference.clone(),
                    planar: false,
                };
                crate::sketch_ui::select(w, None, false);
                w.selected = Some(model_hit);
                w.context_selection = Some(target);
                crate::selection_ui::open(w, &hit, pointer);
                return;
            }
        }
        let continue_dimension = w.fields.contains_key("smart-dimension-mode")
            && w.sketch.as_ref().is_some_and(|s| s.selection.len() == 1);
        let picked_entity = entity.is_some();
        let preserve_context_pair = hit.secondary_clicked()
            && entity.as_ref().is_some_and(|entity| {
                w.sketch
                    .as_ref()
                    .is_some_and(|s| s.selection.len() > 1 && s.selection.contains(entity))
            });
        if !preserve_context_pair {
            crate::sketch_ui::select(w, entity, continue_dimension || multiselect);
        }
        if w.fields.contains_key("smart-dimension-mode") {
            crate::sketch_ui::selection_changed(w);
            if converted_model && w.sketch.as_ref().is_some_and(|s| s.dimension.is_none()) {
                solve(w);
            }
        } else if picked_entity
            && w.sketch.as_ref().is_some_and(|s| !s.selection.is_empty())
            && (hit.secondary_clicked() || !multiselect)
        {
            crate::selection_ui::open(w, &hit, hit.interact_pointer_pos().unwrap_or(r.center()));
        }
    }
    if let Some(point) = placed_hole_center {
        crate::holes_ui::remember_placed_center(w, point);
    }
    if edited {
        solve(w);
        if let Some(error) = w.sketch.as_ref().and_then(|s| s.placement_error.as_ref()) {
            w.status = error.clone();
        }
    }
    if placed_conic {
        finish_conic(w);
    }
    if let (Some(cursor), Some(p)) = (cursor, pointer) {
        ui.painter().with_clip_rect(r).text(
            p + egui::vec2(12., 16.),
            egui::Align2::LEFT_TOP,
            format!("{:.2}, {:.2} mm", cursor[0], cursor[1]),
            egui::FontId::monospace(11.),
            blue,
        );
    }
}
