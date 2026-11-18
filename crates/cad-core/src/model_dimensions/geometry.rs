//! Finished dimensions use only native trimmed-face witnesses and incidence.
//! The display mesh carries these records, but its triangles and visual bounds
//! never supply a nominal size or select a manufacturing reference.
mod chamfers;
mod cones;
mod contour_corners;
mod drills;

use super::{Characteristic, Kind};
use crate::{
    Id, NodeRef, Operation, Part, Result, TopologyRef,
    protocol::{BodyKind, FaceRange, MeshData, NativeCylinder},
};
use nalgebra::Vector3;
use serde_json::Value;
use std::{
    cell::Cell,
    collections::{BTreeMap, BTreeSet},
};

type Point = [f64; 3];
type Vector = Vector3<f64>;
const AXES: [&str; 3] = ["x", "y", "z"];
const MAX_VISITS: usize = 250_000;
const MAX_SOURCE_KEY: usize = 128 * 1024;
const MAX_REPORT_KEYS: usize = 4 * 1024 * 1024;

fn vec(p: Point) -> Vector {
    Vector::from(p)
}
fn normal(f: &FaceRange) -> Option<Vector> {
    let plane = f.plane.as_ref()?;
    vec(plane.u).cross(&vec(plane.v)).try_normalize(1e-12)
}
fn aligned(a: Vector, b: Vector) -> bool {
    a.cross(&b).norm() <= 1e-9
}
fn world_axis(dir: Vector) -> Option<usize> {
    (0..3).find(|&i| aligned(dir, Vector::ith(i, 1.)))
}
fn anchor(f: &FaceRange) -> Option<Vector> {
    Some(vec(f.measurement.as_ref()?.witnesses[0][2]))
}

fn serialize_identity(family: &str, values: Vec<Value>) -> Result<String> {
    // Sort after re-keying too. Component copying may change both UUIDs and
    // their relative order without changing the measured relationship.
    let mut keys = values
        .into_iter()
        .map(|v| serde_json::to_string(&v))
        .collect::<std::result::Result<Vec<_>, _>>()?;
    keys.sort();
    keys.dedup();
    Ok(format!("{family}|[{}]", keys.join(",")))
}

pub fn remap_id(id: &str, mapping: &BTreeMap<Id, Id>) -> Result<String> {
    let Some((family, encoded)) = id.split_once('|') else {
        return Ok(id.to_owned());
    };
    let mut values: Vec<Value> = serde_json::from_str(encoded)?;
    for value in &mut values {
        crate::library::remap_source(value, mapping, "")?;
    }
    serialize_identity(family, values)
}

#[derive(Clone)]
struct Extent {
    low: f64,
    high: f64,
    low_faces: Vec<usize>,
}

