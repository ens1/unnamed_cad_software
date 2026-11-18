//! Straight section line placement in the sheet's existing orthographic views.
use super::*;
use cad_core::{
    drawing::{self, PointReference, Sheet, ViewLayout},
    drawing_sections::{Definition, KEY},
};

#[derive(Clone, Debug)]
pub(super) enum Placement {
    Anchor,
    Direction {
        cut: Box<Definition>,
        anchor: [f64; 2],
    },
    View {
        cut: Box<Definition>,
        anchor: [f64; 2],
    },
}
fn contains(rect: [f64; 4], p: [f64; 2]) -> bool {
    (0..2).all(|i| p[i] >= rect[i] && p[i] <= rect[i] + rect[i + 2])
}
fn target_rect(sheet: &Drawing, rendered: &Sheet, p: [f64; 2]) -> Result<[f64; 4]> {
    let [low, high] = drawing::paper_bounds(sheet);
    let bottom = rendered
        .views
        .values()
        .map(|view| view.rect_mm[1])
        .fold(high[1], f64::min);
    let lower = [low[0], bottom];
    let upper = [high[0], high[1] - 7.];
    let size = [(upper[0] - lower[0]) * 0.4, (upper[1] - lower[1]) * 0.4];
    if size.iter().any(|size| *size < 10.) {
        return Err(invalid("The sheet needs more space for a section view"));
    }
    let at: [f64; 2] =
        std::array::from_fn(|i| (p[i] - size[i] * 0.5).clamp(lower[i], upper[i] - size[i]));
    Ok([at[0], at[1], size[0], size[1]])
}
fn direction(anchor: [f64; 2], at: [f64; 2], tol: f64) -> Result<[f64; 2]> {
    let mut d = [at[0] - anchor[0], at[1] - anchor[1]];
    if d[0].hypot(d[1]) < tol {
        return Err(invalid(
            "Pick a second position farther along the cutting line",
        ));
    }
    if d[0].abs() < tol * 0.5 {
        d[0] = 0.
    } else if d[1].abs() < tol * 0.5 {
        d[1] = 0.
    }
    let len = d[0].hypot(d[1]);
    Ok(d.map(|v| v / len))
}
pub(super) fn click(
    w: &mut Workspace,
    state: &mut State,
    sheet: &Drawing,
    scene: &Scene,
    p: [f64; 2],
    tol: f64,
    placement: Placement,
) -> Result<()> {
    let cache = state
        .cache
        .as_ref()
        .ok_or_else(|| invalid("Wait for the drawing geometry"))?;
    match placement {
        Placement::Anchor => {
            let mut parents = cache.paper.projections.iter().filter(|(k, v)| {
                matches!(k.as_str(), "front" | "top" | "right") && contains(v.rect_mm, p)
            });
            let (key, projection) = parents
                .next()
                .ok_or_else(|| invalid("Pick inside a Front, Top or Right view"))?;
            if parents.next().is_some() {
                return Err(invalid(
                    "The parent views overlap here. Pick an unambiguous position",
                ));
            }
            let hit = interaction::nearest_reference(scene, p, tol, Some(key));
            let through = if let Some(hit) = hit {
                let circular = hit.curve.as_ref().filter(|r| {
                    drawing::circular(&cache.geometry, &cache.evaluation, sheet, r).is_ok()
                });
                Some(if let Some(curve) = circular {
                    PointReference::CurveCenter {
                        curve: Box::new(curve.clone()),
                    }
                } else {
                    hit.point
                })
            } else {
                None
            };
            let anchor = if through.is_some() {
                projection.paper_point(
                    key,
                    drawing::point(&cache.geometry, &cache.evaluation, sheet, [0.; 3], &through)?,
                )?
            } else {
                p
            };
            let cut = Box::new(Definition {
                id: sheet
                    .section
                    .as_ref()
                    .map(|s| s.id.clone())
                    .unwrap_or_else(cad_core::id),
                parent: key.clone(),
                at: projection.unscale(anchor),
                through,
                direction: [1., 0.],
                reversed: false,
            });
            state.tool = Tool::Section(Placement::Direction { cut, anchor });
        }
        Placement::Direction { mut cut, anchor } => {
            cut.direction = direction(anchor, p, tol)?;
            state.tool = Tool::Section(Placement::View { cut, anchor });
        }
        Placement::View { mut cut, anchor } => {
            let rect = target_rect(sheet, &cache.paper, p)?;
            // The viewer lies on the positive section normal. The line-of-sight
            // arrows point from that side back toward the material.
            cut.reversed =
                (p[0] - anchor[0]) * cut.direction[1] - (p[1] - anchor[1]) * cut.direction[0] < 0.;
            let mut edited = sheet.clone();
            if !edited.views.iter().any(|key| key == KEY) {
                let mut views = drawing::view_keys(&edited);
                views.push(KEY.into());
                drawing::set_views(&mut edited, views)?;
                for (key, layout) in &cache.paper.views {
                    edited
                        .view_layouts
                        .entry(key.clone())
                        .or_insert_with(|| layout.clone());
                }
            }
            edited.section = Some(*cut);
            edited.view_layouts.insert(
                KEY.into(),
                ViewLayout {
                    rect_mm: rect,
                    scale: None,
                    alignment: None,
                },
            );
            publish(w, edited)?;
            state.tool = Tool::Select;
            w.status = "Section A-A placed".into();
        }
    }
    Ok(())
}
pub(super) fn preview(
    placement: &Placement,
    sheet: &Drawing,
    rendered: &Sheet,
    p: [f64; 2],
    painter: &egui::Painter,
    paper_view: (Rect, f32),
    color: Color32,
) {
    let (paper, scale) = paper_view;
    let (cut, anchor, placing) = match placement {
        Placement::Direction { cut, anchor } => (cut, *anchor, false),
        Placement::View { cut, anchor } => (cut, *anchor, true),
        _ => return,
    };
    let Some(parent) = rendered.projections.get(&cut.parent) else {
        return;
    };
    let d = if placing {
        cut.direction
    } else {
        direction(anchor, p, 0.5).unwrap_or([1., 0.])
    };
    let mut range = [f64::NEG_INFINITY, f64::INFINITY];
    for i in 0..2 {
        if d[i].abs() > 1e-12 {
            let a = (parent.rect_mm[i] - anchor[i]) / d[i];
            let b = (parent.rect_mm[i] + parent.rect_mm[i + 2] - anchor[i]) / d[i];
            range = [range[0].max(a.min(b)), range[1].min(a.max(b))];
        }
    }
    let ends = range.map(|t| at(paper, scale, [anchor[0] + d[0] * t, anchor[1] + d[1] * t]));
    let stroke = Stroke::new(1.5, color);
    painter.line_segment(ends, stroke);
    painter.circle_stroke(at(paper, scale, anchor), 4., stroke);
    if placing && let Ok([x, y, width, height]) = target_rect(sheet, rendered, p) {
        let sign = if (p[0] - anchor[0]) * d[1] - (p[1] - anchor[1]) * d[0] < 0. {
            -1.
        } else {
            1.
        };
        let gaze = egui::vec2(-d[1] as f32, -d[0] as f32) * sign * 14.;
        for end in ends {
            painter.arrow(end - gaze, gaze, stroke)
        }
        let rect = Rect::from_two_pos(
            at(paper, scale, [x, y]),
            at(paper, scale, [x + width, y + height]),
        );
        painter.rect_stroke(rect, 0., stroke, egui::StrokeKind::Inside);
    }
}
