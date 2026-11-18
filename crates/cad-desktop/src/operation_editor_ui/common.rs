//! Typed controls for common body modification and pattern intent.
use super::*;

pub(super) fn supports(operation: &Operation) -> bool {
    matches!(
        operation,
        Operation::Fillet { .. }
            | Operation::Chamfer { .. }
            | Operation::Shell { .. }
            | Operation::Draft { .. }
            | Operation::Pattern { .. }
            | Operation::Transform { .. }
            | Operation::Scale { .. }
            | Operation::Boolean { .. }
            | Operation::Mirror { .. }
    )
}

/// Omitted saved defaults are materialized only in this disposable editor DTO.
/// Parsing an edited DTO restores the native omission rules and source shape.
pub(super) fn normalize_source(operation: &Operation, source: &mut Value) {
    let op = &mut source["operation"];
    match operation {
        Operation::Shell { options, .. } => op["options"] = json!(options),
        Operation::Pattern {
            options,
            axis_reference,
            ..
        } => {
            op["options"] = json!(options);
            op["options"]["direction"] = json!(options.direction);
            if let Some(second) = &options.second_direction {
                op["options"]["second_direction"]["direction"] = json!(second.direction);
            }
            op["axis_reference"] = json!(axis_reference);
        }
        Operation::Mirror {
            plane_reference, ..
        }
        | Operation::Draft {
            plane_reference, ..
        } => {
            op["plane_reference"] = json!(plane_reference);
        }
        _ => {}
    }
}

fn optional(fields: &mut Fields<'_>, path: &str, label: &str, default: Value) -> bool {
    let enabled = fields.source.pointer(path).is_some_and(|v| !v.is_null());
    let id = fields.identity(&format!("{}/enabled", path.trim_start_matches('/')));
    let mut data = fields.metadata();
    data["setter"] = json!("operation-editor");
    data["pointer"] = json!(path);
    data["optional_default"] = default;
    data["json"] = json!(enabled);
    data["editable"] = json!(fields.enabled());
    data["classes"] = json!(["modeling-field", fields.group]);
    data["accessibility_label"] = json!(label);
    fields.out.push(SemanticElement {
        id,
        role: "operation-editor-field".into(),
        label: label.into(),
        value: Some(enabled.to_string()),
        state: if fields.enabled() {
            "draft"
        } else {
            "disabled"
        }
        .into(),
        commands: vec![],
        relationships: vec![fields.feature.id.clone()],
        data,
    });
    enabled
}

fn plane(
    fields: &mut Fields<'_>,
    label: &str,
    frame: &cad_core::Plane,
    reference: Option<&PlaneSource>,
) {
    let path = "/operation/plane_reference";
    let mut choices = plane_choices(fields.w, fields.owner, &fields.feature.id);
    let origin_plane = [OriginPlane::Top, OriginPlane::Front, OriginPlane::Right]
        .into_iter()
        .find(|plane| &plane.plane() == frame);
    let explicit = origin_plane.map_or("Custom plane".into(), |plane| {
        format!("{} (fixed)", plane.label())
    });
    choices.insert(0, choice(&explicit, Value::Null));
    if let Some(PlaneSource::Face { reference, .. }) = reference {
        let value = fields.source.pointer(path).cloned().unwrap_or_default();
        if !choices.iter().any(|item| item["value"] == value) {
            let feature = fields
                .w
                .session
                .visible()
                .parts
                .get(&reference.producer.definition)
                .and_then(|p| p.features.iter().find(|f| f.id == reference.producer.node));
            choices.push(choice(
                feature.map_or("Model face".into(), |f| format!("{} face", f.name)),
                value,
            ));
        }
    }
    fields.choice(path, label, choices);
    if reference.is_none() {
        fields.folding_group("plane-frame", "Plane coordinates", false);
        for (key, label, unit) in [
            ("origin", "Origin", "mm"),
            ("u", "U direction", ""),
            ("v", "V direction", ""),
        ] {
            fields.coordinates(&format!("/operation/plane/{key}"), label, unit);
        }
    }
}

