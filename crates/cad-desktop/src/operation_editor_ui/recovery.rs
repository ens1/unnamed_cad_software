//! Explicit edits to a failed edge selection. Diagnostic references always
//! belong to the current worker reply; omitted edges live only in this editor.
use super::*;
use crate::{selection_ui, viewport};
use cad_core::{
    TopologyRef,
    edge_features::{Recovery, RecoveryKind},
};

const SAVED: &str = "edge-recovery-omissions";
#[derive(serde::Deserialize, serde::Serialize)]
struct Omissions {
    editor: String,
    body: NodeRef,
    edges: Vec<(usize, TopologyRef)>,
}
fn omissions(w: &Workspace, body: &NodeRef) -> Option<Omissions> {
    let saved: Omissions = serde_json::from_str(w.fields.get(SAVED)?).ok()?;
    (&saved.body == body && w.fields.get("feature-editor-generation") == Some(&saved.editor))
        .then_some(saved)
}
fn failure<'a>(w: &'a Workspace, feature: &Feature) -> Option<&'a Recovery> {
    let evaluation = w.session.draft.as_ref()?.evaluation.as_ref()?;
    if evaluation.valid || evaluation.identity != crate::source_cache::identity(w) {
        return None;
    }
    evaluation.diagnostics.iter().find_map(|d| {
        (d.target.as_ref() == Some(&feature.id))
            .then_some(d.edge_recovery.as_ref())
            .flatten()
    })
}
fn indices(selection: &[TopologyRef], failure: &Recovery) -> Vec<usize> {
    selection
        .iter()
        .enumerate()
        .filter_map(|(i, edge)| {
            failure
                .edges
                .iter()
                .any(|r| selection_ui::same_edge(r, edge))
                .then_some(i)
        })
        .collect()
}
pub(super) fn fields(fields: &mut Fields<'_>) {
    let Some((body, edges)) = selection_ui::edge_selection(fields.feature) else {
        return;
    };
    let failed = failure(fields.w, fields.feature);
    let saved = omissions(fields.w, body);
    if saved.is_none() && failed.is_none() {
        return;
    }
    fields.group("edge-recovery", "Edge repair");
    if let Some(failure) = failed {
        let picks = indices(edges, failure);
        if picks.is_empty() {
            fields.summary(
                "unmapped",
                "The failed references could not be matched to the current selection.",
                "error",
            );
        }
        let label = if picks.len() == 1 {
            format!("edge {}", picks[0] + 1)
        } else {
            format!("{} edges", picks.len())
        };
        let text = match failure.kind {
            RecoveryKind::UnresolvedReference => {
                format!("Select a replacement for {label} on the input body.")
            }
            RecoveryKind::ValidRemainder => format!("The input body rebuilt with {label} omitted."),
            RecoveryKind::FailedSelection if picks.len() == 1 => {
                format!("Check the size and adjoining faces at {label}.")
            }
            RecoveryKind::FailedSelection => {
                "These edges could not be treated together. Check the size or edit the selection."
                    .into()
            }
        };
        if !picks.is_empty() {
            fields.summary("cause", &text, "error");
            fields.out.last_mut().unwrap().data["edge_repair"] =
                json!({"body":body,"indices":picks,"references":failure.edges,"kind":failure.kind});
        }
        if let [index] = picks.as_slice()
            && failure.kind != RecoveryKind::UnresolvedReference
        {
            fields.action(
                "focus",
                &format!("Show edge {}", index + 1),
                json!({"kind":"focus_failed","index":index}),
                true,
                "",
            );
        }
        if !picks.is_empty() && picks.len() < edges.len() {
            fields.action(
                "omit",
                &format!("Preview without {label}"),
                json!({"kind":"omit_failed","indices":picks}),
                true,
                "",
            );
        } else if picks.len() > 1 && picks.len() == edges.len() {
            fields.folding_group("edge-trials", "Try individual edges", false);
            for index in &picks {
                fields.action(
                    &format!("omit-{index}"),
                    &format!("Preview without edge {}", index + 1),
                    json!({"kind":"omit_failed","indices":[index]}),
                    true,
                    "",
                );
            }
        }
    }
    if saved.is_some() {
        fields.action(
            "restore",
            "Restore omitted edges",
            json!({"kind":"restore_omitted"}),
            true,
            "",
        );
    }
}
pub(super) fn edit(w: &mut Workspace, feature: &Feature, mutation: &Value) -> Result<()> {
    let mut changed = feature.clone();
    let (body, edges) = match &mut changed.operation {
        Operation::Fillet { body, edges, .. } | Operation::Chamfer { body, edges, .. } => {
            (body, edges)
        }
        _ => return Err(invalid("This feature has no edge selection")),
    };
    if mutation["kind"] == "focus_failed" {
        let index = mutation["index"]
            .as_u64()
            .and_then(|i| usize::try_from(i).ok())
            .ok_or_else(|| invalid("The failed edge is unavailable"))?;
        let edge = edges
            .get(index)
            .ok_or_else(|| invalid("The edge selection has changed"))?;
        return selection_ui::focus_edge(w, body, edge);
    }
    let mut saved = omissions(w, body).unwrap_or_else(|| Omissions {
        editor: w
            .fields
            .get("feature-editor-generation")
            .cloned()
            .unwrap_or_default(),
        body: body.clone(),
        edges: vec![],
    });
    let restoring = mutation["kind"] == "restore_omitted";
    if restoring {
        // Reverse the sequence of removals, retaining edits to surviving edges.
        for (index, edge) in saved.edges.iter().rev() {
            if !edges
                .iter()
                .any(|present| selection_ui::same_edge(present, edge))
            {
                edges.insert((*index).min(edges.len()), edge.clone());
            }
        }
    } else {
        let recovery =
            failure(w, feature).ok_or_else(|| invalid("Wait for the current edge diagnosis"))?;
        let failed = indices(edges, recovery);
        let omit: Vec<usize> = serde_json::from_value(mutation["indices"].clone())?;
        if omit.is_empty()
            || omit.len() >= edges.len()
            || !omit.windows(2).all(|pair| pair[0] < pair[1])
            || !omit.iter().all(|i| failed.contains(i))
        {
            return Err(invalid("The edge selection has changed"));
        }
        for index in omit.into_iter().rev() {
            saved.edges.push((index, edges.remove(index)));
        }
    }
    publish(w, changed)?;
    if restoring {
        w.fields.remove(SAVED);
    } else {
        w.fields
            .insert(SAVED.into(), serde_json::to_string(&saved)?);
    }
    w.confirm_requested = false;
    Ok(())
}

