//! One hole-position sketch point can produce several cylindrical/conical
//! lands. Keep one center location and measure blind depth to the finished
//! bottom from the actual entry, including a counterbore or countersink.
use super::{
    Characteristics, Incidence, MAX_SOURCE_KEY, aligned, anchor, normal, serialize_identity, vec,
};
use crate::{
    Operation, Part, TopologyRef,
    model_dimensions::{Characteristic, Kind},
};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

struct Drill {
    source: Value,
    faces: BTreeSet<usize>,
    dimensions: Vec<usize>,
}

fn source(face: &TopologyRef) -> Option<(String, Value)> {
    let encoded = face.role.strip_prefix("hole-position:")?;
    if encoded.len() > MAX_SOURCE_KEY {
        return None;
    }
    let data: Value = serde_json::from_str(encoded).ok()?;
    // Ordinals identify individual tool faces and change with the head style.
    // Definition, point and target stock identify the actual drilled position.
    let value = serde_json::json!({"producer":face.producer,"topology":"drilled_position","role":{
        "point":data.get("point")?.as_str()?, "body_source":data.get("body_source")?
    }});
    Some((serde_json::to_string(&value).ok()?, value))
}

fn assign_id(out: &mut Characteristics<'_>, family: &str, source: &Value) -> Option<String> {
    let id = serialize_identity(family, vec![source.clone()]).ok()?;
    if id.len() > out.remaining_key_bytes || id.len() > MAX_SOURCE_KEY {
        out.coverage
            .insert("A drilled-hole dimension exceeds the source-reference budget.".into());
        return None;
    }
    out.remaining_key_bytes -= id.len();
    Some(id)
}

fn locations(out: &mut Characteristics<'_>, drill: &Drill, removed: &mut BTreeSet<usize>) {
    let mut by_axis = BTreeMap::<String, Vec<usize>>::new();
    for &i in &drill.dimensions {
        let Some((family, _)) = out.dimensions[i].id.split_once('|') else {
            continue;
        };
        if family.starts_with("location-") {
            by_axis.entry(family.to_owned()).or_default().push(i);
        }
    }
    for (family, items) in by_axis {
        let Some((&chosen, others)) = items.split_first() else {
            continue;
        };
        let value = out.dimensions[chosen].nominal;
        if others
            .iter()
            .any(|&i| (out.dimensions[i].nominal - value).abs() > out.tolerance)
        {
            continue;
        }
        let Some(id) = assign_id(out, &family, &drill.source) else {
            continue;
        };
        out.dimensions[chosen].id = id;
        // The existing measurements already paid for these support records.
        for &i in others {
            let supports = std::mem::take(&mut out.dimensions[i].supports);
            for face in supports {
                if !out.dimensions[chosen]
                    .supports
                    .iter()
                    .any(|s| s.producer == face.producer && s.role == face.role)
                {
                    out.dimensions[chosen].supports.push(face);
                }
            }
            removed.insert(i);
        }
    }
}

