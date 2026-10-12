//! Visual finishes and studio lighting use the shared document transaction.
//! Engineering materials remain independent of these renderer-owned effects.
use crate::{Workspace, cad_ui, presentation_controls as controls};
use bevy_egui::egui;
use cad_core::{
    CadError, Id, Result,
    appearance::{Finish, Lighting},
    commands::Command,
    presentation::SemanticElement,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const EDITOR: &str = "appearance-editor";
const TOOL: &str = "appearance-tool";
const SOURCE: &str = "appearance-source";
const INVALID: &str = "appearance-source-invalid";
const IMPORT: &str = "appearance.texture.import/";
const CLEAR: &str = "appearance.texture.clear/";
const TARGET_PART: &str = "appearance-target-part";
const TARGET_SCOPE: &str = "appearance-target-scope";
const TARGET_INSTANCE: &str = "appearance-target-instance";
const TARGET_FACE: &str = "appearance-target-face";
const EDIT_MODE: &str = "appearance-edit-mode";
const CUSTOM_TARGET: &str = "appearance-custom-target";
const CUSTOM_FINISH: &str = "appearance-custom-finish";
const SECTION_TOGGLE: &str = "appearance.section.toggle/";
const SECTIONS: &[(&str, &str, bool)] = &[
    ("shared-library", "Shared finish library", false),
    ("library-details", "Library description and tags", false),
    ("manufacturing", "Manufacturing finish requirements", false),
    ("color-values", "Color values", false),
    ("surface", "Surface response", true),
    ("textures", "Textures · import images", true),
    ("mapping", "Tile size and mapping", true),
    ("source", "Advanced source", false),
];

fn selected(w: &Workspace) -> Option<Id> {
    w.fields
        .get(EDITOR)
        .cloned()
        .filter(|id| w.session.visible().finishes.contains_key(id))
}
fn initial(w: &Workspace) -> Option<Id> {
    assigned_finish(w).or_else(|| w.session.visible().finishes.keys().next().cloned())
}
fn target_part(w: &Workspace) -> Option<Id> {
    w.fields
        .get(TARGET_PART)
        .or(w.view.active_part.as_ref())
        .filter(|id| w.session.visible().parts.contains_key(*id))
        .cloned()
        .or_else(|| w.session.visible().parts.keys().next().cloned())
}
fn instance_choices(w: &Workspace) -> Vec<(Id, Id, String)> {
    let part = target_part(w);
    w.session
        .visible()
        .assemblies
        .values()
        .flat_map(|assembly| {
            assembly
                .instances
                .values()
                .filter(|instance| part.as_ref() == Some(&instance.definition))
                .map(|instance| {
                    (
                        assembly.id.clone(),
                        instance.id.clone(),
                        format!("{} / {}", assembly.name, instance.name),
                    )
                })
        })
        .collect()
}
fn instance_key(assembly: &str, instance: &str) -> String {
    format!("{assembly}/{instance}")
}
fn target_instance(w: &Workspace) -> Option<(Id, Id)> {
    let choices = instance_choices(w);
    if let Some(key) = w.fields.get(TARGET_INSTANCE) {
        return choices.into_iter().find_map(|(assembly, instance, _)| {
            (instance_key(&assembly, &instance) == *key).then_some((assembly, instance))
        });
    }
    let id = w.view.active_instance.as_ref()?;
    let mut matching = choices
        .into_iter()
        .filter(|(_, instance, _)| instance == id);
    let (assembly, instance, _) = matching.next()?;
    matching.next().is_none().then_some((assembly, instance))
}
fn instance_scope(w: &Workspace) -> bool {
    w.fields.get(TARGET_SCOPE).map(String::as_str) == Some("instance")
        && target_instance(w).is_some()
}
fn face_scope(w: &Workspace) -> bool {
    w.fields.get(TARGET_SCOPE).map(String::as_str) == Some("face") && target_face(w).is_some()
}
fn target_face(w: &Workspace) -> Option<cad_core::appearance::FaceFinish> {
    let face: cad_core::appearance::FaceFinish =
        serde_json::from_str(w.fields.get(TARGET_FACE)?).ok()?;
    (target_part(w).as_ref() == Some(&face.body.definition)).then_some(face)
}
fn face_assignment(w: &Workspace) -> Option<cad_core::appearance::FaceFinish> {
    let face = target_face(w)?;
    let part = w.session.visible().parts.get(&face.body.definition)?;
    let mut canonical = cad_core::appearance::FaceFinishIdentityCache::default();
    let target = canonical.key(&face.face).ok()?;
    if let Some(assignment) = part.face_finishes.iter().find(|assignment| {
        assignment.body == face.body
            && canonical.key(&assignment.face).ok().as_ref() == Some(&target)
    }) {
        return Some(assignment.clone());
    }
    let evaluation = crate::viewport::display_evaluation(w)?;
    let placed = crate::viewport::display_instances(w, evaluation)
        .into_iter()
        .find(|placed| placed.definition == part.id)?;
    let meshes = crate::viewport::display_meshes(w, evaluation, placed);
    let resolved = cad_core::appearance::resolve_face_finishes(part, &meshes);
    let assignment = resolved.resolved.iter().find(|assignment| {
        assignment.body == face.body
            && canonical.key(&assignment.face).ok().as_ref() == Some(&target)
    })?;
    part.face_finishes
        .iter()
        .find(|source| source.id == assignment.assignment)
        .cloned()
}
fn capture_face(w: &mut Workspace) -> Result<()> {
    let selected = w
        .selected
        .as_ref()
        .filter(|selection| selection.reference.topology == "face")
        .ok_or_else(|| {
            CadError::Invalid("Select a face on the model before choosing face appearance".into())
        })?;
    let body = selected.body.clone().ok_or_else(|| {
        CadError::Invalid("The selected face has no native output body reference".into())
    })?;
    let evaluation = crate::viewport::display_evaluation(w)
        .filter(|evaluation| {
            (evaluation.valid || evaluation.geometry_valid)
                && evaluation.identity == w.session.identity()
        })
        .ok_or_else(|| {
            CadError::Invalid(
                "Wait for the current model evaluation before assigning a face finish".into(),
            )
        })?;
    let placed = crate::viewport::display_instances(w, evaluation)
        .into_iter()
        .find(|placed| placed.path == selected.path && placed.definition == selected.definition)
        .ok_or_else(|| {
            CadError::Invalid("The selected component occurrence no longer exists".into())
        })?;
    let meshes = crate::viewport::display_meshes(w, evaluation, placed);
    // Capturing a face is authoritative: malformed or exhausted provenance
    // must produce its explicit error, not silently masquerade as a missing
    // selection. The same canonical source identity accepts saved tree roles
    // and current closure roles without looking at numerical signatures.
    let mut canonical = cad_core::appearance::FaceFinishIdentityCache::default();
    let selected_key = canonical.key(&selected.reference)?;
    let mut matches = 0usize;
    for face in meshes.iter().flat_map(|mesh| &mesh.faces) {
        if face.body.as_ref() == Some(&body) && canonical.key(&face.reference)? == selected_key {
            matches += 1;
        }
    }
    if matches != 1 {
        return Err(CadError::Invalid(
            "Face appearance needs one unambiguous native face in the current model".into(),
        ));
    }
    let face = cad_core::appearance::FaceFinish {
        id: cad_core::id(),
        body,
        face: selected.reference.clone(),
        finish: w
            .session
            .visible()
            .parts
            .get(&selected.definition)
            .ok_or_else(|| CadError::Invalid("The selected part is missing".into()))?
            .finish
            .clone(),
    };
    w.fields
        .insert(TARGET_PART.into(), selected.definition.clone());
    w.fields
        .insert(TARGET_FACE.into(), serde_json::to_string(&face)?);
    w.fields.insert(TARGET_SCOPE.into(), "face".into());
    Ok(())
}
fn scope(w: &Workspace) -> &'static str {
    if face_scope(w) {
        "face"
    } else if instance_scope(w) {
        "instance"
    } else {
        "part"
    }
}
fn assigned_finish(w: &Workspace) -> Option<Id> {
    let document = w.session.visible();
    if face_scope(w)
        && let Some(assignment) = face_assignment(w)
    {
        return Some(assignment.finish);
    }
    if instance_scope(w)
        && let Some((assembly, instance)) = target_instance(w)
    {
        if let Ok(placed) = document.placed_parts()
            && let Some(part) = placed.iter().find(|placed| {
                target_part(w).as_ref() == Some(&placed.definition)
                    && placed.path.last() == Some(&instance)
            })
            && let Ok(finish) = cad_core::appearance::resolve_finish(document, part)
        {
            return Some(finish.id.clone());
        }
        if let Some(finish) = document
            .assemblies
            .get(&assembly)
            .and_then(|assembly| assembly.instances.get(&instance))
            .and_then(|instance| instance.finish.clone())
        {
            return Some(finish);
        }
    }
    target_part(w)
        .and_then(|id| document.parts.get(&id))
        .map(|part| part.finish.clone())
        .filter(|id| document.finishes.contains_key(id))
}
fn target_key(w: &Workspace) -> Option<String> {
    if face_scope(w) {
        let face = target_face(w)?;
        Some(format!("face:{}:{}", face.body.definition, face.id))
    } else if instance_scope(w) {
        let (assembly, instance) = target_instance(w)?;
        Some(format!("instance:{assembly}:{instance}"))
    } else {
        target_part(w).map(|part| format!("part:{part}"))
    }
}
fn custom_is_current(w: &Workspace) -> bool {
    w.fields.get(EDIT_MODE).map(String::as_str) == Some("custom")
        && w.fields.get(CUSTOM_TARGET) == target_key(w).as_ref()
        && w.fields.get(CUSTOM_FINISH) == selected(w).as_ref()
        && selected(w) == assigned_finish(w)
}
fn reset_target_editor(w: &mut Workspace) {
    w.fields.insert(EDIT_MODE.into(), "assignment".into());
    w.fields.remove(CUSTOM_TARGET);
    w.fields.remove(CUSTOM_FINISH);
    if let Some(id) = initial(w) {
        load(w, &id);
    }
}
fn load(w: &mut Workspace, id: &str) {
    w.fields.insert(EDITOR.into(), id.into());
    w.fields.insert(
        "appearance-catalog-selected".into(),
        format!("project:{id}"),
    );
    w.fields.remove(INVALID);
    w.fields.remove(SOURCE);
    w.fields.retain(|key, _| {
        !key.starts_with("presentation-field:finish:")
            && !key.starts_with("appearance-field-invalid:finish:")
            && !matches!(
                key.as_str(),
                "appearance-field-invalid:appearance:source"
                    | "presentation-field:appearance:source"
            )
    });
}
fn available(w: &Workspace) -> bool {
    w.sketch.is_none()
        && (w.tool_checkpoint.is_none()
            || w.fields.contains_key(TOOL)
            || w.fields.contains_key("material-tool"))
}
fn finish_editable(w: &Workspace, id: &str) -> bool {
    available(w) && !cad_core::library::finish_is_pinned(w.session.visible(), id)
}
fn bound_field(element: &SemanticElement) -> bool {
    matches!(
        element.data["setter"].as_str(),
        Some("appearance-finish" | "appearance-lighting")
    ) || element.data["setter"] == "appearance-ui" && element.data["field"] == "source"
}
fn field_revision(w: &Workspace) -> Value {
    json!([
        w.session.generation,
        w.session.committed.revision,
        w.session
            .draft
            .as_ref()
            .map(|draft| (&draft.id, draft.edit))
    ])
}
fn field_source(w: &Workspace, element: &SemanticElement) -> Option<Value> {
    if element.data["setter"] == "appearance-lighting" {
        serde_json::to_value(&w.session.visible().lighting).ok()
    } else {
        let id = element.data["bound_finish"].as_str()?;
        serde_json::to_value(w.session.visible().finishes.get(id)?).ok()
    }
}
fn field_digest(value: &Value) -> Option<String> {
    serde_json::to_vec(value)
        .ok()
        .map(|bytes| format!("{:x}", Sha256::digest(bytes)))
}
fn field_identity_current(w: &Workspace, element: &SemanticElement) -> bool {
    element.data["document"] == json!(w.session.visible().id)
        && element.data["generation"] == json!(w.session.generation)
        && (element.data["setter"] != "appearance-ui"
            || element.data["bound_finish"] == json!(selected(w)))
}
fn protect_finish_field(w: &Workspace, element: &mut SemanticElement) {
    let setter = element.data.get("setter").and_then(Value::as_str);
    let finish = if setter == Some("appearance-finish") {
        element
            .data
            .get("finish")
            .and_then(Value::as_str)
            .map(str::to_owned)
    } else if setter == Some("appearance-ui")
        && element.data.get("field").and_then(Value::as_str) == Some("source")
    {
        selected(w)
    } else {
        None
    };
    if bound_field(element) {
        element.data["document"] = json!(w.session.visible().id);
        element.data["generation"] = json!(w.session.generation);
        element.data["source_revision"] = field_revision(w);
        element.data["bound_finish"] = json!(finish);
        element.data["source_sha256"] =
            json!(field_source(w, element).as_ref().and_then(field_digest));
        element.data["editor"] = json!(format!(
            "{}:{}",
            w.session.visible().id,
            finish.as_deref().unwrap_or("lighting")
        ));
    }
    if finish
        .as_ref()
        .is_some_and(|id| cad_core::library::finish_is_pinned(w.session.visible(), id))
    {
        element.data["editable"] = false.into();
        element.data["disabled_reason"] = json!(
            "This finish is pinned by a linked component. Duplicate it to customize a unique component."
        );
        element.data["diagnostics"] = json!([
            "This finish is pinned by a linked component. Duplicate it to customize a unique component."
        ]);
        element.state = "readonly".into();
    }
}
pub fn begin(w: &mut Workspace, lighting: bool) {
    if w.fields.contains_key("material-tool") && !crate::materials_ui::current_source_is_valid(w) {
        w.status = "Correct the current material fields before switching to appearances".into();
        return;
    }
    if !available(w) {
        w.status = "Apply or cancel the current operation before editing visual finishes".into();
        return;
    }
    if !lighting
        && !w.fields.contains_key(TOOL)
        && w.selected
            .as_ref()
            .is_some_and(|selection| selection.reference.topology == "face")
        && let Err(error) = capture_face(w)
    {
        w.status = error.to_string();
        return;
    }
    w.start_tool();
    w.fields.insert(TOOL.into(), "true".into());
    if !w.fields.contains_key(TARGET_PART) {
        if let Some(id) = target_part(w) {
            w.fields.insert(TARGET_PART.into(), id);
        }
        if let Some((assembly, instance)) = target_instance(w) {
            w.fields
                .insert(TARGET_INSTANCE.into(), instance_key(&assembly, &instance));
            w.fields.insert(TARGET_SCOPE.into(), "instance".into());
        } else {
            w.fields.insert(TARGET_SCOPE.into(), "part".into());
        }
        w.fields.insert(EDIT_MODE.into(), "assignment".into());
    }
    if !w.fields.contains_key(EDITOR)
        && let Some(id) = initial(w)
    {
        load(w, &id);
    }
    w.fields.insert(
        "appearance-tab".into(),
        if lighting { "lighting" } else { "finish" }.into(),
    );
    w.open_editor("appearances");
}
pub fn current_source_is_valid(w: &Workspace) -> bool {
    source_text_is_valid(w, None)
        && (!w.fields.contains_key(TOOL) || crate::render_appearance::resources_are_current(w))
        && w.appearance_diagnostics.is_empty()
}
fn source_text_is_valid(w: &Workspace, ignored: Option<&str>) -> bool {
    !w.fields.contains_key(INVALID)
        && !w.fields.keys().any(|key| {
            key.starts_with("appearance-field-invalid:")
                && ignored.is_none_or(|id| key != &format!("appearance-field-invalid:{id}"))
        })
        && w.fields.get(SOURCE).is_none_or(|source| {
            serde_json::from_str::<Finish>(source).is_ok_and(|finish| {
                selected(w).as_ref() == Some(&finish.id)
                    && w.session.visible().finishes.get(&finish.id) == Some(&finish)
            })
        })
}
pub fn selection_ready(w: &Workspace) -> Result<()> {
    if source_text_is_valid(w, None) {
        Ok(())
    } else {
        Err(CadError::Invalid("Correct retained finish/lighting text or explicitly Refresh finish source before changing the selected finish or assignment; your text is preserved".into()))
    }
}
fn selection_changing(command: &str) -> bool {
    matches!(
        command,
        "appearance.edit"
            | "appearance.customize-target"
            | "appearance.use-assigned"
            | "appearance.assign-target"
            | "appearance.new"
            | "appearance.duplicate"
            | "appearance.delete"
            | "appearance.assign-part"
            | "appearance.assign-instance"
            | "appearance.inherit-instance"
            | "appearance.assign-face"
            | "appearance.repair-face"
            | "appearance.remove-face-override"
            | "appearance.capture-face"
            | "appearance.inherit-face"
            | "appearance.clear-manufacturing"
    )
}
pub fn field_error(w: &mut Workspace, element: &SemanticElement, error: Option<String>) {
    if !element
        .data
        .get("setter")
        .and_then(Value::as_str)
        .is_some_and(|setter| setter.starts_with("appearance-"))
    {
        return;
    }
    if bound_field(element)
        && (!field_identity_current(w, element)
            || error.is_some() && element.data["source_revision"] != field_revision(w))
    {
        return;
    }
    let key = format!("appearance-field-invalid:{}", element.id);
    if let Some(error) = error {
        w.fields.insert(key, error);
    } else {
        w.fields.remove(&key);
    }
}
pub fn field_is_invalid(w: &Workspace, element: &SemanticElement) -> bool {
    element.data["setter"]
        .as_str()
        .is_some_and(|setter| setter.starts_with("appearance-"))
        && w.fields
            .contains_key(&format!("appearance-field-invalid:{}", element.id))
}
fn checked_dispatch(
    w: &mut Workspace,
    command: Command,
    expected: Value,
    finish: Option<&str>,
) -> Result<()> {
    w.dispatch(command);
    let actual = finish
        .map(|id| serde_json::to_value(w.session.visible().finishes.get(id)))
        .unwrap_or_else(|| serde_json::to_value(&w.session.visible().lighting))?;
    if actual != expected {
        return Err(CadError::Invalid(w.status.clone()));
    }
    w.rendered_key.clear();
    Ok(())
}
fn put(w: &mut Workspace, finish: Finish) -> Result<()> {
    if cad_core::library::finish_is_pinned(w.session.visible(), &finish.id)
        && w.session.visible().finishes.get(&finish.id) != Some(&finish)
    {
        return Err(CadError::Invalid("This finish is pinned by a linked component. Duplicate it to customize a unique component.".into()));
    }
    let id = finish.id.clone();
    let source_was_current = w.fields.get(SOURCE).is_none_or(|text| {
        serde_json::from_str::<Finish>(text).is_ok_and(|source| {
            source == finish || w.session.visible().finishes.get(&id) == Some(&source)
        })
    });
    let expected = serde_json::to_value(&finish)?;
    checked_dispatch(
        w,
        Command::PutFinish {
            finish: Box::new(finish),
        },
        expected,
        Some(&id),
    )?;
    if source_was_current {
        w.fields.remove(INVALID);
    }
    if source_was_current && w.fields.contains_key(SOURCE) {
        w.fields.insert(
            SOURCE.into(),
            serde_json::to_string_pretty(&w.session.visible().finishes[&id])?,
        );
    }
    Ok(())
}
fn can_delete(w: &Workspace, id: &str) -> bool {
    cad_core::appearance::delete_reason(w.session.visible(), id).is_none()
}
fn choose_id(w: &Workspace, arguments: Option<&Value>) -> Option<Id> {
    arguments
        .and_then(|arguments| arguments.get("finish"))
        .and_then(Value::as_str)
        .map(str::to_owned)
        .or_else(|| selected(w))
}
fn customize_target(w: &mut Workspace) -> Result<()> {
    if custom_is_current(w) {
        return Ok(());
    }
    let key = target_key(w)
        .ok_or_else(|| CadError::Invalid("Choose a part to customize its appearance".into()))?;
    let source = assigned_finish(w)
        .and_then(|id| w.session.visible().finishes.get(&id))
        .ok_or_else(|| CadError::Invalid("The assigned visual finish is missing".into()))?;
    let mut finish = source.clone();
    finish.id = cad_core::id();
    let name = if instance_scope(w) {
        target_instance(w).and_then(|(assembly, instance)| {
            w.session
                .visible()
                .assemblies
                .get(&assembly)?
                .instances
                .get(&instance)
                .map(|instance| instance.name.clone())
        })
    } else {
        target_part(w)
            .and_then(|id| w.session.visible().parts.get(&id))
            .map(|part| part.name.clone())
    }
    .unwrap_or_else(|| "Part".into());
    finish.name = format!("{name} · Custom appearance");
    while finish.name.len() > 256 {
        finish.name.pop();
    }
    let id = finish.id.clone();
    let mut document = w.session.visible().clone();
    document.finishes.insert(id.clone(), finish);
    if face_scope(w) {
        let mut face = face_assignment(w)
            .or_else(|| target_face(w))
            .ok_or_else(|| CadError::Invalid("Choose a native face".into()))?;
        face.finish = id.clone();
        let definition = face.body.definition.clone();
        cad_core::appearance::set_face_finish(&mut document, &definition, face)?;
    } else if instance_scope(w) {
        let (assembly, instance) = target_instance(w)
            .ok_or_else(|| CadError::Invalid("Choose a component instance".into()))?;
        document
            .assemblies
            .get_mut(&assembly)
            .and_then(|assembly| assembly.instances.get_mut(&instance))
            .ok_or_else(|| CadError::Invalid("The component instance no longer exists".into()))?
            .finish = Some(id.clone());
    } else {
        let part =
            target_part(w).ok_or_else(|| CadError::Invalid("Choose a part to customize".into()))?;
        document
            .parts
            .get_mut(&part)
            .ok_or_else(|| CadError::Invalid("The part no longer exists".into()))?
            .finish = id.clone();
    }
    document.validate()?;
    let expected = document.fingerprint();
    w.dispatch(Command::ReplaceDocument { document });
    if w.session.visible().fingerprint() != expected {
        return Err(CadError::Invalid(w.status.clone()));
    }
    load(w, &id);
    w.fields.insert(EDIT_MODE.into(), "custom".into());
    w.fields.insert(CUSTOM_TARGET.into(), key);
    w.fields.insert(CUSTOM_FINISH.into(), id);
    Ok(())
}
pub fn action(w: &mut Workspace, command: &str, arguments: Option<&Value>) -> bool {
    if matches!(command, "appearance" | "appearances" | "lighting") {
        begin(w, command == "lighting");
        return true;
    }
    if !command.starts_with("appearance.") {
        return false;
    }
    if command == "appearance.refresh-source" {
        w.fields.remove(SOURCE);
        w.fields.remove(INVALID);
        w.fields
            .remove("appearance-field-invalid:appearance:source");
        w.fields
            .retain(|key, _| !key.starts_with("presentation-field:appearance:source:"));
        w.status = "Finish source refreshed from the current draft".into();
        return true;
    }
    if let Some(section) = command.strip_prefix(SECTION_TOGGLE) {
        if SECTIONS.iter().any(|(id, _, _)| *id == section) {
            let open = section_open(w, section);
            w.fields
                .insert(format!("appearance-section:{section}"), (!open).to_string());
        }
        return true;
    }
    if !available(w) {
        w.status = "Apply or cancel the current operation before editing visual finishes".into();
        return true;
    }
    if matches!(command, "appearance.tab.finish" | "appearance.tab.lighting") {
        w.fields.insert(
            "appearance-tab".into(),
            if command.ends_with("lighting") {
                "lighting"
            } else {
                "finish"
            }
            .into(),
        );
        return true;
    }
    if selection_changing(command)
        && let Err(error) = selection_ready(w)
    {
        w.status = error.to_string();
        return true;
    }
    if !w.fields.contains_key(TOOL) {
        begin(w, command.starts_with("appearance.environment."));
        if !w.fields.contains_key(TOOL) {
            return true;
        }
    }
    let finish = choose_id(w, arguments);
    let result = (|| -> Result<()> {
        match command {
            "appearance.environment.import" => edit_environment(w, true)?,
            "appearance.environment.clear" => edit_environment(w, false)?,
            "appearance.open-materials" => {
                if !current_source_is_valid(w) {
                    return Err(CadError::Invalid("Correct the current appearance fields and finish loading texture resources before switching to materials".into()));
                }
                crate::materials_ui::begin(w);
            }
            "appearance.edit" => {
                if let Some(id) = &finish {
                    load(w, id);
                }
                w.fields.insert(EDIT_MODE.into(), "shared".into());
                w.open_editor("appearances");
            }
            "appearance.customize-target" => customize_target(w)?,
            "appearance.use-assigned" => reset_target_editor(w),
            "appearance.assign-target" => {
                let finish = finish
                    .ok_or_else(|| CadError::Invalid("Choose a named finish to assign".into()))?;
                if face_scope(w) {
                    let mut assignment = face_assignment(w)
                        .or_else(|| target_face(w))
                        .ok_or_else(|| CadError::Invalid("Choose a native face".into()))?;
                    assignment.finish = finish.clone();
                    let definition = assignment.body.definition.clone();
                    let expected = assignment.clone();
                    w.dispatch(Command::SetFaceFinish {
                        definition: definition.clone(),
                        assignment: Box::new(assignment),
                    });
                    if w.session
                        .visible()
                        .parts
                        .get(&definition)
                        .is_none_or(|part| !part.face_finishes.contains(&expected))
                    {
                        return Err(CadError::Invalid(w.status.clone()));
                    }
                } else if instance_scope(w) {
                    let (assembly, instance) = target_instance(w)
                        .ok_or_else(|| CadError::Invalid("Choose a component instance".into()))?;
                    w.dispatch(Command::SetInstanceFinish {
                        assembly: assembly.clone(),
                        instance: instance.clone(),
                        finish: Some(finish.clone()),
                    });
                    if w.session
                        .visible()
                        .assemblies
                        .get(&assembly)
                        .and_then(|assembly| assembly.instances.get(&instance))
                        .is_none_or(|instance| instance.finish.as_ref() != Some(&finish))
                    {
                        return Err(CadError::Invalid(w.status.clone()));
                    }
                } else {
                    let definition =
                        target_part(w).ok_or_else(|| CadError::Invalid("Choose a part".into()))?;
                    w.dispatch(Command::SetPartFinish {
                        definition: definition.clone(),
                        finish: finish.clone(),
                    });
                    if w.session
                        .visible()
                        .parts
                        .get(&definition)
                        .is_none_or(|part| part.finish != finish)
                    {
                        return Err(CadError::Invalid(w.status.clone()));
                    }
                }
                reset_target_editor(w);
            }
            "appearance.clear-manufacturing" => {
                let id =
                    finish.ok_or_else(|| CadError::Invalid("Choose a visual finish".into()))?;
                let mut source = w
                    .session
                    .visible()
                    .finishes
                    .get(&id)
                    .cloned()
                    .ok_or_else(|| CadError::Invalid("The finish no longer exists".into()))?;
                source.manufacturing = None;
                put(w, source)?;
            }
            "appearance.new" | "appearance.duplicate" => {
                let source = finish
                    .as_ref()
                    .and_then(|id| w.session.visible().finishes.get(id))
                    .or_else(|| w.session.visible().finishes.values().next())
                    .ok_or_else(|| {
                        CadError::Invalid(
                            "Create a visual finish in the source before duplicating it".into(),
                        )
                    })?;
                let mut new = source.clone();
                new.id = cad_core::id();
                new.name = if command == "appearance.duplicate" {
                    format!("{} copy", source.name)
                } else {
                    "Custom finish".into()
                };
                while new.name.len() > 256 {
                    new.name.pop();
                }
                if command == "appearance.new" {
                    new.textures = Default::default();
                }
                let id = new.id.clone();
                put(w, new)?;
                load(w, &id);
                w.fields.insert(EDIT_MODE.into(), "shared".into());
            }
            "appearance.delete" => {
                let id = finish
                    .as_ref()
                    .ok_or_else(|| CadError::Invalid("Choose a finish to delete".into()))?;
                if !can_delete(w, id) {
                    return Err(CadError::Invalid("Reassign parts, occurrences and configurations using this finish before deleting it".into()));
                }
                w.dispatch(Command::DeleteFinish { finish: id.clone() });
                if w.session.visible().finishes.contains_key(id) {
                    return Err(CadError::Invalid(w.status.clone()));
                }
                if let Some(id) = initial(w) {
                    load(w, &id);
                }
            }
            "appearance.assign-part" => {
                let definition = arguments
                    .and_then(|v| v.get("definition"))
                    .and_then(Value::as_str)
                    .map(str::to_owned)
                    .or_else(|| target_part(w))
                    .ok_or_else(|| {
                        CadError::Invalid("Select a part to assign its visual finish".into())
                    })?;
                let finish = finish.ok_or_else(|| CadError::Invalid("Choose a finish".into()))?;
                w.dispatch(Command::SetPartFinish {
                    definition: definition.clone(),
                    finish: finish.clone(),
                });
                if w.session
                    .visible()
                    .parts
                    .get(&definition)
                    .is_none_or(|part| part.finish != finish)
                {
                    return Err(CadError::Invalid(w.status.clone()));
                }
            }
            "appearance.assign-instance" | "appearance.inherit-instance" => {
                let target = arguments
                    .and_then(|v| {
                        Some((
                            v.get("assembly")?.as_str()?.into(),
                            v.get("instance")?.as_str()?.into(),
                        ))
                    })
                    .or_else(|| target_instance(w))
                    .ok_or_else(|| {
                        CadError::Invalid(
                            "Select one component occurrence to override its visual finish".into(),
                        )
                    })?;
                w.dispatch(Command::SetInstanceFinish {
                    assembly: target.0.clone(),
                    instance: target.1.clone(),
                    finish: if command == "appearance.inherit-instance" {
                        None
                    } else {
                        finish.clone()
                    },
                });
                let expected = if command == "appearance.inherit-instance" {
                    None
                } else {
                    finish
                };
                if w.session
                    .visible()
                    .assemblies
                    .get(&target.0)
                    .and_then(|assembly| assembly.instances.get(&target.1))
                    .is_none_or(|instance| instance.finish != expected)
                {
                    return Err(CadError::Invalid(w.status.clone()));
                }
                if command == "appearance.inherit-instance" {
                    reset_target_editor(w);
                }
            }
            "appearance.assign-face" => {
                capture_face(w)?;
                action(w, "appearance.assign-target", arguments);
            }
            "appearance.repair-face" => {
                let previous = arguments
                    .and_then(|arguments| arguments.get("assignment"))
                    .and_then(|value| {
                        serde_json::from_value::<cad_core::appearance::FaceFinish>(value.clone())
                            .ok()
                    })
                    .ok_or_else(|| {
                        CadError::Invalid("Choose the face finish assignment to reattach".into())
                    })?;
                capture_face(w)?;
                let mut replacement = target_face(w).ok_or_else(|| {
                    CadError::Invalid("Select the replacement native face".into())
                })?;
                if replacement.body.definition != previous.body.definition {
                    return Err(CadError::Invalid(
                        "The replacement face must belong to the same part definition".into(),
                    ));
                }
                replacement.id = previous.id;
                replacement.finish = previous.finish;
                let definition = replacement.body.definition.clone();
                w.dispatch(Command::SetFaceFinish {
                    definition,
                    assignment: Box::new(replacement),
                });
                reset_target_editor(w);
            }
            "appearance.remove-face-override" => {
                let definition = arguments
                    .and_then(|arguments| arguments.get("definition"))
                    .and_then(Value::as_str)
                    .ok_or_else(|| {
                        CadError::Invalid("The face assignment part is missing".into())
                    })?;
                let assignment = arguments
                    .and_then(|arguments| arguments.get("assignment_id"))
                    .and_then(Value::as_str)
                    .ok_or_else(|| {
                        CadError::Invalid("The face assignment identity is missing".into())
                    })?;
                w.dispatch(Command::DeleteFaceFinish {
                    definition: definition.into(),
                    assignment: assignment.into(),
                });
                if w.session
                    .visible()
                    .parts
                    .get(definition)
                    .is_some_and(|part| {
                        part.face_finishes
                            .iter()
                            .any(|source| source.id == assignment)
                    })
                {
                    return Err(CadError::Invalid(w.status.clone()));
                }
                reset_target_editor(w);
            }
            "appearance.capture-face" => {
                capture_face(w)?;
                reset_target_editor(w);
            }
            "appearance.inherit-face" => {
                let assignment = face_assignment(w).ok_or_else(|| {
                    CadError::Invalid("The selected face already uses its part finish".into())
                })?;
                let definition = assignment.body.definition.clone();
                w.dispatch(Command::DeleteFaceFinish {
                    definition: definition.clone(),
                    assignment: assignment.id.clone(),
                });
                if w.session
                    .visible()
                    .parts
                    .get(&definition)
                    .is_none_or(|part| {
                        part.face_finishes
                            .iter()
                            .any(|source| source.id == assignment.id)
                    })
                {
                    return Err(CadError::Invalid(w.status.clone()));
                }
                reset_target_editor(w);
            }
            "appearance.texture.import" | "appearance.texture.clear" => {
                let id =
                    finish.ok_or_else(|| CadError::Invalid("Choose a visual finish".into()))?;
                let slot = arguments
                    .and_then(|v| v.get("slot"))
                    .and_then(Value::as_str)
                    .ok_or_else(|| CadError::Invalid("Choose the texture map to replace".into()))?;
                texture(w, &id, slot, command == "appearance.texture.import")?;
            }
            _ if command.starts_with(IMPORT) || command.starts_with(CLEAR) => {
                let payload = command
                    .strip_prefix(IMPORT)
                    .or_else(|| command.strip_prefix(CLEAR))
                    .unwrap_or_default();
                let (id, slot) = payload.rsplit_once('/').ok_or_else(|| {
                    CadError::Invalid("Texture action is missing its finish and map".into())
                })?;
                texture(w, id, slot, command.starts_with(IMPORT))?;
            }
            _ => {
                return Err(CadError::Invalid(format!(
                    "Unknown visual finish command: {command}"
                )));
            }
        }
        w.rendered_key.clear();
        Ok(())
    })();
    if let Err(error) = result {
        w.status = error.to_string();
    }
    true
}
fn texture(w: &mut Workspace, finish_id: &str, slot: &str, import: bool) -> Result<()> {
    if cad_core::library::finish_is_pinned(w.session.visible(), finish_id) {
        return Err(CadError::Invalid("This finish is pinned by a linked component. Duplicate it before importing or clearing textures.".into()));
    }
    if ![
        "base_color",
        "normal",
        "roughness",
        "metallic",
        "occlusion",
        "height",
    ]
    .contains(&slot)
    {
        return Err(CadError::Invalid("Unknown texture map".into()));
    }
    let finish = w
        .session
        .visible()
        .finishes
        .get(finish_id)
        .cloned()
        .ok_or_else(|| CadError::Invalid("The visual finish no longer exists".into()))?;
    let mut document = w.session.visible().clone();
    let resource = if import {
        let Some(path) = rfd::FileDialog::new()
            .set_title(if slot == "height" {
                "Import a grayscale bump / height map"
            } else {
                "Import a visual texture map"
            })
            .add_filter(
                "Texture images",
                &["png", "jpg", "jpeg", "tif", "tiff", "webp"],
            )
            .pick_file()
        else {
            return Ok(());
        };
        let bytes = crate::texture_assets::validate_texture_file(&path)?;
        Some(cad_core::persistence::import_resource_bytes(
            &mut document,
            &w.root(),
            &path,
            &bytes,
        )?)
    } else {
        None
    };
    let mut source = serde_json::to_value(&finish)?;
    source["textures"][slot] = json!(resource);
    let finish: Finish = serde_json::from_value(source)?;
    document.finishes.insert(finish_id.into(), finish);
    document.validate()?;
    let expected = document.fingerprint();
    w.dispatch(Command::ReplaceDocument { document });
    if w.session.visible().fingerprint() != expected {
        return Err(CadError::Invalid(w.status.clone()));
    }
    load(w, finish_id);
    Ok(())
}
fn texture_label(slot: &str) -> &str {
    match slot {
        "height" => "Grayscale bump / height",
        "base_color" => "Color",
        "normal" => "Normal",
        "roughness" => "Roughness",
        "metallic" => "Metallic",
        "occlusion" => "Occlusion",
        _ => "Texture",
    }
}

