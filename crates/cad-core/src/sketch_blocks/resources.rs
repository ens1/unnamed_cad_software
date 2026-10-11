//! Portable exact sketch blocks with pinned, detached cross-document source.
use super::Definition;
use crate::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::Path;

pub const CONTRACT_VERSION: u32 = 1;
pub const MAX_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_POINTS: usize = 512;
pub const MAX_CONSTRAINTS: usize = 128;
/// Admission bounds for the current local source editor's dense solve path.
pub const MAX_SOLVE_WORK: usize = 16_384;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Source {
    pub document_sha256: String,
    pub definition: Id,
    pub sketch: Id,
    pub block: Id,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Link {
    pub resource: Id,
    pub sha256: String,
    pub source: Source,
    /// Portable source identities mapped to local source controls. Historical
    /// provenance stays unchanged when a part is copied or made unique.
    pub local_ids: BTreeMap<Id, Id>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Package {
    pub schema_version: u32,
    pub id: Id,
    /// Native coordinates and literal dimensional values have explicit units.
    pub length_unit: String,
    pub angle_unit: String,
    pub source: Source,
    pub definition: Definition,
}

fn invalid(message: &str) -> CadError {
    CadError::Invalid(format!("Sketch block resource: {message}"))
}
fn identity(value: &str) -> bool {
    !value.trim().is_empty() && value.len() <= 256 && !value.chars().any(char::is_control)
}
fn digest(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}
fn validate_source(source: &Source) -> Result<()> {
    if !digest(&source.document_sha256)
        || !identity(&source.definition)
        || !identity(&source.sketch)
        || !identity(&source.block)
    {
        return Err(invalid(
            "Source identities or source fingerprint are invalid",
        ));
    }
    Ok(())
}
pub fn validate_link(link: &Link) -> Result<()> {
    if !identity(&link.resource) || !digest(&link.sha256) {
        return Err(invalid(
            "Pinned resource identity or fingerprint is invalid",
        ));
    }
    if link.local_ids.len() > MAX_POINTS + super::MAX_MEMBERS + MAX_CONSTRAINTS
        || link
            .local_ids
            .iter()
            .any(|(source, local)| !identity(source) || !identity(local))
        || link.local_ids.values().collect::<BTreeSet<_>>().len() != link.local_ids.len()
    {
        return Err(invalid("Pinned source identity correspondence is invalid"));
    }
    validate_source(&link.source)
}

fn ids(definition: &Definition) -> Vec<Id> {
    definition
        .points
        .keys()
        .chain(definition.members.keys())
        .cloned()
        .chain(
            definition
                .constraints
                .iter()
                .map(|constraint| constraint.id.clone()),
        )
        .collect()
}
fn remap(definition: &mut Definition, mapping: &BTreeMap<Id, Id>) -> Result<()> {
    let key = |identity: &str| {
        mapping
            .get(identity)
            .cloned()
            .unwrap_or_else(|| identity.into())
    };
    let mapped_ids = ids(definition)
        .iter()
        .map(|source| key(source))
        .collect::<Vec<_>>();
    if mapped_ids.iter().collect::<BTreeSet<_>>().len() != mapped_ids.len() {
        return Err(invalid(
            "Portable-to-local source correspondence collides; repair source identities before exporting or updating",
        ));
    }
    definition.points = std::mem::take(&mut definition.points)
        .into_iter()
        .map(|(source, point)| (key(&source), point))
        .collect();
    definition.members = std::mem::take(&mut definition.members)
        .into_iter()
        .map(|(source, mut member)| {
            member.curve = crate::sketch_transform::remap(&member.curve, mapping)?;
            Ok((key(&source), member))
        })
        .collect::<Result<_>>()?;
    for constraint in &mut definition.constraints {
        constraint.id = key(&constraint.id);
        constraint.entities = constraint
            .entities
            .iter()
            .map(|entity| key(entity))
            .collect();
    }
    Ok(())
}
fn literal(doc: &Document, owner: &str, expr: &Expr, dimension: Dimension) -> Result<Expr> {
    let value = expression::value(doc, owner, expr, dimension)?;
    if !value.is_finite() {
        return Err(invalid("Expressions must evaluate to finite values"));
    }
    Ok(Expr(match dimension {
        Dimension::Length => format!("{value} mm"),
        Dimension::Angle => format!("{value} rad"),
        Dimension::Scalar => value.to_string(),
    }))
}
fn freeze(doc: &Document, owner: &str, definition: &mut Definition) -> Result<()> {
    for member in definition.members.values_mut() {
        match &mut member.curve {
            SketchCurve::Circle { radius, .. } => {
                *radius = literal(doc, owner, radius, Dimension::Length)?;
            }
            SketchCurve::Ellipse {
                major,
                minor,
                angle,
                ..
            } => {
                *major = literal(doc, owner, major, Dimension::Length)?;
                *minor = literal(doc, owner, minor, Dimension::Length)?;
                *angle = literal(doc, owner, angle, Dimension::Angle)?;
            }
            _ => {}
        }
    }
    for constraint in &mut definition.constraints {
        constraint.value = if sketch_actions::is_dimension(&constraint.kind) {
            literal(
                doc,
                owner,
                &constraint.value,
                sketch_actions::dimension_unit(&constraint.kind),
            )?
        } else if constraint.kind == "symmetric_member"
            || (matches!(constraint.kind.as_str(), "tangent" | "curvature")
                && constraint.contact.is_some())
        {
            literal(doc, owner, &constraint.value, Dimension::Scalar)?
        } else {
            // Other geometric relations have no value input. Do not carry an
            // unused expression that accidentally links the importing document.
            "0".into()
        };
    }
    Ok(())
}

/// Snapshot exact source intent, resolving external expressions explicitly.
/// Callers choose the effective configuration to export; no document is mutated.
pub fn snapshot(doc: &Document, owner: &str, sketch: &Sketch, key: &str) -> Result<Package> {
    let definition = sketch
        .blocks
        .get(key)
        .ok_or_else(|| invalid("Choose a local block definition"))?;
    super::validate_definition(key, definition)?;
    let mut definition = definition.clone();
    freeze(doc, owner, &mut definition)?;
    let resource = definition
        .resource
        .as_ref()
        .map(|link| link.resource.clone())
        .unwrap_or_else(|| format!("{owner}.{}.{}", sketch.id, key));
    if let Some(link) = definition.resource.take() {
        let mut original = ids(&definition)
            .into_iter()
            .map(|key| (key.clone(), key))
            .collect::<BTreeMap<_, _>>();
        original.extend(
            link.local_ids
                .into_iter()
                .map(|(source, local)| (local, source)),
        );
        remap(&mut definition, &original)?;
    }
    let mut source_document = doc.clone();
    source_document
        .parts
        .get_mut(owner)
        .ok_or_else(|| invalid("Source part is missing"))?
        .sketches
        .insert(sketch.id.clone(), sketch.clone());
    let package = Package {
        schema_version: CONTRACT_VERSION,
        id: resource,
        length_unit: "mm".into(),
        angle_unit: "rad".into(),
        source: Source {
            document_sha256: source_document.fingerprint(),
            definition: owner.into(),
            sketch: sketch.id.clone(),
            block: key.into(),
        },
        definition,
    };
    validate(&package)?;
    Ok(package)
}

pub fn validate(package: &Package) -> Result<()> {
    if package.definition.points.len() > MAX_POINTS
        || package.definition.constraints.len() > MAX_CONSTRAINTS
        || package
            .definition
            .points
            .len()
            .saturating_add(package.definition.members.len())
            .saturating_mul(package.definition.constraints.len().saturating_add(1))
            > MAX_SOLVE_WORK
    {
        return Err(invalid(
            "Block exceeds the current local solve budget (512 controls, 128 relations, or 16384 control-and-member/relation work units)",
        ));
    }
    if package.schema_version != CONTRACT_VERSION
        || !identity(&package.id)
        || package.length_unit != "mm"
        || package.angle_unit != "rad"
        || package.definition.resource.is_some()
        || package.definition.name.len() > 4096
        || package.definition.name.chars().any(char::is_control)
    {
        return Err(invalid(
            "Expected version 1, millimeters/radians and a named self-contained block",
        ));
    }
    validate_source(&package.source)?;
    super::validate_definition(&package.id, &package.definition)?;
    for expression in package.definition.expressions() {
        if !crate::expression::references(&expression.0)?.is_empty() {
            return Err(invalid(
                "Portable block expressions cannot refer to external parameters",
            ));
        }
    }
    let source_ids = ids(&package.definition);
    if source_ids.iter().collect::<BTreeSet<_>>().len() != source_ids.len() {
        return Err(invalid(
            "Local point, member and relation identities must be distinct",
        ));
    }
    // Every serialized expression must resolve without the exporting document.
    // Bounded input validation is admission, not an acceptance test.
    let document = Document::empty("Block resource");
    let mut independent = package.definition.clone();
    freeze(&document, &document.root, &mut independent)?;
    if package
        .definition
        .points
        .keys()
        .chain(package.definition.members.keys())
        .any(|key| !identity(key))
        || package
            .definition
            .constraints
            .iter()
            .any(|constraint| !identity(&constraint.id))
    {
        return Err(invalid(
            "Local source identities must be bounded and nonempty",
        ));
    }
    Ok(())
}
pub fn sha256(package: &Package) -> Result<String> {
    Ok(format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(package)?)
    ))
}
fn imported_definition(package: &Package) -> Result<Definition> {
    validate(package)?;
    let mut definition = package.definition.clone();
    definition.resource = Some(Link {
        resource: package.id.clone(),
        sha256: sha256(package)?,
        source: package.source.clone(),
        local_ids: ids(&package.definition)
            .into_iter()
            .map(|key| (key.clone(), key))
            .collect(),
    });
    Ok(definition)
}

