//! Exact thin walls and planar tapering behind the replaceable B-rep adapter.
use crate::adapter::{KernelAdapter, MonstertruckAdapter, Shape};
use cad_core::{
    CadError, Dimension, Document, Expr, ExtrudeDraft, ExtrudeEnd, Id, NodeRef, Operation, Plane,
    Profile, Result, Sketch, SketchCurve, SketchMember, ThinExtrude, ThinExtrudeMode, expression,
    id,
    solver::{ConstraintSolver, NativeSolver},
};
use monstertruck::modeling::*;
use std::collections::{BTreeMap, BTreeSet};

fn invalid(message: impl ToString) -> CadError {
    CadError::Invalid(format!("Geometry: {}", message.to_string()))
}

pub(crate) fn curve_hull(curve: &Curve) -> Result<Vec<Point3>> {
    let points = match curve {
        Curve::Line(line) => vec![line.subs(0.), line.subs(1.)],
        Curve::BsplineCurve(curve) => curve.control_points().clone(),
        Curve::NurbsCurve(curve) => curve
            .control_points()
            .iter()
            .map(|p| {
                if ![p.x, p.y, p.z, p.w].iter().all(|v| v.is_finite()) || p.w <= 0. {
                    return Err(CadError::Unsupported(
                        "Extrusion bounds require finite positive rational weights".into(),
                    ));
                }
                Ok(Point3::new(p.x / p.w, p.y / p.w, p.z / p.w))
            })
            .collect::<Result<Vec<_>>>()?,
        _ => {
            return Err(CadError::Unsupported(
                "This extrusion boundary has no exact positive-weight control hull".into(),
            ));
        }
    };
    if points.is_empty()
        || points
            .iter()
            .any(|p| ![p.x, p.y, p.z].iter().all(|v| v.is_finite()))
    {
        return Err(invalid("Extrusion has empty or non-finite control bounds"));
    }
    Ok(points)
}
pub(crate) fn face_hull(face: &Face) -> Result<Vec<Point3>> {
    let mut points = face
        .edge_iter()
        .map(|edge| curve_hull(&edge.curve()))
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    match face.surface() {
        Surface::Plane(_) => {}
        Surface::BsplineSurface(surface) => {
            points.extend(surface.control_points().iter().flatten().copied())
        }
        Surface::NurbsSurface(surface) => {
            for p in surface.control_points().iter().flatten() {
                if ![p.x, p.y, p.z, p.w].iter().all(|v| v.is_finite()) || p.w <= 0. {
                    return Err(CadError::Unsupported(
                        "Extrusion caps require finite positive-weight surface bounds".into(),
                    ));
                }
                points.push(Point3::new(p.x / p.w, p.y / p.w, p.z / p.w));
            }
        }
        _ => {
            return Err(CadError::Unsupported(
                "Extrusion cap carrier has no exact finite control hull".into(),
            ));
        }
    }
    if points.is_empty()
        || points
            .iter()
            .any(|p| ![p.x, p.y, p.z].iter().all(|v| v.is_finite()))
    {
        return Err(invalid("Extrusion cap has non-finite bounds"));
    }
    Ok(points)
}
fn unit(direction: Vector3) -> Result<Vector3> {
    let length = direction.magnitude();
    if !length.is_finite() || length <= 1e-12 {
        return Err(invalid("Extrusion direction must be finite and nonzero"));
    }
    Ok(direction / length)
}
fn positive(value: f64) -> Result<f64> {
    if !value.is_finite() || value <= 1e-9 {
        return Err(invalid("Extrusion extent must be finite and positive"));
    }
    Ok(value)
}
fn shape_bodies<'a>(inputs: &'a BTreeMap<NodeRef, Shape>, body: &NodeRef) -> Result<&'a [Solid]> {
    match inputs.get(body) {
        Some(Shape::Bodies(solids)) if !solids.is_empty() => Ok(solids),
        _ => Err(invalid(
            "The selected extrusion scope must produce current solid bodies",
        )),
    }
}

/// Affine projection changes the section location, never its authoritative
/// sketch definition. Unlike an end projection, a start may lie behind it.
pub(crate) fn project_face(
    face: &Face,
    direction: Vector3,
    end: &Plane,
    offset: f64,
) -> Result<Face> {
    let mut normal = unit(Vector3::from(end.u).cross(Vector3::from(end.v)))?;
    if normal.dot(direction) < 0. {
        normal = -normal;
    }
    let dot = normal.dot(direction);
    if dot.abs() < 1e-9 || !offset.is_finite() {
        return Err(invalid(
            "Referenced extrusion plane must be transverse to the direction with a finite offset",
        ));
    }
    let translation = direction * ((Point3::from(end.origin).to_vec().dot(normal) - offset) / dot);
    let matrix = Matrix4::from_cols(
        (Vector3::unit_x() - direction * (normal.x / dot)).extend(0.),
        (Vector3::unit_y() - direction * (normal.y / dot)).extend(0.),
        (Vector3::unit_z() - direction * (normal.z / dot)).extend(0.),
        translation.extend(1.),
    );
    let wires = face
        .boundaries()
        .iter()
        .map(|wire| builder::transformed(wire, matrix))
        .collect::<Vec<_>>();
    let cap = builder::try_attach_plane(wires).map_err(invalid)?;
    face_hull(&cap)?;
    Ok(cap)
}
fn cap_plane(face: &Face) -> Result<Plane> {
    if !matches!(face.surface(), Surface::Plane(_)) {
        return Err(CadError::Unsupported("This cap is not planar".into()));
    }
    let surface = face.oriented_surface();
    let u = unit(surface.uder(0., 0.))?;
    let v = unit(surface.normal(0., 0.).cross(u))?;
    Ok(Plane {
        origin: surface.subs(0., 0.).into(),
        u: u.into(),
        v: v.into(),
    })
}
fn anchor(face: &Face) -> Result<Point3> {
    face.vertex_iter()
        .next()
        .map(|vertex| vertex.point())
        .ok_or_else(|| invalid("Empty extrusion profile"))
}
fn parallel_projected(a: Point3, b: Point3, axis: Vector3) -> bool {
    (b - a).cross(axis).magnitude() <= 1e-7
}

/// Caps share a boundary correspondence and are joined without an internal
/// coincident cap or a touching Boolean union. Straight projected sides use
/// exact planar carriers even when one boundary is a curved surface section.
pub(crate) fn between_caps(lower: &Face, upper: &Face, axis: Vector3) -> Result<Solid> {
    let lower = if lower.oriented_surface().normal(0., 0.).dot(axis) < 0. {
        lower.inverse()
    } else {
        lower.clone()
    };
    let upper = if upper.oriented_surface().normal(0., 0.).dot(axis) < 0. {
        upper.inverse()
    } else {
        upper.clone()
    };
    let bottoms = lower.boundaries();
    let mut tops = upper.boundaries();
    if bottoms.len() != tops.len() {
        return Err(invalid("Extrusion caps have incompatible contour topology"));
    }
    let origin = anchor(&lower)?;
    let lower_hull = face_hull(&lower)?;
    let upper_hull = face_hull(&upper)?;
    let gap = upper_hull
        .iter()
        .map(|p| (*p - origin).dot(axis))
        .fold(f64::INFINITY, f64::min)
        - lower_hull
            .iter()
            .map(|p| (*p - origin).dot(axis))
            .fold(f64::NEG_INFINITY, f64::max);
    // Boundary separation alone cannot prove that two curved interiors do not
    // intersect. Equal homogeneous surface bases permit a positive-control
    // proof; unrelated carriers need disjoint axial envelopes.
    if gap <= 1e-9
        && !(matches!(lower.surface(), Surface::Plane(_))
            && matches!(upper.surface(), Surface::Plane(_)))
        && !translated_surface_separation(&lower, &upper, axis)
    {
        return Err(CadError::Unsupported("Curved caps need positive matching surface controls or strictly separated axial envelopes".into()));
    }
    let mut shell = Shell::from(vec![lower.inverse()]);
    for (bottom, top) in bottoms.iter().zip(&mut tops) {
        if bottom.len() != top.len() {
            return Err(invalid("Extrusion caps have incompatible member counts"));
        }
        let Some(first) = bottom.front() else {
            return Err(invalid("Empty cap boundary"));
        };
        let Some(index) = top.iter().position(|edge| {
            parallel_projected(first.front().point(), edge.front().point(), axis)
                && parallel_projected(first.back().point(), edge.back().point(), axis)
        }) else {
            return Err(CadError::Unsupported(
                "Extrusion end contour does not preserve the source's projected member boundaries"
                    .into(),
            ));
        };
        top.rotate_left(index);
        let mut connectors = std::collections::HashMap::new();
        for (a, b) in bottom.iter().zip(top.iter()) {
            let ah = curve_hull(&a.oriented_curve())?;
            let bh = curve_hull(&b.oriented_curve())?;
            if !parallel_projected(a.front().point(), b.front().point(), axis)
                || !parallel_projected(a.back().point(), b.back().point(), axis)
            {
                return Err(invalid("Extrusion changes its projected contour"));
            }
            let ac = a.oriented_curve().lift_up();
            let bc = b.oriented_curve().lift_up();
            let same_parameters = ac.degree() == bc.degree()
                && ac.knot_vector() == bc.knot_vector()
                && ac.control_points().len() == bc.control_points().len()
                && ac
                    .control_points()
                    .iter()
                    .zip(bc.control_points())
                    .all(|(a, b)| (a.w - b.w).abs() <= 1e-10 * a.w.abs().max(b.w.abs()).max(1.));
            let common = same_parameters
                && ah.len() == bh.len()
                && ah
                    .iter()
                    .zip(&bh)
                    .all(|(a, b)| parallel_projected(*a, *b, axis) && (*b - *a).dot(axis) > 1e-9);
            if !common && gap <= 1e-9 {
                return Err(CadError::Unsupported(
                    "Independent curved caps require strictly separated positive control envelopes"
                        .into(),
                ));
            }
            let tangent = a.back().point() - a.front().point();
            let normal = tangent.cross(axis);
            let planar = normal.magnitude2() > 1e-18
                && ah
                    .iter()
                    .chain(&bh)
                    .all(|p| (*p - a.front().point()).dot(normal.normalize()).abs() <= 1e-7);
            if planar {
                let left = connectors
                    .entry((a.front().id(), b.front().id()))
                    .or_insert_with(|| builder::line(a.front(), b.front()))
                    .clone();
                let right = connectors
                    .entry((a.back().id(), b.back().id()))
                    .or_insert_with(|| builder::line(a.back(), b.back()))
                    .clone();
                shell.push(
                    builder::try_attach_plane(vec![Wire::from(vec![
                        a.clone(),
                        right,
                        b.inverse(),
                        left.inverse(),
                    ])])
                    .map_err(invalid)?,
                );
            } else {
                // Affine projections preserve the curve's homogeneous weights
                // and parameterization, which makes this a true straight ruling.
                if !same_parameters {
                    return Err(CadError::Unsupported(
                        "Curved profile walls need matching exact homogeneous parameterization"
                            .into(),
                    ));
                }
                let walls = builder::try_wire_homotopy::<Curve, Surface>(
                    &Wire::from(vec![a.clone()]),
                    &Wire::from(vec![b.clone()]),
                )
                .map_err(invalid)?;
                // Replace connector edges with the same objects used by adjacent
                // members, retaining the wall's exact rational carrier.
                let left = connectors
                    .entry((a.front().id(), b.front().id()))
                    .or_insert_with(|| builder::line(a.front(), b.front()))
                    .clone();
                let right = connectors
                    .entry((a.back().id(), b.back().id()))
                    .or_insert_with(|| builder::line(a.back(), b.back()))
                    .clone();
                shell.push(
                    Face::try_new(
                        vec![Wire::from(vec![
                            a.clone(),
                            right,
                            b.inverse(),
                            left.inverse(),
                        ])],
                        walls[0].oriented_surface(),
                    )
                    .map_err(invalid)?,
                );
            }
        }
    }
    // The aligned top wires still use the original cap edge identities.
    shell.push(upper);
    let solid = Solid::try_new(vec![shell]).map_err(invalid)?;
    crate::validation::solid(&solid)?;
    Ok(solid)
}

