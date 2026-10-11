//! Docked native placement records using the shared feature draft/checkpoint.
use crate::{Workspace, cad_ui, reference_ui, ui};
use bevy_egui::egui;
use cad_core::{
    Document, Feature, Id, NodeRef, Operation, Profile, Sketch,
    commands::Command,
    placement_pattern::{self, Definition, Kind, Record},
    reference_geometry::PointSource,
};
use std::io::Read;
const DEFERRED: &str = "tcad:placement-pattern-preset:";
const PAGE_SIZE: usize = 24;
#[derive(serde::Serialize, serde::Deserialize)]
struct Pending {
    document: Id,
    definition: Id,
    kind: Kind,
    body: Option<NodeRef>,
    points: Vec<PointSource>,
}
pub fn is_preset(name: &str) -> bool {
    matches!(name, "Sketch Driven Pattern" | "Table Driven Pattern") || name.starts_with(DEFERRED)
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
fn points(doc: &Document, owner: &str, current: &str) -> Vec<(PointSource, String)> {
    let mut out = Vec::new();
    if let Some(part) = doc.parts.get(owner) {
        for feature in part
            .features
            .iter()
            .take_while(|feature| feature.id != current)
            .filter(|feature| !cad_core::configuration::feature_suppressed(doc, owner, feature))
        {
            let reference = part.reference(&feature.id);
            if let Some(sketch) = sketch(doc, &reference) {
                let suppressed = cad_core::sketch_transform::suppressed_points(sketch);
                for (index, (point, _)) in sketch
                    .points
                    .iter()
                    .filter(|(point, _)| !suppressed.contains(*point))
                    .enumerate()
                {
                    out.push((
                        PointSource::SketchPoint {
                            profile: reference.clone(),
                            point: point.clone(),
                        },
                        format!("{} · Point {}", feature.name, index + 1),
                    ));
                }
            }
        }
    }
    out
}
fn selected_points(w: &Workspace) -> Vec<PointSource> {
    let Some(state) = &w.sketch else {
        return vec![];
    };
    let reference = NodeRef {
        definition: state.definition.clone(),
        node: state.feature.clone(),
    };
    let Some(source) = sketch(w.session.visible(), &reference) else {
        return vec![];
    };
    let suppressed = cad_core::sketch_transform::suppressed_points(source);
    let mut ids = std::collections::BTreeSet::new();
    state
        .selection
        .iter()
        .filter_map(|entity| {
            let cad_core::sketch_actions::SketchEntity::Point { point } = entity else {
                return None;
            };
            (source.points.contains_key(point)
                && !suppressed.contains(point)
                && ids.insert(point.clone()))
            .then(|| PointSource::SketchPoint {
                profile: reference.clone(),
                point: point.clone(),
            })
        })
        .collect()
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
/// Capture the final explicit selection before Finish Sketch removes UI state.
pub fn remember_points(w: &mut Workspace) {
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
    if pending.kind != Kind::SketchDriven || w.sketch.is_none() {
        return;
    }
    if pending.document != w.session.visible().id
        || w.sketch
            .as_ref()
            .is_none_or(|state| state.definition != pending.definition)
    {
        w.after_confirm = None;
        return;
    }
    pending.points = selected_points(w);
    encode(w, &pending);
}
fn point_choice(
    ui: &mut egui::Ui,
    source: &mut PointSource,
    choices: &[(PointSource, String)],
    label: &str,
) -> bool {
    let mut changed = false;
    egui::ComboBox::from_label(label)
        .selected_text(
            choices
                .iter()
                .find(|(point, _)| point == source)
                .map(|(_, label)| label.as_str())
                .unwrap_or("Choose Sketch Point"),
        )
        .show_ui(ui, |ui| {
            for (point, label) in choices {
                changed |= ui.selectable_value(source, point.clone(), label).changed();
            }
        });
    changed
}
fn read_table(path: &std::path::Path) -> cad_core::Result<Vec<Record>> {
    let mut source = String::new();
    std::fs::File::open(path)?
        .take(placement_pattern::MAX_TABLE_BYTES as u64 + 1)
        .read_to_string(&mut source)?;
    placement_pattern::import_csv(&source)
}
fn table_files(
    ui: &mut egui::Ui,
    doc: &Document,
    owner: &str,
    current: &str,
    definition: &mut Definition,
) -> bool {
    let key = egui::Id::new(("placement-table-message", &doc.id, owner, current));
    let mut message = ui.ctx().data_mut(|data| data.get_temp::<String>(key));
    let mut changed = false;
    ui.horizontal_wrapped(|ui| {
        if ui.button("Import CSV…").clicked()
            && let Some(path) = rfd::FileDialog::new()
                .add_filter("Placement table", &["csv"])
                .pick_file()
        {
            match read_table(&path) {
                Ok(records) => {
                    definition.placements = records;
                    changed = true;
                    message = Some(format!(
                        "Imported {} placement records",
                        definition.placements.len()
                    ));
                }
                Err(error) => message = Some(error.to_string()),
            }
        }
        if ui.button("Export CSV…").clicked()
            && let Some(path) = rfd::FileDialog::new()
                .add_filter("Placement table", &["csv"])
                .set_file_name("placements.csv")
                .save_file()
        {
            match placement_pattern::table_csv(definition)
                .and_then(|source| cad_core::persistence::atomic_write(&path, source.as_bytes()))
            {
                Ok(()) => message = Some(format!("Saved {}", path.display())),
                Err(error) => message = Some(error.to_string()),
            }
        }
    });
    ui.small("CSV columns: key, x, y, z, angle, skipped. Keep each key when editing or reordering rows; quantities accept units and equations. Import replaces this feature's draft records.");
    if let Some(message) = message {
        ui.small(&message);
        ui.ctx().data_mut(|data| data.insert_temp(key, message));
    }
    changed
}
pub fn fields(
    ui: &mut egui::Ui,
    w: &Workspace,
    doc: &Document,
    current: &str,
    definition: &mut Definition,
) -> bool {
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
    changed |= reference_ui::point(ui, w, doc, current, "Seed Point", &mut definition.seed);
    ui.small("Each target locates this seed point. Rotation happens about the seed using the selected axis direction.");
    changed |= reference_ui::axis(ui, w, doc, current, "Rotation Axis", &mut definition.axis);
    changed |= ui
        .checkbox(&mut definition.keep_source, "Keep Source Body")
        .changed();
    ui.small("Copies remain separate bodies. An unchanged target at the seed reuses the retained source once. Identical active placements must be skipped or moved.");
    let point_choices = points(doc, &owner, current);
    if definition.kind == Kind::TableDriven {
        changed |= table_files(ui, doc, &owner, current, definition);
    }
    ui.separator();
    ui.strong(format!(
        "Placement Records ({})",
        definition.placements.len()
    ));
    if definition.kind == Kind::SketchDriven {
        let key = egui::Id::new(("placement-add-point", &doc.id, &owner, current));
        let mut chosen = ui.ctx().data_mut(|data| data.get_temp::<PointSource>(key));
        egui::ComboBox::from_label("Add Explicit Sketch Point")
            .selected_text(
                chosen
                    .as_ref()
                    .and_then(|point| {
                        point_choices
                            .iter()
                            .find(|(candidate, _)| candidate == point)
                    })
                    .map(|(_, label)| label.as_str())
                    .unwrap_or("Choose Point"),
            )
            .show_ui(ui, |ui| {
                for (point, label) in &point_choices {
                    if ui
                        .selectable_label(chosen.as_ref() == Some(point), label)
                        .clicked()
                    {
                        chosen = Some(point.clone());
                    }
                }
            });
        if let Some(point) = chosen.clone() {
            ui.ctx().data_mut(|data| data.insert_temp(key, point));
        }
        if ui
            .add_enabled(
                chosen.as_ref().is_some_and(|point| {
                    point_choices
                        .iter()
                        .any(|(candidate, _)| candidate == point)
                }) && definition.placements.len() < placement_pattern::MAX_PLACEMENTS,
                egui::Button::new("Add Placement"),
            )
            .clicked()
            && let Some(point) = chosen
        {
            definition.placements.push(Record::new(point));
            changed = true;
        }
        ui.small("Only points you explicitly add become placements. Drawing more sketch points does not change this feature's record set.");
    } else if ui
        .add_enabled(
            definition.placements.len() < placement_pattern::MAX_PLACEMENTS,
            egui::Button::new("Add Coordinate Row"),
        )
        .clicked()
    {
        definition
            .placements
            .push(Record::new(PointSource::Coordinates {
                coordinates: ["30 mm".into(), "0 mm".into(), "0 mm".into()],
            }));
        changed = true;
    }
    let page_key = egui::Id::new(("placement-record-page", &doc.id, &owner, current));
    let pages = definition.placements.len().div_ceil(PAGE_SIZE).max(1);
    let mut page = ui
        .ctx()
        .data_mut(|data| data.get_temp::<usize>(page_key))
        .unwrap_or(0)
        .min(pages - 1);
    if pages > 1 {
        ui.horizontal(|ui| {
            if ui
                .add_enabled(page > 0, egui::Button::new("Previous"))
                .clicked()
            {
                page -= 1;
            }
            ui.label(format!("Page {} / {pages}", page + 1));
            if ui
                .add_enabled(page + 1 < pages, egui::Button::new("Next"))
                .clicked()
            {
                page += 1;
            }
        });
    }
    ui.ctx().data_mut(|data| data.insert_temp(page_key, page));
    let count = definition.placements.len();
    let kind = definition.kind;
    let mut remove = None;
    let mut reorder = None;
    egui::ScrollArea::vertical()
        .id_salt("placement-records")
        .max_height(420.)
        .show(ui, |ui| {
            for (index, record) in definition
                .placements
                .iter_mut()
                .enumerate()
                .skip(page * PAGE_SIZE)
                .take(PAGE_SIZE)
            {
                let record_key = record.id.clone();
                ui.push_id(record_key, |ui| {
                    egui::CollapsingHeader::new(format!(
                        "Placement {}{}",
                        index + 1,
                        if record.skipped { " · Skipped" } else { "" }
                    ))
                    .default_open(count <= 4)
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            changed |= ui.checkbox(&mut record.skipped, "Skip").changed();
                            if ui
                                .add_enabled(index > 0, egui::Button::new("↑"))
                                .on_hover_text("Move row earlier; keep its identity")
                                .clicked()
                            {
                                reorder = Some((index, index - 1));
                            }
                            if ui
                                .add_enabled(index + 1 < count, egui::Button::new("↓"))
                                .on_hover_text("Move row later; keep its identity")
                                .clicked()
                            {
                                reorder = Some((index, index + 1));
                            }
                            if ui.button("Remove").clicked() {
                                remove = Some(index);
                            }
                        });
                        if kind == Kind::SketchDriven {
                            changed |= point_choice(
                                ui,
                                &mut record.target,
                                &point_choices,
                                "Target Sketch Point",
                            );
                        } else {
                            changed |= reference_ui::point(
                                ui,
                                w,
                                doc,
                                current,
                                "Target Point",
                                &mut record.target,
                            );
                        }
                        changed |= cad_ui::expression(ui, "Rotation", &mut record.angle);
                    });
                });
            }
        });
    if let Some(index) = remove {
        definition.placements.remove(index);
        changed = true;
    } else if let Some((from, to)) = reorder {
        definition.placements.swap(from, to);
        changed = true;
    }
    ui.small("Skip preserves a placement's saved key and references. Remove deletes the record; adding a new row gives it a new key. Keys are available in Advanced Source and CSV.");
    changed
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
                w.status="The pending placement pattern belongs to another document or part; invoke it again".into();
                return;
            }
            pending
        }
        Ok(None) => Pending {
            document: w.session.visible().id.clone(),
            definition: w
                .sketch
                .as_ref()
                .map(|state| state.definition.clone())
                .or_else(|| w.view.active_part.clone())
                .unwrap_or_default(),
            kind: if name == "Sketch Driven Pattern" {
                Kind::SketchDriven
            } else {
                Kind::TableDriven
            },
            body: w
                .selected
                .as_ref()
                .and_then(|selected| selected.body.clone())
                .or_else(|| {
                    w.sketch
                        .as_ref()
                        .and_then(|state| state.reference_body.clone())
                }),
            points: selected_points(w),
        },
        Err(error) => {
            w.status = format!("Pending placement pattern is invalid: {error}");
            return;
        }
    };
    if w.session
        .visible()
        .libraries
        .values()
        .any(|link| link.pinned_definitions.contains_key(&pending.definition))
    {
        w.status = "Make this linked component unique before adding a placement pattern".into();
        return;
    }
    crate::history::latest(w);
    if w.confirm_requested {
        encode(w, &pending);
        return;
    }
    if w.sketch.is_some() {
        if encode(w, &pending) {
            w.confirm_tool();
            if w.sketch.is_some() && !w.confirm_requested {
                w.after_confirm = None;
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
        w.status = "Create a sketch-based solid body before adding a placement pattern".into();
        return;
    };
    let candidates = points(w.session.visible(), &part.id, "");
    if pending.kind == Kind::SketchDriven && candidates.is_empty() {
        crate::sketch::begin(w, None);
        if w.sketch.is_some() {
            pending.body = Some(body);
            pending.points.clear();
            encode(w, &pending);
            w.status="Draw and constrain placement points, select the desired points, then Finish Sketch".into();
        }
        return;
    }
    if pending.kind == Kind::SketchDriven
        && pending
            .points
            .iter()
            .any(|point| !candidates.iter().any(|(candidate, _)| candidate == point))
    {
        w.status =
            "A selected placement point is missing or suppressed; select its replacement".into();
        return;
    }
    // A structurally valid, visible draft row lets the property manager open
    // without inferring all points or committing an unreviewed feature.
    let default_target = if pending.kind == Kind::SketchDriven && pending.points.is_empty() {
        let candidate=candidates.iter().find(|(point,_)| {
            matches!(point,PointSource::SketchPoint {profile,..} if w.view.active_node.as_ref()==Some(&profile.node))
        }).or_else(||candidates.iter().find(|(point,_)| {
            matches!(point,PointSource::SketchPoint {profile,..} if sketch(w.session.visible(),profile).is_some_and(|source|source.members.is_empty() && source.circles.is_empty() && source.segments.is_empty()))
        })).or_else(||candidates.last());
        let Some((point, label)) = candidate else {
            w.status =
                "Choose a current native sketch point before adding Sketch Driven Pattern".into();
            return;
        };
        pending.points.push(point.clone());
        Some(label.clone())
    } else {
        None
    };
    let owner = part.id.clone();
    let number = part
        .features
        .iter()
        .filter(|feature| matches!(feature.operation, Operation::PlacementPattern { .. }))
        .count()
        + 1;
    let mut definition = Definition::new(body, pending.kind);
    if pending.kind == Kind::SketchDriven {
        definition.placements = pending.points.into_iter().map(Record::new).collect();
    } else {
        definition
            .placements
            .push(Record::new(PointSource::Coordinates {
                coordinates: ["30 mm".into(), "0 mm".into(), "0 mm".into()],
            }));
    }
    let feature = Feature {
        id: cad_core::id(),
        name: format!(
            "{}Pattern{number}",
            if definition.kind == Kind::SketchDriven {
                "SketchDriven"
            } else {
                "TableDriven"
            }
        ),
        operation: Operation::PlacementPattern {
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
    if let Some(label) = default_target {
        w.status = format!(
            "Draft target: {label}. Review or replace it, add desired placements, then Apply"
        );
    }
}
