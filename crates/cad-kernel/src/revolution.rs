//! Exact angular sweeps of bounded planar sketch regions.
use crate::adapter::{MonstertruckAdapter, Shape, TopologyHistory};
use cad_core::{
    CadError, Dimension, Document, NodeRef, Operation, Result, TopologyRef, expression,
};
use monstertruck::modeling::*;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

#[derive(Clone)]
struct MemberOrigin {
    sources: Vec<TopologyRef>,
    side: usize,
    piece: usize,
    cap: bool,
    pole_split: Option<usize>,
    reversed: bool,
}
struct Construction<'a> {
    origin: Point3,
    axis: Vector3,
    start: f64,
    total: f64,
    owner: &'a NodeRef,
    generated: &'a mut TopologyHistory,
}
fn member_reference(profile: &NodeRef, member: &str) -> Result<TopologyRef> {
    Ok(TopologyRef {
        producer: profile.clone(),
        role: format!(
            "construction:{}",
            serde_json::to_string(
                &serde_json::json!({"sources":[],"purpose":{"operation":"sketch_member","member":member},"topology":"curve"})
            )?
        ),
        topology: "curve".into(),
        signature: None,
    })
}
fn endpoint_reference(profile: &NodeRef, source: &MemberOrigin, end: usize) -> Result<TopologyRef> {
    let sources = crate::shell_history::closure(&source.sources)?;
    Ok(TopologyRef {
        producer: profile.clone(),
        role: format!(
            "construction:{}",
            serde_json::to_string(
                &serde_json::json!({"sources":sources,"purpose":{"operation":"section_endpoint","side":source.side,"piece":source.piece,"cap":source.cap,"pole_split":source.pole_split,"endpoint":if source.reversed {1-end}else{end}},"topology":"point"})
            )?
        ),
        topology: "point".into(),
        signature: None,
    })
}
impl Construction<'_> {
    fn section(
        &mut self,
        section: &crate::extrusion::ThinSection,
        profile: &NodeRef,
    ) -> Result<Solid> {
        let mut face = section.face.clone();
        let normal = face.oriented_surface().normal(0., 0.).normalize();
        let radial = normal.cross(self.axis);
        let direction = crate::extrusion::face_hull(&face)?
            .iter()
            .map(|point| (*point - self.origin).dot(radial))
            .sum::<f64>();
        if direction < 0. {
            face = face.inverse();
        }
        let original = face.boundaries();
        if self.start != 0. {
            face = builder::rotated(&face, self.origin, self.axis, Rad(self.start));
        }
        let mut origins = HashMap::new();
        for (before, after) in original.iter().zip(face.boundaries()) {
            for (before, after) in before.iter().zip(after.iter()) {
                let source = section
                    .members
                    .get(&before.id())
                    .ok_or_else(|| invalid("A section boundary has no native member ancestry"))?;
                origins.insert(
                    after.id(),
                    MemberOrigin {
                        sources: source
                            .sources
                            .iter()
                            .map(|member| member_reference(profile, member))
                            .collect::<Result<_>>()?,
                        side: source.side,
                        piece: source.piece,
                        cap: source.cap,
                        pole_split: None,
                        reversed: !before.orientation(),
                    },
                );
            }
        }
        let closed = self.total == std::f64::consts::TAU;
        let sweep = if closed {
            builder::SweepAngle::Closed
        } else {
            builder::SweepAngle::Partial(Rad(self.total))
        };
        let mut shells = Vec::new();
        let mut start_wires = Vec::new();
        let mut end_wires = Vec::new();
        let mut face_refs = HashMap::new();
        let mut axis_edges = Vec::new();
        let mut named_edges = HashSet::new();
        for (contour, mut wire) in face.boundaries().into_iter().enumerate() {
            for edge in &wire {
                let offset = (edge.front().point() - self.origin).cross(self.axis);
                if offset.so_small() && offset.magnitude() > 1e-7 {
                    return Err(CadError::Unsupported("Revolve: A boundary vertex lies in the kernel's uncertain near-axis band; constrain it to the axis or move it farther away".into()));
                }
            }
            let axis_flags = wire
                .iter()
                .map(|edge| {
                    crate::extrusion::curve_hull(&edge.oriented_curve()).map(|controls| {
                        controls.iter().all(|point| {
                            (*point - self.origin).cross(self.axis).magnitude() <= 1e-7
                        })
                    })
                })
                .collect::<Result<Vec<_>>>()?;
            let axis_count = axis_flags.iter().filter(|axis| **axis).count();
            if axis_count == wire.len() {
                return Err(invalid("A contour lies entirely on the revolution axis"));
            }
            if axis_count > 0 {
                if contour != 0 {
                    return Err(CadError::Unsupported(
                        "Revolve: Interior holes may not touch the axis".into(),
                    ));
                }
                let transitions = (0..wire.len())
                    .filter(|i| !axis_flags[*i] && axis_flags[(*i + wire.len() - 1) % wire.len()])
                    .collect::<Vec<_>>();
                if transitions.len() != 1 {
                    return Err(CadError::Unsupported("Revolve: Several disjoint axis contacts require exact singularity reconstruction".into()));
                }
                wire.rotate_left(transitions[0]);
            } else if wire.iter().any(|edge| {
                (edge.front().point() - self.origin)
                    .cross(self.axis)
                    .so_small()
            }) {
                return Err(CadError::Unsupported(
                    "Revolve: An isolated axis contact requires exact singularity reconstruction"
                        .into(),
                ));
            }
            let moving = wire.len() - axis_count;
            let closing = wire.iter().skip(moving).cloned().collect::<Wire>();
            axis_edges.extend(closing.iter().cloned());
            let mut path = wire.iter().take(moving).cloned().collect::<Wire>();
            if path.len() == 1 && axis_count > 0 {
                // Split the real carrier once, so the native pole-collapse
                // constructor and both caps share the same source edges.
                let edge = path
                    .pop_front()
                    .ok_or_else(|| invalid("Missing pole boundary"))?;
                let mut curve = edge.oriented_curve();
                let (a, b) = curve.range_tuple();
                let t = a + (b - a) / 2.;
                let vertex = builder::vertex(curve.subs(t));
                let right = curve.cut(t);
                let first = Edge::try_new(edge.front(), &vertex, curve)
                    .map_err(|error| invalid(&error.to_string()))?;
                let second = Edge::try_new(&vertex, edge.back(), right)
                    .map_err(|error| invalid(&error.to_string()))?;
                let source = origins[&edge.id()].clone();
                for (index, edge) in [&first, &second].into_iter().enumerate() {
                    origins.insert(
                        edge.id(),
                        MemberOrigin {
                            pole_split: Some(if source.reversed { 1 - index } else { index }),
                            ..source.clone()
                        },
                    );
                }
                path = Wire::from(vec![first, second]);
            }
            if path.iter().skip(1).any(|edge| {
                (edge.front().point() - self.origin)
                    .cross(self.axis)
                    .so_small()
            }) {
                return Err(CadError::Unsupported("Revolve: A moving boundary has an interior pole; split the section into regular axis-bounded regions".into()));
            }
            let mut vertices: HashMap<VertexId, Vec<TopologyRef>> = HashMap::new();
            for edge in &path {
                let source = &origins[&edge.id()];
                vertices
                    .entry(edge.front().id())
                    .or_default()
                    .push(endpoint_reference(profile, source, 0)?);
                vertices
                    .entry(edge.back().id())
                    .or_default()
                    .push(endpoint_reference(profile, source, 1)?);
                let spans = crate::bounded_carriers::spans(&edge.oriented_curve())?;
                for (index, span) in spans.iter().enumerate() {
                    let point = |h: Vector4| Point3::new(h.x / h.w, h.y / h.w, h.z / h.w);
                    if span
                        .iter()
                        .all(|h| (point(*h) - self.origin).cross(self.axis).so_small())
                    {
                        return Err(CadError::Unsupported(
                            "Revolve: A moving curve contains an axis-bound parameter span".into(),
                        ));
                    }
                    if index > 0 && (point(span[0]) - self.origin).cross(self.axis).so_small() {
                        return Err(CadError::Unsupported("Revolve: A spline has an interior axis contact at a knot; split it into regular axis-bounded sections".into()));
                    }
                    let normal = face.oriented_surface().normal(0., 0.).normalize();
                    let radial = normal.cross(self.axis);
                    if span
                        .iter()
                        .any(|h| (point(*h) - self.origin).dot(radial) < -1e-7)
                    {
                        return Err(CadError::Unsupported(
                            "Revolve: The moving curve has an uncertain axis-crossing control hull"
                                .into(),
                        ));
                    }
                }
            }
            let shell: Shell = builder::revolve_wire(&path, self.origin, self.axis, sweep, 8);
            if shell.len() != 8 * path.len() {
                return Err(invalid(
                    "Angular constructor changed its source member topology",
                ));
            }
            for (index, wall) in shell.iter().enumerate() {
                let source = &origins[&path[index % path.len()].id()];
                let purpose = serde_json::json!({"operation":"revolution","constructor":"wall","station":index/path.len(),"side":source.side,"piece":source.piece,"cap":source.cap,"pole_split":source.pole_split});
                let reference =
                    self.generated
                        .tool_face(self.owner, wall, &source.sources, &purpose)?;
                face_refs.insert(wall.id(), reference);
                let boundary = &wall.boundaries()[0];
                let edge = &path[index % path.len()];
                let front_pole = (edge.front().point() - self.origin)
                    .cross(self.axis)
                    .so_small();
                let back_pole = (edge.back().point() - self.origin)
                    .cross(self.axis)
                    .so_small();
                let connectors = [
                    (!front_pole)
                        .then(|| (boundary.back().expect("angular wall"), edge.front().id())),
                    (!back_pole).then(|| (&boundary[1], edge.back().id())),
                ];
                for (connector, vertex) in connectors.into_iter().flatten() {
                    if named_edges.insert(connector.id()) {
                        self.generated.tool_edge(self.owner,connector,&vertices[&vertex],&serde_json::json!({"operation":"revolution","constructor":"angular_connector","station":index/path.len()}))?;
                    }
                }
            }
            if !closed {
                let top = shell
                    .iter()
                    .skip(7 * path.len())
                    .enumerate()
                    .map(|(index, wall)| {
                        let boundary = &wall.boundaries()[0];
                        let first_pole = axis_count > 0 && index == 0;
                        let last_pole = axis_count > 0 && index + 1 == path.len();
                        let slot = if last_pole && !first_pole { 1 } else { 2 };
                        boundary
                            .get(slot)
                            .map(Edge::inverse)
                            .ok_or_else(|| invalid("Missing angular end boundary"))
                    })
                    .collect::<Result<Wire>>()?;
                start_wires.push(path.iter().chain(closing.iter()).cloned().collect::<Wire>());
                end_wires.push(top.iter().chain(closing.iter()).cloned().collect::<Wire>());
            }
            shells.push(shell);
        }
        if !closed {
            let source_refs = origins
                .values()
                .flat_map(|source| source.sources.clone())
                .collect::<Vec<_>>();
            let start = Face::try_new(start_wires, face.oriented_surface())
                .map_err(|error| invalid(&error.to_string()))?
                .inverse();
            let rotated = builder::rotated(&face, self.origin, self.axis, Rad(self.total));
            let end = Face::try_new(end_wires, rotated.oriented_surface())
                .map_err(|error| invalid(&error.to_string()))?;
            for (end_name, cap) in [("start", start.clone()), ("end", end.clone())] {
                let reference=self.generated.tool_face(self.owner,&cap,&source_refs,&serde_json::json!({"operation":"revolution","constructor":"end_cap","end":end_name}))?;
                face_refs.insert(cap.id(), reference);
            }
            let mut combined = Shell::from(vec![start]);
            for shell in shells {
                combined.extend(shell);
            }
            combined.push(end);
            shells = vec![combined];
            for edge in axis_edges {
                let source = &origins[&edge.id()];
                self.generated.tool_edge(self.owner,&edge,&source.sources,&serde_json::json!({"operation":"revolution","constructor":"axis_boundary","side":source.side,"piece":source.piece,"cap":source.cap}))?;
                named_edges.insert(edge.id());
            }
        }
        let solid = Solid::try_new(shells).map_err(|error| invalid(&error.to_string()))?;
        crate::validation::solid(&solid)?;
        let mut edges: HashMap<EdgeId, (Edge, Vec<TopologyRef>)> = HashMap::new();
        for face in solid.face_iter() {
            for edge in face.edge_iter() {
                edges
                    .entry(edge.id())
                    .or_insert_with(|| (edge.clone(), vec![]))
                    .1
                    .push(face_refs[&face.id()].clone());
            }
        }
        let mut roles = HashSet::new();
        for (edge, sources) in edges.values() {
            if named_edges.contains(&edge.id()) {
                continue;
            }
            let reference = self.generated.tool_edge(
                self.owner,
                edge,
                sources,
                &serde_json::json!({"operation":"revolution","constructor":"shared_boundary"}),
            )?;
            if !roles.insert(reference.role) {
                return Err(CadError::Unsupported(
                    "Revolve: Repeated shared boundaries need explicit segmented ancestry".into(),
                ));
            }
        }
        Ok(solid)
    }
}

