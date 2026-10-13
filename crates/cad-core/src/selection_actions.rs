//! Selection capabilities and command identities, independent of popup widgets and placement.
use crate::{Document, Id, Operation, TopologyRef, presentation::SemanticElement};
use serde::{Deserialize, Serialize};

pub const CONTRACT_VERSION: u32 = 4;

/// Default menu composition hints; action identity and availability remain independent.
/// Custom presentations may promote, reorder, regroup or omit these controls.
pub fn menu_metadata(element: &mut SemanticElement) {
    let Some(command) = element.commands.first() else {
        return;
    };
    let existing = element
        .data
        .get("group")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("More");
    let (group, order, primary) = match command.as_str() {
        "sketch.new" | "sketch.convert-entities" => ("Create", 0, true),
        "sketch.smart-dimension" => ("Dimensions", 0, true),
        "feature.fillet" => ("Modify", 0, true),
        "feature.chamfer" => ("Modify", 1, true),
        "feature.extrude" | "feature.drilled-holes" => ("Create", 1, true),
        "feature.cut" | "feature.shell" => ("Modify", 2, true),
        "feature.move-face" => ("Modify", 3, true),
        "feature.offset-face" => ("Modify", 4, true),
        "feature.edit" | "selection.edit-in-place" => ("Edit", 5, true),
        "sketch.relations" => ("Relations", 6, true),
        "sketch.toggle-construction" => ("Edit", 7, true),
        "sketch.delete-selection" => ("Edit", 8, true),
        "assembly.insert-existing" | "part.new" => ("Component", 0, true),
        "assembly.mate-new" | "selection.mate" => ("Component", 1, true),
        "selection.measure" => ("View", 90, true),
        "selection.fit" | "viewport.fit" => ("View", 91, true),
        "feature.tapped-hole" => ("Hole types", 0, false),
        "feature.counterbore" => ("Hole types", 1, false),
        "feature.countersink" => ("Hole types", 2, false),
        _ => (existing, 50, false),
    };
    let group_order = match group {
        "Create" | "Model" => 0,
        "Modify" | "Edit" | "Feature" => 10,
        "Hole types" => 20,
        "Selection" => 30,
        "Dimensions" | "Relations" => 35,
        "Direct Editing" => 40,
        "Bodies" => 50,
        "Surfaces" => 60,
        "Reference" => 70,
        "Component" => 80,
        "Weld" | "Weldments" => 90,
        "Mold" => 100,
        "View" => 110,
        _ => 120,
    };
    let metadata = serde_json::json!({"group":group,"group_id":format!("selection-group/{group}"),"order":order,"group_order":group_order,"primary":primary});
    element.data["menu"] = metadata;
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SelectionTarget {
    Geometry {
        definition: Id,
        instance: Id,
        #[serde(default)]
        path: Vec<Id>,
        reference: TopologyRef,
        #[serde(default)]
        body: Option<crate::NodeRef>,
        planar: bool,
    },
    Feature {
        definition: Id,
        feature: Id,
    },
    Sketch {
        definition: Id,
        feature: Id,
        sketch: Id,
        entities: Vec<crate::sketch_actions::SketchEntity>,
    },
    Reference {
        definition: Id,
        reference: String,
    },
    Object {
        definition: Id,
        object: Id,
        role: String,
    },
}
impl SelectionTarget {
    pub fn definition(&self) -> &str {
        match self {
            Self::Geometry { definition, .. }
            | Self::Feature { definition, .. }
            | Self::Sketch { definition, .. }
            | Self::Reference { definition, .. }
            | Self::Object { definition, .. } => definition,
        }
    }
    pub fn feature(&self) -> &str {
        match self {
            Self::Geometry { reference, .. } => &reference.producer.node,
            Self::Feature { feature, .. } | Self::Sketch { feature, .. } => feature,
            Self::Reference { reference, .. } => reference,
            Self::Object { object, .. } => object,
        }
    }
}

/// Publish only relevant commands. Unavailable commands carry a human-readable reason.
pub fn project(
    document: &Document,
    target: &SelectionTarget,
    pinned: &[Id],
    editing: bool,
    editable_part: Option<&str>,
) -> Vec<SemanticElement> {
    if let SelectionTarget::Geometry {
        definition,
        body: Some(body),
        ..
    } = target
        && let Some(bead) = document
            .assemblies
            .get(definition)
            .and_then(|assembly| assembly.weld_beads.get(&body.node))
    {
        let reason = editing.then_some("Confirm or cancel the current operation first");
        let suppressed = document
            .active_configurations
            .get(definition)
            .and_then(|id| crate::configuration::resolve(document, definition, id).ok())
            .and_then(|resolved| resolved.overrides.assembly_welds.get(&bead.id).copied())
            .unwrap_or(bead.suppressed);
        return [
            ("assembly.weld-edit", "Edit Weld Bead"),
            (
                "assembly.weld-suppress",
                if suppressed {
                    "Unsuppress Weld Bead"
                } else {
                    "Suppress Weld Bead"
                },
            ),
            ("assembly.weld-delete", "Delete Weld Bead"),
        ]
        .into_iter()
        .map(|(command, label)| descriptor(target, command, label, "Weld", reason))
        .collect();
    }
    match target {
        SelectionTarget::Sketch {
            definition,
            sketch,
            entities,
            ..
        } => {
            return document
                .parts
                .get(definition)
                .and_then(|p| p.sketches.get(sketch))
                .map(|s| sketch_project(document, definition, s, entities, target))
                .unwrap_or_default();
        }
        SelectionTarget::Reference {
            definition,
            reference,
        } => {
            let reason = editing.then_some("Confirm or cancel the current operation first");
            let mut items = vec![
                (
                    "sketch.new",
                    if reference == "Origin" {
                        "New Sketch…"
                    } else {
                        "Sketch on Plane"
                    },
                    "Model",
                    reason,
                ),
                ("viewport.normal-reference", "Normal To", "View", None),
                ("viewport.center-origin", "Center Origin", "View", None),
            ];
            if matches!(
                reference.as_str(),
                "Front Plane" | "Top Plane" | "Right Plane"
            ) {
                let split_reason = reason
                    .or(editable_part
                        .is_some_and(|part| part != definition)
                        .then_some("Use Edit in Place to edit this component"))
                    .or(
                        (!crate::configuration::has_solid_output(document, definition))
                            .then_some("Create a solid body from a sketch first"),
                    )
                    .or(document
                        .libraries
                        .values()
                        .any(|link| link.pinned_definitions.contains_key(definition))
                        .then_some("Make the library component unique before editing it"));
                items.push(("feature.split", "Split With Plane", "Bodies", split_reason));
                items.push((
                    "feature.cut-with-plane",
                    "Cut With Plane",
                    "Bodies",
                    split_reason,
                ));
            }
            return items
                .into_iter()
                .map(|(command, label, group, reason)| {
                    descriptor(target, command, label, group, reason)
                })
                .collect();
        }
        SelectionTarget::Object { role, object, .. } => {
            let reason = editing.then_some("Confirm or cancel the current operation first");
            let items = match role.as_str() {
                "part" => vec![
                    ("sketch.new", "New Sketch…", "Model", reason),
                    ("selection.fit", "Zoom to Part", "View", None),
                    ("materials", "Engineering Material…", "Properties", reason),
                    ("appearances", "Colors and Textures…", "Properties", reason),
                    ("parameter.new", "New Parameter", "Properties", reason),
                    ("configurations", "Configurations", "Properties", reason),
                    ("equations", "Equations", "Properties", reason),
                ],
                "component-instance" => vec![
                    (
                        "selection.edit-in-place",
                        "Edit in Place",
                        "Component",
                        reason,
                    ),
                    ("selection.isolate", "Isolate", "Component", reason),
                    (
                        "selection.pin",
                        if pinned.contains(object) {
                            "Unpin Reference"
                        } else {
                            "Pin Reference"
                        },
                        "Component",
                        None,
                    ),
                    ("assembly.edit", "Edit Component", "Properties", reason),
                    ("appearances", "Colors and Textures…", "Properties", reason),
                    ("selection.mate", "Mate…", "Component", reason),
                ],
                "component-pattern" => vec![
                    (
                        "object.edit",
                        "Edit Component Pattern",
                        "Properties",
                        reason,
                    ),
                    (
                        "object.delete",
                        "Delete Component Pattern",
                        "Properties",
                        reason,
                    ),
                ],
                "assembly-mate" => vec![
                    ("object.edit", "Edit Mate", "Properties", reason),
                    ("object.delete", "Delete Mate", "Properties", reason),
                ],
                "parameter-field" => vec![("object.edit", "Edit Parameter", "Properties", None)],
                "material" => vec![("materials", "Engineering Material…", "Properties", reason)],
                "surface-finish" | "appearance-assignment" | "occurrence-appearance" => {
                    vec![("appearances", "Colors and Textures…", "Properties", reason)]
                }
                "assembly" => vec![
                    (
                        "assembly.inspection-new",
                        "Interference / Clearance…",
                        "Inspect",
                        reason,
                    ),
                    ("assembly.edit", "Edit Assembly", "Properties", reason),
                    ("configurations", "Configurations", "Properties", reason),
                    (
                        "assembly.pattern",
                        "Component Pattern…",
                        "Component",
                        reason,
                    ),
                    ("part.new", "New Part in Assembly", "Component", reason),
                    (
                        "assembly.insert-existing",
                        "Insert Existing Component…",
                        "Component",
                        reason,
                    ),
                    ("assembly.mate-new", "Mate…", "Component", reason),
                    ("viewport.fit", "Zoom to Fit", "View", None),
                ],
                _ => vec![],
            };
            return items
                .into_iter()
                .map(|(command, label, group, reason)| {
                    descriptor(target, command, label, group, reason)
                })
                .collect();
        }
        _ => {}
    }
    let Some(part) = document.parts.get(target.definition()) else {
        return vec![];
    };
    let Some(feature) = part.features.iter().find(|f| f.id == target.feature()) else {
        return vec![];
    };
    let suppressed = crate::configuration::feature_suppressed(document, &part.id, feature);
    if matches!(feature.operation, Operation::Sketch3d { .. }) {
        let busy = if editing {
            Some("Finish or cancel the current operation first")
        } else if editable_part.is_some_and(|id| id != part.id) {
            Some("Use Edit in Place to edit this component")
        } else {
            None
        };
        let reason = busy.or(suppressed.then_some("Unsuppress this 3D sketch first"));
        let mut out = [
            ("feature.edit", "Edit 3D Sketch", "Sketch"),
            ("feature.sweep", "Sweep Along Sketch", "Model"),
            ("feature.curve-pattern", "Curve Driven Pattern", "Model"),
            ("weldment.members", "Structural Member", "Model"),
            ("reference.composite-curve", "Composite Curve", "Reference"),
            ("reference.axis", "Reference Axis", "Reference"),
            ("reference.point", "Reference Point", "Reference"),
        ]
        .into_iter()
        .map(|(command, label, group)| descriptor(target, command, label, group, reason))
        .chain([
            descriptor(target, "selection.fit", "Zoom to Sketch", "View", None),
            descriptor(target, "selection.measure", "Measure", "View", None),
            descriptor(
                target,
                "feature.suppress",
                if suppressed { "Unsuppress" } else { "Suppress" },
                "Feature",
                busy,
            ),
            descriptor(target, "feature.delete", "Delete Feature", "Feature", busy),
        ])
        .collect::<Vec<_>>();
        if matches!(target, SelectionTarget::Feature { .. }) {
            for (command, label, earlier) in [
                ("feature.move-up", "Move Up", true),
                ("feature.move-down", "Move Down", false),
            ] {
                let order_error = crate::feature_order::destination(part, &feature.id, earlier)
                    .err()
                    .map(|error| error.to_string());
                out.push(descriptor(
                    target,
                    command,
                    label,
                    "Feature",
                    busy.or(order_error.as_deref()),
                ));
            }
        }
        return out;
    }
    let mut out = vec![];
    let busy = if editing {
        Some("Confirm or cancel the current operation first")
    } else if editable_part.is_some_and(|id| id != part.id) {
        Some("Use Edit in Place to edit this component")
    } else {
        None
    };
    let body = crate::configuration::has_solid_output(document, &part.id);
    let mut add = |command: &str, label: &str, group: &str, reason: Option<&str>| {
        out.push(descriptor(target, command, label, group, reason));
    };
    let selected_operation = if let SelectionTarget::Geometry {
        body: Some(body), ..
    } = target
    {
        document
            .parts
            .get(&body.definition)
            .and_then(|p| p.features.iter().find(|f| f.id == body.node))
            .map(|f| &f.operation)
            .unwrap_or(&feature.operation)
    } else {
        &feature.operation
    };
    let solid_reason = busy.or(if !selected_operation.produces_solid() {
        Some("Select a solid body for this operation")
    } else if !body {
        Some("Create a solid body from a sketch first")
    } else if suppressed {
        Some("Unsuppress this feature first")
    } else {
        None
    });
    match target {
        SelectionTarget::Geometry {
            reference, planar, ..
        } => {
            match reference.topology.as_str() {
                "plane" => {
                    let split_reason =
                        busy.or((!body).then_some("Create a solid body from a sketch first"));
                    add("feature.split", "Split With Plane", "Bodies", split_reason);
                    add(
                        "feature.cut-with-plane",
                        "Cut With Plane",
                        "Bodies",
                        split_reason,
                    );
                    add("sketch.new", "Sketch on Plane", "Model", busy);
                    add(
                        "reference.plane",
                        "Offset / Angled Plane",
                        "Reference",
                        busy,
                    );
                    add("reference.axis", "Axis Normal to Plane", "Reference", busy);
                    add(
                        "reference.point",
                        "Point at Plane Origin",
                        "Reference",
                        busy,
                    );
                    add("viewport.normal-reference", "Normal To", "View", None);
                }
                "axis" => {
                    add("reference.plane", "Plane Normal to Axis", "Reference", busy);
                    add("reference.point", "Point on Axis", "Reference", busy);
                }
                "point" => {
                    add("reference.plane", "Plane Through Point", "Reference", busy);
                }
                "coordinate_system" => {
                    add("sketch.new", "Sketch on XY Plane", "Model", busy);
                    add(
                        "reference.coordinate-system",
                        "Coordinate System From Reference",
                        "Reference",
                        busy,
                    );
                    add(
                        "viewport.normal-reference",
                        "Normal to XY Plane",
                        "View",
                        None,
                    );
                }
                "edge" => {
                    add(
                        "weld.bead",
                        "Weld Bead",
                        "Weldments",
                        editing.then_some("Confirm or cancel the current operation first"),
                    );
                    add(
                        "mold.parting-surface",
                        "Parting Surface",
                        "Mold",
                        solid_reason,
                    );
                    add("mold.shutoff", "Shut-off Surface", "Mold", solid_reason);
                    add(
                        "reference.axis",
                        "Reference Axis Along Edge",
                        "Reference",
                        busy,
                    );
                    add(
                        "reference.point",
                        "Reference Point on Edge",
                        "Reference",
                        busy,
                    );
                    add("feature.fillet", "Fillet", "Model", solid_reason);
                    add("feature.chamfer", "Chamfer", "Model", solid_reason);
                    add("surface.ruled", "Ruled Surface", "Surfaces", busy);
                }
                "face" => {
                    add(
                        "feature.split",
                        "Split With Plane",
                        "Bodies",
                        solid_reason
                            .or((!planar).then_some("Requires a planar face or datum plane")),
                    );
                    add(
                        "feature.cut-with-plane",
                        "Cut With Plane",
                        "Bodies",
                        solid_reason
                            .or((!planar).then_some("Requires a planar face or datum plane")),
                    );
                    add("surface.mid", "Mid-Surface", "Surfaces", busy);
                    add("feature.keep-bodies", "Keep Body", "Bodies", solid_reason);
                    add(
                        "feature.delete-bodies",
                        "Delete Body",
                        "Bodies",
                        solid_reason,
                    );
                    let hole_reason =
                        solid_reason
                            .or((!*planar)
                                .then_some("Hole positioning sketches require a planar face"));
                    add("feature.drilled-holes", "Hole", "Model", hole_reason);
                    add("feature.tapped-hole", "Tapped Hole", "Model", hole_reason);
                    add("feature.counterbore", "Counterbore", "Model", hole_reason);
                    add("feature.countersink", "Countersink", "Model", hole_reason);
                    add(
                        "reference.face-axis",
                        "Axis From Rotational Face",
                        "Reference",
                        busy.or(planar
                            .then_some("Select a rotational cylindrical/conical or revolved face")),
                    );
                    add(
                        "reference.plane",
                        "Reference Plane on Face",
                        "Reference",
                        busy.or((!planar).then_some("Requires a planar face")),
                    );
                    add(
                        "reference.axis",
                        "Axis Normal to Face",
                        "Reference",
                        busy.or((!planar).then_some("Requires a planar face")),
                    );
                    add(
                        "sketch.new",
                        "Sketch on Face",
                        "Model",
                        busy.or((!planar).then_some("Sketches require a planar face")),
                    );
                    add(
                        "feature.shell",
                        "Shell (remove face)",
                        "Model",
                        solid_reason,
                    );
                    add("feature.draft", "Draft", "Model", solid_reason);
                    add(
                        "feature.move-face",
                        "Move Face",
                        "Direct Editing",
                        solid_reason,
                    );
                    add(
                        "feature.offset-face",
                        "Offset Face",
                        "Direct Editing",
                        solid_reason,
                    );
                    add(
                        "feature.delete-face",
                        "Delete Face / Heal",
                        "Direct Editing",
                        solid_reason,
                    );
                    add(
                        "feature.replace-face",
                        "Replace Face",
                        "Direct Editing",
                        solid_reason,
                    );
                    add(
                        "mold.draft-analysis",
                        "Draft Analysis",
                        "Mold",
                        solid_reason,
                    );
                }
                _ => {}
            }
            if selected_operation.is_surface() {
                add("surface.mid", "Mid-Surface", "Surfaces", busy);
                add("surface.offset", "Offset Surface", "Surfaces", busy);
                add("surface.thicken", "Thicken", "Surfaces", busy);
                add("surface.knit", "Knit Surface", "Surfaces", busy);
                add("surface.reverse", "Reverse Surface", "Surfaces", busy);
                add("surface.trim", "Trim Surface", "Surfaces", busy);
                add(
                    "surface.trim-domain",
                    "Parameter Trim Surface",
                    "Surfaces",
                    busy,
                );
                add("surface.untrim", "Untrim Surface", "Surfaces", busy);
                add("surface.extend", "Extend Surface", "Surfaces", busy);
            }
            add("selection.measure", "Measure", "View", None);
            add("selection.fit", "Zoom to Part", "View", None);
        }
        SelectionTarget::Feature { .. } => {
            if feature.operation.is_reference() {
                if matches!(
                    feature.operation,
                    Operation::ReferencePlane { .. } | Operation::ReferenceCoordinateSystem { .. }
                ) {
                    add(
                        "sketch.new",
                        "Sketch on Plane",
                        "Model",
                        busy.or(suppressed.then_some("Unsuppress this plane first")),
                    );
                    add("viewport.normal-reference", "Normal To", "View", None);
                }
            } else if matches!(feature.operation, Operation::Profile { .. }) {
                add(
                    "sketch.smart-dimension",
                    "Smart Dimension",
                    "Dimensions",
                    busy,
                );
                add("sketch.relations", "Sketch Relations", "Relations", busy);
                let profile_reason = busy.or(suppressed.then_some("Unsuppress this sketch first"));
                for command in [
                    "feature.extrude",
                    "feature.drilled-holes",
                    "feature.tapped-hole",
                    "feature.counterbore",
                    "feature.countersink",
                    "feature.cut",
                    "feature.revolve",
                    "feature.loft",
                    "feature.sweep",
                    "surface.planar",
                    "surface.extrude",
                    "surface.revolve",
                    "surface.loft",
                    "surface.sweep",
                    "surface.fill",
                    "surface.ruled",
                ] {
                    if let Some(tool) = crate::modeling_tools::TOOLS
                        .iter()
                        .find(|t| t.command == command)
                    {
                        add(
                            command,
                            tool.label,
                            "Model",
                            if tool.requires_body {
                                solid_reason
                            } else {
                                profile_reason
                            },
                        );
                    }
                }
            } else if selected_operation.is_surface() {
                add("surface.mid", "Mid-Surface", "Surfaces", busy);
                add("surface.offset", "Offset Surface", "Surfaces", busy);
                add("surface.thicken", "Thicken", "Surfaces", busy);
                add("surface.knit", "Knit Surface", "Surfaces", busy);
                add("surface.reverse", "Reverse Surface", "Surfaces", busy);
                add("surface.trim", "Trim Surface", "Surfaces", busy);
                add(
                    "surface.trim-domain",
                    "Parameter Trim Surface",
                    "Surfaces",
                    busy,
                );
                add("surface.untrim", "Untrim Surface", "Surfaces", busy);
                add("surface.extend", "Extend Surface", "Surfaces", busy);
                add("selection.fit", "Zoom to Part", "View", None);
            } else if selected_operation.is_analysis() {
                add("selection.fit", "Zoom to Part", "View", None);
            } else {
                for command in [
                    "feature.shell",
                    "feature.drilled-holes",
                    "feature.tapped-hole",
                    "feature.counterbore",
                    "feature.countersink",
                    "feature.mirror",
                    "feature.pattern",
                    "feature.circular-pattern",
                    "feature.transform",
                    "feature.scale",
                    "mold.draft-analysis",
                    "mold.shrink",
                    "mold.tooling-split",
                ] {
                    if let Some(tool) = crate::modeling_tools::TOOLS
                        .iter()
                        .find(|t| t.command == command)
                    {
                        add(command, tool.label, "Model", solid_reason);
                    }
                }
                add("selection.fit", "Zoom to Part", "View", None);
            }
        }
        _ => unreachable!(),
    }
    add(
        "feature.edit",
        if matches!(feature.operation, Operation::Profile { .. }) {
            "Edit Sketch"
        } else {
            "Edit Feature"
        },
        "Feature",
        busy,
    );
    if matches!(target, SelectionTarget::Feature { .. }) {
        for (command, label, earlier) in [
            ("feature.move-up", "Move Up", true),
            ("feature.move-down", "Move Down", false),
        ] {
            let reason = crate::feature_order::destination(part, &feature.id, earlier)
                .err()
                .map(|e| e.to_string());
            add(command, label, "Feature", busy.or(reason.as_deref()));
        }
        add(
            "feature.suppress",
            if suppressed { "Unsuppress" } else { "Suppress" },
            "Feature",
            busy,
        );
        add("feature.delete", "Delete Feature", "Feature", busy);
    }
    if let SelectionTarget::Geometry { instance, .. } = target
        && document
            .assemblies
            .values()
            .any(|a| a.instances.contains_key(instance))
    {
        let component_reason = editing.then_some("Confirm or cancel the current operation first");
        add(
            "selection.edit-in-place",
            "Edit in Place",
            "Component",
            component_reason,
        );
        add(
            "selection.isolate",
            "Isolate",
            "Component",
            component_reason,
        );
        add(
            "selection.pin",
            if pinned.contains(instance) {
                "Unpin Reference"
            } else {
                "Pin Reference"
            },
            "Component",
            None,
        );
        add("selection.mate", "Mate…", "Component", component_reason);
        add(
            "assembly.move",
            "Move Component",
            "Component",
            component_reason,
        );
        add(
            "assembly.rotate",
            "Rotate Component",
            "Component",
            component_reason,
        );
        add(
            "assembly.group-new",
            "Rigid Group…",
            "Component",
            component_reason,
        );
        add(
            "assembly.copy-with-mates",
            "Copy With Mates…",
            "Component",
            component_reason,
        );
    }
    out
}

fn descriptor(
    target: &SelectionTarget,
    command: &str,
    label: &str,
    group: &str,
    reason: Option<&str>,
) -> SemanticElement {
    let mut element = SemanticElement {
        id: format!("selection:{command}"),
        role: "selection-action".into(),
        label: label.into(),
        value: reason.map(str::to_string),
        state: if reason.is_some() {
            "disabled"
        } else {
            "ready"
        }
        .into(),
        commands: vec![command.into()],
        relationships: vec![target.definition().into(), target.feature().into()],
        data: serde_json::json!({"contract_version": CONTRACT_VERSION, "target": target, "group": group}),
    };
    menu_metadata(&mut element);
    element
}
pub fn sketch_project(
    document: &Document,
    definition: &str,
    sketch: &crate::Sketch,
    entities: &[crate::sketch_actions::SketchEntity],
    target: &SelectionTarget,
) -> Vec<SemanticElement> {
    use crate::sketch_actions as a;
    let mut out = vec![];
    if !crate::sketch_dimensions::selected_points(sketch, entities).is_empty() {
        for (command, label) in [
            ("sketch.ordinate-dimension", "Ordinate Dimensions"),
            ("sketch.baseline-dimension", "Baseline Dimensions"),
            ("sketch.chain-dimension", "Chain Dimensions"),
        ] {
            out.push(descriptor(target, command, label, "Dimension Groups", None));
        }
    }
    if crate::sketch_dimensions::selected_group(sketch, entities).is_some() {
        out.push(descriptor(
            target,
            "sketch.edit-dimension-group",
            "Edit Dimension Group",
            "Dimension Groups",
            None,
        ));
        out.push(descriptor(
            target,
            "sketch.delete-dimension-group",
            "Delete Dimension Group",
            "Dimension Groups",
            None,
        ));
    }
    let dimension = if entities.is_empty() {
        None
    } else {
        a::dimensions(document, definition, sketch, entities)
            .err()
            .map(|e| e.to_string())
    };
    out.push(descriptor(
        target,
        "sketch.smart-dimension",
        "Smart Dimension",
        "Dimensions",
        dimension.as_deref(),
    ));
    out.push(descriptor(
        target,
        "sketch.reference-dimension",
        "Reference Dimension",
        "Dimensions",
        dimension.as_deref(),
    ));
    out.push(descriptor(
        target,
        "sketch.repair",
        "Repair Sketch…",
        "Edit",
        None,
    ));
    out.push(descriptor(
        target,
        "sketch.text",
        "Sketch Text…",
        "Geometry",
        None,
    ));
    out.push(descriptor(
        target,
        "sketch.equation",
        "Equation Curve…",
        "Curves",
        None,
    ));
    if entities.iter().any(|e|matches!(e,a::SketchEntity::Constraint{constraint} if sketch.constraints.iter().any(|c|&c.id==constraint && a::is_dimension(&c.kind)))) {
        out.push(descriptor(target,"sketch.toggle-dimension-mode","Toggle Driving / Reference","Dimensions",None));
    }
    for (kind, label) in a::RELATIONS {
        let availability = a::relation(sketch, entities, kind);
        if availability.is_ok() || *kind == "tangent" {
            let reason = availability.as_ref().err().map(ToString::to_string);
            let mut action = descriptor(
                target,
                &format!("sketch.relation.{kind}"),
                label,
                "Relations",
                reason.as_deref(),
            );
            action.data["description"] = serde_json::json!(a::relation_description(kind));
            action.data["selection_count"] = serde_json::json!(entities.len());
            out.push(action);
        }
    }
    let members = entities
        .iter()
        .filter_map(|e| {
            if let a::SketchEntity::Member { member } = e {
                Some(member.clone())
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    let create_reason = members
        .is_empty()
        .then_some("Select independent sketch members")
        .or_else(|| {
            members
                .iter()
                .any(|member| crate::sketch_edit::driven_member(sketch, member))
                .then_some("Detach generated geometry before creating a local block")
        });
    out.push(descriptor(
        target,
        "sketch.block-create",
        "Create Block…",
        "Block",
        create_reason,
    ));
    out.push(descriptor(
        target,
        "sketch.block-insert",
        "Insert Block…",
        "Block",
        sketch
            .blocks
            .is_empty()
            .then_some("Create a saved local block definition first"),
    ));
    out.push(descriptor(
        target,
        "sketch.block-import",
        "Insert Block From File…",
        "Block",
        None,
    ));
    if members.iter().any(|member|crate::sketch_blocks::instance_for_member(sketch,member).is_some()) || entities.iter().any(|entity|matches!(entity,a::SketchEntity::Point{point} if sketch.block_instances.values().any(|instance|&instance.anchor==point))) {
        for(command,label)in [("sketch.block-edit","Edit Block Definition…"),("sketch.block-instance","Block Instance Properties…"),("sketch.block-explode","Explode Block"),("sketch.block-export","Export Block…")] {out.push(descriptor(target,command,label,"Block",None));}
    }
    let corner = entities.iter().find_map(|e| {
        if let a::SketchEntity::Point { point } = e {
            Some(point.as_str())
        } else {
            None
        }
    });
    if members.iter().any(|member|crate::sketch_text::operation_for(sketch,member).is_some()) || entities.iter().any(|entity|matches!(entity,a::SketchEntity::Point{point} if sketch.texts.values().any(|definition|&definition.anchor==point))) {
        out.push(descriptor(target,"sketch.edit-text","Edit Sketch Text…","Edit",None));
        if !members.is_empty(){out.push(descriptor(target,"sketch.detach-conversion","Detach Text Outlines","Edit",None));}
    }
    if members.iter().any(|member|sketch.equations.contains_key(member)) || entities.iter().any(|entity|matches!(entity,a::SketchEntity::Point{point} if sketch.equations.values().any(|definition|&definition.anchor==point))) {
        out.push(descriptor(target,"sketch.edit-equation","Edit Equation Curve…","Edit",None));
        if !members.is_empty(){out.push(descriptor(target,"sketch.detach-conversion","Detach Equation Curve","Edit",None));}
    }
    let offset_edit = sketch
        .offsets
        .values()
        .any(|o| o.members.iter().any(|m| members.contains(&m.member)));
    let fillet_edit = sketch.fillets.values().any(|f| members.contains(&f.arc));
    let chamfer_edit = sketch
        .chamfers
        .values()
        .any(|c| members.contains(&c.member));
    let offset_reason = (!offset_edit)
        .then(|| {
            crate::sketch_edit::offset_selection(sketch, &members)
                .err()
                .map(|e| e.to_string())
        })
        .flatten();
    let fillet_reason = (!fillet_edit)
        .then(|| {
            crate::sketch_edit::fillet_selection(sketch, &members, corner)
                .err()
                .map(|e| e.to_string())
        })
        .flatten();
    out.push(descriptor(
        target,
        "sketch.convert-entities",
        "Convert Entities…",
        "Edit",
        None,
    ));
    out.push(descriptor(
        target,
        "sketch.offset-entities",
        if offset_edit {
            "Edit Offset…"
        } else {
            "Offset Entities…"
        },
        "Edit",
        offset_reason.as_deref(),
    ));
    out.push(descriptor(
        target,
        "sketch.fillet",
        if fillet_edit {
            "Edit Sketch Fillet…"
        } else {
            "Sketch Fillet…"
        },
        "Edit",
        fillet_reason.as_deref(),
    ));
    out.push(descriptor(
        target,
        "sketch.chamfer",
        if chamfer_edit {
            "Edit Sketch Chamfer…"
        } else {
            "Sketch Chamfer…"
        },
        "Edit",
        if chamfer_edit {
            None
        } else {
            fillet_reason.as_deref().or_else(|| {
                members
                    .iter()
                    .any(|key| {
                        !matches!(
                            sketch.members.get(key).map(|m| &m.curve),
                            Some(crate::SketchCurve::Line { .. })
                        )
                    })
                    .then_some("Sketch Chamfer requires joined straight members")
            })
        },
    ));
    for (command, label, kind) in [
        (
            "sketch.trim",
            "Trim Entities…",
            crate::sketch_trim::Kind::Trim,
        ),
        (
            "sketch.extend",
            "Extend Entities…",
            crate::sketch_trim::Kind::Extend,
        ),
        (
            "sketch.split",
            "Split Entities…",
            crate::sketch_trim::Kind::Split,
        ),
    ] {
        let reason = if members.len() == 1 {
            crate::sketch_trim::editable(sketch, &members[0], kind)
                .err()
                .map(|e| e.to_string())
        } else {
            Some("Select one sketch member".into())
        };
        out.push(descriptor(
            target,
            command,
            label,
            "Edit",
            reason.as_deref(),
        ));
    }
    let spline_reason = if members.len() == 1 {
        crate::sketch_spline::editable(sketch, &members[0])
            .err()
            .map(|e| e.to_string())
    } else {
        Some("Select one spline".into())
    };
    out.push(descriptor(
        target,
        "sketch.edit-spline",
        "Edit Spline…",
        "Edit",
        spline_reason.as_deref(),
    ));
    if members
        .iter()
        .any(|member| sketch.splines.contains_key(member))
    {
        out.push(descriptor(
            target,
            "sketch.detach-conversion",
            "Detach Spline Intent",
            "Edit",
            None,
        ));
    }
    if members.len() == 1 && crate::sketch_conic::operation_for(sketch, &members[0]).is_some() {
        out.push(descriptor(
            target,
            "sketch.edit-conic",
            "Edit Conic / Arc Slot…",
            "Edit",
            None,
        ));
        out.push(descriptor(
            target,
            "sketch.detach-conversion",
            "Detach Conic / Arc Slot Intent",
            "Edit",
            None,
        ));
    }
    for (command, label, group) in [
        ("sketch.move-copy", "Move/Copy Entities…", "Transform"),
        ("sketch.rotate", "Rotate Entities…", "Transform"),
        ("sketch.scale", "Scale Entities…", "Transform"),
        ("sketch.mirror", "Mirror Entities…", "Pattern"),
        (
            "sketch.dynamic-mirror",
            "Dynamic Mirror Entities…",
            "Pattern",
        ),
        ("sketch.linear-pattern", "Linear Sketch Pattern…", "Pattern"),
        (
            "sketch.circular-pattern",
            "Circular Sketch Pattern…",
            "Pattern",
        ),
    ] {
        out.push(descriptor(
            target,
            command,
            label,
            group,
            members.is_empty().then_some("Select sketch members"),
        ));
    }
    out.push(descriptor(
        target,
        "sketch.stretch",
        "Stretch Source Controls…",
        "Transform",
        (!entities.iter().any(|entity| {
            matches!(
                entity,
                a::SketchEntity::Point { .. } | a::SketchEntity::Member { .. }
            )
        }))
        .then_some("Select source controls or members"),
    ));
    out.push(descriptor(
        target,
        "sketch.replace-entities",
        "Replace Entities…",
        "Edit",
        members.is_empty().then_some("Select the member to replace"),
    ));
    if members
        .iter()
        .any(|member| crate::sketch_transform::instance_for_member(sketch, member).is_some())
    {
        out.push(descriptor(
            target,
            "sketch.instance-controls",
            "Control Instance (Suppress / Placement)…",
            "Pattern",
            None,
        ));
    }
    if sketch.transforms.values().any(|pattern| pattern.dynamic) {
        out.push(descriptor(
            target,
            "sketch.stop-dynamic-mirror",
            "Stop Dynamic Mirror",
            "Pattern",
            None,
        ));
    }
    if sketch.transforms.values().any(|p| {
        crate::sketch_transform::outputs(p)
            .iter()
            .any(|m| members.contains(m))
    }) {
        out.push(descriptor(
            target,
            "sketch.detach-conversion",
            "Detach Pattern/Copy Relation",
            "Edit",
            None,
        ));
    }
    if entities
        .iter()
        .any(|e| matches!(e, a::SketchEntity::Member { .. }))
    {
        out.push(descriptor(
            target,
            "sketch.toggle-construction",
            "Toggle Construction",
            "Edit",
            None,
        ));
    }
    if members.iter().any(|id| sketch.conversions.contains_key(id)) {
        out.push(descriptor(
            target,
            "sketch.detach-conversion",
            "Detach Source Relation",
            "Edit",
            None,
        ));
    }
    if entities.iter().any(|e| {
        matches!(
            e,
            a::SketchEntity::Member { .. }
                | a::SketchEntity::Point { .. }
                | a::SketchEntity::Constraint { .. }
        )
    }) {
        out.push(descriptor(
            target,
            "sketch.delete-selection",
            "Delete Selection",
            "Edit",
            None,
        ));
    }
    if entities.is_empty() {
        for (command, label, group) in crate::modeling_tools::SKETCH_TOOLS {
            if matches!(*group, "Constraints" | "Edit") || *command == "sketch.select" {
                continue;
            }
            out.push(descriptor(target, command, label, "Draw", None));
        }
    }
    out.push(descriptor(
        target,
        "sketch.select",
        "Select / Multi-select",
        "View",
        None,
    ));
    out.push(descriptor(
        target,
        "sketch.handles",
        "Drag Handles",
        "Edit",
        None,
    ));
    out.push(descriptor(
        target,
        "sketch.normal-to",
        "Normal To Sketch",
        "View",
        None,
    ));
    out.push(descriptor(
        target,
        "sketch.finish",
        "Apply Sketch",
        "Operation",
        None,
    ));
    out.push(descriptor(
        target,
        "sketch.cancel",
        "Cancel Sketch",
        "Operation",
        None,
    ));
    out
}

/// A model edge or face boundary can be converted into the current sketch.
pub fn convert_action(target: &SelectionTarget, reason: Option<&str>) -> SemanticElement {
    descriptor(
        target,
        "sketch.convert-entities",
        "Convert Entities…",
        "Sketch",
        reason,
    )
}
