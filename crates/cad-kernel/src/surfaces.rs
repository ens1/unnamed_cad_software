//! Exact surface B-reps. This adapter never promotes an open shell to a solid.
use crate::adapter::{MonstertruckAdapter, Shape, TopologyHistory};
use cad_core::{
    CadError, Dimension, Document, NodeRef, Result, expression, surface::SurfaceOperation,
};
use monstertruck::modeling::*;
use monstertruck::{
    geometry::prelude::TryIntoHomogeneousBsplineCurve, topology::shell::ShellCondition,
};
use std::collections::{BTreeMap, HashMap, HashSet};

fn invalid(message: impl std::fmt::Display) -> CadError {
    CadError::Invalid(format!("Surface: {message}"))
}
fn point(p: [f64; 3]) -> Result<Point3> {
    if p.iter().any(|v| !v.is_finite()) {
        return Err(invalid("Coordinates must be finite"));
    }
    Ok(Point3::new(p[0], p[1], p[2]))
}
fn direction(v: [f64; 3]) -> Result<Vector3> {
    let v = point(v)?.to_vec();
    let length = v.magnitude();
    if !length.is_finite() || length < 1e-9 {
        return Err(invalid("Direction must have a finite, nonzero magnitude"));
    }
    Ok(v / length)
}
pub(crate) fn sections(inputs: &BTreeMap<NodeRef, Shape>, profile: &NodeRef) -> Result<Vec<Wire>> {
    let wires = match inputs.get(profile) {
        Some(Shape::Profile(face)) => face.boundaries().clone(),
        Some(Shape::Sketch { faces, paths }) => faces
            .iter()
            .flat_map(Face::boundaries)
            .chain(paths.iter().cloned())
            .collect(),
        _ => return Err(invalid("Select a sketch section")),
    };
    if wires.is_empty() || wires.iter().any(|w| w.is_empty()) {
        return Err(invalid(
            "A section needs at least one non-construction member",
        ));
    }
    Ok(wires)
}
pub(crate) fn planar_faces(
    inputs: &BTreeMap<NodeRef, Shape>,
    profile: &NodeRef,
) -> Result<Vec<Face>> {
    let faces = match inputs.get(profile) {
        Some(Shape::Profile(face)) => vec![face.clone()],
        Some(Shape::Sketch { faces, .. }) => faces.clone(),
        _ => return Err(invalid("Select a closed planar sketch")),
    };
    if faces.is_empty() {
        return Err(invalid("Planar Surface requires closed contours"));
    }
    Ok(faces)
}
pub(crate) fn surfaces<'a>(
    inputs: &'a BTreeMap<NodeRef, Shape>,
    reference: &NodeRef,
) -> Result<&'a [Shell]> {
    match inputs.get(reference) {
        Some(Shape::Surfaces(shells)) => Ok(shells),
        _ => Err(invalid("Select a surface body")),
    }
}
fn planar_normal(shell: &Shell) -> Result<Vector3> {
    if shell.is_empty() || !matches!(shell[0].surface(), Surface::Plane(_)) {
        return Err(CadError::Unsupported("Offset Surface and Thicken require coplanar face shells; curved normal offsets require additional exact surface construction".into()));
    }
    let normal = shell[0].oriented_surface().normal(0., 0.);
    if !normal.magnitude().is_finite() || normal.magnitude() < 1e-9 {
        return Err(invalid("Degenerate or non-finite surface normal"));
    }
    let normal = normal.normalize();
    let origin = shell[0].oriented_surface().subs(0., 0.);
    for face in shell {
        let candidate = face.oriented_surface().normal(0., 0.);
        let length = candidate.magnitude();
        if !matches!(face.surface(), Surface::Plane(_))
            || !length.is_finite()
            || length < 1e-9
            || (candidate / length).dot(normal) < 1. - 1e-10
            || (face.oriented_surface().subs(0., 0.) - origin)
                .dot(normal)
                .abs()
                > 1e-7
        {
            return Err(CadError::Unsupported("Normal offset and Thicken require all faces in a shell to be coplanar and consistently oriented".into()));
        }
    }
    Ok(normal)
}

