//! Exact layered Booleans for common-axis line and circular extrusion walls.
//!
//! One analytic planar arrangement is shared by every height interval. Native
//! curves are cut at line/line, line/circle and circle/circle intersections;
//! membership is constant on each resulting interval. Horizontal transitions
//! use the same partition as the walls, so coplanar caps and coincident curved
//! walls never depend on tessellated intersection seeds.
use crate::boolean_ops::Mode;
use crate::join::region::{
    Frame, Location, Segment, checked_intersections, locate, wire_from_parts,
};
use cad_core::{CadError, Result};
use monstertruck::{geometry::prelude::TryIntoHomogeneousBsplineSurface, modeling::*};
use std::collections::{HashMap, HashSet};

const EPS: f64 = 1e-7;
const MAX_FACES: usize = 4096;
const MAX_ATOMS: usize = 16_384;
const MAX_WORK: usize = 16_000_000;
const MAX_CONTROLS: usize = 262_144;

fn unsupported(message: &str) -> CadError {
    CadError::Unsupported(format!("Prismatic Boolean: {message}"))
}
fn invalid(message: impl std::fmt::Display) -> CadError {
    CadError::Invalid(format!("Geometry: prismatic Boolean: {message}"))
}
fn spend(work: &mut usize, count: usize) -> Result<()> {
    *work = work.saturating_add(count);
    if *work > MAX_WORK {
        return Err(unsupported(
            "the complete arrangement exceeds its bounded work budget",
        ));
    }
    Ok(())
}
fn finite(p: Point3) -> bool {
    [p.x, p.y, p.z]
        .into_iter()
        .all(|v| v.is_finite() && v.abs() <= 1e12)
}
fn rational(p: Vector4) -> Option<Point3> {
    if ![p.x, p.y, p.z, p.w].into_iter().all(f64::is_finite) || p.w <= 0. {
        return None;
    }
    let p = Point3::new(p.x / p.w, p.y / p.w, p.z / p.w);
    finite(p).then_some(p)
}

// The entire carrier, rather than a few evaluations, must be a translation
// along the common axis. Its parameter height may exceed the trimmed face.
fn translation(face: &Face, axis: Vector3) -> bool {
    if let Surface::Plane(_) = face.surface() {
        let normal = face.oriented_surface().normal(0., 0.).normalize();
        return normal.magnitude2().is_finite() && normal.dot(axis).abs() <= 1e-10;
    }
    let Some(mut surface) = face.surface().try_into_homogeneous_bspline_surface() else {
        return false;
    };
    if surface.vdegree() != 1 && surface.udegree() == 1 {
        surface.invert();
    }
    let controls = surface.control_points();
    if surface.vdegree() != 1
        || controls.is_empty()
        || controls.len() > 1024
        || controls.iter().any(|row| row.len() != 2)
    {
        return false;
    }
    let first = &controls[0];
    let (Some(p), Some(q)) = (rational(first[0]), rational(first[1])) else {
        return false;
    };
    let delta = q - p;
    if delta.magnitude() <= EPS || delta.cross(axis).magnitude() > EPS {
        return false;
    }
    let weight = first[1].w / first[0].w;
    controls.iter().all(|row| {
        let (Some(p), Some(q)) = (rational(row[0]), rational(row[1])) else {
            return false;
        };
        let ratio = row[1].w / row[0].w;
        ratio.is_finite()
            && (ratio - weight).abs() <= 1e-12 * weight.abs()
            && (q - p - delta).magnitude() <= EPS
    })
}

