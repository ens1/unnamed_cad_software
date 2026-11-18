//! Authored cutting planes and disposable, native section boundaries in model millimetres.
use crate::{
    CadError, Document, Drawing, Id, Result,
    drawing::{self, CurveReference, PointReference},
    protocol::Evaluation,
    sketch_edit::Curve3,
};
use serde::{Deserialize, Serialize};

type Point = [f64; 3];
pub const KEY: &str = "section-z";
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Definition {
    pub id: Id,
    pub parent: String,
    /// Unscaled model coordinates in the parent view for an unanchored cut.
    pub at: [f64; 2],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub through: Option<PointReference>,
    /// A section transferred from a model view is fixed in the sheet model's coordinates.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_plane: Option<Box<Frame>>,
    pub direction: [f64; 2],
    pub reversed: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Frame {
    pub origin: Point,
    pub u: Point,
    pub v: Point,
    pub normal: Point,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Region {
    pub loops: Vec<Vec<Curve3>>,
    pub ghost: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Vertex {
    pub reference: PointReference,
    pub position: Point,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Evaluated {
    pub source: Definition,
    pub frame: Option<Frame>,
    pub regions: Vec<Region>,
    pub vertices: Vec<Vertex>,
    pub error: Option<String>,
}
pub fn dot(a: Point, b: Point) -> f64 {
    a.into_iter().zip(b).map(|(a, b)| a * b).sum()
}
fn cross(a: Point, b: Point) -> Point {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn invalid(s: &str) -> CadError {
    CadError::Invalid(s.into())
}
impl Definition {
    pub fn validate(&self, sheet: &Drawing) -> Result<()> {
        if let Some(&frame) = self.model_plane.as_deref() {
            frame.validate()?;
            if self.through.is_some() {
                return Err(invalid(
                    "A model section already specifies its cutting plane",
                ));
            }
        }
        let length = self.direction[0].hypot(self.direction[1]);
        if self.id.trim().is_empty()
            || self.id.len() > 256
            || self.id.chars().any(char::is_control)
            || !length.is_finite()
            || self.model_plane.is_none()
                && (drawing::orthographic_axes(&self.parent).is_none()
                    || !sheet.views.contains(&self.parent))
            || !sheet.views.iter().any(|s| s == KEY)
            || self
                .direction
                .iter()
                .chain(&self.at)
                .any(|x| !x.is_finite())
            || length < 1e-9
            || matches!(self.through, Some(PointReference::SectionVertex { .. }))
        {
            return Err(invalid(
                "The section needs an orthographic parent and a straight cutting line with two distinct points",
            ));
        }
        Ok(())
    }
    pub fn frame(&self, doc: &Document, e: &Evaluation, sheet: &Drawing) -> Result<Frame> {
        self.validate(sheet)?;
        if let Some(&frame) = self.model_plane.as_deref() {
            return Ok(if self.reversed {
                frame.flipped()
            } else {
                frame
            });
        }
        let [a, b] = drawing::orthographic_axes(&self.parent)
            .ok_or_else(|| invalid("Choose an orthographic parent for the section"))?;
        let sign = if self.reversed { -1. } else { 1. };
        let len = self.direction[0].hypot(self.direction[1]);
        let d = self.direction.map(|x| sign * x / len);
        let u = std::array::from_fn(|i| d[0] * a[i] + d[1] * b[i]);
        let v = cross(a, b);
        let origin = drawing::point(
            doc,
            e,
            sheet,
            std::array::from_fn(|i| self.at[0] * a[i] + self.at[1] * b[i]),
            &self.through,
        )?;
        Ok(Frame {
            origin,
            u,
            v,
            normal: cross(u, v),
        })
    }
}
impl Frame {
    pub fn validate(&self) -> Result<()> {
        let axes = [self.u, self.v, self.normal];
        if self
            .origin
            .iter()
            .chain(axes.iter().flatten())
            .any(|x| !x.is_finite())
            || axes.iter().any(|a| (dot(*a, *a) - 1.).abs() > 1e-8)
            || dot(self.u, self.v).abs() > 1e-8
            || cross(self.u, self.v)
                .iter()
                .zip(self.normal)
                .any(|(a, b)| (a - b).abs() > 1e-8)
        {
            return Err(invalid(
                "The section plane needs a finite origin and perpendicular unit axes",
            ));
        }
        Ok(())
    }
    pub fn flipped(self) -> Self {
        Self {
            u: self.u.map(|x| -x),
            normal: self.normal.map(|x| -x),
            ..self
        }
    }
    pub fn edge_on(&self, parent: &str) -> bool {
        drawing::orthographic_axes(parent)
            .is_some_and(|[a, b]| dot(self.normal, cross(a, b)).abs() <= 1e-8)
    }
    pub fn project(&self, p: Point) -> [f64; 2] {
        [dot(self.u, p), dot(self.v, p)]
    }
    pub fn distance(&self, p: Point) -> f64 {
        dot(self.normal, std::array::from_fn(|i| p[i] - self.origin[i]))
    }
}
pub fn current<'a>(e: &'a Evaluation, sheet: &Drawing) -> Result<&'a Evaluated> {
    let cut = e
        .drawing_sections
        .get(&sheet.id)
        .filter(|cut| Some(&cut.source) == sheet.section.as_ref())
        .ok_or_else(|| invalid("Waiting for the current cutting plane to intersect the model"))?;
    if let Some(error) = &cut.error {
        return Err(invalid(error));
    }
    Ok(cut)
}
pub fn vertex(
    doc: &Document,
    e: &Evaluation,
    sheet: &Drawing,
    reference: &PointReference,
) -> Result<Point> {
    let PointReference::SectionVertex {
        section,
        source,
        crossing,
        count,
    } = reference
    else {
        return Err(invalid("Expected a section reference"));
    };
    let CurveReference::ModelEdge {
        edge,
        body,
        instance_path,
    } = source.as_ref()
    else {
        return Err(invalid("A section intersection needs a native model edge"));
    };
    let wanted = crate::provenance::normalized_reference(edge)?;
    let mut position = None;
    for candidate in &current(e, sheet)?.vertices {
        let PointReference::SectionVertex {
            section: id,
            source,
            crossing: branch,
            count: total,
        } = &candidate.reference
        else {
            continue;
        };
        if section != id || count != total || crossing != branch {
            continue;
        }
        let CurveReference::ModelEdge {
            edge: actual,
            body: owner,
            instance_path: path,
        } = source.as_ref()
        else {
            continue;
        };
        if path == instance_path
            && drawing::body_matches(doc, owner, body)
            && crate::provenance::matches_normalized(actual, edge, &wanted)?
        {
            if position.is_some() {
                return Err(invalid(
                    "The section intersection is ambiguous. Reattach its dimension",
                ));
            }
            position = Some(candidate.position);
        }
    }
    position.ok_or_else(||invalid("A section intersection changed or disappeared. Reattach its dimension on the section view"))
}
/// Transverse intersections in the native edge's parameter order. A root shared
/// by two rational spans or by the seam of a circle is one physical vertex.
pub fn crossings(curve: &Curve3, frame: &Frame, tol: f64, work: &mut usize) -> Result<Vec<Point>> {
    let mut out = vec![];
    for span in crate::drawing_visibility::bezier_spans(curve)? {
        let max = span.weights.iter().copied().fold(0., f64::max);
        let c = span
            .control_points
            .iter()
            .zip(&span.weights)
            .map(|(p, w)| frame.distance(*p) * w / max)
            .collect::<Vec<_>>();
        if c.iter().all(|v| v.abs() <= tol * 1e-4) {
            continue;
        }
        for [a, b] in crate::drawing_visibility::roots(&c, 1e-12, tol * 1e-5, work)? {
            let p = drawing::curve_point(&span, (a + b) * 0.5)?;
            if frame.distance(p).abs() > tol {
                return Err(invalid(
                    "Section intersection could not be resolved to the model tolerance",
                ));
            }
            if !out.iter().any(|q| near(p, *q, tol)) {
                out.push(p)
            }
        }
    }
    Ok(out)
}
pub fn near(p: Point, q: Point, tol: f64) -> bool {
    p.into_iter()
        .zip(q)
        .map(|(a, b)| (a - b) * (a - b))
        .sum::<f64>()
        <= tol * tol
}
pub fn reference(id: &str, edge: CurveReference, crossing: usize, count: usize) -> PointReference {
    PointReference::SectionVertex {
        section: id.into(),
        source: Box::new(edge),
        crossing,
        count,
    }
}

/// Clip each 45-degree paper hatch directly to the rational cap loops. A scan
/// through an endpoint or a stationary contact is displaced a fraction of its
/// pitch so that parity counts only transverse crossings.
pub fn hatch(region: &Region, projection: &drawing::ViewProjection) -> Result<Vec<[[f64; 2]; 2]>> {
    let mut spans = vec![];
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    for curve in region.loops.iter().flatten() {
        for mut span in crate::drawing_visibility::bezier_spans(curve)? {
            for p in &mut span.control_points {
                let q = projection.paper_point(KEY, *p)?;
                *p = [q[0], q[1], 0.];
                lo = lo.min(q[1] - q[0]);
                hi = hi.max(q[1] - q[0]);
            }
            spans.push(span);
        }
    }
    let pitch = 3. * std::f64::consts::SQRT_2;
    let mut segments = vec![];
    let mut work = 0;
    let rows = ((hi - lo) / pitch).ceil() as usize;
    if rows > 8192 || rows.saturating_mul(spans.len()) > 4_000_000 {
        return Err(invalid(
            "Section hatch exceeds the drawing’s geometry budget",
        ));
    }
    let pitch = (hi - lo) / rows.max(1) as f64;
    for row in 0..rows {
        let mut resolved = false;
        for shift in [0.50, 0.501, 0.499, 0.51] {
            let height = lo + (row as f64 + shift) * pitch;
            let mut hits = vec![];
            let mut ambiguous = false;
            for span in &spans {
                let distance = |p: Point| p[1] - p[0] - height;
                let c = span
                    .control_points
                    .iter()
                    .zip(&span.weights)
                    .map(|(p, w)| distance(*p) * w)
                    .collect::<Vec<_>>();
                if c.iter().all(|x| *x > 1e-10) || c.iter().all(|x| *x < -1e-10) {
                    continue;
                }
                if c[0].abs() < 1e-9 || c.last().is_some_and(|v| v.abs() < 1e-9) {
                    ambiguous = true;
                    break;
                }
                for [a, b] in crate::drawing_visibility::roots(&c, 1e-12, 1e-12, &mut work)? {
                    if b - a > 1e-7 {
                        ambiguous = true;
                        break;
                    }
                    let t = (a + b) * 0.5;
                    let before = drawing::curve_point(span, (t - 1e-7).max(0.))?;
                    let after = drawing::curve_point(span, (t + 1e-7).min(1.))?;
                    if distance(before) * distance(after) >= 0. {
                        ambiguous = true;
                        break;
                    }
                    let p = drawing::curve_point(span, t)?;
                    hits.push(p[0]);
                }
                if ambiguous {
                    break;
                }
            }
            if ambiguous {
                continue;
            }
            hits.sort_by(f64::total_cmp);
            hits.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
            if hits.len() % 2 != 0 {
                continue;
            }
            for pair in hits.as_chunks::<2>().0 {
                segments.push([[pair[0], pair[0] + height], [pair[1], pair[1] + height]])
            }
            resolved = true;
            break;
        }
        if !resolved {
            return Err(invalid(
                "Section hatching could not resolve a boundary crossing. Adjust the cutting line",
            ));
        }
    }
    Ok(segments)
}
