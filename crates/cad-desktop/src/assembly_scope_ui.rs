//! Inspect all occurrences affected by an in-place definition edit without changing selection.
use crate::{Workspace, camera_views_ui, presentation_controls as controls, viewport};
use bevy_egui::egui;
use cad_core::{
    Document, Id, PlacedPart, camera_views::Pose, component_fork, presentation::SemanticElement,
    protocol::RequestIdentity,
};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Mutex;

pub const INSPECT: &str = "assembly.inspect-shared-instances";
pub const FORK: &str = "assembly.make-edited-instance-independent";
const CLASSES: &[&str] = &["semantic-control", "viewport-control", "instance-scope"];

#[derive(Default)]
pub struct State {
    overview: Option<(Id, Option<Pose>)>,
    counts: Mutex<Option<CountCache>>,
    // Only the two edit anchors of each accepted occurrence fork. This view state
    // lasts as long as the project’s in-memory undo history.
    forks: Vec<(Anchor, Anchor)>,
}

struct CountCache {
    identity: RequestIdentity,
    definition: Id,
    evaluated: bool,
    total: Option<usize>,
    active: Option<usize>,
}

impl State {
    pub fn highlight(&self) -> Option<&str> {
        self.overview.as_ref().map(|(id, _)| id.as_str())
    }
}

pub fn highlighted(w: &Workspace, i: &PlacedPart) -> bool {
    w.view.isolation
        && w.view.active_part.as_ref() == Some(&i.definition)
        && w.assembly_scope.highlight() == Some(i.definition.as_str())
}

pub fn close(w: &mut Workspace) -> bool {
    let Some((_, pose)) = w.assembly_scope.overview.take() else {
        return false;
    };
    if let Some(pose) = pose {
        camera_views_ui::set_current(w, pose);
        crate::navigation::stop(w);
    }
    w.rendered_key.clear();
    true
}

// Count stored uses of the definition even if a configuration suppresses their geometry.
// Shared subassemblies contribute their quantity once per occurrence without expanding the tree.
fn stored_count(
    doc: &Document,
    definition: &str,
    owner: &str,
    memo: &mut BTreeMap<Id, usize>,
    visiting: &mut BTreeSet<Id>,
) -> Option<usize> {
    if definition == owner {
        return Some(1);
    }
    if let Some(n) = memo.get(owner) {
        return Some(*n);
    }
    let Some(assembly) = doc.assemblies.get(owner) else {
        return Some(0);
    };
    if visiting.len() >= 64 || !visiting.insert(owner.into()) {
        return None;
    }
    let mut count = 0usize;
    for instance in assembly.instances.values() {
        count = count.checked_add(stored_count(
            doc,
            definition,
            &instance.definition,
            memo,
            visiting,
        )?)?;
    }
    visiting.remove(owner);
    memo.insert(owner.into(), count);
    Some(count)
}

fn counts(w: &Workspace, definition: &str) -> (Option<usize>, Option<usize>) {
    let identity = crate::source_cache::identity(w);
    let evaluation = viewport::display_evaluation(w).filter(|e| e.identity == identity && e.valid);
    let mut cache = w
        .assembly_scope
        .counts
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    if let Some(old) = cache.as_ref()
        && old.identity == identity
        && old.definition == definition
        && old.evaluated == evaluation.is_some()
    {
        return (old.total, old.active);
    }
    let doc = w.session.visible();
    // A saved project can contain assembly definitions outside the active root.
    // Start at each top-level assembly exactly once, reusing the descendant counts.
    let referenced: BTreeSet<_> = doc
        .assemblies
        .values()
        .flat_map(|assembly| assembly.instances.values().map(|i| &i.definition))
        .collect();
    let additional_roots = doc
        .assemblies
        .keys()
        .filter(|owner| **owner != doc.root && !referenced.contains(owner));
    let mut memo = BTreeMap::new();
    let mut visiting = BTreeSet::new();
    let mut total =
        std::iter::once(&doc.root)
            .chain(additional_roots)
            .try_fold(0usize, |total, root| {
                total.checked_add(stored_count(
                    doc,
                    definition,
                    root,
                    &mut memo,
                    &mut visiting,
                )?)
            });
    let active = evaluation.map(|e| {
        let mut active = 0;
        for i in e.instances.iter().filter(|i| i.definition == definition) {
            active += 1;
            // A configuration can generate additional component-pattern placements.
            if crate::assembly_tree_ui::lookup(doc, &i.path).is_none() {
                total = total.and_then(|count| count.checked_add(1));
            }
        }
        active
    });
    *cache = Some(CountCache {
        identity,
        definition: definition.into(),
        evaluated: evaluation.is_some(),
        total,
        active,
    });
    (total, active)
}

