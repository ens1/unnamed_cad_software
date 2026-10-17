//! Sourced standard hardware insertion, with replaceable semantic controls.
use crate::{Workspace, cad_ui, presentation_controls as controls};
use bevy_egui::egui;
use cad_core::{
    CadError, Dimension, Expr, Result,
    presentation::SemanticElement,
    standard_parts::{self, Catalog, Family, Insertion, Request},
};
use serde_json::{Value, json};
const TOOL: &str = "standard-parts-tool";
const PREFIX: &str = "standard-parts-";
const ERRORS: &str = "standard-parts-field-invalid:";
const LAST: &str = "standard-parts-preview-request";
const ERROR: &str = "standard-parts-preview-error";
fn catalog() -> Catalog {
    standard_parts::catalog().unwrap_or(Catalog {
        schema_version: 1,
        catalog_version: String::new(),
        name: String::new(),
        sources: vec![],
        entries: vec![],
    })
}
fn library_notice() -> Option<(&'static str, String)> {
    match standard_parts::catalog() {
        Ok(catalog) if catalog.entries.is_empty() => Some(("empty", "No standard-parts pack is available. Saved hardware keeps its geometry and material. Download a pack to insert hardware, or make your own part from a sketch.".into())),
        Err(error) => Some(("error", error.to_string())),
        _ => None,
    }
}
fn stored<'a>(w: &'a Workspace, key: &str) -> Option<&'a str> {
    w.fields.get(&format!("{PREFIX}{key}")).map(String::as_str)
}
fn put(w: &mut Workspace, key: &str, value: impl Into<String>) {
    w.fields.insert(format!("{PREFIX}{key}"), value.into());
}
fn available(w: &Workspace) -> bool {
    w.sketch.is_none()
        && w.settings.is_none()
        && (w.tool_checkpoint.is_none() || w.fields.contains_key(TOOL))
}
fn owner(w: &Workspace) -> Option<String> {
    stored(w, "owner")
        .map(str::to_owned)
        .or_else(|| w.fields.get("assembly-id").cloned())
        .or_else(|| Some(w.session.visible().root.clone()))
        .filter(|owner| w.session.visible().assemblies.contains_key(owner))
}
fn entry(w: &Workspace) -> Option<standard_parts::Entry> {
    let selected = stored(w, "entry");
    catalog()
        .entries
        .into_iter()
        .find(|entry| selected == Some(entry.id.as_str()))
        .or_else(|| {
            catalog()
                .entries
                .into_iter()
                .find(|entry| entry.shape.family() == Family::SocketScrew)
        })
}
fn family(w: &Workspace) -> Family {
    entry(w)
        .map(|entry| entry.shape.family())
        .unwrap_or(Family::SocketScrew)
}
fn base(w: &Workspace) -> Result<cad_core::Document> {
    w.tool_checkpoint
        .as_ref()
        .map(|checkpoint| checkpoint.visible().clone())
        .ok_or_else(|| CadError::Invalid("The hardware insertion checkpoint is unavailable".into()))
}
fn parsed(w: &Workspace) -> Result<Insertion> {
    let document = base(w)?;
    let owner = owner(w).ok_or_else(|| CadError::Invalid("Choose an owning assembly".into()))?;
    let entry =
        entry(w).ok_or_else(|| CadError::Invalid("Choose a sourced hardware size".into()))?;
    let length_mm = if entry.shape.family().has_length() {
        Some(cad_core::expression::value(
            &document,
            &owner,
            &Expr(stored(w, "length").unwrap_or("20 mm").into()),
            Dimension::Length,
        )?)
    } else {
        None
    };
    let mut placement = cad_core::Placement::default();
    for axis in 0..3 {
        placement.translation[axis] = cad_core::expression::value(
            &document,
            &owner,
            &Expr(
                stored(w, &format!("translation-{axis}"))
                    .unwrap_or("0 mm")
                    .into(),
            ),
            Dimension::Length,
        )?;
        placement.rotation_deg[axis] = cad_core::expression::value(
            &document,
            &owner,
            &Expr(
                stored(w, &format!("rotation-{axis}"))
                    .unwrap_or("0 deg")
                    .into(),
            ),
            Dimension::Angle,
        )?
        .to_degrees();
    }
    Ok(Insertion {
        assembly: owner,
        definition: stored(w, "definition")
            .ok_or_else(|| CadError::Invalid("The hardware source identity is missing".into()))?
            .into(),
        instance: stored(w, "instance")
            .ok_or_else(|| CadError::Invalid("The hardware instance identity is missing".into()))?
            .into(),
        request: Request {
            entry: entry.id,
            length_mm,
            material: stored(w, "material")
                .unwrap_or(&w.session.visible().settings.default_new_part_material)
                .into(),
            finish: stored(w, "finish")
                .unwrap_or(cad_core::appearance::DEFAULT_FINISH)
                .into(),
            reuse_unchanged: stored(w, "reuse").is_none_or(|value| value == "true"),
        },
        placement,
        fixed: stored(w, "fixed") == Some("true"),
    })
}
fn refresh(w: &mut Workspace) -> Result<()> {
    let insertion = parsed(w)?;
    let mut candidate = base(w)?;
    let definition = standard_parts::insert(&mut candidate, &insertion)?;
    let expected = candidate.assemblies[&insertion.assembly].instances[&insertion.instance].clone();
    let command = cad_core::library::replace_command(w.session.visible(), candidate, &w.root())?;
    w.dispatch(command);
    if w.session
        .visible()
        .assemblies
        .get(&insertion.assembly)
        .and_then(|assembly| assembly.instances.get(&insertion.instance))
        != Some(&expected)
    {
        return Err(CadError::Invalid(w.status.clone()));
    }
    w.fields
        .insert(LAST.into(), serde_json::to_string(&insertion)?);
    put(w, "preview-definition", definition);
    w.fields.remove(ERROR);
    w.view.active_part = None;
    w.view.active_instance = Some(insertion.instance);
    w.rendered_key.clear();
    Ok(())
}
pub fn begin(w: &mut Workspace) {
    if !available(w) {
        w.status = "Apply or cancel the active operation before inserting standard hardware".into();
        return;
    }
    let Some(owner) = owner(w) else {
        w.status = "Create or select an assembly before inserting standard hardware".into();
        return;
    };
    if cad_core::library::is_definition_pinned(w.session.visible(), &owner) {
        w.status = "Make the linked assembly unique before inserting hardware".into();
        return;
    }
    if let Some((_, notice)) = library_notice() {
        crate::ui::action(w, "content-repositories.show-standard-parts");
        w.status = notice;
        return;
    }
    if !w.fields.contains_key(TOOL) {
        cancel(w);
        w.activate_context(None, None);
        w.start_tool();
        w.fields.insert(TOOL.into(), "true".into());
        put(w, "owner", owner.clone());
        put(w, "definition", cad_core::id());
        put(w, "instance", cad_core::id());
        let catalog = catalog();
        if let Some(entry) = catalog
            .entries
            .iter()
            .find(|entry| entry.shape.family() == Family::SocketScrew && entry.size == "M6")
            .or_else(|| catalog.entries.first())
        {
            put(w, "entry", entry.id.clone());
        }
        let material = if w.session.visible().materials.contains_key("steel") {
            "steel".into()
        } else {
            w.session
                .visible()
                .settings
                .default_new_part_material
                .clone()
        };
        let finish = w.session.visible().settings.default_new_part_finish.clone();
        let fixed = w.session.visible().assemblies[&owner].instances.is_empty();
        put(w, "material", material);
        put(w, "finish", finish);
        put(w, "length", "20 mm");
        put(w, "fixed", fixed.to_string());
        put(w, "reuse", "true");
    }
    w.open_editor("standard-parts");
    if let Err(error) = refresh(w) {
        w.fields.insert(ERROR.into(), error.to_string());
        w.status = error.to_string();
    }
}
pub fn cancel(w: &mut Workspace) {
    w.fields.retain(|key, _| {
        !key.starts_with(PREFIX) && !key.starts_with("presentation-field:standard-parts:")
    });
}
pub fn current_source_is_valid(w: &Workspace) -> bool {
    if !w.fields.contains_key(TOOL) {
        return true;
    }
    if w.fields.contains_key(ERROR) || w.fields.keys().any(|key| key.starts_with(ERRORS)) {
        return false;
    }
    parsed(w).is_ok_and(|insertion| {
        serde_json::to_string(&insertion).is_ok_and(|json| w.fields.get(LAST) == Some(&json))
            && w.session
                .visible()
                .assemblies
                .get(&insertion.assembly)
                .and_then(|assembly| assembly.instances.get(&insertion.instance))
                .is_some_and(|instance| {
                    stored(w, "preview-definition") == Some(instance.definition.as_str())
                        && instance.placement == insertion.placement
                        && instance.fixed == insertion.fixed
                        && standard_parts::origin(&insertion.request)
                            .and_then(|origin| {
                                standard_parts::build(
                                    &origin,
                                    &instance.definition,
                                    &insertion.request.material,
                                    &insertion.request.finish,
                                )
                            })
                            .is_ok_and(|expected| {
                                w.session.visible().parts.get(&instance.definition)
                                    == Some(&expected)
                            })
                })
    })
}
pub fn field_is_invalid(w: &Workspace, element: &SemanticElement) -> bool {
    w.fields.contains_key(&format!("{ERRORS}{}", element.id))
}
pub fn field_error(w: &mut Workspace, element: &SemanticElement, error: Option<String>) {
    if element.data.get("setter").and_then(Value::as_str) != Some("standard-parts") {
        return;
    }
    let key = format!("{ERRORS}{}", element.id);
    if let Some(error) = error {
        w.fields.insert(key, error);
    } else {
        w.fields.remove(&key);
    }
}
pub fn action(w: &mut Workspace, command: &str, _arguments: Option<&Value>) -> bool {
    if command == "standard-parts" {
        begin(w);
        return true;
    }
    if command != "standard-parts.preview" {
        return false;
    }
    if let Err(error) = refresh(w) {
        w.fields.insert(ERROR.into(), error.to_string());
        w.status = error.to_string();
    }
    true
}
fn field(w: &Workspace, name: &str, label: &str, value: Value, choices: Value) -> SemanticElement {
    SemanticElement {
        id: format!("standard-parts:{name}"),
        role: "standard-part-field".into(),
        label: label.into(),
        value: Some(
            value
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| value.to_string()),
        ),
        state: if available(w) { "draft" } else { "disabled" }.into(),
        commands: vec![],
        relationships: vec!["workspace.standard-parts".into()],
        data: json!({"setter":"standard-parts","field":name,"json":value,"editable":available(w),"choices":choices,"accessibility_label":label,"classes":["modeling-field"]}),
    }
}
fn library_actions() -> Vec<SemanticElement> {
    let mut actions = vec![];
    if library_notice().is_some() {
        actions.push((
            "download-pack",
            "content-repositories.download-standard-parts",
            "Download standard-parts pack",
        ));
    }
    actions.push((
        "get-libraries",
        "content-repositories.show-standard-parts",
        "Manage libraries…",
    ));
    actions
        .into_iter()
        .map(|(id, command, label)| SemanticElement {
            id: format!("command:standard-parts:{id}"),
            role: "standard-part-action".into(),
            label: label.into(),
            value: None,
            state: "ready".into(),
            commands: vec![command.into()],
            relationships: vec!["workspace.standard-parts".into()],
            data: json!({"enabled":true,"accessibility_label":label}),
        })
        .collect()
}
fn show_library_actions(ui: &mut egui::Ui, w: &mut Workspace) {
    ui.horizontal_wrapped(|ui| {
        for element in library_actions() {
            if controls::button(
                ui,
                &element.id,
                &element.role,
                &["semantic-control"],
                &element.label,
                true,
                false,
            )
            .clicked()
            {
                crate::ui::semantic_action(w, &element, &element.commands[0]);
            }
        }
    });
}
pub fn semantics(w: &Workspace) -> Vec<SemanticElement> {
    let catalog = catalog();
    let family = family(w);
    let entry = entry(w);
    if let Some((state, notice)) = library_notice() {
        let mut elements = vec![
            SemanticElement {
                id: "workspace.standard-parts".into(),
                role: "standard-parts-editor".into(),
                label: "Standard Hardware".into(),
                value: Some(notice.clone()),
                state: state.into(),
                commands: vec!["content-repositories.show-standard-parts".into()],
                relationships: vec![],
                data: json!({"renderer":"standard-parts-editor","library_required_for_new_insertion":true,"saved_geometry_preserved":true,"pack":"standard-parts"}),
            },
            SemanticElement {
                id: "standard-parts:library-notice".into(),
                role: "standard-part-help".into(),
                label: notice,
                value: None,
                state: state.into(),
                commands: vec![],
                relationships: vec!["workspace.standard-parts".into()],
                data: json!({"pack":"standard-parts","saved_geometry_preserved":true}),
            },
        ];
        elements.extend(library_actions());
        return elements;
    }

    let mut elements=vec![SemanticElement{id:"workspace.standard-parts".into(),role:"standard-parts-editor".into(),label:"Standard Hardware".into(),value:entry.as_ref().map(|entry|entry.id.clone()),state:if w.dialog.as_deref()==Some("standard-parts"){"open"}else{"closed"}.into(),commands:vec!["standard-parts".into()],relationships:owner(w).into_iter().collect(),data:json!({"renderer":"standard-parts-editor","version":standard_parts::CONTRACT_VERSION,"catalog_version":catalog.catalog_version,"native_sketch_geometry":true,"simplified_threads":true,"transaction":"shared-document-draft"})},
 field(w,"owner","Owning assembly",json!(owner(w).unwrap_or_default()),json!(w.session.visible().assemblies.values().map(|assembly|json!({"value":assembly.id,"label":assembly.name})).collect::<Vec<_>>())),
 field(w,"family","Hardware",json!(family),json!(Family::ALL.into_iter().map(|family|json!({"value":family,"label":family.label()})).collect::<Vec<_>>())),
 field(w,"entry","Size / dimensional reference",json!(entry.as_ref().map(|entry|entry.id.as_str()).unwrap_or_default()),json!(catalog.entries.iter().filter(|entry|entry.shape.family()==family).map(|entry|json!({"value":entry.id,"label":format!("{} · {}",entry.size,entry.standard_reference)})).collect::<Vec<_>>())),
 field(w,"material","Engineering material",json!(stored(w,"material").unwrap_or(&w.session.visible().settings.default_new_part_material)),json!(w.session.visible().materials.values().map(|material|json!({"value":material.id,"label":material.name})).collect::<Vec<_>>())),
 field(w,"finish","Surface finish",json!(stored(w,"finish").unwrap_or(cad_core::appearance::DEFAULT_FINISH)),json!(w.session.visible().finishes.values().map(|finish|json!({"value":finish.id,"label":finish.name})).collect::<Vec<_>>())),
 ];
    if family.has_length() {
        let mut length = field(
            w,
            "length",
            "Under-head length",
            json!(stored(w, "length").unwrap_or("20 mm")),
            json!([]),
        );
        length.data["unit"] = json!("mm / unit expression");
        length.data["value_type"] = "quantity-expression".into();
        elements.push(length);
        elements.push(field(w,"length-preset","Common modeling length",Value::Null,json!([6,8,10,12,16,20,25,30,35,40,50,60,80,100].into_iter().map(|length|json!({"value":format!("{length} mm"),"label":format!("{length} mm")})).collect::<Vec<_>>())));
    }
    for axis in 0..3 {
        let label = ["X", "Y", "Z"][axis];
        let name = format!("translation-{axis}");
        let mut element = field(
            w,
            &name,
            &format!("{label} position"),
            json!(stored(w, &name).unwrap_or("0 mm")),
            json!([]),
        );
        element.data["unit"] = "mm / expression".into();
        elements.push(element);
        let name = format!("rotation-{axis}");
        let mut element = field(
            w,
            &name,
            &format!("{label} rotation"),
            json!(stored(w, &name).unwrap_or("0 deg")),
            json!([]),
        );
        element.data["unit"] = "deg / expression".into();
        elements.push(element);
    }
    for (name, label, value) in [
        (
            "fixed",
            "Fix this occurrence",
            stored(w, "fixed") == Some("true"),
        ),
        (
            "reuse",
            "Reuse an identical unchanged definition",
            stored(w, "reuse").is_none_or(|value| value == "true"),
        ),
    ] {
        let mut element = field(w, name, label, json!(value), json!([]));
        element.data["value_type"] = "boolean".into();
        elements.push(element);
    }
    if let Some(entry) = entry {
        let sources = catalog
            .sources
            .iter()
            .filter(|source| entry.sources.contains(&source.id))
            .cloned()
            .collect::<Vec<_>>();
        elements.push(SemanticElement{id:"standard-parts:dimensions".into(),role:"standard-part-dimensions".into(),label:"Sourced dimensional envelope".into(),value:Some(entry.standard_reference.clone()),state:"sourced".into(),commands:vec![],relationships:vec!["workspace.standard-parts".into()],data:json!({"entry":entry,"sources":sources,"units":"mm","supplier_inventory":false,"certification":false,"source_dimensions_only":true})});
    }
    elements.push(SemanticElement{id:"standard-parts:simplification".into(),role:"standard-part-help".into(),label:if family.has_length(){"Smooth shank envelope. Threads, runout, tip and edge blends are omitted; socket geometry has a flat bottom. Under-head length runs in −Z; the head bearing plane is Z=0."}else if family==Family::HexNut{"Smooth nominal-thread-diameter bore. Thread teeth and face chamfers are omitted; bottom bearing plane is Z=0."}else{"Form A annular washer from sourced inner/outer diameters and thickness; bottom bearing plane is Z=0."}.into(),value:None,state:"simplified".into(),commands:vec![],relationships:vec!["workspace.standard-parts".into()],data:json!({"mass_basis":"evaluated-simplified-geometry","material_finish_independent":true,"certification":false})});
    elements.push(SemanticElement {
        id: "command:standard-parts.preview".into(),
        role: "standard-part-action".into(),
        label: "Refresh hardware preview".into(),
        value: None,
        state: if available(w) { "ready" } else { "disabled" }.into(),
        commands: vec!["standard-parts.preview".into()],
        relationships: vec!["workspace.standard-parts".into()],
        data: json!({"enabled":available(w)}),
    });
    elements.extend(library_actions());
    elements
}
pub fn value(w: &Workspace, element: &SemanticElement) -> Option<Value> {
    let name = element.data.get("field")?.as_str()?;
    match name {
        "owner" => Some(json!(owner(w).unwrap_or_default())),
        "family" => Some(json!(family(w))),
        "entry" => Some(json!(entry(w)?.id)),
        "fixed" => Some(json!(stored(w, name) == Some("true"))),
        "reuse" => Some(json!(stored(w, name).is_none_or(|value| value == "true"))),
        "length-preset" => Some(json!(stored(w, "length").unwrap_or("20 mm"))),
        "material" => Some(json!(
            stored(w, "material")
                .unwrap_or(&w.session.visible().settings.default_new_part_material)
        )),
        "finish" => Some(json!(
            stored(w, "finish").unwrap_or(cad_core::appearance::DEFAULT_FINISH)
        )),
        _ => Some(json!(stored(w, name).unwrap_or_else(|| {
            if name.starts_with("translation-") {
                "0 mm"
            } else if name.starts_with("rotation-") {
                "0 deg"
            } else {
                ""
            }
        }))),
    }
}
pub fn set_value(w: &mut Workspace, element: &SemanticElement, value: Value) -> Result<()> {
    if !available(w) {
        return Err(CadError::Invalid(
            "Finish the active operation before inserting hardware".into(),
        ));
    }
    if !w.fields.contains_key(TOOL) {
        begin(w);
        if !w.fields.contains_key(TOOL) {
            return Err(CadError::Invalid(w.status.clone()));
        }
    }
    let name = element
        .data
        .get("field")
        .and_then(Value::as_str)
        .ok_or_else(|| CadError::Invalid("The hardware field is missing".into()))?;
    match name {
        "family" => {
            let family: Family = serde_json::from_value(value)?;
            let entry = catalog()
                .entries
                .into_iter()
                .find(|entry| entry.shape.family() == family)
                .ok_or_else(|| {
                    CadError::Invalid("That hardware family has no sourced sizes".into())
                })?;
            put(w, "entry", entry.id);
        }
        "owner" => {
            let value = value
                .as_str()
                .ok_or_else(|| CadError::Invalid("Choose an assembly".into()))?;
            if !w.session.visible().assemblies.contains_key(value)
                || cad_core::library::is_definition_pinned(w.session.visible(), value)
            {
                return Err(CadError::Invalid(
                    "Choose an editable owning assembly".into(),
                ));
            }
            put(w, name, value);
        }
        "entry" => {
            let value = value
                .as_str()
                .ok_or_else(|| CadError::Invalid("Choose a sourced size".into()))?;
            if !catalog().entries.iter().any(|entry| entry.id == value) {
                return Err(CadError::Invalid(
                    "That sourced hardware size is missing".into(),
                ));
            }
            put(w, name, value);
        }
        "fixed" | "reuse" => put(
            w,
            name,
            value
                .as_bool()
                .ok_or_else(|| CadError::Invalid("Choose enabled or disabled".into()))?
                .to_string(),
        ),
        "material" | "finish" | "length" | "length-preset" | "translation-0" | "translation-1"
        | "translation-2" | "rotation-0" | "rotation-1" | "rotation-2" => {
            let value = value.as_str().ok_or_else(|| {
                CadError::Invalid("Enter a unit expression or choose a named definition".into())
            })?;
            if value.len() > 8192 {
                return Err(CadError::Invalid(
                    "The hardware field exceeds the supported text bound".into(),
                ));
            }
            put(
                w,
                if name == "length-preset" {
                    "length"
                } else {
                    name
                },
                value,
            );
        }
        _ => return Err(CadError::Invalid("Unknown hardware insertion field".into())),
    }
    if !family(w).has_length() {
        for field in ["standard-parts:length", "standard-parts:length-preset"] {
            w.fields.remove(&format!("{ERRORS}{field}"));
        }
    }
    match refresh(w) {
        Ok(()) => Ok(()),
        Err(error) => {
            w.fields.insert(ERROR.into(), error.to_string());
            Err(error)
        }
    }
}
pub fn show(ui: &mut egui::Ui, w: &mut Workspace) {
    if let Some((state, notice)) = library_notice() {
        if w.fields.contains_key(TOOL) {
            cad_ui::confirmation(ui, w);
        }
        controls::label(
            ui,
            "standard-parts:library-notice",
            "standard-part-help",
            &[],
            &notice,
            state,
        );
        show_library_actions(ui, w);
        return;
    }
    if !w.fields.contains_key(TOOL) {
        begin(w);
        if !w.fields.contains_key(TOOL) {
            return;
        }
    }
    cad_ui::confirmation(ui, w);
    show_library_actions(ui, w);
    let elements = semantics(w);
    for element in &elements {
        if element.data.get("setter").and_then(Value::as_str) == Some("standard-parts") {
            crate::ui::semantic_field(ui, w, element);
        }
    }
    for element in &elements {
        if element.role == "standard-part-help" {
            controls::label(
                ui,
                &element.id,
                &element.role,
                &[],
                &element.label,
                &element.state,
            );
        }
    }
    if let Some(element) = elements
        .iter()
        .find(|element| element.role == "standard-part-dimensions")
    {
        if let Some(source) = element
            .data
            .get("entry")
            .and_then(|source| source.get("shape"))
            .and_then(Value::as_object)
        {
            for (key, value) in source {
                if let Some(value) = value.as_f64() {
                    controls::label(
                        ui,
                        &format!("standard-parts:dimension:{key}"),
                        "standard-part-dimension",
                        &[],
                        &format!(
                            "{}: {value} mm",
                            key.trim_end_matches("_mm").replace('_', " ")
                        ),
                        "sourced",
                    );
                }
            }
        }
        if let Some(sources) = element.data.get("sources").and_then(Value::as_array) {
            for source in sources {
                let title = source
                    .get("title")
                    .and_then(Value::as_str)
                    .unwrap_or("Supplier dimensions");
                controls::label(
                    ui,
                    &format!("standard-parts:source:{}", source["id"]),
                    "standard-part-source",
                    &[],
                    title,
                    "sourced",
                )
                .on_hover_text(format!(
                    "{}\n{}",
                    source["url"].as_str().unwrap_or_default(),
                    source["notes"].as_str().unwrap_or_default()
                ));
            }
        }
    }
    if controls::button(
        ui,
        "command:standard-parts.preview",
        "standard-part-action",
        &["semantic-control"],
        "Refresh hardware preview",
        available(w),
        false,
    )
    .clicked()
    {
        action(w, "standard-parts.preview", None);
    }
    if let Some(error) = w.fields.get(ERROR) {
        controls::label(ui, ERROR, "standard-part-error", &[], error, "invalid");
    }
    for (key, error) in w.fields.iter().filter(|(key, _)| key.starts_with(ERRORS)) {
        controls::label(ui, key, "standard-part-error", &[], error, "invalid");
    }
}
