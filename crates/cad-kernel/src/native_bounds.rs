//! Conservative native enclosures for retaining geometry outside an edit.
//! Exact rational controls remain the tight first choice. Analytic carriers
//! use affine support bounds; intersection leaders and display meshes are never
//! enclosure inputs.
use cad_core::{CadError, Result};
use monstertruck::{geometry::prelude::TryIntoHomogeneousBsplineSurface, modeling::*};

const MAX_HULL_POINTS: usize = 262_144;

fn point_budget(count: usize, expansion: usize) -> Result<()> {
    if count == 0
        || count
            .checked_mul(expansion)
            .is_none_or(|count| count > MAX_HULL_POINTS)
    {
        return Err(unsupported(
            "The enclosure is empty or exceeds its control budget",
        ));
    }
    Ok(())
}

/// Directed rounding encloses every product and sum before cancellation.
/// These intervals represent arithmetic uncertainty, never model tolerance.
#[derive(Clone, Copy)]
pub(crate) struct Interval {
    pub(crate) low: f64,
    pub(crate) high: f64,
}
impl Interval {
    pub(crate) fn new(low: f64, high: f64) -> Result<Self> {
        if !low.is_finite() || !high.is_finite() || low > high {
            return Err(unsupported(
                "Enclosure arithmetic has no finite outward error bound",
            ));
        }
        Ok(Self { low, high })
    }
    pub(crate) fn exact(value: f64) -> Result<Self> {
        Self::new(value, value)
    }
    pub(crate) fn rounded(value: f64) -> Result<Self> {
        Self::new(value.next_down(), value.next_up())
    }
    pub(crate) fn add(self, other: Self) -> Result<Self> {
        Self::new(
            (self.low + other.low).next_down(),
            (self.high + other.high).next_up(),
        )
    }
    pub(crate) fn sub(self, other: Self) -> Result<Self> {
        Self::new(
            (self.low - other.high).next_down(),
            (self.high - other.low).next_up(),
        )
    }
    pub(crate) fn mul(self, other: Self) -> Result<Self> {
        let products = [
            self.low * other.low,
            self.low * other.high,
            self.high * other.low,
            self.high * other.high,
        ];
        if products.iter().any(|value| !value.is_finite()) {
            return Err(unsupported(
                "Enclosure multiplication has no finite outward error bound",
            ));
        }
        let low = products
            .into_iter()
            .fold(f64::INFINITY, f64::min)
            .next_down();
        let high = products
            .into_iter()
            .fold(f64::NEG_INFINITY, f64::max)
            .next_up();
        Self::new(low, high)
    }
    pub(crate) fn div(self, other: Self) -> Result<Self> {
        if other.low <= 0. && other.high >= 0. {
            return Err(unsupported("Enclosure division cannot exclude zero"));
        }
        self.mul(Self::new(
            (1. / other.high).next_down(),
            (1. / other.low).next_up(),
        )?)
    }
    pub(crate) fn sqrt(self) -> Result<Self> {
        if self.low < 0. {
            return Err(unsupported(
                "Enclosure square root cannot exclude negative values",
            ));
        }
        Self::new(
            self.low.sqrt().next_down().max(0.),
            self.high.sqrt().next_up(),
        )
    }
    pub(crate) fn magnitude(self) -> f64 {
        self.low.abs().max(self.high.abs())
    }
    pub(crate) fn widen(self, reach: f64) -> Result<Self> {
        if reach < 0. {
            return Err(unsupported("Enclosure support radius is negative"));
        }
        Self::new(
            (self.low - reach).next_down(),
            (self.high + reach).next_up(),
        )
    }
}

pub(crate) type IntervalPoint = [Interval; 3];
pub(crate) fn exact_point(point: Point3) -> Result<IntervalPoint> {
    Ok([
        Interval::exact(point.x)?,
        Interval::exact(point.y)?,
        Interval::exact(point.z)?,
    ])
}
pub(crate) fn dot_intervals(a: &IntervalPoint, b: &IntervalPoint) -> Result<Interval> {
    let mut result = Interval::exact(0.)?;
    for index in 0..3 {
        result = result.add(a[index].mul(b[index])?)?;
    }
    Ok(result)
}

