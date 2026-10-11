//! Saved manufacturing intent, independent of widgets, fonts and kernel objects.
//! These are explicitly bounded drafting conventions, not standards certification.
use super::{Primitive, arrow, invalid, line, text};
use crate::{Dimension, Document, Expr, Result};
use serde::{Deserialize, Serialize};

pub const SYMBOL_CONTRACT_VERSION: u32 = 1;
fn version() -> u32 {
    SYMBOL_CONTRACT_VERSION
}
fn text_size() -> f64 {
    2.5
}
fn symbol_size() -> f64 {
    6.
}
fn reference_length() -> f64 {
    48.
}
fn precision() -> usize {
    2
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SurfaceConvention {
    #[default]
    Iso1302,
    AsmeY14_36,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MaterialRemoval {
    Basic,
    #[default]
    Required,
    Prohibited,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RoughnessParameter {
    #[default]
    Ra,
    Rz,
    Rq,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RoughnessUnits {
    #[default]
    Micrometers,
    Microinches,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SurfaceLay {
    #[default]
    None,
    Parallel,
    Perpendicular,
    Crossed,
    Multidirectional,
    Circular,
    Radial,
    Particulate,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SurfaceFinish {
    #[serde(default = "version")]
    pub version: u32,
    #[serde(default)]
    pub convention: SurfaceConvention,
    #[serde(default)]
    pub removal: MaterialRemoval,
    #[serde(default)]
    pub parameter: RoughnessParameter,
    /// A dimensional expression in model length units, never a unitless label.
    pub maximum: Expr,
    #[serde(default)]
    pub minimum: Option<Expr>,
    #[serde(default)]
    pub roughness_units: RoughnessUnits,
    #[serde(default = "precision")]
    pub precision: usize,
    #[serde(default)]
    pub sampling_length: Option<Expr>,
    #[serde(default)]
    pub allowance: Option<Expr>,
    #[serde(default)]
    pub process: String,
    #[serde(default)]
    pub lay: SurfaceLay,
    #[serde(default = "text_size")]
    pub text_size_mm: f64,
    #[serde(default = "symbol_size")]
    pub symbol_height_mm: f64,
}
impl Default for SurfaceFinish {
    fn default() -> Self {
        Self {
            version: version(),
            convention: SurfaceConvention::default(),
            removal: MaterialRemoval::default(),
            parameter: RoughnessParameter::default(),
            maximum: "3.2 um".into(),
            minimum: None,
            roughness_units: RoughnessUnits::default(),
            precision: precision(),
            sampling_length: None,
            allowance: None,
            process: String::new(),
            lay: SurfaceLay::default(),
            text_size_mm: text_size(),
            symbol_height_mm: symbol_size(),
        }
    }
}
fn printable(value: &str) -> bool {
    value.len() <= 128 && !value.chars().any(char::is_control)
}
fn valid_expression(value: &Expr) -> bool {
    !value.0.trim().is_empty() && value.0.len() <= 1024 && !value.0.chars().any(char::is_control)
}
fn style_valid(text: f64, symbol: f64, precision: usize) -> bool {
    text.is_finite()
        && (1. ..=10.).contains(&text)
        && symbol.is_finite()
        && (3. ..=20.).contains(&symbol)
        && symbol >= text * 1.5
        && precision <= 6
}
fn length(doc: &Document, definition: &str, value: &Expr, label: &str, zero: bool) -> Result<f64> {
    let result = crate::expression::value(doc, definition, value, Dimension::Length)?;
    if !result.is_finite() || result < 0. || (!zero && result == 0.) {
        return Err(invalid(format!(
            "{label} must be a finite {} length",
            if zero { "nonnegative" } else { "positive" }
        )));
    }
    Ok(result)
}
fn optional_length(
    doc: &Document,
    definition: &str,
    value: &Option<Expr>,
    label: &str,
    zero: bool,
) -> Result<Option<f64>> {
    value
        .as_ref()
        .map(|v| length(doc, definition, v, label, zero))
        .transpose()
}
fn number(value: f64, precision: usize) -> Result<String> {
    if !value.is_finite() {
        return Err(invalid(
            "A manufacturing symbol quantity overflowed its printed unit conversion",
        ));
    }
    Ok(format!("{value:.precision$}"))
}
fn circle(out: &mut Vec<Primitive>, center: [f64; 2], radius: f64) {
    out.push(Primitive::Circle { center, radius });
}
fn leader(out: &mut Vec<Primitive>, target: [f64; 2], at: [f64; 2]) -> Result<()> {
    if (target[0] - at[0]).hypot(target[1] - at[1]) < 3. {
        return Err(invalid(
            "Move the manufacturing symbol at least 3 paper mm from its leader target",
        ));
    }
    line(out, target, at);
    arrow(out, target, at);
    Ok(())
}
impl SurfaceFinish {
    pub fn validate(&self) -> Result<()> {
        if self.version != SYMBOL_CONTRACT_VERSION
            || !style_valid(self.text_size_mm, self.symbol_height_mm, self.precision)
            || !printable(&self.process)
            || !valid_expression(&self.maximum)
            || [&self.minimum, &self.sampling_length, &self.allowance]
                .into_iter()
                .flatten()
                .any(|v| !valid_expression(v))
        {
            return Err(invalid(
                "Surface finish version 1 requires valid expressions, up to 128 printable process characters, precision 0..6, 1–10 mm text and 3–20 mm symbols",
            ));
        }
        if self.removal == MaterialRemoval::Prohibited && self.allowance.is_some() {
            return Err(invalid(
                "A material-removal-prohibited surface cannot specify a removal allowance",
            ));
        }
        Ok(())
    }
    pub(super) fn render(
        &self,
        doc: &Document,
        definition: &str,
        target: [f64; 2],
        offset: [f64; 2],
        out: &mut Vec<Primitive>,
    ) -> Result<()> {
        self.validate()?;
        let maximum = length(doc, definition, &self.maximum, "Maximum roughness", false)?;
        let minimum = optional_length(doc, definition, &self.minimum, "Minimum roughness", true)?;
        if minimum.is_some_and(|v| v > maximum) {
            return Err(invalid("Minimum roughness exceeds maximum roughness"));
        }
        let sampling = optional_length(
            doc,
            definition,
            &self.sampling_length,
            "Sampling length",
            false,
        )?;
        let allowance =
            optional_length(doc, definition, &self.allowance, "Removal allowance", true)?;
        let at = [target[0] + offset[0], target[1] + offset[1]];
        leader(out, target, at)?;
        let h = self.symbol_height_mm;
        let s = self.text_size_mm;
        let a = [at[0] - h * 0.3, at[1] + h * 0.5];
        let b = [at[0] + h * 0.6, at[1] + h];
        line(out, a, at);
        line(out, at, b);
        if self.removal == MaterialRemoval::Required {
            line(out, a, [at[0] + h * 0.3, a[1]]);
        }
        if self.removal == MaterialRemoval::Prohibited {
            circle(out, [at[0], at[1] + h * 0.32], h * 0.14);
        }
        let factor = if self.roughness_units == RoughnessUnits::Micrometers {
            1000.
        } else {
            1. / 0.0000254
        };
        let unit = if self.roughness_units == RoughnessUnits::Micrometers {
            "um"
        } else {
            "uin"
        };
        let parameter = match self.parameter {
            RoughnessParameter::Ra => "Ra",
            RoughnessParameter::Rz => "Rz",
            RoughnessParameter::Rq => "Rq",
        };
        // Keep the saved convention explicit: ISO's parameter goes above the
        // extension, while the ASME roughness value goes inside its upper field.
        let x = at[0] + h * 0.6;
        let roughness = if let Some(minimum) = minimum {
            format!(
                "{parameter} {}-{} {unit}",
                number(minimum * factor, self.precision)?,
                number(maximum * factor, self.precision)?
            )
        } else {
            format!(
                "{parameter} {} {unit}",
                number(maximum * factor, self.precision)?
            )
        };
        let width =
            (roughness.chars().count().max(self.process.chars().count()) as f64 * s * 0.6).max(h);
        line(out, b, [x + width, b[1]]);
        let roughness_y = if self.convention == SurfaceConvention::Iso1302 {
            b[1] + s * 0.5
        } else {
            at[1] + h * 0.55
        };
        text(out, [x + s * 0.3, roughness_y], s, roughness);
        if !self.process.is_empty() {
            text(out, [x + s * 0.3, b[1] + s * 2.], s, &self.process);
        }
        if let Some(sampling) = sampling {
            text(
                out,
                [x + s * 0.3, at[1] + s * 0.1],
                s,
                format!("L {} mm", number(sampling, self.precision)?),
            );
        }
        if let Some(allowance) = allowance {
            let value = format!("{} mm", number(allowance, self.precision)?);
            let width = value.chars().count() as f64 * s * 0.6;
            text(
                out,
                [at[0] - h * 0.3 - width - s * 0.5, at[1] + h * 0.2],
                s,
                value,
            );
        }
        lay(out, &self.lay, [x + width + s, at[1] + h * 0.5], s);
        Ok(())
    }
}
fn lay(out: &mut Vec<Primitive>, kind: &SurfaceLay, at: [f64; 2], size: f64) {
    let x = at[0];
    let y = at[1];
    let h = size;
    match kind {
        SurfaceLay::None => {}
        SurfaceLay::Parallel => {
            line(out, [x, y], [x + h, y]);
            line(out, [x, y + h * 0.45], [x + h, y + h * 0.45]);
        }
        SurfaceLay::Perpendicular => {
            line(out, [x, y], [x + h, y]);
            line(out, [x + h * 0.5, y], [x + h * 0.5, y + h]);
        }
        SurfaceLay::Crossed => {
            line(out, [x, y], [x + h, y + h]);
            line(out, [x, y + h], [x + h, y]);
        }
        SurfaceLay::Multidirectional => {
            out.push(Primitive::Polyline {
                points: vec![
                    [x, y],
                    [x, y + h],
                    [x + h * 0.5, y + h * 0.4],
                    [x + h, y + h],
                    [x + h, y],
                ],
                dashed: false,
            });
        }
        SurfaceLay::Circular => {
            arc(
                out,
                [x + h * 0.5, y + h * 0.5],
                h * 0.5,
                0.6,
                std::f64::consts::TAU - 0.6,
            );
        }
        SurfaceLay::Radial => {
            line(out, [x, y], [x, y + h]);
            arc(
                out,
                [x, y + h * 0.7],
                h * 0.3,
                -std::f64::consts::FRAC_PI_2,
                std::f64::consts::FRAC_PI_2,
            );
            line(out, [x, y + h * 0.4], [x + h * 0.5, y]);
        }
        SurfaceLay::Particulate => {
            line(out, [x, y], [x, y + h]);
            arc(
                out,
                [x, y + h * 0.7],
                h * 0.3,
                -std::f64::consts::FRAC_PI_2,
                std::f64::consts::FRAC_PI_2,
            );
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WeldConvention {
    #[default]
    AwsA2_4,
    Iso2553SystemA,
    Iso2553SystemB,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WeldJointKind {
    #[default]
    Fillet,
    SquareGroove,
    VGroove,
    BevelGroove,
    PlugSlot,
    Spot,
    Seam,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FilletSize {
    #[default]
    Leg,
    Throat,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WeldContour {
    #[default]
    None,
    Flush,
    Convex,
    Concave,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct WeldJoint {
    #[serde(default)]
    pub joint: WeldJointKind,
    #[serde(default)]
    pub fillet_size: FilletSize,
    #[serde(default)]
    pub size: Option<Expr>,
    #[serde(default)]
    pub length: Option<Expr>,
    /// Center-to-center spacing of intermittent welds; never the clear gap.
    #[serde(default)]
    pub pitch: Option<Expr>,
    #[serde(default)]
    pub count: Option<u32>,
    #[serde(default)]
    pub contour: WeldContour,
    /// AWS finishing-method letter. ISO process information belongs in the tail.
    #[serde(default)]
    pub finish: String,
}
impl Default for WeldJoint {
    fn default() -> Self {
        Self {
            joint: WeldJointKind::default(),
            fillet_size: FilletSize::default(),
            size: Some("3 mm".into()),
            length: None,
            pitch: None,
            count: None,
            contour: WeldContour::default(),
            finish: String::new(),
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct WeldSymbol {
    #[serde(default = "version")]
    pub version: u32,
    #[serde(default)]
    pub convention: WeldConvention,
    #[serde(default)]
    pub arrow_side: Option<WeldJoint>,
    #[serde(default)]
    pub other_side: Option<WeldJoint>,
    #[serde(default)]
    pub all_around: bool,
    #[serde(default)]
    pub field_weld: bool,
    #[serde(default)]
    pub staggered: bool,
    /// ISO System A identification line above (true) or below the continuous line.
    #[serde(default)]
    pub identification_above: bool,
    #[serde(default)]
    pub units: super::Units,
    #[serde(default = "precision")]
    pub precision: usize,
    #[serde(default)]
    pub tail: String,
    #[serde(default = "reference_length")]
    pub reference_length_mm: f64,
    #[serde(default = "text_size")]
    pub text_size_mm: f64,
    #[serde(default = "symbol_size")]
    pub symbol_height_mm: f64,
}
impl Default for WeldSymbol {
    fn default() -> Self {
        Self {
            version: version(),
            convention: WeldConvention::default(),
            arrow_side: Some(WeldJoint::default()),
            other_side: None,
            all_around: false,
            field_weld: false,
            staggered: false,
            identification_above: false,
            units: super::Units::default(),
            precision: precision(),
            tail: String::new(),
            reference_length_mm: reference_length(),
            text_size_mm: text_size(),
            symbol_height_mm: symbol_size(),
        }
    }
}
impl WeldSymbol {
    pub fn validate(&self) -> Result<()> {
        if self.version != SYMBOL_CONTRACT_VERSION
            || !style_valid(self.text_size_mm, self.symbol_height_mm, self.precision)
            || !self.reference_length_mm.is_finite()
            || !(24. ..=200.).contains(&self.reference_length_mm)
            || !printable(&self.tail)
            || (self.arrow_side.is_none() && self.other_side.is_none())
        {
            return Err(invalid(
                "Weld symbol version 1 requires at least one joint, up to 128 printable tail characters, precision 0..6, 1–10 mm text, 3–20 mm symbols and a 24–200 mm reference line",
            ));
        }
        for joint in [&self.arrow_side, &self.other_side].into_iter().flatten() {
            if [&joint.size, &joint.length, &joint.pitch]
                .into_iter()
                .flatten()
                .any(|v| !valid_expression(v))
                || joint.count.is_some_and(|n| !(2..=9999).contains(&n))
                || joint.finish.len() > 1
                || !joint.finish.chars().all(|c| "CGHMRU".contains(c))
            {
                return Err(invalid(
                    "Weld dimensions need valid expressions; counts are 2..9999 and AWS finish is C, G, H, M, R or U",
                ));
            }
            if joint.pitch.is_some() && joint.length.is_none()
                || joint.count.is_some() && joint.pitch.is_none()
            {
                return Err(invalid(
                    "An intermittent weld needs length and pitch; a count additionally requires pitch",
                ));
            }
            if !joint.finish.is_empty()
                && (joint.contour == WeldContour::None
                    || self.convention != WeldConvention::AwsA2_4)
            {
                return Err(invalid(
                    "AWS finish letters require a contour and the AWS convention; ISO process instructions belong in the tail",
                ));
            }
            if joint.fillet_size == FilletSize::Throat
                && (joint.joint != WeldJointKind::Fillet
                    || self.convention == WeldConvention::AwsA2_4)
            {
                return Err(invalid(
                    "An explicit throat-size prefix is supported only for ISO fillet welds",
                ));
            }
            if matches!(joint.joint, WeldJointKind::Spot | WeldJointKind::Seam)
                && joint.contour != WeldContour::None
            {
                return Err(invalid(
                    "Spot and seam contour modifiers are outside the supported symbol subset",
                ));
            }
        }
        if self.convention != WeldConvention::AwsA2_4
            && self.arrow_side.is_some()
            && self.other_side.is_some()
            && [&self.arrow_side, &self.other_side]
                .into_iter()
                .flatten()
                .any(|joint| matches!(joint.joint, WeldJointKind::Spot | WeldJointKind::Seam))
        {
            return Err(invalid(
                "ISO spot/seam interface symbols currently require one joint; mixed or two-sided interface symbols are unsupported",
            ));
        }
        if self.staggered
            && ![&self.arrow_side, &self.other_side]
                .into_iter()
                .all(|side| {
                    side.as_ref().is_some_and(|j| {
                        j.joint == WeldJointKind::Fillet && j.length.is_some() && j.pitch.is_some()
                    })
                })
        {
            return Err(invalid(
                "Staggering currently requires intermittent fillet welds on both sides",
            ));
        }
        Ok(())
    }
    pub(super) fn render(
        &self,
        doc: &Document,
        definition: &str,
        target: [f64; 2],
        offset: [f64; 2],
        out: &mut Vec<Primitive>,
    ) -> Result<()> {
        self.validate()?;
        let at = [target[0] + offset[0], target[1] + offset[1]];
        leader(out, target, at)?;
        let end = [at[0] + self.reference_length_mm, at[1]];
        line(out, at, end);
        if self.all_around {
            circle(out, at, self.symbol_height_mm * 0.2);
        }
        if self.field_weld {
            let h = self.symbol_height_mm;
            line(out, at, [at[0], at[1] + h * 1.5]);
            out.push(Primitive::Polyline {
                points: vec![
                    [at[0], at[1] + h * 1.5],
                    [at[0] + h * 0.65, at[1] + h * 1.15],
                    [at[0], at[1] + h * 0.8],
                ],
                dashed: false,
            });
        }
        let symmetric = self.arrow_side.is_some() && self.arrow_side == self.other_side;
        let interface = [&self.arrow_side, &self.other_side]
            .into_iter()
            .flatten()
            .any(|joint| matches!(joint.joint, WeldJointKind::Spot | WeldJointKind::Seam));
        let (arrow_y, arrow_sign, other_y, other_sign) =
            if self.convention == WeldConvention::Iso2553SystemA && !symmetric && !interface {
                let sign = if self.identification_above { 1. } else { -1. };
                let y = at[1] + sign * self.symbol_height_mm * 0.65;
                out.push(Primitive::Polyline {
                    points: vec![[at[0] + self.symbol_height_mm * 0.7, y], [end[0], y]],
                    dashed: true,
                });
                (at[1], -sign, y, sign)
            } else if self.convention == WeldConvention::Iso2553SystemA {
                (at[1], 1., at[1], -1.)
            } else {
                (at[1], -1., at[1], 1.)
            };
        if let Some(joint) = &self.arrow_side {
            self.render_joint(
                doc,
                definition,
                joint,
                [at[0] + self.reference_length_mm * 0.4, arrow_y],
                arrow_sign,
                out,
            )?;
        }
        if let Some(joint) = &self.other_side {
            self.render_joint(
                doc,
                definition,
                joint,
                [
                    at[0]
                        + self.reference_length_mm * 0.4
                        + if self.staggered {
                            self.symbol_height_mm * 1.2
                        } else {
                            0.
                        },
                    other_y,
                ],
                other_sign,
                out,
            )?;
        }
        if !self.tail.is_empty() {
            let h = self.symbol_height_mm * 0.6;
            line(out, end, [end[0] + h, end[1] + h]);
            line(out, end, [end[0] + h, end[1] - h]);
            text(
                out,
                [
                    end[0] + h + self.text_size_mm * 0.4,
                    end[1] - self.text_size_mm * 0.4,
                ],
                self.text_size_mm,
                &self.tail,
            );
        }
        // Annotation units are independent of a sheet's notes and dimension
        // settings. Retain the distinction in every paper/export presentation.
        text(
            out,
            [
                at[0],
                at[1] - self.symbol_height_mm * 2.2 - self.text_size_mm * 2.,
            ],
            self.text_size_mm,
            if self.units == super::Units::Inches {
                "Weld dimensions: in"
            } else {
                "Weld dimensions: mm"
            },
        );
        Ok(())
    }
    fn render_joint(
        &self,
        doc: &Document,
        definition: &str,
        joint: &WeldJoint,
        at: [f64; 2],
        sign: f64,
        out: &mut Vec<Primitive>,
    ) -> Result<()> {
        let size = optional_length(doc, definition, &joint.size, "Weld size", false)?;
        let length = optional_length(doc, definition, &joint.length, "Weld length", false)?;
        let pitch = optional_length(doc, definition, &joint.pitch, "Weld pitch", false)?;
        if let (Some(length), Some(pitch)) = (length, pitch)
            && pitch <= length
        {
            return Err(invalid("Intermittent weld pitch must exceed weld length"));
        }
        let convert = |value: f64| {
            number(
                if self.units == super::Units::Inches {
                    value / 25.4
                } else {
                    value
                },
                self.precision,
            )
        };
        let h = self.symbol_height_mm;
        let x = at[0];
        let y = at[1];
        let high = y + sign * h;
        match joint.joint {
            WeldJointKind::Fillet => {
                out.push(Primitive::Polyline {
                    points: vec![[x, y], [x, high], [x + h, y], [x, y]],
                    dashed: false,
                });
            }
            WeldJointKind::SquareGroove => {
                line(out, [x, y], [x, high]);
                line(out, [x + h * 0.55, y], [x + h * 0.55, high]);
            }
            WeldJointKind::VGroove => {
                line(out, [x, y], [x - h * 0.45, high]);
                line(out, [x, y], [x + h * 0.45, high]);
            }
            WeldJointKind::BevelGroove => {
                line(out, [x, y], [x, high]);
                line(out, [x, y], [x + h * 0.6, high]);
            }
            WeldJointKind::PlugSlot => {
                out.push(Primitive::Polyline {
                    points: vec![
                        [x - h * 0.4, y],
                        [x - h * 0.4, high],
                        [x + h * 0.4, high],
                        [x + h * 0.4, y],
                    ],
                    dashed: false,
                });
            }
            WeldJointKind::Spot => {
                let center_y = if self.convention == WeldConvention::AwsA2_4 {
                    y + sign * h * 0.55
                } else {
                    y
                };
                circle(out, [x, center_y], h * 0.45);
            }
            WeldJointKind::Seam => {
                let center_y = if self.convention == WeldConvention::AwsA2_4 {
                    y + sign * h * 0.55
                } else {
                    y
                };
                circle(out, [x, center_y], h * 0.45);
                line(
                    out,
                    [x - h * 0.65, center_y - h * 0.2],
                    [x + h * 0.65, center_y - h * 0.2],
                );
                line(
                    out,
                    [x - h * 0.65, center_y + h * 0.2],
                    [x + h * 0.65, center_y + h * 0.2],
                );
            }
        }
        let baseline = if sign > 0. { y + h * 0.3 } else { y - h * 0.8 };
        if let Some(size) = size {
            let prefix = if self.convention != WeldConvention::AwsA2_4
                && joint.joint == WeldJointKind::Fillet
            {
                if joint.fillet_size == FilletSize::Throat {
                    "a"
                } else {
                    "z"
                }
            } else {
                ""
            };
            let value = format!("{prefix}{}", convert(size)?);
            text(
                out,
                [
                    x - h * 0.8 - value.len() as f64 * self.text_size_mm * 0.6,
                    baseline,
                ],
                self.text_size_mm,
                value,
            );
        }
        if let Some(length) = length {
            let value = if let Some(pitch) = pitch {
                if self.convention == WeldConvention::AwsA2_4 {
                    format!(
                        "{}-{}{}",
                        convert(length)?,
                        convert(pitch)?,
                        joint.count.map(|n| format!(" ({n})")).unwrap_or_default()
                    )
                } else {
                    format!(
                        "{}{}({})",
                        joint.count.map(|n| format!("{n}x")).unwrap_or_default(),
                        convert(length)?,
                        convert(pitch - length)?
                    )
                }
            } else {
                convert(length)?
            };
            text(out, [x + h * 1.2, baseline], self.text_size_mm, value);
        }
        let contour_y = y + sign * h * 1.2;
        match joint.contour {
            WeldContour::None => {}
            WeldContour::Flush => line(out, [x - h * 0.25, contour_y], [x + h * 0.8, contour_y]),
            WeldContour::Convex | WeldContour::Concave => {
                let bulge = if joint.contour == WeldContour::Convex {
                    sign
                } else {
                    -sign
                };
                let points = (0..=16)
                    .map(|i| {
                        let t = i as f64 / 16.;
                        [
                            x - h * 0.25 + h * 1.05 * t,
                            contour_y + bulge * h * 0.2 * (std::f64::consts::PI * t).sin(),
                        ]
                    })
                    .collect();
                out.push(Primitive::Polyline {
                    points,
                    dashed: false,
                });
            }
        }
        if !joint.finish.is_empty() {
            text(
                out,
                [
                    x + h * 0.15,
                    contour_y + sign * self.text_size_mm * 1.2
                        - if sign < 0. { self.text_size_mm } else { 0. },
                ],
                self.text_size_mm,
                &joint.finish,
            );
        }
        Ok(())
    }
}
fn arc(out: &mut Vec<Primitive>, center: [f64; 2], radius: f64, start: f64, end: f64) {
    out.push(Primitive::Polyline {
        points: (0..=24)
            .map(|i| {
                let a = start + (end - start) * i as f64 / 24.;
                [center[0] + radius * a.cos(), center[1] + radius * a.sin()]
            })
            .collect(),
        dashed: false,
    });
}

/// Stable annotation-owned fields can be moved, duplicated and restyled without
/// coupling their source identities to a paper position or native editor.
pub fn semantic_fields(
    sheet: &crate::Drawing,
    entity: &super::Annotation,
    state: &str,
) -> Vec<crate::presentation::SemanticElement> {
    fn optional(
        sheet: &crate::Drawing,
        entity: &super::Annotation,
        source: &serde_json::Value,
        path: &str,
        default: serde_json::Value,
        out: &mut Vec<crate::presentation::SemanticElement>,
        state: &str,
    ) {
        let Some(value) = source.pointer(path) else {
            return;
        };
        let label = format!(
            "Enable {}",
            path.trim_start_matches("/kind/symbol/")
                .replace('/', " · ")
                .replace('_', " ")
        );
        out.push(crate::presentation::SemanticElement {id:format!("{}::symbol{path}/enabled",entity.id),role:"drawing-annotation-field".into(),label,value:Some((!value.is_null()).to_string()),state:state.into(),commands:vec!["drawing.sheet-edit".into()],relationships:vec![sheet.id.clone(),entity.id.clone()],data:serde_json::json!({"contract":"tcad.drawing-symbol.field","version":SYMBOL_CONTRACT_VERSION,"setter":"drawing-annotation","sheet_id":sheet.id,"annotation_id":entity.id,"field":path,"optional_enabled":true,"default":default,"json":!value.is_null(),"value_type":"boolean","editable":true,"edit_command":"drawing.sheet-edit"})});
    }
    fn emit(
        sheet: &crate::Drawing,
        entity: &super::Annotation,
        value: &serde_json::Value,
        path: &str,
        out: &mut Vec<crate::presentation::SemanticElement>,
        state: &str,
    ) {
        match value {
            serde_json::Value::Object(values) => {
                for (key, value) in values {
                    emit(sheet, entity, value, &format!("{path}/{key}"), out, state);
                }
            }
            serde_json::Value::Array(values) => {
                for (index, value) in values.iter().enumerate() {
                    emit(sheet, entity, value, &format!("{path}/{index}"), out, state);
                }
            }
            _ => {
                let key = path.rsplit('/').next().unwrap_or(path);
                let surface = matches!(entity.kind, super::AnnotationKind::SurfaceFinish { .. });
                let choices: &[&str] = match key {
                    "convention" if surface => &["iso1302", "asme_y14_36"],
                    "convention" => &["aws_a2_4", "iso2553_system_a", "iso2553_system_b"],
                    "removal" => &["basic", "required", "prohibited"],
                    "parameter" => &["ra", "rz", "rq"],
                    "roughness_units" => &["micrometers", "microinches"],
                    "lay" => &[
                        "none",
                        "parallel",
                        "perpendicular",
                        "crossed",
                        "multidirectional",
                        "circular",
                        "radial",
                        "particulate",
                    ],
                    "units" => &["millimeters", "inches"],
                    "joint" => &[
                        "fillet",
                        "square_groove",
                        "v_groove",
                        "bevel_groove",
                        "plug_slot",
                        "spot",
                        "seam",
                    ],
                    "fillet_size" => &["leg", "throat"],
                    "contour" => &["none", "flush", "convex", "concave"],
                    _ => &[],
                };
                let quantity = matches!(
                    key,
                    "maximum"
                        | "minimum"
                        | "sampling_length"
                        | "allowance"
                        | "size"
                        | "length"
                        | "pitch"
                ) && !value.is_null();
                let kind = if value.is_null() {
                    "json"
                } else if !choices.is_empty() {
                    "choice"
                } else if quantity {
                    "expression"
                } else if value.is_number() {
                    "number"
                } else if value.is_boolean() {
                    "boolean"
                } else {
                    "text"
                };
                let label = path
                    .trim_start_matches("/kind/symbol/")
                    .trim_start_matches('/')
                    .replace('/', " · ")
                    .replace('_', " ");
                let mut data = serde_json::json!({"contract":"tcad.drawing-symbol.field","version":SYMBOL_CONTRACT_VERSION,"setter":"drawing-annotation","sheet_id":sheet.id,"annotation_id":entity.id,"field":path,"json":value,"value_type":kind,"editable":key!="version","choices":choices,"edit_command":"drawing.sheet-edit","accessibility_label":format!("{} · {label}",entity.kind.label())});
                if quantity {
                    data["unit"] = serde_json::json!("mm");
                    data["dimension"] = serde_json::json!("length");
                }
                if key == "precision" {
                    data["minimum"] = serde_json::json!(0);
                    data["maximum"] = serde_json::json!(6);
                }
                if key == "text_size_mm" {
                    data["minimum"] = serde_json::json!(1);
                    data["maximum"] = serde_json::json!(10);
                    data["unit"] = serde_json::json!("paper mm");
                }
                if key == "symbol_height_mm" {
                    data["minimum"] = serde_json::json!(3);
                    data["maximum"] = serde_json::json!(20);
                    data["unit"] = serde_json::json!("paper mm");
                }
                if key == "reference_length_mm" {
                    data["minimum"] = serde_json::json!(24);
                    data["maximum"] = serde_json::json!(200);
                    data["unit"] = serde_json::json!("paper mm");
                }
                if key == "count" && !value.is_null() {
                    data["minimum"] = serde_json::json!(2);
                    data["maximum"] = serde_json::json!(9999);
                }
                if path == "/view" {
                    data["value_type"] = serde_json::json!("choice");
                    data["choices"] = serde_json::json!(super::view_keys(sheet));
                }
                out.push(crate::presentation::SemanticElement {
                    id: format!("{}::symbol{path}", entity.id),
                    role: "drawing-annotation-field".into(),
                    label,
                    value: Some(
                        value
                            .as_str()
                            .map(str::to_owned)
                            .unwrap_or_else(|| value.to_string()),
                    ),
                    state: state.into(),
                    commands: vec!["drawing.sheet-edit".into()],
                    relationships: vec![sheet.id.clone(), entity.id.clone()],
                    data,
                });
            }
        }
    }
    let mut out = vec![];
    if let Ok(value) = serde_json::to_value(entity) {
        if let Some(symbol) = value.pointer("/kind/symbol") {
            emit(sheet, entity, symbol, "/kind/symbol", &mut out, state);
        }
        if let Some(offset) = value.get("offset_mm") {
            emit(sheet, entity, offset, "/offset_mm", &mut out, state);
        }
        if let Some(view) = value.get("view") {
            emit(sheet, entity, view, "/view", &mut out, state);
        }
        if matches!(entity.kind, super::AnnotationKind::SurfaceFinish { .. }) {
            for (field, default) in [
                ("minimum", "0.8 um"),
                ("sampling_length", "0.8 mm"),
                ("allowance", "0.1 mm"),
            ] {
                optional(
                    sheet,
                    entity,
                    &value,
                    &format!("/kind/symbol/{field}"),
                    serde_json::json!(default),
                    &mut out,
                    state,
                );
            }
        } else {
            for side in ["arrow_side", "other_side"] {
                let base = format!("/kind/symbol/{side}");
                optional(
                    sheet,
                    entity,
                    &value,
                    &base,
                    serde_json::json!(WeldJoint::default()),
                    &mut out,
                    state,
                );
                for (field, default) in [
                    ("size", serde_json::json!("3 mm")),
                    ("length", serde_json::json!("20 mm")),
                    ("pitch", serde_json::json!("50 mm")),
                    ("count", serde_json::json!(2)),
                ] {
                    optional(
                        sheet,
                        entity,
                        &value,
                        &format!("{base}/{field}"),
                        default,
                        &mut out,
                        state,
                    );
                }
            }
        }
    }
    out
}
