//! Exact ruled strips and corresponding-position mid-surfaces. Native carriers,
//! source construction ancestry and bounded Bernstein proofs remain authority.
use crate::adapter::{MonstertruckAdapter, Shape, TopologyHistory};
use cad_core::{
    CadError, Dimension, Document, NodeRef, Result, TopologyRef, expression,
    reference_curves::Segment,
    reference_geometry::DirectionSource,
    surface::{FaceSource, PatchCorrespondence, SurfaceOperation},
};
use monstertruck::{geometry::prelude::TryIntoHomogeneousBsplineCurve, modeling::*};
use std::collections::{BTreeMap, BTreeSet, HashSet};

const EPS: f64 = 1e-7;
const MAX_FACES: usize = 4096;
const MAX_CARRIER_DEGREE: usize = 16;
const MAX_PROOF_TERMS: usize = 8_000_000;
const MAX_OPERATION_PROOF_TERMS: usize = 32_000_000;
fn invalid(message: &str) -> CadError {
    CadError::Invalid(format!("Surface construction: {message}"))
}
fn unsupported(message: &str) -> CadError {
    CadError::Unsupported(format!("Surface construction: {message}"))
}
fn finite(p: Point3) -> bool {
    [p.x, p.y, p.z].into_iter().all(f64::is_finite)
}
fn unit(v: Vector3) -> Result<Vector3> {
    let length = v.magnitude();
    if ![v.x, v.y, v.z, length].into_iter().all(f64::is_finite) || length <= 1e-12 {
        return Err(invalid("A finite nonzero direction is required"));
    }
    Ok(v / length)
}
fn point(h: Vector4) -> Result<Point3> {
    if ![h.x, h.y, h.z, h.w].into_iter().all(f64::is_finite) || h.w <= 0. {
        return Err(unsupported(
            "Finite positive rational controls are required",
        ));
    }
    let p = Point3::new(h.x / h.w, h.y / h.w, h.z / h.w);
    if !finite(p) {
        return Err(invalid("Rational coordinates exceed finite values"));
    }
    Ok(p)
}

// Scalar Bernstein polynomials in two variables. Products retain their exact
// basis rather than inferring regularity from sampled normals.
#[derive(Clone)]
struct Polynomial {
    u: usize,
    v: usize,
    coefficients: Vec<Vec<f64>>,
    majorant: f64,
}
impl Polynomial {
    fn from_net(net: &[Vec<Vector4>], component: usize) -> Self {
        Self {
            u: net.len() - 1,
            v: net[0].len() - 1,
            coefficients: net
                .iter()
                .map(|row| row.iter().map(|h| h[component]).collect())
                .collect(),
            majorant: net
                .iter()
                .flatten()
                .map(|h| h[component].abs())
                .fold(0., f64::max),
        }
    }
    fn derivative(&self, along_u: bool, span: f64) -> Self {
        let (u, v) = if along_u {
            (self.u - 1, self.v)
        } else {
            (self.u, self.v - 1)
        };
        let coefficients = (0..=u)
            .map(|i| {
                (0..=v)
                    .map(|j| {
                        if along_u {
                            (self.coefficients[i + 1][j] - self.coefficients[i][j]) * self.u as f64
                                / span
                        } else {
                            (self.coefficients[i][j + 1] - self.coefficients[i][j]) * self.v as f64
                                / span
                        }
                    })
                    .collect()
            })
            .collect();
        Self {
            u,
            v,
            coefficients,
            majorant: self.majorant
                * 2.
                * if along_u {
                    self.u as f64 / span
                } else {
                    self.v as f64 / span
                },
        }
    }
    fn elevate(&self, along_u: bool) -> Self {
        let (u, v) = if along_u {
            (self.u + 1, self.v)
        } else {
            (self.u, self.v + 1)
        };
        let coefficients = (0..=u)
            .map(|i| {
                (0..=v)
                    .map(|j| {
                        let (k, degree) = if along_u { (i, u) } else { (j, v) };
                        let alpha = k as f64 / degree as f64;
                        let left = if k == 0 {
                            0.
                        } else if along_u {
                            self.coefficients[i - 1][j]
                        } else {
                            self.coefficients[i][j - 1]
                        };
                        let right = if k == degree {
                            0.
                        } else {
                            self.coefficients[i][j]
                        };
                        alpha * left + (1. - alpha) * right
                    })
                    .collect()
            })
            .collect();
        Self {
            u,
            v,
            coefficients,
            majorant: self.majorant,
        }
    }
    fn subtract(mut self, other: &Self) -> Self {
        for (i, row) in self.coefficients.iter_mut().enumerate() {
            for (j, value) in row.iter_mut().enumerate() {
                *value -= other.coefficients[i][j];
            }
        }
        self.majorant += other.majorant;
        self
    }
    fn positive(&self) -> bool {
        let scale = self
            .coefficients
            .iter()
            .flatten()
            .map(|x| x.abs())
            .fold(0., f64::max);
        scale.is_finite()
            && scale > 0.
            && self.majorant.is_finite()
            && self
                .coefficients
                .iter()
                .flatten()
                .all(|x| x.is_finite() && *x > self.majorant.max(scale) * 1e-10)
    }

