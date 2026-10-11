//! Four-sided Euclidean Coons filling over exact native boundary carriers.
//! Positive rational denominators are multiplied, never discarded or sampled.
use crate::adapter::{MonstertruckAdapter, Shape, TopologyHistory};
use cad_core::{CadError, Document, NodeRef, Result};
use monstertruck::geometry::prelude::TryIntoHomogeneousBsplineSurface;
use monstertruck::modeling::*;
use std::collections::BTreeMap;

const EPS: f64 = 1e-7;
const MAX_CONTROLS: usize = 4096;
const MAX_NET: usize = 32768;
const MAX_PATCHES: usize = 256;
const MAX_DEGREE: usize = 16;

fn invalid(message: impl std::fmt::Display) -> CadError {
    CadError::Invalid(format!("Filled Surface: {message}"))
}
fn unsupported(message: &str) -> CadError {
    CadError::Unsupported(format!("Filled Surface: {message}"))
}
fn point(h: Vector4) -> Result<Point3> {
    if ![h.x, h.y, h.z, h.w].into_iter().all(f64::is_finite) || h.w <= 0. {
        return Err(unsupported("Finite positive rational weights are required"));
    }
    let p = Point3::new(h.x / h.w, h.y / h.w, h.z / h.w);
    if ![p.x, p.y, p.z].into_iter().all(f64::is_finite) {
        return Err(invalid("Rational coordinates exceed finite bounds"));
    }
    Ok(p)
}
fn carrier(curve: &Curve) -> Result<BsplineCurve<Vector4>> {
    let count = match curve {
        Curve::Line(_) => 2,
        Curve::BsplineCurve(curve) => curve.control_points().len(),
        Curve::NurbsCurve(curve) => curve.control_points().len(),
        _ => {
            return Err(unsupported(
                "Use explicit line, B-spline or positive-weight NURBS boundaries; implicit intersection and surface-parameter guides are not exact substitutes",
            ));
        }
    };
    if count > MAX_CONTROLS {
        return Err(unsupported("One boundary exceeds 4096 native controls"));
    }
    crate::extrusion::curve_hull(curve)?;
    let mut curve = curve.lift_up();
    curve.clamp();
    curve.knot_normalize();
    if !(1..=MAX_DEGREE).contains(&curve.degree()) || curve.control_points().len() > MAX_CONTROLS {
        return Err(unsupported(
            "Boundary carriers require degree one through sixteen and at most 4096 controls",
        ));
    }
    let (knots, multiplicities) = curve.knot_vector().to_single_multi();
    if knots.len() < 2
        || knots.iter().any(|k| !k.is_finite())
        || knots
            .iter()
            .zip(multiplicities)
            .any(|(knot, count)| *knot > 0. && *knot < 1. && count > curve.degree())
    {
        return Err(unsupported("Every boundary must be position-continuous"));
    }
    let maximum = curve
        .control_points()
        .iter()
        .map(|p| p.w)
        .fold(0f64, f64::max);
    for h in curve.control_points_mut() {
        point(*h)?;
        *h /= maximum;
        point(*h)?;
        if h.w < 1e-12 {
            return Err(unsupported(
                "Boundary weights exceed the positive rational conditioning bound",
            ));
        }
    }
    Ok(curve)
}

struct Boundary {
    curve: Curve,
    source: crate::section_history::Member,
    profile: NodeRef,
    reversed: bool,
}
fn boundary(
    adapter: &MonstertruckAdapter,
    doc: &Document,
    profile: &NodeRef,
    inputs: &BTreeMap<NodeRef, Shape>,
) -> Result<Boundary> {
    // Rebuild the solved native sketch with its member trace. An open sketch
    // is not a body and has no meaningful location-based topology fallback.
    let (source, plane) = crate::extrusion::source(adapter, doc, profile, inputs)?;
    let built = crate::sketches::build_traced(doc, &profile.definition, &source, &plane)?;
    let Shape::Sketch { faces, paths } = built.shape else {
        return Err(invalid("Select a native sketch boundary"));
    };
    if !faces.is_empty() || paths.len() != 1 || paths[0].len() != 1 || paths[0].is_closed() {
        return Err(unsupported(
            "Select four ordered, end-connected open sketch curves, one native member per sketch; construction members are excluded",
        ));
    }
    let edge = &paths[0][0];
    let (member, piece) = built
        .members
        .get(&edge.id())
        .ok_or_else(|| invalid("A boundary lost its exact sketch-member ancestry"))?;
    Ok(Boundary {
        curve: edge.oriented_curve(),
        source: crate::section_history::member(
            profile,
            &crate::extrusion::ThinMember {
                sources: vec![member.clone()],
                side: 0,
                piece: *piece,
                cap: false,
            },
        )?,
        profile: profile.clone(),
        reversed: !edge.orientation(),
    })
}

