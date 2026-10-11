//! Application-owned extrusion presentation and intent contract.
use crate::{
    Dimension, Document, Expr, ExtrudeEnd, ExtrudeOptions, Id, NodeRef, Operation, TopologyRef,
    presentation::SemanticElement,
};
use serde::{Deserialize, Serialize};

pub const CONTRACT_VERSION: u32 = 3;

/// A source reference, independent of evaluated geometry or widget state.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Start {
    #[default]
    SketchPlane,
    Face {
        face: Box<TopologyRef>,
        body: NodeRef,
    },
    Vertex {
        profile: NodeRef,
        point: Id,
    },
    Point {
        point: Box<crate::reference_geometry::PointSource>,
    },
}
impl Start {
    pub fn is_sketch_plane(&self) -> bool {
        matches!(self, Self::SketchPlane)
    }
    pub fn dependencies(&self) -> Vec<NodeRef> {
        match self {
            Self::SketchPlane => vec![],
            Self::Face { body, .. } => vec![body.clone()],
            Self::Vertex { profile, .. } => vec![profile.clone()],
            Self::Point { point } => point.dependencies(),
        }
    }
    pub fn expressions(&self) -> Vec<&Expr> {
        match self {
            Self::Point { point } => point.expressions(),
            _ => vec![],
        }
    }
}

/// Independently authored opposite-direction extent. Older reverse_distance
/// definitions remain readable and are resolved as a blind direction two.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DirectionEnd {
    #[serde(default)]
    pub end: ExtrudeEnd,
    pub distance: Expr,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<TopologyRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_body: Option<NodeRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_vertex: Option<(NodeRef, Id)>,
    #[serde(default = "zero")]
    pub end_offset: Expr,
}
fn zero() -> Expr {
    "0 mm".into()
}
impl Default for DirectionEnd {
    fn default() -> Self {
        Self {
            end: ExtrudeEnd::Blind,
            distance: "10 mm".into(),
            target: None,
            target_body: None,
            target_vertex: None,
            end_offset: zero(),
        }
    }
}
impl DirectionEnd {
    pub fn dependencies(&self) -> Vec<NodeRef> {
        let mut dependencies: Vec<_> = self
            .target_body
            .clone()
            .or_else(|| self.target.as_ref().map(|target| target.producer.clone()))
            .into_iter()
            .collect();
        dependencies.extend(
            self.target_vertex
                .as_ref()
                .map(|(profile, _)| profile.clone()),
        );
        dependencies
    }
    pub fn expressions(&self) -> Vec<&Expr> {
        let mut values = vec![&self.end_offset];
        if self.end == ExtrudeEnd::Blind {
            values.push(&self.distance);
        }
        values
    }
}
pub fn dependencies(options: &ExtrudeOptions) -> Vec<NodeRef> {
    let mut dependencies = options.start.dependencies();
    dependencies.extend(options.scope.iter().cloned());
    dependencies.extend(
        options
            .contours
            .iter()
            .map(|selection| selection.profile.clone()),
    );
    dependencies.extend(
        options
            .direction2
            .iter()
            .flat_map(DirectionEnd::dependencies),
    );
    dependencies
}
pub fn expressions(options: &ExtrudeOptions) -> Vec<&Expr> {
    options
        .start
        .expressions()
        .into_iter()
        .chain(
            options
                .direction2
                .iter()
                .flat_map(DirectionEnd::expressions),
        )
        .collect()
}

