//! Native associative assembly occurrence patterns and copied internal mates.
use crate::{
    Assembly, CadError, Document, Expr, Id, Mate, Placement, Result,
    assembly_geometry::{Frame, Reference},
    configuration::Overrides,
};
use nalgebra::{Matrix4, Rotation3, Vector3};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
pub const CONTRACT_VERSION: u32 = 1;
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct AxisReference {
    pub instance: Id,
    pub geometry: Reference,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Mode {
    Linear {
        step: [Expr; 3],
    },
    Circular {
        origin: [Expr; 3],
        axis: [f64; 3],
        angle: Expr,
        #[serde(default)]
        fixed_orientation: bool,
        #[serde(default)]
        axis_reference: Option<Box<AxisReference>>,
    },
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Occurrence {
    pub index: usize,
    pub seed: Id,
    pub instance: Id,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct MateCopy {
    pub index: usize,
    pub source: Id,
    pub mate: Id,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Definition {
    pub version: u32,
    pub id: Id,
    pub name: String,
    pub seeds: Vec<Id>,
    pub count: Expr,
    pub mode: Mode,
    #[serde(default)]
    pub copy_internal_mates: bool,
    /// Reservations survive count reduction, suppression, and later restoration.
    #[serde(default)]
    pub occurrences: Vec<Occurrence>,
    #[serde(default)]
    pub mates: Vec<MateCopy>,
}
impl Definition {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            version: CONTRACT_VERSION,
            id: crate::id(),
            name: name.into(),
            seeds: Vec::new(),
            count: "3".into(),
            mode: Mode::Linear {
                step: ["30 mm".into(), "0 mm".into(), "0 mm".into()],
            },
            copy_internal_mates: false,
            occurrences: Vec::new(),
            mates: Vec::new(),
        }
    }
}
fn invalid(s: impl Into<String>) -> CadError {
    CadError::Invalid(format!("Component Pattern: {}", s.into()))
}
pub fn count(doc: &Document, assembly: &str, pattern: &Definition) -> Result<usize> {
    let n = crate::expression::value(doc, assembly, &pattern.count, crate::Dimension::Scalar)?;
    if !(1. ..=256.).contains(&n) || n.fract() != 0. {
        return Err(invalid(
            "Count must be an integer from 1 to 256, including the seed",
        ));
    }
    Ok(n as usize)
}
pub fn generated(assembly: &Assembly) -> BTreeSet<Id> {
    assembly
        .component_patterns
        .values()
        .flat_map(|p| p.occurrences.iter().map(|e| e.instance.clone()))
        .collect()
}
pub fn order(assembly: &Assembly) -> Result<Vec<Id>> {
    fn visit(
        a: &Assembly,
        id: &str,
        active: &mut BTreeSet<Id>,
        done: &mut BTreeSet<Id>,
        out: &mut Vec<Id>,
    ) -> Result<()> {
        if done.contains(id) {
            return Ok(());
        }
        if !active.insert(id.into()) {
            return Err(invalid("Pattern seed/reference dependency cycle"));
        }
        let p = &a.component_patterns[id];
        let anchor = if let Mode::Circular {
            axis_reference: Some(reference),
            ..
        } = &p.mode
        {
            Some(&reference.instance)
        } else {
            None
        };
        for seed in p.seeds.iter().chain(anchor) {
            if let Some(parent) = a
                .component_patterns
                .values()
                .find(|p| p.occurrences.iter().any(|e| &e.instance == seed))
            {
                visit(a, &parent.id, active, done, out)?;
            }
        }
        active.remove(id);
        done.insert(id.into());
        out.push(id.into());
        Ok(())
    }
    let mut out = Vec::new();
    let mut done = BTreeSet::new();
    let mut active = BTreeSet::new();
    for id in assembly.component_patterns.keys() {
        visit(assembly, id, &mut active, &mut done, &mut out)?;
    }
    Ok(out)
}
pub fn validate(doc: &Document) -> Result<()> {
    for a in doc.assemblies.values() {
        let mut occurrences = BTreeSet::new();
        let mut mate_ids = BTreeSet::new();
        for (id, p) in &a.component_patterns {
            if id != &p.id
                || p.version != CONTRACT_VERSION
                || p.name.trim().is_empty()
                || p.seeds.is_empty()
                || p.seeds.len() > 256
            {
                return Err(invalid(
                    "Invalid pattern identity, version, name or seed set",
                ));
            }
            let seeds = p.seeds.iter().cloned().collect::<BTreeSet<_>>();
            if seeds.len() != p.seeds.len() || p.seeds.iter().any(|s| !a.instances.contains_key(s))
            {
                return Err(invalid("Seed components are missing or duplicated"));
            }
            let mut positions = BTreeSet::new();
            for entry in &p.occurrences {
                if entry.index == 0
                    || entry.index >= 256
                    || !seeds.contains(&entry.seed)
                    || !positions.insert((entry.index, entry.seed.clone()))
                    || !occurrences.insert(entry.instance.clone())
                    || !a.instances.contains_key(&entry.instance)
                    || seeds.contains(&entry.instance)
                {
                    return Err(invalid(
                        "Generated occurrence identity is missing, duplicated, or cyclic",
                    ));
                }
            }
            for entry in &p.mates {
                if entry.index == 0
                    || entry.index >= 256
                    || !mate_ids.insert(entry.mate.clone())
                    || a.mates.iter().any(|m| m.id == entry.mate)
                    || !a.mates.iter().any(|m| m.id == entry.source)
                {
                    return Err(invalid(
                        "Copied mate source or reserved identity is invalid",
                    ));
                }
            }
            if let Mode::Circular {
                axis,
                axis_reference,
                ..
            } = &p.mode
            {
                let n = Vector3::from(*axis);
                if axis.iter().any(|v| !v.is_finite()) || !n.norm().is_finite() || n.norm() < 1e-12
                {
                    return Err(invalid("Circular axis direction is invalid"));
                }
                if let Some(reference) = axis_reference {
                    let i = a
                        .instances
                        .get(&reference.instance)
                        .ok_or_else(|| invalid("Circular pattern axis component is missing"))?;
                    crate::assembly_geometry::validate_reference(
                        doc,
                        &i.definition,
                        &reference.geometry,
                    )?;
                }
            }
        }
        order(a)?;
        // A pattern is the placement driver for its generated occurrences. External
        // mates would feed solved generated placement back into that driver.
        for mate in &a.mates {
            if mate
                .participants()
                .iter()
                .any(|id| occurrences.contains(*id))
            {
                return Err(CadError::Unsupported("Mates on pattern-generated occurrences require coupled pattern constraints. Mate the seeds or use the pattern's copied internal mates".into()));
            }
        }
    }
    Ok(())
}
/// Reserve native occurrence and copied-mate identities before handing source to a worker.
pub fn put(doc: &mut Document, assembly: &str, mut pattern: Definition) -> Result<()> {
    let n = maximum_count(doc, assembly, &pattern)?;
    let a = doc
        .assemblies
        .get_mut(assembly)
        .ok_or_else(|| invalid("Assembly is missing"))?;
    if let Some(previous) = a.component_patterns.get(&pattern.id).cloned() {
        for entry in &previous.occurrences {
            if pattern.seeds.contains(&entry.seed)
                && !pattern
                    .occurrences
                    .iter()
                    .any(|e| e.index == entry.index && e.seed == entry.seed)
            {
                pattern.occurrences.push(entry.clone());
            }
        }
        for entry in &previous.mates {
            if a.mates.iter().any(|m| m.id == entry.source)
                && !pattern
                    .mates
                    .iter()
                    .any(|e| e.index == entry.index && e.source == entry.source)
            {
                pattern.mates.push(entry.clone());
            }
        }
        let removed = previous
            .occurrences
            .iter()
            .filter(|e| !pattern.seeds.contains(&e.seed))
            .map(|e| e.instance.clone())
            .collect::<BTreeSet<_>>();
        if a.component_patterns.values().any(|other| other.id != pattern.id && (other.seeds.iter().any(|seed| removed.contains(seed)) || matches!(&other.mode, Mode::Circular { axis_reference: Some(r), .. } if removed.contains(&r.instance)))) {
            return Err(invalid("A dependent pattern references an occurrence of the removed seed; update that pattern first"));
        }
        a.instances.retain(|id, _| !removed.contains(id));
        for c in a.configurations.values_mut() {
            c.overrides.components.retain(|id, _| !removed.contains(id));
            c.overrides
                .component_configurations
                .retain(|id, _| !removed.contains(id));
        }
        let removed_mates = previous
            .mates
            .iter()
            .filter(|e| !a.mates.iter().any(|m| m.id == e.source))
            .map(|e| e.mate.clone())
            .collect::<BTreeSet<_>>();
        for c in a.configurations.values_mut() {
            c.overrides
                .mates
                .retain(|id, _| !removed_mates.contains(id));
        }
    }
    pattern
        .occurrences
        .retain(|e| pattern.seeds.contains(&e.seed));
    pattern
        .mates
        .retain(|e| a.mates.iter().any(|m| m.id == e.source));
    if n.saturating_sub(1).saturating_mul(pattern.seeds.len()) > 4096 {
        return Err(invalid(
            "A pattern supports at most 4096 generated occurrences",
        ));
    }
    for index in 1..n {
        for seed in &pattern.seeds {
            let seed_instance = a
                .instances
                .get(seed)
                .cloned()
                .ok_or_else(|| invalid("Seed component is missing"))?;
            let id = if let Some(entry) = pattern
                .occurrences
                .iter()
                .find(|e| e.index == index && &e.seed == seed)
            {
                entry.instance.clone()
            } else {
                let instance = crate::id();
                pattern.occurrences.push(Occurrence {
                    index,
                    seed: seed.clone(),
                    instance: instance.clone(),
                });
                instance
            };
            let mut instance = seed_instance;
            instance.id = id.clone();
            instance.name = format!("{} · {}", instance.name, index + 1);
            instance.translation_expressions = None;
            if let Some(existing) = a.instances.get_mut(&id) {
                existing.definition = instance.definition;
                existing.name = instance.name;
                existing.configuration = instance.configuration;
            } else {
                a.instances.insert(id, instance);
            }
        }
        for mate in &a.mates {
            if pattern.seeds.contains(&mate.first)
                && pattern.seeds.contains(&mate.second)
                && !pattern
                    .mates
                    .iter()
                    .any(|e| e.index == index && e.source == mate.id)
            {
                pattern.mates.push(MateCopy {
                    index,
                    source: mate.id.clone(),
                    mate: crate::id(),
                });
            }
        }
    }
    a.component_patterns.insert(pattern.id.clone(), pattern);
    Ok(())
}
pub fn remove(doc: &mut Document, assembly: &str, id: &str) -> Result<()> {
    let a = doc
        .assemblies
        .get_mut(assembly)
        .ok_or_else(|| invalid("Assembly is missing"))?;
    let p = a
        .component_patterns
        .get(id)
        .cloned()
        .ok_or_else(|| invalid("Pattern is missing"))?;
    let ids = p
        .occurrences
        .iter()
        .map(|e| e.instance.clone())
        .collect::<BTreeSet<_>>();
    if a.component_patterns
        .values()
        .any(|other| other.id != id && (other.seeds.iter().any(|seed| ids.contains(seed)) || matches!(&other.mode, Mode::Circular { axis_reference: Some(r), .. } if ids.contains(&r.instance))))
    {
        return Err(invalid(
            "Delete dependent patterns before removing their seed or axis pattern",
        ));
    }
    a.component_patterns.remove(id);
    a.instances.retain(|id, _| !ids.contains(id));
    for c in a.configurations.values_mut() {
        c.overrides.component_patterns.remove(id);
        for mate in &p.mates {
            c.overrides.mates.remove(&mate.mate);
            c.overrides.mate_curve_seeds.remove(&mate.mate);
        }
        for id in &ids {
            c.overrides.components.remove(id);
            c.overrides.component_configurations.remove(id);
            c.overrides.component_placements.remove(id);
        }
    }
    Ok(())
}
fn placement(matrix: Matrix4<f64>) -> Result<Placement> {
    if matrix.iter().any(|v| !v.is_finite()) {
        return Err(invalid("Pattern placement is not finite"));
    }
    let rotation = Rotation3::from_matrix_unchecked(matrix.fixed_view::<3, 3>(0, 0).into_owned());
    let (x, y, z) = rotation.euler_angles();
    Ok(Placement {
        translation: [matrix[(0, 3)], matrix[(1, 3)], matrix[(2, 3)]],
        rotation_deg: [x, y, z].map(f64::to_degrees),
    })
}
pub fn transform(
    doc: &Document,
    owner: &str,
    p: &Definition,
    index: usize,
    frame: Option<&Frame>,
) -> Result<Matrix4<f64>> {
    match &p.mode {
        Mode::Linear { step } => {
            let mut m = Matrix4::identity();
            for axis in 0..3 {
                m[(axis, 3)] =
                    crate::expression::value(doc, owner, &step[axis], crate::Dimension::Length)?
                        * index as f64;
            }
            Ok(m)
        }
        Mode::Circular {
            origin,
            axis,
            angle,
            ..
        } => {
            let mut center = [0.; 3];
            for i in 0..3 {
                center[i] =
                    crate::expression::value(doc, owner, &origin[i], crate::Dimension::Length)?;
            }
            let direction = frame.map(|f| f.axis).unwrap_or(*axis);
            let center = Vector3::from(frame.map(|f| f.origin).unwrap_or(center));
            let axis = Vector3::from(direction)
                .try_normalize(1e-12)
                .ok_or_else(|| invalid("Circular axis is zero"))?;
            let angle = crate::expression::value(doc, owner, angle, crate::Dimension::Angle)?
                * index as f64;
            let r = Rotation3::from_axis_angle(&nalgebra::Unit::new_normalize(axis), angle);
            let mut matrix = r.to_homogeneous();
            let shift = center - r * center;
            for i in 0..3 {
                matrix[(i, 3)] = shift[i];
            }
            Ok(matrix)
        }
    }
}
/// Called after the seed assembly solves. Axis frames are exact kernel-owned
/// inputs behind an application-owned f64 frame; source traversal may omit them.
pub fn expand(
    doc: &Document,
    assembly: &mut Assembly,
    p: &Definition,
    overrides: &Overrides,
    frame: Option<&Frame>,
) -> Result<()> {
    if overrides
        .component_patterns
        .get(&p.id)
        .copied()
        .unwrap_or(false)
    {
        return Ok(());
    }
    let n = count(doc, &assembly.id, p)?;
    if (1..n).any(|index| {
        p.seeds.iter().any(|seed| {
            !p.occurrences
                .iter()
                .any(|e| e.index == index && &e.seed == seed)
        })
    }) {
        return Err(invalid(
            "The current count needs native occurrence reservations; edit the count through the document command API",
        ));
    }
    for entry in p.occurrences.iter().filter(|e| e.index < n) {
        if overrides
            .components
            .get(&entry.instance)
            .copied()
            .unwrap_or(false)
        {
            continue;
        }
        let Some(seed) = assembly.instances.get(&entry.seed).cloned() else {
            continue;
        };
        let mut instance = seed.clone();
        instance.id = entry.instance.clone();
        instance.name = format!("{} · {}", seed.name, entry.index + 1);
        instance.translation_expressions = None;
        if let Some(configuration) = overrides.component_configurations.get(&entry.instance) {
            instance.configuration = configuration.clone();
        }
        let matrix = transform(doc, &assembly.id, p, entry.index, frame)?;
        let mut target = matrix * seed.placement.matrix();
        if matches!(
            p.mode,
            Mode::Circular {
                fixed_orientation: true,
                ..
            }
        ) {
            target
                .fixed_view_mut::<3, 3>(0, 0)
                .copy_from(&seed.placement.matrix().fixed_view::<3, 3>(0, 0));
        }
        instance.placement = placement(target)?;
        assembly.instances.insert(instance.id.clone(), instance);
    }
    Ok(())
}
pub fn internal_mates(assembly: &Assembly, p: &Definition) -> Result<Vec<Mate>> {
    if !p.copy_internal_mates {
        return Ok(Vec::new());
    }
    let mut result = Vec::new();
    for reservation in &p.mates {
        let Some(source) = assembly.mates.iter().find(|m| m.id == reservation.source) else {
            continue;
        };
        let first = p
            .occurrences
            .iter()
            .find(|e| e.index == reservation.index && e.seed == source.first);
        let second = p
            .occurrences
            .iter()
            .find(|e| e.index == reservation.index && e.seed == source.second);
        if let (Some(first), Some(second)) = (first, second)
            && assembly.instances.contains_key(&first.instance)
            && assembly.instances.contains_key(&second.instance)
        {
            let mut mate = source.clone();
            mate.id = reservation.mate.clone();
            mate.first = first.instance.clone();
            mate.second = second.instance.clone();
            if let Some(relationship) = mate.relationship.as_deref_mut() {
                for endpoint in relationship.endpoints_mut() {
                    if let Some(copy) = p
                        .occurrences
                        .iter()
                        .find(|e| e.index == reservation.index && e.seed == endpoint.instance)
                    {
                        endpoint.instance = copy.instance.clone();
                    }
                }
                if let crate::assembly_geometry::Relationship::Coupler { support, zero, .. } =
                    relationship
                {
                    if let Some(copy) = support.as_ref().and_then(|id| {
                        p.occurrences
                            .iter()
                            .find(|e| e.index == reservation.index && e.seed == *id)
                    }) {
                        *support = Some(copy.instance.clone());
                    } else {
                        // A universe/external-support zero frame moves with its copied group.
                        let seed = assembly
                            .instances
                            .get(&source.first)
                            .ok_or_else(|| invalid("Copied mate's seed occurrence is missing"))?;
                        let copied = assembly
                            .instances
                            .get(&first.instance)
                            .ok_or_else(|| invalid("Copied mate's target occurrence is missing"))?;
                        let inverse =
                            seed.placement.matrix().try_inverse().ok_or_else(|| {
                                invalid("Copied mate's seed placement is singular")
                            })?;
                        let world = copied.placement.matrix() * inverse;
                        let transform = if let Some(id) = support {
                            let placement = assembly
                                .instances
                                .get(id)
                                .ok_or_else(|| {
                                    invalid("Copied mate's support occurrence is missing")
                                })?
                                .placement
                                .matrix();
                            placement.try_inverse().ok_or_else(|| {
                                invalid("Copied mate's support placement is singular")
                            })? * world
                                * placement
                        } else {
                            world
                        };
                        for frame in zero {
                            *frame = frame.transformed(&transform)?;
                        }
                    }
                }
            }
            result.push(mate);
        }
    }
    Ok(result)
}
pub fn prepare(doc: &Document, assembly: &Assembly, overrides: &Overrides) -> Result<Assembly> {
    let mut a = assembly.clone();
    let generated = generated(&a);
    a.instances.retain(|id, _| !generated.contains(id));
    for id in order(&a)? {
        let p = a.component_patterns[&id].clone();
        expand(doc, &mut a, &p, overrides, None)?;
    }
    Ok(a)
}

fn maximum_count(doc: &Document, owner: &str, pattern: &Definition) -> Result<usize> {
    let mut context = doc.clone();
    for (definition, configuration) in &doc.active_configurations {
        let resolved = crate::configuration::resolve(doc, definition, configuration)?;
        let parameters = if let Some(p) = context.parts.get_mut(definition) {
            &mut p.parameters
        } else if let Some(a) = context.assemblies.get_mut(definition) {
            &mut a.parameters
        } else {
            return Err(invalid("Configured pattern parameter scope is missing"));
        };
        for (id, expression) in resolved.overrides.parameters {
            parameters
                .get_mut(&id)
                .ok_or_else(|| invalid("Configured pattern parameter is missing"))?
                .expression = expression;
        }
    }
    context
        .assemblies
        .get_mut(owner)
        .ok_or_else(|| invalid("Pattern assembly is missing"))?
        .parameters = doc.assemblies[owner].parameters.clone();
    let mut maximum = count(&context, owner, pattern)?;
    for configuration in crate::configuration::names(doc, owner).keys() {
        let mut variant = context.clone();
        let a = variant
            .assemblies
            .get_mut(owner)
            .ok_or_else(|| invalid("Pattern assembly is missing"))?;
        a.parameters = doc.assemblies[owner].parameters.clone();
        let resolved = crate::configuration::resolve(doc, owner, configuration)?;
        for (id, expression) in resolved.overrides.parameters {
            a.parameters
                .get_mut(&id)
                .ok_or_else(|| invalid("Configured pattern count parameter is missing"))?
                .expression = expression;
        }
        maximum = maximum.max(count(&variant, owner, pattern)?);
    }
    Ok(maximum)
}
/// Native commands reserve any newly driven count before worker evaluation. The
/// expression remains authoritative; growing and shrinking reuses every old ID.
pub fn synchronize(doc: &mut Document) -> Result<()> {
    let patterns = doc
        .assemblies
        .iter()
        .flat_map(|(owner, a)| {
            a.component_patterns
                .values()
                .map(|p| (owner.clone(), p.clone()))
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    for (owner, pattern) in patterns {
        put(doc, &owner, pattern)?;
    }
    Ok(())
}
