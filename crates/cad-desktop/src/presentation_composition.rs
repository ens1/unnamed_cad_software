//! Native adapter for the application-owned presentation contracts.
//!
//! A renderer is a replaceable view of semantic data. Bindings and setters are
//! resolved independently of the view; custom layouts never own CAD state.
use crate::{Workspace, ui};
use bevy_egui::egui;
use cad_core::presentation::{Region, SemanticElement};
use std::collections::BTreeMap;

pub struct RenderContext<'a> {
    pub region: &'a Region,
    pub layout: &'a cad_core::presentation::LayoutDocument,
    pub texture: egui::TextureId,
    pub viewport_index: usize,
    pub semantics: &'a [SemanticElement],
}

pub type Renderer = fn(&mut egui::Ui, &mut Workspace, &RenderContext<'_>);

/// Render the declared hierarchy, rather than giving every leaf an unrelated
/// window. Parent clipping and scroll offsets therefore apply to all descendants.
pub fn render(
    ctx: &egui::Context,
    w: &mut Workspace,
    layout: &cad_core::presentation::LayoutDocument,
    regions: &[Region],
    semantics: &[SemanticElement],
    registry: &Registry,
    texture: egui::TextureId,
) {
    let Some(root) = regions.first() else { return };
    let children: BTreeMap<String, Vec<usize>> = regions.iter().enumerate().skip(1).fold(
        BTreeMap::new(),
        |mut children, (index, region)| {
            if let Some(parent) = layout
                .style_target(region, &w.view.active_layout, "ready")
                .ancestors
                .last()
            {
                children.entry(parent.id.clone()).or_default().push(index);
            }
            children
        },
    );
    struct Frame<'a> {
        layout: &'a cad_core::presentation::LayoutDocument,
        regions: &'a [Region],
        semantics: &'a [SemanticElement],
        children: &'a BTreeMap<String, Vec<usize>>,
        registry: &'a Registry,
        texture: egui::TextureId,
        viewport_index: usize,
    }
    impl Frame<'_> {
        fn node(&mut self, ui: &mut egui::Ui, w: &mut Workspace, index: usize, offset: egui::Vec2) {
            let region = &self.regions[index];
            let binding = self.layout.binding_for_in(&w.view.active_layout, region);
            let bound = binding
                .as_ref()
                .and_then(|binding| binding.bind.as_deref())
                .and_then(|id| self.semantics.iter().find(|element| element.id == id));
            let state = bound
                .map(|element| element.state.as_str())
                .unwrap_or("ready");
            let target = self
                .layout
                .style_target(region, &w.view.active_layout, state);
            let props = w.theme.properties_for(&target);
            if props.get("display").is_some_and(|value| value == "none")
                || props
                    .get("visibility")
                    .is_some_and(|value| value == "hidden")
                || self
                    .layout
                    .node_for_in(&w.view.active_layout, &region.id)
                    .is_some_and(|node| node.visibility.as_deref() == Some("hidden"))
            {
                return;
            }
            let rect = egui::Rect::from_min_size(
                egui::pos2(region.rect[0], region.rect[1]) + offset,
                egui::vec2(region.rect[2], region.rect[3]),
            );
            if rect.width() < 1. || rect.height() < 1. {
                return;
            }
            let mut child = ui.new_child(egui::UiBuilder::new().id_salt(&region.id).max_rect(rect));
            child.set_clip_rect(ui.clip_rect().intersect(rect));
            let container = self.layout.is_container_in(&w.view.active_layout, region);
            crate::presentation_controls::with_target(target, &mut child, |ui| {
                crate::presentation_controls::apply_properties(ui, &props);
                let frame = crate::presentation_controls::frame_from_properties(ui, &props);
                if region.role != "model-viewport" {
                    ui.painter().add(frame.paint(rect));
                }
                if container {
                    let descendants = self.children.get(&region.id).cloned().unwrap_or_default();
                    let overflow = props
                        .get("overflow")
                        .map(String::as_str)
                        .or_else(|| {
                            self.layout
                                .node_for_in(&w.view.active_layout, &region.id)
                                .and_then(|node| node.overflow.as_deref())
                        })
                        .unwrap_or("clip");
                    if matches!(overflow, "auto" | "scroll") {
                        let extent = descendants.iter().fold(
                            egui::vec2(rect.width(), rect.height()),
                            |extent, index| {
                                let child = &self.regions[*index];
                                egui::vec2(
                                    extent.x.max(child.rect[0] + child.rect[2] - region.rect[0]),
                                    extent.y.max(child.rect[1] + child.rect[3] - region.rect[1]),
                                )
                            },
                        );
                        egui::ScrollArea::both()
                            .id_salt(("presentation-scroll", &region.id))
                            .max_width(rect.width())
                            .max_height(rect.height())
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                let shifted =
                                    ui.max_rect().min - egui::pos2(region.rect[0], region.rect[1]);
                                ui.set_min_size(extent);
                                for index in descendants {
                                    self.node(ui, w, index, shifted);
                                }
                            });
                    } else {
                        for index in descendants {
                            self.node(ui, w, index, offset);
                        }
                    }
                } else {
                    let padding = props.get("padding").map(String::as_str).unwrap_or(
                        if region.role == "model-viewport" {
                            "0"
                        } else {
                            "4px"
                        },
                    );
                    let values = padding
                        .split_whitespace()
                        .filter_map(|value| value.trim_end_matches("px").parse::<f32>().ok())
                        .collect::<Vec<_>>();
                    let [top, right, bottom, left] = match values.as_slice() {
                        [all] => [*all; 4],
                        [vertical, horizontal] => [*vertical, *horizontal, *vertical, *horizontal],
                        [top, horizontal, bottom] => [*top, *horizontal, *bottom, *horizontal],
                        [top, right, bottom, left] => [*top, *right, *bottom, *left],
                        _ => [0.; 4],
                    };
                    let content = egui::Rect::from_min_max(
                        rect.min + egui::vec2(left, top),
                        rect.max - egui::vec2(right, bottom),
                    );
                    let mut content_ui = ui.new_child(egui::UiBuilder::new().max_rect(content));
                    let context = RenderContext {
                        region,
                        layout: self.layout,
                        texture: self.texture,
                        viewport_index: self.viewport_index,
                        semantics: self.semantics,
                    };
                    self.registry.render(&mut content_ui, w, &context);
                    if region.role == "model-viewport"
                        || bound.is_some_and(|element| {
                            element
                                .data
                                .get("renderer")
                                .and_then(|value| value.as_str())
                                == Some("model-viewport")
                        })
                    {
                        self.viewport_index += 1;
                    }
                }
            });
        }
    }
    egui::Area::new(egui::Id::new("presentation-workspace"))
        .order(egui::Order::Middle)
        .fixed_pos(egui::pos2(root.rect[0], root.rect[1]))
        .show(ctx, |ui| {
            ui.set_min_size(egui::vec2(root.rect[2], root.rect[3]));
            ui.set_max_size(egui::vec2(root.rect[2], root.rect[3]));
            Frame {
                layout,
                regions,
                semantics,
                children: &children,
                registry,
                texture,
                viewport_index: 0,
            }
            .node(ui, w, 0, egui::Vec2::ZERO);
        });
}