pub fn semantic_fields(
    doc: &Document,
    owner: &str,
    node: &str,
    operation: &Operation,
    state: &str,
) -> Vec<SemanticElement> {
    let Operation::Extrude {
        distance,
        direction,
        profile,
        body,
        mode,
        options,
    } = operation
    else {
        return section_semantics(doc, owner, node, operation, state);
    };
    let mut out = Vec::new();
    let mut quantity = |key: &str, label: &str, expression: &Expr, dimension: Dimension| {
        out.push(SemanticElement { id: format!("{node}::extrusion::{key}"), role: "feature-field".into(),
            label: label.into(), value: Some(expression.0.clone()), state: state.into(),
            commands: vec!["feature.edit".into()], relationships: vec![node.into()],
            data: serde_json::json!({"contract":"tcad.extrusion","version":CONTRACT_VERSION,"field":key,"type":"quantity","dimension":dimension,"expression":expression,"evaluated":crate::expression::value(doc,owner,expression,dimension).ok(),"editable":true}) });
    };
    quantity("distance", "Extrusion Depth", distance, Dimension::Length);
    quantity(
        "start_offset",
        "Start Offset",
        &options.start_offset,
        Dimension::Length,
    );
    quantity(
        "end_offset",
        "End Offset",
        &options.end_offset,
        Dimension::Length,
    );
    if let Some(reverse) = &options.reverse_distance {
        quantity(
            "reverse_distance",
            "Direction 2 Depth",
            reverse,
            Dimension::Length,
        );
    }
    if let Some(end) = &options.direction2 {
        if end.end == ExtrudeEnd::Blind {
            quantity(
                "direction2.distance",
                "Direction 2 Depth",
                &end.distance,
                Dimension::Length,
            );
        }
        quantity(
            "direction2.end_offset",
            "Direction 2 End Offset",
            &end.end_offset,
            Dimension::Length,
        );
    }
    if let Some(thin) = &options.thin {
        quantity(
            "thin.thickness",
            "Thin Feature Thickness",
            &thin.thickness,
            Dimension::Length,
        );
        if let Some(second) = &thin.second_thickness {
            quantity(
                "thin.second_thickness",
                "Side 2 Thickness",
                second,
                Dimension::Length,
            );
        }
    }
    if let Some(draft) = &options.draft {
        quantity(
            "draft.angle",
            "Extrusion Draft Angle",
            &draft.angle,
            Dimension::Angle,
        );
    }
    out.push(SemanticElement { id: format!("{node}::extrusion::options"), role: "feature-options".into(),
        label: "Extrusion Options".into(), value: None, state: state.into(),
        commands: vec!["feature.edit".into()], relationships: vec![node.into(),profile.node.clone()],
        data: serde_json::json!({"contract":"tcad.extrusion","version":CONTRACT_VERSION,"profile":profile,"direction":direction,"body":body,"result":mode,"options":options,"contours":crate::contours::semantics(&options.contours,options.thin.is_some()),"choices":choices(options)}) });
    out
}

