//! Exact planar arrangements for partially overlapping contact caps.
//! Intersections determine split parameters; all output edges retain the
//! original native line or rational circular carrier.
use super::*;
#[path = "checked.rs"]
mod checked;
pub(crate) use checked::checked_intersections;

#[derive(Clone)]
enum Kind {
    Line,
    Arc {
        center: Point2,
        radius: f64,
        sweep: f64,
    },
}
#[derive(Clone)]
pub(crate) struct Segment {
    a: Point2,
    b: Point2,
    kind: Kind,
}
impl Segment {
    pub(crate) fn at(&self, t: f64) -> Point2 {
        match self.kind {
            Kind::Line => self.a + (self.b - self.a) * t,
            Kind::Arc { center, sweep, .. } => {
                let v = self.a - center;
                let (s, c) = (sweep * t).sin_cos();
                center + Vector2::new(c * v.x - s * v.y, s * v.x + c * v.y)
            }
        }
    }
    fn fraction(&self, p: Point2) -> Option<f64> {
        match self.kind {
            Kind::Line => {
                let v = self.b - self.a;
                let t = (p - self.a).dot(v) / v.magnitude2();
                (t >= -EPS / v.magnitude()
                    && t <= 1. + EPS / v.magnitude()
                    && (self.a + v * t).distance(p) <= EPS)
                    .then_some(t.clamp(0., 1.))
            }
            Kind::Arc {
                center,
                radius,
                sweep,
            } => {
                if ((p - center).magnitude() - radius).abs() > EPS {
                    return None;
                }
                if p.distance(self.a) <= EPS {
                    return Some(0.);
                }
                if p.distance(self.b) <= EPS {
                    return Some(1.);
                }
                let a = self.a - center;
                let b = p - center;
                let angle = cross(a, b).atan2(a.dot(b));
                let directed = if sweep > 0. {
                    angle.rem_euclid(std::f64::consts::TAU)
                } else {
                    -(-angle).rem_euclid(std::f64::consts::TAU)
                };
                let t = directed / sweep;
                (t > 0. && t < 1.).then_some(t)
            }
        }
    }
    fn area(&self) -> (f64, f64) {
        let terms = match self.kind {
            Kind::Line => [self.a.x * self.b.y, -self.a.y * self.b.x, 0.],
            Kind::Arc {
                center,
                radius,
                sweep,
            } => [
                center.x * (self.b.y - self.a.y),
                -center.y * (self.b.x - self.a.x),
                radius * radius * sweep,
            ],
        };
        (
            terms.iter().sum::<f64>() / 2.,
            terms.iter().map(|x| x.abs()).sum::<f64>() / 2.,
        )
    }
    fn crossings(&self, p: Point2) -> usize {
        match self.kind {
            Kind::Line => usize::from(
                (self.a.y > p.y) != (self.b.y > p.y)
                    && self.a.x + (p.y - self.a.y) * (self.b.x - self.a.x) / (self.b.y - self.a.y)
                        > p.x,
            ),
            Kind::Arc {
                center,
                radius,
                sweep,
            } => {
                let dy = p.y - center.y;
                let discriminant = radius * radius - dy * dy;
                // A tangent ray does not cross the contour.
                if discriminant <= 0. {
                    return 0;
                }
                let dx = discriminant.sqrt();
                [-dx, dx]
                    .into_iter()
                    .filter(|x| {
                        let q = Point2::new(center.x + x, p.y);
                        if q.x <= p.x {
                            return false;
                        }
                        // Ray parity needs strict interval membership. The
                        // contact splitter's geometric endpoint snapping would
                        // admit a hit just outside an arc and double-count a
                        // neighboring line at a mixed line/arc vertex.
                        let start = self.a - center;
                        let hit = q - center;
                        let angle = cross(start, hit).atan2(start.dot(hit));
                        let directed = if sweep > 0. {
                            angle.rem_euclid(std::f64::consts::TAU)
                        } else {
                            -(-angle).rem_euclid(std::f64::consts::TAU)
                        };
                        let t = directed / sweep;
                        let derivative = x * sweep;
                        if derivative == 0. {
                            return false;
                        }
                        // The same half-open rule as the linear ray predicate.
                        (t > 0. && t < 1.)
                            || (t == 0. && derivative > 0.)
                            || (t == 1. && derivative < 0.)
                    })
                    .count()
            }
        }
    }
}
fn cross(a: Vector2, b: Vector2) -> f64 {
    a.x * b.y - a.y * b.x
}

