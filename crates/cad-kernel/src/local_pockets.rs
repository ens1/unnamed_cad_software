//! Exact local cap insertion. Remote bevels, blends and voids need finite native
//! separation, rather than turning the complete stock into a convex prism.
use crate::planar::{BoundaryPlane, CutFace, CutOutput};
use cad_core::{CadError, Result};
use monstertruck::modeling::*;
use std::collections::{HashMap, HashSet};

const EPS: f64 = 1e-7;
const ANGULAR: f64 = 1e-10;
const MAX_WORK: usize = 2_000_000;
pub(crate) struct Budget {
    remaining: usize,
}
impl Budget {
    pub(crate) fn new() -> Self {
        Self {
            remaining: MAX_WORK,
        }
    }
    fn charge(&mut self, work: usize) -> Result<()> {
        self.remaining = self.remaining.checked_sub(work).ok_or_else(|| {
            CadError::Unsupported(
                "Local planar pocket exceeds its aggregate native proof work".into(),
            )
        })?;
        Ok(())
    }
    fn face_controls(&mut self, face: &Face) -> Result<usize> {
        let mut controls = crate::native_bounds::face(face)?.len();
        for edge in face.edge_iter() {
            controls = controls
                .checked_add(crate::native_bounds::curve(&edge.curve())?.len())
                .ok_or_else(|| {
                    CadError::Unsupported(
                        "Local planar pocket control count overflows its proof budget".into(),
                    )
                })?;
        }
        self.charge(controls)?;
        Ok(controls)
    }
    pub(crate) fn covered(&mut self, target: &Face, cap: &Face) -> Result<bool> {
        let controls = self
            .face_controls(target)?
            .checked_add(self.face_controls(cap)?)
            .ok_or_else(|| {
                CadError::Unsupported(
                    "Local planar pocket coverage exceeds its control budget".into(),
                )
            })?;
        let cost = controls
            .checked_mul(target.edge_iter().count().saturating_add(1))
            .and_then(|cost| cost.checked_mul(4))
            .ok_or_else(|| {
                CadError::Unsupported("Local planar pocket coverage exceeds its work budget".into())
            })?;
        self.charge(cost)?;
        crate::extrusion_surface::covered(target, cap)
    }
}
fn invalid(message: &str) -> CadError {
    CadError::Invalid(format!("Local planar pocket: {message}"))
}
fn admit<T>(result: Result<T>) -> Result<Option<T>> {
    match result {
        Ok(value) => Ok(Some(value)),
        Err(CadError::Unsupported(_)) => Ok(None),
        Err(error) => Err(error),
    }
}

pub(crate) fn planes_with_budget(stock: &Solid, budget: &mut Budget) -> Result<Vec<BoundaryPlane>> {
    if stock.face_iter().count() > 16_384 {
        return Err(CadError::Unsupported(
            "Local planar pocket exceeds its native face budget".into(),
        ));
    }
    // Admission already needs every remote carrier bounded. Check the complete
    // native enclosure/control budget before repeated planarity/coverage work.
    let enclosures = crate::native_bounds::solid(stock)?;
    let mut planes = vec![];
    for (face, bound) in stock.face_iter().zip(enclosures) {
        budget.charge(bound.len())?;
        if matches!(
            face.surface(),
            Surface::Plane(_) | Surface::BsplineSurface(_) | Surface::NurbsSurface(_)
        ) {
            budget.face_controls(face)?;
        }
        if let Some(plane) = crate::planar::boundary_plane(face)? {
            planes.push(plane);
        }
    }
    Ok(planes)
}

