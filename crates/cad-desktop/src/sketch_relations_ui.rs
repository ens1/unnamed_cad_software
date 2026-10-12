//! Selection-aware sketch feedback over the existing solver and shared sketch draft.
use crate::{Workspace, presentation_controls as controls, sketch, sketch_ui};
use bevy_egui::egui;
use cad_core::{
    presentation::SemanticElement,
    sketch_actions::{self as actions, SketchEntity},
    *,
};
use std::collections::{BTreeMap, BTreeSet};

const FILTER: &str = "sketch-relations-filter";
const MOTION: &str = "sketch-motion-display";
const SEARCH: &str = "sketch-relations-search";
const CATEGORY: &str = "sketch-relations-category";
const SORT: &str = "sketch-relations-sort";

#[derive(Clone, Default)]
pub struct Feedback {
    pub report: Option<solver::SolveReport>,
    pub residuals: BTreeMap<Id, Vec<f64>>,
    pub maximum_residual: Option<f64>,
    pub unsatisfied: BTreeMap<Id, f64>,
    pub error: Option<String>,
    pub motion: Option<sketch_diagnostics::Analysis>,
    pub motion_error: Option<String>,
    pub duplicates: BTreeMap<Id, Id>,
    pub analysis_token: Id,
    pub motion_pending: bool,
    source: Option<Sketch>,
    edit: Option<(Id, String, usize)>,
}

/// Residuals describe the current authored geometry. A failed solve does not
/// expose its final internal candidate, so these cannot identify a minimal
/// conflicting set or claim to be the failed solver's final residuals.
pub fn refresh(w: &mut Workspace, report: Option<solver::SolveReport>) {
    let Some(s) = &w.sketch else { return };
    let source = sketch::sketch_definition(s);
    let duplicates = sketch_diagnostics::exact_duplicates(&source);
    let residuals = equation_feedback(w.session.visible(), &s.definition, &source);
    let (residuals, unsatisfied, maximum_residual, error) = match residuals {
        Ok(values) => {
            let maxima = values
                .iter()
                .map(|(key, values)| {
                    (
                        key.clone(),
                        values.iter().map(|value| value.abs()).fold(0_f64, f64::max),
                    )
                })
                .collect::<BTreeMap<_, _>>();
            let maximum = maxima.values().copied().fold(0_f64, f64::max);
            let unsatisfied = maxima
                .into_iter()
                .filter(|(_, value)| *value > 1e-6)
                .collect();
            (values, unsatisfied, Some(maximum), None)
        }
        Err(error) => (
            BTreeMap::new(),
            BTreeMap::new(),
            None,
            Some(error.to_string()),
        ),
    };
    if let Some(s) = &mut w.sketch {
        let edit = s.feedback.edit.take();
        s.feedback = Feedback {
            report,
            residuals,
            maximum_residual,
            unsatisfied,
            error,
            motion: None,
            motion_error: None,
            motion_pending: s.error.is_none(),
            analysis_token: id(),
            duplicates,
            source: Some(source),
            edit,
        };
    }
}

pub fn end_edit(w: &mut Workspace) {
    if let Some(s) = &mut w.sketch {
        s.feedback.edit = None;
    }
}

fn equation_feedback(
    doc: &Document,
    owner: &str,
    source: &Sketch,
) -> Result<BTreeMap<Id, Vec<f64>>> {
    let mut geometry = source.clone();
    actions::apply_curve_dimensions(doc, owner, &mut geometry)?;
    sketch_edit::derive(doc, owner, &mut geometry)?;
    let blocks = actions::residual_blocks(doc, owner, &geometry, |key| {
        actions::entity_point(&geometry, key)
    })?;
    if blocks
        .iter()
        .any(|block| block.values.iter().any(|value| !value.is_finite()))
    {
        return Err(CadError::Invalid(
            "Current equation feedback contains a non-finite residual".into(),
        ));
    }
    Ok(blocks
        .into_iter()
        .map(|block| (block.constraint, block.values))
        .collect())
}

pub fn retain_report(s: &sketch::EditingSketch) -> Option<solver::SolveReport> {
    current_source(s)
        .then(|| s.feedback.report.clone())
        .flatten()
}

fn current_source(s: &sketch::EditingSketch) -> bool {
    s.feedback.source.as_ref().is_some_and(|source| {
        source.id == s.sketch_id
            && source.name == s.name
            && source.points.len() == s.keys.len()
            && s.keys.len() == s.points.len()
            && s.keys
                .iter()
                .zip(&s.points)
                .all(|(key, point)| source.points.get(key) == Some(point))
            && source.constraints == s.constraints
            && source.members == s.members
            && source.conversions == s.conversions
            && source.offsets == s.offsets
            && source.fillets == s.fillets
            && source.chamfers == s.chamfers
            && source.transforms == s.transforms
            && source.splines == s.splines
            && source.conics == s.conics
            && source.texts == s.texts
            && source.equations == s.equations
            && source.blocks == s.blocks
            && source.block_instances == s.block_instances
            && source.dimension_groups == s.dimension_groups
    })
}

pub fn motion(s: &sketch::EditingSketch) -> Option<&sketch_diagnostics::Analysis> {
    (s.error.is_none() && current_source(s))
        .then_some(s.feedback.motion.as_ref())
        .flatten()
}

pub(crate) fn analysis_source(s: &sketch::EditingSketch) -> Option<&Sketch> {
    (!s.choosing_plane && s.error.is_none() && current_source(s))
        .then_some(s.feedback.source.as_ref())
        .flatten()
}

/// View preference only; dimensions and source definitions are untouched.
pub fn motion_display(w: &Workspace) -> bool {
    w.fields.get(MOTION).is_some_and(|value| value == "true")
}

fn filter(w: &Workspace) -> &str {
    w.fields
        .get(FILTER)
        .map(String::as_str)
        .unwrap_or("selected")
}

fn geometry_keys(relation: &SketchConstraint) -> Vec<&str> {
    let implicit: &[&str] = match relation.kind.as_str() {
        "fix_x" => &[actions::ORIGIN, actions::X_AXIS],
        "fix_y" => &[actions::ORIGIN, actions::Y_AXIS],
        "horizontal" | "distance_x" => &[actions::X_AXIS],
        "vertical" | "distance_y" => &[actions::Y_AXIS],
        "symmetric_origin" => &[actions::ORIGIN],
        _ => &[],
    };
    relation
        .entities
        .iter()
        .map(String::as_str)
        .chain(implicit.iter().copied())
        .collect()
}

/// Relate one level of member/point ownership to the selected geometry without
/// expanding through neighboring members into the entire sketch.
pub(crate) fn selection_keys(source: &Sketch, selection: &[SketchEntity]) -> BTreeSet<Id> {
    let mut keys = BTreeSet::new();
    for entity in selection {
        match entity {
            SketchEntity::Constraint { constraint } => {
                if let Some(relation) = source.constraints.iter().find(|c| &c.id == constraint) {
                    keys.insert(constraint.clone());
                    keys.extend(geometry_keys(relation).into_iter().map(str::to_string));
                }
            }
            SketchEntity::Point { point } => {
                keys.insert(point.clone());
                keys.extend(
                    source
                        .members
                        .iter()
                        .filter(|(_, member)| member.curve.points().contains(&point.as_str()))
                        .map(|(key, _)| key.clone()),
                );
            }
            SketchEntity::Member { member } => {
                keys.insert(member.clone());
                if let Some(member) = source.members.get(member) {
                    keys.extend(member.curve.points().into_iter().map(str::to_string));
                }
            }
            _ => {
                keys.insert(entity.key().into());
            }
        }
    }
    keys
}

pub fn related_entities(s: &sketch::EditingSketch) -> BTreeSet<Id> {
    let mut related = BTreeSet::new();
    for key in s.selection.iter().filter_map(|entity| match entity {
        SketchEntity::Constraint { constraint } => Some(constraint),
        _ => None,
    }) {
        if let Some(relation) = s.constraints.iter().find(|c| &c.id == key) {
            related.extend(geometry_keys(relation).into_iter().map(str::to_string));
            for member in relation
                .entities
                .iter()
                .filter_map(|key| s.members.get(key))
            {
                related.extend(member.curve.points().into_iter().map(str::to_string));
            }
        }
    }
    related
}

