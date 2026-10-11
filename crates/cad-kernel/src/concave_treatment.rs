//! An enclosed or planar-entry convex void is the exact material dual of a
//! convex solid. Rounding/chamfering its edges removes void and adds material
//! to concave pocket corners. The temporary cap is removed again, and a changed
//! entry wire is shared with the actual stock cap.
use super::Treatment;
use crate::{planar::ChamferSpec, shell_history::ShellHistory};
use cad_core::{CadError, Result};
use monstertruck::modeling::*;
use std::collections::{HashMap, HashSet};
const EPS: f64 = 1e-7;
fn invalid(message: &str) -> CadError {
    CadError::Invalid(format!("Pocket edge treatment: {message}"))
}
fn operate(
    void: &Solid,
    selected: &[EdgeId],
    spec: &ChamferSpec,
    treatment: Treatment,
    history: &mut ShellHistory,
) -> Result<Option<Solid>> {
    match treatment {
        Treatment::Fillet => crate::edge_round::round(void, selected, spec.distance, history),
        Treatment::Chamfer => {
            // Inverting both source normals reverses their lexical default
            // order. Preserve Distance 1's source-face meaning through duality.
            let spec = ChamferSpec {
                distance: spec.distance,
                second_distance: spec.second_distance,
                angle: spec.angle,
                flip: spec.flip ^ spec.first_face.is_none(),
                first_face: spec.first_face,
            };
            crate::planar::chamfer(void, selected, &spec, history)
        }
    }
}
fn protect_other_components(
    source: &Solid,
    void: &Solid,
    selected: &[EdgeId],
    ignored: &HashSet<FaceId>,
    spec: &ChamferSpec,
    treatment: Treatment,
    work: &mut usize,
) -> Result<()> {
    let planes = crate::planar::supporting_planes(void)?;
    if planes.len() != void.face_iter().count() || planes.len() < 4 {
        return Err(CadError::Unsupported(
            "Pocket duality requires a proven convex planar material void".into(),
        ));
    }
    for id in selected {
        let adjacent = planes
            .iter()
            .filter(|plane| plane.face.edge_iter().any(|edge| edge.id() == *id))
            .collect::<Vec<_>>();
        let [a, b] = adjacent.as_slice() else {
            return Err(invalid(
                "A selected void edge has ambiguous supporting planes",
            ));
        };
        let depth = match treatment {
            // Every cylinder strip remains within 2r of its supporting planes;
            // a trihedral corner ball has center distance r and radius r.
            Treatment::Fillet => 2. * spec.distance,
            Treatment::Chamfer => {
                let second = if let Some(angle) = spec.angle {
                    let interior = (-a.normal.dot(b.normal)).clamp(-1., 1.).acos();
                    spec.distance * angle.sin() / (interior + angle).sin()
                } else {
                    spec.second_distance.unwrap_or(spec.distance)
                };
                spec.distance.max(second)
            }
        };
        if !depth.is_finite() || depth <= EPS {
            return Err(invalid(
                "The changed void region has an invalid finite bound",
            ));
        }
        let mut bounds = planes
            .iter()
            .map(|plane| (plane.origin, plane.normal))
            .collect::<Vec<_>>();
        for plane in [a, b] {
            bounds.push((plane.origin - plane.normal * depth, -plane.normal));
        }
        for face in source
            .face_iter()
            .filter(|face| !ignored.contains(&face.id()))
        {
            if !super::local_blend::face_outside(face, &bounds, work)? {
                return Err(CadError::Unsupported("Pocket edge treatment may reach another shell or material island; reduce the size".into()));
            }
        }
    }
    Ok(())
}
pub(super) fn construct(
    solid: &Solid,
    selected: &[EdgeId],
    spec: &ChamferSpec,
    treatment: Treatment,
    history: &mut ShellHistory,
) -> Result<Option<Solid>> {
    let mut pending = selected.to_vec();
    let mut current = solid.clone();
    let mut work = 0usize;
    if pending.is_empty() || pending.len() > cad_core::edge_features::MAX_SELECTED_EDGES {
        return Ok(None);
    }
    while !pending.is_empty() {
        let Some((output, handled)) =
            single(&current, &pending, spec, treatment, history, &mut work)?
        else {
            return Ok(None);
        };
        if handled.is_empty() {
            return Err(invalid(
                "A pocket treatment did not consume a selected edge",
            ));
        }
        pending.retain(|id| !handled.contains(id));
        current = output;
    }
    Ok(Some(current))
}