/// Import a detached definition and its first associative local instance as one
/// source edit. New output identities cannot collide with an existing instance.
pub fn insert(
    doc: &Document,
    owner: &str,
    sketch: &mut Sketch,
    package: &Package,
    position: [f64; 2],
) -> Result<(Id, Id)> {
    let definition = imported_definition(package)?;
    let mut candidate = sketch.clone();
    let key = id();
    candidate.blocks.insert(key.clone(), definition);
    let instance = super::insert(doc, owner, &mut candidate, &key, position)?;
    *sketch = candidate;
    Ok((key, instance))
}

/// Explicit update of the same library identity. The existing source mappings
/// let set_definition preserve compatible output controls and reject retirement
/// of referenced geometry before the caller changes its live draft.
pub fn update(
    doc: &Document,
    owner: &str,
    sketch: &mut Sketch,
    key: &str,
    package: &Package,
) -> Result<()> {
    let old = sketch
        .blocks
        .get(key)
        .and_then(|definition| definition.resource.as_ref())
        .ok_or_else(|| invalid("This block has no imported resource to update"))?;
    if old.resource != package.id {
        return Err(invalid(
            "The file belongs to a different block; import it as a separate definition",
        ));
    }
    let mut definition = imported_definition(package)?;
    let correspondence = ids(&definition)
        .into_iter()
        .map(|source| {
            let local = old
                .local_ids
                .get(&source)
                .cloned()
                .unwrap_or_else(crate::id);
            (source, local)
        })
        .collect::<BTreeMap<_, _>>();
    remap(&mut definition, &correspondence)?;
    definition.resource.as_mut().unwrap().local_ids = correspondence;
    super::set_definition(doc, owner, sketch, key, definition)
}

pub fn read(path: &Path) -> Result<Package> {
    let mut bytes = Vec::new();
    File::open(path)?
        .take(MAX_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > MAX_BYTES {
        return Err(invalid("File exceeds 16 MiB"));
    }
    let package = serde_json::from_slice(&bytes)?;
    validate(&package)?;
    Ok(package)
}
pub fn write(package: &Package, path: &Path) -> Result<()> {
    validate(package)?;
    let bytes = serde_json::to_vec_pretty(package)?;
    if bytes.len() > MAX_BYTES {
        return Err(invalid("File exceeds 16 MiB"));
    }
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let name = path
        .file_name()
        .ok_or_else(|| invalid("Choose a file name"))?
        .to_string_lossy();
    let temporary = parent.join(format!(".{name}.{}.tmp", id()));
    let result = (|| -> Result<()> {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        std::fs::rename(&temporary, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}
