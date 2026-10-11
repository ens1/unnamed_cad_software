//! Exact f64 constructions used by sketch drawing adapters.
use crate::{CadError, Result};
use nalgebra::Vector2;
fn invalid(message: &str) -> CadError {
    CadError::Invalid(message.into())
}

pub fn rectangle(
    a: [f64; 2],
    b: [f64; 2],
    c: [f64; 2],
    centered: bool,
    parallelogram: bool,
) -> Result<[[f64; 2]; 4]> {
    let a = Vector2::from(a);
    let b = Vector2::from(b);
    let c = Vector2::from(c);
    let direction = (b - a)
        .try_normalize(1e-9)
        .ok_or_else(|| invalid("Rectangle needs a nonzero first edge"))?;
    let normal = Vector2::new(-direction.y, direction.x);
    let height = normal.dot(&(c - a));
    if height.abs() <= 1e-9 {
        return Err(invalid("Rectangle/parallelogram height must be nonzero"));
    }
    let points = if parallelogram {
        [a, b, c, a + c - b]
    } else if centered {
        let u = b - a;
        let v = normal * height;
        [a - u - v, a + u - v, a + u + v, a - u + v]
    } else {
        let v = normal * height;
        [a, b, b + v, a + v]
    };
    Ok(points.map(|p| [p.x, p.y]))
}
pub fn center_arc(
    center: [f64; 2],
    start: [f64; 2],
    end_hint: [f64; 2],
    clockwise: bool,
) -> Result<[[f64; 2]; 3]> {
    let center = Vector2::from(center);
    let a = Vector2::from(start) - center;
    let b = Vector2::from(end_hint) - center;
    let radius = a.norm();
    if radius <= 1e-9 || b.norm() <= 1e-9 {
        return Err(invalid("Arc radius and end direction must be nonzero"));
    }
    let start_angle = a.y.atan2(a.x);
    let mut sweep = (b.y.atan2(b.x) - start_angle).rem_euclid(std::f64::consts::TAU);
    if sweep <= 1e-9 {
        return Err(invalid(
            "Choose a distinct arc endpoint; use Circle for a full turn",
        ));
    }
    if clockwise {
        sweep -= std::f64::consts::TAU;
    }
    let at = |t: f64| {
        let angle = start_angle + sweep * t;
        [
            center.x + radius * angle.cos(),
            center.y + radius * angle.sin(),
        ]
    };
    Ok([start, at(0.5), at(1.)])
}
pub fn tangent_arc(start: [f64; 2], end: [f64; 2], tangent: [f64; 2]) -> Result<[[f64; 2]; 3]> {
    let a = Vector2::from(start);
    let delta = Vector2::from(end) - a;
    let direction = Vector2::from(tangent)
        .try_normalize(1e-9)
        .ok_or_else(|| invalid("Tangent source collapsed"))?;
    let normal = Vector2::new(-direction.y, direction.x);
    let denominator = 2. * delta.dot(&normal);
    if denominator.abs() <= 1e-9 * delta.norm().max(1.) {
        return Err(invalid("Arc endpoint must lie off the tangent line"));
    }
    let signed_radius = delta.norm_squared() / denominator;
    let center = a + normal * signed_radius;
    let result = center_arc([center.x, center.y], start, end, signed_radius < 0.)?;
    if result.iter().flatten().any(|v| !v.is_finite()) {
        return Err(invalid("Tangent arc construction overflow"));
    }
    Ok(result)
}
