//! Versioned curve-driven rigid body-pattern intent without kernel objects.
use crate::{
    CadError, Dimension, Document, Expr, Id, NodeRef, Result, presentation::SemanticElement,
    reference_geometry::PointSource,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const CONTRACT_VERSION: u32 = 1;
pub const MAX_INSTANCES: usize = 10000;
fn version() -> u32 {
    CONTRACT_VERSION
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Path {
    Sketch {
        profile: NodeRef,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        member: Option<Id>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        start_vertex: Option<Id>,
    },
    ReferenceCurve {
        feature: NodeRef,
    },
}
impl Path {
    pub fn dependency(&self) -> &NodeRef {
        match self {
            Self::Sketch { profile, .. } => profile,
            Self::ReferenceCurve { feature } => feature,
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Spacing {
    #[default]
    EqualParameter,
    ExactArcLength,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Orientation {
    #[default]
    Fixed,
    FollowTangent,
}
pub fn default_range() -> [Expr; 2] {
    ["0".into(), "1".into()]
}
pub fn default_normal() -> [Expr; 3] {
    ["0".into(), "0".into(), "1".into()]
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Definition {
    #[serde(default = "version")]
    pub version: u32,
    pub body: NodeRef,
    pub path: Path,
    pub count: Expr,
    #[serde(default)]
    pub spacing: Spacing,
    #[serde(default = "default_range")]
    pub range: [Expr; 2],
    #[serde(default)]
    pub reverse: bool,
    #[serde(default)]
    pub orientation: Orientation,
    /// Explicit document-space frame-up direction, not a fitted path normal.
    #[serde(default = "default_normal")]
    pub normal: [Expr; 3],
    #[serde(default)]
    pub seed: Box<PointSource>,
    /// Move the seed point onto the first path station. Otherwise preserve its
    /// initial offset and rotate the source around the first station.
    #[serde(default)]
    pub align_seed: bool,
    #[serde(default)]
    pub keep_source: bool,
    /// Zero-based stable station keys. Count/range edits never renumber survivors.
    #[serde(default)]
    pub skipped: Vec<usize>,
}
#[derive(Clone, Copy, Debug)]
pub struct Resolved {
    pub count: usize,
    pub range: [f64; 2],
    pub normal: [f64; 3],
}
impl Definition {
    pub fn new(body: NodeRef, path: Path) -> Self {
        Self {
            version: CONTRACT_VERSION,
            body,
            path,
            count: "5".into(),
            spacing: Spacing::EqualParameter,
            range: default_range(),
            reverse: false,
            orientation: Orientation::Fixed,
            normal: default_normal(),
            seed: Box::default(),
            align_seed: false,
            keep_source: false,
            skipped: vec![],
        }
    }
    pub fn dependencies(&self) -> Vec<NodeRef> {
        let mut refs = vec![self.body.clone(), self.path.dependency().clone()];
        if self.align_seed {
            refs.extend(self.seed.dependencies());
        }
        refs.sort_by(|a, b| (&a.definition, &a.node).cmp(&(&b.definition, &b.node)));
        refs.dedup();
        refs
    }
    pub fn consumed_outputs(&self) -> Vec<NodeRef> {
        vec![self.body.clone()]
    }
    pub fn expressions(&self) -> Vec<&Expr> {
        let mut out = vec![&self.count, &self.range[0], &self.range[1]];
        if self.orientation == Orientation::FollowTangent {
            out.extend(&self.normal);
        }
        if self.align_seed {
            out.extend(self.seed.expressions());
        }
        out
    }
    pub fn resolve(&self, doc: &Document, owner: &str) -> Result<Resolved> {
        let invalid = |message: &str| CadError::Invalid(format!("Curve Pattern: {message}"));
        if self.version != CONTRACT_VERSION {
            return Err(invalid("Unsupported native contract version"));
        }
        let quantity =
            |expression: &Expr| crate::expression::value(doc, owner, expression, Dimension::Scalar);
        let count = quantity(&self.count)?;
        if !count.is_finite() || count.fract() != 0. || count < 1. || count > MAX_INSTANCES as f64 {
            return Err(invalid("Instances must be an integer from 1 to 10000"));
        }
        let count = count as usize;
        let range = [quantity(&self.range[0])?, quantity(&self.range[1])?];
        if range
            .iter()
            .any(|x| !x.is_finite() || !(0. ..=1.).contains(x))
            || range[1] - range[0] <= 1e-12
        {
            return Err(invalid("Path Range must satisfy 0 ≤ Start < End ≤ 1"));
        }
        let mut normal = [0., 0., 1.];
        if self.orientation == Orientation::FollowTangent {
            for (i, value) in normal.iter_mut().enumerate() {
                *value = quantity(&self.normal[i])?;
            }
            let magnitude = normal.iter().map(|x| x * x).sum::<f64>().sqrt();
            if !magnitude.is_finite() || magnitude <= 1e-12 {
                return Err(invalid("Frame Up must be a finite nonzero direction"));
            }
            normal.iter_mut().for_each(|x| *x /= magnitude);
        }
        let mut skipped = BTreeSet::new();
        if self
            .skipped
            .iter()
            .any(|index| *index >= MAX_INSTANCES || !skipped.insert(*index))
        {
            return Err(invalid(
                "Skipped station keys must be unique and within the 10000-slot reservation range",
            ));
        }
        if skipped.iter().filter(|index| **index < count).count() == count && !self.keep_source {
            return Err(invalid("Keep at least one station or enable Keep Source"));
        }
        Ok(Resolved {
            count,
            range,
            normal,
        })
    }
}
/// Native source endpoints for a sketch path selector. The evaluated adapter
/// validates actual connectivity, construction status and current solved geometry.
pub fn sketch_endpoints(sketch: &crate::Sketch, selected: Option<&str>) -> Result<Vec<Id>> {
    let aliases = crate::sketch_relations::topological_point_aliases(sketch, 1e-6)?;
    let suppressed = crate::sketch_transform::suppressed_members(sketch);
    let mut degree = BTreeMap::<Id, (usize, Id)>::new();
    for (id, member) in &sketch.members {
        if member.construction
            || suppressed.contains(id)
            || selected.is_some_and(|selected| selected != id)
        {
            continue;
        }
        let curve = crate::sketch_edit::effective_curve(sketch, id)?;
        let endpoints = match &curve {
            crate::SketchCurve::Line { start, end }
            | crate::SketchCurve::Arc { start, end, .. } => Some((start, end)),
            crate::SketchCurve::Bezier { points } | crate::SketchCurve::Nurbs { points, .. } => {
                points.first().zip(points.last())
            }
            _ => None,
        };
        if let Some((a, b)) = endpoints {
            for id in [a, b] {
                let key = aliases.get(id).unwrap_or(id).clone();
                let entry = degree.entry(key).or_insert_with(|| (0, id.clone()));
                entry.0 += 1;
            }
        }
    }
    Ok(degree
        .into_values()
        .filter_map(|(degree, id)| (degree == 1).then_some(id))
        .collect())
}
pub fn slot_key(owner: &NodeRef, index: usize) -> String {
    format!(
        "{}::{}::curve-pattern::{index}",
        owner.definition, owner.node
    )
}
pub fn semantic_fields(
    doc: &Document,
    owner: &str,
    node: &str,
    definition: &Definition,
    state: &str,
) -> Vec<SemanticElement> {
    let target = serde_json::json!({"kind":"feature","definition":owner,"feature":node});
    let mut quantities = vec![
        ("count", "Instances", &definition.count),
        ("range.0", "Path Range Start", &definition.range[0]),
        ("range.1", "Path Range End", &definition.range[1]),
    ];
    if definition.orientation == Orientation::FollowTangent {
        quantities.extend([
            ("normal.0", "Frame Up X", &definition.normal[0]),
            ("normal.1", "Frame Up Y", &definition.normal[1]),
            ("normal.2", "Frame Up Z", &definition.normal[2]),
        ]);
    }
    let mut out=quantities.into_iter().map(|(field,label,expr)|SemanticElement {id:format!("{node}::curve-pattern::{field}"),role:"feature-field".into(),label:label.into(),value:Some(expr.0.clone()),state:state.into(),commands:vec!["feature.edit".into()],relationships:vec![node.into()],data:serde_json::json!({"contract":"tcad.curve-pattern","version":CONTRACT_VERSION,"target":target,"field":field,"dimension":Dimension::Scalar,"expression":expr,"evaluated":crate::expression::value(doc,owner,expr,Dimension::Scalar).ok(),"editable":true})}).collect::<Vec<_>>();
    out.push(SemanticElement {id:format!("{node}::curve-pattern::options"),role:"feature-options".into(),label:"Curve Pattern".into(),value:None,state:state.into(),commands:vec!["feature.edit".into()],relationships:definition.dependencies().iter().map(|r|format!("{}::{}",r.definition,r.node)).collect(),data:serde_json::json!({"contract":"tcad.curve-pattern","version":CONTRACT_VERSION,"target":target,"source":definition,"spacing":["equal_parameter","exact_arc_length"],"range_measure":match definition.spacing {Spacing::EqualParameter=>"normalized_concatenated_native_parameter_intervals",Spacing::ExactArcLength=>"normalized_exact_path_distance"},"orientation":["fixed","follow_tangent"],"frame":"authored_up_vector_projected_perpendicular_to_native_tangent","closed_full_range":"exclude_duplicate_closing_station","instance_keys":"stable_feature_owner_plus_zero_based_slot","body_copies":"separate_validated_rigid_breps","seed_alignment":"align_seed_point_or_preserve_initial_offset","shared_draft":true,"requires_current_validation":true,"editable":true})});
    out
}