fn greville(curve: &BsplineCurve<Point3>) -> Vec<f64> {
    let degree = curve.degree();
    (0..curve.control_points().len())
        .map(|i| {
            curve.knot_vector()[i + 1..i + degree + 1]
                .iter()
                .sum::<f64>()
                / degree as f64
        })
        .collect()
}

fn paired_span_count(curves: &[BsplineCurve<Vector4>], first: usize, last: usize) -> usize {
    let mut knots = curves[first]
        .knot_vector()
        .iter()
        .chain(curves[last].knot_vector())
        .copied()
        .collect::<Vec<_>>();
    knots.sort_by(f64::total_cmp);
    knots.dedup();
    knots.len() - 1
}
fn preflight(curves: &[BsplineCurve<Vector4>]) -> Result<()> {
    let u = paired_span_count(curves, 0, 2);
    let v = paired_span_count(curves, 1, 3);
    if u.checked_mul(v).is_none_or(|n| n > MAX_PATCHES) {
        return Err(unsupported(
            "The complete fill supports at most 256 synchronized native Bezier cells",
        ));
    }
    Ok(())
}

fn polynomial_fill(curves: &[BsplineCurve<Vector4>]) -> Result<NurbsSurface<Vector4>> {
    let curves = curves
        .iter()
        .map(|curve| {
            BsplineCurve::try_new(
                curve.knot_vector().clone(),
                curve
                    .control_points()
                    .iter()
                    .copied()
                    .map(point)
                    .collect::<Result<Vec<_>>>()?,
            )
            .map_err(invalid)
        })
        .collect::<Result<Vec<_>>>()?;
    let mut horizontal = vec![curves[0].clone(), curves[2].inverse()];
    let mut vertical = vec![curves[3].inverse(), curves[1].clone()];
    monstertruck::geometry::nurbs::compat::make_curves_compatible(&mut horizontal)
        .map_err(invalid)?;
    monstertruck::geometry::nurbs::compat::make_curves_compatible(&mut vertical)
        .map_err(invalid)?;
    let u = greville(&horizontal[0]);
    let v = greville(&vertical[0]);
    if u.len().checked_mul(v.len()).is_none_or(|n| n > MAX_NET) {
        return Err(unsupported("The filled carrier exceeds 32768 controls"));
    }
    let h0 = horizontal[0].control_points();
    let h1 = horizontal[1].control_points();
    let v0 = vertical[0].control_points();
    let v1 = vertical[1].control_points();
    let corners = [h0[0], h0[h0.len() - 1], h1[0], h1[h1.len() - 1]];
    let origin = corners[0];
    let corners = corners.map(|p| p - origin);
    let net = u
        .iter()
        .enumerate()
        .map(|(i, &u)| {
            v.iter()
                .enumerate()
                .map(|(j, &v)| {
                    let bilinear = corners[0] * (1. - u) * (1. - v)
                        + corners[1] * u * (1. - v)
                        + corners[2] * (1. - u) * v
                        + corners[3] * u * v;
                    origin
                        + (h0[i] - origin) * (1. - v)
                        + (h1[i] - origin) * v
                        + (v0[j] - origin) * (1. - u)
                        + (v1[j] - origin) * u
                        - bilinear
                })
                .collect()
        })
        .collect();
    let surface = BsplineSurface::try_new(
        (
            horizontal[0].knot_vector().clone(),
            vertical[0].knot_vector().clone(),
        ),
        net,
    )
    .map_err(invalid)?;
    Ok(NurbsSurface::new(
        surface
            .try_into_homogeneous_bspline_surface()
            .ok_or_else(|| invalid("Polynomial fill conversion failed"))?,
    ))
}

