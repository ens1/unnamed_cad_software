//! Searchable current semantic actions, independent of palette layout or styles.
use crate::{Workspace, presentation_controls as controls};
use bevy_egui::egui;
use cad_core::{
    CadError, Result,
    presentation::{ActionDescriptor, SemanticElement},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;
const QUERY: &str = "command-query";
const ONLY: &str = "command-palette-only-available";
const INDEX: &str = "command-palette-index";
const RETURN: &str = "command-palette-return";
const TREE: &str = "command-palette-return-tree";
const BROWSE: &str = "command-palette-browse";
const PAGE_SIZE: usize = 12;
const SUGGESTION_LIMIT: usize = 5;
#[derive(Clone)]
struct Candidate {
    id: String,
    label: String,
    category: String,
    source: SemanticElement,
    action: ActionDescriptor,
    context: bool,
}
pub fn before_open(w: &mut Workspace) {
    if w.dialog.as_deref() != Some("commands") {
        w.fields
            .insert(RETURN.into(), w.dialog.clone().unwrap_or_default());
        w.fields.insert(TREE.into(), w.manager_tree.to_string());
        w.fields.remove(INDEX);
        w.fields.remove(QUERY);
        w.fields.remove(BROWSE);
        w.fields
            .insert("focus-request:command-palette:query".into(), "true".into());
    }
}
pub fn escape(w: &mut Workspace) -> bool {
    if w.dialog.as_deref() != Some("commands") {
        return false;
    }
    restore(w);
    true
}
pub fn context_editor(w: &Workspace) -> Option<&str> {
    if w.dialog.as_deref() == Some("commands") {
        w.fields
            .get(RETURN)
            .map(String::as_str)
            .filter(|dialog| !dialog.is_empty())
    } else {
        w.dialog.as_deref()
    }
}
fn restore(w: &mut Workspace) {
    if w.dialog.as_deref() != Some("commands") {
        return;
    }
    w.dialog = w.fields.remove(RETURN).filter(|dialog| !dialog.is_empty());
    w.manager_tree = w.fields.remove(TREE).is_none_or(|value| value == "true");
    w.fields.remove(INDEX);
}
fn group(command: &str) -> String {
    if let Some(tool) = cad_core::modeling_tools::TOOLS
        .iter()
        .find(|tool| tool.command == command)
    {
        return tool.group.into();
    }
    match command.split('.').next().unwrap_or(command) {
        "file" | "document" => "Document",
        "sketch" | "sketch-library" | "sketch-block-library" => "Sketch",
        "feature" | "surface" => "Modeling",
        "assembly" | "components" | "standard-parts" => "Assembly",
        "drawing" | "production" | "drawings" => "Drawing",
        "material" | "materials" => "Materials",
        "appearance" | "appearances" | "lighting" | "finish-library" => "Appearance",
        "camera-view" | "camera-views" | "viewport" | "navigation" => "View",
        "inspection" | "sensors" | "sensor" | "tolerance" => "Inspection",
        _ => "Workspace",
    }
    .into()
}
fn target_label(w: &Workspace, source: &SemanticElement, action: &ActionDescriptor) -> String {
    let mut label = if action.label == source.label {
        cad_core::presentation::command_catalog()
            .into_iter()
            .find(|(command, _)| *command == action.command)
            .map(|(_, label)| label.to_owned())
            .unwrap_or_else(|| action.label.clone())
    } else {
        action.label.clone()
    };
    let document = w.session.visible();
    let target = ["finish", "material", "view", "sensor", "stack", "library"]
        .into_iter()
        .find_map(|key| {
            let id = action
                .arguments
                .get(key)
                .or_else(|| source.data.get(key))?
                .as_str()?;
            match key {
                "finish" => document.finishes.get(id).map(|value| value.name.as_str()),
                "material" => document.materials.get(id).map(|value| value.name.as_str()),
                "view" => document
                    .camera_views
                    .get(id)
                    .map(|value| value.name.as_str()),
                _ => None,
            }
        });
    let bound_record = matches!(
        source.role.as_str(),
        "material-library-entry"
            | "finish-library-entry"
            | "design-monitor"
            | "tolerance-stack"
            | "tolerance-stack-term"
            | "sketch-library-entry"
            | "camera-view"
            | "drawing-sheet"
            | "drawing-annotation"
            | "exploded-view"
            | "explode-step"
    );
    let target = target.or_else(|| bound_record.then_some(source.label.as_str()));
    if let Some(target) = target.filter(|target| !target.is_empty())
        && !label.to_lowercase().contains(&target.to_lowercase())
    {
        label.push_str(" · ");
        label.push_str(target);
    }
    label
}
fn relevant(w: &Workspace, source: &SemanticElement, action: &ActionDescriptor) -> bool {
    if source.state == "current" || source.state == "selected" {
        return true;
    }
    if let Some(finish) = action.arguments.get("finish").and_then(Value::as_str)
        && crate::appearances_ui::selected_finish(w).as_deref() == Some(finish)
    {
        return true;
    }
    if let Some(view) = action.arguments.get("view").and_then(Value::as_str)
        && w.fields.get("camera-view-selected").map(String::as_str) == Some(view)
    {
        return true;
    }
    let category = group(&action.command);
    match context_editor(w) {
        Some("appearances" | "appearance-panel" | "finish-library") => category == "Appearance",
        Some("materials") => category == "Materials",
        Some("camera-views") => category == "View",
        Some("drawings" | "drawing-editor" | "production") => category == "Drawing",
        Some("sketch") => category == "Sketch",
        Some("assembly" | "component-insert" | "mates") => category == "Assembly",
        _ => false,
    }
}
// A catalog name alone cannot execute an action that needs a saved object.
// Keep it discoverable, but require a bound result below or the owning editor.
fn unbound_reason(command: &str) -> Option<&'static str> {
    match command {
        "drawing.sheet-edit"
        | "drawing.sheet-preview"
        | "drawing.sheet-duplicate"
        | "drawing.sheet-delete"
        | "drawing.sheet-up"
        | "drawing.sheet-down"
        | "drawing.sheet-template-save"
        | "drawing.sheet-format-load" => {
            Some("Create a drawing sheet first, then choose its named command")
        }
        "drawing.annotation-delete" => Some("Select an annotation in a drawing sheet first"),
        "assembly.exploded-edit" | "assembly.exploded-show" | "assembly.exploded-step" => {
            Some("Create an exploded view first, then choose its named command")
        }
        "assembly.inspection-edit" | "assembly.inspection-delete" => {
            Some("Choose a saved inspection in Assembly and Mates")
        }
        "assembly.inspection-highlight"
        | "assembly.inspection-select-first"
        | "assembly.inspection-select-second" => {
            Some("Choose a result pair in the assembly inspection")
        }
        "reference.repair-station" => Some("Choose an upstream station in Repair References"),
        "navigation.set" => Some("Choose a preference in Navigation Settings"),
        _ => None,
    }
}
fn candidates(w: &Workspace) -> Vec<Candidate> {
    let chrome = crate::cad_ui::semantics(w);
    let toolbar: BTreeMap<_, _> = chrome
        .iter()
        .filter_map(|element| Some((element.id.strip_prefix("command-toolbar/")?, element)))
        .collect();
    let mut out = BTreeMap::new();
    for (command, label) in cad_core::presentation::command_catalog() {
        let source = toolbar
            .get(command)
            .copied()
            .cloned()
            .unwrap_or_else(|| SemanticElement {
                id: format!("command:{command}"),
                role: "command".into(),
                label: label.into(),
                value: None,
                state: "ready".into(),
                commands: vec![command.into()],
                relationships: vec![],
                data: json!({}),
            });
        if let Some(mut action) = source
            .action_descriptors()
            .into_iter()
            .find(|action| action.command == command)
        {
            if let Some(reason) = unbound_reason(command) {
                action.enabled = false;
                action.disabled_reason = Some(reason.into());
            }
            out.insert(
                command.to_owned(),
                Candidate {
                    id: format!("command-palette-result:{command}"),
                    label: label.into(),
                    category: group(command),
                    source,
                    action,
                    context: false,
                },
            );
        }
    }
    // Include desktop commands such as Undo, standard views and drawing DXF.
    // Sketch toolbar aliases already have named entries in the core catalog.
    for source in &chrome {
        let Some(command) = source
            .id
            .strip_prefix("command-toolbar/")
            .or_else(|| source.id.strip_prefix("command-menu/"))
        else {
            continue;
        };
        if out.contains_key(command) || command.starts_with("sketch.tool.") {
            continue;
        }
        if let Some(action) = source
            .action_descriptors()
            .into_iter()
            .find(|action| action.command == command)
        {
            out.insert(
                command.to_owned(),
                Candidate {
                    id: format!("command-palette-result:{command}"),
                    label: source.label.clone(),
                    category: group(command),
                    source: source.clone(),
                    action,
                    context: false,
                },
            );
        }
    }
    let mut dynamic: BTreeMap<String, BTreeMap<String, Candidate>> = BTreeMap::new();
    for source in crate::materials_ui::semantics(w)
        .into_iter()
        .chain(crate::theme_library::semantics(w))
        .chain(
            cad_core::drawing::semantic_elements(w.session.visible(), "ready")
                .into_iter()
                .filter(|element| {
                    matches!(
                        element.role.as_str(),
                        "drawing-sheet" | "drawing-annotation"
                    )
                }),
        )
        .chain(cad_core::exploded_views::semantic_elements(
            w.session.visible(),
            None,
            "ready",
        ))
        .chain(crate::camera_views_ui::elements(w))
        .chain(crate::drawing_interaction_ui::semantics(w))
        .chain(crate::inspection_ui::semantics(w))
        .chain(crate::appearances_ui::semantics(w))
        .chain(crate::finish_library_ui::semantics(w))
        .chain(crate::properties_ui::semantics(w))
        .chain(crate::standard_parts_ui::semantics(w))
        .chain(crate::sensors_ui::semantics(w))
        .chain(crate::tolerance_stacks_ui::semantics(w))
        .chain(crate::sketch_block_library_ui::elements(w))
        .chain(crate::sketch_overlay::semantics(w))
        .chain(crate::document_identity_ui::semantics(w))
    {
        let bound_record = matches!(
            source.role.as_str(),
            "material-library-entry"
                | "finish-library-entry"
                | "design-monitor"
                | "tolerance-stack"
                | "tolerance-stack-term"
                | "sketch-library-entry"
                | "camera-view"
                | "drawing-sheet"
                | "drawing-annotation"
                | "exploded-view"
                | "explode-step"
        );
        if !(source.id.starts_with("command:")
            || source.role == "command"
            || source.role.ends_with("-action")
            || bound_record)
        {
            continue;
        }
        for action in source.action_descriptors() {
            let command = action.command.clone();
            let digest = format!(
                "{:x}",
                Sha256::digest(serde_json::to_vec(&action.arguments).unwrap_or_default())
            );
            let id = format!("command-palette-bound:{}:{command}:{digest}", source.id);
            dynamic.entry(command.clone()).or_default().insert(
                id.clone(),
                Candidate {
                    id,
                    label: target_label(w, &source, &action),
                    category: group(&command),
                    source: source.clone(),
                    context: relevant(w, &source, &action),
                    action,
                },
            );
        }
    }
    let mut bound = Vec::new();
    for (command, values) in dynamic {
        out.remove(&command);
        bound.extend(values.into_values());
    }
    let mut result = out.into_values().chain(bound).collect::<Vec<_>>();
    for source in crate::selection_ui::semantics(w)
        .into_iter()
        .filter(|source| source.role == "selection-action")
    {
        for action in source.action_descriptors() {
            let label = target_label(w, &source, &action);
            result.push(Candidate {
                id: format!("command-palette-context:{}:{}", source.id, action.command),
                label,
                category: "Selected geometry".into(),
                source: source.clone(),
                action,
                context: true,
            });
        }
    }
    for candidate in &mut result {
        if let Some(reason) = crate::cad_ui::command_disabled_reason(w, &candidate.action.command) {
            candidate.action.enabled = false;
            candidate.action.disabled_reason = Some(reason.into());
        }
    }
    result
}
fn search_aliases(command: &str) -> &'static str {
    match command {
        "feature.extrude" => "pad boss extrusion add material",
        "feature.cut" => "pocket subtract remove material",
        "feature.fillet" => "round edge radius",
        "feature.chamfer" => "bevel edge",
        "sketch.fillet" => "round sketch corner",
        "sketch.chamfer" => "bevel sketch corner",
        "sketch.smart-dimension" => "distance diameter radius angle",
        "viewport.fit" => "zoom extents fit all",
        "settings.model" => "model units inches millimeters precision",
        "appearances" => "color finish texture paint",
        _ => "",
    }
}

