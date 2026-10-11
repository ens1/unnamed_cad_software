//! Disposable material-change geometry. Native bodies remain authoritative;
//! tessellated differences are explicitly marked and never enter body caches.
use crate::adapter::{KernelAdapter, MonstertruckAdapter, Shape};
use cad_core::{
    CadError, NodeRef, Result,
    protocol::{Diagnostic, MeshData, OperationPreviewChange, PreviewRepresentation},
};
use monstertruck::modeling::*;
use std::collections::BTreeSet;

const MAX_BODIES: usize = 256;
const MAX_FACES: usize = 4096;
const MAX_BOOLEAN_PAIRS: usize = 256;
const MAX_DISPLAY_TRIANGLES: usize = 65_536;
const MAX_DISPLAY_POINTS: usize = MAX_DISPLAY_TRIANGLES * 3;

fn unsupported(message: impl ToString) -> CadError {
    CadError::Unsupported(format!("Localized preview: {}", message.to_string()))
}

pub(crate) fn warning(owner: Option<&NodeRef>, message: impl Into<String>) -> Diagnostic {
    Diagnostic {
        severity: "warning".into(),
        target: owner.map(|owner| owner.node.clone()),
        message: message.into(),
    }
}

fn face_key(face: &Face) -> Result<Vec<u8>> {
    let mut wires = face
        .boundaries()
        .iter()
        .map(|wire| {
            let mut edges = wire
                .iter()
                .map(|edge| {
                    serde_json::to_vec(&(
                        edge.front().point(),
                        edge.back().point(),
                        edge.oriented_curve(),
                    ))
                    .map_err(CadError::from)
                })
                .collect::<Result<Vec<_>>>()?;
            edges.sort();
            Ok(edges)
        })
        .collect::<Result<Vec<_>>>()?;
    wires.sort();
    Ok(serde_json::to_vec(&(face.oriented_surface(), wires))?)
}

fn solid_key(solid: &Solid) -> Result<Vec<Vec<u8>>> {
    let mut faces = solid
        .face_iter()
        .map(face_key)
        .collect::<Result<Vec<_>>>()?;
    faces.sort();
    Ok(faces)
}

fn admit(before: &[Solid], after: &[Solid]) -> Result<()> {
    if before.len().saturating_add(after.len()) > MAX_BODIES
        || before
            .iter()
            .chain(after)
            .flat_map(Solid::face_iter)
            .take(MAX_FACES + 1)
            .count()
            > MAX_FACES
    {
        return Err(unsupported(
            "display comparison exceeds 256 solids or 4096 faces",
        ));
    }
    Ok(())
}

fn disjoint(a: &Solid, b: &Solid) -> Result<bool> {
    let bounds = |solid: &Solid| -> Result<[[f64; 3]; 2]> {
        let points = crate::adapter::solid_control_points(solid)?;
        let mut bounds = [[f64::INFINITY; 3], [f64::NEG_INFINITY; 3]];
        for point in points {
            for (axis, value) in [point.x, point.y, point.z].into_iter().enumerate() {
                bounds[0][axis] = bounds[0][axis].min(value);
                bounds[1][axis] = bounds[1][axis].max(value);
            }
        }
        Ok(bounds)
    };
    let (a, b) = (bounds(a)?, bounds(b)?);
    Ok((0..3).any(|axis| a[1][axis] <= b[0][axis] || b[1][axis] <= a[0][axis]))
}

fn difference(source: &[Solid], tools: &[Solid], tolerance: f64) -> Result<Vec<Solid>> {
    let mut pairs = 0usize;
    let tool_keys = tools.iter().map(solid_key).collect::<Result<Vec<_>>>()?;
    let mut result = Vec::new();
    for stock in source {
        if tool_keys.contains(&solid_key(stock)?) {
            continue;
        }
        let mut pieces = vec![stock.clone()];
        for (tool, tool_key) in tools.iter().zip(&tool_keys) {
            let mut next = Vec::new();
            for piece in pieces {
                if &solid_key(&piece)? == tool_key {
                    continue;
                }
                if disjoint(&piece, tool).unwrap_or(false) {
                    next.push(piece);
                    continue;
                }
                pairs += 1;
                if pairs > MAX_BOOLEAN_PAIRS {
                    return Err(unsupported("display comparison exceeds 256 Boolean pairs"));
                }
                if let Some(exact) = crate::planar::convex_difference(&piece, tool)? {
                    next.extend(exact.bodies);
                } else {
                    let cut = monstertruck::solid::difference(&piece, tool, tolerance)
                        .map_err(unsupported)?;
                    if !cut.boundaries().is_empty() {
                        crate::validation::solid(&cut)?;
                        next.push(cut);
                    }
                }
                if next.len() > MAX_BODIES {
                    return Err(unsupported(
                        "display comparison exceeds 256 resulting regions",
                    ));
                }
            }
            pieces = next;
        }
        if result.len().saturating_add(pieces.len()) > MAX_BODIES {
            return Err(unsupported(
                "display comparison exceeds 256 resulting regions",
            ));
        }
        result.extend(pieces);
    }
    Ok(result)
}

fn mesh(
    adapter: &MonstertruckAdapter,
    solids: Vec<Solid>,
    owner: &NodeRef,
    tolerance: f64,
) -> Result<Option<MeshData>> {
    if solids.is_empty() {
        return Ok(None);
    }
    admit(&solids, &[])?;
    let mesh = adapter.mesh(&Shape::Bodies(solids), owner, tolerance)?;
    admit_mesh(&mesh)?;
    Ok((!mesh.indices.is_empty()).then_some(mesh))
}