fn invalid(message: &str) -> CadError {
    CadError::Invalid(format!("Revolve: {message}"))
}
fn angle(doc: &Document, owner: &str, expression: &cad_core::Expr) -> Result<f64> {
    expression::value(doc, owner, expression, Dimension::Angle)
}
fn positive_angle(value: f64) -> Result<f64> {
    if !value.is_finite() || value <= 1e-10 || value > std::f64::consts::TAU {
        return Err(invalid("Each angular extent must be in (0, 360] degrees"));
    }
    Ok(value)
}
fn section_plane(faces: &[Face], origin: Point3, axis: Vector3) -> Result<()> {
    let Some(first) = faces.first() else {
        return Err(invalid("The sketch contains no material regions"));
    };
    let normal = first.oriented_surface().normal(0., 0.).normalize();
    if ![normal.x, normal.y, normal.z].iter().all(|x| x.is_finite())
        || normal.dot(axis).abs() > 1e-8
    {
        return Err(invalid(
            "The axis of revolution must lie in the sketch plane",
        ));
    }
    let radial = normal.cross(axis).normalize();
    let mut low = f64::INFINITY;
    let mut high = f64::NEG_INFINITY;
    for face in faces {
        if !matches!(face.surface(), Surface::Plane(_)) {
            return Err(invalid("Revolution requires planar sketch sections"));
        }
        for point in crate::extrusion::face_hull(face)? {
            let delta = point - origin;
            if delta.dot(normal).abs() > 1e-7 {
                return Err(invalid(
                    "The axis of revolution must lie in every selected section plane",
                ));
            }
            let position = delta.dot(radial);
            low = low.min(position);
            high = high.max(position);
        }
    }
    if !low.is_finite() || !high.is_finite() || high - low <= 1e-7 {
        return Err(invalid("The revolution section is collapsed"));
    }
    if low < -1e-7 && high > 1e-7 {
        return Err(CadError::Unsupported("Revolve: Section control bounds cross the axis. Trim the section to one axis half-plane; uncertain or self-overlapping revolutions require exact collision resolution".into()));
    }
    Ok(())
}
fn separated(a: &Solid, b: &Solid) -> bool {
    crate::boolean_ops::disjoint(a, b)
}
pub(crate) struct Combination<'a> {
    pub adapter: &'a MonstertruckAdapter,
    pub owner: &'a NodeRef,
    pub generated: &'a mut crate::adapter::TopologyHistory,
}
pub(crate) fn combine(
    tools: Vec<Solid>,
    scope: &[NodeRef],
    mode: &str,
    inputs: &BTreeMap<NodeRef, Shape>,
    tolerance: f64,
    context: Combination<'_>,
) -> Result<Shape> {
    if !matches!(mode, "" | "new" | "join" | "cut" | "intersect") {
        return Err(invalid("Unknown result mode"));
    }
    let mut result: Vec<Solid> = Vec::new();
    let scope_refs = scope.iter().cloned().collect::<BTreeSet<_>>();
    for reference in scope {
        let Some(Shape::Bodies(solids)) = inputs.get(reference) else {
            return Err(invalid(
                "The selected scope must produce current solid bodies",
            ));
        };
        for solid in solids {
            let identity = solid.face_iter().map(Face::id).collect::<HashSet<_>>();
            if !result
                .iter()
                .any(|other| other.face_iter().map(Face::id).collect::<HashSet<_>>() == identity)
            {
                result.push(solid.clone());
            }
        }
    }
    if result.is_empty() && !matches!(mode, "" | "new") {
        return Err(invalid("Select a target body for the result"));
    }
    if mode == "intersect" {
        let mut intersections = Vec::new();
        for tool in tools {
            crate::validation::solid(&tool)?;
            for body in &result {
                if separated(body, &tool) {
                    continue;
                }
                if let Some(common) = crate::boolean_ops::binary(
                    body,
                    &tool,
                    crate::boolean_ops::Mode::Intersection,
                    tolerance,
                )? {
                    intersections.push(common);
                }
            }
        }
        return Ok(Shape::Bodies(intersections));
    }
    for tool in tools {
        crate::validation::solid(&tool)?;
        if matches!(mode, "" | "new") {
            result.push(tool);
            continue;
        }
        if mode == "join" {
            let mut joined = tool;
            let mut retained = Vec::new();
            for body in result {
                if separated(&body, &joined) {
                    retained.push(body);
                    continue;
                }
                if let Some(contact) = crate::join::contact(&body, &joined)? {
                    crate::join::record_history(
                        context.adapter,
                        context.owner,
                        &scope_refs,
                        &contact,
                        context.generated,
                    )?;
                    joined = contact.solid;
                } else {
                    joined = crate::boolean_ops::union_with_history(
                        &body,
                        &joined,
                        tolerance,
                        &scope_refs,
                        Combination {
                            adapter: context.adapter,
                            owner: context.owner,
                            generated: context.generated,
                        },
                    )?;
                }
            }
            retained.push(joined);
            result = retained;
            continue;
        }
        let mut next = Vec::new();
        for body in result {
            if separated(&body, &tool) {
                if mode == "cut" {
                    next.push(body);
                }
                continue;
            }
            if let Some(combined) = crate::boolean_ops::binary(
                &body,
                &tool,
                if mode == "cut" {
                    crate::boolean_ops::Mode::Difference
                } else {
                    crate::boolean_ops::Mode::Intersection
                },
                tolerance,
            )? {
                next.push(combined);
            }
        }
        result = next;
    }
    Ok(Shape::Bodies(result))
}