/// Resolve the solved, exact sketch again so every refined boundary retains its
/// authored member identity. Sketch topology is intentionally not a model body
/// and therefore is not named by the generic body-history fallback.
fn loft_section(
    adapter: &MonstertruckAdapter,
    doc: &Document,
    profile: &NodeRef,
    inputs: &BTreeMap<NodeRef, Shape>,
) -> Result<(Wire, HashMap<EdgeId, crate::section_history::Member>)> {
    let (source, plane) = crate::extrusion::source(adapter, doc, profile, inputs)?;
    let built = crate::sketches::build_traced(doc, &profile.definition, &source, &plane)?;
    let Shape::Sketch { faces, paths } = built.shape else {
        return Err(invalid("Select a native sketch section"));
    };
    let mut wires = faces
        .iter()
        .flat_map(Face::boundaries)
        .chain(paths)
        .collect::<Vec<_>>();
    if wires.len() != 1 {
        return Err(CadError::Unsupported(
            "Surface Loft needs one open or closed material contour per section; construction members do not participate".into(),
        ));
    }
    let wire = wires.remove(0);
    let members =
        wire.iter()
            .map(|edge| {
                let (member, piece) = built.members.get(&edge.id()).ok_or_else(|| {
                    invalid("A loft boundary lost its exact sketch-member ancestry")
                })?;
                Ok((
                    edge.id(),
                    crate::section_history::member(
                        profile,
                        &crate::extrusion::ThinMember {
                            sources: vec![member.clone()],
                            side: 0,
                            piece: *piece,
                            cap: false,
                        },
                    )?,
                ))
            })
            .collect::<Result<HashMap<_, _>>>()?;
    Ok((wire, members))
}

fn loft_diagnostic(error: CadError, index: usize, patch: Option<usize>) -> CadError {
    let context = match patch {
        Some(patch) => format!(
            "Loft sections {}–{}, strip {}",
            index + 1,
            index + 2,
            patch + 1
        ),
        None => format!("Loft sections {}–{}", index + 1, index + 2),
    };
    match error {
        CadError::Unsupported(message) => CadError::Unsupported(format!("{context}: {message}")),
        CadError::Invalid(message) => CadError::Invalid(format!("{context}: {message}")),
        other => other,
    }
}

fn loft_patch(
    carrier: &NurbsSurface<Vector4>,
    prepared: &crate::loft_correspondence::Prepared,
    index: usize,
    proof_work: &mut usize,
) -> Result<()> {
    let normal = prepared.normals[index];
    let height = (prepared.centers[index + 1] - prepared.centers[index]).dot(normal);
    if height.abs() > 1e-7 && normal.cross(prepared.normals[index + 1]).magnitude() <= 1e-9 {
        let ((u0, u1), (v0, v1)) = carrier.range_tuple();
        let tangent = carrier.uder((u0 + u1) * 0.5, (v0 + v1) * 0.5);
        let tangent = tangent - normal * tangent.dot(normal);
        if tangent.magnitude().is_finite() && tangent.magnitude() > 1e-12 {
            // Validate the actual rational Jacobian, including plane drift.
            // Parallelism is only a useful frame choice, never the proof.
            match crate::surface_construct::prove_projection(
                carrier,
                tangent.normalize(),
                normal * height.signum(),
                proof_work,
            ) {
                Ok(()) => return Ok(()),
                Err(CadError::Unsupported(_)) => {}
                Err(error) => return Err(error),
            }
        }
    }
    crate::surface_construct::prove_regular(carrier, proof_work)
}

