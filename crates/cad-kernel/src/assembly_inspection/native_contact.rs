//! Native separation and contact without constructing a zero-volume Boolean.
//!
//! A complete body must lie on each side of the same plane. Native positive
//! rational control signs are evaluated as exact dyadic integers, so a small
//! penetration can never become contact through a modeling tolerance. A weak
//! separator proves disjoint interiors only; touching additionally requires an
//! actual shared native boundary point. Display triangles are never inputs.
use crate::native_bounds::Enclosure;
use cad_core::{CadError, Result};
use monstertruck::modeling::*;
use num_bigint::{BigInt, Sign};
use std::cmp::Ordering;

mod analytic;
mod radial;

const MAX_WORK: usize = 4_000_000;
const MAX_CONTROLS: usize = 262_144;
const MAX_BEZIER_DEPTH: usize = 24;

fn unsupported(message: &str) -> CadError {
    CadError::Unsupported(format!("Native contact: {message}"))
}

#[derive(Default)]
pub(super) struct Budget {
    work: usize,
}
impl Budget {
    fn spend(&mut self, count: usize) -> Result<()> {
        self.work = self.work.saturating_add(count);
        if self.work > MAX_WORK {
            return Err(unsupported(
                "the shared exact predicate budget is exhausted",
            ));
        }
        Ok(())
    }
}

// Every finite binary64 is an integer times a power of two. Keeping this form
// avoids both division of homogeneous controls and cancellation at a support
// plane. The finite input exponent range bounds the integer sizes as well as
// the predicate count above.
#[derive(Clone)]
struct Exact {
    integer: BigInt,
    exponent: i32,
}
impl Exact {
    fn new(value: f64) -> Result<Self> {
        if !value.is_finite() {
            return Err(unsupported("a predicate input is nonfinite"));
        }
        let bits = value.to_bits();
        let power = ((bits >> 52) & 0x7ff) as i32;
        let mantissa = bits & ((1u64 << 52) - 1);
        let (mantissa, exponent) = if power == 0 {
            (mantissa, -1074)
        } else {
            (mantissa | (1u64 << 52), power - 1023 - 52)
        };
        let mut integer = BigInt::from(mantissa);
        if bits >> 63 != 0 {
            integer = -integer;
        }
        Ok(Self { integer, exponent })
    }
    fn zero() -> Self {
        Self {
            integer: BigInt::from(0),
            exponent: 0,
        }
    }
    fn add(&self, other: &Self) -> Self {
        if self.sign() == Ordering::Equal {
            return other.clone();
        }
        if other.sign() == Ordering::Equal {
            return self.clone();
        }
        let exponent = self.exponent.min(other.exponent);
        Self {
            integer: (&self.integer << (self.exponent - exponent) as usize)
                + (&other.integer << (other.exponent - exponent) as usize),
            exponent,
        }
    }
    fn neg(&self) -> Self {
        Self {
            integer: -&self.integer,
            exponent: self.exponent,
        }
    }
    fn sub(&self, other: &Self) -> Self {
        self.add(&other.neg())
    }
    fn mul(&self, other: &Self) -> Self {
        Self {
            integer: &self.integer * &other.integer,
            exponent: self.exponent + other.exponent,
        }
    }
    fn half(&self) -> Self {
        Self {
            integer: self.integer.clone(),
            exponent: self.exponent - 1,
        }
    }
    fn sign(&self) -> Ordering {
        match self.integer.sign() {
            Sign::Minus => Ordering::Less,
            Sign::NoSign => Ordering::Equal,
            Sign::Plus => Ordering::Greater,
        }
    }
}

#[derive(Clone)]
struct Homogeneous([Exact; 4]);
impl Homogeneous {
    fn point(point: Point3) -> Result<Self> {
        Self::control(point.to_homogeneous())
    }
    fn control(point: Vector4) -> Result<Self> {
        if point.w <= 0. {
            return Err(unsupported("native rational weights must be positive"));
        }
        Ok(Self([
            Exact::new(point.x)?,
            Exact::new(point.y)?,
            Exact::new(point.z)?,
            Exact::new(point.w)?,
        ]))
    }
    fn midpoint(&self, other: &Self) -> Self {
        Self(std::array::from_fn(|i| self.0[i].add(&other.0[i]).half()))
    }
    fn compare(&self, other: &Self, axis: usize) -> Ordering {
        self.0[axis]
            .mul(&other.0[3])
            .sub(&other.0[axis].mul(&self.0[3]))
            .sign()
    }
}

