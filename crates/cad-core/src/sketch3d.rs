//! Spatial sketch intent, analytic curve evaluation and native constraint solving.
//! Coordinates are authoritative f64 millimetres; display lines and drag goals
//! are disposable. A spatial wire is never implicitly flattened into material.
use crate::{CadError, Dimension, Document, Expr, Id, NodeRef, Plane, Result};
use nalgebra::{DMatrix, DVector, Vector3};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const CONTRACT_VERSION: u32 = 1;
pub const MAX_POINTS: usize = 256;
pub const MAX_MEMBERS: usize = 256;
pub const MAX_CONSTRAINTS: usize = 512;
const EPS: f64 = 1e-7;
const ANGULAR_SCALE: f64 = 100.;
fn version() -> u32 {
    CONTRACT_VERSION
}
fn no(value: &bool) -> bool {
    !value
}
fn default_plane(value: &Plane) -> bool {
    value == &Plane::default()
}
fn invalid(message: impl ToString) -> CadError {
    CadError::Invalid(format!("3D sketch: {}", message.to_string()))
}
fn unsupported(message: &str) -> CadError {
    CadError::Unsupported(format!("3D sketch: {message}"))
}
fn identity(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 256
        && !matches!(id, "." | "..")
        && !id
            .chars()
            .any(|c| c.is_control() || matches!(c, '/' | '\\' | ':'))
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Axis {
    X,
    Y,
    Z,
}
impl Axis {
    pub fn index(self) -> usize {
        match self {
            Self::X => 0,
            Self::Y => 1,
            Self::Z => 2,
        }
    }
    pub fn direction(self) -> [f64; 3] {
        std::array::from_fn(|i| if i == self.index() { 1. } else { 0. })
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Definition {
    #[serde(default = "version")]
    pub version: u32,
    pub id: Id,
    pub name: String,
    #[serde(default)]
    pub points: BTreeMap<Id, [f64; 3]>,
    #[serde(default)]
    pub members: BTreeMap<Id, Member>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub constraints: Vec<Constraint>,
    #[serde(default, skip_serializing_if = "default_plane")]
    pub workplane: Plane,
}
impl Definition {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            version: CONTRACT_VERSION,
            id: crate::id(),
            name: name.into(),
            points: BTreeMap::new(),
            members: BTreeMap::new(),
            constraints: vec![],
            workplane: Plane::default(),
        }
    }
    pub fn expressions(&self) -> Vec<&Expr> {
        self.members
            .values()
            .flat_map(|m| m.curve.expressions())
            .chain(
                self.constraints
                    .iter()
                    .flat_map(|c| c.relation.expressions()),
            )
            .collect()
    }
    pub fn source_ids(&self) -> Vec<&Id> {
        std::iter::once(&self.id)
            .chain(self.points.keys())
            .chain(self.members.keys())
            .chain(self.constraints.iter().map(|constraint| &constraint.id))
            .collect()
    }
    /// Source validation admits unfinished geometry for a recoverable shared
    /// draft. Native evaluation separately rejects collapsed curves.
    pub fn validate(&self) -> Result<()> {
        if self.version != CONTRACT_VERSION
            || !identity(&self.id)
            || self.name.len() > 4096
            || self.points.len() > MAX_POINTS
            || self.members.len() > MAX_MEMBERS
            || self.constraints.len() > MAX_CONSTRAINTS
        {
            return Err(invalid(
                "Unsupported version, identity or spatial source limits",
            ));
        }
        validate_plane(&self.workplane)?;
        let mut ids = BTreeSet::from([self.id.as_str()]);
        for (id, p) in &self.points {
            if !identity(id)
                || !ids.insert(id.as_str())
                || p.iter().any(|v| !v.is_finite() || v.abs() > 1e9)
            {
                return Err(invalid(
                    "Point identities and finite XYZ coordinates must be bounded",
                ));
            }
        }
        for (id, member) in &self.members {
            if !identity(id)
                || !ids.insert(id.as_str())
                || member
                    .curve
                    .points()
                    .iter()
                    .any(|p| !self.points.contains_key(*p))
            {
                return Err(invalid("A member identity or point reference is missing"));
            }
            match &member.curve {
                Curve::Circle { plane, .. } => validate_plane(plane)?,
                Curve::Bezier { points } if !(2..=16).contains(&points.len()) => {
                    return Err(invalid("A Bézier needs 2–16 controls"));
                }
                Curve::Nurbs {
                    degree,
                    points,
                    knots,
                    weights,
                } => {
                    validate_knots(*degree, points.len(), knots)?;
                    if weights.len() != points.len() {
                        return Err(invalid("NURBS control and weight counts differ"));
                    }
                }
                Curve::InterpolatingSpline { points, degree, .. }
                    if !(2..=64).contains(&points.len())
                        || *degree == 0
                        || *degree >= points.len()
                        || *degree > 15 =>
                {
                    return Err(invalid(
                        "An interpolating spline needs 2–64 points and degree 1–15 below its point count",
                    ));
                }
                _ => {}
            }
        }
        for c in &self.constraints {
            if !identity(&c.id) || !ids.insert(&c.id) {
                return Err(invalid("Constraint identities must be unique"));
            }
            c.relation.validate(self)?;
        }
        let mut expression_bytes = 0usize;
        for expression in self.expressions() {
            expression_bytes = expression_bytes.saturating_add(expression.0.len());
            if expression.0.len() > 65_536 || expression_bytes > 2 * 1024 * 1024 {
                return Err(invalid(
                    "Spatial expressions exceed their bounded source size",
                ));
            }
            crate::expression::references(&expression.0)?;
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Member {
    #[serde(default, skip_serializing_if = "no")]
    pub construction: bool,
    pub curve: Curve,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Curve {
    Line {
        start: Id,
        end: Id,
    },
    Arc {
        start: Id,
        through: Id,
        end: Id,
    },
    /// Plane u/v supply orientation. Its stored origin is not a second center;
    /// the circle follows the authoritative absolute center point.
    Circle {
        center: Id,
        plane: Plane,
        radius: Expr,
    },
    Bezier {
        points: Vec<Id>,
    },
    Nurbs {
        degree: usize,
        points: Vec<Id>,
        knots: Vec<f64>,
        weights: Vec<Expr>,
    },
    InterpolatingSpline {
        points: Vec<Id>,
        degree: usize,
        parameterization: crate::sketch_spline::Parameterization,
    },
}
impl Curve {
    pub fn points(&self) -> Vec<&Id> {
        match self {
            Self::Line { start, end } => vec![start, end],
            Self::Arc {
                start,
                through,
                end,
            } => vec![start, through, end],
            Self::Circle { center, .. } => vec![center],
            Self::Bezier { points }
            | Self::Nurbs { points, .. }
            | Self::InterpolatingSpline { points, .. } => points.iter().collect(),
        }
    }
    pub fn expressions(&self) -> Vec<&Expr> {
        match self {
            Self::Circle { radius, .. } => vec![radius],
            Self::Nurbs { weights, .. } => weights.iter().collect(),
            _ => vec![],
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Constraint {
    pub id: Id,
    pub relation: Relation,
    #[serde(default, skip_serializing_if = "no")]
    pub driven: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Relation {
    Fixed {
        point: Id,
        position: [Expr; 3],
    },
    Coordinate {
        point: Id,
        axis: Axis,
        value: Expr,
    },
    Coincident {
        first: Id,
        second: Id,
    },
    Distance {
        first: Id,
        second: Id,
        value: Expr,
    },
    AxisDistance {
        first: Id,
        second: Id,
        axis: Axis,
        value: Expr,
    },
    PointLineDistance {
        point: Id,
        line: Id,
        value: Expr,
    },
    Length {
        member: Id,
        value: Expr,
    },
    Radius {
        member: Id,
        value: Expr,
    },
    Angle {
        first: Id,
        second: Id,
        value: Expr,
    },
    AxisAngle {
        member: Id,
        axis: Axis,
        value: Expr,
    },
    AlignAxis {
        member: Id,
        axis: Axis,
    },
    Parallel {
        first: Id,
        second: Id,
    },
    Perpendicular {
        first: Id,
        second: Id,
    },
    EqualLength {
        first: Id,
        second: Id,
    },
    OnLine {
        point: Id,
        line: Id,
        #[serde(default, skip_serializing_if = "no")]
        bounded: bool,
    },
    OnPlane {
        point: Id,
        plane: Plane,
    },
    Coplanar {
        points: Vec<Id>,
        plane: Plane,
    },
    PointOnCurve {
        point: Id,
        member: Id,
        parameter: Expr,
        #[serde(default, skip_serializing_if = "no")]
        free: bool,
    },
    Tangent {
        first: Id,
        second: Id,
        first_parameter: Expr,
        second_parameter: Expr,
    },
}
impl Relation {
    pub fn expressions(&self) -> Vec<&Expr> {
        match self {
            Self::Fixed { position, .. } => position.iter().collect(),
            Self::Coordinate { value, .. }
            | Self::Distance { value, .. }
            | Self::AxisDistance { value, .. }
            | Self::PointLineDistance { value, .. }
            | Self::Length { value, .. }
            | Self::Radius { value, .. }
            | Self::Angle { value, .. }
            | Self::AxisAngle { value, .. } => vec![value],
            Self::PointOnCurve { parameter, .. } => vec![parameter],
            Self::Tangent {
                first_parameter,
                second_parameter,
                ..
            } => vec![first_parameter, second_parameter],
            _ => vec![],
        }
    }
    fn validate(&self, d: &Definition) -> Result<()> {
        let point = |id: &Id| {
            if d.points.contains_key(id) {
                Ok(())
            } else {
                Err(invalid("Constraint point is missing"))
            }
        };
        let member = |id: &Id| {
            if d.members.contains_key(id) {
                Ok(())
            } else {
                Err(invalid("Constraint member is missing"))
            }
        };
        let line = |id: &Id| {
            if matches!(
                d.members.get(id).map(|m| &m.curve),
                Some(Curve::Line { .. })
            ) {
                Ok(())
            } else {
                Err(invalid("This relation requires a straight member"))
            }
        };
        match self {
            Self::Fixed { point: p, .. } | Self::Coordinate { point: p, .. } => point(p),
            Self::Coincident { first, second }
            | Self::Distance { first, second, .. }
            | Self::AxisDistance { first, second, .. } => {
                point(first)?;
                point(second)
            }
            Self::OnLine {
                point: p, line: l, ..
            }
            | Self::PointLineDistance {
                point: p, line: l, ..
            } => {
                point(p)?;
                line(l)
            }
            Self::OnPlane { point: p, plane } => {
                point(p)?;
                validate_plane(plane)
            }
            Self::Coplanar { points, plane } => {
                if points.is_empty() || points.len() > MAX_POINTS {
                    return Err(invalid("Coplanar needs 1–256 point references"));
                }
                for p in points {
                    point(p)?;
                }
                validate_plane(plane)
            }
            Self::Angle { first, second, .. }
            | Self::Parallel { first, second }
            | Self::Perpendicular { first, second } => {
                line(first)?;
                line(second)
            }
            Self::AxisAngle { member: m, .. } | Self::AlignAxis { member: m, .. } => line(m),
            Self::Length { member: m, .. } | Self::Radius { member: m, .. } => member(m),
            Self::EqualLength { first, second } | Self::Tangent { first, second, .. } => {
                member(first)?;
                member(second)
            }
            Self::PointOnCurve {
                point: p,
                member: m,
                ..
            } => {
                point(p)?;
                member(m)
            }
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct DragTarget {
    pub point: Id,
    pub position: [f64; 3],
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Polyline {
    pub member: Id,
    pub points: Vec<[f64; 3]>,
    pub closed: bool,
    pub construction: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Evaluated {
    pub version: u32,
    pub producer: NodeRef,
    pub solved: Definition,
    pub report: crate::solver::SolveReport,
    pub polylines: Vec<Polyline>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub point_motion: BTreeMap<Id, Vec<[f64; 3]>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub constraint_residuals: Vec<ConstraintResidual>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<String>,
}
#[derive(Clone, Debug)]
pub struct ArcFrame {
    pub center: [f64; 3],
    pub normal: [f64; 3],
    pub u: [f64; 3],
    pub v: [f64; 3],
    pub radius_mm: f64,
    pub start_angle_rad: f64,
    pub sweep_angle_rad: f64,
}
pub fn arc_frame(a: [f64; 3], through: [f64; 3], end: [f64; 3]) -> Result<ArcFrame> {
    if a.iter().chain(&through).chain(&end).any(|v| !v.is_finite()) {
        return Err(invalid("Arc coordinates must be finite"));
    }
    let ab = Vector3::from(through) - Vector3::from(a);
    let ac = Vector3::from(end) - Vector3::from(a);
    if ab.norm() <= EPS || ac.norm() <= EPS {
        return Err(invalid("Arc points must be separated"));
    }
    let cross = ab.cross(&ac);
    if cross.norm() <= ab.norm() * ac.norm() * 1e-12 {
        return Err(invalid("Arc points are collinear or uncertain"));
    }
    let u = ab.normalize();
    let normal = cross.normalize();
    let v = normal.cross(&u);
    let (c, r, start, sweep) =
        crate::sketch_geometry::arc_frame([0., 0.], [ab.norm(), 0.], [ac.dot(&u), ac.dot(&v)])?;
    Ok(ArcFrame {
        center: (Vector3::from(a) + u * c[0] + v * c[1]).into(),
        normal: normal.into(),
        u: u.into(),
        v: v.into(),
        radius_mm: r,
        start_angle_rad: start,
        sweep_angle_rad: sweep,
    })
}
pub fn validate_plane(plane: &Plane) -> Result<()> {
    let u = Vector3::from(plane.u);
    let v = Vector3::from(plane.v);
    if plane
        .origin
        .iter()
        .chain(&plane.u)
        .chain(&plane.v)
        .any(|x| !x.is_finite())
        || (u.norm_squared() - 1.).abs() > 1e-9
        || (v.norm_squared() - 1.).abs() > 1e-9
        || u.dot(&v).abs() > 1e-9
    {
        return Err(invalid(
            "Workplane coordinates require a finite orthonormal frame",
        ));
    }
    Ok(())
}
fn validate_knots(degree: usize, count: usize, knots: &[f64]) -> Result<()> {
    if !(2..=64).contains(&count)
        || degree == 0
        || degree > 15
        || degree >= count
        || knots.len() != count + degree + 1
        || knots.iter().any(|x| !x.is_finite())
        || knots.windows(2).any(|p| p[0] > p[1])
    {
        return Err(invalid(
            "NURBS degree, control count and finite ordered knots are inconsistent",
        ));
    }
    let a = knots[degree];
    let b = knots[count];
    if b <= a
        || !(b - a).is_finite()
        || knots[..=degree].iter().any(|x| *x != a)
        || knots[count..].iter().any(|x| *x != b)
        || knots[degree + 1..count].iter().any(|x| *x <= a || *x >= b)
    {
        return Err(invalid(
            "A spatial NURBS needs a finite noncollapsed clamped domain",
        ));
    }
    let mut multiplicity = 0;
    let mut previous = None;
    for knot in &knots[degree + 1..count] {
        multiplicity = if previous == Some(*knot) {
            multiplicity + 1
        } else {
            1
        };
        previous = Some(*knot);
        if multiplicity > degree {
            return Err(invalid("Disconnected interior NURBS knots are unsupported"));
        }
    }
    Ok(())
}
fn point(d: &Definition, id: &str) -> Result<Vector3<f64>> {
    d.points
        .get(id)
        .copied()
        .map(Vector3::from)
        .ok_or_else(|| invalid(format!("Missing spatial point {id}")))
}
fn member<'a>(d: &'a Definition, id: &str) -> Result<&'a Curve> {
    d.members
        .get(id)
        .map(|m| &m.curve)
        .ok_or_else(|| invalid(format!("Missing spatial member {id}")))
}
fn quantity(doc: &Document, owner: &str, value: &Expr, dimension: Dimension) -> Result<f64> {
    crate::expression::value(doc, owner, value, dimension)
}
fn basis(knots: &[f64], degree: usize, count: usize, t: f64) -> Vec<f64> {
    if t == knots[count] {
        let mut r = vec![0.; count];
        r[count - 1] = 1.;
        return r;
    }
    let mut values = (0..knots.len() - 1)
        .map(|i| {
            if knots[i] <= t && t < knots[i + 1] {
                1.
            } else {
                0.
            }
        })
        .collect::<Vec<_>>();
    for order in 1..=degree {
        for i in 0..knots.len() - order - 1 {
            let a = knots[i + order] - knots[i];
            let b = knots[i + order + 1] - knots[i + 1];
            values[i] = if a > 0. {
                (t - knots[i]) / a * values[i]
            } else {
                0.
            } + if b > 0. {
                (knots[i + order + 1] - t) / b * values[i + 1]
            } else {
                0.
            };
        }
    }
    values.truncate(count);
    values
}
/// Exact rational carrier data, including SVD-interpolated spatial fit intent.
/// No projected or sampled geometry supplies the carrier.
pub fn rational_curve(
    doc: &Document,
    owner: &str,
    d: &Definition,
    curve: &Curve,
) -> Result<crate::sketch_edit::Curve3> {
    if let Curve::Bezier { points } = curve
        && !(2..=16).contains(&points.len())
    {
        return Err(invalid("A Bézier needs 2–16 controls"));
    }
    if let Curve::InterpolatingSpline { points, degree, .. } = curve
        && (!(2..=64).contains(&points.len())
            || *degree == 0
            || *degree >= points.len()
            || *degree > 15)
    {
        return Err(invalid(
            "Invalid spatial interpolation degree or point count",
        ));
    }
    let (controls, degree, knots, weights) = match curve {
        Curve::Bezier { points } => {
            let degree = points.len() - 1;
            (
                points
                    .iter()
                    .map(|p| point(d, p).map(Into::into))
                    .collect::<Result<Vec<[f64; 3]>>>()?,
                degree,
                [vec![0.; degree + 1], vec![1.; degree + 1]].concat(),
                vec![1.; points.len()],
            )
        }
        Curve::Nurbs {
            points,
            degree,
            knots,
            weights,
        } => (
            points
                .iter()
                .map(|p| point(d, p).map(Into::into))
                .collect::<Result<Vec<[f64; 3]>>>()?,
            *degree,
            knots.clone(),
            weights
                .iter()
                .map(|w| quantity(doc, owner, w, Dimension::Scalar))
                .collect::<Result<Vec<_>>>()?,
        ),
        Curve::InterpolatingSpline {
            points,
            degree,
            parameterization,
        } => {
            let fit = points
                .iter()
                .map(|p| point(d, p))
                .collect::<Result<Vec<_>>>()?;
            let mut parameters = vec![0.];
            for pair in fit.windows(2) {
                let distance = (pair[1] - pair[0]).norm();
                if !distance.is_finite() || distance <= 1e-9 {
                    return Err(invalid("Consecutive spatial fit points must be separated"));
                }
                let step = match parameterization {
                    crate::sketch_spline::Parameterization::Uniform => 1.,
                    crate::sketch_spline::Parameterization::Chord => distance,
                    crate::sketch_spline::Parameterization::Centripetal => distance.sqrt(),
                };
                parameters.push(parameters.last().copied().unwrap() + step);
            }
            let total = *parameters
                .last()
                .ok_or_else(|| invalid("No spatial fit points"))?;
            if !total.is_finite() || total <= 0. {
                return Err(invalid("Spatial fit parameterization is degenerate"));
            }
            for t in &mut parameters {
                *t /= total;
            }
            if parameters.windows(2).any(|p| p[0] >= p[1]) {
                return Err(invalid("Spatial fit spacing exceeds parameter resolution"));
            }
            let mut knots = vec![0.; degree + 1];
            for start in 1..points.len() - degree {
                knots.push(parameters[start..start + degree].iter().sum::<f64>() / *degree as f64);
            }
            knots.extend(std::iter::repeat_n(1., degree + 1));
            let rows = parameters
                .iter()
                .map(|t| basis(&knots, *degree, points.len(), *t))
                .collect::<Vec<_>>();
            let matrix = DMatrix::from_fn(points.len(), points.len(), |i, j| rows[i][j]);
            let svd = matrix.svd(true, true);
            let largest = svd.singular_values.max();
            if !largest.is_finite() || svd.singular_values.min() <= largest * 1e-12 {
                return Err(invalid(
                    "Spatial spline interpolation is singular or ill-conditioned",
                ));
            }
            let axes = (0..3)
                .map(|axis| {
                    svd.solve(
                        &DVector::from_iterator(
                            fit.len(),
                            fit.iter().map(|p| p[axis] - fit[0][axis]),
                        ),
                        largest * 1e-13,
                    )
                    .map_err(|_| invalid("Spatial interpolation could not be solved"))
                })
                .collect::<Result<Vec<_>>>()?;
            let mut controls = (0..fit.len())
                .map(|i| std::array::from_fn(|axis| axes[axis][i] + fit[0][axis]))
                .collect::<Vec<[f64; 3]>>();
            controls[0] = fit[0].into();
            let last = fit.len() - 1;
            controls[last] = fit[last].into();
            for (row, target) in rows.iter().zip(&fit) {
                let actual = row
                    .iter()
                    .zip(&controls)
                    .fold(Vector3::zeros(), |sum, (c, p)| sum + Vector3::from(*p) * *c);
                if (actual - target).norm() > EPS {
                    return Err(invalid(
                        "Spatial interpolation exceeds the 1e-7 mm fit tolerance",
                    ));
                }
            }
            (controls, *degree, knots, vec![1.; points.len()])
        }
        _ => {
            return Err(unsupported(
                "Only Bézier, NURBS and interpolating members have this rational control representation",
            ));
        }
    };
    validate_knots(degree, controls.len(), &knots)?;
    let maximum = weights.iter().copied().fold(0., f64::max);
    if controls.iter().flatten().any(|x| !x.is_finite())
        || weights.len() != controls.len()
        || maximum <= 0.
        || weights
            .iter()
            .any(|w| !w.is_finite() || *w <= 0. || *w < maximum * 1e-12)
    {
        return Err(invalid(
            "Spatial rational controls need positive, finite, well-conditioned weights",
        ));
    }
    Ok(crate::sketch_edit::Curve3 {
        control_points: controls,
        weights,
        knots,
        degree,
    })
}
#[derive(Clone, Debug)]
pub struct Jet {
    pub point: [f64; 3],
    pub tangent: [f64; 3],
}
fn rational_jet(curve: &crate::sketch_edit::Curve3, t: f64) -> Result<Jet> {
    let count = curve.control_points.len();
    let a = curve.knots[curve.degree];
    let b = curve.knots[count];
    let u = if t == 1. { b } else { a + t * (b - a) };
    let values = basis(&curve.knots, curve.degree, count, u);
    // Derivative controls use the trimmed knot sequence, valid at clamped endpoints.
    let mut derivative_knots = curve.knots.clone();
    derivative_knots.remove(0);
    derivative_knots.pop();
    let derivative_basis = basis(&derivative_knots, curve.degree - 1, count - 1, u);
    let mut numerator = Vector3::zeros();
    let mut denominator = 0.;
    for (i, value) in values.iter().enumerate() {
        numerator += Vector3::from(curve.control_points[i]) * (value * curve.weights[i]);
        denominator += value * curve.weights[i];
    }
    let mut numerator_derivative = Vector3::zeros();
    let mut denominator_derivative = 0.;
    for (i, value) in derivative_basis.iter().enumerate() {
        let span = curve.knots[i + curve.degree + 1] - curve.knots[i + 1];
        if span > 0. {
            let scale = *value * curve.degree as f64 / span;
            numerator_derivative += (Vector3::from(curve.control_points[i + 1])
                * curve.weights[i + 1]
                - Vector3::from(curve.control_points[i]) * curve.weights[i])
                * scale;
            denominator_derivative += (curve.weights[i + 1] - curve.weights[i]) * scale;
        }
    }
    if !denominator.is_finite() || denominator <= 0. {
        return Err(invalid("Spatial rational denominator is invalid"));
    }
    let p = numerator / denominator;
    let tangent = (numerator_derivative - p * denominator_derivative) / denominator * (b - a);
    if p.iter().chain(tangent.iter()).any(|v| !v.is_finite()) {
        return Err(invalid("Spatial curve evaluation is nonfinite"));
    }
    Ok(Jet {
        point: p.into(),
        tangent: tangent.into(),
    })
}
/// Exact current carrier position and normalized-domain first derivative.
pub fn curve_jet(
    doc: &Document,
    owner: &str,
    d: &Definition,
    curve: &Curve,
    t: f64,
) -> Result<Jet> {
    if !t.is_finite() || !(0.0..=1.0).contains(&t) {
        return Err(invalid("Curve parameter must be within 0–1"));
    }
    match curve {
        Curve::Line { start, end } => {
            let a = point(d, start)?;
            let b = point(d, end)?;
            Ok(Jet {
                point: (a + (b - a) * t).into(),
                tangent: (b - a).into(),
            })
        }
        Curve::Arc {
            start,
            through,
            end,
        } => {
            let frame = arc_frame(
                point(d, start)?.into(),
                point(d, through)?.into(),
                point(d, end)?.into(),
            )?;
            let angle = frame.start_angle_rad + frame.sweep_angle_rad * t;
            let u = Vector3::from(frame.u);
            let v = Vector3::from(frame.v);
            Ok(Jet {
                point: (Vector3::from(frame.center)
                    + (u * angle.cos() + v * angle.sin()) * frame.radius_mm)
                    .into(),
                tangent: ((-u * angle.sin() + v * angle.cos())
                    * (frame.radius_mm * frame.sweep_angle_rad))
                    .into(),
            })
        }
        Curve::Circle {
            center,
            plane,
            radius,
        } => {
            let r = quantity(doc, owner, radius, Dimension::Length)?;
            if r <= EPS {
                return Err(invalid("A circle needs positive radius"));
            }
            let a = if t == 1. {
                0.
            } else {
                t * std::f64::consts::TAU
            };
            let u = Vector3::from(plane.u);
            let v = Vector3::from(plane.v);
            Ok(Jet {
                point: (point(d, center)? + (u * a.cos() + v * a.sin()) * r).into(),
                tangent: ((-u * a.sin() + v * a.cos()) * (r * std::f64::consts::TAU)).into(),
            })
        }
        _ => rational_jet(&rational_curve(doc, owner, d, curve)?, t),
    }
}
pub fn curve_point(
    doc: &Document,
    owner: &str,
    d: &Definition,
    curve: &Curve,
    t: f64,
) -> Result<[f64; 3]> {
    Ok(curve_jet(doc, owner, d, curve, t)?.point)
}
pub fn member_length(doc: &Document, owner: &str, d: &Definition, id: &str) -> Result<f64> {
    match member(d, id)? {
        Curve::Line { start, end } => Ok((point(d, end)? - point(d, start)?).norm()),
        Curve::Arc {
            start,
            through,
            end,
        } => {
            let a = arc_frame(
                point(d, start)?.into(),
                point(d, through)?.into(),
                point(d, end)?.into(),
            )?;
            Ok(a.radius_mm * a.sweep_angle_rad.abs())
        }
        Curve::Circle { radius, .. } => {
            Ok(quantity(doc, owner, radius, Dimension::Length)? * std::f64::consts::TAU)
        }
        _ => Err(unsupported(
            "A driving curve length currently needs a straight or circular carrier; spline lengths are not approximated",
        )),
    }
}
pub fn member_radius(doc: &Document, owner: &str, d: &Definition, id: &str) -> Result<f64> {
    match member(d, id)? {
        Curve::Circle { radius, .. } => quantity(doc, owner, radius, Dimension::Length),
        Curve::Arc {
            start,
            through,
            end,
        } => Ok(arc_frame(
            point(d, start)?.into(),
            point(d, through)?.into(),
            point(d, end)?.into(),
        )?
        .radius_mm),
        _ => Err(invalid(
            "Radius dimensions require a circle or circular arc",
        )),
    }
}
pub fn validate_geometry(doc: &Document, owner: &str, d: &Definition) -> Result<()> {
    d.validate()?;
    for (id, m) in &d.members {
        match &m.curve {
            Curve::Line { start, end } if (point(d, start)? - point(d, end)?).norm() <= EPS => {
                return Err(invalid(format!("Member {id} has zero length")));
            }
            Curve::Bezier { .. } | Curve::Nurbs { .. } | Curve::InterpolatingSpline { .. } => {
                let carrier = rational_curve(doc, owner, d, &m.curve)?;
                if carrier.control_points.iter().all(|p| {
                    (Vector3::from(*p) - Vector3::from(carrier.control_points[0])).norm() <= EPS
                }) {
                    return Err(invalid(format!("Member {id} has collapsed controls")));
                }
            }
            _ => {
                curve_jet(doc, owner, d, &m.curve, 0.)?;
            }
        }
    }
    Ok(())
}
/// Construction topology follows explicit coincident relations and source IDs;
/// geometrically nearby independent endpoints never join automatically.
pub fn point_aliases(d: &Definition) -> Result<BTreeMap<Id, Id>> {
    let mut map = d
        .points
        .keys()
        .map(|id| (id.clone(), id.clone()))
        .collect::<BTreeMap<_, _>>();
    for c in d.constraints.iter().filter(|c| !c.driven) {
        if let Relation::Coincident { first, second } = &c.relation {
            let a = map
                .get(first)
                .cloned()
                .ok_or_else(|| invalid("Coincident point is missing"))?;
            let b = map
                .get(second)
                .cloned()
                .ok_or_else(|| invalid("Coincident point is missing"))?;
            let canonical = a.clone().min(b.clone());
            for alias in map.values_mut() {
                if *alias == a || *alias == b {
                    *alias = canonical.clone();
                }
            }
        }
    }
    Ok(map)
}
/// Logical native endpoints, independent of a kernel's circle partition count.
/// A full circle has a periodic seam at normalized parameter zero/one; other
/// circle contacts require an explicit native split and do not create aliases.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Endpoint {
    CircleSeam { member: Id },
    Point { point: Id },
}
fn member_endpoint(d: &Definition, id: &str, t: f64) -> Result<Option<Endpoint>> {
    if t != 0. && t != 1. {
        return Ok(None);
    }
    let point = match member(d, id)? {
        Curve::Line { start, end } | Curve::Arc { start, end, .. } => {
            if t == 0. {
                start
            } else {
                end
            }
        }
        Curve::Bezier { points }
        | Curve::Nurbs { points, .. }
        | Curve::InterpolatingSpline { points, .. } => (if t == 0. {
            points.first()
        } else {
            points.last()
        })
        .ok_or_else(|| invalid("An endpoint curve has no source controls"))?,
        Curve::Circle { .. } => return Ok(Some(Endpoint::CircleSeam { member: id.into() })),
    };
    Ok(Some(Endpoint::Point {
        point: point.clone(),
    }))
}
pub fn endpoint_position(
    doc: &Document,
    owner: &str,
    d: &Definition,
    endpoint: &Endpoint,
) -> Result<[f64; 3]> {
    match endpoint {
        Endpoint::Point { point: id } => Ok(point(d, id)?.into()),
        Endpoint::CircleSeam { member: id } => {
            let Curve::Circle {
                center,
                plane,
                radius,
            } = member(d, id)?
            else {
                return Err(invalid("A circle seam requires a current circle"));
            };
            let radius = quantity(doc, owner, radius, Dimension::Length)?;
            if radius <= EPS {
                return Err(invalid("A circle seam needs a positive radius"));
            }
            Ok((point(d, center)? + Vector3::from(plane.u) * radius).into())
        }
    }
}
/// Aliases follow explicit equality equations and proven endpoint tangencies.
/// Fixed contact expressions are evaluated in the same configured context as
/// the solver. Near endpoints and interior contacts never qualify by proximity.
pub fn endpoint_aliases(
    doc: &Document,
    owner: &str,
    d: &Definition,
) -> Result<BTreeMap<Endpoint, Endpoint>> {
    let mut aliases = d
        .points
        .keys()
        .map(|point| {
            let key = Endpoint::Point {
                point: point.clone(),
            };
            (key.clone(), key)
        })
        .chain(
            d.members
                .iter()
                .filter(|(_, m)| matches!(m.curve, Curve::Circle { .. }))
                .map(|(id, _)| {
                    let key = Endpoint::CircleSeam { member: id.clone() };
                    (key.clone(), key)
                }),
        )
        .collect::<BTreeMap<_, _>>();
    let mut join = |a: Endpoint, b: Endpoint| -> Result<()> {
        let a = aliases
            .get(&a)
            .cloned()
            .ok_or_else(|| invalid("An endpoint equality has a missing source"))?;
        let b = aliases
            .get(&b)
            .cloned()
            .ok_or_else(|| invalid("An endpoint equality has a missing source"))?;
        let canonical = a.clone().min(b.clone());
        for alias in aliases.values_mut() {
            if *alias == a || *alias == b {
                *alias = canonical.clone();
            }
        }
        Ok(())
    };
    for c in d.constraints.iter().filter(|c| !c.driven) {
        match &c.relation {
            Relation::Coincident { first, second } => join(
                Endpoint::Point {
                    point: first.clone(),
                },
                Endpoint::Point {
                    point: second.clone(),
                },
            )?,
            Relation::Distance {
                first,
                second,
                value,
            } if desired(doc, owner, value, Dimension::Length, false)? == 0. => join(
                Endpoint::Point {
                    point: first.clone(),
                },
                Endpoint::Point {
                    point: second.clone(),
                },
            )?,
            Relation::Tangent {
                first,
                second,
                first_parameter,
                second_parameter,
            } => {
                let first = member_endpoint(d, first, parameter(doc, owner, first_parameter)?)?;
                let second = member_endpoint(d, second, parameter(doc, owner, second_parameter)?)?;
                if let (Some(first), Some(second)) = (first, second) {
                    join(first, second)?;
                }
            }
            _ => {}
        }
    }
    Ok(aliases)
}
fn line_direction(d: &Definition, id: &str) -> Result<Vector3<f64>> {
    let Curve::Line { start, end } = member(d, id)? else {
        return Err(invalid("This relation requires a straight member"));
    };
    let direction = point(d, end)? - point(d, start)?;
    if direction.norm() <= EPS {
        return Err(invalid("A line relation needs a noncollapsed direction"));
    }
    Ok(direction.normalize())
}
fn parameter(doc: &Document, owner: &str, value: &Expr) -> Result<f64> {
    let value = quantity(doc, owner, value, Dimension::Scalar)?;
    if !(0.0..=1.0).contains(&value) {
        return Err(invalid("Contact parameters must be within 0–1"));
    }
    Ok(value)
}
fn desired(
    doc: &Document,
    owner: &str,
    value: &Expr,
    dimension: Dimension,
    positive: bool,
) -> Result<f64> {
    let result = quantity(doc, owner, value, dimension)?;
    if dimension == Dimension::Length && ((positive && result <= EPS) || (!positive && result < 0.))
    {
        return Err(invalid(
            "Lengths must be nonnegative; radius/length dimensions need a positive value",
        ));
    }
    if dimension == Dimension::Angle && !(0.0..=std::f64::consts::PI).contains(&result) {
        return Err(invalid(
            "Spatial line angles must be between 0 and 180 degrees",
        ));
    }
    Ok(result)
}
fn regular_contact(
    doc: &Document,
    owner: &str,
    d: &Definition,
    curve: &Curve,
    t: f64,
) -> Result<()> {
    if matches!(
        curve,
        Curve::Bezier { .. } | Curve::Nurbs { .. } | Curve::InterpolatingSpline { .. }
    ) {
        let carrier = rational_curve(doc, owner, d, curve)?;
        let start = carrier.knots[carrier.degree];
        let finish = carrier.knots[carrier.control_points.len()];
        let native = if t == 1. {
            finish
        } else {
            start + t * (finish - start)
        };
        if native > start && native < finish {
            let multiplicity = carrier.knots.iter().filter(|knot| **knot == native).count();
            if multiplicity >= carrier.degree {
                return Err(unsupported(
                    "Tangency at a non-C1 interior spline knot needs an explicit one-sided contact",
                ));
            }
        }
    }
    Ok(())
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConstraintResidual {
    pub constraint: Id,
    pub values: Vec<f64>,
    pub driven: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub measured: Option<f64>,
}
fn relation_residual(
    doc: &Document,
    owner: &str,
    d: &Definition,
    relation: &Relation,
) -> Result<(Vec<f64>, Option<f64>)> {
    let components = |v: Vector3<f64>| vec![v.x, v.y, v.z];
    match relation {
        Relation::Fixed { point: p, position } => Ok((
            components(
                point(d, p)?
                    - Vector3::new(
                        quantity(doc, owner, &position[0], Dimension::Length)?,
                        quantity(doc, owner, &position[1], Dimension::Length)?,
                        quantity(doc, owner, &position[2], Dimension::Length)?,
                    ),
            ),
            None,
        )),
        Relation::Coordinate {
            point: p,
            axis,
            value,
        } => {
            let actual = point(d, p)?[axis.index()];
            Ok((
                vec![actual - quantity(doc, owner, value, Dimension::Length)?],
                Some(actual),
            ))
        }
        Relation::Coincident { first, second } => {
            Ok((components(point(d, first)? - point(d, second)?), None))
        }
        Relation::Distance {
            first,
            second,
            value,
        } => {
            let delta = point(d, first)? - point(d, second)?;
            let actual = delta.norm();
            let target = desired(doc, owner, value, Dimension::Length, false)?;
            Ok((
                if target == 0. {
                    components(delta)
                } else {
                    vec![actual - target]
                },
                Some(actual),
            ))
        }
        Relation::AxisDistance {
            first,
            second,
            axis,
            value,
        } => {
            let actual = point(d, second)?[axis.index()] - point(d, first)?[axis.index()];
            Ok((
                vec![actual - quantity(doc, owner, value, Dimension::Length)?],
                Some(actual),
            ))
        }
        Relation::Length { member: m, value } => {
            let actual = member_length(doc, owner, d, m)?;
            let target = desired(doc, owner, value, Dimension::Length, true)?;
            Ok((
                // A circle's circumference drives its radius expression in
                // prepare(), without inventing a positional radius variable.
                if matches!(member(d, m)?, Curve::Circle { .. }) {
                    vec![]
                } else {
                    vec![actual - target]
                },
                Some(actual),
            ))
        }
        Relation::Radius { member: m, value } => {
            let actual = member_radius(doc, owner, d, m)?;
            let target = desired(doc, owner, value, Dimension::Length, true)?;
            // A full-circle radius is direct expression intent, rather than a
            // fictitious positional DOF. The prepared source owns that value.
            Ok((
                if matches!(member(d, m)?, Curve::Circle { .. }) {
                    vec![]
                } else {
                    vec![actual - target]
                },
                Some(actual),
            ))
        }
        Relation::Angle {
            first,
            second,
            value,
        } => {
            let a = line_direction(d, first)?;
            let b = line_direction(d, second)?;
            let actual = a.cross(&b).norm().atan2(a.dot(&b));
            Ok((
                vec![
                    (actual - desired(doc, owner, value, Dimension::Angle, false)?) * ANGULAR_SCALE,
                ],
                Some(actual),
            ))
        }
        Relation::AxisAngle {
            member: m,
            axis,
            value,
        } => {
            let a = line_direction(d, m)?;
            let b = Vector3::from(axis.direction());
            let actual = a.cross(&b).norm().atan2(a.dot(&b));
            Ok((
                vec![
                    (actual - desired(doc, owner, value, Dimension::Angle, false)?) * ANGULAR_SCALE,
                ],
                Some(actual),
            ))
        }
        Relation::AlignAxis { member: m, axis } => Ok((
            components(
                line_direction(d, m)?.cross(&Vector3::from(axis.direction())) * ANGULAR_SCALE,
            ),
            None,
        )),
        Relation::Parallel { first, second } => Ok((
            components(
                line_direction(d, first)?.cross(&line_direction(d, second)?) * ANGULAR_SCALE,
            ),
            None,
        )),
        Relation::Perpendicular { first, second } => Ok((
            vec![line_direction(d, first)?.dot(&line_direction(d, second)?) * ANGULAR_SCALE],
            None,
        )),
        Relation::EqualLength { first, second } => Ok((
            vec![member_length(doc, owner, d, first)? - member_length(doc, owner, d, second)?],
            None,
        )),
        Relation::OnLine {
            point: p,
            line,
            bounded,
        } => {
            let Curve::Line { start, end } = member(d, line)? else {
                return Err(invalid("Point-on-line requires a line"));
            };
            let a = point(d, start)?;
            let delta = point(d, end)? - a;
            if delta.norm() <= EPS {
                return Err(invalid("Point-on-line needs a noncollapsed line"));
            }
            let t = (point(d, p)? - a).dot(&delta) / delta.norm_squared();
            let t = if *bounded { t.clamp(0., 1.) } else { t };
            Ok((components(point(d, p)? - a - delta * t), None))
        }
        Relation::PointLineDistance {
            point: p,
            line,
            value,
        } => {
            let Curve::Line { start, .. } = member(d, line)? else {
                return Err(invalid("Distance requires a straight member"));
            };
            let direction = line_direction(d, line)?;
            let offset = point(d, p)? - point(d, start)?;
            let delta = offset - direction * offset.dot(&direction);
            let actual = delta.norm();
            let target = desired(doc, owner, value, Dimension::Length, false)?;
            Ok((
                if target == 0. {
                    components(delta)
                } else {
                    vec![actual - target]
                },
                Some(actual),
            ))
        }
        Relation::OnPlane { point: p, plane } => {
            let n = Vector3::from(plane.u).cross(&Vector3::from(plane.v));
            Ok((
                vec![(point(d, p)? - Vector3::from(plane.origin)).dot(&n)],
                None,
            ))
        }
        Relation::Coplanar { points, plane } => {
            let n = Vector3::from(plane.u).cross(&Vector3::from(plane.v));
            Ok((
                points
                    .iter()
                    .map(|p| Ok((point(d, p)? - Vector3::from(plane.origin)).dot(&n)))
                    .collect::<Result<Vec<_>>>()?,
                None,
            ))
        }
        Relation::PointOnCurve {
            point: p,
            member: m,
            parameter: t,
            ..
        } => Ok((
            components(
                point(d, p)?
                    - Vector3::from(curve_point(
                        doc,
                        owner,
                        d,
                        member(d, m)?,
                        parameter(doc, owner, t)?,
                    )?),
            ),
            None,
        )),
        Relation::Tangent {
            first,
            second,
            first_parameter,
            second_parameter,
        } => {
            regular_contact(
                doc,
                owner,
                d,
                member(d, first)?,
                parameter(doc, owner, first_parameter)?,
            )?;
            regular_contact(
                doc,
                owner,
                d,
                member(d, second)?,
                parameter(doc, owner, second_parameter)?,
            )?;
            let a = curve_jet(
                doc,
                owner,
                d,
                member(d, first)?,
                parameter(doc, owner, first_parameter)?,
            )?;
            let b = curve_jet(
                doc,
                owner,
                d,
                member(d, second)?,
                parameter(doc, owner, second_parameter)?,
            )?;
            let ta = Vector3::from(a.tangent);
            let tb = Vector3::from(b.tangent);
            if ta.norm() <= EPS || tb.norm() <= EPS {
                return Err(invalid(
                    "Tangency requires regular nonzero curve derivatives",
                ));
            }
            let mut equations = components(Vector3::from(a.point) - Vector3::from(b.point));
            equations.extend(components(
                ta.normalize().cross(&tb.normalize()) * ANGULAR_SCALE,
            ));
            Ok((equations, None))
        }
    }
}
pub fn constraint_residuals(
    doc: &Document,
    owner: &str,
    d: &Definition,
) -> Result<Vec<ConstraintResidual>> {
    d.constraints
        .iter()
        .map(|c| {
            let (values, measured) = relation_residual(doc, owner, d, &c.relation)?;
            Ok(ConstraintResidual {
                constraint: c.id.clone(),
                values,
                driven: c.driven,
                measured,
            })
        })
        .collect()
}
/// Expression ownership of a full circle's effective size. A follower seed is
/// not an independent size input while its Equal Radius relation is active.
#[derive(Clone, Debug, PartialEq)]
pub struct CircleSizeOwner {
    pub reference: Id,
    pub driving_constraint: Option<Id>,
    pub circumference: bool,
    pub effective_radius: Expr,
}
/// Resolve the same circle-size ownership used by native preparation without
/// evaluating expressions or modifying authored source. Presentation adapters
/// can expose the actual driving field instead of accepting dormant seed edits.
pub fn circle_size_owner(d: &Definition, member_id: &str) -> Result<CircleSizeOwner> {
    d.validate()?;
    if !matches!(member(d, member_id)?, Curve::Circle { .. }) {
        return Err(invalid("Circle size ownership requires a full circle"));
    }
    resolve_circle_size_owners(d)?
        .remove(member_id)
        .ok_or_else(|| invalid("Circle size ownership requires a full circle"))
}
/// Resolve all full-circle size fields in one source-validation and graph pass.
/// Presentation adapters should reuse this result while projecting multiple
/// selected circles rather than preparing the same source for each field.
pub fn circle_size_owners(d: &Definition) -> Result<BTreeMap<Id, CircleSizeOwner>> {
    d.validate()?;
    resolve_circle_size_owners(d)
}
fn resolve_circle_size_owners(d: &Definition) -> Result<BTreeMap<Id, CircleSizeOwner>> {
    let (prepared, owners) = prepare_circle_sizes(d)?;
    let drivers = d
        .constraints
        .iter()
        .filter(|c| !c.driven)
        .filter_map(|c| match &c.relation {
            Relation::Radius { member, .. } => Some((member.clone(), (c.id.clone(), false))),
            Relation::Length { member, .. } => Some((member.clone(), (c.id.clone(), true))),
            _ => None,
        })
        .collect::<BTreeMap<_, _>>();
    owners
        .into_iter()
        .map(|(member_id, reference)| {
            let driver = drivers.get(&reference);
            let Curve::Circle { radius, .. } = member(&prepared, &member_id)? else {
                unreachable!()
            };
            Ok((
                member_id,
                CircleSizeOwner {
                    reference,
                    driving_constraint: driver.map(|(id, _)| id.clone()),
                    circumference: driver.is_some_and(|(_, circumference)| *circumference),
                    effective_radius: radius.clone(),
                },
            ))
        })
        .collect()
}
fn prepare(d: &Definition) -> Result<Definition> {
    prepare_circle_sizes(d).map(|(definition, _)| definition)
}
fn prepare_circle_sizes(d: &Definition) -> Result<(Definition, BTreeMap<Id, Id>)> {
    let mut result = d.clone();
    let mut sizes = BTreeSet::new();
    for c in d.constraints.iter().filter(|c| !c.driven) {
        let (member, value, circumference) = match &c.relation {
            Relation::Radius { member, value } => (member, value, false),
            Relation::Length { member, value } => (member, value, true),
            _ => continue,
        };
        if let Some(Member {
            curve: Curve::Circle { radius, .. },
            ..
        }) = result.members.get_mut(member)
        {
            if !sizes.insert(member) {
                return Err(invalid(
                    "A circle can have one driving size dimension: radius or circumference",
                ));
            }
            *radius = if circumference {
                Expr(format!("({}) / (2 * pi)", value.0))
            } else {
                value.clone()
            };
        }
    }
    let owners = prepare_equal_circles(d, &mut result, &sizes)?;
    Ok((result, owners))
}
/// Full-circle radius is expression intent, not a point-coordinate variable.
/// Reduce directed equalities before solving the positional equations so an
/// undriven circle follows the first selected circle's authored expression.
fn prepare_equal_circles(
    source: &Definition,
    result: &mut Definition,
    sizes: &BTreeSet<&Id>,
) -> Result<BTreeMap<Id, Id>> {
    let circle = |id: &Id| matches!(member(source, id), Ok(Curve::Circle { .. }));
    let mut owners = source
        .members
        .keys()
        .filter(|id| circle(id))
        .map(|id| (id.clone(), id.clone()))
        .collect::<BTreeMap<_, _>>();
    let mut links = BTreeMap::<Id, BTreeSet<Id>>::new();
    let mut outgoing = BTreeMap::<Id, BTreeSet<Id>>::new();
    let mut incoming = BTreeMap::<Id, usize>::new();
    for constraint in source.constraints.iter().filter(|c| !c.driven) {
        let Relation::EqualLength { first, second } = &constraint.relation else {
            continue;
        };
        if first == second {
            return Err(invalid("Equal requires two different members"));
        }
        // Lines and three-point arcs have real positional size freedom and
        // continue through the native residual. Unsupported spline length
        // equations are diagnosed by member_length rather than approximated.
        if !circle(first) || !circle(second) {
            continue;
        }
        links
            .entry(first.clone())
            .or_default()
            .insert(second.clone());
        links
            .entry(second.clone())
            .or_default()
            .insert(first.clone());
        incoming.entry(first.clone()).or_default();
        if outgoing
            .entry(first.clone())
            .or_default()
            .insert(second.clone())
        {
            *incoming.entry(second.clone()).or_default() += 1;
        }
    }
    let mut visited = BTreeSet::<Id>::new();
    for start in links.keys() {
        if visited.contains(start) {
            continue;
        }
        let mut group = BTreeSet::<Id>::new();
        let mut pending = vec![start.clone()];
        while let Some(id) = pending.pop() {
            if !group.insert(id.clone()) {
                continue;
            }
            pending.extend(links[&id].iter().cloned());
        }
        visited.extend(group.iter().cloned());
        let roots = group
            .iter()
            .filter(|id| incoming[*id] == 0)
            .cloned()
            .collect::<Vec<_>>();
        let mut counts = incoming.clone();
        let mut ready = roots.clone();
        let mut processed = 0;
        while let Some(id) = ready.pop() {
            processed += 1;
            if let Some(targets) = outgoing.get(&id) {
                for target in targets {
                    let count = counts.get_mut(target).unwrap();
                    *count -= 1;
                    if *count == 0 {
                        ready.push(target.clone());
                    }
                }
            }
        }
        if processed != group.len() {
            return Err(invalid(
                "Equal Radius has a circular reference; choose one reference circle and make the others equal to it",
            ));
        }
        let drivers = group
            .iter()
            .filter(|id| sizes.contains(*id))
            .collect::<Vec<_>>();
        if drivers.len() > 1 {
            return Err(invalid(
                "An Equal Radius group can have one driving size dimension; remove additional radius/circumference dimensions or make them driven",
            ));
        }
        let reference = if let Some(driver) = drivers.first() {
            *driver
        } else if roots.len() == 1 {
            &roots[0]
        } else {
            return Err(invalid(
                "Equal Radius has competing reference circles; choose one reference circle or add one driving size dimension to the group",
            ));
        };
        let reference = reference.clone();
        let Curve::Circle { radius, .. } = member(result, &reference)? else {
            unreachable!()
        };
        let radius = radius.clone();
        for id in group {
            owners.insert(id.clone(), reference.clone());
            let Curve::Circle { radius: target, .. } =
                &mut result.members.get_mut(&id).unwrap().curve
            else {
                unreachable!()
            };
            *target = radius.clone();
        }
    }
    Ok(owners)
}
struct System<'a> {
    doc: &'a Document,
    owner: &'a str,
    base: Definition,
    keys: Vec<Id>,
    contacts: Vec<Id>,
    seed: DVector<f64>,
    work: std::cell::Cell<usize>,
    work_per_evaluation: usize,
}
impl<'a> System<'a> {
    fn new(doc: &'a Document, owner: &'a str, d: &Definition) -> Result<Self> {
        d.validate()?;
        let base = prepare(d)?;
        let keys = d.points.keys().cloned().collect::<Vec<_>>();
        let contacts = d
            .constraints
            .iter()
            .filter(|c| {
                !c.driven && matches!(c.relation, Relation::PointOnCurve { free: true, .. })
            })
            .map(|c| c.id.clone())
            .collect::<Vec<_>>();
        let mut values = keys.iter().flat_map(|id| d.points[id]).collect::<Vec<_>>();
        for id in &contacts {
            let Relation::PointOnCurve { parameter: t, .. } =
                &d.constraints.iter().find(|c| &c.id == id).unwrap().relation
            else {
                unreachable!()
            };
            values.push(parameter(doc, owner, t)?);
        }
        if values.len() > MAX_POINTS * 3 + 128 {
            return Err(unsupported(
                "Spatial solve exceeds its bounded variable count",
            ));
        }
        let fit_work = base
            .members
            .values()
            .filter_map(|m| {
                if let Curve::InterpolatingSpline { points, .. } = &m.curve {
                    Some(points.len().pow(3))
                } else {
                    None
                }
            })
            .sum::<usize>();
        Ok(Self {
            doc,
            owner,
            base,
            keys,
            contacts,
            seed: DVector::from_vec(values),
            work: std::cell::Cell::new(0),
            work_per_evaluation: (d.points.len() * 3 + d.constraints.len() * 6 + fit_work).max(1),
        })
    }
    fn derive(&self, x: &DVector<f64>) -> Result<Definition> {
        if x.len() != self.seed.len() || x.iter().any(|v| !v.is_finite()) {
            return Err(invalid("Spatial solve vector is inconsistent or nonfinite"));
        }
        let mut result = self.base.clone();
        for (i, id) in self.keys.iter().enumerate() {
            let p = [x[3 * i], x[3 * i + 1], x[3 * i + 2]];
            if p.iter().any(|v| v.abs() > 1e9) {
                return Err(invalid("Spatial coordinates exceed the source bound"));
            }
            result.points.insert(id.clone(), p);
        }
        for (i, id) in self.contacts.iter().enumerate() {
            let value = x[self.keys.len() * 3 + i];
            if !(0.0..=1.0).contains(&value) {
                return Err(invalid("A solved contact leaves its bounded curve"));
            }
            let c = result.constraints.iter_mut().find(|c| &c.id == id).unwrap();
            if let Relation::PointOnCurve { parameter, .. } = &mut c.relation {
                parameter.0 = value.to_string();
            }
        }
        Ok(result)
    }
    fn residuals(&self, x: &DVector<f64>) -> Result<DVector<f64>> {
        let work = self.work.get().saturating_add(self.work_per_evaluation);
        self.work.set(work);
        if work > 64_000_000 {
            return Err(unsupported(
                "Spatial constraint solve exhausted its shared work budget",
            ));
        }
        let candidate = self.derive(x)?;
        let values = candidate
            .constraints
            .iter()
            .filter(|c| !c.driven)
            .map(|c| {
                relation_residual(self.doc, self.owner, &candidate, &c.relation)
                    .map(|(values, _)| values)
            })
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();
        if values.len() > 4096 {
            return Err(unsupported(
                "Spatial constraint equations exceed their row bound",
            ));
        }
        Ok(DVector::from_vec(values))
    }
}
fn finish(
    doc: &Document,
    owner: &str,
    mut candidate: Definition,
    mut report: crate::solver::SolveReport,
) -> Result<(Definition, crate::solver::SolveReport)> {
    if !report.converged {
        return Err(invalid(format!(
            "Spatial constraints did not converge; check conflicting dimensions or degenerate members (maximum scaled residual {:.6})",
            report.residual
        )));
    }
    let aliases = endpoint_aliases(doc, owner, &candidate)?;
    let positions = aliases
        .values()
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .map(|endpoint| {
            endpoint_position(doc, owner, &candidate, &endpoint)
                .map(|position| (endpoint, position))
        })
        .collect::<Result<BTreeMap<_, _>>>()?;
    for (endpoint, canonical) in &aliases {
        if let Endpoint::Point { point } = endpoint {
            candidate.points.insert(point.clone(), positions[canonical]);
        }
    }
    for (endpoint, canonical) in &aliases {
        let actual = Vector3::from(endpoint_position(doc, owner, &candidate, endpoint)?);
        let shared = Vector3::from(endpoint_position(doc, owner, &candidate, canonical)?);
        if (actual - shared).norm() > EPS {
            return Err(unsupported(
                "Endpoint tangency cannot establish shared native topology within the 1e-7 mm boundary tolerance",
            ));
        }
    }
    let mut maximum = 0_f64;
    for c in candidate.constraints.iter().filter(|c| !c.driven) {
        let (values, _) = relation_residual(doc, owner, &candidate, &c.relation)?;
        maximum = values
            .iter()
            .fold(maximum, |max, value| max.max(value.abs()));
    }
    if maximum > 1e-6 {
        return Err(invalid(
            "Exact coincident topology conflicts with the solved spatial dimensions",
        ));
    }
    report.residual = maximum;
    validate_geometry(doc, owner, &candidate)?;
    Ok((candidate, report))
}
pub fn solve(
    doc: &Document,
    owner: &str,
    d: &mut Definition,
) -> Result<crate::solver::SolveReport> {
    let system = System::new(doc, owner, d)?;
    let (x, report) = crate::solver::minimize(system.seed.clone(), |x| system.residuals(x), 100)?;
    let (candidate, report) = finish(doc, owner, system.derive(&x)?, report)?;
    *d = candidate;
    Ok(report)
}
/// Cursor goals are soft; native engineering equations are projected and then
/// strictly revalidated before any caller can publish the returned source.
pub fn drag(
    doc: &Document,
    owner: &str,
    d: &mut Definition,
    targets: &[DragTarget],
) -> Result<crate::solver::SolveReport> {
    if targets.is_empty()
        || targets.len() > 64
        || targets
            .iter()
            .any(|t| t.position.iter().any(|x| !x.is_finite() || x.abs() > 1e9))
        || targets
            .iter()
            .map(|t| &t.point)
            .collect::<BTreeSet<_>>()
            .len()
            != targets.len()
    {
        return Err(invalid("Drag needs 1–64 distinct finite spatial targets"));
    }
    let mut source = d.clone();
    solve(doc, owner, &mut source)?;
    let system = System::new(doc, owner, &source)?;
    let index = system
        .keys
        .iter()
        .enumerate()
        .map(|(i, id)| (id.clone(), i))
        .collect::<BTreeMap<_, _>>();
    if targets.iter().any(|t| !index.contains_key(&t.point)) {
        return Err(invalid("Dragged spatial point is missing"));
    }
    let equations = |x: &DVector<f64>| -> Result<DVector<f64>> {
        let mut r = system
            .residuals(x)?
            .as_slice()
            .iter()
            .map(|v| v * 10_000.)
            .collect::<Vec<_>>();
        for target in targets {
            let i = index[&target.point];
            r.extend((0..3).map(|axis| x[i * 3 + axis] - target.position[axis]));
        }
        r.extend(
            x.iter()
                .zip(system.seed.iter())
                .map(|(a, b)| (a - b) * 0.0001),
        );
        Ok(DVector::from_vec(r))
    };
    let (x, _) = crate::solver::minimize(system.seed.clone(), equations, 60)?;
    let mut candidate = system.derive(&x)?;
    let report = solve(doc, owner, &mut candidate)?;
    *d = candidate;
    Ok(report)
}
/// Local infinitesimal point freedom from the same current native equations.
/// This does not claim finite reachability or a minimal conflict set.
pub fn point_motion(
    doc: &Document,
    owner: &str,
    d: &Definition,
) -> Result<BTreeMap<Id, Vec<[f64; 3]>>> {
    let system = System::new(doc, owner, d)?;
    let r = system.residuals(&system.seed)?;
    if r.iter().any(|x| x.abs() > 1e-6) {
        return Err(invalid(
            "Motion analysis needs satisfied current constraints",
        ));
    }
    let j = crate::solver::jacobian(&system.seed, &r, &|x| system.residuals(x))?;
    let (scaled, scale) = crate::solver::scaled_jacobian(&j);
    let mut projection = DMatrix::identity(system.seed.len(), system.seed.len());
    if !r.is_empty() && !system.seed.is_empty() {
        let svd = scaled.svd(false, true);
        let cutoff = (svd.singular_values.amax() * 1e-8).max(1e-12);
        let vt = svd.v_t.unwrap();
        for i in 0..svd.singular_values.len() {
            if svd.singular_values[i] > cutoff {
                let v = vt.row(i);
                projection -= v.transpose() * v;
            }
        }
    }
    for i in 0..projection.nrows() {
        projection.row_mut(i).scale_mut(1. / scale[i]);
    }
    for col in 0..projection.ncols() {
        let norm = projection.column(col).norm();
        if norm > 1e-10 {
            projection.column_mut(col).scale_mut(1. / norm);
        } else {
            projection.column_mut(col).fill(0.);
        }
    }
    let mut result = BTreeMap::new();
    for (i, id) in system.keys.iter().enumerate() {
        let block = projection.rows(i * 3, 3);
        let eigen = (block * block.transpose()).symmetric_eigen();
        let cutoff = eigen.eigenvalues.amax().max(1.) * 1e-8;
        let directions = (0..3)
            .filter(|axis| eigen.eigenvalues[*axis] > cutoff)
            .map(|axis| std::array::from_fn(|j| eigen.eigenvectors[(j, axis)]))
            .collect();
        result.insert(id.clone(), directions);
    }
    Ok(result)
}