fn finish_source(finish: &Finish) -> Value {
    let mut source = serde_json::to_value(finish).unwrap_or_default();
    let metadata = finish.metadata.clone().unwrap_or_default();
    source["metadata"] = serde_json::to_value(&metadata).unwrap_or_default();
    source["metadata"]["tags"] = json!(metadata.tags);
    for slot in [
        "base_color",
        "normal",
        "roughness",
        "metallic",
        "occlusion",
        "height",
    ] {
        if source["textures"].get(slot).is_none() {
            source["textures"][slot] = Value::Null;
        }
    }
    source
}
pub fn current_value(w: &Workspace, element: &SemanticElement) -> Option<Value> {
    if bound_field(element) && !field_identity_current(w, element) {
        return None;
    }
    match element.data.get("setter")?.as_str()? {
        "appearance-ui" => match element.data.get("field")?.as_str()? {
            "library" => Some(json!(selected(w).or_else(|| initial(w)))),
            "target-part" => Some(json!(target_part(w))),
            "target-scope" => Some(json!(scope(w))),
            "target-instance" => {
                Some(json!(target_instance(w).map(|(assembly, instance)| {
                    instance_key(&assembly, &instance)
                })))
            }
            "section" => Some(json!(section_open(
                w,
                element.data.get("section")?.as_str()?
            ))),
            "source" => Some(json!(
                w.fields
                    .get(SOURCE)
                    .cloned()
                    .or_else(|| selected(w)
                        .and_then(|id| w.session.visible().finishes.get(&id))
                        .and_then(|finish| serde_json::to_string_pretty(finish).ok()))
                    .unwrap_or_default()
            )),
            _ => None,
        },
        "appearance-finish" => {
            let finish = w
                .session
                .visible()
                .finishes
                .get(element.data.get("finish")?.as_str()?)?;
            let field = element.data.get("field")?.as_str()?;
            if let Some(field) = field.strip_prefix("/manufacturing/") {
                let source = finish.manufacturing.clone().unwrap_or_default();
                Some(match field {
                    "process" => json!(source.process),
                    "specification" => json!(source.specification),
                    "designation" => json!(source.designation),
                    "notes" => json!(source.notes),
                    "coating_thickness_um" => json!(
                        source
                            .coating_thickness_um
                            .map(|value| value.to_string())
                            .unwrap_or_default()
                    ),
                    "surface_roughness_ra_um" => json!(
                        source
                            .surface_roughness_ra_um
                            .map(|value| value.to_string())
                            .unwrap_or_default()
                    ),
                    _ => return None,
                })
            } else if field == "/metadata/tags" {
                Some(json!(
                    finish
                        .metadata
                        .as_ref()
                        .map(|metadata| metadata.tags.join(", "))
                        .unwrap_or_default()
                ))
            } else {
                finish_source(finish).pointer(field).cloned()
            }
        }
        "appearance-lighting" => {
            let field = element.data.get("field")?.as_str()?;
            if field == "/environment" {
                Some(json!(w.session.visible().lighting.environment))
            } else {
                serde_json::to_value(&w.session.visible().lighting)
                    .ok()?
                    .pointer(field)
                    .cloned()
            }
        }
        "appearance-part" => Some(json!(
            w.session
                .visible()
                .parts
                .get(element.data.get("definition")?.as_str()?)?
                .finish
        )),
        "appearance-instance" => Some(json!(
            w.session
                .visible()
                .assemblies
                .get(element.data.get("assembly")?.as_str()?)?
                .instances
                .values()
                .find(|instance| Some(instance.id.as_str())
                    == element.data.get("instance").and_then(Value::as_str))?
                .finish
        )),
        _ => None,
    }
}
pub fn set_value(w: &mut Workspace, element: &SemanticElement, value: Value) -> Result<()> {
    if bound_field(element)
        && (!field_identity_current(w, element)
            || element.data["source_revision"] != field_revision(w)
            || field_source(w, element)
                .as_ref()
                .and_then(field_digest)
                .is_none_or(|digest| element.data["source_sha256"] != digest))
    {
        return Err(CadError::Invalid("This finish/lighting field belongs to an earlier document or source; use the current editor control".into()));
    }
    if !available(w) {
        return Err(CadError::Invalid(
            "Apply or cancel the current operation before editing visual finishes".into(),
        ));
    }
    let setter = element
        .data
        .get("setter")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let edited_finish = if setter == "appearance-finish" {
        element
            .data
            .get("finish")
            .and_then(Value::as_str)
            .map(str::to_owned)
    } else if setter == "appearance-ui"
        && element.data.get("field").and_then(Value::as_str) == Some("source")
    {
        selected(w)
    } else {
        None
    };
    if edited_finish.is_some_and(|id| cad_core::library::finish_is_pinned(w.session.visible(), &id))
    {
        return Err(CadError::Invalid("This finish is pinned by a linked component. Duplicate it to customize a unique component.".into()));
    }
    if setter == "appearance-ui" {
        if element
            .data
            .get("field")
            .and_then(Value::as_str)
            .is_some_and(|field| {
                matches!(
                    field,
                    "library" | "target-part" | "target-scope" | "target-instance"
                )
            })
            && !source_text_is_valid(w, Some(&element.id))
        {
            return Err(CadError::Invalid("Correct retained finish/lighting text before changing selection or assignment; your text is preserved".into()));
        }
        match element.data.get("field").and_then(Value::as_str) {
            Some("library") => {
                let id = value
                    .as_str()
                    .filter(|id| w.session.visible().finishes.contains_key(*id))
                    .ok_or_else(|| {
                        CadError::Invalid("Choose a visual finish from the library".into())
                    })?;
                if selected(w).as_deref() != Some(id) {
                    load(w, id);
                    w.fields.insert(EDIT_MODE.into(), "assignment".into());
                }
                return Ok(());
            }
            Some("target-part") => {
                let id = value
                    .as_str()
                    .filter(|id| w.session.visible().parts.contains_key(*id))
                    .ok_or_else(|| CadError::Invalid("Choose a part".into()))?;
                if target_part(w).as_deref() == Some(id) {
                    return Ok(());
                }
                w.fields.insert(TARGET_PART.into(), id.into());
                w.fields.remove(TARGET_INSTANCE);
                w.fields.remove(TARGET_FACE);
                w.fields.insert(TARGET_SCOPE.into(), "part".into());
                reset_target_editor(w);
                return Ok(());
            }
            Some("target-scope") => {
                let scope = value
                    .as_str()
                    .filter(|scope| matches!(*scope, "part" | "instance" | "face"))
                    .ok_or_else(|| {
                        CadError::Invalid(
                            "Choose part, component instance or selected face scope".into(),
                        )
                    })?;
                if crate::appearances_ui::scope(w) == scope {
                    return Ok(());
                }
                if scope == "face" && target_face(w).is_none() {
                    capture_face(w)?;
                }
                if scope == "instance" && target_instance(w).is_none() {
                    let (assembly, instance, _) =
                        instance_choices(w).into_iter().next().ok_or_else(|| {
                            CadError::Invalid("This part has no component instances".into())
                        })?;
                    w.fields
                        .insert(TARGET_INSTANCE.into(), instance_key(&assembly, &instance));
                }
                w.fields.insert(TARGET_SCOPE.into(), scope.into());
                reset_target_editor(w);
                return Ok(());
            }
            Some("target-instance") => {
                let key = value
                    .as_str()
                    .filter(|key| {
                        instance_choices(w)
                            .iter()
                            .any(|(assembly, instance, _)| instance_key(assembly, instance) == *key)
                    })
                    .ok_or_else(|| {
                        CadError::Invalid("Choose one of this part's component instances".into())
                    })?;
                if target_instance(w)
                    .as_ref()
                    .is_some_and(|(assembly, instance)| instance_key(assembly, instance) == key)
                {
                    return Ok(());
                }
                w.fields.insert(TARGET_INSTANCE.into(), key.into());
                w.fields.insert(TARGET_SCOPE.into(), "instance".into());
                reset_target_editor(w);
                return Ok(());
            }
            Some("section") => {
                let section = element
                    .data
                    .get("section")
                    .and_then(Value::as_str)
                    .filter(|section| SECTIONS.iter().any(|(id, _, _)| id == section))
                    .ok_or_else(|| CadError::Invalid("Unknown appearance section".into()))?;
                let open = value.as_bool().ok_or_else(|| {
                    CadError::Invalid("Section visibility must be true or false".into())
                })?;
                w.fields
                    .insert(format!("appearance-section:{section}"), open.to_string());
                return Ok(());
            }
            _ => {}
        }
    }
    if !w.fields.contains_key(TOOL) {
        begin(w, false);
        if !w.fields.contains_key(TOOL) {
            return Err(CadError::Invalid(w.status.clone()));
        }
    }
    match setter {
        "appearance-ui" => match element.data.get("field").and_then(Value::as_str) {
            Some("library") => {
                let id = value
                    .as_str()
                    .filter(|id| w.session.visible().finishes.contains_key(*id))
                    .ok_or_else(|| {
                        CadError::Invalid("Choose a visual finish from the library".into())
                    })?;
                load(w, id);
            }
            Some("source") => {
                let source = value
                    .as_str()
                    .ok_or_else(|| CadError::Invalid("Finish source must be JSON text".into()))?;
                w.fields.insert(SOURCE.into(), source.into());
                let result = (|| -> Result<()> {
                    let finish: Finish = serde_json::from_str(source)?;
                    if selected(w).as_ref() != Some(&finish.id) {
                        return Err(CadError::Invalid(
                            "The finish identity cannot change in its editor".into(),
                        ));
                    }
                    put(w, finish)
                })();
                if let Err(error) = result {
                    w.fields.insert(INVALID.into(), error.to_string());
                    return Err(error);
                }
            }
            _ => {
                return Err(CadError::Invalid(
                    "Unknown visual finish editor field".into(),
                ));
            }
        },
        "appearance-finish" => {
            let id = element
                .data
                .get("finish")
                .and_then(Value::as_str)
                .ok_or_else(|| CadError::Invalid("Missing finish identity".into()))?;
            if let Some(field) = element
                .data
                .get("field")
                .and_then(Value::as_str)
                .and_then(|pointer| pointer.strip_prefix("/manufacturing/"))
            {
                let mut finish = w
                    .session
                    .visible()
                    .finishes
                    .get(id)
                    .cloned()
                    .ok_or_else(|| CadError::Invalid("The finish no longer exists".into()))?;
                let mut requirements = finish.manufacturing.clone().unwrap_or_default();
                let text = value.as_str().ok_or_else(|| {
                    CadError::Invalid("Enter a manufacturing finish value".into())
                })?;
                match field {
                    "process" => requirements.process = text.into(),
                    "specification" => requirements.specification = text.into(),
                    "designation" => requirements.designation = text.into(),
                    "notes" => requirements.notes = text.into(),
                    "coating_thickness_um" | "surface_roughness_ra_um" => {
                        let number = if text.trim().is_empty() {
                            None
                        } else {
                            Some(text.trim().parse::<f64>().map_err(|_|CadError::Invalid("Enter a finite micrometre value, or clear the field to leave it unspecified".into()))?)
                        };
                        if field == "coating_thickness_um" {
                            requirements.coating_thickness_um = number;
                        } else {
                            requirements.surface_roughness_ra_um = number;
                        }
                    }
                    _ => {
                        return Err(CadError::Invalid(
                            "Unknown manufacturing finish requirement".into(),
                        ));
                    }
                }
                requirements.validate()?;
                finish.manufacturing = if requirements == Default::default() {
                    None
                } else {
                    Some(requirements)
                };
                put(w, finish)?;
                return Ok(());
            }
            let mut source =
                finish_source(w.session.visible().finishes.get(id).ok_or_else(|| {
                    CadError::Invalid("The visual finish no longer exists".into())
                })?);
            let pointer = element
                .data
                .get("field")
                .and_then(Value::as_str)
                .ok_or_else(|| CadError::Invalid("Missing finish field".into()))?;
            let value = if pointer == "/metadata/tags" {
                json!(
                    value
                        .as_str()
                        .unwrap_or_default()
                        .split(',')
                        .map(str::trim)
                        .filter(|tag| !tag.is_empty())
                        .map(str::to_owned)
                        .collect::<Vec<_>>()
                )
            } else {
                value
            };
            *source
                .pointer_mut(pointer)
                .ok_or_else(|| CadError::Invalid("The finish field no longer exists".into()))? =
                value;
            put(w, serde_json::from_value(source)?)?;
        }
        "appearance-lighting" => {
            let mut source = serde_json::to_value(&w.session.visible().lighting)?;
            source["environment"] = json!(w.session.visible().lighting.environment);
            let pointer = element
                .data
                .get("field")
                .and_then(Value::as_str)
                .ok_or_else(|| CadError::Invalid("Missing lighting field".into()))?;
            *source
                .pointer_mut(pointer)
                .ok_or_else(|| CadError::Invalid("The lighting field no longer exists".into()))? =
                value;
            let lighting: Lighting = serde_json::from_value(source.clone())?;
            checked_dispatch(w, Command::SetLighting { lighting }, source, None)?;
        }
        "appearance-part" => {
            let definition = element
                .data
                .get("definition")
                .and_then(Value::as_str)
                .ok_or_else(|| CadError::Invalid("Missing part identity".into()))?;
            let finish = value
                .as_str()
                .ok_or_else(|| CadError::Invalid("Choose a named visual finish".into()))?;
            w.dispatch(Command::SetPartFinish {
                definition: definition.into(),
                finish: finish.into(),
            });
            if w.session
                .visible()
                .parts
                .get(definition)
                .is_none_or(|part| part.finish != finish)
            {
                return Err(CadError::Invalid(w.status.clone()));
            }
        }
        "appearance-instance" => {
            let assembly = element
                .data
                .get("assembly")
                .and_then(Value::as_str)
                .ok_or_else(|| CadError::Invalid("Missing assembly identity".into()))?;
            let instance = element
                .data
                .get("instance")
                .and_then(Value::as_str)
                .ok_or_else(|| CadError::Invalid("Missing component identity".into()))?;
            let finish: Option<Id> = serde_json::from_value(value)?;
            w.dispatch(Command::SetInstanceFinish {
                assembly: assembly.into(),
                instance: instance.into(),
                finish: finish.clone(),
            });
            if w.session
                .visible()
                .assemblies
                .get(assembly)
                .and_then(|assembly| {
                    assembly
                        .instances
                        .values()
                        .find(|source| source.id == instance)
                })
                .is_none_or(|instance| instance.finish != finish)
            {
                return Err(CadError::Invalid(w.status.clone()));
            }
        }
        _ => {
            return Err(CadError::Invalid(
                "No visual finish setter for this field".into(),
            ));
        }
    }
    w.rendered_key.clear();
    Ok(())
}