fn translated_surface_separation(lower: &Face, upper: &Face, axis: Vector3) -> bool {
    let (Surface::NurbsSurface(a), Surface::NurbsSurface(b)) = (lower.surface(), upper.surface())
    else {
        return false;
    };
    a.degrees() == b.degrees()
        && a.knot_vector_u() == b.knot_vector_u()
        && a.knot_vector_v() == b.knot_vector_v()
        && a.control_points().len() == b.control_points().len()
        && a.control_points()
            .iter()
            .zip(b.control_points())
            .all(|(a, b)| {
                a.len() == b.len()
                    && a.iter().zip(b).all(|(a, b)| {
                        if a.w <= 0.
                            || b.w <= 0.
                            || (a.w - b.w).abs() > 1e-10 * a.w.abs().max(b.w.abs()).max(1.)
                        {
                            return false;
                        }
                        let delta = Vector3::new(
                            b.x / b.w - a.x / a.w,
                            b.y / b.w - a.y / a.w,
                            b.z / b.w - a.z / a.w,
                        );
                        delta.cross(axis).magnitude() <= 1e-7 && delta.dot(axis) > 1e-9
                    })
            })
}

pub(crate) fn profile_normal(
    adapter: &MonstertruckAdapter,
    doc: &Document,
    reference: &NodeRef,
    inputs: &BTreeMap<NodeRef, Shape>,
) -> Result<Vector3> {
    let operation = doc
        .parts
        .get(&reference.definition)
        .and_then(|part| part.features.iter().find(|f| f.id == reference.node))
        .map(|f| &f.operation)
        .ok_or_else(|| invalid("Missing extrusion sketch feature"))?;
    let Operation::Profile {
        plane,
        plane_reference,
        reference_body,
        ..
    } = operation
    else {
        return Err(CadError::Unsupported(
            "Associative Sketch Normal requires a source sketch feature".into(),
        ));
    };
    let plane = adapter.sketch_plane(
        plane,
        plane_reference.as_ref(),
        reference_body.as_ref(),
        inputs,
    )?;
    let normal = Vector3::from(plane.u).cross(Vector3::from(plane.v));
    if !normal.magnitude2().is_finite() || normal.magnitude2() < 1e-18 {
        return Err(invalid("Sketch plane has no valid normal"));
    }
    Ok(normal.normalize())
}

pub(crate) fn source(
    adapter: &MonstertruckAdapter,
    doc: &Document,
    reference: &NodeRef,
    inputs: &BTreeMap<NodeRef, Shape>,
) -> Result<(Sketch, Plane)> {
    if !matches!(
        inputs.get(reference),
        Some(Shape::Sketch { .. } | Shape::Profile(_))
    ) {
        return Err(invalid(
            "The source sketch is suppressed, unavailable, or does not produce a planar profile",
        ));
    }
    let part = doc
        .parts
        .get(&reference.definition)
        .ok_or_else(|| invalid("Missing sketch part"))?;
    let feature = part
        .features
        .iter()
        .find(|f| f.id == reference.node)
        .ok_or_else(|| invalid("Missing sketch feature"))?;
    let Operation::Profile {
        profile,
        plane,
        plane_reference,
        reference_body,
    } = &feature.operation
    else {
        return Err(CadError::Unsupported(
            "Thin extrusion requires a source sketch or planar profile feature".into(),
        ));
    };
    let plane = adapter.sketch_plane(
        plane,
        plane_reference.as_ref(),
        reference_body.as_ref(),
        inputs,
    )?;
    let u = Vector3::from(plane.u);
    let v = Vector3::from(plane.v);
    if !u.magnitude2().is_finite()
        || !v.magnitude2().is_finite()
        || (u.magnitude2() - 1.).abs() > 1e-8
        || (v.magnitude2() - 1.).abs() > 1e-8
        || u.dot(v).abs() > 1e-8
    {
        return Err(CadError::Unsupported(
            "Thin walls require orthonormal sketch-plane coordinates".into(),
        ));
    }
    let mut sketch = match profile {
        Profile::Sketch { sketch } => {
            let sketch = part
                .sketches
                .get(sketch)
                .ok_or_else(|| invalid("Missing sketch definition"))?;
            let mut sketch = adapter.prepare_sketch(sketch, &plane, inputs)?;
            NativeSolver.solve_sketch(doc, &part.id, &mut sketch)?;
            sketch
        }
        Profile::Polygon { points } => {
            let mut sketch = empty_sketch();
            for (index, p) in points.iter().enumerate() {
                let point = format!("p{index}");
                sketch.points.insert(point.clone(), *p);
                sketch.profile_order.push(point);
            }
            sketch
        }
        Profile::Circle { radius, center } => {
            let mut sketch = empty_sketch();
            let point = "center".to_string();
            sketch.points.insert(point.clone(), *center);
            sketch.members.insert(
                "circle".into(),
                SketchMember {
                    construction: false,
                    curve: SketchCurve::Circle {
                        center: point,
                        radius: radius.clone(),
                    },
                },
            );
            sketch
        }
    };
    // Solved generated geometry is the exact input. Temporary derivation IDs never
    // enter the native definition; the extrusion's source and intent stay stable.
    for member in sketch.members.keys().cloned().collect::<Vec<_>>() {
        let mut curve = cad_core::sketch_edit::effective_curve(&sketch, &member)?;
        let controls = match &curve {
            SketchCurve::Bezier { points } => Some(points),
            SketchCurve::Nurbs {
                points, weights, ..
            } if weights.iter().all(|w| w.is_finite() && *w > 0.) => Some(points),
            _ => None,
        };
        if let Some(points) = controls {
            let a = sketch.points[&points[0]];
            let b = sketch.points[points.last().unwrap()];
            let d = [b[0] - a[0], b[1] - a[1]];
            let size = d[0] * d[0] + d[1] * d[1];
            let mut previous = -1e-9;
            let straight = size > 1e-18
                && points.iter().all(|id| {
                    let p = sketch.points[id];
                    let delta = [p[0] - a[0], p[1] - a[1]];
                    let t = (delta[0] * d[0] + delta[1] * d[1]) / size;
                    let valid = t.is_finite()
                        && t >= previous
                        && t <= 1. + 1e-9
                        && (delta[0] - d[0] * t).hypot(delta[1] - d[1] * t) < 1e-8;
                    previous = t;
                    valid
                });
            if straight {
                curve = SketchCurve::Line {
                    start: points[0].clone(),
                    end: points.last().unwrap().clone(),
                };
            }
        }
        sketch.members.get_mut(&member).unwrap().curve = curve;
    }
    let suppressed = cad_core::sketch_transform::suppressed_members(&sketch);
    sketch.members.retain(|key, _| !suppressed.contains(key));
    sketch.conversions.clear();
    sketch.offsets.clear();
    sketch.fillets.clear();
    sketch.splines.clear();
    sketch.conics.clear();
    sketch.texts.clear();
    sketch.equations.clear();
    sketch.blocks.clear();
    sketch.block_instances.clear();
    sketch.chamfers.clear();
    sketch.transforms.clear();
    sketch.constraints.clear();
    sketch.dimension_groups.clear();
    if !sketch.profile_order.is_empty() {
        for (index, (a, b)) in sketch
            .profile_order
            .iter()
            .zip(sketch.profile_order.iter().cycle().skip(1))
            .take(sketch.profile_order.len())
            .enumerate()
        {
            sketch.members.insert(
                format!("legacy-profile-edge-{index}"),
                SketchMember {
                    construction: false,
                    curve: SketchCurve::Line {
                        start: a.clone(),
                        end: b.clone(),
                    },
                },
            );
        }
    } else {
        for (index, [a, b]) in sketch.segments.iter().enumerate() {
            sketch.members.insert(
                format!("legacy-segment-{index}"),
                SketchMember {
                    construction: false,
                    curve: SketchCurve::Line {
                        start: a.clone(),
                        end: b.clone(),
                    },
                },
            );
        }
    }
    for (key, circle) in &sketch.circles {
        sketch.members.insert(
            format!("legacy-circle-{key}"),
            SketchMember {
                construction: false,
                curve: SketchCurve::Circle {
                    center: circle.center.clone(),
                    radius: circle.radius.clone(),
                },
            },
        );
    }
    sketch.profile_order.clear();
    sketch.segments.clear();
    sketch.circles.clear();
    Ok((sketch, plane))
}

