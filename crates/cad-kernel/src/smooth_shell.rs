//! Uniform exact normal offsets of fully rounded convex polyhedra.
//!
//! Each closed source boundary must be P + B(r), where P is a bounded convex
//! polyhedron and B(r) is a Euclidean ball. Every core facet, edge and vertex
//! has exactly one complete planar, cylindrical or spherical patch. Changing
//! r certifies injectivity, including normal strips around removed faces.
//! Multiple boundaries additionally require complete outer/cavity containment
//! and pairwise cavity separation throughout the source-to-offset collars.
use crate::{
    analytic_offset::{self, Carrier, EPS},
    shell_history::ShellHistory,
};
use cad_core::{CadError, Result};
use monstertruck::modeling::*;
use std::collections::{HashMap, HashSet};

const REL: f64 = 1e-10;
const MAX_FACETS: usize = 64;
const MAX_VERTICES: usize = 2048;
const MAX_BOUNDARIES: usize = 32;
const MAX_RELATION_WORK: usize = 8_000_000;

fn invalid(message: &str) -> CadError {
    CadError::Invalid(format!("Rounded shell: {message}"))
}
fn unsupported(message: &str) -> CadError {
    CadError::Unsupported(format!("Rounded shell: {message}"))
}
fn finite(p: Point3) -> bool {
    [p.x, p.y, p.z].iter().all(|x| x.is_finite())
}
fn pair(a: usize, b: usize) -> (usize, usize) {
    if a < b { (a, b) } else { (b, a) }
}
#[derive(Clone)]
struct Facet {
    source: FaceId,
    origin: Point3,
    normal: Vector3,
    vertices: Vec<usize>,
}
impl Facet {
    fn signed(&self, p: Point3) -> f64 {
        (p - self.origin).dot(self.normal)
    }
}
struct Core {
    facets: Vec<Facet>,
    vertices: Vec<Point3>,
    edges: HashMap<(usize, usize), [usize; 2]>,
}
impl Core {
    fn recognize(mut facets: Vec<Facet>) -> Option<Self> {
        if !(4..=MAX_FACETS).contains(&facets.len()) {
            return None;
        }
        // A full-dimensional recession cone has an extreme ray at the
        // intersection of two support planes. Excluding both directions for
        // every nonparallel normal pair proves that P is bounded. The strict
        // angular margin rejects numerically unresolved recession directions.
        let mut spans_space = false;
        for i in 0..facets.len() {
            for j in i + 1..facets.len() {
                let cross = facets[i].normal.cross(facets[j].normal);
                let length = cross.magnitude();
                if length <= REL {
                    continue;
                }
                let ray = cross / length;
                let mut positive = false;
                let mut negative = false;
                for facet in &facets {
                    let dot = facet.normal.dot(ray);
                    positive |= dot > REL;
                    negative |= dot < -REL;
                }
                if !positive || !negative {
                    return None;
                }
                spans_space = true;
            }
        }
        if !spans_space {
            return None;
        }
        let anchor = facets[0].origin;
        let mut vertices: Vec<Point3> = Vec::new();
        for i in 0..facets.len() {
            for j in i + 1..facets.len() {
                for k in j + 1..facets.len() {
                    let [a, b, c] = [&facets[i], &facets[j], &facets[k]];
                    let determinant = a.normal.dot(b.normal.cross(c.normal));
                    if determinant.abs() <= REL {
                        continue;
                    }
                    let p = anchor
                        + (b.normal.cross(c.normal) * (a.origin - anchor).dot(a.normal)
                            + c.normal.cross(a.normal) * (b.origin - anchor).dot(b.normal)
                            + a.normal.cross(b.normal) * (c.origin - anchor).dot(c.normal))
                            / determinant;
                    if !finite(p) {
                        return None;
                    }
                    if !facets.iter().all(|f| f.signed(p) <= EPS) {
                        continue;
                    }
                    let matches = vertices.iter().filter(|q| p.distance(**q) <= EPS).count();
                    if matches > 1 {
                        return None;
                    }
                    if matches == 0 {
                        vertices.push(p);
                        if vertices.len() > MAX_VERTICES {
                            return None;
                        }
                    }
                }
            }
        }
        if vertices.len() < 4 {
            return None;
        }
        let interior = Point3::from_vec(
            vertices.iter().map(|p| p.to_vec()).sum::<Vector3>() / vertices.len() as f64,
        );
        if !finite(interior) || facets.iter().any(|f| f.signed(interior) >= -EPS) {
            return None;
        }
        let mut adjacency: HashMap<(usize, usize), Vec<usize>> = HashMap::new();
        for (index, facet) in facets.iter_mut().enumerate() {
            facet.vertices = vertices
                .iter()
                .enumerate()
                .filter_map(|(id, p)| (facet.signed(*p).abs() <= EPS).then_some(id))
                .collect();
            if facet.vertices.len() < 3 {
                return None;
            }
            let center = Point3::from_vec(
                facet
                    .vertices
                    .iter()
                    .map(|i| vertices[*i].to_vec())
                    .sum::<Vector3>()
                    / facet.vertices.len() as f64,
            );
            let delta = vertices[facet.vertices[0]] - center;
            if !delta.magnitude().is_finite() || delta.magnitude() <= EPS {
                return None;
            }
            let x = delta.normalize();
            let y = facet.normal.cross(x);
            facet.vertices.sort_by(|a, b| {
                let a = vertices[*a] - center;
                let b = vertices[*b] - center;
                a.dot(y)
                    .atan2(a.dot(x))
                    .total_cmp(&b.dot(y).atan2(b.dot(x)))
            });
            let n = facet.vertices.len();
            for i in 0..n {
                let a = vertices[facet.vertices[i]];
                let b = vertices[facet.vertices[(i + 1) % n]];
                let c = vertices[facet.vertices[(i + 2) % n]];
                if (b - a).cross(c - b).dot(facet.normal) <= EPS * EPS {
                    return None;
                }
                adjacency
                    .entry(pair(facet.vertices[i], facet.vertices[(i + 1) % n]))
                    .or_default()
                    .push(index);
            }
        }
        if vertices.len() + facets.len() != adjacency.len() + 2 {
            return None;
        }
        let edges = adjacency
            .into_iter()
            .map(|(edge, faces)| match faces.as_slice() {
                [a, b] if a != b => Some((edge, [*a, *b])),
                _ => None,
            })
            .collect::<Option<HashMap<_, _>>>()?;
        Some(Self {
            facets,
            vertices,
            edges,
        })
    }
    fn vertex(&self, point: Point3) -> Option<usize> {
        let mut matches = self
            .vertices
            .iter()
            .enumerate()
            .filter_map(|(i, p)| (p.distance(point) <= EPS).then_some(i));
        let first = matches.next()?;
        matches.next().is_none().then_some(first)
    }
    fn active(&self, vertex: usize) -> Vec<usize> {
        self.facets
            .iter()
            .enumerate()
            .filter_map(|(index, facet)| facet.vertices.contains(&vertex).then_some(index))
            .collect()
    }
}