/// All attained high/low witnesses are on the finished solid. An unknown or
/// loose face enclosure suppresses that axis if it could hide a larger extent.
fn overall(faces: &[&FaceRange], axis: usize, tolerance: f64) -> Option<(Extent, Characteristic)> {
    let mut enclosure = [f64::INFINITY, f64::NEG_INFINITY];
    let mut reached = [f64::INFINITY, f64::NEG_INFINITY];
    for face in faces {
        let native = face.measurement.as_ref()?;
        let bound = native.enclosure_mm?;
        enclosure[0] = enclosure[0].min(bound[0][axis]);
        enclosure[1] = enclosure[1].max(bound[1][axis]);
        reached[0] = reached[0].min(native.witnesses[0][axis][axis]);
        reached[1] = reached[1].max(native.witnesses[1][axis][axis]);
    }
    // The modeling residual permits surface incidence error, but cannot make a
    // recessed face the nominal extremum. Admit only roundoff when matching a
    // conservative box to a native witness. The allowance never exceeds the
    // modeling residual, including at large part-coordinate offsets.
    let slack = reached.map(|value| (64. * f64::EPSILON * value.abs().max(1.)).min(tolerance));
    if faces.is_empty()
        || (reached[0] - enclosure[0]).abs() > slack[0]
        || (enclosure[1] - reached[1]).abs() > slack[1]
    {
        return None;
    }
    let size = reached[1] - reached[0];
    if !size.is_finite() || size <= tolerance {
        return None;
    }
    let n = Vector::ith(axis, 1.);
    let ends = [0, 1].map(|side| {
        let candidates = faces
            .iter()
            .enumerate()
            .filter(|(_, f)| {
                (f.measurement.as_ref().unwrap().witnesses[side][axis][axis] - reached[side]).abs()
                    <= slack[side]
            })
            .map(|(i, _)| i)
            .collect::<Vec<_>>();
        let caps = candidates
            .iter()
            .copied()
            .filter(|&i| normal(faces[i]).is_some_and(|v| aligned(v, n)))
            .collect::<Vec<_>>();
        if caps.is_empty() { candidates } else { caps }
    });
    let first = &faces[*ends[0].first()?].measurement.as_ref()?.witnesses[0][axis];

    // Extension lines make these axis-projected locations clear in both 2D and 3D.
    let mut start = *first;
    let mut finish = *first;
    start[axis] = reached[0];
    finish[axis] = reached[1];
    // The ID describes the whole part extent. Only representative end faces
    // need to travel as selectable witnesses, even on a large patterned part.
    let supports = ends
        .iter()
        .filter_map(|indices| {
            indices
                .iter()
                .min_by_key(|&&i| faces[i].reference.role.len())
        })
        .map(|&i| &faces[i].reference)
        .filter(|r| r.role.len() <= MAX_SOURCE_KEY)
        .cloned()
        .collect();
    Some((
        Extent {
            low: reached[0],
            high: reached[1],
            low_faces: ends[0].clone(),
        },
        Characteristic {
            id: format!("overall/{}", AXES[axis]),
            label: ["Overall width", "Overall depth", "Overall height"][axis].into(),
            kind: Kind::Linear,
            angle_vertex: None,
            nominal: size,
            witnesses: [start, finish],
            plane_normal: None,
            supports,
        },
    ))
}

struct Incidence {
    /// Adjacent faces within each boundary wire. Never measure across separate
    /// wires on a common stock face (which would cross unrelated pockets).
    wires: BTreeMap<(usize, u32), BTreeSet<usize>>,
    adjacent: Vec<BTreeSet<usize>>,
}
type ReferenceKey<'a> = (Option<&'a NodeRef>, &'a NodeRef, &'a str);
fn key<'a>(body: Option<&'a NodeRef>, r: &'a TopologyRef) -> ReferenceKey<'a> {
    (body, &r.producer, &r.role)
}
type FaceIndex<'a> = BTreeMap<ReferenceKey<'a>, Option<usize>>;

fn face_index<'a>(faces: &[&'a FaceRange]) -> FaceIndex<'a> {
    let mut index = BTreeMap::new();
    for (i, face) in faces.iter().enumerate() {
        index
            .entry(key(face.body.as_ref(), &face.reference))
            .and_modify(|value| *value = None)
            .or_insert(Some(i));
    }
    index
}

fn neighboring_faces(
    index: &FaceIndex<'_>,
    edge: &crate::protocol::EdgeData,
) -> Option<[(usize, u32); 2]> {
    let native = edge.native.as_ref()?;
    if native.version != crate::protocol::NATIVE_SELECTION_VERSION {
        return None;
    }
    let [left, right] = native.boundaries.as_slice() else {
        return None;
    };
    let lookup = |r: &TopologyRef| index.get(&key(edge.body.as_ref(), r)).copied().flatten();
    let (a, b) = (lookup(&left.face)?, lookup(&right.face)?);
    (a != b).then_some([(a, left.boundary), (b, right.boundary)])
}

fn incidence(faces: &[&FaceRange], mesh: &MeshData, index: &FaceIndex<'_>) -> Incidence {
    let mut result = Incidence {
        wires: BTreeMap::new(),
        adjacent: vec![BTreeSet::new(); faces.len()],
    };
    for edge in &mesh.edges {
        let Some([(a, left), (b, right)]) = neighboring_faces(index, edge) else {
            continue;
        };
        result.wires.entry((a, left)).or_default().insert(b);
        result.wires.entry((b, right)).or_default().insert(a);
        result.adjacent[a].insert(b);
        result.adjacent[b].insert(a);
    }
    result
}

