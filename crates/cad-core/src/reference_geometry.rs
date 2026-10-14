//! Versioned, application-owned reference geometry intent and evaluated f64 geometry.
use crate::{CadError, Dimension, Document, Expr, Id, NodeRef, Plane, Result, TopologyRef};
use nalgebra::{Rotation3, Unit, Vector3};
use serde::{Deserialize, Serialize};

pub const CONTRACT_VERSION: u32 = 5;
pub const COORDINATE_SYSTEM_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OriginPlane {
    Front,
    Top,
    Right,
}
impl OriginPlane {
    pub fn plane(self) -> Plane {
        match self {
            Self::Top => Plane::default(),
            Self::Front => Plane {
                origin: [0.; 3],
                u: [1., 0., 0.],
                v: [0., 0., 1.],
            },
            Self::Right => Plane {
                origin: [0.; 3],
                u: [0., 1., 0.],
                v: [0., 0., 1.],
            },
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Front => "Front Plane",
            Self::Top => "Top Plane",
            Self::Right => "Right Plane",
        }
    }
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OriginAxis {
    X,
    Y,
    Z,
}
impl OriginAxis {
    pub fn direction(self) -> [f64; 3] {
        match self {
            Self::X => [1., 0., 0.],
            Self::Y => [0., 1., 0.],
            Self::Z => [0., 0., 1.],
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::X => "X Axis",
            Self::Y => "Y Axis",
            Self::Z => "Z Axis",
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PlaneSource {
    Origin {
        plane: OriginPlane,
    },
    Datum {
        feature: NodeRef,
    },
    CoordinateSystem {
        feature: NodeRef,
        plane: OriginPlane,
    },
    Face {
        reference: TopologyRef,
        body: NodeRef,
    },
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AxisSource {
    Origin {
        axis: OriginAxis,
    },
    Datum {
        feature: NodeRef,
    },
    CoordinateSystem {
        feature: NodeRef,
        axis: OriginAxis,
    },
    Edge {
        reference: TopologyRef,
        body: NodeRef,
    },
    FaceAxis {
        reference: TopologyRef,
        body: NodeRef,
    },
    CircleEdge {
        reference: TopologyRef,
        body: NodeRef,
    },
    SketchLine {
        profile: NodeRef,
        member: Id,
    },
    SketchCircle {
        profile: NodeRef,
        member: Id,
    },
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct EdgePointSource {
    pub body: NodeRef,
    pub reference: TopologyRef,
    /// Normalized native curve parameter, not arc-length percentage. The edge's
    /// stored constructor orientation defines zero and one independently of faces.
    pub parameter: Expr,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PointSource {
    Coordinates { coordinates: [Expr; 3] },
    Datum { feature: NodeRef },
    CoordinateSystemOrigin { feature: NodeRef },
    SketchPoint { profile: NodeRef, point: Id },
    PlaneOrigin { plane: PlaneSource },
    AxisOrigin { axis: AxisSource },
    EdgePoint { source: Box<EdgePointSource> },
}
impl Default for PointSource {
    fn default() -> Self {
        Self::Coordinates {
            coordinates: std::array::from_fn(|_| "0 mm".into()),
        }
    }
}
/// A direction retains its driving intent independently of the evaluated frame.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DirectionSource {
    Explicit { direction: [Expr; 3] },
    Axis { axis: Box<AxisSource> },
    PlaneNormal { plane: PlaneSource },
    TwoPoints { points: Box<[PointSource; 2]> },
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct CoordinateSystemDefinition {
    pub version: u32,
    pub origin: PointSource,
    pub primary_axis: OriginAxis,
    pub primary: DirectionSource,
    pub secondary_axis: OriginAxis,
    pub secondary: DirectionSource,
    #[serde(default)]
    pub reverse_primary: bool,
    #[serde(default)]
    pub reverse_secondary: bool,
}
impl Default for CoordinateSystemDefinition {
    fn default() -> Self {
        Self {
            version: COORDINATE_SYSTEM_VERSION,
            origin: PointSource::default(),
            primary_axis: OriginAxis::X,
            primary: DirectionSource::Axis {
                axis: Box::new(AxisSource::Origin {
                    axis: OriginAxis::X,
                }),
            },
            secondary_axis: OriginAxis::Y,
            secondary: DirectionSource::Axis {
                axis: Box::new(AxisSource::Origin {
                    axis: OriginAxis::Y,
                }),
            },
            reverse_primary: false,
            reverse_secondary: false,
        }
    }
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
pub struct CoordinateSystem {
    pub origin: [f64; 3],
    pub x: [f64; 3],
    pub y: [f64; 3],
    pub z: [f64; 3],
}
impl CoordinateSystem {
    pub fn validate(self) -> Result<()> {
        finite_point(self.origin)?;
        let axes = [vector(self.x), vector(self.y), vector(self.z)];
        if axes
            .iter()
            .any(|a| a.iter().any(|v| !v.is_finite()) || (a.norm_squared() - 1.).abs() > 1e-10)
            || axes[0].dot(&axes[1]).abs() > 1e-10
            || axes[0].dot(&axes[2]).abs() > 1e-10
            || axes[1].dot(&axes[2]).abs() > 1e-10
            || (axes[0].cross(&axes[1]).dot(&axes[2]) - 1.).abs() > 1e-10
        {
            return Err(invalid(
                "coordinate-system frame must be finite, orthonormal, and right-handed",
            ));
        }
        Ok(())
    }
    pub fn axis(self, axis: OriginAxis) -> Axis {
        Axis {
            origin: self.origin,
            direction: match axis {
                OriginAxis::X => self.x,
                OriginAxis::Y => self.y,
                OriginAxis::Z => self.z,
            },
        }
    }
    pub fn plane(self, plane: OriginPlane) -> Plane {
        let (u, v) = match plane {
            OriginPlane::Top => (self.x, self.y),
            OriginPlane::Front => (self.x, self.z),
            OriginPlane::Right => (self.y, self.z),
        };
        Plane {
            origin: self.origin,
            u,
            v,
        }
    }
}
pub fn coordinate_plane_role(plane: OriginPlane) -> &'static str {
    match plane {
        OriginPlane::Top => "datum:coordinate-system:xy",
        OriginPlane::Front => "datum:coordinate-system:xz",
        OriginPlane::Right => "datum:coordinate-system:yz",
    }
}
pub fn coordinate_axis_role(axis: OriginAxis) -> &'static str {
    match axis {
        OriginAxis::X => "datum:coordinate-system:x",
        OriginAxis::Y => "datum:coordinate-system:y",
        OriginAxis::Z => "datum:coordinate-system:z",
    }
}
pub fn coordinate_plane_element(role: &str) -> Option<OriginPlane> {
    [OriginPlane::Top, OriginPlane::Front, OriginPlane::Right]
        .into_iter()
        .find(|p| coordinate_plane_role(*p) == role)
}
pub fn coordinate_axis_element(role: &str) -> Option<OriginAxis> {
    [OriginAxis::X, OriginAxis::Y, OriginAxis::Z]
        .into_iter()
        .find(|a| coordinate_axis_role(*a) == role)
}
pub const COORDINATE_ORIGIN_ROLE: &str = "datum:coordinate-system:origin";
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BisectorSolution {
    SameSignedDistance,
    OppositeSignedDistance,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum TangentAnchor {
    /// Normalized coordinates of the adapter-proven selected face domain.
    Parameters {
        parameters: [Expr; 2],
    },
    Point {
        point: Box<PointSource>,
    },
}
#[derive(Clone, Copy, Debug)]
pub enum TangentAnchorValue {
    Parameters([f64; 2]),
    Point([f64; 3]),
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct TangentPlaneSource {
    pub body: NodeRef,
    pub reference: TopologyRef,
    pub anchor: TangentAnchor,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum PlaneDefinition {
    Offset {
        plane: PlaneSource,
        distance: Expr,
        #[serde(default)]
        flip: bool,
    },
    Angle {
        plane: PlaneSource,
        axis: AxisSource,
        angle: Expr,
    },
    ThroughPoints {
        points: [PointSource; 3],
    },
    Midplane {
        planes: [PlaneSource; 2],
    },
    NormalToAxis {
        axis: AxisSource,
        point: PointSource,
    },
    AngularBisector {
        planes: [PlaneSource; 2],
        solution: BisectorSolution,
        #[serde(default)]
        reverse: bool,
    },
    Tangent {
        source: Box<TangentPlaneSource>,
        #[serde(default)]
        reverse: bool,
    },
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum AxisDefinition {
    ThroughPoints {
        points: [PointSource; 2],
    },
    PlaneIntersection {
        planes: [PlaneSource; 2],
    },
    NormalToPlane {
        plane: PlaneSource,
        point: PointSource,
    },
    AlongEdge {
        axis: AxisSource,
    },
    Geometry {
        axis: AxisSource,
    },
    Direction {
        origin: PointSource,
        direction: [Expr; 3],
    },
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum PointDefinition {
    AtPoint {
        point: PointSource,
    },
    OnAxis {
        axis: AxisSource,
        distance: Expr,
    },
    PlaneCoordinates {
        plane: PlaneSource,
        u: Expr,
        v: Expr,
        offset: Expr,
    },
    AxisPlaneIntersection {
        axis: AxisSource,
        plane: PlaneSource,
    },
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Axis {
    pub origin: [f64; 3],
    pub direction: [f64; 3],
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Geometry {
    Plane { plane: Plane },
    Axis { axis: Axis },
    Point { point: [f64; 3] },
    CoordinateSystem { frame: CoordinateSystem },
}
impl Geometry {
    pub fn topology(&self) -> &'static str {
        match self {
            Self::Plane { .. } => "plane",
            Self::Axis { .. } => "axis",
            Self::Point { .. } => "point",
            Self::CoordinateSystem { .. } => "coordinate_system",
        }
    }
    pub fn origin(&self) -> [f64; 3] {
        match self {
            Self::Plane { plane } => plane.origin,
            Self::Axis { axis } => axis.origin,
            Self::Point { point } => *point,
            Self::CoordinateSystem { frame } => frame.origin,
        }
    }
    pub fn selected_plane(&self, role: &str) -> Result<Plane> {
        match self {
            Self::Plane { plane } if role == "datum:plane" => Ok(plane.clone()),
            Self::CoordinateSystem { frame } => {
                frame.validate()?;
                coordinate_plane_element(role)
                    .map(|p| frame.plane(p))
                    .ok_or_else(|| invalid("unknown coordinate-system plane construction role"))
            }
            _ => Err(invalid("selected datum is not a plane")),
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct EvaluatedReference {
    pub version: u32,
    pub producer: NodeRef,
    pub name: String,
    pub geometry: Geometry,
}
impl EvaluatedReference {
    pub fn topology_reference(&self) -> TopologyRef {
        TopologyRef {
            producer: self.producer.clone(),
            role: format!("datum:{}", self.geometry.topology()),
            topology: self.geometry.topology().into(),
            signature: None,
        }
    }
    /// Publish selectable construction elements independently of presentation.
    pub fn elements(&self) -> Vec<(Geometry, TopologyRef, String)> {
        let Geometry::CoordinateSystem { frame } = &self.geometry else {
            return vec![(
                self.geometry.clone(),
                self.topology_reference(),
                self.name.clone(),
            )];
        };
        let mut elements = vec![(
            Geometry::Point {
                point: frame.origin,
            },
            COORDINATE_ORIGIN_ROLE,
            format!("{} · Origin", self.name),
        )];
        for axis in [OriginAxis::X, OriginAxis::Y, OriginAxis::Z] {
            elements.push((
                Geometry::Axis {
                    axis: frame.axis(axis),
                },
                coordinate_axis_role(axis),
                format!("{} · {}", self.name, axis.label()),
            ));
        }
        for plane in [OriginPlane::Top, OriginPlane::Front, OriginPlane::Right] {
            elements.push((
                Geometry::Plane {
                    plane: frame.plane(plane),
                },
                coordinate_plane_role(plane),
                format!("{} · {}", self.name, plane.label()),
            ));
        }
        elements
            .into_iter()
            .map(|(geometry, role, label)| {
                let reference = TopologyRef {
                    producer: self.producer.clone(),
                    topology: geometry.topology().into(),
                    role: role.into(),
                    signature: None,
                };
                (geometry, reference, label)
            })
            .collect()
    }
}
/// Implemented by adapters; source identities and evaluated values remain application-owned.
pub trait Resolver {
    /// Current solved root-to-occurrence transform. Source-only approximations are not valid here.
    fn occurrence_placement(&self, _path: &[Id]) -> Result<[[f64; 4]; 4]> {
        Err(CadError::Unsupported(
            "This adapter does not resolve current solved occurrence transforms".into(),
        ))
    }
    fn mate_profile_center(&self, _profile: &NodeRef) -> Result<crate::assembly_geometry::Frame> {
        Err(CadError::Unsupported(
            "This adapter does not evaluate planar profile centroids".into(),
        ))
    }
    fn mate_face_center(
        &self,
        _reference: &TopologyRef,
        _body: &NodeRef,
    ) -> Result<crate::assembly_geometry::Frame> {
        Err(CadError::Unsupported(
            "This adapter does not evaluate planar face centroids".into(),
        ))
    }
    fn mate_edge_curve(
        &self,
        _reference: &TopologyRef,
        _body: &NodeRef,
    ) -> Result<crate::assembly_geometry::Curve> {
        Err(CadError::Unsupported(
            "This adapter does not resolve planar model curve carriers".into(),
        ))
    }

    fn mate_curve(
        &self,
        _profile: &NodeRef,
        _member: &str,
    ) -> Result<crate::assembly_geometry::Curve> {
        Err(CadError::Unsupported(
            "This adapter does not resolve exact sketch mate curves".into(),
        ))
    }
    fn datum(&self, feature: &NodeRef) -> Result<Geometry>;
    fn face_plane(&self, reference: &TopologyRef, body: &NodeRef) -> Result<Plane>;
    fn tangent_plane(
        &self,
        _reference: &TopologyRef,
        _body: &NodeRef,
        _anchor: TangentAnchorValue,
    ) -> Result<Plane> {
        Err(CadError::Unsupported(
            "This adapter does not prove bounded tangent-face anchors".into(),
        ))
    }
    fn edge_axis(&self, reference: &TopologyRef, body: &NodeRef) -> Result<Axis>;
    fn edge_point(
        &self,
        _reference: &TopologyRef,
        _body: &NodeRef,
        _parameter: f64,
    ) -> Result<[f64; 3]> {
        Err(CadError::Unsupported(
            "This adapter does not evaluate native model-edge parameters".into(),
        ))
    }
    fn face_axis(&self, _reference: &TopologyRef, _body: &NodeRef) -> Result<Axis> {
        Err(CadError::Unsupported(
            "This adapter does not resolve exact rotational face axes".into(),
        ))
    }
    fn circle_axis(&self, _reference: &TopologyRef, _body: &NodeRef) -> Result<Axis> {
        Err(CadError::Unsupported(
            "This adapter does not resolve exact circular edge axes".into(),
        ))
    }
    fn sketch_point(&self, profile: &NodeRef, point: &str) -> Result<[f64; 3]>;
    fn sketch_axis(&self, profile: &NodeRef, member: &str) -> Result<Axis>;
    fn sketch_circle_axis(&self, _profile: &NodeRef, _member: &str) -> Result<Axis> {
        Err(CadError::Unsupported(
            "This adapter does not resolve sketch circle axes".into(),
        ))
    }
}
fn invalid(message: &str) -> CadError {
    CadError::Invalid(format!("Reference geometry: {message}"))
}
fn vector(a: [f64; 3]) -> Vector3<f64> {
    Vector3::from(a)
}
fn normalized(a: Vector3<f64>) -> Result<Vector3<f64>> {
    let norm = a.norm();
    if !a.iter().all(|v| v.is_finite()) || !norm.is_finite() || norm <= 1e-12 {
        return Err(invalid("zero or invalid direction"));
    }
    Ok(a / norm)
}
fn finite_point(a: [f64; 3]) -> Result<[f64; 3]> {
    if a.iter().all(|v| v.is_finite()) {
        Ok(a)
    } else {
        Err(invalid("invalid point"))
    }
}
pub fn normalized_plane(plane: Plane) -> Result<Plane> {
    finite_point(plane.origin)?;
    let u = normalized(vector(plane.u))?;
    let n = normalized(u.cross(&vector(plane.v)))?;
    Ok(Plane {
        origin: plane.origin,
        u: u.into(),
        v: n.cross(&u).into(),
    })
}
impl PlaneSource {
    pub fn from_datum(reference: &TopologyRef) -> Result<Self> {
        if reference.topology == "plane"
            && let Some(plane) = coordinate_plane_element(&reference.role)
        {
            return Ok(Self::CoordinateSystem {
                feature: reference.producer.clone(),
                plane,
            });
        }
        if reference.topology == "plane" && reference.role == "datum:plane" {
            Ok(Self::Datum {
                feature: reference.producer.clone(),
            })
        } else {
            Err(invalid("unknown reference-plane construction role"))
        }
    }
    pub fn resolve(&self, resolver: &impl Resolver) -> Result<Plane> {
        normalized_plane(match self {
            Self::Origin { plane } => plane.plane(),
            Self::Face { reference, body } => resolver.face_plane(reference, body)?,
            Self::Datum { feature } => match resolver.datum(feature)? {
                Geometry::Plane { plane } => plane,
                _ => return Err(invalid("selected datum is not a plane")),
            },
            Self::CoordinateSystem { feature, plane } => match resolver.datum(feature)? {
                Geometry::CoordinateSystem { frame } => {
                    frame.validate()?;
                    frame.plane(*plane)
                }
                _ => return Err(invalid("selected datum is not a coordinate system")),
            },
        })
    }
    pub fn dependencies(&self) -> Vec<NodeRef> {
        match self {
            Self::Origin { .. } => vec![],
            Self::Datum { feature } | Self::CoordinateSystem { feature, .. } => {
                vec![feature.clone()]
            }
            Self::Face { body, .. } => vec![body.clone()],
        }
    }
}
impl AxisSource {
    pub fn from_datum(reference: &TopologyRef) -> Result<Self> {
        if reference.topology == "axis"
            && let Some(axis) = coordinate_axis_element(&reference.role)
        {
            return Ok(Self::CoordinateSystem {
                feature: reference.producer.clone(),
                axis,
            });
        }
        if reference.topology == "axis" && reference.role == "datum:axis" {
            Ok(Self::Datum {
                feature: reference.producer.clone(),
            })
        } else {
            Err(invalid("unknown reference-axis construction role"))
        }
    }
    pub fn resolve(&self, resolver: &impl Resolver) -> Result<Axis> {
        let mut axis = match self {
            Self::Origin { axis } => Axis {
                origin: [0.; 3],
                direction: axis.direction(),
            },
            Self::Edge { reference, body } => resolver.edge_axis(reference, body)?,
            Self::FaceAxis { reference, body } => resolver.face_axis(reference, body)?,
            Self::CircleEdge { reference, body } => resolver.circle_axis(reference, body)?,
            Self::SketchLine { profile, member } => resolver.sketch_axis(profile, member)?,
            Self::SketchCircle { profile, member } => {
                resolver.sketch_circle_axis(profile, member)?
            }
            Self::Datum { feature } => match resolver.datum(feature)? {
                Geometry::Axis { axis } => axis,
                _ => return Err(invalid("selected datum is not an axis")),
            },
            Self::CoordinateSystem { feature, axis } => match resolver.datum(feature)? {
                Geometry::CoordinateSystem { frame } => {
                    frame.validate()?;
                    frame.axis(*axis)
                }
                _ => return Err(invalid("selected datum is not a coordinate system")),
            },
        };
        finite_point(axis.origin)?;
        axis.direction = normalized(vector(axis.direction))?.into();
        Ok(axis)
    }
    pub fn dependencies(&self) -> Vec<NodeRef> {
        match self {
            Self::Origin { .. } => vec![],
            Self::Datum { feature } | Self::CoordinateSystem { feature, .. } => {
                vec![feature.clone()]
            }
            Self::Edge { body, .. }
            | Self::FaceAxis { body, .. }
            | Self::CircleEdge { body, .. } => vec![body.clone()],
            Self::SketchLine { profile, .. } | Self::SketchCircle { profile, .. } => {
                vec![profile.clone()]
            }
        }
    }
}
impl PointSource {
    pub fn from_datum(reference: &TopologyRef) -> Result<Self> {
        if reference.topology == "point" && reference.role == COORDINATE_ORIGIN_ROLE {
            return Ok(Self::CoordinateSystemOrigin {
                feature: reference.producer.clone(),
            });
        }
        if reference.topology == "point" && reference.role == "datum:point" {
            Ok(Self::Datum {
                feature: reference.producer.clone(),
            })
        } else {
            Err(invalid("unknown reference-point construction role"))
        }
    }
    pub fn resolve(
        &self,
        doc: &Document,
        owner: &str,
        resolver: &impl Resolver,
    ) -> Result<[f64; 3]> {
        finite_point(match self {
            Self::Coordinates { coordinates } => [
                quantity(doc, owner, &coordinates[0], Dimension::Length)?,
                quantity(doc, owner, &coordinates[1], Dimension::Length)?,
                quantity(doc, owner, &coordinates[2], Dimension::Length)?,
            ],
            Self::Datum { feature } => match resolver.datum(feature)? {
                Geometry::Point { point } => point,
                _ => return Err(invalid("selected datum is not a point")),
            },
            Self::CoordinateSystemOrigin { feature } => match resolver.datum(feature)? {
                Geometry::CoordinateSystem { frame } => {
                    frame.validate()?;
                    frame.origin
                }
                _ => return Err(invalid("selected datum is not a coordinate system")),
            },
            Self::SketchPoint { profile, point } => resolver.sketch_point(profile, point)?,
            Self::EdgePoint { source } => {
                if source.reference.topology != "edge" {
                    return Err(invalid("an edge point requires an edge topology reference"));
                }
                let parameter = quantity(doc, owner, &source.parameter, Dimension::Scalar)?;
                if !parameter.is_finite() || !(0. ..=1.).contains(&parameter) {
                    return Err(invalid("model-edge parameter must be finite and in 0..1"));
                }
                resolver.edge_point(&source.reference, &source.body, parameter)?
            }
            Self::PlaneOrigin { plane } => plane.resolve(resolver)?.origin,
            Self::AxisOrigin { axis } => axis.resolve(resolver)?.origin,
        })
    }
    pub fn dependencies(&self) -> Vec<NodeRef> {
        match self {
            Self::Coordinates { .. } => vec![],
            Self::Datum { feature } | Self::CoordinateSystemOrigin { feature } => {
                vec![feature.clone()]
            }
            Self::SketchPoint { profile, .. } => vec![profile.clone()],
            Self::EdgePoint { source } => vec![source.body.clone()],
            Self::PlaneOrigin { plane } => plane.dependencies(),
            Self::AxisOrigin { axis } => axis.dependencies(),
        }
    }
    pub fn expressions(&self) -> Vec<&Expr> {
        match self {
            Self::Coordinates { coordinates } => coordinates.iter().collect(),
            Self::EdgePoint { source } => vec![&source.parameter],
            _ => vec![],
        }
    }
}
/// Publish edge-point sources and quantity fields wherever a feature embeds
/// PointSource, including coordinate frames, scale/mold centers and mate sources.
pub fn edge_point_semantic_fields(
    doc: &Document,
    owner: &str,
    node: &str,
    source: &impl Serialize,
    state: &str,
) -> Vec<crate::presentation::SemanticElement> {
    fn collect(
        value: &serde_json::Value,
        field: &str,
        sources: &mut Vec<(String, EdgePointSource)>,
    ) {
        if value.get("kind").and_then(serde_json::Value::as_str) == Some("edge_point") {
            if let Some(source) = value
                .get("source")
                .and_then(|source| serde_json::from_value::<EdgePointSource>(source.clone()).ok())
            {
                sources.push((field.into(), source));
            }
            return;
        }
        match value {
            serde_json::Value::Object(values) => {
                for (key, value) in values {
                    let key = key.replace('~', "~0").replace('/', "~1");
                    collect(value, &format!("{field}/{key}"), sources);
                }
            }
            serde_json::Value::Array(values) => {
                for (index, value) in values.iter().enumerate() {
                    collect(value, &format!("{field}/{index}"), sources);
                }
            }
            _ => {}
        }
    }
    let mut sources = vec![];
    if let Ok(source) = serde_json::to_value(source) {
        collect(&source, "", &mut sources);
    }
    let mut fields = vec![];
    let feature = doc.parts.contains_key(owner);
    let command = if feature {
        "feature.edit"
    } else {
        "assembly.edit"
    };
    for (field, source) in sources {
        fields.push(crate::presentation::SemanticElement {
            id:format!("{node}::reference::{field}::source"),role:if feature {"feature-reference"} else {"mate-reference"}.into(),label:"Model-edge Point Source".into(),value:Some(source.body.node.clone()),state:state.into(),commands:vec![command.into()],relationships:vec![node.into(),source.body.node.clone(),source.reference.producer.node.clone()],
            data:serde_json::json!({"contract":"tcad.reference_geometry","version":CONTRACT_VERSION,"field":format!("{field}/source"),"type":"topology_reference","body":source.body,"reference":source.reference,"orientation":"stored_constructor","editable":true}),
        });
        fields.push(crate::presentation::SemanticElement {
            id:format!("{node}::reference::{field}::parameter"),role:if feature {"feature-field"} else {"mate-field"}.into(),label:"Native Edge Parameter".into(),value:Some(source.parameter.0.clone()),state:state.into(),commands:vec![command.into()],relationships:vec![node.into(),source.body.node.clone()],
            data:serde_json::json!({"contract":"tcad.reference_geometry","version":CONTRACT_VERSION,"field":format!("{field}/source/parameter"),"type":"quantity","dimension":Dimension::Scalar,"expression":source.parameter,"evaluated":crate::expression::value(doc,owner,&source.parameter,Dimension::Scalar).ok(),"minimum":0.,"maximum":1.,"parameterization":"normalized_native_curve","arc_length_fraction":false,"editable":true}),
        });
    }
    fields
}
impl DirectionSource {
    pub fn dependencies(&self) -> Vec<NodeRef> {
        match self {
            Self::Explicit { .. } => vec![],
            Self::Axis { axis } => axis.dependencies(),
            Self::PlaneNormal { plane } => plane.dependencies(),
            Self::TwoPoints { points } => {
                points.iter().flat_map(PointSource::dependencies).collect()
            }
        }
    }
    pub fn expressions(&self) -> Vec<&Expr> {
        match self {
            Self::Explicit { direction } => direction.iter().collect(),
            Self::TwoPoints { points } => {
                points.iter().flat_map(PointSource::expressions).collect()
            }
            _ => vec![],
        }
    }
    pub fn resolve(
        &self,
        doc: &Document,
        owner: &str,
        resolver: &impl Resolver,
    ) -> Result<[f64; 3]> {
        Ok(normalized(match self {
            Self::Explicit { direction } => vector([
                quantity(doc, owner, &direction[0], Dimension::Scalar)?,
                quantity(doc, owner, &direction[1], Dimension::Scalar)?,
                quantity(doc, owner, &direction[2], Dimension::Scalar)?,
            ]),
            Self::Axis { axis } => vector(axis.resolve(resolver)?.direction),
            Self::PlaneNormal { plane } => normal(&plane.resolve(resolver)?),
            Self::TwoPoints { points } => {
                vector(points[1].resolve(doc, owner, resolver)?)
                    - vector(points[0].resolve(doc, owner, resolver)?)
            }
        })?
        .into())
    }
}
impl CoordinateSystemDefinition {
    pub fn validate(&self) -> Result<()> {
        if self.version != COORDINATE_SYSTEM_VERSION {
            return Err(invalid("unsupported coordinate-system contract version"));
        }
        if self.primary_axis == self.secondary_axis {
            return Err(invalid(
                "primary and secondary output axes must be different",
            ));
        }
        Ok(())
    }
    pub fn dependencies(&self) -> Vec<NodeRef> {
        self.origin
            .dependencies()
            .into_iter()
            .chain(self.primary.dependencies())
            .chain(self.secondary.dependencies())
            .collect()
    }
    pub fn expressions(&self) -> Vec<&Expr> {
        self.origin
            .expressions()
            .into_iter()
            .chain(self.primary.expressions())
            .chain(self.secondary.expressions())
            .collect()
    }
    pub fn evaluate(
        &self,
        doc: &Document,
        owner: &str,
        resolver: &impl Resolver,
    ) -> Result<CoordinateSystem> {
        self.validate()?;
        let origin = self.origin.resolve(doc, owner, resolver)?;
        let p = vector(self.primary.resolve(doc, owner, resolver)?)
            * if self.reverse_primary { -1. } else { 1. };
        let s = vector(self.secondary.resolve(doc, owner, resolver)?)
            * if self.reverse_secondary { -1. } else { 1. };
        let secondary = s - p * p.dot(&s);
        if secondary.norm() <= 1e-9 {
            return Err(invalid(
                "coordinate-system directions are parallel or too close to parallel",
            ));
        }
        let q = normalized(secondary)?;
        let index = |axis| match axis {
            OriginAxis::X => 0,
            OriginAxis::Y => 1,
            OriginAxis::Z => 2,
        };
        let i = index(self.primary_axis);
        let j = index(self.secondary_axis);
        let k = 3 - i - j;
        let mut axes = [Vector3::zeros(); 3];
        axes[i] = p;
        axes[j] = q;
        axes[k] = normalized(p.cross(&q) * if (i + 1) % 3 == j { 1. } else { -1. })?;
        let frame = CoordinateSystem {
            origin,
            x: axes[0].into(),
            y: axes[1].into(),
            z: axes[2].into(),
        };
        frame.validate()?;
        Ok(frame)
    }
}
fn quantity(doc: &Document, owner: &str, expression: &Expr, dimension: Dimension) -> Result<f64> {
    crate::expression::value(doc, owner, expression, dimension)
}
fn normal(plane: &Plane) -> Vector3<f64> {
    vector(plane.u).cross(&vector(plane.v))
}
fn frame(origin: [f64; 3], n: Vector3<f64>) -> Result<Plane> {
    let n = normalized(n)?;
    let seed = if n.x.abs() < 0.8 {
        Vector3::x()
    } else {
        Vector3::y()
    };
    let u = normalized(seed - n * seed.dot(&n))?;
    normalized_plane(Plane {
        origin,
        u: u.into(),
        v: n.cross(&u).into(),
    })
}
impl PlaneDefinition {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Offset { .. } => "Offset",
            Self::Angle { .. } => "At Angle",
            Self::ThroughPoints { .. } => "Three Points",
            Self::Midplane { .. } => "Midplane",
            Self::NormalToAxis { .. } => "Normal to Axis",
            Self::AngularBisector { .. } => "Angular Bisector",
            Self::Tangent { .. } => "Tangent to Face",
        }
    }
    pub fn dependencies(&self) -> Vec<NodeRef> {
        match self {
            Self::Offset { plane, .. } => plane.dependencies(),
            Self::Angle { plane, axis, .. } => plane
                .dependencies()
                .into_iter()
                .chain(axis.dependencies())
                .collect(),
            Self::ThroughPoints { points } => {
                points.iter().flat_map(PointSource::dependencies).collect()
            }
            Self::Midplane { planes } | Self::AngularBisector { planes, .. } => {
                planes.iter().flat_map(PlaneSource::dependencies).collect()
            }
            Self::NormalToAxis { axis, point } => axis
                .dependencies()
                .into_iter()
                .chain(point.dependencies())
                .collect(),
            Self::Tangent { source, .. } => source.dependencies(),
        }
    }
    pub fn expressions(&self) -> Vec<&Expr> {
        match self {
            Self::Offset { distance, .. } => vec![distance],
            Self::Angle { angle, .. } => vec![angle],
            Self::ThroughPoints { points } => {
                points.iter().flat_map(PointSource::expressions).collect()
            }
            Self::NormalToAxis { point, .. } => point.expressions(),
            Self::Tangent { source, .. } => source.expressions(),
            _ => vec![],
        }
    }
    pub fn evaluate(&self, doc: &Document, owner: &str, resolver: &impl Resolver) -> Result<Plane> {
        match self {
            Self::Offset {
                plane,
                distance,
                flip,
            } => {
                let mut p = plane.resolve(resolver)?;
                p.origin = (vector(p.origin)
                    + normal(&p) * quantity(doc, owner, distance, Dimension::Length)?)
                .into();
                if *flip {
                    p.v = p.v.map(|v| -v);
                }
                normalized_plane(p)
            }
            Self::Angle { plane, axis, angle } => {
                let p = plane.resolve(resolver)?;
                let axis = axis.resolve(resolver)?;
                let rotation = Rotation3::from_axis_angle(
                    &Unit::new_normalize(vector(axis.direction)),
                    quantity(doc, owner, angle, Dimension::Angle)?,
                );
                normalized_plane(Plane {
                    origin: (vector(axis.origin)
                        + rotation * (vector(p.origin) - vector(axis.origin)))
                    .into(),
                    u: (rotation * vector(p.u)).into(),
                    v: (rotation * vector(p.v)).into(),
                })
            }
            Self::ThroughPoints { points } => {
                let a = vector(points[0].resolve(doc, owner, resolver)?);
                let b = vector(points[1].resolve(doc, owner, resolver)?);
                let c = vector(points[2].resolve(doc, owner, resolver)?);
                let u = normalized(b - a)?;
                let n = normalized(u.cross(&(c - a)))?;
                normalized_plane(Plane {
                    origin: a.into(),
                    u: u.into(),
                    v: n.cross(&u).into(),
                })
            }
            Self::Midplane { planes } => {
                let mut a = planes[0].resolve(resolver)?;
                let b = planes[1].resolve(resolver)?;
                let n = normal(&a);
                if n.cross(&normal(&b)).norm() > 1e-9 {
                    return Err(CadError::Unsupported(
                        "Midplane requires parallel planes; choose Angular Bisector for intersecting planes"
                            .into(),
                    ));
                }
                let distance = (vector(b.origin) - vector(a.origin)).dot(&n);
                if distance.abs() <= 1e-9 {
                    return Err(invalid("midplane references are coincident"));
                }
                a.origin = (vector(a.origin) + n * distance * 0.5).into();
                Ok(a)
            }
            Self::NormalToAxis { axis, point } => frame(
                point.resolve(doc, owner, resolver)?,
                vector(axis.resolve(resolver)?.direction),
            ),
            Self::AngularBisector {
                planes,
                solution,
                reverse,
            } => {
                let a = planes[0].resolve(resolver)?;
                let b = planes[1].resolve(resolver)?;
                let n1 = normal(&a);
                let n2 = normal(&b);
                let direction = n1.cross(&n2);
                let length = direction.norm();
                if !length.is_finite() || length <= 1e-9 {
                    return Err(invalid(
                        "angular bisector references are parallel or coincident",
                    ));
                }
                // Solve relative to the first origin instead of subtracting
                // absolute plane constants at distant document coordinates.
                let delta = vector(b.origin) - vector(a.origin);
                let displacement = direction.cross(&n1) * (delta.dot(&n2) / length.powi(2));
                let origin = vector(a.origin) + displacement;
                let scale = delta
                    .norm()
                    .max(displacement.norm())
                    .max(vector(a.origin).norm());
                if !scale.is_finite()
                    || 32. * f64::EPSILON * scale / length > 1e-7
                    || displacement.dot(&n1).abs() > 1e-7
                    || (displacement - delta).dot(&n2).abs() > 1e-7
                {
                    return Err(invalid(
                        "angular bisector intersection is too ill-conditioned at these coordinates",
                    ));
                }
                let n = normalized(match solution {
                    BisectorSolution::SameSignedDistance => n1 - n2,
                    BisectorSolution::OppositeSignedDistance => n1 + n2,
                })?;
                let u = normalized(direction)?;
                let mut v = n.cross(&u);
                if *reverse {
                    v = -v;
                }
                normalized_plane(Plane {
                    origin: origin.into(),
                    u: u.into(),
                    v: v.into(),
                })
            }
            Self::Tangent { source, reverse } => {
                let mut plane = source.resolve(doc, owner, resolver)?;
                if *reverse {
                    plane.v = plane.v.map(|value| -value);
                }
                normalized_plane(plane)
            }
        }
    }
}
impl TangentPlaneSource {
    pub fn dependencies(&self) -> Vec<NodeRef> {
        let mut dependencies = vec![self.body.clone(), self.reference.producer.clone()];
        if let TangentAnchor::Point { point } = &self.anchor {
            dependencies.extend(point.dependencies());
        }
        dependencies.sort();
        dependencies.dedup();
        dependencies
    }
    pub fn expressions(&self) -> Vec<&Expr> {
        match &self.anchor {
            TangentAnchor::Parameters { parameters } => parameters.iter().collect(),
            TangentAnchor::Point { point } => point.expressions(),
        }
    }
    fn resolve(&self, doc: &Document, owner: &str, resolver: &impl Resolver) -> Result<Plane> {
        if self.reference.topology != "face" {
            return Err(invalid(
                "tangent planes require a persistent face reference",
            ));
        }
        let anchor = match &self.anchor {
            TangentAnchor::Parameters { parameters } => {
                let values = [
                    quantity(doc, owner, &parameters[0], Dimension::Scalar)?,
                    quantity(doc, owner, &parameters[1], Dimension::Scalar)?,
                ];
                if values
                    .iter()
                    .any(|value| !value.is_finite() || !(0. ..=1.).contains(value))
                {
                    return Err(invalid(
                        "tangent face parameters must be finite scalars in 0..1",
                    ));
                }
                TangentAnchorValue::Parameters(values)
            }
            TangentAnchor::Point { point } => {
                TangentAnchorValue::Point(point.resolve(doc, owner, resolver)?)
            }
        };
        normalized_plane(resolver.tangent_plane(&self.reference, &self.body, anchor)?)
    }
}
/// Task fields separate from the native editor's layout and visual styling.
pub fn plane_semantic_fields(
    doc: &Document,
    owner: &str,
    node: &str,
    definition: &PlaneDefinition,
    state: &str,
) -> Vec<crate::presentation::SemanticElement> {
    use crate::presentation::SemanticElement;
    let mut fields = Vec::new();
    let target = serde_json::json!({"kind":"feature","definition":owner,"feature":node});
    let source = match definition {
        PlaneDefinition::AngularBisector {
            planes,
            solution,
            reverse,
        } => {
            for (index, plane) in planes.iter().enumerate() {
                fields.push(SemanticElement {
                    id: format!("{node}::plane::source:{index}"), role: "feature-reference".into(),
                    label: format!("{} Plane", if index == 0 {"First"} else {"Second"}), value: None,
                    state: state.into(), commands: vec!["feature.edit".into()],
                    relationships: plane.dependencies().into_iter().map(|source| source.node).chain([node.into()]).collect(),
                    data: serde_json::json!({"contract":"tcad.reference_geometry","version":CONTRACT_VERSION,"field":format!("/definition/planes/{index}"),"type":"plane_source","source":plane,"editable":true,"target":target}),
                });
            }
            fields.push(SemanticElement {
                id: format!("{node}::plane::solution"), role:"feature-field".into(), label:"Bisector Solution".into(),
                value:Some(match solution { BisectorSolution::SameSignedDistance => "Equal signed distance", BisectorSolution::OppositeSignedDistance => "Opposite signed distance" }.into()),
                state:state.into(), commands:vec!["feature.edit".into()], relationships:vec![node.into()],
                data:serde_json::json!({"contract":"tcad.reference_geometry","version":CONTRACT_VERSION,"field":"/definition/solution","type":"choice","source":solution,"choices":["same_signed_distance","opposite_signed_distance"],"reverse":reverse,"editable":true,"target":target}),
            });
            None
        }
        PlaneDefinition::Tangent { source, .. } => Some(source),
        _ => None,
    };
    if let Some(source) = source {
        fields.push(SemanticElement {
            id:format!("{node}::plane::tangent-face"),role:"feature-reference".into(),label:"Tangent Face".into(),value:Some(source.body.node.clone()),
            state:state.into(),commands:vec!["feature.edit".into()],relationships:source.dependencies().into_iter().map(|source|source.node).chain([node.into()]).collect(),
            data:serde_json::json!({"contract":"tcad.reference_geometry","version":CONTRACT_VERSION,"field":"/definition/source","type":"bounded_face_reference","body":source.body,"reference":source.reference,"anchor":source.anchor,"editable":true,"target":target}),
        });
        if let TangentAnchor::Parameters { parameters } = &source.anchor {
            for (index, expression) in parameters.iter().enumerate() {
                fields.push(SemanticElement {
                    id:format!("{node}::plane::tangent-parameter:{index}"),role:"feature-field".into(),label:format!("Face {}",if index==0 {"U"} else {"V"}),value:Some(expression.0.clone()),
                    state:state.into(),commands:vec!["feature.edit".into()],relationships:vec![node.into(),source.body.node.clone()],
                    data:serde_json::json!({"contract":"tcad.reference_geometry","version":CONTRACT_VERSION,"field":format!("/definition/source/anchor/parameters/{index}"),"type":"quantity","dimension":Dimension::Scalar,"expression":expression,"evaluated":crate::expression::value(doc,owner,expression,Dimension::Scalar).ok(),"minimum":0.,"maximum":1.,"parameterization":"normalized_proven_face_domain","arc_length_fraction":false,"editable":true,"target":target}),
                });
            }
        }
    }
    let reverse = match definition {
        PlaneDefinition::AngularBisector { reverse, .. }
        | PlaneDefinition::Tangent { reverse, .. } => Some(reverse),
        _ => None,
    };
    if let Some(reverse) = reverse {
        fields.push(SemanticElement {
            id:format!("{node}::plane::reverse"),role:"feature-field".into(),label:"Reverse Plane Normal".into(),value:Some(reverse.to_string()),state:state.into(),commands:vec!["feature.edit".into()],relationships:vec![node.into()],
            data:serde_json::json!({"contract":"tcad.reference_geometry","version":CONTRACT_VERSION,"field":"/definition/reverse","type":"boolean","value":reverse,"editable":true,"target":target}),
        });
    }
    fields
}
impl AxisDefinition {
    pub fn label(&self) -> &'static str {
        match self {
            Self::ThroughPoints { .. } => "Two Points",
            Self::PlaneIntersection { .. } => "Two Planes",
            Self::NormalToPlane { .. } => "Normal to Plane",
            Self::AlongEdge { .. } => "Linear Edge / Axis",
            Self::Geometry { .. } => "Rotational Geometry",
            Self::Direction { .. } => "Point and Direction",
        }
    }
    pub fn dependencies(&self) -> Vec<NodeRef> {
        match self {
            Self::ThroughPoints { points } => {
                points.iter().flat_map(PointSource::dependencies).collect()
            }
            Self::PlaneIntersection { planes } => {
                planes.iter().flat_map(PlaneSource::dependencies).collect()
            }
            Self::NormalToPlane { plane, point } => plane
                .dependencies()
                .into_iter()
                .chain(point.dependencies())
                .collect(),
            Self::AlongEdge { axis } | Self::Geometry { axis } => axis.dependencies(),
            Self::Direction { origin, .. } => origin.dependencies(),
        }
    }
    pub fn expressions(&self) -> Vec<&Expr> {
        match self {
            Self::ThroughPoints { points } => {
                points.iter().flat_map(PointSource::expressions).collect()
            }
            Self::NormalToPlane { point, .. } => point.expressions(),
            Self::Direction { origin, direction } => origin
                .expressions()
                .into_iter()
                .chain(direction.iter())
                .collect(),
            _ => vec![],
        }
    }
    pub fn evaluate(&self, doc: &Document, owner: &str, resolver: &impl Resolver) -> Result<Axis> {
        let axis = match self {
            Self::ThroughPoints { points } => {
                let a = points[0].resolve(doc, owner, resolver)?;
                let b = points[1].resolve(doc, owner, resolver)?;
                Axis {
                    origin: a,
                    direction: normalized(vector(b) - vector(a))?.into(),
                }
            }
            Self::PlaneIntersection { planes } => {
                let a = planes[0].resolve(resolver)?;
                let b = planes[1].resolve(resolver)?;
                let n1 = normal(&a);
                let n2 = normal(&b);
                let d = n1.cross(&n2);
                if d.norm() <= 1e-9 {
                    return Err(invalid("planes are parallel or coincident"));
                }
                let o = (n2.cross(&d) * n1.dot(&vector(a.origin))
                    + d.cross(&n1) * n2.dot(&vector(b.origin)))
                    / d.norm_squared();
                Axis {
                    origin: o.into(),
                    direction: normalized(d)?.into(),
                }
            }
            Self::NormalToPlane { plane, point } => Axis {
                origin: point.resolve(doc, owner, resolver)?,
                direction: normal(&plane.resolve(resolver)?).into(),
            },
            Self::AlongEdge { axis } | Self::Geometry { axis } => axis.resolve(resolver)?,
            Self::Direction { origin, direction } => Axis {
                origin: origin.resolve(doc, owner, resolver)?,
                direction: [
                    quantity(doc, owner, &direction[0], Dimension::Scalar)?,
                    quantity(doc, owner, &direction[1], Dimension::Scalar)?,
                    quantity(doc, owner, &direction[2], Dimension::Scalar)?,
                ],
            },
        };
        Ok(Axis {
            origin: finite_point(axis.origin)?,
            direction: normalized(vector(axis.direction))?.into(),
        })
    }
}
impl PointDefinition {
    pub fn label(&self) -> &'static str {
        match self {
            Self::AtPoint { .. } => "Coordinates / Point",
            Self::OnAxis { .. } => "Distance Along Axis",
            Self::PlaneCoordinates { .. } => "On Plane",
            Self::AxisPlaneIntersection { .. } => "Axis / Plane Intersection",
        }
    }
    pub fn dependencies(&self) -> Vec<NodeRef> {
        match self {
            Self::AtPoint { point } => point.dependencies(),
            Self::OnAxis { axis, .. } => axis.dependencies(),
            Self::PlaneCoordinates { plane, .. } => plane.dependencies(),
            Self::AxisPlaneIntersection { axis, plane } => axis
                .dependencies()
                .into_iter()
                .chain(plane.dependencies())
                .collect(),
        }
    }
    pub fn expressions(&self) -> Vec<&Expr> {
        match self {
            Self::AtPoint { point } => point.expressions(),
            Self::OnAxis { distance, .. } => vec![distance],
            Self::PlaneCoordinates { u, v, offset, .. } => vec![u, v, offset],
            _ => vec![],
        }
    }
    pub fn evaluate(
        &self,
        doc: &Document,
        owner: &str,
        resolver: &impl Resolver,
    ) -> Result<[f64; 3]> {
        finite_point(match self {
            Self::AtPoint { point } => point.resolve(doc, owner, resolver)?,
            Self::OnAxis { axis, distance } => {
                let a = axis.resolve(resolver)?;
                (vector(a.origin)
                    + vector(a.direction) * quantity(doc, owner, distance, Dimension::Length)?)
                .into()
            }
            Self::PlaneCoordinates {
                plane,
                u,
                v,
                offset,
            } => {
                let p = plane.resolve(resolver)?;
                (vector(p.origin)
                    + vector(p.u) * quantity(doc, owner, u, Dimension::Length)?
                    + vector(p.v) * quantity(doc, owner, v, Dimension::Length)?
                    + normal(&p) * quantity(doc, owner, offset, Dimension::Length)?)
                .into()
            }
            Self::AxisPlaneIntersection { axis, plane } => {
                let a = axis.resolve(resolver)?;
                let p = plane.resolve(resolver)?;
                let n = normal(&p);
                let d = vector(a.direction);
                let den = n.dot(&d);
                if den.abs() <= 1e-9 {
                    return Err(invalid("axis is parallel to or lies in the plane"));
                }
                (vector(a.origin) + d * n.dot(&(vector(p.origin) - vector(a.origin))) / den).into()
            }
        })
    }
}
