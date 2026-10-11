//! Derived section connectivity over authoritative sketch identities and controls.
//!
//! Connections inherit authored relations and pattern provenance. An isolated pair
//! of material endpoints may additionally share a section vertex within the stated
//! millimetre tolerance. This never changes saved points, members or constraints;
//! centers, intermediate controls and ambiguous endpoint clusters are not healed.
use crate::{CadError, Id, Result, Sketch, SketchCurve};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const CONTRACT_VERSION: u32 = 1;
pub const DEFAULT_TOLERANCE_MM: f64 = 1e-6;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContourStatus {
    Closed,
    Open,
    Branched,
}

/// Connectivity does not certify nonintersection, positive area or hole nesting.
/// Those checks remain part of current native section validation.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Contour {
    pub members: Vec<Id>,
    pub status: ContourStatus,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Topology {
    pub point_aliases: BTreeMap<Id, Id>,
    pub contours: Vec<Contour>,
}

fn invalid(message: &str) -> CadError {
    CadError::Invalid(message.into())
}

struct Member {
    id: Id,
    endpoints: Option<[Id; 2]>,
    curve: SketchCurve,
}

fn material_members(sketch: &Sketch) -> Result<Vec<Member>> {
    let suppressed = crate::sketch_transform::suppressed_members(sketch);
    sketch
        .members
        .iter()
        .filter(|(id, member)| !member.construction && !suppressed.contains(*id))
        .map(|(id, _)| {
            let curve = crate::sketch_edit::effective_curve(sketch, id)?;
            let endpoints = match &curve {
                SketchCurve::Circle { .. } | SketchCurve::Ellipse { .. } => None,
                _ => {
                    let points = curve.points();
                    if points.len() < 2 {
                        return Err(invalid("Sketch contour member needs endpoint controls"));
                    }
                    Some([points[0].into(), points[points.len() - 1].into()])
                }
            };
            if curve
                .points()
                .iter()
                .any(|point| !sketch.points.contains_key(*point))
            {
                return Err(invalid("Sketch contour member has a missing point"));
            }
            Ok(Member {
                id: id.clone(),
                endpoints,
                curve,
            })
        })
        .collect()
}

fn join(
    sketch: &Sketch,
    aliases: &mut BTreeMap<Id, Id>,
    first: &str,
    second: &str,
    tolerance: f64,
) -> Result<bool> {
    let first = aliases
        .get(first)
        .cloned()
        .ok_or_else(|| invalid("Sketch topology source point is missing"))?;
    let second = aliases
        .get(second)
        .cloned()
        .ok_or_else(|| invalid("Sketch topology output point is missing"))?;
    if first == second {
        return Ok(false);
    }
    let points = aliases
        .iter()
        .filter(|(_, canonical)| **canonical == first || **canonical == second)
        .map(|(point, _)| sketch.points[point])
        .collect::<Vec<_>>();
    for (index, p) in points.iter().enumerate() {
        if points[index + 1..]
            .iter()
            .any(|q| (p[0] - q[0]).hypot(p[1] - q[1]) > tolerance)
        {
            return Err(invalid(
                "Sketch section connections exceed the endpoint tolerance",
            ));
        }
    }
    let (canonical, other) = if first < second {
        (first, second)
    } else {
        (second, first)
    };
    for value in aliases.values_mut() {
        if *value == other {
            *value = canonical.clone();
        }
    }
    Ok(true)
}

fn inherit_patterns(sketch: &Sketch, aliases: &mut BTreeMap<Id, Id>, tolerance: f64) -> Result<()> {
    // Structural equality is a provenance proof: the same source point, saved
    // transform and instance placement must have the same derived destination.
    // Different expressions are not assumed to mean the same transformation.
    let mut groups = BTreeMap::<String, Vec<&crate::sketch_transform::Instance>>::new();
    for pattern in sketch.transforms.values() {
        for (index, instance) in pattern.instances.iter().enumerate() {
            if instance.suppressed {
                continue;
            }
            let key = serde_json::to_string(&serde_json::json!({
                "transform": pattern.transform,
                "instance": index,
                "placement": instance.placement,
            }))?;
            groups.entry(key).or_default().push(instance);
        }
    }
    // Sources can themselves be instances. Propagate their canonical identity
    // until no further instance connections are established.
    loop {
        let mut changed = false;
        for instances in groups.values() {
            let mut outputs = BTreeMap::<Id, Id>::new();
            for instance in instances {
                for (source, target) in &instance.points {
                    let source = aliases
                        .get(source)
                        .cloned()
                        .ok_or_else(|| invalid("Sketch pattern source point is missing"))?;
                    if !aliases.contains_key(target) {
                        return Err(invalid("Sketch pattern output point is missing"));
                    }
                    if let Some(previous) = outputs.get(&source) {
                        changed |= join(sketch, aliases, previous, target, tolerance)?;
                    } else {
                        outputs.insert(source, target.clone());
                    }
                }
            }
        }
        if !changed {
            return Ok(());
        }
    }
}