struct InverseAffine {
    matrix: Matrix3,
    translation: IntervalPoint,
    residual_factor: f64,
}
impl InverseAffine {
    fn new(transform: &Matrix4) -> Result<Self> {
        let source = Matrix3::from_cols(
            transform.x.truncate(),
            transform.y.truncate(),
            transform.z.truncate(),
        );
        let matrix = source
            .invert()
            .ok_or_else(|| unsupported("A collapsed affine transform has no inverse enclosure"))?;
        let mut residual = 0.0f64;
        for row in 0..3 {
            let mut row_sum = Interval::exact(0.)?;
            for column in 0..3 {
                let mut value = Interval::exact(0.)?;
                for k in 0..3 {
                    value = value.add(
                        Interval::exact(matrix[k][row])?
                            .mul(Interval::exact(source[column][k])?)?,
                    )?;
                }
                let entry = Interval::exact(if row == column { 1. } else { 0. })?.sub(value)?;
                row_sum = row_sum.add(Interval::exact(entry.magnitude())?)?;
            }
            residual = residual.max(row_sum.high);
        }
        if residual >= 1. {
            return Err(unsupported(
                "The affine inverse residual cannot certify a finite enclosure",
            ));
        }
        // B*A = I-R. The Neumann bound ||x-B*y||∞ <=
        // ||R||∞/(1-||R||∞) ||B*y||∞ encloses the exact inverse;
        // merely applying rounded inverse coefficients is insufficient.
        let residual_factor = Interval::exact(residual)?
            .div(Interval::exact(1.)?.sub(Interval::exact(residual)?)?)?
            .high;
        Ok(Self {
            matrix,
            translation: exact_point(Point3::from_vec(transform.w.truncate()))?,
            residual_factor,
        })
    }
    fn point(&self, point: Point3) -> Result<IntervalPoint> {
        let point = exact_point(point)?;
        let mut local = [Interval::exact(0.)?; 3];
        for (row, result) in local.iter_mut().enumerate() {
            for (column, coordinate) in point.iter().enumerate() {
                *result = result.add(
                    Interval::exact(self.matrix[column][row])?
                        .mul(coordinate.sub(self.translation[column])?)?,
                )?;
            }
        }
        let scale = local
            .iter()
            .fold(0.0f64, |scale, value| scale.max(value.magnitude()));
        let error = Interval::exact(scale)?
            .mul(Interval::exact(self.residual_factor)?)?
            .high;
        for coordinate in &mut local {
            *coordinate = coordinate.widen(error)?;
        }
        Ok(local)
    }
}

fn interval_dot(a: Vector3, b: Vector3) -> Result<Interval> {
    let mut sum = Interval::exact(0.)?;
    for index in 0..3 {
        sum = sum.add(Interval::exact(a[index])?.mul(Interval::exact(b[index])?)?)?;
    }
    Ok(sum)
}

fn affine_projection(
    transform: &Matrix4,
    center: Point3,
    origin: Point3,
    normal: Vector3,
) -> Result<Interval> {
    let mut signed = Interval::exact(0.)?;
    for row in 0..3 {
        let mut coordinate = Interval::exact(transform.w[row])?;
        for column in 0..3 {
            coordinate = coordinate.add(
                Interval::exact(transform[column][row])?.mul(Interval::exact(center[column])?)?,
            )?;
        }
        signed = signed.add(
            coordinate
                .sub(Interval::exact(origin[row])?)?
                .mul(Interval::exact(normal[row])?)?,
        )?;
    }
    Ok(signed)
}

