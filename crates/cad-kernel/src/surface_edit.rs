//! Exact surface boundary construction and surgery. Tessellation is never input.
use crate::adapter::{MonstertruckAdapter, Shape};
use cad_core::{
    CadError, Dimension, Document, NodeRef, Result, expression, surface::SurfaceOperation,
};
use monstertruck::geometry::prelude::TryIntoHomogeneousBsplineSurface;
use monstertruck::modeling::*;
use std::collections::BTreeMap;

fn invalid(message: impl std::fmt::Display) -> CadError {
    CadError::Invalid(format!("Surface edit: {message}"))
}
fn unsupported(message: &str) -> CadError {
    CadError::Unsupported(message.into())
}
const EPS: f64 = 1e-7;

pub(crate) fn extrudable_edges(edges: impl IntoIterator<Item = Edge>) -> Result<()> {
    // Native extrusion cannot sweep an implicit intersection or parameter
    // curve. Its guide is not an exact substitute for the actual boundary.
    if edges.into_iter().any(|e| {
        !matches!(
            e.curve(),
            Curve::Line(_) | Curve::BsplineCurve(_) | Curve::NurbsCurve(_)
        )
    }) {
        return Err(unsupported(
            "Exact surface extrusion currently requires explicit line/B-spline/NURBS boundaries; implicit intersection and parameter curves cannot be replaced by their guides",
        ));
    }
    Ok(())
}

pub(crate) fn thicken_planar(shell: &Shell, delta: Vector3) -> Result<Vec<Solid>> {
    extrudable_edges(shell.edge_iter())?;
    let source = if shell[0].oriented_surface().normal(0., 0.).dot(delta) < 0. {
        shell.inverse().into()
    } else {
        shell.clone()
    };
    let solids = builder::extrude::<_, Vec<_>>(&source, delta);
    solids.into_iter().map(|s| s.map_err(invalid)).collect()
}

fn rational_surface(face: &Face) -> Result<NurbsSurface<Vector4>> {
    if matches!(face.surface(), Surface::Plane(_)) {
        return Err(unsupported(
            "An infinite plane has no natural finite domain; use a closed sketch to trim or straight boundaries to extend it",
        ));
    }
    let net = face
        .oriented_surface()
        .try_into_homogeneous_bspline_surface()
        .ok_or_else(|| {
            unsupported("This carrier cannot be represented by an exact finite rational patch")
        })?;
    if net.control_points().iter().flatten().any(|p| {
        !p.x.is_finite() || !p.y.is_finite() || !p.z.is_finite() || !p.w.is_finite() || p.w <= 0.
    }) {
        return Err(unsupported(
            "Carrier-domain editing requires finite positive rational weights",
        ));
    }
    Ok(NurbsSurface::new(net))
}

pub(crate) fn patch_face(surface: NurbsSurface<Vector4>) -> Result<Face> {
    let curves = surface.splitted_boundary();
    let vertices: Vec<_> = curves
        .iter()
        .map(|c| builder::vertex(c.subs(c.range_tuple().0)))
        .collect();
    for i in 0..4 {
        if (curves[i].subs(curves[i].range_tuple().1) - vertices[(i + 1) % 4].point()).magnitude()
            > EPS
        {
            return Err(invalid("Natural carrier boundaries do not meet"));
        }
        if (vertices[i].point() - vertices[(i + 1) % 4].point()).magnitude() < 1e-9 {
            return Err(unsupported(
                "Carrier-domain editing does not yet handle collapsed patch sides or periodic seams",
            ));
        }
    }
    let wire: Wire = curves
        .into_iter()
        .enumerate()
        .map(|(i, c)| Edge::new(&vertices[i], &vertices[(i + 1) % 4], Curve::NurbsCurve(c)))
        .collect();
    Face::try_new(vec![wire], Surface::NurbsSurface(surface)).map_err(invalid)
}

fn natural_rectangle(face: &Face, patch: &Face) -> bool {
    let original = face.boundaries();
    let native = patch.boundaries();
    original.len() == 1
        && original[0].len() == 4
        && (0..4).any(|shift| {
            (0..4).all(|i| {
                crate::surfaces::same_curve(
                    &original[0][i].oriented_curve(),
                    &native[0][(i + shift) % 4].oriented_curve(),
                    EPS,
                )
            })
        })
}

