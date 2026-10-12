//! Docked curve-pattern authoring over the shared feature draft/checkpoint.
use crate::{Workspace, cad_ui, reference_ui, ui};
use bevy_egui::egui;
use cad_core::{
    Document, Feature, Id, NodeRef, Operation, Profile, Sketch,
    commands::Command,
    curve_pattern::{self, Definition, Orientation, Path, Spacing},
};
const DEFERRED: &str = "tcad:curve-pattern-preset:";
fn effective(doc: &Document) -> cad_core::Result<std::borrow::Cow<'_, Document>> {
    if doc.active_configurations.is_empty() {
        Ok(std::borrow::Cow::Borrowed(doc))
    } else {
        cad_core::configuration::effective(doc).map(std::borrow::Cow::Owned)
    }
}
fn spatial<'a>(
    doc: &'a Document,
    reference: &NodeRef,
) -> Option<&'a cad_core::sketch3d::Definition> {
    let feature = doc
        .parts
        .get(&reference.definition)?
        .features
        .iter()
        .find(|feature| feature.id == reference.node)?;
    match &feature.operation {
        Operation::Sketch3d { definition } => Some(definition),
        _ => None,
    }
}
#[derive(Default)]
struct SpatialPathInfo {
    valid: bool,
    closed: bool,
    endpoints: Vec<Id>,
    closed_vertices: Vec<Id>,
}
/// This source-level choice aid checks explicit endpoint connectivity. The
/// native path still validates curves, coincidence solving and frame limits.
fn spatial_path_info(
    doc: &Document,
    owner: &str,
    source: &cad_core::sketch3d::Definition,
    selected: Option<&str>,
) -> SpatialPathInfo {
    use cad_core::sketch3d::Endpoint;
    use std::collections::{BTreeMap, BTreeSet};
    let Ok(aliases) = cad_core::sketch3d::endpoint_aliases(doc, owner, source) else {
        return SpatialPathInfo::default();
    };
    let members = source
        .members
        .iter()
        .filter(|(id, member)| {
            selected.map_or(!member.construction, |selected| selected == id.as_str())
        })
        .collect::<Vec<_>>();
    if members.is_empty() {
        return SpatialPathInfo::default();
    }
    if members.len() == 1 && matches!(members[0].1.curve, cad_core::sketch3d::Curve::Circle { .. })
    {
        return SpatialPathInfo {
            valid: true,
            closed: true,
            endpoints: vec![],
            closed_vertices: vec![],
        };
    }
    let mut degree = BTreeMap::<Endpoint, (usize, Id)>::new();
    let mut graph = BTreeMap::<Endpoint, BTreeSet<Endpoint>>::new();
    for (_, member) in members {
        let (a, b) = match &member.curve {
            cad_core::sketch3d::Curve::Line { start, end }
            | cad_core::sketch3d::Curve::Arc { start, end, .. } => (start, end),
            cad_core::sketch3d::Curve::Bezier { points }
            | cad_core::sketch3d::Curve::Nurbs { points, .. }
            | cad_core::sketch3d::Curve::InterpolatingSpline { points, .. } => {
                let (Some(a), Some(b)) = (points.first(), points.last()) else {
                    return SpatialPathInfo::default();
                };
                (a, b)
            }
            _ => return SpatialPathInfo::default(),
        };
        let a_key = Endpoint::Point { point: a.clone() };
        let b_key = Endpoint::Point { point: b.clone() };
        let a_key = aliases.get(&a_key).unwrap_or(&a_key).clone();
        let b_key = aliases.get(&b_key).unwrap_or(&b_key).clone();
        degree.entry(a_key.clone()).or_insert((0, a.clone())).0 += 1;
        degree.entry(b_key.clone()).or_insert((0, b.clone())).0 += 1;
        graph
            .entry(a_key.clone())
            .or_default()
            .insert(b_key.clone());
        graph.entry(b_key).or_default().insert(a_key);
    }
    let Some(first) = graph.keys().next().cloned() else {
        return SpatialPathInfo::default();
    };
    let mut seen = BTreeSet::new();
    let mut pending = vec![first];
    while let Some(point) = pending.pop() {
        if seen.insert(point.clone())
            && let Some(links) = graph.get(&point)
        {
            pending.extend(links.iter().filter(|point| !seen.contains(*point)).cloned());
        }
    }
    let endpoints = degree
        .iter()
        .filter(|(_, (degree, _))| *degree == 1)
        .map(|(_, (_, point))| point.clone())
        .collect::<Vec<_>>();
    let valid = seen.len() == degree.len()
        && degree.values().all(|(degree, _)| *degree <= 2)
        && (endpoints.len() == 2 || endpoints.is_empty());
    let closed = valid && endpoints.is_empty();
    SpatialPathInfo {
        valid,
        closed,
        endpoints,
        closed_vertices: if closed {
            degree.values().map(|(_, point)| point.clone()).collect()
        } else {
            vec![]
        },
    }
}
#[derive(serde::Serialize, serde::Deserialize)]
struct Pending {
    document: Id,
    definition: Id,
    body: Option<NodeRef>,
    path: Option<Path>,
}
pub fn is_preset(name: &str) -> bool {
    name == "Curve Pattern" || name.starts_with(DEFERRED)
}
fn sketch<'a>(doc: &'a Document, reference: &NodeRef) -> Option<&'a Sketch> {
    let part = doc.parts.get(&reference.definition)?;
    let Operation::Profile {
        profile: Profile::Sketch { sketch },
        ..
    } = &part
        .features
        .iter()
        .find(|feature| feature.id == reference.node)?
        .operation
    else {
        return None;
    };
    part.sketches.get(sketch)
}
fn initial_path(doc: &Document, profile: NodeRef) -> Path {
    if let Some(source) = spatial(doc, &profile) {
        let material = source
            .members
            .iter()
            .filter(|(_, member)| !member.construction)
            .map(|(id, _)| id.clone())
            .collect::<Vec<_>>();
        let member = (material.len() == 1).then(|| material[0].clone());
        let start_vertex = spatial_path_info(doc, &profile.definition, source, member.as_deref())
            .endpoints
            .into_iter()
            .next();
        return Path::Sketch {
            profile,
            member,
            start_vertex,
        };
    }
    let member = sketch(doc, &profile).and_then(|sketch| {
        let suppressed = cad_core::sketch_transform::suppressed_members(sketch);
        let members = sketch
            .members
            .iter()
            .filter(|(id, m)| !m.construction && !suppressed.contains(*id))
            .map(|(id, _)| id.clone())
            .collect::<Vec<_>>();
        (members.len() == 1).then(|| members[0].clone())
    });
    let start_vertex = sketch(doc, &profile)
        .and_then(|sketch| curve_pattern::sketch_endpoints(sketch, member.as_deref()).ok())
        .and_then(|endpoints| endpoints.first().cloned());
    Path::Sketch {
        profile,
        member,
        start_vertex,
    }
}
fn paths(doc: &Document, owner: &str, current: &str) -> Vec<(Path, String)> {
    doc.parts
        .get(owner)
        .into_iter()
        .flat_map(|part| {
            part.features
                .iter()
                .take_while(move |feature| feature.id != current)
                .filter(move |feature| {
                    !cad_core::configuration::feature_suppressed(doc, &part.id, feature)
                })
                .filter_map(move |feature| match &feature.operation {
                    Operation::ReferenceCurve { .. } => Some((
                        Path::ReferenceCurve {
                            feature: part.reference(&feature.id),
                        },
                        feature.name.clone(),
                    )),
                    Operation::Profile {
                        profile: Profile::Sketch { .. },
                        ..
                    } => Some((
                        initial_path(doc, part.reference(&feature.id)),
                        feature.name.clone(),
                    )),
                    Operation::Sketch3d { .. } => Some((
                        initial_path(doc, part.reference(&feature.id)),
                        format!("{} · 3D sketch", feature.name),
                    )),
                    _ => None,
                })
        })
        .collect()
}
fn bodies(doc: &Document, owner: &str, current: &str) -> Vec<(NodeRef, String)> {
    doc.parts
        .get(owner)
        .into_iter()
        .flat_map(|part| {
            part.features
                .iter()
                .take_while(move |feature| feature.id != current)
                .filter(move |feature| {
                    !cad_core::configuration::feature_suppressed(doc, &part.id, feature)
                        && feature.operation.produces_solid()
                })
                .map(move |feature| (part.reference(&feature.id), feature.name.clone()))
        })
        .collect()
}
fn selected_path(w: &Workspace) -> Option<Path> {
    let configured = effective(w.session.visible()).ok()?;
    let doc = configured.as_ref();
    if let Some(state) = &w.sketch3d {
        let profile = NodeRef {
            definition: state.owner.clone(),
            node: state.feature.clone(),
        };
        let source = spatial(doc, &profile)?;
        let members = state
            .selection
            .iter()
            .filter_map(|entity| match entity {
                crate::sketch3d_ui::Entity::Member { member } => Some(member.clone()),
                _ => None,
            })
            .collect::<Vec<_>>();
        let vertices = state
            .selection
            .iter()
            .filter_map(|entity| match entity {
                crate::sketch3d_ui::Entity::Point { point } => Some(point.clone()),
                _ => None,
            })
            .collect::<Vec<_>>();
        let member = if members.len() == 1 {
            Some(members[0].clone())
        } else if members.is_empty() {
            let material = source
                .members
                .iter()
                .filter(|(_, member)| !member.construction)
                .map(|(id, _)| id.clone())
                .collect::<Vec<_>>();
            (material.len() == 1).then(|| material[0].clone())
        } else {
            None
        };
        let info = spatial_path_info(doc, &profile.definition, source, member.as_deref());
        let aliases =
            cad_core::sketch3d::endpoint_aliases(doc, &profile.definition, source).ok()?;
        let chosen = vertices
            .first()
            .filter(|_| vertices.len() == 1)
            .and_then(|point| {
                let key = cad_core::sketch3d::Endpoint::Point {
                    point: point.clone(),
                };
                let wanted = aliases.get(&key).unwrap_or(&key);
                info.endpoints
                    .iter()
                    .chain(&info.closed_vertices)
                    .find(|candidate| {
                        let key = cad_core::sketch3d::Endpoint::Point {
                            point: (*candidate).clone(),
                        };
                        aliases.get(&key).unwrap_or(&key) == wanted
                    })
                    .cloned()
            });
        return Some(Path::Sketch {
            profile,
            member,
            start_vertex: chosen.or_else(|| info.endpoints.first().cloned()),
        });
    }
    let state = w.sketch.as_ref()?;
    let profile = NodeRef {
        definition: state.definition.clone(),
        node: state.feature.clone(),
    };
    let mut path = initial_path(doc, profile);
    let members = state
        .selection
        .iter()
        .filter_map(|entity| {
            if let cad_core::sketch_actions::SketchEntity::Member { member } = entity {
                Some(member.clone())
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    let vertices = state
        .selection
        .iter()
        .filter_map(|entity| {
            if let cad_core::sketch_actions::SketchEntity::Point { point } = entity {
                Some(point.clone())
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    if let Path::Sketch {
        profile,
        member,
        start_vertex,
    } = &mut path
    {
        if members.len() == 1 {
            *member = Some(members[0].clone());
        } else if members.len() > 1 {
            *member = None;
        }
        if let Some(source) = sketch(doc, profile) {
            let endpoints =
                curve_pattern::sketch_endpoints(source, member.as_deref()).unwrap_or_default();
            *start_vertex = if vertices.len() == 1 && endpoints.contains(&vertices[0]) {
                Some(vertices[0].clone())
            } else {
                endpoints.first().cloned()
            };
        }
    }
    Some(path)
}
fn encode(w: &mut Workspace, pending: &Pending) -> bool {
    match serde_json::to_string(pending) {
        Ok(payload) => {
            w.after_confirm = Some(format!("{DEFERRED}{payload}"));
            true
        }
        Err(error) => {
            w.status = error.to_string();
            false
        }
    }
}
fn spatial_fields(
    ui: &mut egui::Ui,
    doc: &Document,
    owner: &str,
    source: &cad_core::sketch3d::Definition,
    member: &mut Option<Id>,
    start_vertex: &mut Option<Id>,
) -> bool {
    let mut changed = false;
    let info = spatial_path_info(doc, owner, source, member.as_deref());
    let whole = spatial_path_info(doc, owner, source, None);
    let selected = member
        .as_ref()
        .and_then(|id| source.members.keys().position(|candidate| candidate == id));
    let label = member
        .as_ref()
        .map(|_| {
            selected
                .map(|index| format!("Spatial member {}", index + 1))
                .unwrap_or_else(|| "Missing Spatial Member".into())
        })
        .unwrap_or_else(|| {
            if info.closed {
                "Complete Closed Spatial Path"
            } else {
                "Complete Spatial Path"
            }
            .into()
        });
    egui::ComboBox::from_label("Path Members")
        .selected_text(label)
        .show_ui(ui, |ui| {
            if ui
                .selectable_label(
                    member.is_none(),
                    if whole.closed {
                        "Complete Closed Spatial Path"
                    } else {
                        "Complete Spatial Path"
                    },
                )
                .clicked()
                && member.is_some()
            {
                *member = None;
                *start_vertex = whole.endpoints.first().cloned();
                changed = true;
            }
            for (index, (id, record)) in source.members.iter().enumerate() {
                let kind = match &record.curve {
                    cad_core::sketch3d::Curve::Line { .. } => "Line",
                    cad_core::sketch3d::Curve::Arc { .. } => "Arc",
                    cad_core::sketch3d::Curve::Circle { .. } => "Circle",
                    cad_core::sketch3d::Curve::Bezier { .. } => "Bézier",
                    cad_core::sketch3d::Curve::Nurbs { .. } => "NURBS",
                    cad_core::sketch3d::Curve::InterpolatingSpline { .. } => "Interpolating spline",
                };
                if ui
                    .selectable_label(
                        member.as_ref() == Some(id),
                        format!(
                            "{kind} {}{}",
                            index + 1,
                            if record.construction {
                                " · construction"
                            } else {
                                ""
                            }
                        ),
                    )
                    .clicked()
                    && member.as_ref() != Some(id)
                {
                    *member = Some(id.clone());
                    *start_vertex = spatial_path_info(doc, owner, source, Some(id))
                        .endpoints
                        .into_iter()
                        .next();
                    changed = true;
                }
            }
        });
    let info = spatial_path_info(doc, owner, source, member.as_deref());
    if !info.valid {
        ui.small("Choose one spatial member or one connected nonbranching path; unrelated islands and branches require an ordered Composite Curve.");
    }
    let vertices = if info.closed {
        &info.closed_vertices
    } else {
        &info.endpoints
    };
    if !vertices.is_empty() {
        let label = start_vertex
            .as_ref()
            .and_then(|id| vertices.iter().position(|point| point == id))
            .map(|index| {
                format!(
                    "{} {}",
                    if info.closed { "Vertex" } else { "Endpoint" },
                    index + 1
                )
            })
            .unwrap_or_else(|| {
                if info.closed {
                    "Native Starting Phase"
                } else {
                    "Choose Endpoint"
                }
                .into()
            });
        egui::ComboBox::from_label("Path Start Vertex")
            .selected_text(label)
            .show_ui(ui, |ui| {
                if info.closed {
                    changed |= ui
                        .selectable_value(start_vertex, None, "Native Starting Phase")
                        .changed();
                }
                for (index, point) in vertices.iter().enumerate() {
                    changed |= ui
                        .selectable_value(
                            start_vertex,
                            Some(point.clone()),
                            format!(
                                "{} {}",
                                if info.closed { "Vertex" } else { "Endpoint" },
                                index + 1
                            ),
                        )
                        .changed();
                }
            });
    }
    if info.closed {
        ui.small("Closed spatial path: the full 0–1 range omits the duplicate closing copy.");
        if start_vertex.is_some() && ui.button("Use Native Closed Starting Phase").clicked() {
            *start_vertex = None;
            changed = true;
        }
    }
    changed
}
pub fn remember_path(w: &mut Workspace) {
    let Some(payload) = w
        .after_confirm
        .as_deref()
        .and_then(|value| value.strip_prefix(DEFERRED))
    else {
        return;
    };
    let Ok(mut pending) = serde_json::from_str::<Pending>(payload) else {
        return;
    };
    let Some(path) = selected_path(w) else {
        return;
    };
    if pending.document != w.session.visible().id
        || pending.definition != path.dependency().definition
    {
        w.after_confirm = None;
        return;
    }
    pending.path = Some(path);
    encode(w, &pending);
}
pub fn fields(
    ui: &mut egui::Ui,
    w: &Workspace,
    doc: &Document,
    current: &str,
    definition: &mut Definition,
) -> bool {
    let configured = match effective(doc) {
        Ok(document) => document,
        Err(error) => {
            ui.small(format!("Configured path choices unavailable: {error}"));
            return false;
        }
    };
    let doc = configured.as_ref();
    let owner = w
        .view
        .active_part
        .clone()
        .unwrap_or_else(|| definition.body.definition.clone());
    let mut changed = false;
    let body_choices = bodies(doc, &owner, current);
    egui::ComboBox::from_label("Bodies to Pattern")
        .selected_text(
            body_choices
                .iter()
                .find(|(body, _)| body == &definition.body)
                .map(|(_, label)| label.as_str())
                .unwrap_or("Missing Body"),
        )
        .show_ui(ui, |ui| {
            for (body, label) in body_choices {
                changed |= ui
                    .selectable_value(&mut definition.body, body, label)
                    .changed();
            }
        });
    let path_choices = paths(doc, &owner, current);
    egui::ComboBox::from_label("Curve / Sketch Path")
        .selected_text(
            path_choices
                .iter()
                .find(|(path, _)| path.dependency() == definition.path.dependency())
                .map(|(_, label)| label.as_str())
                .unwrap_or("Missing Path"),
        )
        .show_ui(ui, |ui| {
            for (path, label) in path_choices {
                if ui
                    .selectable_label(path.dependency() == definition.path.dependency(), label)
                    .clicked()
                    && path.dependency() != definition.path.dependency()
                {
                    definition.path = path;
                    changed = true;
                }
            }
        });
    if let Path::Sketch {
        profile,
        member,
        start_vertex,
    } = &mut definition.path
        && let Some(source) = sketch(doc, profile)
    {
        let suppressed = cad_core::sketch_transform::suppressed_members(source);
        let material = source
            .members
            .iter()
            .enumerate()
            .filter(|(_, (id, m))| !m.construction && !suppressed.contains(*id))
            .collect::<Vec<_>>();
        let label = member
            .as_ref()
            .map(|selected| {
                material
                    .iter()
                    .find(|(_, (id, _))| *id == selected)
                    .map(|(i, _)| format!("Member {}", i + 1))
                    .unwrap_or_else(|| "Missing Member".into())
            })
            .unwrap_or_else(|| "Complete Open Path".into());
        egui::ComboBox::from_label("Path Members")
            .selected_text(label)
            .show_ui(ui, |ui| {
                if ui
                    .selectable_label(member.is_none(), "Complete Open Path")
                    .clicked()
                    && member.is_some()
                {
                    *member = None;
                    *start_vertex = curve_pattern::sketch_endpoints(source, None)
                        .ok()
                        .and_then(|points| points.first().cloned());
                    changed = true;
                }
                for (index, (id, record)) in &material {
                    let kind = match record.curve {
                        cad_core::SketchCurve::Line { .. } => "Line",
                        cad_core::SketchCurve::Arc { .. } => "Arc",
                        cad_core::SketchCurve::Circle { .. } => "Circle",
                        cad_core::SketchCurve::Ellipse { .. } => "Ellipse",
                        _ => "Spline",
                    };
                    if ui
                        .selectable_label(
                            member.as_ref() == Some(*id),
                            format!("{kind} {}", index + 1),
                        )
                        .clicked()
                        && member.as_ref() != Some(*id)
                    {
                        *member = Some((*id).clone());
                        *start_vertex = curve_pattern::sketch_endpoints(source, Some(id))
                            .ok()
                            .and_then(|points| points.first().cloned());
                        changed = true;
                    }
                }
            });
        let endpoints =
            curve_pattern::sketch_endpoints(source, member.as_deref()).unwrap_or_default();
        if !endpoints.is_empty() {
            let label = start_vertex
                .as_ref()
                .and_then(|point| endpoints.iter().position(|candidate| candidate == point))
                .map(|i| format!("Endpoint {}", i + 1))
                .unwrap_or_else(|| "Choose Endpoint".into());
            egui::ComboBox::from_label("Path Start Vertex")
                .selected_text(label)
                .show_ui(ui, |ui| {
                    for (index, point) in endpoints.iter().enumerate() {
                        changed |= ui
                            .selectable_value(
                                start_vertex,
                                Some(point.clone()),
                                format!("Endpoint {}", index + 1),
                            )
                            .changed();
                    }
                });
        } else if start_vertex.is_some() && ui.button("Use Native Closed Starting Phase").clicked()
        {
            *start_vertex = None;
            changed = true;
        }
    }
    if let Path::Sketch {
        profile,
        member,
        start_vertex,
    } = &mut definition.path
        && let Some(source) = spatial(doc, profile)
    {
        changed |= spatial_fields(ui, doc, &profile.definition, source, member, start_vertex);
    }
    changed |= cad_ui::expression(ui, "Instances", &mut definition.count);
    egui::ComboBox::from_label("Spacing")
        .selected_text(match definition.spacing {
            Spacing::EqualParameter => "Equal Parameter",
            Spacing::ExactArcLength => "Equal Distance (Exact)",
        })
        .show_ui(ui, |ui| {
            changed |= ui
                .selectable_value(
                    &mut definition.spacing,
                    Spacing::EqualParameter,
                    "Equal Parameter",
                )
                .changed();
            changed |= ui
                .selectable_value(
                    &mut definition.spacing,
                    Spacing::ExactArcLength,
                    "Equal Distance (Exact)",
                )
                .changed();
        });
    ui.small(match definition.spacing {Spacing::EqualParameter=>"Space by the curve's saved parameter intervals. The distances may differ.",Spacing::ExactArcLength=>"Equal distance is supported for straight lines, circular arcs and native helices or spirals. General spline/ellipse distance is unavailable."});
    ui.horizontal(|ui| {
        changed |= cad_ui::expression(ui, "Start", &mut definition.range[0]);
        changed |= cad_ui::expression(ui, "End", &mut definition.range[1]);
    });
    ui.small("Path range: 0 to 1. A full closed curve excludes the duplicate closing position.");
    changed |= ui
        .checkbox(&mut definition.reverse, "Reverse Path Direction")
        .changed();
    egui::ComboBox::from_label("Orientation")
        .selected_text(match definition.orientation {
            Orientation::Fixed => "Fixed",
            Orientation::FollowTangent => "Follow Tangent",
        })
        .show_ui(ui, |ui| {
            changed |= ui
                .selectable_value(&mut definition.orientation, Orientation::Fixed, "Fixed")
                .changed();
            changed |= ui
                .selectable_value(
                    &mut definition.orientation,
                    Orientation::FollowTangent,
                    "Follow Tangent",
                )
                .changed();
        });
    if definition.orientation == Orientation::FollowTangent {
        ui.small("Keep the first body's orientation and rotate later bodies with the tangent. Frame Up controls roll and must stay away from the tangent.");
        for (axis, expr) in definition.normal.iter_mut().enumerate() {
            changed |= cad_ui::expression(ui, &format!("Frame Up {}", ["X", "Y", "Z"][axis]), expr);
        }
        ui.small("Smooth path junctions are required. Fixed orientation supports sharp corners.");
    }
    changed |= ui
        .checkbox(&mut definition.align_seed, "Align Seed Point to Path Start")
        .changed();
    if definition.align_seed {
        changed |= reference_ui::point(ui, w, doc, current, "Seed Point", &mut definition.seed);
    } else {
        ui.small("Preserve the body's offset from the first path station. Following tangent rotates that offset with the body.");
    }
    changed |= ui
        .checkbox(&mut definition.keep_source, "Keep Source Body")
        .changed();
    ui.small("Copies remain separate bodies. Keeping an unchanged first station retains the original once.");
    egui::CollapsingHeader::new("Skip Instances").show(ui,|ui| {
        let count=cad_core::expression::value(doc,&owner,&definition.count,cad_core::Dimension::Scalar).ok().filter(|n|n.is_finite() && n.fract()==0. && *n>=1. && *n<=curve_pattern::MAX_INSTANCES as f64).map(|n|n as usize).unwrap_or(0);
        egui::ScrollArea::vertical().max_height(180.).show_rows(ui,ui.spacing().interact_size.y,count,|ui,rows| {
            for index in rows {let mut skipped=definition.skipped.contains(&index);
                if ui.checkbox(&mut skipped,format!("Skip Instance {}",index+1)).changed() {if skipped {definition.skipped.push(index);definition.skipped.sort_unstable();}else{definition.skipped.retain(|slot|*slot!=index);}changed=true;}
            }
        });
        let dormant=definition.skipped.iter().filter(|slot|**slot>=count).count();
        if dormant>0 {ui.small(format!("{dormant} skipped slots are reserved above the current count and return when it grows."));
            if ui.button("Clear Reserved Skips").clicked() {definition.skipped.retain(|slot|*slot<count);changed=true;}
        }
        if !definition.skipped.is_empty() && ui.button("Clear All Skips").clicked() {definition.skipped.clear();changed=true;}
    });
    changed
}
fn eligible(doc: &Document, path: &Path) -> bool {
    match path {
        Path::ReferenceCurve { .. } => true,
        Path::Sketch {
            profile, member, ..
        } if spatial(doc, profile).is_some() => spatial(doc, profile).is_some_and(|source| {
            spatial_path_info(doc, &profile.definition, source, member.as_deref()).valid
        }),
        Path::Sketch { profile, .. } => sketch(doc, profile).is_some_and(|source| {
            let suppressed = cad_core::sketch_transform::suppressed_members(source);
            source
                .members
                .iter()
                .filter(|(id, m)| !m.construction && !suppressed.contains(*id))
                .count()
                == 1
                || curve_pattern::sketch_endpoints(source, None)
                    .is_ok_and(|points| points.len() == 2)
        }),
    }
}
fn initial_normal(doc: &Document, path: &Path) -> [cad_core::Expr; 3] {
    if let Path::Sketch {
        profile, member, ..
    } = path
        && let Some(source) = spatial(doc, profile)
    {
        let u = bevy::math::DVec3::from_array(source.workplane.u);
        let v = bevy::math::DVec3::from_array(source.workplane.v);
        let normal = u.cross(v);
        let first = member
            .as_ref()
            .and_then(|id| source.members.get(id))
            .or_else(|| source.members.values().find(|member| !member.construction));
        let tangent = first
            .and_then(|member| {
                cad_core::sketch3d::curve_jet(doc, &profile.definition, source, &member.curve, 0.)
                    .ok()
            })
            .map(|jet| bevy::math::DVec3::from_array(jet.tangent));
        let normal = if tangent
            .is_some_and(|tangent| normal.cross(tangent).length() <= 1e-10 * tangent.length())
        {
            u
        } else {
            normal
        };
        return normal
            .to_array()
            .map(|value| cad_core::Expr(value.to_string()));
    }
    if let Path::Sketch { profile, .. } = path
        && let Some(part) = doc.parts.get(&profile.definition)
        && let Some(feature) = part
            .features
            .iter()
            .find(|feature| feature.id == profile.node)
        && let Operation::Profile { plane, .. } = &feature.operation
    {
        let a = plane.u;
        let b = plane.v;
        let normal = [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ];
        return normal.map(|value| cad_core::Expr(format!("{value:.17}")));
    }
    curve_pattern::default_normal()
}
pub fn preset(w: &mut Workspace, name: &str) {
    if !is_preset(name) {
        return;
    }
    if [
        "configuration-editor",
        "component-pattern-editor",
        "mate-editor",
    ]
    .iter()
    .any(|key| w.fields.contains_key(*key))
    {
        w.status =
            "Confirm or cancel the current configuration, component pattern or mate first".into();
        return;
    }
    let mut pending = match name
        .strip_prefix(DEFERRED)
        .map(serde_json::from_str::<Pending>)
        .transpose()
    {
        Ok(Some(pending)) => {
            if pending.document != w.session.visible().id
                || w.view.active_part.as_deref() != Some(pending.definition.as_str())
            {
                w.status="The pending Curve Pattern belongs to another document or part; invoke it again".into();
                return;
            }
            pending
        }
        Ok(None) => Pending {
            document: w.session.visible().id.clone(),
            definition: w
                .sketch3d
                .as_ref()
                .map(|state| state.owner.clone())
                .or_else(|| w.sketch.as_ref().map(|state| state.definition.clone()))
                .or_else(|| w.view.active_part.clone())
                .unwrap_or_default(),
            body: w
                .selected
                .as_ref()
                .and_then(|selection| selection.body.clone())
                .or_else(|| {
                    w.sketch
                        .as_ref()
                        .and_then(|state| state.reference_body.clone())
                }),
            path: selected_path(w),
        },
        Err(error) => {
            w.status = format!("Pending Curve Pattern is invalid: {error}");
            return;
        }
    };
    if w.session
        .visible()
        .libraries
        .values()
        .any(|link| link.pinned_definitions.contains_key(&pending.definition))
    {
        w.status = "Make this linked component unique before adding a Curve Pattern".into();
        return;
    }
    crate::history::latest(w);
    if w.confirm_requested {
        encode(w, &pending);
        return;
    }
    if w.sketch.is_some() || w.sketch3d.is_some() {
        if encode(w, &pending) {
            w.confirm_tool();
            if !w.confirm_requested
                && w.sketch.is_none()
                && w.sketch3d.is_none()
                && w.tool_checkpoint.is_none()
            {
                // An unchanged sketch closes without Apply's deferred callback.
                // Start the consumer only after that checkpoint has closed.
                w.after_confirm = None;
                if let Ok(payload) = serde_json::to_string(&pending) {
                    preset(w, &format!("{DEFERRED}{payload}"));
                }
            }
        }
        return;
    }
    if w.tool_checkpoint.is_some() {
        w.status = "Confirm or cancel the current feature first".into();
        return;
    }
    let Some(part) = w.session.visible().parts.get(&pending.definition) else {
        w.status = "Create or select a part first".into();
        return;
    };
    let choices = bodies(w.session.visible(), &part.id, "");
    let body = pending
        .body
        .take()
        .filter(|body| choices.iter().any(|(candidate, _)| candidate == body))
        .or_else(|| {
            part.features
                .iter()
                .rev()
                .find(|feature| {
                    !cad_core::configuration::feature_suppressed(
                        w.session.visible(),
                        &part.id,
                        feature,
                    ) && part.outputs.contains(&feature.id)
                        && feature.operation.produces_solid()
                })
                .map(|feature| part.reference(&feature.id))
        });
    let Some(body) = body else {
        w.status = "Create a sketch-based solid body before adding Curve Pattern".into();
        return;
    };
    let configured = match effective(w.session.visible()) {
        Ok(document) => document,
        Err(error) => {
            w.status = format!("Configured Curve Pattern sources are unavailable: {error}");
            return;
        }
    };
    let candidates = paths(configured.as_ref(), &part.id, "");
    if pending.path.as_ref().is_some_and(|path| {
        !candidates
            .iter()
            .any(|(candidate, _)| candidate.dependency() == path.dependency())
    }) {
        w.status = "The pending path is missing or suppressed; select its replacement".into();
        return;
    }
    let path = pending
        .path
        .take()
        .or_else(|| {
            candidates
                .iter()
                .find(|(path, _)| w.view.active_node.as_ref() == Some(&path.dependency().node))
                .map(|(path, _)| path.clone())
        })
        .or_else(|| {
            candidates
                .iter()
                .rev()
                .find(|(path, _)| eligible(configured.as_ref(), path))
                .map(|(path, _)| path.clone())
        });
    let Some(path) = path else {
        crate::sketch::begin(w, None);
        if w.sketch.is_some() {
            pending.body = Some(body);
            pending.path = None;
            encode(w, &pending);
            w.status =
                "Draw and constrain a curve path, then Finish Sketch to open Curve Pattern".into();
        }
        return;
    };
    let owner = part.id.clone();
    let number = part
        .features
        .iter()
        .filter(|feature| matches!(feature.operation, Operation::CurvePattern { .. }))
        .count()
        + 1;
    let mut definition = Definition::new(body, path);
    definition.normal = initial_normal(configured.as_ref(), &definition.path);
    let feature = Feature {
        id: cad_core::id(),
        name: format!("CurvePattern{number}"),
        operation: Operation::CurvePattern {
            definition: Box::new(definition),
        },
        suppressed: false,
    };
    w.start_tool();
    w.dispatch(Command::PutFeature {
        definition: owner.clone(),
        feature: feature.clone().into(),
        output: true,
    });
    if !w
        .session
        .visible()
        .parts
        .get(&owner)
        .is_some_and(|part| part.features.iter().any(|current| current == &feature))
    {
        let error = w.status.clone();
        w.cancel_tool();
        w.status = error;
        return;
    }
    w.view.active_node = Some(feature.id.clone());
    ui::feature_dialog(w, Some(feature));
}
