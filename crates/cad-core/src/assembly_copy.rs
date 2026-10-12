//! One-time copied components and mates. Evaluated placement is captured input;
//! inserted occurrences and constraints remain ordinary editable source objects.
use crate::{
    Assembly, CadError, Dimension, Document, Expr, Id, Mate, Placement, Result,
    assembly_geometry::{EvaluatedAssembly, Reference, Relationship},
    configuration::{self, PlacementScope, PoseOverride},
    protocol::{Evaluation, RequestIdentity},
};
use nalgebra::{Matrix4, Rotation3, Vector3};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const CONTRACT_VERSION: u32 = 1;
pub const MAX_COPIES: usize = 64;
pub const MAX_INSERTED: usize = 4096;
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Slot {
    First,
    Second,
    Auxiliary { index: usize },
    Support,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Binding {
    pub slot: Slot,
    /// Original outside component; checked against the captured source mate.
    pub source: Id,
    pub target: Id,
    /// Empty retains same-definition geometry below the rebound participant.
    /// A different definition needs an explicit current selected geometry source.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference: Option<Reference>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ExternalMate {
    pub source: Id,
    #[serde(default)]
    pub enabled: bool,
    pub bindings: Vec<Binding>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Reservation {
    pub index: usize,
    pub instances: BTreeMap<Id, Id>,
    pub mates: BTreeMap<Id, Id>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Intent {
    pub version: u32,
    pub id: Id,
    pub assembly: Id,
    pub seeds: Vec<Id>,
    /// Number of new groups, excluding the original selected components.
    pub count: Expr,
    pub translation: [Expr; 3],
    pub rotation: [Expr; 3],
    pub pivot: [Expr; 3],
    pub scope: PlacementScope,
    #[serde(default)]
    pub keep_fixed: bool,
    #[serde(default)]
    pub external: Vec<ExternalMate>,
    #[serde(default)]
    pub reservations: Vec<Reservation>,
}
impl Intent {
    pub fn new(assembly: Id, scope: PlacementScope) -> Self {
        Self {
            version: CONTRACT_VERSION,
            id: crate::id(),
            assembly,
            seeds: vec![],
            count: "1".into(),
            translation: ["30 mm".into(), "0 mm".into(), "0 mm".into()],
            rotation: ["0 deg".into(), "0 deg".into(), "0 deg".into()],
            pivot: ["0 mm".into(), "0 mm".into(), "0 mm".into()],
            scope,
            keep_fixed: false,
            external: vec![],
            reservations: vec![],
        }
    }
}
#[derive(Clone)]
pub struct Baseline {
    pub source: Document,
    pub context: Document,
    pub assembly: Assembly,
    pub solution: EvaluatedAssembly,
    pub identity: RequestIdentity,
}
fn invalid(message: impl Into<String>) -> CadError {
    CadError::Invalid(format!("Copy With Mates: {}", message.into()))
}
impl Baseline {
    pub fn capture(
        doc: &Document,
        evaluation: &Evaluation,
        identity: &RequestIdentity,
        owner: &str,
    ) -> Result<Self> {
        if !evaluation.valid
            || &evaluation.identity != identity
            || identity.source != doc.fingerprint()
        {
            return Err(invalid(
                "A current validated mechanical evaluation is required to capture solved source poses",
            ));
        }
        if owner != doc.root {
            return Err(CadError::Unsupported("Copy With Mates currently edits the root assembly; whole nested subassembly components and their nested references may be copied".into()));
        }
        let context = configuration::effective(doc)?;
        let assembly = context
            .assemblies
            .get(owner)
            .cloned()
            .ok_or_else(|| invalid("Owning assembly is missing"))?;
        // Group rigidity is source intent projected into exact lock equations.
        // Copies receive ordinary independent lock mates, never group membership.
        let mut assembly = crate::assembly_groups::prepared(&assembly)?;
        let solution = evaluation
            .assemblies
            .get(&configuration::key(
                owner,
                doc.active_configurations.get(owner).map(String::as_str),
            ))
            .cloned()
            .ok_or_else(|| invalid("Current solved assembly context is unavailable"))?;
        for (id, instance) in &mut assembly.instances {
            instance.placement =
                solution.placements.get(id).cloned().ok_or_else(|| {
                    invalid("An active component has no captured solved placement")
                })?;
            // Child geometry follows the evaluated occurrence configuration.
            instance.configuration = configuration::selected(
                &context,
                &instance.definition,
                instance.configuration.as_ref(),
            )
            .cloned();
        }
        Ok(Self {
            source: doc.clone(),
            context,
            assembly,
            solution,
            identity: identity.clone(),
        })
    }
    pub fn available(&self) -> Vec<Id> {
        let generated =
            crate::component_pattern::generated(&self.source.assemblies[&self.assembly.id]);
        self.assembly
            .instances
            .keys()
            .filter(|id| !generated.contains(*id))
            .cloned()
            .collect()
    }
}
fn count(baseline: &Baseline, intent: &Intent) -> Result<usize> {
    let value = crate::expression::value(
        &baseline.context,
        &intent.assembly,
        &intent.count,
        Dimension::Scalar,
    )?;
    if !value.is_finite() || value.fract() != 0. || !(1. ..=MAX_COPIES as f64).contains(&value) {
        return Err(invalid(
            "Number of copied groups must be an integer from 1 through 64",
        ));
    }
    Ok(value as usize)
}
fn slots(mate: &Mate) -> Vec<(Slot, Id, Option<Reference>)> {
    let mut out = vec![
        (
            Slot::First,
            mate.first.clone(),
            mate.first_reference.clone(),
        ),
        (
            Slot::Second,
            mate.second.clone(),
            mate.second_reference.clone(),
        ),
    ];
    if let Some(relationship) = mate.relationship.as_deref() {
        out.extend(
            relationship
                .endpoints()
                .into_iter()
                .enumerate()
                .map(|(index, endpoint)| {
                    (
                        Slot::Auxiliary { index },
                        endpoint.instance.clone(),
                        Some(endpoint.reference.clone()),
                    )
                }),
        );
        if let Relationship::Coupler {
            support: Some(id), ..
        } = relationship
        {
            out.push((Slot::Support, id.clone(), None));
        }
    }
    out
}
pub fn synchronize(baseline: &Baseline, intent: &mut Intent) -> Result<()> {
    validate_header(baseline, intent)?;
    let seeds = intent.seeds.iter().cloned().collect::<BTreeSet<_>>();
    let relevant = baseline
        .assembly
        .mates
        .iter()
        .filter(|mate| mate.participants().iter().any(|id| seeds.contains(*id)))
        .collect::<Vec<_>>();
    let mut external = vec![];
    for mate in &relevant {
        let outside = slots(mate)
            .into_iter()
            .filter(|(_, id, _)| !seeds.contains(id))
            .collect::<Vec<_>>();
        if outside.is_empty() {
            continue;
        }
        let previous = intent.external.iter().find(|entry| entry.source == mate.id);
        external.push(ExternalMate {
            source: mate.id.clone(),
            enabled: previous.is_some_and(|entry| entry.enabled),
            bindings: outside
                .into_iter()
                .map(|(slot, source, _)| {
                    previous
                        .and_then(|entry| {
                            entry
                                .bindings
                                .iter()
                                .find(|binding| binding.slot == slot && binding.source == source)
                        })
                        .cloned()
                        .unwrap_or(Binding {
                            slot,
                            source: source.clone(),
                            target: source,
                            reference: None,
                        })
                })
                .collect(),
        });
    }
    intent.external = external;
    let copies = count(baseline, intent)?;
    for index in 1..=copies {
        if !intent
            .reservations
            .iter()
            .any(|reservation| reservation.index == index)
        {
            intent.reservations.push(Reservation {
                index,
                instances: BTreeMap::new(),
                mates: BTreeMap::new(),
            });
        }
        let reservation = intent
            .reservations
            .iter_mut()
            .find(|reservation| reservation.index == index)
            .ok_or_else(|| invalid("Copy identity reservation is missing"))?;
        for seed in &intent.seeds {
            reservation
                .instances
                .entry(seed.clone())
                .or_insert_with(crate::id);
        }
        for mate in &relevant {
            reservation
                .mates
                .entry(mate.id.clone())
                .or_insert_with(crate::id);
        }
    }
    Ok(())
}
fn validate_header(baseline: &Baseline, intent: &Intent) -> Result<()> {
    if intent.version != CONTRACT_VERSION
        || intent.assembly != baseline.assembly.id
        || intent.id.is_empty()
    {
        return Err(invalid("Invalid intent identity, owner or version"));
    }
    let available = baseline.available();
    let unique = intent.seeds.iter().collect::<BTreeSet<_>>();
    if intent.seeds.is_empty()
        || unique.len() != intent.seeds.len()
        || intent.seeds.len() > 256
        || intent.seeds.iter().any(|seed| !available.contains(seed))
    {
        return Err(invalid(
            "Choose one or more active ordinary direct components; pattern-generated components cannot be copied by this command",
        ));
    }
    if intent.seeds.len().saturating_mul(count(baseline, intent)?) > MAX_INSERTED {
        return Err(invalid(
            "At most 4096 occurrences may be inserted per copy operation",
        ));
    }
    if let PlacementScope::Named(id) = &intent.scope
        && baseline.source.active_configurations.get(&intent.assembly) != Some(id)
    {
        return Err(invalid(
            "Named placement scope must be the captured current assembly configuration",
        ));
    }
    Ok(())
}
fn delta(baseline: &Baseline, intent: &Intent, index: usize) -> Result<Matrix4<f64>> {
    let values = |expressions: &[Expr; 3], dimension| -> Result<[f64; 3]> {
        let mut out = [0.; 3];
        for i in 0..3 {
            out[i] = crate::expression::value(
                &baseline.context,
                &intent.assembly,
                &expressions[i],
                dimension,
            )?;
        }
        Ok(out)
    };
    let translation = Vector3::from(values(&intent.translation, Dimension::Length)?) * index as f64;
    let angles = values(&intent.rotation, Dimension::Angle)?.map(|angle| angle * index as f64);
    let pivot = Vector3::from(values(&intent.pivot, Dimension::Length)?);
    let result = Matrix4::new_translation(&(pivot + translation))
        * Rotation3::from_euler_angles(angles[0], angles[1], angles[2]).to_homogeneous()
        * Matrix4::new_translation(&-pivot);
    if result.iter().any(|value| !value.is_finite()) {
        return Err(invalid("Copy transform is nonfinite"));
    }
    Ok(result)
}
fn placement(matrix: Matrix4<f64>) -> Result<Placement> {
    let rotation = Rotation3::from_matrix_unchecked(matrix.fixed_view::<3, 3>(0, 0).into_owned());
    let (x, y, z) = rotation.euler_angles();
    let result = Placement {
        translation: [matrix[(0, 3)], matrix[(1, 3)], matrix[(2, 3)]],
        rotation_deg: [x, y, z].map(f64::to_degrees),
    };
    if result
        .translation
        .iter()
        .chain(&result.rotation_deg)
        .any(|value| !value.is_finite())
    {
        return Err(invalid("A copied component has an invalid rigid placement"));
    }
    Ok(result)
}
fn rebound(
    baseline: &Baseline,
    seeds: &BTreeSet<Id>,
    copies: &BTreeMap<Id, Id>,
    external: Option<&ExternalMate>,
    slot: Slot,
    id: &Id,
    reference: &Option<Reference>,
) -> Result<(Id, Option<Reference>)> {
    if seeds.contains(id) {
        return Ok((
            copies
                .get(id)
                .cloned()
                .ok_or_else(|| invalid("A copied participant has no reserved identity"))?,
            reference.clone(),
        ));
    }
    let binding = external
        .and_then(|entry| {
            entry
                .bindings
                .iter()
                .find(|binding| binding.slot == slot && &binding.source == id)
        })
        .ok_or_else(|| invalid("An enabled outside mate lacks an explicit participant mapping"))?;
    let old = baseline
        .assembly
        .instances
        .get(id)
        .ok_or_else(|| invalid("Outside mate source participant is missing"))?;
    let target = baseline
        .assembly
        .instances
        .get(&binding.target)
        .ok_or_else(|| invalid("Outside mate target is inactive or missing"))?;
    let component_lock = external
        .and_then(|entry| {
            baseline
                .assembly
                .mates
                .iter()
                .find(|mate| mate.id == entry.source)
        })
        .is_some_and(|mate| mate.kind == "lock")
        && matches!(slot, Slot::First | Slot::Second);
    if component_lock {
        return Ok((binding.target.clone(), None));
    }
    if old.definition != target.definition && binding.reference.is_none() && slot != Slot::Support {
        return Err(invalid(
            "Rebinding to another definition requires an explicitly selected native mate reference",
        ));
    }
    let reference = binding.reference.clone().or_else(|| reference.clone());
    if let Some(reference) = &reference {
        crate::assembly_geometry::validate_reference(
            &baseline.context,
            &target.definition,
            reference,
        )?;
    }
    Ok((binding.target.clone(), reference))
}
fn copy_mate(
    baseline: &Baseline,
    intent: &Intent,
    mate: &Mate,
    reservation: &Reservation,
    transform: &Matrix4<f64>,
    placements: &BTreeMap<Id, Placement>,
) -> Result<Option<Mate>> {
    let seeds = intent.seeds.iter().cloned().collect::<BTreeSet<_>>();
    if !mate.participants().iter().any(|id| seeds.contains(*id)) {
        return Ok(None);
    }
    let outside = mate.participants().iter().any(|id| !seeds.contains(*id));
    let external = intent.external.iter().find(|entry| entry.source == mate.id);
    if outside && !external.is_some_and(|entry| entry.enabled) {
        return Ok(None);
    }
    let mut copied = mate.clone();
    copied.id = reservation
        .mates
        .get(&mate.id)
        .cloned()
        .ok_or_else(|| invalid("Copied mate identity reservation is missing"))?;
    (copied.first, copied.first_reference) = rebound(
        baseline,
        &seeds,
        &reservation.instances,
        external,
        Slot::First,
        &mate.first,
        &mate.first_reference,
    )?;
    (copied.second, copied.second_reference) = rebound(
        baseline,
        &seeds,
        &reservation.instances,
        external,
        Slot::Second,
        &mate.second,
        &mate.second_reference,
    )?;
    if copied.kind == "lock" && copied.lock_placement.is_none() {
        copied.lock_placement = Some(crate::assembly_geometry::relative_placement(
            &baseline.assembly.instances[&mate.first].placement,
            &baseline.assembly.instances[&mate.second].placement,
        )?);
    }
    if let Some(relationship) = copied.relationship.as_deref_mut() {
        for (index, endpoint) in relationship.endpoints_mut().into_iter().enumerate() {
            let (id, reference) = rebound(
                baseline,
                &seeds,
                &reservation.instances,
                external,
                Slot::Auxiliary { index },
                &endpoint.instance,
                &Some(endpoint.reference.clone()),
            )?;
            endpoint.instance = id;
            endpoint.reference = reference
                .ok_or_else(|| invalid("Auxiliary mate endpoint lost its geometry reference"))?;
        }
        if let Relationship::Coupler { support, zero, .. } = relationship {
            let old_support = support.clone();
            if let Some(id) = old_support.as_ref() {
                let (new, _) = rebound(
                    baseline,
                    &seeds,
                    &reservation.instances,
                    external,
                    Slot::Support,
                    id,
                    &None,
                )?;
                *support = Some(new);
            }
            let old = old_support
                .as_ref()
                .map(|id| baseline.assembly.instances[id].placement.matrix())
                .unwrap_or_else(Matrix4::identity);
            let new = support
                .as_ref()
                .map(|id| {
                    placements
                        .get(id)
                        .map(Placement::matrix)
                        .unwrap_or_else(|| baseline.assembly.instances[id].placement.matrix())
                })
                .unwrap_or_else(Matrix4::identity);
            let transformed = new
                .try_inverse()
                .ok_or_else(|| invalid("Rebound mechanical support pose is singular"))?
                * transform
                * old;
            for frame in zero {
                *frame = frame.transformed(&transformed)?;
            }
        }
        let diagnostic = baseline
            .solution
            .diagnostics
            .get(&mate.id)
            .and_then(|diagnostic| diagnostic.curve_parameter);
        let closed = baseline
            .solution
            .frames
            .get(&mate.id)
            .and_then(|pair| pair[0].curve.as_deref())
            .is_some_and(|curve| curve.closed());
        match relationship {
            Relationship::Path {
                position: crate::assembly_geometry::CurvePosition::Free { seed },
                ..
            }
            | Relationship::Slot {
                position: crate::assembly_geometry::CurvePosition::Free { seed },
                ..
            }
            | Relationship::Cam {
                position: crate::assembly_geometry::CurvePosition::Free { seed },
                ..
            } => {
                if let Some(value) = diagnostic {
                    *seed = if closed {
                        value.rem_euclid(1.)
                    } else {
                        value.clamp(0., 1.)
                    };
                }
            }
            _ => {}
        }
    }
    if copied.first == copied.second {
        return Err(invalid(
            "Rebinding would create a mate from a component to itself",
        ));
    }
    Ok(Some(copied))
}
#[derive(Clone, Debug)]
pub struct Built {
    pub document: Document,
    pub inserted: Vec<Id>,
    pub mates: Vec<Id>,
    pub skipped_external: usize,
}
pub fn build(baseline: &Baseline, intent: &Intent) -> Result<Built> {
    validate_header(baseline, intent)?;
    let copies = count(baseline, intent)?;
    let mut document = baseline.source.clone();
    let mut placements = BTreeMap::new();
    let mut inserted = vec![];
    let mut mates = vec![];
    let mut ids = document.assemblies[&intent.assembly]
        .instances
        .keys()
        .chain(
            document.assemblies[&intent.assembly]
                .mates
                .iter()
                .map(|mate| &mate.id),
        )
        .cloned()
        .collect::<BTreeSet<_>>();
    for index in 1..=copies {
        let reservation = intent
            .reservations
            .iter()
            .find(|reservation| reservation.index == index)
            .ok_or_else(|| invalid("Reserve component/mate identities before preview"))?;
        let transform = delta(baseline, intent, index)?;
        for seed in &intent.seeds {
            let mut instance = baseline.assembly.instances[seed].clone();
            instance.id = reservation
                .instances
                .get(seed)
                .cloned()
                .ok_or_else(|| invalid("Copied occurrence reservation is missing"))?;
            if !ids.insert(instance.id.clone()) {
                return Err(invalid(
                    "Copied component identity collides with existing source",
                ));
            }
            instance.name = format!("{} Copy{}", instance.name, index);
            instance.placement = placement(transform * instance.placement.matrix())?;
            instance.fixed = intent.keep_fixed && instance.fixed;
            instance.translation_expressions = None;
            placements.insert(instance.id.clone(), instance.placement.clone());
            inserted.push(instance.id.clone());
            document
                .assemblies
                .get_mut(&intent.assembly)
                .ok_or_else(|| invalid("Owning assembly is missing"))?
                .instances
                .insert(instance.id.clone(), instance);
        }
        for mate in &baseline.assembly.mates {
            if let Some(copied) =
                copy_mate(baseline, intent, mate, reservation, &transform, &placements)?
            {
                if !ids.insert(copied.id.clone()) {
                    return Err(invalid(
                        "Copied mate identity collides with existing source",
                    ));
                }
                mates.push(copied.id.clone());
                document
                    .assemblies
                    .get_mut(&intent.assembly)
                    .ok_or_else(|| invalid("Owning assembly is missing"))?
                    .mates
                    .push(copied);
            }
        }
    }
    // Named scope holds captured pose hints without changing any existing seed.
    // Occurrences and mates are still shared assembly source, not named-only.
    if let PlacementScope::Named(_) = &intent.scope {
        let poses = placements
            .iter()
            .map(|(id, pose)| {
                (
                    id.clone(),
                    PoseOverride {
                        translation: pose.translation.map(Some),
                        rotation_deg: pose.rotation_deg.map(Some),
                    },
                )
            })
            .collect();
        configuration::set_component_placements(
            &mut document,
            &intent.assembly,
            &intent.scope,
            &poses,
            &BTreeMap::new(),
        )?;
    }
    document.validate()?;
    let skipped_external = intent.external.iter().filter(|mate| !mate.enabled).count();
    Ok(Built {
        document,
        inserted,
        mates,
        skipped_external,
    })
}
