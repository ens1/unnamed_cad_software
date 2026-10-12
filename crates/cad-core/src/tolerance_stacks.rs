//! Saved linear dimensional stacks, with explicit nominal bindings and variation assumptions.
use crate::{CadError, Dimension, Document, Expr, Id, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
pub const CONTRACT_VERSION: u32 = 1;
pub const MAX_STACKS: usize = 128;
pub const MAX_TERMS: usize = 128;
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Scope {
    pub definition: Id,
    #[serde(default)]
    pub configuration: Option<Id>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Nominal {
    Expression {
        scope: Scope,
        value: Expr,
    },
    Parameter {
        scope: Scope,
        parameter: Id,
    },
    SketchDimension {
        scope: Scope,
        sketch: Id,
        constraint: Id,
    },
}
impl Nominal {
    pub fn scope(&self) -> &Scope {
        match self {
            Self::Expression { scope, .. }
            | Self::Parameter { scope, .. }
            | Self::SketchDimension { scope, .. } => scope,
        }
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Distribution {
    #[default]
    Unspecified,
    Uniform,
    /// Sigma and process mean offset are explicitly supplied, never inferred from tolerance limits.
    Normal {
        sigma: Expr,
        mean_offset: Expr,
    },
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Term {
    pub id: Id,
    pub name: String,
    pub nominal: Nominal,
    pub coefficient: f64,
    pub lower_deviation: Expr,
    pub upper_deviation: Expr,
    #[serde(default)]
    pub distribution: Distribution,
    /// Terms with the same nonempty group share one variation variable, rather than independent copies.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variation_group: Option<String>,
    #[serde(default = "enabled")]
    pub enabled: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Definition {
    #[serde(default = "version")]
    pub version: u32,
    pub id: Id,
    pub name: String,
    pub scope: Scope,
    pub terms: Vec<Term>,
    #[serde(default)]
    pub lower_requirement: Option<Expr>,
    #[serde(default)]
    pub upper_requirement: Option<Expr>,
    #[serde(default)]
    pub assume_independent_groups: bool,
    #[serde(default = "sigma_multiplier")]
    pub sigma_multiplier: f64,
    #[serde(default)]
    pub notes: String,
}
fn version() -> u32 {
    CONTRACT_VERSION
}
fn enabled() -> bool {
    true
}
fn sigma_multiplier() -> f64 {
    3.
}
fn invalid(message: impl Into<String>) -> CadError {
    CadError::Invalid(format!("Tolerance stack: {}", message.into()))
}
fn identity(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && !matches!(value, "." | "..")
        && !value
            .chars()
            .any(|c| c.is_control() || matches!(c, '/' | '\\' | ':'))
}
fn text(value: &str) -> Result<()> {
    if value.trim().is_empty() || value.len() > 4096 {
        return Err(invalid("A dimensional expression requires bounded text"));
    }
    crate::expression::references(value)?;
    Ok(())
}
fn scope_valid(scope: &Scope) -> bool {
    identity(&scope.definition) && scope.configuration.as_ref().is_none_or(|id| identity(id))
}
pub fn validate(document: &Document) -> Result<()> {
    if document.tolerance_stacks.len() > MAX_STACKS {
        return Err(invalid("A document is limited to 128 stacks"));
    }
    let mut names = BTreeSet::new();
    for (id, stack) in &document.tolerance_stacks {
        if id != &stack.id
            || !identity(id)
            || stack.version != CONTRACT_VERSION
            || stack.name.trim().is_empty()
            || stack.name.len() > 256
            || stack.notes.len() > 4096
            || !names.insert(stack.name.trim().to_lowercase())
            || !scope_valid(&stack.scope)
            || stack.terms.is_empty()
            || stack.terms.len() > MAX_TERMS
            || !stack.sigma_multiplier.is_finite()
            || !(0. ..=10.).contains(&stack.sigma_multiplier)
        {
            return Err(invalid(
                "Stack identity, name, scope, rows or multiplier are invalid",
            ));
        }
        for expression in stack
            .lower_requirement
            .iter()
            .chain(&stack.upper_requirement)
        {
            text(&expression.0)?;
        }
        let mut ids = BTreeSet::new();
        for term in &stack.terms {
            if !identity(&term.id)
                || !ids.insert(&term.id)
                || term.name.trim().is_empty()
                || term.name.len() > 256
                || !term.coefficient.is_finite()
                || term.coefficient.abs() > 1000.
                || !scope_valid(term.nominal.scope())
                || term.variation_group.as_ref().is_some_and(|group| {
                    group.trim().is_empty()
                        || group.len() > 256
                        || group.chars().any(char::is_control)
                })
            {
                return Err(invalid(
                    "Each term needs a stable identity, name, scope and finite coefficient",
                ));
            }
            text(&term.lower_deviation.0)?;
            text(&term.upper_deviation.0)?;
            match &term.nominal {
                Nominal::Expression { value, .. } => text(&value.0)?,
                Nominal::Parameter { parameter, .. } => {
                    if !identity(parameter) {
                        return Err(invalid("Invalid parameter binding"));
                    }
                }
                Nominal::SketchDimension {
                    sketch, constraint, ..
                } => {
                    if !identity(sketch) || !identity(constraint) {
                        return Err(invalid("Invalid driving sketch dimension binding"));
                    }
                }
            }
            if let Distribution::Normal { sigma, mean_offset } = &term.distribution {
                text(&sigma.0)?;
                text(&mean_offset.0)?;
            }
        }
    }
    Ok(())
}
pub fn put(document: &mut Document, stack: Definition) -> Result<()> {
    let mut candidate = document.clone();
    candidate.tolerance_stacks.insert(stack.id.clone(), stack);
    validate(&candidate)?;
    *document = candidate;
    Ok(())
}
#[derive(Default)]
struct Sum {
    value: f64,
    correction: f64,
}
impl Sum {
    fn add(&mut self, value: f64) {
        let corrected = value - self.correction;
        let next = self.value + corrected;
        self.correction = (next - self.value) - corrected;
        self.value = next;
    }
}
type Contexts = BTreeMap<(Id, Option<Id>), std::result::Result<Document, String>>;
fn context<'a>(
    document: &Document,
    contexts: &'a mut Contexts,
    scope: &Scope,
) -> Result<&'a Document> {
    let key = (scope.definition.clone(), scope.configuration.clone());
    if !contexts.contains_key(&key) && contexts.len() >= 8 {
        contexts.clear();
    }
    contexts
        .entry(key)
        .or_insert_with(|| {
            if !document.parts.contains_key(&scope.definition)
                && !document.assemblies.contains_key(&scope.definition)
            {
                return Err("The nominal's definition is missing".into());
            }
            let configuration = scope.configuration.as_deref().or_else(|| {
                document
                    .active_configurations
                    .get(&scope.definition)
                    .map(String::as_str)
            });
            if configuration.is_some_and(|configuration| {
                !crate::configuration::exists(document, &scope.definition, configuration)
            }) {
                return Err("The nominal's configuration is missing".into());
            }
            crate::configuration::variant(document, &scope.definition, configuration)
                .map_err(|error| error.to_string())
        })
        .as_ref()
        .map_err(|error| invalid(error.clone()))
}
fn length(context: &Document, scope: &Scope, expression: &Expr) -> Result<f64> {
    let value =
        crate::expression::value(context, &scope.definition, expression, Dimension::Length)?;
    if value.is_finite() {
        Ok(value)
    } else {
        Err(invalid("A length exceeded finite bounds"))
    }
}
fn nominal(context: &Document, source: &Nominal) -> Result<f64> {
    let scope = source.scope();
    let expression = match source {
        Nominal::Expression { value, .. } => value,
        Nominal::Parameter { parameter, .. } => {
            let parameter = context
                .parts
                .get(&scope.definition)
                .map(|part| &part.parameters)
                .or_else(|| {
                    context
                        .assemblies
                        .get(&scope.definition)
                        .map(|assembly| &assembly.parameters)
                })
                .and_then(|parameters| parameters.get(parameter))
                .ok_or_else(|| invalid("The nominal parameter is missing"))?;
            if parameter.dimension != Dimension::Length {
                return Err(invalid("A linear stack requires length parameters"));
            }
            &parameter.expression
        }
        Nominal::SketchDimension {
            sketch, constraint, ..
        } => {
            let sketch = context
                .parts
                .get(&scope.definition)
                .and_then(|part| part.sketches.get(sketch))
                .ok_or_else(|| invalid("The nominal sketch is missing"))?;
            let candidates = sketch
                .constraints
                .iter()
                .filter(|candidate| &candidate.id == constraint)
                .collect::<Vec<_>>();
            if candidates.len() != 1
                || candidates[0].driven
                || !crate::sketch_actions::is_dimension(&candidates[0].kind)
                || matches!(
                    candidates[0].kind.as_str(),
                    "angle" | "arc_angle" | "rotation"
                )
            {
                return Err(invalid(
                    "The nominal needs one current linear driving dimension",
                ));
            }
            &candidates[0].value
        }
    };
    length(context, scope, expression)
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Contribution {
    pub term: Id,
    pub nominal_mm: f64,
    pub coefficient: f64,
    pub lower_deviation_mm: f64,
    pub upper_deviation_mm: f64,
    pub weighted_nominal_mm: f64,
    pub variation_group: Option<String>,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Calculated,
    WithinWorstCaseLimits,
    ExceedsWorstCaseLimits,
    Unavailable,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Statistics {
    pub mean_mm: f64,
    pub sigma_mm: f64,
    pub multiplier: f64,
    pub spread_mm: [f64; 2],
    pub within_requirements: Option<bool>,
    pub assumption: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Reading {
    pub stack: Id,
    pub status: Status,
    pub nominal_mm: Option<f64>,
    pub worst_case_mm: Option<[f64; 2]>,
    pub requirements_mm: [Option<f64>; 2],
    pub statistics: Option<Statistics>,
    pub contributions: Vec<Contribution>,
    pub diagnostics: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Report {
    pub version: u32,
    pub source: String,
    pub stacks: BTreeMap<Id, Reading>,
}
struct Variation {
    coefficient: Sum,
    lower: f64,
    upper: f64,
    process: Option<(f64, f64)>,
}
fn assess(bounds: [f64; 2], limits: [Option<f64>; 2]) -> Option<bool> {
    (limits[0].is_some() || limits[1].is_some()).then(|| {
        limits[0].is_none_or(|lower| bounds[0] >= lower)
            && limits[1].is_none_or(|upper| bounds[1] <= upper)
    })
}
fn calculate(document: &Document, stack: &Definition, contexts: &mut Contexts) -> Result<Reading> {
    let scope_context = context(document, contexts, &stack.scope)?;
    let lower = stack
        .lower_requirement
        .as_ref()
        .map(|expression| length(scope_context, &stack.scope, expression))
        .transpose()?;
    let upper = stack
        .upper_requirement
        .as_ref()
        .map(|expression| length(scope_context, &stack.scope, expression))
        .transpose()?;
    if lower.zip(upper).is_some_and(|(lower, upper)| lower > upper) {
        return Err(invalid("Result requirements must have ordered bounds"));
    }
    let mut total = Sum::default();
    let mut contributions = vec![];
    let mut variations: BTreeMap<String, Variation> = BTreeMap::new();
    let mut diagnostics = vec![];
    for term in stack.terms.iter().filter(|term| term.enabled) {
        let context = context(document, contexts, term.nominal.scope())?;
        let nominal = nominal(context, &term.nominal)?;
        let lower = length(context, term.nominal.scope(), &term.lower_deviation)?;
        let upper = length(context, term.nominal.scope(), &term.upper_deviation)?;
        if lower > upper {
            return Err(invalid(format!(
                "{} has reversed deviation bounds",
                term.name
            )));
        }
        let weighted = term.coefficient * nominal;
        if !weighted.is_finite() {
            return Err(invalid("A weighted nominal exceeded finite bounds"));
        }
        total.add(weighted);
        let process = match &term.distribution {
            Distribution::Unspecified => None,
            Distribution::Uniform => {
                Some((lower / 2. + upper / 2., (upper - lower) / 12f64.sqrt()))
            }
            Distribution::Normal { sigma, mean_offset } => {
                let sigma = length(context, term.nominal.scope(), sigma)?;
                let mean = length(context, term.nominal.scope(), mean_offset)?;
                if sigma < 0. {
                    return Err(invalid(format!(
                        "{} needs a nonnegative process sigma",
                        term.name
                    )));
                }
                if mean < lower || mean > upper {
                    diagnostics.push(format!(
                        "{}: the supplied process mean lies outside its tolerance band",
                        term.name
                    ));
                }
                Some((mean, sigma))
            }
        };
        if process.is_some_and(|(mean, sigma)| !mean.is_finite() || !sigma.is_finite()) {
            return Err(invalid("A process assumption exceeded finite bounds"));
        }
        let group = term
            .variation_group
            .as_ref()
            .map(|group| format!("group:{}", group.trim()))
            .unwrap_or_else(|| format!("term:{}", term.id));
        if let Some(variation) = variations.get_mut(&group) {
            if variation.lower != lower || variation.upper != upper || variation.process != process
            {
                return Err(invalid(format!(
                    "{} uses a shared variation group with inconsistent deviation/process assumptions",
                    term.name
                )));
            }
            variation.coefficient.add(term.coefficient);
        } else {
            let mut coefficient = Sum::default();
            coefficient.add(term.coefficient);
            variations.insert(
                group,
                Variation {
                    coefficient,
                    lower,
                    upper,
                    process,
                },
            );
        }
        contributions.push(Contribution {
            term: term.id.clone(),
            nominal_mm: nominal,
            coefficient: term.coefficient,
            lower_deviation_mm: lower,
            upper_deviation_mm: upper,
            weighted_nominal_mm: weighted,
            variation_group: term.variation_group.clone(),
        });
    }
    if contributions.is_empty() {
        return Err(invalid("Enable at least one dimensional term"));
    }
    let mut low = Sum::default();
    let mut high = Sum::default();
    let mut mean = Sum::default();
    let mut sigma = 0f64;
    let mut complete = true;
    for variation in variations.values() {
        let coefficient = variation.coefficient.value;
        let first = coefficient * variation.lower;
        let second = coefficient * variation.upper;
        low.add(first.min(second));
        high.add(first.max(second));
        if let Some((offset, spread)) = variation.process {
            mean.add(coefficient * offset);
            sigma = sigma.hypot(coefficient * spread);
        } else if coefficient != 0. {
            complete = false;
        }
    }
    let nominal = total.value;
    let bounds = [nominal + low.value, nominal + high.value];
    let requirements = [lower, upper];
    if !nominal.is_finite() || bounds.iter().any(|value| !value.is_finite()) {
        return Err(invalid("Stack arithmetic exceeded finite bounds"));
    }
    let statistics = if stack.assume_independent_groups && complete {
        let center = nominal + mean.value;
        let spread = [
            center - stack.sigma_multiplier * sigma,
            center + stack.sigma_multiplier * sigma,
        ];
        if !center.is_finite()
            || !sigma.is_finite()
            || spread.iter().any(|value| !value.is_finite())
        {
            return Err(invalid("Statistical spread exceeded finite bounds"));
        }
        Some(Statistics{mean_mm:center,sigma_mm:sigma,multiplier:stack.sigma_multiplier,spread_mm:spread,within_requirements:assess(spread,requirements),assumption:"Independent variation groups; shared groups use one variation variable. A k-sigma spread is not a worst-case guarantee or an inferred process yield.".into()})
    } else {
        diagnostics.push(
            if !stack.assume_independent_groups {
                "Statistics omitted: independence of variation groups has not been declared"
            } else {
                "Statistics omitted: supply a distribution for every non-cancelled variation group"
            }
            .into(),
        );
        None
    };
    let status = match assess(bounds, requirements) {
        None => Status::Calculated,
        Some(true) => Status::WithinWorstCaseLimits,
        Some(false) => Status::ExceedsWorstCaseLimits,
    };
    Ok(Reading {
        stack: stack.id.clone(),
        status,
        nominal_mm: Some(nominal),
        worst_case_mm: Some(bounds),
        requirements_mm: requirements,
        statistics,
        contributions,
        diagnostics,
    })
}
pub fn evaluate(document: &Document) -> Report {
    let source = document.fingerprint();
    let mut contexts = Contexts::new();
    let stacks = document
        .tolerance_stacks
        .values()
        .map(|stack| {
            let reading =
                calculate(document, stack, &mut contexts).unwrap_or_else(|error| Reading {
                    stack: stack.id.clone(),
                    status: Status::Unavailable,
                    nominal_mm: None,
                    worst_case_mm: None,
                    requirements_mm: [None, None],
                    statistics: None,
                    contributions: vec![],
                    diagnostics: vec![error.to_string()],
                });
            (stack.id.clone(), reading)
        })
        .collect();
    Report {
        version: CONTRACT_VERSION,
        source,
        stacks,
    }
}
