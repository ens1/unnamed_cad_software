//! Native contour controls over application-owned topology and shared feature drafts.
use crate::{Workspace, presentation_controls as controls};
use bevy_egui::egui;
use cad_core::{
    Document, Feature, Id, NodeRef, Operation, Profile, Sketch, SketchCurve,
    contours::Selection,
    presentation::SemanticElement,
    sketch_topology::{self, Contour, ContourStatus},
};

const DEFERRED: &str = "tcad:contour-preset:";
const ACTION: &str = "contours.edit:";

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct Pending {
    document: Id,
    pub requested_name: String,
    pub profile: NodeRef,
    members: Vec<Id>,
}
fn section_preset(name: &str) -> bool {
    matches!(
        name,
        "Extrude"
            | "Cut"
            | "Mid-plane Extrude"
            | "Two-direction Extrude"
            | "Thin Extrude"
            | "Through All Cut"
            | "Through Both Cut"
            | "Up to Face Cut"
            | "Thin Cut"
            | "Revolve"
            | "Revolved Cut"
            | "Loft"
            | "Lofted Cut"
            | "Sweep"
            | "Swept Cut"
    )
}
fn selected_members(w: &Workspace) -> Vec<Id> {
    w.sketch
        .as_ref()
        .into_iter()
        .flat_map(|sketch| &sketch.selection)
        .filter_map(|entity| match entity {
            cad_core::sketch_actions::SketchEntity::Member { member } => Some(member.clone()),
            _ => None,
        })
        .collect()
}
pub(crate) fn deferred_preset(w: &Workspace, requested_name: &str) -> String {
    let Some(sketch) = w.sketch.as_ref().filter(|_| section_preset(requested_name)) else {
        return requested_name.into();
    };
    let pending = Pending {
        document: w.session.visible().id.clone(),
        requested_name: requested_name.into(),
        profile: NodeRef {
            definition: sketch.definition.clone(),
            node: sketch.feature.clone(),
        },
        members: selected_members(w),
    };
    serde_json::to_string(&pending)
        .map(|payload| format!("{DEFERRED}{payload}"))
        .unwrap_or_else(|_| requested_name.into())
}
pub(crate) fn pending(w: &Workspace, name: &str) -> Result<Option<Pending>, String> {
    let Some(payload) = name.strip_prefix(DEFERRED) else {
        return Ok(None);
    };
    let pending: Pending = serde_json::from_str(payload).map_err(|error| error.to_string())?;
    let profile_exists = w
        .session
        .visible()
        .parts
        .get(&pending.profile.definition)
        .is_some_and(|part| {
            part.features.iter().any(|feature| {
                feature.id == pending.profile.node
                    && !cad_core::configuration::feature_suppressed(
                        w.session.visible(),
                        &part.id,
                        feature,
                    )
                    && matches!(feature.operation, Operation::Profile { .. })
            })
        });
    if !profile_exists
        || pending.document != w.session.visible().id
        || w.view.active_part.as_deref() != Some(pending.profile.definition.as_str())
        || !section_preset(&pending.requested_name)
    {
        return Err("The pending contour selection belongs to another document or part; invoke the modeling command again".into());
    }
    Ok(Some(pending))
}
pub(crate) fn remember_contours(w: &mut Workspace) {
    let Some(name) = w
        .after_confirm
        .clone()
        .filter(|name| name.starts_with(DEFERRED))
    else {
        return;
    };
    let Ok(Some(mut pending)) = pending(w, &name) else {
        w.after_confirm = None;
        return;
    };
    let Some(sketch) = &w.sketch else { return };
    if sketch.definition != pending.profile.definition || sketch.feature != pending.profile.node {
        w.after_confirm = None;
        return;
    }
    pending.members = selected_members(w);
    if let Ok(payload) = serde_json::to_string(&pending) {
        w.after_confirm = Some(format!("{DEFERRED}{payload}"));
    }
}

