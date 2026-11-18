//! A rectangle width can come from a signed distance between two points on
//! its opposing sketch lines. Recognize the attachments and the relations
//! keeping those lines parallel to the measurement planes.
use super::{Characteristic, Descriptor, Driver, DrivingValue, Sources, axial, normal};
use crate::{
    Dimension, Operation, Profile, Sketch, SketchCurve, TopologyRef,
    protocol::{BodyKind, MeshData},
};
use nalgebra::Vector3;
use std::collections::BTreeSet;

#[derive(PartialEq, Eq)]
struct Side<'a> {
    profile: &'a str,
    sketch: &'a str,
    member: String,
    points: [&'a str; 2],
    local_axis: usize,
}

impl<'a> Sources<'a> {
    pub(super) fn sketch_span(
        &self,
        dim: &Characteristic,
        mesh: &MeshData,
        axis: usize,
    ) -> Option<Vec<DrivingValue>> {
        let [a, b] = dim.witnesses;
        let first = self.sketch_side(mesh, axis, a[axis], 0)?;
        let second = self.sketch_side(mesh, axis, b[axis], 1)?;
        if first.profile != second.profile
            || first.member == second.member
            || first.local_axis != second.local_axis
        {
            return None;
        }
        let sketch = self.part.sketches.get(first.sketch)?;
        let kind = ["distance_x", "distance_y"][first.local_axis];
        let mut candidates = sketch.constraints.iter().filter(|dim| {
            if dim.kind != kind || dim.driven {
                return false;
            }
            let [start, end] = dim.entities.as_slice() else {
                return false;
            };
            first.points.contains(&start.as_str()) && second.points.contains(&end.as_str())
                || first.points.contains(&end.as_str()) && second.points.contains(&start.as_str())
        });
        let constraint = candidates.next()?;
        if candidates.next().is_some() {
            return None;
        }
        let measured = crate::expression::value(
            self.document,
            &self.part.id,
            &constraint.value,
            Dimension::Length,
        )
        .ok()?;
        self.matches(measured.abs(), dim.nominal).then(|| {
            vec![DrivingValue {
                source: Driver::SketchDimension {
                    sketch: first.sketch.into(),
                    constraint: constraint.id.clone(),
                },
                factor: Some(measured.signum()),
            }]
        })
    }

    fn sketch_side(
        &self,
        mesh: &MeshData,
        axis: usize,
        coordinate: f64,
        side: usize,
    ) -> Option<Side<'a>> {
        let mut resolved = None;
        let mut attained = BTreeSet::new();
        let mut capped = BTreeSet::new();
        for face in mesh.faces.iter().filter(|f| f.body_kind == BodyKind::Solid) {
            if !self.matches(
                face.measurement.as_ref()?.witnesses[side][axis][axis],
                coordinate,
            ) {
                continue;
            }
            attained.insert(&face.body);
            let Some(plane) = &face.plane else { continue };
            if !axial(normal(plane)?, axis) {
                continue;
            }
            capped.insert(&face.body);
            let source = self.line_side(&face.reference, axis)?;
            if resolved.as_ref().is_some_and(|s| s != &source) {
                return None;
            }
            resolved = Some(source)
        }
        if attained != capped {
            return None;
        }
        resolved
    }

    fn line_side(&self, reference: &TopologyRef, axis: usize) -> Option<Side<'a>> {
        if reference.producer.definition != self.part.id {
            return None;
        }
        let Operation::Extrude {
            options,
            profile,
            direction,
            ..
        } = &self
            .features
            .get(reference.producer.node.as_str())?
            .operation
        else {
            return None;
        };
        if options.draft.is_some() || options.thin.is_some() {
            return None;
        }
        let plane = self.plane(profile)?;
        let n = normal(plane)?;
        if !options.profile_normal
            && Vector3::from(*direction)
                .try_normalize(1e-12)?
                .cross(&n)
                .norm()
                > 1e-9
        {
            return None;
        }
        let local_axis = [plane.u, plane.v].into_iter().position(|v| {
            Vector3::from(v)
                .try_normalize(1e-12)
                .is_some_and(|v| axial(v, axis))
        })?;
        let desc = Descriptor::new(reference)?;
        let root = desc.root();
        if root["purpose"]["operation"] != "extrusion"
            || !root["purpose"]["constructor"]["wall"].is_object()
        {
            return None;
        }
        let [input] = root.get("sources")?.as_array()?.as_slice() else {
            return None;
        };
        let (producer, intent) = desc.source(input, &self.part.id, "curve")?;
        if producer != profile.node || intent["purpose"]["operation"] != "sketch_member" {
            return None;
        }
        let member = intent["purpose"]["member"].as_str()?;
        let Operation::Profile {
            profile: Profile::Sketch { sketch },
            ..
        } = &self.features.get(producer)?.operation
        else {
            return None;
        };
        let sketch = self.part.sketches.get(sketch)?;
        let SketchCurve::Line { start, end } = &sketch.members.get(member)?.curve else {
            return None;
        };
        if !parallel_relation(sketch, member, start, end, local_axis) {
            return None;
        }
        Some(Side {
            profile: &profile.node,
            sketch: &sketch.id,
            member: member.into(),
            points: [start, end],
            local_axis,
        })
    }
}

fn parallel_relation(sketch: &Sketch, member: &str, start: &str, end: &str, axis: usize) -> bool {
    let kind = ["vertical", "horizontal"][axis];
    sketch.constraints.iter().any(|c| {
        !c.driven
            && c.kind == kind
            && match c.entities.as_slice() {
                [line] => line == member,
                [a, b] => a == start && b == end || a == end && b == start,
                _ => false,
            }
    })
}