/// Project a cap only onto a complete source face, along the authored unit axis.
/// A remote elevated boss with the same normal cannot become the opening.
pub(crate) fn projected_cap(
    cap: &Face,
    plane: &BoundaryPlane,
    axis: Vector3,
    budget: &mut Budget,
) -> Result<Option<Face>> {
    let u = plane
        .normal
        .cross(if plane.normal.x.abs() < 0.8 {
            Vector3::unit_x()
        } else {
            Vector3::unit_y()
        })
        .normalize();
    let frame = cad_core::Plane {
        origin: plane.origin.into(),
        u: u.into(),
        v: plane.normal.cross(u).into(),
    };
    let projected = crate::extrusion::project_face(cap, axis, &frame, 0.)?;
    Ok(budget
        .covered(&plane.face, &projected)?
        .then_some(projected))
}

pub(crate) fn cut_from_caps(
    stock: &Solid,
    lower: &Face,
    upper: &Face,
    axis: Vector3,
) -> Result<Option<CutOutput>> {
    let mut budget = Budget::new();
    if !axis.magnitude().is_finite() || (axis.magnitude() - 1.).abs() > ANGULAR {
        return Err(invalid("The cut axis must be finite and unit length"));
    }
    let Some(original_tool) = admit(crate::extrusion::between_caps(lower, upper, axis))? else {
        return Ok(None);
    };
    let Some(planes) = admit(planes_with_budget(stock, &mut budget))? else {
        return Ok(None);
    };
    let anchor = lower
        .vertex_iter()
        .next()
        .ok_or_else(|| invalid("The cut section is empty"))?
        .point();
    let mut caps = [lower.clone(), upper.clone()];
    for (index, cap) in caps.iter_mut().enumerate() {
        let Some(hull) = admit(crate::extrusion::face_hull(cap))? else {
            return Ok(None);
        };
        let mut choices = vec![];
        for plane in &planes {
            let dot = plane.normal.dot(axis);
            if (index == 0 && dot >= -1. + ANGULAR)
                || (index == 1 && dot <= 1. - ANGULAR)
                || plane.normal.cross(axis).magnitude() > ANGULAR
                || !hull.iter().all(|point| plane.signed(*point) >= -EPS)
            {
                continue;
            }
            if let Some(projected) = admit(projected_cap(cap, plane, axis, &mut budget))?.flatten()
            {
                choices.push(((plane.origin - anchor).dot(axis), projected));
            }
        }
        choices.sort_by(|a, b| a.0.total_cmp(&b.0));
        if choices.len() > 1 && (choices[0].0 - choices[1].0).abs() <= EPS {
            return Ok(None);
        }
        if let Some((_, projected)) = choices.into_iter().next() {
            *cap = projected;
        }
    }
    let Some(tool) = admit(crate::extrusion::between_caps(&caps[0], &caps[1], axis))? else {
        return Ok(None);
    };
    insert_tool_with_budget(stock, &tool, axis, &original_tool, &mut budget)
}

