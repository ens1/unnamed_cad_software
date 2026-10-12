//! Themeable actions for bounded cohorts of drawing annotations.
use crate::Workspace;
use cad_core::{
    CadError, Drawing, Id, Result, drawing_interaction::Target, presentation::SemanticElement,
};
use serde_json::{Value, json};
pub(crate) fn ids(targets: &[Target]) -> Result<Vec<Id>> {
    if targets.is_empty() || targets.len() > cad_core::drawing_layout::MAX_COHORT {
        return Err(CadError::Invalid("Select 1–128 paper annotations".into()));
    }
    targets.iter().map(|target|if let Target::Annotation{id}=target{Ok(id.clone())}else{Err(CadError::Invalid("Paper cleanup applies to annotations; dimensions, views and tables retain their own layout controls".into()))}).collect()
}
pub(crate) fn commands(
    w: &Workspace,
    source: &Drawing,
    targets: &[Target],
    primary: Option<&Target>,
    identity: Value,
    available: bool,
) -> Vec<SemanticElement> {
    let ids = ids(targets);
    if ids.is_err() {
        return vec![];
    }
    let movable = ids.as_ref().is_ok_and(|ids| {
        ids.iter().all(|id| {
            source
                .entities
                .iter()
                .find(|annotation| &annotation.id == id)
                .is_some_and(cad_core::drawing_layout::movable)
        })
    });
    let count = targets.len();
    let mut out = Vec::new();
    for (key, label, minimum, requires_move) in [
        (
            "drawing.interact-cohort-align-x",
            "Align origins in a column",
            2,
            true,
        ),
        (
            "drawing.interact-cohort-align-y",
            "Align origins in a row",
            2,
            true,
        ),
        (
            "drawing.interact-cohort-distribute-x",
            "Even horizontal origins",
            3,
            true,
        ),
        (
            "drawing.interact-cohort-distribute-y",
            "Even vertical origins",
            3,
            true,
        ),
        ("drawing.interact-cohort-stack-x", "Row at spacing", 2, true),
        (
            "drawing.interact-cohort-stack-y",
            "Column at spacing",
            2,
            true,
        ),
        (
            "drawing.interact-cohort-copy",
            "Duplicate annotations",
            1,
            true,
        ),
        (
            "drawing.interact-cohort-delete",
            "Delete annotations",
            1,
            false,
        ),
    ] {
        if count < 2 && minimum > 1 {
            continue;
        }
        let enabled = available
            && w.settings.is_none()
            && ids.is_ok()
            && count >= minimum
            && (!requires_move || movable);
        let reason = if ids.is_err() {
            "Select annotation symbols for paper cleanup; other item types retain their own layout controls"
        } else if requires_move && !movable {
            "Center geometry stays attached to its source; choose movable annotations"
        } else if count < minimum {
            "Select more annotations with Command/Shift-click"
        } else {
            "Finish the current gesture and correct retained field text first"
        };
        let mut data = identity.clone();
        data["cohort"] = json!(targets);
        data["primary"] = json!(primary);
        data["annotations"] = json!(ids.as_ref().ok());
        data["enabled"] = json!(enabled);
        data["disabled_reason"] = json!(reason);
        data["layout_contract"] = json!("tcad.drawing-layout");
        data["layout_version"] = json!(cad_core::drawing_layout::CONTRACT_VERSION);
        data["actions"] = json!([{"command":key,"label":label,"enabled":enabled,"disabled_reason":reason,"arguments":data.clone()}]);
        out.push(SemanticElement {
            id: format!("command:{key}"),
            role: "command".into(),
            label: label.into(),
            value: None,
            state: if enabled { "available" } else { "disabled" }.into(),
            commands: vec![key.into()],
            relationships: vec![source.id.clone()],
            data,
        });
    }
    out
}