fn empty_sketch() -> Sketch {
    Sketch {
        id: id(),
        name: "Thin extrusion section".into(),
        points: BTreeMap::new(),
        segments: vec![],
        constraints: vec![],
        profile_order: vec![],
        circles: BTreeMap::new(),
        members: BTreeMap::new(),
        conversions: BTreeMap::new(),
        offsets: BTreeMap::new(),
        fillets: BTreeMap::new(),
        chamfers: BTreeMap::new(),
        transforms: BTreeMap::new(),
        splines: BTreeMap::new(),
        conics: BTreeMap::new(),
        texts: BTreeMap::new(),
        equations: BTreeMap::new(),
        blocks: BTreeMap::new(),
        block_instances: BTreeMap::new(),
        dimension_groups: BTreeMap::new(),
    }
}

fn ends(curve: &SketchCurve) -> Result<(&Id, &Id)> {
    match curve {
        SketchCurve::Line { start, end } | SketchCurve::Arc { start, end, .. } => Ok((start, end)),
        _ => Err(CadError::Unsupported("Thin extrusion supports nonbranching straight/circular-arc chains and circles; ellipse and spline offsets are not available".into())),
    }
}

fn chains(sketch: &Sketch) -> Result<Vec<Vec<Id>>> {
    fn endpoints(curve: &SketchCurve) -> Result<(&Id, &Id)> {
        match curve {
            SketchCurve::Line { start, end } | SketchCurve::Arc { start, end, .. } => {
                Ok((start, end))
            }
            SketchCurve::Bezier { points } | SketchCurve::Nurbs { points, .. } => points
                .first()
                .zip(points.last())
                .ok_or_else(|| invalid("A selected chain has an empty spline")),
            _ => Err(invalid(
                "A closed circular member cannot be part of an endpoint chain",
            )),
        }
    }
    let mut pending = sketch
        .members
        .iter()
        .filter(|(_, m)| !m.construction)
        .map(|(id, _)| id.clone())
        .collect::<BTreeSet<_>>();
    let mut groups = Vec::new();
    while let Some(first) = pending.pop_first() {
        if matches!(
            sketch.members[&first].curve,
            SketchCurve::Circle { .. } | SketchCurve::Ellipse { .. }
        ) {
            groups.push(vec![first]);
            continue;
        }
        let (a, b) = endpoints(&sketch.members[&first].curve)?;
        let mut points = BTreeSet::from([a.clone(), b.clone()]);
        let mut group = vec![first];
        loop {
            let mut found = Vec::new();
            for member in &pending {
                if matches!(
                    sketch.members[member].curve,
                    SketchCurve::Circle { .. } | SketchCurve::Ellipse { .. }
                ) {
                    continue;
                }
                let (a, b) = endpoints(&sketch.members[member].curve)?;
                if points.contains(a) || points.contains(b) {
                    found.push(member.clone());
                }
            }
            if found.is_empty() {
                break;
            }
            for member in found {
                pending.remove(&member);
                let (a, b) = endpoints(&sketch.members[&member].curve)?;
                points.extend([a.clone(), b.clone()]);
                group.push(member);
            }
        }
        groups.push(group);
    }
    if groups.is_empty() {
        return Err(invalid("The sketch has no material members"));
    }
    Ok(groups)
}

fn side(
    sketch: &mut Sketch,
    selected: &[Id],
    distance: f64,
    reverse: bool,
    doc: &Document,
    owner: &str,
) -> Result<(Vec<Id>, Option<[Id; 2]>)> {
    if distance == 0. {
        let (ordered, closed) = cad_core::sketch_edit::offset_selection(sketch, selected)?;
        let endpoints = if closed {
            None
        } else {
            let (first, reversed) = ordered.first().unwrap();
            let (a, b) = ends(&sketch.members[first].curve)?;
            let start = if *reversed { b } else { a }.clone();
            let (last, reversed) = ordered.last().unwrap();
            let (a, b) = ends(&sketch.members[last].curve)?;
            Some([start, if *reversed { a } else { b }.clone()])
        };
        return Ok((ordered.into_iter().map(|(id, _)| id).collect(), endpoints));
    }
    let key = cad_core::sketch_edit::add_offset(
        sketch,
        selected,
        Expr(format!("{distance} mm")),
        reverse,
        false,
    )?;
    cad_core::sketch_edit::derive(doc, owner, sketch)?;
    let operation = &sketch.offsets[&key];
    let endpoints = if operation.closed {
        None
    } else {
        Some([
            operation.members.first().unwrap().points[0].clone(),
            operation
                .members
                .last()
                .unwrap()
                .points
                .last()
                .unwrap()
                .clone(),
        ])
    };
    Ok((
        operation.members.iter().map(|m| m.member.clone()).collect(),
        endpoints,
    ))
}

