//! Links from finished measurements back to authored dimensions. The measured
//! value stays authoritative even when no editable source can be established.
mod sketch_spans;
use super::Characteristic;
use crate::{
    Dimension, Document, Expr, Feature, Id, Operation, Part, Profile, SketchCurve, TopologyRef,
    protocol::{BodyKind, EvaluatedSketch, MeshData},
};
use nalgebra::Vector3;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "source", rename_all = "snake_case")]
pub enum Driver {
    Feature { feature: Id, path: String },
    SketchDimension { sketch: Id, constraint: Id },
    CircleRadius { sketch: Id, member: Id },
}

impl Driver {
    fn field(feature: &str, path: &str) -> Self {
        Self::Feature {
            feature: feature.into(),
            path: path.into(),
        }
    }
}

struct Sources<'a> {
    part: &'a Part,
    features: BTreeMap<&'a str, &'a Feature>,
    document: &'a Document,
    mesh_key: &'a str,
    sketches: &'a BTreeMap<String, EvaluatedSketch>,
}

pub(super) fn resolve(
    dimensions: &mut [Characteristic],
    part: &Part,
    mesh: &MeshData,
    document: &Document,
    mesh_key: &str,
    sketches: &BTreeMap<String, EvaluatedSketch>,
) {
    let sources = Sources {
        part,
        document,
        mesh_key,
        sketches,
        features: part
            .features
            .iter()
            .filter(|f| !f.suppressed)
            .map(|f| (f.id.as_str(), f))
            .collect(),
    };
    for dimension in dimensions {
        let drivers = match dimension.id.as_str() {
            "overall/x" => sources
                .extent(dimension, mesh, 0)
                .or_else(|| sources.sketch_span(dimension, mesh, 0)),
            "overall/y" => sources
                .extent(dimension, mesh, 1)
                .or_else(|| sources.sketch_span(dimension, mesh, 1)),
            "overall/z" => sources
                .extent(dimension, mesh, 2)
                .or_else(|| sources.sketch_span(dimension, mesh, 2)),
            key if key.starts_with("depth|")
                || key.starts_with("span|")
                || key.starts_with("boss-height|") =>
            {
                sources.span(dimension)
            }
            _ => sources.local(dimension).map(|source| {
                vec![DrivingValue {
                    source,
                    factor: None,
                }]
            }),
        };
        dimension.drivers = drivers.unwrap_or_default();
    }
}

type Terms = BTreeMap<Driver, f64>;

/// Signed response of this measured distance to an axial source, in the
/// neighborhood of the current topology. Other measurements link directly.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DrivingValue {
    pub source: Driver,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub factor: Option<f64>,
}

fn difference(lower: Terms, mut upper: Terms) -> Vec<DrivingValue> {
    for (driver, value) in lower {
        *upper.entry(driver).or_default() -= value;
    }
    upper
        .into_iter()
        .filter(|(_, v)| v.abs() > 1e-9)
        .map(|(source, value)| DrivingValue {
            source,
            factor: Some(value),
        })
        .collect()
}