pub fn problem_entities(w: &Workspace) -> BTreeSet<Id> {
    let mut related = BTreeSet::new();
    if filter(w) != "unsatisfied" {
        return related;
    }
    let Some(s) = &w.sketch else { return related };
    for relation in s
        .constraints
        .iter()
        .filter(|relation| s.feedback.unsatisfied.contains_key(&relation.id))
    {
        related.extend(geometry_keys(relation).into_iter().map(str::to_string));
        for member in relation
            .entities
            .iter()
            .filter_map(|key| s.members.get(key))
        {
            related.extend(member.curve.points().into_iter().map(str::to_string));
        }
    }
    related
}

pub fn constraint_emphasized(s: &sketch::EditingSketch, relation: &SketchConstraint) -> bool {
    s.selection.iter().any(|entity| match entity {
        SketchEntity::Constraint { constraint } => constraint == &relation.id,
        SketchEntity::Point { point } => relation.entities.iter().any(|key| {
            key == point
                || s.members
                    .get(key)
                    .is_some_and(|member| member.curve.points().contains(&point.as_str()))
        }),
        SketchEntity::Member { member } => relation.entities.iter().any(|key| {
            key == member
                || s.members
                    .get(member)
                    .is_some_and(|member| member.curve.points().contains(&key.as_str()))
        }),
        _ => geometry_keys(relation).contains(&entity.key()),
    })
}

fn scope_visible(w: &Workspace, keys: &BTreeSet<Id>, relation: &SketchConstraint) -> bool {
    let Some(s) = &w.sketch else { return false };
    match filter(w) {
        "unsatisfied" => s.feedback.unsatisfied.contains_key(&relation.id),
        "all" => true,
        "dependent" => {
            s.feedback.duplicates.contains_key(&relation.id)
                || motion(s)
                    .and_then(|analysis| analysis.constraints.get(&relation.id))
                    .is_some_and(|analysis| analysis.dependent_equations > 0)
        }
        _ if s.selection.is_empty() => true,
        _ => {
            keys.contains(&relation.id)
                || geometry_keys(relation)
                    .into_iter()
                    .any(|key| keys.contains(key))
        }
    }
}

fn visible(
    w: &Workspace,
    source: &Sketch,
    keys: &BTreeSet<Id>,
    relation: &SketchConstraint,
) -> bool {
    if !scope_visible(w, keys, relation) {
        return false;
    }
    let category = w.fields.get(CATEGORY).map(String::as_str).unwrap_or("all");
    let dimension = actions::is_dimension(&relation.kind);
    if category == "dimensions" && !dimension || category == "relations" && dimension {
        return false;
    }
    let search = w.fields.get(SEARCH).map(String::as_str).unwrap_or_default();
    if search.trim().is_empty() {
        return true;
    }
    let searchable = format!(
        "{} {} {} {} {}",
        actions::label(&relation.kind),
        relation.kind,
        relation.value.0,
        geometry_keys(relation)
            .iter()
            .map(|key| entity_name(source, key))
            .collect::<Vec<_>>()
            .join(" "),
        if relation.driven {
            "reference driven"
        } else if dimension {
            "driving dimension"
        } else {
            "relation"
        }
    )
    .to_lowercase();
    w.fields
        .get(SEARCH)
        .map(String::as_str)
        .unwrap_or_default()
        .split_whitespace()
        .all(|term| searchable.contains(&term.to_lowercase()))
}
fn visible_rows(w: &Workspace, source: &Sketch, keys: &BTreeSet<Id>) -> Vec<SketchConstraint> {
    let mut rows = source
        .constraints
        .iter()
        .filter(|relation| visible(w, source, keys, relation))
        .cloned()
        .collect::<Vec<_>>();
    match w.fields.get(SORT).map(String::as_str).unwrap_or("authored") {
        "type" => rows.sort_by(|a, b| {
            actions::label(&a.kind)
                .cmp(actions::label(&b.kind))
                .then_with(|| a.id.cmp(&b.id))
        }),
        "status" => {
            if let Some(sketch) = w.sketch.as_ref() {
                rows.sort_by(|a, b| {
                    let rank = |relation: &SketchConstraint| match relation_state(sketch, relation)
                    {
                        "selected" => 0,
                        "unsatisfied" => 1,
                        "dependent" => 2,
                        "reference" => 3,
                        _ => 4,
                    };
                    rank(a)
                        .cmp(&rank(b))
                        .then_with(|| {
                            sketch
                                .feedback
                                .unsatisfied
                                .get(&b.id)
                                .copied()
                                .unwrap_or_default()
                                .total_cmp(
                                    &sketch
                                        .feedback
                                        .unsatisfied
                                        .get(&a.id)
                                        .copied()
                                        .unwrap_or_default(),
                                )
                        })
                        .then_with(|| a.id.cmp(&b.id))
                });
            }
        }
        _ => {}
    }
    rows
}
fn browser_fields(w: &Workspace) -> Vec<SemanticElement> {
    let Some(sketch) = w.sketch.as_ref() else {
        return vec![];
    };
    [("search","Search relations",w.fields.get(SEARCH).cloned().unwrap_or_default(),serde_json::json!([])),
        ("category","Type",w.fields.get(CATEGORY).cloned().unwrap_or_else(||"all".into()),serde_json::json!([{"value":"all","label":"Dimensions and relations"},{"value":"dimensions","label":"Dimensions only"},{"value":"relations","label":"Relations only"}])),
        ("sort","Order",w.fields.get(SORT).cloned().unwrap_or_else(||"authored".into()),serde_json::json!([{"value":"authored","label":"Created order"},{"value":"type","label":"Relation type"},{"value":"status","label":"Selection / problems first"}]))].into_iter().map(|(key,label,value,choices)| {
            let mut element=field(sketch,format!("{}::feedback.{key}",sketch.sketch_id),label,key,serde_json::json!(value),None,true);
            element.role="sketch-relation-browser-field".into();element.data["choices"]=choices;
            element.data["view_only"]=true.into();element.data["classes"]=serde_json::json!(["modeling-field"]);element
        }).collect()
}
fn entity_name(source: &Sketch, key: &str) -> String {
    match key {
        actions::ORIGIN => "Origin".into(),
        actions::X_AXIS => "X Axis".into(),
        actions::Y_AXIS => "Y Axis".into(),
        _ => source
            .members
            .get(key)
            .map(|member| {
                let kind = match member.curve {
                    SketchCurve::Line { .. } => "Line",
                    SketchCurve::Arc { .. } => "Arc",
                    SketchCurve::Bezier { .. } => "Spline",
                    SketchCurve::Nurbs { .. } => "NURBS",
                    SketchCurve::Ellipse { .. } => "Ellipse",
                    SketchCurve::Circle { .. } => "Circle",
                };
                let index = source
                    .members
                    .keys()
                    .position(|member| member == key)
                    .unwrap_or_default()
                    + 1;
                format!(
                    "{kind} {index}{}",
                    if member.construction {
                        " · Construction"
                    } else {
                        ""
                    }
                )
            })
            .or_else(|| {
                source
                    .points
                    .keys()
                    .position(|point| point == key)
                    .map(|index| format!("Point {}", index + 1))
            })
            .unwrap_or_else(|| "Missing geometry".into()),
    }
}

fn relation_state(s: &sketch::EditingSketch, relation: &SketchConstraint) -> &'static str {
    if s.selection.iter().any(|entity| matches!(entity, SketchEntity::Constraint { constraint } if constraint == &relation.id)) {
        "selected"
    } else if s.feedback.unsatisfied.contains_key(&relation.id) {
        "unsatisfied"
    } else if relation.driven {
        "reference"
    } else if s.feedback.duplicates.contains_key(&relation.id)
        || motion(s).and_then(|analysis| analysis.constraints.get(&relation.id))
            .is_some_and(|analysis| analysis.dependent_equations > 0)
    {
        "dependent"
    } else {
        "ready"
    }
}

