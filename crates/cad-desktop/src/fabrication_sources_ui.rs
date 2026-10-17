//! Explicit fabrication sources over native constructor ancestry and shared drafts.
use crate::{Workspace, presentation_controls as controls, viewport};
use bevy_egui::egui;
use cad_core::{
    CadError, Feature, Id, NodeRef, Operation, Result, TopologyRef,
    commands::Command,
    presentation::SemanticElement,
    protocol::{
        EDGE_CENTER_VERSION, EdgeCenter, EdgeData, Evaluation, MeshData, NATIVE_SELECTION_VERSION,
    },
    sheet_metal::{SheetOperation, Stock},
    weldment::WeldmentOperation,
};
use serde_json::{Value, json};
use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
};

pub const CONTRACT_VERSION: u32 = 1;
const ACTION: &str = "fabrication.source-pick";
const PICK: &str = "fabrication-source-pick";
const MAX_ROLE: usize = 512 * 1024;
const MAX_CACHE: usize = 2 * 1024 * 1024;
fn invalid(message: &str) -> CadError {
    CadError::Invalid(format!("Fabrication source: {message}"))
}
fn current(w: &Workspace) -> Option<(NodeRef, Feature)> {
    if crate::command_palette::context_editor(w) != Some("feature")
        || w.tool_checkpoint.is_none()
        || w.fields.contains_key("feature-source-invalid")
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
    matches!(
        feature.operation,
        Operation::Sheet { .. } | Operation::Weldment { .. }
    )
    .then(|| {
        (
            NodeRef {
                definition: definition.clone(),
                node: id.clone(),
            },
            feature.clone(),
        )
    })
}
fn bend(operation: &SheetOperation) -> Option<(&NodeRef, &str, Option<&NodeRef>)> {
    match operation {
        SheetOperation::EdgeFlange {
            body,
            member,
            attachment,
            ..
        }
        | SheetOperation::Hem {
            body,
            member,
            attachment,
            ..
        }
        | SheetOperation::Jog {
            body,
            member,
            attachment,
            ..
        } => Some((body, member, attachment.as_ref())),
        _ => None,
    }
}
pub fn input_body(w: &Workspace) -> Option<NodeRef> {
    let (_, feature) = current(w)?;
    let Operation::Sheet { operation } = feature.operation else {
        return None;
    };
    bend(&operation).map(|(body, _, _)| body.clone())
}
fn mode(w: &Workspace) -> Option<String> {
    let state: Value = serde_json::from_str(w.fields.get(PICK)?).ok()?;
    let (owner, _) = current(w)?;
    (state["document"] == w.session.visible().id
        && state["owner"] == json!(owner)
        && state["editor"].as_str()
            == w.fields
                .get("feature-editor-generation")
                .map(String::as_str))
    .then(|| state["kind"].as_str().map(str::to_owned))
    .flatten()
}
fn scope(w: &Workspace, owner: &NodeRef, kind: &str) -> Value {
    json!({"contract":"tcad.fabrication-source","version":CONTRACT_VERSION,"document":w.session.visible().id,
        "owner":owner,"editor":w.fields.get("feature-editor-generation"),"kind":kind})
}

/// Human labels are display only. IDs remain the sole member-selection intent.
pub fn base_member_labels(w: &Workspace, stock: &Stock) -> Vec<(Id, String)> {
    let Some(sketch) = crate::fabrication_ui::source_sketch(w, &stock.profile) else {
        return vec![];
    };
    let settings = &w.session.visible().settings;
    sketch
        .members
        .keys()
        .enumerate()
        .filter_map(|(ordinal, id)| {
            if sketch.members[id].construction {
                return None;
            }
            let cad_core::SketchCurve::Line { start, end } =
                cad_core::sketch_edit::effective_curve(&sketch, id).ok()?
            else {
                return None;
            };
            let a = sketch.points.get(&start)?;
            let b = sketch.points.get(&end)?;
            let dx = b[0] - a[0];
            let dy = b[1] - a[1];
            let length = dx.hypot(dy);
            if !length.is_finite() || length <= 0. {
                return None;
            }
            let epsilon = length * 1e-9;
            let location = if dy.abs() <= epsilon {
                format!(
                    "horizontal · Y {}",
                    settings.format_length((a[1] + b[1]) * 0.5)
                )
            } else if dx.abs() <= epsilon {
                format!(
                    "vertical · X {}",
                    settings.format_length((a[0] + b[0]) * 0.5)
                )
            } else {
                "diagonal".into()
            };
            Some((
                id.clone(),
                format!(
                    "Edge {} · {} · {location}",
                    ordinal + 1,
                    settings.format_length(length)
                ),
            ))
        })
        .collect()
}

