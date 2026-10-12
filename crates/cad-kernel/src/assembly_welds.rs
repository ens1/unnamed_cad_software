//! Native assembly deposition resolves each configured source with its own provenance cache.
use crate::{
    adapter::{KernelAdapter, MonstertruckAdapter, Shape, TopologyHistory},
    evaluate::Evaluator,
};
use cad_core::{CadError, Document, NodeRef, Result, weld_beads::Definition};
use monstertruck::modeling::{Matrix4, Solid, builder};
use std::{collections::BTreeMap, path::Path};
fn pose(m: [[f64; 4]; 4]) -> Matrix4 {
    Matrix4::new(
        m[0][0], m[1][0], m[2][0], m[3][0], m[0][1], m[1][1], m[2][1], m[3][1], m[0][2], m[1][2],
        m[2][2], m[3][2], m[0][3], m[1][3], m[2][3], m[3][3],
    )
}
fn prepared(
    contexts: &BTreeMap<Vec<String>, (Document, [[f64; 4]; 4])>,
    path: &[String],
    root: &Path,
    tolerance: f64,
) -> Result<(Evaluator, Document, Matrix4)> {
    let (doc, transform) = contexts.get(path).ok_or_else(|| {
        CadError::Invalid(
            "Weld Bead: selected component is suppressed or missing in this configuration".into(),
        )
    })?;
    let mut doc = doc.clone();
    // The occurrence walker already applied its complete parameter/configuration scope.
    doc.active_configurations.clear();
    let mut evaluator = Evaluator::default();
    let solved = evaluator.build(&doc, root, tolerance)?;
    Ok((evaluator, solved, pose(*transform)))
}
pub(crate) fn build(
    adapter: &MonstertruckAdapter,
    doc: &Document,
    owner: &NodeRef,
    definition: &Definition,
    contexts: &BTreeMap<Vec<String>, (Document, [[f64; 4]; 4])>,
    root: &Path,
    tolerance: f64,
) -> Result<(
    cad_core::protocol::MeshData,
    Vec<Solid>,
    cad_core::weld_beads::Evaluated,
)> {
    // Reuse a native provenance cache when a seam and its support faces belong
    // to the same configured occurrence. Producer IDs never cross scopes.
    let mut scoped = BTreeMap::new();
    for path in std::iter::once(definition.seam.path())
        .chain(definition.supports.iter().map(|face| face.path.as_slice()))
    {
        if !scoped.contains_key(path) {
            scoped.insert(path.to_vec(), prepared(contexts, path, root, tolerance)?);
        }
    }
    let (source, solved, transform) = &scoped[definition.seam.path()];
    let seam = crate::weld_beads::seam(&source.adapter, solved, &definition.seam, &source.shapes)?
        .transformed(*transform);
    let mut supports = vec![];
    for face in &definition.supports {
        let (source, _, transform) = &scoped[&face.path];
        let (geometry, reference) =
            crate::weld_beads::support(&source.adapter, face, &source.shapes)?;
        supports.push((builder::transformed(&geometry, *transform), reference));
    }
    let mut history = TopologyHistory::default();
    let mut preview = None;
    let result = crate::weld_beads::build(
        doc,
        owner,
        definition,
        &seam,
        &supports,
        &mut history,
        &mut preview,
    );
    if let Some(preview) = &mut preview {
        preview.frame_label = "Owning assembly frame".into();
    }
    crate::weld_beads::record_preview(adapter, preview, result.as_ref().err());
    let built = result?;
    adapter.register_weld_history(owner, history);
    let mesh = adapter.mesh(&Shape::Bodies(built.solids.clone()), owner, tolerance)?;
    Ok((mesh, built.solids, built.metadata))
}
