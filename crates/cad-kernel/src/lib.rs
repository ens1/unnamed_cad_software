pub mod adapter;
pub mod evaluate;
mod planar;

pub mod exchange;
mod step_import;

mod fabrication;
mod sheet_stock;
mod weld_stock;

mod units;

mod sketches;

mod validation;

mod analytic_offset;
mod axis_geometry;
mod body_filter;
mod boolean_ops;
mod bounded_carriers;
mod curve_pattern;
mod curve_sampling;
mod curved_shell;
mod direct_edit;
mod drilled_pockets;
mod edge_round;
mod extrusion;
mod extrusion_surface;
mod general_chamfer;
mod general_round;
mod holes;
mod join;
mod loft_correspondence;
mod mold;
mod native_bounds;
mod operation_preview;
mod placement_pattern;
mod pocket_shell;
mod polyhedron;
mod preview_csg;
mod prismatic_shell;
mod reference_curves;
mod revolution;
mod rib;
mod section_checks;
mod section_history;
mod shell_history;
mod smooth_shell;
mod solid_sections;
mod split;
mod surface_construct;
mod surface_edit;
mod surface_fill;
mod surfaces;
mod swept;
mod tangent_plane;
mod turned_shell;

mod assemblies;
mod assembly_inspection;

mod mate_geometry;