// Bernstein products with a linear polynomial, and degree elevation, retain
// the exact basis. No fit through points or control-polygon substitution occurs.
fn linear_product(values: &[f64], linear: [f64; 2]) -> Vec<f64> {
    let degree = values.len();
    (0..=degree)
        .map(|i| {
            let alpha = i as f64 / degree as f64;
            let left = if i == 0 {
                0.
            } else {
                values[i - 1] * linear[1]
            };
            let right = if i == degree {
                0.
            } else {
                values[i] * linear[0]
            };
            alpha * left + (1. - alpha) * right
        })
        .collect()
}
fn elevate(values: &[Vector3]) -> Vec<Vector3> {
    let degree = values.len();
    (0..=degree)
        .map(|i| {
            let alpha = i as f64 / degree as f64;
            let left = if i == 0 {
                Vector3::zero()
            } else {
                values[i - 1]
            };
            let right = if i == degree {
                Vector3::zero()
            } else {
                values[i]
            };
            left * alpha + right * (1. - alpha)
        })
        .collect()
}
struct Span {
    numerator: [Vec<Vector3>; 2],
    weight: Vec<f64>,
    first: Vec<f64>,
    last: Vec<f64>,
    start: f64,
}
fn spans(
    first: &BsplineCurve<Vector4>,
    last: &BsplineCurve<Vector4>,
    origin: Point3,
) -> Result<Vec<Span>> {
    let first_spans = first.bezier_decomposition();
    let last_spans = last.bezier_decomposition();
    if first_spans.len() != last_spans.len() {
        return Err(invalid("The exact opposing boundary partition was lost"));
    }
    first_spans
        .iter()
        .zip(last_spans)
        .map(|(first, last)| {
            if first.range_tuple() != last.range_tuple()
                || first.degree() != last.degree()
                || first
                    .control_points()
                    .iter()
                    .zip(last.control_points())
                    .any(|(a, b)| a.w != b.w)
            {
                return Err(invalid(
                    "Opposing native boundaries lost their common denominator",
                ));
            }
            let range = first.range_tuple();
            let weight = first
                .control_points()
                .iter()
                .map(|h| h.w)
                .collect::<Vec<_>>();
            let numerator = [first, &last].map(|curve| {
                elevate(
                    &curve
                        .control_points()
                        .iter()
                        .map(|h| Vector3::new(h.x, h.y, h.z) - origin.to_vec() * h.w)
                        .collect::<Vec<_>>(),
                )
            });
            Ok(Span {
                numerator,
                weight: linear_product(&weight, [1., 1.]),
                first: linear_product(&weight, [1. - range.0, 1. - range.1]),
                last: linear_product(&weight, [range.0, range.1]),
                start: range.0,
            })
        })
        .collect()
}
fn knots(spans: &[Span], degree: usize) -> KnotVector {
    let mut knots = vec![0.; degree + 1];
    for span in spans.iter().skip(1) {
        knots.extend(std::iter::repeat_n(span.start, degree));
    }
    knots.extend(std::iter::repeat_n(1., degree + 1));
    KnotVector::from(knots)
}

fn rational_fill(curves: &[BsplineCurve<Vector4>]) -> Result<NurbsSurface<Vector4>> {
    // Opposing numerators use one common positive denominator. Their weights
    // are not projectively interpolated: the Coons sum is Euclidean.
    if curves[0].degree() + curves[2].degree() + 1 > MAX_DEGREE
        || curves[1].degree() + curves[3].degree() + 1 > MAX_DEGREE
    {
        return Err(unsupported(
            "Rational filling requires opposing source degrees whose sum plus one is at most sixteen; conic and cubic rational boundaries are supported",
        ));
    }
    let as_curve = |index: usize| Curve::NurbsCurve(NurbsCurve::new(curves[index].clone()));
    let horizontal = crate::loft_correspondence::compatible(&as_curve(0), &as_curve(2).inverse())?;
    let vertical = crate::loft_correspondence::compatible(&as_curve(3).inverse(), &as_curve(1))?;
    let u_degree = horizontal.0.degree() + 1;
    let v_degree = vertical.0.degree() + 1;
    let origin = point(horizontal.0.control_points()[0])?;
    let corners = [
        point(horizontal.0.control_points()[0])? - origin,
        point(*horizontal.0.control_points().last().unwrap())? - origin,
        point(horizontal.1.control_points()[0])? - origin,
        point(*horizontal.1.control_points().last().unwrap())? - origin,
    ];
    let u = spans(&horizontal.0, &horizontal.1, origin)?;
    let v = spans(&vertical.0, &vertical.1, origin)?;
    if u.len().checked_mul(v.len()).is_none_or(|n| n > MAX_PATCHES) {
        return Err(unsupported(
            "Rational filling supports at most 256 native Bezier cells",
        ));
    }
    let rows = u.len() * u_degree + 1;
    let columns = v.len() * v_degree + 1;
    if rows.checked_mul(columns).is_none_or(|n| n > MAX_NET) {
        return Err(unsupported("The filled carrier exceeds 32768 controls"));
    }
    let mut net = vec![vec![Vector4::zero(); columns]; rows];
    for (a, horizontal) in u.iter().enumerate() {
        for (b, vertical) in v.iter().enumerate() {
            for (i, row) in net
                .iter_mut()
                .skip(a * u_degree)
                .take(u_degree + 1)
                .enumerate()
            {
                if a > 0 && i == 0 {
                    continue;
                }
                for (j, h) in row
                    .iter_mut()
                    .skip(b * v_degree)
                    .take(v_degree + 1)
                    .enumerate()
                {
                    if b > 0 && j == 0 {
                        continue;
                    }
                    let weight = horizontal.weight[i] * vertical.weight[j];
                    let bilinear = corners[0] * horizontal.first[i] * vertical.first[j]
                        + corners[1] * horizontal.last[i] * vertical.first[j]
                        + corners[2] * horizontal.first[i] * vertical.last[j]
                        + corners[3] * horizontal.last[i] * vertical.last[j];
                    let numerator = horizontal.numerator[0][i] * vertical.first[j]
                        + horizontal.numerator[1][i] * vertical.last[j]
                        + vertical.numerator[0][j] * horizontal.first[i]
                        + vertical.numerator[1][j] * horizontal.last[i]
                        - bilinear
                        + origin.to_vec() * weight;
                    *h = Vector4::new(numerator.x, numerator.y, numerator.z, weight);
                    point(*h)?;
                }
            }
        }
    }
    Ok(NurbsSurface::new(
        BsplineSurface::try_new((knots(&u, u_degree), knots(&v, v_degree)), net)
            .map_err(invalid)?,
    ))
}

