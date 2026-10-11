//! Versioned, application-owned surface modeling intent. Evaluated B-reps are adapter data.
use crate::{
    Expr, NodeRef, TopologyRef, reference_curves::Segment, reference_geometry::DirectionSource,
};
use serde::{Deserialize, Serialize};

pub const CONTRACT_VERSION: u32 = 3;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct FaceSource {
    pub body: NodeRef,
    pub reference: TopologyRef,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct PatchCorrespondence {
    #[serde(default)]
    pub swap_uv: bool,
    #[serde(default)]
    pub reverse_u: bool,
    #[serde(default)]
    pub reverse_v: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SurfaceOperation {
    Planar {
        profile: NodeRef,
    },
    Ruled {
        sources: Vec<Segment>,
        direction: Box<DirectionSource>,
        distance: Expr,
        #[serde(default)]
        reverse: bool,
    },
    MidSurface {
        faces: Box<[FaceSource; 2]>,
        fraction: Expr,
        #[serde(default)]
        correspondence: PatchCorrespondence,
        #[serde(default)]
        reverse: bool,
    },
    Extrude {
        profile: NodeRef,
        distance: Expr,
        direction: [f64; 3],
        #[serde(default)]
        mid_plane: bool,
        #[serde(default)]
        profile_normal: bool,
        #[serde(default)]
        reverse: bool,
    },
    Revolve {
        profile: NodeRef,
        origin: [f64; 3],
        axis: [f64; 3],
        angle: Expr,
        #[serde(default)]
        axis_reference: Option<crate::reference_geometry::AxisSource>,
    },
    Loft {
        profiles: Vec<NodeRef>,
    },
    Sweep {
        profile: NodeRef,
        path: Vec<[f64; 3]>,
        #[serde(default)]
        path_sketch: Option<NodeRef>,
    },
    /// Four ordered, end-connected explicit sketch curves. Euclidean Coons
    /// blending retains polynomial or positive-weight rational boundaries;
    /// unsupported carriers and unproven folded patches fail explicitly.
    Fill {
        boundaries: [NodeRef; 4],
        #[serde(default)]
        reverse: bool,
    },
    Trim {
        surface: NodeRef,
        profile: NodeRef,
        #[serde(default)]
        keep_inside: bool,
    },
    TrimDomain {
        surface: NodeRef,
        u: [Expr; 2],
        v: [Expr; 2],
    },
    Untrim {
        surface: NodeRef,
    },
    Extend {
        surface: NodeRef,
        distance: Expr,
        #[serde(default)]
        edges: Vec<crate::TopologyRef>,
    },
    Offset {
        surface: NodeRef,
        distance: Expr,
    },
    Reverse {
        surface: NodeRef,
    },
    Knit {
        surfaces: Vec<NodeRef>,
        tolerance: Expr,
        #[serde(default)]
        make_solid: bool,
        #[serde(default)]
        reverse: bool,
    },
    Thicken {
        surface: NodeRef,
        thickness: Expr,
        #[serde(default)]
        mid_plane: bool,
        #[serde(default)]
        reverse: bool,
    },
}
impl SurfaceOperation {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Planar { .. } => "Planar Surface",
            Self::Ruled { .. } => "Ruled Surface",
            Self::MidSurface { .. } => "Mid-Surface",
            Self::Extrude { .. } => "Extruded Surface",
            Self::Revolve { .. } => "Revolved Surface",
            Self::Loft { .. } => "Lofted Surface",
            Self::Sweep { .. } => "Swept Surface",
            Self::Fill { .. } => "Filled Surface",
            Self::Trim { .. } | Self::TrimDomain { .. } => "Trim Surface",
            Self::Untrim { .. } => "Untrim Surface",
            Self::Extend { .. } => "Extend Surface",
            Self::Offset { .. } => "Offset Surface",
            Self::Reverse { .. } => "Reverse Surface",
            Self::Knit { .. } => "Knit Surface",
            Self::Thicken { .. } => "Thicken",
        }
    }
    pub fn expressions(&self) -> Vec<&Expr> {
        match self {
            Self::Ruled {
                direction,
                distance,
                ..
            } => direction
                .expressions()
                .into_iter()
                .chain([distance])
                .collect(),
            Self::MidSurface { fraction, .. } => vec![fraction],
            Self::Extrude { distance, .. }
            | Self::Offset { distance, .. }
            | Self::Extend { distance, .. } => vec![distance],
            Self::TrimDomain { u, v, .. } => u.iter().chain(v).collect(),
            Self::Revolve { angle, .. } => vec![angle],
            Self::Thicken { thickness, .. } => vec![thickness],
            Self::Knit { tolerance, .. } => vec![tolerance],
            _ => vec![],
        }
    }
    pub fn dependencies(&self) -> Vec<NodeRef> {
        match self {
            Self::Ruled {
                sources, direction, ..
            } => sources
                .iter()
                .map(|source| source.source.dependency().clone())
                .chain(direction.dependencies())
                .collect(),
            Self::MidSurface { faces, .. } => faces.iter().map(|face| face.body.clone()).collect(),
            Self::Planar { profile } | Self::Extrude { profile, .. } => vec![profile.clone()],
            Self::Revolve {
                profile,
                axis_reference,
                ..
            } => std::iter::once(profile.clone())
                .chain(axis_reference.iter().flat_map(|a| a.dependencies()))
                .collect(),
            Self::Loft { profiles } => profiles.clone(),
            Self::Fill { boundaries, .. } => boundaries.to_vec(),
            Self::Trim {
                surface, profile, ..
            } => vec![surface.clone(), profile.clone()],
            Self::Sweep {
                profile,
                path_sketch,
                ..
            } => std::iter::once(profile.clone())
                .chain(path_sketch.clone())
                .collect(),
            Self::Offset { surface, .. }
            | Self::TrimDomain { surface, .. }
            | Self::Untrim { surface }
            | Self::Extend { surface, .. }
            | Self::Reverse { surface }
            | Self::Thicken { surface, .. } => vec![surface.clone()],
            Self::Knit { surfaces, .. } => surfaces.clone(),
        }
    }
    pub fn consumed_surfaces(&self) -> Vec<NodeRef> {
        match self {
            Self::Thicken { surface, .. }
            | Self::Reverse { surface }
            | Self::Trim { surface, .. }
            | Self::TrimDomain { surface, .. }
            | Self::Untrim { surface }
            | Self::Extend { surface, .. } => vec![surface.clone()],
            Self::Knit { surfaces, .. } => surfaces.clone(),
            _ => vec![],
        }
    }
    pub fn is_surface(&self) -> bool {
        !matches!(
            self,
            Self::Thicken { .. }
                | Self::Knit {
                    make_solid: true,
                    ..
                }
        )
    }
}

