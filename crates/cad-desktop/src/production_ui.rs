//! Compact, replaceable presentation of source-derived manufacturing reports.
use crate::{Workspace, presentation_controls as controls};
use bevy_egui::egui;
use cad_core::{
    CadError, Result, commands::Command, presentation::SemanticElement, production::Preferences,
};
use serde_json::{Value, json};
use std::sync::{Arc, mpsc};

const EDITOR: &str = "production-editor";
const SOURCE: &str = "production-source";
const ROOT: &str = "production-root";
const PENDING: &str = "production-pending:";
const INVALID: &str = "production-invalid:";
const CONTRACT: &str = "tcad.production-panel";

#[derive(Clone, PartialEq, Eq)]
struct ReportKey {
    document: cad_core::Id,
    epoch: cad_core::Id,
    source: cad_core::protocol::RequestIdentity,
    evaluated: bool,
}
#[derive(Default)]
struct Report {
    semantics: Vec<SemanticElement>,
    bom_headers: Vec<String>,
    bom_cells: Vec<(String, Vec<String>)>,
    bom_error: Option<String>,
    stock_headers: Vec<String>,
    stock_cells: Vec<(String, Vec<String>)>,
    stock_error: Option<String>,
    evaluated: bool,
}
struct ReportJob {
    key: ReportKey,
    document: cad_core::Document,
    evaluation: Option<cad_core::protocol::Evaluation>,
}
struct ReportWorker {
    sender: mpsc::SyncSender<ReportJob>,
    receiver: mpsc::Receiver<(ReportKey, Arc<Report>)>,
}
impl ReportWorker {
    fn start() -> std::io::Result<Self> {
        let (sender, input) = mpsc::sync_channel::<ReportJob>(1);
        let (output, receiver) = mpsc::channel();
        std::thread::Builder::new()
            .name("tcad-production-report".into())
            .spawn(move || {
                while let Ok(job) = input.recv() {
                    let report = build_report(&job);
                    if output.send((job.key, Arc::new(report))).is_err() {
                        break;
                    }
                    crate::redraw::wake();
                }
            })?;
        Ok(Self { sender, receiver })
    }
}
/// A single source-derived report supplies every native/custom presentation.
/// No report cache participates in mutation or Apply validation.
#[derive(Default)]
pub struct Cache {
    worker: Option<ReportWorker>,
    in_flight: Option<ReportKey>,
    current: Option<(ReportKey, Arc<Report>)>,
}

