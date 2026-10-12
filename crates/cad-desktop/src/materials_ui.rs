//! Engineering material selection and editing over the shared document draft.
//! Catalog records are copied into a project only when explicitly added or assigned.
use crate::{Workspace, cad_ui, presentation_controls as controls};
use bevy_egui::egui;
use cad_core::{CadError, Id, Material, Result, commands::Command, presentation::SemanticElement};
use serde_json::{Value, json};

const TOOL: &str = "material-tool";
const SELECTED: &str = "material-selected";
const TARGET: &str = "material-target";
const CONFIGURATION: &str = "material-configuration";
const SOURCE: &str = "material-source";
const ERROR_PREFIX: &str = "material-field-invalid:";

fn available(w: &Workspace) -> bool {
    w.sketch.is_none() && (w.tool_checkpoint.is_none() || w.fields.contains_key(TOOL))
}
fn target(w: &Workspace) -> Option<Id> {
    w.fields
        .get(TARGET)
        .or(w.view.active_part.as_ref())
        .filter(|id| w.session.visible().parts.contains_key(*id))
        .cloned()
}
fn configuration(w: &Workspace) -> Option<Id> {
    let owner = target(w)?;
    w.fields
        .get(CONFIGURATION)
        .filter(|id| cad_core::configuration::exists(w.session.visible(), &owner, id))
        .cloned()
}
fn assigned(w: &Workspace) -> Option<Id> {
    cad_core::configuration::material(
        w.session.visible(),
        &target(w)?,
        configuration(w).as_deref(),
    )
    .ok()
}
fn catalog() -> Vec<Material> {
    cad_core::material_library::bundled_catalog()
        .map(|catalog| catalog.materials)
        .unwrap_or_default()
}
fn key(w: &Workspace) -> Option<String> {
    w.fields
        .get(SELECTED)
        .cloned()
        .or_else(|| assigned(w).map(|id| format!("project:{id}")))
        .or_else(|| {
            w.session
                .visible()
                .materials
                .keys()
                .next()
                .map(|id| format!("project:{id}"))
        })
}
fn selected(w: &Workspace) -> Option<Material> {
    let key = key(w)?;
    if let Some(id) = key.strip_prefix("project:") {
        w.session.visible().materials.get(id).cloned()
    } else {
        let id = key.strip_prefix("catalog:")?;
        catalog().into_iter().find(|material| material.id == id)
    }
}
fn editable(w: &Workspace) -> bool {
    key(w).is_some_and(|key| key.starts_with("project:")) && available(w)
}
fn choose(w: &mut Workspace, key: String) {
    w.fields.insert(SELECTED.into(), key);
    w.fields.remove(SOURCE);
    w.fields.retain(|key, _| {
        !key.starts_with(ERROR_PREFIX) && !key.starts_with("presentation-field:material:")
    });
}
pub fn begin(w: &mut Workspace) {
    if !available(w) {
        w.status =
            "Apply or cancel the current operation before editing engineering materials".into();
        return;
    }
    w.start_tool();
    w.fields.insert(TOOL.into(), "true".into());
    if !w.fields.contains_key(TARGET)
        && let Some(id) = target(w)
    {
        w.fields.insert(TARGET.into(), id.clone());
        if let Some(configuration) = w.session.visible().active_configurations.get(&id) {
            w.fields.insert(CONFIGURATION.into(), configuration.clone());
        }
    }
    if !w.fields.contains_key(SELECTED)
        && let Some(key) = key(w)
    {
        choose(w, key);
    }
    w.open_editor("materials");
}
pub fn cancel(w: &mut Workspace) {
    w.fields.retain(|key, _| {
        !key.starts_with("material-")
            && !key.starts_with(ERROR_PREFIX)
            && !key.starts_with("presentation-field:material:")
    });
}
pub fn current_source_is_valid(w: &Workspace) -> bool {
    !w.fields.keys().any(|key| key.starts_with(ERROR_PREFIX))
        && w.fields.get(SOURCE).is_none_or(|source| {
            serde_json::from_str::<Material>(source)
                .is_ok_and(|material| editable(w) && selected(w).as_ref() == Some(&material))
        })
}
pub fn field_is_invalid(w: &Workspace, element: &SemanticElement) -> bool {
    w.fields
        .contains_key(&format!("{ERROR_PREFIX}{}", element.id))
}
pub fn field_error(w: &mut Workspace, element: &SemanticElement, error: Option<String>) {
    if !matches!(
        element.data.get("setter").and_then(Value::as_str),
        Some("material-editor" | "material-ui")
    ) {
        return;
    }
    let key = format!("{ERROR_PREFIX}{}", element.id);
    if let Some(error) = error {
        w.fields.insert(key, error);
    } else {
        w.fields.remove(&key);
    }
}
fn replace(w: &mut Workspace, document: cad_core::Document) -> Result<()> {
    document.validate()?;
    let expected = document.fingerprint();
    w.dispatch(Command::ReplaceDocument { document });
    if w.session.visible().fingerprint() != expected {
        return Err(CadError::Invalid(w.status.clone()));
    }
    Ok(())
}
fn put(w: &mut Workspace, material: Material) -> Result<()> {
    let mut document = w.session.visible().clone();
    cad_core::materials::put(&mut document, material)?;
    replace(w, document)
}
fn recent(w: &mut Workspace, material: &Material) {
    let key = preference_key(material);
    w.view.library_recent.retain(|existing| existing != &key);
    w.view.library_recent.insert(0, key);
    w.view.library_recent.truncate(64);
    crate::settings::save_live_application(w);
}
fn preference_key(material: &Material) -> String {
    let source = serde_json::to_value(material).unwrap_or_default();
    let id = source
        .pointer("/metadata/catalog_id")
        .and_then(Value::as_str)
        .unwrap_or(&material.id);
    format!("material:{id}")
}
fn add_catalog(w: &mut Workspace) -> Result<Material> {
    let source = selected(w).ok_or_else(|| CadError::Invalid("Choose a material".into()))?;
    if key(w).is_some_and(|key| key.starts_with("project:")) {
        return Ok(source);
    }
    let material = cad_core::material_library::instantiate(&source.id)?;
    Ok(material)
}
fn assign(w: &mut Workspace, inherit: bool) -> Result<()> {
    let owner = target(w).ok_or_else(|| CadError::Invalid("Choose a target part".into()))?;
    let config = configuration(w);
    let mut document = w.session.visible().clone();
    let material = if inherit { None } else { Some(add_catalog(w)?) };
    if let Some(material) = &material {
        cad_core::materials::put(&mut document, material.clone())?;
    }
    if let Some(config) = config {
        let mut source = cad_core::configuration::definitions(&document, &owner)
            .remove(&config)
            .ok_or_else(|| CadError::Invalid("The target configuration no longer exists".into()))?;
        source.overrides.material = material.as_ref().map(|material| material.id.clone());
        cad_core::configuration::put(&mut document, &owner, source)?;
    } else {
        let material = material.as_ref().ok_or_else(|| {
            CadError::Invalid("Only a named configuration can inherit its material".into())
        })?;
        cad_core::materials::assign(&mut document, &owner, &material.id)?;
    }
    replace(w, document)?;
    if let Some(material) = material {
        recent(w, &material);
        choose(w, format!("project:{}", material.id));
    }
    Ok(())
}
fn import(w: &mut Workspace) -> Result<()> {
    let Some(path) = rfd::FileDialog::new()
        .set_title("Import an engineering material library")
        .add_filter("TCAD material library", &["tcadmaterials", "json"])
        .pick_file()
    else {
        return Ok(());
    };
    let metadata = std::fs::metadata(&path)?;
    if metadata.len() > 16 * 1024 * 1024 {
        return Err(CadError::Invalid(
            "Material library files are limited to 16 MiB".into(),
        ));
    }
    let source = std::fs::read_to_string(path)?;
    let catalog = cad_core::material_library::parse_json(&source)?;
    let mut document = w.session.visible().clone();
    // Import is additive. Existing assigned records and IDs remain untouched.
    let plan = cad_core::material_library::plan_import(
        &document,
        &catalog,
        cad_core::material_library::CollisionPolicy::NewIdentity,
    )?;
    let first = plan.materials.first().map(|material| material.id.clone());
    cad_core::material_library::apply_import(&mut document, plan)?;
    replace(w, document)?;
    if let Some(id) = first {
        choose(w, format!("project:{id}"));
    }
    Ok(())
}
fn export(w: &mut Workspace, all: bool) -> Result<()> {
    let materials = if all {
        w.session.visible().materials.values().cloned().collect()
    } else {
        vec![selected(w).ok_or_else(|| CadError::Invalid("Choose a material to export".into()))?]
    };
    let catalog = cad_core::material_library::Catalog::new("TCAD project materials", materials);
    let source = cad_core::material_library::export_json(&catalog)?;
    let Some(path) = rfd::FileDialog::new()
        .set_title("Export engineering materials")
        .set_file_name("Materials.tcadmaterials")
        .add_filter("TCAD material library", &["tcadmaterials"])
        .save_file()
    else {
        return Ok(());
    };
    cad_core::persistence::atomic_write(&path, source.as_bytes())?;
    w.status = format!(
        "Exported engineering material library to {}",
        path.display()
    );
    Ok(())
}
pub fn action(w: &mut Workspace, command: &str, arguments: Option<&Value>) -> bool {
    if command == "materials" {
        begin(w);
        return true;
    }
    if !command.starts_with("material.") {
        return false;
    }
    if command == "material.select" {
        if let Some(key) = arguments
            .and_then(|value| value.get("key"))
            .and_then(Value::as_str)
        {
            choose(w, key.into());
        }
        return true;
    }
    if command == "material.favorite" {
        if let Some(material) = selected(w) {
            let key = preference_key(&material);
            if w.view.library_favorites.contains(&key) {
                w.view.library_favorites.retain(|id| id != &key);
            } else {
                w.view.library_favorites.push(key);
            }
            crate::settings::save_live_application(w);
        }
        return true;
    }
    if command == "material.section" {
        if let Some(section) = arguments
            .and_then(|value| value.get("section"))
            .and_then(Value::as_str)
        {
            let field = format!("material-section:{section}");
            let open = section_open(w, section);
            w.fields.insert(field, (!open).to_string());
        }
        return true;
    }
    if !available(w) {
        w.status = "Apply or cancel the current operation before editing materials".into();
        return true;
    }
    if !w.fields.contains_key(TOOL) {
        begin(w);
    }
    let result = (|| -> Result<()> {
        match command {
            "material.add" => {
                let material = add_catalog(w)?;
                put(w, material.clone())?;
                recent(w, &material);
                choose(w, format!("project:{}", material.id));
            }
            "material.assign" => assign(w, false)?,
            "material.use-for-new-parts" => {
                let material = selected(w).filter(|_| editable(w)).ok_or_else(|| {
                    CadError::Invalid(
                        "Add this material to the project before using it as the new-part default"
                            .into(),
                    )
                })?;
                let mut settings = w.session.visible().settings.clone();
                settings.default_new_part_material = material.id.clone();
                w.dispatch(Command::SetModelSettings { settings });
                if w.session.visible().settings.default_new_part_material != material.id {
                    return Err(CadError::Invalid(w.status.clone()));
                }
            }
            "material.inherit" => assign(w, true)?,
            "material.new" | "material.duplicate" => {
                let material = if command == "material.new" {
                    cad_core::materials::custom("Custom material", 1000.)
                } else {
                    let source = selected(w).ok_or_else(|| {
                        CadError::Invalid("Choose a material to duplicate".into())
                    })?;
                    cad_core::materials::duplicate(&source, format!("{} copy", source.name))
                };
                put(w, material.clone())?;
                choose(w, format!("project:{}", material.id));
            }
            "material.delete" => {
                let material = selected(w)
                    .filter(|_| editable(w))
                    .ok_or_else(|| CadError::Invalid("Select a project material".into()))?;
                let mut document = w.session.visible().clone();
                cad_core::material_library::delete_material(&mut document, &material.id)?;
                replace(w, document)?;
                w.fields.remove(SELECTED);
                w.fields.remove(SOURCE);
            }
            "material.property.add" | "material.property.remove" => {
                let mut material = selected(w)
                    .filter(|_| editable(w))
                    .ok_or_else(|| CadError::Invalid("Choose a project material".into()))?;
                let name = arguments
                    .and_then(|value| value.get("property"))
                    .and_then(Value::as_str)
                    .or_else(|| w.fields.get("material-property").map(String::as_str))
                    .ok_or_else(|| CadError::Invalid("Choose a property".into()))?;
                let property = parse_property(name)?;
                let value = if command == "material.property.remove" {
                    None
                } else {
                    let text = w
                        .fields
                        .get("material-property-value")
                        .map(String::as_str)
                        .unwrap_or_default();
                    let value = text.parse::<f64>().map_err(|_| {
                        CadError::Invalid(
                            "Enter a finite property value in the displayed unit".into(),
                        )
                    })? * property_presentation(property).1;
                    if !property.valid_value(value) {
                        return Err(CadError::Invalid(format!(
                            "Invalid {} value",
                            property.label()
                        )));
                    }
                    Some(cad_core::materials::PropertyValue::user_defined(value))
                };
                material.properties.set(property, value);
                put(w, material)?;
                w.fields.remove("material-property-value");
            }
            "material.import" => import(w)?,
            "material.export" => export(w, false)?,
            "material.export-project" => export(w, true)?,
            _ => {
                return Err(CadError::Invalid(format!(
                    "Unknown material action: {command}"
                )));
            }
        }
        Ok(())
    })();
    if let Err(error) = result {
        w.status = error.to_string();
    }
    true
}

