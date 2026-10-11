//! Complete branch certificates for a cylindrical end of a straight fillet.
//!
//! In the orthogonal frame (edge, inward cap, cylinder axis), the terminal is
//! x = cx +/- sqrt(R² - (y - cy)²). Positive outward discriminant bounds and a
//! fixed root half-space isolate the entire branch. No sampled guide supplies
//! the branch extent, carrier recognition, or angular traversal proof.
use crate::{
    analytic_offset::Carrier,
    native_bounds::{Interval, IntervalPoint, dot_intervals},
};
use cad_core::{CadError, Result};
use monstertruck::modeling::*;

const EPS: f64 = 1e-7;
const REL: f64 = 1e-10;

fn unsupported(message: &str) -> CadError {
    CadError::Unsupported(format!("Fillet cylindrical end: {message}"))
}

pub(crate) struct Certificate {
    pub(crate) points: [Point3; 2],
    /// Coordinates along the selected edge's positive axis, relative to source.
    pub(crate) axial_range: [f64; 2],
    pub(crate) arc_side: usize,
    pub(crate) circle_center: Point3,
    pub(crate) circle_normal: Vector3,
    pub(crate) circle_radius: f64,
    /// Signed turn from the source radial to the new rim radial.
    pub(crate) arc_extension: f64,
    /// The chosen root is strictly on the positive side of this plane.
    pub(crate) branch_origin: Point3,
    pub(crate) branch_normal: Vector3,
    pub(crate) branch_clearance: f64,
}

fn square(value: Interval) -> Result<Interval> {
    let a = value.low.abs();
    let b = value.high.abs();
    let low = if value.low <= 0. && value.high >= 0. {
        0.
    } else {
        (a.min(b) * a.min(b)).next_down().max(0.)
    };
    Interval::new(low, (a.max(b) * a.max(b)).next_up())
}

fn vector(v: Vector3) -> Result<IntervalPoint> {
    Ok([
        Interval::exact(v.x)?,
        Interval::exact(v.y)?,
        Interval::exact(v.z)?,
    ])
}

fn radial(v: IntervalPoint, normal: IntervalPoint) -> Result<IntervalPoint> {
    let along = dot_intervals(&v, &normal)?.div(dot_intervals(&normal, &normal)?)?;
    Ok([
        v[0].sub(normal[0].mul(along)?)?,
        v[1].sub(normal[1].mul(along)?)?,
        v[2].sub(normal[2].mul(along)?)?,
    ])
}

fn norm_squared(v: IntervalPoint) -> Result<Interval> {
    square(v[0])?.add(square(v[1])?)?.add(square(v[2])?)
}

fn control_point(h: Vector4) -> Result<IntervalPoint> {
    if ![h.x, h.y, h.z, h.w].into_iter().all(f64::is_finite) || h.w <= 0. {
        return Err(unsupported(
            "The meridian requires finite positive rational controls",
        ));
    }
    let weight = Interval::exact(h.w)?;
    Ok([
        Interval::exact(h.x)?.div(weight)?,
        Interval::exact(h.y)?.div(weight)?,
        Interval::exact(h.z)?.div(weight)?,
    ])
}

fn opposite<'a>(edge: &'a Edge, source: &Vertex) -> Option<&'a Vertex> {
    if edge.front().id() == source.id() {
        Some(edge.back())
    } else if edge.back().id() == source.id() {
        Some(edge.front())
    } else {
        None
    }
}

fn same_axis(a: Vector3, b: Vector3) -> bool {
    a.cross(b).magnitude() <= REL
}