fn loft(
    adapter: &MonstertruckAdapter,
    doc: &Document,
    owner: &NodeRef,
    profiles: &[NodeRef],
    inputs: &BTreeMap<NodeRef, Shape>,
    history: &mut TopologyHistory,
) -> Result<Shell> {
    if profiles.len() < 2 || profiles.len() > 256 {
        return Err(invalid("Loft needs 2–256 ordered sections"));
    }
    if profiles
        .iter()
        .collect::<std::collections::BTreeSet<_>>()
        .len()
        != profiles.len()
    {
        return Err(CadError::Unsupported(
            "Surface Loft needs distinct source section sketches; repeated sections and closed periodic lofts require explicit occurrence and seam intent".into(),
        ));
    }
    let sections = profiles
        .iter()
        .map(|profile| loft_section(adapter, doc, profile, inputs))
        .collect::<Result<Vec<_>>>()?;
    let wires = sections
        .iter()
        .map(|(wire, _)| wire.clone())
        .collect::<Vec<_>>();
    let mut prepared = crate::loft_correspondence::prepare(&wires, &vec![None; wires.len()])?;
    if !prepared.radial {
        crate::loft_correspondence::refine_ruled_charts(&mut prepared)?;
    }
    let mut named_sections = Vec::new();
    for (index, wire) in prepared.wires.iter().enumerate() {
        let mut names = HashMap::new();
        for edge in wire {
            let piece = &prepared.pieces[index][&edge.id()];
            let source = &sections[index].1[&piece.source];
            let reference = history.tool_edge(
                owner,
                edge,
                &source.sources,
                &serde_json::json!({"operation":"surface_loft","boundary":"section","section":profiles[index],"member":source.purpose,"interval":piece.interval}),
            )?;
            names.insert(edge.id(), reference);
        }
        named_sections.push(names);
    }
    let mut shell: Shell = Vec::new().into();
    let mut proof_work = 0;
    for index in 0..prepared.wires.len() - 1 {
        let lower = &prepared.wires[index];
        let upper = &prepared.wires[index + 1];
        let skin = crate::loft_correspondence::skin(lower, upper, None, None)?;
        if skin.len() != lower.len() || upper.len() != lower.len() {
            return Err(invalid(
                "Loft changed its refined native boundary correspondence",
            ));
        }
        let radial = crate::loft_correspondence::prove_radial_pair(&prepared, index)
            .map_err(|error| loft_diagnostic(error, index, None))?;
        let ruled = !radial
            && crate::loft_correspondence::prove_ruled_pair(&prepared, index)
                .map_err(|error| loft_diagnostic(error, index, None))?;
        if !radial && !ruled {
            for (patch, face) in skin.iter().enumerate() {
                let Surface::NurbsSurface(carrier) = face.surface() else {
                    return Err(invalid("Loft lost its exact rational supporting carrier"));
                };
                loft_patch(&carrier, &prepared, index, &mut proof_work)
                    .map_err(|error| loft_diagnostic(error, index, Some(patch)))?;
            }
        }
        let mut connectors = HashMap::new();
        for (a, b) in lower.iter().zip(upper) {
            let lower_piece = &prepared.pieces[index][&a.id()];
            let upper_piece = &prepared.pieces[index + 1][&b.id()];
            let names = vec![
                named_sections[index][&a.id()].clone(),
                named_sections[index + 1][&b.id()].clone(),
            ];
            connectors.insert(
                (a.front().id(), b.front().id()),
                (
                    names.clone(),
                    [lower_piece.interval[0], upper_piece.interval[0]],
                ),
            );
            // The final endpoint of an open wire has no following boundary
            // member from which to derive its endpoint identity.
            if !lower.is_closed() && a.id() == lower.back().unwrap().id() {
                connectors.insert(
                    (a.back().id(), b.back().id()),
                    (names, [lower_piece.interval[1], upper_piece.interval[1]]),
                );
            }
        }
        for ((a, b), face) in lower.iter().zip(upper).zip(&skin) {
            let lower_piece = &prepared.pieces[index][&a.id()];
            let upper_piece = &prepared.pieces[index + 1][&b.id()];
            let lower_source = &sections[index].1[&lower_piece.source];
            let upper_source = &sections[index + 1].1[&upper_piece.source];
            let sources = lower_source
                .sources
                .iter()
                .chain(&upper_source.sources)
                .cloned()
                .collect::<Vec<_>>();
            history.tool_face(
                owner,
                face,
                &sources,
                &serde_json::json!({"operation":"surface_loft","lower":{"section":profiles[index],"member":lower_source.purpose,"interval":lower_piece.interval},"upper":{"section":profiles[index+1],"member":upper_source.purpose,"interval":upper_piece.interval}}),
            )?;
        }
        let mut seen = HashSet::new();
        for edge in skin.edge_iter().filter(|edge| seen.insert(edge.id())) {
            if named_sections[index].contains_key(&edge.id())
                || named_sections[index + 1].contains_key(&edge.id())
            {
                continue;
            }
            let endpoints = (edge.front().id(), edge.back().id());
            let (sources, station) = connectors
                .get(&endpoints)
                .or_else(|| connectors.get(&(endpoints.1, endpoints.0)))
                .ok_or_else(|| invalid("A loft connector lost its native section endpoints"))?;
            history.tool_edge(
                owner,
                &edge,
                sources,
                &serde_json::json!({"operation":"surface_loft","boundary":"connector","endpoint_station":station}),
            )?;
        }
        shell.extend(skin);
    }
    Ok(shell)
}