pub(crate) fn overlay(w: &Workspace, ui: &egui::Ui, rect: egui::Rect) {
    let Some(feature) = selection_ui::edge_feature(w) else {
        return;
    };
    let Some(failure) = failure(w, &feature) else {
        return;
    };
    let Some((body, authored)) = selection_ui::edge_selection(&feature) else {
        return;
    };
    let Some(path) = selection_ui::known_edge_occurrence(w, &body.definition) else {
        return;
    };
    let Some(e) = viewport::selection_evaluation(w) else {
        return;
    };
    let checkpoint = w.tool_checkpoint.as_ref().unwrap_or(&w.session);
    if e.identity != checkpoint.identity() {
        return;
    }
    let Some(instance) = viewport::display_instances(w, e)
        .into_iter()
        .find(|i| i.path == path && i.definition == body.definition && viewport::visible(w, i))
    else {
        return;
    };
    let shown = viewport::selection_meshes(w, e, instance);
    let painter = ui.painter_at(rect);
    for index in indices(authored, failure) {
        let mut matches = shown.iter().flat_map(|m| &m.edges).filter(|edge| {
            edge.body.as_ref() == Some(body)
                && selection_ui::same_edge(&authored[index], &edge.reference)
        });
        let Some(edge) = matches.next() else {
            continue;
        };
        if matches.next().is_some() {
            continue;
        }
        let props = controls::properties(
            &format!("edge-repair:{}:{index}", feature.id),
            "edge-repair-guide",
            &["viewport-guide"],
            "error",
        );
        if props.get("display").is_some_and(|p| p == "none")
            || props.get("visibility").is_some_and(|p| p == "hidden")
        {
            continue;
        }
        let color = props
            .get("color")
            .and_then(|s| crate::ui::color(s))
            .unwrap_or(ui.visuals().error_fg_color);
        let stroke = egui::Stroke::new(controls::number(&props, "stroke-width", 3.0), color);
        for pair in edge.points.windows(2) {
            let project = |point| viewport::project(w, rect, viewport::placed(point, instance));
            if let (Some(a), Some(b)) = (project(pair[0]), project(pair[1])) {
                painter.line_segment([a, b], stroke);
            }
        }
    }
}