struct Envelope {
    radius: f64,
    carriers: HashMap<FaceId, Carrier>,
    core: Core,
}
fn single_wire(face: &Face) -> Option<Wire> {
    let wires = face.boundaries();
    (wires.len() == 1 && wires[0].is_closed()).then(|| wires[0].clone())
}
fn cyclic_equal(actual: &[usize], expected: &[usize]) -> bool {
    actual.len() == expected.len()
        && (0..expected.len()).any(|start| {
            actual
                .iter()
                .enumerate()
                .all(|(i, id)| *id == expected[(i + start) % expected.len()])
        })
}
fn planar_patch(face: &Face, facet: &Facet, core: &Core, radius: f64) -> bool {
    let Some(wire) = single_wire(face) else {
        return false;
    };
    let Some(vertices) = wire
        .iter()
        .map(|edge| {
            crate::bounded_carriers::line(&edge.oriented_curve()).ok()?;
            core.vertex(edge.front().point() - facet.normal * radius)
        })
        .collect::<Option<Vec<_>>>()
    else {
        return false;
    };
    cyclic_equal(&vertices, &facet.vertices)
}
fn cylindrical_patch(
    face: &Face,
    edge: (usize, usize),
    adjacent: [usize; 2],
    core: &Core,
    radius: f64,
) -> bool {
    let Some(wire) = single_wire(face) else {
        return false;
    };
    if wire.len() != 4 {
        return false;
    }
    let [start, end] = [core.vertices[edge.0], core.vertices[edge.1]];
    let delta = end - start;
    let length = delta.magnitude();
    if !length.is_finite() || length <= EPS {
        return false;
    }
    let axis = delta / length;
    let normals = adjacent.map(|i| core.facets[i].normal);
    if normals[0].dot(normals[1]).abs() >= 1. - REL {
        return false;
    }
    let mut lines = [false; 2];
    let mut arcs = [false; 2];
    for boundary in wire {
        let curve = boundary.oriented_curve();
        if crate::bounded_carriers::line(&curve).is_ok() {
            let p = boundary.front().point();
            let q = boundary.back().point();
            let Some(side) = (0..2).find(|i| {
                let a = start + normals[*i] * radius;
                let b = end + normals[*i] * radius;
                (p.distance(a) <= EPS && q.distance(b) <= EPS)
                    || (p.distance(b) <= EPS && q.distance(a) <= EPS)
            }) else {
                return false;
            };
            if lines[side] {
                return false;
            }
            lines[side] = true;
        } else {
            let Ok(((center, normal, r), sweep)) = crate::bounded_carriers::circle(&curve) else {
                return false;
            };
            let Some(at) = [start, end].iter().position(|p| p.distance(center) <= EPS) else {
                return false;
            };
            if arcs[at]
                || (r - radius).abs() > EPS
                || normal.dot(axis).abs() < 1. - REL
                || sweep.abs() >= std::f64::consts::PI - REL
                || sweep * normal.dot(axis) * if at == 0 { 1. } else { -1. } <= REL
            {
                return false;
            }
            let p = boundary.front().point();
            let q = boundary.back().point();
            let a = center + normals[0] * radius;
            let b = center + normals[1] * radius;
            if !((p.distance(a) <= EPS && q.distance(b) <= EPS)
                || (p.distance(b) <= EPS && q.distance(a) <= EPS))
            {
                return false;
            }
            arcs[at] = true;
        }
    }
    lines.iter().chain(arcs.iter()).all(|present| *present)
}
fn spherical_patch(face: &Face, vertex: usize, core: &Core, radius: f64) -> bool {
    let Some(wire) = single_wire(face) else {
        return false;
    };
    let active = core.active(vertex);
    if active.len() < 3 || wire.len() != active.len() {
        return false;
    }
    let center = core.vertices[vertex];
    let normals = active
        .iter()
        .map(|i| core.facets[*i].normal)
        .collect::<Vec<_>>();
    let interior = Point3::from_vec(
        core.vertices.iter().map(|p| p.to_vec()).sum::<Vector3>() / core.vertices.len() as f64,
    );
    let hemisphere = (center - interior).normalize();
    if normals.iter().any(|n| n.dot(hemisphere) <= REL) {
        return false;
    }
    let mut seen = HashSet::new();
    for edge in wire {
        let p = edge.front().point();
        let q = edge.back().point();
        let Some(from) = normals
            .iter()
            .position(|n| (center + n * radius).distance(p) <= EPS)
        else {
            return false;
        };
        let Some(to) = normals
            .iter()
            .position(|n| (center + n * radius).distance(q) <= EPS)
        else {
            return false;
        };
        if from == to || !seen.insert(from) {
            return false;
        }
        let Ok(((circle_center, normal, r), sweep)) =
            crate::bounded_carriers::circle(&edge.oriented_curve())
        else {
            return false;
        };
        if circle_center.distance(center) > EPS
            || (r - radius).abs() > EPS
            || sweep.abs() >= std::f64::consts::PI - REL
        {
            return false;
        }
        let cross = normals[from].cross(normals[to]);
        if cross.magnitude() <= REL {
            return false;
        }
        let left = cross.normalize();
        if normal.dot(left) * sweep <= REL
            || normals.iter().any(|n| n.dot(left) < -REL)
            || !normals.iter().any(|n| n.dot(left) > REL)
        {
            return false;
        }
    }
    seen.len() == active.len()
}
impl Envelope {
    fn recognize(solid: &Solid) -> Result<Option<Self>> {
        if solid.boundaries().len() != 1 {
            return Ok(None);
        }
        let mut carriers = HashMap::new();
        let mut radius: Option<f64> = None;
        let mut facets = Vec::new();
        for face in solid.face_iter() {
            let carrier = match analytic_offset::recognize(face)? {
                Some(carrier) => carrier,
                None => match crate::planar::boundary_plane(face)? {
                    Some(plane) => Carrier::Plane {
                        origin: plane.origin,
                        normal: plane.normal,
                    },
                    None => return Ok(None),
                },
            };
            let carrier = match carrier {
                Carrier::Plane { origin, normal } => Carrier::Plane {
                    origin,
                    normal: normal * analytic_offset::side(face, carrier)?,
                },
                Carrier::Sphere { radius: r, .. }
                | Carrier::Cone {
                    radius: r,
                    slope: 0.,
                    ..
                } => {
                    if !r.is_finite()
                        || r <= EPS
                        || analytic_offset::side(face, carrier)? < 0.
                        || radius.is_some_and(|old| (old - r).abs() > EPS)
                    {
                        return Ok(None);
                    }
                    radius = Some(r);
                    carrier
                }
                _ => return Ok(None),
            };
            carriers.insert(face.id(), carrier);
        }
        let Some(radius) = radius else {
            return Ok(None);
        };
        for face in solid.face_iter() {
            if let Carrier::Plane { origin, normal } = carriers[&face.id()] {
                if facets
                    .iter()
                    .any(|f: &Facet| f.normal.dot(normal) > 1. - REL)
                {
                    return Ok(None);
                }
                facets.push(Facet {
                    source: face.id(),
                    origin: origin - normal * radius,
                    normal,
                    vertices: Vec::new(),
                });
            }
        }
        let Some(core) = Core::recognize(facets) else {
            return Ok(None);
        };
        let mut cylinders = HashSet::new();
        let mut spheres = HashSet::new();
        for face in solid.face_iter() {
            match carriers[&face.id()] {
                Carrier::Plane { .. } => {
                    let Some(facet) = core.facets.iter().find(|f| f.source == face.id()) else {
                        return Ok(None);
                    };
                    if !planar_patch(face, facet, &core, radius) {
                        return Ok(None);
                    }
                }
                Carrier::Cone { origin, axis, .. } => {
                    let Some((&edge, &adjacent)) = core.edges.iter().find(|(edge, _)| {
                        [core.vertices[edge.0], core.vertices[edge.1]]
                            .iter()
                            .all(|p| (*p - origin).cross(axis).magnitude() <= EPS)
                    }) else {
                        return Ok(None);
                    };
                    if !cylinders.insert(edge)
                        || !cylindrical_patch(face, edge, adjacent, &core, radius)
                    {
                        return Ok(None);
                    }
                }
                Carrier::Sphere { center, .. } => {
                    let Some(vertex) = core.vertex(center) else {
                        return Ok(None);
                    };
                    if !spheres.insert(vertex) || !spherical_patch(face, vertex, &core, radius) {
                        return Ok(None);
                    }
                }
                _ => unreachable!(),
            }
        }
        if cylinders.len() != core.edges.len() || spheres.len() != core.vertices.len() {
            return Ok(None);
        }
        crate::validation::solid(solid)?;
        Ok(Some(Self {
            radius,
            carriers,
            core,
        }))
    }
    fn offset(&self, shell: &Shell, distance: f64) -> Result<analytic_offset::Offset> {
        if !distance.is_finite()
            || !(self.radius + distance).is_finite()
            || self.radius + distance <= EPS
        {
            return Err(invalid(
                "Thickness reaches a rounded edge or corner focal locus; reduce thickness below the fillet radius",
            ));
        }
        let mut vertices: HashMap<VertexId, Vertex> = HashMap::new();
        let mut edges: HashMap<EdgeId, Edge> = HashMap::new();
        for face in shell {
            let carrier = self.carriers[&face.id()];
            let transform = carrier
                .affine(distance)?
                .ok_or_else(|| invalid("Missing affine normal map"))?;
            for edge in face.edge_iter() {
                for vertex in [edge.absolute_front(), edge.absolute_back()] {
                    let point = carrier.offset_point(vertex.point(), distance)?;
                    if let Some(old) = vertices.get(&vertex.id()) {
                        if old.point().distance(point) > EPS {
                            return Err(unsupported(
                                "Adjacent carriers do not share a tangent normal at their common vertex",
                            ));
                        }
                    } else {
                        vertices.insert(vertex.id(), builder::vertex(point));
                    }
                }
                let curve = edge.curve().transformed(transform);
                if let Some(old) = edges.get(&edge.id()) {
                    // Complete equal-denominator controls prove coincident maps
                    // along the whole native boundary, not just its endpoints.
                    let (a, b) = crate::loft_correspondence::compatible(&old.curve(), &curve)?;
                    if a.control_points()
                        .iter()
                        .zip(b.control_points())
                        .any(|(a, b)| {
                            let pa = Point3::from_vec(a.truncate() / a.w);
                            let pb = Point3::from_vec(b.truncate() / b.w);
                            !finite(pa) || !finite(pb) || pa.distance(pb) > EPS
                        })
                    {
                        return Err(unsupported(
                            "Adjacent offset carriers do not share the complete tangent boundary",
                        ));
                    }
                } else {
                    edges.insert(
                        edge.id(),
                        Edge::try_new(
                            &vertices[&edge.absolute_front().id()],
                            &vertices[&edge.absolute_back().id()],
                            curve,
                        )
                        .map_err(|e| invalid(&e.to_string()))?,
                    );
                }
            }
        }
        let mut mapped = Shell::new();
        let mut faces = HashMap::new();
        for face in shell {
            let boundaries = face
                .boundaries()
                .into_iter()
                .map(|wire| {
                    wire.iter()
                        .map(|edge| {
                            if edge.orientation() {
                                edges[&edge.id()].clone()
                            } else {
                                edges[&edge.id()].inverse()
                            }
                        })
                        .collect()
                })
                .collect();
            let surface =
                self.carriers[&face.id()].offset_surface(&face.oriented_surface(), distance)?;
            let output = Face::try_new(boundaries, surface).map_err(|e| invalid(&e.to_string()))?;
            faces.insert(face.id(), output.id());
            mapped.push(output);
        }
        crate::validation::surface(&mapped)?;
        Ok(analytic_offset::Offset {
            shell: mapped,
            edges,
            faces,
        })
    }
}

