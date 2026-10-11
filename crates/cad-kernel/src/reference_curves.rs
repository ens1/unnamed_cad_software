//! Exact native spatial curve carriers and application-owned construction ancestry.
use crate::adapter::{MonstertruckAdapter, Shape, TopologyHistory};
use cad_core::{
    CadError, Dimension, Document, NodeRef, Result, TopologyRef, expression,
    reference_curves::{CurveSource, Definition, Evaluated, HelixGeometry, Interpolation, Piece},
};
use monstertruck::modeling::*;
use std::collections::BTreeMap;
fn invalid(message: impl ToString) -> CadError {
    CadError::Invalid(format!("Reference curve: {}", message.to_string()))
}
fn unsupported(message: impl ToString) -> CadError {
    CadError::Unsupported(format!("Reference curve: {}", message.to_string()))
}
struct NativePiece {
    id: String,
    curve: Curve,
    sources: Vec<TopologyRef>,
    purpose: serde_json::Value,
}
fn finite(point: Point3) -> bool {
    [point.x, point.y, point.z].iter().all(|v| v.is_finite())
}
fn endpoints(curve: &Curve) -> Result<(Point3, Point3)> {
    if matches!(curve, Curve::IntersectionCurve(_)) {
        return Err(unsupported(
            "Implicit intersection curves need fallible supporting-carrier evaluation before they can be reusable composite sources",
        ));
    }
    let (a, b) = curve.range_tuple();
    if !a.is_finite() || !b.is_finite() || b <= a {
        return Err(invalid(
            "The source curve has no finite increasing bounded interval",
        ));
    }
    let first = curve.subs(a);
    let last = curve.subs(b);
    if !finite(first) || !finite(last) {
        return Err(invalid("Source curve endpoints are nonfinite"));
    }
    Ok((first, last))
}
/// A bound over the actual native carrier, independent of its authoring source.
/// This survives copied/composite/reversed paths without relying on the outer
/// feature's analytic descriptor or sampling interior points for a proof.
pub(crate) fn display_bound(curve: &Curve) -> Result<Option<f64>> {
    let Curve::ParameterCurve(parameter) = curve else {
        return Ok(None);
    };
    let Curve2D::Line(uv) = parameter.curve() else {
        return Err(unsupported(
            "Reusable analytic parameter curves need a proven linear parameter-space path",
        ));
    };
    let Surface::RevolutionSurface(processor) = parameter.surface().as_ref() else {
        return Err(unsupported(
            "Reusable analytic parameter curves need a proven revolution carrier",
        ));
    };
    let source = processor.entity();
    let Curve::Line(generator) = source.entity_curve() else {
        return Err(unsupported(
            "Reusable analytic revolution paths need a proven straight generatrix",
        ));
    };
    if !finite(generator.0) || !finite(generator.1) {
        return Err(invalid("Analytic curve generatrix endpoints are nonfinite"));
    }
    let matrix = processor.transform();
    let columns = [matrix.x, matrix.y, matrix.z, matrix.w];
    if columns.iter().any(|column| {
        ![column.x, column.y, column.z, column.w]
            .into_iter()
            .all(f64::is_finite)
    }) || matrix.x.w != 0.
        || matrix.y.w != 0.
        || matrix.z.w != 0.
        || matrix.w.w != 1.
    {
        return Err(invalid(
            "Analytic curve display bounds require a finite affine carrier transform",
        ));
    }
    let linear_norm = columns[..3]
        .iter()
        .flat_map(|column| [column.x, column.y, column.z])
        .fold(0_f64, f64::hypot);
    let axis = source.axis();
    if !axis.magnitude2().is_finite()
        || (axis.magnitude2() - 1.).abs() > 1e-10
        || !finite(source.origin())
    {
        return Err(invalid(
            "Analytic curve carrier axis must be finite and normalized",
        ));
    }
    let (start, end) = curve.range_tuple();
    let mut a = uv.subs(start);
    let mut b = uv.subs(end);
    if !processor.orientation() {
        a = Point2::new(a.y, a.x);
        b = Point2::new(b.y, b.x);
    }
    if ![a.x, a.y, b.x, b.y].into_iter().all(f64::is_finite) {
        return Err(invalid(
            "Analytic curve parameter-space extent is nonfinite",
        ));
    }
    let radial = |u| {
        let vector = generator.subs(u) - source.origin();
        vector - axis * vector.dot(axis)
    };
    let radius = radial(a.x).magnitude().max(radial(b.x).magnitude());
    let change = (generator.1 - generator.0) * (b.x - a.x);
    let radial_change = (change - axis * change.dot(axis)).magnitude();
    let angle = b.y - a.y;
    let bound = linear_norm * (radius * angle.powi(2) + 2. * angle.abs() * radial_change);
    if !bound.is_finite() || bound < 0. {
        return Err(invalid("Analytic curve second-derivative bound overflow"));
    }
    Ok(Some(bound))
}
fn model_piece(
    adapter: &MonstertruckAdapter,
    body: &NodeRef,
    reference: &TopologyRef,
    inputs: &BTreeMap<NodeRef, Shape>,
) -> Result<Vec<(Curve, TopologyRef)>> {
    let edges = adapter.selected_boundary_edges(body, std::slice::from_ref(reference), inputs)?;
    if edges.len() != 1 {
        return Err(invalid("The model-edge reference is missing or ambiguous"));
    }
    let edge = edges[0].absolute_clone();
    let source = adapter
        .edge_source(body, edge.id())
        .ok_or_else(|| invalid("The model edge has no construction ancestry"))?;
    if source.producer != reference.producer || source.role != reference.role {
        return Err(invalid(
            "Model edge must resolve by its saved construction identity",
        ));
    }
    let curve = edge.curve();
    endpoints(&curve)?;
    Ok(vec![(curve, source)])
}
pub(crate) fn source(
    adapter: &MonstertruckAdapter,
    doc: &Document,
    source: &CurveSource,
    inputs: &BTreeMap<NodeRef, Shape>,
) -> Result<Vec<(Curve, TopologyRef)>> {
    match source {
        CurveSource::ModelEdge { body, reference } => model_piece(adapter, body, reference, inputs),
        CurveSource::Curve { feature } => {
            let Some(Shape::ReferenceCurves { paths, .. }) = inputs.get(feature) else {
                return Err(invalid("Select an available reusable reference curve"));
            };
            if paths.len() != 1 {
                return Err(unsupported(
                    "Composite inputs require one reference-curve path",
                ));
            }
            paths[0]
                .iter()
                .map(|edge| {
                    let reference = adapter.edge_source(feature, edge.id()).ok_or_else(|| {
                        invalid("Reusable curve piece has no construction identity")
                    })?;
                    Ok((edge.oriented_curve(), reference))
                })
                .collect()
        }
        CurveSource::SketchMember { profile, member } => {
            let (mut sketch, plane) = crate::extrusion::source(adapter, doc, profile, inputs)?;
            if cad_core::sketch_transform::suppressed_members(&sketch).contains(member) {
                return Err(invalid("The source sketch member is suppressed"));
            }
            let original = sketch
                .members
                .get_mut(member)
                .ok_or_else(|| invalid("The source sketch member no longer exists"))?;
            original.construction = false;
            sketch.members.retain(|key, _| key == member);
            sketch.profile_order.clear();
            sketch.segments.clear();
            sketch.circles.clear();
            let built = crate::sketches::build_traced(doc, &profile.definition, &sketch, &plane)?;
            let edges: Vec<Edge> = match built.shape {
                Shape::Profile(face) => face
                    .boundaries()
                    .iter()
                    .flat_map(|wire| wire.iter().cloned())
                    .collect(),
                Shape::Sketch { faces, paths } => faces
                    .iter()
                    .flat_map(|face| face.boundaries())
                    .chain(paths.iter().cloned())
                    .flat_map(|wire| wire.into_iter())
                    .collect(),
                _ => {
                    return Err(invalid(
                        "The selected sketch source did not produce an exact curve",
                    ));
                }
            };
            let mut pieces=edges.into_iter().map(|edge| {
                let (_,piece)=built.members.get(&edge.id()).ok_or_else(||invalid("Sketch curve member ancestry is missing"))?;
                let source=TopologyRef {producer:profile.clone(),topology:"curve".into(),
                    role:format!("construction:{}",serde_json::to_string(&serde_json::json!({"sources":[],"purpose":{"operation":"sketch_member","member":member,"piece":piece},"topology":"curve"}))?),signature:None};
                Ok((*piece,edge.oriented_curve(),source))
            }).collect::<Result<Vec<_>>>()?;
            pieces.sort_by_key(|(piece, _, _)| *piece);
            if pieces.is_empty() {
                return Err(invalid("The selected source member is empty"));
            }
            Ok(pieces
                .into_iter()
                .map(|(_, curve, reference)| (curve, reference))
                .collect())
        }
    }
}
fn join(
    owner: &NodeRef,
    native: Vec<NativePiece>,
    closed: bool,
    tolerance: f64,
    history: &mut TopologyHistory,
) -> Result<(Wire, Vec<Piece>)> {
    if native.is_empty() {
        return Err(invalid("The reference curve is empty"));
    }
    if native.len() > cad_core::reference_curves::MAX_NATIVE_PIECES {
        return Err(unsupported(
            "Reference curves support at most 4096 native pieces after source expansion",
        ));
    }
    if !tolerance.is_finite() || !(1e-10..=1e-6).contains(&tolerance) {
        return Err(invalid("Join tolerance must be between 1e-10 and 1e-6 mm"));
    }
    let ends = native
        .iter()
        .map(|p| endpoints(&p.curve))
        .collect::<Result<Vec<_>>>()?;
    for pair in ends.windows(2) {
        if pair[0].1.distance(pair[1].0) > tolerance {
            return Err(invalid(
                "Ordered sources are disconnected; reverse or reorder the retained source references",
            ));
        }
    }
    if closed && ends.last().unwrap().1.distance(ends[0].0) > tolerance {
        return Err(invalid(
            "The declared closed curve has an open endpoint gap",
        ));
    }
    if !closed && ends.last().unwrap().1.distance(ends[0].0) <= tolerance {
        return Err(invalid(
            "The selected curve closes; enable Closed instead of declaring an open path",
        ));
    }
    // Authoring supplies the order. Reject repeated junctions rather than guess a branch.
    for (i, (point, _)) in ends.iter().enumerate() {
        for (other, _) in &ends[..i] {
            if point.distance(*other) <= tolerance {
                return Err(invalid(
                    "Repeated source junctions give an ambiguous/branching composite path",
                ));
            }
        }
    }
    let first = builder::vertex(ends[0].0);
    let mut current = first.clone();
    let mut edges = Vec::new();
    let mut pieces = Vec::new();
    let count = native.len();
    for (index, piece) in native.into_iter().enumerate() {
        let last = if closed && index + 1 == count {
            first.clone()
        } else {
            builder::vertex(ends[index].1)
        };
        let second_derivative_bound_mm = display_bound(&piece.curve)?;
        let edge = Edge::try_new(&current, &last, piece.curve).map_err(invalid)?;
        let reference =
            history.constructed_edge(owner, edge.id(), &piece.sources, &piece.purpose)?;
        pieces.push(Piece {
            id: piece.id,
            reference,
            start: current.point().into(),
            end: last.point().into(),
            second_derivative_bound_mm,
        });
        current = last;
        edges.push(edge);
    }
    let wire: Wire = edges.into();
    if !wire.is_continuous() || closed && !wire.is_closed() {
        return Err(invalid(
            "Native curve topology did not retain the declared connectivity",
        ));
    }
    Ok((wire, pieces))
}
fn through_points(
    adapter: &MonstertruckAdapter,
    doc: &Document,
    owner: &NodeRef,
    points: &[cad_core::reference_curves::PointRecord],
    interpolation: Interpolation,
    closed: bool,
    inputs: &BTreeMap<NodeRef, Shape>,
) -> Result<Vec<NativePiece>> {
    let positions = points
        .iter()
        .map(|point| {
            adapter
                .resolve_surface_point(doc, &owner.definition, &point.point, inputs)
                .map(Point3::from)
        })
        .collect::<Result<Vec<_>>>()?;
    let count = positions.len();
    let spans = if closed { count } else { count - 1 };
    let lengths = (0..spans)
        .map(|i| {
            let length = positions[i].distance(positions[(i + 1) % count]);
            if !length.is_finite() || length <= 1e-7 {
                Err(invalid(
                    "Adjacent curve points must be finitely separated by more than 1e-7 mm",
                ))
            } else {
                Ok(length)
            }
        })
        .collect::<Result<Vec<_>>>()?;
    let mut second = vec![Vector3::zero(); count];
    if interpolation == Interpolation::NaturalCubic {
        let mut matrix = nalgebra::DMatrix::<f64>::zeros(count, count);
        let mut rhs = nalgebra::DMatrix::<f64>::zeros(count, 3);
        for i in 0..count {
            if !closed && (i == 0 || i + 1 == count) {
                matrix[(i, i)] = 1.;
                continue;
            }
            let previous = (i + count - 1) % count;
            let next = (i + 1) % count;
            let before = lengths[previous];
            let after = lengths[i];
            matrix[(i, previous)] += before;
            matrix[(i, i)] += 2. * (before + after);
            matrix[(i, next)] += after;
            let value = 6.
                * ((positions[next] - positions[i]) / after
                    - (positions[i] - positions[previous]) / before);
            rhs[(i, 0)] = value.x;
            rhs[(i, 1)] = value.y;
            rhs[(i, 2)] = value.z;
        }
        let solved = matrix
            .clone()
            .lu()
            .solve(&rhs)
            .ok_or_else(|| invalid("The spatial spline interpolation system is singular"))?;
        let residual = &matrix * &solved - &rhs;
        let scale = rhs.iter().fold(1_f64, |s, v| s.max(v.abs()));
        if solved.iter().any(|v| !v.is_finite())
            || residual
                .iter()
                .any(|v| !v.is_finite() || v.abs() > scale * 1e-10)
        {
            return Err(invalid(
                "Spatial spline interpolation exceeds its finite residual bound",
            ));
        }
        for i in 0..count {
            second[i] = Vector3::new(solved[(i, 0)], solved[(i, 1)], solved[(i, 2)]);
        }
    }
    (0..spans).map(|i| {
        let j=(i+1)%count;let a=positions[i];let b=positions[j];let h=lengths[i];
        let curve=match interpolation {
            Interpolation::Linear=>Curve::Line(Line(a,b)),
            Interpolation::NaturalCubic=> {
                let start=(b-a)/h-h*(2.*second[i]+second[j])/6.;
                let end=(b-a)/h+h*(second[i]+2.*second[j])/6.;
                let controls=vec![a,a+start*h/3.,b-end*h/3.,b];
                if controls.iter().any(|p|!finite(*p)) {return Err(invalid("Spatial spline control overflow"));}
                Curve::BsplineCurve(BsplineCurve::new(KnotVector::bezier_knot(3),controls))
            }
        };
        Ok(NativePiece {id:format!("{}::{}",points[i].id,points[j].id),curve,sources:vec![],
            purpose:serde_json::json!({"curve_through_points":{"points":[points[i].id,points[j].id],"interpolation":interpolation}})})
    }).collect()
}
pub(crate) fn build(
    adapter: &MonstertruckAdapter,
    doc: &Document,
    owner: &NodeRef,
    definition: &Definition,
    inputs: &BTreeMap<NodeRef, Shape>,
    history: &mut TopologyHistory,
) -> Result<(Vec<Wire>, Evaluated)> {
    definition.validate()?;
    let (native, closed, tolerance, analytic) = match definition {
        Definition::Composite {
            segments,
            closed,
            join_tolerance,
        } => {
            let mut native = Vec::new();
            for segment in segments {
                let mut sources = source(adapter, doc, &segment.source, inputs)?;
                if native.len().saturating_add(sources.len())
                    > cad_core::reference_curves::MAX_NATIVE_PIECES
                {
                    return Err(unsupported(
                        "Composite source expansion exceeds 4096 native pieces; split the reusable paths",
                    ));
                }
                if segment.reverse {
                    sources.reverse();
                    for (curve, _) in &mut sources {
                        curve.invert();
                    }
                }
                for (piece, (curve, source)) in sources.into_iter().enumerate() {
                    native.push(NativePiece {
                        id: format!("{}::{piece}", segment.id),
                        curve,
                        sources: vec![source],
                        purpose: serde_json::json!({"composite":{"source":segment.id}}),
                    });
                }
            }
            (
                native,
                *closed,
                expression::value(doc, &owner.definition, join_tolerance, Dimension::Length)?,
                None,
            )
        }
        Definition::ThroughPoints {
            points,
            interpolation,
            closed,
        } => (
            through_points(adapter, doc, owner, points, *interpolation, *closed, inputs)?,
            *closed,
            1e-6,
            None,
        ),
        Definition::Helix {
            axis,
            radial,
            radius,
            end_radius,
            pitch,
            turns,
            start_angle,
            reverse,
        } => {
            let axis = adapter.resolve_surface_axis(doc, axis, inputs)?;
            let origin = Point3::from(axis.origin);
            let axis = Vector3::from(axis.direction).normalize();
            let direction = Vector3::from(adapter.resolve_surface_direction(
                doc,
                &owner.definition,
                radial,
                inputs,
            )?);
            let projection = direction - axis * direction.dot(axis);
            if !projection.magnitude2().is_finite() || projection.magnitude2() < 1e-16 {
                return Err(invalid(
                    "Start direction must have a stable component perpendicular to the helix axis",
                ));
            }
            let radial = projection.normalize();
            let quantity = |expr: &cad_core::Expr, dimension| {
                expression::value(doc, &owner.definition, expr, dimension)
            };
            let radius = quantity(radius, Dimension::Length)?;
            let end_radius = end_radius
                .as_ref()
                .map(|r| quantity(r, Dimension::Length))
                .transpose()?
                .unwrap_or(radius);
            let pitch = quantity(pitch, Dimension::Length)?;
            let turns = quantity(turns, Dimension::Scalar)?;
            let authored_angle = quantity(start_angle, Dimension::Angle)?;
            if authored_angle.abs() > std::f64::consts::TAU * 1e6 {
                return Err(invalid(
                    "Start angle exceeds the stable phase-reduction range of one million turns",
                ));
            }
            let start_angle = authored_angle.rem_euclid(std::f64::consts::TAU);
            if radius <= 1e-7 || end_radius <= 1e-7 || turns <= 0. || turns > 256. {
                return Err(invalid(
                    "Helix/Spiral needs positive radii above 1e-7 mm and 0–256 positive turns",
                ));
            }
            let height = pitch * turns;
            let angle = std::f64::consts::TAU * turns * if *reverse { -1. } else { 1. };
            let generatrix = Curve::Line(Line(
                origin + radial * radius,
                origin + radial * end_radius + axis * height,
            ));
            let surface = Surface::RevolutionSurface(Processor::new(
                RevolutionSurface::by_revolution(generatrix, origin, axis),
            ));
            let count = (turns * 4.).ceil() as usize;
            let mut native = Vec::new();
            for station in 0..count {
                let a = (station as f64 / (4. * turns)).min(1.);
                let b = ((station + 1) as f64 / (4. * turns)).min(1.);
                let curve = if height == 0. && radius == end_radius {
                    // Circular geometry has an exact positive quadratic
                    // rational representation. Its native parameter is the
                    // rational arc parameter, not a uniform angular fraction.
                    let start = start_angle + angle * a;
                    let end = start_angle + angle * b;
                    let weight = ((end - start) / 2.).cos();
                    if !weight.is_finite() || weight <= 0. {
                        return Err(invalid(
                            "Circular station needs finite positive rational weights",
                        ));
                    }
                    let radial_at =
                        |phase: f64| radial * phase.cos() + axis.cross(radial) * phase.sin();
                    let controls = vec![
                        (origin + radial_at(start) * radius).to_homogeneous(),
                        (origin.to_vec() * weight + radial_at((start + end) / 2.) * radius)
                            .extend(weight),
                        (origin + radial_at(end) * radius).to_homogeneous(),
                    ];
                    if controls
                        .iter()
                        .any(|h| ![h.x, h.y, h.z, h.w].into_iter().all(f64::is_finite))
                    {
                        return Err(invalid("Circular rational station control overflow"));
                    }
                    Curve::NurbsCurve(NurbsCurve::new(BsplineCurve::new(
                        KnotVector::bezier_knot(2),
                        controls,
                    )))
                } else {
                    Curve::ParameterCurve(ParameterCurve::new(
                        Curve2D::Line(Line(
                            Point2::new(a, start_angle + angle * a),
                            Point2::new(b, start_angle + angle * b),
                        )),
                        Box::new(surface.clone()),
                    ))
                };
                if !height.is_finite() || !angle.is_finite() {
                    return Err(invalid("Helix extent overflow"));
                }
                native.push(NativePiece {
                    id: format!("helix-station:{station}"),
                    curve,
                    sources: vec![],
                    purpose: serde_json::json!({"helix":{"station":station}}),
                });
            }
            // A zero-pitch, constant-radius multi-turn circle repeats geometry and is not a unique path.
            if height == 0. && radius == end_radius && turns > 1. {
                return Err(invalid(
                    "A circular zero-pitch path supports at most one turn; repeated coincident turns are ambiguous",
                ));
            }
            let closed = height == 0. && radius == end_radius && turns == 1.;
            let analytic = HelixGeometry {
                origin: origin.into(),
                axis: axis.into(),
                radial: radial.into(),
                radius,
                end_radius,
                height,
                angle,
                start_angle,
            };
            (native, closed, 1e-6, Some(analytic))
        }
    };
    let (wire, pieces) = join(owner, native, closed, tolerance, history)?;
    Ok((
        vec![wire],
        Evaluated {
            producer: owner.clone(),
            kind: definition.name().into(),
            closed,
            pieces,
            analytic,
        },
    ))
}
