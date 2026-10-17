use crate::{Document, Id, Session};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
mod layout;
mod semantic;
mod style;
pub use semantic::{ActionDescriptor, ValueDescriptor, ValueKind};
pub use style::{SUPPORTED_STYLE_PROPERTIES, StyleNode, StyleTarget};
pub const SEMANTIC_CONTRACT_VERSION: u32 = 1;
pub const LAYOUT_SCHEMA_VERSION: u32 = 1;
pub const STYLE_SCHEMA_VERSION: u32 = 1;
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SemanticElement {
    pub id: Id,
    pub role: String,
    pub label: String,
    pub value: Option<String>,
    pub state: String,
    pub commands: Vec<String>,
    pub relationships: Vec<Id>,
    #[serde(default)]
    pub data: serde_json::Value,
}
pub fn project(session: &Session, active: Option<&str>) -> Vec<SemanticElement> {
    let mut out = vec![SemanticElement {
        id: "document".into(),
        role: "document-context".into(),
        label: session.visible().name.clone(),
        value: Some(session.committed.revision.to_string()),
        state: "ready".into(),
        commands: vec!["file.save".into(), "file.open".into()],
        relationships: vec![],
        data: serde_json::Value::Null,
    }];
    let doc = session.visible();
    out.extend(crate::library::semantic(doc));
    out.extend(crate::appearance::semantic(
        doc,
        active,
        if session.draft.is_some() {
            "draft"
        } else {
            "committed"
        },
    ));
    out.extend(crate::configuration::semantic(
        doc,
        session
            .draft
            .as_ref()
            .and_then(|d| d.preview_evaluation())
            .or(session.evaluation.as_ref())
            .and_then(|e| e.configurations.as_ref()),
        if session.draft.is_some() {
            "draft"
        } else {
            "committed"
        },
    ));
    let property_evaluation = session
        .draft
        .as_ref()
        .and_then(|draft| draft.preview_evaluation())
        .or(session.evaluation.as_ref());
    if let Some(evaluation) = property_evaluation {
        for (key, curve) in &evaluation.reference_curves {
            if active.is_none_or(|definition| definition == curve.producer.definition) {
                out.push(SemanticElement {
                    id:format!("reference-curve:{key}"),role:"reference-curve-evaluation".into(),label:curve.kind.clone(),value:Some(format!("{} native curve segments",curve.pieces.len())),state:if session.draft.as_ref().is_some_and(|draft| draft.pending || draft.error.is_some()) {"last-valid"} else {"evaluated"}.into(),commands:vec!["feature.edit".into()],relationships:vec![curve.producer.definition.clone(),curve.producer.node.clone()],data:serde_json::json!({"contract":"tcad.reference-curves","version":crate::reference_curves::CONTRACT_VERSION,"output_key":key,"geometry":curve,"target":{"kind":"feature","definition":curve.producer.definition,"feature":curve.producer.node},"display_cache_is_authority":false}),
                });
            }
        }
        for (key, sheet) in &evaluation.sheets {
            if active.is_none_or(|definition| definition == sheet.source.definition) {
                out.push(SemanticElement { id: format!("fabrication:sheet:{key}"), role: "sheet-manufacturing-data".into(), label: "Sheet / Flat Pattern".into(), value: Some(format!("{} mm · {} bends",sheet.thickness_mm,sheet.bends.len())), state: if session.draft.as_ref().is_some_and(|draft|draft.pending || draft.error.is_some()) {"last-valid"}else{"evaluated"}.into(), commands: vec!["feature.edit".into(),"file.export-flat-pattern".into()], relationships: vec![sheet.source.definition.clone(),sheet.source.node.clone(),sheet.base_source.node.clone(),sheet.profile.node.clone()], data: serde_json::json!({"contract":"tcad.sheet_metal","version":crate::sheet_metal::CONTRACT_VERSION,"output_key":key,"manufacturing":sheet}) });
            }
        }
        for (key, weldment) in &evaluation.weldments {
            if active.is_none_or(|definition| definition == weldment.source.definition) {
                out.push(SemanticElement { id: format!("fabrication:weldment:{key}"), role: "weldment-manufacturing-data".into(), label: "Member Stock / Cut List".into(), value: Some(format!("{} members · {} end caps · {} gussets",weldment.members.len(),weldment.cap_count,weldment.gusset_count)), state: if session.draft.as_ref().is_some_and(|draft|draft.pending || draft.error.is_some()) {"last-valid"}else{"evaluated"}.into(), commands: vec!["feature.edit".into(),"file.export-cutlist".into()], relationships: std::iter::once(weldment.source.node.clone()).chain(weldment.members.iter().map(|member|member.reference.group.node.clone())).collect(), data: serde_json::json!({"contract":"tcad.weldment","version":crate::weldment::CONTRACT_VERSION,"output_key":key,"manufacturing":weldment}) });
            }
        }
        for (key, properties) in &evaluation.properties {
            if active.is_none_or(|id| id == properties.definition) {
                out.push(SemanticElement {
                    id: format!("inspection:{key}"), role: "engineering-properties".into(), label: "Mass Properties".into(),
                    value: properties.total.mass_kg.map(|mass|format!("{mass} kg")), state: if session.draft.as_ref().is_some_and(|d|d.pending || d.error.is_some()) {"last-valid"} else {"evaluated"}.into(),
                    commands: vec!["inspection.properties".into()], relationships: vec![properties.definition.clone()],
                    data: serde_json::json!({"contract":"tcad.inspection","version":crate::inspection::CONTRACT_VERSION,"mesh_key":key,"properties":properties}),
                });
            }
        }
    }
    for p in doc
        .parts
        .values()
        .filter(|p| active.is_none_or(|a| a == p.id))
    {
        for (id, param) in &p.parameters {
            out.push(SemanticElement {
                id: format!("{}::{id}", p.id),
                role: "parameter-field".into(),
                label: param.name.clone(),
                value: Some(param.expression.0.clone()),
                state: if session.draft.is_some() {
                    "draft"
                } else {
                    "committed"
                }
                .into(),
                commands: vec!["parameter.edit".into()],
                relationships: vec![p.id.clone()],
                data:serde_json::json!({"type":"quantity","dimension":param.dimension,"expression":param.expression,"evaluated":crate::expression::value(doc,&p.id,&param.expression,param.dimension).ok(),"previous":session.committed.parts.get(&p.id).and_then(|p|p.parameters.get(id)).map(|p|&p.expression),"editable":true}),
            });
        }
        for node in &p.features {
            let target = serde_json::json!({"kind":"feature","definition":p.id,"feature":node.id});
            let state = if session.draft.is_some() {
                "draft"
            } else {
                "committed"
            };
            match &node.operation {
                crate::Operation::Rib { definition } => out.extend(crate::rib::semantic_fields(
                    doc, &p.id, &node.id, definition, state,
                )),
                crate::Operation::DirectEdit { definition } => out.extend(
                    crate::direct_edit::semantic_fields(doc, &p.id, &node.id, definition, state),
                ),
                crate::Operation::CurvePattern { definition } => out.extend(
                    crate::curve_pattern::semantic_fields(doc, &p.id, &node.id, definition, state),
                ),
                crate::Operation::PlacementPattern { definition } => {
                    out.extend(crate::placement_pattern::semantic_fields(
                        doc, &p.id, &node.id, definition, state,
                    ))
                }
                crate::Operation::ReferencePlane { definition } => {
                    out.extend(crate::reference_geometry::plane_semantic_fields(
                        doc, &p.id, &node.id, definition, state,
                    ))
                }
                crate::Operation::BodyFilter { definition } => out.extend(
                    crate::body_filter::semantic_fields(doc, &p.id, &node.id, definition, state),
                ),
                crate::Operation::Split { definition } => out.extend(
                    crate::split::semantic_fields(doc, &p.id, &node.id, definition, state),
                ),
                crate::Operation::ReferenceCurve { definition } => out.extend(
                    crate::reference_curves::semantic_fields(
                        doc, &p.id, &node.id, definition, state,
                    )
                    .into_iter()
                    .map(|mut element| {
                        element.data["target"] = target.clone();
                        element
                    }),
                ),
                _ => {}
            }
            out.extend(
                crate::reference_geometry::edge_point_semantic_fields(
                    doc,
                    &p.id,
                    &node.id,
                    &node.operation,
                    if session.draft.is_some() {
                        "draft"
                    } else {
                        "committed"
                    },
                )
                .into_iter()
                .map(|mut element| {
                    element.data["target"] = target.clone();
                    element
                }),
            );
            if let crate::Operation::DrilledHoles { definition } = &node.operation {
                out.extend(crate::holes::semantic_fields(
                    doc,
                    &p.id,
                    &node.id,
                    definition,
                    if session.draft.is_some() {
                        "draft"
                    } else {
                        "committed"
                    },
                ));
            }
            if let crate::Operation::Sheet { operation } = &node.operation {
                out.extend(crate::sheet_metal::semantic_fields(
                    doc,
                    &p.id,
                    &node.id,
                    operation,
                    if session.draft.is_some() {
                        "draft"
                    } else {
                        "committed"
                    },
                ));
            }
            if let crate::Operation::Weldment { operation } = &node.operation {
                out.extend(crate::weldment::semantic_fields(
                    doc,
                    &p.id,
                    &node.id,
                    operation,
                    if session.draft.is_some() {
                        "draft"
                    } else {
                        "committed"
                    },
                ));
            }
            if let crate::Operation::WeldBead { definition } = &node.operation {
                out.extend(crate::weld_beads::semantic_fields(
                    doc,
                    &p.id,
                    &node.id,
                    definition,
                    if session.draft.is_some() {
                        "draft"
                    } else {
                        "committed"
                    },
                ));
            }
            out.extend(crate::body_editing::pattern_semantic_fields(
                doc,
                &p.id,
                &node.id,
                &node.operation,
                if session.draft.is_some() {
                    "draft"
                } else {
                    "committed"
                },
            ));
            if let crate::Operation::Mold { operation } = &node.operation {
                out.extend(crate::mold::semantic_fields(
                    doc,
                    &p.id,
                    &node.id,
                    operation,
                    if session.draft.is_some() {
                        "draft"
                    } else {
                        "committed"
                    },
                ));
            }
            if let crate::Operation::Scale { options, .. } = &node.operation {
                out.extend(crate::body_editing::semantic_fields(
                    doc,
                    &p.id,
                    &node.id,
                    options,
                    if session.draft.is_some() {
                        "draft"
                    } else {
                        "committed"
                    },
                ));
            }
            if let crate::Operation::Surface { operation } = &node.operation {
                out.extend(crate::surface::semantic_fields(
                    doc,
                    &p.id,
                    &node.id,
                    operation,
                    if session.draft.is_some() {
                        "draft"
                    } else {
                        "committed"
                    },
                ));
            }
            out.extend(crate::edge_features::semantic_fields(
                doc,
                &p.id,
                &node.id,
                &node.operation,
                if session.draft.is_some() {
                    "draft"
                } else {
                    "committed"
                },
            ));
            if let crate::Operation::Shell {
                thickness, options, ..
            } = &node.operation
            {
                out.extend(crate::wall::semantic_fields(
                    doc,
                    &p.id,
                    &node.id,
                    thickness,
                    options,
                    if session.draft.is_some() {
                        "draft"
                    } else {
                        "committed"
                    },
                ));
            }
            out.extend(crate::revolution::semantic_fields(
                doc,
                &p.id,
                &node.id,
                &node.operation,
                if session.draft.is_some() {
                    "draft"
                } else {
                    "committed"
                },
            ));
            out.extend(crate::extrusion::semantic_fields(
                doc,
                &p.id,
                &node.id,
                &node.operation,
                if session.draft.is_some() {
                    "draft"
                } else {
                    "committed"
                },
            ));
            out.push(SemanticElement {
                id: node.id.clone(),
                role: "feature-station".into(),
                label: node.name.clone(),
                value: Some(node.operation.name().into()),
                state: if crate::configuration::feature_suppressed(doc, &p.id, node) {
                    "suppressed"
                } else {
                    "ready"
                }
                .into(),
                commands: vec![
                    "feature.edit".into(),
                    "feature.suppress".into(),
                    "feature.delete".into(),
                    "feature.move-up".into(),
                    "feature.move-down".into(),
                ],
                data: {
                    let mut data = serde_json::to_value(node).unwrap_or_default();
                    data["target"] = target;
                    data
                },
                relationships: p
                    .feature_dependencies(node)
                    .into_iter()
                    .map(|r| r.node)
                    .chain(
                        node.operation
                            .expressions()
                            .into_iter()
                            .flat_map(|e| parameter_links(doc, &p.id, e)),
                    )
                    .chain(
                        if let crate::Operation::Profile {
                            profile: crate::Profile::Sketch { sketch },
                            ..
                        } = &node.operation
                        {
                            p.sketches
                                .get(sketch)
                                .map(|s| {
                                    s.expressions()
                                        .into_iter()
                                        .flat_map(|expression| {
                                            parameter_links(doc, &p.id, expression)
                                        })
                                        .collect::<Vec<_>>()
                                })
                                .unwrap_or_default()
                        } else {
                            vec![]
                        },
                    )
                    .collect(),
            });
        }
    }
    for part in doc
        .parts
        .values()
        .filter(|p| active.is_none_or(|id| id == p.id))
    {
        let evaluation = session
            .draft
            .as_ref()
            .map_or(session.evaluation.as_ref(), |draft| {
                draft.evaluation.as_ref()
            })
            .filter(|evaluation| evaluation.valid && evaluation.identity == session.identity());
        let configuration = doc.active_configurations.get(&part.id);
        let evaluated_key = crate::configuration::key(&part.id, configuration.map(String::as_str));
        let effective = evaluation.and_then(|_| {
            crate::configuration::variant(doc, &part.id, configuration.map(String::as_str)).ok()
        });
        for sketch in part.sketches.values() {
            let solved = evaluation.and_then(|evaluation| {
                part.features.iter().find_map(|feature| {
                    if !matches!(&feature.operation, crate::Operation::Profile {
                        profile: crate::Profile::Sketch { sketch: source }, ..
                    } if source == &sketch.id)
                    {
                        return None;
                    }
                    evaluation
                        .sketches
                        .get(&format!("{evaluated_key}::{}", feature.id))
                        .or_else(|| {
                            evaluation
                                .sketches
                                .get(&format!("{}::{}", part.id, feature.id))
                        })
                        .and_then(|record| record.solved.as_ref())
                })
            });
            out.extend(sketch_elements(
                effective.as_ref().unwrap_or(doc),
                &part.id,
                solved.unwrap_or(sketch),
            ));
        }
        for feature in &part.features {
            if let crate::Operation::Mold { operation } = &feature.operation {
                let evaluation = session
                    .draft
                    .as_ref()
                    .map_or(session.evaluation.as_ref(), |d| d.evaluation.as_ref())
                    .filter(|e| e.valid && e.identity == session.identity());
                let report = evaluation
                    .and_then(|e| e.mold_analyses.get(&format!("{}::{}", part.id, feature.id)));
                out.push(SemanticElement { id:format!("mold:{}",feature.id),role:if operation.is_analysis(){"mold-analysis"}else{"mold-operation"}.into(),label:feature.name.clone(),value:Some(operation.name().into()),state:if feature.suppressed{"suppressed"}else if report.is_some(){"evaluated"}else{"active"}.into(),commands:vec!["feature.edit".into()],relationships:operation.dependencies().into_iter().map(|r|r.node).collect(),data:serde_json::json!({"contract_version":crate::mold::CONTRACT_VERSION,"definition":part.id,"feature":feature.id,"source":operation,"report":report}) });
            }
            if let crate::Operation::Surface { operation } = &feature.operation {
                out.push(SemanticElement {
                    id: format!("surface:{}", feature.id), role: "surface-operation".into(), label: feature.name.clone(),
                    value: Some(operation.name().into()), state: if feature.suppressed { "suppressed" } else { "active" }.into(),
                    commands: if operation.is_surface() { vec!["feature.edit".into(), "surface.offset".into(), "surface.thicken".into(), "surface.knit".into(), "surface.reverse".into(),"surface.trim".into(),"surface.trim-domain".into(),"surface.untrim".into(),"surface.extend".into()] } else { vec!["feature.edit".into()] },
                    relationships: operation.dependencies().into_iter().map(|r| r.node).collect(),
                    data: serde_json::json!({"contract_version":crate::surface::CONTRACT_VERSION, "definition":part.id,"feature":feature.id,"source":operation,"body_kind":if operation.is_surface(){"surface"}else{"solid"}}),
                });
            }
        }
        let evaluation = session
            .draft
            .as_ref()
            .and_then(|d| d.preview_evaluation())
            .or(session.evaluation.as_ref());
        for feature in part.features.iter().filter(|f| {
            f.operation.is_reference()
                && !matches!(f.operation, crate::Operation::ReferenceCurve { .. })
        }) {
            let key = format!("{}::{}", part.id, feature.id);
            let evaluated = evaluation
                .filter(|e| e.valid && e.identity == session.identity())
                .and_then(|e| e.references.get(&key));
            out.push(SemanticElement {
                id: format!("reference:{}", feature.id),
                role: "reference-geometry".into(),
                label: feature.name.clone(),
                value: Some(feature.operation.name().into()),
                state: if feature.suppressed { "suppressed" } else if evaluated.is_some() { "evaluated" } else { "pending" }.into(),
                commands: if matches!(feature.operation, crate::Operation::ReferencePlane { .. } | crate::Operation::ReferenceCoordinateSystem {..}) {
                    vec!["feature.edit".into(), "sketch.new".into(), "viewport.normal-reference".into()]
                } else { vec!["feature.edit".into(), "selection.measure".into()] },
                relationships: feature.operation.dependencies().into_iter().map(|r| r.node).collect(),
                data: serde_json::json!({"contract_version": crate::reference_geometry::CONTRACT_VERSION, "definition": part.id, "feature":feature.id, "source":feature.operation, "evaluated":evaluated}),
            });
            if let Some(reference) = evaluated.filter(|r| {
                matches!(
                    r.geometry,
                    crate::reference_geometry::Geometry::CoordinateSystem { .. }
                )
            }) {
                for (geometry, topology, label) in reference.elements() {
                    out.push(SemanticElement {
                        id:format!("reference:{}:{}",feature.id,topology.role),
                        role:"reference-element".into(),label,value:Some(geometry.topology().into()),state:"evaluated".into(),
                        commands:match geometry {crate::reference_geometry::Geometry::Plane {..}=>vec!["sketch.new".into(),"reference.plane".into(),"viewport.normal-reference".into()],crate::reference_geometry::Geometry::Axis {..}=>vec!["reference.axis".into(),"reference.point".into()],_=>vec!["reference.point".into(),"reference.plane".into()]},
                        relationships:vec![feature.id.clone()],
                        data:serde_json::json!({"contract_version":crate::reference_geometry::CONTRACT_VERSION,"definition":part.id,"feature":feature.id,"reference":topology,"geometry":geometry}),
                    });
                }
            }
        }
    }
    let state = session
        .draft
        .as_ref()
        .map(|d| {
            if d.pending {
                "evaluating"
            } else if d.error.is_some() {
                "failed"
            } else {
                "valid"
            }
        })
        .unwrap_or("committed");
    out.push(SemanticElement {
        id: "draft".into(),
        role: "preview-status".into(),
        label: state.into(),
        value: session.draft.as_ref().and_then(|d| d.error.clone()),
        state: state.into(),
        commands: vec!["draft.apply".into(), "draft.cancel".into()],
        relationships: vec![],
        data: serde_json::Value::Null,
    });
    out.extend(crate::drawing::semantic_elements(doc, state));
    out.extend(crate::drawing_tables::evaluated_semantic_rows(
        doc,
        property_evaluation,
        state,
    ));
    out.extend(crate::exploded_views::semantic_elements(
        doc,
        property_evaluation,
        state,
    ));
    let inspection_evaluation =
        property_evaluation.filter(|evaluation| evaluation.identity == session.identity());
    for assembly in doc.assemblies.values() {
        out.extend(crate::assembly_inspection::semantic_fields(
            doc,
            &assembly.id,
            state,
            inspection_evaluation,
        ));
        out.push(SemanticElement {
            id: assembly.id.clone(),
            role: "assembly".into(),
            label: assembly.name.clone(),
            value: None,
            state: state.into(),
            commands: vec![
                "assembly.edit".into(),
                "assembly.exploded-new".into(),
                "assembly.inspection-new".into(),
            ],
            relationships: assembly
                .instances
                .values()
                .map(|i| i.definition.clone())
                .collect(),
            data: serde_json::to_value(assembly).unwrap_or_default(),
        });
        for pattern in assembly.component_patterns.values() {
            out.push(SemanticElement {id:pattern.id.clone(),role:"component-pattern".into(),label:pattern.name.clone(),value:Some(pattern.count.0.clone()),state:state.into(),commands:vec!["assembly.pattern".into(),"assembly.edit".into()],relationships:pattern.seeds.clone(),data:serde_json::json!({"version":crate::component_pattern::CONTRACT_VERSION,"source":pattern,"evaluated":property_evaluation.and_then(|e|e.assemblies.get(&crate::configuration::key(&assembly.id,doc.active_configurations.get(&assembly.id).map(String::as_str)))),"placement_driver":true,"modes":["linear","circular"],"stable_occurrence_ids":true,"copy_internal_mates":true})});
        }
        for mate in &assembly.mates {
            out.extend(
                crate::reference_geometry::edge_point_semantic_fields(
                    doc,
                    &assembly.id,
                    &mate.id,
                    mate,
                    state,
                )
                .into_iter()
                .map(|mut element| {
                    if element.role == "mate-field"
                        && let Some(data) = element.data.as_object_mut()
                    {
                        // Mate point expressions resolve in the configured participant
                        // leaf, which this source-only projection does not materialize.
                        data.remove("evaluated");
                        data.insert(
                            "evaluation_scope".into(),
                            serde_json::json!("configured_participant_leaf"),
                        );
                    }
                    element
                }),
            );
            let evaluated = property_evaluation.and_then(|e| {
                e.assemblies.get(&crate::configuration::key(
                    &assembly.id,
                    doc.active_configurations
                        .get(&assembly.id)
                        .map(String::as_str),
                ))
            });
            out.push(SemanticElement {
                id: mate.id.clone(),
                role: "assembly-mate".into(),
                label: mate.kind.clone(),
                value: Some(mate.offset.0.clone()),
                state: state.into(),
                commands: vec!["object.edit".into(), "object.delete".into(), "assembly.edit".into()],
                relationships: mate.participants().into_iter().cloned().chain(mate.first_reference.iter().chain(mate.second_reference.iter()).chain(mate.relationship.iter().flat_map(|r|r.endpoints().into_iter().map(|e|&e.reference))).flat_map(|r|r.source.dependencies().into_iter().map(|n|format!("{}::{}",n.definition,n.node)))).collect(),
                data: serde_json::json!({"version":crate::assembly_geometry::CONTRACT_VERSION,"source":mate,"evaluated":evaluated.and_then(|a|a.frames.get(&mate.id)),"auxiliary_frames":evaluated.and_then(|a|a.auxiliary_frames.get(&mate.id)),"diagnostic":evaluated.and_then(|a|a.diagnostics.get(&mate.id)),"available_modes":crate::assembly_geometry::MATE_MODES,"first_reference_classes":crate::assembly_geometry::reference_classes(&mate.kind,"first"),"second_reference_classes":crate::assembly_geometry::reference_classes(&mate.kind,"second"),"offset_dimension":if mate.kind=="angle" {crate::Dimension::Angle}else{crate::Dimension::Length},"shared_draft":true,"requires_current_validation":true}),
            });
        }
        for (id, parameter) in &assembly.parameters {
            out.push(SemanticElement { id: format!("{}::{id}",assembly.id), role: "parameter-field".into(), label: parameter.name.clone(), value: Some(parameter.expression.0.clone()), state: state.into(), commands: vec!["parameter.edit".into()], relationships: vec![assembly.id.clone()], data: serde_json::json!({"type":"quantity","dimension":parameter.dimension,"expression":parameter.expression,"evaluated":crate::expression::value(doc,&assembly.id,&parameter.expression,parameter.dimension).ok(),"editable":true}) });
        }
        for instance in assembly.instances.values() {
            out.push(SemanticElement {
                id: instance.id.clone(),
                role: "component-instance".into(),
                label: instance.name.clone(),
                value: Some(definition_name(doc, &instance.definition)),
                state: if instance.fixed {
                    "fixed"
                } else {
                    "adjustable"
                }
                .into(),
                commands: vec!["assembly.edit".into()],
                relationships: vec![assembly.id.clone(), instance.definition.clone()],
                data: serde_json::json!({"source":instance,"motion":property_evaluation.and_then(|e|e.assemblies.get(&crate::configuration::key(&assembly.id,doc.active_configurations.get(&assembly.id).map(String::as_str)))).and_then(|a|a.component_motion.get(&instance.id))}),
            });
        }
    }
    for (command, label) in command_catalog() {
        let tool = crate::modeling_tools::TOOLS
            .iter()
            .find(|t| t.command == command);
        let disabled = match command {
            "assembly.exploded-new"
            | "assembly.inspection-new"
            | "assembly.move"
            | "assembly.rotate"
                if !doc.assemblies.contains_key(&doc.root) =>
            {
                Some("Open an assembly first")
            }
            _ if tool.is_some() && active.is_none() => Some("Select an editing part"),
            _ if tool.is_some_and(|t| t.requires_body)
                && !active.and_then(|id| doc.parts.get(id)).is_some_and(|p| {
                    p.outputs.iter().any(|id| {
                        p.features
                            .iter()
                            .any(|f| &f.id == id && !f.suppressed && f.operation.produces_solid())
                    })
                }) =>
            {
                Some("Create a solid body from a sketch first")
            }
            "surface.offset"
            | "surface.thicken"
            | "surface.trim"
            | "surface.trim-domain"
            | "surface.untrim"
            | "surface.extend"
            | "surface.reverse"
            | "surface.knit"
                if !active.and_then(|id| doc.parts.get(id)).is_some_and(|p| {
                    p.features
                        .iter()
                        .any(|f| !f.suppressed && f.operation.is_surface())
                }) =>
            {
                Some("Create a surface body first")
            }
            "draft.apply"
                if !session.draft.as_ref().is_some_and(|d| {
                    !d.pending
                        && d.error.is_none()
                        && d.evaluation
                            .as_ref()
                            .is_some_and(|e| e.valid && e.identity == session.identity())
                }) =>
            {
                Some("A current valid preview is required")
            }
            "draft.cancel" if session.draft.is_none() => Some("No pending draft"),
            "feature.new" | "sketch.new" | "sketch.new-3d" if active.is_none() => {
                Some("Select an editing part")
            }
            command if command.starts_with("feature.") && active.is_none() => {
                Some("Select an editing part")
            }
            "surface.planar" | "surface.extrude" | "surface.revolve" | "surface.loft"
            | "surface.sweep" | "feature.extrude" | "feature.revolve" | "feature.loft"
            | "feature.sweep"
                if !active.and_then(|id| doc.parts.get(id)).is_some_and(|p| {
                    p.features.iter().any(|f| {
                        !f.suppressed && matches!(f.operation, crate::Operation::Profile { .. })
                    })
                }) =>
            {
                Some("Create a sketch first")
            }
            "feature.cut" | "feature.fillet" | "feature.chamfer" | "feature.pattern"
            | "feature.shell"
                if !active.and_then(|id| doc.parts.get(id)).is_some_and(|p| {
                    p.outputs.iter().any(|id| {
                        p.features
                            .iter()
                            .any(|f| &f.id == id && !f.suppressed && f.operation.produces_solid())
                    })
                }) =>
            {
                Some("Create a solid body first")
            }
            _ => None,
        };
        out.push(SemanticElement { id: format!("command:{command}"), role: "command".into(), label: label.into(), value: disabled.map(str::to_string), state: if disabled.is_some(){"disabled"}else{"available"}.into(), commands: vec![command.into()], relationships: vec![], data: serde_json::json!({"enabled":disabled.is_none(),"disabled_reason":disabled,"accessibility_label":label,"group":tool.map(|t|t.group).or_else(||crate::modeling_tools::SKETCH_TOOLS.iter().find(|t|t.0==command).map(|t|t.2))}) });
    }
    if let Ok(source) = serde_json::to_value(doc) {
        source_fields(&source, "", "", &mut out, state);
    }
    out
}