struct Wall {
    face: Face,
    edge: Edge,
    low: f64,
    high: f64,
    side: usize,
}
struct Cap {
    face: FaceId,
    height: f64,
}
struct Stock {
    walls: Vec<Wall>,
    caps: Vec<Cap>,
}
fn height(frame: &Frame, p: Point3) -> f64 {
    (p - frame.origin).dot(frame.normal)
}
fn recognize(body: &Solid, side: usize, frame: &Frame) -> Result<Option<Stock>> {
    let mut walls = Vec::new();
    let mut caps = Vec::new();
    let mut control_count = 0usize;
    for face in body.face_iter() {
        if let Surface::Plane(_) = face.surface() {
            let normal = face.oriented_surface().normal(0., 0.).normalize();
            if normal.cross(frame.normal).magnitude() <= 1e-10 {
                let Some(vertex) = face.vertex_iter().next() else {
                    return Ok(None);
                };
                let z = height(frame, vertex.point());
                for edge in face.edge_iter() {
                    let points = match crate::extrusion::curve_hull(&edge.curve()) {
                        Ok(points) => points,
                        Err(CadError::Unsupported(_)) => return Ok(None),
                        Err(error) => return Err(error),
                    };
                    control_count = control_count.saturating_add(points.len());
                    if control_count > MAX_CONTROLS {
                        return Ok(None);
                    }
                    if points
                        .iter()
                        .any(|p| !finite(*p) || (height(frame, *p) - z).abs() > EPS)
                    {
                        return Ok(None);
                    }
                }
                caps.push(Cap {
                    face: face.id(),
                    height: z,
                });
                continue;
            }
        }
        let boundaries = face.boundaries();
        if boundaries.len() != 1 || boundaries[0].len() != 4 || !translation(face, frame.normal) {
            return Ok(None);
        }
        let mut horizontal = Vec::new();
        let mut vertical = 0;
        for edge in &boundaries[0] {
            let a = edge.front().point();
            let b = edge.back().point();
            if !finite(a) || !finite(b) {
                return Ok(None);
            }
            let az = height(frame, a);
            let bz = height(frame, b);
            if (az - bz).abs() <= EPS {
                let points = match crate::extrusion::curve_hull(&edge.curve()) {
                    Ok(points) => points,
                    Err(CadError::Unsupported(_)) => return Ok(None),
                    Err(error) => return Err(error),
                };
                control_count = control_count.saturating_add(points.len());
                if control_count > MAX_CONTROLS {
                    return Ok(None);
                }
                if points
                    .iter()
                    .any(|p| !finite(*p) || (height(frame, *p) - az).abs() > EPS)
                    || frame.segment(edge)?.is_none()
                {
                    return Ok(None);
                }
                horizontal.push((az, edge.clone()));
            } else if crate::bounded_carriers::line(&edge.curve()).is_ok()
                && (b - a).cross(frame.normal).magnitude() <= EPS
            {
                vertical += 1;
            } else {
                return Ok(None);
            }
        }
        if horizontal.len() != 2 || vertical != 2 {
            return Ok(None);
        }
        horizontal.sort_by(|a, b| a.0.total_cmp(&b.0));
        let (low, lower) = &horizontal[0];
        let (high, upper) = &horizontal[1];
        if high - low <= EPS {
            return Ok(None);
        }
        let translated = builder::translated(lower, frame.normal * (high - low));
        if !crate::join::matching_edge(&translated, &upper.inverse())? {
            return Ok(None);
        }
        let edge = builder::translated(lower, -frame.normal * *low);
        // A source's forward direction is always its oriented lower coedge.
        let edge =
            Edge::try_new(edge.front(), edge.back(), edge.oriented_curve()).map_err(invalid)?;
        walls.push(Wall {
            face: face.clone(),
            edge,
            low: *low,
            high: *high,
            side,
        });
    }
    if walls.is_empty() || caps.len() < 2 {
        return Ok(None);
    }
    Ok(Some(Stock { walls, caps }))
}

struct Source {
    edge: Edge,
    segment: Segment,
    cuts: Vec<(f64, usize)>,
    controls: usize,
}
struct Atom {
    edge: Edge,
    segment: Segment,
    // Wall index and orientation relative to this atom.
    sources: Vec<(usize, i8, usize)>,
}
struct Arrangement {
    atoms: Vec<Atom>,
}

// Native IDs are evaluation-local lookup keys only. Saved descriptors contain
// source construction references and ordered source-curve intervals.
struct Descriptor {
    kind: &'static str,
    elements: Vec<(FaceId, usize, i8)>,
    planes: Vec<Vec<FaceId>>,
}
pub(crate) struct Output {
    pub(crate) solid: Option<Solid>,
    faces: Vec<(Face, Descriptor)>,
    edges: Vec<(Edge, Descriptor)>,
}
fn elements(atom: &Atom, walls: &[Wall]) -> Vec<(FaceId, usize, i8)> {
    atom.sources
        .iter()
        .map(|(wall, _, interval)| (walls[*wall].face.id(), *interval, 0))
        .collect()
}

