//! Join opposing planar contact faces without asking a volume Boolean to
//! classify coincident caps. All boundaries retain their native carriers.
use crate::adapter::MonstertruckAdapter;
use cad_core::{CadError, NodeRef, Result};
use monstertruck::modeling::*;
use std::collections::{BTreeSet, HashMap, HashSet};
pub(crate) mod region;

const EPS: f64 = 1e-7;
const MAX_FACES: usize = 4096;
const MAX_EDGES: usize = 16384;
const MAX_CONTROLS: usize = 262144;
const MAX_RESULT_FACES: usize = 8192;
const MAX_PAIR_WORK: usize = 2_000_000;
const MAX_HULL_WORK: usize = 64_000_000;

pub(crate) struct ContactFace {
    pub face: FaceId,
    pub sources: Vec<FaceId>,
    pub component: usize,
}
pub(crate) struct ContactEdge {
    pub edge: EdgeId,
    pub sources: Vec<EdgeId>,
    pub interval: usize,
}
pub(crate) struct ContactOutput {
    pub solid: Solid,
    pub trimmed: Vec<ContactFace>,
    pub face_aliases: Vec<(FaceId, FaceId)>,
    pub edge_aliases: Vec<(EdgeId, EdgeId)>,
    pub split_edges: Vec<ContactEdge>,
}
fn invalid(message: impl std::fmt::Display) -> CadError {
    CadError::Invalid(format!("Geometry: planar boss join: {message}"))
}
fn unsupported(message: &str) -> CadError {
    CadError::Unsupported(format!("Planar boss join: {message}"))
}
fn finite(point: Point3) -> bool {
    [point.x, point.y, point.z]
        .into_iter()
        .all(|value| value.is_finite() && value.abs() <= 1e12)
}
fn admit(work: &mut usize, a: usize, b: usize) -> Result<()> {
    *work = a
        .checked_mul(b)
        .and_then(|amount| work.checked_add(amount))
        .ok_or_else(|| unsupported("Contact native predicate work overflowed"))?;
    if *work > MAX_HULL_WORK {
        return Err(unsupported(
            "Contact proofs exceed their bounded native predicate work",
        ));
    }
    Ok(())
}
fn preflight(solid: &Solid) -> Result<()> {
    if solid.face_iter().take(MAX_FACES + 1).count() > MAX_FACES
        || solid.edge_iter().take(MAX_EDGES + 1).count() > MAX_EDGES
    {
        return Err(unsupported(
            "Native contact topology exceeds its bounded face/edge budget",
        ));
    }
    crate::split::control_cost(solid, MAX_CONTROLS)?;
    Ok(())
}
fn hull(solid: &Solid) -> Result<Option<Vec<Point3>>> {
    // Conversion of an analytic/implicit surface is not a contact proof.
    // Native spline surfaces have exact positive homogeneous control hulls.
    if solid.face_iter().any(|face| {
        !matches!(
            face.surface(),
            Surface::Plane(_) | Surface::BsplineSurface(_) | Surface::NurbsSurface(_)
        )
    }) {
        return Ok(None);
    }
    let points = match crate::adapter::solid_control_points(solid) {
        Ok(points) => points,
        Err(CadError::Unsupported(_)) => return Ok(None),
        Err(error) => return Err(error),
    };
    if points.is_empty() || points.iter().any(|point| !finite(*point)) {
        return Err(invalid("A finite positive native control hull is required"));
    }
    Ok(Some(points))
}
struct PlaneFace {
    face: Face,
    origin: Point3,
    normal: Vector3,
}
fn planar_faces(
    solid: &Solid,
    hull: &[Point3],
    require_support: bool,
    work: &mut usize,
) -> Result<Vec<PlaneFace>> {
    let mut result = Vec::new();
    for face in solid.face_iter() {
        let Some(vertex) = face.vertex_iter().next() else {
            continue;
        };
        let origin = vertex.point();
        let normal = face.oriented_surface().normal(0., 0.);
        let length = normal.magnitude();
        if !length.is_finite() || length < 1e-12 {
            continue;
        }
        let normal = normal / length;
        let controls = match crate::extrusion::face_hull(face) {
            Ok(controls) => controls,
            Err(CadError::Unsupported(_)) => continue,
            Err(error) => return Err(error),
        };
        if !controls
            .iter()
            .all(|point| (*point - origin).dot(normal).abs() <= EPS)
        {
            continue;
        }
        if require_support {
            admit(work, hull.len(), 2)?;
            if !hull
                .iter()
                .all(|point| (*point - origin).dot(normal) <= EPS)
                || !hull
                    .iter()
                    .any(|point| (*point - origin).dot(normal) < -EPS)
            {
                continue;
            }
        }
        result.push(PlaneFace {
            face: face.clone(),
            origin,
            normal,
        });
    }
    Ok(result)
}
fn straight(edge: &Edge) -> Result<bool> {
    match crate::bounded_carriers::line(&edge.oriented_curve()) {
        Ok(()) => Ok(true),
        Err(CadError::Unsupported(_)) => Ok(false),
        Err(error) => Err(error),
    }
}
fn convex_polygon(wire: &Wire, normal: Vector3) -> Result<Option<Vec<(Point3, Vector3)>>> {
    if wire.len() < 3 {
        return Ok(None);
    }
    let mut sides = Vec::new();
    for edge in wire {
        if !straight(edge)? {
            return Ok(None);
        }
        let delta = edge.back().point() - edge.front().point();
        let length = delta.magnitude();
        if !length.is_finite() || length <= EPS {
            return Ok(None);
        }
        sides.push((edge.front().point(), normal.cross(delta / length)));
    }
    if sides.iter().all(|(start, inward)| {
        wire.iter()
            .all(|edge| (edge.front().point() - *start).dot(*inward) >= -EPS)
    }) {
        Ok(Some(sides))
    } else {
        Ok(None)
    }
}
fn wire_hull(wire: &Wire) -> Result<Vec<Point3>> {
    let mut result = Vec::new();
    for edge in wire {
        result.extend(crate::extrusion::curve_hull(&edge.curve())?);
    }
    Ok(result)
}
fn frame(normal: Vector3) -> [Vector3; 2] {
    let u = normal
        .cross(if normal.x.abs() < 0.8 {
            Vector3::unit_x()
        } else {
            Vector3::unit_y()
        })
        .normalize();
    [u, normal.cross(u)]
}
fn projection_range(points: &[Point3], origin: Point3, axis: Vector3) -> (f64, f64) {
    points
        .iter()
        .map(|point| (*point - origin).dot(axis))
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), value| {
            (lo.min(value), hi.max(value))
        })
}
fn local_clear(
    support: &PlaneFace,
    stock_faces: &[(FaceId, Vec<Point3>)],
    tool_hull: &[Point3],
    work: &mut usize,
) -> Result<bool> {
    let [u, v] = frame(support.normal);
    admit(work, tool_hull.len(), 5)?;
    let axes = [
        Vector3::unit_x(),
        Vector3::unit_y(),
        Vector3::unit_z(),
        u,
        v,
    ];
    let tool_ranges = axes.map(|axis| projection_range(tool_hull, support.origin, axis));
    for (face, controls) in stock_faces {
        if *face == support.face.id() {
            continue;
        }
        admit(work, controls.len(), 6)?;
        if controls
            .iter()
            .all(|point| (*point - support.origin).dot(support.normal) <= EPS)
        {
            continue;
        }
        if !axes.into_iter().enumerate().any(|(index, axis)| {
            let (alo, ahi) = projection_range(controls, support.origin, axis);
            let (blo, bhi) = tool_ranges[index];
            ahi < blo - EPS || bhi < alo - EPS
        }) {
            return Ok(false);
        }
    }
    Ok(true)
}
fn strict_inset(stock: &PlaneFace, cap: &Face, work: &mut usize) -> Result<bool> {
    let stock_wires = stock.face.boundaries();
    let cap_wires = cap.boundaries();
    let (Some(outer), Some(contact)) = (stock_wires.first(), cap_wires.first()) else {
        return Ok(false);
    };
    let contact_hull = wire_hull(contact)?;
    admit(work, outer.len(), outer.len())?;
    let contained = if let Some(sides) = convex_polygon(outer, stock.normal)? {
        if let Some((center, _, radius)) = crate::prismatic_shell::wire_circle(contact)? {
            sides
                .iter()
                .all(|(start, inward)| (center - *start).dot(*inward) - radius > EPS)
        } else {
            admit(work, sides.len(), contact_hull.len())?;
            sides.iter().all(|(start, inward)| {
                contact_hull
                    .iter()
                    .all(|point| (*point - *start).dot(*inward) > EPS)
            })
        }
    } else if let Some((center, _, radius)) = crate::prismatic_shell::wire_circle(outer)? {
        if let Some((contact_center, _, contact_radius)) =
            crate::prismatic_shell::wire_circle(contact)?
        {
            center.distance(contact_center) + contact_radius < radius - EPS
        } else {
            contact_hull
                .iter()
                .all(|point| point.distance(center) < radius - EPS)
        }
    } else {
        false
    };
    if !contained {
        return Ok(false);
    }
    // Old holes must not intersect the contact region. A separating bound in
    // the actual face frame is conservative and independent of world axes.
    let [u, v] = frame(stock.normal);
    admit(work, contact_hull.len(), 2)?;
    let contact_ranges = [
        projection_range(&contact_hull, stock.origin, u),
        projection_range(&contact_hull, stock.origin, v),
    ];
    for hole in stock_wires.iter().skip(1) {
        let hole_hull = wire_hull(hole)?;
        admit(work, hole_hull.len(), 2)?;
        if ![u, v].into_iter().enumerate().any(|(index, axis)| {
            let (alo, ahi) = projection_range(&hole_hull, stock.origin, axis);
            let (blo, bhi) = contact_ranges[index];
            ahi < blo - EPS || bhi < alo - EPS
        }) {
            return Ok(false);
        }
    }
    Ok(true)
}
pub(crate) fn matching_edge(stock: &Edge, contact: &Edge) -> Result<bool> {
    if stock.front().point().distance(contact.front().point()) > EPS
        || stock.back().point().distance(contact.back().point()) > EPS
    {
        return Ok(false);
    }
    if straight(stock)? && straight(contact)? {
        return Ok(true);
    }
    let a = crate::bounded_carriers::circle(&stock.oriented_curve());
    let b = crate::bounded_carriers::circle(&contact.oriented_curve());
    if let (Ok(((ac, an, ar), aa)), Ok(((bc, bn, br), ba))) = (a, b) {
        return Ok(ac.distance(bc) <= EPS
            && (ar - br).abs() <= EPS
            && (an * aa - bn * ba).magnitude() <= 1e-9
            && aa.abs() <= std::f64::consts::TAU + 1e-9);
    }
    // Exact native rational representation equality supports authored spline
    // outlines too, without fitting implicit intersection/parameter guides.
    let a = stock.oriented_curve();
    let b = contact.oriented_curve();
    Ok(matches!(
        a,
        Curve::Line(_) | Curve::BsplineCurve(_) | Curve::NurbsCurve(_)
    ) && matches!(
        b,
        Curve::Line(_) | Curve::BsplineCurve(_) | Curve::NurbsCurve(_)
    ) && crate::surfaces::same_curve(&a, &b, EPS))
}
fn outline_map(
    stock: &Face,
    cap: &Face,
    work: &mut usize,
) -> Result<Option<HashMap<EdgeId, Edge>>> {
    let stock_wires = stock.boundaries();
    let cap_wires = cap.boundaries();
    if stock_wires.len() != cap_wires.len() {
        return Ok(None);
    }
    let mut used = HashSet::new();
    let mut result = HashMap::new();
    for (index, wire) in cap_wires.iter().enumerate() {
        let mut candidates = Vec::new();
        for (stock_index, stock_wire) in stock_wires.iter().enumerate() {
            if (index == 0) != (stock_index == 0) || wire.len() != stock_wire.len() {
                continue;
            }
            let cost = wire
                .len()
                .checked_mul(stock_wire.len())
                .ok_or_else(|| unsupported("Contact outline work overflowed"))?;
            *work = work
                .checked_add(cost)
                .ok_or_else(|| unsupported("Contact outline work overflowed"))?;
            if *work > MAX_PAIR_WORK {
                return Err(unsupported(
                    "Contact outline matching exceeds its bounded work budget",
                ));
            }
            let mut matched = HashMap::new();
            let mut edge_used = HashSet::new();
            for edge in wire {
                let mut matches = Vec::new();
                for old in stock_wire {
                    let replacement = old.inverse();
                    if matching_edge(&replacement, edge)? {
                        matches.push(replacement);
                    }
                }
                let [replacement] = matches.as_slice() else {
                    matched.clear();
                    break;
                };
                if !edge_used.insert(replacement.id()) {
                    matched.clear();
                    break;
                }
                matched.insert(
                    edge.id(),
                    if edge.orientation() {
                        replacement.clone()
                    } else {
                        replacement.inverse()
                    },
                );
            }
            if matched.len() == wire.len() {
                candidates.push((stock_index, matched));
            }
        }
        let [(stock_index, matched)] = candidates.as_slice() else {
            return Ok(None);
        };
        if !used.insert(*stock_index) {
            return Ok(None);
        }
        result.extend(matched.clone());
    }
    Ok(Some(result))
}
fn append_shells(
    stock: &Solid,
    tool: &Solid,
    stock_face: FaceId,
    cap: FaceId,
    replacements: Vec<Face>,
) -> Result<Solid> {
    let mut shells = stock.boundaries().clone();
    let Some(shell) = shells
        .iter_mut()
        .find(|shell| shell.iter().any(|face| face.id() == stock_face))
    else {
        return Err(invalid("The contact stock face is missing"));
    };
    shell.retain(|face| face.id() != stock_face);
    shell.extend(replacements);
    shell.extend(tool.face_iter().filter(|face| face.id() != cap).cloned());
    let solid = Solid::try_new(shells).map_err(invalid)?;
    crate::validation::solid(&solid)?;
    Ok(solid)
}
fn inset(
    stock: &Solid,
    tool: &Solid,
    support: &PlaneFace,
    cap: &Face,
    work: &mut usize,
) -> Result<ContactOutput> {
    let contact = cap.boundaries();
    if stock.face_iter().count() + tool.face_iter().count() - 2 + contact.len() > MAX_RESULT_FACES {
        return Err(unsupported(
            "Contact islands exceed the bounded output face budget",
        ));
    }
    admit(work, contact.len(), tool.edge_iter().count())?;
    let mut boundaries = support.face.boundaries();
    boundaries.push(contact[0].clone());
    let mut replacements =
        vec![Face::try_new(boundaries, support.face.oriented_surface()).map_err(invalid)?];
    for island in contact.iter().skip(1) {
        replacements.push(
            Face::try_new(vec![island.clone()], support.face.oriented_surface())
                .map_err(invalid)?,
        );
    }
    let mut trimmed = Vec::new();
    for face in &replacements {
        let edges = face
            .edge_iter()
            .map(|edge| edge.id())
            .collect::<HashSet<_>>();
        let mut sources = vec![support.face.id()];
        sources.extend(
            tool.face_iter()
                .filter(|face| {
                    face.id() != cap.id() && face.edge_iter().any(|edge| edges.contains(&edge.id()))
                })
                .map(Face::id),
        );
        trimmed.push(ContactFace {
            face: face.id(),
            sources,
            component: trimmed.len(),
        });
    }
    Ok(ContactOutput {
        solid: append_shells(stock, tool, support.face.id(), cap.id(), replacements)?,
        trimmed,
        face_aliases: vec![],
        edge_aliases: vec![],
        split_edges: vec![],
    })
}
fn continuation(
    stock: &Solid,
    tool: &Solid,
    support: &PlaneFace,
    cap: &Face,
    mut edge_map: HashMap<EdgeId, Edge>,
) -> Result<ContactOutput> {
    let mut vertices: HashMap<VertexId, Vertex> = HashMap::new();
    let mut used: HashMap<VertexId, VertexId> = HashMap::new();
    for edge in cap.edge_iter() {
        let replacement = &edge_map[&edge.id()];
        // The mapped edge is oriented as the original tool's absolute edge,
        // even when its underlying retained stock edge has inverse orientation.
        for (old, new) in [
            (edge.absolute_front(), replacement.front()),
            (edge.absolute_back(), replacement.back()),
        ] {
            if vertices
                .get(&old.id())
                .is_some_and(|vertex| vertex.id() != new.id())
                || used.get(&new.id()).is_some_and(|id| *id != old.id())
            {
                return Err(unsupported(
                    "Coincident contact vertices do not have a unique topology correspondence",
                ));
            }
            vertices.insert(old.id(), new.clone());
            used.insert(new.id(), old.id());
        }
    }
    let mut edge_aliases = Vec::new();
    for edge in tool.edge_iter() {
        if edge_map.contains_key(&edge.id()) {
            continue;
        }
        let a = edge.absolute_front();
        let b = edge.absolute_back();
        let front = vertices.get(&a.id()).unwrap_or(a);
        let back = vertices.get(&b.id()).unwrap_or(b);
        let mapped = if front.id() != a.id() || back.id() != b.id() {
            if front.id() == back.id() && a.id() != b.id() {
                return Err(invalid("Contact sewing would collapse a native edge"));
            }
            let mapped = Edge::new(front, back, edge.curve());
            edge_aliases.push((mapped.id(), edge.id()));
            mapped
        } else {
            if edge.orientation() {
                edge.clone()
            } else {
                edge.inverse()
            }
        };
        edge_map.insert(edge.id(), mapped);
    }
    let mut face_aliases = Vec::new();
    let mut replacements = Vec::new();
    for face in tool.face_iter().filter(|face| face.id() != cap.id()) {
        let mut changed = false;
        let boundaries = face
            .boundaries()
            .iter()
            .map(|wire| {
                wire.iter()
                    .map(|edge| {
                        let mapped = &edge_map[&edge.id()];
                        changed |= mapped.id() != edge.id();
                        if edge.orientation() {
                            mapped.clone()
                        } else {
                            mapped.inverse()
                        }
                    })
                    .collect::<Wire>()
            })
            .collect::<Vec<_>>();
        if changed {
            let mapped = Face::try_new(boundaries, face.oriented_surface()).map_err(invalid)?;
            face_aliases.push((mapped.id(), face.id()));
            replacements.push(mapped);
        } else {
            replacements.push(face.clone());
        }
    }
    let mut shells = stock.boundaries().clone();
    let Some(shell) = shells
        .iter_mut()
        .find(|shell| shell.iter().any(|face| face.id() == support.face.id()))
    else {
        return Err(invalid("The continuation stock face is missing"));
    };
    shell.retain(|face| face.id() != support.face.id());
    shell.extend(replacements);
    let solid = Solid::try_new(shells).map_err(invalid)?;
    crate::validation::solid(&solid)?;
    Ok(ContactOutput {
        solid,
        trimmed: vec![],
        face_aliases,
        edge_aliases,
        split_edges: vec![],
    })
}