pub(crate) fn scaled_norm(scale: f64, vector: &[Interval]) -> Result<f64> {
    if !scale.is_finite() || scale < 0. {
        return Err(unsupported(
            "Enclosure support radius is not finite and nonnegative",
        ));
    }
    let maximum = vector.iter().fold(0.0f64, |maximum, value| {
        maximum.max(value.low.abs()).max(value.high.abs())
    });
    if maximum == 0. || scale == 0. {
        return Ok(0.);
    }
    // Scale before squaring: large but finite affine coefficients must not
    // overflow merely because their Euclidean norm uses squared components.
    let mut sum = Interval::exact(0.)?;
    for value in vector {
        let upper = (value.low.abs().max(value.high.abs()) / maximum).next_up();
        let upper = Interval::exact(upper)?;
        sum = sum.add(upper.mul(upper)?)?;
    }
    let root = Interval::exact(sum.high.sqrt().next_up())?;
    Ok(root
        .mul(Interval::exact(maximum)?)?
        .mul(Interval::exact(scale)?)?
        .high)
}

enum Analytic {
    Sphere {
        transform: Matrix4,
        center: Point3,
        radius: f64,
    },
    Torus {
        transform: Matrix4,
        center: Point3,
        major: f64,
        minor: f64,
    },
    RevolutionRegion {
        transform: Matrix4,
        center: Point3,
        axis: IntervalPoint,
        axial: (f64, f64),
        radius: f64,
    },
}

pub(crate) struct Enclosure {
    hull: Vec<Point3>,
    analytic: Option<Analytic>,
}

impl Enclosure {
    fn from_analytic(analytic: Analytic) -> Result<Self> {
        let mut result = Self {
            hull: Vec::new(),
            analytic: Some(analytic),
        };
        let ranges = [Vector3::unit_x(), Vector3::unit_y(), Vector3::unit_z()]
            .map(|direction| result.projection(Point3::origin(), direction));
        let [x, y, z] = ranges;
        let (x, y, z) = (x?, y?, z?);
        result.hull = box_hull(Point3::new(x.0, y.0, z.0), Point3::new(x.1, y.1, z.1))?;
        Ok(result)
    }

    pub(crate) fn from_hull(hull: Vec<Point3>) -> Result<Self> {
        Ok(Self {
            hull: checked(hull)?,
            analytic: None,
        })
    }

    fn from_interval_hull(points: impl IntoIterator<Item = IntervalPoint>) -> Result<Self> {
        let mut hull = Vec::new();
        for point in points {
            if hull.len().saturating_add(8) > MAX_HULL_POINTS {
                return Err(unsupported(
                    "The outward control hull exceeds its enclosure budget",
                ));
            }
            hull.extend(box_hull(
                Point3::new(point[0].low, point[1].low, point[2].low),
                Point3::new(point[0].high, point[1].high, point[2].high),
            )?);
        }
        Self::from_hull(hull)
    }

    pub(crate) fn len(&self) -> usize {
        self.hull.len()
    }

    pub(crate) fn points(&self) -> &[Point3] {
        &self.hull
    }

