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
    let row = w
        .selected
        .as_ref()
        .and_then(|s| rows.iter().find(|r| r.definition == s.definition))
        .or_else(|| rows.first())
        .ok_or_else(|| {
            cad_core::CadError::Invalid("This drawing scope has no included BOM items".into())
        })?;
    Ok(AnnotationKind::Balloon {
        target,
        table: table.id.clone(),
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
    table.position_mm = cad_core::drawing::default_table_position(sheet);
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
