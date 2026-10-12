//! Assembly-owned deposited metal and configured occurrence contexts.
use crate::{CadError, Document, Id, NodeRef, Result, weld_beads::Definition};
use nalgebra::Matrix4;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Evaluated {
    pub version: u32,
    pub owner: NodeRef,
    pub placement: crate::PlacedPart,
    pub bead: crate::weld_beads::Evaluated,
    pub properties: crate::inspection::Metrics,
    pub material: Id,
}
/// Disposable context, prepared from source configurations and solved placements.
pub struct Occurrence {
    pub path: Vec<Id>,
    pub matrix: Matrix4<f64>,
    pub definition: Id,
    pub configuration: Option<Id>,
    pub context: Document,
}
fn invalid(message: &str) -> CadError {
    CadError::Invalid(format!("Weld Bead: {message}"))
}
fn assignment(doc: &Document, weld: &Definition) -> Result<()> {
    weld.validate()?;
    if weld
        .finish
        .as_ref()
        .is_some_and(|id| !doc.finishes.contains_key(id))
        || weld
            .material
            .as_ref()
            .is_some_and(|id| !doc.materials.contains_key(id))
    {
        return Err(invalid(
            "Assigned finish or engineering material is missing",
        ));
    }
    Ok(())
}
fn source(doc: &Document, assembly: &str, path: &[Id], node: &NodeRef) -> Result<()> {
    let mut owner = assembly;
    if path.is_empty() {
        return Err(invalid(
            "Assembly seams and supports need a complete component path",
        ));
    }
    for id in path {
        owner = &doc
            .assemblies
            .get(owner)
            .and_then(|a| a.instances.get(id))
            .ok_or_else(|| invalid("A seam or support component is missing"))?
            .definition;
    }
    if owner != node.definition
        || !doc
            .parts
            .get(owner)
            .is_some_and(|p| p.features.iter().any(|f| f.id == node.node))
    {
        return Err(invalid(
            "A seam or support must reference a feature on its selected component",
        ));
    }
    Ok(())
}
pub fn validate(doc: &Document) -> Result<()> {
    for part in doc.parts.values() {
        for feature in &part.features {
            if let crate::Operation::WeldBead { definition } = &feature.operation {
                assignment(doc, definition)?;
                if definition.id != feature.id
                    || !definition.seam.path().is_empty()
                    || definition.supports.iter().any(|s| !s.path.is_empty())
                {
                    return Err(invalid(
                        "Part weld identities must match their feature and use local references",
                    ));
                }
            }
        }
    }
    for assembly in doc.assemblies.values() {
        if assembly.weld_beads.len() > 4096 {
            return Err(invalid("At most 4096 beads per assembly"));
        }
        for (id, weld) in &assembly.weld_beads {
            assignment(doc, weld)?;
            if id != &weld.id {
                return Err(invalid("Saved weld identity mismatch"));
            }
            if weld.suppressed {
                continue;
            }
            source(doc, &assembly.id, weld.seam.path(), weld.seam.dependency())?;
            for face in &weld.supports {
                source(doc, &assembly.id, &face.path, &face.body)?;
            }
        }
    }
    Ok(())
}
pub fn put(doc: &mut Document, assembly: &str, weld: Definition) -> Result<()> {
    let mut candidate = doc.clone();
    candidate
        .assemblies
        .get_mut(assembly)
        .ok_or_else(|| invalid("Owning assembly is missing"))?
        .weld_beads
        .insert(weld.id.clone(), weld);
    candidate.validate()?;
    *doc = candidate;
    Ok(())
}
pub fn remove(doc: &mut Document, assembly: &str, id: &str) -> Result<()> {
    let owner = doc
        .assemblies
        .get_mut(assembly)
        .ok_or_else(|| invalid("Owning assembly is missing"))?;
    if owner.weld_beads.remove(id).is_none() {
        return Err(invalid("Saved weld is missing"));
    }
    for configuration in owner.configurations.values_mut() {
        configuration.overrides.assembly_welds.remove(id);
    }
    Ok(())
}

