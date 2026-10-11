use crate::{CadError, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub type Id = String;
pub fn id() -> Id {
    uuid::Uuid::new_v4().to_string()
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Expr(pub String);
impl From<f64> for Expr {
    fn from(v: f64) -> Self {
        Self(v.to_string())
    }
}
impl From<&str> for Expr {
    fn from(v: &str) -> Self {
        Self(v.into())
    }
}
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Dimension {
    #[default]
    Scalar,
    Length,
    Angle,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Parameter {
    pub name: String,
    pub expression: Expr,
    pub dimension: Dimension,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct NodeRef {
    pub definition: Id,
    pub node: Id,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Plane {
    pub origin: [f64; 3],
    pub u: [f64; 3],
    pub v: [f64; 3],
}
impl Default for Plane {
    fn default() -> Self {
        Self {
            origin: [0.; 3],
            u: [1., 0., 0.],
            v: [0., 1., 0.],
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Profile {
    Polygon { points: Vec<[f64; 2]> },
    Circle { radius: Expr, center: [f64; 2] },
    Sketch { sketch: Id },
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct TopologyRef {
    pub producer: NodeRef,
    pub role: String,
    pub topology: String,
    pub signature: Option<TopologySignature>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct TopologySignature {
    pub center: [f64; 3],
    pub measure: f64,
    pub tolerance: f64,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SheetBend {
    pub angle: Expr,
    pub inside_radius: Expr,
    pub k_factor: f64,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExtrudeEnd {
    #[default]
    Blind,
    MidPlane,
    ThroughAll,
    ThroughAllBoth,
    UpToFace,
    UpToVertex,
    UpToNext,
    UpToBody,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ThinExtrudeMode {
    #[default]
    OneSide,
    TwoSide,
    MidPlane,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ThinExtrude {
    #[serde(default)]
    pub mode: ThinExtrudeMode,
    pub thickness: Expr,
    #[serde(default)]
    pub second_thickness: Option<Expr>,
    #[serde(default)]
    pub reverse: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ExtrudeDraft {
    pub angle: Expr,
    #[serde(default)]
    pub outward: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ExtrudeOptions {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub contours: Vec<crate::contours::Selection>,
    #[serde(
        default,
        skip_serializing_if = "crate::extrusion::Start::is_sketch_plane"
    )]
    pub start: crate::extrusion::Start,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub direction2: Option<crate::extrusion::DirectionEnd>,
    /// Additional explicitly selected feature outputs. Every solid in each
    /// output belongs to this scope; the primary body remains included.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scope: Vec<NodeRef>,
    /// False preserves the explicit direction vector in older native files.
    #[serde(default)]
    pub profile_normal: bool,
    #[serde(default)]
    pub reverse_direction: bool,
    #[serde(default)]
    pub end: ExtrudeEnd,
    #[serde(default = "zero_length")]
    pub start_offset: Expr,
    #[serde(default)]
    pub reverse_distance: Option<Expr>,
    #[serde(default)]
    pub target: Option<TopologyRef>,
    #[serde(default)]
    pub target_body: Option<NodeRef>,
    #[serde(default = "zero_length")]
    pub end_offset: Expr,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thin: Option<ThinExtrude>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub draft: Option<ExtrudeDraft>,
    /// Stable sketch point identity for an Up to Vertex end condition.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_vertex: Option<(NodeRef, Id)>,
}
fn zero_length() -> Expr {
    "0 mm".into()
}
impl Default for ExtrudeOptions {
    fn default() -> Self {
        Self {
            contours: vec![],
            start: crate::extrusion::Start::default(),
            direction2: None,
            scope: vec![],
            profile_normal: false,
            reverse_direction: false,
            end: ExtrudeEnd::Blind,
            start_offset: zero_length(),
            reverse_distance: None,
            target: None,
            target_body: None,
            end_offset: zero_length(),
            thin: None,
            draft: None,
            target_vertex: None,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Operation {
    ReferencePlane {
        definition: crate::reference_geometry::PlaneDefinition,
    },
    ReferenceAxis {
        definition: crate::reference_geometry::AxisDefinition,
    },
    ReferencePoint {
        definition: crate::reference_geometry::PointDefinition,
    },
    ReferenceCoordinateSystem {
        definition: Box<crate::reference_geometry::CoordinateSystemDefinition>,
    },
    ReferenceCurve {
        definition: Box<crate::reference_curves::Definition>,
    },
    Rib {
        definition: Box<crate::rib::Definition>,
    },
    DirectEdit {
        definition: Box<crate::direct_edit::Definition>,
    },
    CurvePattern {
        definition: Box<crate::curve_pattern::Definition>,
    },
    PlacementPattern {
        definition: Box<crate::placement_pattern::Definition>,
    },
    BodyFilter {
        definition: Box<crate::body_filter::Definition>,
    },
    Split {
        definition: Box<crate::split::Definition>,
    },
    Surface {
        operation: crate::surface::SurfaceOperation,
    },
    Mold {
        operation: crate::mold::MoldOperation,
    },
    Profile {
        profile: Profile,
        plane: Plane,
        #[serde(default)]
        plane_reference: Option<TopologyRef>,
        #[serde(default)]
        reference_body: Option<NodeRef>,
    },
    Box {
        size: [Expr; 3],
    },
    Cylinder {
        radius: Expr,
        length: Expr,
        axis: [f64; 3],
    },
    Sphere {
        radius: Expr,
    },
    Extrude {
        profile: NodeRef,
        distance: Expr,
        direction: [f64; 3],
        #[serde(default)]
        body: Option<NodeRef>,
        #[serde(default)]
        mode: String,
        #[serde(default)]
        options: Box<ExtrudeOptions>,
    },
    Revolve {
        profile: NodeRef,
        origin: [f64; 3],
        axis: [f64; 3],
        #[serde(default, skip_serializing_if = "Option::is_none")]
        axis_reference: Option<crate::reference_geometry::AxisSource>,
        angle: Expr,
        #[serde(default)]
        body: Option<NodeRef>,
        #[serde(default)]
        mode: String,
        #[serde(
            default,
            skip_serializing_if = "crate::revolution::Options::is_default"
        )]
        options: Box<crate::revolution::Options>,
    },
    Boolean {
        left: NodeRef,
        right: NodeRef,
        mode: String,
    },
    Hole {
        body: NodeRef,
        radius: Expr,
        origin: [f64; 3],
        axis: [f64; 3],
        depth: Expr,
    },
    DrilledHoles {
        definition: Box<crate::holes::Definition>,
    },
    Fillet {
        body: NodeRef,
        edges: Vec<TopologyRef>,
        radius: Expr,
    },
    Chamfer {
        body: NodeRef,
        edges: Vec<TopologyRef>,
        distance: Expr,
        #[serde(default)]
        options: Box<crate::edge_features::ChamferOptions>,
    },
    ReferenceBody {
        body: NodeRef,
        instance: Id,
        #[serde(default)]
        relative_to: Option<Id>,
    },
    Transform {
        body: NodeRef,
        placement: Placement,
    },
    Scale {
        body: NodeRef,
        #[serde(default)]
        options: Box<crate::body_editing::ScaleOptions>,
    },
    Pattern {
        body: NodeRef,
        count: usize,
        step: [Expr; 3],
        angle: Expr,
        axis: [f64; 3],
        #[serde(default, skip_serializing_if = "Option::is_none")]
        axis_reference: Option<crate::reference_geometry::AxisSource>,
        #[serde(default)]
        options: Box<crate::body_editing::PatternOptions>,
    },
    Mirror {
        body: NodeRef,
        plane: Plane,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        plane_reference: Option<crate::reference_geometry::PlaneSource>,
        #[serde(default)]
        keep_original: bool,
    },
    Loft {
        profiles: Vec<NodeRef>,
        #[serde(default)]
        body: Option<NodeRef>,
        #[serde(default)]
        mode: String,
        #[serde(default, skip_serializing_if = "crate::solid_sections::loft_default")]
        options: Box<crate::solid_sections::LoftOptions>,
    },
    Sweep {
        profile: NodeRef,
        path: Vec<[f64; 3]>,
        #[serde(default)]
        path_sketch: Option<NodeRef>,
        #[serde(default)]
        body: Option<NodeRef>,
        #[serde(default)]
        mode: String,
        #[serde(default, skip_serializing_if = "crate::solid_sections::sweep_default")]
        options: Box<crate::solid_sections::SweepOptions>,
    },
    Shell {
        body: NodeRef,
        thickness: Expr,
        removed_faces: Vec<TopologyRef>,
        #[serde(default, skip_serializing_if = "crate::wall::is_default")]
        options: Box<crate::wall::Options>,
    },
    Draft {
        body: NodeRef,
        faces: Vec<TopologyRef>,
        angle: Expr,
        plane: Plane,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        plane_reference: Option<crate::reference_geometry::PlaneSource>,
    },
    StructuralMember {
        section: Profile,
        start: [f64; 3],
        end: [f64; 3],
        roll: Expr,
    },
    Sheet {
        operation: Box<crate::sheet_metal::SheetOperation>,
    },
    Weldment {
        operation: Box<crate::weldment::WeldmentOperation>,
    },
    SheetMetal {
        width: Expr,
        thickness: Expr,
        sections: Vec<Expr>,
        bends: Vec<SheetBend>,
    },
    Import {
        resource: Id,
        #[serde(default)]
        solid_ids: Vec<u64>,
    },
    ImportSurfaces {
        resource: Id,
        surface_model_ids: Vec<u64>,
    },
}
impl Operation {
    pub fn produces_solid(&self) -> bool {
        !self.is_reference()
            && !self.is_analysis()
            && !matches!(self, Self::Profile { .. })
            && !self.is_surface()
    }
    pub fn is_surface(&self) -> bool {
        matches!(self, Self::ImportSurfaces { .. })
            || matches!(self, Self::Surface { operation } if operation.is_surface())
            || matches!(self, Self::Mold { operation } if operation.is_surface())
    }
    pub fn is_analysis(&self) -> bool {
        matches!(self, Self::Mold { operation } if operation.is_analysis())
    }
    /// Existing output branches carried into or consumed by the resulting body set.
    pub fn consumed_outputs(&self) -> Vec<NodeRef> {
        match self {
            Self::Sheet { operation } => operation.consumed_outputs(),
            Self::Weldment { operation } => operation.consumed_outputs(),
            Self::Surface { operation } => operation.consumed_surfaces(),
            Self::DrilledHoles { definition } => vec![definition.body.clone()],
            Self::Rib { definition } => definition.consumed_outputs(),
            Self::DirectEdit { definition } => vec![definition.body.clone()],
            Self::CurvePattern { definition } => definition.consumed_outputs(),
            Self::PlacementPattern { definition } => definition.consumed_outputs(),
            Self::BodyFilter { definition } => definition.consumed_outputs(),
            Self::Split { definition } => definition.consumed_outputs(),
            Self::Mold { operation } => operation.consumed_outputs(),
            Self::Extrude { body, options, .. } => {
                let mut consumed: Vec<_> = body
                    .iter()
                    .cloned()
                    .chain(options.scope.iter().cloned())
                    .collect();
                let mut seen = BTreeSet::new();
                consumed.retain(|body| seen.insert(body.clone()));
                consumed
            }
            Self::Revolve { body, options, .. } => body
                .iter()
                .chain(&options.scope)
                .cloned()
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect(),
            Self::Loft { body, options, .. } => body
                .iter()
                .chain(&options.scope)
                .cloned()
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect(),
            Self::Sweep { body, options, .. } => body
                .iter()
                .chain(&options.scope)
                .cloned()
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect(),
            Self::Boolean { left, right, .. } => vec![left.clone(), right.clone()],
            Self::Hole { body, .. }
            | Self::Fillet { body, .. }
            | Self::Chamfer { body, .. }
            | Self::Transform { body, .. }
            | Self::Scale { body, .. }
            | Self::Pattern { body, .. }
            | Self::Mirror { body, .. }
            | Self::Shell { body, .. }
            | Self::Draft { body, .. } => vec![body.clone()],
            _ => vec![],
        }
    }
    pub fn is_reference(&self) -> bool {
        matches!(
            self,
            Self::ReferencePlane { .. }
                | Self::ReferenceAxis { .. }
                | Self::ReferencePoint { .. }
                | Self::ReferenceCoordinateSystem { .. }
                | Self::ReferenceCurve { .. }
        )
    }
    pub fn name(&self) -> &'static str {
        match self {
            Self::Sheet { operation } => operation.name(),
            Self::Weldment { operation } => operation.name(),
            Self::ReferencePlane { .. } => "Plane",
            Self::ReferenceAxis { .. } => "Axis",
            Self::ReferencePoint { .. } => "Point",
            Self::ReferenceCoordinateSystem { .. } => "Coordinate System",
            Self::ReferenceCurve { definition } => definition.name(),
            Self::Rib { .. } => "Rib",
            Self::DirectEdit { definition } => definition.name(),
            Self::CurvePattern { .. } => "Curve Pattern",
            Self::PlacementPattern { definition } => definition.label(),
            Self::BodyFilter { definition } => definition.name(),
            Self::Split { definition } => definition.name(),
            Self::Surface { operation } => operation.name(),
            Self::Mold { operation } => operation.name(),
            Self::Profile { .. } => "Sketch",
            Self::Box { .. } => "Box",
            Self::Cylinder { .. } => "Cylinder",
            Self::Sphere { .. } => "Sphere",
            Self::Extrude { .. } => "Extrude",
            Self::Revolve { .. } => "Revolve",
            Self::Boolean { .. } => "Boolean",
            Self::Hole { .. } => "Hole",
            Self::DrilledHoles { definition } => definition.name(),
            Self::Fillet { .. } => "Fillet",
            Self::Chamfer { .. } => "Chamfer",
            Self::Transform { .. } => "Transform",
            Self::Scale { .. } => "Scale",
            Self::ReferenceBody { .. } => "Associative reference",
            Self::Pattern { .. } => "Pattern",
            Self::Loft { .. } => "Loft",
            Self::Mirror { .. } => "Mirror",
            Self::Sweep { .. } => "Sweep",
            Self::Shell { .. } => "Shell",
            Self::Draft { .. } => "Draft",
            Self::Import { .. } => "Import",
            Self::ImportSurfaces { .. } => "Imported Surfaces",
            Self::StructuralMember { .. } => "Structural member",
            Self::SheetMetal { .. } => "Sheet metal",
        }
    }
    pub fn expressions(&self) -> Vec<&Expr> {
        match self {
            Self::Sheet { operation } => operation.expressions(),
            Self::Weldment { operation } => operation.expressions(),
            Self::ReferencePlane { definition } => definition.expressions(),
            Self::ReferenceAxis { definition } => definition.expressions(),
            Self::ReferencePoint { definition } => definition.expressions(),
            Self::ReferenceCoordinateSystem { definition } => definition.expressions(),
            Self::ReferenceCurve { definition } => definition.expressions(),
            Self::Rib { definition } => definition.expressions(),
            Self::DirectEdit { definition } => definition.expressions(),
            Self::CurvePattern { definition } => definition.expressions(),
            Self::PlacementPattern { definition } => definition.expressions(),
            Self::Split { definition } => definition.expressions(),
            Self::Surface { operation } => operation.expressions(),
            Self::DrilledHoles { definition } => definition.expressions(),
            Self::Mold { operation } => operation.expressions(),
            Self::Scale { options, .. } => options.expressions(),
            Self::Box { size, .. } => size.iter().collect(),
            Self::Cylinder { radius, length, .. } => vec![radius, length],
            Self::Sphere { radius, .. } => vec![radius],
            Self::Extrude {
                distance, options, ..
            } => {
                let mut values = vec![distance, &options.start_offset, &options.end_offset];
                values.extend(options.reverse_distance.as_ref());
                if let Some(thin) = &options.thin {
                    values.push(&thin.thickness);
                    values.extend(thin.second_thickness.as_ref());
                }
                values.extend(options.draft.as_ref().map(|draft| &draft.angle));
                values.extend(crate::extrusion::expressions(options));
                values
            }
            Self::Revolve { angle, options, .. } => {
                let mut fields = vec![angle];
                fields.extend(options.expressions());
                fields
            }
            Self::Draft { angle, .. } => vec![angle],
            Self::Loft { options, .. } => options.expressions(),
            Self::Sweep { options, .. } => options.expressions(),
            Self::Hole { radius, depth, .. } => vec![radius, depth],
            Self::Fillet { radius, .. } => vec![radius],
            Self::Chamfer {
                distance, options, ..
            } => {
                let mut fields = vec![distance];
                match options.method {
                    crate::edge_features::ChamferMethod::EqualDistance => {}
                    crate::edge_features::ChamferMethod::TwoDistances => {
                        fields.push(&options.second_distance)
                    }
                    crate::edge_features::ChamferMethod::DistanceAngle => {
                        fields.push(&options.angle)
                    }
                }
                fields
            }
            Self::Pattern {
                step,
                angle,
                options,
                ..
            } => step
                .iter()
                .chain(std::iter::once(angle))
                .chain(options.expressions())
                .collect(),
            Self::Shell {
                thickness, options, ..
            } => std::iter::once(thickness)
                .chain(options.face_thicknesses.iter().map(|wall| &wall.thickness))
                .collect(),
            Self::StructuralMember { roll, .. } => vec![roll],
            Self::SheetMetal {
                width,
                thickness,
                sections,
                bends,
            } => std::iter::once(width)
                .chain(std::iter::once(thickness))
                .chain(sections.iter())
                .chain(bends.iter().flat_map(|b| [&b.angle, &b.inside_radius]))
                .collect(),
            Self::Profile {
                profile: Profile::Circle { radius, .. },
                ..
            } => vec![radius],
            _ => vec![],
        }
    }
    pub fn dependencies(&self) -> Vec<NodeRef> {
        let mut dependencies = match self {
            Self::Sheet { operation } => operation.dependencies(),
            Self::Weldment { operation } => operation.dependencies(),
            Self::ReferencePlane { definition } => definition.dependencies(),
            Self::ReferenceAxis { definition } => definition.dependencies(),
            Self::ReferencePoint { definition } => definition.dependencies(),
            Self::ReferenceCoordinateSystem { definition } => definition.dependencies(),
            Self::ReferenceCurve { definition } => definition.dependencies(),
            Self::Rib { definition } => definition.dependencies(),
            Self::DirectEdit { definition } => definition.dependencies(),
            Self::CurvePattern { definition } => definition.dependencies(),
            Self::PlacementPattern { definition } => definition.dependencies(),
            Self::BodyFilter { definition } => definition.dependencies(),
            Self::Split { definition } => definition.dependencies(),
            Self::Surface { operation } => operation.dependencies(),
            Self::DrilledHoles { definition } => definition.dependencies(),
            Self::Mold { operation } => operation.dependencies(),
            Self::Extrude {
                profile,
                body: Some(body),
                ..
            }
            | Self::Revolve {
                profile,
                body: Some(body),
                ..
            } => vec![body.clone(), profile.clone()],
            Self::Extrude { profile, .. }
            | Self::Revolve { profile, .. }
            | Self::Sweep { profile, .. } => vec![profile.clone()],
            Self::Boolean { left, right, .. } => vec![left.clone(), right.clone()],
            Self::Profile {
                plane_reference: Some(reference),
                reference_body,
                ..
            } => vec![
                reference_body
                    .as_ref()
                    .unwrap_or(&reference.producer)
                    .clone(),
            ],
            Self::ReferenceBody { body, .. } => vec![body.clone()],
            Self::Mirror { body, .. }
            | Self::Hole { body, .. }
            | Self::Fillet { body, .. }
            | Self::Chamfer { body, .. }
            | Self::Transform { body, .. }
            | Self::Scale { body, .. }
            | Self::Pattern { body, .. }
            | Self::Shell { body, .. }
            | Self::Draft { body, .. } => vec![body.clone()],
            Self::Loft { profiles, .. } => profiles.clone(),
            _ => vec![],
        };
        match self {
            Self::Scale { options, .. } => dependencies.extend(options.center.dependencies()),
            Self::Pattern { options, .. } if !options.rotate_instances => {
                dependencies.extend(options.seed.dependencies())
            }
            _ => {}
        }
        match self {
            Self::Revolve { axis_reference, .. } | Self::Pattern { axis_reference, .. } => {
                dependencies.extend(
                    axis_reference
                        .iter()
                        .flat_map(crate::reference_geometry::AxisSource::dependencies),
                )
            }
            Self::Mirror {
                plane_reference, ..
            }
            | Self::Draft {
                plane_reference, ..
            } => dependencies.extend(
                plane_reference
                    .iter()
                    .flat_map(crate::reference_geometry::PlaneSource::dependencies),
            ),
            _ => {}
        }
        match self {
            Self::Extrude { options, .. } => {
                dependencies.extend(crate::extrusion::dependencies(options));
                dependencies.extend(options.target_body.clone().or_else(|| {
                    options
                        .target
                        .as_ref()
                        .map(|target| target.producer.clone())
                }));
                dependencies.extend(options.target_vertex.as_ref().map(|(node, _)| node.clone()));
            }
            Self::Revolve { body, options, .. } => {
                dependencies.extend(body.clone());
                dependencies.extend(options.scope.iter().cloned());
                dependencies.extend(
                    options
                        .contours
                        .iter()
                        .map(|selection| selection.profile.clone()),
                );
            }
            Self::Loft { body, options, .. } => {
                dependencies.extend(body.clone());
                dependencies.extend(options.dependencies());
            }
            Self::Sweep {
                body,
                path_sketch,
                options,
                ..
            } => {
                dependencies.extend(body.clone());
                dependencies.extend(path_sketch.clone());
                dependencies.extend(options.scope.iter().cloned());
                dependencies.extend(
                    options
                        .contours
                        .iter()
                        .map(|selection| selection.profile.clone()),
                );
            }
            _ => {}
        }
        let mut seen = BTreeSet::new();
        dependencies.retain(|dependency| seen.insert(dependency.clone()));
        dependencies
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Feature {
    pub id: Id,
    pub name: String,
    pub operation: Operation,
    #[serde(default)]
    pub suppressed: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Part {
    pub id: Id,
    pub name: String,
    pub parameters: BTreeMap<Id, Parameter>,
    pub features: Vec<Feature>,
    pub outputs: Vec<Id>,
    pub material: Id,
    #[serde(
        default = "crate::appearance::default_finish",
        skip_serializing_if = "crate::appearance::is_default_finish"
    )]
    pub finish: Id,
    #[serde(default)]
    pub properties: BTreeMap<String, String>,
    #[serde(default)]
    pub sketches: BTreeMap<Id, Sketch>,
    #[serde(default)]
    pub configurations: BTreeMap<Id, BTreeMap<Id, Expr>>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub configuration_details: BTreeMap<Id, crate::configuration::Definition>,
}
impl Part {
    pub fn feature_dependencies(&self, feature: &Feature) -> Vec<NodeRef> {
        let mut dependencies = feature.operation.dependencies();
        if let Operation::Profile {
            profile: Profile::Sketch { sketch },
            ..
        } = &feature.operation
            && let Some(sketch) = self.sketches.get(sketch)
        {
            dependencies.extend(sketch.conversions.values().map(|c| c.body.clone()));
        }
        let mut seen = BTreeSet::new();
        dependencies.retain(|dependency| seen.insert(dependency.clone()));
        dependencies
    }
    pub fn new(name: &str) -> Self {
        Self {
            id: id(),
            name: name.into(),
            parameters: BTreeMap::new(),
            features: vec![],
            outputs: vec![],
            material: "aluminum".into(),
            finish: crate::appearance::default_finish(),
            properties: BTreeMap::new(),
            sketches: BTreeMap::new(),
            configurations: BTreeMap::new(),
            configuration_details: BTreeMap::new(),
        }
    }
    pub fn reference(&self, node: &str) -> NodeRef {
        NodeRef {
            definition: self.id.clone(),
            node: node.into(),
        }
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Placement {
    pub translation: [f64; 3],
    pub rotation_deg: [f64; 3],
}
impl Placement {
    pub fn matrix(&self) -> nalgebra::Matrix4<f64> {
        let a = self.rotation_deg.map(f64::to_radians);
        let r = nalgebra::Rotation3::from_euler_angles(a[0], a[1], a[2]);
        nalgebra::Translation3::from(nalgebra::Vector3::from(self.translation)).to_homogeneous()
            * r.to_homogeneous()
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Instance {
    pub id: Id,
    pub name: String,
    pub definition: Id,
    pub placement: Placement,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finish: Option<Id>,
    #[serde(default)]
    pub configuration: Option<Id>,
    #[serde(default)]
    pub fixed: bool,
    #[serde(default)]
    pub translation_expressions: Option<[Expr; 3]>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Mate {
    pub id: Id,
    pub kind: String,
    pub first: Id,
    pub second: Id,
    pub first_origin: [f64; 3],
    pub second_origin: [f64; 3],
    pub first_axis: [f64; 3],
    pub second_axis: [f64; 3],
    pub offset: Expr,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_reference: Option<crate::assembly_geometry::Reference>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub second_reference: Option<crate::assembly_geometry::Reference>,
    #[serde(default)]
    pub alignment: crate::assembly_geometry::Alignment,
    #[serde(default)]
    pub lock_rotation: bool,
    #[serde(default = "zero_angle")]
    pub rotation_offset: Expr,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lock_placement: Option<Placement>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relationship: Option<Box<crate::assembly_geometry::Relationship>>,
}
fn zero_angle() -> Expr {
    "0 deg".into()
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ParameterRef {
    pub definition: Id,
    pub parameter: Id,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Assembly {
    pub id: Id,
    pub name: String,
    pub instances: BTreeMap<Id, Instance>,
    #[serde(default)]
    pub mates: Vec<Mate>,
    #[serde(default)]
    pub promoted_parameters: BTreeMap<Id, ParameterRef>,
    #[serde(default)]
    pub parameters: BTreeMap<Id, Parameter>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub configurations: BTreeMap<Id, crate::configuration::Definition>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub component_patterns: BTreeMap<Id, crate::component_pattern::Definition>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub exploded_views: BTreeMap<Id, crate::exploded_views::Definition>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub inspections: BTreeMap<Id, crate::assembly_inspection::Definition>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Sketch {
    pub id: Id,
    pub name: String,
    pub points: BTreeMap<Id, [f64; 2]>,
    pub segments: Vec<[Id; 2]>,
    #[serde(default)]
    pub constraints: Vec<SketchConstraint>,
    pub profile_order: Vec<Id>,
    #[serde(default)]
    pub circles: BTreeMap<Id, SketchCircle>,
    #[serde(default)]
    pub members: BTreeMap<Id, SketchMember>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub conversions: BTreeMap<Id, crate::sketch_edit::Conversion>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub offsets: BTreeMap<Id, crate::sketch_edit::Offset>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub fillets: BTreeMap<Id, crate::sketch_edit::Fillet>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub chamfers: BTreeMap<Id, crate::sketch_edit::Chamfer>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub transforms: BTreeMap<Id, crate::sketch_transform::Pattern>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub splines: BTreeMap<Id, crate::sketch_spline::Definition>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub conics: BTreeMap<Id, crate::sketch_conic::Definition>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub texts: BTreeMap<Id, crate::sketch_text::Definition>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub equations: BTreeMap<Id, crate::sketch_equation::Definition>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub blocks: BTreeMap<Id, crate::sketch_blocks::Definition>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub block_instances: BTreeMap<Id, crate::sketch_blocks::Instance>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub dimension_groups: BTreeMap<Id, crate::sketch_dimensions::Group>,
}
impl Sketch {
    pub fn expressions(&self) -> Vec<&Expr> {
        let mut values = self
            .constraints
            .iter()
            .map(|constraint| &constraint.value)
            .collect::<Vec<_>>();
        values.extend(
            self.members
                .values()
                .flat_map(|member| member.curve.expressions()),
        );
        values.extend(self.circles.values().map(|circle| &circle.radius));
        values.extend(self.offsets.values().map(|offset| &offset.distance));
        values.extend(self.fillets.values().flat_map(|fillet| {
            std::iter::once(&fillet.radius).chain(
                fillet
                    .source_curves
                    .values()
                    .flat_map(SketchCurve::expressions),
            )
        }));
        values.extend(
            self.chamfers
                .values()
                .flat_map(|chamfer| chamfer.size.expressions()),
        );
        values.extend(
            self.transforms
                .values()
                .flat_map(crate::sketch_transform::Pattern::expressions),
        );
        values.extend(
            self.splines
                .values()
                .flat_map(crate::sketch_spline::Definition::expressions),
        );
        values.extend(
            self.conics
                .values()
                .flat_map(crate::sketch_conic::Definition::expressions),
        );
        values.extend(
            self.texts
                .values()
                .flat_map(crate::sketch_text::Definition::expressions),
        );
        values.extend(
            self.equations
                .values()
                .flat_map(crate::sketch_equation::Definition::expressions),
        );
        values.extend(
            self.blocks
                .values()
                .flat_map(crate::sketch_blocks::Definition::expressions),
        );
        values.extend(
            self.block_instances
                .values()
                .flat_map(crate::sketch_blocks::Instance::expressions),
        );
        values
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SketchMember {
    #[serde(default)]
    pub construction: bool,
    pub curve: SketchCurve,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SketchCurve {
    Line {
        start: Id,
        end: Id,
    },
    Arc {
        start: Id,
        through: Id,
        end: Id,
    },
    Bezier {
        points: Vec<Id>,
    },
    Nurbs {
        points: Vec<Id>,
        weights: Vec<f64>,
        knots: Vec<f64>,
        degree: usize,
    },
    Ellipse {
        center: Id,
        major: Expr,
        minor: Expr,
        angle: Expr,
    },
    Circle {
        center: Id,
        radius: Expr,
    },
}
impl SketchCurve {
    pub fn expressions(&self) -> Vec<&Expr> {
        match self {
            Self::Circle { radius, .. } => vec![radius],
            Self::Ellipse {
                major,
                minor,
                angle,
                ..
            } => vec![major, minor, angle],
            _ => vec![],
        }
    }
    pub fn points(&self) -> Vec<&str> {
        match self {
            Self::Line { start, end } => vec![start, end],
            Self::Arc {
                start,
                through,
                end,
            } => vec![start, through, end],
            Self::Bezier { points } | Self::Nurbs { points, .. } => {
                points.iter().map(String::as_str).collect()
            }
            Self::Ellipse { center, .. } | Self::Circle { center, .. } => vec![center],
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SketchCircle {
    pub center: Id,
    pub radius: Expr,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SketchConstraint {
    pub id: Id,
    pub kind: String,
    pub entities: Vec<Id>,
    pub value: Expr,
    #[serde(default, skip_serializing_if = "crate::sketch_actions::is_false")]
    pub driven: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contact: Option<crate::sketch_relations::Contact>,
    /// Picked side of an oriented line for a perpendicular point-line gap.
    /// Absent values retain the legacy unsigned-distance constraint.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub distance_side: Option<crate::sketch_actions::DistanceSide>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Material {
    pub id: Id,
    pub name: String,
    pub density_kg_m3: f64,
    pub color: [f32; 3],
    pub metallic: f32,
    pub roughness: f32,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Resource {
    pub id: Id,
    pub path: String,
    pub sha256: String,
    pub kind: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ReviewComment {
    pub id: Id,
    pub target: Id,
    pub text: String,
    pub author: String,
    pub resolved: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct DrawingDimension {
    pub id: Id,
    pub view: String,
    pub first: [f64; 3],
    pub second: [f64; 3],
    pub offset_mm: f64,
    #[serde(default)]
    pub parameter: Option<ParameterRef>,
    #[serde(default)]
    pub tolerance: String,
    #[serde(default)]
    pub options: crate::drawing::DimensionOptions,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Drawing {
    pub id: Id,
    pub name: String,
    pub definition: Id,
    pub views: Vec<String>,
    pub annotations: Vec<String>,
    pub paper_mm: [f64; 2],
    #[serde(default)]
    pub dimensions: Vec<DrawingDimension>,
    #[serde(default)]
    pub title_block: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub entities: Vec<crate::drawing::Annotation>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tables: Vec<crate::drawing_tables::Table>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<crate::drawing::SheetFormat>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub view_layouts: BTreeMap<String, crate::drawing::ViewLayout>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exploded_view: Option<Id>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Document {
    pub schema_version: u32,
    pub id: Id,
    pub name: String,
    pub revision: u64,
    pub root: Id,
    pub parts: BTreeMap<Id, Part>,
    pub assemblies: BTreeMap<Id, Assembly>,
    pub materials: BTreeMap<Id, Material>,
    #[serde(
        default = "crate::appearance::default_finishes",
        skip_serializing_if = "crate::appearance::is_default_finishes"
    )]
    pub finishes: BTreeMap<Id, crate::appearance::Finish>,
    #[serde(
        default,
        skip_serializing_if = "crate::appearance::is_default_lighting"
    )]
    pub lighting: crate::appearance::Lighting,
    #[serde(default, skip_serializing_if = "crate::settings::is_default_settings")]
    pub settings: crate::settings::ModelSettings,
    #[serde(default)]
    pub resources: BTreeMap<Id, Resource>,
    #[serde(default)]
    pub drawings: Vec<Drawing>,
    #[serde(default)]
    pub comments: Vec<ReviewComment>,
    #[serde(default)]
    pub properties: BTreeMap<String, String>,
    #[serde(default)]
    pub libraries: BTreeMap<Id, crate::library::ComponentLink>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub active_configurations: BTreeMap<Id, Id>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PlacedPart {
    pub path: Vec<Id>,
    pub definition: Id,
    pub name: String,
    pub matrix: [[f64; 4]; 4],
    pub configuration: Option<Id>,
    #[serde(default)]
    pub mesh_key: Id,
    #[serde(default)]
    pub bom_excluded: bool,
    /// Effective part/configuration/occurrence surface finish, independent of its mass material.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finish: Option<Id>,
}
impl Document {
    pub fn fingerprint(&self) -> String {
        use sha2::{Digest, Sha256};
        format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(self).unwrap_or_default())
        )
    }
    pub fn validate(&self) -> Result<()> {
        self.settings.validate(self)?;
        crate::appearance::validate(self)?;
        crate::exploded_views::validate(self)?;
        crate::assembly_inspection::validate(self)?;
        crate::drawing::validate_document(self)?;
        fn valid_id(id: &str) -> bool {
            !id.is_empty()
                && id.len() <= 256
                && !matches!(id, "." | "..")
                && !id
                    .chars()
                    .any(|c| c.is_control() || matches!(c, '/' | '\\' | ':'))
        }
        if !valid_id(&self.id)
            || self
                .parts
                .keys()
                .chain(self.assemblies.keys())
                .any(|id| !valid_id(id))
        {
            return Err(CadError::Invalid(
                "Document and definition identities must be portable filename identifiers".into(),
            ));
        }
        let mut filenames = BTreeSet::new();
        for key in self.parts.keys().chain(self.assemblies.keys()) {
            if !filenames.insert(key.to_lowercase()) {
                return Err(CadError::Invalid(
                    "Definition identities collide on a case-insensitive filesystem".into(),
                ));
            }
        }
        if self.schema_version != 1 {
            return Err(CadError::Invalid(format!(
                "Unsupported document schema {}",
                self.schema_version
            )));
        }
        crate::library::validate_pins(self)?;
        let mut ids = BTreeSet::new();
        for (key, part) in &self.parts {
            if key != &part.id || !ids.insert(key) {
                return Err(CadError::Invalid("Invalid part identity".into()));
            }
            for node in &part.features {
                if let Operation::ImportSurfaces {
                    resource,
                    surface_model_ids,
                } = &node.operation
                    && (!self.resources.contains_key(resource)
                        || surface_model_ids.is_empty()
                        || surface_model_ids.len() > 4096
                        || surface_model_ids.contains(&0)
                        || surface_model_ids.iter().collect::<BTreeSet<_>>().len()
                            != surface_model_ids.len())
                {
                    return Err(CadError::Invalid(
                        "Imported surfaces require a pinned resource and 1–4096 distinct STEP model identities".into(),
                    ));
                }
                if let Operation::ReferenceCoordinateSystem { definition } = &node.operation {
                    definition.validate()?;
                }
                if let Operation::ReferenceCurve { definition } = &node.operation {
                    definition.validate()?;
                }
                if let Operation::BodyFilter { definition } = &node.operation {
                    definition.validate()?;
                }
                if let Operation::Split { definition } = &node.operation {
                    definition.validate()?;
                }
                if let Operation::PlacementPattern { definition } = &node.operation {
                    definition.validate()?;
                }
                if !ids.insert(&node.id) {
                    return Err(CadError::Invalid(format!(
                        "Duplicate feature identity {}",
                        node.id
                    )));
                }
            }
            for out in &part.outputs {
                if !part.features.iter().any(|f| {
                    &f.id == out && (f.operation.produces_solid() || f.operation.is_surface())
                }) {
                    return Err(CadError::Invalid(format!(
                        "Missing solid or surface body output {out}"
                    )));
                }
            }
        }
        for (key, assembly) in &self.assemblies {
            if key != &assembly.id || !ids.insert(key) {
                return Err(CadError::Invalid("Invalid assembly identity".into()));
            }
            for (key, pattern) in &assembly.component_patterns {
                if key != &pattern.id || !valid_id(key) || !ids.insert(key) {
                    return Err(CadError::Invalid(
                        "Invalid component pattern identity".into(),
                    ));
                }
                for reservation in &pattern.mates {
                    if !valid_id(&reservation.mate) || !ids.insert(&reservation.mate) {
                        return Err(CadError::Invalid("Invalid copied mate identity".into()));
                    }
                }
            }
            for (key, view) in &assembly.exploded_views {
                if key != &view.id || !valid_id(key) || !ids.insert(key) {
                    return Err(CadError::Invalid("Invalid exploded view identity".into()));
                }
                for step in &view.steps {
                    if !valid_id(&step.id) || !ids.insert(&step.id) {
                        return Err(CadError::Invalid("Invalid explode step identity".into()));
                    }
                }
            }
            for (key, inspection) in &assembly.inspections {
                if key != &inspection.id || !valid_id(key) || !ids.insert(key) {
                    return Err(CadError::Invalid(
                        "Invalid assembly inspection identity".into(),
                    ));
                }
            }
            for (key, instance) in &assembly.instances {
                if key != &instance.id || !ids.insert(key) {
                    return Err(CadError::Invalid("Invalid instance identity".into()));
                }
                if instance
                    .placement
                    .translation
                    .iter()
                    .chain(&instance.placement.rotation_deg)
                    .any(|v| !v.is_finite())
                {
                    return Err(CadError::Invalid("Invalid instance placement".into()));
                }
                if let Some(configuration) = &instance.configuration
                    && !crate::configuration::exists(self, &instance.definition, configuration)
                {
                    return Err(CadError::Invalid(format!(
                        "Missing configuration {configuration}"
                    )));
                }
            }
            let mut mate_ids = std::collections::BTreeSet::new();
            for mate in &assembly.mates {
                if let Some(relationship) = &mate.relationship {
                    if relationship.kind() != mate.kind {
                        return Err(CadError::Invalid(
                            "Mate type and relationship intent differ".into(),
                        ));
                    }
                    relationship.validate(self, assembly)?;
                    match relationship.as_ref() {
                        crate::assembly_geometry::Relationship::Width{first_wall,second_wall,..} if first_wall.instance!=mate.first||second_wall.instance!=mate.second => return Err(CadError::Invalid("Width wall pairs must belong to their respective primary components".into())),
                        crate::assembly_geometry::Relationship::Hinge{first_plane,second_plane,..} if first_plane.instance!=mate.first||second_plane.instance!=mate.second => return Err(CadError::Invalid("Hinge seating planes must belong to their respective primary components".into())),
                        crate::assembly_geometry::Relationship::Coupler{support:Some(s),..} if s==&mate.first||s==&mate.second => return Err(CadError::Invalid("Choose a separate bearing/support component or the assembly universe for mechanical motion coordinates".into())),
                        _ => {}
                    }
                }
                if !valid_id(&mate.id) || !mate_ids.insert(&mate.id) {
                    return Err(CadError::Invalid(
                        "Invalid or duplicate mate identity".into(),
                    ));
                }
                if mate
                    .first_origin
                    .iter()
                    .chain(&mate.second_origin)
                    .chain(&mate.first_axis)
                    .chain(&mate.second_axis)
                    .any(|v| !v.is_finite())
                {
                    return Err(CadError::Invalid(
                        "Mate frames must contain finite coordinates".into(),
                    ));
                }
                if !assembly.instances.contains_key(&mate.first)
                    || !assembly.instances.contains_key(&mate.second)
                    || mate.first == mate.second
                {
                    return Err(CadError::Invalid("Invalid mate participants".into()));
                }
                for (participant, reference) in [
                    (&mate.first, mate.first_reference.as_ref()),
                    (&mate.second, mate.second_reference.as_ref()),
                ] {
                    if let Some(reference) = reference {
                        let role = if participant == &mate.first {
                            "first"
                        } else {
                            "second"
                        };
                        if !crate::assembly_geometry::reference_classes(&mate.kind, role)
                            .contains(&reference.source.class())
                        {
                            return Err(CadError::Invalid(format!(
                                "{} mate {role} reference has the wrong geometry type",
                                mate.kind
                            )));
                        }
                        crate::assembly_geometry::validate_reference(
                            self,
                            &assembly.instances[participant].definition,
                            reference,
                        )?;
                    }
                }
                if let Some(placement) = &mate.lock_placement
                    && placement
                        .translation
                        .iter()
                        .chain(placement.rotation_deg.iter())
                        .any(|v| !v.is_finite())
                {
                    return Err(CadError::Invalid(
                        "Lock mate placement must be finite".into(),
                    ));
                }
            }
            for reference in assembly.promoted_parameters.values() {
                let params = self
                    .parts
                    .get(&reference.definition)
                    .map(|p| &p.parameters)
                    .or_else(|| {
                        self.assemblies
                            .get(&reference.definition)
                            .map(|a| &a.parameters)
                    })
                    .ok_or_else(|| CadError::Invalid("Missing promoted parameter owner".into()))?;
                if !params.contains_key(&reference.parameter) {
                    return Err(CadError::Invalid("Missing promoted parameter".into()));
                }
            }
        }
        for part in self.parts.values() {
            if !self.materials.contains_key(&part.material) {
                return Err(CadError::Invalid(format!(
                    "Missing material {}",
                    part.material
                )));
            }
            for sketch in part.sketches.values() {
                crate::sketch_edit::validate(sketch)?;
                for text in sketch.texts.values() {
                    crate::sketch_text::validate_font_resource(self, &text.font)?;
                }
                if sketch.points.values().flatten().any(|v| !v.is_finite())
                    || sketch
                        .profile_order
                        .iter()
                        .any(|id| !sketch.points.contains_key(id))
                    || sketch
                        .segments
                        .iter()
                        .flatten()
                        .any(|id| !sketch.points.contains_key(id))
                {
                    return Err(CadError::Invalid(
                        "Invalid sketch geometry or entity reference".into(),
                    ));
                }
                if sketch.members.values().any(|m| {
                    m.curve
                        .points()
                        .iter()
                        .any(|id| !sketch.points.contains_key(*id))
                }) {
                    return Err(CadError::Invalid(
                        "A sketch member has a missing point".into(),
                    ));
                }
                if sketch
                    .circles
                    .values()
                    .any(|c| !sketch.points.contains_key(&c.center))
                {
                    return Err(CadError::Invalid(
                        "A sketch circle has a missing center".into(),
                    ));
                }
            }
            for overrides in part.configurations.values() {
                if overrides.keys().any(|id| !part.parameters.contains_key(id)) {
                    return Err(CadError::Invalid(
                        "Configuration references an unknown parameter".into(),
                    ));
                }
            }
        }
        for material in self.materials.values() {
            if !material.density_kg_m3.is_finite()
                || material.density_kg_m3 <= 0.
                || !material
                    .color
                    .iter()
                    .all(|v| v.is_finite() && (0.0..=1.0).contains(v))
                || !(0.0..=1.0).contains(&material.metallic)
                || !(0.0..=1.0).contains(&material.roughness)
            {
                return Err(CadError::Invalid("Invalid physical material values".into()));
            }
        }
        for drawing in &self.drawings {
            crate::drawing::validate_definition(drawing)?;
            if drawing.paper_mm.iter().any(|v| !v.is_finite() || *v <= 0.)
                || drawing.dimensions.iter().any(|d| {
                    d.first
                        .iter()
                        .chain(&d.second)
                        .chain(std::iter::once(&d.offset_mm))
                        .any(|v| !v.is_finite())
                })
            {
                return Err(CadError::Invalid(
                    "Drawing sheets and dimensions must contain finite, valid coordinates".into(),
                ));
            }
            if !self.parts.contains_key(&drawing.definition)
                && !self.assemblies.contains_key(&drawing.definition)
            {
                return Err(CadError::Invalid(
                    "Drawing has a broken definition reference".into(),
                ));
            }
        }
        self.evaluation_order()?;
        crate::configuration::validate(self)?;
        crate::component_pattern::validate(self)?;
        self.placed_parts()?;
        Ok(())
    }
    pub fn evaluation_order(&self) -> Result<Vec<NodeRef>> {
        fn visit(
            doc: &Document,
            r: &NodeRef,
            temporary: &mut BTreeSet<NodeRef>,
            done: &mut BTreeSet<NodeRef>,
            order: &mut Vec<NodeRef>,
        ) -> Result<()> {
            if done.contains(r) {
                return Ok(());
            }
            if !temporary.insert(r.clone()) {
                return Err(CadError::Invalid(format!("Dependency cycle at {}", r.node)));
            }
            let feature = doc
                .parts
                .get(&r.definition)
                .and_then(|p| p.features.iter().find(|f| f.id == r.node))
                .ok_or_else(|| CadError::Invalid(format!("Missing feature {}", r.node)))?;
            for dep in doc.parts[&r.definition].feature_dependencies(feature) {
                visit(doc, &dep, temporary, done, order)?;
            }
            temporary.remove(r);
            done.insert(r.clone());
            order.push(r.clone());
            Ok(())
        }
        let mut order = vec![];
        let mut done = BTreeSet::new();
        let mut temporary = BTreeSet::new();
        for p in self.parts.values() {
            for f in &p.features {
                visit(
                    self,
                    &p.reference(&f.id),
                    &mut temporary,
                    &mut done,
                    &mut order,
                )?;
            }
        }
        Ok(order)
    }
    pub fn placed_parts(&self) -> Result<Vec<PlacedPart>> {
        crate::configuration::placed_parts(self, None)
    }

    pub fn empty(name: &str) -> Self {
        let a = Assembly {
            id: id(),
            name: name.into(),
            instances: BTreeMap::new(),
            mates: vec![],
            promoted_parameters: BTreeMap::new(),
            configurations: BTreeMap::new(),
            component_patterns: BTreeMap::new(),
            exploded_views: BTreeMap::new(),
            inspections: BTreeMap::new(),
            parameters: BTreeMap::new(),
        };
        let root = a.id.clone();
        let mut assemblies = BTreeMap::new();
        assemblies.insert(root.clone(), a);
        let materials = [
            ("aluminum", "Aluminum", 2700., [0.68, 0.72, 0.77], 1., 0.28),
            ("steel", "Steel", 7850., [0.42, 0.46, 0.52], 1., 0.22),
            ("rubber", "Rubber", 1100., [0.025, 0.028, 0.03], 0., 0.85),
            ("plastic", "Polymer", 1200., [0.17, 0.2, 0.25], 0., 0.4),
        ]
        .into_iter()
        .map(|(id, name, density, color, metallic, roughness)| {
            (
                id.into(),
                Material {
                    id: id.into(),
                    name: name.into(),
                    density_kg_m3: density,
                    color,
                    metallic,
                    roughness,
                },
            )
        })
        .collect();
        Self {
            schema_version: 1,
            id: id(),
            name: name.into(),
            revision: 0,
            root,
            parts: BTreeMap::new(),
            assemblies,
            materials,
            finishes: crate::appearance::default_finishes(),
            lighting: crate::appearance::Lighting::default(),
            settings: crate::settings::ModelSettings::default(),
            resources: BTreeMap::new(),
            drawings: vec![],
            comments: vec![],
            properties: BTreeMap::new(),
            libraries: BTreeMap::new(),
            active_configurations: BTreeMap::new(),
        }
    }
    /// Create a part with this model's finish preference, without inserting an occurrence.
    pub fn new_part(&self, name: &str) -> Part {
        let mut part = Part::new(name);
        if self
            .finishes
            .contains_key(&self.settings.default_new_part_finish)
        {
            part.finish = self.settings.default_new_part_finish.clone();
        }
        part
    }

    /// A new part document has only an empty definition at the origin.
    pub fn blank_part(name: &str) -> Self {
        let mut document = Self::empty(name);
        document
            .properties
            .insert("document_kind".into(), "part".into());
        let part = document.new_part(name);
        let instance = Instance {
            id: id(),
            name: name.into(),
            definition: part.id.clone(),
            placement: Placement::default(),
            finish: None,
            configuration: None,
            fixed: true,
            translation_expressions: None,
        };
        document.parts.insert(part.id.clone(), part);
        document
            .assemblies
            .get_mut(&document.root)
            .expect("new root")
            .instances
            .insert(instance.id.clone(), instance);
        document
    }
    pub fn sample() -> Self {
        let mut d = Self::empty("Drive unit");
        let mut p = Part::new("Bearing support");
        for (key, name, v) in [
            ("thickness", "Thickness", "8 mm"),
            ("bore", "Bearing bore diameter", "36 mm"),
            ("radius", "Fillet radius", "3 mm"),
        ] {
            p.parameters.insert(
                key.into(),
                Parameter {
                    name: name.into(),
                    expression: v.into(),
                    dimension: Dimension::Length,
                },
            );
        }
        let sketch = id();
        let extrude = id();
        let bore_profile = id();
        let bore = id();
        p.features = vec![
            Feature {
                id: sketch.clone(),
                name: "Support profile".into(),
                suppressed: false,
                operation: Operation::Profile {
                    profile: Profile::Polygon {
                        points: vec![[0., 0.], [88., 0.], [88., 64.], [0., 64.]],
                    },
                    plane_reference: None,
                    reference_body: None,
                    plane: Plane {
                        origin: [0.; 3],
                        u: [1., 0., 0.],
                        v: [0., 0., 1.],
                    },
                },
            },
            Feature {
                id: extrude.clone(),
                name: "Plate thickness".into(),
                suppressed: false,
                operation: Operation::Extrude {
                    profile: p.reference(&sketch),
                    distance: "thickness".into(),
                    direction: [0., 1., 0.],
                    body: None,
                    mode: "new".into(),
                    options: Default::default(),
                },
            },
            Feature {
                id: bore_profile.clone(),
                name: "Bore sketch".into(),
                suppressed: false,
                operation: Operation::Profile {
                    profile: Profile::Circle {
                        radius: "bore / 2".into(),
                        center: [44., 36.],
                    },
                    plane: Plane {
                        origin: [0., -1., 0.],
                        u: [1., 0., 0.],
                        v: [0., 0., 1.],
                    },
                    plane_reference: None,
                    reference_body: None,
                },
            },
            Feature {
                id: bore.clone(),
                name: "Bearing bore".into(),
                suppressed: false,
                operation: Operation::Extrude {
                    profile: p.reference(&bore_profile),
                    body: Some(p.reference(&extrude)),
                    distance: "thickness + 2 mm".into(),
                    direction: [0., 1., 0.],
                    mode: "cut".into(),
                    options: Default::default(),
                },
            },
        ];
        p.outputs = vec![bore];
        let support = p.id.clone();
        d.parts.insert(support.clone(), p);
        let mut shaft = Part::new("Shaft");
        shaft.material = "steel".into();
        shaft.parameters.insert(
            "length".into(),
            Parameter {
                name: "Length".into(),
                expression: "204 mm".into(),
                dimension: Dimension::Length,
            },
        );
        let shaft_node = id();
        let shaft_profile = id();
        shaft.features.push(Feature {
            id: shaft_profile.clone(),
            name: "Shaft sketch".into(),
            suppressed: false,
            operation: Operation::Profile {
                profile: Profile::Circle {
                    radius: "6 mm".into(),
                    center: [0.; 2],
                },
                plane: Plane {
                    origin: [0.; 3],
                    u: [0., 1., 0.],
                    v: [0., 0., 1.],
                },
                plane_reference: None,
                reference_body: None,
            },
        });
        shaft.features.push(Feature {
            id: shaft_node.clone(),
            name: "Turned shaft".into(),
            suppressed: false,
            operation: Operation::Extrude {
                profile: shaft.reference(&shaft_profile),
                distance: "length".into(),
                direction: [1., 0., 0.],
                body: None,
                mode: "new".into(),
                options: Default::default(),
            },
        });
        shaft.outputs.push(shaft_node);
        let shaft_id = shaft.id.clone();
        d.parts.insert(shaft_id.clone(), shaft);
        let a = d.assemblies.get_mut(&d.root).expect("new root");
        for (name, def, pos, rot) in [
            (
                "Support left",
                support.clone(),
                [-90., -44., 0.],
                [0., 0., 90.],
            ),
            ("Support right", support, [90., -44., 0.], [0., 0., 90.]),
            ("Shaft", shaft_id, [-102., 0., 36.], [0.; 3]),
        ] {
            let i = id();
            a.instances.insert(
                i.clone(),
                Instance {
                    id: i,
                    name: name.into(),
                    definition: def,
                    placement: Placement {
                        translation: pos,
                        rotation_deg: rot,
                    },
                    finish: None,
                    configuration: None,
                    fixed: true,
                    translation_expressions: None,
                },
            );
        }
        d
    }
}