fn endpoint_degrees(
    sketch: &Sketch,
    members: &[Member],
    aliases: &BTreeMap<Id, Id>,
) -> (BTreeMap<Id, usize>, BTreeMap<Id, BTreeSet<Id>>) {
    let mut degrees = BTreeMap::<Id, usize>::new();
    let mut owners = BTreeMap::<Id, BTreeSet<Id>>::new();
    for member in members {
        if let Some(endpoints) = &member.endpoints {
            for point in endpoints {
                let canonical = &aliases[point];
                *degrees.entry(canonical.clone()).or_default() += 1;
                owners
                    .entry(canonical.clone())
                    .or_default()
                    .insert(member.id.clone());
            }
        }
    }
    if sketch.profile_order.is_empty() {
        for (index, segment) in sketch.segments.iter().enumerate() {
            for point in segment {
                if let Some(canonical) = aliases.get(point) {
                    *degrees.entry(canonical.clone()).or_default() += 1;
                    owners
                        .entry(canonical.clone())
                        .or_default()
                        .insert(format!("legacy-segment-{index}"));
                }
            }
        }
    }
    (degrees, owners)
}

fn on_line_interior(p: [f64; 2], a: [f64; 2], b: [f64; 2], tolerance: f64) -> bool {
    let delta = [b[0] - a[0], b[1] - a[1]];
    let length = delta[0].hypot(delta[1]);
    if !length.is_finite() || length <= 2. * tolerance {
        return false;
    }
    let direction = delta.map(|value| value / length);
    let along = (p[0] - a[0]) * direction[0] + (p[1] - a[1]) * direction[1];
    let across = (p[0] - a[0]) * direction[1] - (p[1] - a[1]) * direction[0];
    along > tolerance && along < length - tolerance && across.abs() <= tolerance
}

fn heal_isolated_endpoints(
    sketch: &Sketch,
    members: &[Member],
    aliases: &mut BTreeMap<Id, Id>,
    tolerance: f64,
) -> Result<()> {
    let (degrees, owners) = endpoint_degrees(sketch, members, aliases);
    let endpoints = degrees
        .iter()
        .filter(|(_, degree)| **degree == 1)
        .map(|(point, _)| point.clone())
        .collect::<Vec<_>>();
    let mut neighbors = BTreeMap::<Id, Vec<Id>>::new();
    let mut blocked = BTreeSet::<Id>::new();
    for endpoint in &endpoints {
        let p = sketch.points[endpoint];
        if degrees.iter().any(|(other, degree)| {
            *degree > 1 && {
                let q = sketch.points[other];
                (p[0] - q[0]).hypot(p[1] - q[1]) <= tolerance
            }
        }) || members.iter().any(|member| {
            if owners[endpoint].contains(&member.id) {
                return false;
            }
            if let SketchCurve::Line { start, end } = &member.curve {
                on_line_interior(p, sketch.points[start], sketch.points[end], tolerance)
            } else {
                false
            }
        }) || (sketch.profile_order.is_empty()
            && sketch.segments.iter().any(|segment| {
                on_line_interior(
                    p,
                    sketch.points[&segment[0]],
                    sketch.points[&segment[1]],
                    tolerance,
                )
            }))
        {
            blocked.insert(endpoint.clone());
        }
    }
    for (index, first) in endpoints.iter().enumerate() {
        for second in &endpoints[index + 1..] {
            if !owners[first].is_disjoint(&owners[second]) {
                continue;
            }
            let (p, q) = (sketch.points[first], sketch.points[second]);
            if (p[0] - q[0]).hypot(p[1] - q[1]) <= tolerance {
                neighbors
                    .entry(first.clone())
                    .or_default()
                    .push(second.clone());
                neighbors
                    .entry(second.clone())
                    .or_default()
                    .push(first.clone());
            }
        }
    }
    for (first, choices) in &neighbors {
        if blocked.contains(first) || choices.len() != 1 {
            continue;
        }
        let second = &choices[0];
        if first >= second
            || blocked.contains(second)
            || neighbors
                .get(second)
                .is_none_or(|choices| choices.len() != 1)
        {
            continue;
        }
        join(sketch, aliases, first, second, tolerance)?;
    }
    Ok(())
}

