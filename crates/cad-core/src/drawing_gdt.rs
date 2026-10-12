//! Bounded drafting intent for datum targets and multi-row feature-control frames.
//! These definitions communicate requirements; they do not certify a model or
//! infer contact regions from rendered geometry.
use crate::{
    CadError, Dimension, Document, Drawing, Expr, Id, Result,
    drawing::{
        self, Annotation, AnnotationKind, GeometricCharacteristic, MaterialCondition, Primitive,
        Units,
    },
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
pub const CONTRACT_VERSION: u32 = 1;
fn version() -> u32 {
    CONTRACT_VERSION
}
fn precision() -> usize {
    3
}
fn text_size() -> f64 {
    2.5
}
fn callout_diameter() -> f64 {
    12.
}
fn hatch_spacing() -> f64 {
    1.5
}
fn cell_height() -> f64 {
    6.
}
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TargetShape {
    #[default]
    PointCross,
    PointDot,
    Line,
    Circle,
    Rectangle,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct DatumTarget {
    #[serde(default = "version")]
    pub version: u32,
    pub identifier: String,
    pub number: u16,
    #[serde(default)]
    pub shape: TargetShape,
    pub diameter: Expr,
    pub width: Expr,
    pub height: Expr,
    pub rotation: Expr,
    #[serde(default)]
    pub show_size: bool,
    #[serde(default)]
    pub units: Units,
    #[serde(default = "precision")]
    pub precision: usize,
    #[serde(default = "text_size")]
    pub text_size_mm: f64,
    #[serde(default = "callout_diameter")]
    pub callout_diameter_mm: f64,
    #[serde(default = "hatch_spacing")]
    pub hatch_spacing_mm: f64,
}
impl Default for DatumTarget {
    fn default() -> Self {
        Self {
            version: version(),
            identifier: "A".into(),
            number: 1,
            shape: TargetShape::PointCross,
            diameter: "10 mm".into(),
            width: "10 mm".into(),
            height: "5 mm".into(),
            rotation: "0 deg".into(),
            show_size: false,
            units: Units::Millimeters,
            precision: precision(),
            text_size_mm: text_size(),
            callout_diameter_mm: callout_diameter(),
            hatch_spacing_mm: hatch_spacing(),
        }
    }
}
fn identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 3
        && value
            .chars()
            .all(|character| character.is_ascii_alphabetic())
}
fn invalid(message: &str) -> CadError {
    CadError::Invalid(format!("Drawing GD&T: {message}"))
}
impl DatumTarget {
    pub fn validate(&self) -> Result<()> {
        if self.version != CONTRACT_VERSION
            || !identifier(&self.identifier)
            || self.number == 0
            || self.number > 999
            || self.precision > 8
        {
            return Err(invalid(
                "Datum targets need a supported version, 1–3 letter identifier, target number 1–999 and precision 0–8",
            ));
        }
        if !self.text_size_mm.is_finite()
            || !(0.5..=5.).contains(&self.text_size_mm)
            || !self.callout_diameter_mm.is_finite()
            || !(6.0..=100.).contains(&self.callout_diameter_mm)
            || !self.hatch_spacing_mm.is_finite()
            || !(0.25..=20.).contains(&self.hatch_spacing_mm)
        {
            return Err(invalid(
                "Datum target text/callout/hatch dimensions exceed their finite paper-mm bounds",
            ));
        }
        Ok(())
    }
}
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FrameMode {
    #[default]
    Composite,
    Stacked,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct DatumReference {
    pub identifier: String,
    #[serde(default)]
    pub material: MaterialCondition,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct FrameRow {
    pub id: Id,
    /// None inherits the frame characteristic. Composite frames require inheritance.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub characteristic: Option<GeometricCharacteristic>,
    pub value: Expr,
    #[serde(default)]
    pub diameter: bool,
    #[serde(default)]
    pub material: MaterialCondition,
    #[serde(default)]
    pub datums: Vec<DatumReference>,
    #[serde(default)]
    pub free_state: bool,
    #[serde(default)]
    pub tangent_plane: bool,
    #[serde(default)]
    pub projected_zone: bool,
    pub projected_height: Expr,
}
impl FrameRow {
    pub fn new(value: impl Into<Expr>) -> Self {
        Self {
            id: crate::id(),
            characteristic: None,
            value: value.into(),
            diameter: false,
            material: MaterialCondition::Regardless,
            datums: vec![],
            free_state: false,
            tangent_plane: false,
            projected_zone: false,
            projected_height: "5 mm".into(),
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ToleranceFrame {
    #[serde(default = "version")]
    pub version: u32,
    #[serde(default)]
    pub mode: FrameMode,
    #[serde(default)]
    pub characteristic: GeometricCharacteristic,
    pub rows: Vec<FrameRow>,
    #[serde(default)]
    pub units: Units,
    #[serde(default = "precision")]
    pub precision: usize,
    #[serde(default = "text_size")]
    pub text_size_mm: f64,
    #[serde(default = "cell_height")]
    pub cell_height_mm: f64,
}
impl Default for ToleranceFrame {
    fn default() -> Self {
        Self {
            version: version(),
            mode: FrameMode::Composite,
            characteristic: GeometricCharacteristic::Position,
            rows: vec![FrameRow::new("0.1 mm"), FrameRow::new("0.05 mm")],
            units: Units::Millimeters,
            precision: precision(),
            text_size_mm: text_size(),
            cell_height_mm: cell_height(),
        }
    }
}
impl ToleranceFrame {
    pub fn validate(&self) -> Result<()> {
        if self.version != CONTRACT_VERSION
            || self.rows.is_empty()
            || self.rows.len() > 8
            || self.precision > 8
            || !self.text_size_mm.is_finite()
            || !(0.5..=5.).contains(&self.text_size_mm)
            || !self.cell_height_mm.is_finite()
            || !(6.0..=30.).contains(&self.cell_height_mm)
            || self.cell_height_mm < self.text_size_mm * 1.5
        {
            return Err(invalid(
                "Tolerance frames need 1–8 rows, precision 0–8 and finite bounded text/cell dimensions",
            ));
        }
        if self.mode == FrameMode::Composite && self.rows.len() < 2 {
            return Err(invalid(
                "A composite tolerance frame needs at least two segments",
            ));
        }
        let mut ids = BTreeSet::new();
        for row in &self.rows {
            if row.id.trim().is_empty()
                || row.id.len() > 256
                || row.id.chars().any(char::is_control)
                || !ids.insert(&row.id)
                || row.datums.len() > 3
            {
                return Err(invalid(
                    "Frame rows need stable unique IDs and at most three datum references",
                ));
            }
            let mut datums = BTreeSet::new();
            for datum in &row.datums {
                if !identifier(&datum.identifier) || !datums.insert(datum.identifier.to_uppercase())
                {
                    return Err(invalid(
                        "Datum references need unique 1–3 letter identifiers within each frame row",
                    ));
                }
            }
            if self.mode == FrameMode::Composite
                && row
                    .characteristic
                    .as_ref()
                    .is_some_and(|characteristic| *characteristic != self.characteristic)
            {
                return Err(invalid(
                    "Composite rows share one geometric characteristic; choose Stacked for independent symbols",
                ));
            }
        }
        Ok(())
    }
}
fn length(doc: &Document, owner: &str, value: &Expr) -> Result<f64> {
    let value = crate::expression::value(doc, owner, value, Dimension::Length)?;
    if !value.is_finite() || value <= 0. {
        return Err(invalid(
            "Target dimensions and tolerance zones must be positive finite lengths",
        ));
    }
    Ok(value)
}
fn line(out: &mut Vec<Primitive>, a: [f64; 2], b: [f64; 2]) {
    out.push(Primitive::Polyline {
        points: vec![a, b],
        dashed: false,
    });
}
fn rectangle(out: &mut Vec<Primitive>, at: [f64; 2], size: [f64; 2]) {
    out.push(Primitive::Polyline {
        points: vec![
            at,
            [at[0] + size[0], at[1]],
            [at[0] + size[0], at[1] + size[1]],
            [at[0], at[1] + size[1]],
            at,
        ],
        dashed: false,
    });
}
fn text(out: &mut Vec<Primitive>, at: [f64; 2], size: f64, value: impl Into<String>) {
    out.push(Primitive::Text {
        at,
        size,
        text: value.into(),
    });
}
fn circled(out: &mut Vec<Primitive>, at: [f64; 2], size: f64, value: &str) {
    out.push(Primitive::Circle {
        center: at,
        radius: size * 0.65,
    });
    text(
        out,
        [at[0] - size * 0.35, at[1] - size * 0.35],
        size * 0.8,
        value,
    );
}
fn format(value: f64, units: Units, precision: usize) -> String {
    format!(
        "{:.*}",
        precision,
        if units == Units::Inches {
            value / 25.4
        } else {
            value
        }
    )
}
fn rotated(point: [f64; 2], center: [f64; 2], angle: f64) -> [f64; 2] {
    let (s, c) = angle.sin_cos();
    [
        center[0] + point[0] * c - point[1] * s,
        center[1] + point[0] * s + point[1] * c,
    ]
}
pub fn render_target(
    doc: &Document,
    sheet: &Drawing,
    annotation: &Annotation,
    target: [f64; 2],
    scale: f64,
    symbol: &DatumTarget,
    out: &mut Vec<Primitive>,
) -> Result<()> {
    symbol.validate()?;
    if !scale.is_finite() || scale <= 0. {
        return Err(invalid(
            "Datum target projection must have a positive finite scale",
        ));
    }
    let owner = &sheet.definition;
    let angle = crate::expression::value(doc, owner, &symbol.rotation, Dimension::Angle)?;
    if !angle.is_finite() || target.iter().any(|value| !value.is_finite()) {
        return Err(invalid(
            "Datum target coordinates and rotation must be finite",
        ));
    }
    let paper_length = |value: f64| -> Result<f64> {
        let value = value * scale;
        if !value.is_finite() || value > 1_000_000. {
            return Err(invalid(
                "Projected target dimensions exceed their finite paper bounds",
            ));
        }
        Ok(value)
    };
    let mut upper = String::new();
    match symbol.shape {
        TargetShape::PointCross => {
            for sign in [-1., 1.] {
                line(
                    out,
                    [target[0] - 1.4, target[1] - sign * 1.4],
                    [target[0] + 1.4, target[1] + sign * 1.4],
                );
            }
            if symbol.show_size {
                upper = format!(
                    "Ø{}",
                    format(
                        length(doc, owner, &symbol.diameter)?,
                        symbol.units,
                        symbol.precision
                    )
                );
            }
        }
        TargetShape::PointDot => {
            for index in -5..=5 {
                let y = f64::from(index) * 0.075;
                let x = (0.45_f64.powi(2) - y * y).sqrt();
                line(
                    out,
                    [target[0] - x, target[1] + y],
                    [target[0] + x, target[1] + y],
                );
            }
            out.push(Primitive::Circle {
                center: target,
                radius: 0.45,
            });
            if symbol.show_size {
                upper = format!(
                    "Ø{}",
                    format(
                        length(doc, owner, &symbol.diameter)?,
                        symbol.units,
                        symbol.precision
                    )
                );
            }
        }
        TargetShape::Line => {
            let width = length(doc, owner, &symbol.width)?;
            let half = paper_length(width)? * 0.5;
            line(
                out,
                rotated([-half, 0.], target, angle),
                rotated([half, 0.], target, angle),
            );
            upper = format(width, symbol.units, symbol.precision);
        }
        TargetShape::Circle => {
            let diameter = length(doc, owner, &symbol.diameter)?;
            let radius = paper_length(diameter)? * 0.5;
            out.push(Primitive::Circle {
                center: target,
                radius,
            });
            let count = (2. * radius / symbol.hatch_spacing_mm).ceil() as usize;
            if count > 512 {
                return Err(invalid(
                    "Target-area hatching exceeds its 512-line bound; increase paper hatch spacing",
                ));
            }
            for index in 1..count {
                let x = -radius + index as f64 * symbol.hatch_spacing_mm;
                if x.abs() < radius {
                    let y = (radius * radius - x * x).sqrt();
                    line(
                        out,
                        rotated([x, -y], target, angle + std::f64::consts::FRAC_PI_4),
                        rotated([x, y], target, angle + std::f64::consts::FRAC_PI_4),
                    );
                }
            }
            upper = format!("Ø{}", format(diameter, symbol.units, symbol.precision));
        }
        TargetShape::Rectangle => {
            let width = length(doc, owner, &symbol.width)?;
            let height = length(doc, owner, &symbol.height)?;
            let half = [paper_length(width)? * 0.5, paper_length(height)? * 0.5];
            let corners = [
                [-half[0], -half[1]],
                [half[0], -half[1]],
                [half[0], half[1]],
                [-half[0], half[1]],
                [-half[0], -half[1]],
            ];
            out.push(Primitive::Polyline {
                points: corners
                    .into_iter()
                    .map(|point| rotated(point, target, angle))
                    .collect(),
                dashed: false,
            });
            let extent = half[0] + half[1];
            let count = (2. * extent / symbol.hatch_spacing_mm).ceil() as usize;
            if count > 512 {
                return Err(invalid(
                    "Target-area hatching exceeds its 512-line bound; increase paper hatch spacing",
                ));
            }
            for index in 1..count {
                let intercept = -extent + index as f64 * symbol.hatch_spacing_mm;
                let x0 = (-half[0]).max(-half[1] - intercept);
                let x1 = half[0].min(half[1] - intercept);
                if x1 > x0 {
                    line(
                        out,
                        rotated([x0, x0 + intercept], target, angle),
                        rotated([x1, x1 + intercept], target, angle),
                    );
                }
            }
            upper = format!(
                "{}×{}",
                format(width, symbol.units, symbol.precision),
                format(height, symbol.units, symbol.precision)
            );
        }
    }
    if !upper.is_empty() && symbol.units == Units::Inches {
        upper.push_str(" in");
    }
    let lower = format!("{}{}", symbol.identifier.to_uppercase(), symbol.number);
    let radius = (symbol.callout_diameter_mm * 0.5)
        .max(symbol.text_size_mm * 2. + 1.)
        .max(
            upper.chars().count().max(lower.chars().count()) as f64 * symbol.text_size_mm * 0.32
                + 1.5,
        );
    let center = [
        target[0] + annotation.offset_mm[0],
        target[1] + annotation.offset_mm[1],
    ];
    let approach = annotation
        .leader_points_mm
        .last()
        .map(|point| [target[0] + point[0], target[1] + point[1]])
        .unwrap_or(target);
    let delta = [approach[0] - center[0], approach[1] - center[1]];
    let distance = delta[0].hypot(delta[1]);
    if distance <= radius + 1e-8 {
        return Err(invalid(
            "Move the datum-target callout away from its attachment or last leader elbow",
        ));
    }
    let rim = [
        center[0] + delta[0] * radius / distance,
        center[1] + delta[1] * radius / distance,
    ];
    drawing::leader_polyline(out, annotation, target, rim, false);
    out.push(Primitive::Circle { center, radius });
    line(
        out,
        [center[0] - radius, center[1]],
        [center[0] + radius, center[1]],
    );
    if !upper.is_empty() {
        text(
            out,
            [
                center[0] - upper.chars().count() as f64 * symbol.text_size_mm * 0.28,
                center[1] + symbol.text_size_mm * 0.5,
            ],
            symbol.text_size_mm,
            &upper,
        );
    }
    text(
        out,
        [
            center[0] - lower.chars().count() as f64 * symbol.text_size_mm * 0.28,
            center[1] - symbol.text_size_mm * 1.4,
        ],
        symbol.text_size_mm,
        lower,
    );
    Ok(())
}
pub fn render_frame(
    doc: &Document,
    sheet: &Drawing,
    annotation: &Annotation,
    target: [f64; 2],
    frame: &ToleranceFrame,
    out: &mut Vec<Primitive>,
) -> Result<()> {
    frame.validate()?;
    let declared = sheet
        .entities
        .iter()
        .filter_map(|annotation| match &annotation.kind {
            AnnotationKind::Datum { identifier, .. } => Some(identifier.as_str()),
            AnnotationKind::DatumTarget { symbol, .. } => Some(symbol.identifier.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>();
    let mut rows = Vec::new();
    for row in &frame.rows {
        if row.datums.iter().any(|datum| {
            !declared
                .iter()
                .any(|identifier| identifier.eq_ignore_ascii_case(&datum.identifier))
        }) {
            return Err(invalid(
                "A tolerance frame references an undeclared datum identifier",
            ));
        }
        let value = length(doc, &sheet.definition, &row.value)?;
        let mut zone = format!(
            "{}{}",
            if row.diameter { "Ø" } else { "" },
            format(value, frame.units, frame.precision)
        );
        if frame.units == Units::Inches {
            zone.push_str(" in");
        }
        let projected = if row.projected_zone {
            Some(format(
                length(doc, &sheet.definition, &row.projected_height)?,
                frame.units,
                frame.precision,
            ))
        } else {
            None
        };
        let modifiers = [
            (
                row.material != MaterialCondition::Regardless,
                if row.material == MaterialCondition::Maximum {
                    "M"
                } else {
                    "L"
                },
            ),
            (row.free_state, "F"),
            (row.tangent_plane, "T"),
            (row.projected_zone, "P"),
        ]
        .into_iter()
        .filter_map(|(enabled, label)| enabled.then_some(label))
        .collect::<Vec<_>>();
        rows.push((zone, modifiers, projected));
    }
    let height = frame.cell_height_mm;
    let total_height = height * frame.rows.len() as f64;
    let char_width = 6.;
    let mut datum_widths = [0_f64; 3];
    for row in &frame.rows {
        for (index, datum) in row.datums.iter().enumerate() {
            let width = datum.identifier.len() as f64 * frame.text_size_mm * 0.6
                + 2.
                + if datum.material == MaterialCondition::Regardless {
                    0.
                } else {
                    frame.text_size_mm * 1.5
                };
            datum_widths[index] = datum_widths[index].max(width);
        }
    }
    let zone_width = rows
        .iter()
        .map(|(zone, modifiers, projected)| {
            zone.chars().count() as f64 * frame.text_size_mm * 0.6
                + 2.
                + modifiers.len() as f64 * frame.text_size_mm * 1.5
                + projected
                    .as_ref()
                    .map(|value| value.chars().count() as f64 * frame.text_size_mm * 0.6 + 1.)
                    .unwrap_or_default()
        })
        .fold(8., f64::max);
    let at = [
        target[0] + annotation.offset_mm[0],
        target[1] + annotation.offset_mm[1],
    ];
    drawing::leader_polyline(
        out,
        annotation,
        target,
        [at[0], at[1] + total_height * 0.5],
        true,
    );
    if frame.mode == FrameMode::Composite {
        rectangle(out, at, [char_width, total_height]);
        drawing::characteristic(
            out,
            [at[0], at[1] + (total_height - 6.) * 0.5],
            &frame.characteristic,
        );
    }
    for (index, (row, (zone, modifiers, projected))) in frame.rows.iter().zip(rows).enumerate() {
        let y = at[1] + height * (frame.rows.len() - 1 - index) as f64;
        if frame.mode == FrameMode::Stacked {
            rectangle(out, [at[0], y], [char_width, height]);
            drawing::characteristic(
                out,
                [at[0], y + (height - 6.) * 0.5],
                row.characteristic.as_ref().unwrap_or(&frame.characteristic),
            );
        }
        rectangle(out, [at[0] + char_width, y], [zone_width, height]);
        text(
            out,
            [
                at[0] + char_width + 1.,
                y + (height - frame.text_size_mm) * 0.5,
            ],
            frame.text_size_mm,
            &zone,
        );
        let mut modifier_x = at[0]
            + char_width
            + 1.
            + zone.chars().count() as f64 * frame.text_size_mm * 0.6
            + frame.text_size_mm;
        for modifier in modifiers {
            circled(
                out,
                [modifier_x, y + height * 0.5],
                frame.text_size_mm,
                modifier,
            );
            modifier_x += frame.text_size_mm * 1.5;
        }
        if let Some(projected) = projected {
            text(
                out,
                [
                    modifier_x - frame.text_size_mm * 0.3,
                    y + (height - frame.text_size_mm) * 0.5,
                ],
                frame.text_size_mm,
                projected,
            );
        }
        let mut x = at[0] + char_width + zone_width;
        for (index, datum) in row.datums.iter().enumerate() {
            let width = datum_widths[index];
            rectangle(out, [x, y], [width, height]);
            text(
                out,
                [x + 1., y + (height - frame.text_size_mm) * 0.5],
                frame.text_size_mm,
                datum.identifier.to_uppercase(),
            );
            if datum.material != MaterialCondition::Regardless {
                circled(
                    out,
                    [x + width - frame.text_size_mm * 0.8, y + height * 0.5],
                    frame.text_size_mm,
                    if datum.material == MaterialCondition::Maximum {
                        "M"
                    } else {
                        "L"
                    },
                );
            }
            x += width;
        }
    }
    Ok(())
}
