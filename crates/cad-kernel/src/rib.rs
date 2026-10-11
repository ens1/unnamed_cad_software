//! Native open-contour ribs with proven bounded supporting-body attachment.
use crate::{
    adapter::{MonstertruckAdapter, Shape, TopologyHistory},
    extrusion::{ThinMember, ThinSection},
};
use cad_core::{
    CadError, Dimension, Document, Id, NodeRef, Plane, Result, ThinExtrude, ThinExtrudeMode,
    rib::{Definition, Direction, Placement, Termination},
};
use monstertruck::{geometry::prelude::TryIntoHomogeneousBsplineSurface, modeling::*};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

const EPS: f64 = 1e-7;
fn invalid(message: impl ToString) -> CadError {
    CadError::Invalid(format!("Rib: {}", message.to_string()))
}
fn unsupported(message: impl ToString) -> CadError {
    CadError::Unsupported(format!("Rib: {}", message.to_string()))
}
fn positive(value: f64, label: &str) -> Result<f64> {
    if !value.is_finite() || value <= EPS {
        return Err(invalid(format!(
            "{label} must be finite and greater than geometric precision"
        )));
    }
    Ok(value)
}
fn face_bounds(face: &Face) -> Result<Vec<Point3>> {
    match crate::extrusion::face_hull(face) {
        Ok(points) => return Ok(points),
        Err(CadError::Unsupported(_)) => {}
        Err(error) => return Err(error),
    }
    let mut points = face
        .edge_iter()
        .map(|edge| crate::extrusion::curve_hull(&edge.curve()))
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    let surface = face
        .surface()
        .try_into_homogeneous_bspline_surface()
        .ok_or_else(|| {
            unsupported("The attachment neighborhood has no exact finite rational surface bounds")
        })?;
    for h in surface.control_points().iter().flatten() {
        if ![h.x, h.y, h.z, h.w].iter().all(|v| v.is_finite()) || h.w <= 0. {
            return Err(unsupported(
                "Attachment bounds require finite positive rational weights",
            ));
        }
        points.push(Point3::new(h.x / h.w, h.y / h.w, h.z / h.w));
    }
    Ok(points)
}
fn separated_hulls(a: &[Point3], b: &[Point3]) -> bool {
    if crate::extrusion_surface::bounds_disjoint(a, b, EPS) {
        return true;
    }
    if a.is_empty() || b.is_empty() {
        return false;
    }
    let center = |points: &[Point3]| {
        points
            .iter()
            .fold(Vector3::zero(), |sum, p| sum + p.to_vec())
            / points.len() as f64
    };
    let direction = center(a) - center(b);
    if !direction.magnitude2().is_finite() || direction.magnitude2() <= 1e-18 {
        return false;
    }
    let axis = direction.normalize();
    let interval = |points: &[Point3]| {
        points
            .iter()
            .map(|p| p.to_vec().dot(axis))
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), v| {
                (lo.min(v), hi.max(v))
            })
    };
    let (amin, amax) = interval(a);
    let (bmin, bmax) = interval(b);
    amax < bmin - EPS || bmax < amin - EPS
}
#[derive(Clone)]
struct Support {
    body: NodeRef,
    stock: Solid,
    face: Face,
    origin: Point3,
    normal: Vector3,
}
impl Support {
    fn distance(&self, p: Point3, axis: Vector3) -> f64 {
        (self.origin - p).dot(self.normal) / axis.dot(self.normal)
    }
    fn project(&self, p: Point3, axis: Vector3) -> Point3 {
        p + axis * self.distance(p, axis)
    }
    fn plane(&self) -> Plane {
        let u = self
            .normal
            .cross(if self.normal.x.abs() < 0.8 {
                Vector3::unit_x()
            } else {
                Vector3::unit_y()
            })
            .normalize();
        Plane {
            origin: self.origin.into(),
            u: u.into(),
            v: self.normal.cross(u).into(),
        }
    }
}
fn support(body: &NodeRef, stock: &Solid, face: &Face) -> Result<Option<Support>> {
    let Some(origin) = face.vertex_iter().next().map(|vertex| vertex.point()) else {
        return Ok(None);
    };
    let normal = face.oriented_surface().normal(0., 0.);
    if !normal.magnitude2().is_finite() || normal.magnitude2() < 1e-18 {
        return Ok(None);
    }
    let normal = normal.normalize();
    let hull = match face_bounds(face) {
        Ok(hull) => hull,
        Err(CadError::Unsupported(_)) => return Ok(None),
        Err(error) => return Err(error),
    };
    if !hull
        .iter()
        .all(|point| (*point - origin).dot(normal).abs() <= EPS)
    {
        return Ok(None);
    }
    Ok(Some(Support {
        body: body.clone(),
        stock: stock.clone(),
        face: face.clone(),
        origin,
        normal,
    }))
}
fn supports(
    adapter: &MonstertruckAdapter,
    definition: &Definition,
    inputs: &BTreeMap<NodeRef, Shape>,
) -> Result<Vec<Support>> {
    let scope = definition.consumed_outputs();
    let mut seen = HashSet::new();
    let mut candidates = Vec::new();
    match &definition.termination {
        Termination::UpToFace { face, body } => {
            if !scope.contains(body) {
                return Err(invalid(
                    "The termination face must belong to an output in Feature Scope",
                ));
            }
            let selected = adapter.selected_boundary_face(body, face, inputs)?;
            let Some(Shape::Bodies(stocks)) = inputs.get(body) else {
                return Err(invalid("The termination must belong to solid stock"));
            };
            let stocks = stocks
                .iter()
                .filter(|stock| stock.face_iter().any(|face| face.id() == selected.id()))
                .collect::<Vec<_>>();
            if stocks.len() != 1 {
                return Err(invalid(
                    "The termination face has missing or ambiguous current solid ownership",
                ));
            }
            candidates.push(support(body, stocks[0], &selected)?.ok_or_else(|| {
                unsupported("Termination currently requires an exact planar support face")
            })?);
        }
        Termination::UpToBody => {
            for body in scope {
                let Some(Shape::Bodies(stocks)) = inputs.get(&body) else {
                    return Err(invalid(
                        "Every Feature Scope entry must produce current solid bodies",
                    ));
                };
                for stock in stocks {
                    for face in stock.face_iter() {
                        if seen.insert(face.id())
                            && let Some(support) = support(&body, stock, face)?
                        {
                            candidates.push(support);
                        }
                    }
                }
            }
        }
    }
    if candidates.is_empty() {
        return Err(unsupported(
            "No current planar supporting face is available",
        ));
    }
    Ok(candidates)
}

