//! Display tessellation only. Picking retains source faces and drawing export uses native sections.
use super::Frame;
use crate::viewport;
use bevy::{asset::RenderAssetUsages, math::DVec3, mesh::PrimitiveTopology, prelude::Mesh};
use cad_core::{
    PlacedPart,
    protocol::{BodyKind, MeshData},
};
use spade::{ConstrainedDelaunayTriangulation, Point2, Triangulation};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

pub(super) fn distance(frame: Frame, p: [f64; 3]) -> f64 {
    let offset = DVec3::from(p) - DVec3::from(frame.origin);
    let d = offset.dot(DVec3::from(frame.normal));
    if d.abs() < 1e-10 { 0. } else { d }
}

/// Classify a placed mesh before visiting individual facets or edges.
pub(crate) enum Coverage {
    Kept,
    Removed,
    Cut(Frame),
}
pub(crate) fn coverage(cut: Option<Frame>, source: &MeshData, i: &PlacedPart) -> Coverage {
    let Some(f) = cut else { return Coverage::Kept };
    let mut low = f64::INFINITY;
    let mut high = f64::NEG_INFINITY;
    for mask in 0..8usize {
        let p = std::array::from_fn(|j| source.bounds[(mask >> j) & 1][j]);
        let d = distance(f, viewport::placed(p, i));
        low = low.min(d);
        high = high.max(d);
    }
    if low > 0. {
        Coverage::Removed
    } else if high < 0. {
        Coverage::Kept
    } else {
        Coverage::Cut(f)
    }
}
pub(crate) fn segment(frame: Option<Frame>, a: [f64; 3], b: [f64; 3]) -> Option<[[f64; 3]; 2]> {
    let Some(frame) = frame else {
        return Some([a, b]);
    };
    let (da, db) = (distance(frame, a), distance(frame, b));
    match (da <= 0., db <= 0.) {
        (true, true) => Some([a, b]),
        (false, false) => None,
        _ => {
            let at = DVec3::from(a).lerp(b.into(), da / (da - db)).to_array();
            Some(if da <= 0. { [a, at] } else { [at, b] })
        }
    }
}

#[derive(Clone, Copy, Default)]
struct Vertex {
    p: DVec3,
    n: DVec3,
    d: f64,
}
#[derive(Default)]
struct Output {
    p: Vec<[f32; 3]>,
    n: Vec<[f32; 3]>,
}
impl Output {
    fn push(&mut self, p: DVec3, n: DVec3) {
        self.p.push(viewport::render_point(p.to_array()).to_array());
        self.n.push(
            viewport::render_point(n.to_array())
                .normalize_or_zero()
                .to_array(),
        );
    }
    fn mesh(self) -> Mesh {
        Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.p)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, self.n)
    }
}

fn intersection(a: Vertex, b: Vertex) -> Vertex {
    let t = a.d / (a.d - b.d);
    Vertex {
        p: a.p.lerp(b.p, t),
        n: a.n.lerp(b.n, t),
        d: 0.,
    }
}
pub(crate) fn clipped(
    source: &MeshData,
    range: std::ops::Range<usize>,
    instance: &PlacedPart,
    frame: Frame,
) -> Mesh {
    let nm = viewport::normal_matrix(instance);
    let vertices: Vec<_> = source
        .positions
        .iter()
        .zip(&source.normals)
        .map(|(p, n)| {
            let p = viewport::placed(*p, instance);
            Vertex {
                p: p.into(),
                n: nm * DVec3::from(*n),
                d: distance(frame, p),
            }
        })
        .collect();
    let has_sheets = source
        .faces
        .iter()
        .any(|f| f.body_kind == BodyKind::Surface);
    // A sheet exactly on the cutting plane survives. Solid caps own this plane for closed bodies.
    let first = range.start;
    let indices = &source.indices[range];
    let mut sheets = source
        .faces
        .iter()
        .filter(|f| f.body_kind == BodyKind::Surface)
        .peekable();
    let mut out = Output::default();
    for (index, tri) in indices.as_chunks::<3>().0.iter().enumerate() {
        let tri = tri.map(|i| vertices[i as usize]);
        if tri.iter().all(|v| v.d >= 0.) {
            if has_sheets && tri.iter().all(|v| v.d == 0.) {
                let offset = first + 3 * index;
                while sheets.peek().is_some_and(|f| f.start + f.count <= offset) {
                    sheets.next();
                }
                if sheets.peek().is_some_and(|f| f.start <= offset) {
                    for v in tri {
                        out.push(v.p, v.n);
                    }
                }
            }
            continue;
        }
        let mut polygon = [Vertex::default(); 4];
        let mut count = 0;
        for i in 0..3 {
            let a = tri[i];
            let b = tri[(i + 1) % 3];
            if a.d <= 0. {
                polygon[count] = a;
                count += 1;
            }
            if a.d * b.d < 0. {
                polygon[count] = intersection(a, b);
                count += 1;
            }
        }
        for i in 2..count {
            for v in [polygon[0], polygon[i - 1], polygon[i]] {
                out.push(v.p, v.n);
            }
        }
    }
    out.mesh()
}