pub(crate) fn record_history(
    output: &Output,
    scope: &std::collections::BTreeSet<cad_core::NodeRef>,
    context: crate::revolution::Combination<'_>,
) -> Result<()> {
    let mut origins = HashMap::new();
    let mut ancestry_bytes = 0usize;
    for descriptor in output
        .faces
        .iter()
        .map(|(_, tag)| tag)
        .chain(output.edges.iter().map(|(_, tag)| tag))
    {
        for id in descriptor
            .elements
            .iter()
            .map(|(id, _, _)| id)
            .chain(descriptor.planes.iter().flatten())
        {
            if let Some(origin) = origins.get(id) {
                let origin: &cad_core::TopologyRef = origin;
                ancestry_bytes = ancestry_bytes
                    .saturating_add(origin.role.len().saturating_mul(12).saturating_add(1024));
                if ancestry_bytes > 64 * 1024 * 1024 {
                    return Err(unsupported(
                        "native partition ancestry exceeds its bounded serialization budget",
                    ));
                }
            }
            if origins.contains_key(id) {
                continue;
            }
            let origin = context
                .generated
                .face_source(*id)
                .or_else(|| {
                    scope
                        .iter()
                        .find_map(|reference| context.adapter.face_source(reference, *id))
                })
                .ok_or_else(|| {
                    unsupported(
                        "a native partition contributor has no current source-face ancestry",
                    )
                })?;
            ancestry_bytes = ancestry_bytes
                .saturating_add(origin.role.len().saturating_mul(12).saturating_add(1024));
            if ancestry_bytes > 64 * 1024 * 1024 {
                return Err(unsupported(
                    "native partition ancestry exceeds its bounded serialization budget",
                ));
            }
            origins.insert(*id, origin);
        }
    }
    let resolve = |tag: &Descriptor| -> Result<_> {
        // Keep recursive construction ancestry once in the canonical source
        // table. Purpose fields contain indices, never escaped copies of the
        // complete ancestry tree from every previous Boolean.
        let ids = tag
            .elements
            .iter()
            .map(|(id, _, _)| *id)
            .chain(tag.planes.iter().flatten().copied())
            .collect::<HashSet<_>>();
        let mut table = std::collections::BTreeMap::new();
        let mut keys = HashMap::new();
        for id in ids {
            let source = &origins[&id];
            let key = serde_json::to_string(&crate::adapter::construction_reference(source)?)?;
            table.entry(key.clone()).or_insert_with(|| source.clone());
            keys.insert(id, key);
        }
        let indices = table
            .keys()
            .enumerate()
            .map(|(i, key)| (key.as_str(), i))
            .collect::<HashMap<_, _>>();
        let mut elements = tag
            .elements
            .iter()
            .map(|(id, interval, endpoint)| (indices[keys[id].as_str()], *interval, *endpoint))
            .collect::<Vec<_>>();
        elements.sort_unstable();
        elements.dedup();
        let planes = tag
            .planes
            .iter()
            .map(|level| {
                let mut indices = level
                    .iter()
                    .map(|id| indices[keys[id].as_str()])
                    .collect::<Vec<_>>();
                indices.sort_unstable();
                indices.dedup();
                indices
            })
            .collect::<Vec<_>>();
        Ok((
            table.into_values().collect::<Vec<_>>(),
            serde_json::json!({
                "operation":"prismatic_boolean","constructor":tag.kind,
                "source_table":"canonical_construction_sources","intervals":elements,"planes":planes,
            }),
        ))
    };
    for (face, tag) in &output.faces {
        let (sources, purpose) = resolve(tag)?;
        context
            .generated
            .tool_face(context.owner, face, &sources, &purpose)?;
    }
    for (edge, tag) in &output.edges {
        let (sources, purpose) = resolve(tag)?;
        context
            .generated
            .tool_edge(context.owner, edge, &sources, &purpose)?;
    }
    Ok(())
}
fn vertex(
    vertices: &mut Vec<Vertex>,
    clouds: &mut Vec<Vec<Point3>>,
    p: Point3,
    work: &mut usize,
) -> Result<usize> {
    spend(work, vertices.len())?;
    let candidates = vertices
        .iter()
        .enumerate()
        .filter(|(_, v)| v.point().distance(p) <= EPS)
        .map(|(i, _)| i)
        .collect::<Vec<_>>();
    if candidates.len() > 1 {
        return Err(unsupported(
            "an intersection has more than one distinct native vertex within tolerance",
        ));
    }
    if let Some(&index) = candidates.first() {
        spend(work, clouds[index].len())?;
        if clouds[index]
            .iter()
            .any(|previous| previous.distance(p) > EPS)
        {
            return Err(unsupported(
                "a vertex cluster would bridge distinct boundaries beyond native tolerance",
            ));
        }
        clouds[index].push(p);
        return Ok(index);
    }
    if vertices.len() >= MAX_ATOMS {
        return Err(unsupported("too many arrangement vertices"));
    }
    vertices.push(Vertex::new(p));
    clouds.push(vec![p]);
    Ok(vertices.len() - 1)
}
fn parameter(source: &Source, p: Point3) -> Result<f64> {
    let curve = source.edge.curve();
    let (lo, hi) = curve.range_tuple();
    if source.edge.front().point().distance(p) <= EPS {
        return Ok(lo);
    }
    if source.edge.back().point().distance(p) <= EPS {
        return Ok(hi);
    }
    let t = curve.search_parameter(p, None, 100).ok_or_else(|| {
        unsupported("an analytic crossing could not be placed on its native curve")
    })?;
    if !t.is_finite() || t <= lo || t >= hi || curve.subs(t).distance(p) > EPS {
        return Err(unsupported(
            "an analytic crossing failed its native-curve residual check",
        ));
    }
    Ok(t)
}
fn arrangement(walls: &[Wall], frame: &Frame, work: &mut usize) -> Result<Arrangement> {
    let mut vertices = Vec::new();
    let mut vertex_clouds = Vec::new();
    let mut sources = Vec::new();
    for wall in walls {
        let edge = wall.edge.clone();
        let segment = frame
            .segment(&edge)?
            .ok_or_else(|| invalid("recognized wall lost its analytic section"))?;
        let a = vertex(
            &mut vertices,
            &mut vertex_clouds,
            edge.front().point(),
            work,
        )?;
        let b = vertex(&mut vertices, &mut vertex_clouds, edge.back().point(), work)?;
        let (lo, hi) = edge.curve().range_tuple();
        if a == b || !lo.is_finite() || !hi.is_finite() || hi <= lo {
            return Err(unsupported("a section has a collapsed or unbounded curve"));
        }
        let controls = crate::extrusion::curve_hull(&edge.curve())?.len();
        sources.push(Source {
            edge,
            segment,
            cuts: vec![(lo, a), (hi, b)],
            controls,
        });
    }
    for i in 0..sources.len() {
        for j in i + 1..sources.len() {
            spend(work, 1)?;
            for p in checked_intersections(&sources[i].segment, &sources[j].segment)? {
                spend(
                    work,
                    (sources[i].controls + sources[j].controls).saturating_mul(100),
                )?;
                let p = frame.lift(p);
                let ti = parameter(&sources[i], p)?;
                let tj = parameter(&sources[j], p)?;
                let index = vertex(&mut vertices, &mut vertex_clouds, p, work)?;
                sources[i].cuts.push((ti, index));
                sources[j].cuts.push((tj, index));
            }
        }
    }
    let mut atoms: Vec<Atom> = Vec::new();
    let mut endpoints: HashMap<(VertexId, VertexId), Vec<usize>> = HashMap::new();
    let mut controls = 0usize;
    for (source_index, source) in sources.iter_mut().enumerate() {
        source.cuts.sort_by(|a, b| a.0.total_cmp(&b.0));
        source.cuts.dedup_by(|a, b| a.1 == b.1);
        for (interval, pair) in source.cuts.windows(2).enumerate() {
            let ((lo, a), (hi, b)) = (pair[0], pair[1]);
            if a == b || hi <= lo {
                return Err(unsupported("inconsistent split parameters"));
            }
            controls = controls.saturating_add(source.controls);
            if controls > MAX_CONTROLS {
                return Err(unsupported("split curves exceed their control budget"));
            }
            let mut curve = source.edge.curve();
            let (start, end) = curve.range_tuple();
            if hi < end {
                curve.cut(hi);
            }
            if lo > start {
                curve = curve.cut(lo);
            }
            let edge = Edge::try_new(&vertices[a], &vertices[b], curve).map_err(invalid)?;
            let candidates = endpoints
                .get(&(edge.front().id(), edge.back().id()))
                .into_iter()
                .flatten()
                .chain(
                    endpoints
                        .get(&(edge.back().id(), edge.front().id()))
                        .into_iter()
                        .flatten(),
                );
            let mut matched = None;
            for &index in candidates {
                spend(work, source.controls)?;
                let other = &atoms[index].edge;
                let orientation = if other.front().id() == edge.front().id() {
                    1
                } else {
                    -1
                };
                let aligned = if orientation == 1 {
                    other.clone()
                } else {
                    other.inverse()
                };
                if crate::join::matching_edge(&aligned, &edge)? {
                    matched = Some((index, orientation));
                    break;
                }
            }
            if let Some((index, orientation)) = matched {
                atoms[index]
                    .sources
                    .push((source_index, orientation, interval));
            } else {
                if atoms.len() >= MAX_ATOMS {
                    return Err(unsupported("too many analytic section intervals"));
                }
                let segment = frame
                    .segment(&edge)?
                    .ok_or_else(|| invalid("split lost its analytic carrier"))?;
                endpoints
                    .entry((edge.front().id(), edge.back().id()))
                    .or_default()
                    .push(atoms.len());
                atoms.push(Atom {
                    edge,
                    segment,
                    sources: vec![(source_index, 1, interval)],
                });
            }
        }
    }
    Ok(Arrangement { atoms })
}

