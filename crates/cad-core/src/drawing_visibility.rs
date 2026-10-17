//! Application-owned orthographic visibility. Native planar faces with proved
//! straight closed trims use their exact polygon domains. Other faces use the
//! evaluated display tessellation, explicitly reported in output fidelity.
//! Curves are split in homogeneous coordinates; meshes never replace source
//! curves or establish associative dimensions.
use crate::{CadError, Drawing, Result, protocol::MeshData, sketch_edit::Curve3};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

type Point = [f64; 3];
type Triangle = [Point; 3];
type Bounds = [[f64; 2]; 2];
type BoundaryEdge = (u32, u32, Point, Point);
const MAX_ITEMS: usize = 2_000_000;
const MAX_WORK: usize = 8_000_000;
const PARAM_EPS: f64 = 2e-15;

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DisplayMode {
    #[default]
    Wireframe,
    HiddenLinesRemoved,
    HiddenLinesDashed,
}
impl DisplayMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Wireframe => "Wireframe",
            Self::HiddenLinesRemoved => "Hidden lines removed",
            Self::HiddenLinesDashed => "Hidden lines dashed",
        }
    }
    pub fn key(self) -> &'static str {
        match self {
            Self::Wireframe => "wireframe",
            Self::HiddenLinesRemoved => "hidden_lines_removed",
            Self::HiddenLinesDashed => "hidden_lines_dashed",
        }
    }
}
pub fn mode_for(sheet: &Drawing, view: &str) -> DisplayMode {
    sheet
        .view_display_modes
        .get(view)
        .copied()
        .unwrap_or_default()
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(tag = "method", rename_all = "snake_case")]
pub enum Fidelity {
    #[default]
    Wireframe,
    NativePlanar {
        tolerance_mm: f64,
    },
    Tessellated {
        chord_tolerance_mm: f64,
        tolerance_mm: f64,
    },
    SectionIntersection {
        chord_tolerance_mm: f64,
    },
}
impl Fidelity {
    pub fn approximate(&self) -> bool {
        matches!(
            self,
            Self::Tessellated { .. } | Self::SectionIntersection { .. }
        )
    }
}
#[derive(Default)]
pub(crate) struct Scene {
    faces: Vec<Face>,
    triangle_count: usize,
    work: usize,
}
struct Face {
    plane: Option<Triangle>,
    loops: Option<Vec<Vec<Point>>>,
    triangles: Vec<Triangle>,
    ghost: bool,
    curved: bool,
}
fn invalid(message: &str) -> CadError {
    CadError::Invalid(message.into())
}
fn limit() -> CadError {
    CadError::Unsupported("Drawing visibility exceeded its bounded geometry budget; reduce the displayed assembly or use Wireframe".into())
}
fn charge(work: &mut usize, amount: usize) -> Result<()> {
    *work = work.saturating_add(amount);
    if *work > MAX_WORK {
        Err(limit())
    } else {
        Ok(())
    }
}
fn transform(p: Point, m: [[f64; 4]; 4]) -> Point {
    std::array::from_fn(|i| m[i][0] * p[0] + m[i][1] * p[1] + m[i][2] * p[2] + m[i][3])
}
fn plus(a: Point, b: Point) -> Point {
    std::array::from_fn(|i| a[i] + b[i])
}
fn minus(a: Point, b: Point) -> Point {
    std::array::from_fn(|i| a[i] - b[i])
}
fn cross(a: Point, b: Point) -> Point {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn dot(a: Point, b: Point) -> f64 {
    (0..3).map(|i| a[i] * b[i]).sum()
}
fn length(a: Point) -> f64 {
    dot(a, a).sqrt()
}
fn projected(view: &str, p: Point) -> Result<Point> {
    let xy = crate::drawing::project(view, p)?;
    let depth = match view {
        "front" | "detail-front" => -p[1],
        "top" | "section-z" => p[2],
        "right" => p[0],
        "isometric" => (-p[0] - p[1] + p[2]) / 3f64.sqrt(),
        _ => return Err(invalid("Unknown drawing visibility view")),
    };
    Ok([xy[0], xy[1], depth])
}

impl Scene {
    /// Returns face tokens for each source edge, used only to bound tessellation
    /// self-occlusion error. Tokens never escape this disposable projection.
    pub(crate) fn add_mesh(
        &mut self,
        mesh: &MeshData,
        matrix: [[f64; 4]; 4],
        ghost: bool,
        tolerance_mm: f64,
    ) -> Result<Vec<Vec<usize>>> {
        if !tolerance_mm.is_finite() || tolerance_mm <= 0. {
            return Err(invalid(
                "Drawing visibility requires a positive finite modeling tolerance",
            ));
        }
        charge(
            &mut self.work,
            mesh.faces.len().saturating_mul(mesh.faces.len()),
        )?;
        charge(
            &mut self.work,
            mesh.faces
                .len()
                .saturating_mul(mesh.edges.len())
                .saturating_mul(3),
        )?;
        charge(&mut self.work, mesh.indices.len())?;
        if mesh
            .positions
            .iter()
            .flatten()
            .chain(matrix.iter().flatten())
            .any(|v| !v.is_finite())
            || !mesh.indices.len().is_multiple_of(3)
            || mesh
                .indices
                .iter()
                .any(|i| *i as usize >= mesh.positions.len())
        {
            return Err(invalid("Drawing visibility received invalid mesh indices"));
        }
        self.triangle_count += mesh.indices.len() / 3;
        if self.triangle_count > MAX_ITEMS {
            return Err(limit());
        }
        let first = self.faces.len();
        let mut covered = vec![false; mesh.indices.len() / 3];
        for face in &mesh.faces {
            let end = face.start.checked_add(face.count).ok_or_else(limit)?;
            if face.start % 3 != 0 || face.count % 3 != 0 || end > mesh.indices.len() {
                return Err(invalid("Drawing visibility received an invalid face range"));
            }
            for slot in &mut covered[face.start / 3..end / 3] {
                if *slot {
                    return Err(invalid("Drawing visibility face ranges overlap"));
                }
                *slot = true;
            }
            let triangles = mesh.indices[face.start..end]
                .as_chunks::<3>()
                .0
                .iter()
                .map(|tri| {
                    std::array::from_fn(|i| transform(mesh.positions[tri[i] as usize], matrix))
                })
                .collect();
            let plane = face.plane.as_ref().map(|p| {
                [p.origin, plus(p.origin, p.u), plus(p.origin, p.v)].map(|p| transform(p, matrix))
            });
            let loops = face
                .plane
                .as_ref()
                .and_then(|_| polygon_loops(mesh, &face.reference, tolerance_mm))
                .map(|loops| {
                    loops
                        .into_iter()
                        .map(|points| points.into_iter().map(|p| transform(p, matrix)).collect())
                        .collect::<Vec<Vec<Point>>>()
                })
                .filter(|loops| {
                    plane.as_ref().is_some_and(|plane| {
                        let normal = cross(minus(plane[1], plane[0]), minus(plane[2], plane[0]));
                        let magnitude = length(normal);
                        magnitude.is_finite()
                            && magnitude > 0.
                            && loops.iter().flatten().all(|point| {
                                let residual =
                                    dot(minus(*point, plane[0]), normal).abs() / magnitude;
                                residual.is_finite() && residual <= tolerance_mm * 0.125
                            })
                    })
                });
            self.faces.push(Face {
                plane,
                loops,
                triangles,
                ghost,
                curved: face.plane.is_none(),
            });
        }
        // Older adapters can omit face metadata. Preserve their occluders while
        // reporting the tessellated method rather than guessing native trims.
        let remainder = mesh
            .indices
            .as_chunks::<3>()
            .0
            .iter()
            .zip(covered)
            .filter(|(_, covered)| !covered)
            .map(|(tri, _)| {
                std::array::from_fn(|i| transform(mesh.positions[tri[i] as usize], matrix))
            })
            .collect::<Vec<_>>();
        if !remainder.is_empty() {
            self.faces.push(Face {
                plane: None,
                loops: None,
                triangles: remainder,
                ghost,
                curved: true,
            });
        }
        Ok(mesh
            .edges
            .iter()
            .map(|edge| {
                mesh.faces
                    .iter()
                    .enumerate()
                    .filter(|(_, face)| edge.faces.iter().any(|r| same_face(r, &face.reference)))
                    .map(|(i, _)| first + i)
                    .collect()
            })
            .collect())
    }
    pub(crate) fn project(
        &self,
        view: &str,
        accuracy: crate::tolerance::Accuracy,
    ) -> Result<ProjectedScene> {
        accuracy.modeling.validate()?;
        crate::tolerance::validate_display(accuracy.display_mm)?;
        let mut occluders = Vec::new();
        let mut silhouettes = Vec::new();
        let mut approximate = false;
        let mut work = self.work;
        for (face_id, face) in self.faces.iter().enumerate() {
            charge(
                &mut work,
                face.triangles
                    .len()
                    .saturating_mul(if face.curved { 6 } else { 1 }),
            )?;
            if !face.ghost {
                if let (Some(plane), Some(loops)) = (&face.plane, &face.loops) {
                    let plane = plane
                        .map(|p| projected(view, p))
                        .into_iter()
                        .collect::<Result<Vec<_>>>()?;
                    if let Some(depth) = depth_plane([plane[0], plane[1], plane[2]]) {
                        let loops = loops
                            .iter()
                            .map(|points| {
                                points
                                    .iter()
                                    .map(|p| projected(view, *p))
                                    .collect::<Result<Vec<_>>>()
                            })
                            .collect::<Result<Vec<_>>>()?;
                        occluders.push(Occluder::new(
                            loops,
                            depth,
                            face_id,
                            false,
                            accuracy.modeling.linear_mm,
                        )?);
                    }
                } else {
                    approximate = true;
                    for tri in &face.triangles {
                        let p = tri
                            .map(|p| projected(view, p))
                            .into_iter()
                            .collect::<Result<Vec<_>>>()?;
                        if let Some(depth) = depth_plane([p[0], p[1], p[2]]) {
                            occluders.push(Occluder::new(
                                vec![p],
                                depth,
                                face_id,
                                true,
                                accuracy.modeling.linear_mm,
                            )?);
                        }
                    }
                }
            }
            if face.curved {
                let mut edges: BTreeMap<([u64; 3], [u64; 3]), SilhouetteEdge> = BTreeMap::new();
                for tri in &face.triangles {
                    let p = tri
                        .map(|p| projected(view, p))
                        .into_iter()
                        .collect::<Result<Vec<_>>>()?;
                    let normal = cross(minus(p[1], p[0]), minus(p[2], p[0]));
                    let side = if normal[2] > length(normal) * 1e-10 {
                        1
                    } else if normal[2] < -length(normal) * 1e-10 {
                        -1
                    } else {
                        0
                    };
                    for i in 0..3 {
                        let (a, b) = (tri[i], tri[(i + 1) % 3]);
                        let bits = |p: Point| p.map(|v| if v == 0. { 0 } else { v.to_bits() });
                        let (a_key, b_key) = (bits(a), bits(b));
                        let key = if a_key < b_key {
                            (a_key, b_key)
                        } else {
                            (b_key, a_key)
                        };
                        let entry = edges.entry(key).or_insert(SilhouetteEdge {
                            points: [a, b],
                            positive: false,
                            negative: false,
                            count: 0,
                        });
                        entry.positive |= side > 0;
                        entry.negative |= side < 0;
                        entry.count += 1;
                    }
                }
                for edge in edges
                    .into_values()
                    .filter(|e| e.count == 2 && e.positive && e.negative)
                {
                    silhouettes.push((edge.points, face.ghost));
                    approximate = true;
                }
            }
            if occluders.len() + silhouettes.len() > MAX_ITEMS {
                return Err(limit());
            }
        }
        let bvh = Bvh::build((0..occluders.len()).collect(), &occluders);
        Ok(ProjectedScene {
            view: view.into(),
            occluders,
            bvh,
            silhouettes,
            accuracy,
            approximate,
            work,
        })
    }
}
struct SilhouetteEdge {
    points: [Point; 2],
    positive: bool,
    negative: bool,
    count: usize,
}
/// Exact straight trims are admitted only with native closed vertex incidence.
fn same_face(a: &crate::TopologyRef, b: &crate::TopologyRef) -> bool {
    a.producer == b.producer && a.role == b.role && a.topology == b.topology
}
fn polygon_loops(
    mesh: &MeshData,
    reference: &crate::TopologyRef,
    tolerance_mm: f64,
) -> Option<Vec<Vec<Point>>> {
    if mesh
        .faces
        .iter()
        .filter(|f| same_face(&f.reference, reference))
        .count()
        != 1
    {
        return None;
    }
    let mut boundaries: BTreeMap<(u32, u32), Vec<BoundaryEdge>> = BTreeMap::new();
    for edge in &mesh.edges {
        if !edge.faces.iter().any(|r| same_face(r, reference)) {
            continue;
        }
        let native = edge.native.as_ref()?;
        if native.version != crate::protocol::NATIVE_SELECTION_VERSION {
            return None;
        }
        let crate::protocol::EdgeCenter::Line {
            start,
            end,
            version,
        } = edge.center.as_ref()?
        else {
            return None;
        };
        if *version != crate::protocol::EDGE_CENTER_VERSION {
            return None;
        }
        // The guide's straight-carrier recognition has its own tolerance.
        // Admit only a native two-control degree-one curve here, then check
        // its actual endpoints against this evaluation's smaller budget.
        let curve = edge.curve.as_ref()?;
        if curve.degree != 1
            || curve.control_points.len() != 2
            || curve.weights.len() != 2
            || curve.weights.iter().any(|w| !w.is_finite() || *w <= 0.)
            || curve.knots.len() != 4
            || curve.knots.iter().any(|k| !k.is_finite())
            || curve.knots[0] != curve.knots[1]
            || curve.knots[2] != curve.knots[3]
            || curve.knots[1] >= curve.knots[2]
        {
            return None;
        }
        let [p, q] = native.endpoints;
        if [
            p,
            q,
            *start,
            *end,
            curve.control_points[0],
            curve.control_points[1],
        ]
        .iter()
        .flatten()
        .any(|v| !v.is_finite())
        {
            return None;
        }
        let residual = length(minus(*start, p))
            .max(length(minus(*end, q)))
            .min(length(minus(*start, q)).max(length(minus(*end, p))));
        if !residual.is_finite() || residual > tolerance_mm * 0.125 {
            return None;
        }
        let [a, b] = [curve.control_points[0], curve.control_points[1]];
        let source_residual = length(minus(a, p))
            .max(length(minus(b, q)))
            .min(length(minus(a, q)).max(length(minus(b, p))));
        if source_residual > tolerance_mm * 0.125 {
            return None;
        }
        let mut entries = 0;
        for boundary in native
            .boundaries
            .iter()
            .filter(|b| same_face(&b.face, reference))
        {
            boundaries
                .entry((native.component, boundary.boundary))
                .or_default()
                .push((native.vertices[0], native.vertices[1], p, q));
            entries += 1;
        }
        if entries == 0 {
            return None;
        }
    }
    if boundaries.is_empty() {
        return None;
    }
    let mut result = Vec::new();
    for edges in boundaries.into_values() {
        if edges.len() < 3 {
            return None;
        }
        let mut incidence: BTreeMap<u32, Vec<usize>> = BTreeMap::new();
        for (index, (a, b, _, _)) in edges.iter().enumerate() {
            if a == b {
                return None;
            }
            incidence.entry(*a).or_default().push(index);
            incidence.entry(*b).or_default().push(index);
        }
        if incidence.values().any(|v| v.len() != 2) {
            return None;
        }
        let mut visited = BTreeSet::new();
        let start = edges[0].0;
        let mut current = start;
        let mut points = Vec::new();
        loop {
            let index = *incidence
                .get(&current)?
                .iter()
                .find(|i| !visited.contains(*i))?;
            visited.insert(index);
            let (a, b, p, q) = edges[index];
            if current == a {
                points.push(p);
                current = b;
            } else {
                points.push(q);
                current = a;
            }
            if current == start {
                break;
            }
            if visited.len() >= edges.len() {
                return None;
            }
        }
        if visited.len() != edges.len() {
            return None;
        }
        result.push(points);
    }
    Some(result)
}
fn depth_plane(p: Triangle) -> Option<Point> {
    let n = cross(minus(p[1], p[0]), minus(p[2], p[0]));
    if n[2].abs() <= length(n) * f64::EPSILON * 16. || !n.iter().all(|v| v.is_finite()) {
        return None;
    }
    Some([-n[0] / n[2], -n[1] / n[2], dot(n, p[0]) / n[2]])
}
struct Occluder {
    loops: Vec<Vec<Point>>,
    depth: Point,
    bounds: Bounds,
    max_depth: f64,
    face: usize,
    approximate: bool,
}
impl Occluder {
    fn new(
        loops: Vec<Vec<Point>>,
        depth: Point,
        face: usize,
        approximate: bool,
        tolerance: f64,
    ) -> Result<Self> {
        if loops
            .iter()
            .flatten()
            .flatten()
            .any(|v| !v.is_finite() || v.abs() * f64::EPSILON * 128. > tolerance)
            || depth.iter().any(|v| !v.is_finite())
        {
            return Err(invalid(
                "Drawing visibility produced nonfinite or unbounded coordinates",
            ));
        }
        let bounds = bounds(loops.iter().flatten().copied());
        let arithmetic_scale = depth[0].abs() * bounds[0][0].abs().max(bounds[1][0].abs())
            + depth[1].abs() * bounds[0][1].abs().max(bounds[1][1].abs())
            + depth[2].abs();
        if arithmetic_scale * f64::EPSILON * 64. > tolerance {
            return Err(CadError::Unsupported("Drawing visibility plane depth is too poorly conditioned for the requested tolerance; use Wireframe or a different view".into()));
        }
        let max_depth = loops
            .iter()
            .flatten()
            .map(|p| p[2])
            .fold(f64::NEG_INFINITY, f64::max);
        Ok(Self {
            loops,
            depth,
            bounds,
            max_depth,
            face,
            approximate,
        })
    }
    fn contains(&self, p: Point, epsilon: f64, work: &mut usize) -> Result<bool> {
        let mut inside = false;
        for polygon in &self.loops {
            for i in 0..polygon.len() {
                charge(work, 1)?;
                let (a, b) = (polygon[i], polygon[(i + 1) % polygon.len()]);
                let dx = b[0] - a[0];
                let dy = b[1] - a[1];
                let len = dx.hypot(dy);
                let cross = (p[0] - a[0]) * dy - (p[1] - a[1]) * dx;
                if len > 0.
                    && cross.abs() <= epsilon * len
                    && (p[0] - a[0]) * (p[0] - b[0]) + (p[1] - a[1]) * (p[1] - b[1])
                        <= epsilon * epsilon
                {
                    return Ok(true);
                }
                if (a[1] > p[1]) != (b[1] > p[1]) && p[0] < a[0] + (p[1] - a[1]) * dx / dy {
                    inside = !inside;
                }
            }
        }
        Ok(inside)
    }
}
fn bounds(points: impl IntoIterator<Item = Point>) -> Bounds {
    let mut b = [[f64::INFINITY; 2], [f64::NEG_INFINITY; 2]];
    for p in points {
        for i in 0..2 {
            b[0][i] = b[0][i].min(p[i]);
            b[1][i] = b[1][i].max(p[i]);
        }
    }
    b
}
fn overlaps(a: Bounds, b: Bounds) -> bool {
    (0..2).all(|i| a[1][i] >= b[0][i] && b[1][i] >= a[0][i])
}
enum Bvh {
    Empty,
    Leaf {
        bounds: Bounds,
        items: Vec<usize>,
    },
    Branch {
        bounds: Bounds,
        children: Box<[Bvh; 2]>,
    },
}
impl Bvh {
    fn build(mut items: Vec<usize>, faces: &[Occluder]) -> Self {
        if items.is_empty() {
            return Self::Empty;
        }
        let b = bounds(
            items
                .iter()
                .flat_map(|i| faces[*i].bounds.map(|p| [p[0], p[1], 0.])),
        );
        if items.len() <= 8 {
            return Self::Leaf { bounds: b, items };
        }
        let axis = usize::from(b[1][1] - b[0][1] > b[1][0] - b[0][0]);
        items.sort_unstable_by(|a, b| {
            (faces[*a].bounds[0][axis] + faces[*a].bounds[1][axis])
                .total_cmp(&(faces[*b].bounds[0][axis] + faces[*b].bounds[1][axis]))
        });
        let second = items.split_off(items.len() / 2);
        Self::Branch {
            bounds: b,
            children: Box::new([Self::build(items, faces), Self::build(second, faces)]),
        }
    }
    fn query(&self, b: Bounds, out: &mut Vec<usize>, work: &mut usize) -> Result<()> {
        charge(work, 1)?;
        match self {
            Self::Empty => (),
            Self::Leaf { bounds, items } if overlaps(*bounds, b) => out.extend(items),
            Self::Branch { bounds, children } if overlaps(*bounds, b) => {
                children[0].query(b, out, work)?;
                children[1].query(b, out, work)?;
            }
            _ => (),
        }
        Ok(())
    }
}
pub(crate) struct ProjectedScene {
    view: String,
    occluders: Vec<Occluder>,
    bvh: Bvh,
    pub silhouettes: Vec<([Point; 2], bool)>,
    accuracy: crate::tolerance::Accuracy,
    approximate: bool,
    work: usize,
}
pub(crate) struct Piece {
    pub curve: Curve3,
    pub hidden: bool,
}
impl ProjectedScene {
    pub fn fidelity(&self) -> Fidelity {
        if self.approximate {
            Fidelity::Tessellated {
                chord_tolerance_mm: self.accuracy.display_mm,
                tolerance_mm: self.accuracy.modeling.linear_mm,
            }
        } else {
            Fidelity::NativePlanar {
                tolerance_mm: self.accuracy.modeling.linear_mm,
            }
        }
    }
    pub fn split(&mut self, curve: &Curve3, source_faces: &[usize]) -> Result<Vec<Piece>> {
        charge(
            &mut self.work,
            curve
                .control_points
                .len()
                .saturating_mul(curve.control_points.len())
                .saturating_mul(curve.degree.saturating_add(1)),
        )?;
        let spans = bezier_spans(curve)?;
        let mut result = Vec::new();
        for world in spans {
            let controls = world
                .control_points
                .iter()
                .map(|p| projected(&self.view, *p))
                .collect::<Result<Vec<_>>>()?;
            let tolerance = self.accuracy.modeling.linear_mm.max(1e-9);
            let coordinate = controls
                .iter()
                .flatten()
                .map(|v| v.abs())
                .fold(1., f64::max);
            if coordinate * f64::EPSILON * 128. > tolerance {
                return Err(CadError::Unsupported("Drawing visibility coordinates exceed the requested numerical tolerance; move the model closer to its origin or use Wireframe".into()));
            }
            let diameter = controls
                .iter()
                .flat_map(|a| controls.iter().map(move |b| length(minus(*a, *b))))
                .fold(0., f64::max);
            let min_weight = world.weights.iter().copied().fold(f64::INFINITY, f64::min);
            let max_weight = world.weights.iter().copied().fold(0., f64::max);
            let ratio = max_weight / min_weight;
            if ratio > 1e8 {
                return Err(CadError::Unsupported("Drawing visibility rational weights exceed the bounded numerical conditioning contract".into()));
            }
            let speed = (2. * world.degree as f64 * ratio * ratio * diameter).max(tolerance);
            let parameter_tolerance = (tolerance / (speed * 8.)).min(1e-8);
            if parameter_tolerance < PARAM_EPS {
                return Err(CadError::Unsupported("Drawing visibility cannot isolate this rational curve within its requested spatial tolerance".into()));
            }
            let zero_tolerance = tolerance * min_weight / max_weight * 0.001;
            let b = bounds(controls.iter().copied());
            let mut candidates = Vec::new();
            self.bvh.query(b, &mut candidates, &mut self.work)?;
            let min_depth = controls.iter().map(|p| p[2]).fold(f64::INFINITY, f64::min);
            let mut hidden = Vec::new();
            for id in candidates {
                charge(&mut self.work, 1 + source_faces.len())?;
                let face = &self.occluders[id];
                if !overlaps(face.bounds, b)
                    || face.max_depth <= min_depth + self.accuracy.modeling.linear_mm
                {
                    continue;
                }
                let bias = if face.approximate && source_faces.contains(&face.face) {
                    self.accuracy.display_mm
                        * (1. + face.depth[0].powi(2) + face.depth[1].powi(2)).sqrt()
                        + tolerance
                } else {
                    tolerance
                };
                let mut uncertain = Vec::new();
                let mut boundaries = vec![0., 1.];
                for polygon in &face.loops {
                    for i in 0..polygon.len() {
                        charge(&mut self.work, controls.len())?;
                        let (a, b) = (polygon[i], polygon[(i + 1) % polygon.len()]);
                        let delta = [b[0] - a[0], b[1] - a[1]];
                        let scale = delta[0].hypot(delta[1]);
                        if scale <= tolerance {
                            continue;
                        }
                        let linear = [
                            -delta[1] / scale,
                            delta[0] / scale,
                            0.,
                            (delta[1] * a[0] - delta[0] * a[1]) / scale,
                        ];
                        uncertain.extend(roots(
                            &coefficients(&controls, &world.weights, linear),
                            parameter_tolerance,
                            zero_tolerance,
                            &mut self.work,
                        )?);
                    }
                }
                uncertain.extend(roots(
                    &coefficients(
                        &controls,
                        &world.weights,
                        [face.depth[0], face.depth[1], -1., face.depth[2] - bias],
                    ),
                    parameter_tolerance,
                    zero_tolerance,
                    &mut self.work,
                )?);
                boundaries.extend(uncertain.iter().flatten().copied());
                boundaries.sort_by(f64::total_cmp);
                boundaries.dedup_by(|a, b| (*a - *b).abs() < PARAM_EPS);
                for pair in boundaries.windows(2) {
                    charge(
                        &mut self.work,
                        uncertain.len() + controls.len() * controls.len(),
                    )?;
                    if pair[1] - pair[0] <= PARAM_EPS
                        || uncertain.iter().any(|u| pair[0] < u[1] && pair[1] > u[0])
                    {
                        continue;
                    }
                    let point = bezier_point(&controls, &world.weights, (pair[0] + pair[1]) * 0.5);
                    if face.depth[0] * point[0] + face.depth[1] * point[1] + face.depth[2]
                        > point[2] + bias
                        && face.contains(point, tolerance, &mut self.work)?
                    {
                        hidden.push([pair[0], pair[1]]);
                    }
                }
            }
            hidden.sort_by(|a, b| a[0].total_cmp(&b[0]));
            let mut merged: Vec<[f64; 2]> = Vec::new();
            for interval in hidden {
                if let Some(last) = merged.last_mut()
                    && interval[0] <= last[1] + PARAM_EPS
                {
                    last[1] = last[1].max(interval[1]);
                } else {
                    merged.push(interval);
                }
            }
            let mut at = 0.;
            for [start, end] in merged {
                charge(&mut self.work, controls.len() * controls.len())?;
                if start > at + PARAM_EPS {
                    result.push(Piece {
                        curve: subcurve(&world, at, start),
                        hidden: false,
                    });
                }
                result.push(Piece {
                    curve: subcurve(&world, start, end),
                    hidden: true,
                });
                at = end;
            }
            if at < 1. - PARAM_EPS {
                result.push(Piece {
                    curve: subcurve(&world, at, 1.),
                    hidden: false,
                });
            }
            if result.len() > MAX_ITEMS {
                return Err(limit());
            }
        }
        Ok(result)
    }
}
pub(crate) fn line_curve(points: [Point; 2]) -> Curve3 {
    Curve3 {
        control_points: points.to_vec(),
        weights: vec![1., 1.],
        knots: vec![0., 0., 1., 1.],
        degree: 1,
    }
}
fn bezier_spans(curve: &Curve3) -> Result<Vec<Curve3>> {
    if curve.degree > 32
        || curve.control_points.len() > 100_000
        || curve
            .control_points
            .iter()
            .flatten()
            .any(|p| !p.is_finite())
    {
        return Err(limit());
    }
    let xy = crate::sketch_edit::Curve2 {
        control_points: curve.control_points.iter().map(|p| [p[0], p[1]]).collect(),
        weights: curve.weights.clone(),
        knots: curve.knots.clone(),
        degree: curve.degree,
    };
    let xz = crate::sketch_edit::Curve2 {
        control_points: curve.control_points.iter().map(|p| [p[0], p[2]]).collect(),
        ..xy.clone()
    };
    let a = crate::sketch_trim::bezier_spans(&xy)?;
    let b = crate::sketch_trim::bezier_spans(&xz)?;
    if a.len() != b.len() {
        return Err(invalid("Inconsistent rational drawing curve spans"));
    }
    Ok(a.into_iter()
        .zip(b)
        .map(|(a, b)| Curve3 {
            control_points: a
                .control_points
                .iter()
                .zip(&b.control_points)
                .map(|(a, b)| [a[0], a[1], b[1]])
                .collect(),
            weights: a.weights,
            knots: a.knots,
            degree: a.degree,
        })
        .collect())
}
fn coefficients(controls: &[Point], weights: &[f64], plane: [f64; 4]) -> Vec<f64> {
    let max_weight = weights.iter().copied().fold(0., f64::max);
    controls
        .iter()
        .zip(weights)
        .map(|(p, w)| {
            (plane[0] * p[0] + plane[1] * p[1] + plane[2] * p[2] + plane[3]) * (w / max_weight)
        })
        .collect()
}
fn roots(
    values: &[f64],
    parameter_tolerance: f64,
    zero_tolerance: f64,
    work: &mut usize,
) -> Result<Vec<[f64; 2]>> {
    charge(work, values.len())?;
    let scale = values.iter().map(|v| v.abs()).fold(0., f64::max);
    if !scale.is_finite() {
        return Err(invalid("Drawing visibility polynomial overflow"));
    }
    if scale <= zero_tolerance {
        return Ok(vec![]);
    }
    let values = values.iter().map(|v| v / scale).collect::<Vec<_>>();
    if values.len() == 2 {
        let denominator = values[0] - values[1];
        if denominator.abs() < 1e-15 {
            return Ok(vec![]);
        }
        let t = values[0] / denominator;
        return Ok(if (0. ..=1.).contains(&t) {
            vec![[
                (t - parameter_tolerance).max(0.),
                (t + parameter_tolerance).min(1.),
            ]]
        } else {
            vec![]
        });
    }
    let mut stack = vec![(values, 0., 1., 0)];
    let mut out = Vec::new();
    while let Some((values, a, b, depth)) = stack.pop() {
        charge(work, values.len().saturating_mul(values.len()))?;
        let min = values.iter().copied().fold(f64::INFINITY, f64::min);
        let max = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        if min > 1e-14 || max < -1e-14 {
            continue;
        }
        if b - a <= parameter_tolerance {
            out.push([a, b]);
            continue;
        }
        if depth >= 50 {
            return Err(CadError::Unsupported(
                "Drawing visibility root isolation cannot meet its spatial tolerance".into(),
            ));
        }
        // Near-coincident intervals remain visible; their parameter extent is
        // retained rather than silently discarding possible roots.
        if max.abs().max(min.abs()) < 1e-14 {
            out.push([a, b]);
            continue;
        }
        let mut values = values;
        let mut left = vec![values[0]];
        let mut right = vec![*values.last().unwrap()];
        for width in (1..values.len()).rev() {
            for i in 0..width {
                values[i] = (values[i] + values[i + 1]) * 0.5;
            }
            left.push(values[0]);
            right.push(values[width - 1]);
        }
        right.reverse();
        let mid = (a + b) * 0.5;
        stack.push((right, mid, b, depth + 1));
        stack.push((left, a, mid, depth + 1));
    }
    out.sort_by(|a, b| a[0].total_cmp(&b[0]));
    Ok(out)
}
fn bezier_point(controls: &[Point], weights: &[f64], t: f64) -> Point {
    let mut h = controls
        .iter()
        .zip(weights)
        .map(|(p, w)| [p[0] * w, p[1] * w, p[2] * w, *w])
        .collect::<Vec<_>>();
    for width in (1..h.len()).rev() {
        for i in 0..width {
            h[i] = std::array::from_fn(|j| h[i][j] * (1. - t) + h[i + 1][j] * t);
        }
    }
    [h[0][0] / h[0][3], h[0][1] / h[0][3], h[0][2] / h[0][3]]
}
fn subcurve(curve: &Curve3, start: f64, end: f64) -> Curve3 {
    let controls = curve
        .control_points
        .iter()
        .zip(&curve.weights)
        .map(|(p, w)| [p[0] * w, p[1] * w, p[2] * w, *w])
        .collect::<Vec<_>>();
    let split = |mut p: Vec<[f64; 4]>, t: f64| {
        let mut left = vec![p[0]];
        let mut right = vec![*p.last().unwrap()];
        for width in (1..p.len()).rev() {
            for i in 0..width {
                p[i] = std::array::from_fn(|j| p[i][j] * (1. - t) + p[i + 1][j] * t);
            }
            left.push(p[0]);
            right.push(p[width - 1]);
        }
        right.reverse();
        (left, right)
    };
    let controls = if end < 1. {
        split(controls, end).0
    } else {
        controls
    };
    let controls = if start > 0. {
        split(controls, start / end).1
    } else {
        controls
    };
    Curve3 {
        control_points: controls
            .iter()
            .map(|p| [p[0] / p[3], p[1] / p[3], p[2] / p[3]])
            .collect(),
        weights: controls.iter().map(|p| p[3]).collect(),
        knots: std::iter::repeat_n(0., curve.degree + 1)
            .chain(std::iter::repeat_n(1., curve.degree + 1))
            .collect(),
        degree: curve.degree,
    }
}
