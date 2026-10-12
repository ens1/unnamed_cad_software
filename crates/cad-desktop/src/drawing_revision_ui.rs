//! Semantic controls for paper revision clouds and linked revision labels.
use crate::{Workspace, drawing_editor_ui::Fields};
use cad_core::{
    CadError, Drawing, Id, Result,
    drawing::AnnotationKind,
    drawing_revision::{Cloud, RevisionLink, Shape, Vertex},
    drawing_tables::TableKind,
};
use serde_json::{Value, json};
fn invalid(message: &str) -> CadError {
    CadError::Invalid(format!("Revision cloud: {message}"))
}
fn choice(label: &str, value: impl serde::Serialize) -> Value {
    json!({"label":label,"value":value})
}
pub(crate) fn fields(f: &mut Fields<'_>, path: &str, cloud: &Cloud) {
    let annotation = path
        .trim_start_matches("/entities/")
        .split('/')
        .next()
        .and_then(|value| value.parse::<usize>().ok())
        .and_then(|index| f.sheet.entities.get(index))
        .map(|annotation| annotation.id.clone())
        .unwrap_or_else(|| path.into());
    f.group("revision-cloud-outline", "Revision cloud outline");
    f.select(
        &format!("{path}/shape"),
        "Outline",
        vec![
            choice("Rectangle", Shape::Rectangle),
            choice("Ellipse", Shape::Ellipse),
            choice("Closed polygon", Shape::Polygon),
        ],
    );
    if cloud.shape != Shape::Polygon {
        for (key, label) in [("width_mm", "Paper width"), ("height_mm", "Paper height")] {
            f.number(
                &format!("{path}/{key}"),
                label,
                "mm",
                Some(0.5),
                Some(100_000.),
            );
        }
    } else {
        let selected =
            f.w.fields
                .get("drawing-cloud-vertex")
                .filter(|id| cloud.vertices.iter().any(|vertex| &vertex.id == *id))
                .or_else(|| cloud.vertices.first().map(|vertex| &vertex.id));
        f.transient(
            "cloud-vertex",
            "Polygon vertex",
            json!(selected),
            cloud
                .vertices
                .iter()
                .enumerate()
                .map(|(index, vertex)| {
                    choice(
                        &format!(
                            "{} · {:.2}, {:.2} mm",
                            index + 1,
                            vertex.point_mm[0],
                            vertex.point_mm[1]
                        ),
                        &vertex.id,
                    )
                })
                .collect(),
        );
        if let Some((index, vertex)) = cloud
            .vertices
            .iter()
            .enumerate()
            .find(|(_, vertex)| Some(&vertex.id) == selected)
        {
            for (axis, label) in [(0, "Vertex X"), (1, "Vertex Y")] {
                f.number(
                    &format!("{path}/vertices/{index}/point_mm/{axis}"),
                    label,
                    "mm",
                    Some(-100_000.),
                    Some(100_000.),
                );
            }
            f.action(
                &format!("cloud/{annotation}/vertex/{}/add", vertex.id),
                "Insert midpoint after vertex",
                json!({"kind":"cloud-vertex-add","annotation":annotation,"vertex":vertex.id}),
                cloud.vertices.len() < cad_core::drawing_revision::MAX_VERTICES,
                "A cloud supports at most 128 vertices",
            );
            f.action(
                &format!("cloud/{annotation}/vertex/{}/remove", vertex.id),
                "Remove vertex",
                json!({"kind":"cloud-vertex-remove","annotation":annotation,"vertex":vertex.id}),
                cloud.vertices.len() > 3,
                "A closed polygon needs at least three vertices",
            );
        }
        f.action(
            &format!("cloud/{annotation}/polygon/reseed"),
            "Reset polygon to current box",
            json!({"kind":"cloud-polygon-reseed","annotation":annotation}),
            true,
            "Close Settings first",
        );
        f.note("revision-cloud-polygon-edit","Drag the selected cloud's vertex handles on paper. The outline must remain a simple closed polygon; rectangle/ellipse sizes and saved polygon vertices are independent.","ready");
    }
    f.number(
        &format!("{path}/rotation_deg"),
        "Paper rotation",
        "deg",
        Some(-1_000_000.),
        Some(1_000_000.),
    );
    f.number(
        &format!("{path}/arc_pitch_mm"),
        "Maximum scallop chord",
        "mm",
        Some(0.5),
        Some(100.),
    );
    f.number(
        &format!("{path}/bulge"),
        "Scallop bulge / chord",
        "",
        Some(0.1),
        Some(0.45),
    );
    f.group("revision-cloud-label", "Revision association and callout");
    let mut revisions = vec![choice("Unlinked authored label", Value::Null)];
    for table in &f.sheet.tables {
        if let TableKind::Revisions { rows } = &table.kind {
            for row in rows {
                revisions.push(choice(
                    &format!("{} · {}", table.title, row.revision),
                    RevisionLink {
                        table: table.id.clone(),
                        row: row.id.clone(),
                    },
                ));
            }
        }
    }
    if let Some(link) = &cloud.revision
        && !revisions
            .iter()
            .any(|choice| choice["value"] == json!(link))
    {
        revisions.push(choice(
            "Missing revision row · choose a current row or unlink",
            link,
        ));
    }
    f.select(&format!("{path}/revision"), "Revision row", revisions);
    f.text(
        &format!("{path}/label"),
        if cloud.revision.is_some() {
            "Callout prefix"
        } else {
            "Authored label"
        },
        false,
    );
    f.toggle(&format!("{path}/show_label"), "Show revision callout");
    if cloud.show_label {
        f.number(
            &format!("{path}/text_size_mm"),
            "Callout text size",
            "mm",
            Some(0.5),
            Some(10.),
        );
        for (axis, label) in [(0, "Callout offset X"), (1, "Callout offset Y")] {
            f.number(
                &format!("{path}/label_offset_mm/{axis}"),
                label,
                "mm",
                Some(-1_000_000.),
                Some(1_000_000.),
            );
        }
    }
    f.note("revision-cloud-association","A linked callout follows the exact saved revision-table row, including label changes. Reattach or unlink clouds before deleting that row or its table. Clouds mark authored drawing changes; they do not infer geometric differences.","ready");
}
fn cloud_mut<'a>(sheet: &'a mut Drawing, id: &str) -> Result<&'a mut Cloud> {
    match &mut sheet
        .entities
        .iter_mut()
        .find(|annotation| annotation.id == id)
        .ok_or_else(|| invalid("The annotation no longer exists"))?
        .kind
    {
        AnnotationKind::RevisionCloud { cloud, .. } => Ok(cloud),
        _ => Err(invalid("The annotation is no longer a revision cloud")),
    }
}
pub(crate) fn apply_action(
    w: &mut Workspace,
    sheet: &mut Drawing,
    mutation: &Value,
) -> Result<bool> {
    let kind = mutation["kind"].as_str().unwrap_or_default();
    if !kind.starts_with("cloud-") {
        return Ok(false);
    }
    let id = mutation["annotation"]
        .as_str()
        .ok_or_else(|| invalid("Choose a cloud"))?;
    let cloud = cloud_mut(sheet, id)?;
    if kind == "cloud-polygon-reseed" {
        cloud.vertices = [
            [-cloud.width_mm * 0.5, -cloud.height_mm * 0.5],
            [cloud.width_mm * 0.5, -cloud.height_mm * 0.5],
            [cloud.width_mm * 0.5, cloud.height_mm * 0.5],
            [-cloud.width_mm * 0.5, cloud.height_mm * 0.5],
        ]
        .into_iter()
        .map(Vertex::new)
        .collect();
        w.fields
            .insert("drawing-cloud-vertex".into(), cloud.vertices[0].id.clone());
    } else {
        let vertex = mutation["vertex"]
            .as_str()
            .ok_or_else(|| invalid("Choose a vertex"))?;
        let index = cloud
            .vertices
            .iter()
            .position(|point| point.id == vertex)
            .ok_or_else(|| invalid("The vertex no longer exists"))?;
        match kind {
            "cloud-vertex-add" => {
                if cloud.vertices.len() >= cad_core::drawing_revision::MAX_VERTICES {
                    return Err(invalid("A cloud supports at most 128 vertices"));
                }
                let a = cloud.vertices[index].point_mm;
                let b = cloud.vertices[(index + 1) % cloud.vertices.len()].point_mm;
                let vertex = Vertex::new([(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5]);
                w.fields
                    .insert("drawing-cloud-vertex".into(), vertex.id.clone());
                cloud.vertices.insert(index + 1, vertex);
            }
            "cloud-vertex-remove" => {
                if cloud.vertices.len() <= 3 {
                    return Err(invalid("A polygon needs at least three vertices"));
                }
                cloud.vertices.remove(index);
                w.fields.insert(
                    "drawing-cloud-vertex".into(),
                    cloud.vertices[index.min(cloud.vertices.len() - 1)]
                        .id
                        .clone(),
                );
            }
            _ => return Err(invalid("Unknown cloud vertex action")),
        }
    }
    cloud.validate()?;
    Ok(true)
}
pub(crate) fn create(w: &Workspace, sheet: &Drawing) -> AnnotationKind {
    let mut cloud = Cloud::default();
    if let Some((table, row)) = sheet.tables.iter().find_map(|table| {
        if let TableKind::Revisions { rows } = &table.kind {
            rows.last().map(|row| (table, row))
        } else {
            None
        }
    }) {
        cloud.revision = Some(RevisionLink {
            table: table.id.clone(),
            row: row.id.clone(),
        });
    }
    AnnotationKind::RevisionCloud {
        target: crate::drawing_ui::selected_anchor(w),
        cloud: Box::new(cloud),
    }
}
pub(crate) fn vertex_point(
    annotation: &cad_core::drawing::Annotation,
    vertex: &Id,
) -> Result<[f64; 2]> {
    let AnnotationKind::RevisionCloud { cloud, .. } = &annotation.kind else {
        return Err(invalid("The annotation is not a revision cloud"));
    };
    cloud
        .vertices
        .iter()
        .find(|source| &source.id == vertex)
        .map(|source| source.point_mm)
        .ok_or_else(|| invalid("The cloud vertex no longer exists"))
}
