//! Catalog structural members: sketch entities, pinned section, and locating point.
//! Every field changes the ordinary shared feature draft; profile graphics are
//! disposable views of application-owned catalog contours and locating points.
use crate::{Workspace, presentation_controls as controls, viewport};
use bevy_egui::egui;
use cad_core::{
    CadError, Dimension, Feature, Id, NodeRef, Operation, Profile, Result,
    commands::Command,
    presentation::SemanticElement,
    weldment::{Corner, MemberMode, WeldmentOperation},
    weldment_profiles::{Catalog, Contour, Entry, Family, Origin},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{cell::RefCell, collections::BTreeSet, sync::Arc};

pub const CONTRACT_VERSION: u32 = 1;
const DEFERRED: &str = "tcad:structural-member:";
const ACTION: &str = "structural-member.action";
const PICK: &str = "structural-member-pick";
const INVALID: &str = "structural-member-invalid:";
const MAX_PICK_SEGMENTS: usize = 65536;
const MAX_LIBRARY_BYTES: u64 = 4 * 1024 * 1024;

#[derive(Clone, Serialize, Deserialize)]
struct Pending {
    document: Id,
    owner: Id,
    path: Option<NodeRef>,
    members: Vec<Id>,
}
#[derive(Default)]
struct Libraries {
    initialized: bool,
    catalogs: Vec<Arc<Catalog>>,
    diagnostics: Vec<String>,
}
thread_local! {static LIBRARIES:RefCell<Libraries>=RefCell::new(Libraries::default());}

fn invalid(message: &str) -> CadError {
    CadError::Invalid(format!("Structural member: {message}"))
}
fn library_directory() -> std::path::PathBuf {
    crate::settings_preferences::path()
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .join("structural-profiles")
}
fn read_catalog(path: &std::path::Path) -> Result<Catalog> {
    use std::io::Read;
    let file = std::fs::File::open(path)?;
    if file.metadata()?.len() > MAX_LIBRARY_BYTES {
        return Err(invalid("Profile library exceeds the 4 MiB import limit"));
    }
    let mut source = String::new();
    file.take(MAX_LIBRARY_BYTES + 1)
        .read_to_string(&mut source)?;
    if source.len() as u64 > MAX_LIBRARY_BYTES {
        return Err(invalid("Profile library exceeds the 4 MiB import limit"));
    }
    cad_core::weldment_profiles::parse_catalog(&source)
}
fn catalogs() -> Vec<Arc<Catalog>> {
    LIBRARIES.with(|libraries| {
        let mut libraries = libraries.borrow_mut();
        if !libraries.initialized {
            libraries.initialized = true;
            match cad_core::weldment_profiles::builtin() {
                Ok(catalog) => libraries.catalogs.push(Arc::new(catalog)),
                Err(error) => libraries.diagnostics.push(error.to_string()),
            }
            if let Ok(paths) = std::fs::read_dir(library_directory()) {
                let mut paths = paths
                    .filter_map(std::result::Result::ok)
                    .filter(|entry| entry.path().extension().is_some_and(|e| e == "json"))
                    .take(31)
                    .map(|entry| entry.path())
                    .collect::<Vec<_>>();
                paths.sort();
                for path in paths {
                    match read_catalog(&path) {
                        Ok(catalog) => {
                            if let Some(existing) = libraries
                                .catalogs
                                .iter_mut()
                                .find(|existing| existing.id == catalog.id)
                            {
                                *existing = Arc::new(catalog);
                            } else {
                                libraries.catalogs.push(Arc::new(catalog));
                            }
                        }
                        Err(error) => libraries
                            .diagnostics
                            .push(format!("{}: {error}", path.display())),
                    }
                }
            }
        }
        libraries.catalogs.clone()
    })
}
fn current(w: &Workspace) -> Option<(NodeRef, Feature)> {
    if crate::command_palette::context_editor(w) != Some("feature")
        || w.tool_checkpoint.is_none()
        || w.settings.is_some()
        || w.view.history_cursor.is_some()
    {
        return None;
    }
    let definition = w.view.active_part.as_ref()?;
    let id = w.fields.get("feature-editor-id")?;
    let feature = w
        .session
        .visible()
        .parts
        .get(definition)?
        .features
        .iter()
        .find(|feature| &feature.id == id)?;
    if !matches!(&feature.operation, Operation::Weldment { operation }
        if matches!(operation.as_ref(), WeldmentOperation::Members { library_profile:Some(_), .. }))
    {
        return None;
    }
    Some((
        NodeRef {
            definition: definition.clone(),
            node: id.clone(),
        },
        feature.clone(),
    ))
}
pub fn active(w: &Workspace) -> bool {
    current(w).is_some()
}
fn origin(feature: &Feature) -> Option<&Origin> {
    let Operation::Weldment { operation } = &feature.operation else {
        return None;
    };
    let WeldmentOperation::Members {
        library_profile, ..
    } = operation.as_ref()
    else {
        return None;
    };
    library_profile.as_deref()
}
fn selected_path(w: &Workspace, owner: &str) -> Option<NodeRef> {
    let generated = generated_sections(w);
    let explicit = match &w.context_selection {
        Some(cad_core::selection_actions::SelectionTarget::Feature {
            definition,
            feature,
        }) if definition == owner => Some(NodeRef {
            definition: definition.clone(),
            node: feature.clone(),
        }),
        _ => None,
    };
    explicit
        .or_else(|| {
            w.view.active_node.as_ref().map(|node| NodeRef {
                definition: owner.into(),
                node: node.clone(),
            })
        })
        .filter(|source| !crate::fabrication_ui::structural_path_members(w, source).is_empty())
        .filter(|source| !generated.contains(source))
        .or_else(|| {
            w.session
                .visible()
                .parts
                .get(owner)?
                .features
                .iter()
                .find(|feature| w.view.active_node.as_ref() == Some(&feature.id))
                .and_then(|feature| match &feature.operation {
                    Operation::Weldment { operation } => match operation.as_ref() {
                        WeldmentOperation::Members { path, .. } => Some(path.clone()),
                        _ => None,
                    },
                    _ => None,
                })
        })
        .or_else(|| {
            w.session
                .visible()
                .parts
                .get(owner)?
                .features
                .iter()
                .rev()
                .filter(|feature| {
                    !feature.suppressed
                        && matches!(
                            feature.operation,
                            Operation::Profile { .. } | Operation::Sketch3d { .. }
                        )
                })
                .map(|feature| NodeRef {
                    definition: owner.into(),
                    node: feature.id.clone(),
                })
                .find(|source| {
                    !generated.contains(source)
                        && !crate::fabrication_ui::structural_path_members(w, source).is_empty()
                })
        })
}
fn generated_sections(w: &Workspace) -> BTreeSet<NodeRef> {
    w.session
        .visible()
        .parts
        .values()
        .flat_map(|part| part.features.iter())
        .filter_map(|feature| match &feature.operation {
            Operation::Weldment { operation } => match operation.as_ref() {
                WeldmentOperation::Members {
                    section,
                    library_profile: Some(_),
                    ..
                } => Some(section.clone()),
                _ => None,
            },
            _ => None,
        })
        .collect()
}
fn pending(w: &Workspace) -> Option<Pending> {
    let owner = w
        .sketch3d
        .as_ref()
        .map(|sketch| sketch.owner.clone())
        .or_else(|| w.sketch.as_ref().map(|sketch| sketch.definition.clone()))
        .or_else(|| w.view.active_part.clone())?;
    let (path, members) = if let Some(sketch) = &w.sketch3d {
        (
            Some(NodeRef {
                definition: sketch.owner.clone(),
                node: sketch.feature.clone(),
            }),
            sketch
                .selection
                .iter()
                .filter_map(|entity| match entity {
                    crate::sketch3d_ui::Entity::Member { member } => Some(member.clone()),
                    _ => None,
                })
                .collect(),
        )
    } else if let Some(sketch) = &w.sketch {
        (
            Some(NodeRef {
                definition: sketch.definition.clone(),
                node: sketch.feature.clone(),
            }),
            sketch
                .selection
                .iter()
                .filter_map(|entity| match entity {
                    cad_core::sketch_actions::SketchEntity::Member { member } => {
                        Some(member.clone())
                    }
                    _ => None,
                })
                .collect(),
        )
    } else {
        (selected_path(w, &owner), vec![])
    };
    Some(Pending {
        document: w.session.visible().id.clone(),
        owner,
        path,
        members,
    })
}
pub fn is_deferred(name: &str) -> bool {
    name.starts_with(DEFERRED)
}
/// Preserve selected sketch-member identities across the existing sketch's
/// validated confirmation, then open the ordinary structural feature draft.
pub fn begin(w: &mut Workspace) {
    let Some(pending) = pending(w) else {
        w.status = "Create a layout sketch in a part first".into();
        return;
    };
    if w.sketch.is_some() || w.sketch3d.is_some() || w.confirm_requested {
        match serde_json::to_string(&pending) {
            Ok(source) => w.after_confirm = Some(format!("{DEFERRED}{source}")),
            Err(error) => {
                w.status = error.to_string();
                return;
            }
        }
        if !w.confirm_requested {
            w.confirm_tool();
            if (w.sketch.is_some() || w.sketch3d.is_some()) && !w.confirm_requested {
                w.after_confirm = None;
            }
        }
        return;
    }
    begin_pending(w, pending);
}
pub fn resume(w: &mut Workspace, name: &str) -> bool {
    let Some(source) = name.strip_prefix(DEFERRED) else {
        return false;
    };
    match serde_json::from_str::<Pending>(source) {
        Ok(pending)
            if pending.document == w.session.visible().id
                && w.session.visible().parts.contains_key(&pending.owner) =>
        {
            begin_pending(w, pending)
        }
        Ok(_) => {
            w.status = "The structural-member request belongs to another document or part".into()
        }
        Err(error) => w.status = error.to_string(),
    }
    true
}
fn begin_pending(w: &mut Workspace, pending: Pending) {
    if w.tool_checkpoint.is_some() || w.settings.is_some() {
        w.status = "Confirm or cancel the current operation first".into();
        return;
    }
    let Some(mut part) = w.session.visible().parts.get(&pending.owner).cloned() else {
        return;
    };
    let Some(path) = pending.path.filter(|path| {
        path.definition == pending.owner
            && !crate::fabrication_ui::structural_path_members(w, path).is_empty()
    }) else {
        w.status = "Create or select a layout sketch with line or circular entities first".into();
        return;
    };
    let libraries = catalogs();
    let Some(catalog) = libraries.first() else {
        w.status = "No valid structural profile library is available".into();
        return;
    };
    let Some(entry) = catalog
        .profiles
        .iter()
        .find(|entry| entry.family == Family::SquareTube)
        .or_else(|| catalog.profiles.first())
    else {
        w.status = "The structural profile library is empty".into();
        return;
    };
    let section_id = cad_core::id();
    let section_feature_id = cad_core::id();
    let section = match catalog.instantiate(&entry.id, &section_id) {
        Ok(section) => section,
        Err(error) => {
            w.status = error.to_string();
            return;
        }
    };
    let source = NodeRef {
        definition: part.id.clone(),
        node: section_feature_id.clone(),
    };
    let body = part
        .features
        .iter()
        .rev()
        .find(|feature| {
            !feature.suppressed
                && part.outputs.contains(&feature.id)
                && matches!(feature.operation, Operation::Weldment { .. })
        })
        .map(|feature| part.reference(&feature.id));
    let available = crate::fabrication_ui::structural_path_members(w, &path);
    let mut members = pending
        .members
        .into_iter()
        .filter(|member| available.iter().any(|(id, _)| id == member))
        .collect::<Vec<_>>();
    members.dedup();
    let spatial_path = part.features.iter().any(|feature| {
        feature.id == path.node && matches!(feature.operation, Operation::Sketch3d { .. })
    });
    let feature = Feature {
        id:cad_core::id(), name:format!("Structural Member{}", part.features.iter().filter(|feature|
            matches!(&feature.operation, Operation::Weldment { operation } if matches!(operation.as_ref(), WeldmentOperation::Members { .. }))).count()+1),
        suppressed:false,
        operation:Operation::Weldment { operation:Box::new(WeldmentOperation::Members {
            body, section:source, path, members, member_mode:MemberMode::Explicit,
            roll:"0 deg".into(), anchor:None, profile_offset:["0 mm".into(), "0 mm".into()],
            start_offset:"0 mm".into(), end_offset:"0 mm".into(), corner:if spatial_path {Corner::Butt} else {Corner::Miter},
            joint_gap:"0 mm".into(), library_profile:Some(Box::new(section.origin)),
        }) },
    };
    let profile = Feature {
        id: section_feature_id.clone(),
        name: format!("{} profile", entry.label),
        suppressed: false,
        operation: Operation::Profile {
            plane: Default::default(),
            profile: Profile::Sketch {
                sketch: section_id.clone(),
            },
            plane_reference: None,
            reference_body: None,
        },
    };
    part.sketches.insert(section_id, section.sketch);
    part.features.push(profile);
    part.outputs.retain(|output| {
        !feature
            .operation
            .consumed_outputs()
            .iter()
            .any(|input| input.node == *output && input.definition == part.id)
    });
    part.outputs.push(feature.id.clone());
    part.features.push(feature.clone());
    w.view.active_part = Some(part.id.clone());
    w.start_tool();
    if let Err(error) = put_part(w, part) {
        w.cancel_tool();
        w.status = error.to_string();
        return;
    }
    w.view.active_node = Some(feature.id.clone());
    crate::ui::feature_dialog(w, Some(feature));
    w.fields.insert(PICK.into(), "true".into());
    w.status = "Pick sketch entities, choose a profile, then locate it on the path".into();
}

fn metadata(w: &Workspace, owner: &NodeRef, group: &str) -> Value {
    json!({"contract":"tcad.presentation.structural-member", "version":CONTRACT_VERSION,
        "document":w.session.visible().id,"owner":owner.definition,"feature":owner.node,
        "editor":w.fields.get("feature-editor-generation"),"group":group,"classes":["modeling-field",group]})
}
fn group(w: &Workspace, owner: &NodeRef, key: &str, label: &str) -> SemanticElement {
    let mut data = metadata(w, owner, key);
    data["classes"] = json!(["modeling-field", "modeling-group", "inspector-section", key]);
    SemanticElement {
        id: format!("structural-member:{}/group/{key}", owner.node),
        role: "structural-member-group".into(),
        label: label.into(),
        value: None,
        state: "draft".into(),
        commands: vec![],
        relationships: vec![owner.node.clone()],
        data,
    }
}
fn choice(label: impl Into<String>, value: impl Serialize) -> Value {
    json!({"label":label.into(),"value":value})
}
#[expect(
    clippy::too_many_arguments,
    reason = "semantic field exposes identity, type, choices, and units"
)]
fn field(
    w: &Workspace,
    owner: &NodeRef,
    group: &str,
    key: &str,
    label: &str,
    value: Value,
    choices: Vec<Value>,
    unit: Option<&str>,
) -> SemanticElement {
    let mut data = metadata(w, owner, group);
    data["setter"] = json!("structural-member");
    data["field"] = json!(key);
    data["json"] = value.clone();
    data["editable"] = json!(!w.fields.contains_key("feature-source-invalid"));
    data["choices"] = json!(choices);
    data["unit"] = json!(unit);
    data["dimension"] = json!(match unit {
        Some("mm") => "length",
        Some("deg") => "angle",
        _ => "scalar",
    });
    data["value_type"] = json!(if !choices.is_empty() {
        "choice"
    } else if value.is_boolean() {
        "boolean"
    } else if unit.is_some() {
        "quantity"
    } else {
        "text"
    });
    data["accessibility_label"] = json!(label);
    SemanticElement {
        id: format!("structural-member:{}/field/{key}", owner.node),
        role: "structural-member-field".into(),
        label: label.into(),
        value: Some(
            value
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| value.to_string()),
        ),
        state: "draft".into(),
        commands: vec![],
        relationships: vec![owner.node.clone()],
        data,
    }
}
fn action_element(
    w: &Workspace,
    owner: &NodeRef,
    group: &str,
    key: &str,
    label: &str,
    enabled: bool,
) -> SemanticElement {
    let mut data = metadata(w, owner, group);
    data["enabled"] = json!(enabled);
    data["action"] = json!(key);
    SemanticElement {
        id: format!("structural-member:{}/action/{key}", owner.node),
        role: "structural-member-action".into(),
        label: label.into(),
        value: None,
        state: if enabled { "available" } else { "disabled" }.into(),
        commands: vec![ACTION.into()],
        relationships: vec![owner.node.clone()],
        data,
    }
}
fn note(w: &Workspace, owner: &NodeRef, group: &str, key: &str, label: &str) -> SemanticElement {
    SemanticElement {
        id: format!("structural-member:{}/note/{key}", owner.node),
        role: "structural-member-note".into(),
        label: label.into(),
        value: None,
        state: "ready".into(),
        commands: vec![],
        relationships: vec![owner.node.clone()],
        data: metadata(w, owner, group),
    }
}
fn available_entries<'a>(
    profile: &'a Origin,
    catalogs: &'a [Arc<Catalog>],
) -> Vec<(&'a str, &'a Entry)> {
    let mut entries = catalogs
        .iter()
        .flat_map(|catalog| {
            catalog
                .profiles
                .iter()
                .map(move |entry| (catalog.id.as_str(), entry))
        })
        .collect::<Vec<_>>();
    // A document's snapshot remains available even if the personal catalog was
    // removed or changed. Merely opening the editor never replaces the section.
    entries.retain(|(catalog, entry)| {
        *catalog != profile.catalog_id || entry.id != profile.profile.id
    });
    entries.push((profile.catalog_id.as_str(), &profile.profile));
    entries
}
fn selected_members(operation: &WeldmentOperation, w: &Workspace) -> Vec<Id> {
    let WeldmentOperation::Members {
        members,
        member_mode,
        path,
        ..
    } = operation
    else {
        return vec![];
    };
    if *member_mode == MemberMode::All && members.is_empty() {
        crate::fabrication_ui::structural_path_members(w, path)
            .into_iter()
            .map(|(id, _)| id)
            .collect()
    } else {
        members.clone()
    }
}
pub fn semantics(w: &Workspace) -> Vec<SemanticElement> {
    let Some((owner, feature)) = current(w) else {
        return vec![];
    };
    let Operation::Weldment { operation } = &feature.operation else {
        return vec![];
    };
    let WeldmentOperation::Members {
        body,
        section,
        path,
        roll,
        anchor,
        profile_offset,
        start_offset,
        end_offset,
        corner,
        joint_gap,
        ..
    } = operation.as_ref()
    else {
        return vec![];
    };
    let Some(profile) = origin(&feature) else {
        return vec![];
    };
    let libraries = catalogs();
    let entries = available_entries(profile, &libraries);
    let generated = generated_sections(w);
    let mut out = vec![group(w, &owner, "paths", "Sketch entities")];
    let path_choices = w
        .session
        .visible()
        .parts
        .get(&owner.definition)
        .into_iter()
        .flat_map(|part| {
            part.features
                .iter()
                .take_while(|feature| feature.id != owner.node)
        })
        .filter(|feature| {
            !feature.suppressed
                && matches!(
                    feature.operation,
                    Operation::Profile { .. } | Operation::Sketch3d { .. }
                )
        })
        .filter_map(|feature| {
            let source = NodeRef {
                definition: owner.definition.clone(),
                node: feature.id.clone(),
            };
            (source != *section
                && !generated.contains(&source)
                && !crate::fabrication_ui::structural_path_members(w, &source).is_empty())
            .then(|| choice(&feature.name, source))
        })
        .collect();
    out.push(field(
        w,
        &owner,
        "paths",
        "path",
        "Layout sketch",
        json!(path),
        path_choices,
        None,
    ));
    let selected = selected_members(operation, w);
    out.push(action_element(
        w,
        &owner,
        "paths",
        "pick",
        "Pick sketch entities",
        true,
    ));
    out.push(action_element(
        w,
        &owner,
        "paths",
        "all",
        "Select all",
        true,
    ));
    out.push(action_element(
        w,
        &owner,
        "paths",
        "clear",
        "Clear selection",
        !selected.is_empty(),
    ));
    for (id, label) in crate::fabrication_ui::structural_path_members(w, path) {
        out.push(field(
            w,
            &owner,
            "paths",
            &format!("member/{id}"),
            &label,
            json!(selected.contains(&id)),
            vec![],
            None,
        ));
    }
    out.push(note(
        w,
        &owner,
        "paths",
        "selection",
        &format!(
            "{} sketch entities selected. Click their lines on the model to add or remove them.",
            selected.len()
        ),
    ));
    if selected.is_empty() {
        out.push(note(
            w,
            &owner,
            "paths",
            "empty",
            "Select at least one sketch entity before confirming.",
        ));
    }
    out.push(group(w, &owner, "profile", "Pre-made profile"));
    let mut collections = entries
        .iter()
        .map(|(catalog, entry)| (catalog.to_string(), entry.collection.clone()))
        .collect::<BTreeSet<_>>();
    collections.insert((
        profile.catalog_id.clone(),
        profile.profile.collection.clone(),
    ));
    let collection_labels = |catalog_id: &str, id: &str| {
        libraries
            .iter()
            .find(|catalog| catalog.id == catalog_id)
            .map(|catalog| {
                format!(
                    "{} / {}",
                    catalog.name,
                    catalog
                        .collections
                        .iter()
                        .find(|collection| collection.id == id)
                        .map_or("Saved collection", |collection| collection.label.as_str())
                )
            })
            .unwrap_or_else(|| format!("{} / saved collection", profile.catalog_name))
    };
    out.push(field(
        w,
        &owner,
        "profile",
        "collection",
        "Standard / collection",
        json!([profile.catalog_id, profile.profile.collection]),
        collections
            .into_iter()
            .map(|(catalog, id)| choice(collection_labels(&catalog, &id), json!([catalog, id])))
            .collect(),
        None,
    ));
    let families = entries
        .iter()
        .filter(|(catalog, entry)| {
            *catalog == profile.catalog_id && entry.collection == profile.profile.collection
        })
        .map(|(_, entry)| entry.family)
        .collect::<BTreeSet<_>>();
    out.push(field(
        w,
        &owner,
        "profile",
        "family",
        "Profile type",
        json!(profile.profile.family),
        families
            .into_iter()
            .map(|family| choice(family.label(), family))
            .collect(),
        None,
    ));
    out.push(field(
        w,
        &owner,
        "profile",
        "profile",
        "Size",
        json!([profile.catalog_id, profile.profile.id]),
        entries
            .iter()
            .filter(|(catalog, entry)| {
                *catalog == profile.catalog_id
                    && entry.collection == profile.profile.collection
                    && entry.family == profile.profile.family
            })
            .map(|(catalog, entry)| choice(&entry.size, json!([catalog, entry.id])))
            .collect(),
        None,
    ));
    out.push(note(
        w,
        &owner,
        "profile",
        "snapshot",
        &format!(
            "{} · pinned profile revision {}",
            profile.catalog_name, profile.profile.revision
        ),
    ));
    if !profile.profile.notes.is_empty() {
        out.push(note(
            w,
            &owner,
            "profile",
            "profile-notes",
            &profile.profile.notes,
        ));
    }
    out.push(action_element(
        w,
        &owner,
        "profile",
        "import",
        "Import profile library…",
        true,
    ));
    out.push(action_element(
        w,
        &owner,
        "profile",
        "export",
        "Export profile library…",
        true,
    ));
    out.push(group(w, &owner, "placement", "Locate profile"));
    let locating = locators(w, section, profile, anchor.as_ref());
    let mut points = vec![choice("Profile origin", Value::Null)];
    points.extend(
        locating
            .iter()
            .map(|locator| choice(&locator.name, &locator.sketch_point)),
    );
    out.push(field(
        w,
        &owner,
        "placement",
        "anchor",
        "Locating point",
        json!(anchor),
        points,
        None,
    ));
    out.push(field(
        w,
        &owner,
        "placement",
        "roll",
        "Rotation",
        json!(roll),
        vec![],
        Some("deg"),
    ));
    out.push(field(
        w,
        &owner,
        "placement",
        "profile_offset/0",
        "Horizontal offset",
        json!(profile_offset[0]),
        vec![],
        Some("mm"),
    ));
    out.push(field(
        w,
        &owner,
        "placement",
        "profile_offset/1",
        "Vertical offset",
        json!(profile_offset[1]),
        vec![],
        Some("mm"),
    ));
    out.push(SemanticElement {id:format!("structural-member:{}/locator",owner.node),role:"structural-member-locator".into(),label:"Profile locating diagram".into(),value:None,state:"draft".into(),commands:vec![ACTION.into()],relationships:vec![owner.node.clone()],
        data:json!({"contract":"tcad.presentation.structural-profile-locator","version":1,"document":w.session.visible().id,"owner":owner.definition,"feature":owner.node,"editor":w.fields.get("feature-editor-generation"),"contours_mm":profile.profile.contours,"locating_points_mm":locating,"anchor":anchor,"roll":roll,"offset":profile_offset,"path_origin_mm":[0.,0.],"source_authority":"pinned catalog profile","action":"locate","classes":["profile-locator"]})});
    out.push(group(w, &owner, "joints", "Group and ends"));
    let mut body_choices = vec![choice("New weldment", Value::Null)];
    body_choices.extend(
        w.session
            .visible()
            .parts
            .get(&owner.definition)
            .into_iter()
            .flat_map(|part| {
                part.features
                    .iter()
                    .take_while(|feature| feature.id != owner.node)
            })
            .filter(|feature| {
                !feature.suppressed && matches!(feature.operation, Operation::Weldment { .. })
            })
            .map(|feature| {
                choice(
                    &feature.name,
                    NodeRef {
                        definition: owner.definition.clone(),
                        node: feature.id.clone(),
                    },
                )
            }),
    );
    out.push(field(
        w,
        &owner,
        "joints",
        "body",
        "Add to weldment",
        json!(body),
        body_choices,
        None,
    ));
    out.push(field(
        w,
        &owner,
        "joints",
        "corner",
        "Corner treatment",
        json!(corner),
        vec![
            choice("Miter", Corner::Miter),
            choice("Independent butt", Corner::Butt),
            choice("Trim to primary member", Corner::ButtTrim),
        ],
        None,
    ));
    out.push(field(
        w,
        &owner,
        "joints",
        "joint_gap",
        "Joint gap",
        json!(joint_gap),
        vec![],
        Some("mm"),
    ));
    out.push(field(
        w,
        &owner,
        "joints",
        "start_offset",
        "Start shortening (+) / extension (−)",
        json!(start_offset),
        vec![],
        Some("mm"),
    ));
    out.push(field(
        w,
        &owner,
        "joints",
        "end_offset",
        "Finish shortening (+) / extension (−)",
        json!(end_offset),
        vec![],
        Some("mm"),
    ));
    if *corner == Corner::ButtTrim {
        let choices = crate::fabrication_ui::structural_path_members(w, path)
            .into_iter()
            .filter(|(id, _)| selected.contains(id))
            .map(|(id, label)| choice(label, id))
            .collect();
        out.push(field(
            w,
            &owner,
            "joints",
            "primary",
            "Primary member",
            json!(selected.first()),
            choices,
            None,
        ));
    }
    out.push(note(
        w,
        &owner,
        "joints",
        "custom",
        "Custom section sketches remain available in Custom Section Along Path.",
    ));
    out.push(action_element(
        w,
        &owner,
        "joints",
        "restore-profile",
        "Restore saved profile",
        true,
    ));
    out.push(action_element(
        w,
        &owner,
        "joints",
        "custom",
        "Use custom section",
        !selected.is_empty(),
    ));
    for element in &mut out {
        if let Some(error) = w.fields.get(&error_key(element)) {
            element.state = "error".into();
            element.data["diagnostics"] = json!([error]);
        }
    }
    out
}
fn section_sketch_id(w: &Workspace, section: &NodeRef) -> Option<Id> {
    let feature = w
        .session
        .visible()
        .parts
        .get(&section.definition)?
        .features
        .iter()
        .find(|feature| feature.id == section.node)?;
    if let Operation::Profile {
        profile: Profile::Sketch { sketch },
        ..
    } = &feature.operation
    {
        Some(sketch.clone())
    } else {
        None
    }
}
fn authoritative_section<'a>(w: &'a Workspace, section: &NodeRef) -> Option<&'a cad_core::Sketch> {
    let part = w.session.visible().parts.get(&section.definition)?;
    let feature = part
        .features
        .iter()
        .find(|feature| feature.id == section.node)?;
    let Operation::Profile {
        profile: Profile::Sketch { sketch },
        ..
    } = &feature.operation
    else {
        return None;
    };
    part.sketches.get(sketch)
}
fn catalog_instantiation(
    origin: &Origin,
    sketch_id: &str,
) -> Result<cad_core::weldment_profiles::Instantiation> {
    origin.instantiate(sketch_id)
}
/// Library/Make Unique insertion can remap local IDs. The pinned source check
/// proves fixed point geometry first; exact authored coordinates then recover
/// each locator's actual point identity without a spatial/proximity heuristic.
fn locators(
    w: &Workspace,
    section: &NodeRef,
    profile: &Origin,
    anchor: Option<&Id>,
) -> Vec<cad_core::weldment_profiles::Locator> {
    let Some(sketch) = authoritative_section(w, section) else {
        return vec![];
    };
    if !profile.matches_sketch(sketch).unwrap_or(false) {
        return vec![];
    }
    let Ok(instance) = profile.instantiate(&sketch.id) else {
        return vec![];
    };
    instance
        .locators
        .into_iter()
        .filter_map(|mut locator| {
            let point = anchor
                .filter(|id| sketch.points.get(*id) == Some(&locator.point))
                .cloned()
                .or_else(|| {
                    (sketch.points.get(&locator.sketch_point) == Some(&locator.point))
                        .then(|| locator.sketch_point.clone())
                })
                .or_else(|| {
                    sketch
                        .points
                        .iter()
                        .find(|(_, point)| **point == locator.point)
                        .map(|(id, _)| id.clone())
                })?;
            locator.sketch_point = point;
            Some(locator)
        })
        .collect()
}
fn error_key(element: &SemanticElement) -> String {
    format!("{INVALID}{}:{}", element.id, element.data["editor"])
}
fn live(w: &Workspace, element: &SemanticElement) -> Result<SemanticElement> {
    let fresh = semantics(w)
        .into_iter()
        .find(|fresh| fresh.id == element.id)
        .ok_or_else(|| invalid("This control has closed"))?;
    for key in ["document", "owner", "feature", "editor"] {
        if fresh.data.get(key) != element.data.get(key) {
            return Err(invalid("Use the current structural member controls"));
        }
    }
    if w.fields.contains_key("feature-source-invalid") {
        return Err(invalid(
            "Correct Advanced Source before using these controls",
        ));
    }
    Ok(fresh)
}
pub fn current_value(w: &Workspace, element: &SemanticElement) -> Option<Value> {
    let (owner, feature) = current(w)?;
    if element.data["document"] != w.session.visible().id
        || element.data["owner"] != owner.definition
        || element.data["feature"] != owner.node
        || element.data["editor"] != json!(w.fields.get("feature-editor-generation"))
    {
        return None;
    }
    let Operation::Weldment { operation } = &feature.operation else {
        return None;
    };
    let WeldmentOperation::Members {
        body,
        path,
        members,
        member_mode,
        roll,
        anchor,
        profile_offset,
        start_offset,
        end_offset,
        corner,
        joint_gap,
        ..
    } = operation.as_ref()
    else {
        return None;
    };
    let profile = origin(&feature)?;
    Some(match element.data["field"].as_str()? {
        "collection" => json!([profile.catalog_id, profile.profile.collection]),
        "family" => json!(profile.profile.family),
        "profile" => json!([profile.catalog_id, profile.profile.id]),
        "path" => json!(path),
        "body" => json!(body),
        "anchor" => json!(anchor),
        "roll" => json!(roll),
        "profile_offset/0" => json!(profile_offset[0]),
        "profile_offset/1" => json!(profile_offset[1]),
        "start_offset" => json!(start_offset),
        "end_offset" => json!(end_offset),
        "corner" => json!(corner),
        "joint_gap" => json!(joint_gap),
        "primary" => json!(selected_members(operation, w).first()),
        key if key.starts_with("member/") => json!(
            *member_mode == MemberMode::All && members.is_empty()
                || members
                    .iter()
                    .any(|member| member == key.trim_start_matches("member/"))
        ),
        _ => return None,
    })
}
pub fn field_is_invalid(w: &Workspace, element: &SemanticElement) -> bool {
    element.data["setter"] == "structural-member" && w.fields.contains_key(&error_key(element))
}
pub fn field_error(w: &mut Workspace, element: &SemanticElement, error: Option<String>) {
    if element.data["setter"] != "structural-member" {
        return;
    }
    if let Some(error) = error {
        w.fields.insert(error_key(element), error);
        w.confirm_requested = false;
    } else {
        w.fields.remove(&error_key(element));
    }
}
pub fn current_source_is_valid(w: &Workspace) -> bool {
    let Some((_, feature)) = current(w) else {
        return true;
    };
    let Operation::Weldment { operation } = &feature.operation else {
        return true;
    };
    !selected_members(operation, w).is_empty()
        && !semantics(w)
            .iter()
            .any(|element| field_is_invalid(w, element))
}
fn publish(w: &mut Workspace, feature: Feature) -> Result<()> {
    let (owner, _) =
        current(w).ok_or_else(|| invalid("The structural member editor has closed"))?;
    let expected = feature.clone();
    w.dispatch(Command::PutFeature {
        definition: owner.definition.clone(),
        feature: Box::new(feature),
        output: false,
    });
    if !w
        .session
        .visible()
        .parts
        .get(&owner.definition)
        .is_some_and(|part| part.features.iter().any(|feature| feature == &expected))
    {
        return Err(invalid(&w.status));
    }
    w.json = serde_json::to_string_pretty(&expected)?;
    Ok(())
}
fn put_part(w: &mut Workspace, part: cad_core::Part) -> Result<()> {
    let expected = part.clone();
    w.dispatch(Command::PutPart {
        part,
        assembly: None,
        placement: Default::default(),
    });
    if w.session.visible().parts.get(&expected.id) != Some(&expected) {
        return Err(invalid(&w.status));
    }
    Ok(())
}
fn select_profile(w: &mut Workspace, catalog_id: &str, entry: &Entry) -> Result<()> {
    let (owner, mut feature) = current(w).ok_or_else(|| invalid("The editor has closed"))?;
    let Operation::Weldment { operation } = &mut feature.operation else {
        return Err(invalid("Not a structural member"));
    };
    let WeldmentOperation::Members {
        section,
        anchor,
        library_profile,
        ..
    } = operation.as_mut()
    else {
        return Err(invalid("Not a structural member"));
    };
    let sketch_id = section_sketch_id(w, section)
        .ok_or_else(|| invalid("The saved profile sketch is missing"))?;
    let catalog = catalogs()
        .into_iter()
        .find(|catalog| catalog.id == catalog_id);
    let instance = if let Some(snapshot) = library_profile
        .as_deref()
        .filter(|snapshot| snapshot.catalog_id == catalog_id && snapshot.profile == *entry)
    {
        snapshot.instantiate(&sketch_id)?
    } else if let Some(catalog) = catalog {
        catalog.instantiate(&entry.id, &sketch_id)?
    } else {
        let mut snapshot = library_profile
            .clone()
            .ok_or_else(|| invalid("No pinned profile is available"))?;
        snapshot.profile = entry.clone();
        catalog_instantiation(&snapshot, &sketch_id)?
    };
    *anchor = None;
    *library_profile = Some(Box::new(instance.origin));
    let mut part = w
        .session
        .visible()
        .parts
        .get(&owner.definition)
        .cloned()
        .ok_or_else(|| invalid("The editing part closed"))?;
    let section_feature = part
        .features
        .iter_mut()
        .find(|item| item.id == section.node)
        .ok_or_else(|| invalid("The profile feature is missing"))?;
    section_feature.name = format!("{} profile", entry.label);
    part.sketches.insert(sketch_id, instance.sketch);
    let target = part
        .features
        .iter_mut()
        .find(|item| item.id == feature.id)
        .ok_or_else(|| invalid("The structural member feature is missing"))?;
    *target = feature.clone();
    put_part(w, part)?;
    w.json = serde_json::to_string_pretty(&feature)?;
    Ok(())
}
pub fn set_value(w: &mut Workspace, element: &SemanticElement, value: Value) -> Result<()> {
    let result = set_field(w, element, value);
    field_error(w, element, result.as_ref().err().map(ToString::to_string));
    result
}
fn set_field(w: &mut Workspace, element: &SemanticElement, value: Value) -> Result<()> {
    let element = live(w, element)?;
    if element.data["json"] == value {
        return Ok(());
    }
    let descriptor = element.value_descriptor();
    if !descriptor.choices.is_empty()
        && !descriptor
            .choices
            .iter()
            .any(|choice| choice.get("value").unwrap_or(choice) == &value)
    {
        return Err(invalid(
            "Choose one of the current profile or sketch options",
        ));
    }
    let key = element.data["field"]
        .as_str()
        .ok_or_else(|| invalid("The control has no field binding"))?;
    let (_, mut feature) = current(w).ok_or_else(|| invalid("The editor has closed"))?;
    let profile = origin(&feature)
        .cloned()
        .ok_or_else(|| invalid("The pinned profile is missing"))?;
    if matches!(key, "collection" | "family" | "profile") {
        let libraries = catalogs();
        let entries = available_entries(&profile, &libraries);
        let chosen = match key {
            "collection" => entries
                .iter()
                .find(|(catalog, entry)| json!([catalog, entry.collection]) == value),
            "family" => entries.iter().find(|(catalog, entry)| {
                *catalog == profile.catalog_id
                    && entry.collection == profile.profile.collection
                    && json!(entry.family) == value
            }),
            _ => entries
                .iter()
                .find(|(catalog, entry)| json!([catalog, entry.id]) == value),
        }
        .ok_or_else(|| invalid("That profile selection is unavailable"))?;
        return select_profile(w, chosen.0, chosen.1);
    }
    let Operation::Weldment { operation } = &mut feature.operation else {
        return Err(invalid("Not a structural member"));
    };
    let selected = selected_members(operation, w);
    let WeldmentOperation::Members {
        body,
        path,
        members,
        member_mode,
        roll,
        anchor,
        profile_offset,
        start_offset,
        end_offset,
        corner,
        joint_gap,
        ..
    } = operation.as_mut()
    else {
        return Err(invalid("Not a structural member"));
    };
    match key {
        "path" => {
            *path = serde_json::from_value(value)?;
            *members = vec![];
            *member_mode = MemberMode::Explicit;
        }
        "body" => *body = serde_json::from_value(value)?,
        "anchor" => *anchor = serde_json::from_value(value)?,
        "corner" => *corner = serde_json::from_value(value)?,
        "roll" | "profile_offset/0" | "profile_offset/1" | "start_offset" | "end_offset"
        | "joint_gap" => {
            let expression = value
                .as_str()
                .ok_or_else(|| invalid("Enter an expression"))?;
            if expression.trim().is_empty() || expression.len() > 65536 {
                return Err(invalid("Enter a finite quantity or expression"));
            }
            let target = match key {
                "roll" => roll,
                "profile_offset/0" => &mut profile_offset[0],
                "profile_offset/1" => &mut profile_offset[1],
                "start_offset" => start_offset,
                "end_offset" => end_offset,
                _ => joint_gap,
            };
            target.0 = expression.into();
        }
        "primary" => {
            let id = value.as_str().ok_or_else(|| invalid("Select a member"))?;
            *members = selected;
            members.retain(|member| member != id);
            members.insert(0, id.into());
            *member_mode = MemberMode::Explicit;
        }
        _ if key.starts_with("member/") => {
            let id = key.trim_start_matches("member/");
            let selected_value = value
                .as_bool()
                .ok_or_else(|| invalid("Expected a selection toggle"))?;
            *members = selected;
            members.retain(|member| member != id);
            if selected_value {
                members.push(id.into());
            }
            *member_mode = MemberMode::Explicit;
        }
        _ => return Err(invalid("Unsupported structural member field")),
    }
    publish(w, feature)
}