fn text(ui: &mut egui::Ui, id: &str, role: &str, value: &str, state: &str) {
    controls::label(ui, id, role, &[], value, state);
}

fn summary(s: &sketch::EditingSketch) -> String {
    if let Some(report) = retain_report(s) {
        let dependencies = motion(s).map(|analysis| {
            analysis
                .constraints
                .values()
                .map(|constraint| constraint.dependent_equations)
                .sum::<usize>()
        });
        if let Some(dependencies) = dependencies {
            format!(
                "{} local DOF · {dependencies} positional dependencies · residual {:.3e}",
                report.degrees_of_freedom, report.residual
            )
        } else {
            format!(
                "{} local DOF · residual {:.3e}",
                report.degrees_of_freedom, report.residual
            )
        }
    } else {
        s.feedback
            .maximum_residual
            .map(|value| format!("DOF unavailable · current equation residual {value:.3e}"))
            .unwrap_or_else(|| "Solve report unavailable for this geometry".into())
    }
}

pub fn panel(ui: &mut egui::Ui, w: &mut Workspace) {
    if ui.input(|input| input.pointer.any_pressed() || input.key_pressed(egui::Key::Tab)) {
        end_edit(w);
    }
    let Some(s) = &w.sketch else { return };
    let source = sketch::sketch_definition(s);
    let selected_keys = selection_keys(&source, &s.selection);
    let prefix = format!("{}::feedback", s.sketch_id);
    text(
        ui,
        &format!("{prefix}.summary"),
        "sketch-solve-summary",
        &summary(s),
        if s.error.is_some() {
            "invalid"
        } else {
            "ready"
        },
    );
    if let Some(error) = &s.feedback.error {
        text(
            ui,
            &format!("{prefix}.error"),
            "sketch-feedback-error",
            &format!("Equation feedback unavailable: {error}"),
            "invalid",
        );
    } else if s.error.is_some() {
        text(
            ui,
            &format!("{prefix}.explanation"),
            "sketch-feedback-note",
            "Unsatisfied shows equations at the current authored geometry; it does not identify a minimal conflicting set. The last valid model remains visible.",
            "invalid",
        );
    }
    if let Some(error) = &s.feedback.motion_error {
        text(
            ui,
            &format!("{prefix}.motion-error"),
            "sketch-motion-note",
            &format!("Motion display unavailable: {error}"),
            "unavailable",
        );
        if controls::button(
            ui,
            &format!("{prefix}.retry-analysis"),
            "sketch-repair-action",
            &["compact"],
            "Retry analysis",
            true,
            false,
        )
        .clicked()
        {
            crate::sketch_analysis_ui::retry(w);
            return;
        }
    } else if s.feedback.motion_pending && (motion_display(w) || filter(w) == "dependent") {
        text(
            ui,
            &format!("{prefix}.motion-pending"),
            "sketch-motion-note",
            "Updating sketch freedom…",
            "pending",
        );
    }
    if motion_display(w) && motion(s).is_some() {
        text(
            ui,
            &format!("{prefix}.motion-note"),
            "sketch-motion-note",
            "Marker: constrained position · arrows: local movement directions",
            "ready",
        );
    }
    for element in browser_fields(w) {
        crate::ui::semantic_field(ui, w, &element);
    }
    let selected_filter = filter(w).to_string();
    let mut command = None;
    ui.horizontal_wrapped(|ui| {
        for (key, label) in [
            ("selected", "Selection"),
            ("all", "All"),
            ("unsatisfied", "Unsatisfied"),
            ("dependent", "Dependent"),
        ] {
            if controls::button(
                ui,
                &format!("{prefix}.filter.{key}"),
                "sketch-feedback-filter",
                &["compact"],
                label,
                true,
                selected_filter == key,
            )
            .clicked()
            {
                command = Some(format!("sketch.feedback-filter-{key}"));
            }
        }
    });
    ui.horizontal_wrapped(|ui| {
        for (index, key, label) in [
            (0, "dimensions", "Dimensions"),
            (1, "relations", "Relations"),
        ] {
            if controls::button(
                ui,
                &format!("{prefix}.annotations.{key}"),
                "sketch-annotation-toggle",
                &["compact"],
                label,
                true,
                w.view.sketch_annotation_visibility[index],
            )
            .clicked()
            {
                command = Some(format!("sketch.feedback-toggle-{key}"));
            }
        }
        if controls::button(ui, &format!("{prefix}.motion-toggle"), "sketch-motion-toggle", &["compact"], "Freedom", true, motion_display(w)).on_hover_text("Show local movement directions on selected sketch points. With no selection, show all points.").clicked() {
            command = Some("sketch.feedback-toggle-motion".into());
        }
    });
    ui.horizontal_wrapped(|ui| {
        if controls::button(ui,&format!("{prefix}.clear-filters"),"sketch-feedback-filter",&["compact"],"Clear filters",true,false).clicked(){command=Some("sketch.feedback-clear-filters".into());}
        let duplicates = w.sketch.as_ref().map(|s| s.feedback.duplicates.len()).unwrap_or(0);
        let can_repair = editable(w).is_ok() && w.sketch.as_ref().is_some_and(|s| s.dimension.is_none() && s.dimension_pick.is_none());
        if controls::button(ui, &format!("{prefix}.remove-duplicates"), "sketch-repair-action", &["compact"], &format!("Remove duplicates ({duplicates})"), duplicates > 0 && can_repair, false).on_hover_text("Remove only authored equations with identical kind, ordered geometry, expression, contact and side. Dimension-group IDs are retained.").clicked() {
            command = Some("sketch.feedback-remove-duplicates".into());
        }
        if controls::button(ui, &format!("{prefix}.selected-reference"), "sketch-repair-action", &["compact"], "Selected → Reference", !selected_dimensions(w).is_empty() && can_repair, false).on_hover_text("Make the selected driving dimensions reference dimensions in one pending sketch edit; their expressions are retained.").clicked() {
            command = Some("sketch.feedback-selected-reference".into());
        }
    });
    if let Some(command) = command {
        action(w, &command);
    }
    let rows = visible_rows(w, &source, &selected_keys);
    let inactive = sketch_transform::inactive_entities(&source);
    let count = rows.len();
    let total = source.constraints.len();
    text(
        ui,
        &format!("{prefix}.count"),
        "sketch-feedback-count",
        &format!("{count} of {total} dimensions and relations"),
        "ready",
    );
    let props = controls::properties(
        &format!("{prefix}.list"),
        "sketch-relations-list",
        &["compact"],
        "ready",
    );
    if props.get("display").is_some_and(|value| value == "none") {
        return;
    }
    let height = props
        .get("max-height")
        .and_then(|value| value.trim_end_matches("px").parse::<f32>().ok())
        .filter(|value| value.is_finite() && *value > 0.)
        .unwrap_or(260.);
    let mut clicked = None;
    let mut deleted = None;
    egui::ScrollArea::vertical().id_salt(&prefix).max_height(height).show(ui, |ui| {
        controls::apply_properties(ui, &props);
        for relation in rows {
            let Some(s) = &w.sketch else { break };
            let suppressed = relation.entities.iter().any(|key| inactive.contains(key));
            let selected = relation_state(s, &relation) == "selected";
            let state = if suppressed { "suppressed" } else { relation_state(s, &relation) };
            let unsatisfied = s.feedback.unsatisfied.get(&relation.id).copied();
            let equation = motion(s).and_then(|analysis| analysis.constraints.get(&relation.id));
            let dependency = if let Some(other) = s.feedback.duplicates.get(&relation.id) {
                source.constraints.iter().position(|constraint| &constraint.id == other)
                    .map(|index| format!(" · exact duplicate of {} {}", actions::label(&source.constraints[index].kind), index + 1))
                    .unwrap_or_else(|| " · exact duplicate equation".into())
            } else if let Some(equation) = equation {
                format!(" · equations: {} independent, {} locally dependent, {} with no current positional derivative", equation.independent_equations, equation.dependent_equations, equation.nonpositional_equations)
            } else { String::new() };
            let dimension = actions::is_dimension(&relation.kind);
            let value = if dimension && relation.driven {
                actions::measured_dimension(w.session.visible(), &s.definition, &source, &relation)
                    .map(|value| if actions::dimension_unit(&relation.kind) == Dimension::Angle { w.session.visible().settings.format_angle_radians(value) } else { w.session.visible().settings.format_length(value) })
                    .unwrap_or_else(|error| error.to_string())
            } else if dimension { relation.value.0.clone() } else { String::new() };
            let label = format!("{}{}{}{}", if unsatisfied.is_some() { "! " } else { "" }, actions::label(&relation.kind), if dimension { if relation.driven { " · Ref " } else { " · " } } else { "" }, value);
            let description = format!("{}{}{}{}", relation.entities.iter().map(|key| entity_name(&source, key)).collect::<Vec<_>>().join(" · "), unsatisfied.map(|value| format!(" · maximum scaled equation residual {value:.6e} (mm; angles ×100 mm/rad)")).unwrap_or_default(), dependency, if suppressed { " · inactive geometry" } else { "" });
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // Reserve the independently styled delete target before the
                    // row consumes its remaining width. Full text stays in the
                    // semantic snapshot, accessibility label and hover text.
                    if controls::button(ui, &format!("{}.delete", relation.id), "sketch-constraint-delete", &["compact"], "×", editable(w).is_ok(), false).on_hover_text("Remove this dimension or relation").clicked() { deleted = Some(relation.id.clone()); }
                    let response = controls::button(ui, &relation.id, "sketch-constraint", &["compact", if dimension { "dimension" } else { "relation" }, state], &label, true, selected).on_hover_text(format!("{label}\n{description}"));
                    if response.clicked() { clicked = Some((relation.id.clone(), dimension && response.double_clicked(), ui.input(|input| input.modifiers.command || input.modifiers.shift))); }
                });
            });
            if selected {
                text(ui, &format!("{}.geometry", relation.id), "sketch-constraint-geometry", &description, state);
                if dimension && w.sketch.as_ref().is_some_and(|s| s.dimension.is_none()) { dimension_controls(ui, w, &relation); }
                contact_controls(ui, w, &relation);
                if controls::button(ui,&format!("{}.focus",relation.id),"sketch-constraint-focus",&["compact"],"Show referenced geometry",editable(w).is_ok(),false).clicked()
                    && let Err(error)=focus_relation(w,&relation.id){w.status=error.to_string();}
            }
        }
        if count == 0 {
            let empty = if w.fields.get(SEARCH).is_some_and(|text|!text.trim().is_empty()) {"No dimensions or relations match the search and active filters. Clear filters to see all equations."} else if selected_filter == "dependent" {
                "No exact duplicates or local positional dependencies are available for this geometry. Local dependence is not proof of a global conflict."
            } else if selected_filter == "unsatisfied" {
                if w.sketch.as_ref().is_some_and(|s| s.feedback.error.is_some()) { "Equation feedback is unavailable; correct the reported expression or reference." } else { "No equations exceed the solver's 1e-6 scaled residual threshold at this geometry." }
            } else { "No dimensions or relations refer to the selected geometry." };
            text(ui, &format!("{prefix}.empty"), "sketch-feedback-empty", empty, "ready");
        }
    });
    if let Some((key, edit, additive)) = clicked {
        sketch_ui::select(
            w,
            Some(SketchEntity::Constraint { constraint: key }),
            additive,
        );
        if w.fields.contains_key("smart-dimension-mode") {
            sketch_ui::selection_changed(w);
        } else if edit {
            sketch_ui::smart_dimension(w);
        }
    }
    if let Some(key) = deleted {
        remove(w, &key);
    }
}

