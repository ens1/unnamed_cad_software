//! Native member ancestry names section walls independently of traversal order.
use crate::{
    adapter::TopologyHistory,
    extrusion::{ThinMember, ThinSection},
};
use cad_core::{CadError, NodeRef, Result, TopologyRef};
use monstertruck::modeling::*;
use std::collections::{HashMap, HashSet};
#[derive(Clone)]
pub(crate) struct Member {
    pub sources: Vec<TopologyRef>,
    pub purpose: serde_json::Value,
}
pub(crate) fn member(profile: &NodeRef, source: &ThinMember) -> Result<Member> {
    let sources=source.sources.iter().map(|member|Ok(TopologyRef {producer:profile.clone(),role:format!("construction:{}",serde_json::to_string(&serde_json::json!({"sources":[],"purpose":{"operation":"sketch_member","member":member},"topology":"curve"}))?),topology:"curve".into(),signature:None})).collect::<Result<Vec<_>>>()?;
    Ok(Member {
        sources,
        purpose: serde_json::json!({"side":source.side,"piece":source.piece,"cap":source.cap}),
    })
}
pub(crate) fn members(profile: &NodeRef, section: &ThinSection) -> Result<HashMap<EdgeId, Member>> {
    section
        .members
        .iter()
        .map(|(edge, source)| Ok((*edge, member(profile, source)?)))
        .collect()
}
pub(crate) struct History<'a> {
    owner: &'a NodeRef,
    operation: &'a str,
    generated: &'a mut TopologyHistory,
    faces: HashMap<FaceId, TopologyRef>,
}
impl<'a> History<'a> {
    pub(crate) fn new(
        owner: &'a NodeRef,
        operation: &'a str,
        generated: &'a mut TopologyHistory,
    ) -> Self {
        Self {
            owner,
            operation,
            generated,
            faces: HashMap::new(),
        }
    }
    pub(crate) fn face(
        &mut self,
        face: &Face,
        sources: &[TopologyRef],
        purpose: serde_json::Value,
    ) -> Result<()> {
        let reference = self.generated.tool_face(
            self.owner,
            face,
            sources,
            &serde_json::json!({"operation":self.operation,"constructor":purpose}),
        )?;
        self.faces.insert(face.id(), reference);
        Ok(())
    }
    pub(crate) fn finish(self, solid: &Solid) -> Result<()> {
        let mut edges: HashMap<EdgeId, (Edge, Vec<TopologyRef>)> = HashMap::new();
        for face in solid.face_iter() {
            let reference = self.faces.get(&face.id()).ok_or_else(|| {
                CadError::Unsupported("Section constructor lost a wall's native ancestry".into())
            })?;
            for edge in face.edge_iter() {
                edges
                    .entry(edge.id())
                    .or_insert_with(|| (edge.clone(), vec![]))
                    .1
                    .push(reference.clone());
            }
        }
        let mut roles = HashSet::new();
        for (edge, sources) in edges.values() {
            let reference = self.generated.tool_edge(
                self.owner,
                edge,
                sources,
                &serde_json::json!({"operation":self.operation,"constructor":"shared_boundary"}),
            )?;
            if !roles.insert(reference.role) {
                return Err(CadError::Unsupported(
                    "Repeated section boundaries need explicit segmented native ancestry".into(),
                ));
            }
        }
        Ok(())
    }
}
pub(crate) struct ToolContext {
    pub axis: Vector3,
    pub drafted: bool,
}
pub(crate) fn extrusion(
    owner: &NodeRef,
    profile: &NodeRef,
    section: &ThinSection,
    tool: &Solid,
    options: ToolContext,
    context: &serde_json::Value,
    generated: &mut TopologyHistory,
) -> Result<()> {
    let ToolContext { axis, drafted } = options;
    let origins = members(profile, section)?;
    let source = if section.face.oriented_surface().normal(0., 0.).dot(axis) < 0. {
        section.face.inverse()
    } else {
        section.face.clone()
    };
    let faces = tool.face_iter().collect::<Vec<_>>();
    let source_edges = source
        .boundaries()
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    if faces.len() != source_edges.len() + 2 {
        return Err(CadError::Unsupported(
            "Extrusion constructor changed its native member topology".into(),
        ));
    }
    let lower = faces[0].inverse();
    let bottom_edges = lower.boundaries().into_iter().flatten().collect::<Vec<_>>();
    let sources = origins
        .values()
        .flat_map(|origin| origin.sources.clone())
        .collect::<Vec<_>>();
    let mut history = History::new(owner, "extrusion", generated);
    history.face(
        faces[0],
        &sources,
        serde_json::json!({"end":"start","scope":context}),
    )?;
    history.face(
        faces.last().unwrap(),
        &sources,
        serde_json::json!({"end":"end","scope":context}),
    )?;
    for (index, (edge, wall)) in bottom_edges
        .iter()
        .zip(faces.iter().skip(1).take(source_edges.len()))
        .enumerate()
    {
        let source = if drafted {
            &source_edges[index]
        } else {
            let matching = source_edges
                .iter()
                .filter(|source| {
                    (edge.front().point() - source.front().point())
                        .cross(axis)
                        .magnitude()
                        <= 1e-7
                        && (edge.back().point() - source.back().point())
                            .cross(axis)
                            .magnitude()
                            <= 1e-7
                })
                .collect::<Vec<_>>();
            if matching.len() != 1 {
                return Err(CadError::Unsupported(
                    "Projected extrusion boundary has ambiguous native member ancestry".into(),
                ));
            }
            matching[0]
        };
        let origin = &origins[&source.id()];
        history.face(
            wall,
            &origin.sources,
            serde_json::json!({"wall":origin.purpose,"scope":context}),
        )?;
    }
    history.finish(tool)
}
