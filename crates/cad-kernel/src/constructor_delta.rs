//! Proven constructor-local material regions, disposable and never exported.
use cad_core::{CadError, Result};
use monstertruck::modeling::*;
use std::collections::{HashMap, HashSet};
/// Optional display construction may fail independently of validated modeling.
pub(crate) fn capture<T>(construct: impl FnOnce() -> Result<T>) -> Option<T> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(construct))
        .ok()
        .and_then(std::result::Result::ok)
}
#[derive(Clone, Default)]
pub(crate) struct Native {
    pub added: Vec<Solid>,
    pub removed: Vec<Solid>,
}
pub(crate) struct Record {
    before: HashSet<(FaceId, bool)>,
    after: HashSet<(FaceId, bool)>,
    pub change: Native,
}
fn keys(solids: &[Solid]) -> HashSet<(FaceId, bool)> {
    solids
        .iter()
        .flat_map(Solid::face_iter)
        .map(|f| (f.id(), f.orientation()))
        .collect()
}
impl Record {
    pub fn new(before: &[Solid], after: &[Solid], change: Native) -> Self {
        Self {
            before: keys(before),
            after: keys(after),
            change,
        }
    }
    pub fn matches(&self, before: &[Solid], after: &[Solid]) -> bool {
        self.before == keys(before) && self.after == keys(after)
    }
    pub fn storage(&self) -> usize {
        self.before.len()
            + self.after.len()
            + self
                .change
                .added
                .iter()
                .chain(&self.change.removed)
                .flat_map(Solid::face_iter)
                .count()
    }
}
fn unavailable(message: &str) -> CadError {
    CadError::Unsupported(format!("Constructor material preview: {message}"))
}
/// Sequential clipping has already proved disjoint interiors. Sew their
/// opposing planar contact patches so the preview has no interior interfaces.
pub(crate) fn removal(pieces: Vec<Solid>) -> Result<Native> {
    if pieces.len() > 64 || pieces.iter().flat_map(Solid::face_iter).take(4097).count() > 4096 {
        return Err(unavailable(
            "Planar removal exceeds its display construction budget",
        ));
    }
    let bounds = |solid: &Solid| {
        let mut lo = [f64::INFINITY; 3];
        let mut hi = [f64::NEG_INFINITY; 3];
        for vertex in solid.vertex_iter() {
            let p: [f64; 3] = vertex.point().into();
            for i in 0..3 {
                lo[i] = lo[i].min(p[i]);
                hi[i] = hi[i].max(p[i]);
            }
        }
        (lo, hi)
    };
    let mut removed: Vec<Solid> = Vec::new();
    let mut attempts = 0usize;
    for mut piece in pieces {
        let mut index = 0;
        while index < removed.len() {
            let (alo, ahi) = bounds(&removed[index]);
            let (blo, bhi) = bounds(&piece);
            if (0..3).any(|i| ahi[i] < blo[i] - 1e-7 || bhi[i] < alo[i] - 1e-7) {
                index += 1;
                continue;
            }
            attempts += 1;
            if attempts > 128 {
                return Err(unavailable(
                    "Planar removal exceeds its bounded contact work",
                ));
            }
            if let Some(joined) = crate::join::contact(&removed[index], &piece)? {
                piece = joined.solid;
                removed.remove(index);
                index = 0;
            } else {
                // Only a proven exterior is published. Unresolved touching
                // fragments use the existing bounded display comparison.
                return Err(unavailable(
                    "Planar removal contacts cannot be sewn exactly",
                ));
            }
        }
        removed.push(piece);
    }
    Ok(Native {
        added: vec![],
        removed,
    })
}
/// A complete native bevel quad and its four original planar boundaries close
/// one finite removal prism. This does not intersect or re-Boolean the stock.
pub(crate) fn straight_bevel(
    before: &Solid,
    after: &Solid,
    selected: &[EdgeId],
) -> Result<Option<Native>> {
    if selected.len() != 1 {
        return Ok(None);
    }
    let Some(edge) = before.edge_iter().find(|e| e.id() == selected[0]) else {
        return Ok(None);
    };
    if !crate::planar::straight_edge(&edge)? {
        return Ok(None);
    }
    let axis = (edge.back().point() - edge.front().point()).normalize();
    let walls = before
        .face_iter()
        .filter(|f| f.edge_iter().any(|e| e.id() == edge.id()))
        .map(crate::planar::boundary_plane)
        .collect::<Result<Vec<_>>>()?;
    let [Some(wall0), Some(wall1)] = walls.as_slice() else {
        return Ok(None);
    };
    let ends = [edge.front().clone(), edge.back().clone()];
    let mut terminals = Vec::new();
    for end in &ends {
        let faces = before
            .face_iter()
            .filter(|f| {
                f.vertex_iter().any(|v| v.id() == end.id())
                    && f.id() != wall0.face.id()
                    && f.id() != wall1.face.id()
            })
            .collect::<Vec<_>>();
        let [face] = faces.as_slice() else {
            return Ok(None);
        };
        let Some(plane) = crate::planar::boundary_plane(face)? else {
            return Ok(None);
        };
        if plane.normal.cross(axis).magnitude() > 1e-10 {
            return Ok(None);
        }
        terminals.push(plane);
    }
    let ids = before.face_iter().map(|f| f.id()).collect::<HashSet<_>>();
    let candidates = after
        .face_iter()
        .filter(|f| !ids.contains(&f.id()))
        .filter_map(|f| {
            let plane = crate::planar::boundary_plane(f).ok().flatten()?;
            let normal = plane.normal;
            // Positive wall coefficients prove the exterior convex material corner.
            let cross = wall0.normal.cross(wall1.normal);
            let denominator = cross.magnitude2();
            if denominator < 1e-12 {
                return None;
            }
            let c0 = normal.cross(wall1.normal).dot(cross) / denominator;
            let c1 = wall0.normal.cross(normal).dot(cross) / denominator;
            let positive = c0 > 1e-7 && c1 > 1e-7;
            let wires = f.boundaries();
            (positive
                && normal.dot(axis).abs() < 1e-10
                && plane.signed(edge.front().point()) > 1e-7
                && wires.len() == 1
                && wires[0].len() == 4
                && wires[0]
                    .iter()
                    .all(|e| crate::planar::straight_edge(e).unwrap_or(false)))
            .then_some(f.clone())
        })
        .collect::<Vec<_>>();
    let [bevel] = candidates.as_slice() else {
        return Ok(None);
    };
    let removed_bevel = bevel.inverse();
    let mut connectors = HashMap::<(VertexId, VertexId), Edge>::new();
    let connect =
        |a: &Vertex, b: &Vertex, cache: &mut HashMap<(VertexId, VertexId), Edge>| -> Edge {
            if let Some(edge) = cache.get(&(a.id(), b.id())) {
                return edge.clone();
            }
            if let Some(edge) = cache.get(&(b.id(), a.id())) {
                return edge.inverse();
            }
            let edge = builder::line(a, b);
            cache.insert((a.id(), b.id()), edge.clone());
            edge
        };
    let endpoint = |vertex: &Vertex| -> Result<usize> {
        let matching = terminals
            .iter()
            .enumerate()
            .filter(|(_, p)| p.signed(vertex.point()).abs() <= 1e-7)
            .map(|(i, _)| i)
            .collect::<Vec<_>>();
        match matching.as_slice() {
            [i] => Ok(*i),
            _ => Err(unavailable("Bevel rail has no unique original terminal")),
        }
    };
    let mut shell = Shell::from(vec![removed_bevel.clone()]);
    for rail in &removed_bevel.boundaries()[0] {
        let a = rail.front();
        let b = rail.back();
        let ia = endpoint(a)?;
        let ib = endpoint(b)?;
        let mut wire = Wire::new();
        wire.push_back(rail.inverse());
        wire.push_back(connect(a, &ends[ia], &mut connectors));
        let carrier = if ia != ib {
            wire.push_back(if edge.front().id() == ends[ia].id() {
                edge.clone()
            } else {
                edge.inverse()
            });
            let matching = [wall0, wall1]
                .into_iter()
                .filter(|p| p.signed(a.point()).abs() <= 1e-7 && p.signed(b.point()).abs() <= 1e-7)
                .collect::<Vec<_>>();
            let [wall] = matching.as_slice() else {
                return Err(unavailable("Bevel rail has no unique original support"));
            };
            wall.face.oriented_surface()
        } else {
            terminals[ia].face.oriented_surface()
        };
        wire.push_back(connect(&ends[ib], b, &mut connectors));
        shell.push(Face::try_new(vec![wire], carrier).map_err(|e| unavailable(&e.to_string()))?);
    }
    let solid = Solid::try_new(vec![shell]).map_err(|e| unavailable(&e.to_string()))?;
    crate::validation::solid(&solid)?;
    Ok(Some(Native {
        added: vec![],
        removed: vec![solid],
    }))
}