    /// Signed support bounds in the caller's direction. Analytic support
    /// avoids the excess corners of an axis-aligned box on rotated parts.
    /// For an affine transform A, use A^T n in the source carrier's support
    /// function; this remains exact for nonuniform scaling and shear.
    pub(crate) fn projection(&self, origin: Point3, normal: Vector3) -> Result<(f64, f64)> {
        let range = match &self.analytic {
            Some(analytic) => {
                let (transform, center) = match analytic {
                    Analytic::Sphere {
                        transform, center, ..
                    }
                    | Analytic::Torus {
                        transform, center, ..
                    }
                    | Analytic::RevolutionRegion {
                        transform, center, ..
                    } => (transform, center),
                };
                // Keep the source center until this calculation. A rounded
                // transformed center can conceal arbitrarily large cancelled
                // products; widening only its final magnitude is insufficient.
                let signed = affine_projection(transform, *center, origin, normal)?;
                let direction = [
                    interval_dot(transform.x.truncate(), normal)?,
                    interval_dot(transform.y.truncate(), normal)?,
                    interval_dot(transform.z.truncate(), normal)?,
                ];
                match analytic {
                    Analytic::Sphere { radius, .. } => {
                        signed.widen(scaled_norm(*radius, &direction)?)?
                    }
                    Analytic::Torus { major, minor, .. } => {
                        let major = Interval::exact(scaled_norm(*major, &direction[..2])?)?;
                        let minor = Interval::exact(scaled_norm(*minor, &direction)?)?;
                        signed.widen(major.add(minor)?.high)?
                    }
                    Analytic::RevolutionRegion {
                        axis,
                        axial,
                        radius,
                        ..
                    } => {
                        let mut along = Interval::exact(0.)?;
                        for index in 0..3 {
                            along = along.add(direction[index].mul(axis[index])?)?;
                        }
                        let radial = [
                            direction[0].sub(along.mul(axis[0])?)?,
                            direction[1].sub(along.mul(axis[1])?)?,
                            direction[2].sub(along.mul(axis[2])?)?,
                        ];
                        signed
                            .add(along.mul(Interval::new(axial.0, axial.1)?)?)?
                            .widen(scaled_norm(*radius, &radial)?)?
                    }
                }
            }
            None => {
                let mut low = f64::INFINITY;
                let mut high = f64::NEG_INFINITY;
                for point in &self.hull {
                    let mut signed = Interval::exact(0.)?;
                    for index in 0..3 {
                        signed = signed.add(
                            Interval::exact(point[index])?
                                .sub(Interval::exact(origin[index])?)?
                                .mul(Interval::exact(normal[index])?)?,
                        )?;
                    }
                    low = low.min(signed.low);
                    high = high.max(signed.high);
                }
                Interval::new(low, high)?
            }
        };
        Ok((range.low, range.high))
    }
}

fn affine(transform: &Matrix4) -> bool {
    transform.x.w == 0. && transform.y.w == 0. && transform.z.w == 0. && transform.w.w == 1.
}

pub(crate) fn face(face: &Face) -> Result<Enclosure> {
    let surface = face.surface();
    if let Some(analytic) = analytic(&surface) {
        return Enclosure::from_analytic(analytic);
    }
    match &surface {
        Surface::Plane(_) => Enclosure::from_hull(boundary_hull(face)?),
        Surface::RevolutionSurface(processor)
            if matches!(processor.entity().entity_curve(), Curve::Line(_)) =>
        {
            if !affine(processor.transform()) {
                return Err(unsupported(
                    "A line revolution requires an affine enclosure",
                ));
            }
            let inverse = InverseAffine::new(processor.transform())?;
            let local = boundary_hull(face)?
                .into_iter()
                .map(|point| inverse.point(point))
                .collect::<Result<Vec<_>>>()?;
            revolution_region(
                processor.transform(),
                processor.entity().origin(),
                processor.entity().axis(),
                local,
            )
        }
        _ => full_surface(&surface),
    }
}

fn analytic(surface: &Surface) -> Option<Analytic> {
    match surface {
        Surface::SphericalSurface(processor) if affine(processor.transform()) => {
            Some(Analytic::Sphere {
                transform: *processor.transform(),
                center: processor.entity().center(),
                radius: processor.entity().radius().abs(),
            })
        }
        Surface::ToroidalSurface(processor) if affine(processor.transform()) => {
            Some(Analytic::Torus {
                transform: *processor.transform(),
                center: processor.entity().center(),
                major: processor.entity().large_radius().abs(),
                minor: processor.entity().small_radius().abs(),
            })
        }
        _ => None,
    }
}

