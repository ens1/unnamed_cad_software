//! Paper presentation of evaluated, part-owned manufacturing requirements.
//! Only placement is saved on a sheet. Values and face attachments come from the model.
use super::{
    Primitive, Sheet, ViewProjection, arrow, invalid, line, paper_bounds, rectangle, text,
    transform,
};
use crate::{
    Document, Drawing, Id, Result,
    drawing_display_states::DisplayInstance,
    drawing_interaction::{PrimitiveGroup, Target},
    model_dimensions::{self, Kind},
    protocol::Evaluation,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

const TEXT_MM: f64 = 2.6;
type Point = [f64; 2];
type Bounds = [Point; 2];

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Placement {
    pub view: String,
    /// Paper millimeters from the continuing dimension witness or datum anchor.
    pub offset_mm: Point,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PaperItem {
    pub definition: Id,
    pub source: Id,
    pub configuration: Option<Id>,
    pub path: Vec<Id>,
    pub datum: bool,
    pub label: String,
    pub text: String,
    pub view: String,
    pub anchor_mm: Point,
    pub label_mm: Point,
}

pub(super) fn default_enabled() -> bool {
    true
}

fn part_for_sheet<'a>(doc: &'a Document, definition: &str) -> Option<&'a crate::Part> {
    if let Some(part) = doc.parts.get(definition) {
        return Some(part);
    }
    if definition != doc.root
        || doc
            .properties
            .get("document_kind")
            .is_none_or(|kind| kind != "part")
    {
        return None;
    }
    let mut instances = doc.assemblies.get(definition)?.instances.values();
    let first = instances.next()?;
    if instances.next().is_some() {
        return None;
    }
    doc.parts.get(&first.definition)
}

pub(super) fn part_scope(doc: &Document, definition: &str) -> bool {
    part_for_sheet(doc, definition).is_some()
}

/// Frame datums may be authored on paper or established on the single represented part.
/// Keep a broken model datum declared so its missing face can be repaired in the model.
pub fn declared_datums(doc: &Document, sheet: &Drawing) -> BTreeSet<String> {
    let mut names: BTreeSet<_> = sheet
        .entities
        .iter()
        .filter_map(|a| match &a.kind {
            super::AnnotationKind::Datum { identifier, .. } => {
                Some(identifier.to_ascii_uppercase())
            }
            super::AnnotationKind::DatumTarget { symbol, .. } => {
                Some(symbol.identifier.to_ascii_uppercase())
            }
            _ => None,
        })
        .collect();
    if sheet.show_model_dimensions
        && let Some(part) = part_for_sheet(doc, &sheet.definition)
    {
        names.extend(part.model_dimensions.datums.keys().cloned());
    }
    names
}

pub(super) fn validate_placements(sheet: &Drawing) -> Result<()> {
    if sheet.model_item_placements.iter().any(|(key, p)| {
        key.is_empty()
            || !super::VIEW_KEYS.contains(&p.view.as_str())
            || p.offset_mm
                .iter()
                .any(|x| !x.is_finite() || x.abs() > 1_000_000.)
    }) {
        return Err(invalid(
            "A model callout needs a valid view and finite paper offset",
        ));
    }
    Ok(())
}

struct Item {
    id: Id,
    definition: Id,
    source: Id,
    configuration: Option<Id>,
    path: Vec<Id>,
    label: String,
    text: String,
    points: [[f64; 3]; 2],
    normal: Option<[f64; 3]>,
    style: Style,
}

#[derive(Clone, Copy, PartialEq)]
enum Style {
    Linear,
    Circular,
    Datum,
}

fn sum(a: Point, b: Point) -> Point {
    [a[0] + b[0], a[1] + b[1]]
}
fn diff(a: Point, b: Point) -> Point {
    [a[0] - b[0], a[1] - b[1]]
}
fn mul(a: Point, k: f64) -> Point {
    [a[0] * k, a[1] * k]
}
fn dot(a: Point, b: Point) -> f64 {
    a[0] * b[0] + a[1] * b[1]
}
fn length(v: Point) -> f64 {
    v[0].hypot(v[1])
}

fn world_direction(m: [[f64; 4]; 4], n: [f64; 3]) -> [f64; 3] {
    std::array::from_fn(|i| m[i][0] * n[0] + m[i][1] * n[1] + m[i][2] * n[2])
}

