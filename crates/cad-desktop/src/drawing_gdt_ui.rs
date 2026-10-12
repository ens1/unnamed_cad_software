//! Compact, semantic datum-target and multi-row frame controls.
use crate::{Workspace, drawing_editor_ui::Fields};
use cad_core::{
    CadError, Drawing, Result,
    drawing::{AnnotationKind, GeometricCharacteristic, MaterialCondition, Units},
    drawing_gdt::{DatumReference, DatumTarget, FrameMode, FrameRow, TargetShape, ToleranceFrame},
};
use serde_json::{Value, json};
fn choice(label: &str, value: impl serde::Serialize) -> Value {
    json!({"label":label,"value":value})
}
fn characteristic_choices() -> Vec<Value> {
    [
        ("Position", GeometricCharacteristic::Position),
        ("Straightness", GeometricCharacteristic::Straightness),
        ("Flatness", GeometricCharacteristic::Flatness),
        ("Circularity", GeometricCharacteristic::Circularity),
        ("Cylindricity", GeometricCharacteristic::Cylindricity),
        ("Profile of line", GeometricCharacteristic::ProfileLine),
        (
            "Profile of surface",
            GeometricCharacteristic::ProfileSurface,
        ),
        ("Parallelism", GeometricCharacteristic::Parallelism),
        (
            "Perpendicularity",
            GeometricCharacteristic::Perpendicularity,
        ),
        ("Angularity", GeometricCharacteristic::Angularity),
        ("Circular runout", GeometricCharacteristic::CircularRunout),
        ("Total runout", GeometricCharacteristic::TotalRunout),
    ]
    .into_iter()
    .map(|(label, value)| choice(label, value))
    .collect()
}
fn material_choices() -> Vec<Value> {
    vec![
        choice("Regardless of size", MaterialCondition::Regardless),
        choice("Maximum material (M)", MaterialCondition::Maximum),
        choice("Least material (L)", MaterialCondition::Least),
    ]
}
pub(crate) fn target_fields(fields: &mut Fields<'_>, path: &str, symbol: &DatumTarget) {
    fields.group("datum-target", "Datum target");
    fields.text(&format!("{path}/identifier"), "Datum letter", false);
    fields.number(
        &format!("{path}/number"),
        "Target number",
        "",
        Some(1.),
        Some(999.),
    );
    fields.select(
        &format!("{path}/shape"),
        "Target marker",
        vec![
            choice("Point / X", TargetShape::PointCross),
            choice("Point / dot", TargetShape::PointDot),
            choice("Line", TargetShape::Line),
            choice("Circular area", TargetShape::Circle),
            choice("Rectangular area", TargetShape::Rectangle),
        ],
    );
    let unit = if symbol.units == Units::Inches {
        "in"
    } else {
        "mm"
    };
    match symbol.shape {
        TargetShape::PointCross | TargetShape::PointDot => {
            fields.toggle(&format!("{path}/show_size"), "State contact diameter");
            if symbol.show_size {
                fields.field(
                    &format!("{path}/diameter"),
                    "Contact diameter",
                    vec![],
                    Some(unit),
                );
            }
        }
        TargetShape::Line => fields.field(
            &format!("{path}/width"),
            "Contact line length",
            vec![],
            Some(unit),
        ),
        TargetShape::Circle => fields.field(
            &format!("{path}/diameter"),
            "Area diameter",
            vec![],
            Some(unit),
        ),
        TargetShape::Rectangle => {
            fields.field(&format!("{path}/width"), "Area width", vec![], Some(unit));
            fields.field(&format!("{path}/height"), "Area height", vec![], Some(unit));
        }
    }
    if matches!(symbol.shape, TargetShape::Line | TargetShape::Rectangle) {
        fields.field(
            &format!("{path}/rotation"),
            "Orientation in view",
            vec![],
            Some("deg"),
        );
    }
    fields.note("datum-target-coordinate-scope","Target areas are declared drafting requirements in this view plane. Their sizes and location are retained with the native attachment; they do not create or infer model contact geometry.","ready");
    fields.group("datum-target-format", "Target symbol formatting");
    fields.select(
        &format!("{path}/units"),
        "Size units",
        vec![
            choice("Millimeters", Units::Millimeters),
            choice("Inches", Units::Inches),
        ],
    );
    fields.number(
        &format!("{path}/precision"),
        "Decimals",
        "",
        Some(0.),
        Some(8.),
    );
    fields.number(
        &format!("{path}/text_size_mm"),
        "Paper text size",
        "mm",
        Some(0.5),
        Some(5.),
    );
    fields.number(
        &format!("{path}/callout_diameter_mm"),
        "Callout diameter",
        "mm",
        Some(6.),
        Some(100.),
    );
    if matches!(symbol.shape, TargetShape::Circle | TargetShape::Rectangle) {
        fields.number(
            &format!("{path}/hatch_spacing_mm"),
            "Paper hatch spacing",
            "mm",
            Some(0.25),
            Some(20.),
        );
    }
}
fn annotation_id(fields: &Fields<'_>, path: &str) -> String {
    path.trim_start_matches("/entities/")
        .split('/')
        .next()
        .and_then(|index| index.parse::<usize>().ok())
        .and_then(|index| fields.sheet.entities.get(index))
        .map(|annotation| annotation.id.clone())
        .unwrap_or_else(|| path.into())
}
fn declared(sheet: &Drawing) -> Vec<String> {
    sheet
        .entities
        .iter()
        .filter_map(|annotation| match &annotation.kind {
            AnnotationKind::Datum { identifier, .. } => Some(identifier.to_uppercase()),
            AnnotationKind::DatumTarget { symbol, .. } => Some(symbol.identifier.to_uppercase()),
            _ => None,
        })
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect()
}
pub(crate) fn frame_fields(fields: &mut Fields<'_>, path: &str, frame: &ToleranceFrame) {
    let annotation = annotation_id(fields, path);
    fields.group("tolerance-frame", "Feature control frame");
    fields.select(
        &format!("{path}/mode"),
        "Frame structure",
        vec![
            choice("Composite · shared symbol", FrameMode::Composite),
            choice("Stacked · independent symbols", FrameMode::Stacked),
        ],
    );
    fields.select(
        &format!("{path}/characteristic"),
        "Geometric characteristic",
        characteristic_choices(),
    );
    fields.note("tolerance-frame-scope","Composite segments share one characteristic; stacked rows can carry independent symbols. Requirements and datum modifiers remain authored intent, with source and expression validation; standards certification is separate.","ready");
    let selected = fields
        .w
        .fields
        .get("drawing-gdt-row")
        .filter(|id| frame.rows.iter().any(|row| &row.id == *id))
        .or_else(|| frame.rows.first().map(|row| &row.id));
    fields.transient(
        "gdt-row",
        "Frame segment",
        json!(selected),
        frame
            .rows
            .iter()
            .enumerate()
            .map(|(index, row)| {
                choice(&format!("Segment {} · {}", index + 1, row.value.0), &row.id)
            })
            .collect(),
    );
    let unit = if frame.units == Units::Inches {
        "in"
    } else {
        "mm"
    };
    for (index, row) in frame
        .rows
        .iter()
        .enumerate()
        .filter(|(_, row)| selected == Some(&row.id))
    {
        let row_path = format!("{path}/rows/{index}");
        fields.group("tolerance-frame-row", "Selected segment");
        if frame.mode == FrameMode::Stacked {
            fields.select(
                &format!("{row_path}/characteristic"),
                "Segment symbol",
                std::iter::once(choice("Use frame characteristic", Value::Null))
                    .chain(characteristic_choices())
                    .collect(),
            );
        }
        fields.field(
            &format!("{row_path}/value"),
            "Tolerance zone",
            vec![],
            Some(unit),
        );
        fields.toggle(&format!("{row_path}/diameter"), "Diametrical zone");
        fields.select(
            &format!("{row_path}/material"),
            "Zone material condition",
            material_choices(),
        );
        fields.toggle(&format!("{row_path}/free_state"), "Free state (F)");
        fields.toggle(&format!("{row_path}/tangent_plane"), "Tangent plane (T)");
        fields.toggle(
            &format!("{row_path}/projected_zone"),
            "Projected tolerance zone (P)",
        );
        if row.projected_zone {
            fields.field(
                &format!("{row_path}/projected_height"),
                "Projected height",
                vec![],
                Some(unit),
            );
        }
        fields.group("tolerance-frame-datums", "Datum reference sequence");
        let choices = declared(fields.sheet)
            .iter()
            .map(|datum| choice(datum, datum))
            .collect::<Vec<_>>();
        for (datum_index, _) in row.datums.iter().enumerate() {
            fields.select(
                &format!("{row_path}/datums/{datum_index}/identifier"),
                &format!("Datum {}", datum_index + 1),
                choices.clone(),
            );
            fields.select(
                &format!("{row_path}/datums/{datum_index}/material"),
                "Datum condition",
                material_choices(),
            );
            fields.action(
                &format!("gdt/{annotation}/row/{}/datum/{datum_index}/remove", row.id),
                "Remove datum",
                json!({"kind":"gdt-datum-remove","frame":path,"row":row.id,"index":datum_index}),
                true,
                "Close Settings first",
            );
        }
        fields.action(
            &format!("gdt/{annotation}/row/{}/datum/add", row.id),
            "Add datum reference",
            json!({"kind":"gdt-datum-add","frame":path,"row":row.id}),
            row.datums.len() < 3
                && declared(fields.sheet).iter().any(|identifier| {
                    !row.datums
                        .iter()
                        .any(|datum| datum.identifier.eq_ignore_ascii_case(identifier))
                }),
            "Declare an unused datum identifier; a frame row supports at most three datums",
        );
        fields.group("tolerance-frame-order", "Segment order");
        for (step, label) in [(-1, "Earlier segment"), (1, "Later segment")] {
            fields.action(
                &format!("gdt/{annotation}/row/{}/order/{step}", row.id),
                label,
                json!({"kind":"gdt-row-order","frame":path,"row":row.id,"step":step}),
                if step < 0 {
                    index > 0
                } else {
                    index + 1 < frame.rows.len()
                },
                "This segment is already at the end of the sequence",
            );
        }
        fields.action(
            &format!("gdt/{annotation}/row/{}/remove", row.id),
            "Remove segment",
            json!({"kind":"gdt-row-remove","frame":path,"row":row.id}),
            frame.rows.len()
                > if frame.mode == FrameMode::Composite {
                    2
                } else {
                    1
                },
            "Composite needs two segments; stacked needs at least one",
        );
    }
    fields.action(
        &format!("gdt/{annotation}/row/add"),
        "Add segment",
        json!({"kind":"gdt-row-add","frame":path}),
        frame.rows.len() < 8,
        "A frame supports at most eight segments",
    );
    fields.group("tolerance-frame-format", "Frame formatting");
    fields.select(
        &format!("{path}/units"),
        "Zone units",
        vec![
            choice("Millimeters", Units::Millimeters),
            choice("Inches", Units::Inches),
        ],
    );
    fields.number(
        &format!("{path}/precision"),
        "Decimals",
        "",
        Some(0.),
        Some(8.),
    );
    fields.number(
        &format!("{path}/text_size_mm"),
        "Paper text size",
        "mm",
        Some(0.5),
        Some(5.),
    );
    fields.number(
        &format!("{path}/cell_height_mm"),
        "Paper cell height",
        "mm",
        Some(6.),
        Some(30.),
    );
}
fn frame_mut<'a>(sheet: &'a mut Drawing, path: &str) -> Result<&'a mut ToleranceFrame> {
    let index = path
        .trim_start_matches("/entities/")
        .split('/')
        .next()
        .and_then(|index| index.parse::<usize>().ok())
        .ok_or_else(|| CadError::Invalid("The tolerance-frame source is missing".into()))?;
    let AnnotationKind::GeometricToleranceFrame { frame, .. } = &mut sheet
        .entities
        .get_mut(index)
        .ok_or_else(|| CadError::Invalid("The tolerance frame no longer exists".into()))?
        .kind
    else {
        return Err(CadError::Invalid(
            "The annotation is no longer a tolerance frame".into(),
        ));
    };
    Ok(frame)
}
pub(crate) fn apply_action(
    w: &mut Workspace,
    sheet: &mut Drawing,
    mutation: &Value,
) -> Result<bool> {
    let kind = mutation["kind"].as_str().unwrap_or_default();
    if !kind.starts_with("gdt-") {
        return Ok(false);
    }
    let available = declared(sheet);
    let frame = frame_mut(
        sheet,
        mutation["frame"]
            .as_str()
            .ok_or_else(|| CadError::Invalid("The tolerance-frame source is missing".into()))?,
    )?;
    if kind == "gdt-row-add" {
        if frame.rows.len() >= 8 {
            return Err(CadError::Invalid(
                "A frame supports at most eight rows".into(),
            ));
        }
        let row = FrameRow::new("0.05 mm");
        w.fields.insert("drawing-gdt-row".into(), row.id.clone());
        frame.rows.push(row);
        frame.validate()?;
        return Ok(true);
    }
    let id = mutation["row"]
        .as_str()
        .ok_or_else(|| CadError::Invalid("The frame segment is missing".into()))?;
    let index = frame
        .rows
        .iter()
        .position(|row| row.id == id)
        .ok_or_else(|| CadError::Invalid("The frame segment no longer exists".into()))?;
    match kind {
        "gdt-row-remove" => {
            if frame.rows.len()
                <= if frame.mode == FrameMode::Composite {
                    2
                } else {
                    1
                }
            {
                return Err(CadError::Invalid(
                    "Composite needs two segments; stacked needs at least one".into(),
                ));
            }
            frame.rows.remove(index);
        }
        "gdt-row-order" => {
            let destination = (index as i64 + mutation["step"].as_i64().unwrap_or_default())
                .try_into()
                .ok()
                .filter(|destination: &usize| *destination < frame.rows.len())
                .ok_or_else(|| {
                    CadError::Invalid("This segment is already at the end of the sequence".into())
                })?;
            frame.rows.swap(index, destination);
        }
        "gdt-datum-add" => {
            let row = &mut frame.rows[index];
            if row.datums.len() >= 3 {
                return Err(CadError::Invalid(
                    "A frame row supports at most three datum references".into(),
                ));
            }
            let id = available
                .iter()
                .find(|id| {
                    !row.datums
                        .iter()
                        .any(|datum| datum.identifier.eq_ignore_ascii_case(id))
                })
                .ok_or_else(|| {
                    CadError::Invalid(
                        "Declare an unused datum identifier before adding another reference".into(),
                    )
                })?;
            row.datums.push(DatumReference {
                identifier: id.clone(),
                material: MaterialCondition::Regardless,
            });
        }
        "gdt-datum-remove" => {
            let row = &mut frame.rows[index];
            let datum = mutation["index"]
                .as_u64()
                .and_then(|index| usize::try_from(index).ok())
                .filter(|index| *index < row.datums.len())
                .ok_or_else(|| CadError::Invalid("The datum reference no longer exists".into()))?;
            row.datums.remove(datum);
        }
        _ => {
            return Err(CadError::Invalid(
                "Unsupported tolerance-frame action".into(),
            ));
        }
    }
    frame.validate()?;
    Ok(true)
}
pub(crate) fn normalize_mode(sheet: &mut Drawing, pointer: &str, value: &Value) -> Result<()> {
    if pointer.ends_with("/kind/frame/mode") && value.as_str() == Some("composite") {
        let path = pointer.trim_end_matches("/mode");
        let frame = frame_mut(sheet, path)?;
        for row in &mut frame.rows {
            row.characteristic = None;
        }
        if frame.rows.len() < 2 {
            frame.rows.push(FrameRow::new("0.05 mm"));
        }
    }
    Ok(())
}
pub(crate) fn create_target(w: &Workspace, sheet: &Drawing) -> Result<AnnotationKind> {
    let mut symbol = DatumTarget::default();
    let number = (1..=999)
        .find(|number| {
            !sheet.entities.iter().any(|annotation| {
                matches!(&annotation.kind, AnnotationKind::DatumTarget { symbol: existing, .. }
                    if existing.identifier.eq_ignore_ascii_case(&symbol.identifier) && existing.number == *number)
            })
        })
        .ok_or_else(|| CadError::Invalid("All target numbers for datum A are used; change an existing target's datum identifier before adding another".into()))?;
    symbol.number = number;
    Ok(AnnotationKind::DatumTarget {
        target: crate::drawing_ui::selected_anchor(w),
        symbol: Box::new(symbol),
    })
}
pub(crate) fn create_frame(w: &Workspace, sheet: &Drawing) -> AnnotationKind {
    let mut frame = ToleranceFrame::default();
    if let Some(identifier) = declared(sheet).first() {
        frame.rows[0].datums.push(DatumReference {
            identifier: identifier.clone(),
            material: MaterialCondition::Regardless,
        });
    }
    AnnotationKind::GeometricToleranceFrame {
        target: crate::drawing_ui::selected_anchor(w),
        frame: Box::new(frame),
    }
}
