//! Explicit source-reference replacement. Geometry proximity never chooses a replacement.
use crate::{presentation::SemanticElement, protocol::Evaluation, *};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;

pub const CONTRACT_VERSION: u32 = 1;
const MAX_SLOTS: usize = 16384;
const MAX_DEPTH: usize = 64;
const MAX_VISITS: usize = 1_048_576;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Class {
    Body,
    Profile,
    Plane,
    Axis,
    Point,
    CoordinateSystem,
    Curve,
    Feature,
    Topology(String),
    Sketch,
}
impl Class {
    pub fn label(&self) -> &str {
        match self {
            Self::Body => "Body",
            Self::Profile => "Sketch profile",
            Self::Plane => "Plane / planar face",
            Self::Axis => "Axis",
            Self::Point => "Point",
            Self::CoordinateSystem => "Coordinate system",
            Self::Curve => "Reference curve",
            Self::Feature => "Feature",
            Self::Topology(topology) => topology,
            Self::Sketch => "Sketch",
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "storage", rename_all = "snake_case")]
pub enum Storage {
    Operation,
    Sketch { sketch: Id },
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Binding {
    pub path: String,
    pub expected: Option<NodeRef>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Input {
    pub owner: NodeRef,
    pub storage: Storage,
    /// Exact application-owned serialized source location; never exposed as the primary UI label.
    pub path: String,
    pub label: String,
    pub class: Class,
    pub expected: Value,
    pub body: Option<Binding>,
    pub element: Option<ElementBinding>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ElementClass {
    Point,
    Member,
    Line,
    Circle,
    Members,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ElementBinding {
    pub path: String,
    pub expected: Value,
    pub class: ElementClass,
}
impl Input {
    pub fn semantic_id(&self) -> String {
        format!(
            "repair-input:{}::{}:{}:{}",
            self.owner.definition,
            self.owner.node,
            serde_json::to_string(&self.storage).unwrap_or_default(),
            self.path
        )
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "replacement", rename_all = "snake_case")]
pub enum Replacement {
    Node {
        node: NodeRef,
    },
    SketchElement {
        profile: NodeRef,
        elements: Vec<Id>,
    },
    Topology {
        reference: TopologyRef,
        body: NodeRef,
    },
    Sketch {
        sketch: Id,
    },
    OriginPlane {
        plane: crate::reference_geometry::OriginPlane,
    },
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Repair {
    pub input: Input,
    pub replacement: Replacement,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Failure {
    pub owner: NodeRef,
    pub label: String,
    pub messages: Vec<String>,
    pub inputs: Vec<Input>,
    pub configuration: Option<Id>,
}
fn invalid(message: impl Into<String>) -> CadError {
    CadError::Invalid(format!("Reference repair: {}", message.into()))
}
fn pointer(path: &str, key: &str) -> String {
    format!("{path}/{}", key.replace('~', "~0").replace('/', "~1"))
}
fn words(field: &str) -> String {
    let value = field.replace('_', " ");
    let mut chars = value.chars();
    chars
        .next()
        .map(|c| c.to_uppercase().collect::<String>() + chars.as_str())
        .unwrap_or(value)
}
fn node_class(document: &Document, node: &NodeRef, field: &str, context: &str) -> Class {
    if matches!(
        field,
        "body" | "reference_body" | "target_body" | "left" | "right" | "scope"
    ) {
        return Class::Body;
    }
    if matches!(
        field,
        "profile" | "profiles" | "path_sketch" | "guide" | "guides" | "sketch"
    ) {
        return Class::Profile;
    }
    if field == "feature" {
        return match context {
            "datum" | "plane" => document
                .parts
                .get(&node.definition)
                .and_then(|p| p.features.iter().find(|f| f.id == node.node))
                .map(|f| class_for_operation(&f.operation))
                .unwrap_or(Class::Feature),
            "coordinate_system" | "coordinate_system_origin" => Class::CoordinateSystem,
            "curve" => Class::Curve,
            _ => Class::Feature,
        };
    }
    document
        .parts
        .get(&node.definition)
        .and_then(|p| p.features.iter().find(|f| f.id == node.node))
        .map(|f| class_for_operation(&f.operation))
        .unwrap_or(Class::Feature)
}
fn class_for_operation(operation: &Operation) -> Class {
    match operation {
        Operation::Profile { .. } => Class::Profile,
        Operation::ReferencePlane { .. } => Class::Plane,
        Operation::ReferenceAxis { .. } => Class::Axis,
        Operation::ReferencePoint { .. } => Class::Point,
        Operation::ReferenceCoordinateSystem { .. } => Class::CoordinateSystem,
        Operation::ReferenceCurve { .. } => Class::Curve,
        operation if operation.produces_solid() || operation.is_surface() => Class::Body,
        _ => Class::Feature,
    }
}
struct Inventory<'a> {
    document: &'a Document,
    owner: &'a NodeRef,
    storage: Storage,
    output: Vec<Input>,
    visits: usize,
}
impl Inventory<'_> {
    fn walk(
        &mut self,
        value: &Value,
        path: &str,
        field: &str,
        context: &str,
        body: Option<Binding>,
        depth: usize,
    ) -> Result<()> {
        self.visits += 1;
        if depth > MAX_DEPTH || self.output.len() >= MAX_SLOTS || self.visits > MAX_VISITS {
            return Err(invalid("Source inventory exceeds its resource limit"));
        }
        let input = |class, body| Input {
            owner: self.owner.clone(),
            storage: self.storage.clone(),
            path: path.into(),
            label: words(field),
            class,
            expected: value.clone(),
            body,
            element: None,
        };
        let topology_record = value.as_object().is_some_and(|object| {
            object.contains_key("producer")
                && object.contains_key("role")
                && object.contains_key("topology")
                && object.keys().all(|key| {
                    matches!(key.as_str(), "producer" | "role" | "topology" | "signature")
                })
        });
        if topology_record
            && let Ok(reference) = serde_json::from_value::<TopologyRef>(value.clone())
        {
            let class = if field == "plane_reference" {
                Class::Plane
            } else {
                Class::Topology(reference.topology)
            };
            self.output.push(input(class, body));
            return Ok(());
        }
        let node_record = value.as_object().is_some_and(|object| {
            object.len() == 2 && object.contains_key("definition") && object.contains_key("node")
        });
        if node_record && let Ok(node) = serde_json::from_value::<NodeRef>(value.clone()) {
            self.output.push(input(
                node_class(self.document, &node, field, context),
                None,
            ));
            return Ok(());
        }
        match value {
            Value::Object(object) => {
                let context = object
                    .get("kind")
                    .and_then(Value::as_str)
                    .unwrap_or(context);
                let body = ["body", "reference_body", "target_body"]
                    .into_iter()
                    .find_map(|name| {
                        let expected =
                            serde_json::from_value::<Option<NodeRef>>(object.get(name)?.clone())
                                .ok()?;
                        Some(Binding {
                            path: pointer(path, name),
                            expected,
                        })
                    })
                    .or(body);
                for (key, child) in object {
                    if key == "sketch" && context == "sketch" && child.is_string() {
                        self.output.push(Input {
                            owner: self.owner.clone(),
                            storage: self.storage.clone(),
                            path: pointer(path, key),
                            label: "Source sketch".into(),
                            class: Class::Sketch,
                            expected: child.clone(),
                            body: None,
                            element: None,
                        });
                    } else {
                        let before = self.output.len();
                        self.walk(
                            child,
                            &pointer(path, key),
                            key,
                            context,
                            body.clone(),
                            depth + 1,
                        )?;
                        if key == "profile" {
                            let element = [
                                (
                                    "member",
                                    match context {
                                        "sketch_line" => ElementClass::Line,
                                        "sketch_circle" => ElementClass::Circle,
                                        _ => ElementClass::Member,
                                    },
                                ),
                                ("point", ElementClass::Point),
                                ("outer_members", ElementClass::Members),
                            ]
                            .into_iter()
                            .find_map(|(key, class)| {
                                object.get(key).map(|expected| ElementBinding {
                                    path: pointer(path, key),
                                    expected: expected.clone(),
                                    class,
                                })
                            });
                            if let Some(input) = self
                                .output
                                .get_mut(before)
                                .filter(|input| input.path == pointer(path, key))
                            {
                                input.element = element;
                            }
                        }
                    }
                }
            }
            Value::Array(array) => {
                for (index, child) in array.iter().enumerate() {
                    let before = self.output.len();
                    self.walk(
                        child,
                        &pointer(path, &index.to_string()),
                        field,
                        context,
                        body.clone(),
                        depth + 1,
                    )?;
                    for input in &mut self.output[before..] {
                        input.label = format!("{} {}", input.label, index + 1);
                        if field == "target_vertex" && index == 0 && array.len() == 2 {
                            input.element = Some(ElementBinding {
                                path: pointer(path, "1"),
                                expected: array[1].clone(),
                                class: ElementClass::Point,
                            });
                        }
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }
}
pub fn inputs(document: &Document, owner: &NodeRef) -> Result<Vec<Input>> {
    let part = document
        .parts
        .get(&owner.definition)
        .ok_or_else(|| invalid("Part is missing"))?;
    let feature = part
        .features
        .iter()
        .find(|f| f.id == owner.node)
        .ok_or_else(|| invalid("Feature is missing"))?;
    let mut inventory = Inventory {
        document,
        owner,
        storage: Storage::Operation,
        output: Vec::new(),
        visits: 0,
    };
    inventory.walk(
        &serde_json::to_value(&feature.operation)?,
        "",
        "Input",
        "",
        None,
        0,
    )?;
    if let Operation::Profile {
        profile: Profile::Sketch { sketch },
        ..
    } = &feature.operation
        && let Some(source) = part.sketches.get(sketch)
    {
        inventory.storage = Storage::Sketch {
            sketch: sketch.clone(),
        };
        // Only associative conversions hold external references; local sketch member IDs are left intact.
        let source = serde_json::to_value(source)?;
        if let Some(conversions) = source.get("conversions") {
            inventory.walk(conversions, "/conversions", "Converted edge", "", None, 0)?;
        }
    }
    Ok(inventory.output)
}
pub fn failures(document: &Document, evaluation: Option<&Evaluation>) -> Vec<Failure> {
    let mut failures = Vec::new();
    for part in document.parts.values() {
        for feature in &part.features {
            let messages = evaluation
                .into_iter()
                .flat_map(|e| &e.diagnostics)
                .filter(|d| d.severity == "error" && d.target.as_ref() == Some(&feature.id))
                .map(|d| d.message.clone())
                .collect::<Vec<_>>();
            if !messages.is_empty() {
                let owner = part.reference(&feature.id);
                failures.push(Failure {
                    inputs: inputs(document, &owner).unwrap_or_default(),
                    owner,
                    label: format!("{} / {}", part.name, feature.name),
                    messages,
                    configuration: evaluation
                        .and_then(|e| e.configurations.as_ref())
                        .and_then(|c| c.selected.get(&part.id))
                        .or_else(|| document.active_configurations.get(&part.id))
                        .cloned(),
                });
            }
        }
    }
    failures
}
pub fn node_compatible(document: &Document, input: &Input, node: &NodeRef) -> Result<()> {
    let part = document
        .parts
        .get(&node.definition)
        .ok_or_else(|| invalid("Replacement part is missing"))?;
    let feature = part
        .features
        .iter()
        .find(|f| f.id == node.node && !f.suppressed)
        .ok_or_else(|| invalid("Replacement feature is missing or suppressed"))?;
    let class = class_for_operation(&feature.operation);
    let compatible = match &input.class {
        Class::Feature => true,
        Class::Topology(_) => {
            class == Class::Body || class == Class::Profile || class == Class::Curve
        }
        Class::Plane => {
            class == Class::Body || class == Class::Plane || class == Class::CoordinateSystem
        }
        expected => expected == &class,
    };
    if !compatible {
        return Err(invalid("Replacement has an incompatible source type"));
    }
    if node.definition == input.owner.definition {
        let part = &document.parts[&node.definition];
        let replacement = part.features.iter().position(|f| f.id == node.node);
        let consumer = part.features.iter().position(|f| f.id == input.owner.node);
        if replacement
            .zip(consumer)
            .is_none_or(|(source, consumer)| source >= consumer)
        {
            return Err(invalid(
                "Pick a source before the repaired feature in history",
            ));
        }
    }
    Ok(())
}
pub fn element_choices(document: &Document, input: &Input, profile: &NodeRef) -> Vec<(Id, String)> {
    let Some(binding) = &input.element else {
        return Vec::new();
    };
    let Some(part) = document.parts.get(&profile.definition) else {
        return Vec::new();
    };
    let Some(Operation::Profile {
        profile: Profile::Sketch { sketch },
        ..
    }) = part
        .features
        .iter()
        .find(|f| f.id == profile.node)
        .map(|f| &f.operation)
    else {
        return Vec::new();
    };
    let Some(sketch) = part.sketches.get(sketch) else {
        return Vec::new();
    };
    if binding.class == ElementClass::Point {
        return sketch
            .points
            .iter()
            .enumerate()
            .map(|(index, (id, point))| {
                (
                    id.clone(),
                    format!("Point {} ({:.3}, {:.3} mm)", index + 1, point[0], point[1]),
                )
            })
            .collect();
    }
    sketch
        .members
        .iter()
        .enumerate()
        .filter(|(_, (_, member))| match binding.class {
            ElementClass::Line => matches!(member.curve, SketchCurve::Line { .. }),
            ElementClass::Circle => matches!(member.curve, SketchCurve::Circle { .. }),
            ElementClass::Members => !member.construction,
            _ => true,
        })
        .map(|(index, (id, member))| {
            let kind = match member.curve {
                SketchCurve::Line { .. } => "Line",
                SketchCurve::Arc { .. } => "Arc",
                SketchCurve::Bezier { .. } | SketchCurve::Nurbs { .. } => "Spline",
                SketchCurve::Ellipse { .. } => "Ellipse",
                SketchCurve::Circle { .. } => "Circle",
            };
            (
                id.clone(),
                format!(
                    "{kind} {}{}",
                    index + 1,
                    if member.construction {
                        " · construction"
                    } else {
                        ""
                    }
                ),
            )
        })
        .collect()
}
pub fn repair(document: &mut Document, repairs: &[Repair]) -> Result<()> {
    if repairs.is_empty() || repairs.len() > MAX_SLOTS {
        return Err(invalid("No bounded repair set was supplied"));
    }
    let mut candidate = document.clone();
    let mut occupied = BTreeSet::new();
    for repair in repairs {
        let input = &repair.input;
        let current = inputs(&candidate, &input.owner)?
            .into_iter()
            .find(|slot| slot.storage == input.storage && slot.path == input.path)
            .ok_or_else(|| invalid("The reference input no longer exists"))?;
        if &current != input {
            return Err(invalid(
                "The source reference changed; select its current input again",
            ));
        }
        if !occupied.insert((
            input.owner.clone(),
            serde_json::to_string(&input.storage)?,
            input.path.clone(),
        )) {
            return Err(invalid("The same source input was repaired twice"));
        }
        let (value, body) = match &repair.replacement {
            Replacement::Node { node } => {
                if input.element.is_some() {
                    return Err(invalid("Select an explicit source sketch member or point"));
                }
                if !input.expected.is_object() || input.expected.get("producer").is_some() {
                    return Err(invalid("This input requires a geometry reference"));
                }
                node_compatible(&candidate, input, node)?;
                (serde_json::to_value(node)?, None)
            }
            Replacement::SketchElement { profile, elements } => {
                let binding = input
                    .element
                    .as_ref()
                    .ok_or_else(|| invalid("This input has no sketch element binding"))?;
                node_compatible(&candidate, input, profile)?;
                if elements.is_empty()
                    || elements.len() > MAX_SLOTS
                    || (binding.class != ElementClass::Members && elements.len() != 1)
                    || elements.iter().collect::<BTreeSet<_>>().len() != elements.len()
                {
                    return Err(invalid("Select the required source sketch elements"));
                }
                let choices = element_choices(&candidate, input, profile)
                    .into_iter()
                    .map(|(id, _)| id)
                    .collect::<BTreeSet<_>>();
                if elements.iter().any(|element| !choices.contains(element)) {
                    return Err(invalid(
                        "The selected sketch element is missing or has an incompatible type",
                    ));
                }
                (serde_json::to_value(profile)?, None)
            }
            Replacement::Topology { reference, body } => {
                let topology = match &input.class {
                    Class::Topology(expected) => reference.topology == *expected,
                    Class::Plane => matches!(reference.topology.as_str(), "face" | "plane"),
                    _ => false,
                };
                if !topology {
                    return Err(invalid("Pick a compatible face, edge or plane"));
                }
                node_compatible(&candidate, input, body)?;
                let producer = candidate
                    .parts
                    .get(&reference.producer.definition)
                    .and_then(|p| p.features.iter().find(|f| f.id == reference.producer.node))
                    .ok_or_else(|| invalid("Replacement construction source is missing"))?;
                if producer.suppressed {
                    return Err(invalid("Replacement construction source is suppressed"));
                }
                (serde_json::to_value(reference)?, Some(body))
            }
            Replacement::Sketch { sketch } => {
                if input.class != Class::Sketch
                    || !candidate.parts[&input.owner.definition]
                        .sketches
                        .contains_key(sketch)
                {
                    return Err(invalid("Pick an existing source sketch"));
                }
                (Value::String(sketch.clone()), None)
            }
            Replacement::OriginPlane { .. } => {
                if input.storage != Storage::Operation
                    || input.path != "/plane_reference"
                    || input.class != Class::Plane
                {
                    return Err(invalid("Origin planes can replace a sketch support plane"));
                }
                (Value::Null, None)
            }
        };
        let part = candidate
            .parts
            .get_mut(&input.owner.definition)
            .ok_or_else(|| invalid("Part is missing"))?;
        let mut source = match &input.storage {
            Storage::Operation => serde_json::to_value(
                &part
                    .features
                    .iter()
                    .find(|f| f.id == input.owner.node)
                    .ok_or_else(|| invalid("Feature is missing"))?
                    .operation,
            )?,
            Storage::Sketch { sketch } => serde_json::to_value(
                part.sketches
                    .get(sketch)
                    .ok_or_else(|| invalid("Sketch is missing"))?,
            )?,
        };
        *source
            .pointer_mut(&input.path)
            .ok_or_else(|| invalid("Input path is missing"))? = value;
        if let Replacement::SketchElement { elements, .. } = &repair.replacement {
            let binding = input.element.as_ref().unwrap();
            let target = source
                .pointer_mut(&binding.path)
                .ok_or_else(|| invalid("The sketch element input is missing"))?;
            if *target != binding.expected {
                return Err(invalid("The source sketch element changed"));
            }
            *target = if binding.class == ElementClass::Members {
                serde_json::to_value(elements)?
            } else {
                Value::String(elements[0].clone())
            };
        }
        if let Some(binding) = &input.body {
            let target = source
                .pointer_mut(&binding.path)
                .ok_or_else(|| invalid("Reference body binding is missing"))?;
            if *target != serde_json::to_value(&binding.expected)? {
                return Err(invalid("Reference body binding changed"));
            }
            *target = body
                .map(serde_json::to_value)
                .transpose()?
                .unwrap_or(Value::Null);
        }
        if let Replacement::OriginPlane { plane } = &repair.replacement {
            source["plane"] = serde_json::to_value(plane.plane())?;
            source["reference_body"] = Value::Null;
        }
        match &input.storage {
            Storage::Operation => {
                let mut feature = part
                    .features
                    .iter()
                    .find(|f| f.id == input.owner.node)
                    .unwrap()
                    .clone();
                feature.operation = serde_json::from_value(source)?;
                let output = part.outputs.contains(&feature.id);
                crate::commands::Command::PutFeature {
                    definition: input.owner.definition.clone(),
                    feature: Box::new(feature),
                    output,
                }
                .execute(&mut candidate)?;
            }
            Storage::Sketch { sketch } => {
                part.sketches
                    .insert(sketch.clone(), serde_json::from_value(source)?);
            }
        }
    }
    candidate.validate()?;
    *document = candidate;
    Ok(())
}
pub fn project(failures: &[Failure]) -> Vec<SemanticElement> {
    failures.iter().flat_map(|failure| {
        let mut elements = vec![SemanticElement { id: format!("repair:{}::{}",failure.owner.definition,failure.owner.node),role:"failed-feature".into(),label:failure.label.clone(),value:Some(failure.messages.join("\n")),state:"error".into(),commands:vec!["reference.repair".into()],relationships:vec![failure.owner.definition.clone(),failure.owner.node.clone()],data:serde_json::json!({"version":CONTRACT_VERSION,"owner":failure.owner,"configuration":failure.configuration,"source_repair":true}) }];
        elements.extend(failure.inputs.iter().map(|input| SemanticElement { id:format!("repair-input:{}::{}:{}:{}", input.owner.definition,input.owner.node,serde_json::to_string(&input.storage).unwrap_or_default(),input.path),role:"repair-input".into(),label:input.label.clone(),value:None,state:"available".into(),commands:vec!["reference.repair-input".into()],relationships:vec![input.owner.definition.clone(),input.owner.node.clone()],data:serde_json::json!({"version":CONTRACT_VERSION,"input":input,"explicit_replacement":true,"proximity_remap":false}) }));
        elements
    }).collect()
}
