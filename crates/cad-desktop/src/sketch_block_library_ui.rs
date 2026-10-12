//! Personal pinned sketch-block catalog; insertion uses the existing sketch editor.
use crate::{Workspace, presentation_controls as controls};
use bevy_egui::egui;
use cad_core::{
    CadError, Id, Result, Sketch, configuration,
    presentation::SemanticElement,
    sketch_blocks::{
        self as blocks,
        library::{self, Catalog, Entry},
    },
};
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
};
const SELECTED: &str = "sketch-library-selected";
const LOCAL: &str = "sketch-library-local-block";
const QUERY: &str = "sketch-library-query";
const CATEGORY: &str = "sketch-library-category";
const FAVORITES: &str = "sketch-library-favorites";
const ARCHIVED: &str = "sketch-library-archived";
const ERROR: &str = "sketch-library-error";
#[derive(Default)]
struct Cache {
    catalog: Option<Catalog>,
    error: Option<String>,
}
fn cache() -> &'static Mutex<Cache> {
    static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(Cache::default()))
}
fn root() -> PathBuf {
    crate::settings_preferences::path()
        .parent()
        .unwrap_or(Path::new("."))
        .join("sketch-blocks")
}
fn cached() -> std::result::Result<Catalog, String> {
    let mut cache = cache()
        .lock()
        .map_err(|_| "Library cache is unavailable".to_owned())?;
    if cache.catalog.is_none() && cache.error.is_none() {
        match library::load(&root()) {
            Ok(catalog) => cache.catalog = Some(catalog),
            Err(error) => cache.error = Some(error.to_string()),
        }
    }
    cache.catalog.clone().ok_or_else(|| {
        cache
            .error
            .clone()
            .unwrap_or_else(|| "Library is unavailable".into())
    })
}
fn install(catalog: Catalog) {
    if let Ok(mut cache) = cache().lock() {
        cache.catalog = Some(catalog);
        cache.error = None;
    }
}
fn refresh() -> Result<()> {
    let result = library::load(&root());
    if let Ok(mut cache) = cache().lock() {
        match &result {
            Ok(catalog) => {
                cache.catalog = Some(catalog.clone());
                cache.error = None;
            }
            Err(error) => {
                cache.catalog = None;
                cache.error = Some(error.to_string());
            }
        }
    }
    result.map(|_| ())
}
fn native_source(w: &Workspace) -> Result<Sketch> {
    let sketch = w
        .sketch
        .as_ref()
        .ok_or_else(|| CadError::Invalid("Open a sketch first".into()))?;
    if sketch.choosing_plane
        || sketch.drag.is_some()
        || !sketch.chain.is_empty()
        || !sketch.clicks.is_empty()
    {
        return Err(CadError::Invalid(
            "Choose the sketch plane and finish the current drawing gesture first".into(),
        ));
    }
    if sketch.edit.is_some() {
        return Err(CadError::Invalid(
            "Accept or cancel the active sketch operation before using a library copy".into(),
        ));
    }
    if let Some(error) = sketch.error.as_ref().or(sketch.placement_error.as_ref()) {
        return Err(CadError::Invalid(format!(
            "Resolve the current sketch error first: {error}"
        )));
    }
    let source = crate::sketch::sketch_definition(sketch);
    if crate::sketch::source_buffer_is_pending(w, &source) {
        return Err(CadError::Invalid("Correct, Load or explicitly Refresh the unapplied Advanced Sketch Source before using the library; your text will be preserved".into()));
    }
    Ok(source)
}
fn target(w: &Workspace) -> Option<Value> {
    let sketch = w.sketch.as_ref()?;
    let source = crate::sketch::sketch_definition(sketch);
    use sha2::{Digest, Sha256};
    let bytes = serde_json::to_vec(&source).ok()?;
    Some(
        json!({"document":w.session.visible().id,"generation":w.session.generation,"owner":sketch.definition,"sketch":sketch.sketch_id,"sha256":format!("{:x}",Sha256::digest(bytes))}),
    )
}
fn check_target(w: &Workspace, args: Option<&Value>) -> Result<()> {
    if args.is_none() {
        return target(w)
            .map(|_| ())
            .ok_or_else(|| CadError::Invalid("Open a sketch first".into()));
    }
    let expected = args
        .and_then(|a| a.get("target"))
        .ok_or_else(|| CadError::Invalid("Use a current sketch-bound library control".into()))?;
    if target(w).as_ref() != Some(expected) {
        return Err(CadError::Invalid("The active sketch/source changed; refresh the library control before inserting or saving".into()));
    }
    Ok(())
}
fn entry(w: &Workspace, args: Option<&Value>) -> Result<(Catalog, Entry)> {
    let catalog = library::load(&root())?;
    let id = args
        .and_then(|a| a.get("entry"))
        .and_then(Value::as_str)
        .or_else(|| w.fields.get(SELECTED).map(String::as_str))
        .ok_or_else(|| CadError::Invalid("Select a library entry".into()))?;
    let entry = catalog.entries.get(id).cloned().ok_or_else(|| {
        CadError::Invalid("The library entry was removed; refresh the browser".into())
    })?;
    if args
        .and_then(|a| a.get("sha256"))
        .and_then(Value::as_str)
        .is_some_and(|sha| sha != entry.sha256)
    {
        return Err(CadError::Invalid(
            "The pinned library entry changed; refresh the browser".into(),
        ));
    }
    Ok((catalog, entry))
}
fn local_key(w: &Workspace, source: &Sketch) -> Option<Id> {
    w.fields
        .get(LOCAL)
        .filter(|id| source.blocks.contains_key(*id))
        .cloned()
        .or_else(|| {
            w.sketch
                .as_ref()?
                .selection
                .iter()
                .find_map(|entity| match entity {
                    cad_core::sketch_actions::SketchEntity::Member { member } => {
                        blocks::instance_for_member(source, member)
                            .map(|(_, instance)| instance.definition.clone())
                    }
                    cad_core::sketch_actions::SketchEntity::Point { point } => source
                        .block_instances
                        .values()
                        .find(|instance| &instance.anchor == point)
                        .map(|instance| instance.definition.clone()),
                    _ => None,
                })
        })
        .or_else(|| source.blocks.keys().next().cloned())
}
fn metadata(root: &Path, catalog: &Catalog, e: &Entry) -> Result<Catalog> {
    library::update_metadata(
        root,
        &e.id,
        &e.sha256,
        catalog.revision,
        library::Metadata {
            name: e.name.clone(),
            category: e.category.clone(),
            tags: e.tags.clone(),
            favorite: e.favorite,
            archived: e.archived,
        },
    )
}

