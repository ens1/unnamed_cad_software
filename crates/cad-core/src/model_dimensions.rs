//! Finished-part measurements and manufacturing requirements. Only interventions
//! are stored. Every consumer resolves the same characteristics on final geometry.
use crate::{
    CadError, Document, Id, Part, Result, TopologyRef, protocol::MeshData, settings::ModelSettings,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

mod datums;
pub mod geometry;
pub(crate) mod remap;
pub use datums::{Datum, DatumCarrier, datum_eligible};

pub const CONTRACT_VERSION: u32 = 1;
pub const DEFAULT_TOLERANCE_MM: f64 = 0.1;
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
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Characteristic {
    /// Part-local identity of the measured relationship, independent of its value.
    pub id: Id,
    pub label: String,
    pub kind: Kind,
    pub nominal_mm: f64,
    pub witnesses: [[f64; 3]; 2],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plane_normal: Option<[f64; 3]>,
    pub supports: Vec<TopologyRef>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
pub struct Allowance {
    pub minus_mm: f64,
    pub plus_mm: f64,
}
impl Allowance {
    pub fn symmetric(value: f64) -> Self {
        Self {
            minus_mm: value,
            plus_mm: value,
        }
    }
    pub fn validate(&self) -> Result<()> {
        if [self.minus_mm, self.plus_mm]
            .iter()
            .all(|v| (0.0..=MAX_ALLOWANCE_MM).contains(v))
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
    pub label: String,
    pub tolerance: Allowance,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Definition {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub overrides: BTreeMap<Id, DimensionOverride>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub datums: BTreeMap<String, TopologyRef>,
}

impl Definition {
    pub fn is_empty(&self) -> bool {
        self.overrides.is_empty() && self.datums.is_empty()
    }
    pub fn validate(&self, part: &str) -> Result<()> {
        for (id, tolerance) in &self.overrides {
            if id.trim().is_empty() || id.chars().any(char::is_control) {
                return Err(CadError::Invalid(
                    "Missing model dimension identity.".into(),
                ));
            }
            if tolerance.label.trim().is_empty() || tolerance.label.chars().any(char::is_control) {
                return Err(CadError::Invalid("Missing model dimension label.".into()));
            }
            tolerance.tolerance.validate()?;
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
    SetTolerance {
        id: Id,
        label: String,
        tolerance: Option<Allowance>,
    },
    SetDatum {
        label: String,
        face: Option<TopologyRef>,
    },
}

pub fn apply(doc: &mut Document, part_id: &str, edit: Edit) -> Result<()> {
    let part = doc
        .parts
        .get_mut(part_id)
        .ok_or_else(|| CadError::Invalid("The selected part is missing.".into()))?;
    let mut requirements = part.model_dimensions.clone();
    match edit {
        Edit::SetTolerance {
            id,
            label,
            tolerance,
        } => match tolerance {
            Some(value) => {
                requirements.overrides.insert(
                    id,
                    DimensionOverride {
                        label,
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

pub fn evaluate(part: &Part, mesh: &MeshData, settings: &ModelSettings) -> Report {
    let (dimensions, coverage) = geometry::infer(part, mesh, settings.modeling.linear_mm);
    let mut report = Report {
        dimensions,
        coverage,
        ..Default::default()
    };
    let mut seen = BTreeSet::new();
    let mut ambiguous = BTreeSet::new();
    for dimension in &report.dimensions {
        if !seen.insert(dimension.id.clone()) {
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
        if !seen.contains(id) || ambiguous.contains(id) {
            report.issues.push(format!("{} no longer identifies one finished size. Review its tolerance in Model Dimensions.", saved.label));
        }
    }
    datums::resolve(part, mesh, &mut report);
    report
}

pub fn effective_tolerance(part: &Part, settings: &ModelSettings, dimension_id: &str) -> Allowance {
    part.model_dimensions
        .overrides
        .get(dimension_id)
        .map(|entry| entry.tolerance)
        .unwrap_or_else(|| Allowance::symmetric(settings.manufacturing_tolerance_mm))
}

/// Preserve small deviations when the ordinary display precision would round them away.
/// In uncommon units a short extra precision is needed to express the manufacturing limits.
fn number(value: f64, places: usize) -> String {
    if value != 0.0 && value.abs() < 1e-8 {
        return format!("{value:e}");
    }
    format!("{value:.places$}")
}
pub fn dimension_text(part: &Part, settings: &ModelSettings, dim: &Characteristic) -> String {
    let units = settings.display_length_unit;
    let scale = units.millimeters_per_unit();
    let allowance = effective_tolerance(part, settings, &dim.id);
    let minus = allowance.minus_mm / scale;
    let plus = allowance.plus_mm / scale;
    let nominal = dim.nominal_mm / scale;
    let mut places = settings.length_decimal_places.min(8) as usize;
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
        Kind::Linear => "",
        Kind::Diameter => "Ø",
        Kind::Radius => "R",
    };
    let tolerance = if minus == plus {
        format!("±{}", number(plus, places))
    } else {
        format!("+{}/-{}", number(plus, places), number(minus, places))
    };
    format!(
        "{prefix}{} {tolerance} {}",
        number(nominal, places),
        units.symbol()
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
        if part.model_dimensions.is_empty() {
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
