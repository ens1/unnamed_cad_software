//! Build a drawing indication from the configuration that produced the bead.
use super::{WeldJoint, WeldJointKind, WeldSymbol, invalid, stitch_and_pitch, weld_number};
use crate::{
    Expr, Result,
    drawing::Units,
    weld_beads::{Callout, JointType, Profile},
};

impl WeldSymbol {
    /// The caller determines the physical side from the bead and view frames.
    /// An exposed deposited crown is on the arrow side of its local joint.
    pub fn from_bead(callout: &Callout, units: Units, near_side: bool) -> Result<Self> {
        let size = callout.size_mm;
        if !size.is_finite()
            || size <= 0.
            || callout.square_groove && callout.profile != Profile::Butt
        {
            return Err(invalid(
                "The weld bead has no valid evaluated size or preparation",
            ));
        }
        let pitch_pair = if let Some([length, gap]) = callout.stitch_mm {
            let pitch = length + gap;
            if !length.is_finite() || length <= 0. || !pitch.is_finite() || pitch <= length {
                return Err(invalid(
                    "The weld bead needs a positive stitch length and gap",
                ));
            }
            Some([length, pitch])
        } else {
            None
        };
        let scale = if units == Units::Inches { 25.4 } else { 1. };
        let min_precision = if units == Units::Inches { 3 } else { 2 };
        let precision = (min_precision..=6)
            .find(|&n| {
                let factor = 10_f64.powi(n as i32);
                std::iter::once(size)
                    .chain(pitch_pair.into_iter().flatten())
                    .all(|mm| {
                        let v = mm / scale;
                        ((v * factor).round() / factor - v).abs() <= v.abs() * 1e-8
                    })
            })
            .unwrap_or(6);
        let printed = |mm| weld_number(mm / scale, precision);
        let width = printed(size)?;
        let mm_expression = |mm| Expr(format!("{mm} mm"));
        let mut tail = Vec::new();
        if callout.joint_type != JointType::Unspecified {
            tail.push(callout.joint_type.label().to_uppercase());
        }
        let (kind, size_expression, label) = match callout.profile {
            Profile::Fillet => (WeldJointKind::Fillet, Some(mm_expression(size)), width),
            Profile::Butt if callout.square_groove => {
                // The cap width gives no penetration depth or groove size.
                tail.push(format!("CAP WIDTH {width}"));
                (WeldJointKind::SquareGroove, None, String::new())
            }
            Profile::Butt => (
                WeldJointKind::CapWidth,
                Some(mm_expression(size)),
                format!("CAP WIDTH {width}"),
            ),
        };
        let stitch_label = if let Some([length, pitch]) = pitch_pair {
            stitch_and_pitch(length / scale, pitch / scale, precision)?
        } else {
            String::new()
        };
        let joint = WeldJoint {
            joint: kind,
            size: size_expression,
            length: pitch_pair.map(|[length, _]| mm_expression(length)),
            pitch: pitch_pair.map(|[_, pitch]| mm_expression(pitch)),
            ..WeldJoint::default()
        };
        let (arrow_side, other_side) = if near_side {
            (Some(joint), None)
        } else {
            (None, Some(joint))
        };
        let text_size_mm = 2.25;
        let symbol_height_mm = 4.;
        let text_width = |s: &str| s.chars().count() as f64 * text_size_mm * 0.66;
        // Keep the size to the left of the basic weld symbol and the length
        // and pitch to its right, including when the arrow and tail reverse.
        let reference_length_mm = 27_f64
            .max((text_width(&label) + symbol_height_mm + 2.) / 0.4)
            .max((text_width(&stitch_label) + symbol_height_mm * 1.2 + 2.) / 0.6);
        let symbol = Self {
            arrow_side,
            other_side,
            all_around: callout.all_around,
            field_weld: callout.field_weld,
            units,
            precision,
            tail: tail.join(" / "),
            reference_length_mm,
            text_size_mm,
            symbol_height_mm,
            ..Self::default()
        };
        symbol.validate()?;
        Ok(symbol)
    }
}