fn dimension_controls(ui: &mut egui::Ui, w: &mut Workspace, relation: &SketchConstraint) {
    let can_edit = editable(w).is_ok();
    let mut mode = None;
    ui.horizontal_wrapped(|ui| {
        for (driven, key, label) in [
            (false, "driving", "Driving"),
            (true, "reference", "Reference"),
        ] {
            if controls::button(
                ui,
                &format!("{}.mode.{key}", relation.id),
                "sketch-dimension-mode",
                &["compact"],
                label,
                can_edit,
                relation.driven == driven,
            )
            .clicked()
            {
                mode = Some(driven);
            }
        }
    });
    if let Some(driven) = mode {
        mutate(w, &relation.id, "driven", serde_json::json!(driven));
    }
    let mut value = relation.value.clone();
    let response = ui.add_enabled_ui(can_edit && !relation.driven, |ui| {
        controls::expression(
            ui,
            &format!("{}.value", relation.id),
            if relation.driven {
                "Saved driving expression"
            } else {
                "Value or expression"
            },
            &mut value,
        )
    });
    if response.inner {
        mutate(w, &relation.id, "value", serde_json::json!(value.0));
    }
}

fn contact_controls(ui: &mut egui::Ui, w: &mut Workspace, relation: &SketchConstraint) {
    let Some(contact) = &relation.contact else {
        return;
    };
    let can_edit = editable(w).is_ok();
    let props = controls::properties(
        &format!("{}.contact", relation.id),
        "sketch-contact-fields",
        &["compact"],
        "ready",
    );
    ui.scope(|ui| {
        controls::apply_properties(ui, &props);
        for index in 0..2 {
            if relation.kind == "point_on_curve" && index == 0 {
                continue;
            }
            ui.horizontal(|ui| {
                let mut parameter = contact.parameters[index];
                text(
                    ui,
                    &format!("{}.contact.{index}.label", relation.id),
                    "field-label",
                    &format!("Contact {}", index + 1),
                    "ready",
                );
                let response = ui
                    .add_enabled_ui(can_edit, |ui| {
                        number_field(
                            ui,
                            &format!("{}.contact-parameter-{index}", relation.id),
                            &mut parameter,
                        )
                    })
                    .inner;
                if response.as_ref().is_some_and(|response| response.changed()) {
                    mutate(
                        w,
                        &relation.id,
                        &format!("contact-parameter-{index}"),
                        serde_json::json!(parameter),
                    );
                }
                if response
                    .as_ref()
                    .is_some_and(|response| response.drag_stopped() || response.lost_focus())
                {
                    end_edit(w);
                }
                if controls::button(
                    ui,
                    &format!("{}.contact-free-{index}", relation.id),
                    "presentation-field",
                    &["compact"],
                    "Slide",
                    can_edit,
                    contact.free[index],
                )
                .clicked()
                {
                    mutate(
                        w,
                        &relation.id,
                        &format!("contact-free-{index}"),
                        serde_json::json!(!contact.free[index]),
                    );
                }
            });
        }
        if relation.kind != "point_on_curve" {
            let opposed = relation
                .value
                .0
                .trim()
                .parse::<f64>()
                .is_ok_and(|value| value < 0.);
            if controls::button(
                ui,
                &format!("{}.opposed", relation.id),
                "presentation-field",
                &["compact"],
                "Opposed tangents",
                can_edit,
                opposed,
            )
            .clicked()
            {
                mutate(w, &relation.id, "opposed", serde_json::json!(!opposed));
            }
        }
    });
}

fn number_field(ui: &mut egui::Ui, id: &str, value: &mut f64) -> Option<egui::Response> {
    let props = controls::properties(
        id,
        "presentation-field",
        &["parameter-field", "number-field"],
        "draft",
    );
    if props.get("display").is_some_and(|value| value == "none") {
        return None;
    }
    Some(
        ui.push_id(id, |ui| {
            ui.scope(|ui| {
                controls::apply_properties(ui, &props);
                let size = |key: &str, default: f32| {
                    props
                        .get(key)
                        .and_then(|value| value.trim_end_matches("px").parse::<f32>().ok())
                        .filter(|value| value.is_finite() && *value >= 0.)
                        .unwrap_or(default)
                };
                ui.add_sized(
                    egui::vec2(
                        size("width", 64.),
                        size("height", ui.spacing().interact_size.y),
                    ),
                    egui::DragValue::new(value).speed(0.002).range(0.0..=1.0),
                )
            })
            .inner
        })
        .inner,
    )
}

