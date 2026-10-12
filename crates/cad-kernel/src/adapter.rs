use cad_core::{
    CadError, Dimension, Document, Expr, NodeRef, Operation, Plane as CadPlane, Profile, Result,
    TopologyRef, TopologySignature, expression,
    protocol::{EDGE_CENTER_VERSION, EdgeCenter, EdgeData, FaceRange, MeshData},
};
use monstertruck::{
    geometry::prelude::{TryIntoHomogeneousBsplineCurve, TryIntoHomogeneousBsplineSurface},
    meshing::prelude::*,
    modeling::*,
    topology::compress::{CompressedEdge, CompressedFace, CompressedShell, CompressedSolid},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};
#[path = "component_history.rs"]
mod component_history;
use component_history::{Budget as HistoryBudget, MAX_HISTORY_BYTES, budget_error, history_size};

#[derive(Clone)]
pub enum Shape {
    Reference(cad_core::reference_geometry::Geometry),
    Analysis(cad_core::mold::DraftReport),
    Surfaces(Vec<Shell>),
    Profile(Face),
    Sketch {
        faces: Vec<Face>,
        paths: Vec<Wire>,
    },
    ReferenceCurves {
        paths: Vec<Wire>,
        evaluated: cad_core::reference_curves::Evaluated,
    },
    Bodies(Vec<Solid>),
}
impl Shape {
    pub(crate) fn boundary_shells(&self) -> Option<Vec<&Shell>> {
        match self {
            Self::Bodies(bodies) => Some(bodies.iter().flat_map(|b| b.boundaries()).collect()),
            Self::Surfaces(shells) => Some(shells.iter().collect()),
            _ => None,
        }
    }
}
pub trait KernelAdapter {
    fn fingerprint(&self) -> &str;
    fn clear_weld_joint_previews(&self) {}
    fn weld_joint_previews(&self) -> Vec<cad_core::weld_beads::JointPreview> {
        vec![]
    }
    fn alias_history(&self, owner: &NodeRef, input: &NodeRef) -> Result<()>;
    fn alias_histories(&self, owner: &NodeRef, inputs: &[NodeRef]) -> Result<()> {
        match inputs {
            [input] => self.alias_history(owner, input),
            _ => Err(CadError::Unsupported(
                "This geometry adapter cannot restore multiple suppressed body branches".into(),
            )),
        }
    }
    fn prepare_sketch(
        &self,
        sketch: &cad_core::Sketch,
        plane: &CadPlane,
        inputs: &BTreeMap<NodeRef, Shape>,
    ) -> Result<cad_core::Sketch>;
    /// Disposable material-region membership for semantic contour selection.
    /// Only stable application source IDs cross the adapter boundary.
    fn sketch_material_regions(
        &self,
        _doc: &Document,
        _owner: &str,
        _sketch: &cad_core::Sketch,
        _plane: &CadPlane,
    ) -> Result<Vec<cad_core::protocol::EvaluatedMaterialRegion>> {
        Ok(vec![])
    }
    fn sketch_plane(
        &self,
        plane: &CadPlane,
        reference: Option<&TopologyRef>,
        reference_body: Option<&NodeRef>,
        inputs: &BTreeMap<NodeRef, Shape>,
    ) -> Result<CadPlane>;
    fn mate_frame(
        &self,
        _doc: &Document,
        _owner: &str,
        _source: &cad_core::assembly_geometry::Source,
        _inputs: &BTreeMap<NodeRef, Shape>,
    ) -> Result<cad_core::assembly_geometry::Frame> {
        Err(CadError::Unsupported(
            "This kernel adapter does not resolve associative mate geometry".into(),
        ))
    }
    fn operation(
        &self,
        doc: &Document,
        owner: &NodeRef,
        operation: &Operation,
        inputs: &BTreeMap<NodeRef, Shape>,
        root: &Path,
        tolerance: f64,
    ) -> Result<Shape>;
    fn sheet_metadata(
        &self,
        _doc: &Document,
        _owner: &NodeRef,
        _inputs: &BTreeMap<NodeRef, Shape>,
        _tolerance: f64,
    ) -> Result<Option<cad_core::sheet_metal::EvaluatedSheet>> {
        Err(CadError::Unsupported(
            "This geometry adapter does not provide sheet manufacturing metadata".into(),
        ))
    }
    fn weldment_metadata(
        &self,
        _doc: &Document,
        _owner: &NodeRef,
        _inputs: &BTreeMap<NodeRef, Shape>,
    ) -> Result<Option<cad_core::weldment::EvaluatedWeldment>> {
        Err(CadError::Unsupported(
            "This geometry adapter does not provide weldment manufacturing metadata".into(),
        ))
    }
    fn weld_bead_metadata(
        &self,
        _doc: &Document,
        _owner: &NodeRef,
        _inputs: &BTreeMap<NodeRef, Shape>,
    ) -> Result<Option<cad_core::weld_beads::Evaluated>> {
        Err(CadError::Unsupported(
            "This geometry adapter does not provide native weld bead metadata".into(),
        ))
    }
    fn mesh(&self, shape: &Shape, producer: &NodeRef, tolerance: f64) -> Result<MeshData>;
    fn assembly_weld(
        &self,
        _doc: &Document,
        _owner: &NodeRef,
        _definition: &cad_core::weld_beads::Definition,
        _contexts: &BTreeMap<Vec<String>, (Document, [[f64; 4]; 4])>,
        _root: &Path,
        _tolerance: f64,
    ) -> Result<(MeshData, Vec<Solid>, cad_core::weld_beads::Evaluated)> {
        Err(CadError::Unsupported(
            "This geometry adapter does not construct assembly weld beads".into(),
        ))
    }
    /// Disposable operation display geometry, independent from evaluated bodies.
    fn operation_delta(
        &self,
        _before: &Shape,
        _after: &Shape,
        _owner: &NodeRef,
        _tolerance: f64,
    ) -> Result<cad_core::protocol::OperationPreviewChange> {
        Err(CadError::Unsupported(
            "This geometry adapter does not provide localized operation previews".into(),
        ))
    }
}
#[derive(Default)]
pub struct MonstertruckAdapter {
    histories: std::cell::RefCell<BTreeMap<NodeRef, TopologyHistory>>,
    tables: std::cell::RefCell<BTreeMap<String, (monstertruck::step::load::Table, f64)>>,
    weld_joints: std::cell::RefCell<BTreeMap<NodeRef, cad_core::weld_beads::JointPreview>>,
}
#[derive(Clone, Default)]
pub(crate) struct TopologyHistory {
    edges: std::collections::HashMap<EdgeId, TopologyRef>,
    faces: std::collections::HashMap<FaceId, TopologyRef>,
    generated_curves: BTreeMap<String, Vec<TopologyRef>>,
    generated_surfaces: BTreeMap<String, Vec<TopologyRef>>,
    unresolved_curves: BTreeSet<String>,
    unresolved_surfaces: BTreeSet<String>,
}
pub(crate) fn construction_reference(reference: &TopologyRef) -> Result<serde_json::Value> {
    cad_core::provenance::normalized_reference(reference)
}
impl TopologyHistory {
    pub(crate) fn inherit_face(&mut self, face: FaceId, reference: TopologyRef) {
        self.faces.insert(face, reference);
    }
    pub(crate) fn inherit_edge(&mut self, edge: EdgeId, reference: TopologyRef) {
        self.edges.insert(edge, reference);
    }
    pub(crate) fn merge(&mut self, other: Self) {
        self.faces.extend(other.faces);
        self.edges.extend(other.edges);
        self.unresolved_curves.extend(other.unresolved_curves);
        self.unresolved_surfaces.extend(other.unresolved_surfaces);
        for (curve, references) in other.generated_curves {
            let existing = self.generated_curves.entry(curve).or_default();
            for reference in references {
                if !existing
                    .iter()
                    .any(|old| old.producer == reference.producer && old.role == reference.role)
                {
                    existing.push(reference);
                }
            }
        }
        for (surface, references) in other.generated_surfaces {
            let existing = self.generated_surfaces.entry(surface).or_default();
            for reference in references {
                if !existing
                    .iter()
                    .any(|old| old.producer == reference.producer && old.role == reference.role)
                {
                    existing.push(reference);
                }
            }
        }
    }
    pub(crate) fn face_source(&self, face: FaceId) -> Option<TopologyRef> {
        self.faces.get(&face).cloned()
    }
    pub(crate) fn edge_source(&self, edge: EdgeId) -> Option<TopologyRef> {
        self.edges.get(&edge).cloned()
    }
    pub(crate) fn constructed_face(
        &mut self,
        owner: &NodeRef,
        face: FaceId,
        sources: &[TopologyRef],
        purpose: &serde_json::Value,
    ) -> Result<TopologyRef> {
        let mut sources = sources
            .iter()
            .map(construction_reference)
            .collect::<Result<Vec<_>>>()?;
        sources.sort_by_key(serde_json::Value::to_string);
        sources.dedup();
        let reference = TopologyRef {
            producer: owner.clone(),
            role: format!(
                "construction:{}",
                serde_json::to_string(&cad_core::provenance::construction_payload(
                    serde_json::json!({"sources":sources,"purpose":purpose,"topology":"face"})
                )?)?
            ),
            topology: "face".into(),
            signature: None,
        };
        self.faces.insert(face, reference.clone());
        Ok(reference)
    }
    pub(crate) fn constructed_edge(
        &mut self,
        owner: &NodeRef,
        edge: EdgeId,
        sources: &[TopologyRef],
        purpose: &serde_json::Value,
    ) -> Result<TopologyRef> {
        let mut sources = sources
            .iter()
            .map(construction_reference)
            .collect::<Result<Vec<_>>>()?;
        sources.sort_by_key(serde_json::Value::to_string);
        sources.dedup();
        let reference = TopologyRef {
            producer: owner.clone(),
            role: format!(
                "construction:{}",
                serde_json::to_string(&cad_core::provenance::construction_payload(
                    serde_json::json!({"sources":sources,"purpose":purpose,"topology":"edge"})
                )?)?
            ),
            topology: "edge".into(),
            signature: None,
        };
        self.edges.insert(edge, reference.clone());
        Ok(reference)
    }
    pub(crate) fn tool_face(
        &mut self,
        owner: &NodeRef,
        face: &Face,
        sources: &[TopologyRef],
        purpose: &serde_json::Value,
    ) -> Result<TopologyRef> {
        let reference = self.constructed_face(owner, face.id(), sources, purpose)?;
        self.generated_surfaces
            .entry(serde_json::to_string(&face.surface())?)
            .or_default()
            .push(reference.clone());
        Ok(reference)
    }
    pub(crate) fn tool_edge(
        &mut self,
        owner: &NodeRef,
        edge: &Edge,
        sources: &[TopologyRef],
        purpose: &serde_json::Value,
    ) -> Result<TopologyRef> {
        let reference = self.constructed_edge(owner, edge.id(), sources, purpose)?;
        self.generated_curves
            .entry(serde_json::to_string(&edge.curve())?)
            .or_default()
            .push(reference.clone());
        Ok(reference)
    }
    pub(crate) fn drill_tool(
        &mut self,
        owner: &NodeRef,
        point: &str,
        body_source: &TopologyRef,
        solid: &Solid,
    ) -> Result<()> {
        let body_source = construction_reference(body_source)?;
        let reference = |kind: &str, index: usize| -> Result<TopologyRef> {
            Ok(TopologyRef {
                producer: owner.clone(),
                role: format!(
                    "hole-position:{}",
                    serde_json::to_string(
                        &serde_json::json!({"point":point,"body_source":body_source,"topology":kind,"ordinal":index})
                    )?
                ),
                topology: kind.into(),
                signature: None,
            })
        };
        let mut seen = std::collections::HashSet::new();
        for (index, edge) in solid
            .edge_iter()
            .filter(|edge| seen.insert(edge.id()))
            .enumerate()
        {
            let origin = reference("edge", index)?;
            self.edges.insert(edge.id(), origin.clone());
            self.generated_curves
                .entry(serde_json::to_string(&edge.curve())?)
                .or_default()
                .push(origin);
        }
        for (index, face) in solid.face_iter().enumerate() {
            let origin = reference("face", index)?;
            self.faces.insert(face.id(), origin.clone());
            self.generated_surfaces
                .entry(serde_json::to_string(&face.surface())?)
                .or_default()
                .push(origin);
        }
        Ok(())
    }
}

fn pattern_history(
    owner: &NodeRef,
    source: &Solid,
    copy: &Solid,
    source_history: &TopologyHistory,
    position: [usize; 2],
    target: &mut TopologyHistory,
) -> Result<()> {
    let mut seen = std::collections::HashSet::new();
    let edges = source
        .edge_iter()
        .filter(|edge| seen.insert(edge.id()))
        .collect::<Vec<_>>();
    seen.clear();
    let copies = copy
        .edge_iter()
        .filter(|edge| seen.insert(edge.id()))
        .collect::<Vec<_>>();
    if edges.len() != copies.len() {
        return Err(err("Affine pattern mapping changed edge topology"));
    }
    let reference = |source: &TopologyRef, kind: &str| -> Result<TopologyRef> {
        if position == [0, 0] {
            Ok(source.clone())
        } else {
            Ok(TopologyRef {
                producer: owner.clone(),
                role: format!(
                    "pattern:{}",
                    serde_json::to_string(
                        &serde_json::json!({"position":position,"topology":kind,"source":construction_reference(source)?})
                    )?
                ),
                topology: kind.into(),
                signature: None,
            })
        }
    };
    for (edge, copy) in edges.into_iter().zip(copies) {
        let origin = source_history
            .edges
            .get(&edge.id())
            .ok_or_else(|| err("Pattern source edge provenance is missing"))?;
        target.edges.insert(copy.id(), reference(origin, "edge")?);
    }
    let faces = source.face_iter().collect::<Vec<_>>();
    let copies = copy.face_iter().collect::<Vec<_>>();
    if faces.len() != copies.len() {
        return Err(err("Affine pattern mapping changed face topology"));
    }
    for (face, copy) in faces.into_iter().zip(copies) {
        let origin = source_history
            .faces
            .get(&face.id())
            .ok_or_else(|| err("Pattern source face provenance is missing"))?;
        target.faces.insert(copy.id(), reference(origin, "face")?);
    }
    Ok(())
}

fn err(e: impl std::fmt::Display) -> CadError {
    CadError::Invalid(format!("Geometry: {e}"))
}

/// Evaluate one-sided native derivatives without display sampling or an
/// intersection curve's approximate guide. Singular derivatives stay unknown.
fn selection_tangents(curve: &Curve) -> [Option<[f64; 3]>; 2] {
    fn supported(curve: &Curve, depth: usize) -> bool {
        if depth > 8 {
            return false;
        }
        match curve {
            Curve::Line(_) | Curve::BsplineCurve(_) | Curve::NurbsCurve(_) => true,
            Curve::ParameterCurve(parameter) => match parameter.surface().as_ref() {
                Surface::RevolutionSurface(processor) => {
                    supported(processor.entity().entity_curve(), depth + 1)
                }
                _ => true,
            },
            Curve::IntersectionCurve(_) => false,
        }
    }
    if !supported(curve, 0) {
        return [None; 2];
    }
    let (a, b) = curve.range_tuple();
    if !a.is_finite() || !b.is_finite() || a >= b {
        return [None; 2];
    }
    [curve.der(a), -curve.der(b)].map(|direction| {
        let values = [direction.x, direction.y, direction.z];
        let scale = values.iter().map(|value| value.abs()).fold(0., f64::max);
        if !scale.is_finite() || scale <= 0. || values.iter().any(|value| !value.is_finite()) {
            return None;
        }
        let scaled = values.map(|value| value / scale);
        let length = scaled.iter().map(|value| value * value).sum::<f64>().sqrt();
        (length.is_finite() && length > 0.).then(|| scaled.map(|value| value / length))
    })
}

