//! Hierarchical occurrence navigation over source IDs. Tree visibility is
//! presentation state; selecting an occurrence does not enter its definition.
use crate::{Workspace, presentation_controls as controls};
use bevy_egui::egui;
use cad_core::{Document, Id, Instance, commands::Command, presentation::SemanticElement};
use serde_json::{Value, json};
use std::collections::BTreeSet;

#[derive(Clone)]
struct Node {
    id: String,
    name: String,
    path: Vec<Id>,
    definition: Option<Id>,
    kind: &'static str,
    children: Vec<Node>,
}
#[derive(Default)]
pub struct State {
    key: String,
    roots: Vec<Node>,
    expanded: BTreeSet<String>,
    query: String,
}
fn key(w: &Workspace) -> String {
    format!(
        "{}:{}:{}:{:?}",
        w.session.visible().id,
        w.session.generation,
        w.session.committed.revision,
        w.session.draft.as_ref().map(|d| (&d.id, d.edit))
    )
}
fn build(doc: &Document, owner: &str, path: &[Id], active: &[Vec<Id>], depth: usize) -> Vec<Node> {
    if depth > 64 {
        return vec![];
    }
    let Some(assembly) = doc.assemblies.get(owner) else {
        return vec![];
    };
    let mut ordinary = Vec::new();
    let mut groups = std::collections::BTreeMap::<Id, Node>::new();
    for instance in assembly.instances.values() {
        let mut path = path.to_vec();
        path.push(instance.id.clone());
        if !active.iter().any(|leaf| leaf.starts_with(&path)) {
            continue;
        }
        let is_assembly = doc.assemblies.contains_key(&instance.definition);
        let node = Node {
            id: format!("component:{}", path.join("/")),
            name: instance.name.clone(),
            definition: Some(instance.definition.clone()),
            kind: if is_assembly { "assembly" } else { "part" },
            children: if is_assembly {
                build(doc, &instance.definition, &path, active, depth + 1)
            } else {
                vec![]
            },
            path,
        };
        if let Some(pattern) = assembly
            .component_patterns
            .values()
            .find(|p| p.occurrences.iter().any(|o| o.instance == instance.id))
        {
            groups
                .entry(pattern.id.clone())
                .or_insert_with(|| Node {
                    id: format!(
                        "component-pattern:{}/{}",
                        node.path[..node.path.len() - 1].join("/"),
                        pattern.id
                    ),
                    name: pattern.name.clone(),
                    path: node.path[..node.path.len() - 1].to_vec(),
                    definition: None,
                    kind: "pattern",
                    children: vec![],
                })
                .children
                .push(node);
        } else {
            ordinary.push(node);
        }
    }
    ordinary.extend(groups.into_values());
    sort(&mut ordinary);
    ordinary
}
fn natural(value: &str) -> Vec<(String, Option<u64>)> {
    let mut out = Vec::new();
    let mut token = String::new();
    let mut digit = false;
    for c in value.chars().chain(std::iter::once('\0')) {
        if c == '\0' || c.is_ascii_digit() != digit {
            if !token.is_empty() {
                out.push(if digit {
                    (String::new(), token.parse().ok())
                } else {
                    (token.to_lowercase(), None)
                });
                token.clear();
            }
            digit = c.is_ascii_digit();
        }
        if c != '\0' {
            token.push(c);
        }
    }
    out
}
fn sort(nodes: &mut [Node]) {
    nodes.sort_by(|a, b| {
        natural(&a.name)
            .cmp(&natural(&b.name))
            .then(a.id.cmp(&b.id))
    });
    for node in nodes {
        sort(&mut node.children);
    }
}
fn refresh(w: &Workspace, state: &mut State) {
    let next = key(w);
    if state.key == next {
        return;
    }
    let doc = w.session.visible();
    let active = doc
        .placed_parts()
        .unwrap_or_default()
        .into_iter()
        .map(|p| p.path)
        .collect::<Vec<_>>();
    state.roots = build(doc, &doc.root, &[], &active, 0);
    state.key = next;
}
fn lookup<'a>(doc: &'a Document, path: &[Id]) -> Option<(&'a str, &'a Instance)> {
    let mut owner = doc.root.as_str();
    for (index, id) in path.iter().enumerate() {
        let instance = doc.assemblies.get(owner)?.instances.get(id)?;
        if index + 1 == path.len() {
            return Some((owner, instance));
        }
        owner = &instance.definition;
    }
    None
}
pub fn selection_label(w: &Workspace) -> Option<String> {
    let path = w.selected_component.as_ref()?;
    let (_, instance) = lookup(w.session.visible(), path)?;
    Some(format!("{} · component", instance.name))
}
fn rename(w: &mut Workspace, path: &[Id]) {
    if w.tool_checkpoint.is_some() || w.sketch.is_some() {
        w.status = "Finish the current operation before renaming a component".into();
        return;
    }
    let Some((_, instance)) = lookup(w.session.visible(), path) else {
        return;
    };
    let name = instance.name.clone();
    w.fields
        .insert("assembly-tree-rename".into(), json!(path).to_string());
    w.fields.insert("assembly-tree-name".into(), name);
}
pub fn select(w: &mut Workspace, path: Vec<Id>) {
    let Some((_, instance)) = lookup(w.session.visible(), &path) else {
        return;
    };
    let definition = instance.definition.clone();
    let object = instance.id.clone();
    let name = instance.name.clone();
    w.selected = None;
    w.geometry_selection.clear();
    w.candidates.clear();
    w.selection_popup = None;
    w.selected_feature_path = None;
    w.view.active_node = None;
    w.view.active_instance = Some(object.clone());
    w.selected_component = Some(path);
    crate::selection_ui::object_selected(
        w,
        cad_core::selection_actions::SelectionTarget::Object {
            definition,
            object,
            role: "component-instance".into(),
        },
    );
    w.status = format!("Selected {name} · Move, Pattern, or Edit definition");
    w.rendered_key.clear();
}
fn edit(w: &mut Workspace, path: Vec<Id>) {
    if w.tool_checkpoint.is_some() || w.sketch.is_some() {
        w.status = "Finish the current operation before editing a component".into();
        return;
    }
    let Some((_, instance)) = lookup(w.session.visible(), &path) else {
        return;
    };
    let definition = instance.definition.clone();
    select(w, path.clone());
    if w.session.visible().parts.contains_key(&definition) {
        w.activate_context(Some(definition), Some(path));
    } else {
        w.fields.insert("assembly-id".into(), definition);
        w.open_editor("assembly");
    }
}
fn matches(node: &Node, query: &str) -> bool {
    query.is_empty()
        || node.name.to_lowercase().contains(query)
        || node.children.iter().any(|n| matches(n, query))
}
fn rows<'a>(nodes: &'a [Node], state: &State, depth: usize, out: &mut Vec<(&'a Node, usize)>) {
    let query = state.query.to_lowercase();
    for node in nodes {
        if !matches(node, &query) {
            continue;
        }
        out.push((node, depth));
        if !query.is_empty() || state.expanded.contains(&node.id) {
            rows(&node.children, state, depth + 1, out);
        }
    }
}
pub fn show(ui: &mut egui::Ui, w: &mut Workspace) {
    let mut state = std::mem::take(&mut w.assembly_tree);
    refresh(w, &mut state);
    ui.add(
        egui::TextEdit::singleline(&mut state.query)
            .hint_text("Find components or patterns")
            .desired_width(f32::INFINITY),
    );
    let mut visible = Vec::new();
    rows(&state.roots, &state, 0, &mut visible);
    let props = controls::properties(
        "assembly.tree-row",
        "component-instance",
        &["feature-tree-row"],
        "ready",
    );
    let height = controls::number(&props, "height", 22.)
        .max(controls::number(&props, "min-target-size", 22.))
        .max(controls::number(&props, "min-height", 0.));
    egui::ScrollArea::vertical()
        .id_salt("assembly-occurrence-tree")
        .max_height(ui.clip_rect().height().max(height))
        .show_rows(ui, height, visible.len(), |ui, range| {
            let mut toggled = Vec::new();
            for index in range {
                let (node, depth) = visible[index];
                ui.horizontal(|ui| {
                    ui.add_space(depth as f32 * controls::number(&props, "indent", 12.));
                    if !node.children.is_empty() {
                        if controls::button(
                            ui,
                            &format!("{}/toggle", node.id),
                            "component-disclosure",
                            &["compact"],
                            if state.expanded.contains(&node.id) {
                                "▾"
                            } else {
                                "▸"
                            },
                            true,
                            false,
                        )
                        .clicked()
                        {
                            toggled.push(node.id.clone());
                        }
                    } else {
                        ui.add_space(18.);
                    }
                    let selected = w.selected_component.as_ref() == Some(&node.path)
                        && node.definition.is_some();
                    let label = if node.kind == "pattern" {
                        format!("{} · {} copies", node.name, node.children.len())
                    } else {
                        node.name.clone()
                    };
                    let response = controls::button(
                        ui,
                        &node.id,
                        "component-instance",
                        &["feature-tree-row", node.kind],
                        &label,
                        true,
                        selected,
                    )
                    .on_hover_text(format!(
                        "{} · {}\nClick to select · Double-click to edit",
                        node.name, node.kind
                    ));
                    if response.double_clicked() && node.definition.is_some() {
                        edit(w, node.path.clone());
                    } else if response.clicked() {
                        if node.definition.is_some() {
                            select(w, node.path.clone());
                        } else {
                            toggled.push(node.id.clone());
                        }
                    }
                    if node.definition.is_some() {
                        response.context_menu(|ui| {
                            if ui.button("Edit definition").clicked() {
                                edit(w, node.path.clone());
                                ui.close();
                            }
                            if ui.button("Move").clicked() {
                                select(w, node.path.clone());
                                crate::ui::action(w, "assembly.move");
                                ui.close();
                            }
                            if ui.button("Rotate").clicked() {
                                select(w, node.path.clone());
                                crate::ui::action(w, "assembly.rotate");
                                ui.close();
                            }
                            if ui.button("Rename occurrence").clicked() {
                                rename(w, &node.path);
                                ui.close();
                            }
                            if ui.button("Ground / release").clicked() {
                                ground(w, &node.path);
                                ui.close();
                            }
                        });
                    }
                });
            }
            for id in toggled {
                if !state.expanded.remove(&id) {
                    state.expanded.insert(id);
                }
            }
        });
    if w.tool_checkpoint.is_none()
        && w.sketch.is_none()
        && let Some(path) = w
            .fields
            .get("assembly-tree-rename")
            .and_then(|s| serde_json::from_str::<Vec<Id>>(s).ok())
        && let Some((owner, instance)) = lookup(w.session.visible(), &path)
    {
        let owner = owner.to_owned();
        let mut instance = instance.clone();
        let text = w
            .fields
            .entry("assembly-tree-name".into())
            .or_insert_with(|| instance.name.clone());
        let response = ui.text_edit_singleline(text);
        if response.lost_focus()
            && ui.input(|i| i.key_pressed(egui::Key::Enter))
            && !text.trim().is_empty()
        {
            instance.name = text.trim().into();
            w.fields.remove("assembly-tree-rename");
            w.fields.remove("assembly-tree-name");
            w.start_tool();
            w.dispatch(Command::PutInstance {
                assembly: owner,
                instance,
            });
            w.confirm_tool();
        }
    }
    w.assembly_tree = state;
}
fn ground(w: &mut Workspace, path: &[Id]) {
    if w.tool_checkpoint.is_some() || w.sketch.is_some() {
        return;
    }
    let Some((owner, instance)) = lookup(w.session.visible(), path) else {
        return;
    };
    let owner = owner.to_owned();
    let mut instance = instance.clone();
    instance.fixed = !instance.fixed;
    w.start_tool();
    w.dispatch(Command::PutInstance {
        assembly: owner,
        instance,
    });
    w.confirm_tool();
}
pub fn semantics(w: &Workspace) -> Vec<SemanticElement> {
    if w.view.active_part.is_some() {
        return vec![];
    }
    let mut state = State::default();
    let roots = if w.assembly_tree.key == key(w) {
        &w.assembly_tree.roots
    } else {
        refresh(w, &mut state);
        &state.roots
    };
    fn visit(
        w: &Workspace,
        nodes: &[Node],
        parent: &str,
        depth: usize,
        out: &mut Vec<SemanticElement>,
    ) {
        for node in nodes {
            out.push(SemanticElement{id:node.id.clone(),role:"component-instance".into(),label:node.name.clone(),value:Some(node.kind.into()),
                state:if w.selected_component.as_ref()==Some(&node.path)&&node.definition.is_some(){"selected"}else{"ready"}.into(),
                commands:if node.definition.is_some(){vec!["assembly.tree-select".into(),"assembly.tree-edit".into(),"assembly.tree-ground".into(),"assembly.tree-rename".into()]}else{vec!["assembly.tree-toggle".into()]},relationships:vec![parent.into()],
                data:json!({"contract":"tcad.presentation.assembly-tree","version":1,"document":w.session.visible().id,"path":node.path,"definition":node.definition,"kind":node.kind,"depth":depth,"children":node.children.iter().map(|n|&n.id).collect::<Vec<_>>(),"node":node.id,"classes":["feature-tree-row",node.kind]})});
            visit(w, &node.children, &node.id, depth + 1, out);
        }
    }
    let mut out = vec![];
    visit(w, roots, "assembly.tree", 0, &mut out);
    out
}
pub fn action(w: &mut Workspace, command: &str, args: Option<&Value>) -> bool {
    if !command.starts_with("assembly.tree-") {
        return false;
    }
    let Some(args) =
        args.filter(|v| v.get("document").and_then(Value::as_str) == Some(&w.session.visible().id))
    else {
        return true;
    };
    let path = args
        .get("path")
        .and_then(|v| serde_json::from_value::<Vec<Id>>(v.clone()).ok())
        .unwrap_or_default();
    match command {
        "assembly.tree-select" => select(w, path),
        "assembly.tree-edit" => edit(w, path),
        "assembly.tree-ground" => ground(w, &path),
        "assembly.tree-rename" => rename(w, &path),
        "assembly.tree-toggle" => {
            if let Some(id) = args.get("node").and_then(Value::as_str)
                && !w.assembly_tree.expanded.remove(id)
            {
                w.assembly_tree.expanded.insert(id.into());
            }
        }
        _ => {}
    }
    true
}