fn invalid(message: &str) -> CadError {
    CadError::Invalid(message.into())
}
fn editing(w: &Workspace) -> bool {
    w.fields.contains_key(EDITOR)
}
fn available(w: &Workspace) -> bool {
    w.settings.is_none()
        && w.sketch.is_none()
        && (editing(w) || w.tool_checkpoint.is_none() && w.session.draft.is_none())
}
fn evaluation(w: &Workspace) -> Option<&cad_core::protocol::Evaluation> {
    w.session
        .draft
        .as_ref()
        .and_then(|draft| draft.preview_evaluation())
        .or(w.session.evaluation.as_ref())
        .filter(|evaluation| {
            evaluation.valid && evaluation.identity == crate::source_cache::identity(w)
        })
}
pub fn reset(w: &mut Workspace) {
    w.fields.remove(EDITOR);
    w.fields.remove(SOURCE);
    w.fields.remove(ROOT);
    w.fields
        .retain(|key, _| !key.starts_with(PENDING) && !key.starts_with(INVALID));
}
fn structural_current(w: &Workspace) -> bool {
    !editing(w)
        || (w.fields.get(ROOT) == Some(&w.session.visible().root)
            && w.fields.get(SOURCE).is_some_and(|source| {
                serde_json::from_str::<Preferences>(source)
                    .is_ok_and(|source| source == w.session.visible().settings.production)
            })
            && w.session.visible().settings.production.validate().is_ok())
}
pub fn current_source_is_valid(w: &Workspace) -> bool {
    structural_current(w) && !w.fields.keys().any(|key| key.starts_with(INVALID))
}
pub fn field_error<'a>(w: &'a Workspace, element: &SemanticElement) -> Option<&'a str> {
    w.fields
        .get(&format!("{INVALID}{}", element.id))
        .map(String::as_str)
}
pub fn field_is_invalid(w: &Workspace, element: &SemanticElement) -> bool {
    field_error(w, element).is_some()
}
pub fn record_field_error(w: &mut Workspace, element: &SemanticElement, error: Option<String>) {
    if !editing(w)
        || element.role != "production-field"
        || element.data["editor"] != json!(w.fields.get(EDITOR))
        || element.data["document"] != w.session.visible().id
    {
        return;
    }
    let key = format!("{INVALID}{}", element.id);
    if let Some(error) = error {
        if semantics(w).iter().any(|current| current.id == element.id) {
            w.fields.insert(key, error);
        }
    } else {
        w.fields.remove(&key);
    }
}
pub fn begin(w: &mut Workspace) -> Result<()> {
    if editing(w) {
        return Ok(());
    }
    if !available(w) {
        return Err(invalid(
            "Apply or cancel the current operation before editing production preferences",
        ));
    }
    w.start_tool();
    w.fields.insert(EDITOR.into(), cad_core::id());
    w.fields
        .insert(ROOT.into(), w.session.visible().root.clone());
    w.fields.insert(
        SOURCE.into(),
        serde_json::to_string(&w.session.visible().settings.production)?,
    );
    w.open_editor("bom");
    Ok(())
}
fn submit(w: &mut Workspace, preferences: Preferences) -> Result<()> {
    if !structural_current(w) {
        return Err(invalid(
            "Production preferences changed outside this editor; cancel and reopen it",
        ));
    }
    preferences.validate()?;
    let mut settings = w.session.visible().settings.clone();
    settings.production = preferences.clone();
    settings.validate(w.session.visible())?;
    let mut candidate = w.session.visible().clone();
    Command::SetModelSettings {
        settings: settings.clone(),
    }
    .execute(&mut candidate)?;
    w.dispatch(Command::SetModelSettings { settings });
    if w.session.visible().settings.production != preferences {
        return Err(invalid("Production preference update did not stage"));
    }
    w.fields
        .insert(SOURCE.into(), serde_json::to_string(&preferences)?);
    let live: std::collections::BTreeSet<_> = semantics(w)
        .into_iter()
        .filter(|element| element.role == "production-field")
        .map(|element| element.id)
        .collect();
    w.fields.retain(|key, _| {
        key.strip_prefix(PENDING)
            .or_else(|| key.strip_prefix(INVALID))
            .is_none_or(|field| live.contains(field))
    });
    Ok(())
}
fn choice(label: impl Into<String>, value: impl serde::Serialize) -> Value {
    json!({"label":label.into(),"value":value})
}
fn field(
    w: &Workspace,
    pointer: &str,
    label: &str,
    choices: Vec<Value>,
    range: Option<[f64; 2]>,
) -> SemanticElement {
    let source = json!(w.session.visible().settings.production);
    let id = format!(
        "production:field:{}",
        pointer.trim_start_matches('/').replace('/', ":")
    );
    let source_value = source.pointer(pointer).cloned().unwrap_or_default();
    let value = w
        .fields
        .get(&format!("{PENDING}{id}"))
        .and_then(|value| serde_json::from_str::<Value>(value).ok())
        .unwrap_or_else(|| source_value.clone());
    let error = w.fields.get(&format!("{INVALID}{id}"));
    let editable = available(w) && editing(w) && structural_current(w) && !w.confirm_requested;
    SemanticElement {
        id: format!(
            "production:field:{}",
            pointer.trim_start_matches('/').replace('/', ":")
        ),
        role: "production-field".into(),
        label: label.into(),
        value: Some(
            value
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| value.to_string()),
        ),
        state: if error.is_some() {
            "invalid"
        } else if editable {
            "editable"
        } else {
            "readonly"
        }
        .into(),
        commands: vec!["production.edit".into()],
        relationships: vec![w.session.visible().root.clone()],
        data: json!({"contract":CONTRACT,"version":1,"setter":"production","pointer":pointer,"json":value,"source_json":source_value,"error":error,"choices":choices,"minimum":range.map(|range|range[0]),"maximum":range.map(|range|range[1]),"editable":editable,"editor":w.fields.get(EDITOR),"document":w.session.visible().id,"root":w.session.visible().root,"source":source,"kind":if !choices.is_empty(){"choice"}else if value.is_number(){"number"}else{"text"}}),
    }
}
fn action(
    w: &Workspace,
    key: &str,
    label: &str,
    mutation: Value,
    enabled: bool,
) -> SemanticElement {
    SemanticElement {
        id: format!("production:action:{key}"),
        role: "production-action".into(),
        label: label.into(),
        value: None,
        state: if enabled { "available" } else { "disabled" }.into(),
        commands: vec!["production.action".into()],
        relationships: vec![w.session.visible().root.clone()],
        data: json!({"contract":CONTRACT,"version":1,"mutation":mutation,"enabled":enabled,"editor":w.fields.get(EDITOR),"document":w.session.visible().id,"root":w.session.visible().root,"source":w.session.visible().settings.production}),
    }
}
pub fn semantics(w: &Workspace) -> Vec<SemanticElement> {
    use cad_core::{bom, cut_list};
    let mut out = vec![action(
        w,
        "edit",
        "Edit production preferences",
        json!({"kind":"edit"}),
        available(w) && !editing(w),
    )];
    out.push(field(
        w,
        "/bom/mode",
        "BOM structure",
        vec![
            choice("Parts only", bom::Mode::PartsOnly),
            choice("Top level", bom::Mode::TopLevel),
            choice("Indented hierarchy", bom::Mode::Indented),
        ],
        None,
    ));
    out.push(field(
        w,
        "/bom/grouping",
        "Component grouping",
        vec![
            choice(
                "Definition and configuration",
                bom::Grouping::DefinitionConfiguration,
            ),
            choice(
                "Include finish",
                bom::Grouping::DefinitionConfigurationFinish,
            ),
        ],
        None,
    ));
    out.push(field(
        w,
        "/cut_list/grouping",
        "Stock grouping",
        vec![
            choice("Individual source members", cut_list::Grouping::BySource),
            choice(
                "Equivalent production stock",
                cut_list::Grouping::EquivalentStock,
            ),
        ],
        None,
    ));
    out.push(field(
        w,
        "/units",
        "Cut-list units",
        vec![
            choice("Millimeters", cad_core::drawing::Units::Millimeters),
            choice("Inches", cad_core::drawing::Units::Inches),
        ],
        None,
    ));
    out.push(field(
        w,
        "/precision",
        "Cut-list decimals",
        vec![],
        Some([0., 8.]),
    ));
    let editable = editing(w) && available(w) && current_source_is_valid(w) && !w.confirm_requested;
    let bom_columns = bom::column_choices();
    for (index, column) in w
        .session
        .visible()
        .settings
        .production
        .bom
        .columns
        .iter()
        .enumerate()
    {
        out.push(field(
            w,
            &format!("/bom/columns/{index}"),
            &format!("BOM column {}", index + 1),
            bom_columns
                .iter()
                .map(|column| choice(column.label(), column))
                .collect(),
            None,
        ));
        if matches!(column, bom::Column::Property { .. }) {
            out.push(field(
                w,
                &format!("/bom/columns/{index}/key"),
                "Property key",
                vec![],
                None,
            ));
            out.push(field(
                w,
                &format!("/bom/columns/{index}/label"),
                "Column heading",
                vec![],
                None,
            ));
        }
        column_actions(
            w,
            &mut out,
            "bom",
            index,
            w.session.visible().settings.production.bom.columns.len(),
            editable,
        );
    }
    out.push(action(
        w,
        "bom/column/add",
        "Add BOM column",
        json!({"kind":"column-add","scope":"bom"}),
        editable && w.session.visible().settings.production.bom.columns.len() < 32,
    ));
    let stock_columns = cut_list::column_choices();
    for (index, _) in w
        .session
        .visible()
        .settings
        .production
        .cut_list
        .columns
        .iter()
        .enumerate()
    {
        out.push(field(
            w,
            &format!("/cut_list/columns/{index}"),
            &format!("Stock column {}", index + 1),
            stock_columns
                .iter()
                .map(|column| choice(column.label(), column))
                .collect(),
            None,
        ));
        column_actions(
            w,
            &mut out,
            "cut_list",
            index,
            w.session
                .visible()
                .settings
                .production
                .cut_list
                .columns
                .len(),
            editable,
        );
    }
    out.push(action(
        w,
        "cut_list/column/add",
        "Add stock column",
        json!({"kind":"column-add","scope":"cut_list"}),
        editable
            && w.session
                .visible()
                .settings
                .production
                .cut_list
                .columns
                .len()
                < 32,
    ));
    out.push(action(
        w,
        "apply",
        "Apply preferences",
        json!({"kind":"apply"}),
        editable && w.session.draft.is_some(),
    ));
    out.push(action(
        w,
        "cancel",
        "Cancel preferences",
        json!({"kind":"cancel"}),
        editing(w),
    ));
    let exportable =
        w.session.draft.is_none() && w.tool_checkpoint.is_none() && evaluation(w).is_some();
    for (key, label, format) in [
        ("bom", "Export BOM CSV", "bom"),
        ("cutlist", "Export stock CSV", "cutlist"),
    ] {
        out.push(action(
            w,
            &format!("export/{key}"),
            label,
            json!({"kind":"export","format":format}),
            exportable,
        ));
    }
    for sheet in &w.session.visible().drawings {
        for table in &sheet.tables {
            out.push(action(w,&format!("export/table/{}/{}",sheet.id,table.id),&format!("Export {} / {} CSV",sheet.name,table.title),json!({"kind":"export","format":cad_core::production::table_export_format(&sheet.id,&table.id)}),exportable));
        }
    }
    out
}
fn column_actions(
    w: &Workspace,
    out: &mut Vec<SemanticElement>,
    scope: &str,
    index: usize,
    count: usize,
    editable: bool,
) {
    for (direction, label, enabled) in [
        (-1, "Move column left", index > 0),
        (1, "Move column right", index + 1 < count),
    ] {
        out.push(action(
            w,
            &format!("{scope}/{index}/move/{direction}"),
            label,
            json!({"kind":"column-move","scope":scope,"index":index,"direction":direction}),
            editable && enabled,
        ));
    }
    out.push(action(
        w,
        &format!("{scope}/{index}/remove"),
        "Remove column",
        json!({"kind":"column-remove","scope":scope,"index":index}),
        editable && count > 1,
    ));
}
fn guarded(w: &Workspace, element: &SemanticElement) -> Result<SemanticElement> {
    let current = semantics(w)
        .into_iter()
        .find(|current| current.id == element.id)
        .ok_or_else(|| invalid("Production control is no longer available"))?;
    if current.data != element.data || current.state != element.state {
        return Err(invalid(
            "This production control belongs to an earlier source; use the current control",
        ));
    }
    Ok(current)
}
pub fn field_value(w: &Workspace, element: &SemanticElement) -> Option<Value> {
    if element.role != "production-field"
        || element.data["contract"] != CONTRACT
        || element.data["document"] != w.session.visible().id
        || element.data["root"] != w.session.visible().root
        || element.data["editor"] != json!(w.fields.get(EDITOR))
    {
        return None;
    }
    let pointer = element.data.get("pointer")?.as_str()?;
    let id = format!(
        "production:field:{}",
        pointer.trim_start_matches('/').replace('/', ":")
    );
    if element.id != id {
        return None;
    }
    w.fields
        .get(&format!("{PENDING}{id}"))
        .and_then(|value| serde_json::from_str(value).ok())
        .or_else(|| {
            json!(w.session.visible().settings.production)
                .pointer(pointer)
                .cloned()
        })
}