fn single(
    solid: &Solid,
    selected: &[EdgeId],
    spec: &ChamferSpec,
    treatment: Treatment,
    history: &mut ShellHistory,
    work: &mut usize,
) -> Result<Option<(Solid, Vec<EdgeId>)>> {
    if selected.is_empty() {
        return Ok(None);
    }
    let selected_set = selected.iter().copied().collect::<HashSet<_>>();
    // Enclosed cavities already have exactly the complete temporary solid.
    for (index, shell) in solid.boundaries().iter().enumerate() {
        let source_edges = shell
            .edge_iter()
            .map(|edge| edge.id())
            .collect::<HashSet<_>>();
        let local = selected
            .iter()
            .filter(|id| source_edges.contains(id))
            .copied()
            .collect::<Vec<_>>();
        if local.is_empty() {
            continue;
        }
        let void = Solid::try_new(vec![shell.inverse().into()])
            .map_err(|_| invalid("The enclosed void is not an oriented closed shell"))?;
        let Ok(planes) = crate::planar::supporting_planes(&void) else {
            continue;
        };
        if planes.len() < 4 || planes.len() != void.face_iter().count() {
            continue;
        }
        let ignored = shell.iter().map(|face| face.id()).collect::<HashSet<_>>();
        protect_other_components(solid, &void, &local, &ignored, spec, treatment, work)?;
        let Some(rounded) = operate(&void, &local, spec, treatment, history)? else {
            return Ok(None);
        };
        if rounded.boundaries().len() != 1 {
            return Err(invalid("A cavity treatment unexpectedly split its void"));
        }
        let mut shells = solid.boundaries().to_vec();
        shells[index] = rounded.boundaries()[0].inverse().into();
        let result =
            Solid::try_new(shells).map_err(|_| invalid("The treated cavity is not closed"))?;
        crate::validation::solid(&result)?;
        return Ok(Some((result, local)));
    }
    let faces = solid
        .face_iter()
        .map(|face| (face.id(), face.clone()))
        .collect::<HashMap<_, _>>();
    if faces.len() > 4096 {
        return Ok(None);
    }
    let mut adjacent: HashMap<EdgeId, Vec<FaceId>> = HashMap::new();
    for face in solid.face_iter() {
        for edge in face.edge_iter() {
            adjacent.entry(edge.id()).or_default().push(face.id());
        }
    }
    for cap in solid.face_iter() {
        let Some(plane) = crate::planar::boundary_plane(cap)? else {
            continue;
        };
        let wires = cap.boundaries();
        for (opening_index, opening) in wires.iter().enumerate().skip(1) {
            let opening_edges = opening.iter().map(|edge| edge.id()).collect::<HashSet<_>>();
            // A rim is a real convex stock edge, not an internal void edge.
            // It must use the actual pair of adjacent stock carriers.
            if !selected_set.is_disjoint(&opening_edges) {
                continue;
            }
            let mut component = HashSet::new();
            let mut pending = Vec::new();
            for edge in opening {
                for face in &adjacent[&edge.id()] {
                    if *face != cap.id() {
                        pending.push(*face);
                    }
                }
            }
            while let Some(id) = pending.pop() {
                *work = work.saturating_add(1);
                if *work > 4_000_000 {
                    return Err(CadError::Unsupported(
                        "Pocket edge treatment exceeds its connectivity work budget".into(),
                    ));
                }
                if !component.insert(id) {
                    continue;
                }
                for edge in faces[&id].edge_iter() {
                    for neighbour in &adjacent[&edge.id()] {
                        if *neighbour != cap.id() && !component.contains(neighbour) {
                            pending.push(*neighbour);
                        }
                    }
                }
            }
            let component_edges = component
                .iter()
                .flat_map(|id| faces[id].edge_iter().map(|edge| edge.id()))
                .collect::<HashSet<_>>();
            let local = selected
                .iter()
                .filter(|id| component_edges.contains(id))
                .copied()
                .collect::<Vec<_>>();
            if local.is_empty() {
                continue;
            }
            // The component must close at this one inner wire, not another
            // opening or any segment of the stock's external silhouette.
            let open_edges = component_edges
                .iter()
                .filter(|id| adjacent[id].iter().any(|face| *face == cap.id()))
                .copied()
                .collect::<HashSet<_>>();
            if open_edges != opening_edges {
                continue;
            }
            let closure = builder::try_attach_plane(vec![opening.inverse()])
                .map_err(|_| invalid("The pocket opening cannot form an exact planar cap"))?;
            let mut shell = Shell::new();
            for face in solid
                .face_iter()
                .filter(|face| component.contains(&face.id()))
            {
                shell.push(face.inverse());
            }
            shell.push(closure.clone());
            let void = match Solid::try_new(vec![shell]) {
                Ok(void) => void,
                Err(_) => continue,
            };
            // Restrict the duality pass to convex planar bounded voids. This
            // establishes that every removed void wedge remains inside the
            // original pocket and cannot cross material elsewhere in the part.
            let Ok(planes) = crate::planar::supporting_planes(&void) else {
                continue;
            };
            if planes.len() != void.face_iter().count() {
                continue;
            }
            let mut sources = vec![history.face(cap.id())?];
            sources.extend(
                opening
                    .iter()
                    .map(|edge| history.edge(edge.id()))
                    .collect::<Result<Vec<_>>>()?,
            );
            history.construct_face(closure.id(),&sources,serde_json::json!({"operation":treatment.name(),"constructor":"temporary_pocket_dual_cap"}))?;
            let mut ignored = component.clone();
            ignored.insert(cap.id());
            protect_other_components(solid, &void, &local, &ignored, spec, treatment, work)?;
            let Some(treated) = operate(&void, &local, spec, treatment, history)? else {
                return Ok(None);
            };
            if treated.boundaries().len() != 1 {
                return Err(invalid("A pocket treatment unexpectedly split its void"));
            }
            let mut new_caps = Vec::new();
            for face in treated.face_iter() {
                if let Some(candidate) = crate::planar::boundary_plane(face)?
                    && candidate.normal.dot(plane.normal) > 1. - 1e-9
                    && plane.signed(candidate.origin).abs() <= EPS
                {
                    new_caps.push(face);
                }
            }
            let [new_cap] = new_caps.as_slice() else {
                return Err(invalid("The treated pocket has no unique planar entry cap"));
            };
            if new_cap.boundaries().len() != 1 {
                return Err(invalid(
                    "A treated convex pocket has a multiply bounded entry",
                ));
            }
            let mut new_wires = wires.clone();
            new_wires[opening_index] = new_cap.boundaries()[0].inverse();
            let source_cap = Face::try_new(new_wires, cap.oriented_surface())
                .map_err(|_| invalid("The treated opening cannot join the original stock cap"))?;
            history.retain_face(source_cap.id(), cap.id())?;
            let mut shells = Vec::new();
            for source_shell in solid.boundaries() {
                let contains = source_shell.iter().any(|face| face.id() == cap.id());
                let mut result = Shell::new();
                for face in source_shell {
                    if face.id() == cap.id() {
                        result.push(source_cap.clone());
                    } else if !component.contains(&face.id()) {
                        result.push(face.clone());
                    }
                }
                if contains {
                    result.extend(
                        treated
                            .face_iter()
                            .filter(|face| face.id() != new_cap.id())
                            .map(Face::inverse),
                    );
                }
                shells.push(result);
            }
            let result = Solid::try_new(shells).map_err(|_| {
                invalid("The treated pocket does not join a closed oriented stock body")
            })?;
            crate::validation::solid(&result)?;
            return Ok(Some((result, local)));
        }
    }
    Ok(None)
}