/// A bounded authoritative curve enclosure. Composite curves use their
/// parameter maps or finite carrier bounds, never their approximate leaders.
pub(crate) fn curve(curve: &Curve) -> Result<Enclosure> {
    match curve {
        Curve::Line(Line(front, back)) => Enclosure::from_hull(vec![*front, *back]),
        Curve::BsplineCurve(curve) => {
            point_budget(curve.control_points().len(), 1)?;
            Enclosure::from_hull(curve.control_points().clone())
        }
        Curve::NurbsCurve(curve) => {
            point_budget(curve.control_points().len(), 8)?;
            Enclosure::from_interval_hull(
                curve
                    .control_points()
                    .iter()
                    .map(rational_point)
                    .collect::<Result<Vec<_>>>()?,
            )
        }
        Curve::ParameterCurve(curve) => parameter_curve(curve),
        Curve::IntersectionCurve(curve) => {
            // Each candidate encloses the entire authoritative intersection.
            // Unknown candidates contribute no constraint; do not let a native
            // approximate-similarity box shrink a separately proven enclosure.
            let mut result: Option<Enclosure> = None;
            for candidate in curve
                .boundary0()
                .into_iter()
                .chain(curve.boundary1())
                .map(parameter_curve)
                .chain([
                    full_surface(curve.surface0()),
                    full_surface(curve.surface1()),
                ])
            {
                let candidate = match candidate {
                    Ok(candidate) => candidate,
                    Err(CadError::Unsupported(_)) => continue,
                    Err(error) => return Err(error),
                };
                result = Some(match result {
                    Some(current) => intersect(current, candidate)?,
                    None => candidate,
                });
            }
            result.ok_or_else(|| {
                unsupported("The intersection has no proven finite carrier enclosure")
            })
        }
    }
}

fn rational_point(point: &Vector4) -> Result<IntervalPoint> {
    if point.w <= 0. {
        return Err(unsupported("The rational enclosure needs positive weights"));
    }
    let weight = Interval::exact(point.w)?;
    Ok([
        Interval::exact(point.x)?.div(weight)?,
        Interval::exact(point.y)?.div(weight)?,
        Interval::exact(point.z)?.div(weight)?,
    ])
}

fn boundary_hull(face: &Face) -> Result<Vec<Point3>> {
    let mut hull = Vec::new();
    for edge in face.edge_iter() {
        let edge_hull = curve(&edge.curve())?.hull;
        if hull.len().saturating_add(edge_hull.len()) > MAX_HULL_POINTS {
            return Err(unsupported(
                "The boundary enclosure exceeds its control budget",
            ));
        }
        hull.extend(edge_hull);
    }
    checked(hull)
}

fn box_hull(low: Point3, high: Point3) -> Result<Vec<Point3>> {
    if low.x > high.x || low.y > high.y || low.z > high.z {
        return Err(unsupported("Independent carrier enclosures do not overlap"));
    }
    checked(
        [low.x, high.x]
            .into_iter()
            .flat_map(|x| {
                [low.y, high.y].into_iter().flat_map(move |y| {
                    [low.z, high.z]
                        .into_iter()
                        .map(move |z| Point3::new(x, y, z))
                })
            })
            .collect(),
    )
}

fn intersect(first: Enclosure, second: Enclosure) -> Result<Enclosure> {
    let mut low = Point3::origin();
    let mut high = Point3::origin();
    for (index, axis) in [Vector3::unit_x(), Vector3::unit_y(), Vector3::unit_z()]
        .into_iter()
        .enumerate()
    {
        let a = first.projection(Point3::origin(), axis)?;
        let b = second.projection(Point3::origin(), axis)?;
        low[index] = a.0.max(b.0);
        high[index] = a.1.min(b.1);
    }
    Enclosure::from_hull(box_hull(low, high)?)
}