pub fn set_field(w: &mut Workspace, element: &SemanticElement, value: Value) -> Result<()> {
    let current = guarded(w, element)?;
    if current.data["editable"] != true {
        return Err(invalid("Start editing production preferences first"));
    }
    let pointer = current.data["pointer"]
        .as_str()
        .ok_or_else(|| invalid("Missing production field address"))?;
    let pending = format!("{PENDING}{}", current.id);
    let diagnostic = format!("{INVALID}{}", current.id);
    w.fields
        .insert(pending.clone(), serde_json::to_string(&value)?);
    let result = (|| -> Result<()> {
        if let Some(choices) = current.data["choices"]
            .as_array()
            .filter(|choices| !choices.is_empty())
            && !choices.iter().any(|choice| choice["value"] == value)
        {
            return Err(invalid("Choose a supported production option"));
        }
        let mut source = json!(w.session.visible().settings.production);
        *source
            .pointer_mut(pointer)
            .ok_or_else(|| invalid("Production field was removed"))? = value;
        submit(w, serde_json::from_value(source)?)
    })();
    match &result {
        Ok(()) => {
            w.fields.remove(&pending);
            w.fields.remove(&diagnostic);
        }
        Err(error) => {
            w.fields.insert(diagnostic, error.to_string());
        }
    }
    result
}
pub fn dispatch(w: &mut Workspace, element: &SemanticElement) -> Result<()> {
    let current = guarded(w, element)?;
    if current.data["enabled"] != true {
        return Err(invalid(
            "Production action is unavailable in the current operation",
        ));
    }
    let mutation = &current.data["mutation"];
    match mutation["kind"].as_str().unwrap_or("") {
        "edit" => begin(w),
        "apply" => {
            w.confirm_tool();
            Ok(())
        }
        "cancel" => {
            reset(w);
            w.cancel_tool();
            Ok(())
        }
        "export" => export(
            w,
            mutation["format"]
                .as_str()
                .ok_or_else(|| invalid("Choose an export format"))?,
        ),
        kind => {
            let mut preferences = w.session.visible().settings.production.clone();
            let index = mutation["index"].as_u64().unwrap_or(u64::MAX) as usize;
            let direction = mutation["direction"].as_i64().unwrap_or(0);
            fn edit<T>(
                columns: &mut Vec<T>,
                kind: &str,
                index: usize,
                direction: i64,
                new: T,
            ) -> Result<()> {
                match kind {
                    "column-add" if columns.len() < 32 => columns.push(new),
                    "column-remove" if columns.len() > 1 && index < columns.len() => {
                        columns.remove(index);
                    }
                    "column-move" if index < columns.len() => {
                        let target = (index as i64 + direction)
                            .try_into()
                            .ok()
                            .filter(|target: &usize| *target < columns.len())
                            .ok_or_else(|| invalid("Column is already at this end"))?;
                        columns.swap(index, target);
                    }
                    _ => return Err(invalid("Column no longer exists")),
                }
                Ok(())
            }
            match mutation["scope"].as_str() {
                Some("bom") => edit(
                    &mut preferences.bom.columns,
                    kind,
                    index,
                    direction,
                    cad_core::bom::Column::Finish,
                )?,
                Some("cut_list") => edit(
                    &mut preferences.cut_list.columns,
                    kind,
                    index,
                    direction,
                    cad_core::cut_list::Column::Finish,
                )?,
                _ => return Err(invalid("Unknown production column scope")),
            }
            submit(w, preferences)
        }
    }
}
pub fn export(w: &mut Workspace, format: &str) -> Result<()> {
    if w.session.draft.is_some() || w.tool_checkpoint.is_some() || evaluation(w).is_none() {
        return Err(invalid(
            "Apply the current model and production preferences before exporting",
        ));
    }
    if let Some(path) = rfd::FileDialog::new()
        .add_filter("Manufacturing CSV", &["csv"])
        .set_file_name(if format == "bom" {
            "bom.csv"
        } else if format == "cutlist" {
            "cut-list.csv"
        } else {
            "drawing-table.csv"
        })
        .save_file()
    {
        w.exporter
            .submit(cad_core::protocol::WorkerRequest::Export {
                identity: w.session.identity(),
                document: w.session.committed.clone(),
                project_root: w.root().to_string_lossy().into(),
                path: path.to_string_lossy().into(),
                format: format.into(),
                display_tolerance_mm: w.display_tolerance(),
            });
        w.status = "Exporting the committed production source".into();
    }
    Ok(())
}