pub(crate) fn sketch<'a>(doc: &'a Document, profile: &NodeRef) -> Option<&'a Sketch> {
    let part = doc.parts.get(&profile.definition)?;
    let feature = part
        .features
        .iter()
        .find(|feature| feature.id == profile.node)?;
    let Operation::Profile {
        profile: Profile::Sketch { sketch },
        ..
    } = &feature.operation
    else {
        return None;
    };
    part.sketches.get(sketch)
}
pub(crate) fn members(doc: &Document, profile: &NodeRef) -> Vec<(Id, String)> {
    let Some(sketch) = sketch(doc, profile) else {
        return vec![];
    };
    let suppressed = cad_core::sketch_transform::suppressed_members(sketch);
    sketch
        .members
        .iter()
        .filter(|(id, member)| !member.construction && !suppressed.contains(*id))
        .enumerate()
        .map(|(index, (id, member))| {
            let kind = match member.curve {
                SketchCurve::Line { .. } => "Line",
                SketchCurve::Arc { .. } => "Arc",
                SketchCurve::Circle { .. } => "Circle",
                SketchCurve::Ellipse { .. } => "Ellipse",
                _ => "Spline",
            };
            (id.clone(), format!("{kind} {}", index + 1))
        })
        .collect()
}
fn profile_name(doc: &Document, profile: &NodeRef) -> String {
    doc.parts
        .get(&profile.definition)
        .and_then(|part| {
            part.features
                .iter()
                .find(|feature| feature.id == profile.node)
                .map(|feature| format!("{} / {}", part.name, feature.name))
        })
        .unwrap_or("Missing Sketch".into())
}
struct SourceChoices {
    profile: NodeRef,
    contours: Vec<Contour>,
    error: Option<String>,
    current: bool,
    native: bool,
    material_regions: bool,
}
fn evaluated_source<'a>(
    w: &'a Workspace,
    profile: &NodeRef,
) -> Option<&'a cad_core::protocol::EvaluatedSketch> {
    let identity = w.session.identity();
    let evaluation = w
        .session
        .draft
        .as_ref()
        .and_then(|draft| draft.evaluation.as_ref())
        .or(w.session.evaluation.as_ref())
        .filter(|evaluation| evaluation.identity == identity)?;
    let key = evaluation
        .instances
        .iter()
        .find(|instance| {
            instance.definition == profile.definition
                && w.view
                    .active_instance
                    .as_ref()
                    .is_none_or(|id| instance.path.contains(id))
        })
        .map(|instance| {
            if instance.mesh_key.is_empty() {
                &instance.definition
            } else {
                &instance.mesh_key
            }
        })
        .unwrap_or(&profile.definition);
    evaluation.sketches.get(&format!("{key}::{}", profile.node))
}
fn choices(w: &Workspace, doc: &Document, profiles: &[NodeRef], thin: bool) -> Vec<SourceChoices> {
    profiles
        .iter()
        .map(|profile| {
            let evaluated = evaluated_source(w, profile);
            let authored = sketch(doc, profile);
            let source = evaluated
                .and_then(|evaluated| evaluated.solved.as_ref())
                .or(authored);
            let current = evaluated.is_some_and(|evaluated| evaluated.solved.is_some());
            let material_regions =
                evaluated.is_some_and(|evaluated| !evaluated.material_regions.is_empty());
            let topology = source
                .map(|sketch| {
                    sketch_topology::analyze(sketch, sketch_topology::DEFAULT_TOLERANCE_MM)
                })
                .transpose();
            let (mut contours, error) = match topology {
                Ok(topology) => (
                    topology
                        .map(|topology| topology.contours)
                        .unwrap_or_default(),
                    None,
                ),
                Err(error) => (vec![], Some(error.to_string())),
            };
            if !thin
                && material_regions
                && let Some(evaluated) = evaluated
            {
                contours.retain(|contour| contour.status != ContourStatus::Closed);
                let mut regions = evaluated
                    .material_regions
                    .iter()
                    .map(|region| {
                        let mut members = region.outer_members.clone();
                        for hole in &region.hole_members {
                            members.extend(hole.iter().cloned());
                        }
                        // Prefer an outer-boundary anchor so deleting a hole
                        // does not invalidate an otherwise surviving region.
                        let mut seen = std::collections::BTreeSet::new();
                        members.retain(|member| seen.insert(member.clone()));
                        Contour {
                            members,
                            status: ContourStatus::Closed,
                        }
                    })
                    .filter(|region| !region.members.is_empty())
                    .collect::<Vec<_>>();
                regions.sort_by(|left, right| left.members.cmp(&right.members));
                regions.extend(contours);
                contours = regions;
            }
            SourceChoices {
                profile: profile.clone(),
                contours,
                error,
                current,
                native: authored.is_some(),
                material_regions,
            }
        })
        .collect()
}
fn ready(sources: &[SourceChoices]) -> bool {
    sources
        .iter()
        .all(|source| !source.native || source.current)
}
fn enabled(source: &SourceChoices, contour: &Contour, thin: bool) -> bool {
    eligible(contour, thin) && source.current && (thin || source.material_regions)
}
fn eligible(contour: &Contour, thin: bool) -> bool {
    contour.status == ContourStatus::Closed || thin && contour.status == ContourStatus::Open
}
fn touches(selection: &Selection, profile: &NodeRef, contour: &Contour) -> bool {
    &selection.profile == profile
        && selection
            .outer_members
            .iter()
            .any(|member| contour.members.contains(member))
}
fn included(selections: &[Selection], profile: &NodeRef, contour: &Contour, thin: bool) -> bool {
    if !selections
        .iter()
        .any(|selection| &selection.profile == profile)
    {
        return eligible(contour, thin);
    }
    selections
        .iter()
        .any(|selection| touches(selection, profile, contour))
}
fn empty_profile(selections: &mut Vec<Selection>, profile: &NodeRef) {
    if !selections
        .iter()
        .any(|selection| &selection.profile == profile)
    {
        selections.push(Selection {
            profile: profile.clone(),
            outer_members: vec![],
        });
    }
}
fn select_all(sources: &[SourceChoices], thin: bool) -> Vec<Selection> {
    let mut selections = Vec::new();
    for source in sources {
        for contour in source
            .contours
            .iter()
            .filter(|contour| enabled(source, contour, thin))
        {
            selections.push(Selection {
                profile: source.profile.clone(),
                outer_members: contour.members.first().cloned().into_iter().collect(),
            });
        }
        // Native sketches without an eligible contour must not silently become All.
        if !source.contours.is_empty() || source.error.is_some() {
            empty_profile(&mut selections, &source.profile);
        }
    }
    selections
}
fn clear(sources: &[SourceChoices]) -> Vec<Selection> {
    sources
        .iter()
        .map(|source| Selection {
            profile: source.profile.clone(),
            outer_members: vec![],
        })
        .collect()
}
fn toggle(
    sources: &[SourceChoices],
    selections: &mut Vec<Selection>,
    profile: &NodeRef,
    contour: &Contour,
    thin: bool,
) {
    let was_included = included(selections, profile, contour, thin);
    if selections.is_empty() {
        *selections = select_all(sources, thin);
    } else if !selections
        .iter()
        .any(|selection| &selection.profile == profile)
    {
        let source = sources.iter().find(|source| &source.profile == profile);
        if let Some(source) = source {
            selections.extend(select_all(std::slice::from_ref(source), thin));
        }
    }
    for selection in selections
        .iter_mut()
        .filter(|selection| &selection.profile == profile)
    {
        selection
            .outer_members
            .retain(|member| !contour.members.contains(member));
    }
    selections
        .retain(|selection| &selection.profile != profile || !selection.outer_members.is_empty());
    if !was_included {
        selections.push(Selection {
            profile: profile.clone(),
            outer_members: contour.members.first().cloned().into_iter().collect(),
        });
    }
    empty_profile(selections, profile);
}
fn selected_from_members(
    w: &Workspace,
    doc: &Document,
    profile: &NodeRef,
    selected: &[Id],
    thin: bool,
) -> Vec<Selection> {
    let sources = choices(w, doc, std::slice::from_ref(profile), thin);
    let material = members(doc, profile);
    let mut selections: Vec<Selection> = Vec::new();
    for member in selected
        .iter()
        .filter(|member| material.iter().any(|(id, _)| id == *member))
    {
        let contour = sources[0]
            .contours
            .iter()
            .find(|contour| contour.members.contains(member));
        if let Some(selection) = selections
            .iter_mut()
            .find(|selection| contour.is_some_and(|contour| touches(selection, profile, contour)))
        {
            if !selection.outer_members.contains(member) {
                selection.outer_members.push(member.clone());
            }
        } else {
            selections.push(Selection {
                profile: profile.clone(),
                outer_members: vec![member.clone()],
            });
        }
    }
    selections
}
fn definition_mut(operation: &mut Operation) -> Option<(Vec<NodeRef>, &mut Vec<Selection>, bool)> {
    match operation {
        Operation::Extrude {
            profile, options, ..
        } => Some((
            vec![profile.clone()],
            &mut options.contours,
            options.thin.is_some(),
        )),
        Operation::Revolve {
            profile, options, ..
        } => Some((
            vec![profile.clone()],
            &mut options.contours,
            options.thin.is_some(),
        )),
        Operation::Sweep {
            profile, options, ..
        } => Some((
            vec![profile.clone()],
            &mut options.contours,
            options.thin.is_some(),
        )),
        Operation::Loft {
            profiles, options, ..
        } => Some((
            profiles.clone(),
            &mut options.contours,
            options.thin.is_some(),
        )),
        _ => None,
    }
}
pub(crate) fn seed_operation(w: &Workspace, operation: &mut Operation, pending: Option<&Pending>) {
    let Some(pending) = pending else { return };
    let Some((profiles, selections, thin)) = definition_mut(operation) else {
        return;
    };
    if profiles.contains(&pending.profile) {
        *selections = selected_from_members(
            w,
            w.session.visible(),
            &pending.profile,
            &pending.members,
            thin,
        );
    }
}
fn contour_label(index: usize, contour: &Contour) -> String {
    let kind = match contour.status {
        ContourStatus::Closed => "Closed contour",
        ContourStatus::Open => "Open chain",
        ContourStatus::Branched => "Branched chain",
    };
    format!("{kind} {} · {} members", index + 1, contour.members.len())
}

