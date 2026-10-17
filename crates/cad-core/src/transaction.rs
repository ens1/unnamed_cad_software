use crate::{
    CadError, Document, Id, Result,
    protocol::{Evaluation, PROTOCOL_VERSION, RequestIdentity},
};
#[derive(Clone)]
pub struct Draft {
    pub id: Id,
    pub base_revision: u64,
    pub edit: u64,
    pub document: Document,
    pub evaluation: Option<Evaluation>,
    /// Display-only fallback during rebuilds; never eligible for Apply.
    pub last_valid_evaluation: Option<Evaluation>,
    pub pending: bool,
    pub error: Option<String>,
}
impl Draft {
    pub fn preview_evaluation(&self) -> Option<&Evaluation> {
        self.evaluation
            .as_ref()
            .filter(|e| e.valid)
            .or(self.last_valid_evaluation.as_ref())
    }
}
#[derive(Clone)]
pub struct Session {
    pub committed: Document,
    pub evaluation: Option<Evaluation>,
    pub draft: Option<Draft>,
    undo: Vec<Document>,
    redo: Vec<Document>,
    /// Disposable source identity for presentation caches. Clones retain their
    /// snapshot identity; every new edit gets a fresh token, including edits
    /// made after restoring an earlier checkpoint with the same draft counter.
    source_epoch: Id,
    pub generation: u64,
    pub backend: String,
    /// Set only by a successful metadata-only command over a current validated
    /// evaluation. The desktop can avoid scheduling an unnecessary worker job.
    pub last_dispatch_reused_evaluation: bool,
}
impl Session {
    pub fn new(document: Document, backend: &str) -> Self {
        Self {
            committed: document,
            evaluation: None,
            draft: None,
            undo: vec![],
            redo: vec![],
            source_epoch: crate::id(),
            generation: 1,
            backend: backend.into(),
            last_dispatch_reused_evaluation: false,
        }
    }
    pub fn visible(&self) -> &Document {
        self.draft
            .as_ref()
            .map(|d| &d.document)
            .unwrap_or(&self.committed)
    }
    pub fn source_epoch(&self) -> &str {
        &self.source_epoch
    }
    pub fn edit(&mut self, change: impl FnOnce(&mut Document) -> Result<()>) -> Result<()> {
        self.last_dispatch_reused_evaluation = false;
        let mut d = self.draft.clone().unwrap_or(Draft {
            id: crate::id(),
            base_revision: self.committed.revision,
            edit: 0,
            document: self.committed.clone(),
            evaluation: None,
            last_valid_evaluation: None,
            pending: false,
            error: None,
        });
        change(&mut d.document)?;
        d.document.validate()?;
        d.edit += 1;
        d.pending = true;
        if let Some(evaluation) = d.evaluation.take().filter(|e| e.valid) {
            d.last_valid_evaluation = Some(evaluation);
        }
        d.error = None;
        self.draft = Some(d);
        self.source_epoch = crate::id();
        Ok(())
    }
    pub fn identity(&self) -> RequestIdentity {
        if let Some(d) = &self.draft {
            RequestIdentity {
                protocol: PROTOCOL_VERSION,
                generation: self.generation,
                base_revision: d.base_revision,
                draft: Some(d.id.clone()),
                edit: d.edit,
                source: d.document.fingerprint(),
                backend: self.backend.clone(),
            }
        } else {
            RequestIdentity {
                protocol: PROTOCOL_VERSION,
                generation: self.generation,
                base_revision: self.committed.revision,
                draft: None,
                edit: 0,
                source: self.committed.fingerprint(),
                backend: self.backend.clone(),
            }
        }
    }
    pub fn accept(&mut self, result: Evaluation) -> bool {
        if result.identity != self.identity() {
            return false;
        }
        if let Some(d) = &mut self.draft {
            d.pending = false;
            d.error = if result.valid {
                d.last_valid_evaluation = Some(result.clone());
                None
            } else {
                Some(
                    result
                        .diagnostics
                        .iter()
                        .map(|d| d.message.as_str())
                        .collect::<Vec<_>>()
                        .join("\n"),
                )
            };
            d.evaluation = Some(result);
        } else {
            self.evaluation = Some(result);
        }
        true
    }
    pub fn fail(&mut self, identity: &RequestIdentity, message: String) {
        if identity == &self.identity()
            && let Some(d) = &mut self.draft
        {
            d.pending = false;
            d.error = Some(message);
        }
    }
    pub fn apply(&mut self) -> Result<()> {
        let d = self
            .draft
            .as_ref()
            .ok_or_else(|| CadError::Invalid("No pending edit".into()))?;
        if d.base_revision != self.committed.revision {
            return Err(CadError::Invalid(
                "Committed revision changed; cancel or rebase the draft".into(),
            ));
        }
        let evaluation = d
            .evaluation
            .as_ref()
            .filter(|e| e.valid && e.identity == self.identity())
            .ok_or_else(|| {
                CadError::Invalid("Apply requires the current validated preview".into())
            })?
            .clone();
        let mut doc = d.document.clone();
        doc.revision = self.committed.revision + 1;
        self.undo.push(self.committed.clone());
        self.redo.clear();
        self.committed = doc;
        let mut e = evaluation;
        self.draft = None;
        self.source_epoch = crate::id();
        e.identity = self.identity();
        self.evaluation = Some(e);
        Ok(())
    }
    pub fn cancel(&mut self) {
        self.draft = None;
        self.source_epoch = crate::id();
    }
    pub fn undo(&mut self) -> bool {
        if let Some(previous) = self.undo.pop() {
            self.draft = None;
            self.redo.push(self.committed.clone());
            let next = self.committed.revision + 1;
            self.committed = previous;
            self.committed.revision = next;
            self.evaluation = None;
            self.source_epoch = crate::id();
            true
        } else {
            false
        }
    }
    pub fn redo(&mut self) -> bool {
        if let Some(next) = self.redo.pop() {
            self.draft = None;
            self.undo.push(self.committed.clone());
            let revision = self.committed.revision + 1;
            self.committed = next;
            self.committed.revision = revision;
            self.evaluation = None;
            self.source_epoch = crate::id();
            true
        } else {
            false
        }
    }
    pub fn restart_worker(&mut self) {
        self.generation += 1;
        self.evaluation = None;
        if let Some(d) = &mut self.draft {
            d.evaluation = None;
            d.last_valid_evaluation = None;
            d.pending = true;
        }
    }
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
}
