//! Associative production dimension groups, independent of paper widgets.
//! A group owns its datum, ordered targets, and label offsets. Native point
//! references are resolved by the drawing adapter; display samples are not input.
use crate::{
    CadError, Document, Drawing, DrawingDimension, Id, Result,
    drawing::{
        self, Anchor, DimensionKind, DimensionOptions, Primitive, Tolerance, Units, ViewProjection,
    },
    protocol::Evaluation,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const CONTRACT_VERSION: u32 = 1;
pub const MAX_TARGETS: usize = 128;
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    #[default]
    Ordinate,
    Baseline,
    Chain,
}
impl Mode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Ordinate => "Ordinate",
            Self::Baseline => "Baseline",
            Self::Chain => "Chain",
        }
    }
}
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Axis {
    #[default]
    X,
    Y,
}
impl Axis {
    pub fn index(self) -> usize {
        usize::from(self == Self::Y)
    }
}
fn version() -> u32 {
    CONTRACT_VERSION
}
fn spacing() -> f64 {
    7.
}
fn precision() -> usize {
    3
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Target {
    pub id: Id,
    pub anchor: Anchor,
    /// F64 paper-mm displacement of this label and its routing only.
    #[serde(default)]
    pub label_offset_mm: [f64; 2],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tolerance: Option<Tolerance>,
}
impl Target {
    pub fn new(anchor: Anchor) -> Self {
        Self {
            id: crate::id(),
            anchor,
            label_offset_mm: [0.; 2],
            tolerance: None,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Group {
    #[serde(default = "version")]
    pub version: u32,
    #[serde(default)]
    pub mode: Mode,
    #[serde(default)]
    pub axis: Axis,
    pub datum: Anchor,
    pub targets: Vec<Target>,
    #[serde(default = "spacing")]
    pub spacing_mm: f64,
    #[serde(default)]
    pub units: Units,
    #[serde(default = "precision")]
    pub precision: usize,
    #[serde(default)]
    pub tolerance: Tolerance,
    #[serde(default)]
    pub reference: bool,
    #[serde(default)]
    pub prefix: String,
    #[serde(default)]
    pub suffix: String,
}
impl Group {
    pub fn new(datum: Anchor, target: Anchor) -> Self {
        Self {
            version: CONTRACT_VERSION,
            mode: Mode::Ordinate,
            axis: Axis::X,
            datum,
            targets: vec![Target::new(target)],
            spacing_mm: spacing(),
            units: Units::Millimeters,
            precision: precision(),
            tolerance: Tolerance::None,
            reference: false,
            prefix: String::new(),
            suffix: String::new(),
        }
    }
    pub fn validate(&self) -> Result<()> {
        if self.version != CONTRACT_VERSION {
            return Err(invalid("Unsupported drawing dimension-group version"));
        }
        if self.targets.is_empty()
            || self.targets.len() > MAX_TARGETS
            || !self.spacing_mm.is_finite()
            || !(0.5..=1000.).contains(&self.spacing_mm)
            || self.precision > 8
        {
            return Err(invalid(
                "Dimension groups need 1–128 targets, 0.5–1000 mm paper spacing, and precision 0–8",
            ));
        }
        let mut ids = BTreeSet::new();
        for target in &self.targets {
            if target.id.trim().is_empty()
                || target.id.len() > 256
                || target.id.chars().any(char::is_control)
                || !ids.insert(&target.id)
                || target
                    .label_offset_mm
                    .iter()
                    .any(|value| !value.is_finite())
            {
                return Err(invalid(
                    "Dimension group targets need stable unique identities and finite label offsets",
                ));
            }
        }
        if self
            .datum
            .point
            .iter()
            .chain(
                self.targets
                    .iter()
                    .flat_map(|target| target.anchor.point.iter()),
            )
            .any(|value| !value.is_finite())
        {
            return Err(invalid("Dimension group anchors must be finite"));
        }
        if [&self.prefix, &self.suffix]
            .iter()
            .any(|value| value.len() > 1024 || value.chars().any(char::is_control))
        {
            return Err(invalid(
                "Dimension group prefixes and suffixes exceed their printable text bounds",
            ));
        }
        Ok(())
    }
    fn options(&self) -> DimensionOptions {
        DimensionOptions {
            kind: if self.axis == Axis::X {
                DimensionKind::Horizontal
            } else {
                DimensionKind::Vertical
            },
            units: self.units,
            precision: self.precision,
            tolerance: self.tolerance.clone(),
            reference: self.reference,
            prefix: self.prefix.clone(),
            suffix: self.suffix.clone(),
            ..Default::default()
        }
    }
}
fn invalid(message: &str) -> CadError {
    CadError::Invalid(format!("Drawing dimension group: {message}"))
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TargetLayout {
    pub target: Id,
    pub anchor_mm: [f64; 2],
    pub label_mm: [f64; 2],
    pub dimension_line_mm: f64,
    pub value_mm: f64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Layout {
    pub datum_mm: [f64; 2],
    pub datum_label_mm: [f64; 2],
    pub targets: Vec<TargetLayout>,
}
pub fn layout(
    doc: &Document,
    evaluation: &Evaluation,
    sheet: &Drawing,
    view: &str,
    projection: &ViewProjection,
    group: &Group,
    offset: [f64; 2],
) -> Result<Layout> {
    group.validate()?;
    // Orthographic X/Y has physical lengths. Isometric projection cannot be
    // called a manufacturing coordinate dimension without a different intent.
    if !matches!(view, "front" | "top" | "right") {
        return Err(CadError::Unsupported(
            "Drawing dimension groups require a standard orthographic view".into(),
        ));
    }
    if !projection.scale.is_finite()
        || projection.scale <= 0.
        || offset.iter().any(|value| !value.is_finite())
    {
        return Err(invalid(
            "The current group projection and offset must be finite",
        ));
    }
    let axis = group.axis.index();
    let normal = 1 - axis;
    let datum_model = drawing::point(
        doc,
        evaluation,
        sheet,
        group.datum.point,
        &group.datum.reference,
    )?;
    let datum_mm = projection.paper_point(view, datum_model)?;
    let datum_coordinate = drawing::project(view, datum_model)?[axis];
    let datum_label_mm =
        std::array::from_fn(|coordinate| datum_mm[coordinate] + offset[coordinate]);
    let mut previous = datum_coordinate;
    let mut previous_mm = datum_mm;
    let mut targets = Vec::new();
    for (index, target) in group.targets.iter().enumerate() {
        let model = drawing::point(
            doc,
            evaluation,
            sheet,
            target.anchor.point,
            &target.anchor.reference,
        )?;
        let coordinate = drawing::project(view, model)?[axis];
        let anchor_mm = projection.paper_point(view, model)?;
        let first = if group.mode == Mode::Chain {
            previous
        } else {
            datum_coordinate
        };
        let value = coordinate - first;
        if !value.is_finite() || (group.mode != Mode::Ordinate && value.abs() < 1e-9) {
            return Err(invalid(
                "A group target has zero or nonfinite projected distance from its datum/preceding target",
            ));
        }
        let first_mm = if group.mode == Mode::Chain {
            previous_mm
        } else {
            datum_mm
        };
        let dimension_line_mm = datum_label_mm[normal]
            + target.label_offset_mm[normal]
            + if group.mode == Mode::Baseline {
                group.spacing_mm * index as f64
            } else {
                0.
            };
        let mut label_mm = anchor_mm;
        label_mm[axis] = if group.mode == Mode::Ordinate {
            anchor_mm[axis]
        } else {
            (first_mm[axis] + anchor_mm[axis]) * 0.5
        };
        label_mm[axis] += offset[axis] + target.label_offset_mm[axis];
        label_mm[normal] = dimension_line_mm;
        label_mm[0] += 1.;
        label_mm[1] += 2.;
        targets.push(TargetLayout {
            target: target.id.clone(),
            anchor_mm,
            label_mm,
            dimension_line_mm,
            value_mm: if group.mode == Mode::Ordinate {
                value
            } else {
                value.abs()
            },
        });
        previous = coordinate;
        previous_mm = anchor_mm;
    }
    Ok(Layout {
        datum_mm,
        datum_label_mm,
        targets,
    })
}
fn line(out: &mut Vec<Primitive>, a: [f64; 2], b: [f64; 2]) {
    out.push(Primitive::Polyline {
        points: vec![a, b],
        dashed: false,
    });
}
fn text(out: &mut Vec<Primitive>, at: [f64; 2], value: String) {
    out.push(Primitive::Text {
        at,
        size: 3.,
        text: value,
    });
}
fn arrow(out: &mut Vec<Primitive>, tip: [f64; 2], other: [f64; 2]) {
    let delta = [other[0] - tip[0], other[1] - tip[1]];
    let length = delta[0].hypot(delta[1]);
    if length < 1e-9 {
        return;
    }
    let direction = [delta[0] / length, delta[1] / length];
    for side in [-1., 1.] {
        line(
            out,
            tip,
            [
                tip[0] + direction[0] * 2.5 - direction[1] * side * 0.8,
                tip[1] + direction[1] * 2.5 + direction[0] * side * 0.8,
            ],
        );
    }
}
fn dimension(group: &Group, view: &str, id: &str) -> DrawingDimension {
    DrawingDimension {
        id: id.into(),
        view: view.into(),
        first: [0.; 3],
        second: [0.; 3],
        offset_mm: 0.,
        parameter: None,
        tolerance: String::new(),
        options: group.options(),
    }
}
fn label(
    doc: &Document,
    sheet: &Drawing,
    group: &Group,
    view: &str,
    id: &str,
    value: f64,
) -> Result<String> {
    let mut dimension = dimension(group, view, id);
    if id == "datum" {
        dimension.options.tolerance = Tolerance::None;
        dimension.options.reference = false;
        dimension.options.prefix.clear();
        dimension.options.suffix.clear();
    } else if let Some(tolerance) = group
        .targets
        .iter()
        .find(|target| target.id == id)
        .and_then(|target| target.tolerance.as_ref())
    {
        dimension.options.tolerance = tolerance.clone();
    }
    drawing::label(doc, sheet, &dimension, value, false)
}
fn label_box(out: &mut Vec<Primitive>, at: [f64; 2], value: &str) {
    out.push(Primitive::Polyline {
        points: vec![
            [at[0] - 0.5, at[1] - 0.7],
            [
                at[0] + value.chars().count() as f64 * 1.9 + 0.5,
                at[1] - 0.7,
            ],
            [
                at[0] + value.chars().count() as f64 * 1.9 + 0.5,
                at[1] + 3.3,
            ],
            [at[0] - 0.5, at[1] + 3.3],
            [at[0] - 0.5, at[1] - 0.7],
        ],
        dashed: false,
    });
}
pub fn render(
    doc: &Document,
    evaluation: &Evaluation,
    sheet: &Drawing,
    view: &str,
    projection: &ViewProjection,
    group: &Group,
    offset: [f64; 2],
) -> Result<(Layout, Vec<Primitive>)> {
    let mut primitives = vec![];
    let out = &mut primitives;
    let layout = layout(doc, evaluation, sheet, view, projection, group, offset)?;
    let axis = group.axis.index();
    let normal = 1 - axis;
    if group.mode == Mode::Ordinate {
        let mut datum = layout.datum_label_mm;
        datum[axis] = layout.datum_mm[axis];
        line(out, layout.datum_mm, datum);
        out.push(Primitive::Circle {
            center: datum,
            radius: 0.65,
        });
        text(
            out,
            [datum[0] + 1., datum[1] + 2.],
            label(doc, sheet, group, view, "datum", 0.)?,
        );
    }
    let mut previous = layout.datum_mm;
    for target in &layout.targets {
        let at = target.label_mm;
        if group.mode == Mode::Ordinate {
            let mut elbow = target.anchor_mm;
            elbow[normal] = target.dimension_line_mm;
            line(out, target.anchor_mm, elbow);
            line(out, elbow, [at[0] - 1., at[1] - 2.]);
        } else {
            let first = if group.mode == Mode::Chain {
                previous
            } else {
                layout.datum_mm
            };
            let mut a = first;
            let mut b = target.anchor_mm;
            a[normal] = target.dimension_line_mm;
            b[normal] = target.dimension_line_mm;
            line(out, first, a);
            line(out, target.anchor_mm, b);
            line(out, a, b);
            arrow(out, a, b);
            arrow(out, b, a);
        }
        let value = label(doc, sheet, group, view, &target.target, target.value_mm)?;
        let tolerance = group
            .targets
            .iter()
            .find(|source| source.id == target.target)
            .and_then(|source| source.tolerance.as_ref())
            .unwrap_or(&group.tolerance);
        if *tolerance == Tolerance::Basic {
            label_box(out, at, &value);
        }
        text(out, at, value);
        previous = target.anchor_mm;
    }
    Ok((layout, primitives))
}