/// The collar enters through a proven covered bounded face. Every other
/// boundary is either a supporting half-space or has a disjoint exact positive
/// control hull; therefore no cavity or exterior can cut through the overlap.
fn contained_collar(support: &Support, collar: &Solid) -> Result<bool> {
    let hull = crate::adapter::solid_control_points(collar)?;
    let stock = crate::adapter::solid_control_points(&support.stock)?;
    for face in support.stock.face_iter() {
        if face.id() == support.face.id() {
            continue;
        }
        let bounds = match face_bounds(face) {
            Ok(bounds) => bounds,
            Err(CadError::Unsupported(_)) => return Ok(false),
            Err(error) => return Err(error),
        };
        let Some(origin) = face.vertex_iter().next().map(|vertex| vertex.point()) else {
            return Ok(false);
        };
        let normal = face.oriented_surface().normal(0., 0.);
        if normal.magnitude2().is_finite() && normal.magnitude2() > 1e-18 {
            let normal = normal.normalize();
            if bounds
                .iter()
                .all(|p| (*p - origin).dot(normal).abs() <= EPS)
                && stock.iter().all(|p| (*p - origin).dot(normal) <= EPS)
                && hull.iter().all(|p| (*p - origin).dot(normal) <= EPS)
            {
                continue;
            }
        }
        if !separated_hulls(&bounds, &hull) {
            return Ok(false);
        }
    }
    Ok(true)
}
type MemberTrace = HashMap<EdgeId, (Id, usize)>;
type NativePaths = (Plane, Vec<Wire>, MemberTrace);
fn native_paths(
    adapter: &MonstertruckAdapter,
    doc: &Document,
    definition: &Definition,
    inputs: &BTreeMap<NodeRef, Shape>,
) -> Result<NativePaths> {
    let (source, plane) = crate::extrusion::source(adapter, doc, &definition.profile, inputs)?;
    for member in &definition.members {
        if !source.members.get(member).is_some_and(|m| !m.construction) {
            return Err(invalid(
                "A selected Rib contour is missing, suppressed, or construction geometry",
            ));
        }
    }
    let built =
        crate::sketches::build_traced(doc, &definition.profile.definition, &source, &plane)?;
    let Shape::Sketch { paths, .. } = built.shape else {
        return Err(invalid("Rib requires a native sketch"));
    };
    let mut selected = Vec::new();
    for path in paths {
        if path
            .iter()
            .any(|edge| !built.members.contains_key(&edge.id()))
        {
            return Err(invalid(
                "Rib centerlines require native sketch member identities",
            ));
        }
        if definition.members.is_empty()
            || path
                .iter()
                .any(|edge| definition.members.contains(&built.members[&edge.id()].0))
        {
            selected.push(path);
        }
    }
    for member in &definition.members {
        if !selected.iter().any(|path| {
            path.iter()
                .any(|edge| built.members[&edge.id()].0 == *member)
        }) {
            return Err(invalid(
                "A selected Rib member no longer belongs to an open contour",
            ));
        }
    }
    if selected.is_empty() {
        return Err(invalid(
            "Draw an open Rib contour or select its current native sketch members",
        ));
    }
    Ok((plane, selected, built.members))
}
fn spans(curve: &Curve) -> Result<Vec<BsplineCurve<Vector4>>> {
    crate::extrusion::curve_hull(curve)?;
    let mut curve = curve.lift_up();
    curve.clamp();
    if curve.degree() > 32 || curve.control_points().len() > 1024 {
        return Err(unsupported(
            "The open contour exceeds the exact carrier budget",
        ));
    }
    let mut knots = curve.knot_vector().iter().copied().collect::<Vec<_>>();
    knots.dedup();
    let mut result = Vec::new();
    for knot in knots.into_iter().skip(1).rev().skip(1).rev() {
        let tail = curve.cut(knot);
        result.push(curve);
        curve = tail;
    }
    result.push(curve);
    Ok(result)
}
fn binomial(n: usize, k: usize) -> f64 {
    (0..k.min(n - k)).fold(1., |value, index| {
        value * (n - index) as f64 / (index + 1) as f64
    })
}
fn circular_coverage(target: &Face, cap: &Face) -> Result<bool> {
    let wires = target.boundaries();
    let Some(outer) = wires.first() else {
        return Ok(false);
    };
    let Some(first) = outer.front() else {
        return Ok(false);
    };
    let Ok((center, _, radius)) = crate::axis_geometry::circle_geometry(&first.curve()) else {
        return Ok(false);
    };
    let normal = target.oriented_surface().normal(0., 0.).normalize();
    let mut angle = 0.;
    for edge in outer {
        let Ok((c, n, r)) = crate::axis_geometry::circle_geometry(&edge.curve()) else {
            return Ok(false);
        };
        if c.distance(center) > EPS
            || (r - radius).abs() > EPS
            || n.cross(normal).magnitude() > 1e-8
        {
            return Ok(false);
        }
        for curve in spans(&edge.oriented_curve())? {
            let points = curve.control_points();
            let degree = curve.degree();
            if degree == 0 || points.len() != degree + 1 {
                return Ok(false);
            }
            let radial = points
                .iter()
                .map(|h| {
                    Vector3::new(
                        h.x - center.x * h.w,
                        h.y - center.y * h.w,
                        h.z - center.z * h.w,
                    ) / points[0].w
                })
                .collect::<Vec<_>>();
            let a = radial[0];
            let b = *radial.last().unwrap();
            let middle = a.normalize() + b.normalize();
            if middle.magnitude2() <= 1e-12 || radial.iter().any(|r| r.dot(middle) <= EPS) {
                return Ok(false);
            }
            // X cross X' / W² is the angular derivative. Its complete
            // Bernstein coefficients prove one forward circular arc per span.
            let mut coefficients = vec![0.; degree * 2];
            for (i, a) in radial.iter().enumerate() {
                for j in 0..degree {
                    let derivative = (radial[j + 1] - radial[j]) * degree as f64;
                    coefficients[i + j] += a.cross(derivative).dot(normal)
                        * binomial(degree, i)
                        * binomial(degree - 1, j)
                        / binomial(2 * degree - 1, i + j);
                }
            }
            let precision = 1e-10 * radius * radius;
            if coefficients.iter().any(|value| *value < -precision)
                || coefficients.iter().sum::<f64>() <= precision
            {
                return Ok(false);
            }
            let sweep = normal.dot(a.cross(b)).atan2(a.dot(b));
            if !sweep.is_finite() || sweep <= 0. || sweep >= std::f64::consts::PI {
                return Ok(false);
            }
            angle += sweep;
        }
    }
    if (angle - std::f64::consts::TAU).abs() > 1e-6 {
        return Ok(false);
    }
    let hull = crate::extrusion::face_hull(cap)?;
    if hull
        .iter()
        .any(|p| (*p - center).magnitude() > radius + EPS)
    {
        return Ok(false);
    }
    for hole in wires.iter().skip(1) {
        let hole = hole
            .iter()
            .map(|edge| crate::extrusion::curve_hull(&edge.curve()))
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();
        if !separated_hulls(&hole, &hull) {
            return Ok(false);
        }
    }
    Ok(true)
}
fn covered(target: &Face, cap: &Face) -> Result<bool> {
    match crate::extrusion_surface::covered(target, cap) {
        Ok(true) => Ok(true),
        Ok(false) | Err(CadError::Unsupported(_)) => circular_coverage(target, cap),
        Err(error) => Err(error),
    }
}
fn progress(path: &mut Wire, axis: Vector3) -> Result<()> {
    let first = path
        .front()
        .ok_or_else(|| invalid("Empty centerline"))?
        .front()
        .point();
    let last = path.back().unwrap().back().point();
    if (last - first).dot(axis) < 0. {
        path.invert();
    }
    for edge in path.iter() {
        for curve in spans(&edge.oriented_curve())? {
            let coordinates = curve
                .control_points()
                .iter()
                .map(|h| {
                    if ![h.x, h.y, h.z, h.w].iter().all(|v| v.is_finite()) || h.w <= 0. {
                        return Err(unsupported(
                            "Open contour progress needs positive finite rational weights",
                        ));
                    }
                    Ok(Point3::new(h.x / h.w, h.y / h.w, h.z / h.w)
                        .to_vec()
                        .dot(axis))
                })
                .collect::<Result<Vec<_>>>()?;
            if coordinates.windows(2).any(|pair| pair[1] < pair[0] - 1e-10)
                || coordinates.last().unwrap() - coordinates[0] <= EPS
            {
                return Err(unsupported(
                    "The open contour must progress in one direction across the material-fill axis; folds, vertical returns, and uncertain spline hulls need a broader Rib intersection solver",
                ));
            }
        }
    }
    Ok(())
}
fn closure(
    path: &Wire,
    trace: &HashMap<EdgeId, (Id, usize)>,
    support: &Support,
    axis: Vector3,
    overlap: f64,
) -> Result<ThinSection> {
    let mut wire = path.clone();
    let start = path.front().unwrap().front();
    let end = path.back().unwrap().back();
    let a = builder::vertex(support.project(start.point(), axis) + axis * overlap);
    let b = builder::vertex(support.project(end.point(), axis) + axis * overlap);
    let sources = path
        .iter()
        .map(|edge| trace[&edge.id()].0.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let mut members = path
        .iter()
        .map(|edge| {
            let (member, piece) = &trace[&edge.id()];
            (
                edge.id(),
                ThinMember {
                    sources: vec![member.clone()],
                    side: 0,
                    piece: *piece,
                    cap: false,
                },
            )
        })
        .collect::<HashMap<_, _>>();
    for (edge, source, side) in [
        (
            builder::line(end, &b),
            vec![trace[&path.back().unwrap().id()].0.clone()],
            2,
        ),
        (builder::line(&b, &a), sources, 1),
        (
            builder::line(&a, start),
            vec![trace[&path.front().unwrap().id()].0.clone()],
            3,
        ),
    ] {
        members.insert(
            edge.id(),
            ThinMember {
                sources: source,
                side,
                piece: 0,
                cap: side != 1,
            },
        );
        wire.push_back(edge);
    }
    let face = builder::try_attach_plane(vec![wire]).map_err(invalid)?;
    Ok(ThinSection { face, members })
}
fn footprint(
    path: &Wire,
    support: &Support,
    axis: Vector3,
    normal: Vector3,
    low: f64,
    high: f64,
) -> Result<Face> {
    let a = support.project(path.front().unwrap().front().point(), axis);
    let b = support.project(path.back().unwrap().back().point(), axis);
    let points = [
        a + normal * low,
        b + normal * low,
        b + normal * high,
        a + normal * high,
    ];
    let vertices = points.map(builder::vertex);
    let wire = Wire::from(
        (0..4)
            .map(|i| builder::line(&vertices[i], &vertices[(i + 1) % 4]))
            .collect::<Vec<_>>(),
    );
    let mut face: Face = builder::try_attach_plane(vec![wire]).map_err(invalid)?;
    if face.oriented_surface().normal(0., 0.).dot(support.normal) < 0. {
        face.invert();
    }
    Ok(face)
}
fn envelope(hull: &[Point3], support: &Support, axis: Vector3) -> Option<(f64, f64)> {
    if axis.dot(support.normal) >= -1e-9 {
        return None;
    }
    let distances = hull
        .iter()
        .map(|p| support.distance(*p, axis))
        .collect::<Vec<_>>();
    if distances.iter().any(|d| !d.is_finite() || *d < -EPS) {
        return None;
    }
    let min = distances.iter().copied().fold(f64::INFINITY, f64::min);
    let max = distances.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    (max > EPS).then_some((min, max))
}
struct Tool {
    solid: Solid,
    section: ThinSection,
    axis: Vector3,
    support: Support,
    range: (f64, f64),
}
fn nearest(mut candidates: Vec<Tool>, explicit: bool) -> Result<Tool> {
    if candidates.is_empty() {
        return Err(unsupported(
            "No bounded forward face contains the full Rib attachment and its overlap; select a compatible planar face, reverse the material direction, or reduce thickness/overlap",
        ));
    }
    candidates.sort_by(|a, b| a.range.1.total_cmp(&b.range.1));
    if !explicit
        && candidates
            .iter()
            .skip(1)
            .any(|other| candidates[0].range.1 >= other.range.0 - EPS)
    {
        return Err(unsupported(
            "Forward support envelopes overlap; select an explicit termination face",
        ));
    }
    Ok(candidates.remove(0))
}
#[allow(clippy::too_many_arguments)]
fn parallel_tools(
    paths: Vec<Wire>,
    trace: &HashMap<EdgeId, (Id, usize)>,
    normal: Vector3,
    axis: Vector3,
    supports: &[Support],
    thickness: f64,
    placement: Placement,
    overlap: f64,
    explicit: bool,
) -> Result<Vec<Tool>> {
    let (low, high) = match placement {
        Placement::FirstSide => (0., thickness),
        Placement::Centered => (-thickness / 2., thickness / 2.),
        Placement::SecondSide => (-thickness, 0.),
    };
    let across = normal.cross(axis).normalize();
    let mut tools = Vec::new();
    for mut path in paths {
        progress(&mut path, across)?;
        let hull = path
            .iter()
            .map(|edge| crate::extrusion::curve_hull(&edge.curve()))
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();
        let mut candidates = Vec::new();
        for support in supports {
            if support.normal.dot(normal).abs() > 1e-9 {
                continue;
            }
            let Some(range) = envelope(&hull, support, axis) else {
                continue;
            };
            let foot = footprint(&path, support, axis, normal, low, high)?;
            if !covered(&support.face, &foot)? {
                continue;
            }
            let collar = crate::adapter::oriented_extrude(&foot, axis * overlap);
            crate::validation::solid(&collar)?;
            if !contained_collar(support, &collar)? {
                continue;
            }
            let original = closure(&path, trace, support, axis, overlap)?;
            let face = builder::translated(&original.face, normal * low);
            // Translation changes EdgeIds; preserve constructor correspondence.
            let members = original.face.boundaries()[0]
                .iter()
                .zip(&face.boundaries()[0])
                .map(|(a, b)| (b.id(), original.members[&a.id()].clone()))
                .collect();
            let section = ThinSection { face, members };
            let solid = crate::adapter::oriented_extrude(&section.face, normal * thickness);
            crate::validation::solid(&solid)?;
            candidates.push(Tool {
                solid,
                section,
                axis: normal,
                support: support.clone(),
                range,
            });
        }
        tools.push(nearest(candidates, explicit)?);
    }
    Ok(tools)
}
#[allow(clippy::too_many_arguments)]
fn normal_tools(
    adapter: &MonstertruckAdapter,
    doc: &Document,
    owner: &NodeRef,
    definition: &Definition,
    paths: &[Wire],
    trace: &HashMap<EdgeId, (Id, usize)>,
    inputs: &BTreeMap<NodeRef, Shape>,
    axis: Vector3,
    supports: &[Support],
    overlap: f64,
) -> Result<Vec<Tool>> {
    let contours = paths
        .iter()
        .map(|path| cad_core::contours::Selection {
            profile: definition.profile.clone(),
            outer_members: vec![trace[&path.front().unwrap().id()].0.clone()],
        })
        .collect::<Vec<_>>();
    let thin = ThinExtrude {
        mode: if definition.placement == Placement::Centered {
            ThinExtrudeMode::MidPlane
        } else {
            ThinExtrudeMode::OneSide
        },
        thickness: definition.thickness.clone(),
        second_thickness: None,
        reverse: definition.placement == Placement::SecondSide,
    };
    let sections = crate::extrusion::thin_profiles_traced(
        adapter,
        doc,
        &definition.profile,
        &thin,
        inputs,
        &owner.definition,
        &contours,
    )?;
    let mut tools = Vec::new();
    for section in sections {
        let hull = crate::extrusion::face_hull(&section.face)?;
        let mut candidates = Vec::new();
        for support in supports {
            let Some(range) = envelope(&hull, support, axis) else {
                continue;
            };
            if range.0 <= EPS {
                continue;
            }
            let foot = crate::extrusion::project_face(&section.face, axis, &support.plane(), 0.)?;
            if !covered(&support.face, &foot)? {
                continue;
            }
            let collar = crate::adapter::oriented_extrude(&foot, axis * overlap);
            crate::validation::solid(&collar)?;
            if !contained_collar(support, &collar)? {
                continue;
            }
            let end = builder::translated(&foot, axis * overlap);
            let solid = crate::extrusion::between_caps(&section.face, &end, axis)?;
            crate::validation::solid(&solid)?;
            candidates.push(Tool {
                solid,
                section: section.clone(),
                axis,
                support: support.clone(),
                range,
            });
        }
        tools.push(nearest(
            candidates,
            !matches!(definition.termination, Termination::UpToBody),
        )?);
    }
    Ok(tools)
}
pub(crate) fn operation(
    adapter: &MonstertruckAdapter,
    doc: &Document,
    owner: &NodeRef,
    definition: &Definition,
    inputs: &BTreeMap<NodeRef, Shape>,
    tolerance: f64,
    generated: &mut TopologyHistory,
) -> Result<Shape> {
    let length =
        |expr| cad_core::expression::value(doc, &owner.definition, expr, Dimension::Length);
    let thickness = positive(length(&definition.thickness)?, "Thickness")?;
    let overlap = positive(
        length(&definition.attachment_overlap)?,
        "Attachment overlap",
    )?;
    let (plane, paths, trace) = native_paths(adapter, doc, definition, inputs)?;
    let normal = Vector3::from(plane.u)
        .cross(Vector3::from(plane.v))
        .normalize();
    let sign = if definition.reverse { -1. } else { 1. };
    let supports = supports(adapter, definition, inputs)?;
    let tools = match definition.direction {
        Direction::ParallelToSketch => {
            let components = definition
                .in_plane_direction
                .iter()
                .map(|expr| {
                    cad_core::expression::value(doc, &owner.definition, expr, Dimension::Scalar)
                })
                .collect::<Result<Vec<_>>>()?;
            let direction =
                Vector3::from(plane.u) * components[0] + Vector3::from(plane.v) * components[1];
            if !direction.magnitude2().is_finite() || direction.magnitude2() < 1e-18 {
                return Err(invalid("Material direction must be finite and nonzero"));
            }
            parallel_tools(
                paths,
                &trace,
                normal,
                direction.normalize() * sign,
                &supports,
                thickness,
                definition.placement,
                overlap,
                !matches!(definition.termination, Termination::UpToBody),
            )?
        }
        Direction::NormalToSketch => normal_tools(
            adapter,
            doc,
            owner,
            definition,
            &paths,
            &trace,
            inputs,
            normal * sign,
            &supports,
            overlap,
        )?,
    };
    let mut solids = Vec::new();
    for tool in tools {
        let support_source = adapter
            .face_source(&tool.support.body, tool.support.face.id())
            .ok_or_else(|| {
                unsupported("The current supporting face lacks persistent construction ancestry")
            })?;
        crate::section_history::extrusion(
            owner,
            &definition.profile,
            &tool.section,
            &tool.solid,
            crate::section_history::ToolContext {
                axis: tool.axis,
                drafted: false,
            },
            &serde_json::json!({"operation":"rib","support":crate::adapter::construction_reference(&support_source)?,"direction":definition.direction}),
            generated,
        )?;
        solids.push(tool.solid);
    }
    crate::revolution::combine(
        solids,
        &definition.consumed_outputs(),
        "join",
        inputs,
        tolerance,
        crate::revolution::Combination {
            adapter,
            owner,
            generated,
        },
    )
}