/// New native views register here without adding a layout role to a closed enum.
/// Scalar fields, actions, and repeated semantic templates need no native view.
#[derive(Default)]
pub struct Registry {
    renderers: BTreeMap<&'static str, Renderer>,
}
impl Registry {
    pub fn register(&mut self, role: &'static str, renderer: Renderer) {
        self.renderers.insert(role, renderer);
    }
    pub fn render(&self, ui: &mut egui::Ui, w: &mut Workspace, context: &RenderContext<'_>) {
        let binding = context
            .layout
            .binding_for_in(&w.view.active_layout, context.region);
        if let Some(binding) = &binding {
            if let Some(command) = &binding.command {
                let descriptor = context
                    .semantics
                    .iter()
                    .find(|element| element.id == format!("command:{command}"));
                ui::semantic_command(ui, w, context.region, command, descriptor);
                return;
            }
            if let Some(id) = &binding.bind {
                if let Some(element) = context.semantics.iter().find(|element| &element.id == id) {
                    // Workspace views are ordinary registered adapters. A binding
                    // to a field or action takes precedence over any native role.
                    if let Some(role) = element.data.get("renderer").and_then(|v| v.as_str())
                        && let Some(renderer) = self.renderers.get(role)
                    {
                        renderer(ui, w, context);
                        return;
                    }
                    if let Some(label) = &binding.label {
                        let mut presented = element.clone();
                        presented.label.clone_from(label);
                        ui::semantic_element(ui, w, context.region, binding, &presented);
                    } else {
                        ui::semantic_element(ui, w, context.region, binding, element);
                    }
                    return;
                }
                if !id.starts_with("workspace.") {
                    ui.weak(format!("Binding unavailable: {id}"));
                    return;
                }
            }
        }
        if let Some(renderer) = self.renderers.get(context.region.role.as_str()) {
            renderer(ui, w, context);
        } else {
            ui.weak(format!(
                "No view or semantic binding for {}",
                context.region.id
            ));
        }
    }
}

