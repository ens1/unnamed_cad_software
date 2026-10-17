//! Whole-strip containment on convex native planar stock.
//!
//! A weld footprint is not a Boolean arrangement: on convex line-bounded
//! stock, membership of its affine corners proves membership of the complete
//! strip. In particular, its root may coincide with a miter trim without
//! solving a new, ill-conditioned line/line crossing at that same trim.
use super::{EPS, invalid};
use crate::{analytic_offset::Carrier, native_bounds::Interval};
use cad_core::Result;
use monstertruck::modeling::*;

pub(super) struct ConvexPatch {
    origin: Point3,
    normal: Vector3,
    dropped: usize,
    points: Vec<Point3>,
    projected: Vec<Point2>,
    winding: f64,
}

fn orient(a: Point2, b: Point2, p: Point2) -> f64 {
    let coord = |point: Point2| robust::Coord {
        x: point.x,
        y: point.y,
    };
    robust::orient2d(coord(a), coord(b), coord(p))
}

fn projected(point: Point3, dropped: usize) -> Point2 {
    // Dropping a coordinate preserves the input dyadics. Rotating into an
    // independently rounded local frame would perturb coincident boundaries.
    Point2::new(point[(dropped + 1) % 3], point[(dropped + 2) % 3])
}

fn square(value: Interval) -> Result<Interval> {
    if value.low >= 0. {
        value.mul(value)
    } else if value.high <= 0. {
        let positive = Interval::new(-value.high, -value.low)?;
        positive.mul(positive)
    } else {
        let high = value.magnitude();
        Interval::new(0., (high * high).next_up())
    }
}

fn norm(value: [Interval; 3]) -> Result<Interval> {
    let mut squared = Interval::exact(0.)?;
    for coordinate in value {
        squared = squared.add(square(coordinate)?)?;
    }
    Interval::new(squared.low.max(0.), squared.high)?.sqrt()
}

fn displacement(a: Point3, b: Point3) -> Result<[Interval; 3]> {
    let mut result = [Interval::exact(0.)?; 3];
    for index in 0..3 {
        result[index] = Interval::exact(a[index])?.sub(Interval::exact(b[index])?)?;
    }
    Ok(result)
}

/// Enclose the distance to a particular point on the *native* straight side.
/// The rounded closest-point parameter only chooses that witness; it need not
/// be an exact minimizer. The final distance includes all arithmetic rounding.
fn boundary_distance(point: Point3, a: Point3, b: Point3) -> Result<Interval> {
    let direction = b - a;
    let fraction = ((point - a).dot(direction) / direction.magnitude2()).clamp(0., 1.);
    let t = Interval::exact(fraction)?;
    let edge = displacement(b, a)?;
    let offset = displacement(point, a)?;
    let mut delta = [Interval::exact(0.)?; 3];
    for index in 0..3 {
        delta[index] = offset[index].sub(edge[index].mul(t)?)?;
    }
    norm(delta)
}

fn signed_area(a: Point2, b: Point2, p: Point2, winding: f64) -> Result<Interval> {
    let difference = |a, b| Interval::exact(a)?.sub(Interval::exact(b)?);
    difference(b.x, a.x)?
        .mul(difference(p.y, a.y)?)?
        .sub(difference(b.y, a.y)?.mul(difference(p.x, a.x)?)?)?
        .mul(Interval::exact(winding)?)
}

