//! Route a measured size to a stored expression, preserving its configuration
//! and the existing model-dimension tool's checkpoint and validation gate.
use super::{Context, Workspace, button, field, invalid, mbd, widget_key};
use cad_core::{
    Dimension, Expr, Operation, Result, SketchCurve, commands::Command, configuration,
    model_dimensions::drivers::Driver, presentation::SemanticElement,
};
use serde_json::{Value, json};

const SHOW: &str = "mbd-show-source";
const PICK: &str = "mbd-picked-source";
pub(super) const EDITING: &str = "mbd-editing-source";

struct Binding {
    driver: Driver,
    label: String,
    value: Expr,
    configuration: Option<String>,
}
impl Binding {
    fn key(&self) -> String {
        widget_key(&serde_json::to_string(&self.driver).unwrap_or_default())
    }
    fn resolve(w: &Workspace, ctx: &Context<'_>, driver: &Driver) -> Option<Self> {
        let doc = w.session.visible();
        let part = ctx.part;
        let config = configuration::selected(doc, &part.id, ctx.instance.configuration.as_ref());
        let resolved = config
            .map(|id| configuration::resolve(doc, &part.id, id))
            .transpose()
            .ok()?;
        let overrides = resolved.as_ref().map(|r| &r.overrides);
        let (owner, label, base, overridden) = match driver {
            Driver::Feature { feature, path } => {
                let f = part.features.iter().find(|f| f.id == *feature)?;
                let label = match path.as_str() {
                    "/distance" => "Depth",
                    "/options/start_offset" => "Start offset",
                    "/radius" | "/profile/radius" => "Radius",
                    "/definition/diameter" => "Bore diameter",
                    "/definition/thread/tap_drill_diameter" => "Tap-drill diameter",
                    "/definition/head/diameter" if matches!(&f.operation,Operation::DrilledHoles{definition} if matches!(definition.head,cad_core::holes::Head::Counterbore{..})) => {
                        "Counterbore diameter"
                    }
                    "/definition/head/diameter" => "Countersink diameter",
                    "/definition/head/depth" => "Counterbore depth",
                    "/definition/head/angle" => "Countersink angle",
                    "/definition/depth" => "Hole depth",
                    _ => return None,
                };
                let source = serde_json::to_value(&f.operation).ok()?;
                let base = Expr(source.pointer(path)?.as_str()?.into());
                let overridden =
                    overrides.and_then(|o| o.feature_dimensions.get(feature)?.get(path));
                (&f.name, label, base, overridden)
            }
            Driver::SketchDimension { sketch, constraint } => {
                let s = part.sketches.get(sketch)?;
                let c = s
                    .constraints
                    .iter()
                    .find(|c| c.id == *constraint && !c.driven)?;
                let label = cad_core::sketch_actions::label(&c.kind);
                let overridden =
                    overrides.and_then(|o| o.sketch_dimensions.get(sketch)?.get(constraint));
                (&s.name, label, c.value.clone(), overridden)
            }
            Driver::CircleRadius { sketch, member } => {
                let s = part.sketches.get(sketch)?;
                let SketchCurve::Circle { radius, .. } = &s.members.get(member)?.curve else {
                    return None;
                };
                (&s.name, "Radius", radius.clone(), None)
            }
        };
        let configuration = overridden.and(config).cloned();
        let mut label = format!("{owner} · {label}");
        if let Some(r) = resolved.as_ref() {
            label.push_str(" · ");
            label.push_str(if configuration.is_some() {
                &r.name
            } else {
                "Base part"
            });
        }
        Some(Self {
            driver: driver.clone(),
            label,
            value: overridden.cloned().unwrap_or(base),
            configuration,
        })
    }
    fn command(&self, ctx: &Context<'_>, w: &Workspace, text: &str) -> Result<Option<Command>> {
        let doc = w.session.visible();
        let definition = ctx.part.id.clone();
        let selected =
            configuration::selected(doc, &definition, ctx.instance.configuration.as_ref());
        // Resolve input units and named parameters in the occurrence the user actually picked.
        let effective;
        let source = if let Some(selected) = selected {
            effective = configuration::variant(doc, &definition, Some(selected))?;
            &effective
        } else {
            doc
        };
        let is_angle =
            matches!(&self.driver,Driver::Feature{path,..} if path=="/definition/head/angle");
        let expr = if is_angle {
            let raw = text.trim();
            let expr = raw
                .strip_suffix('°')
                .map_or_else(|| Expr(raw.into()), |v| Expr(format!("({v}) * 1 deg")));
            let q = cad_core::expression::quantity(source, &definition, &expr)?;
            if q.matches(Dimension::Angle) {
                expr
            } else if q.matches(Dimension::Scalar) {
                Expr(format!("({}) * 1 deg", expr.0))
            } else {
                return Err(invalid("Enter an angle"));
            }
        } else {
            crate::quantity_display::input_length(source, &definition, text)?.0
        };
        if expr == self.value {
            return Ok(None);
        }
        if let Some(config) = &self.configuration {
            let mut configuration = configuration::definitions(doc, &definition)
                .remove(config)
                .ok_or_else(|| invalid("Choose the configuration again"))?;
            match &self.driver {
                Driver::Feature { feature, path } => configuration
                    .overrides
                    .feature_dimensions
                    .entry(feature.clone())
                    .or_default()
                    .insert(path.clone(), expr),
                Driver::SketchDimension { sketch, constraint } => configuration
                    .overrides
                    .sketch_dimensions
                    .entry(sketch.clone())
                    .or_default()
                    .insert(constraint.clone(), expr),
                Driver::CircleRadius { .. } => {
                    return Err(invalid("This circle has no configuration override"));
                }
            };
            return Ok(Some(Command::PutConfiguration {
                definition,
                configuration: Box::new(configuration),
            }));
        }
        let command = match &self.driver {
            Driver::Feature { feature, path } => {
                let mut f = ctx
                    .part
                    .features
                    .iter()
                    .find(|f| f.id == *feature)
                    .cloned()
                    .ok_or_else(|| invalid("The source feature changed"))?;
                let mut operation = serde_json::to_value(&f.operation)?;
                *operation
                    .pointer_mut(path)
                    .ok_or_else(|| invalid("The source field changed"))? = json!(expr);
                f.operation = serde_json::from_value(operation)?;
                Command::PutFeature {
                    definition,
                    feature: Box::new(f),
                    output: ctx.part.outputs.contains(feature),
                }
            }
            Driver::SketchDimension { sketch, .. } | Driver::CircleRadius { sketch, .. } => {
                let mut sketch = ctx
                    .part
                    .sketches
                    .get(sketch)
                    .cloned()
                    .ok_or_else(|| invalid("The source sketch changed"))?;
                match &self.driver {
                    Driver::SketchDimension { constraint, .. } => {
                        sketch
                            .constraints
                            .iter_mut()
                            .find(|c| c.id == *constraint && !c.driven)
                            .ok_or_else(|| invalid("The driving dimension changed"))?
                            .value = expr;
                    }
                    Driver::CircleRadius { member, .. } => {
                        let SketchCurve::Circle { radius, .. } = &mut sketch
                            .members
                            .get_mut(member)
                            .ok_or_else(|| invalid("The circle changed"))?
                            .curve
                        else {
                            return Err(invalid("The circle changed"));
                        };
                        *radius = expr;
                    }
                    _ => unreachable!(),
                }
                Command::PutSketch { definition, sketch }
            }
        };
        Ok(Some(command))
    }
}