/// One snapshot supplies the native workspace, binding catalog and user templates.
pub fn semantics(w: &Workspace) -> Vec<SemanticElement> {
    let mut elements = cad_core::presentation::project(&w.session, w.view.active_part.as_deref());
    if let Some(sketch) = &w.sketch {
        elements.retain(|element| {
            !(matches!(
                element.role.as_str(),
                "sketch-point"
                    | "sketch-constraint"
                    | "sketch-member"
                    | "sketch-modifier"
                    | "sketch-instance"
                    | "sketch-dimension-group"
                    | "sketch-equation"
                    | "sketch-equation-field"
                    | "sketch-reference"
                    | "sketch-handle"
                    | "sketch-dimension-editor"
            ) && element.relationships.contains(&sketch.sketch_id))
        });
        elements.extend(cad_core::presentation::sketch_elements(
            w.session.visible(),
            &sketch.definition,
            &crate::sketch::sketch_definition(sketch),
        ));
    }
    elements.extend(cad_core::history::project(
        &w.session,
        &w.view,
        w.tool_checkpoint.is_some() || w.sketch.is_some(),
    ));
    elements.extend(crate::selection_ui::semantics(w));
    elements.extend(crate::sketch_edit_ui::semantics(w));
    for element in crate::sketch_blocks_ui::semantics(w) {
        if let Some(existing) = elements
            .iter_mut()
            .find(|existing| existing.id == element.id)
        {
            *existing = element;
        } else {
            elements.push(element);
        }
    }
    elements.extend(crate::configuration_ui::semantics(w));
    elements.extend(crate::sketch_ui::dimension_semantics(w));
    elements.extend(cad_core::navigation::semantic_fields(&w.view.navigation));
    elements.extend(crate::presentation_resources::semantics(w));
    for element in crate::cad_ui::semantics(w) {
        if let Some(existing) = elements
            .iter_mut()
            .find(|existing| existing.id == element.id)
        {
            *existing = element;
        } else {
            elements.push(element);
        }
    }
    for (id, role, label) in [
        ("workspace.context", "document-context", "Document context"),
        (
            "workspace.navigation",
            "view-navigation",
            "Workspace navigation",
        ),
        (
            "workspace.dependencies",
            "dependency-view",
            "Model and history",
        ),
        (
            "workspace.active_operation.parameters",
            "property-manager",
            "Active tool controls",
        ),
        (
            "workspace.draft.status",
            "preview-status",
            "Preview validation",
        ),
        ("workspace.viewport", "model-viewport", "Model viewport"),
    ] {
        elements.push(SemanticElement {
            id: id.into(), role: role.into(), label: label.into(), value: None,
            state: if w.editing_manager() { "editing" } else { "ready" }.into(),
            commands: vec![], relationships: vec![],
            data: serde_json::json!({"contract":"tcad.presentation.view","version":1,"renderer":role}),
        });
    }
    active_fields(w, &mut elements);
    dimension_fields(w, &mut elements);
    for (id, role, label) in [
        (
            "viewport.toolbar",
            "viewport-toolbar",
            "Model context and view controls",
        ),
        (
            "viewport.start-hint",
            "viewport-start-hint",
            "Start a sketch",
        ),
        ("viewport.view-cube", "view-cube", "View orientation cube"),
        (
            "viewport.candidates",
            "selection-candidates",
            "Selection candidates",
        ),
        (
            "viewport.isolation",
            "isolation-controls",
            "Edit in place controls",
        ),
        (
            "viewport.navigation-hint",
            "navigation-hint",
            "Navigation instructions",
        ),
    ] {
        elements.push(SemanticElement {
            id: id.into(), role: role.into(), label: label.into(), value: None,
            state: "ready".into(), commands: vec![], relationships: vec!["workspace.viewport".into()],
            data: serde_json::json!({"contract":"tcad.presentation.overlay","version":1,"styleable":true}),
        });
    }
    elements
}

