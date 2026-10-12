//! Direct exact drill insertion into a proven convex planar stock boundary.
//!
//! Positive rational control hulls prove stock containment. Disjoint circular
//! envelopes prove that independently shared drill wires cannot intersect.
//! Unsupported geometry returns None for the ordinary Boolean operation;
//! malformed definitions and inconsistent constructed shells remain errors.
use cad_core::{CadError, Id, Result};
use monstertruck::{modeling::*, topology::shell::ShellCondition};
use std::collections::BTreeSet;

const EPS: f64 = 1e-7;
const ANGULAR: f64 = 1e-10;

#[derive(Clone)]
pub(crate) struct Drill {
    pub point: Id,
    pub origin: Point3,
    /// Unit direction; station depths are measured from origin along this axis.
    pub axis: Vector3,
    /// Increasing depths, with repeated depths only for inward shoulders.
    pub stations: Vec<(f64, f64)>,
}

fn invalid(message: impl std::fmt::Display) -> CadError {
    CadError::Invalid(format!("Direct drilled pockets: {message}"))
}
fn finite(point: Point3) -> bool {
    [point.x, point.y, point.z].iter().all(|v| v.is_finite())
}

#[derive(Clone, Copy)]
struct Plane {
    normal: Vector3,
    origin: Point3,
}
impl Plane {
    fn signed(self, point: Point3) -> f64 {
        (point - self.origin).dot(self.normal)
    }
    fn crossing(self, drill: &Drill) -> f64 {
        (self.origin - drill.origin).dot(self.normal) / drill.axis.dot(self.normal)
    }
}

fn stock(solid: &Solid) -> Option<Vec<Plane>> {
    if solid.boundaries().len() != 1
        || solid.boundaries()[0].shell_condition() != ShellCondition::Closed
    {
        return None;
    }
    let vertices = solid.vertex_iter().map(|v| v.point()).collect::<Vec<_>>();
    if vertices.len() < 4 || vertices.iter().any(|p| !finite(*p)) {
        return None;
    }
    let mut planes: Vec<Plane> = Vec::new();
    for face in &solid.boundaries()[0] {
        let wires = face.boundaries();
        if !matches!(face.surface(), Surface::Plane(_))
            || wires.len() != 1
            || wires[0].len() < 3
            || wires[0]
                .iter()
                .any(|e| !matches!(e.curve(), Curve::Line(_)))
        {
            return None;
        }
        let normal = face.oriented_surface().normal(0., 0.);
        let length = normal.magnitude();
        if !length.is_finite() || length <= EPS {
            return None;
        }
        let normal = normal / length;
        let points = wires[0]
            .iter()
            .map(|e| e.front().point())
            .collect::<Vec<_>>();
        let plane = Plane {
            normal,
            origin: points[0],
        };
        if vertices.iter().any(|p| {
            let distance = plane.signed(*p);
            !distance.is_finite() || distance > EPS
        }) || points.iter().any(|p| plane.signed(*p).abs() > EPS)
        {
            return None;
        }
        // Each unsplit face is a convex polygon with the same outward
        // orientation as its carrier. No sample supplies this proof.
        for (index, point) in points.iter().enumerate() {
            let edge = points[(index + 1) % points.len()] - *point;
            let edge_length = edge.magnitude();
            if !edge_length.is_finite() || edge_length <= EPS {
                return None;
            }
            let inward = normal.cross(edge / edge_length);
            if points.iter().any(|p| (*p - *point).dot(inward) < -EPS) {
                return None;
            }
        }
        // Coplanar subdivisions would require distributing each opening
        // across several faces; keep them on the general Boolean path.
        if planes
            .iter()
            .any(|p| p.normal.dot(normal) > 1. - ANGULAR && p.signed(plane.origin).abs() <= EPS)
        {
            return None;
        }
        planes.push(plane);
    }
    (planes.len() >= 4).then_some(planes)
}