struct Station {
    distance: f64,
    source: usize,
}
fn bore(
    out: &mut Characteristics<'_>,
    graph: &Incidence,
    drill: &Drill,
    removed: &mut BTreeSet<usize>,
) {
    let axis = drill.faces.iter().find_map(|&i| {
        let m = out.faces[i].measurement.as_ref()?;
        m.cylinder
            .as_ref()
            .map(|c| &c.axis)
            .or_else(|| m.cone.as_ref().map(|c| &c.axis))
    });
    let Some(axis) = axis else {
        return;
    };
    let n = vec(axis.direction);
    let o = vec(axis.origin);
    let mut walls = BTreeSet::new();
    for &i in &drill.faces {
        let Some(m) = &out.faces[i].measurement else {
            continue;
        };
        let wall = m
            .cylinder
            .as_ref()
            .map(|c| &c.axis)
            .or_else(|| m.cone.as_ref().map(|c| &c.axis));
        if let Some(wall) = wall {
            // Multiple disconnected holes can share source roles after an
            // import or pattern. Never combine depths from distinct axes.
            if !aligned(vec(wall.direction), n)
                || (vec(wall.origin) - o).cross(&n).norm() > out.tolerance
            {
                return;
            }
            walls.insert(i);
        }
    }
    // A later slot or subtractive split can sever a drill into distinct
    // material segments. Only the original wall and its actual transitions
    // connect the entry to the bottom. A common axis alone cannot do so.
    let Some(&first) = walls.first() else {
        return;
    };
    let mut reached = BTreeSet::from([first]);
    let mut pending = vec![first];
    while let Some(face) = pending.pop() {
        for &next in &graph.adjacent[face] {
            if drill.faces.contains(&next) && reached.insert(next) {
                pending.push(next);
            }
        }
    }
    if !walls.is_subset(&reached) {
        return;
    }
    let mut stations = BTreeMap::new();
    for &wall in &walls {
        for &i in &graph.adjacent[wall] {
            if normal(out.faces[i]).is_some_and(|p| aligned(p, n))
                && let Some(point) = anchor(out.faces[i])
            {
                stations.entry(i).or_insert(Station {
                    distance: (point - o).dot(&n),
                    source: i,
                });
            }
        }
    }
    let mut stations = stations.into_values().collect::<Vec<_>>();
    stations.sort_by(|a, b| a.distance.total_cmp(&b.distance));
    if stations.len() < 2 {
        return;
    }
    let (low, high) = (&stations[0], stations.last().unwrap());
    // Require unique terminal faces and only one native stock entry. A through
    // bore or interrupted material has no simple blind bottom to inspect.
    if stations
        .iter()
        .skip(1)
        .any(|s| (s.distance - low.distance).abs() <= out.tolerance)
        || stations
            .iter()
            .take(stations.len() - 1)
            .any(|s| (s.distance - high.distance).abs() <= out.tolerance)
    {
        return;
    }
    if stations[1..stations.len() - 1]
        .iter()
        .any(|s| !drill.faces.contains(&s.source))
    {
        return;
    }
    let (entry, bottom) = match (
        drill.faces.contains(&low.source),
        drill.faces.contains(&high.source),
    ) {
        (false, true) => (low, high),
        (true, false) => (high, low),
        _ => return,
    };
    if normal(out.faces[entry.source])
        .zip(normal(out.faces[bottom.source]))
        .is_none_or(|(a, b)| a.dot(&b) <= 0.)
    {
        return;
    }
    let bore_depth = (bottom.distance - entry.distance).abs();
    if bore_depth <= out.tolerance {
        return;
    }
    let Some(id) = assign_id(out, "drilled-depth", &drill.source) else {
        return;
    };
    let matches_face = |i: usize, r: &TopologyRef| {
        let src = &out.faces[i].reference;
        src.producer == r.producer && src.role == r.role
    };
    for &i in &drill.dimensions {
        let dim = &mut out.dimensions[i];
        if !dim.id.starts_with("depth|") {
            continue;
        }
        if dim.supports.iter().any(|r| matches_face(bottom.source, r)) {
            // The lower land is already determined by the entrance-to-bottom
            // depth plus the head geometry. Leave a single governing depth.
            removed.insert(i);
        } else if stations.len() == 3 && dim.supports.iter().any(|r| matches_face(entry.source, r))
        {
            dim.label = "Counterbore depth".into();
        }
    }
    out.dimensions.push(Characteristic {
        id,
        label: "Bore depth".into(),
        kind: Kind::Linear,
        nominal: bore_depth,
        angle_vertex: None,
        witnesses: [
            (o + n * entry.distance).into(),
            (o + n * bottom.distance).into(),
        ],
        plane_normal: None,
        supports: out.refs([entry.source, bottom.source].into_iter().chain(walls)),
    });
}