    fn split(&self, along_u: bool) -> (Self, Self) {
        let mut left = self.clone();
        let mut right = self.clone();
        let degree = if along_u { self.u } else { self.v };
        let count = if along_u { self.v + 1 } else { self.u + 1 };
        for index in 0..count {
            let mut controls = (0..=degree)
                .map(|i| {
                    if along_u {
                        self.coefficients[i][index]
                    } else {
                        self.coefficients[index][i]
                    }
                })
                .collect::<Vec<_>>();
            for level in 0..=degree {
                if along_u {
                    left.coefficients[level][index] = controls[0];
                    right.coefficients[degree - level][index] = controls[degree - level];
                } else {
                    left.coefficients[index][level] = controls[0];
                    right.coefficients[index][degree - level] = controls[degree - level];
                }
                for i in 0..degree - level {
                    controls[i] = controls[i] * 0.5 + controls[i + 1] * 0.5;
                }
            }
        }
        // Keep the original cancellation majorant: refinement cannot turn
        // rounding uncertainty into a positive certificate.
        (left, right)
    }

    fn positive_bounded(&self, budget: &mut usize, operation_work: &mut usize) -> Result<bool> {
        let mut pending = vec![(self.clone(), 0)];
        let mut cells = 0;
        while let Some((polynomial, depth)) = pending.pop() {
            cells += 1;
            if polynomial.positive() {
                continue;
            }
            let corner_margin = polynomial.majorant * 1e-10;
            if depth >= 12
                || cells >= 4096
                || [
                    polynomial.coefficients[0][0],
                    polynomial.coefficients[polynomial.u][0],
                    polynomial.coefficients[0][polynomial.v],
                    polynomial.coefficients[polynomial.u][polynomial.v],
                ]
                .iter()
                .any(|x| !x.is_finite() || *x <= corner_margin)
            {
                return Ok(false);
            }
            let along_u = polynomial.v == 0 || (polynomial.u > 0 && depth % 2 == 0);
            let amount = (polynomial.u + 1)
                .checked_mul(polynomial.v + 1)
                .and_then(|n| {
                    n.checked_mul(if along_u {
                        polynomial.u + 1
                    } else {
                        polynomial.v + 1
                    })
                })
                .and_then(|n| n.checked_mul(2))
                .ok_or_else(|| unsupported("The rational subdivision budget overflowed"))?;
            charge_proof(budget, operation_work, amount)?;
            let (left, right) = polynomial.split(along_u);
            pending.push((right, depth + 1));
            pending.push((left, depth + 1));
        }
        Ok(true)
    }
}
fn binomial(n: usize, k: usize) -> f64 {
    (1..=k.min(n - k)).fold(1., |value, i| value * (n + 1 - i) as f64 / i as f64)
}
fn charge_proof(budget: &mut usize, operation_work: &mut usize, terms: usize) -> Result<()> {
    *budget = budget
        .checked_add(terms)
        .ok_or_else(|| unsupported("The rational proof budget overflowed"))?;
    if *budget > MAX_PROOF_TERMS {
        return Err(unsupported(
            "The rational regularity proof exceeds eight million bounded coefficient operations",
        ));
    }
    *operation_work = operation_work
        .checked_add(terms)
        .ok_or_else(|| unsupported("The operation proof budget overflowed"))?;
    if *operation_work > MAX_OPERATION_PROOF_TERMS {
        return Err(unsupported(
            "Surface construction exceeds 32 million coefficient operations across the complete operation",
        ));
    }
    Ok(())
}
fn product(
    a: &Polynomial,
    b: &Polynomial,
    budget: &mut usize,
    operation_work: &mut usize,
) -> Result<Polynomial> {
    let terms = (a.u + 1)
        .checked_mul(a.v + 1)
        .and_then(|n| n.checked_mul(b.u + 1))
        .and_then(|n| n.checked_mul(b.v + 1))
        .ok_or_else(|| unsupported("The rational proof budget overflowed"))?;
    charge_proof(budget, operation_work, terms)?;
    let (u, v) = (a.u + b.u, a.v + b.v);
    charge_proof(
        budget,
        operation_work,
        (a.u + 1) * (b.u + 1) + (a.v + 1) * (b.v + 1),
    )?;
    // Bernstein product weights depend only on the two degrees and indices.
    // Reusing them keeps high-degree native proofs within their existing work
    // budgets without repeating binomial arithmetic for every coefficient.
    let u_weights = (0..=a.u)
        .map(|i| {
            (0..=b.u)
                .map(|k| binomial(a.u, i) * binomial(b.u, k) / binomial(u, i + k))
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let v_weights = (0..=a.v)
        .map(|j| {
            (0..=b.v)
                .map(|l| binomial(a.v, j) * binomial(b.v, l) / binomial(v, j + l))
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let mut result = Polynomial {
        u,
        v,
        coefficients: vec![vec![0.; v + 1]; u + 1],
        majorant: a.majorant * b.majorant,
    };
    for (i, u_weights) in u_weights.iter().enumerate() {
        for (j, v_weights) in v_weights.iter().enumerate() {
            for (k, u_weight) in u_weights.iter().enumerate() {
                for (l, v_weight) in v_weights.iter().enumerate() {
                    let weight = u_weight * v_weight;
                    result.coefficients[i + k][j + l] +=
                        weight * a.coefficients[i][j] * b.coefficients[k][l];
                }
            }
        }
    }
    if !result.majorant.is_finite() || result.coefficients.iter().flatten().any(|x| !x.is_finite())
    {
        return Err(unsupported(
            "The rational regularity proof has nonfinite coefficients or cancellation bounds",
        ));
    }
    Ok(result)
}
fn projection(components: &[Polynomial], axis: Vector3) -> Polynomial {
    let mut result = components[0].clone();
    for (i, row) in result.coefficients.iter_mut().enumerate() {
        for (j, value) in row.iter_mut().enumerate() {
            *value = (0..3)
                .map(|k| components[k].coefficients[i][j] * axis[k])
                .sum();
        }
    }
    result.majorant = (0..3).map(|k| components[k].majorant * axis[k].abs()).sum();
    result
}
fn inner_knots(knots: &KnotVector, range: (f64, f64)) -> Vec<f64> {
    let mut result = knots
        .iter()
        .copied()
        .filter(|k| *k > range.0 && *k < range.1)
        .collect::<Vec<_>>();
    result.sort_by(f64::total_cmp);
    result.dedup();
    result
}
fn continuous_knots(knots: &KnotVector, range: (f64, f64), degree: usize) -> Result<()> {
    let mut previous = None;
    let mut multiplicity = 0;
    for knot in knots.iter().copied() {
        if !knot.is_finite() {
            return Err(invalid("Native carrier knots must be finite"));
        }
        if previous == Some(knot) {
            multiplicity += 1;
        } else {
            previous = Some(knot);
            multiplicity = 1;
        }
        if knot > range.0 && knot < range.1 && multiplicity > degree {
            return Err(unsupported(
                "The carrier must be position-continuous across every interior knot; discontinuous spans cannot establish global injectivity",
            ));
        }
    }
    Ok(())
}
pub(crate) fn prove_regular(
    surface: &NurbsSurface<Vector4>,
    operation_work: &mut usize,
) -> Result<()> {
    prove_injective_projection(surface, None, operation_work)
}

pub(crate) fn prove_projection(
    surface: &NurbsSurface<Vector4>,
    u: Vector3,
    v: Vector3,
    operation_work: &mut usize,
) -> Result<()> {
    prove_injective_projection(surface, Some((u, v)), operation_work)
}

fn prove_injective_projection(
    surface: &NurbsSurface<Vector4>,
    frame: Option<(Vector3, Vector3)>,
    operation_work: &mut usize,
) -> Result<()> {
    let rows = surface.control_points();
    let (degree_u, degree_v) = (
        surface.knot_vector_u().len() - rows.len() - 1,
        surface.knot_vector_v().len() - rows[0].len() - 1,
    );
    if !(1..=MAX_CARRIER_DEGREE).contains(&degree_u)
        || !(1..=MAX_CARRIER_DEGREE).contains(&degree_v)
    {
        return Err(unsupported(
            "Surface regularity proof supports degrees one through sixteen",
        ));
    }
    let ((u0, u1), (v0, v1)) = surface.range_tuple();
    continuous_knots(surface.knot_vector_u(), (u0, u1), degree_u)?;
    continuous_knots(surface.knot_vector_v(), (v0, v1), degree_v)?;
    let (u, v) = if let Some(frame) = frame {
        frame
    } else {
        let du = surface.uder(u0 + (u1 - u0) * 0.5, v0 + (v1 - v0) * 0.5);
        let dv = surface.vder(u0 + (u1 - u0) * 0.5, v0 + (v1 - v0) * 0.5);
        let a = unit(du)?;
        let b = unit(dv)?;
        let cosine = a.dot(b);
        let denominator = 1. - cosine * cosine;
        if !denominator.is_finite() || denominator <= 1e-12 {
            return Err(unsupported(
                "The source patch has a singular or ill-conditioned tangent frame",
            ));
        }
        // Choosing the seed frame is not the proof. The same fixed axes
        // must certify the complete carrier, including every knot span.
        (
            (a - b * cosine) / (du.magnitude() * denominator),
            (b - a * cosine) / (dv.magnitude() * denominator),
        )
    };
    if ![u.x, u.y, u.z, v.x, v.y, v.z]
        .into_iter()
        .all(f64::is_finite)
    {
        return Err(unsupported(
            "The rational proof frame exceeds finite coordinates",
        ));
    }
    let us = inner_knots(surface.knot_vector_u(), (u0, u1));
    let vs = inner_knots(surface.knot_vector_v(), (v0, v1));
    // The P-matrix global-univalence theorem requires differentiability.
    // A C0 knot join keeps the original convex SPD certificate instead.
    let differentiable = us.iter().all(|knot| {
        surface
            .knot_vector_u()
            .iter()
            .filter(|k| *k == knot)
            .count()
            < degree_u
    }) && vs.iter().all(|knot| {
        surface
            .knot_vector_v()
            .iter()
            .filter(|k| *k == knot)
            .count()
            < degree_v
    });
    if (us.len() + 1)
        .checked_mul(vs.len() + 1)
        .is_none_or(|n| n > 256)
    {
        return Err(unsupported(
            "The regularity proof supports at most 256 native Bezier patches",
        ));
    }
    let mut remainder = surface.clone();
    let mut strips = Vec::new();
    for knot in us {
        let right = remainder.cut_u(knot);
        strips.push(remainder);
        remainder = right;
    }
    strips.push(remainder);
    let mut budget = 0;
    for mut strip in strips {
        let mut patches = Vec::new();
        for knot in &vs {
            let right = strip.cut_v(*knot);
            patches.push(strip);
            strip = right;
        }
        patches.push(strip);
        for patch in patches {
            let rows = patch.control_points();
            if rows.len() != degree_u + 1 || rows.iter().any(|row| row.len() != degree_v + 1) {
                return Err(unsupported(
                    "The native patch has an unsupported Bezier knot structure",
                ));
            }
            let origin = point(rows[0][0])?;
            let mut scale: f64 = 0.;
            let mut weights: f64 = 0.;
            for h in rows.iter().flatten() {
                scale = scale.max((point(*h)? - origin).magnitude());
                weights = weights.max(h.w);
            }
            if !scale.is_finite() || scale <= 1e-12 || !weights.is_finite() || weights <= 0. {
                return Err(unsupported(
                    "A rational proof patch is collapsed or nonfinite",
                ));
            }
            let net = rows
                .iter()
                .map(|row| {
                    row.iter()
                        .map(|h| {
                            let p = (point(*h)? - origin) / scale;
                            let w = h.w / weights;
                            let result = Vector4::new(p.x * w, p.y * w, p.z * w, w);
                            point(result)?;
                            Ok(result)
                        })
                        .collect::<Result<Vec<_>>>()
                })
                .collect::<Result<Vec<_>>>()?;
            let weight = Polynomial::from_net(&net, 3);
            let ((a, b), (c, d)) = patch.range_tuple();
            let constant_weight = net.iter().flatten().all(|h| h.w == 1.);
            let weight_derivatives = (!constant_weight).then(|| {
                (
                    weight.derivative(true, b - a),
                    weight.derivative(false, d - c),
                )
            });
            let mut nu = Vec::new();
            let mut nv = Vec::new();
            for component in 0..3 {
                let position = Polynomial::from_net(&net, component);
                if constant_weight {
                    // A genuine constant denominator cancels exactly. Using
                    // polynomial derivatives avoids raising the proof degree
                    // and propagating artificial zero-weight cancellation.
                    nu.push(position.derivative(true, b - a).elevate(true));
                    nv.push(position.derivative(false, d - c).elevate(false));
                    continue;
                }
                let (duw, dvw) = weight_derivatives
                    .as_ref()
                    .ok_or_else(|| invalid("A rational proof lost its denominator derivatives"))?;
                nu.push(
                    product(
                        &position.derivative(true, b - a),
                        &weight,
                        &mut budget,
                        operation_work,
                    )?
                    .subtract(&product(&position, duw, &mut budget, operation_work)?)
                    .elevate(true),
                );
                nv.push(
                    product(
                        &position.derivative(false, d - c),
                        &weight,
                        &mut budget,
                        operation_work,
                    )?
                    .subtract(&product(&position, dvw, &mut budget, operation_work)?)
                    .elevate(false),
                );
            }
            let a = projection(&nu, u);
            let d = projection(&nv, v);
            let c = projection(&nu, v);
            let b = projection(&nv, u);
            let ad = product(&a, &d, &mut budget, operation_work)?;
            if differentiable {
                let determinant = ad.subtract(&product(&b, &c, &mut budget, operation_work)?);
                // Gale–Nikaido: all principal minors of one fixed projected
                // Jacobian are positive on a differentiable rectangle. This
                // proves whole-carrier injectivity without an SPD shear limit.
                if a.positive_bounded(&mut budget, operation_work)?
                    && d.positive_bounded(&mut budget, operation_work)?
                    && determinant.positive_bounded(&mut budget, operation_work)?
                {
                    continue;
                }
                return Err(unsupported(
                    "Could not certify an injective native patch within bounded rational refinement; this does not establish that the source is folded",
                ));
            }
            let mut off = c;
            let other = b;
            for (i, row) in off.coefficients.iter_mut().enumerate() {
                for (j, value) in row.iter_mut().enumerate() {
                    *value = (*value + other.coefficients[i][j]) * 0.5;
                }
            }
            off.majorant = (off.majorant + other.majorant) * 0.5;
            let determinant = ad.subtract(&product(&off, &off, &mut budget, operation_work)?);
            // The symmetric part of the projected Jacobian is positive
            // definite throughout the complete rectangular domain. This
            // establishes strict monotonicity and therefore injectivity,
            // including between patches, rather than only local regularity.
            if !a.positive_bounded(&mut budget, operation_work)?
                || !determinant.positive_bounded(&mut budget, operation_work)?
            {
                return Err(unsupported(
                    "Could not certify the complete nonsmooth carrier's injective projection within bounded rational refinement",
                ));
            }
        }
    }
    Ok(())
}

struct Context<'a> {
    adapter: &'a MonstertruckAdapter,
    doc: &'a Document,
    owner: &'a NodeRef,
    inputs: &'a BTreeMap<NodeRef, Shape>,
    history: &'a mut TopologyHistory,
    proof_work: usize,
}
pub(crate) fn operation(
    adapter: &MonstertruckAdapter,
    doc: &Document,
    owner: &NodeRef,
    operation: &SurfaceOperation,
    inputs: &BTreeMap<NodeRef, Shape>,
    history: &mut TopologyHistory,
) -> Result<Vec<Shell>> {
    let mut context = Context {
        adapter,
        doc,
        owner,
        inputs,
        history,
        proof_work: 0,
    };
    match operation {
        SurfaceOperation::Ruled {
            sources,
            direction,
            distance,
            reverse,
        } => ruled(&mut context, sources, direction, distance, *reverse),
        SurfaceOperation::MidSurface {
            faces,
            fraction,
            correspondence,
            reverse,
        } => mid_surface(&mut context, faces, fraction, correspondence, *reverse),
        _ => Err(invalid("Select Ruled Surface or Mid-Surface")),
    }
}
fn ruled(
    context: &mut Context<'_>,
    sources: &[Segment],
    direction: &DirectionSource,
    distance: &cad_core::Expr,
    reverse: bool,
) -> Result<Vec<Shell>> {
    let Context {
        adapter,
        doc,
        owner,
        inputs,
        history,
        proof_work,
    } = context;
    if sources.is_empty() || sources.len() > 256 {
        return Err(invalid(
            "Ruled Surface needs one through 256 explicit curve sources",
        ));
    }
    let mut ids = BTreeSet::new();
    if sources
        .iter()
        .any(|s| s.id.is_empty() || !ids.insert(&s.id))
    {
        return Err(invalid(
            "Ruled source record identities must be distinct and nonempty",
        ));
    }
    let distance = expression::value(doc, &owner.definition, distance, Dimension::Length)?;
    if !distance.is_finite() || distance.abs() <= 1e-9 {
        return Err(invalid("Ruled extension must be finite and nonzero"));
    }
    let delta = unit(Vector3::from(adapter.resolve_surface_direction(
        doc,
        &owner.definition,
        direction,
        inputs,
    )?))?
        * distance;
    if ![delta.x, delta.y, delta.z].into_iter().all(f64::is_finite) {
        return Err(invalid("Ruled extension exceeds finite coordinates"));
    }
    let mut shells = Vec::new();
    for source in sources {
        for (mut curve, reference) in
            crate::reference_curves::source(adapter, doc, &source.source, inputs)?
        {
            if source.reverse {
                curve.invert();
            }
            if !matches!(
                curve,
                Curve::Line(_) | Curve::BsplineCurve(_) | Curve::NurbsCurve(_)
            ) {
                return Err(unsupported(
                    "Ruled Surface requires exact explicit line/B-spline/NURBS curves; analytic parameter and implicit intersection carriers need exact extrusion support",
                ));
            }
            let mut curve = curve
                .try_into_homogeneous_bspline_curve()
                .ok_or_else(|| unsupported("The source has no exact rational curve carrier"))?;
            if curve.control_points().len() > 4096 {
                return Err(unsupported(
                    "One ruled source curve exceeds 4096 native controls",
                ));
            }
            curve.clamp();
            if !(1..=MAX_CARRIER_DEGREE).contains(&curve.degree()) {
                return Err(unsupported(
                    "Ruled Surface regularity supports native degrees one through sixteen",
                ));
            }
            let range = curve.range_tuple();
            continuous_knots(curve.knot_vector(), range, curve.degree())?;
            let knots = inner_knots(curve.knot_vector(), range);
            if shells.len() + knots.len() + 1 > MAX_FACES {
                return Err(unsupported(
                    "Ruled Surface exceeds 4096 native source spans",
                ));
            }
            let mut spans = Vec::new();
            for knot in knots {
                let right = curve.cut(knot);
                spans.push(curve);
                curve = right;
            }
            spans.push(curve);
            for span in spans {
                let native_range = span.range_tuple();
                let station = [
                    (native_range.0 - range.0) / (range.1 - range.0),
                    (native_range.1 - range.0) / (range.1 - range.0),
                ];
                if !station.into_iter().all(f64::is_finite)
                    || span.control_points().len() != span.degree() + 1
                {
                    return Err(unsupported(
                        "Ruled source spans need finite nonempty native parameter bounds",
                    ));
                }
                let net = span
                    .control_points()
                    .iter()
                    .map(|h| {
                        point(*h)?;
                        let end =
                            *h + Vector4::new(delta.x * h.w, delta.y * h.w, delta.z * h.w, 0.);
                        point(end)?;
                        Ok(vec![*h, end])
                    })
                    .collect::<Result<Vec<_>>>()?;
                let carrier = NurbsSurface::new(
                    BsplineSurface::try_new(
                        (
                            KnotVector::bezier_knot(span.degree()),
                            KnotVector::bezier_knot(1),
                        ),
                        net,
                    )
                    .map_err(|e| invalid(&e.to_string()))?,
                );
                prove_regular(&carrier, proof_work)?;
                let mut face = crate::surface_edit::patch_face(carrier)?;
                let purpose = serde_json::json!({"operation":"ruled_surface","source_record":source.id,"span":station});
                history.tool_face(owner, &face, std::slice::from_ref(&reference), &purpose)?;
                let wire = &face.absolute_boundaries()[0];
                for (index, edge) in wire.iter().enumerate() {
                    history.tool_edge(owner,edge,std::slice::from_ref(&reference),&serde_json::json!({"operation":"ruled_surface","source_record":source.id,"span":station,"boundary":(["source","end_ruling","translated","start_ruling"][index])}))?;
                }
                if reverse {
                    face.invert();
                }
                shells.push(vec![face].into());
            }
        }
    }
    Ok(shells)
}

fn equivalent(a: &Curve, b: &Curve) -> bool {
    if !matches!(
        a,
        Curve::Line(_) | Curve::BsplineCurve(_) | Curve::NurbsCurve(_)
    ) || !matches!(
        b,
        Curve::Line(_) | Curve::BsplineCurve(_) | Curve::NurbsCurve(_)
    ) {
        return false;
    }
    let (ar0, ar1) = a.range_tuple();
    let (br0, br1) = b.range_tuple();
    if ![ar0, ar1, br0, br1].into_iter().all(f64::is_finite)
        || ar1 <= ar0
        || br1 <= br0
        || ![a.subs(ar0), a.subs(ar1), b.subs(br0), b.subs(br1)]
            .into_iter()
            .all(finite)
    {
        return false;
    }
    let compare = |b: &Curve| {
        if crate::surfaces::same_curve(a, b, EPS) {
            return true;
        }
        let (a0, a1) = a.range_tuple();
        let (b0, b1) = b.range_tuple();
        crate::bounded_carriers::line(a).is_ok()
            && crate::bounded_carriers::line(b).is_ok()
            && (a.subs(a0) - b.subs(b0)).magnitude() <= EPS
            && (a.subs(a1) - b.subs(b1)).magnitude() <= EPS
    };
    compare(b) || compare(&b.inverse())
}
fn source_face(
    adapter: &MonstertruckAdapter,
    source: &FaceSource,
    inputs: &BTreeMap<NodeRef, Shape>,
) -> Result<Face> {
    let face = adapter.selected_boundary_face(&source.body, &source.reference, inputs)?;
    let actual = adapter.boundary_face_reference(&source.body, &face)?;
    if actual.producer != source.reference.producer || actual.role != source.reference.role {
        return Err(invalid(
            "A face source no longer has its authored construction identity",
        ));
    }
    Ok(face)
}
fn boundary_sources(
    adapter: &MonstertruckAdapter,
    source: &FaceSource,
    face: &Face,
    curves: &[Curve],
) -> Result<Vec<TopologyRef>> {
    let edges = face
        .absolute_boundaries()
        .iter()
        .flat_map(|w| w.iter())
        .collect::<Vec<_>>();
    let mut used = HashSet::new();
    let mut result = Vec::new();
    for curve in curves {
        let candidates = edges
            .iter()
            .filter(|edge| equivalent(curve, &edge.curve()))
            .collect::<Vec<_>>();
        if candidates.len() != 1 || !used.insert(candidates[0].id()) {
            return Err(unsupported(
                "Natural boundary correspondence must have one unique source edge per side",
            ));
        }
        result.push(adapter.boundary_edge_reference(&source.body, candidates[0])?);
    }
    Ok(result)
}
fn align_patch(
    mut patch: NurbsSurface<Vector4>,
    correspondence: &PatchCorrespondence,
) -> Result<NurbsSurface<Vector4>> {
    if correspondence.swap_uv {
        patch.swap_axes();
    }
    let mut knots = patch.knot_vectors().clone();
    let mut rows = patch.control_points().clone();
    if correspondence.reverse_u {
        knots.0.invert();
        rows.reverse();
    }
    if correspondence.reverse_v {
        knots.1.invert();
        for row in &mut rows {
            row.reverse();
        }
    }
    knots.0.normalize();
    knots.1.normalize();
    Ok(NurbsSurface::new(
        BsplineSurface::try_new(knots, rows).map_err(|e| invalid(&e.to_string()))?,
    ))
}
fn planar_mid(
    adapter: &MonstertruckAdapter,
    owner: &NodeRef,
    sources: &[FaceSource; 2],
    faces: &[Face; 2],
    fraction: f64,
    reverse: bool,
    history: &mut TopologyHistory,
) -> Result<Face> {
    let normal = unit(faces[0].oriented_surface().normal(0., 0.))?;
    let other = unit(faces[1].oriented_surface().normal(0., 0.))?;
    if normal.dot(other).abs() < 1. - 1e-10 {
        return Err(unsupported(
            "Planar Mid-Surface requires parallel source planes",
        ));
    }
    let distance = (faces[1].surface().subs(0., 0.) - faces[0].surface().subs(0., 0.)).dot(normal);
    if !distance.is_finite() || distance.abs() <= 1e-9 {
        return Err(invalid(
            "Mid-Surface source planes must have finite nonzero separation",
        ));
    }
    let delta = normal * distance;
    let translated = builder::translated(&faces[0], delta);
    let first = translated.absolute_boundaries();
    let second = faces[1].absolute_boundaries();
    if first.len() != second.len() {
        return Err(unsupported(
            "Parallel planar source faces must have matching complete trim loops",
        ));
    }
    let mut matches = Vec::new();
    let mut used = BTreeSet::new();
    for wire in first.iter() {
        let mut candidates = Vec::new();
        for (index, candidate) in second
            .iter()
            .enumerate()
            .filter(|(_, w)| w.len() == wire.len())
        {
            for reverse_order in [false, true] {
                for shift in 0..wire.len() {
                    let indices = (0..wire.len())
                        .map(|i| {
                            if reverse_order {
                                (shift + wire.len() - i) % wire.len()
                            } else {
                                (shift + i) % wire.len()
                            }
                        })
                        .collect::<Vec<_>>();
                    if indices
                        .iter()
                        .enumerate()
                        .all(|(i, j)| equivalent(&wire[i].curve(), &candidate[*j].curve()))
                    {
                        candidates.push((index, indices));
                    }
                }
            }
        }
        // A one/two-edge loop can have duplicate cyclic-order descriptions of
        // the same actual correspondence. Deduplicate those descriptions.
        candidates.sort();
        candidates.dedup();
        if candidates.len() != 1 || !used.insert(candidates[0].0) {
            return Err(unsupported(
                "Planar trims need unique translated source-loop correspondence, including holes",
            ));
        }
        matches.push(candidates.remove(0));
    }
    let mut result = builder::translated(&faces[0], delta * fraction);
    history.tool_face(
        owner,
        &result,
        &sources
            .iter()
            .map(|s| s.reference.clone())
            .collect::<Vec<_>>(),
        &serde_json::json!({"operation":"mid_surface","mode":"parallel_planar"}),
    )?;
    let original = faces[0].absolute_boundaries();
    let output = result.absolute_boundaries();
    for (i, wire) in output.iter().enumerate() {
        for (j, edge) in wire.iter().enumerate() {
            let pair = [
                adapter.boundary_edge_reference(&sources[0].body, &original[i][j])?,
                adapter.boundary_edge_reference(
                    &sources[1].body,
                    &second[matches[i].0][matches[i].1[j]],
                )?,
            ];
            history.tool_edge(owner,edge,&pair,&serde_json::json!({"operation":"mid_surface","mode":"parallel_planar","boundary":"paired_trim"}))?;
        }
    }
    if reverse {
        result.invert();
    }
    Ok(result)
}
fn mid_surface(
    context: &mut Context<'_>,
    sources: &[FaceSource; 2],
    fraction: &cad_core::Expr,
    correspondence: &PatchCorrespondence,
    reverse: bool,
) -> Result<Vec<Shell>> {
    let Context {
        adapter,
        doc,
        owner,
        inputs,
        history,
        proof_work,
    } = context;
    let fraction = expression::value(doc, &owner.definition, fraction, Dimension::Scalar)?;
    if !fraction.is_finite() || !(0. ..=1.).contains(&fraction) {
        return Err(invalid(
            "Position between source faces must be finite and between zero and one",
        ));
    }
    if sources[0].body == sources[1].body && sources[0].reference == sources[1].reference {
        return Err(invalid("Select two different source faces"));
    }
    let faces = [
        source_face(adapter, &sources[0], inputs)?,
        source_face(adapter, &sources[1], inputs)?,
    ];
    if faces[0].id() == faces[1].id() {
        return Err(invalid(
            "Select two distinct native faces; output aliases of one face are not independent sources",
        ));
    }
    if faces
        .iter()
        .all(|f| matches!(f.surface(), Surface::Plane(_)))
    {
        return Ok(vec![
            vec![planar_mid(
                adapter, owner, sources, &faces, fraction, reverse, history,
            )?]
            .into(),
        ]);
    }
    let first = align_patch(
        crate::tangent_plane::full_rational_patch(&faces[0])?,
        &PatchCorrespondence::default(),
    )?;
    let second = align_patch(
        crate::tangent_plane::full_rational_patch(&faces[1])?,
        correspondence,
    )?;
    if first.knot_vectors() != second.knot_vectors()
        || first.control_points().len() != second.control_points().len()
        || first.control_points()[0].len() != second.control_points()[0].len()
    {
        return Err(unsupported(
            "Rational Mid-Surface requires equal normalized knot bases and control-grid dimensions after explicit UV correspondence",
        ));
    }
    let wa = first.control_points()[0][0].w;
    let wb = second.control_points()[0][0].w;
    let net=first.control_points().iter().zip(second.control_points()).map(|(a,b)|a.iter().zip(b).map(|(a,b)|{
        if a.w/wa!=b.w/wb{return Err(unsupported("Corresponding rational patches need exactly equal normalized weights; unequal-weight point means are not approximated"));}
        let pa=point(*a)?;let pb=point(*b)?;
        let p=pa.to_vec()*(1.-fraction)+pb.to_vec()*fraction;
        let w=a.w/wa;let h=Vector4::new(p.x*w,p.y*w,p.z*w,w);point(h)?;Ok(h)
    }).collect::<Result<Vec<_>>>()).collect::<Result<Vec<_>>>()?;
    let carrier = NurbsSurface::new(
        BsplineSurface::try_new(first.knot_vectors().clone(), net)
            .map_err(|e| invalid(&e.to_string()))?,
    );
    prove_regular(&carrier, proof_work)?;
    let first_curves = first.splitted_boundary().map(Curve::NurbsCurve);
    let second_curves = second.splitted_boundary().map(Curve::NurbsCurve);
    let first_refs = boundary_sources(adapter, &sources[0], &faces[0], &first_curves)?;
    let second_refs = boundary_sources(adapter, &sources[1], &faces[1], &second_curves)?;
    let mut result = crate::surface_edit::patch_face(carrier)?;
    history.tool_face(
        owner,
        &result,
        &sources
            .iter()
            .map(|s| s.reference.clone())
            .collect::<Vec<_>>(),
        &serde_json::json!({"operation":"mid_surface","mode":"rational_correspondence"}),
    )?;
    for (i, edge) in result.absolute_boundaries()[0].iter().enumerate() {
        history.tool_edge(owner,edge,&[first_refs[i].clone(),second_refs[i].clone()],&serde_json::json!({"operation":"mid_surface","mode":"rational_correspondence","boundary":"paired_natural_side"}))?;
    }
    if reverse == faces[0].orientation() {
        result.invert();
    }
    Ok(vec![vec![result].into()])
}