struct Support {
    equation: [Exact; 4],
    projected: [usize; 2],
}
impl Support {
    fn new(plane: Plane) -> Result<Option<Self>> {
        let [a, b, c] = plane.defining_points().map(Homogeneous::point);
        let (a, b, c) = (a?, b?, c?);
        let u: [Exact; 3] = std::array::from_fn(|i| b.0[i].sub(&a.0[i]));
        let v: [Exact; 3] = std::array::from_fn(|i| c.0[i].sub(&a.0[i]));
        let normal: [Exact; 3] = std::array::from_fn(|i| {
            u[(i + 1) % 3]
                .mul(&v[(i + 2) % 3])
                .sub(&u[(i + 2) % 3].mul(&v[(i + 1) % 3]))
        });
        let Some(drop) = normal.iter().position(|n| n.sign() != Ordering::Equal) else {
            return Ok(None);
        };
        let mut offset = Exact::zero();
        for (i, coefficient) in normal.iter().enumerate() {
            offset = offset.sub(&coefficient.mul(&a.0[i]));
        }
        Ok(Some(Self {
            equation: [
                normal[0].clone(),
                normal[1].clone(),
                normal[2].clone(),
                offset,
            ],
            projected: [(drop + 1) % 3, (drop + 2) % 3],
        }))
    }
    fn sign(&self, point: &Homogeneous, budget: &mut Budget) -> Result<Ordering> {
        budget.spend(1)?;
        let mut value = Exact::zero();
        for i in 0..4 {
            value = value.add(&self.equation[i].mul(&point.0[i]));
        }
        Ok(value.sign())
    }
    fn contains_plane(&self, plane: Plane, budget: &mut Budget) -> Result<bool> {
        for point in plane.defining_points() {
            if self.sign(&Homogeneous::point(point)?, budget)? != Ordering::Equal {
                return Ok(false);
            }
        }
        Ok(true)
    }
}

#[derive(Clone, Copy, Default)]
struct Sides {
    negative: bool,
    positive: bool,
    zero: bool,
}
impl Sides {
    fn add(&mut self, sign: Ordering) {
        match sign {
            Ordering::Less => self.negative = true,
            Ordering::Equal => self.zero = true,
            Ordering::Greater => self.positive = true,
        }
    }
    fn merge(&mut self, other: Self) {
        self.negative |= other.negative;
        self.positive |= other.positive;
        self.zero |= other.zero;
    }
    fn straddles(self) -> bool {
        self.negative && self.positive
    }
}

