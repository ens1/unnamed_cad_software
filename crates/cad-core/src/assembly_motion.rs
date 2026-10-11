//! Transient, application-owned assembly manipulation. Goals never become mates.
use crate::{
    CadError, Document, Id, Placement, Result,
    assembly_geometry::{AssemblySolution, EvaluatedAssembly},
    configuration,
    solver::{ConstraintSolver, NativeSolver},
};
use nalgebra::{Matrix4, Rotation3, Vector3, Vector4};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const CONTRACT_VERSION: u32 = 1;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Goal {
    pub instance: Id,
    /// Mechanical anchor in the direct component's local frame. A nested picked
    /// face/point is converted into this frame without losing its occurrence path.
    pub anchor: [f64; 3],
    /// Desired anchor location in the owning assembly, millimeters.
    pub position: Option<[f64; 3]>,
    /// Desired component orientation, XYZ Euler degrees in the owning assembly.
    pub rotation_deg: Option<[f64; 3]>,
}
impl Goal {
    pub fn from_offsets(
        instance: Id,
        placement: &Placement,
        anchor: [f64; 3],
        translation_mm: [f64; 3],
        rotation_rad: Option<[f64; 3]>,
    ) -> Self {
        let p = placement.matrix() * Vector4::new(anchor[0], anchor[1], anchor[2], 1.);
        let rotation_deg = rotation_rad.map(|angles| {
            let delta = Rotation3::from_euler_angles(angles[0], angles[1], angles[2]);
            let base = placement.rotation_deg.map(f64::to_radians);
            let matrix = delta * Rotation3::from_euler_angles(base[0], base[1], base[2]);
            let (x, y, z) = matrix.euler_angles();
            [x, y, z].map(f64::to_degrees)
        });
        Self {
            instance,
            anchor,
            position: Some([
                p.x + translation_mm[0],
                p.y + translation_mm[1],
                p.z + translation_mm[2],
            ]),
            rotation_deg,
        }
    }
    pub fn validate(&self) -> Result<()> {
        if self.instance.is_empty()
            || self.position.is_none() && self.rotation_deg.is_none()
            || self.anchor.iter().any(|v| !v.is_finite())
            || self.position.iter().flatten().any(|v| !v.is_finite())
            || self.rotation_deg.iter().flatten().any(|v| !v.is_finite())
        {
            return Err(CadError::Invalid("Invalid assembly movement goal".into()));
        }
        Ok(())
    }
    pub(crate) fn residual(&self, matrix: &Matrix4<f64>) -> Vec<f64> {
        let mut values = vec![];
        if let Some(position) = self.position {
            let p = matrix * Vector4::new(self.anchor[0], self.anchor[1], self.anchor[2], 1.);
            values.extend((Vector3::new(p.x, p.y, p.z) - Vector3::from(position)).iter());
        }
        if let Some(angles) = self.rotation_deg {
            let angles = angles.map(f64::to_radians);
            let desired = Rotation3::from_euler_angles(angles[0], angles[1], angles[2]);
            let actual =
                Rotation3::from_matrix_unchecked(matrix.fixed_view::<3, 3>(0, 0).into_owned());
            values.extend(((actual * desired.inverse()).scaled_axis() * 100.).iter());
        }
        values
    }
    pub(crate) fn preference(&self, baseline: &Placement) -> Placement {
        let rotation_deg = self.rotation_deg.unwrap_or(baseline.rotation_deg);
        let angles = rotation_deg.map(f64::to_radians);
        let rotation = Rotation3::from_euler_angles(angles[0], angles[1], angles[2]);
        let translation = self
            .position
            .map(|position| {
                (Vector3::from(position) - rotation * Vector3::from(self.anchor)).into()
            })
            .unwrap_or(baseline.translation);
        Placement {
            translation,
            rotation_deg,
        }
    }
}