pub fn semantic_control(ui: &mut egui::Ui, w: &mut Workspace, element: &SemanticElement) {
    if element.role == "production-action" {
        if controls::button(
            ui,
            &element.id,
            &element.role,
            &["semantic-control"],
            &element.label,
            element.state == "available",
            false,
        )
        .clicked()
            && let Err(error) = dispatch(w, element)
        {
            w.status = error.to_string();
        }
        return;
    }
    let Some(current) = field_value(w, element) else {
        return;
    };
    let mut value = current.clone();
    let editable = element.data["editable"].as_bool().unwrap_or(false);
    let properties = controls::properties(
        &element.id,
        &element.role,
        &["semantic-control"],
        &element.state,
    );
    if properties
        .get("display")
        .is_some_and(|value| value == "none")
    {
        return;
    }
    let changed = ui
        .push_id(&element.id, |ui| {
            ui.scope(|ui| {
                controls::apply_properties(ui, &properties);
                let size = controls::field_size(ui, &properties);
                ui.add_enabled_ui(editable, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        controls::label(
                            ui,
                            &format!("{}.label", element.id),
                            "production-field-label",
                            &[],
                            &element.label,
                            "ready",
                        );
                        if let Some(choices) = element.data["choices"]
                            .as_array()
                            .filter(|choices| !choices.is_empty())
                        {
                            let label = choices
                                .iter()
                                .find(|choice| choice["value"] == value)
                                .and_then(|choice| choice["label"].as_str())
                                .unwrap_or("Choose…");
                            let mut changed = false;
                            egui::ComboBox::from_id_salt(&element.id)
                                .width(size.x)
                                .selected_text(label)
                                .show_ui(ui, |ui| {
                                    for choice in choices {
                                        if let Some(label) = choice["label"].as_str() {
                                            changed |= ui
                                                .selectable_value(
                                                    &mut value,
                                                    choice["value"].clone(),
                                                    label,
                                                )
                                                .changed();
                                        }
                                    }
                                });
                            changed
                        } else if let Some(number) = value.as_u64() {
                            let mut candidate = number;
                            let minimum = element.data["minimum"].as_f64().unwrap_or(0.) as u64;
                            let maximum =
                                element.data["maximum"].as_f64().unwrap_or(1_000_000.) as u64;
                            let changed = ui
                                .add_sized(
                                    size,
                                    egui::DragValue::new(&mut candidate).range(minimum..=maximum),
                                )
                                .changed();
                            value = json!(candidate);
                            changed
                        } else {
                            let mut candidate = value.as_str().unwrap_or("").to_string();
                            let changed = ui
                                .add_sized(size, egui::TextEdit::singleline(&mut candidate))
                                .changed();
                            value = json!(candidate);
                            changed
                        }
                    })
                    .inner
                })
                .inner
            })
            .inner
        })
        .inner;
    if changed
        && value != current
        && let Err(error) = set_field(w, element, value)
    {
        w.status = error.to_string();
    }
    if let Some(error) = field_error(w, element) {
        controls::label(
            ui,
            &format!("{}.diagnostic", element.id),
            "production-error",
            &[],
            error,
            "invalid",
        );
    }
}