struct NativeFace {
    face: Face,
    bound: Option<Enclosure>,
    planar_region: Option<PlanarRegion>,
    parameter_regions: Option<Vec<Vec<NativeCurve>>>,
}
pub(super) struct Body {
    faces: Vec<NativeFace>,
    supports: Vec<Support>,
}
impl Body {
    pub(super) fn new(solid: &Solid, budget: &mut Budget) -> Result<Self> {
        let mut faces = Vec::new();
        let mut supports = Vec::new();
        for face in solid.face_iter() {
            let surface = face.surface();
            let parameter_regions = parameter_regions(face, budget)?;
            let planar_region = if let Surface::Plane(plane) = surface {
                if let Some(support) = Support::new(plane)? {
                    let region = planar_region(face, &support, budget)?;
                    supports.push(support);
                    region
                } else {
                    None
                }
            } else {
                None
            };
            let bound = match surface {
                // Plane and unbounded line-revolution extents come from the
                // authoritative face-local UV loops below, not independently
                // rounded 3D edge controls.
                Surface::Plane(_) | Surface::RevolutionSurface(_) => None,
                _ => crate::native_bounds::face(face).ok(),
            };
            faces.push(NativeFace {
                face: face.clone(),
                bound,
                planar_region,
                parameter_regions,
            });
        }
        Ok(Self { faces, supports })
    }
    pub(super) fn bounds(&self) -> Option<[[f64; 3]; 2]> {
        let mut bounds = [[f64::INFINITY; 3], [f64::NEG_INFINITY; 3]];
        for face in &self.faces {
            let bound = face.bound.as_ref()?;
            for (index, direction) in [Vector3::unit_x(), Vector3::unit_y(), Vector3::unit_z()]
                .into_iter()
                .enumerate()
            {
                let (low, high) = bound.projection(Point3::origin(), direction).ok()?;
                bounds[0][index] = bounds[0][index].min(low);
                bounds[1][index] = bounds[1][index].max(high);
            }
        }
        (!self.faces.is_empty()).then_some(bounds)
    }
    fn sides(&self, support: &Support, budget: &mut Budget) -> Result<Option<Sides>> {
        let mut result = Sides::default();
        for face in &self.faces {
            let Some(sides) = face_sides(face, support, budget)? else {
                return Ok(None);
            };
            result.merge(sides);
            if result.straddles() {
                return Ok(Some(result));
            }
        }
        Ok(Some(result))
    }
}
fn face_sides(face: &NativeFace, support: &Support, budget: &mut Budget) -> Result<Option<Sides>> {
    let mut result = Sides::default();
    if let Some(bound) = &face.bound {
        for point in bound.points() {
            result.add(support.sign(&Homogeneous::point(*point)?, budget)?);
            if result.straddles() {
                break;
            }
        }
        if !result.straddles() {
            return Ok(Some(result));
        }
    }
    if let Some(sides) = analytic::sides(face, support, budget)? {
        return Ok(Some(sides));
    }
    let points = if matches!(face.face.surface(), Surface::Plane(_)) {
        let Some(region) = &face.planar_region else {
            return Ok(None);
        };
        if !region.closed {
            return Ok(None);
        }
        region
            .curves
            .iter()
            .flat_map(|curve| curve.controls.iter().cloned())
            .collect()
    } else {
        let Some(points) = face_controls(&face.face, budget)? else {
            return Ok(None);
        };
        points
    };
    result = Sides::default();
    for point in points {
        result.add(support.sign(&point, budget)?);
        if result.straddles() {
            break;
        }
    }
    Ok(Some(result))
}

fn admit_count(count: usize, budget: &mut Budget) -> Result<()> {
    if count == 0 || count > MAX_CONTROLS {
        return Err(unsupported("a native carrier exceeds its control budget"));
    }
    budget.spend(count)
}

fn face_controls(face: &Face, budget: &mut Budget) -> Result<Option<Vec<Homogeneous>>> {
    match face.surface() {
        Surface::Plane(_) => {
            let mut points = Vec::new();
            for edge in face.edge_iter() {
                let Some(curve) = native_curve(&edge.oriented_curve(), budget)? else {
                    return Ok(None);
                };
                if points.len().saturating_add(curve.controls.len()) > MAX_CONTROLS {
                    return Err(unsupported("a planar boundary exceeds its control budget"));
                }
                points.extend(curve.controls);
            }
            Ok((!points.is_empty()).then_some(points))
        }
        Surface::BsplineSurface(surface) => {
            admit_count(surface.control_points().iter().map(Vec::len).sum(), budget)?;
            surface
                .control_points()
                .iter()
                .flatten()
                .copied()
                .map(Homogeneous::point)
                .collect::<Result<Vec<_>>>()
                .map(Some)
        }
        Surface::NurbsSurface(surface) => {
            admit_count(surface.control_points().iter().map(Vec::len).sum(), budget)?;
            surface
                .control_points()
                .iter()
                .flatten()
                .copied()
                .map(Homogeneous::control)
                .collect::<Result<Vec<_>>>()
                .map(Some)
        }
        _ => Ok(None),
    }
}

