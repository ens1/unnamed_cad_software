//! Model-owned finished requirements in the property manager and on the selected occurrence.
use crate::{Workspace, presentation_controls as controls, viewport};
use bevy_egui::egui;
use cad_core::{
    CadError, Part, PlacedPart, Result, TopologyRef,
    commands::Command,
    model_dimensions::{self as mbd, Characteristic, Edit, Report},
    presentation::SemanticElement,
    protocol::{Evaluation, MeshData},
    selection_actions::SelectionTarget,
};
use serde_json::{Value, json};

const PANEL: &str = "model-dimensions";
const DIMENSION: &str = "mbd-dimension";
const PATH: &str = "mbd-occurrence";
const OWNER: &str = "mbd-part";
const EDITOR: &str = "mbd-editor";
const UNEQUAL: &str = "mbd-unequal";

fn invalid(message: &str) -> CadError {
    CadError::Invalid(message.into())
}
pub(crate) fn active(w: &Workspace) -> bool {
    w.dialog.as_deref() == Some(PANEL) && w.fields.contains_key(EDITOR)
}
fn owner(w: &Workspace) -> Option<&str> {
    if active(w) {
        return w.fields.get(OWNER).map(String::as_str);
    }
    w.selected
        .as_ref()
        .map(|s| s.definition.as_str())
        .or(w.view.active_part.as_deref())
}
pub(crate) fn unavailable(w: &Workspace) -> Option<&'static str> {
    if w.view.history_cursor.is_some() || w.view.original && w.session.draft.is_some() {
        return Some("Return to the current part to inspect finished dimensions");
    }
    if w.settings.is_some()
        || w.sketch.is_some()
        || !active(w) && (w.tool_checkpoint.is_some() || w.session.draft.is_some())
    {
        return Some("Finish the current operation first");
    }
    if owner(w).is_none_or(|id| !w.session.visible().parts.contains_key(id)) {
        return Some("Select a part to inspect its dimensions");
    }
    None
}

/// Permit repair on current valid geometry even if a sheet has lost an attachment.
/// Last-valid frames are useful to navigate, but cannot establish new model requirements.
pub(crate) fn current_geometry(w: &Workspace) -> Option<&Evaluation> {
    if w.view.history_cursor.is_some() {
        return None;
    }
    let eval = match &w.session.draft {
        Some(draft) => draft.evaluation.as_ref()?,
        None => w.session.evaluation.as_ref()?,
    };
    (eval.identity == crate::source_cache::identity(w) && (eval.valid || eval.geometry_valid))
        .then_some(eval)
}

