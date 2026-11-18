//! In-model explanation of a spatial handle's validated, unrealized movement.
use super::*;

#[derive(Clone)]
pub(super) struct Limit {
    point: Id,
    goal: [f64; 3],
    blockers: Vec<Id>,
    editing: Option<Id>,
}

impl Limit {
    pub(super) fn from_goal(
        source: &Definition,
        goal: Option<(Id, [f64; 3])>,
        mut blockers: Vec<Id>,
    ) -> Option<Self> {
        let (point, goal) = goal?;
        let actual = source.points.get(&point)?;
        let delta = DVec3::from_array(goal) - DVec3::from_array(*actual);
        if delta.length() <= 1e-4 {
            return None;
        }
        if blockers.is_empty() {
            for constraint in source.constraints.iter().filter(|c| !c.driven) {
                if match &constraint.relation {
                    Relation::Fixed { point: p, .. } => p == &point,
                    Relation::Coordinate { point: p, axis, .. } => {
                        p == &point && delta[axis.index()].abs() > delta.length() * 1e-4
                    }
                    _ => false,
                } {
                    blockers.push(constraint.id.clone());
                }
            }
        }
        Some(Self {
            point,
            goal,
            blockers,
            editing: None,
        })
    }

    pub(super) fn contains(&self, id: &str) -> bool {
        self.blockers.iter().any(|key| key == id)
    }
    pub(super) fn editing_field(&self, key: &str) -> bool {
        key.split('/')
            .nth(1)
            .is_some_and(|id| Some(id) == self.editing.as_deref())
            && key.starts_with("constraint/")
    }
    pub(super) fn highlights(&self, source: &Definition) -> std::collections::BTreeSet<Id> {
        source
            .constraints
            .iter()
            .filter(|c| {
                self.editing
                    .as_ref()
                    .map_or_else(|| self.contains(&c.id), |id| id == &c.id)
            })
            .flat_map(|c| relation_entities(&c.relation))
            .cloned()
            .collect()
    }
}

pub(super) fn elements(w: &Workspace, source: &Definition) -> Vec<SemanticElement> {
    let Some(state) = &w.sketch3d else {
        return vec![];
    };
    let Some(limit) = &state.drag_limit else {
        return vec![];
    };
    let mut panel = element(
        w,
        "drag-limit",
        "sketch-drag-limit",
        "Movement limited",
        None,
    );
    panel.state = "blocked".into();
    panel.data["point"] = json!(limit.point);
    panel.data["position_mm"] = json!(source.points.get(&limit.point));
    panel.data["constraints"] = json!(limit.blockers);
    panel.value = Some("No driving relation could be isolated at this position.".into());
    let mut out = vec![panel];
    let overrides = configured_fields(w);
    for id in &limit.blockers {
        let Some(c) = source.constraints.iter().find(|c| c.id == *id && !c.driven) else {
            continue;
        };
        let name = relation_label(source, &c.relation);
        let label = constraint_value(&c.relation)
            .map(|(value, _)| {
                let pointer = source_pointer(source, &format!("constraint/{id}/value"));
                let value = pointer.and_then(|p| overrides.get(&p)).unwrap_or(value);
                format!("{name} · {}", value.0)
            })
            .unwrap_or_else(|| name.to_owned());
        let mut driver = action_element(w, &format!("drag-driver/{id}"), &label, !captured(w));
        driver.role = "sketch-drag-driver".into();
        driver.relationships.push(id.clone());
        driver.data["constraint"] = json!(id);
        out.push(driver);
    }
    out
}

pub(super) fn activate(w: &mut Workspace, id: &str) -> Result<()> {
    let source = source(w).ok_or_else(|| invalid("The sketch has closed"))?;
    let state = w
        .sketch3d
        .as_mut()
        .ok_or_else(|| invalid("The sketch has closed"))?;
    let limit = state
        .drag_limit
        .as_mut()
        .filter(|limit| limit.contains(id))
        .ok_or_else(|| invalid("Select a current drag constraint"))?;
    let c = source
        .constraints
        .iter()
        .find(|c| c.id == id && !c.driven)
        .ok_or_else(|| invalid("This constraint is no longer driving"))?;
    limit.editing = Some(id.into());
    state.selection = relation_entities(&c.relation)
        .into_iter()
        .filter_map(|key| {
            if source.points.contains_key(key) {
                Some(Entity::Point { point: key.clone() })
            } else if source.members.contains_key(key) {
                Some(Entity::Member {
                    member: key.clone(),
                })
            } else {
                None
            }
        })
        .collect();
    Ok(())
}

pub(super) fn overlay(w: &mut Workspace, ui: &mut egui::Ui, rect: egui::Rect) -> bool {
    let Some(state) = &w.sketch3d else {
        return false;
    };
    let Some(limit) = &state.drag_limit else {
        return false;
    };
    let Some(source) = source_ref(w) else {
        return false;
    };
    let Some(actual) = source.points.get(&limit.point).copied() else {
        return false;
    };
    let Some(matrix) = part_matrix(w) else {
        return false;
    };
    let project = |p| viewport::project(w, rect, transform(&matrix, p));
    let (Some(anchor), Some(goal)) = (project(actual), project(limit.goal)) else {
        return false;
    };
    if !rect.contains(anchor) || (limit.editing.is_none() && anchor.distance(goal) <= 3.) {
        return false;
    }
    let elements = elements(w, source);
    let Some((panel, drivers)) = elements.split_first() else {
        return false;
    };
    let fields = if let Some(id) = &limit.editing {
        let prefix = format!("constraint/{id}/");
        // Use the same fields, including configuration ownership and invalid-input
        // recovery, as the docked inspector.
        semantics(w)
            .into_iter()
            .filter(|e| {
                e.data["field"]
                    .as_str()
                    .and_then(|key| key.strip_prefix(&prefix))
                    .is_some_and(|key| {
                        key == "value" || key.starts_with("fixed/") || key.starts_with("tangent/")
                    })
            })
            .collect::<Vec<_>>()
    } else {
        vec![]
    };
    let Some((_, chosen, hovered)) = crate::sketch_handles::limit_panel(
        ui,
        rect,
        anchor,
        panel,
        drivers,
        34. * fields.len() as f32,
        |ui| {
            for field in &fields {
                render_control(ui, w, field);
            }
        },
    ) else {
        return false;
    };
    if let Some(index) = chosen {
        action(w, ACTION, Some(&drivers[index].data));
    }
    hovered || chosen.is_some()
}