#[derive(Clone)]
pub(crate) struct ThinMember {
    pub sources: Vec<Id>,
    pub side: usize,
    pub piece: usize,
    pub cap: bool,
}
#[derive(Clone)]
pub(crate) struct ThinSection {
    pub face: Face,
    pub members: std::collections::HashMap<EdgeId, ThinMember>,
}
fn validate_native_member(doc: &Document, profile: &NodeRef, member: &str) -> Result<()> {
    let part = doc
        .parts
        .get(&profile.definition)
        .ok_or_else(|| invalid("The selected contour's source part no longer exists"))?;
    let feature = part
        .features
        .iter()
        .find(|feature| feature.id == profile.node)
        .ok_or_else(|| invalid("The selected contour's source profile no longer exists"))?;
    let Operation::Profile {
        profile: Profile::Sketch { sketch },
        ..
    } = &feature.operation
    else {
        return Err(invalid(
            "Selected Contours require native sketch member identities; legacy profile records remain available with All Contours",
        ));
    };
    let sketch = part
        .sketches
        .get(sketch)
        .ok_or_else(|| invalid("The selected contour's source sketch no longer exists"))?;
    let native = sketch.members.get(member).ok_or_else(|| {
        invalid("A selected contour member is missing from its authoritative source sketch")
    })?;
    if native.construction
        || cad_core::sketch_transform::suppressed_members(sketch).contains(member)
    {
        return Err(invalid(
            "Construction or suppressed geometry cannot select a material contour",
        ));
    }
    Ok(())
}
fn selection_requests<'a>(
    doc: &Document,
    source: &Sketch,
    profile: &NodeRef,
    selections: &'a [cad_core::contours::Selection],
) -> Result<Vec<&'a cad_core::contours::Selection>> {
    let requests = selections
        .iter()
        .filter(|selection| &selection.profile == profile)
        .collect::<Vec<_>>();
    for selection in &requests {
        if selection.outer_members.is_empty() {
            return Err(invalid("Selected Contours need an outer-boundary member"));
        }
        for member in &selection.outer_members {
            validate_native_member(doc, profile, member)?;
            let native=source.members.get(member).ok_or_else(||invalid("A selected contour member was deleted, suppressed, or is no longer native material geometry"))?;
            if native.construction {
                return Err(invalid(
                    "Construction geometry cannot select a material contour",
                ));
            }
        }
    }
    Ok(requests)
}
pub(crate) fn outer_members(section: &ThinSection) -> BTreeSet<Id> {
    section.face.boundaries()[0]
        .iter()
        .flat_map(|edge| section.members[&edge.id()].sources.iter().cloned())
        .collect()
}
pub(crate) fn region_for_member(
    doc: &Document,
    profile: &NodeRef,
    sections: &[ThinSection],
    member: &str,
) -> Result<usize> {
    validate_native_member(doc, profile, member)?;
    let regions = sections
        .iter()
        .enumerate()
        .filter(|(_, section)| outer_members(section).contains(member))
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    if regions.len() != 1 {
        return Err(invalid(
            "A loft region member must identify exactly one current outer material boundary; missing, inner-hole, or split ancestry needs a new selection",
        ));
    }
    Ok(regions[0])
}
fn filter_sections(
    sections: Vec<ThinSection>,
    requests: &[&cad_core::contours::Selection],
) -> Result<Vec<ThinSection>> {
    if requests.is_empty() {
        return Ok(sections);
    }
    let mut selected = Vec::new();
    for selection in requests {
        let regions = sections
            .iter()
            .enumerate()
            .filter(|(_, section)| {
                let members = outer_members(section);
                selection
                    .outer_members
                    .iter()
                    .all(|member| members.contains(member))
            })
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        if regions.len() != 1 {
            return Err(invalid(
                "A Selected Contour must identify exactly one outer material boundary; holes follow their outer region automatically",
            ));
        }
        if !selected.contains(&regions[0]) {
            selected.push(regions[0]);
        }
    }
    let mut sections = sections.into_iter().map(Some).collect::<Vec<_>>();
    Ok(selected
        .into_iter()
        .map(|index| sections[index].take().unwrap())
        .collect())
}
pub(crate) fn section_intent(profile: &NodeRef, section: &ThinSection) -> serde_json::Value {
    let source_members = section
        .members
        .values()
        .flat_map(|member| member.sources.iter().cloned())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let contours=section.face.boundaries().iter().map(|wire|wire.iter().map(|edge| {
        let member=&section.members[&edge.id()];serde_json::json!({"source_members":member.sources,"side":member.side,"piece":member.piece,"cap":member.cap})
    }).collect::<Vec<_>>()).collect::<Vec<_>>();
    serde_json::json!({"profile":profile,"source_members":source_members,"contours":contours})
}
fn canonical_section(mut section: ThinSection) -> Result<ThinSection> {
    let mut boundaries = section.face.boundaries();
    if boundaries.len() > 2 {
        boundaries[1..].sort_by_key(|wire| {
            wire.iter()
                .flat_map(|edge| section.members[&edge.id()].sources.iter().cloned())
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>()
        });
        section.face =
            Face::try_new(boundaries, section.face.oriented_surface()).map_err(invalid)?;
    }
    Ok(section)
}
pub(crate) fn profiles_traced(
    adapter: &MonstertruckAdapter,
    doc: &Document,
    reference: &NodeRef,
    inputs: &BTreeMap<NodeRef, Shape>,
    selections: &[cad_core::contours::Selection],
) -> Result<Vec<ThinSection>> {
    let (source, plane) = source(adapter, doc, reference, inputs)?;
    let requests = selection_requests(doc, &source, reference, selections)?;
    let built = crate::sketches::build_traced(doc, &reference.definition, &source, &plane)?;
    let Shape::Sketch { faces, paths } = built.shape else {
        return Err(invalid("Expected native source sketch sections"));
    };
    if faces.is_empty() || (!paths.is_empty() && requests.is_empty()) {
        return Err(invalid(
            "Solid sections need closed sketch material contours; use Selected Contours to exclude unrelated open chains",
        ));
    }
    let sections = faces
        .into_iter()
        .map(|face| {
            let members =
                face.edge_iter()
                    .map(|edge| {
                        let (source, piece) = built.members.get(&edge.id()).ok_or_else(|| {
                            invalid("Native source member provenance is unavailable")
                        })?;
                        Ok((
                            edge.id(),
                            ThinMember {
                                sources: vec![source.clone()],
                                side: 0,
                                piece: *piece,
                                cap: false,
                            },
                        ))
                    })
                    .collect::<Result<_>>()?;
            canonical_section(ThinSection { face, members })
        })
        .collect::<Result<Vec<_>>>()?;
    filter_sections(sections, &requests)
}
pub(crate) fn thin_profiles_traced(
    adapter: &MonstertruckAdapter,
    doc: &Document,
    reference: &NodeRef,
    thin: &ThinExtrude,
    inputs: &BTreeMap<NodeRef, Shape>,
    owner: &str,
    selections: &[cad_core::contours::Selection],
) -> Result<Vec<ThinSection>> {
    let value = |e: &Expr| expression::value(doc, owner, e, Dimension::Length);
    let thickness = value(&thin.thickness)?;
    if !thickness.is_finite() || thickness <= 1e-9 {
        return Err(invalid("Thin-feature thickness must be positive"));
    }
    let (first, second) = match thin.mode {
        ThinExtrudeMode::OneSide => (thickness, 0.),
        ThinExtrudeMode::MidPlane => (thickness / 2., thickness / 2.),
        ThinExtrudeMode::TwoSide => {
            let second = value(
                thin.second_thickness
                    .as_ref()
                    .ok_or_else(|| invalid("Enter the second thin-feature thickness"))?,
            )?;
            if !second.is_finite() || second <= 1e-9 {
                return Err(invalid("The second thickness must be positive"));
            }
            (thickness, second)
        }
    };
    let (source, plane) = source(adapter, doc, reference, inputs)?;
    let requests = selection_requests(doc, &source, reference, selections)?;
    let mut groups = chains(&source)?;
    if !requests.is_empty() {
        let mut selected = BTreeSet::new();
        for selection in &requests {
            let matches = groups
                .iter()
                .enumerate()
                .filter(|(_, group)| {
                    selection
                        .outer_members
                        .iter()
                        .all(|member| group.contains(member))
                })
                .map(|(index, _)| index)
                .collect::<Vec<_>>();
            if matches.len() != 1 {
                return Err(invalid(
                    "A thin Selected Contour must identify one connected source chain",
                ));
            }
            selected.insert(matches[0]);
        }
        groups = groups
            .into_iter()
            .enumerate()
            .filter(|(index, _)| selected.contains(index))
            .map(|(_, group)| group)
            .collect();
    }
    for group in &groups {
        cad_core::sketch_edit::offset_selection(&source, group)?;
    }
    let mut section = empty_sketch();
    let mut lineage = BTreeMap::new();
    let mut point_ids = BTreeMap::new();
    for (group, selected) in groups.into_iter().enumerate() {
        let mut sketch = source.clone();
        let (a, a_ends) = side(
            &mut sketch,
            &selected,
            first,
            thin.reverse,
            doc,
            &reference.definition,
        )?;
        let (b, b_ends) = side(
            &mut sketch,
            &selected,
            second,
            !thin.reverse,
            doc,
            &reference.definition,
        )?;
        let mut canonical_point = |old: &Id| -> Id {
            let count = point_ids.len();
            let key = point_ids
                .entry(old.clone())
                .or_insert_with(|| format!("thin-point-{count:06}"));
            section.points.insert(key.clone(), sketch.points[old]);
            key.clone()
        };
        for (index, (side, member)) in a
            .into_iter()
            .map(|member| (0, member))
            .chain(b.into_iter().map(|member| (1, member)))
            .enumerate()
        {
            let mut geometry = sketch.members[&member].clone();
            geometry.construction = false;
            geometry.curve = match geometry.curve {
                SketchCurve::Line { start, end } => SketchCurve::Line {
                    start: canonical_point(&start),
                    end: canonical_point(&end),
                },
                SketchCurve::Arc {
                    start,
                    through,
                    end,
                } => SketchCurve::Arc {
                    start: canonical_point(&start),
                    through: canonical_point(&through),
                    end: canonical_point(&end),
                },
                SketchCurve::Circle { center, radius } => SketchCurve::Circle {
                    center: canonical_point(&center),
                    radius,
                },
                _ => {
                    return Err(CadError::Unsupported(
                        "Thin section offsets require line, arc, or circle geometry".into(),
                    ));
                }
            };
            let key = format!("thin-member-{group:06}-{index:06}");
            let source = sketch
                .offsets
                .values()
                .flat_map(|offset| &offset.members)
                .find(|derived| derived.member == member)
                .map(|derived| derived.source.clone())
                .unwrap_or(member);
            lineage.insert(key.clone(), (vec![source], side, false));
            section.members.insert(key, geometry);
        }
        if let (Some(a), Some(b)) = (a_ends, b_ends) {
            for (index, (a, b)) in a.into_iter().zip(b).enumerate() {
                let key = format!("thin-cap-{group:06}-{index:06}");
                lineage.insert(key.clone(), (selected.clone(), index, true));
                section.members.insert(
                    key,
                    SketchMember {
                        construction: false,
                        curve: SketchCurve::Line {
                            start: canonical_point(&a),
                            end: canonical_point(&b),
                        },
                    },
                );
            }
        }
    }
    let built = crate::sketches::build_traced(doc, &reference.definition, &section, &plane)?;
    let sections = match built.shape {
        Shape::Sketch { faces, paths } if !faces.is_empty() && paths.is_empty() => faces
            .into_iter()
            .map(|face| {
                let members = face
                    .edge_iter()
                    .map(|edge| {
                        let (member, piece) = built.members.get(&edge.id()).ok_or_else(|| {
                            invalid("Thin section member provenance is unavailable")
                        })?;
                        let (sources, side, cap) = &lineage[member];
                        Ok((
                            edge.id(),
                            ThinMember {
                                sources: sources.clone(),
                                side: *side,
                                piece: *piece,
                                cap: *cap,
                            },
                        ))
                    })
                    .collect::<Result<_>>()?;
                canonical_section(ThinSection { face, members })
            })
            .collect(),
        _ => Err(invalid(
            "Thin-feature offsets did not produce closed wall sections",
        )),
    }?;
    filter_sections(sections, &requests)
}

pub(crate) fn sketch_vertex(
    adapter: &MonstertruckAdapter,
    doc: &Document,
    reference: &NodeRef,
    vertex: &str,
    inputs: &BTreeMap<NodeRef, Shape>,
) -> Result<Point3> {
    let (sketch, plane) = source(adapter, doc, reference, inputs)?;
    let point = sketch
        .points
        .get(vertex)
        .ok_or_else(|| invalid("The extrusion end vertex no longer exists"))?;
    Ok(Point3::from(plane.origin)
        + Vector3::from(plane.u) * point[0]
        + Vector3::from(plane.v) * point[1])
}