/// None means that this exact planar-contact constructor cannot prove the
/// arrangement; the caller may still use its validated general native Boolean.
pub(crate) fn contact(stock: &Solid, tool: &Solid) -> Result<Option<ContactOutput>> {
    if tool.boundaries().len() != 1 {
        return Ok(None);
    }
    preflight(stock)?;
    preflight(tool)?;
    let (Some(stock_hull), Some(tool_hull)) = (hull(stock)?, hull(tool)?) else {
        return Ok(None);
    };
    let mut hull_work = 0;
    let stock_planes = planar_faces(stock, &stock_hull, false, &mut hull_work)?;
    let tool_planes = planar_faces(tool, &tool_hull, true, &mut hull_work)?;
    let mut stock_faces = None;
    let mut work = stock_planes
        .len()
        .checked_mul(tool_planes.len())
        .ok_or_else(|| unsupported("Contact face work overflowed"))?;
    if work > MAX_PAIR_WORK {
        return Err(unsupported(
            "Contact faces exceed their bounded matching work",
        ));
    }
    let mut candidate = None;
    for support in &stock_planes {
        for cap in &tool_planes {
            if support.normal.dot(cap.normal) > -1. + 1e-10
                || (cap.origin - support.origin).dot(support.normal).abs() > EPS
            {
                continue;
            }
            admit(&mut hull_work, stock_hull.len() + tool_hull.len(), 1)?;
            // The tool occupies only the outgoing side of its contact cap.
            if !tool_hull
                .iter()
                .all(|point| (*point - support.origin).dot(support.normal) >= -EPS)
            {
                continue;
            }
            if !stock_hull
                .iter()
                .all(|point| (*point - support.origin).dot(support.normal) <= EPS)
            {
                if stock_faces.is_none() {
                    stock_faces = Some(
                        stock
                            .face_iter()
                            .map(|face| Ok((face.id(), crate::extrusion::face_hull(face)?)))
                            .collect::<Result<Vec<_>>>()?,
                    );
                }
                // On a stepped stock or after another boss, a globally
                // supporting plane is unavailable. Every old boundary on the
                // outgoing side must instead be strictly separated from the
                // entire connected tool's exact native hull. Together with an
                // outgoing contact in the exposed face this rules out crossing
                // into old material anywhere along the new boss.
                if !local_clear(
                    support,
                    stock_faces.as_ref().unwrap(),
                    &tool_hull,
                    &mut hull_work,
                )? {
                    continue;
                }
            }
            let output = if let Some(map) = outline_map(&support.face, &cap.face, &mut work)? {
                Some(continuation(stock, tool, support, &cap.face, map)?)
            } else if strict_inset(support, &cap.face, &mut hull_work)? {
                Some(inset(stock, tool, support, &cap.face, &mut hull_work)?)
            } else {
                region::contact(stock, tool, support, &cap.face, &mut work)?
            };
            if let Some(output) = output {
                if candidate.is_some() {
                    return Err(unsupported(
                        "More than one planar contact arrangement is admissible; select separate body scopes",
                    ));
                }
                candidate = Some(output);
            }
        }
    }
    Ok(candidate)
}

pub(crate) fn record_history(
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
            &serde_json::json!({"operation":"extrusion_join","constructor":"contact_trim",
                "base": crate::adapter::construction_reference(&sources[0])?, "component":tag.component}),
        )?;
    }
    for tag in &output.split_edges {
        let sources = tag
            .sources
            .iter()
            .map(|original| {
                generated
                    .edge_source(*original)
                    .or_else(|| {
                        scope
                            .iter()
                            .find_map(|body| adapter.edge_source(body, *original))
                    })
                    .ok_or_else(|| {
                        CadError::Unsupported(
                            "Planar contact split lost its source-edge ancestry".into(),
                        )
                    })
            })
            .collect::<Result<Vec<_>>>()?;
        let edge = output
            .solid
            .edge_iter()
            .find(|edge| edge.id() == tag.edge)
            .ok_or_else(|| invalid("Planar contact split has no output boundary"))?;
        generated.tool_edge(owner, &edge, &sources, &serde_json::json!({
            "operation":"extrusion_join", "constructor":"contact_interval",
            "base":crate::adapter::construction_reference(&sources[0])?, "interval":tag.interval,
        }))?;
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
