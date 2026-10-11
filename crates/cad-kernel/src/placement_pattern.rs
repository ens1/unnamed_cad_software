//! Exact rigid copies with authored placement identities and shared topology.
use crate::adapter::{MonstertruckAdapter, Shape, TopologyHistory};
use cad_core::{
    CadError, Document, NodeRef, Result, TopologyRef,
    placement_pattern::{Definition, MAX_PLACEMENTS},
};
use monstertruck::modeling::*;
use std::collections::{BTreeMap, HashMap, HashSet};
const MAX_TOPOLOGY: usize = 2_000_000;
fn invalid(message: impl ToString) -> CadError {
    CadError::Invalid(format!("Placement Pattern: {}", message.to_string()))
}
fn finite(point: Point3) -> bool {
    [point.x, point.y, point.z]
        .iter()
        .all(|value| value.is_finite())
}
fn source_key(reference: &TopologyRef) -> Result<String> {
    if reference.role.is_empty() {
        return Err(CadError::Invalid(
            "Body Pattern: A source boundary has no current construction identity".into(),
        ));
    }
    Ok(serde_json::to_string(&serde_json::json!([
        reference.producer,
        reference.topology,
        reference.role
    ]))?)
}
pub(crate) struct Stock {
    /// One absolute occurrence of each shared face. Mapping the aggregate shell
    /// retains shared edge/vertex objects across all reconstructed stock solids.
    shell: Shell,
    face_refs: HashMap<FaceId, TopologyRef>,
    edge_refs: HashMap<EdgeId, TopologyRef>,
    topology: usize,
}
impl Stock {
    pub(crate) fn new(
        adapter: &MonstertruckAdapter,
        body: &NodeRef,
        solids: &[Solid],
    ) -> Result<Self> {
        let invalid = |message: &str| CadError::Invalid(format!("Body Pattern: {message}"));
        let mut shell = Shell::new();
        let mut vertices = HashSet::new();
        let mut face_refs = HashMap::new();
        let mut edge_refs = HashMap::new();
        let mut face_keys = HashMap::new();
        let mut edge_keys = HashMap::new();
        for solid in solids {
            for face in solid.face_iter() {
                if face_refs.contains_key(&face.id()) {
                    continue;
                }
                let reference = adapter
                    .face_source(body, face.id())
                    .ok_or_else(|| invalid("A source face has no current construction ancestry"))?;
                if face_keys
                    .insert(source_key(&reference)?, face.id())
                    .is_some()
                {
                    return Err(invalid(
                        "Distinct source faces have ambiguous retained ancestry; repair the source feature before patterning",
                    ));
                }
                face_refs.insert(face.id(), reference);
                shell.push(face.absolute_clone());
                for edge in face.edge_iter() {
                    vertices.extend([edge.front().id(), edge.back().id()]);
                    if edge_refs.contains_key(&edge.id()) {
                        continue;
                    }
                    let reference = adapter.edge_source(body, edge.id()).ok_or_else(|| {
                        invalid("A source edge has no current construction ancestry")
                    })?;
                    if edge_keys
                        .insert(source_key(&reference)?, edge.id())
                        .is_some()
                    {
                        return Err(invalid(
                            "Distinct source edges have ambiguous retained ancestry; repair the source feature before patterning",
                        ));
                    }
                    edge_refs.insert(edge.id(), reference);
                }
            }
        }
        let topology = vertices
            .len()
            .checked_add(face_refs.len())
            .and_then(|n| n.checked_add(edge_refs.len()))
            .ok_or_else(|| invalid("Source topology exceeds the materialization budget"))?;
        if shell.is_empty() || topology > MAX_TOPOLOGY {
            return Err(invalid(
                "Select a nonempty source body set within two million native topology entities",
            ));
        }
        Ok(Self {
            shell,
            face_refs,
            edge_refs,
            topology,
        })
    }
    pub(crate) fn topology_size(&self) -> usize {
        self.topology
    }
    pub(crate) fn inherit(&self, generated: &mut TopologyHistory) {
        for (id, reference) in &self.face_refs {
            generated.inherit_face(*id, reference.clone());
        }
        for (id, reference) in &self.edge_refs {
            generated.inherit_edge(*id, reference.clone());
        }
    }
    pub(crate) fn copy(
        &self,
        solids: &[Solid],
        matrix: Matrix4,
        owner: &NodeRef,
        purpose: &serde_json::Value,
        additional_sources: &[TopologyRef],
        generated: &mut TopologyHistory,
    ) -> Result<Vec<Solid>> {
        let invalid = |message: &str| CadError::Invalid(format!("Body Pattern: {message}"));
        for vertex in self.shell.face_iter().flat_map(Face::vertex_iter) {
            if !finite(matrix.transform_point(vertex.point())) {
                return Err(invalid("A placement overflows finite document coordinates"));
            }
        }
        let transformed = builder::transformed(&self.shell, matrix);
        if transformed.len() != self.shell.len() {
            return Err(invalid("A rigid copy changed the source face topology"));
        }
        let mut face_map = HashMap::new();
        let mut edge_map = HashMap::new();
        for (source, target) in self.shell.face_iter().zip(transformed.face_iter()) {
            let sources = std::iter::once(self.face_refs[&source.id()].clone())
                .chain(additional_sources.iter().cloned())
                .collect::<Vec<_>>();
            generated.tool_face(owner, target, &sources, purpose)?;
            face_map.insert(source.id(), target.clone());
            let source_edges = source.edge_iter().collect::<Vec<_>>();
            let target_edges = target.edge_iter().collect::<Vec<_>>();
            if source_edges.len() != target_edges.len() {
                return Err(invalid("A rigid copy changed the source edge topology"));
            }
            for (source, target) in source_edges.into_iter().zip(target_edges) {
                if let Some(previous) = edge_map.insert(source.id(), target.id()) {
                    if previous != target.id() {
                        return Err(invalid("The copy failed to retain a shared source edge"));
                    }
                } else {
                    let sources = std::iter::once(self.edge_refs[&source.id()].clone())
                        .chain(additional_sources.iter().cloned())
                        .collect::<Vec<_>>();
                    generated.tool_edge(owner, &target, &sources, purpose)?;
                }
            }
        }
        solids
            .iter()
            .map(|source| {
                let boundaries = source
                    .boundaries()
                    .iter()
                    .map(|shell| {
                        shell
                            .face_iter()
                            .map(|face| {
                                let mapped = face_map.get(&face.id()).ok_or_else(|| {
                                    invalid("A source shell face was lost while copying")
                                })?;
                                Ok(if face.orientation() {
                                    mapped.clone()
                                } else {
                                    mapped.inverse()
                                })
                            })
                            .collect::<Result<Shell>>()
                    })
                    .collect::<Result<Vec<_>>>()?;
                let solid = Solid::try_new(boundaries).map_err(|error| {
                    invalid(&format!(
                        "The exact placement is not an oriented solid: {error}"
                    ))
                })?;
                crate::validation::solid(&solid)?;
                Ok(solid)
            })
            .collect()
    }
}
fn matrix_key(matrix: Matrix4) -> [u64; 16] {
    let values = [
        matrix.x.x, matrix.x.y, matrix.x.z, matrix.x.w, matrix.y.x, matrix.y.y, matrix.y.z,
        matrix.y.w, matrix.z.x, matrix.z.y, matrix.z.z, matrix.z.w, matrix.w.x, matrix.w.y,
        matrix.w.z, matrix.w.w,
    ];
    values.map(|value| if value == 0. { 0 } else { value.to_bits() })
}
struct Placement<'a> {
    id: &'a str,
    matrix: Matrix4,
    identity: bool,
}
pub(crate) fn operation(
    adapter: &MonstertruckAdapter,
    doc: &Document,
    owner: &NodeRef,
    definition: &Definition,
    inputs: &BTreeMap<NodeRef, Shape>,
    _tolerance: f64,
    generated: &mut TopologyHistory,
) -> Result<Shape> {
    definition.validate()?;
    let Some(Shape::Bodies(solids)) = inputs.get(&definition.body) else {
        return Err(invalid(
            "Select a current unsuppressed native solid body output",
        ));
    };
    if solids.is_empty() {
        return Err(invalid("The source body output is empty"));
    }
    let stock = Stock::new(adapter, &definition.body, solids)?;
    let seed = Point3::from(adapter.resolve_surface_point(
        doc,
        &owner.definition,
        &definition.seed,
        inputs,
    )?);
    let resolved_axis = adapter.resolve_surface_axis(doc, &definition.axis, inputs)?;
    let axis = Vector3::from(resolved_axis.direction);
    let magnitude = axis.magnitude();
    if !finite(seed) || !magnitude.is_finite() || magnitude <= 1e-12 {
        return Err(invalid(
            "Seed point and rotation axis must be finite and the axis must have nonzero direction",
        ));
    }
    let axis = axis / magnitude;
    let mut placements = Vec::new();
    let mut transforms = HashSet::new();
    let mut reused = false;
    for record in definition
        .placements
        .iter()
        .filter(|record| !record.skipped)
    {
        let point = Point3::from(adapter.resolve_surface_point(
            doc,
            &owner.definition,
            &record.target,
            inputs,
        )?);
        if !finite(point) {
            return Err(invalid("A current placement point is nonfinite"));
        }
        let angle = definition.angle(doc, &owner.definition, record)?;
        let rotation = if angle == 0. {
            Matrix3::identity()
        } else {
            Matrix3::from_axis_angle(axis, Rad(angle))
        };
        let translation = point.to_vec() - rotation * seed.to_vec();
        let matrix = Matrix4::from_cols(
            rotation.x.extend(0.),
            rotation.y.extend(0.),
            rotation.z.extend(0.),
            translation.extend(1.),
        );
        if [matrix.x, matrix.y, matrix.z, matrix.w]
            .iter()
            .any(|column| {
                ![column.x, column.y, column.z, column.w]
                    .iter()
                    .all(|value| value.is_finite())
            })
            || !rotation.determinant().is_finite()
            || (rotation.determinant() - 1.).abs() > 1e-9
        {
            return Err(invalid(
                "The requested placement is not a finite proper rigid transform",
            ));
        }
        let identity = angle == 0. && point == seed;
        if !transforms.insert(matrix_key(matrix)) {
            return Err(invalid(
                "Two active placement records request the same exact rigid transform; skip or move one of them",
            ));
        }
        if identity && definition.keep_source {
            reused = true;
        }
        placements.push(Placement {
            id: &record.id,
            matrix,
            identity,
        });
    }
    let copies = placements.len() - usize::from(reused);
    let positions = copies + usize::from(definition.keep_source);
    if solids
        .len()
        .checked_mul(positions)
        .is_none_or(|n| n > MAX_PLACEMENTS)
        || stock
            .topology
            .checked_mul(positions)
            .is_none_or(|n| n > MAX_TOPOLOGY)
    {
        return Err(CadError::Unsupported("Placement Pattern exceeds 10000 materialized solids or two million copied topology entities; reduce active records or source complexity".into()));
    }
    let mut output = Vec::new();
    if definition.keep_source {
        stock.inherit(generated);
        output.extend(solids.iter().cloned());
    }
    for placement in placements {
        if placement.identity && definition.keep_source {
            continue;
        }
        let purpose = serde_json::json!({"operation":"placement_pattern","placement":placement.id});
        output.extend(stock.copy(solids, placement.matrix, owner, &purpose, &[], generated)?);
    }
    if output.is_empty() {
        return Err(invalid(
            "The current placement pattern has no retained or active bodies",
        ));
    }
    Ok(Shape::Bodies(output))
}