pub fn action(w: &mut Workspace, command: &str, payload: &Value) -> bool {
    if command != ACTION {
        return false;
    }
    let result = (|| -> Result<()> {
        let (owner, mut feature) =
            current(w).ok_or_else(|| invalid("Open a structural member first"))?;
        for (key, value) in [
            ("document", json!(w.session.visible().id)),
            ("owner", json!(owner.definition)),
            ("feature", json!(owner.node)),
            ("editor", json!(w.fields.get("feature-editor-generation"))),
        ] {
            if payload.get(key).is_some_and(|actual| actual != &value) {
                return Err(invalid(
                    "This action belongs to another structural member editor",
                ));
            }
        }
        match payload["action"].as_str().unwrap_or("") {
            "pick" => {
                w.fields.insert(PICK.into(), "true".into());
                w.status = "Click layout sketch entities to add or remove members".into();
                Ok(())
            }
            "all" | "clear" => {
                let Operation::Weldment { operation } = &mut feature.operation else {
                    return Err(invalid("Not a member group"));
                };
                let WeldmentOperation::Members {
                    path,
                    members,
                    member_mode,
                    ..
                } = operation.as_mut()
                else {
                    return Err(invalid("Not a member group"));
                };
                *members = if payload["action"] == "all" {
                    crate::fabrication_ui::structural_path_members(w, path)
                        .into_iter()
                        .map(|(id, _)| id)
                        .collect()
                } else {
                    vec![]
                };
                *member_mode = MemberMode::Explicit;
                publish(w, feature)
            }
            "locate" => {
                let locator = payload.get("locator").cloned().unwrap_or(Value::Null);
                let field = semantics(w)
                    .into_iter()
                    .find(|field| field.data["field"] == "anchor")
                    .ok_or_else(|| invalid("The locating-point control closed"))?;
                set_value(w, &field, locator)
            }
            "import" => import_library(w),
            "export" => export_library(w),
            "restore-profile" => restore_profile(w),
            "custom" => {
                let Operation::Weldment { operation } = &mut feature.operation else {
                    return Err(invalid("Not a structural member"));
                };
                let selected = selected_members(operation, w);
                if selected.is_empty() {
                    return Err(invalid(
                        "Select sketch entities before using a custom section",
                    ));
                }
                let WeldmentOperation::Members {
                    library_profile,
                    member_mode,
                    members,
                    ..
                } = operation.as_mut()
                else {
                    return Err(invalid("Not a structural member"));
                };
                *library_profile = None;
                *member_mode = MemberMode::All;
                *members = selected;
                publish(w, feature)?;
                w.status="Custom section editing enabled; the current section sketch and selected path entities are retained".into();
                Ok(())
            }
            _ => Err(invalid("Unknown structural member action")),
        }
    })();
    if let Err(error) = result {
        w.status = error.to_string();
    }
    true
}
fn restore_profile(w: &mut Workspace) -> Result<()> {
    let (owner, mut feature) = current(w).ok_or_else(|| invalid("The editor has closed"))?;
    let Operation::Weldment { operation } = &mut feature.operation else {
        return Err(invalid("Not a structural member"));
    };
    let WeldmentOperation::Members {
        section,
        library_profile,
        anchor,
        ..
    } = operation.as_mut()
    else {
        return Err(invalid("Not a structural member"));
    };
    let profile = library_profile
        .as_deref()
        .ok_or_else(|| invalid("No pinned profile snapshot is available"))?;
    let sketch_id =
        section_sketch_id(w, section).ok_or_else(|| invalid("The profile sketch is missing"))?;
    let previous = authoritative_section(w, section).and_then(|sketch| {
        anchor
            .as_ref()
            .and_then(|point| sketch.points.get(point))
            .copied()
    });
    let instance = profile.instantiate(&sketch_id)?;
    *anchor = previous.and_then(|point| {
        instance
            .locators
            .iter()
            .find(|locator| locator.point == point)
            .map(|locator| locator.sketch_point.clone())
    });
    let mut part = w
        .session
        .visible()
        .parts
        .get(&owner.definition)
        .cloned()
        .ok_or_else(|| invalid("The part has closed"))?;
    part.sketches.insert(sketch_id, instance.sketch);
    *part
        .features
        .iter_mut()
        .find(|item| item.id == feature.id)
        .ok_or_else(|| invalid("The structural member feature is missing"))? = feature.clone();
    put_part(w, part)?;
    w.json = serde_json::to_string_pretty(&feature)?;
    w.status = "Restored the saved profile snapshot in the current draft".into();
    Ok(())
}
fn import_library(w: &mut Workspace) -> Result<()> {
    let Some(path) = rfd::FileDialog::new()
        .set_title("Import structural profile library")
        .add_filter("TCAD profile library", &["json"])
        .pick_file()
    else {
        return Ok(());
    };
    let catalog = read_catalog(&path)?;
    if catalog.id == cad_core::weldment_profiles::builtin()?.id {
        return Err(invalid("Use a distinct library ID for personal profiles"));
    }
    if catalogs().len() >= 32 && !catalogs().iter().any(|existing| existing.id == catalog.id) {
        return Err(invalid("At most 32 profile libraries may be loaded"));
    }
    let source = cad_core::weldment_profiles::catalog_json(&catalog)?;
    std::fs::create_dir_all(library_directory())?;
    let existing = std::fs::read_dir(library_directory())?
        .filter_map(std::result::Result::ok)
        .take(32)
        .find(|entry| read_catalog(&entry.path()).is_ok_and(|saved| saved.id == catalog.id))
        .map(|entry| entry.path());
    let target =
        existing.unwrap_or_else(|| library_directory().join(format!("{}.json", cad_core::id())));
    cad_core::persistence::atomic_write(&target, source.as_bytes())?;
    LIBRARIES.with(|libraries| {
        let mut libraries = libraries.borrow_mut();
        if let Some(existing) = libraries
            .catalogs
            .iter_mut()
            .find(|existing| existing.id == catalog.id)
        {
            *existing = Arc::new(catalog.clone());
        } else {
            libraries.catalogs.push(Arc::new(catalog.clone()));
        }
    });
    w.status = format!(
        "Loaded {}. Existing part profiles retain their saved snapshots.",
        catalog.name
    );
    Ok(())
}
fn export_library(w: &mut Workspace) -> Result<()> {
    let (_, feature) = current(w).ok_or_else(|| invalid("Open a structural member first"))?;
    let profile = origin(&feature).ok_or_else(|| invalid("No pinned profile is selected"))?;
    let catalog = catalogs()
        .into_iter()
        .find(|catalog| catalog.id == profile.catalog_id)
        .ok_or_else(|| {
            invalid("Import this profile's source library before exporting the complete library")
        })?;
    let Some(path) = rfd::FileDialog::new()
        .set_title("Export structural profile library")
        .set_file_name("structural-profiles.json")
        .add_filter("TCAD profile library", &["json"])
        .save_file()
    else {
        return Ok(());
    };
    let source = cad_core::weldment_profiles::catalog_json(&catalog)?;
    cad_core::persistence::atomic_write(&path, source.as_bytes())?;
    w.status = format!("Exported profile library to {}", path.display());
    Ok(())
}

