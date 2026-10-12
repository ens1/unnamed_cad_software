//! Compact drawing controls and the same vector sheet used by PDF/DXF export.
use crate::Workspace;
use bevy_egui::egui::{self, Color32, Stroke};
use cad_core::{Drawing, commands::Command, drawing::*};

pub(crate) fn evaluation(w: &Workspace) -> Option<&cad_core::protocol::Evaluation> {
    w.session
        .draft
        .as_ref()
        .and_then(|d| d.preview_evaluation())
        .or(w.session.evaluation.as_ref())
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
    for instance in &e.instances {
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
            for (point, p) in &geometry.points {
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
    for weld in e.assembly_welds.values() {
        let Some(mesh) = e.meshes.get(&weld.placement.mesh_key) else {
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
                        instance_path: weld.placement.path.clone(),
                    },
                    format!(
                        "{} / Bead Edge {} · {}",
                        weld.placement.name,
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
                    instance_path: weld.placement.path.clone(),
                },
                format!("{} / Bead Plane {}", weld.placement.name, index + 1),
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
    true
}
pub(crate) fn select_sheet(w: &mut Workspace, key: &str) {
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
    crate::drawing_editor_ui::show(ui, w);
}

pub fn preview(w: &Workspace, ui: &mut egui::Ui, rect: egui::Rect) -> bool {
    if w.dialog.as_deref() != Some("drawings") {
        return false;
    }
    let Some(id) = w.fields.get("drawing-preview") else {
        return false;
    };
    let Some(sheet) = w.session.visible().drawings.iter().find(|d| &d.id == id) else {
        return false;
    };
    let region = cad_core::presentation::Region {
        id: sheet.id.clone(),
        role: "drawing-sheet".into(),
        component: None,
        rect: [rect.left(), rect.top(), rect.width(), rect.height()],
        classes: vec!["paper".into(), "preview".into()],
        resizable: false,
    };
    let style = w.theme.properties(
        &region,
        &w.view.active_layout,
        if w.session.draft.as_ref().is_some_and(|draft| draft.pending) {
            "pending"
        } else {
            "ready"
        },
    );
    if style.get("display").is_some_and(|value| value == "none") {
        return true;
    }
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0., ui.visuals().faint_bg_color);
    let Some(e) = evaluation(w) else {
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "Preparing drawing geometry…",
            egui::FontId::proportional(14.),
            ui.visuals().text_color(),
        );
        return true;
    };
    match build_sheet(w.session.visible(), e, sheet) {
        Ok(sheet) => {
            let scale = ((rect.width() - 24.) / sheet.paper_mm[0] as f32)
                .min((rect.height() - 24.) / sheet.paper_mm[1] as f32);
            let size = egui::vec2(
                sheet.paper_mm[0] as f32 * scale,
                sheet.paper_mm[1] as f32 * scale,
            );
            let paper = egui::Rect::from_center_size(rect.center(), size);
            painter.rect_filled(
                paper,
                0.,
                style
                    .get("background")
                    .and_then(|value| crate::ui::color(value))
                    .unwrap_or(Color32::from_rgb(250, 249, 243)),
            );
            let at = |p: [f64; 2]| {
                egui::pos2(
                    paper.left() + p[0] as f32 * scale,
                    paper.bottom() - p[1] as f32 * scale,
                )
            };
            let stroke = Stroke::new(
                style
                    .get("stroke-width")
                    .and_then(|value| value.trim_end_matches("px").parse::<f32>().ok())
                    .filter(|value| value.is_finite() && *value > 0.)
                    .unwrap_or((0.16 * scale).max(0.7)),
                style
                    .get("color")
                    .and_then(|value| crate::ui::color(value))
                    .unwrap_or(Color32::from_rgb(25, 27, 28)),
            );
            for primitive in sheet.primitives {
                match primitive {
                    Primitive::Polyline { points, dashed } => {
                        for pair in points.windows(2) {
                            let a = at(pair[0]);
                            let b = at(pair[1]);
                            if dashed {
                                let delta = b - a;
                                let length = delta.length();
                                let mut start = 0.;
                                let mut index = 0;
                                while start < length {
                                    let end = (start
                                        + if index % 2 == 0 { 4. * scale } else { scale })
                                    .min(length);
                                    painter.line_segment(
                                        [a + delta * start / length, a + delta * end / length],
                                        stroke,
                                    );
                                    start = end + scale;
                                    index += 1;
                                }
                            } else {
                                painter.line_segment([a, b], stroke);
                            }
                        }
                    }
                    Primitive::Text { at: p, size, text } => {
                        painter.text(
                            at(p),
                            egui::Align2::LEFT_BOTTOM,
                            text,
                            egui::FontId::proportional((size as f32 * scale).max(1.)),
                            stroke.color,
                        );
                    }
                    Primitive::Spline { curve } => {
                        if let Ok(points) = display_curve(&curve) {
                            for pair in points.windows(2) {
                                painter.line_segment([at(pair[0]), at(pair[1])], stroke);
                            }
                        }
                    }
                    Primitive::Circle { center, radius } => {
                        painter.circle_stroke(at(center), radius as f32 * scale, stroke);
                    }
                }
            }
            if w.session.draft.as_ref().is_some_and(|d| d.pending) {
                painter.text(
                    rect.left_top() + egui::vec2(10., 10.),
                    egui::Align2::LEFT_TOP,
                    "Updating drawing preview…",
                    egui::FontId::proportional(12.),
                    ui.visuals().text_color(),
                );
            }
        }
        Err(error) => {
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                error.to_string(),
                egui::FontId::proportional(13.),
                ui.visuals().text_color(),
            );
        }
    }
    true
}
