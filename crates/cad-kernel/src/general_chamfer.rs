//! Exact conical bevels and toroidal rounds on complete turned rims.
//! Circular controls remain rational; shared rails are built once and reused.
//! No relay points, sampled strips, or display meshes are construction inputs.
use crate::{planar::ChamferSpec, shell_history::ShellHistory};
use cad_core::{CadError, Result};
use monstertruck::{
    geometry::prelude::{TryIntoHomogeneousBsplineCurve, TryIntoHomogeneousBsplineSurface},
    modeling::*,
};
use std::collections::{HashMap, HashSet};
#[path = "concave_treatment.rs"]
mod concave_treatment;
#[path = "local_blend.rs"]
mod local_blend;

const EPS: f64 = 1e-7;
const MAX_FACES: usize = 4096;
const MAX_CONTROLS: usize = 131_072;
const MAX_PROOF_WORK: usize = 4_000_000;

#[derive(Clone, Copy)]
pub(crate) enum Treatment {
    Chamfer,
    Fillet,
}
impl Treatment {
    fn name(self) -> &'static str {
        match self {
            Self::Chamfer => "chamfer",
            Self::Fillet => "fillet",
        }
    }
}

fn invalid(message: &str) -> CadError {
    CadError::Invalid(format!("Circular edge treatment: {message}"))
}
fn unsupported(message: &str) -> CadError {
    CadError::Unsupported(format!("Circular edge treatment: {message}"))
}
fn point(h: Vector4) -> Result<Point3> {
    if ![h.x, h.y, h.z, h.w].iter().all(|v| v.is_finite()) || h.w <= 0. {
        return Err(unsupported(
            "A carrier requires finite positive rational weights",
        ));
    }
    let p = Point3::from_vec(h.truncate() / h.w);
    if ![p.x, p.y, p.z].iter().all(|v| v.is_finite()) {
        return Err(invalid("A carrier exceeds finite coordinates"));
    }
    Ok(p)
}

struct Wall {
    face: Face,
    net: BsplineSurface<Vector4>,
    transposed: bool,
    ends: [Point3; 2],
    radii: [f64; 2],
    scales: [f64; 2],
    rims: [Edge; 2],
    offsets: [Vector3; 2],
}
struct Rim {
    cap: Face,
    edge: Edge,
    center: Point3,
    axis: Vector3,
    radius: f64,
    wall_radius: f64,
    profile_weight: f64,
    radial: f64,
    depth: f64,
    side: f64,
    axial_side: f64,
    wall: FaceId,
    column: usize,
}
impl Rim {
    fn plane_point(&self, p: Point3) -> Point3 {
        self.center + (p - self.center) * (1. - self.side * self.radial / self.radius)
    }
    fn wall_point(&self, p: Point3) -> Point3 {
        self.center
            + (p - self.center) * (self.wall_radius / self.radius)
            + self.axis * (self.axial_side * self.depth)
    }
    fn radii(&self) -> (f64, f64) {
        let other = self.radius - self.side * self.radial;
        (
            self.radius.min(other).min(self.wall_radius),
            self.radius.max(other).max(self.wall_radius),
        )
    }
}

fn carrier(face: &Face) -> Result<Wall> {
    let mut net = face
        .oriented_surface()
        .try_into_homogeneous_bspline_surface()
        .ok_or_else(|| unsupported("A rim wall needs an exact rational turned carrier"))?;
    if net
        .control_points()
        .len()
        .saturating_mul(net.control_points().first().map_or(0, Vec::len))
        > MAX_CONTROLS
    {
        return Err(unsupported(
            "The turned carrier exceeds the rational control budget",
        ));
    }
    if net.control_points().is_empty() || net.control_points()[0].is_empty() {
        return Err(invalid("The turned carrier has no controls"));
    }
    let transposed = net.vdegree() != 1 || net.control_points()[0].len() != 2;
    if transposed {
        net.invert();
    }
    if net.vdegree() != 1 || net.control_points()[0].len() != 2 || !net.is_clamped() {
        return Err(unsupported(
            "A rim wall needs a clamped linear axial parameter",
        ));
    }
    let curves: Vec<_> = (0..2)
        .map(|column| {
            BsplineCurve::try_new(
                net.knot_vector_u().clone(),
                net.control_points().iter().map(|row| row[column]).collect(),
            )
            .map(|curve| Curve::NurbsCurve(NurbsCurve::new(curve)))
            .map_err(|_| unsupported("A rim wall has invalid circular column knots"))
        })
        .collect::<Result<_>>()?;
    let circles = [
        crate::bounded_carriers::circle(&curves[0])?.0,
        crate::bounded_carriers::circle(&curves[1])?.0,
    ];
    let delta = circles[1].0 - circles[0].0;
    let length = delta.magnitude();
    if !length.is_finite()
        || length <= EPS
        || circles
            .iter()
            .any(|(_, normal, _)| normal.dot(delta / length).abs() < 1. - 1e-10)
    {
        return Err(unsupported(
            "The adjacent carrier is not an exact cylinder or cone",
        ));
    }
    let weight_ratio = net.control_points()[0][1].w / net.control_points()[0][0].w;
    let radial_ratio = circles[1].2 / circles[0].2;
    if !radial_ratio.is_finite() || radial_ratio <= 0. {
        return Err(unsupported(
            "A turned wall reaches or crosses its cone apex",
        ));
    }
    for row in net.control_points() {
        if (point(row[1])? - circles[1].0 - (point(row[0])? - circles[0].0) * radial_ratio)
            .magnitude()
            > EPS
            || (row[1].w / row[0].w - weight_ratio).abs() > 1e-12 * weight_ratio.abs()
        {
            return Err(unsupported(
                "Circular wall columns do not prove an exact coaxial homothety",
            ));
        }
    }
    let wires = face.boundaries();
    if wires.len() != 1 || wires[0].len() != 4 {
        return Err(unsupported(
            "A turned patch needs two bounded circular rims and two axial seams",
        ));
    }
    let mut rims = Vec::new();
    for (column, circle) in circles.iter().enumerate() {
        let matches: Vec<_> = wires[0]
            .iter()
            .filter(|edge| {
                crate::bounded_carriers::circle(&edge.curve()).is_ok_and(|(c, _)| {
                    c.0.distance(circle.0) <= EPS
                        && (c.2 - circle.2).abs() <= EPS
                        && c.1.dot(circle.1).abs() > 1. - 1e-10
                }) && corresponding_arc(edge, &curves[column])
            })
            .cloned()
            .collect();
        if matches.len() != 1 {
            return Err(unsupported(
                "The turned wall trim does not follow its exact end circles",
            ));
        }
        rims.push(matches[0].clone());
    }
    let source_rims: HashSet<_> = rims.iter().map(Edge::id).collect();
    for edge in &wires[0] {
        if source_rims.contains(&edge.id()) {
            continue;
        }
        crate::bounded_carriers::line(&edge.curve())?;
        let natural_seam = [false, true].iter().any(|at_end| {
            let endpoints = curves
                .iter()
                .map(|curve| {
                    let (a, b) = curve.range_tuple();
                    curve.subs(if *at_end { b } else { a })
                })
                .collect::<Vec<_>>();
            (edge.front().point().distance(endpoints[0]) <= EPS
                && edge.back().point().distance(endpoints[1]) <= EPS)
                || (edge.back().point().distance(endpoints[0]) <= EPS
                    && edge.front().point().distance(endpoints[1]) <= EPS)
        });
        if !natural_seam {
            return Err(unsupported(
                "A turned wall trim does not follow its complete natural generatrix",
            ));
        }
    }
    Ok(Wall {
        face: face.clone(),
        net,
        transposed,
        ends: [circles[0].0, circles[1].0],
        radii: [circles[0].2, circles[1].2],
        scales: [1.; 2],
        rims: [rims[0].clone(), rims[1].clone()],
        offsets: [Vector3::zero(); 2],
    })
}

