//! Measure the surviving circular rails of a true conical face. A countersink
//! is defined by its opening diameter and included angle; the adjoining bore
//! already supplies its own diameter.
use super::{
    Characteristics, FaceIndex, Incidence, Vector, aligned, anchor, neighboring_faces, normal, vec,
};
use crate::{
    Operation, Part,
    model_dimensions::{Characteristic, Kind},
    protocol::{EdgeCenter, FaceRange, MeshData, NativeCone},
};
use std::collections::BTreeSet;

fn cone(face: &FaceRange) -> Option<&NativeCone> {
    face.measurement.as_ref()?.cone.as_ref()
}

fn related(a: &NativeCone, b: &NativeCone, tolerance: f64) -> bool {
    aligned(vec(a.axis.direction), vec(b.axis.direction))
        && vec(a.axis.direction).dot(&vec(b.axis.direction)) > 0.
        && (vec(a.axis.origin) - vec(b.axis.origin)).norm() <= tolerance
        && (a.slope - b.slope).abs() <= 1e-9 * a.slope.max(b.slope)
        && a.internal == b.internal
}

#[derive(Clone)]
struct Rim {
    station: f64,
    on_cylinder: bool,
}

fn transverse(
    edge: &crate::protocol::EdgeData,
    other: &FaceRange,
    c: &NativeCone,
    tolerance: f64,
) -> Option<Rim> {
    let n = vec(c.axis.direction);
    let apex = vec(c.axis.origin);
    let circular_wall = other.measurement.as_ref()?.cylinder.as_ref();
    let cylindrical = circular_wall.filter(|wall| {
        aligned(vec(wall.axis.direction), n)
            && (vec(wall.axis.origin) - apex).cross(&n).norm() <= tolerance
    });
    // An exact circular edge or the shared boundary of two proven elementary
    // carriers identifies a station. A sloping trim cannot become a circle.
    let station = if let Some(EdgeCenter::Circle {
        center,
        normal,
        radius_mm,
        ..
    }) = &edge.center
    {
        let delta = vec(*center) - apex;
        let t = delta.dot(&n);
        if !aligned(vec(*normal), n)
            || delta.cross(&n).norm() > tolerance
            || (t * c.slope - *radius_mm).abs() > tolerance
        {
            return None;
        }
        t
    } else if normal(other).is_some_and(|v| aligned(v, n)) {
        (anchor(other)? - apex).dot(&n)
    } else {
        cylindrical?.radius_mm / c.slope
    };
    if !station.is_finite() || station <= 0. {
        return None;
    }
    if !edge.native.as_ref()?.endpoints.iter().all(|&p| {
        let d = vec(p) - apex;
        (d.dot(&n) - station).abs() <= tolerance
            && (d.cross(&n).norm() - station * c.slope).abs() <= tolerance
    }) {
        return None;
    }
    Some(Rim {
        station,
        on_cylinder: cylindrical.is_some(),
    })
}

fn merge(rims: &mut Vec<Rim>, incoming: Rim, tolerance: f64) {
    if let Some(old) = rims
        .iter_mut()
        .find(|r| (r.station - incoming.station).abs() <= tolerance)
    {
        old.on_cylinder |= incoming.on_cylinder;
    } else if rims.len() < 3 {
        // Once three distinct transverse stations are present the two end
        // stations are ambiguous. No unbounded allocation for a perforated wall.
        rims.push(incoming);
    }
}

