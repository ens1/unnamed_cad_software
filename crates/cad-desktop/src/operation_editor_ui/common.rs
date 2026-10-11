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

fn plane(fields: &mut Fields<'_>) {
    let path = "/operation/plane_reference";
    let mut choices = plane_choices(fields.w, fields.owner, &fields.feature.id);
    choices.insert(0, choice("Explicit plane frame", Value::Null));
    fields.choice(path, "Neutral / mirror plane", choices);
    if fields.source.pointer(path).is_none_or(Value::is_null) {
        for (key, label, unit) in [
            ("origin", "Origin", "mm"),
            ("u", "U direction", ""),
            ("v", "V direction", ""),
        ] {
            fields.coordinates(&format!("/operation/plane/{key}"), label, unit);
        }
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
    fields.group("sources", "Input bodies");
    match &operation {
        Operation::Boolean { .. } => {
            fields.reference("/operation/left", "Main body", "solid", false);
            fields.reference("/operation/right", "Tool body", "solid", false);
        }
        _ => fields.reference("/operation/body", "Input body", "solid", false),
    }
    match &operation {
        Operation::Fillet { .. } | Operation::Chamfer { .. } => {
            fields.group("selection", "Selected edges");
            fields
                .out
                .extend(crate::selection_ui::edge_controls(fields.w));
            fields.group("extent", "Edge treatment");
            if let Operation::Fillet { .. } = operation {
                fields.quantity("/operation/radius", "Radius", "mm");
            } else if let Operation::Chamfer { body, options, .. } = &operation {
                fields.choice(
                    "/operation/options/method",
                    "Method",
                    enum_choices(&[
                        ("equal_distance", "Equal distance"),
                        ("two_distances", "Two distances"),
                        ("distance_angle", "Distance and angle"),
                    ]),
                );
                fields.quantity("/operation/distance", "Distance 1", "mm");
                match options.method {
                    cad_core::edge_features::ChamferMethod::EqualDistance => {}
                    cad_core::edge_features::ChamferMethod::TwoDistances => {
                        fields.quantity("/operation/options/second_distance", "Distance 2", "mm")
                    }
                    cad_core::edge_features::ChamferMethod::DistanceAngle => {
                        fields.quantity("/operation/options/angle", "Angle", "deg")
                    }
                }
                if options.method != cad_core::edge_features::ChamferMethod::EqualDistance {
                    fields.toggle("/operation/options/flip", "Flip distance 1 side");
                    let mut choices = vec![choice("Adjacent face in normal order", Value::Null)];
                    if let Some(pick) = picked_face(fields, body) {
                        choices.push(choice("Selected face", pick));
                    }
                    fields.choice("/operation/options/first_face", "Distance 1 face", choices);
                }
            }
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
            fields.group("selection", "Opening faces");
            fields.topology_array("/operation/removed_faces", body, "face");
            if removed_faces.is_empty() {
                fields.note(
                    "closed-shell",
                    "No opening faces: create an enclosed cavity.",
                    "ready",
                );
            }
            fields.group("face-thickness", "Individual face thicknesses");
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
        Operation::Draft { body, .. } => {
            fields.group("selection", "Faces to draft");
            fields.topology_array("/operation/faces", body, "face");
            fields.group("extent", "Draft");
            fields.quantity("/operation/angle", "Angle", "deg");
            fields.group("placement", "Neutral plane");
            plane(fields);
        }
        Operation::Transform { .. } => {
            fields.group("placement", "Move / rotate body");
            fields.coordinates("/operation/placement/translation", "Move", "mm");
            fields.coordinates("/operation/placement/rotation_deg", "Rotate", "deg");
        }
        Operation::Scale { options, .. } => {
            fields.group("extent", "Scale");
            fields.toggle("/operation/options/uniform", "Uniform factors");
            if options.uniform {
                fields.quantity("/operation/options/factors/0", "Scale factor", "");
            } else {
                fields.coordinates("/operation/options/factors", "Factor", "");
            }
            fields.toggle("/operation/options/keep_original", "Copy body");
            fields.group("placement", "Scale center");
            fields.point("/operation/options/center", "Center");
            fields.note(
                "scale-axes",
                "Nonuniform factors use document X, Y and Z axes.",
                "ready",
            );
        }
        Operation::Boolean { .. } => {
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
        Operation::Mirror { .. } => {
            fields.group("placement", "Mirror plane");
            plane(fields);
            fields.toggle("/operation/keep_original", "Keep original body");
        }
        Operation::Pattern { count, options, .. } => {
            fields.group("extent", "Pattern direction 1");
            fields.quantity("/operation/count", "Instances", "");
            fields.coordinates("/operation/step", "Spacing", "mm");
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
            fields.group("second-direction", "Pattern direction 2");
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
                fields.coordinates(
                    "/operation/options/second_direction/step",
                    "Direction 2 spacing",
                    "mm",
                );
            }
            fields.group("skipped", "Instances to skip");
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
                fields.note(
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
}