fn pattern_direction(
    fields: &mut Fields<'_>,
    path: &str,
    step: &str,
    slot: usize,
    direction: Option<&cad_core::body_editing::PatternDirection>,
) {
    use crate::component_pattern_ui as pattern;
    use cad_core::body_editing::PatternDirection;
    let doc = fields.w.session.visible();
    let mut choices = vec![choice("XYZ vector", Value::Null)];
    for (name, axis) in pattern::axis_choices(doc, fields.owner, Some(&fields.feature.id)) {
        choices.push(choice(
            name,
            PatternDirection {
                axis,
                occurrence: None,
                ..direction.cloned().unwrap_or_default()
            },
        ));
    }
    let picked =
        fields.w.selected.as_ref().and_then(|selection| {
            pattern::picked_part_direction(fields.w, selection, direction).ok()
        });
    for (candidate, from_selection) in [(direction, false), (picked.as_ref(), true)] {
        let Some(candidate) = candidate else { continue };
        let value = json!(candidate);
        if choices.iter().any(|item| item["value"] == value) {
            continue;
        }
        let axis = pattern::axis_name(doc, &candidate.axis);
        let name = candidate.occurrence.as_ref().map_or_else(
            || axis.clone(),
            |occurrence| format!("{} / {axis}", pattern::path_name(doc, &occurrence.path)),
        );
        choices.push(choice(
            if from_selection {
                format!("Use {name}")
            } else {
                name
            },
            value,
        ));
    }
    fields.choice(path, "Direction", choices);
    let picking = fields
        .w
        .fields
        .get("part-pattern-pick-direction")
        .is_some_and(|active| active == &format!("Direction {}", slot + 1));
    let command = if picking {
        "pattern.direction-cancel"
    } else {
        "pattern.direction-pick"
    };
    let label = if picking {
        "Cancel direction pick"
    } else {
        "Pick direction on model"
    };
    let mut data = fields.metadata();
    data["pattern"] = json!(fields.feature.id);
    data["slot"] = json!(slot);
    data["component"] = json!(false);
    data["classes"] = json!(["modeling-action", "direction-pick", fields.group]);
    fields.out.push(SemanticElement {
        id: fields.identity(&format!("action/pick-direction-{slot}")),
        role: "pattern-direction-action".into(),
        label: label.into(),
        value: Some("Select a straight edge, sketch line or axis.".into()),
        state: if !fields.enabled() {
            "disabled"
        } else if picking {
            "selected"
        } else {
            "available"
        }
        .into(),
        commands: vec![command.into()],
        relationships: vec![fields.feature.id.clone(), fields.owner.into()],
        data,
    });
    if direction.is_some() {
        fields.quantity(&format!("{path}/spacing"), "Spacing", "mm");
        fields.toggle(&format!("{path}/reverse"), "Reverse direction");
    } else {
        fields.coordinates(step, "Spacing", "mm");
    }
}

fn picked_face(fields: &Fields<'_>, body: &NodeRef) -> Option<Value> {
    fields
        .w
        .selected
        .as_ref()
        .filter(|s| {
            s.reference.topology == "face"
                && crate::reference_ui::selected_body(fields.w, s) == *body
                && crate::selection_ui::edge_occurrence(fields.w, &s.path)
        })
        .map(|s| json!(s.reference))
}

