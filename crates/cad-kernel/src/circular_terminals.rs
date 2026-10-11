//! Exact planar terminations for native circular treatment chains.
//!
//! Each open end must already be the trihedral intersection of the cap, the
//! turned wall and a radial plane. The meridian is shared by the new blend and
//! this existing plane. No artificial end cap or fitted transition is added.
use super::{EPS, Rim, invalid, oriented, preserve_surface, same_ring, unsupported};
use crate::shell_history::ShellHistory;
use cad_core::Result;
use monstertruck::modeling::*;
use std::collections::{HashMap, HashSet};

struct Terminal {
    vertex: VertexId,
    rim: usize,
    face: Face,
    radial: EdgeId,
    seam: EdgeId,
}

#[derive(Default)]
pub(super) struct Terminations {
    ends: Vec<Terminal>,
}

fn absolute(edge: &Edge) -> Edge {
    if edge.orientation() {
        edge.clone()
    } else {
        edge.inverse()
    }
}

fn point_on_trim(edge: &Edge, old: VertexId, replacement: &Vertex) -> Result<()> {
    if !crate::planar::straight_edge(edge)? {
        return Err(unsupported(
            "A partial circular rim must end on straight radial and generatrix edges",
        ));
    }
    let (start, end) = if edge.front().id() == old {
        (edge.front().point(), edge.back().point())
    } else {
        (edge.back().point(), edge.front().point())
    };
    let delta = end - start;
    let length = delta.magnitude();
    let offset = replacement.point() - start;
    let t = offset.dot(delta) / (length * length);
    if !length.is_finite()
        || length <= EPS
        || !t.is_finite()
        || t <= EPS / length
        || t >= 1. - EPS / length
        || (offset - delta * t).magnitude() > EPS
    {
        return Err(invalid(
            "The partial circular setback consumes or leaves a terminal edge; reduce its size",
        ));
    }
    Ok(())
}

impl Terminations {
    pub(super) fn discover(
        solid: &Solid,
        rims: &[Rim],
        plane_vertices: &HashMap<VertexId, Vertex>,
        wall_vertices: &HashMap<VertexId, Vertex>,
    ) -> Result<Self> {
        let mut selected: HashMap<VertexId, Vec<usize>> = HashMap::new();
        for (index, rim) in rims.iter().enumerate() {
            for vertex in [rim.edge.front(), rim.edge.back()] {
                selected.entry(vertex.id()).or_default().push(index);
            }
        }
        let mut adjacency: HashMap<EdgeId, HashSet<FaceId>> = HashMap::new();
        let mut incident: HashMap<VertexId, HashSet<EdgeId>> = HashMap::new();
        let mut edges = HashMap::new();
        let mut faces = HashMap::new();
        for face in solid.face_iter() {
            faces.insert(face.id(), face);
            for edge in face.edge_iter() {
                adjacency.entry(edge.id()).or_default().insert(face.id());
                for vertex in [edge.front(), edge.back()] {
                    incident.entry(vertex.id()).or_default().insert(edge.id());
                }
                edges.entry(edge.id()).or_insert_with(|| absolute(&edge));
            }
        }
        let mut ends = Vec::new();
        // The caller orders rims by their construction ancestry. Iterating this
        // order rather than the native vertex map makes diagnostics stable too.
        let mut visited = HashSet::new();
        for rim in rims {
            for vertex in [rim.edge.absolute_front(), rim.edge.absolute_back()] {
                if !visited.insert(vertex.id()) {
                    continue;
                }
                let indices = &selected[&vertex.id()];
                if let [a, b] = indices.as_slice() {
                    if !same_ring(&rims[*a], &rims[*b]) {
                        return Err(unsupported(
                            "Connected circular edges require one common cap and circle at their shared vertex",
                        ));
                    }
                    continue;
                }
                let [index] = indices.as_slice() else {
                    return Err(unsupported(
                        "A circular treatment chain branches at a vertex",
                    ));
                };
                let rim = &rims[*index];
                let neighboring = &incident[&vertex.id()];
                if neighboring.len() != 3 {
                    return Err(unsupported(
                        "Select the complete circular chain, or end it on a three-face radial-plane corner",
                    ));
                }
                let face_ids = neighboring
                    .iter()
                    .flat_map(|edge| adjacency[edge].iter().copied())
                    .collect::<HashSet<_>>();
                let terminal = face_ids
                    .iter()
                    .filter(|id| **id != rim.cap.id() && **id != rim.wall)
                    .collect::<Vec<_>>();
                let [terminal] = terminal.as_slice() else {
                    return Err(unsupported(
                        "A partial circular treatment has no unique terminal face",
                    ));
                };
                if face_ids.len() != 3 {
                    return Err(unsupported(
                        "A partial circular treatment requires a three-face terminal corner",
                    ));
                }
                let face = faces[terminal];
                let Some(plane) = crate::planar::boundary_plane(face)? else {
                    return Err(unsupported(
                        "A partial circular treatment currently requires a planar terminal face",
                    ));
                };
                if plane.normal.dot(rim.axis).abs() > 1e-10 || plane.signed(rim.center).abs() > EPS
                {
                    return Err(unsupported(
                        "A partial circular treatment must terminate on a plane through its axis",
                    ));
                }
                let neighbor_on = |support: FaceId| -> Result<EdgeId> {
                    let candidates = neighboring
                        .iter()
                        .filter(|id| {
                            **id != rim.edge.id()
                                && adjacency[id].len() == 2
                                && adjacency[id].contains(&support)
                                && adjacency[id].contains(terminal)
                        })
                        .copied()
                        .collect::<Vec<_>>();
                    let [edge] = candidates.as_slice() else {
                        return Err(unsupported(
                            "The terminal corner has ambiguous cap or turned-wall incidence",
                        ));
                    };
                    Ok(*edge)
                };
                let radial = neighbor_on(rim.cap.id())?;
                let seam = neighbor_on(rim.wall)?;
                if radial == seam {
                    return Err(unsupported("A circular terminal corner is collapsed"));
                }
                point_on_trim(&edges[&radial], vertex.id(), &plane_vertices[&vertex.id()])?;
                point_on_trim(&edges[&seam], vertex.id(), &wall_vertices[&vertex.id()])?;
                ends.push(Terminal {
                    vertex: vertex.id(),
                    rim: *index,
                    face: face.clone(),
                    radial,
                    seam,
                });
            }
        }
        Ok(Self { ends })
    }