/// Project a planar section along the extrusion direction onto another plane.
/// Affine curve projection and rational ruled walls preserve the exact curves.
pub(crate) fn to_plane(
    face: &Face,
    direction: Vector3,
    start: f64,
    end: &Plane,
    offset: f64,
) -> Result<Solid> {
    if !start.is_finite() || !offset.is_finite() {
        return Err(invalid("Extrusion offsets must be finite"));
    }
    let bottom = builder::translated(face, direction * start);
    let base_normal = bottom.oriented_surface().normal(0., 0.).normalize();
    if !matches!(bottom.surface(), Surface::Plane(_)) || base_normal.dot(direction).abs() < 1e-9 {
        return Err(invalid(
            "Up to Face requires a planar section transverse to its extrusion direction",
        ));
    }
    let bottom = if base_normal.dot(direction) < 0. {
        bottom.inverse()
    } else {
        bottom
    };
    let mut n = Vector3::from(end.u).cross(Vector3::from(end.v)).normalize();
    if n.dot(direction) < 0. {
        n = -n;
    }
    let dot = n.dot(direction);
    if !dot.is_finite() || dot.abs() < 1e-9 {
        return Err(invalid(
            "The end face is parallel to the extrusion direction",
        ));
    }
    let origin = Point3::from(end.origin);
    for edge in bottom.edge_iter() {
        let controls =
            match edge.curve() {
                Curve::Line(line) => vec![line.subs(0.), line.subs(1.)],
                Curve::BsplineCurve(curve) => curve.control_points().clone(),
                Curve::NurbsCurve(curve) => curve
                    .control_points()
                    .iter()
                    .map(|p| {
                        if !p.w.is_finite() || p.w <= 0. {
                            return Err(CadError::Unsupported(
                                "Up to Face requires positive-weight curve bounds".into(),
                            ));
                        }
                        Ok(Point3::new(p.x / p.w, p.y / p.w, p.z / p.w))
                    })
                    .collect::<Result<Vec<_>>>()?,
                _ => return Err(CadError::Unsupported(
                    "Up to Face cannot project this intersection or parameter curve representation"
                        .into(),
                )),
            };
        if controls
            .iter()
            .any(|p| ((origin - *p).dot(n) - offset) / dot <= 1e-9)
        {
            return Err(invalid(
                "The end plane meets or lies behind the profile's exact control hull; choose another face, offset, or direction",
            ));
        }
    }
    let translation = direction * (origin.to_vec().dot(n) - offset) / dot;
    let project = Matrix4::from_cols(
        (Vector3::unit_x() - direction * (n.x / dot)).extend(0.),
        (Vector3::unit_y() - direction * (n.y / dot)).extend(0.),
        (Vector3::unit_z() - direction * (n.z / dot)).extend(0.),
        translation.extend(1.),
    );
    let lower = bottom.boundaries();
    let upper = lower
        .iter()
        .map(|wire| builder::transformed(wire, project))
        .collect::<Vec<_>>();
    let top = builder::try_attach_plane(upper.clone()).map_err(invalid)?;
    let mut shell = Shell::from(vec![bottom.inverse()]);
    for (a, b) in lower.iter().zip(&upper) {
        shell.extend(builder::try_wire_homotopy::<Curve, Surface>(a, b).map_err(invalid)?);
    }
    shell.push(top);
    let solid = Solid::try_new(vec![shell]).map_err(invalid)?;
    crate::validation::solid(&solid)?;
    Ok(solid)
}

// Parallel-offset lines create exact planar tapered walls. The sketch plane is
// the neutral plane, including when material extends on both sides of it.
pub(crate) fn drafted(
    face: &Face,
    direction: Vector3,
    start: f64,
    depth: f64,
    draft: &ExtrudeDraft,
    doc: &Document,
    owner: &str,
) -> Result<Solid> {
    if !start.is_finite() || !depth.is_finite() || depth <= 1e-9 {
        return Err(invalid(
            "Extrusion start and depth must be finite, with a positive depth",
        ));
    }
    let angle = expression::value(doc, owner, &draft.angle, Dimension::Angle)?;
    if !angle.is_finite() || angle.abs() >= std::f64::consts::FRAC_PI_2 - 1e-6 {
        return Err(invalid(
            "Extrusion draft angle must be between -90 and 90 degrees",
        ));
    }
    if angle.abs() < 1e-12 {
        return between_caps(
            &builder::translated(face, direction * start),
            &builder::translated(face, direction * (start + depth)),
            direction,
        );
    }
    let surface = face.oriented_surface();
    if !matches!(surface, Surface::Plane(_)) {
        return Err(CadError::Unsupported(
            "Drafted extrusion requires a planar profile".into(),
        ));
    }
    let normal = surface.normal(0., 0.).normalize();
    if normal.cross(direction).magnitude2() > 1e-12 {
        return Err(CadError::Unsupported(
            "Drafted extrusion currently requires the sketch normal as its direction".into(),
        ));
    }
    let face = if normal.dot(direction) < 0. {
        face.inverse()
    } else {
        face.clone()
    };
    let u = face.oriented_surface().uder(0., 0.).normalize();
    let v = direction.cross(u).normalize();
    let origin = face
        .vertex_iter()
        .next()
        .ok_or_else(|| invalid("Empty extrusion profile"))?
        .point();
    let slope = angle.tan() * if draft.outward { -1. } else { 1. };
    let mut lower = Vec::new();
    let mut upper = Vec::new();
    for wire in face.boundaries() {
        let points = wire.iter().map(|edge| {
            if !straight(edge) {
                return Err(CadError::Unsupported("Extrusion draft supports straight-edge profiles; circular and spline tapering is not available".into()));
            }
            let p = edge.front().point()-origin;
            Ok([p.dot(u),p.dot(v)])
        }).collect::<Result<Vec<_>>>()?;
        let offset = |level: f64| -> Result<Wire> {
            let mut shifted = Vec::new();
            for i in 0..points.len() {
                let a = points[i];
                let b = points[(i + 1) % points.len()];
                let dx = b[0] - a[0];
                let dy = b[1] - a[1];
                let length = dx.hypot(dy);
                if length <= 1e-9 {
                    return Err(invalid("A draft boundary edge is collapsed"));
                }
                let delta = [-dy / length * level * slope, dx / length * level * slope];
                shifted.push((
                    [a[0] + delta[0], a[1] + delta[1]],
                    [dx / length, dy / length],
                ));
            }
            let mut result = Vec::new();
            for i in 0..points.len() {
                let (a, d) = shifted[(i + points.len() - 1) % points.len()];
                let (b, e) = shifted[i];
                let cross = d[0] * e[1] - d[1] * e[0];
                if cross.abs() <= 1e-10 {
                    return Err(CadError::Unsupported(
                        "Draft profiles need distinct corners; merge collinear sketch members"
                            .into(),
                    ));
                }
                let delta = [b[0] - a[0], b[1] - a[1]];
                let t = (delta[0] * e[1] - delta[1] * e[0]) / cross;
                result.push([a[0] + d[0] * t, a[1] + d[1] * t]);
            }
            for i in 0..points.len() {
                let a = result[i];
                let b = result[(i + 1) % points.len()];
                let d = shifted[i].1;
                if (b[0] - a[0]) * d[0] + (b[1] - a[1]) * d[1] <= 1e-9 {
                    return Err(invalid(
                        "Draft collapses or reverses a profile edge; reduce depth or angle",
                    ));
                }
            }
            let vertices = result
                .into_iter()
                .map(|p| builder::vertex(origin + u * p[0] + v * p[1] + direction * level))
                .collect::<Vec<_>>();
            Ok((0..vertices.len())
                .map(|i| builder::line(&vertices[i], &vertices[(i + 1) % vertices.len()]))
                .collect())
        };
        lower.push(offset(start)?);
        upper.push(offset(start + depth)?);
    }
    validate_sections(&lower, origin, u, v)?;
    validate_sections(&upper, origin, u, v)?;
    let bottom = builder::try_attach_plane(lower.clone()).map_err(invalid)?;
    let top = builder::try_attach_plane(upper.clone()).map_err(invalid)?;
    let mut shell = Shell::from(vec![bottom.inverse()]);
    for (lower, upper) in lower.iter().zip(&upper) {
        shell.extend(builder::try_wire_homotopy::<Curve, Surface>(lower, upper).map_err(invalid)?);
    }
    shell.push(top);
    let solid = Solid::try_new(vec![shell]).map_err(invalid)?;
    crate::validation::solid(&solid)?;
    Ok(solid)
}

fn straight(edge: &Edge) -> bool {
    let controls = match edge.oriented_curve() {
        Curve::Line(_) => return true,
        Curve::BsplineCurve(curve) => curve.control_points().clone(),
        Curve::NurbsCurve(curve) => {
            if curve
                .control_points()
                .iter()
                .any(|p| !p.w.is_finite() || p.w <= 0.)
            {
                return false;
            }
            curve
                .control_points()
                .iter()
                .map(|p| Point3::new(p.x / p.w, p.y / p.w, p.z / p.w))
                .collect()
        }
        _ => return false,
    };
    let origin = edge.front().point();
    let delta = edge.back().point() - origin;
    if delta.magnitude2() <= 1e-18 {
        return false;
    }
    let mut previous = -1e-9;
    for point in controls {
        let distance = (point - origin).dot(delta) / delta.magnitude2();
        if !distance.is_finite()
            || (point - origin - delta * distance).magnitude2() > 1e-16
            || distance < previous
            || distance > 1. + 1e-9
        {
            return false;
        }
        previous = distance;
    }
    true
}

