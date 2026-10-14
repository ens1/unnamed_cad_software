use cad_core::{
    Assembly, CadError, Document, Feature, Id, Instance, Operation, Part, Placement, Result,
};
use monstertruck::{
    assembly::assy::{Assembly as KernelAssembly, EdgeEntity, NodeEntity},
    modeling::{Matrix4, Shell, Solid},
    step::{
        common::PartAttributes,
        save::{
            CompleteStepDisplay, StepDesign, StepHeaderDescriptor, StepMeasurementContext,
            StepModel,
        },
    },
    topology::compress::CompressedTrimmedSolid,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};
fn attrs(id: &str, name: &str) -> PartAttributes {
    PartAttributes {
        id: id.replace('\'', "''"),
        name: name.replace('\'', "''"),
        description: String::new(),
        shape_representation: None,
    }
}

fn format_step(value: impl std::fmt::Display) -> Result<String> {
    let mut content = String::new();
    std::fmt::write(&mut content, format_args!("{value}")).map_err(|_| {
        CadError::Unsupported(
            "STEP serialization cannot faithfully encode a native carrier or boundary in this model".into(),
        )
    })?;
    Ok(content)
}

fn solid_step(body: &Solid, linear_mm: f64) -> Result<StepBody> {
    let compressed = CompressedTrimmedSolid {
        boundaries: body
            .boundaries()
            .iter()
            .map(|shell| crate::native_trims::exchange_shell(shell, linear_mm))
            .collect::<Result<Vec<_>>>()?,
        id_allocator: Some(body.id_allocator().clone()),
        attributes: Some(body.attributes().clone()),
    };
    StepBody::new(format_step(StepModel::from(&compressed))?)
}

fn surface_step(shell: &Shell, linear_mm: f64) -> Result<StepBody> {
    let compressed = crate::native_trims::exchange_shell(shell, linear_mm)?;
    StepBody::new(format_step(StepModel::from(&compressed))?)
}

