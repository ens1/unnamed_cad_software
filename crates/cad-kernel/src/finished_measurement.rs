//! Native evidence for finished-size measurements. Boundary witnesses belong to
//! the trimmed solid. Conservative hulls can disqualify a size, never supply one.
use crate::{adapter::Shape, native_bounds};
use cad_core::protocol::{FaceRange, NativeCone, NativeCylinder, NativeFaceMeasurement};
use monstertruck::modeling::*;
use std::collections::HashMap;

type Point = [f64; 3];
type Bounds = [[f64; 3]; 2];
const AXES: [Vector3; 3] = [
    Vector3::new(1., 0., 0.),
    Vector3::new(0., 1., 0.),
    Vector3::new(0., 0., 1.),
];

fn bounds(enclosure: &native_bounds::Enclosure) -> Option<Bounds> {
    let mut result = [[0.; 3]; 2];
    for (index, axis) in AXES.iter().enumerate() {
        (result[0][index], result[1][index]) =
            enclosure.projection(Point3::origin(), *axis).ok()?;
    }
    Some(result)
}

fn ranges(points: &[Point]) -> Option<Bounds> {
    let (first, rest) = points.split_first()?;
    let mut result = [*first; 2];
    for p in rest {
        for (i, &v) in p.iter().enumerate() {
            result[0][i] = result[0][i].min(v);
            result[1][i] = result[1][i].max(v);
        }
    }
    Some(result)
}
struct Boundary {
    points: Vec<Point>,
    bounds: Option<Bounds>,
}

fn boundary(edge: &Edge) -> Boundary {
    let c = edge.curve();
    let mut points: Vec<Point> = vec![
        edge.absolute_front().point().into(),
        edge.absolute_back().point().into(),
    ];
    // Native trimming vertices and underlying curve endpoints may differ by
    // the modeling incidence residual. Include both in the boundary envelope.
    if matches!(
        &c,
        Curve::Line(_) | Curve::BsplineCurve(_) | Curve::NurbsCurve(_)
    ) {
        let (low, high) = c.range_tuple();
        if low.is_finite() && high.is_finite() {
            points.push(c.subs(low).into());
            points.push(c.subs(high).into());
        }
    }
    // Native lines and verified monotone arcs attain each coordinate extreme
    // at an endpoint or an angular station inside the actual trim.
    let simple = if matches!(&c, Curve::Line(_)) {
        true
    } else if matches!(&c, Curve::BsplineCurve(_) | Curve::NurbsCurve(_)) {
        if crate::bounded_carriers::line(&c).is_ok() {
            true
        } else if let Ok(((center, normal, radius), sweep)) = crate::bounded_carriers::circle(&c) {
            let u = (c.subs(c.range_tuple().0) - center).normalize();
            let v = normal.cross(u);
            for axis in AXES {
                let flat = axis - normal * axis.dot(normal);
                if flat.magnitude2() < 1e-24 {
                    continue;
                }
                for dir in [flat.normalize(), -flat.normalize()] {
                    let angle = (dir.dot(v).atan2(dir.dot(u)) * sweep.signum())
                        .rem_euclid(std::f64::consts::TAU);
                    if angle <= sweep.abs() {
                        points.push((center + dir * radius).into());
                    }
                }
            }
            true
        } else {
            false
        }
    } else {
        false
    };
    let bound = if simple {
        ranges(&points)
    } else {
        native_bounds::curve(&c)
            .ok()
            .and_then(|value| bounds(&value))
    };
    Boundary {
        points,
        bounds: bound,
    }
}

