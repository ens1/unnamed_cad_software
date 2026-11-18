//! Part browser. Sketch ownership, labels and actions are shared by the docked
//! tree and independently placed model browsers. Filtering never edits the part.
use crate::{Workspace, cad_ui::feature_row, presentation_controls as controls};
use bevy_egui::egui;
use cad_core::{Feature, NodeRef, Operation, Part, presentation::SemanticElement};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Default)]
pub(super) struct State {
    queries: BTreeMap<String, String>,
    expanded: BTreeSet<NodeRef>,
    last_selection: Option<NodeRef>,
}

const CLEAR: &str = "model-tree.clear-filter";
const TOGGLE: &str = "model-tree.toggle-sketches";

fn sketch(feature: &Feature) -> bool {
    matches!(
        feature.operation,
        Operation::Profile { .. } | Operation::Sketch3d { .. }
    )
}

struct Branch<'a> {
    feature: &'a Feature,
    sketches: Vec<&'a Feature>,
}

fn branches(part: &Part) -> Vec<Branch<'_>> {
    let sketches: BTreeMap<_, _> = part
        .features
        .iter()
        .filter(|f| sketch(f))
        .map(|f| (f.id.as_str(), f))
        .collect();
    let mut consumed = BTreeSet::new();
    let mut branches = Vec::with_capacity(part.features.len());
    for feature in &part.features {
        let mut inputs = Vec::new();
        if !sketch(feature) {
            for source in part.feature_dependencies(feature) {
                if source.definition == part.id
                    && let Some(&input) = sketches.get(source.node.as_str())
                {
                    consumed.insert(input.id.as_str());
                    inputs.push(input);
                }
            }
        }
        branches.push(Branch {
            feature,
            sketches: inputs,
        });
    }
    branches.retain(|branch| !consumed.contains(branch.feature.id.as_str()));
    branches
}
fn query<'a>(w: &'a Workspace, part: &str) -> &'a str {
    w.assembly_tree
        .features
        .queries
        .get(part)
        .map(String::as_str)
        .unwrap_or("")
}
pub(super) fn field_value(w: &Workspace, data: &Value) -> Option<Value> {
    let part = data["part"].as_str()?;
    (w.view.active_part.as_deref() == Some(part)).then(|| json!(query(w, part)))
}
pub(super) fn set_query(w: &mut Workspace, part: &str, value: &str) {
    if value.is_empty() {
        w.assembly_tree.features.queries.remove(part);
    } else {
        w.assembly_tree
            .features
            .queries
            .insert(part.into(), value.into());
    }
}
fn search_field(w: &Workspace, part: &Part) -> SemanticElement {
    let query = query(w, &part.id);
    SemanticElement {
        id: format!("feature-tree:{}:filter", part.id),
        role: "feature-tree-search".into(),
        label: "Find feature".into(),
        value: Some(query.into()),
        state: "ready".into(),
        commands: vec![],
        relationships: vec![format!("feature-tree:{}:features", part.id)],
        data: json!({"contract":"tcad.presentation.model-tree", "version":1, "document":w.session.visible().id, "part":part.id, "setter":"assembly-tree", "field":"feature-query", "json":query, "editable":true, "value_type":"string", "classes":["search-field","compact"], "accessibility_label":"Find a feature by name or type"}),
    }
}
fn selection(w: &Workspace, part: &Part) -> Option<NodeRef> {
    w.view.active_node.as_ref().map(|id| part.reference(id))
}
fn expanded(w: &Workspace, part: &Part, branch: &Branch<'_>) -> bool {
    w.assembly_tree
        .features
        .expanded
        .contains(&part.reference(&branch.feature.id))
        || (w.assembly_tree.features.last_selection != selection(w, part)
            && branch
                .sketches
                .iter()
                .any(|sketch| w.view.active_node.as_ref() == Some(&sketch.id)))
}
fn disclosure(w: &Workspace, part: &Part, branch: &Branch<'_>) -> SemanticElement {
    let open = expanded(w, part, branch);
    let feature = &branch.feature;
    SemanticElement {
        id: format!("feature-tree:{}:inputs:{}", part.id, feature.id),
        role: "feature-disclosure".into(),
        label: format!(
            "{} sketches for {}",
            if open { "Hide" } else { "Show" },
            feature.name
        ),
        value: None,
        state: if open { "expanded" } else { "collapsed" }.into(),
        commands: vec![TOGGLE.into()],
        relationships: vec![feature.id.clone()],
        data: json!({"contract":"tcad.presentation.model-tree","version":1,"document":w.session.visible().id,"part":part.id,"feature":feature.id,"expanded":open,"children":branch.sketches.iter().map(|s| &s.id).collect::<Vec<_>>(),"classes":["compact","tree-disclosure"]}),
    }
}
fn clear_action(w: &Workspace, part: &Part) -> SemanticElement {
    SemanticElement {
        id: format!("feature-tree:{}:clear-filter", part.id),
        role: "feature-tree-action".into(),
        label: "Clear filter".into(),
        value: None,
        state: "ready".into(),
        commands: vec![CLEAR.into()],
        relationships: vec![format!("feature-tree:{}:filter", part.id)],
        data: json!({"contract":"tcad.presentation.model-tree","version":1,"document":w.session.visible().id,"part":part.id}),
    }
}
pub(super) fn semantics(w: &Workspace, part: &Part) -> Vec<SemanticElement> {
    let mut elements = vec![search_field(w, part)];
    if !query(w, &part.id).is_empty() {
        elements.push(clear_action(w, part));
    }
    elements.extend(
        branches(part)
            .iter()
            .filter(|branch| !branch.sketches.is_empty())
            .map(|branch| disclosure(w, part, branch)),
    );
    elements
}
pub(super) fn action(w: &mut Workspace, command: &str, data: &Value) -> bool {
    if !matches!(command, CLEAR | TOGGLE) {
        return false;
    }
    let Some(part_id) = data["part"]
        .as_str()
        .filter(|id| Some(*id) == w.view.active_part.as_deref())
    else {
        return true;
    };
    if data["document"].as_str() != Some(&w.session.visible().id) {
        return true;
    }
    if command == CLEAR {
        set_query(w, part_id, "");
    } else if let Some(part) = w.session.visible().parts.get(part_id)
        && let Some(branch) = branches(part).into_iter().find(|branch| {
            Some(branch.feature.id.as_str()) == data["feature"].as_str()
                && !branch.sketches.is_empty()
        })
    {
        let leaf = part.reference(&branch.feature.id);
        let was_open = expanded(w, part, &branch);
        // Capture the selection that made the branch open, so an explicit close sticks.
        w.assembly_tree.features.last_selection = selection(w, part);
        if was_open {
            w.assembly_tree.features.expanded.remove(&leaf);
        } else {
            w.assembly_tree.features.expanded.insert(leaf);
        }
    }
    true
}

