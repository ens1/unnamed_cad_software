use crate::*;
use serde::{Deserialize, Serialize};

/// Stable document commands, independent of any editor or renderer.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "command", rename_all = "snake_case")]
pub enum Command {
    ReplaceDocument {
        document: Document,
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
    PutAssemblyInspection {
        assembly: Id,
        inspection: Box<crate::assembly_inspection::Definition>,
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
}
impl Command {
    pub fn execute(self, document: &mut Document) -> Result<()> {
        let missing = || CadError::Invalid("The command target no longer exists".into());
        match self {
            Self::RepairReferences { repairs } => {
                crate::reference_repair::repair(document, &repairs)?;
            }
            Self::ReplaceDocument { document: d } => {
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
            Self::PutAssemblyInspection {
                assembly,
                inspection,
            } => crate::assembly_inspection::put(document, &assembly, *inspection)?,
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
                    document
                        .assemblies
                        .get_mut(&a)
                        .ok_or_else(missing)?
                        .instances
                        .insert(
                            key.clone(),
                            Instance {
                                id: key,
                                name: part.name.clone(),
                                definition: part.id.clone(),
                                placement,
                                finish: None,
                                configuration: None,
                                fixed: false,
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
                if crate::component_pattern::generated(a).contains(&instance) {
                    return Err(CadError::Invalid("Suppress this driven occurrence in a configuration, or edit/delete its component pattern".into()));
                }
                if a.component_patterns.values().any(|p| p.seeds.contains(&instance) || matches!(&p.mode, crate::component_pattern::Mode::Circular { axis_reference: Some(r), .. } if r.instance == instance)) {
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
                document.materials.insert(material.id.clone(), material);
            }
            Self::PutFinish { finish } => {
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
                if document
                    .assemblies
                    .get(&assembly)
                    .is_some_and(|a| crate::component_pattern::generated(a).contains(&instance))
                {
                    return Err(CadError::Invalid("Make the pattern seed unique or detach its pattern before making a generated occurrence unique".into()));
                }
                let def = document
                    .assemblies
                    .get(&assembly)
                    .and_then(|a| a.instances.get(&instance))
                    .ok_or_else(missing)?
                    .definition
                    .clone();
                let mut part = document
                    .parts
                    .get(&def)
                    .ok_or_else(|| {
                        CadError::Unsupported(
                            "Make unique currently requires a part instance".into(),
                        )
                    })?
                    .clone();
                let new_id = id();
                let mapping: std::collections::BTreeMap<_, _> =
                    part.features.iter().map(|f| (f.id.clone(), id())).collect();
                // Rewrite only references to this definition; external references keep their source.
                let mut json = serde_json::to_value(&part)?;
                fn rewrite(
                    value: &mut serde_json::Value,
                    old: &str,
                    new: &str,
                    mapping: &std::collections::BTreeMap<String, String>,
                ) -> Result<()> {
                    let mut identities = mapping.clone();
                    identities.insert(old.into(), new.into());
                    crate::library::remap_owned_features(value, &identities, "")
                }
                rewrite(&mut json, &def, &new_id, &mapping)?;
                part = serde_json::from_value(json)?;
                part.id = new_id.clone();
                part.name.push_str(" copy");
                if let Some(configuration) = document.active_configurations.get(&def).cloned() {
                    document
                        .active_configurations
                        .insert(new_id.clone(), configuration);
                }

                document.parts.insert(new_id.clone(), part);
                // Retarget associative mate endpoints only when their stable instance
                // path enters the component being made unique; sibling instances keep
                // their original shared definition and feature identities.
                let mut endpoints = Vec::new();
                for owner in document.assemblies.values() {
                    for mate in &owner.mates {
                        for (first, participant, reference) in [
                            (true, &mate.first, mate.first_reference.as_ref()),
                            (false, &mate.second, mate.second_reference.as_ref()),
                        ] {
                            let Some(reference) = reference else {
                                continue;
                            };
                            let Some(component) = owner.instances.get(participant) else {
                                continue;
                            };
                            let mut reaches = owner.id == assembly && participant == &instance;
                            let mut definition = component.definition.clone();
                            for key in &reference.path {
                                let Some(nested) = document.assemblies.get(&definition) else {
                                    break;
                                };
                                reaches |= nested.id == assembly && key == &instance;
                                let Some(child) = nested.instances.get(key) else {
                                    break;
                                };
                                definition = child.definition.clone();
                            }
                            if reaches {
                                endpoints.push((owner.id.clone(), mate.id.clone(), first));
                            }
                        }
                    }
                }
                for (owner, mate, first) in endpoints {
                    let mate = document
                        .assemblies
                        .get_mut(&owner)
                        .and_then(|a| a.mates.iter_mut().find(|m| m.id == mate))
                        .ok_or_else(missing)?;
                    let reference = if first {
                        &mut mate.first_reference
                    } else {
                        &mut mate.second_reference
                    };
                    if let Some(source) = reference {
                        let mut json = serde_json::to_value(&*source)?;
                        rewrite(&mut json, &def, &new_id, &mapping)?;
                        *source = serde_json::from_value(json)?;
                    }
                }
                let mut advanced_endpoints = Vec::new();
                for owner in document.assemblies.values() {
                    for mate in &owner.mates {
                        if let Some(relationship) = &mate.relationship {
                            for (index, endpoint) in
                                relationship.endpoints().into_iter().enumerate()
                            {
                                let Some(component) = owner.instances.get(&endpoint.instance)
                                else {
                                    continue;
                                };
                                let mut reaches = owner.id == assembly
                                    && (endpoint.instance == instance
                                        || owner.component_patterns.values().any(|p| {
                                            p.occurrences.iter().any(|e| {
                                                e.instance == endpoint.instance
                                                    && e.seed == instance
                                            })
                                        }));
                                let mut definition = component.definition.clone();
                                for key in &endpoint.reference.path {
                                    let Some(nested) = document.assemblies.get(&definition) else {
                                        break;
                                    };
                                    reaches |= nested.id == assembly && key == &instance;
                                    let Some(child) = nested.instances.get(key) else {
                                        break;
                                    };
                                    definition = child.definition.clone();
                                }
                                if reaches {
                                    advanced_endpoints.push((
                                        owner.id.clone(),
                                        mate.id.clone(),
                                        index,
                                    ));
                                }
                            }
                        }
                    }
                }
                for (owner, mate, index) in advanced_endpoints {
                    let relation = document
                        .assemblies
                        .get_mut(&owner)
                        .and_then(|a| a.mates.iter_mut().find(|m| m.id == mate))
                        .and_then(|m| m.relationship.as_deref_mut())
                        .ok_or_else(missing)?;
                    let endpoint = relation
                        .endpoints_mut()
                        .into_iter()
                        .nth(index)
                        .ok_or_else(missing)?;
                    let mut json = serde_json::to_value(&endpoint.reference)?;
                    rewrite(&mut json, &def, &new_id, &mapping)?;
                    endpoint.reference = serde_json::from_value(json)?;
                }
                let mut pattern_references = Vec::new();
                for owner in document.assemblies.values() {
                    for pattern in owner.component_patterns.values() {
                        if let crate::component_pattern::Mode::Circular {
                            axis_reference: Some(reference),
                            ..
                        } = &pattern.mode
                        {
                            let Some(component) = owner.instances.get(&reference.instance) else {
                                continue;
                            };
                            let mut reaches = owner.id == assembly
                                && (reference.instance == instance
                                    || owner.component_patterns.values().any(|p| {
                                        p.occurrences.iter().any(|e| {
                                            e.instance == reference.instance && e.seed == instance
                                        })
                                    }));
                            let mut definition = component.definition.clone();
                            for key in &reference.geometry.path {
                                let Some(nested) = document.assemblies.get(&definition) else {
                                    break;
                                };
                                reaches |= nested.id == assembly && key == &instance;
                                let Some(child) = nested.instances.get(key) else {
                                    break;
                                };
                                definition = child.definition.clone();
                            }
                            if reaches {
                                pattern_references.push((owner.id.clone(), pattern.id.clone()));
                            }
                        }
                    }
                }
                for (owner, pattern) in pattern_references {
                    let pattern = document
                        .assemblies
                        .get_mut(&owner)
                        .and_then(|a| a.component_patterns.get_mut(&pattern))
                        .ok_or_else(missing)?;
                    if let crate::component_pattern::Mode::Circular {
                        axis_reference: Some(reference),
                        ..
                    } = &mut pattern.mode
                    {
                        let mut json = serde_json::to_value(&reference.geometry)?;
                        rewrite(&mut json, &def, &new_id, &mapping)?;
                        reference.geometry = serde_json::from_value(json)?;
                    }
                }
                retarget_drawing_occurrence(
                    document, &assembly, &instance, &def, &new_id, &mapping,
                )?;
                document
                    .assemblies
                    .get_mut(&assembly)
                    .ok_or_else(missing)?
                    .instances
                    .get_mut(&instance)
                    .ok_or_else(missing)?
                    .definition = new_id;
            }
        }
        crate::component_pattern::synchronize(document)?;
        crate::drawing_tables::reconcile_reservations(document)?;
        document.validate()
    }
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
        self.edit(|d| command.execute(d))
    }
}

/// Preserve associativity for drawing references explicitly attached to the
/// occurrence being made unique. A direct definition sheet and shared siblings
/// retain their original source; pathless attachments are never guessed.
fn retarget_drawing_occurrence(
    document: &mut Document,
    assembly: &str,
    instance: &str,
    old: &str,
    new: &str,
    nodes: &std::collections::BTreeMap<String, String>,
) -> Result<()> {
    use crate::drawing::{AnnotationKind, CurveReference, PointReference};
    fn path_enters(
        assemblies: &std::collections::BTreeMap<Id, Assembly>,
        root: &str,
        path: &[Id],
        owner: &str,
        selected: &str,
    ) -> bool {
        let mut definition = root;
        let mut reaches = false;
        for key in path {
            let Some(a) = assemblies.get(definition) else {
                return false;
            };
            if a.id == owner
                && (key == selected
                    || a.component_patterns.values().any(|p| {
                        p.occurrences
                            .iter()
                            .any(|e| &e.instance == key && e.seed == selected)
                    }))
            {
                reaches = true;
            }
            let Some(i) = a.instances.get(key) else {
                return false;
            };
            definition = &i.definition;
        }
        reaches
    }
    fn remap<T: serde::Serialize + serde::de::DeserializeOwned>(
        source: &mut T,
        ids: &std::collections::BTreeMap<Id, Id>,
    ) -> Result<()> {
        let mut json = serde_json::to_value(&*source)?;
        crate::library::remap_owned_features(&mut json, ids, "")?;
        *source = serde_json::from_value(json)?;
        Ok(())
    }
    fn point(
        source: &mut Option<PointReference>,
        reaches: &impl Fn(&[Id]) -> bool,
        ids: &std::collections::BTreeMap<Id, Id>,
    ) -> Result<bool> {
        let Some(reference) = source else {
            return Ok(false);
        };
        let path = match reference {
            PointReference::SketchPoint { instance_path, .. }
            | PointReference::EdgePoint { instance_path, .. }
            | PointReference::FaceOrigin { instance_path, .. } => instance_path,
        };
        if !reaches(path) {
            return Ok(false);
        }
        remap(reference, ids)?;
        Ok(true)
    }
    fn curve(
        source: &mut CurveReference,
        reaches: &impl Fn(&[Id]) -> bool,
        ids: &std::collections::BTreeMap<Id, Id>,
    ) -> Result<bool> {
        let path = match source {
            CurveReference::SketchMember { instance_path, .. }
            | CurveReference::ModelEdge { instance_path, .. } => instance_path,
        };
        if !reaches(path) {
            return Ok(false);
        }
        remap(source, ids)?;
        Ok(true)
    }
    let mut ids = nodes.clone();
    ids.insert(old.into(), new.into());
    let assemblies = document.assemblies.clone();
    let mut row_targets = std::collections::BTreeMap::new();
    for sheet in &mut document.drawings {
        let root = sheet.definition.clone();
        let reaches = |path: &[Id]| path_enters(&assemblies, &root, path, assembly, instance);
        for table in &mut sheet.tables {
            let retarget = if let crate::drawing_tables::TableKind::Holes {
                features,
                instance_path,
                datum,
                ..
            } = &mut table.kind
            {
                if reaches(instance_path) {
                    for feature in features {
                        remap(feature, &ids)?;
                    }
                    if let crate::drawing_tables::HoleDatum::Reference { point: reference } = datum
                    {
                        let mut reference = Some(reference.as_ref().clone());
                        point(&mut reference, &reaches, &ids)?;
                        *datum = crate::drawing_tables::HoleDatum::Reference {
                            point: reference
                                .ok_or_else(|| {
                                    CadError::Invalid(
                                        "Retargeted hole table datum is missing".into(),
                                    )
                                })?
                                .into(),
                        };
                    }
                    true
                } else {
                    false
                }
            } else {
                false
            };
            if retarget {
                row_targets.extend(crate::drawing_tables::retarget_reserved_rows(table, &ids)?);
            }
        }
        for dimension in &mut sheet.dimensions {
            let mut changed = point(&mut dimension.options.first, &reaches, &ids)?;
            changed |= point(&mut dimension.options.second, &reaches, &ids)?;
            changed |= point(&mut dimension.options.third, &reaches, &ids)?;
            if let Some(reference) = &mut dimension.options.curve {
                changed |= curve(reference, &reaches, &ids)?;
            }
            if changed && let Some(feature) = &mut dimension.options.hole_feature {
                remap(feature, &ids)?;
            }
        }
        for annotation in &mut sheet.entities {
            match &mut annotation.kind {
                AnnotationKind::Balloon {
                    target, definition, ..
                } => {
                    if point(&mut target.reference, &reaches, &ids)? && definition == old {
                        *definition = new.into();
                    }
                }
                AnnotationKind::Leader { target, .. }
                | AnnotationKind::Datum { target, .. }
                | AnnotationKind::SurfaceFinish { target, .. }
                | AnnotationKind::WeldSymbol { target, .. }
                | AnnotationKind::GeometricTolerance { target, .. } => {
                    point(&mut target.reference, &reaches, &ids)?;
                }
                AnnotationKind::CenterLine { first, second, .. } => {
                    point(&mut first.reference, &reaches, &ids)?;
                    point(&mut second.reference, &reaches, &ids)?;
                }
                AnnotationKind::CenterMark {
                    curve: reference, ..
                } => {
                    curve(reference, &reaches, &ids)?;
                }
            }
        }
    }
    for comment in &mut document.comments {
        if let Some(target) = row_targets.get(&comment.target) {
            comment.target = target.clone();
        }
    }
    Ok(())
}