fn admit_mesh(mesh: &MeshData) -> Result<()> {
    let edge_points = mesh
        .edges
        .iter()
        .map(|edge| edge.points.len())
        .sum::<usize>();
    if mesh.indices.len() / 3 > MAX_DISPLAY_TRIANGLES
        || mesh.positions.len().saturating_add(edge_points) > MAX_DISPLAY_POINTS
        || mesh.faces.len() > MAX_FACES
    {
        return Err(unsupported(
            "display cache exceeds 65536 triangles, 196608 points or 4096 faces",
        ));
    }
    Ok(())
}

fn exact_solids(
    adapter: &MonstertruckAdapter,
    before: &[Solid],
    after: &[Solid],
    owner: &NodeRef,
    tolerance: f64,
) -> Result<OperationPreviewChange> {
    let added = difference(after, before, tolerance)?;
    let removed = difference(before, after, tolerance)?;
    Ok(OperationPreviewChange {
        added: mesh(adapter, added, owner, tolerance)?,
        removed: mesh(adapter, removed, owner, tolerance)?,
        diagnostics: Vec::new(),
        representation: PreviewRepresentation::ExactBrep,
    })
}

fn surfaces(
    adapter: &MonstertruckAdapter,
    before: &[Shell],
    after: &[Shell],
    owner: &NodeRef,
    tolerance: f64,
) -> Result<OperationPreviewChange> {
    if before.len().saturating_add(after.len()) > MAX_BODIES
        || before
            .iter()
            .chain(after)
            .flat_map(|shell| shell.iter())
            .take(MAX_FACES + 1)
            .count()
            > MAX_FACES
    {
        return Err(unsupported(
            "surface display comparison exceeds 256 shells or 4096 faces",
        ));
    }
    let keys = |shells: &[Shell]| -> Result<BTreeSet<Vec<u8>>> {
        shells
            .iter()
            .flat_map(|shell| shell.iter())
            .map(face_key)
            .collect()
    };
    let (before_keys, after_keys) = (keys(before)?, keys(after)?);
    let added_faces = after
        .iter()
        .flat_map(|shell| shell.iter())
        .filter_map(|face| match face_key(face) {
            Ok(key) => (!before_keys.contains(&key)).then_some(Ok(face.clone())),
            Err(error) => Some(Err(error)),
        })
        .collect::<Result<Vec<_>>>()?;
    let removed_faces = before
        .iter()
        .flat_map(|shell| shell.iter())
        .filter_map(|face| match face_key(face) {
            Ok(key) => (!after_keys.contains(&key)).then_some(Ok(face.clone())),
            Err(error) => Some(Err(error)),
        })
        .collect::<Result<Vec<_>>>()?;
    // Changed overlapping surface carriers need exact 2D trim differences. Do
    // not present two complete overlapping faces as a material-volume preview.
    if !added_faces.is_empty() && !removed_faces.is_empty() {
        return Err(unsupported(
            "changed surface trims need localized surface-region comparison; the current surface result is displayed neutrally",
        ));
    }
    let display = |faces: Vec<Face>| -> Result<Option<MeshData>> {
        if faces.is_empty() {
            return Ok(None);
        }
        let mesh = adapter.mesh(&Shape::Surfaces(vec![Shell::from(faces)]), owner, tolerance)?;
        admit_mesh(&mesh)?;
        Ok(Some(mesh))
    };
    Ok(OperationPreviewChange {
        added: display(added_faces)?,
        removed: display(removed_faces)?,
        diagnostics: Vec::new(),
        representation: PreviewRepresentation::ExactBrep,
    })
}

pub(crate) fn delta(
    adapter: &MonstertruckAdapter,
    before: &Shape,
    after: &Shape,
    owner: &NodeRef,
    tolerance: f64,
) -> Result<OperationPreviewChange> {
    match (before, after) {
        (Shape::Bodies(before), Shape::Bodies(after)) => {
            admit(before, after)?;
            let exact = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                exact_solids(adapter, before, after, owner, tolerance)
            }));
            match exact {
                Ok(Ok(change)) => Ok(change),
                failed => {
                    let reason = match failed {
                        Ok(Err(error)) => error.to_string(),
                        Err(_) => "native display comparison failed internally".into(),
                        Ok(Ok(_)) => unreachable!(),
                    };
                    let before = adapter.mesh(&Shape::Bodies(before.clone()), owner, tolerance)?;
                    let after = adapter.mesh(&Shape::Bodies(after.clone()), owner, tolerance)?;
                    let (added, removed) = crate::preview_csg::difference(&before, &after, owner)?;
                    Ok(OperationPreviewChange {
                        added,
                        removed,
                        representation: PreviewRepresentation::Tessellated,
                        diagnostics: vec![warning(
                            Some(owner),
                            format!(
                                "Approximate operation highlight from display tessellation at {tolerance:.6} mm; exact localized B-rep comparison was unavailable: {reason}"
                            ),
                        )],
                    })
                }
            }
        }
        (Shape::Surfaces(before), Shape::Surfaces(after)) => {
            surfaces(adapter, before, after, owner, tolerance)
        }
        (Shape::Bodies(before), Shape::Surfaces(after)) if before.is_empty() => {
            surfaces(adapter, &[], after, owner, tolerance)
        }
        (Shape::Surfaces(before), Shape::Bodies(after)) if after.is_empty() => {
            surfaces(adapter, before, &[], owner, tolerance)
        }
        (Shape::Surfaces(_), Shape::Bodies(after)) => {
            admit(&[], after)?;
            exact_solids(adapter, &[], after, owner, tolerance)
        }
        _ => Err(unsupported(
            "this feature changes body kind or does not produce solid/surface geometry",
        )),
    }
}
