//! Versioned, bounded, portable engineering catalogs. Imports are explicit copies.
use crate::{
    CadError, Document, Id, Material, Result,
    materials::{self, Family},
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const CONTRACT_VERSION: u32 = 1;
pub const MAX_JSON_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_MATERIALS: usize = 2000;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Catalog {
    pub schema_version: u32,
    pub name: String,
    pub materials: Vec<Material>,
}
impl Catalog {
    pub fn new(name: impl Into<String>, materials: Vec<Material>) -> Self {
        Self {
            schema_version: CONTRACT_VERSION,
            name: name.into(),
            materials,
        }
    }
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != CONTRACT_VERSION {
            return Err(CadError::Invalid(format!(
                "Unsupported material catalog version {}; supported version is {}",
                self.schema_version, CONTRACT_VERSION
            )));
        }
        if self.name.trim().is_empty()
            || self.name.len() > materials::MAX_TEXT_BYTES
            || self.name.chars().any(char::is_control)
        {
            return Err(CadError::Invalid(
                "The material catalog needs a bounded, printable name".into(),
            ));
        }
        if self.materials.len() > MAX_MATERIALS {
            return Err(CadError::Invalid(format!(
                "Material catalogs support at most {MAX_MATERIALS} entries"
            )));
        }
        let mut ids = BTreeSet::new();
        for material in &self.materials {
            materials::validate(material)?;
            if !ids.insert(&material.id) {
                return Err(CadError::Invalid(format!(
                    "Duplicate material identity {}",
                    material.id
                )));
            }
        }
        Ok(())
    }
    pub fn search<'a>(&'a self, query: &Query) -> Vec<&'a Material> {
        let terms: Vec<_> = query
            .text
            .split_whitespace()
            .map(str::to_lowercase)
            .collect();
        self.materials
            .iter()
            .filter(|material| {
                let metadata = material.metadata.as_ref();
                if query
                    .family
                    .is_some_and(|family| materials::family(material) != family)
                {
                    return false;
                }
                if !query.tags.iter().all(|tag| {
                    metadata.is_some_and(|m| m.tags.iter().any(|t| t.eq_ignore_ascii_case(tag)))
                }) {
                    return false;
                }
                let mut searchable =
                    format!("{} {}", material.name, materials::family(material).label());
                if let Some(m) = metadata {
                    searchable.push_str(&format!(
                        " {} {} {} {}",
                        m.grade,
                        m.condition,
                        m.description,
                        m.tags.join(" ")
                    ));
                }
                let searchable = searchable.to_lowercase();
                terms.iter().all(|term| searchable.contains(term))
            })
            .collect()
    }
    pub fn instantiate(&self, catalog_id: &str) -> Result<Material> {
        let entry = if let Some(exact) = self.materials.iter().find(|m| m.id == catalog_id) {
            exact
        } else {
            let mut matching = self.materials.iter().filter(|m| {
                m.metadata
                    .as_ref()
                    .and_then(|metadata| metadata.catalog_id.as_deref())
                    == Some(catalog_id)
            });
            let entry = matching.next().ok_or_else(|| {
                CadError::Invalid("The requested catalog material does not exist".into())
            })?;
            if matching.next().is_some() {
                return Err(CadError::Invalid("Multiple local copies have this catalog origin; choose the exact local material identity".into()));
            }
            entry
        };
        materials::validate(entry)?;
        Ok(materials::duplicate(entry, &entry.name))
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Query {
    pub text: String,
    pub family: Option<Family>,
    pub tags: Vec<String>,
}
pub fn parse_json(json: &str) -> Result<Catalog> {
    if json.len() > MAX_JSON_BYTES {
        return Err(CadError::Invalid(format!(
            "Material catalog exceeds the {} MiB import limit",
            MAX_JSON_BYTES / 1024 / 1024
        )));
    }
    let catalog: Catalog = serde_json::from_str(json)?;
    catalog.validate()?;
    Ok(catalog)
}
pub fn export_json(catalog: &Catalog) -> Result<String> {
    catalog.validate()?;
    let json = serde_json::to_string_pretty(catalog)?;
    if json.len() > MAX_JSON_BYTES {
        return Err(CadError::Invalid(
            "The material catalog is too large to export in the supported interchange format"
                .into(),
        ));
    }
    Ok(json)
}
/// Installed repository definitions, cached until the repository registry is refreshed.
/// Copying a record into a project preserves its complete independent snapshot.
pub fn installed_catalog() -> Result<std::sync::Arc<Catalog>> {
    type Cached = (u64, std::result::Result<std::sync::Arc<Catalog>, String>);
    static CACHE: std::sync::OnceLock<std::sync::Mutex<Option<Cached>>> =
        std::sync::OnceLock::new();
    let generation = crate::content_repositories::catalog_generation();
    let mut cache = CACHE
        .get_or_init(Default::default)
        .lock()
        .map_err(|_| CadError::Invalid("The installed material cache is unavailable".into()))?;
    if cache
        .as_ref()
        .is_none_or(|(cached_generation, _)| *cached_generation != generation)
    {
        let snapshot = crate::content_repositories::catalog_snapshot();
        let loaded = (|| -> Result<std::sync::Arc<Catalog>> {
            let mut materials = Vec::new();
            let mut ids = BTreeSet::new();
            for (index, source) in snapshot.entries.iter().filter(|source| {
                source.entry.kind == crate::content_repositories::ContentKind::Materials
            }).enumerate() {
                if index >= 32 {
                    return Err(CadError::Invalid("At most 32 catalogs of this kind can be enabled at once. Disable unused catalogs in Libraries.".into()));
                }
                let bytes = crate::content_repositories::read_entry(source, MAX_JSON_BYTES)?;
                let json = std::str::from_utf8(&bytes).map_err(|_| CadError::Invalid("Material catalog JSON must be UTF-8".into()))?;
                let catalog = parse_json(json).map_err(|error| CadError::Invalid(format!("{} / {}: {error}", source.repository_name, source.entry.name)))?;
                for material in catalog.materials {
                    if !ids.insert(material.id.clone()) {
                        return Err(CadError::Invalid(format!("Multiple enabled libraries use material ID {}. Disable one copy or give your fork distinct material IDs.", material.id)));
                    }
                    materials.push(material);
                }
            }
            Ok(std::sync::Arc::new(Catalog::new("Installed material libraries", materials)))
        })()
        .map_err(|error| error.to_string());
        *cache = Some((snapshot.generation, loaded));
    }
    cache
        .as_ref()
        .expect("cache was populated")
        .1
        .clone()
        .map_err(CadError::Invalid)
}
pub fn installed_material(id: &str) -> Result<Material> {
    installed_catalog()?.materials.iter().find(|material| material.id == id).cloned()
        .ok_or_else(|| CadError::Invalid("The requested material is unavailable. Enable its content repository in Libraries.".into()))
}
pub fn instantiate(catalog_id: &str) -> Result<Material> {
    installed_catalog()?.instantiate(catalog_id)
}
pub fn delete_material(document: &mut Document, material: &str) -> Result<()> {
    materials::delete_material(document, material)
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CollisionPolicy {
    /// Every imported definition gets a fresh local ID. Never overwrites local edits.
    #[default]
    NewIdentity,
    KeepExisting,
    /// Explicitly replace colliding local definitions, subject to pinned-use guards.
    ReplaceExisting,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ImportPlan {
    pub materials: Vec<Material>,
    pub identity_map: BTreeMap<Id, Id>,
    pub skipped: Vec<Id>,
}
fn reserved_ids(document: &Document) -> BTreeSet<Id> {
    // Domain object IDs share a document identity namespace. Source citations and
    // catalog origin strings are metadata and are deliberately not remapped.
    let mut ids: BTreeSet<_> = document
        .materials
        .keys()
        .chain(document.parts.keys())
        .chain(document.assemblies.keys())
        .chain(document.finishes.keys())
        .chain(document.resources.keys())
        .cloned()
        .collect();
    ids.insert(document.id.clone());
    for part in document.parts.values() {
        ids.extend(part.features.iter().map(|feature| feature.id.clone()));
        ids.extend(part.sketches.keys().cloned());
        ids.extend(part.parameters.keys().cloned());
        ids.extend(part.configuration_details.keys().cloned());
        for sketch in part.sketches.values() {
            ids.extend(sketch.points.keys().cloned());
            ids.extend(sketch.members.keys().cloned());
            ids.extend(sketch.circles.keys().cloned());
            ids.extend(sketch.constraints.iter().map(|c| c.id.clone()));
        }
    }
    for assembly in document.assemblies.values() {
        ids.extend(assembly.instances.keys().cloned());
        ids.extend(assembly.mates.iter().map(|m| m.id.clone()));
        ids.extend(assembly.parameters.keys().cloned());
        ids.extend(assembly.configurations.keys().cloned());
    }
    ids.extend(document.drawings.iter().map(|drawing| drawing.id.clone()));
    ids
}
pub fn plan_import(
    document: &Document,
    catalog: &Catalog,
    collision: CollisionPolicy,
) -> Result<ImportPlan> {
    catalog.validate()?;
    let mut reserved = reserved_ids(document);
    let mut plan = ImportPlan {
        materials: vec![],
        identity_map: BTreeMap::new(),
        skipped: vec![],
    };
    for source in &catalog.materials {
        let mut material = source.clone();
        let material_collision = document.materials.contains_key(&source.id);
        match collision {
            CollisionPolicy::KeepExisting if material_collision => {
                plan.skipped.push(source.id.clone());
                plan.identity_map
                    .insert(source.id.clone(), source.id.clone());
                continue;
            }
            CollisionPolicy::ReplaceExisting if material_collision => {}
            CollisionPolicy::NewIdentity => {
                material.id = crate::id();
            }
            _ if reserved.contains(&source.id) => {
                return Err(CadError::Invalid(format!(
                    "Imported material identity {} collides with a different document object; choose New identity",
                    source.id
                )));
            }
            _ => {}
        }
        if material
            .metadata
            .as_ref()
            .and_then(|m| m.catalog_id.as_ref())
            .is_none()
        {
            material.metadata.get_or_insert_default().catalog_id = Some(source.id.clone());
        }
        if collision == CollisionPolicy::NewIdentity {
            while reserved.contains(&material.id) {
                material.id = crate::id();
            }
        }
        reserved.insert(material.id.clone());
        plan.identity_map
            .insert(source.id.clone(), material.id.clone());
        plan.materials.push(material);
    }
    // Guard the entire plan before returning anything that can be applied.
    let mut candidate = document.clone();
    for material in &plan.materials {
        materials::put(&mut candidate, material.clone())?;
    }
    Ok(plan)
}
pub fn apply_import(document: &mut Document, plan: ImportPlan) -> Result<()> {
    if plan.materials.len() > MAX_MATERIALS {
        return Err(CadError::Invalid(
            "The material import plan exceeds the supported entry count".into(),
        ));
    }
    let mut candidate = document.clone();
    let mut ids = BTreeSet::new();
    let reserved = reserved_ids(document);
    for material in plan.materials {
        if !ids.insert(material.id.clone()) {
            return Err(CadError::Invalid(
                "The material import plan contains duplicate identities".into(),
            ));
        }
        if reserved.contains(&material.id) && !document.materials.contains_key(&material.id) {
            return Err(CadError::Invalid(format!(
                "Imported material identity {} collides with another document object",
                material.id
            )));
        }
        materials::put(&mut candidate, material)?;
    }
    candidate.validate()?;
    *document = candidate;
    Ok(())
}