#[derive(Clone, Copy, Default)]
struct Sides {
    left: bool,
    right: bool,
}
impl Sides {
    fn sign(self) -> i8 {
        match (self.left, self.right) {
            (true, false) => 1,
            (false, true) => -1,
            _ => 0,
        }
    }
    fn combine(self, other: Self, mode: Mode) -> Self {
        let apply = |a, b| match mode {
            Mode::Union => a || b,
            Mode::Difference => a && !b,
            Mode::Intersection => a && b,
        };
        Self {
            left: apply(self.left, other.left),
            right: apply(self.right, other.right),
        }
    }
}
fn oriented(edge: &Edge, sign: i8) -> Edge {
    if sign > 0 {
        edge.clone()
    } else {
        edge.inverse()
    }
}
fn on_carrier(wires: Vec<Wire>, source: &Face, same_orientation: bool) -> Result<Face> {
    if source.orientation() == same_orientation {
        Face::try_new(wires, source.surface()).map_err(invalid)
    } else {
        Face::try_new(
            wires.into_iter().map(|wire| wire.inverse()).collect(),
            source.surface(),
        )
        .map(|face| face.inverse())
        .map_err(invalid)
    }
}
fn section(
    arrangement: &Arrangement,
    walls: &[Wall],
    frame: &Frame,
    z: f64,
    side: usize,
    work: &mut usize,
) -> Result<Vec<Sides>> {
    let mut signs = Vec::with_capacity(arrangement.atoms.len());
    let mut contour = Vec::new();
    let mut edges = Vec::new();
    for atom in &arrangement.atoms {
        spend(work, atom.sources.len())?;
        let sources = atom
            .sources
            .iter()
            .filter(|(i, _, _)| {
                let wall = &walls[*i];
                wall.side == side && wall.low < z && z < wall.high
            })
            .collect::<Vec<_>>();
        if sources.len() > 1 {
            return Err(unsupported(
                "an operand has overlapping or ambiguous material boundaries",
            ));
        }
        let sign = sources.first().map_or(0, |(_, sign, _)| *sign);
        if sign != 0 {
            let edge = oriented(&atom.edge, sign);
            contour.push(
                frame
                    .segment(&edge)?
                    .ok_or_else(|| invalid("section lost a native interval"))?,
            );
            edges.push(edge);
        }
        signs.push(sign);
    }
    // Degree-two oriented cycles and consistent outer/hole nesting are required
    // before the ray parity predicate can classify complete interval interiors.
    wire_from_parts(
        edges,
        frame,
        1.,
        &format!("prismatic Boolean operand {} section at {z} mm", side + 1),
    )?;
    arrangement
        .atoms
        .iter()
        .zip(signs)
        .map(|(atom, sign)| {
            if sign != 0 {
                return Ok(Sides {
                    left: sign > 0,
                    right: sign < 0,
                });
            }
            spend(work, contour.len())?;
            match locate(atom.segment.at(0.5), &contour)? {
                Location::Inside => Ok(Sides {
                    left: true,
                    right: true,
                }),
                Location::Outside => Ok(Sides::default()),
                Location::Boundary => Err(unsupported(
                    "coincident section intervals do not share a resolved native partition",
                )),
            }
        })
        .collect()
}