pub fn export_step(
    doc: &Document,
    evaluated: &BTreeMap<Id, cad_core::assembly_geometry::EvaluatedAssembly>,
    bodies: &BTreeMap<String, Vec<Solid>>,
    surfaces: &BTreeMap<String, Vec<Shell>>,
    assembly_welds: &BTreeMap<String, cad_core::assembly_welds::Evaluated>,
    path: &Path,
) -> Result<()> {
    doc.settings.modeling.validate()?;
    struct Traversal<'a> {
        doc: &'a Document,
        evaluated: &'a BTreeMap<Id, cad_core::assembly_geometry::EvaluatedAssembly>,
        bodies: &'a BTreeMap<String, Vec<Solid>>,
        surfaces: &'a BTreeMap<String, Vec<Shell>>,
        assembly_welds: &'a BTreeMap<String, cad_core::assembly_welds::Evaluated>,
        graph: KernelAssembly<Vec<StepBody>, PartAttributes, Matrix4, PartAttributes>,
        active: BTreeSet<Id>,
        nodes: BTreeMap<Id, monstertruck::assembly::dag::NodeIndex>,
    }
    impl Traversal<'_> {
        fn visit(
            &mut self,
            context: &Document,
            definition: &str,
            configuration: Option<&str>,
        ) -> Result<monstertruck::assembly::dag::NodeIndex> {
            let key = cad_core::configuration::key(definition, configuration);
            if let Some(node) = self.nodes.get(&key) {
                return Ok(*node);
            }
            if !self.active.insert(definition.into()) {
                return Err(CadError::Invalid(
                    "Cannot serialize cyclic configured STEP assembly".into(),
                ));
            }
            let scoped =
                cad_core::configuration::scope(self.doc, context, definition, configuration)?;
            let name = scoped
                .parts
                .get(definition)
                .map(|p| p.name.clone())
                .or_else(|| scoped.assemblies.get(definition).map(|a| a.name.clone()))
                .ok_or_else(|| CadError::Invalid("STEP definition is missing".into()))?;
            let name = if let Some(configuration) = configuration {
                format!(
                    "{name} · {}",
                    cad_core::configuration::resolve(self.doc, definition, configuration)?.name
                )
            } else {
                name
            };
            let shape =
                if scoped.parts.contains_key(definition) {
                    self.bodies
                        .get(&key)
                        .into_iter()
                        .flatten()
                        .map(|body| solid_step(body, self.doc.settings.modeling.linear_mm))
                        .chain(
                            self.surfaces.get(&key).into_iter().flatten().map(|shell| {
                                surface_step(shell, self.doc.settings.modeling.linear_mm)
                            }),
                        )
                        .collect::<Result<Vec<_>>>()?
                } else {
                    Vec::new()
                };
            let node = self.graph.create_node(NodeEntity {
                shape,
                attrs: attrs(&key, &name),
            });
            self.nodes.insert(key.clone(), node);
            if let Some(assembly) = scoped.assemblies.get(definition) {
                let evaluated = self.evaluated;
                let placements = evaluated.get(&key).ok_or_else(|| {
                    CadError::Invalid(
                        "Current configured assembly placements are unavailable for STEP export"
                            .into(),
                    )
                })?;
                for instance in assembly.instances.values() {
                    let configuration = cad_core::configuration::selected(
                        &scoped,
                        &instance.definition,
                        instance.configuration.as_ref(),
                    );
                    let child = self.visit(
                        &scoped,
                        &instance.definition,
                        configuration.map(String::as_str),
                    )?;
                    let m = placements
                        .placements
                        .get(&instance.id)
                        .ok_or_else(|| {
                            CadError::Invalid(
                                "Configured STEP component placement is missing".into(),
                            )
                        })?
                        .matrix();
                    let matrix = crate::adapter::matrix(std::array::from_fn(|r| {
                        std::array::from_fn(|c| m[(r, c)])
                    }));
                    if !self.graph.create_edge(
                        node,
                        child,
                        EdgeEntity {
                            matrix,
                            attrs: attrs(&instance.id, &instance.name),
                        },
                    ) {
                        return Err(CadError::Invalid(
                            "Cannot serialize cyclic STEP assembly".into(),
                        ));
                    }
                }
                for weld in assembly.weld_beads.values().filter(|weld| !weld.suppressed) {
                    let evaluated=self.assembly_welds.values().find(|value|value.owner.definition==definition&&value.owner.node==weld.id&&value.placement.configuration.as_deref()==configuration).ok_or_else(||CadError::Invalid("Current configured assembly weld geometry is unavailable for STEP export".into()))?;
                    let geometry =
                        self.bodies
                            .get(&evaluated.placement.mesh_key)
                            .ok_or_else(|| {
                                CadError::Invalid(
                                    "Deposited metal B-rep is unavailable for STEP export".into(),
                                )
                            })?;
                    let shape = geometry
                        .iter()
                        .map(|body| solid_step(body, self.doc.settings.modeling.linear_mm))
                        .collect::<Result<Vec<_>>>()?;
                    let weld_key = format!("assembly-weld:{key}::{}", weld.id);
                    let child = if let Some(child) = self.nodes.get(&weld_key) {
                        *child
                    } else {
                        let child = self.graph.create_node(NodeEntity {
                            shape,
                            attrs: attrs(&weld_key, &weld.name),
                        });
                        self.nodes.insert(weld_key.clone(), child);
                        child
                    };
                    if !self.graph.create_edge(
                        node,
                        child,
                        EdgeEntity {
                            matrix: Matrix4::from_scale(1.),
                            attrs: attrs(&weld.id, &weld.name),
                        },
                    ) {
                        return Err(CadError::Invalid(
                            "Cannot serialize deposited metal STEP structure".into(),
                        ));
                    }
                }
            }
            self.active.remove(definition);
            Ok(node)
        }
    }
    let context = cad_core::configuration::effective(doc)?;
    let mut traversal = Traversal {
        doc,
        evaluated,
        bodies,
        surfaces,
        assembly_welds,
        graph: KernelAssembly::new(),
        active: BTreeSet::new(),
        nodes: BTreeMap::new(),
    };
    traversal.visit(
        &context,
        &doc.root,
        doc.active_configurations.get(&doc.root).map(String::as_str),
    )?;
    let content = format_step(CompleteStepDisplay::new(
        StepDesign::<StepBody, Vec<StepBody>, Matrix4>::new(traversal.graph)
            .with_measurement_context(StepMeasurementContext {
                distance_accuracy_value: doc.settings.modeling.linear_mm,
                ..Default::default()
            }),
        StepHeaderDescriptor {
            organization_system: "TCAD / Monstertruck 0.4.1".into(),
            ..Default::default()
        },
    ))?;
    cad_core::persistence::atomic_write(path, content.as_bytes())
}
pub fn import_structure(doc: &mut Document, resource: &str, root: &Path) -> Result<Id> {
    let r = doc
        .resources
        .get(resource)
        .ok_or_else(|| CadError::Invalid("Missing STEP resource".into()))?;
    let source = std::fs::read_to_string(cad_core::persistence::safe_join(root, &r.path)?)?;
    let length_scale = crate::units::length_scale(&source)?;
    let table = crate::step_import::load_table(&source)?;
    for (name, tally) in table
        .entity_report
        .refused()
        .chain(table.entity_report.shape_unexpected())
    {
        // These application/product description contexts carry no geometry,
        // units or occurrence transform. The pinned resource still preserves
        // their source records; metadata parity is not claimed.
        let descriptive_only = name.split('+').all(|name| {
            matches!(
                name,
                "APPLICATION_CONTEXT"
                    | "PRODUCT_CONTEXT"
                    | "PRODUCT_DEFINITION_CONTEXT"
                    | "ORGANIZATION"
                    | "ORGANIZATION_RELATIONSHIP"
                    | "PERSON"
                    | "PERSON_AND_ORGANIZATION"
                    | "ADDRESS"
                    | "ORGANIZATIONAL_ADDRESS"
                    | "PERSONAL_ADDRESS"
                    | "COLOUR_RGB"
                    | "PRESENTATION_STYLE_ASSIGNMENT"
                    | "STYLED_ITEM"
            )
        });
        if !descriptive_only {
            return Err(CadError::Invalid(format!(
                "STEP entity loading lost {name} records ({}; first entity {:?})",
                tally.count, tally.first_id
            )));
        }
    }
    let graph = table
        .step_assy()
        .map_err(|e| CadError::Invalid(format!("STEP product structure: {e}")))?;
    if graph.is_empty() {
        let (solids, surfaces) = crate::step_import::unstructured_items(&table, &source)?;
        if solids.is_empty() && surfaces.is_empty() {
            return Err(CadError::Unsupported(
                "STEP contains no supported solid or shell-based surface model".into(),
            ));
        }
        let part = imported_part("Imported STEP", resource, solids, surfaces);
        let id = part.id.clone();
        doc.parts.insert(id.clone(), part);
        return Ok(id);
    }
    let mut definitions = BTreeMap::new();
    for node in graph.all_nodes() {
        let index = node.index();
        let key = cad_core::id();
        definitions.insert(index, key.clone());
        let (solids, surfaces) = crate::step_import::items(
            &table,
            node.shape(),
            table.shape_representation_of_node(node.entity()),
        )?;
        let has_geometry = !solids.is_empty() || !surfaces.is_empty();
        let name = if node.attributes().name.is_empty() {
            "Imported component"
        } else {
            &node.attributes().name
        };
        if node.edges().next().is_none() && has_geometry {
            let mut part = imported_part(name, resource, solids, surfaces);
            part.id = key.clone();
            part.properties
                .insert("part_number".into(), node.attributes().id.clone());
            doc.parts.insert(key, part);
        } else {
            let mut assembly = Assembly {
                id: key.clone(),
                name: name.into(),
                properties: BTreeMap::new(),
                instances: BTreeMap::new(),
                mates: vec![],
                promoted_parameters: BTreeMap::new(),
                configurations: BTreeMap::new(),
                component_patterns: BTreeMap::new(),
                exploded_views: BTreeMap::new(),
                display_states: BTreeMap::new(),
                inspections: BTreeMap::new(),
                weld_beads: BTreeMap::new(),
                rigid_groups: BTreeMap::new(),
                parameters: BTreeMap::new(),
            };
            if has_geometry {
                let part = imported_part(&format!("{name} bodies"), resource, solids, surfaces);
                let instance = cad_core::id();
                assembly.instances.insert(
                    instance.clone(),
                    Instance {
                        id: instance,
                        name: part.name.clone(),
                        definition: part.id.clone(),
                        placement: Default::default(),
                        configuration: None,
                        finish: None,
                        fixed: true,
                        translation_expressions: None,
                    },
                );
                doc.parts.insert(part.id.clone(), part);
            }
            doc.assemblies.insert(key, assembly);
        }
    }
    let mut children = BTreeSet::new();
    for edge in graph.all_edges() {
        let (parent, child) = edge.nodes();
        children.insert(child);
        let m = Matrix4::try_from(edge.matrix()).map_err(|e| CadError::Invalid(e.to_string()))?;
        let values: [[f64; 4]; 4] = m.into();
        if values.iter().flatten().any(|value| !value.is_finite())
            || m.x.w != 0.
            || m.y.w != 0.
            || m.z.w != 0.
            || m.w.w != 1.
        {
            return Err(CadError::Invalid(
                "STEP instance placement must be finite and genuinely affine".into(),
            ));
        }
        let rotation = nalgebra::Matrix3::from_fn(|r, c| m[c][r]);
        if (rotation.transpose() * rotation - nalgebra::Matrix3::identity()).norm() > 1e-7
            || rotation.determinant() < 0.
        {
            return Err(CadError::Unsupported(
                "STEP instance scale or reflection requires an affine placement adapter".into(),
            ));
        }
        let (x, y, z) = nalgebra::Rotation3::from_matrix_unchecked(rotation).euler_angles();
        let translation = [
            m.w.x * length_scale,
            m.w.y * length_scale,
            m.w.z * length_scale,
        ];
        if translation
            .into_iter()
            .chain([x, y, z])
            .any(|value| !value.is_finite())
        {
            return Err(CadError::Invalid(
                "STEP instance placement exceeds finite document coordinates".into(),
            ));
        }
        let id = cad_core::id();
        let definition = definitions[&child].clone();
        let name = if edge.attributes().name.is_empty() {
            cad_core::presentation::definition_name(doc, &definition)
        } else {
            edge.attributes().name.clone()
        };
        let instance = Instance {
            id: id.clone(),
            name,
            definition,
            placement: Placement {
                translation,
                rotation_deg: [x.to_degrees(), y.to_degrees(), z.to_degrees()],
            },
            configuration: None,
            finish: None,
            fixed: true,
            translation_expressions: None,
        };
        doc.assemblies
            .get_mut(&definitions[&parent])
            .ok_or_else(|| CadError::Invalid("STEP parent has no assembly definition".into()))?
            .instances
            .insert(id, instance);
    }
    let roots = graph
        .all_nodes()
        .filter(|n| !children.contains(&n.index()))
        .map(|n| definitions[&n.index()].clone())
        .collect::<Vec<_>>();
    if roots.len() == 1 {
        return Ok(roots[0].clone());
    }
    let root = cad_core::id();
    let mut assembly = Assembly {
        id: root.clone(),
        name: "Imported STEP assembly".into(),
        properties: BTreeMap::new(),
        instances: BTreeMap::new(),
        mates: vec![],
        promoted_parameters: BTreeMap::new(),
        configurations: BTreeMap::new(),
        component_patterns: BTreeMap::new(),
        exploded_views: BTreeMap::new(),
        display_states: BTreeMap::new(),
        inspections: BTreeMap::new(),
        weld_beads: BTreeMap::new(),
        rigid_groups: BTreeMap::new(),
        parameters: BTreeMap::new(),
    };
    for definition in roots {
        let id = cad_core::id();
        assembly.instances.insert(
            id.clone(),
            Instance {
                id,
                name: cad_core::presentation::definition_name(doc, &definition),
                definition,
                placement: Default::default(),
                configuration: None,
                finish: None,
                fixed: true,
                translation_expressions: None,
            },
        );
    }
    doc.assemblies.insert(root.clone(), assembly);
    Ok(root)
}

