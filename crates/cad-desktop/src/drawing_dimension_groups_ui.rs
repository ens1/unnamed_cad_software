//! Docked dimension-group authoring uses the existing stable drawing field adapter.
use crate::{Workspace, drawing_editor_ui::Fields};
use cad_core::{
    CadError, Drawing, Result,
    drawing::{Anchor, Annotation, AnnotationKind, Tolerance, Units},
    drawing_dimension_groups::{Axis, Group, Mode, Target},
};
use serde_json::{Value, json};
fn choice(label: &str, value: impl serde::Serialize) -> Value {
    json!({"label":label,"value":value})
}
pub(crate) fn fields(fields: &mut Fields<'_>, path: &str, group: &Group) {
    let annotation_id = path
        .trim_start_matches("/entities/")
        .split('/')
        .next()
        .and_then(|index| index.parse::<usize>().ok())
        .and_then(|index| fields.sheet.entities.get(index))
        .map(|annotation| annotation.id.clone())
        .unwrap_or_else(|| path.into());
    fields.group("dimension-group", "Dimension group");
    fields.note("dimension-group-intent","Datum and targets follow saved native point references. Use their paper attachment handles or named reference choices to reattach explicitly. Labels and spacing use paper millimeters; coordinate values use model dimensions.","ready");
    fields.select(
        &format!("{path}/mode"),
        "Group style",
        vec![
            choice("Ordinate", Mode::Ordinate),
            choice("Baseline", Mode::Baseline),
            choice("Chain", Mode::Chain),
        ],
    );
    fields.select(
        &format!("{path}/axis"),
        "Projected direction",
        vec![
            choice("Horizontal X", Axis::X),
            choice("Vertical Y", Axis::Y),
        ],
    );
    fields.anchor(&format!("{path}/datum"), "Common datum");
    fields.number(
        &format!("{path}/spacing_mm"),
        "Baseline spacing",
        "mm",
        Some(0.5),
        Some(1000.),
    );
    fields.group("dimension-group-format", "Group formatting");
    fields.select(
        &format!("{path}/units"),
        "Coordinate units",
        vec![
            choice("Millimeters", Units::Millimeters),
            choice("Inches", Units::Inches),
        ],
    );
    fields.number(
        &format!("{path}/precision"),
        "Decimals",
        "",
        Some(0.),
        Some(8.),
    );
    fields.toggle(&format!("{path}/reference"), "Reference (parentheses)");
    fields.text(&format!("{path}/prefix"), "Prefix", false);
    fields.text(&format!("{path}/suffix"), "Suffix", false);
    let unit = if group.units == Units::Inches {
        "in"
    } else {
        "mm"
    };
    fields.select(
        &format!("{path}/tolerance"),
        "Tolerance",
        vec![
            choice("None", Tolerance::None),
            choice("Basic", Tolerance::Basic),
            choice(
                "Symmetric",
                Tolerance::Symmetric {
                    value: cad_core::Expr(format!("0.1 {unit}")),
                },
            ),
            choice(
                "Deviation",
                Tolerance::Deviation {
                    lower: cad_core::Expr(format!("-0.1 {unit}")),
                    upper: cad_core::Expr(format!("0.1 {unit}")),
                },
            ),
            choice(
                "Limits",
                Tolerance::Limits {
                    lower: cad_core::Expr(format!("9.9 {unit}")),
                    upper: cad_core::Expr(format!("10.1 {unit}")),
                },
            ),
        ],
    );
    match group.tolerance {
        Tolerance::Symmetric { .. } => fields.field(
            &format!("{path}/tolerance/value"),
            "± tolerance",
            vec![],
            Some(unit),
        ),
        Tolerance::Deviation { .. } | Tolerance::Limits { .. } => {
            fields.field(
                &format!("{path}/tolerance/lower"),
                "Lower",
                vec![],
                Some(unit),
            );
            fields.field(
                &format!("{path}/tolerance/upper"),
                "Upper",
                vec![],
                Some(unit),
            );
        }
        _ => {}
    }
    fields.group("dimension-group-targets", "Ordered targets");
    let selected = fields
        .w
        .fields
        .get("drawing-group-target")
        .filter(|id| group.targets.iter().any(|target| &target.id == *id))
        .or_else(|| group.targets.first().map(|target| &target.id));
    fields.transient(
        "group-target",
        "Target",
        json!(selected),
        group
            .targets
            .iter()
            .enumerate()
            .map(|(index, target)| {
                choice(
                    &format!(
                        "Target {} · {}",
                        index + 1,
                        if target.anchor.reference.is_some() {
                            "Native reference"
                        } else {
                            "Explicit coordinates"
                        }
                    ),
                    &target.id,
                )
            })
            .collect(),
    );
    for (index, target) in group
        .targets
        .iter()
        .enumerate()
        .filter(|(_, target)| selected == Some(&target.id))
    {
        let target_path = format!("{path}/targets/{index}");
        fields.anchor(
            &format!("{target_path}/anchor"),
            &format!("Target {}", index + 1),
        );
        fields.coordinates(
            &format!("{target_path}/label_offset_mm"),
            &["Label offset X", "Label offset Y"],
            "mm",
        );
        fields.select(
            &format!("{target_path}/tolerance"),
            "Target tolerance",
            vec![
                choice("Use group tolerance", Value::Null),
                choice("None", Some(Tolerance::None)),
                choice("Basic", Some(Tolerance::Basic)),
                choice(
                    "Symmetric",
                    Some(Tolerance::Symmetric {
                        value: cad_core::Expr(format!("0.1 {unit}")),
                    }),
                ),
                choice(
                    "Deviation",
                    Some(Tolerance::Deviation {
                        lower: cad_core::Expr(format!("-0.1 {unit}")),
                        upper: cad_core::Expr(format!("0.1 {unit}")),
                    }),
                ),
                choice(
                    "Limits",
                    Some(Tolerance::Limits {
                        lower: cad_core::Expr(format!("9.9 {unit}")),
                        upper: cad_core::Expr(format!("10.1 {unit}")),
                    }),
                ),
            ],
        );
        match target.tolerance {
            Some(Tolerance::Symmetric { .. }) => fields.field(
                &format!("{target_path}/tolerance/value"),
                "± tolerance",
                vec![],
                Some(unit),
            ),
            Some(Tolerance::Deviation { .. } | Tolerance::Limits { .. }) => {
                fields.field(
                    &format!("{target_path}/tolerance/lower"),
                    "Lower",
                    vec![],
                    Some(unit),
                );
                fields.field(
                    &format!("{target_path}/tolerance/upper"),
                    "Upper",
                    vec![],
                    Some(unit),
                );
            }
            _ => {}
        }
        for (step, label) in [(-1, "Earlier"), (1, "Later")] {
            fields.action(
                &format!(
                    "dimension-group/{annotation_id}/target/{}/order/{step}",
                    target.id
                ),
                label,
                json!({"kind":"dimension-group-order","group":path,"target":target.id,"step":step}),
                if step < 0 {
                    index > 0
                } else {
                    index + 1 < group.targets.len()
                },
                "This target is already at the end of the sequence",
            );
        }
        fields.action(
            &format!(
                "dimension-group/{annotation_id}/target/{}/remove",
                target.id
            ),
            "Remove target",
            json!({"kind":"dimension-group-remove","group":path,"target":target.id}),
            group.targets.len() > 1,
            "A group needs at least one target",
        );
    }
    fields.action(
        &format!("dimension-group/{annotation_id}/add"),
        "Add target",
        json!({"kind":"dimension-group-add","group":path}),
        group.targets.len() < cad_core::drawing_dimension_groups::MAX_TARGETS,
        "A group supports at most 128 targets",
    );
}
fn group_mut<'a>(sheet: &'a mut Drawing, path: &str) -> Result<&'a mut Group> {
    let index = path
        .trim_start_matches("/entities/")
        .split('/')
        .next()
        .and_then(|index| index.parse::<usize>().ok())
        .ok_or_else(|| CadError::Invalid("Dimension group source is missing".into()))?;
    let AnnotationKind::DimensionGroup { group } = &mut sheet
        .entities
        .get_mut(index)
        .ok_or_else(|| CadError::Invalid("The dimension group no longer exists".into()))?
        .kind
    else {
        return Err(CadError::Invalid(
            "The annotation is no longer a dimension group".into(),
        ));
    };
    Ok(group)
}
pub(crate) fn apply_action(
    w: &mut Workspace,
    sheet: &mut Drawing,
    mutation: &Value,
) -> Result<bool> {
    let kind = mutation["kind"].as_str().unwrap_or_default();
    if !kind.starts_with("dimension-group-") {
        return Ok(false);
    }
    let path = mutation["group"]
        .as_str()
        .ok_or_else(|| CadError::Invalid("The dimension group source is missing".into()))?;
    let group = group_mut(sheet, path)?;
    match kind {
        "dimension-group-add" => {
            if group.targets.len() >= cad_core::drawing_dimension_groups::MAX_TARGETS {
                return Err(CadError::Invalid(
                    "A group supports at most 128 targets".into(),
                ));
            }
            let mut anchor = crate::drawing_ui::selected_anchor(w);
            if anchor.reference.is_none() {
                anchor = group
                    .targets
                    .last()
                    .map(|target| target.anchor.clone())
                    .unwrap_or_else(|| group.datum.clone());
                anchor.reference = None;
                // Explicit coordinates are declared as such. No approximate model
                // edge or screen position is substituted for a native attachment.
                anchor.point[0] += 10.;
                anchor.point[1] += 10.;
                anchor.point[2] += 10.;
            }
            let target = Target::new(anchor);
            w.fields
                .insert("drawing-group-target".into(), target.id.clone());
            group.targets.push(target);
        }
        "dimension-group-remove" | "dimension-group-order" => {
            let id = mutation["target"].as_str().ok_or_else(|| {
                CadError::Invalid("The selected dimension group target is missing".into())
            })?;
            let index = group
                .targets
                .iter()
                .position(|target| target.id == id)
                .ok_or_else(|| {
                    CadError::Invalid("The selected dimension group target no longer exists".into())
                })?;
            if kind == "dimension-group-remove" {
                if group.targets.len() < 2 {
                    return Err(CadError::Invalid(
                        "A group needs at least one target".into(),
                    ));
                }
                group.targets.remove(index);
            } else {
                let destination = (index as i64 + mutation["step"].as_i64().unwrap_or_default())
                    .try_into()
                    .ok()
                    .filter(|destination: &usize| *destination < group.targets.len())
                    .ok_or_else(|| {
                        CadError::Invalid(
                            "This target is already at the end of the sequence".into(),
                        )
                    })?;
                group.targets.swap(index, destination);
            }
        }
        _ => {
            return Err(CadError::Invalid(
                "Unsupported dimension group action".into(),
            ));
        }
    }
    group.validate()?;
    Ok(true)
}
pub(crate) fn create(w: &Workspace, sheet: &Drawing, view: &str) -> AnnotationKind {
    let selected = crate::drawing_ui::selected_anchor(w);
    let dimension = sheet
        .dimensions
        .iter()
        .find(|dimension| {
            w.fields.get("drawing-item") == Some(&dimension.id)
                && dimension.view == view
                && matches!(
                    dimension.options.kind,
                    cad_core::drawing::DimensionKind::Aligned
                        | cad_core::drawing::DimensionKind::Horizontal
                        | cad_core::drawing::DimensionKind::Vertical
                )
                && dimension.options.curve.is_none()
                && dimension.options.second_curve.is_none()
        })
        .or_else(|| {
            sheet.dimensions.iter().find(|dimension| {
                dimension.view == view
                    && dimension.options.curve.is_none()
                    && dimension.options.second_curve.is_none()
                    && !matches!(
                        dimension.options.kind,
                        cad_core::drawing::DimensionKind::Angular
                            | cad_core::drawing::DimensionKind::Diameter
                            | cad_core::drawing::DimensionKind::Radius
                            | cad_core::drawing::DimensionKind::HoleCallout
                    )
            })
        });
    let (datum, target) = if let Some(dimension) = dimension {
        (
            Anchor {
                point: dimension.first,
                reference: dimension.options.first.clone(),
            },
            Anchor {
                point: dimension.second,
                reference: dimension.options.second.clone(),
            },
        )
    } else {
        let mut target = selected.clone();
        target.reference = None;
        if view == "right" {
            target.point[1] += 50.;
        } else {
            target.point[0] += 50.;
        }
        (selected, target)
    };
    let mut group = Group::new(datum, target);
    if dimension.is_some_and(|dimension| {
        dimension.options.kind == cad_core::drawing::DimensionKind::Vertical
    }) {
        group.axis = Axis::Y;
    }
    AnnotationKind::DimensionGroup {
        group: Box::new(group),
    }
}
pub(crate) fn group_target(annotation: &Annotation, target: &str) -> Result<[f64; 2]> {
    let AnnotationKind::DimensionGroup { group } = &annotation.kind else {
        return Err(CadError::Invalid(
            "This annotation is not a dimension group".into(),
        ));
    };
    group
        .targets
        .iter()
        .find(|source| source.id == target)
        .map(|target| target.label_offset_mm)
        .ok_or_else(|| CadError::Invalid("The dimension group target no longer exists".into()))
}
