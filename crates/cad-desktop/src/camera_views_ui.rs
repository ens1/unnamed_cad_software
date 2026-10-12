//! Named camera source editing and a replaceable f64 navigation adapter.
use crate::{Workspace, presentation_controls as controls};
use bevy_egui::egui;
use cad_core::{
    CadError, Result,
    camera_views::{self, Definition, Pose, Projection},
    commands::Command,
    presentation::SemanticElement,
};
use serde_json::{Value, json};
const TOOL: &str = "camera-view-tool";
const SELECTED: &str = "camera-view-selected";
const TOKEN: &str = "camera-view-token";
const BEFORE: &str = "camera-view-before";
const INVALID: &str = "camera-view-invalid:";
fn mirror(pose: &Pose) -> ([f32; 2], f32, [f32; 3], f32) {
    (
        pose.orbit_rad.map(|v| v as f32),
        pose.roll_rad as f32,
        [
            pose.target_mm[0] as f32 * 0.01,
            pose.target_mm[2] as f32 * 0.01,
            -pose.target_mm[1] as f32 * 0.01,
        ],
        pose.distance_mm as f32 * 0.01,
    )
}
/// Existing fit, sketch-normal and standard-view callers may still write the
/// legacy mirrors. Detect that change; never let an old bookmark pin navigation.
pub fn precise(view: &cad_core::presentation::PresentationState) -> Option<&Pose> {
    view.camera_pose.as_ref().filter(|pose| {
        let (orbit, roll, target, distance) = mirror(pose);
        orbit == view.orbit
            && roll == view.roll
            && target == view.target
            && distance == view.distance
            && pose.validate().is_ok()
    })
}
pub fn current(w: &Workspace) -> Pose {
    let mut pose = if let Some(pose) = precise(&w.view) {
        pose.clone()
    } else {
        Pose {
            target_mm: [
                f64::from(w.view.target[0]) * 100.,
                -f64::from(w.view.target[2]) * 100.,
                f64::from(w.view.target[1]) * 100.,
            ],
            orbit_rad: w.view.orbit.map(f64::from),
            roll_rad: f64::from(w.view.roll),
            distance_mm: f64::from(w.view.distance) * 100.,
            projection: w.view.camera_projection.clone(),
        }
    };
    pose.projection = w.view.camera_projection.clone();
    // Older focus/normal-to/zoom callers express scale with their view distance.
    // Preserve that intent when the current projection is orthographic.
    if precise(&w.view).is_none()
        && let Some(previous) = &w.view.camera_pose
        && let Projection::Orthographic { vertical_span_mm } = &mut pose.projection
    {
        let (_, _, _, old_distance) = mirror(previous);
        if old_distance.is_finite() && old_distance > 0. && old_distance != w.view.distance {
            *vertical_span_mm = (*vertical_span_mm * f64::from(w.view.distance)
                / f64::from(old_distance))
            .clamp(0.1, 1e8);
        }
    }
    if pose.validate().is_err() {
        pose = Pose::default();
    }
    pose.normalize();
    pose
}
pub fn set_current(w: &mut Workspace, mut pose: Pose) {
    pose.normalize();
    if let Err(error) = pose.validate() {
        w.status = error.to_string();
        return;
    }
    let (orbit, roll, target, distance) = mirror(&pose);
    w.view.orbit = orbit;
    w.view.roll = roll;
    w.view.target = target;
    w.view.distance = distance;
    w.view.camera_projection = pose.projection.clone();
    w.view.camera_pose = Some(pose);
    w.hover_guide = None;
    w.hover_cache = Default::default();
}
pub fn after_fit(w: &mut Workspace) {
    let mut pose = current(w);
    let aspect = f64::from(w.viewport_size[0].clamp(32, 4096))
        / f64::from(w.viewport_size[1].clamp(32, 4096));
    match pose.projection {
        Projection::Orthographic { .. } => {
            pose.projection = Projection::Orthographic {
                vertical_span_mm: (2. * pose.distance_mm * (std::f64::consts::PI / 8.).tan()
                    / aspect.min(1.))
                .clamp(0.1, 1e8),
            };
        }
        Projection::Perspective { vertical_fov_deg } => {
            pose.distance_mm = (pose.distance_mm * (std::f64::consts::PI / 8.).tan()
                / (vertical_fov_deg.to_radians() * 0.5).tan()
                / aspect.min(1.))
            .clamp(0.5, 1e6);
        }
    }
    set_current(w, pose);
}

