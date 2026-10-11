use super::{
    BindingDescriptor, Component, LayoutDocument, LayoutNode, Region, RepeatBinding,
    SemanticElement, StyleNode, StyleTarget, Theme, style,
};
use crate::{CadError, Result};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

fn invalid(message: impl Into<String>) -> CadError {
    CadError::Invalid(message.into())
}
fn identity(node: &LayoutNode, path: &str) -> String {
    node.id.clone().unwrap_or_else(|| {
        format!(
            "{}@{path}",
            node.component.as_deref().unwrap_or("container")
        )
    })
}
fn role(doc: &LayoutDocument, node: &LayoutNode) -> String {
    node.role
        .clone()
        .or_else(|| {
            node.component
                .as_ref()
                .and_then(|key| doc.components.get(key))
                .map(|component| component.role.clone())
        })
        .unwrap_or_else(|| "container".into())
}
fn style_node(
    doc: &LayoutDocument,
    node: &LayoutNode,
    path: &str,
    layout: &str,
    state: &str,
) -> StyleNode {
    let mut attributes = node.attributes.clone();
    attributes.insert("layout".into(), layout.into());
    if let Some(semantic) = node.semantic.as_ref().or(node.bind.as_ref()) {
        attributes.insert("semantic".into(), semantic.clone());
    }
    if let Some(component) = &node.component {
        attributes.insert("component".into(), component.clone());
    }
    let mut states = node.states.clone();
    for state in state.split_whitespace() {
        if !states.iter().any(|value| value == state) {
            states.push(state.into());
        }
    }
    StyleNode {
        id: identity(node, path),
        role: role(doc, node),
        classes: node.classes.clone(),
        attributes,
        states,
    }
}
impl LayoutDocument {
    pub fn load(path: &Path) -> Result<Self> {
        let doc: Self = serde_json::from_slice(&std::fs::read(path)?)?;
        doc.validate()?;
        Ok(doc)
    }
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != 1 {
            return Err(invalid(format!(
                "Unsupported layout schema_version {}; expected 1",
                self.schema_version
            )));
        }
        if self.layouts.is_empty() || self.layouts.len() > 256 || self.components.len() > 4096 {
            return Err(invalid(
                "Layout must contain 1 to 256 compositions and at most 4096 components",
            ));
        }
        for (key, component) in &self.components {
            if key.is_empty() || component.role.is_empty() {
                return Err(invalid(
                    "Layout component names and roles must not be empty",
                ));
            }
            if component
                .command
                .as_ref()
                .is_some_and(|command| command.trim().is_empty())
                || component
                    .bind
                    .as_ref()
                    .is_some_and(|bind| bind.trim().is_empty())
            {
                return Err(invalid(format!(
                    "Component {key} has an empty command or binding"
                )));
            }
        }
        fn walk(
            node: &LayoutNode,
            components: &BTreeMap<String, Component>,
            depth: usize,
            path: &str,
            ids: &mut BTreeSet<String>,
            count: &mut usize,
        ) -> Result<()> {
            *count += 1;
            if depth > 32 || *count > 16_384 {
                return Err(invalid(format!(
                    "Layout nesting or element budget exceeded at {path}"
                )));
            }
            let id = identity(node, path);
            if id.is_empty() || !ids.insert(id.clone()) {
                return Err(invalid(format!(
                    "Layout placement identity {id:?} is empty or repeated; duplicate bindings need distinct placement IDs"
                )));
            }
            if let Some(component) = &node.component
                && !components.contains_key(component)
            {
                return Err(invalid(format!("Unknown component {component:?} at {id}")));
            }
            if node
                .layout
                .as_deref()
                .is_some_and(|value| !matches!(value, "row" | "column"))
            {
                return Err(invalid(format!("Layout {id} supports row or column flow")));
            }
            for value in [
                node.width_logical_px,
                node.height_logical_px,
                node.grow,
                node.gap_logical_px,
                node.min_width_logical_px,
                node.max_width_logical_px,
                node.min_height_logical_px,
                node.max_height_logical_px,
            ]
            .into_iter()
            .flatten()
            .chain(node.padding_logical_px.into_iter().flatten())
            .chain(
                node.editing
                    .iter()
                    .flat_map(|value| [value.width_logical_px, value.height_logical_px, value.grow])
                    .flatten(),
            ) {
                if !value.is_finite() || value < 0. {
                    return Err(invalid(format!(
                        "Layout {id} requires finite nonnegative sizes"
                    )));
                }
            }
            if node
                .min_width_logical_px
                .zip(node.max_width_logical_px)
                .is_some_and(|(min, max)| min > max)
                || node
                    .min_height_logical_px
                    .zip(node.max_height_logical_px)
                    .is_some_and(|(min, max)| min > max)
            {
                return Err(invalid(format!("Layout {id} minimum exceeds maximum")));
            }
            if node
                .position
                .is_some_and(|position| position.iter().any(|value| !value.is_finite()))
            {
                return Err(invalid(format!("Layout {id} has a nonfinite position")));
            }
            if node
                .align
                .as_deref()
                .is_some_and(|value| !matches!(value, "stretch" | "start" | "end" | "center"))
                || node.justify.as_deref().is_some_and(|value| {
                    !matches!(value, "start" | "end" | "center" | "space-between")
                })
            {
                return Err(invalid(format!("Layout {id} has unsupported alignment")));
            }
            if node.anchor.as_deref().is_some_and(|value| {
                !matches!(
                    value,
                    "top-left"
                        | "top"
                        | "top-right"
                        | "left"
                        | "center"
                        | "right"
                        | "bottom-left"
                        | "bottom"
                        | "bottom-right"
                )
            }) {
                return Err(invalid(format!("Layout {id} has an unsupported anchor")));
            }
            if node
                .visibility
                .as_deref()
                .is_some_and(|value| !matches!(value, "visible" | "hidden"))
                || node
                    .overflow
                    .as_deref()
                    .is_some_and(|value| !matches!(value, "clip" | "hidden" | "scroll" | "auto"))
            {
                return Err(invalid(format!(
                    "Layout {id} has unsupported visibility or overflow"
                )));
            }
            if node.repeat.is_some() != node.template.is_some() {
                return Err(invalid(format!(
                    "Layout {id} requires repeat and template together"
                )));
            }
            if let Some(repeat) = &node.repeat
                && repeat.limit.is_some_and(|limit| limit > 4096)
            {
                return Err(invalid(format!("Layout {id} repeat limit exceeds 4096")));
            }
            if node.repeat.is_some() && !node.children.is_empty() {
                return Err(invalid(format!(
                    "Layout {id} cannot combine repeated template and static children"
                )));
            }
            for (index, child) in node.children.iter().enumerate() {
                walk(
                    child,
                    components,
                    depth + 1,
                    &format!("{path}/{index}"),
                    ids,
                    count,
                )?;
            }
            if let Some(template) = &node.template {
                let mut template_ids = BTreeSet::new();
                walk(
                    template,
                    components,
                    depth + 1,
                    &format!("{path}/template"),
                    &mut template_ids,
                    count,
                )?;
            }
            Ok(())
        }
        for (name, node) in &self.layouts {
            if name.trim().is_empty() {
                return Err(invalid("Layout name cannot be empty"));
            }
            walk(
                node,
                &self.components,
                0,
                "root",
                &mut BTreeSet::new(),
                &mut 0,
            )?;
        }
        Ok(())
    }
    pub fn node_for(&self, id: &str) -> Option<&LayoutNode> {
        fn walk<'a>(node: &'a LayoutNode, path: &str, id: &str) -> Option<&'a LayoutNode> {
            if identity(node, path) == id {
                return Some(node);
            }
            node.children
                .iter()
                .enumerate()
                .find_map(|(index, child)| walk(child, &format!("{path}/{index}"), id))
        }
        self.layouts
            .values()
            .find_map(|root| walk(root, "root", id))
    }
    pub fn node_for_in(&self, layout: &str, id: &str) -> Option<&LayoutNode> {
        fn walk<'a>(node: &'a LayoutNode, path: &str, id: &str) -> Option<&'a LayoutNode> {
            if identity(node, path) == id {
                return Some(node);
            }
            node.children
                .iter()
                .enumerate()
                .find_map(|(index, child)| walk(child, &format!("{path}/{index}"), id))
        }
        self.layouts
            .get(layout)
            .and_then(|root| walk(root, "root", id))
    }
    pub fn is_container(&self, region: &Region) -> bool {
        self.node_for(&region.id).is_some_and(|node| {
            !node.children.is_empty()
                || node.repeat.is_some()
                || node
                    .attributes
                    .get("collection")
                    .is_some_and(|value| value == "true")
        })
    }
    pub fn binding_for(&self, region: &Region) -> Option<BindingDescriptor> {
        let node = self.node_for(&region.id)?;
        self.binding_for_node(node)
    }
    pub fn binding_for_in(&self, layout: &str, region: &Region) -> Option<BindingDescriptor> {
        self.binding_for_node(self.node_for_in(layout, &region.id)?)
    }
    pub fn is_container_in(&self, layout: &str, region: &Region) -> bool {
        self.node_for_in(layout, &region.id).is_some_and(|node| {
            !node.children.is_empty()
                || node.repeat.is_some()
                || node
                    .attributes
                    .get("collection")
                    .is_some_and(|value| value == "true")
        })
    }
    fn binding_for_node(&self, node: &LayoutNode) -> Option<BindingDescriptor> {
        let component = node
            .component
            .as_ref()
            .and_then(|key| self.components.get(key));
        let binding = BindingDescriptor {
            bind: node
                .bind
                .clone()
                .or_else(|| node.semantic.clone())
                .or_else(|| component.and_then(|component| component.bind.clone())),
            command: node
                .command
                .clone()
                .or_else(|| component.and_then(|component| component.command.clone())),
            widget: node
                .widget
                .clone()
                .or_else(|| component.and_then(|component| component.widget.clone())),
            label: node
                .label
                .clone()
                .or_else(|| component.and_then(|component| component.label.clone())),
        };
        (binding.bind.is_some()
            || binding.command.is_some()
            || binding.widget.is_some()
            || binding.label.is_some())
        .then_some(binding)
    }
    pub fn style_target(&self, region: &Region, layout: &str, state: &str) -> StyleTarget {
        fn walk(
            doc: &LayoutDocument,
            node: &LayoutNode,
            path: &str,
            target: &str,
            layout: &str,
            state: &str,
            ancestors: &mut Vec<StyleNode>,
        ) -> Option<StyleTarget> {
            let current = style_node(
                doc,
                node,
                path,
                layout,
                if identity(node, path) == target {
                    state
                } else {
                    "ready"
                },
            );
            if current.id == target {
                return Some(StyleTarget {
                    node: current,
                    ancestors: ancestors.to_vec(),
                });
            }
            ancestors.push(current);
            for (index, child) in node.children.iter().enumerate() {
                if let Some(target) = walk(
                    doc,
                    child,
                    &format!("{path}/{index}"),
                    target,
                    layout,
                    state,
                    ancestors,
                ) {
                    ancestors.pop();
                    return Some(target);
                }
            }
            ancestors.pop();
            None
        }
        let fallback = region.style_target(layout, state);
        self.layouts
            .get(layout)
            .and_then(|root| {
                walk(
                    self,
                    root,
                    "root",
                    &region.id,
                    layout,
                    state,
                    &mut fallback.ancestors.clone(),
                )
            })
            .unwrap_or(fallback)
    }
    /// Collection publishers can add controls without editing the renderer.
    /// Bindings remain semantic identities; placement IDs are independently stable.
    pub fn expand_semantics(&self, elements: &[SemanticElement]) -> Result<Self> {
        let mut doc = self.clone();
        fn stable_part(id: &str) -> String {
            id.chars()
                .flat_map(|character| {
                    if matches!(character, '%' | '/') {
                        format!("%{:02X}", character as u32)
                            .chars()
                            .collect::<Vec<_>>()
                    } else {
                        vec![character]
                    }
                })
                .collect()
        }
        fn fill(
            node: &mut LayoutNode,
            prefix: &str,
            element: &SemanticElement,
            components: &BTreeMap<String, Component>,
        ) {
            let owner = element
                .data
                .get("owner")
                .and_then(|value| value.as_str())
                .or_else(|| {
                    element
                        .data
                        .get("target")
                        .and_then(|target| target.get("definition"))
                        .and_then(|value| value.as_str())
                })
                .or_else(|| element.relationships.first().map(String::as_str))
                .unwrap_or("");
            let replace = |value: &str| {
                value
                    .replace("{id}", &element.id)
                    .replace("{role}", &element.role)
                    .replace("{label}", &element.label)
                    .replace("{owner}", owner)
                    .replace(
                        "{command}",
                        element.commands.first().map(String::as_str).unwrap_or(""),
                    )
            };
            node.id = Some(format!(
                "{prefix}/{}",
                stable_part(node.id.as_deref().unwrap_or("control"))
            ));
            for text in [
                &mut node.bind,
                &mut node.command,
                &mut node.semantic,
                &mut node.label,
                &mut node.role,
            ]
            .into_iter()
            .flatten()
            {
                *text = replace(text);
            }
            for value in node.attributes.values_mut() {
                *value = replace(value);
            }
            if node.bind.is_none()
                && node.semantic.is_none()
                && node
                    .component
                    .as_ref()
                    .and_then(|key| components.get(key))
                    .and_then(|component| component.bind.as_ref())
                    .is_none()
            {
                node.semantic = Some(element.id.clone());
            }
            if let Some(repeat) = &mut node.repeat {
                for text in [&mut repeat.role, &mut repeat.id_prefix, &mut repeat.owner]
                    .into_iter()
                    .flatten()
                {
                    *text = replace(text);
                }
                for text in repeat
                    .ids
                    .iter_mut()
                    .chain(&mut repeat.relationships)
                    .chain(&mut repeat.states)
                {
                    *text = replace(text);
                }
            }
            let placement = node.id.clone().unwrap_or_default();
            for (index, child) in node.children.iter_mut().enumerate() {
                let child_prefix = if child.id.is_some() {
                    format!("{placement}/children")
                } else {
                    format!("{placement}/{index}")
                };
                fill(child, &child_prefix, element, components);
            }
        }
        fn matches(repeat: &RepeatBinding, element: &SemanticElement) -> bool {
            repeat
                .role
                .as_ref()
                .is_none_or(|role| role == &element.role)
                && repeat
                    .id_prefix
                    .as_ref()
                    .is_none_or(|prefix| element.id.starts_with(prefix))
                && (repeat.ids.is_empty() || repeat.ids.contains(&element.id))
                && (repeat.states.is_empty()
                    || repeat
                        .states
                        .iter()
                        .any(|state| element.state.split_whitespace().any(|value| value == state)))
                && (repeat.relationships.is_empty()
                    || repeat
                        .relationships
                        .iter()
                        .any(|relationship| element.relationships.contains(relationship)))
                && repeat.owner.as_ref().is_none_or(|owner| {
                    element.data.get("owner").and_then(|value| value.as_str()) == Some(owner)
                        || element
                            .data
                            .get("target")
                            .and_then(|target| target.get("definition"))
                            .and_then(|value| value.as_str())
                            == Some(owner)
                        || element.relationships.contains(owner)
                })
        }
        fn expand(
            node: &mut LayoutNode,
            path: &str,
            elements: &[SemanticElement],
            components: &BTreeMap<String, Component>,
            budget: &mut usize,
        ) -> Result<()> {
            if let Some(repeat) = node.repeat.take() {
                let template = node
                    .template
                    .take()
                    .ok_or_else(|| invalid("Repeated semantic collection has no template"))?;
                let parent = identity(node, path);
                node.attributes.insert("collection".into(), "true".into());
                node.children = elements
                    .iter()
                    .filter(|element| matches(&repeat, element))
                    .take(repeat.limit.unwrap_or(4096))
                    .map(|element| {
                        let mut child = (*template).clone();
                        fill(
                            &mut child,
                            &format!("{parent}/{}", stable_part(&element.id)),
                            element,
                            components,
                        );
                        if child.bind.is_none()
                            && child.command.is_none()
                            && child.semantic.is_none()
                        {
                            child.semantic = Some(element.id.clone());
                        }
                        child
                    })
                    .collect();
            }
            if let Some(element) = node
                .bind
                .as_ref()
                .or(node.semantic.as_ref())
                .or_else(|| {
                    node.component
                        .as_ref()
                        .and_then(|key| components.get(key))
                        .and_then(|component| component.bind.as_ref())
                })
                .and_then(|id| elements.iter().find(|element| &element.id == id))
            {
                for state in element.state.split_whitespace() {
                    if !node.states.iter().any(|value| value == state) {
                        node.states.push(state.into());
                    }
                }
                node.attributes
                    .insert("semantic-role".into(), element.role.clone());
                if let Some(owner) = element.data.get("owner").and_then(|value| value.as_str()) {
                    node.attributes.insert("owner".into(), owner.into());
                }
            }
            *budget += 1;
            if *budget > 16_384 {
                return Err(invalid("Expanded presentation exceeds 16384 placements"));
            }
            for (index, child) in node.children.iter_mut().enumerate() {
                expand(
                    child,
                    &format!("{path}/{index}"),
                    elements,
                    components,
                    budget,
                )?;
            }
            Ok(())
        }
        let editing = elements.iter().any(|element| {
            element.id.starts_with("workspace.")
                && element
                    .state
                    .split_whitespace()
                    .any(|state| state == "editing")
        });
        for root in doc.layouts.values_mut() {
            if editing && !root.states.iter().any(|state| state == "editing") {
                root.states.push("editing".into());
            }
            expand(root, "root", elements, &doc.components, &mut 0)?;
        }
        doc.validate()?;
        Ok(doc)
    }
    pub fn regions(&self, name: &str, width: f32, height: f32) -> Result<Vec<Region>> {
        self.regions_editing(name, width, height, false)
    }
    pub fn regions_editing(
        &self,
        name: &str,
        width: f32,
        height: f32,
        editing: bool,
    ) -> Result<Vec<Region>> {
        self.arrange(name, width, height, editing, None)
    }
    pub fn regions_presented(
        &self,
        name: &str,
        width: f32,
        height: f32,
        editing: bool,
        theme: &Theme,
    ) -> Result<Vec<Region>> {
        self.arrange(name, width, height, editing, Some(theme))
    }
    fn arrange(
        &self,
        name: &str,
        width: f32,
        height: f32,
        editing: bool,
        theme: Option<&Theme>,
    ) -> Result<Vec<Region>> {
        if !width.is_finite() || !height.is_finite() || width < 0. || height < 0. {
            return Err(invalid(
                "Presentation bounds must be finite and nonnegative",
            ));
        }
        let mut root = self
            .layouts
            .get(name)
            .cloned()
            .ok_or_else(|| invalid(format!("Unknown layout {name:?}")))?;
        struct StyleContext<'a> {
            doc: &'a LayoutDocument,
            name: &'a str,
            editing: bool,
            theme: Option<&'a Theme>,
            error: &'a std::cell::RefCell<Option<CadError>>,
        }
        fn apply(
            context: &StyleContext<'_>,
            node: &mut LayoutNode,
            path: &str,
            ancestors: &[StyleNode],
            size: [f32; 2],
        ) {
            let StyleContext {
                doc,
                name,
                editing,
                theme,
                error,
            } = *context;
            if error.borrow().is_some() {
                return;
            }
            if editing && let Some(sizing) = node.editing.take() {
                if let Some(width) = sizing.width_logical_px {
                    node.width_logical_px = Some(width);
                }
                if let Some(height) = sizing.height_logical_px {
                    node.height_logical_px = Some(height);
                }
                if let Some(grow) = sizing.grow {
                    node.grow = Some(grow);
                }
            }
            let current = style_node(
                doc,
                node,
                path,
                name,
                if editing { "editing" } else { "ready" },
            );
            node.states = current.states.clone();
            if let Some(theme) = theme {
                let properties = match theme.properties_checked_for(&StyleTarget {
                    node: current.clone(),
                    ancestors: ancestors.to_vec(),
                }) {
                    Ok(properties) => properties,
                    Err(diagnostic) => {
                        *error.borrow_mut() = Some(diagnostic);
                        return;
                    }
                };
                for (key, destination, reference) in [
                    ("width", &mut node.width_logical_px, size[0]),
                    ("height", &mut node.height_logical_px, size[1]),
                    ("min-width", &mut node.min_width_logical_px, size[0]),
                    ("max-width", &mut node.max_width_logical_px, size[0]),
                    ("min-height", &mut node.min_height_logical_px, size[1]),
                    ("max-height", &mut node.max_height_logical_px, size[1]),
                    ("gap", &mut node.gap_logical_px, size[0]),
                ] {
                    if let Some(value) = properties
                        .get(key)
                        .and_then(|value| style::length(value, reference))
                    {
                        *destination = Some(value);
                    }
                }
                if let Some(grow) = properties
                    .get("flex-grow")
                    .and_then(|value| value.parse::<f32>().ok())
                {
                    node.grow = Some(grow);
                }
                for (key, destination) in [
                    ("align-items", &mut node.align),
                    ("justify-content", &mut node.justify),
                    ("flex-direction", &mut node.layout),
                    ("anchor", &mut node.anchor),
                    ("visibility", &mut node.visibility),
                    ("overflow", &mut node.overflow),
                ] {
                    if let Some(value) = properties.get(key) {
                        *destination = Some(value.clone());
                    }
                }
                let gap_axis = if node.layout.as_deref() == Some("row") {
                    "column-gap"
                } else {
                    "row-gap"
                };
                if let Some(value) = properties
                    .get(gap_axis)
                    .and_then(|value| style::length(value, size[0]))
                {
                    node.gap_logical_px = Some(value);
                }
                if properties
                    .get("display")
                    .is_some_and(|value| value == "none")
                {
                    node.hidden = true;
                }
                if !node.children.is_empty() {
                    let mut padding = properties
                        .get("padding")
                        .and_then(|value| style::insets(value, size[0]))
                        .unwrap_or(node.padding_logical_px.unwrap_or([0.; 4]));
                    for (index, key) in [
                        "padding-top",
                        "padding-right",
                        "padding-bottom",
                        "padding-left",
                    ]
                    .iter()
                    .enumerate()
                    {
                        if let Some(value) = properties
                            .get(*key)
                            .and_then(|value| style::length(value, size[0]))
                        {
                            padding[index] = value;
                        }
                    }
                    node.padding_logical_px = Some(padding);
                }
                if properties
                    .get("position")
                    .is_some_and(|value| value == "absolute")
                {
                    let left = properties
                        .get("left")
                        .and_then(|value| style::length(value, size[0]));
                    let top = properties
                        .get("top")
                        .and_then(|value| style::length(value, size[1]));
                    let right = properties
                        .get("right")
                        .and_then(|value| style::length(value, size[0]));
                    let bottom = properties
                        .get("bottom")
                        .and_then(|value| style::length(value, size[1]));
                    node.anchor.get_or_insert_with(|| {
                        if left.is_none() && right.is_some() && top.is_none() && bottom.is_some() {
                            "bottom-right"
                        } else if left.is_none() && right.is_some() {
                            "top-right"
                        } else if top.is_none() && bottom.is_some() {
                            "bottom-left"
                        } else {
                            "top-left"
                        }
                        .into()
                    });
                    node.position = Some([
                        left.unwrap_or_else(|| -right.unwrap_or(0.)),
                        top.unwrap_or_else(|| -bottom.unwrap_or(0.)),
                    ]);
                    if let (Some(left), Some(right)) = (left, right) {
                        node.width_logical_px = Some((size[0] - left - right).max(0.));
                    }
                    if let (Some(top), Some(bottom)) = (top, bottom) {
                        node.height_logical_px = Some((size[1] - top - bottom).max(0.));
                    }
                }
            }
            if node
                .min_width_logical_px
                .zip(node.max_width_logical_px)
                .is_some_and(|(min, max)| min > max)
                || node
                    .min_height_logical_px
                    .zip(node.max_height_logical_px)
                    .is_some_and(|(min, max)| min > max)
            {
                *error.borrow_mut() = Some(invalid(format!(
                    "Style target {} minimum exceeds maximum at actual parent bounds",
                    current.id
                )));
            }
        }
        let synthetic = Region {
            id: String::new(),
            role: String::new(),
            component: None,
            rect: [0.; 4],
            classes: vec![],
            resizable: false,
        }
        .style_target(name, "ready");
        let style_error = std::cell::RefCell::new(None);
        let context = StyleContext {
            doc: self,
            name,
            editing,
            theme,
            error: &style_error,
        };
        apply(
            &context,
            &mut root,
            "root",
            &synthetic.ancestors,
            [width, height],
        );
        let mut regions = vec![];
        arrange_node(
            self,
            &root,
            [
                0.,
                0.,
                root.width_logical_px.unwrap_or(width),
                root.height_logical_px.unwrap_or(height),
            ],
            "root",
            &synthetic.ancestors,
            &|node, path, ancestors, size| apply(&context, node, path, ancestors, size),
            &mut regions,
        );
        if let Some(error) = style_error.into_inner() {
            return Err(error);
        }
        if let Some(region) = regions.iter().find(|region| {
            region.rect.iter().any(|value| !value.is_finite())
                || region.rect[2] < 0.
                || region.rect[3] < 0.
        }) {
            return Err(invalid(format!(
                "Layout {} produced nonfinite or negative bounds; reduce overflowing sizes",
                region.id
            )));
        }
        Ok(regions)
    }
}
fn bound(value: f32, min: Option<f32>, max: Option<f32>) -> f32 {
    value
        .max(min.unwrap_or(0.))
        .min(max.unwrap_or(f32::MAX))
        .max(0.)
}
fn intrinsic(doc: &LayoutDocument, node: &LayoutNode, row: bool) -> f32 {
    if let Some(size) = if row {
        node.width_logical_px
    } else {
        node.height_logical_px
    } {
        return size;
    }
    if node.grow.unwrap_or(0.) > 0. {
        return 0.;
    }
    match role(doc, node).as_str() {
        "document-context" => 76.,
        "view-navigation" | "preview-status" => 42.,
        "draft-actions" => 86.,
        "apply-action" => 82.,
        "cancel-action" => 48.,
        "parameter-instrument" => 180.,
        "semantic-control" | "action" | "field" => 30.,
        _ => 80.,
    }
}
fn arrange_node(
    doc: &LayoutDocument,
    node: &LayoutNode,
    mut rect: [f32; 4],
    path: &str,
    ancestors: &[StyleNode],
    prepare: &impl Fn(&mut LayoutNode, &str, &[StyleNode], [f32; 2]),
    out: &mut Vec<Region>,
) {
    if node.hidden || node.visibility.as_deref() == Some("hidden") {
        return;
    }
    if node.anchor.is_some() || node.position.is_some() {
        let anchor = node.anchor.as_deref().unwrap_or("top-left");
        let width = bound(
            node.width_logical_px.unwrap_or(rect[2]),
            node.min_width_logical_px,
            node.max_width_logical_px,
        );
        let height = bound(
            node.height_logical_px.unwrap_or(rect[3]),
            node.min_height_logical_px,
            node.max_height_logical_px,
        );
        let fx = if anchor.ends_with("right") || anchor == "right" {
            1.
        } else if matches!(anchor, "top" | "center" | "bottom") {
            0.5
        } else {
            0.
        };
        let fy = if anchor.starts_with("bottom") || anchor == "bottom" {
            1.
        } else if matches!(anchor, "left" | "center" | "right") {
            0.5
        } else {
            0.
        };
        let position = node.position.unwrap_or([0.; 2]);
        rect = [
            rect[0] + (rect[2] - width) * fx + position[0],
            rect[1] + (rect[3] - height) * fy + position[1],
            width,
            height,
        ];
    }
    out.push(Region {
        id: identity(node, path),
        role: role(doc, node),
        component: node.component.clone(),
        rect,
        classes: node.classes.clone(),
        resizable: node.resizable,
    });
    if node.children.is_empty() {
        return;
    }
    let padding = node.padding_logical_px.unwrap_or([0.; 4]);
    let inner = [
        rect[0] + padding[3],
        rect[1] + padding[0],
        (rect[2] - padding[1] - padding[3]).max(0.),
        (rect[3] - padding[0] - padding[2]).max(0.),
    ];
    let row = node.layout.as_deref() == Some("row");
    let layout = ancestors
        .first()
        .and_then(|node| node.attributes.get("layout"))
        .map(String::as_str)
        .unwrap_or("");
    let mut ancestry = ancestors.to_vec();
    ancestry.push(style_node(doc, node, path, layout, "ready"));
    let mut children = node.children.clone();
    for (index, child) in children.iter_mut().enumerate() {
        prepare(
            child,
            &format!("{path}/{index}"),
            &ancestry,
            [inner[2], inner[3]],
        );
    }
    let visible: Vec<_> = children
        .iter()
        .enumerate()
        .filter(|(_, child)| !child.hidden && child.anchor.is_none() && child.position.is_none())
        .collect();
    let available = if row { inner[2] } else { inner[3] };
    let cross = if row { inner[3] } else { inner[2] };
    let gap = node.gap_logical_px.unwrap_or(0.);
    let gap_total = gap * visible.len().saturating_sub(1) as f32;
    let mut spans: Vec<_> = visible
        .iter()
        .map(|(_, child)| {
            bound(
                intrinsic(doc, child, row),
                if row {
                    child.min_width_logical_px
                } else {
                    child.min_height_logical_px
                },
                if row {
                    child.max_width_logical_px
                } else {
                    child.max_height_logical_px
                },
            )
        })
        .collect();
    let mut remaining = (available - gap_total - spans.iter().sum::<f32>()).max(0.);
    // Redistribute growth after max bounds, rather than leaving a capped sibling's
    // unused allocation stranded or exceeding its declared maximum.
    for _ in 0..visible.len() {
        let growth: f32 = visible
            .iter()
            .enumerate()
            .filter(|(index, (_, child))| {
                spans[*index]
                    < (if row {
                        child.max_width_logical_px
                    } else {
                        child.max_height_logical_px
                    })
                    .unwrap_or(f32::MAX)
            })
            .map(|(_, (_, child))| child.grow.unwrap_or(0.))
            .sum();
        if remaining <= f32::EPSILON || growth <= 0. {
            break;
        }
        let mut used = 0.;
        for (index, (_, child)) in visible.iter().enumerate() {
            let max = if row {
                child.max_width_logical_px
            } else {
                child.max_height_logical_px
            };
            let add = (remaining * child.grow.unwrap_or(0.) / growth)
                .min((max.unwrap_or(f32::MAX) - spans[index]).max(0.));
            spans[index] += add;
            used += add;
        }
        if used <= f32::EPSILON {
            break;
        }
        remaining = (remaining - used).max(0.);
    }
    let justify = node.justify.as_deref().unwrap_or("start");
    let offset = match justify {
        "end" => remaining,
        "center" => remaining * 0.5,
        _ => 0.,
    };
    let gap = if justify == "space-between" && visible.len() > 1 {
        gap + remaining / (visible.len() - 1) as f32
    } else {
        gap
    };
    let mut cursor = if row { inner[0] } else { inner[1] } + offset;
    for (index, (child_index, child)) in visible.iter().enumerate() {
        let explicit = if row {
            child.height_logical_px
        } else {
            child.width_logical_px
        };
        let min = if row {
            child.min_height_logical_px
        } else {
            child.min_width_logical_px
        };
        let max = if row {
            child.max_height_logical_px
        } else {
            child.max_width_logical_px
        };
        let cross_span = bound(explicit.unwrap_or(cross), min, max);
        let align = child
            .align
            .as_deref()
            .or(node.align.as_deref())
            .unwrap_or("stretch");
        let cross_offset = match align {
            "end" => (cross - cross_span).max(0.),
            "center" => (cross - cross_span).max(0.) * 0.5,
            _ => 0.,
        };
        let child_rect = if row {
            [cursor, inner[1] + cross_offset, spans[index], cross_span]
        } else {
            [inner[0] + cross_offset, cursor, cross_span, spans[index]]
        };
        arrange_node(
            doc,
            child,
            child_rect,
            &format!("{path}/{child_index}"),
            &ancestry,
            prepare,
            out,
        );
        cursor += spans[index] + gap;
    }
    for (index, child) in children
        .iter()
        .enumerate()
        .filter(|(_, child)| !child.hidden && (child.anchor.is_some() || child.position.is_some()))
    {
        arrange_node(
            doc,
            child,
            inner,
            &format!("{path}/{index}"),
            &ancestry,
            prepare,
            out,
        );
    }
}
