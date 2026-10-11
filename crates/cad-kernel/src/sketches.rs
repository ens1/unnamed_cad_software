//! Exact sketch curves stay inside the replaceable geometry adapter.
use crate::adapter::Shape;
use cad_core::{CadError, Dimension, Document, Expr, Result, Sketch, SketchCurve, expression};
use monstertruck::modeling::*;
use std::collections::BTreeMap;
fn invalid(message: impl ToString) -> CadError {
    CadError::Invalid(message.to_string())
}
pub(crate) fn build(
    doc: &Document,
    owner: &str,
    sketch: &Sketch,
    plane: &cad_core::Plane,
) -> Result<Shape> {
    Ok(build_traced(doc, owner, sketch, plane)?.shape)
}
pub(crate) struct BuiltSketch {
    pub shape: Shape,
    pub members: std::collections::HashMap<EdgeId, (String, usize)>,
}
pub(crate) fn build_traced(
    doc: &Document,
    owner: &str,
    sketch: &Sketch,
    plane: &cad_core::Plane,
) -> Result<BuiltSketch> {
    let mut members = std::collections::HashMap::new();
    let value = |e: &Expr| expression::value(doc, owner, e, Dimension::Length);
    let u = Vector3::from(plane.u);
    let v = Vector3::from(plane.v);
    let origin = Point3::from(plane.origin);
    let at = |p: [f64; 2]| origin + u * p[0] + v * p[1];
    let aliases = cad_core::sketch_relations::topological_point_aliases(sketch, 1e-6)?;
    let canonical_vertices = sketch
        .points
        .iter()
        .filter(|(id, _)| aliases.get(*id).is_none_or(|canonical| canonical == *id))
        .map(|(id, p)| (id.clone(), builder::vertex(at(*p))))
        .collect::<BTreeMap<_, _>>();
    let vertices = sketch
        .points
        .keys()
        .map(|id| {
            let canonical = aliases.get(id).unwrap_or(id);
            canonical_vertices
                .get(canonical)
                .cloned()
                .map(|vertex| (id.clone(), vertex))
                .ok_or_else(|| invalid("Sketch relation endpoint has no canonical point"))
        })
        .collect::<Result<BTreeMap<_, _>>>()?;
    let vertex = |id: &str| {
        vertices
            .get(id)
            .ok_or_else(|| invalid(format!("Missing sketch point {id}")))
    };
    let mut closed = Vec::<Wire>::new();
    let mut edges = Vec::<Edge>::new();
    if !sketch.profile_order.is_empty() {
        closed.push(crate::adapter::wire_polygon(
            &sketch
                .profile_order
                .iter()
                .map(|id| sketch.points[id])
                .collect::<Vec<_>>(),
            plane,
        )?);
    }
    if sketch.profile_order.is_empty() {
        for segment in &sketch.segments {
            edges.push(builder::line(vertex(&segment[0])?, vertex(&segment[1])?));
        }
    }
    let ellipse = |center: &str, a: f64, b: f64, rotation: f64| -> Result<Wire> {
        if a <= 1e-9 || b <= 1e-9 {
            return Err(invalid("Sketch radii must be positive"));
        }
        let c = vertex(center)?.point();
        let x = (u * rotation.cos() + v * rotation.sin()) * a;
        let y = (-u * rotation.sin() + v * rotation.cos()) * b;
        let n = u.cross(v).normalize();
        let transform =
            Matrix4::from_cols(x.extend(0.), y.extend(0.), n.extend(0.), c.to_homogeneous());
        let circle: Wire = primitive::circle(
            Point3::new(1., 0., 0.),
            Point3::origin(),
            Vector3::unit_z(),
            4,
        );
        Ok(builder::transformed(&circle, transform))
    };
    for c in sketch.circles.values() {
        let r = value(&c.radius)?;
        closed.push(ellipse(&c.center, r, r, 0.)?);
    }
    let suppressed = cad_core::sketch_transform::suppressed_members(sketch);
    for (id, member) in sketch
        .members
        .iter()
        .filter(|(id, m)| !m.construction && !suppressed.contains(*id))
    {
        let edge_start = edges.len();
        let closed_start = closed.len();
        match &member.curve {
            SketchCurve::Line { start, end } => {
                edges.push(builder::line(vertex(start)?, vertex(end)?))
            }
            SketchCurve::Arc {
                start,
                through,
                end,
            } => edges.push(
                builder::try_circle_arc(vertex(start)?, vertex(end)?, vertex(through)?.point())
                    .map_err(invalid)?,
            ),
            SketchCurve::Bezier { points } => {
                if !(2..=16).contains(&points.len()) {
                    return Err(invalid("Control-point splines need 2–16 points"));
                }
                let inter = points[1..points.len() - 1]
                    .iter()
                    .map(|id| vertex(id).map(Vertex::point))
                    .collect::<Result<Vec<_>>>()?;
                edges.push(builder::bezier(
                    vertex(&points[0])?,
                    vertex(points.last().unwrap())?,
                    inter,
                ));
            }
            SketchCurve::Nurbs {
                points,
                weights,
                knots,
                degree,
            } => {
                let curve = cad_core::sketch_edit::Curve2 {
                    control_points: points
                        .iter()
                        .map(|p| {
                            sketch
                                .points
                                .get(aliases.get(p).unwrap_or(p))
                                .copied()
                                .ok_or_else(|| invalid("Missing spline control"))
                        })
                        .collect::<Result<_>>()?,
                    weights: weights.clone(),
                    knots: knots.clone(),
                    degree: *degree,
                };
                cad_core::sketch_edit::validate_nurbs(&curve)?;
                let controls = curve
                    .control_points
                    .iter()
                    .zip(weights)
                    .map(|(p, w)| at(*p).to_homogeneous() * *w)
                    .collect();
                let mut curve =
                    NurbsCurve::new(BsplineCurve::new(KnotVector::from(knots.clone()), controls));
                let first = vertex(&points[0])?;
                let last = vertex(points.last().unwrap())?;
                if first == last {
                    let t = (knots[*degree] + knots[points.len()]) * 0.5;
                    let middle = builder::vertex(curve.subs(t));
                    let second = curve.cut(t);
                    closed.push(Wire::from(vec![
                        Edge::try_new(first, &middle, Curve::NurbsCurve(curve)).map_err(invalid)?,
                        Edge::try_new(&middle, first, Curve::NurbsCurve(second))
                            .map_err(invalid)?,
                    ]));
                } else {
                    edges.push(
                        Edge::try_new(first, last, Curve::NurbsCurve(curve)).map_err(invalid)?,
                    );
                }
            }
            SketchCurve::Circle { center, radius } => {
                let r = value(radius)?;
                closed.push(ellipse(center, r, r, 0.)?);
            }
            SketchCurve::Ellipse {
                center,
                major,
                minor,
                angle,
            } => closed.push(ellipse(
                center,
                value(major)?,
                value(minor)?,
                expression::value(doc, owner, angle, Dimension::Angle)?,
            )?),
        }
        for (ordinal, edge) in edges[edge_start..]
            .iter()
            .chain(closed[closed_start..].iter().flatten())
            .enumerate()
        {
            members.insert(edge.id(), (id.clone(), ordinal));
        }
    }
    let mut paths = Vec::new();
    while !edges.is_empty() {
        // Start at an open endpoint when one exists. Stable point identities
        // join mixed lines/arcs/splines without inferring approximate coincidence.
        let endpoint = edges
            .iter()
            .flat_map(|e| [e.front().id(), e.back().id()])
            .find(|id| {
                edges
                    .iter()
                    .filter(|e| e.front().id() == *id || e.back().id() == *id)
                    .count()
                    == 1
            });
        let index = endpoint
            .and_then(|id| {
                edges
                    .iter()
                    .position(|e| e.front().id() == id || e.back().id() == id)
            })
            .unwrap_or(0);
        let mut first = edges.remove(index);
        if endpoint.is_some_and(|id| first.back().id() == id) {
            first.invert();
        }
        let mut wire = Wire::from(vec![first]);
        loop {
            let end = wire.back().unwrap().back().id();
            let candidates = edges
                .iter()
                .enumerate()
                .filter(|(_, e)| e.front().id() == end || e.back().id() == end)
                .map(|(i, _)| i)
                .collect::<Vec<_>>();
            if candidates.len() > 1 {
                return Err(invalid(
                    "A sketch contour branches; separate construction geometry or remove duplicate members",
                ));
            }
            let Some(index) = candidates.first() else {
                break;
            };
            let mut edge = edges.remove(*index);
            if edge.back().id() == end {
                edge.invert();
            }
            wire.push_back(edge);
        }
        if wire.front().unwrap().front().id() == wire.back().unwrap().back().id() {
            closed.push(wire);
        } else {
            paths.push(wire);
        }
    }
    let uv = |p: Point3| {
        let p = p - origin;
        let uu = u.dot(u);
        let vv = v.dot(v);
        let cross = u.dot(v);
        let d = uu * vv - cross * cross;
        [
            (p.dot(u) * vv - p.dot(v) * cross) / d,
            (p.dot(v) * uu - p.dot(u) * cross) / d,
        ]
    };
    let samples = |wire: &Wire| {
        wire.iter()
            .flat_map(|e| {
                let c = e.oriented_curve();
                let (a, b) = c.range_tuple();
                (0..24).map(move |i| uv(c.subs(a + (b - a) * i as f64 / 24.)))
            })
            .collect::<Vec<_>>()
    };
    let area = |p: &[[f64; 2]]| {
        p.iter()
            .zip(p.iter().cycle().skip(1))
            .take(p.len())
            .map(|(a, b)| a[0] * b[1] - a[1] * b[0])
            .sum::<f64>()
            / 2.
    };
    let contains = |p: &[[f64; 2]], q: [f64; 2]| {
        let mut inside = false;
        for (a, b) in p.iter().zip(p.iter().cycle().skip(1)).take(p.len()) {
            if (a[1] > q[1]) != (b[1] > q[1])
                && q[0] < (b[0] - a[0]) * (q[1] - a[1]) / (b[1] - a[1]) + a[0]
            {
                inside = !inside;
            }
        }
        inside
    };
    let mut loops = closed
        .into_iter()
        .map(|mut wire| {
            let points = samples(&wire);
            let a = area(&points);
            if a < 0. {
                wire.invert();
            }
            (wire, points, a.abs())
        })
        .collect::<Vec<_>>();
    // Do not guess material regions for crossing or touching contours. This
    // guard uses derived planar samples; the retained curves remain exact B-reps.
    let intersects = |a: [f64; 2], b: [f64; 2], c: [f64; 2], d: [f64; 2]| {
        let orient = |a: [f64; 2], b: [f64; 2], c: [f64; 2]| {
            (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
        };
        let within = |a: [f64; 2], b: [f64; 2], p: [f64; 2]| {
            (0..2).all(|i| p[i] >= a[i].min(b[i]) - 1e-9 && p[i] <= a[i].max(b[i]) + 1e-9)
        };
        let (ab_c, ab_d, cd_a, cd_b) = (
            orient(a, b, c),
            orient(a, b, d),
            orient(c, d, a),
            orient(c, d, b),
        );
        (ab_c * ab_d < 0. && cd_a * cd_b < 0.)
            || (ab_c.abs() < 1e-10 && within(a, b, c))
            || (ab_d.abs() < 1e-10 && within(a, b, d))
            || (cd_a.abs() < 1e-10 && within(c, d, a))
            || (cd_b.abs() < 1e-10 && within(c, d, b))
    };
    for (i, (_, points, area)) in loops.iter().enumerate() {
        if !area.is_finite() || *area < 1e-12 {
            return Err(invalid("A closed sketch contour has no area"));
        }
        for a in 0..points.len() {
            let b = (a + 1) % points.len();
            for c in a + 1..points.len() {
                let d = (c + 1) % points.len();
                if b == c || d == a {
                    continue;
                }
                if intersects(points[a], points[b], points[c], points[d]) {
                    return Err(invalid("A sketch contour crosses or touches itself"));
                }
            }
        }
        for (_, other, _) in loops.iter().skip(i + 1) {
            for a in 0..points.len() {
                for c in 0..other.len() {
                    if intersects(
                        points[a],
                        points[(a + 1) % points.len()],
                        other[c],
                        other[(c + 1) % other.len()],
                    ) {
                        return Err(invalid(
                            "Sketch material contours intersect or touch; separate them or use construction members",
                        ));
                    }
                }
            }
        }
    }
    loops.sort_by(|a, b| b.2.total_cmp(&a.2));
    let mut parents = Vec::new();
    for i in 0..loops.len() {
        let parent = (0..i).rev().find(|j| contains(&loops[*j].1, loops[i].1[0]));
        parents.push(parent);
    }
    let depth = |mut i: usize| {
        let mut d = 0;
        while let Some(parent) = parents[i] {
            d += 1;
            i = parent;
        }
        d
    };
    let mut faces = Vec::new();
    for i in 0..loops.len() {
        if depth(i) % 2 == 1 {
            continue;
        }
        let mut wires = vec![loops[i].0.clone()];
        for (j, parent) in parents.iter().enumerate() {
            if *parent == Some(i) {
                wires.push(loops[j].0.inverse());
            }
        }
        faces.push(builder::try_attach_plane(wires).map_err(invalid)?);
    }
    Ok(BuiltSketch {
        shape: Shape::Sketch { faces, paths },
        members,
    })
}