/// The exact perimeters used to draw a display cap, including its holes.
#[derive(Default)]
pub(crate) struct CapOutline(Vec<[[f64; 2]; 2]>);
impl CapOutline {
    pub(super) fn contains(&self, [x, y]: [f64; 2]) -> bool {
        self.0.iter().fold(false, |inside, &[[ax, ay], [bx, by]]| {
            inside ^ ((ay > y) != (by > y) && x < ax + (y - ay) * (bx - ax) / (by - ay))
        })
    }
}
#[derive(Default)]
struct Boundary {
    points: Vec<Point2<f64>>,
    keys: BTreeMap<[i64; 2], usize>,
    edges: BTreeSet<[usize; 2]>,
}
impl Boundary {
    fn index(&mut self, p: DVec3, frame: Frame, grid: f64) -> usize {
        let dp = p - DVec3::from(frame.origin);
        let uv = [dp.dot(frame.u.into()), dp.dot(frame.v.into())];
        let key = uv.map(|v| (v / grid).round() as i64);
        *self.keys.entry(key).or_insert_with(|| {
            let i = self.points.len();
            self.points.push(Point2::new(uv[0], uv[1]));
            i
        })
    }
    fn add(&mut self, tri: [DVec3; 3], d: [f64; 3], frame: Frame, grid: f64) {
        let mut ends = [0; 2];
        let mut count = 0;
        for i in 0..3 {
            let j = (i + 1) % 3;
            if (d[i] < 0.) != (d[j] < 0.) {
                ends[count] = self.index(tri[i].lerp(tri[j], d[i] / (d[i] - d[j])), frame, grid);
                count += 1;
            }
        }
        if count == 2 && ends[0] != ends[1] {
            ends.sort_unstable();
            // Two facets touching the plane on the same side cancel at their common edge.
            if !self.edges.insert(ends) {
                self.edges.remove(&ends);
            }
        }
    }
    fn fill(self, out: &mut Output, frame: Frame, outlines: &mut Vec<CapOutline>) {
        if self.edges.len() < 3 {
            return;
        }
        let mut degree = vec![0usize; self.points.len()];
        for &[a, b] in &self.edges {
            degree[a] += 1;
            degree[b] += 1;
        }
        if degree.iter().any(|d| d % 2 != 0) {
            return;
        }
        let mut conflict = false;
        let Ok(cdt) = ConstrainedDelaunayTriangulation::<Point2<f64>>::try_bulk_load_cdt(
            self.points,
            self.edges.into_iter().collect(),
            |_| conflict = true,
        ) else {
            return;
        };
        if conflict {
            return;
        }
        // Classify triangles across every closed perimeter, including internal holes.
        let mut neighbors = vec![Vec::new(); cdt.num_all_faces()];
        for edge in cdt.undirected_edges() {
            let e = edge.as_directed();
            let (a, b) = (e.face().index(), e.rev().face().index());
            let crosses = edge.is_constraint_edge();
            neighbors[a].push((b, crosses));
            neighbors[b].push((a, crosses));
        }
        let outside = cdt.outer_face().index();
        let mut inside = vec![None; neighbors.len()];
        inside[outside] = Some(false);
        let mut queue = VecDeque::from([outside]);
        while let Some(face) = queue.pop_front() {
            for &(next, crosses) in &neighbors[face] {
                let side = inside[face].unwrap() ^ crosses;
                if let Some(prior) = inside[next] {
                    if side != prior {
                        return;
                    }
                } else {
                    inside[next] = Some(side);
                    queue.push_back(next);
                }
            }
        }
        let edges = cdt
            .undirected_edges()
            .filter(|e| e.is_constraint_edge())
            .map(|e| e.positions().map(|p| [p.x, p.y]))
            .collect();
        outlines.push(CapOutline(edges));
        for triangle in cdt
            .inner_faces()
            .filter(|f| inside[f.index()] == Some(true))
        {
            for p in triangle.positions() {
                out.push(
                    DVec3::from(frame.origin)
                        + DVec3::from(frame.u) * p.x
                        + DVec3::from(frame.v) * p.y,
                    frame.normal.into(),
                );
            }
        }
    }
}
pub(crate) fn cap(
    source: &MeshData,
    instance: &PlacedPart,
    frame: Frame,
) -> Option<(Mesh, Vec<CapOutline>)> {
    let positions: Vec<_> = source
        .positions
        .iter()
        .map(|p| DVec3::from(viewport::placed(*p, instance)))
        .collect();
    let d: Vec<_> = positions
        .iter()
        .map(|p| distance(frame, p.to_array()))
        .collect();
    if d.iter().all(|d| *d < 0.) || d.iter().all(|d| *d >= 0.) {
        return None;
    }
    let span = (DVec3::from(source.bounds[1]) - DVec3::from(source.bounds[0])).length();
    let grid = (span * 1e-10).max(1e-10);
    let mut boundaries = BTreeMap::new();
    for face in source
        .faces
        .iter()
        .filter(|f| f.body_kind == BodyKind::Solid)
    {
        let boundary = boundaries
            .entry(face.body.as_ref().map(|b| (&b.definition, &b.node)))
            .or_insert_with(Boundary::default);
        for tri in source.indices[face.start..face.start + face.count]
            .as_chunks::<3>()
            .0
        {
            boundary.add(
                tri.map(|i| positions[i as usize]),
                tri.map(|i| d[i as usize]),
                frame,
                grid,
            );
        }
    }
    let mut out = Output::default();
    let mut outlines = vec![];
    for (_, boundary) in boundaries {
        boundary.fill(&mut out, frame, &mut outlines);
    }
    (!out.p.is_empty()).then(|| (out.mesh(), outlines))
}