/// Axial and radial extrema of a straight generator occur at the boundary.
/// Compute those in the source frame, then apply A^T to support directions;
/// a shear or nonuniform scale must never be replaced by a nominal scale.
fn revolution_region(
    transform: &Matrix4,
    origin: Point3,
    axis: Vector3,
    points: Vec<IntervalPoint>,
) -> Result<Enclosure> {
    if !affine(transform) || points.is_empty() || points.len() > MAX_HULL_POINTS {
        return Err(unsupported("The revolution has no finite affine region"));
    }
    let raw_axis = exact_point(Point3::from_vec(axis))?;
    let square_length = dot_intervals(&raw_axis, &raw_axis)?;
    let length = square_length.sqrt()?;
    let axis = [
        raw_axis[0].div(length)?,
        raw_axis[1].div(length)?,
        raw_axis[2].div(length)?,
    ];
    // A native axis is normalized in floating point. Rodrigues' formula
    // therefore has axial scale k²+(1-k²)cos(v), and transverse scale between
    // min(1,k) and max(1,k), where k is the exact stored axis length. Enclose
    // those small deviations instead of assuming its rounded square equals 1.
    let one = Interval::exact(1.)?;
    let axial_scale = square_length.add(one.sub(square_length)?.mul(Interval::new(-1., 1.)?)?)?;
    if axial_scale.low <= 0. || length.low <= 0. {
        return Err(unsupported(
            "The native revolution axis has no invertible angular enclosure",
        ));
    }
    let axial_factor = axial_scale.div(axial_scale)?;
    let radial_factor = Interval::exact(length.high.max(1.))?
        .div(Interval::exact(length.low.min(1.))?)?
        .high;
    let origin_intervals = exact_point(origin)?;
    let mut axial = (f64::INFINITY, f64::NEG_INFINITY);
    let mut radius = 0.0f64;
    for point in points {
        let offset = [
            point[0].sub(origin_intervals[0])?,
            point[1].sub(origin_intervals[1])?,
            point[2].sub(origin_intervals[2])?,
        ];
        let along = dot_intervals(&offset, &axis)?;
        let radial = [
            offset[0].sub(axis[0].mul(along)?)?,
            offset[1].sub(axis[1].mul(along)?)?,
            offset[2].sub(axis[2].mul(along)?)?,
        ];
        let along = along.mul(axial_factor)?;
        axial = (axial.0.min(along.low), axial.1.max(along.high));
        radius = radius.max(scaled_norm(radial_factor, &radial)?);
    }
    Enclosure::from_analytic(Analytic::RevolutionRegion {
        transform: *transform,
        center: origin,
        axis,
        axial,
        radius,
    })
}

fn parameter_curve(curve: &ParameterCurve<Curve2D, Box<Surface>>) -> Result<Enclosure> {
    // Only these unbounded surfaces require a parameter-domain enclosure.
    // Other admitted carriers have a finite whole-surface bound.
    let surface = curve.surface().as_ref();
    let needs_domain = match surface {
        Surface::Plane(_) => true,
        Surface::RevolutionSurface(processor) => {
            matches!(processor.entity().entity_curve(), Curve::Line(_))
        }
        _ => false,
    };
    if !needs_domain {
        return full_surface(surface);
    }
    let (low, high) = parameter_domain(curve.curve())?;
    match surface {
        Surface::Plane(plane) => {
            let origin = exact_point(plane.origin())?;
            // axis_u/v each evaluate one subtraction of stored coordinates.
            let u = plane.axis_u();
            let v = plane.axis_v();
            let mut points = Vec::new();
            for parameter_u in [low.x, high.x] {
                for parameter_v in [low.y, high.y] {
                    let mut point = origin;
                    for index in 0..3 {
                        point[index] = point[index]
                            .add(Interval::rounded(u[index])?.mul(Interval::exact(parameter_u)?)?)?
                            .add(
                                Interval::rounded(v[index])?.mul(Interval::exact(parameter_v)?)?,
                            )?;
                    }
                    points.push(point);
                }
            }
            Enclosure::from_interval_hull(points)
        }
        Surface::RevolutionSurface(processor) => {
            let Curve::Line(Line(front, back)) = processor.entity().entity_curve() else {
                return full_surface(surface);
            };
            // Processor inversion exchanges the surface parameters.
            let interval = if processor.orientation() {
                [low.x, high.x]
            } else {
                [low.y, high.y]
            };
            let front = exact_point(*front)?;
            let back = exact_point(*back)?;
            let points = interval
                .into_iter()
                .map(|u| {
                    let u = Interval::exact(u)?;
                    let mut point = front;
                    for index in 0..3 {
                        point[index] = front[index].add(back[index].sub(front[index])?.mul(u)?)?;
                    }
                    Ok(point)
                })
                .collect::<Result<Vec<_>>>()?;
            revolution_region(
                processor.transform(),
                processor.entity().origin(),
                processor.entity().axis(),
                points,
            )
        }
        _ => unreachable!(),
    }
}