/// Optional native guide geometry. Recognition verifies exact rational
/// carriers, never sampled display points; unsupported cases remain absent and
/// must not invalidate an otherwise supported mesh or modeling operation.
fn edge_center(curve: &Curve) -> Option<EdgeCenter> {
    if !matches!(
        curve,
        Curve::Line(_) | Curve::BsplineCurve(_) | Curve::NurbsCurve(_)
    ) {
        return None;
    }
    let (a, b) = curve.range_tuple();
    if !a.is_finite() || !b.is_finite() || a >= b {
        return None;
    }
    let start = curve.subs(a);
    let end = curve.subs(b);
    let coordinates = |point: Point3| [point.x, point.y, point.z];
    if coordinates(start)
        .into_iter()
        .chain(coordinates(end))
        .any(|value| !value.is_finite())
    {
        return None;
    }
    if crate::bounded_carriers::line(curve).is_ok() {
        return Some(EdgeCenter::Line {
            version: EDGE_CENTER_VERSION,
            start: coordinates(start),
            end: coordinates(end),
        });
    }
    let (center, normal, radius_mm) = crate::axis_geometry::circle_geometry(curve).ok()?;
    let radial = start - center;
    let length = radial.magnitude();
    if !length.is_finite() || length <= 1e-9 {
        return None;
    }
    let radial = radial / length;
    Some(EdgeCenter::Circle {
        version: EDGE_CENTER_VERSION,
        center: coordinates(center),
        normal: [normal.x, normal.y, normal.z],
        radius_mm,
        radial: [radial.x, radial.y, radial.z],
    })
}

pub(crate) fn exact_curve(curve: &Curve) -> Option<cad_core::sketch_edit::Curve3> {
    // Intersection guides are not the actual curve. Never convert them as if
    // they were an exact B-spline, even though the vendor conversion permits it.
    if !matches!(
        curve,
        Curve::Line(_) | Curve::BsplineCurve(_) | Curve::NurbsCurve(_)
    ) {
        return None;
    }
    let mut curve = curve.try_into_homogeneous_bspline_curve()?;
    curve.clamp();
    if curve
        .control_points()
        .iter()
        .any(|p| !p.w.is_finite() || p.w <= 0.)
    {
        return None;
    }
    Some(cad_core::sketch_edit::Curve3 {
        control_points: curve
            .control_points()
            .iter()
            .map(|p| [p.x / p.w, p.y / p.w, p.z / p.w])
            .collect(),
        weights: curve.control_points().iter().map(|p| p.w).collect(),
        knots: curve.knot_vector().to_vec(),
        degree: curve.degree(),
    })
}
fn vector(v: [f64; 3]) -> Result<Vector3> {
    let v = Vector3::new(v[0], v[1], v[2]);
    if !v.magnitude2().is_finite() || v.magnitude2() < 1e-18 {
        return Err(err("Zero or invalid direction"));
    }
    Ok(v.normalize())
}
pub fn matrix(m: [[f64; 4]; 4]) -> Matrix4 {
    Matrix4::from_cols(
        Vector4::new(m[0][0], m[1][0], m[2][0], m[3][0]),
        Vector4::new(m[0][1], m[1][1], m[2][1], m[3][1]),
        Vector4::new(m[0][2], m[1][2], m[2][2], m[3][2]),
        Vector4::new(m[0][3], m[1][3], m[2][3], m[3][3]),
    )
}
fn point(p: [f64; 3]) -> Point3 {
    Point3::new(p[0], p[1], p[2])
}
fn positive(v: f64) -> Result<f64> {
    if v > 1e-9 && v.is_finite() {
        Ok(v)
    } else {
        Err(err("Dimension must be positive"))
    }
}
pub(crate) fn wire_polygon(points: &[[f64; 2]], plane: &CadPlane) -> Result<Wire> {
    if points.len() < 3 {
        return Err(err("A closed profile needs at least three points"));
    }
    let area: f64 = (0..points.len())
        .map(|i| {
            points[i][0] * points[(i + 1) % points.len()][1]
                - points[(i + 1) % points.len()][0] * points[i][1]
        })
        .sum();
    if area.abs() < 1e-9 {
        return Err(err("Degenerate profile"));
    }
    let mut points = points.to_vec();
    if area < 0. {
        points.reverse();
    }
    let vertices = points
        .iter()
        .map(|p| {
            builder::vertex(Point3::new(
                plane.origin[0] + p[0] * plane.u[0] + p[1] * plane.v[0],
                plane.origin[1] + p[0] * plane.u[1] + p[1] * plane.v[1],
                plane.origin[2] + p[0] * plane.u[2] + p[1] * plane.v[2],
            ))
        })
        .collect::<Vec<_>>();
    Ok((0..vertices.len())
        .map(|i| builder::line(&vertices[i], &vertices[(i + 1) % vertices.len()]))
        .collect())
}
pub(crate) fn oriented_extrude(face: &Face, direction: Vector3) -> Solid {
    let face = if face.oriented_surface().normal(0., 0.).dot(direction) < 0. {
        face.inverse()
    } else {
        face.clone()
    };
    builder::extrude(&face, direction)
}
fn cylinder(origin: Point3, axis: Vector3, radius: f64, length: f64) -> Result<Solid> {
    let perpendicular = if axis.x.abs() < 0.9 {
        axis.cross(Vector3::unit_x())
    } else {
        axis.cross(Vector3::unit_y())
    }
    .normalize();
    let wire: Wire = primitive::circle(origin + perpendicular * positive(radius)?, origin, axis, 4);
    let face: Face = builder::try_attach_plane(vec![wire]).map_err(err)?;
    Ok(oriented_extrude(&face, axis * positive(length)?))
}
fn bodies<'a>(inputs: &'a BTreeMap<NodeRef, Shape>, r: &NodeRef) -> Result<&'a Vec<Solid>> {
    match inputs.get(r) {
        Some(Shape::Bodies(s)) => Ok(s),
        _ => Err(err(format!("{} is not a solid output", r.node))),
    }
}
fn profile<'a>(inputs: &'a BTreeMap<NodeRef, Shape>, r: &NodeRef) -> Result<&'a Face> {
    match inputs.get(r) {
        Some(Shape::Profile(f)) => Ok(f),
        Some(Shape::Sketch { faces, .. }) if faces.len() == 1 => Ok(&faces[0]),
        Some(Shape::Sketch { faces, .. }) if faces.is_empty() => Err(err(
            "This sketch has no closed material contour; close its members or choose a section sketch",
        )),
        Some(Shape::Sketch { .. }) => Err(CadError::Unsupported(
            "This feature requires one closed section; extrude supports multiple separate contours"
                .into(),
        )),
        _ => Err(err(format!("{} is not a planar profile", r.node))),
    }
}
pub(crate) fn solid_control_points(solid: &Solid) -> Result<Vec<Point3>> {
    let mut points = solid.vertex_iter().map(|v| v.point()).collect::<Vec<_>>();
    for edge in solid.edge_iter() {
        match edge.curve(){
        Curve::Line(_)=>{},Curve::BsplineCurve(curve)=>points.extend(curve.control_points().iter().copied()),
        Curve::NurbsCurve(curve)=>{for point in curve.control_points(){if point.w<=0.||!point.w.is_finite(){return Err(CadError::Unsupported("Through All needs positive-weight curve bounds".into()));}points.push(Point3::new(point.x/point.w,point.y/point.w,point.z/point.w));}},
        _=>return Err(CadError::Unsupported("Through All cannot yet bound this imported intersection or parameter curve; use an explicit depth".into())),
    }
    }
    for face in solid.face_iter() {
        let surface = face.surface();
        if matches!(surface, Surface::Plane(_)) {
            continue;
        }
        let net = surface
            .try_into_homogeneous_bspline_surface()
            .ok_or_else(|| {
                CadError::Unsupported(
                    "Through All cannot yet bound this surface; use an explicit depth".into(),
                )
            })?;
        for point in net.control_points().iter().flatten() {
            if point.w <= 0. || !point.w.is_finite() {
                return Err(CadError::Unsupported(
                    "Through All needs positive-weight surface bounds".into(),
                ));
            }
            points.push(Point3::new(
                point.x / point.w,
                point.y / point.w,
                point.z / point.w,
            ));
        }
    }
    Ok(points)
}
fn edge_signature(edge: &Edge) -> Result<TopologySignature> {
    let c = edge.oriented_curve();
    let (a, b) = c.range_tuple();
    let points = (0..=16)
        .map(|i| crate::curve_sampling::evaluate_point(&c, a + (b - a) * i as f64 / 16.))
        .collect::<Result<Vec<_>>>()?;
    let center = points
        .iter()
        .fold(Vector3::new(0., 0., 0.), |s, p| s + p.to_vec())
        / points.len() as f64;
    let measure: f64 = points.windows(2).map(|p| p[0].distance(p[1])).sum();
    if ![center.x, center.y, center.z, measure]
        .into_iter()
        .all(f64::is_finite)
    {
        return Err(err("Edge signature is nonfinite"));
    }
    Ok(TopologySignature {
        center: [center.x, center.y, center.z],
        measure,
        tolerance: 1e-5,
    })
}
fn role(center: [f64; 3], bounds: [[f64; 3]; 2]) -> String {
    let a: Vec<_> = (0..3)
        .map(|i| {
            let size = (bounds[1][i] - bounds[0][i]).max(1e-9);
            let fraction = (center[i] - bounds[0][i]) / size;
            if fraction.abs() < 1e-7 {
                "min".into()
            } else if (fraction - 1.).abs() < 1e-7 {
                "max".into()
            } else {
                format!("{fraction:.8}")
            }
        })
        .collect();
    format!("x:{};y:{};z:{}", a[0], a[1], a[2])
}
fn shell_bounds(shell: &Shell) -> [[f64; 3]; 2] {
    let mut bounds = [[f64::INFINITY; 3], [f64::NEG_INFINITY; 3]];
    for vertex in shell.vertex_iter() {
        let p = vertex.point();
        for (axis, value) in [p.x, p.y, p.z].into_iter().enumerate() {
            bounds[0][axis] = bounds[0][axis].min(value);
            bounds[1][axis] = bounds[1][axis].max(value);
        }
    }
    bounds
}
/// Compare saved tree roles and compact closure roles through the same
/// application-owned identity, without using numerical signatures as aliases.
fn matches_reference(
    source: &TopologyRef,
    wanted: &TopologyRef,
    canonical: &serde_json::Value,
) -> Result<bool> {
    if source.producer != wanted.producer || source.topology != wanted.topology {
        return Ok(false);
    }
    if source.role == wanted.role {
        return Ok(true);
    }
    Ok(construction_reference(source)? == *canonical)
}
fn resolve_shape_edges(
    shape: &Shape,
    refs: &[TopologyRef],
    history: Option<&TopologyHistory>,
) -> Result<Vec<EdgeId>> {
    if let Shape::ReferenceCurves { paths, .. } = shape {
        return resolve_boundary_edges(
            &paths
                .iter()
                .flat_map(|wire| wire.iter().cloned())
                .collect::<Vec<_>>(),
            refs,
            history,
        );
    }
    let shells = shape
        .boundary_shells()
        .ok_or_else(|| err("Select a solid or surface body"))?;
    resolve_boundary_edges(
        &shells
            .iter()
            .flat_map(|s| s.edge_iter())
            .collect::<Vec<_>>(),
        refs,
        history,
    )
}
fn resolve_boundary_edges(
    edges: &[Edge],
    refs: &[TopologyRef],
    history: Option<&TopologyHistory>,
) -> Result<Vec<EdgeId>> {
    let mut resolved = vec![];
    for r in refs {
        let wanted = construction_reference(r)?;
        let mut candidates = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for edge in edges.iter().filter(|edge| seen.insert(edge.id())) {
            let matches = if let Some(origin) = history.and_then(|h| h.edges.get(&edge.id())) {
                matches_reference(origin, r, &wanted)?
            } else if let Some(wanted) = &r.signature {
                let sig = edge_signature(edge)?;
                sig.center
                    .iter()
                    .zip(wanted.center)
                    .all(|(a, b)| (a - b).abs() <= wanted.tolerance)
                    && (sig.measure - wanted.measure).abs() <= wanted.tolerance
            } else {
                false
            };
            if matches {
                candidates.push(edge);
            }
        }
        if candidates.len() != 1 {
            return Err(err(format!(
                "Reference {} is {} ({} candidates); select an explicit replacement",
                r.role,
                if candidates.is_empty() {
                    "unresolved"
                } else {
                    "ambiguous"
                },
                candidates.len()
            )));
        }
        resolved.push(candidates[0].id());
    }
    if resolved.is_empty() {
        return Err(err("Select at least one edge"));
    }
    Ok(resolved)
}

