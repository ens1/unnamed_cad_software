//! Exact turned-rim fillets share topology, containment and collision
//! proofs with circular chamfers; only the meridian and carrier change.
use crate::{
    general_chamfer::{self, Treatment},
    planar::ChamferSpec,
    shell_history::ShellHistory,
};
use cad_core::Result;
use monstertruck::modeling::*;

pub(crate) fn round(
    solid: &Solid,
    selected: &[EdgeId],
    radius: f64,
    history: &mut ShellHistory,
) -> Result<Option<Solid>> {
    general_chamfer::construct(
        solid,
        selected,
        &ChamferSpec {
            distance: radius,
            second_distance: None,
            angle: None,
            flip: false,
            first_face: None,
        },
        Treatment::Fillet,
        history,
    )
}