pub(crate) fn operation(
    adapter: &MonstertruckAdapter,
    doc: &Document,
    owner: &NodeRef,
    operation: &Operation,
    inputs: &BTreeMap<NodeRef, Shape>,
    tolerance: f64,
    generated: &mut TopologyHistory,
) -> Result<Shape> {
    let Operation::Revolve {
        profile,
        origin,
        axis,
        axis_reference,
        angle: first,
        body,
        mode,
        options,
    } = operation
    else {
        return Err(invalid("Expected a revolution operation"));
    };
    let axis_frame = axis_reference
        .as_ref()
        .map(|source| adapter.resolve_surface_axis(doc, source, inputs))
        .transpose()?;
    let origin = Point3::from(
        axis_frame
            .as_ref()
            .map(|axis| axis.origin)
            .unwrap_or(*origin),
    );
    let axis = Vector3::from(
        axis_frame
            .as_ref()
            .map(|axis| axis.direction)
            .unwrap_or(*axis),
    );
    let length = axis.magnitude();
    if ![origin.x, origin.y, origin.z, length]
        .iter()
        .all(|x| x.is_finite())
        || length <= 1e-12
    {
        return Err(invalid(
            "The axis needs a finite origin and nonzero direction",
        ));
    }
    let axis = axis / length * if options.reverse { -1. } else { 1. };
    let first = positive_angle(angle(doc, &owner.definition, first)?)?;
    let second = options
        .second_angle
        .as_ref()
        .map(|expression| angle(doc, &owner.definition, expression).and_then(positive_angle))
        .transpose()?;
    if options.midplane && second.is_some() {
        return Err(invalid("Disable Direction 2 when using Mid Plane"));
    }
    let total = first + second.unwrap_or(0.);
    if total > std::f64::consts::TAU {
        return Err(invalid("The combined directions cannot exceed 360 degrees"));
    }
    let offset = options
        .start_angle
        .as_ref()
        .map(|expression| angle(doc, &owner.definition, expression))
        .transpose()?
        .unwrap_or(0.);
    if !offset.is_finite() {
        return Err(invalid("The start angle must be finite"));
    }
    let start = offset.rem_euclid(std::f64::consts::TAU)
        - if options.midplane {
            first / 2.
        } else {
            second.unwrap_or(0.)
        };
    cad_core::contours::validate_profiles(std::slice::from_ref(profile), &options.contours)?;
    let sections = if let Some(thin) = &options.thin {
        crate::extrusion::thin_profiles_traced(
            adapter,
            doc,
            profile,
            thin,
            inputs,
            &owner.definition,
            &options.contours,
        )?
    } else {
        crate::extrusion::profiles_traced(adapter, doc, profile, inputs, &options.contours)?
    };
    section_plane(
        &sections
            .iter()
            .map(|section| section.face.clone())
            .collect::<Vec<_>>(),
        origin,
        axis,
    )?;
    let mut construction = Construction {
        origin,
        axis,
        start,
        total,
        owner,
        generated,
    };
    let tools = sections
        .iter()
        .map(|section| construction.section(section, profile))
        .collect::<Result<Vec<_>>>()?;
    let scope = body
        .iter()
        .chain(&options.scope)
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    combine(
        tools,
        &scope,
        mode,
        inputs,
        tolerance,
        Combination {
            adapter,
            owner,
            generated,
        },
    )
}