fn validate(drill: &Drill) -> Result<()> {
    let axis_length = drill.axis.magnitude();
    if !finite(drill.origin)
        || !axis_length.is_finite()
        || (axis_length - 1.).abs() > ANGULAR
        || drill.point.is_empty()
        || drill.stations.len() < 2
        || drill
            .stations
            .iter()
            .any(|(depth, radius)| !depth.is_finite() || !radius.is_finite() || *radius <= EPS)
    {
        return Err(invalid(
            "Drill requires a stable point ID, finite coordinates, a unit axis, and positive finite radii",
        ));
    }
    for pair in drill.stations.windows(2) {
        let span = pair[1].0 - pair[0].0;
        if !span.is_finite() || span < 0. || (span <= 1e-12 && pair[0].1 <= pair[1].1) {
            return Err(invalid(
                "Stations must increase in depth; an inward shoulder may repeat a depth",
            ));
        }
    }
    Ok(())
}

fn radius_at(stations: &[(f64, f64)], depth: f64) -> Option<f64> {
    for pair in stations.windows(2) {
        let [(a, r0), (b, r1)] = [pair[0], pair[1]];
        if b > a && depth >= a && depth <= b {
            let fraction = (depth - a) / (b - a);
            let radius = r0 * (1. - fraction) + r1 * fraction;
            return (radius.is_finite() && radius > EPS).then_some(radius);
        }
    }
    None
}

fn clipped(stations: &[(f64, f64)], start: f64, end: f64) -> Option<Vec<(f64, f64)>> {
    if !start.is_finite() || !end.is_finite() || end - start <= EPS {
        return None;
    }
    // Coincident opening/shoulder planes need a different topological
    // treatment. Avoid fabricating zero-depth recess walls or coplanar caps.
    if stations.windows(2).any(|p| {
        p[1].0 - p[0].0 <= 1e-12 && ((p[0].0 - start).abs() <= EPS || (p[0].0 - end).abs() <= EPS)
    }) {
        return None;
    }
    let mut output = vec![(start, radius_at(stations, start)?)];
    output.extend(
        stations
            .iter()
            .copied()
            .filter(|(d, _)| *d > start && *d < end),
    );
    output.push((end, radius_at(stations, end)?));
    if output.windows(2).any(|pair| {
        let span = pair[1].0 - pair[0].0;
        span > 1e-12 && span <= EPS
    }) {
        return None;
    }
    Some(output)
}

struct Pocket {
    point: Id,
    tool: Solid,
    entry: usize,
    exit: Option<usize>,
    center: Point3,
    axis: Vector3,
    radius: f64,
}

pub(crate) type Cut = (Solid, Vec<(Id, Solid)>);

fn prepare(drill: &Drill, planes: &[Plane]) -> Result<Option<Pocket>> {
    let direction = drill.axis.normalize();
    let entries = planes
        .iter()
        .enumerate()
        .filter(|(_, p)| {
            p.normal.dot(direction) < -1. + ANGULAR
                && p.normal.cross(direction).magnitude() <= ANGULAR
        })
        .collect::<Vec<_>>();
    if entries.len() != 1 {
        return Ok(None);
    }
    let (entry, entry_plane) = entries[0];
    let start = entry_plane.crossing(drill);
    let first = drill.stations[0].0;
    let last = drill.stations.last().unwrap().0;
    if !start.is_finite() || start < first || start >= last - EPS {
        return Ok(None);
    }
    let exits = planes
        .iter()
        .enumerate()
        .filter(|(_, p)| {
            p.normal.dot(direction) > 1. - ANGULAR
                && p.normal.cross(direction).magnitude() <= ANGULAR
        })
        .collect::<Vec<_>>();
    if exits.len() > 1 {
        return Ok(None);
    }
    let exit = exits.first().map(|(i, p)| (*i, p.crossing(drill)));
    if exit.is_some_and(|(_, depth)| !depth.is_finite() || depth <= start + EPS) {
        return Ok(None);
    }
    let through = exit.filter(|(_, depth)| last >= *depth);
    let end = through.map_or(last, |(_, depth)| depth);
    let Some(stations) = clipped(&drill.stations, start, end) else {
        return Ok(None);
    };
    let radius = stations.iter().map(|(_, r)| *r).fold(0., f64::max);
    if radius * entry_plane.normal.cross(direction).magnitude() > EPS * 0.25
        || through.is_some_and(|(index, _)| {
            radius * planes[index].normal.cross(direction).magnitude() > EPS * 0.25
        })
    {
        return Ok(None);
    }
    let tool = crate::holes::tool(drill.origin, drill.axis, &stations)?;
    let hull = crate::adapter::solid_control_points(&tool)?;
    // All rational weights are positive. Thus every exact boundary/surface
    // point lies inside this control hull; stations include every shoulder
    // and taper endpoint, without sampling the circular profile.
    if hull.iter().any(|point| {
        !finite(*point)
            || planes.iter().enumerate().any(|(index, plane)| {
                let distance = plane.signed(*point);
                if !distance.is_finite() {
                    return true;
                }
                if index == entry || through.is_some_and(|(exit, _)| index == exit) {
                    distance > EPS
                } else {
                    distance >= -EPS
                }
            })
    }) {
        return Ok(None);
    }
    Ok(Some(Pocket {
        point: drill.point.clone(),
        tool,
        entry,
        exit: through.map(|(i, _)| i),
        center: drill.origin + drill.axis * start,
        axis: direction,
        radius,
    }))
}

