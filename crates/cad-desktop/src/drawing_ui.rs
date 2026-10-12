//! Compact drawing controls and the same vector sheet used by PDF/DXF export.
use crate::Workspace;
use bevy_egui::egui;
use cad_core::{Drawing, commands::Command, drawing::*};

pub(crate) fn evaluation(w: &Workspace) -> Option<&cad_core::protocol::Evaluation> {
    w.session
        .draft
        .as_ref()
        .and_then(|d| d.evaluation.as_ref())
        .or(w.session.evaluation.as_ref())
        .filter(|e| (e.valid || e.geometry_valid) && e.identity == w.session.identity())
}
/// Paper controls use the represented model's local placements, independently
/// of the part/component currently displayed in the 3D workspace.
pub(crate) fn represented_instances(w: &Workspace) -> Vec<cad_core::PlacedPart> {
    let Some(evaluation) = evaluation(w) else {
        return vec![];
    };
    let source = w
        .fields
        .get("drawing-preview")
        .or_else(|| w.fields.get("drawing-active"))
        .and_then(|key| sheet_source(w, key));
    if let Some(sheet) = source {
        sheet_instances(w.session.visible(), evaluation, &sheet).unwrap_or_default()
    } else {
        evaluation.instances.clone()
    }
}

fn selected_path(w: &Workspace) -> Vec<String> {
    let Some(selected) = &w.selected else {
        return vec![];
    };
    if evaluation(w).is_some_and(|evaluation| {
        evaluation
            .instances
            .iter()
            .chain(
                evaluation
                    .assembly_welds
                    .values()
                    .map(|weld| &weld.placement),
            )
            .any(|instance| {
                instance.path == selected.path && instance.definition == selected.definition
            })
    }) {
        return selected.path.clone();
    }
    let paths = evaluation(w)
        .map(|e| {
            e.instances
                .iter()
                .chain(e.assembly_welds.values().map(|weld| &weld.placement))
                .filter(|i| {
                    i.definition == selected.definition
                        && i.path.last().is_some_and(|id| id == &selected.instance)
                })
                .map(|i| i.path.clone())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if paths.len() == 1 {
        paths[0].clone()
    } else {
        vec![]
    }
}
pub(crate) fn selected_curve(w: &Workspace) -> Option<CurveReference> {
    w.selected
        .as_ref()
        .filter(|s| s.reference.topology == "edge")
        .map(|s| CurveReference::ModelEdge {
            edge: s.reference.clone(),
            body: s.body.clone(),
            instance_path: selected_path(w),
        })
}
pub(crate) fn selected_anchor(w: &Workspace) -> Anchor {
    let reference =
        w.selected
            .as_ref()
            .and_then(|selected| match selected.reference.topology.as_str() {
                "edge" => Some(PointReference::EdgePoint {
                    edge: selected.reference.clone(),
                    body: selected.body.clone(),
                    parameter: 0.,
                    instance_path: selected_path(w),
                }),
                "face" => Some(PointReference::FaceOrigin {
                    face: selected.reference.clone(),
                    body: selected.body.clone(),
                    instance_path: selected_path(w),
                }),
                _ => None,
            });
    Anchor {
        point: w
            .selected
            .as_ref()
            .map(|selected| selected.point)
            .unwrap_or([0.; 3]),
        reference,
    }
}
pub(crate) fn reference_choices(w: &Workspace) -> Vec<(PointReference, String)> {
    let mut out = Vec::new();
    let Some(e) = evaluation(w) else {
        return out;
    };
    let doc = w.session.visible();
    let instances = represented_instances(w);
    for instance in &instances {
        let Some(part) = doc.parts.get(&instance.definition) else {
            continue;
        };
        let key = if instance.mesh_key.is_empty() {
            &instance.definition
        } else {
            &instance.mesh_key
        };
        for feature in &part.features {
            let Some(geometry) = e.sketches.get(&format!("{key}::{}", feature.id)) else {
                continue;
            };
            let suppressed = geometry
                .solved
                .as_ref()
                .map(cad_core::sketch_transform::suppressed_points)
                .unwrap_or_default();
            for (point, p) in &geometry.points {
                if suppressed.contains(point) {
                    continue;
                }
                out.push((
                    PointReference::SketchPoint {
                        sketch: part.reference(&feature.id),
                        point: point.clone(),
                        instance_path: instance.path.clone(),
                    },
                    format!(
                        "{} / {} ({:.2}, {:.2})",
                        instance.name, feature.name, p[0], p[1]
                    ),
                ));
            }
        }
    }
    for placement in instances
        .iter()
        .filter(|placement| !doc.parts.contains_key(&placement.definition))
    {
        let Some(mesh) = e.meshes.get(&placement.mesh_key) else {
            continue;
        };
        for (index, edge) in mesh
            .edges
            .iter()
            .enumerate()
            .filter(|(_, edge)| edge.curve.is_some())
        {
            for parameter in [0., 1.] {
                out.push((
                    PointReference::EdgePoint {
                        edge: edge.reference.clone(),
                        body: edge.body.clone(),
                        parameter,
                        instance_path: placement.path.clone(),
                    },
                    format!(
                        "{} / Bead Edge {} · {}",
                        placement.name,
                        index + 1,
                        if parameter == 0. { "Start" } else { "Finish" }
                    ),
                ));
            }
        }
        for (index, face) in mesh
            .faces
            .iter()
            .enumerate()
            .filter(|(_, face)| face.plane.is_some())
        {
            out.push((
                PointReference::FaceOrigin {
                    face: face.reference.clone(),
                    body: face.body.clone(),
                    instance_path: placement.path.clone(),
                },
                format!("{} / Bead Plane {}", placement.name, index + 1),
            ));
        }
    }
    out
}
pub(crate) fn sheet_source(w: &Workspace, key: &str) -> Option<Drawing> {
    w.fields
        .get(&format!("drawing-editor:{key}"))
        .and_then(|s| serde_json::from_str(s).ok())
        .or_else(|| {
            w.session
                .visible()
                .drawings
                .iter()
                .find(|s| s.id == key)
                .cloned()
        })
}
pub fn current_source_is_valid(w: &Workspace) -> bool {
    w.fields
        .iter()
        .filter(|(key, _)| key.starts_with("drawing-editor:"))
        .all(|(_, value)| {
            serde_json::from_str::<Drawing>(value)
                .is_ok_and(|source| w.session.visible().drawings.iter().any(|s| s == &source))
        })
}
fn sync_source_validity(w: &mut Workspace) {
    if current_source_is_valid(w) {
        w.fields.remove("drawing-source-invalid");
    } else {
        w.fields
            .insert("drawing-source-invalid".into(), "true".into());
    }
}
pub(crate) fn put(w: &mut Workspace, sheet: Drawing) {
    match serde_json::to_string(&sheet) {
        Ok(source) => {
            w.start_tool();
            w.fields
                .insert(format!("drawing-editor:{}", sheet.id), source);
            let requested = sheet.clone();
            w.dispatch(Command::PutDrawing { drawing: sheet });
            // The command may append native source-row reservations. Accept that
            // normalization only if every explicitly edited field was preserved.
            if let Some(actual) = w
                .session
                .visible()
                .drawings
                .iter()
                .find(|s| s.id == requested.id)
            {
                let mut normalized = requested.clone();
                for table in &mut normalized.tables {
                    if let Some(current) = actual.tables.iter().find(|t| t.id == table.id)
                        && table
                            .item_numbers
                            .iter()
                            .all(|(key, number)| current.item_numbers.get(key) == Some(number))
                        && current.row_order.starts_with(&table.row_order)
                    {
                        table.item_numbers = current.item_numbers.clone();
                        table.row_order = current.row_order.clone();
                    }
                }
                if &normalized == actual
                    && let Ok(source) = serde_json::to_string(actual)
                {
                    w.fields
                        .insert(format!("drawing-editor:{}", actual.id), source);
                }
            }
            sync_source_validity(w);
        }
        Err(error) => {
            w.status = error.to_string();
            w.fields
                .insert("drawing-source-invalid".into(), "true".into());
        }
    }
}
/// Both custom compositions and the docked editor read the latest staged source,
/// including a rejected expression, rather than copying a frame-old document.
pub fn current_value(
    w: &Workspace,
    element: &cad_core::presentation::SemanticElement,
) -> Option<serde_json::Value> {
    let sheet = sheet_source(w, element.data.get("sheet_id")?.as_str()?)?;
    let entity = sheet.entities.iter().find(|entity| {
        Some(entity.id.as_str()) == element.data.get("annotation_id").and_then(|v| v.as_str())
    })?;
    let value = serde_json::to_value(entity)
        .ok()?
        .pointer(element.data.get("field")?.as_str()?)
        .cloned()?;
    Some(
        if element
            .data
            .get("optional_enabled")
            .and_then(|v| v.as_bool())
            == Some(true)
        {
            serde_json::json!(!value.is_null())
        } else {
            value
        },
    )
}
pub fn set_value(
    w: &mut Workspace,
    element: &cad_core::presentation::SemanticElement,
    value: serde_json::Value,
) -> cad_core::Result<()> {
    if !drawing_context(w) {
        return Err(cad_core::CadError::Invalid(w.status.clone()));
    }
    let sheet_id = element
        .data
        .get("sheet_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| cad_core::CadError::Invalid("Missing drawing sheet field owner".into()))?;
    let annotation_id = element
        .data
        .get("annotation_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            cad_core::CadError::Invalid("Missing drawing annotation field owner".into())
        })?;
    let path = element
        .data
        .get("field")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            cad_core::CadError::Invalid("Missing drawing annotation field path".into())
        })?;
    if !(path.starts_with("/kind/symbol/") || path.starts_with("/offset_mm/") || path == "/view")
        || path.ends_with("/version")
    {
        return Err(cad_core::CadError::Invalid(
            "This annotation field is not editable".into(),
        ));
    }
    let mut sheet = sheet_source(w, sheet_id).ok_or_else(|| {
        cad_core::CadError::Invalid("The annotation's sheet no longer exists".into())
    })?;
    let entity = sheet
        .entities
        .iter_mut()
        .find(|entity| entity.id == annotation_id)
        .ok_or_else(|| cad_core::CadError::Invalid("The annotation no longer exists".into()))?;
    if !matches!(
        entity.kind,
        AnnotationKind::SurfaceFinish { .. } | AnnotationKind::WeldSymbol { .. }
    ) {
        return Err(cad_core::CadError::Invalid(
            "The annotation's field contract has changed".into(),
        ));
    }
    let mut source = serde_json::to_value(&*entity)?;
    let field = source.pointer_mut(path).ok_or_else(|| {
        cad_core::CadError::Invalid("The annotation field no longer exists".into())
    })?;
    if element
        .data
        .get("optional_enabled")
        .and_then(|v| v.as_bool())
        == Some(true)
    {
        let enabled = value.as_bool().ok_or_else(|| {
            cad_core::CadError::Invalid("Expected an annotation option toggle".into())
        })?;
        if !enabled {
            *field = serde_json::Value::Null;
        } else if field.is_null() {
            *field = element.data.get("default").cloned().ok_or_else(|| {
                cad_core::CadError::Invalid("Missing annotation option default".into())
            })?;
        }
    } else {
        *field = value;
    }
    *entity = serde_json::from_value(source)?;
    select_sheet(w, sheet_id);
    w.fields.insert("drawing-item".into(), annotation_id.into());
    put(w, sheet);
    if !current_source_is_valid(w) {
        return Err(cad_core::CadError::Invalid(w.status.clone()));
    }
    Ok(())
}
fn unique_name(doc: &cad_core::Document, base: &str) -> String {
    let mut name = base.to_string();
    let mut index = 2;
    while doc
        .drawings
        .iter()
        .any(|s| s.name.trim().to_lowercase() == name.trim().to_lowercase())
    {
        name = format!("{base} {index}");
        index += 1;
    }
    name
}
pub fn is_workspace(w: &Workspace) -> bool {
    w.view.drawing_workspace || w.dialog.as_deref() == Some("drawings")
}
/// Enter a paper workspace without starting a model draft.
pub fn open_workspace(w: &mut Workspace) -> bool {
    if !drawing_context(w) {
        return false;
    }
    restore_workspace_fields(w);
    true
}
pub fn leave_workspace(w: &mut Workspace) -> bool {
    if w.tool_checkpoint.is_some() || crate::drawing_interaction_ui::active(w) {
        w.status = "Apply or cancel the current drawing edit before returning to Model".into();
        return false;
    }
    w.view.drawing_workspace = false;
    if w.command_tab == "Drawing" {
        w.command_tab = "Features".into();
    }
    w.fields.remove("drawing-preview");
    if w.dialog.as_deref() == Some("drawings") {
        w.dialog = None;
    }
    w.manager_tree = true;
    true
}
fn restore_workspace_fields(w: &mut Workspace) {
    let key = w
        .view
        .drawing_sheet
        .as_ref()
        .filter(|key| {
            w.session
                .visible()
                .drawings
                .iter()
                .any(|sheet| &sheet.id == *key)
        })
        .cloned()
        .or_else(|| {
            w.session
                .visible()
                .drawings
                .first()
                .map(|sheet| sheet.id.clone())
        });
    w.view.drawing_sheet = key.clone();
    w.command_tab = "Drawing".into();
    w.dialog = Some("drawings".into());
    w.manager_tree = false;
    w.fields
        .entry("drawing-editor-generation".into())
        .or_insert_with(cad_core::id);
    if let Some(key) = key {
        let max = w
            .session
            .visible()
            .drawings
            .iter()
            .find(|sheet| sheet.id == key)
            .map(cad_core::drawing_tables::page_count)
            .unwrap_or(1);
        w.view.drawing_page = w.view.drawing_page.min(max.saturating_sub(1));
        w.fields.insert("drawing-active".into(), key.clone());
        w.fields.insert("drawing-preview".into(), key);
        w.fields
            .insert("drawing-page".into(), w.view.drawing_page.to_string());
    } else {
        w.fields.remove("drawing-active");
        w.fields.remove("drawing-preview");
        w.view.drawing_selection = None;
        w.view.drawing_page = 0;
    }
}
/// Called after shared Apply/Cancel clears transient source controls. Paper zoom,
/// pan and surviving selection remain view preferences, independent of the draft.
pub fn retain_workspace(w: &mut Workspace) {
    if !w.view.drawing_workspace {
        return;
    }
    crate::drawing_editor_ui::reset(w);
    crate::drawing_interaction_ui::finish_edit(w);
    restore_workspace_fields(w);
    if let Some(target) = w.view.drawing_selection.clone() {
        crate::drawing_interaction_ui::select_item(w, target);
    }
}
pub(crate) fn drawing_context(w: &mut Workspace) -> bool {
    if w.sketch.is_some() || w.tool_checkpoint.is_some() && w.dialog.as_deref() != Some("drawings")
    {
        w.status = "Accept or cancel the current operation before editing drawing sheets".into();
        return false;
    }
    if w.dialog.as_deref() != Some("drawings") {
        w.open_editor("drawings");
    }
    w.manager_tree = false;
    w.command_tab = "Drawing".into();
    w.view.drawing_workspace = true;
    true
}
pub(crate) fn select_sheet(w: &mut Workspace, key: &str) {
    if w.fields.get("drawing-preview").is_none_or(|id| id != key) {
        crate::drawing_interaction_ui::escape(w);
        w.fields.insert("drawing-page".into(), "0".into());
        w.view.drawing_page = 0;
        w.view.drawing_selection = None;
    }
    w.view.drawing_sheet = Some(key.into());
    w.view.drawing_workspace = true;
    w.fields.insert("drawing-preview".into(), key.into());
    w.fields.insert("drawing-active".into(), key.into());
}
fn read_template() -> cad_core::Result<Option<SheetTemplate>> {
    let Some(path) = rfd::FileDialog::new()
        .add_filter("TCAD sheet template", &["json"])
        .pick_file()
    else {
        return Ok(None);
    };
    if std::fs::metadata(&path)?.len() > 1024 * 1024 {
        return Err(cad_core::CadError::Invalid(
            "A sheet template may not exceed 1 MiB".into(),
        ));
    }
    let template: SheetTemplate = serde_json::from_str(&std::fs::read_to_string(path)?)?;
    template.validate()?;
    Ok(Some(template))
}
fn load_template(w: &mut Workspace, key: Option<&str>) {
    let result = (|| -> cad_core::Result<Option<Drawing>> {
        let Some(template) = read_template()? else {
            return Ok(None);
        };
        if let Some(key) = key {
            let mut sheet = sheet_source(w, key)
                .ok_or_else(|| cad_core::CadError::Invalid("Drawing sheet is missing".into()))?;
            template.apply_format(&mut sheet)?;
            Ok(Some(sheet))
        } else {
            let doc = w.session.visible();
            let definition = w.view.active_part.as_deref().unwrap_or(&doc.root);
            Ok(Some(template.instantiate(
                doc,
                definition,
                unique_name(doc, &template.name),
            )?))
        }
    })();
    match result {
        Ok(Some(sheet)) => {
            select_sheet(w, &sheet.id);
            put(w, sheet);
        }
        Ok(None) => {}
        Err(error) => w.status = error.to_string(),
    }
}
fn save_template(w: &mut Workspace, key: &str) {
    let Some(sheet) = sheet_source(w, key) else {
        return;
    };
    let template = SheetTemplate::from_sheet(&sheet);
    let result = (|| -> cad_core::Result<()> {
        template.validate()?;
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("TCAD sheet template", &["json"])
            .set_file_name("sheet-template.json")
            .save_file()
        {
            cad_core::persistence::atomic_write(
                &path,
                serde_json::to_string_pretty(&template)?.as_bytes(),
            )?;
        }
        Ok(())
    })();
    if let Err(error) = result {
        w.status = error.to_string();
    }
}
fn add_symbol(w: &mut Workspace, requested: Option<&str>, command: &str) {
    if !drawing_context(w) {
        return;
    }
    let key = requested
        .map(str::to_owned)
        .or_else(|| w.fields.get("drawing-active").cloned())
        .filter(|key| {
            w.session
                .visible()
                .drawings
                .iter()
                .any(|sheet| &sheet.id == key)
        })
        .or_else(|| {
            w.session
                .visible()
                .drawings
                .first()
                .map(|sheet| sheet.id.clone())
        });
    let mut sheet = if let Some(key) = key {
        let Some(sheet) = sheet_source(w, &key) else {
            return;
        };
        sheet
    } else {
        let doc = w.session.visible();
        let definition = w.view.active_part.as_deref().unwrap_or(&doc.root);
        match new_sheet(doc, definition, unique_name(doc, "Sheet")) {
            Ok(sheet) => sheet,
            Err(error) => {
                w.status = error.to_string();
                return;
            }
        }
    };
    let target = selected_anchor(w);
    let kind = if command == "drawing.surface-finish-add" {
        AnnotationKind::SurfaceFinish {
            target,
            symbol: SurfaceFinish::default(),
        }
    } else {
        AnnotationKind::WeldSymbol {
            target,
            symbol: WeldSymbol::default(),
        }
    };
    let annotation = Annotation {
        id: cad_core::id(),
        view: w
            .fields
            .get("drawing-view")
            .filter(|view| view_keys(&sheet).contains(view))
            .cloned()
            .unwrap_or_else(|| view_keys(&sheet)[0].clone()),
        offset_mm: [15., 15.],
        leader_points_mm: vec![],
        kind,
    };
    w.fields
        .insert("drawing-item".into(), annotation.id.clone());
    w.fields.insert("drawing-section".into(), "items".into());
    select_sheet(w, &sheet.id);
    sheet.entities.push(annotation);
    put(w, sheet);
}
pub fn action(w: &mut Workspace, command: &str) -> bool {
    match command {
        "drawing.workspace-open" => {
            open_workspace(w);
            return true;
        }
        "drawing.workspace-model" => {
            leave_workspace(w);
            return true;
        }
        _ => {}
    }
    if matches!(
        command,
        "drawing.surface-finish-add" | "drawing.weld-symbol-add"
    ) {
        add_symbol(w, None, command);
        return true;
    }
    if !matches!(command, "drawing.sheet-add" | "drawing.sheet-template-load") {
        return false;
    }
    if !drawing_context(w) {
        return true;
    }
    if command == "drawing.sheet-template-load" {
        load_template(w, None);
        return true;
    }
    let doc = w.session.visible();
    let definition = w.view.active_part.as_deref().unwrap_or(&doc.root);
    match new_sheet(doc, definition, unique_name(doc, "Sheet")) {
        Ok(sheet) => {
            select_sheet(w, &sheet.id);
            put(w, sheet);
        }
        Err(error) => w.status = error.to_string(),
    }
    true
}
pub fn semantic_action(
    w: &mut Workspace,
    element: &cad_core::presentation::SemanticElement,
    command: &str,
) -> bool {
    if matches!(
        command,
        "drawing.surface-finish-add" | "drawing.weld-symbol-add"
    ) {
        add_symbol(
            w,
            element
                .data
                .get("sheet_id")
                .and_then(serde_json::Value::as_str),
            command,
        );
        return true;
    }
    if action(w, command) {
        return true;
    }
    if !command.starts_with("drawing.") {
        return false;
    }
    if !drawing_context(w) {
        return true;
    }
    let key = element
        .data
        .get("sheet_id")
        .and_then(serde_json::Value::as_str)
        .or_else(|| {
            element
                .data
                .get("sheet")
                .and_then(|s| s.get("id"))
                .and_then(serde_json::Value::as_str)
        });
    let Some(key) = key
        .filter(|key| w.session.visible().drawings.iter().any(|s| &s.id == key))
        .map(str::to_string)
    else {
        return true;
    };
    select_sheet(w, &key);
    if command == "drawing.tree-select" {
        if let Some(target) = element
            .data
            .get("target")
            .filter(|target| !target.is_null())
            .and_then(|target| serde_json::from_value(target.clone()).ok())
        {
            crate::drawing_interaction_ui::select_item(w, target);
        } else {
            w.fields.insert("drawing-section".into(), "sheet".into());
        }
        return true;
    }
    if command == "drawing.sheet-edit" {
        let section = match element.role.as_str() {
            "drawing-sheet-format" | "drawing-title-field" => "format",
            "drawing-view" => "views",
            _ => "items",
        };
        w.fields.insert("drawing-section".into(), section.into());
        w.fields.insert(
            "drawing-item".into(),
            element
                .data
                .get("annotation_id")
                .and_then(serde_json::Value::as_str)
                .unwrap_or(&element.id)
                .into(),
        );
        if let Some(table) = element
            .data
            .get("table_id")
            .and_then(serde_json::Value::as_str)
        {
            w.fields.insert("drawing-table".into(), table.into());
        }
        if let Some(view) = element
            .data
            .get("view_key")
            .and_then(serde_json::Value::as_str)
        {
            w.fields.insert("drawing-view".into(), view.into());
        }
    }
    match command {
        "drawing.sheet-edit" | "drawing.sheet-preview" => {}
        "drawing.annotation-delete" => {
            let annotation_id = element
                .data
                .get("annotation_id")
                .and_then(serde_json::Value::as_str);
            if let Some(mut sheet) = sheet_source(w, &key)
                && let Some(id) = annotation_id
            {
                let old = sheet.entities.len();
                sheet.entities.retain(|entity| entity.id != id);
                if old != sheet.entities.len() {
                    put(w, sheet);
                }
            }
        }
        "drawing.sheet-template-save" => save_template(w, &key),
        "drawing.sheet-format-load" => load_template(w, Some(&key)),
        "drawing.sheet-delete"
        | "drawing.sheet-duplicate"
        | "drawing.sheet-up"
        | "drawing.sheet-down" => {
            if !current_source_is_valid(w) {
                w.status =
                    "Correct or cancel the current drawing source before managing sheets".into();
                return true;
            }
            let doc = w.session.visible();
            let index = doc.drawings.iter().position(|s| s.id == key).unwrap();
            let before = doc
                .drawings
                .iter()
                .map(|s| s.id.clone())
                .collect::<std::collections::BTreeSet<_>>();
            let command = match command {
                "drawing.sheet-delete" => Command::DeleteDrawing {
                    drawing: key.clone(),
                },
                "drawing.sheet-duplicate" => Command::DuplicateDrawing {
                    drawing: key.clone(),
                    name: unique_name(doc, &format!("{} Copy", doc.drawings[index].name)),
                },
                "drawing.sheet-up" if index > 0 => Command::MoveDrawing {
                    drawing: key.clone(),
                    index: index - 1,
                },
                "drawing.sheet-down" if index + 1 < doc.drawings.len() => Command::MoveDrawing {
                    drawing: key.clone(),
                    index: index + 1,
                },
                _ => return true,
            };
            w.start_tool();
            w.dispatch(command);
            if let Some(next) = w
                .session
                .visible()
                .drawings
                .iter()
                .find(|s| !before.contains(&s.id))
                .map(|s| s.id.clone())
            {
                select_sheet(w, &next);
            }
            if !w.session.visible().drawings.iter().any(|s| s.id == key) {
                w.fields.remove(&format!("drawing-editor:{key}"));
                if let Some(next) = w.session.visible().drawings.first().map(|s| s.id.clone()) {
                    select_sheet(w, &next);
                } else {
                    w.fields.remove("drawing-preview");
                    w.fields.remove("drawing-active");
                }
            }
            sync_source_validity(w);
        }
        _ => {}
    }
    true
}
pub(crate) fn sheet_command(w: &mut Workspace, key: &str, command: &str) {
    let element = cad_core::presentation::SemanticElement {
        id: key.into(),
        role: "drawing-sheet".into(),
        label: "Drawing sheet".into(),
        value: None,
        state: "ready".into(),
        commands: vec![],
        relationships: vec![],
        data: serde_json::json!({"sheet_id":key}),
    };
    semantic_action(w, &element, command);
}
pub fn editor(ui: &mut egui::Ui, w: &mut Workspace) {
    if w.view.drawing_workspace && !w.fields.contains_key("drawing-preview") {
        restore_workspace_fields(w);
    }
    crate::drawing_interaction_ui::show_controls(ui, w);
    crate::drawing_editor_ui::show(ui, w);
}

