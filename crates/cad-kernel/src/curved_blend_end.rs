//! Native cylindrical terminations for straight planar fillets and chamfers.
//! A fillet uses a branch-constrained intersection. A chamfer has an explicit
//! rational ellipse arc obtained by projecting the cylinder rim onto its plane.
use super::*;
use crate::curved_blend_proof::Certificate;

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
    cylinder_extensions: &mut HashMap<FaceId, (EdgeId, Surface)>,
    history: &mut ShellHistory,
) -> Result<Option<End>> {
    let Some(surface) = surface else {
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
    let mut controls = vec![(source.point() + offsets[0]).to_homogeneous()];
    if matches!(treatment, Treatment::Fillet) {
        controls.push(source.point().to_homogeneous() * weight);
    }
    controls.push((source.point() + offsets[1]).to_homogeneous());
    let Some(certificate) =
        crate::curved_blend_proof::certify(source, axis, offsets, cap, &neighbours, &controls)?
    else {
        return Ok(None);
    };
    let points = certificate.points.map(Vertex::new);
    let arc_side = certificate.arc_side;
    let arc = &neighbours[arc_side];
    let generator = &neighbours[1 - arc_side];
    // Opposite ends of one rim share exactly the same native cylinder,
    // including its parameter frame. This preserves carrier identity on both
    // fillet intersections as well as on the rebuilt terminal face.
    let cap_surface = match cylinder_extensions.entry(cap.id()) {
        std::collections::hash_map::Entry::Occupied(entry) => {
            if entry.get().0 != arc.id() {
                return Err(unsupported(
                    "Two different rims on one cylindrical terminal need a joint trimming construction",
                ));
            }
            &entry.into_mut().1
        }
        std::collections::hash_map::Entry::Vacant(entry) => {
            let carrier = continued_cylinder(
                cap,
                source,
                generator,
                certificate.circle_center,
                certificate.circle_normal,
                certificate.circle_radius,
            )?;
            &entry.insert((arc.id(), carrier)).1
        }
    };
    let geometry = match (treatment, surface) {
        (Treatment::Chamfer, Surface::Plane(plane)) => {
            chamfer_ellipse(&certificate, source.point(), axis, plane)?
        }
        (Treatment::Chamfer, _) => return Ok(None),
        (Treatment::Fillet, _) => fillet_intersection(
            &certificate,
            source.point(),
            offsets,
            &controls,
            surface,
            cap_surface,
        )?,
    };
    let curve = Edge::try_new(&points[0], &points[1], geometry)
        .map_err(|_| invalid("The cylindrical terminal has inconsistent endpoints"))?;
    history.construct_edge(
        curve.id(),
        &[history.edge(edge.id())?, history.face(cap.id())?],
        serde_json::json!({"operation":treatment.name(),"constructor":"cylindrical_terminal_intersection"}),
    )?;
    let arc_trim = Some((arc.id(), certificate.arc_extension));
    Ok(Some(End {
        source: source.clone(),
        cap: cap.clone(),
        points,
        neighbours: neighbours.map(|edge| edge.id()),
        curve,
        controls,
        axial_range: Some(certificate.axial_range),
        arc_trim,
        branch_halfspace: Some((certificate.branch_origin, certificate.branch_normal)),
    }))
}

fn fillet_intersection(
    certificate: &Certificate,
    source: Point3,
    offsets: [Vector3; 2],
    controls: &[Vector4],
    surface: &Surface,
    cap_surface: &Surface,
) -> Result<Curve> {
    if offsets[0].magnitude() <= 2. * monstertruck::core::tolerance::TOLERANCE
        || certificate.branch_clearance <= 2. * monstertruck::core::tolerance::TOLERANCE
    {
        return Err(unsupported(
            "The terminal branches cannot be distinguished at the native intersection solver's precision",
        ));
    }
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
            .with_branch_halfspace(source, offset)
            .and_then(|intersection| intersection.with_branch_halfspace(source + offset, -offset))
            .ok_or_else(|| unsupported("The fillet meridian domain is not finite"))?;
    }
    // Whole-branch existence and separation are established by certify().
    // These residual checks ensure the native solver resolves that proven
    // branch and its shared endpoint vertices rather than an opposite root.
    for (parameter, point) in [(0., certificate.points[0]), (1., certificate.points[1])] {
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
    Ok(Curve::IntersectionCurve(intersection))
}

/// A parallel projection along the cylinder generator preserves the circular
/// cross section in radial coordinates and acts affinely on rational controls.
/// The two existing rail vertices fix the ends of the bounded ellipse.
fn chamfer_ellipse(
    certificate: &Certificate,
    source: Point3,
    edge_axis: Vector3,
    plane: &Plane,
) -> Result<Curve> {
    let axis = certificate.circle_normal;
    let normal = plane.normal();
    let denominator = normal.dot(axis);
    if !denominator.is_finite() || denominator.abs() <= 1e-10 {
        return Err(unsupported(
            "The bevel plane is parallel to the cylindrical generator",
        ));
    }
    let center = certificate.circle_center;
    let radial = source - center;
    let radial = (radial - axis * radial.dot(axis)).normalize() * certificate.circle_radius;
    let (sine, weight) = (certificate.arc_extension * 0.5).sin_cos();
    if !weight.is_finite() || weight <= 0. {
        return Err(unsupported(
            "The chamfer ellipse has no bounded angular chart",
        ));
    }
    // The homogeneous middle control of a circular arc is (w*C + R*mid, w).
    // Project it onto the bevel plane before pairing it with the shared ends.
    let middle_offset = radial * weight + axis.cross(radial) * sine;
    let shift = ((plane.origin() - center) * weight - middle_offset).dot(normal) / denominator;
    let middle = center.to_vec() * weight + (middle_offset + axis * shift);
    let controls = [
        certificate.points[0].to_homogeneous(),
        middle.extend(weight),
        certificate.points[1].to_homogeneous(),
    ];
    crate::curved_blend_proof::certify_bevel_arc(&controls, plane, edge_axis, certificate)?;
    Ok(Curve::NurbsCurve(NurbsCurve::new(BsplineCurve::new(
        KnotVector::bezier_knot(2),
        controls.to_vec(),
    ))))
}

pub(super) fn continued_arc(
    source: &Edge,
    front: &Vertex,
    back: &Vertex,
    expected_angles: [Option<f64>; 2],
) -> Result<Edge> {
    let ((center, normal, radius), sweep) = crate::bounded_carriers::circle(&source.curve())?;
    let mut delta = [0.; 2];
    for (i, (old, new)) in [(source.front(), front), (source.back(), back)]
        .into_iter()
        .enumerate()
    {
        let Some(expected) = expected_angles[i] else {
            if old.id() != new.id() {
                return Err(unsupported(
                    "A changed circular rim endpoint has no verified angular domain",
                ));
            }
            continue;
        };
        let a = old.point() - center;
        let b = new.point() - center;
        delta[i] = normal.dot(a.cross(b)).atan2(a.dot(b));
        if (delta[i] - expected).abs() * radius > EPS {
            return Err(unsupported(
                "The native rim continuation disagrees with its certified angular domain",
            ));
        }
    }
    let trimmed = sweep + delta[1] - delta[0];
    if !trimmed.is_finite()
        || trimmed * sweep <= 0.
        || trimmed.abs() * radius <= EPS
        || trimmed.abs() >= std::f64::consts::TAU - EPS / radius
    {
        return Err(invalid(
            "The combined setbacks consume, reverse or close the circular rim; reduce their size",
        ));
    }
    let radial = front.point() - center;
    let (sine, cosine) = (trimmed * 0.5).sin_cos();
    let transit = center + radial * cosine + normal.cross(radial) * sine;
    let edge = builder::try_circle_arc(front, back, transit)
        .map_err(|_| invalid("The circular terminal continuation collapsed"))?;
    let ((actual_center, actual_normal, actual_radius), actual_sweep) =
        crate::bounded_carriers::circle(&edge.curve())?;
    if center.distance(actual_center)
        + (radius - actual_radius).abs()
        + (actual_normal.cross(normal)).magnitude() * radius
        > EPS
        || (actual_sweep * actual_normal.dot(normal) - trimmed).abs() * radius > EPS
    {
        return Err(unsupported(
            "The continued native circle cannot retain its source carrier within modeling tolerance",
        ));
    }
    Ok(edge)
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
