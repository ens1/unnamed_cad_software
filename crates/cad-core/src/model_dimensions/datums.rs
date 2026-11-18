//! Datum labels are part-owned declarations. Suggestions rank compatible
//! analytic faces but never participate in a released datum reference frame.
use super::Report;
use crate::{
    Part, Plane, TopologyRef,
    protocol::{BodyKind, FaceRange, MeshData},
    reference_geometry::Axis,
};
use nalgebra::Vector3;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DatumCarrier {
    Plane { plane: Plane },
    Axis { axis: Axis },
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Datum {
    pub label: String,
    pub face: TopologyRef,
    pub anchor: [f64; 3],
    pub explicit: bool,
    pub carrier: DatumCarrier,
}

pub(super) fn stable_face(source: &TopologyRef) -> bool {
    !source.role.is_empty() && !source.role.starts_with("face:")
}

pub fn datum_eligible(face: &FaceRange) -> bool {
    face.body_kind == BodyKind::Solid
        && stable_face(&face.reference)
        && face
            .measurement
            .as_ref()
            .is_some_and(|measured| face.plane.is_some() || measured.cylinder.is_some())
}

fn from_face(label: &str, face: &FaceRange, explicit: bool) -> Option<Datum> {
    let measurement = face.measurement.as_ref()?;
    let carrier = if let Some(plane) = &face.plane {
        DatumCarrier::Plane {
            plane: plane.clone(),
        }
    } else {
        DatumCarrier::Axis {
            axis: measurement.cylinder.as_ref()?.axis.clone(),
        }
    };
    Some(Datum {
        label: label.into(),
        face: face.reference.clone(),
        anchor: measurement.witnesses[0][0],
        explicit,
        carrier,
    })
}

fn direction(face: &FaceRange) -> Option<Vector3<f64>> {
    if let Some(plane) = &face.plane {
        Vector3::from(plane.u)
            .cross(&Vector3::from(plane.v))
            .try_normalize(1e-12)
    } else {
        Some(
            face.measurement
                .as_ref()?
                .cylinder
                .as_ref()?
                .axis
                .direction
                .into(),
        )
    }
}

fn eligible_suggestion(face: &FaceRange, established: &[&FaceRange]) -> bool {
    let Some(dir) = direction(face) else {
        return false;
    };
    let Some(first) = established.first() else {
        return true;
    };
    let Some(primary) = direction(first) else {
        return false;
    };
    if face.plane.is_none() {
        // An axis normal to the primary plane locates a hole in both planar directions.
        return established.len() == 1
            && first.plane.is_some()
            && primary.dot(&dir).abs() > 0.999_999;
    }
    if primary.dot(&dir).abs() > 1e-6 {
        return false;
    }
    for other in established.iter().skip(1) {
        let Some(axis) = direction(other) else {
            return false;
        };
        if axis.dot(&dir).abs() > 1e-6 {
            return false;
        }
    }
    true
}

pub(super) fn resolve(part: &Part, mesh: &MeshData, report: &mut Report) {
    // A designation is part-local. Two bodies can carry the same construction role.
    let mut by_role = BTreeMap::new();
    for face in mesh
        .faces
        .iter()
        .filter(|f| f.reference.producer.definition == part.id)
    {
        by_role
            .entry((&face.reference.producer, &face.reference.role))
            .and_modify(|item| *item = None)
            .or_insert(Some(face));
    }
    let eligible: Vec<_> = by_role
        .values()
        .flatten()
        .copied()
        .filter(|f| datum_eligible(f))
        .collect();
    let unique_face = |target: &TopologyRef| {
        let canonical = crate::provenance::normalized_reference(target).ok()?;
        let mut matches = mesh.faces.iter().filter(|face| {
            crate::provenance::matches_normalized(&face.reference, target, &canonical)
                .unwrap_or(false)
        });
        let face = matches.next()?;
        (matches.next().is_none() && datum_eligible(face)).then_some(face)
    };
    let mut established = Vec::new();
    for (label, target) in &part.model_dimensions.datums {
        if let Some(face) = unique_face(target)
            && let Some(datum) = from_face(label, face, true)
        {
            established.push(face);
            report.datums.push(datum);
        } else {
            report.issues.push(format!("Datum {label} has lost its unique supporting face. Choose a replacement in Model Dimensions."));
        }
    }
    // Use area only to order suggestions. It never identifies or resolves an assigned datum.
    let area = |face: &FaceRange| face.reference.signature.as_ref().map_or(0., |s| s.measure);
    for label in ["A", "B", "C"] {
        if part.model_dimensions.datums.contains_key(label) {
            continue;
        }
        let candidate = eligible
            .iter()
            .copied()
            .filter(|face| eligible_suggestion(face, &established))
            .max_by(|a, b| {
                let preference = |f: &FaceRange| {
                    if established.len() == 1 {
                        f.measurement
                            .as_ref()
                            .and_then(|m| m.cylinder.as_ref())
                            .is_some_and(|c| c.internal)
                    } else {
                        f.plane.is_some()
                    }
                };
                preference(a)
                    .cmp(&preference(b))
                    .then_with(|| area(a).total_cmp(&area(b)))
                    .then_with(|| b.reference.producer.cmp(&a.reference.producer))
                    .then_with(|| b.reference.role.cmp(&a.reference.role))
            });
        if let Some(face) = candidate.and_then(|f| unique_face(&f.reference))
            && let Some(datum) = from_face(label, face, false)
        {
            established.push(face);
            report.datums.push(datum);
        }
    }
    report.datums.sort_by(|a, b| a.label.cmp(&b.label));
}