pub(crate) struct Frame {
    pub(crate) origin: Point3,
    pub(crate) axes: [Vector3; 2],
    pub(crate) normal: Vector3,
}
impl Frame {
    pub(crate) fn project(&self, p: Point3) -> Point2 {
        let d = p - self.origin;
        Point2::new(d.dot(self.axes[0]), d.dot(self.axes[1]))
    }
    pub(crate) fn lift(&self, p: Point2) -> Point3 {
        self.origin + self.axes[0] * p.x + self.axes[1] * p.y
    }
    pub(crate) fn segment(&self, edge: &Edge) -> Result<Option<Segment>> {
        let curve = edge.oriented_curve();
        let a = self.project(edge.front().point());
        let b = self.project(edge.back().point());
        let kind = if super::straight(edge)? {
            Kind::Line
        } else {
            let ((center, normal, radius), sweep) = match crate::bounded_carriers::circle(&curve) {
                Ok(circle) => circle,
                Err(CadError::Unsupported(_)) => return Ok(None),
                Err(error) => return Err(error),
            };
            if normal.dot(self.normal).abs() < 1. - 1e-10 {
                return Err(invalid("Contact arc is not in the contact plane"));
            }
            Kind::Arc {
                center: self.project(center),
                radius,
                sweep: sweep * normal.dot(self.normal).signum(),
            }
        };
        Ok(Some(Segment { a, b, kind }))
    }
}

pub(crate) fn intersections(a: &Segment, b: &Segment) -> Vec<Point2> {
    let mut result = Vec::new();
    // Endpoints also split coincident lines and circular carriers into a
    // common partition, without assuming equal edge counts in both wires.
    for p in [a.a, a.b, b.a, b.b] {
        if a.fraction(p).is_some() && b.fraction(p).is_some() {
            result.push(p);
        }
    }
    match (&a.kind, &b.kind) {
        (Kind::Line, Kind::Line) => {
            let u = a.b - a.a;
            let v = b.b - b.a;
            let denominator = cross(u, v);
            if denominator.abs() > 64. * f64::EPSILON * u.magnitude() * v.magnitude() {
                result.push(a.a + u * (cross(b.a - a.a, v) / denominator));
            }
        }
        (Kind::Line, Kind::Arc { center, radius, .. }) => {
            line_circle(a, *center, *radius, &mut result);
        }
        (Kind::Arc { center, radius, .. }, Kind::Line) => {
            line_circle(b, *center, *radius, &mut result);
        }
        (
            Kind::Arc {
                center: c0,
                radius: r0,
                ..
            },
            Kind::Arc {
                center: c1,
                radius: r1,
                ..
            },
        ) => {
            let delta = c1 - c0;
            let d = delta.magnitude();
            if d > EPS && d <= r0 + r1 + EPS && d >= (r0 - r1).abs() - EPS {
                let along = ((r0 - r1) * (r0 + r1) + d * d) / (2. * d);
                let height2 = (r0 - along.abs()) * (r0 + along.abs());
                if height2 >= -EPS * r0.max(*r1) {
                    let u = delta / d;
                    let base = *c0 + u * along;
                    let side = Vector2::new(-u.y, u.x) * height2.max(0.).sqrt();
                    result.extend([base + side, base - side]);
                }
            }
        }
    }
    result.retain(|p| a.fraction(*p).is_some() && b.fraction(*p).is_some());
    let mut unique: Vec<Point2> = Vec::new();
    for p in result {
        if !unique.iter().any(|other| other.distance(p) <= EPS) {
            unique.push(p);
        }
    }
    unique
}
fn line_circle(line: &Segment, center: Point2, radius: f64, result: &mut Vec<Point2>) {
    let direction = line.b - line.a;
    let length = direction.magnitude();
    let u = direction / length;
    let offset = center - line.a;
    let along = offset.dot(u);
    let perpendicular = cross(offset, u);
    let discriminant = (radius - perpendicular.abs()) * (radius + perpendicular.abs());
    if discriminant >= -EPS * radius {
        let delta = discriminant.max(0.).sqrt();
        result.extend([line.a + u * (along - delta), line.a + u * (along + delta)]);
    }
}

