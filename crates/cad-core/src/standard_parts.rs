//! Sourced dimensional hardware envelopes, built from ordinary native sketches.
use crate::{CadError, Document, Id, Part, Placement, Result, materials::SourceRef};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
mod generate;
pub const CONTRACT_VERSION: u32 = 1;
const EMBEDDED: &str = include_str!("../../../assets/standard-parts/metric-hardware.json");
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Family {
    FlatWasher,
    HexNut,
    SocketScrew,
    HexScrew,
}
impl Family {
    pub const ALL: [Self; 4] = [
        Self::FlatWasher,
        Self::HexNut,
        Self::SocketScrew,
        Self::HexScrew,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::FlatWasher => "Flat washer",
            Self::HexNut => "Hex nut",
            Self::SocketScrew => "Socket head screw",
            Self::HexScrew => "Hex head screw",
        }
    }
    pub fn has_length(self) -> bool {
        matches!(self, Self::SocketScrew | Self::HexScrew)
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Shape {
    FlatWasher {
        inner_diameter_mm: f64,
        outer_diameter_mm: f64,
        thickness_mm: f64,
    },
    HexNut {
        across_flats_mm: f64,
        height_mm: f64,
        pitch_mm: f64,
    },
    SocketScrew {
        head_diameter_mm: f64,
        height_mm: f64,
        socket_across_flats_mm: f64,
        socket_depth_mm: f64,
        pitch_mm: f64,
    },
    HexScrew {
        across_flats_mm: f64,
        height_mm: f64,
        pitch_mm: f64,
    },
}
impl Shape {
    pub fn family(&self) -> Family {
        match self {
            Self::FlatWasher { .. } => Family::FlatWasher,
            Self::HexNut { .. } => Family::HexNut,
            Self::SocketScrew { .. } => Family::SocketScrew,
            Self::HexScrew { .. } => Family::HexScrew,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub id: Id,
    pub size: String,
    pub nominal_diameter_mm: f64,
    pub standard_reference: String,
    pub shape: Shape,
    pub sources: Vec<Id>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Catalog {
    pub schema_version: u32,
    pub catalog_version: String,
    pub name: String,
    pub sources: Vec<SourceRef>,
    pub entries: Vec<Entry>,
}
/// Original dimension/source snapshot, not certification of a subsequently edited part.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Origin {
    pub version: u32,
    pub catalog_version: String,
    pub entry: Entry,
    pub sources: Vec<SourceRef>,
    pub length_mm: Option<f64>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub entry: Id,
    pub length_mm: Option<f64>,
    pub material: Id,
    pub finish: Id,
    pub reuse_unchanged: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Insertion {
    pub assembly: Id,
    pub definition: Id,
    pub instance: Id,
    pub request: Request,
    pub placement: Placement,
    pub fixed: bool,
}
fn invalid(message: impl ToString) -> CadError {
    CadError::Invalid(format!("Standard hardware: {}", message.to_string()))
}
fn text(value: &str, max: usize) -> bool {
    !value.trim().is_empty() && value.len() <= max && !value.chars().any(char::is_control)
}
fn identity(value: &str) -> bool {
    text(value, 256) && !matches!(value, "." | "..") && !value.contains(['/', '\\', ':'])
}
fn positive(value: f64) -> bool {
    value.is_finite() && (0.0..=100_000.0).contains(&value) && value > 0.0
}
fn validate_entry(entry: &Entry) -> Result<()> {
    if !identity(&entry.id)
        || !text(&entry.size, 64)
        || !text(&entry.standard_reference, 256)
        || !positive(entry.nominal_diameter_mm)
        || entry.sources.is_empty()
        || entry.sources.len() > 16
    {
        return Err(invalid(
            "A catalog entry needs bounded identities, positive dimensions and source references",
        ));
    }
    let d = entry.nominal_diameter_mm;
    let okay = match entry.shape {
        Shape::FlatWasher {
            inner_diameter_mm,
            outer_diameter_mm,
            thickness_mm,
        } => {
            [inner_diameter_mm, outer_diameter_mm, thickness_mm]
                .into_iter()
                .all(positive)
                && inner_diameter_mm > d
                && outer_diameter_mm > inner_diameter_mm
        }
        Shape::HexNut {
            across_flats_mm,
            height_mm,
            pitch_mm,
        }
        | Shape::HexScrew {
            across_flats_mm,
            height_mm,
            pitch_mm,
        } => {
            [across_flats_mm, height_mm, pitch_mm]
                .into_iter()
                .all(positive)
                && across_flats_mm > d
                && pitch_mm < d
        }
        Shape::SocketScrew {
            head_diameter_mm,
            height_mm,
            socket_across_flats_mm,
            socket_depth_mm,
            pitch_mm,
        } => {
            [
                head_diameter_mm,
                height_mm,
                socket_across_flats_mm,
                socket_depth_mm,
                pitch_mm,
            ]
            .into_iter()
            .all(positive)
                && head_diameter_mm > d
                && socket_depth_mm < height_mm
                && socket_across_flats_mm * 2.0 / 3f64.sqrt() < head_diameter_mm
                && pitch_mm < d
        }
    };
    if !okay {
        return Err(invalid(
            "The dimensional envelope is degenerate or inconsistent",
        ));
    }
    Ok(())
}
fn validate_sources(sources: &[SourceRef]) -> Result<BTreeSet<&str>> {
    if sources.len() > 64 {
        return Err(invalid("At most 64 supplier references are supported"));
    }
    let mut ids = BTreeSet::new();
    for source in sources {
        if !identity(&source.id)
            || !ids.insert(source.id.as_str())
            || !text(&source.title, 1024)
            || !text(&source.url, 4096)
            || !(source.url.starts_with("https://") || source.url.starts_with("http://"))
            || !text(&source.retrieved, 64)
            || source.notes.len() > 8192
            || source
                .notes
                .chars()
                .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
        {
            return Err(invalid(
                "A supplier reference is invalid, duplicated or oversized",
            ));
        }
    }
    Ok(ids)
}
impl Catalog {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != CONTRACT_VERSION
            || !text(&self.catalog_version, 128)
            || !text(&self.name, 1024)
            || self.entries.len() > 2000
        {
            return Err(invalid("Unsupported or oversized catalog"));
        }
        let sources = validate_sources(&self.sources)?;
        let mut ids = BTreeSet::new();
        for entry in &self.entries {
            validate_entry(entry)?;
            if !ids.insert(&entry.id)
                || entry
                    .sources
                    .iter()
                    .any(|id| !sources.contains(id.as_str()))
            {
                return Err(invalid(
                    "A catalog identity is repeated or a source reference is missing",
                ));
            }
        }
        Ok(())
    }
}
pub fn parse_catalog(json: &str) -> Result<Catalog> {
    if json.len() > 4 * 1024 * 1024 {
        return Err(invalid("Catalog JSON exceeds 4 MiB"));
    }
    let value: Catalog = serde_json::from_str(json)?;
    value.validate()?;
    Ok(value)
}
pub fn catalog() -> Result<Catalog> {
    static CATALOG: std::sync::OnceLock<std::result::Result<Catalog, String>> =
        std::sync::OnceLock::new();
    CATALOG
        .get_or_init(|| parse_catalog(EMBEDDED).map_err(|error| error.to_string()))
        .clone()
        .map_err(CadError::Invalid)
}
pub fn validate_origin(origin: &Origin) -> Result<()> {
    if origin.version != CONTRACT_VERSION || !text(&origin.catalog_version, 128) {
        return Err(invalid("Unsupported origin snapshot version"));
    }
    validate_entry(&origin.entry)?;
    let sources = validate_sources(&origin.sources)?;
    if origin
        .entry
        .sources
        .iter()
        .any(|id| !sources.contains(id.as_str()))
    {
        return Err(invalid(
            "An origin snapshot is missing its supplier citation",
        ));
    }
    validate_length(origin.entry.shape.family(), origin.length_mm)
}
fn validate_length(family: Family, length: Option<f64>) -> Result<()> {
    if family.has_length() {
        if length.is_none_or(|length| !length.is_finite() || !(0.1..=1000.0).contains(&length)) {
            return Err(invalid(
                "Under-head length must be finite and between 0.1 and 1000 mm",
            ));
        }
    } else if length.is_some() {
        return Err(invalid(
            "Washers and nuts use their catalog thickness; a screw length is not applicable",
        ));
    }
    Ok(())
}
pub fn origin(request: &Request) -> Result<Origin> {
    let catalog = catalog()?;
    let entry = catalog
        .entries
        .into_iter()
        .find(|entry| entry.id == request.entry)
        .ok_or_else(|| invalid("The selected hardware size does not exist"))?;
    let sources = catalog
        .sources
        .into_iter()
        .filter(|source| entry.sources.contains(&source.id))
        .collect();
    let origin = Origin {
        version: CONTRACT_VERSION,
        catalog_version: catalog.catalog_version,
        entry,
        sources,
        length_mm: request.length_mm,
    };
    validate_origin(&origin)?;
    Ok(origin)
}
pub fn build(origin: &Origin, definition: &str, material: &str, finish: &str) -> Result<Part> {
    validate_origin(origin)?;
    if !identity(definition) {
        return Err(invalid(
            "The generated part needs a portable stable identity",
        ));
    }
    generate::build(origin, definition, material, finish)
}
/// Reuse only an exact, unchanged source definition, never an edited look-alike.
pub fn insert(document: &mut Document, insertion: &Insertion) -> Result<Id> {
    if !identity(&insertion.instance)
        || !identity(&insertion.definition)
        || !document.assemblies.contains_key(&insertion.assembly)
    {
        return Err(invalid(
            "Choose an assembly and portable component identities",
        ));
    }
    if crate::library::is_definition_pinned(document, &insertion.assembly) {
        return Err(invalid(
            "Make the linked assembly unique before adding hardware",
        ));
    }
    if !document.materials.contains_key(&insertion.request.material)
        || !document.finishes.contains_key(&insertion.request.finish)
    {
        return Err(invalid(
            "Choose existing independent material and finish definitions",
        ));
    }
    if insertion
        .placement
        .translation
        .iter()
        .chain(&insertion.placement.rotation_deg)
        .any(|value| !value.is_finite() || value.abs() > 1e12)
    {
        return Err(invalid(
            "The component placement is not finite or exceeds supported bounds",
        ));
    }
    if document.assemblies[&insertion.assembly]
        .instances
        .contains_key(&insertion.instance)
    {
        return Err(invalid("That component identity already exists"));
    }
    let origin = origin(&insertion.request)?;
    let mut definition = None;
    if insertion.request.reuse_unchanged {
        for part in document.parts.values() {
            if part.standard_part.as_ref() == Some(&origin)
                && build(
                    &origin,
                    &part.id,
                    &insertion.request.material,
                    &insertion.request.finish,
                )? == *part
            {
                definition = Some(part.id.clone());
                break;
            }
        }
    }
    let mut candidate = document.clone();
    let definition = if let Some(definition) = definition {
        definition
    } else {
        if candidate.parts.contains_key(&insertion.definition)
            || candidate.assemblies.contains_key(&insertion.definition)
            || candidate.materials.contains_key(&insertion.definition)
            || candidate.finishes.contains_key(&insertion.definition)
            || candidate.resources.contains_key(&insertion.definition)
        {
            return Err(invalid(
                "The proposed source definition identity is already in use",
            ));
        }
        let part = build(
            &origin,
            &insertion.definition,
            &insertion.request.material,
            &insertion.request.finish,
        )?;
        let definition = part.id.clone();
        candidate.parts.insert(definition.clone(), part);
        definition
    };
    let name = candidate.parts[&definition].name.clone();
    candidate
        .assemblies
        .get_mut(&insertion.assembly)
        .ok_or_else(|| invalid("The owning assembly disappeared"))?
        .instances
        .insert(
            insertion.instance.clone(),
            crate::Instance {
                id: insertion.instance.clone(),
                name,
                definition: definition.clone(),
                placement: insertion.placement.clone(),
                finish: None,
                configuration: None,
                fixed: insertion.fixed,
                translation_expressions: None,
            },
        );
    candidate.validate()?;
    *document = candidate;
    Ok(definition)
}
pub fn source_dimensions(origin: &Origin) -> BTreeMap<String, f64> {
    let mut values = BTreeMap::from([(
        "nominal_diameter_mm".into(),
        origin.entry.nominal_diameter_mm,
    )]);
    if let Ok(serde_json::Value::Object(shape)) = serde_json::to_value(&origin.entry.shape) {
        for (key, value) in shape {
            if let Some(value) = value.as_f64() {
                values.insert(key, value);
            }
        }
    }
    if let Some(length) = origin.length_mm {
        values.insert("under_head_length_mm".into(), length);
    }
    values
}
