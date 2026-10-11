//! Sketch-driven rib intent, independent of evaluated boundary representations.
use crate::{Dimension, Document, Expr, Id, NodeRef, TopologyRef, presentation::SemanticElement};
use serde::{Deserialize, Serialize};

pub const CONTRACT_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    #[default]
    ParallelToSketch,
    NormalToSketch,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Placement {
    FirstSide,
    #[default]
    Centered,
    SecondSide,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Termination {
    #[default]
    UpToBody,
    UpToFace {
        face: Box<TopologyRef>,
        body: NodeRef,
    },
}
pub fn default_in_plane_direction() -> [Expr; 2] {
    ["0".into(), "-1".into()]
}
pub fn default_overlap() -> Expr {
    "0.1 mm".into()
}

/// Source-topology groups for contour selection. Geometry is still resolved and
/// validated by the current sketch solver and B-rep adapter before Apply.
pub fn open_contours(sketch: &crate::Sketch) -> crate::Result<Vec<Vec<Id>>> {
    use std::collections::{BTreeMap, BTreeSet};
    let aliases = crate::sketch_relations::topological_point_aliases(sketch, 1e-6)?;
    let suppressed = crate::sketch_transform::suppressed_members(sketch);
    let mut endpoints = BTreeMap::<Id, (Id, Id)>::new();
    for (id, member) in &sketch.members {
        if member.construction || suppressed.contains(id) {
            continue;
        }
        let curve = crate::sketch_edit::effective_curve(sketch, id)?;
        let (a, b) = match &curve {
            crate::SketchCurve::Line { start, end }
            | crate::SketchCurve::Arc { start, end, .. } => (start, end),
            crate::SketchCurve::Bezier { points } | crate::SketchCurve::Nurbs { points, .. } => {
                let (Some(a), Some(b)) = (points.first(), points.last()) else {
                    continue;
                };
                (a, b)
            }
            _ => continue,
        };
        endpoints.insert(
            id.clone(),
            (
                aliases.get(a).unwrap_or(a).clone(),
                aliases.get(b).unwrap_or(b).clone(),
            ),
        );
    }
    let mut pending = endpoints.keys().cloned().collect::<BTreeSet<_>>();
    let mut contours = Vec::new();
    while let Some(first) = pending.pop_first() {
        let mut group = vec![first.clone()];
        let mut points = BTreeSet::from([endpoints[&first].0.clone(), endpoints[&first].1.clone()]);
        loop {
            let next = pending
                .iter()
                .filter(|id| {
                    points.contains(&endpoints[*id].0) || points.contains(&endpoints[*id].1)
                })
                .cloned()
                .collect::<Vec<_>>();
            if next.is_empty() {
                break;
            }
            for id in next {
                pending.remove(&id);
                points.extend([endpoints[&id].0.clone(), endpoints[&id].1.clone()]);
                group.push(id);
            }
        }
        let mut degree = BTreeMap::<Id, usize>::new();
        for id in &group {
            let (a, b) = &endpoints[id];
            *degree.entry(a.clone()).or_default() += 1;
            *degree.entry(b.clone()).or_default() += 1;
        }
        if degree.values().all(|degree| *degree <= 2)
            && degree.values().filter(|degree| **degree == 1).count() == 2
        {
            group.sort();
            contours.push(group);
        }
    }
    Ok(contours)
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Definition {
    pub profile: NodeRef,
    /// Each anchor selects its current connected open native sketch chain.
    #[serde(default)]
    pub members: Vec<Id>,
    pub body: NodeRef,
    #[serde(default)]
    pub scope: Vec<NodeRef>,
    #[serde(default)]
    pub direction: Direction,
    pub thickness: Expr,
    #[serde(default)]
    pub placement: Placement,
    #[serde(default)]
    pub reverse: bool,
    #[serde(default)]
    pub termination: Termination,
    #[serde(default = "default_in_plane_direction")]
    pub in_plane_direction: [Expr; 2],
    /// Deliberately authored overlap must remain entirely inside the support.
    #[serde(default = "default_overlap")]
    pub attachment_overlap: Expr,
}
impl Definition {
    pub fn dependencies(&self) -> Vec<NodeRef> {
        let mut refs = vec![self.profile.clone(), self.body.clone()];
        refs.extend(self.scope.iter().cloned());
        if let Termination::UpToFace { face, body } = &self.termination {
            refs.extend([body.clone(), face.producer.clone()]);
        }
        refs.sort_by(|a, b| (&a.definition, &a.node).cmp(&(&b.definition, &b.node)));
        refs.dedup();
        refs
    }
    pub fn consumed_outputs(&self) -> Vec<NodeRef> {
        let mut refs = vec![self.body.clone()];
        refs.extend(self.scope.iter().cloned());
        refs.sort_by(|a, b| (&a.definition, &a.node).cmp(&(&b.definition, &b.node)));
        refs.dedup();
        refs
    }
    pub fn expressions(&self) -> Vec<&Expr> {
        let mut refs = vec![&self.thickness, &self.attachment_overlap];
        if self.direction == Direction::ParallelToSketch {
            refs.extend(&self.in_plane_direction);
        }
        refs
    }
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
        (
            "thickness",
            "Rib Thickness",
            &definition.thickness,
            Dimension::Length,
        ),
        (
            "attachment_overlap",
            "Attachment Overlap",
            &definition.attachment_overlap,
            Dimension::Length,
        ),
    ];
    if definition.direction == Direction::ParallelToSketch {
        quantities.extend([
            (
                "in_plane_direction.0",
                "Material Direction U",
                &definition.in_plane_direction[0],
                Dimension::Scalar,
            ),
            (
                "in_plane_direction.1",
                "Material Direction V",
                &definition.in_plane_direction[1],
                Dimension::Scalar,
            ),
        ]);
    }
    let mut elements=quantities.into_iter().map(|(field,label,expr,dimension)|SemanticElement {
        id:format!("{node}::rib::{field}"),role:"feature-field".into(),label:label.into(),value:Some(expr.0.clone()),state:state.into(),commands:vec!["feature.edit".into()],relationships:vec![node.into()],
        data:serde_json::json!({"contract":"tcad.rib","version":CONTRACT_VERSION,"target":target,"field":field,"type":"quantity","dimension":dimension,"expression":expr,"evaluated":crate::expression::value(doc,owner,expr,dimension).ok(),"editable":true}),
    }).collect::<Vec<_>>();
    elements.push(SemanticElement {id:format!("{node}::rib::options"),role:"feature-options".into(),label:"Rib".into(),value:None,state:state.into(),commands:vec!["feature.edit".into()],relationships:definition.dependencies().iter().map(|r|r.node.clone()).collect(),data:serde_json::json!({"contract":"tcad.rib","version":CONTRACT_VERSION,"target":target,"source":definition,"directions":["parallel_to_sketch","normal_to_sketch"],"thickness_placement":["first_side","centered","second_side"],"selected_contours":"native_connected_open_chain_anchors","termination":["up_to_body","up_to_face"],"attachment":"bounded_planar_support_with_proven_contained_authored_overlap","shared_draft":true,"requires_current_validation":true,"editable":true})});
    elements
}
