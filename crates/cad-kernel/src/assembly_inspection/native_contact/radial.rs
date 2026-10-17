//! Cylindrical separators for seated shafts and through bores. The open
//! cylinder is connected and unbounded. A bounded closed solid whose entire
//! boundary excludes it cannot contain any of its interior. This is why the
//! same boundary-only argument is deliberately not used for finite spheres.
use super::*;
use num_rational::BigRational as Rational;
use num_traits::{One, ToPrimitive, Zero};

type Point = [Rational; 4];
type Patch = Vec<Vec<Point>>;
const MAX_RADIAL_DEPTH: usize = 12;
const MAX_CANDIDATES: usize = 32;
fn number(value: f64) -> Result<Rational> {
    Rational::from_float(value).ok_or_else(|| unsupported("a radial predicate input is nonfinite"))
}
fn exact(value: &Exact) -> Rational {
    if value.exponent >= 0 {
        Rational::from_integer(&value.integer << value.exponent as usize)
    } else {
        Rational::new(
            value.integer.clone(),
            BigInt::one() << -value.exponent as usize,
        )
    }
}
fn hpoint(point: &Homogeneous) -> Point {
    std::array::from_fn(|i| exact(&point.0[i]))
}
fn point(point: Vector4) -> Result<Point> {
    if point.w <= 0. {
        return Err(unsupported("a radial carrier weight is not positive"));
    }
    Ok([
        number(point.x)?,
        number(point.y)?,
        number(point.z)?,
        number(point.w)?,
    ])
}
fn homogeneous(point: &[Rational; 3]) -> Homogeneous {
    let weight = point.iter().fold(BigInt::one(), |a, b| a * b.denom());
    let mut result = Homogeneous(std::array::from_fn(|_| Exact::zero()));
    for (i, coordinate) in point.iter().enumerate() {
        result.0[i].integer = coordinate.numer() * (&weight / coordinate.denom());
    }
    result.0[3].integer = weight;
    result
}
fn dot(a: &[Rational; 3], b: &[Rational; 3]) -> Rational {
    (0..3).map(|i| &a[i] * &b[i]).sum()
}
fn cross(a: &[Rational; 3], b: &[Rational; 3]) -> [Rational; 3] {
    std::array::from_fn(|i| &a[(i + 1) % 3] * &b[(i + 2) % 3] - &a[(i + 2) % 3] * &b[(i + 1) % 3])
}
fn checked_size(
    points: impl IntoIterator<Item = Point>,
    budget: &mut Budget,
) -> Result<Vec<Point>> {
    let points: Vec<_> = points.into_iter().collect();
    admit_count(points.len(), budget)?;
    if points
        .iter()
        .flatten()
        .any(|p| p.numer().bits() > 32768 || p.denom().bits() > 32768)
    {
        return Err(unsupported(
            "exact radial coefficients exceed the integer bit budget",
        ));
    }
    Ok(points)
}
fn knot_spans(
    knots: &[f64],
    controls: Vec<Point>,
    budget: &mut Budget,
) -> Result<Option<Vec<Vec<Point>>>> {
    let Some(degree) = knots.len().checked_sub(controls.len() + 1) else {
        return Ok(None);
    };
    if degree > 32 || controls.len() <= degree {
        return Ok(None);
    }
    let start = knots[degree];
    let end = knots[controls.len()];
    if start >= end
        || !knots[..=degree].iter().all(|x| *x == start)
        || !knots[controls.len()..].iter().all(|x| *x == end)
    {
        return Ok(None);
    }
    let mut values: Vec<_> = knots
        .iter()
        .copied()
        .filter(|x| *x > start && *x < end)
        .collect();
    values.sort_by(f64::total_cmp);
    values.dedup();
    let mut knots = knots
        .iter()
        .copied()
        .map(number)
        .collect::<Result<Vec<_>>>()?;
    let mut controls = controls;
    for value in values {
        let value = number(value)?;
        loop {
            let multiplicity = knots.iter().filter(|k| **k == value).count();
            if multiplicity >= degree {
                break;
            }
            let span = knots
                .iter()
                .rposition(|k| *k <= value)
                .ok_or_else(|| unsupported("a native knot span is unavailable"))?;
            budget.spend(controls.len().saturating_mul(degree + 1))?;
            let mut next = Vec::with_capacity(controls.len() + 1);
            for index in 0..=controls.len() {
                if index <= span - degree {
                    next.push(controls[index].clone());
                } else if index > span - multiplicity {
                    next.push(controls[index - 1].clone());
                } else {
                    let denominator = &knots[index + degree] - &knots[index];
                    if denominator <= Rational::zero() {
                        return Ok(None);
                    }
                    let alpha = (&value - &knots[index]) / denominator;
                    let beta = Rational::one() - &alpha;
                    next.push(std::array::from_fn(|i| {
                        &alpha * &controls[index][i] + &beta * &controls[index - 1][i]
                    }));
                }
            }
            controls = checked_size(next, budget)?;
            knots.insert(span + 1, value.clone());
        }
    }
    let mut spans = Vec::new();
    for index in degree..controls.len() {
        if knots[index] < knots[index + 1] {
            spans.push(controls[index - degree..=index].to_vec());
        }
    }
    Ok((!spans.is_empty()).then_some(spans))
}
pub(super) fn native_spans(
    curve: &NativeCurve,
    budget: &mut Budget,
) -> Result<Option<Vec<Vec<Homogeneous>>>> {
    let controls = curve.controls.iter().map(hpoint).collect::<Vec<_>>();
    let spans = if curve.bezier {
        vec![controls]
    } else {
        let Some(knots) = &curve.knots else {
            return Ok(None);
        };
        let Some(spans) = knot_spans(knots, controls, budget)? else {
            return Ok(None);
        };
        spans
    };
    let mut output = Vec::new();
    for span in spans {
        let mut denominator = BigInt::one();
        for coordinate in span.iter().flatten() {
            // Ratios already normalize their denominator. Repeated factors are
            // removed by the rational denominator of 1/a + 1/b (an LCM or a
            // divisor); the fallback product remains exact and bounded.
            if &denominator % coordinate.denom() != BigInt::zero() {
                denominator *= coordinate.denom();
            }
            if denominator.bits() > 32768 {
                return Err(unsupported(
                    "exact winding span exceeds its integer bit budget",
                ));
            }
        }
        budget.spend(span.len().saturating_mul(4))?;
        output.push(
            span.into_iter()
                .map(|point| {
                    Homogeneous(std::array::from_fn(|i| Exact {
                        integer: point[i].numer() * (&denominator / point[i].denom()),
                        exponent: 0,
                    }))
                })
                .collect(),
        );
    }
    Ok(Some(output))
}
fn curve_spans(curve: &Curve, budget: &mut Budget) -> Result<Option<Vec<Vec<Point>>>> {
    match curve {
        Curve::Line(Line(a, b)) => Ok(Some(vec![vec![
            point(a.to_homogeneous())?,
            point(b.to_homogeneous())?,
        ]])),
        Curve::BsplineCurve(curve) => knot_spans(
            curve.knot_vector(),
            curve
                .control_points()
                .iter()
                .map(|p| point(p.to_homogeneous()))
                .collect::<Result<_>>()?,
            budget,
        ),
        Curve::NurbsCurve(curve) => knot_spans(
            curve.knot_vector(),
            curve
                .control_points()
                .iter()
                .copied()
                .map(point)
                .collect::<Result<_>>()?,
            budget,
        ),
        _ => {
            let Some(curve) = native_curve(curve, budget)? else {
                return Ok(None);
            };
            if !curve.bezier {
                return Ok(None);
            }
            Ok(Some(vec![curve.controls.iter().map(hpoint).collect()]))
        }
    }
}
fn surface_patches(surface: &Surface, budget: &mut Budget) -> Result<Option<Vec<Patch>>> {
    let (u, v, controls) = match surface {
        Surface::BsplineSurface(surface) => (
            surface.knot_vector_u(),
            surface.knot_vector_v(),
            surface
                .control_points()
                .iter()
                .map(|row| {
                    row.iter()
                        .map(|p| point(p.to_homogeneous()))
                        .collect::<Result<Vec<_>>>()
                })
                .collect::<Result<Vec<_>>>()?,
        ),
        Surface::NurbsSurface(surface) => (
            surface.knot_vector_u(),
            surface.knot_vector_v(),
            surface
                .control_points()
                .iter()
                .map(|row| row.iter().copied().map(point).collect::<Result<Vec<_>>>())
                .collect::<Result<Vec<_>>>()?,
        ),
        _ => return Ok(None),
    };
    if controls.is_empty() || controls[0].is_empty() {
        return Ok(None);
    }
    admit_count(controls.len().saturating_mul(controls[0].len()), budget)?;
    let mut columns = Vec::new();
    for j in 0..controls[0].len() {
        let Some(spans) = knot_spans(
            u,
            controls.iter().map(|row| row[j].clone()).collect(),
            budget,
        )?
        else {
            return Ok(None);
        };
        columns.push(spans);
    }
    let mut result = Vec::new();
    for index in 0..columns[0].len() {
        let mut rows = Vec::new();
        for i in 0..columns[0][index].len() {
            let Some(spans) = knot_spans(
                v,
                columns
                    .iter()
                    .map(|column| column[index][i].clone())
                    .collect(),
                budget,
            )?
            else {
                return Ok(None);
            };
            rows.push(spans);
        }
        for j in 0..rows[0].len() {
            result.push(rows.iter().map(|row| row[j].clone()).collect());
        }
    }
    Ok(Some(result))
}
fn revolution_patches(face: &NativeFace, budget: &mut Budget) -> Result<Option<Vec<Patch>>> {
    let Surface::RevolutionSurface(processor) = face.face.surface() else {
        return Ok(None);
    };
    let transform = processor.transform();
    if transform.x.w != 0. || transform.y.w != 0. || transform.z.w != 0. || transform.w.w != 1. {
        return Ok(None);
    }
    let entity = processor.entity();
    let origin = [
        number(entity.origin().x)?,
        number(entity.origin().y)?,
        number(entity.origin().z)?,
    ];
    let axis = [
        number(entity.axis().x)?,
        number(entity.axis().y)?,
        number(entity.axis().z)?,
    ];
    let spans = if let Curve::Line(Line(a, b)) = entity.entity_curve() {
        let coordinate = usize::from(!processor.orientation());
        let Some(parameters) = generator_controls(face, coordinate, budget)? else {
            return Ok(None);
        };
        let mut low: Option<Rational> = None;
        let mut high: Option<Rational> = None;
        for control in parameters {
            let control = hpoint(control);
            let t = &control[coordinate] / &control[3];
            low = Some(low.map_or_else(|| t.clone(), |v| v.min(t.clone())));
            high = Some(high.map_or_else(|| t.clone(), |v| v.max(t.clone())));
        }
        let (Some(low), Some(high)) = (low, high) else {
            return Ok(None);
        };
        let a = point(a.to_homogeneous())?;
        let b = point(b.to_homogeneous())?;
        vec![
            [low, high]
                .into_iter()
                .map(|t| std::array::from_fn(|i| &a[i] + &t * (&b[i] - &a[i])))
                .collect(),
        ]
    } else {
        let Some(spans) = curve_spans(entity.entity_curve(), budget)? else {
            return Ok(None);
        };
        spans
    };
    let mut patches = Vec::new();
    for span in spans {
        for quarter in 0..4 {
            budget.spend(span.len().saturating_mul(64))?;
            let mut patch = Vec::new();
            for generator in &span {
                let delta = std::array::from_fn(|i| &generator[i] - &origin[i] * &generator[3]);
                let along = dot(&axis, &delta);
                let axial: [Rational; 3] = std::array::from_fn(|i| &axis[i] * &along);
                let center: [Rational; 3] =
                    std::array::from_fn(|i| &origin[i] * &generator[3] + &axial[i]);
                let cosine: [Rational; 3] = std::array::from_fn(|i| &delta[i] - &axial[i]);
                let sine = cross(&axis, &delta);
                let mut row = Vec::new();
                for (x, y, w) in [(1., 0., 1.), (1., 1., 1.), (0., 2., 2.)] {
                    let (x, y) = match quarter {
                        0 => (x, y),
                        1 => (-y, x),
                        2 => (-x, -y),
                        _ => (y, -x),
                    };
                    let (x, y, w) = (number(x)?, number(y)?, number(w)?);
                    let local: [Rational; 3] =
                        std::array::from_fn(|i| &center[i] * &w + &cosine[i] * &x + &sine[i] * &y);
                    let weight = &generator[3] * &w;
                    let mut world: Point = std::array::from_fn(|_| Rational::zero());
                    for i in 0..3 {
                        world[i] = number(transform.w[i])? * &weight;
                        for j in 0..3 {
                            world[i] += number(transform[j][i])? * &local[j];
                        }
                    }
                    world[3] = weight;
                    row.push(world);
                }
                patch.push(row);
            }
            patches.push(patch);
        }
    }
    Ok(Some(patches))
}
fn binomial(n: usize, k: usize) -> BigInt {
    let mut value = BigInt::one();
    for i in 1..=k.min(n - k) {
        value = value * BigInt::from(n - i + 1) / BigInt::from(i);
    }
    value
}
struct Cylinder {
    origin: [Rational; 3],
    axis: [Rational; 3],
    axis2: Rational,
    radius2: Rational,
}
impl Cylinder {
    fn radial(&self, point: &Point) -> [Rational; 3] {
        cross(
            &self.axis,
            &std::array::from_fn(|i| &point[i] - &self.origin[i] * &point[3]),
        )
    }
    fn q(&self, point: &Point, radius2: &Rational) -> Rational {
        let radial = self.radial(point);
        dot(&radial, &radial) - radius2 * &self.axis2 * &point[3] * &point[3]
    }
    fn candidate(face: &NativeFace, budget: &mut Budget) -> Result<Option<Self>> {
        // Recognition proposes an axis only. Every claim below rechecks the
        // complete native carriers with exact rational predicates.
        let control_count = match face.face.surface() {
            Surface::NurbsSurface(surface) => {
                surface.control_points().iter().map(Vec::len).sum::<usize>()
            }
            Surface::BsplineSurface(surface) => {
                surface.control_points().iter().map(Vec::len).sum::<usize>()
            }
            Surface::RevolutionSurface(_) => 4,
            _ => return Ok(None),
        };
        budget.spend(
            control_count
                .saturating_mul(control_count)
                .saturating_mul(32),
        )?;
        let Ok(axis) = crate::axis_geometry::face_axis(&face.face) else {
            return Ok(None);
        };
        let origin = axis
            .origin
            .into_iter()
            .map(number)
            .collect::<Result<Vec<_>>>()?;
        let axis = axis
            .direction
            .into_iter()
            .map(number)
            .collect::<Result<Vec<_>>>()?;
        let origin: [Rational; 3] = origin.try_into().unwrap();
        let axis: [Rational; 3] = axis.try_into().unwrap();
        let axis2 = dot(&axis, &axis);
        if axis2.is_zero() {
            return Ok(None);
        }
        let Some(p) = native_endpoints(face, budget)?.into_iter().next() else {
            return Ok(None);
        };
        let mut cylinder = Self {
            origin,
            axis,
            axis2,
            radius2: Rational::zero(),
        };
        let radial = cylinder.radial(&p);
        cylinder.radius2 = dot(&radial, &radial) / (&cylinder.axis2 * &p[3] * &p[3]);
        Ok((cylinder.radius2 > Rational::zero()).then_some(cylinder))
    }
    fn patch(
        &self,
        patch: &Patch,
        radius2: &Rational,
        inside: bool,
        budget: &mut Budget,
    ) -> Result<bool> {
        let nu = patch.len() - 1;
        let nv = patch[0].len() - 1;
        let cost = patch.len().saturating_mul(patch[0].len()).pow(2);
        budget.spend(cost.saturating_mul(4))?;
        let radial: Vec<Vec<_>> = patch
            .iter()
            .map(|row| row.iter().map(|p| self.radial(p)).collect())
            .collect();
        for ku in 0..=2 * nu {
            for kv in 0..=2 * nv {
                let mut coefficient = Rational::zero();
                for i in ku.saturating_sub(nu)..=ku.min(nu) {
                    for j in kv.saturating_sub(nv)..=kv.min(nv) {
                        let m = ku - i;
                        let n = kv - j;
                        let scale =
                            binomial(nu, i) * binomial(nu, m) * binomial(nv, j) * binomial(nv, n);
                        coefficient += Rational::from_integer(scale)
                            * (dot(&radial[i][j], &radial[m][n])
                                - radius2 * &self.axis2 * &patch[i][j][3] * &patch[m][n][3]);
                    }
                }
                if (inside && coefficient > Rational::zero())
                    || (!inside && coefficient < Rational::zero())
                {
                    return Ok(false);
                }
            }
        }
        Ok(true)
    }
    fn curve(
        &self,
        controls: &[Point],
        radius2: &Rational,
        inside: bool,
        budget: &mut Budget,
    ) -> Result<bool> {
        self.patch(
            &controls.iter().map(|p| vec![p.clone()]).collect(),
            radius2,
            inside,
            budget,
        )
    }
    fn hull(
        &self,
        points: &[Point],
        radius2: &Rational,
        inside: bool,
        budget: &mut Budget,
    ) -> Result<bool> {
        if points.is_empty() {
            return Ok(false);
        }
        budget.spend(points.len().saturating_mul(8))?;
        if inside {
            return Ok(points
                .iter()
                .all(|p| self.q(p, radius2) <= Rational::zero()));
        }
        // Propose a radial supporting direction from the hull centroid. Its
        // exact projection must exclude the entire convex cylinder disk.
        let mut center: [Rational; 3] = std::array::from_fn(|_| Rational::zero());
        for point in points {
            for i in 0..3 {
                center[i] += &point[i] / &point[3];
            }
        }
        let count = Rational::from_integer(BigInt::from(points.len()));
        for (i, c) in center.iter_mut().enumerate() {
            *c = &*c / &count - &self.origin[i];
        }
        let normal = cross(&self.axis, &cross(&center, &self.axis));
        let norm2 = dot(&normal, &normal);
        if norm2.is_zero() {
            return Ok(false);
        }
        Ok(points.iter().all(|point| {
            let delta = std::array::from_fn(|i| &point[i] - &self.origin[i] * &point[3]);
            let projection = dot(&normal, &delta);
            projection >= Rational::zero()
                && &projection * &projection >= radius2 * &norm2 * &point[3] * &point[3]
        }))
    }
    fn planar(
        &self,
        face: &NativeFace,
        radius2: &Rational,
        inside: bool,
        budget: &mut Budget,
    ) -> Result<bool> {
        let Some(region) = &face.planar_region else {
            return Ok(false);
        };
        if !region.closed {
            return Ok(false);
        }
        let points: Vec<_> = region
            .curves
            .iter()
            .flat_map(|c| c.controls.iter().map(hpoint))
            .collect();
        if self.hull(&points, radius2, inside, budget)? {
            return Ok(true);
        }
        if inside {
            return Ok(false);
        }
        // No boundary may enter the open cylinder. For an oblique cap, the
        // intersection is a connected ellipse; its axis point must be outside
        // the material loops. Parallel cuts give unbounded strips and cannot
        // lie inside a bounded planar material region without a crossing.
        for curve in &region.curves {
            let Some(spans) = native_spans(curve, budget)? else {
                return Ok(false);
            };
            for span in spans {
                if !self.curve(
                    &span.iter().map(hpoint).collect::<Vec<_>>(),
                    radius2,
                    false,
                    budget,
                )? {
                    return Ok(false);
                }
            }
        }
        let Surface::Plane(plane) = face.face.surface() else {
            return Ok(false);
        };
        let Some(support) = Support::new(plane)? else {
            return Ok(false);
        };
        let normal = std::array::from_fn(|i| exact(&support.equation[i]));
        let denominator = dot(&normal, &self.axis);
        if denominator.is_zero() {
            return Ok(true);
        }
        let t = -(dot(&normal, &self.origin) + exact(&support.equation[3])) / denominator;
        let axis_point = std::array::from_fn(|i| &self.origin[i] + &self.axis[i] * &t);
        Ok(matches!(
            locate(
                &homogeneous(&axis_point),
                &region.curves,
                support.projected,
                budget
            )?,
            Location::Outside
        ))
    }
    fn face(
        &self,
        face: &NativeFace,
        radius2: &Rational,
        inside: bool,
        budget: &mut Budget,
    ) -> Result<bool> {
        if matches!(face.face.surface(), Surface::Plane(_)) {
            return self.planar(face, radius2, inside, budget);
        }
        if let Some(bound) = &face.bound {
            let points = bound
                .points()
                .iter()
                .map(|p| point(p.to_homogeneous()))
                .collect::<Result<Vec<_>>>()?;
            if self.hull(&points, radius2, inside, budget)? {
                return Ok(true);
            }
        }
        let patches = if matches!(face.face.surface(), Surface::RevolutionSurface(_)) {
            revolution_patches(face, budget)?
        } else {
            surface_patches(&face.face.surface(), budget)?
        };
        let Some(patches) = patches else {
            return Ok(false);
        };
        for patch in patches {
            if !self.patch_recursive(patch, radius2, inside, 0, budget)? {
                return Ok(false);
            }
        }
        Ok(true)
    }
    fn patch_recursive(
        &self,
        patch: Patch,
        radius2: &Rational,
        inside: bool,
        depth: usize,
        budget: &mut Budget,
    ) -> Result<bool> {
        if self.patch(&patch, radius2, inside, budget)? {
            return Ok(true);
        }
        if depth == MAX_RADIAL_DEPTH {
            return Ok(false);
        }
        let (left, right) = split_patch(patch, depth.is_multiple_of(2), budget)?;
        Ok(
            self.patch_recursive(left, radius2, inside, depth + 1, budget)?
                && self.patch_recursive(right, radius2, inside, depth + 1, budget)?,
        )
    }
    fn body(
        &self,
        body: &Body,
        radius2: &Rational,
        inside: bool,
        budget: &mut Budget,
    ) -> Result<bool> {
        for face in &body.faces {
            if !self.face(face, radius2, inside, budget)? {
                return Ok(false);
            }
        }
        Ok(!body.faces.is_empty())
    }
}
fn split_curve(mut controls: Vec<Point>) -> (Vec<Point>, Vec<Point>) {
    let mut left = vec![controls[0].clone()];
    let mut right = vec![controls.last().unwrap().clone()];
    let half = Rational::new(BigInt::one(), BigInt::from(2));
    while controls.len() > 1 {
        controls = controls
            .windows(2)
            .map(|p| std::array::from_fn(|i| (&p[0][i] + &p[1][i]) * &half))
            .collect();
        left.push(controls[0].clone());
        right.push(controls.last().unwrap().clone());
    }
    right.reverse();
    (left, right)
}
fn split_patch(patch: Patch, u: bool, budget: &mut Budget) -> Result<(Patch, Patch)> {
    budget.spend(
        patch
            .len()
            .saturating_mul(patch[0].len())
            .saturating_mul(patch.len().max(patch[0].len())),
    )?;
    if u {
        let columns: Vec<_> = (0..patch[0].len())
            .map(|j| split_curve(patch.iter().map(|row| row[j].clone()).collect()))
            .collect();
        Ok((
            (0..patch.len())
                .map(|i| columns.iter().map(|c| c.0[i].clone()).collect())
                .collect(),
            (0..patch.len())
                .map(|i| columns.iter().map(|c| c.1[i].clone()).collect())
                .collect(),
        ))
    } else {
        Ok(patch.into_iter().map(split_curve).unzip())
    }
}
fn native_endpoints(face: &NativeFace, budget: &mut Budget) -> Result<Vec<Point>> {
    let mut points = Vec::new();
    // Exact boundary points use the face-local carrier when planar. For other
    // rational faces the shared edge is accepted only when all its controls
    // lie on a complete native surface boundary iso-curve (checked below).
    if let Some(region) = &face.planar_region {
        for curve in &region.curves {
            if curve.clamped {
                points.push(hpoint(&curve.controls[0]));
                points.push(hpoint(curve.controls.last().unwrap()));
            }
        }
        return Ok(points);
    }
    for edge in face.face.edge_iter() {
        if let Some(spans) = curve_spans(&edge.oriented_curve(), budget)? {
            for span in spans {
                points.push(span[0].clone());
                points.push(span.last().unwrap().clone());
            }
        }
    }
    Ok(points)
}
fn uv_admits(face: &NativeFace, uv: &[Rational; 2], budget: &mut Budget) -> Result<bool> {
    let Some(regions) = &face.parameter_regions else {
        return Ok(false);
    };
    let point = homogeneous(&[uv[0].clone(), uv[1].clone(), Rational::zero()]);
    // An exactly evaluated original trim endpoint belongs to the face even
    // when a periodic UV ring has no planar closure. Derived chart connectors
    // are never present in these source curves.
    for curve in regions.iter().flatten() {
        if curve.clamped {
            for endpoint in [&curve.controls[0], curve.controls.last().unwrap()] {
                budget.spend(2)?;
                if (0..2).all(|axis| point.compare(endpoint, axis) == Ordering::Equal) {
                    return Ok(true);
                }
            }
        }
    }
    let mut curves = Vec::new();
    for region in regions {
        if !closed_loop(region, budget)? {
            return Ok(false);
        }
        curves.extend(region.iter().cloned());
    }
    Ok(matches!(
        locate(&point, &curves, [0, 1], budget)?,
        Location::Inside | Location::Boundary
    ))
}
struct Seam<'a> {
    face: &'a NativeFace,
    points: [Point; 2],
    uv: [[Rational; 2]; 2],
}
fn seam_segments<'a>(face: &'a NativeFace, budget: &mut Budget) -> Result<Vec<Seam<'a>>> {
    let surface = face.face.surface();
    let (u, v, controls) = match &surface {
        Surface::BsplineSurface(surface) => (
            surface.knot_vector_u(),
            surface.knot_vector_v(),
            surface
                .control_points()
                .iter()
                .map(|row| {
                    row.iter()
                        .map(|p| point(p.to_homogeneous()))
                        .collect::<Result<Vec<_>>>()
                })
                .collect::<Result<Vec<_>>>()?,
        ),
        Surface::NurbsSurface(surface) => (
            surface.knot_vector_u(),
            surface.knot_vector_v(),
            surface
                .control_points()
                .iter()
                .map(|row| row.iter().copied().map(point).collect::<Result<Vec<_>>>())
                .collect::<Result<Vec<_>>>()?,
        ),
        _ => return Ok(vec![]),
    };
    if controls.is_empty() || controls[0].is_empty() {
        return Ok(vec![]);
    }
    let (nu, nv) = (controls.len(), controls[0].len());
    let (Some(p), Some(q)) = (u.len().checked_sub(nu + 1), v.len().checked_sub(nv + 1)) else {
        return Ok(vec![]);
    };
    if p >= nu
        || q >= nv
        || !u[..=p].iter().all(|x| *x == u[p])
        || !u[nu..].iter().all(|x| *x == u[nu])
        || !v[..=q].iter().all(|x| *x == v[q])
        || !v[nv..].iter().all(|x| *x == v[nv])
    {
        return Ok(vec![]);
    }
    budget.spend(nu.saturating_mul(nv))?;
    let mut seams = Vec::new();
    if q == 1 && nv == 2 {
        for (i, parameter) in [(0, u[p]), (nu - 1, u[nu])] {
            seams.push(Seam {
                face,
                points: [controls[i][0].clone(), controls[i][1].clone()],
                uv: [
                    [number(parameter)?, number(v[q])?],
                    [number(parameter)?, number(v[nv])?],
                ],
            });
        }
    }
    if p == 1 && nu == 2 {
        for (j, parameter) in [(0, v[q]), (nv - 1, v[nv])] {
            seams.push(Seam {
                face,
                points: [controls[0][j].clone(), controls[1][j].clone()],
                uv: [
                    [number(u[p])?, number(parameter)?],
                    [number(u[nu])?, number(parameter)?],
                ],
            });
        }
    }
    Ok(seams)
}
fn point_seam(
    point: &Point,
    seam: &Seam<'_>,
    limit2: &Rational,
    budget: &mut Budget,
) -> Result<bool> {
    budget.spend(32)?;
    let p: [Rational; 3] = std::array::from_fn(|i| &point[i] / &point[3]);
    let a: [Rational; 3] = std::array::from_fn(|i| &seam.points[0][i] / &seam.points[0][3]);
    let b: [Rational; 3] = std::array::from_fn(|i| &seam.points[1][i] / &seam.points[1][3]);
    let delta = std::array::from_fn(|i| &b[i] - &a[i]);
    let denominator = dot(&delta, &delta);
    if denominator.is_zero() {
        return Ok(false);
    }
    let t = (dot(&std::array::from_fn(|i| &p[i] - &a[i]), &delta) / denominator)
        .max(Rational::zero())
        .min(Rational::one());
    let distance = std::array::from_fn(|i| &p[i] - (&a[i] + &t * &delta[i]));
    if dot(&distance, &distance) > *limit2 {
        return Ok(false);
    }
    // Linear rational carriers have a projective parameter. Euclidean chord
    // interpolation is converted exactly before checking the native UV trim.
    let tau = &t * &seam.points[0][3]
        / ((Rational::one() - &t) * &seam.points[1][3] + &t * &seam.points[0][3]);
    let uv = std::array::from_fn(|i| &seam.uv[0][i] + &tau * (&seam.uv[1][i] - &seam.uv[0][i]));
    uv_admits(seam.face, &uv, budget)
}
fn affine_point(point: &Point, transform: &Matrix4) -> Result<Option<Point>> {
    if transform.x.w != 0. || transform.y.w != 0. || transform.z.w != 0. || transform.w.w != 1. {
        return Ok(None);
    }
    let mut world: Point = std::array::from_fn(|_| Rational::zero());
    for i in 0..3 {
        world[i] = number(transform.w[i])? * &point[3];
        for j in 0..3 {
            world[i] += number(transform[j][i])? * &point[j];
        }
    }
    world[3] = point[3].clone();
    Ok(Some(world))
}
fn evaluate_curve(
    curve: &Curve,
    parameter: &Rational,
    budget: &mut Budget,
) -> Result<Option<Point>> {
    let (knots, controls) = match curve {
        Curve::Line(Line(a, b)) => {
            let a = point(a.to_homogeneous())?;
            let b = point(b.to_homogeneous())?;
            return Ok(Some(std::array::from_fn(|i| {
                &a[i] + parameter * (&b[i] - &a[i])
            })));
        }
        Curve::BsplineCurve(curve) => (
            curve.knot_vector(),
            curve
                .control_points()
                .iter()
                .map(|p| point(p.to_homogeneous()))
                .collect::<Result<Vec<_>>>()?,
        ),
        Curve::NurbsCurve(curve) => (
            curve.knot_vector(),
            curve
                .control_points()
                .iter()
                .copied()
                .map(point)
                .collect::<Result<Vec<_>>>()?,
        ),
        _ => return Ok(None),
    };
    let Some(degree) = knots.len().checked_sub(controls.len() + 1) else {
        return Ok(None);
    };
    if degree > 32 || controls.len() <= degree {
        return Ok(None);
    }
    budget.spend(controls.len().saturating_mul(degree + 1))?;
    let knots = knots
        .iter()
        .copied()
        .map(number)
        .collect::<Result<Vec<_>>>()?;
    if parameter < &knots[degree] || parameter > &knots[controls.len()] {
        return Ok(None);
    }
    let Some(span) = (degree..controls.len()).rfind(|i| knots[*i] <= *parameter) else {
        return Ok(None);
    };
    let mut row = controls[span - degree..=span].to_vec();
    for level in 1..=degree {
        for j in (level..=degree).rev() {
            let index = span - degree + j;
            let denominator = &knots[index + degree - level + 1] - &knots[index];
            if denominator.is_zero() {
                return Ok(None);
            }
            let alpha = (parameter - &knots[index]) / denominator;
            let beta = Rational::one() - &alpha;
            row[j] = std::array::from_fn(|i| &alpha * &row[j][i] + &beta * &row[j - 1][i]);
        }
    }
    Ok(row.pop())
}
fn analytic_points(face: &NativeFace, budget: &mut Budget) -> Result<Vec<Point>> {
    let Some(regions) = &face.parameter_regions else {
        return Ok(vec![]);
    };
    let mut parameters = Vec::new();
    for region in regions {
        for curve in region {
            if curve.clamped {
                for control in [&curve.controls[0], curve.controls.last().unwrap()] {
                    let control = hpoint(control);
                    parameters.push([&control[0] / &control[3], &control[1] / &control[3]]);
                }
            }
        }
    }
    if parameters.is_empty() {
        return Ok(vec![]);
    }
    let mut result = Vec::new();
    match face.face.surface() {
        Surface::RevolutionSurface(processor) => {
            let generator = usize::from(!processor.orientation());
            let angle = 1 - generator;
            let low = parameters
                .iter()
                .map(|p| p[generator].clone())
                .min()
                .unwrap();
            let high = parameters
                .iter()
                .map(|p| p[generator].clone())
                .max()
                .unwrap();
            for t in [
                low.clone(),
                high.clone(),
                (low + high) / Rational::from_integer(BigInt::from(2)),
            ] {
                let mut uv = [Rational::zero(), Rational::zero()];
                uv[generator] = t.clone();
                uv[angle] = Rational::zero();
                if uv_admits(face, &uv, budget)?
                    && let Some(local) =
                        evaluate_curve(processor.entity().entity_curve(), &t, budget)?
                    && let Some(world) = affine_point(&local, processor.transform())?
                {
                    result.push(world);
                }
            }
        }
        Surface::SphericalSurface(processor) => {
            let polar = usize::from(!processor.orientation());
            let azimuth = 1 - polar;
            let low = parameters.iter().map(|p| p[azimuth].clone()).min().unwrap();
            let high = parameters.iter().map(|p| p[azimuth].clone()).max().unwrap();
            let mut uv = [Rational::zero(), Rational::zero()];
            uv[azimuth] = (low + high) / Rational::from_integer(BigInt::from(2));
            if uv_admits(face, &uv, budget)? {
                let mut local = point(processor.entity().center().to_homogeneous())?;
                local[2] += number(processor.entity().radius())?;
                if let Some(world) = affine_point(&local, processor.transform())? {
                    result.push(world);
                }
            }
        }
        Surface::ToroidalSurface(processor)
            if uv_admits(face, &[Rational::zero(), Rational::zero()], budget)? =>
        {
            let mut local = point(processor.entity().center().to_homogeneous())?;
            local[0] += number(processor.entity().large_radius())?
                + number(processor.entity().small_radius())?;
            if let Some(world) = affine_point(&local, processor.transform())? {
                result.push(world);
            }
        }
        _ => {}
    }
    Ok(result)
}
fn native_corners(face: &NativeFace, budget: &mut Budget) -> Result<Vec<Point>> {
    let surface = face.face.surface();
    let (u, v, controls) = match &surface {
        Surface::BsplineSurface(surface) if surface.is_clamped() => (
            surface.range_tuple().0,
            surface.range_tuple().1,
            surface
                .control_points()
                .iter()
                .map(|row| {
                    row.iter()
                        .map(|p| point(p.to_homogeneous()))
                        .collect::<Result<Vec<_>>>()
                })
                .collect::<Result<Vec<_>>>()?,
        ),
        Surface::NurbsSurface(surface) if surface.is_clamped() => (
            surface.range_tuple().0,
            surface.range_tuple().1,
            surface
                .control_points()
                .iter()
                .map(|row| row.iter().copied().map(point).collect::<Result<Vec<_>>>())
                .collect::<Result<Vec<_>>>()?,
        ),
        _ => return Ok(vec![]),
    };
    if controls.is_empty() || controls[0].is_empty() {
        return Ok(vec![]);
    }
    budget.spend(controls.len().saturating_mul(controls[0].len()))?;
    let mut result = Vec::new();
    for (i, pu) in [(0, u.0), (controls.len() - 1, u.1)] {
        for (j, pv) in [(0, v.0), (controls[0].len() - 1, v.1)] {
            if uv_admits(face, &[number(pu)?, number(pv)?], budget)? {
                result.push(controls[i][j].clone());
            }
        }
    }
    Ok(result)
}
fn point_plane(
    point: &Point,
    face: &NativeFace,
    limit2: &Rational,
    budget: &mut Budget,
) -> Result<bool> {
    let Some(region) = &face.planar_region else {
        return Ok(false);
    };
    if !region.closed {
        return Ok(false);
    }
    let Surface::Plane(plane) = face.face.surface() else {
        return Ok(false);
    };
    let Some(support) = Support::new(plane)? else {
        return Ok(false);
    };
    budget.spend(32)?;
    let normal = std::array::from_fn(|i| exact(&support.equation[i]));
    let p: [Rational; 3] = std::array::from_fn(|i| &point[i] / &point[3]);
    let signed = dot(&normal, &p) + exact(&support.equation[3]);
    let norm2 = dot(&normal, &normal);
    if &signed * &signed > limit2 * &norm2 {
        return Ok(false);
    }
    let projection = std::array::from_fn(|i| &p[i] - &signed * &normal[i] / &norm2);
    Ok(matches!(
        locate(
            &homogeneous(&projection),
            &region.curves,
            support.projected,
            budget
        )?,
        Location::Inside | Location::Boundary
    ))
}
pub(super) fn close_witness(
    first: &Body,
    second: &Body,
    limit: f64,
    budget: &mut Budget,
) -> Result<bool> {
    let limit2 = number(limit)?.pow(2);
    let mut a = Vec::new();
    let mut b = Vec::new();
    let mut a_seams = Vec::new();
    let mut b_seams = Vec::new();
    for (body, points, seams) in [
        (first, &mut a, &mut a_seams),
        (second, &mut b, &mut b_seams),
    ] {
        for face in &body.faces {
            if face.planar_region.is_some() {
                points.extend(native_endpoints(face, budget)?);
            }
            points.extend(native_corners(face, budget)?);
            points.extend(analytic_points(face, budget)?);
            seams.extend(seam_segments(face, budget)?);
        }
    }
    for (points, body) in [(&a, second), (&b, first)] {
        for point in points {
            for face in &body.faces {
                if point_plane(point, face, &limit2, budget)? {
                    return Ok(true);
                }
            }
        }
    }
    for (points, seams) in [(&a, &b_seams), (&b, &a_seams)] {
        for point in points {
            for seam in seams {
                if point_seam(point, seam, &limit2, budget)? {
                    return Ok(true);
                }
            }
        }
    }
    for p in &a {
        for q in &b {
            budget.spend(8)?;
            let d: [Rational; 3] = std::array::from_fn(|i| &p[i] / &p[3] - &q[i] / &q[3]);
            if dot(&d, &d) <= limit2 {
                return Ok(true);
            }
        }
    }
    Ok(false)
}
fn rational_radius_bounds(radius2: &Rational) -> Option<(f64, f64)> {
    let value = radius2.to_f64()?;
    if !value.is_finite() || value <= 0. {
        return None;
    }
    let low = value.next_down().sqrt().next_down().max(0.);
    let high = value.next_up().sqrt().next_up();
    // Conversion is checked against the exact rational, not assumed correctly
    // rounded by a library conversion.
    let lower = Rational::from_float(low)?;
    let upper = Rational::from_float(high)?;
    (&lower * &lower <= *radius2 && &upper * &upper >= *radius2).then_some((low, high))
}
pub(super) fn planar_band(
    first: &Body,
    second: &Body,
    budget: &mut Budget,
    linear_mm: f64,
) -> Result<Option<Classification>> {
    for support in first.supports.iter().chain(&second.supports) {
        let normal = std::array::from_fn(|i| exact(&support.equation[i]));
        let Some((low, _)) = rational_radius_bounds(&dot(&normal, &normal)) else {
            continue;
        };
        let shift = (linear_mm * 0.2 * low).next_down();
        let band = ((shift * 2.).next_up() / low).next_up();
        if !shift.is_finite() || shift <= 0. || !band.is_finite() || band > linear_mm {
            continue;
        }
        let mut upper = Support {
            equation: support.equation.clone(),
            projected: support.projected,
        };
        let mut lower = Support {
            equation: support.equation.clone(),
            projected: support.projected,
        };
        upper.equation[3] = upper.equation[3].sub(&Exact::new(shift)?);
        lower.equation[3] = lower.equation[3].add(&Exact::new(shift)?);
        for (a, b) in [(first, second), (second, first)] {
            let Some(a) = a.sides(&upper, budget)? else {
                continue;
            };
            if a.positive || !a.negative {
                continue;
            }
            let Some(b) = b.sides(&lower, budget)? else {
                continue;
            };
            if b.negative || !b.positive {
                continue;
            }
            if close_witness(first, second, band, budget)? {
                return Ok(Some(Classification::ContactWithinTolerance {
                    band_mm: band,
                }));
            }
        }
    }
    Ok(None)
}
fn capped(
    cylinder: &Cylinder,
    inner: &Body,
    outer: &Body,
    inside2: &Rational,
    outside2: &Rational,
    budget: &mut Budget,
    band: f64,
) -> Result<bool> {
    if !cylinder.body(inner, inside2, true, budget)? {
        return Ok(false);
    }
    for support in inner.supports.iter().take(32) {
        // Cylinder intersect halfspace remains convex, connected and unbounded;
        // every admitted solid is bounded. A complete outer boundary excluded
        // from this open region therefore excludes its entire interior.
        let normal = std::array::from_fn(|i| exact(&support.equation[i]));
        let Some((length, _)) = rational_radius_bounds(&dot(&normal, &normal)) else {
            continue;
        };
        let shift = if band == 0. {
            0.
        } else {
            (band * 0.25 * length).next_down().max(0.)
        };
        for direction in [1., -1.] {
            let mut inner_support = Support {
                equation: support.equation.clone(),
                projected: support.projected,
            };
            if direction < 0. {
                for coefficient in &mut inner_support.equation {
                    *coefficient = coefficient.neg();
                }
            }
            let mut outer_support = Support {
                equation: inner_support.equation.clone(),
                projected: inner_support.projected,
            };
            inner_support.equation[3] = inner_support.equation[3].add(&Exact::new(shift)?);
            outer_support.equation[3] = outer_support.equation[3].sub(&Exact::new(shift)?);
            let Some(sides) = inner.sides(&inner_support, budget)? else {
                continue;
            };
            if sides.negative || !sides.positive {
                continue;
            }
            let mut excludes = true;
            for face in &outer.faces {
                if face_sides(face, &outer_support, budget)?.is_some_and(|sides| !sides.positive) {
                    continue;
                }
                if !cylinder.face(face, outside2, false, budget)? {
                    excludes = false;
                    break;
                }
            }
            if excludes {
                return Ok(true);
            }
        }
    }
    Ok(false)
}
fn flanged(
    cylinder: &Cylinder,
    inner: &Body,
    outer: &Body,
    inside2: &Rational,
    outside2: &Rational,
    budget: &mut Budget,
    band: f64,
) -> Result<bool> {
    if !cylinder.body(outer, outside2, false, budget)? {
        return Ok(false);
    }
    for support in outer.supports.iter().take(32) {
        let normal = std::array::from_fn(|i| exact(&support.equation[i]));
        let Some((length, _)) = rational_radius_bounds(&dot(&normal, &normal)) else {
            continue;
        };
        let shift = if band == 0. {
            0.
        } else {
            (band * 0.25 * length).next_down().max(0.)
        };
        for direction in [1., -1.] {
            let mut inner_support = Support {
                equation: support.equation.clone(),
                projected: support.projected,
            };
            if direction < 0. {
                for coefficient in &mut inner_support.equation {
                    *coefficient = coefficient.neg();
                }
            }
            let mut outer_support = Support {
                equation: inner_support.equation.clone(),
                projected: inner_support.projected,
            };
            inner_support.equation[3] = inner_support.equation[3].add(&Exact::new(shift)?);
            outer_support.equation[3] = outer_support.equation[3].sub(&Exact::new(shift)?);
            let Some(sides) = outer.sides(&outer_support, budget)? else {
                continue;
            };
            if sides.positive || !sides.negative {
                continue;
            }
            // The part may continue above the bore as a head or flange. The
            // complement of (inside cylinder OR above plane) is connected and
            // unbounded; complete boundary exclusion proves containment.
            let mut contained = true;
            for face in &inner.faces {
                if face_sides(face, &inner_support, budget)?.is_some_and(|sides| !sides.negative) {
                    continue;
                }
                if !cylinder.face(face, inside2, true, budget)? {
                    contained = false;
                    break;
                }
            }
            if contained {
                return Ok(true);
            }
        }
    }
    Ok(false)
}
fn ordered_parallel(floor: &Support, flange: &Support) -> Option<(Support, Support)> {
    let axis = (0..3).find(|&i| floor.equation[i].sign() != Ordering::Equal)?;
    if flange.equation[axis].sign() == Ordering::Equal
        || (0..3).any(|i| {
            floor.equation[i]
                .mul(&flange.equation[axis])
                .sub(&flange.equation[i].mul(&floor.equation[axis]))
                .sign()
                != Ordering::Equal
        })
    {
        return None;
    }
    // Multiplication gives both planes exactly the same normal without a
    // rounded normalization or division. Above-floor must include the flange.
    let floor_scale = floor.equation[axis].clone();
    let mut floor = Support {
        equation: std::array::from_fn(|i| floor.equation[i].mul(&flange.equation[axis])),
        projected: floor.projected,
    };
    let mut flange = Support {
        equation: std::array::from_fn(|i| flange.equation[i].mul(&floor_scale)),
        projected: flange.projected,
    };
    match floor.equation[3].sub(&flange.equation[3]).sign() {
        Ordering::Equal => return None,
        Ordering::Less => {
            for coefficient in floor.equation.iter_mut().chain(&mut flange.equation) {
                *coefficient = coefficient.neg();
            }
        }
        Ordering::Greater => {}
    }
    Some((floor, flange))
}
fn capped_flanged(
    cylinder: &Cylinder,
    inner: &Body,
    outer: &Body,
    inside2: &Rational,
    outside2: &Rational,
    budget: &mut Budget,
    band: f64,
) -> Result<bool> {
    let inside = inner
        .faces
        .iter()
        .map(|face| cylinder.face(face, inside2, true, budget))
        .collect::<Result<Vec<_>>>()?;
    let outside = outer
        .faces
        .iter()
        .map(|face| cylinder.face(face, outside2, false, budget))
        .collect::<Result<Vec<_>>>()?;
    let mut candidates = 0;
    for floor in inner.supports.iter().take(32) {
        for flange in outer.supports.iter().take(32) {
            budget.spend(16)?;
            let Some((floor, flange)) = ordered_parallel(floor, flange) else {
                continue;
            };
            candidates += 1;
            if candidates > 64 {
                return Ok(false);
            }
            let normal = std::array::from_fn(|i| exact(&floor.equation[i]));
            // The cylinder must cross both planes. This supplies an actual
            // open connection to the above-flange halfspace; parallel planes
            // alone would not exclude a detached radial cap beside that space.
            if dot(&normal, &cylinder.axis).is_zero() {
                continue;
            }
            let Some((length, _)) = rational_radius_bounds(&dot(&normal, &normal)) else {
                continue;
            };
            let shift = Exact::new(if band == 0. {
                0.
            } else {
                (band * 0.25 * length).next_down().max(0.)
            })?;
            let shifted = |support: &Support, expand: bool| Support {
                equation: std::array::from_fn(|i| {
                    if i != 3 {
                        support.equation[i].clone()
                    } else if expand {
                        support.equation[i].add(&shift)
                    } else {
                        support.equation[i].sub(&shift)
                    }
                }),
                projected: support.projected,
            };
            let inner_floor = shifted(&floor, true);
            let inner_flange = shifted(&flange, true);
            let outer_floor = shifted(&floor, false);
            let outer_flange = shifted(&flange, false);
            // R = (inside cylinder AND above floor) OR above flange. With
            // exactly parallel, ordered planes, both open R and its open
            // complement are connected and unbounded. Entire boundary
            // containment/exclusion therefore proves the same for bounded
            // solid interiors. Equal plane shifts preserve their strict order.
            let mut contained = true;
            for (face, radial_inside) in inner.faces.iter().zip(&inside) {
                if face_sides(face, &inner_flange, budget)?.is_some_and(|sides| !sides.negative) {
                    continue;
                }
                if !radial_inside
                    || !face_sides(face, &inner_floor, budget)?.is_some_and(|sides| !sides.negative)
                {
                    contained = false;
                    break;
                }
            }
            if !contained {
                continue;
            }
            let mut excluded = true;
            for (face, radial_outside) in outer.faces.iter().zip(&outside) {
                if !face_sides(face, &outer_flange, budget)?.is_some_and(|sides| !sides.positive)
                    || (!radial_outside
                        && !face_sides(face, &outer_floor, budget)?
                            .is_some_and(|sides| !sides.positive))
                {
                    excluded = false;
                    break;
                }
            }
            if excluded {
                return Ok(true);
            }
        }
    }
    Ok(false)
}
pub(super) fn classify(
    first: &Body,
    second: &Body,
    budget: &mut Budget,
    linear_mm: f64,
) -> Result<Option<Classification>> {
    let mut candidates = 0;
    for index in 0..first.faces.len().max(second.faces.len()) {
        for face in [first.faces.get(index), second.faces.get(index)]
            .into_iter()
            .flatten()
        {
            if candidates == MAX_CANDIDATES {
                return Ok(None);
            }
            let Some(cylinder) = Cylinder::candidate(face, budget)? else {
                continue;
            };
            candidates += 1;
            for (inner, outer) in [(first, second), (second, first)] {
                if (cylinder.body(inner, &cylinder.radius2, true, budget)?
                    && cylinder.body(outer, &cylinder.radius2, false, budget)?)
                    || capped(
                        &cylinder,
                        inner,
                        outer,
                        &cylinder.radius2,
                        &cylinder.radius2,
                        budget,
                        0.,
                    )?
                    || flanged(
                        &cylinder,
                        inner,
                        outer,
                        &cylinder.radius2,
                        &cylinder.radius2,
                        budget,
                        0.,
                    )?
                    || capped_flanged(
                        &cylinder,
                        inner,
                        outer,
                        &cylinder.radius2,
                        &cylinder.radius2,
                        budget,
                        0.,
                    )?
                {
                    return Ok(Some(if close_witness(inner, outer, 0., budget)? {
                        Classification::Touching
                    } else {
                        Classification::DisjointInteriors
                    }));
                }
                let Some((low, high)) = rational_radius_bounds(&cylinder.radius2) else {
                    continue;
                };
                let allowance = linear_mm * 0.2;
                let inside = (high + allowance).next_up();
                let outside = (low - allowance).next_down();
                let band = (inside - outside).next_up();
                if outside <= 0. || !band.is_finite() || band > linear_mm {
                    continue;
                }
                let inner2 = number(inside)?.pow(2);
                let outer2 = number(outside)?.pow(2);
                if ((cylinder.body(inner, &inner2, true, budget)?
                    && cylinder.body(outer, &outer2, false, budget)?)
                    || capped(&cylinder, inner, outer, &inner2, &outer2, budget, band)?
                    || flanged(&cylinder, inner, outer, &inner2, &outer2, budget, band)?
                    || capped_flanged(&cylinder, inner, outer, &inner2, &outer2, budget, band)?)
                    && close_witness(inner, outer, band, budget)?
                {
                    return Ok(Some(Classification::ContactWithinTolerance {
                        band_mm: band,
                    }));
                }
            }
        }
    }
    Ok(None)
}
