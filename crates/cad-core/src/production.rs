//! Saved production report preferences shared by UI and worker exports.
use crate::{CadError, Document, Drawing, Id, Result, protocol::Evaluation};
use serde::{Deserialize, Serialize};

pub const CONTRACT_VERSION: u32 = 1;
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Preferences {
    pub version: u32,
    pub bom: crate::bom::Options,
    pub cut_list: crate::cut_list::Options,
    pub units: crate::drawing::Units,
    pub precision: usize,
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            version: CONTRACT_VERSION,
            bom: Default::default(),
            cut_list: Default::default(),
            units: crate::drawing::Units::Millimeters,
            precision: 3,
        }
    }
}
impl Preferences {
    pub fn validate(&self) -> Result<()> {
        if self.version != CONTRACT_VERSION || self.precision > 8 {
            return Err(CadError::Invalid(
                "Production preferences need contract version 1 and precision 0–8".into(),
            ));
        }
        crate::bom::validate(&self.bom)?;
        crate::cut_list::validate(&self.cut_list)
    }
}
pub fn bom_csv(doc: &Document, evaluation: &Evaluation) -> Result<String> {
    doc.settings.production.validate()?;
    let options = &doc.settings.production.bom;
    let mut output = crate::bom::csv_record(options.columns.iter().map(crate::bom::Column::label));
    for (index, row) in
        crate::bom::rows_with_placements(doc, options, Some(&evaluation.assemblies))?
            .iter()
            .enumerate()
    {
        output.push_str(&crate::bom::csv_record(
            crate::bom::cells(row, &options.columns, (index + 1) as u32)
                .iter()
                .map(String::as_str),
        ));
    }
    Ok(output)
}
pub fn cut_list_csv(doc: &Document, evaluation: &Evaluation) -> Result<String> {
    let preferences = &doc.settings.production;
    preferences.validate()?;
    crate::cut_list::csv(
        doc,
        evaluation,
        &preferences.cut_list,
        preferences.units,
        preferences.precision,
    )
}
/// Export the entire current source table, retaining its saved row numbers,
/// ordering, configurable columns and displayed units across all paper pages.
pub fn table_csv(
    doc: &Document,
    evaluation: &Evaluation,
    sheet: &Drawing,
    table: &crate::drawing_tables::Table,
) -> Result<String> {
    let content = crate::drawing_tables::content_for_sheet(doc, evaluation, sheet, table)?;
    let mut output = crate::bom::csv_record(content.headers.iter().map(String::as_str));
    for row in content.rows {
        output.push_str(&crate::bom::csv_record(row.iter().map(String::as_str)));
    }
    Ok(output)
}
pub fn table_export_format(sheet: &str, table: &str) -> String {
    format!("drawing-table-csv:{}", serde_json::json!([sheet, table]))
}
pub fn export_table(doc: &Document, evaluation: &Evaluation, format: &str) -> Result<String> {
    let pair = format
        .strip_prefix("drawing-table-csv:")
        .ok_or_else(|| CadError::Invalid("Missing drawing-table CSV export contract".into()))?;
    let (sheet_id, table_id): (Id, Id) = serde_json::from_str(pair)?;
    let sheet = doc
        .drawings
        .iter()
        .find(|sheet| sheet.id == sheet_id)
        .ok_or_else(|| CadError::Invalid("The selected drawing sheet was removed".into()))?;
    let table = sheet
        .tables
        .iter()
        .find(|table| table.id == table_id)
        .ok_or_else(|| CadError::Invalid("The selected drawing table was removed".into()))?;
    table_csv(doc, evaluation, sheet, table)
}
