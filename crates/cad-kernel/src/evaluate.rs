use crate::adapter::{KernelAdapter, MonstertruckAdapter, Shape};
use cad_core::{
    CadError, Document, NodeRef, Result,
    protocol::*,
    solver::{ConstraintSolver, NativeSolver},
};
use std::{collections::BTreeMap, path::Path, time::Instant};
pub struct Evaluator<K: KernelAdapter = MonstertruckAdapter> {
    pub adapter: K,
    pub shapes: BTreeMap<NodeRef, Shape>,
    pub body_sets: BTreeMap<String, Vec<monstertruck::modeling::Solid>>,
    pub surface_sets: BTreeMap<String, Vec<monstertruck::modeling::Shell>>,
    assemblies: BTreeMap<String, cad_core::assembly_geometry::EvaluatedAssembly>,
    last_source: String,
    last_tolerance: f64,
    history_cache: BTreeMap<String, (String, f64, cad_core::history::PartHistory)>,
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
        }
    }
}
impl<K: KernelAdapter> Evaluator<K> {
    fn build(&mut self, doc: &Document, root: &Path, tolerance: f64) -> Result<Document> {
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
            let active = part
                .features
                .iter()
                .filter(|f| !f.suppressed)
                .filter_map(|f| match &f.operation {
                    cad_core::Operation::Profile {
                        profile: cad_core::Profile::Sketch { sketch },
                        ..
                    } => Some(sketch),
                    _ => None,
                })
                .collect::<std::collections::BTreeSet<_>>();
            for sketch in part
                .sketches
                .values()
                .filter(|s| s.conversions.is_empty() && active.contains(&s.id))
            {
                let mut sketch = sketch.clone();
                NativeSolver.solve_sketch(&solved, &part.id, &mut sketch)?;
            }
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
        let shapes =
            crate::assemblies::build_shapes(&self.adapter, &solved, root, tolerance, true)?;
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
            crate::assemblies::build_shapes(&self.adapter, &solved, root, tolerance, false)?
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
        let solved = self.build(document, root, tolerance)?;
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
        let mut history = BTreeMap::new();
        for part in solved.parts.values() {
            for (feature, weldment) in self.weldment_output(&solved, &part.id)? {
                weldments.insert(format!("{}::{feature}", part.id), weldment);
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
                let configured_solved = self.build(&variant, root, tolerance)?;
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
                for (feature, geometry) in self.sketch_output(&configured_solved, &i.definition)? {
                    sketches.insert(format!("{key}::{feature}"), geometry);
                }
                history.insert(
                    key.clone(),
                    self.history_output(&configured_solved, &i.definition, &key, tolerance),
                );
                let part = &configured_solved.parts[&i.definition];
                if !part.outputs.is_empty() {
                    let (mesh, bodies, surfaces) = self.part_output(part, tolerance)?;
                    meshes.insert(key.clone(), mesh);
                    body_sets.insert(key.clone(), bodies);
                    surface_sets.insert(key, surfaces);
                }
            }
        }
        self.body_sets = body_sets;
        self.surface_sets = surface_sets;
        self.history_cache
            .retain(|key, _| history.contains_key(key));
        let properties = cad_core::inspection::evaluate(document, &meshes)?;
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
            history: std::sync::Arc::new(history),
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
                path,
            ),
            _ => Err(CadError::Unsupported(format!("Export format {format}"))),
        }
    }
}
