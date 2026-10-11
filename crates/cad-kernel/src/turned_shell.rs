//! Exact offsets of coaxial turned line/arc profiles, including toroidal blends.
//! The radius/height plane contains the authoritative analytic join equations.
//! Native circular rails and meridians are rebuilt with shared topology.
use crate::{
    analytic_offset::{self, Carrier, EPS},
    shell_history::ShellHistory,
};
use cad_core::{CadError, Result};
use monstertruck::modeling::*;
use std::collections::{HashMap, HashSet};
const REL: f64 = 1e-10;
const MAX_FACES: usize = 4096;
fn invalid(s: &str) -> CadError {
    CadError::Invalid(format!("Turned shell: {s}"))
}
fn unsupported(s: &str) -> CadError {
    CadError::Unsupported(format!("Turned shell: {s}"))
}
#[derive(Clone, Copy, Debug)]
enum Equation {
    Line { n: Vector2, b: f64 },
    Circle { center: Point2, radius: f64 },
}
impl Equation {
    fn same(self, other: Self) -> bool {
        match (self, other) {
            (Self::Line { n: a, b: x }, Self::Line { n: b, b: y }) => {
                a.dot(b) > 1. - REL && (x - y).abs() <= EPS
            }
            (
                Self::Circle {
                    center: a,
                    radius: r,
                },
                Self::Circle {
                    center: b,
                    radius: s,
                },
            ) => a.distance(b) <= EPS && (r - s).abs() <= EPS,
            _ => false,
        }
    }
    fn residual(self, p: Point2) -> f64 {
        match self {
            Self::Line { n, b } => p.to_vec().dot(n) - b,
            Self::Circle { center, radius } => p.distance(center) - radius,
        }
    }
    fn normal(self, p: Point2) -> Vector2 {
        match self {
            Self::Line { n, .. } => n,
            Self::Circle { center, .. } => (p - center).normalize(),
        }
    }
    fn offset(self, d: f64) -> Result<Self> {
        match self {
            Self::Line { n, b } => Ok(Self::Line { n, b: b + d }),
            Self::Circle { center, radius } => {
                let radius = radius + d;
                if !radius.is_finite() || radius <= EPS {
                    return Err(invalid(
                        "Thickness collapses a circular blend at its focal center",
                    ));
                }
                if center.x > EPS && radius >= center.x - EPS {
                    return Err(invalid(
                        "Thickness closes a toroidal opening or makes its carrier singular",
                    ));
                }
                Ok(Self::Circle { center, radius })
            }
        }
    }
    fn project_offset(self, p: Point2, other: Self) -> Point2 {
        match (self, other) {
            (Self::Line { n, b }, Self::Line { b: new, .. }) => p + n * (new - b),
            (Self::Circle { center, radius }, Self::Circle { radius: new, .. }) => {
                center + (p - center) * (new / radius)
            }
            _ => p,
        }
    }
}
struct Frame {
    origin: Point3,
    axis: Vector3,
}
impl Frame {
    fn rz(&self, p: Point3) -> Point2 {
        let q = p - self.origin;
        let z = q.dot(self.axis);
        Point2::new((q - self.axis * z).magnitude(), z)
    }
    fn radial(&self, p: Point3) -> Option<Vector3> {
        let q = p - self.origin;
        let q = q - self.axis * q.dot(self.axis);
        let r = q.magnitude();
        (r > EPS && r.is_finite()).then(|| q / r)
    }
    fn place(&self, p: Point2, radial: Vector3) -> Point3 {
        self.origin + self.axis * p.y + radial * p.x
    }
    fn equation(&self, c: Carrier) -> Option<Equation> {
        match c {
            Carrier::Plane { origin, normal } => (normal.dot(self.axis).abs() > 1. - REL)
                .then_some(Equation::Line {
                    n: Vector2::unit_y(),
                    b: (origin - self.origin).dot(self.axis),
                }),
            Carrier::Cone {
                origin,
                axis,
                radius,
                slope,
            } => {
                if axis.dot(self.axis).abs() < 1. - REL
                    || (origin - self.origin).cross(self.axis).magnitude() > EPS
                {
                    return None;
                }
                let k = slope * axis.dot(self.axis).signum();
                let r = radius + slope * (self.origin - origin).dot(axis);
                let length = (1. + k * k).sqrt();
                Some(Equation::Line {
                    n: Vector2::new(1., -k) / length,
                    b: r / length,
                })
            }
            Carrier::Sphere { center, radius } => {
                ((center - self.origin).cross(self.axis).magnitude() <= EPS).then_some(
                    Equation::Circle {
                        center: Point2::new(0., (center - self.origin).dot(self.axis)),
                        radius,
                    },
                )
            }
            Carrier::Torus {
                origin,
                axis,
                major,
                minor,
            } => (axis.dot(self.axis).abs() > 1. - REL
                && (origin - self.origin).cross(self.axis).magnitude() <= EPS)
                .then_some(Equation::Circle {
                    center: Point2::new(major, (origin - self.origin).dot(self.axis)),
                    radius: minor,
                }),
        }
    }
}
#[derive(Clone, Copy)]
struct Wall {
    before: Equation,
    after: Equation,
    side: f64,
    plane: bool,
}
fn sign(face: &Face, frame: &Frame, equation: Equation) -> Result<f64> {
    let surface = face.oriented_surface();
    let (u, v) = surface.try_range_tuple();
    let (u, v) = (
        u.map_or(0., |(a, b)| (a + b) * 0.5),
        v.map_or(0., |(a, b)| (a + b) * 0.5),
    );
    let p = surface.subs(u, v);
    let rz = frame.rz(p);
    let normal = equation.normal(rz);
    let radial = frame.radial(p).unwrap_or_else(Vector3::zero);
    let expected = radial * normal.x + frame.axis * normal.y;
    let dot = surface.normal(u, v).normalize().dot(expected);
    if !dot.is_finite() || dot.abs() < 1. - 1e-8 {
        return Err(unsupported(
            "Could not establish the oriented normal of a turned wall",
        ));
    }
    Ok(dot.signum())
}
/// None denotes coincident analytic carriers; otherwise the finite complete
/// set of intersections is returned, without sampled or fitted geometry.
fn intersections(a: Equation, b: Equation) -> Result<Option<Vec<Point2>>> {
    if a.same(b) {
        return Ok(None);
    }
    let result = match (a, b) {
        (Equation::Line { n: a, b: x }, Equation::Line { n: b, b: y }) => {
            let determinant = a.x * b.y - a.y * b.x;
            if determinant.abs() < REL {
                return Ok(Some(Vec::new()));
            }
            vec![Point2::new(
                (x * b.y - a.y * y) / determinant,
                (a.x * y - x * b.x) / determinant,
            )]
        }
        (Equation::Line { n, b }, Equation::Circle { center, radius })
        | (Equation::Circle { center, radius }, Equation::Line { n, b }) => {
            let distance = center.to_vec().dot(n) - b;
            if distance.abs() > radius + EPS {
                return Ok(Some(Vec::new()));
            }
            let foot = center - n * distance;
            let square = (radius - distance.abs()) * (radius + distance.abs());
            if square < 0. && (radius - distance.abs()).abs() > EPS {
                return Ok(Some(Vec::new()));
            }
            let half = square.max(0.).sqrt();
            let tangent = Vector2::new(-n.y, n.x);
            if half <= EPS {
                vec![foot]
            } else {
                vec![foot + tangent * half, foot - tangent * half]
            }
        }
        (
            Equation::Circle {
                center: a,
                radius: r,
            },
            Equation::Circle {
                center: b,
                radius: s,
            },
        ) => {
            let delta = b - a;
            let d = delta.magnitude();
            if d <= EPS || d > r + s + EPS || d < (r - s).abs() - EPS {
                return Ok(Some(Vec::new()));
            }
            let length = (d * d + (r - s) * (r + s)) / (2. * d);
            let h2 = (r - length.abs()) * (r + length.abs());
            if h2 < 0. && (r - length.abs()).abs() > EPS {
                return Ok(Some(Vec::new()));
            }
            let foot = a + delta * (length / d);
            let tangent = Vector2::new(-delta.y, delta.x) / d;
            let h = h2.max(0.).sqrt();
            if h <= EPS {
                vec![foot]
            } else {
                vec![foot + tangent * h, foot - tangent * h]
            }
        }
    };
    if result.iter().any(|p| !p.x.is_finite() || !p.y.is_finite()) {
        return Err(invalid("Offset intersection exceeds finite coordinates"));
    }
    Ok(Some(result))
}
fn vertex(p: Point2, walls: &[Wall]) -> Result<Point2> {
    let mut unique = Vec::<Wall>::new();
    for wall in walls {
        if let Some(existing) = unique.iter().find(|other| other.before.same(wall.before)) {
            if !existing.after.same(wall.after) {
                return Err(unsupported(
                    "Angular patches of one continuous turned wall need the same thickness and opening selection",
                ));
            }
        } else {
            unique.push(*wall);
        }
    }
    let Some(first) = unique.first() else {
        return Err(invalid("Offset vertex has no incident walls"));
    };
    if unique.len() == 1 {
        return Ok(first.before.project_offset(p, first.after));
    }
    let target = unique
        .iter()
        .map(|wall| wall.before.project_offset(p, wall.after).to_vec())
        .fold(Vector2::zero(), |a, b| a + b)
        / unique.len() as f64;
    let mut candidates = Vec::new();
    for i in 0..unique.len() {
        for j in i + 1..unique.len() {
            if let Some(points) = intersections(unique[i].after, unique[j].after)? {
                candidates.extend(points);
            }
        }
    }
    candidates.retain(|p| {
        p.x >= -EPS
            && unique
                .iter()
                .all(|wall| wall.after.residual(*p).abs() <= EPS)
    });
    candidates.sort_by(|a, b| {
        a.to_vec()
            .distance2(target)
            .total_cmp(&b.to_vec().distance2(target))
    });
    candidates.dedup_by(|a, b| a.distance(*b) <= EPS);
    let Some(result) = candidates.first().copied() else {
        return Err(invalid(
            "Thickness removes the intersection between adjacent offset walls",
        ));
    };
    if candidates.get(1).is_some_and(|other| {
        (other.to_vec().distance(target) - result.to_vec().distance(target)).abs() <= EPS
    }) {
        return Err(unsupported(
            "Offset join has two equally near branches; a unique material-side intersection cannot be established",
        ));
    }
    Ok(Point2::new(
        if result.x.abs() <= EPS { 0. } else { result.x },
        result.y,
    ))
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum EdgeKind {
    Angular,
    Meridian,
}
fn kind(edge: &Edge, frame: &Frame) -> Result<Option<EdgeKind>> {
    let curve = edge.curve();
    if let Ok(((center, normal, _), _)) = crate::bounded_carriers::circle(&curve) {
        if (center - frame.origin).cross(frame.axis).magnitude() <= EPS
            && normal.dot(frame.axis).abs() > 1. - REL
        {
            return Ok(Some(EdgeKind::Angular));
        }
        if normal.dot(frame.axis).abs() <= REL && (center - frame.origin).dot(normal).abs() <= EPS {
            return Ok(Some(EdgeKind::Meridian));
        }
        return Ok(None);
    }
    if crate::bounded_carriers::line(&curve).is_err() {
        return Ok(None);
    }
    let a = frame.radial(edge.absolute_front().point());
    let b = frame.radial(edge.absolute_back().point());
    Ok(
        (a.is_none() || b.is_none() || a.zip(b).is_some_and(|(a, b)| a.dot(b) > 1. - REL))
            .then_some(EdgeKind::Meridian),
    )
}
fn orient(edge: &Edge, source: &Edge) -> Edge {
    if source.orientation() {
        edge.clone()
    } else {
        edge.inverse()
    }
}
fn edge_curve(
    source: &Edge,
    kind: EdgeKind,
    frame: &Frame,
    wall: Wall,
    front: &Vertex,
    back: &Vertex,
) -> Result<Curve> {
    let curve = source.curve();
    if kind == EdgeKind::Angular {
        let (center, _, radius) = crate::axis_geometry::circle_geometry(&curve)?;
        let a = frame.rz(front.point());
        let b = frame.rz(back.point());
        if a.distance(b) > EPS || a.x <= EPS {
            return Err(invalid(
                "Circular offset rail collapses or has unequal end stations",
            ));
        }
        let next = frame.origin + frame.axis * a.y;
        return Ok(curve.transformed(
            Matrix4::from_translation(next.to_vec())
                * Matrix4::from_scale(a.x / radius)
                * Matrix4::from_translation(-center.to_vec()),
        ));
    }
    if crate::axis_geometry::line_axis(&curve).is_ok() {
        return Ok(Curve::Line(Line(front.point(), back.point())));
    }
    let ((_, normal, _), old_sweep) = crate::bounded_carriers::circle(&curve)?;
    let Equation::Circle { center, radius } = wall.after else {
        return Err(unsupported(
            "Curved meridian is not supported by a circular offset carrier",
        ));
    };
    let radial = frame
        .radial(source.absolute_front().point())
        .or_else(|| frame.radial(source.absolute_back().point()))
        .ok_or_else(|| invalid("Circular meridian has no radial direction"))?;
    let center = frame.place(center, radial);
    let a = (front.point() - center) / radius;
    let b = (back.point() - center) / radius;
    if (a.magnitude() - 1.).abs() * radius > EPS || (b.magnitude() - 1.).abs() * radius > EPS {
        return Err(invalid(
            "Offset meridian endpoints are off its exact circle",
        ));
    }
    let mut sweep = a.cross(b).dot(normal).atan2(a.dot(b));
    if sweep * old_sweep < 0. {
        sweep += std::f64::consts::TAU * old_sweep.signum();
    }
    if sweep.abs() <= REL
        || sweep.abs() >= std::f64::consts::TAU - REL
        || (sweep - old_sweep).abs() > std::f64::consts::PI
    {
        return Err(invalid(
            "Thickness collapses a blend or changes its meridian branch",
        ));
    }
    // The radius coordinate is signed in the generating meridian plane. A
    // spherical arc must not pass through an interior pole and then be folded
    // back by the magnitude used for cylindrical coordinates.
    let radial_origin = (center - frame.origin).dot(radial);
    let cosine = a.dot(radial) * radius;
    let sine = normal.cross(a).dot(radial) * radius;
    let critical = sine.atan2(cosine);
    for angle in [0., sweep]
        .into_iter()
        .chain((-2..=2).map(|i| critical + i as f64 * std::f64::consts::PI))
    {
        if angle < sweep.min(0.) - REL || angle > sweep.max(0.) + REL {
            continue;
        }
        let r = radial_origin + cosine * angle.cos() + sine * angle.sin();
        if r < -EPS || (r.abs() <= EPS && angle.abs() > REL && (angle - sweep).abs() > REL) {
            return Err(invalid(
                "Offset circular meridian reaches an interior rotational pole",
            ));
        }
    }
    let midpoint = center + (Matrix3::from_axis_angle(normal, Rad(sweep * 0.5)) * a) * radius;
    Ok(builder::try_circle_arc::<Curve>(front, back, midpoint)
        .map_err(|e| invalid(&e.to_string()))?
        .curve())
}
#[derive(Clone)]
struct Segment {
    equation: Equation,
    start: Point2,
    end: Point2,
    mid: Point2,
    opening: bool,
    source_start: Point2,
    source_end: Point2,
    stations: [Option<VertexId>; 2],
}
impl Segment {
    fn endpoint(&self, p: Point2) -> bool {
        p.distance(self.start) <= EPS || p.distance(self.end) <= EPS
    }
    fn contains(&self, p: Point2) -> bool {
        if self.equation.residual(p).abs() > EPS {
            return false;
        }
        match self.equation {
            Equation::Line { .. } => {
                let v = self.end - self.start;
                let t = (p - self.start).dot(v) / v.magnitude2();
                (-REL..=1. + REL).contains(&t)
            }
            Equation::Circle { center, .. } => {
                let angle = |p: Point2| (p.y - center.y).atan2(p.x - center.x);
                let start = angle(self.start);
                let ccw = (angle(self.end) - start).rem_euclid(std::f64::consts::TAU);
                let mid = (angle(self.mid) - start).rem_euclid(std::f64::consts::TAU);
                let candidate = (angle(p) - start).rem_euclid(std::f64::consts::TAU);
                self.endpoint(p)
                    || if mid <= ccw + REL {
                        candidate <= ccw + REL
                    } else {
                        candidate >= ccw - REL
                    }
            }
        }
    }
    fn duplicate(&self, other: &Self) -> bool {
        self.equation.same(other.equation)
            && ((self.start.distance(other.start) <= EPS && self.end.distance(other.end) <= EPS)
                || (self.start.distance(other.end) <= EPS && self.end.distance(other.start) <= EPS))
            && self.contains(other.mid)
            && other.contains(self.mid)
    }
}
fn related_endpoint(a: &Segment, b: &Segment, p: Point2) -> bool {
    [
        (a.start, a.source_start, a.stations[0]),
        (a.end, a.source_end, a.stations[1]),
    ]
    .iter()
    .any(|(end, source, station)| {
        p.distance(*end) <= EPS
            && [
                (b.start, b.source_start, b.stations[0]),
                (b.end, b.source_end, b.stations[1]),
            ]
            .iter()
            .any(|(other, origin, other_station)| {
                p.distance(*other) <= EPS
                    && match (station, other_station) {
                        (Some(a), Some(b)) => a == b,
                        (None, None) => {
                            source.x.abs() <= EPS
                                && origin.x.abs() <= EPS
                                && source.distance(*origin) <= EPS
                        }
                        _ => false,
                    }
            })
    })
}
fn crossing(a: &Segment, b: &Segment, allow_shared_ends: bool) -> Result<bool> {
    if let Some(points) = intersections(a.equation, b.equation)? {
        return Ok(points.into_iter().any(|p| {
            a.contains(p) && b.contains(p) && !(allow_shared_ends && related_endpoint(a, b, p))
        }));
    }
    if [a.start, a.end]
        .iter()
        .any(|p| b.endpoint(*p) && !(allow_shared_ends && related_endpoint(a, b, *p)))
    {
        return Ok(true);
    }
    Ok([a.start, a.mid, a.end]
        .iter()
        .any(|p| b.contains(*p) && !b.endpoint(*p))
        || [b.start, b.mid, b.end]
            .iter()
            .any(|p| a.contains(*p) && !a.endpoint(*p)))
}
/// Analytic parity in the radial section. Ray directions which hit an endpoint
/// or a circular tangency are discarded; the accepted ray has only transverse
/// intersections, so no tessellation, finite sampling or ambiguous vertex vote
/// participates in material classification.
fn contains_material(profile: &[Segment], query: Point2) -> Result<bool> {
    let mut boundaries = profile.to_vec();
    let mut poles = Vec::<(Point2, usize)>::new();
    for segment in profile {
        for p in [segment.start, segment.end] {
            if p.x.abs() > EPS {
                continue;
            }
            if let Some((_, degree)) = poles
                .iter_mut()
                .find(|(other, _)| p.distance(*other) <= EPS)
            {
                *degree += 1;
            } else {
                poles.push((p, 1));
            }
        }
    }
    let mut poles = poles
        .into_iter()
        .filter_map(|(p, degree)| (degree % 2 == 1).then_some(p))
        .collect::<Vec<_>>();
    poles.sort_by(|a, b| a.y.total_cmp(&b.y));
    if poles.len() % 2 != 0 {
        return Err(invalid(
            "Turned material profile has an unpaired axis endpoint",
        ));
    }
    for pair in poles.as_chunks::<2>().0 {
        boundaries.push(Segment {
            equation: Equation::Line {
                n: Vector2::unit_x(),
                b: 0.,
            },
            start: pair[0],
            end: pair[1],
            mid: pair[0] + (pair[1] - pair[0]) * 0.5,
            source_start: pair[0],
            source_end: pair[1],
            stations: [None, None],
            opening: false,
        });
    }
    if boundaries.iter().any(|segment| segment.contains(query)) {
        return Ok(true);
    }
    for attempt in 0..32 {
        let angle = (attempt as f64 + 0.3819660112501051) * std::f64::consts::PI / 32.;
        let ray = Vector2::new(angle.cos(), angle.sin());
        let normal = Vector2::new(-ray.y, ray.x);
        if boundaries.iter().any(|segment| {
            [segment.start, segment.end]
                .iter()
                .any(|p| (*p - query).dot(normal).abs() <= EPS)
        }) {
            continue;
        }
        let mut count = 0;
        let mut ambiguous = false;
        for segment in &boundaries {
            let candidates = match segment.equation {
                Equation::Line { n, b } => {
                    let denominator = n.dot(ray);
                    if denominator.abs() <= REL {
                        ambiguous = true;
                        break;
                    }
                    vec![(b - query.to_vec().dot(n)) / denominator]
                }
                Equation::Circle { center, radius } => {
                    let delta = center - query;
                    let h = delta.dot(normal);
                    if (h.abs() - radius).abs() <= EPS {
                        ambiguous = true;
                        break;
                    }
                    if h.abs() > radius {
                        continue;
                    }
                    let distance = ((radius - h.abs()) * (radius + h.abs())).sqrt();
                    let middle = delta.dot(ray);
                    vec![middle - distance, middle + distance]
                }
            };
            for distance in candidates {
                let hit = query + ray * distance;
                if !distance.is_finite() || !hit.x.is_finite() || !hit.y.is_finite() {
                    return Err(unsupported(
                        "Material classification exceeds finite analytic intersection bounds",
                    ));
                }
                if distance > EPS && segment.contains(hit) {
                    count += 1;
                }
            }
        }
        if !ambiguous {
            return Ok(count % 2 == 1);
        }
    }
    Err(unsupported(
        "The material-side proof could not find a transverse analytic ray",
    ))
}

fn profiles(before: &[Segment], after: &[Segment], outward: bool) -> Result<()> {
    if before.len() > MAX_FACES || after.len() > MAX_FACES {
        return Err(unsupported(
            "Turned profile exceeds the exact collision budget",
        ));
    }
    for i in 0..after.len() {
        if after[i].start.distance(after[i].end) <= EPS {
            return Err(invalid("Thickness collapses a meridian span"));
        }
        if matches!(after[i].equation, Equation::Line { .. })
            && (after[i].end - after[i].start).dot(after[i].source_end - after[i].source_start)
                <= EPS * EPS
        {
            return Err(invalid(
                "Thickness crosses a meridian collapse and reverses its material boundary",
            ));
        }
        if after[i].start.x < -EPS || after[i].end.x < -EPS {
            return Err(invalid("Offset passes through the rotational axis"));
        }
        for j in i + 1..after.len() {
            if crossing(&after[i], &after[j], true)? {
                return Err(invalid(
                    "Thickness makes two offset meridians cross or overlap",
                ));
            }
        }
        for source in before {
            if source.opening {
                continue;
            }
            if crossing(&after[i], source, false)? {
                return Err(invalid(
                    "Offset shell meets or crosses the original material boundary",
                ));
            }
        }
    }
    let (boundary, region) = if outward {
        (before, after)
    } else {
        (after, before)
    };
    for segment in boundary.iter().filter(|segment| !segment.opening) {
        if !contains_material(region, segment.mid)? {
            return Err(invalid(
                "Offset component leaves the intended material domain",
            ));
        }
    }
    Ok(())
}
fn make_segment(edge: &Edge, frame: &Frame, equation: Equation, opening: bool) -> Segment {
    let c = edge.curve();
    let (a, b) = c.range_tuple();
    Segment {
        equation,
        start: frame.rz(edge.absolute_front().point()),
        end: frame.rz(edge.absolute_back().point()),
        mid: frame.rz(c.subs((a + b) * 0.5)),
        opening,
        source_start: frame.rz(edge.absolute_front().point()),
        source_end: frame.rz(edge.absolute_back().point()),
        stations: [
            Some(edge.absolute_front().id()),
            Some(edge.absolute_back().id()),
        ],
    }
}
fn add_segment(list: &mut Vec<Segment>, segment: Segment) -> Result<()> {
    if let Some(existing) = list.iter().find(|other| other.duplicate(&segment)) {
        if !related_endpoint(existing, &segment, segment.start)
            || !related_endpoint(existing, &segment, segment.end)
        {
            return Err(invalid(
                "Distinct source meridians collapse onto the same offset span",
            ));
        }
        if existing.opening != segment.opening {
            return Err(unsupported(
                "Removing part of a rotational end face requires a nonrotational opening construction",
            ));
        }
    } else {
        list.push(segment);
    }
    Ok(())
}

pub(crate) fn shell(
    solid: &Solid,
    thickness: f64,
    removed: &[FaceId],
    outward: bool,
    overrides: &HashMap<FaceId, f64>,
    history: &mut ShellHistory,
) -> Result<Option<Vec<Solid>>> {
    if solid.boundaries().len() != 1 || solid.face_iter().count() > MAX_FACES {
        return Ok(None);
    }
    let faces = solid.face_iter().cloned().collect::<Vec<_>>();
    let mut carriers = HashMap::new();
    for face in &faces {
        let Some(carrier) = analytic_offset::recognize(face)? else {
            return Ok(None);
        };
        carriers.insert(face.id(), carrier);
    }
    let Some((origin, axis)) = faces
        .iter()
        .filter_map(|face| carriers.get(&face.id()))
        .find_map(|carrier| match carrier {
            Carrier::Cone { origin, axis, .. } | Carrier::Torus { origin, axis, .. } => {
                Some((*origin, *axis))
            }
            _ => None,
        })
    else {
        return Ok(None);
    };
    let frame = Frame { origin, axis };
    let direction = if outward { 1. } else { -1. };
    let mut walls = HashMap::new();
    for face in &faces {
        let carrier = carriers[&face.id()];
        let Some(before) = frame.equation(carrier) else {
            return Ok(None);
        };
        let plane = matches!(carrier, Carrier::Plane { .. });
        if removed.contains(&face.id()) && !plane {
            return Ok(None);
        }
        let side = sign(face, &frame, before)?;
        let distance = if removed.contains(&face.id()) {
            0.
        } else {
            direction * side * overrides.get(&face.id()).copied().unwrap_or(thickness)
        };
        walls.insert(
            face.id(),
            Wall {
                before,
                after: before.offset(distance)?,
                side,
                plane,
            },
        );
    }
    let mut incidence: HashMap<VertexId, Vec<Wall>> = HashMap::new();
    let mut source_edges = HashMap::new();
    let mut kinds = HashMap::new();
    let mut edge_walls = HashMap::new();
    for face in &faces {
        for edge in face.edge_iter() {
            let Some(edge_kind) = kind(&edge, &frame)? else {
                return Ok(None);
            };
            for v in [edge.absolute_front(), edge.absolute_back()] {
                incidence.entry(v.id()).or_default().push(walls[&face.id()]);
            }
            source_edges.entry(edge.id()).or_insert_with(|| {
                if edge.orientation() {
                    edge.clone()
                } else {
                    edge.inverse()
                }
            });
            kinds.insert(edge.id(), edge_kind);
            edge_walls.entry(edge.id()).or_insert(walls[&face.id()]);
        }
    }
    // A latitude consists of angular edges in the original B-rep. Its native
    // connected vertices become one meridian station; coincident coordinates
    // on disconnected source boundaries never imply adjacency.
    let mut stations = incidence
        .keys()
        .map(|id| (*id, *id))
        .collect::<HashMap<_, _>>();
    fn station(stations: &HashMap<VertexId, VertexId>, mut id: VertexId) -> VertexId {
        while stations[&id] != id {
            id = stations[&id];
        }
        id
    }
    for edge in source_edges
        .values()
        .filter(|edge| kinds[&edge.id()] == EdgeKind::Angular)
    {
        let a = station(&stations, edge.absolute_front().id());
        let b = station(&stations, edge.absolute_back().id());
        if a != b {
            stations.insert(b, a);
        }
    }
    let stations = stations
        .keys()
        .map(|id| (*id, station(&stations, *id)))
        .collect::<HashMap<_, _>>();
    let mut vertices: HashMap<VertexId, Vertex> = HashMap::new();
    for source in source_edges
        .values()
        .flat_map(|edge| [edge.absolute_front(), edge.absolute_back()])
    {
        if vertices.contains_key(&source.id()) {
            continue;
        }
        let p = source.point();
        let rz = frame.rz(p);
        let next = vertex(rz, &incidence[&source.id()])?;
        if rz.x <= EPS && next.x > EPS {
            return Err(unsupported(
                "An axis pole expands to a circle and requires a topology change",
            ));
        }
        let radial = frame.radial(p).unwrap_or_else(Vector3::zero);
        let next = frame.place(next, radial);
        if ![next.x, next.y, next.z].iter().all(|x| x.is_finite()) {
            return Err(invalid("Offset vertex exceeds finite coordinates"));
        }
        vertices.insert(source.id(), builder::vertex(next));
    }
    let mut edges = HashMap::new();
    let mut before_profile = Vec::new();
    let mut after_profile = Vec::new();
    for (id, edge) in &source_edges {
        let wall = edge_walls[id];
        let kind = kinds[id];
        let front = &vertices[&edge.absolute_front().id()];
        let back = &vertices[&edge.absolute_back().id()];
        if front.point().distance(back.point()) <= EPS {
            return Err(invalid("Thickness collapses a topological edge"));
        }
        let changed = Edge::try_new(
            front,
            back,
            edge_curve(edge, kind, &frame, wall, front, back)?,
        )
        .map_err(|e| invalid(&e.to_string()))?;
        if kind == EdgeKind::Meridian {
            let opening = wall.before.same(wall.after);
            let ancestry = [
                Some(stations[&edge.absolute_front().id()]),
                Some(stations[&edge.absolute_back().id()]),
            ];
            let mut original = make_segment(edge, &frame, wall.before, opening);
            original.stations = ancestry;
            add_segment(&mut before_profile, original)?;
            let mut segment = make_segment(&changed, &frame, wall.after, opening);
            segment.source_start = frame.rz(edge.absolute_front().point());
            segment.source_end = frame.rz(edge.absolute_back().point());
            segment.stations = ancestry;
            add_segment(&mut after_profile, segment)?;
        }
        history.construct_edge(
            changed.id(),
            &[history.edge(*id)?],
            serde_json::json!({"operation":"shell","constructor":"intersected_normal_boundary"}),
        )?;
        edges.insert(*id, changed);
    }
    // A planar circular cap can have no meridian edge at all. Its material
    // interval in the radial section is still required for collision checks.
    let mut cap_groups = HashSet::new();
    for face in &faces {
        let wall = walls[&face.id()];
        if !wall.plane || cap_groups.contains(&face.id()) {
            continue;
        }
        let group = faces
            .iter()
            .filter(|other| walls[&other.id()].plane && walls[&other.id()].before.same(wall.before))
            .collect::<Vec<_>>();
        cap_groups.extend(group.iter().map(|face| face.id()));
        if group
            .iter()
            .any(|other| !walls[&other.id()].after.same(wall.after))
        {
            return Err(unsupported(
                "Angular patches of one end plane need the same thickness and opening selection",
            ));
        }
        let cap = group.iter().map(|face| (*face).clone()).collect::<Shell>();
        let boundaries = cap.extract_boundaries();
        if boundaries.is_empty()
            || boundaries.len() > 2
            || boundaries
                .iter()
                .flatten()
                .any(|edge| kinds[&edge.id()] != EdgeKind::Angular)
        {
            return Ok(None);
        }
        let mut paired = Vec::new();
        for wire in boundaries {
            let edge = wire
                .front()
                .ok_or_else(|| invalid("End-plane boundary is empty"))?;
            paired.push((
                frame.rz(edge.front().point()),
                frame.rz(vertices[&edge.front().id()].point()),
                Some(stations[&edge.front().id()]),
            ));
        }
        paired.sort_by(|a, b| a.0.x.total_cmp(&b.0.x));
        if paired.len() == 1 {
            let pole = group
                .iter()
                .flat_map(|face| face.vertex_iter())
                .find(|vertex| frame.rz(vertex.point()).x <= EPS)
                .map(|vertex| stations[&vertex.id()]);
            paired.insert(
                0,
                (
                    Point2::new(0., paired[0].0.y),
                    Point2::new(0., paired[0].1.y),
                    pole,
                ),
            );
        }
        let old = [paired[0].0, paired[1].0];
        let new = [paired[0].1, paired[1].1];
        let ancestry = [paired[0].2, paired[1].2];
        let opening = wall.before.same(wall.after);
        add_segment(
            &mut before_profile,
            Segment {
                equation: wall.before,
                start: old[0],
                end: old[1],
                mid: old[0] + (old[1] - old[0]) * 0.5,
                opening,
                source_start: old[0],
                source_end: old[1],
                stations: ancestry,
            },
        )?;
        add_segment(
            &mut after_profile,
            Segment {
                equation: wall.after,
                start: new[0],
                end: new[1],
                mid: new[0] + (new[1] - new[0]) * 0.5,
                opening,
                source_start: old[0],
                source_end: old[1],
                stations: ancestry,
            },
        )?;
    }
    profiles(&before_profile, &after_profile, outward)?;
    let mut changed = Shell::new();
    let mut changed_faces = HashMap::new();
    for face in &faces {
        let wall = walls[&face.id()];
        let wires = face
            .boundaries()
            .into_iter()
            .map(|wire| {
                wire.iter()
                    .map(|edge| orient(&edges[&edge.id()], edge))
                    .collect()
            })
            .collect::<Vec<Wire>>();
        let mapped = if wall.plane {
            builder::try_attach_plane(wires).map_err(|e| invalid(&e.to_string()))?
        } else {
            let Some(meridian) = face
                .edge_iter()
                .find(|edge| kinds[&edge.id()] == EdgeKind::Meridian)
            else {
                return Err(unsupported(
                    "Periodic turned face needs an explicit meridian seam",
                ));
            };
            let curve = edges[&meridian.id()].curve();
            let (a, b) = curve.range_tuple();
            let p = curve.subs((a + b) * 0.5);
            let expected = wall.after.normal(frame.rz(p));
            let radial = frame
                .radial(p)
                .ok_or_else(|| invalid("Offset meridian degenerates at the axis"))?;
            let expected = (radial * expected.x + axis * expected.y) * wall.side;
            let mut carrier = Surface::RevolutionSurface(Processor::new(
                RevolutionSurface::by_revolution(curve, origin, axis),
            ));
            let normal = carrier.normal((a + b) * 0.5, 0.).normalize();
            let alignment = normal.dot(expected);
            if !alignment.is_finite() || alignment.abs() < 1. - 1e-8 {
                return Err(invalid(
                    "Reconstructed offset surface lost its normal orientation",
                ));
            }
            if alignment < 0. {
                carrier.invert();
            }
            Face::try_new(wires, carrier).map_err(|e| invalid(&e.to_string()))?
        };
        history.construct_face(mapped.id(),&[history.face(face.id())?],serde_json::json!({"operation":"shell","constructor":if outward{"outer_intersected_normal_wall"}else{"inner_intersected_normal_wall"}}))?;
        changed_faces.insert(face.id(), mapped.id());
        changed.push(mapped);
    }
    crate::validation::solid(
        &Solid::try_new(vec![changed.clone()]).map_err(|e| invalid(&e.to_string()))?,
    )?;
    let mut result = Shell::new();
    if outward {
        result.extend(
            changed
                .iter()
                .filter(|face| {
                    !removed
                        .iter()
                        .any(|source| changed_faces[source] == face.id())
                })
                .cloned(),
        );
        result.extend(
            faces
                .iter()
                .filter(|face| !removed.contains(&face.id()))
                .map(Face::inverse),
        );
    } else {
        result.extend(
            faces
                .iter()
                .filter(|face| !removed.contains(&face.id()))
                .cloned(),
        );
        result.extend(
            changed
                .iter()
                .filter(|face| {
                    !removed
                        .iter()
                        .any(|source| changed_faces[source] == face.id())
                })
                .map(Face::inverse),
        );
    }
    let mut done = HashSet::new();
    for opening in removed {
        if done.contains(opening) {
            continue;
        }
        let equation = walls[opening].before;
        let group = faces
            .iter()
            .filter(|face| walls[&face.id()].plane && walls[&face.id()].before.same(equation))
            .collect::<Vec<_>>();
        if group.iter().any(|face| !removed.contains(&face.id())) {
            return Err(unsupported(
                "Select every angular patch of a rotational opening plane",
            ));
        }
        done.extend(group.iter().map(|face| face.id()));
        let opening_shell = group.iter().map(|face| (*face).clone()).collect::<Shell>();
        let normal = group[0].oriented_surface().normal(0., 0.).normalize();
        for wire in opening_shell.extract_boundaries() {
            if wire
                .iter()
                .any(|edge| kinds[&edge.id()] != EdgeKind::Angular)
            {
                return Err(unsupported(
                    "Rotational opening requires complete circular boundary loops",
                ));
            }
            let mapped = wire
                .iter()
                .map(|edge| orient(&edges[&edge.id()], edge))
                .collect::<Wire>();
            let radius = |wire: &Wire| -> Result<f64> {
                let edge = wire
                    .front()
                    .ok_or_else(|| invalid("Opening loop is empty"))?;
                Ok(crate::axis_geometry::circle_geometry(&edge.curve())?.2)
            };
            let (mut outer, mut inner) = if radius(&wire)? > radius(&mapped)? {
                (wire.clone(), mapped)
            } else {
                (mapped, wire.clone())
            };
            let ((_, axis, _), angle) = crate::bounded_carriers::circle(
                &outer
                    .front()
                    .ok_or_else(|| invalid("Opening loop is empty"))?
                    .oriented_curve(),
            )?;
            if axis.dot(normal) * angle < 0. {
                outer.invert();
            }
            let ((_, axis, _), angle) = crate::bounded_carriers::circle(
                &inner
                    .front()
                    .ok_or_else(|| invalid("Opening loop is empty"))?
                    .oriented_curve(),
            )?;
            if axis.dot(normal) * angle > 0. {
                inner.invert();
            }
            let rim = builder::try_attach_plane(vec![outer, inner])
                .map_err(|e| invalid(&e.to_string()))?;
            let mut sources = group
                .iter()
                .map(|face| history.face(face.id()))
                .collect::<Result<Vec<_>>>()?;
            sources.extend(
                wire.iter()
                    .map(|edge| history.edge(edge.id()))
                    .collect::<Result<Vec<_>>>()?,
            );
            history.rim(
                rim.id(),
                *opening,
                &sources,
                serde_json::json!({"operation":"shell","constructor":"turned_opening_ring"}),
            )?;
            result.push(rim);
        }
    }
    let components = result.connected_components();
    let results = if removed.is_empty() {
        vec![Solid::try_new(components).map_err(|e| invalid(&e.to_string()))?]
    } else {
        components
            .into_iter()
            .map(|shell| Solid::try_new(vec![shell]).map_err(|e| invalid(&e.to_string())))
            .collect::<Result<Vec<_>>>()?
    };
    for result in &results {
        crate::validation::solid(result)?;
    }
    Ok(Some(results))
}
