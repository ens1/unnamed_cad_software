//! Saved sketch/table placements for rigid solid-body patterns.
use crate::{
    CadError, Dimension, Document, Expr, Id, NodeRef, Result,
    presentation::SemanticElement,
    reference_geometry::{AxisSource, OriginAxis, PointSource},
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
pub const CONTRACT_VERSION: u32 = 1;
pub const MAX_PLACEMENTS: usize = 10000;
pub const MAX_TABLE_BYTES: usize = 1024 * 1024;
fn version() -> u32 {
    CONTRACT_VERSION
}
fn invalid(message: impl ToString) -> CadError {
    CadError::Invalid(format!("Placement Pattern: {}", message.to_string()))
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    SketchDriven,
    TableDriven,
}
impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Self::SketchDriven => "Sketch Driven Pattern",
            Self::TableDriven => "Table Driven Pattern",
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Record {
    /// Authored placement identity; it is never derived from a row number or position.
    pub id: Id,
    pub target: Box<PointSource>,
    pub angle: Expr,
    #[serde(default)]
    pub skipped: bool,
}
impl Record {
    pub fn new(target: PointSource) -> Self {
        Self {
            id: crate::id(),
            target: Box::new(target),
            angle: "0 deg".into(),
            skipped: false,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Definition {
    #[serde(default = "version")]
    pub version: u32,
    pub kind: Kind,
    pub body: NodeRef,
    pub seed: Box<PointSource>,
    pub axis: Box<AxisSource>,
    pub placements: Vec<Record>,
    #[serde(default)]
    pub keep_source: bool,
}
impl Definition {
    pub fn new(body: NodeRef, kind: Kind) -> Self {
        Self {
            version: CONTRACT_VERSION,
            kind,
            body,
            seed: Box::default(),
            axis: Box::new(AxisSource::Origin {
                axis: OriginAxis::Z,
            }),
            placements: vec![],
            keep_source: true,
        }
    }
    pub fn label(&self) -> &'static str {
        self.kind.label()
    }
    pub fn dependencies(&self) -> Vec<NodeRef> {
        let mut refs = vec![self.body.clone()];
        refs.extend(self.seed.dependencies());
        refs.extend(self.axis.dependencies());
        for record in &self.placements {
            refs.extend(record.target.dependencies());
        }
        refs.sort_by(|a, b| (&a.definition, &a.node).cmp(&(&b.definition, &b.node)));
        refs.dedup();
        refs
    }
    pub fn consumed_outputs(&self) -> Vec<NodeRef> {
        vec![self.body.clone()]
    }
    pub fn expressions(&self) -> Vec<&Expr> {
        let mut out = self.seed.expressions();
        for record in &self.placements {
            out.push(&record.angle);
            out.extend(record.target.expressions());
        }
        out
    }
    pub fn validate(&self) -> Result<()> {
        if self.version != CONTRACT_VERSION {
            return Err(invalid("Unsupported native contract version"));
        }
        validate_records(self.kind, &self.placements, self.keep_source)
    }
    pub fn angle(&self, doc: &Document, owner: &str, record: &Record) -> Result<f64> {
        let angle = crate::expression::value(doc, owner, &record.angle, Dimension::Angle)?;
        if !angle.is_finite() || angle.abs() > std::f64::consts::TAU * 1e6 {
            return Err(invalid(
                "Placement rotation must be finite and within one million turns",
            ));
        }
        Ok(angle.rem_euclid(std::f64::consts::TAU))
    }
}
fn validate_records(kind: Kind, records: &[Record], keep_source: bool) -> Result<()> {
    if records.is_empty() || records.len() > MAX_PLACEMENTS {
        return Err(invalid(
            "Select or author 1–10000 explicit placement records",
        ));
    }
    let mut ids = BTreeSet::new();
    for record in records {
        if record.id.trim().is_empty()
            || !ids.insert(&record.id)
            || record.id.len() > 256
            || record.id.chars().any(char::is_control)
        {
            return Err(invalid(
                "Placement keys must be nonempty, unique, at most 256 bytes, and contain no control characters",
            ));
        }
        if kind == Kind::SketchDriven
            && !matches!(record.target.as_ref(), PointSource::SketchPoint { .. })
        {
            return Err(invalid(
                "Sketch Driven Pattern requires explicitly selected native sketch points",
            ));
        }
    }
    if !keep_source && records.iter().all(|record| record.skipped) {
        return Err(invalid(
            "Keep an active placement or enable Keep Source Body",
        ));
    }
    Ok(())
}
fn csv_field(value: &str) -> String {
    if value
        .chars()
        .any(|ch| matches!(ch, ',' | '"' | '\n' | '\r'))
    {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.into()
    }
}
/// Portable table-source export. Linked point references stay in the native
/// definition and cannot be replaced by invented or unevaluated CSV coordinates.
pub fn table_csv(definition: &Definition) -> Result<String> {
    definition.validate()?;
    if definition.kind != Kind::TableDriven {
        return Err(invalid("CSV table export requires Table Driven Pattern"));
    }
    let mut out = "key,x,y,z,angle,skipped\n".to_string();
    for record in &definition.placements {
        let PointSource::Coordinates { coordinates } = record.target.as_ref() else {
            return Err(CadError::Unsupported("Placement Pattern: Coordinate CSV cannot encode linked point references; keep those rows in the native model".into()));
        };
        let fields = [
            record.id.clone(),
            coordinates[0].0.clone(),
            coordinates[1].0.clone(),
            coordinates[2].0.clone(),
            record.angle.0.clone(),
            record.skipped.to_string(),
        ];
        out.push_str(
            &fields
                .iter()
                .map(|field| csv_field(field))
                .collect::<Vec<_>>()
                .join(","),
        );
        out.push('\n');
    }
    if out.len() > MAX_TABLE_BYTES {
        return Err(invalid(
            "The source table exceeds the 1 MiB portable-table limit",
        ));
    }
    Ok(out)
}
fn csv_rows(source: &str) -> Result<Vec<Vec<String>>> {
    if source.len() > MAX_TABLE_BYTES {
        return Err(invalid("CSV input exceeds 1 MiB"));
    }
    let mut chars = source.trim_start_matches('\u{feff}').chars().peekable();
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut closed = false;
    while let Some(ch) = chars.next() {
        if quoted {
            if ch == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    field.push('"');
                } else {
                    quoted = false;
                    closed = true;
                }
            } else {
                field.push(ch);
            }
            continue;
        }
        match ch {
            '"' if field.is_empty() && !closed => quoted = true,
            ',' => {
                row.push(std::mem::take(&mut field));
                closed = false;
            }
            '\n' | '\r' => {
                if ch == '\r' && chars.peek() == Some(&'\n') {
                    chars.next();
                }
                let explicit = !row.is_empty() || closed;
                row.push(std::mem::take(&mut field));
                closed = false;
                if explicit || row.iter().any(|field| !field.trim().is_empty()) {
                    rows.push(std::mem::take(&mut row));
                } else {
                    row.clear();
                }
                if rows.len() > MAX_PLACEMENTS + 1 {
                    return Err(invalid("CSV input exceeds 10000 placements"));
                }
            }
            ch if closed && ch.is_whitespace() => {}
            _ if closed => return Err(invalid("Unexpected content after a quoted CSV field")),
            '"' => {
                return Err(invalid(
                    "Quote an entire CSV field and double embedded quotes",
                ));
            }
            _ => field.push(ch),
        }
    }
    if quoted {
        return Err(invalid("CSV has an unclosed quoted field"));
    }
    if !field.is_empty() || !row.is_empty() || closed {
        let explicit = !row.is_empty() || closed;
        row.push(field);
        if explicit || row.iter().any(|field| !field.trim().is_empty()) {
            rows.push(row);
        }
    }
    Ok(rows)
}
/// Stable keys are required on import, so reordering external rows never assigns
/// one retained placement's references to another placement by coordinate/order.
pub fn import_csv(source: &str) -> Result<Vec<Record>> {
    let rows = csv_rows(source)?;
    let Some(header) = rows.first() else {
        return Err(invalid("CSV is empty"));
    };
    if header
        .iter()
        .map(|field| field.trim().to_lowercase())
        .collect::<Vec<_>>()
        != ["key", "x", "y", "z", "angle", "skipped"]
    {
        return Err(invalid("CSV header must be key,x,y,z,angle,skipped"));
    }
    let mut records = Vec::new();
    for (index, row) in rows.into_iter().skip(1).enumerate() {
        if row.len() != 6 {
            return Err(invalid(format!(
                "CSV row {} must have six fields",
                index + 2
            )));
        }
        let fields = row.iter().map(|field| field.trim()).collect::<Vec<_>>();
        if fields.iter().take(5).any(|field| field.is_empty()) {
            return Err(invalid(format!(
                "CSV row {} has an empty key or quantity expression",
                index + 2
            )));
        }
        let skipped = match fields[5].to_lowercase().as_str() {
            "true" | "1" => true,
            "false" | "0" | "" => false,
            _ => {
                return Err(invalid(format!(
                    "CSV row {} Skip must be true/false or 1/0",
                    index + 2
                )));
            }
        };
        records.push(Record {
            id: row[0].clone(),
            target: Box::new(PointSource::Coordinates {
                coordinates: [
                    Expr(row[1].clone()),
                    Expr(row[2].clone()),
                    Expr(row[3].clone()),
                ],
            }),
            angle: Expr(row[4].clone()),
            skipped,
        });
    }
    validate_records(Kind::TableDriven, &records, true)?;
    Ok(records)
}
pub fn semantic_fields(
    doc: &Document,
    owner: &str,
    node: &str,
    definition: &Definition,
    state: &str,
) -> Vec<SemanticElement> {
    let target = serde_json::json!({"kind":"feature","definition":owner,"feature":node});
    let mut out = Vec::new();
    for (index, record) in definition.placements.iter().enumerate() {
        // Arbitrary CSV keys can contain semantic separators or reserved words.
        // Encode the exact UTF-8 bytes in a dedicated record namespace.
        let mut key = String::with_capacity(record.id.len() * 2);
        let digits = b"0123456789abcdef";
        for byte in record.id.bytes() {
            key.push(char::from(digits[usize::from(byte >> 4)]));
            key.push(char::from(digits[usize::from(byte & 15)]));
        }
        let mut quantities = vec![(
            "angle".to_string(),
            "Rotation".to_string(),
            &record.angle,
            Dimension::Angle,
        )];
        if let PointSource::Coordinates { coordinates } = record.target.as_ref() {
            quantities.extend(coordinates.iter().enumerate().map(|(axis, expr)| {
                (
                    format!("target.coordinates.{axis}"),
                    ["X", "Y", "Z"][axis].into(),
                    expr,
                    Dimension::Length,
                )
            }));
        }
        for (field, label, expr, dimension) in quantities {
            out.push(SemanticElement {id:format!("{node}::placement-pattern::record::{key}::field::{field}"),role:"feature-field".into(),label:format!("Placement {} {label}",index+1),value:Some(expr.0.clone()),state:state.into(),commands:vec!["feature.edit".into()],relationships:vec![node.into(),record.id.clone()],data:serde_json::json!({"contract":"tcad.placement-pattern","version":CONTRACT_VERSION,"target":target,"placement":record.id,"field":field,"expression":expr,"dimension":dimension,"evaluated":crate::expression::value(doc,owner,expr,dimension).ok(),"skipped":record.skipped,"editable":true})});
        }
        out.push(SemanticElement {id:format!("{node}::placement-pattern::record::{key}"),role:"pattern-instance".into(),label:format!("Placement {}",index+1),value:None,state:if record.skipped {"skipped".into()}else{state.into()},commands:vec!["feature.edit".into()],relationships:record.target.dependencies().iter().map(|r|format!("{}::{}",r.definition,r.node)).collect(),data:serde_json::json!({"target":target,"source":record,"instance_key":record.id,"order_is_identity":false})});
    }
    out.push(SemanticElement {id:format!("{node}::placement-pattern::options"),role:"feature-options".into(),label:definition.label().into(),value:None,state:state.into(),commands:vec!["feature.edit".into()],relationships:definition.dependencies().iter().map(|r|format!("{}::{}",r.definition,r.node)).collect(),data:serde_json::json!({"contract":"tcad.placement-pattern","version":CONTRACT_VERSION,"target":target,"source":definition,"selection":"explicit_saved_point_sources","source_scope":"complete_native_solid_body_set","rotation":"associative_axis_direction_about_seed_then_seed_maps_to_target","instance_identity":"authored_record_id","keep_source":"retain_original_once_and_reuse_identity_target","geometry":"validated_exact_rigid_brep_copies","shared_draft":true,"requires_current_validation":true,"editable":true})});
    out
}