/// Section vertex identity, including isolated tolerance healing. Authoritative
/// sketch coordinates and solver relation identities remain unchanged.
pub fn point_aliases(sketch: &Sketch, tolerance_mm: f64) -> Result<BTreeMap<Id, Id>> {
    let mut aliases = crate::sketch_relations::topological_point_aliases(sketch, tolerance_mm)?;
    let mut authored_clusters = BTreeMap::<&Id, Vec<[f64; 2]>>::new();
    for (point, canonical) in &aliases {
        authored_clusters
            .entry(canonical)
            .or_default()
            .push(sketch.points[point]);
    }
    for points in authored_clusters.values() {
        for (index, p) in points.iter().enumerate() {
            if points[index + 1..]
                .iter()
                .any(|q| (p[0] - q[0]).hypot(p[1] - q[1]) > tolerance_mm)
            {
                return Err(invalid(
                    "Authored sketch endpoint connections exceed the section tolerance",
                ));
            }
        }
    }
    if sketch
        .segments
        .iter()
        .flatten()
        .chain(&sketch.profile_order)
        .any(|point| !sketch.points.contains_key(point))
    {
        return Err(invalid("Legacy sketch contour has a missing endpoint"));
    }
    let members = material_members(sketch)?;
    inherit_patterns(sketch, &mut aliases, tolerance_mm)?;
    heal_isolated_endpoints(sketch, &members, &mut aliases, tolerance_mm)?;
    Ok(aliases)
}

/// Active native material member connectivity, ordered by stable member identity.
pub fn analyze(sketch: &Sketch, tolerance_mm: f64) -> Result<Topology> {
    let point_aliases = point_aliases(sketch, tolerance_mm)?;
    let members = material_members(sketch)?;
    let mut at_point = BTreeMap::<Id, Vec<usize>>::new();
    let mut contours = Vec::new();
    let mut remaining = BTreeSet::new();
    for (index, member) in members.iter().enumerate() {
        if let Some(endpoints) = &member.endpoints {
            for point in endpoints {
                at_point
                    .entry(point_aliases[point].clone())
                    .or_default()
                    .push(index);
            }
            remaining.insert(index);
        } else {
            contours.push(Contour {
                members: vec![member.id.clone()],
                status: ContourStatus::Closed,
            });
        }
    }
    while let Some(first) = remaining.pop_first() {
        let mut queue = vec![first];
        let mut group = BTreeSet::from([first]);
        let mut vertices = BTreeSet::new();
        while let Some(index) = queue.pop() {
            if let Some(endpoints) = &members[index].endpoints {
                for point in endpoints {
                    let canonical = &point_aliases[point];
                    vertices.insert(canonical.clone());
                    for adjacent in &at_point[canonical] {
                        if remaining.remove(adjacent) {
                            queue.push(*adjacent);
                            group.insert(*adjacent);
                        }
                    }
                }
            }
        }
        let status = if vertices.iter().any(|point| at_point[point].len() > 2) {
            ContourStatus::Branched
        } else if vertices.iter().all(|point| at_point[point].len() == 2) {
            ContourStatus::Closed
        } else {
            ContourStatus::Open
        };
        contours.push(Contour {
            members: group
                .into_iter()
                .map(|index| members[index].id.clone())
                .collect(),
            status,
        });
    }
    contours.sort_by(|first, second| first.members.cmp(&second.members));
    Ok(Topology {
        point_aliases,
        contours,
    })
}