pub fn close(w: &mut Workspace) {
    if crate::command_palette::context_editor(w) != Some("sketch-block-library") {
        return;
    }
    let before = w
        .fields
        .remove("sketch-library-before-dialog")
        .and_then(|raw| serde_json::from_str::<Option<String>>(&raw).ok())
        .flatten();
    w.dialog = before;
    w.manager_tree = w
        .fields
        .remove("sketch-library-before-tree")
        .map_or_else(|| w.dialog.is_none(), |value| value == "true");
}
pub fn action(w: &mut Workspace, command: &str, args: Option<&Value>) -> bool {
    if command != "sketch-block-library" && !command.starts_with("sketch-library.") {
        return false;
    }
    let result = (|| -> Result<()> {
        match command {
            "sketch-block-library" => {
                refresh()?;
                if crate::command_palette::context_editor(w) != Some("sketch-block-library") {
                    let tree = if w.dialog.as_deref() == Some("commands") {
                        w.fields
                            .get("command-palette-return-tree")
                            .is_none_or(|value| value == "true")
                    } else {
                        w.manager_tree
                    };
                    w.fields
                        .insert("sketch-library-before-tree".into(), tree.to_string());
                    w.fields.insert(
                        "sketch-library-before-dialog".into(),
                        serde_json::to_string(&crate::command_palette::context_editor(w))?,
                    );
                }
                w.dialog = Some("sketch-block-library".into());
                w.manager_tree = false;
            }
            "sketch-library.close" => close(w),
            "sketch-library.refresh" => refresh()?,
            "sketch-library.add" => {
                if let Some(paths) = rfd::FileDialog::new()
                    .set_title("Copy native sketch blocks into the personal library")
                    .add_filter("TCAD Sketch Block", &["tcadblock", "json"])
                    .pick_files()
                {
                    if paths.len() > library::MAX_ENTRIES {
                        return Err(CadError::Invalid("Add at most 512 files per action".into()));
                    }
                    for path in paths {
                        let (catalog, id) = library::add_file(&root(), &path)?;
                        install(catalog);
                        w.fields.insert(SELECTED.into(), id);
                    }
                }
            }
            "sketch-library.save-local" => {
                check_target(w, args)?;
                let source = native_source(w)?;
                let key = args
                    .and_then(|a| a.get("block"))
                    .and_then(Value::as_str)
                    .map(str::to_owned)
                    .or_else(|| local_key(w, &source))
                    .ok_or_else(|| {
                        CadError::Invalid("Create or choose a local block first".into())
                    })?;
                let owner = &w.sketch.as_ref().unwrap().definition;
                let effective = configuration::effective(w.session.visible())?;
                let package = blocks::resources::snapshot(&effective, owner, &source, &key)?;
                let (catalog, id) = library::add_package(&root(), &package)?;
                install(catalog);
                w.fields.insert(SELECTED.into(), id);
                w.status =
                    "Saved a pinned self-contained block copy; current sketch source is unchanged"
                        .into();
            }
            "sketch-library.select" => {
                let (catalog, e) = entry(w, args)?;
                install(catalog);
                w.fields.insert(SELECTED.into(), e.id);
            }
            "sketch-library.insert" => {
                check_target(w, args)?;
                native_source(w)?;
                let (catalog, e) = entry(w, args)?;
                let package = library::read_entry(&root(), &e.id, &e.sha256)?;
                install(catalog);
                crate::sketch_edit_ui::begin_block(w, crate::sketch_blocks_ui::Mode::Import, None);
                let block = w
                    .sketch
                    .as_mut()
                    .and_then(|s| s.edit.as_mut())
                    .and_then(|edit| edit.block.as_mut())
                    .ok_or_else(|| {
                        CadError::Invalid("Could not open the native block insertion editor".into())
                    })?;
                block.resource = Some(package);
                block.resource_error = None;
                block.resource_message=Some("Detached library copy · pinned native provenance retained · edit the insertion anchor in the sketch".into());
                close(w);
                w.dialog = None;
                w.manager_tree = false;
                crate::sketch_edit_ui::rebuild(w, false);
                if let Some(error) = w.sketch.as_ref().and_then(|sketch| {
                    sketch
                        .edit
                        .as_ref()
                        .and_then(|edit| edit.error.as_ref())
                        .or(sketch.error.as_ref())
                }) {
                    return Err(CadError::Invalid(format!(
                        "The native block insertion preview needs correction: {error}"
                    )));
                }
                w.status="Library copy is in the current sketch preview; use the native insertion controls and accept/cancel".into();
            }
            "sketch-library.favorite" | "sketch-library.remove" | "sketch-library.restore" => {
                let (catalog, mut e) = entry(w, args)?;
                if args
                    .and_then(|a| a.get("revision"))
                    .and_then(Value::as_u64)
                    .is_some_and(|revision| revision != catalog.revision)
                {
                    return Err(CadError::Invalid(
                        "Library metadata changed; refresh the browser before applying this action"
                            .into(),
                    ));
                }
                if command.ends_with("favorite") {
                    e.favorite = !e.favorite;
                } else {
                    e.archived = command.ends_with("remove");
                }
                install(metadata(&root(), &catalog, &e)?);
            }
            "sketch-library.export" => {
                let (_, e) = entry(w, args)?;
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("TCAD Sketch Block", &["tcadblock"])
                    .set_file_name("SketchBlock.tcadblock")
                    .save_file()
                {
                    library::export(&root(), &e.id, &e.sha256, &path)?;
                    w.status = format!("Exported pinned source to {}", path.display());
                }
            }
            _ => return Err(CadError::Invalid("Unknown sketch library action".into())),
        }
        Ok(())
    })();
    match result {
        Ok(()) => {
            w.fields.remove(ERROR);
        }
        Err(error) => {
            w.fields.insert(ERROR.into(), error.to_string());
            w.status = error.to_string();
        }
    }
    true
}
fn field(
    w: &Workspace,
    id: &str,
    label: &str,
    key: &str,
    value: Value,
    choices: Value,
) -> SemanticElement {
    SemanticElement {
        id: id.into(),
        role: "sketch-library-field".into(),
        label: label.into(),
        value: Some(
            value
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| value.to_string()),
        ),
        state: "ready".into(),
        commands: vec![],
        relationships: vec!["workspace.sketch-block-library".into()],
        data: json!({"setter":"sketch-block-library","field":key,"json":value,"editable":true,"choices":choices,"transient":true,"accessibility_label":label,"document":w.session.visible().id}),
    }
}
fn filtered(w: &Workspace, e: &Entry) -> bool {
    let archived = w.fields.get(ARCHIVED).is_some_and(|v| v == "true");
    if e.archived != archived {
        return false;
    }
    if w.fields.get(FAVORITES).is_some_and(|v| v == "true") && !e.favorite {
        return false;
    }
    if w.fields
        .get(CATEGORY)
        .is_some_and(|category| !category.is_empty() && category != &e.category)
    {
        return false;
    }
    let query = w
        .fields
        .get(QUERY)
        .map(|s| s.to_lowercase())
        .unwrap_or_default();
    query.split_whitespace().all(|q| {
        format!(
            "{} {} {} {}",
            e.name,
            e.category,
            e.tags.join(" "),
            e.package_id
        )
        .to_lowercase()
        .contains(q)
    })
}
pub fn elements(w: &Workspace) -> Vec<SemanticElement> {
    let open = w.dialog.as_deref() == Some("sketch-block-library");
    let target = target(w);
    let native = native_source(w).ok();
    let mut out = vec![SemanticElement {
        id: "workspace.sketch-block-library".into(),
        role: "sketch-block-library-editor".into(),
        label: "Sketch block library".into(),
        value: None,
        state: if open { "open" } else { "closed" }.into(),
        commands: vec!["sketch-block-library".into()],
        relationships: vec![],
        data: json!({"renderer":"sketch-block-library-editor","contract":"tcad.sketch-block-library","version":library::CONTRACT_VERSION,"root":root(),"maximum_entries":library::MAX_ENTRIES,"maximum_pinned_bytes":library::MAX_LIBRARY_BYTES,"admission_points":blocks::resources::MAX_POINTS,"admission_constraints":blocks::resources::MAX_CONSTRAINTS,"admission_work":blocks::resources::MAX_SOLVE_WORK,"source_unchanged_by_browsing":true}),
    }];
    let catalog = match cached() {
        Ok(catalog) => catalog,
        Err(error) => {
            out[0].state = "unavailable".into();
            out[0].data["diagnostics"] = json!([error]);
            Catalog::default()
        }
    };
    let categories = catalog
        .entries
        .values()
        .map(|e| e.category.clone())
        .filter(|s| !s.is_empty())
        .collect::<std::collections::BTreeSet<_>>();
    out.push(field(
        w,
        "sketch-library:query",
        "Search",
        QUERY,
        json!(w.fields.get(QUERY).map(String::as_str).unwrap_or("")),
        json!([]),
    ));
    out.push(field(
        w,
        "sketch-library:category",
        "Category",
        CATEGORY,
        json!(w.fields.get(CATEGORY).map(String::as_str).unwrap_or("")),
        json!(
            std::iter::once(json!({"value":"","label":"All categories"}))
                .chain(categories.into_iter().map(|c| json!({"value":c,"label":c})))
                .collect::<Vec<_>>()
        ),
    ));
    out.push(field(
        w,
        "sketch-library:favorites",
        "Favorites only",
        FAVORITES,
        json!(w.fields.get(FAVORITES).is_some_and(|v| v == "true")),
        json!([]),
    ));
    out.push(field(
        w,
        "sketch-library:removed",
        "Show removed copies",
        ARCHIVED,
        json!(w.fields.get(ARCHIVED).is_some_and(|v| v == "true")),
        json!([]),
    ));
    let source = native.as_ref();
    let local = source.and_then(|s| local_key(w, s));
    out.push(field(
        w,
        "sketch-library:local-block",
        "Current sketch block",
        LOCAL,
        json!(local),
        json!(
            source
                .map(|s| s
                    .blocks
                    .iter()
                    .map(|(id, b)| json!({"value":id,"label":b.name}))
                    .collect::<Vec<_>>())
                .unwrap_or_default()
        ),
    ));
    for (command, label, enabled) in [
        ("sketch-block-library", "Open sketch block library", true),
        ("sketch-library.add", "Add block files…", true),
        ("sketch-library.refresh", "Refresh index", true),
        (
            "sketch-library.save-local",
            "Save current block copy",
            local.is_some(),
        ),
    ] {
        out.push(SemanticElement {
            id: format!("command:{command}"),
            role: "sketch-library-action".into(),
            label: label.into(),
            value: None,
            state: if enabled { "ready" } else { "disabled" }.into(),
            commands: vec![command.into()],
            relationships: vec!["workspace.sketch-block-library".into()],
            data: json!({"target":target,"block":local,"accessibility_label":label}),
        });
    }
    for e in catalog.entries.values().filter(|e| filtered(w, e)) {
        out.push(SemanticElement{id:format!("sketch-library:entry:{}",e.id),role:"sketch-library-entry".into(),label:e.name.clone(),value:Some(e.category.clone()),state:if e.archived{"removed"}else if w.fields.get(SELECTED)==Some(&e.id){"selected"}else{"ready"}.into(),commands:vec!["sketch-library.select".into(),"sketch-library.insert".into(),"sketch-library.favorite".into(),"sketch-library.export".into(),if e.archived{"sketch-library.restore"}else{"sketch-library.remove"}.into()],relationships:vec!["workspace.sketch-block-library".into()],data:json!({"entry":e.id,"sha256":e.sha256,"revision":catalog.revision,"target":target,"category":e.category,"tags":e.tags,"favorite":e.favorite,"archived":e.archived,"package_id":e.package_id,"source":e.source,"points":e.points,"members":e.members,"constraints":e.constraints,"bytes":e.bytes,"package_sha256":e.package_sha256,"insertion_available":native.is_some()&&!e.archived})});
    }
    for row in out.iter_mut().filter(|e| e.role == "sketch-library-entry") {
        let removed = row.data["archived"] == true;
        let can_insert = row.data["insertion_available"] == true;
        row.data["actions"]=json!(row.commands.iter().map(|command| {
            let enabled=command!="sketch-library.insert"||can_insert;
            let label=match command.as_str(){"sketch-library.select"=>"Select block","sketch-library.insert"=>"Insert block copy","sketch-library.favorite"=>"Toggle favorite","sketch-library.export"=>"Export pinned copy","sketch-library.restore"=>"Restore removed entry",_=>"Remove from library"};
            json!({"command":command,"label":format!("{label} · {}",row.label),"enabled":enabled,"disabled_reason":if !enabled{Some(if removed{"Restore this library entry before insertion".to_owned()}else{native_source(w).err().map(|e|e.to_string()).unwrap_or_else(||"Open an active valid sketch".into())})}else{None}})
        }).collect::<Vec<_>>());
    }
    if let Some(e) = w
        .fields
        .get(SELECTED)
        .and_then(|id| catalog.entries.get(id))
    {
        for (key, label, value) in [
            ("name", "Library label", json!(e.name)),
            ("category", "Entry category", json!(e.category)),
            ("tags", "Search tags", json!(e.tags.join(", "))),
        ] {
            let mut f = field(
                w,
                &format!("sketch-library:entry:{}:{key}", e.id),
                label,
                key,
                value,
                json!([]),
            );
            f.data["entry"] = json!(e.id);
            f.data["sha256"] = json!(e.sha256);
            f.data["revision"] = json!(catalog.revision);
            f.data["transient"] = false.into();
            f.data["editor"] = json!(format!("{}:{}:{}", w.session.visible().id, e.id, e.sha256));
            out.push(f);
        }
    }
    for field in out
        .iter_mut()
        .filter(|e| e.data.get("setter").and_then(Value::as_str) == Some("sketch-block-library"))
    {
        if let Some(error) = w
            .fields
            .get(&format!("sketch-library-field-error:{}", field.id))
        {
            field.state = "invalid".into();
            field.data["diagnostics"] = json!([error]);
        }
    }
    out
}
pub fn field_is_invalid(w: &Workspace, e: &SemanticElement) -> bool {
    e.data.get("setter").and_then(Value::as_str) == Some("sketch-block-library")
        && w.fields
            .contains_key(&format!("sketch-library-field-error:{}", e.id))
}
pub fn field_error(w: &mut Workspace, e: &SemanticElement, error: Option<String>) {
    if e.data.get("setter").and_then(Value::as_str) != Some("sketch-block-library")
        || e.data.get("document").and_then(Value::as_str) != Some(w.session.visible().id.as_str())
        || e.data
            .get("entry")
            .and_then(Value::as_str)
            .is_some_and(|id| {
                cached()
                    .ok()
                    .and_then(|catalog| catalog.entries.get(id).cloned())
                    .is_none_or(|entry| {
                        e.data.get("sha256").and_then(Value::as_str) != Some(entry.sha256.as_str())
                    })
            })
    {
        return;
    }
    let key = format!("sketch-library-field-error:{}", e.id);
    match error {
        Some(error) => {
            w.fields.insert(key, error);
        }
        None => {
            w.fields.remove(&key);
        }
    }
}
pub fn current_value(w: &Workspace, e: &SemanticElement) -> Option<Value> {
    if e.data.get("setter")?.as_str()? != "sketch-block-library" {
        return None;
    }
    let key = e.data.get("field")?.as_str()?;
    if let Some(id) = e.data.get("entry").and_then(Value::as_str) {
        let catalog = cached().ok()?;
        let entry = catalog.entries.get(id)?;
        return Some(match key {
            "name" => json!(entry.name),
            "category" => json!(entry.category),
            "tags" => json!(entry.tags.join(", ")),
            _ => return None,
        });
    }
    Some(match key {
        FAVORITES | ARCHIVED => json!(w.fields.get(key).is_some_and(|v| v == "true")),
        LOCAL => json!(
            native_source(w)
                .ok()
                .and_then(|source| local_key(w, &source))
        ),
        _ => json!(w.fields.get(key).map(String::as_str).unwrap_or("")),
    })
}
pub fn set_value(w: &mut Workspace, e: &SemanticElement, value: Value) -> Result<()> {
    if e.data.get("setter").and_then(Value::as_str) != Some("sketch-block-library") {
        return Err(CadError::Invalid("Unknown library setter".into()));
    }
    let key = e
        .data
        .get("field")
        .and_then(Value::as_str)
        .ok_or_else(|| CadError::Invalid("Missing library field".into()))?;
    if e.data.get("entry").is_some() {
        let (catalog, mut entry) = entry(w, Some(&e.data))?;
        if e.data.get("revision").and_then(Value::as_u64) != Some(catalog.revision) {
            return Err(CadError::Invalid(
                "Library metadata changed; refresh this field".into(),
            ));
        }
        let text = value
            .as_str()
            .ok_or_else(|| CadError::Invalid("Enter library metadata text".into()))?;
        match key {
            "name" => entry.name = text.into(),
            "category" => entry.category = text.into(),
            "tags" => {
                entry.tags = text
                    .split(',')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned)
                    .collect()
            }
            _ => return Err(CadError::Invalid("Unknown entry metadata".into())),
        };
        install(metadata(&root(), &catalog, &entry)?);
        return Ok(());
    }
    if ![QUERY, CATEGORY, FAVORITES, ARCHIVED, LOCAL].contains(&key) {
        return Err(CadError::Invalid("Unknown browser field".into()));
    }
    let text = if matches!(key, FAVORITES | ARCHIVED) {
        value
            .as_bool()
            .ok_or_else(|| CadError::Invalid("Choose true or false".into()))?
            .to_string()
    } else {
        value
            .as_str()
            .ok_or_else(|| CadError::Invalid("Choose or enter a browser value".into()))?
            .to_owned()
    };
    if text.len() > 512 {
        return Err(CadError::Invalid("Browser value exceeds 512 bytes".into()));
    }
    w.fields.insert(key.into(), text);
    Ok(())
}
pub fn semantic_action(w: &mut Workspace, e: &SemanticElement, command: &str) {
    action(w, command, Some(&e.data));
}
pub fn show(ui: &mut egui::Ui, w: &mut Workspace) {
    controls::label(
        ui,
        "sketch-library:heading",
        "sketch-block-library-editor",
        &[],
        "Personal sketch blocks",
        "ready",
    );
    let current = elements(w);
    for field in current
        .iter()
        .filter(|e| e.role == "sketch-library-field" && e.data.get("entry").is_none())
    {
        crate::ui::semantic_field(ui, w, field);
    }
    ui.horizontal_wrapped(|ui| {
        for element in current.iter().filter(|e| {
            e.role == "sketch-library-action" && e.commands[0] != "sketch-block-library"
        }) {
            if controls::button(
                ui,
                &element.id,
                &element.role,
                &["semantic-control"],
                &element.label,
                element.state != "disabled",
                false,
            )
            .clicked()
            {
                semantic_action(w, element, &element.commands[0]);
            }
        }
    });
    let props = controls::properties(
        "sketch-library:list",
        "sketch-library-collection",
        &[],
        "ready",
    );
    egui::ScrollArea::vertical()
        .max_height(controls::number(&props, "max-height", 260.).max(24.))
        .show(ui, |ui| {
            for row in elements(w)
                .into_iter()
                .filter(|e| e.role == "sketch-library-entry")
            {
                if controls::button(
                    ui,
                    &row.id,
                    &row.role,
                    &["semantic-control"],
                    &format!(
                        "{}{} · {}",
                        if row.data["favorite"] == true {
                            "★ "
                        } else {
                            ""
                        },
                        row.label,
                        row.value.as_deref().unwrap_or("")
                    ),
                    true,
                    row.state == "selected",
                )
                .clicked()
                {
                    semantic_action(w, &row, "sketch-library.select");
                }
                if w.fields.get(SELECTED).map(String::as_str)
                    == row.data.get("entry").and_then(Value::as_str)
                {
                    controls::label(
                        ui,
                        &format!("{}:counts", row.id),
                        "sketch-library-description",
                        &[],
                        &format!(
                            "{} members · {} controls · {} relations",
                            row.data["members"], row.data["points"], row.data["constraints"]
                        ),
                        "ready",
                    );
                    ui.horizontal_wrapped(|ui| {
                        for (command, label) in [
                            ("sketch-library.insert", "Insert copy"),
                            ("sketch-library.favorite", "★"),
                            ("sketch-library.export", "Export copy…"),
                            (
                                if row.state == "removed" {
                                    "sketch-library.restore"
                                } else {
                                    "sketch-library.remove"
                                },
                                if row.state == "removed" {
                                    "Restore"
                                } else {
                                    "Remove"
                                },
                            ),
                        ] {
                            let enabled = command != "sketch-library.insert"
                                || row.data["insertion_available"] == true;
                            if controls::button(
                                ui,
                                &format!("{}:{command}", row.id),
                                "sketch-library-action",
                                &["semantic-control"],
                                label,
                                enabled,
                                false,
                            )
                            .clicked()
                            {
                                semantic_action(w, &row, command);
                            }
                        }
                    });
                }
            }
        });
    for field in elements(w)
        .into_iter()
        .filter(|e| e.role == "sketch-library-field" && e.data.get("entry").is_some())
    {
        crate::ui::semantic_field(ui, w, &field);
    }
    if let Err(error) = cached() {
        ui.colored_label(ui.visuals().error_fg_color, error);
    }
    if let Some(error) = w.fields.get(ERROR) {
        ui.colored_label(ui.visuals().error_fg_color, error);
    }
    controls::label(
        ui,
        "sketch-library:help",
        "sketch-library-description",
        &[],
        "Copies retain exact native source/provenance and become detached sketch definitions. Removing an entry is reversible; its pinned bytes remain. Accept/cancel insertion in the active sketch.",
        "ready",
    );
}