pub fn show(ui: &mut egui::Ui, w: &mut Workspace) -> bool {
    if !active(w) {
        return false;
    }
    let elements = semantics(w);
    let mut groups: Vec<(SemanticElement, Vec<SemanticElement>)> = vec![];
    for element in elements {
        if element.role.ends_with("-group") {
            groups.push((element, vec![]));
        } else if let Some((_, children)) = groups.last_mut() {
            children.push(element);
        }
    }
    for (group, children) in groups {
        let classes = group
            .data
            .get("classes")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>();
        let open = group.data["group"] != "joints";
        controls::inspector_section(
            ui,
            controls::InspectorSection {
                id: &group.id,
                role: &group.role,
                classes: &classes,
                label: &group.label,
                state: "draft",
                collapse: Some(open),
                open: None,
            },
            |ui| {
                let mut children = children.into_iter().peekable();
                while let Some(element) = children.next() {
                    if element.role.ends_with("-action") {
                        let mut row = vec![element];
                        while children
                            .peek()
                            .is_some_and(|next| next.role.ends_with("-action"))
                        {
                            if let Some(next) = children.next() {
                                row.push(next);
                            }
                        }
                        ui.horizontal_wrapped(|ui| {
                            for element in row {
                                render_control(ui, w, &element);
                            }
                        });
                    } else {
                        render_control(ui, w, &element);
                    }
                }
            },
        );
    }
    true
}
pub(crate) fn render_control(ui: &mut egui::Ui, w: &mut Workspace, element: &SemanticElement) {
    if element.data.get("setter").is_some() {
        crate::ui::semantic_field(ui, w, element);
    } else if element.role.ends_with("-action") {
        let response = controls::button(
            ui,
            &element.id,
            &element.role,
            &["modeling-action"],
            &element.label,
            element.state != "disabled",
            element.data["action"] == "pick"
                && w.fields.get(PICK).is_some_and(|value| value == "true"),
        );
        if response.clicked() {
            action(w, ACTION, &element.data);
        }
    } else if element.role == "structural-member-locator" {
        locator(ui, w, element);
    } else {
        controls::label(
            ui,
            &element.id,
            &element.role,
            &["modeling-help"],
            &element.label,
            &element.state,
        );
    }
}

