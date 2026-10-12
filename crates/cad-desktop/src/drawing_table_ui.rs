//! Docked controls over saved drawing tables and associative BOM row references.
use crate::Workspace;
use cad_core::{
    Drawing,
    drawing::{Anchor, AnnotationKind},
    drawing_tables::*,
};

pub(crate) fn evaluation(w: &Workspace) -> Option<&cad_core::protocol::Evaluation> {
    w.session
        .draft
        .as_ref()
        .and_then(|d| d.preview_evaluation())
        .or(w.session.evaluation.as_ref())
}
pub(crate) fn scoped(w: &Workspace, sheet: &Drawing) -> Option<cad_core::Document> {
    evaluation(w).map(|e| cad_core::drawing::scoped_document(w.session.visible(), e, sheet))
}
pub fn default_balloon(
    w: &Workspace,
    sheet: &Drawing,
    target: Anchor,
) -> cad_core::Result<AnnotationKind> {
    let table = sheet
        .tables
        .iter()
        .find(|t| matches!(t.kind, TableKind::BillOfMaterials))
        .ok_or_else(|| cad_core::CadError::Invalid("Add a BOM table to this sheet first".into()))?;
    let evaluation = evaluation(w).ok_or_else(|| {
        cad_core::CadError::Invalid("Wait for the current model evaluation".into())
    })?;
    let doc = cad_core::drawing::scoped_document(w.session.visible(), evaluation, sheet);
    let rows = content(&doc, evaluation, table)?.items;
    let row = target
        .reference
        .as_ref()
        .and_then(|reference| {
            rows.iter()
                .find(|row| cad_core::drawing_tables::reference_matches_item(&doc, reference, row))
        })
        .or_else(|| {
            w.selected.as_ref().and_then(|selected| {
                rows.iter().find(|row| {
                    row.definition == selected.definition
                        && row
                            .occurrences
                            .iter()
                            .any(|occurrence| occurrence.path == selected.path)
                })
            })
        })
        .or_else(|| rows.first())
        .ok_or_else(|| {
            cad_core::CadError::Invalid("This drawing scope has no included BOM items".into())
        })?;
    Ok(AnnotationKind::Balloon {
        target,
        table: table.id.clone(),
        row_key: Some(row.key.clone()),
        definition: row.definition.clone(),
        configuration: row.configuration.clone(),
        show_quantity: false,
    })
}
pub(crate) fn represented_parts(w: &Workspace, sheet: &Drawing) -> Vec<String> {
    let Some(doc) = scoped(w, sheet) else {
        return vec![];
    };
    doc.placed_parts()
        .unwrap_or_default()
        .into_iter()
        .map(|i| i.definition)
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect()
}
pub(crate) fn hole_candidates(w: &Workspace, sheet: &Drawing) -> Vec<(cad_core::NodeRef, String)> {
    let represented = represented_parts(w, sheet);
    w.session
        .visible()
        .parts
        .values()
        .filter(|p| represented.contains(&p.id))
        .flat_map(|p| {
            p.features
                .iter()
                .filter(|f| {
                    matches!(
                        f.operation,
                        cad_core::Operation::Hole { .. } | cad_core::Operation::DrilledHoles { .. }
                    )
                })
                .map(|f| (p.reference(&f.id), format!("{} / {}", p.name, f.name)))
        })
        .collect()
}

