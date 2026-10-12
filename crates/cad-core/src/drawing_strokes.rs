//! Bounded paper-mm reference dash geometry shared by desktop and vector exchange.
use crate::{CadError, Result};
pub const REFERENCE_PATTERN_MM: [f64; 4] = [4., 1., 1., 1.];
pub fn reference_segments(points: &[[f64; 2]]) -> Result<Vec<[[f64; 2]; 2]>> {
    if points.len() > 1_000_000
        || points
            .iter()
            .flatten()
            .any(|value| !value.is_finite() || value.abs() > 1e12)
    {
        return Err(CadError::Invalid(
            "Reference stroke coordinates exceed their bounded finite paper geometry contract"
                .into(),
        ));
    }
    let mut segments = vec![];
    let mut phase = 0_usize;
    let mut remaining = REFERENCE_PATTERN_MM[0];
    for pair in points.windows(2) {
        let delta = [pair[1][0] - pair[0][0], pair[1][1] - pair[0][1]];
        let length = delta[0].hypot(delta[1]);
        if length < 1e-12 {
            continue;
        }
        let mut position = 0.;
        while position < length {
            let step = remaining.min(length - position);
            if phase.is_multiple_of(2) {
                segments.push(std::array::from_fn(|end| {
                    std::array::from_fn(|axis| {
                        pair[0][axis]
                            + delta[axis] * (position + if end == 0 { 0. } else { step }) / length
                    })
                }));
                if segments.len() > 1_000_000 {
                    return Err(CadError::Invalid(
                        "Reference stroke exceeds one million visible dash segments".into(),
                    ));
                }
            }
            position += step;
            remaining -= step;
            if remaining <= 1e-10 {
                phase = (phase + 1) % REFERENCE_PATTERN_MM.len();
                remaining = REFERENCE_PATTERN_MM[phase];
            }
        }
    }
    Ok(segments)
}