/// Theme-independent members, points, references and constraint intent, including pending edits.
pub fn sketch_elements(
    doc: &Document,
    definition: &str,
    sketch: &crate::Sketch,
) -> Vec<SemanticElement> {
    let mut out = vec![];
    let inactive = crate::sketch_transform::inactive_entities(sketch);
    for (reference, label) in [
        (crate::sketch_actions::ORIGIN, "Sketch Origin"),
        (crate::sketch_actions::X_AXIS, "Sketch X Axis"),
        (crate::sketch_actions::Y_AXIS, "Sketch Y Axis"),
    ] {
        out.push(SemanticElement {
            id: format!("{}::{reference}", sketch.id), role: "sketch-reference".into(), label: label.into(),
            value: None, state: "reference".into(), commands: vec!["sketch.select-entity".into()],
            relationships: vec![sketch.id.clone()], data: serde_json::json!({"definition":definition,"sketch":sketch.id,"reference":reference}),
        });
    }
    for (index, (id, point)) in sketch.points.iter().enumerate() {
        out.push(SemanticElement {
            id: id.clone(),
            role: "sketch-point".into(),
            label: format!("Point {}", index + 1),
            value: Some(format!("{}, {} mm", point[0], point[1])),
            state: if inactive.contains(id) {
                "suppressed"
            } else {
                "geometry"
            }
            .into(),
            commands: vec!["sketch.select-entity".into()],
            relationships: vec![sketch.id.clone()],
            data: serde_json::json!({"definition":definition,"sketch":sketch.id,"point":point}),
        });
    }
    for constraint in &sketch.constraints {
        let dimension = crate::sketch_actions::is_dimension(&constraint.kind);
        let mut data = serde_json::to_value(constraint).unwrap_or_default();
        if let Some(fields) = data.as_object_mut() {
            fields.insert("definition".into(), serde_json::json!(definition));
            fields.insert("sketch".into(), serde_json::json!(sketch.id));
            if dimension {
                let unit = crate::sketch_actions::dimension_unit(&constraint.kind);
                fields.insert("dimension".into(), serde_json::json!(unit));
                fields.insert(
                    "evaluated".into(),
                    serde_json::json!(if constraint.driven {
                        crate::sketch_actions::measured_dimension(
                            doc, definition, sketch, constraint,
                        )
                        .ok()
                    } else {
                        crate::expression::value(doc, definition, &constraint.value, unit).ok()
                    }),
                );
            }
        }
        out.push(SemanticElement {
            id: constraint.id.clone(),
            role: "sketch-constraint".into(),
            label: crate::sketch_actions::label(&constraint.kind).into(),
            value: crate::sketch_actions::is_dimension(&constraint.kind).then(|| {
                if constraint.driven {
                    crate::sketch_actions::measured_dimension(doc, definition, sketch, constraint)
                        .map(|value| {
                            if crate::sketch_actions::dimension_unit(&constraint.kind)
                                == crate::Dimension::Angle
                            {
                                format!("{} deg", value.to_degrees())
                            } else {
                                format!("{value} mm")
                            }
                        })
                        .unwrap_or_else(|e| e.to_string())
                } else {
                    constraint.value.0.clone()
                }
            }),
            state: if constraint
                .entities
                .iter()
                .any(|entity| inactive.contains(entity))
            {
                "suppressed"
            } else if constraint.driven {
                "reference-dimension"
            } else if crate::sketch_actions::is_dimension(&constraint.kind) {
                "dimension"
            } else {
                "relation"
            }
            .into(),
            commands: if dimension {
                vec![
                    "sketch.select-entity".into(),
                    "sketch.edit-constraint".into(),
                    "sketch.toggle-dimension-mode".into(),
                    "sketch.delete-constraint".into(),
                ]
            } else {
                vec![
                    "sketch.select-entity".into(),
                    "sketch.delete-constraint".into(),
                ]
            },
            relationships: constraint
                .entities
                .iter()
                .cloned()
                .chain(std::iter::once(sketch.id.clone()))
                .chain(parameter_links(doc, definition, &constraint.value))
                .collect(),
            data,
        });
    }
    for (id, member) in &sketch.members {
        out.push(SemanticElement {
            id: id.clone(),
            role: "sketch-member".into(),
            label: match member.curve {
                crate::SketchCurve::Line { .. } => "Line",
                crate::SketchCurve::Arc { .. } => "Arc",
                crate::SketchCurve::Bezier { .. } => "Control-point spline",
                crate::SketchCurve::Nurbs { .. } => "Rational spline",
                crate::SketchCurve::Circle { .. } => "Circle",
                crate::SketchCurve::Ellipse { .. } => "Ellipse",
            }
            .into(),
            value: None,
            state: if crate::sketch_transform::suppressed_members(sketch).contains(id) {
                "suppressed"
            } else if member.construction {
                "construction"
            } else {
                "geometry"
            }
            .into(),
            commands: vec![
                "sketch.edit-member".into(),
                "sketch.smart-dimension".into(),
                "sketch.offset-entities".into(),
                "sketch.fillet".into(),
                "sketch.chamfer".into(),
                "sketch.trim".into(),
                "sketch.extend".into(),
                "sketch.split".into(),
                "sketch.stretch".into(),
                "sketch.replace-entities".into(),
                "sketch.instance-controls".into(),
                "sketch.edit-spline".into(),
                "sketch.edit-conic".into(),
                "sketch.edit-text".into(),
                "sketch.toggle-construction".into(),
                "sketch.delete-selection".into(),
            ],
            relationships: vec![sketch.id.clone()],
            data: serde_json::to_value(member).unwrap_or_default(),
        });
    }
    for (id, intent) in &sketch.equations {
        if let Some(element) = out
            .iter_mut()
            .find(|element| element.id == *id && element.role == "sketch-member")
        {
            element.label = "Equation Curve".into();
            element.commands.push("sketch.edit-equation".into());
        }
        let feature = doc.parts.get(definition).into_iter().flat_map(|part|&part.features).find(|feature| matches!(&feature.operation,crate::Operation::Profile {profile:crate::Profile::Sketch {sketch:source},..} if source==&sketch.id));
        let target = feature.map(|feature| {
            serde_json::to_value(crate::selection_actions::SelectionTarget::Sketch {
                definition: definition.into(),
                feature: feature.id.clone(),
                sketch: sketch.id.clone(),
                entities: vec![crate::sketch_actions::SketchEntity::Member { member: id.clone() }],
            })
            .unwrap_or_default()
        });
        out.extend(
            crate::sketch_equation::semantic_fields(
                doc,
                definition,
                sketch,
                id,
                intent,
                "parametric",
            )
            .into_iter()
            .map(|mut element| {
                if let Some(target) = &target {
                    element.data["target"] = target.clone();
                }
                element.data["operation_id"] = serde_json::json!(id);
                element.data["member"] = serde_json::json!(id);
                element.data["sketch"] = serde_json::json!(sketch.id);
                if !element.relationships.contains(&sketch.id) {
                    element.relationships.push(sketch.id.clone());
                }
                element
            }),
        );
    }
    for (id, block) in &sketch.blocks {
        out.push(SemanticElement {id:id.clone(),role:"sketch-modifier".into(),label:format!("Block definition · {}",block.name),value:Some(format!("{} members",block.members.len())),state:"local-block-source".into(),commands:vec!["sketch.block-edit".into(),"sketch.block-insert".into(),"sketch.block-export".into()],relationships:vec![sketch.id.clone()].into_iter().chain(sketch.block_instances.iter().filter(|(_,instance)|&instance.definition==id).map(|(key,_)|key.clone())).chain(block.expressions().into_iter().flat_map(|expr|parameter_links(doc,definition,expr))).collect(),data:serde_json::json!({"version":crate::sketch_blocks::CONTRACT_VERSION,"operation_id":id,"definition_id":id,"operation":block})});
    }
    for (id, instance) in &sketch.block_instances {
        out.push(SemanticElement {id:id.clone(),role:"sketch-modifier".into(),label:format!("Block instance · {}",sketch.blocks.get(&instance.definition).map(|block|block.name.as_str()).unwrap_or("Missing definition")),value:Some(instance.scale.0.clone()),state:if instance.suppressed{"suppressed"}else{"associative-block"}.into(),commands:vec!["sketch.block-edit".into(),"sketch.block-instance".into(),"sketch.block-export".into(),"sketch.block-explode".into(),"sketch.delete-selection".into()],relationships:vec![sketch.id.clone(),instance.definition.clone(),instance.anchor.clone()].into_iter().chain(instance.outputs()).chain(instance.expressions().into_iter().flat_map(|expr|parameter_links(doc,definition,expr))).collect(),data:serde_json::json!({"version":crate::sketch_blocks::CONTRACT_VERSION,"member":instance.outputs().first(),"operation_id":id,"definition_id":instance.definition,"operation":instance})});
    }
    for (id, text) in &sketch.texts {
        out.push(SemanticElement {id:id.clone(),role:"sketch-modifier".into(),label:format!("Sketch Text · {}",text.text.lines().next().unwrap_or_default()),value:Some(text.height.0.clone()),state:"parametric-text".into(),commands:vec!["sketch.edit-text".into(),"sketch.detach-conversion".into(),"sketch.delete-selection".into()],relationships:vec![sketch.id.clone(),text.anchor.clone(),text.font.resource.clone()].into_iter().chain(text.outputs()).chain(text.expressions().into_iter().flat_map(|expr|parameter_links(doc,definition,expr))).collect(),data:serde_json::json!({"version":crate::sketch_text::CONTRACT_VERSION,"member":text.outputs().first(),"operation_id":id,"operation":text})});
    }
    for (id, offset) in &sketch.offsets {
        out.push(SemanticElement {
            id: id.clone(), role: "sketch-modifier".into(), label: "Offset Entities".into(),
            value: Some(offset.distance.0.clone()), state: "parametric".into(),
            commands: vec!["sketch.offset-entities".into(), "sketch.delete-selection".into()],
            relationships: std::iter::once(sketch.id.clone()).chain(offset.members.iter().flat_map(|m| [m.source.clone(),m.member.clone()])).collect(),
            data: serde_json::json!({"version":crate::sketch_edit::CONTRACT_VERSION,"member":offset.members.first().map(|m| &m.member),"operation":offset}),
        });
    }
    for (id, spline) in &sketch.splines {
        out.push(SemanticElement {
            id: format!("{id}::spline"), role: "sketch-modifier".into(), label: spline.label().into(),
            value: Some(format!("degree {}", spline.degree())), state: "parametric".into(),
            commands: vec!["sketch.edit-spline".into(), "sketch.detach-conversion".into(), "sketch.delete-selection".into()],
            relationships: std::iter::once(sketch.id.clone()).chain(std::iter::once(id.clone())).chain(spline.fit_points()).chain(spline.expressions().into_iter().flat_map(|expr| parameter_links(doc, definition, expr))).collect(),
            data: serde_json::json!({"version":crate::sketch_spline::CONTRACT_VERSION,"member":id,"operation":spline}),
        });
    }
    for (id, definition_intent) in &sketch.conics {
        out.push(SemanticElement {
            id: format!("{id}::conic"),role:"sketch-modifier".into(),label:definition_intent.label().into(),value:None,state:"parametric".into(),
            commands:vec!["sketch.edit-conic".into(),"sketch.detach-conversion".into(),"sketch.delete-selection".into()],
            relationships:std::iter::once(sketch.id.clone()).chain(definition_intent.outputs(id)).chain(definition_intent.source_points()).chain(definition_intent.expressions().into_iter().flat_map(|expr| parameter_links(doc,definition,expr))).collect(),
            data:serde_json::json!({"version":crate::sketch_conic::CONTRACT_VERSION,"member":id,"operation":definition_intent}),
        });
    }
    for (id, fillet) in &sketch.fillets {
        out.push(SemanticElement {
            id: id.clone(), role: "sketch-modifier".into(), label: "Sketch Fillet".into(),
            value: Some(fillet.radius.0.clone()), state: "parametric".into(),
            commands: vec!["sketch.fillet".into(), "sketch.delete-selection".into()],
            relationships: vec![sketch.id.clone(),fillet.arc.clone(),fillet.lines[0].clone(),fillet.lines[1].clone()],
            data: serde_json::json!({"version":crate::sketch_edit::CONTRACT_VERSION,"member":fillet.arc,"operation":fillet}),
        });
    }
    for (id, conversion) in &sketch.conversions {
        out.push(SemanticElement {
            id: format!("{id}::conversion"), role: "sketch-modifier".into(), label: "Converted Entity".into(), value: None, state: "linked".into(),
            commands: vec!["sketch.detach-conversion".into(),"sketch.delete-selection".into()],
            relationships: vec![sketch.id.clone(),id.clone(),conversion.body.node.clone()],
            data: serde_json::json!({"version":crate::sketch_edit::CONTRACT_VERSION,"member":id,"operation":conversion}),
        });
    }
    for (id, c) in &sketch.chamfers {
        out.push(SemanticElement {
            id: id.clone(), role: "sketch-modifier".into(), label: "Sketch Chamfer".into(), value: None, state: "parametric".into(),
            commands: vec!["sketch.chamfer".into(), "sketch.delete-selection".into()],
            relationships: vec![sketch.id.clone(), c.member.clone(), c.lines[0].clone(), c.lines[1].clone()],
            data: serde_json::json!({"version":crate::sketch_edit::CONTRACT_VERSION,"member":c.member,"operation":c}),
        });
    }
    for (id, p) in &sketch.transforms {
        out.push(SemanticElement {
            id: id.clone(), role: "sketch-modifier".into(), label: p.transform.label().into(), value: None, state: if p.dynamic {"dynamic-mirror"} else {"parametric"}.into(),
            commands: vec![p.transform.command().into(), "sketch.instance-controls".into(), "sketch.detach-conversion".into(), "sketch.delete-selection".into()].into_iter().chain(p.dynamic.then(||"sketch.stop-dynamic-mirror".into())).collect(),
            relationships: std::iter::once(sketch.id.clone()).chain(crate::sketch_transform::sources(p)).chain(crate::sketch_transform::outputs(p)).collect(),
            data: serde_json::json!({"version":crate::sketch_transform::CONTRACT_VERSION,"member":crate::sketch_transform::outputs(p).first(),"operation_id":id,"operation":p}),
        });
        for (index, instance) in p.instances.iter().enumerate() {
            out.push(SemanticElement{
            id:format!("{id}::instance::{}",index+1),role:"sketch-instance".into(),label:format!("{} · Instance {}",p.transform.label(),index+2),value:None,state:if instance.suppressed {"suppressed"} else {"active"}.into(),
            commands:vec!["sketch.instance-controls".into()],relationships:vec![sketch.id.clone(),id.clone()].into_iter().chain(instance.members.values().cloned()).collect(),
            data:serde_json::json!({"version":crate::sketch_transform::CONTRACT_VERSION,"operation_id":id,"instance_index":index,"member":instance.members.values().next(),"instance":instance}),
        });
        }
    }
    for handle in crate::sketch_handles::handles(doc, definition, sketch).unwrap_or_default() {
        out.push(SemanticElement {
            id: handle.id.clone(), role: "sketch-handle".into(), label: handle.label.clone(),
            value: Some(format!("{}, {} mm", handle.position[0], handle.position[1])),
            state: if handle.disabled_reason.is_some() { "locked" } else { "available" }.into(),
            commands: vec!["sketch.select-handle".into()],
            relationships: vec![sketch.id.clone(), handle.target.entity().key().into()],
            data: serde_json::json!({"version": crate::sketch_handles::CONTRACT_VERSION, "definition":definition,"sketch":sketch.id,"handle":handle}),
        });
    }
    out.extend(crate::sketch_dimensions::semantics(sketch));
    out
}
fn parameter_links(doc: &Document, owner: &str, expression: &crate::Expr) -> Vec<Id> {
    crate::expression::references(&expression.0)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|name| {
            let (definition, key) = name.rsplit_once("::").unwrap_or((owner, &name));
            let part = doc
                .parts
                .get(definition)
                .or_else(|| doc.parts.values().find(|p| p.name == definition));
            let assembly = doc
                .assemblies
                .get(definition)
                .or_else(|| doc.assemblies.values().find(|a| a.name == definition));
            let (definition, parameters) = part
                .map(|p| (&p.id, &p.parameters))
                .or_else(|| assembly.map(|a| (&a.id, &a.parameters)))?;
            let (id, _) = parameters
                .get_key_value(key)
                .or_else(|| parameters.iter().find(|(_, p)| p.name == key))?;
            Some(format!("{definition}::{id}"))
        })
        .collect()
}