/// Returns exact cropped tools sharing their station wires with the result,
/// allowing the caller to publish construction provenance for every drill.
pub(crate) fn cut(solid: &Solid, drills: &[Drill]) -> Result<Option<Cut>> {
    if drills.is_empty() {
        return Ok(None);
    }
    let mut points = BTreeSet::new();
    for drill in drills {
        validate(drill)?;
        if !points.insert(&drill.point) {
            return Err(invalid("Hole-position point IDs must be distinct"));
        }
    }
    let Some(planes) = stock(solid) else {
        return local_cut(solid, drills);
    };
    let mut pockets = Vec::new();
    for drill in drills {
        let Some(pocket) = prepare(drill, &planes)? else {
            return Ok(None);
        };
        if pockets.iter().any(|other: &Pocket| {
            // The radius envelope is extruded along one common direction.
            // Accepting merely nearby directions would miss lateral drift
            // on a sufficiently deep hole. Other axes use the Boolean path.
            if pocket.axis != other.axis {
                return true;
            }
            let delta = pocket.center - other.center;
            let radial = delta - pocket.axis * delta.dot(pocket.axis);
            let distance = radial.magnitude();
            !distance.is_finite() || distance <= pocket.radius + other.radius + EPS
        }) {
            return Ok(None);
        }
        pockets.push(pocket);
    }
    let mut shell = solid.boundaries()[0].clone();
    let mut tools = Vec::new();
    for pocket in pockets {
        let tool_shell = &pocket.tool.boundaries()[0];
        let first = &tool_shell[0];
        let last = &tool_shell[tool_shell.len() - 1];
        let mut cap_ids = vec![first.id()];
        for (index, cap) in
            std::iter::once((pocket.entry, first)).chain(pocket.exit.map(|index| (index, last)))
        {
            let plane = planes[index];
            if !matches!(cap.surface(), Surface::Plane(_))
                || cap.boundaries().len() != 1
                || cap
                    .oriented_surface()
                    .normal(0., 0.)
                    .normalize()
                    .dot(plane.normal)
                    < 1. - ANGULAR
                || cap
                    .vertex_iter()
                    .any(|v| plane.signed(v.point()).abs() > EPS)
            {
                return Err(invalid(
                    "Cropped drill cap does not match its supporting stock face",
                ));
            }
            let mut face = shell[index].clone();
            face.add_boundary(cap.boundaries()[0].inverse())
                .map_err(invalid)?;
            shell[index] = face;
            if cap.id() != first.id() {
                cap_ids.push(cap.id());
            }
        }
        for face in pocket
            .tool
            .face_iter()
            .filter(|f| !cap_ids.contains(&f.id()))
        {
            shell.push(face.inverse());
        }
        tools.push((pocket.point, pocket.tool));
    }
    let result = Solid::try_new(vec![shell]).map_err(invalid)?;
    crate::validation::solid(&result)?;
    Ok(Some((result, tools)))
}

