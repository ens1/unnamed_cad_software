//! Associative mold reports and exact tool geometry built from saved source intent.
use crate::adapter::{MonstertruckAdapter, Shape};
use cad_core::{
    CadError, Dimension, Document, NodeRef, Result, expression,
    mold::{DraftClass, DraftReport, FaceDraft, MoldOperation, ToolingSide},
};
use monstertruck::modeling::*;
use std::{
    collections::{BTreeMap, HashMap},
    f64::consts::FRAC_PI_2,
};

fn invalid(message: impl std::fmt::Display) -> CadError {
    CadError::Invalid(format!("Mold: {message}"))
}
fn unsupported(message: &str) -> CadError {
    CadError::Unsupported(message.into())
}
fn vector(p: [f64; 3]) -> Result<Vector3> {
    if p.iter().any(|v| !v.is_finite()) {
        return Err(invalid("Direction must be finite"));
    }
    let v = Vector3::new(p[0], p[1], p[2]);
    let length = v.magnitude();
    if !length.is_finite() || length < 1e-9 {
        return Err(invalid("Direction must have a finite nonzero magnitude"));
    }
    Ok(v / length)
}
fn bodies<'a>(inputs: &'a BTreeMap<NodeRef, Shape>, source: &NodeRef) -> Result<&'a [Solid]> {
    match inputs.get(source) {
        Some(Shape::Bodies(b)) if !b.is_empty() => Ok(b),
        _ => Err(invalid("Select a solid source body")),
    }
}

fn assessment(
    adapter: &MonstertruckAdapter,
    owner: &NodeRef,
    source: &NodeRef,
    solids: &[Solid],
    pull: Vector3,
    minimum: f64,
) -> Result<DraftReport> {
    let mut faces = Vec::new();
    let mut classes = HashMap::new();
    let mut edge_faces: HashMap<EdgeId, Vec<FaceId>> = HashMap::new();
    let mut unique_edges = HashMap::new();
    for solid in solids {
        for face in solid.face_iter() {
            let native = face.oriented_surface();
            let angle = if matches!(native, Surface::Plane(_)) {
                let normal = vector({
                    let n = native.normal(0., 0.);
                    [n.x, n.y, n.z]
                })?;
                Some(normal.dot(pull).clamp(-1., 1.).asin())
            } else {
                None
            };
            let class = match angle {
                None => DraftClass::Unassessed,
                Some(a) if a.abs() <= 1e-10 => DraftClass::Neutral,
                Some(a) if a.abs() + 1e-10 < minimum => DraftClass::Insufficient,
                Some(a) if a > 0. => DraftClass::Positive,
                _ => DraftClass::Negative,
            };
            classes.insert(
                face.id(),
                angle.map(|a| {
                    if a > 1e-10 {
                        1
                    } else if a < -1e-10 {
                        -1
                    } else {
                        0
                    }
                }),
            );
            for edge in face.edge_iter() {
                edge_faces.entry(edge.id()).or_default().push(face.id());
                unique_edges.insert(edge.id(), edge);
            }
            faces.push(FaceDraft {face:adapter.boundary_face_reference(source,face)?, class, angle_radians:angle, reason: angle.is_none().then(|| "Curved-face draft extrema require exact normal bounds and are not classified by tessellation".into())});
        }
    }
    let mut parting_edges = Vec::new();
    for (id, adjacent) in edge_faces {
        if adjacent.len() == 2
            && matches!(
                (classes[&adjacent[0]], classes[&adjacent[1]]),
                (Some(1), Some(-1)) | (Some(-1), Some(1))
            )
        {
            parting_edges.push(adapter.boundary_edge_reference(source, &unique_edges[&id])?);
        }
    }
    parting_edges.sort_by(|a, b| a.producer.cmp(&b.producer).then(a.role.cmp(&b.role)));
    Ok(DraftReport {
        version: cad_core::mold::CONTRACT_VERSION,
        producer: owner.clone(),
        source: source.clone(),
        pull: [pull.x, pull.y, pull.z],
        minimum_radians: minimum,
        complete: faces.iter().all(|f| f.angle_radians.is_some()),
        faces,
        parting_edges,
    })
}