fn resolve_faces(
    solids: &[Solid],
    refs: &[TopologyRef],
    history: Option<&TopologyHistory>,
) -> Result<Vec<FaceId>> {
    let mut resolved = Vec::new();
    for reference in refs {
        if reference.topology != "face" {
            return Err(err("This operation requires face references"));
        }
        let wanted = construction_reference(reference)?;
        let mut candidates = std::collections::HashSet::new();
        for face in solids.iter().flat_map(|solid| solid.face_iter()) {
            let matches = if let Some(origin) = history.and_then(|h| h.faces.get(&face.id())) {
                matches_reference(origin, reference, &wanted)?
            } else if let Some(signature) = &reference.signature {
                let points = face.vertex_iter().map(|v| v.point()).collect::<Vec<_>>();
                let center = points.iter().fold(Vector3::zero(), |s, p| s + p.to_vec())
                    / points.len().max(1) as f64;
                let area = face
                    .boundaries()
                    .iter()
                    .map(|wire| {
                        let points = wire.iter().map(|e| e.front().point()).collect::<Vec<_>>();
                        points
                            .as_slice()
                            .get(1..)
                            .unwrap_or_default()
                            .windows(2)
                            .map(|p| (p[0] - points[0]).cross(p[1] - points[0]))
                            .fold(Vector3::zero(), |s, p| s + p)
                            * 0.5
                    })
                    .fold(Vector3::zero(), |s, p| s + p)
                    .magnitude();
                [center.x, center.y, center.z]
                    .iter()
                    .zip(signature.center)
                    .all(|(a, b)| (a - b).abs() <= signature.tolerance)
                    && (area - signature.measure).abs() <= signature.tolerance
            } else {
                false
            };
            if matches {
                candidates.insert(face.id());
            }
        }
        if candidates.len() != 1 {
            return Err(err(format!(
                "Face reference {} is {} ({} candidates); select an explicit replacement",
                reference.role,
                if candidates.is_empty() {
                    "unresolved"
                } else {
                    "ambiguous"
                },
                candidates.len()
            )));
        }
        let id = *candidates.iter().next().unwrap();
        if !resolved.contains(&id) {
            resolved.push(id);
        }
    }
    Ok(resolved)
}
impl KernelAdapter for MonstertruckAdapter {
    fn prepare_sketch(
        &self,
        source: &cad_core::Sketch,
        plane: &CadPlane,
        inputs: &BTreeMap<NodeRef, Shape>,
    ) -> Result<cad_core::Sketch> {
        let mut sketch = source.clone();
        let histories = self.histories.borrow();
        for conversion in sketch.conversions.values_mut() {
            let shape = inputs
                .get(&conversion.body)
                .ok_or_else(|| err("Converted body is missing"))?;
            let ids = resolve_shape_edges(
                shape,
                std::slice::from_ref(&conversion.reference),
                histories.get(&conversion.body),
            )?;
            let mut candidates = match shape {
                Shape::ReferenceCurves { paths, .. } => paths
                    .iter()
                    .flat_map(|wire| wire.iter())
                    .filter(|edge| ids.contains(&edge.id()))
                    .cloned()
                    .collect::<Vec<_>>(),
                _ => shape
                    .boundary_shells()
                    .ok_or_else(|| err("Converted source must contain native edges"))?
                    .iter()
                    .flat_map(|shell| shell.edge_iter())
                    .filter(|edge| ids.contains(&edge.id()))
                    .collect::<Vec<_>>(),
            };
            let mut seen = std::collections::HashSet::new();
            candidates.retain(|e| seen.insert(e.id()));
            if candidates.len() != 1 {
                return Err(err(
                    "Converted edge is missing or ambiguous; replace its source reference",
                ));
            }
            let carrier = if conversion.construction_orientation {
                candidates[0].curve()
            } else {
                candidates[0].oriented_curve()
            };
            let curve = exact_curve(&carrier).ok_or_else(|| {
                CadError::Unsupported(
                    "Convert Entities cannot exactly project this intersection or parameter curve"
                        .into(),
                )
            })?;
            conversion.curve = cad_core::sketch_edit::project(&curve, plane)?;
        }
        Ok(sketch)
    }
    fn fingerprint(&self) -> &str {
        "monstertruck/0.4.1+fc2306852a46;adapter/39;solver/tcad-native-lm/16"
    }
    fn clear_weld_joint_previews(&self) {
        self.weld_joints.borrow_mut().clear();
    }
    fn weld_joint_previews(&self) -> Vec<cad_core::weld_beads::JointPreview> {
        self.weld_joints.borrow().values().cloned().collect()
    }
    fn sketch_material_regions(
        &self,
        doc: &Document,
        owner: &str,
        sketch: &cad_core::Sketch,
        plane: &CadPlane,
    ) -> Result<Vec<cad_core::protocol::EvaluatedMaterialRegion>> {
        crate::sketches::material_regions(doc, owner, sketch, plane)
    }
    fn sheet_metadata(
        &self,
        doc: &Document,
        owner: &NodeRef,
        inputs: &BTreeMap<NodeRef, Shape>,
        tolerance: f64,
    ) -> Result<Option<cad_core::sheet_metal::EvaluatedSheet>> {
        crate::sheet_stock::metadata(self, doc, owner, inputs, tolerance)
    }
    fn weldment_metadata(
        &self,
        doc: &Document,
        owner: &NodeRef,
        inputs: &BTreeMap<NodeRef, Shape>,
    ) -> Result<Option<cad_core::weldment::EvaluatedWeldment>> {
        crate::weld_stock::metadata(self, doc, owner, inputs)
    }
    fn weld_bead_metadata(
        &self,
        doc: &Document,
        owner: &NodeRef,
        inputs: &BTreeMap<NodeRef, Shape>,
    ) -> Result<Option<cad_core::weld_beads::Evaluated>> {
        let Some(feature) = doc.parts.get(&owner.definition).and_then(|part| {
            part.features
                .iter()
                .find(|feature| feature.id == owner.node)
        }) else {
            return Ok(None);
        };
        let Operation::WeldBead { definition } = &feature.operation else {
            return Ok(None);
        };
        crate::weld_beads::metadata(self, doc, owner, definition, inputs).map(Some)
    }
    fn alias_history(&self, owner: &NodeRef, input: &NodeRef) -> Result<()> {
        let history = self
            .histories
            .borrow()
            .get(input)
            .cloned()
            .unwrap_or_default();
        self.histories.borrow_mut().insert(owner.clone(), history);
        Ok(())
    }
    fn alias_histories(&self, owner: &NodeRef, inputs: &[NodeRef]) -> Result<()> {
        let sources = self.histories.borrow();
        let mut merged = TopologyHistory::default();
        for input in inputs {
            let source = sources
                .get(input)
                .ok_or_else(|| err("Suppressed source construction history is unavailable"))?;
            if source.faces.iter().any(|(id, reference)| {
                merged.faces.get(id).is_some_and(|existing| {
                    existing.producer != reference.producer || existing.role != reference.role
                })
            }) || source.edges.iter().any(|(id, reference)| {
                merged.edges.get(id).is_some_and(|existing| {
                    existing.producer != reference.producer || existing.role != reference.role
                })
            }) {
                return Err(err(
                    "Suppressed source branches have conflicting construction ancestry",
                ));
            }
            merged.merge(source.clone());
        }
        drop(sources);
        self.histories.borrow_mut().insert(owner.clone(), merged);
        Ok(())
    }
    fn sketch_plane(
        &self,
        plane: &CadPlane,
        reference: Option<&TopologyRef>,
        reference_body: Option<&NodeRef>,
        inputs: &BTreeMap<NodeRef, Shape>,
    ) -> Result<CadPlane> {
        if let Some(reference) = reference {
            if reference.topology == "plane" {
                return match inputs.get(&reference.producer) {
                    Some(Shape::Reference(geometry)) => geometry.selected_plane(&reference.role),
                    _ => Err(err("Missing or invalid reference plane")),
                };
            }
            self.reference_plane(reference, reference_body, inputs)
        } else {
            Ok(plane.clone())
        }
    }
    fn mate_frame(
        &self,
        doc: &Document,
        owner: &str,
        source: &cad_core::assembly_geometry::Source,
        inputs: &BTreeMap<NodeRef, Shape>,
    ) -> Result<cad_core::assembly_geometry::Frame> {
        source.resolve(
            doc,
            owner,
            &DatumResolver {
                adapter: self,
                doc,
                inputs,
            },
        )
    }
    fn operation(
        &self,
        doc: &Document,
        owner: &NodeRef,
        op: &Operation,
        inputs: &BTreeMap<NodeRef, Shape>,
        root: &Path,
        tolerance: f64,
    ) -> Result<Shape> {
        let value = |e: &Expr| expression::value(doc, &owner.definition, e, Dimension::Length);
        let angle = |e: &Expr| expression::value(doc, &owner.definition, e, Dimension::Angle);
        let mut mapped_input = None;
        let mut generated_history = TopologyHistory::default();
        let result = match op {
            Operation::ReferencePlane { definition } => {
                Shape::Reference(cad_core::reference_geometry::Geometry::Plane {
                    plane: definition.evaluate(
                        doc,
                        &owner.definition,
                        &DatumResolver {
                            adapter: self,
                            doc,
                            inputs,
                        },
                    )?,
                })
            }
            Operation::ReferenceAxis { definition } => {
                Shape::Reference(cad_core::reference_geometry::Geometry::Axis {
                    axis: definition.evaluate(
                        doc,
                        &owner.definition,
                        &DatumResolver {
                            adapter: self,
                            doc,
                            inputs,
                        },
                    )?,
                })
            }
            Operation::ReferencePoint { definition } => {
                Shape::Reference(cad_core::reference_geometry::Geometry::Point {
                    point: definition.evaluate(
                        doc,
                        &owner.definition,
                        &DatumResolver {
                            adapter: self,
                            doc,
                            inputs,
                        },
                    )?,
                })
            }
            Operation::ReferenceCoordinateSystem { definition } => {
                Shape::Reference(cad_core::reference_geometry::Geometry::CoordinateSystem {
                    frame: definition.evaluate(
                        doc,
                        &owner.definition,
                        &DatumResolver {
                            adapter: self,
                            doc,
                            inputs,
                        },
                    )?,
                })
            }
            Operation::ReferenceCurve { definition } => {
                let (paths, evaluated) = crate::reference_curves::build(
                    self,
                    doc,
                    owner,
                    definition,
                    inputs,
                    &mut generated_history,
                )?;
                Shape::ReferenceCurves { paths, evaluated }
            }
            Operation::Rib { definition } => crate::rib::operation(
                self,
                doc,
                owner,
                definition,
                inputs,
                tolerance,
                &mut generated_history,
            )?,
            Operation::DirectEdit { definition } => crate::direct_edit::operation(
                self,
                doc,
                owner,
                definition,
                inputs,
                &mut generated_history,
            )?,
            Operation::BodyFilter { definition } => crate::body_filter::operation(
                self,
                owner,
                definition,
                inputs,
                &mut generated_history,
            )?,
            Operation::Split { definition } => crate::split::operation(
                self,
                doc,
                owner,
                definition,
                inputs,
                tolerance,
                &mut generated_history,
            )?,
            Operation::CurvePattern { definition } => crate::curve_pattern::operation(
                self,
                doc,
                owner,
                definition,
                inputs,
                tolerance,
                &mut generated_history,
            )?,
            Operation::PlacementPattern { definition } => crate::placement_pattern::operation(
                self,
                doc,
                owner,
                definition,
                inputs,
                tolerance,
                &mut generated_history,
            )?,
            Operation::Surface { operation } => crate::surfaces::operation(
                self,
                doc,
                owner,
                operation,
                inputs,
                &mut generated_history,
            )?,
            Operation::Mold { operation } => {
                crate::mold::operation(self, doc, owner, operation, inputs, tolerance)?
            }
            Operation::Profile {
                profile: source,
                plane,
                plane_reference,
                reference_body,
            } => {
                let plane = self.sketch_plane(
                    plane,
                    plane_reference.as_ref(),
                    reference_body.as_ref(),
                    inputs,
                )?;
                let wire: Wire = match source {
                    Profile::Polygon { points } => wire_polygon(points, &plane)?,
                    Profile::Circle { radius, center } => {
                        let origin = point(plane.origin)
                            + Vector3::new(plane.u[0], plane.u[1], plane.u[2]) * center[0]
                            + Vector3::new(plane.v[0], plane.v[1], plane.v[2]) * center[1];
                        let u = vector(plane.u)?;
                        let v = vector(plane.v)?;
                        primitive::circle(
                            origin + u * positive(value(radius)?)?,
                            origin,
                            u.cross(v).normalize(),
                            4,
                        )
                    }
                    Profile::Sketch { sketch } => {
                        let s = doc.parts[&owner.definition]
                            .sketches
                            .get(sketch)
                            .ok_or_else(|| err("Missing sketch"))?
                            .clone();
                        let mut s = self.prepare_sketch(&s, &plane, inputs)?;
                        use cad_core::solver::ConstraintSolver;
                        cad_core::solver::NativeSolver.solve_sketch(
                            doc,
                            &owner.definition,
                            &mut s,
                        )?;
                        return crate::sketches::build(doc, &owner.definition, &s, &plane);
                    }
                };
                Shape::Profile(builder::try_attach_plane(vec![wire]).map_err(err)?)
            }
            Operation::Box { size } => {
                let s = [
                    positive(value(&size[0])?)?,
                    positive(value(&size[1])?)?,
                    positive(value(&size[2])?)?,
                ];
                Shape::Bodies(vec![primitive::cuboid(BoundingBox::from_iter([
                    Point3::new(0., 0., 0.),
                    point(s),
                ]))])
            }
            Operation::Cylinder {
                radius,
                length,
                axis,
            } => Shape::Bodies(vec![cylinder(
                Point3::new(0., 0., 0.),
                vector(*axis)?,
                value(radius)?,
                value(length)?,
            )?]),
            Operation::Sphere { radius } => {
                let r = positive(value(radius)?)?;
                let a = builder::vertex(Point3::new(0., 0., -r));
                let b = builder::vertex(Point3::new(0., 0., r));
                let arc: Edge =
                    builder::try_circle_arc(&a, &b, Point3::new(r, 0., 0.)).map_err(err)?;
                let line: Edge = builder::line(&b, &a);
                let wire: Wire = vec![arc, line].into();
                let face: Face = builder::try_attach_plane(vec![wire]).map_err(err)?;
                let solid: Solid = builder::revolve(
                    &face,
                    Point3::new(0., 0., 0.),
                    Vector3::unit_z(),
                    builder::SweepAngle::Closed,
                    4,
                );
                Shape::Bodies(vec![solid])
            }
            Operation::Extrude { .. } => crate::extrusion::operation(
                self,
                doc,
                owner,
                op,
                inputs,
                tolerance,
                &mut generated_history,
            )?,
            Operation::Revolve { .. } => crate::revolution::operation(
                self,
                doc,
                owner,
                op,
                inputs,
                tolerance,
                &mut generated_history,
            )?,
            Operation::Boolean { left, right, mode } => {
                let mode = match mode.as_str() {
                    "union" => crate::boolean_ops::Mode::Union,
                    "intersection" => crate::boolean_ops::Mode::Intersection,
                    "difference" => crate::boolean_ops::Mode::Difference,
                    _ => return Err(err("Unknown Boolean mode")),
                };
                let left_bodies = bodies(inputs, left)?;
                let right_bodies = bodies(inputs, right)?;
                Shape::Bodies(if matches!(mode, crate::boolean_ops::Mode::Union) {
                    crate::boolean_ops::union_with_contact(
                        left_bodies,
                        right_bodies,
                        tolerance,
                        &std::collections::BTreeSet::from([left.clone(), right.clone()]),
                        crate::revolution::Combination {
                            adapter: self,
                            owner,
                            generated: &mut generated_history,
                        },
                    )?
                } else {
                    crate::boolean_ops::sets_with_history(
                        left_bodies,
                        right_bodies,
                        mode,
                        tolerance,
                        &std::collections::BTreeSet::from([left.clone(), right.clone()]),
                        crate::revolution::Combination {
                            adapter: self,
                            owner,
                            generated: &mut generated_history,
                        },
                    )?
                })
            }
            Operation::Hole {
                body,
                radius,
                origin,
                axis,
                depth,
            } => {
                let tool = cylinder(
                    point(*origin),
                    vector(*axis)?,
                    value(radius)?,
                    value(depth)?,
                )?;
                Shape::Bodies(crate::boolean_ops::sets(
                    bodies(inputs, body)?,
                    &[tool],
                    crate::boolean_ops::Mode::Difference,
                    tolerance,
                )?)
            }
            Operation::DrilledHoles { definition } => crate::holes::operation(
                self,
                doc,
                owner,
                definition,
                inputs,
                tolerance,
                &mut generated_history,
            )?,
            Operation::Fillet { .. } | Operation::Chamfer { .. } => {
                self.edge_feature(doc, owner, op, inputs, &mut generated_history)?
            }
            Operation::ReferenceBody {
                body,
                instance,
                relative_to,
            } => {
                let placements = doc.placed_parts()?;
                let source = placements
                    .iter()
                    .find(|i| i.path.last() == Some(instance))
                    .ok_or_else(|| err("Reference instance is absent or not a part"))?;
                if source.definition != body.definition {
                    return Err(err("Reference body and instance definitions disagree"));
                }
                let source_matrix = matrix(source.matrix);
                let relative = if let Some(target) = relative_to {
                    let target = placements
                        .iter()
                        .find(|i| i.path.last() == Some(target))
                        .ok_or_else(|| err("Reference editing instance is absent"))?;
                    matrix(target.matrix)
                        .invert()
                        .ok_or_else(|| err("Singular reference placement"))?
                        * source_matrix
                } else {
                    source_matrix
                };
                Shape::Bodies(
                    bodies(inputs, body)?
                        .iter()
                        .map(|s| builder::transformed(s, relative))
                        .collect(),
                )
            }
            Operation::Transform { body, placement } => {
                let m = placement.matrix();
                let m = matrix(std::array::from_fn(|i| std::array::from_fn(|j| m[(i, j)])));
                mapped_input = Some((body, m));
                Shape::Bodies(
                    bodies(inputs, body)?
                        .iter()
                        .map(|s| builder::transformed(s, m))
                        .collect(),
                )
            }
            Operation::Scale { body, options } => {
                let intent = self.resolve_body_scale(doc, &owner.definition, options, inputs)?;
                let m = matrix(intent.matrix);
                if m.invert().is_none() {
                    return Err(err("Scale transform is numerically singular"));
                }
                let sources = bodies(inputs, body)?;
                for vertex in sources.iter().flat_map(|s| s.vertex_iter()) {
                    let p = m.transform_point(vertex.point());
                    if ![p.x, p.y, p.z].iter().all(|x| x.is_finite()) {
                        return Err(err("Scaled geometry exceeds the finite coordinate range"));
                    }
                }
                let scaled = sources.iter().map(|s| builder::transformed(s, m));
                let all = if options.keep_original {
                    sources.iter().cloned().chain(scaled).collect()
                } else {
                    mapped_input = Some((body, m));
                    scaled.collect()
                };
                Shape::Bodies(all)
            }
            Operation::Pattern {
                body,
                count,
                step,
                angle: a,
                axis,
                axis_reference,
                options,
            } => {
                let secondary_count = options.validate(*count)?;
                let step = Vector3::new(value(&step[0])?, value(&step[1])?, value(&step[2])?);
                let mut a = angle(a)?;
                if options.angle_is_total {
                    a /= if *count < 2 {
                        1.
                    } else if (a.abs() - std::f64::consts::TAU).abs() <= 1e-10 {
                        *count as f64
                    } else {
                        (*count - 1) as f64
                    };
                }
                let second = options
                    .second_direction
                    .as_ref()
                    .map(|direction| {
                        Ok::<_, CadError>(Vector3::new(
                            value(&direction.step[0])?,
                            value(&direction.step[1])?,
                            value(&direction.step[2])?,
                        ))
                    })
                    .transpose()?
                    .unwrap_or_else(Vector3::zero);
                if (*count > 1 && step.magnitude2() <= 1e-18 && a.abs() <= 1e-12)
                    || (secondary_count > 1 && second.magnitude2() <= 1e-18)
                {
                    return Err(err(
                        "Repeated pattern positions need nonzero spacing or rotation",
                    ));
                }
                let reference_axis = axis_reference
                    .as_ref()
                    .map(|r| {
                        r.resolve(&DatumResolver {
                            adapter: self,
                            doc,
                            inputs,
                        })
                    })
                    .transpose()?;
                let origin = reference_axis.as_ref().map(|a| a.origin).unwrap_or([0.; 3]);
                let axis = vector(
                    reference_axis
                        .as_ref()
                        .map(|a| a.direction)
                        .unwrap_or(*axis),
                )?;
                let seed = if options.rotate_instances {
                    point(origin)
                } else {
                    point(options.seed.resolve(
                        doc,
                        &owner.definition,
                        &DatumResolver {
                            adapter: self,
                            doc,
                            inputs,
                        },
                    )?)
                };
                let sources = bodies(inputs, body)?;
                if sources
                    .len()
                    .checked_mul(count * secondary_count)
                    .is_none_or(|n| n > 10000)
                {
                    return Err(err(
                        "A body pattern supports at most 10000 resulting bodies",
                    ));
                }
                let skipped = options
                    .skipped
                    .iter()
                    .copied()
                    .collect::<std::collections::BTreeSet<_>>();
                let source_history = self
                    .histories
                    .borrow()
                    .get(body)
                    .cloned()
                    .ok_or_else(|| err("Pattern source topology history is missing"))?;
                let mut all = vec![];
                for j in 0..secondary_count {
                    for i in 0..*count {
                        if skipped.contains(&[i, j]) {
                            continue;
                        }
                        let angle = a * i as f64;
                        let translation = step * i as f64 + second * j as f64;
                        if !angle.is_finite()
                            || ![translation.x, translation.y, translation.z]
                                .iter()
                                .all(|v| v.is_finite())
                        {
                            return Err(err(
                                "Pattern transforms exceed the finite coordinate range",
                            ));
                        }
                        let rotation = Matrix4::from_translation(point(origin).to_vec())
                            * Matrix4::from_axis_angle(axis, Rad(angle))
                            * Matrix4::from_translation(-point(origin).to_vec());
                        let m = if options.rotate_instances {
                            Matrix4::from_translation(translation) * rotation
                        } else {
                            Matrix4::from_translation(
                                translation + (rotation.transform_point(seed) - seed),
                            )
                        };
                        for s in sources {
                            let copy = builder::transformed(s, m);
                            pattern_history(
                                owner,
                                s,
                                &copy,
                                &source_history,
                                [i, j],
                                &mut generated_history,
                            )?;
                            all.push(copy);
                        }
                    }
                }
                Shape::Bodies(all)
            }
            Operation::Mirror {
                body,
                plane,
                plane_reference,
                keep_original,
            } => {
                let plane = plane_reference
                    .as_ref()
                    .map(|r| {
                        r.resolve(&DatumResolver {
                            adapter: self,
                            doc,
                            inputs,
                        })
                    })
                    .transpose()?
                    .unwrap_or_else(|| plane.clone());
                let normal = vector(plane.u)?.cross(vector(plane.v)?);
                if normal.magnitude2() < 1e-12 {
                    return Err(err("Mirror plane axes must be independent"));
                }
                let normal = normal.normalize();
                let mut transform = Matrix4::identity();
                for row in 0..3 {
                    for col in 0..3 {
                        transform[col][row] -= 2. * normal[row] * normal[col];
                    }
                }
                let offset = normal * (2. * point(plane.origin).to_vec().dot(normal));
                transform.w = offset.extend(1.);
                let mut solids = if *keep_original {
                    bodies(inputs, body)?.clone()
                } else {
                    vec![]
                };
                for solid in bodies(inputs, body)? {
                    let reflected = builder::transformed(solid, transform);
                    solids.push(
                        Solid::try_new(
                            reflected
                                .boundaries()
                                .iter()
                                .map(|s| s.inverse().into())
                                .collect(),
                        )
                        .map_err(err)?,
                    );
                }
                Shape::Bodies(solids)
            }
            Operation::Loft {
                profiles,
                body,
                mode,
                options,
            } => {
                if profiles.len() < 2 {
                    return Err(err("Loft needs two or more profiles"));
                }
                cad_core::contours::validate_profiles(profiles, &options.contours)?;
                let sections = profiles
                    .iter()
                    .map(|r| {
                        if let Some(thin) = &options.thin {
                            crate::extrusion::thin_profiles_traced(
                                self,
                                doc,
                                r,
                                thin,
                                inputs,
                                &owner.definition,
                                &options.contours,
                            )
                        } else {
                            crate::extrusion::profiles_traced(
                                self,
                                doc,
                                r,
                                inputs,
                                &options.contours,
                            )
                        }
                    })
                    .collect::<Result<Vec<_>>>()?;
                let regions = sections[0].len();
                if sections.iter().any(|s| s.len() != regions) {
                    return Err(CadError::Unsupported("Loft section sketches need matching material-region counts and contour order".into()));
                }
                let connectors=options.connectors.iter().map(|connector| {
                    let section=profiles.iter().position(|reference|reference==&connector.profile).ok_or_else(||err("A loft connector references a sketch outside the ordered sections"))?;
                    let region=if let Some(member)=&connector.region_member {crate::extrusion::region_for_member(doc,&connector.profile,&sections[section],member)?}else {
                        if options.contours.iter().any(|selection|selection.profile==connector.profile){return Err(err("A legacy ordinal loft connector needs a native Region Member before using Selected Contours"));}
                        if connector.region>=regions{return Err(err("A legacy loft connector references a missing material region"));}connector.region
                    };
                    Ok((section,region,connector))
                }).collect::<Result<Vec<_>>>()?;
                let overall = sections.last().unwrap()[0]
                    .face
                    .vertex_iter()
                    .next()
                    .unwrap()
                    .point()
                    - sections[0][0].face.vertex_iter().next().unwrap().point();
                let end_control =
                    |end: &cad_core::solid_sections::End, face: &Face| -> Result<Option<Vector3>> {
                        use cad_core::solid_sections::End;
                        let (mut direction, influence) = match end {
                            End::Default => return Ok(None),
                            End::Normal { influence } => (
                                face.oriented_surface().normal(0., 0.).normalize(),
                                influence,
                            ),
                            End::Direction {
                                axis,
                                influence,
                                reverse,
                            } => {
                                let axis = self.resolve_surface_axis(doc, axis, inputs)?;
                                (
                                    Vector3::from(axis.direction) * if *reverse { -1. } else { 1. },
                                    influence,
                                )
                            }
                        };
                        if matches!(end, End::Normal { .. }) && direction.dot(overall) < 0. {
                            direction = -direction;
                        }
                        let influence = positive(value(influence)?)?;
                        Ok(Some(direction.normalize() * influence))
                    };
                let start = end_control(&options.start, &sections[0][0].face)?;
                let end = end_control(&options.end, &sections.last().unwrap()[0].face)?;
                let tools = (0..regions)
                    .map(|region| {
                        let faces = sections
                            .iter()
                            .map(|section| section[region].face.clone())
                            .collect::<Vec<_>>();
                        let mut alignments = faces
                            .iter()
                            .map(|face| vec![None; face.boundaries().len()])
                            .collect::<Vec<_>>();
                        for (section, _, connector) in connectors
                            .iter()
                            .filter(|(_, selected, _)| *selected == region)
                        {
                            let point = crate::extrusion::sketch_vertex(
                                self,
                                doc,
                                &connector.profile,
                                &connector.vertex,
                                inputs,
                            )?;
                            let contour =
                                alignments[*section].get_mut(connector.contour).ok_or_else(
                                    || err("The loft connector contour no longer exists"),
                                )?;
                            if contour.is_some() {
                                return Err(err(
                                    "A loft section contour can have only one connector",
                                ));
                            }
                            *contour = Some(point);
                        }
                        let origins = sections
                            .iter()
                            .zip(profiles)
                            .map(|(sections, profile)| {
                                crate::section_history::members(profile, &sections[region])
                            })
                            .collect::<Result<Vec<_>>>()?;
                        let mut history = crate::section_history::History::new(
                            owner,
                            "loft",
                            &mut generated_history,
                        );
                        let solid = crate::solid_sections::loft(
                            &faces,
                            &alignments,
                            start,
                            end,
                            &origins,
                            &mut history,
                        )?;
                        history.finish(&solid)?;
                        Ok(solid)
                    })
                    .collect::<Result<Vec<_>>>()?;
                crate::revolution::combine(
                    tools,
                    &body
                        .iter()
                        .chain(&options.scope)
                        .cloned()
                        .collect::<Vec<_>>(),
                    mode,
                    inputs,
                    tolerance,
                    crate::revolution::Combination {
                        adapter: self,
                        owner,
                        generated: &mut generated_history,
                    },
                )?
            }
            Operation::Sweep {
                profile: r,
                path,
                path_sketch,
                body,
                mode,
                options,
            } => {
                cad_core::contours::validate_profiles(std::slice::from_ref(r), &options.contours)?;
                let sections = if let Some(thin) = &options.thin {
                    crate::extrusion::thin_profiles_traced(
                        self,
                        doc,
                        r,
                        thin,
                        inputs,
                        &owner.definition,
                        &options.contours,
                    )?
                } else {
                    crate::extrusion::profiles_traced(self, doc, r, inputs, &options.contours)?
                };
                let solids = crate::swept::sweep(
                    crate::swept::Context {
                        adapter: self,
                        doc,
                        owner,
                        inputs,
                        generated: &mut generated_history,
                    },
                    r,
                    &sections,
                    path_sketch.as_ref(),
                    path,
                    options,
                )?;
                crate::revolution::combine(
                    solids,
                    &body
                        .iter()
                        .chain(&options.scope)
                        .cloned()
                        .collect::<Vec<_>>(),
                    mode,
                    inputs,
                    tolerance,
                    crate::revolution::Combination {
                        adapter: self,
                        owner,
                        generated: &mut generated_history,
                    },
                )?
            }
            Operation::Shell {
                body,
                thickness,
                removed_faces,
                options,
            } => {
                let thickness = positive(value(thickness)?)?;
                let solids = bodies(inputs, body)?;
                let histories = self.histories.borrow();
                let removed = resolve_faces(solids, removed_faces, histories.get(body))?;
                let mut face_thicknesses = std::collections::HashMap::new();
                for wall in &options.face_thicknesses {
                    let ids = resolve_faces(
                        solids,
                        std::slice::from_ref(&wall.face),
                        histories.get(body),
                    )?;
                    let amount = positive(value(&wall.thickness)?)?;
                    for id in ids {
                        if removed.contains(&id) {
                            return Err(err("A removed Shell face has no retained wall thickness"));
                        }
                        if face_thicknesses.insert(id, amount).is_some() {
                            return Err(err("A Shell face can have only one thickness override"));
                        }
                    }
                }
                let mut result = Vec::new();
                for solid in solids {
                    let selected = solid
                        .face_iter()
                        .filter(|f| removed.contains(&f.id()))
                        .map(|f| f.id())
                        .collect::<Vec<_>>();
                    if !removed.is_empty() && selected.is_empty() {
                        result.push(solid.clone());
                    } else {
                        let mut history =
                            crate::shell_history::ShellHistory::new(self, owner, body, solid)?;
                        let shells = if let Some(shells) = crate::prismatic_shell::shell(
                            doc,
                            solid,
                            thickness,
                            &selected,
                            options.outward,
                            &face_thicknesses,
                            &mut history,
                        )? {
                            shells
                        } else if let Some(shells) = crate::curved_shell::shell(
                            solid,
                            thickness,
                            &selected,
                            options.outward,
                            &face_thicknesses,
                            &mut history,
                        )? {
                            shells
                        } else if let Some(shells) = crate::pocket_shell::shell(
                            solid,
                            thickness,
                            &selected,
                            options.outward,
                            &face_thicknesses,
                            &mut history,
                        )? {
                            shells
                        } else {
                            crate::polyhedron::shell(
                                solid,
                                thickness,
                                &selected,
                                options.outward,
                                &face_thicknesses,
                                &mut history,
                            )?
                        };
                        generated_history.merge(history.finish(&shells)?);
                        result.extend(shells);
                    }
                }
                Shape::Bodies(result)
            }
            Operation::Draft {
                body,
                faces,
                angle: a,
                plane,
                plane_reference,
            } => {
                let plane = plane_reference
                    .as_ref()
                    .map(|r| {
                        r.resolve(&DatumResolver {
                            adapter: self,
                            doc,
                            inputs,
                        })
                    })
                    .transpose()?
                    .unwrap_or_else(|| plane.clone());
                if faces.is_empty() {
                    return Err(err("Select side faces to draft"));
                }
                let normal = vector(plane.u)?.cross(vector(plane.v)?);
                let normal = vector([normal.x, normal.y, normal.z])?;
                let angle = angle(a)?;
                if !angle.is_finite() || angle.abs() >= 89_f64.to_radians() {
                    return Err(err("Draft angle must be between -89 and 89 degrees"));
                }
                let solids = bodies(inputs, body)?;
                let histories = self.histories.borrow();
                let selected = resolve_faces(solids, faces, histories.get(body))?;
                let mut result = Vec::new();
                for solid in solids {
                    let selected = solid
                        .face_iter()
                        .filter(|f| selected.contains(&f.id()))
                        .map(|f| f.id())
                        .collect::<Vec<_>>();
                    result.push(if selected.is_empty() {
                        solid.clone()
                    } else {
                        crate::polyhedron::draft(
                            solid,
                            &selected,
                            angle,
                            point(plane.origin),
                            normal,
                        )?
                    });
                }
                Shape::Bodies(result)
            }
            Operation::StructuralMember {
                section,
                start,
                end,
                roll,
            } => {
                let delta = point(*end) - point(*start);
                let axis = vector([delta.x, delta.y, delta.z])?;
                let u = if axis.z.abs() < 0.9 {
                    axis.cross(Vector3::unit_z())
                } else {
                    axis.cross(Vector3::unit_y())
                }
                .normalize();
                let u = Matrix3::from_axis_angle(axis, Rad(angle(roll)?)) * u;
                let v = axis.cross(u);
                let plane = CadPlane {
                    origin: *start,
                    u: [u.x, u.y, u.z],
                    v: [v.x, v.y, v.z],
                };
                let source = self.operation(
                    doc,
                    owner,
                    &Operation::Profile {
                        profile: section.clone(),
                        plane,
                        plane_reference: None,
                        reference_body: None,
                    },
                    inputs,
                    root,
                    tolerance,
                )?;
                let face = match source {
                    Shape::Profile(face) => face,
                    Shape::Sketch { mut faces, .. } if faces.len() == 1 => faces.remove(0),
                    _ => return Err(err("Structural section needs one closed planar contour")),
                };
                Shape::Bodies(vec![oriented_extrude(&face, delta)])
            }
            Operation::SheetMetal {
                width,
                thickness,
                sections,
                bends,
            } => Shape::Bodies(vec![crate::fabrication::sheet_strip(
                doc,
                &owner.definition,
                width,
                thickness,
                sections,
                bends,
            )?]),
            Operation::Sheet { .. } => crate::sheet_stock::operation(
                self,
                doc,
                owner,
                inputs,
                tolerance,
                &mut generated_history,
            )?,
            Operation::Weldment { .. } => {
                crate::weld_stock::operation(self, doc, owner, inputs, &mut generated_history)?
            }
            Operation::WeldBead { definition } => crate::weld_beads::operation(
                self,
                doc,
                owner,
                definition,
                inputs,
                &mut generated_history,
            )?,
            Operation::Import {
                resource,
                solid_ids,
            } => {
                let fingerprint = self.load_step_table(doc, resource, root)?;
                let tables = self.tables.borrow();
                let (table, length_scale) = tables
                    .get(&fingerprint)
                    .ok_or_else(|| err("Missing STEP table cache"))?;
                let mut keys = table
                    .manifold_solid_brep
                    .keys()
                    .copied()
                    .collect::<Vec<_>>();
                keys.sort();
                if !solid_ids.is_empty() {
                    if solid_ids.iter().any(|k| !keys.contains(k)) {
                        return Err(err(
                            "Imported solid identity is absent in the pinned STEP resource",
                        ));
                    }
                    keys.retain(|k| solid_ids.contains(k));
                }
                let mut all = vec![];
                for key in keys {
                    let (compressed, reports) = table
                        .to_compressed_solid_reported(&table.manifold_solid_brep[&key])
                        .map_err(err)?;
                    for report in reports {
                        report.require_lossless().map_err(err)?;
                    }
                    let mut boundaries = vec![];
                    for s in compressed.boundaries {
                        let edges = s
                            .edges
                            .into_iter()
                            .map(|e| {
                                Ok(CompressedEdge {
                                    vertices: e.vertices,
                                    curve: Curve::try_from(&e.curve).map_err(err)?,
                                })
                            })
                            .collect::<Result<Vec<_>>>()?;
                        let faces = s
                            .faces
                            .into_iter()
                            .map(|f| {
                                Ok(CompressedFace {
                                    boundaries: f.boundaries,
                                    orientation: f.orientation,
                                    surface: Surface::try_from(&f.surface).map_err(err)?,
                                })
                            })
                            .collect::<Result<Vec<_>>>()?;
                        boundaries.push(CompressedShell {
                            vertices: s.vertices,
                            edges,
                            faces,
                            vertex_stable_ids: s.vertex_stable_ids,
                            edge_stable_ids: s.edge_stable_ids,
                            face_stable_ids: s.face_stable_ids,
                        });
                    }
                    all.push(builder::transformed(
                        &Solid::extract(CompressedSolid {
                            boundaries,
                            id_allocator: compressed.id_allocator,
                            attributes: compressed.attributes,
                        })
                        .map_err(err)?,
                        Matrix4::from_scale(*length_scale),
                    ));
                }
                if all.is_empty() {
                    return Err(err("STEP contains no supported manifold solid"));
                }
                Shape::Bodies(all)
            }
            Operation::ImportSurfaces {
                resource,
                surface_model_ids,
            } => {
                let fingerprint = self.load_step_table(doc, resource, root)?;
                let tables = self.tables.borrow();
                let (table, scale) = tables
                    .get(&fingerprint)
                    .ok_or_else(|| err("Missing STEP table cache"))?;
                Shape::Surfaces(crate::step_import::surfaces(
                    table,
                    surface_model_ids,
                    *scale,
                    &fingerprint,
                    owner,
                    &mut generated_history,
                )?)
            }
        };
        if let Operation::Mold {
            operation:
                cad_core::mold::MoldOperation::Shrink {
                    body,
                    factors,
                    center,
                },
        } = op
        {
            let options = cad_core::body_editing::ScaleOptions {
                uniform: false,
                factors: factors.clone(),
                center: center.clone(),
                keep_original: false,
            };
            mapped_input = Some((
                body,
                matrix(
                    self.resolve_body_scale(doc, &owner.definition, &options, inputs)?
                        .matrix,
                ),
            ));
        }
        if let Shape::Bodies(all) = &result {
            for s in all {
                crate::validation::solid(s)?;
            }
        }
        self.record_history(owner, op, &result, inputs, mapped_input, &generated_history)?;
        Ok(result)
    }
    fn operation_delta(
        &self,
        before: &Shape,
        after: &Shape,
        owner: &NodeRef,
        tolerance: f64,
    ) -> Result<cad_core::protocol::OperationPreviewChange> {
        crate::operation_preview::delta(self, before, after, owner, tolerance)
    }
    fn assembly_weld(
        &self,
        doc: &Document,
        owner: &NodeRef,
        definition: &cad_core::weld_beads::Definition,
        contexts: &BTreeMap<Vec<String>, (Document, [[f64; 4]; 4])>,
        root: &Path,
        tolerance: f64,
    ) -> Result<(MeshData, Vec<Solid>, cad_core::weld_beads::Evaluated)> {
        self.weld_joints.borrow_mut().remove(owner);
        crate::assembly_welds::build(self, doc, owner, definition, contexts, root, tolerance)
    }
    fn mesh(&self, shape: &Shape, producer: &NodeRef, tolerance: f64) -> Result<MeshData> {
        if !tolerance.is_finite() || tolerance <= 0. {
            return Err(err("Display tolerance must be finite and positive"));
        }
        if let Shape::ReferenceCurves { paths, evaluated } = shape {
            let mut mesh = MeshData {
                positions: vec![],
                normals: vec![],
                indices: vec![],
                faces: vec![],
                edges: vec![],
                bounds: [[f64::INFINITY; 3], [f64::NEG_INFINITY; 3]],
                volume_mm3: 0.,
                body_counts: Some(cad_core::protocol::BodyCounts {
                    version: cad_core::protocol::BODY_COUNTS_VERSION,
                    solids: 0,
                    surfaces: 0,
                }),
            };
            let history = self.histories.borrow();
            let history = history
                .get(producer)
                .ok_or_else(|| err("Reference curve construction history is missing"))?;
            let mut point_count = 0usize;
            for edge in paths.iter().flat_map(|wire| wire.iter()) {
                // Source descriptors follow construction parameter orientation,
                // independently of an incident face or a display path traversal.
                let curve = edge.curve();
                let reference = history
                    .edge_source(edge.id())
                    .ok_or_else(|| err("Reference curve edge has no constructor ancestry"))?;
                let wanted = construction_reference(&reference)?;
                let mut matching_piece = false;
                for piece in &evaluated.pieces {
                    if matches_reference(&piece.reference, &reference, &wanted)? {
                        matching_piece = true;
                        break;
                    }
                }
                if !matching_piece {
                    return Err(err(
                        "Reference curve display has no matching native piece definition",
                    ));
                }
                let points = crate::curve_sampling::display_points(&curve, tolerance)?;
                point_count = point_count.saturating_add(points.len());
                if points.len() < 2 || point_count > 1_000_000 {
                    return Err(err(
                        "Reference curve display subdivision is nonfinite, empty or exceeds its resource bound",
                    ));
                }
                for point in &points {
                    for (axis, value) in point.iter().enumerate() {
                        mesh.bounds[0][axis] = mesh.bounds[0][axis].min(*value);
                        mesh.bounds[1][axis] = mesh.bounds[1][axis].max(*value);
                    }
                }
                mesh.positions.extend(points.iter().copied());
                mesh.edges.push(EdgeData {
                    points,
                    reference,
                    curve: exact_curve(&curve),
                    faces: vec![],
                    body: Some(producer.clone()),
                    native: None,
                    center: edge_center(&curve),
                });
            }
            return Ok(mesh);
        }
        let shells = shape
            .boundary_shells()
            .ok_or_else(|| err("Cannot tessellate a sketch or datum as a body"))?;
        let solid_volume = matches!(shape, Shape::Bodies(_));
        let body_counts = cad_core::protocol::BodyCounts {
            version: cad_core::protocol::BODY_COUNTS_VERSION,
            solids: match shape {
                Shape::Bodies(bodies) => bodies.len(),
                _ => 0,
            },
            surfaces: match shape {
                Shape::Surfaces(surfaces) => surfaces.len(),
                _ => 0,
            },
        };
        let mut mesh = MeshData {
            positions: vec![],
            normals: vec![],
            indices: vec![],
            faces: vec![],
            edges: vec![],
            bounds: [[f64::INFINITY; 3], [f64::NEG_INFINITY; 3]],
            volume_mm3: 0.,
            body_counts: Some(body_counts),
        };
        let mut native_vertices = std::collections::HashMap::new();
        let mut boundary_token = 0u32;
        // Optional selection metadata must never invalidate otherwise supported
        // geometry. Preflight the native boundary graph before allocating it.
        let selection_cache = shells.len() <= 65_536
            && shells
                .iter()
                .flat_map(|shell| shell.iter())
                .flat_map(Face::absolute_boundaries)
                .try_fold(0usize, |count, wire| {
                    count
                        .checked_add(wire.len().max(1))
                        .filter(|count| *count <= 65_536)
                })
                .is_some();
        for (component, shell) in shells.into_iter().enumerate() {
            let b = shell_bounds(shell);
            for (axis, _) in b[0].iter().enumerate() {
                mesh.bounds[0][axis] = mesh.bounds[0][axis].min(b[0][axis]);
                mesh.bounds[1][axis] = mesh.bounds[1][axis].max(b[1][axis]);
            }
            let tess = shell.robust_triangulation(tolerance);
            for (face, tri_face) in shell.iter().zip(tess.iter()) {
                let mut poly = tri_face
                    .surface()
                    .ok_or_else(|| err("Tessellation omitted a face"))?;
                if !tri_face.orientation() {
                    poly.invert();
                }
                if poly.tri_faces().is_empty() {
                    return Err(err("Tessellation produced an empty face"));
                }
                let start = mesh.indices.len();
                let pts = face.vertex_iter().map(|v| v.point()).collect::<Vec<_>>();
                let center = pts
                    .iter()
                    .fold(Vector3::new(0., 0., 0.), |s, p| s + p.to_vec())
                    / pts.len().max(1) as f64;
                let center = [center.x, center.y, center.z];
                let mut area = 0.;
                // Display normals belong to this native face only. Do not
                // average across a model edge, even when adjacent triangles
                // happen to share the same spatial position.
                let unit_normal = |value: Vector3| {
                    let scale = value.x.abs().max(value.y.abs()).max(value.z.abs());
                    if ![value.x, value.y, value.z, scale]
                        .iter()
                        .all(|v| v.is_finite())
                        || scale <= 0.
                    {
                        return None;
                    }
                    let scaled = value / scale;
                    let length = scaled.magnitude();
                    (length.is_finite() && length > 0.).then(|| scaled / length)
                };
                let mut fallback_normals = vec![Vector3::zero(); poly.positions().len()];
                for tri in poly.tri_faces() {
                    let p = tri.map(|v| poly.positions()[v.pos]);
                    let normal = (p[1] - p[0]).cross(p[2] - p[0]);
                    if [normal.x, normal.y, normal.z].iter().all(|v| v.is_finite()) {
                        for vertex in tri {
                            fallback_normals[vertex.pos] += normal;
                        }
                    }
                }
                let carrier = face.surface();
                let carrier_side = if face.orientation() { 1. } else { -1. };
                let mut carrier_normals = std::collections::HashMap::new();
                for tri in poly.tri_faces() {
                    let p = tri.map(|v| poly.positions()[v.pos]);
                    let n = (p[1] - p[0]).cross(p[2] - p[0]);
                    area += n.magnitude() * 0.5;
                    if solid_volume {
                        mesh.volume_mm3 +=
                            p[0].to_vec().dot(p[1].to_vec().cross(p[2].to_vec())) / 6.;
                    }
                    let normal = unit_normal(n).unwrap_or_else(Vector3::unit_z);
                    for (vertex, p) in tri.iter().zip(p) {
                        // Monstertruck's mesh UVs parameterize face.surface(),
                        // not its inverted oriented_surface(). Preserve that
                        // parameter frame and apply the face orientation once.
                        // The stored tessellation normal may be missing,
                        // singular, unnormalized or reversed after inversion.
                        let native = vertex.uv.and_then(|index| {
                            *carrier_normals.entry(index).or_insert_with(|| {
                                let uv = poly.uv_coords().get(index)?;
                                if !uv.x.is_finite() || !uv.y.is_finite() {
                                    return None;
                                }
                                unit_normal(carrier.normal(uv.x, uv.y) * carrier_side)
                            })
                        });
                        let native = native.filter(|_| {
                            vertex.uv.is_some_and(|index| {
                                let uv = poly.uv_coords()[index];
                                let expected = carrier.subs(uv.x, uv.y);
                                expected.distance(p) <= (tolerance * 2.).max(1e-7)
                            })
                        });
                        let stored = vertex
                            .nor
                            .and_then(|index| poly.normals().get(index))
                            .and_then(unit_normal)
                            .map(|value| {
                                if value.dot(normal) < 0. {
                                    -value
                                } else {
                                    value
                                }
                            });
                        let n = native
                            .or(stored)
                            .or_else(|| unit_normal(fallback_normals[vertex.pos]))
                            .unwrap_or(normal);
                        mesh.indices.push(mesh.positions.len() as u32);
                        mesh.positions.push([p.x, p.y, p.z]);
                        mesh.normals.push([n.x, n.y, n.z]);
                    }
                }
                mesh.faces.push(FaceRange {
                    body: Some(producer.clone()),
                    body_kind: if solid_volume {
                        cad_core::protocol::BodyKind::Solid
                    } else {
                        cad_core::protocol::BodyKind::Surface
                    },
                    start,
                    count: mesh.indices.len() - start,
                    plane: if matches!(face.surface(), Surface::Plane(_)) {
                        Some(cad_plane(face))
                    } else {
                        None
                    },
                    reference: TopologyRef {
                        producer: self
                            .histories
                            .borrow()
                            .get(producer)
                            .and_then(|h| h.faces.get(&face.id()))
                            .map(|r| r.producer.clone())
                            .unwrap_or_else(|| producer.clone()),
                        role: self
                            .histories
                            .borrow()
                            .get(producer)
                            .and_then(|h| h.faces.get(&face.id()))
                            .map(|r| r.role.clone())
                            .unwrap_or_else(|| format!("face:{}", role(center, b))),
                        topology: "face".into(),
                        signature: Some(TopologySignature {
                            center,
                            measure: area,
                            tolerance: 1e-5,
                        }),
                    },
                });
            }
            let mut seen_edges = std::collections::HashSet::new();
            let mut native_boundaries =
                std::collections::HashMap::<_, Vec<cad_core::protocol::NativeEdgeBoundary>>::new();
            let histories = self.histories.borrow();
            let history = histories.get(producer);
            for face in shell {
                for wire in face.absolute_boundaries() {
                    if selection_cache {
                        boundary_token += 1;
                    }
                    for edge in wire {
                        if let Some(reference) =
                            history.and_then(|history| history.faces.get(&face.id()))
                        {
                            native_boundaries.entry(edge.id()).or_default().push(
                                cad_core::protocol::NativeEdgeBoundary {
                                    boundary: boundary_token,
                                    face: reference.clone(),
                                },
                            );
                        }
                    }
                }
            }
            for edge in shell.edge_iter().filter(|e| seen_edges.insert(e.id())) {
                // Retained edge direction survives removal/reordering of faces.
                let c = edge.curve();
                let points = crate::curve_sampling::display_points(&c, tolerance)?;
                let signature = edge_signature(&edge)?;
                let boundaries = native_boundaries.remove(&edge.id()).unwrap_or_default();
                let mut faces = boundaries
                    .iter()
                    .map(|boundary| boundary.face.clone())
                    .collect::<Vec<_>>();
                faces.sort_by(|a, b| (&a.producer, &a.role).cmp(&(&b.producer, &b.role)));
                faces.dedup_by(|a, b| a.producer == b.producer && a.role == b.role);
                let native = if selection_cache
                    && history
                        .and_then(|history| history.edges.get(&edge.id()))
                        .is_some()
                {
                    let vertices = [edge.absolute_front(), edge.absolute_back()];
                    let mut tokens = [0u32; 2];
                    for (index, vertex) in vertices.iter().enumerate() {
                        let next = native_vertices.len() as u32;
                        tokens[index] = *native_vertices.entry(vertex.id()).or_insert(next);
                    }
                    Some(cad_core::protocol::NativeEdgeTopology {
                        version: cad_core::protocol::NATIVE_SELECTION_VERSION,
                        component: component as u32,
                        vertices: tokens,
                        endpoints: vertices.map(|vertex| {
                            let point = vertex.point();
                            [point.x, point.y, point.z]
                        }),
                        tangents: selection_tangents(&c),
                        boundaries,
                    })
                } else {
                    None
                };
                mesh.edges.push(EdgeData {
                    body: Some(producer.clone()),
                    points,
                    curve: exact_curve(&c),
                    native,
                    center: edge_center(&c),
                    faces,
                    reference: TopologyRef {
                        producer: self
                            .histories
                            .borrow()
                            .get(producer)
                            .and_then(|h| h.edges.get(&edge.id()))
                            .map(|r| r.producer.clone())
                            .unwrap_or_else(|| producer.clone()),
                        role: self
                            .histories
                            .borrow()
                            .get(producer)
                            .and_then(|h| h.edges.get(&edge.id()))
                            .map(|r| r.role.clone())
                            .unwrap_or_else(|| format!("edge:{}", role(signature.center, b))),
                        topology: "edge".into(),
                        signature: Some(signature),
                    },
                });
            }
        }
        for p in &mesh.positions {
            for (axis, value) in p.iter().enumerate() {
                mesh.bounds[0][axis] = mesh.bounds[0][axis].min(*value);
                mesh.bounds[1][axis] = mesh.bounds[1][axis].max(*value);
            }
        }
        mesh.volume_mm3 = mesh.volume_mm3.abs();
        if mesh.positions.is_empty() {
            mesh.bounds = [[0.; 3]; 2];
        }
        Ok(mesh)
    }
}

