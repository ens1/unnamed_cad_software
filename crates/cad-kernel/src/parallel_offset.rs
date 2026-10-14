//! Exact offsets of line/arc sheets extruded along one common direction.
//!
//! The complete two-dimensional normal collars are bounded by lines and
//! circular arcs. Analytic intersections and containment exclude collisions
//! throughout the requested thickness, including between its two sides.
//! Shared native generator edges identify permitted contacts; proximity never
//! invents adjacency. The output retains exact carriers and shared topology.
use crate::analytic_offset::{self, Carrier, EPS, Offset};
use crate::native_bounds::Interval;
use cad_core::{CadError, Result};
use monstertruck::modeling::*;
use std::collections::HashMap;

const REL: f64 = 1e-10;
const MAX_FACES: usize = 512;
const MAX_CURVE_CONTROLS: usize = 4096;
const MAX_TOTAL_CONTROLS: usize = 131_072;

fn invalid(message: &str) -> CadError {
    CadError::Invalid(format!("Extruded sheet offset: {message}"))
}
fn unsupported(message: &str) -> CadError {
    CadError::Unsupported(format!("Extruded sheet offset: {message}"))
}
fn finite(p: Point2) -> bool {
    p.x.is_finite() && p.y.is_finite()
}
fn cross(a: Vector2, b: Vector2) -> f64 {
    a.x * b.y - a.y * b.x
}

struct Frame {
    origin: Point3,
    axis: Vector3,
    x: Vector3,
    y: Vector3,
}
impl Frame {
    fn point(&self, p: Point3) -> Point2 {
        let p = p - self.origin;
        Point2::new(p.dot(self.x), p.dot(self.y))
    }
    fn vector(&self, v: Vector3) -> Vector2 {
        Vector2::new(v.dot(self.x), v.dot(self.y))
    }
    fn height(&self, p: Point3) -> f64 {
        (p - self.origin).dot(self.axis)
    }
}

#[derive(Clone, Copy)]
enum Support {
    Line { normal: Vector2, offset: f64 },
    Circle { center: Point2, radius: f64 },
}
impl Support {
    fn same(self, other: Self) -> bool {
        match (self, other) {
            (
                Self::Line {
                    normal: a,
                    offset: x,
                },
                Self::Line {
                    normal: b,
                    offset: y,
                },
            ) => (a == b && x == y) || (a == -b && x == -y),
            (
                Self::Circle {
                    center: a,
                    radius: x,
                },
                Self::Circle {
                    center: b,
                    radius: y,
                },
            ) => a == b && x == y,
            _ => false,
        }
    }
    fn residual(self, p: Point2) -> f64 {
        match self {
            Self::Line { normal, offset } => p.to_vec().dot(normal) - offset,
            Self::Circle { center, radius } => p.distance(center) - radius,
        }
    }
}

