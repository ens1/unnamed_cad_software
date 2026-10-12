//! Exact whole-solid selection and native source topology preservation.
use crate::adapter::{MonstertruckAdapter, Shape, TopologyHistory};
use cad_core::{
    CadError, NodeRef, Result, TopologyRef,
    body_filter::{Definition, Mode},
};
use monstertruck::modeling::FaceId;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

fn same_reference(a: &TopologyRef, b: &TopologyRef) -> bool {
    a.producer == b.producer && a.role == b.role && a.topology == b.topology
}

pub(crate) fn operation(
    adapter: &MonstertruckAdapter,
    _owner: &NodeRef,
    definition: &Definition,
    inputs: &BTreeMap<NodeRef, Shape>,
    generated: &mut TopologyHistory,
) -> Result<Shape> {
    definition.validate()?;
    let mut anchors: BTreeMap<(NodeRef, String), BTreeSet<usize>> = BTreeMap::new();
    // An ancestor, suppressed modifier, or earlier filter can alias the same
    // exact native solid through another scoped output. Reject that overlap
    // before selection: filtering one alias cannot retain or double another.
    // These transient IDs prove shared objects only; they are never persisted.
    let mut solid_keys: Vec<HashSet<(FaceId, bool)>> = Vec::new();
    let mut face_owners: HashMap<FaceId, Vec<usize>> = HashMap::new();
    for source in &definition.scope {
        let Some(Shape::Bodies(bodies)) = inputs.get(source) else {
            return Err(CadError::Invalid(
                "Keep/Delete Bodies scope requires evaluated solid outputs; surfaces, sketches and reference curves are unsupported".into(),
            ));
        };
        for (index, body) in bodies.iter().enumerate() {
            let key = body
                .face_iter()
                .map(|face| (face.id(), face.orientation()))
                .collect::<HashSet<_>>();
            if body.face_iter().next().is_some_and(|face| {
                face_owners
                    .get(&face.id())
                    .is_some_and(|previous| previous.iter().any(|index| solid_keys[*index] == key))
            }) {
                return Err(CadError::Invalid("Keep/Delete Bodies: Scoped outputs contain the same exact native solid more than once; remove an overlapping source output or repair a repeated source body set".into()));
            }
            let key_index = solid_keys.len();
            for (face, _) in &key {
                face_owners.entry(*face).or_default().push(key_index);
            }
            solid_keys.push(key);
            for face in body.face_iter() {
                if let Some(reference) = adapter.face_source(source, face.id())
                    && reference.topology == "face"
                {
                    anchors
                        .entry((
                            source.clone(),
                            cad_core::provenance::canonical_key(&reference)?,
                        ))
                        .or_default()
                        .insert(index);
                }
            }
        }
    }
    let mut selected = BTreeSet::new();
    for anchor in &definition.selected {
        let Some(matches) = anchors.get(&(
            anchor.body.clone(),
            cad_core::provenance::canonical_key(&anchor.face)?,
        )) else {
            return Err(CadError::Invalid(
                "Keep/Delete Bodies: A saved face anchor no longer belongs to a solid in its owning output; select an explicit replacement".into(),
            ));
        };
        if matches.len() != 1 {
            return Err(CadError::Invalid(
                "Keep/Delete Bodies: A saved face anchor identifies several solids in its owning output; select an unambiguous construction face".into(),
            ));
        }
        if let Some(index) = matches.first() {
            selected.insert((anchor.body.clone(), *index));
        }
    }
    let mut survivors = Vec::new();
    for source in &definition.scope {
        let Some(Shape::Bodies(bodies)) = inputs.get(source) else {
            return Err(CadError::Invalid(
                "A scoped solid output is unavailable".into(),
            ));
        };
        for (index, body) in bodies.iter().enumerate() {
            let picked = selected.contains(&(source.clone(), index));
            if picked != matches!(definition.mode, Mode::KeepSelected) {
                continue;
            }
            for face in body.face_iter() {
                let reference = adapter.face_source(source, face.id()).ok_or_else(|| {
                    CadError::Unsupported(
                        "Keep/Delete Bodies: A surviving face has no exact source ancestry".into(),
                    )
                })?;
                if generated
                    .face_source(face.id())
                    .is_some_and(|previous| !same_reference(&previous, &reference))
                {
                    return Err(CadError::Unsupported(
                        "Keep/Delete Bodies: Scoped outputs share native faces with conflicting source ancestry".into(),
                    ));
                }
                generated.inherit_face(face.id(), reference);
            }
            for edge in body.edge_iter() {
                let reference = adapter.edge_source(source, edge.id()).ok_or_else(|| {
                    CadError::Unsupported(
                        "Keep/Delete Bodies: A surviving edge has no exact source ancestry".into(),
                    )
                })?;
                if generated
                    .edge_source(edge.id())
                    .is_some_and(|previous| !same_reference(&previous, &reference))
                {
                    return Err(CadError::Unsupported(
                        "Keep/Delete Bodies: Scoped outputs share native edges with conflicting source ancestry".into(),
                    ));
                }
                generated.inherit_edge(edge.id(), reference);
            }
            survivors.push(body.clone());
        }
    }
    Ok(Shape::Bodies(survivors))
}