fn collect(
    doc: &Document,
    e: &Evaluation,
    sheet: &Drawing,
    instances: &[DisplayInstance],
    paper: &mut Sheet,
) -> Result<Vec<Item>> {
    let mut items = vec![];
    let mut seen = BTreeSet::new();
    let mut ordered: Vec<_> = instances.iter().collect();
    ordered.sort_by(|a, b| a.placement.path.cmp(&b.placement.path));
    for displayed in ordered {
        if displayed.effect.visibility == Some(crate::display_states::Visibility::Hidden) {
            continue;
        }
        let instance = &displayed.placement;
        let Some(part) = doc.parts.get(&instance.definition) else {
            continue;
        };
        let key = crate::history::mesh_key(instance);
        if !seen.insert(key) {
            continue;
        }
        let Some(report) = e.model_dimensions.get(key) else {
            if sheet.show_model_dimensions
                && (!part.outputs.is_empty()
                    || e.meshes.get(key).is_some_and(|m| !m.faces.is_empty()))
            {
                paper.model_review.push(format!(
                    "{}: finished dimensions are unavailable; rebuild the part",
                    part.name
                ));
            }
            continue;
        };
        paper.model_review.extend(
            report
                .issues
                .iter()
                .map(|issue| format!("{}: {issue}", part.name)),
        );
        if !sheet.show_model_dimensions {
            continue;
        }
        paper.model_coverage.extend(
            report
                .coverage
                .iter()
                .map(|detail| format!("{}: {detail}", part.name)),
        );
        let placement_id = |kind: &str, id: &str| {
            serde_json::to_string(&(
                &instance.definition,
                &instance.configuration,
                &instance.path,
                kind,
                id,
            ))
        };
        let m = instance.matrix;
        for d in &report.dimensions {
            items.push(Item {
                id: placement_id("dimension", &d.id)?,
                definition: part.id.clone(),
                source: d.id.clone(),
                configuration: instance.configuration.clone(),
                path: instance.path.clone(),
                label: d.label.clone(),
                text: model_dimensions::dimension_text(part, &doc.settings, d),
                points: d.witnesses.map(|p| transform(p, m)),
                normal: d.plane_normal.map(|n| world_direction(m, n)),
                style: if d.kind == Kind::Linear {
                    Style::Linear
                } else {
                    Style::Circular
                },
            });
        }
        // An assembly-level letter cannot unambiguously identify several parts' A datums.
        if !part_scope(doc, &sheet.definition) {
            continue;
        }
        for datum in report.datums.iter().filter(|d| d.explicit) {
            let point = transform(datum.anchor, m);
            items.push(Item {
                id: placement_id("datum", &datum.label)?,
                definition: part.id.clone(),
                source: datum.label.clone(),
                configuration: instance.configuration.clone(),
                path: instance.path.clone(),
                label: format!("Datum {}", datum.label),
                text: datum.label.clone(),
                points: [point, point],
                normal: match &datum.carrier {
                    model_dimensions::DatumCarrier::Plane { plane } => {
                        let n = nalgebra::Vector3::from(plane.u)
                            .cross(&nalgebra::Vector3::from(plane.v));
                        Some(world_direction(m, n.into()))
                    }
                    model_dimensions::DatumCarrier::Axis { axis } => {
                        Some(world_direction(m, axis.direction))
                    }
                },
                style: Style::Datum,
            });
        }
    }
    Ok(items)
}

fn label_width(label: &str) -> f64 {
    label.chars().count().max(1) as f64 * TEXT_MM * 0.66
}
fn label_bounds(at: Point, w: f64) -> Bounds {
    [
        [at[0] - 1., at[1] - 1.4],
        [at[0] + w + 1., at[1] + TEXT_MM + 1.],
    ]
}
fn inside(b: Bounds, r: Bounds) -> bool {
    (0..2).all(|i| b[0][i] >= r[0][i] && b[1][i] <= r[1][i])
}

// Index labels in paper rows. Large patterns need only inspect nearby labels.
#[derive(Default)]
struct Occupancy(BTreeMap<i32, Vec<Bounds>>);
impl Occupancy {
    fn rows(b: Bounds) -> std::ops::RangeInclusive<i32> {
        (b[0][1] / 6.).floor() as i32..=(b[1][1] / 6.).floor() as i32
    }
    fn insert(&mut self, b: Bounds) {
        for row in Self::rows(b) {
            self.0.entry(row).or_default().push(b);
        }
    }
    fn intersects(&self, b: Bounds) -> bool {
        Self::rows(b).any(|row| {
            self.0
                .get(&row)
                .is_some_and(|values| values.iter().any(|v| super::rectangles_overlap(*v, b)))
        })
    }
}

