//! Bounded viewport-only work in a persistent worker. Never an Apply result.
use crate::evaluate::Evaluator;
use cad_core::{Result, display_mesh::*, protocol::*};
use std::collections::BTreeMap;
#[derive(Default)]
pub struct Service {
    evaluators: BTreeMap<(String, Option<String>, String), NativeCache>,
}
#[derive(Default)]
struct NativeCache {
    evaluator: Evaluator,
    source: String,
    modeling: Option<cad_core::tolerance::Modeling>,
    solved: Option<cad_core::Document>,
}
impl Service {
    pub fn mesh(&mut self, request: Request, backend: &str) -> Result<Output> {
        request.validate(backend)?;
        // Keep a bounded native cache for common configured occurrences. A cache
        // miss rebuilds only this isolated worker at the original model tolerance.
        if self.evaluators.len() > 8 {
            self.evaluators.clear();
        }
        let mut output = Output {
            version: VERSION,
            identity: request.identity.clone(),
            generation: request.generation,
            tolerance_mm: request.display_tolerance_mm,
            meshes: BTreeMap::new(),
            diagnostics: Vec::new(),
        };
        let mut triangles = 0usize;
        let mut vertices = 0usize;
        for target in &request.targets {
            let variant_key = (
                target.definition.clone(),
                target.configuration.clone(),
                request.project_root.clone(),
            );
            if !self.evaluators.contains_key(&variant_key) && self.evaluators.len() >= 8 {
                self.evaluators.clear();
            }
            let cache = self.evaluators.entry(variant_key).or_default();
            let result = (|| {
                if cache.source != request.identity.source
                    || cache.modeling != Some(request.modeling)
                    || cache.solved.is_none()
                {
                    let variant = cad_core::configuration::source_variant(
                        &request.document,
                        &target.definition,
                        target.configuration.as_deref(),
                    );
                    cache.solved = None;
                    cache.solved = Some(
                        cache
                            .evaluator
                            .build(&variant, std::path::Path::new(&request.project_root))?,
                    );
                    cache.source = request.identity.source.clone();
                    cache.modeling = Some(request.modeling);
                }
                let document = cache.solved.as_ref().ok_or_else(|| {
                    cad_core::CadError::Invalid("Display native cache is unavailable".into())
                })?;
                let mut part =
                    document
                        .parts
                        .get(&target.definition)
                        .cloned()
                        .ok_or_else(|| {
                            cad_core::CadError::Invalid("Display part is unavailable".into())
                        })?;
                part.outputs = target.outputs.clone();
                cache
                    .evaluator
                    .display_output(&part, request.display_tolerance_mm)
            })();
            match result {
                Ok(mesh) => {
                    triangles = triangles.saturating_add(mesh.indices.len() / 3);
                    vertices = vertices.saturating_add(mesh.positions.len());
                    if triangles > MAX_TRIANGLES || vertices > MAX_VERTICES {
                        output.diagnostics.push(Diagnostic { severity: "warning".into(), target: None,
                            message: "Viewport refinement reached its disposable mesh budget; original display detail is retained for remaining parts".into() });
                        break;
                    }
                    output.meshes.insert(target.key.clone(), mesh);
                }
                Err(error) => output.diagnostics.push(Diagnostic {
                    severity: "warning".into(),
                    target: Some(target.definition.clone()),
                    message: format!("Viewport refinement retained original detail: {error}"),
                }),
            }
        }
        Ok(output)
    }
}