pub(super) fn add(
    out: &mut Characteristics<'_>,
    part: &Part,
    mesh: &MeshData,
    graph: &Incidence,
    index: &FaceIndex<'_>,
) {
    if !out.faces.iter().any(|f| cone(f).is_some()) {
        return;
    }

    let mut rails: Vec<Vec<Rim>> = vec![vec![]; out.faces.len()];
    for edge in &mesh.edges {
        let Some(native) = &edge.native else {
            continue;
        };
        let Some([(a, _), (b, _)]) = neighboring_faces(index, edge) else {
            continue;
        };
        for (wall, other) in [(a, b), (b, a)] {
            let Some(c) = cone(out.faces[wall]) else {
                continue;
            };
            if native
                .endpoints
                .iter()
                .any(|&p| (vec(p) - vec(c.axis.origin)).norm() <= out.tolerance)
            {
                merge(
                    &mut rails[wall],
                    Rim {
                        station: 0.,
                        on_cylinder: false,
                    },
                    out.tolerance,
                );
            }
            if let Some(rim) = transverse(edge, out.faces[other], c, out.tolerance) {
                merge(&mut rails[wall], rim, out.tolerance);
            }
        }
    }
    let mut visited = BTreeSet::new();
    for root in 0..out.faces.len() {
        let Some(c) = cone(out.faces[root]) else {
            continue;
        };
        if !visited.insert(root) {
            continue;
        }
        let mut group = vec![root];
        let mut rims = vec![];
        let mut k = 0;
        while k < group.len() {
            let i = group[k];
            for station in &rails[i] {
                merge(&mut rims, station.clone(), out.tolerance);
            }
            for &j in &graph.adjacent[i] {
                if !visited.contains(&j)
                    && cone(out.faces[j]).is_some_and(|other| related(c, other, out.tolerance))
                {
                    visited.insert(j);
                    group.push(j);
                }
            }
            k += 1;
        }
        rims.sort_by(|a, b| a.station.total_cmp(&b.station));
        let [small, large] = rims.as_slice() else {
            out.coverage
                .insert("A conical face has no unique pair of circular end stations.".into());
            continue;
        };
        let n = vec(c.axis.direction);
        let apex = vec(c.axis.origin);
        let basis = if n.x.abs() < 0.8 {
            Vector::x()
        } else {
            Vector::y()
        };
        let radial = (basis - n * basis.dot(&n)).normalize();
        let cn = |rim: &Rim| apex + n * rim.station;
        let radius = |rim: &Rim| rim.station * c.slope;
        let small_center = cn(small);
        let large_center = cn(large);
        if !(small_center
            .iter()
            .chain(large_center.iter())
            .all(|v| v.is_finite())
            && radius(large).is_finite())
        {
            continue;
        }
        let owner = &out.faces[root].reference.producer.node;
        let feature = part
            .features
            .iter()
            .find(|f| &f.id == owner)
            .map(|f| &f.operation);
        if matches!(feature, Some(Operation::Chamfer { .. })) {
            let a = small_center + radial * radius(small);
            let b = large_center + radial * radius(large);
            let corner = large_center + radial * radius(small);
            for (label, family, witness, nominal) in [
                (
                    "Chamfer axial setback",
                    "cone-axial-setback",
                    a,
                    large.station - small.station,
                ),
                (
                    "Chamfer radial setback",
                    "cone-radial-setback",
                    b,
                    radius(large) - radius(small),
                ),
            ] {
                if let Some(id) = out.id(family, &group) {
                    out.dimensions.push(Characteristic {
                        drivers: vec![],
                        id,
                        label: label.into(),
                        kind: Kind::Linear,
                        nominal,
                        angle_vertex: None,
                        witnesses: [corner.into(), witness.into()],
                        plane_normal: Some(n.cross(&radial).into()),
                        supports: out.refs(group.iter().copied()),
                    });
                }
            }
            continue;
        }
        let countersink = c.internal
            && matches!(feature,Some(Operation::DrilledHoles{definition}) if matches!(definition.head,crate::holes::Head::Countersink{..}));
        for (rim, label, family) in [
            (
                large,
                if countersink {
                    "Countersink opening diameter"
                } else {
                    "Cone major diameter"
                },
                "cone-major",
            ),
            (small, "Cone minor diameter", "cone-minor"),
        ] {
            if rim.on_cylinder || radius(rim) <= out.tolerance {
                continue;
            }
            if let Some(id) = out.id(family, &group) {
                let center = cn(rim);
                let d = radial * radius(rim);
                out.dimensions.push(Characteristic {
                    drivers: vec![],
                    id,
                    label: label.into(),
                    kind: Kind::Diameter,
                    nominal: 2. * radius(rim),
                    angle_vertex: None,
                    witnesses: [(center - d).into(), (center + d).into()],
                    plane_normal: Some(n.into()),
                    supports: out.refs(group.iter().copied()),
                });
            }
        }
        if let Some(id) = out.id("cone-included-angle", &group) {
            let d = radial * radius(large);
            out.dimensions.push(Characteristic {
                drivers: vec![],
                id,
                label: if countersink {
                    "Countersink included angle"
                } else {
                    "Cone included angle"
                }
                .into(),
                kind: Kind::Angle,
                nominal: 2. * c.slope.atan(),
                angle_vertex: Some(apex.into()),
                witnesses: [(large_center - d).into(), (large_center + d).into()],
                plane_normal: Some(n.cross(&radial).into()),
                supports: out.refs(group.iter().copied()),
            });
        }
    }
}