/// Author one native table with stable source reservations. Widgets and custom
/// semantic actions use the same factory, without carrying evaluated rows.
pub(crate) fn create(w: &Workspace, sheet: &Drawing, kind: &str) -> cad_core::Result<Table> {
    let invalid = |message: &str| cad_core::CadError::Invalid(message.into());
    let kind = match kind {
        "bill_of_materials" => TableKind::BillOfMaterials,
        "revisions" => TableKind::Revisions { rows: vec![] },
        "general" => TableKind::General {
            headers: vec!["Property".into(), "Value".into()],
            rows: vec![],
        },
        "bends" => {
            let source = w
                .session
                .visible()
                .parts
                .values()
                .flat_map(|p| {
                    p.features
                        .iter()
                        .filter(|f| {
                            p.outputs.contains(&f.id)
                                && !f.suppressed
                                && matches!(f.operation, cad_core::Operation::Sheet { .. })
                        })
                        .map(|f| p.reference(&f.id))
                })
                .next()
                .ok_or_else(|| invalid("Create sketch-based sheet stock first"))?;
            TableKind::Bends {
                source,
                configuration: None,
            }
        }
        "holes" => {
            let candidates = hole_candidates(w, sheet);
            let source = w
                .selected
                .as_ref()
                .and_then(|s| candidates.iter().find(|(f, _)| f == &s.reference.producer))
                .or_else(|| candidates.first())
                .map(|(source, _)| source.clone())
                .ok_or_else(|| {
                    invalid("Create a designed hole feature in this sheet scope first")
                })?;
            let mut tag = "A".to_string();
            let mut index = 1;
            while sheet
                .tables
                .iter()
                .any(|t| matches!(&t.kind,TableKind::Holes{tag_prefix,..}if tag_prefix==&tag))
            {
                index += 1;
                tag = format!("H{index}_");
            }
            TableKind::Holes {
                tag_prefix: tag,
                features: vec![source],
                configuration: None,
                instance_path: vec![],
                view: cad_core::drawing::view_keys(sheet)
                    .into_iter()
                    .find(|v| matches!(v.as_str(), "front" | "top" | "right"))
                    .unwrap_or("front".into()),
                datum: HoleDatum::ModelOrigin,
                units: cad_core::drawing::Units::Millimeters,
                precision: 3,
            }
        }
        "weldment_cut_list" => TableKind::WeldmentCutList {
            scope: sheet.definition.clone(),
            configuration: None,
            sources: vec![],
            units: cad_core::drawing::Units::Millimeters,
            precision: 3,
        },
        _ => return Err(invalid("Unsupported drawing table type")),
    };
    let mut table = Table::new(kind);
    let preferences = &w.session.visible().settings.production;
    if matches!(table.kind, TableKind::BillOfMaterials) {
        table.bom_options = preferences.bom.clone();
        table.column_widths_mm = table
            .bom_options
            .columns
            .iter()
            .map(|column| match column {
                cad_core::bom::Column::Item
                | cad_core::bom::Column::Quantity
                | cad_core::bom::Column::Depth => 12.,
                cad_core::bom::Column::Description => 65.,
                _ => 35.,
            })
            .collect();
    }
    if let TableKind::WeldmentCutList {
        units, precision, ..
    } = &mut table.kind
    {
        table.cut_list_options = preferences.cut_list.clone();
        *units = preferences.units;
        *precision = preferences.precision;
        table.column_widths_mm = table
            .cut_list_options
            .columns
            .iter()
            .map(|column| match column {
                cad_core::cut_list::Column::Item | cad_core::cut_list::Column::Quantity => 12.,
                cad_core::cut_list::Column::Description => 42.,
                cad_core::cut_list::Column::Section => 30.,
                _ => 25.,
            })
            .collect();
    }
    table.position_mm = cad_core::drawing::default_table_position(sheet);
    // Start new configurable production tables within the paper width; long
    // wrapped rows remain complete and can be split/paginated explicitly.
    if matches!(
        table.kind,
        TableKind::BillOfMaterials | TableKind::WeldmentCutList { .. }
    ) {
        let available = cad_core::drawing::paper_bounds(sheet)[1][0] - table.position_mm[0];
        let width = table.column_widths_mm.iter().sum::<f64>();
        let minimum = table.column_widths_mm.len() as f64 * 4.;
        if width > available && available > minimum {
            let scale = (available - minimum) / (width - minimum);
            for column in &mut table.column_widths_mm {
                *column = 4. + (*column - 4.) * scale;
            }
        }
    }
    if let Some(e) = evaluation(w) {
        if matches!(table.kind, TableKind::Holes { .. }) {
            for key in cad_core::drawing::view_keys(sheet)
                .into_iter()
                .filter(|v| matches!(v.as_str(), "front" | "top" | "right"))
            {
                if let TableKind::Holes { view, .. } = &mut table.kind {
                    *view = key;
                }
                if content_for_sheet(w.session.visible(), e, sheet, &table).is_ok() {
                    break;
                }
            }
        }
        pin_source_rows(w.session.visible(), e, sheet, &mut table)?;
    }
    Ok(table)
}

fn option(label: impl Into<String>, value: impl serde::Serialize) -> serde_json::Value {
    serde_json::json!({"label":label.into(),"value":value})
}

