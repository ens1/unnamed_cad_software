//! Direct editing by exact supporting-plane reconstruction with source ancestry.
use crate::{
    adapter::{MonstertruckAdapter, Shape, TopologyHistory},
    polyhedron::{self, FaceEdit},
    shell_history::ShellHistory,
};
use cad_core::{
    CadError, Dimension, Document, NodeRef, Result,
    direct_edit::{Definition, Method},
    expression,
};
use monstertruck::modeling::*;
use std::collections::{BTreeMap, HashSet};

fn invalid(message: &str) -> CadError {
    CadError::Invalid(format!("Direct Edit: {message}"))
}
pub(crate) fn operation(
    adapter: &MonstertruckAdapter,
    doc: &Document,
    owner: &NodeRef,
    definition: &Definition,
    inputs: &BTreeMap<NodeRef, Shape>,
    generated: &mut TopologyHistory,
) -> Result<Shape> {
    if definition.faces.is_empty() || definition.faces.len() > 256 {
        return Err(invalid("Select 1..256 explicit model faces"));
    }
    let Some(Shape::Bodies(bodies)) = inputs.get(&definition.body) else {
        return Err(invalid("Select a solid body output"));
    };
    let mut selected = HashSet::new();
    for reference in &definition.faces {
        let face = adapter.selected_boundary_face(&definition.body, reference, inputs)?;
        if !selected.insert(face.id()) {
            return Err(invalid("Selected faces must be unique"));
        }
    }
    let edit = match &definition.method {
        Method::Offset { distance } => FaceEdit::Offset(expression::value(
            doc,
            &owner.definition,
            distance,
            Dimension::Length,
        )?),
        Method::Move {
            translation,
            axis,
            angle,
        } => {
            let mut delta = [0.; 3];
            for (i, value) in translation.iter().enumerate() {
                delta[i] = expression::value(doc, &owner.definition, value, Dimension::Length)?;
            }
            let angle = expression::value(doc, &owner.definition, angle, Dimension::Angle)?;
            let axis = adapter.resolve_surface_axis(doc, axis, inputs)?;
            let center = Vector3::from(axis.origin);
            let rotation = Matrix4::from_axis_angle(Vector3::from(axis.direction), Rad(angle));
            let transform = Matrix4::from_translation(Vector3::from(delta))
                * Matrix4::from_translation(center)
                * rotation
                * Matrix4::from_translation(-center);
            if !AsRef::<[[f64; 4]; 4]>::as_ref(&transform)
                .iter()
                .flatten()
                .all(|v| v.is_finite())
            {
                return Err(invalid("Face movement must be finite"));
            }
            FaceEdit::Move(transform)
        }
        Method::DeleteHeal => FaceEdit::DeleteHeal,
        Method::Replace { plane, reverse } => {
            FaceEdit::Replace(adapter.resolve_surface_plane(doc, plane, inputs)?, *reverse)
        }
    };
    let mut output = Vec::new();
    let mut budget = polyhedron::DirectEditBudget::new();
    for body in bodies {
        let faces = body
            .face_iter()
            .filter(|face| selected.contains(&face.id()))
            .map(|face| face.id())
            .collect::<Vec<_>>();
        if faces.is_empty() {
            output.push(body.clone());
            continue;
        }
        budget.admit(body)?;
        let mut history =
            ShellHistory::new_operation(adapter, owner, &definition.body, body, "direct_edit")?;
        let result = polyhedron::edit_faces(body, &faces, &edit, &mut history, &mut budget)?;
        crate::validation::solid(&result)?;
        generated.merge(history.finish(std::slice::from_ref(&result))?);
        output.push(result);
    }
    Ok(Shape::Bodies(output))
}