impl MonstertruckAdapter {
    fn load_step_table(&self, doc: &Document, resource: &str, root: &Path) -> Result<String> {
        use sha2::{Digest, Sha256};
        let resource = doc
            .resources
            .get(resource)
            .ok_or_else(|| err("Missing imported resource"))?;
        if !self.tables.borrow().contains_key(&resource.sha256) {
            let data = std::fs::read(cad_core::persistence::safe_join(root, &resource.path)?)?;
            if format!("{:x}", Sha256::digest(&data)) != resource.sha256 {
                return Err(err("Imported resource fingerprint changed"));
            }
            let source = String::from_utf8(data).map_err(err)?;
            let scale = crate::units::length_scale(&source)?;
            let table = crate::step_import::load_table(&source)?;
            self.tables
                .borrow_mut()
                .insert(resource.sha256.clone(), (table, scale));
        }
        Ok(resource.sha256.clone())
    }
    /// Resolve the complete authored selection against the input once, then
    /// partition it by native body. Unselected bodies retain their authority.
    fn edge_feature(
        &self,
        doc: &Document,
        owner: &NodeRef,
        operation: &Operation,
        inputs: &BTreeMap<NodeRef, Shape>,
        generated: &mut TopologyHistory,
    ) -> Result<Shape> {
        use cad_core::edge_features::ChamferMethod;
        let (body, references, quantity, name) = match operation {
            Operation::Fillet {
                body,
                edges,
                radius,
            } => (body, edges, radius, "Fillet"),
            Operation::Chamfer {
                body,
                edges,
                distance,
                ..
            } => (body, edges, distance, "Chamfer"),
            _ => return Err(err("Expected an edge modifier")),
        };
        cad_core::edge_features::validate_selection(references)?;
        let source = bodies(inputs, body)?;
        let selected = self.selected_boundary_edges(body, references, inputs)?;
        let selected_ids = selected
            .iter()
            .map(Edge::id)
            .collect::<std::collections::HashSet<_>>();
        // A repeated source object in two outputs cannot provide unambiguous
        // body ownership. Do not modify both merely because its IDs coincide.
        for id in &selected_ids {
            let owners = source
                .iter()
                .filter(|solid| solid.edge_iter().any(|edge| edge.id() == *id))
                .count();
            if owners != 1 {
                return Err(CadError::Unsupported(format!(
                    "{name}: A selected edge has {owners} input-body owners; choose an unambiguous body output"
                )));
            }
        }
        let size = positive(expression::value(
            doc,
            &owner.definition,
            quantity,
            Dimension::Length,
        )?)?;
        let specification = if let Operation::Chamfer { options, .. } = operation {
            // Inactive method fields remain authored but do not affect an
            // equal-distance operation after switching from another method.
            let first_face = if options.method != ChamferMethod::EqualDistance {
                options
                    .first_face
                    .as_ref()
                    .map(|reference| {
                        self.selected_boundary_face(body, reference, inputs)
                            .map(|face| face.id())
                    })
                    .transpose()?
            } else {
                None
            };
            Some(crate::planar::ChamferSpec {
                distance: size,
                second_distance: (options.method == ChamferMethod::TwoDistances)
                    .then(|| {
                        expression::value(
                            doc,
                            &owner.definition,
                            &options.second_distance,
                            Dimension::Length,
                        )
                        .and_then(positive)
                    })
                    .transpose()?,
                angle: (options.method == ChamferMethod::DistanceAngle)
                    .then(|| {
                        expression::value(doc, &owner.definition, &options.angle, Dimension::Angle)
                    })
                    .transpose()?,
                flip: options.flip,
                first_face,
            })
        } else {
            None
        };
        let mut result = Vec::with_capacity(source.len());
        for solid in source {
            let mut seen = std::collections::HashSet::new();
            let local = solid
                .edge_iter()
                .filter(|edge| selected_ids.contains(&edge.id()) && seen.insert(edge.id()))
                .map(|edge| edge.id())
                .collect::<Vec<_>>();
            if local.is_empty() {
                result.push(solid.clone());
                continue;
            }
            let mut history = crate::shell_history::ShellHistory::new_operation(
                self,
                owner,
                body,
                solid,
                if specification.is_some() {
                    "chamfer"
                } else {
                    "fillet"
                },
            )?;
            let output = if let Some(specification) = &specification {
                crate::general_chamfer::chamfer(solid, &local, specification, &mut history)?
            } else {
                crate::general_round::round(solid, &local, size, &mut history)?
            }.ok_or_else(|| CadError::Unsupported(format!(
                "{name}: This selection needs an unresolved carrier transition or corner junction; no complete native blend can be constructed for the selected edges"
            )))?;
            crate::validation::solid(&output)?;
            generated.merge(history.finish(std::slice::from_ref(&output))?);
            result.push(output);
        }
        Ok(Shape::Bodies(result))
    }
    pub(crate) fn register_weld_history(&self, owner: &NodeRef, history: TopologyHistory) {
        self.histories.borrow_mut().insert(owner.clone(), history);
    }
    pub(crate) fn register_weld_joint(&self, preview: cad_core::weld_beads::JointPreview) {
        self.weld_joints
            .borrow_mut()
            .insert(preview.owner.clone(), preview);
    }
    pub(crate) fn face_source(&self, body: &NodeRef, face: FaceId) -> Option<TopologyRef> {
        self.histories
            .borrow()
            .get(body)
            .and_then(|history| history.face_source(face))
    }
    pub(crate) fn edge_source(&self, body: &NodeRef, edge: EdgeId) -> Option<TopologyRef> {
        self.histories
            .borrow()
            .get(body)
            .and_then(|history| history.edges.get(&edge).cloned())
    }
    pub(crate) fn body_construction_origin(
        &self,
        body: &NodeRef,
        solid: &Solid,
    ) -> Result<TopologyRef> {
        let histories = self.histories.borrow();
        let history = histories
            .get(body)
            .ok_or_else(|| err("Source body provenance is unavailable"))?;
        solid
            .face_iter()
            .find_map(|face| history.faces.get(&face.id()))
            .cloned()
            .ok_or_else(|| err("Source body construction identity is unavailable"))
    }
    pub(crate) fn resolve_surface_point(
        &self,
        doc: &Document,
        owner: &str,
        source: &cad_core::reference_geometry::PointSource,
        inputs: &BTreeMap<NodeRef, Shape>,
    ) -> Result<[f64; 3]> {
        source.resolve(
            doc,
            owner,
            &DatumResolver {
                adapter: self,
                doc,
                inputs,
            },
        )
    }
    pub(crate) fn resolve_surface_plane(
        &self,
        doc: &Document,
        source: &cad_core::reference_geometry::PlaneSource,
        inputs: &BTreeMap<NodeRef, Shape>,
    ) -> Result<CadPlane> {
        source.resolve(&DatumResolver {
            adapter: self,
            doc,
            inputs,
        })
    }
    pub(crate) fn selected_boundary_edges(
        &self,
        body: &NodeRef,
        refs: &[TopologyRef],
        inputs: &BTreeMap<NodeRef, Shape>,
    ) -> Result<Vec<Edge>> {
        let shape = inputs.get(body).ok_or_else(|| err("Missing source body"))?;
        let histories = self.histories.borrow();
        let ids = resolve_shape_edges(shape, refs, histories.get(body))?;
        let mut seen = std::collections::HashSet::new();
        if let Shape::ReferenceCurves { paths, .. } = shape {
            return Ok(paths
                .iter()
                .flat_map(|wire| wire.iter())
                .filter(|edge| ids.contains(&edge.id()) && seen.insert(edge.id()))
                .cloned()
                .collect());
        }
        Ok(shape
            .boundary_shells()
            .ok_or_else(|| err("Select a solid or surface body"))?
            .into_iter()
            .flat_map(|s| s.edge_iter())
            .filter(|e| ids.contains(&e.id()) && seen.insert(e.id()))
            .collect())
    }
    pub(crate) fn boundary_face_reference(
        &self,
        body: &NodeRef,
        face: &Face,
    ) -> Result<TopologyRef> {
        self.histories
            .borrow()
            .get(body)
            .and_then(|h| h.faces.get(&face.id()))
            .cloned()
            .ok_or_else(|| err("Face provenance is unavailable"))
    }
    pub(crate) fn selected_boundary_face(
        &self,
        body: &NodeRef,
        reference: &TopologyRef,
        inputs: &BTreeMap<NodeRef, Shape>,
    ) -> Result<Face> {
        if reference.topology != "face" {
            return Err(err("Select a model face"));
        }
        let shape = inputs
            .get(body)
            .ok_or_else(|| err("Face source body is missing"))?;
        let history = self.histories.borrow();
        let names = history
            .get(body)
            .ok_or_else(|| err("Face source provenance is unavailable"))?;
        let wanted = construction_reference(reference)?;
        let mut seen = std::collections::HashSet::new();
        let mut faces = Vec::new();
        for face in shape
            .boundary_shells()
            .ok_or_else(|| err("Select a solid or surface body"))?
            .into_iter()
            .flat_map(|shell| shell.iter())
        {
            if !seen.insert(face.id()) {
                continue;
            }
            if let Some(source) = names.faces.get(&face.id())
                && matches_reference(source, reference, &wanted)?
            {
                faces.push(face);
            }
        }
        if faces.len() != 1 {
            return Err(err(
                "Face reference is missing or ambiguous; select an explicit replacement",
            ));
        }
        Ok(faces[0].clone())
    }
    pub(crate) fn boundary_edge_reference(
        &self,
        body: &NodeRef,
        edge: &Edge,
    ) -> Result<TopologyRef> {
        self.histories
            .borrow()
            .get(body)
            .and_then(|h| h.edges.get(&edge.id()))
            .cloned()
            .ok_or_else(|| err("Edge provenance is unavailable"))
    }
    pub(crate) fn resolve_body_scale(
        &self,
        doc: &Document,
        owner: &str,
        options: &cad_core::body_editing::ScaleOptions,
        inputs: &BTreeMap<NodeRef, Shape>,
    ) -> Result<cad_core::body_editing::AffineScale> {
        options.resolve(
            doc,
            owner,
            &DatumResolver {
                adapter: self,
                doc,
                inputs,
            },
        )
    }
    pub(crate) fn resolve_surface_axis(
        &self,
        doc: &Document,
        source: &cad_core::reference_geometry::AxisSource,
        inputs: &BTreeMap<NodeRef, Shape>,
    ) -> Result<cad_core::reference_geometry::Axis> {
        source.resolve(&DatumResolver {
            adapter: self,
            doc,
            inputs,
        })
    }
    pub(crate) fn resolve_surface_direction(
        &self,
        doc: &Document,
        owner: &str,
        source: &cad_core::reference_geometry::DirectionSource,
        inputs: &BTreeMap<NodeRef, Shape>,
    ) -> Result<[f64; 3]> {
        source.resolve(
            doc,
            owner,
            &DatumResolver {
                adapter: self,
                doc,
                inputs,
            },
        )
    }
    /// Preserve proven carrier ancestry between native Boolean steps. A later
    /// contact join must be able to reference faces created by an earlier join
    /// in the same feature, before the final document history is published.
    /// This records only existing identities and unique native carrier origins;
    /// ambiguous or unrecognized ancestry is never guessed from position.
    pub(crate) fn carry_boolean_history(
        &self,
        owner: &NodeRef,
        scope: &BTreeSet<NodeRef>,
        inputs: [&Solid; 2],
        output: &Solid,
        generated: &mut TopologyHistory,
    ) -> Result<()> {
        self.carry_boolean_results(owner, scope, inputs, &[output], generated)
    }