/// A positive rational control hull lies outside the complete annular bevel
/// slab when one of these separating linear/convex bounds succeeds.
fn separated(hull: &[Point3], rim: &Rim) -> bool {
    if hull.is_empty() {
        return false;
    }
    let (inner, outer) = rim.radii();
    let vectors: Vec<_> = hull.iter().map(|p| *p - rim.center).collect();
    let end = rim.axial_side * rim.depth;
    if vectors.iter().all(|v| v.dot(rim.axis) > end.max(0.) + EPS)
        || vectors.iter().all(|v| v.dot(rim.axis) < end.min(0.) - EPS)
    {
        return true;
    }
    let radial: Vec<_> = vectors
        .iter()
        .map(|v| *v - rim.axis * v.dot(rim.axis))
        .collect();
    if radial.iter().all(|v| v.magnitude() < inner - EPS) {
        return true;
    }
    let mean = radial.iter().copied().sum::<Vector3>() / radial.len() as f64;
    std::iter::once(mean)
        .chain(radial.iter().copied())
        .any(|direction| {
            let length = direction.magnitude();
            length.is_finite()
                && length > EPS
                && radial
                    .iter()
                    .all(|v| v.dot(direction / length) > outer + EPS)
        })
}

/// Equality of circle, endpoints and non-retracing angular extent establishes
/// the complete bounded arc, independently of its rational parameterization.
fn corresponding_arc(edge: &Edge, carrier: &Curve) -> bool {
    let Ok(((_, edge_normal, _), edge_angle)) =
        crate::bounded_carriers::circle(&edge.oriented_curve())
    else {
        return false;
    };
    let Ok(((_, carrier_normal, _), carrier_angle)) = crate::bounded_carriers::circle(carrier)
    else {
        return false;
    };
    let (a, b) = carrier.range_tuple();
    let start = carrier.subs(a);
    let end = carrier.subs(b);
    let edge_rotation = edge_normal * edge_angle;
    let carrier_rotation = carrier_normal * carrier_angle;
    // Signed rotation distinguishes the two semicircles with the same endpoints,
    // and does not depend on a curve's chosen plane-normal orientation.
    (edge.front().point().distance(start) <= EPS
        && edge.back().point().distance(end) <= EPS
        && (edge_rotation - carrier_rotation).magnitude() <= 1e-9)
        || (edge.back().point().distance(start) <= EPS
            && edge.front().point().distance(end) <= EPS
            && (edge_rotation + carrier_rotation).magnitude() <= 1e-9)
}

fn same_ring(a: &Rim, b: &Rim) -> bool {
    a.cap.id() == b.cap.id()
        && a.center.distance(b.center) <= EPS
        && (a.radius - b.radius).abs() <= EPS
}

