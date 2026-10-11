use cad_core::{CadError, Result};
use std::collections::{BTreeMap, BTreeSet};
use step_p21::ast::{EntityInstance, Exchange, Name, Parameter, Record};
/// Read units from STEP representation contexts. Refuse mixed length contexts
/// rather than silently interpreting imported coordinates in the wrong units.
pub fn length_scale(source: &str) -> Result<f64> {
    let ast: Exchange = source
        .parse()
        .map_err(|e: step_p21::error::Error| CadError::Invalid(e.to_string()))?;
    let records: BTreeMap<u64, Vec<Record>> = ast
        .data
        .into_iter()
        .flat_map(|d| d.entities)
        .map(|e| match e {
            EntityInstance::Simple { id, record } => (id, vec![record]),
            EntityInstance::Complex { id, subsuper } => (id, subsuper.0),
        })
        .collect();
    fn list(p: &Parameter) -> &[Parameter] {
        if let Parameter::List(items) = p {
            items
        } else {
            &[]
        }
    }
    fn numeric(p: &Parameter) -> Option<f64> {
        match p {
            Parameter::Real(v) => Some(*v),
            Parameter::Integer(v) => Some(*v as f64),
            Parameter::Typed { parameter, .. } => numeric(parameter),
            Parameter::List(items) if items.len() == 1 => numeric(&items[0]),
            _ => None,
        }
    }
    fn reference(p: &Parameter) -> Option<u64> {
        if let Parameter::Ref(Name::Entity(id)) = p {
            Some(*id)
        } else {
            None
        }
    }
    fn scale(
        id: u64,
        records: &BTreeMap<u64, Vec<Record>>,
        stack: &mut BTreeSet<u64>,
    ) -> Result<f64> {
        if !stack.insert(id) {
            return Err(CadError::Invalid("STEP unit cycle".into()));
        }
        let unit = records
            .get(&id)
            .ok_or_else(|| CadError::Invalid("Missing STEP length unit".into()))?;
        let result = if let Some(si) = unit.iter().find(|r| r.name == "SI_UNIT") {
            let p = list(&si.parameter);
            if p.len() != 2 || !matches!(&p[1],Parameter::Enumeration(name)if name=="METRE") {
                return Err(CadError::Unsupported(
                    "Unknown STEP length base unit".into(),
                ));
            }
            let prefix = match &p[0] {
                Parameter::NotProvided => 1.,
                Parameter::Enumeration(name) => match name.as_str() {
                    "MILLI" => 0.001,
                    "CENTI" => 0.01,
                    "DECI" => 0.1,
                    "MICRO" => 1e-6,
                    "KILO" => 1000.,
                    _ => return Err(CadError::Unsupported(format!("STEP SI prefix {name}"))),
                },
                _ => return Err(CadError::Invalid("Invalid STEP SI prefix".into())),
            };
            1000. * prefix
        } else if let Some(conversion) = unit.iter().find(|r| r.name == "CONVERSION_BASED_UNIT") {
            let p = list(&conversion.parameter);
            let measure = p
                .get(1)
                .and_then(reference)
                .and_then(|id| records.get(&id))
                .and_then(|r| {
                    r.iter().find(|r| {
                        r.name == "LENGTH_MEASURE_WITH_UNIT" || r.name == "MEASURE_WITH_UNIT"
                    })
                })
                .ok_or_else(|| CadError::Unsupported("STEP conversion-based length unit".into()))?;
            let p = list(&measure.parameter);
            let factor = p
                .first()
                .and_then(numeric)
                .ok_or_else(|| CadError::Invalid("Invalid STEP conversion factor".into()))?;
            factor
                * scale(
                    p.get(1)
                        .and_then(reference)
                        .ok_or_else(|| CadError::Invalid("Invalid STEP conversion unit".into()))?,
                    records,
                    stack,
                )?
        } else {
            return Err(CadError::Unsupported(
                "STEP length unit representation".into(),
            ));
        };
        stack.remove(&id);
        Ok(result)
    }
    let mut scales = vec![];
    for group in records.values() {
        for context in group
            .iter()
            .filter(|r| r.name == "GLOBAL_UNIT_ASSIGNED_CONTEXT")
        {
            for unit in list(&context.parameter).iter().flat_map(list) {
                if let Some(id) = reference(unit)
                    && records
                        .get(&id)
                        .is_some_and(|r| r.iter().any(|r| r.name == "LENGTH_UNIT"))
                {
                    scales.push(scale(id, &records, &mut BTreeSet::new())?);
                }
            }
        }
    }
    let first = *scales.first().ok_or_else(|| {
        CadError::Unsupported("STEP has no explicit supported length-unit context".into())
    })?;
    if scales
        .iter()
        .any(|scale| !scale.is_finite() || *scale <= 0. || (scale - first).abs() > first * 1e-10)
    {
        return Err(CadError::Unsupported(
            "Mixed STEP length units require a per-representation unit adapter".into(),
        ));
    }
    Ok(first)
}