fn section_semantics(
    doc: &Document,
    owner: &str,
    node: &str,
    operation: &Operation,
    state: &str,
) -> Vec<SemanticElement> {
    let capability = match operation {
        Operation::Loft { options, .. } => {
            serde_json::json!({"operation":"loft","representation":"exact_rational_ruled_or_cubic_brep","holes":true,"multiple_material_regions":true,"region_correspondence":"selected_source_record_order_or_legacy_descending_area_with_source_closure_provenance","contour_correspondence":"compatible_rational_bases_and_member_counts","connectors":"persistent_native_region_members_and_sketch_vertices_with_explicit_legacy_ordinals","section_planes":"parallel_with_strict_axial_progress","separation_proof":["affine_family","convex_polygon_family","concentric_circle_family"],"endpoint_control":["default","profile_normal","reference_direction"],"thin_modes":["one_side","two_side","mid_plane"],"scope":"explicit_feature_outputs","contours":crate::contours::semantics(&options.contours,options.thin.is_some()),"options":options,"guide_curves":false})
        }
        Operation::Sweep { options, .. } => {
            serde_json::json!({"operation":"sweep","representation":"exact_tensor_rational_or_revolution_brep","holes":true,"multiple_material_regions":true,"orientation":["fixed","follow_tangent"],"fixed_path":["line_chain","positive_weight_spline_chain"],"path_progress":"proved_monotonic_along_section_normal","tangent_path":["collinear_chain","native_arc","native_circle"],"placement":["keep_source","at_path_start"],"path_range":"normalized_curve_parameter_equal_spans_per_chain_member","path_start":"native_member_declared_start_or_persistent_chain_endpoint","twist_law":"piecewise_rational_half_angle_per_retained_bezier_span","tangent_circular_twist":"zero","roll":true,"thin_modes":["one_side","two_side","mid_plane"],"scope":"explicit_feature_outputs","contours":crate::contours::semantics(&options.contours,options.thin.is_some()),"options":options,"guide_curves":false})
        }
        _ => return vec![],
    };
    let mut out = vec![SemanticElement {
        id: format!("{node}::sections::capabilities"),
        role: "feature-capability".into(),
        label: "Section Feature Geometry".into(),
        value: None,
        state: state.into(),
        commands: vec!["feature.edit".into()],
        relationships: operation
            .dependencies()
            .iter()
            .map(|r| r.node.clone())
            .collect(),
        data: serde_json::json!({"contract":"tcad.solid_sections","version":crate::solid_sections::CONTRACT_VERSION,"capability":capability}),
    }];
    let mut quantity = |field: &str, label: &str, value: &Expr, dimension: Dimension| {
        out.push(SemanticElement {id:format!("{node}::sections::{field}"),role:"feature-field".into(),label:label.into(),value:Some(value.0.clone()),state:state.into(),commands:vec!["feature.edit".into()],relationships:vec![node.into()],data:serde_json::json!({"contract":"tcad.solid_sections","version":crate::solid_sections::CONTRACT_VERSION,"field":field,"type":"quantity","dimension":dimension,"expression":value,"evaluated":crate::expression::value(doc,owner,value,dimension).ok(),"editable":true})})
    };
    let thin = match operation {
        Operation::Loft { options, .. } => {
            for (field, label, end) in [
                ("start.influence", "Start Tangent Influence", &options.start),
                ("end.influence", "End Tangent Influence", &options.end),
            ] {
                if let crate::solid_sections::End::Normal { influence }
                | crate::solid_sections::End::Direction { influence, .. } = end
                {
                    quantity(field, label, influence, Dimension::Length);
                }
            }
            options.thin.as_ref()
        }
        Operation::Sweep { options, .. } => {
            quantity(
                "start",
                "Start Parameter",
                &options.start,
                Dimension::Scalar,
            );
            quantity("end", "End Parameter", &options.end, Dimension::Scalar);
            quantity("roll", "Initial Roll", &options.roll, Dimension::Angle);
            quantity("twist", "Total Twist", &options.twist, Dimension::Angle);
            options.thin.as_ref()
        }
        _ => None,
    };
    if let Some(thin) = thin {
        quantity(
            "thin.thickness",
            "Thin Feature Thickness",
            &thin.thickness,
            Dimension::Length,
        );
        if let Some(second) = &thin.second_thickness {
            quantity(
                "thin.second_thickness",
                "Side 2 Thickness",
                second,
                Dimension::Length,
            );
        }
    }
    out
}

fn choices(options: &ExtrudeOptions) -> serde_json::Value {
    serde_json::json!({"direction":["sketch_normal","specified_vector"],"start":["sketch_plane","face","vertex","point"],"start_offset_direction":"extrusion_axis","start_point":"associative_point_source_projection_along_axis","edge_point_parameter":"native_constructor_oriented_parameter_0_to_1","end":["blind","mid_plane","through_all","through_all_both","up_to_face","up_to_vertex","up_to_next","up_to_body"],"direction2_end":["blind","through_all","up_to_face","up_to_vertex","up_to_next","up_to_body"],"independent_ends":true,"scope":"explicit_feature_outputs_all_contained_bodies","end_face_geometry":["planar_carrier","bounded_separable_positive_rational_graph_patch"],"curved_end_section":"one_rectangle_normal_to_axis","curved_end_domain":"full_four_sided_patch_without_holes_or_existing_trims","end_offset_direction":"end_face_normal","curved_end_offset":"zero_or_authored_exact_offset_surface","next_body_termination":"one_complete_nearest_forward_bounded_face","end_vertex_geometry":"sketch_point","thin_modes":["one_side","two_side","mid_plane"],"thin_enabled":options.thin.is_some(),"draft_enabled":options.draft.is_some(),"thin_geometry":["line","arc","circle"],"thin_open_ends":"flat","draft_geometry":["line"],"draft_neutral":"sketch_plane"})
}
