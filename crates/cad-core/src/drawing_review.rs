//! Compact evidence of changes to the vectors actually sent to paper. Produced
//! during ordinary sheet validation, never by rebuilding sheets in the UI.
use crate::{
    Drawing, Id,
    drawing::{DimensionKind, Primitive, Sheet},
    drawing_interaction::Target,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Item {
    #[serde(default)]
    pub page: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<Target>,
    pub label: String,
    pub printed_text: String,
    pub digest: String,
}
pub type Sheets = BTreeMap<Id, BTreeMap<String, Item>>;

/// Compare at sub-micron paper resolution. Reversing a native wire or
/// reparameterizing the same rational stroke must not flag a drawing change.
fn number(hash: &mut Sha256, value: f64, scale: f64) {
    let rounded = (value * scale).round();
    hash.update(if rounded == 0. { 0_f64 } else { rounded }.to_le_bytes());
}

fn stroke_key(primitive: &Primitive) -> [u8; 32] {
    let oriented = |reverse: bool| {
        let mut hash = Sha256::new();
        match primitive {
            Primitive::Polyline { points, .. } | Primitive::HiddenPolyline { points } => {
                let tag = match primitive {
                    Primitive::HiddenPolyline { .. } => 2,
                    Primitive::Polyline { dashed: true, .. } => 1,
                    _ => 0,
                };
                hash.update([tag]);
                for i in 0..points.len() {
                    let p = points[if reverse { points.len() - 1 - i } else { i }];
                    for x in p {
                        number(&mut hash, x, 1e6);
                    }
                }
            }
            Primitive::Spline { curve, .. } | Primitive::HiddenSpline { curve } => {
                let tag = match primitive {
                    Primitive::HiddenSpline { .. } => 5,
                    Primitive::Spline { dashed: true, .. } => 4,
                    _ => 3,
                };
                hash.update([tag]);
                hash.update((curve.degree as u64).to_le_bytes());
                hash.update((curve.control_points.len() as u64).to_le_bytes());
                let largest = curve.weights.iter().copied().fold(0_f64, f64::max);
                for i in 0..curve.control_points.len() {
                    let i = if reverse {
                        curve.control_points.len() - 1 - i
                    } else {
                        i
                    };
                    for x in curve.control_points[i] {
                        number(&mut hash, x, 1e6);
                    }
                    number(
                        &mut hash,
                        curve.weights.get(i).map_or(1., |w| w / largest),
                        1e12,
                    );
                }
                if let (Some(low), Some(high)) = (curve.knots.first(), curve.knots.last()) {
                    for i in 0..curve.knots.len() {
                        let t = if reverse {
                            high - curve.knots[curve.knots.len() - 1 - i]
                        } else {
                            curve.knots[i] - low
                        };
                        number(&mut hash, t / (high - low), 1e12);
                    }
                }
            }
            Primitive::Text { at, size, text } => {
                hash.update([6]);
                for x in at {
                    number(&mut hash, *x, 1e6);
                }
                number(&mut hash, *size, 1e6);
                hash.update(text.as_bytes());
            }
            Primitive::Circle { center, radius } => {
                hash.update([7]);
                for x in center {
                    number(&mut hash, *x, 1e6);
                }
                number(&mut hash, *radius, 1e6);
            }
        }
        <[u8; 32]>::from(hash.finalize())
    };
    let forward = oriented(false);
    match primitive {
        Primitive::Circle { .. } | Primitive::Text { .. } => forward,
        _ => forward.min(oriented(true)),
    }
}

pub fn collect(source: &Drawing, page: &Sheet, number: u32, items: &mut BTreeMap<String, Item>) {
    // A native B-rep can enumerate unchanged strokes in a different order.
    // Hash individual strokes, then combine them as an unordered multiset.
    let hashes = page.primitives.iter().map(stroke_key).collect::<Vec<_>>();
    for group in &page.groups {
        let label = match &group.target {
            Target::View { key } => key.replace('-', " "),
            Target::ModelItem { id } => page
                .model_items
                .get(id)
                .map_or_else(|| "Model dimension".into(), |item| item.label.clone()),
            Target::Dimension { id } => source
                .dimensions
                .iter()
                .position(|d| &d.id == id)
                .map_or_else(
                    || "Dimension".into(),
                    |index| {
                        let name = if source.dimensions[index].options.kind
                            == DimensionKind::HoleCallout
                        {
                            "Hole callout"
                        } else {
                            "Dimension"
                        };
                        format!("{name} {}", index + 1)
                    },
                ),
            Target::Annotation { id } => source
                .entities
                .iter()
                .find(|a| &a.id == id)
                .map_or_else(|| "Annotation".into(), |a| a.kind.label().into()),
            Target::Table { id } | Target::TableSegment { table: id, .. } => source
                .tables
                .iter()
                .find(|t| &t.id == id)
                .map_or_else(|| "Table".into(), |t| t.title.clone()),
            Target::TitleCell { .. } => continue,
        };
        let span = group.primitive_start..group.primitive_start + group.primitive_count;
        let mut ordered = hashes[span.clone()].to_vec();
        ordered.sort_unstable();
        let mut digest = Sha256::new();
        for entry in ordered {
            digest.update(entry);
        }
        let printed_text = if matches!(
            group.target,
            Target::Dimension { .. } | Target::Annotation { .. } | Target::ModelItem { .. }
        ) {
            page.primitives[span]
                .iter()
                .filter_map(|p| {
                    if let Primitive::Text { text, .. } = p {
                        Some(text.as_str())
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>()
                .join(" ")
        } else {
            String::new()
        };
        items.insert(
            format!("{number}/{}", group.target.identity()),
            Item {
                page: number,
                target: Some(group.target.clone()),
                label,
                printed_text,
                digest: format!("{:x}", digest.finalize()),
            },
        );
    }
}
