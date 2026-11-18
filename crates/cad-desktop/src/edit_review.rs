//! Downstream review for ordinary modeling edits, computed once per accepted
//! worker reply against the exact checkpoint the editor will restore on Cancel.
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
}
impl Review {
    fn add(
        &mut self,
        key: String,
        name: String,
        effect: impl Into<String>,
        repair: bool,
        source: &str,
    ) {
        self.rows.push(Row {
            key,
            name,
            effect: effect.into(),
            repair,
            source: source.into(),
        });
    }
    fn summary(&self) -> String {
        let mut counts = Vec::new();
        for (count, label) in [
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
                let failure = issues.contains_key(m.id.as_str());
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
                let effect = if failure {
                    "Needs a new reference"
                } else if changed_frame {
                    "Reference moved · solved"
                } else if frames.is_some() {
                    "References resolved"
                } else {
                    "Waiting for a valid assembly"
                };
                review.add(
                    format!("mate/{key}/{}", m.id),
                    label,
                    effect,
                    failure,
                    &m.id,
                );
                review.mates += 1;
            }
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
        review.add(
            format!("sheet/{}", sheet.id),
            sheet.name.clone(),
            effect,
            repair,
            &sheet.id,
        );
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
                review.add(
                    format!("sheet/{}/{key}", sheet.id),
                    item.label.clone(),
                    effect,
                    false,
                    &sheet.id,
                );
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
                | "reference-repair"
        )
    ) || (doc.drawings.is_empty()
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
        data: json!({"contract":"tcad.presentation.edit-review","version":1,"components":review.components,"mates":review.mates,"sheets":review.sheets}),
    }];
    items.extend(review.rows.iter().map(|row| SemanticElement {
        id: format!("edit-review/{}", row.key),
        role: "edit-impact-item".into(),
        label: row.name.clone(),
        value: Some(row.effect.clone()),
        state: if row.repair { "error" } else { "evaluated" }.into(),
        commands: vec![],
        relationships: vec![row.source.clone(), "edit-review".into()],
        data: json!({"contract":"tcad.presentation.edit-review","version":1}),
    }));
    items
}
pub(crate) fn show(ui: &mut egui::Ui, w: &Workspace) {
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
            collapse: Some(false),
            open: None,
        },
        |ui| rows(ui, &review),
    );
}
fn rows(ui: &mut egui::Ui, review: &Review) {
    egui::ScrollArea::vertical()
        .max_height(240.)
        .id_salt("edit-review-items")
        .show(ui, |ui| {
            for r in &review.rows {
                controls::label(
                    ui,
                    &format!("edit-review/{}", r.key),
                    "edit-impact-item",
                    &[],
                    &format!("{} · {}", r.name, r.effect),
                    if r.repair { "error" } else { "evaluated" },
                );
            }
        });
}
pub(crate) fn status_menu(ui: &mut egui::Ui, w: &Workspace) {
    if let Some(review) = report(w).filter(|r| !r.rows.is_empty()) {
        controls::menu_button(ui, "edit-review:menu", "Affected items", |ui| {
            rows(ui, &review)
        });
    }
}
