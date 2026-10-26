//! Spatial sketch authoring over shared feature drafts and persistent workers.
//! Native solving stays outside the UI process. Controls, handles and annotations
//! bind stable semantic identities; pixels never become sketch authority.
use crate::{Workspace, presentation_controls as controls, viewport, worker::WorkerClient};
use bevy::math::DVec3;
use bevy_egui::egui;
use cad_core::{
    CadError, Dimension, Expr, Feature, Id, NodeRef, Operation, Plane, Result, Session,
    commands::Command,
    presentation::SemanticElement,
    protocol::{RequestIdentity, WorkerRequest, WorkerResponse},
    sketch3d::{self as spatial, Axis, Constraint, Curve, Definition, Member, Relation},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::time::Instant;

pub const CONTRACT_VERSION: u32 = 1;
const ACTION: &str = "sketch3d.action";
const INVALID: &str = "sketch3d-invalid:";
const HISTORY_LIMIT: usize = 64;
#[derive(Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Tool {
    #[default]
    Select,
    Line,
    Point,
    Arc,
    Circle,
    Bezier,
    Nurbs,
    InterpolatingSpline,
    Dimension,
}
impl Tool {
    fn label(self) -> &'static str {
        match self {
            Self::Select => "Select",
            Self::Line => "Line / Polyline",
            Self::Point => "Point",
            Self::Arc => "3-Point Arc",
            Self::Circle => "Circle",
            Self::Bezier => "Bézier",
            Self::Nurbs => "NURBS",
            Self::InterpolatingSpline => "Interpolating Spline",
            Self::Dimension => "Smart Dimension",
        }
    }
}
#[derive(Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DrawingPlane {
    #[default]
    XY,
    YZ,
    ZX,
}
impl DrawingPlane {
    fn label(self) -> &'static str {
        match self {
            Self::XY => "XY",
            Self::YZ => "YZ",
            Self::ZX => "ZX",
        }
    }
    fn normal(self) -> usize {
        match self {
            Self::XY => 2,
            Self::YZ => 0,
            Self::ZX => 1,
        }
    }
    fn next(self) -> Self {
        match self {
            Self::XY => Self::YZ,
            Self::YZ => Self::ZX,
            Self::ZX => Self::XY,
        }
    }
    fn frame(self, origin: [f64; 3]) -> Plane {
        let (a, b) = match self {
            Self::XY => (0, 1),
            Self::YZ => (1, 2),
            Self::ZX => (2, 0),
        };
        Plane {
            origin,
            u: std::array::from_fn(|i| if i == a { 1. } else { 0. }),
            v: std::array::from_fn(|i| if i == b { 1. } else { 0. }),
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Entity {
    Point { point: Id },
    Member { member: Id },
    Origin,
}
impl Entity {
    fn key(&self) -> &str {
        match self {
            Self::Point { point } => point,
            Self::Member { member } => member,
            Self::Origin => "@origin",
        }
    }
}
struct Drag {
    point: Id,
    axis: Option<Axis>,
    viewport: egui::Id,
    session: Session,
    matrix: [[f64; 4]; 4],
    position: [f64; 3],
    plane: Plane,
    grab: [f64; 3],
    axis_grab: f64,
    last: Option<[f64; 3]>,
    released: bool,
    sequence: u64,
    pending: Option<RequestIdentity>,
    history_len: usize,
}
fn hit_plane(
    w: &Workspace,
    rect: egui::Rect,
    cursor: egui::Pos2,
    plane: &Plane,
    matrix: &[[f64; 4]; 4],
) -> Option<[f64; 3]> {
    let uv = crate::sketch::hit_plane(w, rect, cursor, plane, matrix)?;
    Some(std::array::from_fn(|index| {
        plane.origin[index] + plane.u[index] * uv[0] + plane.v[index] * uv[1]
    }))
}
fn axis_cursor(
    w: &Workspace,
    rect: egui::Rect,
    cursor: egui::Pos2,
    origin: [f64; 3],
    matrix: &[[f64; 4]; 4],
    axis: Axis,
) -> Option<f64> {
    let (ray_origin, ray_direction) = viewport::ray(w, rect, cursor);
    let ray_direction = DVec3::from_array(ray_direction);
    let world_origin = DVec3::from_array(transform(matrix, origin));
    let direction = DVec3::new(
        matrix[0][axis.index()],
        matrix[1][axis.index()],
        matrix[2][axis.index()],
    )
    .try_normalize()?;
    let delta = DVec3::from_array(ray_origin) - world_origin;
    let alignment = direction.dot(ray_direction);
    let divisor = 1. - alignment * alignment;
    (divisor > 1e-8)
        .then(|| (direction.dot(delta) - alignment * ray_direction.dot(delta)) / divisor)
        .filter(|value| value.is_finite())
}
fn start_drag(
    w: &mut Workspace,
    rect: egui::Rect,
    response: &egui::Response,
    cursor: egui::Pos2,
    point: Id,
    direction: Option<Axis>,
) -> Result<()> {
    let definition = source(w).ok_or_else(|| invalid("The sketch is missing"))?;
    let display = displayed(w).ok_or_else(|| invalid("The sketch is unavailable"))?;
    let current = feature(w).ok_or_else(|| invalid("The feature is missing"))?;
    let owner = w
        .sketch3d
        .as_ref()
        .ok_or_else(|| invalid("The editor closed"))?
        .owner
        .clone();
    if let Some(configuration) = w.session.visible().active_configurations.get(&owner) {
        let resolved =
            cad_core::configuration::resolve(w.session.visible(), &owner, configuration)?;
        if let Some(overrides) = resolved.overrides.feature_dimensions.get(&current.id) {
            let fields = cad_core::configuration::dimensions::fields(&current)?;
            for (index, constraint) in definition.constraints.iter().enumerate() {
                if !constraint.driven
                    && matches!(
                        constraint.relation,
                        Relation::PointOnCurve { free: true, .. }
                    )
                    && fields.iter().any(|field| {
                        field.pointer
                            == format!("/definition/constraints/{index}/relation/parameter")
                            && overrides.contains_key(&field.path)
                    })
                {
                    return Err(invalid(
                        "The active configuration owns a free curve-position seed. Edit or remove that configured seed before dragging",
                    ));
                }
            }
        }
    }
    let position = *display
        .points
        .get(&point)
        .ok_or_else(|| invalid("The point is missing"))?;
    let matrix =
        part_matrix(w).ok_or_else(|| invalid("Choose the intended part occurrence first"))?;
    let state = w
        .sketch3d
        .as_ref()
        .ok_or_else(|| invalid("The editor closed"))?;
    let plane = state.plane.frame(position);
    let grab = hit_plane(w, rect, cursor, &plane, &matrix).unwrap_or(position);
    let axis_grab = direction
        .and_then(|axis| axis_cursor(w, rect, cursor, position, &matrix, axis))
        .unwrap_or(0.);
    let history_len = state.history.len();
    let session = w.session.clone();
    history(w);
    crate::navigation::stop(w);
    w.selection_popup = None;
    w.confirm_requested = false;
    if let Some(state) = &mut w.sketch3d {
        state.selection = vec![Entity::Point {
            point: point.clone(),
        }];
        state.popup = None;
        state.dimension = None;
        state.drag = Some(Drag {
            point,
            axis: direction,
            viewport: response.id,
            session,
            matrix,
            position,
            plane,
            grab,
            axis_grab,
            last: None,
            released: false,
            sequence: 0,
            pending: None,
            history_len,
        });
    }
    w.status="Drag in the current plane or along the selected XYZ axis · Tab changes plane · Alt bypasses snap · Escape cancels this drag".into();
    Ok(())
}
fn request_drag(w: &mut Workspace, position: [f64; 3]) -> Result<()> {
    let identity = w.session.identity();
    let state = w
        .sketch3d
        .as_mut()
        .ok_or_else(|| invalid("The editor has closed"))?;
    let drag = state
        .drag
        .as_mut()
        .ok_or_else(|| invalid("No active spatial handle"))?;
    if drag.last == Some(position) {
        return Ok(());
    }
    let document = w.session.visible().clone();
    state.sequence = state.sequence.saturating_add(1);
    drag.sequence = state.sequence;
    drag.pending = Some(identity.clone());
    drag.last = Some(position);
    state.worker.submit(WorkerRequest::SolveSketch3d {
        identity,
        document,
        feature: NodeRef {
            definition: state.owner.clone(),
            node: state.feature.clone(),
        },
        editor: state.token.clone(),
        sequence: drag.sequence,
        targets: vec![spatial::DragTarget {
            point: drag.point.clone(),
            position,
        }],
    });
    Ok(())
}
pub fn escape_drag(w: &mut Workspace) -> bool {
    let Some(drag) = w.sketch3d.as_mut().and_then(|state| state.drag.take()) else {
        return false;
    };
    if let Some(state) = &mut w.sketch3d {
        state.worker.cancel();
        state.history.truncate(drag.history_len);
        state.hover = None;
        state.error = None;
    }
    w.worker.cancel();
    w.requested = None;
    w.session = drag.session;
    w.changed = Instant::now();
    w.rendered_key.clear();
    w.confirm_requested = false;
    if let Some(feature) = feature(w) {
        w.json = serde_json::to_string_pretty(&feature).unwrap_or_default();
    }
    w.status = "Spatial drag canceled; earlier sketch edits retained".into();
    true
}
pub fn poll(w: &mut Workspace) {
    let replies = w
        .sketch3d
        .as_ref()
        .map(|state| state.worker.drain())
        .unwrap_or_default();
    for reply in replies {
        let identity = w.session.identity();
        let accepted = match &reply {
            WorkerResponse::Sketch3dSolved {
                identity: result,
                feature,
                editor,
                sequence,
                ..
            }
            | WorkerResponse::Sketch3dSolveFailed {
                identity: result,
                feature,
                editor,
                sequence,
                ..
            } => w.sketch3d.as_ref().is_some_and(|state| {
                result == &identity
                    && editor == &state.token
                    && feature.definition == state.owner
                    && feature.node == state.feature
                    && state.drag.as_ref().is_some_and(|drag| {
                        *sequence == drag.sequence && drag.pending.as_ref() == Some(result)
                    })
            }),
            WorkerResponse::Error {
                identity: Some(result),
                ..
            } => w.sketch3d.as_ref().is_some_and(|state| {
                result == &identity
                    && state
                        .drag
                        .as_ref()
                        .is_some_and(|drag| drag.pending.as_ref() == Some(result))
            }),
            _ => false,
        };
        if !accepted {
            continue;
        }
        match reply {
            WorkerResponse::Sketch3dSolved { solved, report, .. } => {
                let Some(mut definition) = source(w) else {
                    continue;
                };
                if !report.converged
                    || definition.id != solved.id
                    || definition.points.keys().ne(solved.points.keys())
                {
                    if let Some(state) = &mut w.sketch3d {
                        state.error =
                            Some("Spatial drag returned incompatible source identities".into());
                        if let Some(drag) = &mut state.drag {
                            drag.pending = None;
                        }
                        if state.drag.as_ref().is_some_and(|drag| drag.released) {
                            state.drag = None;
                        }
                    }
                    continue;
                }
                let goal = w
                    .sketch3d
                    .as_ref()
                    .and_then(|state| state.drag.as_ref())
                    .and_then(|drag| drag.last.map(|position| (drag.point.clone(), position)));
                definition.points = solved.points;
                for constraint in &mut definition.constraints {
                    if !constraint.driven
                        && let Relation::PointOnCurve {
                            point,
                            member,
                            parameter,
                            free: true,
                        } = &mut constraint.relation
                        && let Some(result) = solved
                            .constraints
                            .iter()
                            .find(|result| result.id == constraint.id && !result.driven)
                        && let Relation::PointOnCurve {
                            point: result_point,
                            member: result_member,
                            parameter: result_parameter,
                            free: true,
                        } = &result.relation
                        && point == result_point
                        && member == result_member
                    {
                        *parameter = result_parameter.clone();
                    }
                }
                let limited = goal.is_some_and(|(point, goal)| {
                    definition.points.get(&point).is_some_and(|actual| {
                        DVec3::from_array(*actual).distance(DVec3::from_array(goal)) > 1e-4
                    })
                });
                if let Some(state) = &mut w.sketch3d
                    && let Some(drag) = &mut state.drag
                {
                    drag.pending = None;
                }
                if let Err(error) = publish(w, definition)
                    && let Some(state) = &mut w.sketch3d
                {
                    state.error = Some(error.to_string());
                }
                w.status = if limited {
                    "Movement limited by dimensions and relations · last solved geometry retained"
                } else {
                    "Live spatial sketch · release keeps the edit pending"
                }
                .into();
                if w.sketch3d
                    .as_ref()
                    .is_some_and(|state| state.drag.as_ref().is_some_and(|drag| drag.released))
                    && let Some(state) = &mut w.sketch3d
                {
                    state.drag = None;
                }
            }
            WorkerResponse::Sketch3dSolveFailed { message, .. }
            | WorkerResponse::Error { message, .. } => {
                w.status = format!("{message} · last solved sketch retained");
                if let Some(state) = &mut w.sketch3d {
                    state.error = Some(message);
                    if let Some(drag) = &mut state.drag {
                        drag.pending = None;
                    }
                    if state.drag.as_ref().is_some_and(|drag| drag.released) {
                        state.drag = None;
                    }
                }
            }
            _ => {}
        }
    }
    // An unrelated authoritative edit invalidates the captured goal rather than
    // leaving a released handle waiting for a result that can never be adopted.
    let identity = w.session.identity();
    if w.sketch3d.as_ref().is_some_and(|state| {
        state
            .drag
            .as_ref()
            .and_then(|drag| drag.pending.as_ref())
            .is_some_and(|pending| pending != &identity)
    }) && let Some(state) = &mut w.sketch3d
    {
        state.worker.cancel();
        state.drag = None;
        state.error = Some("Spatial cursor solve discarded because its source changed".into());
    }
}
fn snap(
    w: &Workspace,
    rect: egui::Rect,
    cursor: egui::Pos2,
    mut point: [f64; 3],
    bypass: bool,
) -> ([f64; 3], Option<Entity>) {
    let Some(state) = w.sketch3d.as_ref() else {
        return (point, None);
    };
    if bypass || !state.snap {
        return (point, None);
    }
    let Some(definition) = displayed(w) else {
        return (point, None);
    };
    let Some(matrix) = part_matrix(w) else {
        return (point, None);
    };
    let mut best = None;
    for (id, value) in &definition.points {
        if !crate::sketch_overlay::point3d_visible(w, &definition, id) {
            continue;
        }
        if let Some(position) = viewport::project(w, rect, transform(&matrix, *value)) {
            let distance = position.distance(cursor);
            if distance <= 9. && best.as_ref().is_none_or(|(_, _, prior)| distance < *prior) {
                best = Some((*value, Entity::Point { point: id.clone() }, distance));
            }
        }
    }
    if w.view.visibility.origins
        && let Some(position) = viewport::project(w, rect, transform(&matrix, [0.; 3]))
        && position.distance(cursor) <= 9.
        && best
            .as_ref()
            .is_none_or(|(point, _, prior)| *point == [0.; 3] || position.distance(cursor) < *prior)
    {
        best = Some(([0.; 3], Entity::Origin, position.distance(cursor)));
    }
    if let Some((position, entity, _)) = best {
        return (position, Some(entity));
    }
    let step = cad_core::expression::value(
        w.session.visible(),
        &state.owner,
        &state.grid,
        Dimension::Length,
    )
    .ok()
    .filter(|value| *value > 0.);
    if let Some(step) = step {
        let step = if state.fine { step * 0.1 } else { step };
        for (index, coordinate) in point.iter_mut().enumerate() {
            if index != state.plane.normal() {
                *coordinate = (*coordinate / step).round() * step;
            }
        }
    }
    if let Some(previous) = state
        .clicks
        .last()
        .and_then(|id| definition.points.get(id))
        .copied()
        && state.auto_axis
    {
        for index in 0..3 {
            let mut aligned = previous;
            aligned[index] = point[index];
            if let Some(position) = viewport::project(w, rect, transform(&matrix, aligned))
                && position.distance(cursor) <= 7.
            {
                point = aligned;
                break;
            }
        }
    }
    (point, None)
}
struct DimensionEdit {
    constraint: Id,
    checkpoint: Session,
    fields: std::collections::BTreeMap<String, String>,
    history_len: usize,
    kind: String,
}
struct Placement {
    checkpoint: Session,
    fields: std::collections::BTreeMap<String, String>,
    history_len: usize,
    redo: Vec<Definition>,
}
pub struct State {
    document: Id,
    pub owner: Id,
    pub feature: Id,
    token: Id,
    pub tool: Tool,
    plane: DrawingPlane,
    plane_origin: [f64; 3],
    occurrence: Option<Vec<Id>>,
    pub selection: Vec<Entity>,
    clicks: Vec<Id>,
    placement: Option<Placement>,
    hover: Option<[f64; 3]>,
    snap_target: Option<Entity>,
    snap: bool,
    grid: Expr,
    fine: bool,
    construction: bool,
    auto_axis: bool,
    inputs: [Expr; 3],
    radius: Expr,
    numeric_radius: bool,
    degree: usize,
    parameterization: cad_core::sketch_spline::Parameterization,
    history: Vec<Definition>,
    redo: Vec<Definition>,
    drag: Option<Drag>,
    dimension: Option<DimensionEdit>,
    popup: Option<egui::Pos2>,
    worker: WorkerClient,
    sequence: u64,
    error: Option<String>,
}
fn invalid(message: &str) -> CadError {
    CadError::Invalid(format!("3D sketch: {message}"))
}
fn feature(w: &Workspace) -> Option<Feature> {
    let state = w.sketch3d.as_ref()?;
    if state.document != w.session.visible().id || w.tool_checkpoint.is_none() {
        return None;
    }
    w.session
        .visible()
        .parts
        .get(&state.owner)?
        .features
        .iter()
        .find(|feature| feature.id == state.feature)
        .filter(|feature| matches!(feature.operation, Operation::Sketch3d { .. }))
        .cloned()
}
fn source(w: &Workspace) -> Option<Definition> {
    let feature = feature(w)?;
    let Operation::Sketch3d { definition } = feature.operation else {
        return None;
    };
    Some(*definition)
}
fn source_ref(w: &Workspace) -> Option<&Definition> {
    let state = w.sketch3d.as_ref()?;
    let feature = w
        .session
        .visible()
        .parts
        .get(&state.owner)?
        .features
        .iter()
        .find(|feature| feature.id == state.feature)?;
    match &feature.operation {
        Operation::Sketch3d { definition } => Some(definition),
        _ => None,
    }
}
pub fn active(w: &Workspace) -> bool {
    w.sketch3d.as_ref().is_some_and(|state| {
        state.document == w.session.visible().id
            && w.tool_checkpoint.is_some()
            && source_ref(w).is_some()
    })
}
pub fn captured(w: &Workspace) -> bool {
    w.sketch3d
        .as_ref()
        .is_some_and(|state| state.drag.is_some())
}
fn axis_name(axis: Axis) -> &'static str {
    match axis {
        Axis::X => "X",
        Axis::Y => "Y",
        Axis::Z => "Z",
    }
}
fn axis(index: usize) -> Axis {
    match index {
        0 => Axis::X,
        1 => Axis::Y,
        _ => Axis::Z,
    }
}
fn coordinate(value: f64) -> Expr {
    Expr(format!("{value} mm"))
}
fn identity_matrix() -> [[f64; 4]; 4] {
    std::array::from_fn(|i| std::array::from_fn(|j| if i == j { 1. } else { 0. }))
}
fn transform(matrix: &[[f64; 4]; 4], point: [f64; 3]) -> [f64; 3] {
    std::array::from_fn(|i| {
        matrix[i][0] * point[0] + matrix[i][1] * point[1] + matrix[i][2] * point[2] + matrix[i][3]
    })
}
fn part_matrix(w: &Workspace) -> Option<[[f64; 4]; 4]> {
    let state = w.sketch3d.as_ref()?;
    if w.session.visible().root == state.owner {
        return Some(identity_matrix());
    }
    let evaluation = viewport::display_evaluation(w)
        .or_else(|| w.tool_checkpoint.as_ref()?.evaluation.as_ref())?;
    let instances = viewport::display_instances(w, evaluation)
        .into_iter()
        .filter(|instance| {
            instance.definition == state.owner
                && state
                    .occurrence
                    .as_ref()
                    .is_none_or(|path| path == &instance.path)
                && w.view
                    .active_instance
                    .as_ref()
                    .is_none_or(|id| instance.path.last() == Some(id))
        })
        .collect::<Vec<_>>();
    (instances.len() == 1).then(|| instances[0].matrix)
}
fn current_evaluation(w: &Workspace) -> Option<&spatial::Evaluated> {
    let state = w.sketch3d.as_ref()?;
    let key = format!(
        "{}::{}",
        cad_core::configuration::key(
            &state.owner,
            w.session
                .visible()
                .active_configurations
                .get(&state.owner)
                .map(String::as_str)
        ),
        state.feature
    );
    w.session
        .draft
        .as_ref()
        .and_then(|draft| draft.evaluation.as_ref())
        .or(w.session.evaluation.as_ref())
        .filter(|evaluation| evaluation.identity == w.session.identity())?
        .spatial_sketches
        .get(&key)
}
fn display_evaluation(w: &Workspace) -> Option<&spatial::Evaluated> {
    if let Some(current) = current_evaluation(w) {
        return Some(current);
    }
    let state = w.sketch3d.as_ref()?;
    let key = format!(
        "{}::{}",
        cad_core::configuration::key(
            &state.owner,
            w.session
                .visible()
                .active_configurations
                .get(&state.owner)
                .map(String::as_str)
        ),
        state.feature
    );
    viewport::display_evaluation(w)?.spatial_sketches.get(&key)
}
fn displayed(w: &Workspace) -> Option<Definition> {
    let mut definition = source(w)?;
    if let Some(evaluation) = display_evaluation(w) {
        for (id, point) in &mut definition.points {
            if let Some(solved) = evaluation.solved.points.get(id) {
                *point = *solved;
            }
        }
    }
    Some(definition)
}
pub fn begin(w: &mut Workspace, existing: Option<Feature>) {
    if active(w) {
        return;
    }
    if w.sketch.is_some()
        || w.tool_checkpoint.is_some()
        || w.confirm_requested
        || w.settings.is_some()
    {
        w.status = "Finish or cancel the current operation before opening a 3D sketch".into();
        return;
    }
    let Some(owner) = w
        .view
        .active_part
        .clone()
        .filter(|owner| w.session.visible().parts.contains_key(owner))
    else {
        w.status = "Create or select a part first".into();
        return;
    };
    if cad_core::library::is_definition_pinned(w.session.visible(), &owner) {
        w.status = "Make the linked component unique before editing its 3D sketch".into();
        return;
    }
    let occurrence = w.selected_feature_path.clone().or_else(|| {
        w.selected
            .as_ref()
            .filter(|selection| selection.definition == owner)
            .map(|selection| selection.path.clone())
    });
    if let Some(occurrence) = &occurrence
        && let Some(evaluation) = viewport::display_evaluation(w)
        && let Some(instance) = viewport::display_instances(w, evaluation)
            .into_iter()
            .find(|instance| &instance.path == occurrence && instance.definition == owner)
        && instance.configuration.as_ref() != w.session.visible().active_configurations.get(&owner)
    {
        w.status =
            "Open the component's matching part configuration before editing its 3D sketch".into();
        return;
    }
    let editing_existing = existing.is_some();
    let feature = if let Some(feature) = existing {
        if !matches!(feature.operation, Operation::Sketch3d { .. }) {
            w.status = "Choose a spatial sketch feature".into();
            return;
        }
        feature
    } else {
        let count = w.session.visible().parts[&owner]
            .features
            .iter()
            .filter(|feature| matches!(feature.operation, Operation::Sketch3d { .. }))
            .count()
            + 1;
        let definition = Definition::new(format!("3D Sketch{count}"));
        Feature {
            id: cad_core::id(),
            name: definition.name.clone(),
            suppressed: false,
            operation: Operation::Sketch3d {
                definition: Box::new(definition),
            },
        }
    };
    let Operation::Sketch3d { definition } = &feature.operation else {
        return;
    };
    let frame = definition.workplane.clone();
    let plane = if frame.u == [0., 1., 0.] && frame.v == [0., 0., 1.] {
        DrawingPlane::YZ
    } else if frame.u == [0., 0., 1.] && frame.v == [1., 0., 0.] {
        DrawingPlane::ZX
    } else {
        DrawingPlane::XY
    };
    let token = cad_core::id();
    w.start_tool();
    if !w.session.visible().parts[&owner]
        .features
        .iter()
        .any(|source| source.id == feature.id)
    {
        w.dispatch(Command::PutFeature {
            definition: owner.clone(),
            feature: Box::new(feature.clone()),
            output: false,
        });
        if !w.session.visible().parts[&owner]
            .features
            .iter()
            .any(|source| source == &feature)
        {
            let error = w.status.clone();
            w.cancel_tool();
            w.status = error;
            return;
        }
    }
    w.fields
        .insert("feature-editor-id".into(), feature.id.clone());
    w.fields
        .insert("feature-editor-generation".into(), token.clone());
    w.fields.remove("feature-source-invalid");
    w.fields.remove("feature-source-error");
    w.json = serde_json::to_string_pretty(&feature).unwrap_or_default();
    w.dialog_title = feature.name.clone();
    w.view.active_node = Some(feature.id.clone());
    w.command_tab = "Sketch".into();
    w.open_editor("feature");
    w.sketch3d = Some(State {
        document: w.session.visible().id.clone(),
        owner,
        feature: feature.id,
        token,
        tool: if editing_existing {
            Tool::Select
        } else {
            Tool::Line
        },
        plane,
        plane_origin: frame.origin,
        occurrence,
        selection: vec![],
        clicks: vec![],
        placement: None,
        hover: None,
        snap_target: None,
        snap: true,
        grid: "1 mm".into(),
        fine: false,
        construction: false,
        auto_axis: true,
        inputs: ["0 mm".into(), "0 mm".into(), "0 mm".into()],
        radius: "10 mm".into(),
        numeric_radius: false,
        degree: 3,
        parameterization: cad_core::sketch_spline::Parameterization::Chord,
        history: vec![],
        redo: vec![],
        drag: None,
        dimension: None,
        popup: None,
        worker: WorkerClient::new(),
        sequence: 0,
        error: None,
    });
    w.status =
        "Draw in the model · Tab changes XY / YZ / ZX · Alt bypasses snap · Finish Sketch confirms"
            .into();
}
pub fn cancel(w: &mut Workspace) {
    if let Some(state) = w.sketch3d.take() {
        state.worker.cancel();
    }
    w.fields.retain(|key, _| !key.starts_with(INVALID));
}
fn history(w: &mut Workspace) {
    if let Some(definition) = source(w)
        && let Some(state) = &mut w.sketch3d
    {
        state.redo.clear();
        state.history.push(definition);
        if state.history.len() > HISTORY_LIMIT {
            state.history.remove(0);
        }
    }
}
fn publish(w: &mut Workspace, definition: Definition) -> Result<()> {
    definition.validate()?;
    let state = w
        .sketch3d
        .as_ref()
        .ok_or_else(|| invalid("The editor has closed"))?;
    let owner = state.owner.clone();
    let mut feature = feature(w).ok_or_else(|| invalid("The feature has closed"))?;
    feature.name = definition.name.clone();
    feature.operation = Operation::Sketch3d {
        definition: Box::new(definition),
    };
    let expected = feature.clone();
    w.dispatch(Command::PutFeature {
        definition: owner.clone(),
        feature: Box::new(feature),
        output: false,
    });
    if !w.session.visible().parts[&owner]
        .features
        .iter()
        .any(|feature| feature == &expected)
    {
        return Err(invalid(&w.status));
    }
    w.json = serde_json::to_string_pretty(&expected)?;
    w.dialog_title = expected.name;
    if let Some(state) = &mut w.sketch3d {
        state.error = None;
    }
    Ok(())
}
pub fn undo(w: &mut Workspace) -> bool {
    history_command(w, false)
}
pub fn history_available(w: &Workspace, redo: bool) -> bool {
    w.sketch3d.as_ref().is_some_and(|state| {
        if redo {
            !state.redo.is_empty()
        } else {
            !state.history.is_empty()
        }
    })
}
pub fn history_command(w: &mut Workspace, redo: bool) -> bool {
    if !active(w) {
        return false;
    }
    if escape_drag(w) {
        return true;
    }
    if cancel_dimension(w) {
        return true;
    }
    if cancel_placement(w) {
        return true;
    }
    let current = source(w);
    let definition = w.sketch3d.as_mut().and_then(|state| {
        if redo {
            state.redo.pop()
        } else {
            state.history.pop()
        }
    });
    if let Some(definition) = definition {
        if let Some(current) = current
            && let Some(state) = &mut w.sketch3d
        {
            if redo {
                state.history.push(current);
            } else {
                state.redo.push(current);
            }
        }
        if let Err(error) = publish(w, definition) {
            w.status = error.to_string();
        }
        if let Some(state) = &mut w.sketch3d {
            state.clicks.clear();
            state.dimension = None;
            state.selection.clear();
        }
    }
    true
}
pub fn ready_to_confirm(w: &mut Workspace) -> bool {
    if !active(w) {
        return true;
    }
    if captured(w) {
        w.status =
            "Waiting for the final spatial drag solve; release the handle or press Escape".into();
        return false;
    }
    if w.sketch3d
        .as_ref()
        .is_some_and(|state| state.dimension.is_some())
    {
        finish_dimension(w);
    }
    if w.session.draft.is_none()
        && current_source_is_valid(w)
        && serde_json::from_str::<Feature>(&w.json).ok() == feature(w)
    {
        let after = w.after_confirm.take();
        w.cancel_tool();
        w.status = "3D sketch closed; no source changes".into();
        if let Some(after) = after {
            crate::ui::feature_preset(w, &after);
        }
        return false;
    }
    if w.sketch3d.as_ref().is_some_and(|state| {
        matches!(
            state.tool,
            Tool::Arc | Tool::Circle | Tool::Bezier | Tool::Nurbs | Tool::InterpolatingSpline
        ) && !state.clicks.is_empty()
    }) && let Err(error) = finish_curve(w)
    {
        w.status = error.to_string();
        return false;
    }
    current_source_is_valid(w)
}
fn points_selected(w: &Workspace) -> Vec<Id> {
    w.sketch3d
        .as_ref()
        .map(|state| {
            state
                .selection
                .iter()
                .filter_map(|entity| match entity {
                    Entity::Point { point } => Some(point.clone()),
                    Entity::Origin => {
                        source(w).map(|definition| format!("origin-{}", definition.id))
                    }
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default()
}
fn members_selected(w: &Workspace) -> Vec<Id> {
    w.sketch3d
        .as_ref()
        .map(|state| {
            state
                .selection
                .iter()
                .filter_map(|entity| match entity {
                    Entity::Member { member } => Some(member.clone()),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default()
}
fn ensure_origin(definition: &mut Definition, w: &Workspace) {
    if w.sketch3d
        .as_ref()
        .is_some_and(|state| state.selection.contains(&Entity::Origin))
    {
        let id = format!("origin-{}", definition.id);
        if !definition.points.contains_key(&id) {
            definition.points.insert(id.clone(), [0.; 3]);
            definition.constraints.push(Constraint {
                id: cad_core::id(),
                driven: false,
                relation: Relation::Fixed {
                    point: id,
                    position: ["0 mm".into(), "0 mm".into(), "0 mm".into()],
                },
            });
        }
    }
}
fn measured_point(definition: &Definition, id: &str) -> Option<[f64; 3]> {
    definition
        .points
        .get(id)
        .copied()
        .or_else(|| (id == format!("origin-{}", definition.id)).then_some([0.; 3]))
}
fn circle_radius(w: &Workspace, definition: &Definition, member: &str) -> Result<f64> {
    circle_radius_with_size(w, definition, member, None)
}
fn circle_radius_with_size(
    w: &Workspace,
    definition: &Definition,
    member: &str,
    radius: Option<&Expr>,
) -> Result<f64> {
    if let Some(evaluation) = current_evaluation(w)
        && let Some(Member {
            curve: Curve::Circle { center, .. },
            ..
        }) = evaluation.solved.members.get(member)
        && let Some(center) = evaluation.solved.points.get(center)
        && let Some(point) = evaluation
            .polylines
            .iter()
            .find(|line| line.member == member)
            .and_then(|line| line.points.first())
    {
        return Ok(DVec3::from_array(*point).distance(DVec3::from_array(*center)));
    }
    if w.sketch3d.as_ref().is_some_and(|state| {
        w.session
            .visible()
            .active_configurations
            .contains_key(&state.owner)
    }) {
        return Err(invalid(
            "Wait for the current configured circle before measuring its radius",
        ));
    }
    let radius = match radius {
        Some(radius) => radius.clone(),
        None => spatial::circle_size_owner(definition, member)?.effective_radius,
    };
    cad_core::expression::value(
        w.session.visible(),
        &w.sketch3d
            .as_ref()
            .ok_or_else(|| invalid("The editor has closed"))?
            .owner,
        &radius,
        Dimension::Length,
    )
}
fn member_length(w: &Workspace, definition: &Definition, member: &str) -> Result<f64> {
    let source = definition
        .members
        .get(member)
        .ok_or_else(|| invalid("The member is missing"))?;
    let point = |id: &str| {
        definition
            .points
            .get(id)
            .copied()
            .ok_or_else(|| invalid("The member point is missing"))
    };
    match &source.curve {
        Curve::Line { start, end } => {
            Ok(DVec3::from_array(point(start)?).distance(DVec3::from_array(point(end)?)))
        }
        Curve::Arc {
            start,
            through,
            end,
        } => {
            let frame = spatial::arc_frame(point(start)?, point(through)?, point(end)?)?;
            Ok(frame.radius_mm * frame.sweep_angle_rad.abs())
        }
        Curve::Circle { .. } => Ok(std::f64::consts::TAU * circle_radius(w, definition, member)?),
        _ => Err(CadError::Unsupported(
            "3D sketch: Driving spline arc lengths need a supported exact native length constraint"
                .into(),
        )),
    }
}
fn line_direction(definition: &Definition, member: &str) -> Result<DVec3> {
    let Some(Member {
        curve: Curve::Line { start, end },
        ..
    }) = definition.members.get(member)
    else {
        return Err(invalid("Select a straight line"));
    };
    let a = definition
        .points
        .get(start)
        .ok_or_else(|| invalid("Line start is missing"))?;
    let b = definition
        .points
        .get(end)
        .ok_or_else(|| invalid("Line end is missing"))?;
    (DVec3::from_array(*b) - DVec3::from_array(*a))
        .try_normalize()
        .ok_or_else(|| invalid("The line is collapsed"))
}
fn dimension_choices(w: &Workspace) -> Vec<(String, String, Relation)> {
    if w.sketch3d.as_ref().is_some_and(|state| {
        w.session
            .visible()
            .active_configurations
            .contains_key(&state.owner)
    }) && current_evaluation(w).is_none()
    {
        return vec![];
    }
    let Some(definition) = displayed(w) else {
        return vec![];
    };
    let points = points_selected(w);
    let members = members_selected(w);
    let mut out = vec![];
    if points.len() == 2 && members.is_empty() {
        if let (Some(a), Some(b)) = (
            measured_point(&definition, &points[0]),
            measured_point(&definition, &points[1]),
        ) {
            out.push((
                "distance".into(),
                "3D distance".into(),
                Relation::Distance {
                    first: points[0].clone(),
                    second: points[1].clone(),
                    value: coordinate(DVec3::from_array(a).distance(DVec3::from_array(b))),
                },
            ));
            for index in 0..3 {
                out.push((
                    format!("delta-{index}"),
                    format!("{} distance (signed)", axis_name(axis(index))),
                    Relation::AxisDistance {
                        first: points[0].clone(),
                        second: points[1].clone(),
                        axis: axis(index),
                        value: coordinate(b[index] - a[index]),
                    },
                ));
            }
        }
    } else if points.len() == 1 && members.is_empty() {
        if let Some(point) = measured_point(&definition, &points[0]) {
            for (index, coordinate_value) in point.into_iter().enumerate() {
                out.push((
                    format!("coordinate-{index}"),
                    format!("{} from origin", axis_name(axis(index))),
                    Relation::Coordinate {
                        point: points[0].clone(),
                        axis: axis(index),
                        value: coordinate(coordinate_value),
                    },
                ));
            }
        }
    } else if members.len() == 1 && points.is_empty() {
        let member = &members[0];
        if let Ok(length) = member_length(w, &definition, member) {
            out.push((
                "length".into(),
                "Length".into(),
                Relation::Length {
                    member: member.clone(),
                    value: coordinate(length),
                },
            ));
        }
        let radius = match definition.members.get(member).map(|member| &member.curve) {
            Some(Curve::Circle { .. }) => circle_radius(w, &definition, member).ok(),
            Some(Curve::Arc {
                start,
                through,
                end,
            }) => definition
                .points
                .get(start)
                .zip(definition.points.get(through))
                .zip(definition.points.get(end))
                .and_then(|((a, b), c)| {
                    spatial::arc_frame(*a, *b, *c)
                        .ok()
                        .map(|frame| frame.radius_mm)
                }),
            _ => None,
        };
        if let Some(radius) = radius {
            out.insert(
                0,
                (
                    "radius".into(),
                    "Radius".into(),
                    Relation::Radius {
                        member: member.clone(),
                        value: coordinate(radius),
                    },
                ),
            );
        }
        if let Ok(direction) = line_direction(&definition, member) {
            for index in 0..3 {
                let angle = direction
                    .dot(DVec3::from_array(axis(index).direction()))
                    .clamp(-1., 1.)
                    .acos();
                out.push((
                    format!("angle-{index}"),
                    format!("Angle to {} axis", axis_name(axis(index))),
                    Relation::AxisAngle {
                        member: member.clone(),
                        axis: axis(index),
                        value: Expr(format!("{} deg", angle.to_degrees())),
                    },
                ));
            }
        }
    } else if points.len() == 1 && members.len() == 1 {
        if let (
            Some(point),
            Some(Member {
                curve: Curve::Line { start, .. },
                ..
            }),
        ) = (
            measured_point(&definition, &points[0]),
            definition.members.get(&members[0]),
        ) && let (Some(start), Ok(direction)) = (
            definition.points.get(start),
            line_direction(&definition, &members[0]),
        ) {
            let delta = DVec3::from_array(point) - DVec3::from_array(*start);
            let distance = (delta - direction * delta.dot(direction)).length();
            out.push((
                "point-line".into(),
                "Shortest distance to supporting line".into(),
                Relation::PointLineDistance {
                    point: points[0].clone(),
                    line: members[0].clone(),
                    value: coordinate(distance),
                },
            ));
        }
    } else if members.len() == 2
        && points.is_empty()
        && let (Ok(a), Ok(b)) = (
            line_direction(&definition, &members[0]),
            line_direction(&definition, &members[1]),
        )
    {
        out.push((
            "angle".into(),
            "Angle between lines".into(),
            Relation::Angle {
                first: members[0].clone(),
                second: members[1].clone(),
                value: Expr(format!(
                    "{} deg",
                    a.dot(b).clamp(-1., 1.).acos().to_degrees()
                )),
            },
        ));
    }
    out
}
fn update_dimension(w: &mut Workspace, kind: Option<&str>) -> Result<()> {
    let choices = dimension_choices(w);
    let Some(choice) = kind
        .and_then(|kind| choices.iter().find(|choice| choice.0 == kind))
        .or_else(|| choices.first())
    else {
        return Err(invalid(
            "Select a point, a line/circle/arc, two points, two lines, or a point and line",
        ));
    };
    let mut definition = source(w).ok_or_else(|| invalid("The editor has closed"))?;
    ensure_origin(&mut definition, w);
    if w.sketch3d
        .as_ref()
        .is_some_and(|state| state.dimension.is_none())
    {
        let checkpoint = w.session.clone();
        let history_len = w.sketch3d.as_ref().map_or(0, |state| state.history.len());
        history(w);
        if let Some(state) = &mut w.sketch3d {
            state.dimension = Some(DimensionEdit {
                constraint: cad_core::id(),
                checkpoint,
                fields: w.fields.clone(),
                history_len,
                kind: choice.0.clone(),
            });
        }
    }
    let id = w
        .sketch3d
        .as_ref()
        .and_then(|state| state.dimension.as_ref())
        .ok_or_else(|| invalid("Start Smart Dimension first"))?
        .constraint
        .clone();
    if let Some(constraint) = definition
        .constraints
        .iter_mut()
        .find(|constraint| constraint.id == id)
    {
        constraint.relation = choice.2.clone();
    } else {
        definition.constraints.push(Constraint {
            id,
            relation: choice.2.clone(),
            driven: false,
        });
    }
    publish(w, definition)?;
    if let Some(state) = &mut w.sketch3d {
        if let Some(dimension) = &mut state.dimension {
            dimension.kind = choice.0.clone();
        }
        state.tool = Tool::Dimension;
    }
    Ok(())
}
fn finish_dimension(w: &mut Workspace) {
    if let Some(state) = &mut w.sketch3d {
        state.dimension = None;
        state.tool = Tool::Select;
    }
    w.status =
        "Dimension kept in the sketch draft · Finish Sketch confirms the complete sketch".into();
}
fn cancel_dimension(w: &mut Workspace) -> bool {
    let Some(dimension) = w.sketch3d.as_mut().and_then(|state| state.dimension.take()) else {
        return false;
    };
    w.worker.cancel();
    w.requested = None;
    w.session = dimension.checkpoint;
    w.fields = dimension.fields;
    w.changed = Instant::now();
    w.rendered_key.clear();
    w.confirm_requested = false;
    if let Some(state) = &mut w.sketch3d {
        state.history.truncate(dimension.history_len);
        state.tool = Tool::Select;
    }
    if let Some(feature) = feature(w) {
        w.json = serde_json::to_string_pretty(&feature).unwrap_or_default();
    }
    w.status = "Dimension canceled; earlier sketch edits retained".into();
    true
}
fn cancel_placement(w: &mut Workspace) -> bool {
    let Some(placement) = w.sketch3d.as_mut().and_then(|state| state.placement.take()) else {
        return false;
    };
    w.worker.cancel();
    w.requested = None;
    w.session = placement.checkpoint;
    w.fields = placement.fields;
    w.changed = Instant::now();
    w.rendered_key.clear();
    w.confirm_requested = false;
    if let Some(state) = &mut w.sketch3d {
        state.history.truncate(placement.history_len);
        state.redo = placement.redo;
        state.clicks.clear();
        state.tool = Tool::Select;
        state.popup = None;
    }
    if let Some(feature) = feature(w) {
        w.json = serde_json::to_string_pretty(&feature).unwrap_or_default();
    }
    w.status = "Unfinished member canceled; completed sketch geometry retained".into();
    true
}
fn relation_for(w: &Workspace, kind: &str) -> Result<Vec<Relation>> {
    let points = points_selected(w);
    let members = members_selected(w);
    let definition = displayed(w).ok_or_else(|| invalid("The sketch is unavailable"))?;
    if points
        .iter()
        .any(|point| measured_point(&definition, point).is_none())
        || members
            .iter()
            .any(|member| !definition.members.contains_key(member))
    {
        return Err(invalid("The selection contains removed geometry"));
    }
    match kind {
        "fixed" if !points.is_empty() || !members.is_empty() => {
            let mut controls = points;
            for member in &members {
                for point in definition.members[member].curve.points() {
                    if !controls.contains(point) {
                        controls.push(point.clone());
                    }
                }
            }
            controls
                .into_iter()
                .filter(|point| definition.points.contains_key(point))
                .map(|point| {
                    Ok(Relation::Fixed {
                        position: measured_point(&definition, &point)
                            .ok_or_else(|| invalid("A selected control point is missing"))?
                            .map(coordinate),
                        point,
                    })
                })
                .collect()
        }
        "coincident" if points.len() >= 2 && members.is_empty() => Ok(points[1..]
            .iter()
            .map(|point| Relation::Coincident {
                first: points[0].clone(),
                second: point.clone(),
            })
            .collect()),
        "along-x" | "along-y" | "along-z"
            if points.is_empty()
                && !members.is_empty()
                && members
                    .iter()
                    .all(|member| line_direction(&definition, member).is_ok()) =>
        {
            Ok(members
                .into_iter()
                .map(|member| Relation::AlignAxis {
                    member,
                    axis: match kind {
                        "along-x" => Axis::X,
                        "along-y" => Axis::Y,
                        _ => Axis::Z,
                    },
                })
                .collect())
        }
        "parallel" | "perpendicular"
            if points.is_empty()
                && members.len() >= 2
                && members
                    .iter()
                    .all(|member| line_direction(&definition, member).is_ok()) =>
        {
            Ok(members[1..]
                .iter()
                .map(|member| {
                    if kind == "parallel" {
                        Relation::Parallel {
                            first: members[0].clone(),
                            second: member.clone(),
                        }
                    } else {
                        Relation::Perpendicular {
                            first: members[0].clone(),
                            second: member.clone(),
                        }
                    }
                })
                .collect())
        }
        "equal-length"
            if points.is_empty()
                && members.len() >= 2
                && members.iter().all(|member| {
                    matches!(
                        definition.members[member].curve,
                        Curve::Line { .. } | Curve::Arc { .. }
                    ) && member_length(w, &definition, member).is_ok()
                }) =>
        {
            // Circle radius expressions are fixed engineering inputs, rather
            // than point solve variables. Do not offer an equality that would
            // merely move their centers while leaving the radii unchanged.
            Ok(members[1..]
                .iter()
                .map(|member| Relation::EqualLength {
                    first: members[0].clone(),
                    second: member.clone(),
                })
                .collect())
        }
        "equal-radius"
            if points.is_empty()
                && members.len() >= 2
                && members.iter().all(|member| {
                    matches!(definition.members[member].curve, Curve::Circle { .. })
                }) =>
        {
            // Existing groups retain their authored reference. Join roots so
            // a dependent selection does not create competing size ancestry.
            let owners = spatial::circle_size_owners(&definition)?;
            for member in &members {
                circle_radius_with_size(
                    w,
                    &definition,
                    member,
                    Some(&owners[member].effective_radius),
                )?;
            }
            let reference = owners[&members[0]].reference.clone();
            let mut seen = std::collections::BTreeSet::from([reference.clone()]);
            let mut relations = Vec::new();
            for member in &members[1..] {
                let target = owners[member].reference.clone();
                if seen.insert(target.clone()) {
                    relations.push(Relation::EqualLength {
                        first: reference.clone(),
                        second: target,
                    });
                }
            }
            Ok(relations)
        }
        "on-plane" if !points.is_empty() && members.is_empty() => {
            let plane = w
                .sketch3d
                .as_ref()
                .map(|state| state.plane.frame(state.plane_origin))
                .unwrap_or_default();
            Ok(points
                .into_iter()
                .map(|point| Relation::OnPlane {
                    point,
                    plane: plane.clone(),
                })
                .collect())
        }
        "on-line" | "coincident"
            if !points.is_empty()
                && members.len() == 1
                && line_direction(&definition, &members[0]).is_ok() =>
        {
            Ok(points
                .into_iter()
                .map(|point| Relation::OnLine {
                    point,
                    line: members[0].clone(),
                    bounded: false,
                })
                .collect())
        }
        "on-curve" | "coincident" if !points.is_empty() && members.len() == 1 => Ok(points
            .into_iter()
            .map(|point| Relation::PointOnCurve {
                point,
                member: members[0].clone(),
                parameter: "0.5".into(),
                free: true,
            })
            .collect()),
        "tangent" if points.is_empty() && members.len() == 2 => Ok(vec![Relation::Tangent {
            first: members[0].clone(),
            second: members[1].clone(),
            first_parameter: "1".into(),
            second_parameter: "0".into(),
        }]),
        _ => Err(invalid(
            "All selected spatial geometry must be compatible with this relation",
        )),
    }
}
fn equivalent_relation(first: &Relation, second: &Relation) -> bool {
    if first == second {
        return true;
    }
    match (first, second) {
        (Relation::Fixed { point: a, .. }, Relation::Fixed { point: b, .. }) => a == b,
        (
            Relation::Coincident {
                first: a,
                second: b,
            },
            Relation::Coincident {
                first: c,
                second: d,
            },
        )
        | (
            Relation::Parallel {
                first: a,
                second: b,
            },
            Relation::Parallel {
                first: c,
                second: d,
            },
        )
        | (
            Relation::Perpendicular {
                first: a,
                second: b,
            },
            Relation::Perpendicular {
                first: c,
                second: d,
            },
        )
        | (
            Relation::EqualLength {
                first: a,
                second: b,
            },
            Relation::EqualLength {
                first: c,
                second: d,
            },
        ) => a == d && b == c,
        (
            Relation::PointOnCurve {
                point: a,
                member: b,
                free: true,
                ..
            },
            Relation::PointOnCurve {
                point: c,
                member: d,
                free: true,
                ..
            },
        ) => a == c && b == d,
        (
            Relation::Tangent {
                first: a,
                second: b,
                first_parameter: at,
                second_parameter: bt,
            },
            Relation::Tangent {
                first: c,
                second: d,
                first_parameter: ct,
                second_parameter: dt,
            },
        ) => a == d && b == c && at == dt && bt == ct,
        _ => false,
    }
}
fn equivalence_pair(relation: &Relation) -> Option<(u8, &Id, &Id)> {
    match relation {
        Relation::Coincident { first, second } => Some((0, first, second)),
        Relation::Parallel { first, second } => Some((1, first, second)),
        Relation::EqualLength { first, second } => Some((2, first, second)),
        _ => None,
    }
}
fn has_relation(definition: &Definition, relation: &Relation) -> bool {
    if definition.constraints.iter().any(|constraint| {
        !constraint.driven
            && (equivalent_relation(&constraint.relation, relation)
                || matches!(
                    (&constraint.relation, relation),
                    (
                        Relation::Coplanar { points, plane: a },
                        Relation::OnPlane { point, plane: b },
                    ) if a == b && points.contains(point)
                ))
    }) {
        return true;
    }
    // Equivalence relations already imply their transitive closure. Keeping a
    // connected set avoids redundant solver rows when a batch joins groups.
    let Some((kind, first, second)) = equivalence_pair(relation) else {
        return false;
    };
    let mut connected = std::collections::BTreeSet::from([first]);
    loop {
        let previous = connected.len();
        for constraint in definition
            .constraints
            .iter()
            .filter(|constraint| !constraint.driven)
        {
            if let Some((other_kind, a, b)) = equivalence_pair(&constraint.relation)
                && other_kind == kind
            {
                if connected.contains(a) {
                    connected.insert(b);
                }
                if connected.contains(b) {
                    connected.insert(a);
                }
            }
        }
        if connected.contains(second) {
            return true;
        }
        if previous == connected.len() {
            return false;
        }
    }
}
fn add_relations(w: &mut Workspace, kind: &str) -> Result<()> {
    if !current_source_is_valid(w) {
        return Err(invalid(
            "Resolve the current field error before adding relations",
        ));
    }
    let relations = relation_for(w, kind)?;
    let original = source(w).ok_or_else(|| invalid("The sketch is missing"))?;
    let mut definition = original.clone();
    if relations
        .iter()
        .all(|relation| has_relation(&definition, relation))
    {
        w.status = if kind == "equal-radius" {
            "The selected circles are already equal; their existing size reference is retained"
        } else {
            "The selected geometry already has these relations"
        }
        .into();
        return Ok(());
    }
    ensure_origin(&mut definition, w);
    let mut added = 0;
    for relation in relations {
        if !has_relation(&definition, &relation) {
            definition.constraints.push(Constraint {
                id: cad_core::id(),
                relation,
                driven: false,
            });
            added += 1;
        }
    }
    definition.validate()?;
    // A batch is one sketch history entry and one shared draft publication.
    // Take the checkpoint only after source validation, and leave no no-op.
    if definition != original {
        publish(w, definition)?;
        if let Some(state) = &mut w.sketch3d {
            state.redo.clear();
            state.history.push(original);
            if state.history.len() > HISTORY_LIMIT {
                state.history.remove(0);
            }
        }
        w.status = format!(
            "Added {added} spatial relation{}; preview updating",
            if added == 1 { "" } else { "s" }
        );
    }
    Ok(())
}
fn error_key(element: &SemanticElement) -> String {
    format!("{INVALID}{}:{}", element.id, element.data["editor"])
}
pub fn field_is_invalid(w: &Workspace, element: &SemanticElement) -> bool {
    element.data["setter"] == "sketch3d" && w.fields.contains_key(&error_key(element))
}
pub fn field_error(w: &mut Workspace, element: &SemanticElement, error: Option<String>) {
    if element.data["setter"] != "sketch3d" {
        return;
    }
    if let Some(error) = error {
        w.fields.insert(error_key(element), error);
        w.confirm_requested = false;
    } else {
        w.fields.remove(&error_key(element));
    }
}
fn live(w: &Workspace, element: &SemanticElement) -> Result<SemanticElement> {
    let state = w
        .sketch3d
        .as_ref()
        .filter(|_| active(w))
        .ok_or_else(|| invalid("The editor has closed"))?;
    if element.data["document"] != state.document
        || element.data["owner"] != state.owner
        || element.data["feature"] != state.feature
        || element.data["editor"] != state.token
    {
        return Err(invalid("This control belongs to a different sketch editor"));
    }
    semantics(w)
        .into_iter()
        .find(|fresh| fresh.id == element.id)
        .ok_or_else(|| invalid("The selected spatial field no longer exists"))
}
fn matching_scope(w: &Workspace, element: &SemanticElement) -> bool {
    w.sketch3d.as_ref().is_some_and(|state| {
        element.data["document"] == state.document
            && element.data["owner"] == state.owner
            && element.data["feature"] == state.feature
            && element.data["editor"] == state.token
            && active(w)
    })
}
pub fn current_value(w: &Workspace, element: &SemanticElement) -> Option<Value> {
    if !matching_scope(w, element) {
        return None;
    }
    if element.data.get("configured_expression").is_some() {
        return element.data.get("json").cloned();
    }
    let state = w.sketch3d.as_ref()?;
    let definition = source_ref(w)?;
    let key = element.data["field"].as_str()?;
    let value = match key {
        "tool" => json!(state.tool),
        "name" => json!(definition.name),
        "plane" => json!(state.plane),
        "reference-frame" => json!(definition.workplane),
        "depth" => json!(coordinate(state.plane_origin[state.plane.normal()])),
        "snap" => json!(state.snap),
        "fine" => json!(state.fine),
        "auto-axis" => json!(state.auto_axis),
        "construction" => json!(state.construction),
        "grid" => json!(state.grid),
        "radius" => json!(state.radius),
        "degree" => json!(state.degree),
        "parameterization" => json!(state.parameterization),
        "pick-filter" => json!(crate::sketch_overlay::pick_filter(w)),
        "dimension-kind" => json!(state.dimension.as_ref()?.kind),
        _ => {
            let parts = key.split('/').collect::<Vec<_>>();
            match parts.as_slice() {
                ["input", index] => json!(state.inputs.get(index.parse::<usize>().ok()?)?),
                ["point", point, index] => {
                    let index = index.parse::<usize>().ok().filter(|index| *index < 3)?;
                    if let Some(constraint) = coordinate_constraint(definition, point, axis(index))
                    {
                        json!(constraint_value(&constraint.relation)?.0)
                    } else {
                        let position = current_evaluation(w)
                            .and_then(|evaluation| evaluation.solved.points.get(*point))
                            .or_else(|| definition.points.get(*point))?;
                        json!(coordinate(position[index]))
                    }
                }
                ["member", member, "construction"] => {
                    json!(definition.members.get(*member)?.construction)
                }
                ["member", member, "radius"] if element.data["measured_radius"] == true => {
                    json!(coordinate(circle_radius(w, definition, member).ok()?))
                }
                ["member", _, "radius"] if element.data["inherited_radius"] == true => {
                    element.data["json"].clone()
                }
                ["member", member, "radius"] => json!(radius_expression(definition, member)?),
                ["member", member, "weight", index] => {
                    let Curve::Nurbs { weights, .. } = &definition.members.get(*member)?.curve
                    else {
                        return None;
                    };
                    json!(weights.get(index.parse::<usize>().ok()?)?)
                }
                ["constraint", id, field] => {
                    let constraint = definition
                        .constraints
                        .iter()
                        .find(|constraint| constraint.id == *id)?;
                    match *field {
                        "driven" => json!(constraint.driven),
                        "value" if constraint.driven => json!(measured_constraint(w, constraint)?),
                        "value" => json!(constraint_value(&constraint.relation)?.0),
                        "free" => {
                            let Relation::PointOnCurve { free, .. } = constraint.relation else {
                                return None;
                            };
                            json!(free)
                        }
                        _ => return None,
                    }
                }
                ["constraint", id, "fixed", index] => {
                    let constraint = definition
                        .constraints
                        .iter()
                        .find(|constraint| constraint.id == *id)?;
                    let Relation::Fixed { position, .. } = &constraint.relation else {
                        return None;
                    };
                    json!(position.get(index.parse::<usize>().ok()?)?)
                }
                ["constraint", id, "tangent", index] => {
                    let constraint = definition
                        .constraints
                        .iter()
                        .find(|constraint| constraint.id == *id)?;
                    let Relation::Tangent {
                        first_parameter,
                        second_parameter,
                        ..
                    } = &constraint.relation
                    else {
                        return None;
                    };
                    json!(match *index {
                        "0" => first_parameter,
                        "1" => second_parameter,
                        _ => return None,
                    })
                }
                _ => return None,
            }
        }
    };
    Some(value)
}
pub fn set_value(w: &mut Workspace, element: &SemanticElement, value: Value) -> Result<()> {
    let result = set_field(w, element, value);
    field_error(w, element, result.as_ref().err().map(ToString::to_string));
    result
}
fn set_field(w: &mut Workspace, element: &SemanticElement, value: Value) -> Result<()> {
    let element = live(w, element)?;
    if element.data["editable"] == false {
        return Err(invalid(
            element.data["readonly_reason"]
                .as_str()
                .unwrap_or("This is a measured reference field"),
        ));
    }
    if captured(w) {
        return Err(invalid(
            "Release or cancel the active handle before editing another field",
        ));
    }
    if w.fields.contains_key("feature-source-invalid") {
        return Err(invalid("Correct Advanced Source first"));
    }
    if element.data["json"] == value {
        return Ok(());
    }
    let choices = element.value_descriptor().choices;
    if !choices.is_empty()
        && !choices
            .iter()
            .any(|choice| choice.get("value").unwrap_or(choice) == &value)
    {
        return Err(invalid("Choose a current spatial option"));
    }
    let key = element.data["field"]
        .as_str()
        .ok_or_else(|| invalid("The field binding is missing"))?;
    let expression = || {
        value
            .as_str()
            .filter(|value| !value.trim().is_empty())
            .map(|value| Expr(value.into()))
            .ok_or_else(|| invalid("Enter a quantity or parameter expression"))
    };
    match key {
        "pick-filter" => {
            let filter = value
                .as_str()
                .filter(|filter| matches!(*filter, "all" | "points" | "members" | "construction"))
                .ok_or_else(|| invalid("Choose a sketch selection filter"))?;
            w.fields.insert("sketch-pick-filter".into(), filter.into());
            return Ok(());
        }
        "tool" => {
            let tool = serde_json::from_value(value)?;
            cancel_placement(w);
            if let Some(state) = &mut w.sketch3d {
                state.tool = tool;
                state.clicks.clear();
                state.popup = None;
            }
            return Ok(());
        }
        "plane" => {
            let plane = serde_json::from_value(value)?;
            change_plane(w, plane)?;
            return Ok(());
        }
        "depth" => {
            let expression = expression()?;
            let owner = w
                .sketch3d
                .as_ref()
                .ok_or_else(|| invalid("The editor closed"))?
                .owner
                .clone();
            let depth = cad_core::expression::value(
                w.session.visible(),
                &owner,
                &expression,
                Dimension::Length,
            )?;
            if let Some(state) = &mut w.sketch3d {
                state.plane_origin[state.plane.normal()] = depth;
            }
            return Ok(());
        }
        "snap" | "fine" | "auto-axis" | "construction" => {
            let boolean = value
                .as_bool()
                .ok_or_else(|| invalid("Expected a toggle"))?;
            if let Some(state) = &mut w.sketch3d {
                match key {
                    "snap" => state.snap = boolean,
                    "fine" => state.fine = boolean,
                    "auto-axis" => state.auto_axis = boolean,
                    _ => state.construction = boolean,
                }
            }
            return Ok(());
        }
        "grid" | "radius" => {
            let expression = expression()?;
            if let Some(state) = &mut w.sketch3d {
                if key == "grid" {
                    state.grid = expression;
                } else {
                    state.radius = expression;
                    state.numeric_radius = true;
                }
            }
            return Ok(());
        }
        "degree" => {
            let degree = value
                .as_u64()
                .and_then(|value| usize::try_from(value).ok())
                .filter(|value| (1..=15).contains(value))
                .ok_or_else(|| invalid("Choose degree 1–15"))?;
            if let Some(state) = &mut w.sketch3d {
                state.degree = degree;
            }
            return Ok(());
        }
        "parameterization" => {
            let parameterization = serde_json::from_value(value)?;
            if let Some(state) = &mut w.sketch3d {
                state.parameterization = parameterization;
            }
            return Ok(());
        }
        _ if key.starts_with("input/") => {
            let index = key
                .trim_start_matches("input/")
                .parse::<usize>()
                .ok()
                .filter(|index| *index < 3)
                .ok_or_else(|| invalid("Invalid coordinate field"))?;
            let expression = expression()?;
            if let Some(state) = &mut w.sketch3d {
                state.inputs[index] = expression;
            }
            return Ok(());
        }
        "dimension-kind" => {
            return update_dimension(w, value.as_str());
        }
        _ => {}
    }
    let mut definition = source(w).ok_or_else(|| invalid("The sketch is missing"))?;
    history(w);
    let parts = key.split('/').collect::<Vec<_>>();
    match parts.as_slice() {
        ["name"] => {
            definition.name = value
                .as_str()
                .filter(|name| !name.trim().is_empty() && name.len() <= 512)
                .ok_or_else(|| invalid("Enter a sketch name"))?
                .into()
        }
        ["reference-frame"] => definition.workplane = serde_json::from_value(value)?,
        ["point", point, index] => {
            let index = index
                .parse::<usize>()
                .ok()
                .filter(|index| *index < 3)
                .ok_or_else(|| invalid("Invalid point axis"))?;
            if !definition.points.contains_key(*point) {
                return Err(invalid("The point was removed"));
            }
            let value = expression()?;
            let existing = coordinate_constraint(&definition, point, axis(index))
                .map(|constraint| constraint.id.clone());
            if let Some(id) = existing
                && let Some(constraint) = definition
                    .constraints
                    .iter_mut()
                    .find(|constraint| constraint.id == id)
            {
                constraint.relation = Relation::Coordinate {
                    point: (*point).into(),
                    axis: axis(index),
                    value,
                };
            } else {
                definition.constraints.push(Constraint {
                    id: cad_core::id(),
                    driven: false,
                    relation: Relation::Coordinate {
                        point: (*point).into(),
                        axis: axis(index),
                        value,
                    },
                });
            }
        }
        ["member", member, "construction"] => {
            definition
                .members
                .get_mut(*member)
                .ok_or_else(|| invalid("The member was removed"))?
                .construction = value
                .as_bool()
                .ok_or_else(|| invalid("Expected a construction toggle"))?
        }
        ["member", member, "radius"] => {
            let value = expression()?;
            if let Some(constraint)=definition.constraints.iter_mut().find(|constraint|!constraint.driven&&matches!(&constraint.relation,Relation::Radius{member:target,..}if target==member)) {constraint.relation=Relation::Radius{member:(*member).into(),value};}
            else if let Some(Member{curve:Curve::Circle{radius,..},..})=definition.members.get_mut(*member){*radius=value;}else{return Err(invalid("Select a circle"));}
        }
        ["member", member, "weight", index] => {
            let index = index
                .parse::<usize>()
                .map_err(|_| invalid("Invalid NURBS control"))?;
            let Some(Member {
                curve: Curve::Nurbs { weights, .. },
                ..
            }) = definition.members.get_mut(*member)
            else {
                return Err(invalid("Select a NURBS member"));
            };
            *weights
                .get_mut(index)
                .ok_or_else(|| invalid("The NURBS control was removed"))? = expression()?;
        }
        ["constraint", constraint, "driven"] => {
            definition
                .constraints
                .iter_mut()
                .find(|target| target.id == *constraint)
                .ok_or_else(|| invalid("The relation was removed"))?
                .driven = value
                .as_bool()
                .ok_or_else(|| invalid("Expected a reference toggle"))?
        }
        ["constraint", constraint, "free"] => {
            let target = &mut definition
                .constraints
                .iter_mut()
                .find(|target| target.id == *constraint)
                .ok_or_else(|| invalid("The relation was removed"))?
                .relation;
            if let Relation::PointOnCurve { free, .. } = target {
                *free = value
                    .as_bool()
                    .ok_or_else(|| invalid("Expected a free parameter toggle"))?;
            } else {
                return Err(invalid("Select a point-on-curve relation"));
            }
        }
        ["constraint", constraint, "fixed", index] => {
            let index = index
                .parse::<usize>()
                .ok()
                .filter(|index| *index < 3)
                .ok_or_else(|| invalid("Invalid fixed point axis"))?;
            let target = &mut definition
                .constraints
                .iter_mut()
                .find(|target| target.id == *constraint)
                .ok_or_else(|| invalid("The relation was removed"))?
                .relation;
            if let Relation::Fixed { position, .. } = target {
                position[index] = expression()?;
            } else {
                return Err(invalid("Select a fixed point relation"));
            }
        }
        ["constraint", constraint, "tangent", index] => {
            let target = &mut definition
                .constraints
                .iter_mut()
                .find(|target| target.id == *constraint)
                .ok_or_else(|| invalid("The relation was removed"))?
                .relation;
            if let Relation::Tangent {
                first_parameter,
                second_parameter,
                ..
            } = target
            {
                if *index == "0" {
                    *first_parameter = expression()?;
                } else if *index == "1" {
                    *second_parameter = expression()?;
                } else {
                    return Err(invalid("Invalid tangent parameter"));
                }
            } else {
                return Err(invalid("Select a tangent relation"));
            }
        }
        ["constraint", constraint, "value"] => {
            let value = expression()?;
            let target = &mut definition
                .constraints
                .iter_mut()
                .find(|target| target.id == *constraint)
                .ok_or_else(|| invalid("The relation was removed"))?
                .relation;
            match target {
                Relation::Coordinate { value: target, .. }
                | Relation::Distance { value: target, .. }
                | Relation::AxisDistance { value: target, .. }
                | Relation::PointLineDistance { value: target, .. }
                | Relation::Length { value: target, .. }
                | Relation::Radius { value: target, .. }
                | Relation::Angle { value: target, .. }
                | Relation::AxisAngle { value: target, .. }
                | Relation::PointOnCurve {
                    parameter: target, ..
                } => *target = value,
                _ => return Err(invalid("This relation has no scalar value")),
            }
        }
        _ => return Err(invalid("Unsupported spatial field")),
    }
    publish(w, definition)
}
fn change_plane(w: &mut Workspace, plane: DrawingPlane) -> Result<()> {
    let displayed = displayed(w).ok_or_else(|| invalid("The sketch is unavailable"))?;
    if let Some(state) = &mut w.sketch3d {
        if let Some(point) = state
            .clicks
            .last()
            .and_then(|id| displayed.points.get(id))
            .copied()
        {
            state.plane_origin = point;
        }
        state.plane = plane;
    }
    Ok(())
}
fn insert_point(definition: &mut Definition, position: [f64; 3], target: Option<&Entity>) -> Id {
    if let Some(Entity::Point { point }) = target
        && definition.points.contains_key(point)
    {
        return point.clone();
    }
    if matches!(target, Some(Entity::Origin)) {
        let id = format!("origin-{}", definition.id);
        if !definition.points.contains_key(&id) {
            definition.points.insert(id.clone(), [0.; 3]);
            definition.constraints.push(Constraint {
                id: cad_core::id(),
                driven: false,
                relation: Relation::Fixed {
                    point: id.clone(),
                    position: ["0 mm".into(), "0 mm".into(), "0 mm".into()],
                },
            });
        }
        return id;
    }
    let id = cad_core::id();
    definition.points.insert(id.clone(), position);
    id
}
fn place(
    w: &mut Workspace,
    position: [f64; 3],
    target: Option<Entity>,
    numeric: bool,
    bypass: bool,
) -> Result<()> {
    let mut definition = source(w).ok_or_else(|| invalid("Open a 3D sketch first"))?;
    let state = w
        .sketch3d
        .as_ref()
        .ok_or_else(|| invalid("The editor has closed"))?;
    let (tool, construction, previous, numeric_radius, radius, plane, auto_axis, inputs) = (
        state.tool,
        state.construction,
        state.clicks.clone(),
        state.numeric_radius,
        state.radius.clone(),
        state.plane.frame(state.plane_origin),
        state.auto_axis,
        state.inputs.clone(),
    );
    if matches!(tool, Tool::Select | Tool::Dimension) {
        return Err(invalid("Choose a drawing tool first"));
    }
    if previous.is_empty() && tool != Tool::Point {
        let placement = Placement {
            checkpoint: w.session.clone(),
            fields: w.fields.clone(),
            history_len: state.history.len(),
            redo: state.redo.clone(),
        };
        if let Some(state) = &mut w.sketch3d {
            state.placement = Some(placement);
        }
    }
    history(w);
    if tool == Tool::Circle && !previous.is_empty() {
        let center = previous[0].clone();
        let center_point = *definition
            .points
            .get(&center)
            .ok_or_else(|| invalid("The circle center was removed"))?;
        let radius = if numeric_radius {
            radius
        } else {
            coordinate(DVec3::from_array(position).distance(DVec3::from_array(center_point)))
        };
        definition.members.insert(
            cad_core::id(),
            Member {
                construction,
                curve: Curve::Circle {
                    center,
                    plane,
                    radius,
                },
            },
        );
        publish(w, definition)?;
        if let Some(state) = &mut w.sketch3d {
            state.clicks.clear();
            state.placement = None;
        }
        return Ok(());
    }
    let point = insert_point(&mut definition, position, target.as_ref());
    if numeric {
        for (index, input) in inputs.iter().enumerate() {
            if !definition.constraints.iter().any(|constraint|matches!(&constraint.relation,Relation::Coordinate{point:target,axis:direction,..}if target==&point&&*direction==axis(index))) {
        definition.constraints.push(Constraint{id:cad_core::id(),driven:false,relation:Relation::Coordinate{point:point.clone(),axis:axis(index),value:input.clone()}});
    }
        }
    }
    let mut clicks = previous.clone();
    clicks.push(point.clone());
    let mut member = None;
    match tool {
        Tool::Line if clicks.len() == 2 => {
            if clicks[0] == clicks[1] {
                return Err(invalid("A line needs two different points"));
            }
            member = Some(Curve::Line {
                start: clicks[0].clone(),
                end: clicks[1].clone(),
            });
            clicks = vec![point.clone()];
        }
        Tool::Arc if clicks.len() == 3 => {
            member = Some(Curve::Arc {
                start: clicks[0].clone(),
                through: clicks[1].clone(),
                end: clicks[2].clone(),
            });
            clicks.clear();
        }
        Tool::Point => clicks.clear(),
        _ => {}
    }
    let completed = member.is_some() || tool == Tool::Point;
    if let Some(curve) = member {
        let id = cad_core::id();
        if !bypass
            && auto_axis
            && let Curve::Line { start, end } = &curve
        {
            let a = definition.points[start];
            let b = definition.points[end];
            let delta = DVec3::from_array(b) - DVec3::from_array(a);
            let length = delta.length();
            if length > 1e-9
                && let Some(index) = (0..3).find(|index| {
                    (0..3)
                        .filter(|other| other != index)
                        .all(|other| (b[other] - a[other]).abs() <= length * 1e-9)
                })
            {
                definition.constraints.push(Constraint {
                    id: cad_core::id(),
                    driven: false,
                    relation: Relation::AlignAxis {
                        member: id.clone(),
                        axis: axis(index),
                    },
                });
            }
        }
        definition.members.insert(
            id,
            Member {
                construction,
                curve,
            },
        );
    }
    publish(w, definition)?;
    if let Some(state) = &mut w.sketch3d {
        state.clicks = clicks;
        if completed {
            state.placement = None;
        }
        state.plane_origin = position;
        state.hover = None;
        state.placement = None;
        state.snap_target = None;
        state.error = None;
    }
    Ok(())
}
fn finish_curve(w: &mut Workspace) -> Result<()> {
    let mut definition = source(w).ok_or_else(|| invalid("The sketch is missing"))?;
    let state = w
        .sketch3d
        .as_ref()
        .ok_or_else(|| invalid("The editor has closed"))?;
    let points = state.clicks.clone();
    let construction = state.construction;
    let curve = match state.tool {
        Tool::Bezier => {
            if !(2..=16).contains(&points.len()) {
                return Err(invalid("A Bézier needs 2–16 control points"));
            }
            Some(Curve::Bezier { points })
        }
        Tool::Nurbs => {
            let n = points.len();
            let degree = state.degree;
            if n <= degree || n > 64 {
                return Err(invalid(
                    "A NURBS needs more controls than its degree, and at most 64 controls",
                ));
            }
            let knots = (0..n + degree + 1)
                .map(|index| {
                    if index <= degree {
                        0.
                    } else if index >= n {
                        1.
                    } else {
                        (index - degree) as f64 / (n - degree) as f64
                    }
                })
                .collect();
            Some(Curve::Nurbs {
                degree,
                points,
                knots,
                weights: vec!["1".into(); n],
            })
        }
        Tool::InterpolatingSpline => {
            if points.len() <= state.degree || points.len() > 64 {
                return Err(invalid(
                    "An interpolating spline needs more points than its degree, and at most 64 points",
                ));
            }
            Some(Curve::InterpolatingSpline {
                points,
                degree: state.degree,
                parameterization: state.parameterization,
            })
        }
        Tool::Circle if points.len() == 1 => Some(Curve::Circle {
            center: points[0].clone(),
            plane: state.plane.frame(state.plane_origin),
            radius: state.radius.clone(),
        }),
        Tool::Arc if !points.is_empty() => {
            return Err(invalid("An arc needs start, through and finish points"));
        }
        _ => None,
    };
    if let Some(curve) = curve {
        history(w);
        definition.members.insert(
            cad_core::id(),
            Member {
                construction,
                curve,
            },
        );
        publish(w, definition)?;
    }
    if let Some(state) = &mut w.sketch3d {
        state.clicks.clear();
        state.tool = Tool::Select;
        state.hover = None;
    }
    Ok(())
}
fn relation_entities(relation: &Relation) -> Vec<&Id> {
    match relation {
        Relation::Fixed { point, .. }
        | Relation::Coordinate { point, .. }
        | Relation::OnPlane { point, .. } => vec![point],
        Relation::Coincident { first, second }
        | Relation::Distance { first, second, .. }
        | Relation::AxisDistance { first, second, .. }
        | Relation::Angle { first, second, .. }
        | Relation::Parallel { first, second }
        | Relation::Perpendicular { first, second }
        | Relation::EqualLength { first, second }
        | Relation::Tangent { first, second, .. } => vec![first, second],
        Relation::PointLineDistance { point, line, .. } | Relation::OnLine { point, line, .. } => {
            vec![point, line]
        }
        Relation::Length { member, .. }
        | Relation::Radius { member, .. }
        | Relation::AxisAngle { member, .. }
        | Relation::AlignAxis { member, .. } => vec![member],
        Relation::Coplanar { points, .. } => points.iter().collect(),
        Relation::PointOnCurve { point, member, .. } => vec![point, member],
    }
}
fn delete_selection(w: &mut Workspace) -> Result<()> {
    let mut definition = source(w).ok_or_else(|| invalid("The sketch is missing"))?;
    let points = points_selected(w);
    let mut members = members_selected(w);
    members.extend(
        definition
            .members
            .iter()
            .filter(|(_, member)| {
                member
                    .curve
                    .points()
                    .iter()
                    .any(|point| points.contains(point))
            })
            .map(|(id, _)| id.clone()),
    );
    history(w);
    definition.members.retain(|id, _| !members.contains(id));
    definition.points.retain(|id, _| !points.contains(id));
    definition.constraints.retain(|constraint| {
        !relation_entities(&constraint.relation)
            .iter()
            .any(|id| points.contains(id) || members.contains(id))
    });
    publish(w, definition)?;
    if let Some(state) = &mut w.sketch3d {
        state.selection.clear();
        state.clicks.clear();
        state.dimension = None;
        state.popup = None;
    }
    Ok(())
}
pub fn tool_supported(command: &str) -> bool {
    matches!(
        command,
        "sketch.new-3d"
            | "sketch.line"
            | "sketch.point"
            | "sketch.arc"
            | "sketch.circle"
            | "sketch.spline"
            | "sketch.nurbs"
            | "sketch.interpolating-spline"
            | "sketch.select"
            | "sketch.dimension"
            | "sketch.smart-dimension"
            | "sketch.finish"
            | "sketch.finish-dimension"
            | "sketch.cancel-dimension"
            | "sketch.construction"
            | "sketch.undo"
            | "sketch.delete"
            | "sketch.tool.polyline"
            | "sketch.tool.line"
            | "sketch.tool.point"
            | "sketch.tool.arc"
            | "sketch.tool.circle"
            | "sketch.tool.spline"
            | "sketch.tool.bezier"
            | "sketch.tool.nurbs"
            | "sketch.tool.interpolating-spline"
    )
}
pub fn owns_input(w: &Workspace) -> bool {
    active(w)
}
pub fn action(w: &mut Workspace, command: &str, payload: Option<&Value>) -> bool {
    if command == "sketch.new-3d" {
        begin(w, None);
        return true;
    }
    if !active(w) || (command != ACTION && !tool_supported(command)) {
        return false;
    }
    let alias = match command {
        "sketch.select" => Some("tool/select"),
        "sketch.line" | "sketch.tool.polyline" | "sketch.tool.line" => Some("tool/line"),
        "sketch.point" | "sketch.tool.point" => Some("tool/point"),
        "sketch.arc" | "sketch.tool.arc" => Some("tool/arc"),
        "sketch.circle" | "sketch.tool.circle" => Some("tool/circle"),
        "sketch.spline" | "sketch.tool.spline" | "sketch.tool.bezier" => Some("tool/bezier"),
        "sketch.nurbs" | "sketch.tool.nurbs" => Some("tool/nurbs"),
        "sketch.interpolating-spline" | "sketch.tool.interpolating-spline" => {
            Some("tool/interpolating_spline")
        }
        "sketch.dimension" | "sketch.smart-dimension" => Some("smart-dimension"),
        "sketch.finish-dimension" => Some("finish-dimension"),
        "sketch.cancel-dimension" => Some("cancel-dimension"),
        "sketch.construction" => Some("make-construction"),
        "sketch.delete" => Some("delete"),
        "sketch.undo" => {
            undo(w);
            return true;
        }
        "sketch.finish" => {
            w.confirm_tool();
            return true;
        }
        _ => None,
    };
    if command != ACTION && alias.is_none() {
        return false;
    }
    let result = (|| -> Result<()> {
        let state = w
            .sketch3d
            .as_ref()
            .ok_or_else(|| invalid("The editor has closed"))?;
        if let Some(payload) = payload {
            for (key, value) in [
                ("document", json!(state.document)),
                ("owner", json!(state.owner)),
                ("feature", json!(state.feature)),
                ("editor", json!(state.token)),
            ] {
                if payload.get(key).is_some_and(|actual| actual != &value) {
                    return Err(invalid("Use the current 3D sketch actions"));
                }
            }
        }
        let kind = alias
            .or_else(|| payload.and_then(|payload| payload["action"].as_str()))
            .ok_or_else(|| invalid("Choose a spatial action"))?;
        if captured(w) && !matches!(kind, "cancel-drag") {
            return Err(invalid("Release or cancel the active spatial handle first"));
        }
        match kind {
            "fit-selection" => {
                fit_selection(w);
                Ok(())
            }
            _ if kind.starts_with("tool/") => {
                let tool: Tool = serde_json::from_value(json!(kind.trim_start_matches("tool/")))?;
                cancel_placement(w);
                if let Some(state) = &mut w.sketch3d {
                    state.tool = tool;
                    state.clicks.clear();
                    state.popup = None;
                }
                Ok(())
            }
            "cycle-plane" => {
                let plane = w
                    .sketch3d
                    .as_ref()
                    .ok_or_else(|| invalid("The editor closed"))?
                    .plane
                    .next();
                change_plane(w, plane)
            }
            "plane-last" => {
                let definition = displayed(w).ok_or_else(|| invalid("The sketch is missing"))?;
                let point = w
                    .sketch3d
                    .as_ref()
                    .and_then(|state| {
                        state.clicks.last().or_else(|| {
                            state.selection.iter().find_map(|entity| match entity {
                                Entity::Point { point } => Some(point),
                                _ => None,
                            })
                        })
                    })
                    .and_then(|point| definition.points.get(point))
                    .copied()
                    .ok_or_else(|| invalid("Select a point or begin a member first"))?;
                if let Some(state) = &mut w.sketch3d {
                    state.plane_origin = point;
                }
                let plane = w
                    .sketch3d
                    .as_ref()
                    .ok_or_else(|| invalid("The editor closed"))?
                    .plane;
                change_plane(w, plane)
            }
            "plane-origin" => {
                if let Some(state) = &mut w.sketch3d {
                    state.plane_origin = [0.; 3];
                }
                Ok(())
            }
            "place-point" => {
                let state = w
                    .sketch3d
                    .as_ref()
                    .ok_or_else(|| invalid("The editor closed"))?;
                let mut position = [0.; 3];
                for (index, coordinate) in position.iter_mut().enumerate() {
                    *coordinate = cad_core::expression::value(
                        w.session.visible(),
                        &state.owner,
                        &state.inputs[index],
                        Dimension::Length,
                    )?;
                }
                place(w, position, None, true, false)
            }
            "finish-member" => finish_curve(w),
            "smart-dimension" => {
                cancel_placement(w);
                if let Some(state) = &mut w.sketch3d {
                    state.tool = Tool::Dimension;
                    state.clicks.clear();
                    state.popup = None;
                }
                if dimension_choices(w).is_empty() {
                    w.status="Pick dimension references on the 3D sketch; Command/Ctrl-click adds references".into();
                    Ok(())
                } else {
                    update_dimension(w, None)
                }
            }
            "finish-dimension" => {
                finish_dimension(w);
                Ok(())
            }
            "cancel-dimension" => {
                cancel_dimension(w);
                Ok(())
            }
            "delete" => delete_selection(w),
            "make-construction" => {
                let selected = members_selected(w);
                if selected.is_empty() {
                    if let Some(state) = &mut w.sketch3d {
                        state.construction = !state.construction;
                    }
                    return Ok(());
                }
                let mut definition = source(w).ok_or_else(|| invalid("The sketch is missing"))?;
                let construction = !selected.iter().all(|id| {
                    definition
                        .members
                        .get(id)
                        .is_some_and(|member| member.construction)
                });
                history(w);
                for id in selected {
                    if let Some(member) = definition.members.get_mut(&id) {
                        member.construction = construction;
                    }
                }
                publish(w, definition)
            }
            "close-menu" => {
                if let Some(state) = &mut w.sketch3d {
                    state.popup = None;
                }
                Ok(())
            }
            "select-entity" => {
                let entity: Entity = serde_json::from_value(
                    payload
                        .and_then(|payload| payload.get("entity"))
                        .cloned()
                        .ok_or_else(|| invalid("Choose a current spatial entity"))?,
                )?;
                let source = source_ref(w).ok_or_else(|| invalid("The sketch is missing"))?;
                let valid = match &entity {
                    Entity::Point { point } => source.points.contains_key(point),
                    Entity::Member { member } => source.members.contains_key(member),
                    Entity::Origin => true,
                };
                if !valid {
                    return Err(invalid("This spatial entity was removed"));
                }
                let dimension = w
                    .sketch3d
                    .as_ref()
                    .is_some_and(|state| state.tool == Tool::Dimension);
                select(
                    w,
                    Some(entity),
                    payload.is_some_and(|payload| payload["extend"] == true),
                    dimension,
                )
            }
            _ if kind.starts_with("relation/") => {
                add_relations(w, kind.trim_start_matches("relation/"))
            }
            _ if kind.starts_with("delete-constraint/") => {
                let id = kind.trim_start_matches("delete-constraint/");
                let mut definition = source(w).ok_or_else(|| invalid("The sketch is missing"))?;
                history(w);
                definition
                    .constraints
                    .retain(|constraint| constraint.id != id);
                publish(w, definition)
            }
            "cancel-drag" => {
                escape_drag(w);
                Ok(())
            }
            _ => Err(invalid("Unsupported spatial action")),
        }
    })();
    if let Err(error) = result {
        w.status = error.to_string();
        if let Some(state) = &mut w.sketch3d {
            state.error = Some(error.to_string());
        }
    }
    true
}
pub fn current_source_is_valid(w: &Workspace) -> bool {
    !active(w)
        || (!captured(w)
            && !w.fields.keys().any(|key| key.starts_with(INVALID))
            && !w.fields.contains_key("feature-source-invalid"))
}
fn metadata(w: &Workspace) -> Value {
    let state = w.sketch3d.as_ref().expect("active spatial editor");
    json!({"contract":"tcad.presentation.sketch3d","version":CONTRACT_VERSION,"document":state.document,"owner":state.owner,"feature":state.feature,"editor":state.token,"classes":["spatial-sketch","modeling-field"],"source_revision":[w.session.generation,w.session.committed.revision,w.session.draft.as_ref().map(|draft|(&draft.id,draft.edit))]})
}
fn element(
    w: &Workspace,
    key: &str,
    role: &str,
    label: &str,
    value: Option<Value>,
) -> SemanticElement {
    let state = w.sketch3d.as_ref().expect("active spatial editor");
    let mut data = metadata(w);
    if let Some(value) = &value {
        data["json"] = value.clone();
    }
    SemanticElement {
        id: format!("sketch3d:{}/{key}", state.feature),
        role: role.into(),
        label: label.into(),
        value: value.map(|value| {
            value
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| value.to_string())
        }),
        state: "draft".into(),
        commands: vec![],
        relationships: vec![state.feature.clone()],
        data,
    }
}
fn group(w: &Workspace, key: &str, label: &str) -> SemanticElement {
    let mut element = element(w, &format!("group/{key}"), "sketch3d-group", label, None);
    element.data["classes"] = json!([
        "spatial-sketch",
        "modeling-field",
        "modeling-group",
        "inspector-section",
        key
    ]);
    element
}
fn choice(label: &str, value: impl Serialize) -> Value {
    json!({"label":label,"value":value})
}
fn field(
    w: &Workspace,
    key: &str,
    label: &str,
    value: Value,
    choices: Vec<Value>,
    unit: Option<&str>,
) -> SemanticElement {
    let mut element = element(
        w,
        &format!("field/{key}"),
        "sketch3d-field",
        label,
        Some(value.clone()),
    );
    element.data["setter"] = json!("sketch3d");
    element.data["field"] = json!(key);
    element.data["choices"] = json!(choices);
    element.data["editable"] =
        json!(!captured(w) && !w.fields.contains_key("feature-source-invalid"));
    element.data["unit"] = json!(unit);
    element.data["value_type"] = json!(if !choices.is_empty() {
        "choice"
    } else if value.is_boolean() {
        "boolean"
    } else if value.is_number() {
        "number"
    } else if unit.is_some() {
        "quantity"
    } else {
        "text"
    });
    element.data["dimension"] = json!(match unit {
        Some("mm") => "length",
        Some("deg") => "angle",
        _ => "scalar",
    });
    element.data["accessibility_label"] = json!(label);
    element
}
fn action_element(w: &Workspace, key: &str, label: &str, enabled: bool) -> SemanticElement {
    let mut element = element(w, &format!("action/{key}"), "sketch3d-action", label, None);
    element.state = if enabled { "available" } else { "disabled" }.into();
    element.commands = vec![ACTION.into()];
    element.data["action"] = json!(key);
    element.data["enabled"] = json!(enabled);
    element
}
fn note(w: &Workspace, key: &str, label: &str, state: &str) -> SemanticElement {
    let mut element = element(w, &format!("note/{key}"), "sketch3d-note", label, None);
    element.state = state.into();
    element
}
fn coordinate_constraint<'a>(
    definition: &'a Definition,
    point: &str,
    axis: Axis,
) -> Option<&'a Constraint> {
    definition.constraints.iter().find(|constraint|!constraint.driven&&matches!(&constraint.relation,Relation::Coordinate {point:target,axis:direction,..} if target==point&&*direction==axis))
}
fn radius_expression<'a>(definition: &'a Definition, member: &str) -> Option<&'a Expr> {
    definition
        .constraints
        .iter()
        .find_map(|constraint| match &constraint.relation {
            Relation::Radius {
                member: target,
                value,
            } if !constraint.driven && target == member => Some(value),
            _ => None,
        })
        .or_else(|| match &definition.members.get(member)?.curve {
            Curve::Circle { radius, .. } => Some(radius),
            _ => None,
        })
}
fn measured_constraint(w: &Workspace, constraint: &Constraint) -> Option<Expr> {
    let measured = current_evaluation(w)?
        .constraint_residuals
        .iter()
        .find(|row| row.constraint == constraint.id)?
        .measured?;
    Some(match constraint_value(&constraint.relation)?.1 {
        "deg" => Expr(format!("{} deg", measured.to_degrees())),
        "mm" => coordinate(measured),
        _ => Expr(measured.to_string()),
    })
}
fn source_pointer(definition: &Definition, key: &str) -> Option<String> {
    let parts = key.split('/').collect::<Vec<_>>();
    let constraint_pointer = |id: &str, tail: &str| {
        definition
            .constraints
            .iter()
            .position(|constraint| constraint.id == id)
            .map(|index| format!("/definition/constraints/{index}/relation/{tail}"))
    };
    let escape = |key: &str| key.replace('~', "~0").replace('/', "~1");
    match parts.as_slice() {
        ["point", point, index] => constraint_pointer(&coordinate_constraint(definition, point, axis(index.parse().ok()?))?.id, "value"),
        ["member", member, "radius"] => {
            if let Some(constraint) = definition.constraints.iter().find(|constraint| !constraint.driven && matches!(&constraint.relation, Relation::Radius { member: target, .. } if target == member)) {
                constraint_pointer(&constraint.id, "value")
            } else { Some(format!("/definition/members/{}/curve/radius", escape(member))) }
        }
        ["member", member, "weight", index] => Some(format!("/definition/members/{}/curve/weights/{index}", escape(member))),
        ["constraint", constraint, "value"] => {
            let row = definition.constraints.iter().find(|row| row.id == *constraint)?;
            constraint_pointer(constraint, if matches!(row.relation, Relation::PointOnCurve { .. }) { "parameter" } else { "value" })
        }
        ["constraint", constraint, "fixed", index] => constraint_pointer(constraint, &format!("position/{index}")),
        ["constraint", constraint, "tangent", index] => constraint_pointer(constraint, if *index == "0" { "first_parameter" } else { "second_parameter" }),
        _ => None,
    }
}
fn configured_fields(w: &Workspace) -> std::collections::BTreeMap<String, Expr> {
    let Some(state) = &w.sketch3d else {
        return Default::default();
    };
    let Some(configuration) = w.session.visible().active_configurations.get(&state.owner) else {
        return Default::default();
    };
    let Ok(resolved) =
        cad_core::configuration::resolve(w.session.visible(), &state.owner, configuration)
    else {
        return Default::default();
    };
    let Some(overrides) = resolved.overrides.feature_dimensions.get(&state.feature) else {
        return Default::default();
    };
    feature(w)
        .and_then(|feature| cad_core::configuration::dimensions::fields(&feature).ok())
        .unwrap_or_default()
        .into_iter()
        .filter_map(|field| {
            overrides
                .get(&field.path)
                .cloned()
                .map(|value| (field.pointer, value))
        })
        .collect()
}
fn constraint_value(relation: &Relation) -> Option<(&Expr, &'static str)> {
    match relation {
        Relation::Coordinate { value, .. }
        | Relation::Distance { value, .. }
        | Relation::AxisDistance { value, .. }
        | Relation::PointLineDistance { value, .. }
        | Relation::Length { value, .. }
        | Relation::Radius { value, .. } => Some((value, "mm")),
        Relation::Angle { value, .. } | Relation::AxisAngle { value, .. } => Some((value, "deg")),
        Relation::PointOnCurve { parameter, .. } => Some((parameter, "")),
        _ => None,
    }
}
fn relation_label(definition: &Definition, relation: &Relation) -> &'static str {
    match relation {
        Relation::Fixed { .. } => "Fixed XYZ",
        Relation::Coordinate { axis: Axis::X, .. } => "X coordinate",
        Relation::Coordinate { axis: Axis::Y, .. } => "Y coordinate",
        Relation::Coordinate { axis: Axis::Z, .. } => "Z coordinate",
        Relation::Coincident { .. } => "Coincident",
        Relation::Distance { .. } => "Distance",
        Relation::AxisDistance { axis: Axis::X, .. } => "X distance",
        Relation::AxisDistance { axis: Axis::Y, .. } => "Y distance",
        Relation::AxisDistance { axis: Axis::Z, .. } => "Z distance",
        Relation::PointLineDistance { .. } => "Point / line distance",
        Relation::Length { .. } => "Length",
        Relation::Radius { .. } => "Radius",
        Relation::Angle { .. } => "Angle",
        Relation::AxisAngle { .. } => "Angle to axis",
        Relation::AlignAxis { axis: Axis::X, .. } => "Along X",
        Relation::AlignAxis { axis: Axis::Y, .. } => "Along Y",
        Relation::AlignAxis { axis: Axis::Z, .. } => "Along Z",
        Relation::Parallel { .. } => "Parallel",
        Relation::Perpendicular { .. } => "Perpendicular",
        Relation::EqualLength { first, second }
            if [first, second].iter().all(|member| {
                matches!(
                    definition.members.get(*member).map(|member| &member.curve),
                    Some(Curve::Circle { .. })
                )
            }) =>
        {
            "Equal radius"
        }
        Relation::EqualLength { .. } => "Equal length",
        Relation::OnLine { .. } => "Point on line",
        Relation::OnPlane { .. } => "On workplane",
        Relation::Coplanar { .. } => "Coplanar",
        Relation::PointOnCurve { .. } => "Point on curve",
        Relation::Tangent { .. } => "Tangent",
    }
}
pub fn semantics(w: &Workspace) -> Vec<SemanticElement> {
    let Some(definition) = source(w) else {
        return vec![];
    };
    let Some(state) = w.sketch3d.as_ref() else {
        return vec![];
    };
    let mut out = vec![group(w, "tools", "3D Sketch")];
    for (id, member) in &definition.members {
        if let Curve::Line { start, end } = &member.curve {
            let visible = w.view.visibility.sketch_midpoints
                && crate::sketch_overlay::member_visible(w, member.construction);
            out.push(SemanticElement { id: format!("sketch3d:{}/midpoint/{id}", state.feature), role: "sketch-midpoint".into(), label: "Line midpoint".into(), value: None, state: if visible { "ready" } else { "hidden" }.into(), commands: vec![], relationships: vec![state.feature.clone(), id.clone()], data: json!({"contract":"tcad.viewport-visibility","version":1,"visible":visible,"anchor":{"kind":"member_midpoint","member":id,"endpoints":[start,end]},"selectable":false,"classes":["viewport-overlay"]}) });
        }
    }
    out.push(field(
        w,
        "name",
        "Name",
        json!(definition.name),
        vec![],
        None,
    ));
    out.push(field(
        w,
        "tool",
        "Tool",
        json!(state.tool),
        [
            Tool::Select,
            Tool::Line,
            Tool::Point,
            Tool::Arc,
            Tool::Circle,
            Tool::Bezier,
            Tool::Nurbs,
            Tool::InterpolatingSpline,
            Tool::Dimension,
        ]
        .into_iter()
        .map(|tool| choice(tool.label(), tool))
        .collect(),
        None,
    ));
    out.push(field(
        w,
        "construction",
        "Construction geometry",
        json!(state.construction),
        vec![],
        None,
    ));
    out.push(action_element(
        w,
        "finish-member",
        "Finish member",
        !state.clicks.is_empty(),
    ));
    out.push(action_element(
        w,
        "smart-dimension",
        "Smart Dimension",
        true,
    ));
    out.push(group(w, "plane", "Drawing plane and precision"));
    out.push(field(
        w,
        "plane",
        "Workplane",
        json!(state.plane),
        [DrawingPlane::XY, DrawingPlane::YZ, DrawingPlane::ZX]
            .into_iter()
            .map(|plane| choice(plane.label(), plane))
            .collect(),
        None,
    ));
    out.push(field(
        w,
        "depth",
        "Plane depth",
        json!(coordinate(state.plane_origin[state.plane.normal()])),
        vec![],
        Some("mm"),
    ));
    out.push(action_element(w, "cycle-plane", "Cycle plane · Tab", true));
    out.push(action_element(
        w,
        "plane-last",
        "Plane through last point",
        true,
    ));
    out.push(action_element(
        w,
        "plane-origin",
        "Plane through origin",
        true,
    ));
    out.push(field(w, "snap", "Snap", json!(state.snap), vec![], None));
    out.push(field(
        w,
        "grid",
        "Grid step",
        json!(state.grid),
        vec![],
        Some("mm"),
    ));
    out.push(field(
        w,
        "fine",
        "Fine precision",
        json!(state.fine),
        vec![],
        None,
    ));
    out.push(field(
        w,
        "auto-axis",
        "Infer world-axis alignment",
        json!(state.auto_axis),
        vec![],
        None,
    ));
    out.push(note(w,"plane-help","Part XYZ coordinates · Tab cycles planes through the last point · Alt bypasses grid, joins and inferred axes","ready"));
    out.push(field(
        w,
        "pick-filter",
        "Selection filter",
        json!(crate::sketch_overlay::pick_filter(w)),
        [
            ("all", "All geometry"),
            ("points", "Points & centers"),
            ("members", "Members"),
            ("construction", "Construction"),
        ]
        .into_iter()
        .map(|(value, label)| choice(label, value))
        .collect(),
        None,
    ));
    let mut inference = note(
        w,
        "snap-inference",
        "Origin snaps save a fixed part-origin point; entity snaps share a native point. Alt keeps placement independent.",
        "ready",
    );
    inference.data["snap_target"] = json!(state.snap_target);
    inference.data["origin_inference"] = json!("persistent_fixed_world_origin");
    out.push(inference);
    out.push(field(
        w,
        "reference-frame",
        "Sketch reference frame",
        json!(definition.workplane),
        vec![
            choice("XY", DrawingPlane::XY.frame([0.; 3])),
            choice("YZ", DrawingPlane::YZ.frame([0.; 3])),
            choice("ZX", DrawingPlane::ZX.frame([0.; 3])),
            choice("Saved frame", &definition.workplane),
        ],
        None,
    ));
    out.push(note(w,"frame-help","The reference frame controls downstream orientation. Drawing-plane and Tab changes leave it unchanged.","ready"));
    if matches!(state.tool, Tool::Nurbs | Tool::InterpolatingSpline) {
        out.push(field(
            w,
            "degree",
            "Degree",
            json!(state.degree),
            vec![],
            None,
        ));
    }
    if state.tool == Tool::InterpolatingSpline {
        out.push(field(
            w,
            "parameterization",
            "Spline parameterization",
            json!(state.parameterization),
            vec![
                choice(
                    "Uniform",
                    cad_core::sketch_spline::Parameterization::Uniform,
                ),
                choice(
                    "Chord length",
                    cad_core::sketch_spline::Parameterization::Chord,
                ),
                choice(
                    "Centripetal",
                    cad_core::sketch_spline::Parameterization::Centripetal,
                ),
            ],
            None,
        ));
    }
    if state.tool == Tool::Circle {
        out.push(field(
            w,
            "radius",
            "Circle radius",
            json!(state.radius),
            vec![],
            Some("mm"),
        ));
    }
    out.push(group(w, "xyz", "Place point by XYZ"));
    for index in 0..3 {
        out.push(field(
            w,
            &format!("input/{index}"),
            axis_name(axis(index)),
            json!(state.inputs[index]),
            vec![],
            Some("mm"),
        ));
    }
    out.push(action_element(
        w,
        "place-point",
        "Place point",
        state.tool != Tool::Select && state.tool != Tool::Dimension,
    ));
    if !state.selection.is_empty() {
        out.push(group(
            w,
            "selection",
            &format!("Selected geometry · {}", state.selection.len()),
        ));
        out.push(action_element(
            w,
            "fit-selection",
            "Fit sketch selection",
            true,
        ));
        out.push(note(
            w,
            "selection-hint",
            "Cmd-click adds or removes geometry · Equality and direction relations use the first selected reference; equal circle groups retain their size reference",
            "ready",
        ));
        let displayed = displayed(w).unwrap_or_else(|| definition.clone());
        for entity in &state.selection {
            match entity {
                Entity::Point { point } => {
                    if let Some(position) = displayed.points.get(point) {
                        for (index, coordinate_value) in position.iter().copied().enumerate() {
                            let value = coordinate_constraint(&definition, point, axis(index))
                                .and_then(|constraint| {
                                    constraint_value(&constraint.relation)
                                        .map(|(value, _)| value.clone())
                                })
                                .unwrap_or_else(|| coordinate(coordinate_value));
                            let mut coordinate_field = field(
                                w,
                                &format!("point/{point}/{index}"),
                                &format!("Driving {} from Origin", axis_name(axis(index))),
                                json!(value),
                                vec![],
                                Some("mm"),
                            );
                            coordinate_field.data["coordinate_edit_intent"] =
                                json!("driving_dimension");
                            coordinate_field.data["description"] = json!(
                                "Editing saves a driving coordinate relation from the part origin. Drag handles move solver seeds while existing relations remain active."
                            );
                            out.push(coordinate_field);
                        }
                    }
                }
                Entity::Member { member } => {
                    if let Some(source) = definition.members.get(member) {
                        out.push(field(
                            w,
                            &format!("member/{member}/construction"),
                            "Member is construction",
                            json!(source.construction),
                            vec![],
                            None,
                        ));
                        if matches!(source.curve, Curve::Circle { .. })
                            && let Some(radius) = radius_expression(&definition, member)
                        {
                            out.push(field(
                                w,
                                &format!("member/{member}/radius"),
                                "Radius",
                                json!(radius),
                                vec![],
                                Some("mm"),
                            ));
                        }
                        if let Curve::Nurbs { weights, .. } = &source.curve {
                            for (index, weight) in weights.iter().enumerate() {
                                out.push(field(
                                    w,
                                    &format!("member/{member}/weight/{index}"),
                                    &format!("Control weight {}", index + 1),
                                    json!(weight),
                                    vec![],
                                    Some(""),
                                ));
                            }
                        }
                    }
                }
                Entity::Origin => out.push(note(w, "origin", "Part origin", "ready")),
            }
        }
        out.push(action_element(w, "delete", "Delete selection", true));
        out.push(action_element(
            w,
            "make-construction",
            "Toggle construction",
            state
                .selection
                .iter()
                .any(|entity| matches!(entity, Entity::Member { .. })),
        ));
        for (kind, label) in [
            ("fixed", "Fix XYZ"),
            ("coincident", "Coincident"),
            ("along-x", "Along X"),
            ("along-y", "Along Y"),
            ("along-z", "Along Z"),
            ("parallel", "Parallel"),
            ("perpendicular", "Perpendicular"),
            ("equal-length", "Equal Length"),
            ("equal-radius", "Equal Radius"),
            ("on-plane", "On Workplane"),
            ("on-line", "Point On Line"),
            ("on-curve", "Point On Curve"),
            ("tangent", "Tangent"),
        ] {
            let plan = relation_for(w, kind);
            let enabled = current_source_is_valid(w)
                && plan.as_ref().is_ok_and(|relations| {
                    !relations.is_empty()
                        && relations
                            .iter()
                            .any(|relation| !has_relation(&definition, relation))
                });
            let mut element = action_element(w, &format!("relation/{kind}"), label, enabled);
            element.data["selection_count"] = json!(state.selection.len());
            element.data["description"] = json!(match kind {
                "fixed" => "Fix XYZ for every distinct control in the selected geometry.",
                "coincident" =>
                    "Join the selected points, or place every selected point on the one selected line or curve.",
                "along-x" | "along-y" | "along-z" =>
                    "Align every selected straight member with this world axis.",
                "parallel" => "Make every other selected line parallel to the first selected line.",
                "perpendicular" =>
                    "Make every other selected line perpendicular to the first selected line; they need not be perpendicular to one another.",
                "equal-length" =>
                    "Give every selected line or arc the length of the first selected member.",
                "equal-radius" =>
                    "Join selected full-circle size groups, retaining existing size references and driving dimensions.",
                "on-plane" => "Place every selected point on the current drawing workplane.",
                "on-line" =>
                    "Place every selected point on the supporting line of the one selected straight member.",
                "on-curve" =>
                    "Place every selected point on the one selected curve with a free bounded contact parameter.",
                _ =>
                    "Select two members for tangency at the end of the first and start of the second.",
            });
            if !enabled {
                element.data["disabled_reason"] = json!(if !current_source_is_valid(w) {
                    "Resolve the current field error before adding relations".into()
                } else if let Err(error) = plan {
                    error.to_string()
                } else {
                    if kind == "equal-radius" {
                        "The selected circles are already equal; their existing size reference is retained"
                    } else {
                        "The selected geometry already has these relations"
                    }.into()
                });
            }
            out.push(element);
        }
    }
    if let Some(dimension) = &state.dimension {
        out.push(group(w, "dimension", "Smart Dimension"));
        let choices = dimension_choices(w);
        out.push(field(
            w,
            "dimension-kind",
            "Dimension",
            json!(dimension.kind),
            choices
                .iter()
                .map(|(kind, label, _)| choice(label, kind))
                .collect(),
            None,
        ));
        if let Some(constraint) = definition
            .constraints
            .iter()
            .find(|constraint| constraint.id == dimension.constraint)
            && let Some((value, unit)) = constraint_value(&constraint.relation)
        {
            out.push(field(
                w,
                &format!("constraint/{}/value", constraint.id),
                relation_label(&definition, &constraint.relation),
                json!(value),
                vec![],
                Some(unit),
            ));
        }
        out.push(action_element(
            w,
            "finish-dimension",
            "Done Dimension",
            true,
        ));
        out.push(action_element(
            w,
            "cancel-dimension",
            "Cancel Dimension",
            true,
        ));
    }
    out.push(group(w, "constraints", "Dimensions and relations"));
    for (index, constraint) in definition.constraints.iter().enumerate() {
        out.push(note(
            w,
            &format!("constraint/{}/label", constraint.id),
            &format!(
                "{} · {}{}",
                index + 1,
                relation_label(&definition, &constraint.relation),
                if constraint.driven {
                    " · reference"
                } else {
                    ""
                }
            ),
            "ready",
        ));
        if let Some((value, unit)) = constraint_value(&constraint.relation)
            && state
                .dimension
                .as_ref()
                .is_none_or(|dimension| dimension.constraint != constraint.id)
        {
            out.push(field(
                w,
                &format!("constraint/{}/value", constraint.id),
                "Value",
                json!(value),
                vec![],
                Some(unit),
            ));
        }
        if let Relation::Fixed { position, .. } = &constraint.relation {
            for (index, value) in position.iter().enumerate() {
                out.push(field(
                    w,
                    &format!("constraint/{}/fixed/{index}", constraint.id),
                    &format!("Fixed {}", axis_name(axis(index))),
                    json!(value),
                    vec![],
                    Some("mm"),
                ));
            }
        }
        if let Relation::PointOnCurve { free, .. } = &constraint.relation {
            out.push(field(
                w,
                &format!("constraint/{}/free", constraint.id),
                "Free curve parameter",
                json!(free),
                vec![],
                None,
            ));
        }
        if let Relation::Tangent {
            first_parameter,
            second_parameter,
            ..
        } = &constraint.relation
        {
            for (index, value) in [first_parameter, second_parameter].into_iter().enumerate() {
                out.push(field(
                    w,
                    &format!("constraint/{}/tangent/{index}", constraint.id),
                    &format!("Curve {} parameter", index + 1),
                    json!(value),
                    vec![],
                    Some(""),
                ));
            }
        }
        out.push(field(
            w,
            &format!("constraint/{}/driven", constraint.id),
            "Reference only",
            json!(constraint.driven),
            vec![],
            None,
        ));
        out.push(action_element(
            w,
            &format!("delete-constraint/{}", constraint.id),
            "Remove relation",
            true,
        ));
    }
    let status = current_evaluation(w)
        .map(|evaluation| {
            format!(
                "{} degrees of freedom · residual {:.3e}",
                evaluation.report.degrees_of_freedom, evaluation.report.residual
            )
        })
        .unwrap_or_else(|| {
            "Evaluating spatial sketch; current source handles remain editable".into()
        });
    out.push(note(
        w,
        "status",
        state.error.as_deref().unwrap_or(&status),
        if state.error.is_some() {
            "error"
        } else if current_evaluation(w).is_some() {
            "ready"
        } else {
            "pending"
        },
    ));
    out.push(group(w, "entities", "Sketch entities"));
    let solved = displayed(w).unwrap_or_else(|| definition.clone());
    for (index, entity) in definition
        .points
        .keys()
        .cloned()
        .map(|point| Entity::Point { point })
        .chain(
            definition
                .members
                .keys()
                .cloned()
                .map(|member| Entity::Member { member }),
        )
        .chain(std::iter::once(Entity::Origin))
        .enumerate()
    {
        let key = entity.key();
        let label = match &entity {
            Entity::Point { .. } => format!("Point {}", index + 1),
            Entity::Member { member } => format!(
                "{} {}",
                match definition.members[member].curve {
                    Curve::Line { .. } => "Line",
                    Curve::Arc { .. } => "Arc",
                    Curve::Circle { .. } => "Circle",
                    Curve::Bezier { .. } => "Bézier",
                    Curve::Nurbs { .. } => "NURBS",
                    Curve::InterpolatingSpline { .. } => "Spline",
                },
                index + 1 - definition.points.len()
            ),
            Entity::Origin => "Origin".into(),
        };
        let selected = state.selection.contains(&entity);
        let mut action = action_element(w, &format!("select/{key}"), &label, !captured(w));
        action.data["action"] = json!("select-entity");
        action.data["entity"] = json!(entity);
        action.data["extend"] = json!(state.tool == Tool::Dimension);
        action.data["selected"] = json!(selected);
        out.push(action);
        let role = match &entity {
            Entity::Member { .. } => "sketch3d-member",
            _ => "sketch3d-point-handle",
        };
        let overlay_key = match &entity {
            Entity::Point { point } => format!("point/{point}"),
            Entity::Member { member } => format!("member/{member}"),
            Entity::Origin => "origin".into(),
        };
        let mut handle = element(w, &overlay_key, role, &label, None);
        handle.state = if selected {
            "selected"
        } else if current_evaluation(w).is_none() {
            "pending"
        } else {
            "ready"
        }
        .into();
        handle.commands = vec![ACTION.into()];
        handle.data["entity"] = json!(entity);
        handle.data["action"] = json!("select-entity");
        handle.data["extend"] = json!(state.tool == Tool::Dimension);
        handle.data["coordinate_system"] = json!("part");
        handle.data["occurrence_path"] = json!(state.occurrence);
        handle.data["classes"] = json!([
            "spatial-sketch",
            "viewport-overlay",
            if selected {
                "selected-entity"
            } else {
                "spatial-entity"
            }
        ]);
        match &entity {
            Entity::Point { point } => {
                handle.data["position_mm"] = json!(solved.points.get(point));
                handle.data["motion_directions"] = json!(
                    current_evaluation(w).and_then(|evaluation| evaluation.point_motion.get(point))
                );
                handle.data["drag"] = json!({"worker":"SolveSketch3d","source_point":point,"drawing_plane":state.plane,"axis_choices":["x","y","z"],"alt_bypass_snap":true});
            }
            Entity::Member { member } => {
                handle.data["curve"] = json!(definition.members[member].curve);
                handle.data["construction"] = json!(definition.members[member].construction);
                handle.data["polyline"] = json!(display_evaluation(w).and_then(|evaluation| {
                    evaluation
                        .polylines
                        .iter()
                        .find(|line| line.member == *member)
                }));
            }
            Entity::Origin => handle.data["position_mm"] = json!([0., 0., 0.]),
        }
        out.push(handle);
    }
    let mut plane = element(
        w,
        "drawing-plane",
        "sketch3d-workplane",
        "Drawing plane",
        None,
    );
    plane.data["plane"] = json!(state.plane.frame(state.plane_origin));
    plane.data["transient"] = json!(true);
    plane.data["reference_frame"] = json!(definition.workplane);
    out.push(plane);
    out.push(action_element(
        w,
        "close-menu",
        "Close selection menu",
        true,
    ));
    let overrides = configured_fields(w);
    let circle_owners = spatial::circle_size_owners(&definition).unwrap_or_default();
    for element in &mut out {
        if let Some(key) = element.data["field"].as_str().map(str::to_string) {
            if let Some(pointer) = source_pointer(&definition, &key)
                && let Some(value) = overrides.get(&pointer)
            {
                element.data["editable"] = json!(false);
                element.data["configured_expression"] = json!(value);
                element.data["json"] = json!(value);
                element.value = Some(value.0.clone());
                element.data["readonly_reason"] = json!(
                    "The active configuration controls this dimension. Edit its override in Configurations"
                );
            }
            if let Some(id) = key
                .strip_prefix("constraint/")
                .and_then(|key| key.strip_suffix("/value"))
                && let Some(constraint) = definition
                    .constraints
                    .iter()
                    .find(|constraint| constraint.id == id && constraint.driven)
            {
                element.data["editable"] = json!(false);
                element.data["readonly_reason"] = json!(
                    "Reference dimensions report current measurements; clear Reference only to drive geometry"
                );
                if let Some(value) = measured_constraint(w, constraint) {
                    element.data["json"] = json!(value);
                    element.value = Some(value.0);
                } else {
                    element.state = "pending".into();
                    element.value = Some("Pending measurement".into());
                }
            }
            if let Some(member) = key.strip_prefix("member/").and_then(|key| key.strip_suffix("/radius"))
                && definition.constraints.iter().any(|constraint| !constraint.driven && matches!(&constraint.relation, Relation::Length { member: target, .. } if target == member)) {
                element.data["editable"] = json!(false);
                element.data["readonly_reason"] = json!("The circumference dimension controls this circle; edit that dimension to change its size");
                if let Ok(radius) = circle_radius_with_size(
                    w,
                    &definition,
                    member,
                    circle_owners.get(member).map(|owner| &owner.effective_radius),
                ) {
                    element.data["json"] = json!(coordinate(radius));
                    element.value = Some(coordinate(radius).0);
                    element.data["measured_radius"] = json!(true);
                }
            }
            if let Some(member) = key
                .strip_prefix("member/")
                .and_then(|key| key.strip_suffix("/radius"))
                && let Some(owner) = circle_owners.get(member)
                && owner.reference != member
            {
                let reference = definition
                    .members
                    .keys()
                    .position(|id| id == &owner.reference)
                    .map_or_else(
                        || "the reference circle".into(),
                        |index| format!("Circle {}", index + 1),
                    );
                element.data["editable"] = json!(false);
                element.data["readonly_reason"] = json!(format!(
                    "Equal Radius inherits its size from {reference}; edit that circle or its driving dimension, or remove the equality relation"
                ));
                element.data["inherited_radius"] = json!(true);
                element.data["json"] = json!(owner.effective_radius);
                element.value = Some(owner.effective_radius.0.clone());
                if let Some(evaluation) = current_evaluation(w)
                    && let Some(Member {
                        curve: Curve::Circle { radius, .. },
                        ..
                    }) = evaluation.solved.members.get(member)
                {
                    element.data["json"] = json!(radius);
                    element.value = Some(radius.0.clone());
                }
            }
        }
        if let Some(error) = w.fields.get(&error_key(element)) {
            element.state = "error".into();
            element.data["diagnostics"] = json!([error]);
        }
        if let Some(reason) = element.data["readonly_reason"].as_str() {
            element.data["accessibility_label"] = json!(format!("{} · {reason}", element.label));
        }
    }
    out
}

#[derive(Clone)]
struct Hit {
    entity: Entity,
    axis: Option<Axis>,
    distance: f32,
}
fn axis_handles(w: &Workspace, rect: egui::Rect) -> Vec<(Id, Axis, [f64; 3], [f64; 3])> {
    if !w.view.visibility.sketch_points || !w.view.visibility.axes {
        return vec![];
    }
    let Some(state) = w.sketch3d.as_ref() else {
        return vec![];
    };
    let Some(definition) = displayed(w) else {
        return vec![];
    };
    let Some(matrix) = part_matrix(w) else {
        return vec![];
    };
    let Some(point) = state
        .selection
        .iter()
        .find_map(|entity| match entity {
            Entity::Point { point } => Some(point),
            _ => None,
        })
        .and_then(|id| definition.points.get(id).map(|point| (id.clone(), *point)))
    else {
        return vec![];
    };
    if !crate::sketch_overlay::point3d_visible(w, &definition, &point.0) {
        return vec![];
    }
    let mut out = vec![];
    for index in 0..3 {
        let a = transform(&matrix, point.1);
        let mut next = point.1;
        next[index] += 1.;
        let b = transform(&matrix, next);
        let (Some(a_screen), Some(b_screen)) =
            (viewport::project(w, rect, a), viewport::project(w, rect, b))
        else {
            continue;
        };
        let length = a_screen.distance(b_screen);
        if length < 0.01 {
            continue;
        }
        next = point.1;
        next[index] += 42f64 / f64::from(length);
        if viewport::project(w, rect, transform(&matrix, next)).is_some() {
            out.push((point.0.clone(), axis(index), point.1, next));
        }
    }
    out
}
fn segment_distance(point: egui::Pos2, a: egui::Pos2, b: egui::Pos2) -> f32 {
    let delta = b - a;
    let parameter = if delta.length_sq() > 0. {
        ((point - a).dot(delta) / delta.length_sq()).clamp(0., 1.)
    } else {
        0.
    };
    point.distance(a + delta * parameter)
}
pub fn fit_selection(w: &mut Workspace) {
    let Some(definition) = displayed(w) else {
        return;
    };
    let Some(state) = &w.sketch3d else {
        return;
    };
    let Some(matrix) = part_matrix(w) else {
        return;
    };
    let points = state
        .selection
        .iter()
        .filter_map(|entity| match entity {
            Entity::Point { point } => definition.points.get(point).copied(),
            Entity::Origin => Some([0.; 3]),
            _ => None,
        })
        .collect::<Vec<_>>();
    let members = state
        .selection
        .iter()
        .filter_map(|entity| match entity {
            Entity::Member { member } => Some(member),
            _ => None,
        })
        .collect::<Vec<_>>();
    let mut coordinates = points;
    if let Some(evaluation) = display_evaluation(w) {
        coordinates.extend(
            evaluation
                .polylines
                .iter()
                .filter(|line| members.contains(&&line.member))
                .flat_map(|line| line.points.iter().copied()),
        );
    }
    for member in members {
        if let Some(member) = definition.members.get(member) {
            coordinates.extend(
                member
                    .curve
                    .points()
                    .into_iter()
                    .filter_map(|point| definition.points.get(point).copied()),
            );
        }
    }
    let coordinates = coordinates
        .into_iter()
        .map(|point| transform(&matrix, point))
        .collect::<Vec<_>>();
    if coordinates.is_empty() {
        w.status = "Select spatial members or points to frame".into();
        return;
    }
    viewport::frame_points(w, &coordinates, None, false);
    w.pending_context_focus = false;
    crate::navigation::stop(w);
}
fn hit(w: &Workspace, rect: egui::Rect, cursor: egui::Pos2, handles: bool) -> Option<Hit> {
    let definition = displayed(w)?;
    let matrix = part_matrix(w)?;
    if handles {
        for (point, axis, _, end) in axis_handles(w, rect) {
            if let Some(end) = viewport::project(w, rect, transform(&matrix, end))
                && end.distance(cursor) <= 9.
            {
                return Some(Hit {
                    entity: Entity::Point { point },
                    axis: Some(axis),
                    distance: end.distance(cursor),
                });
            }
        }
    }
    let mut best = None;
    for (id, point) in &definition.points {
        let allowed = match crate::sketch_overlay::pick_filter(w) {
            "members" => false,
            "construction" => definition
                .members
                .values()
                .any(|member| member.construction && member.curve.points().contains(&id)),
            _ => true,
        };
        if !allowed || !crate::sketch_overlay::point3d_visible(w, &definition, id) {
            continue;
        }
        if let Some(point) = viewport::project(w, rect, transform(&matrix, *point)) {
            let distance = point.distance(cursor);
            if distance <= 9.
                && best
                    .as_ref()
                    .is_none_or(|previous: &Hit| distance < previous.distance)
            {
                best = Some(Hit {
                    entity: Entity::Point { point: id.clone() },
                    axis: None,
                    distance,
                });
            }
        }
    }
    if best.as_ref().is_some_and(|hit| hit.distance <= 4.) {
        return best;
    }
    if best.is_none()
        && w.view.visibility.origins
        && crate::sketch_overlay::pick_reference(w)
        && let Some(origin) = viewport::project(w, rect, transform(&matrix, [0.; 3]))
        && origin.distance(cursor) <= 8.
    {
        return Some(Hit {
            entity: Entity::Origin,
            axis: None,
            distance: origin.distance(cursor),
        });
    }
    if let Some(evaluation) = display_evaluation(w) {
        for line in &evaluation.polylines {
            if definition
                .members
                .get(&line.member)
                .is_none_or(|member| !crate::sketch_overlay::pick_member(w, member.construction))
            {
                continue;
            }
            for pair in line.points.windows(2) {
                if let (Some(a), Some(b)) = (
                    viewport::project(w, rect, transform(&matrix, pair[0])),
                    viewport::project(w, rect, transform(&matrix, pair[1])),
                ) {
                    let distance = segment_distance(cursor, a, b);
                    if distance <= 8.
                        && best
                            .as_ref()
                            .is_none_or(|previous| distance < previous.distance)
                    {
                        best = Some(Hit {
                            entity: Entity::Member {
                                member: line.member.clone(),
                            },
                            axis: None,
                            distance,
                        });
                    }
                }
            }
        }
    }
    // New straight members stay selectable while their first native solve is
    // pending. Curved control polygons are not substituted for real curves.
    if display_evaluation(w).is_none() {
        for (id, member) in &definition.members {
            if !crate::sketch_overlay::pick_member(w, member.construction) {
                continue;
            }
            if let Curve::Line { start, end } = &member.curve
                && let (Some(a), Some(b)) =
                    (definition.points.get(start), definition.points.get(end))
                && let (Some(a), Some(b)) = (
                    viewport::project(w, rect, transform(&matrix, *a)),
                    viewport::project(w, rect, transform(&matrix, *b)),
                )
            {
                let distance = segment_distance(cursor, a, b);
                if distance <= 8.
                    && best
                        .as_ref()
                        .is_none_or(|previous| distance < previous.distance)
                {
                    best = Some(Hit {
                        entity: Entity::Member { member: id.clone() },
                        axis: None,
                        distance,
                    });
                }
            }
        }
    }
    best
}
fn select(w: &mut Workspace, entity: Option<Entity>, toggle: bool, dimension: bool) -> Result<()> {
    if toggle && entity.is_some() && !dimension {
        // Modified clicks select actual geometry from every drawing tool. End
        // temporary authoring through its checkpoint instead of leaving an
        // unfinished point/member behind or creating geometry under the click.
        cancel_placement(w);
        let definition = source(w).ok_or_else(|| invalid("The sketch is missing"))?;
        let valid = |entity: &Entity| match entity {
            Entity::Point { point } => definition.points.contains_key(point),
            Entity::Member { member } => definition.members.contains_key(member),
            Entity::Origin => true,
        };
        if entity.as_ref().is_some_and(|entity| !valid(entity)) {
            return Err(invalid(
                "The unfinished member was canceled; select existing geometry",
            ));
        }
        if let Some(state) = &mut w.sketch3d {
            state.selection.retain(valid);
            state.tool = Tool::Select;
            state.clicks.clear();
            state.hover = None;
            state.snap_target = None;
        }
    }
    if let Some(state) = &mut w.sketch3d {
        state.popup = None;
        if let Some(entity) = entity {
            if toggle || dimension {
                if let Some(index) = state
                    .selection
                    .iter()
                    .position(|existing| existing == &entity)
                {
                    if toggle {
                        state.selection.remove(index);
                    }
                } else {
                    state.selection.push(entity);
                }
            } else {
                state.selection = vec![entity];
            }
            if dimension && state.selection.len() > 2 {
                state.selection.remove(0);
            }
        } else if !toggle && !dimension {
            state.selection.clear();
        }
    }
    if dimension && !dimension_choices(w).is_empty() {
        update_dimension(w, None)?;
    }
    Ok(())
}
fn overlay_properties(
    w: &Workspace,
    key: &str,
    role: &str,
    classes: &[&str],
    state: &str,
) -> std::collections::BTreeMap<String, String> {
    let id = w.sketch3d.as_ref().map_or_else(
        || format!("sketch3d:{key}"),
        |editor| format!("sketch3d:{}/{key}", editor.feature),
    );
    w.theme.properties_for(
        &viewport::style_target(w).child(
            &id,
            role,
            &classes
                .iter()
                .map(|class| class.to_string())
                .collect::<Vec<_>>(),
            state,
        ),
    )
}
fn shown(props: &std::collections::BTreeMap<String, String>) -> bool {
    !props.get("display").is_some_and(|value| value == "none")
        && !props
            .get("visibility")
            .is_some_and(|value| value == "hidden")
}
fn draw_segment(
    w: &Workspace,
    ui: &egui::Ui,
    rect: egui::Rect,
    pair: [[f64; 3]; 2],
    stroke: egui::Stroke,
    visible: bool,
    work: &mut usize,
) {
    let world = pair;
    if visible {
        for segment in crate::overlay_depth::segments(w, rect, world, work) {
            ui.painter()
                .with_clip_rect(rect)
                .line_segment(segment, stroke);
        }
    } else if let (Some(a), Some(b)) = (
        viewport::project(w, rect, world[0]),
        viewport::project(w, rect, world[1]),
    ) {
        ui.painter()
            .with_clip_rect(rect)
            .line_segment([a, b], stroke);
    }
}
fn draw(w: &Workspace, ui: &mut egui::Ui, rect: egui::Rect) {
    let Some(state) = w.sketch3d.as_ref() else {
        return;
    };
    let Some(definition) = displayed(w) else {
        return;
    };
    let Some(matrix) = part_matrix(w) else {
        return;
    };
    let painter = ui.painter().with_clip_rect(rect);
    let current = current_evaluation(w).is_some();
    let accent = viewport::selection_color(w);
    let mut work = 250_000;
    let plane_props = overlay_properties(
        w,
        "workplane",
        "sketch3d-workplane",
        &["viewport-guide", "spatial-workplane"],
        "ready",
    );
    if w.view.visibility.planes && shown(&plane_props) {
        let span = cad_core::expression::value(
            w.session.visible(),
            &state.owner,
            &state.grid,
            Dimension::Length,
        )
        .ok()
        .filter(|step| *step > 0.)
        .unwrap_or(1.)
            * 20.;
        let frame = state.plane.frame(state.plane_origin);
        let vertices = [[-span, -span], [span, -span], [span, span], [-span, span]];
        let points = vertices.map(|point| {
            std::array::from_fn(|axis| {
                frame.origin[axis] + frame.u[axis] * point[0] + frame.v[axis] * point[1]
            })
        });
        let color = plane_props
            .get("color")
            .and_then(|value| crate::ui::color(value))
            .unwrap_or(egui::Color32::from_rgba_unmultiplied(90, 130, 155, 110));
        for index in 0..4 {
            draw_segment(
                w,
                ui,
                rect,
                [points[index], points[(index + 1) % 4]].map(|point| transform(&matrix, point)),
                egui::Stroke::new(controls::number(&plane_props, "stroke-width", 1.), color),
                plane_props
                    .get("renderer-overlay-depth")
                    .is_some_and(|mode| mode == "visible"),
                &mut work,
            );
        }
    }
    if let Some(evaluation) = display_evaluation(w) {
        for line in &evaluation.polylines {
            if !definition.members.contains_key(&line.member)
                || !crate::sketch_overlay::member_visible(w, line.construction)
            {
                continue;
            }
            let selected = state.selection.contains(&Entity::Member {
                member: line.member.clone(),
            });
            let props = overlay_properties(
                w,
                &format!("member/{}", line.member),
                "sketch3d-member",
                &[
                    "spatial-curve",
                    if line.construction {
                        "construction"
                    } else {
                        "regular"
                    },
                ],
                if selected {
                    "selected"
                } else if current {
                    "ready"
                } else {
                    "last-valid"
                },
            );
            if !shown(&props) {
                continue;
            }
            let color = props
                .get("color")
                .and_then(|value| crate::ui::color(value))
                .unwrap_or_else(|| {
                    if selected {
                        accent
                    } else if line.construction {
                        egui::Color32::from_rgb(145, 120, 165)
                    } else {
                        egui::Color32::from_rgb(30, 114, 182)
                    }
                });
            let stroke = egui::Stroke::new(
                controls::number(&props, "stroke-width", if selected { 3. } else { 1.5 }),
                color,
            );
            for pair in line.points.windows(2) {
                draw_segment(
                    w,
                    ui,
                    rect,
                    [pair[0], pair[1]].map(|point| transform(&matrix, point)),
                    stroke,
                    props
                        .get("renderer-overlay-depth")
                        .is_some_and(|mode| mode == "visible"),
                    &mut work,
                );
            }
        }
    } else {
        for (id, member) in &definition.members {
            if !crate::sketch_overlay::member_visible(w, member.construction) {
                continue;
            }
            if let Curve::Line { start, end } = &member.curve
                && let (Some(a), Some(b)) =
                    (definition.points.get(start), definition.points.get(end))
            {
                let props = overlay_properties(
                    w,
                    &format!("member/{id}"),
                    "sketch3d-member",
                    &["spatial-curve"],
                    "pending",
                );
                if shown(&props) {
                    draw_segment(
                        w,
                        ui,
                        rect,
                        [*a, *b].map(|point| transform(&matrix, point)),
                        egui::Stroke::new(1.5, accent),
                        props
                            .get("renderer-overlay-depth")
                            .is_some_and(|mode| mode == "visible"),
                        &mut work,
                    );
                }
            }
        }
    }
    for (id, point) in &definition.points {
        if !crate::sketch_overlay::point3d_visible(w, &definition, id) {
            continue;
        }
        let selected = state
            .selection
            .contains(&Entity::Point { point: id.clone() });
        let props = overlay_properties(
            w,
            &format!("point/{id}"),
            "sketch3d-handle",
            &["point-handle"],
            if selected {
                "selected"
            } else if current {
                "ready"
            } else {
                "pending"
            },
        );
        if !shown(&props) {
            continue;
        }
        let Some(point) = viewport::project(w, rect, transform(&matrix, *point)) else {
            continue;
        };
        let color = props
            .get("color")
            .and_then(|value| crate::ui::color(value))
            .unwrap_or(if selected {
                accent
            } else {
                egui::Color32::from_rgb(51, 117, 162)
            });
        painter.circle_filled(
            point,
            controls::number(&props, "handle-size", if selected { 4.5 } else { 2.7 }),
            color,
        );
    }
    for (member_id, member) in &definition.members {
        if !crate::sketch_overlay::member_visible(w, member.construction) {
            continue;
        }
        if let Curve::Line { start, end } = &member.curve
            && let (Some(a), Some(b)) = (definition.points.get(start), definition.points.get(end))
        {
            let center = std::array::from_fn(|axis| (a[axis] + b[axis]) * 0.5);
            crate::sketch_overlay::midpoint(
                w,
                ui,
                rect,
                &format!("sketch3d:{}/midpoint/{member_id}", state.feature),
                transform(&matrix, center),
            );
        }
    }
    for (id, axis, a, b) in axis_handles(w, rect) {
        let props = overlay_properties(
            w,
            &format!("point/{id}/axis/{}", axis.index()),
            "sketch3d-axis-handle",
            &["point-handle", "axis-handle"],
            "ready",
        );
        if !shown(&props) {
            continue;
        }
        let color = props
            .get("color")
            .and_then(|value| crate::ui::color(value))
            .unwrap_or(match axis {
                Axis::X => egui::Color32::from_rgb(194, 66, 58),
                Axis::Y => egui::Color32::from_rgb(57, 151, 82),
                Axis::Z => egui::Color32::from_rgb(53, 111, 192),
            });
        if let (Some(a), Some(b)) = (
            viewport::project(w, rect, transform(&matrix, a)),
            viewport::project(w, rect, transform(&matrix, b)),
        ) {
            painter.line_segment(
                [a, b],
                egui::Stroke::new(controls::number(&props, "stroke-width", 1.7), color),
            );
            painter.circle_filled(b, controls::number(&props, "handle-size", 4.), color);
            painter.text(
                b + egui::vec2(4., -4.),
                egui::Align2::LEFT_BOTTOM,
                axis_name(axis),
                egui::FontId::proportional(controls::number(&props, "font-size", 11.)),
                color,
            );
        }
    }
    if w.view.visibility.origins
        && let Some(origin) = viewport::project(w, rect, transform(&matrix, [0.; 3]))
    {
        let props = overlay_properties(
            w,
            "origin",
            "sketch3d-origin",
            &["viewport-guide"],
            if state.selection.contains(&Entity::Origin) {
                "selected"
            } else {
                "ready"
            },
        );
        if shown(&props) {
            let color = props
                .get("color")
                .and_then(|value| crate::ui::color(value))
                .unwrap_or(accent);
            painter.line_segment(
                [origin - egui::vec2(5., 0.), origin + egui::vec2(5., 0.)],
                egui::Stroke::new(1., color),
            );
            painter.line_segment(
                [origin - egui::vec2(0., 5.), origin + egui::vec2(0., 5.)],
                egui::Stroke::new(1., color),
            );
        }
    }
    if let Some(hover) = state.hover {
        let props = overlay_properties(
            w,
            "cursor",
            "sketch3d-cursor",
            &["viewport-guide", "placement-preview"],
            "pending",
        );
        if shown(&props) {
            if let Some(point) = viewport::project(w, rect, transform(&matrix, hover)) {
                painter.circle_stroke(
                    point,
                    controls::number(&props, "handle-size", 4.),
                    egui::Stroke::new(1.5, accent),
                );
                if let Some(target) = &state.snap_target {
                    painter.text(
                        point + egui::vec2(12., -12.),
                        egui::Align2::LEFT_BOTTOM,
                        if matches!(target, Entity::Origin) {
                            "Coincident with part origin · persistent reference · Alt bypasses"
                        } else {
                            "Join endpoint · persistent shared point · Alt bypasses"
                        },
                        egui::FontId::proportional(controls::number(&props, "font-size", 11.)),
                        accent,
                    );
                }
            }
            if let Some(start) = state
                .clicks
                .last()
                .and_then(|id| definition.points.get(id))
                .copied()
            {
                draw_segment(
                    w,
                    ui,
                    rect,
                    [start, hover].map(|point| transform(&matrix, point)),
                    egui::Stroke::new(1.2, accent),
                    props
                        .get("renderer-overlay-depth")
                        .is_some_and(|mode| mode == "visible"),
                    &mut work,
                );
            }
        }
    }
    if let Some(drag) = &state.drag
        && let Some(goal) = drag.last
    {
        let props = overlay_properties(
            w,
            "drag-goal",
            "sketch3d-drag-goal",
            &["viewport-guide", "cursor-goal"],
            "pending",
        );
        if shown(&props)
            && let Some(point) = viewport::project(w, rect, transform(&drag.matrix, goal))
        {
            painter.circle_stroke(point, 6., egui::Stroke::new(1.2, accent));
        }
    }
    if let Some(area) = controls::overlay_rect(
        "sketch3d.viewport-status",
        "sketch3d-status",
        rect,
        egui::Rect::from_min_size(
            egui::pos2(rect.left() + 12., rect.bottom() - 38.),
            egui::vec2(rect.width() * 0.66, 26.),
        ),
    ) {
        ui.scope_builder(egui::UiBuilder::new().max_rect(area), |ui| {
            let label = format!(
                "3D Sketch · {} plane · {}{}",
                state.plane.label(),
                state.tool.label(),
                if current {
                    ""
                } else {
                    " · retained curves / pending handles"
                }
            );
            controls::label(
                ui,
                "sketch3d.viewport-status",
                "sketch3d-status",
                &["viewport-overlay"],
                &label,
                if current { "ready" } else { "pending" },
            );
        });
    }
}
pub fn escape(w: &mut Workspace) -> bool {
    if !active(w) {
        return false;
    }
    if escape_drag(w) || cancel_dimension(w) || cancel_placement(w) {
        return true;
    }
    if let Some(state) = &mut w.sketch3d {
        if !state.clicks.is_empty() || state.tool != Tool::Select {
            state.clicks.clear();
            state.tool = Tool::Select;
            state.popup = None;
            state.hover = None;
            w.status = "Sketch tool stopped; completed geometry retained. Escape again exits and keeps the sketch.".into();
            return true;
        }
        if state.popup.take().is_some() {
            return true;
        }
    }
    false
}
pub fn overlay(
    w: &mut Workspace,
    ui: &mut egui::Ui,
    rect: egui::Rect,
    response: &egui::Response,
    blocked: bool,
) -> bool {
    if !owns_input(w) {
        return false;
    }
    poll(w);
    let modifiers = ui.input(|input| input.modifiers);
    let cycle_plane = response.hovered()
        && !blocked
        && !ui.ctx().egui_wants_keyboard_input()
        && ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Tab));
    if cycle_plane {
        ui.ctx()
            .memory_mut(|memory| memory.request_focus(response.id));
        if let Some(state) = &mut w.sketch3d
            && let Some(drag) = &mut state.drag
        {
            state.plane = state.plane.next();
            drag.plane = state.plane.frame(drag.position);
            let _ = drag;
        } else {
            let plane = w
                .sketch3d
                .as_ref()
                .map_or(DrawingPlane::XY, |state| state.plane.next());
            if let Err(error) = change_plane(w, plane) {
                w.status = error.to_string();
            }
        }
    }
    if cycle_plane
        && let Some(cursor) = response.hover_pos()
        && let Some(drag) = w.sketch3d.as_ref().and_then(|state| state.drag.as_ref())
    {
        let grab = hit_plane(w, rect, cursor, &drag.plane, &drag.matrix).unwrap_or(drag.position);
        if let Some(drag) = w.sketch3d.as_mut().and_then(|state| state.drag.as_mut()) {
            drag.grab = grab;
        }
    }
    if response.hovered() && !blocked && ui.input(|input| input.key_pressed(egui::Key::Escape)) {
        escape(w);
    }
    if captured(w) {
        let drag = w.sketch3d.as_ref().and_then(|state| state.drag.as_ref());
        let cursor = response
            .interact_pointer_pos()
            .or_else(|| ui.input(|input| input.pointer.interact_pos()));
        if let Some(drag) = drag.filter(|drag| drag.viewport == response.id && !drag.released)
            && let Some(cursor) = cursor
        {
            let mut position = if let Some(axis) = drag.axis {
                axis_cursor(w, rect, cursor, drag.position, &drag.matrix, axis).map(|parameter| {
                    let mut point = drag.position;
                    point[axis.index()] += parameter - drag.axis_grab;
                    point
                })
            } else {
                hit_plane(w, rect, cursor, &drag.plane, &drag.matrix).map(|point| {
                    std::array::from_fn(|index| {
                        drag.position[index] + point[index] - drag.grab[index]
                    })
                })
            };
            if let Some(point) = position.as_mut()
                && !modifiers.alt
                && let Some(state) = &w.sketch3d
                && state.snap
                && let Ok(step) = cad_core::expression::value(
                    w.session.visible(),
                    &state.owner,
                    &state.grid,
                    Dimension::Length,
                )
                && step > 0.
            {
                let step = if state.fine { step * 0.1 } else { step };
                if let Some(axis) = drag.axis {
                    point[axis.index()] = (point[axis.index()] / step).round() * step;
                } else {
                    for (index, coordinate) in point.iter_mut().enumerate() {
                        if index != state.plane.normal() {
                            *coordinate = (*coordinate / step).round() * step;
                        }
                    }
                }
            }
            if let Some(position) = position
                && let Err(error) = request_drag(w, position)
            {
                w.status = error.to_string();
            }
        }
        if ui.input(|input| input.pointer.button_released(egui::PointerButton::Primary))
            && let Some(state) = &mut w.sketch3d
        {
            if let Some(drag) = &mut state.drag {
                drag.released = true;
            }
            if state
                .drag
                .as_ref()
                .is_some_and(|drag| drag.pending.is_none())
            {
                state.drag = None;
            }
        }
        draw(w, ui, rect);
        return true;
    }
    let tool = w.sketch3d.as_ref().map_or(Tool::Select, |state| state.tool);
    if response.hovered()
        && !blocked
        && let Some(cursor) = response.hover_pos()
    {
        if !matches!(tool, Tool::Select | Tool::Dimension)
            && let Some(matrix) = part_matrix(w)
            && let Some(plane) = w
                .sketch3d
                .as_ref()
                .map(|state| state.plane.frame(state.plane_origin))
            && let Some(point) = hit_plane(w, rect, cursor, &plane, &matrix)
        {
            let (point, target) = snap(w, rect, cursor, point, modifiers.alt);
            if let Some(state) = &mut w.sketch3d {
                state.hover = Some(point);
                state.snap_target = target;
            }
        }
        let extend_selection = modifiers.command || modifiers.ctrl || modifiers.shift;
        if tool == Tool::Select
            && !extend_selection
            && response.drag_started_by(egui::PointerButton::Primary)
            && let Some(hit) = hit(w, rect, cursor, true)
            && let Entity::Point { point } = hit.entity
        {
            if let Err(error) = start_drag(w, rect, response, cursor, point, hit.axis) {
                w.status = error.to_string();
            }
        } else if response.clicked() {
            let result = if extend_selection || matches!(tool, Tool::Select | Tool::Dimension) {
                let hit = hit(w, rect, cursor, false);
                let entity = hit.map(|hit| hit.entity);
                let selected = entity.is_some();
                let result = select(w, entity, extend_selection, tool == Tool::Dimension);
                if selected
                    && tool == Tool::Select
                    && !extend_selection
                    && let Some(state) = &mut w.sketch3d
                    && !state.selection.is_empty()
                {
                    state.popup = Some(cursor);
                }
                result
            } else {
                let cursor = w
                    .sketch3d
                    .as_ref()
                    .and_then(|state| state.hover.map(|point| (point, state.snap_target.clone())));
                if let Some((point, target)) = cursor {
                    place(w, point, target, false, modifiers.alt)
                } else {
                    Err(invalid(
                        "The cursor is parallel to the drawing plane; cycle planes or use XYZ inputs",
                    ))
                }
            };
            if let Err(error) = result {
                w.status = error.to_string();
            }
        } else if response.secondary_clicked()
            && let Some(hit) = hit(w, rect, cursor, false)
        {
            cancel_placement(w);
            if w.sketch3d
                .as_ref()
                .is_some_and(|state| state.dimension.is_some())
            {
                finish_dimension(w);
            }
            let definition = source(w);
            let current = definition
                .as_ref()
                .is_some_and(|definition| match &hit.entity {
                    Entity::Point { point } => definition.points.contains_key(point),
                    Entity::Member { member } => definition.members.contains_key(member),
                    Entity::Origin => true,
                });
            if current {
                let already_selected = w
                    .sketch3d
                    .as_ref()
                    .is_some_and(|state| state.selection.contains(&hit.entity));
                if !already_selected && let Err(error) = select(w, Some(hit.entity), false, false) {
                    w.status = error.to_string();
                }
                if let Some(state) = &mut w.sketch3d {
                    state.tool = Tool::Select;
                    state.clicks.clear();
                    state.hover = None;
                    state.popup = Some(cursor);
                }
            }
        }
    } else if let Some(state) = &mut w.sketch3d {
        state.hover = None;
    }
    if response.hovered() && !blocked && !ui.ctx().egui_wants_keyboard_input() {
        if ui.input(|input| input.key_pressed(egui::Key::Enter)) {
            if w.sketch3d
                .as_ref()
                .is_some_and(|state| state.dimension.is_some())
            {
                finish_dimension(w);
            } else if let Err(error) = finish_curve(w) {
                w.status = error.to_string();
            }
        }
        if ui.input(|input| {
            input.key_pressed(egui::Key::Delete) || input.key_pressed(egui::Key::Backspace)
        }) && let Err(error) = delete_selection(w)
        {
            w.status = error.to_string();
        }
    }
    draw(w, ui, rect);
    popup(w, ui, rect);
    captured(w)
}
fn popup(w: &mut Workspace, ui: &mut egui::Ui, rect: egui::Rect) {
    let Some(position) = w.sketch3d.as_ref().and_then(|state| state.popup) else {
        return;
    };
    let elements = semantics(w)
        .into_iter()
        .filter(|element| {
            element.role == "sketch3d-action"
                && (element.data["action"] == "smart-dimension"
                    || element.data["action"] == "make-construction"
                    || element.data["action"] == "delete"
                    || element.data["action"]
                        .as_str()
                        .is_some_and(|action| action.starts_with("relation/")))
        })
        .collect::<Vec<_>>();
    let id = w
        .sketch3d
        .as_ref()
        .map(|state| format!("sketch3d:{}/context-menu", state.feature))
        .unwrap_or_default();
    let props = controls::properties(&id, "sketch3d-context-menu", &["selection-popup"], "ready");
    if !shown(&props) {
        return;
    }
    egui::Area::new(egui::Id::new(&id))
        .fixed_pos(egui::pos2(
            position.x.min(rect.right() - 160.).max(rect.left()),
            position.y.min(rect.bottom() - 230.).max(rect.top()),
        ))
        .order(egui::Order::Foreground)
        .show(ui.ctx(), |ui| {
            controls::apply_properties(ui, &props);
            controls::frame_from_properties(ui, &props).show(ui, |ui| {
                ui.set_max_width(230.);
                controls::label(
                    ui,
                    &format!("{id}/title"),
                    "sketch3d-menu-heading",
                    &[],
                    "Sketch geometry",
                    "ready",
                );
                for element in elements {
                    if element.state != "disabled"
                        && controls::button(
                            ui,
                            &element.id,
                            &element.role,
                            &["selection-action"],
                            &element.label,
                            true,
                            false,
                        )
                        .clicked()
                    {
                        action(w, ACTION, Some(&element.data));
                        if let Some(state) = &mut w.sketch3d {
                            state.popup = None;
                        }
                    }
                }
                if controls::button(
                    ui,
                    &format!("{id}/close"),
                    "sketch3d-action",
                    &[],
                    "Close",
                    true,
                    false,
                )
                .clicked()
                    && let Some(state) = &mut w.sketch3d
                {
                    state.popup = None;
                }
            });
        });
}
pub(crate) fn render_control(ui: &mut egui::Ui, w: &mut Workspace, element: &SemanticElement) {
    if element.data.get("setter").is_some() {
        crate::ui::semantic_field(ui, w, element);
    } else if element.role.ends_with("-action") {
        let response = controls::button(
            ui,
            &element.id,
            &element.role,
            &["modeling-action"],
            &element.label,
            element.state != "disabled",
            false,
        );
        let response = if let Some(reason) = element.data["disabled_reason"].as_str() {
            response.on_disabled_hover_text(reason)
        } else {
            response
        };
        let response = if let Some(description) = element.data["description"].as_str() {
            response.on_hover_text(description)
        } else {
            response
        };
        if response.clicked() {
            action(w, ACTION, Some(&element.data));
        }
    } else {
        controls::label(
            ui,
            &element.id,
            &element.role,
            &["modeling-help"],
            &element.label,
            &element.state,
        );
    }
}
pub fn show(ui: &mut egui::Ui, w: &mut Workspace) -> bool {
    if !active(w) {
        return false;
    }
    let mut groups: Vec<(SemanticElement, Vec<SemanticElement>)> = vec![];
    for element in semantics(w) {
        if !matches!(
            element.role.as_str(),
            "sketch3d-group" | "sketch3d-field" | "sketch3d-action" | "sketch3d-note"
        ) {
            continue;
        }
        if element.role.ends_with("-group") {
            groups.push((element, vec![]));
        } else if let Some((_, children)) = groups.last_mut() {
            children.push(element);
        }
    }
    for (group, children) in groups {
        let classes = group
            .data
            .get("classes")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>();
        let open = group.id.ends_with("/tools")
            || group.id.ends_with("/selection")
            || group.id.ends_with("/dimension");
        controls::inspector_section(
            ui,
            controls::InspectorSection {
                id: &group.id,
                role: &group.role,
                classes: &classes,
                label: &group.label,
                state: "draft",
                collapse: Some(open),
                open: None,
            },
            |ui| {
                let mut children = children.into_iter().peekable();
                while let Some(element) = children.next() {
                    if element.role.ends_with("-action") {
                        let mut row = vec![element];
                        while children
                            .peek()
                            .is_some_and(|next| next.role.ends_with("-action"))
                        {
                            if let Some(next) = children.next() {
                                row.push(next);
                            }
                        }
                        ui.horizontal_wrapped(|ui| {
                            for element in row {
                                render_control(ui, w, &element);
                            }
                        });
                    } else {
                        render_control(ui, w, &element);
                    }
                }
            },
        );
    }
    true
}
struct InactivePoint {
    definition: Id,
    feature: Id,
    point: Id,
    path: Vec<Id>,
    instance: Option<Id>,
    position: [f64; 3],
    selected: bool,
}
pub fn visibility_changed(w: &mut Workspace) {
    if let Some(state) = &mut w.sketch3d {
        state.popup = None;
        state.hover = None;
        state.snap_target = None;
    }
}
fn inactive_points(w: &Workspace) -> Vec<InactivePoint> {
    if active(w) || !w.view.visibility.sketch_points {
        return vec![];
    }
    let mode = crate::sketch_overlay::mode(w, true);
    if matches!(mode, cad_core::presentation::OverlayVisibility::Hidden) {
        return vec![];
    }
    let Some(evaluation) = viewport::display_evaluation(w) else {
        return vec![];
    };
    let mut out = vec![];
    for instance in viewport::display_instances(w, evaluation)
        .into_iter()
        .filter(|instance| viewport::visible(w, instance))
    {
        let key = cad_core::history::mesh_key(instance);
        let limit = w
            .view
            .history_cursor
            .as_ref()
            .filter(|cursor| cursor.definition == instance.definition)
            .and_then(|cursor| cad_core::history::stage(evaluation, key, &cursor.node))
            .map(|stage| stage.index);
        let Some(part) = w.session.visible().parts.get(&instance.definition) else {
            continue;
        };
        for feature in part.features.iter().filter(|feature| !feature.suppressed) {
            let Operation::Sketch3d { definition } = &feature.operation else {
                continue;
            };
            let Some(geometry) = evaluation
                .spatial_sketches
                .get(&format!("{key}::{}", feature.id))
            else {
                continue;
            };
            if geometry.producer.definition != instance.definition
                || geometry.producer.node != feature.id
                || geometry.solved.id != definition.id
            {
                continue;
            }
            if limit.is_some_and(|limit| {
                cad_core::history::stage(evaluation, key, &feature.id)
                    .is_none_or(|stage| stage.index > limit)
            }) {
                continue;
            }
            let selected = viewport::spatial_feature_selected(w, instance, &geometry.producer);
            if matches!(mode, cad_core::presentation::OverlayVisibility::Auto) && !selected {
                continue;
            }
            let used = geometry
                .solved
                .members
                .values()
                .flat_map(|member| member.curve.points())
                .collect::<std::collections::BTreeSet<_>>();
            for (point, position) in &geometry.solved.points {
                if used.contains(point) || !definition.points.contains_key(point) {
                    continue;
                }
                out.push(InactivePoint {
                    definition: instance.definition.clone(),
                    feature: feature.id.clone(),
                    point: point.clone(),
                    path: instance.path.clone(),
                    instance: instance.path.last().cloned(),
                    position: transform(&instance.matrix, *position),
                    selected,
                });
            }
        }
    }
    out
}
fn inactive_properties(
    w: &Workspace,
    point: &InactivePoint,
) -> std::collections::BTreeMap<String, String> {
    w.theme.properties_for(&viewport::style_target(w).child(
        &format!(
            "sketch3d:{}:{}/point/{}:{}",
            point.definition,
            point.feature,
            point.point,
            point.path.join("/")
        ),
        "sketch3d-point-handle",
        &[
            "spatial-sketch".into(),
            "viewport-overlay".into(),
            "isolated-point".into(),
        ],
        if point.selected { "selected" } else { "ready" },
    ))
}
pub fn overlay_inactive(w: &Workspace, ui: &egui::Ui, rect: egui::Rect) {
    let painter = ui.painter().with_clip_rect(rect);
    let mut work = crate::sketch_overlay::DEPTH_WORK;
    for point in inactive_points(w) {
        let props = inactive_properties(w, &point);
        if !shown(&props)
            || (!crate::sketch_overlay::xray(w, true, &props)
                && !viewport::visible_at(w, rect, point.position, &mut work))
        {
            continue;
        }
        if let Some(position) = viewport::project(w, rect, point.position) {
            let color = props
                .get("color")
                .and_then(|value| crate::ui::color(value))
                .unwrap_or_else(|| {
                    if point.selected {
                        viewport::selection_color(w)
                    } else {
                        egui::Color32::from_rgb(35, 120, 180)
                    }
                });
            painter.circle_filled(
                position,
                controls::number(&props, "handle-size", 3.5),
                color,
            );
        }
    }
}
pub fn pick_inactive(
    w: &Workspace,
    rect: egui::Rect,
    pointer: egui::Pos2,
) -> Option<crate::sketch::SketchHit> {
    let mut work = crate::sketch_overlay::DEPTH_WORK;
    inactive_points(w)
        .into_iter()
        .filter_map(|point| {
            let props = inactive_properties(w, &point);
            if !shown(&props)
                || (!crate::sketch_overlay::xray(w, true, &props)
                    && !viewport::visible_at(w, rect, point.position, &mut work))
            {
                return None;
            }
            let distance = viewport::project(w, rect, point.position)?.distance(pointer);
            (distance <= 8.).then_some((distance, point))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, point)| crate::sketch::SketchHit {
            definition: point.definition,
            feature: point.feature,
            path: point.path,
            instance: point.instance,
        })
}
pub fn toolbar(ui: &mut egui::Ui, w: &mut Workspace) {
    if !active(w) {
        return;
    }
    crate::sketch_overlay::visibility_controls(ui, w);
    for tool in [
        Tool::Select,
        Tool::Line,
        Tool::Point,
        Tool::Arc,
        Tool::Circle,
        Tool::Bezier,
        Tool::Nurbs,
        Tool::InterpolatingSpline,
        Tool::Dimension,
    ] {
        let selected = w.sketch3d.as_ref().is_some_and(|state| state.tool == tool);
        let element = action_element(
            w,
            &format!(
                "tool/{}",
                serde_json::to_value(tool)
                    .ok()
                    .and_then(|value| value.as_str().map(str::to_owned))
                    .unwrap_or_default()
            ),
            tool.label(),
            !captured(w),
        );
        if controls::button(
            ui,
            &element.id,
            "sketch3d-tool",
            &["command-button"],
            &element.label,
            element.state != "disabled",
            selected,
        )
        .clicked()
        {
            if tool == Tool::Dimension {
                action(w, ACTION, Some(&metadata_action(w, "smart-dimension")));
            } else {
                action(w, ACTION, Some(&element.data));
            }
        }
    }
    let plane = w
        .sketch3d
        .as_ref()
        .map_or(DrawingPlane::XY, |state| state.plane);
    let element = action_element(
        w,
        "cycle-plane",
        &format!("{} · Tab", plane.label()),
        !captured(w),
    );
    render_control(ui, w, &element);
}
fn metadata_action(w: &Workspace, kind: &str) -> Value {
    let mut data = metadata(w);
    data["action"] = json!(kind);
    data
}
