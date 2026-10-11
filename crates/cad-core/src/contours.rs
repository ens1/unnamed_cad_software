//! Stable application-owned material contour selection, independent of evaluated B-rep order.
use crate::{CadError, Id, NodeRef, Result};
use serde::{Deserialize, Serialize};

pub const CONTRACT_VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Selection {
    pub profile: NodeRef,
    /// One or more native material members on the same outer boundary.
    /// In thin mode these identify one connected source chain.
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
                "A Selected Contour needs a native outer-boundary member".into(),
            ));
        }
    }
    Ok(())
}

pub fn semantics(selections: &[Selection], thin: bool) -> serde_json::Value {
    serde_json::json!({"contract":"tcad.contours","version":CONTRACT_VERSION,"selection":"native_outer_boundary_members","selections":selections,"order":"selected_record_order_per_profile","unlisted_profiles":"all_material_regions","contained_holes":"retained","thin_selection":if thin {"connected_source_chain"}else{"disabled"},"validation":"one_unique_current_region_or_chain_per_selection","editable":true})
}
