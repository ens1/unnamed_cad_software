//! Finished-part measurements and manufacturing requirements. Only interventions
//! are stored. Every consumer resolves the same characteristics on final geometry.
use crate::{
    CadError, Dimension, Document, Id, Part, Result, TopologyRef, protocol::MeshData,
    settings::ModelSettings,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

mod datums;
pub mod geometry;
pub(crate) mod remap;
pub use datums::{Datum, DatumCarrier, datum_eligible};

pub const CONTRACT_VERSION: u32 = 1;
fn contract_version() -> u32 {
    CONTRACT_VERSION
}
pub const DEFAULT_TOLERANCE_MM: f64 = 0.1;
pub const DEFAULT_ANGULAR_TOLERANCE_DEG: f64 = 0.5;
pub fn is_default_angular_tolerance(value: &f64) -> bool {
    *value == DEFAULT_ANGULAR_TOLERANCE_DEG
}
fn length_dimension() -> Dimension {
    Dimension::Length
}
pub const MAX_ALLOWANCE_MM: f64 = 1e9;

pub fn is_default_tolerance(value: &f64) -> bool {
    *value == DEFAULT_TOLERANCE_MM
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Linear,
    Diameter,
    Radius,
    Angle,
}
impl Kind {
    pub fn dimension(self) -> Dimension {
        if self == Self::Angle {
            Dimension::Angle
        } else {
            Dimension::Length
        }
    }
    pub fn display_scale(self, settings: &ModelSettings) -> f64 {
        if self == Self::Angle {
            std::f64::consts::PI / 180.
        } else {
            settings.display_length_unit.millimeters_per_unit()
        }
    }
    pub fn display_unit(self, settings: &ModelSettings) -> &'static str {
        if self == Self::Angle {
            "°"
        } else {
            settings.display_length_unit.symbol()
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Characteristic {
    /// Part-local identity of the measured relationship, independent of its value.
    pub id: Id,
    pub label: String,
    pub kind: Kind,
    /// Canonical millimeters for size and location, radians for included angles.
    #[serde(alias = "nominal_mm")]
    pub nominal: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub angle_vertex: Option<[f64; 3]>,
    pub witnesses: [[f64; 3]; 2],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plane_normal: Option<[f64; 3]>,
    pub supports: Vec<TopologyRef>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
pub struct Allowance {
    #[serde(alias = "minus_mm")]
    pub minus: f64,
    #[serde(alias = "plus_mm")]
    pub plus: f64,
}
impl Allowance {
    pub fn symmetric(value: f64) -> Self {
        Self {
            minus: value,
            plus: value,
        }
    }
    pub fn validate(&self, dimension: Dimension) -> Result<()> {
        let maximum = match dimension {
            Dimension::Length => MAX_ALLOWANCE_MM,
            Dimension::Angle => std::f64::consts::PI,
            Dimension::Scalar => {
                return Err(CadError::Invalid(
                    "A manufacturing tolerance needs length or angle units.".into(),
                ));
            }
        };
        if [self.minus, self.plus]
            .iter()
            .all(|v| (0.0..=maximum).contains(v))
        {
            Ok(())
        } else {
            Err(CadError::Invalid(
                "Enter a finite, nonnegative size tolerance on each side of nominal.".into(),
            ))
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DimensionOverride {
    #[serde(default = "length_dimension")]
    pub dimension: Dimension,
    pub label: String,
    pub tolerance: Allowance,
}

/// Manufacturing defaults owned by a component's source. Paper/viewport display
/// units still come from the viewing document.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct SourceDefaults {
    pub linear_mm: f64,
    pub angular_deg: f64,
}
impl SourceDefaults {
    pub fn from_settings(settings: &ModelSettings) -> Self {
        Self {
            linear_mm: settings.manufacturing_tolerance_mm,
            angular_deg: settings.manufacturing_tolerance_deg,
        }
    }
    pub fn validate(&self) -> Result<()> {
        Allowance::symmetric(self.linear_mm).validate(Dimension::Length)?;
        Allowance::symmetric(self.angular_deg.to_radians()).validate(Dimension::Angle)
    }
    fn allowance(self, kind: Kind) -> Allowance {
        Allowance::symmetric(if kind == Kind::Angle {
            self.angular_deg.to_radians()
        } else {
            self.linear_mm
        })
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Definition {
    #[serde(default = "contract_version")]
    pub version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_defaults: Option<SourceDefaults>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub overrides: BTreeMap<Id, DimensionOverride>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub datums: BTreeMap<String, TopologyRef>,
}

impl Default for Definition {
    fn default() -> Self {
        Self {
            version: CONTRACT_VERSION,
            source_defaults: None,
            overrides: BTreeMap::new(),
            datums: BTreeMap::new(),
        }
    }
}
impl Definition {
    pub fn is_empty(&self) -> bool {
        self.source_defaults.is_none() && !self.has_requirements()
    }
    /// Only authored attachments require geometry to be present for release.
    pub fn has_requirements(&self) -> bool {
        !self.overrides.is_empty() || !self.datums.is_empty()
    }
    pub fn validate(&self, part: &str) -> Result<()> {
        if self.version != CONTRACT_VERSION {
            return Err(CadError::Unsupported(format!(
                "Finished model dimension contract version {}",
                self.version
            )));
        }
        if let Some(defaults) = self.source_defaults {
            defaults.validate()?;
        }
        for (id, tolerance) in &self.overrides {
            if id.trim().is_empty() || id.chars().any(char::is_control) {
                return Err(CadError::Invalid(
                    "Missing model dimension identity.".into(),
                ));
            }
            if tolerance.label.trim().is_empty() || tolerance.label.chars().any(char::is_control) {
                return Err(CadError::Invalid("Missing model dimension label.".into()));
            }
            tolerance.tolerance.validate(tolerance.dimension)?;
        }
        let mut faces = BTreeSet::new();
        for (label, face) in &self.datums {
            if !(1..=3).contains(&label.len()) || !label.bytes().all(|c| c.is_ascii_uppercase()) {
                return Err(CadError::Invalid(
                    "Use one to three uppercase letters for a datum.".into(),
                ));
            }
            if face.producer.definition != part
                || face.topology != "face"
                || !datums::stable_face(face)
            {
                return Err(CadError::Invalid(format!(
                    "Datum {label} needs a part face with a stable source."
                )));
            }
            if !faces.insert(crate::provenance::canonical_key(face)?) {
                return Err(CadError::Invalid(
                    "This face already has a datum designation.".into(),
                ));
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Edit {
    UseDocumentDefaults,
    ReassignTolerance {
        source: Id,
        replacement: Id,
        label: String,
        dimension: Dimension,
    },
    SetTolerance {
        id: Id,
        label: String,
        #[serde(default = "length_dimension")]
        dimension: Dimension,
        tolerance: Option<Allowance>,
    },
    SetDatum {
        label: String,
        face: Option<TopologyRef>,
    },
    AcceptDatums {
        datums: BTreeMap<String, TopologyRef>,
    },
}

pub fn apply(doc: &mut Document, part_id: &str, edit: Edit) -> Result<()> {
    let part = doc
        .parts
        .get_mut(part_id)
        .ok_or_else(|| CadError::Invalid("The selected part is missing.".into()))?;
    let mut requirements = part.model_dimensions.clone();
    match edit {
        Edit::UseDocumentDefaults => requirements.source_defaults = None,
        Edit::ReassignTolerance {
            source,
            replacement,
            label,
            dimension,
        } => {
            if requirements.overrides.contains_key(&replacement) {
                return Err(CadError::Invalid(
                    "The replacement size already has a tolerance override.".into(),
                ));
            }
            let mut previous = requirements.overrides.remove(&source).ok_or_else(|| {
                CadError::Invalid("The original tolerance override no longer exists.".into())
            })?;
            if previous.dimension != dimension {
                return Err(CadError::Invalid(
                    "Choose a replacement with the same units.".into(),
                ));
            }
            previous.label = label;
            requirements.overrides.insert(replacement, previous);
        }
        Edit::SetTolerance {
            id,
            label,
            dimension,
            tolerance,
        } => match tolerance {
            Some(value) => {
                requirements.overrides.insert(
                    id,
                    DimensionOverride {
                        label,
                        dimension,
                        tolerance: value,
                    },
                );
            }
            None => {
                requirements.overrides.remove(&id);
            }
        },
        Edit::SetDatum { label, face } => match face {
            Some(mut reference) => {
                reference.signature = None;
                requirements.datums.insert(label, reference);
            }
            None => {
                requirements.datums.remove(&label);
            }
        },
        Edit::AcceptDatums { datums } => {
            for (label, mut face) in datums {
                if requirements.datums.contains_key(&label) {
                    return Err(CadError::Invalid(format!(
                        "Datum {label} is already assigned."
                    )));
                }
                face.signature = None;
                requirements.datums.insert(label, face);
            }
        }
    }
    requirements.validate(part_id)?;
    part.model_dimensions = requirements;
    Ok(())
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Report {
    pub dimensions: Vec<Characteristic>,
    pub datums: Vec<Datum>,
    /// Unresolved explicit requirements require review before a sheet can be issued.
    pub issues: Vec<String>,
    /// Geometry outside the supported recognition set, separate from lost references.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub coverage: Vec<String>,
}

pub fn evaluate(part: &Part, mesh: &MeshData, document: &Document) -> Report {
    let (dimensions, coverage) = geometry::infer(part, mesh, document);
    let mut report = Report {
        dimensions,
        coverage,
        ..Default::default()
    };
    let mut seen = BTreeMap::new();
    let mut ambiguous = BTreeSet::new();
    for dimension in &report.dimensions {
        if seen
            .insert(dimension.id.clone(), dimension.kind.dimension())
            .is_some()
        {
            ambiguous.insert(dimension.id.clone());
        }
    }
    if !ambiguous.is_empty() {
        report.dimensions.retain(|dim| !ambiguous.contains(&dim.id));
        report.coverage.push(
            "Some dimensions have more than one matching source. Their sizes were omitted.".into(),
        );
    }
    for (id, saved) in &part.model_dimensions.overrides {
        if seen.get(id) != Some(&saved.dimension) || ambiguous.contains(id) {
            report.issues.push(format!("{} no longer identifies one finished size. Review its tolerance in Model Dimensions.", saved.label));
        }
    }
    datums::resolve(part, mesh, &mut report);
    report
}

pub fn effective_tolerance(
    part: &Part,
    settings: &ModelSettings,
    dimension: &Characteristic,
) -> Allowance {
    part.model_dimensions
        .overrides
        .get(&dimension.id)
        .filter(|entry| entry.dimension == dimension.kind.dimension())
        .map(|entry| entry.tolerance)
        .unwrap_or_else(|| {
            part.model_dimensions
                .source_defaults
                .unwrap_or_else(|| SourceDefaults::from_settings(settings))
                .allowance(dimension.kind)
        })
}

/// Preserve small deviations when the ordinary display precision would round them away.
/// In uncommon units a short extra precision is needed to express the manufacturing limits.
fn number(value: f64, places: usize, minimum_places: usize) -> String {
    if value != 0.0 && value.abs() < 1e-8 {
        return format!("{value:e}");
    }
    let mut text = format!("{value:.places$}");
    for _ in minimum_places..places {
        if !text.ends_with('0') {
            break;
        }
        text.pop();
    }
    if text.ends_with('.') {
        text.pop();
    }
    text
}
pub fn dimension_text(part: &Part, settings: &ModelSettings, dim: &Characteristic) -> String {
    let scale = dim.kind.display_scale(settings);
    let allowance = effective_tolerance(part, settings, dim);
    let minus = allowance.minus / scale;
    let plus = allowance.plus / scale;
    let nominal = dim.nominal / scale;
    let minimum_places = if dim.kind == Kind::Angle {
        settings.angle_decimal_places
    } else {
        settings.length_decimal_places
    }
    .min(8) as usize;
    let mut places = minimum_places;
    let allowance_requires_precision = |decimals: usize, value: f64| {
        let scale = 10f64.powi(decimals as i32);
        value != 0. && ((value * scale).round() / scale - value).abs() > 1e-6 * value.abs()
    };
    while places < 8
        && (allowance_requires_precision(places, plus)
            || allowance_requires_precision(places, minus)
            || (nominal != 0. && nominal.abs() * 10f64.powi(places as i32) < 0.5))
    {
        places += 1;
    }
    let prefix = match dim.kind {
        Kind::Linear | Kind::Angle => "",
        Kind::Diameter => "Ø",
        Kind::Radius => "R",
    };
    let display = |value| number(value, places, minimum_places);
    let tolerance = if minus == plus {
        format!("±{}", display(plus))
    } else {
        format!("+{}/-{}", display(plus), display(minus))
    };
    format!(
        "{prefix}{} {tolerance} {}",
        display(nominal),
        dim.kind.display_unit(settings)
    )
}

/// Working models can keep broken source references for repair. A release must
/// resolve explicit requirements on every configured part it actually contains.
pub fn validate_release(doc: &Document, evaluation: &crate::protocol::Evaluation) -> Result<()> {
    let mut reviewed = BTreeSet::new();
    let mut problems = Vec::new();
    for instance in &evaluation.instances {
        let key = crate::history::mesh_key(instance);
        if !reviewed.insert(key) {
            continue;
        }
        let Some(part) = doc.parts.get(&instance.definition) else {
            continue;
        };
        if !part.model_dimensions.has_requirements() {
            continue;
        }
        if let Some(report) = evaluation.model_dimensions.get(key) {
            problems.extend(
                report
                    .issues
                    .iter()
                    .map(|issue| format!("{}: {issue}", instance.name)),
            );
        } else {
            problems.push(format!(
                "{}: rebuild the part to resolve its model dimensions.",
                instance.name
            ));
        }
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(CadError::Invalid(problems.join("\n")))
    }
}