fn field(
    id: impl Into<String>,
    label: &str,
    setter: &str,
    pointer: &str,
    value: Value,
    choices: Value,
    display: (Option<&str>, bool),
) -> SemanticElement {
    let (unit, editable) = display;
    SemanticElement {
        id: id.into(),
        role: "material-field".into(),
        label: label.into(),
        value: Some(
            value
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| value.to_string()),
        ),
        state: if editable { "draft" } else { "readonly" }.into(),
        commands: vec![],
        relationships: vec!["workspace.material-library".into()],
        data: json!({"setter":setter,"field":pointer,"json":value,"choices":choices,"unit":unit,"editable":editable,"classes":["modeling-field"],"accessibility_label":label}),
    }
}
fn view_field(
    w: &Workspace,
    name: &str,
    label: &str,
    default: &str,
    choices: Value,
) -> SemanticElement {
    field(
        format!("material:browser:{name}"),
        label,
        "material-ui",
        name,
        json!(
            w.fields
                .get(&format!("material-{name}"))
                .map(String::as_str)
                .unwrap_or(default)
        ),
        choices,
        (None, true),
    )
}
fn property_fields(w: &Workspace) -> Vec<SemanticElement> {
    use cad_core::materials::{Family, PropertyKey};
    let Some(material) = selected(w) else {
        return vec![];
    };
    let edit = editable(w);
    let prefix = format!("material:{}", material.id);
    let metadata = material.metadata.clone().unwrap_or_default();
    let mut out = vec![
        field(
            format!("{prefix}:name"),
            "Name",
            "material-editor",
            "/name",
            json!(material.name),
            json!([]),
            (None, edit),
        ),
        field(
            format!("{prefix}:density"),
            "Density",
            "material-editor",
            "/density_kg_m3",
            json!(material.density_kg_m3),
            json!([]),
            (Some("kg/m³"), edit),
        ),
        field(
            format!("{prefix}:family"),
            "Category",
            "material-editor",
            "/metadata/family",
            json!(metadata.family),
            json!(
                Family::ALL
                    .into_iter()
                    .map(|family| json!({"value":family,"label":family.label()}))
                    .collect::<Vec<_>>()
            ),
            (None, edit),
        ),
    ];
    for (name, label, value) in [
        ("grade", "Grade / designation", metadata.grade),
        ("condition", "Condition / temper", metadata.condition),
        ("description", "Description", metadata.description),
    ] {
        out.push(field(
            format!("{prefix}:metadata:{name}"),
            label,
            "material-editor",
            &format!("/metadata/{name}"),
            json!(value),
            json!([]),
            (None, edit),
        ));
    }
    out.push(field(
        format!("{prefix}:metadata:tags"),
        "Tags · comma separated",
        "material-editor",
        "/metadata/tags",
        json!(metadata.tags.join(", ")),
        json!([]),
        (None, edit),
    ));
    for property_key in PropertyKey::all() {
        let Some(property) = material.properties.get(*property_key) else {
            continue;
        };
        let name = property_key.key();
        let (unit, scale) = property_presentation(*property_key);
        let mut element = field(
            format!("{prefix}:property:{name}:value"),
            property_key.label(),
            "material-editor",
            &format!("/properties/{name}/value"),
            json!(property.value / scale),
            json!([]),
            (Some(unit), edit),
        );
        element.data["scale"] = json!(scale);
        element.data["property"] = json!(name);
        out.push(element);
        for (detail, label, value) in [
            (
                "conditions",
                "Condition",
                json!(property.provenance.conditions),
            ),
            (
                "reference",
                "Reference",
                json!(property.provenance.reference),
            ),
        ] {
            let mut element = field(
                format!("{prefix}:property:{name}:{detail}"),
                label,
                "material-editor",
                &format!("/properties/{name}/provenance/{detail}"),
                value,
                json!([]),
                (None, edit),
            );
            element.data["property"] = json!(name);
            element.data["property_detail"] = json!(true);
            out.push(element);
        }
        let choices = std::iter::once(json!({"value":"","label":"User defined / no citation"}))
            .chain(
                material
                    .metadata
                    .as_ref()
                    .into_iter()
                    .flat_map(|metadata| metadata.sources.iter())
                    .map(|source| json!({"value":source.id,"label":source.title})),
            )
            .collect::<Vec<_>>();
        let mut element = field(
            format!("{prefix}:property:{name}:source"),
            "Source citation",
            "material-editor",
            &format!("/properties/{name}/provenance/source"),
            json!(property.provenance.source),
            json!(choices),
            (None, edit),
        );
        element.data["property"] = json!(name);
        element.data["property_detail"] = json!(true);
        out.push(element);
        let mut element = field(
            format!("{prefix}:property:{name}:basis"),
            "Value basis",
            "material-editor",
            &format!("/properties/{name}/provenance/basis"),
            json!(property.provenance.basis),
            json!([{"value":"user_defined","label":"User defined"},{"value":"typical","label":"Typical"},{"value":"minimum","label":"Minimum"},{"value":"maximum","label":"Maximum"}]),
            (None, edit),
        );
        element.data["property"] = json!(name);
        element.data["property_detail"] = json!(true);
        out.push(element);
    }
    out
}
fn property_presentation(key: cad_core::materials::PropertyKey) -> (&'static str, f64) {
    use cad_core::materials::PropertyKey;
    match key {
        PropertyKey::YoungsModulusPa | PropertyKey::ShearModulusPa => ("GPa", 1e9),
        PropertyKey::YieldStrengthPa | PropertyKey::TensileStrengthPa => ("MPa", 1e6),
        PropertyKey::ElongationRatio => ("%", 0.01),
        _ => (key.unit(), 1.),
    }
}
fn parse_property(key: &str) -> Result<cad_core::materials::PropertyKey> {
    cad_core::materials::PropertyKey::all()
        .iter()
        .copied()
        .find(|property| property.key() == key)
        .ok_or_else(|| CadError::Invalid("Unknown engineering property".into()))
}
pub fn current_value(w: &Workspace, element: &SemanticElement) -> Option<Value> {
    let field = element.data.get("field")?.as_str()?;
    match element.data.get("setter")?.as_str()? {
        "material-ui" => match field {
            "target" => Some(json!(target(w))),
            "configuration" => Some(json!(configuration(w).unwrap_or_default())),
            "selected" => Some(json!(key(w))),
            "source" => Some(json!(
                w.fields
                    .get(SOURCE)
                    .cloned()
                    .or_else(|| selected(w)
                        .and_then(|material| serde_json::to_string_pretty(&material).ok()))
                    .unwrap_or_default()
            )),
            _ => Some(json!(
                w.fields
                    .get(&format!("material-{field}"))
                    .cloned()
                    .unwrap_or_default()
            )),
        },
        "material-editor" => {
            let material = selected(w)?;
            if field == "/metadata/tags" {
                return Some(json!(
                    material
                        .metadata
                        .as_ref()
                        .map(|metadata| metadata.tags.join(", "))
                        .unwrap_or_default()
                ));
            }
            let mut source = serde_json::to_value(&material).ok()?;
            if field.starts_with("/metadata/") {
                source["metadata"] =
                    serde_json::to_value(material.metadata.unwrap_or_default()).ok()?;
            }
            let value = source
                .pointer(field)
                .cloned()
                .unwrap_or_else(|| element.data.get("json").cloned().unwrap_or(Value::Null));
            if let Some(scale) = element.data.get("scale").and_then(Value::as_f64) {
                Some(json!(value.as_f64()? / scale))
            } else {
                Some(value)
            }
        }
        _ => None,
    }
}
pub fn set_value(w: &mut Workspace, element: &SemanticElement, value: Value) -> Result<()> {
    let pointer = element
        .data
        .get("field")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if element.data.get("setter").and_then(Value::as_str) == Some("material-ui") {
        let text = value.as_str().ok_or_else(|| {
            CadError::Invalid("Material browser fields require text or a named choice".into())
        })?;
        match pointer {
            "target" => {
                if !w.session.visible().parts.contains_key(text) {
                    return Err(CadError::Invalid("Choose a target part".into()));
                }
                w.fields.insert(TARGET.into(), text.into());
                w.fields.remove(CONFIGURATION);
                if let Some(id) = assigned(w) {
                    choose(w, format!("project:{id}"));
                }
            }
            "configuration" => {
                if text.is_empty() {
                    w.fields.remove(CONFIGURATION);
                } else if target(w).is_some_and(|owner| {
                    cad_core::configuration::exists(w.session.visible(), &owner, text)
                }) {
                    w.fields.insert(CONFIGURATION.into(), text.into());
                } else {
                    return Err(CadError::Invalid(
                        "Choose a configuration of the target part".into(),
                    ));
                }
                if let Some(id) = assigned(w) {
                    choose(w, format!("project:{id}"));
                }
            }
            "selected" => choose(w, text.into()),
            "source" => {
                if !editable(w) {
                    return Err(CadError::Invalid(
                        "Add a catalog material to the project before editing it".into(),
                    ));
                }
                w.fields.insert(SOURCE.into(), text.into());
                let material: Material = serde_json::from_str(text)?;
                if selected(w).is_none_or(|current| current.id != material.id) {
                    return Err(CadError::Invalid(
                        "Material identity cannot change in its editor".into(),
                    ));
                }
                put(w, material)?;
            }
            "search" | "category" | "collection" | "property" | "property-value" => {
                w.fields.insert(format!("material-{pointer}"), text.into());
            }
            _ => return Err(CadError::Invalid("Unknown material browser field".into())),
        }
        return Ok(());
    }
    if !editable(w) {
        return Err(CadError::Invalid(
            "Add a catalog material to the project before editing it".into(),
        ));
    }
    if !w.fields.contains_key(TOOL) {
        begin(w);
    }
    let mut material =
        selected(w).ok_or_else(|| CadError::Invalid("Choose a project material".into()))?;
    if pointer == "/density_kg_m3" {
        let density = value
            .as_f64()
            .ok_or_else(|| CadError::Invalid("Enter a density in kg/m³".into()))?;
        if density != material.density_kg_m3 {
            material.density_kg_m3 = density;
            material.metadata.get_or_insert_default().density_provenance = Some(Default::default());
        }
    } else if let Some(name) = element.data.get("property").and_then(Value::as_str) {
        let property = parse_property(name)?;
        if pointer.ends_with("/value") {
            let scale = element
                .data
                .get("scale")
                .and_then(Value::as_f64)
                .unwrap_or(1.);
            let canonical = value
                .as_f64()
                .ok_or_else(|| CadError::Invalid("Enter a finite property value".into()))?
                * scale;
            if material
                .properties
                .get(property)
                .is_none_or(|previous| previous.value != canonical)
            {
                material.properties.set(
                    property,
                    Some(cad_core::materials::PropertyValue::user_defined(canonical)),
                );
            }
        } else {
            let mut property_value = material
                .properties
                .get(property)
                .cloned()
                .ok_or_else(|| CadError::Invalid("The property no longer exists".into()))?;
            let detail = pointer.rsplit('/').next().unwrap_or_default();
            match detail {
                "conditions" => {
                    property_value.provenance.conditions = value.as_str().unwrap_or_default().into()
                }
                "reference" => {
                    property_value.provenance.reference = value.as_str().unwrap_or_default().into()
                }
                "source" => {
                    property_value.provenance.source = value.as_str().unwrap_or_default().into();
                    if property_value.provenance.source.is_empty() {
                        property_value.provenance.basis =
                            cad_core::materials::PropertyBasis::UserDefined;
                    }
                }
                "basis" => property_value.provenance.basis = serde_json::from_value(value)?,
                _ => {
                    return Err(CadError::Invalid(
                        "Unknown property provenance field".into(),
                    ));
                }
            }
            material.properties.set(property, Some(property_value));
        }
    } else if let Some(detail) = pointer.strip_prefix("/metadata/") {
        let metadata = material.metadata.get_or_insert_default();
        match detail {
            "family" => metadata.family = serde_json::from_value(value)?,
            "grade" => metadata.grade = value.as_str().unwrap_or_default().into(),
            "condition" => metadata.condition = value.as_str().unwrap_or_default().into(),
            "description" => metadata.description = value.as_str().unwrap_or_default().into(),
            "tags" => {
                metadata.tags = value
                    .as_str()
                    .unwrap_or_default()
                    .split(',')
                    .map(str::trim)
                    .filter(|tag| !tag.is_empty())
                    .map(str::to_owned)
                    .collect()
            }
            _ => return Err(CadError::Invalid("Unknown material metadata field".into())),
        }
    } else if pointer == "/name" {
        material.name = value
            .as_str()
            .ok_or_else(|| CadError::Invalid("Material name must be text".into()))?
            .into();
    } else {
        return Err(CadError::Invalid("Unknown material field".into()));
    }
    put(w, material.clone())?;
    if w.fields.contains_key(SOURCE) {
        w.fields
            .insert(SOURCE.into(), serde_json::to_string_pretty(&material)?);
    }
    Ok(())
}