fn fork_reason(w: &Workspace, definition: &str) -> Option<String> {
    if w.tool_checkpoint.is_some()
        || w.session.draft.is_some()
        || w.sketch.is_some()
        || w.confirm_requested
        || w.assembly_motion.is_some()
        || w.settings.is_some()
    {
        return Some("Finish the current edit first".into());
    }
    let Some(path) = w.view.editing_occurrence.as_deref() else {
        return Some("Edit a specific instance from the assembly tree".into());
    };
    component_fork::check_occurrence(w.session.visible(), path, definition)
        .err()
        .map(|e| e.to_string())
}

pub fn semantics(w: &Workspace) -> Vec<SemanticElement> {
    if crate::drawing_ui::is_workspace(w) || !crate::workspace_controls::in_place(w) {
        return vec![];
    }
    let Some(definition) = w.view.active_part.as_deref() else {
        return vec![];
    };
    let (total, active) = counts(w, definition);
    let open = w.assembly_scope.highlight() == Some(definition);
    let label = match (total, active) {
        (Some(n), Some(a)) if n != a => format!("{a} of {n} instances"),
        (Some(1), _) => "1 instance".into(),
        (Some(n), _) => format!("{n} instances"),
        _ => "Instances".into(),
    };
    let tip = match (total, active) {
        (Some(n), Some(a)) if n != a => {
            format!(
                "{a} placed in this assembly configuration · {n} instances across project assemblies"
            )
        }
        (_, Some(_)) if open => "Return to the editing view".into(),
        (_, Some(_)) => "Show these instances in this project".into(),
        _ => "Waiting for the current assembly geometry".into(),
    };
    let mut elements = vec![];
    let unavailable = (!open && active.is_none_or(|n| n == 0)).then(|| tip.clone());
    for (command, label, selected, reason, help) in [
        (INSPECT, label, open, unavailable, tip),
        (
            FORK,
            "Make this one independent".into(),
            false,
            if open {
                fork_reason(w, definition)
            } else {
                None
            },
            "Create a copy for the instance being edited".into(),
        ),
    ] {
        if command == FORK && !open {
            continue;
        }
        let reason = crate::cad_ui::command_disabled_reason(w, command)
            .map(str::to_owned)
            .or(reason);
        let state = if reason.is_some() {
            "disabled"
        } else if selected {
            "selected"
        } else {
            "ready"
        };
        let accessibility = format!("{label}. {help}");
        elements.push(SemanticElement {
            id: command.into(),
            role: "workspace-action".into(),
            label,
            value: reason.clone(),
            state: state.into(),
            commands: vec![command.into()],
            relationships: vec!["viewport.toolbar".into(), definition.into()],
            data: json!({
                "contract": "tcad.presentation.assembly-edit-scope",
                "version": 1,
                "document": w.session.visible().id,
                "definition": definition,
                "editing_path": w.view.editing_occurrence,
                "instances_in_project": total,
                "instances_in_configuration": active,
                "expanded": open,
                "classes": CLASSES,
                "accessibility_label": accessibility,
                "tooltip": reason.unwrap_or(help),
            }),
        });
    }
    elements
}

pub fn show(ui: &mut egui::Ui, w: &mut Workspace, command: &str) {
    let Some(item) = semantics(w).into_iter().find(|item| item.id == command) else {
        return;
    };
    if controls::button(
        ui,
        &item.id,
        &item.role,
        CLASSES,
        &item.label,
        item.state != "disabled",
        item.state == "selected",
    )
    .on_hover_text(item.data["tooltip"].as_str().unwrap_or_default())
    .on_disabled_hover_text(item.data["tooltip"].as_str().unwrap_or_default())
    .clicked()
    {
        crate::ui::semantic_action(w, &item, &item.id);
    }
}

pub fn action(w: &mut Workspace, command: &str, data: Option<&Value>) -> bool {
    if !matches!(command, INSPECT | FORK) {
        return false;
    }
    let Some(item) = semantics(w).into_iter().find(|item| item.id == command) else {
        w.status = "This edit scope is no longer active".into();
        return true;
    };
    if data
        .filter(|d| d["contract"] == "tcad.presentation.assembly-edit-scope")
        .is_some_and(|data| {
            ["document", "definition", "editing_path"]
                .iter()
                .any(|key| data[*key] != item.data[*key])
        })
    {
        w.status = "The component being edited has changed".into();
        return true;
    }
    if let Some(reason) = item.value {
        w.status = reason;
        return true;
    }
    let Some(definition) = w.view.active_part.clone() else {
        return true;
    };
    if command == INSPECT {
        if !close(w) {
            let frame =
                w.sketch.is_none() && w.tool_checkpoint.is_none() && w.session.draft.is_none();
            let camera = frame.then(|| camera_views_ui::current(w));
            w.assembly_scope.overview = Some((definition, camera));
            if frame {
                w.pending_context_focus = false;
                viewport::fit_shared_instances(w);
            }
            w.rendered_key.clear();
        }
    } else if let Some(path) = w.view.editing_occurrence.clone() {
        close(w);
        crate::component_fork_ui::begin(
            w,
            &definition,
            component_fork::Target::Occurrence { path },
        );
    }
    true
}