#[derive(Clone, Copy)]
struct Span {
    support: Support,
    start: Point2,
    end: Point2,
    /// Signed complete angular traversal; zero for a straight segment.
    sweep: f64,
}
impl Span {
    fn line(start: Point2, end: Point2) -> Result<Self> {
        let delta = end - start;
        let length = delta.magnitude();
        if !finite(start) || !finite(end) || !length.is_finite() || length <= EPS {
            return Err(invalid(
                "An offset boundary collapses or exceeds finite coordinates",
            ));
        }
        let normal = Vector2::new(-delta.y, delta.x) / length;
        Ok(Self {
            support: Support::Line {
                normal,
                offset: start.to_vec().dot(normal),
            },
            start,
            end,
            sweep: 0.,
        })
    }
    fn native(curve: &Curve, frame: &Frame) -> Result<Option<Self>> {
        let (a, b) = curve.range_tuple();
        let start = frame.point(curve.subs(a));
        let end = frame.point(curve.subs(b));
        if crate::bounded_carriers::line(curve).is_ok() {
            return Self::line(start, end).map(Some);
        }
        let Ok(((center, normal, radius), sweep)) = crate::bounded_carriers::circle(curve) else {
            return Ok(None);
        };
        if normal.dot(frame.axis).abs() < 1. - REL {
            return Ok(None);
        }
        Ok(Some(Self {
            support: Support::Circle {
                center: frame.point(center),
                radius,
            },
            start,
            end,
            sweep: sweep * normal.dot(frame.axis).signum(),
        }))
    }
    fn point(self, fraction: f64) -> Point2 {
        match self.support {
            Support::Line { .. } => self.start + (self.end - self.start) * fraction,
            Support::Circle { center, .. } => {
                let delta = self.start - center;
                let angle = self.sweep * fraction;
                center
                    + Vector2::new(
                        delta.x * angle.cos() - delta.y * angle.sin(),
                        delta.x * angle.sin() + delta.y * angle.cos(),
                    )
            }
        }
    }
    fn contains(self, p: Point2) -> bool {
        if !finite(p) || self.support.residual(p).abs() > EPS {
            return false;
        }
        if p.distance(self.start) <= EPS || p.distance(self.end) <= EPS {
            return true;
        }
        match self.support {
            Support::Line { .. } => {
                let delta = self.end - self.start;
                let t = (p - self.start).dot(delta) / delta.magnitude2();
                (0. ..=1.).contains(&t)
            }
            Support::Circle { center, radius } => {
                let a = self.start - center;
                let b = p - center;
                let angle = (cross(a, b).atan2(a.dot(b)) * self.sweep.signum())
                    .rem_euclid(std::f64::consts::TAU);
                angle <= self.sweep.abs() + EPS / radius
            }
        }
    }
    fn same(self, other: Self) -> bool {
        // Bounded rail/native-seam equivalence is distinct from coincidence
        // of infinite supports in the intersection classifier.
        let same_support = match (self.support, other.support) {
            (Support::Line { .. }, Support::Line { .. }) => true,
            (
                Support::Circle {
                    center: a,
                    radius: r,
                },
                Support::Circle {
                    center: b,
                    radius: s,
                },
            ) => {
                a.distance(b) + (r - s).abs() <= EPS
                    && (self.sweep.abs() - other.sweep.abs()).abs() * r.max(s) <= EPS
            }
            _ => false,
        };
        same_support
            && ((self.start.distance(other.start) <= EPS && self.end.distance(other.end) <= EPS)
                || (self.start.distance(other.end) <= EPS && self.end.distance(other.start) <= EPS))
            && self.contains(other.point(0.5))
            && other.contains(self.point(0.5))
    }
    fn reversed(self) -> Self {
        Self {
            start: self.end,
            end: self.start,
            sweep: -self.sweep,
            ..self
        }
    }
}