fn validate_sections(wires: &[Wire], origin: Point3, u: Vector3, v: Vector3) -> Result<()> {
    let loops = wires
        .iter()
        .map(|wire| {
            wire.iter()
                .map(|edge| {
                    let p = edge.front().point() - origin;
                    [p.dot(u), p.dot(v)]
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let cross = |a: [f64; 2], b: [f64; 2], c: [f64; 2]| {
        (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
    };
    let hit = |a: [f64; 2], b: [f64; 2], c: [f64; 2], d: [f64; 2]| {
        let on = |a: [f64; 2], b: [f64; 2], p: [f64; 2]| {
            cross(a, b, p).abs() < 1e-9
                && (0..2).all(|i| p[i] >= a[i].min(b[i]) - 1e-9 && p[i] <= a[i].max(b[i]) + 1e-9)
        };
        (cross(a, b, c) * cross(a, b, d) < 0. && cross(c, d, a) * cross(c, d, b) < 0.)
            || on(a, b, c)
            || on(a, b, d)
            || on(c, d, a)
            || on(c, d, b)
    };
    for (i, points) in loops.iter().enumerate() {
        let area = points
            .iter()
            .zip(points.iter().cycle().skip(1))
            .take(points.len())
            .map(|(a, b)| a[0] * b[1] - a[1] * b[0])
            .sum::<f64>();
        if !area.is_finite() || (i == 0 && area <= 1e-9) || (i > 0 && area >= -1e-9) {
            return Err(invalid("Draft collapses or reverses a section boundary"));
        }
        for a in 0..points.len() {
            for b in a + 1..points.len() {
                if b == a + 1 || (a == 0 && b == points.len() - 1) {
                    continue;
                }
                if hit(
                    points[a],
                    points[(a + 1) % points.len()],
                    points[b],
                    points[(b + 1) % points.len()],
                ) {
                    return Err(invalid(
                        "Draft causes a section boundary to cross or touch itself",
                    ));
                }
            }
            for other in loops.iter().skip(i + 1) {
                for b in 0..other.len() {
                    if hit(
                        points[a],
                        points[(a + 1) % points.len()],
                        other[b],
                        other[(b + 1) % other.len()],
                    ) {
                        return Err(invalid(
                            "Draft causes the outer wall and hole boundaries to intersect",
                        ));
                    }
                }
            }
        }
    }
    let inside = |boundary: &[[f64; 2]], point: [f64; 2]| {
        let mut inside = false;
        for (a, b) in boundary
            .iter()
            .zip(boundary.iter().cycle().skip(1))
            .take(boundary.len())
        {
            if (a[1] > point[1]) != (b[1] > point[1])
                && point[0] < (b[0] - a[0]) * (point[1] - a[1]) / (b[1] - a[1]) + a[0]
            {
                inside = !inside;
            }
        }
        inside
    };
    for (i, hole) in loops.iter().enumerate().skip(1) {
        if !inside(&loops[0], hole[0])
            || loops
                .iter()
                .enumerate()
                .skip(1)
                .any(|(j, other)| i != j && inside(other, hole[0]))
        {
            return Err(invalid(
                "Draft no longer preserves the section's material and hole regions",
            ));
        }
    }
    Ok(())
}

struct ExtentContext<'a> {
    adapter: &'a MonstertruckAdapter,
    doc: &'a Document,
    owner: &'a NodeRef,
    inputs: &'a BTreeMap<NodeRef, Shape>,
    axis: Vector3,
    scope: &'a [Solid],
    tolerance: f64,
}
impl ExtentContext<'_> {
    fn length(&self, value: &Expr) -> Result<f64> {
        expression::value(self.doc, &self.owner.definition, value, Dimension::Length)
    }
    fn referenced_cap(
        &self,
        section: &Face,
        start: &Face,
        target: &cad_core::TopologyRef,
        body: Option<&NodeRef>,
        offset: f64,
    ) -> Result<Face> {
        if target.topology == "plane" {
            let plane =
                self.adapter
                    .sketch_plane(&Plane::default(), Some(target), body, self.inputs)?;
            return project_face(section, self.axis, &plane, offset);
        }
        let target = self.adapter.selected_boundary_face(
            body.unwrap_or(&target.producer),
            target,
            self.inputs,
        )?;
        if matches!(target.surface(), Surface::Plane(_)) {
            project_face(section, self.axis, &cap_plane(&target)?, offset)
        } else {
            if offset.abs() > 1e-12 {
                return Err(CadError::Unsupported("Normal offsets of a curved extrusion end require an exact offset carrier; use zero offset or a separately authored offset surface".into()));
            }
            crate::extrusion_surface::cap(section, start, self.axis, &target)
        }
    }
    fn end(
        &self,
        start: &Face,
        section: &Face,
        end: &cad_core::extrusion::DirectionEnd,
    ) -> Result<Face> {
        let offset = self.length(&end.end_offset)?;
        let cap = match end.end {
            ExtrudeEnd::Blind => {
                builder::translated(start, self.axis * positive(self.length(&end.distance)?)?)
            }
            ExtrudeEnd::ThroughAll => {
                if self.scope.is_empty() {
                    return Err(invalid("Through All requires an explicit solid-body scope"));
                }
                let upper = self
                    .scope
                    .iter()
                    .map(crate::adapter::solid_control_points)
                    .collect::<Result<Vec<_>>>()?
                    .into_iter()
                    .flatten()
                    .map(|p| p.to_vec().dot(self.axis))
                    .fold(f64::NEG_INFINITY, f64::max);
                let origin = anchor(start)?;
                let plane = axis_plane(
                    origin
                        + self.axis
                            * (upper - origin.to_vec().dot(self.axis)
                                + (self.tolerance * 4.).max(0.001)),
                    self.axis,
                )?;
                project_face(start, self.axis, &plane, 0.)?
            }
            ExtrudeEnd::UpToFace => self.referenced_cap(
                section,
                start,
                end.target
                    .as_ref()
                    .ok_or_else(|| invalid("Select an end face or datum plane"))?,
                end.target_body.as_ref(),
                offset,
            )?,
            ExtrudeEnd::UpToVertex => {
                let (profile, point) = end
                    .target_vertex
                    .as_ref()
                    .ok_or_else(|| invalid("Select an end sketch vertex"))?;
                let point = sketch_vertex(self.adapter, self.doc, profile, point, self.inputs)?;
                project_face(start, self.axis, &axis_plane(point, self.axis)?, offset)?
            }
            ExtrudeEnd::UpToNext | ExtrudeEnd::UpToBody => {
                let target = self.inputs;
                let solids = if end.end == ExtrudeEnd::UpToBody {
                    shape_bodies(
                        target,
                        end.target_body
                            .as_ref()
                            .ok_or_else(|| invalid("Select the Up to Body termination body"))?,
                    )?
                } else {
                    self.scope
                };
                self.next_cap(start, section, solids, offset)?
            }
            ExtrudeEnd::MidPlane | ExtrudeEnd::ThroughAllBoth => {
                return Err(invalid(
                    "Mid-plane and Through All Both are combined direction-one conditions; use independent single-direction conditions for Direction 2",
                ));
            }
        };
        // Reconstructing this transient solid also verifies positive separation
        // and every cap/wall's true supporting geometry before acceptance.
        between_caps(start, &cap, self.axis)?;
        Ok(cap)
    }
    fn next_cap(
        &self,
        start: &Face,
        section: &Face,
        solids: &[Solid],
        offset: f64,
    ) -> Result<Face> {
        if solids.is_empty() {
            return Err(invalid(
                "Up to Next/Body requires an explicit current solid scope",
            ));
        }
        let mut candidates = Vec::new();
        for face in solids.iter().flat_map(Solid::face_iter) {
            let cap = if matches!(face.surface(), Surface::Plane(_)) {
                match project_face(section, self.axis, &cap_plane(face)?, 0.) {
                    Ok(cap) => cap,
                    Err(_) => continue,
                }
            } else {
                if offset.abs() > 1e-12 {
                    continue;
                }
                match crate::extrusion_surface::cap(section, start, self.axis, face) {
                    Ok(cap) => cap,
                    Err(CadError::Unsupported(_) | CadError::Invalid(_)) => continue,
                    Err(error) => return Err(error),
                }
            };
            if matches!(face.surface(), Surface::Plane(_))
                && !crate::extrusion_surface::covered(face, &cap)?
                || between_caps(start, &cap, self.axis).is_err()
            {
                continue;
            }
            candidates.push(cap);
        }
        if candidates.is_empty() {
            return Err(CadError::Unsupported("Up to Next/Body could not prove a complete forward end face for this section; intersecting several faces or an unrecognized curved carrier needs exact segmented termination".into()));
        }
        for index in 0..candidates.len() {
            let nearest = candidates.iter().enumerate().all(|(other, cap)| {
                other == index || cap_precedes(&candidates[index], cap, self.axis).unwrap_or(false)
            });
            if nearest {
                let cap = candidates.remove(index);
                if offset.abs() <= 1e-12 {
                    return Ok(cap);
                }
                if !matches!(cap.surface(), Surface::Plane(_)) {
                    return Err(CadError::Unsupported(
                        "Up to Next/Body normal offsets require a planar termination face".into(),
                    ));
                }
                return project_face(section, self.axis, &cap_plane(&cap)?, offset);
            }
        }
        Err(CadError::Unsupported("Several termination faces overlap in the extrusion direction; exact piecewise end-face construction is required".into()))
    }
}
fn axis_plane(origin: Point3, axis: Vector3) -> Result<Plane> {
    let u = unit(axis.cross(if axis.x.abs() < 0.8 {
        Vector3::unit_x()
    } else {
        Vector3::unit_y()
    }))?;
    Ok(Plane {
        origin: origin.into(),
        u: u.into(),
        v: axis.cross(u).into(),
    })
}
fn proven_disjoint(a: &Solid, b: &Solid) -> bool {
    crate::adapter::solid_control_points(a)
        .ok()
        .zip(crate::adapter::solid_control_points(b).ok())
        .is_some_and(|(a, b)| crate::extrusion_surface::bounds_disjoint(&a, &b, 0.))
}
fn exterior_caps(
    solid: &Solid,
    lower: &Face,
    upper: &Face,
    axis: Vector3,
    tolerance: f64,
) -> Result<(Face, Face)> {
    let hull = match crate::adapter::solid_control_points(solid) {
        Ok(hull) => hull,
        Err(CadError::Unsupported(_)) => return Ok((lower.clone(), upper.clone())),
        Err(error) => return Err(error),
    };
    let padding = (tolerance * 4.).max(0.001);
    let mut ends = [lower.clone(), upper.clone()];
    for (index, cap) in ends.iter_mut().enumerate() {
        if !matches!(cap.surface(), Surface::Plane(_)) {
            continue;
        }
        let plane = cap_plane(cap)?;
        let mut normal = unit(Vector3::from(plane.u).cross(Vector3::from(plane.v)))?;
        if normal.dot(axis) < 0. {
            normal = -normal;
        }
        let dot = normal.dot(axis);
        if dot < 1e-9 {
            continue;
        }
        let origin = Point3::from(plane.origin);
        let distances = hull
            .iter()
            .map(|p| (*p - origin).dot(normal))
            .collect::<Vec<_>>();
        let exterior = if index == 0 {
            distances.iter().all(|d| *d >= -1e-7)
        } else {
            distances.iter().all(|d| *d <= 1e-7)
        };
        if exterior && distances.iter().any(|d| d.abs() <= 1e-7) {
            *cap = builder::translated(
                cap,
                axis * (if index == 0 {
                    -padding / dot
                } else {
                    padding / dot
                }),
            );
        }
    }
    let [lower, upper] = ends;
    Ok((lower, upper))
}
pub(crate) fn cap_precedes(a: &Face, b: &Face, axis: Vector3) -> Result<bool> {
    if matches!(a.surface(), Surface::Plane(_)) && matches!(b.surface(), Surface::Plane(_)) {
        let ah = face_hull(a)?;
        let plane = cap_plane(b)?;
        let n = unit(Vector3::from(plane.u).cross(Vector3::from(plane.v)))?;
        let dot = n.dot(axis);
        if dot.abs() < 1e-9 {
            return Ok(false);
        }
        return Ok(ah
            .iter()
            .all(|p| (Point3::from(plane.origin) - *p).dot(n) / dot >= -1e-7));
    }
    let ah = face_hull(a)?;
    let bh = face_hull(b)?;
    let origin = anchor(a)?;
    Ok(ah
        .iter()
        .map(|p| (*p - origin).dot(axis))
        .fold(f64::NEG_INFINITY, f64::max)
        <= bh
            .iter()
            .map(|p| (*p - origin).dot(axis))
            .fold(f64::INFINITY, f64::min)
            + 1e-7)
}

fn cut_history(
    adapter: &MonstertruckAdapter,
    owner: &NodeRef,
    scope: &BTreeSet<NodeRef>,
    section: &serde_json::Value,
    output: &crate::planar::CutOutput,
    generated: &mut crate::adapter::TopologyHistory,
) -> Result<()> {
    let mut identities = BTreeSet::new();
    for tag in &output.faces {
        let sources=tag.sources.iter().map(|(face,_)|*face).chain(tag.partition_sources.iter().copied()).map(|face|generated.face_source(face).or_else(||scope.iter().find_map(|body|adapter.face_source(body,face))).ok_or_else(||CadError::Unsupported("Exact cut source-face provenance is unavailable; reselect its current authored source".into()))).collect::<Result<Vec<_>>>()?;
        let face = output
            .bodies
            .iter()
            .flat_map(Solid::face_iter)
            .find(|face| face.id() == tag.face)
            .ok_or_else(|| invalid("Exact cut constructor lost a named face"))?;
        let reference = generated.tool_face(
            owner,
            face,
            &sources,
            &serde_json::json!({"operation":"extrusion_cut","scope":section["scope_body_sources"]}),
        )?;
        if !identities.insert(reference.role) {
            return Err(CadError::Unsupported("Disconnected cut faces share ambiguous source ancestry; explicit split topology reattachment is required".into()));
        }
    }
    let mut edges: std::collections::HashMap<EdgeId, (Edge, Vec<cad_core::TopologyRef>)> =
        std::collections::HashMap::new();
    for face in output.bodies.iter().flat_map(Solid::face_iter) {
        let source = generated
            .face_source(face.id())
            .or_else(|| {
                scope
                    .iter()
                    .find_map(|body| adapter.face_source(body, face.id()))
            })
            .ok_or_else(|| {
                CadError::Unsupported("Exact cut output lost its source-face ancestry".into())
            })?;
        for edge in face.edge_iter() {
            edges
                .entry(edge.id())
                .or_insert_with(|| (edge.clone(), vec![]))
                .1
                .push(source.clone());
        }
    }
    let mut identities = BTreeSet::new();
    for (edge, sources) in edges.values() {
        let reference = if let Some(reference) = generated.edge_source(edge.id()).or_else(|| {
            scope
                .iter()
                .find_map(|body| adapter.edge_source(body, edge.id()))
        }) {
            generated.inherit_edge(edge.id(), reference.clone());
            reference
        } else {
            generated.tool_edge(owner,edge,sources,&serde_json::json!({"operation":"extrusion_cut","constructor":"shared_boundary","scope":section["scope_body_sources"]}))?
        };
        if !identities.insert(reference.role) {
            return Err(CadError::Unsupported("Disconnected cut boundaries share ambiguous source ancestry; explicit split topology reattachment is required".into()));
        }
    }
    Ok(())
}
fn join_history(
    adapter: &MonstertruckAdapter,
    owner: &NodeRef,
    scope: &BTreeSet<NodeRef>,
    output: &crate::join::ContactOutput,
    generated: &mut crate::adapter::TopologyHistory,
) -> Result<()> {
    let face_source = |history: &crate::adapter::TopologyHistory, face: FaceId| {
        history.face_source(face).or_else(|| {
            scope
                .iter()
                .find_map(|body| adapter.face_source(body, face))
        })
    };
    for (mapped, original) in &output.face_aliases {
        let reference = face_source(generated, *original).ok_or_else(|| {
            CadError::Unsupported(
                "Planar boss join lost a retained tool face's source ancestry".into(),
            )
        })?;
        generated.inherit_face(*mapped, reference);
    }
    for (mapped, original) in &output.edge_aliases {
        let reference = generated
            .edge_source(*original)
            .or_else(|| {
                scope
                    .iter()
                    .find_map(|body| adapter.edge_source(body, *original))
            })
            .ok_or_else(|| {
                CadError::Unsupported(
                    "Planar boss join lost a retained tool edge's source ancestry".into(),
                )
            })?;
        generated.inherit_edge(*mapped, reference);
    }
    for tag in &output.trimmed {
        let sources = tag
            .sources
            .iter()
            .map(|face| {
                face_source(generated, *face).ok_or_else(|| {
                    CadError::Unsupported(
                        "Planar boss contact trim has no current source-face ancestry".into(),
                    )
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let face = output
            .solid
            .face_iter()
            .find(|face| face.id() == tag.face)
            .ok_or_else(|| invalid("Planar boss join lost a constructed contact trim"))?;
        generated.tool_face(
            owner,
            face,
            &sources,
            &serde_json::json!({"operation":"extrusion_join","constructor":"contact_trim"}),
        )?;
    }
    let mut identities = BTreeSet::new();
    let mut edges: std::collections::HashMap<EdgeId, (Edge, Vec<cad_core::TopologyRef>)> =
        std::collections::HashMap::new();
    for face in output.solid.face_iter() {
        let source = face_source(generated, face.id()).ok_or_else(|| {
            CadError::Unsupported(
                "Planar boss join output has no current source-face ancestry".into(),
            )
        })?;
        if !identities.insert(crate::adapter::construction_reference(&source)?.to_string()) {
            return Err(CadError::Unsupported(
                "Planar boss join has ambiguous retained face ancestry".into(),
            ));
        }
        generated.inherit_face(face.id(), source.clone());
        for edge in face.edge_iter() {
            edges
                .entry(edge.id())
                .or_insert_with(|| (edge.clone(), vec![]))
                .1
                .push(source.clone());
        }
    }
    identities.clear();
    for (edge, sources) in edges.values() {
        let reference = if let Some(reference) = generated.edge_source(edge.id()).or_else(|| {
            scope
                .iter()
                .find_map(|body| adapter.edge_source(body, edge.id()))
        }) {
            generated.inherit_edge(edge.id(), reference.clone());
            reference
        } else {
            generated.tool_edge(
                owner,
                edge,
                sources,
                &serde_json::json!({"operation":"extrusion_join","constructor":"shared_boundary"}),
            )?
        };
        if !identities.insert(crate::adapter::construction_reference(&reference)?.to_string()) {
            return Err(CadError::Unsupported(
                "Planar boss join has ambiguous retained edge ancestry".into(),
            ));
        }
    }
    Ok(())
}
fn scoped_section_identity(
    adapter: &MonstertruckAdapter,
    scope: &BTreeSet<NodeRef>,
    section: &serde_json::Value,
    stock: &Solid,
    generated: &crate::adapter::TopologyHistory,
) -> Result<serde_json::Value> {
    let mut sources = stock
        .face_iter()
        .filter_map(|face| {
            generated.face_source(face.id()).or_else(|| {
                scope
                    .iter()
                    .find_map(|body| adapter.face_source(body, face.id()))
            })
        })
        .map(|reference| {
            crate::adapter::construction_reference(&reference).map(|value| value.to_string())
        })
        .collect::<Result<Vec<_>>>()?;
    sources.sort();
    sources.dedup();
    if sources.is_empty() {
        return Err(CadError::Unsupported(
            "Scoped extrusion body provenance is unavailable".into(),
        ));
    }
    let sources = sources
        .iter()
        .map(|value| serde_json::from_str::<serde_json::Value>(value))
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(serde_json::json!({"section":section,"scope_body_sources":sources}))
}
pub(crate) fn operation(
    adapter: &MonstertruckAdapter,
    doc: &Document,
    owner: &NodeRef,
    operation: &Operation,
    inputs: &BTreeMap<NodeRef, Shape>,
    tolerance: f64,
    generated: &mut crate::adapter::TopologyHistory,
) -> Result<Shape> {
    let Operation::Extrude {
        profile,
        distance,
        direction,
        body,
        mode,
        options,
    } = operation
    else {
        return Err(invalid("Expected an extrusion operation"));
    };
    cad_core::contours::validate_profiles(std::slice::from_ref(profile), &options.contours)?;
    let sections = if let Some(thin) = &options.thin {
        thin_profiles_traced(
            adapter,
            doc,
            profile,
            thin,
            inputs,
            &owner.definition,
            &options.contours,
        )?
    } else {
        profiles_traced(adapter, doc, profile, inputs, &options.contours)?
    };
    let axis = if options.profile_normal {
        profile_normal(adapter, doc, profile, inputs)?
            * if options.reverse_direction { -1. } else { 1. }
    } else {
        unit(Vector3::from(*direction))?
    };
    let scope_refs = body
        .iter()
        .chain(&options.scope)
        .cloned()
        .collect::<BTreeSet<_>>();
    let mut scope: Vec<Solid> = Vec::new();
    for reference in &scope_refs {
        for solid in shape_bodies(inputs, reference)? {
            let identity = solid
                .face_iter()
                .map(Face::id)
                .collect::<std::collections::HashSet<_>>();
            if !scope.iter().any(|other| {
                other
                    .face_iter()
                    .map(Face::id)
                    .collect::<std::collections::HashSet<_>>()
                    == identity
            }) {
                scope.push(solid.clone());
            }
        }
    }
    if scope.is_empty() && !matches!(mode.as_str(), "" | "new") {
        return Err(invalid("Select a target body for the extrusion result"));
    }
    if !scope.is_empty()
        && !matches!(
            mode.as_str(),
            "" | "new" | "join" | "union" | "cut" | "difference" | "intersect" | "intersection"
        )
    {
        return Err(invalid("Unknown extrusion result mode"));
    }
    let context = ExtentContext {
        adapter,
        doc,
        owner,
        inputs,
        axis,
        scope: &scope,
        tolerance,
    };
    let first = cad_core::extrusion::DirectionEnd {
        end: options.end.clone(),
        distance: distance.clone(),
        target: options.target.clone(),
        target_body: options.target_body.clone(),
        target_vertex: options.target_vertex.clone(),
        end_offset: options.end_offset.clone(),
    };
    let second = if let Some(end) = &options.direction2 {
        Some(end.clone())
    } else if options.end == ExtrudeEnd::Blind {
        options
            .reverse_distance
            .as_ref()
            .map(|distance| cad_core::extrusion::DirectionEnd {
                distance: distance.clone(),
                ..Default::default()
            })
    } else {
        None
    };
    if second.is_some()
        && matches!(
            options.end,
            ExtrudeEnd::MidPlane | ExtrudeEnd::ThroughAllBoth
        )
    {
        return Err(invalid(
            "Disable Direction 2 when using Mid-plane or Through All Both",
        ));
    }
    let intersection = matches!(mode.as_str(), "intersect" | "intersection");
    let mut result = if intersection { vec![] } else { scope.clone() };
    for traced in &sections {
        let section_key = section_intent(profile, traced);
        let section = &traced.face;
        let start_offset = context.length(&options.start_offset)?;
        let start = match &options.start {
            cad_core::extrusion::Start::SketchPlane => {
                builder::translated(section, axis * start_offset)
            }
            cad_core::extrusion::Start::Vertex { profile, point } => {
                let target = sketch_vertex(adapter, doc, profile, point, inputs)?;
                let normal = unit(section.oriented_surface().normal(0., 0.))?;
                let dot = normal.dot(axis);
                if dot.abs() < 1e-9 {
                    return Err(invalid(
                        "The extrusion direction must cross its source sketch plane",
                    ));
                }
                builder::translated(
                    section,
                    axis * ((target - anchor(section)?).dot(normal) / dot + start_offset),
                )
            }
            cad_core::extrusion::Start::Face { face, body } => builder::translated(
                &context.referenced_cap(section, section, face, Some(body), 0.)?,
                axis * start_offset,
            ),
            cad_core::extrusion::Start::Point { point } => {
                let target = Point3::from(adapter.resolve_surface_point(
                    doc,
                    &owner.definition,
                    point,
                    inputs,
                )?);
                let normal = unit(section.oriented_surface().normal(0., 0.))?;
                let dot = normal.dot(axis);
                if dot.abs() < 1e-9 {
                    return Err(invalid(
                        "The extrusion direction must cross its source sketch plane",
                    ));
                }
                builder::translated(
                    section,
                    axis * ((target - anchor(section)?).dot(normal) / dot + start_offset),
                )
            }
        };
        let (lower, upper) = match options.end {
            ExtrudeEnd::MidPlane => {
                let depth = positive(context.length(distance)?)?;
                (
                    builder::translated(&start, -axis * depth / 2.),
                    builder::translated(&start, axis * depth / 2.),
                )
            }
            ExtrudeEnd::ThroughAllBoth => {
                if options.start != cad_core::extrusion::Start::SketchPlane {
                    return Err(invalid(
                        "Through All Both starts from the sketch plane; use two independent Through All ends for a referenced start",
                    ));
                }
                let controls = scope
                    .iter()
                    .map(crate::adapter::solid_control_points)
                    .collect::<Result<Vec<_>>>()?
                    .into_iter()
                    .flatten()
                    .collect::<Vec<_>>();
                if controls.is_empty() {
                    return Err(invalid("Through All Both requires an explicit solid scope"));
                }
                let origin = anchor(section)?;
                let margin = (tolerance * 4.).max(0.001);
                let lo = controls
                    .iter()
                    .map(|p| (*p - origin).dot(axis))
                    .fold(f64::INFINITY, f64::min)
                    - margin;
                let hi = controls
                    .iter()
                    .map(|p| (*p - origin).dot(axis))
                    .fold(f64::NEG_INFINITY, f64::max)
                    + margin;
                (
                    project_face(section, axis, &axis_plane(origin + axis * lo, axis)?, 0.)?,
                    project_face(section, axis, &axis_plane(origin + axis * hi, axis)?, 0.)?,
                )
            }
            _ => {
                let upper = context.end(&start, section, &first)?;
                let lower = if let Some(second) = &second {
                    let reverse = ExtentContext {
                        axis: -axis,
                        ..context
                    };
                    reverse.end(&start, section, second)?
                } else {
                    start.clone()
                };
                (lower, upper)
            }
        };
        let tool = if let Some(draft) = &options.draft {
            if !matches!(lower.surface(), Surface::Plane(_))
                || !matches!(upper.surface(), Surface::Plane(_))
                || lower
                    .oriented_surface()
                    .normal(0., 0.)
                    .normalize()
                    .cross(axis)
                    .magnitude2()
                    > 1e-12
                || upper
                    .oriented_surface()
                    .normal(0., 0.)
                    .normalize()
                    .cross(axis)
                    .magnitude2()
                    > 1e-12
            {
                return Err(CadError::Unsupported("Draft with an oblique or curved start/end needs exact tapered-surface trimming".into()));
            }
            drafted(
                section,
                axis,
                (anchor(&lower)? - anchor(section)?).dot(axis),
                (anchor(&upper)? - anchor(&lower)?).dot(axis),
                draft,
                doc,
                &owner.definition,
            )?
        } else {
            between_caps(&lower, &upper, axis)?
        };
        let tag_options = || crate::section_history::ToolContext {
            axis,
            drafted: options.draft.is_some(),
        };
        if !matches!(mode.as_str(), "cut" | "difference") {
            crate::section_history::extrusion(
                owner,
                profile,
                traced,
                &tool,
                tag_options(),
                &serde_json::Value::Null,
                generated,
            )?;
        }
        if matches!(mode.as_str(), "" | "new") || scope.is_empty() {
            result.push(tool);
            continue;
        }
        if matches!(mode.as_str(), "join" | "union") {
            // Carry one shared tool through all intersecting scoped bodies.
            // A separate union per body would duplicate the tool's material.
            let mut joined = tool;
            let mut retained = Vec::new();
            for solid in result {
                if proven_disjoint(&solid, &joined) {
                    retained.push(solid);
                    continue;
                }
                if let Some(contact) = crate::join::contact(&solid, &joined)? {
                    join_history(adapter, owner, &scope_refs, &contact, generated)?;
                    joined = contact.solid;
                    continue;
                }
                joined = monstertruck::solid::or(&solid, &joined, tolerance).map_err(invalid)?;
                crate::validation::solid(&joined)?;
            }
            retained.push(joined);
            result = retained;
            continue;
        }
        let mut next = if intersection {
            std::mem::take(&mut result)
        } else {
            vec![]
        };
        let stock = if intersection {
            scope.clone()
        } else {
            std::mem::take(&mut result)
        };
        for solid in stock {
            if proven_disjoint(&solid, &tool) {
                if matches!(mode.as_str(), "cut" | "difference") {
                    next.push(solid);
                }
                continue;
            }
            match mode.as_str() {
                "cut" | "difference" => {
                    let scoped_key = scoped_section_identity(
                        adapter,
                        &scope_refs,
                        &section_key,
                        &solid,
                        generated,
                    )?;
                    let scope_identity = &scoped_key["scope_body_sources"];
                    crate::section_history::extrusion(
                        owner,
                        profile,
                        traced,
                        &tool,
                        tag_options(),
                        scope_identity,
                        generated,
                    )?;
                    if options.draft.is_none()
                        && let Some(cut) = crate::planar::insert_cut(&solid, &lower, &upper, axis)?
                    {
                        if let Some(tool) = &cut.tool {
                            crate::section_history::extrusion(
                                owner,
                                profile,
                                traced,
                                tool,
                                tag_options(),
                                scope_identity,
                                generated,
                            )?;
                        }
                        cut_history(adapter, owner, &scope_refs, &scoped_key, &cut, generated)?;
                        next.extend(cut.bodies);
                        continue;
                    }
                    if let Some(cut) = crate::planar::convex_difference(&solid, &tool)? {
                        cut_history(adapter, owner, &scope_refs, &scoped_key, &cut, generated)?;
                        next.extend(cut.bodies);
                        continue;
                    }
                    let (cut_lower, cut_upper) =
                        exterior_caps(&solid, &lower, &upper, axis, tolerance)?;
                    let cutter = if let Some(draft) = &options.draft {
                        drafted(
                            section,
                            axis,
                            (anchor(&cut_lower)? - anchor(section)?).dot(axis),
                            (anchor(&cut_upper)? - anchor(&cut_lower)?).dot(axis),
                            draft,
                            doc,
                            &owner.definition,
                        )?
                    } else {
                        between_caps(&cut_lower, &cut_upper, axis)?
                    };
                    crate::section_history::extrusion(
                        owner,
                        profile,
                        traced,
                        &cutter,
                        tag_options(),
                        scope_identity,
                        generated,
                    )?;
                    let cut = monstertruck::solid::difference(&solid, &cutter, tolerance)
                        .map_err(invalid)?;
                    crate::validation::solid(&cut)?;
                    next.push(cut);
                }
                "intersect" | "intersection" => {
                    let common =
                        monstertruck::solid::and(&solid, &tool, tolerance).map_err(invalid)?;
                    crate::validation::solid(&common)?;
                    next.push(common);
                }
                _ => return Err(invalid("Unknown extrusion result mode")),
            }
        }
        result = next;
    }
    for solid in &result {
        crate::validation::solid(solid)?;
    }
    Ok(Shape::Bodies(result))
}
