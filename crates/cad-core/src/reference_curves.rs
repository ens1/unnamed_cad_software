//! Reusable spatial reference-curve intent, independent of native carrier objects.
use crate::{
    CadError, Dimension, Document, Expr, Id, NodeRef, Result, TopologyRef,
    presentation::SemanticElement,
    reference_geometry::{AxisSource, DirectionSource, PointSource},
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
pub const CONTRACT_VERSION: u32 = 1;
pub const MAX_SOURCES: usize = 256;
pub const MAX_NATIVE_PIECES: usize = 4096;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CurveSource {
    SketchMember {
        profile: NodeRef,
        member: Id,
    },
    ModelEdge {
        body: NodeRef,
        reference: Box<TopologyRef>,
    },
    Curve {
        feature: NodeRef,
    },
}
impl CurveSource {
    pub fn dependency(&self) -> &NodeRef {
        match self {
            Self::SketchMember { profile, .. } => profile,
            Self::ModelEdge { body, .. } => body,
            Self::Curve { feature } => feature,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Segment {
    pub id: Id,
    pub source: CurveSource,
    #[serde(default)]
    pub reverse: bool,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PointRecord {
    pub id: Id,
    pub point: PointSource,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Interpolation {
    Linear,
    NaturalCubic,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum Definition {
    Composite {
        segments: Vec<Segment>,
        #[serde(default)]
        closed: bool,
        join_tolerance: Expr,
    },
    ThroughPoints {
        points: Vec<PointRecord>,
        interpolation: Interpolation,
        #[serde(default)]
        closed: bool,
    },
    Helix {
        axis: Box<AxisSource>,
        radial: Box<DirectionSource>,
        radius: Expr,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        end_radius: Option<Expr>,
        pitch: Expr,
        turns: Expr,
        start_angle: Expr,
        #[serde(default)]
        reverse: bool,
    },
}
impl Definition {
    pub fn name(&self) -> &'static str {
        self.label()
    }
    pub fn label(&self) -> &'static str {
        match self {
            Self::Composite { .. } => "Composite Curve",
            Self::ThroughPoints { .. } => "Curve Through Points",
            Self::Helix { .. } => "Helix / Spiral",
        }
    }
    pub fn dependencies(&self) -> Vec<NodeRef> {
        match self {
            Self::Composite { segments, .. } => segments
                .iter()
                .map(|s| s.source.dependency().clone())
                .collect(),
            Self::ThroughPoints { points, .. } => {
                points.iter().flat_map(|p| p.point.dependencies()).collect()
            }
            Self::Helix { axis, radial, .. } => axis
                .dependencies()
                .into_iter()
                .chain(radial.dependencies())
                .collect(),
        }
    }
    pub fn expressions(&self) -> Vec<&Expr> {
        match self {
            Self::Composite { join_tolerance, .. } => vec![join_tolerance],
            Self::ThroughPoints { points, .. } => {
                points.iter().flat_map(|p| p.point.expressions()).collect()
            }
            Self::Helix {
                radial,
                radius,
                end_radius,
                pitch,
                turns,
                start_angle,
                ..
            } => radial
                .expressions()
                .into_iter()
                .chain([radius, pitch, turns, start_angle])
                .chain(end_radius.iter())
                .collect(),
        }
    }
    pub fn source_ids(&self) -> Vec<&Id> {
        match self {
            Self::Composite { segments, .. } => segments.iter().map(|s| &s.id).collect(),
            Self::ThroughPoints { points, .. } => points.iter().map(|p| &p.id).collect(),
            Self::Helix { .. } => vec![],
        }
    }
    pub fn validate(&self) -> Result<()> {
        let ids = self.source_ids();
        if ids.iter().any(|id| {
            id.trim().is_empty()
                || matches!(
                    id.as_str(),
                    crate::sketch_actions::ORIGIN
                        | crate::sketch_actions::X_AXIS
                        | crate::sketch_actions::Y_AXIS
                )
        }) || ids.iter().copied().collect::<BTreeSet<_>>().len() != ids.len()
        {
            return Err(CadError::Invalid("Reference curve source identities must be nonempty, unique and distinct from reserved universe entities".into()));
        }
        match self {
            Self::Composite {segments,..} if segments.is_empty() || segments.len()>MAX_SOURCES =>
                Err(CadError::Invalid("Composite Curve needs 1–256 ordered sources".into())),
            Self::ThroughPoints {points,closed,..} if points.len()<(if *closed {3}else{2}) || points.len()>MAX_SOURCES =>
                Err(CadError::Invalid("Curve Through Points needs 2–256 points, or at least three for a closed curve".into())),
            Self::Composite {segments,..} if segments.iter().any(|s|matches!(&s.source,CurveSource::ModelEdge {reference,..} if reference.topology!="edge")) =>
                Err(CadError::Invalid("Composite model sources must retain an edge reference".into())),
            _ => Ok(()),
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Piece {
    pub id: Id,
    pub reference: TopologyRef,
    pub start: [f64; 3],
    pub end: [f64; 3],
    /// Conservative second-derivative norm for this piece's normalized native
    /// parameter in 0..1. Disposable display metadata, not geometric authority.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub second_derivative_bound_mm: Option<f64>,
}
/// Analytic authority for a constant-angle-rate cylindrical/conical helix or spiral.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HelixGeometry {
    pub origin: [f64; 3],
    pub axis: [f64; 3],
    pub radial: [f64; 3],
    pub radius: f64,
    pub end_radius: f64,
    pub height: f64,
    pub angle: f64,
    pub start_angle: f64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Evaluated {
    pub producer: NodeRef,
    pub kind: String,
    pub closed: bool,
    pub pieces: Vec<Piece>,
    pub analytic: Option<HelixGeometry>,
}

pub fn semantic_fields(
    doc: &Document,
    owner: &str,
    node: &str,
    definition: &Definition,
    state: &str,
) -> Vec<SemanticElement> {
    let mut out = Vec::new();
    let quantities: Vec<(&str, &Expr, Dimension)> = match definition {
        Definition::Composite { join_tolerance, .. } => {
            vec![("join_tolerance", join_tolerance, Dimension::Length)]
        }
        Definition::ThroughPoints { .. } => vec![],
        Definition::Helix {
            radius,
            end_radius,
            pitch,
            turns,
            start_angle,
            ..
        } => {
            let mut q = vec![
                ("radius", radius, Dimension::Length),
                ("pitch", pitch, Dimension::Length),
                ("turns", turns, Dimension::Scalar),
                ("start_angle", start_angle, Dimension::Angle),
            ];
            if let Some(radius) = end_radius {
                q.push(("end_radius", radius, Dimension::Length));
            }
            q
        }
    };
    out.push(SemanticElement {id:format!("{owner}::{node}::curve-intent"),role:"reference-curve".into(),
        label:definition.label().into(),value:None,state:state.into(),commands:vec!["feature.edit".into()],
        relationships:definition.dependencies().into_iter().map(|r|format!("{}::{}",r.definition,r.node)).collect(),
        data:serde_json::json!({"version":CONTRACT_VERSION,"target":node,"definition":definition,"units":"mm","parameterization":"Native curve parameter; not arc length"})});
    if let Definition::ThroughPoints { points, .. } = definition {
        for (index, record) in points.iter().enumerate() {
            out.push(SemanticElement {id:format!("{owner}::{node}::point::{}",record.id),role:"reference-curve-point".into(),
                label:format!("Curve Point {}",index+1),value:None,state:state.into(),commands:vec!["feature.edit".into()],
                relationships:record.point.dependencies().into_iter().map(|r|format!("{}::{}",r.definition,r.node)).collect(),
                data:serde_json::json!({"target":node,"pointer":format!("/definition/points/{index}/point"),"source_id":record.id,"point":record.point,"units":"mm"})});
            if let PointSource::Coordinates { coordinates } = &record.point {
                for (axis, expr) in coordinates.iter().enumerate() {
                    out.push(SemanticElement {id:format!("{owner}::{node}::point::{}::{axis}",record.id),role:"feature-field".into(),
                        label:format!("Point {} {}",index+1,["X","Y","Z"][axis]),value:Some(expr.0.clone()),state:state.into(),commands:vec!["feature.edit".into()],
                        relationships:vec![node.into(),record.id.clone()],data:serde_json::json!({"target":node,"pointer":format!("/definition/points/{index}/point/coordinates/{axis}"),
                            "dimension":"Length","evaluated":crate::expression::value(doc,owner,expr,Dimension::Length).ok(),"version":CONTRACT_VERSION})});
                }
            }
        }
    }
    for (field, expr, dimension) in quantities {
        out.push(SemanticElement {id:format!("{owner}::{node}::{field}"),role:"feature-field".into(),label:field.replace('_'," "),
            value:Some(expr.0.clone()),state:state.into(),commands:vec!["feature.edit".into()],relationships:vec![node.into()],
            data:serde_json::json!({"target":node,"pointer":format!("/definition/{field}"),"dimension":format!("{dimension:?}"),
                "evaluated":crate::expression::value(doc,owner,expr,dimension).ok(),"version":CONTRACT_VERSION})});
    }
    out
}