/// The supplied tool is already cropped. Its two native terminal caps can open
/// stock; shoulders and tapered walls are retained exactly, never re-lofted.
pub(crate) fn insert_tool(stock: &Solid, tool: &Solid, axis: Vector3) -> Result<Option<CutOutput>> {
    insert_tool_with_envelope(stock, tool, axis, tool)
}
pub(crate) fn insert_tool_with_envelope(
    stock: &Solid,
    tool: &Solid,
    axis: Vector3,
    original_tool: &Solid,
) -> Result<Option<CutOutput>> {
    insert_tool_with_budget(stock, tool, axis, original_tool, &mut Budget::new())
}
pub(crate) fn insert_tool_with_budget(
    stock: &Solid,
    tool: &Solid,
    axis: Vector3,
    original_tool: &Solid,
    budget: &mut Budget,
) -> Result<Option<CutOutput>> {
    if !axis.magnitude().is_finite() || (axis.magnitude() - 1.).abs() > ANGULAR {
        return Err(invalid("The cut axis must be finite and unit length"));
    }
    if tool.boundaries().len() != 1 || stock.boundaries().is_empty() {
        return Ok(None);
    }
    let shell = &tool.boundaries()[0];
    if shell.len() < 3 {
        return Ok(None);
    }
    let terminals = [&shell[0], shell.last().unwrap()];
    if terminals.iter().any(|cap| cap.boundaries().len() != 1) {
        return Ok(None);
    }
    let Some(planes) = admit(planes_with_budget(stock, budget))? else {
        return Ok(None);
    };
    let Some(bounds) = admit(crate::native_bounds::solid(tool))? else {
        return Ok(None);
    };
    let Some(original_bounds) = admit(crate::native_bounds::solid(original_tool))? else {
        return Ok(None);
    };
    let mut openings = vec![];
    for (index, cap) in terminals.iter().enumerate() {
        let Some(cap_plane) = crate::planar::boundary_plane(cap)? else {
            return Ok(None);
        };
        if cap_plane.normal.cross(axis).magnitude() > ANGULAR
            || cap_plane.normal.dot(axis) * (if index == 0 { -1. } else { 1. }) < 1. - ANGULAR
        {
            return Ok(None);
        }
        let Some(cap_hull) = admit(crate::extrusion::face_hull(cap))? else {
            return Ok(None);
        };
        let mut matches = vec![];
        for plane in &planes {
            if plane.normal.dot(cap_plane.normal) < 1. - ANGULAR
                || !cap_hull
                    .iter()
                    .all(|point| plane.signed(*point).abs() <= EPS)
            {
                continue;
            }
            if admit(budget.covered(&plane.face, cap))? == Some(true) {
                matches.push(plane.clone());
            }
        }
        if matches.len() > 1 {
            return Ok(None);
        }
        if let Some(plane) = matches.pop() {
            // The cap's outward orientation supplies the material seed. Every
            // complete tool carrier must lie on its inward side.
            for bound in &bounds {
                if bound.projection(plane.origin, plane.normal)?.1 > EPS {
                    return Ok(None);
                }
            }
            openings.push((index, plane));
        }
    }
    if openings.is_empty() {
        return Ok(None);
    }
    let mut axes = vec![
        Vector3::unit_x(),
        Vector3::unit_y(),
        Vector3::unit_z(),
        axis,
    ];
    for (_, plane) in &openings {
        let u = plane
            .normal
            .cross(if plane.normal.x.abs() < 0.8 {
                Vector3::unit_x()
            } else {
                Vector3::unit_y()
            })
            .normalize();
        axes.extend([u, plane.normal.cross(u)]);
    }
    // Native cap chords propose additional separator directions for rotated
    // profiles. They never classify material: every direction below still
    // projects the complete outward carrier enclosures, not chord samples.
    for cap in terminals {
        for edge in cap.edge_iter() {
            let chord = edge.back().point() - edge.front().point();
            if chord.magnitude2().is_finite() && chord.magnitude2() > EPS * EPS {
                for direction in [chord.normalize(), axis.cross(chord).normalize()] {
                    if direction.magnitude2().is_finite()
                        && !axes
                            .iter()
                            .any(|old| old.cross(direction).magnitude() <= ANGULAR)
                    {
                        axes.push(direction);
                    }
                    if axes.len() >= 32 {
                        break;
                    }
                }
            }
            if axes.len() >= 32 {
                break;
            }
        }
        if axes.len() >= 32 {
            break;
        }
    }
    let origin = terminals[0].vertex_iter().next().unwrap().point();
    let mut envelope = vec![];
    for direction in &axes {
        let mut interval = (f64::INFINITY, f64::NEG_INFINITY);
        for bound in original_bounds.iter().chain(&bounds) {
            budget.charge(bound.len())?;
            let (low, high) = bound.projection(origin, *direction)?;
            interval = (interval.0.min(low), interval.1.max(high));
        }
        envelope.push(interval);
    }
    for face in stock.face_iter() {
        if openings
            .iter()
            .any(|(_, plane)| plane.face.id() == face.id())
        {
            continue;
        }
        let Some(bound) = admit(crate::native_bounds::face(face))? else {
            return Ok(None);
        };
        let mut separated = false;
        for (direction, tool) in axes.iter().zip(&envelope) {
            budget.charge(bound.len())?;
            let face = bound.projection(origin, *direction)?;
            if face.1 < tool.0 - EPS || tool.1 < face.0 - EPS {
                separated = true;
                break;
            }
        }
        if !separated {
            return Ok(None);
        }
    }
    reconstruct(stock, tool, &openings)
}