impl Sources<'_> {
    fn matches(&self, a: f64, b: f64) -> bool {
        (a - b).abs()
            <= self
                .document
                .settings
                .modeling
                .linear_mm
                .min(1e-8 * a.abs().max(b.abs()).max(1.))
    }

    fn plane(&self, profile: &crate::NodeRef) -> Option<&crate::Plane> {
        if profile.definition != self.part.id {
            return None;
        }
        if let Some(sketch) = self
            .sketches
            .get(&format!("{}::{}", self.mesh_key, profile.node))
        {
            return Some(&sketch.plane);
        }
        match &self.features.get(profile.node.as_str())?.operation {
            Operation::Profile {
                plane,
                plane_reference: None,
                ..
            } => Some(plane),
            _ => None,
        }
    }

    fn extent(
        &self,
        dimension: &Characteristic,
        mesh: &MeshData,
        axis: usize,
    ) -> Option<Vec<DrivingValue>> {
        let mut sides = [Terms::new(), Terms::new()];
        for (side, terms) in sides.iter_mut().enumerate() {
            let coordinate = dimension.witnesses[side][axis];
            let mut resolved = None;
            let mut attained = BTreeSet::new();
            let mut capped = BTreeSet::new();
            let mut budget = 8192;
            for face in mesh.faces.iter().filter(|f| f.body_kind == BodyKind::Solid) {
                let native = face.measurement.as_ref()?;
                if !self.matches(native.witnesses[side][axis][axis], coordinate) {
                    continue;
                }
                attained.insert(&face.body);
                let Some(plane) = &face.plane else { continue };
                let normal = normal(plane)?;
                if !axial(normal, axis) {
                    continue;
                }
                capped.insert(&face.body);
                let local = self.cap_terms(&face.reference, axis, coordinate, &mut budget, 0)?;
                if resolved.as_ref().is_some_and(|other| other != &local) {
                    return None;
                }
                resolved = Some(local);
            }
            if attained != capped {
                return None;
            }
            *terms = resolved?;
        }
        let [lower, upper] = sides;
        Some(difference(lower, upper))
    }

    fn span(&self, dimension: &Characteristic) -> Option<Vec<DrivingValue>> {
        // geometry::span records both end faces in witness order followed by
        // the adjoining wall. Incomplete selection evidence cannot drive an edit.
        let [first, second, _] = dimension.supports.as_slice() else {
            return None;
        };
        let [a, b] = dimension.witnesses;
        let n = (Vector3::from(a) - Vector3::from(b)).try_normalize(1e-12)?;
        let axis = (0..3).find(|&i| axial(n, i))?;
        let mut budget = 8192;
        let a_terms = self.cap_terms(first, axis, a[axis], &mut budget, 0)?;
        let b_terms = self.cap_terms(second, axis, b[axis], &mut budget, 0)?;
        Some(if a[axis] < b[axis] {
            difference(a_terms, b_terms)
        } else {
            difference(b_terms, a_terms)
        })
    }

    fn cap_terms(
        &self,
        face: &TopologyRef,
        axis: usize,
        coordinate: f64,
        budget: &mut usize,
        depth: usize,
    ) -> Option<Terms> {
        if depth >= 64 || face.topology != "face" || face.producer.definition != self.part.id {
            return None;
        }
        let desc = Descriptor::new(face)?;
        let mut found = None;
        self.walk_caps(
            desc.root(),
            &face.producer.node,
            &desc,
            axis,
            coordinate,
            budget,
            depth,
            &mut found,
        )?;
        found
    }

    #[allow(clippy::too_many_arguments)]
    fn walk_caps(
        &self,
        intent: &Value,
        feature_id: &str,
        desc: &Descriptor,
        axis: usize,
        coordinate: f64,
        budget: &mut usize,
        depth: usize,
        found: &mut Option<Terms>,
    ) -> Option<()> {
        if depth >= 64 {
            return None;
        }
        *budget = budget.checked_sub(1)?;
        let purpose = intent.get("purpose")?;
        match purpose.get("operation")?.as_str()? {
            "extrusion" => {
                let Operation::Extrude {
                    profile,
                    distance,
                    direction,
                    options,
                    ..
                } = &self.features.get(feature_id)?.operation
                else {
                    return None;
                };
                let plane = self.plane(profile)?;
                let normal = normal(plane)?;
                let direction = if options.profile_normal {
                    normal * if options.reverse_direction { -1. } else { 1. }
                } else {
                    Vector3::from(*direction).try_normalize(1e-12)?
                };
                if !axial(normal, axis) || !axial(direction, axis) || options.draft.is_some() {
                    return None;
                }
                let constructor = purpose.get("constructor")?;
                if constructor.get("wall").is_some() {
                    return Some(());
                }
                let end = constructor.get("end")?.as_str()?;
                if !options.start.is_sketch_plane()
                    || options.direction2.is_some()
                    || options.reverse_distance.is_some()
                {
                    return None;
                }
                let coefficient = match (&options.end, end) {
                    (crate::ExtrudeEnd::Blind, "start") => 0.,
                    (crate::ExtrudeEnd::Blind, "end") => 1.,
                    (crate::ExtrudeEnd::MidPlane, "start") => -0.5,
                    (crate::ExtrudeEnd::MidPlane, "end") => 0.5,
                    _ => return None,
                };
                let length = |expr: &Expr| {
                    crate::expression::value(self.document, &self.part.id, expr, Dimension::Length)
                        .ok()
                };
                let start = length(&options.start_offset)?;
                let origin = plane.origin[axis];
                let expected = origin + direction[axis] * (start + coefficient * length(distance)?);
                if !self.matches(expected, coordinate) {
                    return Some(());
                }
                let Operation::Profile {
                    plane_reference, ..
                } = &self.features.get(profile.node.as_str())?.operation
                else {
                    return None;
                };
                let mut terms = match plane_reference {
                    None => Terms::new(),
                    Some(attachment) => {
                        self.cap_terms(attachment, axis, origin, budget, depth + 1)?
                    }
                };
                if coefficient != 0. {
                    *terms
                        .entry(Driver::field(feature_id, "/distance"))
                        .or_default() += coefficient * direction[axis];
                }
                // A nonzero offset or authored equation is an intentional extra
                // control. Omit the zero default from the ordinary blind edit.
                if start != 0.
                    || !crate::expression::references(&options.start_offset.0)
                        .ok()?
                        .is_empty()
                {
                    *terms
                        .entry(Driver::field(feature_id, "/options/start_offset"))
                        .or_default() += direction[axis];
                }
                if found.as_ref().is_some_and(|other| other != &terms) {
                    return None;
                }
                *found = Some(terms);
            }
            "extrusion_cut" | "prismatic_boolean" => {
                // Partition faces carry both surface and clipping ancestry. Only
                // axial caps whose evaluated station is the measured plane can
                // supply this coordinate. All matching paths must agree.
                for source in intent.get("sources")?.as_array()? {
                    let (producer, intent) = desc.source(source, &self.part.id, "face")?;
                    self.walk_caps(
                        intent,
                        producer,
                        desc,
                        axis,
                        coordinate,
                        budget,
                        depth + 1,
                        found,
                    )?;
                }
            }
            _ => return None,
        }
        Some(())
    }

    fn local(&self, dim: &Characteristic) -> Option<Driver> {
        let (family, encoded) = dim.id.split_once('|')?;
        if family.starts_with("drill-") || family == "drilled-depth" {
            let records: Vec<Value> = serde_json::from_str(encoded).ok()?;
            let [position] = records.as_slice() else {
                return None;
            };
            if position["topology"] != "drilled_position"
                || position["producer"]["definition"] != self.part.id
            {
                return None;
            }
            let node = position["producer"]["node"].as_str()?;
            let Operation::DrilledHoles { definition } = &self.features.get(node)?.operation else {
                return None;
            };
            let (path, expr, angle) = match (family, &definition.head) {
                ("drill-pilot", _) => (
                    if definition.thread.is_some() {
                        "/definition/thread/tap_drill_diameter"
                    } else {
                        "/definition/diameter"
                    },
                    definition.bore_diameter(),
                    false,
                ),
                ("drill-counterbore", crate::holes::Head::Counterbore { diameter, .. })
                | ("drill-countersink", crate::holes::Head::Countersink { diameter, .. }) => {
                    ("/definition/head/diameter", diameter, false)
                }
                ("drill-countersink-angle", crate::holes::Head::Countersink { angle, .. }) => {
                    ("/definition/head/angle", angle, true)
                }
                ("drilled-depth", _)
                    if definition.end == crate::holes::End::Blind
                        && self.drill_entry(dim, definition)? =>
                {
                    ("/definition/depth", &definition.depth, false)
                }
                ("drill-counterbore-depth", crate::holes::Head::Counterbore { depth, .. })
                    if self.drill_entry(dim, definition)? =>
                {
                    ("/definition/head/depth", depth, false)
                }
                _ => return None,
            };
            let value = crate::expression::value(
                self.document,
                &self.part.id,
                expr,
                if angle {
                    Dimension::Angle
                } else {
                    Dimension::Length
                },
            )
            .ok()?;
            return self
                .matches(value, dim.nominal)
                .then(|| Driver::field(node, path));
        }
        if !matches!(family, "diameter" | "radius") {
            return None;
        }
        // The report can omit selectable faces to stay within its byte budget.
        // Every unique measured carrier must participate in source agreement.
        let measured: Vec<Value> = serde_json::from_str(encoded).ok()?;
        let available = dim
            .supports
            .iter()
            .map(|r| (&r.producer, &r.topology, &r.role))
            .collect::<BTreeSet<_>>();
        if available.len() != measured.len() {
            return None;
        }
        let mut found = None;
        for support in &dim.supports {
            if support.producer.definition != self.part.id {
                return None;
            }
            let f = self.features.get(support.producer.node.as_str())?;
            let driver = match &f.operation {
                Operation::Fillet { radius, .. }
                    if family == "radius"
                        && Descriptor::new(support).is_some_and(|desc| {
                            desc.root()["purpose"]["operation"] == "fillet"
                                && desc.root()["purpose"]["constructor"] == "circular_cylinder"
                        })
                        && self.matches(
                            crate::expression::value(
                                self.document,
                                &self.part.id,
                                radius,
                                Dimension::Length,
                            )
                            .ok()?,
                            dim.nominal,
                        ) =>
                {
                    Driver::field(&f.id, "/radius")
                }
                Operation::Extrude { .. } if family == "diameter" => self.circle(support)?,
                _ => return None,
            };
            if found.as_ref().is_some_and(|other| other != &driver) {
                return None;
            }
            found = Some(driver);
        }
        found
    }

    fn drill_entry(&self, dim: &Characteristic, drill: &crate::holes::Definition) -> Option<bool> {
        let plane = self.plane(&drill.positions)?;
        let n = normal(plane)?;
        let offset = crate::expression::value(
            self.document,
            &self.part.id,
            &drill.start_offset,
            Dimension::Length,
        )
        .ok()?;
        if offset != 0. {
            return Some(false);
        }
        Some(dim.witnesses.iter().any(|p| {
            self.matches(
                (Vector3::from(*p) - Vector3::from(plane.origin)).dot(&n),
                0.,
            )
        }))
    }

    fn circle(&self, face: &TopologyRef) -> Option<Driver> {
        let desc = Descriptor::new(face)?;
        let mut drivers = BTreeSet::new();
        self.circle_candidates(
            &desc,
            desc.root(),
            &face.producer.node,
            &mut drivers,
            &mut 1024,
            0,
        )?;
        (drivers.len() == 1).then(|| drivers.pop_first()).flatten()
    }

    fn circle_candidates(
        &self,
        desc: &Descriptor,
        intent: &Value,
        producer: &str,
        drivers: &mut BTreeSet<Driver>,
        budget: &mut usize,
        depth: usize,
    ) -> Option<()> {
        if depth >= 64 {
            return None;
        }
        *budget = budget.checked_sub(1)?;
        let purpose = &intent["purpose"];
        let inputs = intent.get("sources")?.as_array()?;
        match purpose["operation"].as_str()? {
            "extrusion_cut" | "prismatic_boolean" => {
                for input in inputs {
                    let (producer, intent) = desc.source(input, &self.part.id, "face")?;
                    self.circle_candidates(desc, intent, producer, drivers, budget, depth + 1)?;
                }
                return Some(());
            }
            "extrusion" if purpose["constructor"]["end"].is_string() => return Some(()),
            "extrusion" if purpose["constructor"]["wall"].is_object() => {}
            _ => return None,
        }
        let Operation::Extrude {
            profile, options, ..
        } = &self.features.get(producer)?.operation
        else {
            return None;
        };
        if options.draft.is_some() || options.thin.is_some() {
            return None;
        }
        let [input] = inputs.as_slice() else {
            return None;
        };
        let (producer, intent) = desc.source(input, &self.part.id, "curve")?;
        if producer != profile.node
            || profile.definition != self.part.id
            || intent["purpose"]["operation"] != "sketch_member"
        {
            return None;
        }
        if let Some(driver) = self.circle_member(producer, intent)? {
            drivers.insert(driver);
        }
        Some(())
    }

    // Some(None) denotes a provably planar wall in a partition's clipping ancestry.
    fn circle_member(&self, producer: &str, intent: &Value) -> Option<Option<Driver>> {
        let member = intent["purpose"]["member"].as_str()?;
        let Operation::Profile { profile, .. } = &self.features.get(producer)?.operation else {
            return None;
        };
        let sketch = match profile {
            Profile::Circle { .. } if member == "circle" => {
                return Some(Some(Driver::field(producer, "/profile/radius")));
            }
            Profile::Polygon { .. } => return Some(None),
            Profile::Sketch { sketch } => self.part.sketches.get(sketch)?,
            _ => return None,
        };
        if let Some(index) = member
            .strip_prefix("legacy-profile-edge-")
            .and_then(|s| s.parse::<usize>().ok())
            && index < sketch.profile_order.len()
        {
            return Some(None);
        }
        if let Some(index) = member
            .strip_prefix("legacy-segment-")
            .and_then(|s| s.parse::<usize>().ok())
            && index < sketch.segments.len()
        {
            return Some(None);
        }
        let curve = &sketch.members.get(member)?.curve;
        if matches!(curve, SketchCurve::Line { .. }) {
            return Some(None);
        }
        if !matches!(curve, SketchCurve::Circle { .. } | SketchCurve::Arc { .. }) {
            return None;
        }
        if sketch.conversions.contains_key(member)
            || sketch
                .offsets
                .values()
                .any(|o| o.members.iter().any(|m| m.member == member))
            || sketch.fillets.values().any(|f| f.arc == member)
            || !sketch.transforms.is_empty()
            || !sketch.block_instances.is_empty()
        {
            return None;
        }
        let mut driving = sketch
            .constraints
            .iter()
            .filter(|c| !c.driven && c.entities.iter().any(|e| e == member));
        if let Some(c) = driving.next() {
            if !matches!(c.kind.as_str(), "radius" | "diameter")
                || c.entities.len() != 1
                || driving.next().is_some()
            {
                return None;
            }
            return Some(Some(Driver::SketchDimension {
                sketch: sketch.id.clone(),
                constraint: c.id.clone(),
            }));
        }
        matches!(curve, SketchCurve::Circle { .. }).then(|| {
            Some(Driver::CircleRadius {
                sketch: sketch.id.clone(),
                member: member.into(),
            })
        })
    }
}