#[derive(Clone)]
struct NativeCurve {
    controls: Vec<Homogeneous>,
    // A clamped carrier has exact first/last control endpoints. A single
    // Bernstein span also admits exact half subdivision for winding proofs.
    clamped: bool,
    bezier: bool,
    line: bool,
    knots: Option<Vec<f64>>,
}
fn native_curve(curve: &Curve, budget: &mut Budget) -> Result<Option<NativeCurve>> {
    let (controls, clamped, bezier, line) = match curve {
        Curve::Line(Line(a, b)) => (
            vec![Homogeneous::point(*a)?, Homogeneous::point(*b)?],
            true,
            true,
            true,
        ),
        Curve::BsplineCurve(curve) => {
            admit_count(curve.control_points().len(), budget)?;
            (
                curve
                    .control_points()
                    .iter()
                    .copied()
                    .map(Homogeneous::point)
                    .collect::<Result<_>>()?,
                curve.is_clamped(),
                curve.is_clamped() && curve.control_points().len() == curve.degree() + 1,
                false,
            )
        }
        Curve::NurbsCurve(curve) => {
            admit_count(curve.control_points().len(), budget)?;
            (
                curve
                    .control_points()
                    .iter()
                    .copied()
                    .map(Homogeneous::control)
                    .collect::<Result<_>>()?,
                curve.is_clamped(),
                curve.is_clamped() && curve.control_points().len() == curve.degree() + 1,
                false,
            )
        }
        Curve::ParameterCurve(curve) => {
            let Surface::Plane(plane) = curve.surface().as_ref() else {
                return Ok(None);
            };
            let [origin, one, another] = plane.defining_points().map(Homogeneous::point);
            let (origin, one, another) = (origin?, one?, another?);
            let lift = |point: Vector3| -> Result<Homogeneous> {
                if point.z <= 0. {
                    return Err(unsupported(
                        "a parameter curve needs positive native weights",
                    ));
                }
                let weight = Exact::new(point.z)?;
                let u = Exact::new(point.x)?;
                let v = Exact::new(point.y)?;
                let mut output = origin.clone();
                for i in 0..3 {
                    output.0[i] = origin.0[i]
                        .mul(&weight)
                        .add(&one.0[i].sub(&origin.0[i]).mul(&u))
                        .add(&another.0[i].sub(&origin.0[i]).mul(&v));
                }
                output.0[3] = weight;
                Ok(output)
            };
            match curve.curve() {
                Curve2D::Line(Line(a, b)) => (
                    vec![
                        lift(Vector3::new(a.x, a.y, 1.))?,
                        lift(Vector3::new(b.x, b.y, 1.))?,
                    ],
                    true,
                    true,
                    true,
                ),
                Curve2D::BsplineCurve(curve) => {
                    admit_count(curve.control_points().len(), budget)?;
                    (
                        curve
                            .control_points()
                            .iter()
                            .map(|p| lift(Vector3::new(p.x, p.y, 1.)))
                            .collect::<Result<_>>()?,
                        curve.is_clamped(),
                        curve.is_clamped() && curve.control_points().len() == curve.degree() + 1,
                        false,
                    )
                }
                Curve2D::NurbsCurve(curve) => {
                    admit_count(curve.control_points().len(), budget)?;
                    (
                        curve
                            .control_points()
                            .iter()
                            .copied()
                            .map(lift)
                            .collect::<Result<_>>()?,
                        curve.is_clamped(),
                        curve.is_clamped() && curve.control_points().len() == curve.degree() + 1,
                        false,
                    )
                }
                _ => return Ok(None),
            }
        }
        _ => return Ok(None),
    };
    let knots = match curve {
        Curve::BsplineCurve(curve) => Some(curve.knot_vector().to_vec()),
        Curve::NurbsCurve(curve) => Some(curve.knot_vector().to_vec()),
        Curve::ParameterCurve(curve) => match curve.curve() {
            Curve2D::BsplineCurve(curve) => Some(curve.knot_vector().to_vec()),
            Curve2D::NurbsCurve(curve) => Some(curve.knot_vector().to_vec()),
            _ => None,
        },
        _ => None,
    };
    admit_count(controls.len(), budget)?;
    Ok(Some(NativeCurve {
        controls,
        clamped,
        bezier,
        line,
        knots,
    }))
}

