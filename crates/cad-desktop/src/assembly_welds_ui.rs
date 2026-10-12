//! Docked, styleable assembly weld editing over configured occurrence sources.
use crate::{Workspace, presentation_controls as controls, viewport};
use bevy_egui::egui;
use cad_core::{
    CadError, NodeRef, Result,
    commands::Command,
    presentation::SemanticElement,
    weld_beads::{Definition, FaceSource, Pattern, Profile, SeamSource},
};
use serde_json::{Value, json};

const EDITOR: &str = "assembly-weld-editor";
const INVALID: &str = "assembly-weld-source-invalid";
const ACTION: &str = "assembly.weld-action";
const ERRORS: &str = "assembly-weld-field-error:";
fn invalid(message: &str) -> CadError {
    CadError::Invalid(format!("Assembly weld: {message}"))
}
fn editor_owner(w: &Workspace) -> Option<&str> {
    w.fields.get("assembly-id").map(String::as_str)
}
fn context(w: &Workspace) -> String {
    let doc = w.session.visible();
    let instances = doc
        .assemblies
        .values()
        .map(|assembly| {
            (
                &assembly.id,
                assembly
                    .instances
                    .values()
                    .map(|instance| (&instance.id, &instance.definition, &instance.configuration))
                    .collect::<Vec<_>>(),
            )
        })
        .collect::<Vec<_>>();
    json!({"document":doc.id,"root":doc.root,"active":doc.active_configurations,"instances":instances}).to_string()
}
fn parsed(w: &Workspace) -> Result<Definition> {
    let source: Definition = serde_json::from_str(&w.json)?;
    if w.fields.get(EDITOR) != Some(&source.id) {
        return Err(invalid("Saved bead identity cannot change"));
    }
    Ok(source)
}
pub(crate) fn feedback_target(w: &Workspace) -> Option<NodeRef> {
    if !w.fields.contains_key(EDITOR) || !source_is_current(w) {
        return None;
    }
    Some(NodeRef {
        definition: editor_owner(w)?.into(),
        node: parsed(w).ok()?.id,
    })
}
fn source_is_current(w: &Workspace) -> bool {
    if !w.fields.contains_key(EDITOR) {
        return true;
    }
    let doc = w.session.visible();
    w.fields.get("assembly-weld-context") == Some(&context(w))
        && parsed(w).is_ok_and(|source| {
            editor_owner(w)
                .and_then(|owner| doc.assemblies.get(owner))
                .is_some_and(|assembly| {
                    if w.fields.contains_key("assembly-weld-deletion") {
                        !assembly.weld_beads.contains_key(&source.id)
                    } else {
                        assembly.weld_beads.get(&source.id) == Some(&source)
                    }
                })
        })
}
pub fn current_source_is_valid(w: &Workspace) -> bool {
    source_is_current(w) && !w.fields.keys().any(|key| key.starts_with(ERRORS))
}
fn guard(w: &mut Workspace) {
    if source_is_current(w) {
        w.fields.remove(INVALID);
    } else {
        w.fields.insert(INVALID.into(), "true".into());
    }
}
pub fn cancel(w: &mut Workspace) {
    w.fields
        .retain(|key, _| !key.starts_with("assembly-weld-") && !key.starts_with(ERRORS));
}
fn busy(w: &Workspace) -> bool {
    w.tool_checkpoint.is_some()
        || w.sketch.is_some()
        || w.session.draft.is_some()
        || w.confirm_requested
        || w.assembly_motion.is_some()
}
fn start(w: &mut Workspace, owner: &str, source: Definition) -> bool {
    if busy(w) {
        w.status = "Confirm or cancel the current operation before editing an assembly weld".into();
        return false;
    }
    if !w.session.visible().assemblies.contains_key(owner) {
        w.status = "The owning assembly is missing".into();
        return false;
    }
    let snapshot = context(w);
    w.start_tool();
    w.fields.insert("assembly-id".into(), owner.into());
    w.fields.insert("assembly-weld-context".into(), snapshot);
    w.fields
        .insert("assembly-weld-generation".into(), cad_core::id());
    w.fields.insert(EDITOR.into(), source.id.clone());
    w.json = serde_json::to_string_pretty(&source).unwrap_or_default();
    w.open_editor("assembly");
    true
}
fn submit(w: &mut Workspace, source: Definition) -> Result<()> {
    let owner = editor_owner(w)
        .ok_or_else(|| invalid("Owning assembly is missing"))?
        .to_string();
    if w.fields.get("assembly-weld-context") != Some(&context(w)) {
        return Err(invalid(
            "Assembly or configuration context changed; cancel and reopen the weld",
        ));
    }
    if w.fields.get(EDITOR) != Some(&source.id) {
        return Err(invalid("Saved bead identity cannot change"));
    }
    let expected = source.clone();
    let mut document = w.session.visible().clone();
    (Command::PutAssemblyWeld {
        assembly: owner.clone(),
        weld: Box::new(source),
    })
    .execute(&mut document)?;
    w.dispatch(Command::ReplaceDocument { document });
    if !w
        .session
        .visible()
        .assemblies
        .get(&owner)
        .is_some_and(|assembly| assembly.weld_beads.get(&expected.id) == Some(&expected))
    {
        return Err(invalid(&w.status));
    }
    w.json = serde_json::to_string_pretty(&expected)?;
    guard(w);
    Ok(())
}
/// Convert a selected root occurrence path to the owning assembly's frame.
/// Never drop a path by guessing a leaf definition or instance name.
fn relative_path(w: &Workspace, owner: &str, path: &[String]) -> Option<Vec<String>> {
    let doc = w.session.visible();
    let mut current = doc.root.as_str();
    for index in 0..=path.len() {
        if current == owner {
            return (index < path.len()).then(|| path[index..].to_vec());
        }
        let id = path.get(index)?;
        current = &doc.assemblies.get(current)?.instances.get(id)?.definition;
    }
    None
}
fn selected_seam(w: &Workspace, owner: &str) -> Option<SeamSource> {
    let mut seam = crate::weld_beads_ui::selected_seam(w, false)?;
    let path = relative_path(w, owner, seam.path())?;
    match &mut seam {
        SeamSource::ModelEdge { path: target, .. }
        | SeamSource::SketchMember { path: target, .. }
        | SeamSource::ReferenceCurve { path: target, .. } => *target = path,
    }
    Some(seam)
}
fn selected_supports(w: &Workspace, owner: &str) -> Vec<FaceSource> {
    crate::weld_beads_ui::selected_supports(w, false)
        .into_iter()
        .filter_map(|mut face| {
            face.path = relative_path(w, owner, &face.path)?;
            Some(face)
        })
        .collect()
}
pub(crate) fn joint_pick_source(w: &Workspace) -> Option<(NodeRef, Definition, String)> {
    if !w.fields.contains_key(EDITOR)
        || w.fields.contains_key("assembly-weld-deletion")
        || !source_is_current(w)
    {
        return None;
    }
    let definition = parsed(w).ok()?;
    Some((
        NodeRef {
            definition: editor_owner(w)?.into(),
            node: definition.id.clone(),
        },
        definition,
        w.fields.get("assembly-weld-generation")?.clone(),
    ))
}
pub(crate) fn picked_joint_seam(w: &Workspace, owner: &str) -> Option<SeamSource> {
    selected_seam(w, owner)
}
pub(crate) fn picked_joint_support(w: &Workspace, owner: &str) -> Option<FaceSource> {
    let selected = w
        .selected
        .as_ref()
        .filter(|selected| selected.reference.topology == "face")?;
    Some(FaceSource {
        body: crate::reference_ui::selected_body(w, selected),
        reference: Box::new(selected.reference.clone()),
        path: relative_path(w, owner, &selected.path)?,
    })
}
pub(crate) fn submit_joint_pick(w: &mut Workspace, definition: Definition) -> Result<()> {
    submit(w, definition)
}
pub fn new(w: &mut Workspace, owner: &str) {
    let Some(seam) = selected_seam(w, owner) else {
        w.status="Select a model edge, sketch member or reference curve on a component in the owning assembly".into();
        return;
    };
    let Some(assembly) = w.session.visible().assemblies.get(owner) else {
        w.status = "Select the owning assembly first".into();
        return;
    };
    let name = (1..)
        .map(|n| format!("Weld Bead{n}"))
        .find(|name| {
            !assembly
                .weld_beads
                .values()
                .any(|bead| bead.name.eq_ignore_ascii_case(name))
        })
        .unwrap();
    let mut source = Definition::new(cad_core::id(), name, seam);
    let supports = selected_supports(w, owner);
    if supports.len() == 2 {
        source.supports = supports;
    }
    if start(w, owner, source.clone())
        && let Err(error) = submit(w, source)
    {
        w.status = error.to_string();
        guard(w);
    }
}
pub fn open(w: &mut Workspace, owner: &str, id: &str) {
    let source = w
        .session
        .visible()
        .assemblies
        .get(owner)
        .and_then(|assembly| assembly.weld_beads.get(id))
        .cloned();
    if let Some(source) = source {
        if start(w, owner, source) {
            guard(w);
        }
    } else {
        w.status = "The saved weld does not belong to this assembly".into();
    }
}
fn editable(source: &Definition) -> Value {
    let mut value = json!(source);
    value["finish"] = json!(source.finish);
    value["material"] = json!(source.material);
    value["supports"] = json!(source.supports);
    value
}
fn revision(w: &Workspace) -> Value {
    json!([
        w.session.generation,
        w.session.committed.revision,
        w.session
            .draft
            .as_ref()
            .map(|draft| (&draft.id, draft.edit))
    ])
}
fn metadata(w: &Workspace) -> Value {
    json!({"contract":"tcad.presentation.assembly-weld-editor","version":1,"document":w.session.visible().id,"assembly":editor_owner(w),"weld":w.fields.get(EDITOR),"editor":w.fields.get("assembly-weld-generation"),"source_revision":revision(w),"shared_draft":true,"requires_current_validation":true})
}
fn identity(w: &Workspace, key: &str) -> String {
    format!(
        "assembly-weld-editor:{}/{key}",
        w.fields
            .get(EDITOR)
            .map(String::as_str)
            .unwrap_or("missing")
    )
}
fn choice<T: serde::Serialize>(label: impl Into<String>, value: T) -> Value {
    json!({"label":label.into(),"value":value})
}
fn group(w: &Workspace, out: &mut Vec<SemanticElement>, key: &str, label: &str) {
    let mut data = metadata(w);
    data["group"] = json!(key);
    data["classes"] = json!(["modeling-group", key]);
    out.push(SemanticElement {
        id: identity(w, &format!("group/{key}")),
        role: "assembly-weld-group".into(),
        label: label.into(),
        value: None,
        state: "draft".into(),
        commands: vec![],
        relationships: vec![w.fields.get(EDITOR).cloned().unwrap_or_default()],
        data,
    });
}
#[allow(clippy::too_many_arguments)]
fn field(
    w: &Workspace,
    out: &mut Vec<SemanticElement>,
    source: &Value,
    group: &str,
    path: &str,
    label: &str,
    unit: Option<&str>,
    mut choices: Vec<Value>,
) {
    let Some(value) = source.pointer(path).cloned() else {
        return;
    };
    if !choices.is_empty() && !choices.iter().any(|choice| choice["value"] == value) {
        choices.push(choice("Saved source", value.clone()));
    }
    let id = identity(w, path.trim_start_matches('/'));
    let enabled = source_is_current(w)
        && !w.fields.contains_key("assembly-weld-deletion")
        && w.settings.is_none();
    let mut data = metadata(w);
    data["setter"] = json!("assembly-weld");
    data["pointer"] = json!(path);
    data["json"] = value.clone();
    data["editable"] = json!(enabled);
    data["choices"] = json!(choices);
    data["unit"] = json!(unit);
    data["group"] = json!(group);
    data["classes"] = json!(["modeling-field", group]);
    data["accessibility_label"] = json!(label);
    data["accepts_quantity_text"] =
        json!(value.is_number() && unit.is_some_and(|unit| !unit.is_empty()));
    data["dimension"] = json!(match unit {
        Some("mm") => "length",
        Some("deg") => "angle",
        _ => "scalar",
    });
    data["value_type"] = json!(if !choices.is_empty() {
        if value.is_object() {
            "reference"
        } else {
            "choice"
        }
    } else if value.is_boolean() {
        "boolean"
    } else if value.is_number() {
        "number"
    } else if unit.is_some() {
        "quantity"
    } else {
        "text"
    });
    if let Some(error) = w.fields.get(&format!("{ERRORS}{id}")) {
        data["diagnostics"] = json!([error]);
    }
    out.push(SemanticElement {
        id,
        role: "assembly-weld-field".into(),
        label: label.into(),
        value: Some(
            value
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| value.to_string()),
        ),
        state: if enabled { "draft" } else { "disabled" }.into(),
        commands: vec![],
        relationships: vec![w.fields.get(EDITOR).cloned().unwrap_or_default()],
        data,
    });
}
fn command_element(
    w: &Workspace,
    out: &mut Vec<SemanticElement>,
    key: &str,
    label: &str,
    command: &str,
    mutation: Value,
    enabled: bool,
) {
    let id = identity(w, &format!("action/{key}"));
    let mut data = metadata(w);
    data["element"] = json!(id);
    data["mutation"] = mutation;
    data["actions"] = json!([{"command":command,"label":label,"enabled":enabled,"disabled_reason":(!enabled).then_some("Use the current configured assembly source and selected geometry"),"arguments":data.clone()}]);
    out.push(SemanticElement {
        id,
        role: "assembly-weld-action".into(),
        label: label.into(),
        value: None,
        state: if enabled { "ready" } else { "disabled" }.into(),
        commands: vec![command.into()],
        relationships: vec![w.fields.get(EDITOR).cloned().unwrap_or_default()],
        data,
    });
}
fn geometry_choices(w: &Workspace, owner: &str) -> (Vec<Value>, Vec<Value>) {
    let Some(evaluation) = viewport::selection_evaluation(w) else {
        return (vec![], vec![]);
    };
    let mut seams = vec![];
    let mut supports = vec![];
    for instance in &evaluation.instances {
        let Some(path) = relative_path(w, owner, &instance.path) else {
            continue;
        };
        if !w.session.visible().parts.contains_key(&instance.definition) {
            continue;
        }
        let Some(mesh) = evaluation.meshes.get(&instance.mesh_key) else {
            continue;
        };
        for (index, edge) in mesh.edges.iter().enumerate() {
            let Some(body) = &edge.body else {
                continue;
            };
            if body.definition != instance.definition {
                continue;
            }
            let source = SeamSource::ModelEdge {
                body: body.clone(),
                reference: Box::new(edge.reference.clone()),
                path: path.clone(),
            };
            let value = json!(source);
            if seams.len() < 4096 && !seams.iter().any(|choice: &Value| choice["value"] == value) {
                seams.push(choice(
                    format!("{} · edge {}", instance.name, index + 1),
                    source,
                ));
            }
        }
        for (index, face) in mesh.faces.iter().enumerate() {
            let Some(body) = &face.body else {
                continue;
            };
            if body.definition != instance.definition {
                continue;
            }
            let source = FaceSource {
                body: body.clone(),
                reference: Box::new(face.reference.clone()),
                path: path.clone(),
            };
            let value = json!(source);
            if supports.len() < 4096
                && !supports
                    .iter()
                    .any(|choice: &Value| choice["value"] == value)
            {
                supports.push(choice(
                    format!(
                        "{} · {}",
                        instance.name,
                        crate::weld_beads_ui::face_caption(w, face, index)
                    ),
                    source,
                ));
            }
        }
        if seams.len() >= 4096 && supports.len() >= 4096 {
            break;
        }
    }
    if let Some(selected) = selected_seam(w, owner) {
        let value = json!(selected);
        if !seams.iter().any(|choice| choice["value"] == value) {
            seams.insert(0, choice("Selected seam", selected));
        }
    }
    (seams, supports)
}
fn scoped_suppression(w: &Workspace, owner: &str, bead: &Definition) -> bool {
    w.session
        .visible()
        .active_configurations
        .get(owner)
        .and_then(|id| cad_core::configuration::resolve(w.session.visible(), owner, id).ok())
        .and_then(|resolved| resolved.overrides.assembly_welds.get(&bead.id).copied())
        .unwrap_or(bead.suppressed)
}
pub fn semantics(w: &Workspace) -> Vec<SemanticElement> {
    let mut out = vec![];
    for assembly in w.session.visible().assemblies.values() {
        for bead in assembly.weld_beads.values() {
            let enabled = !busy(w) && w.settings.is_none();
            let suppressed = scoped_suppression(w, &assembly.id, bead);
            let scope = if w
                .session
                .visible()
                .active_configurations
                .contains_key(&assembly.id)
            {
                "Selected named configuration"
            } else {
                "Base definition"
            };
            let args = json!({"document":w.session.visible().id,"assembly":assembly.id,"weld":bead.id,"source":bead,"source_revision":revision(w),"configuration":w.session.visible().active_configurations.get(&assembly.id)});
            out.push(SemanticElement{id:format!("assembly-weld:{}/{}",assembly.id,bead.id),role:"assembly-weld".into(),label:format!("{} / {}",assembly.name,bead.name),value:Some(if suppressed {"Suppressed"} else {"Deposited weld metal"}.into()),state:if suppressed {"suppressed"} else {"committed"}.into(),commands:vec!["assembly.weld-edit".into(),"assembly.weld-suppress".into(),"assembly.weld-delete".into()],relationships:vec![assembly.id.clone(),bead.seam.dependency().node.clone()],data:json!({"contract":"tcad.weld_beads","version":cad_core::weld_beads::CONTRACT_VERSION,"source":bead,"target":args,"suppression_scope":scope,"actions":[{"command":"assembly.weld-edit","label":"Edit weld","enabled":enabled,"arguments":args},{"command":"assembly.weld-suppress","label":if suppressed {format!("Unsuppress · {scope}")}else{format!("Suppress · {scope}")},"enabled":enabled,"arguments":args},{"command":"assembly.weld-delete","label":"Delete weld","enabled":enabled,"arguments":args}],"real_solid":true})});
        }
    }
    if !w.fields.contains_key(EDITOR) {
        return out;
    }
    let Some(owner) = editor_owner(w) else {
        return out;
    };
    let Ok(definition) = parsed(w) else {
        return out;
    };
    if w.fields.contains_key("assembly-weld-deletion") {
        group(
            w,
            &mut out,
            "deletion",
            &format!("Delete {}", definition.name),
        );
        return out;
    }
    let source = editable(&definition);
    group(w, &mut out, "identity", "Assembly weld");
    field(
        w,
        &mut out,
        &source,
        "identity",
        "/name",
        "Name",
        None,
        vec![],
    );
    field(
        w,
        &mut out,
        &source,
        "identity",
        "/suppressed",
        "Base weld suppression",
        None,
        vec![],
    );
    group(w, &mut out, "seam", "Seam and support faces");
    let mut note = |key: &str, label: &str| {
        out.push(SemanticElement {
        id: identity(w, key), role: "assembly-weld-help".into(), label: label.into(), value: None,
        state: "ready".into(), commands: vec![], relationships: vec![definition.id.clone()],
        data: json!({"group":"seam","classes":["modeling-status","weld-feedback"],"owner":owner,"weld":definition.id}),
    })
    };
    note(
        "joint-requirements",
        crate::weld_beads_ui::joint_help(&definition),
    );
    note(
        "owner-frame",
        "Seam and support geometry is evaluated in the owning assembly's frame using each component's current solved placement.",
    );
    out.extend(crate::weld_beads_ui::feedback(w, owner, &definition.id));
    out.extend(crate::weld_beads_ui::pick_actions(w));
    let (seams, supports) = geometry_choices(w, owner);
    field(
        w,
        &mut out,
        &source,
        "seam",
        "/seam",
        "Seam source",
        None,
        seams,
    );
    let selected = selected_seam(w, owner);
    command_element(
        w,
        &mut out,
        "selected-seam",
        "Use selected seam",
        ACTION,
        json!({"kind":"set","pointer":"/seam","value":selected}),
        source_is_current(w) && selected.is_some(),
    );
    for index in 0..definition.supports.len() {
        field(
            w,
            &mut out,
            &source,
            "seam",
            &format!("/supports/{index}"),
            &format!("Support face {}", index + 1),
            None,
            supports.clone(),
        );
        command_element(
            w,
            &mut out,
            &format!("remove-support/{index}"),
            "Remove support face",
            ACTION,
            json!({"kind":"remove_support","index":index}),
            source_is_current(w),
        );
    }
    let maximum = 2;
    let selected = selected_supports(w, owner)
        .into_iter()
        .find(|face| !definition.supports.contains(face));
    command_element(
        w,
        &mut out,
        "add-support",
        "Add selected support face",
        ACTION,
        json!({"kind":"add_support","value":selected}),
        source_is_current(w) && selected.is_some() && definition.supports.len() < maximum,
    );
    command_element(
        w,
        &mut out,
        "infer-supports",
        "Infer contact-proven joint faces",
        ACTION,
        json!({"kind":"set","pointer":"/supports","value":[]}),
        source_is_current(w) && !definition.supports.is_empty(),
    );
    group(w, &mut out, "profile", "Visible deposited metal");
    field(
        w,
        &mut out,
        &source,
        "profile",
        "/profile",
        "Weld profile",
        None,
        vec![
            choice("Fillet bead", Profile::Fillet),
            choice("Butt bead", Profile::Butt),
        ],
    );
    field(
        w,
        &mut out,
        &source,
        "profile",
        "/size",
        if definition.profile == Profile::Fillet {
            "Leg size"
        } else {
            "Bead width"
        },
        Some("mm"),
        vec![],
    );
    field(
        w,
        &mut out,
        &source,
        "profile",
        "/reinforcement",
        "Crown rise",
        Some("mm"),
        vec![],
    );
    let mut patterns = vec![
        choice("Continuous", Pattern::Continuous),
        choice(
            "Intermittent",
            Pattern::Intermittent {
                length: "20 mm".into(),
                gap: "20 mm".into(),
            },
        ),
    ];
    if let Pattern::Intermittent { .. } = &definition.pattern {
        patterns[1] = choice("Intermittent", &definition.pattern);
    }
    field(
        w,
        &mut out,
        &source,
        "profile",
        "/pattern",
        "Deposition",
        None,
        patterns,
    );
    if let Pattern::Intermittent { .. } = &definition.pattern {
        field(
            w,
            &mut out,
            &source,
            "profile",
            "/pattern/length",
            "Stitch length",
            Some("mm"),
            vec![],
        );
        field(
            w,
            &mut out,
            &source,
            "profile",
            "/pattern/gap",
            "Clear stitch gap",
            Some("mm"),
            vec![],
        );
    }
    field(
        w,
        &mut out,
        &source,
        "profile",
        "/start_offset",
        "Start trim",
        Some("mm"),
        vec![],
    );
    field(
        w,
        &mut out,
        &source,
        "profile",
        "/end_offset",
        "End trim",
        Some("mm"),
        vec![],
    );
    if !matches!(definition.seam, SeamSource::ModelEdge { .. }) && definition.supports.is_empty() {
        group(
            w,
            &mut out,
            "orientation",
            "Owning assembly frame · bead orientation",
        );
        for (index, axis) in ["X", "Y", "Z"].iter().enumerate() {
            field(
                w,
                &mut out,
                &source,
                "orientation",
                &format!("/direction/{index}"),
                &format!("Owner-frame direction {axis}"),
                Some(""),
                vec![],
            );
        }
        field(
            w,
            &mut out,
            &source,
            "orientation",
            "/roll",
            "Roll about seam",
            Some("deg"),
            vec![],
        );
    } else {
        out.push(SemanticElement {
            id: identity(w, "joint-orientation"), role: "assembly-weld-help".into(),
            label: "Joint faces determine bead orientation; direction and roll apply only to an authored seam without support faces.".into(), value: None, state: "ready".into(), commands: vec![], relationships: vec![definition.id.clone()], data: json!({"group":"seam","classes":["modeling-status","weld-feedback"]}),
        });
        if definition
            .values(w.session.visible(), owner)
            .is_ok_and(|values| values.roll_rad.abs() > 1e-10)
        {
            command_element(
                w,
                &mut out,
                "reset-joint-roll",
                "Use joint orientation",
                ACTION,
                json!({"kind":"set","pointer":"/roll","value":"0 deg"}),
                source_is_current(w),
            );
        }
    }
    group(
        w,
        &mut out,
        "assignments",
        "Finish and engineering material",
    );
    let mut finishes = vec![choice("Default finish", Option::<String>::None)];
    finishes.extend(
        w.session
            .visible()
            .finishes
            .values()
            .map(|finish| choice(&finish.name, Some(&finish.id))),
    );
    field(
        w,
        &mut out,
        &source,
        "assignments",
        "/finish",
        "Optical finish",
        None,
        finishes,
    );
    let mut materials = vec![choice(
        "Default engineering material",
        Option::<String>::None,
    )];
    materials.extend(
        w.session
            .visible()
            .materials
            .values()
            .map(|material| choice(&material.name, Some(&material.id))),
    );
    field(
        w,
        &mut out,
        &source,
        "assignments",
        "/material",
        "Engineering material",
        None,
        materials,
    );
    out
}
fn live_field(w: &Workspace, element: &SemanticElement) -> Result<SemanticElement> {
    let fresh = semantics(w)
        .into_iter()
        .find(|fresh| fresh.id == element.id && fresh.role == "assembly-weld-field")
        .ok_or_else(|| invalid("The weld field is no longer available"))?;
    for key in [
        "document",
        "assembly",
        "weld",
        "editor",
        "source_revision",
        "pointer",
    ] {
        if fresh.data[key] != element.data[key] {
            return Err(invalid(
                "The weld source changed; use the current editor field",
            ));
        }
    }
    if !source_is_current(w) {
        return Err(invalid(
            "Preview the current configured source before changing weld fields",
        ));
    }
    Ok(fresh)
}
pub fn current_value(w: &Workspace, element: &SemanticElement) -> Option<Value> {
    live_field(w, element).ok()?;
    editable(&parsed(w).ok()?)
        .pointer(element.data["pointer"].as_str()?)
        .cloned()
}
pub fn field_is_invalid(w: &Workspace, element: &SemanticElement) -> bool {
    element.data["setter"].as_str() == Some("assembly-weld")
        && w.fields.contains_key(&format!("{ERRORS}{}", element.id))
}
pub fn field_error(w: &mut Workspace, element: &SemanticElement, error: Option<String>) {
    if element.data["setter"].as_str() != Some("assembly-weld") {
        return;
    }
    let key = format!("{ERRORS}{}", element.id);
    if let Some(error) = error {
        w.fields.insert(key, error);
    } else {
        w.fields.remove(&key);
    }
}
pub fn set_value(w: &mut Workspace, element: &SemanticElement, value: Value) -> Result<()> {
    let element = live_field(w, element)?;
    let pointer = element.data["pointer"]
        .as_str()
        .ok_or_else(|| invalid("Weld field pointer is missing"))?;
    let mut source = editable(&parsed(w)?);
    let target = source
        .pointer_mut(pointer)
        .ok_or_else(|| invalid("Weld field is no longer present"))?;
    *target = value;
    if pointer == "/seam" {
        *source
            .pointer_mut("/supports")
            .ok_or_else(|| invalid("Support list is missing"))? = json!([]);
    }
    if pointer == "/profile" {
        source["supports"]
            .as_array_mut()
            .ok_or_else(|| invalid("Support list is missing"))?
            .truncate(2);
    }
    let next: Definition = serde_json::from_value(source)?;
    submit(w, next)
}
fn active_action(w: &mut Workspace, data: &Value) -> Result<()> {
    let id = data["element"]
        .as_str()
        .ok_or_else(|| invalid("The weld action has no target"))?;
    let fresh = semantics(w)
        .into_iter()
        .find(|element| element.id == id)
        .ok_or_else(|| invalid("The weld action is no longer available"))?;
    for key in [
        "document",
        "assembly",
        "weld",
        "editor",
        "source_revision",
        "mutation",
    ] {
        if fresh.data[key] != data[key] {
            return Err(invalid(
                "Selection or source changed; use the current weld action",
            ));
        }
    }
    if !source_is_current(w)
        || !fresh
            .action_descriptors()
            .iter()
            .any(|action| action.enabled)
    {
        return Err(invalid("The current weld action is unavailable"));
    }
    let mutation = &data["mutation"];
    let mut source = parsed(w)?;
    match mutation["kind"].as_str() {
        Some("set") if mutation["pointer"] == "/seam" => {
            source.seam = serde_json::from_value(mutation["value"].clone())?;
            source.supports.clear();
        }
        Some("set") if mutation["pointer"] == "/supports" => source.supports.clear(),
        Some("add_support") => source
            .supports
            .push(serde_json::from_value(mutation["value"].clone())?),
        Some("remove_support") => {
            let index = mutation["index"]
                .as_u64()
                .ok_or_else(|| invalid("Support index is missing"))?
                as usize;
            if index >= source.supports.len() {
                return Err(invalid("The selected support changed"));
            }
            source.supports.remove(index);
        }
        _ => return Err(invalid("The weld action has no supported mutation")),
    }
    submit(w, source)
}
pub fn action(w: &mut Workspace, command: &str, data: Option<&Value>) -> bool {
    if command == "weld.bead" {
        if w.session
            .visible()
            .assemblies
            .contains_key(&w.session.visible().root)
        {
            let owner = w.session.visible().root.clone();
            new(w, &owner);
            return true;
        }
        return false;
    }
    if !command.starts_with("assembly.weld-") {
        return false;
    }
    let result = (|| -> Result<()> {
        if w.settings.is_some() {
            return Err(invalid("Close Settings before editing a weld"));
        }
        if command == ACTION {
            return active_action(
                w,
                data.ok_or_else(|| invalid("Choose a current weld action"))?,
            );
        }
        if command == "assembly.weld-preview-source" {
            let source = parsed(w)?;
            return submit(w, source);
        }
        let target = data.and_then(|value| value.get("target")).or(data);
        if target
            .and_then(|data| data.get("document"))
            .and_then(Value::as_str)
            .is_some_and(|id| id != w.session.visible().id)
        {
            return Err(invalid("This weld action belongs to another document"));
        }
        let geometry = target
            .and_then(|value| {
                serde_json::from_value::<cad_core::selection_actions::SelectionTarget>(
                    value.clone(),
                )
                .ok()
            })
            .and_then(|target| match target {
                cad_core::selection_actions::SelectionTarget::Geometry {
                    definition,
                    body: Some(body),
                    ..
                } => Some((definition, body.node)),
                _ => None,
            });
        let owner = geometry
            .as_ref()
            .map(|(owner, _)| owner.clone())
            .or_else(|| {
                target
                    .and_then(|data| data.get("assembly"))
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            })
            .or_else(|| editor_owner(w).map(str::to_owned))
            .unwrap_or_else(|| w.session.visible().root.clone());
        if command == "assembly.weld-new" {
            new(w, &owner);
            return Ok(());
        }
        let id = geometry
            .map(|(_, id)| id)
            .or_else(|| {
                target
                    .and_then(|data| data.get("weld"))
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            })
            .or_else(|| w.fields.get(EDITOR).cloned())
            .ok_or_else(|| invalid("Select a saved weld first"))?;
        let source = w
            .session
            .visible()
            .assemblies
            .get(&owner)
            .and_then(|assembly| assembly.weld_beads.get(&id))
            .cloned()
            .ok_or_else(|| invalid("Saved weld is missing from its owning assembly"))?;
        if let Some(saved) = target.and_then(|data| data.get("source"))
            && *saved != json!(source)
        {
            return Err(invalid(
                "The saved weld changed; choose its current tree action",
            ));
        }
        if let Some(saved) = target.and_then(|data| data.get("source_revision"))
            && *saved != revision(w)
        {
            return Err(invalid(
                "The assembly source changed; use its current weld action",
            ));
        }
        if let Some(saved) = target.and_then(|data| data.get("configuration"))
            && *saved != json!(w.session.visible().active_configurations.get(&owner))
        {
            return Err(invalid(
                "The named configuration changed; use its current weld action",
            ));
        }
        match command {
            "assembly.weld-edit" => open(w, &owner, &id),
            "assembly.weld-suppress" => {
                let selected = w
                    .session
                    .visible()
                    .active_configurations
                    .get(&owner)
                    .cloned();
                let suppressed = scoped_suppression(w, &owner, &source);
                let named = selected
                    .map(|id| {
                        cad_core::configuration::definitions(w.session.visible(), &owner)
                            .remove(&id)
                            .ok_or_else(|| invalid("The selected named configuration is missing"))
                    })
                    .transpose()?;
                if start(w, &owner, source.clone()) {
                    if let Some(mut configuration) = named {
                        configuration.version = cad_core::configuration::CONTRACT_VERSION;
                        configuration
                            .overrides
                            .assembly_welds
                            .insert(source.id.clone(), !suppressed);
                        w.dispatch(Command::PutConfiguration {
                            definition: owner,
                            configuration: Box::new(configuration),
                        });
                        guard(w);
                    } else {
                        let mut source = source;
                        source.suppressed = !suppressed;
                        submit(w, source)?;
                    }
                }
            }
            "assembly.weld-delete" => {
                if start(w, &owner, source.clone()) {
                    w.dispatch(Command::DeleteAssemblyWeld {
                        assembly: owner,
                        weld: id,
                    });
                    w.fields
                        .insert("assembly-weld-deletion".into(), "true".into());
                    guard(w);
                }
            }
            _ => return Err(invalid("Unknown assembly weld command")),
        }
        Ok(())
    })();
    if let Err(error) = result {
        w.status = error.to_string();
        guard(w);
    }
    true
}
pub fn show(ui: &mut egui::Ui, w: &mut Workspace) {
    guard(w);
    for element in semantics(w).into_iter().filter(|element| {
        element.id.starts_with("assembly-weld-editor:") || element.role == "weld-joint-feedback"
    }) {
        match element.role.as_str() {
            "assembly-weld-group" => {
                controls::label(
                    ui,
                    &element.id,
                    &element.role,
                    &["modeling-group"],
                    &element.label,
                    &element.state,
                );
            }
            "assembly-weld-field" => crate::ui::semantic_field(ui, w, &element),
            "assembly-weld-help" | "weld-joint-feedback" => {
                controls::label(
                    ui,
                    &element.id,
                    &element.role,
                    &["modeling-status", "weld-feedback"],
                    &element.label,
                    &element.state,
                );
            }
            "assembly-weld-action"
                if controls::button(
                    ui,
                    &element.id,
                    &element.role,
                    &["modeling-action"],
                    &element.label,
                    element
                        .action_descriptors()
                        .iter()
                        .any(|action| action.enabled),
                    false,
                )
                .clicked() =>
            {
                action(w, ACTION, Some(&element.data));
            }
            _ => {}
        }
    }
    if let Some(draft) = &w.session.draft {
        let status = draft.error.as_deref().unwrap_or(if draft.pending {
            "Updating deposited weld metal…"
        } else {
            "Current weld preview evaluated"
        });
        controls::label(
            ui,
            "assembly-weld-preview",
            "preview-status",
            &["modeling-status"],
            status,
            if draft.error.is_some() {
                "invalid"
            } else if draft.pending {
                "pending"
            } else {
                "ready"
            },
        );
    }
    if w.fields.contains_key(INVALID) {
        controls::label(
            ui,
            "assembly-weld-source-status",
            "source-diagnostic",
            &[],
            "Preview the edited source in its original assembly/configuration before applying.",
            "invalid",
        );
    }
    egui::CollapsingHeader::new("Advanced Source")
        .id_salt("assembly-weld-source")
        .show(ui, |ui| {
            if ui
                .add(
                    egui::TextEdit::multiline(&mut w.json)
                        .code_editor()
                        .desired_rows(8),
                )
                .changed()
            {
                guard(w);
            }
            if controls::button(
                ui,
                "assembly-weld-preview-source",
                "assembly-weld-action",
                &[],
                "Preview edited source",
                w.fields.get("assembly-weld-context") == Some(&context(w))
                    && !w.fields.contains_key("assembly-weld-deletion"),
                false,
            )
            .clicked()
            {
                action(w, "assembly.weld-preview-source", None);
            }
        });
    crate::cad_ui::confirmation(ui, w);
}
pub fn tree(ui: &mut egui::Ui, w: &mut Workspace) {
    let elements = semantics(w)
        .into_iter()
        .filter(|element| element.role == "assembly-weld")
        .collect::<Vec<_>>();
    if elements.is_empty() {
        return;
    }
    controls::label(
        ui,
        "assembly-weld-tree",
        "feature-tree-group",
        &[],
        "Weld Beads",
        "ready",
    );
    for element in elements {
        ui.horizontal(|ui| {
            if controls::button(
                ui,
                &element.id,
                &element.role,
                &["feature-tree-row"],
                &element.label,
                !busy(w),
                false,
            )
            .clicked()
            {
                action(w, "assembly.weld-edit", Some(&element.data));
            }
            for (command, label) in [
                ("assembly.weld-suppress", "Suppress / restore"),
                ("assembly.weld-delete", "Delete"),
            ] {
                if controls::button(
                    ui,
                    &format!("{}:{command}", element.id),
                    "assembly-weld-action",
                    &["tree-action"],
                    label,
                    !busy(w),
                    false,
                )
                .clicked()
                {
                    action(w, command, Some(&element.data));
                }
            }
        });
    }
}