pub(crate) fn locator(ui: &mut egui::Ui, w: &mut Workspace, element: &SemanticElement) {
    let Some((_, feature)) = current(w) else {
        return;
    };
    let Some(profile) = origin(&feature) else {
        return;
    };
    let Operation::Weldment { operation } = &feature.operation else {
        return;
    };
    let WeldmentOperation::Members {
        section,
        anchor,
        roll,
        profile_offset,
        ..
    } = operation.as_ref()
    else {
        return;
    };
    let Some(sketch) = authoritative_section(w, section) else {
        return;
    };
    let locating = locators(w, section, profile, anchor.as_ref());
    let angle = cad_core::expression::value(
        w.session.visible(),
        &section.definition,
        roll,
        Dimension::Angle,
    );
    let x = cad_core::expression::value(
        w.session.visible(),
        &section.definition,
        &profile_offset[0],
        Dimension::Length,
    );
    let y = cad_core::expression::value(
        w.session.visible(),
        &section.definition,
        &profile_offset[1],
        Dimension::Length,
    );
    let (Ok(angle), Ok(x), Ok(y)) = (angle, x, y) else {
        controls::label(
            ui,
            &element.id,
            &element.role,
            &["profile-locator"],
            "Waiting for valid profile-placement quantities",
            "pending",
        );
        return;
    };
    let a = anchor
        .as_ref()
        .and_then(|id| sketch.points.get(id))
        .copied()
        .unwrap_or([0., 0.]);
    let (s, c) = angle.sin_cos();
    let transform = |point: [f64; 2]| {
        let p = [point[0] - a[0] + x, point[1] - a[1] + y];
        [c * p[0] - s * p[1], s * p[0] + c * p[1]]
    };
    let props = controls::properties(&element.id, &element.role, &["profile-locator"], "draft");
    if props.get("display").is_some_and(|v| v == "none")
        || props.get("visibility").is_some_and(|v| v == "hidden")
    {
        return;
    }
    let width = controls::length(
        &props,
        "width",
        ui.available_width(),
        ui.available_width().min(240.),
    )
    .clamp(64., ui.available_width().max(64.));
    let height = controls::number(&props, "height", 150.).max(64.);
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::click());
    let painter = ui.painter().with_clip_rect(rect);
    let color = props
        .get("color")
        .and_then(|v| crate::ui::color(v))
        .unwrap_or(ui.visuals().text_color());
    let accent = props
        .get("selection-color")
        .and_then(|v| crate::ui::color(v))
        .unwrap_or_else(|| viewport::selection_color(w));
    if let Some(background) = props.get("background").and_then(|v| crate::ui::color(v)) {
        painter.rect_filled(rect, 0., background);
    }
    let mut contours = vec![];
    for contour in &profile.profile.contours {
        let mut points = match contour {
            Contour::Polygon { points } => points.clone(),
            Contour::Circle { center, radius_mm } => (0..=96)
                .map(|index| {
                    let angle = index as f64 * std::f64::consts::TAU / 96.;
                    [
                        center[0] + radius_mm * angle.cos(),
                        center[1] + radius_mm * angle.sin(),
                    ]
                })
                .collect(),
        };
        if let Some(first) = points.first().copied()
            && points.last() != Some(&first)
        {
            points.push(first);
        }
        contours.push(points.into_iter().map(transform).collect::<Vec<_>>());
    }
    let mut minimum = [0f64; 2];
    let mut maximum = [0f64; 2];
    for point in contours.iter().flatten() {
        for axis in 0..2 {
            minimum[axis] = minimum[axis].min(point[axis]);
            maximum[axis] = maximum[axis].max(point[axis]);
        }
    }
    let scale = ((f64::from(rect.width()) - 36.) / (maximum[0] - minimum[0]).max(1.))
        .min((f64::from(rect.height()) - 36.) / (maximum[1] - minimum[1]).max(1.));
    let center = [
        (minimum[0] + maximum[0]) * 0.5,
        (minimum[1] + maximum[1]) * 0.5,
    ];
    let screen = |point: [f64; 2]| {
        rect.center()
            + egui::vec2(
                ((point[0] - center[0]) * scale) as f32,
                ((center[1] - point[1]) * scale) as f32,
            )
    };
    for contour in contours {
        for pair in contour.windows(2) {
            painter.line_segment(
                [screen(pair[0]), screen(pair[1])],
                egui::Stroke::new(controls::number(&props, "stroke-width", 1.5), color),
            );
        }
    }
    let origin = screen([0., 0.]);
    painter.line_segment(
        [origin - egui::vec2(7., 0.), origin + egui::vec2(7., 0.)],
        egui::Stroke::new(1.2, accent),
    );
    painter.line_segment(
        [origin - egui::vec2(0., 7.), origin + egui::vec2(0., 7.)],
        egui::Stroke::new(1.2, accent),
    );
    let mut markers = vec![(
        Value::Null,
        "Profile origin".into(),
        screen(transform([0., 0.])),
        anchor.is_none(),
    )];
    markers.extend(locating.iter().map(|locator| {
        (
            json!(locator.sketch_point),
            locator.name.clone(),
            screen(transform(locator.point)),
            anchor.as_ref() == Some(&locator.sketch_point),
        )
    }));
    let radius = controls::number(&props, "handle-size", 3.5).max(2.);
    for (_, _, point, selected) in &markers {
        painter.circle_filled(
            *point,
            if *selected { radius + 1.5 } else { radius },
            if *selected { accent } else { color },
        );
    }
    if let Some(point) = response.hover_pos()
        && let Some((value, name, location, _)) = markers
            .iter()
            .filter(|(_, _, location, _)| location.distance(point) <= radius.max(7.) + 3.)
            .min_by(|a, b| a.2.distance_sq(point).total_cmp(&b.2.distance_sq(point)))
    {
        painter.circle_stroke(*location, radius + 3., egui::Stroke::new(1.5, accent));
        response.clone().on_hover_text(format!(
            "{name}\nClick to locate this point on the selected path"
        ));
        if response.clicked() {
            let mut payload = element.data.clone();
            payload["action"] = json!("locate");
            payload["locator"] = value.clone();
            action(w, ACTION, &payload);
        }
    }
    controls::label(
        ui,
        &format!("{}/help", element.id),
        "structural-member-locator-help",
        &["modeling-help"],
        "Crosshair = sketch path. Click a profile point to locate it.",
        "ready",
    );
}