/// Make Unique remaps only sources whose complete component path enters that
/// occurrence. Welds on sibling instances keep the shared definition ancestry.
pub(crate) fn retarget_instance(
    doc: &mut Document,
    assembly: &str,
    instance: &str,
    old: &str,
    new: &str,
    features: &BTreeMap<Id, Id>,
) -> Result<()> {
    let reaches = |owner: &str, path: &[Id]| {
        let mut definition = owner.to_string();
        let mut selected = false;
        for key in path {
            let Some(parent) = doc.assemblies.get(&definition) else {
                return false;
            };
            let Some(child) = parent.instances.get(key) else {
                return false;
            };
            selected |= parent.id == assembly
                && (key == instance
                    || parent.component_patterns.values().any(|pattern| {
                        pattern
                            .occurrences
                            .iter()
                            .any(|copy| copy.instance == *key && copy.seed == instance)
                    }));
            definition = child.definition.clone();
        }
        selected && definition == old
    };
    let mut targets = vec![];
    for owner in doc.assemblies.values() {
        for bead in owner.weld_beads.values() {
            if reaches(&owner.id, bead.seam.path()) {
                targets.push((owner.id.clone(), bead.id.clone(), None));
            }
            for (index, face) in bead.supports.iter().enumerate() {
                if reaches(&owner.id, &face.path) {
                    targets.push((owner.id.clone(), bead.id.clone(), Some(index)));
                }
            }
        }
    }
    let mut mapping = features.clone();
    mapping.insert(old.into(), new.into());
    for (owner, id, face) in targets {
        let bead = doc
            .assemblies
            .get_mut(&owner)
            .and_then(|assembly| assembly.weld_beads.get_mut(&id))
            .ok_or_else(|| invalid("A weld disappeared while retargeting an occurrence"))?;
        if let Some(index) = face {
            let support = bead.supports.get_mut(index).ok_or_else(|| {
                invalid("A weld support disappeared while retargeting an occurrence")
            })?;
            let mut value = serde_json::to_value(&*support)?;
            crate::library::remap_owned_features(&mut value, &mapping, "")?;
            *support = serde_json::from_value(value)?;
        } else {
            let mut value = serde_json::to_value(&bead.seam)?;
            crate::library::remap_owned_features(&mut value, &mapping, "")?;
            bead.seam = serde_json::from_value(value)?;
        }
    }
    Ok(())
}
pub fn occurrences(
    doc: &Document,
    evaluated: &BTreeMap<Id, crate::assembly_geometry::EvaluatedAssembly>,
) -> Result<Vec<Occurrence>> {
    struct Traversal<'a> {
        source: &'a Document,
        evaluated: &'a BTreeMap<Id, crate::assembly_geometry::EvaluatedAssembly>,
        required: BTreeSet<Vec<Id>>,
        output: Vec<Occurrence>,
        visits: usize,
        ancestry: BTreeSet<Id>,
    }
    impl Traversal<'_> {
        fn descendant_welds(&self, owner: &str, stack: &mut BTreeSet<Id>) -> Result<bool> {
            if stack.len() > 64 || !stack.insert(owner.into()) {
                return Err(invalid(
                    "Assembly weld ownership contains a hierarchy cycle or excessive depth",
                ));
            }
            let result = if let Some(assembly) = self.source.assemblies.get(owner) {
                if !assembly.weld_beads.is_empty() {
                    true
                } else {
                    let mut found = false;
                    for instance in assembly.instances.values() {
                        if self.descendant_welds(&instance.definition, stack)? {
                            found = true;
                            break;
                        }
                    }
                    found
                }
            } else {
                false
            };
            stack.remove(owner);
            Ok(result)
        }
        fn walk(
            &mut self,
            context: &Document,
            owner: &str,
            config: Option<Id>,
            path: Vec<Id>,
            matrix: Matrix4<f64>,
            depth: usize,
        ) -> Result<()> {
            self.visits += 1;
            if depth > 64 || self.visits > 100_000 || !self.ancestry.insert(owner.into()) {
                return Err(invalid(
                    "Occurrence context budget or hierarchy depth exceeded",
                ));
            }
            let scoped =
                crate::configuration::scope(self.source, context, owner, config.as_deref())?;
            let mut owns_welds = false;
            if let Some(assembly) = scoped.assemblies.get(owner) {
                for bead in assembly.weld_beads.values().filter(|bead| !bead.suppressed) {
                    owns_welds = true;
                    for local in std::iter::once(bead.seam.path())
                        .chain(bead.supports.iter().map(|face| face.path.as_slice()))
                    {
                        self.required
                            .insert(path.iter().chain(local).cloned().collect());
                    }
                }
                let poses = self
                    .evaluated
                    .get(&crate::configuration::key(owner, config.as_deref()));
                for instance in assembly.instances.values() {
                    let mut child_path = path.clone();
                    child_path.push(instance.id.clone());
                    let wanted = self
                        .required
                        .iter()
                        .any(|required| required.starts_with(&child_path));
                    if !wanted
                        && !self.descendant_welds(&instance.definition, &mut BTreeSet::new())?
                    {
                        continue;
                    }
                    let placement = poses
                        .and_then(|e| e.placements.get(&instance.id))
                        .unwrap_or(&instance.placement);
                    let child_config = crate::configuration::selected(
                        &scoped,
                        &instance.definition,
                        instance.configuration.as_ref(),
                    )
                    .cloned();
                    self.walk(
                        &scoped,
                        &instance.definition,
                        child_config,
                        child_path,
                        matrix * placement.matrix(),
                        depth + 1,
                    )?;
                }
            }
            if owns_welds || self.required.contains(&path) {
                self.output.push(Occurrence {
                    path,
                    matrix,
                    definition: owner.into(),
                    configuration: config,
                    context: scoped,
                });
            }
            self.ancestry.remove(owner);
            Ok(())
        }
    }
    // Ordinary models incur no weld occurrence-context traversal or document copies.
    if !doc
        .assemblies
        .values()
        .any(|assembly| !assembly.weld_beads.is_empty())
    {
        return Ok(vec![]);
    }
    let mut traversal = Traversal {
        source: doc,
        evaluated,
        required: BTreeSet::new(),
        output: vec![],
        visits: 0,
        ancestry: BTreeSet::new(),
    };
    traversal.walk(
        &crate::configuration::effective(doc)?,
        &doc.root,
        doc.active_configurations.get(&doc.root).cloned(),
        vec![],
        Matrix4::identity(),
        0,
    )?;
    Ok(traversal.output)
}