pub(super) fn reveal(w: &mut Workspace, id: &str) {
    w.fields.insert(SHOW.into(), id.into());
}
pub(super) fn reset(w: &mut Workspace) {
    w.fields.remove(SHOW);
    w.fields.remove(PICK);
    w.fields.remove(EDITING);
    w.fields.remove("mbd-source-measurement");
}
fn bindings(w: &Workspace, ctx: &Context<'_>, dim: &mbd::Characteristic) -> Vec<Binding> {
    dim.drivers
        .iter()
        .map(|d| Binding::resolve(w, ctx, &d.source))
        .collect::<Option<Vec<_>>>()
        .unwrap_or_default()
}
fn selected<'a>(w: &Workspace, options: &'a [Binding]) -> Option<&'a Binding> {
    match options {
        [only] => Some(only),
        _ => options
            .iter()
            .find(|o| Some(&o.key()) == w.fields.get(PICK)),
    }
}

pub(super) fn elements(
    w: &Workspace,
    ctx: &Context<'_>,
    dim: &mbd::Characteristic,
) -> Vec<SemanticElement> {
    let bind = ctx.bind(w, json!({"characteristic":dim.id,"category":"size-source"}));
    if w.fields.get(SHOW) != Some(&dim.id) {
        return vec![button(
            ctx,
            w,
            "mbd.show-source",
            "What sets this size?".into(),
            &dim.id,
            bind,
            true,
        )];
    }
    let options = bindings(w, ctx, dim);
    if options.is_empty() {
        let mut names = ctx
            .part
            .features
            .iter()
            .filter(|f| {
                dim.supports
                    .iter()
                    .any(|s| s.producer.definition == ctx.part.id && s.producer.node == f.id)
            })
            .map(|f| f.name.as_str())
            .take(3)
            .collect::<Vec<_>>();
        names.dedup();
        let label = if names.is_empty() {
            "Measured on the finished part. No verified driving dimension.".into()
        } else {
            format!(
                "Measured on {}. No verified driving dimension.",
                names.join(", ")
            )
        };
        return vec![SemanticElement {
            id: format!("model-dimensions:source-note:{}", widget_key(&dim.id)),
            role: "model-size-source-note".into(),
            label,
            value: None,
            state: "read-only".into(),
            commands: vec![],
            relationships: vec![ctx.part.id.clone()],
            data: bind,
        }];
    }
    let mut result = vec![];
    let source = selected(w, &options);
    if options.len() > 1 {
        let choices = std::iter::once(json!({"value":"","label":"Choose a driving value…"}))
            .chain(options.iter().zip(&dim.drivers).map(|(s, d)| {
                let label = match d.factor {
                    Some(f) if f < 0. => format!("{} · reduces size", s.label),
                    Some(f) if f > 0. => format!("{} · adds to size", s.label),
                    _ => s.label.clone(),
                };
                json!({"value":s.key(),"label":label})
            }))
            .collect();
        result.push(field(
            "source",
            "What sets this size?",
            bind.clone(),
            json!(source.map(Binding::key).unwrap_or_default()),
            choices,
            true,
        ));
    }
    if let Some(source) = source {
        let mut bind = bind;
        bind["driver"] = json!(source.driver);
        bind["configuration_override"] = json!(source.configuration);
        bind["unit"] = json!(if dim.kind == mbd::Kind::Angle {
            "deg"
        } else {
            w.session.visible().settings.display_length_unit.symbol()
        });
        let mut e = field(
            "source-value",
            &source.label,
            bind,
            json!(source.value.0),
            vec![],
            ctx.writable(w),
        );
        e.id.push('-');
        e.id.push_str(&source.key());
        e.data["select_on_focus"] = true.into();
        e.data["focus_request"] = json!(format!("focus-request:{}", e.id));
        result.push(e);
    }
    result
}

