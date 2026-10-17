//! Disposable paper display work runs off the event thread. One worker has at
//! most one running request, one replacement request, and one reply.
use super::*;
use std::sync::{
    Condvar, Mutex,
    atomic::{AtomicBool, AtomicU64, Ordering},
};

struct Request {
    ticket: u64,
    key: BuildKey,
    geometry: cad_core::Document,
    evaluation: Arc<Evaluation>,
}
type Reply = (u64, std::result::Result<Cache, String>);
#[derive(Default)]
struct Mailbox {
    pending: Mutex<Option<Request>>,
    reply: Mutex<Option<Reply>>,
    wake: Condvar,
    latest: AtomicU64,
    stopped: AtomicBool,
}
pub struct PaperWorker {
    mailbox: Arc<Mailbox>,
}
impl PaperWorker {
    pub fn new() -> Self {
        let mailbox = Arc::new(Mailbox::default());
        let thread = mailbox.clone();
        std::thread::spawn(move || {
            loop {
                let Ok(mut pending) = thread.pending.lock() else {
                    break;
                };
                while pending.is_none() && !thread.stopped.load(Ordering::Acquire) {
                    let Ok(next) = thread.wake.wait(pending) else {
                        return;
                    };
                    pending = next;
                }
                if thread.stopped.load(Ordering::Acquire) {
                    break;
                }
                let Some(request) = pending.take() else {
                    continue;
                };
                drop(pending);
                let ticket = request.ticket;
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| build(request, &thread)))
                .map_err(|_| "Drawing preparation failed unexpectedly; change the sheet or reopen it to retry".to_owned())
                .and_then(|result| result.map_err(|error| error.to_string()));
                if thread.latest.load(Ordering::Acquire) == ticket
                    && !thread.stopped.load(Ordering::Acquire)
                    && let Ok(mut reply) = thread.reply.lock()
                {
                    *reply = Some((ticket, result));
                }
            }
        });
        Self { mailbox }
    }
    pub(super) fn request(
        &self,
        key: BuildKey,
        geometry: cad_core::Document,
        evaluation: Arc<Evaluation>,
    ) -> u64 {
        let ticket = self.mailbox.latest.fetch_add(1, Ordering::AcqRel) + 1;
        if let Ok(mut pending) = self.mailbox.pending.lock() {
            *pending = Some(Request {
                ticket,
                key,
                geometry,
                evaluation,
            });
            self.mailbox.wake.notify_one();
        }
        ticket
    }
    pub(super) fn take(&self) -> Option<Reply> {
        self.mailbox.reply.lock().ok()?.take()
    }
}
impl Default for PaperWorker {
    fn default() -> Self {
        Self::new()
    }
}
impl Drop for PaperWorker {
    fn drop(&mut self) {
        self.mailbox.stopped.store(true, Ordering::Release);
        if let Ok(mut pending) = self.mailbox.pending.lock() {
            *pending = None;
        }
        self.mailbox.wake.notify_one();
    }
}
fn current(mailbox: &Mailbox, ticket: u64) -> Result<()> {
    if mailbox.stopped.load(Ordering::Acquire) || mailbox.latest.load(Ordering::Acquire) != ticket {
        return Err(invalid("Superseded paper projection"));
    }
    Ok(())
}
fn build(request: Request, mailbox: &Mailbox) -> Result<Cache> {
    current(mailbox, request.ticket)?;
    let paper = cad_core::drawing::build_sheet_preview(
        &request.geometry,
        &request.evaluation,
        &request.key.source,
        request.key.page,
    )?;
    current(mailbox, request.ticket)?;
    let scene = interaction::build(
        &request.geometry,
        &request.evaluation,
        &request.key.source,
        &paper,
        request.key.page,
    )?;
    let mut paint = Vec::with_capacity(paper.primitives.len());
    for primitive in &paper.primitives {
        current(mailbox, request.ticket)?;
        paint.push(PreparedPrimitive::new(primitive)?);
    }
    Ok(Cache {
        source: request.key.source.clone(),
        page: request.key.page,
        key: request.key,
        geometry: request.geometry,
        evaluation: request.evaluation,
        paper: Arc::new(paper),
        scene: Arc::new(scene),
        paint: Arc::new(paint),
    })
}

