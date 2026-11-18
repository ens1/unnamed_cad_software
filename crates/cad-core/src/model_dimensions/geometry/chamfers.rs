//! Measure a bevel from its surviving rails back to the virtual sharp. An
//! authored chamfer distance alone cannot establish either finished setback.
use super::{
    Characteristic, Characteristics, FaceIndex, Kind, MAX_VISITS, Vector, aligned,
    neighboring_faces, normal, vec,
};
use crate::{
    Operation, Part,
    protocol::{EdgeCenter, FaceRange, MeshData},
};
use std::collections::HashMap;

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

pub(super) fn add(
    result: &mut Characteristics<'_>,
    part: &Part,
    mesh: &MeshData,
    index: &FaceIndex,
) {
    let chamfers: HashMap<_, _> = part
        .features
        .iter()
        .filter_map(|feature| {
            if let Operation::Chamfer { options, .. } = &feature.operation {
                Some((feature.id.as_str(), options.as_ref()))
            } else {
                None
            }
        })
        .collect();
    if chamfers.is_empty() {
        return;
    }
    let mut rails = HashMap::<usize, Vec<Rail>>::new();
    for (index, face) in result.faces.iter().enumerate() {
        if !chamfers.contains_key(face.reference.producer.node.as_str()) {
            continue;
        }
        if face.plane.is_some()
            && matches!(
                bevel_constructor(face).as_deref(),
                Some("edge_setback" | "local_exact_planar_wedge")
            )
        {
            rails.insert(index, Vec::new());
        }
    }
    if rails.is_empty() {
        return;
    }
    for edge in &mesh.edges {
        let Some(EdgeCenter::Line { start, end, .. }) = &edge.center else {
            continue;
        };
        let Some([(a, _), (b, _)]) = neighboring_faces(index, edge) else {
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
        let options = chamfers[result.faces[i].reference.producer.node.as_str()];
        let first = if options.method == crate::edge_features::ChamferMethod::DistanceAngle {
            first_support(&spans, result.faces, options)
        } else {
            None
        };
        for (index, (support, [sharp, rail])) in spans.iter().copied().enumerate() {
            if first.is_some_and(|selected| selected != index) {
                continue;
            }
            if first == Some(index) {
                let other_rail = spans[1 - index].1[1];
                let incoming = sharp - rail;
                let outgoing = other_rail - rail;
                let section_normal = incoming.cross(&outgoing);
                let angle = section_normal.norm().atan2(incoming.dot(&outgoing));
                if let Some(direction) = section_normal.try_normalize(1e-14)
                    && let Some(id) = result.id("chamfer-angle", &[i, support])
                {
                    result.dimensions.push(Characteristic {
                        id,
                        label: "Chamfer angle".into(),
                        kind: Kind::Angle,
                        nominal: angle,
                        angle_vertex: Some(rail.into()),
                        witnesses: [sharp.into(), other_rail.into()],
                        plane_normal: Some(direction.into()),
                        supports: result.refs([i, support]),
                    });
                }
            }
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
                angle_vertex: None,
                nominal: span.norm(),
                witnesses: [sharp.into(), rail.into()],
                plane_normal: None,
                supports: result.refs([i, support]),
            });
        }
    }
}

/// The selected setback side is explicit, or follows the feature's ordered
/// outward normals. Retain both measured lengths if a saved side cannot be resolved.
fn first_support(
    spans: &[(usize, [Vector; 2]); 2],
    faces: &[&FaceRange],
    options: &crate::edge_features::ChamferOptions,
) -> Option<usize> {
    let [a, b] = spans.map(|(index, _)| faces[index]);
    let index = if let Some(reference) = &options.first_face {
        let matches =
            [a, b].map(|f| crate::provenance::equivalent(&f.reference, reference).unwrap_or(false));
        match matches {
            [true, false] => 0,
            [false, true] => 1,
            _ => return None,
        }
    } else {
        let a = normal(a)?;
        let b = normal(b)?;
        usize::from(
            a.x.total_cmp(&b.x)
                .then_with(|| a.y.total_cmp(&b.y))
                .then_with(|| a.z.total_cmp(&b.z))
                .is_gt(),
        )
    };
    Some(if options.flip { 1 - index } else { index })
}
