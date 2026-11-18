//! Locate the corners of a straight-sided oblique profile on a principal plane.
//! Its bounding width and height alone do not locate the apex of a triangle.
use super::{
    AXES, Characteristic, Characteristics, FaceIndex, Kind, Vector, aligned, neighboring_faces,
    normal, vec, world_axis,
};
use crate::{
    Operation, Part,
    protocol::{EdgeCenter, MeshData},
};
use std::collections::BTreeSet;

pub(super) fn add(
    result: &mut Characteristics<'_>,
    part: &Part,
    mesh: &MeshData,
    faces: &FaceIndex,
) {
    let treatments: BTreeSet<_> = part
        .features
        .iter()
        .filter(|f| {
            matches!(
                f.operation,
                Operation::Chamfer { .. } | Operation::Fillet { .. }
            )
        })
        .map(|f| &f.id)
        .collect();
    let mut placed = BTreeSet::new();
    for edge in &mesh.edges {
        let Some(EdgeCenter::Line { start, end, .. }) = edge.center.as_ref() else {
            continue;
        };
        let Some(dir) = (vec(*end) - vec(*start)).try_normalize(result.tolerance) else {
            continue;
        };
        let Some(axial) = world_axis(dir) else {
            continue;
        };
        let Some([(a, _), (b, _)]) = neighboring_faces(faces, edge) else {
            continue;
        };
        let [one, two] = [result.faces[a], result.faces[b]];
        let (Some(n), Some(m)) = (normal(one), normal(two)) else {
            continue;
        };
        if aligned(n, m)
            || world_axis(n).is_some() && world_axis(m).is_some()
            || treatments.contains(&one.reference.producer.node)
            || treatments.contains(&two.reference.producer.node)
        {
            continue;
        }
        // Every point on the exact rail shares both profile coordinates. Split
        // edge pieces on the same two support planes identify one corner.
        let center = (vec(*start) + vec(*end)) * 0.5;
        for (axis, name) in AXES.iter().enumerate() {
            if axis == axial {
                continue;
            }
            let Some(extent) = &result.extents[axis] else {
                continue;
            };
            let location = center[axis] - extent.low;
            if location <= result.tolerance
                || extent.high - center[axis] <= result.tolerance
                || !placed.insert((axis, a.min(b), a.max(b)))
            {
                continue;
            }
            let mut base = center;
            base[axis] = extent.low;
            let mut support = vec![a, b];
            if let Some(index) = extent.low_faces.first() {
                support.push(*index);
            }
            let Some(id) = result.id(&format!("corner-{name}"), &[a, b]) else {
                continue;
            };
            result.dimensions.push(Characteristic {
                drivers: vec![],
                id,
                label: format!("Corner location {}", name.to_ascii_uppercase()),
                kind: Kind::Linear,
                nominal: location,
                witnesses: [base.into(), center.into()],
                angle_vertex: None,
                plane_normal: Some(Vector::ith(axial, 1.).into()),
                supports: result.refs(support),
            });
        }
    }
}
