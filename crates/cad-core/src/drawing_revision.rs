//! Source-owned drawing revision clouds and stable revision-table associations.
//! Cloud arcs are paper graphics, never inferred model geometry or change proof.
use crate::{
    CadError, Drawing, Id, Result,
    drawing::{Annotation, Primitive},
    drawing_tables::TableKind,
    sketch_edit::Curve2,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const CONTRACT_VERSION: u32 = 1;
pub const MAX_VERTICES: usize = 128;
pub const MAX_ARCS: usize = 2048;
fn version() -> u32 {
    CONTRACT_VERSION
}
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Shape {
    #[default]
    Rectangle,
    Ellipse,
    Polygon,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Vertex {
    pub id: Id,
    pub point_mm: [f64; 2],
}
impl Vertex {
    pub fn new(point_mm: [f64; 2]) -> Self {
        Self {
            id: crate::id(),
            point_mm,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct RevisionLink {
    pub table: Id,
    pub row: Id,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Cloud {
    #[serde(default = "version")]
    pub version: u32,
    pub shape: Shape,
    pub width_mm: f64,
    pub height_mm: f64,
    pub vertices: Vec<Vertex>,
    pub rotation_deg: f64,
    pub arc_pitch_mm: f64,
    /// Sagitta divided by chord. Bounded below a semicircle so every rational
    /// quadratic has positive finite weights and control coordinates.
    pub bulge: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<RevisionLink>,
    pub show_label: bool,
    pub label: String,
    pub label_offset_mm: [f64; 2],
    pub text_size_mm: f64,
}
impl Default for Cloud {
    fn default() -> Self {
        Self {
            version: version(),
            shape: Shape::Rectangle,
            width_mm: 30.,
            height_mm: 20.,
            vertices: [[-15., -10.], [15., -10.], [15., 10.], [-15., 10.]]
                .into_iter()
                .map(Vertex::new)
                .collect(),
            rotation_deg: 0.,
            arc_pitch_mm: 5.,
            bulge: 0.25,
            revision: None,
            show_label: true,
            label: String::new(),
            label_offset_mm: [22., 15.],
            text_size_mm: 3.,
        }
    }
}
fn invalid(message: &str) -> CadError {
    CadError::Invalid(format!("Drawing revision cloud: {message}"))
}
fn cross(a: [f64; 2], b: [f64; 2]) -> f64 {
    a[0] * b[1] - a[1] * b[0]
}
fn sub(a: [f64; 2], b: [f64; 2]) -> [f64; 2] {
    [a[0] - b[0], a[1] - b[1]]
}
fn area(points: &[[f64; 2]]) -> f64 {
    (0..points.len())
        .map(|i| cross(points[i], points[(i + 1) % points.len()]))
        .sum::<f64>()
        * 0.5
}
fn intersects(a: [f64; 2], b: [f64; 2], c: [f64; 2], d: [f64; 2]) -> bool {
    let ab = sub(b, a);
    let cd = sub(d, c);
    let ac = sub(c, a);
    let determinant = cross(ab, cd);
    let epsilon = 1e-10 * ab[0].hypot(ab[1]).max(1.) * cd[0].hypot(cd[1]).max(1.);
    if determinant.abs() <= epsilon {
        if cross(ac, ab).abs() > epsilon {
            return false;
        }
        let axis = usize::from(ab[1].abs() > ab[0].abs());
        return a[axis].min(b[axis]) <= c[axis].max(d[axis]) + 1e-9
            && c[axis].min(d[axis]) <= a[axis].max(b[axis]) + 1e-9;
    }
    let u = cross(ac, cd) / determinant;
    let v = cross(ac, ab) / determinant;
    (-1e-10..=1. + 1e-10).contains(&u) && (-1e-10..=1. + 1e-10).contains(&v)
}
impl Cloud {
    pub fn validate(&self) -> Result<()> {
        if self.version != CONTRACT_VERSION
            || !self.width_mm.is_finite()
            || !(0.5..=100_000.).contains(&self.width_mm)
            || !self.height_mm.is_finite()
            || !(0.5..=100_000.).contains(&self.height_mm)
            || !self.rotation_deg.is_finite()
            || self.rotation_deg.abs() > 1_000_000.
            || !self.arc_pitch_mm.is_finite()
            || !(0.5..=100.).contains(&self.arc_pitch_mm)
            || !self.bulge.is_finite()
            || !(0.1..=0.45).contains(&self.bulge)
            || !self.text_size_mm.is_finite()
            || !(0.5..=10.).contains(&self.text_size_mm)
            || self
                .label_offset_mm
                .iter()
                .any(|value| !value.is_finite() || value.abs() > 1_000_000.)
        {
            return Err(invalid(
                "Cloud size, rotation, scallops and paper label dimensions exceed finite bounds",
            ));
        }
        if self.label.len() > 256 || self.label.chars().any(char::is_control) {
            return Err(invalid(
                "Cloud label must be a single line of at most 256 bytes",
            ));
        }
        if self.vertices.len() < 3 || self.vertices.len() > MAX_VERTICES {
            return Err(invalid("A cloud retains 3–128 polygon vertices"));
        }
        let mut ids = BTreeSet::new();
        for vertex in &self.vertices {
            if vertex.id.is_empty()
                || vertex.id.len() > 256
                || vertex.id.chars().any(char::is_control)
                || !ids.insert(&vertex.id)
                || vertex
                    .point_mm
                    .iter()
                    .any(|value| !value.is_finite() || value.abs() > 100_000.)
            {
                return Err(invalid(
                    "Polygon vertices need stable unique IDs and finite bounded paper points",
                ));
            }
        }
        if self.shape == Shape::Polygon {
            let points = self
                .vertices
                .iter()
                .map(|vertex| vertex.point_mm)
                .collect::<Vec<_>>();
            if area(&points).abs() < 1e-8 {
                return Err(invalid("Cloud polygon must enclose a nonzero paper area"));
            }
            for i in 0..points.len() {
                let next = (i + 1) % points.len();
                let d = sub(points[next], points[i]);
                if d[0].hypot(d[1]) < 0.1 {
                    return Err(invalid("Cloud polygon sides must be at least 0.1 paper mm"));
                }
                let previous = sub(points[(i + points.len() - 1) % points.len()], points[i]);
                if cross(previous, d).abs()
                    <= 1e-10 * previous[0].hypot(previous[1]).max(1.) * d[0].hypot(d[1]).max(1.)
                    && previous[0] * d[0] + previous[1] * d[1] > 0.
                {
                    return Err(invalid(
                        "Adjacent cloud polygon sides cannot fold back over one another",
                    ));
                }
                for j in i + 1..points.len() {
                    let other = (j + 1) % points.len();
                    if j == next || other == i {
                        continue;
                    }
                    if intersects(points[i], points[next], points[j], points[other]) {
                        return Err(invalid(
                            "Cloud polygon sides cannot cross or touch nonadjacent sides",
                        ));
                    }
                }
            }
        }
        Ok(())
    }
    pub fn local_bounds(&self) -> [[f64; 2]; 2] {
        if self.shape != Shape::Polygon {
            return [
                [-self.width_mm * 0.5, -self.height_mm * 0.5],
                [self.width_mm * 0.5, self.height_mm * 0.5],
            ];
        }
        let mut bounds = [[f64::INFINITY; 2], [f64::NEG_INFINITY; 2]];
        for vertex in &self.vertices {
            for (axis, value) in vertex.point_mm.iter().enumerate() {
                bounds[0][axis] = bounds[0][axis].min(*value);
                bounds[1][axis] = bounds[1][axis].max(*value);
            }
        }
        bounds
    }
    pub fn paper_point(&self, local: [f64; 2], center: [f64; 2]) -> [f64; 2] {
        let (s, c) = self.rotation_deg.to_radians().sin_cos();
        [
            center[0] + local[0] * c - local[1] * s,
            center[1] + local[0] * s + local[1] * c,
        ]
    }
    pub fn local_delta(&self, paper: [f64; 2]) -> [f64; 2] {
        let (s, c) = self.rotation_deg.to_radians().sin_cos();
        [paper[0] * c + paper[1] * s, -paper[0] * s + paper[1] * c]
    }
    pub fn revision_label(&self, sheet: &Drawing) -> Result<String> {
        let Some(link) = &self.revision else {
            return Ok(self.label.clone());
        };
        let table = sheet
            .tables
            .iter()
            .find(|table| table.id == link.table)
            .ok_or_else(|| {
                invalid("The linked revision table is missing; reattach or remove the link")
            })?;
        let TableKind::Revisions { rows } = &table.kind else {
            return Err(invalid("The linked table is no longer a revision table"));
        };
        let row = rows.iter().find(|row| row.id == link.row).ok_or_else(|| {
            invalid("The linked revision row is missing; reattach or remove the link")
        })?;
        if row.revision.len() > 256 || row.revision.chars().any(char::is_control) {
            return Err(invalid(
                "The linked revision label must be a single line of at most 256 bytes",
            ));
        }
        Ok(if self.label.is_empty() {
            row.revision.clone()
        } else {
            format!("{} {}", self.label, row.revision)
        })
    }
    pub fn render(
        &self,
        sheet: &Drawing,
        annotation: &Annotation,
        target: [f64; 2],
    ) -> Result<Vec<Primitive>> {
        self.validate()?;
        let label = self.revision_label(sheet)?;
        if target
            .iter()
            .chain(annotation.offset_mm.iter())
            .any(|value| !value.is_finite())
        {
            return Err(invalid("Cloud placement requires finite paper coordinates"));
        }
        let center = [
            target[0] + annotation.offset_mm[0],
            target[1] + annotation.offset_mm[1],
        ];
        if center.iter().any(|value| !value.is_finite()) {
            return Err(invalid("Cloud placement exceeded finite paper bounds"));
        }
        let mut vertices = match self.shape {
            Shape::Rectangle => vec![
                [-self.width_mm * 0.5, -self.height_mm * 0.5],
                [self.width_mm * 0.5, -self.height_mm * 0.5],
                [self.width_mm * 0.5, self.height_mm * 0.5],
                [-self.width_mm * 0.5, self.height_mm * 0.5],
            ],
            Shape::Polygon => self.vertices.iter().map(|vertex| vertex.point_mm).collect(),
            Shape::Ellipse => {
                let a = self.width_mm * 0.5;
                let b = self.height_mm * 0.5;
                // An angular span has chord length at most max(a,b) * angle;
                // this bound keeps even highly eccentric outlines under pitch.
                let count = (std::f64::consts::TAU * a.max(b) / self.arc_pitch_mm)
                    .ceil()
                    .max(8.) as usize;
                if count > MAX_ARCS {
                    return Err(invalid(
                        "Cloud exceeds 2048 scallop arcs; increase the paper arc pitch",
                    ));
                }
                (0..count)
                    .map(|index| {
                        let angle = std::f64::consts::TAU * index as f64 / count as f64;
                        [a * angle.cos(), b * angle.sin()]
                    })
                    .collect()
            }
        };
        let orientation = area(&vertices).signum();
        for vertex in &mut vertices {
            *vertex = self.paper_point(*vertex, center);
        }
        let weight = (1. - 4. * self.bulge * self.bulge) / (1. + 4. * self.bulge * self.bulge);
        let mut out = Vec::new();
        for index in 0..vertices.len() {
            let a = vertices[index];
            let b = vertices[(index + 1) % vertices.len()];
            let d = sub(b, a);
            let length = d[0].hypot(d[1]);
            if !length.is_finite() || length < 1e-9 {
                return Err(invalid(
                    "Cloud placement lost a finite distinct paper side; move it within a usable paper range",
                ));
            }
            let count = if self.shape == Shape::Ellipse {
                1
            } else {
                (length / self.arc_pitch_mm).ceil().max(1.) as usize
            };
            if out.len().saturating_add(count) > MAX_ARCS {
                return Err(invalid(
                    "Cloud exceeds 2048 scallop arcs; increase the paper arc pitch",
                ));
            }
            let normal = [d[1] / length * orientation, -d[0] / length * orientation];
            let chord = length / count as f64;
            let control_height = 2. * self.bulge * chord / (1. - 4. * self.bulge * self.bulge);
            for step in 0..count {
                let first = [
                    a[0] + d[0] * step as f64 / count as f64,
                    a[1] + d[1] * step as f64 / count as f64,
                ];
                let last = [
                    a[0] + d[0] * (step + 1) as f64 / count as f64,
                    a[1] + d[1] * (step + 1) as f64 / count as f64,
                ];
                let control = [
                    (first[0] + last[0]) * 0.5 + normal[0] * control_height,
                    (first[1] + last[1]) * 0.5 + normal[1] * control_height,
                ];
                out.push(Primitive::Spline {
                    curve: Curve2 {
                        control_points: vec![first, control, last],
                        weights: vec![1., weight, 1.],
                        knots: vec![0., 0., 0., 1., 1., 1.],
                        degree: 2,
                    },
                    dashed: false,
                });
            }
        }
        if self.show_label && !label.is_empty() {
            let at = [
                center[0] + self.label_offset_mm[0],
                center[1] + self.label_offset_mm[1],
            ];
            let text_width = label.chars().count() as f64 * self.text_size_mm * 0.6;
            let width = (text_width / 0.75 + 4.).max(8.);
            let height = (self.text_size_mm * 3.).max(8.);
            out.push(Primitive::Polyline {
                points: vec![
                    [at[0], at[1] + height * 0.5],
                    [at[0] - width * 0.5, at[1] - height * 0.5],
                    [at[0] + width * 0.5, at[1] - height * 0.5],
                    [at[0], at[1] + height * 0.5],
                ],
                dashed: false,
            });
            out.push(Primitive::Text {
                at: [
                    at[0] - text_width * 0.5,
                    at[1] - height * 0.5 + self.text_size_mm * 0.35,
                ],
                size: self.text_size_mm,
                text: label,
            });
        }
        Ok(out)
    }
}
pub fn referenced(sheet: &Drawing, table: &str, row: Option<&str>) -> bool {
    sheet.entities.iter().any(|annotation|matches!(&annotation.kind,crate::drawing::AnnotationKind::RevisionCloud{cloud,..}if cloud.revision.as_ref().is_some_and(|link|link.table==table&&row.is_none_or(|row|link.row==row))))
}
