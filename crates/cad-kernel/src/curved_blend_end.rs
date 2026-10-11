//! Exact cylindrical terminations for a straight planar fillet. The terminal
//! curve is the native intersection of its two unchanged analytic carriers.
//! A rational circle guides its transverse parameter only; it is not geometry.
use super::*;

#[allow(clippy::too_many_arguments)]
pub(super) fn construct(
    source: &Vertex,
    edge: &Edge,
    walls: &[BoundaryPlane; 2],
    axis: Vector3,
    offsets: [Vector3; 2],
    weight: f64,
    cap: &Face,
    adjacency: &HashMap<EdgeId, Vec<FaceId>>,
    edges: &HashMap<EdgeId, Edge>,
    treatment: Treatment,
    surface: Option<&Surface>,
    history: &mut ShellHistory,
) -> Result<Option<End>> {
    let (Treatment::Fillet, Some(surface)) = (treatment, surface) else {
        return Ok(None);
    };
    let mut neighbours = Vec::new();
    for wall in walls {
        let candidates = edges
            .values()
            .filter(|other| {
                other.id() != edge.id()
                    && (other.front().id() == source.id() || other.back().id() == source.id())
                    && adjacency[&other.id()].contains(&wall.face.id())
                    && adjacency[&other.id()].contains(&cap.id())
            })
            .collect::<Vec<_>>();
        let [neighbour] = candidates.as_slice() else {
            return Ok(None);
        };
        neighbours.push((*neighbour).clone());
    }
    let neighbours: [Edge; 2] = neighbours
        .try_into()
        .map_err(|_| invalid("A curved terminal needs two neighbouring boundaries"))?;
    let radius = offsets[0].magnitude();
    let controls = [
        (source.point() + offsets[0]).to_homogeneous(),
        source.point().to_homogeneous() * weight,
        (source.point() + offsets[1]).to_homogeneous(),
    ];
    let Some(certificate) = crate::curved_blend_proof::certify(
        source,
        axis,
        offsets,
        cap,
        &neighbours,
        radius,
        &controls,
    )?
    else {
        return Ok(None);
    };
    if radius <= 2. * monstertruck::core::tolerance::TOLERANCE
        || certificate.branch_clearance <= 2. * monstertruck::core::tolerance::TOLERANCE
    {
        return Err(unsupported(
            "The terminal branches cannot be distinguished at the native intersection solver's precision",
        ));
    }
    let points = certificate.points.map(Vertex::new);
    let arc_side = certificate.arc_side;
    let arc = &neighbours[arc_side];
    let generator = &neighbours[1 - arc_side];
    let new_arc = continued_arc(arc, source, &points[arc_side], certificate.arc_extension)?;
    let cap_surface = continued_cylinder(
        cap,
        source,
        generator,
        certificate.circle_center,
        certificate.circle_normal,
        certificate.circle_radius,
    )?;
    let guide = Curve::NurbsCurve(NurbsCurve::new(BsplineCurve::new(
        KnotVector::bezier_knot(2),
        controls.to_vec(),
    )));
    let mut intersection = SurfaceCurve::with_boundaries(
        Box::new(surface.clone()),
        Box::new(cap_surface.clone()),
        Box::new(guide),
        None,
        None,
    )
    .with_branch_halfspace(certificate.branch_origin, certificate.branch_normal)
    .ok_or_else(|| unsupported("The cylindrical root half-space is not finite"))?;
    for offset in offsets {
        intersection = intersection
            .with_branch_halfspace(source.point(), offset)
            .and_then(|intersection| {
                intersection.with_branch_halfspace(source.point() + offset, -offset)
            })
            .ok_or_else(|| unsupported("The fillet meridian domain is not finite"))?;
    }
    // Whole-branch existence and separation are established by certify().
    // These residual checks ensure the native solver resolves that proven
    // branch and its shared endpoint vertices rather than an opposite root.
    for (parameter, point) in [(0., points[0].point()), (1., points[1].point())] {
        let Some((actual, _, _)) = intersection.search_triple(parameter, 100) else {
            return Err(unsupported(
                "The cylindrical terminal intersection did not resolve its certified endpoints",
            ));
        };
        if actual.distance(point) > EPS
            || (actual - certificate.branch_origin).dot(certificate.branch_normal)
                < certificate.branch_clearance - EPS
        {
            return Err(unsupported(
                "The cylindrical terminal solver did not preserve the certified intersection branch",
            ));
        }
    }
    let curve = Edge::try_new(
        &points[0],
        &points[1],
        Curve::IntersectionCurve(intersection),
    )
    .map_err(|_| invalid("The cylindrical terminal intersection has inconsistent endpoints"))?;
    history.construct_edge(
        curve.id(),
        &[history.edge(edge.id())?, history.face(cap.id())?],
        serde_json::json!({"operation":"fillet","constructor":"cylindrical_terminal_intersection"}),
    )?;
    let mut neighbour_curves = [None, None];
    neighbour_curves[arc_side] = Some(new_arc);
    Ok(Some(End {
        source: source.clone(),
        cap: cap.clone(),
        points,
        neighbours: neighbours.map(|edge| edge.id()),
        curve,
        controls: controls.to_vec(),
        axial_range: Some(certificate.axial_range),
        neighbour_curves,
        cap_surface: Some(cap_surface),
        branch_halfspace: Some((certificate.branch_origin, certificate.branch_normal)),
    }))
}

