//! Assembled native common checks and explicitly approximate boundary distances.
use crate::adapter::KernelAdapter;
use cad_core::{
    CadError, Dimension, Document, Id, PlacedPart, Result,
    assembly_inspection::{
        self, BoundaryDistance, FaceReference, InactiveTarget, InterferenceStatus, Mode,
        Occurrence, Pair, Report,
    },
    expression,
    protocol::{BodyKind, MeshData},
};
use monstertruck::modeling::*;
use std::collections::{BTreeMap, BTreeSet};

mod native_contact;

const MAX_COORDINATE: f64 = 1e12;
const MAX_TRIANGLES: usize = 500_000;
const MAX_DISTANCE_WORK: usize = 16_000_000;
const MAX_BOOLEAN_CALLS: usize = 4096;
const MAX_BOOLEAN_FACE_PAIRS: usize = 1_000_000;
const MAX_REPORT_BYTES: usize = 64 * 1024 * 1024;
const MAX_CACHE_BYTES: usize = 32 * 1024 * 1024;
const MAX_SCOPE_WORK: usize = 16_000_000;
const MAX_NATIVE_CONTROLS: usize = 1_048_576;
const MAX_NATIVE_FACES: usize = 32_768;
const MAX_NATIVE_EDGES: usize = 131_072;
const MAX_NATIVE_WORK: usize = 32_000_000;
const MAX_BODY_PAIRS: usize = 65_536;
#[derive(Default)]
struct Budget {
    triangles: usize,
    distance: usize,
    booleans: usize,
    face_pairs: usize,
    report_bytes: usize,
    cache_bytes: usize,
    scope_work: usize,
    native_controls: usize,
    native_faces: usize,
    native_edges: usize,
    native_work: usize,
    body_pairs: usize,
    contact: native_contact::Budget,
}
struct ByteCounter {
    bytes: usize,
    limit: usize,
}
impl std::io::Write for ByteCounter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.bytes = self.bytes.saturating_add(bytes.len());
        if self.bytes > self.limit {
            return Err(std::io::Error::other(
                "Inspection metadata byte budget exhausted",
            ));
        }
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
fn serialized_size(
    value: &impl serde::Serialize,
    limit: usize,
) -> std::result::Result<usize, String> {
    let mut counter = ByteCounter { bytes: 0, limit };
    serde_json::to_writer(&mut counter, value).map_err(message)?;
    Ok(counter.bytes)
}
fn charge_report(
    value: &impl serde::Serialize,
    budget: &mut Budget,
    overhead: usize,
) -> std::result::Result<(), String> {
    let remaining = MAX_REPORT_BYTES
        .saturating_sub(budget.report_bytes)
        .saturating_sub(overhead);
    let bytes = serialized_size(value, remaining)?;
    budget.report_bytes = budget
        .report_bytes
        .saturating_add(bytes)
        .saturating_add(overhead);
    if budget.report_bytes > MAX_REPORT_BYTES {
        return Err(message("Aggregate report metadata exceeds 64 MiB"));
    }
    Ok(())
}
fn charge_cache(
    value: &impl serde::Serialize,
    budget: &mut Budget,
) -> std::result::Result<(), String> {
    let bytes = serialized_size(value, MAX_CACHE_BYTES.saturating_sub(budget.cache_bytes))?;
    budget.cache_bytes = budget.cache_bytes.saturating_add(bytes);
    Ok(())
}
fn scope_work(budget: &mut Budget, work: usize) -> std::result::Result<(), String> {
    budget.scope_work = budget.scope_work.saturating_add(work);
    if budget.scope_work > MAX_SCOPE_WORK {
        return Err(message(
            "Occurrence lookup and selected-path work exceeds sixteen million bounded units",
        ));
    }
    Ok(())
}
fn message(error: impl std::fmt::Display) -> String {
    format!("Assembly inspection: {error}")
        .chars()
        .take(2048)
        .collect()
}
fn diagnostic(pair: &mut Pair, error: String) {
    if pair.diagnostics.len() < 32 {
        pair.diagnostics.push(error);
    } else if pair.diagnostics.len() == 32 {
        pair.diagnostics.push(message(
            "Additional native pair diagnostics were omitted after 32 messages",
        ));
    }
}
fn invalid(error: &str) -> CadError {
    CadError::Invalid(message(error))
}
fn finite(p: Point3) -> bool {
    [p.x, p.y, p.z]
        .into_iter()
        .all(|x| x.is_finite() && x.abs() <= MAX_COORDINATE)
}
fn matrix(instance: &PlacedPart) -> std::result::Result<Matrix4, String> {
    let m = Matrix4::from_cols(
        Vector4::from([
            instance.matrix[0][0],
            instance.matrix[1][0],
            instance.matrix[2][0],
            instance.matrix[3][0],
        ]),
        Vector4::from([
            instance.matrix[0][1],
            instance.matrix[1][1],
            instance.matrix[2][1],
            instance.matrix[3][1],
        ]),
        Vector4::from([
            instance.matrix[0][2],
            instance.matrix[1][2],
            instance.matrix[2][2],
            instance.matrix[3][2],
        ]),
        Vector4::from([
            instance.matrix[0][3],
            instance.matrix[1][3],
            instance.matrix[2][3],
            instance.matrix[3][3],
        ]),
    );
    if instance.matrix.iter().flatten().any(|x| !x.is_finite())
        || instance.matrix[3] != [0., 0., 0., 1.]
        || (0..3).any(|axis| instance.matrix[axis][3].abs() > MAX_COORDINATE)
    {
        return Err(message(
            "Occurrence placement must be finite, affine and within the inspection coordinate bound",
        ));
    }
    let columns = [m.x.truncate(), m.y.truncate(), m.z.truncate()];
    if columns.iter().any(|v| (v.magnitude2() - 1.).abs() > 1e-9)
        || columns[0].dot(columns[1]).abs() > 1e-9
        || columns[0].dot(columns[2]).abs() > 1e-9
        || columns[1].dot(columns[2]).abs() > 1e-9
        || columns[0].cross(columns[1]).dot(columns[2]) < 1. - 1e-9
    {
        return Err(message(
            "Inspection requires the current rigid right-handed mechanical placement",
        ));
    }
    Ok(m)
}
fn point(p: [f64; 3], matrix: Matrix4) -> std::result::Result<Point3, String> {
    let p = matrix.transform_point(Point3::from(p));
    if !finite(p) {
        return Err(message(
            "Inspection coordinates must be finite and within +/-1e12 mm",
        ));
    }
    Ok(p)
}
type Bounds = [[f64; 3]; 2];
struct NativeBody {
    solid: Solid,
    bounds: Option<Bounds>,
    faces: usize,
    contact: Option<native_contact::Body>,
}
fn admission(solid: &Solid, budget: &mut Budget) -> std::result::Result<usize, String> {
    let faces = solid.face_iter().take(4097).count();
    let edges = solid.edge_iter().take(16385).count();
    if faces > 4096 || edges > 16384 {
        return Err(message(
            "One native body exceeds 4096 faces or 16384 edges before topology copying",
        ));
    }
    budget.native_faces = budget.native_faces.saturating_add(faces);
    budget.native_edges = budget.native_edges.saturating_add(edges);
    budget.native_work = budget.native_work.saturating_add(
        faces
            .saturating_mul(faces.saturating_add(edges))
            .saturating_mul(4),
    );
    if budget.native_faces > MAX_NATIVE_FACES
        || budget.native_edges > MAX_NATIVE_EDGES
        || budget.native_work > MAX_NATIVE_WORK
    {
        return Err(message(
            "Native source/result admission exceeds the shared topology or thirty-two million predicate-work budget",
        ));
    }
    let limit = MAX_NATIVE_CONTROLS
        .saturating_sub(budget.native_controls)
        .min(262144);
    // Reserve the complete attempted preflight so repeated rejected sources cannot
    // restart an uncharged, large carrier traversal.
    budget.native_controls = budget.native_controls.saturating_add(limit);
    let controls = crate::split::control_cost(solid, limit).map_err(message)?;
    budget.native_controls = budget.native_controls.saturating_sub(limit - controls);
    Ok(faces)
}
fn native_solids(
    instance: &PlacedPart,
    bodies: &BTreeMap<Id, Vec<Solid>>,
    budget: &mut Budget,
    linear_mm: f64,
) -> std::result::Result<Vec<NativeBody>, String> {
    let transform = matrix(instance)?;
    let sources = bodies
        .get(&instance.mesh_key)
        .map(Vec::as_slice)
        .unwrap_or_default();
    if sources.len() > 256 {
        return Err(message("One occurrence exceeds 256 solid bodies"));
    }
    let mut prepared = Vec::new();
    for source in sources {
        let faces = admission(source, budget)?;
        let solid = builder::transformed(source, transform);
        if solid.vertex_iter().any(|vertex| !finite(vertex.point())) {
            return Err(message(
                "Placed native vertices exceed the finite inspection coordinate bound",
            ));
        }
        for boundary in solid.boundaries() {
            crate::native_trims::shell(boundary, linear_mm).map_err(message)?;
        }
        crate::validation::solid(&solid).map_err(message)?;
        // Exact support admission is optional. A rejected proof must not hide
        // a solid otherwise admitted for the native Boolean fallback.
        let contact = native_contact::Body::new(&solid, &mut budget.contact).ok();
        let bounds = contact.as_ref().and_then(native_contact::Body::bounds);
        prepared.push(NativeBody {
            solid,
            bounds,
            faces,
            contact,
        });
    }
    Ok(prepared)
}
fn union(a: Bounds, b: Bounds) -> Bounds {
    [
        std::array::from_fn(|i| a[0][i].min(b[0][i])),
        std::array::from_fn(|i| a[1][i].max(b[1][i])),
    ]
}
fn points_bounds(points: &[Point3]) -> Bounds {
    let mut b = [[f64::INFINITY; 3], [f64::NEG_INFINITY; 3]];
    for p in points {
        for i in 0..3 {
            b[0][i] = b[0][i].min(p[i]);
            b[1][i] = b[1][i].max(p[i]);
        }
    }
    b
}
fn separated(a: Bounds, b: Bounds, tolerance: f64) -> bool {
    let magnitude = a
        .iter()
        .chain(b.iter())
        .flatten()
        .map(|x| x.abs())
        .fold(1., f64::max);
    let margin = tolerance + magnitude * 1e-12;
    (0..3).any(|i| a[0][i] - b[1][i] > margin || b[0][i] - a[1][i] > margin)
}
fn bounds_distance(a: Bounds, b: Bounds) -> f64 {
    (0..3)
        .map(|i| (a[0][i] - b[1][i]).max(b[0][i] - a[1][i]).max(0.).powi(2))
        .sum()
}
#[derive(Clone)]
struct Triangle {
    points: [Point3; 3],
    bounds: Bounds,
    face: usize,
}
struct Node {
    bounds: Bounds,
    children: Option<[usize; 2]>,
    triangles: Vec<usize>,
}
struct Boundary {
    faces: Vec<FaceReference>,
    triangles: Vec<Triangle>,
    nodes: Vec<Node>,
}
fn build_node(triangles: &[Triangle], mut indices: Vec<usize>, nodes: &mut Vec<Node>) -> usize {
    let bounds = indices
        .iter()
        .skip(1)
        .fold(triangles[indices[0]].bounds, |b, i| {
            union(b, triangles[*i].bounds)
        });
    let index = nodes.len();
    nodes.push(Node {
        bounds,
        children: None,
        triangles: vec![],
    });
    if indices.len() <= 8 {
        nodes[index].triangles = indices;
        return index;
    }
    let axis = (0..3)
        .max_by(|a, b| (bounds[1][*a] - bounds[0][*a]).total_cmp(&(bounds[1][*b] - bounds[0][*b])))
        .unwrap_or(0);
    indices.sort_by(|a, b| {
        (triangles[*a].bounds[0][axis] + triangles[*a].bounds[1][axis])
            .total_cmp(&(triangles[*b].bounds[0][axis] + triangles[*b].bounds[1][axis]))
    });
    let right = indices.split_off(indices.len() / 2);
    let children = [
        build_node(triangles, indices, nodes),
        build_node(triangles, right, nodes),
    ];
    nodes[index].children = Some(children);
    index
}
fn boundary(
    instance: &PlacedPart,
    mesh: &MeshData,
    budget: &mut Budget,
) -> std::result::Result<Boundary, String> {
    let transform = matrix(instance)?;
    let mut faces = Vec::new();
    let mut triangles = Vec::new();
    if mesh.faces.len() > 32768 {
        return Err(message("One evaluated boundary exceeds 32768 face records"));
    }
    let mut triangle_count = 0usize;
    for face in &mesh.faces {
        face.start
            .checked_add(face.count)
            .filter(|end| *end <= mesh.indices.len())
            .ok_or_else(|| message("Invalid evaluated face index range"))?;
        if face.count % 3 != 0 {
            return Err(message(
                "Evaluated faces require complete boundary triangles",
            ));
        }
        triangle_count = triangle_count.saturating_add(face.count / 3);
        charge_cache(&(&face.body, &face.reference), budget)?;
    }
    budget.triangles = budget.triangles.saturating_add(triangle_count);
    if budget.triangles > MAX_TRIANGLES {
        return Err(message(
            "Boundary inspection exceeds 500000 placed triangles before coordinate copying; reduce the selected scope",
        ));
    }
    for face in &mesh.faces {
        let end = face
            .start
            .checked_add(face.count)
            .filter(|end| *end <= mesh.indices.len())
            .ok_or_else(|| message("Invalid evaluated face index range"))?;
        if face.count % 3 != 0 {
            return Err(message(
                "Evaluated faces require complete boundary triangles",
            ));
        }
        let body = face
            .body
            .clone()
            .ok_or_else(|| message("Evaluated boundary has no stable output owner"))?;
        let face_index = faces.len();
        faces.push(FaceReference {
            body,
            reference: face.reference.clone(),
        });
        for tri in mesh.indices[face.start..end].as_chunks::<3>().0 {
            let p = tri
                .iter()
                .map(|i| {
                    mesh.positions
                        .get(*i as usize)
                        .copied()
                        .ok_or_else(|| message("Invalid evaluated triangle vertex"))
                        .and_then(|p| point(p, transform))
                })
                .collect::<std::result::Result<Vec<_>, _>>()?;
            let points = [p[0], p[1], p[2]];
            triangles.push(Triangle {
                points,
                bounds: points_bounds(&points),
                face: face_index,
            });
        }
    }
    if triangles.is_empty() {
        return Err(message(
            "The selected occurrence has no evaluated boundary triangles",
        ));
    }
    let mut nodes = Vec::new();
    build_node(&triangles, (0..triangles.len()).collect(), &mut nodes);
    Ok(Boundary {
        faces,
        triangles,
        nodes,
    })
}
fn segment_point(p: Point3, a: Point3, b: Point3) -> Point3 {
    let d = b - a;
    let size = d.magnitude2();
    if size <= 0. {
        a
    } else {
        a + d * ((p - a).dot(d) / size).clamp(0., 1.)
    }
}
fn triangle_point(p: Point3, t: [Point3; 3]) -> Point3 {
    let [a, b, c] = t;
    let ab = b - a;
    let ac = c - a;
    let normal = ab.cross(ac);
    let size = normal.magnitude2();
    if size > 0. {
        let q = p - normal * ((p - a).dot(normal) / size);
        let qa = q - a;
        let v = qa.cross(ac).dot(normal) / size;
        let w = ab.cross(qa).dot(normal) / size;
        if v >= 0. && w >= 0. && v + w <= 1. {
            return q;
        }
    }
    [(a, b), (b, c), (c, a)]
        .into_iter()
        .map(|(a, b)| segment_point(p, a, b))
        .min_by(|a, b| a.distance2(p).total_cmp(&b.distance2(p)))
        .unwrap_or(a)
}
fn segments(a: Point3, b: Point3, c: Point3, d: Point3) -> (Point3, Point3) {
    let u = b - a;
    let v = d - c;
    let w = a - c;
    let aa = u.dot(u);
    let bb = u.dot(v);
    let cc = v.dot(v);
    let dd = u.dot(w);
    let ee = v.dot(w);
    let mut s = 0.;
    let mut t = 0.;
    if aa <= 0. {
        if cc > 0. {
            t = (ee / cc).clamp(0., 1.);
        }
    } else if cc <= 0. {
        s = (-dd / aa).clamp(0., 1.);
    } else {
        let determinant = aa * cc - bb * bb;
        if determinant > aa * cc * 1e-14 {
            s = ((bb * ee - cc * dd) / determinant).clamp(0., 1.);
        }
        t = (bb * s + ee) / cc;
        if t < 0. {
            t = 0.;
            s = (-dd / aa).clamp(0., 1.);
        } else if t > 1. {
            t = 1.;
            s = ((bb - dd) / aa).clamp(0., 1.);
        }
    }
    (a + u * s, c + v * t)
}
fn piercing(a: Point3, b: Point3, tri: [Point3; 3]) -> Option<Point3> {
    let u = tri[1] - tri[0];
    let v = tri[2] - tri[0];
    let n = u.cross(v);
    let length = n.magnitude2();
    let direction = b - a;
    let denominator = direction.dot(n);
    if length <= 0. || denominator.abs() <= direction.magnitude() * length.sqrt() * 1e-14 {
        return None;
    }
    let t = (tri[0] - a).dot(n) / denominator;
    if !(0. ..=1.).contains(&t) {
        return None;
    }
    let p = a + direction * t;
    let q = p - tri[0];
    let x = q.cross(v).dot(n) / length;
    let y = u.cross(q).dot(n) / length;
    (x >= 0. && y >= 0. && x + y <= 1.).then_some(p)
}
fn triangle_distance(a: [Point3; 3], b: [Point3; 3]) -> (f64, [Point3; 2]) {
    let mut result = (f64::INFINITY, [a[0], b[0]]);
    let mut candidate = |p: Point3, q: Point3| {
        let d = p.distance2(q);
        if d < result.0 {
            result = (d, [p, q]);
        }
    };
    for p in a {
        candidate(p, triangle_point(p, b));
    }
    for q in b {
        candidate(triangle_point(q, a), q);
    }
    for i in 0..3 {
        if let Some(p) = piercing(a[i], a[(i + 1) % 3], b) {
            candidate(p, p);
        }
        if let Some(p) = piercing(b[i], b[(i + 1) % 3], a) {
            candidate(p, p);
        }
        for j in 0..3 {
            let (p, q) = segments(a[i], a[(i + 1) % 3], b[j], b[(j + 1) % 3]);
            candidate(p, q);
        }
    }
    result
}
fn distance(
    first: &Boundary,
    second: &Boundary,
    threshold: f64,
    budget: &mut Budget,
) -> std::result::Result<BoundaryDistance, String> {
    let mut stack = vec![(0usize, 0usize)];
    let mut best = f64::INFINITY;
    let mut witness = None;
    while let Some((a, b)) = stack.pop() {
        budget.distance = budget.distance.saturating_add(1);
        if budget.distance > MAX_DISTANCE_WORK {
            return Err(message(
                "Boundary distance exceeds sixteen million bounded node/triangle comparisons; reduce the selected scope",
            ));
        }
        let x = &first.nodes[a];
        let y = &second.nodes[b];
        if bounds_distance(x.bounds, y.bounds) > best {
            continue;
        }
        match (x.children, y.children) {
            (None, None) => {
                for i in &x.triangles {
                    for j in &y.triangles {
                        budget.distance = budget.distance.saturating_add(1);
                        if budget.distance > MAX_DISTANCE_WORK {
                            return Err(message("Boundary distance comparison budget exhausted"));
                        }
                        let p = &first.triangles[*i];
                        let q = &second.triangles[*j];
                        if bounds_distance(p.bounds, q.bounds) > best {
                            continue;
                        }
                        let (d, points) = triangle_distance(p.points, q.points);
                        if !d.is_finite() || points.iter().copied().any(|p| !finite(p)) {
                            return Err(message(
                                "Boundary distance became nonfinite or numerically uncertain",
                            ));
                        }
                        if d < best {
                            best = d;
                            witness = Some((points, p.face, q.face));
                        }
                    }
                }
            }
            (Some(children), None) => stack.extend(children.into_iter().map(|a| (a, b))),
            (None, Some(children)) => stack.extend(children.into_iter().map(|b| (a, b))),
            (Some(c), Some(d)) => {
                for a in c {
                    for b in d {
                        stack.push((a, b));
                    }
                }
            }
        }
        if stack.len() > 131072 {
            return Err(message("Boundary distance pending-node budget exhausted"));
        }
    }
    let (points, a, b) =
        witness.ok_or_else(|| message("No finite boundary-distance witness is available"))?;
    let distance = best.sqrt();
    Ok(BoundaryDistance {
        distance_mm: distance,
        witness_mm: points.map(|p| [p.x, p.y, p.z]),
        first_face: first.faces[a].clone(),
        second_face: second.faces[b].clone(),
        below_requested: distance < threshold,
        representation: "approximate_f64_boundary_triangles".into(),
        error_bound_mm: None,
    })
}
fn planar_volume(solid: &Solid) -> Option<f64> {
    // No unbounded tessellation is invoked for optional common volume. For a
    // validated planar, straight-edge shell, oriented boundary fans supply a
    // bounded signed integral, including inner loops and cavities.
    let origin = solid.vertex_iter().next()?.point();
    let mut sum = 0.;
    let mut correction = 0.;
    for face in solid.face_iter() {
        if !matches!(face.surface(), Surface::Plane(_)) {
            return None;
        }
        for wire in face.absolute_boundaries() {
            let anchor = wire.front()?.front().point();
            for edge in wire {
                if !matches!(edge.curve(), Curve::Line(_)) {
                    return None;
                }
                let a = edge.front().point();
                let b = edge.back().point();
                if !finite(a) || !finite(b) || !finite(anchor) || !finite(origin) {
                    return None;
                }
                let volume = (anchor - origin).dot((a - origin).cross(b - origin)) / 6.
                    * if face.orientation() { 1. } else { -1. };
                let y = volume - correction;
                let next = sum + y;
                correction = (next - sum) - y;
                sum = next;
            }
        }
    }
    let volume = sum.abs();
    (volume.is_finite() && volume > 0.).then_some(volume)
}
fn interference(
    first: &std::result::Result<Vec<NativeBody>, String>,
    second: &std::result::Result<Vec<NativeBody>, String>,
    tolerance: f64,
    pair: &mut Pair,
    budget: &mut Budget,
    linear_mm: f64,
) {
    let (first, second) = match (first, second) {
        (Ok(a), Ok(b)) if !a.is_empty() && !b.is_empty() => (a, b),
        _ => {
            pair.interference = InterferenceStatus::Unavailable;
            diagnostic(pair,first.as_ref().err().or(second.as_ref().err()).cloned().unwrap_or_else(||message("Volumetric interference requires solid bodies in both occurrences; open surfaces are excluded")));
            return;
        }
    };
    let mut separated_count = 0;
    let mut disjoint_count = 0;
    let mut touching_count = 0;
    let mut tolerance_count = 0;
    let mut contact_band_mm = 0.0f64;
    let mut incomplete = false;
    let mut volumes = Vec::new();
    'body_pairs: for a in first {
        for b in second {
            budget.body_pairs = budget.body_pairs.saturating_add(1);
            if budget.body_pairs > MAX_BODY_PAIRS {
                incomplete = true;
                diagnostic(
                    pair,
                    message(
                        "Native hull/body-pair work exceeds 65536 pairs; remaining body pairs are unresolved",
                    ),
                );
                break 'body_pairs;
            }
            if a.bounds
                .zip(b.bounds)
                .is_some_and(|(a, b)| separated(a, b, tolerance))
            {
                separated_count += 1;
                continue;
            }
            let contact = match (&a.contact, &b.contact) {
                (Some(a), Some(b)) => {
                    native_contact::classify(a, b, &mut budget.contact, linear_mm)
                }
                _ => Ok(native_contact::Classification::Unresolved),
            };
            match contact {
                Ok(native_contact::Classification::Separated) => {
                    separated_count += 1;
                    continue;
                }
                Ok(native_contact::Classification::Touching) => {
                    touching_count += 1;
                    continue;
                }
                Ok(native_contact::Classification::DisjointInteriors) => {
                    disjoint_count += 1;
                    continue;
                }
                Ok(native_contact::Classification::ContactWithinTolerance { band_mm }) => {
                    tolerance_count += 1;
                    contact_band_mm = contact_band_mm.max(band_mm);
                    continue;
                }
                Ok(native_contact::Classification::Unresolved) => {}
                Err(error) => diagnostic(pair, message(error)),
            }
            let work = a.faces.saturating_mul(b.faces);
            budget.booleans = budget.booleans.saturating_add(1);
            budget.face_pairs = budget.face_pairs.saturating_add(work);
            if budget.booleans > MAX_BOOLEAN_CALLS || budget.face_pairs > MAX_BOOLEAN_FACE_PAIRS {
                incomplete = true;
                diagnostic(
                    pair,
                    message(
                        "Native Boolean work exceeds 4096 calls or one million face pairs; remaining body pairs are unresolved",
                    ),
                );
                break 'body_pairs;
            }
            let common = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                monstertruck::solid::and(&a.solid, &b.solid, tolerance)
            }));
            match common {
                Ok(Ok(common)) if common.boundaries().is_empty() => {}
                Ok(Ok(common)) => {
                    if let Err(error) = admission(&common, budget) {
                        incomplete = true;
                        diagnostic(pair, error);
                        continue;
                    }
                    let valid = common
                        .boundaries()
                        .iter()
                        .try_for_each(|boundary| {
                            crate::native_trims::shell(boundary, linear_mm).map(|_| ())
                        })
                        .and_then(|()| crate::validation::solid(&common));
                    match valid {
                        Ok(()) => {
                            pair.common_solid_results += 1;
                            let volume = planar_volume(&common);
                            if volume.is_none() {
                                diagnostic(
                                    pair,
                                    message(
                                        "Common volume omitted: this result is outside the bounded planar straight-edge integral domain",
                                    ),
                                );
                            }
                            volumes.push(volume);
                        }
                        Err(error) => {
                            incomplete = true;
                            diagnostic(pair, message(error));
                        }
                    }
                }
                Ok(Err(error)) => {
                    incomplete = true;
                    diagnostic(pair, message(error));
                }
                Err(_) => {
                    incomplete = true;
                    diagnostic(
                        pair,
                        message(
                            "The native Boolean backend rejected this pair during construction",
                        ),
                    );
                }
            }
        }
    }
    if pair.common_solid_results > 0 {
        pair.interference = InterferenceStatus::ValidatedCommonSolid;
        pair.engineering_clearance_mm = Some(0.);
        if volumes.len() == 1 && !incomplete {
            pair.approximate_common_volume_mm3 = volumes[0];
        } else if volumes.len() > 1 {
            diagnostic(
                pair,
                message(
                    "Several common body-pair solids may overlap; a combined union volume is not reported",
                ),
            );
        }
    } else if separated_count == first.len() * second.len() {
        pair.interference = InterferenceStatus::SeparatedNativeHulls;
    } else if separated_count + touching_count + disjoint_count + tolerance_count
        == first.len() * second.len()
    {
        if tolerance_count > 0 {
            pair.interference = InterferenceStatus::ContactWithinModelTolerance;
            pair.contact_residual_bound_mm = Some(contact_band_mm);
        } else if touching_count > 0 {
            pair.interference = InterferenceStatus::TouchingNativeBoundaries;
            pair.engineering_clearance_mm = Some(0.);
        } else {
            pair.interference = InterferenceStatus::DisjointNativeInteriors;
        }
    } else if incomplete {
        pair.interference = InterferenceStatus::Unavailable;
    } else {
        pair.interference = InterferenceStatus::NoCommonSolidReturned;
        diagnostic(
            pair,
            message(
                "The native Boolean returned no common solid; coincident/tangent contact and missed intersections remain unresolved, so this is not a separation certificate",
            ),
        );
    }
}
struct Cached {
    surface_faces: usize,
    solids: Option<std::result::Result<Vec<NativeBody>, String>>,
    boundary: Option<std::result::Result<Boundary, String>>,
}
fn occurrence_index<'a>(
    instances: &'a [PlacedPart],
    definitions: &[&assembly_inspection::Definition],
    budget: &mut Budget,
) -> std::result::Result<BTreeMap<&'a [Id], Option<&'a PlacedPart>>, String> {
    if instances.len() > 100000 {
        return Err(message(
            "Occurrence lookup exceeds 100000 assembled leaf instances",
        ));
    }
    let wanted = definitions
        .iter()
        .flat_map(|d| d.targets.iter().map(|t| t.path.as_slice()))
        .collect::<BTreeSet<_>>();
    let mut index = BTreeMap::new();
    for instance in instances {
        if instance.path.len() > 64 || instance.path.iter().any(|id| id.len() > 256) {
            return Err(message(
                "Assembled occurrence paths exceed the bounded native identity domain",
            ));
        }
        let path_cost = instance
            .path
            .iter()
            .map(String::len)
            .sum::<usize>()
            .saturating_add(1)
            .saturating_mul(12);
        scope_work(budget, path_cost)?;
        if wanted.contains(&instance.path.as_slice()) {
            index
                .entry(instance.path.as_slice())
                .and_modify(|found| *found = None)
                .or_insert(Some(instance));
        }
    }
    Ok(index)
}
pub(crate) fn evaluate<K: KernelAdapter>(
    _adapter: &K,
    doc: &Document,
    instances: &[PlacedPart],
    meshes: &BTreeMap<Id, MeshData>,
    bodies: &BTreeMap<Id, Vec<Solid>>,
) -> Result<BTreeMap<Id, Report>> {
    assembly_inspection::validate(doc)?;
    let Some(assembly) = doc.assemblies.get(&doc.root) else {
        return Ok(BTreeMap::new());
    };
    let configuration = doc.active_configurations.get(&doc.root).cloned();
    let scoped = cad_core::configuration::scope(doc, doc, &doc.root, configuration.as_deref())?;
    let definitions = assembly
        .inspections
        .values()
        .filter(|d| {
            !d.suppressed && (d.configuration.is_none() || d.configuration == configuration)
        })
        .collect::<Vec<_>>();
    // Reserve diagnostic and object-field overhead for every report, including
    // reports whose occurrence or pair payload is stopped by the shared limit.
    let mut budget = Budget {
        report_bytes: definitions.len() * 8192,
        ..Budget::default()
    };
    let lookup = occurrence_index(instances, &definitions, &mut budget);
    let mut cache: BTreeMap<Vec<Id>, Cached> = BTreeMap::new();
    let mut reports = BTreeMap::new();
    for definition in definitions {
        let clearance = expression::value(
            &scoped,
            &assembly.id,
            &definition.clearance,
            Dimension::Length,
        )?;
        let tolerance = expression::value(
            &scoped,
            &assembly.id,
            &definition.boolean_tolerance,
            Dimension::Length,
        )?;
        if !clearance.is_finite()
            || clearance < 0.
            || !tolerance.is_finite()
            || !(1e-6..=1.).contains(&tolerance)
        {
            return Err(invalid(
                "Requested clearance must be finite and nonnegative; native Boolean tolerance must be 0.000001..1 mm",
            ));
        }
        let mut report = Report {
            version: assembly_inspection::REPORT_VERSION,
            assembly: assembly.id.clone(),
            configuration: configuration.clone(),
            inspection: definition.id.clone(),
            clearance_mm: clearance,
            boolean_tolerance_mm: tolerance,
            occurrences: vec![],
            inactive_targets: vec![],
            expected_pairs: 0,
            pairs: vec![],
            diagnostics: vec![],
        };
        if let Err(error) = charge_report(&report, &mut budget, 0) {
            report.diagnostics.push(error);
            reports.insert(definition.id.clone(), report);
            continue;
        }
        for target in &definition.targets {
            let resolved = match &lookup {
                Err(error) => Err(error.clone()),
                Ok(index) => match index.get(&target.path.as_slice()) {
                    Some(Some(instance)) if instance.configuration == target.configuration => {
                        matrix(instance).map(|_| *instance)
                    }
                    None => Err(message(
                        "Occurrence is missing, suppressed or inactive in the current assembly configuration",
                    )),
                    _ => Err(message(
                        "Occurrence identity is ambiguous or its leaf configuration changed; explicitly update the saved target",
                    )),
                },
            };
            let instance = match resolved {
                Ok(instance) => instance,
                Err(error) => {
                    if let Err(error) = charge_report(&(target, &error), &mut budget, 128) {
                        report.diagnostics.push(error);
                        report.diagnostics.push(message(
                            "Remaining selected-target metadata was not published",
                        ));
                        break;
                    }
                    report.inactive_targets.push(InactiveTarget {
                        target: target.clone(),
                        message: error,
                    });
                    continue;
                }
            };
            if let Err(error) = scope_work(
                &mut budget,
                target
                    .path
                    .iter()
                    .map(String::len)
                    .sum::<usize>()
                    .saturating_mul(12),
            ) {
                report.diagnostics.push(error);
                break;
            }
            let surface_faces = if let Some(entry) = cache.get(&instance.path) {
                entry.surface_faces
            } else {
                let mesh = meshes.get(&instance.mesh_key);
                let faces = mesh.map_or(0, |m| m.faces.len());
                if faces > 32768 {
                    report.diagnostics.push(message("Occurrence metadata exceeds 32768 evaluated face records; remaining targets were not processed"));
                    break;
                }
                if let Err(error) = scope_work(&mut budget, faces) {
                    report.diagnostics.push(error);
                    break;
                }
                mesh.map(|m| {
                    m.faces
                        .iter()
                        .filter(|f| f.body_kind == BodyKind::Surface)
                        .count()
                })
                .unwrap_or(0)
            };
            let solid_count = bodies.get(&instance.mesh_key).map(Vec::len).unwrap_or(0);
            if let Err(error) = charge_report(
                &(
                    target,
                    &instance.definition,
                    &instance.name,
                    &instance.mesh_key,
                    &instance.matrix,
                    solid_count,
                    surface_faces,
                ),
                &mut budget,
                256,
            ) {
                report.diagnostics.push(error);
                report.diagnostics.push(message(
                    "Remaining selected-target metadata was not published",
                ));
                break;
            }
            if !cache.contains_key(&instance.path) {
                if let Err(error) = charge_cache(&(&instance.path, &instance.matrix), &mut budget) {
                    report.diagnostics.push(error);
                    break;
                }
                cache.insert(
                    instance.path.clone(),
                    Cached {
                        surface_faces,
                        solids: None,
                        boundary: None,
                    },
                );
            }
            report.occurrences.push(Occurrence {
                target: target.clone(),
                definition: instance.definition.clone(),
                name: instance.name.clone(),
                mesh_key: instance.mesh_key.clone(),
                matrix: instance.matrix,
                solid_count,
                surface_faces,
            });
            let entry = cache
                .get_mut(&instance.path)
                .expect("Admitted occurrence cache exists");
            if definition.mode != Mode::BoundaryDistance && entry.solids.is_none() {
                entry.solids = Some(native_solids(
                    instance,
                    bodies,
                    &mut budget,
                    doc.settings.modeling.linear_mm,
                ));
            }
            if definition.mode != Mode::Interference && entry.boundary.is_none() {
                entry.boundary = Some(
                    meshes
                        .get(&instance.mesh_key)
                        .ok_or_else(|| message("The configured occurrence mesh is unavailable"))
                        .and_then(|mesh| boundary(instance, mesh, &mut budget)),
                );
            }
        }
        let n = report.occurrences.len();
        report.expected_pairs = n.saturating_mul(n.saturating_sub(1)) / 2;
        if n < 2 {
            report.diagnostics.push(message(
                "Select at least two active, configuration-matching leaf occurrences",
            ));
        }
        'pairs: for i in 0..n {
            for j in i + 1..n {
                let first = &report.occurrences[i];
                let second = &report.occurrences[j];
                let a = &cache[&first.target.path];
                let b = &cache[&second.target.path];
                let path_cost = first
                    .target
                    .path
                    .iter()
                    .chain(&second.target.path)
                    .map(String::len)
                    .sum::<usize>()
                    .saturating_mul(12);
                if let Err(error) = scope_work(&mut budget, path_cost) {
                    report.diagnostics.push(error);
                    report.diagnostics.push(message(
                        "Remaining component pairs were not evaluated or published",
                    ));
                    break 'pairs;
                }
                let mut pair = Pair {
                    id: assembly_inspection::pair_id(&first.target, &second.target),
                    first: first.target.clone(),
                    second: second.target.clone(),
                    interference: InterferenceStatus::NotRequested,
                    common_solid_results: 0,
                    approximate_common_volume_mm3: None,
                    engineering_clearance_mm: None,
                    contact_residual_bound_mm: None,
                    boundary_distance: None,
                    diagnostics: vec![],
                };
                if definition.mode != Mode::BoundaryDistance
                    && let (Some(a), Some(b)) = (&a.solids, &b.solids)
                {
                    interference(
                        a,
                        b,
                        tolerance,
                        &mut pair,
                        &mut budget,
                        doc.settings.modeling.linear_mm,
                    );
                }
                if definition.mode != Mode::Interference {
                    match (a.boundary.as_ref(), b.boundary.as_ref()) {
                        (Some(Ok(a)), Some(Ok(b))) => {
                            match distance(a, b, clearance, &mut budget) {
                                Ok(distance) => pair.boundary_distance = Some(distance),
                                Err(error) => diagnostic(&mut pair, error),
                            }
                        }
                        (a, b) => diagnostic(
                            &mut pair,
                            a.and_then(|r| r.as_ref().err())
                                .or_else(|| b.and_then(|r| r.as_ref().err()))
                                .cloned()
                                .unwrap_or_else(|| message("Boundary distance is unavailable")),
                        ),
                    }
                }
                pair.diagnostics.sort();
                pair.diagnostics.dedup();
                if let Err(error) = charge_report(&pair, &mut budget, 1) {
                    report.diagnostics.push(error);
                    report.diagnostics.push(message(
                        "Remaining component pairs were not evaluated or published",
                    ));
                    break 'pairs;
                }
                report.pairs.push(pair);
            }
        }
        reports.insert(definition.id.clone(), report);
    }
    Ok(reports)
}