/// None denotes coincident supports; every finite intersection is otherwise
/// returned. Tangencies remain contacts and are rejected unless native.
fn intersections(
    a: Support,
    b: Support,
    native_contacts: &[Point2],
) -> Result<Option<Vec<Point2>>> {
    if a.same(b) {
        return Ok(None);
    }
    let number = Interval::exact;
    let zero = number(0.)?;
    let half_roots = |middle: [Interval; 2],
                      direction: [Interval; 2],
                      square: Interval|
     -> Result<Vec<[Interval; 2]>> {
        if square.high < 0. {
            return Ok(Vec::new());
        }
        // A discriminant straddling zero may describe no intersection, a
        // tangency, or two roots. Enclose every possible root; never replace
        // that uncertainty with a vote that the bounded curves are disjoint.
        let half = Interval::new(square.low.max(0.), square.high)?.sqrt()?;
        Ok(vec![
            [
                middle[0].add(direction[0].mul(half)?)?,
                middle[1].add(direction[1].mul(half)?)?,
            ],
            [
                middle[0].sub(direction[0].mul(half)?)?,
                middle[1].sub(direction[1].mul(half)?)?,
            ],
        ])
    };
    let points = match (a, b) {
        (
            Support::Line {
                normal: a,
                offset: x,
            },
            Support::Line {
                normal: b,
                offset: y,
            },
        ) => {
            if a == b || a == -b {
                return Ok(Some(Vec::new()));
            }
            let determinant = number(a.x)?
                .mul(number(b.y)?)?
                .sub(number(a.y)?.mul(number(b.x)?)?)?;
            vec![[
                number(x)?
                    .mul(number(b.y)?)?
                    .sub(number(a.y)?.mul(number(y)?)?)?
                    .div(determinant)?,
                number(a.x)?
                    .mul(number(y)?)?
                    .sub(number(x)?.mul(number(b.x)?)?)?
                    .div(determinant)?,
            ]]
        }
        (Support::Line { normal, offset }, Support::Circle { center, radius })
        | (Support::Circle { center, radius }, Support::Line { normal, offset }) => {
            let nx = number(normal.x)?;
            let ny = number(normal.y)?;
            let length2 = nx.mul(nx)?.add(ny.mul(ny)?)?;
            let length = length2.sqrt()?;
            let distance = number(center.x)?
                .mul(nx)?
                .add(number(center.y)?.mul(ny)?)?
                .sub(number(offset)?)?;
            let along = distance.div(length2)?;
            let middle = [
                number(center.x)?.sub(nx.mul(along)?)?,
                number(center.y)?.sub(ny.mul(along)?)?,
            ];
            let square = number(radius)?
                .mul(number(radius)?)?
                .sub(distance.mul(distance)?.div(length2)?)?;
            half_roots(
                middle,
                [zero.sub(ny)?.div(length)?, nx.div(length)?],
                square,
            )?
        }
        (
            Support::Circle {
                center: a,
                radius: r,
            },
            Support::Circle {
                center: b,
                radius: s,
            },
        ) => {
            if a == b {
                return Ok(Some(Vec::new()));
            }
            let dx = number(b.x)?.sub(number(a.x)?)?;
            let dy = number(b.y)?.sub(number(a.y)?)?;
            let distance2 = dx.mul(dx)?.add(dy.mul(dy)?)?;
            let distance = distance2.sqrt()?;
            let along = distance2
                .add(
                    number(r)?
                        .sub(number(s)?)?
                        .mul(number(r)?.add(number(s)?)?)?,
                )?
                .div(number(2.)?.mul(distance)?)?;
            let factor = along.div(distance)?;
            let middle = [
                number(a.x)?.add(dx.mul(factor)?)?,
                number(a.y)?.add(dy.mul(factor)?)?,
            ];
            let square = number(r)?.sub(along)?.mul(number(r)?.add(along)?)?;
            half_roots(
                middle,
                [zero.sub(dy)?.div(distance)?, dx.div(distance)?],
                square,
            )?
        }
    };
    points
        .into_iter()
        .map(|bounds| {
            for native in native_contacts {
                let dx = number(native.x)?.sub(bounds[0])?;
                let dy = number(native.y)?.sub(bounds[1])?;
                let max_x = dx.low.abs().max(dx.high.abs());
                let max_y = dy.low.abs().max(dy.high.abs());
                let distance = number(max_x)?
                    .mul(number(max_x)?)?
                    .add(number(max_y)?.mul(number(max_y)?)?)?
                    .sqrt()?;
                if distance.high <= EPS {
                    // Every possible root is enclosed inside one actual
                    // native join's positional tolerance. Remote or unshared
                    // uncertain contacts never receive this exception.
                    return Ok(*native);
                }
            }
            if bounds
                .iter()
                .any(|bound| bound.high - bound.low > EPS / 16.)
            {
                return Err(unsupported(
                    "A collar intersection cannot be enclosed within native precision",
                ));
            }
            let point = Point2::new(
                bounds[0].low + (bounds[0].high - bounds[0].low) * 0.5,
                bounds[1].low + (bounds[1].high - bounds[1].low) * 0.5,
            );
            for support in [a, b] {
                let residual = residual_bound(support, point)?;
                if residual.low < -EPS / 4. || residual.high > EPS / 4. {
                    return Err(unsupported(
                        "A collar intersection cannot be validated on both native supports",
                    ));
                }
            }
            Ok(point)
        })
        .collect::<Result<Vec<_>>>()
        .map(Some)
}

fn residual_bound(support: Support, point: Point2) -> Result<Interval> {
    let n = Interval::exact;
    match support {
        Support::Line { normal, offset } => n(point.x)?
            .mul(n(normal.x)?)?
            .add(n(point.y)?.mul(n(normal.y)?)?)?
            .sub(n(offset)?),
        Support::Circle { center, radius } => {
            let x = n(point.x)?.sub(n(center.x)?)?;
            let y = n(point.y)?.sub(n(center.y)?)?;
            x.mul(x)?.add(y.mul(y)?)?.sqrt()?.sub(n(radius)?)
        }
    }
}

struct Strip {
    face: FaceId,
    carrier: Carrier,
    side: f64,
    profile: Span,
    /// Complete common generator edges at the profile's two endpoints.
    ends: [Join; 2],
    /// Oriented unit normal for straight rails; radial sign for circular rails.
    normal: Vector2,
}
impl Strip {
    fn profile_at(&self, distance: f64) -> Result<Span> {
        match self.profile.support {
            Support::Line { .. } => Span::line(
                self.profile.start + self.normal * distance,
                self.profile.end + self.normal * distance,
            ),
            Support::Circle { center, radius } => {
                let next = radius + self.side * distance;
                if !next.is_finite() || next <= EPS {
                    return Err(invalid("Thickness reaches a cylindrical bend's focal axis"));
                }
                Ok(Span {
                    support: Support::Circle {
                        center,
                        radius: next,
                    },
                    start: center + (self.profile.start - center) * (next / radius),
                    end: center + (self.profile.end - center) * (next / radius),
                    sweep: self.profile.sweep,
                })
            }
        }
    }
    fn normal_at(&self, p: Point2) -> Vector2 {
        match self.profile.support {
            Support::Line { .. } => self.normal,
            Support::Circle { center, radius } => (p - center) * (self.side / radius),
        }
    }
    fn collar(&self, lower: f64, upper: f64) -> Result<Collar> {
        let a = self.profile_at(lower)?;
        let b = self.profile_at(upper)?;
        Ok(Collar {
            boundaries: [
                a,
                Span::line(a.end, b.end)?,
                b.reversed(),
                Span::line(b.start, a.start)?,
            ],
            ends: self.ends,
            witness: self.profile_at((lower + upper) * 0.5)?.point(0.5),
        })
    }
}