/// Typed intent fields let a presentation bind controls without inspecting kernel objects.
pub fn semantic_fields(
    doc: &crate::Document,
    owner: &str,
    node: &str,
    operation: &SurfaceOperation,
    state: &str,
) -> Vec<crate::presentation::SemanticElement> {
    use crate::{Dimension, presentation::SemanticElement};
    let quantities: Vec<(&str, &str, &Expr, Dimension)> = match operation {
        SurfaceOperation::Ruled { distance, .. } => {
            vec![("distance", "Signed Extension", distance, Dimension::Length)]
        }
        SurfaceOperation::MidSurface { fraction, .. } => vec![(
            "fraction",
            "Position Between Faces",
            fraction,
            Dimension::Scalar,
        )],
        SurfaceOperation::Extrude { distance, .. } => {
            vec![("distance", "Depth", distance, Dimension::Length)]
        }
        SurfaceOperation::Revolve { angle, .. } => {
            vec![("angle", "Angle", angle, Dimension::Angle)]
        }
        SurfaceOperation::Offset { distance, .. } => {
            vec![("distance", "Offset", distance, Dimension::Length)]
        }
        SurfaceOperation::Extend { distance, .. } => {
            vec![("distance", "Extension", distance, Dimension::Length)]
        }
        SurfaceOperation::TrimDomain { u, v, .. } => vec![
            ("u_start", "U Start", &u[0], Dimension::Scalar),
            ("u_end", "U End", &u[1], Dimension::Scalar),
            ("v_start", "V Start", &v[0], Dimension::Scalar),
            ("v_end", "V End", &v[1], Dimension::Scalar),
        ],
        SurfaceOperation::Knit { tolerance, .. } => {
            vec![("tolerance", "Knit Tolerance", tolerance, Dimension::Length)]
        }
        SurfaceOperation::Thicken { thickness, .. } => {
            vec![("thickness", "Thickness", thickness, Dimension::Length)]
        }
        _ => vec![],
    };
    let mut out: Vec<_> = quantities.into_iter().map(|(field, label, expression, dimension)| SemanticElement {
        id: format!("{node}::surface::{field}"), role: "feature-field".into(), label: label.into(),
        value: Some(expression.0.clone()), state: state.into(), commands: vec!["feature.edit".into()],
        relationships: vec![node.into()],
        data: serde_json::json!({"contract":"tcad.surface","version":CONTRACT_VERSION,"field":field,"type":"quantity","dimension":dimension,"expression":expression,"evaluated":crate::expression::value(doc,owner,expression,dimension).ok(),"editable":true,"target":{"kind":"feature","definition":owner,"feature":node}}),
    }).collect();
    let capabilities = match operation {
        SurfaceOperation::Ruled { .. } => {
            serde_json::json!({"geometry":"explicit_line_positive_rational_native_curves","sources":["sketch_member","model_edge","reference_curve"],"construction":"exact_vector_translation","direction":"associative_or_explicit","bounded_regularity":"positive_bernstein_projection","native_degree_range":[1,8],"maximum_sources":256,"maximum_strips":4096,"surface_orientation_reversal":true})
        }
        SurfaceOperation::MidSurface { .. } => {
            serde_json::json!({"geometry":["parallel_planar_faces_with_matching_translated_trims","compatible_full_natural_rational_patches"],"fraction":"normalized_0_to_1","rational_mean":"corresponding_uv_positions","rational_correspondence":["swap_uv","reverse_u","reverse_v"],"regularity":"bounded_global_injective_projection_certificate","normal_offset_equivalence":"parallel_planar_pairs_only"})
        }
        SurfaceOperation::Planar { .. } => {
            serde_json::json!({"geometry":"closed_coplanar_contours","holes":true,"multiple_regions":true})
        }
        SurfaceOperation::Extrude { .. } => {
            serde_json::json!({"geometry":"open_or_closed_exact_contours","ends":["blind","mid_plane"],"direction":["profile_normal","specified_vector"]})
        }
        SurfaceOperation::Revolve { .. } => {
            serde_json::json!({"geometry":"open_or_closed_exact_contours","axis":"numeric_or_associative","angle":"signed_up_to_360_degrees"})
        }
        SurfaceOperation::Loft { .. } => {
            serde_json::json!({"geometry":"exact_ruled_between_single_native_contours","closed_or_open":true,"unequal_edge_counts":"exact_native_refinement","rational_weights":"exact_common_denominator","correspondence":"automatic_winding_and_geometric_seam","intermediate_topology":"shared_section_boundaries","regularity":"radial_or_native_ruled_chart_or_bounded_projected_jacobian_certificate","ruled_parameter_degree":1,"concave_charts":"exact_native_shared_column_refinement","within_pair_self_intersection_certificate":"parallel_radial_pairs_only","cross_section_pair_intersections":"not_certified","guides":false,"continuity":"positional"})
        }
        SurfaceOperation::Sweep { .. } => {
            serde_json::json!({"geometry":"ruled_fixed_orientation_polyline","sketch_path":"single_open_polyline","guides":false,"twist":false})
        }
        SurfaceOperation::Fill { .. } => {
            serde_json::json!({"geometry":"four_exact_polynomial_boundaries","construction":"coons_tensor_product","continuity":"positional","rational_variable_weights":false})
        }
        SurfaceOperation::Trim { .. } => {
            serde_json::json!({"geometry":"planar_faces_and_coplanar_closed_sketches","modes":["keep_inside","remove_inside"],"holes":true,"curves":"exact_boolean_intersections"})
        }
        SurfaceOperation::TrimDomain { .. } => {
            serde_json::json!({"geometry":"natural_rectangular_positive_weight_rational_patch","trim":"exact_knot_insertion","parameters":"normalized_0_to_1"})
        }
        SurfaceOperation::Untrim { .. } => {
            serde_json::json!({"geometry":"finite_positive_weight_rational_carrier","restores":"natural_parameter_boundary","holes_removed":true})
        }
        SurfaceOperation::Extend { .. } => {
            serde_json::json!({"geometry":"single_convex_planar_straight_boundary","edges":"all_or_selected","construction":"exact_parallel_line_intersections"})
        }
        SurfaceOperation::Offset { .. } => {
            serde_json::json!({"geometry":"coplanar_consistently_oriented_face_shells","signed_offset":true})
        }
        SurfaceOperation::Thicken { .. } => {
            serde_json::json!({"geometry":"coplanar_consistently_oriented_face_shells","boundaries":"explicit_line_bspline_nurbs","implicit_intersections":false,"modes":["one_side","mid_plane"],"reverse":true})
        }
        SurfaceOperation::Reverse { .. } => {
            serde_json::json!({"geometry":"surface_shells","changes":"face_orientation"})
        }
        SurfaceOperation::Knit { .. } => {
            serde_json::json!({"geometry":"coincident_matching_rational_boundary_curves","tolerance_mm":[1e-10,1e-6],"make_solid":"one_closed_oriented_shell","gap_filling":false})
        }
    };
    out.push(SemanticElement {
        id: format!("{node}::surface::options"), role: "feature-options".into(), label: operation.name().into(),
        value: None, state: state.into(), commands: vec!["feature.edit".into()],
        relationships: operation.dependencies().iter().map(|r| r.node.clone()).collect(),
        data: serde_json::json!({"contract":"tcad.surface","version":CONTRACT_VERSION,"source":operation,"body_kind":if operation.is_surface(){"surface"}else{"solid"},"capabilities":capabilities,"editable":true,"target":{"kind":"feature","definition":owner,"feature":node}}),
    });
    let sources: Vec<(&str, String, serde_json::Value, NodeRef)> = match operation {
        SurfaceOperation::Ruled { sources, .. } => sources
            .iter()
            .map(|source| {
                (
                    "Curve Source",
                    source.id.clone(),
                    serde_json::json!(source),
                    source.source.dependency().clone(),
                )
            })
            .collect(),
        SurfaceOperation::MidSurface { faces, .. } => faces
            .iter()
            .enumerate()
            .map(|(index, source)| {
                (
                    "Face Source",
                    index.to_string(),
                    serde_json::json!(source),
                    source.body.clone(),
                )
            })
            .collect(),
        _ => vec![],
    };
    for (label, key, source, dependency) in sources {
        out.push(SemanticElement{id:format!("{node}::surface::source::{key}"),role:"feature-reference".into(),label:label.into(),value:None,state:state.into(),commands:vec!["feature.edit".into()],relationships:vec![dependency.node.clone()],data:serde_json::json!({"contract":"tcad.surface","version":CONTRACT_VERSION,"source":source,"dependency":dependency,"editable":true,"target":{"kind":"feature","definition":owner,"feature":node}})});
    }
    out
}
