//! Validate B-rep geometry using the boundary's actual representation.
use cad_core::{CadError, Result};
use monstertruck::{modeling::*, topology::shell::ShellCondition};
use std::collections::HashMap;

fn invalid(message: &str) -> CadError {
    CadError::Invalid(format!("Geometry: {message}"))
}

fn finite(point: Point3) -> bool {
    point.x.is_finite() && point.y.is_finite() && point.z.is_finite()
}

// The classic Boolean backend creates implicit SurfaceCurves with a B-spline
// guide. The guide's chords are not the intersection curve, and need not lie on
// either curved carrier. Surface::include checks those chords in 0.4.1.
fn intersection_carriers(curve: &Curve) -> Result<Option<[Vec<u8>; 2]>> {
    let Curve::IntersectionCurve(intersection) = curve else {
        return Ok(None);
    };
    let (start, end) = curve.range_tuple();
    if !start.is_finite() || !end.is_finite() || start >= end {
        return Err(invalid(
            "Intersection boundary has an invalid parameter range",
        ));
    }
    let mut parameters = vec![start, end];
    let knots = match intersection.leader().as_ref() {
        Curve::Line(_) => None,
        Curve::BsplineCurve(guide) => Some(guide.knot_vector()),
        Curve::NurbsCurve(guide) => Some(guide.knot_vector()),
        _ => {
            return Err(CadError::Unsupported(
                "Cannot validate this intersection boundary's guide representation".into(),
            ));
        }
    };
    if let Some(knots) = knots {
        if knots.iter().any(|t| !t.is_finite()) {
            return Err(invalid("Intersection boundary has non-finite knots"));
        }
        parameters.extend(knots.iter().copied().filter(|t| *t > start && *t < end));
    }
    parameters.sort_by(f64::total_cmp);
    parameters.dedup();
    // Verify projection convergence and residuals in every guide span, including
    // endpoints. This uses the kernel's geometric precision, not mesh tolerance.
    for span in parameters.windows(2) {
        for step in 0..=4 {
            let t = span[0] + (span[1] - span[0]) * step as f64 / 4.;
            let Some((point, uv0, uv1)) = intersection.search_triple(t, 100) else {
                return Err(invalid(
                    "Intersection boundary could not be resolved on both supporting surfaces",
                ));
            };
            let on0 = intersection.surface0().subs(uv0.x, uv0.y);
            let on1 = intersection.surface1().subs(uv1.x, uv1.y);
            let actual = curve.subs(t);
            if !finite(point)
                || !finite(on0)
                || !finite(on1)
                || !finite(actual)
                || !point.near(&on0)
                || !point.near(&on1)
                || !actual.near(&point)
            {
                return Err(invalid(
                    "Intersection boundary does not lie on both supporting surfaces",
                ));
            }
        }
    }
    Ok(Some([
        serde_json::to_vec(intersection.surface0().as_ref())?,
        serde_json::to_vec(intersection.surface1().as_ref())?,
    ]))
}

pub(crate) fn solid(solid: &Solid) -> Result<()> {
    if solid.boundaries().is_empty()
        || solid
            .boundaries()
            .iter()
            .any(|shell| shell.shell_condition() != ShellCondition::Closed)
    {
        return Err(invalid("Operation did not produce a closed oriented B-rep"));
    }
    faces(&solid.face_iter().cloned().collect::<Vec<_>>())
}

pub(crate) fn surface(shell: &Shell) -> Result<()> {
    if shell.is_empty()
        || !matches!(
            shell.shell_condition(),
            ShellCondition::Oriented | ShellCondition::Closed
        )
    {
        return Err(invalid(
            "Operation did not produce an oriented manifold surface shell",
        ));
    }
    faces(shell)
}

fn faces(faces: &[Face]) -> Result<()> {
    let mut boundaries = HashMap::new();
    for edge in faces.iter().flat_map(Face::edge_iter) {
        if boundaries.contains_key(&edge.id()) {
            continue;
        }
        let carriers = intersection_carriers(&edge.curve())?;
        if !finite(edge.absolute_front().point())
            || !finite(edge.absolute_back().point())
            || !edge.is_geometric_consistent()
        {
            return Err(invalid(
                "B-rep edge endpoints are inconsistent with their curve",
            ));
        }
        boundaries.insert(edge.id(), carriers);
    }
    for face in faces {
        let surface = face.surface();
        let surface_key = serde_json::to_vec(&surface)?;
        for edge in face.edge_iter() {
            let included = if let Some(carriers) = &boundaries[&edge.id()] {
                // Exact carrier identity establishes membership by construction;
                // convergence checks above ensure evaluation preserves it.
                carriers.iter().any(|carrier| carrier == &surface_key)
            } else {
                surface.include(&edge.curve())
            };
            if !included {
                return Err(invalid(
                    "B-rep boundary curve does not belong to its supporting face",
                ));
            }
        }
    }
    Ok(())
}