fn suggested_commands(w: &Workspace) -> &[&str] {
    if w.sketch.is_some() || crate::sketch3d_ui::active(w) {
        &["sketch.line", "sketch.circle", "sketch.smart-dimension"]
    } else if crate::drawing_ui::is_workspace(w) {
        &["drawings", "viewport.fit", "file.export-drawing-dxf"]
    } else if w.view.active_part.is_some() {
        &[
            "sketch.new",
            "feature.extrude",
            "feature.cut",
            "feature.fillet",
            "viewport.fit",
        ]
    } else {
        &[
            "assembly.insert-existing",
            "assembly.mate-new",
            "part.new",
            "viewport.fit",
        ]
    }
}

#[derive(Copy, Clone, PartialEq, Eq)]
enum Mode {
    Suggestions,
    Browse,
    Search,
}
fn mode(w: &Workspace) -> Mode {
    if w.fields
        .get(QUERY)
        .is_some_and(|query| !query.trim().is_empty())
    {
        Mode::Search
    } else if w.fields.contains_key(BROWSE) {
        Mode::Browse
    } else {
        Mode::Suggestions
    }
}
fn cursor(w: &Workspace, count: usize) -> usize {
    w.fields
        .get(INDEX)
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(0)
        .min(count.saturating_sub(1))
}
fn page(index: usize, count: usize) -> Range<usize> {
    let start = index / PAGE_SIZE * PAGE_SIZE;
    start..(start + PAGE_SIZE).min(count)
}
fn match_summary(w: &Workspace, range: &Range<usize>, count: usize) -> String {
    let mode = mode(w);
    if mode == Mode::Suggestions {
        return if count == 0 {
            "Type a command name or browse the catalog."
        } else {
            "Suggested commands"
        }
        .into();
    }
    let noun = if mode == Mode::Browse {
        "commands"
    } else {
        "matches"
    };
    match count {
        0 if w.fields.get(ONLY).is_some_and(|value| value == "true") => {
            "No matches. Clear the availability filter or try another name.".into()
        }
        0 => "No matching commands. Try a tool name such as Extrude or Fillet.".into(),
        1 => "1 command".into(),
        count if count > PAGE_SIZE => {
            format!("{}–{} of {count} {noun}", range.start + 1, range.end)
        }
        count => format!("{count} {noun}"),
    }
}
fn suggestion_rank(candidate: &Candidate, suggestions: &[&str]) -> Option<(usize, usize)> {
    if !candidate.action.enabled {
        return None;
    }
    let menu = &candidate.source.data["menu"];
    if candidate.source.role == "selection-action"
        && menu["primary"] == true
        && candidate.action.command != "sketch.delete-selection"
    {
        return Some((0, menu["order"].as_u64().unwrap_or(50) as usize));
    }
    suggestions
        .iter()
        .position(|command| *command == candidate.action.command)
        .map(|rank| (1, rank))
}
fn filtered(w: &Workspace) -> Vec<Candidate> {
    if mode(w) == Mode::Suggestions {
        let suggestions = suggested_commands(w);
        let mut ranked = candidates(w)
            .into_iter()
            .filter_map(|candidate| {
                suggestion_rank(&candidate, suggestions).map(|rank| (rank, candidate))
            })
            .collect::<Vec<_>>();
        ranked.sort_by(|(a_rank, a), (b_rank, b)| {
            a_rank
                .cmp(b_rank)
                .then_with(|| b.context.cmp(&a.context))
                .then_with(|| a.label.cmp(&b.label))
        });
        let mut seen = BTreeSet::new();
        return ranked
            .into_iter()
            .map(|(_, candidate)| candidate)
            .filter(|candidate| seen.insert(candidate.action.command.clone()))
            .take(SUGGESTION_LIMIT)
            .collect();
    }
    let query = w
        .fields
        .get(QUERY)
        .map(String::as_str)
        .unwrap_or("")
        .trim()
        .to_lowercase();
    let terms = query.split_whitespace().collect::<Vec<_>>();
    let only = w.fields.get(ONLY).is_some_and(|value| value == "true");
    let mut values = candidates(w)
        .into_iter()
        .filter(|candidate| {
            if only && !candidate.action.enabled {
                return false;
            }
            if terms.is_empty() {
                return true;
            }
            let haystack = format!(
                "{} {} {} {}",
                candidate.label,
                candidate.category,
                candidate.action.command,
                search_aliases(&candidate.action.command)
            )
            .to_lowercase();
            terms.iter().all(|term| haystack.contains(term))
        })
        .collect::<Vec<_>>();
    values.sort_by_cached_key(|candidate| {
        let label = candidate.label.to_lowercase();
        let command = candidate.action.command.as_str();
        (
            !query.is_empty()
                && query != label
                && command.rsplit('.').next() != Some(query.as_str()),
            !candidate.action.enabled,
            !candidate.context,
            !label.starts_with(&query),
            candidate.category.clone(),
            label,
        )
    });
    values
}
fn stamp(w: &Workspace) -> Value {
    json!({"document":w.session.visible().id,"generation":w.session.generation,"revision":w.session.visible().revision,"draft":w.session.draft.as_ref().map(|draft|(&draft.id,draft.edit))})
}
fn result_element(w: &Workspace, candidate: &Candidate) -> SemanticElement {
    SemanticElement {
        id: candidate.id.clone(),
        role: "command-palette-result".into(),
        label: candidate.label.clone(),
        value: Some(candidate.category.clone()),
        state: if candidate.action.enabled {
            "available"
        } else {
            "disabled"
        }
        .into(),
        commands: vec!["command-palette.execute".into()],
        relationships: vec!["workspace.commands".into()],
        data: json!({"contract":"tcad.command-palette","version":1,"stamp":stamp(w),"result":candidate.id,"command":candidate.action.command,"category":candidate.category,"contextual":candidate.context,"source":candidate.source,"action":candidate.action,"actions":[{"command":"command-palette.execute","label":candidate.label,"enabled":candidate.action.enabled,"disabled_reason":candidate.action.disabled_reason}]}),
    }
}
fn field(w: &Workspace, pointer: &str, label: &str, value: Value) -> SemanticElement {
    SemanticElement {
        id: format!("command-palette:{pointer}"),
        role: "command-palette-field".into(),
        label: label.into(),
        value: Some(
            value
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| value.to_string()),
        ),
        state: "ready".into(),
        commands: vec![],
        relationships: vec!["workspace.commands".into()],
        data: json!({"contract":"tcad.command-palette","version":1,"setter":"command-palette","field":pointer,"document":w.session.visible().id,"json":value,"editable":true,"focus_request":if pointer=="query" {json!("focus-request:command-palette:query")}else{Value::Null}}),
    }
}
pub fn semantics(w: &Workspace) -> Vec<SemanticElement> {
    let open = w.dialog.as_deref() == Some("commands");
    let mut out = vec![
        SemanticElement {
            id: "workspace.commands".into(),
            role: "command-palette".into(),
            label: "Find a command".into(),
            value: None,
            state: if open { "open" } else { "closed" }.into(),
            commands: vec!["commands".into()],
            relationships: vec![],
            data: json!({"renderer":"command-palette","contract":"tcad.command-palette","version":1,"maximum_results":PAGE_SIZE,"page_size":PAGE_SIZE,"suggestion_limit":SUGGESTION_LIMIT}),
        },
        field(
            w,
            "query",
            "Search commands",
            json!(w.fields.get(QUERY).map(String::as_str).unwrap_or("")),
        ),
        field(
            w,
            "available",
            "Available commands only",
            json!(w.fields.get(ONLY).is_some_and(|value| value == "true")),
        ),
    ];
    if open {
        out.push(SemanticElement {
            id: "command-palette:close".into(),
            role: "command-palette-action".into(),
            label: "Close".into(),
            value: None,
            state: "ready".into(),
            commands: vec!["command-palette.close".into()],
            relationships: vec!["workspace.commands".into()],
            data: json!({"accessibility_label":"Close command search"}),
        });
        let values = filtered(w);
        let count = values.len();
        let range = page(cursor(w, count), count);
        out.push(SemanticElement {
            id: "command-palette:status".into(),
            role: "command-palette-status".into(),
            label: match_summary(w, &range, count),
            value: None,
            state: "ready".into(),
            commands: vec![],
            relationships: vec!["workspace.commands".into()],
            data: json!({"matches":count,"shown":range.len(),"offset":range.start,"suggestions":mode(w) == Mode::Suggestions}),
        });
        out.extend(navigation(w, &range, count));
        out.extend(
            values[range]
                .iter()
                .map(|candidate| result_element(w, candidate)),
        );
    }
    out
}
const ALL: &str = "command-palette.browse";
const SUGGESTED: &str = "command-palette.suggestions";
const PREVIOUS: &str = "command-palette.previous-page";
const NEXT: &str = "command-palette.next-page";

