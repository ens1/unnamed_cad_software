//! Versioned local sketch motion and equation diagnostics over authoritative f64 geometry.
//!
//! Nullspace directions describe infinitesimal motion at a validated solution.
//! They do not certify finite travel or identify a globally minimal conflict set.
use crate::{solver, *};
use nalgebra::{DMatrix, DVector};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const CONTRACT_VERSION: u32 = 1;
pub const MAX_VARIABLES: usize = 512;
pub const MAX_EQUATIONS: usize = 2048;
pub const MAX_POINT_DERIVATIVES: usize = 2_000_000;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PointMotion {
    /// Zero, one or two independent directions in the sketch's local XY frame.
    pub directions: Vec<[f64; 2]>,
    /// Generated geometry follows its sources rather than being directly draggable.
    pub generated: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EquationDiagnostic {
    pub equations: usize,
    pub maximum_residual: f64,
    /// Rank added in authored constraint order; dependencies are local and numerical.
    pub independent_equations: usize,
    pub dependent_equations: usize,
    /// Equations with no current positional derivative, including direct size expressions.
    pub nonpositional_equations: usize,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Analysis {
    pub version: u32,
    pub variables: usize,
    pub equation_rank: usize,
    pub degrees_of_freedom: usize,
    pub maximum_residual: f64,
    pub points: BTreeMap<Id, PointMotion>,
    pub constraints: BTreeMap<Id, EquationDiagnostic>,
}

/// Shared coordinate layout and derivation for native solving and diagnostics.
pub(crate) struct System<'a> {
    doc: &'a Document,
    owner: &'a str,
    base: Sketch,
    prepared: sketch_blocks::Prepared,
    keys: Vec<Id>,
    contacts: Vec<(Id, usize)>,
    rotations: Vec<Id>,
    pub(crate) start: DVector<f64>,
}
impl<'a> System<'a> {
    pub(crate) fn new(doc: &'a Document, owner: &'a str, sketch: &Sketch) -> Result<Self> {
        let base = sketch.clone();
        let prepared = sketch_blocks::prepare(doc, owner, &base)?;
        let driven = sketch_edit::driven_points(&base);
        let suppressed = sketch_transform::suppressed_points(&base);
        let keys = base
            .points
            .keys()
            .filter(|key| !driven.contains(*key) && !suppressed.contains(*key))
            .cloned()
            .collect::<Vec<_>>();
        let contacts = solver::contact_variables(&base);
        let rotations = sketch_blocks::rotation_variables(&base);
        let angles = sketch_blocks::rotation_values(doc, owner, &base, &rotations)?;
        let start = DVector::from_iterator(
            keys.len() * 2 + contacts.len() + rotations.len(),
            keys.iter()
                .flat_map(|key| base.points[key])
                .chain(contacts.iter().map(|(id, i)| {
                    base.constraints
                        .iter()
                        .find(|constraint| &constraint.id == id)
                        .unwrap()
                        .contact
                        .as_ref()
                        .unwrap()
                        .parameters[*i]
                }))
                .chain(angles),
        );
        Ok(Self {
            doc,
            owner,
            base,
            prepared,
            keys,
            contacts,
            rotations,
            start,
        })
    }
    pub(crate) fn derive(&self, x: &DVector<f64>) -> Result<Sketch> {
        let mut candidate = self.base.clone();
        for (i, key) in self.keys.iter().enumerate() {
            candidate
                .points
                .insert(key.clone(), [x[2 * i], x[2 * i + 1]]);
        }
        solver::put_contact_variables(&mut candidate, &self.contacts, self.keys.len() * 2, x)?;
        sketch_blocks::put_rotations(
            &mut candidate,
            &self.rotations,
            &x.as_slice()[self.keys.len() * 2 + self.contacts.len()..],
        )?;
        sketch_actions::apply_curve_dimensions(self.doc, self.owner, &mut candidate)?;
        sketch_edit::derive_prepared(self.doc, self.owner, &mut candidate, &self.prepared)?;
        Ok(candidate)
    }
    fn blocks(&self, candidate: &Sketch) -> Result<Vec<sketch_actions::ConstraintResidual>> {
        sketch_actions::residual_blocks(self.doc, self.owner, candidate, |key| {
            sketch_actions::entity_point(candidate, key)
        })
    }
    pub(crate) fn residuals(&self, x: &DVector<f64>) -> Result<DVector<f64>> {
        let blocks = self.blocks(&self.derive(x)?)?;
        Ok(DVector::from_vec(
            blocks.into_iter().flat_map(|block| block.values).collect(),
        ))
    }
}

/// Authored duplicate equations can be removed without guessing constraint intent.
/// Group-owned IDs are kept because the group has additional associative meaning.
pub fn exact_duplicates(sketch: &Sketch) -> BTreeMap<Id, Id> {
    let protected = sketch
        .dimension_groups
        .values()
        .flat_map(|group| group.entries.iter().map(|entry| entry.constraint.as_str()))
        .collect::<BTreeSet<_>>();
    let mut retained: Vec<&SketchConstraint> = vec![];
    let mut duplicates = BTreeMap::new();
    for constraint in &sketch.constraints {
        if protected.contains(constraint.id.as_str()) {
            retained.push(constraint);
            continue;
        }
        let duplicate = retained.iter().find(|other| {
            other.kind == constraint.kind
                && other.entities == constraint.entities
                && other.value == constraint.value
                && other.driven == constraint.driven
                && other.contact == constraint.contact
                && other.distance_side == constraint.distance_side
        });
        if let Some(other) = duplicate {
            duplicates.insert(constraint.id.clone(), other.id.clone());
        } else {
            retained.push(constraint);
        }
    }
    duplicates
}

pub(crate) fn analyze(doc: &Document, owner: &str, sketch: &Sketch) -> Result<Analysis> {
    let system = System::new(doc, owner, sketch)?;
    let x = &system.start;
    if x.iter().any(|value| !value.is_finite()) {
        return Err(CadError::Invalid(
            "Sketch motion needs finite current coordinates".into(),
        ));
    }
    if x.len() > MAX_VARIABLES {
        return Err(CadError::Unsupported(format!(
            "Sketch motion display supports at most {MAX_VARIABLES} free coordinates; the sketch remains editable"
        )));
    }
    let candidate = system.derive(x)?;
    let blocks = system.blocks(&candidate)?;
    let r = DVector::from_vec(
        blocks
            .iter()
            .flat_map(|block| block.values.iter().copied())
            .collect(),
    );
    if r.len() > MAX_EQUATIONS || r.iter().any(|value| !value.is_finite()) {
        return Err(CadError::Unsupported(
            "Sketch motion display needs finite equations within its analysis work limit".into(),
        ));
    }
    let maximum_residual = r.iter().fold(0_f64, |a, b| a.max(b.abs()));
    if maximum_residual > 1e-6 {
        return Err(CadError::Invalid(
            "Motion directions require the current sketch constraints to be satisfied".into(),
        ));
    }
    sketch_geometry::polylines(doc, owner, &candidate)?;
    let j = solver::jacobian(x, &r, &|x| system.residuals(x))?;
    let (scaled, scale) = solver::scaled_jacobian(&j);
    let mut projector = DMatrix::identity(x.len(), x.len());
    let mut rank = 0;
    let mut threshold = 1e-9;
    if !r.is_empty() && !x.is_empty() {
        let svd = scaled.clone().svd(false, true);
        threshold = (svd.singular_values.amax() * 1e-8).max(1e-12);
        rank = svd.rank(threshold);
        let vt = svd.v_t.unwrap();
        for i in 0..svd.singular_values.len() {
            if svd.singular_values[i] > threshold {
                let row = vt.row(i);
                projector -= row.transpose() * row;
            }
        }
    }
    // A rank-revealing row-space basis explains dependencies in authored order.
    // Exact zero positional rows are kept separate from dependent geometry rows.
    let mut basis: Vec<DVector<f64>> = vec![];
    let mut constraints = BTreeMap::new();
    let mut offset = 0;
    for block in &blocks {
        let mut independent = 0;
        let mut nonpositional = 0;
        for row in offset..offset + block.values.len() {
            let mut value = scaled.row(row).transpose().into_owned();
            if value.norm() <= threshold {
                nonpositional += 1;
                continue;
            }
            // Twice-reorthogonalized modified Gram-Schmidt avoids duplicate
            // equations masquerading as independent ones after cancellation.
            for _ in 0..2 {
                for direction in &basis {
                    value -= direction * direction.dot(&value);
                }
            }
            let norm = value.norm();
            if norm > threshold {
                basis.push(value / norm);
                independent += 1;
            }
        }
        constraints.insert(
            block.constraint.clone(),
            EquationDiagnostic {
                equations: block.values.len(),
                maximum_residual: block.values.iter().fold(0_f64, |a, b| a.max(b.abs())),
                independent_equations: independent,
                dependent_equations: block
                    .values
                    .len()
                    .saturating_sub(independent + nonpositional),
                nonpositional_equations: nonpositional,
            },
        );
        offset += block.values.len();
    }
    if basis.len() != rank {
        return Err(CadError::Unsupported(
            "The current positional equation rank is numerically uncertain; edit the sketch with normal solver validation".into(),
        ));
    }
    let suppressed = sketch_transform::suppressed_points(&candidate);
    let driven = sketch_edit::driven_points(&candidate);
    let keys = candidate
        .points
        .keys()
        .filter(|key| !suppressed.contains(*key))
        .cloned()
        .collect::<Vec<_>>();
    if keys.len().saturating_mul(x.len()).saturating_mul(2) > MAX_POINT_DERIVATIVES {
        return Err(CadError::Unsupported(
            "Sketch motion display exceeds its derived point work limit".into(),
        ));
    }
    let mut derivatives = DMatrix::zeros(keys.len() * 2, x.len());
    let independent = system
        .keys
        .iter()
        .enumerate()
        .map(|(i, key)| (key.as_str(), i))
        .collect::<BTreeMap<_, _>>();
    for (i, key) in keys.iter().enumerate() {
        if let Some(index) = independent.get(key.as_str()) {
            derivatives[(i * 2, index * 2)] = 1.;
            derivatives[(i * 2 + 1, index * 2 + 1)] = 1.;
        }
    }
    if keys.iter().any(|key| driven.contains(key)) {
        for col in 0..x.len() {
            let step = f64::EPSILON.cbrt() * x[col].abs().max(1.);
            let mut shifted = x.clone();
            shifted[col] += step;
            let plus = system.derive(&shifted);
            shifted[col] = x[col] - step;
            let minus = system.derive(&shifted);
            for (i, key) in keys
                .iter()
                .enumerate()
                .filter(|(_, key)| driven.contains(*key))
            {
                for axis in 0..2 {
                    derivatives[(i * 2 + axis, col)] = match (&plus, &minus) {
                        (Ok(a), Ok(b)) => (a.points[key][axis] - b.points[key][axis]) / (2. * step),
                        (Ok(a), Err(_)) => {
                            (a.points[key][axis] - candidate.points[key][axis]) / step
                        }
                        (Err(_), Ok(b)) => {
                            (candidate.points[key][axis] - b.points[key][axis]) / step
                        }
                        (Err(error), Err(_)) => {
                            return Err(CadError::Invalid(format!(
                                "Cannot establish derived sketch motion: {error}"
                            )));
                        }
                    };
                }
            }
        }
    }
    // Convert the scaled coordinate nullspace back to physical coordinates.
    // Normalize each spanning vector before mapping it to derived points; tiny
    // roundoff in a constrained projector column must not be amplified by an
    // unconstrained coordinate's 1e-12 derivative scale.
    for col in 0..x.len() {
        if projector.column(col).norm() < 1e-8 {
            projector.column_mut(col).fill(0.);
            continue;
        }
        for row in 0..x.len() {
            projector[(row, col)] /= scale[row];
        }
        let norm = projector.column(col).norm();
        if !norm.is_finite() || norm <= 0. {
            return Err(CadError::Invalid(
                "Sketch motion coordinate scaling is uncertain".into(),
            ));
        }
        projector.column_mut(col).scale_mut(1. / norm);
    }
    let allowed = derivatives * projector;
    if allowed.iter().any(|value| !value.is_finite()) {
        return Err(CadError::Invalid(
            "Sketch motion contains nonfinite derivatives".into(),
        ));
    }
    let mut points = BTreeMap::new();
    for (i, key) in keys.iter().enumerate() {
        let block = allowed.rows(i * 2, 2).into_owned();
        let eigen = (&block * block.transpose()).symmetric_eigen();
        let maximum = eigen.eigenvalues.amax();
        let directions = (0..2)
            .filter(|axis| eigen.eigenvalues[*axis] > 1e-12_f64.max(maximum * 1e-8))
            .map(|axis| {
                let mut direction = [eigen.eigenvectors[(0, axis)], eigen.eigenvectors[(1, axis)]];
                let dominant = usize::from(direction[1].abs() > direction[0].abs());
                if direction[dominant] < 0. {
                    direction = direction.map(|value| -value);
                }
                direction
            })
            .collect();
        points.insert(
            key.clone(),
            PointMotion {
                directions,
                generated: driven.contains(key),
            },
        );
    }
    Ok(Analysis {
        version: CONTRACT_VERSION,
        variables: x.len(),
        equation_rank: rank,
        degrees_of_freedom: x.len().saturating_sub(rank),
        maximum_residual,
        points,
        constraints,
    })
}