fn closed_loops(mut edges: Vec<Edge>) -> Result<Vec<Wire>> {
    if edges.is_empty() {
        return Err(invalid("Select the closed boundary edges to shut off"));
    }
    let mut degrees = HashMap::new();
    for edge in &edges {
        *degrees.entry(edge.front().id()).or_insert(0usize) += 1;
        *degrees.entry(edge.back().id()).or_insert(0usize) += 1;
    }
    if degrees.values().any(|d| *d != 2) {
        return Err(invalid(
            "Shut-off boundaries must form unbranched closed loops",
        ));
    }
    let mut loops = Vec::new();
    while let Some(first) = edges.pop() {
        let begin = first.front().id();
        let mut end = first.back().id();
        let mut wire = Wire::from(vec![first]);
        while end != begin {
            let Some(i) = edges
                .iter()
                .position(|e| e.front().id() == end || e.back().id() == end)
            else {
                return Err(invalid("Shut-off boundary cannot be joined"));
            };
            let edge = edges.remove(i);
            let edge = if edge.front().id() == end {
                edge
            } else {
                edge.inverse()
            };
            end = edge.back().id();
            wire.push_back(edge);
        }
        loops.push(wire);
    }
    Ok(loops)
}

pub(crate) fn operation(
    adapter: &MonstertruckAdapter,
    doc: &Document,
    owner: &NodeRef,
    operation: &MoldOperation,
    inputs: &BTreeMap<NodeRef, Shape>,
    tolerance: f64,
) -> Result<Shape> {
    let length = |e| expression::value(doc, &owner.definition, e, Dimension::Length);
    let result = match operation {
        MoldOperation::DraftAnalysis {
            body,
            pull,
            minimum,
            reverse,
        } => {
            let minimum = expression::value(doc, &owner.definition, minimum, Dimension::Angle)?;
            if !(0. ..FRAC_PI_2).contains(&minimum) {
                return Err(invalid("Minimum draft must be between 0 and 90 degrees"));
            }
            let axis = adapter.resolve_surface_axis(doc, pull, inputs)?;
            let pull = vector(axis.direction)? * if *reverse { -1. } else { 1. };
            Shape::Analysis(assessment(
                adapter,
                owner,
                body,
                bodies(inputs, body)?,
                pull,
                minimum,
            )?)
        }
        MoldOperation::PartingSurface {
            body,
            edges,
            pull,
            direction,
            width,
            reverse,
        } => {
            let pull = vector(adapter.resolve_surface_axis(doc, pull, inputs)?.direction)?;
            let direction = vector(*direction)?;
            if pull.dot(direction).abs() > 1e-8 {
                return Err(invalid(
                    "Parting surface extension direction must be perpendicular to the pull axis",
                ));
            }
            let width = length(width)?;
            if width <= 1e-9 {
                return Err(invalid("Parting surface width must be positive"));
            }
            let selected = adapter.selected_boundary_edges(body, edges, inputs)?;
            if selected.is_empty() {
                return Err(invalid(
                    "Select parting edges or use exact analysis adjacency",
                ));
            }
            crate::surface_edit::extrudable_edges(selected.iter().cloned())?;
            let mut shells = Vec::new();
            for edge in selected {
                // A ruled strip must not collapse when its source edge runs
                // in the extension direction. Its surface is exact otherwise.
                let curve = edge.oriented_curve();
                let (a, b) = curve.range_tuple();
                if matches!(curve, Curve::Line(_))
                    && curve.der((a + b) * 0.5).cross(direction).magnitude2() < 1e-18
                {
                    return Err(invalid(
                        "A selected parting edge is parallel to the extension direction",
                    ));
                }
                let shell: Shell = builder::extrude(&Wire::from(vec![edge]), direction * width);
                shells.push(if *reverse {
                    shell.inverse().into()
                } else {
                    shell
                });
            }
            Shape::Surfaces(shells)
        }
        MoldOperation::ShutoffSurface {
            body,
            edges,
            reverse,
        } => {
            let mut shells = Vec::new();
            for wire in closed_loops(adapter.selected_boundary_edges(body, edges, inputs)?)? {
                let face = builder::try_attach_plane(vec![wire]).map_err(|e| {
                    unsupported(&format!(
                        "Shut-off requires a coplanar closed exact boundary: {e}"
                    ))
                })?;
                shells.push(vec![if *reverse { face.inverse() } else { face }].into());
            }
            Shape::Surfaces(shells)
        }
        MoldOperation::Shrink {
            body,
            factors,
            center,
        } => {
            let options = cad_core::body_editing::ScaleOptions {
                uniform: false,
                factors: factors.clone(),
                center: center.clone(),
                keep_original: false,
            };
            let intent = adapter.resolve_body_scale(doc, &owner.definition, &options, inputs)?;
            let transform = crate::adapter::matrix(intent.matrix);
            if transform.invert().is_none() {
                return Err(invalid("Shrink transform is numerically singular"));
            }
            for vertex in bodies(inputs, body)?.iter().flat_map(|s| s.vertex_iter()) {
                let p = transform.transform_point(vertex.point());
                if ![p.x, p.y, p.z].iter().all(|x| x.is_finite()) {
                    return Err(invalid("Shrunk geometry exceeds finite coordinates"));
                }
            }
            Shape::Bodies(
                bodies(inputs, body)?
                    .iter()
                    .map(|s| builder::transformed(s, transform))
                    .collect(),
            )
        }
        MoldOperation::ToolingSplit {
            body,
            stock,
            depth,
            parting,
            side,
        } => {
            let source = bodies(inputs, body)?;
            let profiles = crate::surfaces::planar_faces(inputs, stock)?;
            let depth = length(depth)?;
            if depth <= 1e-9 {
                return Err(invalid("Tooling stock depth must be positive"));
            }
            let plane = adapter.resolve_surface_plane(doc, parting, inputs)?;
            let u = vector(plane.u)?;
            let v = vector(plane.v)?;
            let normal = u.cross(v).normalize();
            let origin = Point3::new(plane.origin[0], plane.origin[1], plane.origin[2]);
            let mut tools = Vec::new();
            for profile in profiles {
                crate::surface_edit::extrudable_edges(profile.edge_iter())?;
                if !matches!(profile.surface(), Surface::Plane(_)) {
                    return Err(unsupported(
                        "Tooling stock must come from planar closed sketch regions",
                    ));
                }
                let n = profile.oriented_surface().normal(0., 0.).normalize();
                if n.dot(normal).abs() < 1. - 1e-10 {
                    return Err(unsupported(
                        "Tooling Split currently requires a planar parting reference parallel to the stock sketch",
                    ));
                }
                let start = profile.oriented_surface().subs(0., 0.);
                let offset = (origin - start).dot(normal);
                if offset.abs() >= depth * 0.5 - 1e-9 {
                    return Err(invalid(
                        "Parting plane must lie strictly inside the stock depth",
                    ));
                }
                let middle = builder::translated(&profile, normal * offset);
                for (enabled, delta) in [
                    (
                        matches!(side, ToolingSide::Core | ToolingSide::Both),
                        -normal * (depth * 0.5 + offset),
                    ),
                    (
                        matches!(side, ToolingSide::Cavity | ToolingSide::Both),
                        normal * (depth * 0.5 - offset),
                    ),
                ] {
                    if enabled {
                        let mut block = crate::adapter::oriented_extrude(&middle, delta);
                        for part in source {
                            block = monstertruck::solid::difference(&block, part, tolerance)
                                .map_err(invalid)?;
                        }
                        tools.push(block);
                    }
                }
            }
            Shape::Bodies(tools)
        }
    };
    if let Shape::Surfaces(shells) = &result {
        for shell in shells {
            crate::validation::surface(shell)?;
        }
    }
    Ok(result)
}
