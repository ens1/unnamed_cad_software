//! Engineering summaries for a reviewed component update. Exact source changes
//! remain available separately for Advanced Source consumers.
use super::ComponentLink;
use crate::{Document, Id, occurrence_names};
use serde_json::Value;
use std::collections::BTreeSet;

fn scalar(value: Option<&Value>) -> Option<String> {
    match value? {
        Value::String(value) if value.len() <= 96 && uuid::Uuid::parse_str(value).is_err() => {
            Some(value.clone())
        }
        Value::Bool(value) => Some(if *value { "Yes" } else { "No" }.into()),
        Value::Number(value) => Some(value.to_string()),
        _ => None,
    }
}
fn feature_changes(
    name: &str,
    before: &crate::Feature,
    after: &crate::Feature,
    out: &mut Vec<String>,
) {
    let scope = format!("{name} › {}", after.name);
    if before.name != after.name {
        out.push(format!("{name}: renamed {} to {}", before.name, after.name));
    }
    if before.suppressed != after.suppressed {
        out.push(format!(
            "{scope}: {}",
            if after.suppressed {
                "suppressed"
            } else {
                "unsuppressed"
            }
        ));
    }
    if before.operation == after.operation {
        return;
    }
    let a = serde_json::to_value(&before.operation).unwrap_or_default();
    let b = serde_json::to_value(&after.operation).unwrap_or_default();
    let keys = a
        .as_object()
        .into_iter()
        .flat_map(|v| v.keys())
        .chain(b.as_object().into_iter().flat_map(|v| v.keys()))
        .collect::<BTreeSet<_>>();
    for key in keys {
        if a.get(key) == b.get(key) {
            continue;
        }
        let label = key.replace('_', " ");
        if let (Some(before), Some(after)) = (scalar(a.get(key)), scalar(b.get(key))) {
            out.push(format!("{scope}: {label} {before} → {after}"));
        } else {
            out.push(format!("{scope}: {label} changed"));
        }
    }
}
pub(super) fn engineering_changes(
    before: &Document,
    after: &Document,
    old: &ComponentLink,
    new: &ComponentLink,
) -> Vec<String> {
    let keys = old
        .pinned_definitions
        .keys()
        .chain(new.pinned_definitions.keys())
        .collect::<BTreeSet<_>>();
    let mut output = Vec::new();
    for id in keys {
        let name = occurrence_names::definition(after, id);
        match (before.parts.get(id), after.parts.get(id)) {
            (None, Some(_)) => output.push(format!("Added part {name}")),
            (Some(part), None) => output.push(format!("Removed part {}", part.name)),
            (Some(a), Some(b)) => {
                if a.name != b.name {
                    output.push(format!("Renamed part {} to {}", a.name, b.name));
                }
                if a.material != b.material {
                    let material = |doc: &Document, id: &str| {
                        doc.materials
                            .get(id)
                            .map(|material| material.name.clone())
                            .unwrap_or_else(|| "Unavailable material".into())
                    };
                    output.push(format!(
                        "{name}: material {} → {}",
                        material(before, &a.material),
                        material(after, &b.material)
                    ));
                } else if before.materials.get(&a.material) != after.materials.get(&b.material) {
                    output.push(format!("{name}: engineering material properties changed"));
                }
                if a.finish != b.finish {
                    let finish = |doc: &Document, id: &str| {
                        doc.finishes
                            .get(id)
                            .map(|finish| finish.name.clone())
                            .unwrap_or_else(|| "Unavailable finish".into())
                    };
                    output.push(format!(
                        "{name}: finish {} → {}",
                        finish(before, &a.finish),
                        finish(after, &b.finish)
                    ));
                } else if before.finishes.get(&a.finish) != after.finishes.get(&b.finish) {
                    output.push(format!("{name}: finish appearance changed"));
                }
                let parameters = a
                    .parameters
                    .keys()
                    .chain(b.parameters.keys())
                    .collect::<BTreeSet<_>>();
                for parameter in parameters {
                    match (a.parameters.get(parameter), b.parameters.get(parameter)) {
                        (Some(a), Some(b)) if a != b => output.push(format!(
                            "{name}: {} {} → {}",
                            b.name, a.expression.0, b.expression.0
                        )),
                        (None, Some(b)) => output.push(format!(
                            "{name}: added parameter {} = {}",
                            b.name, b.expression.0
                        )),
                        (Some(a), None) => {
                            output.push(format!("{name}: removed parameter {}", a.name))
                        }
                        _ => {}
                    }
                }
                let features = a
                    .features
                    .iter()
                    .map(|f| &f.id)
                    .chain(b.features.iter().map(|f| &f.id))
                    .collect::<BTreeSet<_>>();
                for feature in features {
                    match (
                        a.features.iter().find(|f| &f.id == feature),
                        b.features.iter().find(|f| &f.id == feature),
                    ) {
                        (Some(a), Some(b)) => feature_changes(&name, a, b, &mut output),
                        (None, Some(b)) => output.push(format!("{name}: added {}", b.name)),
                        (Some(a), None) => output.push(format!("{name}: removed {}", a.name)),
                        _ => {}
                    }
                }
                if a.features
                    .iter()
                    .map(|feature| &feature.id)
                    .collect::<Vec<_>>()
                    != b.features
                        .iter()
                        .map(|feature| &feature.id)
                        .collect::<Vec<_>>()
                    && a.features.len() == b.features.len()
                {
                    output.push(format!("{name}: feature order changed"));
                }
                if a.sketches != b.sketches {
                    output.push(format!(
                        "{name}: sketch geometry, dimensions or relations changed"
                    ));
                }
                if a.outputs != b.outputs {
                    output.push(format!("{name}: output bodies changed"));
                }
                if a.properties != b.properties {
                    output.push(format!("{name}: engineering properties changed"));
                }
                if a.configuration_details != b.configuration_details
                    || a.configurations != b.configurations
                {
                    output.push(format!("{name}: configurations changed"));
                }
                if a.face_finishes != b.face_finishes {
                    output.push(format!("{name}: face finishes changed"));
                }
            }
            _ => {}
        }
        match (before.assemblies.get(id), after.assemblies.get(id)) {
            (None, Some(_)) => output.push(format!("Added assembly {name}")),
            (Some(a), None) => output.push(format!("Removed assembly {}", a.name)),
            (Some(a), Some(b)) => {
                if a.name != b.name {
                    output.push(format!("Renamed assembly {} to {}", a.name, b.name));
                }
                let components = a
                    .instances
                    .keys()
                    .chain(b.instances.keys())
                    .collect::<BTreeSet<&Id>>();
                for component in components {
                    match (a.instances.get(component), b.instances.get(component)) {
                        (None, Some(instance)) => output.push(format!(
                            "{name}: added component {}",
                            occurrence_names::instance(after, id, instance)
                        )),
                        (Some(instance), None) => output.push(format!(
                            "{name}: removed component {}",
                            occurrence_names::instance(before, id, instance)
                        )),
                        (Some(a), Some(b)) if a != b => output.push(format!(
                            "{name}: component {} changed",
                            occurrence_names::instance(after, id, b)
                        )),
                        _ => {}
                    }
                }
                if a.mates != b.mates {
                    output.push(format!("{name}: assembly mates changed"));
                }
                if a.component_patterns != b.component_patterns {
                    output.push(format!("{name}: component patterns changed"));
                }
                if a.weld_beads != b.weld_beads {
                    output.push(format!("{name}: assembly weld beads changed"));
                }
                if a.rigid_groups != b.rigid_groups {
                    output.push(format!("{name}: rigid component groups changed"));
                }
                if a.inspections != b.inspections {
                    output.push(format!("{name}: saved inspection definitions changed"));
                }
                if a.exploded_views != b.exploded_views {
                    output.push(format!("{name}: exploded views changed"));
                }
                if a.display_states != b.display_states {
                    output.push(format!("{name}: display states changed"));
                }
                if a.configurations != b.configurations {
                    output.push(format!("{name}: assembly configurations changed"));
                }
                if a.properties != b.properties
                    || a.parameters != b.parameters
                    || a.promoted_parameters != b.promoted_parameters
                {
                    output.push(format!("{name}: assembly properties or parameters changed"));
                }
            }
            _ => {}
        }
    }
    if output.is_empty() {
        output.push(
            "Source revision or metadata changed; no modeling definition changes found.".into(),
        );
    }
    if output.len() > 256 {
        let remaining = output.len() - 256;
        output.truncate(256);
        output.push(format!("{remaining} further changes; see Advanced Source."));
    }
    output
}