pub fn preview(
    w: &mut Workspace,
    ui: &mut egui::Ui,
    rect: egui::Rect,
    response: &egui::Response,
    viewport_id: &str,
) -> bool {
    crate::drawing_interaction_ui::frame(w, ui, rect, response, viewport_id)
}

/// Independently bindable paper navigation, sheet tree and construction information.
pub fn workspace_semantics(w: &Workspace) -> Vec<cad_core::presentation::SemanticElement> {
    use cad_core::presentation::SemanticElement;
    use serde_json::json;
    let mut out = vec![];
    for (command, label, enabled) in [
        (
            "drawing.workspace-open",
            "Drawing",
            w.sketch.is_none() && (w.tool_checkpoint.is_none() || is_workspace(w)),
        ),
        (
            "drawing.workspace-model",
            "Model",
            w.tool_checkpoint.is_none() && !crate::drawing_interaction_ui::active(w),
        ),
    ] {
        let enabled = enabled && w.settings.is_none();
        out.push(SemanticElement {
            id: format!("command:{command}"), role: "command".into(), label: label.into(),
            value: None, state: if enabled { "available" } else { "disabled" }.into(),
            commands: vec![command.into()], relationships: vec![],
            data: json!({"enabled":enabled,"workspace":"drawing","actions":[{"command":command,"label":label,"enabled":enabled}]}),
        });
    }
    if !is_workspace(w) {
        return out;
    }
    let active = w
        .fields
        .get("drawing-active")
        .or(w.view.drawing_sheet.as_ref());
    let target =
        crate::drawing_interaction_ui::selected_target(w).or(w.view.drawing_selection.as_ref());
    out.push(SemanticElement {
        id: "drawing.workspace-tree".into(),
        role: "drawing-tree-group".into(),
        label: "Sheets".into(),
        value: None,
        state: "ready".into(),
        commands: vec![
            "drawing.sheet-add".into(),
            "drawing.sheet-template-load".into(),
        ],
        relationships: vec![],
        data: json!({"contract":"tcad.presentation.drawing-workspace","version":1}),
    });
    for sheet in &w.session.visible().drawings {
        let sheet_selected = active == Some(&sheet.id);
        out.push(tree_element(
            &sheet.id,
            &sheet.id,
            &sheet.name,
            None,
            sheet_selected,
            "sheet",
        ));
        for view in view_keys(sheet) {
            let item = cad_core::drawing_interaction::Target::View { key: view.clone() };
            out.push(tree_element(
                &view_id(sheet, &view),
                &sheet.id,
                &view,
                Some(&item),
                sheet_selected && target == Some(&item),
                "view",
            ));
        }
        for dimension in &sheet.dimensions {
            let item = cad_core::drawing_interaction::Target::Dimension {
                id: dimension.id.clone(),
            };
            out.push(tree_element(
                &dimension.id,
                &sheet.id,
                &format!("{:?} dimension", dimension.options.kind),
                Some(&item),
                sheet_selected && target == Some(&item),
                "dimension",
            ));
        }
        for annotation in &sheet.entities {
            let item = cad_core::drawing_interaction::Target::Annotation {
                id: annotation.id.clone(),
            };
            out.push(tree_element(
                &annotation.id,
                &sheet.id,
                annotation.kind.label(),
                Some(&item),
                sheet_selected && target == Some(&item),
                "annotation",
            ));
        }
        for table in &sheet.tables {
            let item = cad_core::drawing_interaction::Target::Table {
                id: table.id.clone(),
            };
            out.push(tree_element(
                &table.id,
                &sheet.id,
                &table.title,
                Some(&item),
                sheet_selected && target == Some(&item),
                "table",
            ));
        }
    }
    if let Some(paper) = crate::drawing_interaction_ui::rendered_sheet(w) {
        out.push(SemanticElement {
            id: format!("{}:construction-info", paper.id), role: "drawing-source-info".into(), label: "Drawing information".into(),
            value: Some(format!("Source revision {}", paper.source_info.revision)), state: "ready".into(), commands: vec![],
            relationships: vec![paper.source_info.document.clone(), paper.id.clone()],
            data: json!({"contract":"tcad.drawing-render-information","version":1,"source":paper.source_info,"view_fidelity":paper.view_fidelity,"diagnostics":paper.diagnostics,"production_vectors_include_debug_metadata":false}),
        });
        for (key, label, value) in [
            (
                "fingerprint",
                "Source",
                paper.source_info.fingerprint.clone(),
            ),
            ("backend", "Evaluator", paper.source_info.backend.clone()),
        ] {
            out.push(SemanticElement {
                id: format!("{}:construction-info:{key}", paper.id), role: "drawing-source-detail".into(),
                label: label.into(), value: Some(value.clone()), state: "ready".into(), commands: vec![],
                relationships: vec![format!("{}:construction-info", paper.id)],
                data: json!({"contract":"tcad.drawing-render-information","version":1,"field":key,"json":value}),
            });
        }
        for (key, fidelity) in &paper.view_fidelity {
            out.push(SemanticElement {
                id: format!("{}:view-info:{key}", paper.id), role: "drawing-view-info".into(), label: key.clone(),
                value: Some(format!("{} · Hidden lines included", fidelity.description)),
                state: if fidelity.approximate { "approximate" } else { "ready" }.into(), commands: vec![],
                relationships: vec![paper.id.clone()],
                data: json!({"contract":"tcad.drawing-render-information","version":1,"view_key":key,"fidelity":fidelity,"projection":paper.projections.get(key)}),
            });
        }
    }
    out.extend(footer_semantics(w));
    out
}
fn tree_element(
    id: &str,
    sheet: &str,
    label: &str,
    target: Option<&cad_core::drawing_interaction::Target>,
    selected: bool,
    kind: &str,
) -> cad_core::presentation::SemanticElement {
    cad_core::presentation::SemanticElement {
        id: format!("drawing-tree:{id}"),
        role: "drawing-tree-item".into(),
        label: label.into(),
        value: None,
        state: if selected { "selected" } else { "ready" }.into(),
        commands: vec!["drawing.tree-select".into()],
        relationships: vec![sheet.into()],
        data: serde_json::json!({"contract":"tcad.presentation.drawing-workspace","version":1,"sheet_id":sheet,"target":target,"item_kind":kind,"actions":[{"command":"drawing.tree-select","label":"Select","enabled":true}]}),
    }
}
pub fn tree(ui: &mut egui::Ui, w: &mut Workspace) {
    use crate::presentation_controls as controls;
    let elements = workspace_semantics(w);
    let props = controls::properties(
        "drawing.workspace-tree",
        "drawing-tree-group",
        &["compact"],
        "ready",
    );
    if props.get("display").is_some_and(|value| value == "none") {
        return;
    }
    ui.scope(|ui| {
        controls::apply_properties(ui, &props);
        controls::label(
            ui,
            "drawing.workspace-tree:title",
            "drawing-tree-title",
            &["compact"],
            "Sheets",
            "ready",
        );
        for element in elements
            .iter()
            .filter(|element| element.role == "drawing-tree-item")
        {
            let kind = element.data["item_kind"].as_str().unwrap_or("sheet");
            let sheet = element.data["sheet_id"].as_str();
            if kind != "sheet" && sheet != w.fields.get("drawing-active").map(String::as_str) {
                continue;
            }
            let clicked = if kind == "sheet" {
                controls::selectable(
                    ui,
                    &element.id,
                    &element.role,
                    &element.label,
                    element.state == "selected",
                )
                .clicked()
            } else {
                ui.indent(&element.id, |ui| {
                    controls::selectable(
                        ui,
                        &element.id,
                        &element.role,
                        &element.label,
                        element.state == "selected",
                    )
                    .clicked()
                })
                .inner
            };
            if clicked {
                semantic_action(w, element, "drawing.tree-select");
            }
        }
        egui::CollapsingHeader::new("Drawing information")
            .id_salt("drawing.workspace-info")
            .show(ui, |ui| {
                for element in elements.iter().filter(|element| {
                    matches!(
                        element.role.as_str(),
                        "drawing-source-info" | "drawing-source-detail" | "drawing-view-info"
                    )
                }) {
                    let text = format!(
                        "{} · {}",
                        element.label,
                        element.value.as_deref().unwrap_or("")
                    );
                    controls::label(
                        ui,
                        &element.id,
                        &element.role,
                        &["compact"],
                        &text,
                        &element.state,
                    );
                }
            });
    });
}
/// The default dock pins this group below scrolling properties; all actions are
/// independently available to user-defined compositions.
pub fn footer_semantics(w: &Workspace) -> Vec<cad_core::presentation::SemanticElement> {
    use serde_json::json;
    if !is_workspace(w) {
        return vec![];
    }
    let source_valid = crate::drawing_editor_ui::current_source_is_valid(w)
        && crate::drawing_interaction_ui::current_source_is_valid(w);
    let export_ready = source_valid
        && w.session.draft.is_none()
        && evaluation(w).is_some_and(|e| e.valid)
        && !w.session.visible().drawings.is_empty();
    [
        ("draft.apply", "Apply", crate::cad_ui::confirmation_available(w)),
        ("draft.cancel", "Cancel edit", w.tool_checkpoint.is_some()),
        ("file.export-pdf", "PDF", export_ready),
        ("file.export-drawing-dxf", "DXF", export_ready),
        ("drawing.workspace-model", "Model", w.tool_checkpoint.is_none() && !crate::drawing_interaction_ui::active(w)),
    ].into_iter().map(|(command,label,enabled)| {
        let enabled = enabled && w.settings.is_none();
        cad_core::presentation::SemanticElement {
        id: format!("drawing-footer:{command}"), role: "drawing-workspace-action".into(), label: label.into(),
        value: (!enabled).then(|| if command.starts_with("file.export") { "Apply a current validated drawing before export" } else { "Finish or validate the current drawing edit" }.into()),
        state: if enabled { "ready" } else { "disabled" }.into(), commands: vec![command.into()], relationships: vec![],
        data: json!({"contract":"tcad.presentation.drawing-workspace","version":1,"group":"drawing.workspace-footer","enabled":enabled,"actions":[{"command":command,"label":label,"enabled":enabled}]}),
    }}).collect()
}
pub fn footer(ui: &mut egui::Ui, w: &mut Workspace) {
    use crate::presentation_controls as controls;
    let props = controls::properties(
        "drawing.workspace-footer",
        "drawing-workspace-footer",
        &["compact"],
        "ready",
    );
    if props.get("display").is_some_and(|value| value == "none") {
        return;
    }
    ui.scope(|ui| {
        controls::apply_properties(ui, &props);
        ui.horizontal_wrapped(|ui| {
            for element in footer_semantics(w) {
                let enabled = element.data["enabled"].as_bool().unwrap_or(false);
                let response = controls::button(
                    ui,
                    &element.id,
                    &element.role,
                    &["compact"],
                    &element.label,
                    enabled,
                    false,
                );
                let response = if let Some(reason) = element.value {
                    response.on_hover_text(reason)
                } else {
                    response
                };
                if response.clicked()
                    && let Some(command) = element.commands.first()
                {
                    crate::ui::action(w, command);
                }
            }
        });
    });
}