pub(crate) fn operation(
    adapter: &MonstertruckAdapter,
    doc: &Document,
    owner: &NodeRef,
    profiles: &[NodeRef; 4],
    reverse: bool,
    inputs: &BTreeMap<NodeRef, Shape>,
    history: &mut TopologyHistory,
) -> Result<Shape> {
    let mut boundaries = profiles
        .iter()
        .map(|profile| boundary(adapter, doc, profile, inputs))
        .collect::<Result<Vec<_>>>()?;
    for index in 1..4 {
        let end = boundaries[index - 1]
            .curve
            .subs(boundaries[index - 1].curve.range_tuple().1);
        if (boundaries[index]
            .curve
            .subs(boundaries[index].curve.range_tuple().0)
            - end)
            .magnitude()
            > EPS
        {
            boundaries[index].curve.invert();
            boundaries[index].reversed = !boundaries[index].reversed;
        }
        if (boundaries[index]
            .curve
            .subs(boundaries[index].curve.range_tuple().0)
            - end)
            .magnitude()
            > EPS
        {
            return Err(invalid(format!(
                "Boundary {} does not meet the preceding boundary; join their sketch endpoints",
                index + 1
            )));
        }
    }
    let end = boundaries[3]
        .curve
        .subs(boundaries[3].curve.range_tuple().1);
    let start = boundaries[0]
        .curve
        .subs(boundaries[0].curve.range_tuple().0);
    if (end - start).magnitude() > EPS {
        return Err(invalid(
            "The fourth boundary does not close the four-sided loop",
        ));
    }
    let curves = boundaries
        .iter()
        .map(|boundary| carrier(&boundary.curve))
        .collect::<Result<Vec<_>>>()?;
    preflight(&curves)?;
    let polynomial = curves.iter().all(|curve| {
        curve
            .control_points()
            .iter()
            .all(|h| h.w == curve.control_points()[0].w)
    });
    let surface = if polynomial {
        polynomial_fill(&curves)?
    } else {
        rational_fill(&curves)?
    };
    let maximum = surface
        .control_points()
        .iter()
        .flatten()
        .map(|h| h.w)
        .fold(0f64, f64::max);
    for h in surface.control_points().iter().flatten() {
        point(*h)?;
        if h.w / maximum < 1e-12 {
            return Err(unsupported(
                "The filled carrier exceeds the positive rational conditioning bound",
            ));
        }
    }
    // A fixed projected Jacobian is certified over every exact Bernstein cell.
    // This accepts legitimate shear and nonconvex control nets without treating
    // a few normals, display samples or local regularity as global injectivity.
    let mut proof_work = 0;
    crate::surface_construct::prove_regular(&surface, &mut proof_work)?;
    let mut face = crate::surface_edit::patch_face(surface)?;
    let sources = boundaries
        .iter()
        .flat_map(|boundary| boundary.source.sources.clone())
        .collect::<Vec<_>>();
    history.tool_face(owner, &face, &sources, &serde_json::json!({"operation":"surface_fill","constructor":"euclidean_coons","boundaries":profiles,"reverse":reverse}))?;
    let edges = face.boundaries();
    for (index, boundary) in boundaries.iter().enumerate() {
        history.tool_edge(owner, &edges[0][index], &boundary.source.sources, &serde_json::json!({"operation":"surface_fill","boundary":index,"section":boundary.profile,"member":boundary.source.purpose,"reversed":boundary.reversed}))?;
    }
    if reverse {
        face.invert();
    }
    Ok(Shape::Surfaces(vec![vec![face].into()]))
}