#[derive(PartialEq)]
pub(crate) enum Location {
    Inside,
    Outside,
    Boundary,
}
pub(crate) fn locate(p: Point2, region: &[Segment]) -> Location {
    if region.iter().any(|segment| segment.fraction(p).is_some()) {
        Location::Boundary
    } else if region
        .iter()
        .map(|segment| segment.crossings(p))
        .sum::<usize>()
        % 2
        == 1
    {
        Location::Inside
    } else {
        Location::Outside
    }
}

struct Source {
    edge: Edge,
    segment: Segment,
    cuts: Vec<(f64, usize)>,
    parts: Vec<usize>,
    controls: usize,
}
struct Part {
    edge: Edge,
    sources: Vec<EdgeId>,
    interval: usize,
}
struct Vertices {
    values: Vec<Vertex>,
    aliases: HashMap<VertexId, usize>,
    work: usize,
}
impl Vertices {
    fn insert(&mut self, p: Point3, original: Option<&Vertex>) -> Result<usize> {
        admit(&mut self.work, self.values.len(), 1)?;
        let mut candidates = self
            .values
            .iter()
            .enumerate()
            .filter(|(_, vertex)| vertex.point().distance(p) <= EPS)
            .map(|(index, _)| index);
        let first = candidates.next();
        if candidates.next().is_some() {
            return Err(unsupported(
                "Contact vertex matches more than one distinct boundary vertex",
            ));
        }
        let index = first.unwrap_or_else(|| {
            self.values
                .push(original.cloned().unwrap_or_else(|| Vertex::new(p)));
            self.values.len() - 1
        });
        if let Some(original) = original {
            self.aliases.insert(original.id(), index);
        }
        Ok(index)
    }
}

fn native_parameter(source: &Source, p: Point3) -> Result<f64> {
    let curve = source.edge.curve();
    let (a, b) = curve.range_tuple();
    if source.edge.absolute_front().point().distance(p) <= EPS {
        return Ok(a);
    }
    if source.edge.absolute_back().point().distance(p) <= EPS {
        return Ok(b);
    }
    let t = curve.search_parameter(p, None, 100).ok_or_else(|| {
        unsupported("Contact intersection could not be resolved on its native curve")
    })?;
    if !t.is_finite() || t <= a || t >= b || curve.subs(t).distance(p) > EPS {
        return Err(invalid(
            "Contact intersection parameter failed its native residual check",
        ));
    }
    Ok(t)
}

pub(crate) fn wire_from_parts(
    selected: Vec<Edge>,
    frame: &Frame,
    normal_sign: f64,
) -> Result<Vec<Vec<Wire>>> {
    let mut outgoing: HashMap<VertexId, usize> = HashMap::new();
    let mut incoming = HashSet::new();
    for (index, edge) in selected.iter().enumerate() {
        if outgoing.insert(edge.front().id(), index).is_some() || !incoming.insert(edge.back().id())
        {
            return Err(unsupported(
                "Contact would create touching or branching cap boundaries",
            ));
        }
    }
    if outgoing.keys().any(|vertex| !incoming.contains(vertex)) {
        return Err(invalid("Contact cap arrangement contains an open boundary"));
    }
    let mut used = HashSet::new();
    let mut loops = Vec::new();
    for start in 0..selected.len() {
        if used.contains(&start) {
            continue;
        }
        let mut wire = Wire::new();
        let mut index = start;
        loop {
            if !used.insert(index) {
                if index == start {
                    break;
                }
                return Err(invalid("Contact cap traversal joined a different loop"));
            }
            let edge = &selected[index];
            wire.push_back(edge.clone());
            index = *outgoing
                .get(&edge.back().id())
                .ok_or_else(|| invalid("Contact cap traversal lost an endpoint"))?;
        }
        let segments = wire
            .iter()
            .map(|edge| {
                frame
                    .segment(edge)?
                    .ok_or_else(|| invalid("A split contact boundary lost its analytic carrier"))
            })
            .collect::<Result<Vec<_>>>()?;
        let (mut area, mut correction, mut magnitude) = (0., 0., 0.);
        for segment in &segments {
            let (term, scale) = segment.area();
            let adjusted = term - correction;
            let next = area + adjusted;
            correction = (next - area) - adjusted;
            area = next;
            magnitude += scale;
        }
        let area = area * normal_sign;
        if !area.is_finite() || area.abs() <= (EPS * EPS).max(64. * f64::EPSILON * magnitude) {
            return Err(unsupported(
                "Contact produces a collapsed or numerically unresolved cap region",
            ));
        }
        loops.push((wire, segments, area));
    }
    let mut groups: Vec<(Vec<Wire>, Vec<Segment>, f64)> = loops
        .iter()
        .filter(|(_, _, area)| *area > 0.)
        .map(|(wire, segments, area)| (vec![wire.clone()], segments.clone(), *area))
        .collect();
    for (wire, segments, area) in loops {
        if area > 0. {
            continue;
        }
        let point = segments[0].at(0.5);
        let index = groups
            .iter()
            .enumerate()
            .filter(|(_, (_, outer, _))| locate(point, outer) == Location::Inside)
            .min_by(|(_, a), (_, b)| a.2.total_cmp(&b.2))
            .map(|(index, _)| index)
            .ok_or_else(|| invalid("Contact inner wire has no enclosing cap region"))?;
        groups[index].0.push(wire);
    }
    Ok(groups.into_iter().map(|(wires, _, _)| wires).collect())
}