struct Topology<'a> {
    arrangement: &'a Arrangement,
    frame: &'a Frame,
    levels: &'a [f64],
    vertices: HashMap<(VertexId, usize), Vertex>,
    edges: HashMap<(EdgeId, usize), Edge>,
    uprights: HashMap<(VertexId, usize), Edge>,
}
impl Topology<'_> {
    fn vertex(&mut self, vertex: &Vertex, level: usize) -> Vertex {
        self.vertices
            .entry((vertex.id(), level))
            .or_insert_with(|| Vertex::new(vertex.point() + self.frame.normal * self.levels[level]))
            .clone()
    }
    fn edge(&mut self, index: usize, level: usize) -> Result<Edge> {
        let source = self.arrangement.atoms[index].edge.clone();
        if let Some(edge) = self.edges.get(&(source.id(), level)) {
            return Ok(edge.clone());
        }
        let a = self.vertex(source.front(), level);
        let b = self.vertex(source.back(), level);
        let translated = builder::translated(&source, self.frame.normal * self.levels[level]);
        let edge = Edge::try_new(&a, &b, translated.curve()).map_err(invalid)?;
        self.edges.insert((source.id(), level), edge.clone());
        Ok(edge)
    }
    fn upright(&mut self, vertex: &Vertex, layer: usize) -> Edge {
        if let Some(edge) = self.uprights.get(&(vertex.id(), layer)) {
            return edge.clone();
        }
        let a = self.vertex(vertex, layer);
        let b = self.vertex(vertex, layer + 1);
        let edge = builder::line(&a, &b);
        self.uprights.insert((vertex.id(), layer), edge.clone());
        edge
    }
    fn wall(
        &mut self,
        index: usize,
        layer: usize,
        sign: i8,
        carrier: &Face,
        same_orientation: bool,
    ) -> Result<Face> {
        let source = oriented(&self.arrangement.atoms[index].edge, sign);
        let lower = oriented(&self.edge(index, layer)?, sign);
        let upper = oriented(&self.edge(index, layer + 1)?, sign);
        let front = self.upright(source.front(), layer);
        let back = self.upright(source.back(), layer);
        let wire = Wire::from(vec![lower, back, upper.inverse(), front.inverse()]);
        on_carrier(vec![wire], carrier, same_orientation)
    }
}

