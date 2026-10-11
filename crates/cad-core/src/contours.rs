//! Stable application-owned material contour selection, independent of evaluated B-rep order.
use crate::{CadError, Id, NodeRef, Result};
use serde::{Deserialize, Serialize};

pub const CONTRACT_VERSION: u32 = 2;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Selection {
    pub profile: NodeRef,
    /// Persistent native material members identifying one or more regions.
    /// Hole boundaries select their containing region, retaining the hole.
    /// In thin mode each member identifies its connected source chain.
    /// The historical field name is retained for saved-document compatibility.
    pub outer_members: Vec<Id>,
}

pub fn validate_profiles(profiles: &[NodeRef], selections: &[Selection]) -> Result<()> {
    for selection in selections {
        if !profiles.contains(&selection.profile) {
            return Err(CadError::Invalid(
                "A Selected Contour references a sketch outside this feature's source profiles"
                    .into(),
            ));
        }
        if selection.outer_members.is_empty() {
            return Err(CadError::Invalid(
                "Select at least one material contour or choose All Closed Contours".into(),
            ));
        }
    }
    Ok(())
}

pub fn semantics(selections: &[Selection], thin: bool) -> serde_json::Value {
    serde_json::json!({"contract":"tcad.contours","version":CONTRACT_VERSION,"selection":"native_material_boundary_members","selections":selections,"order":"selected_record_then_member_order_per_profile","unlisted_profiles":"all_closed_material_regions","unselected_open_chains":"ignored_for_solid_sections","contained_holes":"retained","hole_boundary_selection":"containing_material_region","thin_selection":if thin {"connected_source_chains"}else{"disabled"},"validation":"one_unique_current_region_or_chain_per_member","editable":true})
}
