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

pub const CONTRACT_VERSION: u32 = 5;
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
    GroupTarget { target: Id },
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HandleKind {
    Move,
    Resize { corner: u8 },
    DimensionOffset,
    Anchor { slot: AnchorSlot },
    LeaderBend { index: usize },
    GroupLabel { target: Id },
    CloudCorner { corner: u8 },
    CloudVertex { vertex: Id },
    CloudLabel,
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
    TranslateAnnotations {
        annotations: Vec<Id>,
        delta_mm: [f64; 2],
    },
    ArrangeAnnotations {
        request: Box<crate::drawing_layout::Request>,
    },
    CopyAnnotations {
        annotations: Vec<Id>,
        offset_mm: [f64; 2],
    },
    DeleteAnnotations {
        annotations: Vec<Id>,
    },
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
    MoveGroupLabel {
        id: Id,
        target: Id,
        offset_mm: [f64; 2],
    },
    ResizeCloudCorner {
        id: Id,
        corner: u8,
        delta_mm: [f64; 2],
    },
    MoveCloudVertex {
        id: Id,
        vertex: Id,
        point_mm: [f64; 2],
    },
    MoveCloudLabel {
        id: Id,
        offset_mm: [f64; 2],
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
        curve: Option<Box<CurveReference>>,
        #[serde(default)]
        second_curve: Option<Box<CurveReference>>,
        offset_mm: f64,
    },
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DimensionInference {
    pub kind: DimensionKind,
    pub edge_separation: bool,
}
/// Measurement intent belongs to the native drawing contract, not a widget.
pub fn infer_dimension(
    doc: &Document,
    evaluation: &Evaluation,
    source: &Drawing,
    first: &ReferenceHit,
    second: &ReferenceHit,
) -> DimensionInference {
    if first.view == second.view
        && matches!(first.point, PointReference::EdgePoint { .. })
        && matches!(second.point, PointReference::EdgePoint { .. })
        && let (Some(a), Some(b)) = (&first.curve, &second.curve)
        && let Ok((direction, _, _)) =
            drawing::parallel_lines(doc, evaluation, source, &first.view, a, b)
    {
        return DimensionInference {
            kind: if direction[0].abs() < 1e-8 {
                DimensionKind::Horizontal
            } else if direction[1].abs() < 1e-8 {
                DimensionKind::Vertical
            } else {
                DimensionKind::Perpendicular
            },
            edge_separation: true,
        };
    }
    let delta = [
        second.paper_mm[0] - first.paper_mm[0],
        second.paper_mm[1] - first.paper_mm[1],
    ];
    let tolerance = delta[0].hypot(delta[1]) * 1e-8;
    DimensionInference {
        kind: if delta[1].abs() <= tolerance && delta[0].abs() > tolerance {
            DimensionKind::Horizontal
        } else if delta[0].abs() <= tolerance && delta[1].abs() > tolerance {
            DimensionKind::Vertical
        } else {
            DimensionKind::Aligned
        },
        edge_separation: false,
    }
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
            Primitive::Polyline { points, .. } | Primitive::HiddenPolyline { points } => {
                for point in points {
                    add(*point);
                }
            }
            Primitive::Spline { curve, .. } | Primitive::HiddenSpline { curve } => {
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
pub(crate) fn annotation_anchors(annotation: &drawing::Annotation) -> Vec<(AnchorSlot, &Anchor)> {
    match &annotation.kind {
        AnnotationKind::DimensionGroup { group } => {
            std::iter::once((AnchorSlot::Target, &group.datum))
                .chain(group.targets.iter().map(|target| {
                    (
                        AnchorSlot::GroupTarget {
                            target: target.id.clone(),
                        },
                        &target.anchor,
                    )
                }))
                .collect()
        }
        AnnotationKind::SurfaceFinish { target, .. }
        | AnnotationKind::WeldSymbol { target, .. }
        | AnnotationKind::Leader { target, .. }
        | AnnotationKind::Datum { target, .. }
        | AnnotationKind::GeometricTolerance { target, .. }
        | AnnotationKind::DatumTarget { target, .. }
        | AnnotationKind::GeometricToleranceFrame { target, .. }
        | AnnotationKind::RevisionCloud { target, .. }
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
            | AnnotationKind::DatumTarget { .. }
            | AnnotationKind::GeometricToleranceFrame { .. }
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
                    let separation = if dimension.options.second_curve.is_some() {
                        drawing::edge_separation_points(doc, evaluation, source, dimension).ok()
                    } else {
                        None
                    };
                    for (slot, fallback, reference) in [
                        (AnchorSlot::First, dimension.first, &dimension.options.first),
                        (
                            AnchorSlot::Second,
                            dimension.second,
                            &dimension.options.second,
                        ),
                    ] {
                        if let Some((a, b)) = separation {
                            handles.push(handle(
                                &group.target,
                                HandleKind::Anchor { slot: slot.clone() },
                                projection.paper_projected(if slot == AnchorSlot::First {
                                    a
                                } else {
                                    b
                                }),
                            ));
                            continue;
                        }
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
                let cloud_center = if let AnnotationKind::RevisionCloud { target, .. } =
                    &annotation.kind
                {
                    let point =
                        drawing::point(doc, evaluation, source, target.point, &target.reference)
                            .or_else(|error| {
                                if rendered.diagnostics.contains_key(id) {
                                    Ok(target.point)
                                } else {
                                    Err(error)
                                }
                            })?;
                    let at = projection.paper_point(&annotation.view, point)?;
                    Some([
                        at[0] + annotation.offset_mm[0],
                        at[1] + annotation.offset_mm[1],
                    ])
                } else {
                    None
                };
                if !matches!(
                    annotation.kind,
                    AnnotationKind::CenterMark { .. } | AnnotationKind::CenterLine { .. }
                ) {
                    handles.push(handle(
                        &group.target,
                        HandleKind::Move,
                        cloud_center
                            .unwrap_or_else(|| label_location(primitives).unwrap_or(center)),
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
                if let AnnotationKind::DimensionGroup {
                    group: dimension_group,
                } = &annotation.kind
                    && let Ok(layout) = crate::drawing_dimension_groups::layout(
                        doc,
                        evaluation,
                        source,
                        &annotation.view,
                        projection,
                        dimension_group,
                        annotation.offset_mm,
                    )
                {
                    for target in layout.targets {
                        handles.push(handle(
                            &group.target,
                            HandleKind::GroupLabel {
                                target: target.target,
                            },
                            target.label_mm,
                        ));
                    }
                }
                if let AnnotationKind::RevisionCloud { cloud, .. } = &annotation.kind
                    && let Some(center) = cloud_center
                {
                    if cloud.shape == crate::drawing_revision::Shape::Polygon {
                        for vertex in &cloud.vertices {
                            handles.push(handle(
                                &group.target,
                                HandleKind::CloudVertex {
                                    vertex: vertex.id.clone(),
                                },
                                cloud.paper_point(vertex.point_mm, center),
                            ));
                        }
                    } else {
                        for corner in 0..4 {
                            let local = [
                                if corner & 1 == 0 {
                                    -cloud.width_mm * 0.5
                                } else {
                                    cloud.width_mm * 0.5
                                },
                                if corner & 2 == 0 {
                                    -cloud.height_mm * 0.5
                                } else {
                                    cloud.height_mm * 0.5
                                },
                            ];
                            handles.push(handle(
                                &group.target,
                                HandleKind::CloudCorner { corner },
                                cloud.paper_point(local, center),
                            ));
                        }
                    }
                    if cloud.show_label
                        && cloud
                            .revision_label(source)
                            .is_ok_and(|label| !label.is_empty())
                    {
                        handles.push(handle(
                            &group.target,
                            HandleKind::CloudLabel,
                            [
                                center[0] + cloud.label_offset_mm[0],
                                center[1] + cloud.label_offset_mm[1],
                            ],
                        ));
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
        for displayed in drawing::sheet_display_instances(doc, evaluation, source)? {
            if matches!(
                displayed.effect.visibility,
                Some(
                    crate::display_states::Visibility::Hidden
                        | crate::display_states::Visibility::Ghost
                )
            ) {
                continue;
            }
            let instance = displayed.placement;
            let key = if instance.mesh_key.is_empty() {
                instance.configuration.as_ref().map_or_else(
                    || instance.definition.clone(),
                    |id| format!("{}@{id}", instance.definition),
                )
            } else {
                instance.mesh_key.clone()
            };
            let mesh = evaluation.meshes.get(&key).ok_or_else(|| invalid(&format!(
                "Drawing '{}' requires current evaluated geometry for '{}'; wait for the represented model to finish evaluation",
                source.name, instance.name
            )))?;
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
                    // This edge and occurrence are already resolved from the
                    // current displayed mesh. Looking the same reference up
                    // again scans every edge and canonicalizes its ancestry
                    // for each candidate in each view.
                    if let Ok(circle) = drawing::circular_carrier(curve, instance.matrix) {
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
        Primitive::Polyline { points, .. } | Primitive::HiddenPolyline { points } => points
            .windows(2)
            .map(|pair| segment_distance(point, pair[0], pair[1]))
            .fold(f64::INFINITY, f64::min),
        Primitive::Spline { curve, .. } | Primitive::HiddenSpline { curve } => {
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
            let Some(index) = (0..=count).min_by(|a, b| {
                distance(*a as f64 / count as f64).total_cmp(&distance(*b as f64 / count as f64))
            }) else {
                continue;
            };
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
            let Ok(model_point) = drawing::curve_point(carrier, parameter) else {
                continue;
            };
            let Ok(paper_point) = candidate
                .projection
                .paper_point(&candidate.view, model_point)
            else {
                continue;
            };
            model = model_point;
            paper = paper_point;
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
    snap_excluding(
        scene,
        std::slice::from_ref(target),
        paper_mm,
        tolerance_mm,
        grid_mm,
    )
}
pub fn snap_excluding(
    scene: &Scene,
    excluded: &[Target],
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
            if excluded.contains(&other.target) {
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
    if dimension.options.kind == DimensionKind::Perpendicular {
        let (direction, a, b) = drawing::parallel_lines(
            doc,
            evaluation,
            sheet,
            &dimension.view,
            dimension
                .options
                .curve
                .as_ref()
                .ok_or_else(|| invalid("Select the first straight edge"))?,
            dimension
                .options
                .second_curve
                .as_ref()
                .ok_or_else(|| invalid("Select the second straight edge"))?,
        )?;
        let separation = (b[1] - a[1]) * direction[0] - (b[0] - a[0]) * direction[1];
        if separation.abs() < 1e-9 {
            return Err(invalid(
                "The selected edges coincide in this view; repair their references first",
            ));
        }
        let sign = separation.signum();
        return Ok([-direction[0] * sign, -direction[1] * sign]);
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

/// Start a new dimension outside the represented view's geometry. The saved
/// result is still an ordinary editable paper offset, never a screen position.
/// Existing annotations and captions supply keep-out regions for its label.
pub fn place_dimension_outside(
    doc: &Document,
    evaluation: &Evaluation,
    sheet: &Drawing,
    dimension: &mut DrawingDimension,
) -> Result<()> {
    if matches!(
        dimension.options.kind,
        DimensionKind::Angular | DimensionKind::HoleCallout
    ) {
        return Ok(());
    }
    let rendered = drawing::build_sheet_layout(doc, evaluation, sheet, 0)?;
    let projection = rendered
        .projections
        .get(&dimension.view)
        .ok_or_else(|| invalid("Missing projected dimension view"))?;
    let group = rendered
        .groups
        .iter()
        .find(|group| matches!(&group.target, Target::View { key } if key == &dimension.view))
        .ok_or_else(|| invalid("Missing drawing geometry for dimension placement"))?;
    let geometry = rendered.primitives
        [group.primitive_start..group.primitive_start + group.primitive_count]
        .iter()
        .filter(|primitive| !matches!(primitive, Primitive::Text { .. }))
        .cloned()
        .collect::<Vec<_>>();
    let Some(extent) = bounds(&geometry) else {
        return Ok(());
    };
    let axis = dimension_axis(doc, evaluation, sheet, projection, dimension)?;
    let dot = |p: [f64; 2]| p[0] * axis[0] + p[1] * axis[1];
    let mut base = projection.paper_point(
        &dimension.view,
        drawing::point(
            doc,
            evaluation,
            sheet,
            dimension.first,
            &dimension.options.first,
        )?,
    )?;
    if matches!(
        dimension.options.kind,
        DimensionKind::Horizontal | DimensionKind::Vertical
    ) {
        let (a, b) = if dimension.options.second_curve.is_some() {
            let (a, b) = drawing::edge_separation_points(doc, evaluation, sheet, dimension)?;
            (projection.paper_projected(a), projection.paper_projected(b))
        } else {
            (
                base,
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
        let normal_axis = usize::from(dimension.options.kind == DimensionKind::Horizontal);
        base[normal_axis] = a[normal_axis].max(b[normal_axis]);
    } else if dimension.options.kind == DimensionKind::Perpendicular {
        let (a, _) = drawing::edge_separation_points(doc, evaluation, sheet, dimension)?;
        base = projection.paper_projected(a);
    } else if matches!(
        dimension.options.kind,
        DimensionKind::Diameter | DimensionKind::Radius
    ) {
        let circle = drawing::circular(
            doc,
            evaluation,
            sheet,
            dimension
                .options
                .curve
                .as_ref()
                .ok_or_else(|| invalid("Choose an exact circular edge"))?,
        )?;
        base = projection.paper_point(&dimension.view, circle.center)?;
    }
    let corners = [
        extent[0],
        [extent[0][0], extent[1][1]],
        extent[1],
        [extent[1][0], extent[0][1]],
    ];
    let limits = [
        corners
            .iter()
            .map(|p| dot(*p))
            .fold(f64::INFINITY, f64::min),
        corners
            .iter()
            .map(|p| dot(*p))
            .fold(f64::NEG_INFINITY, f64::max),
    ];
    // Horizontal labels go below the geometry so their extension lines cannot
    // run through the view captions above it. Vertical labels prefer the right.
    let preferred = if axis[1].abs() >= axis[0].abs() {
        -1.
    } else {
        1.
    };
    let keep_out = rendered
        .primitives
        .iter()
        .filter(|primitive| matches!(primitive, Primitive::Text { .. }))
        .filter_map(|primitive| bounds(std::slice::from_ref(primitive)))
        .collect::<Vec<_>>();
    let paper = drawing::paper_bounds(sheet);
    let mut fallback = None;
    for side in [preferred, -preferred] {
        for lane in 0..=sheet.dimensions.len().min(32) {
            let clearance = 8. + lane as f64 * 6.;
            let offset = if side < 0. {
                limits[0] - clearance
            } else {
                limits[1] + clearance
            } - dot(base);
            dimension.offset_mm = offset;
            let mut primitives = Vec::new();
            drawing::dimension_for_placement(
                doc,
                evaluation,
                sheet,
                projection,
                dimension,
                &mut primitives,
            )?;
            let labels = primitives
                .iter()
                .filter(|primitive| matches!(primitive, Primitive::Text { .. }))
                .filter_map(|primitive| bounds(std::slice::from_ref(primitive)))
                .collect::<Vec<_>>();
            fallback.get_or_insert(offset);
            if labels.iter().all(|label| {
                contains(paper, label[0])
                    && contains(paper, label[1])
                    && keep_out.iter().all(|other| {
                        label[1][0] + 2. < other[0][0]
                            || label[0][0] - 2. > other[1][0]
                            || label[1][1] + 2. < other[0][1]
                            || label[0][1] - 2. > other[1][1]
                    })
            }) {
                return Ok(());
            }
        }
    }
    // Dense pages retain a useful exterior starting point for manual layout;
    // final source/geometry validation remains the Apply gate.
    if let Some(offset) = fallback {
        dimension.offset_mm = offset;
    }
    Ok(())
}

/// Apply the same initial placement to a paper Smart Dimension request while
/// preserving `Action::CreateDimension`'s explicit-offset contract for callers.
pub fn place_created_dimension(
    doc: &Document,
    evaluation: &Evaluation,
    sheet: &Drawing,
    action: &mut Action,
) -> Result<()> {
    let Action::CreateDimension {
        view,
        kind,
        first,
        second,
        curve,
        second_curve,
        offset_mm,
    } = action
    else {
        return Err(invalid(
            "Initial dimension placement requires a new dimension",
        ));
    };
    let mut dimension = DrawingDimension {
        id: crate::id(),
        view: view.clone(),
        first: first.point,
        second: second.point,
        offset_mm: *offset_mm,
        parameter: None,
        tolerance: String::new(),
        options: drawing::DimensionOptions {
            kind: kind.clone(),
            first: first.reference.clone(),
            second: second.reference.clone(),
            curve: curve.as_deref().cloned(),
            second_curve: second_curve.as_deref().cloned(),
            ..Default::default()
        },
    };
    place_dimension_outside(doc, evaluation, sheet, &mut dimension)?;
    *offset_mm = dimension.offset_mm;
    Ok(())
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
            | AnnotationKind::GeometricTolerance { target, .. }
            | AnnotationKind::DatumTarget { target, .. }
            | AnnotationKind::GeometricToleranceFrame { target, .. },
            AnchorSlot::Target,
        ) => Ok(target),
        (AnnotationKind::RevisionCloud { target, .. }, AnchorSlot::Target) => Ok(target),
        (AnnotationKind::DimensionGroup { group }, AnchorSlot::Target) => Ok(&mut group.datum),
        (AnnotationKind::DimensionGroup { group }, AnchorSlot::GroupTarget { target }) => group
            .targets
            .iter_mut()
            .find(|source| &source.id == target)
            .map(|target| &mut target.anchor)
            .ok_or_else(|| invalid("The dimension group target no longer exists")),
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
    let rendered = drawing::build_sheet_layout(doc, evaluation, source, page)?;
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
        Action::TranslateAnnotations {
            annotations,
            delta_mm,
        } => {
            if page != 0 {
                return Err(invalid(
                    "Annotation cleanup belongs on the first paper page",
                ));
            }
            sheet = crate::drawing_layout::translate(source, annotations, *delta_mm)?;
        }
        Action::ArrangeAnnotations { request } => {
            if page != 0 {
                return Err(invalid(
                    "Annotation cleanup belongs on the first paper page",
                ));
            }
            sheet = crate::drawing_layout::arrange(doc, evaluation, source, request)?;
        }
        Action::CopyAnnotations {
            annotations,
            offset_mm,
        } => {
            if page != 0 {
                return Err(invalid(
                    "Annotation copying belongs on the first paper page",
                ));
            }
            sheet = crate::drawing_layout::copy(source, annotations, *offset_mm)?.0;
        }
        Action::DeleteAnnotations { annotations } => {
            if page != 0 {
                return Err(invalid(
                    "Annotation deletion belongs on the first paper page",
                ));
            }
            sheet = crate::drawing_layout::delete(source, annotations)?;
        }
        Action::Move { target, delta_mm } => {
            finite(delta_mm)?;
            match target {
                Target::View { key } => {
                    let rendered = drawing::build_sheet_layout(doc, evaluation, source, 0)?;
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
                    let rendered = drawing::build_sheet_layout(doc, evaluation, source, 0)?;
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
                    let rendered = drawing::build_sheet_layout(doc, evaluation, source, 0)?;
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
        Action::MoveGroupLabel {
            id,
            target,
            offset_mm,
        } => {
            finite(offset_mm)?;
            let AnnotationKind::DimensionGroup { group } =
                &mut annotation_mut(&mut sheet, id)?.kind
            else {
                return Err(invalid("Only dimension groups have group-label handles"));
            };
            group
                .targets
                .iter_mut()
                .find(|source| &source.id == target)
                .ok_or_else(|| invalid("The dimension group target no longer exists"))?
                .label_offset_mm = *offset_mm;
        }
        Action::ResizeCloudCorner {
            id,
            corner,
            delta_mm,
        } => {
            finite(delta_mm)?;
            if *corner > 3 {
                return Err(invalid("A cloud resize needs a current corner"));
            }
            let annotation = annotation_mut(&mut sheet, id)?;
            let AnnotationKind::RevisionCloud { cloud, .. } = &mut annotation.kind else {
                return Err(invalid("The annotation is no longer a revision cloud"));
            };
            if cloud.shape == crate::drawing_revision::Shape::Polygon {
                return Err(invalid("Polygon clouds use vertex handles"));
            }
            let local = cloud.local_delta(*delta_mm);
            cloud.width_mm += if corner & 1 == 0 { -local[0] } else { local[0] };
            cloud.height_mm += if corner & 2 == 0 { -local[1] } else { local[1] };
            cloud.validate()?;
            for (axis, delta) in delta_mm.iter().enumerate() {
                annotation.offset_mm[axis] += delta * 0.5;
            }
        }
        Action::MoveCloudVertex {
            id,
            vertex,
            point_mm,
        } => {
            finite(point_mm)?;
            let annotation = annotation_mut(&mut sheet, id)?;
            let AnnotationKind::RevisionCloud { cloud, .. } = &mut annotation.kind else {
                return Err(invalid("The annotation is no longer a revision cloud"));
            };
            if cloud.shape != crate::drawing_revision::Shape::Polygon {
                return Err(invalid("Only polygon clouds have editable vertices"));
            }
            cloud
                .vertices
                .iter_mut()
                .find(|source| &source.id == vertex)
                .ok_or_else(|| invalid("The cloud vertex no longer exists"))?
                .point_mm = *point_mm;
            cloud.validate()?;
        }
        Action::MoveCloudLabel { id, offset_mm } => {
            finite(offset_mm)?;
            let annotation = annotation_mut(&mut sheet, id)?;
            let AnnotationKind::RevisionCloud { cloud, .. } = &mut annotation.kind else {
                return Err(invalid("The annotation is no longer a revision cloud"));
            };
            cloud.label_offset_mm = *offset_mm;
            cloud.validate()?;
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
                    if dimension.options.second_curve.is_some() {
                        let PointReference::EdgePoint {
                            edge,
                            body,
                            instance_path,
                            ..
                        } = reference
                        else {
                            return Err(invalid(
                                "Reattach an edge-separation dimension to an exact straight model edge",
                            ));
                        };
                        let curve = CurveReference::ModelEdge {
                            edge: edge.clone(),
                            body: body.clone(),
                            instance_path: instance_path.clone(),
                        };
                        drawing::linear(doc, evaluation, source, &curve)?;
                        match slot {
                            AnchorSlot::First => dimension.options.curve = Some(curve),
                            AnchorSlot::Second => dimension.options.second_curve = Some(curve),
                            _ => {
                                return Err(invalid(
                                    "Edge separation needs a first or second edge reference",
                                ));
                            }
                        }
                        drawing::parallel_lines(
                            doc,
                            evaluation,
                            source,
                            &dimension.view,
                            dimension
                                .options
                                .curve
                                .as_ref()
                                .ok_or_else(|| invalid("Select the first straight edge"))?,
                            dimension
                                .options
                                .second_curve
                                .as_ref()
                                .ok_or_else(|| invalid("Select the second straight edge"))?,
                        )?;
                    }
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
                        AnchorSlot::Target | AnchorSlot::GroupTarget { .. } => {
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
            second_curve,
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
            if second_curve.is_some() || *kind == DimensionKind::Perpendicular {
                if !matches!(
                    kind,
                    DimensionKind::Horizontal
                        | DimensionKind::Vertical
                        | DimensionKind::Perpendicular
                ) {
                    return Err(invalid(
                        "Choose an edge-separation dimension type for two line carriers",
                    ));
                }
                let (direction, first_line, second_line) = drawing::parallel_lines(
                    doc,
                    evaluation,
                    source,
                    view,
                    curve
                        .as_ref()
                        .ok_or_else(|| invalid("Select the first straight edge"))?,
                    second_curve
                        .as_ref()
                        .ok_or_else(|| invalid("Select the second straight edge"))?,
                )?;
                if (*kind == DimensionKind::Horizontal && direction[0].abs() > 1e-8)
                    || (*kind == DimensionKind::Vertical && direction[1].abs() > 1e-8)
                    || ((second_line[0] - first_line[0]) * direction[1]
                        - (second_line[1] - first_line[1]) * direction[0])
                        .abs()
                        < 1e-9
                {
                    return Err(invalid(
                        "The selected line carriers have zero or incompatible separation in this view",
                    ));
                }
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
                    curve: curve.as_deref().cloned(),
                    second_curve: second_curve.as_deref().cloned(),
                    ..Default::default()
                },
            });
        }
    }
    drawing::validate_definition(&sheet)?;
    // Every gesture may repair one item while another still has a drawing
    // diagnostic. Native reference proofs above stay strict; the worker must
    // validate the complete drawing before final Apply or production export.
    drawing::build_sheet_layout(doc, evaluation, &sheet, page)?;
    Ok(sheet)
}
