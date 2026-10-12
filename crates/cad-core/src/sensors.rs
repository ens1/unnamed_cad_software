//! Saved design monitors over native parameters and current evaluated engineering properties.
//! Monitors report design limits; they never change geometry or authorize an invalid evaluation.
use crate::{CadError, Dimension, Document, Id, Result, inspection, protocol::Evaluation};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const CONTRACT_VERSION: u32 = 1;
pub const MAX_SENSORS: usize = 512;
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Metric {
    Mass,
    Volume,
    Area,
    SizeX,
    SizeY,
    SizeZ,
}
impl Metric {
    pub const ALL: [Self; 6] = [
        Self::Mass,
        Self::Volume,
        Self::Area,
        Self::SizeX,
        Self::SizeY,
        Self::SizeZ,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Mass => "Mass",
            Self::Volume => "Volume",
            Self::Area => "Surface area",
            Self::SizeX => "Bounding size X",
            Self::SizeY => "Bounding size Y",
            Self::SizeZ => "Bounding size Z",
        }
    }
    pub fn unit(self) -> &'static str {
        match self {
            Self::Mass => "kg",
            Self::Volume => "mm³",
            Self::Area => "mm²",
            _ => "mm",
        }
    }
    fn value(self, metrics: &inspection::Metrics) -> Result<f64> {
        let value = match self {
            Self::Mass => metrics.mass_kg.ok_or_else(|| {
                invalid("Mass requires current closed solids and assigned engineering densities")
            })?,
            Self::Volume => {
                if metrics.volume_center_mm.is_none() {
                    return Err(invalid(
                        "Enclosed volume requires current closed solid geometry",
                    ));
                }
                metrics.volume_mm3
            }
            Self::Area => metrics.area_mm2,
            Self::SizeX => metrics.bounds_mm[1][0] - metrics.bounds_mm[0][0],
            Self::SizeY => metrics.bounds_mm[1][1] - metrics.bounds_mm[0][1],
            Self::SizeZ => metrics.bounds_mm[1][2] - metrics.bounds_mm[0][2],
        };
        if value.is_finite() && value >= 0. {
            Ok(value)
        } else {
            Err(invalid("The evaluated quantity is not finite"))
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Target {
    Parameter {
        definition: Id,
        parameter: Id,
        #[serde(default)]
        configuration: Option<Id>,
    },
    Part {
        definition: Id,
        #[serde(default)]
        configuration: Option<Id>,
        metric: Metric,
    },
    /// A full path from this document's root; an empty path monitors the complete root assembly.
    Occurrence { path: Vec<Id>, metric: Metric },
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Limits {
    #[serde(default)]
    pub lower: Option<f64>,
    #[serde(default)]
    pub upper: Option<f64>,
}
impl Limits {
    pub fn validate(&self) -> Result<()> {
        if self.lower.is_none() && self.upper.is_none()
            || self
                .lower
                .into_iter()
                .chain(self.upper)
                .any(|value| !value.is_finite())
            || self
                .lower
                .zip(self.upper)
                .is_some_and(|(lower, upper)| lower > upper)
        {
            return Err(invalid(
                "A design monitor needs finite, ordered lower and/or upper limits",
            ));
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Definition {
    #[serde(default = "version")]
    pub version: u32,
    pub id: Id,
    pub name: String,
    pub target: Target,
    pub limits: Limits,
    #[serde(default = "enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub notes: String,
}
fn version() -> u32 {
    CONTRACT_VERSION
}
fn enabled() -> bool {
    true
}
fn invalid(message: &str) -> CadError {
    CadError::Invalid(format!("Design monitor: {message}"))
}
fn identity(value: &str) -> bool {
    !value.is_empty()
        && !matches!(value, "." | "..")
        && value.len() <= 256
        && !value
            .chars()
            .any(|c| c.is_control() || matches!(c, '/' | '\\' | ':'))
}
pub fn validate(document: &Document) -> Result<()> {
    if document.sensors.len() > MAX_SENSORS {
        return Err(invalid("A document is limited to 512 design monitors"));
    }
    let mut names = BTreeSet::new();
    for (id, sensor) in &document.sensors {
        if !identity(id)
            || id != &sensor.id
            || sensor.version != CONTRACT_VERSION
            || sensor.name.trim().is_empty()
            || sensor.name.len() > 256
            || sensor.notes.len() > 4096
            || !names.insert(sensor.name.trim().to_lowercase())
        {
            return Err(invalid(
                "Monitor identity, version, name or notes are invalid",
            ));
        }
        sensor.limits.validate()?;
        match &sensor.target {
            Target::Parameter {
                definition,
                parameter,
                configuration,
            } => {
                if !identity(definition)
                    || !identity(parameter)
                    || configuration.as_ref().is_some_and(|id| !identity(id))
                {
                    return Err(invalid("Invalid parameter binding identity"));
                }
            }
            Target::Part {
                definition,
                configuration,
                ..
            } => {
                if !identity(definition) || configuration.as_ref().is_some_and(|id| !identity(id)) {
                    return Err(invalid("Invalid part binding identity"));
                }
            }
            Target::Occurrence { path, .. } => {
                if path.len() > 64 || path.iter().any(|id| !identity(id)) {
                    return Err(invalid(
                        "Occurrence bindings require a bounded full source path",
                    ));
                }
            }
        }
    }
    Ok(())
}
pub fn put(document: &mut Document, sensor: Definition) -> Result<()> {
    let mut candidate = document.clone();
    candidate.sensors.insert(sensor.id.clone(), sensor);
    validate(&candidate)?;
    *document = candidate;
    Ok(())
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Disabled,
    WithinLimits,
    LimitExceeded,
    Unavailable,
}
impl Status {
    pub fn label(self) -> &'static str {
        match self {
            Self::Disabled => "Disabled",
            Self::WithinLimits => "Within limits",
            Self::LimitExceeded => "Limit exceeded",
            Self::Unavailable => "Unavailable",
        }
    }
}
/// Disposable property input excludes viewport meshes and retains the worker request identity.
#[derive(Clone)]
pub struct PropertySnapshot {
    pub identity: crate::protocol::RequestIdentity,
    pub valid: bool,
    pub properties: BTreeMap<String, inspection::Properties>,
    pub instances: Vec<crate::PlacedPart>,
    pub welds: Vec<(Vec<Id>, [[f64; 4]; 4], inspection::Metrics)>,
}
impl PropertySnapshot {
    pub fn capture(evaluation: &Evaluation) -> Self {
        Self {
            identity: evaluation.identity.clone(),
            valid: evaluation.valid,
            properties: evaluation.properties.clone(),
            instances: evaluation.instances.clone(),
            welds: evaluation
                .assembly_welds
                .values()
                .map(|weld| {
                    (
                        weld.placement.path.clone(),
                        weld.placement.matrix,
                        weld.properties.clone(),
                    )
                })
                .collect(),
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Reading {
    pub sensor: Id,
    pub status: Status,
    pub value: Option<f64>,
    pub unit: String,
    pub representation: String,
    pub diagnostic: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Report {
    pub version: u32,
    pub source: String,
    pub readings: BTreeMap<Id, Reading>,
}
fn configured(
    document: &Document,
    definition: &str,
    configuration: Option<&str>,
) -> Result<Document> {
    if !document.parts.contains_key(definition) && !document.assemblies.contains_key(definition) {
        return Err(invalid("The referenced definition is missing"));
    }
    let selected = configuration.or_else(|| {
        document
            .active_configurations
            .get(definition)
            .map(String::as_str)
    });
    if let Some(id) = selected
        && !crate::configuration::exists(document, definition, id)
    {
        return Err(invalid("The referenced configuration is missing"));
    }
    crate::configuration::effective(&crate::configuration::source_variant(
        document, definition, selected,
    ))
}
type Contexts = BTreeMap<(Id, Option<Id>), std::result::Result<Document, String>>;
fn context<'a>(
    document: &Document,
    contexts: &'a mut Contexts,
    definition: &str,
    configuration: Option<&str>,
) -> Result<&'a Document> {
    let key = (definition.into(), configuration.map(str::to_owned));
    if !contexts.contains_key(&key) && contexts.len() >= 8 {
        contexts.clear();
    }
    contexts
        .entry(key)
        .or_insert_with(|| {
            configured(document, definition, configuration).map_err(|error| error.to_string())
        })
        .as_ref()
        .map_err(|message| CadError::Invalid(message.clone()))
}
pub fn unit(document: &Document, target: &Target) -> Result<String> {
    unit_cached(document, &mut Contexts::new(), target)
}
fn unit_cached(document: &Document, contexts: &mut Contexts, target: &Target) -> Result<String> {
    match target {
        Target::Parameter {
            definition,
            parameter,
            configuration,
        } => {
            let context = context(document, contexts, definition, configuration.as_deref())?;
            let value = context
                .parts
                .get(definition)
                .map(|part| &part.parameters)
                .or_else(|| {
                    context
                        .assemblies
                        .get(definition)
                        .map(|assembly| &assembly.parameters)
                })
                .and_then(|parameters| parameters.get(parameter))
                .ok_or_else(|| invalid("The referenced parameter is missing"))?;
            Ok(match value.dimension {
                Dimension::Scalar => "",
                Dimension::Length => "mm",
                Dimension::Angle => "rad",
            }
            .into())
        }
        Target::Part { metric, .. } | Target::Occurrence { metric, .. } => Ok(metric.unit().into()),
    }
}
fn value(
    document: &Document,
    evaluation: Option<&PropertySnapshot>,
    source: &str,
    target: &Target,
    contexts: &mut Contexts,
) -> Result<(f64, String)> {
    match target {
        Target::Parameter {
            definition,
            parameter,
            configuration,
        } => {
            let context = context(document, contexts, definition, configuration.as_deref())?;
            let parameter = context
                .parts
                .get(definition)
                .map(|part| &part.parameters)
                .or_else(|| {
                    context
                        .assemblies
                        .get(definition)
                        .map(|assembly| &assembly.parameters)
                })
                .and_then(|parameters| parameters.get(parameter))
                .ok_or_else(|| invalid("The referenced parameter is missing"))?;
            Ok((
                crate::expression::value(
                    context,
                    definition,
                    &parameter.expression,
                    parameter.dimension,
                )?,
                "native parameter expression".into(),
            ))
        }
        _ => {
            let evaluation = evaluation
                .filter(|evaluation| evaluation.valid && evaluation.identity.source == source)
                .ok_or_else(|| invalid("Waiting for a valid evaluation of the current source"))?;
            let (metrics, metric) = match target {
                Target::Part {
                    definition,
                    configuration,
                    metric,
                } => {
                    if !document.parts.contains_key(definition) {
                        return Err(invalid("The referenced part is missing"));
                    }
                    let configuration = configuration.as_deref().or_else(|| {
                        document
                            .active_configurations
                            .get(definition)
                            .map(String::as_str)
                    });
                    let key = crate::configuration::key(definition, configuration);
                    let properties = evaluation.properties.get(&key).ok_or_else(|| {
                        invalid("This part variant has no current evaluated engineering properties")
                    })?;
                    (properties.total.clone(), *metric)
                }
                Target::Occurrence { path, metric } => {
                    let mut metrics = vec![];
                    let instances = evaluation
                        .instances
                        .iter()
                        .filter(|instance| instance.path.starts_with(path))
                        .collect::<Vec<_>>();
                    for instance in instances {
                        let properties=evaluation.properties.get(crate::history::mesh_key(instance)).ok_or_else(||invalid("A component has no current engineering properties; partial assembly totals are rejected"))?;
                        metrics.push(inspection::placed(&properties.total, instance.matrix)?);
                    }
                    for (weld_path, matrix, properties) in evaluation
                        .welds
                        .iter()
                        .filter(|(weld_path, _, _)| weld_path.starts_with(path))
                    {
                        let _ = weld_path;
                        metrics.push(inspection::placed(properties, *matrix)?);
                    }
                    if metrics.is_empty() {
                        return Err(invalid(
                            "This occurrence is missing, suppressed or has no evaluated geometry",
                        ));
                    }
                    (inspection::combine(&metrics)?, *metric)
                }
                Target::Parameter { .. } => unreachable!(),
            };
            Ok((
                metric.value(&metrics)?,
                "boundary tessellation at current model tolerance; mass uses assigned density"
                    .into(),
            ))
        }
    }
}
pub fn evaluate(document: &Document, evaluation: Option<&Evaluation>) -> Report {
    let snapshot = evaluation.map(PropertySnapshot::capture);
    evaluate_snapshot(document, snapshot.as_ref())
}
pub fn evaluate_snapshot(document: &Document, evaluation: Option<&PropertySnapshot>) -> Report {
    let source = document.fingerprint();
    let mut contexts = Contexts::new();
    let readings = document
        .sensors
        .values()
        .map(|sensor| {
            let mut reading = Reading {
                sensor: sensor.id.clone(),
                status: Status::Disabled,
                value: None,
                unit: unit_cached(document, &mut contexts, &sensor.target).unwrap_or_default(),
                representation: String::new(),
                diagnostic: None,
            };
            if sensor.enabled {
                match sensor.limits.validate().and_then(|()| {
                    value(document, evaluation, &source, &sensor.target, &mut contexts)
                }) {
                    Ok((value, representation)) => {
                        reading.value = Some(value);
                        reading.representation = representation;
                        let below = sensor.limits.lower.is_some_and(|lower| value < lower);
                        let above = sensor.limits.upper.is_some_and(|upper| value > upper);
                        reading.status = if below || above {
                            Status::LimitExceeded
                        } else {
                            Status::WithinLimits
                        };
                        if below || above {
                            reading.diagnostic = Some(
                                if below {
                                    "Value is below the saved lower limit"
                                } else {
                                    "Value is above the saved upper limit"
                                }
                                .into(),
                            );
                        }
                    }
                    Err(error) => {
                        reading.status = Status::Unavailable;
                        reading.diagnostic = Some(error.to_string());
                    }
                }
            }
            (sensor.id.clone(), reading)
        })
        .collect();
    Report {
        version: CONTRACT_VERSION,
        source,
        readings,
    }
}
pub fn semantic(document: &Document, report: &Report) -> Vec<crate::presentation::SemanticElement> {
    document.sensors.values().map(|sensor| {
        let reading=report.readings.get(&sensor.id);
        crate::presentation::SemanticElement {
            id:format!("sensor:{}",sensor.id),role:"design-monitor".into(),label:sensor.name.clone(),
            value:reading.and_then(|reading|reading.value).map(|value|format!("{value} {}",reading.map(|reading|reading.unit.as_str()).unwrap_or(""))),
            state:reading.map(|reading|match reading.status{Status::Disabled=>"disabled",Status::WithinLimits=>"within-limits",Status::LimitExceeded=>"limit-exceeded",Status::Unavailable=>"unavailable"}).unwrap_or("unavailable").into(),
            commands:vec!["inspection.sensors".into(),"sensor.edit".into()],relationships:vec![sensor.id.clone()],
            data:serde_json::json!({"contract":"tcad.design-monitor","version":CONTRACT_VERSION,"source":sensor,"reading":reading,"source_fingerprint":report.source,"sensor":sensor.id,"model_mutation":false}),
        }
    }).collect()
}