fn available(w: &Workspace) -> bool {
    w.sketch.is_none()
        && w.settings.is_none()
        && (w.tool_checkpoint.is_none() || w.fields.contains_key(TOOL))
        && !crate::drawing_interaction_ui::active(w)
}
fn source(w: &Workspace) -> Option<&Definition> {
    w.session
        .visible()
        .camera_views
        .get(w.fields.get(SELECTED)?)
}
pub fn active(w: &Workspace) -> bool {
    w.fields.contains_key(TOOL)
}
pub fn field_error(w: &mut Workspace, element: &SemanticElement, error: Option<String>) {
    if active(w)
        && source(w).is_some()
        && element.data.get("setter").and_then(Value::as_str) == Some("camera-view")
        && element.data.get("view").and_then(Value::as_str)
            == w.fields.get(SELECTED).map(String::as_str)
        && element.data.get("token").and_then(Value::as_str)
            == w.fields.get(TOKEN).map(String::as_str)
        && element.data.get("document").and_then(Value::as_str)
            == Some(w.session.visible().id.as_str())
    {
        let key = format!("{INVALID}{}", element.id);
        match error {
            Some(error) => {
                w.confirm_requested = false;
                w.fields.insert(key, error);
            }
            None => {
                w.fields.remove(&key);
            }
        }
    }
}

pub fn field_is_invalid(w: &Workspace, element: &SemanticElement) -> bool {
    element.data.get("setter").and_then(Value::as_str) == Some("camera-view")
        && w.fields.contains_key(&format!("{INVALID}{}", element.id))
}
pub fn current_source_is_valid(w: &Workspace) -> bool {
    if !active(w) {
        return true;
    }
    !w.fields.keys().any(|key| key.starts_with(INVALID))
        && (w
            .fields
            .get("camera-view-deleting")
            .is_some_and(|v| v == "true")
            || source(w).is_some_and(|view| view.validate().is_ok()))
}
pub fn ready_to_confirm(w: &mut Workspace) -> bool {
    if current_source_is_valid(w) {
        true
    } else {
        w.status = "Correct the current named-view fields before applying".into();
        false
    }
}
pub fn cancel(w: &mut Workspace) {
    if !active(w) {
        return;
    }
    if let Some(pose) = w
        .fields
        .get(BEFORE)
        .and_then(|s| serde_json::from_str::<Pose>(s).ok())
    {
        set_current(w, pose);
    }
    w.fields.retain(|key, _| !key.starts_with("camera-view-"));
}