fn editable(w: &Workspace) -> Result<()> {
    let s = w
        .sketch
        .as_ref()
        .ok_or_else(|| CadError::Invalid("Open a sketch to edit its relations".into()))?;
    if s.edit.is_some() || s.drag.is_some() {
        return Err(CadError::Invalid(
            "Finish or cancel the current sketch operation first".into(),
        ));
    }
    Ok(())
}

fn focus_relation(w: &mut Workspace, key: &str) -> Result<()> {
    editable(w)?;
    let sketch = w
        .sketch
        .as_ref()
        .ok_or_else(|| CadError::Invalid("Open the related sketch".into()))?;
    if sketch.dimension.is_some()
        || sketch.dimension_pick.is_some()
        || !sketch.chain.is_empty()
        || !sketch.clicks.is_empty()
    {
        return Err(CadError::Invalid(
            "Finish or cancel the current sketch gesture before focusing a relation".into(),
        ));
    }
    let source = sketch::sketch_definition(sketch);
    let relation = source
        .constraints
        .iter()
        .find(|relation| relation.id == key)
        .ok_or_else(|| CadError::Invalid("This relation no longer exists".into()))?;
    let keys = geometry_keys(relation)
        .into_iter()
        .map(str::to_owned)
        .collect::<BTreeSet<_>>();
    let mut points = source
        .points
        .iter()
        .filter(|(key, _)| keys.contains(*key))
        .map(|(_, point)| *point)
        .collect::<Vec<_>>();
    if keys.contains(actions::ORIGIN) {
        points.push([0.; 2]);
    }
    points.extend(
        source
            .members
            .iter()
            .filter(|(member, _)| keys.contains(*member))
            .flat_map(|(_, member)| member.curve.points())
            .filter_map(|key| source.points.get(key).copied()),
    );
    // Display samples frame the camera only; they never establish constraints or references.
    // Invalid equations still permit framing the remaining authored points.
    if let Ok(polylines) =
        sketch_geometry::polylines(w.session.visible(), &sketch.definition, &source)
    {
        points.extend(
            polylines
                .into_iter()
                .filter(|polyline| keys.contains(&polyline.member))
                .flat_map(|polyline| polyline.points),
        );
    }
    let mut min = [f64::INFINITY; 2];
    let mut max = [f64::NEG_INFINITY; 2];
    for point in points {
        for axis in 0..2 {
            min[axis] = min[axis].min(point[axis]);
            max[axis] = max[axis].max(point[axis]);
        }
    }
    let center = std::array::from_fn(|axis| (min[axis] + max[axis]) * 0.5);
    let target = (min.iter().chain(max.iter()).all(|value| value.is_finite())).then(|| {
        crate::viewport::render_point(sketch::world(&sketch.plane, &sketch.matrix, center))
            .to_array()
    });
    let extent = ((max[0] - min[0]).hypot(max[1] - min[1]) * 0.014).max(0.08) as f32;
    sketch_ui::select(
        w,
        Some(SketchEntity::Constraint {
            constraint: key.into(),
        }),
        false,
    );
    if let Some(target) = target {
        w.view.target = target;
        w.view.distance = extent;
        w.pending_context_focus = false;
        crate::navigation::stop(w);
    }
    Ok(())
}
fn change(w: &mut Workspace, key: &str, field: &str, value: serde_json::Value) -> Result<()> {
    editable(w)?;
    let s = w.sketch.as_mut().unwrap();
    let before = sketch::sketch_definition(s);
    let relation = s
        .constraints
        .iter_mut()
        .find(|relation| relation.id == key)
        .ok_or_else(|| CadError::Invalid("This dimension or relation no longer exists".into()))?;
    let boolean = |value: &serde_json::Value| {
        value
            .as_bool()
            .ok_or_else(|| CadError::Invalid("Expected a toggle value".into()))
    };
    match field {
        "value" if actions::is_dimension(&relation.kind) && !relation.driven => {
            relation.value.0 = value
                .as_str()
                .ok_or_else(|| CadError::Invalid("Expected a dimension expression".into()))?
                .into();
        }
        "driven" if actions::is_dimension(&relation.kind) => {
            relation.driven = boolean(&value)?;
        }
        "opposed" if relation.contact.is_some() && relation.kind != "point_on_curve" => {
            relation.value = if boolean(&value)? {
                (-1.).into()
            } else {
                1.0.into()
            };
        }
        _ => {
            let index = field
                .rsplit('-')
                .next()
                .and_then(|index| index.parse::<usize>().ok())
                .filter(|index| *index < 2)
                .ok_or_else(|| CadError::Invalid("Relation field is unavailable".into()))?;
            let contact = relation.contact.as_mut().ok_or_else(|| {
                CadError::Invalid("This relation has no editable curve contact".into())
            })?;
            if relation.kind == "point_on_curve" && index == 0 {
                return Err(CadError::Invalid("The point has no curve parameter".into()));
            }
            if field.starts_with("contact-parameter-") {
                contact.parameters[index] = value
                    .as_f64()
                    .filter(|value| value.is_finite() && (0.0..=1.0).contains(value))
                    .ok_or_else(|| {
                        CadError::Invalid("Contact position must be from 0 to 1".into())
                    })?;
            } else if field.starts_with("contact-free-") {
                contact.free[index] = boolean(&value)?;
            } else {
                return Err(CadError::Invalid("Relation field is unavailable".into()));
            }
        }
    }
    if sketch::sketch_definition(s) != before {
        let continuous = field == "value" || field.starts_with("contact-parameter-");
        let continuing = continuous
            && s.feedback
                .edit
                .as_ref()
                .is_some_and(|(previous, previous_field, history_len)| {
                    previous == key && previous_field == field && *history_len == s.history.len()
                });
        if !continuing {
            s.history.push(before);
        }
        s.feedback.edit = continuous.then(|| (key.into(), field.into(), s.history.len()));
        sketch::solve(w);
    }
    Ok(())
}

fn mutate(w: &mut Workspace, key: &str, field: &str, value: serde_json::Value) {
    if let Err(error) = change(w, key, field, value) {
        w.status = error.to_string();
    }
}

fn remove(w: &mut Workspace, key: &str) {
    if let Err(error) = editable(w) {
        w.status = error.to_string();
        return;
    }
    let Some(s) = &mut w.sketch else { return };
    s.feedback.edit = None;
    if !s.constraints.iter().any(|relation| relation.id == key) {
        return;
    }
    s.history.push(sketch::sketch_definition(s));
    s.constraints.retain(|relation| relation.id != key);
    s.selection.retain(
        |entity| !matches!(entity, SketchEntity::Constraint { constraint } if constraint == key),
    );
    if s.dimension
        .as_ref()
        .is_some_and(|editor| editor.constraint == key)
    {
        s.dimension = None;
    }
    let mut source = sketch::sketch_definition(s);
    sketch_dimensions::reconcile_removed(&mut source);
    s.dimension_groups = source.dimension_groups;
    sketch::solve(w);
}

fn selected_dimensions(w: &Workspace) -> BTreeSet<Id> {
    let Some(s) = &w.sketch else {
        return BTreeSet::new();
    };
    s.selection
        .iter()
        .filter_map(|entity| match entity {
            SketchEntity::Constraint { constraint }
                if s.constraints.iter().any(|relation| {
                    relation.id == *constraint
                        && !relation.driven
                        && actions::is_dimension(&relation.kind)
                }) =>
            {
                Some(constraint.clone())
            }
            _ => None,
        })
        .collect()
}

