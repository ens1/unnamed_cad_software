//! Saved rigid component intent projected into existing exact lock-mate equations.
use crate::{
    Assembly, CadError, Document, Id, Mate, Placement, Result, configuration, protocol::Evaluation,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
pub const CONTRACT_VERSION: u32 = 1;
fn version() -> u32 {
    CONTRACT_VERSION
}
fn invalid(message: impl Into<String>) -> CadError {
    CadError::Invalid(format!("Rigid Group: {}", message.into()))
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Capture {
    pub source_fingerprint: String,
    pub backend: String,
    pub configuration: Option<Id>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Definition {
    #[serde(default = "version")]
    pub version: u32,
    pub id: Id,
    pub name: String,
    pub anchor: Id,
    /// Captured component-origin transforms in the anchor's frame, mm/XYZ degrees.
    pub members: BTreeMap<Id, Placement>,
    #[serde(default)]
    pub suppressed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capture: Option<Capture>,
}
impl Definition {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            version: CONTRACT_VERSION,
            id: crate::id(),
            name: name.into(),
            anchor: String::new(),
            members: BTreeMap::new(),
            suppressed: false,
            capture: None,
        }
    }
    pub fn validate(&self) -> Result<()> {
        if self.version != CONTRACT_VERSION
            || self.id.is_empty()
            || self.id.len() > 256
            || self
                .id
                .chars()
                .any(|c| c.is_control() || matches!(c, '/' | '\\' | ':'))
            || self.name.trim().is_empty()
            || self.name.len() > 256
            || self.name.chars().any(char::is_control)
            || !(2..=128).contains(&self.members.len())
            || !self.members.contains_key(&self.anchor)
        {
            return Err(invalid(
                "Groups need supported version, portable identity, name, anchor and 2–128 distinct members",
            ));
        }
        for (id, pose) in &self.members {
            if id.is_empty()
                || pose
                    .translation
                    .iter()
                    .chain(&pose.rotation_deg)
                    .any(|v| !v.is_finite())
            {
                return Err(invalid("Captured component frames must be finite"));
            }
        }
        if (self.members[&self.anchor].matrix() - nalgebra::Matrix4::identity()).norm() > 1e-8 {
            return Err(invalid("The anchor-relative anchor frame must be identity"));
        }
        Ok(())
    }
}
/// Distinct source pairs retain IDs when other members are inserted or removed.
pub fn link_id(group: &str, member: &str) -> Id {
    format!(
        "rigid-group-{:x}",
        Sha256::digest(serde_json::to_vec(&(group, member)).unwrap_or_default())
    )
}
fn base_validation(assembly: &Assembly) -> Result<()> {
    let generated = crate::component_pattern::generated(assembly);
    let mut names = BTreeSet::new();
    let mut assigned = BTreeSet::new();
    if assembly.rigid_groups.len() > 4096 {
        return Err(invalid("At most 4096 groups per assembly"));
    }
    for (id, group) in &assembly.rigid_groups {
        group.validate()?;
        if id != &group.id || !names.insert(group.name.trim().to_lowercase()) {
            return Err(invalid("Saved group identities and names must be distinct"));
        }
        for member in group.members.keys() {
            if !assembly.instances.contains_key(member) || generated.contains(member) {
                return Err(invalid(
                    "Group members must be existing ordinary direct components; pattern-owned copies follow their seed",
                ));
            }
            if !assigned.insert(member) {
                return Err(invalid(
                    "A component may belong to one saved rigid group; dissolve or edit its existing group first",
                ));
            }
            if assembly
                .mates
                .iter()
                .any(|mate| mate.id == link_id(id, member))
            {
                return Err(invalid(
                    "A native mate collides with a reserved rigid-group constraint identity",
                ));
            }
        }
    }
    Ok(())
}
pub fn validate(doc: &Document) -> Result<()> {
    for assembly in doc.assemblies.values() {
        base_validation(assembly)?;
    }
    Ok(())
}
/// Capture is exclusively from current source-matched evaluated poses.
pub fn capture(
    doc: &Document,
    evaluation: &Evaluation,
    assembly: &str,
    mut source: Definition,
    members: &[Id],
    anchor: &str,
) -> Result<Definition> {
    if !evaluation.valid || evaluation.identity.source != doc.fingerprint() {
        return Err(invalid(
            "Capture requires the current validated source evaluation",
        ));
    }
    let current = doc
        .assemblies
        .get(assembly)
        .ok_or_else(|| invalid("Owning assembly is missing"))?;
    let configuration = doc.active_configurations.get(assembly);
    let scoped = configuration::scope(doc, doc, assembly, configuration.map(String::as_str))?;
    let active = &scoped.assemblies[assembly];
    let solved = evaluation
        .assemblies
        .get(&configuration::key(
            assembly,
            configuration.map(String::as_str),
        ))
        .filter(|solved| solved.version == crate::assembly_geometry::CONTRACT_VERSION)
        .ok_or_else(|| invalid("Configured assembly has no current solved poses"))?;
    let generated = crate::component_pattern::generated(current);
    let unique = members.iter().cloned().collect::<BTreeSet<_>>();
    if unique.len() != members.len()
        || !(2..=128).contains(&unique.len())
        || !unique.contains(anchor)
    {
        return Err(invalid(
            "Choose 2–128 unique direct components and one of them as anchor",
        ));
    }
    if unique
        .iter()
        .any(|member| !active.instances.contains_key(member) || generated.contains(member))
    {
        return Err(invalid(
            "Captured members must be active ordinary components in this configuration",
        ));
    }
    if current.rigid_groups.values().any(|group| {
        group.id != source.id && group.members.keys().any(|member| unique.contains(member))
    }) {
        return Err(invalid(
            "A selected component already belongs to another saved rigid group; edit or dissolve that group first",
        ));
    }
    let anchor_pose = solved
        .placements
        .get(anchor)
        .ok_or_else(|| invalid("Anchor solved pose is missing"))?;
    source.anchor = anchor.into();
    source.members = unique
        .into_iter()
        .map(|member| {
            let pose = solved
                .placements
                .get(&member)
                .ok_or_else(|| invalid("A solved component pose is missing"))?;
            Ok((
                member,
                crate::assembly_geometry::relative_placement(anchor_pose, pose)?,
            ))
        })
        .collect::<Result<_>>()?;
    source.members.insert(anchor.into(), Placement::default());
    source.capture = Some(Capture {
        source_fingerprint: evaluation.identity.source.clone(),
        backend: evaluation.identity.backend.clone(),
        configuration: configuration.cloned(),
    });
    source.validate()?;
    Ok(source)
}
/// Re-express the same rigid body in another member's captured frame.
pub fn reanchor(source: &mut Definition, anchor: &str) -> Result<()> {
    let origin = source
        .members
        .get(anchor)
        .cloned()
        .ok_or_else(|| invalid("New anchor must belong to this group"))?;
    source.members = source
        .members
        .iter()
        .map(|(id, pose)| {
            Ok((
                id.clone(),
                crate::assembly_geometry::relative_placement(&origin, pose)?,
            ))
        })
        .collect::<Result<_>>()?;
    source.members.insert(anchor.into(), Placement::default());
    source.anchor = anchor.into();
    source.validate()
}
pub fn put(doc: &mut Document, assembly: &str, source: Definition) -> Result<()> {
    source.validate()?;
    let mut candidate = doc.clone();
    candidate
        .assemblies
        .get_mut(assembly)
        .ok_or_else(|| invalid("Owning assembly is missing"))?
        .rigid_groups
        .insert(source.id.clone(), source);
    candidate.validate()?;
    *doc = candidate;
    Ok(())
}
pub fn remove(doc: &mut Document, assembly: &str, id: &str) -> Result<()> {
    let owner = doc
        .assemblies
        .get_mut(assembly)
        .ok_or_else(|| invalid("Owning assembly is missing"))?;
    if owner.rigid_groups.remove(id).is_none() {
        return Err(invalid("Saved group is missing"));
    }
    for config in owner.configurations.values_mut() {
        config.overrides.rigid_groups.remove(id);
    }
    Ok(())
}
pub fn set_suppressed(
    doc: &mut Document,
    assembly: &str,
    id: &str,
    suppressed: bool,
    scope: &configuration::PlacementScope,
) -> Result<()> {
    let mut candidate = doc.clone();
    let owner = candidate
        .assemblies
        .get_mut(assembly)
        .ok_or_else(|| invalid("Owning assembly is missing"))?;
    if !owner.rigid_groups.contains_key(id) {
        return Err(invalid("Saved group is missing"));
    }
    match scope {
        configuration::PlacementScope::Base => {
            owner.rigid_groups.get_mut(id).unwrap().suppressed = suppressed
        }
        configuration::PlacementScope::Named(key) => {
            let config = owner
                .configurations
                .get_mut(key)
                .ok_or_else(|| invalid("Named configuration is missing"))?;
            config.version = configuration::CONTRACT_VERSION;
            config.overrides.rigid_groups.insert(id.into(), suppressed);
        }
    }
    candidate.validate()?;
    *doc = candidate;
    Ok(())
}
/// Configured projection: suppressed members become inactive, while stored
/// frames stay unchanged. Repeated preparation replaces only this projection's
/// reserved IDs; no generated constraints are ever written to source documents.
pub fn prepared(source: &Assembly) -> Result<Assembly> {
    let mut result = source.clone();
    let reserved = source
        .rigid_groups
        .values()
        .flat_map(|group| {
            group
                .members
                .keys()
                .map(|member| link_id(&group.id, member))
        })
        .collect::<BTreeSet<_>>();
    result.mates.retain(|mate| !reserved.contains(&mate.id));
    for group in source
        .rigid_groups
        .values()
        .filter(|group| !group.suppressed)
    {
        group.validate()?;
        let active = group
            .members
            .keys()
            .filter(|member| source.instances.contains_key(*member))
            .collect::<Vec<_>>();
        let Some(anchor) = active
            .iter()
            .find(|member| member.as_str() == group.anchor)
            .copied()
            .or_else(|| active.first().copied())
        else {
            continue;
        };
        if active.len() < 2 {
            continue;
        }
        let first = &group.members[anchor];
        for member in active.into_iter().filter(|member| *member != anchor) {
            let relative =
                crate::assembly_geometry::relative_placement(first, &group.members[member])?;
            result.mates.push(Mate {
                id: link_id(&group.id, member),
                kind: "lock".into(),
                first: anchor.clone(),
                second: member.clone(),
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
    }
    Ok(result)
}
/// Actual solver rows are projected with group-owned source labels.
pub fn semantic(
    doc: &Document,
    evaluation: Option<&Evaluation>,
    assembly: &str,
) -> Vec<crate::presentation::SemanticElement> {
    let Some(owner) = doc.assemblies.get(assembly) else {
        return vec![];
    };
    let config = doc.active_configurations.get(assembly);
    let scoped = configuration::scope(doc, doc, assembly, config.map(String::as_str)).ok();
    let current = evaluation
        .filter(|evaluation| evaluation.valid && evaluation.identity.source == doc.fingerprint())
        .and_then(|evaluation| {
            evaluation
                .assemblies
                .get(&configuration::key(assembly, config.map(String::as_str)))
        });
    let mut out=owner.rigid_groups.values().map(|group| {
        let effective=scoped.as_ref().and_then(|doc|doc.assemblies.get(assembly)).and_then(|assembly|assembly.rigid_groups.get(&group.id)).unwrap_or(group);
        let active=effective.members.keys().filter(|member|scoped.as_ref().and_then(|doc|doc.assemblies.get(assembly)).is_some_and(|assembly|assembly.instances.contains_key(*member))).cloned().collect::<Vec<_>>();
        let links=current.map(|current|group.members.keys().filter_map(|member|current.diagnostics.get(&link_id(&group.id,member)).map(|diagnostic|(member,diagnostic))).collect::<BTreeMap<_,_>>());
        let motion=current.map(|current|active.iter().filter_map(|member|current.component_motion.get(member).map(|motion|(member,motion))).collect::<BTreeMap<_,_>>());
        crate::presentation::SemanticElement{id:format!("assembly-group:{assembly}/{}",group.id),role:"assembly-rigid-group".into(),label:group.name.clone(),value:Some(format!("{} active / {} captured components",active.len(),group.members.len())),state:if effective.suppressed{"suppressed"}else if active.len()<2{"inactive"}else if current.is_none(){"pending"}else{"evaluated"}.into(),commands:vec!["assembly.group-edit".into(),"assembly.group-suppress".into(),"assembly.group-delete".into()],relationships:group.members.keys().cloned().collect(),data:serde_json::json!({"contract":"tcad.assembly-groups","version":CONTRACT_VERSION,"document":doc.id,"assembly":assembly,"group":group.id,"configuration":config,"source":group,"effective_suppressed":effective.suppressed,"active_members":active,"solver_links":links,"component_motion":motion,"kernel_geometry":false,"editable":true})}
    }).collect::<Vec<_>>();
    if let Some(current) = current {
        for group in owner.rigid_groups.values() {
            for member in group.members.keys() {
                let name = owner
                    .instances
                    .get(member)
                    .map(|instance| instance.name.as_str())
                    .unwrap_or("Inactive component");
                if let Some(diagnostic) = current.diagnostics.get(&link_id(&group.id, member)) {
                    out.push(crate::presentation::SemanticElement{id:format!("assembly-group:{assembly}/{}/link/{member}",group.id),role:"assembly-group-solver-link".into(),label:format!("{} · {name}",group.name),value:Some(format!("Residual {:.3e} · rank {}/{} · {} redundant",diagnostic.maximum_residual,diagnostic.independent_equations,diagnostic.equation_count,diagnostic.redundant_equations)),state:"evaluated".into(),commands:vec![],relationships:vec![group.id.clone(),member.clone()],data:serde_json::json!({"contract":"tcad.assembly-groups","version":CONTRACT_VERSION,"document":doc.id,"assembly":assembly,"group":group.id,"member":member,"solver_mate":link_id(&group.id,member),"diagnostic":diagnostic,"configuration":config})});
                }
                if let Some(motion) = current.component_motion.get(member) {
                    out.push(crate::presentation::SemanticElement{id:format!("assembly-group:{assembly}/{}/motion/{member}",group.id),role:"assembly-group-component-motion".into(),label:format!("{name} · Component Freedom"),value:Some(if motion.grounded{"Grounded".into()}else{format!("{} degrees of freedom",motion.degrees_of_freedom)}),state:"evaluated".into(),commands:vec![],relationships:vec![group.id.clone(),member.clone()],data:serde_json::json!({"contract":"tcad.assembly-groups","version":CONTRACT_VERSION,"document":doc.id,"assembly":assembly,"group":group.id,"member":member,"motion":motion,"configuration":config})});
                }
            }
        }
    }
    out
}
