//! Measure a bevel from its surviving rails back to the virtual sharp. An
//! authored chamfer distance alone cannot establish either finished setback.
use super::{Characteristic, Characteristics, Kind, MAX_VISITS, Vector, aligned, key, normal, vec};
use crate::{
    Operation, Part,
    protocol::{EdgeCenter, FaceRange, MeshData},
};
use std::collections::{BTreeMap, HashMap, HashSet};

#[derive(Clone)]
struct Rail {
    support: usize,
    a: Vector,
    b: Vector,
}

fn bevel_constructor(face: &FaceRange) -> Option<String> {
    let role = face.reference.role.strip_prefix("construction:")?;
    if role.len() > super::MAX_SOURCE_KEY {
        return None;
    }
    let value: serde_json::Value = serde_json::from_str(role).ok()?;
    let root = value.get("root").unwrap_or(&value);
    let purpose = root.get("purpose")?;
    (purpose.get("operation")?.as_str()? == "chamfer")
        .then(|| purpose.get("constructor")?.as_str().map(str::to_owned))?
}

fn offsets(
    a: &Rail,
    b: &Rail,
    faces: &[&FaceRange],
    tol: f64,
) -> Option<[(usize, [Vector; 2]); 2]> {
    if a.support == b.support {
        return None;
    }
    let first = normal(faces[a.support])?;
    let second = normal(faces[b.support])?;
    let axis = first.cross(&second).try_normalize(1e-8)?;
    let along_a = (a.b - a.a).try_normalize(tol)?;
    let along_b = (b.b - b.a).try_normalize(tol)?;
    if !aligned(along_a, axis) || !aligned(along_b, axis) {
        return None;
    }
    let project = |p: Vector| axis.dot(&(p - a.a));
    let interval = |r: &Rail| {
        let (s, t) = (project(r.a), project(r.b));
        [s.min(t), s.max(t)]
    };
    let [a0, a1] = interval(a);
    let [b0, b1] = interval(b);
    let (low, high) = (a0.max(b0), a1.min(b1));
    if high - low <= tol {
        return None;
    }
    let t = (low + high) * 0.5;
    let on_a = a.a + axis * t;
    let on_b = b.a + axis * (t - project(b.a));
    // Intersect the two supporting planes in the selected normal section.
    let across = second - first * first.dot(&second);
    let sharp = on_a - across * (second.dot(&(on_a - on_b)) / across.norm_squared());
    let spans = [(a.support, [sharp, on_a]), (b.support, [sharp, on_b])];
    spans
        .iter()
        .all(|(_, [p, q])| (p - q).norm().is_finite() && (p - q).norm() > tol)
        .then_some(spans)
}

pub(super) fn add(result: &mut Characteristics<'_>, part: &Part, mesh: &MeshData) {
    let chamfers: HashSet<_> = part
        .features
        .iter()
        .filter(|f| matches!(f.operation, Operation::Chamfer { .. }))
        .map(|f| f.id.as_str())
        .collect();
    if chamfers.is_empty() {
        return;
    }
    let mut rails = HashMap::<usize, Vec<Rail>>::new();
    for (index, face) in result.faces.iter().enumerate() {
        if !chamfers.contains(face.reference.producer.node.as_str()) {
            continue;
        }
        match bevel_constructor(face).as_deref() {
            Some("edge_setback" | "local_exact_planar_wedge") if face.plane.is_some() => {
                rails.insert(index, Vec::new());
            }
            Some("exact_conical_rim") => {
                result
                    .coverage
                    .insert("Circular chamfer setbacks require a radial section.".into());
            }
            _ => {}
        }
    }
    if rails.is_empty() {
        return;
    }
    let mut faces = BTreeMap::new();
    for (i, face) in result.faces.iter().enumerate() {
        faces
            .entry(key(face.body.as_ref(), &face.reference))
            .and_modify(|idx| *idx = None)
            .or_insert(Some(i));
    }
    for edge in &mesh.edges {
        let (Some(native), Some(EdgeCenter::Line { start, end, .. })) =
            (&edge.native, &edge.center)
        else {
            continue;
        };
        let [one, two] = native.boundaries.as_slice() else {
            continue;
        };
        let lookup = |r| faces.get(&key(edge.body.as_ref(), r)).copied().flatten();
        let (Some(a), Some(b)) = (lookup(&one.face), lookup(&two.face)) else {
            continue;
        };
        for (bevel, support) in [(a, b), (b, a)] {
            if let Some(entries) = rails.get_mut(&bevel)
                && result.faces[support].plane.is_some()
            {
                entries.push(Rail {
                    support,
                    a: vec(*start),
                    b: vec(*end),
                });
            }
        }
    }
    let mut budget = MAX_VISITS;
    for i in 0..result.faces.len() {
        let Some(entries) = rails.get(&i) else {
            continue;
        };
        let mut unique = None;
        let mut ambiguous = false;
        'pairs: for (n, a) in entries.iter().enumerate() {
            for b in entries.iter().skip(n + 1) {
                if budget == 0 {
                    result.coverage.insert(
                        "Additional chamfer setbacks exceed the recognition budget.".into(),
                    );
                    return;
                }
                budget -= 1;
                if let Some(spans) = offsets(a, b, result.faces, result.tolerance) {
                    if unique.is_some() {
                        ambiguous = true;
                        break 'pairs;
                    }
                    unique = Some(spans);
                }
            }
        }
        let Some(spans) = unique.filter(|_| !ambiguous) else {
            result
                .coverage
                .insert("A chamfer has no unique pair of surviving straight support rails.".into());
            continue;
        };
        for (support, [sharp, rail]) in spans {
            let Some(id) = result.id("chamfer-setback", &[i, support]) else {
                continue;
            };
            let span = rail - sharp;
            let axis = super::world_axis(span.normalize());
            let label = axis.map_or_else(
                || "Chamfer setback".into(),
                |a| format!("Chamfer setback ({})", ["X", "Y", "Z"][a]),
            );
            result.dimensions.push(Characteristic {
                id,
                label,
                kind: Kind::Linear,
                nominal_mm: span.norm(),
                witnesses: [sharp.into(), rail.into()],
                plane_normal: None,
                supports: result.refs([i, support]),
            });
        }
    }
}
