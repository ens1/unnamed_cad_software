//! Installed repository finishes and portable, pinned texture interchange.
use crate::{Workspace, presentation_controls as controls};
use bevy_egui::egui;
use cad_core::{
    CadError, Result, appearance::Finish, commands::Command, presentation::SemanticElement,
};
use serde_json::{Value, json};

const SELECTED: &str = "appearance-catalog-selected";
fn key(w: &Workspace) -> Option<String> {
    w.fields
        .get(SELECTED)
        .cloned()
        .or_else(|| crate::appearances_ui::selected_finish(w).map(|id| format!("project:{id}")))
}
fn selected(w: &Workspace) -> Option<Finish> {
    let key = key(w)?;
    if let Some(id) = key.strip_prefix("project:") {
        w.session.visible().finishes.get(id).cloned()
    } else {
        let id = key.strip_prefix("catalog:")?;
        cad_core::finish_library::catalog()
            .iter()
            .find(|entry| entry.finish.id == id)
            .map(|entry| entry.finish.clone())
    }
}
fn preference_key(finish: &Finish) -> String {
    format!(
        "finish:{}",
        finish
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.catalog_id.as_deref())
            .unwrap_or(&finish.id)
    )
}
fn available(w: &Workspace) -> bool {
    w.sketch.is_none()
        && (w.tool_checkpoint.is_none()
            || w.fields.contains_key("appearance-tool")
            || w.fields.contains_key("material-tool"))
}
fn recent(w: &mut Workspace, finish: &Finish) {
    let key = preference_key(finish);
    w.view.library_recent.retain(|previous| previous != &key);
    w.view.library_recent.insert(0, key);
    w.view.library_recent.truncate(64);
    crate::settings::save_live_application(w);
}
fn replace(w: &mut Workspace, document: cad_core::Document) -> Result<()> {
    document.validate()?;
    let expected = document.fingerprint();
    w.dispatch(Command::ReplaceDocument { document });
    if w.session.visible().fingerprint() != expected {
        return Err(CadError::Invalid(w.status.clone()));
    }
    w.rendered_key.clear();
    Ok(())
}
fn import(w: &mut Workspace) -> Result<()> {
    use std::io::Read;
    let Some(path) = rfd::FileDialog::new()
        .set_title("Import visual finishes and pinned texture maps")
        .add_filter("TCAD finish library", &["tcadfinishes", "json"])
        .pick_file()
    else {
        return Ok(());
    };
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(256 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > 256 * 1024 * 1024 {
        return Err(CadError::Invalid(
            "Finish packages are limited to 256 MiB".into(),
        ));
    }
    let package = cad_core::finish_library::decode(&bytes)?;
    let mut document = w.session.visible().clone();
    let ids = cad_core::finish_library::install(&mut document, &w.root(), &package)?;
    replace(w, document)?;
    if let Some(id) = ids.first() {
        crate::appearances_ui::select_named_finish(w, id)?;
        w.fields.insert(SELECTED.into(), format!("project:{id}"));
    }
    Ok(())
}
fn export(w: &mut Workspace, all: bool) -> Result<()> {
    let mut document = w.session.visible().clone();
    let ids = if all {
        document.finishes.keys().cloned().collect::<Vec<_>>()
    } else {
        let finish =
            selected(w).ok_or_else(|| CadError::Invalid("Choose a finish to export".into()))?;
        document.finishes.insert(finish.id.clone(), finish.clone());
        vec![finish.id]
    };
    let package = if !all && key(w).is_some_and(|key| key.starts_with("catalog:")) {
        cad_core::finish_library::package_for_catalog(&ids[0])?
    } else {
        cad_core::finish_library::export(&document, &w.root(), &ids)?
    };
    let bytes = cad_core::finish_library::encode(&package)?;
    let Some(path) = rfd::FileDialog::new()
        .set_title("Export visual finishes with pinned texture maps")
        .set_file_name("Finishes.tcadfinishes")
        .add_filter("TCAD finish library", &["tcadfinishes"])
        .save_file()
    else {
        return Ok(());
    };
    cad_core::persistence::atomic_write(&path, &bytes)?;
    w.status = format!(
        "Exported finishes and pinned texture maps to {}",
        path.display()
    );
    Ok(())
}
pub fn action(w: &mut Workspace, command: &str, arguments: Option<&Value>) -> bool {
    if !command.starts_with("finish-library.") {
        return false;
    }
    if command == "finish-library.libraries" {
        crate::ui::action(w, "content-repositories.show-textures");
        return true;
    }
    if command == "finish-library.select" {
        if let Some(key) = arguments
            .and_then(|arguments| arguments.get("key"))
            .and_then(Value::as_str)
        {
            if crate::finish_library_ui::key(w).as_deref() == Some(key) {
                return true;
            }
            if let Err(error) = crate::appearances_ui::selection_ready(w) {
                w.status = error.to_string();
                return true;
            }
            if let Some(id) = key.strip_prefix("project:")
                && let Err(error) = crate::appearances_ui::select_named_finish(w, id)
            {
                w.status = error.to_string();
                return true;
            }
            w.fields.insert(SELECTED.into(), key.into());
        }
        return true;
    }
    if command == "finish-library.favorite" {
        if let Some(finish) = selected(w) {
            let key = preference_key(&finish);
            if w.view.library_favorites.contains(&key) {
                w.view.library_favorites.retain(|previous| previous != &key);
            } else {
                w.view.library_favorites.push(key);
            }
            crate::settings::save_live_application(w);
        }
        return true;
    }
    if command == "finish-library.toggle" {
        let open = w
            .fields
            .get("appearance-catalog-open")
            .map(|value| value == "true")
            .unwrap_or(true);
        w.fields
            .insert("appearance-catalog-open".into(), (!open).to_string());
        return true;
    }
    if !available(w) {
        w.status = "Apply or cancel the current operation before editing visual finishes".into();
        return true;
    }
    if matches!(
        command,
        "finish-library.assign" | "finish-library.add" | "finish-library.import"
    ) && let Err(error) = crate::appearances_ui::selection_ready(w)
    {
        w.status = error.to_string();
        return true;
    }
    if !w.fields.contains_key("appearance-tool") {
        crate::appearances_ui::begin(w, false);
        if !w.fields.contains_key("appearance-tool") {
            return true;
        }
    }
    let result = (|| -> Result<()> {
        match command {
            "finish-library.assign" | "finish-library.add" => {
                let source = selected(w)
                    .ok_or_else(|| CadError::Invalid("Choose a visual finish".into()))?;
                let finish = if key(w).is_some_and(|key| key.starts_with("catalog:")) {
                    let mut document = w.session.visible().clone();
                    let id = cad_core::finish_library::install_catalog(
                        &mut document,
                        &w.root(),
                        &source.id,
                    )?;
                    let finish = document.finishes.get(&id).cloned().ok_or_else(|| {
                        CadError::Invalid(
                            "Repository finish installation did not produce a finish".into(),
                        )
                    })?;
                    replace(w, document)?;
                    finish
                } else {
                    source
                };
                crate::appearances_ui::add_named_finish(
                    w,
                    finish.clone(),
                    command == "finish-library.assign",
                )?;
                w.fields
                    .insert(SELECTED.into(), format!("project:{}", finish.id));
                recent(w, &finish);
            }
            "finish-library.import" => import(w)?,
            "finish-library.export" => export(w, false)?,
            "finish-library.export-project" => export(w, true)?,
            _ => {
                return Err(CadError::Invalid(format!(
                    "Unknown finish library command: {command}"
                )));
            }
        }
        Ok(())
    })();
    if let Err(error) = result {
        w.status = error.to_string();
    }
    true
}
fn field(w: &Workspace, name: &str, label: &str, choices: Value) -> SemanticElement {
    let value = w
        .fields
        .get(&format!("appearance-catalog-{name}"))
        .cloned()
        .unwrap_or_default();
    SemanticElement {
        id: format!("appearance:catalog:{name}"),
        role: "finish-library-field".into(),
        label: label.into(),
        value: Some(value.clone()),
        state: "ready".into(),
        commands: vec![],
        relationships: vec!["workspace.finish-library".into()],
        data: json!({"setter":"finish-library","field":name,"json":value,"choices":choices,"editable":true,"transient":true,"classes":["modeling-field"],"accessibility_label":label}),
    }
}
fn fields(w: &Workspace) -> Vec<SemanticElement> {
    let mut categories = cad_core::finish_library::catalog()
        .iter()
        .map(|entry| entry.category.clone())
        .collect::<std::collections::BTreeSet<_>>();
    categories.extend(w.session.visible().finishes.values().filter_map(|finish| {
        finish
            .metadata
            .as_ref()
            .map(|metadata| metadata.category.clone())
    }));
    vec![
        field(w, "search", "Search finishes and tags", json!([])),
        field(
            w,
            "category",
            "Finish family",
            json!(
                std::iter::once(json!({"value":"","label":"All finish families"}))
                    .chain(
                        categories
                            .into_iter()
                            .map(|category| json!({"value":category,"label":category}))
                    )
                    .collect::<Vec<_>>()
            ),
        ),
        field(
            w,
            "collection",
            "Collection",
            json!([{"value":"","label":"Project and repositories"},{"value":"project","label":"Project only"},{"value":"catalog","label":"Repositories only"},{"value":"favorites","label":"Favorites"},{"value":"recent","label":"Recently assigned"}]),
        ),
    ]
}
pub fn current_value(w: &Workspace, element: &SemanticElement) -> Option<Value> {
    let field = element.data.get("field")?.as_str()?;
    Some(json!(
        w.fields
            .get(&format!("appearance-catalog-{field}"))
            .cloned()
            .unwrap_or_default()
    ))
}
pub fn set_value(w: &mut Workspace, element: &SemanticElement, value: Value) -> Result<()> {
    let field = element
        .data
        .get("field")
        .and_then(Value::as_str)
        .filter(|field| matches!(*field, "search" | "category" | "collection"))
        .ok_or_else(|| CadError::Invalid("Unknown finish library field".into()))?;
    let text = value
        .as_str()
        .ok_or_else(|| CadError::Invalid("Choose a finish library filter or enter text".into()))?;
    w.fields
        .insert(format!("appearance-catalog-{field}"), text.into());
    Ok(())
}
fn matches(w: &Workspace, finish: &Finish) -> bool {
    let query = w
        .fields
        .get("appearance-catalog-search")
        .map(|value| value.to_lowercase())
        .unwrap_or_default();
    let category = w
        .fields
        .get("appearance-catalog-category")
        .map(String::as_str)
        .unwrap_or_default();
    let collection = w
        .fields
        .get("appearance-catalog-collection")
        .map(String::as_str)
        .unwrap_or_default();
    let searchable = format!(
        "{} {}",
        finish.name,
        finish
            .metadata
            .as_ref()
            .map(|metadata| format!(
                "{} {} {}",
                metadata.category,
                metadata.description,
                metadata.tags.join(" ")
            ))
            .unwrap_or_default()
    )
    .to_lowercase();
    query
        .split_whitespace()
        .all(|term| searchable.contains(term))
        && (category.is_empty()
            || finish
                .metadata
                .as_ref()
                .is_some_and(|metadata| metadata.category == category))
        && (collection != "favorites" || w.view.library_favorites.contains(&preference_key(finish)))
        && (collection != "recent" || w.view.library_recent.contains(&preference_key(finish)))
}
fn entry(w: &Workspace, finish: &Finish, catalog: bool) -> SemanticElement {
    let entry_key = format!(
        "{}:{}",
        if catalog { "catalog" } else { "project" },
        finish.id
    );
    SemanticElement {
        id: format!("appearance:catalog:entry:{entry_key}"),
        role: "finish-library-entry".into(),
        label: finish.name.clone(),
        value: finish
            .metadata
            .as_ref()
            .map(|metadata| metadata.category.clone()),
        state: if key(w).as_deref() == Some(&entry_key) {
            "selected"
        } else {
            "ready"
        }
        .into(),
        commands: vec!["finish-library.select".into()],
        relationships: vec!["workspace.finish-library".into()],
        data: json!({"key":entry_key,"finish":finish,"catalog":catalog,"favorite":w.view.library_favorites.contains(&preference_key(finish)),"engineering_material_independent":true,"accessibility_label":finish.name,"actions":[{"command":"finish-library.select","enabled":key(w).as_deref()==Some(&entry_key)||crate::appearances_ui::selection_ready(w).is_ok(),"disabled_reason":"Correct retained finish/lighting text before changing selection"}]}),
    }
}
fn library_notice() -> Option<SemanticElement> {
    (cad_core::finish_library::catalog().is_empty()
        && cad_core::finish_library::catalog_diagnostics().is_empty())
        .then(|| SemanticElement {
            id: "appearance:catalog:library-notice".into(),
            role: "finish-library-description".into(),
            label: "No texture and finish pack is available. Colors, lighting and saved finishes still work. Download a pack or import your own maps and finishes.".into(),
            value: None,
            state: "empty".into(),
            commands: vec!["content-repositories.show-textures".into()],
            relationships: vec!["workspace.finish-library".into()],
            data: json!({"pack":"textures","project_finishes_preserved":true}),
        })
}
fn actions(w: &Workspace) -> Vec<SemanticElement> {
    let has = selected(w).is_some();
    let mut items = vec![
        (
            "finish-library.assign",
            "Assign selected finish",
            has && available(w),
            "Choose a visual finish and an appearance target",
        ),
        (
            "finish-library.add",
            "Add to project",
            has && available(w) && key(w).is_some_and(|key| key.starts_with("catalog:")),
            "Choose a catalog finish",
        ),
        (
            "finish-library.favorite",
            "Favorite",
            has,
            "Choose a finish",
        ),
    ];
    if cad_core::finish_library::catalog().is_empty() {
        items.push((
            "content-repositories.download-textures",
            "Download texture and finish pack",
            true,
            "",
        ));
    }
    items.extend([
        (
            "content-repositories.show-textures",
            "Manage libraries…",
            true,
            "",
        ),
        (
            "finish-library.import",
            "Import finishes…",
            available(w),
            "Finish the active operation first",
        ),
        (
            "finish-library.export",
            "Export selected…",
            has,
            "Choose a finish",
        ),
        (
            "finish-library.export-project",
            "Export project finishes…",
            !w.session.visible().finishes.is_empty(),
            "The project finish library is empty",
        ),
    ]);
    items.into_iter().map(|(command, label, mut enabled, mut reason)| {
        if matches!(command, "finish-library.assign" | "finish-library.add" | "finish-library.import")
            && crate::appearances_ui::selection_ready(w).is_err()
        {
            enabled = false;
            reason = "Correct retained finish/lighting text before changing selection or assignment";
        }
        SemanticElement {
            id: if command == "content-repositories.show-textures" {
                "command:finish-library.libraries".into()
            } else if command.starts_with("content-repositories.") {
                format!("command:finish-library:{command}")
            } else {
                format!("command:{command}")
            },
            role: "finish-library-action".into(),
            label: label.into(),
            value: None,
            state: if enabled { "ready" } else { "disabled" }.into(),
            commands: vec![command.into()],
            relationships: vec!["workspace.finish-library".into()],
            data: json!({"enabled":enabled,"disabled_reason":if enabled {""} else {reason},"accessibility_label":label}),
        }
    }).collect()
}

pub fn semantics(w: &Workspace) -> Vec<SemanticElement> {
    let mut elements = vec![
        SemanticElement {
            id: "workspace.finish-library".into(),
            role: "finish-library".into(),
            label: "Finish catalog".into(),
            value: key(w),
            state: "ready".into(),
            commands: vec!["appearances".into()],
            relationships: vec!["workspace.appearance-panel".into()],
            data: json!({"version":1,"portable_resources":"pinned-texture-bytes","independent_engineering_material":true}),
        },
        SemanticElement {
            id: "appearance:catalog:heading".into(),
            role: "finish-library-section".into(),
            label: "Finish catalog".into(),
            value: None,
            state: if w
                .fields
                .get("appearance-catalog-open")
                .map(|value| value == "true")
                .unwrap_or(true)
            {
                "expanded"
            } else {
                "collapsed"
            }
            .into(),
            commands: vec!["finish-library.toggle".into()],
            relationships: vec!["workspace.finish-library".into()],
            data: json!({"accessibility_label":"Finish catalog"}),
        },
    ];
    elements.extend(fields(w));
    elements.extend(library_notice());
    for (index, error) in cad_core::finish_library::catalog_diagnostics()
        .into_iter()
        .enumerate()
    {
        elements.push(SemanticElement { id: format!("appearance:catalog:diagnostic:{index}"), role: "diagnostic".into(), label: error, value: None, state: "error".into(), commands: vec!["content-repositories".into()], relationships: vec!["workspace.finish-library".into()], data: json!({"actions":[{"command":"content-repositories","label":"Content Libraries"}]}) });
    }
    elements.extend(actions(w));
    for finish in w.session.visible().finishes.values() {
        elements.push(entry(w, finish, false));
    }
    for catalog in cad_core::finish_library::catalog() {
        elements.push(entry(w, &catalog.finish, true));
    }
    if let Some(finish) = selected(w) {
        elements.push(SemanticElement{id:"appearance:catalog:selected-details".into(),role:"finish-library-details".into(),label:finish.name.clone(),value:finish.metadata.as_ref().map(|metadata|metadata.description.clone()),state:"readonly".into(),commands:vec![],relationships:vec!["workspace.finish-library".into()],data:json!({"finish":finish,"source":finish.metadata.as_ref().map(|metadata|&metadata.source),"authored_visual_preset":true})});
    }
    elements
}
fn render_action(ui: &mut egui::Ui, w: &mut Workspace, element: &SemanticElement) {
    if controls::button(
        ui,
        &element.id,
        &element.role,
        &["semantic-control"],
        &element.label,
        element.data["enabled"].as_bool().unwrap_or(false),
        false,
    )
    .on_hover_text(element.data["disabled_reason"].as_str().unwrap_or_default())
    .clicked()
    {
        if element.commands[0].starts_with("content-repositories.") {
            crate::ui::semantic_action(w, element, &element.commands[0]);
        } else {
            action(w, &element.commands[0], Some(&element.data));
        }
    }
}
pub fn show(ui: &mut egui::Ui, w: &mut Workspace) {
    let open = w
        .fields
        .get("appearance-catalog-open")
        .map(|value| value == "true")
        .unwrap_or(true);
    if controls::button(
        ui,
        "appearance:catalog:heading",
        "finish-library-section",
        &["semantic-control"],
        &format!("{} Finish catalog", if open { "▾" } else { "▸" }),
        true,
        open,
    )
    .clicked()
    {
        action(w, "finish-library.toggle", None);
    }
    if !w
        .fields
        .get("appearance-catalog-open")
        .map(|value| value == "true")
        .unwrap_or(true)
    {
        return;
    }
    if let Some(notice) = library_notice() {
        controls::label(
            ui,
            &notice.id,
            &notice.role,
            &[],
            &notice.label,
            &notice.state,
        );
    }
    for error in cad_core::finish_library::catalog_diagnostics() {
        controls::label(
            ui,
            "appearance:catalog:diagnostic",
            "diagnostic",
            &[],
            &error,
            "error",
        );
    }
    for element in fields(w) {
        crate::ui::semantic_field(ui, w, &element);
    }
    let collection = w
        .fields
        .get("appearance-catalog-collection")
        .cloned()
        .unwrap_or_default();
    let props = controls::properties(
        "appearance:catalog:list",
        "finish-library-list",
        &[],
        "ready",
    );
    let mut count = 0;
    egui::ScrollArea::vertical()
        .id_salt("appearance-finish-catalog")
        .max_height(controls::number(&props, "max-height", 180.))
        .show(ui, |ui| {
            for (is_catalog, finishes) in [
                (
                    false,
                    w.session
                        .visible()
                        .finishes
                        .values()
                        .cloned()
                        .collect::<Vec<_>>(),
                ),
                (
                    true,
                    cad_core::finish_library::catalog()
                        .iter()
                        .map(|entry| entry.finish.clone())
                        .collect::<Vec<_>>(),
                ),
            ] {
                if collection == "project" && is_catalog || collection == "catalog" && !is_catalog {
                    continue;
                }
                let finishes = finishes
                    .into_iter()
                    .filter(|finish| matches(w, finish))
                    .collect::<Vec<_>>();
                if finishes.is_empty() {
                    continue;
                }
                controls::label(
                    ui,
                    if is_catalog {
                        "appearance:catalog:curated"
                    } else {
                        "appearance:catalog:project"
                    },
                    "finish-library-heading",
                    &[],
                    if is_catalog {
                        "Curated visual presets"
                    } else {
                        "In this project"
                    },
                    "ready",
                );
                for finish in finishes {
                    count += 1;
                    let element = entry(w, &finish, is_catalog);
                    let favorite = element.data["favorite"].as_bool().unwrap_or(false);
                    if controls::button(
                        ui,
                        &element.id,
                        &element.role,
                        &["semantic-control"],
                        &format!("{}{}", if favorite { "★ " } else { "" }, element.label),
                        element
                            .action_descriptors()
                            .iter()
                            .any(|action| action.enabled),
                        element.state == "selected",
                    )
                    .on_hover_text(
                        finish
                            .metadata
                            .as_ref()
                            .map(|metadata| metadata.description.as_str())
                            .unwrap_or_default(),
                    )
                    .clicked()
                    {
                        action(w, "finish-library.select", Some(&element.data));
                    }
                }
            }
        });
    if count == 0 {
        controls::label(
            ui,
            "appearance:catalog:empty",
            "finish-library-description",
            &[],
            "No matching finishes. Clear a filter, import a finish package, or download a texture and finish pack.",
            "empty",
        );
    }
    if let Some(finish) = selected(w) {
        controls::label(
            ui,
            "appearance:catalog:selected-details",
            "finish-library-details",
            &[],
            &finish.name,
            "readonly",
        );
        if let Some(metadata) = finish.metadata {
            controls::label(
                ui,
                "appearance:catalog:description",
                "finish-library-description",
                &[],
                &metadata.description,
                "ready",
            )
            .on_hover_text(metadata.source);
        }
    }
    ui.horizontal_wrapped(|ui| {
        for element in actions(w) {
            render_action(ui, w, &element);
        }
    });
}

/// The selected catalog record is a read-only preview until copied or assigned.
pub fn catalog_selected(w: &Workspace) -> bool {
    key(w).is_some_and(|key| key.starts_with("catalog:"))
}
