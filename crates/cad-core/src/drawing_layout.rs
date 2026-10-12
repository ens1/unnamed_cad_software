//! Bounded f64 paper placement actions over ordinary source-owned annotations.
//! Layout aligns placement origins, rather than inferred text-font outlines.
use crate::{
    CadError, Document, Drawing, Id, Result,
    drawing::{self, Annotation, AnnotationKind},
    drawing_interaction,
    protocol::Evaluation,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
pub const CONTRACT_VERSION: u32 = 1;
pub const MAX_COHORT: usize = 128;
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    AlignX,
    AlignY,
    DistributeX,
    DistributeY,
    StackX,
    StackY,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Request {
    pub version: u32,
    pub annotations: Vec<Id>,
    pub primary: Id,
    pub mode: Mode,
    pub spacing_mm: f64,
}
fn invalid(message: &str) -> CadError {
    CadError::Invalid(format!("Drawing layout: {message}"))
}
pub fn movable(annotation: &Annotation) -> bool {
    !matches!(
        annotation.kind,
        AnnotationKind::CenterMark { .. } | AnnotationKind::CenterLine { .. }
    )
}
fn cohort<'a>(source: &'a Drawing, ids: &[Id], move_required: bool) -> Result<Vec<&'a Annotation>> {
    if ids.is_empty() || ids.len() > MAX_COHORT {
        return Err(invalid(
            "Choose 1–128 distinct annotations on the current sheet",
        ));
    }
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for id in ids {
        if !seen.insert(id) {
            return Err(invalid("An annotation cannot occur twice in a cohort"));
        }
        let annotation = source
            .entities
            .iter()
            .find(|annotation| &annotation.id == id)
            .ok_or_else(|| invalid("A selected annotation no longer exists"))?;
        if move_required && !movable(annotation) {
            return Err(invalid(
                "Center marks/lines stay attached to their native geometry; select movable annotations for paper layout or copying",
            ));
        }
        out.push(annotation);
    }
    Ok(out)
}
pub fn translate(source: &Drawing, ids: &[Id], delta_mm: [f64; 2]) -> Result<Drawing> {
    cohort(source, ids, true)?;
    if delta_mm
        .iter()
        .any(|value| !value.is_finite() || value.abs() > 1_000_000.)
    {
        return Err(invalid(
            "Translation requires bounded finite paper millimeters",
        ));
    }
    let selected = ids.iter().collect::<BTreeSet<_>>();
    let mut result = source.clone();
    for annotation in result
        .entities
        .iter_mut()
        .filter(|annotation| selected.contains(&annotation.id))
    {
        for (axis, delta) in delta_mm.iter().enumerate() {
            annotation.offset_mm[axis] += delta;
        }
    }
    drawing::validate_definition(&result)?;
    Ok(result)
}
pub fn arrange(
    document: &Document,
    evaluation: &Evaluation,
    source: &Drawing,
    request: &Request,
) -> Result<Drawing> {
    if request.version != CONTRACT_VERSION
        || !request.spacing_mm.is_finite()
        || !(0.1..=1000.).contains(&request.spacing_mm)
    {
        return Err(invalid(
            "Layout needs contract 1 and 0.1–1000 paper-mm spacing",
        ));
    }
    let annotations = cohort(source, &request.annotations, true)?;
    if annotations.len() < 2 || !request.annotations.contains(&request.primary) {
        return Err(invalid(
            "Choose at least two annotations and a selected reference annotation",
        ));
    }
    if matches!(request.mode, Mode::DistributeX | Mode::DistributeY) && annotations.len() < 3 {
        return Err(invalid("Distribution needs at least three annotations"));
    }
    let paper = drawing::build_sheet_preview(document, evaluation, source, 0)?;
    let mut positions = BTreeMap::new();
    for annotation in annotations {
        if paper.diagnostics.contains_key(&annotation.id) {
            return Err(invalid(
                "Repair selected annotation diagnostics before arranging their placement",
            ));
        }
        let anchor = drawing_interaction::annotation_anchors(annotation)
            .into_iter()
            .next()
            .map(|(_, anchor)| anchor)
            .ok_or_else(|| invalid("This annotation has no editable placement origin"))?;
        let model = if let AnnotationKind::CutListBalloon { source: member, .. } = &annotation.kind
            && anchor.reference.is_none()
        {
            crate::drawing_tables::balloon_source_point(document, evaluation, source, member)?
        } else {
            drawing::point(
                document,
                evaluation,
                source,
                anchor.point,
                &anchor.reference,
            )?
        };
        let projection = paper
            .projections
            .get(&annotation.view)
            .ok_or_else(|| invalid("A selected annotation's drawing view is missing"))?;
        let at = projection.paper_point(&annotation.view, model)?;
        let at = [
            at[0] + annotation.offset_mm[0],
            at[1] + annotation.offset_mm[1],
        ];
        if at.iter().any(|value| !value.is_finite()) {
            return Err(invalid(
                "A selected placement origin exceeded finite paper bounds",
            ));
        }
        positions.insert(annotation.id.clone(), at);
    }
    let reference = positions[&request.primary];
    let axis = usize::from(matches!(
        request.mode,
        Mode::AlignY | Mode::DistributeY | Mode::StackY
    ));
    let mut desired = positions.clone();
    match request.mode {
        Mode::AlignX | Mode::AlignY => {
            for point in desired.values_mut() {
                point[axis] = reference[axis];
            }
        }
        Mode::DistributeX | Mode::DistributeY => {
            let mut order = request.annotations.clone();
            order.sort_by(|a, b| {
                positions[a][axis]
                    .total_cmp(&positions[b][axis])
                    .then_with(|| a.cmp(b))
            });
            let first = positions[&order[0]][axis];
            let last = positions[order.last().unwrap()][axis];
            for (index, id) in order.iter().enumerate() {
                desired.get_mut(id).unwrap()[axis] =
                    first + (last - first) * index as f64 / (order.len() - 1) as f64;
            }
        }
        Mode::StackX | Mode::StackY => {
            for (index, id) in std::iter::once(&request.primary)
                .chain(
                    request
                        .annotations
                        .iter()
                        .filter(|id| *id != &request.primary),
                )
                .enumerate()
            {
                let point = desired.get_mut(id).unwrap();
                point[1 - axis] = reference[1 - axis];
                point[axis] = reference[axis]
                    + if axis == 0 { 1. } else { -1. } * request.spacing_mm * index as f64;
            }
        }
    }
    let mut result = source.clone();
    for annotation in &mut result.entities {
        if let (Some(old), Some(new)) = (positions.get(&annotation.id), desired.get(&annotation.id))
        {
            for axis in 0..2 {
                annotation.offset_mm[axis] += new[axis] - old[axis];
            }
        }
    }
    drawing::validate_definition(&result)?;
    Ok(result)
}
fn datum_identifiers(source: &Drawing) -> BTreeSet<String> {
    source
        .entities
        .iter()
        .filter_map(|annotation| match &annotation.kind {
            AnnotationKind::Datum { identifier, .. } => Some(identifier.to_uppercase()),
            AnnotationKind::DatumTarget { symbol, .. } => Some(symbol.identifier.to_uppercase()),
            _ => None,
        })
        .collect()
}
fn next_identifier(used: &BTreeSet<String>) -> Result<String> {
    for index in 0..26 + 26 * 26 + 26 * 26 * 26 {
        let mut value = index + 1;
        let mut letters = Vec::new();
        while value > 0 {
            value -= 1;
            letters.push(b'A' + (value % 26) as u8);
            value /= 26;
        }
        letters.reverse();
        let label = String::from_utf8(letters).unwrap();
        if !used.contains(&label) {
            return Ok(label);
        }
    }
    Err(invalid(
        "No unused 1–3-letter datum identifier is available",
    ))
}
pub fn copy(source: &Drawing, ids: &[Id], offset_mm: [f64; 2]) -> Result<(Drawing, Vec<Id>)> {
    let annotations = cohort(source, ids, true)?;
    if offset_mm
        .iter()
        .any(|value| !value.is_finite() || value.abs() > 1_000_000.)
    {
        return Err(invalid(
            "Copy offset requires finite bounded paper millimeters",
        ));
    }
    let mut used = datum_identifiers(source);
    let mut datum_map = BTreeMap::new();
    for annotation in &annotations {
        if let AnnotationKind::Datum { identifier, .. } = &annotation.kind {
            let next = next_identifier(&used)?;
            used.insert(next.clone());
            datum_map.insert(identifier.to_uppercase(), next);
        }
    }
    let mut targets = source
        .entities
        .iter()
        .filter_map(|annotation| {
            if let AnnotationKind::DatumTarget { symbol, .. } = &annotation.kind {
                Some((symbol.identifier.to_uppercase(), symbol.number))
            } else {
                None
            }
        })
        .collect::<BTreeSet<_>>();
    let mut result = source.clone();
    let mut created = Vec::new();
    for source in annotations {
        let mut annotation = (*source).clone();
        annotation.id = crate::id();
        for (axis, delta) in offset_mm.iter().enumerate() {
            annotation.offset_mm[axis] += delta;
        }
        match &mut annotation.kind {
            AnnotationKind::Datum { identifier, .. } => {
                *identifier = datum_map[&identifier.to_uppercase()].clone();
            }
            AnnotationKind::DatumTarget { symbol, .. } => {
                if let Some(label) = datum_map.get(&symbol.identifier.to_uppercase()) {
                    symbol.identifier = label.clone();
                }
                let label = symbol.identifier.to_uppercase();
                let number = (1..=999)
                    .find(|number| !targets.contains(&(label.clone(), *number)))
                    .ok_or_else(|| {
                        invalid("No unused target number is available for a copied datum target")
                    })?;
                targets.insert((label, number));
                symbol.number = number;
            }
            AnnotationKind::GeometricTolerance { datums, .. } => {
                for datum in datums {
                    if let Some(label) = datum_map.get(&datum.to_uppercase()) {
                        *datum = label.clone();
                    }
                }
            }
            AnnotationKind::GeometricToleranceFrame { frame, .. } => {
                for row in &mut frame.rows {
                    row.id = crate::id();
                    for datum in &mut row.datums {
                        if let Some(label) = datum_map.get(&datum.identifier.to_uppercase()) {
                            datum.identifier = label.clone();
                        }
                    }
                }
            }
            AnnotationKind::DimensionGroup { group } => {
                for target in &mut group.targets {
                    target.id = crate::id();
                }
            }
            AnnotationKind::RevisionCloud { cloud, .. } => {
                for vertex in &mut cloud.vertices {
                    vertex.id = crate::id();
                }
            }
            _ => {}
        }
        created.push(annotation.id.clone());
        result.entities.push(annotation);
    }
    drawing::validate_definition(&result)?;
    Ok((result, created))
}
pub fn delete(source: &Drawing, ids: &[Id]) -> Result<Drawing> {
    cohort(source, ids, false)?;
    let selected = ids.iter().collect::<BTreeSet<_>>();
    let before = datum_identifiers(source);
    let mut result = source.clone();
    result
        .entities
        .retain(|annotation| !selected.contains(&annotation.id));
    let after = datum_identifiers(&result);
    let retired = before.difference(&after).cloned().collect::<BTreeSet<_>>();
    for annotation in &result.entities {
        let required = match &annotation.kind {
            AnnotationKind::GeometricTolerance { datums, .. } => datums.iter().collect::<Vec<_>>(),
            AnnotationKind::GeometricToleranceFrame { frame, .. } => frame
                .rows
                .iter()
                .flat_map(|row| row.datums.iter().map(|datum| &datum.identifier))
                .collect(),
            _ => vec![],
        };
        if required
            .iter()
            .any(|datum| retired.contains(&datum.to_uppercase()))
        {
            return Err(invalid(
                "A retained tolerance frame uses a selected datum; reattach its references or include that frame in the deletion",
            ));
        }
    }
    drawing::validate_definition(&result)?;
    Ok(result)
}