fn construct(a: Stock, b: Stock, frame: &Frame, mode: Mode) -> Result<Output> {
    let mut caps = a.caps;
    caps.extend(b.caps);
    let mut walls = a.walls;
    walls.extend(b.walls);
    let mut levels = walls
        .iter()
        .flat_map(|wall| [wall.low, wall.high])
        .collect::<Vec<_>>();
    levels.sort_by(f64::total_cmp);
    let mut grouped: Vec<f64> = Vec::new();
    for z in levels {
        if !grouped.last().is_some_and(|last| z - *last <= EPS) {
            grouped.push(z);
        }
    }
    if grouped.len() < 2 || grouped.len() > 512 {
        return Err(unsupported("too many or collapsed height intervals"));
    }
    // Matching native plane heights are canonicalized once; no tolerance is
    // enlarged to bridge distinct geometry.
    for wall in &mut walls {
        wall.low = *grouped
            .iter()
            .find(|z| (**z - wall.low).abs() <= EPS)
            .ok_or_else(|| invalid("lower height lost its canonical plane"))?;
        wall.high = *grouped
            .iter()
            .find(|z| (**z - wall.high).abs() <= EPS)
            .ok_or_else(|| invalid("upper height lost its canonical plane"))?;
        if wall.high - wall.low <= EPS {
            return Err(unsupported(
                "a height interval collapses at native tolerance",
            ));
        }
    }
    let mut work = 0;
    let arrangement = arrangement(&walls, frame, &mut work)?;
    spend(
        &mut work,
        arrangement.atoms.len().saturating_mul(grouped.len()),
    )?;
    let mut sections = Vec::new();
    for pair in grouped.windows(2) {
        let z = pair[0] + (pair[1] - pair[0]) * 0.5;
        let left = section(&arrangement, &walls, frame, z, 0, &mut work)?;
        let right = section(&arrangement, &walls, frame, z, 1, &mut work)?;
        sections.push(
            left.into_iter()
                .zip(right)
                .map(|(a, b)| a.combine(b, mode))
                .collect::<Vec<_>>(),
        );
    }
    let mut topology = Topology {
        arrangement: &arrangement,
        frame,
        levels: &grouped,
        vertices: HashMap::new(),
        edges: HashMap::new(),
        uprights: HashMap::new(),
    };
    spend(
        &mut work,
        walls
            .len()
            .saturating_add(caps.len())
            .saturating_mul(grouped.len()),
    )?;
    let plane_sources = grouped
        .iter()
        .map(|z| {
            let caps = caps
                .iter()
                .filter(|cap| (cap.height - z).abs() <= EPS)
                .map(|cap| cap.face)
                .collect::<Vec<_>>();
            if caps.is_empty() {
                // A previous Boolean may retain a section seam without any
                // material cap at that height. Its native wall boundaries
                // still provide construction ancestry for that section.
                walls
                    .iter()
                    .filter(|wall| (wall.low - z).abs() <= EPS || (wall.high - z).abs() <= EPS)
                    .map(|wall| wall.face.id())
                    .collect()
            } else {
                caps
            }
        })
        .collect::<Vec<_>>();
    if plane_sources.iter().any(Vec::is_empty) {
        return Err(invalid("a layer has no source-plane ancestry"));
    }
    let mut faces = Vec::new();
    let mut shell = Shell::new();
    for (layer, section) in sections.iter().enumerate() {
        let z = grouped[layer] + (grouped[layer + 1] - grouped[layer]) * 0.5;
        for (index, sides) in section.iter().enumerate() {
            let sign = sides.sign();
            if sign == 0 {
                continue;
            }
            let atom = &arrangement.atoms[index];
            let (source, orientation, _) = atom
                .sources
                .iter()
                .find(|(i, _, _)| walls[*i].low < z && z < walls[*i].high)
                .ok_or_else(|| invalid("result wall has no active native carrier"))?;
            let face = topology.wall(
                index,
                layer,
                sign,
                &walls[*source].face,
                sign == *orientation,
            )?;
            faces.push((
                face.clone(),
                Descriptor {
                    kind: "wall",
                    elements: elements(atom, &walls),
                    planes: vec![
                        plane_sources[layer].clone(),
                        plane_sources[layer + 1].clone(),
                    ],
                },
            ));
            shell.push(face);
            if shell.len() > MAX_ATOMS {
                return Err(unsupported("too many output wall faces"));
            }
        }
    }
    for (level, &elevation) in grouped.iter().enumerate() {
        for upward in [true, false] {
            let below = level.checked_sub(1).and_then(|i| sections.get(i));
            let above = sections.get(level);
            let mut edges = Vec::new();
            for index in 0..arrangement.atoms.len() {
                let below = below.map_or(Sides::default(), |section| section[index]);
                let above = above.map_or(Sides::default(), |section| section[index]);
                let sides = if upward {
                    below.combine(above, Mode::Difference)
                } else {
                    above.combine(below, Mode::Difference)
                };
                let sign = sides.sign() * if upward { 1 } else { -1 };
                if sign != 0 {
                    edges.push(oriented(&topology.edge(index, level)?, sign));
                }
            }
            let cap_frame = Frame {
                origin: frame.origin + frame.normal * elevation,
                axes: frame.axes,
                normal: frame.normal,
            };
            for wires in wire_from_parts(
                edges,
                &cap_frame,
                if upward { 1. } else { -1. },
                &format!(
                    "prismatic Boolean {} transition at {elevation} mm",
                    if upward { "upper" } else { "lower" },
                ),
            )? {
                spend(&mut work, arrangement.atoms.len())?;
                // A transition can combine or extend several coplanar source
                // regions. Reusing an arbitrary source Plane would falsely
                // inherit that one trimmed face's ancestry via carrier equality.
                let mut surface = Surface::Plane(Plane::new(
                    cap_frame.origin,
                    cap_frame.origin + frame.axes[0],
                    cap_frame.origin + frame.axes[1],
                ));
                if !upward {
                    surface.invert();
                }
                let ids = wires.iter().flatten().map(Edge::id).collect::<HashSet<_>>();
                let mut contributors = Vec::new();
                for (index, atom) in arrangement.atoms.iter().enumerate() {
                    if topology
                        .edges
                        .get(&(atom.edge.id(), level))
                        .is_some_and(|edge| ids.contains(&edge.id()))
                    {
                        contributors.extend(elements(&arrangement.atoms[index], &walls));
                    }
                }
                let face = Face::try_new(wires, surface).map_err(invalid)?;
                faces.push((
                    face.clone(),
                    Descriptor {
                        kind: if upward {
                            "upper_transition"
                        } else {
                            "lower_transition"
                        },
                        elements: contributors,
                        planes: vec![plane_sources[level].clone()],
                    },
                ));
                shell.push(face);
                if shell.len() > MAX_ATOMS {
                    return Err(unsupported("too many output faces"));
                }
            }
        }
    }
    if shell.is_empty() {
        return Ok(Output {
            solid: None,
            faces: vec![],
            edges: vec![],
        });
    }
    // Topological closure also rejects line-only contacts between disjoint
    // material components; such a union has no manifold boundary.
    let solid = Solid::try_new(shell.connected_components()).map_err(invalid)?;
    crate::validation::solid(&solid)?;
    let mut vertex_elements: HashMap<VertexId, Vec<(FaceId, usize, i8)>> = HashMap::new();
    let mut atoms = HashMap::new();
    for atom in &arrangement.atoms {
        atoms.insert(atom.edge.id(), atom);
        for (wall, orientation, interval) in &atom.sources {
            let front = if *orientation > 0 { 1 } else { 2 };
            vertex_elements
                .entry(atom.edge.front().id())
                .or_default()
                .push((walls[*wall].face.id(), *interval, front));
            vertex_elements
                .entry(atom.edge.back().id())
                .or_default()
                .push((walls[*wall].face.id(), *interval, 3 - front));
        }
    }
    let used = solid
        .edge_iter()
        .map(|edge| edge.id())
        .collect::<HashSet<_>>();
    let mut edges = Vec::new();
    for ((atom, level), edge) in topology.edges {
        if used.contains(&edge.id()) {
            edges.push((
                edge,
                Descriptor {
                    kind: "section_interval",
                    elements: elements(atoms[&atom], &walls),
                    planes: vec![plane_sources[level].clone()],
                },
            ));
        }
    }
    for ((vertex, layer), edge) in topology.uprights {
        if used.contains(&edge.id()) {
            edges.push((
                edge,
                Descriptor {
                    kind: "generator",
                    elements: vertex_elements[&vertex].clone(),
                    planes: vec![
                        plane_sources[layer].clone(),
                        plane_sources[layer + 1].clone(),
                    ],
                },
            ));
        }
    }
    Ok(Output {
        solid: Some(solid),
        faces,
        edges,
    })
}