impl ConvexPatch {
    pub(super) fn from_face(face: &Face, carrier: Carrier) -> Result<Option<Self>> {
        let Carrier::Plane { origin, normal } = carrier else {
            return Ok(None);
        };
        let boundaries = face.boundaries();
        // Multiple wires may contain a hole. Convex hull containment alone
        // must never authorize material across one of those excluded regions.
        if boundaries.len() != 1 {
            return Ok(None);
        }
        let wire = &boundaries[0];
        if !(3..=64).contains(&wire.len()) || !wire.is_continuous() || !wire.is_closed() {
            return Ok(None);
        }
        let dropped = (0..3)
            .max_by(|a, b| normal[*a].abs().total_cmp(&normal[*b].abs()))
            .unwrap();
        if !normal[dropped].is_finite() || normal[dropped].abs() < 0.5 {
            return Ok(None);
        }
        let mut patch = Self {
            origin,
            normal,
            dropped,
            points: Vec::with_capacity(wire.len()),
            projected: Vec::with_capacity(wire.len()),
            winding: 0.,
        };
        for edge in wire {
            // This admission is intentionally narrower than general tolerant
            // line recognition: every side is an exact native segment, with
            // the same endpoints as its topology. Rational/curved trims retain
            // the existing full arrangement proof.
            let Curve::Line(line) = edge.oriented_curve() else {
                return Ok(None);
            };
            if line.0 != edge.front().point() || line.1 != edge.back().point() {
                return Ok(None);
            }
            let point = line.0;
            if [point.x, point.y, point.z]
                .iter()
                .any(|value| !value.is_finite() || value.abs() > 1e12)
                || patch.points.contains(&point)
                || norm(displacement(line.1, line.0)?)?.low <= EPS
                || patch.lift_error(point)? > EPS * 0.125
            {
                return Ok(None);
            }
            patch.points.push(point);
            patch.projected.push(projected(point, dropped));
        }
        // All vertices must lie on the same inward side of *every* edge.
        // Checking only consecutive turns would admit self-crossing stars.
        // Exact adaptive orientation resolves collinear native vertices
        // without introducing a model-sized convexity epsilon.
        for index in 0..patch.projected.len() {
            let a = patch.projected[index];
            let b = patch.projected[(index + 1) % patch.projected.len()];
            if a == b {
                return Ok(None);
            }
            for point in &patch.projected {
                let side = orient(a, b, *point);
                if side == 0. {
                    continue;
                }
                if patch.winding == 0. {
                    patch.winding = side.signum();
                } else if side.signum() != patch.winding {
                    return Ok(None);
                }
            }
        }
        Ok((patch.winding != 0.).then_some(patch))
    }

    /// A bound on the change to the dropped coordinate needed to lie on the
    /// native carrier. For a projected interior point, the query and a convex
    /// combination of native vertices differ by at most the sum of these
    /// bounds. A quarter of the existing contact tolerance covers that sum.
    fn lift_error(&self, point: Point3) -> Result<f64> {
        let offset = displacement(point, self.origin)?;
        let mut residual = Interval::exact(0.)?;
        for (index, coordinate) in offset.iter().enumerate() {
            residual = residual.add(coordinate.mul(Interval::exact(self.normal[index])?)?)?;
        }
        Ok(residual
            .div(Interval::exact(self.normal[self.dropped])?)?
            .magnitude())
    }

    /// Some proves the complete convex hull of the supplied corners is on
    /// support. None leaves an unresolved near-boundary case to the existing
    /// arrangement. A definitely outside corner is a real invalid footprint.
    pub(super) fn contains(&self, corners: &[Point3]) -> Result<Option<()>> {
        for point in corners {
            if self.lift_error(*point)? > EPS * 0.125 {
                return Ok(None);
            }
            let p = projected(*point, self.dropped);
            let mut inside = true;
            for index in 0..self.projected.len() {
                let a = self.projected[index];
                let b = self.projected[(index + 1) % self.projected.len()];
                if orient(a, b, p) * self.winding >= 0. {
                    continue;
                }
                inside = false;
                // Coordinate dropping cannot increase distance. A projected
                // outside half-plane clearance beyond EPS is therefore also
                // outside the actual 3D support, without solving a crossing.
                let length = norm([
                    Interval::exact(b.x)?.sub(Interval::exact(a.x)?)?,
                    Interval::exact(b.y)?.sub(Interval::exact(a.y)?)?,
                    Interval::exact(0.)?,
                ])?;
                let margin = Interval::exact(EPS)?.mul(length)?.high;
                if signed_area(a, b, p, self.winding)?.high < -margin {
                    return Err(invalid(
                        "The bead footprint extends outside the selected support's trimmed boundaries; reduce bead size, increase start/end trimming, or replace the support",
                    ));
                }
            }
            if inside {
                continue;
            }
            let mut on_boundary = false;
            for index in 0..self.points.len() {
                if boundary_distance(
                    *point,
                    self.points[index],
                    self.points[(index + 1) % self.points.len()],
                )?
                .high
                    <= EPS * 0.5
                {
                    on_boundary = true;
                    break;
                }
            }
            if !on_boundary {
                return Ok(None);
            }
        }
        Ok(Some(()))
    }
}