pub(crate) fn fields(
    ui: &mut egui::Ui,
    w: &Workspace,
    doc: &Document,
    profiles: &[NodeRef],
    selections: &mut Vec<Selection>,
    thin: bool,
) -> bool {
    let before = selections.clone();
    let sources = choices(w, doc, profiles, thin);
    controls::label(
        ui,
        "contours.heading",
        "field-label",
        &[],
        "Selected Contours",
        "ready",
    );
    ui.horizontal_wrapped(|ui| {
        if controls::selectable(
            ui,
            "contours.mode.all",
            "contour-mode",
            if thin {
                "All chains"
            } else {
                "All closed contours"
            },
            selections.is_empty(),
        )
        .clicked()
        {
            selections.clear();
        }
        if controls::button(
            ui,
            "contours.mode.selected",
            "contour-mode",
            &["selectable"],
            "Selected",
            ready(&sources),
            !selections.is_empty(),
        )
        .clicked()
            && selections.is_empty()
        {
            *selections = select_all(&sources, thin);
        }
    });
    ui.horizontal_wrapped(|ui| {
        if controls::button(
            ui,
            "contours.select-all",
            "contour-action",
            &[],
            "Select All",
            ready(&sources),
            false,
        )
        .clicked()
        {
            *selections = select_all(&sources, thin);
        }
        if controls::button(
            ui,
            "contours.clear",
            "contour-action",
            &[],
            "Clear",
            !profiles.is_empty(),
            false,
        )
        .clicked()
        {
            *selections = clear(&sources);
        }
    });
    let props = controls::properties("contours.list", "contour-list", &["scrollable"], "ready");
    let height = controls::number(&props, "max-height", 170.).max(24.);
    egui::ScrollArea::vertical().id_salt(("contours", profiles.iter().map(|profile| (&profile.definition, &profile.node)).collect::<Vec<_>>())).max_height(height).show(ui, |ui| {
        for source in &sources {
            if sources.len() > 1 { controls::label(ui, &format!("contours.source.{}", source.profile.node), "contour-source", &[], &profile_name(doc, &source.profile), "ready"); }
            if !source.current && source.native { controls::label(ui, "contours.pending", "context-hint", &[], "Waiting for current contour preview…", "pending"); }
            if let Some(error) = &source.error { controls::label(ui, "contours.error", "diagnostic", &[], error, "error"); }
            for (index, contour) in source.contours.iter().enumerate() {
                let selected = included(selections, &source.profile, contour, thin);
                let check = |painter: &egui::Painter, center: egui::Pos2, size: f32, color: egui::Color32| {
                    let rect = egui::Rect::from_center_size(center, egui::vec2(size * 0.8, size * 0.8));
                    painter.rect_stroke(rect, 1., egui::Stroke::new(1., color), egui::StrokeKind::Inside);
                    if selected {
                        painter.line_segment([rect.left_center(), center + egui::vec2(0., size * 0.2)], egui::Stroke::new(1.5, color));
                        painter.line_segment([center + egui::vec2(0., size * 0.2), rect.right_top()], egui::Stroke::new(1.5, color));
                    }
                };
                if controls::icon_button(ui, &format!("contours.option.{}.{}", source.profile.node, contour.members[0]), "contour-option", &["selectable"], &contour_label(index, contour), enabled(source, contour, thin), selected, Some(&check))
                    .on_disabled_hover_text(if !source.current { "Waiting for the current sketch preview." } else if contour.status == ContourStatus::Branched { "Resolve branching or duplicate members before making a feature." } else { "Solid features require valid closed material regions; open chains need Thin Feature." }).clicked() {
                    toggle(&sources, selections, &source.profile, contour, thin);
                }
            }
            if source.contours.is_empty() && source.error.is_none() { ui.small("This profile uses its complete material boundary."); }
        }
    });
    let mut change = None;
    for (index, selection) in selections.iter().enumerate() {
        let group = sources
            .iter()
            .find(|source| source.profile == selection.profile)
            .and_then(|source| {
                source
                    .contours
                    .iter()
                    .enumerate()
                    .find(|(_, contour)| touches(selection, &source.profile, contour))
            });
        let label = if selection.outer_members.is_empty() {
            "No contours selected".into()
        } else if let Some((ordinal, contour)) = group {
            contour_label(ordinal, contour)
        } else {
            if ready(&sources) {
                "Missing contour member — select its replacement".into()
            } else {
                "Contour classification pending".into()
            }
        };
        ui.horizontal_wrapped(|ui| {
            // Order matters for pairing loft regions; ordinary extrudes need only the checklist.
            if profiles.len() > 1 || group.is_none() {
                controls::label(
                    ui,
                    &format!("contours.selected.{index}"),
                    "selected-contour",
                    &[],
                    &format!("{}. {label}", index + 1),
                    if group.is_none() && !ready(&sources) {
                        "pending"
                    } else if group.is_none() {
                        "error"
                    } else {
                        "selected"
                    },
                );
            }
            if profiles.len() > 1 {
                if controls::button(
                    ui,
                    &format!("contours.move-up.{index}"),
                    "contour-action",
                    &[],
                    "↑",
                    index > 0 && selections[index - 1].profile == selection.profile,
                    false,
                )
                .clicked()
                {
                    change = Some(Change::Move { index, up: true });
                }
                if controls::button(
                    ui,
                    &format!("contours.move-down.{index}"),
                    "contour-action",
                    &[],
                    "↓",
                    index + 1 < selections.len()
                        && selections[index + 1].profile == selection.profile,
                    false,
                )
                .clicked()
                {
                    change = Some(Change::Move { index, up: false });
                }
            }
            if group.is_none()
                && !selection.outer_members.is_empty()
                && controls::button(
                    ui,
                    &format!("contours.remove.{index}"),
                    "contour-action",
                    &[],
                    "Remove",
                    true,
                    false,
                )
                .clicked()
            {
                change = Some(Change::Remove { index });
            }
        });
    }
    if let Some(change) = change {
        apply_change(&sources, selections, thin, &change);
    }
    controls::label(
        ui,
        "contours.hint",
        "context-hint",
        &[],
        if thin {
            "Choose closed contours or open chains. Construction geometry is excluded."
        } else {
            "Select one or more closed contours. Nested holes follow their region; unrelated open chains are ignored in All mode."
        },
        "ready",
    );
    if profiles.len() > 1 {
        ui.small("Selected regions pair in this order within each source sketch.");
    }
    *selections != before
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Change {
    All,
    Selected,
    SelectAll,
    Clear,
    Toggle { profile: NodeRef, member: Id },
    Move { index: usize, up: bool },
    Remove { index: usize },
}
fn apply_change(
    sources: &[SourceChoices],
    selections: &mut Vec<Selection>,
    thin: bool,
    change: &Change,
) {
    match change {
        Change::All => selections.clear(),
        Change::Selected => {
            if selections.is_empty() {
                *selections = select_all(sources, thin);
            }
        }
        Change::SelectAll => *selections = select_all(sources, thin),
        Change::Clear => *selections = clear(sources),
        Change::Toggle { profile, member } => {
            if let Some((source, contour)) = sources
                .iter()
                .find(|source| &source.profile == profile)
                .and_then(|source| {
                    source
                        .contours
                        .iter()
                        .find(|contour| contour.members.contains(member))
                        .map(|contour| (source, contour))
                })
                && enabled(source, contour, thin)
            {
                toggle(sources, selections, profile, contour, thin);
            }
        }
        Change::Move { index, up } => {
            let next = if *up {
                index.checked_sub(1)
            } else {
                index.checked_add(1)
            };
            if let Some(next) = next.filter(|next| *next < selections.len())
                && *index < selections.len()
                && selections[*index].profile == selections[next].profile
            {
                selections.swap(*index, next);
            }
        }
        Change::Remove { index } => {
            if *index < selections.len() {
                let profile = selections.remove(*index).profile;
                empty_profile(selections, &profile);
            }
        }
    }
}
#[derive(serde::Serialize, serde::Deserialize)]
struct Action {
    document: Id,
    feature: Id,
    feature_source: String,
    change: Change,
}
fn command(doc: &Document, feature: &Feature, change: Change) -> String {
    serde_json::to_string(&Action {
        document: doc.id.clone(),
        feature: feature.id.clone(),
        feature_source: serde_json::to_string(&feature.operation).unwrap_or_default(),
        change,
    })
    .map(|payload| format!("{ACTION}{payload}"))
    .unwrap_or_default()
}
fn current(w: &Workspace) -> Option<(Document, Feature)> {
    if w.dialog.as_deref() != Some("feature") || w.fields.contains_key("feature-source-invalid") {
        return None;
    }
    let doc = w.session.visible().clone();
    let feature = doc
        .parts
        .get(w.view.active_part.as_ref()?)?
        .features
        .iter()
        .find(|feature| w.fields.get("feature-editor-id") == Some(&feature.id))?
        .clone();
    Some((doc, feature))
}
pub(crate) fn action(w: &mut Workspace, name: &str) -> bool {
    let Some(payload) = name.strip_prefix(ACTION) else {
        return false;
    };
    let Ok(action) = serde_json::from_str::<Action>(payload) else {
        return true;
    };
    let Some((doc, mut feature)) = current(w) else {
        return true;
    };
    if action.document != doc.id
        || action.feature != feature.id
        || serde_json::to_string(&feature.operation).ok().as_deref()
            != Some(action.feature_source.as_str())
    {
        return true;
    }
    let Some((profiles, selections, thin)) = definition_mut(&mut feature.operation) else {
        return true;
    };
    let sources = choices(w, &doc, &profiles, thin);
    if matches!(
        action.change,
        Change::Selected | Change::SelectAll | Change::Toggle { .. }
    ) && !ready(&sources)
    {
        return true;
    }
    let before = selections.clone();
    apply_change(&sources, selections, thin, &action.change);
    if *selections != before {
        w.confirm_requested = false;
        w.json = serde_json::to_string_pretty(&feature).unwrap_or_default();
        crate::ui::preview_feature(w, feature);
    }
    true
}
pub(crate) fn semantics(w: &Workspace) -> Vec<SemanticElement> {
    let Some((doc, mut feature)) = current(w) else {
        return vec![];
    };
    let Some((profiles, selections, thin)) = definition_mut(&mut feature.operation) else {
        return vec![];
    };
    let selections = selections.clone();
    let sources = choices(w, &doc, &profiles, thin);
    let mut out = vec![];
    let mut publish = |id: String,
                       role: &str,
                       label: String,
                       selected: bool,
                       enabled: bool,
                       change: Change,
                       data: serde_json::Value| {
        out.push(SemanticElement {
            id,
            role: role.into(),
            label,
            value: None,
            state: if !enabled {
                "disabled"
            } else if selected {
                "selected"
            } else {
                "ready"
            }
            .into(),
            commands: vec![command(&doc, &feature, change)],
            relationships: vec!["property-manager".into()],
            data,
        });
    };
    for (id, label, selected, change) in [
        (
            "contours.mode.all",
            if thin {
                "All chains"
            } else {
                "All closed contours"
            },
            selections.is_empty(),
            Change::All,
        ),
        (
            "contours.mode.selected",
            "Selected",
            !selections.is_empty(),
            Change::Selected,
        ),
        (
            "contours.select-all",
            "Select All",
            false,
            Change::SelectAll,
        ),
        ("contours.clear", "Clear", false, Change::Clear),
    ] {
        publish(
            id.into(),
            if id.starts_with("contours.mode.") {
                "contour-mode"
            } else {
                "contour-action"
            },
            label.into(),
            selected,
            !matches!(change, Change::Selected | Change::SelectAll) || ready(&sources),
            change,
            serde_json::json!({"presentation_control":true,"feature":feature.id,"classes":["selectable"]}),
        );
    }
    for source in &sources {
        for (index, contour) in source.contours.iter().enumerate() {
            publish(
                format!(
                    "contours.option.{}.{}",
                    source.profile.node, contour.members[0]
                ),
                "contour-option",
                contour_label(index, contour),
                included(&selections, &source.profile, contour, thin),
                enabled(source, contour, thin),
                Change::Toggle {
                    profile: source.profile.clone(),
                    member: contour.members[0].clone(),
                },
                serde_json::json!({"presentation_control":true,"classes":["selectable"],"profile":source.profile,"members":contour.members,"status":format!("{:?}",contour.status).to_lowercase(),"evaluation_current":source.current,"native_material_region":source.material_regions && !thin,"topology_contract":sketch_topology::CONTRACT_VERSION,"selection_contract":cad_core::contours::CONTRACT_VERSION,"contained_holes":"retained","profile_name":profile_name(&doc,&source.profile)}),
            );
        }
    }
    let mut selected_rows = Vec::new();
    for (index, selection) in selections.iter().enumerate() {
        selected_rows.push(SemanticElement {
            id: format!("contours.selected.{index}"),
            role: "selected-contour".into(),
            label: format!("{}. {}", index + 1, profile_name(&doc, &selection.profile)),
            value: Some(format!(
                "{} selected anchors",
                selection.outer_members.len()
            )),
            state: if selection.outer_members.is_empty() {
                "error"
            } else {
                "selected"
            }
            .into(),
            commands: vec![],
            relationships: vec!["property-manager".into()],
            data: serde_json::json!({"selection":selection,"order":index,"editable":true}),
        });
        if profiles.len() > 1 {
            for (up, label, next) in [
                (true, "Move Up", index.checked_sub(1)),
                (
                    false,
                    "Move Down",
                    index.checked_add(1).filter(|next| *next < selections.len()),
                ),
            ] {
                let enabled =
                    next.is_some_and(|next| selections[next].profile == selection.profile);
                publish(
                    format!("contours.move-{}.{index}", if up { "up" } else { "down" }),
                    "contour-action",
                    label.into(),
                    false,
                    enabled,
                    Change::Move { index, up },
                    serde_json::json!({"presentation_control":true,"selection":selection,"order":index}),
                );
            }
        }
        if !selection.outer_members.is_empty() {
            publish(
                format!("contours.remove.{index}"),
                "contour-action",
                "Remove Contour".into(),
                false,
                true,
                Change::Remove { index },
                serde_json::json!({"presentation_control":true,"selection":selection,"order":index}),
            );
        }
    }
    out.extend(selected_rows);
    out
}
