//! Exact native spatial sketch wires and source-qualified consumer paths.
use crate::adapter::{MonstertruckAdapter, Shape, TopologyHistory};
use cad_core::sketch3d::{Curve as SourceCurve, Definition, Evaluated, Polyline};
use cad_core::solver::ConstraintSolver;
use cad_core::{CadError, Dimension, Document, NodeRef, Result, TopologyRef, expression};
use monstertruck::modeling::*;
use std::collections::{BTreeMap, BTreeSet, HashMap};

const MAX_DISPLAY_POINTS: usize = 1_000_000;
fn invalid(message: impl ToString) -> CadError {
    CadError::Invalid(format!("3D sketch geometry: {}", message.to_string()))
}
fn unsupported(message: &str) -> CadError {
    CadError::Unsupported(format!("3D sketch geometry: {message}"))
}
fn piece_id(member: &str, piece: usize) -> String {
    format!("{member}::{piece}")
}
fn is_member(piece: &cad_core::reference_curves::Piece, member: &str) -> bool {
    piece
        .id
        .strip_prefix(&format!("{member}::"))
        .is_some_and(|suffix| suffix.parse::<usize>().is_ok())
}
fn native_members(
    doc: &Document,
    owner: &NodeRef,
    source: &Definition,
) -> Result<Vec<(String, Wire)>> {
    use cad_core::sketch3d::Endpoint;
    let aliases = cad_core::sketch3d::endpoint_aliases(doc, &owner.definition, source)?;
    let vertices = aliases
        .values()
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .map(|endpoint| {
            Ok((
                endpoint.clone(),
                builder::vertex(Point3::from(cad_core::sketch3d::endpoint_position(
                    doc,
                    &owner.definition,
                    source,
                    &endpoint,
                )?)),
            ))
        })
        .collect::<Result<BTreeMap<_, _>>>()?;
    let endpoint_vertex = |endpoint: Endpoint| {
        vertices
            .get(aliases.get(&endpoint).unwrap_or(&endpoint))
            .ok_or_else(|| invalid("A spatial endpoint is missing"))
    };
    let vertex = |id: &str| endpoint_vertex(Endpoint::Point { point: id.into() });
    let mut members = vec![];
    for (id, member) in &source.members {
        let wire = match &member.curve {
            SourceCurve::Line { start, end } => {
                Wire::from(vec![builder::line(vertex(start)?, vertex(end)?)])
            }
            SourceCurve::Arc {
                start,
                through,
                end,
            } => Wire::from(vec![
                builder::try_circle_arc(vertex(start)?, vertex(end)?, vertex(through)?.point())
                    .map_err(invalid)?,
            ]),
            SourceCurve::Circle {
                center,
                plane,
                radius,
            } => {
                let radius = expression::value(doc, &owner.definition, radius, Dimension::Length)?;
                if !radius.is_finite() || radius <= 1e-7 {
                    return Err(invalid("A spatial circle needs a positive radius"));
                }
                let u = Vector3::from(plane.u);
                let v = Vector3::from(plane.v);
                let n = u.cross(v);
                let transform = Matrix4::from_cols(
                    (u * radius).extend(0.),
                    (v * radius).extend(0.),
                    n.extend(0.),
                    vertex(center)?.point().to_homogeneous(),
                );
                let unit = primitive::circle(
                    Point3::new(1., 0., 0.),
                    Point3::origin(),
                    Vector3::unit_z(),
                    4,
                );
                let circle = builder::transformed(&unit, transform);
                let seam = endpoint_vertex(Endpoint::CircleSeam { member: id.clone() })?;
                if circle
                    .front()
                    .is_none_or(|edge| edge.front().point().distance(seam.point()) > 1e-7)
                    || circle
                        .back()
                        .is_none_or(|edge| edge.back().point().distance(seam.point()) > 1e-7)
                {
                    return Err(unsupported(
                        "Shared circular endpoint is inconsistent with its unchanged native carrier",
                    ));
                }
                circle
                    .iter()
                    .enumerate()
                    .map(|(index, edge)| {
                        Edge::try_new(
                            if index == 0 { seam } else { edge.front() },
                            if index + 1 == circle.len() {
                                seam
                            } else {
                                edge.back()
                            },
                            edge.oriented_curve(),
                        )
                        .map_err(invalid)
                    })
                    .collect::<Result<Vec<_>>>()?
                    .into()
            }
            SourceCurve::Bezier { points }
            | SourceCurve::Nurbs { points, .. }
            | SourceCurve::InterpolatingSpline { points, .. } => {
                let rational = cad_core::sketch3d::rational_curve(
                    doc,
                    &owner.definition,
                    source,
                    &member.curve,
                )?;
                if rational.control_points.len() != rational.weights.len()
                    || rational.control_points.is_empty()
                {
                    return Err(invalid(
                        "Spatial spline controls and weights are inconsistent",
                    ));
                }
                let controls = rational
                    .control_points
                    .iter()
                    .zip(&rational.weights)
                    .map(|(p, w)| Point3::from(*p).to_homogeneous() * *w)
                    .collect();
                let mut curve = NurbsCurve::new(BsplineCurve::new(
                    KnotVector::from(rational.knots.clone()),
                    controls,
                ));
                let first = vertex(&points[0])?;
                let last = vertex(
                    points
                        .last()
                        .ok_or_else(|| invalid("The spline has no final control"))?,
                )?;
                if first == last {
                    let start = rational.knots[rational.degree];
                    let finish = rational.knots[rational.control_points.len()];
                    let t = start + (finish - start) * 0.5;
                    if !t.is_finite() || t <= start || t >= finish {
                        return Err(unsupported(
                            "A closed spatial spline needs a representable interior parameter station",
                        ));
                    }
                    let middle = builder::vertex(curve.subs(t));
                    let second = curve.cut(t);
                    Wire::from(vec![
                        Edge::try_new(first, &middle, Curve::NurbsCurve(curve)).map_err(invalid)?,
                        Edge::try_new(&middle, first, Curve::NurbsCurve(second))
                            .map_err(invalid)?,
                    ])
                } else {
                    Wire::from(vec![
                        Edge::try_new(first, last, Curve::NurbsCurve(curve)).map_err(invalid)?,
                    ])
                }
            }
        };
        if wire.is_empty() || !wire.is_continuous() {
            return Err(invalid(
                "Spatial member construction did not retain a continuous native wire",
            ));
        }
        members.push((id.clone(), wire));
    }
    Ok(members)
}
/// Split branches into maximal native chains. Connectivity comes only from
/// authored point identity / exact coincidence relations, never proximity.
pub(crate) fn assemble_paths(mut edges: Vec<Edge>) -> Vec<Wire> {
    let mut paths = vec![];
    while !edges.is_empty() {
        let mut degree = HashMap::<VertexId, usize>::new();
        for edge in &edges {
            *degree.entry(edge.front().id()).or_default() += 1;
            *degree.entry(edge.back().id()).or_default() += 1;
        }
        let endpoint = edges
            .iter()
            .flat_map(|edge| [edge.front().id(), edge.back().id()])
            .find(|id| degree[id] != 2);
        let index = endpoint
            .and_then(|point| {
                edges
                    .iter()
                    .position(|edge| edge.front().id() == point || edge.back().id() == point)
            })
            .unwrap_or(0);
        let mut first = edges.remove(index);
        if endpoint.is_some_and(|point| first.back().id() == point) {
            first.invert();
        }
        let start = first.front().id();
        let mut wire = Wire::from(vec![first]);
        loop {
            let finish = wire.back().expect("a wire has its first edge").back().id();
            if finish == start || degree[&finish] != 2 {
                break;
            }
            let candidates = edges
                .iter()
                .enumerate()
                .filter(|(_, edge)| edge.front().id() == finish || edge.back().id() == finish)
                .map(|(index, _)| index)
                .collect::<Vec<_>>();
            if candidates.len() != 1 {
                break;
            }
            let mut next = edges.remove(candidates[0]);
            if next.back().id() == finish {
                next.invert();
            }
            wire.push_back(next);
        }
        paths.push(wire);
    }
    paths
}
pub(crate) fn build(
    doc: &Document,
    owner: &NodeRef,
    definition: &Definition,
    tolerance: f64,
    history: &mut TopologyHistory,
    mut publish: impl FnMut(Evaluated),
) -> Result<(Vec<Wire>, cad_core::reference_curves::Evaluated, Evaluated)> {
    if !tolerance.is_finite() || tolerance <= 0. {
        return Err(invalid("Display tolerance must be finite and positive"));
    }
    let mut solved = definition.clone();
    let report =
        cad_core::solver::NativeSolver.solve_sketch3d(doc, &owner.definition, &mut solved)?;
    let mut output = Evaluated {
        version: cad_core::sketch3d::CONTRACT_VERSION,
        producer: owner.clone(),
        solved,
        report,
        polylines: vec![],
        point_motion: BTreeMap::new(),
        constraint_residuals: vec![],
        diagnostics: vec![],
    };
    match cad_core::sketch3d::constraint_residuals(doc, &owner.definition, &output.solved) {
        Ok(residuals) => output.constraint_residuals = residuals,
        Err(error) => output
            .diagnostics
            .push(format!("Constraint residual analysis unavailable: {error}")),
    }
    match cad_core::solver::NativeSolver.analyze_sketch3d(doc, &owner.definition, &output.solved) {
        Ok(motion) => output.point_motion = motion,
        Err(error) => output.diagnostics.push(format!(
            "Local sketch freedom analysis unavailable: {error}"
        )),
    }
    publish(output.clone());
    let construction = (|| -> Result<_> {
        cad_core::sketch3d::validate_geometry(doc, &owner.definition, &output.solved)?;
        let members = native_members(doc, owner, &output.solved)?;
        let mut all = vec![];
        let mut identities = HashMap::new();
        let mut points = 0usize;
        for (member, wire) in members {
            let mut line = vec![];
            for (piece, edge) in wire.iter().enumerate() {
                let reference = history.constructed_edge(owner, edge.id(), &[], &serde_json::json!({"operation":"sketch3d_member","member":member,"piece":piece}))?;
                let curve = edge.oriented_curve();
                let sampled = crate::curve_sampling::display_points(&curve, tolerance)?;
                points = points.saturating_add(sampled.len());
                if sampled.len() < 2 || points > MAX_DISPLAY_POINTS {
                    return Err(invalid(
                        "Spatial sketch display subdivision exceeds its bounded point budget",
                    ));
                }
                let offset = usize::from(
                    line.last()
                        .is_some_and(|last| sampled.first() == Some(last)),
                );
                line.extend(sampled.into_iter().skip(offset));
                identities.insert(edge.id(), (piece_id(&member, piece), reference));
                all.push(edge.clone());
            }
            output.polylines.push(Polyline {
                member: member.clone(),
                points: line,
                closed: wire.is_closed(),
                construction: output.solved.members[&member].construction,
            });
        }
        let paths = assemble_paths(all);
        let mut pieces = vec![];
        for edge in paths.iter().flat_map(|wire| wire.iter()) {
            let (id, reference) = identities
                .get(&edge.id())
                .ok_or_else(|| invalid("A native spatial curve lost its source identity"))?;
            pieces.push(cad_core::reference_curves::Piece {
                id: id.clone(),
                reference: reference.clone(),
                start: edge.front().point().into(),
                end: edge.back().point().into(),
                second_derivative_bound_mm: crate::reference_curves::display_bound(
                    &edge.oriented_curve(),
                )?,
            });
        }
        let evaluated = cad_core::reference_curves::Evaluated {
            producer: owner.clone(),
            kind: "sketch3d".into(),
            closed: paths.len() == 1 && paths[0].is_closed(),
            pieces,
            analytic: None,
        };
        Ok((paths, evaluated))
    })();
    match construction {
        Ok((paths, evaluated)) => {
            publish(output.clone());
            Ok((paths, evaluated, output))
        }
        Err(error) => {
            output.diagnostics.push(error.to_string());
            publish(output);
            Err(error)
        }
    }
}
pub(crate) fn spatial<'a>(
    inputs: &'a BTreeMap<NodeRef, Shape>,
    owner: &NodeRef,
) -> Option<&'a Evaluated> {
    match inputs.get(owner) {
        Some(Shape::ReferenceCurves {
            spatial: Some(spatial),
            ..
        }) => Some(spatial),
        _ => None,
    }
}
pub(crate) fn member_curves(
    adapter: &MonstertruckAdapter,
    inputs: &BTreeMap<NodeRef, Shape>,
    owner: &NodeRef,
    member: &str,
) -> Result<Vec<(Curve, TopologyRef)>> {
    let Some(Shape::ReferenceCurves {
        paths,
        evaluated,
        spatial: Some(spatial),
    }) = inputs.get(owner)
    else {
        return Err(invalid("The spatial sketch input is unavailable"));
    };
    if !spatial.solved.members.contains_key(member) {
        return Err(invalid("The spatial source member was removed"));
    }
    let wanted = evaluated
        .pieces
        .iter()
        .filter(|piece| is_member(piece, member))
        .map(|piece| {
            Ok((
                cad_core::provenance::canonical_key(&piece.reference)?,
                piece.id.clone(),
            ))
        })
        .collect::<Result<BTreeMap<_, _>>>()?;
    let mut result = vec![];
    for edge in paths.iter().flat_map(|wire| wire.iter()) {
        let reference = adapter
            .edge_source(owner, edge.id())
            .ok_or_else(|| invalid("Spatial member ancestry is unavailable"))?;
        if let Some(id) = wanted.get(&cad_core::provenance::canonical_key(&reference)?) {
            result.push((id.clone(), edge.curve(), reference));
        }
    }
    result.sort_by_key(|(id, _, _)| {
        id.rsplit("::")
            .next()
            .and_then(|piece| piece.parse::<usize>().ok())
            .unwrap_or(usize::MAX)
    });
    if result.is_empty() || result.len() != wanted.len() {
        return Err(invalid("The spatial member has no native curve pieces"));
    }
    Ok(result
        .into_iter()
        .map(|(_, curve, reference)| (curve, reference))
        .collect())
}
/// One current native path selected from a spatial sketch, optionally by member
/// and authored endpoint. This is used by Sweep and Curve Pattern, not flattening.
pub(crate) fn selected_path(
    adapter: &MonstertruckAdapter,
    inputs: &BTreeMap<NodeRef, Shape>,
    owner: &NodeRef,
    member: Option<&str>,
    start_vertex: Option<&str>,
) -> Result<(Wire, Vec<TopologyRef>)> {
    let Some(Shape::ReferenceCurves {
        paths,
        evaluated,
        spatial: Some(spatial),
    }) = inputs.get(owner)
    else {
        return Err(invalid("Select an available spatial sketch"));
    };
    if member.is_some_and(|id| !spatial.solved.members.contains_key(id)) {
        return Err(invalid("The selected spatial path member was removed"));
    }
    let selected = spatial
        .solved
        .members
        .iter()
        .filter(|(id, value)| {
            member.map_or(!value.construction, |selected| selected == id.as_str())
        })
        .map(|(id, _)| id.as_str())
        .collect::<BTreeSet<_>>();
    let wanted = evaluated
        .pieces
        .iter()
        .filter(|piece| selected.iter().any(|id| is_member(piece, id)))
        .map(|piece| cad_core::provenance::canonical_key(&piece.reference))
        .collect::<Result<BTreeSet<_>>>()?;
    let edges = paths
        .iter()
        .flat_map(|wire| wire.iter())
        .map(|edge| -> Result<Option<Edge>> {
            let reference = adapter.edge_source(owner, edge.id()).ok_or_else(|| {
                invalid("A spatial path edge has no current construction identity")
            })?;
            match cad_core::provenance::canonical_key(&reference) {
                Ok(key) if wanted.contains(&key) => Ok(Some(edge.absolute_clone())),
                Ok(_) => Ok(None),
                Err(error) => Err(error),
            }
        })
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    if edges.len() != wanted.len() {
        return Err(invalid(
            "The selected spatial path lost native pieces or source ancestry",
        ));
    }
    let mut paths = assemble_paths(edges);
    if paths.len() != 1 || paths[0].is_empty() {
        return Err(unsupported(
            "A sweep/pattern needs one nonbranching spatial path; select a member or create an explicitly ordered Composite Curve",
        ));
    }
    let mut wire = paths.remove(0);
    if let Some(point) = start_vertex {
        let position = Point3::from(
            *spatial
                .solved
                .points
                .get(point)
                .ok_or_else(|| invalid("The selected spatial path start point was removed"))?,
        );
        let a = wire
            .front()
            .ok_or_else(|| invalid("The selected path is empty"))?
            .front()
            .point();
        let b = wire
            .back()
            .ok_or_else(|| invalid("The selected path is empty"))?
            .back()
            .point();
        if wire.is_closed() {
            let candidates = wire
                .iter()
                .enumerate()
                .filter(|(_, edge)| edge.front().point() == position)
                .map(|(i, _)| i)
                .collect::<Vec<_>>();
            if candidates.len() != 1 {
                return Err(invalid(
                    "The closed spatial path start must be one uniquely authored native endpoint",
                ));
            }
            let mut edges = wire.into_iter().collect::<Vec<_>>();
            edges.rotate_left(candidates[0]);
            wire = Wire::from(edges);
        } else if position == b {
            wire.invert();
        } else if position != a {
            return Err(invalid(
                "The spatial path start must be an authored terminal endpoint",
            ));
        }
    }
    let sources = wire
        .iter()
        .map(|edge| {
            adapter
                .edge_source(owner, edge.id())
                .ok_or_else(|| invalid("The selected spatial path lost its native ancestry"))
        })
        .collect::<Result<Vec<_>>>()?;
    Ok((wire, sources))
}