struct Collar {
    boundaries: [Span; 4],
    ends: [Join; 2],
    witness: Point2,
}
impl Collar {
    fn connector(&self, end: usize) -> Span {
        self.boundaries[if end == 0 { 3 } else { 1 }]
    }
    fn contains(&self, query: Point2) -> Result<bool> {
        if self.boundaries.iter().any(|span| span.contains(query)) {
            return Ok(true);
        }
        // Avoid endpoint rays and tangent intersections. Each accepted ray
        // therefore counts only transverse interior intersections exactly.
        for attempt in 0..32 {
            let angle = (attempt as f64 + 0.3819660112501051) * std::f64::consts::PI / 32.;
            let ray = Vector2::new(angle.cos(), angle.sin());
            let normal = Vector2::new(-ray.y, ray.x);
            let support = Support::Line {
                normal,
                offset: query.to_vec().dot(normal),
            };
            let center_residual = residual_bound(support, query)?;
            if center_residual.low < -EPS / 16. || center_residual.high > EPS / 16. {
                return Err(unsupported(
                    "The containment ray cannot be placed within native precision",
                ));
            }
            let mut near_endpoint = false;
            for span in &self.boundaries {
                for endpoint in [span.start, span.end] {
                    let residual = residual_bound(support, endpoint)?;
                    near_endpoint |= residual.low <= EPS && residual.high >= -EPS;
                }
            }
            if near_endpoint {
                continue;
            }
            let mut count = 0;
            let mut ambiguous = false;
            for span in &self.boundaries {
                let points = match intersections(support, span.support, &[]) {
                    Ok(Some(points)) => points,
                    Ok(None) | Err(CadError::Unsupported(_)) => {
                        ambiguous = true;
                        break;
                    }
                    Err(error) => return Err(error),
                };
                if points.len() == 2 && points[0].distance(points[1]) <= EPS {
                    ambiguous = true;
                    break;
                }
                for point in points {
                    let distance = Interval::exact(point.x)?
                        .sub(Interval::exact(query.x)?)?
                        .mul(Interval::exact(ray.x)?)?
                        .add(
                            Interval::exact(point.y)?
                                .sub(Interval::exact(query.y)?)?
                                .mul(Interval::exact(ray.y)?)?,
                        )?;
                    if distance.low <= EPS && distance.high >= -EPS {
                        ambiguous = true;
                        break;
                    }
                    if distance.low > EPS && span.contains(point) {
                        count += 1;
                    }
                }
            }
            if !ambiguous {
                return Ok(count % 2 == 1);
            }
        }
        Err(unsupported(
            "No transverse analytic collar-classification ray could be established",
        ))
    }
}