/// Subdivision narrows positive rational control hulls without changing the
/// authoritative carrier. Exhausted proof budgets reject the operation.
fn curve_separated(curve: &Curve, rim: &Rim, work: &mut usize) -> Result<bool> {
    if !matches!(
        curve,
        Curve::Line(_) | Curve::BsplineCurve(_) | Curve::NurbsCurve(_)
    ) {
        return Err(unsupported(
            "Collision proof requires the native boundary carrier; implicit intersection guides cannot bound material",
        ));
    }
    let Some(curve) = curve.try_into_homogeneous_bspline_curve() else {
        return Ok(separated(&crate::extrusion::curve_hull(curve)?, rim));
    };
    let mut stack = vec![(curve, 0usize)];
    while let Some((mut curve, depth)) = stack.pop() {
        *work = work.saturating_add(curve.control_points().len());
        if *work > MAX_PROOF_WORK {
            return Err(unsupported(
                "The collision proof exceeds its rational control budget",
            ));
        }
        let hull = curve
            .control_points()
            .iter()
            .map(|h| point(*h))
            .collect::<Result<Vec<_>>>()?;
        if separated(&hull, rim) {
            continue;
        }
        if depth == 12 {
            return Ok(false);
        }
        let (a, b) = curve.range_tuple();
        let middle = a + (b - a) * 0.5;
        if middle <= a || middle >= b {
            return Ok(false);
        }
        let other = curve.cut(middle);
        stack.push((curve, depth + 1));
        stack.push((other, depth + 1));
    }
    Ok(true)
}
fn face_separated(face: &Face, rim: &Rim, work: &mut usize) -> Result<bool> {
    let enclosure = crate::native_bounds::face(face)?;
    *work = work.saturating_add(enclosure.len());
    if *work > MAX_PROOF_WORK {
        return Err(unsupported(
            "The collision proof exceeds its rational control budget",
        ));
    }
    let (low, high) = enclosure.projection(rim.center, rim.axis)?;
    let end = rim.axial_side * rim.depth;
    if low > end.max(0.) + EPS || high < end.min(0.) - EPS || separated(enclosure.points(), rim) {
        return Ok(true);
    }
    // A plane's untrimmed parameter rectangle can contain far more than its
    // bounded face. Keep the exact face hull above as its conservative proof.
    if crate::planar::boundary_plane(face)?.is_some() {
        return Ok(false);
    }
    let Some(surface) = crate::native_bounds::subdivision_surface(face)? else {
        return Ok(false);
    };
    let mut stack = vec![(surface, 0usize)];
    while let Some((mut surface, depth)) = stack.pop() {
        let count = surface
            .control_points()
            .len()
            .saturating_mul(surface.control_points().first().map_or(0, Vec::len));
        *work = work.saturating_add(count);
        if count > MAX_CONTROLS || *work > MAX_PROOF_WORK {
            return Err(unsupported(
                "The collision proof exceeds its rational control budget",
            ));
        }
        let hull = surface
            .control_points()
            .iter()
            .flatten()
            .map(|h| point(*h))
            .collect::<Result<Vec<_>>>()?;
        if separated(&hull, rim) {
            continue;
        }
        if depth == 14 {
            return Ok(false);
        }
        let ((u0, u1), (v0, v1)) = surface.range_tuple();
        let other = if depth % 2 == 0 {
            let middle = u0 + (u1 - u0) * 0.5;
            if middle <= u0 || middle >= u1 {
                return Ok(false);
            }
            surface.cut_u(middle)
        } else {
            let middle = v0 + (v1 - v0) * 0.5;
            if middle <= v0 || middle >= v1 {
                return Ok(false);
            }
            surface.cut_v(middle)
        };
        stack.push((surface, depth + 1));
        stack.push((other, depth + 1));
    }
    Ok(true)
}
fn band_separated(source: &Rim, other: &Rim, work: &mut usize) -> Result<bool> {
    let curve = source
        .edge
        .curve()
        .try_into_homogeneous_bspline_curve()
        .ok_or_else(|| unsupported("A treatment band has no exact rational rim"))?;
    let mut stack = vec![(curve, 0usize)];
    while let Some((mut curve, depth)) = stack.pop() {
        *work = work.saturating_add(curve.control_points().len().saturating_mul(3));
        if *work > MAX_PROOF_WORK {
            return Err(unsupported(
                "Selected treatment bands exceed the collision work budget",
            ));
        }
        let mut hull = Vec::new();
        for h in curve.control_points() {
            let original = point(*h)?;
            hull.extend([
                original,
                source.plane_point(original),
                source.wall_point(original),
            ]);
        }
        // These three positive rational rails enclose the entire changed
        // volume, for both a linear bevel and a circular meridian.
        if separated(&hull, other) {
            continue;
        }
        if depth == 12 {
            return Ok(false);
        }
        let (a, b) = curve.range_tuple();
        let middle = a + (b - a) * 0.5;
        if middle <= a || middle >= b {
            return Ok(false);
        }
        let next = curve.cut(middle);
        stack.push((curve, depth + 1));
        stack.push((next, depth + 1));
    }
    Ok(true)
}
fn collision_proof(solid: &Solid, rims: &[Rim]) -> Result<()> {
    let mut work = 0usize;
    for (index, other) in rims.iter().enumerate() {
        if rims[..index].iter().any(|prior| same_ring(prior, other)) {
            continue;
        }
        for source in rims.iter().filter(|source| !same_ring(source, other)) {
            if !band_separated(source, other, &mut work)? {
                return Err(invalid(
                    "Selected circular treatment volumes meet or cannot be separated; reduce their sizes",
                ));
            }
        }
    }
    for (index, rim) in rims.iter().enumerate() {
        if rims[..index].iter().any(|prior| same_ring(prior, rim)) {
            continue;
        }
        // Only this rim's adjacent wall patches can be omitted: another
        // selected wall can still collide with this particular treatment.
        let own_walls = rims
            .iter()
            .filter(|other| same_ring(rim, other))
            .map(|other| other.wall)
            .collect::<HashSet<_>>();
        let own_edges = rims
            .iter()
            .filter(|other| same_ring(rim, other))
            .map(|other| other.edge.id())
            .collect::<HashSet<_>>();
        for face in solid.face_iter() {
            if own_walls.contains(&face.id()) {
                continue;
            }
            if face.id() == rim.cap.id() {
                for edge in face
                    .edge_iter()
                    .filter(|edge| !own_edges.contains(&edge.id()))
                {
                    if !curve_separated(&edge.curve(), rim, &mut work)? {
                        return Err(unsupported(
                            "A circular setback may cross another cap boundary; reduce its size",
                        ));
                    }
                }
            } else if !face_separated(face, rim, &mut work)? {
                return Err(unsupported(
                    "A circular setback may meet another face or cavity; reduce its size",
                ));
            }
        }
        for other in &rims[index + 1..] {
            if same_ring(rim, other) || rim.cap.id() != other.cap.id() {
                continue;
            }
            let a = rim.radius - rim.side * rim.radial;
            let b = other.radius - other.side * other.radial;
            let d = rim.center.distance(other.center);
            let preserved = if d > rim.radius + other.radius + EPS {
                d > a + b + EPS
            } else if d + other.radius < rim.radius - EPS {
                d + b < a - EPS
            } else if d + rim.radius < other.radius - EPS {
                d + a < b - EPS
            } else {
                false
            };
            if !preserved {
                return Err(invalid(
                    "Circular setbacks cross, touch, or change the source cap's containment order",
                ));
            }
        }
    }
    Ok(())
}

