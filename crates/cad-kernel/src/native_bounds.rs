//! Conservative native enclosures for retaining geometry outside an edit.
//! Exact rational controls remain the tight first choice. Analytic carriers
//! use certified bounds; intersection leaders and display meshes are never
//! enclosure inputs.
use cad_core::{CadError, Result};
use monstertruck::{
    geometry::prelude::TryIntoHomogeneousBsplineSurface,
    modeling::{bounding::certified_face_bounding_box, *},
};

const MAX_HULL_POINTS: usize = 262_144;

enum Analytic {
    Sphere {
        transform: Matrix4,
        center: Point3,
        radius: f64,
    },
    Torus {
        transform: Matrix4,
        center: Point3,
        major: f64,
        minor: f64,
    },
}

pub(crate) struct Enclosure {
    hull: Vec<Point3>,
    analytic: Option<Analytic>,
}

impl Enclosure {
    pub(crate) fn from_hull(hull: Vec<Point3>) -> Result<Self> {
        Ok(Self {
            hull: checked(hull)?,
            analytic: None,
        })
    }

    pub(crate) fn len(&self) -> usize {
        self.hull.len()
    }

    pub(crate) fn points(&self) -> &[Point3] {
        &self.hull
    }

    /// Signed support bounds in the caller's direction. Analytic support
    /// avoids the excess corners of an axis-aligned box on rotated parts.
    /// For an affine transform A, use A^T n in the source carrier's support
    /// function; this remains exact for nonuniform scaling and shear.
    pub(crate) fn projection(&self, origin: Point3, normal: Vector3) -> Result<(f64, f64)> {
        let analytic = self.analytic.as_ref().map(|analytic| {
            let (transform, center) = match analytic {
                Analytic::Sphere {
                    transform, center, ..
                }
                | Analytic::Torus {
                    transform, center, ..
                } => (transform, center),
            };
            let direction = Vector3::new(
                transform.x.truncate().dot(normal),
                transform.y.truncate().dot(normal),
                transform.z.truncate().dot(normal),
            );
            let reach = match analytic {
                Analytic::Sphere { radius, .. } => radius * direction.magnitude(),
                Analytic::Torus { major, minor, .. } => {
                    major * direction.x.hypot(direction.y) + minor * direction.magnitude()
                }
            };
            let signed = (*center - origin).dot(normal);
            (signed - reach, signed + reach)
        });
        let (low, high) = match analytic {
            Some(range) => range,
            None => {
                let mut low = f64::INFINITY;
                let mut high = f64::NEG_INFINITY;
                for point in &self.hull {
                    let signed = (*point - origin).dot(normal);
                    if !signed.is_finite() {
                        return Err(CadError::Invalid(
                            "Native enclosure projection exceeds finite coordinates".into(),
                        ));
                    }
                    low = low.min(signed);
                    high = high.max(signed);
                }
                (low, high)
            }
        };
        if !low.is_finite() || !high.is_finite() || low > high {
            return Err(CadError::Invalid(
                "Native enclosure projection exceeds finite coordinates".into(),
            ));
        }
        Ok((low, high))
    }
}

fn affine(transform: &Matrix4) -> bool {
    transform.x.w == 0. && transform.y.w == 0. && transform.z.w == 0. && transform.w.w == 1.
}

pub(crate) fn face(face: &Face) -> Result<Enclosure> {
    let hull = face_hull(face)?;
    let analytic = match face.surface() {
        Surface::SphericalSurface(processor) if affine(processor.transform()) => {
            Some(Analytic::Sphere {
                transform: *processor.transform(),
                center: processor
                    .transform()
                    .transform_point(processor.entity().center()),
                radius: processor.entity().radius().abs(),
            })
        }
        Surface::ToroidalSurface(processor) if affine(processor.transform()) => {
            Some(Analytic::Torus {
                transform: *processor.transform(),
                center: processor
                    .transform()
                    .transform_point(processor.entity().center()),
                major: processor.entity().large_radius().abs(),
                minor: processor.entity().small_radius().abs(),
            })
        }
        _ => None,
    };
    Ok(Enclosure { hull, analytic })
}