struct Characteristics<'a> {
    faces: &'a [&'a FaceRange],
    identities: Vec<Option<Value>>,
    extents: [Option<Extent>; 3],
    dimensions: Vec<Characteristic>,
    coverage: BTreeSet<String>,
    remaining_key_bytes: usize,
    support_bytes: Cell<usize>,
    limited_selection: Cell<bool>,
    locations: BTreeSet<(usize, usize)>,
    tolerance: f64,
}

impl Characteristics<'_> {
    fn id(&mut self, family: &str, sources: &[usize]) -> Option<String> {
        let mut identities = Vec::with_capacity(sources.len());
        let mut size: usize = 0;
        for &i in sources {
            let reference = &self.faces[i].reference;
            if reference.role.starts_with("face:") || reference.role.is_empty() {
                self.coverage
                    .insert("Some faces have no construction reference for a local size.".into());
                return None;
            }
            size = size.saturating_add(reference.role.len());
            if size > MAX_SOURCE_KEY || size > self.remaining_key_bytes {
                self.coverage
                    .insert("Some local dimensions exceed the source-reference budget.".into());
                return None;
            }
            if self.identities[i].is_none() {
                self.identities[i] = crate::provenance::normalized_reference(reference).ok();
            }
            identities.push(self.identities[i].clone()?);
        }
        let id = serialize_identity(family, identities).ok()?;
        if id.len() > self.remaining_key_bytes || id.len() > MAX_SOURCE_KEY {
            self.coverage
                .insert("Some local dimensions exceed the source-reference budget.".into());
            return None;
        }
        self.remaining_key_bytes -= id.len();
        Some(id)
    }
    fn refs(&self, indices: impl IntoIterator<Item = usize>) -> Vec<TopologyRef> {
        indices
            .into_iter()
            .filter_map(|i| {
                let source = &self.faces[i].reference;
                let cost = source.role.len().saturating_add(160);
                if cost > self.support_bytes.get() || cost > MAX_SOURCE_KEY {
                    self.limited_selection.set(true);
                    return None;
                }
                self.support_bytes.set(self.support_bytes.get() - cost);
                Some(source.clone())
            })
            .collect()
    }
    fn span(&mut self, part: &Part, a: usize, b: usize, wall: usize) {
        let (fa, fb) = (self.faces[a], self.faces[b]);
        let (Some(n), Some(nb), Some(pa), Some(pb)) =
            (normal(fa), normal(fb), anchor(fa), anchor(fb))
        else {
            return;
        };
        if !aligned(n, nb) {
            return;
        }
        if let Some(nw) = normal(self.faces[wall]) {
            if n.dot(&nw).abs() > 1e-9 {
                return;
            }
        } else if !self.faces[wall]
            .measurement
            .as_ref()
            .and_then(|m| m.cylinder.as_ref())
            .is_some_and(|c| aligned(vec(c.axis.direction), n))
        {
            return;
        }
        let delta = (pb - pa).dot(&n);
        if delta.abs() <= self.tolerance || !delta.is_finite() {
            return;
        }
        if let Some(axis) = world_axis(n)
            && let Some(extent) = &self.extents[axis]
            && (pa[axis].min(pb[axis]) - extent.low).abs() <= self.tolerance
            && (pa[axis].max(pb[axis]) - extent.high).abs() <= self.tolerance
        {
            return;
        }
        let wall_face = self.faces[wall];
        let same_direction = n.dot(&nb) > 0.;
        let label = if same_direction {
            match part
                .features
                .iter()
                .find(|f| f.id == wall_face.reference.producer.node)
                .map(|f| &f.operation)
            {
                Some(Operation::Extrude { mode, .. })
                    if matches!(mode.as_str(), "cut" | "difference") =>
                {
                    "Pocket depth"
                }
                Some(Operation::Hole { .. } | Operation::DrilledHoles { .. }) => {
                    "Bore segment depth"
                }
                _ if wall_face
                    .measurement
                    .as_ref()
                    .and_then(|m| m.cylinder.as_ref())
                    .is_some_and(|c| c.internal) =>
                {
                    "Bore segment depth"
                }
                _ => "Step height",
            }
        } else if world_axis(n).is_some_and(|i| i == 2) {
            "Local height"
        } else {
            "Face spacing"
        };
        let key = if same_direction {
            let outside = if delta > 0. { b } else { a };
            let inside = if delta > 0. { a } else { b };
            // Depth is the distance from the surrounding rim to this floor. A
            // second pocket can repartition the shared stock face while this
            // floor and its user tolerance remain completely identifiable.
            match label {
                "Pocket depth" | "Bore segment depth" => self.id("depth", &[inside]),
                "Step height"
                    if wall_face.reference.producer == self.faces[outside].reference.producer =>
                {
                    self.id("boss-height", &[outside])
                }
                _ => self.id("span", &[a, b]),
            }
        } else {
            self.id("span", &[a, b])
        };
        let Some(id) = key else {
            return;
        };
        // Start near the actual connecting wall even when the floor is a large face.
        let mid = anchor(wall_face).unwrap_or(pa);
        let start = mid + n * (pa - mid).dot(&n);
        self.dimensions.push(Characteristic {
            id,
            label: label.into(),
            kind: Kind::Linear,
            angle_vertex: None,
            nominal: delta.abs(),
            witnesses: [start.into(), (start + n * delta).into()],
            plane_normal: None,
            supports: self.refs([a, b, wall]),
        });
        // Locate a pocket or boss between two inner side faces using one
        // coordinate from the outer stock face plus the local width above.
        // Measuring both sides from stock as well would be redundant.
        if !same_direction
            && let Some(axis) = world_axis(n)
            && let Some(extent) = &self.extents[axis]
        {
            let (left, lo, hi) = if pa[axis] < pb[axis] {
                (a, pa[axis], pb[axis])
            } else {
                (b, pb[axis], pa[axis])
            };
            if lo > extent.low + self.tolerance
                && hi < extent.high - self.tolerance
                && fa.reference.producer == fb.reference.producer
                && let Some(&datum) = extent.low_faces.iter().find(|&&j| {
                    normal(self.faces[j]).is_some_and(|v| aligned(v, Vector::ith(axis, 1.)))
                })
                && self.locations.insert((axis, left))
            {
                let mut origin = mid;
                origin[axis] = extent.low;
                let mut end = origin;
                end[axis] = lo;
                if let Some(id) = self.id(&format!("face-location-{}", AXES[axis]), &[left]) {
                    self.dimensions.push(Characteristic {
                        id,
                        label: format!("Face location {}", AXES[axis].to_uppercase()),
                        kind: Kind::Linear,
                        angle_vertex: None,
                        nominal: end[axis] - origin[axis],
                        witnesses: [origin.into(), end.into()],
                        plane_normal: None,
                        supports: self.refs([left, datum]),
                    });
                }
            }
        }
    }

    fn circular(
        &mut self,
        part: &Part,
        group: &[usize],
        incidence: &Incidence,
        c: &NativeCylinder,
    ) {
        let n = vec(c.axis.direction);
        let o = vec(c.axis.origin);
        let is_fillet = group.iter().any(|&i| {
            part.features
                .iter()
                .find(|f| f.id == self.faces[i].reference.producer.node)
                .is_some_and(|f| matches!(f.operation, Operation::Fillet { .. }))
        });
        let kind = if is_fillet {
            Kind::Radius
        } else {
            Kind::Diameter
        };
        let family = if is_fillet { "radius" } else { "diameter" };
        let Some(id) = self.id(family, group) else {
            return;
        };
        // Prefer the axial station of a real end rim. Finishing the upper surface
        // moves this station automatically; a deeper, intermediate cut does not
        // invent a new diameter or alter the nominal.
        let rim_faces = group
            .iter()
            .flat_map(|&i| &incidence.adjacent[i])
            .copied()
            .filter(|&i| normal(self.faces[i]).is_some_and(|p| aligned(p, n)))
            .collect::<BTreeSet<_>>();
        let t = rim_faces
            .iter()
            .filter_map(|&i| anchor(self.faces[i]))
            .map(|p| (p - o).dot(&n))
            .max_by(f64::total_cmp)
            .or_else(|| anchor(self.faces[group[0]]).map(|p| (p - o).dot(&n)))
            .unwrap_or(0.);
        let center = o + n * t;
        // A diagonal in an end-on view makes a diameter clear without hiding a
        // horizontal/vertical hole-location witness on the same view.
        let basis = if n.x.abs() < 0.8 {
            Vector::x()
        } else {
            Vector::y()
        };
        let u = (basis - n * basis.dot(&n)).normalize();
        let mut radial = (u + n.cross(&u)).normalize() * c.radius_mm;
        let mut dimension_center = center;
        if is_fillet {
            // The endpoint of a radius leader belongs to the surviving trimmed
            // round. A generic diagonal can point at the removed material.
            let point = anchor(self.faces[group[0]]).unwrap_or(center + radial);
            dimension_center = o + n * (point - o).dot(&n);
            if let Some(dir) = (point - dimension_center).try_normalize(1e-12) {
                radial = dir * c.radius_mm;
            }
        }
        let label = if is_fillet {
            "Fillet radius"
        } else if c.internal {
            "Bore diameter"
        } else {
            "Outside diameter"
        };
        self.dimensions.push(Characteristic {
            id,
            label: label.into(),
            kind,
            angle_vertex: None,
            nominal: if is_fillet {
                c.radius_mm
            } else {
                2. * c.radius_mm
            },
            witnesses: [
                if is_fillet {
                    dimension_center.into()
                } else {
                    (dimension_center - radial).into()
                },
                (dimension_center + radial).into(),
            ],
            plane_normal: Some(n.into()),
            supports: self.refs(group.iter().copied()),
        });
        if is_fillet {
            return;
        }
        if let Some(axial) = world_axis(n) {
            for i in (0..3).filter(|i| *i != axial) {
                let Some(extent) = &self.extents[i] else {
                    continue;
                };
                let Some(&datum) = extent.low_faces.iter().find(|&&j| {
                    normal(self.faces[j]).is_some_and(|p| aligned(p, Vector::ith(i, 1.)))
                }) else {
                    continue;
                };
                let mut start = center;
                start[i] = extent.low;
                let value = center[i] - start[i];
                if value <= self.tolerance {
                    continue;
                }
                let mut supports = group.to_vec();
                supports.push(datum);
                // The location follows the measured axis relative to the minimum
                // planar extent. Its identity therefore survives a new stock cap.
                let Some(id) = self.id(&format!("location-{}", AXES[i]), group) else {
                    continue;
                };
                self.dimensions.push(Characteristic {
                    id,
                    label: format!("Axis location {}", AXES[i].to_uppercase()),
                    kind: Kind::Linear,
                    angle_vertex: None,
                    nominal: value,
                    witnesses: [start.into(), center.into()],
                    plane_normal: Some(n.into()),
                    supports: self.refs(supports),
                });
            }
        }
    }
}