fn oriented(edge: &Edge, replacement: &Edge) -> Edge {
    if edge.orientation() {
        replacement.clone()
    } else {
        replacement.inverse()
    }
}
fn remap_wire(wire: &Wire, map: &HashMap<EdgeId, Edge>) -> Wire {
    wire.iter()
        .map(|edge| {
            map.get(&edge.id())
                .map_or_else(|| edge.clone(), |replacement| oriented(edge, replacement))
        })
        .collect()
}
fn replace_curve(edge: &Edge, vertices: &HashMap<VertexId, Vertex>, curve: Curve) -> Result<Edge> {
    let a = edge.absolute_front();
    let b = edge.absolute_back();
    Edge::try_new(
        vertices.get(&a.id()).unwrap_or(a),
        vertices.get(&b.id()).unwrap_or(b),
        curve,
    )
    .map_err(|_| invalid("A setback collapsed a boundary edge"))
}
fn assign_vertex(map: &mut HashMap<VertexId, Vertex>, source: &Vertex, p: Point3) -> Result<()> {
    if let Some(existing) = map.get(&source.id()) {
        if existing.point().distance(p) > EPS {
            return Err(invalid(
                "Selected circular setbacks require incompatible shared vertices",
            ));
        }
    } else {
        map.insert(source.id(), Vertex::new(p));
    }
    Ok(())
}

pub(crate) fn chamfer(
    solid: &Solid,
    selected: &[EdgeId],
    spec: &ChamferSpec,
    history: &mut ShellHistory,
) -> Result<Option<Solid>> {
    construct(solid, selected, spec, Treatment::Chamfer, history)
}