    pub(crate) fn carry_boolean_results(
        &self,
        owner: &NodeRef,
        scope: &BTreeSet<NodeRef>,
        inputs: [&Solid; 2],
        outputs: &[&Solid],
        generated: &mut TopologyHistory,
    ) -> Result<()> {
        let previous_size = history_size(generated);
        if previous_size > MAX_HISTORY_BYTES {
            return Err(budget_error());
        }
        let mut budget = HistoryBudget::new();
        let mut seen_faces = std::collections::HashSet::new();
        let mut seen_edges = std::collections::HashSet::new();
        let histories = self.histories.borrow();
        let mut inherited = TopologyHistory::default();
        let mut curves = BTreeMap::<String, Vec<TopologyRef>>::new();
        let mut surfaces = BTreeMap::<String, Vec<TopologyRef>>::new();
        let mut unresolved_curves = generated.unresolved_curves.clone();
        let mut unresolved_surfaces = generated.unresolved_surfaces.clone();
        for body in inputs {
            for face in body.face_iter() {
                if !seen_faces.insert(face.id()) {
                    continue;
                }
                if seen_faces.len() > 16_384 {
                    return Err(component_history::face_limit_error());
                }
                let carrier = budget.encoded(&face.surface())?;
                let source = generated.face_source(face.id()).or_else(|| {
                    scope
                        .iter()
                        .find_map(|reference| histories.get(reference)?.face_source(face.id()))
                });
                if let Some(source) = source {
                    budget.reference(&source)?;
                    inherited.inherit_face(face.id(), source.clone());
                    surfaces.entry(carrier).or_default().push(source);
                } else {
                    unresolved_surfaces.insert(carrier);
                }
            }
            for edge in body.edge_iter() {
                if !seen_edges.insert(edge.id()) {
                    continue;
                }
                if seen_edges.len() > 65_536 {
                    return Err(component_history::edge_limit_error());
                }
                let carrier = budget.encoded(&edge.curve())?;
                let source = generated.edge_source(edge.id()).or_else(|| {
                    scope
                        .iter()
                        .find_map(|reference| histories.get(reference)?.edge_source(edge.id()))
                });
                if let Some(source) = source {
                    budget.reference(&source)?;
                    inherited.inherit_edge(edge.id(), source.clone());
                    curves.entry(carrier).or_default().push(source);
                } else {
                    unresolved_curves.insert(carrier);
                }
            }
        }
        // A known contributor does not establish unique ancestry when another
        // entity on that exact carrier has no resolved source identity.
        surfaces.retain(|carrier, _| !unresolved_surfaces.contains(carrier));
        curves.retain(|carrier, _| !unresolved_curves.contains(carrier));
        let mut pending = TopologyHistory::default();
        let mut output_faces = std::collections::HashMap::new();
        let mut output_edges = std::collections::HashMap::new();
        seen_faces.clear();
        seen_edges.clear();
        for face in outputs.iter().flat_map(|body| body.face_iter()) {
            if !seen_faces.insert(face.id()) {
                continue;
            }
            if seen_faces.len() > 16_384 {
                return Err(component_history::face_limit_error());
            }
            let carrier = budget.encoded(&face.surface())?;
            output_faces.insert(face.id(), carrier.clone());
            if let Some(source) = generated
                .face_source(face.id())
                .or_else(|| inherited.face_source(face.id()))
                .or_else(|| unique_origin(surfaces.get(&carrier)))
            {
                budget.reference(&source)?;
                pending.inherit_face(face.id(), source);
            }
        }
        for edge in outputs.iter().flat_map(|body| body.edge_iter()) {
            if !seen_edges.insert(edge.id()) {
                continue;
            }
            if seen_edges.len() > 65_536 {
                return Err(component_history::edge_limit_error());
            }
            let curve = edge.curve();
            let carrier = budget.encoded(&curve)?;
            output_edges.insert(edge.id(), carrier.clone());
            let source = generated
                .edge_source(edge.id())
                .or_else(|| inherited.edge_source(edge.id()))
                .or_else(|| unique_origin(curves.get(&carrier)));
            let source = match source {
                Some(source) => Some(source),
                None => carrier_intersection_origin(owner, &curve, &surfaces, &mut budget)?,
            };
            if let Some(source) = source {
                budget.reference(&source)?;
                pending.inherit_edge(edge.id(), source);
            }
        }
        let originals = inputs
            .iter()
            .flat_map(|body| body.boundaries())
            .collect::<Vec<_>>();
        let shells = outputs
            .iter()
            .flat_map(|body| body.boundaries())
            .collect::<Vec<_>>();
        component_history::resolve(
            owner,
            &shells,
            &originals,
            &inherited.edges,
            &inherited.faces,
            &mut pending,
            &mut budget,
        )?;
        for (id, carrier) in output_faces {
            if let Some(source) = pending.faces.get(&id) {
                pending
                    .generated_surfaces
                    .entry(carrier)
                    .or_default()
                    .push(source.clone());
            } else {
                unresolved_surfaces.insert(carrier);
            }
        }
        for (id, carrier) in output_edges {
            if let Some(source) = pending.edges.get(&id) {
                pending
                    .generated_curves
                    .entry(carrier)
                    .or_default()
                    .push(source.clone());
            } else {
                unresolved_curves.insert(carrier);
            }
        }
        pending.unresolved_curves = unresolved_curves;
        pending.unresolved_surfaces = unresolved_surfaces;
        if previous_size.saturating_add(history_size(&pending)) > MAX_HISTORY_BYTES {
            return Err(budget_error());
        }
        generated.merge(pending);
        Ok(())
    }