fn submit(w: &mut Workspace, view: Definition) -> Result<()> {
    view.validate()?;
    let expected = view.clone();
    let id = view.id.clone();
    w.dispatch(Command::PutCameraView {
        view: Box::new(view),
    });
    if w.session.visible().camera_views.get(&id) != Some(&expected) {
        return Err(CadError::Invalid(w.status.clone()));
    }
    w.json = serde_json::to_string_pretty(&expected)?;
    Ok(())
}
fn begin(w: &mut Workspace, view: Definition, new: bool) -> Result<()> {
    if active(w) && !current_source_is_valid(w) {
        return Err(CadError::Invalid(
            "Correct the current named-view source, or cancel it, before changing views".into(),
        ));
    }
    if !available(w) {
        return Err(CadError::Invalid(
            "Apply or cancel the active operation before editing named views".into(),
        ));
    }
    if !active(w) {
        let pose = current(w);
        let exploded = w.view.exploded_view.clone();
        let display = w.view.display_state.clone();
        w.start_tool();
        w.view.exploded_view = exploded;
        w.view.display_state = display;
        w.fields
            .insert(BEFORE.into(), serde_json::to_string(&pose)?);
    }
    w.fields.retain(|key, _| !key.starts_with(INVALID));
    w.fields.remove("camera-view-deleting");
    w.fields.insert(TOOL.into(), "true".into());
    w.fields.insert(SELECTED.into(), view.id.clone());
    w.fields.insert(TOKEN.into(), cad_core::id());
    w.json = serde_json::to_string_pretty(&view)?;
    w.open_editor("camera-views");
    if new {
        submit(w, view)?;
    }
    Ok(())
}
fn selected_id<'a>(w: &'a Workspace, args: Option<&'a Value>) -> Option<&'a str> {
    args.and_then(|v| v.get("view"))
        .and_then(Value::as_str)
        .or_else(|| w.fields.get(SELECTED).map(String::as_str))
}
pub fn handle_command(w: &mut Workspace, command: &str, args: Option<&Value>) -> bool {
    if command != "camera-views" && !command.starts_with("camera-view.") {
        return false;
    }
    let result = (|| -> Result<()> {
        if command == "camera-views" {
            if !available(w) {
                return Err(CadError::Invalid(
                    "Finish the active operation before opening the named-view manager".into(),
                ));
            }
            w.open_editor("camera-views");
            return Ok(());
        }
        if command == "camera-view.toggle-source" {
            let open = w
                .fields
                .get("camera-views-source-open")
                .is_some_and(|v| v == "true");
            w.fields
                .insert("camera-views-source-open".into(), (!open).to_string());
            return Ok(());
        }
        if command == "camera-view.save" {
            let name = (1..)
                .map(|i| format!("View{i}"))
                .find(|name| {
                    !w.session
                        .visible()
                        .camera_views
                        .values()
                        .any(|v| v.name.eq_ignore_ascii_case(name))
                })
                .unwrap();
            return begin(w, Definition::new(name, current(w)), true);
        }
        if matches!(
            command,
            "camera-view.perspective" | "camera-view.orthographic"
        ) {
            if crate::drawing_interaction_ui::active(w)
                || w.sketch.as_ref().is_some_and(|s| {
                    s.drag.is_some() || !s.chain.is_empty() || !s.clicks.is_empty()
                })
            {
                return Err(CadError::Invalid(
                    "Finish the current viewport gesture before changing projection".into(),
                ));
            }
            let mut pose = current(w);
            pose.switch_projection(command.ends_with("orthographic"));
            crate::navigation::stop(w);
            set_current(w, pose);
            return Ok(());
        }
        let id = selected_id(w, args)
            .ok_or_else(|| CadError::Invalid("Choose a named camera view".into()))?
            .to_owned();
        if args
            .and_then(|v| v.get("document"))
            .and_then(Value::as_str)
            .is_some_and(|id| id != w.session.visible().id)
        {
            return Err(CadError::Invalid(
                "This named-view control belongs to a different document".into(),
            ));
        }
        let view = w
            .session
            .visible()
            .camera_views
            .get(&id)
            .cloned()
            .ok_or_else(|| CadError::Invalid("The named camera view no longer exists".into()))?;
        match command {
            "camera-view.recall" => {
                if crate::drawing_interaction_ui::active(w)
                    || w.sketch.as_ref().is_some_and(|s| {
                        s.drag.is_some() || !s.chain.is_empty() || !s.clicks.is_empty()
                    })
                {
                    return Err(CadError::Invalid(
                        "Finish the active viewport gesture before recalling a view".into(),
                    ));
                }
                crate::navigation::stop(w);
                set_current(w, view.pose);
                if !active(w) {
                    w.fields.insert(SELECTED.into(), id);
                }
                w.status = format!("Recalled {}", view.name);
            }
            "camera-view.edit" | "camera-view.rename" => begin(w, view, false)?,
            "camera-view.update" => {
                begin(w, view.clone(), false)?;
                let mut view = view;
                view.pose = current(w);
                submit(w, view)?;
            }
            "camera-view.delete" => {
                begin(w, view, false)?;
                w.dispatch(Command::DeleteCameraView { view: id.clone() });
                if w.session.visible().camera_views.contains_key(&id) {
                    return Err(CadError::Invalid(w.status.clone()));
                }
                w.fields
                    .insert("camera-view-deleting".into(), "true".into());
            }
            _ => return Err(CadError::Invalid("Unknown named-view command".into())),
        }
        Ok(())
    })();
    if let Err(error) = result {
        w.status = error.to_string();
    }
    true
}
const FIELDS: &[(&str, &str, Option<&str>)] = &[
    ("/name", "Name", None),
    ("/pose/target_mm/0", "Target X", Some("mm")),
    ("/pose/target_mm/1", "Target Y", Some("mm")),
    ("/pose/target_mm/2", "Target Z", Some("mm")),
    ("/pose/orbit_rad/0", "Yaw", Some("deg")),
    ("/pose/orbit_rad/1", "Pitch", Some("deg")),
    ("/pose/roll_rad", "Roll", Some("deg")),
    ("/pose/distance_mm", "Camera distance", Some("mm")),
    ("/pose/projection/kind", "Projection", None),
];
fn value(view: &Definition, pointer: &str) -> Option<Value> {
    let mut value = serde_json::to_value(view).ok()?.pointer(pointer)?.clone();
    if pointer.contains("orbit_rad") || pointer == "/pose/roll_rad" {
        value = json!(value.as_f64()?.to_degrees());
    }
    Some(value)
}
pub fn elements(w: &Workspace) -> Vec<SemanticElement> {
    let mut out = camera_views::semantic(w.session.visible());
    let pose = current(w);
    for element in &mut out {
        if element.role == "camera-view"
            && element
                .data
                .get("view")
                .and_then(Value::as_str)
                .and_then(|id| w.session.visible().camera_views.get(id))
                .is_some_and(|view| {
                    let mut saved = view.pose.clone();
                    saved.normalize();
                    saved == pose
                })
        {
            element.state = "current".into();
        }
    }
    let can_edit = available(w) && current_source_is_valid(w);
    let can_recall = !crate::drawing_interaction_ui::active(w)
        && !w
            .sketch
            .as_ref()
            .is_some_and(|s| s.drag.is_some() || !s.chain.is_empty() || !s.clicks.is_empty());
    for row in out.iter_mut().filter(|e| e.role == "camera-view") {
        row.data["actions"] = json!(row.commands.iter().map(|command| {
            let enabled = if command == "camera-view.recall" { can_recall } else { can_edit };
            json!({"command":command,"label":command.rsplit('.').next().unwrap_or(command),"enabled":enabled,"disabled_reason":if enabled{None}else{Some(if command=="camera-view.recall"{"Finish the active viewport gesture"}else{"Correct, apply or cancel the active editor before changing named views"})}})
        }).collect::<Vec<_>>());
    }
    out.push(SemanticElement{id:"workspace.camera-views".into(),role:"camera-views-editor".into(),label:"Named views".into(),value:None,state:if w.dialog.as_deref()==Some("camera-views"){"open"}else{"closed"}.into(),commands:vec!["camera-views".into()],relationships:vec!["camera-views".into()],data:json!({"renderer":"camera-views-editor","pose":pose,"capture_precision":"f64-or-legacy-adapter","recall_changes_geometry":false})});
    for (command, label, enabled) in [
        ("camera-view.save", "Save current view…", can_edit),
        (
            "camera-view.update",
            "Update from current view",
            can_edit && source(w).is_some(),
        ),
    ] {
        out.push(SemanticElement{id:format!("command:{command}"),role:"camera-view-action".into(),label:label.into(),value:None,state:if enabled{"ready"}else{"disabled"}.into(),commands:vec![command.into()],relationships:vec!["camera-views".into()],data:json!({"document":w.session.visible().id,"view":w.fields.get(SELECTED),"accessibility_label":label})});
    }
    for (command, label, current) in [
        (
            "camera-view.perspective",
            "Perspective",
            matches!(pose.projection, Projection::Perspective { .. }),
        ),
        (
            "camera-view.orthographic",
            "Orthographic",
            matches!(pose.projection, Projection::Orthographic { .. }),
        ),
    ] {
        out.push(SemanticElement{id:format!("command:{command}"),role:"camera-projection-action".into(),label:label.into(),value:None,state:if current{"selected"}else{"ready"}.into(),commands:vec![command.into()],relationships:vec!["workspace.camera-views".into()],data:json!({"projection":pose.projection,"source_mutation":false,"recall_requires_evaluation":false})});
    }
    out.push(SemanticElement{id:"camera-views:help".into(),role:"camera-view-description".into(),label:"Recall changes only camera navigation. Configuration, hidden/ghost states and exploded arrangements stay independent.".into(),value:None,state:"ready".into(),commands:vec![],relationships:vec!["camera-views".into()],data:json!({})});
    let Some(view) = source(w).filter(|_| active(w)) else {
        return out;
    };
    let mut fields = FIELDS.to_vec();
    fields.push(match view.pose.projection {
        Projection::Perspective { .. } => (
            "/pose/projection/vertical_fov_deg",
            "Vertical field of view",
            Some("deg"),
        ),
        Projection::Orthographic { .. } => (
            "/pose/projection/vertical_span_mm",
            "Vertical view span",
            Some("mm"),
        ),
    });
    for (pointer, label, unit) in fields {
        let id = format!("camera-view:{}:field:{pointer}", view.id);
        let current = value(view, pointer).unwrap_or(Value::Null);
        let error = w.fields.get(&format!("{INVALID}{id}"));
        out.push(SemanticElement{id,role:"camera-view-field".into(),label:label.into(),value:Some(current.as_str().map(str::to_owned).unwrap_or_else(||current.to_string())),state:if error.is_some(){"invalid"}else{"draft"}.into(),commands:vec![],relationships:vec![format!("camera-view:{}",view.id)],data:json!({"setter":"camera-view","document":w.session.visible().id,"view":view.id,"token":w.fields.get(TOKEN),"editor":w.fields.get(TOKEN),"field":pointer,"json":current,"editable":available(w),"unit":unit,"value_type":if pointer=="/pose/projection/kind"{"choice"}else if current.is_number(){"number"}else{"text"},"choices":if pointer=="/pose/projection/kind"{json!([{"value":"perspective","label":"Perspective"},{"value":"orthographic","label":"Orthographic"}])}else{json!([])},"diagnostics":error.into_iter().collect::<Vec<_>>()})});
    }
    let open = w
        .fields
        .get("camera-views-source-open")
        .is_some_and(|v| v == "true");
    out.push(SemanticElement{id:"camera-views:advanced-toggle".into(),role:"camera-view-section".into(),label:"Advanced Source".into(),value:Some(open.to_string()),state:if open{"expanded"}else{"collapsed"}.into(),commands:vec!["camera-view.toggle-source".into()],relationships:vec![format!("camera-view:{}:source",view.id)],data:json!({"transient":true,"open":open,"accessibility_label":"Show complete named-view source"})});
    let id = format!("camera-view:{}:source", view.id);
    out.push(SemanticElement{id:id.clone(),role:"camera-view-source".into(),label:"Advanced Source".into(),value:Some(w.json.clone()),state:if w.fields.contains_key(&format!("{INVALID}{id}")){"invalid"}else{"draft"}.into(),commands:vec![],relationships:vec![format!("camera-view:{}",view.id)],data:json!({"setter":"camera-view","document":w.session.visible().id,"view":view.id,"token":w.fields.get(TOKEN),"editor":w.fields.get(TOKEN),"field":"/source","json":w.json,"editable":available(w),"multiline":true,"accessibility_label":"Complete named camera view source"})});
    out
}
pub fn current_value(w: &Workspace, element: &SemanticElement) -> Option<Value> {
    if element.data.get("setter")?.as_str()? != "camera-view" {
        return None;
    }
    if !active(w)
        || source(w).is_none()
        || element.data.get("document").and_then(Value::as_str)
            != Some(w.session.visible().id.as_str())
        || element.data.get("token").and_then(Value::as_str)
            != w.fields.get(TOKEN).map(String::as_str)
        || element.data.get("view").and_then(Value::as_str)
            != w.fields.get(SELECTED).map(String::as_str)
    {
        return None;
    }
    if element.data.get("field").and_then(Value::as_str) == Some("/source") {
        return Some(json!(w.json));
    }
    value(
        w.session
            .visible()
            .camera_views
            .get(element.data.get("view")?.as_str()?)?,
        element.data.get("field")?.as_str()?,
    )
}
pub fn set_value(w: &mut Workspace, element: &SemanticElement, input: Value) -> bool {
    if element.data.get("setter").and_then(Value::as_str) != Some("camera-view") {
        return false;
    }
    if !active(w)
        || source(w).is_none()
        || element.data.get("document").and_then(Value::as_str)
            != Some(w.session.visible().id.as_str())
        || element.data.get("token").and_then(Value::as_str)
            != w.fields.get(TOKEN).map(String::as_str)
        || element.data.get("view").and_then(Value::as_str)
            != w.fields.get(SELECTED).map(String::as_str)
    {
        w.status = "The named-view editor control is stale".into();
        return false;
    }
    let result = (|| -> Result<()> {
        if !available(w)
            || !active(w)
            || element.data.get("document").and_then(Value::as_str)
                != Some(w.session.visible().id.as_str())
            || element.data.get("token").and_then(Value::as_str)
                != w.fields.get(TOKEN).map(String::as_str)
        {
            return Err(CadError::Invalid(
                "The named-view editor control is stale".into(),
            ));
        }
        let view = source(w)
            .cloned()
            .filter(|v| element.data.get("view").and_then(Value::as_str) == Some(v.id.as_str()))
            .ok_or_else(|| CadError::Invalid("The named-view target changed".into()))?;
        let pointer = element
            .data
            .get("field")
            .and_then(Value::as_str)
            .ok_or_else(|| CadError::Invalid("Missing camera field".into()))?;
        if pointer == "/source" {
            let text = input
                .as_str()
                .ok_or_else(|| CadError::Invalid("Enter complete named-view JSON".into()))?
                .to_owned();
            w.json = text.clone();
            let parsed: Definition = serde_json::from_str(&text)?;
            if parsed.id != view.id {
                return Err(CadError::Invalid(
                    "Keep the selected named-view identity".into(),
                ));
            }
            let pose = parsed.pose.clone();
            submit(w, parsed)?;
            set_current(w, pose);
            w.json = text;
            return Ok(());
        }
        if w.fields
            .contains_key(&format!("{INVALID}camera-view:{}:source", view.id))
        {
            return Err(CadError::Invalid(
                "Correct the Advanced Source JSON before editing its individual fields".into(),
            ));
        }
        let mut src = serde_json::to_value(&view)?;
        if pointer == "/pose/projection/kind" {
            let mut pose = view.pose.clone();
            match input.as_str() {
                Some("perspective") => pose.switch_projection(false),
                Some("orthographic") => pose.switch_projection(true),
                _ => {
                    return Err(CadError::Invalid(
                        "Choose perspective or orthographic projection".into(),
                    ));
                }
            };
            src["pose"] = serde_json::to_value(pose)?;
        } else {
            let value = if pointer.contains("orbit_rad") || pointer == "/pose/roll_rad" {
                json!(
                    input
                        .as_f64()
                        .ok_or_else(|| CadError::Invalid("Enter a finite angle in degrees".into()))?
                        .to_radians()
                )
            } else {
                input
            };
            *src.pointer_mut(pointer)
                .ok_or_else(|| CadError::Invalid("Camera field no longer exists".into()))? = value;
        }
        let view: Definition = serde_json::from_value(src)?;
        view.validate()?;
        let pose = view.pose.clone();
        submit(w, view)?;
        set_current(w, pose);
        Ok(())
    })();
    let key = format!("{INVALID}{}", element.id);
    match result {
        Ok(()) => {
            w.fields.remove(&key);
        }
        Err(error) => {
            w.confirm_requested = false;
            w.fields.insert(key, error.to_string());
            w.status = error.to_string();
        }
    }
    true
}
pub fn semantic_action(w: &mut Workspace, element: &SemanticElement, command: &str) {
    handle_command(w, command, Some(&element.data));
}
pub fn show(ui: &mut egui::Ui, w: &mut Workspace) {
    controls::label(
        ui,
        "camera-views:heading",
        "camera-view-library",
        &[],
        "Named views",
        "ready",
    );
    if active(w) {
        crate::cad_ui::confirmation(ui, w);
    }
    if controls::button(
        ui,
        "command:camera-view.save",
        "camera-view-action",
        &["semantic-control"],
        "Save current view…",
        available(w),
        false,
    )
    .clicked()
    {
        handle_command(w, "camera-view.save", None);
    }
    ui.horizontal(|ui| {
        for element in elements(w)
            .into_iter()
            .filter(|e| e.role == "camera-projection-action")
        {
            if controls::button(
                ui,
                &element.id,
                &element.role,
                &["semantic-control"],
                &element.label,
                true,
                element.state == "selected",
            )
            .clicked()
            {
                semantic_action(w, &element, &element.commands[0]);
            }
        }
    });
    let rows = elements(w)
        .into_iter()
        .filter(|e| e.role == "camera-view")
        .collect::<Vec<_>>();
    for row in rows {
        ui.horizontal_wrapped(|ui| {
            if controls::button(
                ui,
                &row.id,
                &row.role,
                &["semantic-control"],
                &row.label,
                true,
                row.state == "current",
            )
            .clicked()
            {
                semantic_action(w, &row, "camera-view.recall");
            }
            for (suffix, label, command) in [
                ("edit", "Edit", "camera-view.edit"),
                ("delete", "Delete", "camera-view.delete"),
            ] {
                if controls::button(
                    ui,
                    &format!("{}:{suffix}", row.id),
                    "camera-view-action",
                    &["semantic-control"],
                    label,
                    available(w),
                    false,
                )
                .clicked()
                {
                    semantic_action(w, &row, command);
                }
            }
        });
    }
    if w.fields
        .get("camera-view-deleting")
        .is_some_and(|v| v == "true")
    {
        ui.weak("This view will be deleted when you Apply.");
        return;
    }
    for field in elements(w)
        .into_iter()
        .filter(|e| e.role == "camera-view-field")
    {
        crate::ui::semantic_field(ui, w, &field);
    }
    if active(w) && source(w).is_some() {
        if controls::button(
            ui,
            "command:camera-view.update",
            "camera-view-action",
            &["semantic-control"],
            "Update from current view",
            available(w),
            false,
        )
        .clicked()
        {
            handle_command(w, "camera-view.update", None);
        }
        if let Some(toggle) = elements(w)
            .into_iter()
            .find(|element| element.role == "camera-view-section")
        {
            if controls::button(
                ui,
                &toggle.id,
                &toggle.role,
                &["semantic-control"],
                &format!(
                    "{} Advanced Source",
                    if toggle.state == "expanded" {
                        "▾"
                    } else {
                        "▸"
                    }
                ),
                true,
                toggle.state == "expanded",
            )
            .clicked()
            {
                handle_command(w, "camera-view.toggle-source", None);
            }
            if w.fields
                .get("camera-views-source-open")
                .is_some_and(|v| v == "true")
                && let Some(field) = elements(w)
                    .into_iter()
                    .find(|element| element.role == "camera-view-source")
            {
                crate::ui::semantic_field(ui, w, &field);
            }
        }
    }
    for (_, error) in w.fields.iter().filter(|(key, _)| key.starts_with(INVALID)) {
        ui.colored_label(ui.visuals().error_fg_color, error);
    }
    controls::label(
        ui,
        "camera-views:help",
        "camera-view-description",
        &[],
        "Recall changes only camera navigation. Configuration, hidden/ghost states and exploded arrangements stay independent.",
        "ready",
    );
}

