//! Human-readable occurrence labels over stable source identities. Labels never
//! rewrite saved names, references, or pinned component definitions.
use crate::{Document, Id, Instance};
use std::collections::BTreeMap;

/// Names assigned by the empty-document workflow are placeholders. An explicit
/// engineering name remains authoritative, even when another occurrence uses it.
pub fn is_placeholder(name: &str) -> bool {
    let name = name.trim();
    name.is_empty()
        || ["Part", "Assembly"].iter().any(|prefix| {
            name.strip_prefix(prefix).is_some_and(|suffix| {
                suffix.is_empty() || suffix.chars().all(|c| c.is_ascii_digit())
            })
        })
}

pub fn definition(doc: &Document, id: &str) -> String {
    fn resolve(doc: &Document, id: &str, depth: usize) -> String {
        let name = doc
            .parts
            .get(id)
            .map(|part| &part.name)
            .or_else(|| doc.assemblies.get(id).map(|assembly| &assembly.name));
        let Some(name) = name else {
            return "Missing component".into();
        };
        if !is_placeholder(name) || depth >= 64 {
            return name.clone();
        }
        if id == doc.root && !is_placeholder(&doc.name) {
            return doc.name.clone();
        }
        if let Some(link) = doc.libraries.values().find(|link| link.definition == id)
            && !is_placeholder(&link.name)
        {
            return link.name.clone();
        }
        // A part document has a wrapper assembly. Older saves may still call
        // that wrapper Part1 after the engineer renames the actual part.
        if let Some(assembly) = doc.assemblies.get(id)
            && assembly.instances.len() == 1
            && let Some(child) = assembly.instances.values().next()
        {
            let child_name = resolve(doc, &child.definition, depth + 1);
            if !is_placeholder(&child_name) {
                return child_name;
            }
        }
        name.clone()
    }
    resolve(doc, id, 0)
}

fn base_instance(doc: &Document, instance: &Instance) -> String {
    if is_placeholder(&instance.name) {
        definition(doc, &instance.definition)
    } else {
        instance.name.clone()
    }
}

/// Owner-local occurrence label. A sibling number distinguishes repeated names
/// without exposing IDs or modifying an engineer's saved occurrence name.
pub fn instance(doc: &Document, owner: &str, occurrence: &Instance) -> String {
    let name = base_instance(doc, occurrence);
    let Some(assembly) = doc.assemblies.get(owner) else {
        return name;
    };
    let siblings = assembly
        .instances
        .values()
        .filter(|sibling| base_instance(doc, sibling) == name)
        .collect::<Vec<_>>();
    if siblings.len() > 1
        && let Some(index) = siblings
            .iter()
            .position(|sibling| sibling.id == occurrence.id)
    {
        format!("{name} <{}>", index + 1)
    } else {
        name
    }
}

/// Compute all sibling labels once for a tree/traversal, rather than scanning
/// the same assembly for every row in a large model.
pub fn siblings(doc: &Document, owner: &str) -> BTreeMap<Id, String> {
    let Some(assembly) = doc.assemblies.get(owner) else {
        return BTreeMap::new();
    };
    let mut definitions = BTreeMap::new();
    let mut groups = BTreeMap::<String, Vec<&Instance>>::new();
    for instance in assembly.instances.values() {
        let name = if is_placeholder(&instance.name) {
            definitions
                .entry(&instance.definition)
                .or_insert_with(|| definition(doc, &instance.definition))
                .clone()
        } else {
            instance.name.clone()
        };
        groups.entry(name).or_default().push(instance);
    }
    let mut names = BTreeMap::new();
    for (name, instances) in groups {
        let multiple = instances.len() > 1;
        for (index, instance) in instances.into_iter().enumerate() {
            names.insert(
                instance.id.clone(),
                if multiple {
                    format!("{name} <{}>", index + 1)
                } else {
                    name.clone()
                },
            );
        }
    }
    names
}

pub fn path_from(doc: &Document, owner: &str, path: &[Id]) -> Option<String> {
    let mut owner = owner;
    let mut names = Vec::with_capacity(path.len());
    for id in path {
        let assembly = doc.assemblies.get(owner)?;
        let occurrence = assembly.instances.get(id)?;
        let name = instance(doc, owner, occurrence);
        // Do not repeat a transparent single-part wrapper in every label.
        if names.last() != Some(&name)
            || !is_placeholder(&assembly.name)
            || assembly.instances.len() != 1
        {
            names.push(name);
        }
        owner = &occurrence.definition;
    }
    (!names.is_empty()).then(|| names.join(" › "))
}

pub fn path(doc: &Document, path: &[Id]) -> Option<String> {
    path_from(doc, &doc.root, path)
}

/// Choose an initial occurrence name. Existing names and IDs are never changed.
pub fn insertion(doc: &Document, owner: &str, suggested: &str) -> String {
    let Some(assembly) = doc.assemblies.get(owner) else {
        return suggested.into();
    };
    if !assembly
        .instances
        .values()
        .any(|instance| instance.name == suggested)
    {
        return suggested.into();
    }
    (2..)
        .map(|number| format!("{suggested} <{number}>"))
        .find(|name| {
            !assembly
                .instances
                .values()
                .any(|instance| &instance.name == name)
        })
        .expect("finite assembly has an unused occurrence name")
}
