//! Face-local native boundaries shared by admission, tessellation, and exchange.
//! Search continuation hints are deliberately excluded from exact trim storage.
use cad_core::{CadError, Result};
use monstertruck::{
    meshing::prelude::*,
    modeling::*,
    topology::{
        compress::{CompressedShell, CompressedTrimmedShell},
        trimmed::TrimmedShell,
    },
};

type Trim = ParameterCurve<Curve2D, Box<Surface>>;
pub(crate) type NativeShell = TrimmedShell<Point3, Curve, Surface, Trim>;
pub(crate) type Compressed = CompressedTrimmedShell<Point3, Curve, Surface, Trim>;

fn invalid(message: &str) -> CadError {
    CadError::Invalid(format!("Geometry: native trim: {message}"))
}
fn finite(point: Point3) -> bool {
    [point.x, point.y, point.z].into_iter().all(f64::is_finite)
}

/// Reconstructs transient branch restrictions from the current carriers.
///
/// Native edges share their geometry storage. Guard installation is a derived
/// admission cache, excluded from serialized geometry and content identity.
/// Compute every replacement before changing that cache so an unresolved proof
/// cannot leave a shell partially refreshed.
fn install_continuation_guards(source: &Shell, linear_mm: f64) -> Result<()> {
    use monstertruck::solid::{
        ContinuationError, certify_intersection_continuation_with_halfspaces,
    };
    let mut seen = std::collections::HashSet::new();
    let mut replacements = Vec::new();
    for edge in source.edge_iter() {
        if !seen.insert(edge.id()) {
            continue;
        }
        let Curve::IntersectionCurve(intersection) = edge.curve() else {
            continue;
        };
        let Some(traces) = intersection.parameter_traces() else {
            continue;
        };
        let key = crate::continuation_cache::key(&intersection, linear_mm);
        let cached = key.and_then(crate::continuation_cache::get);
        let tubes = if let Some(cached) = cached {
            cached
        } else {
            match certify_intersection_continuation_with_halfspaces(
                intersection.leader().as_ref(),
                intersection.surface0().as_ref(),
                intersection.surface1().as_ref(),
                traces,
                intersection.branch_halfspaces(),
                linear_mm,
            ) {
                Ok(certificate) => certificate.parameter_tubes,
                // Angular analytic carriers retain their existing native checks.
                // They are not covered by the rational continuation certificate.
                Err(ContinuationError::UnsupportedCarrier) => continue,
                Err(error) => {
                    return Err(CadError::Unsupported(format!(
                        "Intersection continuation: {error}"
                    )));
                }
            }
        };
        let unchanged = intersection.parameter_tubes() == Some(tubes.as_slice());
        let guarded = intersection.with_parameter_tubes(tubes).ok_or_else(|| {
            invalid("The certified branch restrictions do not cover its parameter range")
        })?;
        let (start, end) = guarded.range_tuple();
        for (point, vertex) in [guarded.subs(start), guarded.subs(end)]
            .into_iter()
            .zip([edge.absolute_front(), edge.absolute_back()])
        {
            if !finite(point) || point.distance(vertex.point()) > linear_mm {
                return Err(invalid(
                    "The certified intersection branch does not meet its shared vertices",
                ));
            }
        }
        if let Some(key) = key {
            crate::continuation_cache::insert(key, guarded.parameter_tubes().unwrap_or_default());
        }
        if !unchanged {
            replacements.push((edge, Curve::IntersectionCurve(guarded)));
        }
    }
    for (edge, curve) in replacements {
        edge.set_curve(curve);
    }
    Ok(())
}

pub(crate) fn shell(shell: &Shell, linear_mm: f64) -> Result<NativeShell> {
    if !linear_mm.is_finite() || linear_mm <= 0. {
        return Err(invalid("Boundary residual must be finite and positive"));
    }
    install_continuation_guards(shell, linear_mm)?;
    let result = shell.to_trimmed_with_exact_face_trims();
    for face in result.faces() {
        let surface_key = serde_json::to_vec(&face.face().surface())?;
        for (wire, trims) in face.face().absolute_boundaries().iter().zip(face.trims()) {
            for (edge, trim) in wire.iter().zip(trims) {
                let Some(trim) = trim else { continue };
                if serde_json::to_vec(trim.surface().as_ref())? != surface_key {
                    return Err(invalid(
                        "An exact boundary refers to a different face carrier",
                    ));
                }
                let (start, end) = trim.range_tuple();
                if !start.is_finite() || !end.is_finite() || start >= end {
                    return Err(invalid(
                        "An exact boundary has no finite increasing interval",
                    ));
                }
                let (front, back) = (trim.subs(start), trim.subs(end));
                if !finite(front)
                    || !finite(back)
                    || front.distance(edge.absolute_front().point()) > linear_mm
                    || back.distance(edge.absolute_back().point()) > linear_mm
                {
                    return Err(invalid(
                        "An exact boundary does not meet its shared edge vertices",
                    ));
                }
                // The exact-boundary provider guarantees the locus. Its UV
                // curve may traverse that locus at a different speed from the
                // 3D edge: a bound between normalized parameters is therefore
                // not a disagreement test. Keep the constructor's native
                // membership checks; never reject from an upper bound alone.
            }
        }
    }
    Ok(result)
}