fn repair(w: &mut Workspace, command: &str) -> Result<()> {
    editable(w)?;
    let selected = selected_dimensions(w);
    let s = w.sketch.as_mut().unwrap();
    if s.dimension.is_some() || s.dimension_pick.is_some() {
        return Err(CadError::Invalid(
            "Accept or cancel Smart Dimension before repairing relations".into(),
        ));
    }
    let before = sketch::sketch_definition(s);
    match command {
        "sketch.feedback-remove-duplicates" => {
            // Recompute from the current source rather than trusting cached selection.
            let duplicates = sketch_diagnostics::exact_duplicates(&before);
            if duplicates.is_empty() {
                return Ok(());
            }
            s.constraints
                .retain(|relation| !duplicates.contains_key(&relation.id));
            for entity in &mut s.selection {
                if let SketchEntity::Constraint { constraint } = entity
                    && let Some(retained) = duplicates.get(constraint)
                {
                    *constraint = retained.clone();
                }
            }
            let mut deduplicated = vec![];
            s.selection.retain(|entity| {
                if deduplicated.contains(entity) {
                    false
                } else {
                    deduplicated.push(entity.clone());
                    true
                }
            });
        }
        "sketch.feedback-selected-reference" => {
            if selected.is_empty() {
                return Ok(());
            }
            for relation in &mut s.constraints {
                if selected.contains(&relation.id) {
                    relation.driven = true;
                }
            }
        }
        _ => return Err(CadError::Invalid("Unknown sketch repair action".into())),
    }
    s.history.push(before);
    sketch::solve(w);
    Ok(())
}

pub fn action(w: &mut Workspace, command: &str) -> bool {
    if !command.starts_with("sketch.feedback-") {
        return false;
    }
    end_edit(w);
    match command {
        "sketch.feedback-clear-filters" => {
            w.fields.remove(SEARCH);
            w.fields.remove(CATEGORY);
            w.fields.insert(FILTER.into(), "all".into());
        }
        "sketch.feedback-filter-selected"
        | "sketch.feedback-filter-all"
        | "sketch.feedback-filter-unsatisfied"
        | "sketch.feedback-filter-dependent" => {
            w.fields
                .insert(FILTER.into(), command.rsplit('-').next().unwrap().into());
        }
        "sketch.feedback-toggle-dimensions" => {
            w.view.sketch_annotation_visibility[0] = !w.view.sketch_annotation_visibility[0];
        }
        "sketch.feedback-toggle-relations" => {
            w.view.sketch_annotation_visibility[1] = !w.view.sketch_annotation_visibility[1];
        }
        "sketch.feedback-toggle-motion" => {
            let enabled = !motion_display(w);
            w.fields.insert(MOTION.into(), enabled.to_string());
        }
        "sketch.feedback-retry-analysis" => crate::sketch_analysis_ui::retry(w),
        "sketch.feedback-remove-duplicates" | "sketch.feedback-selected-reference" => {
            if let Err(error) = repair(w, command) {
                w.status = error.to_string();
            }
        }
        _ => return false,
    }
    true
}

pub fn semantic_action(w: &mut Workspace, element: &SemanticElement, command: &str) -> bool {
    if element.role == "sketch-point-motion" && command == "sketch.select-entity" {
        if let Some(point) = element
            .data
            .get("point")
            .and_then(serde_json::Value::as_str)
            && w.sketch.as_ref().is_some_and(|s| {
                element
                    .data
                    .get("sketch")
                    .and_then(serde_json::Value::as_str)
                    == Some(&s.sketch_id)
                    && s.keys.iter().any(|key| key == point)
            })
        {
            sketch_ui::select(
                w,
                Some(SketchEntity::Point {
                    point: point.into(),
                }),
                false,
            );
        }
        return true;
    }
    if command.starts_with("sketch.feedback-")
        && !matches!(
            command,
            "sketch.feedback-driving" | "sketch.feedback-reference"
        )
    {
        if w.sketch.as_ref().is_none_or(|s| {
            element
                .data
                .get("sketch")
                .and_then(serde_json::Value::as_str)
                != Some(&s.sketch_id)
        }) {
            return true;
        }
        return action(w, command);
    }
    if !matches!(
        element.role.as_str(),
        "sketch-constraint"
            | "sketch-constraint-delete"
            | "sketch-dimension-mode"
            | "sketch-constraint-focus"
    ) || w.sketch.as_ref().is_none_or(|s| {
        element.data.get("sketch").and_then(|value| value.as_str()) != Some(&s.sketch_id)
    }) {
        return false;
    }
    let key = element
        .data
        .get("constraint")
        .and_then(|value| value.as_str())
        .unwrap_or(&element.id)
        .to_string();
    match command {
        "sketch.focus-relation" => {
            if let Err(error) = focus_relation(w, &key) {
                w.status = error.to_string();
            }
        }
        "sketch.select-entity" => {
            sketch_ui::select(w, Some(SketchEntity::Constraint { constraint: key }), false)
        }
        "sketch.edit-constraint" => {
            sketch_ui::select(w, Some(SketchEntity::Constraint { constraint: key }), false);
            sketch_ui::smart_dimension(w);
        }
        "sketch.delete-constraint" => remove(w, &key),
        "sketch.feedback-driving" | "sketch.feedback-reference" => mutate(
            w,
            &key,
            "driven",
            serde_json::json!(command == "sketch.feedback-reference"),
        ),
        "sketch.toggle-dimension-mode" => {
            let driven = w
                .sketch
                .as_ref()
                .and_then(|s| s.constraints.iter().find(|relation| relation.id == key))
                .is_some_and(|relation| relation.driven);
            mutate(w, &key, "driven", serde_json::json!(!driven));
        }
        _ => return false,
    }
    true
}

pub fn set_field(
    w: &mut Workspace,
    element: &SemanticElement,
    value: serde_json::Value,
) -> Result<()> {
    let s = w
        .sketch
        .as_ref()
        .ok_or_else(|| CadError::Invalid("The sketch editor has closed".into()))?;
    if element.data.get("sketch").and_then(|value| value.as_str()) != Some(&s.sketch_id) {
        return Err(CadError::Invalid(
            "The feedback belongs to another sketch".into(),
        ));
    }
    let field = element
        .data
        .get("field")
        .and_then(|value| value.as_str())
        .unwrap_or_default();
    match field {
        "search" => {
            let text = value
                .as_str()
                .filter(|text| text.len() <= 512)
                .ok_or_else(|| {
                    CadError::Invalid("Relation search supports at most 512 bytes of text".into())
                })?;
            w.fields.insert(SEARCH.into(), text.into());
            Ok(())
        }
        "category" | "sort" => {
            let text = value
                .as_str()
                .filter(|text| {
                    if field == "category" {
                        matches!(*text, "all" | "dimensions" | "relations")
                    } else {
                        matches!(*text, "authored" | "type" | "status")
                    }
                })
                .ok_or_else(|| {
                    CadError::Invalid("Choose an available relation browser option".into())
                })?;
            w.fields.insert(
                if field == "category" { CATEGORY } else { SORT }.into(),
                text.into(),
            );
            Ok(())
        }
        "filter" => {
            let value = value
                .as_str()
                .filter(|value| matches!(*value, "selected" | "all" | "unsatisfied" | "dependent"))
                .ok_or_else(|| {
                    CadError::Invalid("Choose Selection, All, Unsatisfied or Dependent".into())
                })?;
            w.fields.insert(FILTER.into(), value.into());
            end_edit(w);
            Ok(())
        }
        "show-dimensions" | "show-relations" => {
            w.view.sketch_annotation_visibility[usize::from(field == "show-relations")] = value
                .as_bool()
                .ok_or_else(|| CadError::Invalid("Expected an annotation toggle".into()))?;
            end_edit(w);
            Ok(())
        }
        "show-motion" => {
            let enabled = value
                .as_bool()
                .ok_or_else(|| CadError::Invalid("Expected a motion display toggle".into()))?;
            w.fields.insert(MOTION.into(), enabled.to_string());
            Ok(())
        }
        _ => change(
            w,
            element
                .data
                .get("constraint")
                .and_then(|value| value.as_str())
                .unwrap_or_default(),
            field,
            value,
        ),
    }
}

