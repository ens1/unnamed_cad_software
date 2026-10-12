//! Curve-driven rigid B-rep copies evaluated on exact native carriers.
use crate::adapter::{MonstertruckAdapter, Shape, TopologyHistory};
use cad_core::{
    CadError, Document, NodeRef, Result, SketchCurve, TopologyRef,
    curve_pattern::{Definition, Orientation, Path, Resolved, Spacing},
    reference_curves::HelixGeometry,
};
use monstertruck::modeling::*;
use std::collections::{BTreeMap, HashSet};
const EPS: f64 = 1e-7;
const MAX_PIECES: usize = 2048;
const MAX_TOPOLOGY: usize = 2_000_000;
fn invalid(message: impl ToString) -> CadError {
    CadError::Invalid(format!("Curve Pattern: {}", message.to_string()))
}
fn unsupported(message: impl ToString) -> CadError {
    CadError::Unsupported(format!("Curve Pattern: {}", message.to_string()))
}
fn finite(point: Point3) -> bool {
    [point.x, point.y, point.z].iter().all(|v| v.is_finite())
}
fn unit(vector: Vector3) -> Result<Vector3> {
    let length = vector.magnitude();
    if !length.is_finite() || length <= 1e-12 {
        return Err(invalid(
            "The selected path has a degenerate or nonfinite tangent",
        ));
    }
    Ok(vector / length)
}
struct Piece {
    curve: Curve,
    source: TopologyRef,
}
struct NativePath {
    pieces: Vec<Piece>,
    closed: bool,
}
fn member_reference(profile: &NodeRef, member: &str, piece: usize) -> Result<TopologyRef> {
    Ok(TopologyRef {
        producer: profile.clone(),
        topology: "curve".into(),
        signature: None,
        role: format!(
            "construction:{}",
            serde_json::to_string(
                &serde_json::json!({"sources":[],"purpose":{"operation":"sketch_member","member":member,"piece":piece},"topology":"curve"})
            )?
        ),
    })
}
fn source_path(
    adapter: &MonstertruckAdapter,
    doc: &Document,
    definition: &Definition,
    inputs: &BTreeMap<NodeRef, Shape>,
) -> Result<NativePath> {
    match &definition.path {
        Path::ReferenceCurve { feature } => {
            let Some(Shape::ReferenceCurves { paths, evaluated }) = inputs.get(feature) else {
                return Err(invalid(
                    "The selected reference curve is missing or suppressed",
                ));
            };
            if paths.len() != 1 || paths[0].is_empty() || paths[0].len() != evaluated.pieces.len() {
                return Err(invalid(
                    "Reference Curve must produce one current ordered native path",
                ));
            }
            let pieces = paths[0]
                .iter()
                .zip(&evaluated.pieces)
                .map(|(edge, piece)| {
                    let source = adapter.edge_source(feature, edge.id()).ok_or_else(|| {
                        invalid("A reference-curve piece has no current construction ancestry")
                    })?;
                    if !cad_core::provenance::equivalent(&source, &piece.reference)? {
                        return Err(invalid(
                            "The reference-curve piece identity is stale or ambiguous",
                        ));
                    }
                    Ok(Piece {
                        curve: edge.oriented_curve(),
                        source,
                    })
                })
                .collect::<Result<Vec<_>>>()?;
            Ok(NativePath {
                pieces,
                closed: evaluated.closed,
            })
        }
        Path::Sketch {
            profile,
            member,
            start_vertex,
        } => {
            let (mut sketch, plane) = crate::extrusion::source(adapter, doc, profile, inputs)?;
            let aliases = cad_core::sketch_relations::topological_point_aliases(&sketch, 1e-6)?;
            let material = sketch
                .members
                .iter()
                .filter(|(_, member)| !member.construction)
                .map(|(id, _)| id.clone())
                .collect::<Vec<_>>();
            let selected = member
                .clone()
                .or_else(|| (material.len() == 1).then(|| material[0].clone()));
            let native=selected.as_ref().map(|member| {
                sketch.members.get(member).filter(|member|!member.construction).map(|member|member.curve.clone())
                    .ok_or_else(||invalid("The selected sketch path member is missing, suppressed or construction geometry"))
            }).transpose()?;
            if let Some(selected) = &selected {
                sketch.members.retain(|id, _| id == selected);
                sketch.profile_order.clear();
                sketch.segments.clear();
                sketch.circles.clear();
            }
            let built = crate::sketches::build_traced(doc, &profile.definition, &sketch, &plane)?;
            let (faces, paths) = match built.shape {
                Shape::Sketch { faces, paths } => (faces, paths),
                Shape::Profile(face) => (vec![face], vec![]),
                _ => {
                    return Err(invalid(
                        "The selected path does not produce exact sketch curves",
                    ));
                }
            };
            let mut wire = if selected.is_some() {
                let mut edges = faces
                    .iter()
                    .flat_map(|face| face.boundaries())
                    .chain(paths.iter().cloned())
                    .flat_map(Wire::into_iter)
                    .map(|edge| {
                        let (source, piece) = built.members.get(&edge.id()).ok_or_else(|| {
                            invalid("The selected path has no native member identity")
                        })?;
                        if Some(source) != selected.as_ref() {
                            return Err(invalid(
                                "A selected-member path contains an unrelated source boundary",
                            ));
                        }
                        Ok((*piece, edge.absolute_clone()))
                    })
                    .collect::<Result<Vec<_>>>()?;
                edges.sort_by_key(|(piece, _)| *piece);
                if edges.windows(2).any(|pair| pair[0].0 == pair[1].0) {
                    return Err(invalid(
                        "The selected member piece identities are ambiguous",
                    ));
                }
                edges.into_iter().map(|(_, edge)| edge).collect::<Wire>()
            } else {
                if paths.len() != 1 {
                    return Err(invalid(
                        "Select one path member, or retain one connected open sketch chain with a Path Start Vertex",
                    ));
                }
                paths[0].clone()
            };
            if wire.is_empty() || !wire.is_continuous() {
                return Err(invalid("The selected sketch path is empty or disconnected"));
            }
            let closed = wire.is_closed();
            let world = |p: [f64; 2]| {
                Point3::from(plane.origin)
                    + Vector3::from(plane.u) * p[0]
                    + Vector3::from(plane.v) * p[1]
            };
            let declared = match native.as_ref() {
                Some(SketchCurve::Line { start, .. } | SketchCurve::Arc { start, .. }) => {
                    Some(start)
                }
                Some(SketchCurve::Bezier { points } | SketchCurve::Nurbs { points, .. }) => {
                    points.first()
                }
                _ => None,
            };
            let start = start_vertex.as_ref().or(declared);
            if closed {
                if start_vertex.is_some() {
                    return Err(invalid(
                        "A closed single-member path retains its native starting phase; clear Path Start Vertex",
                    ));
                }
            } else if let Some(start) = start {
                let position = world(
                    *sketch
                        .points
                        .get(aliases.get(start).unwrap_or(start))
                        .ok_or_else(|| invalid("The saved Path Start Vertex is missing"))?,
                );
                if wire.front().unwrap().front().point().distance(position) > EPS {
                    if wire.back().unwrap().back().point().distance(position) > EPS {
                        return Err(invalid(
                            "Path Start Vertex must identify a current endpoint of this path",
                        ));
                    }
                    wire = wire.inverse();
                }
            } else {
                return Err(invalid(
                    "Choose a persistent Path Start Vertex for a multi-member open sketch path",
                ));
            }
            let pieces = wire
                .iter()
                .map(|edge| {
                    let (member, piece) = built.members.get(&edge.id()).ok_or_else(|| {
                        invalid("Every sketch path boundary requires a native member identity")
                    })?;
                    Ok(Piece {
                        curve: edge.oriented_curve(),
                        source: member_reference(profile, member, *piece)?,
                    })
                })
                .collect::<Result<Vec<_>>>()?;
            Ok(NativePath { pieces, closed })
        }
    }
}
fn spans(curve: &Curve) -> Result<Vec<BsplineCurve<Vector4>>> {
    crate::extrusion::curve_hull(curve)?;
    let mut curve = curve.lift_up();
    curve.clamp();
    if curve.degree() == 0 || curve.degree() > 32 || curve.control_points().len() > 2048 {
        return Err(unsupported(
            "Exact path proofs require degree 1–32 and at most 2048 controls per piece",
        ));
    }
    let (a, b) = curve.range_tuple();
    let mut knots = curve
        .knot_vector()
        .iter()
        .copied()
        .filter(|k| *k > a && *k < b)
        .collect::<Vec<_>>();
    knots.sort_by(f64::total_cmp);
    knots.dedup();
    let mut result = Vec::new();
    for knot in knots {
        let tail = curve.cut(knot);
        result.push(curve);
        curve = tail;
    }
    result.push(curve);
    if result
        .iter()
        .any(|span| span.control_points().len() != span.degree() + 1)
    {
        return Err(unsupported(
            "The native path cannot be decomposed into exact Bernstein spans",
        ));
    }
    Ok(result)
}
fn binomial(n: usize, k: usize) -> f64 {
    (1..=k.min(n - k)).fold(1., |value, i| value * (n + 1 - i) as f64 / i as f64)
}
fn rational_point(h: Vector4) -> Result<Point3> {
    if ![h.x, h.y, h.z, h.w].iter().all(|v| v.is_finite()) || h.w <= 0. {
        return Err(unsupported(
            "Native path weights must be finite and positive",
        ));
    }
    let point = Point3::new(h.x / h.w, h.y / h.w, h.z / h.w);
    if !finite(point) {
        return Err(invalid("Path controls overflow document coordinates"));
    }
    Ok(point)
}
fn derivative_numerator(span: &BsplineCurve<Vector4>) -> Result<Vec<Vector3>> {
    let controls = span.control_points();
    let degree = span.degree();
    let origin = rational_point(controls[0])?;
    let scale = controls.iter().map(|h| h.w).fold(0_f64, f64::max);
    let controls = controls
        .iter()
        .map(|h| {
            rational_point(*h)?;
            Ok((
                Vector3::new(
                    h.x - origin.x * h.w,
                    h.y - origin.y * h.w,
                    h.z - origin.z * h.w,
                ) / scale,
                h.w / scale,
            ))
        })
        .collect::<Result<Vec<_>>>()?;
    let mut coefficients = vec![Vector3::zero(); 2 * degree];
    for i in 0..degree {
        for j in 0..=degree {
            let numerator = (controls[i + 1].0 - controls[i].0) * controls[j].1
                - controls[j].0 * (controls[i + 1].1 - controls[i].1);
            coefficients[i + j] += numerator
                * (degree as f64 * binomial(degree - 1, i) * binomial(degree, j)
                    / binomial(2 * degree - 1, i + j));
        }
    }
    if coefficients
        .iter()
        .any(|v| ![v.x, v.y, v.z].iter().all(|x| x.is_finite()))
    {
        return Err(invalid("Exact derivative coefficients overflow"));
    }
    Ok(coefficients)
}
fn prove_progress(span: &BsplineCurve<Vector4>, direction: Vector3) -> Result<()> {
    let direction = unit(direction)?;
    let coefficients = derivative_numerator(span)?;
    let scale = coefficients
        .iter()
        .map(|v| v.magnitude())
        .fold(0_f64, f64::max);
    if !scale.is_finite()
        || scale <= 1e-14
        || coefficients
            .iter()
            .any(|v| v.dot(direction) <= scale * 1e-10)
    {
        return Err(unsupported(
            "A complete native derivative bound cannot prove regular forward progress; remove the cusp/retracing span or use a simpler path",
        ));
    }
    Ok(())
}
fn prove_frame(curve: &Curve, normal: Vector3) -> Result<()> {
    for span in spans(curve)? {
        let rational = NurbsCurve::new(span.clone());
        let (a, b) = rational.range_tuple();
        // This midpoint chooses a separating direction only. Every coefficient
        // of the complete derivative polynomial must prove the projection.
        let derivative = rational.der(a + (b - a) * 0.5);
        let direction = derivative - normal * derivative.dot(normal);
        prove_progress(&span, direction)?;
    }
    Ok(())
}
struct AnalyticCarrier {
    origin: Point3,
    axis: Vector3,
    radial: Vector3,
    change: Vector3,
    height: f64,
    axial: f64,
    start: f64,
    angle: f64,
    matrix: Matrix4,
    linear_norm: f64,
}
fn analytic_carrier(curve: &Curve) -> Result<AnalyticCarrier> {
    let Curve::ParameterCurve(parameter) = curve else {
        return Err(unsupported("Select a native analytic parameter curve"));
    };
    let Curve2D::Line(uv) = parameter.curve() else {
        return Err(unsupported(
            "Analytic curve stations require an exact linear parameter-space path",
        ));
    };
    let Surface::RevolutionSurface(processor) = parameter.surface().as_ref() else {
        return Err(unsupported(
            "Analytic curve stations require a native revolution carrier",
        ));
    };
    let source = processor.entity();
    let Curve::Line(generator) = source.entity_curve() else {
        return Err(unsupported(
            "Analytic curve stations require a straight native generatrix",
        ));
    };
    let matrix = *processor.transform();
    let columns = [matrix.x, matrix.y, matrix.z, matrix.w];
    if columns
        .iter()
        .any(|c| ![c.x, c.y, c.z, c.w].iter().all(|x| x.is_finite()))
        || matrix.x.w != 0.
        || matrix.y.w != 0.
        || matrix.z.w != 0.
        || matrix.w.w != 1.
    {
        return Err(unsupported(
            "Analytic curve stations require a finite affine carrier transform",
        ));
    }
    let linear_norm = columns[..3]
        .iter()
        .flat_map(|c| [c.x, c.y, c.z])
        .fold(0_f64, f64::hypot);
    if !linear_norm.is_finite() || linear_norm <= 0. {
        return Err(invalid("The analytic carrier transform is collapsed"));
    }
    let axis = source.axis();
    if !axis.magnitude2().is_finite()
        || (axis.magnitude2() - 1.).abs() > 1e-10
        || !finite(source.origin())
        || !finite(generator.0)
        || !finite(generator.1)
    {
        return Err(invalid(
            "The analytic carrier axis/generatrix is nonfinite or inconsistent",
        ));
    }
    let (a, b) = curve.range_tuple();
    let mut first = uv.subs(a);
    let mut last = uv.subs(b);
    if !processor.orientation() {
        first = Point2::new(first.y, first.x);
        last = Point2::new(last.y, last.x);
    }
    if ![first.x, first.y, last.x, last.y]
        .iter()
        .all(|x| x.is_finite())
        || first.y.abs().max(last.y.abs()) > std::f64::consts::TAU * 1e6
    {
        return Err(unsupported(
            "The analytic phase exceeds the stable one-million-turn evaluation range",
        ));
    }
    let vector = generator.subs(first.x) - source.origin();
    let change = (generator.1 - generator.0) * (last.x - first.x);
    let axial = vector.dot(axis);
    let height = change.dot(axis);
    let radial = vector - axis * axial;
    let change = change - axis * height;
    let radius = radial.magnitude().max((radial + change).magnitude());
    let bound = linear_norm
        * (source.origin().to_vec().magnitude() + radius + axial.abs().max((axial + height).abs()))
        + matrix.w.truncate().magnitude();
    if !bound.is_finite() || !height.is_finite() || !axial.is_finite() || !radius.is_finite() {
        return Err(invalid(
            "The complete analytic carrier coordinate bound overflows",
        ));
    }
    Ok(AnalyticCarrier {
        origin: source.origin(),
        axis,
        radial,
        change,
        height,
        axial,
        start: first.y,
        angle: last.y - first.y,
        matrix,
        linear_norm,
    })
}
impl AnalyticCarrier {
    fn helix(&self) -> Result<HelixGeometry> {
        let columns = [
            self.matrix.x.truncate(),
            self.matrix.y.truncate(),
            self.matrix.z.truncate(),
        ];
        let scale = columns[0].magnitude();
        let squared = scale * scale;
        if !squared.is_finite()
            || scale <= 1e-12
            || columns
                .iter()
                .any(|c| (c.magnitude2() - squared).abs() > squared * 1e-10)
            || (0..3)
                .any(|i| ((i + 1)..3).any(|j| columns[i].dot(columns[j]).abs() > squared * 1e-10))
        {
            return Err(unsupported(
                "Exact analytic arc length currently requires a rigid or uniform-scale carrier; sheared/nonuniformly scaled analytic paths use Equal Parameter",
            ));
        }
        let last = self.radial + self.change;
        let a = self.radial.magnitude();
        let b = last.magnitude();
        if a <= EPS
            || b <= EPS
            || self.radial.cross(last).magnitude() > a * b * 1e-10
            || self.radial.dot(last) <= 0.
        {
            return Err(unsupported(
                "Exact analytic arc length requires a positive collinear radial taper on one meridian",
            ));
        }
        let determinant = columns[0].dot(columns[1].cross(columns[2]));
        if !determinant.is_finite() || determinant.abs() <= 1e-36 {
            return Err(invalid("The analytic distance frame is singular"));
        }
        let parity = determinant.signum();
        Ok(HelixGeometry {
            origin: self
                .matrix
                .transform_point(self.origin + self.axis * self.axial)
                .into(),
            axis: unit(self.matrix.transform_vector(self.axis))?.into(),
            radial: unit(self.matrix.transform_vector(self.radial))?.into(),
            radius: a * scale,
            end_radius: b * scale,
            height: self.height * scale,
            angle: self.angle * parity,
            start_angle: self.start * parity,
        })
    }
    fn prove_frame(&self, curve: &Curve, normal: Vector3) -> Result<()> {
        let steps = (self.angle.abs() / (std::f64::consts::PI / 8.))
            .ceil()
            .max(1.) as usize;
        if steps > 16384 {
            return Err(unsupported(
                "Analytic frame proof exceeds 16384 angular intervals",
            ));
        }
        let speed_bound = self.linear_norm
            * (self.change.magnitude()
                + self.angle.abs()
                    * self
                        .radial
                        .magnitude()
                        .max((self.radial + self.change).magnitude())
                + self.height.abs());
        if !speed_bound.is_finite() || speed_bound <= 1e-12 {
            return Err(invalid("The analytic derivative is nonfinite or collapsed"));
        }
        let cross = self.axis.cross(self.radial);
        let cross_change = self.axis.cross(self.change);
        let cos0 = self
            .matrix
            .transform_vector(self.change + cross * self.angle);
        let cos1 = self.matrix.transform_vector(cross_change * self.angle);
        let sin0 = self
            .matrix
            .transform_vector(cross_change - self.radial * self.angle);
        let sin1 = self.matrix.transform_vector(-self.change * self.angle);
        let fixed = self.matrix.transform_vector(self.axis * self.height);
        let (native_a, native_b) = curve.range_tuple();
        for i in 0..steps {
            let a = i as f64 / steps as f64;
            let b = (i + 1) as f64 / steps as f64;
            let derivative = curve.der(native_a + (native_b - native_a) * (a + b) * 0.5);
            let direction = unit(derivative - normal * derivative.dot(normal))
                .map_err(|_| unsupported("Frame Up is parallel to an analytic path tangent"))?;
            let parameter = Interval::new(a, b);
            let phase = Interval::new(self.start + self.angle * a, self.start + self.angle * b);
            let cosine = Interval::constant(cos0.dot(direction))
                .add(parameter.scale(cos1.dot(direction)))
                .mul(phase.cos());
            let sine = Interval::constant(sin0.dot(direction))
                .add(parameter.scale(sin1.dot(direction)))
                .mul(phase.sin());
            let bound = cosine
                .add(sine)
                .add(Interval::constant(fixed.dot(direction)));
            if !bound.low.is_finite() || !bound.high.is_finite() || bound.low <= speed_bound * 1e-10
            {
                return Err(unsupported(
                    "The complete analytic derivative bound cannot prove a stable Frame Up projection; select a different up direction or Fixed orientation",
                ));
            }
        }
        Ok(())
    }
}
#[derive(Clone, Copy)]
struct Interval {
    low: f64,
    high: f64,
}
impl Interval {
    fn new(a: f64, b: f64) -> Self {
        Self {
            low: a.min(b),
            high: a.max(b),
        }
    }
    fn constant(a: f64) -> Self {
        Self::new(a, a)
    }
    fn add(self, other: Self) -> Self {
        Self::new(self.low + other.low, self.high + other.high)
    }
    fn scale(self, value: f64) -> Self {
        Self::new(self.low * value, self.high * value)
    }
    fn mul(self, other: Self) -> Self {
        let p = [
            self.low * other.low,
            self.low * other.high,
            self.high * other.low,
            self.high * other.high,
        ];
        Self {
            low: p.iter().copied().fold(f64::INFINITY, f64::min),
            high: p.iter().copied().fold(f64::NEG_INFINITY, f64::max),
        }
    }
    fn sin(self) -> Self {
        if self.high - self.low >= std::f64::consts::TAU {
            return Self::new(-1., 1.);
        }
        let mut bound = Self::new(self.low.sin(), self.high.sin());
        let first = ((self.low - std::f64::consts::FRAC_PI_2) / std::f64::consts::PI).ceil() as i64;
        let last =
            ((self.high - std::f64::consts::FRAC_PI_2) / std::f64::consts::PI).floor() as i64;
        for index in first..=last {
            let value = if index.rem_euclid(2) == 0 { 1. } else { -1. };
            bound.low = bound.low.min(value);
            bound.high = bound.high.max(value);
        }
        bound
    }
    fn cos(self) -> Self {
        self.add(Self::constant(std::f64::consts::FRAC_PI_2)).sin()
    }
}
fn validate_path(path: &NativePath) -> Result<()> {
    if path.pieces.is_empty() || path.pieces.len() > MAX_PIECES {
        return Err(unsupported(
            "A curve pattern path needs 1–2048 native pieces",
        ));
    }
    for piece in &path.pieces {
        let (a, b) = piece.curve.range_tuple();
        if !a.is_finite() || !b.is_finite() || b <= a {
            return Err(invalid(
                "The path has a nonfinite or reversed native interval",
            ));
        }
        if matches!(piece.curve, Curve::IntersectionCurve(_)) {
            return Err(unsupported(
                "Implicit intersection guides cannot define curve-pattern station authority",
            ));
        }
        if !finite(piece.curve.subs(a)) || !finite(piece.curve.subs(b)) {
            return Err(invalid("The exact path endpoint is nonfinite"));
        }
        if !matches!(piece.curve, Curve::ParameterCurve(_)) {
            spans(&piece.curve)?;
        } else {
            analytic_carrier(&piece.curve)?;
        }
    }
    for pair in path.pieces.windows(2) {
        let left = pair[0].curve.subs(pair[0].curve.range_tuple().1);
        let right = pair[1].curve.subs(pair[1].curve.range_tuple().0);
        if left.distance(right) > EPS {
            return Err(invalid("The current ordered path contains an endpoint gap"));
        }
    }
    if path.closed {
        let first = &path.pieces[0].curve;
        let last = &path.pieces.last().unwrap().curve;
        if first
            .subs(first.range_tuple().0)
            .distance(last.subs(last.range_tuple().1))
            > EPS
        {
            return Err(invalid("The closed path has a current seam gap"));
        }
    }
    Ok(())
}
#[derive(Clone)]
enum DistancePiece {
    Line {
        a: Point3,
        b: Point3,
        length: f64,
    },
    Circle {
        center: Point3,
        normal: Vector3,
        radial: Vector3,
        angle: f64,
        length: f64,
    },
    Helix {
        geometry: Box<HelixGeometry>,
        curve: Box<Curve>,
        length: f64,
    },
}
impl DistancePiece {
    fn length(&self) -> f64 {
        match self {
            Self::Line { length, .. }
            | Self::Circle { length, .. }
            | Self::Helix { length, .. } => *length,
        }
    }
    fn at(&self, fraction: f64) -> Result<(Point3, Vector3)> {
        let (point, tangent) = match self {
            Self::Line { a, b, .. } => (*a + (*b - *a) * fraction, *b - *a),
            Self::Circle {
                center,
                normal,
                radial,
                angle,
                ..
            } => {
                let angle = *angle * fraction;
                let radius = *radial * angle.cos() + normal.cross(*radial) * angle.sin();
                (*center + radius, normal.cross(radius) * self.circle_sign())
            }
            Self::Helix {
                geometry, curve, ..
            } => {
                let parameter = helix_arc_parameter(geometry, fraction)?;
                let (a, b) = curve.range_tuple();
                let parameter = if parameter == 1. {
                    b
                } else {
                    a + (b - a) * parameter
                };
                let point = curve.subs(parameter);
                if !finite(point) {
                    return Err(invalid(
                        "The exact analytic station exceeds finite coordinates",
                    ));
                }
                return Ok((point, unit(curve.der(parameter))?));
            }
        };
        if !finite(point) {
            return Err(invalid(
                "Exact distance station overflows document coordinates",
            ));
        }
        Ok((point, unit(tangent)?))
    }
    fn circle_sign(&self) -> f64 {
        match self {
            Self::Circle { angle, .. } => angle.signum(),
            _ => 1.,
        }
    }
}
fn distance_pieces(path: &NativePath) -> Result<Vec<DistancePiece>> {
    let mut pieces = Vec::new();
    for piece in &path.pieces {
        if matches!(piece.curve, Curve::ParameterCurve(_)) {
            let carrier = analytic_carrier(&piece.curve)?;
            if carrier.angle == 0.
                || carrier.radial.magnitude2() == 0. && carrier.change.magnitude2() == 0.
            {
                let (a, b) = piece.curve.range_tuple();
                let a = piece.curve.subs(a);
                let b = piece.curve.subs(b);
                let length = a.distance(b);
                if !length.is_finite() || length <= EPS {
                    return Err(invalid("The straight analytic interval is degenerate"));
                }
                pieces.push(DistancePiece::Line { a, b, length });
                continue;
            }
            let geometry = carrier.helix()?;
            let length = helix_distance(&geometry, 1.)?;
            if length <= EPS {
                return Err(invalid(
                    "The exact analytic distance interval is degenerate",
                ));
            }
            pieces.push(DistancePiece::Helix {
                geometry: Box::new(geometry),
                curve: Box::new(piece.curve.clone()),
                length,
            });
            continue;
        }
        let line = crate::axis_geometry::line_axis(&piece.curve).is_ok();
        let circle = if line {
            None
        } else {
            Some(crate::axis_geometry::circle_geometry(&piece.curve).map_err(|_|unsupported("Exact Arc Length currently requires proven straight/circular carriers or a native analytic helix/spiral; general spline/ellipse length is unavailable"))?)
        };
        for span in spans(&piece.curve)? {
            let controls = span.control_points();
            let a = rational_point(controls[0])?;
            let b = rational_point(*controls.last().unwrap())?;
            if line {
                let length = a.distance(b);
                if !length.is_finite() || length <= EPS {
                    return Err(invalid("A straight distance interval is degenerate"));
                }
                prove_progress(&span, b - a)?;
                pieces.push(DistancePiece::Line { a, b, length });
            } else if let Some((center, normal, radius)) = circle {
                let radial = a - center;
                let end = b - center;
                let middle = unit(radial)? + unit(end)?;
                if middle.magnitude2() <= 1e-12
                    || controls.iter().any(|h| {
                        (Vector3::new(
                            h.x - center.x * h.w,
                            h.y - center.y * h.w,
                            h.z - center.z * h.w,
                        ))
                        .dot(middle)
                            <= 1e-12 * h.w * radius
                    })
                {
                    return Err(unsupported(
                        "A circular Bernstein span must remain within one unambiguous open half-plane",
                    ));
                }
                let sign = normal
                    .dot(radial.cross(NurbsCurve::new(span.clone()).der(span.range_tuple().0)))
                    .signum();
                let degree = span.degree();
                let scale = controls.iter().map(|h| h.w).fold(0_f64, f64::max);
                let radial = controls
                    .iter()
                    .map(|h| {
                        Vector3::new(
                            h.x - center.x * h.w,
                            h.y - center.y * h.w,
                            h.z - center.z * h.w,
                        ) / scale
                    })
                    .collect::<Vec<_>>();
                let mut coefficients = vec![0.; 2 * degree];
                for (i, value) in radial.iter().enumerate() {
                    for j in 0..degree {
                        coefficients[i + j] += value
                            .cross((radial[j + 1] - radial[j]) * degree as f64)
                            .dot(normal)
                            * sign
                            * binomial(degree, i)
                            * binomial(degree - 1, j)
                            / binomial(2 * degree - 1, i + j);
                    }
                }
                let magnitude = coefficients.iter().copied().fold(0_f64, f64::max);
                if !sign.is_finite()
                    || sign == 0.
                    || !magnitude.is_finite()
                    || magnitude <= 1e-18
                    || coefficients
                        .iter()
                        .any(|v| !v.is_finite() || *v <= magnitude * 1e-10)
                {
                    return Err(unsupported(
                        "The exact circular derivative does not prove unique forward arc-length progress",
                    ));
                }
                let angle = normal
                    .dot((a - center).cross(b - center))
                    .atan2((a - center).dot(b - center));
                if angle.signum() != sign || angle.abs() <= 1e-12 {
                    return Err(unsupported(
                        "The circular span has an ambiguous or degenerate angular interval",
                    ));
                }
                let length = radius * angle.abs();
                if !length.is_finite() || length <= EPS {
                    return Err(invalid(
                        "The exact circular distance interval is degenerate",
                    ));
                }
                pieces.push(DistancePiece::Circle {
                    center,
                    normal,
                    radial: a - center,
                    angle,
                    length,
                });
            }
        }
    }
    if pieces.len() > MAX_PIECES {
        return Err(unsupported(
            "Exact distance decomposition exceeds 2048 path spans",
        ));
    }
    Ok(pieces)
}
fn helix_distance(helix: &HelixGeometry, u: f64) -> Result<f64> {
    let delta = helix.end_radius - helix.radius;
    let angle = helix.angle.abs();
    let k = delta * delta + helix.height * helix.height;
    let a = helix.radius;
    let dr = delta * u;
    let b = a + dr;
    let sa = (k + (angle * a).powi(2)).sqrt();
    let sb = (k + (angle * b).powi(2)).sqrt();
    let length = if delta == 0. {
        sa * u
    } else {
        // Closed-form integral of sqrt(k + angle² radius(u)²). The
        // difference uses log1p and a factored square-root difference so a
        // small radius taper does not lose its authority to cancellation.
        let sum = sa + sb;
        let polynomial = dr * (sb + a * angle * angle * (a + b) / sum);
        let ratio = dr * angle * (1. + angle * (a + b) / sum) / (angle * a + sa);
        0.5 * (polynomial + k / angle * ratio.ln_1p()) / delta
    };
    if !length.is_finite()
        || length < 0.
        || !angle.is_finite()
        || angle <= 0.
        || !k.is_finite()
        || a <= EPS
        || b <= EPS
    {
        return Err(unsupported(
            "The analytic helix/spiral distance law exceeds its finite positive-radius range",
        ));
    }
    Ok(length)
}
fn helix_arc_parameter(helix: &HelixGeometry, fraction: f64) -> Result<f64> {
    if fraction == 0. {
        return Ok(0.);
    }
    if fraction == 1. {
        return Ok(1.);
    }
    let length = helix_distance(helix, 1.)?;
    if helix.radius == helix.end_radius {
        return Ok(fraction);
    }
    let target = length * fraction;
    let mut low = 0.;
    let mut high = 1.;
    for _ in 0..96 {
        let middle = low + (high - low) * 0.5;
        if middle == low || middle == high {
            break;
        }
        if helix_distance(helix, middle)? < target {
            low = middle;
        } else {
            high = middle;
        }
    }
    let u = low + (high - low) * 0.5;
    let residual = (helix_distance(helix, u)? - target).abs();
    let speed = ((helix.end_radius - helix.radius).powi(2)
        + helix.height.powi(2)
        + (helix.angle * helix.radius.max(helix.end_radius)).powi(2))
    .sqrt();
    if residual > 1e-9_f64.max(length * 1e-12) || !speed.is_finite() || speed * (high - low) > EPS {
        return Err(unsupported(
            "The monotone analytic distance inverse cannot meet its exact-source residual/position bound",
        ));
    }
    Ok(u)
}
enum Measure {
    Parameter {
        ends: Vec<f64>,
        total: f64,
    },
    Distance {
        pieces: Vec<DistancePiece>,
        ends: Vec<f64>,
        total: f64,
    },
}
fn cumulative(lengths: impl Iterator<Item = f64>) -> Result<(Vec<f64>, f64)> {
    let mut total = 0.;
    let mut ends = Vec::new();
    for length in lengths {
        if !length.is_finite() || length <= 0. {
            return Err(invalid(
                "A path measure interval is nonfinite or nonpositive",
            ));
        }
        let next = total + length;
        if !next.is_finite() || next <= total {
            return Err(unsupported(
                "Concatenated path intervals exceed the finite distinguishable parameter/distance range",
            ));
        }
        total = next;
        ends.push(total);
    }
    if ends.is_empty() {
        return Err(invalid("The selected path has no measure intervals"));
    }
    Ok((ends, total))
}
impl Measure {
    fn new(path: &NativePath, spacing: Spacing) -> Result<Self> {
        match spacing {
            Spacing::EqualParameter => {
                let (ends, total) = cumulative(path.pieces.iter().map(|p| {
                    let (a, b) = p.curve.range_tuple();
                    b - a
                }))?;
                Ok(Self::Parameter { ends, total })
            }
            Spacing::ExactArcLength => {
                let pieces = distance_pieces(path)?;
                let (ends, total) = cumulative(pieces.iter().map(DistancePiece::length))?;
                Ok(Self::Distance {
                    pieces,
                    ends,
                    total,
                })
            }
        }
    }
    fn at(&self, path: &NativePath, fraction: f64, follow: bool) -> Result<(Point3, Vector3)> {
        let (ends, total) = match self {
            Self::Parameter { ends, total } | Self::Distance { ends, total, .. } => (ends, *total),
        };
        if !fraction.is_finite() || !(0. ..=1.).contains(&fraction) {
            return Err(invalid("A path station left its normalized range"));
        }
        let target = fraction * total;
        let index = ends
            .partition_point(|end| *end < target)
            .min(ends.len() - 1);
        let before = if index == 0 { 0. } else { ends[index - 1] };
        let local = ((target - before) / (ends[index] - before)).clamp(0., 1.);
        match self {
            Self::Distance { pieces, .. } => pieces[index].at(local),
            Self::Parameter { .. } => {
                let curve = &path.pieces[index].curve;
                let (a, b) = curve.range_tuple();
                let parameter = if local == 1. { b } else { a + (b - a) * local };
                let point = curve.subs(parameter);
                if !finite(point) {
                    return Err(invalid(
                        "The current native curve station overflows document coordinates",
                    ));
                }
                let tangent = if follow {
                    unit(curve.der(parameter))?
                } else {
                    Vector3::unit_x()
                };
                Ok((point, tangent))
            }
        }
    }
}
fn validate_frames(path: &NativePath, normal: Vector3) -> Result<()> {
    for piece in &path.pieces {
        if matches!(piece.curve, Curve::ParameterCurve(_)) {
            analytic_carrier(&piece.curve)?.prove_frame(&piece.curve, normal)?;
        } else {
            prove_frame(&piece.curve, normal)?;
        }
    }
    let joints = path
        .pieces
        .windows(2)
        .map(|pair| (&pair[0].curve, &pair[1].curve))
        .chain(
            path.closed
                .then(|| (&path.pieces.last().unwrap().curve, &path.pieces[0].curve)),
        );
    for (a, b) in joints {
        let left = unit(a.der(a.range_tuple().1))?;
        let right = unit(b.der(b.range_tuple().0))?;
        if left.dot(right) < 1. - 1e-9 {
            return Err(unsupported(
                "Follow Tangent requires matching tangent directions at every path junction and closed seam; use Fixed orientation for sharp corners",
            ));
        }
    }
    Ok(())
}
fn frame(tangent: Vector3, normal: Vector3) -> Result<Matrix3> {
    let x = unit(tangent)?;
    let z = unit(normal - x * normal.dot(x))
        .map_err(|_| unsupported("Frame Up is parallel to a selected station tangent"))?;
    let y = unit(z.cross(x))?;
    let z = x.cross(y);
    let matrix = Matrix3::from_cols(x, y, z);
    if !matrix.determinant().is_finite() || (matrix.determinant() - 1.).abs() > 1e-9 {
        return Err(invalid(
            "The current tangent frame is not a finite proper rotation",
        ));
    }
    Ok(matrix)
}
fn path_reference(path: &Path) -> Result<TopologyRef> {
    let purpose = match path {
        Path::Sketch {
            member,
            start_vertex,
            ..
        } => {
            serde_json::json!({"operation":"curve_pattern_path","kind":"sketch","member":member,"start_vertex":start_vertex})
        }
        Path::ReferenceCurve { .. } => {
            serde_json::json!({"operation":"curve_pattern_path","kind":"reference_curve","order":"native"})
        }
    };
    Ok(TopologyRef {
        producer: path.dependency().clone(),
        topology: "curve".into(),
        signature: None,
        role: format!(
            "construction:{}",
            serde_json::to_string(
                &serde_json::json!({"sources":[],"purpose":purpose,"topology":"curve"})
            )?
        ),
    })
}
pub(crate) fn operation(
    adapter: &MonstertruckAdapter,
    doc: &Document,
    owner: &NodeRef,
    definition: &Definition,
    inputs: &BTreeMap<NodeRef, Shape>,
    _tolerance: f64,
    generated: &mut TopologyHistory,
) -> Result<Shape> {
    let resolved: Resolved = definition.resolve(doc, &owner.definition)?;
    let Some(Shape::Bodies(stock)) = inputs.get(&definition.body) else {
        return Err(invalid("Select a current unsuppressed solid body output"));
    };
    if stock.is_empty() {
        return Err(invalid("The source body output is empty"));
    }
    let native_stock = crate::placement_pattern::Stock::new(adapter, &definition.body, stock)?;
    let active = resolved.count
        - definition
            .skipped
            .iter()
            .filter(|index| **index < resolved.count)
            .count();
    let path = source_path(adapter, doc, definition, inputs)?;
    validate_path(&path)?;
    // Read each retained constructor identity; geometry-dependent centroids,
    // area order and path parameter values never identify copied body topology.
    if path.pieces.iter().any(|piece| piece.source.role.is_empty()) {
        return Err(invalid(
            "A current path piece has no native construction identity",
        ));
    }
    let measure = Measure::new(&path, definition.spacing)?;
    let follow = definition.orientation == Orientation::FollowTangent;
    let normal = Vector3::from(resolved.normal);
    if follow {
        validate_frames(&path, normal)?;
    }
    let start = if definition.reverse {
        resolved.range[1]
    } else {
        resolved.range[0]
    };
    let (first, tangent) = measure.at(&path, start, follow)?;
    let sign = if definition.reverse { -1. } else { 1. };
    let initial_frame = if follow {
        frame(tangent * sign, normal)?
    } else {
        Matrix3::identity()
    };
    let anchor = if definition.align_seed {
        Point3::from(adapter.resolve_surface_point(
            doc,
            &owner.definition,
            &definition.seed,
            inputs,
        )?)
    } else {
        first
    };
    if !finite(anchor) {
        return Err(invalid("The current seed anchor is nonfinite"));
    }
    let reused = definition.keep_source
        && !definition.skipped.contains(&0)
        && (!definition.align_seed || anchor == first);
    let positions = active + usize::from(definition.keep_source) - usize::from(reused);
    if stock
        .len()
        .checked_mul(positions)
        .is_none_or(|n| n > cad_core::curve_pattern::MAX_INSTANCES)
        || native_stock
            .topology_size()
            .checked_mul(positions)
            .is_none_or(|n| n > MAX_TOPOLOGY)
    {
        return Err(unsupported(
            "Curve Pattern exceeds 10000 materialized solids or two million copied topology entities; reduce instance count or source complexity",
        ));
    }
    let closure = path_reference(&definition.path)?;
    let mut solids = Vec::new();
    if definition.keep_source {
        native_stock.inherit(generated);
        solids.extend(stock.iter().cloned());
    }
    let closed_full = path.closed && resolved.range == [0., 1.];
    let denominator = if closed_full {
        resolved.count
    } else {
        resolved.count.saturating_sub(1).max(1)
    } as f64;
    let skipped = definition.skipped.iter().copied().collect::<HashSet<_>>();
    for index in 0..resolved.count {
        if skipped.contains(&index) {
            continue;
        }
        let fraction = index as f64 / denominator;
        let station = if definition.reverse {
            resolved.range[1] - (resolved.range[1] - resolved.range[0]) * fraction
        } else {
            resolved.range[0] + (resolved.range[1] - resolved.range[0]) * fraction
        };
        let (position, tangent) = measure.at(&path, station, follow)?;
        let rotation = if index == 0 || !follow {
            Matrix3::identity()
        } else {
            frame(tangent * sign, normal)? * initial_frame.transpose()
        };
        let translation = position.to_vec() - rotation * anchor.to_vec();
        let matrix = Matrix4::from_cols(
            rotation.x.extend(0.),
            rotation.y.extend(0.),
            rotation.z.extend(0.),
            translation.extend(1.),
        );
        if [matrix.x, matrix.y, matrix.z, matrix.w]
            .iter()
            .any(|c| ![c.x, c.y, c.z, c.w].iter().all(|x| x.is_finite()))
        {
            return Err(invalid(
                "The requested station transform overflows finite document coordinates",
            ));
        }
        // The unaligned first station is the authored source exactly. Keeping
        // it occupies that station once instead of introducing coincident solids.
        if definition.keep_source && index == 0 && (!definition.align_seed || anchor == first) {
            continue;
        }
        let purpose = serde_json::json!({"operation":"curve_pattern","slot":index});
        solids.extend(native_stock.copy(
            stock,
            matrix,
            owner,
            &purpose,
            std::slice::from_ref(&closure),
            generated,
        )?);
    }
    if solids.is_empty() {
        return Err(invalid("All current pattern stations are skipped"));
    }
    Ok(Shape::Bodies(solids))
}