fn imported_part(name: &str, resource: &str, solids: Vec<u64>, surfaces: Vec<u64>) -> Part {
    let mut part = Part::new(name);
    for (name, operation) in [
        (!solids.is_empty()).then(|| {
            (
                "Pinned STEP solids",
                Operation::Import {
                    resource: resource.into(),
                    solid_ids: solids,
                },
            )
        }),
        (!surfaces.is_empty()).then(|| {
            (
                "Pinned STEP surfaces",
                Operation::ImportSurfaces {
                    resource: resource.into(),
                    surface_model_ids: surfaces,
                },
            )
        }),
    ]
    .into_iter()
    .flatten()
    {
        let id = cad_core::id();
        part.outputs.push(id.clone());
        part.features.push(Feature {
            id,
            name: name.into(),
            operation,
            suppressed: false,
        });
    }
    part
}

// Upstream StepModel exposes Display rather than StepFormat. Keep its exact B-rep
// serialization and relocate only STEP entity references into the assembly stream.
struct StepBody {
    source: String,
    length: usize,
}
impl StepBody {
    fn new(source: String) -> Result<Self> {
        let start = source.find("#16 =").ok_or_else(|| {
            CadError::Invalid("Kernel STEP serializer changed its entity contract".into())
        })?;
        let source = source[start..].to_string();
        let mut maximum = 16;
        relocate(&source, 16, |id| maximum = maximum.max(id));
        Ok(Self {
            source,
            length: maximum - 15,
        })
    }
}
fn relocate(source: &str, start: usize, mut observe: impl FnMut(usize)) -> String {
    let bytes = source.as_bytes();
    let mut output = String::new();
    let mut cursor = 0;
    let mut quoted = false;
    while cursor < bytes.len() {
        if bytes[cursor] == b'\'' {
            output.push('\'');
            cursor += 1;
            if quoted && bytes.get(cursor) == Some(&b'\'') {
                output.push('\'');
                cursor += 1;
            } else {
                quoted = !quoted;
            }
            continue;
        }
        if !quoted && bytes[cursor] == b'#' {
            let begin = cursor + 1;
            let mut end = begin;
            while bytes.get(end).is_some_and(u8::is_ascii_digit) {
                end += 1;
            }
            if end > begin {
                let id = source[begin..end].parse::<usize>().unwrap_or(0);
                observe(id);
                output.push_str(&format!("#{}", start + id.saturating_sub(16)));
                cursor = end;
                continue;
            }
        }
        let ch = source[cursor..].chars().next().unwrap_or(' ');
        output.push(ch);
        cursor += ch.len_utf8();
    }
    output
}
impl monstertruck::step::save::StepLength for StepBody {
    fn step_length(&self) -> usize {
        self.length
    }
}
impl monstertruck::step::save::StepFormat for StepBody {
    fn fmt(&self, index: usize, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&relocate(&self.source, index, |_| {}))
    }
}