pub fn field_value(w: &Workspace, element: &SemanticElement) -> Option<serde_json::Value> {
    let s = w.sketch.as_ref()?;
    if element.data.get("sketch")?.as_str()? != s.sketch_id {
        return None;
    }
    let field = element.data.get("field")?.as_str()?;
    match field {
        "search" => Some(serde_json::json!(
            w.fields.get(SEARCH).cloned().unwrap_or_default()
        )),
        "category" => Some(serde_json::json!(
            w.fields
                .get(CATEGORY)
                .cloned()
                .unwrap_or_else(|| "all".into())
        )),
        "sort" => Some(serde_json::json!(
            w.fields
                .get(SORT)
                .cloned()
                .unwrap_or_else(|| "authored".into())
        )),
        "filter" => Some(serde_json::json!(filter(w))),
        "show-dimensions" => Some(serde_json::json!(w.view.sketch_annotation_visibility[0])),
        "show-relations" => Some(serde_json::json!(w.view.sketch_annotation_visibility[1])),
        "show-motion" => Some(serde_json::json!(motion_display(w))),
        _ => {
            let key = element.data.get("constraint")?.as_str()?;
            let relation = s.constraints.iter().find(|relation| relation.id == key)?;
            match field {
                "value" => Some(serde_json::json!(relation.value.0)),
                "driven" => Some(serde_json::json!(relation.driven)),
                "opposed" => Some(serde_json::json!(
                    relation
                        .value
                        .0
                        .trim()
                        .parse::<f64>()
                        .is_ok_and(|value| value < 0.)
                )),
                _ => {
                    let index = field
                        .rsplit('-')
                        .next()?
                        .parse::<usize>()
                        .ok()
                        .filter(|index| *index < 2)?;
                    let contact = relation.contact.as_ref()?;
                    if field.starts_with("contact-parameter-") {
                        Some(serde_json::json!(contact.parameters[index]))
                    } else if field.starts_with("contact-free-") {
                        Some(serde_json::json!(contact.free[index]))
                    } else {
                        None
                    }
                }
            }
        }
    }
}

fn field(
    sketch: &sketch::EditingSketch,
    id: String,
    label: &str,
    key: &str,
    value: serde_json::Value,
    constraint: Option<&str>,
    editable: bool,
) -> SemanticElement {
    let editable =
        editable && (constraint.is_none() || (sketch.edit.is_none() && sketch.drag.is_none()));
    SemanticElement {
        id,
        role: "presentation-field".into(),
        label: label.into(),
        value: Some(value.to_string()),
        state: if editable { "draft" } else { "read-only" }.into(),
        commands: vec!["presentation.set-value".into()],
        relationships: std::iter::once(sketch.sketch_id.clone())
            .chain(constraint.map(str::to_string))
            .collect(),
        data: serde_json::json!({"contract":"tcad.presentation.field","version":1,"setter":"sketch-feedback","sketch":sketch.sketch_id,"field":key,"constraint":constraint,"json":value,"editable":editable,"shared_draft":true}),
    }
}