pub(super) enum PreparedPrimitive {
    Path {
        points: Vec<[f64; 2]>,
        segments: Option<Vec<[[f64; 2]; 2]>>,
        bounds: [[f64; 2]; 2],
    },
    Text {
        point: [f64; 2],
        size: f64,
        text: String,
    },
    Circle {
        center: [f64; 2],
        radius: f64,
    },
}
enum Dash {
    Solid,
    Reference,
    Hidden,
}
impl PreparedPrimitive {
    fn new(primitive: &Primitive) -> Result<Self> {
        match primitive {
            Primitive::Polyline { points, dashed } => Self::path(
                points.clone(),
                if *dashed {
                    Dash::Reference
                } else {
                    Dash::Solid
                },
            ),
            Primitive::Spline { curve, dashed } => Self::path(
                cad_core::drawing::display_curve(curve)?,
                if *dashed {
                    Dash::Reference
                } else {
                    Dash::Solid
                },
            ),
            Primitive::HiddenPolyline { points } => Self::path(points.clone(), Dash::Hidden),
            Primitive::HiddenSpline { curve } => {
                Self::path(cad_core::drawing::display_curve(curve)?, Dash::Hidden)
            }
            Primitive::Text { at, size, text } => Ok(Self::Text {
                point: *at,
                size: *size,
                text: text.clone(),
            }),
            Primitive::Circle { center, radius } => Ok(Self::Circle {
                center: *center,
                radius: *radius,
            }),
        }
    }
    fn path(points: Vec<[f64; 2]>, dash: Dash) -> Result<Self> {
        let mut bounds = [[f64::INFINITY; 2], [f64::NEG_INFINITY; 2]];
        for point in &points {
            for axis in 0..2 {
                bounds[0][axis] = bounds[0][axis].min(point[axis]);
                bounds[1][axis] = bounds[1][axis].max(point[axis]);
            }
        }
        let segments = match dash {
            Dash::Solid => None,
            Dash::Reference => Some(cad_core::drawing_strokes::reference_segments(&points)?),
            Dash::Hidden => Some(cad_core::drawing_strokes::hidden_segments(&points)?),
        };
        Ok(Self::Path {
            points,
            segments,
            bounds,
        })
    }
    pub(super) fn draw(&self, painter: &egui::Painter, paper: Rect, scale: f32, stroke: Stroke) {
        match self {
            Self::Path {
                points,
                segments,
                bounds,
            } => {
                if points.len() < 2 {
                    return;
                }
                let bounds =
                    Rect::from_two_pos(at(paper, scale, bounds[0]), at(paper, scale, bounds[1]))
                        .expand(stroke.width);
                if !painter.clip_rect().intersects(bounds) {
                    return;
                }
                if let Some(segments) = segments {
                    painter.extend(segments.iter().filter_map(|[a, b]| {
                        let [a, b] = [at(paper, scale, *a), at(paper, scale, *b)];
                        painter
                            .clip_rect()
                            .intersects(Rect::from_two_pos(a, b).expand(stroke.width))
                            .then(|| egui::Shape::line_segment([a, b], stroke))
                    }));
                } else {
                    painter.add(egui::Shape::line(
                        points.iter().map(|p| at(paper, scale, *p)).collect(),
                        stroke,
                    ));
                }
            }
            Self::Text { point, size, text } => {
                painter.text(
                    at(paper, scale, *point),
                    egui::Align2::LEFT_BOTTOM,
                    text,
                    egui::FontId::proportional((*size as f32 * scale).max(1.)),
                    stroke.color,
                );
            }
            Self::Circle { center, radius } => {
                let center = at(paper, scale, *center);
                let radius = *radius as f32 * scale;
                if painter.clip_rect().intersects(
                    Rect::from_center_size(center, egui::vec2(radius * 2., radius * 2.))
                        .expand(stroke.width),
                ) {
                    painter.circle_stroke(center, radius, stroke);
                }
            }
        }
    }
}