pub(super) fn contact(
    stock: &Solid,
    tool: &Solid,
    support: &PlaneFace,
    cap: &Face,
    work: &mut usize,
) -> Result<Option<ContactOutput>> {
    let frame = Frame {
        origin: support.origin,
        axes: super::frame(support.normal),
        normal: support.normal,
    };
    let mut sources: Vec<Source> = Vec::new();
    let mut source_map = HashMap::new();
    let mut contours = [Vec::new(), Vec::new()];
    let mut coedges = [Vec::new(), Vec::new()];
    let mut vertices = Vertices {
        values: Vec::new(),
        aliases: HashMap::new(),
        work: 0,
    };
    for (side, face) in [&support.face, cap].into_iter().enumerate() {
        for edge in face.edge_iter() {
            let Some(segment) = frame.segment(&edge)? else {
                return Ok(None);
            };
            contours[side].push(segment);
            let index = if let Some(index) = source_map.get(&edge.id()) {
                *index
            } else {
                let absolute = if edge.orientation() {
                    edge.clone()
                } else {
                    edge.inverse()
                };
                let Some(segment) = frame.segment(&absolute)? else {
                    return Ok(None);
                };
                let a = vertices.insert(absolute.front().point(), Some(absolute.front()))?;
                let b = vertices.insert(absolute.back().point(), Some(absolute.back()))?;
                let (lo, hi) = absolute.curve().range_tuple();
                if a == b || !lo.is_finite() || !hi.is_finite() || lo >= hi {
                    return Err(unsupported(
                        "Contact has a collapsed or unbounded native edge",
                    ));
                }
                let index = sources.len();
                let controls = crate::extrusion::curve_hull(&absolute.curve())?.len();
                sources.push(Source {
                    edge: absolute,
                    segment,
                    cuts: vec![(lo, a), (hi, b)],
                    parts: vec![],
                    controls,
                });
                source_map.insert(edge.id(), index);
                index
            };
            coedges[side].push((index, edge.orientation()));
        }
    }
    *work = work
        .checked_add(coedges[0].len().saturating_mul(coedges[1].len()))
        .ok_or_else(|| unsupported("Contact curve intersection work overflowed"))?;
    if *work > MAX_PAIR_WORK {
        return Err(unsupported(
            "Contact curves exceed their bounded intersection work",
        ));
    }
    let mut native_work = 0;
    for &(i, _) in &coedges[0] {
        for &(j, _) in &coedges[1] {
            for p in intersections(&sources[i].segment, &sources[j].segment) {
                admit(
                    &mut native_work,
                    sources[i].controls + sources[j].controls,
                    100,
                )?;
                let p = frame.lift(p);
                let ti = native_parameter(&sources[i], p)?;
                let tj = native_parameter(&sources[j], p)?;
                let vertex = vertices.insert(p, None)?;
                sources[i].cuts.push((ti, vertex));
                sources[j].cuts.push((tj, vertex));
            }
        }
    }
    let mut parts: Vec<Part> = Vec::new();
    let mut result_controls: usize = 0;
    let mut by_endpoints: HashMap<(VertexId, VertexId), Vec<usize>> = HashMap::new();
    for source in &mut sources {
        source.cuts.sort_by(|a, b| a.0.total_cmp(&b.0));
        source.cuts.dedup_by(|a, b| a.1 == b.1);
        for (interval, pair) in source.cuts.windows(2).enumerate() {
            result_controls = result_controls.saturating_add(source.controls);
            if result_controls > MAX_CONTROLS {
                return Err(unsupported(
                    "Contact split carriers exceed their native control budget",
                ));
            }
            let (lo, a) = pair[0];
            let (hi, b) = pair[1];
            if a == b || hi <= lo {
                return Err(invalid("Contact split parameters are inconsistent"));
            }
            let front = &vertices.values[a];
            let back = &vertices.values[b];
            let mut curve = source.edge.curve();
            let (start, end) = curve.range_tuple();
            if hi < end {
                curve.cut(hi);
            }
            if lo > start {
                curve = curve.cut(lo);
            }
            let edge = if lo == start
                && hi == end
                && front.id() == source.edge.front().id()
                && back.id() == source.edge.back().id()
            {
                source.edge.clone()
            } else {
                Edge::try_new(front, back, curve).map_err(invalid)?
            };
            let candidates = by_endpoints
                .get(&(front.id(), back.id()))
                .into_iter()
                .flatten()
                .chain(
                    by_endpoints
                        .get(&(back.id(), front.id()))
                        .into_iter()
                        .flatten(),
                );
            let mut matched = None;
            for &index in candidates {
                let other = &parts[index].edge;
                let aligned = if other.front().id() == edge.front().id() {
                    other.clone()
                } else {
                    other.inverse()
                };
                if super::matching_edge(&aligned, &edge)? {
                    matched = Some(index);
                    break;
                }
            }
            let index = if let Some(index) = matched {
                if !parts[index].sources.contains(&source.edge.id()) {
                    parts[index].sources.push(source.edge.id());
                }
                index
            } else {
                let index = parts.len();
                by_endpoints
                    .entry((front.id(), back.id()))
                    .or_default()
                    .push(index);
                parts.push(Part {
                    edge,
                    sources: vec![source.edge.id()],
                    interval,
                });
                index
            };
            source.parts.push(index);
            if parts.len() > MAX_EDGES {
                return Err(unsupported(
                    "Contact arrangement exceeds its bounded split-edge budget",
                ));
            }
        }
    }
    let oriented_parts = |source: &Source, orientation: bool| -> Vec<(usize, Edge)> {
        let mut result = Vec::new();
        for (interval, &index) in source.parts.iter().enumerate() {
            let a = vertices.values[source.cuts[interval].1].id();
            let edge = &parts[index].edge;
            let edge = if edge.front().id() == a {
                edge.clone()
            } else {
                edge.inverse()
            };
            result.push((index, edge));
        }
        if !orientation {
            result.reverse();
            for (_, edge) in &mut result {
                edge.invert();
            }
        }
        result
    };
    let mut side_parts = [Vec::new(), Vec::new()];
    for side in 0..2 {
        for &(index, orientation) in &coedges[side] {
            side_parts[side].extend(oriented_parts(&sources[index], orientation));
        }
    }
    let mut selected = [Vec::new(), Vec::new()];
    let mut overlap = false;
    for side in 0..2 {
        admit(
            &mut native_work,
            side_parts[side].len(),
            side_parts[1 - side].len() + contours[1 - side].len(),
        )?;
        for (index, edge) in &side_parts[side] {
            let other = &side_parts[1 - side];
            if let Some((_, paired)) = other.iter().find(|(other, _)| other == index) {
                if edge.orientation() == paired.orientation() {
                    return Ok(None); // Edge-only contact has no manifold union.
                }
                overlap = true;
                continue;
            }
            let segment = frame
                .segment(edge)?
                .ok_or_else(|| invalid("Split contact lost its carrier"))?;
            match locate(segment.at(0.5), &contours[1 - side]) {
                Location::Outside => selected[side].push(edge.clone()),
                Location::Inside => {
                    overlap = true;
                    selected[1 - side].push(edge.clone());
                }
                Location::Boundary => {
                    return Err(unsupported(
                        "A contact interval is coincident without a resolved shared native partition",
                    ));
                }
            }
        }
    }
    if !overlap {
        return Ok(None);
    }
    let mut trimmed = Vec::new();
    let mut caps = Vec::new();
    for (side, selected) in selected.into_iter().enumerate() {
        let original = if side == 0 { &support.face } else { cap };
        for (component, wires) in
            wire_from_parts(selected, &frame, if side == 0 { 1. } else { -1. })?
                .into_iter()
                .enumerate()
        {
            let face = Face::try_new(wires, original.oriented_surface()).map_err(invalid)?;
            trimmed.push(ContactFace {
                face: face.id(),
                sources: vec![
                    original.id(),
                    if side == 0 {
                        cap.id()
                    } else {
                        support.face.id()
                    },
                ],
                component,
            });
            caps.push(face);
        }
    }
    let mut edge_map: HashMap<EdgeId, Vec<Edge>> = HashMap::new();
    let mut edge_aliases = Vec::new();
    let mut split_edges = Vec::new();
    for source in &sources {
        edge_map.insert(
            source.edge.id(),
            oriented_parts(source, true)
                .into_iter()
                .map(|(_, edge)| edge)
                .collect(),
        );
    }
    for part in &parts {
        if part.sources.len() == 1 {
            let source = &sources[source_map[&part.sources[0]]];
            if source.parts.len() == 1 {
                if part.edge.id() != source.edge.id() {
                    edge_aliases.push((part.edge.id(), source.edge.id()));
                }
                continue;
            }
        }
        split_edges.push(ContactEdge {
            edge: part.edge.id(),
            sources: part.sources.clone(),
            interval: part.interval,
        });
    }
    for edge in stock.edge_iter().chain(tool.edge_iter()) {
        if edge_map.contains_key(&edge.id()) {
            continue;
        }
        let front = vertices
            .aliases
            .get(&edge.absolute_front().id())
            .map(|index| &vertices.values[*index])
            .unwrap_or_else(|| edge.absolute_front());
        let back = vertices
            .aliases
            .get(&edge.absolute_back().id())
            .map(|index| &vertices.values[*index])
            .unwrap_or_else(|| edge.absolute_back());
        let mapped =
            if front.id() == edge.absolute_front().id() && back.id() == edge.absolute_back().id() {
                if edge.orientation() {
                    edge.clone()
                } else {
                    edge.inverse()
                }
            } else {
                let mapped = Edge::try_new(front, back, edge.curve()).map_err(invalid)?;
                edge_aliases.push((mapped.id(), edge.id()));
                mapped
            };
        edge_map.insert(edge.id(), vec![mapped]);
    }
    let mut face_aliases = Vec::new();
    let mut remap = |face: &Face| -> Result<Face> {
        let mut changed = false;
        let boundaries = face
            .boundaries()
            .iter()
            .map(|wire| {
                let mut output = Wire::new();
                for edge in wire {
                    let mapped = &edge_map[&edge.id()];
                    changed |= mapped.len() != 1 || mapped[0].id() != edge.id();
                    if edge.orientation() {
                        output.extend(mapped.iter().cloned());
                    } else {
                        output.extend(mapped.iter().rev().map(Edge::inverse));
                    }
                }
                output
            })
            .collect();
        if !changed {
            return Ok(face.clone());
        }
        let mapped = Face::try_new(boundaries, face.oriented_surface()).map_err(invalid)?;
        face_aliases.push((mapped.id(), face.id()));
        Ok(mapped)
    };
    let mut shells = Vec::new();
    for shell in stock.boundaries() {
        let contact_shell = shell.iter().any(|face| face.id() == support.face.id());
        let mut output = shell
            .iter()
            .filter(|face| face.id() != support.face.id())
            .map(&mut remap)
            .collect::<Result<Shell>>()?;
        if contact_shell {
            output.extend(
                tool.face_iter()
                    .filter(|face| face.id() != cap.id())
                    .map(&mut remap)
                    .collect::<Result<Vec<_>>>()?,
            );
            output.extend(caps.iter().cloned());
        }
        shells.push(output);
    }
    if shells.iter().map(|shell| shell.len()).sum::<usize>() > MAX_RESULT_FACES {
        return Err(unsupported(
            "Contact arrangement exceeds its bounded face budget",
        ));
    }
    let solid = Solid::try_new(shells).map_err(invalid)?;
    crate::validation::solid(&solid)?;
    Ok(Some(ContactOutput {
        solid,
        trimmed,
        face_aliases,
        edge_aliases,
        split_edges,
    }))
}