fn collars_disjoint(a: &Collar, b: &Collar) -> Result<bool> {
    let shared = (0..2)
        .flat_map(|i| (0..2).map(move |j| (i, j)))
        .filter(|(i, j)| a.ends[*i] == b.ends[*j])
        .map(|(i, j)| {
            let x = a.connector(i);
            let y = b.connector(j);
            if !x.same(y) {
                return Err(unsupported(
                    "Adjacent offset strips do not share a complete normal seam",
                ));
            }
            Ok((i, j, x))
        })
        .collect::<Result<Vec<_>>>()?;
    for (xi, x) in a.boundaries.iter().enumerate() {
        for (yi, y) in b.boundaries.iter().enumerate() {
            let allowed = |point: Point2| {
                shared.iter().any(|(i, j, span)| {
                    let endpoint = |edge: &Span| {
                        point.distance(edge.start) <= EPS || point.distance(edge.end) <= EPS
                    };
                    span.contains(point)
                        && (xi == if *i == 0 { 3 } else { 1 } || endpoint(x))
                        && (yi == if *j == 0 { 3 } else { 1 } || endpoint(y))
                })
            };
            if shared.iter().any(|(i, j, _)| {
                xi == if *i == 0 { 3 } else { 1 } && yi == if *j == 0 { 3 } else { 1 }
            }) {
                continue;
            }
            let native_contacts = shared
                .iter()
                .flat_map(|(_, _, span)| [span.start, span.end])
                .filter(|point| allowed(*point))
                .collect::<Vec<_>>();
            if let (Support::Line { normal: n, .. }, Support::Line { normal: m, .. }) =
                (x.support, y.support)
                && cross(n, m).abs() <= REL
                && !x.support.same(y.support)
            {
                // Complete endpoint bounds can prove disjointness even when
                // the infinite lines' intersection is ill conditioned.
                let separated = |span: &Span, support: Support| -> Result<bool> {
                    let a = residual_bound(support, span.start)?;
                    let b = residual_bound(support, span.end)?;
                    Ok((a.low > EPS && b.low > EPS) || (a.high < -EPS && b.high < -EPS))
                };
                if separated(x, y.support)? || separated(y, x.support)? {
                    continue;
                }
            }
            if let Some(points) = intersections(x.support, y.support, &native_contacts)? {
                if points
                    .into_iter()
                    .any(|p| x.contains(p) && y.contains(p) && !allowed(p))
                {
                    return Ok(false);
                }
            } else {
                // Split coincident supports at every interval endpoint. A
                // positive-length overlap contains an endpoint or a midpoint
                // of one of the resulting exact line/arc intervals.
                let mut parameters = vec![0., 1.];
                for point in [y.start, y.end] {
                    if !x.contains(point) {
                        continue;
                    }
                    let t = match x.support {
                        Support::Line { .. } => {
                            (point - x.start).dot(x.end - x.start) / (x.end - x.start).magnitude2()
                        }
                        Support::Circle { center, .. } => {
                            let p = x.start - center;
                            let q = point - center;
                            (cross(p, q).atan2(p.dot(q)) * x.sweep.signum())
                                .rem_euclid(std::f64::consts::TAU)
                                / x.sweep.abs()
                        }
                    };
                    parameters.push(t.clamp(0., 1.));
                }
                parameters.sort_by(f64::total_cmp);
                let candidates = parameters
                    .iter()
                    .copied()
                    .chain(parameters.windows(2).map(|v| (v[0] + v[1]) * 0.5));
                if candidates
                    .map(|t| x.point(t))
                    .any(|p| y.contains(p) && !allowed(p))
                {
                    return Ok(false);
                }
            }
        }
    }
    Ok(!a.contains(b.witness)? && !b.contains(a.witness)?)
}

struct Sheet {
    strips: Vec<Strip>,
}