struct Context<'a> {
    part: &'a Part,
    instance: &'a PlacedPart,
    mesh: Option<&'a MeshData>,
    report: &'a Report,
}
fn context(w: &Workspace) -> Option<Context<'_>> {
    let id = owner(w)?;
    let eval = current_geometry(w)?;
    let path: Option<Vec<String>> = active(w)
        .then(|| w.fields.get(PATH))
        .flatten()
        .and_then(|s| serde_json::from_str(s).ok());
    let path = path
        .as_deref()
        .or_else(|| {
            w.selected
                .as_ref()
                .filter(|s| s.definition == id)
                .map(|s| s.path.as_slice())
        })
        .or(w.selected_feature_path.as_deref());
    let mut candidates = viewport::display_instances(w, eval)
        .into_iter()
        .filter(|i| {
            i.definition == id
                && path.map_or_else(
                    || {
                        w.view.active_instance.as_ref().is_none_or(|iid| {
                            i.path.contains(iid) || i.path.is_empty() && iid == id
                        })
                    },
                    |p| i.path == p,
                )
        });
    let instance = candidates.next()?;
    if candidates.next().is_some() {
        return None;
    }
    let key = cad_core::history::mesh_key(instance);
    Some(Context {
        part: w.session.visible().parts.get(id)?,
        instance,
        mesh: eval.meshes.get(key),
        report: eval.model_dimensions.get(key)?,
    })
}
impl Context<'_> {
    fn related<'b>(
        &'b self,
        pick: &viewport::Selection,
    ) -> impl Iterator<Item = &'b Characteristic> {
        let mut faces = vec![];
        if pick.definition == self.part.id && pick.path == self.instance.path {
            faces.push(pick.reference.clone());
            if pick.reference.topology == "edge"
                && let Some(mesh) = self.mesh
                && let Some(edge) = mesh.edges.iter().find(|edge| {
                    edge.body == pick.body
                        && cad_core::selection_sets::same_reference(
                            &edge.reference,
                            &pick.reference,
                        )
                })
            {
                faces.extend(edge.faces.iter().cloned());
            }
        }
        self.report.dimensions.iter().filter(move |dim| {
            dim.supports.iter().any(|support| {
                faces.iter().any(|f| {
                    support.producer == f.producer
                        && support.topology == f.topology
                        && support.role == f.role
                })
            })
        })
    }
    fn eligible_face(&self, w: &Workspace) -> Option<&TopologyRef> {
        let selection = w.selected.as_ref()?;
        if selection.reference.topology != "face"
            || selection.definition != self.part.id
            || selection.path != self.instance.path
        {
            return None;
        }
        // Datum source stores the part-local provenance, so a reused face on two bodies is ambiguous.
        let mut matches = self.mesh?.faces.iter().filter(|face| {
            cad_core::selection_sets::same_reference(&face.reference, &selection.reference)
        });
        let face = matches.next()?;
        (matches.next().is_none() && face.body == selection.body && mbd::datum_eligible(face))
            .then_some(&face.reference)
    }
    fn focused(&self, w: &Workspace) -> Option<&Characteristic> {
        let dimensions = &self.report.dimensions;
        dimensions
            .iter()
            .find(|d| Some(&d.id) == w.fields.get(DIMENSION))
            .or_else(|| {
                w.selected
                    .as_ref()
                    .and_then(|pick| self.related(pick).next())
            })
            .or_else(|| {
                w.view.active_node.as_ref().and_then(|node| {
                    dimensions
                        .iter()
                        .find(|d| d.supports.iter().any(|s| &s.producer.node == node))
                })
            })
            .or_else(|| dimensions.first())
    }
    fn writable(&self, w: &Workspace) -> bool {
        !cad_core::library::is_definition_pinned(w.session.visible(), &self.part.id)
    }
    fn bind(&self, w: &Workspace, mut data: Value) -> Value {
        data["document"] = json!(w.session.visible().id);
        data["part"] = json!(self.part.id);
        data["path"] = json!(self.instance.path);
        data["mesh"] = json!(cad_core::history::mesh_key(self.instance));
        data["editor"] = json!(w.fields.get(EDITOR));
        data
    }
    fn is_current(&self, w: &Workspace, data: &Value) -> bool {
        data["document"] == w.session.visible().id
            && data["part"] == self.part.id
            && data["path"] == json!(self.instance.path)
            && data["mesh"] == cad_core::history::mesh_key(self.instance)
            && data["editor"] == json!(w.fields.get(EDITOR))
    }
}
fn select_dimension(w: &mut Workspace, id: &str) {
    if w.fields.get(DIMENSION).is_none_or(|v| v != id) {
        w.fields.remove(UNEQUAL);
    }
    w.fields.insert(DIMENSION.into(), id.into());
}
pub(crate) fn accepts_pick(w: &mut Workspace, pick: &viewport::Selection) -> bool {
    if !crate::quantity_display::source_fields_valid(w) {
        w.status = "Correct the current tolerance before selecting another face".into();
        return false;
    }
    let path: Option<Vec<String>> = w
        .fields
        .get(PATH)
        .and_then(|s| serde_json::from_str(s).ok());
    if owner(w) != Some(&pick.definition) || path.is_some_and(|p| p != pick.path) {
        w.status = "Select geometry on the part shown in Model Dimensions".into();
        return false;
    }
    true
}
pub(crate) fn picked(w: &mut Workspace, pick: &viewport::Selection) {
    if let Some(id) = context(w).and_then(|ctx| ctx.related(pick).next().map(|d| d.id.clone())) {
        select_dimension(w, &id);
    }
}
fn begin(w: &mut Workspace, id: Option<&str>) -> Result<()> {
    if let Some(reason) = unavailable(w) {
        return Err(invalid(reason));
    }
    if !active(w) {
        let owner = owner(w)
            .ok_or_else(|| invalid("Select a part"))?
            .to_string();
        let path = context(w)
            .map(|ctx| ctx.instance.path.clone())
            .or_else(|| w.selected.as_ref().map(|p| p.path.clone()));
        w.fields.retain(|k, _| !k.starts_with("mbd-"));
        w.start_tool();
        w.fields.insert(EDITOR.into(), cad_core::id());
        w.fields.insert(OWNER.into(), owner);
        if let Some(path) = path {
            w.fields.insert(PATH.into(), serde_json::to_string(&path)?);
        }
        w.open_editor(PANEL);
    }
    if let Some(id) = id {
        select_dimension(w, id);
    }
    Ok(())
}