fn disk(drill: &Drill, depth: f64, radius: f64) -> Result<Face> {
    let radial = drill
        .axis
        .cross(if drill.axis.x.abs() < 0.9 {
            Vector3::unit_x()
        } else {
            Vector3::unit_y()
        })
        .normalize();
    let center = drill.origin + drill.axis * depth;
    let wire: Wire = primitive::circle(center + radial * radius, center, drill.axis, 4);
    builder::try_attach_plane(vec![wire]).map_err(invalid)
}

fn local_tool(
    solid: &Solid,
    drill: &Drill,
    budget: &mut crate::local_pockets::Budget,
) -> Result<Option<Solid>> {
    let planes = crate::local_pockets::planes_with_budget(solid, budget)?;
    let first = drill.stations[0].0;
    let last = drill.stations.last().unwrap().0;
    let mut entries = vec![];
    for plane in &planes {
        if plane.normal.dot(drill.axis) > -1. + ANGULAR
            || plane.normal.cross(drill.axis).magnitude() > ANGULAR
        {
            continue;
        }
        let depth = (plane.origin - drill.origin).dot(plane.normal) / drill.axis.dot(plane.normal);
        if !depth.is_finite() || depth < first || depth >= last - EPS {
            continue;
        }
        let Some(radius) = radius_at(&drill.stations, depth) else {
            continue;
        };
        if budget.covered(&plane.face, &disk(drill, depth, radius)?)? {
            entries.push(depth);
        }
    }
    entries.sort_by(f64::total_cmp);
    if entries.len() > 1 && (entries[0] - entries[1]).abs() <= EPS {
        return Ok(None);
    }
    let Some(start) = entries.first().copied() else {
        return Ok(None);
    };
    let mut exits = vec![];
    for plane in &planes {
        if plane.normal.dot(drill.axis) < 1. - ANGULAR
            || plane.normal.cross(drill.axis).magnitude() > ANGULAR
        {
            continue;
        }
        let depth = (plane.origin - drill.origin).dot(plane.normal) / drill.axis.dot(plane.normal);
        if !depth.is_finite() || depth <= start + EPS || depth > last {
            continue;
        }
        let Some(radius) = radius_at(&drill.stations, depth) else {
            continue;
        };
        if budget.covered(&plane.face, &disk(drill, depth, radius)?)? {
            exits.push(depth);
        }
    }
    exits.sort_by(f64::total_cmp);
    if exits.len() > 1 && (exits[0] - exits[1]).abs() <= EPS {
        return Ok(None);
    }
    let end = exits.first().copied().unwrap_or(last);
    let Some(stations) = clipped(&drill.stations, start, end) else {
        return Ok(None);
    };
    Ok(Some(crate::holes::tool(
        drill.origin,
        drill.axis,
        &stations,
    )?))
}

fn local_cut(solid: &Solid, drills: &[Drill]) -> Result<Option<Cut>> {
    let mut result = solid.clone();
    let mut tools = vec![];
    let mut budget = crate::local_pockets::Budget::new();
    for drill in drills {
        let original = crate::holes::tool(drill.origin, drill.axis, &drill.stations)?;
        match crate::prismatic_boolean::admits_along(&result, &original, drill.axis) {
            Ok(true) => return Ok(None),
            Ok(false) | Err(CadError::Unsupported(_)) => {}
            Err(error) => return Err(error),
        }
        let prepared = match local_tool(&result, drill, &mut budget) {
            Ok(prepared) => prepared,
            Err(CadError::Unsupported(_)) => return Ok(None),
            Err(error) => return Err(error),
        };
        let Some(tool) = prepared else {
            return Ok(None);
        };
        let Some(mut cut) = crate::local_pockets::insert_tool_with_budget(
            &result,
            &tool,
            drill.axis,
            &original,
            &mut budget,
        )?
        else {
            return Ok(None);
        };
        if cut.bodies.len() != 1 {
            return Err(invalid(
                "A contained local drill unexpectedly split its stock",
            ));
        }
        result = cut.bodies.remove(0);
        tools.push((drill.point.clone(), tool));
    }
    Ok(Some((result, tools)))
}