struct Projection {
    view: String,
    anchor: Point,
    points: [Point; 2],
    /// False if a linear measurement would be foreshortened in this view.
    true_length: bool,
    quality: f64,
}

fn choose_view(
    item: &Item,
    paper: &Sheet,
    saved: Option<&Placement>,
) -> Result<Option<Projection>> {
    let delta: [f64; 3] = std::array::from_fn(|i| item.points[1][i] - item.points[0][i]);
    let span = delta.iter().map(|x| x * x).sum::<f64>().sqrt();
    let mut best: Option<Projection> = None;
    for name in ["front", "top", "right", "isometric"] {
        let Some(projection) = paper.projections.get(name) else {
            continue;
        };
        let a = projection.paper_point(name, item.points[0])?;
        let b = projection.paper_point(name, item.points[1])?;
        let ratio = if span > 0. {
            length(diff(b, a)) / (span * projection.scale)
        } else {
            1.
        };
        let quality = if let Some(normal) = item.normal {
            let projected = super::project(name, normal)?;
            let norm = normal.iter().map(|x| x * x).sum::<f64>().sqrt();
            let in_plane = (length(projected) / norm.max(f64::EPSILON)).min(1.);
            if item.style == Style::Datum {
                in_plane
            } else {
                1. - in_plane
            }
        } else {
            ratio.min(1.)
        };
        let anchor = if item.style == Style::Circular {
            b
        } else {
            mul(sum(a, b), 0.5)
        };
        let quality = quality
            - if matches!(name, "isometric" | "detail-front") {
                0.25
            } else {
                0.
            };
        let choice = Projection {
            view: name.into(),
            points: [a, b],
            anchor,
            true_length: (ratio - 1.).abs() < 1e-7,
            quality,
        };
        if saved.is_some_and(|p| p.view == name) {
            return Ok(Some(choice));
        }
        if best
            .as_ref()
            .is_none_or(|prior| choice.quality > prior.quality)
        {
            best = Some(choice);
        }
    }
    Ok(best)
}

fn auto_label(
    p: &Projection,
    width: f64,
    area: Bounds,
    projection: &ViewProjection,
    occupied: &Occupancy,
) -> Option<Point> {
    let [x, y, w, h] = projection.rect_mm;
    let anchor = p.anchor;
    let dir = diff(p.points[1], p.points[0]);
    let horizontal = dir[0].abs() >= dir[1].abs();
    let below = [anchor[0] - width / 2., y + 3.];
    let above = [below[0], y + h - 5.];
    let right = [x + w - 2., anchor[1] + 1.5];
    let left = [x + 2. - width, right[1]];
    let primary = if horizontal {
        [below, above, right, left]
    } else {
        [right, left, below, above]
    };
    if width + 2. > area[1][0] - area[0][0] || area[1][1] - area[0][1] < TEXT_MM + 3. {
        return None;
    }
    let constrain = |p: Point| {
        [
            p[0].clamp(area[0][0] + 1., area[1][0] - width - 1.),
            p[1].clamp(area[0][1] + 1.4, area[1][1] - TEXT_MM - 1.),
        ]
    };
    for lane in 0..10 {
        for (i, base) in primary.iter().enumerate() {
            let at = constrain([
                base[0],
                base[1] + lane as f64 * 5.5 * if i == 0 { -1. } else { 1. },
            ]);
            if !occupied.intersects(label_bounds(at, width)) {
                return Some(at);
            }
        }
    }
    // Sparse area elsewhere on the sheet is preferable to overlapping text.
    let mut y = area[1][1] - TEXT_MM - 1.;
    while y >= area[0][1] + 1.4 {
        let mut x = area[0][0] + 1.;
        while x + width + 1. <= area[1][0] {
            let at = [x, y];
            if !occupied.intersects(label_bounds(at, width)) {
                return Some(at);
            }
            x += 5.;
        }
        y -= 5.5;
    }
    None
}