pub(crate) fn validate_shape(shape: &crate::adapter::Shape, linear_mm: f64) -> Result<()> {
    if let Some(shells) = shape.boundary_shells() {
        for boundary in shells {
            shell(boundary, linear_mm)?;
        }
    }
    Ok(())
}

pub(crate) fn triangulate(
    source: &Shell,
    linear_mm: f64,
    display_mm: f64,
) -> Result<CompressedShell<Point3, PolylineCurve<Point3>, Option<PolygonMesh>>> {
    let trimmed = Compressed::from(&shell(source, linear_mm)?);
    Ok(robust_trimmed_cshell_triangulation_with(
        &trimmed,
        TessellationOptions {
            tolerance: display_mm,
            ..Default::default()
        },
    ))
}

fn branch_contains(
    curve: &Curve,
    halfspaces: &[(Point3, Vector3)],
    remaining_mm: f64,
) -> Result<bool> {
    use crate::native_bounds::Interval;
    let Curve::NurbsCurve(curve) = curve else {
        return Ok(false);
    };
    for (origin, normal) in halfspaces {
        let mut norm2 = Interval::exact(0.)?;
        for coordinate in [normal.x, normal.y, normal.z] {
            let coordinate = Interval::exact(coordinate)?;
            norm2 = norm2.add(coordinate.mul(coordinate)?)?;
        }
        let norm = norm2.sqrt()?;
        if norm.low <= 0. {
            return Ok(false);
        }
        for point in curve.control_points() {
            if point.w <= 0. {
                return Ok(false);
            }
            let weight = Interval::exact(point.w)?;
            let mut dot = Interval::exact(0.)?;
            for axis in 0..3 {
                let offset = Interval::exact(point[axis])?
                    .sub(Interval::exact(origin[axis])?.mul(weight)?)?;
                dot = dot.add(offset.mul(Interval::exact(normal[axis])?)?)?;
            }
            if dot.div(weight)?.div(norm)?.low < -remaining_mm {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

/// Materialize a boundary within the saved residual budget, never its search guide.
pub(crate) fn exchange_curve(curve: &Curve, linear_mm: f64) -> Result<Curve> {
    match curve {
        Curve::ParameterCurve(_) => curve
            .try_lift_parameter_curve_with_error()
            .filter(|(_, error)| error.is_finite() && *error <= linear_mm)
            .map(|(curve, _)| curve)
            .ok_or_else(|| CadError::Unsupported("STEP export cannot certify this parameter boundary's algebraic 3D representation within the saved modeling residual; its parameterization, knot-cell crossing, or conditioning is unsupported".into())),
        Curve::IntersectionCurve(intersection) => {
            if let (Some(a), Some(b)) = (intersection.boundary0(), intersection.boundary1()) {
                let a = Curve::ParameterCurve(a.clone());
                let b = Curve::ParameterCurve(b.clone());
                if let Some((lifted, error)) = a.try_lift_parameter_curve_with_error()
                    && error.is_finite() && error <= linear_mm
                    && lifted.try_parameter_distance_bound(&b)
                        .is_some_and(|distance| distance <= linear_mm)
                    && branch_contains(&lifted, intersection.branch_halfspaces(), linear_mm - error)? {
                    return Ok(lifted);
                }
            }
            Err(CadError::Unsupported("STEP export cannot certify this implicit boundary against both adjacent carriers and its branch domain; the search guide is not export geometry. Save the editable TCAD document instead".into()))
        }
        _ => Ok(curve.clone()),
    }
}

pub(crate) fn exchange_shell(source: &Shell, linear_mm: f64) -> Result<Compressed> {
    let mut result = Compressed::from(&shell(source, linear_mm)?);
    for edge in &mut result.edges {
        edge.curve = exchange_curve(&edge.curve, linear_mm)?;
        let (start, end) = edge.curve.range_tuple();
        for (point, vertex) in [edge.curve.subs(start), edge.curve.subs(end)]
            .into_iter()
            .zip([edge.vertices.0, edge.vertices.1])
        {
            if !finite(point) || point.distance(result.vertices[vertex]) > linear_mm {
                return Err(invalid(
                    "The exported 3D boundary exceeds its shared vertex residual",
                ));
            }
        }
    }
    Ok(result)
}