#[derive(Clone, Default)]
struct Origins {
    walls: BTreeSet<(NodeRef, Id)>,
    caps: BTreeSet<NodeRef>,
    terminals: BTreeSet<NodeRef>,
    tangents: BTreeSet<NodeRef>,
}
#[derive(Default)]
struct OriginCache {
    bytes: usize,
    rows: BTreeMap<(NodeRef, String), Origins>,
}
thread_local! {static ORIGINS:RefCell<OriginCache>=RefCell::new(OriginCache::default());}
#[derive(Default)]
struct GuideCache {
    key: String,
    paths: Vec<Vec<[f64; 3]>>,
}
thread_local! {static GUIDE:RefCell<GuideCache>=RefCell::new(GuideCache::default());}
fn charge(work: &mut usize) -> Result<()> {
    *work = work.checked_sub(1).ok_or_else(|| {
        invalid("Source ancestry exceeds the bounded picker work; choose its named source field")
    })?;
    Ok(())
}
fn unwrap<'a>(
    mut value: &'a Value,
    mut references: &'a [Value],
    work: &mut usize,
) -> Result<(&'a Value, &'a [Value])> {
    loop {
        charge(work)?;
        if cad_core::provenance::is_closure(value) {
            if value["version"].as_u64() != Some(cad_core::provenance::VERSION) {
                return Err(invalid("Unsupported source closure"));
            }
            references = value["references"]
                .as_array()
                .ok_or_else(|| invalid("Source table is missing"))?;
            value = value
                .get("root")
                .ok_or_else(|| invalid("Source root is missing"))?;
        } else if let Some(index) = value.get("$tcad.source").and_then(Value::as_u64) {
            value = references
                .get(usize::try_from(index).map_err(|_| invalid("Invalid source table link"))?)
                .ok_or_else(|| invalid("Source table link is missing"))?;
        } else {
            return Ok((value, references));
        }
    }
}
fn intent<'a>(
    descriptor: &'a Value,
    references: &'a [Value],
    work: &mut usize,
) -> Result<Option<(&'a Value, &'a [Value])>> {
    let role = &descriptor["role"];
    if role.get("contract").and_then(Value::as_str) != Some("construction") {
        return Ok(None);
    }
    let value = role
        .get("intent")
        .ok_or_else(|| invalid("Source constructor intent is missing"))?;
    unwrap(value, references, work).map(Some)
}
fn leaves(
    value: &Value,
    references: &[Value],
    work: &mut usize,
    depth: usize,
    out: &mut BTreeSet<(NodeRef, Id)>,
) -> Result<()> {
    if depth > 256 {
        return Err(invalid("Source ancestry is too deep for a viewport pick"));
    }
    let (value, references) = unwrap(value, references, work)?;
    let Some(producer) = value.get("producer") else {
        return Ok(());
    };
    let producer: NodeRef = serde_json::from_value(producer.clone())?;
    if let Some(member) = value["role"]
        .as_str()
        .and_then(|role| role.strip_prefix("member:"))
    {
        out.insert((producer, member.into()));
        return Ok(());
    }
    if let Some((intent, references)) = intent(value, references, work)? {
        if intent["purpose"]["operation"].as_str() == Some("sketch_member")
            && let Some(member) = intent["purpose"]["member"].as_str()
        {
            out.insert((producer, member.into()));
            return Ok(());
        }
        for source in intent["sources"].as_array().into_iter().flatten() {
            leaves(source, references, work, depth + 1, out)?;
        }
    }
    Ok(())
}
fn trace(
    value: &Value,
    references: &[Value],
    work: &mut usize,
    depth: usize,
    out: &mut Origins,
) -> Result<()> {
    if depth > 256 {
        return Err(invalid("Source ancestry is too deep for a viewport pick"));
    }
    let (value, references) = unwrap(value, references, work)?;
    let Some((intent, references)) = intent(value, references, work)? else {
        return Ok(());
    };
    let purpose = &intent["purpose"];
    let operation = purpose["operation"].as_str();
    let is_wall = operation == Some("extrusion") && purpose["constructor"].get("wall").is_some();
    let is_cap = operation == Some("extrusion")
        && purpose["constructor"].get("end").is_some()
        && purpose["constructor"]["scope"]["operation"].as_str() == Some("sheet_stock_extrusion");
    let surface = purpose["surface"].as_str();
    let is_terminal = operation == Some("sheet_flange") && surface == Some("terminal");
    let is_tangent = operation == Some("sheet_flange")
        && matches!(
            surface,
            Some("upper_tangent" | "lower_tangent" | "upper_developed" | "lower_developed")
        );
    if is_wall || is_cap || is_terminal || is_tangent {
        let mut members = BTreeSet::new();
        for source in intent["sources"].as_array().into_iter().flatten() {
            leaves(source, references, work, depth + 1, &mut members)?;
        }
        if is_wall {
            out.walls.extend(members.iter().cloned());
        }
        if is_cap {
            out.caps
                .extend(members.iter().map(|(producer, _)| producer.clone()));
        }
        if is_terminal {
            out.terminals
                .extend(members.iter().map(|(producer, _)| producer.clone()));
        }
        if is_tangent {
            out.tangents
                .extend(members.iter().map(|(producer, _)| producer.clone()));
        }
    }
    // Incidence source_signature entries describe neighboring boundaries. They
    // are never direct material ancestry and cannot identify a picked member.
    for source in intent["sources"].as_array().into_iter().flatten() {
        trace(source, references, work, depth + 1, out)?;
    }
    Ok(())
}
fn origins(reference: &TopologyRef) -> Result<Origins> {
    if reference.role.len() > MAX_ROLE {
        return Err(invalid(
            "Source descriptor is too large for a viewport pick; use its named source field",
        ));
    }
    let key = (reference.producer.clone(), reference.role.clone());
    if let Some(result) = ORIGINS.with(|cache| cache.borrow().rows.get(&key).cloned()) {
        return Ok(result);
    }
    let canonical = cad_core::provenance::normalized_reference(reference)?;
    let mut out = Origins::default();
    trace(&canonical, &[], &mut 32768, 0, &mut out)?;
    ORIGINS.with(|cache| {
        let mut cache = cache.borrow_mut();
        if cache.bytes + key.1.len() > MAX_CACHE || cache.rows.len() >= 128 {
            *cache = OriginCache::default();
        }
        cache.bytes += key.1.len();
        cache.rows.insert(key, out.clone());
    });
    Ok(out)
}
fn edge_target(
    edge: &EdgeData,
    stock: &Stock,
    kind: &str,
) -> Result<Option<(Id, Option<NodeRef>)>> {
    let Some(native) = edge
        .native
        .as_ref()
        .filter(|native| native.version == NATIVE_SELECTION_VERSION)
    else {
        return Ok(None);
    };
    if !matches!(
        edge.center,
        Some(EdgeCenter::Line {
            version: EDGE_CENTER_VERSION,
            ..
        })
    ) {
        return Ok(None);
    }
    let mut found = Origins::default();
    for face in &native.boundaries {
        let value = origins(&face.face)?;
        found.walls.extend(value.walls);
        found.caps.extend(value.caps);
        found.terminals.extend(value.terminals);
        found.tangents.extend(value.tangents);
    }
    if kind == "base-edge" {
        if !found.caps.contains(&stock.profile) {
            return Ok(None);
        }
        let candidates = found
            .walls
            .into_iter()
            .filter(|(profile, member)| {
                profile == &stock.profile
                    && !stock
                        .flanges
                        .iter()
                        .any(|flange| flange.member == *member && flange.attachment.is_none())
            })
            .map(|(_, member)| member)
            .collect::<BTreeSet<_>>();
        return Ok((candidates.len() == 1).then(|| (candidates.into_iter().next().unwrap(), None)));
    }
    let candidates = stock
        .flanges
        .iter()
        .filter(|flange| {
            found.terminals.contains(&flange.source)
                && found.tangents.contains(&flange.source)
                && !stock
                    .flanges
                    .iter()
                    .any(|child| child.attachment.as_ref() == Some(&flange.source))
        })
        .collect::<Vec<_>>();
    Ok((candidates.len() == 1).then(|| {
        (
            candidates[0].member.clone(),
            Some(candidates[0].source.clone()),
        )
    }))
}
fn attachment_paths(
    evaluation: &Evaluation,
    mesh: &MeshData,
    stock: &Stock,
    body: &NodeRef,
    member: &str,
    attachment: Option<&NodeRef>,
    mesh_key: &str,
) -> Vec<Vec<[f64; 3]>> {
    let key = format!(
        "{}:{}:{mesh_key}:{body:?}:{member}:{attachment:?}",
        evaluation.identity.source, evaluation.identity.backend
    );
    if let Some(paths) = GUIDE.with(|cache| {
        let cache = cache.borrow();
        (cache.key == key).then(|| cache.paths.clone())
    }) {
        return paths;
    }
    let kind = if attachment.is_some() {
        "terminal-edge"
    } else {
        "base-edge"
    };
    let mut points_remaining = 16_384usize;
    let paths = mesh
        .edges
        .iter()
        .take(256)
        .filter(|edge| edge.body.as_ref() == Some(body))
        .filter(|edge| {
            edge_target(edge, stock, kind)
                .ok()
                .flatten()
                .is_some_and(|(id, source)| id == member && source.as_ref() == attachment)
        })
        .filter_map(|edge| {
            if edge.points.len() > points_remaining {
                return None;
            }
            points_remaining -= edge.points.len();
            Some(edge.points.clone())
        })
        .collect::<Vec<_>>();
    GUIDE.with(|cache| {
        *cache.borrow_mut() = GuideCache {
            key,
            paths: paths.clone(),
        }
    });
    paths
}
fn input_mesh<'a>(
    evaluation: &'a Evaluation,
    instance: &cad_core::PlacedPart,
    body: &NodeRef,
) -> Option<&'a MeshData> {
    evaluation
        .history
        .get(cad_core::history::mesh_key(instance))?
        .meshes
        .get(&body.node)
        .map(|mesh| mesh.as_ref())
}
fn selected_target(
    w: &Workspace,
    pick: &viewport::Selection,
    body: &NodeRef,
    kind: &str,
) -> Result<(Id, Option<NodeRef>)> {
    if pick.reference.topology != "edge"
        || pick.body.as_ref() != Some(body)
        || pick.definition != body.definition
        || !crate::selection_ui::edge_occurrence(w, &pick.path)
    {
        return Err(invalid(
            "Pick a straight perimeter edge on this sheet input and component occurrence",
        ));
    }
    let checkpoint = w
        .tool_checkpoint
        .as_ref()
        .ok_or_else(|| invalid("The source editor closed"))?;
    let evaluation = viewport::selection_evaluation(w)
        .filter(|e| e.valid && e.identity == checkpoint.identity())
        .ok_or_else(|| {
            invalid("Wait for current native input geometry before choosing a source")
        })?;
    let instance = viewport::display_instances(w, evaluation)
        .into_iter()
        .find(|instance| instance.path == pick.path && instance.definition == pick.definition)
        .ok_or_else(|| invalid("The selected component occurrence is no longer available"))?;
    let mesh = input_mesh(evaluation, instance, body)
        .ok_or_else(|| invalid("The sheet input has no native history mesh"))?;
    let mut edges = mesh.edges.iter().filter(|edge| {
        edge.body.as_ref() == Some(body)
            && cad_core::selection_sets::same_reference(&edge.reference, &pick.reference)
    });
    let edge = edges
        .next()
        .ok_or_else(|| invalid("The picked edge is no longer on this sheet input"))?;
    if edges.next().is_some() {
        return Err(invalid("The picked edge has ambiguous native ancestry"));
    }
    let stock = cad_core::sheet_metal::stock(w.session.visible(), body)?;
    edge_target(edge,&stock,kind)?.ok_or_else(||invalid(if kind=="base-edge" {
        "Pick an unused original base perimeter edge. Thickness edges, corners and cut edges do not uniquely identify a full-width base member."
    } else {"Pick the full-width free terminal edge of the desired flange, hem or jog; its name is also available in Attach bend to."}))
}
/// Presets treat a picked model edge as terminal intent only when its native
/// face incidence proves that exact full-width terminal. A flange side face or
/// thickness edge never silently requests a chained bend.
pub(crate) fn picked_attachment(w: &Workspace, stock: &Stock) -> Option<(Id, Option<NodeRef>)> {
    let pick = w
        .selected
        .as_ref()
        .filter(|pick| pick.reference.topology == "edge")?;
    let body = pick.body.as_ref()?;
    let selected_stock = cad_core::sheet_metal::stock(w.session.visible(), body).ok()?;
    if selected_stock.source != stock.source
        || selected_stock.profile != stock.profile
        || selected_stock
            .flanges
            .iter()
            .map(|flange| &flange.source)
            .collect::<Vec<_>>()
            != stock
                .flanges
                .iter()
                .map(|flange| &flange.source)
                .collect::<Vec<_>>()
    {
        return None;
    }
    let evaluation = viewport::selection_evaluation(w)?;
    let session = w
        .tool_checkpoint
        .as_ref()
        .filter(|_| input_body(w).is_some())
        .unwrap_or(&w.session);
    if !evaluation.valid || evaluation.identity != session.identity() {
        return None;
    }
    let instance = viewport::display_instances(w, evaluation)
        .into_iter()
        .find(|instance| instance.path == pick.path && instance.definition == pick.definition)?;
    let mut candidates = viewport::selection_meshes(w, evaluation, instance)
        .into_iter()
        .flat_map(|mesh| &mesh.edges)
        .filter(|edge| {
            edge.body.as_ref() == Some(body)
                && cad_core::selection_sets::same_reference(&edge.reference, &pick.reference)
        });
    let edge = candidates.next()?;
    if candidates.next().is_some() {
        return None;
    }
    edge_target(edge, stock, "terminal-edge")
        .ok()?
        .or_else(|| edge_target(edge, stock, "base-edge").ok().flatten())
}
fn publish(w: &mut Workspace, owner: &NodeRef, feature: Feature) -> Result<()> {
    let expected = feature.clone();
    w.dispatch(Command::PutFeature {
        definition: owner.definition.clone(),
        feature: feature.into(),
        output: false,
    });
    if w.session
        .visible()
        .parts
        .get(&owner.definition)
        .and_then(|part| part.features.iter().find(|f| f.id == owner.node))
        != Some(&expected)
    {
        return Err(invalid(&w.status));
    }
    w.json = serde_json::to_string_pretty(&expected)?;
    Ok(())
}
pub fn capture_selection(w: &mut Workspace, pick: &viewport::Selection) -> bool {
    let Some(kind) = mode(w) else { return false };
    if w.confirm_requested {
        w.status =
            "Wait for the current confirmation before changing the fabrication source".into();
        return true;
    }
    let result = (|| -> Result<()> {
        let (owner, mut feature) = current(w).ok_or_else(|| invalid("The source editor closed"))?;
        let Operation::Sheet { operation } = &mut feature.operation else {
            return Err(invalid(
                "Pick sources through the current fabrication editor",
            ));
        };
        let body = bend(operation)
            .map(|(body, _, _)| body.clone())
            .ok_or_else(|| invalid("This sheet feature has no bend attachment"))?;
        let (next_member, next_attachment) = selected_target(w, pick, &body, &kind)?;
        match operation.as_mut() {
            SheetOperation::EdgeFlange {
                member, attachment, ..
            }
            | SheetOperation::Hem {
                member, attachment, ..
            }
            | SheetOperation::Jog {
                member, attachment, ..
            } => {
                *member = next_member;
                *attachment = next_attachment;
            }
            _ => return Err(invalid("The attachment operation changed")),
        }
        publish(w, &owner, feature)?;
        w.edge_selection_path = Some(pick.path.clone());
        w.fields.remove(PICK);
        w.status = "Source selected from native ancestry — evaluating the updated bend".into();
        Ok(())
    })();
    if let Err(error) = result {
        w.status = error.to_string();
    }
    true
}
pub fn action(w: &mut Workspace, command: &str, args: &Value) -> bool {
    if command != ACTION {
        return false;
    }
    let result = (|| -> Result<()> {
        let (owner, feature) =
            current(w).ok_or_else(|| invalid("Open a sheet bend editor first"))?;
        if args["owner"] != json!(owner)
            || args["document"] != w.session.visible().id
            || args["editor"].as_str()
                != w.fields
                    .get("feature-editor-generation")
                    .map(String::as_str)
        {
            return Err(invalid(
                "This source action is stale; use its current editor",
            ));
        }
        if w.confirm_requested {
            return Err(invalid(
                "Wait for confirmation to finish before selecting a source",
            ));
        }
        let kind = args["kind"]
            .as_str()
            .ok_or_else(|| invalid("Choose base-edge or terminal-edge intent"))?;
        if kind == "stop" {
            w.fields.remove(PICK);
            w.status = "Source picking ended; the chosen attachment is retained".into();
            return Ok(());
        }
        let Operation::Sheet { operation } = feature.operation else {
            return Err(invalid("Open a sheet bend editor first"));
        };
        if bend(&operation).is_none() || !matches!(kind, "base-edge" | "terminal-edge") {
            return Err(invalid("Unsupported source-picking intent"));
        }
        if w.edge_selection_path.is_none() {
            w.edge_selection_path =
                crate::selection_ui::known_edge_occurrence(w, &owner.definition);
        }
        if w.edge_selection_path.is_none() {
            return Err(invalid(
                "Select the intended sheet component occurrence first",
            ));
        }
        w.fields
            .insert(PICK.into(), serde_json::to_string(&scope(w, &owner, kind))?);
        w.status = if kind == "base-edge" {
            "Pick an unused base perimeter edge on the model"
        } else {
            "Pick the full-width terminal edge of the named flange, hem or jog"
        }
        .into();
        Ok(())
    })();
    if let Err(error) = result {
        w.status = error.to_string();
    }
    true
}
pub fn sheet_controls(w: &Workspace) -> Vec<SemanticElement> {
    let Some((owner, feature)) = current(w) else {
        return vec![];
    };
    let Operation::Sheet { operation } = &feature.operation else {
        return vec![];
    };
    let Some((body, member, attachment)) = bend(operation) else {
        return vec![];
    };
    let selected = mode(w);
    let mut elements = vec![];
    for (kind, label) in [
        ("base-edge", "Pick base edge on model"),
        ("terminal-edge", "Pick flange terminal on model"),
        ("stop", "Stop source picking"),
    ] {
        if kind == "stop" && selected.is_none() {
            continue;
        }
        let args = scope(w, &owner, kind);
        elements.push(SemanticElement {id:format!("fabrication-source:{}/pick/{kind}",owner.node),role:"fabrication-source-action".into(),label:label.into(),value:None,
            state:if selected.as_deref()==Some(kind){"selected"}else if w.confirm_requested{"disabled"}else{"available"}.into(),commands:vec![ACTION.into()],relationships:vec![owner.node.clone()],
            data:json!({"group":"attachment","classes":["modeling-field","source-picker"],"actions":[{"command":ACTION,"label":label,"enabled":!w.confirm_requested,"arguments":args}]})});
    }
    let stock = cad_core::sheet_metal::stock(w.session.visible(), body).ok();
    let label = if let Some(attachment) = attachment {
        source_name(w, attachment)
            .map(|name| format!("{name} · full-width terminal edge"))
            .unwrap_or("Missing terminal attachment".into())
    } else {
        stock
            .as_ref()
            .and_then(|stock| {
                base_member_labels(w, stock)
                    .into_iter()
                    .find(|(id, _)| id == member)
                    .map(|(_, label)| label)
            })
            .unwrap_or("Missing base edge".into())
    };
    elements.push(SemanticElement {id:format!("fabrication-source:{}/chosen",owner.node),role:"fabrication-source-choice".into(),label:"Bend source".into(),value:Some(label),
        state:"draft".into(),commands:vec![],relationships:vec![owner.node.clone(),body.node.clone()],data:json!({"contract":"tcad.fabrication-source","version":CONTRACT_VERSION,"group":"attachment", "body":body,"member":member,"attachment":attachment,
            "selection_identity":"authored-member-and-source-feature","geometry_inference":false,"preview_kind":"source-reference"})});
    elements.push(SemanticElement {id:format!("fabrication-source:{}/help",owner.node),role:"fabrication-source-help".into(),label:
        if selected.as_deref()==Some("base-edge") {"Pick an unused base perimeter edge. The source guide shows the complete authored attachment extent."}
        else if selected.as_deref()==Some("terminal-edge") {"Pick a free full-width terminal edge. Thickness edges are excluded; Attach bend to also names each available source feature."}
        else {"Base edges and flange terminals are separate selection intents. Guides show input sources, independently of added/removed material previews."}.into(),value:None,state:"ready".into(),commands:vec![],relationships:vec![owner.node],data:json!({"group":"attachment"})});
    elements
}
pub(crate) fn source_name(w: &Workspace, source: &NodeRef) -> Option<String> {
    let part = w.session.visible().parts.get(&source.definition)?;
    let feature = part
        .features
        .iter()
        .find(|feature| feature.id == source.node)?;
    Some(format!("{} / {}", part.name, feature.name))
}
pub fn semantics(w: &Workspace) -> Vec<SemanticElement> {
    // Pick controls are published inside the ordinary operation-editor group.
    // Publish source guides separately to avoid duplicate semantic identities.
    let mut elements = vec![];
    let Some((owner, feature)) = current(w) else {
        return elements;
    };
    if let Operation::Sheet { operation } = &feature.operation
        && let Some((body, member, attachment)) = bend(operation)
    {
        elements.push(SemanticElement {id:format!("fabrication-source:{}/attachment",owner.node),role:"fabrication-source-guide".into(),
            label:if let Some(source)=attachment {format!("{} · terminal source",source_name(w,source).unwrap_or("Missing bend".into()))}
                else {"Base edge source".into()},value:None,state:"source".into(),commands:vec![],relationships:vec![owner.node.clone(),body.node.clone()],
            data:json!({"contract":"tcad.fabrication-source","version":CONTRACT_VERSION,"body":body,"member":member,"attachment":attachment,
                "guide_kind":if attachment.is_some(){"terminal-edge"}else{"base-edge"},"preview_kind":"source-reference",
                "native_constructor_sources":true,"authoritative":false,"classes":["source-reference",if attachment.is_some(){"terminal-edge"}else{"base-edge"}]})});
    }
    for (source, kind, members) in sketch_sources(&feature) {
        elements.push(SemanticElement {id:format!("fabrication-source:{}/{kind}",owner.node),role:"fabrication-source-guide".into(),label:source_name(w,&source).unwrap_or("Missing source sketch".into()),
            value:Some(kind.into()),state:"source".into(),commands:vec![],relationships:vec![owner.node.clone(),source.node.clone()],
            data:json!({"contract":"tcad.fabrication-source","version":CONTRACT_VERSION,"source":source,"members":members,"guide_kind":kind,"authoritative":false,"preview_kind":"source-reference", "classes":["source-reference",kind]})});
    }
    elements
}
fn sketch_sources(feature: &Feature) -> Vec<(NodeRef, &'static str, Vec<Id>)> {
    match &feature.operation {
        Operation::Sheet { operation } => match operation.as_ref() {
            SheetOperation::Base { profile, .. }
            | SheetOperation::Tab { profile, .. }
            | SheetOperation::NormalCut { profile, .. } => {
                vec![(profile.clone(), "profile", vec![])]
            }
            _ => vec![],
        },
        Operation::Weldment { operation } => match operation.as_ref() {
            WeldmentOperation::Members {
                section,
                path,
                members,
                ..
            } => vec![
                (section.clone(), "section", vec![]),
                (path.clone(), "path", members.clone()),
            ],
            WeldmentOperation::Gusset { profile, .. } => vec![(profile.clone(), "profile", vec![])],
            _ => vec![],
        },
        _ => vec![],
    }
}
fn guide(
    ui: &egui::Ui,
    w: &Workspace,
    rect: egui::Rect,
    id: &str,
    kind: &str,
    paths: impl IntoIterator<Item = Vec<[f64; 3]>>,
    work: &mut usize,
) {
    let props = controls::properties(
        id,
        "fabrication-source-guide",
        &["source-reference", kind],
        "source",
    );
    if props.get("display").is_some_and(|value| value == "none")
        || props
            .get("visibility")
            .is_some_and(|value| value == "hidden")
    {
        return;
    }
    let color = props
        .get("color")
        .and_then(|value| crate::ui::color(value))
        .unwrap_or_else(|| viewport::selection_color(w));
    let color = color.gamma_multiply(controls::number(&props, "opacity", 0.85).clamp(0., 1.));
    let stroke = egui::Stroke::new(
        controls::number(&props, "stroke-width", 2.).clamp(0., 16.),
        color,
    );
    let painter = ui.painter().with_clip_rect(rect);
    for path in paths {
        for pair in path.windows(2) {
            if *work == 0 {
                return;
            }
            for segment in crate::overlay_depth::segments(w, rect, [pair[0], pair[1]], work) {
                painter.line_segment(segment, stroke);
            }
        }
    }
}
pub fn overlay(w: &Workspace, ui: &egui::Ui, rect: egui::Rect) {
    let Some((owner, feature)) = current(w) else {
        return;
    };
    let mut work = 100_000usize;
    let mut points_remaining = 16_384usize;
    let Some(evaluation) = viewport::selection_evaluation(w).or_else(|| {
        let checkpoint = w.tool_checkpoint.as_ref()?;
        checkpoint
            .draft
            .as_ref()
            .and_then(|draft| draft.preview_evaluation())
            .or(checkpoint.evaluation.as_ref())
            .filter(|evaluation| evaluation.valid && evaluation.identity == checkpoint.identity())
    }) else {
        return;
    };
    let instances = viewport::display_instances(w, evaluation)
        .into_iter()
        .filter(|instance| {
            instance.definition == owner.definition
                && crate::selection_ui::edge_occurrence(w, &instance.path)
                && viewport::visible(w, instance)
        })
        .collect::<Vec<_>>();
    if let Operation::Sheet { operation } = &feature.operation
        && let Some((body, member, attachment)) = bend(operation)
        && let Ok(stock) = cad_core::sheet_metal::stock(w.session.visible(), body)
    {
        let kind = if attachment.is_some() {
            "terminal-edge"
        } else {
            "base-edge"
        };
        for instance in &instances {
            let Some(mesh) = input_mesh(evaluation, instance, body) else {
                continue;
            };
            let paths = attachment_paths(
                evaluation,
                mesh,
                &stock,
                body,
                member,
                attachment,
                cad_core::history::mesh_key(instance),
            )
            .into_iter()
            .filter_map(|points| {
                if points.len() > points_remaining {
                    return None;
                }
                points_remaining -= points.len();
                Some(
                    points
                        .into_iter()
                        .map(|point| viewport::placed(point, instance))
                        .collect::<Vec<_>>(),
                )
            })
            .collect::<Vec<_>>();
            guide(
                ui,
                w,
                rect,
                &format!("fabrication-source:{}/attachment", owner.node),
                kind,
                paths,
                &mut work,
            );
        }
    }
    for (source, kind, members) in sketch_sources(&feature) {
        // A bare cross-definition source does not identify an occurrence frame.
        // Keep its exact binding available without guessing a display placement.
        if source.definition != owner.definition {
            continue;
        }
        for instance in &instances {
            let Some(sketch) = evaluation.sketches.get(&format!(
                "{}::{}",
                cad_core::history::mesh_key(instance),
                source.node
            )) else {
                continue;
            };
            let paths = sketch
                .polylines
                .iter()
                .filter(|path| {
                    (!path.construction || kind == "path")
                        && (members.is_empty() || members.contains(&path.member))
                })
                .take(4096)
                .filter_map(|path| {
                    if path.points.len().saturating_add(usize::from(path.closed)) > points_remaining
                    {
                        return None;
                    }
                    points_remaining -= path.points.len() + usize::from(path.closed);
                    let mut points = path
                        .points
                        .iter()
                        .map(|point| crate::sketch::world(&sketch.plane, &instance.matrix, *point))
                        .collect::<Vec<_>>();
                    if path.closed && points.len() > 1 {
                        points.push(points[0]);
                    }
                    Some(points)
                })
                .collect::<Vec<_>>();
            guide(
                ui,
                w,
                rect,
                &format!("fabrication-source:{}/{kind}", owner.node),
                kind,
                paths,
                &mut work,
            );
        }
    }
}