fn normal(plane: &crate::Plane) -> Option<Vector3<f64>> {
    Vector3::from(plane.u)
        .cross(&Vector3::from(plane.v))
        .try_normalize(1e-12)
}
fn axial(n: Vector3<f64>, axis: usize) -> bool {
    n.cross(&Vector3::ith(axis, 1.)).norm() <= 1e-9
}

// Canonicalizing first gives legacy inline references and compact source tables
// the same interpretation. Follow only explicit constructor sources, never the
// stock/context records embedded in a constructor's purpose.
struct Descriptor(Value);
impl Descriptor {
    fn new(reference: &TopologyRef) -> Option<Self> {
        if !reference.role.starts_with("construction:") || reference.role.len() > 128 * 1024 {
            return None;
        }
        let mut normalized = crate::provenance::normalized_reference(reference).ok()?;
        Some(Self(normalized.get_mut("role")?.get_mut("intent")?.take()))
    }
    fn root(&self) -> &Value {
        self.0.get("root").unwrap_or(&self.0)
    }
    fn source<'a>(
        &'a self,
        entry: &'a Value,
        owner: &str,
        topology: &str,
    ) -> Option<(&'a str, &'a Value)> {
        let source = if let Some(index) = entry.get("$tcad.source") {
            self.0
                .get("references")?
                .get(usize::try_from(index.as_u64()?).ok()?)?
        } else {
            entry
        };
        if source["topology"] != topology
            || source["producer"]["definition"] != owner
            || source["role"]["contract"] != "construction"
        {
            return None;
        }
        Some((
            source["producer"]["node"].as_str()?,
            source["role"].get("intent")?,
        ))
    }
}