pub(crate) fn operation(
    adapter: &MonstertruckAdapter,
    doc: &Document,
    owner: &NodeRef,
    operation: &SurfaceOperation,
    inputs: &BTreeMap<NodeRef, Shape>,
    history: &mut TopologyHistory,
) -> Result<Shape> {
    let length = |e| expression::value(doc, &owner.definition, e, Dimension::Length);
    let result = match operation {
        SurfaceOperation::Ruled { .. } | SurfaceOperation::MidSurface { .. } => Shape::Surfaces(
            crate::surface_construct::operation(adapter, doc, owner, operation, inputs, history)?,
        ),
        SurfaceOperation::Planar { profile } => Shape::Surfaces(
            planar_faces(inputs, profile)?
                .into_iter()
                .map(|f| vec![f].into())
                .collect(),
        ),
        SurfaceOperation::Extrude {
            profile,
            distance,
            direction: axis,
            mid_plane,
            profile_normal,
            reverse,
        } => {
            let distance = length(distance)?;
            if distance.abs() < 1e-9 {
                return Err(invalid("Extrusion depth must be nonzero"));
            }
            let axis = if *profile_normal {
                crate::extrusion::profile_normal(adapter, doc, profile, inputs)?
            } else {
                direction(*axis)?
            };
            let delta = axis * distance * if *reverse { -1. } else { 1. };
            let shells = sections(inputs, profile)?
                .into_iter()
                .map(|wire| {
                    let start = if *mid_plane {
                        builder::translated(&wire, -delta * 0.5)
                    } else {
                        wire
                    };
                    builder::extrude(&start, delta)
                })
                .collect();
            Shape::Surfaces(shells)
        }
        SurfaceOperation::Revolve {
            profile,
            origin,
            axis,
            angle,
            axis_reference,
        } => {
            let a = expression::value(doc, &owner.definition, angle, Dimension::Angle)?;
            if a.abs() < 1e-9 || a.abs() > std::f64::consts::TAU + 1e-8 {
                return Err(invalid(
                    "Angle must have magnitude between 0 and 360 degrees",
                ));
            }
            let reference = axis_reference
                .as_ref()
                .map(|r| adapter.resolve_surface_axis(doc, r, inputs))
                .transpose()?;
            let axis =
                direction(reference.as_ref().map(|r| r.direction).unwrap_or(*axis))? * a.signum();
            let sweep = if (a.abs() - std::f64::consts::TAU).abs() < 1e-8 {
                builder::SweepAngle::Closed
            } else {
                builder::SweepAngle::Partial(Rad(a.abs()))
            };
            let origin = point(reference.map(|r| r.origin).unwrap_or(*origin))?;
            Shape::Surfaces(
                sections(inputs, profile)?
                    .iter()
                    .map(|wire| builder::revolve(wire, origin, axis, sweep, 8))
                    .collect(),
            )
        }
        SurfaceOperation::Loft { profiles } => {
            Shape::Surfaces(vec![loft(adapter, doc, owner, profiles, inputs, history)?])
        }
        SurfaceOperation::Sweep {
            profile,
            path,
            path_sketch,
        } => {
            let mut stations = path
                .iter()
                .copied()
                .map(point)
                .collect::<Result<Vec<_>>>()?;
            if let Some(reference) = path_sketch {
                let Some(Shape::Sketch { paths, .. }) = inputs.get(reference) else {
                    return Err(invalid("Select an open path sketch"));
                };
                if paths.len() != 1
                    || paths[0].is_empty()
                    || paths[0]
                        .iter()
                        .any(|e| !matches!(e.curve(), Curve::Line(_)))
                {
                    return Err(CadError::Unsupported("Surface Sweep currently requires a single open polyline path and fixed section orientation".into()));
                }
                stations = paths[0].iter().map(|e| e.front().point()).collect();
                stations.push(paths[0].back().expect("nonempty path").back().point());
            }
            if stations.len() < 2 || stations.len() > 1024 {
                return Err(invalid("Sweep needs 2–1024 path stations"));
            }
            let mut shells = Vec::new();
            for wire in sections(inputs, profile)? {
                let mut shell: Shell = Vec::new().into();
                let mut previous = wire.clone();
                for pair in stations.windows(2) {
                    let delta = pair[1] - pair[0];
                    if delta.magnitude2() < 1e-18 {
                        return Err(invalid("Sweep path contains a zero-length segment"));
                    }
                    let next = builder::translated(&wire, pair[1] - stations[0]);
                    shell.extend(
                        builder::try_wire_homotopy::<Curve, Surface>(&previous, &next)
                            .map_err(invalid)?,
                    );
                    previous = next;
                }
                shells.push(shell);
            }
            Shape::Surfaces(shells)
        }
        SurfaceOperation::Fill {
            boundaries,
            reverse,
        } => crate::surface_fill::operation(
            adapter, doc, owner, boundaries, *reverse, inputs, history,
        )?,
        SurfaceOperation::Trim { .. }
        | SurfaceOperation::TrimDomain { .. }
        | SurfaceOperation::Untrim { .. }
        | SurfaceOperation::Extend { .. } => {
            crate::surface_edit::operation(adapter, doc, owner, operation, inputs)?
        }
        SurfaceOperation::Offset { surface, distance } => {
            let distance = length(distance)?;
            Shape::Surfaces(
                surfaces(inputs, surface)?
                    .iter()
                    .map(|shell| Ok(builder::translated(shell, planar_normal(shell)? * distance)))
                    .collect::<Result<Vec<_>>>()?,
            )
        }
        SurfaceOperation::Reverse { surface } => Shape::Surfaces(
            surfaces(inputs, surface)?
                .iter()
                .map(|s| s.inverse().into())
                .collect(),
        ),
        SurfaceOperation::Knit {
            surfaces: references,
            tolerance,
            make_solid,
            reverse,
        } => {
            let tolerance = length(tolerance)?;
            if !(1e-10..=1e-6).contains(&tolerance) {
                return Err(invalid(
                    "Knit tolerance must be 0.0000000001–0.000001 mm for the current B-rep precision",
                ));
            }
            if references.is_empty() || references.len() > 256 {
                return Err(invalid("Select 1–256 surface inputs"));
            }
            let input = references
                .iter()
                .map(|r| surfaces(inputs, r))
                .collect::<Result<Vec<_>>>()?;
            let sewn = knit(
                &input
                    .iter()
                    .flat_map(|s| s.iter())
                    .flat_map(|s| s.iter().cloned())
                    .collect::<Vec<_>>(),
                tolerance,
            )?;
            let components = sewn.connected_components();
            if *make_solid {
                if components.len() != 1
                    || components[0].shell_condition() != ShellCondition::Closed
                {
                    return Err(invalid(
                        "Form Solid requires one closed, consistently oriented shell; close the gaps and reverse inward-facing inputs",
                    ));
                }
                let shell = if *reverse {
                    components[0].inverse().into()
                } else {
                    components[0].clone()
                };
                Shape::Bodies(vec![Solid::try_new(vec![shell]).map_err(invalid)?])
            } else {
                Shape::Surfaces(
                    components
                        .into_iter()
                        .map(|s| if *reverse { s.inverse().into() } else { s })
                        .collect(),
                )
            }
        }
        SurfaceOperation::Thicken {
            surface,
            thickness,
            mid_plane,
            reverse,
        } => {
            let thickness = length(thickness)?;
            if thickness < 1e-9 {
                return Err(invalid("Thickness must be positive"));
            }
            let mut solids = Vec::new();
            for shell in surfaces(inputs, surface)? {
                let delta = planar_normal(shell)? * thickness * if *reverse { -1. } else { 1. };
                let start = if *mid_plane {
                    builder::translated(shell, -delta * 0.5)
                } else {
                    shell.clone()
                };
                solids.extend(crate::surface_edit::thicken_planar(&start, delta)?);
            }
            Shape::Bodies(solids)
        }
    };
    if let Shape::Surfaces(shells) = &result {
        if shells.is_empty() {
            return Err(invalid("Operation produced no surface bodies"));
        }
        for shell in shells {
            crate::validation::surface(shell)?;
        }
    }
    Ok(result)
}