fn build_report(job: &ReportJob) -> Report {
    let doc = &job.document;
    let preferences = &doc.settings.production;
    let evaluation = job.evaluation.as_ref();
    let mut report = Report {
        bom_headers: preferences
            .bom
            .columns
            .iter()
            .map(|column| column.label().to_owned())
            .collect(),
        stock_headers: preferences
            .cut_list
            .columns
            .iter()
            .map(|column| column.heading(preferences.units))
            .collect(),
        evaluated: evaluation.is_some(),
        ..Default::default()
    };
    // One exact source identity, reused for every row in this snapshot.
    let source = &job.key.source.source;
    match cad_core::bom::rows_with_placements(
        doc,
        &preferences.bom,
        evaluation.map(|evaluation| &evaluation.assemblies),
    ) {
        Ok(rows) => {
            for (index, row) in rows.iter().enumerate() {
                let id = format!("production:bom:{}", row.key);
                let mut cells =
                    cad_core::bom::cells(row, &preferences.bom.columns, (index + 1) as u32);
                report.semantics.push(SemanticElement {
                    id: id.clone(), role: "production-bom-row".into(), label: row.part_number.clone(), value: Some(cells.join(" · ")), state: "ready".into(), commands: vec![], relationships: vec![doc.root.clone(), row.definition.clone()],
                    data: json!({"contract":"tcad.bom","version":cad_core::bom::CONTRACT_VERSION,"row":row,"columns":preferences.bom.columns,"document":doc.id,"source":source,"quantity_basis":"total_current_configured_occurrences"}),
                });
                if preferences.bom.mode == cad_core::bom::Mode::Indented
                    && let Some(column) = preferences.bom.columns.iter().position(|column| {
                        matches!(
                            column,
                            cad_core::bom::Column::PartNumber | cad_core::bom::Column::Description
                        )
                    })
                {
                    cells[column] = format!("{}{}", "  ".repeat(row.depth.min(256)), cells[column]);
                }
                report.bom_cells.push((id, cells));
            }
        }
        Err(error) => report.bom_error = Some(error.to_string()),
    }
    if let Some(evaluation) = evaluation {
        match cad_core::cut_list::rows(doc, evaluation, &preferences.cut_list) {
            Ok(rows) => {
                for (index, row) in rows.iter().enumerate() {
                    let id = format!("production:stock:{}", row.key);
                    let cells = cad_core::cut_list::cells(
                        row,
                        &preferences.cut_list.columns,
                        (index + 1) as u32,
                        preferences.units,
                        preferences.precision,
                    );
                    report.semantics.push(SemanticElement {
                        id: id.clone(), role: "production-stock-row".into(), label: row.row.description.clone(), value: Some(cells.join(" · ")), state: "ready".into(), commands: vec![], relationships: vec![doc.root.clone(), row.row.definition.clone()].into_iter().chain(row.sources.iter().map(|source| source.member.group.node.clone())).collect(),
                        data: json!({"contract":"tcad.cut-list","version":cad_core::cut_list::CONTRACT_VERSION,"row":row,"columns":preferences.cut_list.columns,"document":doc.id,"source":source,"geometry_basis":"native_manufacturing_metadata"}),
                    });
                    report.stock_cells.push((id, cells));
                }
            }
            Err(error) => report.stock_error = Some(error.to_string()),
        }
    }
    report
}
fn report(w: &Workspace) -> Option<Arc<Report>> {
    let evaluation = evaluation(w);
    let key = ReportKey {
        document: w.session.visible().id.clone(),
        epoch: w.session.source_epoch().into(),
        source: crate::source_cache::identity(w),
        evaluated: evaluation.is_some(),
    };
    let mut cache = w.production_cache.lock().ok()?;
    if let Some((current, report)) = &cache.current
        && current == &key
    {
        return Some(report.clone());
    }
    if cache.worker.is_none() {
        match ReportWorker::start() {
            Ok(worker) => cache.worker = Some(worker),
            Err(error) => {
                let report = Arc::new(Report {
                    bom_error: Some(format!(
                        "Could not start manufacturing report worker: {error}"
                    )),
                    ..Default::default()
                });
                cache.current = Some((key, report.clone()));
                return Some(report);
            }
        }
    }
    loop {
        match cache.worker.as_ref()?.receiver.try_recv() {
            Ok((completed, report)) => {
                if cache.in_flight.as_ref() == Some(&completed) {
                    cache.in_flight = None;
                }
                if completed == key {
                    cache.current = Some((completed, report));
                }
            }
            Err(mpsc::TryRecvError::Empty) => break,
            Err(mpsc::TryRecvError::Disconnected) => {
                cache.worker = None;
                cache.in_flight = None;
                let report = Arc::new(Report {
                    bom_error: Some(
                        "The manufacturing report worker stopped; update the model or reopen the project to retry."
                            .into(),
                    ),
                    ..Default::default()
                });
                cache.current = Some((key, report.clone()));
                return Some(report);
            }
        }
    }
    if let Some((current, report)) = &cache.current
        && current == &key
    {
        return Some(report.clone());
    }
    // One job at a time; intervening edits are coalesced into the newest source.
    // Never show rows derived from a previous draft, configuration or evaluation.
    cache.current = None;
    if cache.in_flight.is_none() {
        let job = ReportJob {
            key: key.clone(),
            document: w.session.visible().clone(),
            evaluation: evaluation.cloned(),
        };
        if cache.worker.as_ref()?.sender.try_send(job).is_ok() {
            cache.in_flight = Some(key);
        }
    }
    None
}