fn escape(part: &str) -> String {
    part.replace('~', "~0").replace('/', "~1")
}

fn active_fields(w: &Workspace, elements: &mut Vec<SemanticElement>) {
    let Some(part) = w
        .view
        .active_part
        .as_ref()
        .and_then(|id| w.session.visible().parts.get(id))
    else {
        return;
    };
    let Some((index, feature)) = part
        .features
        .iter()
        .enumerate()
        .find(|(_, feature)| w.view.active_node.as_ref() == Some(&feature.id))
    else {
        return;
    };
    let prefix = format!("/parts/{}/features/{index}/operation", escape(&part.id));
    let source = serde_json::to_value(&feature.operation).unwrap_or_default();
    fn fields(
        value: &serde_json::Value,
        pointer: String,
        identity: String,
        owner: &str,
        out: &mut Vec<SemanticElement>,
    ) {
        match value {
            serde_json::Value::Object(object) => {
                for (key, value) in object {
                    fields(
                        value,
                        format!("{pointer}/{}", escape(key)),
                        format!("{identity}/{}", escape(key)),
                        owner,
                        out,
                    );
                }
            }
            serde_json::Value::Array(array) => {
                for (index, value) in array.iter().enumerate() {
                    let stable = value
                        .get("id")
                        .and_then(|id| id.as_str())
                        .map(|id| format!("@{}", escape(id)))
                        .unwrap_or_else(|| index.to_string());
                    fields(
                        value,
                        format!("{pointer}/{index}"),
                        format!("{identity}/{stable}"),
                        owner,
                        out,
                    );
                }
            }
            _ => {
                let key = identity.rsplit('/').next().unwrap_or(&identity);
                let editable = !matches!(
                    key,
                    "id" | "kind" | "definition" | "node" | "topology" | "role"
                ) && !key.ends_with("_id")
                    && !value.is_null();
                out.push(SemanticElement {
                    id: identity.clone(), role: "operation-field".into(), label:key.replace('_', " "),
                    value:Some(value.as_str().map(str::to_string).unwrap_or_else(||value.to_string())),
                    state:"draft".into(), commands:vec![], relationships:vec![owner.into()],
                    data:serde_json::json!({"contract":"tcad.presentation.field","version":1,"setter":"document-pointer","pointer":pointer,"json":value,"editable":editable,"accessibility_label":identity}),
                });
            }
        }
    }
    fields(
        &source,
        prefix,
        format!("operation:{}", feature.id),
        &feature.id,
        elements,
    );
}

fn dimension_fields(w: &Workspace, elements: &mut Vec<SemanticElement>) {
    let Some(sketch) = &w.sketch else { return };
    let Some(editor) = &sketch.dimension else {
        return;
    };
    let Some(constraint) = sketch
        .constraints
        .iter()
        .find(|c| c.id == editor.constraint)
    else {
        return;
    };
    for (field, label, value, editable) in [
        (
            "value",
            "Dimension",
            serde_json::json!(constraint.value.0),
            !constraint.driven,
        ),
        (
            "driven",
            "Reference dimension",
            serde_json::json!(constraint.driven),
            true,
        ),
    ] {
        elements.push(SemanticElement {
            id:format!("{}::smart-dimension::{field}", sketch.sketch_id), role:"transient-field".into(), label:label.into(),
            value:Some(value.as_str().map(str::to_string).unwrap_or_else(||value.to_string())), state:"draft".into(), commands:vec![],
            relationships:vec![constraint.id.clone(), sketch.sketch_id.clone()],
            data:serde_json::json!({"contract":"tcad.presentation.field","version":1,"setter":"sketch-dimension","constraint":constraint.id,"field":field,"json":value,"editable":editable,"unit":cad_core::sketch_actions::dimension_unit(&constraint.kind)}),
        });
    }
}