pub fn view_controls(ui: &mut egui::Ui, w: &mut Workspace) {
    for element in elements(w)
        .into_iter()
        .filter(|e| e.role == "camera-projection-action")
    {
        if controls::button(
            ui,
            &element.id,
            &element.role,
            &["semantic-control"],
            &element.label,
            true,
            element.state == "selected",
        )
        .clicked()
        {
            semantic_action(w, &element, &element.commands[0]);
            ui.close();
        }
    }
    controls::menu_button(ui, "view:named-camera-views", "Named views", |ui| {
        for (command, label) in [
            ("camera-views", "Manage named views…"),
            ("camera-view.save", "Save current view…"),
        ] {
            if controls::button(
                ui,
                &format!("command:{command}"),
                "camera-view-action",
                &["semantic-control"],
                label,
                available(w),
                false,
            )
            .clicked()
            {
                handle_command(w, command, None);
                ui.close();
            }
        }
        ui.separator();
        let props = controls::properties(
            "view:named-camera-views",
            "menu",
            &["menu-heading"],
            "ready",
        );
        egui::ScrollArea::vertical()
            .max_height(controls::number(&props, "max-height", 240.).max(24.))
            .show(ui, |ui| {
                for element in elements(w).into_iter().filter(|e| e.role == "camera-view") {
                    if controls::button(
                        ui,
                        &element.id,
                        &element.role,
                        &["semantic-control"],
                        &element.label,
                        true,
                        element.state == "current",
                    )
                    .clicked()
                    {
                        semantic_action(w, &element, "camera-view.recall");
                        ui.close();
                    }
                }
            });
    });
}