pub fn semantics(w: &Workspace) -> Vec<SemanticElement> {
    let Some(s) = &w.sketch else { return vec![] };
    let can_edit = s.edit.is_none() && s.drag.is_none();
    let source = sketch::sketch_definition(s);
    let selected_keys = selection_keys(&source, &s.selection);
    let prefix = format!("{}::feedback", s.sketch_id);
    let mut out = vec![SemanticElement {
        id: prefix.clone(),
        role: "sketch-feedback".into(),
        label: "Sketch dimensions and relations".into(),
        value: Some(format!("{} unsatisfied", s.feedback.unsatisfied.len())),
        state: if s.error.is_some() {
            "invalid"
        } else {
            "ready"
        }
        .into(),
        commands: vec![],
        relationships: vec![s.sketch_id.clone()],
        data: serde_json::json!({"renderer":"sketch-feedback","definition":s.definition,"sketch":s.sketch_id,"filter":filter(w),"solve_report":retain_report(s),"equation_residuals":s.feedback.residuals,"maximum_equation_residual":s.feedback.maximum_residual,"unsatisfied":s.feedback.unsatisfied,"residual_error":s.feedback.error,"residual_geometry":"current-authored-geometry","residual_length_unit":"mm","residual_angle_scale_mm_per_rad":100.0,"residual_threshold":1e-6,"minimal_conflicts":false,"current_error":s.error,"shared_draft":true,"motion_analysis":motion(s),"motion_error":s.feedback.motion_error,"motion_pending":s.feedback.motion_pending,"analysis_token":s.feedback.analysis_token,"show_motion":motion_display(w),"exact_duplicates":s.feedback.duplicates,"dependency_geometry":"local-linearized-authoritative-equations"}),
    }];
    out.extend(browser_fields(w));
    out.push(SemanticElement {
        id: format!("{prefix}.clear-filters"),
        role: "sketch-feedback-filter".into(),
        label: "Clear filters".into(),
        value: None,
        state: "ready".into(),
        commands: vec!["sketch.feedback-clear-filters".into()],
        relationships: vec![s.sketch_id.clone()],
        data: serde_json::json!({"sketch":s.sketch_id,"view_only":true}),
    });
    let shown = visible_rows(w, &source, &selected_keys)
        .into_iter()
        .map(|relation| relation.id)
        .collect::<Vec<_>>();
    for (key, role, label, value) in [
        (
            "summary",
            "sketch-solve-summary",
            "Sketch solve report",
            summary(s),
        ),
        (
            "count",
            "sketch-feedback-count",
            "Visible dimensions and relations",
            format!(
                "{} of {} dimensions and relations",
                shown.len(),
                source.constraints.len()
            ),
        ),
    ] {
        out.push(SemanticElement {
            id: format!("{prefix}.{key}"),
            role: role.into(),
            label: label.into(),
            value: Some(value),
            state: if s.error.is_some() {
                "invalid"
            } else {
                "ready"
            }
            .into(),
            commands: vec![],
            relationships: vec![s.sketch_id.clone()],
            data: serde_json::json!({"sketch":s.sketch_id}),
        });
    }
    out.push(SemanticElement {
        id: format!("{prefix}.list"),
        role: "sketch-relations-list".into(),
        label: "Dimensions and relations".into(),
        value: None,
        state: "ready".into(),
        commands: vec![],
        relationships: std::iter::once(s.sketch_id.clone())
            .chain(shown.iter().cloned())
            .collect(),
        data: serde_json::json!({"sketch":s.sketch_id,"filter":filter(w),"shown":shown}),
    });
    for (key, label) in [
        ("selected", "Selection"),
        ("all", "All"),
        ("unsatisfied", "Unsatisfied"),
        ("dependent", "Dependent"),
    ] {
        out.push(SemanticElement {
            id: format!("{prefix}.filter.{key}"),
            role: "sketch-feedback-filter".into(),
            label: label.into(),
            value: None,
            state: if filter(w) == key {
                "selected"
            } else {
                "ready"
            }
            .into(),
            commands: vec![format!("sketch.feedback-filter-{key}")],
            relationships: vec![s.sketch_id.clone()],
            data: serde_json::json!({"sketch":s.sketch_id,"filter":key}),
        });
    }
    out.push(field(
        s,
        format!("{prefix}.filter"),
        "Relation filter",
        "filter",
        serde_json::json!(filter(w)),
        None,
        true,
    ));
    for (index, key, label) in [
        (0, "dimensions", "Dimensions"),
        (1, "relations", "Relations"),
    ] {
        out.push(SemanticElement { id:format!("{prefix}.annotations.{key}"),role:"sketch-annotation-toggle".into(),label:label.into(),value:None,state:if w.view.sketch_annotation_visibility[index] {"selected"} else {"ready"}.into(),commands:vec![format!("sketch.feedback-toggle-{key}")],relationships:vec![s.sketch_id.clone()],data:serde_json::json!({"sketch":s.sketch_id,"visible":w.view.sketch_annotation_visibility[index]}) });
        out.push(field(
            s,
            format!("{prefix}.show-{key}"),
            label,
            &format!("show-{key}"),
            serde_json::json!(w.view.sketch_annotation_visibility[index]),
            None,
            true,
        ));
    }
    out.push(SemanticElement {
        id: format!("{prefix}.motion-toggle"), role: "sketch-motion-toggle".into(),
        label: "Show sketch freedom".into(), value: None,
        state: if motion_display(w) { "selected" } else { "ready" }.into(),
        commands: vec!["sketch.feedback-toggle-motion".into()],
        relationships: vec![s.sketch_id.clone()],
        data: serde_json::json!({"sketch":s.sketch_id,"visible":motion_display(w),"meaning":"infinitesimal-point-movement","finite-travel":false}),
    });
    out.push(field(
        s,
        format!("{prefix}.show-motion"),
        "Show sketch freedom",
        "show-motion",
        serde_json::json!(motion_display(w)),
        None,
        true,
    ));
    out.push(SemanticElement {
        id: format!("{prefix}.motion-note"), role: "sketch-motion-note".into(),
        label: "Sketch freedom legend".into(), value: Some("Marker: constrained position · arrows: local movement directions".into()),
        state: if motion(s).is_some() { "ready" } else if s.feedback.motion_pending { "pending" } else { "unavailable" }.into(), commands: vec![],
        relationships: vec![s.sketch_id.clone()],
        data: serde_json::json!({"sketch":s.sketch_id,"visible":motion_display(w),"motion_error":s.feedback.motion_error,"finite_travel":false}),
    });
    out.push(SemanticElement {
        id: format!("{prefix}.retry-analysis"),
        role: "sketch-repair-action".into(),
        label: "Retry sketch analysis".into(),
        value: None,
        state: if s.feedback.motion_error.is_some() {
            "ready"
        } else {
            "disabled"
        }
        .into(),
        commands: vec!["sketch.feedback-retry-analysis".into()],
        relationships: vec![s.sketch_id.clone()],
        data: serde_json::json!({"sketch":s.sketch_id,"view_only":true,"retry":true}),
    });
    for (key, label, enabled, command) in [
        (
            "remove-duplicates",
            "Remove exact duplicate relations",
            !s.feedback.duplicates.is_empty(),
            "sketch.feedback-remove-duplicates",
        ),
        (
            "selected-reference",
            "Make selected dimensions reference",
            !selected_dimensions(w).is_empty(),
            "sketch.feedback-selected-reference",
        ),
    ] {
        out.push(SemanticElement {
            id: format!("{prefix}.{key}"),
            role: "sketch-repair-action".into(),
            label: label.into(),
            value: None,
            state: if enabled && can_edit && s.dimension.is_none() && s.dimension_pick.is_none() {
                "ready"
            } else {
                "disabled"
            }
            .into(),
            commands: vec![command.into()],
            relationships: vec![s.sketch_id.clone()],
            data: serde_json::json!({"sketch":s.sketch_id,"shared_draft":true,"undoable":true}),
        });
    }
    if let Some(analysis) = motion(s) {
        for (key, point) in &analysis.points {
            let state = if point.directions.is_empty() {
                "constrained"
            } else if point.directions.len() == 1 {
                "one-direction"
            } else {
                "two-directions"
            };
            out.push(SemanticElement {
                id: format!("{}::motion.{key}", s.sketch_id), role: "sketch-point-motion".into(),
                label: format!("{} local movement", entity_name(&source, key)),
                value: Some(format!("{} directions{}", point.directions.len(), if point.generated { " · follows sources" } else { "" })),
                state: state.into(), commands: vec!["sketch.select-entity".into()],
                relationships: vec![s.sketch_id.clone(), key.clone()],
                data: serde_json::json!({"version":analysis.version,"sketch":s.sketch_id,"point":key,"local_directions":point.directions,"generated":point.generated,"position_mm":source.points.get(key),"frame":"sketch-XY","meaning":"infinitesimal-point-movement","finite_travel":false,"visible":motion_display(w) && (s.selection.is_empty() || selected_keys.contains(key))}),
            });
        }
    }
    for mut element in presentation::sketch_elements(w.session.visible(), &s.definition, &source)
        .into_iter()
        .filter(|element| element.role == "sketch-constraint")
    {
        let Some(relation) = source
            .constraints
            .iter()
            .find(|relation| relation.id == element.id)
        else {
            continue;
        };
        let state = relation_state(s, relation);
        let suppressed = element.state == "suppressed";
        if !suppressed {
            element.state = state.into();
        }
        if let Some(data) = element.data.as_object_mut() {
            data.insert(
                "shown_in_relation_list".into(),
                serde_json::json!(visible(w, &source, &selected_keys, relation)),
            );
            data.insert(
                "equation_residual".into(),
                serde_json::json!(s.feedback.residuals.get(&relation.id)),
            );
            data.insert(
                "related_geometry".into(),
                serde_json::json!(geometry_keys(relation)),
            );
            data.insert(
                "local_equation_diagnostic".into(),
                serde_json::json!(
                    motion(s).and_then(|analysis| analysis.constraints.get(&relation.id))
                ),
            );
            data.insert(
                "exact_duplicate_of".into(),
                serde_json::json!(s.feedback.duplicates.get(&relation.id)),
            );
        }
        out.push(element);
        out.push(SemanticElement{id:format!("{}.focus",relation.id),role:"sketch-constraint-focus".into(),label:"Show referenced geometry".into(),value:None,state:if can_edit{"ready"}else{"disabled"}.into(),commands:vec!["sketch.focus-relation".into()],relationships:std::iter::once(s.sketch_id.clone()).chain(geometry_keys(relation).into_iter().map(str::to_owned)).collect(),data:serde_json::json!({"sketch":s.sketch_id,"constraint":relation.id,"view_only":true,"geometry":geometry_keys(relation),"geometry_labels":geometry_keys(relation).into_iter().map(|key|entity_name(&source,key)).collect::<Vec<_>>()})});
        out.push(SemanticElement {
            id: format!("{}.delete", relation.id),
            role: "sketch-constraint-delete".into(),
            label: "Remove dimension or relation".into(),
            value: None,
            state: if can_edit { "ready" } else { "disabled" }.into(),
            commands: vec!["sketch.delete-constraint".into()],
            relationships: vec![s.sketch_id.clone(), relation.id.clone()],
            data: serde_json::json!({"sketch":s.sketch_id,"constraint":relation.id}),
        });
        if actions::is_dimension(&relation.kind) {
            for (driven, key, label) in [
                (false, "driving", "Driving"),
                (true, "reference", "Reference"),
            ] {
                out.push(SemanticElement {
                    id: format!("{}.mode.{key}", relation.id),
                    role: "sketch-dimension-mode".into(),
                    label: label.into(),
                    value: None,
                    state: if !can_edit {
                        "disabled"
                    } else if relation.driven == driven {
                        "selected"
                    } else {
                        "ready"
                    }
                    .into(),
                    commands: vec![format!("sketch.feedback-{key}")],
                    relationships: vec![s.sketch_id.clone(), relation.id.clone()],
                    data: serde_json::json!({"sketch":s.sketch_id,"constraint":relation.id}),
                });
            }
            out.push(field(
                s,
                format!("{}.value", relation.id),
                "Value or expression",
                "value",
                serde_json::json!(relation.value.0),
                Some(&relation.id),
                !relation.driven,
            ));
            out.push(field(
                s,
                format!("{}.driven", relation.id),
                "Reference dimension",
                "driven",
                serde_json::json!(relation.driven),
                Some(&relation.id),
                true,
            ));
        }
        if let Some(contact) = &relation.contact {
            for index in 0..2 {
                if relation.kind == "point_on_curve" && index == 0 {
                    continue;
                }
                for (key, label, value) in [
                    (
                        format!("contact-parameter-{index}"),
                        format!("Contact {} position", index + 1),
                        serde_json::json!(contact.parameters[index]),
                    ),
                    (
                        format!("contact-free-{index}"),
                        format!("Contact {} slides", index + 1),
                        serde_json::json!(contact.free[index]),
                    ),
                ] {
                    out.push(field(
                        s,
                        format!("{}.{key}", relation.id),
                        &label,
                        &key,
                        value,
                        Some(&relation.id),
                        true,
                    ));
                }
            }
            if relation.kind != "point_on_curve" {
                out.push(field(
                    s,
                    format!("{}.opposed", relation.id),
                    "Opposed parameter tangents",
                    "opposed",
                    serde_json::json!(
                        relation
                            .value
                            .0
                            .trim()
                            .parse::<f64>()
                            .is_ok_and(|value| value < 0.)
                    ),
                    Some(&relation.id),
                    true,
                ));
            }
        }
    }
    out
}