/// Command identities belong to the application; visual menus and shortcuts consume this catalog.
pub const COMMANDS: &[(&str, &str)] = &[
    ("reference.repair", "Repair References…"),
    ("reference.repair-station", "Inspect Repair Upstream Stage"),
    ("sketch.finish-dimension", "Done Dimension"),
    ("sketch.cancel-dimension", "Cancel Dimension"),
    ("assembly.inspection-new", "Interference / Clearance"),
    ("assembly.inspection-edit", "Edit Assembly Inspection"),
    ("assembly.inspection-delete", "Delete Assembly Inspection"),
    ("assembly.inspection-highlight", "Highlight Inspection Pair"),
    (
        "assembly.inspection-select-first",
        "Select First Inspection Component",
    ),
    (
        "assembly.inspection-select-second",
        "Select Second Inspection Component",
    ),
    (
        "assembly.inspection-clear-highlight",
        "Clear Inspection Highlight",
    ),
    ("assembly.exploded-new", "New Exploded View"),
    ("assembly.exploded-edit", "Edit Exploded View"),
    ("assembly.exploded-show", "Show Exploded View"),
    ("assembly.exploded-step", "Show Explode Step"),
    ("assembly.exploded-collapse", "Collapse Exploded View"),
    ("drawing.sheet-add", "Add Drawing Sheet"),
    ("drawing.sheet-edit", "Edit Drawing Sheet"),
    ("drawing.surface-finish-add", "Surface Finish Symbol"),
    ("drawing.weld-symbol-add", "Weld Symbol"),
    ("drawing.annotation-delete", "Delete Drawing Annotation"),
    ("drawing.sheet-preview", "Preview Drawing Sheet"),
    ("drawing.sheet-duplicate", "Duplicate Drawing Sheet"),
    ("drawing.sheet-delete", "Delete Drawing Sheet"),
    ("drawing.sheet-up", "Move Drawing Sheet Up"),
    ("drawing.sheet-down", "Move Drawing Sheet Down"),
    ("drawing.sheet-template-save", "Save Drawing Sheet Template"),
    (
        "drawing.sheet-template-load",
        "New Drawing Sheet From Template",
    ),
    ("drawing.sheet-format-load", "Apply Drawing Sheet Format"),
    ("file.new-part", "New Part"),
    ("file.new", "New assembly"),
    ("file.open", "Open project"),
    ("file.recover", "Recover local work"),
    ("file.save", "Save"),
    ("file.save-as", "Save as"),
    ("file.import", "Import STEP"),
    ("file.export", "Export"),
    ("file.export-flat-pattern", "Export Flat Pattern DXF"),
    ("file.export-cutlist", "Export Cut List CSV"),
    ("file.export-bom", "Export BOM CSV"),
    ("production.edit", "Production Report Preferences"),
    ("file.release", "Create immutable release"),
    ("part.new", "New Part in Assembly"),
    ("assembly.insert-existing", "Insert Existing Component"),
    ("assembly.mate-new", "Mate"),
    ("sketch.new", "Planar sketch"),
    ("sketch.new-3d", "3D Sketch"),
    ("feature.new", "Add feature"),
    ("feature.extrude", "Extruded Boss/Base"),
    ("feature.cut", "Extruded Cut"),
    ("feature.revolve", "Revolve"),
    ("feature.fillet", "Fillet"),
    ("feature.chamfer", "Chamfer"),
    ("feature.pattern", "Pattern"),
    ("feature.shell", "Shell"),
    ("feature.loft", "Loft"),
    ("feature.sweep", "Sweep"),
    ("assembly.edit", "Assembly and mates"),
    ("inspection.sensors", "Design Monitors"),
    ("camera-views", "Named Views"),
    ("camera-view.save", "Save Current View"),
    ("camera-view.edit", "Edit Named View"),
    ("camera-view.rename", "Rename Named View"),
    ("camera-view.update", "Update Named View From Camera"),
    ("camera-view.delete", "Delete Named View"),
    ("camera-view.recall", "Recall Named View"),
    ("camera-view.perspective", "Perspective Projection"),
    ("camera-view.orthographic", "Orthographic Projection"),
    ("standard-parts", "Standard Hardware"),
    ("inspection.tolerance-stacks", "Tolerance Stacks"),
    ("appearance.environment.import", "Import HDR Environment"),
    ("appearance.environment.clear", "Use Studio Environment"),
    ("properties", "Model Properties"),
    ("sketch-block-library", "Sketch Block Library"),
    (
        "sketch-library.add",
        "Add Native Blocks to Personal Library",
    ),
    ("sketch-library.refresh", "Refresh Sketch Block Library"),
    (
        "inspection.frame-selected",
        "Use Selected Component Coordinates",
    ),
    ("inspection.export-json", "Export Mass Properties JSON"),
    (
        "drawing.interact-cohort-align-x",
        "Align Annotation Origins in a Column",
    ),
    (
        "drawing.interact-cohort-align-y",
        "Align Annotation Origins in a Row",
    ),
    (
        "drawing.interact-cohort-distribute-x",
        "Distribute Annotation Origins Horizontally",
    ),
    (
        "drawing.interact-cohort-distribute-y",
        "Distribute Annotation Origins Vertically",
    ),
    (
        "drawing.interact-cohort-stack-x",
        "Space Annotations in a Row",
    ),
    (
        "drawing.interact-cohort-stack-y",
        "Space Annotations in a Column",
    ),
    (
        "drawing.interact-cohort-copy",
        "Duplicate Selected Annotations",
    ),
    (
        "drawing.interact-cohort-delete",
        "Delete Selected Annotations",
    ),
    ("drawing.interact-clear-selection", "Clear Paper Selection"),
    ("material.export-csv", "Export Selected Material CSV"),
    (
        "material.export-project-csv",
        "Export Project Materials CSV",
    ),
    ("assembly.display-state-new", "New Assembly Display State"),
    ("assembly.display-state-normal", "Normal Assembly Display"),
    ("assembly.move", "Move Component"),
    ("assembly.rotate", "Rotate Component"),
    ("assembly.pattern", "Component Pattern"),
    ("assembly.group-new", "Rigid Component Group"),
    ("assembly.copy-with-mates", "Copy With Mates"),
    ("configurations", "Configurations and design tables"),
    ("equations", "Equations and named dimensions"),
    ("parameter.new", "Named parameter"),
    ("presentation", "Layout and theme"),
    ("settings", "Settings…"),
    ("settings.model", "Model Settings…"),
    ("settings.application", "Application Preferences…"),
    ("settings.apply", "Apply Settings"),
    ("settings.ok", "Save Settings and Close"),
    ("settings.cancel", "Cancel Settings"),
    ("settings.presentation", "Customize Layout and Theme…"),
    ("settings.theme-load", "Load Theme or Presentation Package…"),
    ("settings.theme-export", "Export Current Theme…"),
    ("controls", "Navigation controls"),
    ("navigation.preferences", "Trackpad Navigation Preferences"),
    ("navigation.reset", "Reset Navigation Preferences"),
    ("navigation.set", "Set Navigation Preference"),
    ("navigation.stop", "Stop Navigation"),
    ("navigation.upright", "Upright View"),
    ("navigation.fit-all", "Fit Assembly / Part"),
    ("navigation.fit-selection", "Fit Selected Component"),
    ("navigation.zoom-in", "Zoom In"),
    ("navigation.zoom-out", "Zoom Out"),
    ("navigation.orbit-left", "Orbit Left"),
    ("navigation.orbit-right", "Orbit Right"),
    ("navigation.orbit-up", "Orbit Up"),
    ("navigation.orbit-down", "Orbit Down"),
    ("navigation.pan-left", "Pan Left"),
    ("navigation.pan-right", "Pan Right"),
    ("navigation.pan-up", "Pan Up"),
    ("navigation.pan-down", "Pan Down"),
    ("document.rebuild", "Rebuild geometry"),
    ("history.latest", "Return to Latest"),
    ("versions", "Design versions"),
    ("repositories", "Git repositories"),
    ("repositories.open", "Open a Git repository"),
    ("repositories.publish", "Publish model to a Git repository"),
    ("git-connections", "Git connections and credentials"),
    ("component.fork", "Fork part or assembly"),
    ("feature-transfer", "Transfer selected features"),
    ("readme.preview", "Update README model preview"),
    ("bom", "Bill of materials"),
    ("inspection.properties", "Mass Properties"),
    ("comments", "Review comments"),
    ("materials", "Engineering Materials"),
    ("appearances", "Appearance · Colors and Textures"),
    ("lighting", "Studio Lighting"),
    ("libraries", "Component libraries"),
    ("drawings", "Drawings"),
    ("manufacturing", "Manufacturing"),
    ("source", "Source definitions"),
    ("commands", "Command search"),
    ("draft.apply", "Apply preview"),
    ("draft.cancel", "Cancel preview"),
    ("viewport.fit", "Fit assembly"),
    ("document.rename", "Rename Document…"),
    ("viewport.fit-sketch", "Fit active sketch"),
    ("viewport.fit-support", "Fit sketch support face"),
    ("viewport.fit-part-context", "Fit editing part"),
    (
        "viewport.sketches.auto",
        "Sketch outlines · selected / active",
    ),
    ("viewport.sketches.all", "Show all sketch outlines"),
    ("viewport.sketches.hidden", "Hide sketch outlines"),
    ("viewport.sketch-xray", "Sketch outlines · X-ray"),
    ("viewport.references.auto", "References · selected / active"),
    ("viewport.references.all", "Show all reference geometry"),
    ("viewport.references.hidden", "Hide reference geometry"),
    ("viewport.reference-xray", "Reference geometry · X-ray"),
    ("sketch.snap-entities", "Snap sketch entities"),
    ("selection.isolate", "Isolate selection"),
];
pub fn command_catalog() -> Vec<(&'static str, &'static str)> {
    COMMANDS
        .iter()
        .copied()
        .filter(|(command, _)| {
            !crate::modeling_tools::TOOLS
                .iter()
                .any(|t| t.command == *command)
        })
        .chain(
            crate::modeling_tools::TOOLS
                .iter()
                .map(|t| (t.command, t.label)),
        )
        .chain(
            crate::modeling_tools::SKETCH_TOOLS
                .iter()
                .map(|t| (t.0, t.1)),
        )
        .collect()
}
fn source_fields(
    value: &serde_json::Value,
    pointer: &str,
    identity: &str,
    out: &mut Vec<SemanticElement>,
    state: &str,
) {
    fn escape(s: &str) -> String {
        s.replace('~', "~0").replace('/', "~1")
    }
    match value {
        serde_json::Value::Object(object) => {
            for (key, value) in object {
                let key = escape(key);
                source_fields(
                    value,
                    &format!("{pointer}/{key}"),
                    &format!("{identity}/{key}"),
                    out,
                    state,
                );
            }
        }
        serde_json::Value::Array(array) => {
            for (index, value) in array.iter().enumerate() {
                let stable = value
                    .get("id")
                    .and_then(|v| v.as_str())
                    .map(|id| format!("@{}", escape(id)))
                    .unwrap_or_else(|| index.to_string());
                source_fields(
                    value,
                    &format!("{pointer}/{index}"),
                    &format!("{identity}/{stable}"),
                    out,
                    state,
                );
            }
        }
        _ => {
            let label = identity
                .rsplit('/')
                .next()
                .unwrap_or(identity)
                .replace('_', " ");
            let editable = !matches!(pointer, "/schema_version" | "/revision" | "/id")
                && !pointer.ends_with("/id");
            out.push(SemanticElement { id:format!("source:{identity}"),role:"document-field".into(),label,value:Some(value.as_str().map(str::to_string).unwrap_or_else(||value.to_string())),state:state.into(),commands: if editable {vec!["entity.edit".into()]}else{vec![]},relationships:vec![],data:serde_json::json!({"pointer":pointer,"json":value,"editable":editable,"accessibility_label":identity,"value_type":if value.is_string(){"string"}else if value.is_boolean(){"boolean"}else if value.is_number(){"number"}else{"json"}}) });
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Component {
    pub role: String,
    #[serde(default)]
    pub bind: Option<String>,
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub widget: Option<String>,
    #[serde(default)]
    pub label: Option<String>,
}
/// A placement binding never owns a second copy of a domain value or action.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct BindingDescriptor {
    pub bind: Option<String>,
    pub command: Option<String>,
    pub widget: Option<String>,
    pub label: Option<String>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct RepeatBinding {
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub id_prefix: Option<String>,
    #[serde(default)]
    pub ids: Vec<String>,
    #[serde(default)]
    pub relationships: Vec<String>,
    #[serde(default)]
    pub owner: Option<String>,
    #[serde(default)]
    pub states: Vec<String>,
    #[serde(default)]
    pub limit: Option<usize>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct LayoutNode {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default, rename = "use")]
    pub component: Option<String>,
    #[serde(default)]
    pub layout: Option<String>,
    #[serde(default)]
    pub width_logical_px: Option<f32>,
    #[serde(default)]
    pub height_logical_px: Option<f32>,
    #[serde(default)]
    pub grow: Option<f32>,
    #[serde(default)]
    pub position: Option<[f32; 2]>,
    #[serde(default)]
    pub hidden: bool,
    #[serde(default)]
    pub resizable: bool,
    #[serde(default)]
    pub classes: Vec<String>,
    #[serde(default)]
    pub states: Vec<String>,
    #[serde(default)]
    pub attributes: BTreeMap<String, String>,
    #[serde(default)]
    pub children: Vec<LayoutNode>,
    #[serde(default)]
    pub editing: Option<LayoutSizing>,
    #[serde(default)]
    pub bind: Option<String>,
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub semantic: Option<String>,
    #[serde(default)]
    pub widget: Option<String>,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub repeat: Option<RepeatBinding>,
    #[serde(default)]
    pub template: Option<Box<LayoutNode>>,
    #[serde(default)]
    pub gap_logical_px: Option<f32>,
    /// Insets use CSS ordering: top, right, bottom, left.
    #[serde(default)]
    pub padding_logical_px: Option<[f32; 4]>,
    #[serde(default)]
    pub min_width_logical_px: Option<f32>,
    #[serde(default)]
    pub max_width_logical_px: Option<f32>,
    #[serde(default)]
    pub min_height_logical_px: Option<f32>,
    #[serde(default)]
    pub max_height_logical_px: Option<f32>,
    #[serde(default)]
    pub align: Option<String>,
    #[serde(default)]
    pub justify: Option<String>,
    /// Anchored children are out of flow and positioned relative to their parent.
    #[serde(default)]
    pub anchor: Option<String>,
    #[serde(default)]
    pub visibility: Option<String>,
    #[serde(default)]
    pub overflow: Option<String>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct LayoutSizing {
    pub width_logical_px: Option<f32>,
    pub height_logical_px: Option<f32>,
    pub grow: Option<f32>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LayoutDocument {
    pub schema_version: u32,
    #[serde(default)]
    pub components: BTreeMap<String, Component>,
    pub layouts: BTreeMap<String, LayoutNode>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct StyleRule {
    pub selector: String,
    pub properties: BTreeMap<String, String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Theme {
    #[serde(default)]
    pub schema_version: u32,
    #[serde(default)]
    pub variables: BTreeMap<String, String>,
    #[serde(default)]
    pub rules: Vec<StyleRule>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct DrawingPaperView {
    pub zoom: f32,
    pub pan: [f32; 2],
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OverlayVisibility {
    #[default]
    Auto,
    All,
    Hidden,
}
impl OverlayVisibility {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::All => "all",
            Self::Hidden => "hidden",
        }
    }
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "auto" => Some(Self::Auto),
            "all" => Some(Self::All),
            "hidden" => Some(Self::Hidden),
            _ => None,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PresentationState {
    #[serde(default)]
    pub display_quality: crate::display_mesh::Quality,
    /// Transient feature-history inspection. Saved projects reopen at Latest.
    #[serde(skip)]
    pub history_cursor: Option<crate::NodeRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exploded_view: Option<crate::exploded_views::Selection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_state: Option<crate::display_states::Selection>,
    #[serde(default)]
    pub sketch_annotations: BTreeMap<Id, [f32; 2]>,
    #[serde(default = "default_sketch_annotation_visibility")]
    pub sketch_annotation_visibility: [bool; 2],
    /// Optical sketch/reference display controls; they never suppress CAD features.
    #[serde(default)]
    pub sketch_overlay_mode: OverlayVisibility,
    #[serde(default)]
    pub reference_overlay_mode: OverlayVisibility,
    #[serde(default)]
    pub sketch_overlay_xray: bool,
    #[serde(default)]
    pub reference_overlay_xray: bool,
    #[serde(default = "default_entity_snap")]
    pub sketch_entity_snap: bool,
    pub active_layout: String,
    pub active_part: Option<Id>,
    pub active_node: Option<Id>,
    pub active_instance: Option<Id>,
    pub isolation: bool,
    pub ghost_others: bool,
    pub pinned: Vec<Id>,
    pub mode: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub library_favorites: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub library_recent: Vec<String>,
    /// Paper workspace preferences are presentation intent, never drawing source.
    #[serde(default)]
    pub drawing_workspace: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub drawing_sheet: Option<Id>,
    #[serde(default)]
    pub drawing_page: u32,
    #[serde(default)]
    pub drawing_paper_views: BTreeMap<String, DrawingPaperView>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub drawing_selection: Option<crate::drawing_interaction::Target>,
    pub original: bool,
    pub orbit: [f32; 2],
    /// f64 camera authority; legacy transient values remain adapter mirrors.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub camera_pose: Option<crate::camera_views::Pose>,
    #[serde(
        default,
        skip_serializing_if = "crate::camera_views::is_default_projection"
    )]
    pub camera_projection: crate::camera_views::Projection,
    /// User navigation preferences are presentation intent, separate from CAD source.
    #[serde(default)]
    pub navigation: crate::navigation::Preferences,
    #[serde(default)]
    pub roll: f32,
    pub target: [f32; 3],
    pub distance: f32,
    #[serde(default = "default_ui_scale")]
    pub ui_scale: f32,
    #[serde(default = "default_toolbar_height")]
    pub toolbar_height: f32,
    #[serde(default)]
    pub graph_scope: String,
    #[serde(default)]
    pub graph_positions: BTreeMap<Id, [f32; 2]>,
    #[serde(default)]
    pub graph_pan: [f32; 2],
    #[serde(default = "default_ui_scale")]
    pub graph_zoom: f32,
}
fn default_ui_scale() -> f32 {
    1.
}
fn default_sketch_annotation_visibility() -> [bool; 2] {
    [true, true]
}
fn default_entity_snap() -> bool {
    true
}
fn default_toolbar_height() -> f32 {
    68.
}
impl Default for PresentationState {
    fn default() -> Self {
        Self {
            display_quality: Default::default(),
            history_cursor: None,
            exploded_view: None,
            display_state: None,
            sketch_annotations: BTreeMap::new(),
            sketch_annotation_visibility: default_sketch_annotation_visibility(),
            sketch_overlay_mode: OverlayVisibility::Auto,
            reference_overlay_mode: OverlayVisibility::Auto,
            sketch_overlay_xray: false,
            reference_overlay_xray: false,
            sketch_entity_snap: true,
            active_layout: "instrument-rail".into(),
            active_part: None,
            active_node: None,
            active_instance: None,
            isolation: false,
            ghost_others: true,
            pinned: vec![],
            mode: "Model".into(),
            library_favorites: vec![],
            library_recent: vec![],
            drawing_workspace: false,
            drawing_sheet: None,
            drawing_page: 0,
            drawing_paper_views: BTreeMap::new(),
            drawing_selection: None,
            original: false,
            orbit: [0.65, 0.4],
            camera_pose: None,
            camera_projection: Default::default(),
            navigation: crate::navigation::Preferences::default(),
            roll: 0.,
            target: [0.; 3],
            distance: 3.5,
            ui_scale: default_ui_scale(),
            toolbar_height: default_toolbar_height(),
            graph_scope: String::new(),
            graph_positions: BTreeMap::new(),
            graph_pan: [0.; 2],
            graph_zoom: 1.,
        }
    }
}
#[derive(Clone, Debug)]
pub struct Region {
    pub id: String,
    pub role: String,
    pub component: Option<String>,
    pub rect: [f32; 4],
    pub classes: Vec<String>,
    pub resizable: bool,
}
pub fn default_layout() -> LayoutDocument {
    serde_json::from_str(include_str!("../../../assets/ui/layout.json")).expect("bundled layout")
}
pub fn default_theme() -> Theme {
    serde_json::from_str(include_str!("../../../assets/ui/theme.json")).expect("bundled theme")
}
pub fn definition_name(doc: &Document, id: &str) -> String {
    doc.parts
        .get(id)
        .map(|p| p.name.clone())
        .or_else(|| doc.assemblies.get(id).map(|a| a.name.clone()))
        .unwrap_or_else(|| id.into())
}
