//! Disposable presentation source identities. Session edits, undo/redo and worker
//! restarts change the stamp; display reads reuse the authoritative fingerprint.
use crate::Workspace;
use cad_core::{Id, protocol::RequestIdentity};
use serde_json::{Value, json};

#[derive(Clone, PartialEq, Eq)]
struct Stamp {
    document: Id,
    epoch: Id,
    generation: u64,
    revision: u64,
    draft: Option<(Id, u64)>,
    backend: String,
}
#[derive(Default)]
pub struct Cache {
    current: Option<(Stamp, RequestIdentity)>,
    checkpoint: Option<(Stamp, RequestIdentity)>,
    projection: Option<(Value, Vec<cad_core::presentation::SemanticElement>)>,
    source_json: Option<(RequestIdentity, Value)>,
}
pub fn identity(w: &Workspace) -> RequestIdentity {
    input_identity(w, None)
}

/// Selection can use the unedited body even while the draft is rebuilding.
/// Cache this identity independently so moving along an edge never re-hashes
/// the project just to alternate between the draft and its input.
pub fn input_identity(
    w: &Workspace,
    checkpoint: Option<&cad_core::transaction::Session>,
) -> RequestIdentity {
    let session = checkpoint.unwrap_or(&w.session);
    let stamp = Stamp {
        document: session.visible().id.clone(),
        epoch: session.source_epoch().into(),
        generation: session.generation,
        revision: session.committed.revision,
        draft: session
            .draft
            .as_ref()
            .map(|draft| (draft.id.clone(), draft.edit)),
        backend: session.backend.clone(),
    };
    let Ok(mut cache) = w.source_cache.lock() else {
        // A poisoned display cache must never weaken evaluation identity checks.
        return session.identity();
    };
    let slot = if checkpoint.is_some() {
        &mut cache.checkpoint
    } else {
        &mut cache.current
    };
    if let Some((current, identity)) = slot.as_ref()
        && current == &stamp
    {
        return identity.clone();
    }
    let identity = session.identity();
    *slot = Some((stamp, identity.clone()));
    identity
}

/// Cache only the session-owned portion of the semantic model. Native widget
/// focus, draft text, selection and all editor projections are composed after it.
pub fn project(w: &Workspace) -> Vec<cad_core::presentation::SemanticElement> {
    let identity = identity(w);
    let evaluation_stamp = |evaluation: Option<&cad_core::protocol::Evaluation>| {
        evaluation.map(|e| json!([e.identity, e.valid, e.geometry_valid, e.elapsed_ms]))
    };
    let key = json!({
        "identity": identity,
        "active": w.view.active_part,
        "evaluation": evaluation_stamp(w.session.evaluation.as_ref()),
        "draft": w.session.draft.as_ref().map(|draft| json!({
            "pending": draft.pending, "error": draft.error,
            "evaluation": evaluation_stamp(draft.evaluation.as_ref()),
            "fallback": evaluation_stamp(draft.last_valid_evaluation.as_ref()),
        })),
        "undo": w.session.can_undo(), "redo": w.session.can_redo(),
    });
    if let Ok(cache) = w.source_cache.lock()
        && let Some((current, elements)) = &cache.projection
        && current == &key
    {
        return elements.clone();
    }
    let elements = cad_core::presentation::project_with_identity(
        &w.session,
        w.view.active_part.as_deref(),
        &identity,
    );
    if let Ok(mut cache) = w.source_cache.lock() {
        cache.projection = Some((key, elements.clone()));
    }
    elements
}

/// Generic source fields share one serialization of the current source rather
/// than cloning the complete document for each field displayed by a theme.
pub fn value(w: &Workspace, pointer: &str) -> Option<Value> {
    let identity = identity(w);
    let mut cache = w.source_cache.lock().ok()?;
    if !cache
        .source_json
        .as_ref()
        .is_some_and(|(source, _)| source == &identity)
    {
        cache.source_json = Some((identity, serde_json::to_value(w.session.visible()).ok()?));
    }
    cache.source_json.as_ref()?.1.pointer(pointer).cloned()
}
