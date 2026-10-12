use crate::adapter::{KernelAdapter, MonstertruckAdapter, Shape};
use cad_core::{
    CadError, Document, NodeRef, Result,
    protocol::*,
    solver::{ConstraintSolver, NativeSolver},
};
use std::{collections::BTreeMap, path::Path, time::Instant};
struct PreviewByteBudget {
    remaining: usize,
}
impl std::io::Write for PreviewByteBudget {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.remaining = self
            .remaining
            .checked_sub(bytes.len())
            .ok_or_else(|| std::io::Error::other("Localized preview byte budget exceeded"))?;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
pub struct Evaluator<K: KernelAdapter = MonstertruckAdapter> {
    pub adapter: K,
    pub shapes: BTreeMap<NodeRef, Shape>,
    pub body_sets: BTreeMap<String, Vec<monstertruck::modeling::Solid>>,
    pub surface_sets: BTreeMap<String, Vec<monstertruck::modeling::Shell>>,
    assemblies: BTreeMap<String, cad_core::assembly_geometry::EvaluatedAssembly>,
    last_source: String,
    last_tolerance: f64,
    history_cache: BTreeMap<String, (String, f64, cad_core::history::PartHistory)>,
    preview_focus: Option<NodeRef>,
    focused_previews: BTreeMap<String, OperationPreviewChange>,
    failed_build: Option<(Document, Vec<Diagnostic>)>,
}
impl Default for Evaluator {
    fn default() -> Self {
        Self {
            adapter: MonstertruckAdapter::default(),
            shapes: BTreeMap::new(),
            body_sets: BTreeMap::new(),
            surface_sets: BTreeMap::new(),
            assemblies: BTreeMap::new(),
            last_source: String::new(),
            last_tolerance: 0.,
            history_cache: BTreeMap::new(),
            preview_focus: None,
            focused_previews: BTreeMap::new(),
            failed_build: None,
        }
    }
}
impl<K: KernelAdapter> Evaluator<K> {
    fn failure_evaluation(
        &mut self,
        identity: RequestIdentity,
        source: &Document,
        tolerance: f64,
        elapsed_ms: u64,
    ) -> Result<Option<Evaluation>> {
        let Some((document, diagnostics)) = self.failed_build.take() else {
            return Ok(None);
        };
        // All shapes in this cache came from the failing current-source build.
        // Neither a last-valid document nor its adapter provenance is reused.
        let mut history = BTreeMap::new();
        let mut meshes = BTreeMap::new();
        let mut sketches = BTreeMap::new();
        let mut references = BTreeMap::new();
        let mut reference_curves = BTreeMap::new();
        let mut reference_curve_meshes = BTreeMap::new();
        let mut available_keys = std::collections::BTreeSet::new();
        let mut instances = cad_core::configuration::placed_parts(source, None)?;
        let failed = diagnostics
            .iter()
            .filter_map(|d| d.target.as_ref().map(|id| (id.clone(), d.message.clone())))
            .collect::<BTreeMap<_, _>>();
        for part in document.parts.values() {
            let configuration = document
                .active_configurations
                .get(&part.id)
                .map(String::as_str);
            let key = cad_core::configuration::key(&part.id, configuration);
            available_keys.insert(key.clone());
            let mut part_history = self.history_output(&document, &part.id, &key, tolerance);
            let mut first_failure = None;
            for feature in &part.features {
                if let Some(message) = failed.get(&feature.id) {
                    first_failure.get_or_insert_with(|| message.clone());
                }
                if let Some(reason) = &first_failure
                    && let Some(stage) = part_history.stages.get_mut(&feature.id)
                {
                    stage.unavailable_reason = Some(reason.clone());
                }
            }
            // The display endpoint is explicitly the last complete upstream station.
            let station = part_history
                .stages
                .values()
                .filter(|s| s.unavailable_reason.is_none())
                .max_by_key(|s| s.index);
            if let Some(station) = station {
                let mut display_part = part.clone();
                display_part.outputs = station.bodies.clone();
                if let Ok((mesh, _, _)) = self.part_output(&display_part, tolerance) {
                    meshes.insert(key.clone(), mesh);
                }
            }
            let mut usable = document.clone();
            usable
                .parts
                .get_mut(&part.id)
                .unwrap()
                .features
                .retain(|f| self.shapes.contains_key(&part.reference(&f.id)));
            if let Ok(output) = self.sketch_output(&usable, &part.id) {
                sketches.extend(
                    output
                        .into_iter()
                        .map(|(id, value)| (format!("{key}::{id}"), value)),
                );
            }
            references.extend(
                self.reference_output(&document, &part.id)
                    .into_iter()
                    .map(|(id, value)| (format!("{key}::{id}"), value)),
            );
            if let Ok(output) = self.curve_output(&document, &part.id, tolerance) {
                for (id, value, mesh) in output {
                    reference_curves.insert(format!("{key}::{id}"), value);
                    reference_curve_meshes.insert(format!("{key}::{id}"), mesh);
                }
            }
            // Base-part lookup is useful to semantic history even when this part is configured.
            history.insert(part.id.clone(), part_history.clone());
            history.insert(key.clone(), part_history);
            for instance in instances.iter_mut().filter(|i| i.definition == part.id) {
                let actual = instance.configuration.as_deref().or_else(|| {
                    source
                        .active_configurations
                        .get(&part.id)
                        .map(String::as_str)
                });
                if actual == configuration {
                    instance.mesh_key = key.clone();
                }
            }
        }
        instances.retain(|i| available_keys.contains(cad_core::history::mesh_key(i)));
        self.body_sets.clear();
        self.surface_sets.clear();
        self.history_cache.clear();
        Ok(Some(Evaluation {
            identity,
            meshes,
            instances,
            diagnostics,
            valid: false,
            elapsed_ms,
            sketches,
            references,
            reference_curves,
            reference_curve_meshes,
            assemblies: BTreeMap::new(),
            exploded_views: BTreeMap::new(),
            assembly_inspections: BTreeMap::new(),
            properties: BTreeMap::new(),
            sheets: BTreeMap::new(),
            weldments: BTreeMap::new(),
            weld_beads: BTreeMap::new(),
            assembly_welds: BTreeMap::new(),
            mold_analyses: BTreeMap::new(),
            configurations: cad_core::configuration::evaluated(&document).ok(),
            history: std::sync::Arc::new(history),
            operation_preview: None,
        }))
    }
    /// Evaluate authoritative geometry first-class, with optional isolated
    /// display comparison. Highlight failures never invalidate the CAD result.
    pub fn evaluate_with_operation_preview(
        &mut self,
        identity: RequestIdentity,
        document: &Document,
        root: &Path,
        tolerance: f64,
        request: Option<Box<OperationPreviewRequest>>,
    ) -> Result<Evaluation> {
        let start = Instant::now();
        self.preview_focus = None;
        self.focused_previews.clear();
        let Some(request) = request else {
            return self.evaluate(identity, document, root, tolerance);
        };
        let mut preview = OperationPreview {
            version: OPERATION_PREVIEW_VERSION,
            baseline_identity: request.baseline_identity.clone(),
            baseline_kind: if request.focus.is_some() {
                PreviewBaseline::FeatureInput
            } else {
                PreviewBaseline::Checkpoint
            },
            focus: request.focus.clone(),
            tolerance_mm: tolerance,
            changes: BTreeMap::new(),
            baseline_instances: Vec::new(),
            diagnostics: Vec::new(),
        };
        let baseline_valid = request.baseline_identity.protocol == PROTOCOL_VERSION
            && request.baseline_identity.backend == self.fingerprint()
            && request.baseline_identity.source == request.baseline.fingerprint();
        let mut baseline = None;
        if !baseline_valid {
            preview.diagnostics.push(crate::operation_preview::warning(
                request.focus.as_ref(),
                "Localized preview baseline identity does not match its source/backend; the validated model is displayed neutrally",
            ));
        } else if request.focus.is_none() {
            match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                self.evaluate(
                    request.baseline_identity.clone(),
                    &request.baseline,
                    root,
                    tolerance,
                )
            })) {
                Ok(Ok(evaluated)) => {
                    preview.baseline_instances = evaluated
                        .instances
                        .iter()
                        .chain(evaluated.assembly_welds.values().map(|w| &w.placement))
                        .cloned()
                        .collect();
                    baseline = Some((self.body_sets.clone(), self.surface_sets.clone()));
                }
                failed => {
                    let reason = match failed {
                        Ok(Err(error)) => error.to_string(),
                        Err(_) => {
                            self.last_source.clear();
                            "baseline display evaluation failed internally".into()
                        }
                        Ok(Ok(_)) => unreachable!(),
                    };
                    preview.diagnostics.push(crate::operation_preview::warning(
                        None,
                        format!("Localized preview baseline is unavailable: {reason}"),
                    ));
                }
            }
        } else {
            self.preview_focus = request.focus.clone();
        }
        let evaluated = self.evaluate(identity, document, root, tolerance);
        self.preview_focus = None;
        let mut evaluated = evaluated?;
        if request.focus.is_some() && baseline_valid {
            preview.changes = std::mem::take(&mut self.focused_previews);
            preview.baseline_instances = evaluated.instances.clone();
            if preview.changes.is_empty() {
                preview.diagnostics.push(crate::operation_preview::warning(
                    request.focus.as_ref(),
                    "The selected feature has no current solid/surface output for a localized operation preview",
                ));
            }
        } else if let Some((before_bodies, before_surfaces)) = baseline {
            let keys = before_bodies
                .keys()
                .chain(before_surfaces.keys())
                .chain(self.body_sets.keys())
                .chain(self.surface_sets.keys())
                .cloned()
                .collect::<std::collections::BTreeSet<_>>();
            for key in keys {
                let shape = |bodies: &BTreeMap<String, Vec<monstertruck::modeling::Solid>>,
                             surfaces: &BTreeMap<String, Vec<monstertruck::modeling::Shell>>| {
                    if let Some(shells) = surfaces.get(&key).filter(|shells| !shells.is_empty()) {
                        if bodies.get(&key).is_some_and(|bodies| !bodies.is_empty()) {
                            return None;
                        }
                        Some(Shape::Surfaces(shells.clone()))
                    } else {
                        Some(Shape::Bodies(bodies.get(&key).cloned().unwrap_or_default()))
                    }
                };
                let owner = NodeRef {
                    definition: evaluated
                        .instances
                        .iter()
                        .chain(evaluated.assembly_welds.values().map(|w| &w.placement))
                        .chain(&preview.baseline_instances)
                        .find(|instance| instance.mesh_key == key)
                        .map(|instance| instance.definition.clone())
                        .unwrap_or_else(|| key.clone()),
                    node: "operation-preview".into(),
                };
                let change = match (
                    shape(&before_bodies, &before_surfaces),
                    shape(&self.body_sets, &self.surface_sets),
                ) {
                    (Some(before), Some(after)) => {
                        self.display_delta(&before, &after, &owner, tolerance)
                    }
                    _ => Self::unavailable_change(
                        &owner,
                        "Mixed solid and surface output needs separate localized display comparisons",
                    ),
                };
                if change.added.is_some()
                    || change.removed.is_some()
                    || !change.diagnostics.is_empty()
                {
                    preview.changes.insert(key, change);
                }
            }
        }
        let mut budget = PreviewByteBudget {
            remaining: 16 * 1024 * 1024,
        };
        let mut omitted = false;
        preview.changes.retain(|_, change| {
            let start = budget.remaining;
            if serde_json::to_writer(&mut budget, change).is_err() {
                budget.remaining = start;
                omitted = true;
                false
            } else {
                true
            }
        });
        if omitted {
            preview.diagnostics.push(crate::operation_preview::warning(
                request.focus.as_ref(),
                "Some localized highlights exceed the 16 MiB cumulative display cache bound; their validated model geometry is displayed neutrally",
            ));
        }
        evaluated.operation_preview = Some(Box::new(preview));
        evaluated.elapsed_ms = start.elapsed().as_millis().try_into().unwrap_or(u64::MAX);
        Ok(evaluated)
    }

    fn unavailable_change(owner: &NodeRef, message: impl Into<String>) -> OperationPreviewChange {
        OperationPreviewChange {
            added: None,
            removed: None,
            representation: PreviewRepresentation::ExactBrep,
            diagnostics: vec![crate::operation_preview::warning(Some(owner), message)],
        }
    }

    fn display_delta(
        &self,
        before: &Shape,
        after: &Shape,
        owner: &NodeRef,
        tolerance: f64,
    ) -> OperationPreviewChange {
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.adapter
                .operation_delta(before, after, owner, tolerance)
        })) {
            Ok(Ok(change)) => change,
            Ok(Err(error)) => Self::unavailable_change(owner, error.to_string()),
            Err(_) => Self::unavailable_change(
                owner,
                "Localized operation display comparison failed internally; the validated model is displayed neutrally",
            ),
        }
    }

    fn focused_preview(&mut self, part: &cad_core::Part, key: &str, tolerance: f64) {
        let Some(owner) = self
            .preview_focus
            .as_ref()
            .filter(|focus| focus.definition == part.id)
            .cloned()
        else {
            return;
        };
        let Some(feature) = part
            .features
            .iter()
            .find(|feature| feature.id == owner.node && !feature.suppressed)
        else {
            return;
        };
        let Some(after) = self.shapes.get(&owner) else {
            return;
        };
        if !matches!(after, Shape::Bodies(_) | Shape::Surfaces(_)) {
            return;
        }
        // Visibility consumption includes the Boolean tool. Material change
        // starts from its left operand, not the union of left and cutter.
        let inputs = match &feature.operation {
            cad_core::Operation::Boolean { left, .. } => vec![left.clone()],
            operation => operation.consumed_outputs(),
        };
        let mut bodies = Vec::new();
        let mut surfaces = Vec::new();
        let mut seen = std::collections::BTreeSet::new();
        for input in inputs
            .into_iter()
            .filter(|input| seen.insert(input.clone()))
        {
            match self.shapes.get(&input) {
                Some(Shape::Bodies(source)) => bodies.extend(source.clone()),
                Some(Shape::Surfaces(source)) => surfaces.extend(source.clone()),
                _ => {}
            }
        }
        let before = if !surfaces.is_empty() && bodies.is_empty() {
            Shape::Surfaces(surfaces)
        } else if surfaces.is_empty() {
            Shape::Bodies(bodies)
        } else {
            self.focused_previews.insert(key.into(), Self::unavailable_change(&owner, "The selected feature mixes solid and surface input; localized comparison is unavailable"));
            return;
        };
        let change = self.display_delta(&before, after, &owner, tolerance);
        self.focused_previews.insert(key.into(), change);
    }

    pub(crate) fn build(
        &mut self,
        doc: &Document,
        root: &Path,
        tolerance: f64,
    ) -> Result<Document> {
        self.failed_build = None;
        doc.validate()?;
        if !tolerance.is_finite() || !(1e-6..=1.).contains(&tolerance) {
            return Err(CadError::Invalid(
                "Geometry tolerance must be 0.000001..1 mm".into(),
            ));
        }
        let mut solved = cad_core::configuration::effective(doc)?;
        for part in solved.parts.values() {
            for parameter in part.parameters.values() {
                cad_core::expression::value(
                    &solved,
                    &part.id,
                    &parameter.expression,
                    parameter.dimension,
                )?;
            }
            // Profile operations solve their own sketches. Solving them here
            // would discard targeted failures and all usable upstream geometry.
        }
        for assembly in solved.assemblies.values() {
            for parameter in assembly.parameters.values() {
                cad_core::expression::value(
                    &solved,
                    &assembly.id,
                    &parameter.expression,
                    parameter.dimension,
                )?;
            }
        }
        use sha2::{Digest, Sha256};
        for resource in doc.resources.values() {
            let bytes = std::fs::read(cad_core::persistence::safe_join(root, &resource.path)?)?;
            if format!("{:x}", Sha256::digest(bytes)) != resource.sha256 {
                return Err(CadError::Invalid(format!(
                    "Resource {} changed; re-import it explicitly",
                    resource.path
                )));
            }
        }
        // Presentation and report definitions do not change part construction or
        // mate placement. Their worker results are refreshed below under the full
        // request identity without discarding the mechanical geometry cache.
        let mut mechanical_assemblies = doc.assemblies.clone();
        for assembly in mechanical_assemblies.values_mut() {
            assembly.exploded_views.clear();
            assembly.inspections.clear();
        }
        let source = format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(&(
                &doc.parts,
                &doc.resources,
                &mechanical_assemblies,
                &doc.active_configurations,
                root.to_string_lossy()
            ))?)
        );
        if source == self.last_source && tolerance == self.last_tolerance {
            for (definition, assembly) in &mut solved.assemblies {
                let key = cad_core::configuration::key(
                    definition,
                    solved
                        .active_configurations
                        .get(definition)
                        .map(String::as_str),
                );
                if let Some(evaluated) = self.assemblies.get(&key) {
                    for (id, placement) in &evaluated.placements {
                        if let Some(instance) = assembly.instances.get_mut(id) {
                            instance.placement = placement.clone();
                        }
                    }
                }
            }
            self.resolve_sketch_definitions(&mut solved)?;
            return Ok(solved);
        }
        // A failed rebuild may have updated adapter provenance. Never reuse its prior cache key.
        self.last_source.clear();
        let report =
            crate::assemblies::build_shapes_report(&self.adapter, &solved, root, tolerance, true)?;
        if !report.failures.is_empty() {
            let diagnostics = report
                .failures
                .iter()
                .map(|(target, message)| Diagnostic {
                    severity: "error".into(),
                    target: Some(target.node.clone()),
                    message: message.clone(),
                })
                .chain(report.blocked.iter().map(|(target, message)| Diagnostic {
                    severity: "blocked".into(),
                    target: Some(target.node.clone()),
                    message: message.clone(),
                }))
                .collect::<Vec<_>>();
            let message = diagnostics
                .iter()
                .map(|d| d.message.as_str())
                .collect::<Vec<_>>()
                .join("\n");
            self.shapes = report.shapes;
            self.assemblies.clear();
            self.history_cache.clear();
            self.failed_build = Some((solved, diagnostics));
            return Err(CadError::Invalid(message));
        }
        let shapes = report.shapes;
        let assemblies =
            crate::assemblies::solve(&self.adapter, doc, &mut solved, &shapes, root, tolerance)?;
        // Restore base topology provenance after configured reference evaluation and
        // regenerate in-context geometry from the current solved assembly placements.
        let shapes = if solved
            .assemblies
            .values()
            .any(|a| !a.mates.is_empty() || !a.component_patterns.is_empty())
            || solved.parts.values().any(|p| {
                p.features
                    .iter()
                    .any(|f| matches!(f.operation, cad_core::Operation::ReferenceBody { .. }))
            }) {
            let report = crate::assemblies::build_shapes_report(
                &self.adapter,
                &solved,
                root,
                tolerance,
                false,
            )?;
            if !report.failures.is_empty() {
                let diagnostics = report
                    .failures
                    .iter()
                    .map(|(target, message)| Diagnostic {
                        severity: "error".into(),
                        target: Some(target.node.clone()),
                        message: message.clone(),
                    })
                    .chain(report.blocked.iter().map(|(target, message)| Diagnostic {
                        severity: "blocked".into(),
                        target: Some(target.node.clone()),
                        message: message.clone(),
                    }))
                    .collect::<Vec<_>>();
                let message = diagnostics
                    .iter()
                    .map(|d| d.message.as_str())
                    .collect::<Vec<_>>()
                    .join("\n");
                self.shapes = report.shapes;
                self.assemblies.clear();
                self.history_cache.clear();
                self.failed_build = Some((solved, diagnostics));
                return Err(CadError::Invalid(message));
            }
            report.shapes
        } else {
            shapes
        };
        self.assemblies = assemblies;
        self.shapes = shapes;
        self.resolve_sketch_definitions(&mut solved)?;
        self.last_source = source;
        self.last_tolerance = tolerance;
        Ok(solved)
    }
    fn resolve_sketch_definitions(&self, doc: &mut Document) -> Result<()> {
        for node in doc.evaluation_order()? {
            let feature = doc.parts[&node.definition]
                .features
                .iter()
                .find(|f| f.id == node.node)
                .cloned()
                .ok_or_else(|| CadError::Invalid("Missing sketch feature".into()))?;
            if feature.suppressed {
                continue;
            }
            if let cad_core::Operation::Profile {
                profile: cad_core::Profile::Sketch { sketch },
                plane,
                plane_reference,
                reference_body,
            } = feature.operation
            {
                let plane = self.adapter.sketch_plane(
                    &plane,
                    plane_reference.as_ref(),
                    reference_body.as_ref(),
                    &self.shapes,
                )?;
                let mut resolved = self.adapter.prepare_sketch(
                    &doc.parts[&node.definition].sketches[&sketch],
                    &plane,
                    &self.shapes,
                )?;
                NativeSolver.solve_sketch(doc, &node.definition, &mut resolved)?;
                doc.parts
                    .get_mut(&node.definition)
                    .unwrap()
                    .sketches
                    .insert(sketch, resolved);
            }
        }
        Ok(())
    }
    fn part_output(
        &self,
        part: &cad_core::Part,
        tolerance: f64,
    ) -> Result<(
        MeshData,
        Vec<monstertruck::modeling::Solid>,
        Vec<monstertruck::modeling::Shell>,
    )> {
        let mut all = MeshData {
            positions: vec![],
            normals: vec![],
            indices: vec![],
            faces: vec![],
            edges: vec![],
            bounds: [[f64::INFINITY; 3], [f64::NEG_INFINITY; 3]],
            volume_mm3: 0.,
        };
        let mut bodies = vec![];
        let mut surfaces = vec![];
        for node in &part.outputs {
            let r = part.reference(node);
            let shape = self
                .shapes
                .get(&r)
                .ok_or_else(|| CadError::Invalid(format!("Missing output {node}")))?;
            match shape {
                Shape::Bodies(b) => bodies.extend(b.clone()),
                Shape::Surfaces(s) => surfaces.extend(s.clone()),
                _ => {
                    return Err(CadError::Invalid(format!(
                        "{} has a designated output that is neither a solid nor a surface body",
                        part.name
                    )));
                }
            }
            let m = self.adapter.mesh(shape, &r, tolerance)?;
            let has_geometry = !m.positions.is_empty();
            let offset = all.positions.len() as u32;
            let index_offset = all.indices.len();
            all.positions.extend(m.positions);
            all.normals.extend(m.normals);
            all.indices
                .extend(m.indices.into_iter().map(|i| i + offset));
            all.faces.extend(m.faces.into_iter().map(|mut f| {
                f.start += index_offset;
                f
            }));
            all.edges.extend(m.edges);
            all.volume_mm3 += m.volume_mm3;
            for axis in 0..3 {
                if has_geometry {
                    all.bounds[0][axis] = all.bounds[0][axis].min(m.bounds[0][axis]);
                    all.bounds[1][axis] = all.bounds[1][axis].max(m.bounds[1][axis]);
                }
            }
        }
        if all.positions.is_empty() {
            all.bounds = [[0.; 3]; 2];
        }
        Ok((all, bodies, surfaces))
    }
    fn history_output(
        &mut self,
        doc: &Document,
        definition: &str,
        key: &str,
        tolerance: f64,
    ) -> cad_core::history::PartHistory {
        if let Some((source, previous_tolerance, cached)) = self.history_cache.get(key)
            && source == &self.last_source
            && *previous_tolerance == tolerance
        {
            return cached.clone();
        }
        let part = &doc.parts[definition];
        let bodies = part
            .features
            .iter()
            .filter(|f| {
                matches!(
                    self.shapes.get(&part.reference(&f.id)),
                    Some(Shape::Bodies(_) | Shape::Surfaces(_))
                )
            })
            .map(|f| f.id.clone())
            .collect();
        let mut history = cad_core::history::PartHistory {
            version: cad_core::history::CONTRACT_VERSION,
            stages: cad_core::history::stages(part, &bodies),
            meshes: BTreeMap::new(),
        };
        let mut failures = BTreeMap::new();
        for feature in &bodies {
            let reference = part.reference(feature);
            if let Some(shape) = self.shapes.get(&reference) {
                match self.adapter.mesh(shape, &reference, tolerance) {
                    Ok(mesh) => {
                        history
                            .meshes
                            .insert(feature.clone(), std::sync::Arc::new(mesh));
                    }
                    Err(e) => {
                        failures.insert(
                            feature.clone(),
                            format!(
                                "Cached display geometry for {} is unavailable: {e}",
                                part.features
                                    .iter()
                                    .find(|f| &f.id == feature)
                                    .map(|f| f.name.as_str())
                                    .unwrap_or(feature)
                            ),
                        );
                    }
                }
            }
        }
        for stage in history.stages.values_mut() {
            if stage.unavailable_reason.is_none() {
                stage.unavailable_reason =
                    stage.bodies.iter().find_map(|id| failures.get(id).cloned());
            }
        }
        self.history_cache.insert(
            key.into(),
            (self.last_source.clone(), tolerance, history.clone()),
        );
        history
    }
    fn sheet_output(
        &self,
        doc: &Document,
        definition: &str,
        tolerance: f64,
    ) -> Result<BTreeMap<String, cad_core::sheet_metal::EvaluatedSheet>> {
        let part = &doc.parts[definition];
        let mut output = BTreeMap::new();
        for feature in part.features.iter().filter(|f| {
            !f.suppressed
                && part.outputs.contains(&f.id)
                && matches!(f.operation, cad_core::Operation::Sheet { .. })
        }) {
            if let Some(sheet) = self.adapter.sheet_metadata(
                doc,
                &part.reference(&feature.id),
                &self.shapes,
                tolerance,
            )? {
                output.insert(feature.id.clone(), sheet);
            }
        }
        Ok(output)
    }
    fn weldment_output(
        &self,
        doc: &Document,
        definition: &str,
    ) -> Result<BTreeMap<String, cad_core::weldment::EvaluatedWeldment>> {
        let part = &doc.parts[definition];
        let mut output = BTreeMap::new();
        for feature in part.features.iter().filter(|f| {
            !f.suppressed
                && part.outputs.contains(&f.id)
                && matches!(f.operation, cad_core::Operation::Weldment { .. })
        }) {
            if let Some(weldment) =
                self.adapter
                    .weldment_metadata(doc, &part.reference(&feature.id), &self.shapes)?
            {
                output.insert(feature.id.clone(), weldment);
            }
        }
        Ok(output)
    }
    fn weld_bead_output(
        &self,
        doc: &Document,
        definition: &str,
    ) -> Result<BTreeMap<String, cad_core::weld_beads::Evaluated>> {
        let part = &doc.parts[definition];
        let mut output = BTreeMap::new();
        for feature in part.features.iter().filter(|feature| {
            !feature.suppressed
                && part.outputs.contains(&feature.id)
                && matches!(feature.operation, cad_core::Operation::WeldBead { .. })
        }) {
            if let Some(bead) =
                self.adapter
                    .weld_bead_metadata(doc, &part.reference(&feature.id), &self.shapes)?
            {
                output.insert(feature.id.clone(), bead);
            }
        }
        Ok(output)
    }
    fn reference_output(
        &self,
        doc: &Document,
        definition: &str,
    ) -> BTreeMap<String, cad_core::reference_geometry::EvaluatedReference> {
        let part = &doc.parts[definition];
        part.features
            .iter()
            .filter(|f| !f.suppressed && f.operation.is_reference())
            .filter_map(|feature| {
                let producer = part.reference(&feature.id);
                let Shape::Reference(geometry) = self.shapes.get(&producer)? else {
                    return None;
                };
                Some((
                    feature.id.clone(),
                    cad_core::reference_geometry::EvaluatedReference {
                        version: cad_core::reference_geometry::CONTRACT_VERSION,
                        producer,
                        name: feature.name.clone(),
                        geometry: geometry.clone(),
                    },
                ))
            })
            .collect()
    }
    fn mold_output(
        &self,
        doc: &Document,
        definition: &str,
    ) -> BTreeMap<String, cad_core::mold::DraftReport> {
        let part = &doc.parts[definition];
        part.features
            .iter()
            .filter(|f| !f.suppressed && f.operation.is_analysis())
            .filter_map(|feature| {
                let Shape::Analysis(report) = self.shapes.get(&part.reference(&feature.id))? else {
                    return None;
                };
                Some((feature.id.clone(), report.clone()))
            })
            .collect()
    }
    fn curve_output(
        &self,
        doc: &Document,
        definition: &str,
        tolerance: f64,
    ) -> Result<Vec<(String, cad_core::reference_curves::Evaluated, MeshData)>> {
        let part = &doc.parts[definition];
        part.features
            .iter()
            .filter(|feature| !feature.suppressed)
            .filter_map(|feature| {
                let producer = part.reference(&feature.id);
                let Shape::ReferenceCurves { evaluated, .. } = self.shapes.get(&producer)? else {
                    return None;
                };
                Some(
                    self.adapter
                        .mesh(self.shapes.get(&producer).unwrap(), &producer, tolerance)
                        .map(|mesh| (feature.id.clone(), evaluated.clone(), mesh)),
                )
            })
            .collect()
    }
    fn sketch_output(
        &self,
        doc: &Document,
        definition: &str,
    ) -> Result<BTreeMap<String, EvaluatedSketch>> {
        let part = &doc.parts[definition];
        let mut output = BTreeMap::new();
        for feature in part.features.iter().filter(|f| !f.suppressed) {
            if let cad_core::Operation::Profile {
                profile,
                plane,
                plane_reference,
                reference_body,
            } = &feature.operation
            {
                let plane = self.adapter.sketch_plane(
                    plane,
                    plane_reference.as_ref(),
                    reference_body.as_ref(),
                    &self.shapes,
                )?;
                let mut resolved = None;
                let (points, profile_order, circles) = match profile {
                    cad_core::Profile::Polygon { points } => {
                        let keys = (0..points.len())
                            .map(|i| format!("p{i}"))
                            .collect::<Vec<_>>();
                        (
                            keys.iter().cloned().zip(points.iter().copied()).collect(),
                            keys,
                            BTreeMap::new(),
                        )
                    }
                    cad_core::Profile::Circle { center, radius } => {
                        let mut points = BTreeMap::new();
                        points.insert("center".into(), *center);
                        let mut circles = BTreeMap::new();
                        circles.insert(
                            "circle".into(),
                            EvaluatedCircle {
                                center: "center".into(),
                                radius_mm: cad_core::expression::value(
                                    doc,
                                    definition,
                                    radius,
                                    cad_core::Dimension::Length,
                                )?,
                            },
                        );
                        (points, vec![], circles)
                    }
                    cad_core::Profile::Sketch { sketch } => {
                        let mut sketch = self.adapter.prepare_sketch(
                            &part.sketches[sketch],
                            &plane,
                            &self.shapes,
                        )?;
                        NativeSolver.solve_sketch(doc, definition, &mut sketch)?;
                        let circles = sketch
                            .circles
                            .iter()
                            .map(|(id, c)| {
                                Ok((
                                    id.clone(),
                                    EvaluatedCircle {
                                        center: c.center.clone(),
                                        radius_mm: cad_core::expression::value(
                                            doc,
                                            definition,
                                            &c.radius,
                                            cad_core::Dimension::Length,
                                        )?,
                                    },
                                ))
                            })
                            .collect::<Result<_>>()?;
                        resolved = Some(sketch.clone());
                        (sketch.points, sketch.profile_order, circles)
                    }
                };
                output.insert(
                    feature.id.clone(),
                    EvaluatedSketch {
                        plane: plane.clone(),
                        points,
                        profile_order,
                        circles,
                        polylines: resolved
                            .as_ref()
                            .map(|source| {
                                cad_core::sketch_geometry::polylines(doc, definition, source)
                            })
                            .transpose()?
                            .unwrap_or_default(),
                        material_regions: if matches!(
                            self.shapes.get(&part.reference(&feature.id)),
                            Some(Shape::Sketch { .. })
                        ) {
                            resolved
                                .as_ref()
                                .map(|source| {
                                    self.adapter
                                        .sketch_material_regions(doc, definition, source, &plane)
                                })
                                .transpose()?
                                .unwrap_or_default()
                        } else {
                            Vec::new()
                        },
                        solved: resolved,
                    },
                );
            }
        }
        Ok(output)
    }
}
impl<K: KernelAdapter> GeometryService for Evaluator<K> {
    fn fingerprint(&self) -> &str {
        self.adapter.fingerprint()
    }
    fn evaluate(
        &mut self,
        identity: RequestIdentity,
        document: &Document,
        root: &Path,
        tolerance: f64,
    ) -> Result<Evaluation> {
        let start = Instant::now();
        if identity.protocol != PROTOCOL_VERSION {
            return Err(CadError::Invalid(
                "Evaluation protocol version does not match worker".into(),
            ));
        }
        if identity.backend != self.fingerprint() {
            return Err(CadError::Invalid(format!(
                "Evaluation backend does not match worker: requested {}, running {}",
                identity.backend,
                self.fingerprint()
            )));
        }
        if identity.source != document.fingerprint() {
            return Err(CadError::Invalid(
                "Evaluation document fingerprint changed in transport".into(),
            ));
        }
        let solved = match self.build(document, root, tolerance) {
            Ok(solved) => solved,
            Err(error) => {
                if let Some(evaluation) = self.failure_evaluation(
                    identity.clone(),
                    document,
                    tolerance,
                    start.elapsed().as_millis().try_into().unwrap_or(u64::MAX),
                )? {
                    return Ok(evaluation);
                }
                return Err(error);
            }
        };
        let assemblies = self.assemblies.clone();
        let mut meshes = BTreeMap::new();
        let mut body_sets = BTreeMap::new();
        let mut surface_sets = BTreeMap::new();
        let mut sketches = BTreeMap::new();
        let mut references = BTreeMap::new();
        let mut reference_curves = BTreeMap::new();
        let mut reference_curve_meshes = BTreeMap::new();
        let mut mold_analyses = BTreeMap::new();
        let mut sheets = BTreeMap::new();
        let mut weldments = BTreeMap::new();
        let mut weld_beads = BTreeMap::new();
        let mut history = BTreeMap::new();
        for part in solved.parts.values() {
            self.focused_preview(part, &part.id, tolerance);
            for (feature, weldment) in self.weldment_output(&solved, &part.id)? {
                weldments.insert(format!("{}::{feature}", part.id), weldment);
            }
            for (feature, bead) in self.weld_bead_output(&solved, &part.id)? {
                weld_beads.insert(format!("{}::{feature}", part.id), bead);
            }
            for (feature, report) in self.mold_output(&solved, &part.id) {
                mold_analyses.insert(format!("{}::{feature}", part.id), report);
            }
            for (feature, sheet) in self.sheet_output(&solved, &part.id, tolerance)? {
                sheets.insert(format!("{}::{feature}", part.id), sheet);
            }
            for (feature, geometry) in self.reference_output(&solved, &part.id) {
                references.insert(format!("{}::{feature}", part.id), geometry);
            }
            for (feature, geometry, mesh) in self.curve_output(&solved, &part.id, tolerance)? {
                let key = format!("{}::{feature}", part.id);
                reference_curves.insert(key.clone(), geometry);
                reference_curve_meshes.insert(key, mesh);
            }
            for (feature, geometry) in self.sketch_output(&solved, &part.id)? {
                sketches.insert(format!("{}::{feature}", part.id), geometry);
            }
            history.insert(
                part.id.clone(),
                self.history_output(&solved, &part.id, &part.id, tolerance),
            );
        }
        for p in solved.parts.values().filter(|p| !p.outputs.is_empty()) {
            let (mesh, bodies, surfaces) = self.part_output(p, tolerance)?;
            meshes.insert(p.id.clone(), mesh);
            body_sets.insert(p.id.clone(), bodies);
            surface_sets.insert(p.id.clone(), surfaces);
        }
        let instances = cad_core::configuration::placed_parts(document, Some(&assemblies))?;
        for i in &instances {
            let key = i.mesh_key.clone();
            if !history.contains_key(&key) {
                let variant = cad_core::configuration::source_variant(
                    document,
                    &i.definition,
                    i.configuration.as_deref(),
                );
                let configured_solved = match self.build(&variant, root, tolerance) {
                    Ok(solved) => solved,
                    Err(error) => {
                        if let Some(evaluation) = self.failure_evaluation(
                            identity.clone(),
                            document,
                            tolerance,
                            start.elapsed().as_millis().try_into().unwrap_or(u64::MAX),
                        )? {
                            return Ok(evaluation);
                        }
                        return Err(error);
                    }
                };
                for (feature, geometry) in self.reference_output(&configured_solved, &i.definition)
                {
                    references.insert(format!("{key}::{feature}"), geometry);
                }
                for (feature, geometry, mesh) in
                    self.curve_output(&configured_solved, &i.definition, tolerance)?
                {
                    let key = format!("{key}::{feature}");
                    reference_curves.insert(key.clone(), geometry);
                    reference_curve_meshes.insert(key, mesh);
                }
                for (feature, report) in self.mold_output(&configured_solved, &i.definition) {
                    mold_analyses.insert(format!("{key}::{feature}"), report);
                }
                for (feature, sheet) in
                    self.sheet_output(&configured_solved, &i.definition, tolerance)?
                {
                    sheets.insert(format!("{key}::{feature}"), sheet);
                }
                for (feature, weldment) in
                    self.weldment_output(&configured_solved, &i.definition)?
                {
                    weldments.insert(format!("{key}::{feature}"), weldment);
                }
                for (feature, bead) in self.weld_bead_output(&configured_solved, &i.definition)? {
                    weld_beads.insert(format!("{key}::{feature}"), bead);
                }
                for (feature, geometry) in self.sketch_output(&configured_solved, &i.definition)? {
                    sketches.insert(format!("{key}::{feature}"), geometry);
                }
                history.insert(
                    key.clone(),
                    self.history_output(&configured_solved, &i.definition, &key, tolerance),
                );
                let part = &configured_solved.parts[&i.definition];
                self.focused_preview(part, &key, tolerance);
                if !part.outputs.is_empty() {
                    let (mesh, bodies, surfaces) = self.part_output(part, tolerance)?;
                    meshes.insert(key.clone(), mesh);
                    body_sets.insert(key.clone(), bodies);
                    surface_sets.insert(key, surfaces);
                }
            }
        }
        self.surface_sets = surface_sets;
        self.history_cache
            .retain(|key, _| history.contains_key(key));
        let properties = cad_core::inspection::evaluate(document, &meshes)?;
        let mut assembly_welds = BTreeMap::new();
        let occurrences = cad_core::assembly_welds::occurrences(document, &assemblies)?;
        for occurrence in &occurrences {
            let Some(assembly) = occurrence.context.assemblies.get(&occurrence.definition) else {
                continue;
            };
            for weld in assembly.weld_beads.values().filter(|w| !w.suppressed) {
                let owner = NodeRef {
                    definition: assembly.id.clone(),
                    node: weld.id.clone(),
                };
                let inverse = occurrence.matrix.try_inverse().ok_or_else(|| {
                    CadError::Invalid("Weld Bead: assembly pose is singular".into())
                })?;
                let mut contexts = BTreeMap::new();
                for (path, dependency) in
                    std::iter::once((weld.seam.path(), weld.seam.dependency()))
                        .chain(weld.supports.iter().map(|f| (f.path.as_slice(), &f.body)))
                {
                    let full_path = occurrence
                        .path
                        .iter()
                        .chain(path)
                        .cloned()
                        .collect::<Vec<_>>();
                    let leaf = occurrences
                        .iter()
                        .find(|o| o.path == full_path && o.definition == dependency.definition)
                        .ok_or_else(|| {
                            CadError::Invalid(format!(
                                "Weld Bead {}: seam or support component is inactive",
                                weld.name
                            ))
                        })?;
                    let relative = inverse * leaf.matrix;
                    contexts.insert(
                        path.to_vec(),
                        (
                            leaf.context.clone(),
                            std::array::from_fn(|r| std::array::from_fn(|c| relative[(r, c)])),
                        ),
                    );
                }
                let (mesh, bodies, bead) = self
                    .adapter
                    .assembly_weld(
                        &occurrence.context,
                        &owner,
                        weld,
                        &contexts,
                        root,
                        tolerance,
                    )
                    .map_err(|e| {
                        CadError::Invalid(format!("{} / {}: {e}", assembly.name, weld.name))
                    })?;
                let seam_context = &contexts[weld.seam.path()].0;
                let seam_part = &seam_context.parts[&weld.seam.dependency().definition];
                let material = weld
                    .material
                    .clone()
                    .unwrap_or_else(|| seam_part.material.clone());
                let metrics = cad_core::inspection::boundary_metrics(
                    &mesh,
                    document.materials.get(&material).map(|m| m.density_kg_m3),
                )?;
                let mut path = occurrence.path.clone();
                path.push(weld.id.clone());
                let key = format!("assembly-weld:{}", path.join("/"));
                let placement = cad_core::PlacedPart {
                    path,
                    definition: assembly.id.clone(),
                    name: weld.name.clone(),
                    matrix: std::array::from_fn(|r| {
                        std::array::from_fn(|c| occurrence.matrix[(r, c)])
                    }),
                    configuration: occurrence.configuration.clone(),
                    mesh_key: key.clone(),
                    bom_excluded: true,
                    finish: Some(
                        weld.finish
                            .clone()
                            .unwrap_or_else(|| seam_part.finish.clone()),
                    ),
                };
                body_sets.insert(key.clone(), bodies);
                meshes.insert(key.clone(), mesh);
                assembly_welds.insert(
                    key,
                    cad_core::assembly_welds::Evaluated {
                        version: 1,
                        owner,
                        placement,
                        bead,
                        properties: metrics,
                        material,
                    },
                );
            }
        }
        self.body_sets = body_sets;
        let configurations = Some(cad_core::configuration::evaluated(document)?);
        let exploded_views = cad_core::exploded_views::evaluate(document, &instances)?;
        let assembly_inspections = crate::assembly_inspection::evaluate(
            &self.adapter,
            document,
            &instances,
            &meshes,
            &self.body_sets,
        )?;
        let evaluated = Evaluation {
            identity,
            meshes,
            instances,
            diagnostics: vec![],
            valid: true,
            elapsed_ms: start.elapsed().as_millis().try_into().unwrap_or(u64::MAX),
            sketches,
            references,
            reference_curves,
            reference_curve_meshes,
            assemblies,
            exploded_views,
            assembly_inspections,
            properties,
            configurations,
            mold_analyses,
            sheets,
            weldments,
            weld_beads,
            assembly_welds,
            history: std::sync::Arc::new(history),
            operation_preview: None,
        };
        cad_core::drawing::validate_evaluation(document, &evaluated)?;
        Ok(evaluated)
    }
    fn export(
        &mut self,
        document: &Document,
        root: &Path,
        path: &Path,
        format: &str,
        tolerance: f64,
    ) -> Result<()> {
        let identity = RequestIdentity {
            protocol: PROTOCOL_VERSION,
            generation: 0,
            base_revision: document.revision,
            draft: None,
            edit: 0,
            source: document.fingerprint(),
            backend: self.fingerprint().into(),
        };
        let evaluated = self.evaluate(identity, document, root, tolerance)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        match format.to_ascii_lowercase().as_str() {
            "stl" => cad_core::manufacturing::stl(&evaluated, path),
            "pdf" => cad_core::manufacturing::drawing_pdf(document, &evaluated, path),
            "dxf" => {
                let solved = self.build(document, root, tolerance)?;
                cad_core::persistence::atomic_write(
                    path,
                    cad_core::manufacturing::dxf_profiles(&solved)?.as_bytes(),
                )
            }
            "drawing-dxf" => cad_core::persistence::atomic_write(
                path,
                cad_core::drawing::dxf(document, &evaluated)?.as_bytes(),
            ),
            "flat-pattern-dxf" => cad_core::persistence::atomic_write(
                path,
                cad_core::sheet_metal::dxf(&cad_core::sheet_metal::export_sheets(
                    document, &evaluated,
                )?)?
                .as_bytes(),
            ),
            "cutlist" => cad_core::persistence::atomic_write(
                path,
                cad_core::manufacturing::cut_list_csv_evaluated(document, &evaluated)?.as_bytes(),
            ),
            "csv" | "bom" => cad_core::persistence::atomic_write(
                path,
                cad_core::manufacturing::csv(document)?.as_bytes(),
            ),
            "json" => {
                cad_core::persistence::atomic_write(path, &serde_json::to_vec_pretty(&evaluated)?)
            }
            "step" | "stp" => crate::exchange::export_step(
                document,
                &evaluated.assemblies,
                &self.body_sets,
                &self.surface_sets,
                &evaluated.assembly_welds,
                path,
            ),
            _ => Err(CadError::Unsupported(format!("Export format {format}"))),
        }
    }
}