pub fn row_semantics(w: &Workspace) -> Vec<SemanticElement> {
    if let Some(report) = report(w) {
        let mut elements = report.semantics.clone();
        for (scope, error) in [("bom", &report.bom_error), ("stock", &report.stock_error)] {
            if let Some(error) = error {
                elements.push(SemanticElement {
                    id: format!("production:{scope}:error"), role: "production-error".into(),
                    label: error.clone(), value: None, state: "invalid".into(),
                    commands: vec![], relationships: vec![w.session.visible().root.clone()],
                    data: json!({"contract":CONTRACT,"version":1,"document":w.session.visible().id,"source":crate::source_cache::identity(w).source}),
                });
            }
        }
        return elements;
    }
    vec![SemanticElement {
        id: "production:report-status".into(),
        role: "production-report-status".into(),
        label: "Updating manufacturing reports…".into(),
        value: None,
        state: "pending".into(),
        commands: vec![],
        relationships: vec![w.session.visible().root.clone()],
        data: json!({"contract":CONTRACT,"version":1,"document":w.session.visible().id,"source":crate::source_cache::identity(w).source}),
    }]
}

pub fn show(ui: &mut egui::Ui, w: &mut Workspace) {
    let elements = semantics(w);
    ui.horizontal_wrapped(|ui| {
        for element in elements.iter().filter(|element| {
            matches!(
                element.data["mutation"]["kind"].as_str(),
                Some("edit" | "apply" | "cancel" | "export")
            )
        }) {
            semantic_control(ui, w, element);
        }
    });
    for element in elements.iter().filter(|element| {
        element.role == "production-field"
            && !element.data["pointer"]
                .as_str()
                .is_some_and(|pointer| pointer.contains("/columns/"))
    }) {
        semantic_control(ui, w, element);
    }
    for (scope, label) in [("bom", "BOM columns"), ("cut_list", "Cut-list columns")] {
        egui::CollapsingHeader::new(label)
            .id_salt(format!("production-columns-{scope}"))
            .show(ui, |ui| {
                for element in elements.iter().filter(|element| {
                    element.data["pointer"]
                        .as_str()
                        .is_some_and(|pointer| pointer.starts_with(&format!("/{scope}/columns/")))
                        || element.data["mutation"]["scope"] == scope
                }) {
                    semantic_control(ui, w, element);
                }
            });
    }
    let Some(report) = report(w) else {
        controls::label(
            ui,
            "production:report-status",
            "production-report-status",
            &[],
            "Updating manufacturing reports…",
            "pending",
        );
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(50));
        return;
    };
    ui.separator();
    controls::label(
        ui,
        "production:bom:heading",
        "production-table-heading",
        &[],
        "Bill of materials",
        "ready",
    );
    if let Some(error) = &report.bom_error {
        controls::label(
            ui,
            "production:bom:error",
            "production-error",
            &[],
            error,
            "invalid",
        );
    } else {
        table(ui, "production:bom", &report.bom_headers, &report.bom_cells);
    }
    ui.separator();
    controls::label(
        ui,
        "production:stock:heading",
        "production-table-heading",
        &[],
        "Weldment cut list",
        "ready",
    );
    if let Some(error) = &report.stock_error {
        controls::label(
            ui,
            "production:stock:error",
            "production-error",
            &[],
            error,
            "invalid",
        );
    } else if report.evaluated {
        table(
            ui,
            "production:stock",
            &report.stock_headers,
            &report.stock_cells,
        );
    } else {
        ui.small("Wait for current evaluated fabrication metadata.");
    }
}
fn table(ui: &mut egui::Ui, id: &str, headers: &[String], rows: &[(String, Vec<String>)]) {
    let properties = controls::properties(id, "production-table", &[], "ready");
    if properties
        .get("display")
        .is_some_and(|value| value == "none")
    {
        return;
    }
    ui.scope(|ui| {
        controls::apply_properties(ui, &properties);
        egui::ScrollArea::both()
            .id_salt(id)
            .max_height(260.)
            .show(ui, |ui| {
                egui::Grid::new(id)
                    .striped(true)
                    .min_col_width(controls::number(&properties, "column-width", 65.))
                    .show(ui, |ui| {
                        for (index, heading) in headers.iter().enumerate() {
                            controls::label(
                                ui,
                                &format!("{id}:heading:{index}"),
                                "production-column-heading",
                                &[],
                                heading,
                                "ready",
                            );
                        }
                        ui.end_row();
                        for (row_id, cells) in rows {
                            for (index, cell) in cells.iter().enumerate() {
                                controls::label(
                                    ui,
                                    &format!("{row_id}:cell:{index}"),
                                    "production-cell",
                                    &[],
                                    cell,
                                    "ready",
                                );
                            }
                            ui.end_row();
                        }
                    });
            });
    });
}