    fn record_history(
        &self,
        owner: &NodeRef,
        op: &Operation,
        result: &Shape,
        inputs: &BTreeMap<NodeRef, Shape>,
        mapped_input: Option<(&NodeRef, Matrix4)>,
        generated: &TopologyHistory,
    ) -> Result<()> {
        if history_size(generated) > MAX_HISTORY_BYTES {
            return Err(budget_error());
        }
        if matches!(result, Shape::ReferenceCurves { .. }) {
            self.histories
                .borrow_mut()
                .insert(owner.clone(), generated.clone());
            return Ok(());
        }
        let mut budget = HistoryBudget::new();
        let mut history = TopologyHistory::default();
        let histories = self.histories.borrow();
        let mut inherited_edges = std::collections::HashMap::new();
        let mut inherited_faces = std::collections::HashMap::new();
        let mut curve_origins = generated.generated_curves.clone();
        let mut surface_origins = generated.generated_surfaces.clone();
        let mut unresolved_curves = generated.unresolved_curves.clone();
        let mut unresolved_surfaces = generated.unresolved_surfaces.clone();
        let mut originals = Vec::new();
        for input in op.dependencies() {
            let Some(shells) = inputs.get(&input).and_then(Shape::boundary_shells) else {
                continue;
            };
            let h = histories.get(&input);
            let mut seen_edges = std::collections::HashSet::new();
            let mut seen_faces = std::collections::HashSet::new();
            for shell in shells {
                originals.push(shell);
                for edge in shell.edge_iter() {
                    budget.charge(1)?;
                    if !seen_edges.insert(edge.id()) {
                        continue;
                    }
                    let curve = match mapped_input {
                        Some((source, m)) if source == &input => edge.curve().transformed(m),
                        _ => edge.curve(),
                    };
                    let carrier = budget.encoded(&curve)?;
                    if let Some(r) = h.and_then(|h| h.edges.get(&edge.id())) {
                        budget.reference(r)?;
                        inherited_edges.insert(edge.id(), r.clone());
                        curve_origins.entry(carrier).or_default().push(r.clone());
                    } else {
                        unresolved_curves.insert(carrier);
                    }
                }
                for face in shell.iter() {
                    budget.charge(1)?;
                    if !seen_faces.insert(face.id()) {
                        continue;
                    }
                    let surface = match mapped_input {
                        Some((source, m)) if source == &input => face.surface().transformed(m),
                        _ => face.surface(),
                    };
                    let carrier = budget.encoded(&surface)?;
                    if let Some(r) = h.and_then(|h| h.faces.get(&face.id())) {
                        budget.reference(r)?;
                        inherited_faces.insert(face.id(), r.clone());
                        surface_origins.entry(carrier).or_default().push(r.clone());
                    } else {
                        unresolved_surfaces.insert(carrier);
                    }
                }
            }
        }
        curve_origins.retain(|carrier, _| !unresolved_curves.contains(carrier));
        surface_origins.retain(|carrier, _| !unresolved_surfaces.contains(carrier));
        if let Some(shells) = result.boundary_shells() {
            let scale_copy =
                matches!(op, Operation::Scale { options, .. } if options.keep_original);
            let mut seen_edges = std::collections::HashSet::new();
            let mut seen_faces = std::collections::HashSet::new();
            // Collect only proven source ancestry. Current-operation generic
            // roles are assigned after component resolution and cannot anchor it.
            for shell in &shells {
                for edge in shell.edge_iter() {
                    budget.charge(1)?;
                    if !seen_edges.insert(edge.id()) {
                        continue;
                    }
                    let curve = edge.curve();
                    let geometry = budget.encoded(&curve)?;
                    let source = generated.edges.get(&edge.id()).cloned().or_else(|| {
                        inherited_edges.get(&edge.id()).cloned().or_else(|| {
                            (!scale_copy)
                                .then(|| unique_origin(curve_origins.get(&geometry)))
                                .flatten()
                        })
                    });
                    let source = match source {
                        Some(source) => Some(source),
                        None => carrier_intersection_origin(
                            owner,
                            &curve,
                            &surface_origins,
                            &mut budget,
                        )?,
                    };
                    if let Some(source) = source {
                        budget.reference(&source)?;
                        history.inherit_edge(edge.id(), source);
                    }
                }
                for face in shell.iter() {
                    budget.charge(1)?;
                    if !seen_faces.insert(face.id()) {
                        continue;
                    }
                    let geometry = budget.encoded(&face.surface())?;
                    let source = generated.faces.get(&face.id()).cloned().or_else(|| {
                        inherited_faces.get(&face.id()).cloned().or_else(|| {
                            (!scale_copy)
                                .then(|| unique_origin(surface_origins.get(&geometry)))
                                .flatten()
                        })
                    });
                    if let Some(source) = source {
                        // A shell opening rim is a new face even on the
                        // removed carrier. Legacy saved source roles compare
                        // through the bounded canonical closure as well.
                        let mut removed_source = false;
                        if let Operation::Shell { removed_faces, .. } = op {
                            let wanted = construction_reference(&source)?;
                            for removed in removed_faces {
                                if matches_reference(removed, &source, &wanted)? {
                                    removed_source = true;
                                    break;
                                }
                            }
                        }
                        if !removed_source {
                            budget.reference(&source)?;
                            history.inherit_face(face.id(), source);
                        }
                    }
                }
            }
            component_history::resolve(
                owner,
                &shells,
                &originals,
                &inherited_edges,
                &inherited_faces,
                &mut history,
                &mut budget,
            )?;
            for (shell_index, shell) in shells.into_iter().enumerate() {
                let bounds = shell_bounds(shell);
                for edge in shell.edge_iter() {
                    if history.edges.contains_key(&edge.id()) {
                        continue;
                    }
                    let signature = edge_signature(&edge)?;
                    history.inherit_edge(
                        edge.id(),
                        TopologyRef {
                            producer: owner.clone(),
                            role: if matches!(op, Operation::Scale { .. }) {
                                format!(
                                    "edge:scale:body:{shell_index}:{}",
                                    role(signature.center, bounds)
                                )
                            } else {
                                format!(
                                    "edge:{}:{}",
                                    op.name().to_lowercase(),
                                    role(signature.center, bounds)
                                )
                            },
                            topology: "edge".into(),
                            signature: None,
                        },
                    );
                }
                for face in shell.iter() {
                    if history.faces.contains_key(&face.id()) {
                        continue;
                    }
                    let points = face.vertex_iter().map(|v| v.point()).collect::<Vec<_>>();
                    let center = points
                        .iter()
                        .fold(Vector3::zero(), |sum, p| sum + p.to_vec())
                        / points.len().max(1) as f64;
                    history.inherit_face(
                        face.id(),
                        TopologyRef {
                            producer: owner.clone(),
                            role: if matches!(op, Operation::Scale { .. }) {
                                format!(
                                    "face:scale:body:{shell_index}:{}",
                                    role([center.x, center.y, center.z], bounds)
                                )
                            } else {
                                format!(
                                    "face:{}:{}",
                                    op.name().to_lowercase(),
                                    role([center.x, center.y, center.z], bounds)
                                )
                            },
                            topology: "face".into(),
                            signature: None,
                        },
                    );
                }
            }
            component_history::verify_unique(&history, &mut budget)?;
        }
        drop(histories);
        self.histories.borrow_mut().insert(owner.clone(), history);
        Ok(())
    }
}
fn carrier_intersection_origin(
    owner: &NodeRef,
    curve: &Curve,
    surfaces: &BTreeMap<String, Vec<TopologyRef>>,
    budget: &mut HistoryBudget,
) -> Result<Option<TopologyRef>> {
    let Curve::IntersectionCurve(intersection) = curve else {
        return Ok(None);
    };
    let mut lookup = |surface: &Surface| -> Result<Option<TopologyRef>> {
        let origins = surfaces.get(&budget.encoded(surface)?);
        let origin = unique_origin(origins);
        if origin.is_none()
            && origins.is_some_and(|refs| {
                refs.iter().any(|r| {
                    r.role.starts_with("construction:")
                        || (r.producer == *owner && r.role.starts_with("hole-position:"))
                })
            })
        {
            return Err(CadError::Unsupported(
                "Boolean intersection has ambiguous authored carrier ancestry; select or rebuild unambiguous source geometry"
                    .into(),
            ));
        }
        Ok(origin)
    };
    let first = lookup(intersection.surface0().as_ref())?;
    let second = lookup(intersection.surface1().as_ref())?;
    let (Some(first), Some(second)) = (first, second) else {
        return Ok(None);
    };
    let is_hole = |r: &TopologyRef| r.producer == *owner && r.role.starts_with("hole-position:");
    let hole = is_hole(&first) || is_hole(&second);
    if !hole
        && ![&first, &second]
            .iter()
            .any(|r| r.role.starts_with("construction:"))
    {
        return Ok(None);
    }
    let mut carriers = [
        serde_json::from_str::<serde_json::Value>(&budget.reference(&first)?)?,
        serde_json::from_str::<serde_json::Value>(&budget.reference(&second)?)?,
    ];
    carriers.sort_by_key(serde_json::Value::to_string);
    let payload = if hole {
        serde_json::json!({"carriers":carriers})
    } else {
        serde_json::json!({
            "sources":carriers,
            "purpose":{"operation":"boolean","constructor":"carrier_intersection"},
            "topology":"edge"
        })
    };
    let payload = budget.encoded(&cad_core::provenance::construction_payload(payload)?)?;
    Ok(Some(TopologyRef {
        producer: owner.clone(),
        role: if hole {
            format!("hole-intersection:{payload}")
        } else {
            format!("construction:{payload}")
        },
        topology: "edge".into(),
        signature: None,
    }))
}

