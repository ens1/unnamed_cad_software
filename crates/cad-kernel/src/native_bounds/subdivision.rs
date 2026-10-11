//! Outward rational-control subdivision for exclusion certificates.
//! Native cut operations construct rounded geometry; they cannot certify the
//! unchanged source. Every homogeneous insertion here retains its error box.
use super::*;

type Control = [Interval; 4];
const MAX_DEGREE: usize = 64;

fn charge(work: &mut usize, count: usize, limit: usize) -> Result<()> {
    *work = work.saturating_add(count);
    if *work > limit {
        return Err(unsupported(
            "Interval subdivision exceeds its proof work budget",
        ));
    }
    Ok(())
}
fn homogeneous(point: Vector4) -> Result<Control> {
    if point.w <= 0. {
        return Err(unsupported(
            "Subdivision requires positive homogeneous weights",
        ));
    }
    Ok([
        Interval::exact(point.x)?,
        Interval::exact(point.y)?,
        Interval::exact(point.z)?,
        Interval::exact(point.w)?,
    ])
}
fn euclidean(point: Control) -> Result<IntervalPoint> {
    if point[3].low <= 0. {
        return Err(unsupported(
            "Subdivision cannot certify a positive rational weight",
        ));
    }
    Ok([
        point[0].div(point[3])?,
        point[1].div(point[3])?,
        point[2].div(point[3])?,
    ])
}
fn validate(knots: &[f64], degree: usize, count: usize) -> Result<()> {
    point_budget(count, 8)?;
    if degree > MAX_DEGREE
        || count <= degree
        || knots.len() != count + degree + 1
        || knots.iter().any(|knot| !knot.is_finite())
        || knots.windows(2).any(|pair| pair[0] > pair[1])
        || knots
            .windows(degree + 2)
            .any(|run| run[0] == run[degree + 1])
        || knots[degree] >= knots[count]
    {
        return Err(unsupported(
            "Subdivision needs a finite valid bounded knot vector within its degree budget",
        ));
    }
    Ok(())
}
fn native_degree(knots: &[f64], count: usize) -> Result<usize> {
    let degree = knots
        .len()
        .checked_sub(count.saturating_add(1))
        .ok_or_else(|| unsupported("Subdivision knot and control counts are inconsistent"))?;
    validate(knots, degree, count)?;
    Ok(degree)
}

struct Insertion {
    front: usize,
    back: usize,
    blends: Vec<Interval>,
}
struct Partition {
    steps: Vec<Insertion>,
    left_count: usize,
    knots: [Vec<f64>; 2],
}
impl Partition {
    fn new(
        knots: &[f64],
        degree: usize,
        count: usize,
        work: &mut usize,
        limit: usize,
    ) -> Result<Option<Self>> {
        validate(knots, degree, count)?;
        let (a, b) = (knots[degree], knots[count]);
        let middle = a * 0.5 + b * 0.5;
        if !middle.is_finite() || middle <= a || middle >= b {
            return Ok(None);
        }
        charge(work, knots.len(), limit)?;
        let mut knots = knots.to_vec();
        let mut count = count;
        let mut steps = Vec::new();
        loop {
            let first = knots.partition_point(|value| *value < middle);
            let after = knots.partition_point(|value| *value <= middle);
            let multiplicity = after - first;
            if multiplicity > degree + 1 {
                return Err(unsupported(
                    "Subdivision encounters excessive knot multiplicity",
                ));
            }
            if multiplicity == degree + 1 {
                return Ok(Some(Self {
                    steps,
                    left_count: first,
                    knots: [
                        knots[..first + degree + 1].to_vec(),
                        knots[first..].to_vec(),
                    ],
                }));
            }
            point_budget(count + 1, 8)?;
            charge(work, knots.len().saturating_add(degree), limit)?;
            let span = after
                .checked_sub(1)
                .ok_or_else(|| unsupported("Subdivision cannot locate an interior span"))?;
            let front = span
                .checked_sub(degree)
                .ok_or_else(|| unsupported("Subdivision span precedes its bounded domain"))?;
            let back = span
                .checked_sub(multiplicity)
                .ok_or_else(|| unsupported("Subdivision multiplicity exceeds its span"))?;
            if back >= count || front > back {
                return Err(unsupported(
                    "Subdivision has inconsistent insertion indices",
                ));
            }
            let mut blends = Vec::new();
            for index in front + 1..=back {
                let alpha = Interval::exact(middle)?
                    .sub(Interval::exact(knots[index])?)?
                    .div(
                        Interval::exact(knots[index + degree])?
                            .sub(Interval::exact(knots[index])?)?,
                    )?;
                // The insertion indices independently prove 0<=alpha<=1.
                blends.push(Interval::new(alpha.low.max(0.), alpha.high.min(1.))?);
            }
            steps.push(Insertion {
                front,
                back,
                blends,
            });
            knots.insert(after, middle);
            count += 1;
        }
    }
    fn apply(
        &self,
        mut controls: Vec<Control>,
        work: &mut usize,
        limit: usize,
    ) -> Result<[Vec<Control>; 2]> {
        for step in &self.steps {
            charge(
                work,
                controls
                    .len()
                    .saturating_add(step.blends.len().saturating_mul(4)),
                limit,
            )?;
            let mut next = vec![controls[0]; controls.len() + 1];
            next[..=step.front].copy_from_slice(&controls[..=step.front]);
            next[step.back + 1..].copy_from_slice(&controls[step.back..]);
            for (offset, alpha) in step.blends.iter().enumerate() {
                let index = step.front + 1 + offset;
                let beta = Interval::exact(1.)?.sub(*alpha)?;
                for coordinate in 0..4 {
                    next[index][coordinate] = alpha
                        .mul(controls[index][coordinate])?
                        .add(beta.mul(controls[index - 1][coordinate])?)?;
                }
            }
            controls = next;
        }
        charge(work, controls.len(), limit)?;
        let right = controls.split_off(self.left_count);
        Ok([controls, right])
    }
}