// Forking a nested instance also copies its enclosing assemblies and renames their local IDs.
// Follow those explicit correspondences in either direction through Undo and Redo.
#[derive(PartialEq, Eq)]
struct Anchor {
    definition: Id,
    path: Vec<Id>,
}

pub fn remember_fork(
    w: &mut Workspace,
    intent: &component_fork::Intent,
    plan: &component_fork::Plan,
) {
    if let (component_fork::Target::Occurrence { path: from }, Some(to)) =
        (&intent.target, &plan.occurrence_path)
    {
        w.assembly_scope.forks.push((
            Anchor {
                definition: intent.definition.clone(),
                path: from.clone(),
            },
            Anchor {
                definition: plan.definition.clone(),
                path: to.clone(),
            },
        ));
    }
}

pub struct HistoryRoute {
    steps: Vec<(Id, Id, Option<Id>)>,
    anchor: Anchor,
}
pub fn history_route(w: &Workspace) -> Option<HistoryRoute> {
    if !crate::workspace_controls::in_place(w) {
        return None;
    }
    let doc = w.session.visible();
    let current_path = w.view.editing_occurrence.as_ref()?;
    let mut owner = &doc.root;
    let mut route = vec![];
    for id in current_path {
        let child = doc.assemblies.get(owner)?.instances.get(id)?;
        let original = doc
            .component_forks
            .get(owner)
            .filter(|f| f.source_document == doc.id)
            .and_then(|f| {
                f.correspondence
                    .iter()
                    .find(|(_, local)| *local == id)
                    .map(|(original, _)| original.clone())
            });
        route.push((owner.clone(), id.clone(), original));
        owner = &child.definition;
    }
    (w.view.active_part.as_ref() == Some(owner)).then(|| HistoryRoute {
        steps: route,
        anchor: Anchor {
            definition: owner.clone(),
            path: current_path.clone(),
        },
    })
}

pub fn restore_route(w: &mut Workspace, route: HistoryRoute) {
    let doc = w.session.visible();
    if crate::assembly_tree_ui::lookup(doc, &route.anchor.path)
        .is_some_and(|(_, i)| i.definition == route.anchor.definition)
    {
        return;
    }
    // Linked forks retain upstream ancestry, whose IDs differ from the pinned
    // host copy. The accepted planner already identified the exact before/after
    // paths. Verify the destination against the current history entry.
    if let Some(alternative) = w.assembly_scope.forks.iter().rev().find_map(|(from, to)| {
        let candidate = if from == &route.anchor {
            to
        } else if to == &route.anchor {
            from
        } else {
            return None;
        };
        crate::assembly_tree_ui::lookup(doc, &candidate.path)
            .is_some_and(|(_, i)| i.definition == candidate.definition)
            .then_some(candidate)
    }) {
        restore_anchor(w, alternative.definition.clone(), alternative.path.clone());
        return;
    }
    let mut owner = &doc.root;
    let mut path = vec![];
    for (previous, id, original) in route.steps {
        let Some(assembly) = doc.assemblies.get(owner) else {
            w.leave_part_context();
            return;
        };
        let mapped = doc
            .component_forks
            .get(owner)
            .filter(|f| f.source_document == doc.id && f.source_definition == previous)
            .and_then(|f| f.correspondence.get(&id));
        let Some(child) = std::iter::once(&id)
            .chain(mapped)
            .chain(original.as_ref())
            .find_map(|key| assembly.instances.get(key))
        else {
            w.leave_part_context();
            return;
        };
        path.push(child.id.clone());
        owner = &child.definition;
    }
    if !doc.parts.contains_key(owner) {
        w.leave_part_context();
        return;
    }
    restore_anchor(w, owner.clone(), path);
}

fn restore_anchor(w: &mut Workspace, owner: Id, path: Vec<Id>) {
    if w.view.active_part.as_ref() != Some(&owner)
        || w.view.editing_occurrence.as_ref() != Some(&path)
    {
        w.view.active_node = None;
        w.selected = None;
        w.geometry_selection.clear();
        w.context_selection = None;
        w.selected_feature_path = None;
        w.selected_component = Some(path.clone());
        w.view.active_part = Some(owner);
        w.view.active_instance = path.last().cloned();
        w.edge_selection_path = Some(path.clone());
        w.view.editing_occurrence = Some(path);
        w.rendered_key.clear();
    }
}