fn typed_sizes(
    out: &mut Characteristics<'_>,
    drill: &Drill,
    definition: &crate::holes::Definition,
    doc: &crate::Document,
    owner: &str,
    removed: &mut BTreeSet<usize>,
) {
    // The native cutter numbers its faces in station order. Adding a head
    // reuses former pilot ordinals for the head. Establish role from measured
    // geometry before accepting a saved manufacturing override.
    let nominal = |expr: &crate::Expr| {
        crate::expression::value(doc, owner, expr, crate::Dimension::Length).ok()
    };
    let pilot = nominal(definition.bore_diameter());
    let head = match &definition.head {
        crate::holes::Head::Counterbore { diameter, .. } => nominal(diameter),
        _ => None,
    };
    let diameter_indices: Vec<_> = drill
        .dimensions
        .iter()
        .copied()
        .filter(|&i| out.dimensions[i].id.starts_with("diameter|"))
        .collect();
    let extremes = diameter_indices
        .iter()
        .map(|&i| out.dimensions[i].nominal)
        .fold([f64::INFINITY, f64::NEG_INFINITY], |[lo, hi], v| {
            [lo.min(v), hi.max(v)]
        });
    let counterbored = matches!(&definition.head, crate::holes::Head::Counterbore { .. });
    for &i in &drill.dimensions {
        if removed.contains(&i) {
            continue;
        }
        let dim = &out.dimensions[i];
        let family = dim.id.split_once('|').map(|v| v.0);
        let (family, label) = match family {
            Some("diameter") => {
                let value = dim.nominal;
                let matches =
                    |v: Option<f64>| v.is_some_and(|v| (value - v).abs() <= out.tolerance);
                let ordered_roles =
                    diameter_indices.len() == 2 && extremes[1] - extremes[0] > out.tolerance;
                if !counterbored
                    || matches(pilot) && !matches(head)
                    || ordered_roles && value == extremes[0]
                {
                    ("drill-pilot", "Bore diameter")
                } else if matches(head) && !matches(pilot) || ordered_roles && value == extremes[1]
                {
                    ("drill-counterbore", "Counterbore diameter")
                } else {
                    out.coverage.insert("A surviving cylindrical land could not be identified as the pilot or counterbore diameter.".into());
                    removed.insert(i);
                    continue;
                }
            }
            Some("cone-major")
                if matches!(&definition.head, crate::holes::Head::Countersink { .. }) =>
            {
                ("drill-countersink", "Countersink opening diameter")
            }
            Some("cone-included-angle")
                if matches!(&definition.head, crate::holes::Head::Countersink { .. }) =>
            {
                ("drill-countersink-angle", "Countersink included angle")
            }
            Some("depth")
                if counterbored
                    && (dim.label == "Counterbore depth"
                        || dim.supports.iter().any(|r| {
                            drill.faces.iter().any(|&j| {
                                let face = out.faces[j];
                                r.producer == face.reference.producer
                                    && r.role == face.reference.role
                                    && face
                                        .measurement
                                        .as_ref()
                                        .and_then(|m| m.cylinder.as_ref())
                                        .is_some_and(|c| {
                                            head.is_some_and(|h| {
                                                (2. * c.radius_mm - h).abs() <= out.tolerance
                                            })
                                        })
                            })
                        })) =>
            {
                ("drill-counterbore-depth", "Counterbore depth")
            }
            _ => continue,
        };
        if let Some(id) = assign_id(out, family, &drill.source) {
            out.dimensions[i].id = id;
            out.dimensions[i].label = label.into();
        }
    }
}

pub(super) fn consolidate(
    out: &mut Characteristics<'_>,
    part: &Part,
    doc: &crate::Document,
    graph: &Incidence,
) {
    if !part
        .features
        .iter()
        .any(|f| matches!(&f.operation, Operation::DrilledHoles { .. }))
    {
        return;
    }
    let mut jobs = BTreeMap::<String, usize>::new();
    let mut drills = vec![];
    let mut face_jobs = BTreeMap::new();
    for (i, face) in out.faces.iter().enumerate() {
        let Some((key, source)) = source(&face.reference) else {
            continue;
        };
        let index = *jobs.entry(key).or_insert_with(|| {
            let i = drills.len();
            drills.push(Drill {
                source,
                faces: BTreeSet::new(),
                dimensions: vec![],
            });
            i
        });
        drills[index].faces.insert(i);
        face_jobs.insert(
            (&face.reference.producer, face.reference.role.as_str()),
            index,
        );
    }
    for (i, d) in out.dimensions.iter().enumerate() {
        if d.id.starts_with("overall/") {
            continue;
        }
        let jobs = d
            .supports
            .iter()
            .filter_map(|f| face_jobs.get(&(&f.producer, f.role.as_str())))
            .collect::<BTreeSet<_>>();
        if jobs.len() == 1 {
            drills[**jobs.first().unwrap()].dimensions.push(i);
        }
    }
    let mut removed = BTreeSet::new();
    for drill in &drills {
        locations(out, drill, &mut removed);
        bore(out, graph, drill, &mut removed);
        if let Some(face) = drill.faces.first()
            && let Some(Operation::DrilledHoles { definition }) = part
                .features
                .iter()
                .find(|f| f.id == out.faces[*face].reference.producer.node)
                .map(|f| &f.operation)
        {
            typed_sizes(out, drill, definition, doc, &part.id, &mut removed);
        }
    }
    let mut index = 0;
    out.dimensions.retain(|_| {
        let keep = !removed.contains(&index);
        index += 1;
        keep
    });
}