fn orientation(a: &Homogeneous, b: &Homogeneous, p: &Homogeneous, axes: [usize; 2]) -> Ordering {
    let [x, y] = axes;
    let ax = a.0[x].mul(&p.0[3]).sub(&p.0[x].mul(&a.0[3]));
    let ay = a.0[y].mul(&p.0[3]).sub(&p.0[y].mul(&a.0[3]));
    let bx = b.0[x].mul(&p.0[3]).sub(&p.0[x].mul(&b.0[3]));
    let by = b.0[y].mul(&p.0[3]).sub(&p.0[y].mul(&b.0[3]));
    ax.mul(&by).sub(&ay.mul(&bx)).sign()
}
fn between(p: &Homogeneous, a: &Homogeneous, b: &Homogeneous, axes: [usize; 2]) -> bool {
    axes.into_iter().all(|axis| {
        let first = p.compare(a, axis);
        let second = p.compare(b, axis);
        first == Ordering::Equal || second == Ordering::Equal || first != second
    })
}
fn segments_meet(
    a: &NativeCurve,
    b: &NativeCurve,
    axes: [usize; 2],
    budget: &mut Budget,
) -> Result<bool> {
    if !a.line || !b.line {
        return Ok(false);
    }
    budget.spend(4)?;
    let [p, q] = [&a.controls[0], &a.controls[1]];
    let [r, s] = [&b.controls[0], &b.controls[1]];
    let pr = orientation(p, q, r, axes);
    let ps = orientation(p, q, s, axes);
    let rp = orientation(r, s, p, axes);
    let rq = orientation(r, s, q, axes);
    Ok((pr == Ordering::Equal && between(r, p, q, axes))
        || (ps == Ordering::Equal && between(s, p, q, axes))
        || (rp == Ordering::Equal && between(p, r, s, axes))
        || (rq == Ordering::Equal && between(q, r, s, axes))
        || (pr != Ordering::Equal
            && ps != Ordering::Equal
            && pr != ps
            && rp != Ordering::Equal
            && rq != Ordering::Equal
            && rp != rq))
}

enum Location {
    Inside,
    Outside,
    Boundary,
    Unresolved,
}
fn locate(
    point: &Homogeneous,
    curves: &[NativeCurve],
    axes: [usize; 2],
    budget: &mut Budget,
) -> Result<Location> {
    let mut crossings = 0usize;
    for curve in curves {
        if !curve.clamped {
            return Ok(Location::Unresolved);
        }
        let spans = if curve.bezier {
            vec![curve.controls.clone()]
        } else {
            let Some(spans) = radial::native_spans(curve, budget)? else {
                return Ok(Location::Unresolved);
            };
            spans
        };
        let mut stack = spans
            .into_iter()
            .map(|controls| (controls, 0usize))
            .collect::<Vec<_>>();
        while let Some((controls, depth)) = stack.pop() {
            budget.spend(controls.len().saturating_mul(2))?;
            let a = &controls[0];
            let b = controls.last().unwrap();
            let outside = axes.into_iter().any(|axis| {
                controls
                    .iter()
                    .all(|p| p.compare(point, axis) == Ordering::Less)
                    || controls
                        .iter()
                        .all(|p| p.compare(point, axis) == Ordering::Greater)
            });
            if !curve.line && !outside {
                if depth == MAX_BEZIER_DEPTH || controls.len() > 65 {
                    return Ok(Location::Unresolved);
                }
                budget.spend(controls.len().saturating_mul(controls.len()))?;
                let mut row = controls;
                let mut left = vec![row[0].clone()];
                let mut right = vec![row.last().unwrap().clone()];
                while row.len() > 1 {
                    row = row.windows(2).map(|p| p[0].midpoint(&p[1])).collect();
                    left.push(row[0].clone());
                    right.push(row.last().unwrap().clone());
                }
                right.reverse();
                stack.push((right, depth + 1));
                stack.push((left, depth + 1));
                continue;
            }
            // The native positive-control hull excludes the point. Deforming
            // this entire curve to its chord cannot change winding around it.
            let side = orientation(a, b, point, axes);
            if side == Ordering::Equal && between(point, a, b, axes) {
                return Ok(if curve.line {
                    Location::Boundary
                } else {
                    Location::Unresolved
                });
            }
            let ay = a.compare(point, axes[1]);
            let by = b.compare(point, axes[1]);
            if (ay == Ordering::Greater) != (by == Ordering::Greater)
                && side
                    == if b.compare(a, axes[1]) == Ordering::Greater {
                        Ordering::Greater
                    } else {
                        Ordering::Less
                    }
            {
                crossings += 1;
            }
        }
    }
    Ok(if crossings % 2 == 1 {
        Location::Inside
    } else {
        Location::Outside
    })
}