pub(crate) fn show(ui: &mut egui::Ui, w: &mut Workspace, part: &Part) {
    if part.features.is_empty() && w.sketch.is_none() && query(w, &part.id).is_empty() {
        return;
    }
    controls::inspector_section(
        ui,
        controls::InspectorSection {
            id: &format!("feature-tree:{}:features", part.id),
            role: "feature-tree-group",
            classes: &["model-features"],
            label: "Features",
            state: "ready",
            collapse: Some(true),
            open: None,
        },
        |ui| {
            if part.features.len() >= 8 || !query(w, &part.id).is_empty() {
                crate::ui::semantic_field(ui, w, &search_field(w, part));
            }
            if !query(w, &part.id).is_empty() {
                let clear = clear_action(w, part);
                if controls::button(
                    ui,
                    &clear.id,
                    &clear.role,
                    &["compact"],
                    &clear.label,
                    true,
                    false,
                )
                .clicked()
                {
                    action(w, CLEAR, &clear.data);
                }
            }
            let filter = query(w, &part.id).trim().to_lowercase();
            if filter.is_empty() {
                let branches = branches(part);
                let selected = selection(w, part);
                if w.assembly_tree.features.last_selection != selected {
                    // Select in the viewport and reveal the source sketch in the browser.
                    for branch in &branches {
                        if branch
                            .sketches
                            .iter()
                            .any(|s| Some(&s.id) == w.view.active_node.as_ref())
                        {
                            w.assembly_tree
                                .features
                                .expanded
                                .insert(part.reference(&branch.feature.id));
                        }
                    }
                    w.assembly_tree.features.last_selection = selected;
                }
                for branch in branches {
                    if branch.sketches.is_empty() {
                        feature_row(ui, w, part, branch.feature);
                        continue;
                    }
                    let toggle = disclosure(w, part, &branch);
                    let open = toggle.data["expanded"] == true;
                    ui.horizontal(|ui| {
                        let disclosure = controls::button(
                            ui,
                            &toggle.id,
                            &toggle.role,
                            &["tree-disclosure", "compact"],
                            if open { "▾" } else { "▸" },
                            true,
                            false,
                        )
                        .on_hover_text(&toggle.label);
                        disclosure.widget_info(|| {
                            egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &toggle.label)
                        });
                        if disclosure.clicked() {
                            action(w, TOGGLE, &toggle.data);
                        }
                        feature_row(ui, w, part, branch.feature);
                    });
                    if open {
                        ui.indent(&branch.feature.id, |ui| {
                            for sketch in &branch.sketches {
                                feature_row(ui, w, part, sketch);
                            }
                        });
                    }
                }
            } else {
                let mut found = false;
                for feature in &part.features {
                    if feature.name.to_lowercase().contains(&filter)
                        || feature.operation.name().to_lowercase().contains(&filter)
                    {
                        found = true;
                        feature_row(ui, w, part, feature);
                    }
                }
                if !found {
                    ui.weak("No matching features");
                }
            }
            if let Some(s) = &w.sketch
                && !part.features.iter().any(|f| f.id == s.feature)
            {
                ui.colored_label(
                    ui.visuals().selection.stroke.color,
                    format!("{} (Editing)", s.name),
                );
            }
        },
    );
}
