//! Exact configured part-local mate references and bottom-up assembly placement.
use crate::adapter::{KernelAdapter, Shape};
use cad_core::{
    CadError, Document, Id, NodeRef, Operation, Result,
    assembly_geometry::{self, EvaluatedAssembly, Frame, Reference},
    configuration,
    solver::{ConstraintSolver, NativeSolver},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};
type MateFrames = BTreeMap<Id, [Frame; 2]>;
type MateAuxiliary = BTreeMap<Id, Vec<assembly_geometry::ResolvedEndpoint>>;
type ResolvedMateGeometry = (MateFrames, MateAuxiliary);

fn suppressed_shape<K: KernelAdapter>(
    adapter: &K,
    part: &cad_core::Part,
    feature: &cad_core::Feature,
    shapes: &BTreeMap<NodeRef, Shape>,
) -> Result<Option<Shape>> {
    if feature.operation.is_reference()
        || feature.operation.is_analysis()
        || matches!(feature.operation, Operation::Profile { .. })
    {
        return Ok(None);
    }
    let mut inputs = feature.operation.consumed_outputs();
    let mut seen = BTreeSet::new();
    inputs.retain(|input| seen.insert(input.clone()));
    if inputs.is_empty() {
        return Ok(None);
    }
    let sources = inputs
        .iter()
        .map(|input| {
            shapes.get(input).ok_or_else(|| {
                CadError::Invalid(format!(
                    "Suppressed {} cannot restore missing source {}",
                    feature.name, input.node
                ))
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let shape = if sources
        .iter()
        .all(|shape| matches!(shape, Shape::Bodies(_)))
    {
        Shape::Bodies(
            sources
                .into_iter()
                .flat_map(|shape| {
                    if let Shape::Bodies(bodies) = shape {
                        bodies.clone()
                    } else {
                        unreachable!()
                    }
                })
                .collect(),
        )
    } else if sources
        .iter()
        .all(|shape| matches!(shape, Shape::Surfaces(_)))
    {
        Shape::Surfaces(
            sources
                .into_iter()
                .flat_map(|shape| {
                    if let Shape::Surfaces(shells) = shape {
                        shells.clone()
                    } else {
                        unreachable!()
                    }
                })
                .collect(),
        )
    } else {
        return Err(CadError::Unsupported(
            "Suppression cannot combine incompatible solid and surface source branches".into(),
        ));
    };
    adapter.alias_histories(&part.reference(&feature.id), &inputs)?;
    Ok(Some(shape))
}
pub(crate) struct ShapeBuildReport {
    pub shapes: BTreeMap<NodeRef, Shape>,
    pub failures: BTreeMap<NodeRef, String>,
    pub blocked: BTreeMap<NodeRef, String>,
}
/// Retain current-source upstream results when a feature fails. Partial results
/// are display-only: callers must still reject the whole modeling evaluation.
pub(crate) fn build_shapes_report<K: KernelAdapter>(
    adapter: &K,
    doc: &Document,
    root: &Path,
    tolerance: f64,
    defer_placement: bool,
) -> Result<ShapeBuildReport> {
    let mut shapes = BTreeMap::new();
    let mut deferred = BTreeSet::new();
    let mut failures = BTreeMap::new();
    let mut blocked = BTreeMap::new();
    for node in doc.evaluation_order()? {
        let part = &doc.parts[&node.definition];
        let feature = part
            .features
            .iter()
            .find(|f| f.id == node.node)
            .ok_or_else(|| CadError::Invalid("Missing feature".into()))?;
        if defer_placement
            && (matches!(feature.operation, Operation::ReferenceBody { .. })
                || part
                    .feature_dependencies(feature)
                    .iter()
                    .any(|input| deferred.contains(input)))
        {
            deferred.insert(node);
            continue;
        }
        if let Some(source) = part
            .feature_dependencies(feature)
            .into_iter()
            .find(|source| failures.contains_key(source) || blocked.contains_key(source))
        {
            let source_label = doc
                .parts
                .get(&source.definition)
                .and_then(|part| {
                    part.features
                        .iter()
                        .find(|feature| feature.id == source.node)
                        .map(|feature| format!("{} / {}", part.name, feature.name))
                })
                .unwrap_or_else(|| "unavailable source feature".into());
            blocked.insert(
                node,
                format!(
                    "{} / {}: blocked by failed source {source_label}",
                    part.name, feature.name
                ),
            );
            continue;
        }
        let shape = if feature.suppressed {
            let shape = match suppressed_shape(adapter, part, feature, &shapes) {
                Ok(shape) => shape,
                Err(error) => {
                    failures.insert(node, format!("{} / {}: {error}", part.name, feature.name));
                    continue;
                }
            };
            let Some(shape) = shape else {
                continue;
            };
            shape
        } else {
            match adapter.operation(doc, &node, &feature.operation, &shapes, root, tolerance) {
                Ok(shape) => shape,
                Err(error) => {
                    failures.insert(node, format!("{} / {}: {error}", part.name, feature.name));
                    continue;
                }
            }
        };
        shapes.insert(node, shape);
    }
    Ok(ShapeBuildReport {
        shapes,
        failures,
        blocked,
    })
}
fn dependency_closure(doc: &Document, roots: Vec<NodeRef>) -> Result<BTreeSet<NodeRef>> {
    let mut closure = BTreeSet::new();
    let mut pending = roots;
    while let Some(node) = pending.pop() {
        if !closure.insert(node.clone()) {
            continue;
        }
        let part = doc
            .parts
            .get(&node.definition)
            .ok_or_else(|| CadError::Invalid("Mate reference part is missing".into()))?;
        let feature = part
            .features
            .iter()
            .find(|f| f.id == node.node)
            .ok_or_else(|| CadError::Invalid("Mate reference feature is missing".into()))?;
        if matches!(feature.operation, Operation::ReferenceBody { .. }) {
            return Err(CadError::Unsupported("Associative mate geometry cannot depend on an in-context Reference Body: its assembly placement feeds back into geometry. Select an independent local face, edge, sketch, or datum".into()));
        }
        pending.extend(part.feature_dependencies(feature));
    }
    Ok(closure)
}
struct Resolver<'a, K> {
    adapter: &'a K,
    source: &'a Document,
    shapes: &'a BTreeMap<NodeRef, Shape>,
    root: &'a Path,
    tolerance: f64,
    active: BTreeSet<Id>,
    done: BTreeMap<Id, EvaluatedAssembly>,
}
impl<K: KernelAdapter> Resolver<'_, K> {
    fn scope(
        &self,
        context: &Document,
        definition: &str,
        configuration: Option<&str>,
    ) -> Result<Document> {
        let mut doc = configuration::scope(self.source, context, definition, configuration)?;
        if let Some(assembly) = doc.assemblies.get(definition).cloned() {
            let mut assembly = assembly;
            for instance in assembly.instances.values_mut() {
                if let Some(expressions) = &instance.translation_expressions {
                    for (axis, e) in expressions.iter().enumerate() {
                        instance.placement.translation[axis] = cad_core::expression::value(
                            &doc,
                            definition,
                            e,
                            cad_core::Dimension::Length,
                        )?;
                    }
                }
            }
            if let Some(evaluated) = self
                .done
                .get(&configuration::key(definition, configuration))
            {
                for (id, p) in &evaluated.placements {
                    if let Some(i) = assembly.instances.get_mut(id) {
                        i.placement = p.clone();
                    }
                }
            }
            doc.assemblies.insert(definition.into(), assembly);
        }
        Ok(doc)
    }
    fn frame(
        &self,
        context: &Document,
        instance: &cad_core::Instance,
        reference: &Reference,
    ) -> Result<Frame> {
        let config = configuration::selected(
            context,
            &instance.definition,
            instance.configuration.as_ref(),
        );
        let mut scoped = self.scope(context, &instance.definition, config.map(String::as_str))?;
        let mut definition = instance.definition.clone();
        let mut child = nalgebra::Matrix4::identity();
        for id in &reference.path {
            let component = scoped
                .assemblies
                .get(&definition)
                .and_then(|a| a.instances.get(id))
                .cloned()
                .ok_or_else(|| {
                    CadError::Invalid(
                        "Mate reference path enters a suppressed or missing component".into(),
                    )
                })?;
            child *= component.placement.matrix();
            let config = configuration::selected(
                &scoped,
                &component.definition,
                component.configuration.as_ref(),
            )
            .cloned();
            scoped = self.scope(&scoped, &component.definition, config.as_deref())?;
            definition = component.definition;
        }
        for dependency in reference.source.dependencies() {
            if dependency.definition != definition {
                return Err(CadError::Invalid(
                    "Mate reference belongs to another configured participant".into(),
                ));
            }
        }
        configuration::check_geometry_context(self.source, &scoped, &definition)?;
        let closure = dependency_closure(&scoped, reference.source.dependencies())?;
        let mut shapes = self.shapes.clone();
        for node in scoped
            .evaluation_order()?
            .into_iter()
            .filter(|n| closure.contains(n))
        {
            let part = &scoped.parts[&node.definition];
            let feature = part
                .features
                .iter()
                .find(|f| f.id == node.node)
                .ok_or_else(|| CadError::Invalid("Configured mate feature is missing".into()))?;
            if feature.suppressed {
                if let Some(shape) = suppressed_shape(self.adapter, part, feature, &shapes)? {
                    shapes.insert(node, shape);
                } else {
                    shapes.remove(&node);
                }
                continue;
            }
            let shape = self.adapter.operation(
                &scoped,
                &node,
                &feature.operation,
                &shapes,
                self.root,
                self.tolerance,
            )?;
            shapes.insert(node, shape);
        }
        self.adapter
            .mate_frame(&scoped, &definition, &reference.source, &shapes)?
            .transformed(&child)
    }
    fn mate_geometry(
        &self,
        context: &Document,
        assembly: &cad_core::Assembly,
        mates: &[cad_core::Mate],
    ) -> Result<ResolvedMateGeometry> {
        let mut frames = BTreeMap::new();
        let mut auxiliary = BTreeMap::new();
        for mate in mates {
            let mut pair = Vec::new();
            for (participant, reference, first) in [
                (&mate.first, mate.first_reference.as_ref(), true),
                (&mate.second, mate.second_reference.as_ref(), false),
            ] {
                let instance = assembly.instances.get(participant).ok_or_else(|| {
                    CadError::Invalid("Mate participant is suppressed or missing".into())
                })?;
                pair.push(if let Some(reference) = reference {
                    self.frame(context, instance, reference).map_err(|e| {
                        CadError::Invalid(format!(
                            "{} / {} mate / {}: {e}",
                            assembly.name, mate.kind, instance.name
                        ))
                    })?
                } else {
                    Frame::legacy(mate, first)?
                });
            }
            frames.insert(mate.id.clone(), [pair.remove(0), pair.remove(0)]);
            if let Some(relationship) = &mate.relationship {
                let mut resolved = Vec::new();
                for endpoint in relationship.endpoints() {
                    let instance = assembly.instances.get(&endpoint.instance).ok_or_else(|| {
                        CadError::Invalid(
                            "Additional mate participant is suppressed or missing".into(),
                        )
                    })?;
                    resolved.push(assembly_geometry::ResolvedEndpoint {
                        instance: endpoint.instance.clone(),
                        frame: self.frame(context, instance, &endpoint.reference)?,
                    });
                }
                if !resolved.is_empty() {
                    auxiliary.insert(mate.id.clone(), resolved);
                }
            }
        }
        Ok((frames, auxiliary))
    }
    fn visit(
        &mut self,
        context: &Document,
        definition: &str,
        configuration: Option<&str>,
    ) -> Result<()> {
        let scoped = self.scope(context, definition, configuration)?;
        configuration::check_geometry_context(self.source, &scoped, definition)?;
        let key = configuration::key(definition, configuration);
        if self.done.contains_key(&key) {
            return Ok(());
        }
        if !self.active.insert(definition.into()) {
            return Err(CadError::Invalid(
                "Associative assembly configuration/reference cycle".into(),
            ));
        }
        let scoped = self.scope(context, definition, configuration)?;
        let mut assembly = scoped
            .assemblies
            .get(definition)
            .cloned()
            .ok_or_else(|| CadError::Invalid("Missing configured assembly".into()))?;
        let generated = cad_core::component_pattern::generated(&assembly);
        assembly.instances.retain(|id, _| !generated.contains(id));
        for instance in assembly.instances.values() {
            if scoped.assemblies.contains_key(&instance.definition) {
                let config = configuration::selected(
                    &scoped,
                    &instance.definition,
                    instance.configuration.as_ref(),
                );
                self.visit(&scoped, &instance.definition, config.map(String::as_str))?;
            }
        }
        let scoped = self.scope(&scoped, definition, configuration)?;
        let (mut frames, mut auxiliary_frames) =
            self.mate_geometry(&scoped, &assembly, &assembly.mates)?;
        let solution = NativeSolver
            .solve_assembly_geometry(&scoped, &mut assembly, &frames, &auxiliary_frames)
            .map_err(|e| CadError::Invalid(format!("{}: {e}", assembly.name)))?;
        let report = Some(solution.report);
        let mut diagnostics = solution.diagnostics;
        let component_motion = solution.component_motion;
        let overrides = configuration
            .map(|configuration| {
                cad_core::configuration::resolve(self.source, definition, configuration)
            })
            .transpose()?
            .map(|c| c.overrides)
            .unwrap_or_default();
        for id in cad_core::component_pattern::order(&assembly)? {
            let pattern = assembly.component_patterns[&id].clone();
            if overrides
                .component_patterns
                .get(&id)
                .copied()
                .unwrap_or(false)
            {
                continue;
            }
            if cad_core::component_pattern::count(&scoped, definition, &pattern)? == 1
                || pattern
                    .seeds
                    .iter()
                    .all(|seed| !assembly.instances.contains_key(seed))
            {
                continue;
            }
            let mut context = scoped.clone();
            context
                .assemblies
                .insert(definition.into(), assembly.clone());
            let frame = if let cad_core::component_pattern::Mode::Circular {
                axis_reference: Some(reference),
                ..
            } = &pattern.mode
            {
                let instance = assembly.instances.get(&reference.instance).ok_or_else(|| {
                    CadError::Invalid("Pattern axis component is suppressed or missing".into())
                })?;
                let frame = self.frame(&context, instance, &reference.geometry)?;
                if matches!(frame.kind, assembly_geometry::FrameKind::Point) {
                    return Err(CadError::Invalid(
                        "A circular component pattern needs an axis or plane-normal reference"
                            .into(),
                    ));
                }
                Some(frame.transformed(&instance.placement.matrix())?)
            } else {
                None
            };
            cad_core::component_pattern::expand(
                &context,
                &mut assembly,
                &pattern,
                &overrides,
                frame.as_ref(),
            )?;
            for instance in assembly
                .instances
                .values()
                .filter(|i| generated.contains(&i.id))
            {
                if scoped.assemblies.contains_key(&instance.definition) {
                    let config = cad_core::configuration::selected(
                        &context,
                        &instance.definition,
                        instance.configuration.as_ref(),
                    );
                    self.visit(&context, &instance.definition, config.map(String::as_str))?;
                }
            }
            let copied = cad_core::component_pattern::internal_mates(&assembly, &pattern)?
                .into_iter()
                .filter(|m| !overrides.mates.get(&m.id).copied().unwrap_or(false))
                .collect::<Vec<_>>();
            if !copied.is_empty() {
                let mut current = context.clone();
                current
                    .assemblies
                    .insert(definition.into(), assembly.clone());
                let (copied_frames, copied_auxiliary) =
                    self.mate_geometry(&current, &assembly, &copied)?;
                let mut fixed = assembly.clone();
                fixed.mates = copied;
                for instance in fixed.instances.values_mut() {
                    instance.fixed = true;
                }
                let copied_solution = NativeSolver
                    .solve_assembly_geometry(
                        &current,
                        &mut fixed,
                        &copied_frames,
                        &copied_auxiliary,
                    )
                    .map_err(|e| {
                        CadError::Invalid(format!("{} / copied internal mates: {e}", pattern.name))
                    })?;
                frames.extend(copied_frames);
                auxiliary_frames.extend(copied_auxiliary);
                diagnostics.extend(copied_solution.diagnostics);
            }
        }
        self.done.insert(
            key,
            EvaluatedAssembly {
                version: assembly_geometry::CONTRACT_VERSION,
                placements: assembly
                    .instances
                    .iter()
                    .map(|(id, i)| (id.clone(), i.placement.clone()))
                    .collect(),
                frames,
                report,
                auxiliary_frames,
                diagnostics,
                component_motion,
            },
        );
        self.active.remove(definition);
        Ok(())
    }
}
pub(crate) fn solve<K: KernelAdapter>(
    adapter: &K,
    source: &Document,
    doc: &mut Document,
    shapes: &BTreeMap<NodeRef, Shape>,
    root: &Path,
    tolerance: f64,
) -> Result<BTreeMap<Id, EvaluatedAssembly>> {
    let mut resolver = Resolver {
        adapter,
        source,
        shapes,
        root,
        tolerance,
        active: BTreeSet::new(),
        done: BTreeMap::new(),
    };
    for definition in doc.assemblies.keys() {
        resolver.visit(
            doc,
            definition,
            doc.active_configurations
                .get(definition)
                .map(String::as_str),
        )?;
    }
    for (definition, assembly) in &mut doc.assemblies {
        let key = configuration::key(
            definition,
            doc.active_configurations
                .get(definition)
                .map(String::as_str),
        );
        if let Some(evaluated) = resolver.done.get(&key) {
            for (id, p) in &evaluated.placements {
                if let Some(i) = assembly.instances.get_mut(id) {
                    i.placement = p.clone();
                }
            }
        }
    }
    Ok(resolver.done)
}
