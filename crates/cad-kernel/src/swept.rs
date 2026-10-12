//! Exact rational translation/twist and analytic tangent transport of sketch sections.
use crate::adapter::{MonstertruckAdapter, Shape, TopologyHistory};
use crate::{
    extrusion::{ThinMember, ThinSection},
    section_history::{History, Member},
};
use cad_core::{
    CadError, Dimension, Document, NodeRef, Result, SketchCurve, expression,
    solid_sections::{Orientation, Placement, SweepOptions},
};
use monstertruck::modeling::*;
use std::collections::{BTreeMap, HashMap};
const EPS: f64 = 1e-7;
fn invalid(message: impl ToString) -> CadError {
    CadError::Invalid(format!("Sweep geometry: {}", message.to_string()))
}
fn unsupported(message: &str) -> CadError {
    CadError::Unsupported(format!("Sweep geometry: {message}"))
}
fn point(h: Vector4) -> Result<Point3> {
    if ![h.x, h.y, h.z, h.w].iter().all(|v| v.is_finite()) || h.w <= 0. {
        return Err(unsupported(
            "finite positive rational path weights are required",
        ));
    }
    Ok(Point3::new(h.x / h.w, h.y / h.w, h.z / h.w))
}
struct CirclePath {
    center: Point3,
    normal: Vector3,
    sign: f64,
    full: bool,
}
struct Path {
    curves: Vec<Curve>,
    origins: Vec<Member>,
    circle: Option<CirclePath>,
}
fn source_path(
    adapter: &MonstertruckAdapter,
    doc: &Document,
    reference: Option<&NodeRef>,
    legacy: &[[f64; 3]],
    options: &SweepOptions,
    inputs: &BTreeMap<NodeRef, Shape>,
) -> Result<Path> {
    let Some(reference) = reference else {
        if options.path_member.is_some() || options.path_start_vertex.is_some() {
            return Err(invalid(
                "Selected path members and start vertices require their source sketch",
            ));
        }
        if legacy.len() < 2 {
            return Err(invalid("Select a path sketch"));
        }
        return Ok(Path {
            curves: legacy
                .windows(2)
                .map(|pair| Curve::Line(Line(Point3::from(pair[0]), Point3::from(pair[1]))))
                .collect(),
            origins: (0..legacy.len() - 1)
                .map(|index| Member {
                    sources: vec![],
                    purpose: serde_json::json!({"legacy_path_segment":index}),
                })
                .collect(),
            circle: None,
        });
    };
    if crate::sketch3d::spatial(inputs, reference).is_some() {
        let (wire, sources) = crate::sketch3d::selected_path(
            adapter,
            inputs,
            reference,
            options.path_member.as_deref(),
            options.path_start_vertex.as_deref(),
        )?;
        let curves = wire.iter().map(Edge::oriented_curve).collect::<Vec<_>>();
        let circular = curves
            .iter()
            .map(crate::axis_geometry::circle_geometry)
            .collect::<Result<Vec<_>>>();
        let circle = circular.ok().and_then(|frames| {
            let first = frames.first()?;
            if !frames.iter().all(|(center, normal, radius)| {
                center.distance(first.0) <= EPS
                    && normal.cross(first.1).magnitude() <= 1e-10
                    && (radius - first.2).abs() <= EPS
            }) {
                return None;
            }
            let curve = &curves[0];
            let parameter = curve.range_tuple().0;
            let orientation = first
                .1
                .dot((curve.subs(parameter) - first.0).cross(curve.der(parameter)));
            (orientation.is_finite() && orientation.abs() > 1e-12).then_some(CirclePath {
                center: first.0,
                normal: first.1,
                sign: orientation.signum(),
                full: wire.is_closed(),
            })
        });
        return Ok(Path {
            curves,
            origins: sources
                .into_iter()
                .map(|source| Member {
                    sources: vec![source],
                    purpose: serde_json::json!({"spatial_path":"native_piece"}),
                })
                .collect(),
            circle,
        });
    }
    if let Some(Shape::ReferenceCurves {
        paths, evaluated, ..
    }) = inputs.get(reference)
    {
        if options.path_member.is_some() || options.path_start_vertex.is_some() {
            return Err(invalid(
                "A reference curve uses its saved source order and direction; clear sketch-only Path Member and Path Start Vertex selections",
            ));
        }
        if paths.len() != 1 || paths[0].is_empty() {
            return Err(unsupported("Select one available reference-curve path"));
        }
        let wire = &paths[0];
        let identities = evaluated
            .pieces
            .iter()
            .map(|piece| cad_core::provenance::canonical_key(&piece.reference))
            .collect::<Result<std::collections::BTreeSet<_>>>()?;
        let origins = wire.iter().map(|edge| {
            let source = adapter.edge_source(reference, edge.id()).ok_or_else(|| invalid("Reference-curve path construction ancestry is unavailable"))?;
            if !identities.contains(&cad_core::provenance::canonical_key(&source)?) {
                return Err(invalid("The selected reference path does not match its current native construction identities"));
            }
            Ok(Member {sources:vec![source],purpose:serde_json::json!({"reference_path":"native_piece"})})
        }).collect::<Result<Vec<_>>>()?;
        let curves = wire.iter().map(Edge::oriented_curve).collect::<Vec<_>>();
        let circular = curves
            .iter()
            .map(crate::axis_geometry::circle_geometry)
            .collect::<Result<Vec<_>>>();
        let circle = circular.ok().and_then(|frames| {
            let first = frames.first()?;
            if !frames.iter().all(|(center, normal, radius)| {
                center.distance(first.0) <= EPS
                    && normal.cross(first.1).magnitude() <= 1e-10
                    && (radius - first.2).abs() <= EPS
            }) {
                return None;
            }
            let curve = &curves[0];
            let parameter = curve.range_tuple().0;
            let orientation = first
                .1
                .dot((curve.subs(parameter) - first.0).cross(curve.der(parameter)));
            if !orientation.is_finite() || orientation.abs() <= 1e-12 {
                return None;
            }
            Some(CirclePath {
                center: first.0,
                normal: first.1,
                sign: orientation.signum(),
                full: evaluated.closed,
            })
        });
        return Ok(Path {
            curves,
            origins,
            circle,
        });
    }
    let (mut sketch, plane) = crate::extrusion::source(adapter, doc, reference, inputs)?;
    let selected = if let Some(member) = &options.path_member {
        Some(member.clone())
    } else {
        let members = sketch
            .members
            .iter()
            .filter(|(_, m)| !m.construction)
            .map(|(key, _)| key.clone())
            .collect::<Vec<_>>();
        (members.len() == 1).then(|| members[0].clone())
    };
    let world = |p: [f64; 2]| {
        Point3::from(plane.origin) + Vector3::from(plane.u) * p[0] + Vector3::from(plane.v) * p[1]
    };
    let mut circle = None;
    let mut declared_start = None;
    if let Some(member) = &selected {
        let native = sketch
            .members
            .get(member)
            .ok_or_else(|| invalid("The selected path member no longer exists"))?
            .curve
            .clone();
        declared_start = match &native {
            SketchCurve::Line { start, .. } | SketchCurve::Arc { start, .. } => {
                Some(world(sketch.points[start]))
            }
            SketchCurve::Bezier { points } | SketchCurve::Nurbs { points, .. } => {
                points.first().map(|point| world(sketch.points[point]))
            }
            _ => None,
        };
        let normal = Vector3::from(plane.u)
            .cross(Vector3::from(plane.v))
            .normalize();
        circle = match native {
            SketchCurve::Arc {
                start,
                through,
                end,
            } => {
                let (center, _, _, angle) = cad_core::sketch_geometry::arc_frame(
                    sketch.points[&start],
                    sketch.points[&through],
                    sketch.points[&end],
                )?;
                Some(CirclePath {
                    center: world(center),
                    normal,
                    sign: angle.signum(),
                    full: false,
                })
            }
            SketchCurve::Circle { center, .. } => Some(CirclePath {
                center: world(sketch.points[&center]),
                normal,
                sign: 1.,
                full: true,
            }),
            _ => None,
        };
        if options.path_member.is_some() {
            sketch.members.retain(|key, _| key == member);
            sketch.members.get_mut(member).unwrap().construction = false;
            sketch.profile_order.clear();
            sketch.segments.clear();
            sketch.circles.clear();
        }
    }
    let start = if let Some(vertex) = &options.path_start_vertex {
        Some(world(*sketch.points.get(vertex).ok_or_else(|| {
            invalid("The selected path start vertex no longer exists")
        })?))
    } else {
        declared_start
    };
    let built = crate::sketches::build_traced(doc, &reference.definition, &sketch, &plane)?;
    let mut wire = match built.shape {
        Shape::Sketch { paths, faces } if paths.len() == 1 && faces.is_empty() => paths[0].clone(),
        Shape::Sketch { paths, faces }
            if paths.is_empty()
                && faces.len() == 1
                && faces[0].boundaries().len() == 1
                && circle.as_ref().is_some_and(|c| c.full) =>
        {
            faces[0].boundaries()[0].clone()
        }
        _ => {
            return Err(unsupported(
                "select one connected open sketch chain or one native circular member",
            ));
        }
    };
    if wire.is_empty() {
        return Err(invalid("The selected path is empty"));
    }
    if let Some(start) = start {
        let front = wire.front().unwrap().front().point();
        let back = wire.back().unwrap().back().point();
        if front.distance(start) > EPS {
            if back.distance(start) > EPS {
                return Err(invalid(
                    "Path Start Vertex must be an endpoint of the selected open chain",
                ));
            }
            wire = wire.inverse();
        }
    } else if circle.is_none() {
        return Err(invalid(
            "Choose a persistent Path Start Vertex for a multi-member chain",
        ));
    }
    let origins = wire
        .iter()
        .map(|edge| {
            let (source, piece) = built
                .members
                .get(&edge.id())
                .ok_or_else(|| invalid("The path member ancestry is unavailable"))?;
            crate::section_history::member(
                reference,
                &ThinMember {
                    sources: vec![source.clone()],
                    side: 0,
                    piece: *piece,
                    cap: false,
                },
            )
        })
        .collect::<Result<Vec<_>>>()?;
    let curves = wire.iter().map(Edge::oriented_curve).collect::<Vec<_>>();
    if let Some(circle) = &mut circle {
        let curve = &curves[0];
        let parameter = curve.range_tuple().0;
        let sign = circle
            .normal
            .dot((curve.subs(parameter) - circle.center).cross(curve.der(parameter)));
        if !sign.is_finite() || sign.abs() <= 1e-12 {
            return Err(invalid("The circular path orientation is degenerate"));
        }
        circle.sign = sign.signum();
    }
    Ok(Path {
        curves,
        origins,
        circle,
    })
}
fn trim(path: &mut Path, start: f64, end: f64, reverse: bool) -> Result<()> {
    if !start.is_finite()
        || !end.is_finite()
        || !(0. ..=1.).contains(&start)
        || !(0. ..=1.).contains(&end)
        || end - start <= 1e-9
    {
        return Err(invalid("Path range must satisfy 0 ≤ Start < End ≤ 1"));
    }
    let count = path.curves.len() as f64;
    let mut trimmed = Vec::new();
    let mut origins = Vec::new();
    for (index, source) in path.curves.iter().enumerate() {
        crate::extrusion::curve_hull(source)?;
        let lower = (start * count - index as f64).max(0.);
        let upper = (end * count - index as f64).min(1.);
        if upper - lower <= 1e-12 {
            continue;
        }
        let (a, b) = source.range_tuple();
        let mut curve = source.clone();
        if upper < 1. {
            curve.cut(a + (b - a) * upper);
        }
        if lower > 0. {
            curve = curve.cut(a + (b - a) * lower);
        }
        trimmed.push(curve);
        origins.push(path.origins[index].clone());
    }
    if trimmed.is_empty() {
        return Err(invalid("Path trimming removed every interval"));
    }
    if let Some(circle) = &mut path.circle {
        circle.full &= start == 0. && end == 1.;
        if reverse {
            circle.sign = -circle.sign;
        }
    }
    if reverse {
        trimmed.reverse();
        origins.reverse();
        for curve in &mut trimmed {
            curve.invert();
        }
    }
    path.curves = trimmed;
    path.origins = origins;
    Ok(())
}
struct Span {
    curve: BsplineCurve<Vector4>,
    twist: f64,
    origin: Member,
    piece: usize,
    divisions: usize,
}
fn spans(path: &Path, twist: f64) -> Result<Vec<Span>> {
    let subdivisions = (twist.abs() / std::f64::consts::FRAC_PI_2).ceil().max(1.) as usize;
    if subdivisions > 256 || path.curves.len() > 256 {
        return Err(unsupported(
            "path/twist segmentation exceeds the exact carrier budget",
        ));
    }
    let mut spans = Vec::new();
    for (source, origin) in path.curves.iter().zip(&path.origins) {
        if !matches!(
            source,
            Curve::Line(_) | Curve::BsplineCurve(_) | Curve::NurbsCurve(_)
        ) {
            return Err(unsupported(
                "This sweep requires exact line/B-spline/NURBS carriers; analytic helix and implicit intersection paths need an analytic sweep implementation",
            ));
        }
        let mut curve = source.lift_up();
        curve.clamp();
        curve.knot_normalize();
        let degree = curve.degree();
        if !(1..=32).contains(&degree) || curve.control_points().len() > 4096 {
            return Err(unsupported(
                "path degree or controls exceed the exact carrier budget",
            ));
        }
        let mut knots = curve
            .knot_vector()
            .iter()
            .copied()
            .filter(|t| *t > 0. && *t < 1.)
            .collect::<Vec<_>>();
        knots.extend((1..subdivisions).map(|i| i as f64 / subdivisions as f64));
        knots.sort_by(f64::total_cmp);
        knots.dedup();
        let mut lower = 0.;
        for (piece, upper) in knots.into_iter().chain(std::iter::once(1.)).enumerate() {
            let right = if upper < 1. {
                Some(curve.cut(upper))
            } else {
                None
            };
            if curve.control_points().len() != degree + 1 {
                return Err(unsupported(
                    "path span cannot be represented as exact Bernstein controls",
                ));
            }
            for h in curve.control_points() {
                point(*h)?;
            }
            curve.knot_normalize();
            spans.push(Span {
                curve,
                twist: twist * (upper - lower) / path.curves.len() as f64,
                origin: origin.clone(),
                piece,
                divisions: subdivisions,
            });
            if let Some(right) = right {
                curve = right;
            } else {
                break;
            }
            lower = upper;
        }
    }
    if spans.len() > 1024 {
        return Err(unsupported(
            "path span count exceeds the exact carrier budget",
        ));
    }
    Ok(spans)
}
fn binomial(n: usize, k: usize) -> f64 {
    (1..=k.min(n - k)).fold(1., |value, i| value * (n + 1 - i) as f64 / i as f64)
}
fn wall(
    lower: &Edge,
    upper: &Edge,
    path: &BsplineCurve<Vector4>,
    axis: Vector3,
    twist: f64,
) -> Result<NurbsSurface<Vector4>> {
    let start = point(path.control_points()[0])?;
    let m = path.degree();
    let k = (twist / 2.).tan();
    let denominator = [1., 1., 1. + k * k];
    let cosine = [1., 1., 1. - k * k];
    let sine = [0., k, 2. * k];
    let mut profile = lower.oriented_curve().lift_up();
    profile.clamp();
    let rows = profile
        .control_points()
        .iter()
        .map(|control| {
            let q = point(*control)? - start;
            let parallel = axis * q.dot(axis);
            let radial = q - parallel;
            let rotated = std::array::from_fn::<_, 3, _>(|j| {
                parallel * denominator[j] + radial * cosine[j] + axis.cross(q) * sine[j]
            });
            let mut row = vec![Vector4::zero(); m + 3];
            for (i, path) in path.control_points().iter().enumerate() {
                for j in 0..3 {
                    let coefficient = binomial(m, i) * binomial(2, j) / binomial(m + 2, i + j);
                    row[i + j] += Vector4::new(
                        path.x * denominator[j] + rotated[j].x * path.w,
                        path.y * denominator[j] + rotated[j].y * path.w,
                        path.z * denominator[j] + rotated[j].z * path.w,
                        path.w * denominator[j],
                    ) * (coefficient * control.w);
                }
            }
            for h in &row {
                point(*h)?;
            }
            Ok(row)
        })
        .collect::<Result<Vec<_>>>()?;
    let surface = NurbsSurface::new(BsplineSurface::new(
        (
            profile.knot_vector().clone(),
            KnotVector::bezier_knot(m + 2),
        ),
        rows,
    ));
    // Both cap boundaries are exact rational isoparametric boundaries, even
    // though their endpoint parameter bases may have redundant degree/knots.
    if lower
        .front()
        .point()
        .distance(surface.subs(surface.range_tuple().0.0, 0.))
        > EPS
        || upper
            .front()
            .point()
            .distance(surface.subs(surface.range_tuple().0.0, 1.))
            > EPS
    {
        return Err(invalid("Path carrier does not meet its section boundary"));
    }
    Ok(surface)
}
fn translated_sweep(
    face: &Face,
    path: &Path,
    twist: f64,
    mut origins: HashMap<EdgeId, Member>,
    history: &mut History<'_>,
) -> Result<Solid> {
    let spans = spans(path, twist)?;
    let mut normal = face.oriented_surface().normal(0., 0.).normalize();
    let first = &spans[0].curve;
    let delta = point(*first.control_points().last().unwrap())? - point(first.control_points()[0])?;
    if normal.dot(delta) < 0. {
        normal = -normal;
    }
    for span in &spans {
        let coordinates = span
            .curve
            .control_points()
            .iter()
            .map(|h| Ok(point(*h)?.to_vec().dot(normal)))
            .collect::<Result<Vec<_>>>()?;
        if coordinates.windows(2).any(|pair| pair[1] < pair[0] - 1e-10)
            || coordinates.last().unwrap() - coordinates[0] <= EPS
        {
            return Err(unsupported(
                "fixed sections need exact positive path progress through every section plane; use tangent transport for a circular path",
            ));
        }
    }
    let mut section = if face.oriented_surface().normal(0., 0.).dot(normal) < 0. {
        face.inverse()
    } else {
        face.clone()
    };
    let mut shell = Shell::from(vec![section.inverse()]);
    let sources = origins
        .values()
        .flat_map(|origin| origin.sources.clone())
        .chain(path.origins[0].sources.clone())
        .collect::<Vec<_>>();
    history.face(&shell[0], &sources, serde_json::json!({"end":"start"}))?;
    for span in spans {
        let start = point(span.curve.control_points()[0])?;
        let end = point(*span.curve.control_points().last().unwrap())?;
        let upper = builder::translated(
            &builder::rotated(&section, start, normal, Rad(span.twist)),
            end - start,
        );
        let mut connectors: HashMap<(VertexId, VertexId), Edge> = HashMap::new();
        let mut upper_origins = HashMap::new();
        for (a, b) in section.boundaries().iter().zip(upper.boundaries()) {
            for (lower, top) in a.iter().zip(&b) {
                let surface = wall(lower, top, &span.curve, normal, span.twist)?;
                let mut connector = |a: &Vertex, b: &Vertex, index: usize| -> Result<Edge> {
                    if let Some(edge) = connectors.get(&(a.id(), b.id())) {
                        return Ok(edge.clone());
                    }
                    let edge = Edge::try_new(a, b, Curve::NurbsCurve(surface.curve_v(index)))
                        .map_err(invalid)?;
                    connectors.insert((a.id(), b.id()), edge.clone());
                    Ok(edge)
                };
                let left = connector(lower.front(), top.front(), 0)?;
                let right =
                    connector(lower.back(), top.back(), surface.control_points().len() - 1)?;
                let wall = Face::try_new(
                    vec![Wire::from(vec![
                        lower.clone(),
                        right,
                        top.inverse(),
                        left.inverse(),
                    ])],
                    Surface::NurbsSurface(surface),
                )
                .map_err(invalid)?;
                let source = &origins[&lower.id()];
                let sources = source
                    .sources
                    .iter()
                    .chain(&span.origin.sources)
                    .cloned()
                    .collect::<Vec<_>>();
                history.face(&wall,&sources,serde_json::json!({"wall":source.purpose,"path":span.origin.purpose,"path_span":span.piece,"twist_divisions":span.divisions}))?;
                upper_origins.insert(top.id(), source.clone());
                shell.push(wall);
            }
        }
        section = upper;
        origins = upper_origins;
    }
    let sources = origins
        .values()
        .flat_map(|origin| origin.sources.clone())
        .chain(path.origins.last().unwrap().sources.clone())
        .collect::<Vec<_>>();
    history.face(&section, &sources, serde_json::json!({"end":"end"}))?;
    shell.push(section);
    let solid = Solid::try_new(vec![shell]).map_err(invalid)?;
    crate::validation::solid(&solid)?;
    Ok(solid)
}
fn orient(
    face: &Face,
    source: Point3,
    start: Point3,
    tangent: Vector3,
    options: &SweepOptions,
) -> Result<Face> {
    let mut face = face.clone();
    if options.placement == Placement::AtPathStart {
        if options.orientation == Orientation::FollowTangent {
            let normal = face.oriented_surface().normal(0., 0.).normalize();
            let cross = normal.cross(tangent);
            let dot = normal.dot(tangent).clamp(-1., 1.);
            if cross.magnitude() > 1e-10 {
                face = builder::rotated(&face, source, cross.normalize(), Rad(dot.acos()));
            } else if dot < 0. {
                let axis = normal
                    .cross(if normal.x.abs() < 0.8 {
                        Vector3::unit_x()
                    } else {
                        Vector3::unit_y()
                    })
                    .normalize();
                face = builder::rotated(&face, source, axis, Rad(std::f64::consts::PI));
            }
        }
        face = builder::translated(&face, start - source);
    }
    Ok(face)
}
pub(crate) struct Context<'a> {
    pub adapter: &'a MonstertruckAdapter,
    pub doc: &'a Document,
    pub owner: &'a NodeRef,
    pub inputs: &'a BTreeMap<NodeRef, Shape>,
    pub generated: &'a mut TopologyHistory,
}
pub(crate) fn sweep(
    context: Context<'_>,
    profile: &NodeRef,
    sections: &[ThinSection],
    path_sketch: Option<&NodeRef>,
    legacy: &[[f64; 3]],
    options: &SweepOptions,
) -> Result<Vec<Solid>> {
    let Context {
        adapter,
        doc,
        owner,
        inputs,
        generated,
    } = context;
    let value = |expr, dimension| expression::value(doc, &owner.definition, expr, dimension);
    let mut path = source_path(adapter, doc, path_sketch, legacy, options, inputs)?;
    trim(
        &mut path,
        value(&options.start, Dimension::Scalar)?,
        value(&options.end, Dimension::Scalar)?,
        options.reverse,
    )?;
    let roll = value(&options.roll, Dimension::Angle)?;
    let twist = value(&options.twist, Dimension::Angle)?;
    if !roll.is_finite() || !twist.is_finite() {
        return Err(invalid("Roll and twist must be finite"));
    }
    let (_, plane) = crate::extrusion::source(adapter, doc, profile, inputs)?;
    let start = path.curves[0].subs(path.curves[0].range_tuple().0);
    let tangent = path.curves[0].der(path.curves[0].range_tuple().0);
    if ![tangent.x, tangent.y, tangent.z]
        .iter()
        .all(|value| value.is_finite())
        || tangent.magnitude() <= 1e-10
    {
        return Err(invalid("The path start tangent is degenerate"));
    }
    let tangent = tangent.normalize();
    sections.iter().map(|original| {
        let original_origins=crate::section_history::members(profile,original)?;
        let mut face=orient(&original.face,Point3::from(plane.origin),start,tangent,options)?;
        let normal=face.oriented_surface().normal(0.,0.).normalize();
        if options.orientation==Orientation::FollowTangent&&(normal.cross(tangent).magnitude()>1e-8||crate::extrusion::face_hull(&face)?.iter().any(|point|(*point-start).dot(tangent).abs()>EPS)) {return Err(unsupported("tangent transport needs the section plane through the path start and perpendicular to its tangent; select At Path Start to align the source plane"));}
        let (a,b)=path.curves[0].range_tuple();
        let roll_axis=if options.orientation==Orientation::FollowTangent {tangent}else{normal*if normal.dot(path.curves[0].subs(b)-path.curves[0].subs(a))<0. {-1.}else{1.}};
        if roll.abs()>1e-12 {face=builder::rotated(&face,start,roll_axis,Rad(roll));}
        let mut origins=HashMap::new();
        for (before,after) in original.face.boundaries().iter().zip(face.boundaries()){for (before,after) in before.iter().zip(&after){origins.insert(after.id(),original_origins[&before.id()].clone());}}
        let mut history=History::new(owner,"sweep",generated);
        if options.orientation==Orientation::FollowTangent {
            if let Some(circle)=&path.circle {
                if twist.abs()>1e-12 {return Err(unsupported("twisted tangent transport on circular paths needs a rational moving-frame composition; zero twist has an exact revolution carrier"));}
                let radial=(start-circle.center).normalize();
                if crate::extrusion::face_hull(&face)?.iter().any(|p|(*p-circle.center).dot(radial)<=EPS) {return Err(unsupported("the transported section reaches the path revolution axis or lacks a positive radial control-hull clearance"));}
                let end_curve=path.curves.last().unwrap();let end=end_curve.subs(end_curve.range_tuple().1);
                let a=(start-circle.center).normalize();let b=(end-circle.center).normalize();
                let angle=if circle.full {std::f64::consts::TAU}else{(circle.sign*circle.normal.dot(a.cross(b))).atan2(a.dot(b)).rem_euclid(std::f64::consts::TAU)};
                if angle<=1e-9 {return Err(invalid("The retained circular path is degenerate"));}
                if face.oriented_surface().normal(0.,0.).dot(tangent)<0. {face.invert();}
                let solid:Solid=builder::revolve(&face,circle.center,circle.normal*circle.sign,if circle.full {builder::SweepAngle::Closed}else{builder::SweepAngle::Partial(Rad(angle))},(angle/std::f64::consts::FRAC_PI_2).ceil().max(2.) as usize);
                crate::validation::solid(&solid)?;
                let divisions=(angle/std::f64::consts::FRAC_PI_2).ceil().max(2.) as usize;
                let edges=face.boundaries().into_iter().flatten().collect::<Vec<_>>();
                let walls=solid.face_iter().filter(|wall|wall.id()!=face.id()).collect::<Vec<_>>();
                let source_closure=origins.values().flat_map(|origin|origin.sources.clone()).chain(path.origins.iter().flat_map(|origin|origin.sources.clone())).collect::<Vec<_>>();
                let mut tag=|wall:&Face,edge:&Edge,station:usize|->Result<()> {let origin=&origins[&edge.id()];let sources=origin.sources.iter().chain(path.origins.iter().flat_map(|origin|&origin.sources)).cloned().collect::<Vec<_>>();history.face(wall,&sources,serde_json::json!({"wall":origin.purpose,"angular_station":station,"angular_divisions":divisions}))};
                if circle.full {
                    let boundaries=face.boundaries();
                    if solid.boundaries().len()!=boundaries.len(){return Err(unsupported("closed circular constructor changed its contour topology"));}
                    for (shell,wire) in solid.boundaries().iter().zip(&boundaries) {
                        if shell.len()!=divisions*wire.len(){return Err(unsupported("closed circular constructor changed its native member topology"));}
                        for (index,wall) in shell.iter().enumerate(){tag(wall,&wire[index%wire.len()],index/wire.len())?;}
                    }
                }else {
                    let wall_count=divisions*edges.len();if walls.len()!=wall_count+1 {return Err(unsupported("circular constructor changed its native member topology"));}
                    for (index,wall) in walls.iter().take(wall_count).enumerate(){tag(wall,&edges[index%edges.len()],index/edges.len())?;}
                }
                if !circle.full {history.face(&face,&source_closure,serde_json::json!({"end":"start"}))?;history.face(walls.last().unwrap(),&source_closure,serde_json::json!({"end":"end"}))?;}
                history.finish(&solid)?;return Ok(solid);
            }
            if path.curves.iter().any(|curve|crate::extrusion::curve_hull(curve).map(|controls|controls.iter().any(|p|(*p-start).cross(tangent).magnitude()>EPS)).unwrap_or(true)) {return Err(unsupported("exact tangent transport currently supports one native circular path or a collinear path; general spline frames require a rational frame proof"));}
        }
        let solid=translated_sweep(&face,&path,twist,origins,&mut history)?;history.finish(&solid)?;Ok(solid)
    }).collect()
}