pub(crate) fn production_fields(
    f: &mut crate::drawing_editor_ui::Fields<'_>,
    table: &Table,
    path: &str,
) {
    use serde_json::json;
    if matches!(table.kind, TableKind::BillOfMaterials) {
        f.group("bom-options", "BOM structure");
        f.select(
            &format!("{path}/bom_options/mode"),
            "Structure",
            vec![
                option("Parts only", "parts_only"),
                option("Top level", "top_level"),
                option("Indented hierarchy", "indented"),
            ],
        );
        f.select(
            &format!("{path}/bom_options/grouping"),
            "Group components",
            vec![
                option("Definition and configuration", "definition_configuration"),
                option(
                    "Definition, configuration and finish",
                    "definition_configuration_finish",
                ),
            ],
        );
        f.group("production-columns", "Column contents");
        let columns = vec![
            cad_core::bom::Column::Item,
            cad_core::bom::Column::PartNumber,
            cad_core::bom::Column::Description,
            cad_core::bom::Column::Quantity,
            cad_core::bom::Column::Material,
            cad_core::bom::Column::Finish,
            cad_core::bom::Column::Configuration,
            cad_core::bom::Column::Definition,
            cad_core::bom::Column::ComponentType,
            cad_core::bom::Column::Depth,
            cad_core::bom::Column::Property {
                key: "part_number".into(),
                label: "Custom property".into(),
            },
        ];
        for (index, column) in table.bom_options.columns.iter().enumerate() {
            let pointer = format!("{path}/bom_options/columns/{index}");
            f.select(
                &pointer,
                &format!("Column {} contents", index + 1),
                columns
                    .iter()
                    .map(|column| option(column.label(), column))
                    .collect(),
            );
            if matches!(column, cad_core::bom::Column::Property { .. }) {
                f.text(&format!("{pointer}/key"), "Property key", false);
                f.text(&format!("{pointer}/label"), "Column heading", false);
            }
            column_actions(f, table, index, table.bom_options.columns.len());
        }
        f.action(
            &format!("table/{}/column/add", table.id),
            "Add column",
            json!({"kind":"table-column-add","table":table.id}),
            table.bom_options.columns.len() < 32,
            "At most 32 columns",
        );
    }
    if matches!(table.kind, TableKind::WeldmentCutList { .. }) {
        f.group("stock-options", "Stock grouping");
        f.select(
            &format!("{path}/cut_list_options/grouping"),
            "Group stock",
            vec![
                option("Individual source members", "by_source"),
                option("Equivalent production stock", "equivalent_stock"),
            ],
        );
        cut_list_columns(f, table, path);
        f.action(&format!("table/{}/balloons",table.id),"Auto cut-list balloons",json!({"kind":"cut-list-balloons","table":table.id,"view":cad_core::drawing::view_keys(f.sheet).first()}),evaluation(f.w).is_some(),"Wait for current evaluated fabrication metadata");
    }
    f.group("table-pagination", "Splits and continuation pages");
    f.action(
        &format!("table/{}/paginate", table.id),
        "Paginate complete table",
        json!({"kind":"table-paginate","table":table.id}),
        evaluation(f.w).is_some(),
        "Wait for current model evaluation",
    );
    f.action(
        &format!("table/{}/split", table.id),
        "Split into two sections",
        json!({"kind":"table-split","table":table.id}),
        evaluation(f.w).is_some(),
        "Wait for current model evaluation",
    );
    f.action(
        &format!("table/{}/unsplit", table.id),
        "Combine all sections",
        json!({"kind":"table-unsplit","table":table.id}),
        !table.segments.is_empty(),
        "This table has one section",
    );
    for (index, segment) in table.segments.iter().enumerate() {
        let pointer = format!("{path}/segments/{index}");
        f.note(
            &format!("segment/{}", segment.id),
            &format!(
                "Section {} · rows {}–{}",
                index + 1,
                segment.start_row + 1,
                segment
                    .row_count
                    .map(|count| (segment.start_row + count).to_string())
                    .unwrap_or_else(|| "last".into())
            ),
            "ready",
        );
        f.number(
            &format!("{pointer}/page"),
            "Page offset (0 = original)",
            "",
            Some(0.),
            Some(1023.),
        );
        f.coordinates(
            &format!("{pointer}/position_mm"),
            &["Section X", "Section Y"],
            "mm",
        );
        f.toggle(
            &format!("{pointer}/repeat_header"),
            "Repeat column headings",
        );
        f.action(
            &format!("table/{}/segment/{}/combine", table.id, segment.id),
            "Combine with previous",
            json!({"kind":"table-segment-combine","table":table.id,"segment":segment.id}),
            index > 0,
            "First section has no previous section",
        );
    }
}
fn column_actions(
    f: &mut crate::drawing_editor_ui::Fields<'_>,
    table: &Table,
    index: usize,
    count: usize,
) {
    use serde_json::json;
    for (direction, label, enabled) in [
        (-1, "Move column left", index > 0),
        (1, "Move column right", index + 1 < count),
    ] {
        f.action(&format!("table/{}/column/{index}/move/{direction}",table.id),label,json!({"kind":"table-column-move","table":table.id,"index":index,"direction":direction}),enabled,"Column is already at this end");
    }
    f.action(
        &format!("table/{}/column/{index}/remove", table.id),
        "Remove column",
        json!({"kind":"table-column-remove","table":table.id,"index":index}),
        count > 1,
        "A table needs at least one column",
    );
}