struct Work(usize);
impl Work {
    fn charge(&mut self, count: usize) -> Result<()> {
        self.0 = self
            .0
            .checked_add(count)
            .filter(|n| *n <= MAX_RELATION_WORK)
            .ok_or_else(|| {
                unsupported("Rounded cavity relation proof exceeds its native work budget")
            })?;
        Ok(())
    }
}
impl Core {
    /// Signed Euclidean distance to the convex core. Inside, the nearest
    /// supporting plane gives the exact negative distance. Outside, every
    /// possible closest facet interior/edge/vertex is considered. If floating
    /// arithmetic cannot establish facet membership, excluding that candidate
    /// gives a conservative upper bound, never an underestimated corner gap.
    fn signed_distance(&self, point: Point3, work: &mut Work) -> Result<f64> {
        work.charge(self.facets.len())?;
        let mut highest = f64::NEG_INFINITY;
        for facet in &self.facets {
            let distance = facet.signed(point);
            if !distance.is_finite() {
                return Err(invalid("Core distance exceeds finite coordinates"));
            }
            highest = highest.max(distance);
        }
        if highest <= 0. {
            return Ok(highest);
        }
        let mut closest = f64::INFINITY;
        for facet in &self.facets {
            work.charge(self.facets.len())?;
            let projected = point - facet.normal * facet.signed(point);
            if finite(projected) && self.facets.iter().all(|f| f.signed(projected) <= 0.) {
                closest = closest.min(point.distance(projected));
            }
        }
        for &(a, b) in self.edges.keys() {
            work.charge(1)?;
            let start = self.vertices[a];
            let delta = self.vertices[b] - start;
            let fraction = ((point - start).dot(delta) / delta.magnitude2()).clamp(0., 1.);
            let projected = start + delta * fraction;
            let distance = point.distance(projected);
            if !fraction.is_finite() || !distance.is_finite() {
                return Err(invalid("Core edge distance exceeds finite coordinates"));
            }
            closest = closest.min(distance);
        }
        if !closest.is_finite() {
            return Err(invalid("Core distance has no finite bound"));
        }
        Ok(closest)
    }
    fn projection(
        &self,
        origin: Point3,
        direction: Vector3,
        work: &mut Work,
    ) -> Result<(f64, f64)> {
        work.charge(self.vertices.len())?;
        let mut low = f64::INFINITY;
        let mut high = f64::NEG_INFINITY;
        for point in &self.vertices {
            let value = (*point - origin).dot(direction);
            if !value.is_finite() {
                return Err(invalid("Cavity support exceeds finite coordinates"));
            }
            low = low.min(value);
            high = high.max(value);
        }
        Ok((low, high))
    }
    fn separated(&self, other: &Self, radius_sum: f64, work: &mut Work) -> Result<bool> {
        if !radius_sum.is_finite() {
            return Err(invalid("Cavity radius sum exceeds finite coordinates"));
        }
        let origin = self.vertices[0];
        let axis_separates = |direction: Vector3, work: &mut Work| -> Result<bool> {
            let length = direction.magnitude();
            if !length.is_finite() {
                return Err(invalid("Cavity separating axis is nonfinite"));
            }
            if length <= REL {
                return Ok(false);
            }
            let direction = direction / length;
            let (a, b) = self.projection(origin, direction, work)?;
            let (c, d) = other.projection(origin, direction, work)?;
            let gap = (c - b).max(a - d);
            if !gap.is_finite() {
                return Err(invalid("Cavity separation exceeds finite coordinates"));
            }
            Ok(gap > radius_sum + EPS)
        };
        for facet in self.facets.iter().chain(&other.facets) {
            if axis_separates(facet.normal, work)? {
                return Ok(true);
            }
        }
        for &(a, b) in self.edges.keys() {
            let first = (self.vertices[b] - self.vertices[a]).normalize();
            for &(c, d) in other.edges.keys() {
                let second = (other.vertices[d] - other.vertices[c]).normalize();
                if axis_separates(first.cross(second), work)? {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }
}
struct Boundary {
    /// Positive envelope geometry, even for an authored cavity boundary.
    shell: Shell,
    envelope: Envelope,
    outer: bool,
}
impl Boundary {
    fn recognize(shell: &Shell) -> Result<Option<Self>> {
        let Ok(positive) = Solid::try_new(vec![shell.clone()]) else {
            return Ok(None);
        };
        if let Some(envelope) = Envelope::recognize(&positive)? {
            return Ok(Some(Self {
                shell: shell.clone(),
                envelope,
                outer: true,
            }));
        }
        let reversed: Shell = shell.inverse().into();
        let Ok(positive) = Solid::try_new(vec![reversed.clone()]) else {
            return Ok(None);
        };
        Ok(Envelope::recognize(&positive)?.map(|envelope| Self {
            shell: reversed,
            envelope,
            outer: false,
        }))
    }
}
struct Material {
    boundaries: Vec<Boundary>,
    outside: usize,
}
impl Material {
    fn recognize(solid: &Solid) -> Result<Option<Self>> {
        if solid.boundaries().is_empty() || solid.boundaries().len() > MAX_BOUNDARIES {
            return Ok(None);
        }
        let mut boundaries = Vec::new();
        let mut outside = None;
        for shell in solid.boundaries() {
            let Some(boundary) = Boundary::recognize(shell)? else {
                return Ok(None);
            };
            if boundary.outer {
                if outside.is_some() {
                    return Ok(None);
                }
                outside = Some(boundaries.len());
            }
            boundaries.push(boundary);
        }
        let Some(outside) = outside else {
            return Ok(None);
        };
        Ok(Some(Self {
            boundaries,
            outside,
        }))
    }
    fn certify(&self, distance: f64) -> Result<()> {
        let mut radii = Vec::new();
        for boundary in &self.boundaries {
            let radius = boundary.envelope.radius;
            let next = radius + if boundary.outer { distance } else { -distance };
            if !next.is_finite() || next <= EPS {
                return Err(invalid(
                    "Thickness reaches an outer or cavity rounded focal locus",
                ));
            }
            // The entire source-to-offset collar must stay clear of every
            // other boundary, not merely its final surface.
            radii.push(if boundary.outer {
                radius.min(next)
            } else {
                radius.max(next)
            });
        }
        let outside = &self.boundaries[self.outside].envelope.core;
        let mut work = Work(0);
        for (i, boundary) in self.boundaries.iter().enumerate() {
            if i == self.outside {
                continue;
            }
            for vertex in &boundary.envelope.core.vertices {
                let distance = outside.signed_distance(*vertex, &mut work)?;
                let bound = distance + radii[i];
                if !bound.is_finite() || bound >= radii[self.outside] - EPS {
                    return Err(unsupported(
                        "Thickness makes a cavity approach the outer offset wall, or complete rounded containment cannot be certified",
                    ));
                }
            }
            for (j, other) in self.boundaries.iter().enumerate().take(i) {
                if j != self.outside
                    && !boundary.envelope.core.separated(
                        &other.envelope.core,
                        radii[i] + radii[j],
                        &mut work,
                    )?
                {
                    return Err(unsupported(
                        "Expanded cavity walls collide or their complete rounded envelopes have no certified separating axis",
                    ));
                }
            }
        }
        Ok(())
    }
}

/// A closed surface envelope has the same global normal-map certificate as a
/// rounded solid. Open mixed-carrier sheets need their own support certificate.
pub(crate) fn offset(shell: &Shell, distance: f64) -> Result<Option<analytic_offset::Offset>> {
    let Some(boundary) = Boundary::recognize(shell)? else {
        return Ok(None);
    };
    boundary
        .envelope
        .offset(shell, if boundary.outer { distance } else { -distance })
        .map(Some)
}

pub(crate) fn shell(
    solid: &Solid,
    thickness: f64,
    removed: &[FaceId],
    outward: bool,
    overrides: &HashMap<FaceId, f64>,
    history: &mut ShellHistory,
) -> Result<Option<Vec<Solid>>> {
    let Some(material) = Material::recognize(solid)? else {
        return Ok(None);
    };
    if !thickness.is_finite() || thickness <= EPS {
        return Err(invalid("Thickness must be finite and positive"));
    }
    if overrides.iter().any(|(face, t)| {
        !removed.contains(face) && (!t.is_finite() || *t <= EPS || (*t - thickness).abs() > EPS)
    }) {
        return Err(unsupported(
            "Tangent rounded walls require uniform normal thickness; unequal offsets need new transition blends",
        ));
    }
    material.certify(if outward { thickness } else { -thickness })?;
    let mut results = Vec::new();
    for boundary in &material.boundaries {
        let openings = removed
            .iter()
            .filter(|id| boundary.shell.iter().any(|f| f.id() == **id))
            .copied()
            .collect::<Vec<_>>();
        if openings.len() == boundary.shell.len() {
            return Err(unsupported(
                "Removing every face of a complete outer or cavity boundary requires material reclassification; retain at least one wall on each boundary",
            ));
        }
        results.extend(construct_shell(
            &boundary.envelope,
            &boundary.shell,
            thickness,
            &openings,
            outward == boundary.outer,
            history,
        )?);
    }
    if results.is_empty() {
        return Err(invalid(
            "At least one wall must remain after removing faces",
        ));
    }
    Ok(Some(results))
}
fn construct_shell(
    envelope: &Envelope,
    shell: &Shell,
    thickness: f64,
    removed: &[FaceId],
    outward: bool,
    history: &mut ShellHistory,
) -> Result<Vec<Solid>> {
    let mut retained = shell
        .iter()
        .filter(|face| !removed.contains(&face.id()))
        .cloned()
        .collect::<Shell>();
    if retained.is_empty() {
        return Err(invalid(
            "At least one wall must remain after removing faces",
        ));
    }
    let offset = envelope.offset(&retained, if outward { thickness } else { -thickness })?;
    for face in &retained {
        history.construct_face(
            offset.faces[&face.id()],
            &[history.face(face.id())?],
            serde_json::json!({"operation":"shell","constructor":"convex_rounded_normal_offset"}),
        )?;
    }
    for (source, edge) in &offset.edges {
        history.construct_edge(
            edge.id(),
            &[history.edge(*source)?],
            serde_json::json!({"operation":"shell","constructor":"tangent_normal_boundary"}),
        )?;
    }
    let exposed = retained.extract_boundaries();
    if outward {
        retained.invert();
        retained.extend(offset.shell.iter().cloned());
    } else {
        retained.extend(offset.shell.inverse());
    }
    for wire in exposed {
        let changed = wire
            .iter()
            .map(|edge| {
                if edge.orientation() {
                    offset.edges[&edge.id()].clone()
                } else {
                    offset.edges[&edge.id()].inverse()
                }
            })
            .collect::<Wire>();
        let rim = crate::loft_correspondence::skin(&wire, &changed, None, None)?;
        for (edge, face) in wire.iter().zip(rim.iter()) {
            let face = if outward {
                face.clone()
            } else {
                face.inverse()
            };
            history.construct_face(
                face.id(),
                &[history.edge(edge.id())?],
                serde_json::json!({"operation":"shell","constructor":"convex_normal_strip_rim"}),
            )?;
            retained.push(face);
        }
    }
    let components = retained.connected_components();
    let results = if removed.is_empty() {
        vec![Solid::try_new(components).map_err(|e| invalid(&e.to_string()))?]
    } else {
        components
            .into_iter()
            .map(|shell| Solid::try_new(vec![shell]).map_err(|e| invalid(&e.to_string())))
            .collect::<Result<Vec<_>>>()?
    };
    for result in &results {
        crate::validation::solid(result)?;
    }
    Ok(results)
}