/// Contacts are authorized by actual source topology, not proximity. Sheet
/// collars share a generator; planar contour collars share a native vertex.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Join {
    Generator(EdgeId),
    Vertex(VertexId),
}
impl Sheet {
    fn recognize(shell: &Shell) -> Result<Option<Self>> {
        if shell.is_empty() || shell.len() > MAX_FACES {
            return Ok(None);
        }
        let mut source_edges = std::collections::HashSet::new();
        let mut controls = 0usize;
        for edge in shell
            .edge_iter()
            .filter(|edge| source_edges.insert(edge.id()))
        {
            let count = match edge.curve() {
                Curve::Line(_) => 2,
                Curve::BsplineCurve(curve) => curve.control_points().len(),
                Curve::NurbsCurve(curve) => curve.control_points().len(),
                _ => return Ok(None),
            };
            controls = controls.saturating_add(count);
            if count > MAX_CURVE_CONTROLS || controls > MAX_TOTAL_CONTROLS {
                return Err(unsupported(
                    "Exact boundary recognition exceeds the native control budget",
                ));
            }
        }
        let mut carriers = Vec::new();
        for face in shell {
            let Some(carrier) = analytic_offset::recognize(face)? else {
                return Ok(None);
            };
            if !matches!(
                carrier,
                Carrier::Plane { .. } | Carrier::Cone { slope: 0., .. }
            ) {
                return Ok(None);
            }
            carriers.push(carrier);
        }
        if carriers.iter().all(|carrier| carriers[0].same(*carrier)) {
            let side = analytic_offset::side(&shell[0], carriers[0])?;
            let mut common = true;
            for face in shell {
                common &= analytic_offset::side(face, carriers[0])? == side;
            }
            // A single analytic normal field already has a global offset
            // certificate, including arbitrary bounded trims. Keep that
            // broader existing path instead of needlessly partitioning it.
            if common {
                return Ok(None);
            }
        }
        let Some(axis) = carriers.iter().find_map(|c| match c {
            Carrier::Cone { axis, .. } => Some(*axis),
            _ => None,
        }) else {
            return Ok(None);
        };
        let seed = if axis.x.abs() < 0.8 {
            Vector3::unit_x()
        } else {
            Vector3::unit_y()
        };
        let x = (seed - axis * seed.dot(axis)).normalize();
        let Some(origin) = shell[0].vertex_iter().next().map(|v| v.point()) else {
            return Ok(None);
        };
        let frame = Frame {
            origin,
            axis,
            x,
            y: axis.cross(x),
        };
        let mut strips = Vec::new();
        let mut limits: Option<[f64; 2]> = None;
        let mut seams: HashMap<Join, (Point2, Vector2, usize)> = HashMap::new();
        let mut rails = std::collections::HashSet::new();
        for (face, carrier) in shell.iter().zip(carriers) {
            match carrier {
                Carrier::Plane { normal, .. } if normal.dot(axis).abs() <= REL => {}
                Carrier::Cone { axis: other, .. } if other.dot(axis).abs() > 1. - REL => {}
                _ => return Ok(None),
            }
            let wires = face.boundaries();
            if wires.len() != 1 || wires[0].len() != 4 {
                return Ok(None);
            }
            let wire = &wires[0];
            let heights = wire
                .iter()
                .map(|edge| frame.height(edge.front().point()))
                .collect::<Vec<_>>();
            if heights.iter().any(|v| !v.is_finite()) {
                return Err(invalid("Source sheet exceeds finite coordinates"));
            }
            let low = heights.iter().copied().fold(f64::INFINITY, f64::min);
            let high = heights.iter().copied().fold(f64::NEG_INFINITY, f64::max);
            if high - low <= EPS {
                return Ok(None);
            }
            if let Some(old) = limits {
                if (old[0] - low).abs() > EPS || (old[1] - high).abs() > EPS {
                    return Ok(None);
                }
            } else {
                limits = Some([low, high]);
            }
            let on_level = |edge: &Edge, level: f64| {
                (frame.height(edge.front().point()) - level).abs() <= EPS
                    && (frame.height(edge.back().point()) - level).abs() <= EPS
            };
            let lower = wire
                .iter()
                .filter(|edge| on_level(edge, low))
                .collect::<Vec<_>>();
            let upper = wire
                .iter()
                .filter(|edge| on_level(edge, high))
                .collect::<Vec<_>>();
            if lower.len() != 1 || upper.len() != 1 {
                return Ok(None);
            }
            if !rails.insert(lower[0].id()) || !rails.insert(upper[0].id()) {
                return Ok(None);
            }
            let Some(profile) = Span::native(&lower[0].oriented_curve(), &frame)? else {
                return Ok(None);
            };
            let Some(opposite) = Span::native(&upper[0].oriented_curve(), &frame)? else {
                return Ok(None);
            };
            if !profile.same(opposite) {
                return Ok(None);
            }
            let side = analytic_offset::side(face, carrier)?;
            let normal = match carrier {
                Carrier::Plane { normal, .. } => frame.vector(normal) * side,
                _ => Vector2::zero(),
            };
            match (carrier, profile.support) {
                (Carrier::Plane { .. }, Support::Line { .. }) => {}
                (
                    Carrier::Cone { origin, radius, .. },
                    Support::Circle {
                        center,
                        radius: other,
                    },
                ) if frame.point(origin).distance(center) <= EPS
                    && (radius - other).abs() <= EPS => {}
                _ => return Ok(None),
            }
            let mut ends = Vec::new();
            for vertex in [lower[0].front(), lower[0].back()] {
                let connected = wire
                    .iter()
                    .filter(|edge| {
                        edge.id() != lower[0].id()
                            && edge.id() != upper[0].id()
                            && (edge.front().id() == vertex.id() || edge.back().id() == vertex.id())
                    })
                    .collect::<Vec<_>>();
                if connected.len() != 1 {
                    return Ok(None);
                }
                let edge = connected[0];
                if crate::bounded_carriers::line(&edge.curve()).is_err()
                    || (edge.front().point() - edge.back().point())
                        .normalize()
                        .dot(axis)
                        .abs()
                        < 1. - REL
                {
                    return Ok(None);
                }
                ends.push(edge.id());
            }
            let strip = Strip {
                face: face.id(),
                carrier,
                side,
                profile,
                ends: [Join::Generator(ends[0]), Join::Generator(ends[1])],
                normal,
            };
            for (end, p) in strip.ends.iter().zip([profile.start, profile.end]) {
                let normal = strip.normal_at(p);
                if let Some((old, n, count)) = seams.get_mut(end) {
                    if *count >= 2 || old.distance(p) > EPS || (*n - normal).magnitude() > 1e-8 {
                        return Err(unsupported(
                            "Joined strips require a continuous tangent normal; sharp joins need intersected offsets",
                        ));
                    }
                    *count += 1;
                } else {
                    seams.insert(*end, (p, normal, 1));
                }
            }
            strips.push(strip);
        }
        crate::validation::surface(shell)?;
        Ok(Some(Self { strips }))
    }
    fn certify(&self, lower: f64, upper: f64) -> Result<()> {
        if !lower.is_finite() || !upper.is_finite() {
            return Err(invalid("Offset distances must be finite"));
        }
        let (lower, upper) = (lower.min(upper), lower.max(upper));
        if upper == lower {
            return Ok(());
        }
        if upper - lower <= EPS {
            return Err(invalid(
                "Offset distance is below the native collar resolution",
            ));
        }
        let collars = self
            .strips
            .iter()
            .map(|strip| strip.collar(lower, upper))
            .collect::<Result<Vec<_>>>()?;
        for (i, a) in collars.iter().enumerate() {
            for b in &collars[..i] {
                if !collars_disjoint(a, b)? {
                    return Err(invalid(
                        "Thickness makes complete normal strips cross, overlap or touch away from a native seam",
                    ));
                }
            }
        }
        Ok(())
    }
    fn offset(&self, shell: &Shell, distance: f64) -> Result<Offset> {
        let mut vertices: HashMap<VertexId, Vertex> = HashMap::new();
        let mut edges: HashMap<EdgeId, Edge> = HashMap::new();
        for (face, strip) in shell.iter().zip(&self.strips) {
            for edge in face.edge_iter() {
                for vertex in [edge.absolute_front(), edge.absolute_back()] {
                    let point = strip
                        .carrier
                        .offset_point(vertex.point(), distance * strip.side)?;
                    if let Some(existing) = vertices.get(&vertex.id()) {
                        if existing.point().distance(point) > EPS {
                            return Err(invalid("Normal offset split a shared source vertex"));
                        }
                    } else {
                        vertices.insert(vertex.id(), builder::vertex(point));
                    }
                }
                if let std::collections::hash_map::Entry::Vacant(slot) = edges.entry(edge.id()) {
                    slot.insert(
                        Edge::try_new(
                            &vertices[&edge.absolute_front().id()],
                            &vertices[&edge.absolute_back().id()],
                            strip
                                .carrier
                                .offset_curve(&edge.curve(), distance * strip.side)?,
                        )
                        .map_err(|e| invalid(&e.to_string()))?,
                    );
                }
            }
        }
        let mut output = Shell::new();
        let mut faces = HashMap::new();
        for (face, strip) in shell.iter().zip(&self.strips) {
            let wires = face
                .boundaries()
                .iter()
                .map(|wire| {
                    wire.iter()
                        .map(|edge| {
                            if edge.orientation() {
                                edges[&edge.id()].clone()
                            } else {
                                edges[&edge.id()].inverse()
                            }
                        })
                        .collect()
                })
                .collect();
            let mapped = Face::try_new(
                wires,
                strip
                    .carrier
                    .offset_surface(&face.oriented_surface(), distance * strip.side)?,
            )
            .map_err(|e| invalid(&e.to_string()))?;
            faces.insert(strip.face, mapped.id());
            output.push(mapped);
        }
        crate::validation::surface(&output)?;
        Ok(Offset {
            shell: output,
            edges,
            faces,
        })
    }
}