/// `None` means a different native backend is needed. An output with no solid is
/// a proven empty set. Errors after recognition never use sampled geometry.
pub(crate) fn binary(a: &Solid, b: &Solid, mode: Mode) -> Result<Option<Output>> {
    if a.face_iter().count().saturating_add(b.face_iter().count()) > MAX_FACES {
        return Ok(None);
    }
    let mut axes: Vec<Vector3> = Vec::new();
    let mut seen = HashSet::new();
    for face in a.face_iter().chain(b.face_iter()) {
        if !seen.insert(face.id()) {
            continue;
        }
        let Surface::Plane(_) = face.surface() else {
            continue;
        };
        let mut axis = face.oriented_surface().normal(0., 0.).normalize();
        if !axis.magnitude2().is_finite() {
            continue;
        }
        let major = (0..3)
            .max_by(|a, b| axis[*a].abs().total_cmp(&axis[*b].abs()))
            .unwrap_or(2);
        if axis[major] < 0. {
            axis = -axis;
        }
        if !axes
            .iter()
            .any(|other| other.cross(axis).magnitude() <= 1e-10)
        {
            axes.push(axis);
        }
        if axes.len() == 32 {
            break;
        }
    }
    let Some(origin) = a.vertex_iter().next().map(|vertex| vertex.point()) else {
        return Ok(None);
    };
    for normal in axes {
        let u = normal
            .cross(if normal.x.abs() < 0.8 {
                Vector3::unit_x()
            } else {
                Vector3::unit_y()
            })
            .normalize();
        let frame = Frame {
            origin,
            axes: [u, normal.cross(u)],
            normal,
        };
        let Some(left) = recognize(a, 0, &frame)? else {
            continue;
        };
        let Some(right) = recognize(b, 1, &frame)? else {
            continue;
        };
        return construct(left, right, &frame, mode).map(Some);
    }
    Ok(None)
}
