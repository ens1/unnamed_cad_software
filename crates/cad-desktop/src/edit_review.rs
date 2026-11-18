//! Downstream review for ordinary modeling edits, computed once per accepted
//! worker reply against the exact checkpoint the editor will restore on Cancel.
pub(crate) mod mate;
use crate::{Workspace, presentation_controls as controls};
use bevy_egui::egui;
use cad_core::{
    Document, Id, Session,
    assembly_geometry::Frame,
    presentation::SemanticElement,
    protocol::{Evaluation, RequestIdentity},
};
use serde_json::json;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

#[derive(Default)]
pub struct Cache {
    current: Option<(Id, RequestIdentity, Arc<Review>)>,
}
#[derive(Default)]
struct Review {
    rows: Vec<Row>,
    features: usize,
    components: usize,
    mates: usize,
    sheets: usize,
}
struct Row {
    key: String,
    name: String,
    effect: String,
    repair: bool,
    source: Id,
    action: Option<RowAction>,
}
enum RowAction {
    Mate(
        Box<cad_core::assembly_geometry::FailedReference>,
        BTreeSet<Vec<Id>>,
    ),
    Feature(cad_core::NodeRef),
    Sheet {
        id: Id,
        page: u32,
        item: Option<cad_core::drawing_interaction::Target>,
    },
}
impl RowAction {
    fn label(&self) -> &'static str {
        match self {
            Self::Mate(..) => "Replace reference",
            Self::Feature(_) => "Repair inputs",
            Self::Sheet { item: Some(_), .. } => "Show on sheet",
            Self::Sheet { .. } => "Open sheet",
        }
    }
}
impl Review {
    fn add(
        &mut self,
        key: String,
        name: String,
        effect: impl Into<String>,
        repair: bool,
        source: &str,
    ) -> &mut Row {
        self.rows.push(Row {
            key,
            name,
            effect: effect.into(),
            repair,
            source: source.into(),
            action: None,
        });
        self.rows.last_mut().unwrap()
    }
    fn summary(&self) -> String {
        let mut counts = Vec::new();
        for (count, label) in [
            (self.features, "feature"),
            (self.components, "component"),
            (self.mates, "mate"),
            (self.sheets, "sheet"),
        ] {
            if count > 0 {
                counts.push(format!(
                    "{count} {label}{}",
                    if count == 1 { "" } else { "s" }
                ));
            }
        }
        counts.join(" · ")
    }
}
fn evaluated(session: &Session) -> Option<&Evaluation> {
    match &session.draft {
        Some(d) => d.evaluation.as_ref(),
        None => session.evaluation.as_ref(),
    }
}
fn frame_changed(a: &Frame, b: &Frame, linear: f64) -> bool {
    a.kind != b.kind
        || a.origin
            .iter()
            .zip(b.origin)
            .any(|(x, y)| (x - y).abs() > linear)
        || a.axis
            .iter()
            .zip(b.axis)
            .chain(a.secondary.iter().zip(b.secondary))
            .any(|(x, y)| (x - y).abs() > 1e-8)
        || a.curve != b.curve
}
fn related(doc: &Document, before: &Document) -> BTreeSet<Id> {
    let mut changed = doc
        .parts
        .values()
        .filter(|p| before.parts.get(&p.id) != Some(*p))
        .map(|p| p.id.clone())
        .collect::<BTreeSet<_>>();
    changed.extend(
        doc.assemblies
            .values()
            .filter(|a| {
                before
                    .assemblies
                    .get(&a.id)
                    .is_none_or(|old| old.parameters != a.parameters)
            })
            .map(|a| a.id.clone()),
    );
    loop {
        let count = changed.len();
        for part in doc.parts.values() {
            if !changed.contains(&part.id)
                && part
                    .features
                    .iter()
                    .filter(|f| !f.suppressed)
                    .any(|feature| {
                        part.feature_dependencies(feature)
                            .iter()
                            .any(|r| changed.contains(&r.definition))
                    })
            {
                changed.insert(part.id.clone());
            }
        }
        for assembly in doc.assemblies.values() {
            if assembly
                .instances
                .values()
                .any(|i| changed.contains(&i.definition))
            {
                changed.insert(assembly.id.clone());
            }
        }
        if count == changed.len() {
            return changed;
        }
    }
}
fn compare(doc: &Document, baseline: &Document, old: &Evaluation, new: &Evaluation) -> Review {
    let mut review = Review::default();
    let changed = related(doc, baseline);
    if changed.is_empty() {
        return review;
    }
    let issues = new
        .diagnostics
        .iter()
        .filter(|d| d.severity == "error")
        .filter_map(|d| Some((d.target.as_deref()?, d.message.as_str())))
        .collect::<BTreeMap<_, _>>();
    if !issues.is_empty() {
        for failure in cad_core::reference_repair::failures(doc, Some(new)) {
            if !changed.contains(&failure.owner.definition) {
                continue;
            }
            let row = review.add(
                format!(
                    "feature/{}/{}",
                    failure.owner.definition, failure.owner.node
                ),
                failure.label,
                failure.messages.join(" · "),
                true,
                &failure.owner.node,
            );
            if !failure.inputs.is_empty() {
                row.action = Some(RowAction::Feature(failure.owner));
            }
            review.features += 1;
        }
    }
    let linear = new.accuracy.modeling.linear_mm.max(1e-7);
    let assembly_model = doc.assemblies.values().any(|a| {
        a.instances.len() > 1
            || !a.mates.is_empty()
            || a.instances
                .values()
                .any(|i| doc.assemblies.contains_key(&i.definition))
    });
    if assembly_model {
        let previous = old
            .instances
            .iter()
            .map(|i| (&i.path, i))
            .collect::<BTreeMap<_, _>>();
        for i in &new.instances {
            let moved = new.geometry_valid
                && previous.get(&i.path).is_some_and(|p| {
                    (0..3).any(|r| {
                        (0..4).any(|c| {
                            (p.matrix[r][c] - i.matrix[r][c]).abs()
                                > if c == 3 { linear } else { 1e-8 }
                        })
                    })
                });
            if !moved && !changed.contains(&i.definition) {
                continue;
            }
            let name =
                cad_core::occurrence_names::path(doc, &i.path).unwrap_or_else(|| i.name.clone());
            review.add(
                format!("occurrence/{}", i.path.join("/")),
                name,
                if moved { "Moved" } else { "Uses edited part" },
                false,
                &i.definition,
            );
            review.components += 1;
        }
    }
    for a in doc.assemblies.values().filter(|a| changed.contains(&a.id)) {
        let mut configs = BTreeSet::from([cad_core::configuration::key(
            &a.id,
            doc.active_configurations.get(&a.id).map(String::as_str),
        )]);
        configs.extend(
            a.configurations
                .values()
                .map(|c| cad_core::configuration::key(&a.id, Some(&c.id)))
                .filter(|k| new.assemblies.contains_key(k)),
        );
        let failed_reference = new
            .failed_mate_reference
            .as_deref()
            .filter(|f| f.assembly == a.id);
        if let Some(f) = failed_reference {
            configs.insert(cad_core::configuration::key(
                &a.id,
                f.configuration.as_deref(),
            ));
        }
        for key in configs {
            let solution = new.assemblies.get(&key);
            let previous = old.assemblies.get(&key);
            let config_name = a
                .configurations
                .values()
                .find(|c| key == cad_core::configuration::key(&a.id, Some(&c.id)))
                .map(|c| format!(" · {}", c.name))
                .unwrap_or_default();
            for m in &a.mates {
                let broken = failed_reference.filter(|f| {
                    f.mate == m.id
                        && cad_core::configuration::key(&a.id, f.configuration.as_deref()) == key
                });
                let failure = broken.is_some()
                    || (new.failed_mate_reference.is_none() && issues.contains_key(m.id.as_str()));
                let frames = solution.and_then(|s| s.frames.get(&m.id));
                let old_frames = previous.and_then(|s| s.frames.get(&m.id));
                let changed_frame = frames.zip(old_frames).is_some_and(|(frames, old)| {
                    frames
                        .iter()
                        .zip(old)
                        .any(|(x, y)| frame_changed(x, y, linear))
                });
                let owner_changed = baseline
                    .assemblies
                    .get(&a.id)
                    .is_some_and(|base| base.parameters != a.parameters);
                let involved = owner_changed
                    || [&m.first, &m.second].iter().any(|id| {
                        a.instances
                            .get(*id)
                            .is_some_and(|i| changed.contains(&i.definition))
                    });
                if !failure && !changed_frame && !involved {
                    continue;
                }
                if !failure && frames.is_none() && new.geometry_valid {
                    continue;
                } // suppressed in this configuration
                let names = [&m.first, &m.second].map(|id| {
                    a.instances
                        .get(id)
                        .map(|i| cad_core::occurrence_names::instance(doc, &a.id, i))
                        .unwrap_or_else(|| "Component".into())
                });
                let label = format!("{} mate · {} / {}{config_name}", m.kind, names[0], names[1]);
                let effect = if let Some(broken) = broken {
                    let participant = a
                        .instances
                        .get(&broken.participant)
                        .map(|i| cad_core::occurrence_names::instance(doc, &a.id, i))
                        .unwrap_or_default();
                    format!("Missing reference on {participant}")
                } else if failure {
                    issues[m.id.as_str()].to_owned()
                } else if changed_frame {
                    "Reference moved · solved".into()
                } else if frames.is_some() {
                    "References resolved".into()
                } else {
                    "Waiting for a valid assembly".into()
                };
                let row = review.add(
                    format!("mate/{key}/{}", m.id),
                    label,
                    effect,
                    failure,
                    &m.id,
                );
                if let Some(f) = broken {
                    let paths = mate::paths(doc, new, f);
                    if paths.is_empty() {
                        row.effect
                            .push_str(" · replacement unavailable in this model view");
                    } else {
                        row.action = Some(RowAction::Mate(Box::new(f.clone()), paths));
                    }
                }
                review.mates += 1;
            }
        }
        if let Some(problem) = failed_reference.filter(|p| !a.mates.iter().any(|m| m.id == p.mate))
        {
            review.add(
                format!("generated-mate/{}/{}", a.id, problem.mate),
                format!("{} · generated mate", a.name),
                issues
                    .get(problem.mate.as_str())
                    .copied()
                    .unwrap_or("Review the component pattern that created this mate"),
                true,
                &problem.mate,
            );
            review.mates += 1;
        }
        if issues.contains_key(a.id.as_str()) {
            review.add(
                format!("assembly/{}", a.id),
                a.name.clone(),
                "Assembly solve needs attention",
                true,
                &a.id,
            );
        }
    }
    for sheet in doc
        .drawings
        .iter()
        .filter(|s| changed.contains(&s.definition))
    {
        review.sheets += 1;
        let old_items = old.drawing_review.get(&sheet.id);
        let new_items = new.drawing_review.get(&sheet.id);
        let failing = sheet
            .dimensions
            .iter()
            .map(|d| (d.id.as_str(), "Dimension"))
            .chain(
                sheet
                    .entities
                    .iter()
                    .map(|a| (a.id.as_str(), a.kind.label())),
            )
            .find(|(id, _)| issues.contains_key(id));
        let repair = failing.is_some() || issues.contains_key(sheet.id.as_str());
        let effect = if let Some((_, label)) = failing {
            format!("{label} needs repair")
        } else if repair {
            "Needs repair".into()
        } else if let Some(current) = new_items {
            if old_items.is_some_and(|old| {
                current.len() != old.len()
                    || current
                        .iter()
                        .any(|(k, n)| old.get(k).is_none_or(|o| n.digest != o.digest))
            }) {
                "Drawing updated".into()
            } else {
                "Drawing resolved".into()
            }
        } else {
            "Waiting for valid geometry".into()
        };
        let row = review.add(
            format!("sheet/{}", sheet.id),
            sheet.name.clone(),
            effect,
            repair,
            &sheet.id,
        );
        if new.geometry_valid {
            let item = failing.map(|(id, _)| {
                if sheet.dimensions.iter().any(|d| d.id == id) {
                    cad_core::drawing_interaction::Target::Dimension { id: id.into() }
                } else {
                    cad_core::drawing_interaction::Target::Annotation { id: id.into() }
                }
            });
            row.action = Some(RowAction::Sheet {
                id: sheet.id.clone(),
                page: 0,
                item,
            });
        }
        if let Some((old, current)) = old_items.zip(new_items) {
            for (key, item) in current {
                let Some(prior) = old.get(key) else {
                    continue;
                };
                if item.digest == prior.digest {
                    continue;
                }
                let effect = if item.printed_text != prior.printed_text
                    && !prior.printed_text.is_empty()
                    && !item.printed_text.is_empty()
                {
                    format!("{} → {}", prior.printed_text, item.printed_text)
                } else if key.contains("/view:") {
                    "Projection updated".into()
                } else {
                    "Position or geometry updated".into()
                };
                let row = review.add(
                    format!("sheet/{}/{key}", sheet.id),
                    item.label.clone(),
                    effect,
                    false,
                    &sheet.id,
                );
                row.action = Some(RowAction::Sheet {
                    id: sheet.id.clone(),
                    page: item.page,
                    item: item.target.clone(),
                });
            }
        }
    }
    review
}