#[derive(Clone)]
pub(crate) struct CurveBounds {
    knots: Vec<f64>,
    degree: usize,
    controls: Vec<Control>,
}
impl CurveBounds {
    pub(crate) fn from_bezier_controls(controls: Vec<[Interval; 4]>) -> Result<Self> {
        let degree = controls
            .len()
            .checked_sub(1)
            .filter(|degree| *degree <= 32)
            .ok_or_else(|| unsupported("An interval Bezier curve exceeds its degree budget"))?;
        for point in &controls {
            for coordinate in point {
                Interval::new(coordinate.low, coordinate.high)?;
            }
            euclidean(*point)?;
        }
        let mut knots = vec![0.; degree + 1];
        knots.extend(vec![1.; degree + 1]);
        validate(&knots, degree, controls.len())?;
        Ok(Self {
            knots,
            degree,
            controls,
        })
    }
    pub(crate) fn scale_homogeneous(mut self, scale: f64) -> Result<Self> {
        if !scale.is_finite() || scale <= 0. {
            return Err(unsupported(
                "A rational control scale must be finite and positive",
            ));
        }
        let scale = Interval::exact(scale)?;
        for point in &mut self.controls {
            for coordinate in point.iter_mut() {
                *coordinate = coordinate.mul(scale)?;
            }
            euclidean(*point)?;
        }
        Ok(self)
    }
    /// Exact linear homogeneous section of two native columns, enclosing each
    /// arithmetic operation instead of accepting rounded native cut controls.
    pub(crate) fn linear_section(
        first: &BsplineCurve<Vector4>,
        second: &BsplineCurve<Vector4>,
        parameter: f64,
    ) -> Result<Self> {
        let count = first.control_points().len();
        let degree = native_degree(first.knot_vector(), count)?;
        if first.knot_vector() != second.knot_vector()
            || count != second.control_points().len()
            || !parameter.is_finite()
            || !(0.0..=1.0).contains(&parameter)
        {
            return Err(unsupported(
                "A linear section has incompatible native columns",
            ));
        }
        let t = Interval::exact(parameter)?;
        let s = Interval::exact(1.)?.sub(t)?;
        let controls = first
            .control_points()
            .iter()
            .zip(second.control_points())
            .map(|(a, b)| {
                let (a, b) = (homogeneous(*a)?, homogeneous(*b)?);
                let mut output = a;
                for i in 0..4 {
                    output[i] = a[i].mul(s)?.add(b[i].mul(t)?)?;
                }
                euclidean(output)?;
                Ok(output)
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(Self {
            knots: first.knot_vector().iter().copied().collect(),
            degree,
            controls,
        })
    }
    /// Uniform rational-curve displacement bound in a common native knot
    /// frame. The numerator is expressed about a local origin; differences of
    /// weights are retained, rather than assuming rounded weights are equal.
    pub(crate) fn distance_from(
        &self,
        other: &BsplineCurve<Vector4>,
        origin: Point3,
    ) -> Result<f64> {
        if self.knots.as_slice() != other.knot_vector().as_ref()
            || self.controls.len() != other.control_points().len()
        {
            return Err(unsupported(
                "Curve displacement needs an identical knot frame",
            ));
        }
        Self::control_net_distance(&self.controls, other.control_points(), origin)
    }
    /// A tensor-product B-spline basis is positive and sums to one just like
    /// a curve basis. This bound therefore covers an entire rational profile,
    /// including its changed endpoint weights, rather than just its rails.
    pub(crate) fn control_net_distance(
        reference: &[[Interval; 4]],
        actual: &[Vector4],
        origin: Point3,
    ) -> Result<f64> {
        point_budget(reference.len(), 1)?;
        if reference.len() != actual.len() {
            return Err(unsupported(
                "Rational displacement needs matching tensor controls",
            ));
        }
        let origin = exact_point(origin)?;
        let mut displacement = [0.0f64; 3];
        let mut weight_error = 0.0f64;
        let mut radius = 0.0f64;
        let mut weight = f64::INFINITY;
        for (a, b) in reference.iter().zip(actual) {
            euclidean(*a)?;
            let b = homogeneous(*b)?;
            let dw = a[3].sub(b[3])?;
            weight_error = weight_error.max(dw.magnitude());
            weight = weight.min(a[3].low);
            let p = euclidean(b)?;
            let mut relative = p;
            for i in 0..3 {
                displacement[i] =
                    displacement[i].max(a[i].sub(b[i])?.sub(origin[i].mul(dw)?)?.magnitude());
                relative[i] = p[i].sub(origin[i])?;
            }
            radius = radius.max(scaled_norm(1., &relative)?);
        }
        let delta = [
            Interval::exact(displacement[0])?,
            Interval::exact(displacement[1])?,
            Interval::exact(displacement[2])?,
        ];
        Ok(Interval::exact(scaled_norm(1., &delta)?)?
            .add(Interval::exact(radius)?.mul(Interval::exact(weight_error)?)?)?
            .div(Interval::exact(weight)?)?
            .high)
    }
    pub(crate) fn count(&self) -> usize {
        self.controls.len()
    }
    pub(crate) fn enclosure(&self) -> Result<Enclosure> {
        Enclosure::from_interval_hull(
            self.controls
                .iter()
                .copied()
                .map(euclidean)
                .collect::<Result<Vec<_>>>()?,
        )
    }
    pub(crate) fn bisect(self, work: &mut usize, limit: usize) -> Result<Option<[Self; 2]>> {
        let Some(partition) =
            Partition::new(&self.knots, self.degree, self.controls.len(), work, limit)?
        else {
            return Ok(None);
        };
        let [a, b] = partition.apply(self.controls, work, limit)?;
        let [ka, kb] = partition.knots;
        Ok(Some([
            Self {
                knots: ka,
                degree: self.degree,
                controls: a,
            },
            Self {
                knots: kb,
                degree: self.degree,
                controls: b,
            },
        ]))
    }
}
pub(crate) fn subdivision_curve(curve: &Curve) -> Result<Option<CurveBounds>> {
    let (knots, degree, controls) = match curve {
        Curve::Line(Line(a, b)) => (
            vec![0., 0., 1., 1.],
            1,
            vec![
                homogeneous(a.to_homogeneous())?,
                homogeneous(b.to_homogeneous())?,
            ],
        ),
        Curve::BsplineCurve(curve) => {
            point_budget(curve.control_points().len(), 8)?;
            let degree = native_degree(curve.knot_vector(), curve.control_points().len())?;
            (
                curve.knot_vector().iter().copied().collect(),
                degree,
                curve
                    .control_points()
                    .iter()
                    .map(|point| homogeneous(point.to_homogeneous()))
                    .collect::<Result<Vec<_>>>()?,
            )
        }
        Curve::NurbsCurve(curve) => {
            point_budget(curve.control_points().len(), 8)?;
            let degree = native_degree(curve.knot_vector(), curve.control_points().len())?;
            (
                curve.knot_vector().iter().copied().collect(),
                degree,
                curve
                    .control_points()
                    .iter()
                    .copied()
                    .map(homogeneous)
                    .collect::<Result<Vec<_>>>()?,
            )
        }
        _ => return Ok(None),
    };
    validate(&knots, degree, controls.len())?;
    Ok(Some(CurveBounds {
        knots,
        degree,
        controls,
    }))
}

pub(crate) struct SurfaceBounds {
    knots: [Vec<f64>; 2],
    degrees: [usize; 2],
    controls: Vec<Vec<Control>>,
}
impl SurfaceBounds {
    pub(crate) fn count(&self) -> usize {
        self.controls.len() * self.controls[0].len()
    }
    pub(crate) fn enclosure(&self) -> Result<Enclosure> {
        Enclosure::from_interval_hull(
            self.controls
                .iter()
                .flatten()
                .copied()
                .map(euclidean)
                .collect::<Result<Vec<_>>>()?,
        )
    }
    pub(crate) fn bisect(
        self,
        direction: usize,
        work: &mut usize,
        limit: usize,
    ) -> Result<Option<[Self; 2]>> {
        if direction > 1 {
            return Err(unsupported("Subdivision direction must be u or v"));
        }
        let (rows, columns) = (self.controls.len(), self.controls[0].len());
        let count = if direction == 0 { rows } else { columns };
        let Some(partition) = Partition::new(
            &self.knots[direction],
            self.degrees[direction],
            count,
            work,
            limit,
        )?
        else {
            return Ok(None);
        };
        let left = partition.left_count;
        let right = count + partition.steps.len() - left;
        let other = if direction == 0 { columns } else { rows };
        point_budget(
            left.checked_mul(other)
                .ok_or_else(|| unsupported("Subdivision grid exceeds its allocation budget"))?,
            8,
        )?;
        point_budget(
            right
                .checked_mul(other)
                .ok_or_else(|| unsupported("Subdivision grid exceeds its allocation budget"))?,
            8,
        )?;
        charge(work, (left + right).saturating_mul(other), limit)?;
        let empty = self.controls[0][0];
        let mut result = if direction == 0 {
            [
                vec![vec![empty; columns]; left],
                vec![vec![empty; columns]; right],
            ]
        } else {
            [Vec::with_capacity(rows), Vec::with_capacity(rows)]
        };
        if direction == 0 {
            for column in 0..columns {
                charge(work, rows, limit)?;
                let curve = self.controls.iter().map(|row| row[column]).collect();
                let halves = partition.apply(curve, work, limit)?;
                for side in 0..2 {
                    for (row, control) in halves[side].iter().enumerate() {
                        result[side][row][column] = *control;
                    }
                }
            }
        } else {
            for row in self.controls {
                let [a, b] = partition.apply(row, work, limit)?;
                result[0].push(a);
                result[1].push(b);
            }
        }
        let [a, b] = result;
        let [ka, kb] = partition.knots;
        let mut first_knots = self.knots.clone();
        first_knots[direction] = ka;
        let mut second_knots = self.knots;
        second_knots[direction] = kb;
        Ok(Some([
            Self {
                knots: first_knots,
                degrees: self.degrees,
                controls: a,
            },
            Self {
                knots: second_knots,
                degrees: self.degrees,
                controls: b,
            },
        ]))
    }
}
pub(crate) fn subdivision_surface(face: &Face) -> Result<Option<SurfaceBounds>> {
    let Some(surface) = rational_surface(&face.surface())? else {
        return Ok(None);
    };
    let controls = surface.control_points();
    let rows = controls.len();
    let columns = controls.first().map_or(0, Vec::len);
    point_budget(
        rows.checked_mul(columns)
            .ok_or_else(|| unsupported("Subdivision grid exceeds its allocation budget"))?,
        8,
    )?;
    if controls.iter().any(|row| row.len() != columns) {
        return Err(unsupported("Subdivision needs a rectangular control grid"));
    }
    let (u, v) = surface.knot_vectors();
    let du = native_degree(u, rows)?;
    let dv = native_degree(v, columns)?;
    Ok(Some(SurfaceBounds {
        knots: [u.iter().copied().collect(), v.iter().copied().collect()],
        degrees: [du, dv],
        controls: controls
            .iter()
            .map(|row| row.iter().copied().map(homogeneous).collect())
            .collect::<Result<Vec<_>>>()?,
    }))
}