fn continued_arc(
    source: &Edge,
    vertex: &Vertex,
    point: &Vertex,
    expected_angle: f64,
) -> Result<Curve> {
    let ((center, normal, radius), sweep) = crate::bounded_carriers::circle(&source.curve())?;
    let changed_front = source.front().id() == vertex.id();
    let front = if changed_front { point } else { source.front() };
    let back = if changed_front { source.back() } else { point };
    let old = vertex.point() - center;
    let new = point.point() - center;
    let angle = normal.dot(old.cross(new)).atan2(old.dot(new));
    if (angle - expected_angle).abs() * radius > EPS {
        return Err(unsupported(
            "The native rim continuation disagrees with its certified angular domain",
        ));
    }
    let sweep = if changed_front {
        sweep - angle
    } else {
        sweep + angle
    };
    if !sweep.is_finite() || sweep.abs() >= std::f64::consts::TAU - 1e-9 {
        return Err(unsupported(
            "The circular terminal continuation reaches a full turn",
        ));
    }
    let radial = front.point() - center;
    let (sine, cosine) = (sweep * 0.5).sin_cos();
    let transit = center + radial * cosine + normal.cross(radial) * sine;
    let edge = builder::try_circle_arc(front, back, transit)
        .map_err(|_| invalid("The circular terminal continuation collapsed"))?;
    let ((actual_center, actual_normal, actual_radius), actual_sweep) =
        crate::bounded_carriers::circle(&edge.curve())?;
    if center.distance(actual_center)
        + (radius - actual_radius).abs()
        + (actual_normal.cross(normal)).magnitude() * radius
        > EPS
        || (actual_sweep * actual_normal.dot(normal) - sweep).abs() * radius > EPS
    {
        return Err(unsupported(
            "The continued native circle cannot retain its source carrier within modeling tolerance",
        ));
    }
    Ok(edge.curve())
}

fn continued_cylinder(
    face: &Face,
    source: &Vertex,
    generator: &Edge,
    center: Point3,
    axis: Vector3,
    radius: f64,
) -> Result<Surface> {
    let other = if generator.front().id() == source.id() {
        generator.back().point()
    } else {
        generator.front().point()
    };
    let radial = (source.point() - center).normalize() * radius;
    let start = center + radial;
    let end = start + axis * (other - start).dot(axis);
    if start.distance(source.point()) > EPS || end.distance(other) > EPS {
        return Err(unsupported(
            "The continued cylinder cannot preserve its native generator within modeling tolerance",
        ));
    }
    let profile = Curve::Line(Line(start, end));
    let mut surface = Surface::RevolutionSurface(Processor::new(RevolutionSurface::by_revolution(
        profile, center, axis,
    )));
    let original = face.oriented_surface();
    let original_uv = original
        .search_parameter(source.point(), None, 100)
        .ok_or_else(|| unsupported("The source cylindrical carrier lost its terminal vertex"))?;
    let replacement_uv = surface
        .search_parameter(source.point(), None, 100)
        .ok_or_else(|| unsupported("The continued cylindrical carrier lost its source vertex"))?;
    let old_normal = original.normal(original_uv.0, original_uv.1);
    let new_normal = surface.normal(replacement_uv.0, replacement_uv.1);
    if !old_normal.magnitude2().is_finite()
        || !new_normal.magnitude2().is_finite()
        || old_normal.magnitude2() <= EPS * EPS
        || new_normal.magnitude2() <= EPS * EPS
    {
        return Err(unsupported(
            "The cylindrical terminal has a singular carrier normal",
        ));
    }
    if old_normal.dot(new_normal) < 0. {
        surface.invert();
    }
    Ok(surface)
}