#[derive(Clone)]
struct PlanarRegion {
    curves: Vec<NativeCurve>,
    closed: bool,
}
fn closed_loop(curves: &[NativeCurve], budget: &mut Budget) -> Result<bool> {
    if curves.is_empty() || curves.iter().any(|curve| !curve.clamped) {
        return Ok(false);
    }
    for (a, b) in curves
        .iter()
        .zip(curves.iter().cycle().skip(1))
        .take(curves.len())
    {
        budget.spend(3)?;
        if (0..3)
            .any(|axis| a.controls.last().unwrap().compare(&b.controls[0], axis) != Ordering::Equal)
        {
            return Ok(false);
        }
    }
    Ok(true)
}
fn generator_controls<'a>(
    face: &'a NativeFace,
    generator: usize,
    budget: &mut Budget,
) -> Result<Option<Vec<&'a Homogeneous>>> {
    let Some(regions) = &face.parameter_regions else {
        return Ok(None);
    };
    let mut open = Vec::new();
    for (index, region) in regions.iter().enumerate() {
        if !closed_loop(region, budget)? {
            open.push(index);
        }
    }
    if !open.is_empty() {
        if open.len() != 2 {
            return Ok(None);
        }
        // A native full-period ring is open in unwrapped UV. Two opposite
        // monotone angular paths at distinct fixed generator parameters bound
        // a finite periodic band. We only use its generator hull and enclose
        // ALL real angles, so the binary64 angular period introduces no error
        // in these support bounds. No virtual seam becomes a contact witness.
        let angle = 1 - generator;
        let tau = Exact::new(std::f64::consts::TAU)?;
        let mut ends = Vec::new();
        for index in open {
            let region = &regions[index];
            if region.is_empty() || region.iter().any(|curve| !curve.clamped) {
                return Ok(None);
            }
            let first = &region[0].controls[0];
            let last = region.last().unwrap().controls.last().unwrap();
            let direction = last.compare(first, angle);
            let mut previous = first;
            for curve in region {
                let degree_one = curve.line
                    || curve
                        .knots
                        .as_ref()
                        .is_some_and(|knots| knots.len() == curve.controls.len() + 2);
                if !degree_one
                    || (0..2).any(|i| previous.compare(&curve.controls[0], i) != Ordering::Equal)
                {
                    return Ok(None);
                }
                for control in &curve.controls {
                    budget.spend(8)?;
                    let step = control.compare(previous, angle);
                    if control.compare(first, generator) != Ordering::Equal
                        || (step != Ordering::Equal && step != direction)
                    {
                        return Ok(None);
                    }
                    previous = control;
                }
            }
            let delta = last.0[angle]
                .mul(&first.0[3])
                .sub(&first.0[angle].mul(&last.0[3]));
            let period = tau.mul(&first.0[3]).mul(&last.0[3]);
            if delta.sub(&period).sign() != Ordering::Equal
                && delta.add(&period).sign() != Ordering::Equal
            {
                return Ok(None);
            }
            ends.push((first, last));
        }
        if ends[0].0.compare(ends[1].1, angle) != Ordering::Equal
            || ends[0].1.compare(ends[1].0, angle) != Ordering::Equal
            || ends[0].0.compare(ends[1].0, generator) == Ordering::Equal
        {
            return Ok(None);
        }
    }
    Ok(Some(
        regions
            .iter()
            .flatten()
            .flat_map(|curve| &curve.controls)
            .collect(),
    ))
}
fn parameter_regions(face: &Face, budget: &mut Budget) -> Result<Option<Vec<Vec<NativeCurve>>>> {
    let surface = face.surface();
    let mut result = Vec::new();
    for wire in face.boundaries() {
        let mut curves = Vec::new();
        for edge in wire {
            budget.spend(1)?;
            let Some(trim) = edge.oriented_curve().exact_parameter_boundary_2d(&surface) else {
                return Ok(None);
            };
            let Some(curve) = uv_curve(trim.curve(), budget)? else {
                return Ok(None);
            };
            curves.push(curve);
        }
        if curves.is_empty() {
            return Ok(None);
        }
        result.push(curves);
    }
    Ok((!result.is_empty()).then_some(result))
}
fn uv_curve(curve: &Curve2D, budget: &mut Budget) -> Result<Option<NativeCurve>> {
    let plane = Plane::new(
        Point3::origin(),
        Point3::new(1., 0., 0.),
        Point3::new(0., 1., 0.),
    );
    native_curve(
        &Curve::ParameterCurve(ParameterCurve::new(
            curve.clone(),
            Box::new(Surface::Plane(plane)),
        )),
        budget,
    )
}
fn planar_region(
    face: &Face,
    support: &Support,
    budget: &mut Budget,
) -> Result<Option<PlanarRegion>> {
    let mut curves = Vec::new();
    let mut closed = true;
    let surface = face.surface();
    for wire in face.boundaries() {
        let mut contour = Vec::new();
        for edge in wire {
            let source = edge.oriented_curve();
            // Use the same face-local domain as native admission, meshing and
            // STEP exchange. Only a missing UV carrier falls back to an exactly
            // coplanar 3D curve; no modeling residual decides a halfspace sign.
            let curve = if let Some(trim) = source.exact_parameter_boundary_2d(&surface) {
                native_curve(&Curve::ParameterCurve(trim), budget)?
            } else {
                let native = native_curve(&source, budget)?;
                let mut on_plane = native.is_some();
                if let Some(curve) = &native {
                    for point in &curve.controls {
                        if support.sign(point, budget)? != Ordering::Equal {
                            on_plane = false;
                            break;
                        }
                    }
                }
                if on_plane { native } else { None }
            };
            let Some(curve) = curve else { return Ok(None) };
            contour.push(curve);
        }
        closed &= closed_loop(&contour, budget)?;
        curves.extend(contour);
    }
    Ok((!curves.is_empty()).then_some(PlanarRegion { curves, closed }))
}
fn planar_regions(
    body: &Body,
    support: &Support,
    budget: &mut Budget,
) -> Result<Vec<PlanarRegion>> {
    let mut regions = Vec::new();
    for face in &body.faces {
        let Surface::Plane(plane) = face.face.surface() else {
            continue;
        };
        if support.contains_plane(plane, budget)?
            && let Some(region) = &face.planar_region
        {
            regions.push(region.clone());
        }
    }
    Ok(regions)
}
fn touching(a: &Body, b: &Body, support: &Support, budget: &mut Budget) -> Result<bool> {
    let a = planar_regions(a, support, budget)?;
    let b = planar_regions(b, support, budget)?;
    for first in &a {
        for second in &b {
            for (source, target) in [(first, second), (second, first)] {
                if !target.closed {
                    continue;
                }
                for curve in &source.curves {
                    if !curve.clamped {
                        continue;
                    }
                    for point in [&curve.controls[0], curve.controls.last().unwrap()] {
                        if matches!(
                            locate(point, &target.curves, support.projected, budget)?,
                            Location::Inside | Location::Boundary
                        ) {
                            return Ok(true);
                        }
                    }
                }
            }
            for left in &first.curves {
                for right in &second.curves {
                    if segments_meet(left, right, support.projected, budget)? {
                        return Ok(true);
                    }
                }
            }
        }
    }
    Ok(false)
}