fn same_cylinder(a: &NativeCylinder, b: &NativeCylinder, tol: f64) -> bool {
    let n = vec(a.axis.direction);
    aligned(n, vec(b.axis.direction))
        && (vec(a.axis.origin) - vec(b.axis.origin)).cross(&n).norm() <= tol
        && (a.radius_mm - b.radius_mm).abs() <= tol
        && a.internal == b.internal
}

pub fn infer(
    part: &Part,
    mesh: &MeshData,
    document: &crate::Document,
) -> (Vec<Characteristic>, Vec<String>) {
    let linear_mm = document.settings.modeling.linear_mm;
    let faces = mesh
        .faces
        .iter()
        .filter(|f| f.body_kind == BodyKind::Solid)
        .collect::<Vec<_>>();
    let mut output = Characteristics {
        faces: &faces,
        identities: vec![None; faces.len()],
        extents: [None, None, None],
        dimensions: vec![],
        coverage: BTreeSet::new(),
        remaining_key_bytes: MAX_REPORT_KEYS,
        support_bytes: Cell::new(MAX_REPORT_KEYS),
        limited_selection: Cell::new(false),
        locations: BTreeSet::new(),
        tolerance: linear_mm,
    };
    if faces.len() != mesh.faces.len() {
        output
            .coverage
            .insert("Surface bodies have no automatic finished-solid size.".into());
    }
    if faces.is_empty() {
        return (vec![], output.coverage.into_iter().collect());
    }
    for (axis, name) in AXES.iter().enumerate() {
        if let Some((extent, dimension)) = overall(&faces, axis, linear_mm) {
            output.extents[axis] = Some(extent);
            output.dimensions.push(dimension);
        } else {
            output.coverage.insert(format!(
                "Overall {} could not be verified on the finished solid.",
                name.to_uppercase()
            ));
        }
    }
    let index = face_index(&faces);
    let graph = incidence(&faces, mesh, &index);
    let mut visited = BTreeSet::new();
    let mut pairs = BTreeSet::new();
    let for_face = |i: usize| faces[i].measurement.as_ref()?.cylinder.as_ref();
    for root in 0..faces.len() {
        let Some(c) = for_face(root) else {
            continue;
        };
        if !visited.insert(root) {
            continue;
        }
        let mut group = vec![root];
        let mut index = 0;
        while index < group.len() {
            for &neighbor in &graph.adjacent[group[index]] {
                if !visited.contains(&neighbor)
                    && for_face(neighbor).is_some_and(|other| same_cylinder(c, other, linear_mm))
                {
                    visited.insert(neighbor);
                    group.push(neighbor);
                }
            }
            index += 1;
        }
        output.circular(part, &group, &graph, c);
        // A periodic bore may have two disjoint boundary wires on one wall.
        // They bound the same cylindrical land. Generate depth only when two
        // transverse end faces are unique; interrupted lands need review.
        let rims: BTreeSet<_> = group
            .iter()
            .flat_map(|&i| &graph.adjacent[i])
            .copied()
            .filter(|&i| normal(faces[i]).is_some_and(|v| aligned(v, vec(c.axis.direction))))
            .collect();
        if rims.len() == 2 {
            let mut ends = rims.into_iter();
            let a = ends.next().unwrap();
            let b = ends.next().unwrap();
            let count = output.dimensions.len();
            if !pairs.contains(&(a, b)) {
                output.span(part, a, b, root);
            }
            if count < output.dimensions.len() {
                pairs.insert((a, b));
            }
        }
    }
    let mut visits = 0;
    'spans: for ((wall, _), adjacent) in &graph.wires {
        let all = adjacent
            .iter()
            .copied()
            .filter(|&i| faces[i].plane.is_some())
            .collect::<Vec<_>>();
        for (i, &a) in all.iter().enumerate() {
            for &b in &all[i + 1..] {
                visits += 1;
                if visits > MAX_VISITS {
                    output
                        .coverage
                        .insert("Some local sizes exceed the boundary-pair budget.".into());
                    break 'spans;
                }
                // Only record a pair as complete after a compatible connecting wall
                // generated it; the first shared face may be a sloping chamfer.
                if pairs.contains(&(a, b)) {
                    continue;
                }
                let count = output.dimensions.len();
                output.span(part, a, b, *wall);
                if output.dimensions.len() > count {
                    pairs.insert((a, b));
                }
            }
        }
    }
    cones::add(&mut output, part, mesh, &graph, &index);
    drills::consolidate(&mut output, part, document, &graph);
    let other = faces
        .iter()
        .filter(|f| {
            f.plane.is_none()
                && f.measurement
                    .as_ref()
                    .is_none_or(|m| m.cylinder.is_none() && m.cone.is_none())
        })
        .count();
    if other > 0 {
        output.coverage.insert(format!(
            "{other} curved faces have no automatic local size."
        ));
    }
    chamfers::add(&mut output, part, mesh, &index);
    contour_corners::add(&mut output, part, mesh, &index);
    if graph.wires.is_empty() {
        output
            .coverage
            .insert("Local face spacings require native edge incidence for this part.".into());
    }
    if output.limited_selection.get() {
        output
            .coverage
            .insert("Some selection witnesses were omitted to limit report size.".into());
    }
    let overall_count = output.extents.iter().flatten().count();
    output.dimensions[overall_count..]
        .sort_by(|a, b| a.label.cmp(&b.label).then_with(|| a.id.cmp(&b.id)));
    (output.dimensions, output.coverage.into_iter().collect())
}