fn button(ui: &mut egui::Ui, w: &mut Workspace, command: &str, label: &str, enabled: bool) {
    let modifies_finish = command.starts_with(IMPORT)
        || command.starts_with(CLEAR)
        || command == "appearance.clear-manufacturing";
    let enabled = enabled
        && (!modifies_finish || selected(w).is_some_and(|id| finish_editable(w, &id)))
        && (!selection_changing(command) || source_text_is_valid(w, None));
    if controls::button(
        ui,
        &format!("command:{command}"),
        "appearance-action",
        &["semantic-control"],
        label,
        enabled,
        false,
    )
    .clicked()
    {
        action(w, command, None);
    }
}
fn field_element(
    id: &str,
    field: &str,
    label: &str,
    value: Value,
    lighting: bool,
    choices: Value,
    unit: Option<&str>,
) -> SemanticElement {
    SemanticElement {
        id: if lighting {
            format!("viewport-lighting::{field}")
        } else {
            format!("finish:{id}::{field}")
        },
        role: if lighting {
            "lighting-field"
        } else {
            "appearance-field"
        }
        .into(),
        label: label.into(),
        value: Some(
            value
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| value.to_string()),
        ),
        state: "draft".into(),
        commands: vec![],
        relationships: if lighting { vec![] } else { vec![id.into()] },
        data: json!({"setter":if lighting {"appearance-lighting"} else {"appearance-finish"},"finish":id,"field":field,"json":value,"editable":true,"choices":choices,"unit":unit,"accessibility_label":label}),
    }
}
struct Fields {
    elements: std::collections::BTreeMap<String, SemanticElement>,
}
impl Fields {
    fn new(w: &Workspace, id: &str, lighting: bool) -> Self {
        let setter = if lighting {
            "appearance-lighting"
        } else {
            "appearance-finish"
        };
        let mut elements: std::collections::BTreeMap<String, SemanticElement> =
            cad_core::appearance::semantic(
                w.session.visible(),
                w.view.active_part.as_deref(),
                "draft",
            )
            .into_iter()
            .filter(|element| {
                element.data.get("setter").and_then(Value::as_str) == Some(setter)
                    && (lighting || element.data.get("finish").and_then(Value::as_str) == Some(id))
            })
            .filter_map(|element| Some((element.data.get("field")?.as_str()?.to_owned(), element)))
            .collect();
        if !lighting {
            for element in metadata_elements(w, id) {
                if let Some(field) = element.data.get("field").and_then(Value::as_str) {
                    elements.insert(field.into(), element);
                }
            }
        }
        for element in elements.values_mut() {
            if !available(w) {
                element.data["editable"] = false.into();
                element.state = "disabled".into();
            }
            protect_finish_field(w, element);
        }
        Self { elements }
    }
}
fn field(
    ui: &mut egui::Ui,
    w: &mut Workspace,
    fields: &Fields,
    pointer: &str,
    label: &str,
    choices: Value,
) {
    let Some(mut element) = fields.elements.get(pointer).cloned() else {
        return;
    };
    element.label = label.into();
    if choices
        .as_array()
        .is_some_and(|choices| !choices.is_empty())
    {
        element.data["choices"] = choices;
    }
    crate::ui::semantic_field(ui, w, &element);
}
fn panel_open(w: &Workspace) -> bool {
    w.dialog.as_deref() == Some("appearances") || w.fields.contains_key(TOOL)
}
fn description(ui: &mut egui::Ui, id: &str, text: &str) {
    controls::label(ui, id, "appearance-description", &[], text, "ready");
}
fn section_open(w: &Workspace, section: &str) -> bool {
    w.fields
        .get(&format!("appearance-section:{section}"))
        .map(|open| open == "true")
        .unwrap_or_else(|| SECTIONS.iter().any(|(id, _, open)| *id == section && *open))
}
fn section(
    ui: &mut egui::Ui,
    w: &mut Workspace,
    section: &str,
    content: impl FnOnce(&mut egui::Ui, &mut Workspace),
) {
    let Some((_, label, _)) = SECTIONS.iter().find(|(id, _, _)| *id == section) else {
        return;
    };
    let id = format!("appearance:section:{section}");
    let open = section_open(w, section);
    if controls::button(
        ui,
        &id,
        "appearance-section",
        &["semantic-control"],
        &format!("{} {label}", if open { "▾" } else { "▸" }),
        true,
        open,
    )
    .clicked()
    {
        action(w, &format!("{SECTION_TOGGLE}{section}"), None);
    }
    let props = controls::properties(
        &id,
        "appearance-section",
        &[],
        if open { "expanded" } else { "collapsed" },
    );
    if !section_open(w, section)
        || props.get("display").is_some_and(|value| value == "none")
        || props
            .get("visibility")
            .is_some_and(|value| value == "hidden")
    {
        return;
    }
    if let Some(parent) = controls::current_target() {
        let target = parent.child(&id, "appearance-section", &[], "expanded");
        controls::with_target(target, ui, |ui| content(ui, w));
    } else {
        content(ui, w);
    }
}
fn target_elements(w: &Workspace) -> Vec<SemanticElement> {
    let instances = instance_choices(w);
    let mut scopes = vec![json!({"value":"part","label":"Part definition · base finish"})];
    if target_face(w).is_some()
        || w.selected
            .as_ref()
            .is_some_and(|selection| selection.reference.topology == "face")
    {
        scopes.push(json!({"value":"face","label":"Selected face · part override"}));
    }
    if !instances.is_empty() {
        scopes.push(json!({"value":"instance","label":"Component instance · override"}));
    }
    [
        (
            "target-part",
            "Part",
            json!(target_part(w)),
            json!(w.session.visible().parts.values().map(|part| json!({"value":part.id,"label":part.name})).collect::<Vec<_>>()),
        ),
        (
            "target-scope",
            "Apply to",
            json!(scope(w)),
            json!(scopes),
        ),
        (
            "target-instance",
            "Component instance",
            json!(target_instance(w).map(|(assembly, instance)| instance_key(&assembly, &instance))),
            json!(instances.into_iter().map(|(assembly,instance,label)| json!({"value":instance_key(&assembly,&instance),"label":label})).collect::<Vec<_>>()),
        ),
    ]
    .into_iter()
    .map(|(field, label, value, choices)| SemanticElement {
        id: format!("appearance:{field}"),
        role: "appearance-target".into(),
        label: label.into(),
        value: value.as_str().map(str::to_owned),
        state: if available(w) { "ready" } else { "disabled" }.into(),
        commands: vec![],
        relationships: vec!["workspace.appearance-panel".into()],
        data: json!({"setter":"appearance-ui","field":field,"json":value,"editable":available(w),"choices":choices,"value_type":"choice","transient":true,"accessibility_label":label}),
    })
    .map(|mut element| {
        if !source_text_is_valid(w, Some(&element.id)) {
            element.data["editable"] = json!(false);
            element.data["disabled_reason"] = json!("Correct retained finish/lighting text before changing assignment scope");
        }
        element
    })
    .collect()
}
fn assignment_element(w: &Workspace) -> SemanticElement {
    let finish = assigned_finish(w);
    let name = finish
        .as_ref()
        .and_then(|id| w.session.visible().finishes.get(id))
        .map(|finish| finish.name.as_str())
        .unwrap_or("None");
    let label = format!("Assigned finish · {name}");
    SemanticElement {
        id: "appearance:assignment-info".into(),
        role: "appearance-target".into(),
        label,
        value: finish.clone(),
        state: "ready".into(),
        commands: vec![
            "appearance.customize-target".into(),
            "appearance.use-assigned".into(),
        ],
        relationships: target_part(w)
            .into_iter()
            .chain(finish.into_iter().map(|id| format!("finish:{id}")))
            .collect(),
        data: json!({"definition":target_part(w),"instance":target_instance(w),"scope":scope(w),"editing_mode":w.fields.get(EDIT_MODE),"custom_finish":custom_is_current(w),"engineering_material_independent":true}),
    }
}
fn target_warning(w: &Workspace) -> Option<String> {
    let placed = w.session.visible().placed_parts().ok()?;
    if instance_scope(w) {
        let (_, instance) = target_instance(w)?;
        if placed
            .iter()
            .filter(|part| part.path.last() == Some(&instance))
            .count()
            > 1
        {
            return Some("This assembly is repeated. Its component instance finish applies wherever that instance appears.".into());
        }
    } else {
        let part = target_part(w)?;
        let base = assigned_finish(w)?;
        if placed
            .iter()
            .filter(|placed| placed.definition == part)
            .any(|placed| {
                cad_core::appearance::resolve_finish(w.session.visible(), placed)
                    .is_ok_and(|finish| finish.id != base)
            })
        {
            return Some("Some visible configurations or component instances override this part's base finish. Base changes preserve those overrides.".into());
        }
    }
    None
}
fn target_controls(ui: &mut egui::Ui, w: &mut Workspace) {
    for element in target_elements(w) {
        if element.data.get("field").and_then(Value::as_str) != Some("target-instance")
            || instance_scope(w)
        {
            crate::ui::semantic_field(ui, w, &element);
        }
    }
    let assignment = assignment_element(w);
    controls::label(
        ui,
        &assignment.id,
        &assignment.role,
        &[],
        &assignment.label,
        &assignment.state,
    );
    if let Some(warning) = target_warning(w) {
        description(ui, "appearance:target-warning", &warning);
    }
}
pub fn panel(ui: &mut egui::Ui, w: &mut Workspace) {
    controls::label(
        ui,
        "appearance:heading",
        "appearance-heading",
        &[],
        "Appearance",
        "ready",
    );
    if w.fields.contains_key(TOOL) {
        cad_ui::confirmation(ui, w);
    }
    if !panel_open(w) {
        return;
    }
    egui::ScrollArea::vertical()
        .id_salt("appearance-panel-scroll")
        .show(ui, |ui| show_content(ui, w));
}
pub fn show(ui: &mut egui::Ui, w: &mut Workspace) {
    if w.fields.contains_key(TOOL) {
        cad_ui::confirmation(ui, w);
    }
    if !panel_open(w) {
        return;
    }
    show_content(ui, w);
}
fn show_content(ui: &mut egui::Ui, w: &mut Workspace) {
    if !available(w) {
        ui.weak("Apply or cancel the active operation before editing visual finishes");
        return;
    }
    if !w.fields.contains_key(EDITOR) {
        if available(w) {
            if let Some(id) = initial(w) {
                load(w, &id);
            }
        } else {
            ui.weak("Finish the active operation first");
            return;
        }
    }
    if w.dialog.is_none() && !w.fields.contains_key(EDITOR) {
        return;
    }
    ui.horizontal(|ui| {
        for (tab, label) in [("finish", "Finishes"), ("lighting", "Lighting")] {
            if controls::button(
                ui,
                &format!("appearance:tab:{tab}"),
                "appearance-tab",
                &["semantic-control"],
                label,
                true,
                w.fields.get("appearance-tab").map(String::as_str) == Some(tab),
            )
            .clicked()
            {
                action(w, &format!("appearance.tab.{tab}"), None);
            }
        }
    });
    if w.fields.get("appearance-tab").map(String::as_str) == Some("lighting") {
        lighting_controls(ui, w);
        return;
    }
    button(
        ui,
        w,
        "appearance.open-materials",
        "Engineering materials…",
        available(w),
    );
    target_controls(ui, w);
    let selected_id = selected(w);
    crate::finish_library_ui::show(ui, w);
    crate::ui::semantic_field(ui, w, &library_element(w));
    ui.horizontal_wrapped(|ui| {
        button(
            ui,
            w,
            "appearance.assign-target",
            "Assign finish",
            target_key(w).is_some()
                && selected_id.is_some()
                && !crate::finish_library_ui::catalog_selected(w),
        );
        button(
            ui,
            w,
            "appearance.customize-target",
            if face_scope(w) {
                "Customize this face"
            } else if instance_scope(w) {
                "Customize this instance"
            } else {
                "Customize this part"
            },
            target_key(w).is_some(),
        );
    });
    if face_scope(w) {
        button(
            ui,
            w,
            "appearance.inherit-face",
            "Use part finish",
            face_assignment(w).is_some(),
        );
    }
    if instance_scope(w) {
        button(
            ui,
            w,
            "appearance.inherit-instance",
            "Use inherited finish",
            true,
        );
    }
    section(ui, w, "shared-library", |ui, w| {
        description(
            ui,
            "appearance:shared-library-help",
            "Shared finishes can be assigned to many parts. Editing a shared finish updates every assignment that uses it.",
        );
        ui.horizontal_wrapped(|ui| {
            button(
                ui,
                w,
                "appearance.edit",
                "Edit shared finish",
                selected_id.is_some(),
            );
            button(ui, w, "appearance.new", "New", true);
            button(
                ui,
                w,
                "appearance.duplicate",
                "Duplicate",
                selected_id.is_some(),
            );
            button(
                ui,
                w,
                "appearance.delete",
                "Delete",
                selected_id.as_ref().is_some_and(|id| can_delete(w, id)),
            );
        });
    });
    description(
        ui,
        "appearance:material-help",
        "Colors and textures are independent of engineering material, mass and manufacturing data.",
    );
    let shared = w.fields.get(EDIT_MODE).map(String::as_str) == Some("shared");
    if !shared && !custom_is_current(w) {
        description(
            ui,
            "appearance:customize-help",
            "Customize creates a separate finish for this target, so its color and textures can change independently.",
        );
        diagnostics(ui, w);
        return;
    }
    controls::label(
        ui,
        "appearance:edit-scope",
        "appearance-target",
        &[],
        if shared {
            "Editing shared finish · all assignments"
        } else if face_scope(w) {
            "Editing this face's finish"
        } else if instance_scope(w) {
            "Editing this component instance's finish"
        } else {
            "Editing this part's finish"
        },
        if shared { "shared" } else { "custom" },
    );
    let Some(id) = selected(w) else {
        return;
    };
    let finish_editable = finish_editable(w, &id);
    if !finish_editable {
        description(
            ui,
            "appearance:pinned-readonly",
            "This finish is pinned by a linked component. Duplicate it to customize a unique component.",
        );
    }
    let fields = Fields::new(w, &id, false);
    field(ui, w, &fields, "/name", "Finish name", json!([]));
    section(ui, w, "library-details", |ui, w| {
        for element in metadata_elements(w, &id) {
            crate::ui::semantic_field(ui, w, &element);
        }
    });
    section(ui, w, "manufacturing", |ui, w| {
        description(
            ui,
            "appearance:manufacturing-help",
            "Explicit manufacturing requirements are separate from visual roughness and bump relief. Leave optional micrometre fields blank when unspecified; optical presets do not imply a coating specification.",
        );
        for element in manufacturing_elements(w, &id) {
            crate::ui::semantic_field(ui, w, &element);
        }
        button(
            ui,
            w,
            "appearance.clear-manufacturing",
            "Clear manufacturing requirements",
            w.session
                .visible()
                .finishes
                .get(&id)
                .is_some_and(|finish| finish.manufacturing.is_some()),
        );
    });
    if let Some(finish) = w.session.visible().finishes.get(&id) {
        let mut color = finish.base_color.map(|value| value as f32);
        ui.horizontal(|ui| {
            controls::label(
                ui,
                "appearance:color-label",
                "appearance-description",
                &[],
                "Color",
                "ready",
            );
            let props = controls::properties(
                &format!("finish:{id}::swatch"),
                "appearance-swatch",
                &[],
                "draft",
            );
            let changed = !props.get("display").is_some_and(|value| value == "none")
                && !props
                    .get("visibility")
                    .is_some_and(|value| value == "hidden")
                && finish_editable
                && ui
                    .scope(|ui| {
                        controls::apply_properties(ui, &props);
                        ui.color_edit_button_rgb(&mut color).changed()
                    })
                    .inner;
            if changed {
                let element = field_element(
                    &id,
                    "/base_color",
                    "Base color",
                    json!(color),
                    false,
                    json!([]),
                    None,
                );
                if let Err(error) = set_value(w, &element, json!(color.map(f64::from))) {
                    w.status = error.to_string();
                }
            }
        });
        section(ui, w, "color-values", |ui, w| {
            for (pointer, label) in [
                ("/base_color/0", "Red"),
                ("/base_color/1", "Green"),
                ("/base_color/2", "Blue"),
            ] {
                field(ui, w, &fields, pointer, label, json!([]));
            }
        });
    }
    section(ui, w, "surface", |ui, w| {
        for (pointer, label) in [
            ("/metallic", "Metallic"),
            ("/roughness", "Roughness"),
            ("/clearcoat", "Clear coat"),
            ("/clearcoat_roughness", "Coat roughness"),
            ("/anisotropy", "Directional reflections"),
        ] {
            field(ui, w, &fields, pointer, label, json!([]));
        }
    });
    section(ui, w, "textures", |ui, w| {
        description(
            ui,
            "appearance:texture-help",
            "Import grayscale bump maps or color, normal and surface response images. PNG, JPEG, TIFF and WebP are supported.",
        );
        for (slot, label) in [
            ("height", "Grayscale bump / height"),
            ("base_color", "Color"),
            ("normal", "Normal"),
            ("roughness", "Roughness"),
            ("metallic", "Metallic"),
            ("occlusion", "Occlusion"),
        ] {
            let map = serde_json::to_value(w.session.visible().finishes.get(&id))
                .ok()
                .and_then(|source| {
                    source
                        .get("textures")?
                        .get(slot)?
                        .as_str()
                        .map(str::to_owned)
                });
            let path = map
                .as_ref()
                .and_then(|map| w.session.visible().resources.get(map))
                .map(|resource| resource.path.clone());
            ui.horizontal_wrapped(|ui| {
                controls::label(
                    ui,
                    &format!("finish:{id}::texture:{slot}"),
                    "appearance-texture-row",
                    &[],
                    label,
                    "ready",
                );
                button(
                    ui,
                    w,
                    &format!("{IMPORT}{id}/{slot}"),
                    if map.is_some() {
                        "Replace…"
                    } else {
                        "Import…"
                    },
                    true,
                );
                button(
                    ui,
                    w,
                    &format!("{CLEAR}{id}/{slot}"),
                    "Clear",
                    map.is_some(),
                );
            });
            if let Some(path) = path {
                controls::label(
                    ui,
                    &format!("finish:{id}::texture:{slot}:path"),
                    "appearance-description",
                    &[],
                    &path,
                    "ready",
                )
                .on_hover_text(&path);
            } else if map.is_some() {
                ui.colored_label(
                    ui.visuals().error_fg_color,
                    "Missing pinned image; import a replacement.",
                );
            }
            if slot == "height" {
                field(
                    ui,
                    w,
                    &fields,
                    "/builtin_bump",
                    "Built-in bump",
                    json!([{"value":"none","label":"None"},{"value":"brushed","label":"Brushed grain"},{"value":"blasted","label":"Bead blasted"},{"value":"machined","label":"Machined tool marks"},{"value":"orange_peel","label":"Paint / powder coat"},{"value":"molded","label":"Molded polymer"},{"value":"wood","label":"Wood grain"}]),
                );
                field(ui, w, &fields, "/height_mm", "Bump depth (mm)", json!([]));
                description(
                    ui,
                    "appearance:grain-help",
                    "The grayscale bump map drives surface grain. An imported height image overrides the built-in bump; Clear returns to the selected built-in map. Clear the image and choose None to remove bump relief. Tile size and texture direction scale and rotate the bump; depth controls its lighting relief.",
                );
            } else if slot == "normal" {
                field(
                    ui,
                    w,
                    &fields,
                    "/normal_strength",
                    "Normal strength",
                    json!([]),
                );
            }
            field(
                ui,
                w,
                &fields,
                &format!("/textures/{slot}"),
                "Pinned image",
                resource_choices(w),
            );
        }
        description(
            ui,
            "appearance:bump-help",
            "Grayscale height adds lighting relief: white is high, black is low. Depth and tile size use mm; CAD geometry stays unchanged. Color maps multiply the color tint. Normal maps use OpenGL (+Y).",
        );
    });
    section(ui, w, "mapping", |ui, w| {
        field(
            ui,
            w,
            &fields,
            "/mapping/mode",
            "Projection",
            json!([{"value":"triplanar","label":"Box / triplanar"},{"value":"planar","label":"Planar"}]),
        );
        for (pointer, label, unit) in [
            ("/mapping/scale_mm/0", "Tile width", "mm"),
            ("/mapping/scale_mm/1", "Tile height", "mm"),
            ("/mapping/rotation_deg", "Direction", "deg"),
            ("/mapping/offset_mm/0", "Offset U", "mm"),
            ("/mapping/offset_mm/1", "Offset V", "mm"),
        ] {
            field(
                ui,
                w,
                &fields,
                pointer,
                &format!("{label} ({unit})"),
                json!([]),
            );
        }
    });
    section(ui, w, "source", |ui, w| {
        crate::ui::semantic_field(ui, w, &source_element(w));
        button(
            ui,
            w,
            "appearance.refresh-source",
            "Refresh source from draft",
            true,
        );
    });
    if let Some(error) = w.fields.get(INVALID) {
        ui.colored_label(ui.visuals().error_fg_color, error);
    }
    diagnostics(ui, w);
}
fn resource_choices(w: &Workspace) -> Value {
    let mut choices = vec![json!({"value":null,"label":"None"})];
    choices.extend(
        w.session
            .visible()
            .resources
            .values()
            .filter(|resource| {
                ["png", "jpg", "jpeg", "tif", "tiff", "webp"].contains(&resource.kind.as_str())
            })
            .map(|resource| json!({"value":resource.id,"label":resource.path})),
    );
    json!(choices)
}
fn edit_environment(w: &mut Workspace, import: bool) -> Result<()> {
    let mut document = w.session.visible().clone();
    document.lighting.environment = if import {
        let Some(path) = rfd::FileDialog::new()
            .set_title("Import a linear HDR environment panorama")
            .add_filter("HDR environment", &["hdr", "exr"])
            .pick_file()
        else {
            return Ok(());
        };
        let bytes = crate::environment_assets::validate_file(&path)?;
        Some(cad_core::persistence::import_resource_bytes(
            &mut document,
            &w.root(),
            &path,
            &bytes,
        )?)
    } else {
        None
    };
    document.validate()?;
    let expected = document.fingerprint();
    w.dispatch(Command::ReplaceDocument { document });
    if w.session.visible().fingerprint() != expected {
        return Err(CadError::Invalid(w.status.clone()));
    }
    w.fields.insert("appearance-tab".into(), "lighting".into());
    w.status = if import {
        "Validating the environment and filtering reflections; Apply waits for the current resource"
    } else {
        "Built-in studio selected"
    }
    .into();
    Ok(())
}
fn environment_semantics(w: &Workspace) -> Vec<SemanticElement> {
    let mut out = Vec::new();
    for (command, label, enabled) in [
        (
            "appearance.environment.import",
            "Import HDR environment…",
            available(w),
        ),
        (
            "appearance.environment.clear",
            "Use built-in studio",
            available(w) && w.session.visible().lighting.environment.is_some(),
        ),
    ] {
        out.push(SemanticElement{id:format!("command:{command}"),role:"appearance-action".into(),label:label.into(),value:None,state:if enabled{"ready"}else{"disabled"}.into(),commands:vec![command.into()],relationships:vec!["viewport-lighting".into()],data:json!({"file_formats":["hdr","exr"],"projection":"equirectangular-2-to-1","max_file_bytes":crate::environment_assets::MAX_FILE_BYTES,"accessibility_label":label})});
    }
    let info = w
        .fields
        .get("appearance-environment-render-info")
        .and_then(|text| serde_json::from_str::<Value>(text).ok())
        .unwrap_or_else(|| json!({"state":"pending"}));
    out.push(SemanticElement {
        id: "viewport-lighting::environment-render-info".into(),
        role: "appearance-environment-info".into(),
        label: "Environment resource status".into(),
        value: info.get("state").and_then(Value::as_str).map(str::to_owned),
        state: info
            .get("state")
            .and_then(Value::as_str)
            .unwrap_or("pending")
            .into(),
        commands: vec![],
        relationships: w
            .session
            .visible()
            .lighting
            .environment
            .clone()
            .into_iter()
            .collect(),
        data: info,
    });
    out
}
fn lighting_controls(ui: &mut egui::Ui, w: &mut Workspace) {
    let fields = Fields::new(w, "lighting", true);
    ui.small("Environment reflections and direct lights update while you design.");
    field(
        ui,
        w,
        &fields,
        "/environment",
        "Reflection environment",
        json!([]),
    );
    for element in environment_semantics(w)
        .into_iter()
        .filter(|element| !element.commands.is_empty())
    {
        if controls::button(
            ui,
            &element.id,
            &element.role,
            &["semantic-control"],
            &element.label,
            element.state != "disabled",
            false,
        )
        .clicked()
        {
            action(w, &element.commands[0], None);
        }
    }
    description(
        ui,
        "viewport-lighting::environment-profile",
        "Import a 2:1 linear HDR/EXR panorama. Reflection lighting updates after bounded decoding and filtering. The workspace background remains themeable.",
    );
    for (pointer, label, unit) in [
        (
            "/environment_rotation_deg",
            "Environment rotation",
            Some("deg"),
        ),
        ("/exposure", "Exposure", Some("stops")),
        ("/intensity", "Environment strength", None),
        ("/key_intensity", "Key light", None),
        ("/fill_intensity", "Fill light", None),
        ("/shadows", "Shadows", None),
    ] {
        field(
            ui,
            w,
            &fields,
            pointer,
            &unit
                .map(|unit| format!("{label} ({unit})"))
                .unwrap_or_else(|| label.into()),
            json!([]),
        );
    }
    diagnostics(ui, w);
}
pub fn lighting_view(ui: &mut egui::Ui, w: &mut Workspace) {
    if !available(w) {
        ui.weak("Apply or cancel the active operation before editing lighting");
        return;
    }
    if !w.fields.contains_key(EDITOR) {
        if available(w) {
            if let Some(id) = initial(w) {
                load(w, &id);
            }
        } else {
            ui.weak("Finish the active operation first");
            return;
        }
    }
    if w.fields.contains_key(TOOL) {
        cad_ui::confirmation(ui, w);
    }
    lighting_controls(ui, w);
}
fn diagnostics(ui: &mut egui::Ui, w: &mut Workspace) {
    for (key, error) in w
        .fields
        .iter()
        .filter(|(key, _)| key.starts_with("appearance-field-invalid:"))
    {
        ui.colored_label(ui.visuals().error_fg_color, error)
            .on_hover_text(key);
    }
    for error in &w.appearance_diagnostics {
        ui.colored_label(ui.visuals().error_fg_color, error);
    }
    for element in face_diagnostics(w) {
        controls::label(
            ui,
            &element.id,
            &element.role,
            &[],
            &element.label,
            &element.state,
        );
        if controls::button(
            ui,
            &format!("{}:remove", element.id),
            "appearance-action",
            &["semantic-control"],
            "Remove face override",
            available(w),
            false,
        )
        .clicked()
        {
            action(w, "appearance.remove-face-override", Some(&element.data));
        }
        if controls::button(
            ui,
            &format!("{}:repair", element.id),
            "appearance-action",
            &["semantic-control"],
            "Reattach to selected face",
            available(w)
                && w.selected
                    .as_ref()
                    .is_some_and(|selection| selection.reference.topology == "face"),
            false,
        )
        .clicked()
        {
            action(w, "appearance.repair-face", Some(&element.data));
        }
    }
}
pub fn semantics(w: &Workspace) -> Vec<SemanticElement> {
    let mut elements = vec![library_element(w), source_element(w), assignment_element(w)];
    elements.extend(environment_semantics(w));
    elements.extend(target_elements(w));
    if let Some(id) = selected(w) {
        elements.extend(metadata_elements(w, &id));
        elements.extend(manufacturing_elements(w, &id));
    }
    elements.extend(face_diagnostics(w));
    for (section, label, _) in SECTIONS {
        let open = section_open(w, section);
        elements.push(SemanticElement {
            id: format!("appearance:section:{section}"),
            role: "appearance-section".into(),
            label: (*label).into(),
            value: Some(open.to_string()),
            state: if open { "expanded" } else { "collapsed" }.into(),
            commands: vec![format!("{SECTION_TOGGLE}{section}")],
            relationships: vec!["workspace.appearance-panel".into()],
            data: json!({"setter":"appearance-ui","field":"section","section":section,"json":open,"value_type":"boolean","editable":true,"transient":true,"accessibility_label":label}),
        });
    }
    elements.push(SemanticElement {
        id: "workspace.appearance-panel".into(),
        role: "appearance-panel".into(),
        label: "Appearance · Colors and Textures".into(),
        value: selected(w),
        state: if panel_open(w) { "open" } else { "closed" }.into(),
        commands: vec!["appearances".into()],
        relationships: vec!["workspace.appearances".into(), "workspace.lighting".into()],
        data: json!({"renderer":"appearance-panel","definition":target_part(w),"instance":target_instance(w),"scope":scope(w),"transaction":"shared-document-draft","confirmation":"current-validated-evaluation","independent_engineering_material":true}),
    });
    for (id, role, label) in [
        ("appearance:heading", "appearance-heading", "Appearance"),
        (
            "appearance:material-help",
            "appearance-description",
            "Colors and textures are independent of engineering material, mass and manufacturing data.",
        ),
        (
            "appearance:customize-help",
            "appearance-description",
            "Customize creates a separate finish for this target, so its color and textures can change independently.",
        ),
        (
            "appearance:shared-library-help",
            "appearance-description",
            "Shared finishes can be assigned to many parts. Editing a shared finish updates every assignment that uses it.",
        ),
        (
            "appearance:grain-help",
            "appearance-description",
            "The grayscale bump map drives surface grain. An imported height image overrides the built-in bump; Clear returns to the selected built-in map. Clear the image and choose None to remove bump relief. Tile size and texture direction scale and rotate the bump; depth controls its lighting relief.",
        ),
        (
            "appearance:texture-help",
            "appearance-description",
            "Import grayscale bump maps or color, normal and surface response images. PNG, JPEG, TIFF and WebP are supported.",
        ),
        (
            "appearance:bump-help",
            "appearance-description",
            "Grayscale height adds lighting relief: white is high, black is low. Depth and tile size use mm; CAD geometry stays unchanged. Color maps multiply the color tint. Normal maps use OpenGL (+Y).",
        ),
        ("appearance:color-label", "appearance-description", "Color"),
    ] {
        elements.push(SemanticElement {
            id: id.into(),
            role: role.into(),
            label: label.into(),
            value: None,
            state: "ready".into(),
            commands: vec![],
            relationships: vec!["workspace.appearance-panel".into()],
            data: json!({"accessibility_label":label}),
        });
    }
    if let Some(warning) = target_warning(w) {
        elements.push(SemanticElement {
            id: "appearance:target-warning".into(),
            role: "appearance-description".into(),
            label: warning,
            value: None,
            state: "warning".into(),
            commands: vec![],
            relationships: target_part(w).into_iter().collect(),
            data: json!({"scope":scope(w)}),
        });
    }
    let shared = w.fields.get(EDIT_MODE).map(String::as_str) == Some("shared");
    elements.push(SemanticElement {
        id: "appearance:edit-scope".into(),
        role: "appearance-target".into(),
        label: if shared {
            "Editing shared finish · all assignments"
        } else if face_scope(w) {
            "Editing this face's finish"
        } else if instance_scope(w) {
            "Editing this component instance's finish"
        } else {
            "Editing this part's finish"
        }
        .into(),
        value: selected(w),
        state: if shared {
            "shared"
        } else if custom_is_current(w) {
            "custom"
        } else {
            "assignment"
        }
        .into(),
        commands: vec![],
        relationships: vec!["workspace.appearance-panel".into()],
        data: json!({"custom_finish":custom_is_current(w),"shared_finish":shared}),
    });
    for (command, label, enabled) in [
        (
            "appearance.customize-target",
            "Customize target color and textures independently",
            available(w) && target_key(w).is_some(),
        ),
        (
            "appearance.assign-target",
            "Assign selected finish to target",
            available(w) && target_key(w).is_some() && selected(w).is_some(),
        ),
        (
            "appearance.use-assigned",
            "Show assigned target finish",
            available(w) && assigned_finish(w).is_some(),
        ),
        (
            "appearance.edit",
            "Edit shared finish for all assignments",
            available(w) && selected(w).is_some(),
        ),
        ("appearance.new", "New visual finish", available(w)),
        (
            "appearance.refresh-source",
            "Refresh finish source from draft",
            selected(w).is_some(),
        ),
        (
            "appearance.open-materials",
            "Engineering materials…",
            available(w),
        ),
        (
            "appearance.clear-manufacturing",
            "Clear manufacturing finish requirements",
            available(w)
                && selected(w)
                    .filter(|id| finish_editable(w, id))
                    .and_then(|id| w.session.visible().finishes.get(&id))
                    .is_some_and(|finish| finish.manufacturing.is_some()),
        ),
        (
            "appearance.duplicate",
            "Duplicate visual finish",
            available(w) && selected(w).is_some(),
        ),
        (
            "appearance.delete",
            "Delete visual finish",
            available(w) && selected(w).as_ref().is_some_and(|id| can_delete(w, id)),
        ),
        (
            "appearance.capture-face",
            "Use selected model face",
            available(w)
                && w.selected
                    .as_ref()
                    .is_some_and(|selection| selection.reference.topology == "face"),
        ),
        (
            "appearance.inherit-face",
            "Use part finish for selected face",
            available(w) && face_scope(w) && face_assignment(w).is_some(),
        ),
        (
            "appearance.assign-part",
            "Assign finish to base part",
            available(w) && target_part(w).is_some(),
        ),
        (
            "appearance.assign-instance",
            "Assign finish to occurrence",
            available(w) && target_instance(w).is_some(),
        ),
        (
            "appearance.inherit-instance",
            "Use inherited finish",
            available(w) && target_instance(w).is_some(),
        ),
    ] {
        elements.push(SemanticElement {id:format!("command:{command}"),role:"appearance-action".into(),label:label.into(),value:(!enabled).then(||"Select the required part or unused visual finish; finish the active operation first".into()),state:if enabled{"ready"}else{"disabled"}.into(),commands:vec![command.into()],relationships:vec!["workspace.appearances".into()],data:json!({"finish":selected(w),"accessibility_label":label})});
    }
    for (tab, label) in [("finish", "Finishes"), ("lighting", "Lighting")] {
        elements.push(SemanticElement {
            id: format!("appearance:tab:{tab}"),
            role: "appearance-tab".into(),
            label: label.into(),
            value: None,
            state: if w.fields.get("appearance-tab").map(String::as_str) == Some(tab) {
                "selected"
            } else {
                "ready"
            }
            .into(),
            commands: vec![format!("appearance.tab.{tab}")],
            relationships: vec!["workspace.appearances".into()],
            data: json!({"accessibility_label":label}),
        });
    }
    for (id, role, label) in [
        (
            "workspace.appearances",
            "appearances-editor",
            "Visual finishes",
        ),
        ("workspace.lighting", "lighting-editor", "Studio lighting"),
    ] {
        elements.push(SemanticElement {
            id: id.into(),
            role: role.into(),
            label: label.into(),
            value: None,
            state: "ready".into(),
            commands: vec![],
            relationships: vec![],
            data: json!({"renderer":role}),
        });
    }
    for finish in w.session.visible().finishes.values() {
        let mut swatch = field_element(
            &finish.id,
            "/base_color",
            "Color tint",
            json!(finish.base_color),
            false,
            json!([]),
            None,
        );
        swatch.id = format!("finish:{}::swatch", finish.id);
        swatch.role = "appearance-swatch".into();
        swatch.data["value_type"] = "color-srgb".into();
        protect_finish_field(w, &mut swatch);
        elements.push(swatch);
        for slot in [
            "base_color",
            "normal",
            "roughness",
            "metallic",
            "occlusion",
            "height",
        ] {
            let resource = finish_source(finish)["textures"][slot].clone();
            if let Some(path) = resource
                .as_str()
                .and_then(|id| w.session.visible().resources.get(id))
                .map(|resource| &resource.path)
            {
                elements.push(SemanticElement {
                    id: format!("finish:{}::texture:{slot}:path", finish.id),
                    role: "appearance-description".into(),
                    label: path.clone(),
                    value: Some(path.clone()),
                    state: "ready".into(),
                    commands: vec![],
                    relationships: resource.as_str().map(str::to_owned).into_iter().collect(),
                    data: json!({"finish":finish.id,"slot":slot,"path":path,"resource":resource}),
                });
            }
            elements.push(SemanticElement {
                id: format!("finish:{}::texture:{slot}", finish.id),
                role: "appearance-texture-row".into(),
                label: texture_label(slot).into(),
                value: resource.as_str().map(str::to_owned),
                state: if resource.is_null() { "empty" } else { "assigned" }.into(),
                commands: vec![format!("{IMPORT}{}/{slot}",finish.id),format!("{CLEAR}{}/{slot}",finish.id)],
                relationships: vec![format!("finish:{}",finish.id)],
                data: json!({"finish":finish.id,"slot":slot,"resource":resource,"data_space":if slot=="base_color"{"srgb"}else{"linear"},"depth_unit":(slot=="height").then_some("mm")}),
            });
            for (prefix, label) in [(IMPORT, "Import texture"), (CLEAR, "Clear texture")] {
                let command = format!("{prefix}{}/{slot}", finish.id);
                elements.push(SemanticElement {
                    id: format!("command:{command}"),
                    role: "appearance-action".into(),
                    label: format!("{label} · {} · {}", texture_label(slot), finish.name),
                    value: None,
                    state: if finish_editable(w, &finish.id)
                        && (prefix == IMPORT || !resource.is_null())
                    {
                        "ready"
                    } else {
                        "disabled"
                    }
                    .into(),
                    commands: vec![command],
                    relationships: vec![finish.id.clone()],
                    data: json!({"finish":finish.id,"slot":slot}),
                });
            }
        }
    }
    for element in &mut elements {
        if element
            .commands
            .iter()
            .any(|command| selection_changing(command))
            && !source_text_is_valid(w, None)
        {
            element.state = "disabled".into();
            element.value = Some(
                "Correct retained finish/lighting text before changing selection or assignment"
                    .into(),
            );
            element.data["disabled_reason"] = json!(
                "Correct retained finish/lighting text before changing selection or assignment"
            );
        }
    }
    elements
}
pub fn reconcile_semantics(w: &Workspace, elements: &mut [SemanticElement]) {
    for element in elements {
        protect_finish_field(w, element);
        if element.data["setter"] == "appearance-ui"
            && element.data["field"].as_str().is_some_and(|field| {
                matches!(
                    field,
                    "library" | "target-part" | "target-scope" | "target-instance"
                )
            })
            && !source_text_is_valid(w, Some(&element.id))
        {
            element.data["editable"] = json!(false);
            element.data["disabled_reason"] = json!(
                "Correct retained finish/lighting text before changing selection or assignment"
            );
        }
        if element
            .data
            .get("setter")
            .and_then(Value::as_str)
            .is_some_and(|setter| setter.starts_with("appearance-"))
        {
            if !available(w) {
                element.data["editable"] = false.into();
                element.state = "disabled".into();
                element.data["diagnostics"] =
                    json!(["Apply or cancel the active operation before editing visual finishes"]);
            } else if let Some(error) = w
                .fields
                .get(&format!("appearance-field-invalid:{}", element.id))
            {
                element.state = "invalid".into();
                element.data["diagnostics"] = json!([error]);
            }
        }
    }
}
fn library_element(w: &Workspace) -> SemanticElement {
    let mut element = SemanticElement {
        id: "appearance:library".into(),
        role: "appearance-library-selection".into(),
        label: "Named finish".into(),
        value: selected(w).or_else(|| initial(w)),
        state: "ready".into(),
        commands: vec![],
        relationships: vec!["surface-finishes".into()],
        data: json!({"setter":"appearance-ui","field":"library","json":selected(w).or_else(||initial(w)),"editable":available(w),"choices":w.session.visible().finishes.values().map(|finish|json!({"value":finish.id,"label":finish.name})).collect::<Vec<_>>()}),
    };
    if !source_text_is_valid(w, Some(&element.id)) {
        element.data["editable"] = json!(false);
        element.data["disabled_reason"] =
            json!("Correct retained finish/lighting text before changing the selected finish");
    }
    element
}
fn source_element(w: &Workspace) -> SemanticElement {
    let mut element = SemanticElement {
        id: "appearance:source".into(),
        role: "appearance-source".into(),
        label: "Finish JSON".into(),
        value: w.fields.get(SOURCE).cloned(),
        state: if w.fields.contains_key(INVALID) {
            "invalid"
        } else {
            "ready"
        }
        .into(),
        commands: vec![],
        relationships: selected(w).into_iter().collect(),
        data: json!({"setter":"appearance-ui","field":"source","json":w.fields.get(SOURCE).cloned().unwrap_or_default(),"editable":available(w)&&selected(w).is_some(),"multiline":true,"accessibility_label":"Advanced visual finish source"}),
    };
    protect_finish_field(w, &mut element);
    element
}

