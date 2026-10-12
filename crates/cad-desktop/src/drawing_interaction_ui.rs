//! Paper interaction is a presentation controller over validated f64 drawing actions.
use crate::{Workspace, presentation_controls as controls};
use bevy_egui::egui::{self, Color32, Pos2, Rect, Stroke};
use cad_core::{
    CadError, Dimension, Drawing, Expr, Id, Result, Session,
    drawing::{Anchor, DimensionKind, Primitive},
    drawing_interaction::{
        self as interaction, Action, AnchorSlot, Handle, HandleKind, HitTarget, ReferenceHit,
        Scene, Target,
    },
    presentation::SemanticElement,
    protocol::{Evaluation, RequestIdentity},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{collections::BTreeMap, sync::Arc};

#[derive(Clone, Debug, Default)]
struct PaperView {
    zoom: f32,
    pan: egui::Vec2,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum DimensionMode {
    #[default]
    Auto,
    Aligned,
    Horizontal,
    Vertical,
    Perpendicular,
    Diameter,
    Radius,
}
#[derive(Clone, Debug, Default)]
enum Tool {
    #[default]
    Select,
    Dimension {
        first: Option<Box<ReferenceHit>>,
    },
    Reattach {
        target: Target,
        slot: AnchorSlot,
    },
    ReattachCurve {
        target: Target,
    },
}
struct Cache {
    geometry: cad_core::Document,
    evaluation: Arc<Evaluation>,
    source: Drawing,
    page: u32,
    paper: Arc<cad_core::drawing::Sheet>,
    scene: Arc<Scene>,
}
struct Capture {
    session: Session,
    fields: BTreeMap<String, String>,
    source: Drawing,
    scene: Arc<Scene>,
    sheet: Id,
    page: u32,
    target: Target,
    handle: Handle,
    point_mm: [f64; 2],
    bounds_mm: [[f64; 2]; 2],
    expected: RequestIdentity,
    viewport_id: String,
    viewport: Rect,
    paper_rect: Rect,
    scale: f32,
    last_pointer: Pos2,
    click_move: bool,
    error: Option<String>,
}
#[derive(Default)]
pub struct State {
    cache: Option<Cache>,
    views: BTreeMap<String, PaperView>,
    selection: Option<(Id, u32, Target)>,
    hover: Option<Target>,
    context_target: Option<Target>,
    tool: Tool,
    capture: Option<Capture>,
    guides: Vec<[[f64; 2]; 2]>,
    grid: bool,
    grid_mm: f64,
    snapping: bool,
    dimension_mode: DimensionMode,
    dimension_hint: Option<String>,
    errors: BTreeMap<String, String>,
    statuses: BTreeMap<String, (String, String)>,
}
fn invalid(message: &str) -> CadError {
    CadError::Invalid(format!("Drawing interaction: {message}"))
}
fn page(w: &Workspace) -> u32 {
    w.fields
        .get("drawing-page")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0)
}
fn sheet(w: &Workspace) -> Option<Drawing> {
    crate::drawing_ui::sheet_source(w, w.fields.get("drawing-preview")?.as_str())
}
fn geometry_equal(a: &cad_core::Document, b: &cad_core::Document) -> bool {
    a.id == b.id
        && a.root == b.root
        && a.parts == b.parts
        && a.assemblies == b.assemblies
        && a.active_configurations == b.active_configurations
        && a.resources == b.resources
        && a.materials == b.materials
        && a.finishes == b.finishes
        && a.settings == b.settings
        && a.libraries == b.libraries
        && a.properties == b.properties
}
fn current_native(w: &Workspace) -> Option<&Evaluation> {
    w.session
        .draft
        .as_ref()
        .and_then(|d| d.evaluation.as_ref())
        .or(w.session.evaluation.as_ref())
        .filter(|e| (e.valid || e.geometry_valid) && e.identity == w.session.identity())
}
fn session_native(session: &Session) -> Option<&Evaluation> {
    session
        .draft
        .as_ref()
        .and_then(|d| d.evaluation.as_ref())
        .or(session.evaluation.as_ref())
        .filter(|e| (e.valid || e.geometry_valid) && e.identity == session.identity())
}
fn refresh(w: &Workspace, state: &mut State, source: &Drawing, page: u32) -> Result<()> {
    let current = current_native(w);
    if current.is_none()
        && w.session
            .draft
            .as_ref()
            .is_some_and(|draft| !draft.pending && draft.error.is_some())
    {
        return Err(invalid(
            "Current native geometry failed validation; correct the model before picking drawing references",
        ));
    }
    let can_reuse = state.cache.as_ref().is_some_and(|cache| {
        cache.evaluation.identity.backend == w.session.backend
            && cache.evaluation.identity.protocol == cad_core::protocol::PROTOCOL_VERSION
            && geometry_equal(&cache.geometry, w.session.visible())
    });
    if current.is_none() && !can_reuse {
        return Err(invalid(
            "Wait for current model geometry before interacting with the sheet",
        ));
    }
    let geometry_changed = current.is_some_and(|e| {
        state
            .cache
            .as_ref()
            .is_none_or(|c| c.evaluation.identity != e.identity)
    });
    if geometry_changed || state.cache.is_none() || !can_reuse {
        let evaluation = current
            .ok_or_else(|| invalid("Current native geometry is unavailable"))?
            .clone();
        let paper =
            cad_core::drawing::build_sheet_preview(w.session.visible(), &evaluation, source, page)?;
        let scene = interaction::build(w.session.visible(), &evaluation, source, &paper, page)?;
        state.cache = Some(Cache {
            geometry: w.session.visible().clone(),
            evaluation: Arc::new(evaluation),
            source: source.clone(),
            page,
            paper: Arc::new(paper),
            scene: Arc::new(scene),
        });
    } else if let Some(cache) = state.cache.as_mut()
        && (&cache.source != source || cache.page != page)
    {
        cache.paper = Arc::new(cad_core::drawing::build_sheet_preview(
            w.session.visible(),
            &cache.evaluation,
            source,
            page,
        )?);
        cache.scene = Arc::new(interaction::build(
            w.session.visible(),
            &cache.evaluation,
            source,
            &cache.paper,
            page,
        )?);
        cache.source = source.clone();
        cache.page = page;
    }
    if let Some((id, selected_page, target)) = &state.selection
        && (id != &source.id
            || *selected_page != page
            || state
                .cache
                .as_ref()
                .is_none_or(|c| !c.scene.targets.iter().any(|h| &h.target == target)))
    {
        state.selection = None;
        state.context_target = None;
        state.tool = Tool::Select;
    }
    if let Tool::Dimension { first: Some(first) } = &mut state.tool
        && let Some(cache) = &state.cache
    {
        let refreshed = cad_core::drawing::point(
            w.session.visible(),
            &cache.evaluation,
            source,
            first.model_point,
            &Some(first.point.clone()),
        )
        .and_then(|point| {
            cache
                .paper
                .projections
                .get(&first.view)
                .ok_or_else(|| invalid("The first dimension view was removed"))
                .and_then(|projection| {
                    projection
                        .paper_point(&first.view, point)
                        .map(|paper| (point, paper))
                })
        });
        if let Ok((model, paper)) = refreshed {
            first.model_point = model;
            first.paper_mm = paper;
        } else {
            state.tool = Tool::Dimension { first: None };
        }
    }
    Ok(())
}
fn view_key(source: &Drawing, page: u32) -> String {
    format!("{}:{page}", source.id)
}
fn fitted(view: &PaperView, viewport: Rect, paper: [f64; 2]) -> (Rect, f32) {
    let fit = ((viewport.width() - 24.).max(1.) / paper[0] as f32)
        .min((viewport.height() - 24.).max(1.) / paper[1] as f32);
    let scale = fit * if view.zoom == 0. { 1. } else { view.zoom };
    (
        Rect::from_center_size(
            viewport.center() + view.pan,
            egui::vec2(paper[0] as f32 * scale, paper[1] as f32 * scale),
        ),
        scale,
    )
}
fn at(rect: Rect, scale: f32, point: [f64; 2]) -> Pos2 {
    egui::pos2(
        rect.left() + point[0] as f32 * scale,
        rect.bottom() - point[1] as f32 * scale,
    )
}
fn paper_point(rect: Rect, scale: f32, point: Pos2) -> [f64; 2] {
    [
        (point.x - rect.left()) as f64 / scale as f64,
        (rect.bottom() - point.y) as f64 / scale as f64,
    ]
}
fn selected(state: &State) -> Option<&Target> {
    state.selection.as_ref().map(|s| &s.2)
}
fn select(w: &mut Workspace, state: &mut State, source: &Drawing, page: u32, target: Target) {
    state.selection = Some((source.id.clone(), page, target.clone()));
    w.fields.insert("drawing-active".into(), source.id.clone());
    match target {
        Target::View { key } => {
            w.fields.insert("drawing-section".into(), "views".into());
            w.fields.insert("drawing-view".into(), key);
        }
        Target::Dimension { id } | Target::Annotation { id } => {
            w.fields.insert("drawing-section".into(), "items".into());
            w.fields.insert("drawing-item".into(), id);
        }
        Target::Table { id } => {
            w.fields.insert("drawing-section".into(), "tables".into());
            w.fields.insert("drawing-table".into(), id.clone());
            w.fields.insert("drawing-item".into(), id);
        }
        Target::TableSegment { table, segment } => {
            w.fields.insert("drawing-section".into(), "tables".into());
            w.fields.insert("drawing-table".into(), table.clone());
            w.fields.insert("drawing-item".into(), table);
            w.fields.insert("drawing-table-segment".into(), segment);
        }
        Target::TitleCell { id } => {
            w.fields.insert("drawing-section".into(), "format".into());
            w.fields.insert("drawing-title-cell".into(), id);
        }
    }
    w.selected = None;
    w.geometry_selection.clear();
    w.selection_popup = None;
    w.manager_tree = false;
}
fn publish(w: &mut Workspace, source: Drawing) -> Result<()> {
    let id = source.id.clone();
    crate::drawing_ui::put(w, source);
    if !crate::drawing_ui::current_source_is_valid(w) {
        return Err(invalid(&w.status));
    }
    if !w.session.visible().drawings.iter().any(|s| s.id == id) {
        return Err(invalid("The sheet command was rejected"));
    }
    Ok(())
}
fn apply(w: &mut Workspace, state: &mut State, action: &Action) -> Result<()> {
    if !crate::drawing_editor_ui::current_source_is_valid(w)
        || !crate::drawing_ui::current_source_is_valid(w)
    {
        return Err(invalid(
            "Correct the staged drawing fields before changing its paper geometry",
        ));
    }
    let source = sheet(w).ok_or_else(|| invalid("Select a drawing sheet"))?;
    refresh(w, state, &source, page(w))?;
    let cache = state
        .cache
        .as_ref()
        .ok_or_else(|| invalid("Paper geometry is unavailable"))?;
    let updated = interaction::apply(
        w.session.visible(),
        &cache.evaluation,
        &source,
        page(w),
        action,
    )?;
    publish(w, updated)
}
struct CaptureRequest<'a> {
    source: &'a Drawing,
    page: u32,
    target: &'a HitTarget,
    handle: Handle,
    pointer: Pos2,
    viewport_id: &'a str,
    viewport: Rect,
    paper: Rect,
    scale: f32,
    click_move: bool,
}
fn begin_capture(w: &Workspace, state: &mut State, request: CaptureRequest<'_>) -> Result<()> {
    let CaptureRequest {
        source,
        page,
        target,
        handle,
        pointer,
        viewport_id,
        viewport,
        paper,
        scale,
        click_move,
    } = request;
    if !crate::drawing_ui::current_source_is_valid(w)
        || !crate::drawing_editor_ui::current_source_is_valid(w)
    {
        return Err(invalid("Correct the drawing fields before dragging"));
    }
    if current_native(w).is_none() {
        return Err(invalid(
            "Wait for current model and drawing validation before starting a drag",
        ));
    }
    let scene = state
        .cache
        .as_ref()
        .ok_or_else(|| invalid("Paper geometry is unavailable"))?
        .scene
        .clone();
    state.capture = Some(Capture {
        session: w.session.clone(),
        fields: w.fields.clone(),
        source: source.clone(),
        scene,
        sheet: source.id.clone(),
        page,
        target: target.target.clone(),
        handle,
        point_mm: paper_point(paper, scale, pointer),
        bounds_mm: target.bounds_mm,
        expected: w.session.identity(),
        viewport_id: viewport_id.into(),
        viewport,
        paper_rect: paper,
        scale,
        last_pointer: pointer,
        click_move,
        error: None,
    });
    Ok(())
}
fn capture_action(capture: &Capture, delta: [f64; 2]) -> Result<Action> {
    Ok(match &capture.handle.kind {
        HandleKind::Resize { corner } => {
            let mut min = capture.bounds_mm[0];
            let mut max = capture.bounds_mm[1];
            if corner & 1 == 0 {
                min[0] += delta[0]
            } else {
                max[0] += delta[0]
            };
            if corner & 2 == 0 {
                min[1] += delta[1]
            } else {
                max[1] += delta[1]
            };
            Action::Resize {
                target: capture.target.clone(),
                rect_mm: [min[0], min[1], max[0] - min[0], max[1] - min[1]],
            }
        }
        HandleKind::LeaderBend { index } => {
            let Target::Annotation { id } = &capture.target else {
                return Err(invalid("Only annotations have elbow handles"));
            };
            let original = capture
                .source
                .entities
                .iter()
                .find(|annotation| &annotation.id == id)
                .and_then(|annotation| annotation.leader_points_mm.get(*index))
                .copied()
                .ok_or_else(|| invalid("The selected leader elbow no longer exists"))?;
            Action::MoveLeaderBend {
                id: match &capture.target {
                    Target::Annotation { id } => id.clone(),
                    _ => String::new(),
                },
                index: *index,
                offset_mm: [original[0] + delta[0], original[1] + delta[1]],
            }
        }
        HandleKind::Anchor { .. } => {
            return Err(invalid(
                "Attachment drags require a current native geometry pick",
            ));
        }
        _ => Action::Move {
            target: capture.target.clone(),
            delta_mm: delta,
        },
    })
}
fn update_capture(w: &mut Workspace, state: &mut State, pointer: Pos2) -> Result<()> {
    let Some(mut capture) = state.capture.take() else {
        return Ok(());
    };
    if capture.expected != w.session.identity()
        || sheet(w).is_none_or(|s| s.id != capture.sheet)
        || page(w) != capture.page
        || !geometry_equal(capture.session.visible(), w.session.visible())
    {
        state.guides.clear();
        return Err(invalid(
            "Drawing context changed during the gesture; start it again",
        ));
    }
    if capture.last_pointer.distance(pointer) < 0.05 {
        state.capture = Some(capture);
        return Ok(());
    }
    capture.last_pointer = pointer;
    let point = paper_point(capture.paper_rect, capture.scale, pointer);
    let mut delta = [
        point[0] - capture.point_mm[0],
        point[1] - capture.point_mm[1],
    ];
    let moved = [
        capture.handle.at_mm[0] + delta[0],
        capture.handle.at_mm[1] + delta[1],
    ];
    let snap = if state.snapping {
        interaction::snap(
            &capture.scene,
            &capture.target,
            moved,
            6. / capture.scale as f64,
            state.grid.then_some(state.grid_mm.max(0.01)),
        )
    } else {
        interaction::Snap {
            point: moved,
            guides: vec![],
        }
    };
    state.guides = snap.guides;
    delta = [
        snap.point[0] - capture.handle.at_mm[0],
        snap.point[1] - capture.handle.at_mm[1],
    ];
    let result = (|| -> Result<()> {
        let action = if let HandleKind::Anchor { slot } = &capture.handle.kind {
            let hit = interaction::nearest_reference(
                &capture.scene,
                point,
                8. / capture.scale as f64,
                capture
                    .scene
                    .targets
                    .iter()
                    .find(|h| h.target == capture.target)
                    .and_then(|h| h.view.as_deref()),
            )
            .ok_or_else(|| invalid("Drag the attachment onto current model geometry"))?;
            Action::ReattachPoint {
                target: capture.target.clone(),
                slot: slot.clone(),
                reference: hit.point,
            }
        } else {
            capture_action(&capture, delta)?
        };
        let native = session_native(&capture.session)
            .ok_or_else(|| invalid("Gesture-start model geometry is unavailable"))?;
        let updated = interaction::apply(
            w.session.visible(),
            native,
            &capture.source,
            capture.page,
            &action,
        )?;
        publish(w, updated)?;
        capture.expected = w.session.identity();
        Ok(())
    })();
    capture.error = result.as_ref().err().map(ToString::to_string);
    state.capture = Some(capture);
    result
}
pub fn active(w: &Workspace) -> bool {
    w.drawing_interaction.capture.is_some()
}
pub fn current_source_is_valid(w: &Workspace) -> bool {
    !active(w) && w.drawing_interaction.errors.is_empty()
}
pub fn reset(w: &mut Workspace) {
    w.drawing_interaction = State::default();
}
pub fn escape(w: &mut Workspace) -> bool {
    let mut state = std::mem::take(&mut w.drawing_interaction);
    let handled = if let Some(capture) = state.capture.take() {
        if capture.expected == w.session.identity() {
            w.worker.cancel();
            w.session = capture.session;
            w.fields = capture.fields;
            w.requested = None;
            w.changed = std::time::Instant::now();
        }
        state.cache = None;
        state.guides.clear();
        w.confirm_requested = false;
        w.status = "Drawing gesture cancelled; earlier drawing edits retained".into();
        true
    } else if !matches!(state.tool, Tool::Select) {
        state.tool = Tool::Select;
        state.guides.clear();
        w.status = "Drawing selection tool".into();
        true
    } else {
        false
    };
    w.drawing_interaction = state;
    handled
}
fn restore_capture(w: &mut Workspace, state: &mut State, capture: Capture, reason: &str) {
    if capture.expected == w.session.identity() {
        w.worker.cancel();
        w.session = capture.session;
        w.fields = capture.fields;
        w.requested = None;
        w.changed = std::time::Instant::now();
    }
    state.cache = None;
    state.guides.clear();
    w.confirm_requested = false;
    w.status = reason.into();
}
fn end_capture(w: &mut Workspace, state: &mut State) {
    if let Some(capture) = state.capture.take()
        && let Some(error) = &capture.error
    {
        let message = format!("Drawing gesture reverted: {error}");
        restore_capture(w, state, capture, &message);
    }
    state.guides.clear();
}
fn identity(w: &Workspace) -> Value {
    json!({"contract":"tcad.drawing-interaction","version":interaction::CONTRACT_VERSION,"document":w.session.visible().id,"identity":w.session.identity(),"sheet":w.fields.get("drawing-preview"),"page":page(w),"editor":w.fields.get("drawing-editor-generation")})
}
fn field(
    w: &Workspace,
    key: &str,
    label: &str,
    value: Value,
    kind: &str,
    unit: Option<&str>,
    choices: Vec<Value>,
) -> SemanticElement {
    let mut data = identity(w);
    data["setter"] = json!("drawing-interaction");
    data["field"] = json!(key);
    data["value_type"] = json!(kind);
    data["json"] = value;
    data["editable"] = json!(!active(w) && w.settings.is_none());
    data["unit"] = json!(unit);
    data["accepts_quantity_text"] = json!(unit.is_some());
    data["choices"] = json!(choices);
    data["accessibility_label"] = json!(label);
    SemanticElement {
        id: format!("drawing-interaction:{key}"),
        role: "drawing-interaction-field".into(),
        label: label.into(),
        value: None,
        state: if active(w) { "disabled" } else { "ready" }.into(),
        commands: vec![],
        relationships: vec![],
        data,
    }
}
fn command(w: &Workspace, key: &str, label: &str, enabled: bool) -> SemanticElement {
    let mut data = identity(w);
    data["enabled"] = json!(enabled && w.settings.is_none());
    data["actions"] = json!([{"command":key,"label":label,"enabled":enabled && w.settings.is_none(),"arguments":data.clone()}]);
    data["accessibility_label"] = json!(label);
    SemanticElement {
        id: format!("command:{key}"),
        role: "command".into(),
        label: label.into(),
        value: None,
        state: if enabled { "available" } else { "disabled" }.into(),
        commands: vec![key.into()],
        relationships: vec![],
        data,
    }
}
fn supports_curve(source: &Drawing, target: &Target) -> bool {
    match target {
        Target::Dimension { id } => source.dimensions.iter().any(|dimension| {
            &dimension.id == id
                && matches!(
                    dimension.options.kind,
                    DimensionKind::Diameter | DimensionKind::Radius | DimensionKind::HoleCallout
                )
        }),
        Target::Annotation { id } => source.entities.iter().any(|annotation| {
            &annotation.id == id
                && matches!(
                    annotation.kind,
                    cad_core::drawing::AnnotationKind::CenterMark { .. }
                )
        }),
        _ => false,
    }
}
fn supports_leader(source: &Drawing, target: &Target) -> bool {
    if let Target::Annotation { id } = target {
        source.entities.iter().any(|annotation| {
            &annotation.id == id
                && matches!(
                    annotation.kind,
                    cad_core::drawing::AnnotationKind::Leader { .. }
                        | cad_core::drawing::AnnotationKind::Datum { .. }
                        | cad_core::drawing::AnnotationKind::GeometricTolerance { .. }
                        | cad_core::drawing::AnnotationKind::Balloon { .. }
                        | cad_core::drawing::AnnotationKind::CutListBalloon { .. }
                )
        })
    } else {
        false
    }
}
fn selected_commands(w: &Workspace, state: &State) -> Vec<SemanticElement> {
    let mut out = vec![];
    let Some(target) = selected(state) else {
        return out;
    };
    let Some(source) = sheet(w) else {
        return out;
    };
    let enabled = state.capture.is_none() && page(w) == 0;
    let handles = state
        .cache
        .as_ref()
        .and_then(|cache| {
            cache
                .scene
                .targets
                .iter()
                .find(|item| &item.target == target)
        })
        .map(|item| item.handles.as_slice())
        .unwrap_or(&[]);
    for handle in handles {
        if let HandleKind::Anchor { slot } = &handle.kind {
            let mut element = command(
                w,
                "drawing.interact-reattach",
                &format!("Reattach {slot:?}"),
                enabled,
            );
            element.data["target"] = json!(target);
            element.data["slot"] = json!(slot);
            element.id = format!("{}:{slot:?}", element.id);
            out.push(element);
        }
    }
    if supports_curve(&source, target) {
        let mut element = command(
            w,
            "drawing.interact-reattach-curve",
            "Reattach Circle",
            enabled,
        );
        element.data["target"] = json!(target);
        out.push(element);
    }
    if supports_leader(&source, target) {
        out.push(command(
            w,
            "drawing.interact-leader-bend",
            "Add Leader Elbow",
            enabled,
        ));
    }
    out
}
pub fn semantics(w: &Workspace) -> Vec<SemanticElement> {
    if w.dialog.as_deref() != Some("drawings") {
        return vec![];
    }
    let state = &w.drawing_interaction;
    let selection = selected(state);
    let mut out = vec![
        command(w, "drawing.interact-select", "Select", !active(w)),
        command(
            w,
            "drawing.interact-dimension",
            "Smart Dimension",
            !active(w),
        ),
        command(w, "drawing.interact-fit", "Fit Sheet", !active(w)),
        field(
            w,
            "snap",
            "Snap",
            json!(state.snapping),
            "boolean",
            None,
            vec![],
        ),
        field(
            w,
            "grid",
            "Grid",
            json!(state.grid),
            "boolean",
            None,
            vec![],
        ),
        field(
            w,
            "grid-mm",
            "Grid spacing",
            json!(if state.grid_mm == 0. {
                1.
            } else {
                state.grid_mm
            }),
            "quantity",
            Some("mm"),
            vec![],
        ),
        field(
            w,
            "dimension-mode",
            "Dimension",
            json!(state.dimension_mode),
            "enum",
            None,
            [
                "auto",
                "aligned",
                "horizontal",
                "vertical",
                "perpendicular",
                "diameter",
                "radius",
            ]
            .into_iter()
            .map(|v| json!({"label":v,"value":v}))
            .collect(),
        ),
    ];
    if let Some(hint) = &state.dimension_hint {
        out.push(SemanticElement {
            id: "drawing.dimension-inference".into(),
            role: "drawing-dimension-inference".into(),
            label: hint.clone(),
            value: None,
            state: "ready".into(),
            commands: vec![],
            relationships: vec![],
            data: identity(w),
        });
    }
    if let Some(cache) = &state.cache {
        out.push(field(w,"page","Page",json!(page(w)),"integer",None,(0..cad_core::drawing_tables::page_count(&cache.source)).map(|p|json!({"label":if p==0{"Model sheet".into()}else{format!("Continuation {p}")},"value":p})).collect()));
        for target in &cache.scene.targets {
            let mut data = identity(w);
            data["target"] = json!(target.target);
            data["bounds_mm"] = json!(target.bounds_mm);
            data["handles"] = json!(target.handles);
            out.push(SemanticElement {
                id: format!(
                    "{}:paper:{}:{}",
                    cache.source.id,
                    cache.page,
                    target.target.identity()
                ),
                role: "drawing-paper-item".into(),
                label: target.target.identity(),
                value: target.diagnostic.clone(),
                state: if selection == Some(&target.target) {
                    "selected"
                } else if target.diagnostic.is_some() {
                    "invalid"
                } else {
                    "ready"
                }
                .into(),
                commands: vec!["drawing.interact-select-item".into()],
                relationships: vec![cache.source.id.clone()],
                data,
            });
            for handle in &target.handles {
                let mut data = identity(w);
                data["target"] = json!(target.target);
                data["handle"] = json!(handle);
                data["paper_units"] = json!("mm");
                data["classes"] = json!(["paper-handle", handle_class(&handle.kind)]);
                out.push(SemanticElement {
                    id: format!(
                        "{}:paper:{}:handle:{}",
                        cache.source.id, cache.page, handle.id
                    ),
                    role: "drawing-paper-handle".into(),
                    label: format!(
                        "{} {}",
                        target.target.identity(),
                        handle_class(&handle.kind)
                    ),
                    value: None,
                    state: if selection == Some(&target.target) {
                        "selected"
                    } else {
                        "hidden"
                    }
                    .into(),
                    commands: vec!["drawing.interact-select-item".into()],
                    relationships: vec![target.id.clone()],
                    data,
                });
            }
        }
    }
    for (viewport, (message, status)) in &state.statuses {
        let mut data = identity(w);
        data["viewport"] = json!(viewport);
        data["classes"] = json!(["viewport-overlay", "paper-status"]);
        data["diagnostics"] = json!(if status == "error" {
            vec![message.clone()]
        } else {
            vec![]
        });
        out.push(SemanticElement {
            id: format!("drawing.paper-status:{viewport}"),
            role: "drawing-paper-status".into(),
            label: message.clone(),
            value: None,
            state: status.clone(),
            commands: vec![],
            relationships: vec![viewport.clone()],
            data,
        });
    }
    for (index, guide) in state.guides.iter().enumerate() {
        let mut data = identity(w);
        data["points_mm"] = json!(guide);
        data["paper_units"] = json!("mm");
        out.push(SemanticElement {
            id: format!("drawing.snap-guide:{index}"),
            role: "drawing-snap-guide".into(),
            label: "Paper alignment guide".into(),
            value: None,
            state: "active".into(),
            commands: vec![],
            relationships: vec![],
            data,
        });
    }
    if let Tool::Dimension { first: Some(first) } = &state.tool {
        let mut data = identity(w);
        data["reference"] = json!(first.point);
        data["point_mm"] = json!(first.paper_mm);
        data["view"] = json!(first.view);
        data["paper_units"] = json!("mm");
        out.push(SemanticElement {
            id: "drawing.pick-marker:first".into(),
            role: "drawing-pick-marker".into(),
            label: "First dimension reference".into(),
            value: None,
            state: "selected".into(),
            commands: vec![],
            relationships: vec![],
            data,
        });
    }
    out.extend(selected_commands(w, state));
    for element in &mut out {
        if let Some(command) = element.commands.first() {
            let mut arguments = element.data.clone();
            if let Some(map) = arguments.as_object_mut() {
                map.remove("actions");
            }
            let enabled = element.data["enabled"]
                .as_bool()
                .unwrap_or(element.state != "hidden");
            element.data["actions"] = json!([{"command":command,"label":element.label,"enabled":enabled,"arguments":arguments}]);
        }
    }
    out
}
fn verify(w: &Workspace, data: &Value) -> Result<()> {
    let now = identity(w);
    for key in ["document", "identity", "sheet", "page", "editor"] {
        if data[key] != now[key] {
            return Err(invalid(
                "This drawing control changed; use its current value",
            ));
        }
    }
    Ok(())
}
pub fn current_value(w: &Workspace, element: &SemanticElement) -> Option<Value> {
    verify(w, &element.data).ok()?;
    semantics(w)
        .into_iter()
        .find(|e| e.id == element.id)
        .map(|e| e.data["json"].clone())
}
pub fn field_is_invalid(w: &Workspace, e: &SemanticElement) -> bool {
    e.data["setter"] == "drawing-interaction" && w.drawing_interaction.errors.contains_key(&e.id)
}
pub fn field_error(w: &mut Workspace, e: &SemanticElement, error: Option<String>) {
    if e.data["setter"] != "drawing-interaction" {
        return;
    }
    if let Some(error) = error {
        w.drawing_interaction.errors.insert(e.id.clone(), error);
    } else {
        w.drawing_interaction.errors.remove(&e.id);
    }
}
pub fn set_value(w: &mut Workspace, e: &SemanticElement, value: Value) -> Result<()> {
    verify(w, &e.data)?;
    let fresh = semantics(w)
        .into_iter()
        .find(|element| element.id == e.id)
        .ok_or_else(|| invalid("This paper control is no longer available"))?;
    if fresh.data["field"] != e.data["field"] {
        return Err(invalid("This paper field changed; use its current control"));
    }
    let descriptor = fresh.value_descriptor();
    if !descriptor.editable {
        return Err(invalid("This paper field is currently read-only"));
    }
    if !descriptor.choices.is_empty()
        && !descriptor
            .choices
            .iter()
            .any(|choice| choice.get("value").unwrap_or(choice) == &value)
    {
        return Err(invalid("Choose one of the current paper control options"));
    }
    if active(w) {
        return Err(invalid("Finish or cancel the paper gesture first"));
    }
    let result = (|| -> Result<()> {
        match e.data["field"].as_str().unwrap_or("") {
            "snap" => {
                w.drawing_interaction.snapping = value
                    .as_bool()
                    .ok_or_else(|| invalid("Expected snap toggle"))?
            }
            "grid" => {
                w.drawing_interaction.grid = value
                    .as_bool()
                    .ok_or_else(|| invalid("Expected grid toggle"))?
            }
            "grid-mm" => {
                let n = if let Some(n) = value.as_f64() {
                    n
                } else {
                    cad_core::expression::value(
                        w.session.visible(),
                        &w.session.visible().root,
                        &Expr::from(
                            value
                                .as_str()
                                .ok_or_else(|| invalid("Enter a paper length"))?,
                        ),
                        Dimension::Length,
                    )?
                };
                if !n.is_finite() || !(0.01..=1000.).contains(&n) {
                    return Err(invalid("Grid spacing must be 0.01–1000 mm"));
                }
                w.drawing_interaction.grid_mm = n;
            }
            "dimension-mode" => {
                w.drawing_interaction.dimension_mode = serde_json::from_value(value)?;
                if matches!(
                    w.drawing_interaction.dimension_mode,
                    DimensionMode::Diameter | DimensionMode::Radius
                ) || !matches!(w.drawing_interaction.tool, Tool::Dimension { .. })
                {
                    w.drawing_interaction.tool = Tool::Dimension { first: None };
                }
                w.drawing_interaction.dimension_hint = None;
            }
            "page" => {
                let source = sheet(w).ok_or_else(|| invalid("Select a drawing sheet"))?;
                let page = value
                    .as_u64()
                    .filter(|p| *p < (cad_core::drawing_tables::page_count(&source) as u64))
                    .ok_or_else(|| invalid("Choose an existing continuation page"))?
                    as u32;
                w.fields.insert("drawing-page".into(), page.to_string());
                w.drawing_interaction.selection = None;
                w.drawing_interaction.tool = Tool::Select;
            }
            _ => return Err(invalid("Unknown paper interaction field")),
        }
        Ok(())
    })();
    field_error(w, e, result.as_ref().err().map(ToString::to_string));
    result
}
fn execute(w: &mut Workspace, state: &mut State, cmd: &str, data: Option<&Value>) -> Result<()> {
    if let Some(data) = data {
        verify(w, data)?;
    }
    if state.capture.is_some() {
        return Err(invalid("Finish or cancel the drawing gesture first"));
    }
    if w.settings.is_some() {
        return Err(invalid("Close Settings before changing the drawing"));
    }
    match cmd {
        "drawing.interact-select" => state.tool = Tool::Select,
        "drawing.interact-dimension" => {
            if page(w) != 0 {
                return Err(invalid("Model dimensions belong on the first paper page"));
            }
            state.tool = Tool::Dimension { first: None };
        }
        "drawing.interact-fit" => {
            if let Some(source) = sheet(w) {
                state.views.remove(&view_key(&source, page(w)));
            }
        }
        "drawing.interact-select-item" => {
            let target: Target = serde_json::from_value(
                data.ok_or_else(|| invalid("Choose a paper item"))?["target"].clone(),
            )?;
            let source = sheet(w).ok_or_else(|| invalid("Select a sheet"))?;
            refresh(w, state, &source, page(w))?;
            if state
                .cache
                .as_ref()
                .is_none_or(|cache| !cache.scene.targets.iter().any(|item| item.target == target))
            {
                return Err(invalid("This paper item is no longer on the current page"));
            }
            select(w, state, &source, page(w), target);
        }
        "drawing.interact-reattach" => {
            let data = data.ok_or_else(|| invalid("Choose an attachment handle"))?;
            let target: Target = serde_json::from_value(data["target"].clone())?;
            let slot: AnchorSlot = serde_json::from_value(data["slot"].clone())?;
            let source = sheet(w).ok_or_else(|| invalid("Select a sheet"))?;
            refresh(w, state, &source, page(w))?;
            if state.cache.as_ref().is_none_or(|cache| {
                !cache.scene.targets.iter().any(|item| {
                    item.target == target
                        && item
                            .handles
                            .iter()
                            .any(|handle| handle.kind == HandleKind::Anchor { slot: slot.clone() })
                })
            }) {
                return Err(invalid("This annotation has no requested point attachment"));
            }
            state.tool = Tool::Reattach { target, slot };
        }
        "drawing.interact-reattach-curve" => {
            let target = selected(state)
                .cloned()
                .ok_or_else(|| invalid("Select a circular dimension or center mark"))?;
            let source = sheet(w).ok_or_else(|| invalid("Select a sheet"))?;
            if !supports_curve(&source, &target) {
                return Err(invalid(
                    "This item does not use a circular carrier attachment",
                ));
            }
            state.tool = Tool::ReattachCurve { target };
        }
        "drawing.interact-leader-bend" => {
            let target = selected(state)
                .cloned()
                .ok_or_else(|| invalid("Select a leader annotation"))?;
            let source = sheet(w).ok_or_else(|| invalid("Select a sheet"))?;
            if !supports_leader(&source, &target) {
                return Err(invalid("This symbol does not support elbow leaders"));
            }
            let Target::Annotation { id } = target else {
                return Err(invalid("Select an annotation"));
            };
            let annotation = source
                .entities
                .iter()
                .find(|item| item.id == id)
                .ok_or_else(|| invalid("The annotation disappeared"))?;
            let offset_mm = [annotation.offset_mm[0] * 0.5, annotation.offset_mm[1] * 0.5];
            apply(w, state, &Action::AddLeaderBend { id, offset_mm })?;
        }
        _ => return Err(invalid("Unknown paper command")),
    }
    Ok(())
}
pub fn action(w: &mut Workspace, cmd: &str, data: Option<&Value>) -> bool {
    if !cmd.starts_with("drawing.interact-") {
        return false;
    }
    let mut state = std::mem::take(&mut w.drawing_interaction);
    let result = execute(w, &mut state, cmd, data);
    w.drawing_interaction = state;
    if let Err(error) = result {
        w.status = error.to_string();
    }
    true
}
/// The docked controls use the same semantic values and command entry point as
/// user-authored layouts and the paper context menu.
pub fn show_controls(ui: &mut egui::Ui, w: &mut Workspace) {
    let elements = semantics(w);
    let properties = controls::properties(
        "drawing.interaction-controls",
        "drawing-interaction-controls",
        &["compact", "drawing-controls"],
        "ready",
    );
    if properties
        .get("display")
        .is_some_and(|value| value == "none")
    {
        return;
    }
    ui.scope(|ui| {
        controls::apply_properties(ui, &properties);
        controls::frame_from_properties(ui, &properties).show(ui, |ui| {
            controls::label(
                ui,
                "drawing.interaction-controls:title",
                "drawing-group",
                &["compact"],
                "Paper tools",
                "ready",
            );
            ui.horizontal_wrapped(|ui| {
                for element in elements.iter().filter(|element| element.role == "command") {
                    let enabled = element.data["enabled"].as_bool().unwrap_or(false);
                    if let Some(command) = element.commands.first()
                        && controls::button(
                            ui,
                            &element.id,
                            "drawing-tool",
                            &["compact"],
                            &element.label,
                            enabled,
                            false,
                        )
                        .clicked()
                    {
                        action(w, command, Some(&element.data));
                    }
                }
            });
            for element in elements
                .iter()
                .filter(|element| element.role == "drawing-interaction-field")
            {
                crate::ui::semantic_field(ui, w, element);
            }
        });
    });
}
fn handle_class(kind: &HandleKind) -> &'static str {
    match kind {
        HandleKind::Move => "move",
        HandleKind::Resize { .. } => "resize",
        HandleKind::DimensionOffset => "dimension-offset",
        HandleKind::Anchor { .. } => "anchor",
        HandleKind::LeaderBend { .. } => "leader-elbow",
    }
}
fn dimension_label(action: &Action) -> &'static str {
    let Action::CreateDimension { kind, .. } = action else {
        return "Drawing";
    };
    dimension_kind_label(kind)
}
fn dimension_kind_label(kind: &DimensionKind) -> &'static str {
    match kind {
        DimensionKind::Aligned => "Aligned",
        DimensionKind::Horizontal => "Horizontal",
        DimensionKind::Vertical => "Vertical",
        DimensionKind::Perpendicular => "Perpendicular",
        DimensionKind::Diameter => "Diameter",
        DimensionKind::Radius => "Radius",
        DimensionKind::Angular => "Angular",
        DimensionKind::HoleCallout => "Hole callout",
    }
}
fn dimension_mode_label(mode: &DimensionMode) -> &'static str {
    match mode {
        DimensionMode::Auto => "Auto",
        DimensionMode::Aligned => "Aligned",
        DimensionMode::Horizontal => "Horizontal",
        DimensionMode::Vertical => "Vertical",
        DimensionMode::Perpendicular => "Perpendicular",
        DimensionMode::Diameter => "Diameter",
        DimensionMode::Radius => "Radius",
    }
}
fn click_reference(
    w: &mut Workspace,
    state: &mut State,
    source: &Drawing,
    scene: &Scene,
    point: [f64; 2],
    tolerance: f64,
) -> Result<()> {
    let view = match &state.tool {
        Tool::Dimension { first: Some(first) } => Some(first.view.as_str()),
        Tool::Reattach { target, .. } | Tool::ReattachCurve { target } => scene
            .targets
            .iter()
            .find(|h| &h.target == target)
            .and_then(|h| h.view.as_deref()),
        _ => None,
    };
    let hit = interaction::nearest_reference(scene, point, tolerance, view)
        .ok_or_else(|| invalid("Select current model geometry on this drawing view"))?;
    match state.tool.clone() {
        Tool::Reattach { target, slot } => {
            apply(
                w,
                state,
                &Action::ReattachPoint {
                    target,
                    slot,
                    reference: hit.point,
                },
            )?;
            state.tool = Tool::Select;
        }
        Tool::ReattachCurve { target } => {
            let reference = hit
                .curve
                .ok_or_else(|| invalid("Select a supported native curve"))?;
            apply(w, state, &Action::ReattachCurve { target, reference })?;
            state.tool = Tool::Select;
        }
        Tool::Dimension { first } => {
            let cache = state
                .cache
                .as_ref()
                .ok_or_else(|| invalid("Paper geometry is unavailable"))?;
            let circular = hit.curve.as_ref().is_some_and(|curve| {
                cad_core::drawing::circular(w.session.visible(), &cache.evaluation, source, curve)
                    .is_ok()
            });
            let inferred = first.as_ref().map(|first| {
                interaction::infer_dimension(
                    w.session.visible(),
                    &cache.evaluation,
                    source,
                    first,
                    &hit,
                )
            });
            let kind = match state.dimension_mode {
                DimensionMode::Auto
                    if first.is_none()
                        && circular
                        && !matches!(
                            hit.point,
                            cad_core::drawing::PointReference::CurveCenter { .. }
                        ) =>
                {
                    DimensionKind::Diameter
                }
                DimensionMode::Diameter => DimensionKind::Diameter,
                DimensionMode::Radius => DimensionKind::Radius,
                DimensionMode::Horizontal => DimensionKind::Horizontal,
                DimensionMode::Vertical => DimensionKind::Vertical,
                DimensionMode::Perpendicular => DimensionKind::Perpendicular,
                DimensionMode::Auto => inferred
                    .as_ref()
                    .map_or(DimensionKind::Aligned, |inferred| inferred.kind.clone()),
                DimensionMode::Aligned => DimensionKind::Aligned,
            };
            let curved = matches!(kind, DimensionKind::Diameter | DimensionKind::Radius);
            if curved && !circular {
                return Err(invalid(
                    "Choose an exact circular edge for a radius or diameter",
                ));
            }
            if first.is_none() && !curved {
                state.dimension_hint = None;
                state.tool = Tool::Dimension {
                    first: Some(Box::new(hit)),
                };
                w.status = "Select the second dimension reference on this view".into();
                return Ok(());
            }
            let first = first.unwrap_or_else(|| Box::new(hit.clone()));
            let edge_separation = kind == DimensionKind::Perpendicular
                || (state.dimension_mode == DimensionMode::Auto
                    && inferred
                        .as_ref()
                        .is_some_and(|inferred| inferred.edge_separation));
            let make = |hit: &ReferenceHit| Anchor {
                point: hit.model_point,
                reference: Some(hit.point.clone()),
            };
            let action = Action::CreateDimension {
                view: hit.view.clone(),
                kind,
                first: Box::new(make(&first)),
                second: Box::new(make(&hit)),
                curve: if edge_separation {
                    first.curve.clone()
                } else {
                    curved.then_some(hit.curve.clone()).flatten()
                }
                .map(Box::new),
                second_curve: edge_separation.then_some(hit.curve).flatten().map(Box::new),
                offset_mm: 8.,
            };
            apply(w, state, &action)?;
            state.tool = Tool::Dimension { first: None };
            let hint = format!(
                "{}{} dimension added",
                dimension_label(&action),
                if edge_separation {
                    " edge separation"
                } else {
                    ""
                }
            );
            state.dimension_hint = Some(hint.clone());
            w.status = format!("{hint}; select geometry for another dimension");
        }
        Tool::Select => {}
    }
    Ok(())
}
fn draw_primitive(
    painter: &egui::Painter,
    primitive: &Primitive,
    paper: Rect,
    scale: f32,
    stroke: Stroke,
) {
    match primitive {
        Primitive::Polyline { points, dashed } => {
            for pair in points.windows(2) {
                let a = at(paper, scale, pair[0]);
                let b = at(paper, scale, pair[1]);
                if *dashed {
                    let length = a.distance(b);
                    if length <= 0.001 {
                        continue;
                    }
                    let count = (length / (5. * scale).max(2.)).ceil().min(4096.) as usize;
                    for i in 0..count {
                        let s = i as f32 / count as f32;
                        let e = ((i as f32 + 0.65) / count as f32).min(1.);
                        painter.line_segment([a.lerp(b, s), a.lerp(b, e)], stroke);
                    }
                } else {
                    painter.line_segment([a, b], stroke);
                }
            }
        }
        Primitive::Text {
            at: point,
            size,
            text,
        } => {
            painter.text(
                at(paper, scale, *point),
                egui::Align2::LEFT_BOTTOM,
                text,
                egui::FontId::proportional((*size as f32 * scale).max(1.)),
                stroke.color,
            );
        }
        Primitive::Spline { curve } => {
            if let Ok(points) = cad_core::drawing::display_curve(curve) {
                for pair in points.windows(2) {
                    painter.line_segment(
                        [at(paper, scale, pair[0]), at(paper, scale, pair[1])],
                        stroke,
                    );
                }
            }
        }
        Primitive::Circle { center, radius } => {
            painter.circle_stroke(at(paper, scale, *center), *radius as f32 * scale, stroke);
        }
    }
}
fn draw_status(
    ui: &mut egui::Ui,
    state: &mut State,
    viewport: Rect,
    viewport_id: &str,
    message: &str,
    status: &str,
    centered: bool,
) {
    state
        .statuses
        .insert(viewport_id.into(), (message.into(), status.into()));
    let id = format!("drawing.paper-status:{viewport_id}");
    let size = egui::vec2(
        (viewport.width() - 20.).max(0.),
        if centered { 52. } else { 24. },
    );
    let default = if centered {
        Rect::from_center_size(viewport.center(), size)
    } else {
        Rect::from_min_size(viewport.left_bottom() + egui::vec2(10., -size.y - 6.), size)
    };
    if let Some(rect) =
        controls::overlay_rect_state(&id, "drawing-paper-status", status, viewport, default)
    {
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect));
        let properties = controls::properties(
            &id,
            "drawing-paper-status",
            &["viewport-overlay", "paper-status"],
            status,
        );
        controls::apply_properties(&mut child, &properties);
        controls::label(
            &mut child,
            &id,
            "drawing-paper-status",
            &["viewport-overlay", "paper-status"],
            message,
            status,
        );
    }
}
pub fn frame(
    w: &mut Workspace,
    ui: &mut egui::Ui,
    viewport: Rect,
    response: &egui::Response,
    viewport_id: &str,
) -> bool {
    if w.dialog.as_deref() != Some("drawings") {
        return false;
    }
    let Some(source) = sheet(w) else { return false };
    let mut state = std::mem::take(&mut w.drawing_interaction);
    if state.grid_mm == 0. {
        state.grid_mm = 1.;
        state.snapping = true;
    }
    let refresh_result = refresh(w, &mut state, &source, page(w));
    let props = controls::properties(
        "drawing.paper",
        "drawing-sheet",
        &["paper", "preview"],
        if refresh_result.is_err() {
            "pending"
        } else {
            "ready"
        },
    );
    let painter = ui.painter_at(viewport);
    painter.rect_filled(viewport, 0., ui.visuals().faint_bg_color);
    if let Err(error) = &refresh_result {
        if state
            .capture
            .as_ref()
            .is_some_and(|capture| capture.viewport_id == viewport_id)
            && let Some(capture) = state.capture.take()
        {
            restore_capture(
                w,
                &mut state,
                capture,
                &format!("Drawing gesture restored: {error}"),
            );
        }
        draw_status(
            ui,
            &mut state,
            viewport,
            viewport_id,
            &error.to_string(),
            "pending",
            true,
        );
        w.drawing_interaction = state;
        return true;
    }
    let Some(cache) = state.cache.as_ref() else {
        draw_status(
            ui,
            &mut state,
            viewport,
            viewport_id,
            "Preparing drawing geometry…",
            "pending",
            true,
        );
        w.drawing_interaction = state;
        return true;
    };
    let rendered = cache.paper.clone();
    let scene = cache.scene.clone();
    let native_evaluation = cache.evaluation.clone();
    let key = view_key(&source, page(w));
    let view = state.views.entry(key.clone()).or_default();
    let (mut paper, mut scale) = fitted(view, viewport, rendered.paper_mm);
    let pointer = ui.input(|i| i.pointer.interact_pos());
    let inside = pointer.is_some_and(|p| viewport.contains(p));
    let overlay_rect = controls::overlay_rect_state(
        "drawing.toolbar",
        "drawing-toolbar",
        "ready",
        viewport,
        Rect::from_min_size(
            viewport.min + egui::vec2(8., 8.),
            egui::vec2((viewport.width() - 16.).max(0.), 34.),
        ),
    );
    if inside
        && response.hovered()
        && !pointer.is_some_and(|p| overlay_rect.is_some_and(|rect| rect.contains(p)))
        && state.capture.is_none()
        && !ui.ctx().egui_wants_keyboard_input()
    {
        let zoom = ui.input(|i| i.zoom_delta());
        let scroll = ui.input(|i| i.smooth_scroll_delta);
        if (zoom - 1.).abs() > 0.0001 {
            let pivot = pointer.unwrap();
            let old = view.zoom.max(1e-6);
            let old = if view.zoom == 0. { 1. } else { old };
            let next = (old * zoom).clamp(0.2, 12.);
            let delta = pivot - (viewport.center() + view.pan);
            view.pan += delta * (1. - next / old);
            view.zoom = next;
        } else if scroll.length_sq() > 0. {
            view.pan += scroll;
        }
        (paper, scale) = fitted(view, viewport, rendered.paper_mm);
    }
    let color = props
        .get("color")
        .and_then(|v| crate::ui::color(v))
        .unwrap_or(Color32::from_rgb(25, 27, 28));
    let accent = props
        .get("selection-color")
        .and_then(|v| crate::ui::color(v))
        .unwrap_or(Color32::from_rgb(255, 90, 0));
    let stroke = Stroke::new(
        props
            .get("stroke-width")
            .and_then(|v| v.trim_end_matches("px").parse::<f32>().ok())
            .unwrap_or((0.16 * scale).max(0.7)),
        color,
    );
    painter.rect_filled(
        paper,
        0.,
        props
            .get("background")
            .and_then(|v| crate::ui::color(v))
            .unwrap_or(Color32::from_rgb(250, 249, 243)),
    );
    if state.grid {
        let step = state.grid_mm.max(0.01);
        let work = rendered.paper_mm[0] / step + rendered.paper_mm[1] / step;
        if work < 4096. {
            let grid = Stroke::new(0.5, color.gamma_multiply(0.12));
            for i in 0..=(rendered.paper_mm[0] / step) as usize {
                let x = i as f64 * step;
                painter.line_segment(
                    [
                        at(paper, scale, [x, 0.]),
                        at(paper, scale, [x, rendered.paper_mm[1]]),
                    ],
                    grid,
                );
            }
            for i in 0..=(rendered.paper_mm[1] / step) as usize {
                let y = i as f64 * step;
                painter.line_segment(
                    [
                        at(paper, scale, [0., y]),
                        at(paper, scale, [rendered.paper_mm[0], y]),
                    ],
                    grid,
                );
            }
        }
    }
    for primitive in &rendered.primitives {
        draw_primitive(&painter, primitive, paper, scale, stroke);
    }
    let mut toolbar_rect = Rect::NOTHING;
    if let Some(rect) = overlay_rect {
        toolbar_rect = rect;
        let mut toolbar = ui.new_child(egui::UiBuilder::new().max_rect(rect));
        controls::apply_properties(
            &mut toolbar,
            &controls::properties(
                "drawing.toolbar",
                "drawing-toolbar",
                &["viewport-overlay"],
                "ready",
            ),
        );
        toolbar.horizontal_wrapped(|ui| {
            for (command, label) in [
                ("drawing.interact-select", "Select"),
                ("drawing.interact-dimension", "Dimension"),
                ("drawing.interact-fit", "Fit"),
            ] {
                if controls::button(
                    ui,
                    command,
                    "drawing-tool",
                    &["compact"],
                    label,
                    state.capture.is_none(),
                    false,
                )
                .clicked()
                {
                    match command {
                        "drawing.interact-select" => state.tool = Tool::Select,
                        "drawing.interact-dimension" => {
                            state.tool = Tool::Dimension { first: None }
                        }
                        _ => {
                            state.views.remove(&key);
                        }
                    }
                }
            }
            let max = cad_core::drawing_tables::page_count(&source);
            if max > 1 {
                let mut current = page(w);
                ui.add_enabled_ui(state.capture.is_none(), |ui| {
                    egui::ComboBox::from_id_salt("drawing.page")
                        .selected_text(format!("Page {} / {}", current + 1, max))
                        .show_ui(ui, |ui| {
                            for p in 0..max {
                                if ui
                                    .selectable_value(&mut current, p, format!("Page {}", p + 1))
                                    .changed()
                                {
                                    w.fields.insert("drawing-page".into(), p.to_string());
                                    state.selection = None;
                                    state.tool = Tool::Select;
                                }
                            }
                        });
                });
            }
        });
    }
    if page(w) != scene.page {
        state.cache = None;
        w.drawing_interaction = state;
        return true;
    }
    let over = inside && response.hovered() && !pointer.is_some_and(|p| toolbar_rect.contains(p));
    let handle_size = props
        .get("handle-size")
        .and_then(|v| v.trim_end_matches("px").parse::<f32>().ok())
        .unwrap_or(7.)
        .clamp(3., 24.);
    if state.capture.is_none() {
        state.hover = if over {
            pointer.and_then(|pointer| {
                interaction::hit(
                    &scene,
                    paper_point(paper, scale, pointer),
                    6. / scale as f64,
                )
                .map(|target| target.target.clone())
            })
        } else {
            None
        };
    }
    for target in &scene.targets {
        let is_selected = selected(&state) == Some(&target.target);
        let is_hovered = state.hover.as_ref() == Some(&target.target);
        if is_selected || is_hovered || target.diagnostic.is_some() {
            let mut classes = vec!["paper-item"];
            if target.diagnostic.is_some() {
                classes.push("diagnostic");
            }
            let status = if target.diagnostic.is_some() {
                "invalid"
            } else if is_selected {
                "selected"
            } else {
                "hovered"
            };
            let properties = controls::properties(
                &format!(
                    "{}:paper:{}:{}",
                    source.id,
                    scene.page,
                    target.target.identity()
                ),
                "drawing-paper-item",
                &classes,
                status,
            );
            let color = properties
                .get("color")
                .and_then(|value| crate::ui::color(value))
                .unwrap_or(if target.diagnostic.is_some() {
                    ui.visuals().error_fg_color
                } else {
                    accent.gamma_multiply(if is_selected { 1. } else { 0.7 })
                });
            let width = controls::number(
                &properties,
                "stroke-width",
                if is_selected {
                    stroke.width + 0.5
                } else {
                    stroke.width
                },
            );
            if let Some(primitives) = rendered
                .primitives
                .get(target.primitive_start..target.primitive_start + target.primitive_count)
            {
                for primitive in primitives {
                    draw_primitive(&painter, primitive, paper, scale, Stroke::new(width, color));
                }
            }
        }
    }
    let mut hovered_handle = None;
    if let Some(target) =
        selected(&state).and_then(|target| scene.targets.iter().find(|h| &h.target == target))
    {
        let bounds = Rect::from_min_max(
            at(
                paper,
                scale,
                [target.bounds_mm[0][0], target.bounds_mm[1][1]],
            ),
            at(
                paper,
                scale,
                [target.bounds_mm[1][0], target.bounds_mm[0][1]],
            ),
        );
        painter.rect_stroke(
            bounds.expand(2.),
            0.,
            Stroke::new(1., accent),
            egui::StrokeKind::Outside,
        );
        for handle in &target.handles {
            let properties = controls::properties(
                &format!("{}:paper:{}:handle:{}", source.id, scene.page, handle.id),
                "drawing-paper-handle",
                &["paper-handle", handle_class(&handle.kind)],
                "selected",
            );
            if properties
                .get("display")
                .is_some_and(|value| value == "none")
            {
                continue;
            }
            let size = controls::number(&properties, "handle-size", handle_size).clamp(3., 24.);
            let color = properties
                .get("background")
                .or_else(|| properties.get("color"))
                .and_then(|value| crate::ui::color(value))
                .unwrap_or(accent);
            let p = at(paper, scale, handle.at_mm);
            let rect = Rect::from_center_size(p, egui::Vec2::splat(size * 1.8));
            painter.circle_filled(p, size * 0.55, color);
            if over && pointer.is_some_and(|point| rect.contains(point)) {
                hovered_handle = Some((target.clone(), handle.clone()));
            }
        }
    }
    if state
        .capture
        .as_ref()
        .is_some_and(|capture| capture.viewport_id == viewport_id)
    {
        let context_changed = state.capture.as_ref().is_some_and(|capture| {
            capture.viewport != viewport || capture.paper_rect != paper || capture.scale != scale
        });
        if context_changed {
            if let Some(capture) = state.capture.take() {
                restore_capture(
                    w,
                    &mut state,
                    capture,
                    "Paper layout changed; gesture restored",
                );
            }
        } else if let Some(pointer) = pointer {
            if let Err(error) = update_capture(w, &mut state, pointer) {
                w.status = error.to_string();
            }
            let click_move = state.capture.as_ref().is_some_and(|c| c.click_move);
            if (click_move && response.clicked()) || (!click_move && response.drag_stopped()) {
                end_capture(w, &mut state);
            }
        }
        if !ui.input(|i| i.focused)
            && let Some(capture) = state.capture.take()
        {
            restore_capture(
                w,
                &mut state,
                capture,
                "Drawing focus changed; gesture restored",
            );
        }
    } else if state.capture.is_none()
        && over
        && !ui.ctx().egui_wants_keyboard_input()
        && let Some(pointer) = pointer
    {
        let point = paper_point(paper, scale, pointer);
        let hit = interaction::hit(&scene, point, 6. / scale as f64).cloned();
        state.hover = hit.as_ref().map(|h| h.target.clone());
        if response.drag_started() && matches!(state.tool, Tool::Select) {
            if let Some((target, handle)) = hovered_handle.clone().or_else(|| {
                hit.as_ref().and_then(|target| {
                    target
                        .handles
                        .iter()
                        .find(|handle| {
                            matches!(handle.kind, HandleKind::Move | HandleKind::DimensionOffset)
                        })
                        .map(|handle| {
                            let mut handle = handle.clone();
                            handle.at_mm = point;
                            (target.clone(), handle)
                        })
                })
            }) {
                select(w, &mut state, &source, page(w), target.target.clone());
                if let Err(error) = begin_capture(
                    w,
                    &mut state,
                    CaptureRequest {
                        source: &source,
                        page: page(w),
                        target: &target,
                        handle,
                        pointer,
                        viewport_id,
                        viewport,
                        paper,
                        scale,
                        click_move: false,
                    },
                ) {
                    w.status = error.to_string();
                }
            }
        } else if response.clicked() {
            if !matches!(state.tool, Tool::Select) {
                if let Err(error) =
                    click_reference(w, &mut state, &source, &scene, point, 8. / scale as f64)
                {
                    w.status = error.to_string();
                }
            } else if let Some((target, handle)) = hovered_handle {
                if let Err(error) = begin_capture(
                    w,
                    &mut state,
                    CaptureRequest {
                        source: &source,
                        page: page(w),
                        target: &target,
                        handle,
                        pointer,
                        viewport_id,
                        viewport,
                        paper,
                        scale,
                        click_move: true,
                    },
                ) {
                    w.status = error.to_string();
                }
            } else if let Some(hit) = hit {
                select(w, &mut state, &source, page(w), hit.target);
            } else {
                state.selection = None;
            }
        }
    }
    if response.secondary_clicked() && state.capture.is_none() {
        state.context_target = if over {
            pointer.and_then(|pointer| {
                interaction::hit(
                    &scene,
                    paper_point(paper, scale, pointer),
                    6. / scale as f64,
                )
                .map(|target| target.target.clone())
            })
        } else {
            None
        };
        if let Some(target) = state.context_target.clone() {
            select(w, &mut state, &source, page(w), target);
        }
    }
    if state.context_target.is_some() && state.capture.is_none() {
        let mut commands = selected_commands(w, &state);
        commands.insert(0, command(w, "drawing.interact-select", "Select", true));
        commands.push(command(
            w,
            "drawing.interact-dimension",
            "Smart Dimension",
            page(w) == 0,
        ));
        response.context_menu(|ui| {
            controls::apply_properties(
                ui,
                &controls::properties(
                    "drawing.context-menu",
                    "selection-menu",
                    &["drawing-context"],
                    "ready",
                ),
            );
            for element in &commands {
                if let Some(command) = element.commands.first()
                    && controls::button(
                        ui,
                        &element.id,
                        "drawing-tool",
                        &["context-action"],
                        &element.label,
                        element.data["enabled"].as_bool().unwrap_or(false),
                        false,
                    )
                    .clicked()
                {
                    if let Err(error) = execute(w, &mut state, command, Some(&element.data)) {
                        w.status = error.to_string();
                    }
                    state.context_target = None;
                    ui.close();
                }
            }
        });
    }
    for (index, guide) in state.guides.iter().enumerate() {
        let properties = controls::properties(
            &format!("drawing.snap-guide:{index}"),
            "drawing-snap-guide",
            &["paper-guide"],
            "active",
        );
        if properties
            .get("display")
            .is_some_and(|value| value == "none")
        {
            continue;
        }
        let color = properties
            .get("color")
            .and_then(|value| crate::ui::color(value))
            .unwrap_or(accent.gamma_multiply(0.7));
        painter.line_segment(
            [at(paper, scale, guide[0]), at(paper, scale, guide[1])],
            Stroke::new(controls::number(&properties, "stroke-width", 1.), color),
        );
    }
    if let Tool::Dimension { first: Some(first) } = &state.tool {
        let properties = controls::properties(
            "drawing.pick-marker:first",
            "drawing-pick-marker",
            &["paper-reference"],
            "selected",
        );
        if !properties
            .get("display")
            .is_some_and(|value| value == "none")
        {
            let color = properties
                .get("color")
                .and_then(|value| crate::ui::color(value))
                .unwrap_or(accent);
            painter.circle_stroke(
                at(paper, scale, first.paper_mm),
                controls::number(&properties, "handle-size", 5.).max(0.),
                Stroke::new(
                    controls::number(&properties, "stroke-width", 2.).max(0.),
                    color,
                ),
            );
        }
    }
    let inferred_hint = if state.dimension_mode == DimensionMode::Auto
        && let Tool::Dimension { first: Some(first) } = &state.tool
        && let Some(pointer) = pointer.filter(|_| over)
        && let Some(second) = interaction::nearest_reference(
            &scene,
            paper_point(paper, scale, pointer),
            8. / scale as f64,
            Some(&first.view),
        ) {
        let inferred = interaction::infer_dimension(
            w.session.visible(),
            &native_evaluation,
            &source,
            first,
            &second,
        );
        Some(format!(
            "Auto: {}{} · click to add · choose Aligned for point-to-point distance",
            dimension_kind_label(&inferred.kind),
            if inferred.edge_separation {
                " edge separation"
            } else {
                ""
            }
        ))
    } else {
        None
    };
    if matches!(state.tool, Tool::Dimension { first: Some(_) }) {
        state.dimension_hint = inferred_hint.clone();
    }
    let explicit_hint = format!(
        "{}: pick the second reference on this view",
        dimension_mode_label(&state.dimension_mode)
    );
    let status = if let Some(capture) = &state.capture {
        if capture.viewport_id != viewport_id {
            "Drawing gesture active in another viewport"
        } else {
            capture
                .error
                .as_deref()
                .unwrap_or("Drag or click to finish · Esc restores the gesture")
        }
    } else {
        match &state.tool {
            Tool::Select => "Select or drag drawing items · pinch to zoom · two fingers to pan",
            Tool::Dimension { first: None } => {
                if matches!(
                    state.dimension_mode,
                    DimensionMode::Diameter | DimensionMode::Radius
                ) {
                    "Pick an exact circular edge"
                } else {
                    "Pick a circular edge or the first dimension reference"
                }
            }
            Tool::Dimension { first: Some(_) } => {
                inferred_hint.as_deref().unwrap_or(&explicit_hint)
            }
            Tool::Reattach { .. } | Tool::ReattachCurve { .. } => {
                "Pick replacement model geometry · Esc cancels reattachment"
            }
        }
    };
    let message = status.to_owned();
    let status = if state
        .capture
        .as_ref()
        .is_some_and(|capture| capture.error.is_some())
    {
        "error"
    } else {
        "ready"
    };
    draw_status(
        ui,
        &mut state,
        viewport,
        viewport_id,
        &message,
        status,
        false,
    );
    w.drawing_interaction = state;
    true
}
