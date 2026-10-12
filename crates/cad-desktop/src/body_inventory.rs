//! Native body inventory for presentation; never inferred from triangles or
//! shell tokens. Missing legacy cache data is described as output features.
use crate::{Workspace, viewport};
use cad_core::{
    presentation::SemanticElement,
    protocol::{BODY_COUNTS_VERSION, BodyCounts},
};

pub fn counts(w: &Workspace, definition: &str) -> Option<BodyCounts> {
    let evaluation = viewport::display_evaluation(w)?;
    let instances = viewport::display_instances(w, evaluation);
    let instance = instances
        .iter()
        .copied()
        .find(|instance| instance.definition == definition && viewport::active_context(w, instance))
        .or_else(|| {
            instances
                .iter()
                .copied()
                .find(|instance| instance.definition == definition)
        })?;
    let mut result = BodyCounts {
        version: BODY_COUNTS_VERSION,
        solids: 0,
        surfaces: 0,
    };
    for mesh in viewport::display_meshes(w, evaluation, instance) {
        let count = mesh
            .body_counts
            .filter(|count| count.version == BODY_COUNTS_VERSION)?;
        result.solids = result.solids.checked_add(count.solids)?;
        result.surfaces = result.surfaces.checked_add(count.surfaces)?;
    }
    Some(result)
}

pub fn semantics(w: &Workspace) -> Vec<SemanticElement> {
    let Some(definition) = w.view.active_part.as_deref() else {
        return vec![];
    };
    let count = counts(w, definition);
    vec![SemanticElement {
        id: format!("{definition}::body-inventory"),
        role: "body-inventory".into(),
        label: "Evaluated bodies".into(),
        value: count.map(|count| {
            format!(
                "{} solid bodies · {} surface bodies",
                count.solids, count.surfaces
            )
        }),
        state: if count.is_some() {
            "available"
        } else {
            "pending"
        }
        .into(),
        commands: vec![],
        relationships: vec![definition.into()],
        data: serde_json::json!({"contract":"tcad.body-inventory","version":BODY_COUNTS_VERSION,"definition":definition,
            "counts":count,"source":"native_body_containers","tree_grouping":"output_feature","geometry_changes":false,
            "evaluation_identity":viewport::display_evaluation(w).map(|evaluation|&evaluation.identity)}),
    }]
}
