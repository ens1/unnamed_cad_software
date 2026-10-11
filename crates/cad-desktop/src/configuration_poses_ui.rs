//! Native and portable configuration placement controls share one draft adapter.
use super::{Workspace, owner, source, submit, synchronized};
use bevy_egui::egui;
use cad_core::{
    Document, Id,
    configuration::{self, Definition, PoseOverride},
    presentation::SemanticElement,
};
use serde_json::{Value, json};

const AXES: [&str; 6] = ["X", "Y", "Z", "RX", "RY", "RZ"];
const AMBIGUOUS_SCOPE_REASON: &str = "This assembly has a named configuration with ID 'base'. The legacy editor cannot distinguish it from Base. Pose controls for both scopes are unavailable; other named configurations remain editable. Native configuration data is unchanged.";
fn ambiguous_scope(w: &Workspace, owner: &str, id: &str) -> bool {
    id == "base"
        && w.session
            .visible()
            .assemblies
            .get(owner)
            .is_some_and(|assembly| assembly.configurations.contains_key("base"))
}
fn unavailable_scope(owner: &str) -> SemanticElement {
    SemanticElement {
        id: format!("configuration-placement-unavailable:{owner}/base"),
        role: "configuration-placement-help".into(),
        label: AMBIGUOUS_SCOPE_REASON.into(),
        value: None,
        state: "unsupported".into(),
        commands: Vec::new(),
        relationships: vec![owner.into()],
        data: json!({"contract":"tcad.configuration.placement-field","version":1,"owner":owner,"configuration":"base","editable":false,"enabled":false,"disabled_reason":AMBIGUOUS_SCOPE_REASON}),
    }
}
fn error_key_for(w: &Workspace, identity: &str) -> String {
    format!(
        "configuration-pose-error:{}:{identity}",
        w.fields
            .get("configuration-editor-generation")
            .map(String::as_str)
            .unwrap_or("")
    )
}
fn error_key(w: &Workspace, element: &SemanticElement) -> String {
    error_key_for(w, &element.id)
}
pub(super) fn field_error(w: &mut Workspace, element: &SemanticElement, error: Option<String>) {
    if element.role != "configuration-placement-field" {
        return;
    }
    if element.data.get("editor").and_then(Value::as_str)
        != w.fields
            .get("configuration-editor-generation")
            .map(String::as_str)
    {
        return;
    }
    let key = error_key(w, element);
    if error.is_some() {
        w.confirm_requested = false;
        w.after_confirm = None;
    }
    if let Some(error) = error {
        if w.fields.get(&key) == Some(&error) {
            return;
        }
        w.fields.insert(key, error);
    } else if w.fields.remove(&key).is_none() {
        return;
    }
    synchronized(w);
}
pub(super) fn field_is_invalid(w: &Workspace, element: &SemanticElement) -> bool {
    element.role == "configuration-placement-field" && w.fields.contains_key(&error_key(w, element))
}
fn local_configuration(w: &Workspace, owner: &str, id: &str) -> Option<Definition> {
    source(w)
        .ok()
        .filter(|c| c.id == id && super::owner(w).as_deref() == Some(owner))
        .or_else(|| configuration::definitions(w.session.visible(), owner).remove(id))
}
fn coordinate(pose: &PoseOverride, axis: usize) -> Option<f64> {
    if axis < 3 {
        pose.translation[axis]
    } else {
        pose.rotation_deg[axis - 3]
    }
}
fn set_coordinate(pose: &mut PoseOverride, axis: usize, value: Option<f64>) {
    if axis < 3 {
        pose.translation[axis] = value;
    } else {
        pose.rotation_deg[axis - 3] = value;
    }
}
fn effective(
    doc: &Document,
    owner: &str,
    c: Option<&Definition>,
) -> Option<configuration::Resolved> {
    let c = c?;
    if doc.assemblies.get(owner)?.configurations.get(&c.id) == Some(c) {
        return configuration::resolve(doc, owner, &c.id).ok();
    }
    let mut context = doc.clone();
    context
        .assemblies
        .get_mut(owner)?
        .configurations
        .insert(c.id.clone(), c.clone());
    configuration::resolve(&context, owner, &c.id).ok()
}
fn fields_for(w: &Workspace, owner: &str, id: &str) -> Vec<SemanticElement> {
    if ambiguous_scope(w, owner, id) {
        return Vec::new();
    }
    let doc = w.session.visible();
    let Some(assembly) = doc.assemblies.get(owner) else {
        return Vec::new();
    };
    let c = if id == "base" {
        None
    } else {
        local_configuration(w, owner, id)
    };
    if id != "base" && c.is_none() {
        return Vec::new();
    }
    let resolved = effective(doc, owner, c.as_ref());
    let generated = cad_core::component_pattern::generated(assembly);
    let source_fingerprint = doc.fingerprint();
    let mut out = Vec::new();
    for instance in assembly
        .instances
        .values()
        .filter(|i| !generated.contains(&i.id))
    {
        let explicit = c
            .as_ref()
            .and_then(|c| c.overrides.component_placements.get(&instance.id));
        let mut inherited = instance.placement.clone();
        if let Some(pose) = resolved
            .as_ref()
            .and_then(|r| r.overrides.component_placements.get(&instance.id))
        {
            pose.apply(&mut inherited);
        }
        for (axis, label) in AXES.iter().enumerate() {
            let number = if axis < 3 {
                inherited.translation[axis]
            } else {
                inherited.rotation_deg[axis - 3]
            };
            let overridden = explicit.and_then(|p| coordinate(p, axis)).is_some();
            let driven = axis < 3 && instance.translation_expressions.is_some();
            let text = if driven {
                instance
                    .translation_expressions
                    .as_ref()
                    .map(|expressions| expressions[axis].0.clone())
                    .unwrap_or_default()
            } else {
                format!("{number} {}", if axis < 3 { "mm" } else { "deg" })
            };
            for field in if id == "base" {
                vec!["value"]
            } else {
                vec!["override", "value"]
            } {
                let identity = format!(
                    "configuration-placement:{owner}/{id}/{}:{axis}:{field}",
                    instance.id
                );
                let error = w.fields.get(&error_key_for(w, &identity));
                let unit = if axis < 3 { "mm" } else { "deg" };
                out.push(SemanticElement {
                    id: identity, role:"configuration-placement-field".into(),
                    label: format!("{} · {label}{}", instance.name, if field == "override" {" override"} else {""}),
                    value: Some(if field == "override" {overridden.to_string()} else {text.clone()}),
                    state: if error.is_some() {"invalid"} else if driven {"expression-driven"} else if overridden || id == "base" {"overridden"} else {"inherited"}.into(),
                    commands: Vec::new(), relationships:vec![owner.into(),instance.id.clone(),format!("{owner}@{id}")],
                    data: json!({"contract":"tcad.configuration.placement-field","version":1,
                        "setter":"configuration-dimension","owner":owner,"configuration":id,"instance":instance.id,"axis":axis,"field":field,
                        "editor":w.fields.get("configuration-editor-generation"),"source":source_fingerprint,
                        "kind":if field == "override" {"boolean"}else{"string"},"unit":unit,
                        "json":if field == "override" {json!(overridden)}else{json!(text)},
                        "editable": !driven && (field=="override" || id=="base" || overridden),
                        "classes":["configuration-field","placement-coordinate"],
                        "diagnostics":error.into_iter().collect::<Vec<_>>(),
                        "disabled_reason":if driven {Some("Translation is driven by assembly expressions; configure its driving parameter")} else {None}}),
                });
            }
        }
    }
    for mate in &assembly.mates {
        let Some(cad_core::assembly_geometry::CurvePosition::Free { seed }) = mate
            .relationship
            .as_deref()
            .and_then(cad_core::assembly_geometry::Relationship::curve_position)
        else {
            continue;
        };
        let explicit = c
            .as_ref()
            .and_then(|c| c.overrides.mate_curve_seeds.get(&mate.id));
        let inherited = resolved
            .as_ref()
            .and_then(|r| r.overrides.mate_curve_seeds.get(&mate.id))
            .unwrap_or(seed);
        for field in if id == "base" {
            vec!["value"]
        } else {
            vec!["override", "value"]
        } {
            let identity = format!("configuration-curve-seed:{owner}/{id}/{}:{field}", mate.id);
            let error = w.fields.get(&error_key_for(w, &identity));
            out.push(SemanticElement {
                id:identity,role:"configuration-placement-field".into(),
                label:format!("{} · path seed{}",mate.kind,if field=="override" {" override"}else{""}),
                value:Some(if field=="override"{explicit.is_some().to_string()}else{inherited.to_string()}),
                state:if error.is_some(){"invalid"}else if explicit.is_some() || id=="base"{"overridden"}else{"inherited"}.into(),
                commands:Vec::new(),relationships:vec![owner.into(),mate.id.clone(),format!("{owner}@{id}")],
                data:json!({"contract":"tcad.configuration.placement-field","version":1,"setter":"configuration-dimension",
                    "owner":owner,"configuration":id,"mate":mate.id,"field":field,
                    "editor":w.fields.get("configuration-editor-generation"),"source":source_fingerprint,
                    "kind":if field=="override"{"boolean"}else{"string"},"unit":"fraction",
                    "json":if field=="override"{json!(explicit.is_some())}else{json!(inherited.to_string())},
                    "editable":field=="override" || id=="base" || explicit.is_some(),
                    "classes":["configuration-field","curve-seed"],"diagnostics":error.into_iter().collect::<Vec<_>>()}),
            });
        }
    }
    out
}
pub(super) fn semantics(w: &Workspace) -> Vec<SemanticElement> {
    let Some(owner) = owner(w) else {
        return Vec::new();
    };
    let mut out = fields_for(w, &owner, "base");
    if ambiguous_scope(w, &owner, "base") {
        out.push(unavailable_scope(&owner));
    }
    for id in configuration::names(w.session.visible(), &owner).keys() {
        out.extend(fields_for(w, &owner, id));
    }
    if let Some(assembly) = w.session.visible().assemblies.get(&owner) {
        let generated = cad_core::component_pattern::generated(assembly);
        let scopes = std::iter::once("base".to_string())
            .chain(configuration::names(w.session.visible(), &owner).into_keys());
        for scope in scopes {
            if ambiguous_scope(w, &owner, &scope) {
                continue;
            }
            for instance in assembly
                .instances
                .values()
                .filter(|instance| !generated.contains(&instance.id))
            {
                out.push(SemanticElement {
                    id:format!("configuration-placement-group:{owner}/{scope}/{}",instance.id),
                    role:"configuration-placement-group".into(),label:instance.name.clone(),value:None,state:"ready".into(),commands:Vec::new(),
                    relationships:vec![owner.clone(),instance.id.clone(),format!("{owner}@{scope}")],
                    data:json!({"contract":"tcad.configuration.placement-field","version":1,"owner":owner,"configuration":scope,"instance":instance.id,"editable":false,
                        "children":out.iter().filter(|field|field.data.get("configuration").and_then(Value::as_str)==Some(&scope)&&field.data.get("instance").and_then(Value::as_str)==Some(&instance.id)&&field.role=="configuration-placement-field").map(|field|field.id.clone()).collect::<Vec<_>>()})
                });
            }
        }
        out.push(SemanticElement {id:"configuration-placement-help".into(),role:"configuration-placement-help".into(),label:"Placement coordinates are millimetres and Euler XYZ degrees. Omitted coordinates inherit. Mates solve the final placement; expression translations and pattern copies retain their driving sources.".into(),value:None,state:"ready".into(),commands:Vec::new(),relationships:vec![owner.clone()],data:json!({"contract":"tcad.configuration.placement-field","version":1,"editable":false})});
    }
    out
}
pub(super) fn current_value(w: &Workspace, element: &SemanticElement) -> Option<Value> {
    let owner = element.data.get("owner")?.as_str()?;
    let id = element.data.get("configuration")?.as_str()?;
    if ambiguous_scope(w, owner, id) {
        return None;
    }
    let doc = w.session.visible();
    let assembly = doc.assemblies.get(owner)?;
    let c = if id == "base" {
        None
    } else {
        Some(local_configuration(w, owner, id)?)
    };
    let resolved = effective(doc, owner, c.as_ref());
    let field = element.data.get("field")?.as_str()?;
    if let Some(instance) = element.data.get("instance").and_then(Value::as_str) {
        let axis = element
            .data
            .get("axis")?
            .as_u64()
            .filter(|axis| *axis < 6)? as usize;
        let source = assembly.instances.get(instance)?;
        let explicit = c
            .as_ref()
            .and_then(|c| c.overrides.component_placements.get(instance))
            .and_then(|p| coordinate(p, axis));
        if field == "override" {
            return Some(json!(explicit.is_some()));
        }
        if axis < 3
            && let Some(expressions) = &source.translation_expressions
        {
            return Some(json!(expressions[axis].0));
        }
        let mut pose = source.placement.clone();
        if let Some(overridden) = resolved
            .as_ref()
            .and_then(|r| r.overrides.component_placements.get(instance))
        {
            overridden.apply(&mut pose);
        }
        let number = if axis < 3 {
            pose.translation[axis]
        } else {
            pose.rotation_deg[axis - 3]
        };
        Some(json!(format!(
            "{number} {}",
            if axis < 3 { "mm" } else { "deg" }
        )))
    } else {
        let mate = element.data.get("mate")?.as_str()?;
        let source = assembly
            .mates
            .iter()
            .find(|m| m.id == mate)?
            .relationship
            .as_deref()?
            .curve_position()?;
        let cad_core::assembly_geometry::CurvePosition::Free { seed } = source else {
            return None;
        };
        let explicit = c
            .as_ref()
            .and_then(|c| c.overrides.mate_curve_seeds.get(mate));
        if field == "override" {
            Some(json!(explicit.is_some()))
        } else {
            Some(json!(
                resolved
                    .as_ref()
                    .and_then(|r| r.overrides.mate_curve_seeds.get(mate))
                    .unwrap_or(seed)
                    .to_string()
            ))
        }
    }
}
pub(super) fn set_value(
    w: &mut Workspace,
    element: &SemanticElement,
    value: Value,
) -> cad_core::Result<()> {
    fn invalid(message: &str) -> cad_core::CadError {
        cad_core::CadError::Invalid(message.into())
    }
    let owner = element
        .data
        .get("owner")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid("Missing placement owner"))?
        .to_string();
    let id = element
        .data
        .get("configuration")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid("Missing placement scope"))?
        .to_string();
    if ambiguous_scope(w, &owner, &id) {
        return Err(invalid(AMBIGUOUS_SCOPE_REASON));
    }
    if w.dialog.as_deref() != Some("configurations")
        || super::owner(w).as_deref() != Some(&owner)
        || w.tool_checkpoint.is_none()
    {
        return Err(invalid(
            "Open this assembly's Configurations editor before editing placements",
        ));
    }
    if w.fields.get("configuration-editor").map(String::as_str) != Some(&id)
        && !super::current_source_is_valid(w)
    {
        return Err(invalid(
            "Correct the current configuration source before editing another scope",
        ));
    }
    let fresh = fields_for(w, &owner, &id)
        .into_iter()
        .find(|field| field.id == element.id)
        .ok_or_else(|| invalid("This placement field no longer exists"))?;
    if fresh.data != element.data {
        return Err(invalid(
            "This placement control is stale; use its current source field",
        ));
    }
    if fresh.data.get("editable").and_then(Value::as_bool) != Some(true) {
        return Err(invalid(
            "Enable the placement override, or edit its driving assembly parameter",
        ));
    }
    let mut candidate = w.session.visible().clone();
    let mut c = if id == "base" {
        None
    } else {
        Some(
            local_configuration(w, &owner, &id)
                .ok_or_else(|| invalid("Configuration is missing"))?,
        )
    };
    let field = element
        .data
        .get("field")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid("Missing placement field kind"))?;
    if let Some(instance) = element.data.get("instance").and_then(Value::as_str) {
        let axis = element
            .data
            .get("axis")
            .and_then(Value::as_u64)
            .filter(|axis| *axis < 6)
            .ok_or_else(|| invalid("Invalid placement coordinate"))? as usize;
        let inherited = fields_for(w, &owner, &id)
            .into_iter()
            .find(|f| {
                f.data.get("instance").and_then(Value::as_str) == Some(instance)
                    && f.data.get("axis").and_then(Value::as_u64) == Some(axis as u64)
                    && f.data.get("field").and_then(Value::as_str) == Some("value")
            })
            .and_then(|f| {
                f.data
                    .get("json")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
            .ok_or_else(|| invalid("Missing current placement value"))?;
        let number = if field == "override" {
            if value
                .as_bool()
                .ok_or_else(|| invalid("Placement override requires a boolean"))?
            {
                Some(configuration::placement_value(&inherited, axis)?)
            } else {
                None
            }
        } else {
            Some(configuration::placement_value(
                value
                    .as_str()
                    .ok_or_else(|| invalid("Placement requires number and unit text"))?,
                axis,
            )?)
        };
        if let Some(c) = &mut c {
            let pose = c
                .overrides
                .component_placements
                .entry(instance.into())
                .or_default();
            set_coordinate(pose, axis, number);
            if pose.is_empty() {
                c.overrides.component_placements.remove(instance);
            }
        } else {
            let source = candidate
                .assemblies
                .get_mut(&owner)
                .and_then(|a| a.instances.get_mut(instance))
                .ok_or_else(|| invalid("Placement component is missing"))?;
            let number =
                number.ok_or_else(|| invalid("Base placement coordinates cannot inherit"))?;
            if axis < 3 {
                source.placement.translation[axis] = number;
            } else {
                source.placement.rotation_deg[axis - 3] = number;
            }
        }
    } else if let Some(mate) = element.data.get("mate").and_then(Value::as_str) {
        let inherited = fields_for(w, &owner, &id)
            .into_iter()
            .find(|f| {
                f.data.get("mate").and_then(Value::as_str) == Some(mate)
                    && f.data.get("field").and_then(Value::as_str) == Some("value")
            })
            .and_then(|f| {
                f.data
                    .get("json")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
            .ok_or_else(|| invalid("Missing current curve seed"))?;
        let seed = if field == "override" {
            if value
                .as_bool()
                .ok_or_else(|| invalid("Curve seed override requires a boolean"))?
            {
                Some(
                    inherited
                        .parse::<f64>()
                        .map_err(|_| invalid("Invalid inherited curve seed"))?,
                )
            } else {
                None
            }
        } else {
            Some(
                value
                    .as_str()
                    .ok_or_else(|| invalid("Curve seed requires numeric text"))?
                    .parse::<f64>()
                    .map_err(|_| invalid("Curve seed requires a fraction in 0..1"))?,
            )
        };
        if seed.is_some_and(|seed| !seed.is_finite() || !(0.0..=1.0).contains(&seed)) {
            return Err(invalid("Curve seed requires a finite fraction in 0..1"));
        }
        if let Some(c) = &mut c {
            if let Some(seed) = seed {
                c.overrides.mate_curve_seeds.insert(mate.into(), seed);
            } else {
                c.overrides.mate_curve_seeds.remove(mate);
            }
        } else {
            let seeds = std::collections::BTreeMap::from([(
                mate.into(),
                seed.ok_or_else(|| invalid("Base curve seeds cannot inherit"))?,
            )]);
            configuration::set_component_placements(
                &mut candidate,
                &owner,
                &configuration::PlacementScope::Base,
                &Default::default(),
                &seeds,
            )?;
        }
    } else {
        return Err(invalid("Missing placement target identity"));
    }
    if let Some(mut c) = c {
        c.version = configuration::CONTRACT_VERSION;
        w.fields.insert("configuration-editor".into(), id.clone());
        w.json = serde_json::to_string_pretty(&c)?;
        submit(w, &owner, c.clone());
        if configuration::definitions(w.session.visible(), &owner).get(&id) != Some(&c) {
            return Err(invalid(&w.status));
        }
    } else {
        candidate.active_configurations.remove(&owner);
        candidate.validate()?;
        w.dispatch(cad_core::commands::Command::ReplaceDocument {
            document: candidate,
        });
        w.fields
            .insert("configuration-editor".into(), "base".into());
        w.json.clear();
    }
    if field == "override" && value.as_bool() == Some(false) {
        let value_id = element
            .id
            .strip_suffix(":override")
            .map(|prefix| format!("{prefix}:value"));
        if let Some(value_id) = value_id {
            w.fields.remove(&error_key_for(w, &value_id));
            let prefix = format!("presentation-field:{value_id}:");
            w.fields.retain(|key, _| !key.starts_with(&prefix));
        }
    }
    field_error(w, element, None);
    synchronized(w);
    Ok(())
}
fn render_group(
    ui: &mut egui::Ui,
    w: &mut Workspace,
    owner: &str,
    scope: &str,
    target: &str,
    mate: bool,
    all_fields: &mut Vec<SemanticElement>,
) {
    let coordinates: Vec<Option<usize>> = if mate {
        vec![None]
    } else {
        (0..6).map(Some).collect()
    };
    for chunk in coordinates.chunks(3) {
        ui.columns(chunk.len(), |columns| {
            for (column, axis) in columns.iter_mut().zip(chunk) {
                let mut fields = all_fields
                    .iter()
                    .filter(|field| {
                        field
                            .data
                            .get(if mate { "mate" } else { "instance" })
                            .and_then(Value::as_str)
                            == Some(target)
                            && axis.is_none_or(|axis| {
                                field.data.get("axis").and_then(Value::as_u64) == Some(axis as u64)
                            })
                    })
                    .cloned()
                    .collect::<Vec<_>>();
                for index in 0..fields.len() {
                    let mut field = fields[index].clone();
                    if let Some((_, label)) = field.label.split_once(" · ") {
                        field.label = label.to_string();
                    }
                    let before = (
                        w.session.generation,
                        w.session
                            .draft
                            .as_ref()
                            .map(|draft| (draft.id.clone(), draft.edit)),
                    );
                    crate::ui::semantic_field(column, w, &field);
                    if (
                        w.session.generation,
                        w.session
                            .draft
                            .as_ref()
                            .map(|draft| (draft.id.clone(), draft.edit)),
                    ) != before
                    {
                        *all_fields = fields_for(w, owner, scope);
                        fields = all_fields
                            .iter()
                            .filter(|field| {
                                field
                                    .data
                                    .get(if mate { "mate" } else { "instance" })
                                    .and_then(Value::as_str)
                                    == Some(target)
                                    && axis.is_none_or(|axis| {
                                        field.data.get("axis").and_then(Value::as_u64)
                                            == Some(axis as u64)
                                    })
                            })
                            .cloned()
                            .collect();
                    }
                }
            }
        });
    }
}
pub(super) fn show(ui: &mut egui::Ui, w: &mut Workspace, table: bool) {
    let Some(owner) = owner(w) else {
        return;
    };
    let Some(assembly) = w.session.visible().assemblies.get(&owner).cloned() else {
        return;
    };
    let selected = w
        .fields
        .get("configuration-editor")
        .cloned()
        .unwrap_or_else(|| "base".into());
    egui::CollapsingHeader::new(if table {"Component pose table"} else {"Component placement seeds"})
        .default_open(true).show(ui,|ui| {
            crate::presentation_controls::label(ui,"configuration-placement-help","configuration-placement-help",&["configuration-field"],
                "Millimetres and Euler XYZ degrees. Unchecked fields inherit; mates solve the final placement. Pattern copies follow their seeds; expression translations use driving parameters.","ready");
            let scopes:Vec<Id>=if table {configuration::names(w.session.visible(),&owner).into_keys().collect()} else {vec![selected]};
            let generated=cad_core::component_pattern::generated(&assembly);
            for id in scopes {
                if ambiguous_scope(w,&owner,&id) {
                    let unavailable=unavailable_scope(&owner);
                    crate::presentation_controls::label(ui,&unavailable.id,&unavailable.role,&["configuration-field"],&unavailable.label,&unavailable.state);
                    continue;
                }
                let mut all_fields=fields_for(w,&owner,&id);
                let name=configuration::names(w.session.visible(),&owner).get(&id).cloned().unwrap_or_else(||"Base definition".into());
                if table {crate::presentation_controls::label(ui,&format!("configuration-placement-group:{owner}/{id}"),"configuration-placement-group",&[],&name,"ready");}
                for instance in assembly.instances.values().filter(|instance|!generated.contains(&instance.id)) {
                    let identity=format!("configuration-placement-group:{owner}/{id}/{}",instance.id);
                    let props=crate::presentation_controls::properties(&identity,"configuration-placement-group",&["configuration-field"],"ready");
                    if props.get("display").is_some_and(|display|display=="none") {continue;}
                    ui.scope(|ui| {
                        crate::presentation_controls::apply_properties(ui,&props);
                        egui::CollapsingHeader::new(&instance.name).id_salt(&identity).default_open(!table).show(ui,|ui| {
                            render_group(ui,w,&owner,&id,&instance.id,false,&mut all_fields);
                        });
                    });
                }
                for mate in &assembly.mates {
                    if matches!(mate.relationship.as_deref().and_then(cad_core::assembly_geometry::Relationship::curve_position),Some(cad_core::assembly_geometry::CurvePosition::Free{..})) {
                        render_group(ui,w,&owner,&id,&mate.id,true,&mut all_fields);
                    }
                }
            }
        });
}
