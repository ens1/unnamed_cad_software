//! Continuous screen-space visibility against a cached, tiled display mesh.
//! The result is exact for disposable triangles, never analytic CAD visibility.
use crate::{Workspace, viewport};
use bevy::math::DVec3;
use bevy_egui::egui::{Pos2, Rect};
use std::{collections::BTreeSet, time::Instant};
const TILE: f64 = 48.;
const MAX_TRIANGLES: usize = 500_000;
const MAX_REFERENCES: usize = 2_000_000;
#[derive(Clone, Copy)]
struct Point {
    x: f64,
    y: f64,
    z: f64,
}
#[derive(Clone)]
struct Triangle {
    points: [Point; 3],
    plane: [f64; 3],
    sign: f64,
}
#[derive(Default)]
struct Index {
    key: String,
    triangles: Vec<Triangle>,
    cells: Vec<Vec<usize>>,
    columns: usize,
    rows: usize,
    complete: bool,
    exhausted: bool,
    mesh_cursor: usize,
    triangle_cursor: usize,
    source_triangles: usize,
    references: usize,
    last_build: Option<u64>,
    epoch: u64,
    bias_mm: f64,
    triangle_budget: usize,
    reference_budget: usize,
}
/// Four contexts share aggregate storage and one ~1.5 ms frame-time slice.
#[derive(Default)]
pub struct Cache {
    entries: Vec<Entry>,
    source: String,
    clock: u64,
    epoch: u64,
    last_build: Option<u64>,
}
struct Entry {
    rect: [u32; 4],
    index: Index,
    clock: u64,
    seen: u64,
}
impl Cache {
    fn index(&mut self, w: &Workspace, rect: Rect) -> &mut Index {
        let source = key(w, Rect::ZERO);
        if source != self.source {
            self.source = source;
            self.epoch = self.epoch.wrapping_add(1);
            for entry in &mut self.entries {
                entry.index = Index::default();
            }
        }
        let previous = self.entries.len();
        self.entries
            .retain(|entry| w.display_mesh.frame.saturating_sub(entry.seen) <= 8);
        if self.entries.len() != previous {
            self.epoch = self.epoch.wrapping_add(1);
        }
        self.clock = self.clock.wrapping_add(1);
        let rect_key = [
            rect.min.x.to_bits(),
            rect.min.y.to_bits(),
            rect.max.x.to_bits(),
            rect.max.y.to_bits(),
        ];
        let slot = if let Some(slot) = self.entries.iter().position(|e| e.rect == rect_key) {
            slot
        } else {
            if self.entries.len() == 4 {
                let oldest = self
                    .entries
                    .iter()
                    .enumerate()
                    .min_by_key(|(_, e)| e.clock)
                    .map(|(i, _)| i)
                    .unwrap_or(0);
                self.entries.remove(oldest);
                self.epoch = self.epoch.wrapping_add(1);
            }
            self.entries.push(Entry {
                rect: rect_key,
                index: Index::default(),
                clock: self.clock,
                seen: w.display_mesh.frame,
            });
            self.entries.len() - 1
        };
        let triangles = MAX_TRIANGLES.saturating_sub(
            self.entries
                .iter()
                .enumerate()
                .filter(|(i, _)| *i != slot)
                .map(|(_, e)| e.index.triangles.len())
                .sum(),
        );
        let references = MAX_REFERENCES.saturating_sub(
            self.entries
                .iter()
                .enumerate()
                .filter(|(i, _)| *i != slot)
                .map(|(_, e)| e.index.references)
                .sum(),
        );
        let entry = &mut self.entries[slot];
        entry.clock = self.clock;
        entry.seen = w.display_mesh.frame;
        if entry.index.exhausted
            && (triangles > entry.index.triangle_budget
                || references > entry.index.reference_budget)
        {
            entry.index = Index::default();
            self.epoch = self.epoch.wrapping_add(1);
        }
        entry.index.triangle_budget = triangles;
        entry.index.reference_budget = references;
        entry.index.refresh(w, rect, false);
        let pending = self
            .entries
            .iter()
            .enumerate()
            .filter(|(_, e)| {
                !e.index.complete
                    && !e.index.exhausted
                    && w.display_mesh.frame.saturating_sub(e.seen) <= 1
            })
            .map(|(i, _)| i)
            .collect::<Vec<_>>();
        let chosen =
            !pending.is_empty() && pending[w.display_mesh.frame as usize % pending.len()] == slot;
        if chosen && self.last_build != Some(w.display_mesh.frame) {
            self.last_build = Some(w.display_mesh.frame);
            self.entries[slot].index.refresh(w, rect, true);
        }
        &mut self.entries[slot].index
    }
}
fn point(w: &Workspace, rect: Rect, world: [f64; 3]) -> Option<Point> {
    let pose = crate::camera_views_ui::current(w);
    let frame = pose.frame();
    let offset = DVec3::from_array(world) - DVec3::from_array(frame.eye_mm);
    let depth = offset.dot(DVec3::from_array(frame.forward));
    if !depth.is_finite() || depth < (pose.distance_mm * 0.0001).max(0.001) {
        return None;
    }
    let perspective = matches!(
        pose.projection,
        cad_core::camera_views::Projection::Perspective { .. }
    );
    let divisor = if perspective { depth } else { 1. };
    let [fx, fy] = viewport::focal(w, rect);
    let x = offset.dot(DVec3::from_array(frame.right)) * fx / divisor + f64::from(rect.center().x);
    let y = -offset.dot(DVec3::from_array(frame.up)) * fy / divisor + f64::from(rect.center().y);
    let z = if perspective { 1. / depth } else { -depth };
    (x.is_finite() && y.is_finite() && z.is_finite()).then_some(Point { x, y, z })
}
fn cross(a: Point, b: Point, c: Point) -> f64 {
    (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x)
}
impl Triangle {
    fn new(points: [Point; 3]) -> Option<Self> {
        let [a, b, c] = points;
        let area = cross(a, b, c);
        if !area.is_finite() || area.abs() < 1e-9 {
            return None;
        }
        let dx = ((b.z - a.z) * (c.y - a.y) - (c.z - a.z) * (b.y - a.y)) / area;
        let dy = ((b.x - a.x) * (c.z - a.z) - (c.x - a.x) * (b.z - a.z)) / area;
        let dz = a.z - dx * a.x - dy * a.y;
        Some(Self {
            points,
            plane: [dx, dy, dz],
            sign: area.signum(),
        })
    }
    fn depth(&self, p: Point) -> f64 {
        self.plane[0] * p.x + self.plane[1] * p.y + self.plane[2]
    }
    fn covers(&self, p: Point) -> bool {
        (0..3).all(|i| cross(self.points[i], self.points[(i + 1) % 3], p) * self.sign >= -1e-7)
    }
    fn hidden_interval(&self, a: Point, b: Point, bias_mm: f64) -> Option<[f64; 2]> {
        let mut range = [0., 1.];
        for i in 0..3 {
            let p = self.points[i];
            let q = self.points[(i + 1) % 3];
            if !positive_interval(
                cross(p, q, a) * self.sign,
                cross(p, q, b) * self.sign,
                &mut range,
            ) {
                return None;
            }
        }
        let epsilon = depth_epsilon(a.z.abs().max(b.z.abs()), a.z > 0., bias_mm);
        if !positive_interval(
            self.depth(a) - a.z - epsilon,
            self.depth(b) - b.z - epsilon,
            &mut range,
        ) {
            return None;
        }
        (range[1] - range[0] > 1e-9).then_some(range)
    }
}
fn depth_epsilon(value: f64, perspective: bool, bias_mm: f64) -> f64 {
    // Inverse depth has reciprocal-mm units: a fixed epsilon would hide
    // millimetres at close zoom and kilometres at distant zoom. Keep the bias
    // physical, plus a relative floating-point allowance for interpolation.
    let physical = if perspective {
        value * value * bias_mm
    } else {
        bias_mm
    };
    physical + value * f64::EPSILON * 64.
}
fn positive_interval(a: f64, b: f64, range: &mut [f64; 2]) -> bool {
    if a < 0. && b < 0. {
        return false;
    }
    if a >= 0. && b >= 0. {
        return true;
    }
    let t = a / (a - b);
    if a < 0. {
        range[0] = range[0].max(t);
    } else {
        range[1] = range[1].min(t);
    }
    range[0] <= range[1]
}
fn key(w: &Workspace, rect: Rect) -> String {
    let e = viewport::display_evaluation(w);
    let selection = viewport::selection_evaluation(w);
    let instances = e.map(|e| {
        viewport::display_instances(w, e)
            .into_iter()
            .map(|i| {
                (
                    &i.path,
                    i.matrix,
                    viewport::visible(w, i),
                    viewport::ghost_neighbor(w, i),
                )
            })
            .collect::<Vec<_>>()
    });
    let fabrication = crate::fabrication_sources_ui::input_body(w);
    let body = crate::selection_ui::edge_feature(w)
        .and_then(|f| crate::selection_ui::edge_selection(&f).map(|(body, _)| body.clone()));
    format!(
        "{:?}:{rect:?}:{:?}:{:?}:{:?}:{:?}:{}:{instances:?}:{body:?}:{fabrication:?}:{:?}",
        crate::camera_views_ui::current(w),
        e.map(|e| &e.identity),
        selection.map(|e| &e.identity),
        w.view.history_cursor,
        crate::selection_ui::edge_feature(w).map(|f| f.id),
        w.display_mesh.epoch,
        w.edge_selection_path
    )
}
impl Index {
    fn ranges(&self, rect: Rect, a: Point, b: Point) -> Option<[(usize, usize); 2]> {
        let lo = [
            a.x.min(b.x).max(f64::from(rect.min.x)),
            a.y.min(b.y).max(f64::from(rect.min.y)),
        ];
        let hi = [
            a.x.max(b.x).min(f64::from(rect.max.x)),
            a.y.max(b.y).min(f64::from(rect.max.y)),
        ];
        if lo[0] > hi[0] || lo[1] > hi[1] {
            return None;
        }
        Some([
            (
                ((lo[0] - f64::from(rect.min.x)) / TILE).floor().max(0.) as usize,
                (((hi[0] - f64::from(rect.min.x)) / TILE).floor().max(0.) as usize)
                    .min(self.columns - 1),
            ),
            (
                ((lo[1] - f64::from(rect.min.y)) / TILE).floor().max(0.) as usize,
                (((hi[1] - f64::from(rect.min.y)) / TILE).floor().max(0.) as usize)
                    .min(self.rows - 1),
            ),
        ])
    }
    fn insert(&mut self, rect: Rect, triangle: Triangle, references: &mut usize) -> bool {
        let lo = Point {
            x: triangle
                .points
                .iter()
                .map(|p| p.x)
                .fold(f64::INFINITY, f64::min),
            y: triangle
                .points
                .iter()
                .map(|p| p.y)
                .fold(f64::INFINITY, f64::min),
            z: 0.,
        };
        let hi = Point {
            x: triangle
                .points
                .iter()
                .map(|p| p.x)
                .fold(f64::NEG_INFINITY, f64::max),
            y: triangle
                .points
                .iter()
                .map(|p| p.y)
                .fold(f64::NEG_INFINITY, f64::max),
            z: 0.,
        };
        let Some([(x0, x1), (y0, y1)]) = self.ranges(rect, lo, hi) else {
            return true;
        };
        let count = (x1 - x0 + 1).saturating_mul(y1 - y0 + 1);
        *references = references.saturating_add(count);
        if *references > self.reference_budget || self.triangles.len() >= self.triangle_budget {
            return false;
        }
        let index = self.triangles.len();
        self.triangles.push(triangle);
        for y in y0..=y1 {
            for x in x0..=x1 {
                self.cells[y * self.columns + x].push(index);
            }
        }
        true
    }
    fn refresh(&mut self, w: &Workspace, rect: Rect, build: bool) {
        let key = format!("{}:{:?}", key(w, rect), w.viewport_size);
        if self.key != key {
            self.epoch = self.epoch.wrapping_add(1);
            self.key = key;
            self.triangles.clear();
            self.complete = false;
            self.exhausted = false;
            self.mesh_cursor = 0;
            self.triangle_cursor = 0;
            self.source_triangles = 0;
            self.references = 0;
            self.bias_mm =
                (w.session.visible().settings.display_tolerance_mm * 0.5).clamp(1e-6, 0.05);
            self.columns = (f64::from(rect.width().max(1.)) / TILE).ceil().max(1.) as usize;
            self.rows = (f64::from(rect.height().max(1.)) / TILE).ceil().max(1.) as usize;
            if self.columns.saturating_mul(self.rows) > 65_536 {
                self.cells.clear();
                self.exhausted = true;
                return;
            }
            self.cells = vec![Vec::new(); self.columns * self.rows];
        }
        if !build
            || self.complete
            || self.exhausted
            || self.last_build == Some(w.display_mesh.frame)
        {
            return;
        }
        let started = Instant::now();
        self.last_build = Some(w.display_mesh.frame);
        let Some(evaluation) =
            viewport::selection_evaluation(w).or_else(|| viewport::display_evaluation(w))
        else {
            self.exhausted = true;
            return;
        };
        let displayed = viewport::display_evaluation(w).unwrap_or(evaluation);
        let inputs = viewport::display_instances(w, evaluation);
        let mut sources = Vec::new();
        for display in viewport::display_instances(w, displayed) {
            if !viewport::visible(w, display) || viewport::ghost_neighbor(w, display) {
                continue;
            }
            let input = inputs
                .iter()
                .copied()
                .find(|i| i.path == display.path && i.definition == display.definition);
            let input_meshes = input
                .map(|i| viewport::selection_meshes(w, evaluation, i))
                .unwrap_or_default();
            let (instance, meshes) = if let Some(i) = input.filter(|_| !input_meshes.is_empty()) {
                (i, input_meshes)
            } else {
                (display, viewport::display_meshes(w, displayed, display))
            };
            sources.extend(meshes.into_iter().map(|mesh| (instance, mesh)));
        }
        let pose = crate::camera_views_ui::current(w);
        let frame = pose.frame();
        let eye = DVec3::from_array(frame.eye_mm);
        let forward = DVec3::from_array(frame.forward);
        let right = DVec3::from_array(frame.right);
        let up = DVec3::from_array(frame.up);
        let near = (pose.distance_mm * 0.0001).max(0.001);
        let perspective = matches!(
            pose.projection,
            cad_core::camera_views::Projection::Perspective { .. }
        );
        let [fx, fy] = viewport::focal(w, rect);
        let project = |v: DVec3| {
            let o = v - eye;
            let d = o.dot(forward);
            let divisor = if perspective { d } else { 1. };
            Point {
                x: o.dot(right) * fx / divisor + f64::from(rect.center().x),
                y: -o.dot(up) * fy / divisor + f64::from(rect.center().y),
                z: if perspective { 1. / d } else { -d },
            }
        };
        while let Some((instance, mesh)) = sources.get(self.mesh_cursor) {
            let triangles = mesh.indices.as_chunks::<3>().0;
            while let Some(indices) = triangles.get(self.triangle_cursor) {
                self.triangle_cursor += 1;
                self.source_triangles += 1;
                if self.source_triangles > MAX_TRIANGLES {
                    self.exhausted = true;
                    return;
                }
                let vertices = indices.map(|i| {
                    DVec3::from_array(viewport::placed(mesh.positions[i as usize], instance))
                });
                let mut clipped = Vec::with_capacity(4);
                for i in 0..3 {
                    let a = vertices[i];
                    let b = vertices[(i + 1) % 3];
                    let da = (a - eye).dot(forward);
                    let db = (b - eye).dot(forward);
                    if da >= near {
                        clipped.push(a);
                    }
                    if (da >= near) != (db >= near) {
                        clipped.push(a + (b - a) * ((near - da) / (db - da)));
                    }
                }
                for i in 1..clipped.len().saturating_sub(1) {
                    if let Some(t) = Triangle::new([
                        project(clipped[0]),
                        project(clipped[i]),
                        project(clipped[i + 1]),
                    ]) {
                        let mut references = self.references;
                        if !self.insert(rect, t, &mut references) {
                            self.exhausted = true;
                            return;
                        }
                        self.references = references;
                    }
                }
                // Navigation never waits for the full visibility index. Continue
                // on later frames and omit optional decoration until complete.
                if started.elapsed().as_micros() >= 1500 {
                    return;
                }
            }
            self.mesh_cursor += 1;
            self.triangle_cursor = 0;
        }
        self.complete = true;
        self.epoch = self.epoch.wrapping_add(1);
    }
    fn candidates(
        &self,
        rect: Rect,
        a: Point,
        b: Point,
        work: &mut usize,
    ) -> Option<BTreeSet<usize>> {
        let mut interval = [0., 1.];
        for (a, b, lo, hi) in [
            (a.x, b.x, f64::from(rect.min.x), f64::from(rect.max.x)),
            (a.y, b.y, f64::from(rect.min.y), f64::from(rect.max.y)),
        ] {
            if !positive_interval(a - lo, b - lo, &mut interval)
                || !positive_interval(hi - a, hi - b, &mut interval)
            {
                return Some(BTreeSet::new());
            }
        }
        let mut cuts = vec![interval[0], interval[1]];
        for (start, end, origin, count) in [
            (a.x, b.x, f64::from(rect.min.x), self.columns),
            (a.y, b.y, f64::from(rect.min.y), self.rows),
        ] {
            if (end - start).abs() < 1e-12 {
                continue;
            }
            for k in 1..count {
                let t = (origin + k as f64 * TILE - start) / (end - start);
                if t > interval[0] && t < interval[1] {
                    cuts.push(t);
                }
            }
        }
        cuts.sort_by(f64::total_cmp);
        cuts.dedup_by(|a, b| (*a - *b).abs() < 1e-12);
        if cuts.len() == 1 {
            cuts.push(cuts[0]);
        }
        let mut cells = BTreeSet::new();
        for pair in cuts.windows(2) {
            let t = (pair[0] + pair[1]) * 0.5;
            let x = (a.x + (b.x - a.x) * t - f64::from(rect.min.x)) / TILE;
            let y = (a.y + (b.y - a.y) * t - f64::from(rect.min.y)) / TILE;
            let ix = (x.floor().max(0.) as usize).min(self.columns - 1);
            let iy = (y.floor().max(0.) as usize).min(self.rows - 1);
            cells.insert(iy * self.columns + ix);
            // A line on a tile boundary belongs to both adjacent cells.
            if x.fract().abs() < 1e-9 && ix > 0 {
                cells.insert(iy * self.columns + ix - 1);
            }
            if y.fract().abs() < 1e-9 && iy > 0 {
                cells.insert((iy - 1) * self.columns + ix);
            }
        }
        let mut candidates = BTreeSet::new();
        for cell in cells {
            for index in &self.cells[cell] {
                *work = work.checked_sub(1)?;
                candidates.insert(*index);
            }
        }
        Some(candidates)
    }
}
pub fn visible(w: &Workspace, rect: Rect, world: [f64; 3], work: &mut usize) -> bool {
    let Some(p) = point(w, rect, world) else {
        return false;
    };
    let Ok(mut cache) = w.overlay_depth.lock() else {
        return false;
    };
    let cache = cache.index(w, rect);
    if !cache.complete {
        return false;
    }
    let Some(candidates) = cache.candidates(rect, p, p, work) else {
        return false;
    };
    !candidates.into_iter().any(|i| {
        let t = &cache.triangles[i];
        t.covers(p) && t.depth(p) > p.z + depth_epsilon(p.z.abs(), p.z > 0., cache.bias_mm)
    })
}
pub fn segments(
    w: &Workspace,
    rect: Rect,
    endpoints: [[f64; 3]; 2],
    work: &mut usize,
) -> Vec<[Pos2; 2]> {
    let Some(a) = point(w, rect, endpoints[0]) else {
        return Vec::new();
    };
    let Some(b) = point(w, rect, endpoints[1]) else {
        return Vec::new();
    };
    let Ok(mut cache) = w.overlay_depth.lock() else {
        return Vec::new();
    };
    let cache = cache.index(w, rect);
    if !cache.complete {
        return Vec::new();
    }
    let Some(candidates) = cache.candidates(rect, a, b, work) else {
        return Vec::new();
    };
    let mut hidden = candidates
        .into_iter()
        .filter_map(|i| cache.triangles[i].hidden_interval(a, b, cache.bias_mm))
        .collect::<Vec<_>>();
    hidden.sort_by(|a, b| a[0].total_cmp(&b[0]));
    let at = |t: f64| {
        Pos2::new(
            (a.x + (b.x - a.x) * t) as f32,
            (a.y + (b.y - a.y) * t) as f32,
        )
    };
    let mut cursor = 0.;
    let mut result = Vec::new();
    for [lo, hi] in hidden {
        if lo > cursor + 1e-9 {
            result.push([at(cursor), at(lo)]);
        }
        cursor = cursor.max(hi);
    }
    if cursor < 1. - 1e-9 {
        result.push([at(cursor), at(1.)]);
    }
    result
}

/// Advance the bounded index and let cached hover misses expire on completion.
pub fn token(w: &Workspace, rect: Rect) -> (u64, bool, bool) {
    let Ok(mut cache) = w.overlay_depth.lock() else {
        return (0, false, true);
    };
    let (index_epoch, complete, exhausted) = {
        let index = cache.index(w, rect);
        (index.epoch, index.complete, index.exhausted)
    };
    (
        cache.epoch.wrapping_mul(65537).wrapping_add(index_epoch),
        complete,
        exhausted,
    )
}
