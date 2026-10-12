//! Associative, renderer-independent drawing tables and stable BOM item references.
use crate::{
    CadError, Document, Drawing, Id, NodeRef, Result, drawing::Primitive, protocol::Evaluation,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

pub const CONTRACT_VERSION: u32 = 3;
pub const MAX_RESERVATIONS: usize = 100_000;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct RevisionRow {
    pub id: Id,
    pub revision: String,
    pub description: String,
    pub date: String,
    pub approved_by: String,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HoleDatum {
    #[default]
    ModelOrigin,
    Reference {
        point: Box<crate::drawing::PointReference>,
    },
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TableKind {
    BillOfMaterials,
    Revisions {
        rows: Vec<RevisionRow>,
    },
    Bends {
        source: NodeRef,
        #[serde(default)]
        configuration: Option<Id>,
    },
    Holes {
        tag_prefix: String,
        features: Vec<NodeRef>,
        #[serde(default)]
        configuration: Option<Id>,
        #[serde(default)]
        instance_path: Vec<Id>,
        view: String,
        datum: HoleDatum,
        #[serde(default)]
        units: crate::drawing::Units,
        precision: usize,
    },
    WeldmentCutList {
        scope: Id,
        #[serde(default)]
        configuration: Option<Id>,
        #[serde(default)]
        sources: Vec<NodeRef>,
        #[serde(default)]
        units: crate::drawing::Units,
        precision: usize,
    },
    General {
        headers: Vec<String>,
        rows: Vec<Vec<String>>,
    },
}
impl TableKind {
    pub fn name(&self) -> &'static str {
        match self {
            Self::BillOfMaterials => "Bill of Materials",
            Self::Revisions { .. } => "Revision Table",
            Self::Bends { .. } => "Bend Table",
            Self::Holes { .. } => "Hole Table",
            Self::WeldmentCutList { .. } => "Weldment Cut List",
            Self::General { .. } => "General Table",
        }
    }
}
/// One editable portion of a table. Row ranges refer to the complete current
/// ordered source table, before paging; no source row is copied or truncated.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Segment {
    pub id: Id,
    pub page: u32,
    pub start_row: usize,
    /// None includes the remaining current rows, accommodating later additions.
    pub row_count: Option<usize>,
    pub position_mm: [f64; 2],
    #[serde(default = "repeat_header")]
    pub repeat_header: bool,
}
fn repeat_header() -> bool {
    true
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Table {
    pub id: Id,
    pub title: String,
    pub kind: TableKind,
    /// Bottom-left corner in paper millimetres.
    pub position_mm: [f64; 2],
    pub column_widths_mm: Vec<f64>,
    pub row_height_mm: f64,
    pub text_size_mm: f64,
    /// Explicit numbers remain stable when other rows are added or suppressed.
    #[serde(default)]
    pub item_numbers: BTreeMap<String, u32>,
    #[serde(default)]
    pub reserve_space: bool,
    /// Stable source keys; absent rows remain reserved in their saved order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub row_order: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub segments: Vec<Segment>,
    /// None keeps the model page when the complete table leaves usable view room.
    #[serde(default)]
    pub pagination_start_page: Option<u32>,
    #[serde(default)]
    pub bom_options: crate::bom::Options,
    #[serde(default)]
    pub cut_list_options: crate::cut_list::Options,
}
impl Table {
    pub fn new(kind: TableKind) -> Self {
        let widths = match &kind {
            TableKind::BillOfMaterials => vec![12., 35., 65., 12., 35.],
            TableKind::Revisions { .. } => vec![20., 80., 30., 35.],
            TableKind::Bends { .. } => vec![25., 22., 22., 22., 25., 22.],
            TableKind::Holes { .. } => vec![12., 30., 22., 22., 24., 26., 48.],
            TableKind::WeldmentCutList { .. } => vec![12., 42., 30., 25., 22., 22., 12., 25.],
            TableKind::General { headers, .. } => vec![35.; headers.len()],
        };
        Self {
            id: crate::id(),
            title: kind.name().into(),
            kind,
            position_mm: [12., 12.],
            column_widths_mm: widths,
            row_height_mm: 8.,
            text_size_mm: 2.5,
            item_numbers: BTreeMap::new(),
            reserve_space: true,
            row_order: vec![],
            segments: vec![],
            pagination_start_page: None,
            bom_options: Default::default(),
            cut_list_options: Default::default(),
        }
    }
}
#[derive(Clone, Debug)]
pub struct Item {
    pub key: String,
    pub definition: Id,
    pub configuration: Option<Id>,
    pub number: u32,
    pub depth: usize,
    pub occurrences: Vec<crate::bom::Occurrence>,
    pub quantity: usize,
    pub cells: Vec<String>,
}
#[derive(Clone, Debug)]
pub struct Content {
    pub headers: Vec<String>,
    pub rows: Vec<Vec<String>>,
    pub items: Vec<Item>,
    pub entries: Vec<SourceRow>,
    pub reservations: BTreeMap<String, u32>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RowSource {
    Hole {
        feature: NodeRef,
        configuration: Option<Id>,
        point: Option<Id>,
    },
    StockGroup {
        key: String,
        sources: Vec<crate::cut_list::Source>,
    },
    CutList {
        output: NodeRef,
        configuration: Option<Id>,
        member: crate::weldment::MemberRef,
    },
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SourceRow {
    pub key: String,
    pub number: u32,
    pub source: RowSource,
    pub cells: Vec<String>,
    /// World-space designed entry point for a hole label, never a fitted mesh center.
    pub position_mm: Option<[f64; 3]>,
    #[serde(default)]
    pub quantity: Option<usize>,
}
#[derive(Clone, Debug)]
pub struct TableGeometry {
    pub primitives: Vec<Primitive>,
    pub bounds_mm: [[f64; 2]; 2],
    pub reserve_space: bool,
    pub segment: Option<Id>,
}

fn invalid(message: impl ToString) -> CadError {
    CadError::Invalid(format!("Drawing table: {}", message.to_string()))
}
fn admit_rows(current: usize, additional: usize, limit: usize) -> Result<()> {
    if current
        .checked_add(additional)
        .is_none_or(|count| count > limit)
    {
        return Err(invalid(format!(
            "Source row enumeration exceeds the bounded limit of {limit}"
        )));
    }
    Ok(())
}
pub fn item_key(definition: &str, configuration: Option<&str>) -> String {
    format!("{definition}@{}", configuration.unwrap_or_default())
}
pub fn validate(table: &Table) -> Result<()> {
    if table.id.is_empty()
        || table.item_numbers.len() > MAX_RESERVATIONS
        || table.row_order.len() > MAX_RESERVATIONS
        || table
            .item_numbers
            .keys()
            .chain(&table.row_order)
            .any(|key| key.len() > 8192)
        || table.title.chars().any(|c| c.is_control())
        || table.position_mm.iter().any(|v| !v.is_finite())
        || !table.row_height_mm.is_finite()
        || table.row_height_mm < 3.
        || !table.text_size_mm.is_finite()
        || table.text_size_mm < 0.5
        || table.text_size_mm * 1.5 > table.row_height_mm
        || table.column_widths_mm.is_empty()
        || table.column_widths_mm.len() > 32
        || table
            .column_widths_mm
            .iter()
            .any(|v| !v.is_finite() || *v < 4.)
    {
        return Err(invalid(
            "Use a stable identity, finite position, positive column sizes and text that fits its rows",
        ));
    }
    let numbers: BTreeSet<_> = table.item_numbers.values().collect();
    if table.item_numbers.values().any(|n| *n == 0) || numbers.len() != table.item_numbers.len() {
        return Err(invalid(
            "Saved table item numbers must be unique positive integers, including reserved numbers",
        ));
    }
    if table.row_order.iter().any(String::is_empty)
        || table.row_order.iter().collect::<BTreeSet<_>>().len() != table.row_order.len()
    {
        return Err(invalid(
            "Saved row order must contain distinct stable source keys",
        ));
    }
    if matches!(
        table.kind,
        TableKind::Holes { .. } | TableKind::WeldmentCutList { .. }
    ) && table
        .row_order
        .iter()
        .any(|key| !table.item_numbers.contains_key(key))
    {
        return Err(invalid(
            "Every saved source-row order entry needs a retained positive row-number reservation",
        ));
    }
    if table.pagination_start_page.is_some_and(|page| page > 1) {
        return Err(invalid(
            "Pagination starts on the model page or its first continuation page",
        ));
    }
    if table.segments.len() > 1024
        || table.segments.iter().any(|s| {
            s.id.is_empty()
                || s.page > 1023
                || s.start_row > 10000
                || s.row_count.is_some_and(|n| n == 0 || n > 10000)
                || s.position_mm.iter().any(|v| !v.is_finite())
        })
        || table
            .segments
            .iter()
            .map(|s| &s.id)
            .collect::<BTreeSet<_>>()
            .len()
            != table.segments.len()
    {
        return Err(invalid(
            "Table splits need distinct identities, finite paper positions and bounded pages and row ranges",
        ));
    }
    match &table.kind {
        TableKind::BillOfMaterials => {
            crate::bom::validate(&table.bom_options)?;
            if table.column_widths_mm.len() != table.bom_options.columns.len() {
                return Err(invalid("BOM columns and saved widths must agree"));
            }
        }
        TableKind::Revisions { rows } => {
            if table.column_widths_mm.len() != 4 || rows.len() > 10000 {
                return Err(invalid(
                    "Revision tables have four columns and at most 10000 rows",
                ));
            }
            let mut ids = BTreeSet::new();
            let mut names = BTreeSet::new();
            for row in rows {
                if row.id.is_empty()
                    || !ids.insert(&row.id)
                    || row.revision.trim().is_empty()
                    || !names.insert(&row.revision)
                {
                    return Err(invalid(
                        "Revision rows need distinct identities and nonempty, distinct revision labels",
                    ));
                }
            }
        }
        TableKind::Bends { .. } if table.column_widths_mm.len() != 6 => {
            return Err(invalid("Bend tables have six columns"));
        }
        TableKind::Holes {
            features,
            view,
            datum,
            precision,
            tag_prefix,
            ..
        } => {
            if table.column_widths_mm.len() != 7
                || features.is_empty()
                || features.len() > 10000
                || features.iter().collect::<BTreeSet<_>>().len() != features.len()
                || features
                    .iter()
                    .any(|f| f.definition != features[0].definition || f.node.is_empty())
                || !matches!(view.as_str(), "front" | "top" | "right")
                || *precision > 8
                || tag_prefix.is_empty()
                || tag_prefix.len() > 32
                || !tag_prefix
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_')
                || matches!(datum,HoleDatum::Reference {point} if matches!(point.as_ref(),crate::drawing::PointReference::EdgePoint {parameter,..} if !parameter.is_finite() || !(0. ..=1.).contains(parameter)))
            {
                return Err(invalid(
                    "Hole tables need seven columns, distinct designed holes in one part, an orthographic coordinate view and precision 0..8",
                ));
            }
        }
        TableKind::WeldmentCutList {
            scope,
            sources,
            precision,
            ..
        } => {
            crate::cut_list::validate(&table.cut_list_options)?;
            if table.column_widths_mm.len() != table.cut_list_options.columns.len()
                || scope.is_empty()
                || *precision > 8
                || sources.len() > 10000
                || sources.iter().collect::<BTreeSet<_>>().len() != sources.len()
            {
                return Err(invalid(
                    "Weldment cut lists need eight columns, a model scope, distinct source outputs and precision 0..8",
                ));
            }
        }
        TableKind::General { headers, rows }
            if headers.len() != table.column_widths_mm.len()
                || rows.len() > 10000
                || rows.iter().any(|r| r.len() != headers.len()) =>
        {
            return Err(invalid(
                "General table headers, columns and row cell counts must agree; at most 10000 rows are supported",
            ));
        }
        _ => {}
    }
    Ok(())
}
pub fn validate_sources(doc: &Document, table: &Table) -> Result<()> {
    validate(table)?;
    let feature = |reference: &NodeRef| {
        doc.parts
            .get(&reference.definition)
            .and_then(|p| p.features.iter().find(|f| f.id == reference.node))
    };
    match &table.kind {
        TableKind::Holes {
            features,
            configuration,
            datum,
            ..
        } => {
            for reference in features {
                if !feature(reference).is_some_and(|f| {
                    matches!(
                        f.operation,
                        crate::Operation::Hole { .. } | crate::Operation::DrilledHoles { .. }
                    )
                }) {
                    return Err(invalid(
                        "A hole table source was removed or changed type; reattach or remove the table explicitly",
                    ));
                }
            }
            if let Some(configuration) = configuration
                && !crate::configuration::definitions(doc, &features[0].definition)
                    .contains_key(configuration)
            {
                return Err(invalid("Hole table configuration is missing"));
            }
            if let HoleDatum::Reference { point } = datum {
                match point.as_ref() {
                    crate::drawing::PointReference::SketchPoint { sketch, point, .. } => {
                        let source = doc.parts.get(&sketch.definition).and_then(|part| {
                            part.sketches.get(&sketch.node).or_else(|| {
                                feature(sketch).and_then(|f| {
                                    if let crate::Operation::Profile {
                                        profile: crate::Profile::Sketch { sketch },
                                        ..
                                    } = &f.operation
                                    {
                                        part.sketches.get(sketch)
                                    } else {
                                        None
                                    }
                                })
                            })
                        });
                        if !source.is_some_and(|source| source.points.contains_key(point)) {
                            return Err(invalid(
                                "Hole table sketch datum was removed; reattach it explicitly",
                            ));
                        }
                    }
                    crate::drawing::PointReference::CurveCenter { curve } => {
                        let source = match curve.as_ref() {
                            crate::drawing::CurveReference::SketchMember { sketch, .. } => sketch,
                            crate::drawing::CurveReference::ModelEdge { edge, .. } => {
                                &edge.producer
                            }
                        };
                        if feature(source).is_none() {
                            return Err(invalid(
                                "Hole-table center datum producer was removed; reattach it explicitly",
                            ));
                        }
                    }
                    crate::drawing::PointReference::EdgePoint { edge, body, .. }
                    | crate::drawing::PointReference::FaceOrigin {
                        face: edge, body, ..
                    } => {
                        if feature(&edge.producer).is_none()
                            || body.as_ref().is_some_and(|body| feature(body).is_none())
                        {
                            return Err(invalid(
                                "Hole table datum producer or owning body was removed; reattach it explicitly",
                            ));
                        }
                    }
                }
            }
        }
        TableKind::WeldmentCutList {
            scope,
            sources,
            configuration,
            ..
        } => {
            if !doc.parts.contains_key(scope) && !doc.assemblies.contains_key(scope) {
                return Err(invalid("Cut-list model scope was removed"));
            }
            for reference in sources {
                if !feature(reference)
                    .is_some_and(|f| matches!(f.operation, crate::Operation::Weldment { .. }))
                {
                    return Err(invalid(
                        "A selected native weldment output was removed or changed type; reattach it explicitly",
                    ));
                }
            }
            if let Some(configuration) = configuration
                && !crate::configuration::definitions(doc, scope).contains_key(configuration)
            {
                return Err(invalid("Cut-list scope configuration is missing"));
            }
        }
        _ => {}
    }
    Ok(())
}
pub fn content(doc: &Document, evaluation: &Evaluation, table: &Table) -> Result<Content> {
    validate(table)?;
    let mut items = Vec::new();
    let (headers, rows) = match &table.kind {
        TableKind::BillOfMaterials => {
            let source_rows = crate::bom::rows_with_placements(
                doc,
                &table.bom_options,
                Some(&evaluation.assemblies),
            )?;
            let mut next = table.item_numbers.values().max().copied().unwrap_or(0);
            for row in source_rows {
                let number = if let Some(number) = table.item_numbers.get(&row.key) {
                    *number
                } else {
                    next = next
                        .checked_add(1)
                        .ok_or_else(|| invalid("BOM item numbering exceeds the native range"))?;
                    next
                };
                let mut cells = crate::bom::cells(&row, &table.bom_options.columns, number);
                if table.bom_options.mode == crate::bom::Mode::Indented
                    && let Some(index) = table.bom_options.columns.iter().position(|column| {
                        matches!(
                            column,
                            crate::bom::Column::PartNumber | crate::bom::Column::Description
                        )
                    })
                {
                    cells[index] = format!("{}{}", "  ".repeat(row.depth.min(256)), cells[index]);
                }
                items.push(Item {
                    key: row.key,
                    definition: row.definition,
                    configuration: row.configuration,
                    number,
                    quantity: row.quantity,
                    cells,
                    depth: row.depth,
                    occurrences: row.occurrences,
                });
            }
            // Hierarchical traversal order is the definition-owned tree; numeric
            // reservations do not flatten children away from their parents.
            if table.bom_options.mode != crate::bom::Mode::Indented {
                items.sort_by_key(|row| row.number);
            }
            (
                table
                    .bom_options
                    .columns
                    .iter()
                    .map(|column| column.label().to_string())
                    .collect(),
                items.iter().map(|r| r.cells.clone()).collect(),
            )
        }
        TableKind::Revisions { rows } => (
            vec!["Revision", "Description", "Date", "Approved by"]
                .into_iter()
                .map(str::to_string)
                .collect(),
            rows.iter()
                .map(|r| {
                    vec![
                        r.revision.clone(),
                        r.description.clone(),
                        r.date.clone(),
                        r.approved_by.clone(),
                    ]
                })
                .collect(),
        ),
        TableKind::Bends {
            source,
            configuration,
        } => {
            let configuration =
                crate::configuration::selected(doc, &source.definition, configuration.as_ref());
            let key = format!(
                "{}::{}",
                crate::configuration::key(&source.definition, configuration.map(String::as_str)),
                source.node
            );
            let sheet = evaluation.sheets.get(&key).ok_or_else(|| {
                invalid("Selected sheet feature/configuration has no current bend data")
            })?;
            let rows = sheet
                .bends
                .iter()
                .enumerate()
                .map(|(index, bend)| {
                    vec![
                        (index + 1).to_string(),
                        format!("{:.3}", bend.angle_rad.to_degrees()),
                        format!("{:.3}", bend.inside_radius_mm),
                        format!("{:.3}", sheet.thickness_mm),
                        format!("{:.3}", bend.allowance_mm),
                        format!("{:.4}", bend.k_factor),
                    ]
                })
                .collect();
            (
                vec![
                    "Bend",
                    "Angle (deg)",
                    "Radius (mm)",
                    "Stock (mm)",
                    "Allowance (mm)",
                    "K factor",
                ]
                .into_iter()
                .map(str::to_string)
                .collect(),
                rows,
            )
        }
        TableKind::General { headers, rows } => (headers.clone(), rows.clone()),
        TableKind::Holes { .. } | TableKind::WeldmentCutList { .. } => {
            return Err(invalid(
                "Associative hole and cut-list tables require their drawing sheet context",
            ));
        }
    };
    if rows.len() > 10000 {
        return Err(invalid("A table supports at most 10000 rows"));
    }
    Ok(Content {
        headers,
        rows,
        items,
        entries: vec![],
        reservations: BTreeMap::new(),
    })
}
/// Save currently derived numbers while preserving reservations for suppressed rows.
pub fn pin_item_numbers(doc: &Document, evaluation: &Evaluation, table: &mut Table) -> Result<()> {
    if matches!(table.kind, TableKind::BillOfMaterials) {
        for row in content(doc, evaluation, table)?.items {
            table.item_numbers.insert(row.key, row.number);
        }
    }
    Ok(())
}
fn source_key(source: &RowSource) -> Result<String> {
    Ok(match source {
        RowSource::StockGroup { key, .. } => key.clone(),
        RowSource::CutList {
            configuration,
            member,
            ..
        } => serde_json::to_string(
            &serde_json::json!({"kind":"cut_list","configuration":configuration,"member":member}),
        )?,
        _ => serde_json::to_string(&serde_json::to_value(source)?)?,
    })
}
pub fn row_id(table: &Table, key: &str) -> Id {
    format!("{}::row:{:x}", table.id, Sha256::digest(key.as_bytes()))
}
pub fn owned_row_ids(table: &Table) -> Vec<Id> {
    if matches!(
        table.kind,
        TableKind::BillOfMaterials | TableKind::Holes { .. } | TableKind::WeldmentCutList { .. }
    ) {
        table
            .item_numbers
            .keys()
            .map(|key| row_id(table, key))
            .collect()
    } else {
        vec![]
    }
}
pub fn retarget_reserved_rows(
    table: &mut Table,
    ids: &BTreeMap<Id, Id>,
) -> Result<BTreeMap<Id, Id>> {
    let remap = |key: &str| -> Result<String> {
        let mut value: serde_json::Value = serde_json::from_str(key)?;
        crate::library::remap_owned_features(&mut value, ids, "")?;
        Ok(serde_json::to_string(&value)?)
    };
    let mut targets = BTreeMap::new();
    let mut numbers = BTreeMap::new();
    for (key, number) in &table.item_numbers {
        let next = remap(key)?;
        targets.insert(row_id(table, key), row_id(table, &next));
        if numbers.insert(next, *number).is_some() {
            return Err(invalid(
                "Retargeted table rows collide; reattach the table explicitly",
            ));
        }
    }
    let order = table
        .row_order
        .iter()
        .map(|key| remap(key))
        .collect::<Result<_>>()?;
    table.item_numbers = numbers;
    table.row_order = order;
    Ok(targets)
}
fn allocate(table: &Table, sources: &[RowSource]) -> Result<BTreeMap<String, u32>> {
    if sources.len() > MAX_RESERVATIONS {
        return Err(invalid(
            "Source rows exceed the saved reservation bound of 100000",
        ));
    }
    let mut numbers = table.item_numbers.clone();
    let mut next = numbers.values().max().copied().unwrap_or(0);
    let keys = sources
        .iter()
        .map(source_key)
        .collect::<Result<BTreeSet<_>>>()?;
    for key in keys {
        if let std::collections::btree_map::Entry::Vacant(entry) = numbers.entry(key) {
            next = next
                .checked_add(1)
                .ok_or_else(|| invalid("Table numbering exceeds the native range"))?;
            entry.insert(next);
        }
        if numbers.len() > MAX_RESERVATIONS {
            return Err(invalid(
                "A table supports at most 100000 retained row reservations; remove or replace obsolete table source explicitly",
            ));
        }
    }
    Ok(numbers)
}
fn source_content(
    table: &Table,
    headers: Vec<String>,
    mut entries: Vec<SourceRow>,
    reservations: BTreeMap<String, u32>,
) -> Content {
    let order: BTreeMap<_, _> = table
        .row_order
        .iter()
        .enumerate()
        .map(|(i, key)| (key.as_str(), i))
        .collect();
    entries.sort_by(|a, b| {
        order
            .get(a.key.as_str())
            .copied()
            .unwrap_or(usize::MAX)
            .cmp(&order.get(b.key.as_str()).copied().unwrap_or(usize::MAX))
            .then(a.number.cmp(&b.number))
            .then(a.key.cmp(&b.key))
    });
    Content {
        headers,
        rows: entries.iter().map(|r| r.cells.clone()).collect(),
        items: vec![],
        entries,
        reservations,
    }
}
fn transform_point(point: [f64; 3], matrix: [[f64; 4]; 4]) -> [f64; 3] {
    std::array::from_fn(|i| {
        matrix[i][0] * point[0] + matrix[i][1] * point[1] + matrix[i][2] * point[2] + matrix[i][3]
    })
}
fn transform_axis(axis: [f64; 3], matrix: [[f64; 4]; 4]) -> [f64; 3] {
    std::array::from_fn(|i| {
        matrix[i][0] * axis[0] + matrix[i][1] * axis[1] + matrix[i][2] * axis[2]
    })
}
fn unit_name(units: &crate::drawing::Units) -> &'static str {
    if *units == crate::drawing::Units::Inches {
        "in"
    } else {
        "mm"
    }
}
fn size(value: f64, units: &crate::drawing::Units, precision: usize) -> Result<String> {
    if !value.is_finite() {
        return Err(invalid("A table source quantity is nonfinite"));
    }
    let value = if *units == crate::drawing::Units::Inches {
        value / 25.4
    } else {
        value
    };
    Ok(format!("{value:.precision$}"))
}
fn length(doc: &Document, owner: &str, value: &crate::Expr) -> Result<f64> {
    let value = crate::expression::value(doc, owner, value, crate::Dimension::Length)?;
    if !value.is_finite() {
        return Err(invalid("Hole source has a nonfinite length"));
    }
    Ok(value)
}
pub fn hole_datum_position(
    doc: &Document,
    e: &Evaluation,
    sheet: &Drawing,
    table: &Table,
) -> Result<[f64; 3]> {
    let TableKind::Holes {
        features,
        instance_path,
        datum,
        ..
    } = &table.kind
    else {
        return Err(invalid("A datum is defined by a hole table"));
    };
    match datum {
        HoleDatum::Reference { point } => {
            crate::drawing::point(doc, e, sheet, [0.; 3], &Some(point.as_ref().clone()))
        }
        HoleDatum::ModelOrigin => {
            let owner = features
                .first()
                .ok_or_else(|| invalid("Hole table has no model source"))?;
            let (_, matrix) =
                crate::drawing::table_placement(doc, e, sheet, &owner.definition, instance_path)?;
            Ok(transform_point([0.; 3], matrix))
        }
    }
}
fn hole_content(doc: &Document, e: &Evaluation, sheet: &Drawing, table: &Table) -> Result<Content> {
    let TableKind::Holes {
        features,
        configuration,
        instance_path,
        view,
        units,
        precision,
        tag_prefix,
        ..
    } = &table.kind
    else {
        unreachable!()
    };
    if !crate::drawing::view_keys(sheet).contains(view) {
        return Err(invalid(
            "The hole coordinate view must be present on the sheet",
        ));
    }
    let owner = &features[0].definition;
    let (mesh_key, matrix) = crate::drawing::table_placement(doc, e, sheet, owner, instance_path)?;
    let represented_configuration = mesh_key.strip_prefix(&format!("{owner}@"));
    if configuration
        .as_deref()
        .is_some_and(|c| Some(c) != represented_configuration)
    {
        return Err(invalid(
            "Hole table configuration does not match its selected drawing occurrence",
        ));
    }
    let configured = crate::configuration::variant(doc, owner, represented_configuration)?;
    let part = configured
        .parts
        .get(owner)
        .ok_or_else(|| invalid("Hole table model scope is not a part"))?;
    let datum_position = hole_datum_position(doc, e, sheet, table)?;
    if datum_position.iter().any(|v| !v.is_finite()) {
        return Err(invalid("Hole table datum is nonfinite"));
    }
    let projected_datum = crate::drawing::project(view, datum_position)?;
    let mut potential = vec![];
    for reference in features {
        let feature = part
            .features
            .iter()
            .find(|f| f.id == reference.node)
            .ok_or_else(|| invalid("A hole table feature was removed; reattach it explicitly"))?;
        match &feature.operation {
            crate::Operation::Hole { .. } => {
                admit_rows(potential.len(), 1, MAX_RESERVATIONS)?;
                potential.push(RowSource::Hole {
                    feature: reference.clone(),
                    configuration: represented_configuration.map(str::to_string),
                    point: None,
                });
            }
            crate::Operation::DrilledHoles { definition } => {
                admit_rows(potential.len(), definition.points.len(), MAX_RESERVATIONS)?;
                for point in &definition.points {
                    potential.push(RowSource::Hole {
                        feature: reference.clone(),
                        configuration: represented_configuration.map(str::to_string),
                        point: Some(point.clone()),
                    });
                }
            }
            _ => {
                return Err(invalid(
                    "Hole table sources must be designed Hole or DrilledHoles features",
                ));
            }
        }
    }
    let reservations = allocate(table, &potential)?;
    let mut entries = vec![];
    for source in potential {
        let RowSource::Hole {
            feature: reference,
            point,
            ..
        } = &source
        else {
            unreachable!()
        };
        let feature = part
            .features
            .iter()
            .find(|f| f.id == reference.node)
            .ok_or_else(|| invalid("Hole feature is missing"))?;
        if feature.suppressed {
            continue;
        }
        admit_rows(entries.len(), 1, 10000)?;
        let (center, axis, diameter, depth, detail) = match &feature.operation {
            crate::Operation::Hole {
                origin,
                axis,
                radius,
                depth,
                ..
            } => (
                *origin,
                *axis,
                length(&configured, owner, radius)? * 2.,
                format!(
                    "{} {}",
                    size(length(&configured, owner, depth)?, units, *precision)?,
                    unit_name(units)
                ),
                "Simple blind hole".into(),
            ),
            crate::Operation::DrilledHoles { definition } => {
                let position_key = if definition.positions.definition == *owner {
                    mesh_key.clone()
                } else {
                    crate::configuration::key(
                        &definition.positions.definition,
                        configured
                            .active_configurations
                            .get(&definition.positions.definition)
                            .map(String::as_str),
                    )
                };
                let positions = e
                    .sketches
                    .get(&format!("{position_key}::{}", definition.positions.node))
                    .ok_or_else(|| {
                        invalid("The configured hole-position sketch has no current solved points")
                    })?;
                if positions.solved.as_ref().is_some_and(|source| {
                    point.as_ref().is_some_and(|point| {
                        crate::sketch_transform::suppressed_points(source).contains(point)
                    })
                }) {
                    return Err(invalid(
                        "A designed hole position is suppressed in the current configuration",
                    ));
                }
                let point = point
                    .as_ref()
                    .and_then(|id| positions.points.get(id))
                    .ok_or_else(|| invalid("A designed hole center was removed or suppressed"))?;
                let normal = nalgebra::Vector3::from(positions.plane.u)
                    .cross(&nalgebra::Vector3::from(positions.plane.v));
                if !normal.norm().is_finite() || normal.norm() < 1e-9 {
                    return Err(invalid("The hole position sketch has an invalid plane"));
                }
                let axis = normal.normalize() * if definition.reverse { 1. } else { -1. };
                let offset = length(&configured, owner, &definition.start_offset)?;
                let center = std::array::from_fn(|i| {
                    positions.plane.origin[i]
                        + positions.plane.u[i] * point[0]
                        + positions.plane.v[i] * point[1]
                        + axis[i] * offset
                });
                let diameter = length(&configured, owner, definition.bore_diameter())?;
                let depth = match definition.end {
                    crate::holes::End::ThroughAll => "THRU".into(),
                    crate::holes::End::Blind => format!(
                        "{} {}",
                        size(
                            length(&configured, owner, &definition.depth)?,
                            units,
                            *precision
                        )?,
                        unit_name(units)
                    ),
                };
                let mut detail = match &definition.head {
                    crate::holes::Head::Simple => "Simple hole".into(),
                    crate::holes::Head::Counterbore { diameter, depth } => format!(
                        "CBORE Ø{} x {} {}",
                        size(length(&configured, owner, diameter)?, units, *precision)?,
                        size(length(&configured, owner, depth)?, units, *precision)?,
                        unit_name(units)
                    ),
                    crate::holes::Head::Countersink { diameter, angle } => format!(
                        "CSINK Ø{} {} x {:.*}°",
                        size(length(&configured, owner, diameter)?, units, *precision)?,
                        unit_name(units),
                        *precision,
                        crate::expression::value(
                            &configured,
                            owner,
                            angle,
                            crate::Dimension::Angle
                        )?
                        .to_degrees()
                    ),
                };
                if let Some(thread) = &definition.thread {
                    let thread = thread.evaluate(&configured, owner)?;
                    detail = format!(
                        "{detail}; COSMETIC Ø{} P{} {} {}",
                        size(thread.nominal_diameter_mm, units, *precision)?,
                        size(thread.pitch_mm, units, *precision)?,
                        thread.handedness.abbreviation(),
                        match thread.depth_mm {
                            Some(depth) => format!(
                                "DEPTH {} {} BELOW HEAD",
                                size(depth, units, *precision)?,
                                unit_name(units)
                            ),
                            None => "THRU".into(),
                        }
                    );
                }
                (center, axis.into(), diameter, depth, detail)
            }
            _ => unreachable!(),
        };
        let center = transform_point(center, matrix);
        let axis = nalgebra::Vector3::from(transform_axis(axis, matrix));
        if center.iter().any(|v| !v.is_finite())
            || !axis.norm().is_finite()
            || axis.norm() < 1e-9
            || diameter <= 0.
        {
            return Err(invalid(
                "Designed hole has invalid center, axis or diameter",
            ));
        }
        let projected_axis = crate::drawing::project(view, axis.normalize().into())?;
        if projected_axis[0].hypot(projected_axis[1]) > 1e-6 {
            return Err(crate::CadError::Unsupported("Hole table coordinates require hole axes normal to their selected Front, Top or Right view".into()));
        }
        let projected = crate::drawing::project(view, center)?;
        let key = source_key(&source)?;
        let number = reservations[&key];
        entries.push(SourceRow {
            key,
            number,
            source,
            cells: vec![
                format!("{tag_prefix}{number}"),
                feature.name.clone(),
                size(projected[0] - projected_datum[0], units, *precision)?,
                size(projected[1] - projected_datum[1], units, *precision)?,
                size(diameter, units, *precision)?,
                depth,
                detail,
            ],
            position_mm: Some(center),
            quantity: None,
        });
    }
    let unit = unit_name(units);
    Ok(source_content(
        table,
        vec![
            "Tag".into(),
            "Feature".into(),
            format!("X ({unit})"),
            format!("Y ({unit})"),
            format!("Ø ({unit})"),
            "Depth".into(),
            "Hole / head / thread".into(),
        ],
        entries,
        reservations,
    ))
}
fn potential_members(
    doc: &Document,
    output: &NodeRef,
    limit: usize,
) -> Result<Vec<crate::weldment::MemberRef>> {
    let mut pending = vec![output.clone()];
    let mut seen = BTreeSet::new();
    let mut members = BTreeSet::new();
    while let Some(reference) = pending.pop() {
        if !seen.contains(&reference) {
            admit_rows(seen.len(), 1, MAX_RESERVATIONS)?;
        }
        if !seen.insert(reference.clone()) {
            continue;
        }
        let feature = doc
            .parts
            .get(&reference.definition)
            .and_then(|p| p.features.iter().find(|f| f.id == reference.node))
            .ok_or_else(|| invalid("Cut-list source was removed; reattach it explicitly"))?;
        let crate::Operation::Weldment { operation } = &feature.operation else {
            return Err(invalid("Cut-list sources must be native weldment outputs"));
        };
        pending.extend(operation.consumed_outputs());
        if matches!(
            operation.as_ref(),
            crate::weldment::WeldmentOperation::EndCap { .. }
        ) {
            let candidate = crate::weldment::MemberRef {
                group: reference.clone(),
                member: "cap".into(),
            };
            if !members.contains(&candidate) {
                admit_rows(members.len(), 1, limit)?;
                members.insert(candidate);
            }
        }
        if let crate::weldment::WeldmentOperation::Members {
            path,
            members: selected,
            ..
        } = operation.as_ref()
        {
            let part = doc
                .parts
                .get(&path.definition)
                .ok_or_else(|| invalid("Cut-list source path part is missing"))?;
            let profile = part
                .features
                .iter()
                .find(|feature| feature.id == path.node)
                .ok_or_else(|| invalid("Cut-list source path profile is missing"))?;
            let crate::Operation::Profile { profile, .. } = &profile.operation else {
                return Err(invalid(
                    "Cut-list source path must reference a native profile",
                ));
            };
            let mut reserve = |member: Id| -> Result<()> {
                let candidate = crate::weldment::MemberRef {
                    group: reference.clone(),
                    member,
                };
                if !members.contains(&candidate) {
                    admit_rows(members.len(), 1, limit)?;
                    members.insert(candidate);
                }
                Ok(())
            };
            if !selected.is_empty() {
                // Explicit source identities also cover stable legacy path members.
                for member in selected {
                    reserve(member.clone())?;
                }
                continue;
            }
            // Mirror extrusion::source identity materialization without deriving
            // geometry. Nonlinear IDs may remain inactive reservations: only the
            // evaluated native straight-member metadata creates cut-list rows.
            match profile {
                crate::Profile::Sketch { sketch } => {
                    let sketch = part
                        .sketches
                        .get(sketch)
                        .ok_or_else(|| invalid("Cut-list source path sketch is missing"))?;
                    for member in sketch.members.keys() {
                        reserve(member.clone())?;
                    }
                    let (prefix, count) = if sketch.profile_order.is_empty() {
                        ("legacy-segment", sketch.segments.len())
                    } else {
                        ("legacy-profile-edge", sketch.profile_order.len())
                    };
                    for index in 0..count {
                        reserve(format!("{prefix}-{index}"))?;
                    }
                    for key in sketch.circles.keys() {
                        reserve(format!("legacy-circle-{key}"))?;
                    }
                }
                crate::Profile::Polygon { points } => {
                    for index in 0..points.len() {
                        reserve(format!("legacy-profile-edge-{index}"))?;
                    }
                }
                crate::Profile::Circle { .. } => reserve("circle".into())?,
            }
        }
    }
    Ok(members.into_iter().collect())
}
fn cut_list_content(
    doc: &Document,
    e: &Evaluation,
    sheet: &Drawing,
    table: &Table,
) -> Result<Content> {
    let TableKind::WeldmentCutList {
        scope,
        configuration,
        sources,
        units,
        precision,
    } = &table.kind
    else {
        unreachable!()
    };
    let scoped = crate::drawing::scoped_document(doc, e, sheet);
    if scope != &sheet.definition && !doc.parts.contains_key(scope) {
        return Err(invalid(
            "Cut-list scope must be the sheet model or an included part",
        ));
    }
    if scope == &sheet.definition
        && doc.assemblies.contains_key(scope)
        && configuration
            .as_ref()
            .is_some_and(|c| Some(c) != doc.active_configurations.get(scope))
    {
        return Err(invalid(
            "Cut-list assembly configuration must match the current drawing scope",
        ));
    }
    let instances = crate::configuration::placed_parts(&scoped, Some(&e.assemblies))?;
    let represented: BTreeSet<_> = instances
        .into_iter()
        .filter(|i| {
            !i.bom_excluded
                && (scope == &sheet.definition || &i.definition == scope)
                && (scope != &sheet.definition
                    || doc.assemblies.contains_key(scope)
                    || &i.definition == scope)
        })
        .map(|i| (i.definition, i.configuration))
        .filter(|(definition, c)| {
            scope != definition || configuration.is_none() || c == configuration
        })
        .collect();
    if represented.is_empty() {
        return Err(invalid(
            "Cut-list model/configuration is not represented in the drawing scope",
        ));
    }
    for source in sources {
        if !represented
            .iter()
            .any(|(definition, _)| definition == &source.definition)
        {
            return Err(invalid(
                "A selected cut-list output is outside its represented model scope",
            ));
        }
    }
    let mut potential = vec![];
    for (definition, config) in &represented {
        let part = &doc.parts[definition];
        let outputs: Box<dyn Iterator<Item = NodeRef> + '_> = if sources.is_empty() {
            Box::new(
                part.features
                    .iter()
                    .filter(|f| {
                        part.outputs.contains(&f.id)
                            && matches!(f.operation, crate::Operation::Weldment { .. })
                    })
                    .map(|f| part.reference(&f.id)),
            )
        } else {
            Box::new(
                sources
                    .iter()
                    .filter(|s| &s.definition == definition)
                    .cloned(),
            )
        };
        for output in outputs {
            let members = potential_members(
                doc,
                &output,
                MAX_RESERVATIONS.saturating_sub(potential.len()),
            )?;
            admit_rows(potential.len(), members.len(), MAX_RESERVATIONS)?;
            for member in members {
                potential.push(RowSource::CutList {
                    output: output.clone(),
                    configuration: config.clone(),
                    member,
                });
            }
        }
        // The evaluated configured frontier can differ when a saved output is suppressed.
        let effective = crate::configuration::effective_part(doc, definition, config.as_deref())?;
        for output in effective.features.iter().filter(|f| {
            effective.outputs.contains(&f.id)
                && matches!(f.operation, crate::Operation::Weldment { .. })
                && (sources.is_empty() || sources.contains(&effective.reference(&f.id)))
        }) {
            let reference = effective.reference(&output.id);
            if let Some(stock) = e.weldments.get(&format!(
                "{}::{}",
                crate::configuration::key(definition, config.as_deref()),
                output.id
            )) {
                if stock.version != crate::weldment::CONTRACT_VERSION || stock.source != reference {
                    return Err(invalid(
                        "Native cut-list cache contract/source does not match its configured output",
                    ));
                }
                admit_rows(
                    potential.len(),
                    stock.members.len() + stock.accessories.len(),
                    MAX_RESERVATIONS,
                )?;
                potential.extend(stock.members.iter().map(|m| RowSource::CutList {
                    output: reference.clone(),
                    configuration: config.clone(),
                    member: m.reference.clone(),
                }));
                potential.extend(
                    stock
                        .accessories
                        .iter()
                        .map(|accessory| RowSource::CutList {
                            output: reference.clone(),
                            configuration: config.clone(),
                            member: accessory.reference.clone(),
                        }),
                );
            }
        }
    }
    // Keep authored member reservations even when a stock group is temporarily
    // absent, while current grouped rows retain all of their exact occurrences.
    let mut reservations = allocate(table, &potential)?;
    let rows = crate::manufacturing::native_cut_list_for_drawing(
        &scoped,
        e,
        &represented,
        sources,
        10000,
    )?;
    let groups = crate::cut_list::group(&scoped, e, rows, &table.cut_list_options)?;
    let current_sources: Vec<_> = groups
        .iter()
        .map(|group| RowSource::StockGroup {
            key: group.key.clone(),
            sources: group.sources.clone(),
        })
        .collect();
    let mut reserved_table = table.clone();
    reserved_table.item_numbers = reservations;
    reservations = allocate(&reserved_table, &current_sources)?;
    let first_sources: Vec<_> = groups
        .iter()
        .filter_map(|group| group.sources.first().cloned())
        .collect();
    let positions = crate::cut_list::source_points(&scoped, e, &first_sources)?;
    let mut entries = vec![];
    for group in groups {
        let number = reservations[&group.key];
        let mut cells = crate::cut_list::cells(
            &group,
            &table.cut_list_options.columns,
            number,
            *units,
            *precision,
        );
        // Display authored profile names, retaining the portable source refs in
        // the row contract rather than exposing opaque IDs as the primary UI.
        for (index, column) in table.cut_list_options.columns.iter().enumerate() {
            if *column == crate::cut_list::Column::Section
                && let Some(section) = &group.row.section
                && let Some(part) = doc.parts.get(&section.definition)
                && let Some(name) = part
                    .features
                    .iter()
                    .find(|f| f.id == section.node)
                    .map(|f| &f.name)
                    .or_else(|| part.sketches.get(&section.node).map(|s| &s.name))
            {
                cells[index] = name.clone();
            }
        }
        let position_mm = group
            .sources
            .first()
            .and_then(|source| positions.get(source))
            .copied();
        entries.push(SourceRow {
            key: group.key.clone(),
            number,
            source: RowSource::StockGroup {
                key: group.key,
                sources: group.sources,
            },
            cells,
            position_mm,
            quantity: Some(group.quantity),
        });
    }
    let unit = unit_name(units);
    let headers = table
        .cut_list_options
        .columns
        .iter()
        .map(|column| match column {
            crate::cut_list::Column::Length
            | crate::cut_list::Column::BendRadius
            | crate::cut_list::Column::StockWidth
            | crate::cut_list::Column::StockHeight
            | crate::cut_list::Column::Thickness => format!("{} ({unit})", column.label()),
            crate::cut_list::Column::StartCutAngle
            | crate::cut_list::Column::EndCutAngle
            | crate::cut_list::Column::BendSweep => format!("{} (deg)", column.label()),
            _ => column.label().to_string(),
        })
        .collect();
    Ok(source_content(table, headers, entries, reservations))
}
/// Follow the same drawn occurrence placement as reference-based dimensions,
/// including a saved exploded state. Manufacturing quantities stay assembled.
pub fn balloon_source_point(
    doc: &Document,
    e: &Evaluation,
    sheet: &Drawing,
    source: &crate::cut_list::Source,
) -> Result<[f64; 3]> {
    let scoped = crate::drawing::scoped_document(doc, e, sheet);
    let point = crate::cut_list::source_point(&scoped, e, source)?;
    let instance = crate::configuration::placed_parts(&scoped, Some(&e.assemblies))?
        .into_iter()
        .find(|instance| {
            instance.path == source.occurrence_path
                && instance.definition == source.output.definition
                && instance.configuration == source.configuration
        })
        .ok_or_else(|| invalid("Balloon source occurrence/configuration is absent"))?;
    let original = nalgebra::Matrix4::from_fn(|row, column| instance.matrix[row][column]);
    let local = original
        .try_inverse()
        .ok_or_else(|| invalid("Balloon occurrence has a singular placement"))?
        * nalgebra::Vector4::new(point[0], point[1], point[2], 1.);
    let (_, matrix) = crate::drawing::table_placement(
        doc,
        e,
        sheet,
        &source.output.definition,
        &source.occurrence_path,
    )?;
    let drawn = nalgebra::Matrix4::from_fn(|row, column| matrix[row][column]) * local;
    if !drawn.iter().all(|coordinate| coordinate.is_finite()) || (drawn.w - 1.).abs() > 1e-8 {
        return Err(invalid(
            "Balloon source produced an invalid paper-view placement",
        ));
    }
    Ok([drawn.x, drawn.y, drawn.z])
}
pub fn reference_occurrence(reference: &crate::drawing::PointReference) -> (&str, &[Id]) {
    use crate::drawing::{CurveReference, PointReference};
    match reference {
        PointReference::SketchPoint {
            sketch,
            instance_path,
            ..
        } => (&sketch.definition, instance_path),
        PointReference::EdgePoint {
            edge,
            instance_path,
            ..
        } => (&edge.producer.definition, instance_path),
        PointReference::FaceOrigin {
            face,
            instance_path,
            ..
        } => (&face.producer.definition, instance_path),
        PointReference::CurveCenter { curve } => match curve.as_ref() {
            CurveReference::SketchMember {
                sketch,
                instance_path,
                ..
            } => (&sketch.definition, instance_path),
            CurveReference::ModelEdge {
                edge,
                instance_path,
                ..
            } => (&edge.producer.definition, instance_path),
        },
    }
}
pub fn reference_matches_source(
    reference: &crate::drawing::PointReference,
    source: &crate::cut_list::Source,
) -> bool {
    let (definition, path) = reference_occurrence(reference);
    definition == source.output.definition && path == source.occurrence_path
}
pub fn reference_matches_item(
    doc: &Document,
    reference: &crate::drawing::PointReference,
    item: &Item,
) -> bool {
    let (definition, path) = reference_occurrence(reference);
    item.occurrences.iter().any(|occurrence| {
        if doc.assemblies.contains_key(&item.definition) {
            path.starts_with(&occurrence.path)
        } else {
            path == occurrence.path && definition == item.definition
        }
    })
}
pub fn row_contains_source(row: &SourceRow, source: &crate::cut_list::Source) -> bool {
    match &row.source {
        RowSource::StockGroup { sources, .. } => sources.contains(source),
        RowSource::CutList {
            output,
            configuration,
            member,
        } => {
            output == &source.output
                && configuration == &source.configuration
                && member == &source.member
        }
        _ => false,
    }
}
pub fn row_quantity(row: &SourceRow) -> usize {
    row.quantity.unwrap_or(1)
}
pub fn auto_cut_list_balloons(
    doc: &Document,
    e: &Evaluation,
    sheet: &Drawing,
    table: &Table,
    view: &str,
) -> Result<Vec<crate::drawing::Annotation>> {
    if !matches!(table.kind, TableKind::WeldmentCutList { .. }) {
        return Err(invalid(
            "Auto cut-list balloons require a native weldment cut-list table",
        ));
    }
    if !crate::drawing::view_keys(sheet)
        .iter()
        .any(|key| key == view)
    {
        return Err(invalid("Choose a present drawing view"));
    }
    let content = content_for_sheet(doc, e, sheet, table)?;
    let mut balloons = vec![];
    for row in content.entries {
        if sheet.entities.iter().any(|entity|entity.view==view&&matches!(&entity.kind,crate::drawing::AnnotationKind::CutListBalloon{table:existing,row_key,..} if existing==&table.id&&row_key==&row.key)) {continue;}
        let RowSource::StockGroup { sources, .. } = &row.source else {
            continue;
        };
        let source = sources
            .first()
            .ok_or_else(|| invalid("A current stock row has no exact source occurrence"))?
            .clone();
        let point = balloon_source_point(doc, e, sheet, &source)?;
        balloons.push(crate::drawing::Annotation {
            id: crate::id(),
            view: view.into(),
            offset_mm: [20., 15. + (balloons.len() % 5) as f64 * 8.],
            leader_points_mm: vec![],
            kind: crate::drawing::AnnotationKind::CutListBalloon {
                target: crate::drawing::Anchor {
                    point,
                    reference: None,
                },
                table: table.id.clone(),
                row_key: row.key,
                source,
                show_quantity: true,
            },
        });
    }
    Ok(balloons)
}

pub fn content_for_sheet(
    doc: &Document,
    evaluation: &Evaluation,
    sheet: &Drawing,
    table: &Table,
) -> Result<Content> {
    validate(table)?;
    validate_sources(doc, table)?;
    let result = match &table.kind {
        TableKind::Holes { .. } => hole_content(doc, evaluation, sheet, table),
        TableKind::WeldmentCutList { .. } => cut_list_content(doc, evaluation, sheet, table),
        _ => content(
            &crate::drawing::scoped_document(doc, evaluation, sheet),
            evaluation,
            table,
        ),
    }?;
    if result.rows.len() > 10000 {
        return Err(invalid("A table supports at most 10000 source rows"));
    }
    Ok(result)
}
pub fn pin_source_rows(
    doc: &Document,
    evaluation: &Evaluation,
    sheet: &Drawing,
    table: &mut Table,
) -> Result<()> {
    let content = content_for_sheet(doc, evaluation, sheet, table)?;
    if matches!(table.kind, TableKind::BillOfMaterials) {
        pin_item_numbers(
            &crate::drawing::scoped_document(doc, evaluation, sheet),
            evaluation,
            table,
        )?;
    }
    let mut ordered: BTreeSet<_> = table.row_order.iter().cloned().collect();
    for (key, number) in content.reservations {
        table.item_numbers.insert(key.clone(), number);
        if ordered.insert(key.clone()) {
            table.row_order.push(key);
        }
    }
    Ok(())
}
/// Reconcile source identities in the same transaction as a model edit. Geometry
/// and disposable evaluations are not authority for assigning a new row number.
pub fn reconcile_reservations(doc: &mut Document) -> Result<()> {
    if !doc.drawings.iter().flat_map(|s| &s.tables).any(|t| {
        matches!(
            t.kind,
            TableKind::BillOfMaterials
                | TableKind::Holes { .. }
                | TableKind::WeldmentCutList { .. }
        )
    }) {
        return Ok(());
    }
    let mut scoped = doc.clone();
    let mut placements: BTreeMap<Id, Vec<crate::PlacedPart>> = BTreeMap::new();
    let mut updates = vec![];
    for (sheet_index, sheet) in doc.drawings.iter().enumerate() {
        if !sheet.tables.iter().any(|t| {
            matches!(
                t.kind,
                TableKind::BillOfMaterials
                    | TableKind::Holes { .. }
                    | TableKind::WeldmentCutList { .. }
            )
        }) {
            continue;
        }
        if let std::collections::btree_map::Entry::Vacant(entry) =
            placements.entry(sheet.definition.clone())
        {
            scoped.root = sheet.definition.clone();
            entry.insert(scoped.placed_parts()?);
        }
        let instances = &placements[&sheet.definition];
        for (table_index, table) in sheet.tables.iter().enumerate() {
            validate(table)?;
            let mut sources = vec![];
            match &table.kind {
                TableKind::BillOfMaterials => {
                    let mut source_scope = doc.clone();
                    source_scope.root = sheet.definition.clone();
                    let rows = crate::bom::rows(&source_scope, &table.bom_options)?;
                    let mut numbers = table.item_numbers.clone();
                    let mut next = numbers.values().copied().max().unwrap_or(0);
                    let mut order = table.row_order.clone();
                    let mut seen: BTreeSet<_> = order.iter().cloned().collect();
                    for row in rows {
                        if !numbers.contains_key(&row.key) {
                            next = next
                                .checked_add(1)
                                .ok_or_else(|| invalid("BOM numbering exceeds the native range"))?;
                            numbers.insert(row.key.clone(), next);
                        }
                        if seen.insert(row.key.clone()) {
                            order.push(row.key);
                        }
                    }
                    if numbers.len() > MAX_RESERVATIONS {
                        return Err(invalid("BOM reservations exceed their bounded limit"));
                    }
                    updates.push((sheet_index, table_index, numbers, order));
                    continue;
                }
                TableKind::Holes {
                    features,
                    configuration,
                    instance_path,
                    ..
                } => {
                    let Some(first) = features.first() else {
                        continue;
                    };
                    let candidates: Vec<_> = instances
                        .iter()
                        .filter(|i| {
                            i.definition == first.definition
                                && (instance_path.is_empty() || &i.path == instance_path)
                        })
                        .collect();
                    if candidates.len() != 1 {
                        continue;
                    }
                    let represented = &candidates[0].configuration;
                    if configuration
                        .as_ref()
                        .is_some_and(|c| Some(c) != represented.as_ref())
                    {
                        continue;
                    }
                    let part = crate::configuration::effective_part(
                        doc,
                        &first.definition,
                        represented.as_deref(),
                    )?;
                    for reference in features {
                        let Some(feature) = part.features.iter().find(|f| f.id == reference.node)
                        else {
                            continue;
                        };
                        match &feature.operation {
                            crate::Operation::Hole { .. } => {
                                admit_rows(sources.len(), 1, MAX_RESERVATIONS)?;
                                sources.push(RowSource::Hole {
                                    feature: reference.clone(),
                                    configuration: represented.clone(),
                                    point: None,
                                });
                            }
                            crate::Operation::DrilledHoles { definition } => {
                                admit_rows(
                                    sources.len(),
                                    definition.points.len(),
                                    MAX_RESERVATIONS,
                                )?;
                                sources.extend(definition.points.iter().map(|point| {
                                    RowSource::Hole {
                                        feature: reference.clone(),
                                        configuration: represented.clone(),
                                        point: Some(point.clone()),
                                    }
                                }));
                            }
                            _ => {}
                        }
                    }
                }
                TableKind::WeldmentCutList {
                    scope,
                    configuration,
                    sources: selected,
                    ..
                } => {
                    let represented: BTreeSet<_> = instances
                        .iter()
                        .filter(|i| {
                            !i.bom_excluded
                                && (scope == &sheet.definition || &i.definition == scope)
                                && (&i.definition != scope
                                    || configuration.is_none()
                                    || &i.configuration == configuration)
                        })
                        .map(|i| (i.definition.clone(), i.configuration.clone()))
                        .collect();
                    for (definition, config) in represented {
                        let Some(part) = doc.parts.get(&definition) else {
                            continue;
                        };
                        let effective = crate::configuration::effective_part(
                            doc,
                            &definition,
                            config.as_deref(),
                        )?;
                        let candidates: Box<dyn Iterator<Item = NodeRef> + '_> =
                            if selected.is_empty() {
                                Box::new(
                                    part.features
                                        .iter()
                                        .chain(&effective.features)
                                        .filter(|f| {
                                            (part.outputs.contains(&f.id)
                                                || effective.outputs.contains(&f.id))
                                                && matches!(
                                                    f.operation,
                                                    crate::Operation::Weldment { .. }
                                                )
                                        })
                                        .map(|f| part.reference(&f.id)),
                                )
                            } else {
                                Box::new(
                                    selected
                                        .iter()
                                        .filter(|s| s.definition == definition)
                                        .cloned(),
                                )
                            };
                        let mut outputs = BTreeSet::new();
                        for output in candidates {
                            if !outputs.contains(&output) {
                                admit_rows(outputs.len(), 1, MAX_RESERVATIONS)?;
                                outputs.insert(output);
                            }
                        }
                        for output in outputs {
                            let members = potential_members(
                                doc,
                                &output,
                                MAX_RESERVATIONS.saturating_sub(sources.len()),
                            )?;
                            admit_rows(sources.len(), members.len(), MAX_RESERVATIONS)?;
                            sources.extend(members.into_iter().map(|member| RowSource::CutList {
                                output: output.clone(),
                                configuration: config.clone(),
                                member,
                            }));
                        }
                    }
                }
                _ => continue,
            }
            let numbers = allocate(table, &sources)?;
            let mut order = table.row_order.clone();
            let mut ordered: BTreeSet<_> = order.iter().cloned().collect();
            for key in numbers.keys() {
                if ordered.insert(key.clone()) {
                    order.push(key.clone());
                }
            }
            updates.push((sheet_index, table_index, numbers, order));
        }
    }
    for (sheet, table, numbers, order) in updates {
        doc.drawings[sheet].tables[table].item_numbers = numbers;
        doc.drawings[sheet].tables[table].row_order = order;
    }
    Ok(())
}
fn line(out: &mut Vec<Primitive>, a: [f64; 2], b: [f64; 2]) {
    out.push(Primitive::Polyline {
        points: vec![a, b],
        dashed: false,
    });
}
fn cell_lines(value: &str, width: f64, size: f64) -> Vec<String> {
    let capacity = ((width - 2.) / size).floor().max(1.) as usize;
    let mut lines = Vec::new();
    let mut current = String::new();
    let mut length = 0;
    for source in value.lines() {
        for c in source.chars() {
            if length >= capacity {
                lines.push(std::mem::take(&mut current));
                length = 0;
            }
            current.push(c);
            length += 1;
        }
        lines.push(std::mem::take(&mut current));
        length = 0;
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}
/// Lay out full rows using exactly the same wrapping and row-height rules as
/// final vectors. Every page is emitted by drawing::sheets; no clipping occurs.
pub fn paginate(
    doc: &Document,
    evaluation: &Evaluation,
    sheet: &Drawing,
    table: &mut Table,
    first_page: u32,
) -> Result<()> {
    if first_page > 1023 {
        return Err(invalid("At most 1024 pages are supported"));
    }
    let content = content_for_sheet(doc, evaluation, sheet, table)?;
    let bounds = crate::drawing::paper_bounds(sheet);
    let position = table.position_mm;
    let max_y = if sheet.format.is_some() {
        bounds[1][1] - 8.
    } else {
        sheet.paper_mm[1] - 16.
    };
    let width: f64 = table.column_widths_mm.iter().sum();
    if position[0] < bounds[0][0]
        || position[1] < bounds[0][1]
        || position[0] + width > bounds[1][0]
    {
        return Err(invalid(
            "Move or resize the table to fit the paper width before paginating",
        ));
    }
    let height = |row: &[String]| -> f64 {
        table.row_height_mm.max(
            row.iter()
                .zip(&table.column_widths_mm)
                .map(|(cell, width)| cell_lines(cell, *width, table.text_size_mm).len())
                .max()
                .unwrap_or(1) as f64
                * table.text_size_mm
                * 1.4
                + 2.,
        )
    };
    let title_height = table.row_height_mm.max(
        cell_lines(&table.title, width, table.text_size_mm).len() as f64 * table.text_size_mm * 1.4
            + 2.,
    );
    let overhead = title_height + height(&content.headers);
    let available = max_y - position[1];
    if !available.is_finite() || overhead >= available {
        return Err(invalid(
            "Table title and headers do not fit this paper position",
        ));
    }
    let mut segments = vec![];
    let mut start = 0;
    let mut used = overhead;
    let mut page = first_page;
    for (index, row) in content.rows.iter().enumerate() {
        let row_height = height(row);
        if overhead + row_height > available {
            return Err(invalid(
                "A wrapped table row is taller than a complete page; widen its columns or reduce text size",
            ));
        }
        if used + row_height > available {
            segments.push(Segment {
                id: crate::id(),
                page,
                start_row: start,
                row_count: Some(index - start),
                position_mm: position,
                repeat_header: true,
            });
            start = index;
            used = overhead;
            page += 1;
            if page > 1023 {
                return Err(invalid("Table pagination exceeds 1024 pages"));
            }
        }
        used += row_height;
    }
    segments.push(Segment {
        id: crate::id(),
        page,
        start_row: start,
        row_count: None,
        position_mm: position,
        repeat_header: true,
    });
    // Preserve presentation identities for pages that remain after repagination.
    for segment in &mut segments {
        if let Some(old) = table
            .segments
            .iter()
            .find(|old| old.page == segment.page && old.start_row == segment.start_row)
        {
            segment.id = old.id.clone();
        }
    }
    table.segments = segments;
    Ok(())
}
/// Automatic paging preserves model interaction room instead of reserving a
/// full-height table underneath model views. Explicit page choices remain saved.
pub fn recommended_first_page(
    doc: &Document,
    e: &Evaluation,
    sheet: &Drawing,
    table: &Table,
) -> Result<u32> {
    if let Some(page) = table.pagination_start_page {
        return Ok(page);
    }
    // Prove the full original-page layout, including saved/aligned view
    // rectangles, other tables and the title block. A minimum cell-height
    // estimate alone cannot prove that an edited view still has room.
    let mut candidate = sheet.clone();
    let current = candidate
        .tables
        .iter_mut()
        .find(|item| item.id == table.id)
        .ok_or_else(|| invalid("The table is no longer on this sheet"))?;
    *current = table.clone();
    current.segments.clear();
    if crate::drawing::build_sheet(doc, e, &candidate).is_ok() {
        return Ok(0);
    }
    // Automatic continuations use fresh pages. Explicit page choices can
    // share paper, with normal collision validation and editable placement.
    let page = sheet
        .tables
        .iter()
        .filter(|other| other.id != table.id)
        .flat_map(|other| &other.segments)
        .map(|segment| segment.page)
        .max()
        .unwrap_or(0)
        .checked_add(1)
        .ok_or_else(|| invalid("Table continuation page count overflow"))?;
    if page > 1023 {
        return Err(invalid("Table pagination exceeds 1024 pages"));
    }
    Ok(page)
}

pub fn page_count(sheet: &Drawing) -> u32 {
    sheet
        .tables
        .iter()
        .flat_map(|t| &t.segments)
        .map(|s| s.page)
        .max()
        .unwrap_or(0)
        .saturating_add(1)
        .min(1024)
}
fn validate_segments(table: &Table, rows: usize) -> Result<()> {
    if table.segments.is_empty() {
        return Ok(());
    }
    let mut next = 0;
    for (i, segment) in table.segments.iter().enumerate() {
        if segment.start_row != next
            || (segment.row_count.is_none() && i + 1 != table.segments.len())
        {
            return Err(invalid(
                "Table segments must cover every ordered row once in consecutive ranges; only the last may include remaining rows",
            ));
        }
        let count = segment.row_count.unwrap_or(rows.saturating_sub(next));
        next = next
            .checked_add(count)
            .ok_or_else(|| invalid("Segment range exceeds the native row bound"))?;
        if next > rows || (count == 0 && rows != 0) {
            return Err(invalid(
                "A table split references absent rows; repaginate after removing source rows",
            ));
        }
    }
    if next != rows {
        return Err(invalid(
            "Table splits must include every current row; repaginate the table",
        ));
    }
    Ok(())
}
pub fn geometries_for_page(
    doc: &Document,
    evaluation: &Evaluation,
    sheet: &Drawing,
    table: &Table,
    page: u32,
) -> Result<Vec<TableGeometry>> {
    let content = content_for_sheet(doc, evaluation, sheet, table)?;
    validate_segments(table, content.rows.len())?;
    if table.segments.is_empty() {
        return if page == 0 {
            Ok(vec![geometry_rows(
                sheet,
                table,
                table.position_mm,
                content.headers,
                content.rows,
                None,
            )?])
        } else {
            Ok(vec![])
        };
    }
    table
        .segments
        .iter()
        .filter(|segment| segment.page == page)
        .map(|segment| {
            let end = segment.start_row
                + segment
                    .row_count
                    .unwrap_or(content.rows.len() - segment.start_row);
            geometry_rows(
                sheet,
                table,
                segment.position_mm,
                if segment.repeat_header {
                    content.headers.clone()
                } else {
                    vec![]
                },
                content.rows[segment.start_row..end].to_vec(),
                Some(segment.id.clone()),
            )
        })
        .collect()
}
/// Explicit paper repair targets. These rectangles describe controls, not
/// manufacturing rows, and are never returned by strict table geometry/export.
pub fn repair_placeholders(sheet: &Drawing, table: &Table, page: u32) -> Vec<TableGeometry> {
    let positions: Vec<_> = if table.segments.is_empty() {
        if page == 0 {
            vec![(table.position_mm, None)]
        } else {
            vec![]
        }
    } else {
        table
            .segments
            .iter()
            .filter(|segment| segment.page == page)
            .map(|segment| (segment.position_mm, Some(segment.id.clone())))
            .collect()
    };
    let bounds = crate::drawing::paper_bounds(sheet);
    let width = (bounds[1][0] - bounds[0][0]).clamp(1., 80.);
    let height = (bounds[1][1] - bounds[0][1]).clamp(1., 18.);
    positions
        .into_iter()
        .map(|(position, segment)| {
            let min = std::array::from_fn(|axis| {
                position[axis].clamp(
                    bounds[0][axis],
                    (bounds[1][axis] - if axis == 0 { width } else { height }).max(bounds[0][axis]),
                )
            });
            let max = [min[0] + width, min[1] + height];
            let mut primitives = vec![Primitive::Polyline {
                points: vec![min, [max[0], min[1]], max, [min[0], max[1]], min],
                dashed: true,
            }];
            let size = 2.5_f64
                .min((width - 2.).max(0.1) / 13.)
                .min((height - 2.).max(0.1));
            primitives.push(Primitive::Text {
                at: [min[0] + 1., min[1] + height * 0.5 - size * 0.5],
                size,
                text: "TABLE: REPAIR".into(),
            });
            TableGeometry {
                primitives,
                bounds_mm: [min, max],
                reserve_space: false,
                segment,
            }
        })
        .collect()
}
/// Compatibility aggregate: only the source page's split segments. Layout uses
/// geometries_for_page so separate portions reserve separate rectangles.
pub fn geometry(
    doc: &Document,
    evaluation: &Evaluation,
    sheet: &Drawing,
    table: &Table,
) -> Result<TableGeometry> {
    let geometries = geometries_for_page(doc, evaluation, sheet, table, 0)?;
    let mut bounds = [[f64::INFINITY; 2], [f64::NEG_INFINITY; 2]];
    let mut primitives = vec![];
    for geometry in geometries {
        for (corner, values) in bounds.iter_mut().enumerate() {
            for (axis, value) in values.iter_mut().enumerate() {
                *value = if corner == 0 {
                    value.min(geometry.bounds_mm[corner][axis])
                } else {
                    value.max(geometry.bounds_mm[corner][axis])
                };
            }
        }
        primitives.extend(geometry.primitives);
    }
    if primitives.is_empty() {
        bounds = [table.position_mm; 2];
    }
    Ok(TableGeometry {
        primitives,
        bounds_mm: bounds,
        reserve_space: table.reserve_space,
        segment: None,
    })
}
fn geometry_rows(
    sheet: &Drawing,
    table: &Table,
    position: [f64; 2],
    headers: Vec<String>,
    cells: Vec<Vec<String>>,
    segment: Option<Id>,
) -> Result<TableGeometry> {
    let mut rows = if headers.is_empty() {
        vec![]
    } else {
        vec![headers]
    };
    rows.extend(cells);
    let width: f64 = table.column_widths_mm.iter().sum();
    let min = position;
    let bounds = crate::drawing::paper_bounds(sheet);
    let max_y = if sheet.format.is_some() {
        bounds[1][1] - 8.
    } else {
        sheet.paper_mm[1] - 16.
    };
    let title_lines = cell_lines(&table.title, width, table.text_size_mm);
    let title_height = table
        .row_height_mm
        .max(title_lines.len() as f64 * table.text_size_mm * 1.4 + 2.);
    let minimum_height = rows.len() as f64 * table.row_height_mm + title_height;
    if !width.is_finite()
        || !minimum_height.is_finite()
        || min[0] < bounds[0][0]
        || min[1] < bounds[0][1]
        || min[0] + width > bounds[1][0]
        || min[1] + minimum_height > max_y
    {
        return Err(invalid(format!(
            "{} exceeds the paper border; move/resize the table or enlarge the sheet",
            table.title
        )));
    }
    let cell_text = rows
        .iter()
        .map(|row| {
            row.iter()
                .zip(&table.column_widths_mm)
                .map(|(cell, width)| cell_lines(cell, *width, table.text_size_mm))
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let heights = cell_text
        .iter()
        .map(|row| {
            table.row_height_mm.max(
                row.iter().map(Vec::len).max().unwrap_or(1) as f64 * table.text_size_mm * 1.4 + 2.,
            )
        })
        .collect::<Vec<_>>();
    let height = heights.iter().sum::<f64>() + title_height;
    let max = [min[0] + width, min[1] + height];
    if !max.iter().all(|v| v.is_finite())
        || min[0] < bounds[0][0]
        || min[1] < bounds[0][1]
        || max[0] > bounds[1][0]
        || max[1] > max_y
    {
        return Err(invalid(format!(
            "{} exceeds the paper border; move/resize the table or enlarge the sheet",
            table.title
        )));
    }
    let mut out = Vec::new();
    line(&mut out, min, [max[0], min[1]]);
    line(&mut out, [max[0], min[1]], max);
    line(&mut out, max, [min[0], max[1]]);
    line(&mut out, [min[0], max[1]], min);
    let mut y = max[1] - title_height;
    line(&mut out, [min[0], y], [max[0], y]);
    for (index, title) in title_lines.into_iter().enumerate() {
        out.push(Primitive::Text {
            at: [
                min[0] + 1.,
                max[1] - 1. - table.text_size_mm - index as f64 * table.text_size_mm * 1.4,
            ],
            size: table.text_size_mm,
            text: title,
        });
    }
    let mut x = min[0];
    for column in &table.column_widths_mm[..table.column_widths_mm.len() - 1] {
        x += column;
        line(&mut out, [x, min[1]], [x, y]);
    }
    for (row, height) in cell_text.into_iter().zip(heights) {
        let mut x = min[0];
        for (lines, width) in row.into_iter().zip(&table.column_widths_mm) {
            for (index, value) in lines.into_iter().enumerate() {
                out.push(Primitive::Text {
                    at: [
                        x + 1.,
                        y - 1. - table.text_size_mm - index as f64 * table.text_size_mm * 1.4,
                    ],
                    size: table.text_size_mm,
                    text: value,
                });
            }
            x += width;
        }
        y -= height;
        line(&mut out, [min[0], y], [max[0], y]);
    }
    Ok(TableGeometry {
        primitives: out,
        bounds_mm: [min, max],
        reserve_space: table.reserve_space,
        segment,
    })
}

pub fn semantic_elements(
    sheet: &Drawing,
    state: &str,
) -> Vec<crate::presentation::SemanticElement> {
    let mut elements:Vec<_> = sheet.tables.iter().map(|table| crate::presentation::SemanticElement {
        id:table.id.clone(), role:"drawing-table".into(), label:table.title.clone(), value:None,
        state:state.into(), commands:vec!["drawing.sheet-edit".into()], relationships:vec![sheet.id.clone(),sheet.definition.clone()],
        data:serde_json::json!({"contract":"tcad.drawing-tables","version":CONTRACT_VERSION,"sheet_id":sheet.id,"table_id":table.id,"source":table,"paper_units":"mm","editable":true,"bom_scope":table.bom_options.mode,"bom_grouping":table.bom_options.grouping,"stock_grouping":table.cut_list_options.grouping,"page_count":page_count(sheet),"numbering":"saved_source_reservations","ordering":"saved_source_keys_then_reserved_number","hole_scope":"designed_features_solved_positions_one_occurrence_orthographic_datum","cut_list_scope":"native_member_and_accessory_occurrences_configured_material_finish_and_assembled_quantity"}),
    }).collect();
    for table in &sheet.tables {
        for segment in &table.segments {
            elements.push(crate::presentation::SemanticElement {id:segment.id.clone(),role:"drawing-table-segment".into(),label:format!("{} · page {}",table.title,segment.page+1),value:Some(format!("Rows {}–{}",segment.start_row+1,segment.row_count.map(|count|(segment.start_row+count).to_string()).unwrap_or_else(||"last".into()))),state:state.into(),commands:vec!["drawing.sheet-edit".into()],relationships:vec![sheet.id.clone(),table.id.clone()],data:serde_json::json!({"contract":"tcad.drawing-tables","version":CONTRACT_VERSION,"sheet_id":sheet.id,"table_id":table.id,"segment":segment,"paper_units":"mm","editable_fields":["page","position_mm","repeat_header"]})});
        }
    }
    elements
}
pub fn evaluated_semantic_rows(
    doc: &Document,
    evaluation: Option<&Evaluation>,
    state: &str,
) -> Vec<crate::presentation::SemanticElement> {
    let mut elements = vec![];
    for sheet in &doc.drawings {
        for table in sheet
            .tables
            .iter()
            .filter(|table| matches!(table.kind, TableKind::BillOfMaterials))
        {
            let current = evaluation.and_then(|e| content_for_sheet(doc, e, sheet, table).ok());
            let active: BTreeMap<_, _> = current
                .as_ref()
                .map(|content| {
                    content
                        .items
                        .iter()
                        .map(|row| (row.key.as_str(), row))
                        .collect()
                })
                .unwrap_or_default();
            let mut keys: BTreeSet<_> = table.item_numbers.keys().cloned().collect();
            keys.extend(active.keys().map(|key| key.to_string()));
            for key in keys {
                let row = active.get(key.as_str()).copied();
                let number = row
                    .map(|row| row.number)
                    .or_else(|| table.item_numbers.get(&key).copied());
                elements.push(crate::presentation::SemanticElement {id:row_id(table,&key),role:"drawing-table-row".into(),label:format!("BOM item {}",number.unwrap_or(0)),value:row.map(|row|row.cells.join(" · ")),state:if row.is_some(){state}else if current.is_some(){"suppressed"}else{"unavailable"}.into(),commands:vec!["drawing.sheet-edit".into()],relationships:vec![sheet.id.clone(),table.id.clone()].into_iter().chain(row.map(|row|row.definition.clone())).collect(),data:serde_json::json!({"contract":"tcad.drawing-tables","version":CONTRACT_VERSION,"sheet_id":sheet.id,"table_id":table.id,"key":key,"number":number,"definition":row.map(|row|&row.definition),"configuration":row.map(|row|&row.configuration),"depth":row.map(|row|row.depth),"occurrences":row.map(|row|&row.occurrences),"cells":row.map(|row|&row.cells),"reserved":true,"active":row.is_some(),"editable_fields":["number"]})});
            }
        }
        for table in sheet.tables.iter().filter(|t| {
            matches!(
                t.kind,
                TableKind::Holes { .. } | TableKind::WeldmentCutList { .. }
            )
        }) {
            let current = evaluation.and_then(|e| content_for_sheet(doc, e, sheet, table).ok());
            let active: BTreeMap<_, _> = current
                .as_ref()
                .map(|c| {
                    c.entries
                        .iter()
                        .map(|row| (row.key.as_str(), row))
                        .collect()
                })
                .unwrap_or_default();
            let mut keys: BTreeSet<_> = table.item_numbers.keys().cloned().collect();
            keys.extend(active.keys().map(|key| (*key).to_string()));
            for key in keys {
                let row = active.get(key.as_str()).copied();
                let number = row
                    .map(|r| r.number)
                    .or_else(|| table.item_numbers.get(&key).copied());
                let source = row
                    .and_then(|r| serde_json::to_value(&r.source).ok())
                    .or_else(|| serde_json::from_str::<serde_json::Value>(&key).ok());
                let relationships = row
                    .map(|r| match &r.source {
                        RowSource::Hole { feature, point, .. } => {
                            vec![sheet.id.clone(), table.id.clone(), feature.node.clone()]
                                .into_iter()
                                .chain(point.iter().cloned())
                                .collect()
                        }
                        RowSource::StockGroup { sources, .. } => {
                            vec![sheet.id.clone(), table.id.clone()]
                                .into_iter()
                                .chain(sources.iter().flat_map(|source| {
                                    [
                                        source.output.node.clone(),
                                        source.member.group.node.clone(),
                                        source.member.member.clone(),
                                    ]
                                }))
                                .collect()
                        }
                        RowSource::CutList { output, member, .. } => vec![
                            sheet.id.clone(),
                            table.id.clone(),
                            output.node.clone(),
                            member.group.node.clone(),
                            member.member.clone(),
                        ],
                    })
                    .unwrap_or_else(|| vec![sheet.id.clone(), table.id.clone()]);
                elements.push(crate::presentation::SemanticElement {
                    id:row_id(table,&key),role:"drawing-table-row".into(),label:if let TableKind::Holes {tag_prefix,..}=&table.kind {format!("Hole {tag_prefix}{}",number.unwrap_or(0))}else{format!("Cut-list item {}",number.unwrap_or(0))},
                    value:row.map(|r|r.cells.join(" · ")),state:if row.is_some(){state}else if current.is_some(){"suppressed"}else{"unavailable"}.into(),commands:vec!["drawing.sheet-edit".into()],relationships,
                    data:serde_json::json!({"contract":"tcad.drawing-tables","version":CONTRACT_VERSION,"sheet_id":sheet.id,"table_id":table.id,"key":key,"number":number,"source":source,"cells":row.map(|r|&r.cells),"position_mm":row.and_then(|r|r.position_mm),"reserved":true,"active":row.is_some(),"editable_fields":["number","order"]}),
                });
            }
        }
    }
    elements
}