fn reconstruct(
    stock: &Solid,
    tool: &Solid,
    openings: &[(usize, BoundaryPlane)],
) -> Result<Option<CutOutput>> {
    let terminals = [
        &tool.boundaries()[0][0],
        tool.boundaries()[0].last().unwrap(),
    ];
    let mut replacements = HashMap::new();
    let mut removed = HashSet::new();
    let mut tags = vec![];
    for (index, plane) in openings {
        let cap = terminals[*index];
        let mut boundaries = plane.face.boundaries();
        boundaries.push(cap.boundaries()[0].inverse());
        let face = Face::try_new(boundaries, plane.face.oriented_surface())
            .map_err(|error| CadError::Invalid(error.to_string()))?;
        let edges = face
            .edge_iter()
            .map(|edge| edge.id())
            .collect::<HashSet<_>>();
        let mut sources = vec![(plane.face.id(), false)];
        sources.extend(
            tool.face_iter()
                .filter(|wall| {
                    wall.id() != cap.id() && wall.edge_iter().any(|edge| edges.contains(&edge.id()))
                })
                .map(|wall| (wall.id(), true)),
        );
        tags.push(CutFace {
            face: face.id(),
            sources,
            partition_sources: vec![],
        });
        if replacements.insert(plane.face.id(), face).is_some() {
            return Ok(None);
        }
        removed.insert(cap.id());
    }
    let mut shell = Shell::new();
    for face in stock.face_iter() {
        shell.push(
            replacements
                .remove(&face.id())
                .unwrap_or_else(|| face.clone()),
        );
    }
    shell.extend(
        tool.face_iter()
            .filter(|face| !removed.contains(&face.id()))
            .map(Face::inverse),
    );
    // Preserve original negative/remote shells as boundaries of the same stock;
    // a void component must never become a separate positive output body.
    let result = Solid::try_new(shell.connected_components())
        .map_err(|error| CadError::Invalid(error.to_string()))?;
    crate::validation::solid(&result)?;
    Ok(Some(CutOutput {
        bodies: vec![result],
        faces: tags,
        tool: Some(tool.clone()),
    }))
}

pub(crate) fn difference(stock: &Solid, tool: &Solid) -> Result<Option<CutOutput>> {
    let Some(cap) = tool.boundaries().first().and_then(|shell| shell.first()) else {
        return Ok(None);
    };
    let Some(plane) = crate::planar::boundary_plane(cap)? else {
        return Ok(None);
    };
    insert_tool(stock, tool, -plane.normal)
}

pub(crate) fn record_history(
    output: &CutOutput,
    scope: &std::collections::BTreeSet<cad_core::NodeRef>,
    context: crate::revolution::Combination<'_>,
) -> Result<()> {
    for tag in &output.faces {
        let sources=tag.sources.iter().map(|(face,_)|context.generated.face_source(*face).or_else(||scope.iter().find_map(|body|context.adapter.face_source(body,*face))).ok_or_else(||CadError::Unsupported("Local pocket source-face ancestry is unavailable; choose the current authored input".into()))).collect::<Result<Vec<_>>>()?;
        let face = output
            .bodies
            .iter()
            .flat_map(Solid::face_iter)
            .find(|face| face.id() == tag.face)
            .ok_or_else(|| invalid("A local pocket lost its reconstructed source cap"))?;
        context.generated.tool_face(
            context.owner,
            face,
            &sources,
            &serde_json::json!({"operation":"boolean_difference","constructor":"local_planar_cap"}),
        )?;
    }
    Ok(())
}