fn report(w: &Workspace) -> Option<Arc<Review>> {
    let checkpoint = w.tool_checkpoint.as_ref();
    let doc = w.session.visible();
    let new = w.session.draft.as_ref()?.evaluation.as_ref()?;
    if !matches!(
        w.dialog.as_deref(),
        None | Some(
            "feature"
                | "sketch"
                | "parameter"
                | "parameters"
                | "equations"
                | "named-parameters"
                | "feature-order"
                | "model-dimensions"
                | "reference-repair"
        )
    ) || (new.valid
        && doc.drawings.is_empty()
        && doc
            .assemblies
            .values()
            .all(|a| a.instances.len() <= 1 && a.mates.is_empty()))
    {
        return None;
    }
    let identity = crate::source_cache::identity(w);
    if identity != new.identity {
        return None;
    }
    let (base, old) = match checkpoint {
        Some(baseline) => (baseline.visible(), evaluated(baseline)?),
        None => (&w.session.committed, w.session.evaluation.as_ref()?),
    };
    if !old.valid {
        return None;
    }
    let stamp = &old.identity.source;
    if let Ok(cache) = w.edit_review.lock()
        && let Some((key, id, review)) = &cache.current
        && key == stamp
        && id == &identity
    {
        return Some(review.clone());
    }
    let baseline_current = if let Some(baseline) = checkpoint {
        old.identity == baseline.identity()
    } else {
        old.identity.draft.is_none()
            && old.identity.base_revision == base.revision
            && old.identity.source == base.fingerprint()
            && old.identity.generation == w.session.generation
            && old.identity.backend == w.session.backend
    };
    if !baseline_current {
        return None;
    }
    let review = Arc::new(compare(doc, base, old, new));
    if let Ok(mut cache) = w.edit_review.lock() {
        cache.current = Some((stamp.clone(), identity, review.clone()));
    }
    Some(review)
}