pub fn local_anchor(placement: &Placement, point: [f64; 3]) -> Result<[f64; 3]> {
    if point.iter().any(|v| !v.is_finite()) {
        return Err(CadError::Invalid(
            "The picked movement pivot is non-finite".into(),
        ));
    }
    let inverse = placement
        .matrix()
        .try_inverse()
        .ok_or_else(|| CadError::Invalid("The component's movement frame is singular".into()))?;
    let point = inverse * Vector4::new(point[0], point[1], point[2], 1.);
    Ok([point.x, point.y, point.z])
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Intent {
    pub version: u32,
    pub assembly: Id,
    /// Complete root-relative clicked occurrence. A rigid subassembly moves as
    /// one direct participant; this never edits a shared nested definition.
    pub path: Vec<Id>,
    pub goal: Goal,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Outcome {
    pub version: u32,
    pub assembly: Id,
    pub path: Vec<Id>,
    pub placements: BTreeMap<Id, Placement>,
    /// Free path/contact seeds follow the solution so a later evaluation does
    /// not pull a moved follower back toward an obsolete parameter seed.
    pub curve_parameters: BTreeMap<Id, f64>,
    pub solution: AssemblySolution,
    pub position_error_mm: Option<f64>,
    pub rotation_error_deg: Option<f64>,
}

pub fn solve(
    doc: &Document,
    evaluated: &BTreeMap<Id, EvaluatedAssembly>,
    intent: &Intent,
) -> Result<Outcome> {
    doc.validate()?;
    intent.goal.validate()?;
    if intent.version != CONTRACT_VERSION
        || intent.assembly != doc.root
        || intent.path.first() != Some(&intent.goal.instance)
    {
        return Err(CadError::Invalid(
            "Movement needs a current root assembly occurrence and supported contract".into(),
        ));
    }
    let config = doc.active_configurations.get(&intent.assembly);
    let scoped = configuration::scope(doc, doc, &intent.assembly, config.map(String::as_str))?;
    let key = configuration::key(&intent.assembly, config.map(String::as_str));
    let current = evaluated
        .get(&key)
        .filter(|a| a.version == crate::assembly_geometry::CONTRACT_VERSION)
        .ok_or_else(|| {
            CadError::Invalid("Evaluate the current assembly before moving it".into())
        })?;
    let mut assembly = scoped
        .assemblies
        .get(&intent.assembly)
        .cloned()
        .ok_or_else(|| {
            CadError::Invalid("Component movement is available in an assembly".into())
        })?;
    let generated = crate::component_pattern::generated(&assembly);
    if generated.contains(&intent.goal.instance) {
        return Err(CadError::Invalid(
            "This occurrence follows a component pattern; move its seed instead".into(),
        ));
    }
    let selected = assembly
        .instances
        .get(&intent.goal.instance)
        .ok_or_else(|| {
            CadError::Invalid("The selected component is suppressed or missing".into())
        })?;
    if selected.fixed || selected.translation_expressions.is_some() {
        return Err(CadError::Invalid(
            "Release the component's fixed or expression-driven placement before moving it".into(),
        ));
    }
    let occurrences = configuration::placed_parts(doc, Some(evaluated))?;
    if !occurrences.iter().any(|i| i.path.starts_with(&intent.path)) {
        return Err(CadError::Invalid(
            "The complete selected occurrence is no longer active".into(),
        ));
    }
    let motion = current
        .component_motion
        .get(&intent.goal.instance)
        .ok_or_else(|| {
            CadError::Invalid("Current component motion information is unavailable".into())
        })?;
    if motion.grounded || motion.degrees_of_freedom == 0 {
        return Err(CadError::Invalid(if motion.grounded {
            "This component is grounded by the assembly. Fix another reference component before moving it."
        } else {
            "This component has no remaining mate freedom; edit its driving mate instead."
        }.into()));
    }
    assembly.instances.retain(|id, _| !generated.contains(id));
    for (id, instance) in &mut assembly.instances {
        instance.placement = current.placements.get(id).cloned().ok_or_else(|| {
            CadError::Invalid("The movement baseline is missing a component placement".into())
        })?;
    }
    for mate in &mut assembly.mates {
        if let Some(parameter) = current
            .diagnostics
            .get(&mate.id)
            .and_then(|d| d.curve_parameter)
            && let Some(crate::assembly_geometry::CurvePosition::Free { seed }) =
                mate.relationship.as_deref_mut().and_then(|r| match r {
                    crate::assembly_geometry::Relationship::Path { position, .. }
                    | crate::assembly_geometry::Relationship::Slot { position, .. }
                    | crate::assembly_geometry::Relationship::Cam { position, .. } => {
                        Some(position)
                    }
                    _ => None,
                })
        {
            *seed = parameter;
        }
    }
    let solution = NativeSolver.manipulate_assembly(
        &scoped,
        &mut assembly,
        &current.frames,
        &current.auxiliary_frames,
        &intent.goal,
    )?;
    let placement = &assembly.instances[&intent.goal.instance].placement;
    let residual = intent.goal.residual(&placement.matrix());
    let position_error_mm = intent
        .goal
        .position
        .map(|_| residual.iter().take(3).map(|v| v * v).sum::<f64>().sqrt());
    let rotation_error_deg = intent.goal.rotation_deg.map(|_| {
        let offset = if intent.goal.position.is_some() { 3 } else { 0 };
        (residual
            .iter()
            .skip(offset)
            .map(|v| v * v)
            .sum::<f64>()
            .sqrt()
            / 100.)
            .to_degrees()
    });
    let curve_parameters = solution
        .diagnostics
        .iter()
        .filter_map(|(id, d)| d.curve_parameter.map(|value| (id.clone(), value)))
        .collect();
    Ok(Outcome {
        version: CONTRACT_VERSION,
        assembly: intent.assembly.clone(),
        path: intent.path.clone(),
        placements: assembly
            .instances
            .into_iter()
            .map(|(id, i)| (id, i.placement))
            .collect(),
        curve_parameters,
        solution,
        position_error_mm,
        rotation_error_deg,
    })
}

/// Apply solved seed poses atomically. Fixed/patterned identities and mate
/// definitions remain intact. Placement seeds belong to the assembly definition.
pub fn apply(doc: &mut Document, outcome: &Outcome) -> Result<()> {
    if outcome.version != CONTRACT_VERSION || outcome.assembly != doc.root {
        return Err(CadError::Invalid("Invalid assembly movement result".into()));
    }
    let mut candidate = doc.clone();
    let assembly = candidate
        .assemblies
        .get_mut(&outcome.assembly)
        .ok_or_else(|| CadError::Invalid("Movement assembly is missing".into()))?;
    let generated = crate::component_pattern::generated(assembly);
    for (id, placement) in &outcome.placements {
        let instance = assembly.instances.get_mut(id).ok_or_else(|| {
            CadError::Invalid("Movement result contains an unknown component".into())
        })?;
        if generated.contains(id) || instance.fixed {
            continue;
        }
        if placement
            .translation
            .iter()
            .chain(&placement.rotation_deg)
            .any(|v| !v.is_finite())
        {
            return Err(CadError::Invalid(
                "Movement produced a non-finite placement".into(),
            ));
        }
        if instance.translation_expressions.is_some() {
            // Expressions still own translation; preserve any coupled rotational
            // motion without replacing those expressions with numeric positions.
            instance.placement.rotation_deg = placement.rotation_deg;
        } else {
            instance.placement = placement.clone();
        }
    }
    for mate in &mut assembly.mates {
        if let Some(parameter) = outcome.curve_parameters.get(&mate.id)
            && let Some(crate::assembly_geometry::CurvePosition::Free { seed }) =
                mate.relationship.as_deref_mut().and_then(|r| match r {
                    crate::assembly_geometry::Relationship::Path { position, .. }
                    | crate::assembly_geometry::Relationship::Slot { position, .. }
                    | crate::assembly_geometry::Relationship::Cam { position, .. } => {
                        Some(position)
                    }
                    _ => None,
                })
        {
            *seed = *parameter;
        }
    }
    candidate.validate()?;
    *doc = candidate;
    Ok(())
}

/// Solved movement seeds must survive the normal current-source evaluation.
/// An adapter may resolve frames again or choose a different branch; never
/// silently commit an older movement result when its evaluated pose drifted.
pub fn validate_adoption(outcome: &Outcome, current: &EvaluatedAssembly) -> Result<()> {
    if outcome.version != CONTRACT_VERSION
        || current.version != crate::assembly_geometry::CONTRACT_VERSION
    {
        return Err(CadError::Invalid(
            "Assembly movement evaluation contract changed".into(),
        ));
    }
    for (id, expected) in &outcome.placements {
        let actual = current.placements.get(id).ok_or_else(|| {
            CadError::Invalid("The evaluated movement lost a component occurrence".into())
        })?;
        let translation = Vector3::from(actual.translation) - Vector3::from(expected.translation);
        let actual = actual.matrix();
        let expected = expected.matrix();
        let relative =
            actual.fixed_view::<3, 3>(0, 0) * expected.fixed_view::<3, 3>(0, 0).transpose();
        let rotation = Rotation3::from_matrix_unchecked(relative).angle();
        if translation.norm() > 1e-5 || rotation > 1e-7 {
            return Err(CadError::Invalid(format!(
                "The model evaluation changed a solved component pose ({:.6} mm, {:.6}°). Cancel and move again from its current validated placement.",
                translation.norm(),
                rotation.to_degrees(),
            )));
        }
    }
    Ok(())
}