/// Try complete native constructors before decomposing independent selections.
/// No failed candidate, partial component sequence or diagnostic mutates the
/// caller's construction history.
pub(crate) fn construct(
    solid: &Solid,
    selected: &[EdgeId],
    spec: &ChamferSpec,
    treatment: Treatment,
    history: &mut ShellHistory,
) -> Result<Option<Solid>> {
    if selected.is_empty() {
        return Err(invalid("Select at least one edge"));
    }
    if selected.len() > cad_core::edge_features::MAX_SELECTED_EDGES
        || solid.face_iter().count() > MAX_FACES
    {
        return Err(unsupported(
            "The selected body exceeds the edge treatment complexity limit",
        ));
    }
    let selected = selected.iter().copied().collect::<HashSet<_>>();
    let source_edges = solid
        .edge_iter()
        .filter(|edge| selected.contains(&edge.id()))
        .map(|edge| (edge.id(), edge))
        .collect::<HashMap<_, _>>();
    if source_edges.len() != selected.len() {
        return Err(invalid(
            "A selected edge no longer belongs to the source body",
        ));
    }
    if let Some(first) = spec.first_face {
        let face = solid
            .face_iter()
            .find(|face| face.id() == first)
            .ok_or_else(|| invalid("Distance 1 face no longer belongs to the body"))?;
        if !selected
            .iter()
            .all(|id| face.edge_iter().any(|edge| edge.id() == *id))
        {
            return Err(invalid("Distance 1 face must adjoin every selected edge"));
        }
    }
    let mut ordered = selected
        .iter()
        .map(|id| {
            Ok((
                crate::shell_history::closure(&[history.edge(*id)?])?.to_string(),
                *id,
            ))
        })
        .collect::<Result<Vec<_>>>()?;
    ordered.sort_by(|a, b| a.0.cmp(&b.0));
    let ordered = ordered.into_iter().map(|(_, id)| id).collect::<Vec<_>>();
    let original_diagnostic = match attempt(solid, &ordered, spec, treatment, history) {
        Ok(Some(output)) => return Ok(Some(output)),
        Ok(None) => None,
        Err(error @ CadError::Unsupported(_)) => Some(error),
        Err(error) => return Err(error),
    };
    let mut incident: HashMap<VertexId, Vec<EdgeId>> = HashMap::new();
    for edge in source_edges.values() {
        for vertex in [edge.front(), edge.back()] {
            incident.entry(vertex.id()).or_default().push(edge.id());
        }
    }
    let mut seen = HashSet::new();
    let mut groups = Vec::new();
    for id in &ordered {
        if seen.contains(id) {
            continue;
        }
        let mut pending = vec![*id];
        let mut members = HashSet::new();
        while let Some(id) = pending.pop() {
            if !seen.insert(id) {
                continue;
            }
            members.insert(id);
            let edge = &source_edges[&id];
            for vertex in [edge.front(), edge.back()] {
                pending.extend(
                    incident[&vertex.id()]
                        .iter()
                        .filter(|id| !seen.contains(id))
                        .copied(),
                );
            }
        }
        groups.push(
            ordered
                .iter()
                .filter(|id| members.contains(id))
                .copied()
                .collect::<Vec<_>>(),
        );
    }
    if groups.len() <= 1 {
        return original_diagnostic.map_or(Ok(None), Err);
    }
    let mut candidate = history.clone();
    let mut output = solid.clone();
    for (index, group) in groups.iter().enumerate() {
        let first_face = if let Some(original) = spec.first_face {
            let reference = history.face(original)?;
            let descendants = output
                .face_iter()
                .filter(|face| {
                    candidate
                        .face(face.id())
                        .is_ok_and(|current| current == reference)
                        && group
                            .iter()
                            .all(|id| face.edge_iter().any(|edge| edge.id() == *id))
                })
                .map(|face| face.id())
                .collect::<Vec<_>>();
            let [face] = descendants.as_slice() else {
                return Err(unsupported(
                    "Distance 1 face has no unique retained descendant for an independent edge group",
                ));
            };
            Some(*face)
        } else {
            None
        };
        let current_spec = ChamferSpec {
            distance: spec.distance,
            second_distance: spec.second_distance,
            angle: spec.angle,
            flip: spec.flip,
            first_face,
        };
        let Some(next) = attempt(&output, group, &current_spec, treatment, &mut candidate)? else {
            return Err(original_diagnostic.unwrap_or_else(|| unsupported("An independent selected edge group has no complete native treatment constructor")));
        };
        let remaining = groups
            .iter()
            .skip(index + 1)
            .flatten()
            .copied()
            .collect::<HashSet<_>>();
        let surviving = next
            .edge_iter()
            .map(|edge| edge.id())
            .collect::<HashSet<_>>();
        if !remaining.is_subset(&surviving) {
            return Err(invalid(
                "An earlier treatment changes a later selected edge; the groups require a simultaneous corner/intersection construction",
            ));
        }
        output = next;
    }
    crate::validation::solid(&output)?;
    *history = candidate;
    Ok(Some(output))
}
fn attempt(
    solid: &Solid,
    selected: &[EdgeId],
    spec: &ChamferSpec,
    treatment: Treatment,
    history: &mut ShellHistory,
) -> Result<Option<Solid>> {
    let mut diagnostic = None;
    for constructor in 0..4 {
        let mut candidate = history.clone();
        let result = match constructor {
            0 => concave_treatment::construct(solid, selected, spec, treatment, &mut candidate),
            1 => local_blend::construct(solid, selected, spec, treatment, &mut candidate),
            2 => circular(solid, selected, spec, treatment, &mut candidate),
            _ => match treatment {
                Treatment::Chamfer => crate::planar::chamfer(solid, selected, spec, &mut candidate),
                Treatment::Fillet => {
                    crate::edge_round::round(solid, selected, spec.distance, &mut candidate)
                }
            },
        };
        match result {
            Ok(Some(output)) => {
                *history = candidate;
                return Ok(Some(output));
            }
            Ok(None) => {}
            Err(error @ CadError::Unsupported(_)) => {
                if diagnostic.is_none() {
                    diagnostic = Some(error);
                }
            }
            Err(error) => return Err(error),
        }
    }
    diagnostic.map_or(Ok(None), Err)
}

