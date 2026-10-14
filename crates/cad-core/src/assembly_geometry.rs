//! Versioned, renderer/kernel-independent mate geometry and evaluation contracts.
use crate::{
    CadError, Document, Id, Mate, NodeRef, Placement, Result, TopologyRef, reference_geometry::*,
};
use nalgebra::{Matrix4, Rotation3, Vector3, Vector4};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
pub const CONTRACT_VERSION: u32 = 4;
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Source {
    /// A complete orientation uses local Z as its axis and local X as its secondary direction.
    CoordinateSystem {
        feature: NodeRef,
    },
    Plane {
        plane: PlaneSource,
    },
    Axis {
        axis: AxisSource,
    },
    FaceAxis {
        reference: TopologyRef,
        body: NodeRef,
    },
    Point {
        point: PointSource,
    },
    ProfileCenter {
        profile: NodeRef,
    },
    FaceCenter {
        reference: TopologyRef,
        body: NodeRef,
    },
    EdgeCurve {
        reference: TopologyRef,
        body: NodeRef,
    },
    SketchCurve {
        profile: NodeRef,
        member: Id,
    },
}
impl Source {
    pub fn dependencies(&self) -> Vec<NodeRef> {
        match self {
            Self::CoordinateSystem { feature } => vec![feature.clone()],
            Self::Plane { plane } => plane.dependencies(),
            Self::Axis { axis } => axis.dependencies(),
            Self::FaceAxis { body, .. } => vec![body.clone()],
            Self::Point { point } => point.dependencies(),
            Self::SketchCurve { profile, .. } | Self::ProfileCenter { profile } => {
                vec![profile.clone()]
            }
            Self::FaceCenter { body, .. } | Self::EdgeCurve { body, .. } => vec![body.clone()],
        }
    }
    pub fn resolve(&self, doc: &Document, owner: &str, resolver: &impl Resolver) -> Result<Frame> {
        match self {
            Self::CoordinateSystem { feature } => match resolver.datum(feature)? {
                Geometry::CoordinateSystem { frame } => {
                    frame.validate()?;
                    Frame::new(frame.origin, frame.z, Some(frame.x), FrameKind::Axis)
                }
                _ => Err(CadError::Invalid(
                    "Mate coordinate system is missing or has changed type".into(),
                )),
            },
            Self::Plane { plane } => {
                let p = plane.resolve(resolver)?;
                Frame::new(
                    p.origin,
                    Vector3::from(p.u).cross(&Vector3::from(p.v)).into(),
                    Some(p.u),
                    FrameKind::Plane,
                )
            }
            Self::Axis { axis } => {
                let a = axis.resolve(resolver)?;
                Frame::new(a.origin, a.direction, None, FrameKind::Axis)
            }
            Self::FaceAxis { reference, body } => {
                let a = resolver.face_axis(reference, body)?;
                Frame::new(a.origin, a.direction, None, FrameKind::Axis)
            }
            Self::Point { point } => Frame::new(
                point.resolve(doc, owner, resolver)?,
                [0., 0., 1.],
                Some([1., 0., 0.]),
                FrameKind::Point,
            ),
            Self::ProfileCenter { profile } => resolver.mate_profile_center(profile),
            Self::FaceCenter { reference, body } => resolver.mate_face_center(reference, body),
            Self::EdgeCurve { reference, body } => {
                curve_frame(resolver.mate_edge_curve(reference, body)?)
            }
            Self::SketchCurve { profile, member } => {
                curve_frame(resolver.mate_curve(profile, member)?)
            }
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Reference {
    /// Instance identities below the mate's direct participant. Empty for direct parts.
    #[serde(default)]
    pub path: Vec<Id>,
    pub source: Source,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FrameKind {
    Legacy,
    Plane,
    Axis,
    Point,
    Curve,
    Profile,
}
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Alignment {
    #[default]
    Either,
    Aligned,
    Opposed,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Frame {
    pub origin: [f64; 3],
    pub axis: [f64; 3],
    pub secondary: [f64; 3],
    pub kind: FrameKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub curve: Option<Box<Curve>>,
}
impl Frame {
    pub fn new(
        origin: [f64; 3],
        axis: [f64; 3],
        secondary: Option<[f64; 3]>,
        kind: FrameKind,
    ) -> Result<Self> {
        if origin.iter().chain(axis.iter()).any(|v| !v.is_finite()) {
            return Err(CadError::Invalid(
                "Mate frame must contain finite coordinates".into(),
            ));
        }
        let n = Vector3::from(axis)
            .try_normalize(1e-12)
            .ok_or_else(|| CadError::Invalid("Mate reference has a zero axis".into()))?;
        let u = Vector3::from(secondary.unwrap_or(if n.x.abs() < 0.8 {
            [1., 0., 0.]
        } else {
            [0., 1., 0.]
        }));
        let u = (u - n * u.dot(&n))
            .try_normalize(1e-12)
            .ok_or_else(|| CadError::Invalid("Mate transverse direction is invalid".into()))?;
        if n.norm_squared() < 0.5
            || u.norm_squared() < 0.5
            || n.iter().chain(u.iter()).any(|v| !v.is_finite())
        {
            return Err(CadError::Invalid("Mate frame direction is invalid".into()));
        }
        Ok(Self {
            origin,
            axis: n.into(),
            secondary: u.into(),
            kind,
            curve: None,
        })
    }
    pub fn legacy(mate: &Mate, first: bool) -> Result<Self> {
        Self::new(
            if first {
                mate.first_origin
            } else {
                mate.second_origin
            },
            if first {
                mate.first_axis
            } else {
                mate.second_axis
            },
            None,
            FrameKind::Legacy,
        )
    }
    pub fn transformed(&self, matrix: &Matrix4<f64>) -> Result<Self> {
        let p = matrix * Vector4::new(self.origin[0], self.origin[1], self.origin[2], 1.);
        let n = matrix * Vector4::new(self.axis[0], self.axis[1], self.axis[2], 0.);
        let u = matrix * Vector4::new(self.secondary[0], self.secondary[1], self.secondary[2], 0.);
        let mut frame = Self::new(
            [p.x, p.y, p.z],
            [n.x, n.y, n.z],
            Some([u.x, u.y, u.z]),
            self.kind,
        )?;
        frame.curve = self
            .curve
            .as_ref()
            .map(|curve| Box::new(curve.transformed(matrix)));
        Ok(frame)
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EvaluatedAssembly {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub pattern_guides: BTreeMap<Id, crate::body_editing::PatternGuide>,
    pub version: u32,
    pub placements: BTreeMap<Id, Placement>,
    pub frames: BTreeMap<Id, [Frame; 2]>,
    pub report: Option<crate::solver::SolveReport>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub auxiliary_frames: BTreeMap<Id, Vec<ResolvedEndpoint>>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub diagnostics: BTreeMap<Id, MateDiagnostic>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub component_motion: BTreeMap<Id, ComponentMotion>,
}
pub fn relative_placement(a: &Placement, b: &Placement) -> Result<Placement> {
    let inverse = a
        .matrix()
        .try_inverse()
        .ok_or_else(|| CadError::Invalid("Cannot capture relative component placement".into()))?;
    let m = inverse * b.matrix();
    let r = Rotation3::from_matrix_unchecked(m.fixed_view::<3, 3>(0, 0).into_owned());
    let (x, y, z) = r.euler_angles();
    Ok(Placement {
        translation: [m[(0, 3)], m[(1, 3)], m[(2, 3)]],
        rotation_deg: [x, y, z].map(f64::to_degrees),
    })
}
/// Resolve a stable nested path in a participant's definition and its child-local placement.
pub fn participant(
    doc: &Document,
    definition: &str,
    path: &[Id],
) -> Result<(Id, Matrix4<f64>, Option<Id>)> {
    let mut definition = definition.to_string();
    let mut matrix = Matrix4::identity();
    let mut configuration = None;
    for key in path {
        let assembly = doc.assemblies.get(&definition).ok_or_else(|| {
            CadError::Invalid("Mate reference path enters a part rather than a subassembly".into())
        })?;
        let instance = assembly
            .instances
            .get(key)
            .ok_or_else(|| CadError::Invalid("Mate reference component path is missing".into()))?;
        matrix *= instance.placement.matrix();
        definition = instance.definition.clone();
        configuration = instance.configuration.clone();
    }
    Ok((definition, matrix, configuration))
}
pub fn validate_reference(doc: &Document, definition: &str, reference: &Reference) -> Result<()> {
    let (leaf, _, _) = participant(doc, definition, &reference.path)?;
    for dependency in reference.source.dependencies() {
        if dependency.definition != leaf {
            return Err(CadError::Invalid("Mate reference geometry must belong to the selected participant or its nested component path".into()));
        }
        if !doc
            .parts
            .get(&leaf)
            .is_some_and(|p| p.features.iter().any(|f| f.id == dependency.node))
        {
            return Err(CadError::Invalid(
                "Mate reference feature is missing".into(),
            ));
        }
    }
    Ok(())
}

/// An exact planar carrier, evaluated from current solved source rather than display polylines.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Curve {
    pub plane: crate::Plane,
    pub geometry: CurveGeometry,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slot_carrier: Option<SlotCarrier>,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SlotCarrier {
    Line,
    Circle,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CurveGeometry {
    Line {
        start: [f64; 2],
        end: [f64; 2],
    },
    Circle {
        center: [f64; 2],
        radius: f64,
        start: f64,
        sweep: f64,
    },
    Ellipse {
        center: [f64; 2],
        major: f64,
        minor: f64,
        angle: f64,
    },
    Nurbs {
        curve: crate::sketch_edit::Curve2,
    },
}
impl Curve {
    pub fn transformed(&self, m: &Matrix4<f64>) -> Self {
        let p = m * Vector4::new(
            self.plane.origin[0],
            self.plane.origin[1],
            self.plane.origin[2],
            1.,
        );
        let transform = |v: [f64; 3]| {
            let q = m * Vector4::new(v[0], v[1], v[2], 0.);
            [q.x, q.y, q.z]
        };
        Self {
            plane: crate::Plane {
                origin: [p.x, p.y, p.z],
                u: transform(self.plane.u),
                v: transform(self.plane.v),
            },
            geometry: self.geometry.clone(),
            slot_carrier: self.slot_carrier,
        }
    }
    pub fn point(&self, t: f64) -> Result<[f64; 3]> {
        if !t.is_finite() {
            return Err(CadError::Invalid("Curve parameter must be finite".into()));
        }
        let q = match &self.geometry {
            CurveGeometry::Line { start, end } => {
                std::array::from_fn(|i| start[i] + t * (end[i] - start[i]))
            }
            CurveGeometry::Circle {
                center,
                radius,
                start,
                sweep,
            } => {
                let a = start + sweep * t;
                [center[0] + radius * a.cos(), center[1] + radius * a.sin()]
            }
            CurveGeometry::Ellipse {
                center,
                major,
                minor,
                angle,
            } => {
                let a = std::f64::consts::TAU * t;
                let (s, c) = angle.sin_cos();
                [
                    center[0] + major * a.cos() * c - minor * a.sin() * s,
                    center[1] + major * a.cos() * s + minor * a.sin() * c,
                ]
            }
            CurveGeometry::Nurbs { curve } => crate::sketch_edit::nurbs_point(curve, t)?,
        };
        let p = std::array::from_fn(|i| {
            self.plane.origin[i] + self.plane.u[i] * q[0] + self.plane.v[i] * q[1]
        });
        if p.iter().any(|v| !v.is_finite()) {
            return Err(CadError::Invalid("Curve evaluation is nonfinite".into()));
        }
        Ok(p)
    }
    pub fn derivative(&self, t: f64) -> Result<[f64; 3]> {
        if !t.is_finite() {
            return Err(CadError::Invalid(
                "Curve derivative parameter must be finite".into(),
            ));
        }
        let d = match &self.geometry {
            CurveGeometry::Line { start, end } => [end[0] - start[0], end[1] - start[1]],
            CurveGeometry::Circle {
                radius,
                start,
                sweep,
                ..
            } => {
                let a = start + sweep * t;
                [-radius * sweep * a.sin(), radius * sweep * a.cos()]
            }
            CurveGeometry::Ellipse {
                major,
                minor,
                angle,
                ..
            } => {
                let a = std::f64::consts::TAU * t;
                let (s, c) = angle.sin_cos();
                [
                    (-major * a.sin() * c - minor * a.cos() * s) * std::f64::consts::TAU,
                    (-major * a.sin() * s + minor * a.cos() * c) * std::f64::consts::TAU,
                ]
            }
            CurveGeometry::Nurbs { curve } => {
                crate::sketch_edit::validate_nurbs(curve)?;
                let controls = curve
                    .control_points
                    .iter()
                    .zip(&curve.weights)
                    .map(|(p, w)| Vector3::new(p[0] * w, p[1] * w, *w))
                    .collect::<Vec<_>>();
                let n = controls.len();
                let u = curve.knots[curve.degree]
                    + t.clamp(0., 1.) * (curve.knots[n] - curve.knots[curve.degree]);
                let h = homogeneous(&controls, &curve.knots, curve.degree, u)?;
                let derivatives = controls
                    .windows(2)
                    .enumerate()
                    .map(|(i, p)| {
                        let d = curve.knots[i + curve.degree + 1] - curve.knots[i + 1];
                        if d == 0. {
                            Vector3::zeros()
                        } else {
                            (p[1] - p[0]) * (curve.degree as f64 / d)
                        }
                    })
                    .collect::<Vec<_>>();
                let d = homogeneous(
                    &derivatives,
                    &curve.knots[1..curve.knots.len() - 1],
                    curve.degree - 1,
                    u,
                )?;
                [
                    (d.x * h.z - h.x * d.z) / (h.z * h.z)
                        * (curve.knots[n] - curve.knots[curve.degree]),
                    (d.y * h.z - h.y * d.z) / (h.z * h.z)
                        * (curve.knots[n] - curve.knots[curve.degree]),
                ]
            }
        };
        let n = Vector3::from(self.plane.u) * d[0] + Vector3::from(self.plane.v) * d[1];
        if n.iter().any(|v| !v.is_finite()) {
            return Err(CadError::Invalid("Curve derivative is nonfinite".into()));
        }
        Ok(n.into())
    }
    pub fn tangent(&self, t: f64) -> Result<[f64; 3]> {
        Vector3::from(self.derivative(t)?)
            .try_normalize(1e-12)
            .map(Into::into)
            .ok_or_else(|| CadError::Invalid("Path or cam tangent is singular".into()))
    }
    pub fn closed(&self) -> bool {
        match &self.geometry {
            CurveGeometry::Ellipse { .. } => true,
            CurveGeometry::Circle { sweep, .. } => {
                (sweep.abs() - std::f64::consts::TAU).abs() < 1e-9
            }
            CurveGeometry::Nurbs { curve } => {
                let n = curve.control_points.len();
                n > 1
                    && (Vector3::new(
                        curve.control_points[0][0] - curve.control_points[n - 1][0],
                        curve.control_points[0][1] - curve.control_points[n - 1][1],
                        0.,
                    ))
                    .norm()
                        < 1e-9
            }
            _ => false,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Endpoint {
    pub instance: Id,
    pub reference: Reference,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ResolvedEndpoint {
    pub instance: Id,
    pub frame: Frame,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum WidthPosition {
    Centered,
    Free,
    Distance { value: crate::Expr },
    Fraction { value: crate::Expr },
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum CurvePosition {
    Free { seed: f64 },
    Fraction { value: crate::Expr },
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PathOrientation {
    Free,
    Tangent,
    Normal,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CouplerKind {
    Gear,
    RackPinion,
    Screw,
    Linear,
    UniversalJoint,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Range {
    pub minimum: crate::Expr,
    pub maximum: crate::Expr,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Relationship {
    ProfileCenter {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        rotation: Option<crate::Expr>,
    },
    Width {
        first_wall: Endpoint,
        second_wall: Endpoint,
        position: WidthPosition,
    },
    Symmetry {
        plane: Endpoint,
    },
    LimitDistance {
        range: Range,
    },
    LimitAngle {
        range: Range,
    },
    Hinge {
        first_plane: Endpoint,
        second_plane: Endpoint,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        range: Option<Range>,
    },
    Coupler {
        motion: CouplerKind,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        support: Option<Id>,
        zero: [Frame; 2],
        ratio: crate::Expr,
        pitch: crate::Expr,
        phase: crate::Expr,
        reverse: bool,
    },
    Slot {
        position: CurvePosition,
        orientation: PathOrientation,
    },
    Path {
        position: CurvePosition,
        orientation: PathOrientation,
    },
    Cam {
        position: CurvePosition,
        follower_radius: crate::Expr,
        outward: bool,
    },
}
impl Relationship {
    pub fn kind(&self) -> &'static str {
        match self {
            Self::ProfileCenter { .. } => "profile_center",
            Self::Width { .. } => "width",
            Self::Symmetry { .. } => "symmetry",
            Self::LimitDistance { .. } => "limit_distance",
            Self::LimitAngle { .. } => "limit_angle",
            Self::Hinge { .. } => "hinge",
            Self::Coupler { motion, .. } => match motion {
                CouplerKind::Gear => "gear",
                CouplerKind::RackPinion => "rack_pinion",
                CouplerKind::Screw => "screw",
                CouplerKind::Linear => "linear_coupler",
                CouplerKind::UniversalJoint => "universal_joint",
            },
            Self::Slot { .. } => "slot",
            Self::Path { .. } => "path",
            Self::Cam { .. } => "cam",
        }
    }
    pub fn endpoints(&self) -> Vec<&Endpoint> {
        match self {
            Self::Width {
                first_wall,
                second_wall,
                ..
            } => vec![first_wall, second_wall],
            Self::Symmetry { plane } => vec![plane],
            Self::Hinge {
                first_plane,
                second_plane,
                ..
            } => vec![first_plane, second_plane],
            _ => vec![],
        }
    }
    pub fn endpoints_mut(&mut self) -> Vec<&mut Endpoint> {
        match self {
            Self::Width {
                first_wall,
                second_wall,
                ..
            } => vec![first_wall, second_wall],
            Self::Symmetry { plane } => vec![plane],
            Self::Hinge {
                first_plane,
                second_plane,
                ..
            } => vec![first_plane, second_plane],
            _ => vec![],
        }
    }
    pub fn participants(&self) -> Vec<&Id> {
        let mut p = self
            .endpoints()
            .into_iter()
            .map(|e| &e.instance)
            .collect::<Vec<_>>();
        if let Self::Coupler {
            support: Some(s), ..
        } = self
        {
            p.push(s);
        }
        p
    }
    pub fn curve_position(&self) -> Option<&CurvePosition> {
        match self {
            Self::Slot { position, .. }
            | Self::Path { position, .. }
            | Self::Cam { position, .. } => Some(position),
            _ => None,
        }
    }
    pub fn validate(&self, doc: &Document, assembly: &crate::Assembly) -> Result<()> {
        for endpoint in self.endpoints() {
            let i = assembly.instances.get(&endpoint.instance).ok_or_else(|| {
                CadError::Invalid("Additional mate participant is missing".into())
            })?;
            validate_reference(doc, &i.definition, &endpoint.reference)?;
        }
        for id in self.participants() {
            if !assembly.instances.contains_key(id) {
                return Err(CadError::Invalid(
                    "Mechanical mate support is missing".into(),
                ));
            }
        }
        if let Some(CurvePosition::Free { seed }) = self.curve_position()
            && (!seed.is_finite() || *seed < 0. || *seed > 1.)
        {
            return Err(CadError::Invalid("Free path seed must be 0..1".into()));
        }
        if let Self::Coupler { zero, .. } = self {
            for f in zero {
                Frame::new(f.origin, f.axis, Some(f.secondary), f.kind)?;
                if f.kind != FrameKind::Axis || f.curve.is_some() {
                    return Err(CadError::Invalid(
                        "Mechanical zero frames must be plain axes".into(),
                    ));
                }
            }
        }
        Ok(())
    }
}
impl crate::Mate {
    pub fn participants(&self) -> Vec<&Id> {
        let mut p = vec![&self.first, &self.second];
        if let Some(r) = &self.relationship {
            p.extend(r.participants());
        }
        p
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MateDiagnostic {
    pub maximum_residual: f64,
    pub equation_count: usize,
    pub independent_equations: usize,
    pub redundant_equations: usize,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub inequality_equations: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub curve_parameter: Option<f64>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ComponentMotion {
    pub grounded: bool,
    pub translation_directions: Vec<[f64; 3]>,
    pub rotation_directions: Vec<[f64; 3]>,
    pub degrees_of_freedom: usize,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AssemblySolution {
    pub report: crate::solver::SolveReport,
    pub diagnostics: BTreeMap<Id, MateDiagnostic>,
    pub component_motion: BTreeMap<Id, ComponentMotion>,
}

fn homogeneous(
    points: &[Vector3<f64>],
    knots: &[f64],
    degree: usize,
    t: f64,
) -> Result<Vector3<f64>> {
    let n = points.len();
    let span = if t >= knots[n] {
        n - 1
    } else {
        (degree..n)
            .find(|i| t >= knots[*i] && t < knots[*i + 1])
            .ok_or_else(|| CadError::Invalid("Invalid rational path span".into()))?
    };
    let mut p = points[span - degree..=span].to_vec();
    for level in 1..=degree {
        for j in (level..=degree).rev() {
            let i = span - degree + j;
            let d = knots[i + degree + 1 - level] - knots[i];
            let a = if d == 0. { 0. } else { (t - knots[i]) / d };
            p[j] = p[j - 1] * (1. - a) + p[j] * a;
        }
    }
    Ok(p[degree])
}

#[derive(Clone, Copy, Debug, Serialize)]
pub struct MateMode {
    pub kind: &'static str,
    pub label: &'static str,
    pub group: &'static str,
    pub domain: &'static str,
}
pub const MATE_MODES: &[MateMode] = &[
    MateMode {
        kind: "coincident",
        label: "Coincident",
        group: "Standard",
        domain: "Planar faces, axes, points",
    },
    MateMode {
        kind: "concentric",
        label: "Concentric",
        group: "Standard",
        domain: "Reference axes, exact rotational faces and circular edges",
    },
    MateMode {
        kind: "planar",
        label: "Planar",
        group: "Standard",
        domain: "Planar faces or datums",
    },
    MateMode {
        kind: "parallel",
        label: "Parallel",
        group: "Standard",
        domain: "Planes and axes",
    },
    MateMode {
        kind: "perpendicular",
        label: "Perpendicular",
        group: "Standard",
        domain: "Planes and axes",
    },
    MateMode {
        kind: "distance",
        label: "Distance",
        group: "Standard",
        domain: "Planes, axes and points",
    },
    MateMode {
        kind: "angle",
        label: "Angle",
        group: "Standard",
        domain: "Planes and axes",
    },
    MateMode {
        kind: "lock",
        label: "Lock",
        group: "Standard",
        domain: "Captured relative component placement",
    },
    MateMode {
        kind: "profile_center",
        label: "Profile Center",
        group: "Advanced",
        domain: "Area centroids of closed planar sketches or planar faces, optional rotation",
    },
    MateMode {
        kind: "width",
        label: "Width",
        group: "Advanced",
        domain: "Two parallel width walls and two parallel tab walls",
    },
    MateMode {
        kind: "symmetry",
        label: "Symmetric",
        group: "Advanced",
        domain: "Two point or directional frames and a symmetry plane",
    },
    MateMode {
        kind: "limit_distance",
        label: "Limit Distance",
        group: "Advanced",
        domain: "Bounded plane, axis, point separation",
    },
    MateMode {
        kind: "limit_angle",
        label: "Limit Angle",
        group: "Advanced",
        domain: "Directional angle bounded in 0..180 degrees",
    },
    MateMode {
        kind: "linear_coupler",
        label: "Linear Coupler",
        group: "Advanced",
        domain: "Captured support-relative axial displacement coordinates",
    },
    MateMode {
        kind: "path",
        label: "Path",
        group: "Advanced",
        domain: "One exact planar sketch member or model edge and follower point or axis",
    },
    MateMode {
        kind: "hinge",
        label: "Hinge",
        group: "Mechanical",
        domain: "Two axes and their normal seating planes, optional signed angle bounds",
    },
    MateMode {
        kind: "gear",
        label: "Gear",
        group: "Mechanical",
        domain: "Captured support-relative shaft rotations on a selected angular branch",
    },
    MateMode {
        kind: "rack_pinion",
        label: "Rack and Pinion",
        group: "Mechanical",
        domain: "Shaft rotation coupled to axial rack travel",
    },
    MateMode {
        kind: "screw",
        label: "Screw",
        group: "Mechanical",
        domain: "Coaxial relative rotation and relative axial travel by lead per revolution",
    },
    MateMode {
        kind: "universal_joint",
        label: "Universal Joint",
        group: "Mechanical",
        domain: "Intersecting supported shafts with captured nonorthogonal axis directions",
    },
    MateMode {
        kind: "slot",
        label: "Slot",
        group: "Mechanical",
        domain: "Exact line or circular sketch/model center path and follower",
    },
    MateMode {
        kind: "cam",
        label: "Cam / Roller",
        group: "Mechanical",
        domain: "Planar analytic or rational sketch/model curve and roller/point follower",
    },
];

fn is_zero(value: &usize) -> bool {
    *value == 0
}

fn curve_frame(curve: Curve) -> Result<Frame> {
    let n = Vector3::from(curve.plane.u).cross(&Vector3::from(curve.plane.v));
    let mut frame = Frame::new(
        curve.plane.origin,
        n.into(),
        Some(curve.plane.u),
        FrameKind::Curve,
    )?;
    frame.curve = Some(Box::new(curve));
    Ok(frame)
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReferenceClass {
    Plane,
    Axis,
    Point,
    Curve,
    Profile,
}
impl Source {
    pub fn class(&self) -> ReferenceClass {
        match self {
            Self::Plane { .. } => ReferenceClass::Plane,
            Self::Axis { .. } | Self::FaceAxis { .. } | Self::CoordinateSystem { .. } => {
                ReferenceClass::Axis
            }
            Self::Point { .. } => ReferenceClass::Point,
            Self::SketchCurve { .. } | Self::EdgeCurve { .. } => ReferenceClass::Curve,
            Self::ProfileCenter { .. } | Self::FaceCenter { .. } => ReferenceClass::Profile,
        }
    }
}
/// Roles are semantic identities, independent of editor labels and widget layout.
pub fn reference_classes(kind: &str, role: &str) -> Vec<ReferenceClass> {
    use ReferenceClass::*;
    if role == "auxiliary_plane" {
        return vec![Plane];
    }
    match kind {
        "concentric" | "hinge" | "gear" | "rack_pinion" | "screw" | "linear_coupler"
        | "universal_joint" => vec![Axis],
        "planar" | "width" => vec![Plane],
        "angle" | "limit_angle" | "parallel" | "perpendicular" => vec![Plane, Axis],
        "path" | "slot" | "cam" if role == "first" => vec![Curve],
        "path" | "slot" | "cam" => vec![Point, Axis],
        "profile_center" => vec![Profile],
        "lock" => vec![Plane, Axis, Point, Curve, Profile],
        _ => vec![Plane, Axis, Point],
    }
}