fn parameter_domain(curve: &Curve2D) -> Result<(Point2, Point2)> {
    let count = match curve {
        Curve2D::Polyline(curve) => curve.0.len(),
        Curve2D::BsplineCurve(curve) => curve.control_points().len(),
        Curve2D::NurbsCurve(curve) => curve.control_points().len(),
        _ => 1,
    };
    point_budget(count, 1)?;
    let exact = |point: Point2| -> Result<[Interval; 2]> {
        Ok([Interval::exact(point.x)?, Interval::exact(point.y)?])
    };
    let points = match curve {
        Curve2D::Line(Line(front, back)) => vec![exact(*front)?, exact(*back)?],
        Curve2D::Polyline(curve) => curve
            .0
            .iter()
            .copied()
            .map(exact)
            .collect::<Result<Vec<_>>>()?,
        Curve2D::BsplineCurve(curve) => curve
            .control_points()
            .iter()
            .copied()
            .map(exact)
            .collect::<Result<Vec<_>>>()?,
        Curve2D::NurbsCurve(curve) => curve
            .control_points()
            .iter()
            .map(|h| {
                if h.z <= 0. {
                    return Err(unsupported(
                        "The parameter curve needs positive rational weights",
                    ));
                }
                let weight = Interval::exact(h.z)?;
                Ok([
                    Interval::exact(h.x)?.div(weight)?,
                    Interval::exact(h.y)?.div(weight)?,
                ])
            })
            .collect::<Result<Vec<_>>>()?,
        Curve2D::Conic(Conic2D::Ellipse(processor)) => {
            let matrix = processor.transform();
            if matrix.x.z != 0. || matrix.y.z != 0. || matrix.z.z != 1. {
                return Err(unsupported(
                    "A parameter ellipse requires an affine enclosure",
                ));
            }
            let unit = Interval::new(-1., 1.)?;
            vec![[
                Interval::exact(matrix.z.x)?
                    .add(Interval::exact(matrix.x.x)?.mul(unit)?)?
                    .add(Interval::exact(matrix.y.x)?.mul(unit)?)?,
                Interval::exact(matrix.z.y)?
                    .add(Interval::exact(matrix.x.y)?.mul(unit)?)?
                    .add(Interval::exact(matrix.y.y)?.mul(unit)?)?,
            ]]
        }
        _ => {
            return Err(unsupported(
                "The parameter curve has no finite exact enclosure",
            ));
        }
    };
    if points.is_empty() || points.len() > MAX_HULL_POINTS {
        return Err(unsupported(
            "The parameter enclosure exceeds its control budget",
        ));
    }
    let mut low = Point2::new(f64::INFINITY, f64::INFINITY);
    let mut high = Point2::new(f64::NEG_INFINITY, f64::NEG_INFINITY);
    for p in points {
        low.x = low.x.min(p[0].low);
        low.y = low.y.min(p[1].low);
        high.x = high.x.max(p[0].high);
        high.y = high.y.max(p[1].high);
    }
    Ok((low, high))
}

fn unsupported(message: &str) -> CadError {
    CadError::Unsupported(format!("Native enclosure: {message}"))
}

fn checked(points: Vec<Point3>) -> Result<Vec<Point3>> {
    if points.is_empty() || points.len() > MAX_HULL_POINTS {
        return Err(unsupported(
            "The enclosure is empty or exceeds its control budget",
        ));
    }
    if points
        .iter()
        .any(|p| ![p.x, p.y, p.z].iter().all(|value| value.is_finite()))
    {
        return Err(CadError::Invalid(
            "Native enclosure has non-finite coordinates".into(),
        ));
    }
    Ok(points)
}