/// Mutates the shared saved drawing definition only after the caller's current
/// source/editor guards. The caller previews and applies this in one transaction.
pub(crate) fn apply_action(
    w: &Workspace,
    sheet: &mut Drawing,
    payload: &serde_json::Value,
) -> cad_core::Result<bool> {
    let kind = payload["kind"].as_str().unwrap_or("");
    if !matches!(
        kind,
        "table-paginate"
            | "table-split"
            | "table-unsplit"
            | "table-segment-combine"
            | "table-column-add"
            | "table-column-remove"
            | "table-column-move"
            | "cut-list-balloons"
    ) {
        return Ok(false);
    }
    let fail = |message: &str| cad_core::CadError::Invalid(message.into());
    let id = payload["table"]
        .as_str()
        .ok_or_else(|| fail("Choose a table"))?;
    let index = sheet
        .tables
        .iter()
        .position(|t| t.id == id)
        .ok_or_else(|| fail("Table was removed"))?;
    let before = sheet.clone();
    match kind {
        "table-paginate" => {
            let e = evaluation(w).ok_or_else(|| fail("Wait for current model evaluation"))?;
            paginate(w.session.visible(), e, &before, &mut sheet.tables[index], 0)?;
        }
        "table-split" => {
            let e = evaluation(w).ok_or_else(|| fail("Wait for current model evaluation"))?;
            let content =
                content_for_sheet(w.session.visible(), e, &before, &before.tables[index])?;
            if content.rows.len() < 2 {
                return Err(fail("Splitting requires at least two table rows"));
            }
            let table = &mut sheet.tables[index];
            let split = content.rows.len() / 2;
            let width = table.column_widths_mm.iter().sum::<f64>();
            let bounds = cad_core::drawing::paper_bounds(&before);
            let same_page = table.position_mm[0] + width * 2. + 6. <= bounds[1][0];
            table.segments = vec![
                Segment {
                    id: cad_core::id(),
                    page: 0,
                    start_row: 0,
                    row_count: Some(split),
                    position_mm: table.position_mm,
                    repeat_header: true,
                },
                Segment {
                    id: cad_core::id(),
                    page: if same_page { 0 } else { 1 },
                    start_row: split,
                    row_count: None,
                    position_mm: if same_page {
                        [table.position_mm[0] + width + 6., table.position_mm[1]]
                    } else {
                        table.position_mm
                    },
                    repeat_header: true,
                },
            ];
        }
        "table-unsplit" => sheet.tables[index].segments.clear(),
        "table-segment-combine" => {
            let table = &mut sheet.tables[index];
            let segment = payload["segment"]
                .as_str()
                .ok_or_else(|| fail("Select a section"))?;
            let i = table
                .segments
                .iter()
                .position(|s| s.id == segment)
                .ok_or_else(|| fail("Section was removed"))?;
            if i == 0 {
                return Err(fail("First section has no previous section"));
            }
            let count = table.segments[i].row_count;
            table.segments[i - 1].row_count = match (table.segments[i - 1].row_count, count) {
                (Some(a), Some(b)) => Some(a + b),
                _ => None,
            };
            table.segments.remove(i);
        }
        "cut-list-balloons" => {
            let e = evaluation(w).ok_or_else(|| fail("Wait for current model evaluation"))?;
            let view = payload["view"]
                .as_str()
                .ok_or_else(|| fail("Choose a drawing view"))?;
            if !cad_core::drawing::view_keys(sheet)
                .iter()
                .any(|key| key == view)
            {
                return Err(fail("Selected view was removed"));
            }
            let balloons = cad_core::drawing_tables::auto_cut_list_balloons(
                w.session.visible(),
                e,
                &before,
                &before.tables[index],
                view,
            )?;
            sheet.entities.extend(balloons);
        }
        _ => edit_columns(&mut sheet.tables[index], payload)?,
    }
    // Persist currently derived row identities alongside grouping edits, never
    // rely on disposable evaluation to manufacture an identity after Apply.
    if let Some(e) = evaluation(w) {
        let current = sheet.clone();
        pin_source_rows(w.session.visible(), e, &current, &mut sheet.tables[index])?;
    }
    Ok(true)
}
fn cut_list_columns(f: &mut crate::drawing_editor_ui::Fields<'_>, table: &Table, path: &str) {
    use cad_core::cut_list::Column;
    use serde_json::json;
    let columns = [
        Column::Item,
        Column::Description,
        Column::Section,
        Column::Length,
        Column::StartCutAngle,
        Column::EndCutAngle,
        Column::Quantity,
        Column::Material,
        Column::Finish,
        Column::Configuration,
        Column::Definition,
        Column::Source,
        Column::BendRadius,
        Column::BendSweep,
        Column::Accessory,
        Column::StockWidth,
        Column::StockHeight,
        Column::Thickness,
        Column::StockBounds,
    ];
    f.group("production-columns", "Column contents");
    for (index, _) in table.cut_list_options.columns.iter().enumerate() {
        f.select(
            &format!("{path}/cut_list_options/columns/{index}"),
            &format!("Column {} contents", index + 1),
            columns
                .iter()
                .map(|column| option(column.label(), column))
                .collect(),
        );
        column_actions(f, table, index, table.cut_list_options.columns.len());
    }
    f.action(
        &format!("table/{}/column/add", table.id),
        "Add column",
        json!({"kind":"table-column-add","table":table.id}),
        table.cut_list_options.columns.len() < 32,
        "At most 32 columns",
    );
}
fn edit_columns(table: &mut Table, payload: &serde_json::Value) -> cad_core::Result<()> {
    let fail = |message: &str| cad_core::CadError::Invalid(message.into());
    let kind = payload["kind"].as_str().unwrap_or("");
    let index = payload["index"].as_u64().unwrap_or(u64::MAX) as usize;
    let destination = (index as i64 + payload["direction"].as_i64().unwrap_or(0))
        .try_into()
        .ok();
    fn edit<T: Clone>(
        values: &mut Vec<T>,
        widths: &mut Vec<f64>,
        kind: &str,
        index: usize,
        destination: Option<usize>,
        default: T,
    ) -> bool {
        match kind {
            "table-column-add" if values.len() < 32 => {
                values.push(default);
                widths.push(30.);
                true
            }
            "table-column-remove" if values.len() > 1 && index < values.len() => {
                values.remove(index);
                widths.remove(index);
                true
            }
            "table-column-move"
                if index < values.len() && destination.is_some_and(|i| i < values.len()) =>
            {
                values.swap(index, destination.unwrap());
                widths.swap(index, destination.unwrap());
                true
            }
            _ => false,
        }
    }
    let handled = match table.kind {
        TableKind::BillOfMaterials => edit(
            &mut table.bom_options.columns,
            &mut table.column_widths_mm,
            kind,
            index,
            destination,
            cad_core::bom::Column::Finish,
        ),
        TableKind::WeldmentCutList { .. } => edit(
            &mut table.cut_list_options.columns,
            &mut table.column_widths_mm,
            kind,
            index,
            destination,
            cad_core::cut_list::Column::Finish,
        ),
        _ => false,
    };
    if !handled {
        return Err(fail(
            "The selected column no longer exists or this table cannot change production columns",
        ));
    }
    Ok(())
}