/// Catalog/browser actions share the appearance tool checkpoint and assignment path.
pub fn select_named_finish(w: &mut Workspace, id: &str) -> Result<()> {
    if !w.session.visible().finishes.contains_key(id) {
        return Err(CadError::Invalid("The selected finish is missing".into()));
    }
    if selected(w).as_deref() == Some(id) {
        return Ok(());
    }
    selection_ready(w)?;
    load(w, id);
    w.fields.insert(EDIT_MODE.into(), "assignment".into());
    Ok(())
}
pub fn add_named_finish(w: &mut Workspace, finish: Finish, assign: bool) -> Result<()> {
    selection_ready(w)?;
    if !available(w) {
        return Err(CadError::Invalid(
            "Finish the active operation before assigning appearances".into(),
        ));
    }
    if !w.fields.contains_key(TOOL) {
        begin(w, false);
        if !w.fields.contains_key(TOOL) {
            return Err(CadError::Invalid(w.status.clone()));
        }
    }
    let id = finish.id.clone();
    let mut document = w.session.visible().clone();
    document.finishes.insert(id.clone(), finish);
    if assign {
        if face_scope(w) {
            let mut face = face_assignment(w)
                .or_else(|| target_face(w))
                .ok_or_else(|| CadError::Invalid("Choose a native face".into()))?;
            face.finish = id.clone();
            let definition = face.body.definition.clone();
            cad_core::appearance::set_face_finish(&mut document, &definition, face)?;
        } else if instance_scope(w) {
            let (assembly, instance) = target_instance(w)
                .ok_or_else(|| CadError::Invalid("Choose a component instance".into()))?;
            document
                .assemblies
                .get_mut(&assembly)
                .and_then(|assembly| assembly.instances.get_mut(&instance))
                .ok_or_else(|| CadError::Invalid("The component instance no longer exists".into()))?
                .finish = Some(id.clone());
        } else {
            let definition =
                target_part(w).ok_or_else(|| CadError::Invalid("Choose a target part".into()))?;
            document
                .parts
                .get_mut(&definition)
                .ok_or_else(|| CadError::Invalid("The target part no longer exists".into()))?
                .finish = id.clone();
        }
    }
    document.validate()?;
    let expected = document.fingerprint();
    w.dispatch(Command::ReplaceDocument { document });
    if w.session.visible().fingerprint() != expected {
        return Err(CadError::Invalid(w.status.clone()));
    }
    select_named_finish(w, &id)?;
    Ok(())
}