fn circular(
    solid: &Solid,
    selected: &[EdgeId],
    spec: &ChamferSpec,
    treatment: Treatment,
    history: &mut ShellHistory,
) -> Result<Option<Solid>> {
    if selected.is_empty() {
        return Err(invalid("Select a complete circular rim"));
    }
    if solid.face_iter().count() > MAX_FACES
        || selected.len() > cad_core::edge_features::MAX_SELECTED_EDGES
    {
        return Err(unsupported(
            "The selected body exceeds the circular chamfer complexity limit",
        ));
    }
    let selected: HashSet<_> = selected.iter().copied().collect();
    let mut source_edges = HashMap::new();
    for edge in solid.edge_iter() {
        source_edges.entry(edge.id()).or_insert_with(|| {
            if edge.orientation() {
                edge.clone()
            } else {
                edge.inverse()
            }
        });
    }
    for id in &selected {
        let Some(edge) = source_edges.get(id) else {
            return Err(invalid(
                "A selected edge no longer belongs to the source body",
            ));
        };
        match crate::bounded_carriers::circle(&edge.curve()) {
            Ok(_) => {}
            Err(CadError::Unsupported(_)) => return Ok(None),
            Err(error) => return Err(error),
        }
    }
    let mut walls = HashMap::new();
    let mut rims = Vec::new();
    for edge in solid
        .edge_iter()
        .filter(|edge| selected.contains(&edge.id()))
    {
        if rims.iter().any(|rim: &Rim| rim.edge.id() == edge.id()) {
            continue;
        }
        let adjacent: Vec<_> = solid
            .face_iter()
            .filter(|face| {
                face.edge_iter()
                    .any(|candidate| candidate.id() == edge.id())
            })
            .collect();
        if adjacent.len() != 2 {
            return Err(invalid(
                "A circular rim must adjoin exactly two oriented faces",
            ));
        }
        let planes = adjacent
            .iter()
            .map(|face| crate::planar::boundary_plane(face))
            .collect::<Result<Vec<_>>>()?;
        let Some(cap_index) = planes.iter().position(Option::is_some) else {
            return Ok(None);
        };
        if planes[1 - cap_index].is_some() {
            return Ok(None);
        }
        let cap = adjacent[cap_index];
        let wall_face = adjacent[1 - cap_index];
        let cap_plane = planes[cap_index]
            .as_ref()
            .ok_or_else(|| invalid("The selected planar cap is absent"))?;
        let wire = cap
            .boundaries()
            .into_iter()
            .find(|wire| wire.iter().any(|candidate| candidate.id() == edge.id()))
            .ok_or_else(|| invalid("The selected edge is absent from its cap boundary"))?;
        if wire
            .iter()
            .any(|candidate| !selected.contains(&candidate.id()))
            || crate::prismatic_shell::wire_circle(&wire)?.is_none()
        {
            return Err(unsupported(
                "Select every arc of the complete circular rim; partial arc ends need corner transition geometry",
            ));
        }
        let edge = source_edges[&edge.id()].clone();
        let ((center, normal, radius), _) = crate::bounded_carriers::circle(&edge.curve())?;
        if normal.dot(cap_plane.normal).abs() < 1. - 1e-10 {
            return Err(unsupported(
                "The rim is not perpendicular to its planar cap",
            ));
        }
        if let std::collections::hash_map::Entry::Vacant(slot) = walls.entry(wall_face.id()) {
            slot.insert(carrier(wall_face)?);
        }
        let wall = walls
            .get_mut(&wall_face.id())
            .ok_or_else(|| invalid("The turned wall was not classified"))?;
        let column = wall
            .rims
            .iter()
            .position(|candidate| candidate.id() == edge.id())
            .ok_or_else(|| unsupported("The selected circle is not a natural turned rim"))?;
        let axial_extent = (wall.ends[1 - column] - center).dot(cap_plane.normal);
        if axial_extent.abs() <= EPS {
            return Err(unsupported(
                "The turned wall has no axial extent at this rim",
            ));
        }
        let axial_side = axial_extent.signum();
        let curve = edge.curve();
        let (a, b) = curve.range_tuple();
        let middle = curve.subs((a + b) * 0.5);
        let radial_direction = (middle - center).normalize();
        let surface = wall_face.oriented_surface();
        let uv = surface
            .search_parameter(middle, None, 100)
            .ok_or_else(|| unsupported("The rim has no verified parameter on its turned wall"))?;
        let slope = (wall.radii[1 - column] - radius) / axial_extent;
        let slant = (1. + slope * slope).sqrt();
        let expected = (radial_direction - cap_plane.normal * slope) / slant;
        let outward = surface.normal(uv.0, uv.1).normalize().dot(expected);
        if !outward.is_finite() || outward.abs() < 1. - 1e-9 {
            return Err(unsupported(
                "The wall orientation does not prove a regular inner or outer turned surface",
            ));
        }
        let sweep = wire.iter().try_fold(0., |sum, member| -> Result<f64> {
            let ((_, normal, _), angle) =
                crate::bounded_carriers::circle(&member.oriented_curve())?;
            Ok(sum + angle * normal.dot(cap_plane.normal))
        })?;
        let side = sweep.signum();
        if (sweep.abs() - std::f64::consts::TAU).abs() > 1e-8
            || axial_side != -side * outward.signum()
        {
            return Err(invalid(
                "The complete cap winding and turned material orientation disagree",
            ));
        }
        let (radial, depth, profile_weight) = match treatment {
            Treatment::Chamfer => {
                // Distance on the conical wall follows its exact generatrix.
                // The angle method uses the actual cap/wall normal angle.
                let cosine = -outward.signum() * slope / slant;
                let first_cap = match spec.first_face {
                    Some(id) if id == cap.id() => true,
                    Some(id) if id == wall_face.id() => false,
                    Some(_) => {
                        return Err(invalid("Distance 1 face must adjoin every selected rim"));
                    }
                    None => true,
                } != spec.flip;
                let second = if let Some(angle) = spec.angle {
                    let interior = (-cosine).clamp(-1., 1.).acos();
                    if !angle.is_finite() || angle <= 0. || angle >= std::f64::consts::PI - interior
                    {
                        return Err(invalid(
                            "The chamfer angle exceeds this conical material wedge",
                        ));
                    }
                    spec.distance * angle.sin() / (interior + angle).sin()
                } else {
                    spec.second_distance.unwrap_or(spec.distance)
                };
                if !spec.distance.is_finite()
                    || spec.distance <= EPS
                    || !second.is_finite()
                    || second <= EPS
                {
                    return Err(invalid(
                        "Both setbacks must be finite and exceed modeling tolerance",
                    ));
                }
                let (radial, along) = if first_cap {
                    (spec.distance, second)
                } else {
                    (second, spec.distance)
                };
                (radial, along / slant, 1.)
            }
            Treatment::Fillet => {
                let r = spec.distance;
                let cosine = -outward.signum() * slope / slant;
                if !r.is_finite() || r <= EPS || 1. + cosine <= 1e-12 {
                    return Err(invalid(
                        "A finite fillet radius requires a nondegenerate material wedge",
                    ));
                }
                (
                    r / (slant - outward.signum() * slope),
                    r * (1. + outward.signum() * slope / slant),
                    ((1. + cosine) * 0.5).sqrt(),
                )
            }
        };
        let wall_radius = radius + slope * axial_side * depth;
        if !wall_radius.is_finite()
            || wall_radius <= EPS
            || !radial.is_finite()
            || !depth.is_finite()
            || depth <= EPS
        {
            return Err(invalid(
                "The setback reaches a cone apex or exceeds finite geometry",
            ));
        }
        if radius - side * radial <= EPS {
            return Err(invalid("The radial setback consumes the circular cap"));
        }
        wall.offsets[column] = cap_plane.normal * (axial_side * depth);
        wall.scales[column] = wall_radius / radius;
        rims.push(Rim {
            cap: cap.clone(),
            edge,
            center,
            axis: cap_plane.normal,
            radius,
            wall_radius,
            profile_weight,
            radial,
            depth,
            side,
            axial_side,
            wall: wall_face.id(),
            column,
        });
    }
    let mut keyed = rims
        .into_iter()
        .map(|rim| {
            let key = crate::shell_history::closure(&[history.edge(rim.edge.id())?])?.to_string();
            Ok((key, rim))
        })
        .collect::<Result<Vec<_>>>()?;
    keyed.sort_by(|a, b| a.0.cmp(&b.0));
    let rims = keyed.into_iter().map(|(_, rim)| rim).collect::<Vec<_>>();
    for (index, rim) in rims.iter().enumerate() {
        for other in rims
            .iter()
            .skip(index + 1)
            .filter(|other| same_ring(rim, other))
        {
            if (rim.radial - other.radial).abs() > EPS
                || (rim.depth - other.depth).abs() > EPS
                || (rim.wall_radius - other.wall_radius).abs() > EPS
                || (rim.profile_weight - other.profile_weight).abs() > 1e-10
                || rim.side != other.side
                || rim.axial_side != other.axial_side
            {
                return Err(invalid(
                    "A complete circular rim has incompatible adjacent wall slopes or treatment parameters",
                ));
            }
        }
    }
    for wall in walls.values() {
        let original = wall.ends[1] - wall.ends[0];
        let remaining = wall.ends[1] + wall.offsets[1] - wall.ends[0] - wall.offsets[0];
        if remaining.dot(original.normalize()) <= EPS {
            return Err(invalid("Opposite chamfers meet or consume the turned wall"));
        }
    }
    collision_proof(solid, &rims)?;
    let mut plane_vertices = HashMap::new();
    let mut wall_vertices = HashMap::new();
    for rim in &rims {
        for vertex in [rim.edge.absolute_front(), rim.edge.absolute_back()] {
            assign_vertex(&mut plane_vertices, vertex, rim.plane_point(vertex.point()))?;
            assign_vertex(&mut wall_vertices, vertex, rim.wall_point(vertex.point()))?;
        }
    }
    let mut plane_edges = HashMap::new();
    let mut wall_edges = HashMap::new();
    let mut seam_edges = HashMap::new();
    for rim in &rims {
        let source = history.edge(rim.edge.id())?;
        let raw = rim
            .edge
            .curve()
            .try_into_homogeneous_bspline_curve()
            .ok_or_else(|| unsupported("The rim lost its exact rational carrier"))?;
        let mut planar = raw.clone();
        let scale = 1. - rim.side * rim.radial / rim.radius;
        planar.transform_control_points(|h| {
            let p = rim.center.to_vec() * h.w;
            let v = p + (h.truncate() - p) * scale;
            *h = v.extend(h.w);
        });
        let mut turned = raw;
        turned.transform_control_points(|h| {
            let center = rim.center.to_vec() * h.w;
            *h = (center
                + (h.truncate() - center) * (rim.wall_radius / rim.radius)
                + rim.axis * (rim.axial_side * rim.depth * h.w))
                .extend(h.w);
        });
        let p = replace_curve(
            &rim.edge,
            &plane_vertices,
            Curve::NurbsCurve(NurbsCurve::new(planar)),
        )?;
        let w = replace_curve(
            &rim.edge,
            &wall_vertices,
            Curve::NurbsCurve(NurbsCurve::new(turned)),
        )?;
        history.construct_edge(
            p.id(),
            std::slice::from_ref(&source),
            serde_json::json!({"operation":treatment.name(),"constructor":"circular_planar_rail"}),
        )?;
        history.construct_edge(
            w.id(),
            std::slice::from_ref(&source),
            serde_json::json!({"operation":treatment.name(),"constructor":"circular_wall_rail"}),
        )?;
        plane_edges.insert(rim.edge.id(), p);
        wall_edges.insert(rim.edge.id(), w);
    }
    for wall in walls.values() {
        for edge in wall.face.edge_iter() {
            if wall.rims.iter().any(|rim| rim.id() == edge.id())
                || seam_edges.contains_key(&edge.id())
            {
                continue;
            }
            let source = source_edges[&edge.id()].clone();
            let a = source.absolute_front();
            let b = source.absolute_back();
            let start = wall_vertices.get(&a.id()).unwrap_or(a);
            let end = wall_vertices.get(&b.id()).unwrap_or(b);
            if start.id() == a.id() && end.id() == b.id() {
                continue;
            }
            let replacement = builder::line(start, end);
            history.retain_edge(replacement.id(), source.id())?;
            seam_edges.insert(edge.id(), replacement);
        }
    }
    let mut output_faces = HashMap::new();
    for rim in &rims {
        if output_faces.contains_key(&rim.cap.id()) {
            continue;
        }
        let wires = rim
            .cap
            .boundaries()
            .iter()
            .map(|wire| remap_wire(wire, &plane_edges))
            .collect();
        let face = Face::try_new(wires, rim.cap.oriented_surface())
            .map_err(|_| invalid("The planar rim setback breaks its cap boundary"))?;
        history.retain_face(face.id(), rim.cap.id())?;
        output_faces.insert(rim.cap.id(), face);
    }
    for wall in walls.values_mut() {
        for row in wall.net.control_points_mut() {
            point(*row)?;
        }
        for column in 0..2 {
            let offset = wall.offsets[column];
            for row in 0..wall.net.control_points().len() {
                let h = wall.net.control_point_mut(row, column);
                let center = wall.ends[column].to_vec() * h.w;
                *h = (center + (h.truncate() - center) * wall.scales[column] + offset * h.w)
                    .extend(h.w);
            }
        }
        if wall.transposed {
            wall.net.invert();
        }
        let mut map = wall_edges.clone();
        map.extend(seam_edges.iter().map(|(id, edge)| (*id, edge.clone())));
        let wires = wall
            .face
            .boundaries()
            .iter()
            .map(|wire| remap_wire(wire, &map))
            .collect();
        let face = Face::try_new(
            wires,
            Surface::NurbsSurface(NurbsSurface::new(wall.net.clone())),
        )
        .map_err(|_| invalid("The axial setback breaks its turned wall boundary"))?;
        history.retain_face(face.id(), wall.face.id())?;
        output_faces.insert(wall.face.id(), face);
    }
    let mut connectors: HashMap<VertexId, Edge> = HashMap::new();
    let mut bevels: HashMap<FaceId, Vec<Face>> = HashMap::new();
    for rim in &rims {
        let old_oriented = rim
            .cap
            .edge_iter()
            .find(|edge| edge.id() == rim.edge.id())
            .ok_or_else(|| invalid("The circular edge is absent from its source cap"))?;
        let plane = oriented(&old_oriented, &plane_edges[&rim.edge.id()]);
        let wall = oriented(&old_oriented, &wall_edges[&rim.edge.id()]);
        for old in [old_oriented.front(), old_oriented.back()] {
            if let std::collections::hash_map::Entry::Vacant(slot) = connectors.entry(old.id()) {
                let front = &plane_vertices[&old.id()];
                let back = &wall_vertices[&old.id()];
                let connector = match treatment {
                    Treatment::Chamfer => builder::line(front, back),
                    Treatment::Fillet => {
                        let weight = rim.profile_weight;
                        let curve = NurbsCurve::new(BsplineCurve::new(
                            KnotVector::bezier_knot(2),
                            vec![
                                front.point().to_vec().extend(1.),
                                old.point().to_vec().extend(1.) * weight,
                                back.point().to_vec().extend(1.),
                            ],
                        ));
                        Edge::try_new(front, back, Curve::NurbsCurve(curve))
                            .map_err(|_| invalid("A circular meridian collapsed"))?
                    }
                };
                let mut incident: Vec<_> = rims
                    .iter()
                    .filter(|other| {
                        other.edge.front().id() == old.id() || other.edge.back().id() == old.id()
                    })
                    .map(|other| history.edge(other.edge.id()))
                    .collect::<Result<_>>()?;
                // Two semicircles have the same pair of rim-member sources
                // at both endpoints. The original axial seam distinguishes
                // those endpoints without using coordinates or transient IDs.
                let seams = source_edges
                    .values()
                    .filter(|edge| {
                        !selected.contains(&edge.id())
                            && (edge.front().id() == old.id() || edge.back().id() == old.id())
                    })
                    .map(|edge| history.edge(edge.id()))
                    .collect::<Result<Vec<_>>>()?;
                if seams.is_empty() {
                    return Err(unsupported(
                        "A circular transition endpoint has no distinct source seam ancestry",
                    ));
                }
                incident.extend(seams);
                history.construct_edge(connector.id(), &incident, serde_json::json!({"operation":treatment.name(),"constructor":"circular_shared_seam"}))?;
                slot.insert(connector);
            }
        }
        let p = plane
            .oriented_curve()
            .try_into_homogeneous_bspline_curve()
            .ok_or_else(|| unsupported("The planar rail has no exact rational carrier"))?;
        let w = wall
            .oriented_curve()
            .try_into_homogeneous_bspline_curve()
            .ok_or_else(|| unsupported("The turned rail has no exact rational carrier"))?;
        let mut surface = match treatment {
            Treatment::Chamfer => NurbsSurface::new(BsplineSurface::homotopy(p, w)),
            Treatment::Fillet => {
                let original = old_oriented
                    .oriented_curve()
                    .try_into_homogeneous_bspline_curve()
                    .ok_or_else(|| unsupported("The source rail has no exact rational carrier"))?;
                let weight = rim.profile_weight;
                let controls = p
                    .control_points()
                    .iter()
                    .zip(original.control_points())
                    .zip(w.control_points())
                    .map(|((a, corner), b)| vec![*a, *corner * weight, *b])
                    .collect();
                NurbsSurface::new(BsplineSurface::new(
                    (p.knot_vector().clone(), KnotVector::bezier_knot(2)),
                    controls,
                ))
            }
        };
        surface.invert();
        let wire = Wire::from(vec![
            plane.inverse(),
            connectors[&old_oriented.front().id()].clone(),
            wall,
            connectors[&old_oriented.back().id()].inverse(),
        ]);
        let face = Face::try_new(vec![wire], Surface::NurbsSurface(surface))
            .map_err(|_| invalid("The conical strip has inconsistent shared boundaries"))?;
        history.construct_face(face.id(), &[history.edge(rim.edge.id())?, history.face(rim.cap.id())?, history.face(rim.wall)?], serde_json::json!({"operation":treatment.name(),"constructor":match treatment { Treatment::Chamfer => "exact_conical_rim", Treatment::Fillet => "exact_toroidal_rim" },"column":rim.column}))?;
        bevels.entry(rim.cap.id()).or_default().push(face);
    }
    let mut shells = Vec::new();
    for shell in solid.boundaries() {
        let mut out = Shell::new();
        for face in shell {
            out.push(
                output_faces
                    .get(&face.id())
                    .cloned()
                    .unwrap_or_else(|| face.clone()),
            );
            if let Some(strips) = bevels.remove(&face.id()) {
                out.extend(strips);
            }
        }
        shells.push(out);
    }
    let result = Solid::try_new(shells)
        .map_err(|_| invalid("Circular setback boundaries do not close the body"))?;
    crate::validation::solid(&result)?;
    Ok(Some(result))
}