pub(super) fn fields(fields: &mut Fields<'_>) {
    let operation = fields.feature.operation.clone();
    let mut body_choices = None;
    if !matches!(operation, Operation::Boolean { .. }) {
        let choices = node_choices(fields.w, fields.owner, &fields.feature.id, "solid");
        if let Some(name) = choices
            .iter()
            .find(|candidate| Some(&candidate["value"]) == fields.source.pointer("/operation/body"))
            .and_then(|candidate| candidate["label"].as_str())
        {
            body_choices = Some((format!("Input body · {name}"), choices));
        } else {
            fields.folding_group("sources", "Select input body", true);
            fields.choice("/operation/body", "Body", choices);
        }
    }
    match &operation {
        Operation::Fillet { edges, .. } | Operation::Chamfer { edges, .. } => {
            fields.group("extent", "Dimensions");
            if let Operation::Fillet { .. } = operation {
                fields.quantity("/operation/radius", "Radius", "mm");
            } else if let Operation::Chamfer { body, options, .. } = &operation {
                fields.quantity(
                    "/operation/distance",
                    if options.method == cad_core::edge_features::ChamferMethod::EqualDistance {
                        "Distance"
                    } else {
                        "Distance 1"
                    },
                    "mm",
                );
                match options.method {
                    cad_core::edge_features::ChamferMethod::EqualDistance => {}
                    cad_core::edge_features::ChamferMethod::TwoDistances => {
                        fields.quantity("/operation/options/second_distance", "Distance 2", "mm")
                    }
                    cad_core::edge_features::ChamferMethod::DistanceAngle => {
                        fields.quantity("/operation/options/angle", "Angle", "deg")
                    }
                }
                let method_name = match options.method {
                    cad_core::edge_features::ChamferMethod::EqualDistance => "Equal distance",
                    cad_core::edge_features::ChamferMethod::TwoDistances => "Two distances",
                    cad_core::edge_features::ChamferMethod::DistanceAngle => "Distance and angle",
                };
                fields.folding_group("chamfer-method", &format!("Method · {method_name}"), false);
                fields.choice(
                    "/operation/options/method",
                    "Method",
                    enum_choices(&[
                        ("equal_distance", "Equal distance"),
                        ("two_distances", "Two distances"),
                        ("distance_angle", "Distance and angle"),
                    ]),
                );
                if options.method != cad_core::edge_features::ChamferMethod::EqualDistance {
                    fields.toggle("/operation/options/flip", "Swap chamfer sides");
                    let mut choices = vec![choice("Automatic", Value::Null)];
                    if let Some(pick) = picked_face(fields, body) {
                        choices.push(choice("Selected face", pick));
                    }
                    fields.choice("/operation/options/first_face", "Distance 1 face", choices);
                }
            }
            let (picking, selection): (Vec<_>, Vec<_>) =
                crate::selection_ui::edge_controls(fields.w)
                    .into_iter()
                    .partition(|element| {
                        matches!(
                            element.data["contract"].as_str(),
                            Some(
                                "tcad.presentation.edge-picking"
                                    | "tcad.presentation.selection-sets"
                            )
                        )
                    });
            let missing = selection
                .iter()
                .filter(|e| e.role == "edge-selection-item" && e.state == "unresolved")
                .count();
            let label = if missing > 0 {
                format!("Edges ({}) · {missing} unavailable", edges.len())
            } else {
                format!("Edges ({})", edges.len())
            };
            fields.folding_group("selection", &label, edges.is_empty() || missing > 0);
            if edges.is_empty() {
                fields.summary("pick-edges", "Select edges on the model.", "ready");
            }
            let group_id = fields.identity("group/selection");
            fields.out.extend(selection.into_iter().map(|mut element| {
                // The group already reports the count. Direct bindings still
                // offer the complete summary and all actions to custom layouts.
                if element.role == "edge-selection"
                    || element.role == "edge-selection-action"
                        && element.state == "disabled"
                        && element.data["inline_parent"].is_null()
                {
                    element.data["inline_parent"] = json!(group_id);
                }
                element
            }));
            let mode = picking
                .iter()
                .find(|e| e.data["selected"] == true)
                .map_or("Tangent chain", |e| e.label.as_str());
            fields.folding_group("edge-picking", &format!("Pick mode · {mode}"), false);
            fields.out.extend(picking);
        }
        Operation::Shell {
            body,
            options,
            thickness,
            removed_faces,
        } => {
            fields.group("extent", "Wall thickness");
            fields.quantity("/operation/thickness", "Thickness", "mm");
            fields.toggle("/operation/options/outward", "Shell outward");
            let openings = if removed_faces.is_empty() {
                "Openings · Enclosed".into()
            } else {
                format!("Opening faces ({})", removed_faces.len())
            };
            fields.folding_group("selection", &openings, false);
            if removed_faces.is_empty() {
                fields.summary("closed-shell", "Pick a face to open the shell.", "ready");
            }
            fields.topology_array("/operation/removed_faces", body, "face");
            let overrides = if options.face_thicknesses.is_empty() {
                "Individual wall thicknesses".into()
            } else {
                format!(
                    "Individual wall thicknesses ({})",
                    options.face_thicknesses.len()
                )
            };
            fields.folding_group("face-thickness", &overrides, false);
            for (index, wall) in options.face_thicknesses.iter().enumerate() {
                let key = format!("face-thickness/{}", item_identity(&json!(wall.face), index));
                fields.quantity(
                    &format!("/operation/options/face_thicknesses/{index}/thickness"),
                    &format!("Face {} thickness", index + 1),
                    "mm",
                );
                let id = fields.identity(&format!("{key}/thickness"));
                if let Some(last) = fields.out.last_mut() {
                    last.id = id;
                }
                fields.array_actions("/operation/options/face_thicknesses", index, &key, false);
            }
            let face = picked_face(fields, body).filter(|p| {
                !options
                    .face_thicknesses
                    .iter()
                    .any(|wall| json!(wall.face) == *p)
            });
            fields.action("face-thickness/add","Set selected face thickness",json!({"kind":"append","pointer":"/operation/options/face_thicknesses","value":{"face":face,"thickness":thickness}}),face.is_some(),"Pick a face on the input body without a thickness override");
        }
        Operation::Draft {
            body,
            plane: frame,
            plane_reference,
            faces,
            ..
        } => {
            fields.group("extent", "Draft");
            fields.quantity("/operation/angle", "Angle", "deg");
            plane(fields, "Neutral plane", frame, plane_reference.as_ref());
            fields.folding_group(
                "selection",
                &format!("Faces to draft ({})", faces.len()),
                faces.is_empty(),
            );
            if faces.is_empty() {
                fields.summary("pick-faces", "Select faces on the model.", "ready");
            }
            fields.topology_array("/operation/faces", body, "face");
        }
        Operation::Transform { .. } => {
            fields.group("placement", "Move / rotate body");
            fields.coordinates("/operation/placement/translation", "Move", "mm");
            fields.coordinates("/operation/placement/rotation_deg", "Rotate", "deg");
        }
        Operation::Scale { options, .. } => {
            fields.group("extent", "Scale");
            fields.toggle("/operation/options/uniform", "Uniform scale");
            if options.uniform {
                fields.quantity("/operation/options/factors/0", "Scale factor", "");
            } else {
                fields.coordinates("/operation/options/factors", "Factor", "");
            }
            fields.folding_group("placement", "Center and copy", false);
            fields.toggle("/operation/options/keep_original", "Copy body");
            fields.point("/operation/options/center", "Center");
        }
        Operation::Boolean { .. } => {
            fields.group("sources", "Input bodies");
            fields.reference("/operation/left", "Main body", "solid", false);
            fields.reference("/operation/right", "Tool body", "solid", false);
            fields.group("result", "Combine result");
            fields.choice(
                "/operation/mode",
                "Operation",
                enum_choices(&[
                    ("union", "Add"),
                    ("difference", "Subtract"),
                    ("intersection", "Common"),
                ]),
            );
        }
        Operation::Mirror {
            plane: frame,
            plane_reference,
            ..
        } => {
            fields.folding_group("placement", "Mirror", true);
            fields.toggle("/operation/keep_original", "Keep original body");
            plane(fields, "Mirror plane", frame, plane_reference.as_ref());
        }
        Operation::Pattern {
            count,
            options,
            step,
            angle,
            ..
        } => {
            let value = |expression: &cad_core::Expr, dimension| {
                cad_core::expression::value(
                    fields.w.session.visible(),
                    fields.owner,
                    expression,
                    dimension,
                )
                .ok()
            };
            let has_angle = value(angle, cad_core::Dimension::Angle) != Some(0.);
            let circular = has_angle
                && options.direction.is_none()
                && step
                    .iter()
                    .all(|axis| value(axis, cad_core::Dimension::Length) == Some(0.));
            fields.group("extent", "Pattern");
            fields.quantity("/operation/count", "Instances", "");
            if circular {
                fields.folding_group("translation", "Linear spacing", false);
            }
            pattern_direction(
                fields,
                "/operation/options/direction",
                "/operation/step",
                0,
                options.direction.as_ref(),
            );
            fields.folding_group("rotation", "Rotation", has_angle);
            fields.toggle(
                "/operation/options/angle_is_total",
                "Equal spacing over total angle",
            );
            fields.quantity(
                "/operation/angle",
                if options.angle_is_total {
                    "Total angle"
                } else {
                    "Angle per instance"
                },
                "deg",
            );
            fields.toggle("/operation/options/rotate_instances", "Rotate instances");
            if !options.rotate_instances {
                fields.point("/operation/options/seed", "Instance reference point");
            }
            fields.group("placement", "Rotation axis");
            fields.axis("/operation/axis_reference", "Axis reference", true);
            if fields
                .source
                .pointer("/operation/axis_reference")
                .is_none_or(Value::is_null)
            {
                fields.coordinates("/operation/axis", "Axis component", "");
            }
            fields.folding_group(
                "second-direction",
                "Pattern direction 2",
                options.second_direction.is_some(),
            );
            if optional(
                fields,
                "/operation/options/second_direction",
                "Enable direction 2",
                json!(cad_core::body_editing::LinearPatternDirection::default()),
            ) {
                fields.quantity(
                    "/operation/options/second_direction/count",
                    "Direction 2 instances",
                    "",
                );
                pattern_direction(
                    fields,
                    "/operation/options/second_direction/direction",
                    "/operation/options/second_direction/step",
                    1,
                    options
                        .second_direction
                        .as_ref()
                        .and_then(|d| d.direction.as_ref()),
                );
            }
            fields.folding_group("skipped", "Instances to skip", !options.skipped.is_empty());
            let secondary = options
                .second_direction
                .as_ref()
                .map(|s| s.count)
                .unwrap_or(1);
            let valid = options.validate(*count).is_ok();
            let skipped = options
                .skipped
                .iter()
                .copied()
                .collect::<std::collections::BTreeSet<_>>();
            if let Err(error) = options.validate(*count) {
                fields.note("skip-invalid", &error.to_string(), "warning");
            }
            for (index, pair) in options.skipped.iter().take(10_000).enumerate() {
                // A skip coordinate is the identity; moving/removing another row
                // cannot redirect a bound action to a different skipped instance.
                let key = format!("skip/{}/{}", pair[0], pair[1]);
                fields.summary(
                    &key,
                    &format!(
                        "Instance {}, {}",
                        pair[0].saturating_add(1),
                        pair[1].saturating_add(1)
                    ),
                    "ready",
                );
                fields.array_actions("/operation/options/skipped", index, &key, false);
            }
            fields.action(
                "skip/clear",
                "Clear skipped instances",
                json!({"kind":"set","pointer":"/operation/options/skipped","value":[]}),
                !options.skipped.is_empty(),
                "No skipped instances",
            );
            let next = valid
                .then(|| {
                    (0..secondary)
                        .flat_map(|j| (0..*count).map(move |i| [i, j]))
                        .find(|p| *p != [0, 0] && !skipped.contains(p))
                })
                .flatten();
            fields.action(
                "skip/add",
                "Skip next instance",
                json!({"kind":"append","pointer":"/operation/options/skipped","value":next}),
                next.is_some(),
                "All non-seed instances are skipped",
            );
            for axis in 0..2 {
                let id = fields.identity(&format!("skip-input/{axis}"));
                let mut data = fields.metadata();
                data["setter"] = json!("operation-editor");
                data["transient"] = json!("skip-input");
                data["axis"] = json!(axis);
                data["json"] = json!(
                    fields
                        .w
                        .fields
                        .get(&format!(
                            "{id}:{}",
                            fields
                                .w
                                .fields
                                .get("feature-editor-generation")
                                .map(String::as_str)
                                .unwrap_or("")
                        ))
                        .and_then(|v| v.parse::<usize>().ok())
                        .unwrap_or(1)
                );
                data["editable"] = json!(fields.enabled());
                data["classes"] = json!(["modeling-field", "skipped"]);
                fields.out.push(SemanticElement {
                    id,
                    role: "operation-editor-field".into(),
                    label: format!("Skip direction {} instance (1-based)", axis + 1),
                    value: None,
                    state: "draft".into(),
                    commands: vec![],
                    relationships: vec![fields.feature.id.clone()],
                    data,
                });
            }
            let inputs = [0, 1].map(|axis| {
                fields
                    .w
                    .fields
                    .get(&format!(
                        "{}:{}",
                        fields.identity(&format!("skip-input/{axis}")),
                        fields
                            .w
                            .fields
                            .get("feature-editor-generation")
                            .map(String::as_str)
                            .unwrap_or("")
                    ))
                    .and_then(|v| v.parse::<usize>().ok())
                    .unwrap_or(1)
            });
            let pair = inputs.map(|v| v.saturating_sub(1));
            let available = valid
                && inputs.iter().all(|v| *v > 0)
                && pair[0] < *count
                && pair[1] < secondary
                && pair != [0, 0]
                && !options.skipped.contains(&pair);
            fields.action(
                "skip/chosen",
                "Skip specified instance",
                json!({"kind":"append","pointer":"/operation/options/skipped","value":pair}),
                available,
                "Choose an existing non-seed, non-skipped instance",
            );
        }
        _ => {}
    }
    if let Some((label, choices)) = body_choices {
        fields.folding_group("sources", &label, false);
        fields.choice("/operation/body", "Body", choices);
    }
}