// Only leave a parent form when its displayed inputs already match the draft.
fn can_open(w: &Workspace, action: &RowAction) -> bool {
    if w.settings.is_some()
        || crate::drawing_interaction_ui::active(w)
        || crate::git_ui::busy(w)
        || crate::publish_ui::busy(w)
    {
        return false;
    }
    if matches!(action, RowAction::Mate(..)) {
        return crate::reference_repair_ui::current_source_is_valid(w);
    }
    if w.tool_checkpoint.is_none()
        || w.sketch.is_some()
        || crate::named_parameter_ui::active(w)
        || (matches!(action, RowAction::Sheet { .. })
            && !crate::reference_repair_ui::current_source_is_valid(w))
        || !crate::quantity_display::source_fields_valid(w)
        || !crate::operation_editor_ui::current_source_is_valid(w)
        || w.fields.contains_key("feature-source-invalid")
    {
        return false;
    }
    if w.dialog.as_deref() == Some("feature") {
        return serde_json::from_str::<cad_core::Feature>(&w.json).is_ok_and(|f| {
            w.fields.get("feature-editor-id") == Some(&f.id)
                && w.view
                    .active_part
                    .as_ref()
                    .and_then(|id| w.session.visible().parts.get(id))
                    .is_some_and(|p| p.features.iter().any(|v| v == &f))
        });
    }
    true
}
fn open(w: &mut Workspace, action: &RowAction) {
    if !can_open(w, action) {
        w.status = "Finish the current field or gesture first".into();
        return;
    }
    match action {
        RowAction::Mate(problem, paths) => mate::begin(w, (**problem).clone(), paths.clone()),
        RowAction::Feature(owner) => {
            crate::reference_repair_ui::action(
                w,
                "reference.repair",
                Some(&json!({"owner":owner})),
            );
        }
        RowAction::Sheet { id, page, item } => {
            crate::reference_repair_ui::reset(w);
            w.open_editor("drawings");
            w.command_tab = "Drawing".into();
            crate::drawing_ui::select_sheet(w, id);
            w.fields.insert("drawing-page".into(), page.to_string());
            w.view.drawing_page = *page;
            if let Some(item) = item {
                // The paper may still be rebuilding. Its interaction state restores
                // this selection as soon as the current sheet becomes available.
                w.view.drawing_selection = Some(item.clone());
                crate::drawing_interaction_ui::select_item(w, item.clone());
                if w.view.drawing_selection.is_none() {
                    w.view.drawing_selection = Some(item.clone());
                }
            }
            w.status =
                "Review the sheet. Apply saves the original edit and any repairs together.".into();
        }
    }
}
pub(crate) fn action(w: &mut Workspace, command: &str, data: Option<&serde_json::Value>) -> bool {
    match command {
        "edit-review.stop-picking" => {
            mate::stop(w);
            true
        }
        "edit-review.open" => {
            if data
                .and_then(|d| d.get("request"))
                .and_then(|v| serde_json::from_value::<RequestIdentity>(v.clone()).ok())
                .as_ref()
                != Some(&crate::source_cache::identity(w))
            {
                w.status = "The model changed. Select the item in the current review.".into();
                return true;
            }
            let key = data.and_then(|d| d.get("key")).and_then(|k| k.as_str());
            if let Some(review) = report(w)
                && let Some(action) = review
                    .rows
                    .iter()
                    .find(|r| Some(r.key.as_str()) == key)
                    .and_then(|r| r.action.as_ref())
            {
                open(w, action);
            } else {
                w.status = "This item has changed. Select it in the current review.".into();
            }
            true
        }
        _ => false,
    }
}
pub(crate) fn semantics(w: &Workspace) -> Vec<SemanticElement> {
    let Some(review) = report(w).filter(|r| !r.rows.is_empty()) else {
        return vec![];
    };
    let mut items = vec![SemanticElement {
        id: "edit-review".into(),
        role: "edit-impact-summary".into(),
        label: "Affected by this edit".into(),
        value: Some(review.summary()),
        state: "evaluated".into(),
        commands: vec![],
        relationships: vec![],
        data: json!({"contract":"tcad.presentation.edit-review","version":2,"features":review.features,"components":review.components,"mates":review.mates,"sheets":review.sheets}),
    }];
    if mate::evaluation(w).is_some() {
        items.push(SemanticElement {
            id: "edit-review/pick".into(),
            role: "mate-reference-picker".into(),
            label: mate::prompt(w),
            value: None,
            state: "picking".into(),
            commands: vec!["edit-review.stop-picking".into()],
            relationships: vec!["edit-review".into()],
            data: json!({"contract":"tcad.presentation.edit-review","version":2}),
        });
    }
    let request = crate::source_cache::identity(w);
    items.extend(review.rows.iter().map(|row| SemanticElement {
        id: format!("edit-review/{}", row.key),
        role: "edit-impact-item".into(),
        label: row.name.clone(),
        value: Some(row.effect.clone()),
        state: if row.repair { "error" } else { "evaluated" }.into(),
        commands: row.action.as_ref().filter(|a|can_open(w,a)).map_or_else(Vec::new, |_| vec!["edit-review.open".into()]),
        relationships: vec![row.source.clone(), "edit-review".into()],
        data: json!({"contract":"tcad.presentation.edit-review","version":2,"key":row.key,"request":request,"action_label":row.action.as_ref().map(RowAction::label)}),
    }));
    items
}
pub(crate) fn show(ui: &mut egui::Ui, w: &mut Workspace) {
    let Some(review) = report(w).filter(|r| !r.rows.is_empty()) else {
        return;
    };
    let label = format!("Affected by this edit · {}", review.summary());
    let any_error = review.rows.iter().any(|r| r.repair);
    controls::inspector_section(
        ui,
        controls::InspectorSection {
            id: "edit-review",
            role: "edit-impact-summary",
            classes: &["secondary-section"],
            label: &label,
            state: if any_error { "error" } else { "evaluated" },
            collapse: Some(any_error),
            open: None,
        },
        |ui| rows(ui, w, &review),
    );
}
fn rows(ui: &mut egui::Ui, w: &mut Workspace, review: &Review) {
    if mate::evaluation(w).is_some() {
        controls::label(
            ui,
            "edit-review/pick",
            "mate-reference-picker",
            &[],
            &mate::prompt(w),
            "picking",
        );
        if controls::button(
            ui,
            "edit-review/stop-picking",
            "tool-action",
            &[],
            "Back to edit",
            true,
            false,
        )
        .clicked()
        {
            mate::stop(w);
        }
    }
    egui::ScrollArea::vertical()
        .max_height(240.)
        .id_salt("edit-review-items")
        .show(ui, |ui| {
            for r in review
                .rows
                .iter()
                .filter(|r| r.repair)
                .chain(review.rows.iter().filter(|r| !r.repair))
            {
                controls::label(
                    ui,
                    &format!("edit-review/{}", r.key),
                    "edit-impact-item",
                    &[],
                    &format!("{} · {}", r.name, r.effect),
                    if r.repair { "error" } else { "evaluated" },
                );
                if let Some(action) = &r.action {
                    let response = controls::button(
                        ui,
                        &format!("edit-review/{}/open", r.key),
                        "edit-impact-action",
                        &[],
                        action.label(),
                        can_open(w, action),
                        false,
                    );
                    if response
                        .on_disabled_hover_text("Finish the current field or gesture first")
                        .clicked()
                    {
                        open(w, action);
                    }
                }
            }
        });
}
pub(crate) fn status_menu(ui: &mut egui::Ui, w: &mut Workspace) {
    if let Some(review) = report(w).filter(|r| !r.rows.is_empty()) {
        controls::menu_button(ui, "edit-review:menu", "Affected items", |ui| {
            rows(ui, w, &review)
        });
    }
}