pub fn selected_finish(w: &Workspace) -> Option<Id> {
    selected(w)
}

fn metadata_elements(w: &Workspace, id: &str) -> Vec<SemanticElement> {
    let Some(finish) = w.session.visible().finishes.get(id) else {
        return vec![];
    };
    let metadata = finish.metadata.clone().unwrap_or_default();
    [
        ("category", "Finish family", metadata.category),
        ("description", "Description", metadata.description),
        ("tags", "Tags · comma separated", metadata.tags.join(", ")),
        ("source", "Visual preset source", metadata.source),
    ]
    .into_iter()
    .map(|(name, label, value)| {
        let mut element = field_element(
            id,
            &format!("/metadata/{name}"),
            label,
            json!(value),
            false,
            json!([]),
            None,
        );
        element.data["editable"] = json!(finish_editable(w, id));
        protect_finish_field(w, &mut element);
        element
    })
    .collect()
}
fn face_diagnostics(w: &Workspace) -> Vec<SemanticElement> {
    let Some(part) = target_part(w).and_then(|id| w.session.visible().parts.get(&id)) else {
        return vec![];
    };
    let Some(evaluation) = crate::viewport::display_evaluation(w)
        .filter(|evaluation| evaluation.valid && evaluation.identity == w.session.identity())
    else {
        return vec![];
    };
    let Some(placed) = crate::viewport::display_instances(w, evaluation)
        .into_iter()
        .find(|placed| placed.definition == part.id)
    else {
        return vec![];
    };
    let meshes = crate::viewport::display_meshes(w, evaluation, placed);
    cad_core::appearance::resolve_face_finishes(part,&meshes).diagnostics.into_iter().map(|diagnostic| {
        let assignment=part.face_finishes.iter().find(|assignment|assignment.id==diagnostic.assignment);
        SemanticElement{id:format!("appearance:face-diagnostic:{}",diagnostic.assignment),role:"appearance-face-diagnostic".into(),label:format!("Face finish · {} · {}",diagnostic.state,diagnostic.message),value:Some(diagnostic.assignment.clone()),state:"warning".into(),commands:vec!["appearance.remove-face-override".into(),"appearance.repair-face".into()],relationships:vec![part.id.clone()],data:json!({"definition":part.id,"assignment_id":diagnostic.assignment,"assignment":assignment,"diagnostic":diagnostic,"fallback":"part-finish","blocks_apply":false})}
    }).collect()
}

