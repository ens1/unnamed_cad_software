use cad_core::{CadError, Dimension, Document, Expr, Result, SheetBend, expression};
use monstertruck::modeling::*;
#[derive(Clone)]
struct Segment {
    start: Point3,
    end: Point3,
    transit: Option<Point3>,
}
fn invalid(message: &str) -> CadError {
    CadError::Invalid(message.into())
}
pub fn sheet_strip(
    doc: &Document,
    owner: &str,
    width: &Expr,
    thickness: &Expr,
    sections: &[Expr],
    bends: &[SheetBend],
) -> Result<Solid> {
    let length = |e: &Expr| expression::value(doc, owner, e, Dimension::Length);
    let t = length(thickness)?;
    let width = length(width)?;
    if t <= 0. || width <= 0. || sections.is_empty() || bends.len() + 1 != sections.len() {
        return Err(invalid(
            "Sheet strip needs positive width/thickness and one fewer bends than straight sections",
        ));
    }
    let mut center = Vector2::new(0., t * 0.5);
    let mut tangent = Vector2::unit_x();
    let mut left = vec![];
    let mut right = vec![];
    let p3 = |v: Vector2| Point3::new(v.x, 0., v.y);
    for (i, section) in sections.iter().enumerate() {
        let l = length(section)?;
        if l <= 1e-7 {
            return Err(invalid("Sheet straight sections must be positive"));
        }
        let normal = Vector2::new(-tangent.y, tangent.x);
        let next = center + tangent * l;
        for (offset, segments) in [(t * 0.5, &mut left), (-t * 0.5, &mut right)] {
            segments.push(Segment {
                start: p3(center + normal * offset),
                end: p3(next + normal * offset),
                transit: None,
            });
        }
        center = next;
        if let Some(bend) = bends.get(i) {
            let angle = expression::value(doc, owner, &bend.angle, Dimension::Angle)?;
            let radius = length(&bend.inside_radius)?;
            if radius <= 0.
                || angle.abs() < 1e-5
                || angle.abs() >= 175f64.to_radians()
                || !(0.0..=1.0).contains(&bend.k_factor)
            {
                return Err(invalid(
                    "Sheet bends require positive inside radii, angles between 0 and 175 degrees, and K factors in [0,1]",
                ));
            }
            let sign = angle.signum();
            let bend_center = center + normal * ((radius + t * 0.5) * sign);
            let rotate = |v: Vector2, a: f64| {
                Vector2::new(v.x * a.cos() - v.y * a.sin(), v.x * a.sin() + v.y * a.cos())
            };
            let next = bend_center + rotate(center - bend_center, angle);
            let next_tangent = rotate(tangent, angle);
            let next_normal = Vector2::new(-next_tangent.y, next_tangent.x);
            for (offset, segments) in [(t * 0.5, &mut left), (-t * 0.5, &mut right)] {
                let start = center + normal * offset;
                let end = next + next_normal * offset;
                let transit = bend_center + rotate(start - bend_center, angle * 0.5);
                segments.push(Segment {
                    start: p3(start),
                    end: p3(end),
                    transit: Some(p3(transit)),
                });
            }
            center = next;
            tangent = next_tangent;
        }
    }
    let first_left = left
        .first()
        .ok_or_else(|| invalid("Empty sheet outline"))?
        .start;
    let first_right = right
        .first()
        .ok_or_else(|| invalid("Empty sheet outline"))?
        .start;
    let last_left = left
        .last()
        .ok_or_else(|| invalid("Empty sheet outline"))?
        .end;
    let last_right = right
        .last()
        .ok_or_else(|| invalid("Empty sheet outline"))?
        .end;
    let mut segments = left;
    segments.push(Segment {
        start: last_left,
        end: last_right,
        transit: None,
    });
    for segment in right.into_iter().rev() {
        segments.push(Segment {
            start: segment.end,
            end: segment.start,
            transit: segment.transit,
        });
    }
    segments.push(Segment {
        start: first_right,
        end: first_left,
        transit: None,
    });
    let vertices = segments
        .iter()
        .map(|s| builder::vertex(s.start))
        .collect::<Vec<_>>();
    let mut edges = vec![];
    for (index, segment) in segments.iter().enumerate() {
        let a = &vertices[index];
        let b = &vertices[(index + 1) % vertices.len()];
        let edge: Edge = if let Some(transit) = segment.transit {
            builder::try_circle_arc(a, b, transit).map_err(|e| invalid(&e.to_string()))?
        } else {
            builder::line(a, b)
        };
        edges.push(edge);
    }
    let face: Face =
        builder::try_attach_plane(vec![edges.into()]).map_err(|e| invalid(&e.to_string()))?;
    Ok(crate::adapter::oriented_extrude(
        &face,
        Vector3::new(0., width, 0.),
    ))
}