fn matches(w: &Workspace, material: &Material) -> bool {
    let source = serde_json::to_value(material).unwrap_or_default();
    let query = w
        .fields
        .get("material-search")
        .map(|value| value.to_lowercase())
        .unwrap_or_default();
    let category = w
        .fields
        .get("material-category")
        .map(String::as_str)
        .unwrap_or_default();
    let collection = w
        .fields
        .get("material-collection")
        .map(String::as_str)
        .unwrap_or_default();
    (query.is_empty() || source.to_string().to_lowercase().contains(&query))
        && (category.is_empty()
            || source.pointer("/metadata/family").and_then(Value::as_str) == Some(category))
        && (collection != "favorites"
            || w.view.library_favorites.contains(&preference_key(material)))
        && (collection != "recent" || w.view.library_recent.contains(&preference_key(material)))
}
fn entry(material: &Material, catalog: bool, selected: bool, favorite: bool) -> SemanticElement {
    let key = format!(
        "{}:{}",
        if catalog { "catalog" } else { "project" },
        material.id
    );
    SemanticElement {
        id: format!("material:entry:{key}"),
        role: "material-library-entry".into(),
        label: material.name.clone(),
        value: Some(format!("{} kg/m³", material.density_kg_m3)),
        state: if selected { "selected" } else { "ready" }.into(),
        commands: vec!["material.select".into()],
        relationships: vec!["workspace.material-library".into()],
        data: json!({"key":key,"material":material,"catalog":catalog,"favorite":favorite,"engineering_material":true,"visual_finish_independent":true,"accessibility_label":material.name}),
    }
}
fn browser_fields(w: &Workspace) -> Vec<SemanticElement> {
    let mut categories = std::collections::BTreeSet::new();
    for material in catalog()
        .iter()
        .chain(w.session.visible().materials.values())
    {
        if let Some(category) = serde_json::to_value(material).ok().and_then(|source| {
            source
                .pointer("/metadata/family")
                .and_then(Value::as_str)
                .map(str::to_owned)
        }) {
            categories.insert(category);
        }
    }
    let choices = std::iter::once(json!({"value":"","label":"All categories"}))
        .chain(
            categories
                .into_iter()
                .map(|category| json!({"label":category.replace('_'," "),"value":category})),
        )
        .collect::<Vec<_>>();
    vec![
        view_field(w, "search", "Search grades and tags", "", json!([])),
        view_field(w, "category", "Category", "", json!(choices)),
        view_field(
            w,
            "collection",
            "Collection",
            "",
            json!([{"value":"","label":"Project and catalog"},{"value":"project","label":"Project only"},{"value":"catalog","label":"Catalog only"},{"value":"favorites","label":"Favorites"},{"value":"recent","label":"Recently assigned"}]),
        ),
    ]
}
fn target_fields(w: &Workspace) -> Vec<SemanticElement> {
    let configs = target(w)
        .map(|owner| cad_core::configuration::names(w.session.visible(), &owner))
        .unwrap_or_default();
    vec![
        field(
            "material:target",
            "Part",
            "material-ui",
            "target",
            json!(target(w)),
            json!(
                w.session
                    .visible()
                    .parts
                    .values()
                    .map(|part| json!({"value":part.id,"label":part.name}))
                    .collect::<Vec<_>>()
            ),
            (None, available(w)),
        ),
        field(
            "material:configuration",
            "Assignment scope",
            "material-ui",
            "configuration",
            json!(configuration(w).unwrap_or_default()),
            json!(
                std::iter::once(json!({"value":"","label":"Base part definition"}))
                    .chain(configs.into_iter().map(
                        |(id, name)| json!({"value":id,"label":format!("Configuration · {name}")})
                    ))
                    .collect::<Vec<_>>()
            ),
            (None, available(w)),
        ),
    ]
}
fn action_element(command: &str, label: &str, enabled: bool, reason: &str) -> SemanticElement {
    SemanticElement {
        id: format!("command:{command}"),
        role: "material-action".into(),
        label: label.into(),
        value: None,
        state: if enabled { "ready" } else { "disabled" }.into(),
        commands: vec![command.into()],
        relationships: vec!["workspace.material-library".into()],
        data: json!({"enabled":enabled,"disabled_reason":if enabled {""}else{reason},"accessibility_label":label}),
    }
}
fn actions(w: &Workspace) -> Vec<SemanticElement> {
    let has = selected(w).is_some();
    let edit = editable(w);
    let delete_reason = selected(w).and_then(|material| {
        let mut document = w.session.visible().clone();
        cad_core::material_library::delete_material(&mut document, &material.id)
            .err()
            .map(|error| error.to_string())
    });
    let reason = delete_reason
        .as_deref()
        .unwrap_or("Select a project material");
    [
        (
            "material.assign",
            "Assign material",
            available(w) && has && target(w).is_some(),
            "Choose a material and a target part",
        ),
        (
            "material.inherit",
            "Use inherited material",
            available(w) && configuration(w).is_some(),
            "Choose a named configuration",
        ),
        (
            "material.add",
            "Add to project",
            available(w) && has && !edit,
            "Choose a catalog entry",
        ),
        (
            "material.new",
            "New material",
            available(w),
            "Finish the active operation first",
        ),
        (
            "material.duplicate",
            "Duplicate",
            available(w) && has,
            "Choose a material",
        ),
        (
            "material.delete",
            "Delete",
            edit && delete_reason.is_none(),
            reason,
        ),
        ("material.favorite", "Favorite", has, "Choose a material"),
        (
            "material.use-for-new-parts",
            "Use for new parts",
            edit,
            "Add a catalog material to the project first",
        ),
        (
            "material.import",
            "Import library…",
            available(w),
            "Finish the active operation first",
        ),
        (
            "material.export",
            "Export selected…",
            has,
            "Choose a material",
        ),
        (
            "material.export-project",
            "Export project library…",
            !w.session.visible().materials.is_empty(),
            "The project material library is empty",
        ),
    ]
    .into_iter()
    .map(|(command, label, enabled, reason)| action_element(command, label, enabled, reason))
    .collect()
}
pub fn semantics(w: &Workspace) -> Vec<SemanticElement> {
    let mut elements = vec![SemanticElement {
        id: "workspace.material-library".into(),
        role: "material-library".into(),
        label: "Engineering materials".into(),
        value: key(w),
        state: if w.fields.contains_key(TOOL) {
            "draft"
        } else {
            "ready"
        }
        .into(),
        commands: vec!["materials".into()],
        relationships: vec![],
        data: json!({"transaction":"shared-document-draft","independent_visual_finishes":true,"favorite_scope":"personal-presentation","target":target(w),"configuration":configuration(w)}),
    }];
    elements.extend(browser_fields(w));
    elements.extend(target_fields(w));
    elements.extend(actions(w));
    elements.extend(property_fields(w));
    elements.extend(new_property_fields(w));
    for (section, label) in [
        ("browser", "Material library"),
        ("properties", "Properties and conditions"),
        ("source", "Advanced source"),
    ] {
        elements.push(SemanticElement {
            id: format!("material:section:{section}"),
            role: "material-section".into(),
            label: label.into(),
            value: Some(section_open(w, section).to_string()),
            state: if section_open(w, section) {
                "expanded"
            } else {
                "collapsed"
            }
            .into(),
            commands: vec!["material.section".into()],
            relationships: vec!["workspace.material-library".into()],
            data: json!({"section":section,"accessibility_label":label}),
        });
    }
    if let Some(material) = selected(w) {
        for (property, value) in material.properties.iter() {
            let name = property.key();
            elements.push(SemanticElement{id:format!("material:section:property-{name}"),role:"material-property-section".into(),label:format!("{} · source and conditions",property.label()),value:Some(section_open(w,&format!("property-{name}")).to_string()),state:"ready".into(),commands:vec!["material.section".into()],relationships:vec![format!("material:{}:property:{name}:value",material.id)],data:json!({"section":format!("property-{name}"),"property":name,"provenance":value.provenance})});
            let mut remove = action_element(
                "material.property.remove",
                "Remove property",
                editable(w),
                "Choose a project material",
            );
            remove.id = format!("material:{}:property:{name}:remove", material.id);
            remove.data["property"] = json!(name);
            elements.push(remove);
        }
        elements.push(SemanticElement {
            id: format!("material:{}:usage", material.id),
            role: "material-usage".into(),
            label: "Assigned part and configuration definitions".into(),
            value: Some(
                cad_core::materials::usages(w.session.visible(), &material.id)
                    .len()
                    .to_string(),
            ),
            state: "ready".into(),
            commands: vec![],
            relationships: vec!["workspace.material-library".into()],
            data: json!({"usages":cad_core::materials::usages(w.session.visible(),&material.id)}),
        });
    }
    elements.push(action_element(
        "material.property.add",
        "Add property",
        editable(w),
        "Choose a project material and enter a valid property value",
    ));
    let key = key(w);
    for material in w.session.visible().materials.values() {
        elements.push(entry(
            material,
            false,
            key.as_deref() == Some(&format!("project:{}", material.id)),
            w.view.library_favorites.contains(&preference_key(material)),
        ));
    }
    for material in catalog() {
        elements.push(entry(
            &material,
            true,
            key.as_deref() == Some(&format!("catalog:{}", material.id)),
            w.view
                .library_favorites
                .contains(&preference_key(&material)),
        ));
    }
    if let Some(material) = selected(w) {
        let mut source = field(
            format!("material:{}:source", material.id),
            "Advanced source",
            "material-ui",
            "source",
            json!(w.fields.get(SOURCE).cloned().unwrap_or_else(|| {
                serde_json::to_string_pretty(&material).unwrap_or_default()
            })),
            json!([]),
            (None, editable(w)),
        );
        source.data["multiline"] = json!(true);
        elements.push(source);
        if let Some(metadata) = serde_json::to_value(&material)
            .ok()
            .and_then(|source| source.get("metadata").cloned())
        {
            elements.push(SemanticElement {
                id: format!("material:{}:provenance", material.id),
                role: "material-provenance".into(),
                label: "Property sources and conditions".into(),
                value: None,
                state: "ready".into(),
                commands: vec![],
                relationships: vec![format!("material:{}", material.id)],
                data: json!({"metadata":metadata,"explicit_units":true}),
            });
        }
    }
    elements
}
fn render_action(ui: &mut egui::Ui, w: &mut Workspace, element: &SemanticElement) {
    let enabled = element
        .data
        .get("enabled")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    if controls::button(
        ui,
        &element.id,
        &element.role,
        &["semantic-control"],
        &element.label,
        enabled,
        false,
    )
    .on_hover_text(
        element
            .data
            .get("disabled_reason")
            .and_then(Value::as_str)
            .unwrap_or_default(),
    )
    .clicked()
    {
        action(w, &element.commands[0], Some(&element.data));
    }
}
fn heading(ui: &mut egui::Ui, id: &str, label: &str) {
    controls::label(ui, id, "material-heading", &[], label, "ready");
}
fn section_open(w: &Workspace, id: &str) -> bool {
    w.fields
        .get(&format!("material-section:{id}"))
        .map(|value| value == "true")
        .unwrap_or(matches!(id, "browser" | "properties"))
}
fn section(
    ui: &mut egui::Ui,
    w: &mut Workspace,
    id: &str,
    label: &str,
    _default_open: bool,
    content: impl FnOnce(&mut egui::Ui, &mut Workspace),
) {
    let open = section_open(w, id);
    if controls::button(
        ui,
        &format!("material:section:{id}"),
        if id.starts_with("property-") {
            "material-property-section"
        } else {
            "material-section"
        },
        &["semantic-control"],
        &format!("{} {label}", if open { "▾" } else { "▸" }),
        true,
        open,
    )
    .clicked()
    {
        action(w, "material.section", Some(&json!({"section":id})));
    }
    if section_open(w, id) {
        content(ui, w);
    }
}
fn new_property_fields(w: &Workspace) -> Vec<SemanticElement> {
    use cad_core::materials::PropertyKey;
    let property = w
        .fields
        .get("material-property")
        .and_then(|value| parse_property(value).ok());
    let choices = PropertyKey::all()
        .iter()
        .filter(|property| {
            selected(w).is_none_or(|material| material.properties.get(**property).is_none())
        })
        .map(|property| json!({"value":property.key(),"label":property.label()}))
        .collect::<Vec<_>>();
    let mut value = view_field(w, "property-value", "Value", "", json!([]));
    value.data["unit"] = json!(
        property
            .map(|property| property_presentation(property).0)
            .unwrap_or("")
    );
    vec![
        view_field(
            w,
            "property",
            "Add engineering property",
            "",
            json!(choices),
        ),
        value,
    ]
}
pub fn show(ui: &mut egui::Ui, w: &mut Workspace) {
    if !available(w) {
        controls::label(
            ui,
            "material:busy",
            "material-description",
            &[],
            "Finish the current operation before editing materials",
            "disabled",
        );
        return;
    }
    if !w.fields.contains_key(TOOL) {
        begin(w);
    }
    cad_ui::confirmation(ui, w);
    controls::label(
        ui,
        "material:independence",
        "material-description",
        &[],
        "Engineering properties control mass and manufacturing data. Colors, textures and coatings are selected independently in Appearance.",
        "ready",
    );
    if controls::button(
        ui,
        "material:appearance",
        "material-action",
        &["semantic-control"],
        "Visual finishes…",
        true,
        false,
    )
    .clicked()
    {
        w.status =
            "Apply or cancel this material edit, then open Appearance to choose a finish".into();
    }
    for element in target_fields(w) {
        crate::ui::semantic_field(ui, w, &element);
    }
    let assigned_name = assigned(w)
        .and_then(|id| w.session.visible().materials.get(&id))
        .map(|material| material.name.as_str())
        .unwrap_or("None");
    controls::label(
        ui,
        "material:assigned",
        "material-assignment",
        &[],
        &format!("Assigned · {assigned_name}"),
        "ready",
    );
    let all_actions = actions(w);
    ui.horizontal_wrapped(|ui| {
        for element in all_actions.iter().take(3) {
            render_action(ui, w, element);
        }
    });
    section(ui, w, "browser", "Material library", true, |ui, w| {
        for element in browser_fields(w) {
            crate::ui::semantic_field(ui, w, &element);
        }
        let collection = w
            .fields
            .get("material-collection")
            .cloned()
            .unwrap_or_default();
        let key = key(w);
        let mut count = 0;
        let props = controls::properties("material:entries", "material-library-list", &[], "ready");
        egui::ScrollArea::vertical()
            .id_salt("material-library-entries")
            .max_height(controls::number(&props, "max-height", 220.))
            .show(ui, |ui| {
                for (is_catalog, materials) in [
                    (
                        false,
                        w.session
                            .visible()
                            .materials
                            .values()
                            .cloned()
                            .collect::<Vec<_>>(),
                    ),
                    (true, catalog()),
                ] {
                    if collection == "project" && is_catalog
                        || collection == "catalog" && !is_catalog
                    {
                        continue;
                    }
                    let materials = materials
                        .into_iter()
                        .filter(|material| matches(w, material))
                        .collect::<Vec<_>>();
                    if materials.is_empty() {
                        continue;
                    }
                    heading(
                        ui,
                        if is_catalog {
                            "material:catalog-heading"
                        } else {
                            "material:project-heading"
                        },
                        if is_catalog {
                            "Curated catalog"
                        } else {
                            "In this project"
                        },
                    );
                    for material in materials {
                        count += 1;
                        let entry = entry(
                            &material,
                            is_catalog,
                            key.as_deref()
                                == Some(&format!(
                                    "{}:{}",
                                    if is_catalog { "catalog" } else { "project" },
                                    material.id
                                )),
                            w.view
                                .library_favorites
                                .contains(&preference_key(&material)),
                        );
                        let favorite = entry.data["favorite"].as_bool().unwrap_or(false);
                        let response = controls::button(
                            ui,
                            &entry.id,
                            &entry.role,
                            &["semantic-control"],
                            &format!("{}{}", if favorite { "★ " } else { "" }, entry.label),
                            true,
                            entry.state == "selected",
                        )
                        .on_hover_text(format!("{} kg/m³", material.density_kg_m3));
                        if response.clicked() {
                            action(w, "material.select", Some(&entry.data));
                        }
                    }
                }
            });
        if count == 0 {
            controls::label(
                ui,
                "material:no-results",
                "material-description",
                &[],
                "No matching materials. Clear a filter or import a library.",
                "empty",
            );
        }
    });
    ui.horizontal_wrapped(|ui| {
        for element in all_actions.iter().skip(3) {
            render_action(ui, w, element);
        }
    });
    if let Some(material) = selected(w) {
        section(
            ui,
            w,
            "properties",
            "Properties and conditions",
            true,
            |ui, w| {
                if !editable(w) {
                    controls::label(
                        ui,
                        "material:catalog-readonly",
                        "material-description",
                        &[],
                        "Catalog records are read only. Add to project or duplicate to customize.",
                        "readonly",
                    );
                }
                for element in property_fields(w).into_iter().filter(|element| {
                    element.data.get("property_detail").and_then(Value::as_bool) != Some(true)
                }) {
                    crate::ui::semantic_field(ui, w, &element);
                    if let Some(name) = element.data.get("property").and_then(Value::as_str) {
                        let title = format!("{} · source and conditions", element.label);
                        section(
                            ui,
                            w,
                            &format!("property-{name}"),
                            &title,
                            false,
                            |ui, w| {
                                for detail in property_fields(w).into_iter().filter(|element| {
                                    element.data.get("property").and_then(Value::as_str)
                                        == Some(name)
                                        && element
                                            .data
                                            .get("property_detail")
                                            .and_then(Value::as_bool)
                                            == Some(true)
                                }) {
                                    crate::ui::semantic_field(ui, w, &detail);
                                }
                                let mut remove = action_element(
                                    "material.property.remove",
                                    "Remove property",
                                    editable(w),
                                    "Choose a project material",
                                );
                                remove.id =
                                    format!("material:{}:property:{name}:remove", material.id);
                                remove.data["property"] = json!(name);
                                render_action(ui, w, &remove);
                            },
                        );
                    }
                }
                if editable(w) {
                    for element in new_property_fields(w) {
                        crate::ui::semantic_field(ui, w, &element);
                    }
                    render_action(
                        ui,
                        w,
                        &action_element(
                            "material.property.add",
                            "Add property",
                            true,
                            "Choose a property and enter a valid value",
                        ),
                    );
                }
                if let Some(metadata) = serde_json::to_value(&material)
                    .ok()
                    .and_then(|source| source.get("metadata").cloned())
                    && let Some(sources) = metadata
                        .get("sources")
                        .or_else(|| metadata.get("source_refs"))
                        .and_then(Value::as_array)
                {
                    for (index, source) in sources.iter().enumerate() {
                        let title = source
                            .get("title")
                            .and_then(Value::as_str)
                            .unwrap_or("Source");
                        let url = source
                            .get("url")
                            .and_then(Value::as_str)
                            .unwrap_or_default();
                        let retrieved = source
                            .get("retrieved")
                            .and_then(Value::as_str)
                            .unwrap_or_default();
                        controls::label(
                            ui,
                            &format!("material:{}:source:{index}", material.id),
                            "material-provenance",
                            &[],
                            &format!("{title} · retrieved {retrieved}"),
                            "ready",
                        )
                        .on_hover_text(url);
                    }
                }
            },
        );
        section(ui, w, "source", "Advanced source", false, |ui, w| {
            if let Some(element) = semantics(w)
                .into_iter()
                .find(|element| element.data.get("field").and_then(Value::as_str) == Some("source"))
            {
                crate::ui::semantic_field(ui, w, &element);
            }
        });
    }
    for (key, error) in &w.fields {
        if key.starts_with(ERROR_PREFIX) {
            controls::label(ui, key, "material-diagnostic", &[], error, "invalid");
        }
    }
}