/// A plain layout sketch can keep its already solved display while changing
/// only a downstream member. Compare all source parameters/configurations,
/// upstream feature definitions, the exact sketch, other definitions and
/// assembly placement sources. Face-attached/converted paths require a fresh
/// native input evaluation instead of this narrower reuse proof.
fn unchanged_layout(w: &Workspace, path: &NodeRef, checkpoint: &cad_core::Session) -> bool {
    let current = w.session.visible();
    let before = checkpoint.visible();
    if current.id != before.id
        || current.root != before.root
        || current.settings != before.settings
        || current.active_configurations != before.active_configurations
        || current.assemblies != before.assemblies
        || current.resources != before.resources
        || current.libraries != before.libraries
        || current.properties != before.properties
        || current
            .parts
            .iter()
            .filter(|(id, _)| **id != path.definition)
            .ne(before
                .parts
                .iter()
                .filter(|(id, _)| **id != path.definition))
    {
        return false;
    }
    let (Some(a), Some(b)) = (
        current.parts.get(&path.definition),
        before.parts.get(&path.definition),
    ) else {
        return false;
    };
    if a.parameters != b.parameters
        || a.configurations != b.configurations
        || a.configuration_details != b.configuration_details
        || a.properties != b.properties
    {
        return false;
    }
    let (Some(index), Some(previous_index)) = (
        a.features
            .iter()
            .position(|feature| feature.id == path.node),
        b.features
            .iter()
            .position(|feature| feature.id == path.node),
    ) else {
        return false;
    };
    if a.features[..=index] != b.features[..=previous_index] {
        return false;
    }
    if matches!(a.features[index].operation, Operation::Sketch3d { .. }) {
        return true;
    }
    let Operation::Profile {
        profile: Profile::Sketch { sketch },
        plane_reference: None,
        reference_body: None,
        ..
    } = &a.features[index].operation
    else {
        return false;
    };
    let (Some(source), Some(previous)) = (a.sketches.get(sketch), b.sketches.get(sketch)) else {
        return false;
    };
    if source != previous || !source.conversions.is_empty() {
        return false;
    }
    // Upstream sketch definitions may supply expression/block sources. The
    // generated downstream profile is deliberately excluded from this proof.
    a.features[..=index]
        .iter()
        .all(|feature| match &feature.operation {
            Operation::Profile {
                profile: Profile::Sketch { sketch },
                ..
            } => a.sketches.get(sketch) == b.sketches.get(sketch),
            _ => true,
        })
}
fn path_lines(w: &Workspace) -> Vec<(Id, Vec<[f64; 3]>)> {
    let Some((owner, feature)) = current(w) else {
        return vec![];
    };
    let Operation::Weldment { operation } = &feature.operation else {
        return vec![];
    };
    let WeldmentOperation::Members { path, .. } = operation.as_ref() else {
        return vec![];
    };
    // The complete part can be invalid solely because its member selection is
    // empty. Its current solved input sketch remains a valid picker source.
    // Previously solved layout inputs are reusable only after the conservative
    // source/placement proof above, so rapid clicks do not wait for each solid.
    let identity = w.session.identity();
    let Some(evaluation) = w
        .session
        .draft
        .as_ref()
        .and_then(|draft| draft.evaluation.as_ref())
        .or(w.session.evaluation.as_ref())
        .filter(|evaluation| evaluation.identity == identity)
        .or_else(|| {
            let checkpoint = w.tool_checkpoint.as_ref()?;
            if !unchanged_layout(w, path, checkpoint) {
                return None;
            }
            checkpoint
                .draft
                .as_ref()
                .and_then(|draft| draft.evaluation.as_ref())
                .or(checkpoint.evaluation.as_ref())
                .filter(|evaluation| {
                    evaluation.valid && evaluation.identity == checkpoint.identity()
                })
        })
    else {
        return vec![];
    };
    let configuration = w
        .session
        .visible()
        .active_configurations
        .get(&path.definition)
        .map(String::as_str);
    let key = format!(
        "{}::{}",
        cad_core::configuration::key(&path.definition, configuration),
        path.node
    );
    let placement_evaluation = if evaluation
        .instances
        .iter()
        .any(|instance| instance.definition == owner.definition)
    {
        evaluation
    } else if let Some(checkpoint) = w
        .tool_checkpoint
        .as_ref()
        .and_then(|session| session.evaluation.as_ref())
    {
        // This editor only changes the owner part's section/member feature;
        // assembly placements remain those of its starting checkpoint.
        checkpoint
    } else {
        evaluation
    };
    let occurrences = viewport::display_instances(w, placement_evaluation)
        .into_iter()
        .filter(|instance| {
            instance.definition == owner.definition
                && w.view
                    .active_instance
                    .as_ref()
                    .is_none_or(|id| instance.path.last() == Some(id))
        })
        .collect::<Vec<_>>();
    let matrix = if occurrences.len() == 1 {
        occurrences[0].matrix
    } else if w.session.visible().root == owner.definition {
        std::array::from_fn(|i| std::array::from_fn(|j| if i == j { 1. } else { 0. }))
    } else {
        return vec![];
    };
    if let Some(sketch) = evaluation.spatial_sketches.get(&key) {
        let available = crate::fabrication_ui::structural_path_members(w, path)
            .into_iter()
            .map(|(id, _)| id)
            .collect::<BTreeSet<_>>();
        let mut work = MAX_PICK_SEGMENTS;
        return sketch
            .polylines
            .iter()
            .filter(|line| available.contains(&line.member))
            .filter_map(|line| {
                work = work.checked_sub(line.points.len())?;
                Some((
                    line.member.clone(),
                    line.points
                        .iter()
                        .map(|point| {
                            std::array::from_fn(|axis| {
                                matrix[axis][0] * point[0]
                                    + matrix[axis][1] * point[1]
                                    + matrix[axis][2] * point[2]
                                    + matrix[axis][3]
                            })
                        })
                        .collect(),
                ))
            })
            .collect();
    }
    let Some(sketch) = evaluation
        .sketches
        .get(&key)
        .filter(|sketch| sketch.solved.is_some())
    else {
        return vec![];
    };
    let available = crate::fabrication_ui::structural_path_members(w, path)
        .into_iter()
        .map(|(id, _)| id)
        .collect::<BTreeSet<_>>();
    let mut work = MAX_PICK_SEGMENTS;
    sketch
        .polylines
        .iter()
        .filter(|line| available.contains(&line.member))
        .filter_map(|line| {
            work = work.checked_sub(line.points.len())?;
            Some((
                line.member.clone(),
                line.points
                    .iter()
                    .map(|point| crate::sketch::world(&sketch.plane, &matrix, *point))
                    .collect(),
            ))
        })
        .collect()
}
/// Hit testing retains the evaluated polyline's exact authored member identity.
/// The polyline locates the click only; it never becomes structural geometry.
pub fn capture_click(
    w: &mut Workspace,
    rect: egui::Rect,
    point: egui::Pos2,
    _modifiers: egui::Modifiers,
) -> bool {
    if !active(w) || !w.fields.get(PICK).is_some_and(|value| value == "true") {
        return false;
    }
    let mut nearest: Option<(Id, f32)> = None;
    for (id, line) in path_lines(w) {
        for pair in line.windows(2) {
            let (Some(a), Some(b)) = (
                viewport::project(w, rect, pair[0]),
                viewport::project(w, rect, pair[1]),
            ) else {
                continue;
            };
            let d = b - a;
            let t = if d.length_sq() > 0. {
                ((point - a).dot(d) / d.length_sq()).clamp(0., 1.)
            } else {
                0.
            };
            let distance = point.distance(a + d * t);
            if distance <= 10.
                && nearest
                    .as_ref()
                    .is_none_or(|(_, previous)| distance < *previous)
            {
                nearest = Some((id.clone(), distance));
            }
        }
    }
    let Some((id, _)) = nearest else {
        return true;
    };
    let element = semantics(w)
        .into_iter()
        .find(|element| element.data["field"] == format!("member/{id}"));
    if let Some(element) = element {
        let selected = element.data["json"].as_bool().unwrap_or(false);
        if let Err(error) = set_value(w, &element, json!(!selected)) {
            w.status = error.to_string();
        }
    }
    true
}
pub fn overlay(w: &Workspace, ui: &egui::Ui, rect: egui::Rect) {
    let Some((owner, feature)) = current(w) else {
        return;
    };
    let Operation::Weldment { operation } = &feature.operation else {
        return;
    };
    let selected = selected_members(operation, w)
        .into_iter()
        .collect::<BTreeSet<_>>();
    let painter = ui.painter().with_clip_rect(rect);
    for (id, line) in path_lines(w) {
        let state = if selected.contains(&id) {
            "selected"
        } else {
            "ready"
        };
        let props = w.theme.properties_for(&viewport::style_target(w).child(
            &format!("structural-member:{}/path/{id}", owner.node),
            "structural-member-path",
            &["viewport-guide".into(), "structural-path".into()],
            state,
        ));
        if props.get("display").is_some_and(|v| v == "none")
            || props.get("visibility").is_some_and(|v| v == "hidden")
        {
            continue;
        }
        let color = props
            .get("color")
            .and_then(|v| crate::ui::color(v))
            .unwrap_or_else(|| {
                if selected.contains(&id) {
                    viewport::selection_color(w)
                } else {
                    egui::Color32::from_rgb(53, 121, 170)
                }
            });
        let width = controls::number(
            &props,
            "stroke-width",
            if selected.contains(&id) { 3. } else { 1.5 },
        )
        .max(0.);
        for pair in line.windows(2) {
            if let (Some(a), Some(b)) = (
                viewport::project(w, rect, pair[0]),
                viewport::project(w, rect, pair[1]),
            ) {
                painter.line_segment([a, b], egui::Stroke::new(width, color));
            }
        }
    }
}