pub(crate) fn same_curve(a: &Curve, b: &Curve, tolerance: f64) -> bool {
    if matches!(a, Curve::IntersectionCurve(_)) || matches!(b, Curve::IntersectionCurve(_)) {
        return false;
    }
    let (Some(mut a), Some(mut b)) = (
        a.try_into_homogeneous_bspline_curve(),
        b.try_into_homogeneous_bspline_curve(),
    ) else {
        return false;
    };
    a.clamp();
    b.clamp();
    if a.degree() != b.degree()
        || a.control_points().len() != b.control_points().len()
        || a.knot_vector().len() != b.knot_vector().len()
    {
        return false;
    }
    let (a0, a1) = a.range_tuple();
    let (b0, b1) = b.range_tuple();
    if a1 <= a0 || b1 <= b0 {
        return false;
    }
    if a.knot_vector()
        .iter()
        .zip(b.knot_vector().iter())
        .any(|(a, b)| ((a - a0) / (a1 - a0) - (b - b0) / (b1 - b0)).abs() > 1e-12)
    {
        return false;
    }
    let aw = a.control_points()[0].w;
    let bw = b.control_points()[0].w;
    if aw <= 0. || bw <= 0. {
        return false;
    }
    a.control_points()
        .iter()
        .zip(b.control_points())
        .all(|(a, b)| {
            a.w > 0.
                && b.w > 0.
                && ((a.x / a.w - b.x / b.w).powi(2)
                    + (a.y / a.w - b.y / b.w).powi(2)
                    + (a.z / a.w - b.z / b.w).powi(2))
                .sqrt()
                    <= tolerance
                && (a.w / aw - b.w / bw).abs() <= 1e-12
        })
}
/// Sew only proven matching rational representations. General edge intersections are separate work.
pub(crate) fn knit(faces: &[Face], tolerance: f64) -> Result<Shell> {
    let mut vertices: Vec<Vertex> = Vec::new();
    let mut edges: Vec<Edge> = Vec::new();
    let mut result: Shell = Vec::new().into();
    for face in faces {
        let mut boundaries = Vec::new();
        for boundary in face.boundaries() {
            let mut wire: Wire = Vec::new().into();
            for edge in boundary {
                let mut endpoint = |point: Point3| -> Result<Vertex> {
                    let candidates = vertices
                        .iter()
                        .filter(|v| (v.point() - point).magnitude() <= tolerance)
                        .cloned()
                        .collect::<Vec<_>>();
                    match candidates.as_slice() {
                        [v] => Ok(v.clone()),
                        [] => {
                            let v = builder::vertex(point);
                            vertices.push(v.clone());
                            Ok(v)
                        }
                        _ => Err(invalid(
                            "Knit vertex matching is ambiguous; reduce the tolerance",
                        )),
                    }
                };
                let front = endpoint(edge.front().point())?;
                let back = endpoint(edge.back().point())?;
                if front.id() == back.id() {
                    return Err(invalid("Knit tolerance collapses an edge"));
                }
                let curve = edge.oriented_curve();
                let mut candidates = Vec::new();
                for previous in &edges {
                    if previous.front().id() == front.id()
                        && previous.back().id() == back.id()
                        && same_curve(&previous.oriented_curve(), &curve, tolerance)
                    {
                        candidates.push(previous.clone());
                    } else if previous.back().id() == front.id()
                        && previous.front().id() == back.id()
                        && same_curve(&previous.inverse().oriented_curve(), &curve, tolerance)
                    {
                        candidates.push(previous.inverse());
                    }
                }
                match candidates.as_slice() {
                    [e] => wire.push_back(e.clone()),
                    [] => {
                        let e = Edge::new(&front, &back, curve);
                        edges.push(e.clone());
                        wire.push_back(e);
                    }
                    _ => return Err(invalid("Knit edge matching is ambiguous")),
                }
            }
            boundaries.push(wire);
        }
        result.push(Face::try_new(boundaries, face.oriented_surface()).map_err(invalid)?);
    }
    crate::validation::surface(&result)?;
    Ok(result)
}
