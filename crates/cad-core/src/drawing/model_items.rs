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

mod welds;
pub(super) use welds::WeldItems;

const TEXT_MM: f64 = 2.6;
type Point = [f64; 2];
type Bounds = [Point; 2];

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Placement {
    /// Zero selects the main sheet. Positive slots follow all saved table pages.
    #[serde(default, skip_serializing_if = "is_main_page")]
    pub model_page: u32,
    pub view: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub auto_offset: bool,
    /// Paper millimeters from the continuing dimension witness or datum anchor.
    pub offset_mm: Point,
}

fn is_main_page(page: &u32) -> bool {
    *page == 0
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PaperItem {
    #[serde(default)]
    pub model_page: u32,
    pub definition: Id,
    pub source: Id,
    pub configuration: Option<Id>,
    pub path: Vec<Id>,
    pub datum: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub weld: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub weld_views: Vec<String>,
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
        model_dimensions::remap::PaperKey::decode(key).is_err()
            || p.model_page >= 1024
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
    vertex: Option<[f64; 3]>,
    normal: Option<[f64; 3]>,
    style: Style,
}

enum Style {
    Weld(Box<welds::Bead>),
    Linear,
    Circular,
    Angular,
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
            if part.model_dimensions.has_requirements()
                || sheet.show_model_dimensions
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
                vertex: d.angle_vertex.map(|p| transform(p, m)),
                normal: if let Some(vertex) = d.angle_vertex {
                    let v = nalgebra::Vector3::from(vertex);
                    let a = nalgebra::Vector3::from(d.witnesses[0]) - v;
                    let b = nalgebra::Vector3::from(d.witnesses[1]) - v;
                    a.cross(&b)
                        .try_normalize(1e-15)
                        .map(|n| world_direction(m, n.into()))
                } else {
                    d.plane_normal.map(|n| world_direction(m, n))
                },
                style: match d.kind {
                    Kind::Linear => Style::Linear,
                    Kind::Diameter | Kind::Radius => Style::Circular,
                    Kind::Angle => Style::Angular,
                },
            });
        }
        // An assembly-level letter cannot unambiguously identify several parts' A datums.
        if !part_scope(doc, &sheet.definition) {
            continue;
        }
        for datum in report.datums.iter().filter(|d| d.explicit) {
            if let Some(target) = sheet.entities.iter().find_map(|entity| match &entity.kind {
                super::AnnotationKind::Datum { target, identifier }
                    if identifier.eq_ignore_ascii_case(&datum.label) =>
                {
                    Some(target)
                }
                _ => None,
            }) {
                if !matches!(&target.reference, Some(super::PointReference::FaceOrigin { face, .. })
                    if crate::provenance::equivalent(face, &datum.face).unwrap_or(false))
                {
                    paper.model_review.push(format!("Datum {} is also assigned on paper. Reattach the paper datum to the same model face, remove it, or change its letter.", datum.label));
                }
                continue;
            }
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
                vertex: None,
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
#[derive(Clone, Default)]
struct Occupancy {
    rows: BTreeMap<i32, Vec<Bounds>>,
    large: Vec<Bounds>,
    // Occupied space only grows. A previously rejected grid position cannot open up.
    scan_positions: BTreeMap<(u64, bool), usize>,
}
impl Occupancy {
    fn rows(b: Bounds) -> std::ops::RangeInclusive<i32> {
        (b[0][1] / 6.).floor() as i32..=(b[1][1] / 6.).floor() as i32
    }
    fn insert(&mut self, b: Bounds) {
        let rows = Self::rows(b);
        if i64::from(*rows.end()) - i64::from(*rows.start()) > 16 {
            self.large.push(b);
        } else {
            for row in rows {
                self.rows.entry(row).or_default().push(b);
            }
        }
    }
    fn intersects(&self, b: Bounds) -> bool {
        self.large.iter().any(|v| super::rectangles_overlap(*v, b))
            || Self::rows(b).any(|row| {
                self.rows
                    .get(&row)
                    .is_some_and(|values| values.iter().any(|v| super::rectangles_overlap(*v, b)))
            })
    }
}

struct Projection {
    view: String,
    anchor: Point,
    points: [Point; 2],
    vertex: Option<Point>,
    /// Only draw measurement lines/arcs in a view showing their true size.
    true_size: bool,
    quality: f64,
    weld_side: Option<welds::ProjectedSide>,
    weld_views: Vec<String>,
}

fn choose_view(
    item: &Item,
    paper: &Sheet,
    saved: Option<&Placement>,
) -> Result<Option<Projection>> {
    if let Style::Weld(bead) = &item.style {
        return bead.choose_view(paper, saved);
    }
    let delta: [f64; 3] = std::array::from_fn(|i| item.points[1][i] - item.points[0][i]);
    let span = delta.iter().map(|x| x * x).sum::<f64>().sqrt();
    let mut best: Option<Projection> = None;
    for name in [
        "front",
        "top",
        "right",
        "back",
        "bottom",
        "left",
        "isometric",
    ] {
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
            if matches!(item.style, Style::Datum) {
                in_plane
            } else {
                1. - in_plane
            }
        } else {
            ratio.min(1.)
        };
        let vertex = item
            .vertex
            .map(|p| projection.paper_point(name, p))
            .transpose()?;
        let true_size = if matches!(item.style, Style::Angular) {
            item.normal.is_some()
                && quality > 1. - 1e-7
                && vertex.is_some_and(|v| length(diff(a, v)) > 1e-9 && length(diff(b, v)) > 1e-9)
        } else {
            (ratio - 1.).abs() < 1e-7
        };
        let anchor = if matches!(item.style, Style::Circular) {
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
            vertex,
            true_size,
            quality,
            weld_side: None,
            weld_views: Vec::new(),
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
    occupied: &mut Occupancy,
    view_bounds: &BTreeMap<String, Bounds>,
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
    // Prefer exterior labels. On crowded paper use free space within projection rectangles.
    for avoid_model in [true, false] {
        let clear = |at| {
            let label = label_bounds(at, width);
            !occupied.intersects(label)
                && (!avoid_model
                    || view_bounds
                        .values()
                        .all(|bounds| !super::rectangles_overlap(label, *bounds)))
        };
        for lane in 0..10 {
            for (i, base) in primary.iter().enumerate() {
                let at = constrain([
                    base[0],
                    base[1] + lane as f64 * 5.5 * if i == 0 { -1. } else { 1. },
                ]);
                if clear(at) {
                    return Some(at);
                }
            }
        }
        let dx = ((area[1][0] - area[0][0]) / 80.).max(5.);
        let dy = ((area[1][1] - area[0][1]) / 80.).max(5.5);
        let cols = ((area[1][0] - area[0][0] - width - 2.) / dx).floor() as usize + 1;
        let rows = ((area[1][1] - area[0][1] - TEXT_MM - 2.4) / dy).floor() as usize + 1;
        let key = (width.to_bits(), avoid_model);
        let start = occupied.scan_positions.get(&key).copied().unwrap_or(0);
        let limit = cols * rows;
        let candidate = (start..limit).find_map(|index| {
            let at = [
                area[0][0] + 1. + (index % cols) as f64 * dx,
                area[1][1] - TEXT_MM - 1. - (index / cols) as f64 * dy,
            ];
            clear(at).then_some((index, at))
        });
        occupied
            .scan_positions
            .insert(key, candidate.map_or(limit, |(index, _)| index + 1));
        if let Some((_, at)) = candidate {
            return Some(at);
        }
    }
    None
}

fn angular_arc(out: &mut Vec<Primitive>, center: Point, rays: [Point; 2], label: Point) {
    let [a, b] = rays.map(|p| diff(p, center));
    let start = a[1].atan2(a[0]);
    let sweep = (a[0] * b[1] - a[1] * b[0]).atan2(dot(a, b));
    let radius = length(diff(label, center))
        .min(length(a).min(length(b)) + 4.)
        .clamp(2., 16.);
    let station = |t: f64| {
        let angle = start + sweep * t;
        sum(center, [radius * angle.cos(), radius * angle.sin()])
    };
    let divisions = (sweep.abs() * (radius / 0.1).sqrt()).ceil().clamp(2., 128.) as usize;
    let arc = (0..=divisions)
        .map(|i| station(i as f64 / divisions as f64))
        .collect::<Vec<_>>();
    line(out, center, arc[0]);
    line(out, center, arc[divisions]);
    arrow(out, arc[0], arc[1]);
    arrow(out, arc[divisions], arc[divisions - 1]);
    out.push(Primitive::Polyline {
        points: arc,
        dashed: false,
    });
    line(out, station(0.5), label);
}

fn draw(out: &mut Vec<Primitive>, item: &Item, p: &Projection, at: Point, label: &str) {
    let [a, b] = p.points;
    let width = label_width(label);
    let base = [at[0] + width / 2., at[1] - 0.8];
    if matches!(item.style, Style::Angular)
        && p.true_size
        && let Some(vertex) = p.vertex
    {
        angular_arc(out, vertex, p.points, base);
    } else if matches!(item.style, Style::Linear) && p.true_size && length(diff(b, a)) > 1e-9 {
        let direction = mul(diff(b, a), 1. / length(diff(b, a)));
        let normal = [-direction[1], direction[0]];
        let label_center = [at[0] + width * 0.5, at[1] + TEXT_MM * 0.4];
        let distance = dot(diff(label_center, p.anchor), normal);
        let label_clearance = normal[0].abs() * width * 0.5 + normal[1].abs() * TEXT_MM * 0.6 + 0.8;
        let shift = mul(normal, distance - distance.signum() * label_clearance);
        let (start, end) = (sum(a, shift), sum(b, shift));
        line(out, a, start);
        line(out, b, end);
        line(out, start, end);
        arrow(out, start, end);
        arrow(out, end, start);
        let along = dot(diff(base, start), direction);
        if along < 0. {
            line(out, start, sum(start, mul(direction, along)));
        }
        if along > length(diff(end, start)) {
            line(out, end, sum(start, mul(direction, along)));
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
        if matches!(item.style, Style::Datum) {
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

/// A layer is laid out before any large view vectors are copied to continuation sheets.
#[derive(Default)]
pub(super) struct CalloutLayer {
    pub primitives: Vec<Primitive>,
    pub items: BTreeMap<Id, PaperItem>,
    pub groups: Vec<PrimitiveGroup>,
    pub review: Vec<String>,
    occupied: Occupancy,
}

impl CalloutLayer {
    pub fn append(self, paper: &mut Sheet) {
        let start = paper.primitives.len();
        paper.primitives.extend(self.primitives);
        paper.model_items.extend(self.items);
        paper.model_review.extend(self.review);
        paper
            .groups
            .extend(self.groups.into_iter().map(|mut group| {
                group.primitive_start += start;
                group
            }));
    }
}

fn reserve_text(occupied: &mut Occupancy, primitive: &Primitive) {
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

pub(super) struct PaperSpace<'a> {
    pub footer: f64,
    pub model_bounds: &'a BTreeMap<String, Bounds>,
    pub view_ranges: &'a BTreeMap<String, std::ops::Range<usize>>,
}

pub(super) fn project_items(
    doc: &Document,
    e: &Evaluation,
    sheet: &Drawing,
    instances: &[DisplayInstance],
    paper: &mut Sheet,
    space: PaperSpace<'_>,
    welds: WeldItems,
) -> Result<Vec<CalloutLayer>> {
    let mut items = collect(doc, e, sheet, instances, paper)?;
    welds.append(&mut items, &mut paper.model_review);
    if !paper.model_coverage.is_empty() {
        let notes = sheet.annotations.len() + sheet.title_block.len();
        text(
            &mut paper.primitives,
            [
                paper_bounds(sheet)[0][0] + 4.,
                space.footer - 6. - notes as f64 * 3.5,
            ],
            2.6,
            "Automatic dimension coverage is partial",
        );
    }
    if items.is_empty() {
        return Ok(vec![]);
    }
    // A moved item reserves its original page and location before automatic placement.
    items.sort_by_key(|item| {
        let automatic = sheet
            .model_item_placements
            .get(&item.id)
            .is_none_or(|p| p.auto_offset);
        (automatic, !matches!(item.style, Style::Weld(_)))
    });
    let bounds = paper_bounds(sheet);
    let mut main_area = bounds;
    main_area[0][1] = main_area[0][1].max(space.footer + 3.);
    let mut extra_area = bounds;
    extra_area[0][1] = super::title_bounds(sheet).map_or(bounds[0][1] + 5., |b| b[1][1] + 2.);
    extra_area[1][1] -= 9.;
    let mut occupied = Occupancy::default();
    for primitive in &paper.primitives {
        reserve_text(&mut occupied, primitive);
    }
    // Reserve table rectangles as well as cell text, including floating tables.
    for group in &paper.groups {
        if matches!(
            group.target,
            Target::Table { .. } | Target::TableSegment { .. }
        ) && let Some(b) = crate::drawing_interaction::bounds(
            &paper.primitives[group.primitive_start..group.primitive_start + group.primitive_count],
        ) {
            occupied.insert(b);
        }
    }
    let mut extra_occupied = Occupancy::default();
    for range in space.view_ranges.values() {
        for primitive in &paper.primitives[range.clone()] {
            reserve_text(&mut extra_occupied, primitive);
        }
    }
    let max_slot = (1024 - crate::drawing_tables::page_count(sheet)) as usize;
    let mut layers = vec![CalloutLayer {
        occupied,
        ..Default::default()
    }];
    let mut current_slot = 0;
    for item in items {
        let saved = sheet.model_item_placements.get(&item.id);
        let Some(p) = choose_view(&item, paper, saved)? else {
            if matches!(item.style, Style::Weld(_)) {
                if saved.is_some() {
                    paper.model_review.push(format!("{} is outside its saved weld view. Restore a view showing the bead or reset its placement.", item.label));
                }
            } else {
                let message = "Add an ordinary model view to place model dimensions and datums";
                if !paper.model_review.iter().any(|m| m == message) {
                    paper.model_review.push(message.into());
                }
            }
            continue;
        };
        if saved.is_some_and(|placement| placement.view != p.view) {
            paper.model_review.push(format!(
                "{}'s saved view is missing. Restore the view or reset the callout placement.",
                item.label
            ));
        }
        let value = if matches!(item.style, Style::Linear | Style::Angular) && !p.true_size {
            format!("{} {}", item.label, item.text)
        } else {
            item.text.clone()
        };
        let weld_layout = if let Style::Weld(bead) = &item.style {
            Some(bead.layout(doc, &item.definition, &p)?)
        } else {
            None
        };
        let width = label_width(&value);
        let mut slot = saved.map_or(current_slot, |v| v.model_page as usize);
        if slot > max_slot {
            paper.model_review.push(format!("{}'s saved page exceeds the 1024-page sheet limit. Reset its placement or split the drawing.", item.label));
            slot = 0;
        }
        let fallback = sum(p.anchor, [5., 6.]);
        let at = loop {
            if slot >= layers.len() {
                layers.resize_with(slot + 1, || CalloutLayer {
                    occupied: extra_occupied.clone(),
                    ..Default::default()
                });
            }
            if let Some(saved) = saved.filter(|p| !p.auto_offset) {
                break sum(p.anchor, saved.offset_mm);
            }
            if weld_layout.is_none() && width + 2. > bounds[1][0] - bounds[0][0] {
                layers[slot].review.push(format!(
                    "{} is wider than the printable area. Enlarge the paper.",
                    item.label
                ));
                break fallback;
            }
            let layer = &mut layers[slot];
            let area = if slot == 0 { main_area } else { extra_area };
            let available = if let Some(weld) = &weld_layout {
                weld.place(area, &layer.occupied, space.model_bounds)
            } else {
                auto_label(
                    &p,
                    width,
                    area,
                    &paper.projections[&p.view],
                    &mut layer.occupied,
                    space.model_bounds,
                )
            };
            if let Some(at) = available {
                current_slot = slot;
                break at;
            }
            if slot == max_slot || (slot > 0 && layer.items.is_empty()) {
                layers[slot].review.push(format!(
                    "{} cannot fit on a continuation page. Enlarge the paper or split the drawing.",
                    item.label
                ));
                break fallback;
            }
            slot += 1;
        };
        let layer = &mut layers[slot];
        let envelope = weld_layout
            .as_ref()
            .map_or_else(|| label_bounds(at, width), |weld| weld.bounds(at));
        if !inside(envelope, bounds) {
            layer.review.push(format!(
                "{} falls outside the paper; move its label or enlarge the sheet",
                item.label
            ));
        }
        layer.occupied.insert(envelope);
        let start = layer.primitives.len();
        if let Some(weld) = &weld_layout {
            weld.draw(at, &mut layer.primitives, &mut layer.review, &item.label)?;
        } else {
            draw(&mut layer.primitives, &item, &p, at, &value);
        }
        layer.groups.push(PrimitiveGroup {
            target: Target::ModelItem {
                id: item.id.clone(),
            },
            view: Some(p.view.clone()),
            primitive_start: start,
            primitive_count: layer.primitives.len() - start,
        });
        let text = if let Some(weld) = &weld_layout {
            weld.description(at, value, &layer.primitives[start..])
        } else {
            value
        };
        layer.items.insert(
            item.id,
            PaperItem {
                model_page: slot as u32,
                definition: item.definition,
                source: item.source,
                configuration: item.configuration,
                path: item.path,
                datum: matches!(item.style, Style::Datum),
                weld: matches!(item.style, Style::Weld(_)),
                weld_views: p.weld_views,
                label: item.label,
                text,
                view: p.view,
                anchor_mm: p.anchor,
                label_mm: at,
            },
        );
    }
    let extra = layers.split_off(1);
    layers.pop().unwrap().append(paper);
    Ok(extra)
}