/// Only stored native rational controls may enter a subdivision certificate.
/// Converting an analytic processor first rounds its affine map and carrier
/// construction; those derived coefficients are not an outward enclosure.
/// Analytic surfaces retain their complete interval support bounds above.
mod subdivision;
pub(crate) use subdivision::{CurveBounds, subdivision_curve, subdivision_surface};
mod revolution_strip;
pub(crate) use revolution_strip::source_strip;

fn rational_surface(surface: &Surface) -> Result<Option<BsplineSurface<Vector4>>> {
    let count = match surface {
        Surface::BsplineSurface(surface) => surface
            .control_points()
            .iter()
            .try_fold(0usize, |count, row| count.checked_add(row.len())),
        Surface::NurbsSurface(surface) => surface
            .control_points()
            .iter()
            .try_fold(0usize, |count, row| count.checked_add(row.len())),
        _ => return Ok(None),
    }
    .ok_or_else(|| unsupported("The rational carrier exceeds its control budget"))?;
    point_budget(count, 1)?;
    let Some(net) = surface.try_into_homogeneous_bspline_surface() else {
        return Ok(None);
    };
    let count = net
        .control_points()
        .iter()
        .try_fold(0usize, |count, row| count.checked_add(row.len()));
    if !count.is_some_and(|count| count > 0 && count <= MAX_HULL_POINTS) {
        return Err(unsupported(
            "The rational carrier exceeds its control budget",
        ));
    }
    if net
        .control_points()
        .iter()
        .flatten()
        .any(|h| h.w <= 0. || ![h.x, h.y, h.z, h.w].iter().all(|value| value.is_finite()))
    {
        return Err(unsupported(
            "The rational carrier has no finite positive-weight enclosure",
        ));
    }
    Ok(Some(net))
}

fn full_surface(surface: &Surface) -> Result<Enclosure> {
    if let Some(analytic) = analytic(surface) {
        return Enclosure::from_analytic(analytic);
    }
    if matches!(
        surface,
        Surface::SphericalSurface(_) | Surface::ToroidalSurface(_)
    ) {
        return Err(unsupported(
            "An analytic carrier enclosure requires an affine transform",
        ));
    }
    if let Surface::RevolutionSurface(processor) = surface {
        let points = match processor.entity().entity_curve() {
            Curve::BsplineCurve(curve) => {
                point_budget(curve.control_points().len(), 1)?;
                curve
                    .control_points()
                    .iter()
                    .copied()
                    .map(exact_point)
                    .collect::<Result<Vec<_>>>()?
            }
            Curve::NurbsCurve(curve) => {
                point_budget(curve.control_points().len(), 1)?;
                curve
                    .control_points()
                    .iter()
                    .map(rational_point)
                    .collect::<Result<Vec<_>>>()?
            }
            _ => {
                return Err(unsupported(
                    "The revolution generator has no complete bounded control enclosure",
                ));
            }
        };
        return revolution_region(
            processor.transform(),
            processor.entity().origin(),
            processor.entity().axis(),
            points,
        );
    }
    if let Some(net) = rational_surface(surface)? {
        point_budget(net.control_points().iter().map(Vec::len).sum(), 8)?;
        return Enclosure::from_interval_hull(
            net.control_points()
                .iter()
                .flatten()
                .map(rational_point)
                .collect::<Result<Vec<_>>>()?,
        );
    }
    Err(unsupported(
        "The carrier has no proven finite native enclosure",
    ))
}

pub(crate) fn solid(solid: &Solid) -> Result<Vec<Enclosure>> {
    let mut result = Vec::new();
    let mut points = 0usize;
    for source in solid.face_iter() {
        let enclosure = face(source)?;
        points = points.saturating_add(enclosure.len());
        if points > MAX_HULL_POINTS {
            return Err(unsupported("The body enclosure exceeds its control budget"));
        }
        result.push(enclosure);
    }
    if result.is_empty() {
        return Err(unsupported("The body enclosure is empty"));
    }
    Ok(result)
}
