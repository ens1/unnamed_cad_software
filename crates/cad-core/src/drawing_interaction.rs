//! Disposable f64 paper interaction geometry and validated drawing-source actions.
//! Pixels, widgets and renderer/kernel objects never become saved drawing intent.
use crate::{
    CadError, Document, Drawing, DrawingDimension, Id, Result,
    drawing::{
        self, Anchor, AnnotationKind, CurveReference, DimensionKind, PointReference, Primitive,
        ViewLayout, ViewProjection,
    },
    protocol::Evaluation,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const CONTRACT_VERSION: u32 = 1;
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Target {
    View { key: String },
    Dimension { id: Id },
    Annotation { id: Id },
    Table { id: Id },
    TableSegment { table: Id, segment: Id },
    TitleCell { id: Id },
}
impl Target {
    pub fn identity(&self) -> String {
        match self {
            Self::View { key } => format!("view:{key}"),
            Self::Dimension { id } => format!("dimension:{id}"),
            Self::Annotation { id } => format!("annotation:{id}"),
            Self::Table { id } => format!("table:{id}"),
            Self::TableSegment { table, segment } => format!("table:{table}/segment:{segment}"),
            Self::TitleCell { id } => format!("title-cell:{id}"),
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PrimitiveGroup {
    pub target: Target,
    pub view: Option<String>,
    pub primitive_start: usize,
    pub primitive_count: usize,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AnchorSlot {
    First,
    Second,
    Third,
    Target,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HandleKind {
    Move,
    Resize { corner: u8 },
    DimensionOffset,
    Anchor { slot: AnchorSlot },
    LeaderBend { index: usize },
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Handle {
    pub id: String,
    pub kind: HandleKind,
    pub at_mm: [f64; 2],
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HitTarget {
    pub id: String,
    pub target: Target,
    pub view: Option<String>,
    pub bounds_mm: [[f64; 2]; 2],
    pub primitive_start: usize,
    pub primitive_count: usize,
    pub handles: Vec<Handle>,
    pub diagnostic: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReferenceCandidate {
    pub id: String,
    pub view: String,
    pub paper_mm: [f64; 2],
    pub model_point: [f64; 3],
    pub point: PointReference,
    pub curve: Option<CurveReference>,
    /// Exact world-space carrier, used solely for finding a pointer parameter.
    pub carrier: Option<crate::sketch_edit::Curve3>,
    pub projection: ViewProjection,
    /// Proven affine projected control hull, never a sampled mesh-fit bound.
    #[serde(default)]
    pub bounds_mm: Option<[[f64; 2]; 2]>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReferenceHit {
    pub id: String,
    pub view: String,
    pub paper_mm: [f64; 2],
    pub model_point: [f64; 3],
    pub point: PointReference,
    pub curve: Option<CurveReference>,
    pub distance_mm: f64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Scene {
    pub version: u32,
    pub sheet_id: Id,
    pub source_fingerprint: String,
    pub page: u32,
    pub targets: Vec<HitTarget>,
    pub references: Vec<ReferenceCandidate>,
    pub primitives: Vec<Primitive>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Snap {
    pub point: [f64; 2],
    pub guides: Vec<[[f64; 2]; 2]>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum Action {
    Move {
        target: Target,
        delta_mm: [f64; 2],
    },
    Resize {
        target: Target,
        rect_mm: [f64; 4],
    },
    DimensionOffset {
        id: Id,
        offset_mm: f64,
    },
    MoveLeaderBend {
        id: Id,
        index: usize,
        offset_mm: [f64; 2],
    },
    AddLeaderBend {
        id: Id,
        offset_mm: [f64; 2],
    },
    RemoveLeaderBend {
        id: Id,
        index: usize,
    },
    ReattachPoint {
        target: Target,
        slot: AnchorSlot,
        reference: PointReference,
    },
    ReattachCurve {
        target: Target,
        reference: CurveReference,
    },
    CreateDimension {
        view: String,
        kind: DimensionKind,
        first: Box<Anchor>,
        second: Box<Anchor>,
        curve: Option<CurveReference>,
        offset_mm: f64,
    },
}
fn invalid(message: &str) -> CadError {
    CadError::Invalid(format!("Drawing interaction: {message}"))
}
fn transform(point: [f64; 3], matrix: [[f64; 4]; 4]) -> [f64; 3] {
    std::array::from_fn(|row| {
        matrix[row][3] + (0..3).map(|col| matrix[row][col] * point[col]).sum::<f64>()
    })
}
fn finite(point: &[f64]) -> Result<()> {
    if point.iter().any(|v| !v.is_finite()) {
        return Err(invalid("Paper coordinates must be finite"));
    }
    Ok(())
}
fn expanded(mut bounds: [[f64; 2]; 2], pad: f64) -> [[f64; 2]; 2] {
    for coordinate in &mut bounds[0] {
        *coordinate -= pad;
    }
    for coordinate in &mut bounds[1] {
        *coordinate += pad;
    }
    bounds
}
fn bounds(primitives: &[Primitive]) -> Option<[[f64; 2]; 2]> {
    let mut result = [[f64::INFINITY; 2], [f64::NEG_INFINITY; 2]];
    let mut add = |point: [f64; 2]| {
        for axis in 0..2 {
            result[0][axis] = result[0][axis].min(point[axis]);
            result[1][axis] = result[1][axis].max(point[axis]);
        }
    };
    for primitive in primitives {
        match primitive {
            Primitive::Polyline { points, .. } => {
                for point in points {
                    add(*point);
                }
            }
            Primitive::Spline { curve } => {
                for point in &curve.control_points {
                    add(*point);
                }
            }
            Primitive::Circle { center, radius } => {
                add([center[0] - radius, center[1] - radius]);
                add([center[0] + radius, center[1] + radius]);
            }
            Primitive::Text { at, size, text } => {
                add([at[0], at[1] - size * 0.25]);
                add([
                    at[0] + text.chars().count() as f64 * size * 0.66,
                    at[1] + size,
                ]);
            }
        }
    }
    result[0][0].is_finite().then_some(result)
}
fn handle(target: &Target, kind: HandleKind, at_mm: [f64; 2]) -> Handle {
    Handle {
        id: format!(
            "{}/{}",
            target.identity(),
            serde_json::to_string(&kind).unwrap_or_default()
        ),
        kind,
        at_mm,
    }
}
fn resize_handles(target: &Target, bounds: [[f64; 2]; 2]) -> Vec<Handle> {
    (0..4)
        .map(|corner| {
            handle(
                target,
                HandleKind::Resize { corner },
                [
                    bounds[(corner & 1) as usize][0],
                    bounds[((corner >> 1) & 1) as usize][1],
                ],
            )
        })
        .collect()
}
fn annotation_anchors(annotation: &drawing::Annotation) -> Vec<(AnchorSlot, &Anchor)> {
    match &annotation.kind {
        AnnotationKind::SurfaceFinish { target, .. }
        | AnnotationKind::WeldSymbol { target, .. }
        | AnnotationKind::Leader { target, .. }
        | AnnotationKind::Datum { target, .. }
        | AnnotationKind::GeometricTolerance { target, .. }
        | AnnotationKind::Balloon { target, .. }
        | AnnotationKind::CutListBalloon { target, .. } => vec![(AnchorSlot::Target, target)],
        AnnotationKind::CenterLine { first, second, .. } => {
            vec![(AnchorSlot::First, first), (AnchorSlot::Second, second)]
        }
        AnnotationKind::CenterMark { .. } => vec![],
    }
}
fn leader_supported(annotation: &drawing::Annotation) -> bool {
    matches!(
        annotation.kind,
        AnnotationKind::Leader { .. }
            | AnnotationKind::Datum { .. }
            | AnnotationKind::GeometricTolerance { .. }
            | AnnotationKind::Balloon { .. }
            | AnnotationKind::CutListBalloon { .. }
    )
}
fn label_location(primitives: &[Primitive]) -> Option<[f64; 2]> {
    primitives.iter().find_map(|primitive| {
        if let Primitive::Text { at, .. } = primitive {
            Some(*at)
        } else {
            None
        }
    })
}
fn carrier_bounds(
    curve: &crate::sketch_edit::Curve3,
    projection: &ViewProjection,
    view: &str,
) -> Result<Option<[[f64; 2]; 2]>> {
    if curve.weights.len() != curve.control_points.len()
        || curve
            .weights
            .iter()
            .any(|weight| !weight.is_finite() || *weight <= 0.)
    {
        return Ok(None);
    }
    let mut min = [f64::INFINITY; 2];
    let mut max = [f64::NEG_INFINITY; 2];
    for point in &curve.control_points {
        let point = projection.paper_point(view, *point)?;
        for axis in 0..2 {
            min[axis] = min[axis].min(point[axis]);
            max[axis] = max[axis].max(point[axis]);
        }
    }
    Ok(min
        .iter()
        .chain(max.iter())
        .all(|value| value.is_finite())
        .then_some([min, max]))
}
/// Build selection/handles from the same valid vectors which are painted/exported.
pub fn build(
    doc: &Document,
    evaluation: &Evaluation,
    source: &Drawing,
    rendered: &drawing::Sheet,
    page: u32,
) -> Result<Scene> {
    if !evaluation.valid && !evaluation.geometry_valid {
        return Err(invalid("Wait for current valid model geometry"));
    }
    let expected_id = if page == 0 {
        source.id.clone()
    } else {
        format!("{}::continuation:{page}", source.id)
    };
    if rendered.id != expected_id {
        return Err(invalid(
            "Paper interaction belongs to another sheet or page",
        ));
    }
    let mut scene = Scene {
        version: CONTRACT_VERSION,
        sheet_id: source.id.clone(),
        source_fingerprint: format!("{:x}", Sha256::digest(serde_json::to_vec(source)?)),
        page,
        targets: vec![],
        references: vec![],
        primitives: rendered.primitives.clone(),
    };
    for group in &rendered.groups {
        let Some(primitives) = rendered.primitives.get(
            group.primitive_start..group.primitive_start.saturating_add(group.primitive_count),
        ) else {
            return Err(invalid("Stale drawing primitive group"));
        };
        let Some(mut extent) = bounds(primitives) else {
            continue;
        };
        let center = std::array::from_fn(|axis| (extent[0][axis] + extent[1][axis]) * 0.5);
        let mut handles = vec![];
        match &group.target {
            Target::View { key } => {
                let layout = rendered
                    .views
                    .get(key)
                    .ok_or_else(|| invalid("Missing projected view layout"))?;
                extent = [
                    [layout.rect_mm[0], layout.rect_mm[1]],
                    [
                        layout.rect_mm[0] + layout.rect_mm[2],
                        layout.rect_mm[1] + layout.rect_mm[3],
                    ],
                ];
                handles.push(handle(
                    &group.target,
                    HandleKind::Move,
                    [(extent[0][0] + extent[1][0]) * 0.5, extent[1][1] + 4.],
                ));
                handles.extend(resize_handles(&group.target, extent));
            }
            Target::Dimension { id } => {
                let dimension = source
                    .dimensions
                    .iter()
                    .find(|d| &d.id == id)
                    .ok_or_else(|| invalid("Missing dimension source"))?;
                handles.push(handle(
                    &group.target,
                    HandleKind::DimensionOffset,
                    label_location(primitives).unwrap_or(center),
                ));
                if let Some(projection) = rendered.projections.get(&dimension.view)
                    && !matches!(
                        dimension.options.kind,
                        DimensionKind::Diameter
                            | DimensionKind::Radius
                            | DimensionKind::HoleCallout
                    )
                {
                    for (slot, fallback, reference) in [
                        (AnchorSlot::First, dimension.first, &dimension.options.first),
                        (
                            AnchorSlot::Second,
                            dimension.second,
                            &dimension.options.second,
                        ),
                    ] {
                        let model = drawing::point(doc, evaluation, source, fallback, reference)
                            .or_else(|error| {
                                if rendered.diagnostics.contains_key(id) {
                                    Ok(fallback)
                                } else {
                                    Err(error)
                                }
                            })?;
                        handles.push(handle(
                            &group.target,
                            HandleKind::Anchor { slot },
                            projection.paper_point(&dimension.view, model)?,
                        ));
                    }
                    if dimension.options.kind == DimensionKind::Angular {
                        let model = drawing::point(
                            doc,
                            evaluation,
                            source,
                            dimension.options.third_point,
                            &dimension.options.third,
                        )
                        .or_else(|error| {
                            if rendered.diagnostics.contains_key(id) {
                                Ok(dimension.options.third_point)
                            } else {
                                Err(error)
                            }
                        })?;
                        handles.push(handle(
                            &group.target,
                            HandleKind::Anchor {
                                slot: AnchorSlot::Third,
                            },
                            projection.paper_point(&dimension.view, model)?,
                        ));
                    }
                }
            }
            Target::Annotation { id } => {
                let annotation = source
                    .entities
                    .iter()
                    .find(|a| &a.id == id)
                    .ok_or_else(|| invalid("Missing annotation source"))?;
                let projection = rendered
                    .projections
                    .get(&annotation.view)
                    .ok_or_else(|| invalid("Missing annotation projection"))?;
                if !matches!(
                    annotation.kind,
                    AnnotationKind::CenterMark { .. } | AnnotationKind::CenterLine { .. }
                ) {
                    handles.push(handle(
                        &group.target,
                        HandleKind::Move,
                        label_location(primitives).unwrap_or(center),
                    ));
                }
                for (slot, anchor) in annotation_anchors(annotation) {
                    let resolved = if let AnnotationKind::CutListBalloon { source: member, .. } =
                        &annotation.kind
                        && anchor.reference.is_none()
                    {
                        crate::drawing_tables::balloon_source_point(doc, evaluation, source, member)
                    } else {
                        drawing::point(doc, evaluation, source, anchor.point, &anchor.reference)
                    };
                    let model = resolved.or_else(|error| {
                        if rendered.diagnostics.contains_key(id) {
                            Ok(anchor.point)
                        } else {
                            Err(error)
                        }
                    })?;
                    let at = projection.paper_point(&annotation.view, model)?;
                    handles.push(handle(&group.target, HandleKind::Anchor { slot }, at));
                    if leader_supported(annotation) {
                        for (index, offset) in annotation.leader_points_mm.iter().enumerate() {
                            handles.push(handle(
                                &group.target,
                                HandleKind::LeaderBend { index },
                                [at[0] + offset[0], at[1] + offset[1]],
                            ));
                        }
                    }
                }
            }
            Target::Table { .. } | Target::TableSegment { .. } => {
                handles.push(handle(&group.target, HandleKind::Move, extent[1]))
            }
            Target::TitleCell { .. } => {}
        }
        scene.targets.push(HitTarget {
            id: format!("{}/{}", source.id, group.target.identity()),
            target: group.target.clone(),
            view: group.view.clone(),
            bounds_mm: extent,
            primitive_start: group.primitive_start,
            primitive_count: group.primitive_count,
            handles,
            diagnostic: match &group.target {
                Target::Dimension { id } | Target::Annotation { id } | Target::Table { id } => {
                    rendered.diagnostics.get(id).cloned()
                }
                Target::TableSegment { segment, .. } => rendered.diagnostics.get(segment).cloned(),
                Target::View { key } => rendered
                    .diagnostics
                    .get(&drawing::view_id(source, key))
                    .cloned(),
                _ => None,
            },
        });
    }
    if let Some(title) = drawing::title_bounds(source)
        && let Some(block) = source
            .format
            .as_ref()
            .and_then(|format| format.title_block.as_ref())
    {
        for cell in &block.cells {
            let rect = cell.rect_mm;
            let target = Target::TitleCell {
                id: cell.id.clone(),
            };
            let extent = [
                [title[0][0] + rect[0], title[0][1] + rect[1]],
                [
                    title[0][0] + rect[0] + rect[2],
                    title[0][1] + rect[1] + rect[3],
                ],
            ];
            let mut handles = resize_handles(&target, extent);
            handles.push(handle(
                &target,
                HandleKind::Move,
                std::array::from_fn(|axis| (extent[0][axis] + extent[1][axis]) * 0.5),
            ));
            scene.targets.push(HitTarget {
                id: format!("{}/{}", source.id, target.identity()),
                target,
                view: None,
                bounds_mm: extent,
                primitive_start: 0,
                primitive_count: 0,
                handles,
                diagnostic: None,
            });
        }
    }
    if page == 0 {
        for instance in drawing::sheet_instances(doc, evaluation, source)? {
            let key = if instance.mesh_key.is_empty() {
                instance.configuration.as_ref().map_or_else(
                    || instance.definition.clone(),
                    |id| format!("{}@{id}", instance.definition),
                )
            } else {
                instance.mesh_key.clone()
            };
            let Some(mesh) = evaluation.meshes.get(&key) else {
                continue;
            };
            for (view, projection) in &rendered.projections {
                // Tessellated section intersection lines have no source B-rep edge identity.
                if view == "section-z" {
                    continue;
                }
                for edge in &mesh.edges {
                    let Some(curve) = &edge.curve else {
                        continue;
                    };
                    let carrier = crate::sketch_edit::Curve3 {
                        control_points: curve
                            .control_points
                            .iter()
                            .map(|point| transform(*point, instance.matrix))
                            .collect(),
                        ..curve.clone()
                    };
                    let curve_reference = CurveReference::ModelEdge {
                        edge: edge.reference.clone(),
                        body: edge.body.clone(),
                        instance_path: instance.path.clone(),
                    };
                    if let Ok(circle) = drawing::circular(doc, evaluation, source, &curve_reference)
                    {
                        let paper = projection.paper_point(view, circle.center)?;
                        scene.references.push(ReferenceCandidate {
                            id: format!(
                                "{view}/{}/center/{}",
                                instance.path.join("/"),
                                serde_json::to_string(&edge.reference)?
                            ),
                            view: view.clone(),
                            paper_mm: paper,
                            model_point: circle.center,
                            point: PointReference::CurveCenter {
                                curve: Box::new(curve_reference.clone()),
                            },
                            curve: Some(curve_reference.clone()),
                            carrier: None,
                            projection: projection.clone(),
                            bounds_mm: Some([paper, paper]),
                        });
                    }
                    for parameter in [0., 0.5, 1.] {
                        let model = drawing::curve_point(&carrier, parameter)?;
                        let paper = projection.paper_point(view, model)?;
                        let point = PointReference::EdgePoint {
                            edge: edge.reference.clone(),
                            body: edge.body.clone(),
                            parameter,
                            instance_path: instance.path.clone(),
                        };
                        let id = format!(
                            "{view}/{}/{}/{parameter}",
                            instance.path.join("/"),
                            serde_json::to_string(&edge.reference)?
                        );
                        scene.references.push(ReferenceCandidate {
                            id,
                            view: view.clone(),
                            paper_mm: paper,
                            model_point: model,
                            point,
                            curve: Some(curve_reference.clone()),
                            carrier: (parameter == 0.5).then(|| carrier.clone()),
                            projection: projection.clone(),
                            bounds_mm: if parameter == 0.5 {
                                carrier_bounds(&carrier, projection, view)?
                            } else {
                                Some([paper, paper])
                            },
                        });
                    }
                }
                for (sketch_key, sketch) in &evaluation.sketches {
                    let Some(node) = sketch_key.strip_prefix(&format!("{key}::")) else {
                        continue;
                    };
                    let suppressed = sketch
                        .solved
                        .as_ref()
                        .map(crate::sketch_transform::suppressed_points)
                        .unwrap_or_default();
                    for (point_id, position) in &sketch.points {
                        if suppressed.contains(point_id) {
                            continue;
                        }
                        let local = std::array::from_fn(|axis| {
                            sketch.plane.origin[axis]
                                + sketch.plane.u[axis] * position[0]
                                + sketch.plane.v[axis] * position[1]
                        });
                        let model = transform(local, instance.matrix);
                        let paper = projection.paper_point(view, model)?;
                        scene.references.push(ReferenceCandidate {
                            id: format!("{view}/{}/{node}/{point_id}", instance.path.join("/")),
                            view: view.clone(),
                            paper_mm: paper,
                            model_point: model,
                            point: PointReference::SketchPoint {
                                sketch: crate::NodeRef {
                                    definition: instance.definition.clone(),
                                    node: node.into(),
                                },
                                point: point_id.clone(),
                                instance_path: instance.path.clone(),
                            },
                            curve: None,
                            carrier: None,
                            projection: projection.clone(),
                            bounds_mm: Some([paper, paper]),
                        });
                    }
                }
            }
        }
    }
    Ok(scene)
}
fn contains(bounds: [[f64; 2]; 2], point: [f64; 2]) -> bool {
    (0..2).all(|axis| point[axis] >= bounds[0][axis] && point[axis] <= bounds[1][axis])
}
fn segment_distance(point: [f64; 2], a: [f64; 2], b: [f64; 2]) -> f64 {
    let delta = [b[0] - a[0], b[1] - a[1]];
    let length = delta[0] * delta[0] + delta[1] * delta[1];
    let t = if length > 1e-20 {
        ((point[0] - a[0]) * delta[0] + (point[1] - a[1]) * delta[1]) / length
    } else {
        0.
    }
    .clamp(0., 1.);
    (point[0] - a[0] - delta[0] * t).hypot(point[1] - a[1] - delta[1] * t)
}
fn primitive_distance(point: [f64; 2], primitive: &Primitive) -> f64 {
    match primitive {
        Primitive::Polyline { points, .. } => points
            .windows(2)
            .map(|pair| segment_distance(point, pair[0], pair[1]))
            .fold(f64::INFINITY, f64::min),
        Primitive::Spline { curve } => {
            drawing::display_curve(curve)
                .ok()
                .map_or(f64::INFINITY, |points| {
                    points
                        .windows(2)
                        .map(|pair| segment_distance(point, pair[0], pair[1]))
                        .fold(f64::INFINITY, f64::min)
                })
        }
        Primitive::Circle { center, radius } => {
            ((point[0] - center[0]).hypot(point[1] - center[1]) - radius).abs()
        }
        Primitive::Text { .. } => {
            if bounds(std::slice::from_ref(primitive))
                .is_some_and(|bounds| contains(expanded(bounds, 0.5), point))
            {
                0.
            } else {
                f64::INFINITY
            }
        }
    }
}
/// Prefer actual annotation strokes/labels over a view's broad paper rectangle.
pub fn hit(scene: &Scene, paper_mm: [f64; 2], tolerance_mm: f64) -> Option<&HitTarget> {
    scene
        .targets
        .iter()
        .filter(|target| contains(expanded(target.bounds_mm, tolerance_mm), paper_mm))
        .filter_map(|target| {
            let distance = if matches!(
                target.target,
                Target::View { .. }
                    | Target::Table { .. }
                    | Target::TableSegment { .. }
                    | Target::TitleCell { .. }
            ) {
                0.
            } else {
                scene
                    .primitives
                    .get(target.primitive_start..target.primitive_start + target.primitive_count)?
                    .iter()
                    .map(|primitive| primitive_distance(paper_mm, primitive))
                    .fold(f64::INFINITY, f64::min)
            };
            (distance <= tolerance_mm).then_some((target, distance))
        })
        .min_by(|(a, ad), (b, bd)| {
            let priority = |target: &HitTarget| {
                if matches!(target.target, Target::View { .. }) {
                    2
                } else if matches!(
                    target.target,
                    Target::TitleCell { .. } | Target::Table { .. } | Target::TableSegment { .. }
                ) {
                    1
                } else {
                    0
                }
            };
            priority(a).cmp(&priority(b)).then(ad.total_cmp(bd))
        })
        .map(|(target, _)| target)
}
fn visible(candidate: &ReferenceCandidate, point: [f64; 2]) -> bool {
    let rect = candidate.projection.rect_mm;
    contains(
        [[rect[0], rect[1]], [rect[0] + rect[2], rect[1] + rect[3]]],
        point,
    )
}
/// Pointer proximity selects a parameter; the resulting anchor remains an exact
/// evaluated rational curve point with a stable topology reference.
pub fn nearest_reference(
    scene: &Scene,
    paper_mm: [f64; 2],
    tolerance_mm: f64,
    view: Option<&str>,
) -> Option<ReferenceHit> {
    if finite(&paper_mm).is_err() || !tolerance_mm.is_finite() || tolerance_mm <= 0. {
        return None;
    }
    let mut best: Option<ReferenceHit> = None;
    for candidate in &scene.references {
        if view.is_some_and(|view| view != candidate.view) {
            continue;
        }
        if candidate
            .bounds_mm
            .is_some_and(|bounds| !contains(expanded(bounds, tolerance_mm), paper_mm))
        {
            continue;
        }
        let mut model = candidate.model_point;
        let mut paper = candidate.paper_mm;
        let mut point = candidate.point.clone();
        if let Some(carrier) = &candidate.carrier {
            let projected = |t| {
                drawing::curve_point(carrier, t)
                    .and_then(|point| candidate.projection.paper_point(&candidate.view, point))
            };
            let distance = |t| {
                projected(t).map_or(f64::INFINITY, |p| {
                    (p[0] - paper_mm[0]).powi(2) + (p[1] - paper_mm[1]).powi(2)
                })
            };
            let count = (carrier.control_points.len() * 8).clamp(32, 512);
            let index = (0..=count).min_by(|a, b| {
                distance(*a as f64 / count as f64).total_cmp(&distance(*b as f64 / count as f64))
            })?;
            let mut low = index.saturating_sub(1) as f64 / count as f64;
            let mut high = (index + 1).min(count) as f64 / count as f64;
            for _ in 0..24 {
                let left = low + (high - low) / 3.;
                let right = high - (high - low) / 3.;
                if distance(left) < distance(right) {
                    high = right;
                } else {
                    low = left;
                }
            }
            let parameter = (low + high) * 0.5;
            model = drawing::curve_point(carrier, parameter).ok()?;
            paper = candidate
                .projection
                .paper_point(&candidate.view, model)
                .ok()?;
            if let PointReference::EdgePoint {
                parameter: saved, ..
            } = &mut point
            {
                *saved = parameter;
            }
        }
        if !visible(candidate, paper) {
            continue;
        }
        let distance_mm = (paper[0] - paper_mm[0]).hypot(paper[1] - paper_mm[1]);
        if distance_mm > tolerance_mm
            || best
                .as_ref()
                .is_some_and(|best| best.distance_mm <= distance_mm)
        {
            continue;
        }
        best = Some(ReferenceHit {
            id: candidate.id.clone(),
            view: candidate.view.clone(),
            paper_mm: paper,
            model_point: model,
            point,
            curve: candidate.curve.clone(),
            distance_mm,
        });
    }
    best
}
/// Grid and peer bounds/center snapping are paper-layout aids, not geometry constraints.
pub fn snap(
    scene: &Scene,
    target: &Target,
    paper_mm: [f64; 2],
    tolerance_mm: f64,
    grid_mm: Option<f64>,
) -> Snap {
    let mut result = Snap {
        point: paper_mm,
        guides: vec![],
    };
    if finite(&paper_mm).is_err() || !tolerance_mm.is_finite() || tolerance_mm <= 0. {
        return result;
    }
    for axis in 0..2 {
        let mut nearest = grid_mm
            .filter(|grid| grid.is_finite() && *grid > 0.)
            .map(|grid| (paper_mm[axis] / grid).round() * grid);
        let mut peer = None;
        for other in &scene.targets {
            if &other.target == target {
                continue;
            }
            for at in [
                other.bounds_mm[0][axis],
                other.bounds_mm[1][axis],
                (other.bounds_mm[0][axis] + other.bounds_mm[1][axis]) * 0.5,
            ] {
                if nearest.is_none_or(|current| {
                    (at - paper_mm[axis]).abs() < (current - paper_mm[axis]).abs()
                }) {
                    nearest = Some(at);
                    peer = Some(other.bounds_mm);
                }
            }
        }
        if let Some(at) = nearest
            && (at - paper_mm[axis]).abs() <= tolerance_mm
        {
            result.point[axis] = at;
            if let Some(bounds) = peer {
                let mut a = paper_mm;
                let mut b = paper_mm;
                a[axis] = at;
                b[axis] = at;
                a[1 - axis] = a[1 - axis].min(bounds[0][1 - axis]);
                b[1 - axis] = b[1 - axis].max(bounds[1][1 - axis]);
                result.guides.push([a, b]);
            }
        }
    }
    result
}
fn dimension_axis(
    doc: &Document,
    evaluation: &Evaluation,
    sheet: &Drawing,
    projection: &ViewProjection,
    dimension: &DrawingDimension,
) -> Result<[f64; 2]> {
    if dimension.options.kind == DimensionKind::Horizontal {
        return Ok([0., 1.]);
    }
    if dimension.options.kind == DimensionKind::Vertical {
        return Ok([1., 0.]);
    }
    if dimension.options.kind == DimensionKind::HoleCallout {
        return Ok([0.5, 0.5]);
    }
    let (a, b) = if let Some(curve) = &dimension.options.curve
        && matches!(
            dimension.options.kind,
            DimensionKind::Diameter | DimensionKind::Radius
        ) {
        let circle = drawing::circular(doc, evaluation, sheet, curve)?;
        (
            projection.paper_point(&dimension.view, circle.center)?,
            projection.paper_point(&dimension.view, circle.start)?,
        )
    } else {
        (
            projection.paper_point(
                &dimension.view,
                drawing::point(
                    doc,
                    evaluation,
                    sheet,
                    dimension.first,
                    &dimension.options.first,
                )?,
            )?,
            projection.paper_point(
                &dimension.view,
                drawing::point(
                    doc,
                    evaluation,
                    sheet,
                    dimension.second,
                    &dimension.options.second,
                )?,
            )?,
        )
    };
    let delta = [b[0] - a[0], b[1] - a[1]];
    let length = delta[0].hypot(delta[1]);
    if length < 1e-9 {
        return Err(invalid(
            "This dimension collapses in its current projection",
        ));
    }
    if dimension.options.kind == DimensionKind::Angular {
        let third = projection.paper_point(
            &dimension.view,
            drawing::point(
                doc,
                evaluation,
                sheet,
                dimension.options.third_point,
                &dimension.options.third,
            )?,
        )?;
        let start = delta[1].atan2(delta[0]);
        let end = (third[1] - a[1]).atan2(third[0] - a[0]);
        let mut sweep = (end - start).rem_euclid(std::f64::consts::TAU);
        if sweep > std::f64::consts::PI {
            sweep -= std::f64::consts::TAU;
        }
        let angle = start + sweep * 0.5;
        let sign = if dimension.offset_mm < 0. { -1. } else { 1. };
        return Ok([angle.cos() * sign, angle.sin() * sign]);
    }
    Ok([-delta[1] / length, delta[0] / length])
}
fn annotation_mut<'a>(sheet: &'a mut Drawing, id: &str) -> Result<&'a mut drawing::Annotation> {
    sheet
        .entities
        .iter_mut()
        .find(|annotation| annotation.id == id)
        .ok_or_else(|| invalid("The selected annotation no longer exists"))
}
fn anchor_mut<'a>(
    annotation: &'a mut drawing::Annotation,
    slot: &AnchorSlot,
) -> Result<&'a mut Anchor> {
    match (&mut annotation.kind, slot) {
        (
            AnnotationKind::Leader { target, .. }
            | AnnotationKind::Datum { target, .. }
            | AnnotationKind::Balloon { target, .. }
            | AnnotationKind::CutListBalloon { target, .. }
            | AnnotationKind::SurfaceFinish { target, .. }
            | AnnotationKind::WeldSymbol { target, .. }
            | AnnotationKind::GeometricTolerance { target, .. },
            AnchorSlot::Target,
        ) => Ok(target),
        (AnnotationKind::CenterLine { first, .. }, AnchorSlot::First) => Ok(first),
        (AnnotationKind::CenterLine { second, .. }, AnchorSlot::Second) => Ok(second),
        _ => Err(invalid("This annotation has no selected point attachment")),
    }
}
fn diagnostic_position(
    doc: &Document,
    evaluation: &Evaluation,
    source: &Drawing,
    page: u32,
    target: &Target,
) -> Result<Option<[f64; 2]>> {
    let key = match target {
        Target::Table { id } => id,
        Target::TableSegment { segment, .. } => segment,
        _ => return Ok(None),
    };
    let rendered = drawing::build_sheet_preview(doc, evaluation, source, page)?;
    if !rendered.diagnostics.contains_key(key) {
        return Ok(None);
    }
    let group = rendered
        .groups
        .iter()
        .find(|group| &group.target == target)
        .ok_or_else(|| invalid("The diagnostic table is no longer on this page"))?;
    let primitives = rendered
        .primitives
        .get(group.primitive_start..group.primitive_start + group.primitive_count)
        .ok_or_else(|| invalid("Diagnostic table geometry is stale"))?;
    let extent =
        bounds(primitives).ok_or_else(|| invalid("The diagnostic table has no paper handles"))?;
    Ok(Some(extent[0]))
}
/// Apply an edit to a clone, preserving source IDs and the caller's one-operation
/// draft/checkpoint. No partial source mutation survives validation failure.
pub fn apply(
    doc: &Document,
    evaluation: &Evaluation,
    source: &Drawing,
    page: u32,
    action: &Action,
) -> Result<Drawing> {
    if !evaluation.valid && !evaluation.geometry_valid {
        return Err(invalid(
            "Paper source actions require complete current native geometry",
        ));
    }
    let mut sheet = source.clone();
    match action {
        Action::Move { target, delta_mm } => {
            finite(delta_mm)?;
            match target {
                Target::View { key } => {
                    let rendered = drawing::build_sheet_preview(doc, evaluation, source, 0)?;
                    let actual = rendered
                        .views
                        .get(key)
                        .ok_or_else(|| invalid("The view no longer exists"))?;
                    let layout =
                        sheet
                            .view_layouts
                            .entry(key.clone())
                            .or_insert_with(|| ViewLayout {
                                scale: None,
                                ..actual.clone()
                            });
                    for (axis, delta) in delta_mm.iter().enumerate() {
                        layout.rect_mm[axis] = actual.rect_mm[axis] + delta;
                    }
                }
                Target::Dimension { id } => {
                    let rendered = drawing::build_sheet_preview(doc, evaluation, source, 0)?;
                    let dimension = source
                        .dimensions
                        .iter()
                        .find(|dimension| &dimension.id == id)
                        .ok_or_else(|| invalid("The dimension no longer exists"))?;
                    let projection = rendered
                        .projections
                        .get(&dimension.view)
                        .ok_or_else(|| invalid("Missing projected dimension view"))?;
                    let axis = dimension_axis(doc, evaluation, source, projection, dimension)?;
                    let saved = sheet
                        .dimensions
                        .iter_mut()
                        .find(|dimension| &dimension.id == id)
                        .ok_or_else(|| invalid("Missing dimension"))?;
                    saved.offset_mm += delta_mm[0] * axis[0] + delta_mm[1] * axis[1];
                }
                Target::Annotation { id } => {
                    let annotation = annotation_mut(&mut sheet, id)?;
                    if matches!(
                        annotation.kind,
                        AnnotationKind::CenterMark { .. } | AnnotationKind::CenterLine { .. }
                    ) {
                        return Err(invalid(
                            "Center geometry stays attached to its source; reattach its reference to move it",
                        ));
                    }
                    for (axis, delta) in delta_mm.iter().enumerate() {
                        annotation.offset_mm[axis] += delta;
                    }
                }
                Target::Table { id } => {
                    let position = diagnostic_position(doc, evaluation, source, page, target)?;
                    let table = sheet
                        .tables
                        .iter_mut()
                        .find(|table| &table.id == id)
                        .ok_or_else(|| invalid("The table no longer exists"))?;
                    let position = position.unwrap_or(table.position_mm);
                    for (axis, delta) in delta_mm.iter().enumerate() {
                        table.position_mm[axis] = position[axis] + delta;
                    }
                }
                Target::TableSegment { table, segment } => {
                    let position = diagnostic_position(doc, evaluation, source, page, target)?;
                    let table = sheet
                        .tables
                        .iter_mut()
                        .find(|item| &item.id == table)
                        .ok_or_else(|| invalid("The table no longer exists"))?;
                    let segment = table
                        .segments
                        .iter_mut()
                        .find(|item| &item.id == segment && item.page == page)
                        .ok_or_else(|| {
                            invalid("The selected table segment is no longer on this page")
                        })?;
                    let position = position.unwrap_or(segment.position_mm);
                    for (axis, delta) in delta_mm.iter().enumerate() {
                        segment.position_mm[axis] = position[axis] + delta;
                    }
                }
                Target::TitleCell { id } => {
                    let cell = sheet
                        .format
                        .as_mut()
                        .and_then(|format| format.title_block.as_mut())
                        .and_then(|block| block.cells.iter_mut().find(|cell| &cell.id == id))
                        .ok_or_else(|| invalid("The title cell no longer exists"))?;
                    for (axis, delta) in delta_mm.iter().enumerate() {
                        cell.rect_mm[axis] += delta;
                    }
                }
            }
        }
        Action::Resize { target, rect_mm } => {
            finite(rect_mm)?;
            match target {
                Target::View { key } => {
                    let rendered = drawing::build_sheet_preview(doc, evaluation, source, 0)?;
                    let actual = rendered
                        .views
                        .get(key)
                        .ok_or_else(|| invalid("The view no longer exists"))?;
                    let layout =
                        sheet
                            .view_layouts
                            .entry(key.clone())
                            .or_insert_with(|| ViewLayout {
                                scale: None,
                                ..actual.clone()
                            });
                    layout.rect_mm = *rect_mm;
                }
                Target::TitleCell { id } => {
                    let origin = drawing::title_bounds(source)
                        .ok_or_else(|| invalid("The sheet has no title block"))?[0];
                    let cell = sheet
                        .format
                        .as_mut()
                        .and_then(|format| format.title_block.as_mut())
                        .and_then(|block| block.cells.iter_mut().find(|cell| &cell.id == id))
                        .ok_or_else(|| invalid("The title cell no longer exists"))?;
                    cell.rect_mm = [
                        rect_mm[0] - origin[0],
                        rect_mm[1] - origin[1],
                        rect_mm[2],
                        rect_mm[3],
                    ];
                }
                _ => {
                    return Err(invalid(
                        "Only view rectangles and title cells have resize handles",
                    ));
                }
            }
        }
        Action::DimensionOffset { id, offset_mm } => {
            finite(&[*offset_mm])?;
            sheet
                .dimensions
                .iter_mut()
                .find(|dimension| &dimension.id == id)
                .ok_or_else(|| invalid("The dimension no longer exists"))?
                .offset_mm = *offset_mm;
        }
        Action::MoveLeaderBend {
            id,
            index,
            offset_mm,
        } => {
            finite(offset_mm)?;
            let annotation = annotation_mut(&mut sheet, id)?;
            if !leader_supported(annotation) {
                return Err(invalid("This symbol does not support an elbow leader"));
            }
            *annotation
                .leader_points_mm
                .get_mut(*index)
                .ok_or_else(|| invalid("The selected leader elbow no longer exists"))? = *offset_mm;
        }
        Action::AddLeaderBend { id, offset_mm } => {
            finite(offset_mm)?;
            let annotation = annotation_mut(&mut sheet, id)?;
            if !leader_supported(annotation) || annotation.leader_points_mm.len() >= 64 {
                return Err(invalid(
                    "This annotation cannot accept another elbow leader",
                ));
            }
            annotation.leader_points_mm.push(*offset_mm);
        }
        Action::RemoveLeaderBend { id, index } => {
            let annotation = annotation_mut(&mut sheet, id)?;
            if *index >= annotation.leader_points_mm.len() {
                return Err(invalid("The selected leader elbow no longer exists"));
            }
            annotation.leader_points_mm.remove(*index);
        }
        Action::ReattachPoint {
            target,
            slot,
            reference,
        } => {
            let model = drawing::point(doc, evaluation, source, [0.; 3], &Some(reference.clone()))?;
            match target {
                Target::Dimension { id } => {
                    let dimension = sheet
                        .dimensions
                        .iter_mut()
                        .find(|dimension| &dimension.id == id)
                        .ok_or_else(|| invalid("The dimension no longer exists"))?;
                    match slot {
                        AnchorSlot::First => {
                            dimension.first = model;
                            dimension.options.first = Some(reference.clone());
                        }
                        AnchorSlot::Second => {
                            dimension.second = model;
                            dimension.options.second = Some(reference.clone());
                        }
                        AnchorSlot::Third => {
                            dimension.options.third_point = model;
                            dimension.options.third = Some(reference.clone());
                        }
                        AnchorSlot::Target => {
                            return Err(invalid(
                                "Dimensions need a first, second or third reference slot",
                            ));
                        }
                    }
                }
                Target::Annotation { id } => {
                    let anchor = anchor_mut(annotation_mut(&mut sheet, id)?, slot)?;
                    anchor.point = model;
                    anchor.reference = Some(reference.clone());
                }
                _ => {
                    return Err(invalid(
                        "Only dimensions and annotations attach to model references",
                    ));
                }
            }
        }
        Action::ReattachCurve { target, reference } => {
            // Reattachment is accepted only when the referenced carrier proves circular.
            drawing::circular(doc, evaluation, source, reference)?;
            match target {
                Target::Dimension { id } => {
                    let dimension = sheet
                        .dimensions
                        .iter_mut()
                        .find(|dimension| &dimension.id == id)
                        .ok_or_else(|| invalid("The dimension no longer exists"))?;
                    if !matches!(
                        dimension.options.kind,
                        DimensionKind::Diameter
                            | DimensionKind::Radius
                            | DimensionKind::HoleCallout
                    ) {
                        return Err(invalid("This dimension uses point attachments"));
                    }
                    dimension.options.curve = Some(reference.clone());
                }
                Target::Annotation { id } => {
                    let AnnotationKind::CenterMark { curve, .. } =
                        &mut annotation_mut(&mut sheet, id)?.kind
                    else {
                        return Err(invalid(
                            "This annotation has no circular carrier attachment",
                        ));
                    };
                    *curve = reference.clone();
                }
                _ => {
                    return Err(invalid(
                        "Only circular dimensions and center marks attach to curves",
                    ));
                }
            }
        }
        Action::CreateDimension {
            view,
            kind,
            first,
            second,
            curve,
            offset_mm,
        } => {
            finite(&[*offset_mm])?;
            if page != 0 || !drawing::view_keys(source).contains(view) {
                return Err(invalid(
                    "Create dimensions on an existing model view on the first paper page",
                ));
            }
            if matches!(kind, DimensionKind::Angular | DimensionKind::HoleCallout) {
                return Err(invalid(
                    "Angular and hole dimensions require their dedicated additional source controls",
                ));
            }
            if matches!(kind, DimensionKind::Diameter | DimensionKind::Radius) {
                drawing::circular(
                    doc,
                    evaluation,
                    source,
                    curve
                        .as_ref()
                        .ok_or_else(|| invalid("Select an exact circular model edge"))?,
                )?;
            }
            let first_point =
                drawing::point(doc, evaluation, source, first.point, &first.reference)?;
            let second_point =
                drawing::point(doc, evaluation, source, second.point, &second.reference)?;
            sheet.dimensions.push(DrawingDimension {
                id: crate::id(),
                view: view.clone(),
                first: first_point,
                second: second_point,
                offset_mm: *offset_mm,
                parameter: None,
                tolerance: String::new(),
                options: drawing::DimensionOptions {
                    kind: kind.clone(),
                    first: first.reference.clone(),
                    second: second.reference.clone(),
                    curve: curve.clone(),
                    ..Default::default()
                },
            });
        }
    }
    drawing::validate_definition(&sheet)?;
    // Every gesture may repair one item while another still has a drawing
    // diagnostic. Native reference proofs above stay strict; the worker must
    // validate the complete drawing before final Apply or production export.
    drawing::build_sheet_preview(doc, evaluation, &sheet, page)?;
    Ok(sheet)
}