fn unsupported(message: &str) -> CadError {
    CadError::Unsupported(format!("Native enclosure: {message}"))
}

fn checked(points: Vec<Point3>) -> Result<Vec<Point3>> {
    if points.is_empty() || points.len() > MAX_HULL_POINTS {
        return Err(unsupported(
            "The enclosure is empty or exceeds its control budget",
        ));
    }
    if points
        .iter()
        .any(|p| ![p.x, p.y, p.z].iter().all(|value| value.is_finite()))
    {
        return Err(CadError::Invalid(
            "Native enclosure has non-finite coordinates".into(),
        ));
    }
    Ok(points)
}

/// An exact rational carrier whose entire bounded parameter domain encloses
/// the face. A line revolution is unbounded along its generator: converting
/// its nominal line segment does not enclose arbitrary trims on that line.
/// Composite profiles are excluded because converting an intersection curve
/// would substitute its guide for the authoritative intersection.
pub(crate) fn subdivision_surface(face: &Face) -> Result<Option<BsplineSurface<Vector4>>> {
    let surface = face.surface();
    let admitted = match &surface {
        Surface::BsplineSurface(_)
        | Surface::NurbsSurface(_)
        | Surface::SphericalSurface(_)
        | Surface::ToroidalSurface(_) => true,
        Surface::RevolutionSurface(processor) => matches!(
            processor.entity().entity_curve(),
            Curve::BsplineCurve(_) | Curve::NurbsCurve(_)
        ),
        _ => false,
    };
    if !admitted {
        return Ok(None);
    }
    let Some(net) = surface.try_into_homogeneous_bspline_surface() else {
        return Ok(None);
    };
    let count = net
        .control_points()
        .iter()
        .try_fold(0usize, |count, row| count.checked_add(row.len()));
    if !count.is_some_and(|count| count > 0 && count <= MAX_HULL_POINTS) {
        return Err(unsupported(
            "The rational carrier exceeds its control budget",
        ));
    }
    if net
        .control_points()
        .iter()
        .flatten()
        .any(|h| h.w <= 0. || ![h.x, h.y, h.z, h.w].iter().all(|value| value.is_finite()))
    {
        return Err(unsupported(
            "The rational carrier has no finite positive-weight enclosure",
        ));
    }
    Ok(Some(net))
}

pub(crate) fn face_hull(face: &Face) -> Result<Vec<Point3>> {
    match crate::extrusion::face_hull(face) {
        Ok(hull) => return checked(hull),
        Err(CadError::Unsupported(_)) => {}
        Err(error) => return Err(error),
    }
    if let Some(bounds) = certified_face_bounding_box(face) {
        let a = bounds.min();
        let b = bounds.max();
        if a.x > b.x || a.y > b.y || a.z > b.z {
            return Err(unsupported("The certified face box is empty"));
        }
        return checked(
            [a.x, b.x]
                .into_iter()
                .flat_map(|x| {
                    [a.y, b.y].into_iter().flat_map(move |y| {
                        [a.z, b.z].into_iter().map(move |z| Point3::new(x, y, z))
                    })
                })
                .collect(),
        );
    }
    if let Some(net) = subdivision_surface(face)? {
        return checked(
            net.control_points()
                .iter()
                .flatten()
                .map(|h| Point3::new(h.x / h.w, h.y / h.w, h.z / h.w))
                .collect(),
        );
    }
    Err(unsupported(
        "The face has no proven finite native enclosure",
    ))
}

pub(crate) fn solid(solid: &Solid) -> Result<Vec<Enclosure>> {
    let mut result = Vec::new();
    let mut points = 0usize;
    for source in solid.face_iter() {
        let enclosure = face(source)?;
        points = points.saturating_add(enclosure.len());
        if points > MAX_HULL_POINTS {
            return Err(unsupported("The body enclosure exceeds its control budget"));
        }
        result.push(enclosure);
    }
    if result.is_empty() {
        return Err(unsupported("The body enclosure is empty"));
    }
    Ok(result)
}