fn draw(out: &mut Vec<Primitive>, item: &Item, p: &Projection, at: Point, label: &str) {
    let [a, b] = p.points;
    let width = label_width(label);
    let base = [at[0] + width / 2., at[1] - 0.8];
    if item.style == Style::Linear && p.true_length && length(diff(b, a)) > 1e-9 {
        let direction = mul(diff(b, a), 1. / length(diff(b, a)));
        let normal = [-direction[1], direction[0]];
        let shift = mul(normal, dot(diff(base, p.anchor), normal));
        let (start, end) = (sum(a, shift), sum(b, shift));
        line(out, a, start);
        line(out, b, end);
        line(out, start, end);
        arrow(out, start, end);
        arrow(out, end, start);
        let along = dot(diff(base, start), direction);
        if along < 0. {
            line(out, start, base);
        }
        if along > length(diff(end, start)) {
            line(out, end, base);
        }
    } else {
        let right = p.anchor[0] > base[0];
        let elbow = [
            if right {
                at[0] + width + 1.
            } else {
                at[0] - 1.
            },
            at[1] + TEXT_MM * 0.35,
        ];
        line(out, p.anchor, elbow);
        if item.style == Style::Datum {
            rectangle(out, [at[0] - 1., at[1] - 1.], [width + 2., TEXT_MM + 2.]);
            let d = diff(elbow, p.anchor);
            let n = mul(d, 1. / length(d).max(1e-9));
            let v = [-n[1], n[0]];
            let foot = sum(p.anchor, mul(n, 2.3));
            let t1 = sum(foot, mul(v, 1.2));
            let t2 = sum(foot, mul(v, -1.2));
            line(out, p.anchor, t1);
            line(out, t1, t2);
            line(out, t2, p.anchor);
        } else {
            arrow(out, p.anchor, elbow);
        }
    }
    text(out, at, TEXT_MM, label);
}

pub(super) fn project_items(
    doc: &Document,
    e: &Evaluation,
    sheet: &Drawing,
    instances: &[DisplayInstance],
    paper: &mut Sheet,
    footer: f64,
) -> Result<()> {
    let mut items = collect(doc, e, sheet, instances, paper)?;
    // Keep manual labels in their reserved positions while placing automatic labels.
    items.sort_by_key(|item| !sheet.model_item_placements.contains_key(&item.id));
    let mut area = paper_bounds(sheet);
    area[0][1] = area[0][1].max(footer + 3.);
    let mut occupied = Occupancy::default();
    for primitive in &paper.primitives {
        if let Primitive::Text { at, size, text } = primitive {
            occupied.insert([
                [at[0] - 1., at[1] - 1.4],
                [
                    at[0] + text.chars().count() as f64 * size * 0.66 + 1.,
                    at[1] + size + 1.,
                ],
            ]);
        }
    }
    for item in items {
        let saved = sheet.model_item_placements.get(&item.id);
        let Some(p) = choose_view(&item, paper, saved)? else {
            paper
                .model_review
                .push("Add an ordinary model view to place model dimensions and datums".into());
            break;
        };
        let value = if item.style == Style::Linear && !p.true_length {
            format!("{} {}", item.label, item.text)
        } else {
            item.text.clone()
        };
        let width = label_width(&value);
        let manual = saved.filter(|s| s.view == p.view);
        let at = if let Some(manual) = manual {
            sum(p.anchor, manual.offset_mm)
        } else if let Some(at) = auto_label(&p, width, area, &paper.projections[&p.view], &occupied)
        {
            at
        } else {
            paper.model_review.push(format!(
                "{} needs more label space; enlarge the sheet or move its callouts",
                item.label
            ));
            sum(p.anchor, [2., 4.])
        };
        if !inside(label_bounds(at, width), paper_bounds(sheet)) {
            paper.model_review.push(format!(
                "{} falls outside the paper; move its label or enlarge the sheet",
                item.label
            ));
        }
        occupied.insert(label_bounds(at, width));
        let start = paper.primitives.len();
        draw(&mut paper.primitives, &item, &p, at, &value);
        paper.groups.push(PrimitiveGroup {
            target: Target::ModelItem {
                id: item.id.clone(),
            },
            view: Some(p.view.clone()),
            primitive_start: start,
            primitive_count: paper.primitives.len() - start,
        });
        paper.model_items.insert(
            item.id,
            PaperItem {
                definition: item.definition,
                source: item.source,
                configuration: item.configuration,
                path: item.path,
                datum: item.style == Style::Datum,
                label: item.label,
                text: value,
                view: p.view,
                anchor_mm: p.anchor,
                label_mm: at,
            },
        );
    }
    Ok(())
}