pub(crate) fn offset(shell: &Shell, distance: f64) -> Result<Option<Offset>> {
    let Some(sheet) = Sheet::recognize(shell)? else {
        return Ok(None);
    };
    sheet.certify(0., distance)?;
    sheet.offset(shell, distance).map(Some)
}

/// Two independently valid offsets do not prove a valid mid-plane thickness:
/// opposite-side collars can collide with one another. Certify their union.
pub(crate) fn certify_thickness(shell: &Shell, lower: f64, upper: f64) -> Result<()> {
    if let Some(sheet) = Sheet::recognize(shell)? {
        sheet.certify(lower, upper)?;
    }
    Ok(())
}

/// Certify complete normal collars of selected *closed* planar cap contours.
/// Unselected contours remain exact protected boundaries. This is also used
/// by connected rim treatments: their three-dimensional removal is contained
/// in these collars, while shared contour vertices authorize complete seams.
pub(crate) fn certify_cap_collars(
    cap: &Face,
    walls: &HashMap<EdgeId, Face>,
    selected: &std::collections::HashSet<EdgeId>,
    lower: f64,
    upper: f64,
) -> Result<bool> {
    let Some(Carrier::Plane { .. }) = analytic_offset::recognize(cap)? else {
        return Ok(false);
    };
    let axis = cap.oriented_surface().normal(0., 0.).normalize();
    let Some(origin) = cap.vertex_iter().next().map(|v| v.point()) else {
        return Ok(false);
    };
    let seed = if axis.x.abs() < 0.8 {
        Vector3::unit_x()
    } else {
        Vector3::unit_y()
    };
    let x = (seed - axis * seed.dot(axis)).normalize();
    let frame = Frame {
        origin,
        axis,
        x,
        y: axis.cross(x),
    };
    if !lower.is_finite() || !upper.is_finite() {
        return Err(invalid("Normal collar distances must be finite"));
    }
    let (lower, upper) = (lower.min(upper), lower.max(upper));
    if upper - lower <= EPS {
        return Err(invalid("Normal collar width is below native resolution"));
    }
    let mut strips = Vec::new();
    let mut protected = Vec::new();
    let mut count = 0usize;
    for wire in cap.boundaries() {
        let chosen = wire
            .iter()
            .filter(|edge| selected.contains(&edge.id()))
            .count();
        if chosen != 0 && chosen != wire.len() {
            return Ok(false);
        }
        for edge in &wire {
            count = count.saturating_add(1);
            if count > MAX_FACES {
                return Err(unsupported(
                    "Cap collar proof exceeds the profile work budget",
                ));
            }
            let Some(profile) = Span::native(&edge.oriented_curve(), &frame)? else {
                return Ok(false);
            };
            if chosen == 0 {
                protected.push(profile);
                continue;
            }
            let Some(wall) = walls.get(&edge.id()) else {
                return Ok(false);
            };
            let Some(carrier) = analytic_offset::recognize(wall)? else {
                return Ok(false);
            };
            let side = analytic_offset::side(wall, carrier)?;
            let normal = match carrier {
                Carrier::Plane { normal, .. } if normal.dot(axis).abs() <= REL => {
                    frame.vector(normal) * side
                }
                Carrier::Cone {
                    axis: other,
                    slope: 0.,
                    ..
                } if other.dot(axis).abs() > 1. - REL => Vector2::zero(),
                _ => return Ok(false),
            };
            match (carrier, profile.support) {
                (Carrier::Plane { .. }, Support::Line { .. }) => {}
                (Carrier::Cone { origin, radius, .. }, Support::Circle { center, radius: r })
                    if frame.point(origin).distance(center) <= EPS && (radius - r).abs() <= EPS => {
                }
                _ => return Ok(false),
            }
            strips.push(Strip {
                face: wall.id(),
                carrier,
                side,
                profile,
                ends: [
                    Join::Vertex(edge.front().id()),
                    Join::Vertex(edge.back().id()),
                ],
                normal,
            });
        }
    }
    if strips.is_empty() {
        return Ok(false);
    }
    let mut joins: HashMap<Join, (Point2, Vector2, usize)> = HashMap::new();
    for strip in &strips {
        for (join, point) in strip
            .ends
            .iter()
            .zip([strip.profile.start, strip.profile.end])
        {
            let normal = strip.normal_at(point);
            if let Some((previous, prior_normal, incidence)) = joins.get_mut(join) {
                if *incidence != 1
                    || previous.distance(point) > EPS
                    || (*prior_normal - normal).magnitude() > 1e-8
                {
                    return Ok(false);
                }
                *incidence += 1;
            } else {
                joins.insert(*join, (point, normal, 1));
            }
        }
    }
    if joins.values().any(|(_, _, count)| *count != 2) {
        return Ok(false);
    }
    let collars = strips
        .iter()
        .map(|strip| strip.collar(lower, upper))
        .collect::<Result<Vec<_>>>()?;
    for (i, collar) in collars.iter().enumerate() {
        for other in &collars[..i] {
            if !collars_disjoint(collar, other)? {
                return Err(invalid(
                    "Connected rim collars cross, overlap or touch away from a native tangent junction",
                ));
            }
        }
        for span in &protected {
            for boundary in &collar.boundaries {
                let points = intersections(boundary.support, span.support, &[])?;
                let collides = match points {
                    Some(points) => points
                        .into_iter()
                        .any(|p| boundary.contains(p) && span.contains(p)),
                    None => [
                        boundary.start,
                        boundary.end,
                        boundary.point(0.5),
                        span.start,
                        span.end,
                        span.point(0.5),
                    ]
                    .into_iter()
                    .any(|p| boundary.contains(p) && span.contains(p)),
                };
                if collides {
                    return Err(invalid(
                        "A connected rim collar reaches an unselected contour",
                    ));
                }
            }
            if collar.contains(span.point(0.5))? {
                return Err(invalid(
                    "A connected rim collar contains an unselected contour",
                ));
            }
        }
    }
    Ok(true)
}
