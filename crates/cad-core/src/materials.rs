//! Engineering material definitions. Surface finishes remain in `appearance`.
//!
//! Property names carry canonical SI units. Values are a local source snapshot;
//! a catalog update never silently changes a part's density or other properties.
use crate::{CadError, Document, Id, Material, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const CONTRACT_VERSION: u32 = 1;
pub const MAX_TEXT_BYTES: usize = 8192;
pub const MAX_TAGS: usize = 64;
pub const MAX_SOURCES: usize = 64;

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum Family {
    Aluminum,
    CarbonSteel,
    AlloySteel,
    StainlessSteel,
    Titanium,
    Copper,
    Brass,
    Bronze,
    Nickel,
    Magnesium,
    Zinc,
    Polymer,
    Elastomer,
    Ceramic,
    Composite,
    #[default]
    Other,
}
impl Family {
    pub const ALL: [Self; 16] = [
        Self::Aluminum,
        Self::CarbonSteel,
        Self::AlloySteel,
        Self::StainlessSteel,
        Self::Titanium,
        Self::Copper,
        Self::Brass,
        Self::Bronze,
        Self::Nickel,
        Self::Magnesium,
        Self::Zinc,
        Self::Polymer,
        Self::Elastomer,
        Self::Ceramic,
        Self::Composite,
        Self::Other,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Aluminum => "Aluminum",
            Self::CarbonSteel => "Carbon steel",
            Self::AlloySteel => "Alloy steel",
            Self::StainlessSteel => "Stainless steel",
            Self::Titanium => "Titanium",
            Self::Copper => "Copper",
            Self::Brass => "Brass",
            Self::Bronze => "Bronze",
            Self::Nickel => "Nickel alloy",
            Self::Magnesium => "Magnesium",
            Self::Zinc => "Zinc",
            Self::Polymer => "Polymer",
            Self::Elastomer => "Elastomer",
            Self::Ceramic => "Ceramic",
            Self::Composite => "Composite",
            Self::Other => "Other",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SourceRef {
    pub id: String,
    pub title: String,
    /// A citation only; catalog parsing does not fetch or execute this URL.
    pub url: String,
    pub retrieved: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub notes: String,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PropertyBasis {
    Typical,
    Minimum,
    Maximum,
    #[default]
    UserDefined,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PropertyProvenance {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub source: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub reference: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub conditions: String,
    #[serde(default)]
    pub basis: PropertyBasis,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature_k: Option<f64>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct MaterialMetadata {
    #[serde(default)]
    pub family: Family,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub grade: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub condition: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub catalog_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub density_provenance: Option<PropertyProvenance>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sources: Vec<SourceRef>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum PropertyKey {
    YoungsModulusPa,
    PoissonRatio,
    YieldStrengthPa,
    TensileStrengthPa,
    ShearModulusPa,
    ThermalConductivityWPerMK,
    SpecificHeatJPerKgK,
    ThermalExpansionPerK,
    ElectricalResistivityOhmM,
    ElongationRatio,
    MeltingTemperatureK,
}
impl PropertyKey {
    pub const ALL: [Self; 11] = [
        Self::YoungsModulusPa,
        Self::PoissonRatio,
        Self::YieldStrengthPa,
        Self::TensileStrengthPa,
        Self::ShearModulusPa,
        Self::ThermalConductivityWPerMK,
        Self::SpecificHeatJPerKgK,
        Self::ThermalExpansionPerK,
        Self::ElectricalResistivityOhmM,
        Self::ElongationRatio,
        Self::MeltingTemperatureK,
    ];
    pub fn all() -> &'static [Self] {
        &Self::ALL
    }
    pub fn key(self) -> &'static str {
        match self {
            Self::YoungsModulusPa => "youngs_modulus_pa",
            Self::PoissonRatio => "poisson_ratio",
            Self::YieldStrengthPa => "yield_strength_pa",
            Self::TensileStrengthPa => "tensile_strength_pa",
            Self::ShearModulusPa => "shear_modulus_pa",
            Self::ThermalConductivityWPerMK => "thermal_conductivity_w_per_m_k",
            Self::SpecificHeatJPerKgK => "specific_heat_j_per_kg_k",
            Self::ThermalExpansionPerK => "thermal_expansion_per_k",
            Self::ElectricalResistivityOhmM => "electrical_resistivity_ohm_m",
            Self::ElongationRatio => "elongation_ratio",
            Self::MeltingTemperatureK => "melting_temperature_k",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::YoungsModulusPa => "Young's modulus",
            Self::PoissonRatio => "Poisson's ratio",
            Self::YieldStrengthPa => "Yield strength",
            Self::TensileStrengthPa => "Tensile strength",
            Self::ShearModulusPa => "Shear modulus",
            Self::ThermalConductivityWPerMK => "Thermal conductivity",
            Self::SpecificHeatJPerKgK => "Specific heat",
            Self::ThermalExpansionPerK => "Thermal expansion",
            Self::ElectricalResistivityOhmM => "Electrical resistivity",
            Self::ElongationRatio => "Elongation at break",
            Self::MeltingTemperatureK => "Melting temperature",
        }
    }
    pub fn unit(self) -> &'static str {
        match self {
            Self::YoungsModulusPa
            | Self::YieldStrengthPa
            | Self::TensileStrengthPa
            | Self::ShearModulusPa => "Pa",
            Self::PoissonRatio | Self::ElongationRatio => "1",
            Self::ThermalConductivityWPerMK => "W/(m·K)",
            Self::SpecificHeatJPerKgK => "J/(kg·K)",
            Self::ThermalExpansionPerK => "1/K",
            Self::ElectricalResistivityOhmM => "Ω·m",
            Self::MeltingTemperatureK => "K",
        }
    }
    pub fn valid_value(self, value: f64) -> bool {
        value.is_finite()
            && match self {
                Self::PoissonRatio => (-1.0..0.5).contains(&value),
                Self::ThermalExpansionPerK => value.abs() < 1.0,
                Self::ElongationRatio => value >= 0.0,
                _ => value > 0.0,
            }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PropertyValue {
    pub value: f64,
    #[serde(default)]
    pub provenance: PropertyProvenance,
}
impl PropertyValue {
    pub fn user_defined(value: f64) -> Self {
        Self {
            value,
            provenance: PropertyProvenance::default(),
        }
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(transparent)]
pub struct Properties(pub BTreeMap<PropertyKey, PropertyValue>);
impl Properties {
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    pub fn get(&self, key: PropertyKey) -> Option<&PropertyValue> {
        self.0.get(&key)
    }
    pub fn set(&mut self, key: PropertyKey, value: Option<PropertyValue>) {
        if let Some(value) = value {
            self.0.insert(key, value);
        } else {
            self.0.remove(&key);
        }
    }
    pub fn iter(&self) -> impl Iterator<Item = (&PropertyKey, &PropertyValue)> {
        self.0.iter()
    }
}

pub fn custom(name: impl Into<String>, density_kg_m3: f64) -> Material {
    Material {
        id: crate::id(),
        name: name.into(),
        density_kg_m3,
        color: [0.5; 3],
        metallic: 0.0,
        roughness: 0.4,
        metadata: Some(MaterialMetadata::default()),
        properties: Properties::default(),
    }
}
pub fn duplicate(material: &Material, name: impl Into<String>) -> Material {
    let mut copy = material.clone();
    copy.id = crate::id();
    copy.name = name.into();
    copy
}
pub fn family(material: &Material) -> Family {
    material
        .metadata
        .as_ref()
        .map_or(Family::Other, |m| m.family)
}
fn text(value: &str) -> Result<()> {
    if value.len() > MAX_TEXT_BYTES
        || value
            .chars()
            .any(|c| c == '\0' || (c.is_control() && !matches!(c, '\n' | '\r' | '\t')))
    {
        return Err(CadError::Invalid(
            "Material text is too long or contains control characters".into(),
        ));
    }
    Ok(())
}
fn identifier(value: &str) -> bool {
    !value.trim().is_empty()
        && value.len() <= 256
        && !matches!(value, "." | "..")
        && !value
            .chars()
            .any(|c| c.is_control() || matches!(c, '/' | '\\' | ':'))
}
pub fn validate_source(source: &SourceRef) -> Result<()> {
    if !identifier(&source.id)
        || source.title.trim().is_empty()
        || source.retrieved.trim().is_empty()
    {
        return Err(CadError::Invalid(
            "A material citation needs an identity, title and retrieval date".into(),
        ));
    }
    for value in [&source.title, &source.url, &source.retrieved, &source.notes] {
        text(value)?;
    }
    if !source.url.is_empty()
        && !source.url.starts_with("https://")
        && !source.url.starts_with("http://")
    {
        return Err(CadError::Invalid(
            "Material citations support HTTP(S) URLs only".into(),
        ));
    }
    Ok(())
}
fn provenance(value: &PropertyProvenance, sources: &BTreeSet<&str>) -> Result<()> {
    for v in [&value.source, &value.reference, &value.conditions] {
        text(v)?;
    }
    if !value.source.is_empty() && !sources.contains(value.source.as_str()) {
        return Err(CadError::Invalid(format!(
            "Missing material property citation {}",
            value.source
        )));
    }
    if value.basis != PropertyBasis::UserDefined && value.source.is_empty() {
        return Err(CadError::Invalid("Published material properties need a source citation; use user_defined for custom data".into()));
    }
    if value
        .temperature_k
        .is_some_and(|v| !v.is_finite() || v <= 0.0)
    {
        return Err(CadError::Invalid(
            "Material reference temperature must be finite and above absolute zero".into(),
        ));
    }
    Ok(())
}
pub fn validate(material: &Material) -> Result<()> {
    if !identifier(&material.id) || material.name.trim().is_empty() {
        return Err(CadError::Invalid(
            "Material identity and name must not be empty".into(),
        ));
    }
    text(&material.name)?;
    if !material.density_kg_m3.is_finite()
        || material.density_kg_m3 <= 0.0
        || !material
            .color
            .iter()
            .all(|v| v.is_finite() && (0.0..=1.0).contains(v))
        || !material.metallic.is_finite()
        || !(0.0..=1.0).contains(&material.metallic)
        || !material.roughness.is_finite()
        || !(0.0..=1.0).contains(&material.roughness)
    {
        return Err(CadError::Invalid(
            "Invalid material density or legacy appearance values".into(),
        ));
    }
    let mut sources = BTreeSet::new();
    if let Some(m) = &material.metadata {
        for value in [&m.grade, &m.condition, &m.description] {
            text(value)?;
        }
        if m.tags.len() > MAX_TAGS
            || m.sources.len() > MAX_SOURCES
            || m.catalog_id.as_ref().is_some_and(|id| !identifier(id))
        {
            return Err(CadError::Invalid(
                "Material metadata exceeds its supported bounds".into(),
            ));
        }
        let mut tags = BTreeSet::new();
        for tag in &m.tags {
            text(tag)?;
            if tag.trim().is_empty() || !tags.insert(tag.to_lowercase()) {
                return Err(CadError::Invalid(
                    "Material tags must be nonempty and unique".into(),
                ));
            }
        }
        for source in &m.sources {
            validate_source(source)?;
            if !sources.insert(source.id.as_str()) {
                return Err(CadError::Invalid(
                    "Duplicate material citation identity".into(),
                ));
            }
        }
        if let Some(value) = &m.density_provenance {
            provenance(value, &sources)?;
        }
    }
    for (key, value) in material.properties.iter() {
        if !key.valid_value(value.value) {
            return Err(CadError::Invalid(format!(
                "Invalid {} value in {}",
                key.label(),
                key.unit()
            )));
        }
        provenance(&value.provenance, &sources)?;
    }
    Ok(())
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Usage {
    pub definition: Id,
    pub name: String,
    pub configuration: Option<Id>,
}
pub fn usages(document: &Document, material: &str) -> Vec<Usage> {
    let mut out = Vec::new();
    for part in document.parts.values() {
        if part.material == material {
            out.push(Usage {
                definition: part.id.clone(),
                name: part.name.clone(),
                configuration: None,
            });
        }
        for (id, configuration) in &part.configuration_details {
            if configuration.overrides.material.as_deref() == Some(material) {
                out.push(Usage {
                    definition: part.id.clone(),
                    name: part.name.clone(),
                    configuration: Some(id.clone()),
                });
            }
        }
    }
    out
}
pub fn delete_material(document: &mut Document, material: &str) -> Result<()> {
    if !document.materials.contains_key(material) {
        return Err(CadError::Invalid(
            "The material to delete is missing".into(),
        ));
    }
    if material == document.settings.default_new_part_material {
        return Err(CadError::Invalid("This material is the default for new parts. Choose another default in model settings before deleting it".into()));
    }
    if crate::library::material_is_pinned(document, material) {
        return Err(CadError::Invalid("This engineering material belongs to an immutable component snapshot; duplicate it rather than deleting the pinned source".into()));
    }
    let uses = usages(document, material);
    if !uses.is_empty() {
        return Err(CadError::Invalid(format!(
            "Material is assigned to {} part/configuration definitions; reassign them before deleting it",
            uses.len()
        )));
    }
    document.materials.remove(material);
    Ok(())
}
pub fn put(document: &mut Document, material: Material) -> Result<()> {
    validate(&material)?;
    if document.materials.get(&material.id) != Some(&material) {
        let pinned = crate::library::material_is_pinned(document, &material.id);
        if pinned {
            return Err(CadError::Invalid("This material belongs to a pinned component snapshot. Duplicate the material and make the linked component unique before changing it".into()));
        }
    }
    document.materials.insert(material.id.clone(), material);
    Ok(())
}
/// Numeric local edits cannot keep an unchanged manufacturer attribution.
/// Explicit catalog replacement uses `put` and supplies its own source snapshot.
pub fn put_edited(document: &mut Document, mut material: Material) -> Result<()> {
    if let Some(previous) = document.materials.get(&material.id) {
        if previous.density_kg_m3 != material.density_kg_m3 {
            let previous_provenance = previous
                .metadata
                .as_ref()
                .and_then(|m| m.density_provenance.as_ref());
            if let Some(metadata) = &mut material.metadata
                && metadata.density_provenance.as_ref() == previous_provenance
            {
                metadata.density_provenance = Some(PropertyProvenance::default());
            }
        }
        for (key, next) in &mut material.properties.0 {
            if let Some(previous) = previous.properties.get(*key)
                && previous.value != next.value
                && previous.provenance == next.provenance
            {
                next.provenance = PropertyProvenance::default();
            }
        }
    }
    put(document, material)
}
pub fn assign(document: &mut Document, definition: &str, material: &str) -> Result<()> {
    if !document.materials.contains_key(material) {
        return Err(CadError::Invalid(
            "The selected material does not exist".into(),
        ));
    }
    if document
        .libraries
        .values()
        .any(|link| link.pinned_definitions.contains_key(definition))
    {
        return Err(CadError::Invalid(
            "Make the linked component unique before changing its engineering material".into(),
        ));
    }
    document
        .parts
        .get_mut(definition)
        .ok_or_else(|| CadError::Invalid("The material assignment needs a part definition".into()))?
        .material = material.into();
    Ok(())
}
