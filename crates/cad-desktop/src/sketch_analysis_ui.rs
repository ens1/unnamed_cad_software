//! Disposable sketch analysis in a separate persistent worker, independent of navigation.
use crate::{Workspace, sketch_relations_ui, worker::WorkerClient};
use cad_core::{Id, Sketch, protocol::*};
use std::time::Instant;

#[derive(Clone, PartialEq, Eq)]
struct Context {
    document: Id,
    owner: Id,
    sketch: Id,
    token: Id,
    generation: u64,
    revision: u64,
    draft: Option<Id>,
    edit: u64,
    backend: String,
}
impl Context {
    fn current(w: &Workspace) -> Option<Self> {
        let source = w.sketch.as_ref()?;
        sketch_relations_ui::analysis_source(source)?;
        let draft = w.session.draft.as_ref();
        Some(Self {
            document: w.session.visible().id.clone(),
            owner: source.definition.clone(),
            sketch: source.sketch_id.clone(),
            token: source.feedback.analysis_token.clone(),
            generation: w.session.generation,
            revision: draft
                .map(|draft| draft.base_revision)
                .unwrap_or(w.session.committed.revision),
            draft: draft.map(|draft| draft.id.clone()),
            edit: draft.map(|draft| draft.edit).unwrap_or(0),
            backend: w.session.backend.clone(),
        })
    }
}

pub struct State {
    worker: WorkerClient,
    context: Option<Context>,
    source: Option<Sketch>,
    pending: Option<RequestIdentity>,
    submitted: bool,
    changed: Instant,
    sequence: u64,
    token: Id,
}
impl Default for State {
    fn default() -> Self {
        Self {
            worker: WorkerClient::new(),
            context: None,
            source: None,
            pending: None,
            submitted: false,
            changed: Instant::now(),
            sequence: 0,
            token: cad_core::id(),
        }
    }
}
impl State {
    fn cancel(&mut self) {
        // Retain an idle process across sketch edits. Superseded in-flight
        // work is terminated so it cannot delay newer cursor/draft feedback.
        if self.pending.is_some() {
            self.worker.cancel();
        }
        self.pending = None;
        self.context = None;
        self.source = None;
        self.submitted = false;
        self.changed = Instant::now();
        self.sequence = self.sequence.wrapping_add(1);
    }
}

pub fn retry(w: &mut Workspace) {
    w.sketch_analysis.cancel();
    if let Some(sketch) = &mut w.sketch {
        sketch.feedback.motion = None;
        sketch.feedback.motion_error = None;
        sketch.feedback.motion_pending = sketch.error.is_none();
    }
}

pub fn poll(w: &mut Workspace) {
    let current = Context::current(w);
    let changed = w.sketch_analysis.context != current
        || w.sketch_analysis.source.as_ref()
            != w.sketch
                .as_ref()
                .and_then(sketch_relations_ui::analysis_source);
    if changed {
        w.sketch_analysis.cancel();
        w.sketch_analysis.context = current;
        w.sketch_analysis.source = w
            .sketch
            .as_ref()
            .and_then(sketch_relations_ui::analysis_source)
            .cloned();
        if let Some(sketch) = &mut w.sketch {
            sketch.feedback.motion = None;
            sketch.feedback.motion_error = None;
            sketch.feedback.motion_pending = w.sketch_analysis.context.is_some();
        }
    }
    for response in w.sketch_analysis.worker.drain() {
        match response {
            WorkerResponse::SketchAnalyzed {
                identity,
                owner,
                sketch,
                analysis,
            } if w.sketch_analysis.pending.as_ref() == Some(&identity)
                && w.sketch_analysis
                    .context
                    .as_ref()
                    .is_some_and(|context| context.owner == owner && context.sketch == sketch)
                && Context::current(w) == w.sketch_analysis.context
                && w.sketch_analysis.source.as_ref()
                    == w.sketch
                        .as_ref()
                        .and_then(sketch_relations_ui::analysis_source) =>
            {
                w.sketch_analysis.pending = None;
                if let Some(source) = &mut w.sketch {
                    source.feedback.motion = Some(*analysis);
                    source.feedback.motion_error = None;
                    source.feedback.motion_pending = false;
                }
            }
            WorkerResponse::Error {
                identity: Some(identity),
                message,
            } if w.sketch_analysis.pending.as_ref() == Some(&identity)
                && Context::current(w) == w.sketch_analysis.context
                && w.sketch_analysis.source.as_ref()
                    == w.sketch
                        .as_ref()
                        .and_then(sketch_relations_ui::analysis_source) =>
            {
                w.sketch_analysis.pending = None;
                if let Some(source) = &mut w.sketch {
                    source.feedback.motion = None;
                    source.feedback.motion_error = Some(message);
                    source.feedback.motion_pending = false;
                }
            }
            _ => {}
        }
    }
    if !w.sketch_analysis.submitted
        && w.sketch_analysis.changed.elapsed().as_millis() >= 90
        && let (Some(context), Some(source)) = (
            w.sketch_analysis.context.as_ref(),
            w.sketch_analysis.source.as_ref(),
        )
    {
        let mut identity = w.session.identity();
        // Independent disposable analysis sequence. It is never accepted as a
        // document Evaluation and cannot affect Apply or engineering definitions.
        identity.draft = Some(w.sketch_analysis.token.clone());
        identity.edit = w.sketch_analysis.sequence;
        w.sketch_analysis
            .worker
            .submit(WorkerRequest::AnalyzeSketch {
                identity: identity.clone(),
                document: w.session.visible().clone(),
                owner: context.owner.clone(),
                sketch: Box::new(source.clone()),
            });
        w.sketch_analysis.pending = Some(identity);
        w.sketch_analysis.submitted = true;
    }
}