/// A part sheet can be independent of placement. Prefer the active occurrence
/// when several copies have exactly the sheet's part and configuration.
pub(crate) fn open_from_drawing(
    w: &mut Workspace,
    part: &str,
    configuration: Option<&str>,
    path: &[cad_core::Id],
    dimension: Option<&str>,
) -> Result<()> {
    if w.settings.is_some()
        || w.tool_checkpoint.is_some()
        || w.session.draft.is_some()
        || w.sketch.is_some()
    {
        return Err(invalid(
            "Finish the current edit before opening a drawing requirement in the model",
        ));
    }
    let eval = current_geometry(w).ok_or_else(|| invalid("Wait for the current model geometry"))?;
    let doc = w.session.visible();
    let config = configuration.or_else(|| doc.active_configurations.get(part).map(String::as_str));
    let occurrence = eval
        .instances
        .iter()
        .filter(|i| {
            i.definition == part
                && (path.is_empty() || path == i.path)
                && i.configuration
                    .as_deref()
                    .or_else(|| doc.active_configurations.get(part).map(String::as_str))
                    == config
        })
        .min_by_key(|i| (Some(&i.path) != w.selected_component.as_ref(), &i.path))
        .ok_or_else(|| {
            invalid("Open this part in the drawing's configuration to edit its requirements")
        })?;
    let report = eval
        .model_dimensions
        .get(cad_core::history::mesh_key(occurrence))
        .ok_or_else(|| invalid("This part has no evaluated model dimensions"))?;
    if dimension.is_some_and(|id| !report.dimensions.iter().any(|d| d.id == id)) {
        return Err(invalid(
            "This drawing dimension no longer resolves to a finished size",
        ));
    }
    let path = occurrence.path.clone();
    w.activate_context(Some(part.into()), Some(path.clone()));
    begin(w, dimension)?;
    w.fields.insert(PATH.into(), serde_json::to_string(&path)?);
    Ok(())
}

pub(crate) fn selection_actions(
    w: &Workspace,
    target: &SelectionTarget,
) -> Option<SemanticElement> {
    if unavailable(w).is_some()
        || active(w)
        || !matches!(
            target,
            SelectionTarget::Geometry { .. } | SelectionTarget::Feature { .. }
        )
        || owner(w) != Some(target.definition())
    {
        return None;
    }
    let mut element = SemanticElement {
        id: "selection:model-dimensions".into(),
        role: "selection-action".into(),
        label: "Model Dimensions…".into(),
        value: Some("Inspect finished sizes, tolerances and datums".into()),
        state: "ready".into(),
        commands: vec!["selection.model-dimensions".into()],
        relationships: vec![target.definition().into(), target.feature().into()],
        data: json!({"target":target,"group":"Dimensions"}),
    };
    cad_core::selection_actions::menu_metadata(&mut element);
    Some(element)
}