    pub(super) fn cap_edges(&self, rim: &Rim, rims: &[Rim]) -> Vec<EdgeId> {
        self.ends
            .iter()
            .filter(|end| same_ring(rim, &rims[end.rim]))
            .map(|end| end.radial)
            .collect()
    }

    /// Radial and axial source edges meet the intended removed band. Their
    /// exact straight remainders are certified separately; all other terminal
    /// boundaries must be disjoint from that band just like cap boundaries.
    pub(super) fn protected_edges(
        &self,
        face: &Face,
        rim: &Rim,
        rims: &[Rim],
    ) -> Option<Vec<Edge>> {
        let local = self
            .ends
            .iter()
            .filter(|end| end.face.id() == face.id() && same_ring(rim, &rims[end.rim]))
            .flat_map(|end| [end.radial, end.seam])
            .collect::<HashSet<_>>();
        (!local.is_empty()).then(|| {
            face.edge_iter()
                .filter(|edge| !local.contains(&edge.id()))
                .collect()
        })
    }

    pub(super) fn radial_edges(
        &self,
        source_edges: &HashMap<EdgeId, Edge>,
        vertices: &HashMap<VertexId, Vertex>,
        history: &mut ShellHistory,
    ) -> Result<HashMap<EdgeId, Edge>> {
        let mut result = HashMap::new();
        for end in &self.ends {
            if result.contains_key(&end.radial) {
                continue;
            }
            let source = &source_edges[&end.radial];
            let a = vertices
                .get(&source.absolute_front().id())
                .unwrap_or(source.absolute_front());
            let b = vertices
                .get(&source.absolute_back().id())
                .unwrap_or(source.absolute_back());
            let delta = source.absolute_back().point() - source.absolute_front().point();
            if (b.point() - a.point()).dot(delta) <= EPS * delta.magnitude() {
                return Err(invalid(
                    "Circular setbacks meet or consume a shared radial terminal edge",
                ));
            }
            let replacement = builder::line(a, b);
            history.retain_edge(replacement.id(), source.id())?;
            result.insert(source.id(), replacement);
        }
        Ok(result)
    }

    pub(super) fn rebuild(
        &self,
        radial_edges: &HashMap<EdgeId, Edge>,
        seam_edges: &HashMap<EdgeId, Edge>,
        connectors: &HashMap<VertexId, Edge>,
        history: &mut ShellHistory,
    ) -> Result<HashMap<FaceId, Face>> {
        let mut result = HashMap::new();
        for terminal in &self.ends {
            let face = &terminal.face;
            if result.contains_key(&face.id()) {
                continue;
            }
            let plane = crate::planar::boundary_plane(face)?
                .ok_or_else(|| invalid("A circular terminal lost its supporting plane"))?;
            let mut wires = Vec::new();
            for wire in face.boundaries() {
                let segments = wire
                    .iter()
                    .map(|edge| {
                        radial_edges
                            .get(&edge.id())
                            .or_else(|| seam_edges.get(&edge.id()))
                            .map_or_else(|| edge.clone(), |new| oriented(edge, new))
                    })
                    .collect::<Vec<_>>();
                let mut output = Wire::new();
                for (index, current) in segments.iter().enumerate() {
                    let next = &segments[(index + 1) % segments.len()];
                    output.push_back(current.clone());
                    if current.back().id() == next.front().id() {
                        continue;
                    }
                    let vertex = wire[index].back().id();
                    if !self
                        .ends
                        .iter()
                        .any(|end| end.face.id() == face.id() && end.vertex == vertex)
                    {
                        return Err(invalid("A radial terminal has an unrelated open corner"));
                    }
                    let connector = connectors
                        .get(&vertex)
                        .ok_or_else(|| invalid("A radial terminal has no constructed meridian"))?;
                    let connector = if connector.front().id() == current.back().id()
                        && connector.back().id() == next.front().id()
                    {
                        connector.clone()
                    } else if connector.back().id() == current.back().id()
                        && connector.front().id() == next.front().id()
                    {
                        connector.inverse()
                    } else {
                        return Err(invalid(
                            "A radial terminal meridian cannot join its exact trimmed edges",
                        ));
                    };
                    let bounds = crate::native_bounds::curve(&connector.curve())?;
                    let (low, high) = bounds.projection(plane.origin, plane.normal)?;
                    if low < -EPS || high > EPS {
                        return Err(unsupported(
                            "The constructed circular meridian leaves its terminal plane",
                        ));
                    }
                    output.push_back(connector);
                }
                wires.push(output);
            }
            let replacement = preserve_surface(face, wires)
                .map_err(|_| invalid("The circular terminal face has inconsistent boundaries"))?;
            history.retain_face(replacement.id(), face.id())?;
            result.insert(face.id(), replacement);
        }
        Ok(result)
    }
}