/// Certify the complete cylinder/fillet intersection branch, allowing a bounded
/// continuation of the source rim at a concave profile corner. The caller must
/// construct that continuation on the same circular carrier, retain the whole
/// source wall, and prove the changed-material envelope clear of other trims.
pub(crate) fn certify(
    source: &Vertex,
    axis: Vector3,
    offsets: [Vector3; 2],
    terminal: &Face,
    neighbours: &[Edge; 2],
    radius: f64,
    guide_controls: &[Vector4; 3],
) -> Result<Option<Certificate>> {
    if !radius.is_finite()
        || radius <= EPS
        || !axis.magnitude2().is_finite()
        || (axis.magnitude2() - 1.).abs() > REL
        || offsets.iter().any(|v| {
            !v.magnitude2().is_finite()
                || (v.magnitude() - radius).abs() > EPS
                || v.dot(axis).abs() > EPS
        })
        || offsets[0].dot(offsets[1]).abs() > EPS * radius
    {
        return Ok(None);
    }
    let guide = Curve::NurbsCurve(NurbsCurve::new(BsplineCurve::new(
        KnotVector::bezier_knot(2),
        guide_controls.to_vec(),
    )));
    let Ok(((guide_center, guide_axis, guide_radius), guide_sweep)) =
        crate::bounded_carriers::circle(&guide)
    else {
        return Ok(None);
    };
    if !same_axis(guide_axis, axis)
        || guide_center.distance(source.point() + offsets[0] + offsets[1]) > EPS
        || (guide_radius - radius).abs() > EPS
        || (guide_sweep.abs() - std::f64::consts::FRAC_PI_2).abs() * radius > EPS
    {
        return Ok(None);
    }
    let Some(Carrier::Cone {
        origin: carrier_origin,
        axis: cylinder_axis,
        radius: carrier_radius,
        slope: 0.,
    }) = crate::analytic_offset::recognize(terminal)?
    else {
        return Ok(None);
    };
    let mut arc = None;
    for (side, neighbour) in neighbours.iter().enumerate() {
        if opposite(neighbour, source).is_none() {
            return Ok(None);
        }
        if let Ok((circle, sweep)) = crate::bounded_carriers::circle(&neighbour.curve()) {
            if arc.is_some() {
                return Ok(None);
            }
            arc = Some((side, circle, sweep));
        }
    }
    let Some((arc_side, (center, normal, circle_radius), sweep)) = arc else {
        return Ok(None);
    };
    let generator_side = 1 - arc_side;
    if crate::bounded_carriers::line(&neighbours[generator_side].curve()).is_err() {
        return Ok(None);
    }
    let inward = offsets[arc_side].normalize();
    let axial = offsets[generator_side].normalize();
    if !same_axis(normal, cylinder_axis)
        || !same_axis(axial, cylinder_axis)
        || normal.dot(inward).abs() > REL
        || normal.dot(axis).abs() > REL
        || (center - carrier_origin).cross(cylinder_axis).magnitude() > EPS
        || (carrier_radius - circle_radius).abs() > EPS
        || ((source.point() - center).magnitude() - circle_radius).abs() > EPS
    {
        return Ok(None);
    }
    let generator_end = opposite(&neighbours[generator_side], source)
        .ok_or_else(|| unsupported("The native generator lost its source vertex"))?;
    let generator_delta = generator_end.point() - source.point();
    let height = generator_delta.dot(axial);
    if height <= radius + EPS
        || generator_delta.cross(axial).magnitude() > EPS
        || (source.point() + offsets[generator_side] - generator_end.point()).magnitude() <= EPS
    {
        return Err(unsupported(
            "The radius consumes or leaves the cylindrical end's generator; reduce its size",
        ));
    }

    // A complete native four-edge extrusion patch gives a rectangular angular /
    // axial domain. A partial, holed, already-trimmed or conical carrier cannot
    // borrow this proof merely because its two incident edges happen to match.
    let boundaries = terminal.boundaries();
    if boundaries.len() != 1 || boundaries[0].len() != 4 {
        return Ok(None);
    }
    let mut circular = 0;
    let mut linear = 0;
    for edge in &boundaries[0] {
        if let Ok(((c, n, r), turn)) = crate::bounded_carriers::circle(&edge.curve()) {
            let separation = (c - center).dot(axial);
            if !same_axis(n, normal)
                || (c - center).cross(axial).magnitude() > EPS
                || (r - circle_radius).abs() > EPS
                || (turn.abs() - sweep.abs()).abs() * circle_radius > EPS
                || (separation.abs() > EPS && (separation - height).abs() > EPS)
            {
                return Ok(None);
            }
            circular += 1;
        } else if crate::bounded_carriers::line(&edge.curve()).is_ok() {
            let d = edge.back().point() - edge.front().point();
            if d.cross(axial).magnitude() > EPS || (d.magnitude() - height).abs() > EPS {
                return Ok(None);
            }
            linear += 1;
        } else {
            return Ok(None);
        }
    }
    if circular != 2 || linear != 2 {
        return Ok(None);
    }

    // Project with n dot n rather than assuming rounded unit vectors are
    // exactly orthonormal. This is the general quadratic along the selected
    // axis: A*x² + 2*B*x + C = 0. It specializes to the equation in the module
    // comment. Every point of the positive-weight meridian is in the control
    // triangle (source, source+offset0, source+offset1); this box encloses that
    // entire triangle, including all floating-point frame residuals.
    let normal_intervals = vector(cylinder_axis)?;
    let projected_axis = radial(vector(axis)?, normal_intervals)?;
    let a_coefficient = norm_squared(projected_axis)?;
    if a_coefficient.low <= 0. {
        return Ok(None);
    }
    let base = [
        Interval::exact(source.point().x)?.sub(Interval::exact(center.x)?)?,
        Interval::exact(source.point().y)?.sub(Interval::exact(center.y)?)?,
        Interval::exact(source.point().z)?.sub(Interval::exact(center.z)?)?,
    ];
    let offset_components = [
        [offsets[0].x, offsets[1].x],
        [offsets[0].y, offsets[1].y],
        [offsets[0].z, offsets[1].z],
    ];
    let source_coordinates = [source.point().x, source.point().y, source.point().z];
    let mut guide_offsets = Vec::with_capacity(3);
    for h in guide_controls {
        let mut point = control_point(*h)?;
        for index in 0..3 {
            point[index] = point[index].sub(Interval::exact(source_coordinates[index])?)?;
        }
        guide_offsets.push(point);
    }
    let mut offset_hull = [Interval::exact(0.)?; 3];
    let mut meridian = base;
    for index in 0..3 {
        offset_hull[index] = Interval::new(
            offset_components[index][0]
                .min(offset_components[index][1])
                .min(0.),
            offset_components[index][0]
                .max(offset_components[index][1])
                .max(0.),
        )?;
        for control in &guide_offsets {
            offset_hull[index].low = offset_hull[index].low.min(control[index].low);
            offset_hull[index].high = offset_hull[index].high.max(control[index].high);
        }
        meridian[index] = meridian[index].add(offset_hull[index])?;
    }
    let projected_meridian = radial(meridian, normal_intervals)?;
    let b_coefficient = dot_intervals(&projected_axis, &projected_meridian)?;
    let c_coefficient =
        norm_squared(projected_meridian)?.sub(square(Interval::exact(circle_radius)?)?)?;
    let discriminant = square(b_coefficient)?.sub(a_coefficient.mul(c_coefficient)?)?;
    let source_b = dot_intervals(&projected_axis, &radial(base, normal_intervals)?)?;
    if discriminant.low <= EPS * EPS || (source_b.low <= 0. && source_b.high >= 0.) {
        return Err(unsupported(
            "The cylindrical end reaches a root tangency or branch junction; reduce the radius",
        ));
    }
    let root = discriminant.sqrt()?;
    let sign = if source_b.low > 0. { 1. } else { -1. };
    let branch = Interval::exact(0.)?
        .sub(b_coefficient)?
        .add(root.mul(Interval::exact(sign)?)?)?
        .div(a_coefficient)?;
    let other_branch = Interval::exact(0.)?
        .sub(b_coefficient)?
        .sub(root.mul(Interval::exact(sign)?)?)?
        .div(a_coefficient)?;
    let radial_vector =
        |v: Vector3| v - cylinder_axis * (v.dot(cylinder_axis) / cylinder_axis.magnitude2());
    let projected_direction = radial_vector(axis);
    let quadratic_a = projected_direction.magnitude2();
    let branch_normal = projected_direction.normalize() * sign;
    let plane_bound = |extent: Interval| -> Result<Interval> {
        let axis_interval = vector(axis)?;
        let mut entire = meridian;
        for index in 0..3 {
            entire[index] = entire[index].add(axis_interval[index].mul(extent)?)?;
        }
        dot_intervals(&entire, &vector(branch_normal)?)
    };
    let branch_range = Interval::new(branch.low.min(0.), branch.high.max(0.))?;
    let branch_clearance = plane_bound(branch_range)?.low;
    if branch_clearance <= EPS || plane_bound(other_branch)?.high >= -EPS {
        return Err(unsupported(
            "A fixed plane cannot isolate both cylindrical root branches over this setback",
        ));
    }
    // The axis is a rounded unit vector and the supplied setbacks can have a
    // final-bit axial component. Return the true projection extent, not merely
    // the quadratic parameter extent, so the caller's material hull includes
    // those residuals as well.
    let axis_interval = vector(axis)?;
    let mut displacement = offset_hull;
    for index in 0..3 {
        displacement[index] = displacement[index].add(axis_interval[index].mul(branch_range)?)?;
    }
    let station_range = dot_intervals(&displacement, &axis_interval)?;

    // A single open radial half-space is a one-to-one angular chart. Certify
    // that the whole positive-weight guide remains between the two angular
    // endpoint coordinates, accounting for its actual homogeneous roundoff
    // and for the small nonorthogonality of the represented frame. Near a
    // cylindrical tangency the inverse chart amplifies that uncertainty; it
    // must still fit the existing geometric tolerance, never a larger one.
    let transverse = vector(cylinder_axis.cross(axis).normalize())?;
    let endpoint = dot_intervals(&vector(offsets[arc_side])?, &transverse)?;
    let mut transverse_extent = Interval::new(endpoint.low.min(0.), endpoint.high.max(0.))?;
    for control in guide_offsets {
        let projected = dot_intervals(&control, &transverse)?;
        transverse_extent.low = transverse_extent.low.min(projected.low);
        transverse_extent.high = transverse_extent.high.max(projected.high);
    }
    transverse_extent =
        transverse_extent.add(branch_range.mul(dot_intervals(&axis_interval, &transverse)?)?)?;
    let excess = (endpoint.low.min(0.) - transverse_extent.low)
        .max(transverse_extent.high - endpoint.high.max(0.))
        .max(0.);
    let angular_error = Interval::exact(excess)?
        .mul(Interval::exact(circle_radius)?)?
        .div(Interval::exact(branch_clearance)?)?;
    if angular_error.high > EPS {
        return Err(unsupported(
            "The complete meridian cannot be bounded inside its cylindrical angular continuation",
        ));
    }
    let root_for = |offset: Vector3| -> Option<f64> {
        let q = radial_vector(source.point() - center + offset);
        let b = projected_direction.dot(q);
        let c = q.magnitude2() - circle_radius * circle_radius;
        let discriminant = b * b - quadratic_a * c;
        (discriminant.is_finite() && discriminant > 0.)
            .then(|| (-b + sign * discriminant.sqrt()) / quadratic_a)
    };
    if root_for(Vector3::new(0., 0., 0.)).is_none_or(|x| x.abs() > EPS) {
        return Err(unsupported(
            "The source vertex does not resolve on the isolated cylindrical branch",
        ));
    }
    let shift = root_for(offsets[arc_side])
        .ok_or_else(|| unsupported("The cylindrical rim contact is singular"))?;
    let mut points = offsets.map(|offset| source.point() + offset);
    points[arc_side] += axis * shift;
    if points
        .iter()
        .any(|p| ![p.x, p.y, p.z].into_iter().all(f64::is_finite))
    {
        return Err(unsupported("The end contact exceeds finite coordinates"));
    }
    let a = source.point() - center;
    let b = points[arc_side] - center;
    let extension = a.cross(b).dot(normal).atan2(a.dot(b));
    let retained = if neighbours[arc_side].absolute_front().id() == source.id() {
        sweep - extension
    } else {
        sweep + extension
    };
    let angular_margin = EPS / circle_radius;
    if !extension.is_finite()
        || extension.abs() >= std::f64::consts::PI - angular_margin
        || retained * sweep <= 0.
        || retained.abs() <= angular_margin
        || retained.abs() >= std::f64::consts::TAU - angular_margin
    {
        return Err(unsupported(
            "The cylindrical end consumes, retraces or closes its rim; reduce the radius",
        ));
    }

    // A transverse quarter circle has a regular rational tangent. Its tangent
    // dotted with the two carrier normals' cross product equals the nonzero
    // radial root times the meridian's oriented circle-speed numerator. Thus
    // the projection-plane intersection has no singularity on this branch.
    Ok(Some(Certificate {
        points,
        axial_range: [station_range.low.min(0.), station_range.high.max(0.)],
        arc_side,
        circle_center: center,
        circle_normal: normal,
        circle_radius,
        arc_extension: extension,
        branch_origin: center,
        branch_normal,
        branch_clearance,
    }))
}