pub type ValueSetter =
    fn(&mut Workspace, &SemanticElement, serde_json::Value) -> cad_core::Result<()>;
pub type ValueReader = fn(&Workspace, &SemanticElement) -> Option<serde_json::Value>;

#[derive(Default)]
pub struct ValueAdapters {
    setters: BTreeMap<&'static str, ValueSetter>,
    readers: BTreeMap<&'static str, ValueReader>,
}
impl ValueAdapters {
    pub fn register(&mut self, identity: &'static str, setter: ValueSetter, reader: ValueReader) {
        self.setters.insert(identity, setter);
        self.readers.insert(identity, reader);
    }
    pub fn read(&self, w: &Workspace, element: &SemanticElement) -> Option<serde_json::Value> {
        let adapter = element
            .data
            .get("setter")
            .and_then(|value| value.as_str())
            .unwrap_or("document-pointer");
        self.readers
            .get(adapter)
            .and_then(|reader| reader(w, element))
    }
    pub fn set(
        &self,
        w: &mut Workspace,
        element: &SemanticElement,
        value: serde_json::Value,
    ) -> cad_core::Result<()> {
        let descriptor = element.value_descriptor();
        if !descriptor.editable {
            return Err(cad_core::CadError::Invalid(
                "This field is read-only".into(),
            ));
        }
        if let Some(number) = value.as_f64()
            && (descriptor.minimum.is_some_and(|minimum| number < minimum)
                || descriptor.maximum.is_some_and(|maximum| number > maximum))
        {
            return Err(cad_core::CadError::Invalid(
                "The field value is outside its published range".into(),
            ));
        }
        let adapter = element
            .data
            .get("setter")
            .and_then(|value| value.as_str())
            .unwrap_or("document-pointer");
        let setter = self.setters.get(adapter).ok_or_else(|| {
            cad_core::CadError::Unsupported(format!("No value adapter is registered for {adapter}"))
        })?;
        setter(w, element, value)
    }
}

/// Registration is independent of widget kind and layout. A future field adapter
/// only publishes its identity and implements its transaction-preserving setter.
pub fn set_value(
    w: &mut Workspace,
    element: &SemanticElement,
    value: serde_json::Value,
) -> cad_core::Result<()> {
    value_adapters().set(w, element, value)
}

pub fn current_value(w: &Workspace, element: &SemanticElement) -> serde_json::Value {
    value_adapters()
        .read(w, element)
        .unwrap_or_else(|| element.value_descriptor().value)
}

fn value_adapters() -> ValueAdapters {
    let mut adapters = ValueAdapters::default();
    adapters.register(
        "sketch-block",
        crate::sketch_blocks_ui::set_value,
        crate::sketch_blocks_ui::current_value,
    );
    adapters.register(
        "configuration-dimension",
        crate::configuration_ui::set_value,
        crate::configuration_ui::current_value,
    );
    adapters.register(
        "drawing-annotation",
        crate::drawing_ui::set_value,
        crate::drawing_ui::current_value,
    );
    adapters.register("document-pointer", set_document_field, |w, element| {
        serde_json::to_value(w.session.visible())
            .ok()?
            .pointer(element.data.get("pointer")?.as_str()?)
            .cloned()
    });
    adapters.register("sketch-dimension", set_dimension_field, |w, element| {
        let constraint = w.sketch.as_ref()?.constraints.iter().find(|constraint| {
            element
                .data
                .get("constraint")
                .and_then(|value| value.as_str())
                == Some(&constraint.id)
        })?;
        match element.data.get("field")?.as_str()? {
            "value" => Some(serde_json::json!(constraint.value.0)),
            "driven" => Some(serde_json::json!(constraint.driven)),
            _ => None,
        }
    });
    adapters.register(
        "presentation-resource",
        |w, element, value| {
            crate::presentation_resources::set_value(
                w,
                element
                    .data
                    .get("field")
                    .and_then(|value| value.as_str())
                    .unwrap_or_default(),
                value,
            )
        },
        |w, element| match element.data.get("field")?.as_str()? {
            "layout" => Some(serde_json::json!(w.layout_text)),
            "theme" => Some(serde_json::json!(w.theme_text)),
            "watch" => Some(serde_json::json!(w.presentation_resources.watch)),
            _ => None,
        },
    );
    adapters
}