#[derive(Clone, Copy)]
pub(super) enum Classification {
    Separated,
    Touching,
    DisjointInteriors,
    ContactWithinTolerance { band_mm: f64 },
    Unresolved,
}
pub(super) fn classify(
    first: &Body,
    second: &Body,
    budget: &mut Budget,
    linear_mm: f64,
) -> Result<Classification> {
    let mut disjoint_interiors = false;
    for support in first.supports.iter().chain(&second.supports) {
        budget.spend(1)?;
        let Some(a) = first.sides(support, budget)? else {
            continue;
        };
        if a.straddles() || !a.negative && !a.positive {
            continue;
        }
        let Some(b) = second.sides(support, budget)? else {
            continue;
        };
        if b.straddles()
            || !b.negative && !b.positive
            || a.negative == b.negative
            || a.positive == b.positive
        {
            continue;
        }
        if !a.zero || !b.zero {
            return Ok(Classification::Separated);
        }
        disjoint_interiors = true;
        if touching(first, second, support, budget)?
            || radial::close_witness(first, second, 0., budget)?
        {
            return Ok(Classification::Touching);
        }
    }
    if let Some(result) = radial::planar_band(first, second, budget, linear_mm)? {
        return Ok(result);
    }
    if let Some(result) = radial::classify(first, second, budget, linear_mm)? {
        return Ok(result);
    }
    Ok(if disjoint_interiors {
        Classification::DisjointInteriors
    } else {
        Classification::Unresolved
    })
}
