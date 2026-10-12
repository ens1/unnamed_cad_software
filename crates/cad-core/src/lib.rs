pub mod appearance;
pub mod assembly_geometry;
pub mod assembly_inspection;
pub mod assembly_motion;
pub mod body_editing;
pub mod body_filter;
pub mod bom;
pub mod component_pattern;
pub mod configuration;
pub mod contours;
pub mod curve_pattern;
pub mod cut_list;
pub mod direct_edit;
pub mod document;
pub mod drawing;
pub mod drawing_interaction;
pub mod drawing_tables;
pub mod edge_features;
pub mod exploded_views;
pub mod expression;
pub mod extrusion;
pub mod feature_order;
pub mod finish_library;
pub mod history;
pub mod holes;
pub mod inspection;
pub mod manufacturing;
mod mate_solver;
pub mod material_library;
pub mod materials;
pub mod persistence;
pub mod placement_pattern;
pub mod presentation;
pub mod production;
pub mod protocol;
pub mod reference_curves;
pub mod reference_geometry;
pub mod reference_repair;
pub mod revolution;
pub mod rib;
pub mod sketch_diagnostics;
pub mod sketch_dimensions;
pub mod sketch_equation;
pub mod sketch_geometry;
pub mod sketch_handles;
pub mod sketch_topology;
pub mod solid_sections;
pub mod solver;
pub mod split;
pub mod transaction;
pub mod version_control;

pub use document::*;
pub use transaction::*;

#[derive(Debug, thiserror::Error)]
pub enum CadError {
    #[error("{0}")]
    Invalid(String),
    #[error("Unsupported: {0}")]
    Unsupported(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}
pub type Result<T> = std::result::Result<T, CadError>;

pub mod commands;
pub mod library;
pub mod release;
pub mod selection_actions;
pub mod selection_sets;
pub mod settings;
pub mod sketch_actions;

pub mod modeling_tools;

pub mod assembly_copy;
pub mod assembly_groups;
pub mod assembly_welds;
pub mod mold;
pub mod navigation;
pub mod sheet_metal;
pub mod sketch_blocks;
pub mod sketch_conic;
pub mod sketch_construction;
pub mod sketch_edit;
pub mod sketch_relations;
pub mod sketch_spline;
pub mod sketch_text;
pub mod sketch_transform;
pub mod sketch_trim;
pub mod surface;
pub mod wall;
pub mod weld_beads;
pub mod weldment;
