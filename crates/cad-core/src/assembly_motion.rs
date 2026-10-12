//! Transient, application-owned assembly manipulation. Goals never become mates.
use crate::{
    CadError, Document, Id, Placement, Result,
    assembly_geometry::{AssemblySolution, EvaluatedAssembly},
    configuration,
    solver::{ConstraintSolver, NativeSolver},
};
use nalgebra::{Matrix4, Rotation3, Vector3, Vector4};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
pub mod drag;

pub const CONTRACT_VERSION: u32 = 2;
pub const MAX_COMPONENTS: usize = 128;

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
    /// Optional transient starting pose for resolving remaining freedom. It is
    /// a numerical preference, never an engineering constraint or saved mate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preferred_placement: Option<Placement>,
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
            preferred_placement: None,
        }
    }
    pub fn validate(&self) -> Result<()> {
        if self.instance.is_empty()
            || self.position.is_none() && self.rotation_deg.is_none()
            || self.anchor.iter().any(|v| !v.is_finite())
            || self.position.iter().flatten().any(|v| !v.is_finite())
            || self.rotation_deg.iter().flatten().any(|v| !v.is_finite())
            || self.preferred_placement.as_ref().is_some_and(|p| {
                p.translation
                    .iter()
                    .chain(&p.rotation_deg)
                    .any(|v| !v.is_finite())
            })
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
        let baseline = self.preferred_placement.as_ref().unwrap_or(baseline);
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
    /// Additional distinct direct participants, requested in one shared solve.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub additional_goals: Vec<Goal>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub additional_paths: Vec<Vec<Id>>,
}
impl Intent {
    pub fn participants(&self) -> impl Iterator<Item = (&[Id], &Goal)> {
        std::iter::once((self.path.as_slice(), &self.goal)).chain(
            self.additional_paths
                .iter()
                .map(Vec::as_slice)
                .zip(&self.additional_goals),
        )
    }
    pub fn validate(&self) -> Result<()> {
        if !(1..=CONTRACT_VERSION).contains(&self.version)
            || self.additional_goals.len() != self.additional_paths.len()
            || self.additional_goals.len() >= MAX_COMPONENTS
            || self.version == 1 && !self.additional_goals.is_empty()
        {
            return Err(CadError::Invalid(
                "Movement needs a supported contract and 1–128 complete component goals".into(),
            ));
        }
        let mut distinct = BTreeSet::new();
        for (path, goal) in self.participants() {
            goal.validate()?;
            if path.first() != Some(&goal.instance)
                || path.len() > 64
                || !distinct.insert(&goal.instance)
            {
                return Err(CadError::Invalid("Movement component paths must be complete and direct participants must be distinct".into()));
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GoalError {
    pub position_error_mm: Option<f64>,
    pub rotation_error_deg: Option<f64>,
}
fn goal_error(goal: &Goal, placement: &Placement) -> GoalError {
    let residual = goal.residual(&placement.matrix());
    GoalError {
        position_error_mm: goal.position.map(|_| {
            residual
                .iter()
                .take(3)
                .fold(0_f64, |length, value| length.hypot(*value))
        }),
        rotation_error_deg: goal
            .rotation_deg
            .map(|_| {
                residual
                    .iter()
                    .skip(if goal.position.is_some() { 3 } else { 0 })
                    .fold(0_f64, |length, value| length.hypot(*value))
                    / 100.
            })
            .map(f64::to_degrees),
    }
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
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub goal_errors: BTreeMap<Id, GoalError>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub additional_paths: Vec<Vec<Id>>,
}

fn source_parameter(
    parameter: f64,
    pair: Option<&[crate::assembly_geometry::Frame; 2]>,
) -> Result<f64> {
    if !parameter.is_finite() {
        return Err(CadError::Invalid(
            "The solved movement path parameter is non-finite".into(),
        ));
    }
    let curve = pair
        .and_then(|pair| pair[0].curve.as_deref())
        .ok_or_else(|| {
            CadError::Invalid("The solved movement path carrier is unavailable".into())
        })?;
    // These are the same carrier-domain mappings used by mate equations.
    // The source seed stays within its schema without changing evaluated contact.
    Ok(if curve.closed() {
        parameter.rem_euclid(1.)
    } else {
        parameter.clamp(0., 1.)
    })
}

pub fn solve(
    doc: &Document,
    evaluated: &BTreeMap<Id, EvaluatedAssembly>,
    intent: &Intent,
) -> Result<Outcome> {
    doc.validate()?;
    intent.validate()?;
    if intent.assembly != doc.root {
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
    let occurrences = configuration::placed_parts(doc, Some(evaluated))?;
    for (path, goal) in intent.participants() {
        if generated.contains(&goal.instance) {
            return Err(CadError::Invalid(
                "This occurrence follows a component pattern; move its seed instead".into(),
            ));
        }
        let selected = assembly.instances.get(&goal.instance).ok_or_else(|| {
            CadError::Invalid("The selected component is suppressed or missing".into())
        })?;
        if selected.fixed || selected.translation_expressions.is_some() {
            return Err(CadError::Invalid(
                "Release the component's fixed or expression-driven placement before moving it"
                    .into(),
            ));
        }
        if !occurrences.iter().any(|i| i.path.starts_with(path)) {
            return Err(CadError::Invalid(
                "The complete selected occurrence is no longer active".into(),
            ));
        }
        let motion = current
            .component_motion
            .get(&goal.instance)
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
    }
    assembly.instances.retain(|id, _| !generated.contains(id));
    for (id, instance) in &mut assembly.instances {
        instance.placement = current.placements.get(id).cloned().ok_or_else(|| {
            CadError::Invalid("The movement baseline is missing a component placement".into())
        })?;
    }
    assembly = crate::assembly_groups::prepared(&assembly)?;
    // A selection moves as one transient rigid cohort. These relative locks
    // never enter saved definitions, undo records or authored mate lists.
    let primary = &current.placements[&intent.goal.instance];
    for goal in &intent.additional_goals {
        let relative = crate::assembly_geometry::relative_placement(
            primary,
            &current.placements[&goal.instance],
        )?;
        let id = format!("movement-cohort-{}-{}", intent.goal.instance, goal.instance);
        if assembly.mates.iter().any(|mate| mate.id == id) {
            return Err(CadError::Invalid(
                "A saved mate collides with the transient movement identity".into(),
            ));
        }
        assembly.mates.push(crate::Mate {
            id,
            kind: "lock".into(),
            first: intent.goal.instance.clone(),
            second: goal.instance.clone(),
            first_origin: [0.; 3],
            second_origin: [0.; 3],
            first_axis: [0., 0., 1.],
            second_axis: [0., 0., 1.],
            offset: "0 mm".into(),
            first_reference: None,
            second_reference: None,
            alignment: Default::default(),
            lock_rotation: false,
            rotation_offset: "0 deg".into(),
            lock_placement: Some(relative),
            relationship: None,
        });
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
            *seed = source_parameter(parameter, current.frames.get(&mate.id))?;
        }
    }
    let goals = intent
        .participants()
        .map(|(_, goal)| goal.clone())
        .collect::<Vec<_>>();
    let solution = NativeSolver.manipulate_assembly_group(
        &scoped,
        &mut assembly,
        &current.frames,
        &current.auxiliary_frames,
        &goals,
    )?;
    let mut goal_errors = BTreeMap::new();
    let actual_primary = &assembly.instances[&intent.goal.instance].placement;
    for goal in &goals {
        let actual = &assembly.instances[&goal.instance].placement;
        goal_errors.insert(goal.instance.clone(), goal_error(goal, actual));
        if goal.instance != intent.goal.instance {
            let before = crate::assembly_geometry::relative_placement(
                primary,
                &current.placements[&goal.instance],
            )?;
            let after = crate::assembly_geometry::relative_placement(actual_primary, actual)?;
            let translation = Vector3::from(after.translation) - Vector3::from(before.translation);
            let rotation = after.matrix().fixed_view::<3, 3>(0, 0)
                * before.matrix().fixed_view::<3, 3>(0, 0).transpose();
            if translation.norm() > 1e-5
                || Rotation3::from_matrix_unchecked(rotation).angle() > 1e-7
            {
                return Err(CadError::Invalid("The selected movement cohort could not retain its captured relative poses within its mates".into()));
            }
        }
    }
    let primary_error = &goal_errors[&intent.goal.instance];
    let position_error_mm = primary_error.position_error_mm;
    let rotation_error_deg = primary_error.rotation_error_deg;
    let curve_parameters = solution
        .diagnostics
        .iter()
        .filter_map(|(id, d)| {
            d.curve_parameter.map(|value| {
                source_parameter(value, current.frames.get(id)).map(|value| (id.clone(), value))
            })
        })
        .collect::<Result<BTreeMap<_, _>>>()?;
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
        goal_errors,
        additional_paths: intent.additional_paths.clone(),
    })
}

/// Apply solved seed poses in the currently selected named configuration or base.
pub fn apply(doc: &mut Document, outcome: &Outcome) -> Result<()> {
    let scope = configuration::PlacementScope::selected(doc, &outcome.assembly);
    apply_in_scope(doc, outcome, &scope)
}

/// Author solved seeds atomically in the explicitly selected source scope.
/// Fixed and pattern-driven components retain their source; translation equations
/// and hard mate fractions remain authoritative.
pub fn apply_in_scope(
    doc: &mut Document,
    outcome: &Outcome,
    scope: &configuration::PlacementScope,
) -> Result<()> {
    if outcome.version != CONTRACT_VERSION || outcome.assembly != doc.root {
        return Err(CadError::Invalid("Invalid assembly movement result".into()));
    }
    let assembly = doc
        .assemblies
        .get(&outcome.assembly)
        .ok_or_else(|| CadError::Invalid("Movement assembly is missing".into()))?;
    let generated = crate::component_pattern::generated(assembly);
    let mut poses = BTreeMap::new();
    for (id, placement) in &outcome.placements {
        let instance = assembly.instances.get(id).ok_or_else(|| {
            CadError::Invalid("Movement result contains an unknown component".into())
        })?;
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
        if generated.contains(id) || instance.fixed {
            continue;
        }
        poses.insert(
            id.clone(),
            configuration::PoseOverride {
                translation: if instance.translation_expressions.is_some() {
                    [None; 3]
                } else {
                    placement.translation.map(Some)
                },
                rotation_deg: placement.rotation_deg.map(Some),
            },
        );
    }
    let seeds = outcome
        .curve_parameters
        .iter()
        .filter(|(id, _)| {
            assembly.mates.iter().any(|mate| {
                &mate.id == *id
                    && matches!(
                        mate.relationship
                            .as_deref()
                            .and_then(crate::assembly_geometry::Relationship::curve_position),
                        Some(crate::assembly_geometry::CurvePosition::Free { .. })
                    )
            })
        })
        .map(|(id, seed)| (id.clone(), *seed))
        .collect();
    configuration::set_component_placements(doc, &outcome.assembly, scope, &poses, &seeds)
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