fn navigation(w: &Workspace, range: &Range<usize>, count: usize) -> Vec<SemanticElement> {
    let mut actions = Vec::new();
    match mode(w) {
        Mode::Suggestions => actions.push((ALL, "Browse all commands", true)),
        Mode::Browse => actions.push((SUGGESTED, "Show suggestions", true)),
        Mode::Search => {}
    }
    if count > PAGE_SIZE {
        actions.push((PREVIOUS, "Previous", range.start > 0));
        actions.push((NEXT, "Next", range.end < count));
    }
    actions
        .into_iter()
        .map(|(command, label, enabled)| SemanticElement {
            id: command.into(),
            role: "command-palette-action".into(),
            label: label.into(),
            value: None,
            state: if enabled { "ready" } else { "disabled" }.into(),
            commands: vec![command.into()],
            relationships: vec!["workspace.commands".into()],
            data: json!({
                "document": w.session.visible().id,
                "actions": [{"command": command, "label": label, "enabled": enabled}]
            }),
        })
        .collect()
}
fn navigate(w: &mut Workspace, command: &str, count: usize) {
    let index = cursor(w, count);
    let start = page(index, count).start;
    let next = match command {
        ALL if mode(w) == Mode::Suggestions => {
            w.fields.insert(BROWSE.into(), "true".into());
            0
        }
        SUGGESTED if mode(w) == Mode::Browse => {
            w.fields.remove(BROWSE);
            0
        }
        PREVIOUS if start > 0 => start - PAGE_SIZE,
        NEXT if start + PAGE_SIZE < count => start + PAGE_SIZE,
        _ => return,
    };
    w.fields.insert(INDEX.into(), next.to_string());
    w.fields
        .insert("focus-request:command-palette:query".into(), "true".into());
}
pub fn current_value(w: &Workspace, element: &SemanticElement) -> Option<Value> {
    match element.data["field"].as_str()? {
        "query" => Some(json!(w.fields.get(QUERY).map(String::as_str).unwrap_or(""))),
        "available" => Some(json!(
            w.fields.get(ONLY).is_some_and(|value| value == "true")
        )),
        _ => None,
    }
}
pub fn set_value(w: &mut Workspace, element: &SemanticElement, value: Value) -> Result<()> {
    if element.data["setter"] != "command-palette"
        || element.data["document"] != json!(w.session.visible().id)
    {
        return Err(CadError::Invalid(
            "The command search field is stale".into(),
        ));
    }
    match element.data["field"].as_str() {
        Some("query") => {
            let query = value
                .as_str()
                .filter(|value| value.len() <= 1024 && !value.chars().any(char::is_control))
                .ok_or_else(|| {
                    CadError::Invalid("Use bounded printable command-search text".into())
                })?;
            w.fields.insert(QUERY.into(), query.into());
        }
        Some("available") => {
            let enabled = value.as_bool().ok_or_else(|| {
                CadError::Invalid("The command availability filter requires a boolean".into())
            })?;
            w.fields.insert(ONLY.into(), enabled.to_string());
        }
        _ => return Err(CadError::Invalid("Unknown command-search field".into())),
    }
    w.fields.remove(INDEX);
    Ok(())
}
fn execute(w: &mut Workspace, element: &SemanticElement) -> Result<()> {
    if element.data["stamp"] != stamp(w) {
        return Err(CadError::Invalid(
            "The command context changed; choose a current result".into(),
        ));
    }
    let candidate = candidates(w)
        .into_iter()
        .find(|candidate| element.data["result"] == candidate.id)
        .ok_or_else(|| CadError::Invalid("This context action is no longer available".into()))?;
    if !candidate.action.enabled
        || element.data["action"]["arguments"] != candidate.action.arguments
        || element.data["command"] != candidate.action.command
    {
        return Err(CadError::Invalid(
            "The selected action changed or is disabled; use its current search result".into(),
        ));
    }
    let mut source = candidate.source;
    source.data = candidate.action.arguments;
    restore(w);
    crate::ui::semantic_action(w, &source, &candidate.action.command);
    Ok(())
}
pub fn action(w: &mut Workspace, command: &str, data: Option<&Value>) -> bool {
    if matches!(command, ALL | SUGGESTED | PREVIOUS | NEXT) {
        if data.is_none_or(|data| {
            data.get("document")
                .is_none_or(|id| id.as_str() == Some(w.session.visible().id.as_str()))
        }) {
            let count = if matches!(command, PREVIOUS | NEXT) {
                filtered(w).len()
            } else {
                0
            };
            navigate(w, command, count);
        }
        return true;
    }
    if command == "command-palette.close" {
        escape(w);
        return true;
    }
    if command != "command-palette.execute" {
        return false;
    }
    let result = data
        .ok_or_else(|| CadError::Invalid("Choose a command search result".into()))
        .and_then(|data| {
            let element = SemanticElement {
                id: String::new(),
                role: "command-palette-result".into(),
                label: String::new(),
                value: None,
                state: "ready".into(),
                commands: vec![],
                relationships: vec![],
                data: data.clone(),
            };
            execute(w, &element)
        });
    if let Err(error) = result {
        w.status = error.to_string();
    }
    true
}
pub fn show(ui: &mut egui::Ui, w: &mut Workspace) {
    let open = w.dialog.as_deref() == Some("commands");
    let close = ui
        .horizontal(|ui| {
            controls::label(
                ui,
                "command-palette:title",
                "inspector-title",
                &[],
                "Find a command",
                "ready",
            );
            if !open {
                return false;
            }
            controls::button(
                ui,
                "command-palette:close",
                "command-palette-action",
                &["semantic-control"],
                "Close",
                true,
                false,
            )
            .on_hover_text("Close search (Esc)")
            .clicked()
        })
        .inner;
    if close {
        escape(w);
        return;
    }
    let query = field(
        w,
        "query",
        "Search commands",
        json!(w.fields.get(QUERY).map(String::as_str).unwrap_or("")),
    );
    // A theme may dock search alongside other editors. Only use its navigation
    // shortcuts while search is open or its text field has keyboard focus.
    let focused = ui
        .memory(|memory| memory.focused())
        .and_then(|id| ui.ctx().read_response(id));
    let query_row = ui
        .scope(|ui| crate::ui::semantic_field(ui, w, &query))
        .response;
    let keyboard = (open && focused.is_none())
        || focused.is_some_and(|field| {
            field.layer_id == query_row.layer_id && query_row.rect.contains(field.rect.center())
        });
    if mode(w) != Mode::Suggestions {
        let available = field(
            w,
            "available",
            "Available commands only",
            json!(w.fields.get(ONLY).is_some_and(|value| value == "true")),
        );
        crate::ui::semantic_field(ui, w, &available);
    }
    let values = filtered(w);
    let count = values.len();
    let mut index = cursor(w, count);
    let (up, down, page_up, page_down, enter) = ui.input_mut(|input| {
        let mut pressed = |key| keyboard && input.consume_key(egui::Modifiers::NONE, key);
        (
            pressed(egui::Key::ArrowUp),
            pressed(egui::Key::ArrowDown),
            pressed(egui::Key::PageUp),
            pressed(egui::Key::PageDown),
            pressed(egui::Key::Enter),
        )
    });
    if up || page_up {
        index = index.saturating_sub(if page_up { PAGE_SIZE } else { 1 });
    }
    if down || page_down {
        index = (index + if page_down { PAGE_SIZE } else { 1 }).min(count.saturating_sub(1));
    }
    w.fields.insert(INDEX.into(), index.to_string());
    let range = page(index, count);
    let navigation_clicked = ui
        .horizontal_wrapped(|ui| {
            controls::label(
                ui,
                "command-palette:status",
                "command-palette-status",
                &[],
                &match_summary(w, &range, count),
                "ready",
            );
            for item in navigation(w, &range, count) {
                if controls::button(
                    ui,
                    &item.id,
                    &item.role,
                    &["semantic-control"],
                    &item.label,
                    item.state != "disabled",
                    false,
                )
                .clicked()
                {
                    navigate(w, &item.id, count);
                    return true;
                }
            }
            false
        })
        .inner;
    if navigation_clicked {
        return;
    }
    let mut chosen = enter
        .then(|| {
            values
                .get(index)
                .filter(|candidate| candidate.action.enabled)
                .map(|candidate| result_element(w, candidate))
        })
        .flatten();
    egui::ScrollArea::vertical()
        .id_salt(("command-palette-results", range.start))
        .show(ui, |ui| {
            for row in range {
                let candidate = &values[row];
                let response = controls::button(
                    ui,
                    &candidate.id,
                    "command-palette-result",
                    &["semantic-control"],
                    &format!("{} · {}", candidate.label, candidate.category),
                    candidate.action.enabled,
                    row == index,
                )
                .on_disabled_hover_text(
                    candidate
                        .action
                        .disabled_reason
                        .as_deref()
                        .unwrap_or("This action is unavailable in the current context"),
                );
                if row == index
                    && !candidate.action.enabled
                    && let Some(reason) = candidate.action.disabled_reason.as_deref()
                {
                    controls::label(
                        ui,
                        "command-palette:disabled-reason",
                        "command-palette-status",
                        &[],
                        reason,
                        "disabled",
                    );
                }
                if row == index && (up || down || page_up || page_down) {
                    response.scroll_to_me(Some(egui::Align::Center));
                }
                if response.clicked() {
                    chosen = Some(result_element(w, candidate));
                }
            }
        });
    if let Some(element) = chosen
        && let Err(error) = execute(w, &element)
    {
        w.status = error.to_string();
    }
}