pub(crate) fn default_cut_list_balloon(
    w: &Workspace,
    sheet: &Drawing,
    target: Anchor,
) -> cad_core::Result<AnnotationKind> {
    let fail = |message: &str| cad_core::CadError::Invalid(message.into());
    let table = sheet
        .tables
        .iter()
        .find(|table| matches!(table.kind, TableKind::WeldmentCutList { .. }))
        .ok_or_else(|| fail("Add a native weldment cut-list table first"))?;
    let e = evaluation(w).ok_or_else(|| fail("Wait for current fabrication metadata"))?;
    let content = content_for_sheet(w.session.visible(), e, sheet, table)?;
    let row = w
        .selected
        .as_ref()
        .and_then(|selected| {
            content.entries.iter().find(|row| match &row.source {
                RowSource::StockGroup { sources, .. } => sources.iter().any(|source| {
                    selected.body.as_ref() == Some(&source.output)
                        && source.occurrence_path == selected.path
                }),
                _ => false,
            })
        })
        .or_else(|| content.entries.first())
        .ok_or_else(|| fail("This table has no active native stock rows"))?;
    let RowSource::StockGroup { sources, .. } = &row.source else {
        return Err(fail("Choose a current source-linked stock row"));
    };
    let source = w
        .selected
        .as_ref()
        .and_then(|selected| {
            sources.iter().find(|source| {
                source.occurrence_path == selected.path
                    && selected.body.as_ref() == Some(&source.output)
            })
        })
        .or_else(|| sources.first())
        .cloned()
        .ok_or_else(|| fail("Stock row has no source occurrence"))?;
    Ok(AnnotationKind::CutListBalloon {
        target,
        table: table.id.clone(),
        row_key: row.key.clone(),
        source,
        show_quantity: true,
    })
}