fn field(
    key: &str,
    label: &str,
    data: Value,
    value: Value,
    choices: Vec<Value>,
    editable: bool,
) -> SemanticElement {
    let mut data = data;
    data["field"] = key.into();
    data["setter"] = PANEL.into();
    data["json"] = value.clone();
    data["choices"] = json!(choices);
    data["editable"] = editable.into();
    data["value_type"] = if key == "datum-label" {
        "text"
    } else if choices.is_empty() {
        "quantity"
    } else {
        "choice"
    }
    .into();
    let id = if key == "dimension" {
        "all"
    } else {
        data["dimension"].as_str().unwrap_or("all")
    };
    SemanticElement {
        id: format!("model-dimensions:{key}:{id}"),
        role: "model-dimension-field".into(),
        label: label.into(),
        value: value.as_str().map(String::from),
        state: if editable { "ready" } else { "disabled" }.into(),
        commands: vec![],
        relationships: vec![data["part"].as_str().unwrap_or_default().into()],
        data,
    }
}
fn button(
    ctx: &Context<'_>,
    w: &Workspace,
    key: &str,
    label: String,
    suffix: &str,
    data: Value,
    enabled: bool,
) -> SemanticElement {
    let mut data = ctx.bind(w, data);
    data["enabled"] = enabled.into();
    SemanticElement {
        id: format!("{key}:{suffix}"),
        role: "model-dimension-action".into(),
        label,
        state: if enabled { "ready" } else { "disabled" }.into(),
        value: None,
        commands: vec![key.into()],
        relationships: vec![ctx.part.id.clone()],
        data,
    }
}
fn mode(w: &Workspace, ctx: &Context<'_>, id: &str) -> &'static str {
    match ctx.part.model_dimensions.overrides.get(id) {
        None => "default",
        Some(a)
            if a.tolerance.minus_mm != a.tolerance.plus_mm
                || w.fields.get(UNEQUAL).is_some_and(|s| s == id) =>
        {
            "unequal"
        }
        _ => "symmetric",
    }
}
fn next_label(ctx: &Context<'_>) -> Option<String> {
    // Keep the first 26 labels short. Offer two letters if a large part has used them all.
    (b'A'..=b'Z')
        .map(|c| (c as char).to_string())
        .chain(
            (b'A'..=b'Z')
                .flat_map(|a| (b'A'..=b'Z').map(move |b| format!("{}{}", a as char, b as char))),
        )
        .find(|s| !ctx.part.model_dimensions.datums.contains_key(s))
}
fn show_callouts(w: &Workspace) -> bool {
    unavailable(w).is_none() && (active(w) || w.selected.is_some() && w.dialog.is_none())
}
pub(crate) fn semantics(w: &Workspace) -> Vec<SemanticElement> {
    if !show_callouts(w) {
        return vec![];
    }
    let Some(ctx) = context(w) else {
        return vec![];
    };
    let mut elements = callouts(w, &ctx);
    if !active(w) {
        return elements;
    }
    for (kind, entries) in [
        ("review", &ctx.report.issues),
        ("coverage", &ctx.report.coverage),
    ] {
        for (i, entry) in entries.iter().enumerate() {
            elements.push(SemanticElement {
                id: format!("model-dimensions:{kind}/{i}"),
                role: "model-dimension-note".into(),
                label: entry.clone(),
                value: None,
                state: if kind == "review" { "invalid" } else { "ready" }.into(),
                commands: vec![],
                relationships: vec![ctx.part.id.clone()],
                data: ctx.bind(w, json!({"category":kind})),
            });
        }
    }
    let writable = ctx.writable(w);
    let dimension_choices = ctx.report.dimensions.iter().map(|d| json!({"value":d.id, "label":format!("{}   {}",d.label,mbd::dimension_text(ctx.part,&w.session.visible().settings,d))})).collect();
    if let Some(d) = ctx.focused(w) {
        let bind = ctx.bind(w,json!({"dimension":d.id,"nominal_mm":d.nominal_mm,"witnesses":d.witnesses,"supports":d.supports}));
        elements.push(field(
            "dimension",
            "Finished dimension",
            bind.clone(),
            json!(d.id),
            dimension_choices,
            true,
        ));
        let mode = mode(w, &ctx, &d.id);
        elements.push(field(
            "mode",
            "Tolerance",
            bind.clone(),
            json!(mode),
            vec![
                json!({"value":"default","label":"Model default"}),
                json!({"value":"symmetric","label":"Symmetric ±"}),
                json!({"value":"unequal","label":"Unequal limits"}),
            ],
            writable,
        ));
        if mode != "default" {
            let a = mbd::effective_tolerance(ctx.part, &w.session.visible().settings, &d.id);
            let mut quantity = bind;
            quantity["unit"] = json!(w.session.visible().settings.display_length_unit.symbol());
            if mode == "symmetric" {
                elements.push(field(
                    "both",
                    "± allowance",
                    quantity,
                    json!(format!("{} mm", a.plus_mm)),
                    vec![],
                    writable,
                ));
            } else {
                elements.push(field(
                    "lower",
                    "Lower allowance (−)",
                    quantity.clone(),
                    json!(format!("{} mm", a.minus_mm)),
                    vec![],
                    writable,
                ));
                elements.push(field(
                    "upper",
                    "Upper allowance (+)",
                    quantity,
                    json!(format!("{} mm", a.plus_mm)),
                    vec![],
                    writable,
                ));
            }
        }
    }
    let face = ctx.eligible_face(w);
    let selected_used = face.is_some_and(|pick| {
        ctx.part
            .model_dimensions
            .datums
            .values()
            .any(|bound| cad_core::selection_sets::same_reference(bound, pick))
    });
    for (label, bound) in &ctx.part.model_dimensions.datums {
        let same = face.is_some_and(|pick| cad_core::selection_sets::same_reference(bound, pick));
        let valid = ctx
            .report
            .datums
            .iter()
            .any(|d| d.explicit && &d.label == label);
        let description = if valid {
            format!("Datum {label}")
        } else {
            format!("Datum {label} · needs a face")
        };
        elements.push(SemanticElement {
            id: format!("model-datum:{label}"),
            role: "model-datum".into(),
            label: description,
            value: None,
            state: if valid { "ready" } else { "invalid" }.into(),
            commands: vec![],
            relationships: vec![ctx.part.id.clone()],
            data: ctx.bind(w, json!({"letter":label,"face":bound,"explicit":true})),
        });
        if let Some(pick) = face.filter(|_| !same && !selected_used) {
            elements.push(button(
                &ctx,
                w,
                "mbd.replace-datum",
                format!("Move {label} to selected face"),
                label,
                json!({"label":label,"face":pick,"previous":bound}),
                writable,
            ));
        }
        elements.push(button(
            &ctx,
            w,
            "mbd.clear-datum",
            format!("Remove {label}"),
            label,
            json!({"label":label,"previous":bound}),
            writable,
        ));
    }
    if let Some(face) = face.filter(|_| !selected_used)
        && let Some(label) = w
            .fields
            .get("mbd-datum-letter")
            .cloned()
            .or_else(|| next_label(&ctx))
    {
        let mut name = field(
            "datum-label",
            "Datum letter",
            ctx.bind(w, json!({"face":face})),
            json!(label),
            vec![],
            writable,
        );
        name.role = "model-datum-field".into();
        elements.push(name);
        elements.push(button(
            &ctx,
            w,
            "mbd.establish-datum",
            format!("Set selected face as {label}"),
            &label,
            json!({"label":label,"face":face}),
            writable,
        ));
    }
    for datum in &ctx.report.datums {
        if !datum.explicit && !ctx.part.model_dimensions.datums.contains_key(&datum.label) {
            let selected =
                face.is_some_and(|f| cad_core::selection_sets::same_reference(f, &datum.face));
            if !selected {
                elements.push(button(
                    &ctx,
                    w,
                    "mbd.locate-datum",
                    format!("Show suggested datum {}", datum.label),
                    &datum.label,
                    json!({"label":datum.label,"face":datum.face}),
                    true,
                ));
                continue;
            }
            elements.push(button(
                &ctx,
                w,
                "mbd.confirm-datum",
                format!("Accept suggested datum {}", datum.label),
                &datum.label,
                json!({"label":datum.label,"face":datum.face,"anchor":datum.anchor}),
                writable,
            ));
        }
    }
    for (key, saved) in &ctx.part.model_dimensions.overrides {
        if !ctx.report.dimensions.iter().any(|d| &d.id == key) {
            elements.push(button(
                &ctx,
                w,
                "mbd.clear-override",
                format!("Remove override for {}", saved.label),
                key,
                json!({"dimension":key}),
                writable,
            ));
        }
    }
    elements
}
pub(crate) fn current_value(w: &Workspace, e: &SemanticElement) -> Option<Value> {
    let ctx = context(w)?;
    if !active(w) || !ctx.is_current(w, &e.data) {
        return None;
    }
    if e.data["field"] == "datum-label" {
        return Some(json!(
            w.fields
                .get("mbd-datum-letter")
                .cloned()
                .or_else(|| next_label(&ctx))?
        ));
    }
    let d = ctx.focused(w)?;
    if e.data["field"] != "dimension" && e.data["dimension"] != d.id {
        return None;
    }
    let a = mbd::effective_tolerance(ctx.part, &w.session.visible().settings, &d.id);
    Some(match e.data["field"].as_str()? {
        "dimension" => json!(d.id),
        "mode" => json!(mode(w, &ctx, &d.id)),
        "both" | "upper" => json!(format!("{} mm", a.plus_mm)),
        "lower" => json!(format!("{} mm", a.minus_mm)),
        _ => return None,
    })
}
fn submit(w: &mut Workspace, part: String, edit: Edit) -> Result<()> {
    let prior = w.session.source_epoch().to_string();
    w.dispatch(Command::EditModelDimensions { part, edit });
    if prior == w.session.source_epoch() {
        return Err(CadError::Invalid(w.status.clone()));
    }
    Ok(())
}
pub(crate) fn set_value(w: &mut Workspace, e: &SemanticElement, value: Value) -> Result<()> {
    if current_value(w, e).is_none() {
        return Err(invalid("This model dimension changed. Select it again."));
    }
    let text = value
        .as_str()
        .ok_or_else(|| invalid("Enter a length or select an option"))?;
    let ctx = context(w).ok_or_else(|| invalid("Wait for the current part geometry"))?;
    if e.data["field"] == "datum-label" {
        if !ctx.writable(w) || json!(ctx.eligible_face(w)) != e.data["face"] {
            return Err(invalid("Select the intended datum face again"));
        }
        let label = text.trim().to_ascii_uppercase();
        if !(1..=3).contains(&label.len()) || !label.bytes().all(|b| b.is_ascii_uppercase()) {
            return Err(invalid("Enter one to three letters for the datum"));
        }
        if ctx.part.model_dimensions.datums.contains_key(&label) {
            return Err(invalid(
                "That letter is assigned. To reattach it, use Move to selected face.",
            ));
        }
        w.fields.insert("mbd-datum-letter".into(), label);
        return Ok(());
    }
    let dimension = ctx
        .focused(w)
        .ok_or_else(|| invalid("Choose a dimension"))?;
    let id = dimension.id.clone();
    let label = dimension.label.clone();
    let field = e.data["field"].as_str().unwrap_or("");
    if field == "dimension" {
        if w.fields.keys().any(|key| {
            key.starts_with("presentation-field-error:model-dimensions:")
                && !key.starts_with("presentation-field-error:model-dimensions:dimension:")
        }) {
            return Err(invalid("Correct the current tolerance first"));
        }
        if !ctx.report.dimensions.iter().any(|d| d.id == text) {
            return Err(invalid("The dimension is unavailable"));
        }
        select_dimension(w, text);
        return Ok(());
    }
    if !ctx.writable(w) {
        return Err(invalid(
            "Edit this requirement in the linked part's source project",
        ));
    }
    let part = ctx.part.id.clone();
    let previous = ctx
        .part
        .model_dimensions
        .overrides
        .get(&id)
        .map(|d| d.tolerance);
    let mut a = mbd::effective_tolerance(ctx.part, &w.session.visible().settings, &id);
    let tolerance = if field == "mode" {
        match text {
            "default" => None,
            "symmetric" => {
                a.minus_mm = a.plus_mm;
                Some(a)
            }
            "unequal" => Some(a),
            _ => return Err(invalid("Choose a tolerance mode")),
        }
    } else {
        let (_, mm) = crate::quantity_display::input_length(w.session.visible(), &part, text)?;
        if !mm.is_finite() || mm < 0. {
            return Err(invalid("Enter a nonnegative tolerance allowance"));
        }
        match field {
            "both" => {
                a.minus_mm = mm;
                a.plus_mm = mm;
            }
            "upper" => a.plus_mm = mm,
            "lower" => a.minus_mm = mm,
            _ => return Err(invalid("This tolerance field has changed")),
        }
        Some(a)
    };
    if previous != tolerance {
        submit(
            w,
            part,
            Edit::SetTolerance {
                id: id.clone(),
                label,
                tolerance,
            },
        )?;
    }
    if field == "mode" {
        if text == "unequal" {
            w.fields.insert(UNEQUAL.into(), id);
        } else {
            w.fields.remove(UNEQUAL);
        }
    }
    Ok(())
}

fn run(w: &mut Workspace, key: &str, data: &Value) -> Result<()> {
    if key == "inspection.model-dimensions" {
        return begin(w, None);
    }
    if matches!(
        key,
        "mbd.inspect-size" | "mbd.inspect-datum" | "mbd.locate-datum"
    ) {
        if !crate::quantity_display::source_fields_valid(w) {
            return Err(invalid("Correct the current tolerance first"));
        }
        let ctx = context(w)
            .filter(|ctx| ctx.is_current(w, data))
            .ok_or_else(|| invalid("The selected geometry changed"))?;
        if key == "mbd.inspect-size" {
            let dim = ctx
                .report
                .dimensions
                .iter()
                .find(|d| data["dimension"] == d.id)
                .ok_or_else(|| invalid("The dimension is no longer available"))?;
            let id = dim.id.clone();
            return begin(w, Some(&id));
        }
        let d = ctx
            .report
            .datums
            .iter()
            .find(|d| data["label"] == d.label && json!(d.face) == data["face"])
            .ok_or_else(|| invalid("The datum changed. Select it again."))?;
        let mut matches = ctx
            .mesh
            .ok_or_else(|| invalid("The datum face is unavailable"))?
            .faces
            .iter()
            .filter(|face| cad_core::selection_sets::same_reference(&face.reference, &d.face));
        let face = matches
            .next()
            .filter(|_| matches.next().is_none())
            .ok_or_else(|| invalid("The datum face is ambiguous"))?;
        let pick = viewport::Selection {
            instance: ctx.instance.path.last().unwrap_or(&ctx.part.id).clone(),
            path: ctx.instance.path.clone(),
            name: ctx.instance.name.clone(),
            definition: ctx.part.id.clone(),
            reference: face.reference.clone(),
            body: face.body.clone(),
            distance: 0.,
            point: viewport::placed(d.anchor, ctx.instance),
        };
        begin(w, None)?;
        if !viewport::select(w, pick) {
            return Err(invalid("The datum face is unavailable"));
        }
        w.fields.insert("mbd-reveal-datums".into(), "true".into());
        return Ok(());
    }
    if !active(w) || !crate::quantity_display::source_fields_valid(w) {
        return Err(invalid(
            "Open Model Dimensions and correct any unfinished input first",
        ));
    }
    let ctx = context(w).ok_or_else(|| invalid("Wait for the current model dimensions"))?;
    if !ctx.is_current(w, data) {
        return Err(invalid(
            "The selected part changed. Select the control again.",
        ));
    }
    if !ctx.writable(w) {
        return Err(invalid(
            "Edit these requirements in the linked part's source project",
        ));
    }
    let edit = if key == "mbd.clear-override" {
        let id = data["dimension"]
            .as_str()
            .ok_or_else(|| invalid("Choose an unavailable tolerance"))?;
        let source = ctx
            .part
            .model_dimensions
            .overrides
            .get(id)
            .ok_or_else(|| invalid("This override is already removed"))?;
        if ctx.report.dimensions.iter().any(|d| d.id == id) {
            return Err(invalid(
                "This dimension is available again. Choose Model default to clear its override.",
            ));
        }
        Edit::SetTolerance {
            id: id.into(),
            label: source.label.clone(),
            tolerance: None,
        }
    } else {
        let label = data["label"]
            .as_str()
            .ok_or_else(|| invalid("Choose a datum"))?;
        let previous = ctx.part.model_dimensions.datums.get(label);
        if json!(previous) != data["previous"] {
            return Err(invalid("The datum designation changed. Review it again."));
        }
        let face = match key {
            "mbd.establish-datum" | "mbd.replace-datum" => {
                let face = ctx.eligible_face(w).ok_or_else(|| {
                    invalid("Select a uniquely identified planar or cylindrical face on this part")
                })?;
                if json!(face) != data["face"] {
                    return Err(invalid(
                        "The selected face changed. Choose the action again.",
                    ));
                }
                Some(face.clone())
            }
            "mbd.confirm-datum" => Some(
                ctx.report
                    .datums
                    .iter()
                    .find(|d| !d.explicit && d.label == label && json!(d.face) == data["face"])
                    .ok_or_else(|| invalid("The suggested datum changed. Review it again."))?
                    .face
                    .clone(),
            ),
            "mbd.clear-datum" if previous.is_some() => None,
            _ => return Err(invalid("This datum action is unavailable")),
        };
        Edit::SetDatum {
            label: label.into(),
            face,
        }
    };
    let owner = ctx.part.id.clone();
    submit(w, owner, edit)?;
    if key == "mbd.establish-datum" || key == "mbd.confirm-datum" {
        w.fields.remove("mbd-datum-letter");
    }
    Ok(())
}
pub(crate) fn action(w: &mut Workspace, key: &str, data: Option<&Value>) -> bool {
    if key != "inspection.model-dimensions" && !key.starts_with("mbd.") {
        return false;
    }
    if let Err(err) = run(w, key, data.unwrap_or(&Value::Null)) {
        w.confirm_requested = false;
        w.status = err.to_string();
    }
    true
}
fn draw_action(ui: &mut egui::Ui, w: &mut Workspace, element: &SemanticElement) {
    if controls::button(
        ui,
        &element.id,
        &element.role,
        &["compact"],
        &element.label,
        element.state != "disabled",
        false,
    )
    .clicked()
    {
        action(w, &element.commands[0], Some(&element.data));
    }
}
pub(crate) fn show(ui: &mut egui::Ui, w: &mut Workspace) {
    if !active(w) {
        if controls::button(
            ui,
            "model-dimensions:open",
            "model-dimension-action",
            &[],
            "Model Dimensions…",
            unavailable(w).is_none(),
            false,
        )
        .clicked()
        {
            action(w, "inspection.model-dimensions", None);
        }
        return;
    }
    let Some(ctx) = context(w) else {
        let message = if current_geometry(w).is_some() {
            "Select a part with a finished solid to see its dimensions."
        } else {
            "Waiting for the current part geometry…"
        };
        ui.weak(message);
        return;
    };
    let issues = ctx.report.issues.clone();
    let mut coverage = ctx.report.coverage.clone();
    let empty = ctx.report.dimensions.is_empty();
    let has_face = ctx.eligible_face(w).is_some();
    let linked = !ctx.writable(w);
    let elements = semantics(w);
    if empty {
        ui.weak("No finished sizes available.");
    }
    for element in elements
        .iter()
        .filter(|e| e.role == "model-dimension-field")
    {
        crate::ui::semantic_field(ui, w, element);
    }
    if linked {
        ui.weak("Shared library part · edit requirements in its source project.");
    }
    let datum_controls: Vec<_> = elements
        .iter()
        .filter(|e| {
            matches!(e.role.as_str(), "model-datum" | "model-datum-field")
                || e.commands
                    .first()
                    .is_some_and(|cmd| cmd.ends_with("-datum"))
        })
        .collect();
    let reveal = w.fields.remove("mbd-reveal-datums").map(|_| true);
    let unresolved = datum_controls.iter().any(|e| e.state == "invalid");
    controls::inspector_section(
        ui,
        controls::InspectorSection {
            id: "model-dimensions:datums",
            role: "inspector-section",
            classes: &["model-datums"],
            label: "Datums",
            state: if unresolved { "invalid" } else { "ready" },
            collapse: Some(has_face || unresolved),
            open: reveal,
        },
        |ui| {
            if !has_face {
                ui.weak("Select a planar or cylindrical face to establish a datum.");
            }
            for e in datum_controls {
                if e.role == "model-datum-field" {
                    crate::ui::semantic_field(ui, w, e);
                } else if e.commands.is_empty() {
                    controls::label(ui, &e.id, &e.role, &[], &e.label, &e.state);
                } else {
                    draw_action(ui, w, e);
                }
            }
        },
    );
    if !issues.is_empty() {
        controls::inspector_section(
            ui,
            controls::InspectorSection {
                id: "model-dimensions:review",
                role: "inspector-section",
                classes: &["model-dimension-review"],
                label: "Needs review",
                state: "invalid",
                collapse: Some(true),
                open: None,
            },
            |ui| {
                for issue in &issues {
                    ui.label(issue);
                }
                for e in elements.iter().filter(|e| {
                    e.commands
                        .first()
                        .is_some_and(|cmd| cmd == "mbd.clear-override")
                }) {
                    draw_action(ui, w, e);
                }
            },
        );
    }
    if !coverage.is_empty() {
        controls::inspector_section(
            ui,
            controls::InspectorSection {
                id: "model-dimensions:coverage",
                role: "inspector-section",
                classes: &["model-dimension-coverage"],
                label: "Measurement coverage",
                state: "ready",
                collapse: Some(false),
                open: None,
            },
            |ui| {
                for detail in coverage.drain(..) {
                    ui.label(detail);
                }
            },
        );
    }
}

// Expose the same semantic callouts to themes and to the default viewport.
fn callouts(w: &Workspace, ctx: &Context<'_>) -> Vec<SemanticElement> {
    if !viewport::visible(w, ctx.instance) {
        return vec![];
    }
    let dimensions: Vec<_> = if active(w) {
        ctx.focused(w).into_iter().collect()
    } else {
        w.selected
            .as_ref()
            .into_iter()
            .flat_map(|p| ctx.related(p))
            .take(3)
            .collect()
    };

    let mut elements = Vec::new();
    for d in dimensions {
        let mut data=ctx.bind(w,json!({"dimension":d.id,"name":d.label,"nominal_mm":d.nominal_mm,"witnesses":d.witnesses, "supports":d.supports}));
        data["world_witnesses"] = json!(d.witnesses.map(|p| viewport::placed(p, ctx.instance)));
        elements.push(SemanticElement {
            id: format!("model-dimension-callout:{}", d.id),
            role: "model-dimension-callout".into(),
            label: mbd::dimension_text(ctx.part, &w.session.visible().settings, d),
            value: Some(d.label.clone()),
            state: "ready".into(),
            commands: vec!["mbd.inspect-size".into()],
            relationships: vec![ctx.part.id.clone(), d.id.clone()],
            data,
        });
    }
    for d in &ctx.report.datums {
        let selected = w.selected.as_ref().is_some_and(|s| {
            s.definition == ctx.part.id
                && s.path == ctx.instance.path
                && cad_core::selection_sets::same_reference(&s.reference, &d.face)
        });
        if !(selected || active(w) && d.explicit) {
            continue;
        }
        let point = viewport::placed(d.anchor, ctx.instance);
        elements.push(SemanticElement {
            id: format!("model-datum-callout:{}", d.label),
            role: "model-datum-callout".into(),
            label: if d.explicit {
                format!("[ {} ]", d.label)
            } else {
                format!("{}?", d.label)
            },
            value: Some(
                if d.explicit {
                    "Established datum"
                } else {
                    "Suggested datum"
                }
                .into(),
            ),
            state: "ready".into(),
            commands: vec!["mbd.inspect-datum".into()],
            relationships: vec![ctx.part.id.clone()],
            data: ctx.bind(
                w,
                json!({"label":d.label,"face":d.face,"world_witnesses":[point,point]}),
            ),
        });
    }
    elements
}
pub(crate) fn viewport(
    w: &mut Workspace,
    ui: &mut egui::Ui,
    rect: egui::Rect,
    blocked: &[egui::Rect],
) -> bool {
    if !show_callouts(w) || !rect.is_positive() {
        return false;
    }
    let Some(ctx) = context(w) else {
        return false;
    };
    let callouts = callouts(w, &ctx);
    let mut occupied = blocked.to_vec();
    let mut captured = false;
    for callout in callouts {
        let Ok([a, b]) =
            serde_json::from_value::<[[f64; 3]; 2]>(callout.data["world_witnesses"].clone())
        else {
            continue;
        };
        let (Some(a), Some(b)) = (viewport::project(w, rect, a), viewport::project(w, rect, b))
        else {
            continue;
        };
        let mid = a.lerp(b, 0.5);
        if !rect.contains(mid) {
            continue;
        }
        let props =
            controls::properties(&callout.id, &callout.role, &["viewport-overlay"], "ready");
        if props.get("display").is_some_and(|s| s == "none")
            || props.get("visibility").is_some_and(|s| s == "hidden")
        {
            continue;
        }
        let text_size = ui
            .painter()
            .layout_no_wrap(
                callout.label.clone(),
                egui::TextStyle::Button.resolve(ui.style()),
                ui.visuals().text_color(),
            )
            .size();
        let size = egui::vec2(
            controls::number(&props, "width", text_size.x + 16.).max(20.),
            controls::number(&props, "height", text_size.y + 9.).max(16.),
        )
        .min(rect.size());
        let offsets = [
            egui::vec2(14., 8.),
            egui::vec2(14., -size.y - 8.),
            egui::vec2(-size.x - 14., 8.),
            egui::vec2(-size.x - 14., -size.y - 8.),
            egui::vec2(14., size.y + 14.),
            egui::vec2(14., -size.y * 2. - 14.),
        ];
        let Some(default) = offsets
            .into_iter()
            .map(|offset| egui::Rect::from_min_size(mid + offset, size))
            .find(|b| rect.contains_rect(*b) && occupied.iter().all(|r| !r.intersects(*b)))
        else {
            continue;
        };
        let Some(panel) = controls::overlay_rect(&callout.id, &callout.role, rect, default) else {
            continue;
        };
        let color = props
            .get("color")
            .and_then(|c| crate::ui::color(c))
            .unwrap_or_else(|| viewport::selection_color(w));
        let stroke = egui::Stroke::new(controls::number(&props, "border-width", 1.2), color);
        ui.painter().line_segment([a, b], stroke);
        ui.painter().line_segment([mid, panel.clamp(mid)], stroke);
        let mut frame = ui.new_child(egui::UiBuilder::new().id_salt(&callout.id).max_rect(panel));
        controls::apply_properties(&mut frame, &props);
        let chip = controls::button(
            &mut frame,
            &callout.id,
            "dimension-value-button",
            &["viewport-dimension"],
            &callout.label,
            true,
            false,
        )
        .on_hover_text(callout.value.as_deref().unwrap_or(&callout.label));
        occupied.push(panel.union(chip.rect));
        captured |= chip.hovered() || chip.clicked() || chip.is_pointer_button_down_on();
        if chip.clicked() {
            action(w, &callout.commands[0], Some(&callout.data));
        }
    }
    captured
}