fn set_dimension_field(
    w: &mut Workspace,
    element: &SemanticElement,
    value: serde_json::Value,
) -> cad_core::Result<()> {
    let constraint = element
        .data
        .get("constraint")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    let field = element
        .data
        .get("field")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    let Some(sketch) = &mut w.sketch else {
        return Err(cad_core::CadError::Invalid(
            "The sketch editor has closed".into(),
        ));
    };
    if !sketch
        .dimension
        .as_ref()
        .is_some_and(|editor| editor.constraint == constraint)
    {
        return Err(cad_core::CadError::Invalid(
            "The dimension editor has changed".into(),
        ));
    }
    let before = crate::sketch::sketch_definition(sketch);
    let target = sketch
        .constraints
        .iter_mut()
        .find(|c| c.id == constraint)
        .ok_or_else(|| cad_core::CadError::Invalid("The dimension no longer exists".into()))?;
    match field {
        "value" if !target.driven => {
            target.value.0 = value
                .as_str()
                .ok_or_else(|| {
                    cad_core::CadError::Invalid("Expected a dimension expression".into())
                })?
                .into()
        }
        "driven" => {
            target.driven = value.as_bool().ok_or_else(|| {
                cad_core::CadError::Invalid("Expected a reference dimension toggle".into())
            })?
        }
        _ => {
            return Err(cad_core::CadError::Invalid(
                "Dimension field is unavailable".into(),
            ));
        }
    }
    sketch.history.push(before);
    crate::sketch::solve(w);
    Ok(())
}

fn set_document_field(
    w: &mut Workspace,
    element: &SemanticElement,
    value: serde_json::Value,
) -> cad_core::Result<()> {
    let mut current_elements =
        cad_core::presentation::project(&w.session, w.view.active_part.as_deref());
    active_fields(w, &mut current_elements);
    let current = current_elements
        .iter()
        .find(|current| current.id == element.id)
        .ok_or_else(|| {
            cad_core::CadError::Invalid(
                "The field's stable source identity is no longer available".into(),
            )
        })?;
    if !current.value_descriptor().editable {
        return Err(cad_core::CadError::Invalid(
            "The source field is read-only".into(),
        ));
    }
    let pointer = current
        .data
        .get("pointer")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            cad_core::CadError::Invalid("No value adapter is registered for this field".into())
        })?;
    // A snapshot is only an address descriptor; take the source at the instant of
    // the edit so duplicated controls cannot replace one another's newer changes.
    let mut source = serde_json::to_value(w.session.visible())?;
    *source
        .pointer_mut(pointer)
        .ok_or_else(|| cad_core::CadError::Invalid("The bound field no longer exists".into()))? =
        value;
    let document: cad_core::Document = serde_json::from_value(source)?;
    let expected = document.fingerprint();
    w.dispatch(cad_core::commands::Command::ReplaceDocument { document });
    if w.session.visible().fingerprint() != expected {
        return Err(cad_core::CadError::Invalid(w.status.clone()));
    }
    if w.dialog.as_deref() == Some("feature")
        && let Some(feature) = w.fields.get("feature-editor-id").and_then(|id| {
            w.view
                .active_part
                .as_ref()
                .and_then(|owner| w.session.visible().parts.get(owner))
                .and_then(|part| part.features.iter().find(|feature| &feature.id == id))
        })
    {
        w.json = serde_json::to_string_pretty(feature)?;
    }
    Ok(())
}