fn manufacturing_elements(w: &Workspace, id: &str) -> Vec<SemanticElement> {
    let Some(finish) = w.session.visible().finishes.get(id) else {
        return vec![];
    };
    let source = finish.manufacturing.clone().unwrap_or_default();
    [
        ("process", "Process", source.process, None),
        ("specification", "Specification", source.specification, None),
        (
            "designation",
            "Designation / color code",
            source.designation,
            None,
        ),
        (
            "coating_thickness_um",
            "Coating thickness",
            source
                .coating_thickness_um
                .map(|value| value.to_string())
                .unwrap_or_default(),
            Some("µm"),
        ),
        (
            "surface_roughness_ra_um",
            "Specified surface Ra",
            source
                .surface_roughness_ra_um
                .map(|value| value.to_string())
                .unwrap_or_default(),
            Some("µm"),
        ),
        ("notes", "Manufacturing notes", source.notes, None),
    ]
    .into_iter()
    .map(|(name, label, value, unit)| {
        let mut element = field_element(
            id,
            &format!("/manufacturing/{name}"),
            label,
            json!(value),
            false,
            json!([]),
            unit,
        );
        element.data["manufacturing_requirement"] = true.into();
        element.data["optical_property"] = false.into();
        element.data["optional"] = json!(unit.is_some());
        element.data["editable"] = json!(finish_editable(w, id));
        protect_finish_field(w, &mut element);
        element
    })
    .collect()
}