fn unique_origin(origins: Option<&Vec<TopologyRef>>) -> Option<TopologyRef> {
    let origins = origins?;
    let first = origins.first()?;
    origins
        .iter()
        .all(|r| r.producer == first.producer && r.role == first.role)
        .then(|| first.clone())
}

pub(crate) fn cad_plane(face: &Face) -> CadPlane {
    let surface = face.oriented_surface();
    let origin = surface.subs(0., 0.);
    let u = surface.uder(0., 0.).normalize();
    let normal = surface.normal(0., 0.).normalize();
    let v = normal.cross(u);
    CadPlane {
        origin: [origin.x, origin.y, origin.z],
        u: [u.x, u.y, u.z],
        v: [v.x, v.y, v.z],
    }
}
impl MonstertruckAdapter {
    fn reference_plane(
        &self,
        reference: &TopologyRef,
        reference_body: Option<&NodeRef>,
        inputs: &BTreeMap<NodeRef, Shape>,
    ) -> Result<CadPlane> {
        let histories = self.histories.borrow();
        let wanted = construction_reference(reference)?;
        let mut matches = vec![];
        for (key, shape) in inputs
            .iter()
            .filter(|(key, _)| *key == reference_body.unwrap_or(&reference.producer))
        {
            if let (Some(history), Some(shells)) = (histories.get(key), shape.boundary_shells()) {
                for shell in shells {
                    for face in shell.iter() {
                        if let Some(source) = history.faces.get(&face.id())
                            && matches_reference(source, reference, &wanted)?
                            && matches!(face.surface(), Surface::Plane(_))
                            && !matches.iter().any(|f: &Face| f.id() == face.id())
                        {
                            matches.push(face.clone());
                        }
                    }
                }
            }
        }
        if matches.len() != 1 {
            return Err(err(format!(
                "Planar reference {} has {} candidates",
                reference.role,
                matches.len()
            )));
        }
        Ok(cad_plane(&matches[0]))
    }
}