fn planar_trim(face: &Face, cutters: &[Face], keep_inside: bool) -> Result<Vec<Face>> {
    if !matches!(face.surface(), Surface::Plane(_)) {
        return Err(unsupported(
            "Sketch Trim currently requires a planar surface; use exact parameter-domain trim for rectangular rational patches",
        ));
    }
    extrudable_edges(face.edge_iter())?;
    let carrier = face.oriented_surface();
    let normal = carrier.normal(0., 0.).normalize();
    let origin = carrier.subs(0., 0.);
    let mut stock = crate::adapter::oriented_extrude(face, normal);
    if keep_inside && cutters.len() != 1 {
        return Err(unsupported(
            "Keep-inside trim currently requires one closed sketch region; remove-inside supports multiple regions",
        ));
    }
    for cutter in cutters {
        extrudable_edges(cutter.edge_iter())?;
        if !matches!(cutter.surface(), Surface::Plane(_))
            || cutter
                .vertex_iter()
                .any(|v| (v.point() - origin).dot(normal).abs() > EPS)
            || cutter
                .oriented_surface()
                .normal(0., 0.)
                .normalize()
                .dot(normal)
                .abs()
                < 1. - 1e-10
        {
            return Err(invalid(
                "Trim sketch must lie on the selected planar surface",
            ));
        }
        let tool =
            crate::adapter::oriented_extrude(&builder::translated(cutter, -normal), normal * 3.);
        stock = if keep_inside {
            monstertruck::solid::and(&stock, &tool, 1e-7)
        } else {
            monstertruck::solid::difference(&stock, &tool, 1e-7)
        }
        .map_err(invalid)?;
    }
    let out: Vec<_> = stock
        .face_iter()
        .filter(|f| {
            matches!(f.surface(), Surface::Plane(_))
                && (f.oriented_surface().subs(0., 0.) - origin)
                    .dot(normal)
                    .abs()
                    <= EPS
                && f.oriented_surface()
                    .normal(0., 0.)
                    .normalize()
                    .dot(normal)
                    .abs()
                    > 1. - 1e-10
        })
        .map(|f| {
            if f.oriented_surface().normal(0., 0.).dot(normal) < 0. {
                f.inverse()
            } else {
                f.clone()
            }
        })
        .collect();
    if out.is_empty() {
        return Err(invalid("Trim removes the entire surface"));
    }
    Ok(out)
}

fn extend(face: &Face, distance: f64, selected: &[Edge]) -> Result<Face> {
    let boundaries = face.boundaries();
    if !matches!(face.surface(), Surface::Plane(_))
        || boundaries.len() != 1
        || boundaries[0].len() < 3
        || boundaries[0]
            .iter()
            .any(|e| !matches!(e.curve(), Curve::Line(_)))
    {
        return Err(unsupported(
            "Extend currently requires a single convex planar face bounded by straight edges",
        ));
    }
    let wire = &boundaries[0];
    let normal = face.oriented_surface().normal(0., 0.).normalize();
    let centroid = wire
        .iter()
        .fold(Vector3::zero(), |a, e| a + e.front().point().to_vec())
        / wire.len() as f64;
    let mut planes = Vec::new();
    for edge in wire {
        let p = edge.front().point();
        let d = (edge.back().point() - p).normalize();
        let mut n = d.cross(normal);
        if (centroid - p.to_vec()).dot(n) > 0. {
            n = -n;
        }
        if wire.iter().any(|e| (e.front().point() - p).dot(n) > EPS) {
            return Err(unsupported("Extend requires a convex boundary"));
        }
        let shift = if selected.is_empty() || selected.iter().any(|s| s.id() == edge.id()) {
            distance
        } else {
            0.
        };
        planes.push((p + n * shift, n, d));
    }
    let mut vertices = Vec::new();
    for i in 0..planes.len() {
        let (p, n, _) = planes[(i + planes.len() - 1) % planes.len()];
        let (q, m, d) = planes[i];
        let denominator = d.dot(n);
        if denominator.abs() < 1e-12 {
            return Err(invalid("Adjacent extension edges are parallel"));
        }
        let point = q + d * ((p - q).dot(n) / denominator);
        if !point.x.is_finite()
            || !point.y.is_finite()
            || !point.z.is_finite()
            || planes.iter().any(|(p, n, _)| (point - *p).dot(*n) > EPS)
            || (point - q).dot(m).abs() > EPS
        {
            return Err(invalid(
                "Extension collapses or crosses the original boundary",
            ));
        }
        vertices.push(builder::vertex(point));
    }
    let wire: Wire = (0..vertices.len())
        .map(|i| builder::line(&vertices[i], &vertices[(i + 1) % vertices.len()]))
        .collect();
    Face::try_new(vec![wire], face.oriented_surface()).map_err(invalid)
}

