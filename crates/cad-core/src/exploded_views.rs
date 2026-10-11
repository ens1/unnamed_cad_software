//! Saved presentation transforms over configured, mate-solved occurrences.
//! These never change the mechanical placement, mass properties, or solid exports.
use crate::{CadError, Dimension, Document, Expr, Id, PlacedPart, Result};
use nalgebra::{Matrix4, Rotation3, Translation3, Unit, Vector3};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const CONTRACT_VERSION: u32 = 1;
const MAX_STEPS: usize = 128;
const MAX_TARGETS: usize = 1024;
const MAX_CACHED_PLACEMENTS: usize = 1_000_000;
const MAX_CACHE_BYTES: usize = 64 * 1024 * 1024;
const MAX_TARGET_REFERENCES: usize = 16_384;
const MAX_MATCH_WORK: usize = 8_000_000;
const MAX_DISPLAY_MM: f64 = 1e12;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Target {
    /// Complete occurrence path relative to the owning assembly. Subassembly
    /// targets move every active descendant once, including patterned copies.
    pub path: Vec<Id>,
    /// Point in each selected leaf's definition frame for displacement connectors.
    pub anchor: [Expr; 3],
    pub show_line: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Step {
    pub id: Id,
    pub name: String,
    pub targets: Vec<Target>,
    /// Direction is resolved in the owning assembly frame, before explosion.
    pub direction: [Expr; 3],
    pub distance: Expr,
    pub angle: Expr,
    pub pivot: [Expr; 3],
    #[serde(default)]
    pub suppressed: bool,
}
impl Step {
    pub fn new(name: String) -> Self {
        Self {
            id: crate::id(),
            name,
            targets: Vec::new(),
            direction: ["0".into(), "0".into(), "1".into()],
            distance: "30 mm".into(),
            angle: "0 deg".into(),
            pivot: ["0 mm".into(), "0 mm".into(), "0 mm".into()],
            suppressed: false,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Definition {
    pub version: u32,
    pub id: Id,
    pub name: String,
    /// None is reusable in any configuration of this assembly.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub configuration: Option<Id>,
    pub steps: Vec<Step>,
}
impl Definition {
    pub fn new(name: String) -> Self {
        Self {
            version: CONTRACT_VERSION,
            id: crate::id(),
            name,
            configuration: None,
            steps: Vec::new(),
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Line {
    pub step: Id,
    pub path: Vec<Id>,
    pub start_mm: [f64; 3],
    pub end_mm: [f64; 3],
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Stage {
    /// None is the assembled state. Subsequent stages follow saved step order.
    pub step: Option<Id>,
    pub instances: Vec<PlacedPart>,
    pub lines: Vec<Line>,
    pub inactive_targets: Vec<Vec<Id>>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Evaluated {
    pub version: u32,
    pub assembly: Id,
    pub configuration: Option<Id>,
    pub view: Id,
    pub stages: Vec<Stage>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Selection {
    pub view: Id,
    pub stage: usize,
    pub show_lines: bool,
}
fn invalid(message: impl Into<String>) -> CadError {
    CadError::Invalid(format!("Exploded View: {}", message.into()))
}
fn identity(id: &str) -> bool {
    !id.is_empty() && id.len() <= 256 && !id.chars().any(char::is_control)
}
fn path_exists(doc: &Document, owner: &str, path: &[Id]) -> bool {
    let mut owner = owner;
    if path.is_empty() || path.len() > 64 {
        return false;
    }
    for id in path {
        let Some(instance) = doc.assemblies.get(owner).and_then(|a| a.instances.get(id)) else {
            return false;
        };
        owner = &instance.definition;
    }
    doc.parts.contains_key(owner) || doc.assemblies.contains_key(owner)
}
pub fn validate(doc: &Document) -> Result<()> {
    let mut target_budget = 0usize;
    for assembly in doc.assemblies.values() {
        if assembly.exploded_views.len() > 128 {
            return Err(invalid("At most 128 views per assembly"));
        }
        let mut names = BTreeSet::new();
        for (id, view) in &assembly.exploded_views {
            if id != &view.id
                || !identity(id)
                || view.version != CONTRACT_VERSION
                || view.name.trim().is_empty()
                || !names.insert(view.name.to_lowercase())
                || view.steps.len() > MAX_STEPS
            {
                return Err(invalid(
                    "Invalid view identity, version, name, or step count",
                ));
            }
            if view
                .configuration
                .as_ref()
                .is_some_and(|c| !crate::configuration::exists(doc, &assembly.id, c))
            {
                return Err(invalid("The linked assembly configuration is missing"));
            }
            let mut ids = BTreeSet::new();
            for step in &view.steps {
                if !identity(&step.id)
                    || !ids.insert(&step.id)
                    || step.name.trim().is_empty()
                    || step.targets.is_empty()
                    || step.targets.len() > MAX_TARGETS
                {
                    return Err(invalid(
                        "A step needs a unique identity, name and 1–1024 targets",
                    ));
                }
                target_budget = target_budget
                    .checked_add(step.targets.len())
                    .ok_or_else(|| invalid("Target source budget overflowed"))?;
                if target_budget > MAX_TARGET_REFERENCES {
                    return Err(invalid(
                        "At most 16384 exploded target references per document",
                    ));
                }
                let mut paths = step.targets.iter().map(|t| &t.path).collect::<Vec<_>>();
                paths.sort();
                if paths.windows(2).any(|p| p[1].starts_with(p[0])) {
                    return Err(invalid(
                        "A step cannot select an occurrence twice or both a subassembly and its descendant",
                    ));
                }
                for target in &step.targets {
                    if !path_exists(doc, &assembly.id, &target.path) {
                        return Err(invalid(
                            "A step targets a missing occurrence; repair or remove the step before deleting its component",
                        ));
                    }
                }
            }
        }
    }
    Ok(())
}
pub fn put(doc: &mut Document, assembly: &str, view: Definition) -> Result<()> {
    doc.assemblies
        .get_mut(assembly)
        .ok_or_else(|| invalid("Assembly is missing"))?
        .exploded_views
        .insert(view.id.clone(), view);
    validate(doc)
}
pub fn remove(doc: &mut Document, assembly: &str, view: &str) -> Result<()> {
    if doc
        .drawings
        .iter()
        .any(|s| s.exploded_view.as_deref() == Some(view))
    {
        return Err(invalid(
            "A drawing uses this exploded view; change its view before deletion",
        ));
    }
    if doc
        .assemblies
        .get_mut(assembly)
        .ok_or_else(|| invalid("Assembly is missing"))?
        .exploded_views
        .remove(view)
        .is_none()
    {
        return Err(invalid("View is missing"));
    }
    Ok(())
}
fn value(doc: &Document, owner: &str, e: &Expr, dimension: Dimension) -> Result<f64> {
    let value = crate::expression::value(doc, owner, e, dimension)?;
    if !value.is_finite() {
        return Err(invalid("All step quantities must be finite"));
    }
    Ok(value)
}
fn vector(doc: &Document, owner: &str, e: &[Expr; 3], d: Dimension) -> Result<Vector3<f64>> {
    Ok(Vector3::new(
        value(doc, owner, &e[0], d)?,
        value(doc, owner, &e[1], d)?,
        value(doc, owner, &e[2], d)?,
    ))
}
fn matrix(i: &PlacedPart) -> Matrix4<f64> {
    Matrix4::from_fn(|r, c| i.matrix[r][c])
}
fn point(m: &Matrix4<f64>, p: Vector3<f64>) -> Result<[f64; 3]> {
    let p = m.fixed_view::<3, 3>(0, 0) * p + m.fixed_view::<3, 1>(0, 3);
    if p.iter().any(|v| !v.is_finite() || v.abs() > MAX_DISPLAY_MM) {
        return Err(invalid(
            "Exploded display coordinates must remain within ±1e12 mm",
        ));
    }
    Ok(p.into())
}
/// Evaluates all views on the current root assembly over the already solved
/// occurrences. Saved views on nested definitions become available when opened
/// as the document root; they never silently replace a parent's display scope.
pub fn evaluate(doc: &Document, instances: &[PlacedPart]) -> Result<BTreeMap<Id, Evaluated>> {
    let Some(assembly) = doc.assemblies.get(&doc.root) else {
        return Ok(BTreeMap::new());
    };
    let configuration = doc.active_configurations.get(&doc.root).cloned();
    let scoped = crate::configuration::scope(doc, doc, &doc.root, configuration.as_deref())?;
    let mut output = BTreeMap::new();
    let mut budget = 0usize;
    let mut line_budget = 0usize;
    let mut byte_budget = 0usize;
    let mut match_work = 0usize;
    // The serialized base includes all names, IDs, paths and configurations.
    // 480 bytes per placement bounds sixteen finite JSON matrix numbers plus
    // separators independently of the smaller current matrix representation.
    let placement_bytes = serde_json::to_vec(instances)?
        .len()
        .checked_add(instances.len().saturating_mul(480))
        .ok_or_else(|| invalid("Display cache byte estimate overflowed"))?;
    for view in assembly
        .exploded_views
        .values()
        .filter(|v| v.configuration.is_none() || v.configuration == configuration)
    {
        budget = budget
            .checked_add(instances.len().saturating_mul(view.steps.len() + 1))
            .ok_or_else(|| invalid("Display cache size exceeded bounds"))?;
        if budget > MAX_CACHED_PLACEMENTS {
            return Err(invalid(
                "Exploded views exceed one million cached occurrence placements; reduce views or steps",
            ));
        }
        byte_budget = byte_budget
            .checked_add(placement_bytes.saturating_mul(view.steps.len() + 1))
            .ok_or_else(|| invalid("Display cache byte budget overflowed"))?;
        if byte_budget > MAX_CACHE_BYTES {
            return Err(invalid(
                "Exploded display cache exceeds 64 MiB; reduce views or steps",
            ));
        }
        let mut current = instances.to_vec();
        let mut lines = Vec::new();
        let mut stages = vec![Stage {
            step: None,
            instances: current.clone(),
            lines: Vec::new(),
            inactive_targets: Vec::new(),
        }];
        for step in &view.steps {
            let mut inactive_targets = Vec::new();
            if !step.suppressed {
                let cost = step
                    .targets
                    .iter()
                    .map(|t| t.path.len())
                    .sum::<usize>()
                    .checked_mul(current.len())
                    .ok_or_else(|| invalid("Occurrence match work overflowed"))?;
                match_work = match_work
                    .checked_add(cost)
                    .ok_or_else(|| invalid("Occurrence match work overflowed"))?;
                if match_work > MAX_MATCH_WORK {
                    return Err(invalid(
                        "Exploded target matching exceeds eight million path comparisons; use subassembly targets or fewer steps",
                    ));
                }
                let direction = vector(&scoped, &doc.root, &step.direction, Dimension::Scalar)?;
                let norm = direction.norm();
                if !norm.is_finite() || norm <= 1e-12 {
                    return Err(invalid(format!(
                        "{} needs a nonzero finite direction",
                        step.name
                    )));
                }
                let direction = direction / norm;
                let distance = value(&scoped, &doc.root, &step.distance, Dimension::Length)?;
                let angle = value(&scoped, &doc.root, &step.angle, Dimension::Angle)?;
                let pivot = vector(&scoped, &doc.root, &step.pivot, Dimension::Length)?;
                let transform = Translation3::from(direction * distance).to_homogeneous()
                    * Translation3::from(pivot).to_homogeneous()
                    * Rotation3::from_axis_angle(&Unit::new_unchecked(direction), angle)
                        .to_homogeneous()
                    * Translation3::from(-pivot).to_homogeneous();
                if transform.iter().any(|v| !v.is_finite()) {
                    return Err(invalid("Step transform exceeded finite bounds"));
                }
                for target in &step.targets {
                    let mut active = false;
                    let anchor = vector(&scoped, &doc.root, &target.anchor, Dimension::Length)?;
                    for i in current
                        .iter_mut()
                        .filter(|i| i.path.starts_with(&target.path))
                    {
                        active = true;
                        let before = matrix(i);
                        let after = transform * before;
                        if after.iter().any(|v| !v.is_finite())
                            || (0..3).any(|r| after[(r, 3)].abs() > MAX_DISPLAY_MM)
                        {
                            return Err(invalid(
                                "Exploded display coordinates must remain within ±1e12 mm",
                            ));
                        }
                        if target.show_line {
                            // Anchor expressions belong to the assembly quantities;
                            // their coordinates are in this leaf's local frame.
                            let start_mm = point(&before, anchor)?;
                            let end_mm = point(&after, anchor)?;
                            if (Vector3::from(end_mm) - Vector3::from(start_mm)).norm() > 1e-9 {
                                lines.push(Line {
                                    step: step.id.clone(),
                                    path: i.path.clone(),
                                    start_mm,
                                    end_mm,
                                });
                            }
                        }
                        i.matrix = std::array::from_fn(|r| std::array::from_fn(|c| after[(r, c)]));
                    }
                    if !active {
                        inactive_targets.push(target.path.clone());
                    }
                }
            }
            line_budget = line_budget
                .checked_add(lines.len())
                .ok_or_else(|| invalid("Connector cache exceeded bounds"))?;
            if line_budget > MAX_CACHED_PLACEMENTS {
                return Err(invalid(
                    "Explode connector cache exceeds one million records",
                ));
            }
            byte_budget = byte_budget
                .checked_add(serde_json::to_vec(&lines)?.len())
                .ok_or_else(|| invalid("Connector cache byte budget overflowed"))?;
            if byte_budget > MAX_CACHE_BYTES {
                return Err(invalid(
                    "Exploded display cache exceeds 64 MiB; reduce connectors or steps",
                ));
            }
            stages.push(Stage {
                step: Some(step.id.clone()),
                instances: current.clone(),
                lines: lines.clone(),
                inactive_targets,
            });
        }
        output.insert(
            view.id.clone(),
            Evaluated {
                version: CONTRACT_VERSION,
                assembly: assembly.id.clone(),
                configuration: configuration.clone(),
                view: view.id.clone(),
                stages,
            },
        );
    }
    Ok(output)
}
pub fn selected<'a>(
    e: &'a crate::protocol::Evaluation,
    selection: Option<&Selection>,
) -> Option<&'a Stage> {
    let s = selection?;
    let stages = &e.exploded_views.get(&s.view)?.stages;
    stages.get(s.stage.min(stages.len().checked_sub(1)?))
}
pub fn semantic_elements(
    doc: &Document,
    evaluation: Option<&crate::protocol::Evaluation>,
    state: &str,
) -> Vec<crate::presentation::SemanticElement> {
    let mut out = Vec::new();
    for a in doc.assemblies.values() {
        for v in a.exploded_views.values() {
            out.push(crate::presentation::SemanticElement { id: v.id.clone(), role: "exploded-view".into(), label: v.name.clone(), value: None, state: state.into(),
                commands: vec!["assembly.exploded-edit".into(), "assembly.exploded-show".into(), "assembly.exploded-collapse".into()],
                relationships: vec![a.id.clone()].into_iter().chain(v.configuration.clone()).chain(v.steps.iter().flat_map(|s| s.targets.iter().flat_map(|t| t.path.clone()))).collect(),
                data: serde_json::json!({"version":CONTRACT_VERSION,"assembly":a.id,"source":v,"evaluated":evaluation.and_then(|e|e.exploded_views.get(&v.id)),"mechanical_placements_unchanged":true}) });
            for s in &v.steps {
                out.push(crate::presentation::SemanticElement { id:s.id.clone(), role:"explode-step".into(), label:s.name.clone(), value:Some(s.distance.0.clone()), state:if s.suppressed{"suppressed"}else{state}.into(),
                    commands:vec!["assembly.exploded-edit".into(),"assembly.exploded-step".into()], relationships:vec![a.id.clone(),v.id.clone()].into_iter().chain(s.targets.iter().flat_map(|t|t.path.clone())).collect(),
                    data:serde_json::json!({"assembly":a.id,"view":v.id,"source":s,"direction_dimension":"scalar","distance_dimension":"length","angle_dimension":"angle","pivot_dimension":"length"}) });
            }
        }
    }
    out
}