fn inward(face: &Face, axis: &cad_core::reference_geometry::Axis) -> Option<bool> {
    let s = face.surface();
    let (u, v) = match &s {
        Surface::NurbsSurface(p) => p.range_tuple(),
        Surface::BsplineSurface(p) => p.range_tuple(),
        Surface::RevolutionSurface(p) => {
            let line = p.entity().entity_curve().range_tuple();
            let angle = (0., std::f64::consts::TAU);
            if p.orientation() {
                (line, angle)
            } else {
                (angle, line)
            }
        }
        _ => return None,
    };
    // Inverted processor charts interchange their parameters. Either chart is
    // regular on the cylindrical carrier; sign is constant over each patch.
    let (u, v) = ((u.0 + u.1) * 0.5, (v.0 + v.1) * 0.5);
    let d = Vector3::from(axis.direction);
    let from_axis = s.subs(u, v) - Point3::from(axis.origin);
    let radial = from_axis - d * from_axis.dot(d);
    let sign = radial.dot(s.normal(u, v)) * if face.orientation() { 1. } else { -1. };
    (sign.is_finite() && sign.abs() > 1e-12).then_some(sign < 0.)
}

fn measured_ruling(face: &Face) -> Option<(Option<NativeCylinder>, Option<NativeCone>)> {
    let (mut axis, [ra, rb], height) = crate::axis_geometry::circular_ruling(face).ok()?;
    let internal = inward(face, &axis)?;
    if (ra - rb).abs() <= 128. * f64::EPSILON * ra.max(rb).max(1.) {
        return Some((
            Some(NativeCylinder {
                axis,
                radius_mm: (ra + rb) * 0.5,
                internal,
            }),
            None,
        ));
    }
    let slope = (rb - ra) / height;
    let dir = Vector3::from(axis.direction);
    let apex = Point3::from(axis.origin) - dir * (ra / slope);
    if !apex.to_vec().is_finite() || !slope.is_finite() {
        return None;
    }
    axis.origin = apex.into();
    axis.direction = (dir * slope.signum()).into();
    Some((
        None,
        Some(NativeCone {
            axis,
            slope: slope.abs(),
            internal,
        }),
    ))
}
fn face_measurement(
    face: &Face,
    planar: bool,
    cache: &mut HashMap<EdgeId, Boundary>,
) -> Option<NativeFaceMeasurement> {
    let mut witnesses = [[Point::default(); 3]; 2];
    let mut initialized = false;
    let mut boundary_box = Some([[f64::INFINITY; 3], [f64::NEG_INFINITY; 3]]);
    for edge in face.edge_iter() {
        let evidence = cache.entry(edge.id()).or_insert_with(|| boundary(&edge));
        boundary_box = boundary_box.zip(evidence.bounds).map(|(mut total, b)| {
            for i in 0..3 {
                total[0][i] = total[0][i].min(b[0][i]);
                total[1][i] = total[1][i].max(b[1][i]);
            }
            total
        });
        for p in &evidence.points {
            if !p.iter().all(|v| v.is_finite()) {
                return None;
            }
            for i in 0..3 {
                if !initialized || p[i] < witnesses[0][i][i] {
                    witnesses[0][i] = *p;
                }
                if !initialized || p[i] > witnesses[1][i][i] {
                    witnesses[1][i] = *p;
                }
            }
            initialized = true;
        }
    }
    if !initialized {
        return None;
    }
    let (cylinder, cone) = if planar {
        (None, None)
    } else {
        measured_ruling(face).unwrap_or_default()
    };
    // Every point on a bounded trimmed plane, cylinder or cone lies on a
    // straight ruling that terminates on its trim. A linear coordinate reaches
    // its extremum at one of those endpoints, even on an oblique or partial arc.
    let enclosure_mm = if planar || cylinder.is_some() || cone.is_some() {
        boundary_box
    } else {
        native_bounds::face(face)
            .ok()
            .and_then(|value| bounds(&value))
    };
    Some(NativeFaceMeasurement {
        enclosure_mm,
        witnesses,
        cylinder,
        cone,
    })
}

pub(crate) fn measure(shape: &Shape, output: &mut [FaceRange]) {
    let Shape::Bodies(solids) = shape else {
        return;
    };
    if output.len() > 65_536 || solids.iter().flat_map(|s| s.face_iter()).count() != output.len() {
        return;
    }
    let mut cache = HashMap::new();
    for (face, range) in solids.iter().flat_map(|s| s.face_iter()).zip(output) {
        if cache.len() > 65_536 {
            return;
        }
        range.measurement = face_measurement(face, range.plane.is_some(), &mut cache);
    }
}