pub(crate) fn operation(
    adapter: &MonstertruckAdapter,
    doc: &Document,
    owner: &NodeRef,
    operation: &SurfaceOperation,
    inputs: &BTreeMap<NodeRef, Shape>,
) -> Result<Shape> {
    let mut shells = Vec::new();
    match operation {
        SurfaceOperation::Trim {
            surface,
            profile,
            keep_inside,
        } => {
            let cutters = crate::surfaces::planar_faces(inputs, profile)?;
            for shell in crate::surfaces::surfaces(inputs, surface)? {
                for face in shell {
                    for trimmed in planar_trim(face, &cutters, *keep_inside)? {
                        shells.push(vec![trimmed].into());
                    }
                }
            }
        }
        SurfaceOperation::Untrim { surface } => {
            for shell in crate::surfaces::surfaces(inputs, surface)? {
                for face in shell {
                    shells.push(vec![patch_face(rational_surface(face)?)?].into());
                }
            }
        }
        SurfaceOperation::TrimDomain { surface, u, v } => {
            let evaluate = |e| expression::value(doc, &owner.definition, e, Dimension::Scalar);
            let u = [evaluate(&u[0])?, evaluate(&u[1])?];
            let v = [evaluate(&v[0])?, evaluate(&v[1])?];
            if !(0. <= u[0] && u[0] < u[1] && u[1] <= 1. && 0. <= v[0] && v[0] < v[1] && v[1] <= 1.)
            {
                return Err(invalid("Parameter intervals must increase within 0–1"));
            }
            for shell in crate::surfaces::surfaces(inputs, surface)? {
                for face in shell {
                    let mut carrier = rational_surface(face)?;
                    if !natural_rectangle(face, &patch_face(carrier.clone())?) {
                        return Err(unsupported(
                            "Parameter trim requires a natural four-sided patch; Untrim explicitly restores its carrier first",
                        ));
                    }
                    carrier.knot_normalize();
                    if u[0] > 0. {
                        carrier = carrier.cut_u(u[0]);
                    }
                    if u[1] < 1. {
                        carrier.cut_u(u[1]);
                    }
                    if v[0] > 0. {
                        carrier = carrier.cut_v(v[0]);
                    }
                    if v[1] < 1. {
                        carrier.cut_v(v[1]);
                    }
                    let trimmed = patch_face(carrier)?;
                    // Knot insertion derives exact trimming boundaries. Retain the
                    // source carrier/domain so Untrim can recover its full extent.
                    let face = Face::try_new(trimmed.boundaries(), face.oriented_surface())
                        .map_err(invalid)?;
                    shells.push(vec![face].into());
                }
            }
        }
        SurfaceOperation::Extend {
            surface,
            distance,
            edges,
        } => {
            let distance = expression::value(doc, &owner.definition, distance, Dimension::Length)?;
            if distance <= 1e-9 {
                return Err(invalid("Extension distance must be positive"));
            }
            let selected = if edges.is_empty() {
                vec![]
            } else {
                adapter.selected_boundary_edges(surface, edges, inputs)?
            };
            let source = crate::surfaces::surfaces(inputs, surface)?;
            if !edges.is_empty() && (source.len() != 1 || source[0].len() != 1) {
                return Err(unsupported(
                    "Selected-edge extension requires one independent face",
                ));
            }
            for shell in source {
                if shell.len() != 1 {
                    return Err(unsupported(
                        "Extend requires independent planar face shells",
                    ));
                }
                shells.push(vec![extend(&shell[0], distance, &selected)?].into());
            }
        }
        _ => return Err(invalid("Unexpected surface operation")),
    }
    Ok(Shape::Surfaces(shells))
}