struct DatumResolver<'a> {
    adapter: &'a MonstertruckAdapter,
    doc: &'a Document,
    inputs: &'a BTreeMap<NodeRef, Shape>,
}
impl cad_core::reference_geometry::Resolver for DatumResolver<'_> {
    fn tangent_plane(
        &self,
        reference: &TopologyRef,
        body: &NodeRef,
        anchor: cad_core::reference_geometry::TangentAnchorValue,
    ) -> Result<CadPlane> {
        let face = self
            .adapter
            .selected_boundary_face(body, reference, self.inputs)?;
        let origin = self
            .adapter
            .face_source(body, face.id())
            .ok_or_else(|| err("Tangent plane face has no construction provenance"))?;
        if !cad_core::provenance::equivalent(&origin, reference)? {
            return Err(err(
                "Tangent plane face did not resolve by its saved construction identity",
            ));
        }
        crate::tangent_plane::at(&face, anchor)
    }
    fn mate_profile_center(&self, node: &NodeRef) -> Result<cad_core::assembly_geometry::Frame> {
        let face = profile(self.inputs, node)?;
        crate::mate_geometry::profile_center(face)
    }
    fn mate_face_center(
        &self,
        reference: &TopologyRef,
        body: &NodeRef,
    ) -> Result<cad_core::assembly_geometry::Frame> {
        crate::mate_geometry::profile_center(&self.adapter.selected_boundary_face(
            body,
            reference,
            self.inputs,
        )?)
    }
    fn mate_edge_curve(
        &self,
        reference: &TopologyRef,
        body: &NodeRef,
    ) -> Result<cad_core::assembly_geometry::Curve> {
        let edges = self.adapter.selected_boundary_edges(
            body,
            std::slice::from_ref(reference),
            self.inputs,
        )?;
        if edges.len() != 1 {
            return Err(err("Path edge is missing or ambiguous"));
        }
        crate::mate_geometry::edge_curve(&edges[0])
    }

    fn mate_curve(
        &self,
        profile: &NodeRef,
        member: &str,
    ) -> Result<cad_core::assembly_geometry::Curve> {
        use cad_core::assembly_geometry::{Curve, CurveGeometry};
        let part = self
            .doc
            .parts
            .get(&profile.definition)
            .ok_or_else(|| err("Path part is missing"))?;
        let feature = part
            .features
            .iter()
            .find(|f| f.id == profile.node && !f.suppressed)
            .ok_or_else(|| err("Path sketch is missing or suppressed"))?;
        let Operation::Profile {
            profile: Profile::Sketch { sketch },
            plane,
            plane_reference,
            reference_body,
        } = &feature.operation
        else {
            return Err(err("Path mates require a sketch member"));
        };
        let plane = self.adapter.sketch_plane(
            plane,
            plane_reference.as_ref(),
            reference_body.as_ref(),
            self.inputs,
        )?;
        let source = part
            .sketches
            .get(sketch)
            .ok_or_else(|| err("Path sketch is missing"))?;
        if cad_core::sketch_transform::suppressed_members(source).contains(member) {
            return Err(err("Referenced path sketch member is suppressed"));
        }
        let mut solved = self.adapter.prepare_sketch(source, &plane, self.inputs)?;
        use cad_core::solver::ConstraintSolver;
        cad_core::solver::NativeSolver.solve_sketch(self.doc, &profile.definition, &mut solved)?;
        let get = |id: &str| {
            solved
                .points
                .get(id)
                .copied()
                .ok_or_else(|| err("Path sketch point is missing"))
        };
        let curve = if let Some(circle) = solved.circles.get(member) {
            cad_core::SketchCurve::Circle {
                center: circle.center.clone(),
                radius: circle.radius.clone(),
            }
        } else {
            cad_core::sketch_edit::effective_curve(&solved, member)?
        };
        let geometry = match curve {
            cad_core::SketchCurve::Line { start, end } => CurveGeometry::Line {
                start: get(&start)?,
                end: get(&end)?,
            },
            cad_core::SketchCurve::Circle { center, radius } => CurveGeometry::Circle {
                center: get(&center)?,
                radius: expression::value(
                    self.doc,
                    &profile.definition,
                    &radius,
                    Dimension::Length,
                )?,
                start: 0.,
                sweep: std::f64::consts::TAU,
            },
            cad_core::SketchCurve::Arc {
                start,
                through,
                end,
            } => {
                let (center, radius, start, sweep) =
                    cad_core::sketch_geometry::arc_frame(get(&start)?, get(&through)?, get(&end)?)?;
                CurveGeometry::Circle {
                    center,
                    radius,
                    start,
                    sweep,
                }
            }
            cad_core::SketchCurve::Ellipse {
                center,
                major,
                minor,
                angle,
            } => CurveGeometry::Ellipse {
                center: get(&center)?,
                major: expression::value(self.doc, &profile.definition, &major, Dimension::Length)?,
                minor: expression::value(self.doc, &profile.definition, &minor, Dimension::Length)?,
                angle: expression::value(self.doc, &profile.definition, &angle, Dimension::Angle)?,
            },
            cad_core::SketchCurve::Bezier { points } => {
                if points.len() < 2 {
                    return Err(err("Path Bezier needs at least two controls"));
                }
                let degree = points.len() - 1;
                let n = points.len();
                CurveGeometry::Nurbs {
                    curve: cad_core::sketch_edit::Curve2 {
                        control_points: points.iter().map(|p| get(p)).collect::<Result<_>>()?,
                        weights: vec![1.; n],
                        knots: vec![0.; degree + 1]
                            .into_iter()
                            .chain(vec![1.; degree + 1])
                            .collect(),
                        degree,
                    },
                }
            }
            cad_core::SketchCurve::Nurbs {
                points,
                weights,
                knots,
                degree,
            } => CurveGeometry::Nurbs {
                curve: cad_core::sketch_edit::Curve2 {
                    control_points: points.iter().map(|p| get(p)).collect::<Result<_>>()?,
                    weights,
                    knots,
                    degree,
                },
            },
        };
        match &geometry {
            CurveGeometry::Circle { radius, .. } if !radius.is_finite() || *radius <= 0. => {
                return Err(err("Path radius must be positive and finite"));
            }
            CurveGeometry::Ellipse {
                major,
                minor,
                angle,
                ..
            } if !major.is_finite()
                || !minor.is_finite()
                || !angle.is_finite()
                || *major <= 0.
                || *minor <= 0. =>
            {
                return Err(err("Path ellipse axes must be positive and finite"));
            }
            CurveGeometry::Nurbs { curve } => cad_core::sketch_edit::validate_nurbs(curve)?,
            _ => {}
        }
        let curve = Curve {
            plane,
            geometry,
            slot_carrier: None,
        };
        curve.point(0.)?;
        if matches!(curve.geometry, CurveGeometry::Line { .. }) {
            curve.tangent(0.)?;
        }
        Ok(curve)
    }
    fn datum(&self, feature: &NodeRef) -> Result<cad_core::reference_geometry::Geometry> {
        if self
            .doc
            .parts
            .get(&feature.definition)
            .and_then(|p| p.features.iter().find(|f| f.id == feature.node))
            .is_none_or(|f| f.suppressed)
        {
            return Err(err("Reference feature is missing or suppressed"));
        }
        match self.inputs.get(feature) {
            Some(Shape::Reference(geometry)) => Ok(geometry.clone()),
            _ => Err(err(
                "Reference feature is missing, suppressed, or has the wrong type",
            )),
        }
    }
    fn face_plane(&self, reference: &TopologyRef, body: &NodeRef) -> Result<CadPlane> {
        if reference.topology != "face" {
            return Err(err("A plane reference requires a planar face"));
        }
        self.adapter
            .reference_plane(reference, Some(body), self.inputs)
    }
    fn edge_axis(
        &self,
        reference: &TopologyRef,
        body: &NodeRef,
    ) -> Result<cad_core::reference_geometry::Axis> {
        if reference.topology != "edge" {
            return Err(err("An axis reference requires a straight edge"));
        }
        let edges = self.adapter.selected_boundary_edges(
            body,
            std::slice::from_ref(reference),
            self.inputs,
        )?;
        if edges.len() != 1 {
            return Err(err(
                "Axis edge is missing or ambiguous; select an explicit replacement",
            ));
        }
        let edge = &edges[0];
        crate::axis_geometry::line_axis(&edge.oriented_curve())
    }
    fn edge_point(
        &self,
        reference: &TopologyRef,
        body: &NodeRef,
        parameter: f64,
    ) -> Result<[f64; 3]> {
        if reference.topology != "edge"
            || !parameter.is_finite()
            || !(0. ..=1.).contains(&parameter)
        {
            return Err(err(
                "Reference Point requires an edge and a finite native parameter in 0..1",
            ));
        }
        let edges = self.adapter.selected_boundary_edges(
            body,
            std::slice::from_ref(reference),
            self.inputs,
        )?;
        if edges.len() != 1 {
            return Err(err(
                "Reference Point edge is missing or ambiguous; select an explicit replacement",
            ));
        }
        // The stored edge carrier and absolute vertices retain construction
        // orientation; the first incident face may use that edge in reverse.
        let edge = edges[0].absolute_clone();
        let origin = self
            .adapter
            .edge_source(body, edge.id())
            .ok_or_else(|| err("Reference Point edge has no construction provenance"))?;
        if !cad_core::provenance::equivalent(&origin, reference)? {
            return Err(err(
                "Reference Point edge did not resolve by its saved construction identity",
            ));
        }
        let curve = edge.curve();
        let (start, end) = curve.range_tuple();
        let span = end - start;
        if !start.is_finite() || !end.is_finite() || !span.is_finite() || span <= 0. {
            return Err(CadError::Unsupported(
                "Reference Point edge needs a finite nonzero native parameter interval".into(),
            ));
        }
        let finite = |p: Point3| [p.x, p.y, p.z].into_iter().all(f64::is_finite);
        let evaluate = |t: f64| crate::curve_sampling::evaluate_point(&curve, t);
        let first = evaluate(start)?;
        let last = evaluate(end)?;
        if !finite(first)
            || !finite(last)
            || !finite(edge.front().point())
            || !finite(edge.back().point())
            || first.distance(edge.front().point()) > 1e-6
            || last.distance(edge.back().point()) > 1e-6
        {
            return Err(err(
                "Reference Point carrier endpoints are nonfinite or inconsistent with its stored construction orientation",
            ));
        }
        let point = if parameter == 0. {
            edge.front().point()
        } else if parameter == 1. {
            edge.back().point()
        } else {
            evaluate(start + span * parameter)?
        };
        if !finite(point) {
            return Err(err("Reference Point curve evaluation is nonfinite"));
        }
        Ok([point.x, point.y, point.z])
    }
    fn face_axis(
        &self,
        reference: &TopologyRef,
        body: &NodeRef,
    ) -> Result<cad_core::reference_geometry::Axis> {
        crate::axis_geometry::face_axis(&self.adapter.selected_boundary_face(
            body,
            reference,
            self.inputs,
        )?)
    }
    fn circle_axis(
        &self,
        reference: &TopologyRef,
        body: &NodeRef,
    ) -> Result<cad_core::reference_geometry::Axis> {
        let edges = self.adapter.selected_boundary_edges(
            body,
            std::slice::from_ref(reference),
            self.inputs,
        )?;
        if edges.len() != 1 {
            return Err(err("Circular edge is missing or ambiguous"));
        }
        crate::axis_geometry::circle_axis(&edges[0].oriented_curve())
    }
    fn sketch_circle_axis(
        &self,
        profile: &NodeRef,
        member: &str,
    ) -> Result<cad_core::reference_geometry::Axis> {
        let part = self
            .doc
            .parts
            .get(&profile.definition)
            .ok_or_else(|| err("Circle sketch part is missing"))?;
        let feature = part
            .features
            .iter()
            .find(|f| f.id == profile.node && !f.suppressed)
            .ok_or_else(|| err("Circle sketch is missing or suppressed"))?;
        let Operation::Profile {
            profile: Profile::Sketch { sketch },
            plane,
            plane_reference,
            reference_body,
        } = &feature.operation
        else {
            return Err(err("Select a sketch circle or circular arc"));
        };
        let source = part
            .sketches
            .get(sketch)
            .ok_or_else(|| err("Circle sketch is missing"))?;
        if cad_core::sketch_transform::suppressed_members(source).contains(member) {
            return Err(err("Referenced sketch circle or arc is suppressed"));
        }
        let plane = self.adapter.sketch_plane(
            plane,
            plane_reference.as_ref(),
            reference_body.as_ref(),
            self.inputs,
        )?;
        let mut solved = self.adapter.prepare_sketch(source, &plane, self.inputs)?;
        use cad_core::solver::ConstraintSolver;
        cad_core::solver::NativeSolver.solve_sketch(self.doc, &profile.definition, &mut solved)?;
        let get = |key: &str| {
            solved
                .points
                .get(key)
                .copied()
                .ok_or_else(|| err("Sketch circle or arc point is missing"))
        };
        let center = if let Some(circle) = solved.circles.get(member) {
            get(&circle.center)?
        } else {
            match cad_core::sketch_edit::effective_curve(&solved, member)? {
                cad_core::SketchCurve::Circle { center, .. } => get(&center)?,
                cad_core::SketchCurve::Arc {
                    start,
                    through,
                    end,
                } => {
                    let (center, _, _, _) = cad_core::sketch_geometry::arc_frame(
                        get(&start)?,
                        get(&through)?,
                        get(&end)?,
                    )?;
                    center
                }
                _ => {
                    return Err(CadError::Unsupported(
                        "Sketch circle axes require a circle or circular arc member".into(),
                    ));
                }
            }
        };
        let origin = std::array::from_fn(|i| {
            plane.origin[i] + plane.u[i] * center[0] + plane.v[i] * center[1]
        });
        let u = Vector3::new(plane.u[0], plane.u[1], plane.u[2]);
        let v = Vector3::new(plane.v[0], plane.v[1], plane.v[2]);
        let n = u.cross(v).normalize();
        Ok(cad_core::reference_geometry::Axis {
            origin,
            direction: [n.x, n.y, n.z],
        })
    }
    fn sketch_axis(
        &self,
        profile: &NodeRef,
        member: &str,
    ) -> Result<cad_core::reference_geometry::Axis> {
        let part = self
            .doc
            .parts
            .get(&profile.definition)
            .ok_or_else(|| err("Axis sketch part is missing"))?;
        let feature = part
            .features
            .iter()
            .find(|f| f.id == profile.node && !f.suppressed)
            .ok_or_else(|| err("Axis sketch is missing or suppressed"))?;
        let Operation::Profile {
            profile: Profile::Sketch { sketch },
            ..
        } = &feature.operation
        else {
            return Err(err("Axis requires a sketch line or centerline"));
        };
        let sketch = part
            .sketches
            .get(sketch)
            .ok_or_else(|| err("Axis sketch is missing"))?;
        if cad_core::sketch_transform::suppressed_members(sketch).contains(member) {
            return Err(err("Referenced sketch axis member is suppressed"));
        }
        let cad_core::SketchCurve::Line { start, end } =
            cad_core::sketch_edit::effective_curve(sketch, member)?
        else {
            return Err(CadError::Unsupported(
                "Sketch reference axes require straight line members".into(),
            ));
        };
        let a = self.sketch_point(profile, &start)?;
        let b = self.sketch_point(profile, &end)?;
        Ok(cad_core::reference_geometry::Axis {
            origin: a,
            direction: std::array::from_fn(|i| b[i] - a[i]),
        })
    }
    fn sketch_point(&self, profile: &NodeRef, point_id: &str) -> Result<[f64; 3]> {
        let part = self
            .doc
            .parts
            .get(&profile.definition)
            .ok_or_else(|| err("Sketch point part is missing"))?;
        let feature = part
            .features
            .iter()
            .find(|f| f.id == profile.node && !f.suppressed)
            .ok_or_else(|| err("Sketch point profile is missing or suppressed"))?;
        let Operation::Profile {
            profile: Profile::Sketch { sketch },
            plane,
            plane_reference,
            reference_body,
        } = &feature.operation
        else {
            return Err(err("Reference point requires a sketch member point"));
        };
        let plane = self.adapter.sketch_plane(
            plane,
            plane_reference.as_ref(),
            reference_body.as_ref(),
            self.inputs,
        )?;
        let source = part
            .sketches
            .get(sketch)
            .ok_or_else(|| err("Referenced sketch is missing"))?;
        if cad_core::sketch_transform::suppressed_points(source).contains(point_id) {
            return Err(err("Referenced sketch point is suppressed"));
        }
        let mut solved = self.adapter.prepare_sketch(source, &plane, self.inputs)?;
        use cad_core::solver::ConstraintSolver;
        cad_core::solver::NativeSolver.solve_sketch(self.doc, &profile.definition, &mut solved)?;
        let p = solved
            .points
            .get(point_id)
            .ok_or_else(|| err("Referenced sketch point is missing"))?;
        Ok(std::array::from_fn(|i| {
            plane.origin[i] + plane.u[i] * p[0] + plane.v[i] * p[1]
        }))
    }
}
