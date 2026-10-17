use crate::*;
use serde::{Deserialize, Serialize};

/// Stable document commands, independent of any editor or renderer.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "command", rename_all = "snake_case")]
pub enum Command {
    InsertStandardPart {
        insertion: Box<crate::standard_parts::Insertion>,
    },
    EditModelProperties {
        edit: crate::model_properties::Edit,
    },
    ReplaceDocument {
        document: Document,
    },
    ReplaceLinkedComponentSnapshots {
        document: Document,
        snapshots: Vec<crate::library::SnapshotProof>,
    },
    RepairReferences {
        repairs: Vec<crate::reference_repair::Repair>,
    },
    SetParameter {
        definition: Id,
        parameter: Id,
        value: Expr,
    },
    PutParameter {
        definition: Id,
        parameter: Id,
        value: Parameter,
    },
    PutExplodedView {
        assembly: Id,
        view: Box<crate::exploded_views::Definition>,
    },
    DeleteExplodedView {
        assembly: Id,
        view: Id,
    },
    PutDisplayState {
        assembly: Id,
        state: Box<crate::display_states::Definition>,
    },
    DeleteDisplayState {
        assembly: Id,
        state: Id,
    },
    PutAssemblyInspection {
        assembly: Id,
        inspection: Box<crate::assembly_inspection::Definition>,
    },
    PutAssemblyWeld {
        assembly: Id,
        weld: Box<crate::weld_beads::Definition>,
    },
    DeleteAssemblyWeld {
        assembly: Id,
        weld: Id,
    },
    PutAssemblyGroup {
        assembly: Id,
        group: Box<crate::assembly_groups::Definition>,
    },
    DeleteAssemblyGroup {
        assembly: Id,
        group: Id,
    },
    SuppressAssemblyGroup {
        assembly: Id,
        group: Id,
        suppressed: bool,
        scope: crate::configuration::PlacementScope,
    },
    DeleteAssemblyInspection {
        assembly: Id,
        inspection: Id,
    },
    PutComponentPattern {
        assembly: Id,
        pattern: Box<crate::component_pattern::Definition>,
    },
    DeleteComponentPattern {
        assembly: Id,
        pattern: Id,
    },
    PutConfiguration {
        definition: Id,
        configuration: Box<crate::configuration::Definition>,
    },
    DeleteConfiguration {
        definition: Id,
        configuration: Id,
    },
    SelectConfiguration {
        definition: Id,
        configuration: Option<Id>,
    },
    ImportConfigurationTable {
        table: Box<crate::configuration::Table>,
        replace: bool,
    },
    PutFeature {
        definition: Id,
        feature: Box<Feature>,
        output: bool,
    },
    DeleteFeature {
        definition: Id,
        feature: Id,
    },
    ReorderFeature {
        definition: Id,
        feature: Id,
        position: usize,
    },
    ReorderFeatureRelative {
        definition: Id,
        feature: Id,
        anchor: Id,
        before: bool,
    },
    SuppressFeature {
        definition: Id,
        feature: Id,
        suppressed: bool,
    },
    PutPart {
        part: Part,
        assembly: Option<Id>,
        placement: Placement,
    },
    PutInstance {
        assembly: Id,
        instance: Instance,
    },
    DeleteInstance {
        assembly: Id,
        instance: Id,
    },
    PutSketch {
        definition: Id,
        sketch: Sketch,
    },
    PutMate {
        assembly: Id,
        mate: Box<Mate>,
    },
    DeleteMate {
        assembly: Id,
        mate: Id,
    },
    PutMaterial {
        material: Material,
    },
    PutCameraView {
        view: Box<crate::camera_views::Definition>,
    },
    DeleteCameraView {
        view: Id,
    },
    PutSensor {
        sensor: Box<crate::sensors::Definition>,
    },
    DeleteSensor {
        sensor: Id,
    },
    PutToleranceStack {
        stack: Box<crate::tolerance_stacks::Definition>,
    },
    DeleteToleranceStack {
        stack: Id,
    },
    ImportMaterialLibrary {
        catalog: Box<crate::material_library::Catalog>,
        collision: crate::material_library::CollisionPolicy,
    },
    DeleteMaterial {
        material: Id,
    },
    SetPartMaterial {
        definition: Id,
        material: Id,
    },
    PutFinish {
        finish: Box<crate::appearance::Finish>,
    },
    DeleteFinish {
        finish: Id,
    },
    SetPartFinish {
        definition: Id,
        finish: Id,
    },
    SetFaceFinish {
        definition: Id,
        assignment: Box<crate::appearance::FaceFinish>,
    },
    DeleteFaceFinish {
        definition: Id,
        assignment: Id,
    },
    SetInstanceFinish {
        assembly: Id,
        instance: Id,
        finish: Option<Id>,
    },
    SetLighting {
        lighting: crate::appearance::Lighting,
    },
    SetModelSettings {
        settings: crate::settings::ModelSettings,
    },
    DeleteDrawing {
        drawing: Id,
    },
    MoveDrawing {
        drawing: Id,
        index: usize,
    },
    DuplicateDrawing {
        drawing: Id,
        name: String,
    },
    PutDrawing {
        drawing: Drawing,
    },
    PutComment {
        comment: ReviewComment,
    },
    MakeUnique {
        assembly: Id,
        instance: Id,
    },
    ForkComponent {
        intent: crate::component_fork::Intent,
    },
}
impl Command {
    pub fn execute(self, document: &mut Document) -> Result<()> {
        let missing = || CadError::Invalid("The command target no longer exists".into());
        match self {
            Self::InsertStandardPart { insertion } => {
                crate::standard_parts::insert(document, &insertion)?;
            }
            Self::EditModelProperties { edit } => {
                crate::model_properties::apply(document, edit)?;
            }
            Self::RepairReferences { repairs } => {
                crate::reference_repair::repair(document, &repairs)?;
            }
            Self::ReplaceDocument { document: d } => {
                crate::library::validate_replacement(document, &d)?;
                let revision = document.revision;
                *document = d;
                document.revision = revision;
            }
            Self::ReplaceLinkedComponentSnapshots {
                document: d,
                snapshots,
            } => {
                crate::library::validate_verified_replacement(document, &d, &snapshots)?;
                let revision = document.revision;
                *document = d;
                document.revision = revision;
            }
            Self::SetParameter {
                definition,
                parameter,
                value,
            } => {
                let parameters = if let Some(p) = document.parts.get_mut(&definition) {
                    &mut p.parameters
                } else {
                    &mut document
                        .assemblies
                        .get_mut(&definition)
                        .ok_or_else(missing)?
                        .parameters
                };
                parameters
                    .get_mut(&parameter)
                    .ok_or_else(missing)?
                    .expression = value;
            }
            Self::PutParameter {
                definition,
                parameter,
                value,
            } => {
                let parameters = if let Some(p) = document.parts.get_mut(&definition) {
                    &mut p.parameters
                } else {
                    &mut document
                        .assemblies
                        .get_mut(&definition)
                        .ok_or_else(missing)?
                        .parameters
                };
                parameters.insert(parameter, value);
            }
            Self::PutExplodedView { assembly, view } => {
                crate::exploded_views::put(document, &assembly, *view)?
            }
            Self::DeleteExplodedView { assembly, view } => {
                crate::exploded_views::remove(document, &assembly, &view)?
            }
            Self::PutDisplayState { assembly, state } => {
                crate::display_states::put(document, &assembly, *state)?
            }
            Self::DeleteDisplayState { assembly, state } => {
                crate::display_states::remove(document, &assembly, &state)?
            }
            Self::PutAssemblyInspection {
                assembly,
                inspection,
            } => crate::assembly_inspection::put(document, &assembly, *inspection)?,
            Self::PutAssemblyWeld { assembly, weld } => {
                crate::assembly_welds::put(document, &assembly, *weld)?
            }
            Self::DeleteAssemblyWeld { assembly, weld } => {
                crate::assembly_welds::remove(document, &assembly, &weld)?
            }
            Self::PutAssemblyGroup { assembly, group } => {
                crate::assembly_groups::put(document, &assembly, *group)?
            }
            Self::DeleteAssemblyGroup { assembly, group } => {
                crate::assembly_groups::remove(document, &assembly, &group)?
            }
            Self::SuppressAssemblyGroup {
                assembly,
                group,
                suppressed,
                scope,
            } => crate::assembly_groups::set_suppressed(
                document, &assembly, &group, suppressed, &scope,
            )?,
            Self::DeleteAssemblyInspection {
                assembly,
                inspection,
            } => crate::assembly_inspection::remove(document, &assembly, &inspection)?,
            Self::PutComponentPattern { assembly, pattern } => {
                crate::component_pattern::put(document, &assembly, *pattern)?
            }
            Self::DeleteComponentPattern { assembly, pattern } => {
                crate::component_pattern::remove(document, &assembly, &pattern)?
            }
            Self::PutConfiguration {
                definition,
                configuration,
            } => crate::configuration::put(document, &definition, *configuration)?,
            Self::DeleteConfiguration {
                definition,
                configuration,
            } => crate::configuration::remove(document, &definition, &configuration)?,
            Self::SelectConfiguration {
                definition,
                configuration,
            } => {
                if let Some(configuration) = configuration {
                    if !crate::configuration::exists(document, &definition, &configuration) {
                        return Err(CadError::Invalid(
                            "The selected configuration is missing".into(),
                        ));
                    }
                    document
                        .active_configurations
                        .insert(definition, configuration);
                } else {
                    document.active_configurations.remove(&definition);
                }
            }
            Self::ImportConfigurationTable { table, replace } => {
                crate::configuration::import(document, *table, replace)?
            }
            Self::PutFeature {
                definition,
                feature,
                output,
            } => {
                let feature = *feature;
                let p = document.parts.get_mut(&definition).ok_or_else(missing)?;
                let live = output || consuming_closure(p).contains(&feature.id);
                let previous = p
                    .features
                    .iter()
                    .find(|f| f.id == feature.id)
                    .map(|f| f.operation.consumed_outputs())
                    .unwrap_or_default();
                if let Some(f) = p.features.iter_mut().find(|f| f.id == feature.id) {
                    *f = feature.clone();
                } else {
                    p.features.push(feature.clone());
                }
                if live {
                    let consumed = feature.operation.consumed_outputs();
                    p.outputs.retain(|id| {
                        !consumed
                            .iter()
                            .any(|r| r.definition == p.id && &r.node == id)
                    });
                    if output && !p.outputs.contains(&feature.id) {
                        p.outputs.push(feature.id.clone());
                    }
                    let consumed_now = consuming_closure(p);
                    for reference in previous {
                        if reference.definition == p.id
                            && !consumed.iter().any(|r| r == &reference)
                            && !consumed_now.contains(&reference.node)
                            && !p.outputs.contains(&reference.node)
                            && p.features.iter().any(|f| {
                                f.id == reference.node
                                    && !f.operation.is_reference()
                                    && !matches!(f.operation, Operation::Profile { .. })
                            })
                        {
                            p.outputs.push(reference.node);
                        }
                    }
                    p.outputs
                        .sort_by_key(|id| p.features.iter().position(|f| &f.id == id));
                }
            }
            Self::DeleteFeature {
                definition,
                feature,
            } => {
                let p = document.parts.get_mut(&definition).ok_or_else(missing)?;
                p.features.retain(|f| f.id != feature);
                p.outputs.retain(|f| f != &feature);
                // Validation rejects deletion when a consumer still references this node.
            }
            Self::ReorderFeature {
                definition,
                feature,
                position,
            } => {
                crate::feature_order::move_feature(document, &definition, &feature, position)?;
            }
            Self::ReorderFeatureRelative {
                definition,
                feature,
                anchor,
                before,
            } => {
                let part = document.parts.get(&definition).ok_or_else(missing)?;
                let position =
                    crate::feature_order::relative_destination(part, &feature, &anchor, before)?;
                crate::feature_order::move_feature(document, &definition, &feature, position)?;
            }
            Self::SuppressFeature {
                definition,
                feature,
                suppressed,
            } => {
                if let Some(configuration) =
                    document.active_configurations.get(&definition).cloned()
                {
                    let mut c = crate::configuration::definitions(document, &definition)
                        .get(&configuration)
                        .cloned()
                        .ok_or_else(missing)?;
                    c.overrides.features.insert(feature.clone(), suppressed);
                    if let Some(crate::Operation::Profile {
                        profile: crate::Profile::Sketch { sketch },
                        ..
                    }) = document
                        .parts
                        .get(&definition)
                        .and_then(|p| p.features.iter().find(|f| f.id == feature))
                        .map(|f| &f.operation)
                    {
                        c.overrides.sketches.insert(sketch.clone(), suppressed);
                    }
                    crate::configuration::put(document, &definition, c)?;
                } else {
                    document
                        .parts
                        .get_mut(&definition)
                        .and_then(|p| p.features.iter_mut().find(|f| f.id == feature))
                        .ok_or_else(missing)?
                        .suppressed = suppressed;
                }
            }
            Self::PutPart {
                part,
                assembly,
                placement,
            } => {
                if let Some(a) = assembly {
                    let key = id();
                    let assembly = document.assemblies.get_mut(&a).ok_or_else(missing)?;
                    let fixed = assembly.instances.is_empty();
                    assembly.instances.insert(
                        key.clone(),
                        Instance {
                            id: key,
                            name: part.name.clone(),
                            definition: part.id.clone(),
                            placement,
                            finish: None,
                            configuration: None,
                            fixed,
                            translation_expressions: None,
                        },
                    );
                }
                document.parts.insert(part.id.clone(), part);
            }
            Self::PutInstance { assembly, instance } => {
                if document
                    .assemblies
                    .get(&assembly)
                    .is_some_and(|a| crate::component_pattern::generated(a).contains(&instance.id))
                {
                    return Err(CadError::Invalid("This occurrence is driven by a component pattern; edit the seed, pattern, or configuration override".into()));
                }
                document
                    .assemblies
                    .get_mut(&assembly)
                    .ok_or_else(missing)?
                    .instances
                    .insert(instance.id.clone(), instance);
            }
            Self::DeleteInstance { assembly, instance } => {
                let a = document.assemblies.get_mut(&assembly).ok_or_else(missing)?;
                if a.rigid_groups
                    .values()
                    .any(|group| group.members.contains_key(&instance))
                {
                    return Err(CadError::Invalid("Remove this component from its saved rigid group or dissolve that group before deleting it".into()));
                }
                if crate::component_pattern::generated(a).contains(&instance) {
                    return Err(CadError::Invalid("Suppress this driven occurrence in a configuration, or edit/delete its component pattern".into()));
                }
                if a.component_patterns.values().any(|p| {
                    p.seeds.contains(&instance)
                        || p.mode.references().iter().any(|r| r.instance == instance)
                }) {
                    return Err(CadError::Invalid("Update or delete the component patterns using this seed or axis component first".into()));
                }
                let removed_mates = a
                    .mates
                    .iter()
                    .filter(|m| m.participants().contains(&&instance))
                    .map(|m| m.id.clone())
                    .collect::<std::collections::BTreeSet<_>>();
                a.instances.remove(&instance);
                a.mates.retain(|m| !removed_mates.contains(&m.id));
                for c in a.configurations.values_mut() {
                    c.overrides.components.remove(&instance);
                    c.overrides.component_configurations.remove(&instance);
                    c.overrides.component_placements.remove(&instance);
                    c.overrides
                        .mate_curve_seeds
                        .retain(|id, _| !removed_mates.contains(id));
                    c.overrides
                        .mates
                        .retain(|id, _| !removed_mates.contains(id));
                }
            }
            Self::PutSketch { definition, sketch } => {
                document
                    .parts
                    .get_mut(&definition)
                    .ok_or_else(missing)?
                    .sketches
                    .insert(sketch.id.clone(), sketch);
            }
            Self::PutMate { assembly, mate } => {
                let a = document.assemblies.get_mut(&assembly).ok_or_else(missing)?;
                if let Some(m) = a.mates.iter_mut().find(|m| m.id == mate.id) {
                    *m = *mate;
                } else {
                    a.mates.push(*mate);
                }
            }
            Self::DeleteMate { assembly, mate } => {
                let a = document.assemblies.get_mut(&assembly).ok_or_else(missing)?;
                a.mates.retain(|m| m.id != mate);
                for c in a.configurations.values_mut() {
                    c.overrides.mates.remove(&mate);
                    c.overrides.mate_curve_seeds.remove(&mate);
                }
            }
            Self::PutMaterial { material } => {
                crate::materials::put_edited(document, material)?;
            }
            Self::PutCameraView { view } => crate::camera_views::put(document, *view)?,
            Self::DeleteCameraView { view } => crate::camera_views::delete(document, &view)?,
            Self::PutSensor { sensor } => crate::sensors::put(document, *sensor)?,
            Self::PutToleranceStack { stack } => crate::tolerance_stacks::put(document, *stack)?,
            Self::DeleteToleranceStack { stack } => {
                document
                    .tolerance_stacks
                    .remove(&stack)
                    .ok_or_else(missing)?;
            }
            Self::DeleteSensor { sensor } => {
                document.sensors.remove(&sensor).ok_or_else(missing)?;
            }
            Self::ImportMaterialLibrary { catalog, collision } => {
                let plan = crate::material_library::plan_import(document, &catalog, collision)?;
                crate::material_library::apply_import(document, plan)?;
            }
            Self::DeleteMaterial { material } => {
                crate::materials::delete_material(document, &material)?
            }
            Self::SetPartMaterial {
                definition,
                material,
            } => crate::materials::assign(document, &definition, &material)?,
            Self::PutFinish { finish } => {
                if crate::library::finish_is_pinned(document, &finish.id)
                    && document.finishes.get(&finish.id) != Some(finish.as_ref())
                {
                    return Err(CadError::Invalid("This finish belongs to an immutable component snapshot; duplicate it before editing a unique component".into()));
                }
                crate::appearance::validate_finish(document, &finish)?;
                document.finishes.insert(finish.id.clone(), *finish);
            }
            Self::DeleteFinish { finish } => crate::appearance::delete_finish(document, &finish)?,
            Self::SetPartFinish { definition, finish } => {
                if !document.finishes.contains_key(&finish) {
                    return Err(CadError::Invalid(
                        "The selected finish does not exist".into(),
                    ));
                }
                document
                    .parts
                    .get_mut(&definition)
                    .ok_or_else(missing)?
                    .finish = finish;
            }
            Self::SetInstanceFinish {
                assembly,
                instance,
                finish,
            } => {
                if finish
                    .as_ref()
                    .is_some_and(|id| !document.finishes.contains_key(id))
                {
                    return Err(CadError::Invalid(
                        "The selected finish does not exist".into(),
                    ));
                }
                document
                    .assemblies
                    .get_mut(&assembly)
                    .and_then(|a| a.instances.get_mut(&instance))
                    .ok_or_else(missing)?
                    .finish = finish;
            }
            Self::SetFaceFinish {
                definition,
                assignment,
            } => {
                crate::appearance::set_face_finish(document, &definition, *assignment)?;
            }
            Self::DeleteFaceFinish {
                definition,
                assignment,
            } => {
                crate::appearance::delete_face_finish(document, &definition, &assignment)?;
            }
            Self::SetLighting { lighting } => {
                crate::appearance::validate_lighting(&lighting)?;
                document.lighting = lighting;
            }
            Self::SetModelSettings { settings } => {
                settings.validate(document)?;
                document.settings = settings;
            }
            Self::PutDrawing { drawing } => crate::drawing::put_sheet(document, drawing)?,
            Self::DeleteDrawing { drawing } => crate::drawing::delete_sheet(document, &drawing)?,
            Self::MoveDrawing { drawing, index } => {
                crate::drawing::move_sheet(document, &drawing, index)?
            }
            Self::DuplicateDrawing { drawing, name } => {
                crate::drawing::duplicate_sheet(document, &drawing, name)?;
            }
            Self::PutComment { comment } => {
                if let Some(d) = document.comments.iter_mut().find(|d| d.id == comment.id) {
                    *d = comment;
                } else {
                    document.comments.push(comment);
                }
            }
            Self::MakeUnique { assembly, instance } => {
                let paths =
                    crate::component_fork::occurrence_paths(document, &assembly, &instance)?;
                if paths.len() != 1 {
                    return Err(CadError::Invalid("Make independent requires one exact occurrence. Use ForkComponent with an explicit root occurrence path when the enclosing assembly is reused".into()));
                }
                let definition = document
                    .assemblies
                    .get(&assembly)
                    .and_then(|owner| owner.instances.get(&instance))
                    .ok_or_else(missing)?
                    .definition
                    .clone();
                let intent = crate::component_fork::Intent::new(
                    document,
                    &definition,
                    crate::component_fork::Target::Occurrence {
                        path: paths[0].clone(),
                    },
                );
                apply_fork(document, &intent)?;
            }
            Self::ForkComponent { intent } => apply_fork(document, &intent)?,
        }
        crate::component_pattern::synchronize(document)?;
        crate::drawing_tables::reconcile_reservations(document)?;
        document.validate()
    }
}
/// Commands have no filesystem authority. A linked fork must first verify its
/// pinned resource through the library-aware planner and shared replacement draft.
fn apply_fork(document: &mut Document, intent: &crate::component_fork::Intent) -> Result<()> {
    let plan = crate::component_fork::plan(document, intent)?;
    if plan
        .copied
        .iter()
        .any(|change| crate::library::is_definition_pinned(document, &change.source))
    {
        return Err(CadError::Invalid("Linked component forks require the verified library snapshot. Use the component fork editor or plan_with_resources before applying the replacement document".into()));
    }
    *document = plan.document;
    Ok(())
}
/// Follow only consuming body inputs; datum/reference/copy inputs do not hide a source.
fn consuming_closure(part: &Part) -> std::collections::BTreeSet<Id> {
    let mut result = std::collections::BTreeSet::new();
    let mut pending = part.outputs.clone();
    while let Some(node) = pending.pop() {
        if !result.insert(node.clone()) {
            continue;
        }
        if let Some(feature) = part.features.iter().find(|f| f.id == node) {
            pending.extend(
                feature
                    .operation
                    .consumed_outputs()
                    .into_iter()
                    .filter(|r| r.definition == part.id)
                    .map(|r| r.node),
            );
        }
    }
    result
}
impl Session {
    pub fn dispatch(&mut self, command: Command) -> Result<()> {
        self.last_dispatch_reused_evaluation = false;
        let metadata_only = match &command {
            Command::PutInstance { assembly, instance } => self
                .visible()
                .assemblies
                .get(assembly)
                .and_then(|assembly| assembly.instances.get(&instance.id))
                .is_some_and(|previous| {
                    let mut renamed = previous.clone();
                    renamed.name = instance.name.clone();
                    previous.name != instance.name && renamed == *instance
                }),
            Command::SetModelSettings { settings } => {
                let previous = &self.visible().settings;
                let mut reporting_only = previous.clone();
                reporting_only.production = settings.production.clone();
                previous.production != settings.production && reporting_only == *settings
            }
            _ => false,
        };
        // Occurrence names and production table preferences cannot change
        // geometry. Reuse only an exact current validated result; pending,
        // stale and invalid drafts always take the worker path.
        let had_draft = self.draft.is_some();
        let evaluation = metadata_only
            .then(|| {
                let identity = self.identity();
                self.draft
                    .as_ref()
                    .and_then(|draft| (!draft.pending).then_some(draft))
                    .and_then(|draft| draft.evaluation.as_ref())
                    .or_else(|| {
                        self.draft
                            .is_none()
                            .then_some(self.evaluation.as_ref())
                            .flatten()
                    })
                    .filter(|evaluation| evaluation.valid && evaluation.identity == identity)
                    .cloned()
            })
            .flatten();
        self.edit(|d| command.execute(d))?;
        if let Some(mut evaluation) = evaluation {
            evaluation.identity = self.identity();
            evaluation.elapsed_ms = 0;
            if !had_draft {
                evaluation.operation_preview = None;
            }
            for instance in &mut evaluation.instances {
                if let Some(name) = crate::occurrence_names::path(self.visible(), &instance.path) {
                    instance.name = name;
                }
            }
            for report in evaluation.assembly_inspections.values_mut() {
                for occurrence in &mut report.occurrences {
                    if let Some(name) = crate::occurrence_names::path_from(
                        self.visible(),
                        &report.assembly,
                        &occurrence.target.path,
                    ) {
                        occurrence.name = name;
                    }
                }
            }
            self.accept(evaluation);
            self.last_dispatch_reused_evaluation = true;
        }
        Ok(())
    }
}