pub(super) fn current(
    w: &Workspace,
    ctx: &Context<'_>,
    dim: &mbd::Characteristic,
    data: &Value,
) -> Option<Value> {
    if w.fields.get(SHOW) != Some(&dim.id) {
        return None;
    }
    let options = bindings(w, ctx, dim);
    let source = selected(w, &options);
    match data["field"].as_str()? {
        "source" if options.len() > 1 => Some(json!(source.map(Binding::key).unwrap_or_default())),
        "source-value"
            if data["driver"] == json!(source?.driver)
                && data["configuration_override"] == json!(source?.configuration) =>
        {
            Some(json!(source?.value.0))
        }
        _ => None,
    }
}

pub(super) fn update(w: &mut Workspace, data: &Value, text: &str) -> Result<()> {
    let ctx = super::context(w).ok_or_else(|| invalid("Select the finished dimension again"))?;
    let dim = ctx
        .focused(w)
        .ok_or_else(|| invalid("Select the finished dimension again"))?;
    let options = bindings(w, &ctx, dim);
    if data["field"] == "source" {
        if !crate::quantity_display::source_fields_valid(w) {
            return Err(invalid("Correct the source value first"));
        }
        if text.is_empty() {
            w.fields.remove(PICK);
            return Ok(());
        }
        let key = options
            .iter()
            .find(|o| o.key() == text)
            .ok_or_else(|| invalid("The driving value changed"))?
            .key();
        w.fields.insert(PICK.into(), key);
        focus(w);
        return Ok(());
    }
    if !ctx.writable(w) {
        return Err(invalid(
            "Edit this dimension in the linked part’s source project",
        ));
    }
    let source = selected(w, &options)
        .filter(|s| {
            json!(s.driver) == data["driver"]
                && json!(s.configuration) == data["configuration_override"]
        })
        .ok_or_else(|| invalid("Choose the driving value again"))?;
    if let Some(command) = source.command(&ctx, w, text)? {
        let id = dim.id.clone();
        let mut measured = dim.clone();
        measured.supports.clear();
        let snapshot = serde_json::to_string(&measured)?;
        super::dispatch(w, command)?;
        w.fields.insert("mbd-source-measurement".into(), snapshot);
        w.fields.insert(EDITING.into(), id);
    }
    Ok(())
}
pub(super) fn focus(w: &mut Workspace) {
    let Some(ctx) = super::context(w) else { return };
    let Some(dim) = ctx.focused(w) else { return };
    if let Some(e) = elements(w, &ctx, dim)
        .into_iter()
        .find(|e| e.data["field"] == "source-value" && e.state == "ready")
    {
        w.fields
            .insert(format!("focus-request:{}", e.id), "true".into());
    }
}
