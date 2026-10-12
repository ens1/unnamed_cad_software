use crate::{Assembly, CadError, Dimension, Document, Id, Placement, Result, Sketch, expression};
use nalgebra::{DMatrix, DVector, Matrix4, Rotation3, Vector3};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SolveReport {
    pub converged: bool,
    pub residual: f64,
    pub degrees_of_freedom: usize,
    pub redundant_equations: usize,
    pub iterations: usize,
}
/// Version 1 transient cursor goals. These never become document constraints.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SketchDragTarget {
    pub point: Id,
    pub position: [f64; 2],
}
pub trait ConstraintSolver {
    fn fingerprint(&self) -> &str;
    fn solve_sketch(&self, doc: &Document, part: &str, sketch: &mut Sketch) -> Result<SolveReport>;
    fn drag_sketch(
        &self,
        _doc: &Document,
        _part: &str,
        _sketch: &mut Sketch,
        _targets: &[SketchDragTarget],
    ) -> Result<SolveReport> {
        Err(CadError::Unsupported(
            "This solver does not support sketch dragging".into(),
        ))
    }
    fn analyze_sketch(
        &self,
        _doc: &Document,
        _part: &str,
        _sketch: &Sketch,
    ) -> Result<crate::sketch_diagnostics::Analysis> {
        Err(CadError::Unsupported(
            "This solver does not expose sketch motion analysis".into(),
        ))
    }
    fn solve_assembly(&self, doc: &Document, assembly: &mut Assembly) -> Result<SolveReport>;
    fn solve_assembly_frames(
        &self,
        _doc: &Document,
        _assembly: &mut Assembly,
        _frames: &BTreeMap<Id, [crate::assembly_geometry::Frame; 2]>,
    ) -> Result<SolveReport> {
        Err(CadError::Unsupported(
            "This solver does not support associative mate frames".into(),
        ))
    }
    fn solve_assembly_geometry(
        &self,
        doc: &Document,
        assembly: &mut Assembly,
        frames: &BTreeMap<Id, [crate::assembly_geometry::Frame; 2]>,
        auxiliary: &BTreeMap<Id, Vec<crate::assembly_geometry::ResolvedEndpoint>>,
    ) -> Result<crate::assembly_geometry::AssemblySolution> {
        if assembly.mates.iter().any(|m| m.relationship.is_some()) || !auxiliary.is_empty() {
            return Err(CadError::Unsupported(
                "This solver does not implement advanced or mechanical mates".into(),
            ));
        }
        Ok(crate::assembly_geometry::AssemblySolution {
            report: self.solve_assembly_frames(doc, assembly, frames)?,
            diagnostics: BTreeMap::new(),
            component_motion: BTreeMap::new(),
        })
    }
    fn manipulate_assembly(
        &self,
        _doc: &Document,
        _assembly: &mut Assembly,
        _frames: &BTreeMap<Id, [crate::assembly_geometry::Frame; 2]>,
        _auxiliary: &BTreeMap<Id, Vec<crate::assembly_geometry::ResolvedEndpoint>>,
        _goal: &crate::assembly_motion::Goal,
    ) -> Result<crate::assembly_geometry::AssemblySolution> {
        Err(CadError::Unsupported(
            "This solver does not support constraint-respecting assembly movement".into(),
        ))
    }
    fn manipulate_assembly_group(
        &self,
        doc: &Document,
        assembly: &mut Assembly,
        frames: &BTreeMap<Id, [crate::assembly_geometry::Frame; 2]>,
        auxiliary: &BTreeMap<Id, Vec<crate::assembly_geometry::ResolvedEndpoint>>,
        goals: &[crate::assembly_motion::Goal],
    ) -> Result<crate::assembly_geometry::AssemblySolution> {
        match goals {
            [goal] => self.manipulate_assembly(doc, assembly, frames, auxiliary, goal),
            _ => Err(CadError::Unsupported(
                "This solver does not implement simultaneous component manipulation".into(),
            )),
        }
    }
}
pub struct NativeSolver;
const RESIDUAL_TOLERANCE: f64 = 1e-6;

pub(crate) fn contact_variables(sketch: &Sketch) -> Vec<(Id, usize)> {
    let inactive = crate::sketch_transform::inactive_entities(sketch);
    sketch
        .constraints
        .iter()
        .filter(|c| !c.driven && !c.entities.iter().any(|key| inactive.contains(key)))
        .flat_map(|c| {
            c.contact.as_ref().into_iter().flat_map(move |contact| {
                (0..2)
                    .filter(|i| contact.free[*i])
                    .map(move |i| (c.id.clone(), i))
            })
        })
        .collect()
}
pub(crate) fn put_contact_variables(
    sketch: &mut Sketch,
    variables: &[(Id, usize)],
    offset: usize,
    x: &DVector<f64>,
) -> Result<()> {
    for (i, (id, coordinate)) in variables.iter().enumerate() {
        let contact = sketch
            .constraints
            .iter_mut()
            .find(|c| &c.id == id)
            .and_then(|c| c.contact.as_mut())
            .ok_or_else(|| CadError::Invalid("Curve contact variable no longer exists".into()))?;
        contact.parameters[*coordinate] = x[offset + i];
    }
    Ok(())
}

fn finite_residual(r: &DVector<f64>, count: Option<usize>) -> Result<()> {
    if count.is_some_and(|n| n != r.len()) {
        return Err(CadError::Invalid(
            "Constraint equation count changed".into(),
        ));
    }
    if r.iter().any(|v| !v.is_finite()) {
        return Err(CadError::Invalid("Degenerate constraint equations".into()));
    }
    Ok(())
}

pub(crate) fn jacobian(
    x: &DVector<f64>,
    r: &DVector<f64>,
    f: &impl Fn(&DVector<f64>) -> Result<DVector<f64>>,
) -> Result<DMatrix<f64>> {
    let mut j = DMatrix::zeros(r.len(), x.len());
    if r.is_empty() {
        // With no equations every derivative is an empty vector. Avoid cloning
        // and deriving an entire unconstrained sketch for each coordinate.
        return Ok(j);
    }
    for col in 0..x.len() {
        let step = f64::EPSILON.cbrt() * x[col].abs().max(1.);
        let mut shifted = x.clone();
        shifted[col] += step;
        let plus = f(&shifted).and_then(|rr| {
            finite_residual(&rr, Some(r.len()))?;
            Ok(rr)
        });
        shifted[col] = x[col] - step;
        let minus = f(&shifted).and_then(|rr| {
            finite_residual(&rr, Some(r.len()))?;
            Ok(rr)
        });
        for row in 0..r.len() {
            j[(row, col)] = match (&plus, &minus) {
                (Ok(a), Ok(b)) => {
                    let forward = a[row] - r[row];
                    let backward = r[row] - b[row];
                    let discontinuity = f64::EPSILON.sqrt() * (1. + r[row].abs());
                    // Wrapped angles have a branch boundary. Differentiate on
                    // the side continuous with the current equation value.
                    if forward.abs() > discontinuity && forward.abs() > 8. * backward.abs() {
                        j[(row, col)] = backward / step;
                        continue;
                    }
                    if backward.abs() > discontinuity && backward.abs() > 8. * forward.abs() {
                        j[(row, col)] = forward / step;
                        continue;
                    }
                    let central = (a[row] - b[row]) / (2. * step);
                    // At a distance cusp (coincident points), choose a reproducible
                    // one-sided branch instead of accepting a zero Jacobian.
                    if central.abs() < 1e-12 && (a[row] - r[row]).abs() > step * 1e-4 {
                        (a[row] - r[row]) / step
                    } else {
                        central
                    }
                }
                (Ok(a), Err(_)) => (a[row] - r[row]) / step,
                (Err(_), Ok(b)) => (r[row] - b[row]) / step,
                (Err(e), Err(_)) => {
                    return Err(CadError::Invalid(format!(
                        "Cannot differentiate constraint equations: {e}"
                    )));
                }
            };
        }
    }
    if j.iter().any(|v| !v.is_finite()) {
        return Err(CadError::Invalid(
            "Constraint derivatives must be finite".into(),
        ));
    }
    Ok(j)
}

pub(crate) fn scaled_jacobian(j: &DMatrix<f64>) -> (DMatrix<f64>, DVector<f64>) {
    let scale = DVector::from_iterator(j.ncols(), j.column_iter().map(|c| c.norm().max(1e-12)));
    let mut scaled = j.clone();
    for col in 0..j.ncols() {
        scaled.column_mut(col).scale_mut(1. / scale[col]);
    }
    (scaled, scale)
}

// Damped SVD avoids squaring the Jacobian's condition number. Trial steps reuse
// one decomposition; invalid geometry rejects a step and increases damping.
fn minimize(
    x: DVector<f64>,
    f: impl Fn(&DVector<f64>) -> Result<DVector<f64>>,
    limit: usize,
) -> Result<(DVector<f64>, SolveReport)> {
    minimize_in_metric(x, f, limit, None)
}

fn jacobian_in_metric(
    j: &DMatrix<f64>,
    metric: Option<&DVector<f64>>,
) -> (DMatrix<f64>, DVector<f64>) {
    if let Some(metric) = metric {
        let mut scaled = j.clone();
        for col in 0..j.ncols() {
            scaled.column_mut(col).scale_mut(1. / metric[col]);
        }
        (scaled, metric.clone())
    } else {
        scaled_jacobian(j)
    }
}

fn minimize_in_metric(
    mut x: DVector<f64>,
    f: impl Fn(&DVector<f64>) -> Result<DVector<f64>>,
    limit: usize,
    metric: Option<&DVector<f64>>,
) -> Result<(DVector<f64>, SolveReport)> {
    if x.iter().any(|v| !v.is_finite()) {
        return Err(CadError::Invalid(
            "Constraint coordinates must be finite".into(),
        ));
    }
    let mut r = f(&x)?;
    finite_residual(&r, None)?;
    let mut lambda: f64 = 1e-3;
    let mut iterations = 0;
    for _ in 0..limit {
        if r.is_empty() || x.is_empty() || r.amax() < 1e-8 {
            break;
        }
        let j = jacobian(&x, &r, &f)?;
        let (scaled, scale) = jacobian_in_metric(&j, metric);
        let svd = scaled.svd(true, true);
        let u = svd.u.as_ref().unwrap();
        let vt = svd.v_t.as_ref().unwrap();
        let projected = u.transpose() * &r;
        let cost = r.norm_squared();
        let cutoff = if metric.is_some() {
            svd.singular_values.amax() * 1e-8
        } else {
            0.
        };
        let mut accepted = None;
        let mut growth = 2.;
        for _ in 0..16 {
            iterations += 1;
            let coefficients = DVector::from_iterator(
                svd.singular_values.len(),
                svd.singular_values.iter().enumerate().map(|(i, s)| {
                    if *s <= cutoff {
                        0.
                    } else {
                        -projected[i] * s / (s * s + lambda)
                    }
                }),
            );
            let delta = (vt.transpose() * coefficients).component_div(&scale);
            if delta.iter().any(|v| !v.is_finite()) {
                return Err(CadError::Invalid("Constraint step is non-finite".into()));
            }
            let candidate = &x + &delta;
            let predicted = cost - (&r + &j * &delta).norm_squared();
            if predicted > 0.
                && let Ok(rr) = f(&candidate)
                && finite_residual(&rr, Some(r.len())).is_ok()
            {
                let improvement = cost - rr.norm_squared();
                let ratio = improvement / predicted;
                if improvement > 0. && ratio > 1e-4 {
                    lambda = (lambda * (1. - (2. * ratio - 1.).powi(3)).max(1. / 3.)).max(1e-15);
                    accepted = Some((candidate, rr, delta.norm(), improvement));
                    break;
                }
            }
            lambda = (lambda * growth).min(1e16);
            growth = (growth * 2.).min(1e6);
        }
        let Some((candidate, rr, step, improvement)) = accepted else {
            break;
        };
        x = candidate;
        r = rr;
        if step <= 1e-12 * (1. + x.norm()) || improvement <= f64::EPSILON * cost {
            break;
        }
    }
    let j = jacobian(&x, &r, &f)?;
    let rank = if j.nrows() == 0 || j.ncols() == 0 {
        0
    } else {
        let (scaled, _) = jacobian_in_metric(&j, metric);
        let svd = scaled.svd(false, false);
        svd.rank(svd.singular_values.amax() * 1e-8)
    };
    let residual = if r.is_empty() { 0. } else { r.amax() };
    let report = SolveReport {
        converged: residual <= RESIDUAL_TOLERANCE,
        residual,
        degrees_of_freedom: x.len().saturating_sub(rank),
        redundant_equations: r.len().saturating_sub(rank),
        iterations,
    };
    Ok((x, report))
}

fn assembly_matrix(
    assembly: &Assembly,
    index: &BTreeMap<Id, usize>,
    x: &DVector<f64>,
    key: &str,
) -> Result<Matrix4<f64>> {
    let instance = assembly
        .instances
        .get(key)
        .ok_or_else(|| CadError::Invalid(format!("Missing mate instance {key}")))?;
    let mut matrix = instance.placement.matrix();
    if let Some(i) = index.get(key) {
        // Solve an exponential-map increment around the starting orientation.
        // Euler angles remain a file representation, not solver coordinates.
        let rotation = Rotation3::new(Vector3::new(x[i * 6 + 3], x[i * 6 + 4], x[i * 6 + 5]));
        matrix = rotation.to_homogeneous() * matrix;
        for j in 0..3 {
            matrix[(j, 3)] = x[i * 6 + j];
        }
    }
    if matrix.iter().any(|v| !v.is_finite()) {
        return Err(CadError::Invalid(
            "Assembly placement must be finite".into(),
        ));
    }
    Ok(matrix)
}

fn axis_residual(a: Vector3<f64>, b: Vector3<f64>) -> Vector3<f64> {
    let b = if a.dot(&b) < 0. { -b } else { b };
    let cross = a.cross(&b);
    let sine = cross.norm();
    if sine < 1e-12 {
        cross * 100.
    } else {
        cross * (100. * sine.atan2(a.dot(&b)) / sine)
    }
}
impl ConstraintSolver for NativeSolver {
    fn fingerprint(&self) -> &str {
        "tcad-native-lm/16"
    }
    fn solve_sketch(&self, doc: &Document, part: &str, sketch: &mut Sketch) -> Result<SolveReport> {
        if sketch.points.keys().chain(sketch.members.keys()).any(|id| {
            matches!(
                id.as_str(),
                crate::sketch_actions::ORIGIN
                    | crate::sketch_actions::X_AXIS
                    | crate::sketch_actions::Y_AXIS
            )
        }) {
            return Err(CadError::Invalid(
                "Sketch identities cannot use reserved origin/axis references".into(),
            ));
        }
        let system = crate::sketch_diagnostics::System::new(doc, part, sketch)?;
        let (x, report) = minimize(system.start.clone(), |x| system.residuals(x), 120)?;
        let mut candidate = system.derive(&x)?;
        if !report.converged {
            let mut blocks =
                crate::sketch_actions::residual_blocks(doc, part, &candidate, |key| {
                    candidate
                        .points
                        .get(key)
                        .copied()
                        .ok_or_else(|| CadError::Invalid(format!("Missing sketch point {key}")))
                })?;
            let largest = |b: &crate::sketch_actions::ConstraintResidual| {
                b.values.iter().fold(0_f64, |s, v| s.max(v.abs()))
            };
            blocks.sort_by(|a, b| largest(b).total_cmp(&largest(a)));
            let failures = blocks
                .iter()
                .filter(|b| largest(b) > RESIDUAL_TOLERANCE)
                .take(3)
                .map(|b| {
                    format!(
                        "{} ({})",
                        crate::sketch_actions::label(&b.kind),
                        b.constraint
                    )
                })
                .collect::<Vec<_>>()
                .join(", ");
            return Err(CadError::Invalid(format!(
                "Sketch constraints did not converge: {failures}; check conflicting dimensions or degenerate geometry (maximum scaled residual {:.6})",
                report.residual
            )));
        }
        crate::sketch_actions::refresh_measurements(doc, part, &mut candidate)?;
        crate::sketch_geometry::polylines(doc, part, &candidate)?;
        *sketch = candidate;
        Ok(report)
    }
    fn analyze_sketch(
        &self,
        doc: &Document,
        part: &str,
        sketch: &Sketch,
    ) -> Result<crate::sketch_diagnostics::Analysis> {
        crate::sketch_diagnostics::analyze(doc, part, sketch)
    }
    fn drag_sketch(
        &self,
        doc: &Document,
        part: &str,
        sketch: &mut Sketch,
        targets: &[SketchDragTarget],
    ) -> Result<SolveReport> {
        if targets.is_empty()
            || targets
                .iter()
                .any(|t| t.position.iter().any(|v| !v.is_finite()))
        {
            return Err(CadError::Invalid("Invalid sketch drag targets".into()));
        }
        // Start from valid geometry. Cursor goals are soft, engineering constraints hard.
        let mut working = sketch.clone();
        self.solve_sketch(doc, part, &mut working)?;
        let source = &working;
        let driven = crate::sketch_edit::driven_points(source);
        let suppressed = crate::sketch_transform::suppressed_points(source);
        let keys: Vec<_> = source
            .points
            .keys()
            .filter(|key| !driven.contains(*key) && !suppressed.contains(*key))
            .cloned()
            .collect();
        let index: BTreeMap<_, _> = keys
            .iter()
            .enumerate()
            .map(|(i, k)| (k.clone(), i))
            .collect();
        for target in targets {
            if !index.contains_key(&target.point) {
                return Err(CadError::Invalid("Dragged point no longer exists".into()));
            }
        }
        let contacts = contact_variables(source);
        let rotations = crate::sketch_blocks::rotation_variables(source);
        let angles = crate::sketch_blocks::rotation_values(doc, part, source, &rotations)?;
        let seed = DVector::from_iterator(
            keys.len() * 2 + contacts.len() + rotations.len(),
            keys.iter()
                .flat_map(|k| source.points[k])
                .chain(contacts.iter().map(|(id, i)| {
                    source
                        .constraints
                        .iter()
                        .find(|c| &c.id == id)
                        .unwrap()
                        .contact
                        .as_ref()
                        .unwrap()
                        .parameters[*i]
                }))
                .chain(angles),
        );
        let base = source.clone();
        let prepared = crate::sketch_blocks::prepare(doc, part, &base)?;
        let equations = |x: &DVector<f64>| -> Result<DVector<f64>> {
            let mut candidate = base.clone();
            for (i, key) in keys.iter().enumerate() {
                candidate
                    .points
                    .insert(key.clone(), [x[2 * i], x[2 * i + 1]]);
            }
            put_contact_variables(&mut candidate, &contacts, keys.len() * 2, x)?;
            crate::sketch_blocks::put_rotations(
                &mut candidate,
                &rotations,
                &x.as_slice()[keys.len() * 2 + contacts.len()..],
            )?;
            crate::sketch_actions::apply_curve_dimensions(doc, part, &mut candidate)?;
            crate::sketch_edit::derive_prepared(doc, part, &mut candidate, &prepared)?;
            let mut r = crate::sketch_actions::residuals(doc, part, &candidate, |key| {
                candidate
                    .points
                    .get(key)
                    .copied()
                    .ok_or_else(|| CadError::Invalid(format!("Missing sketch point {key}")))
            })?;
            for value in &mut r {
                *value *= 10_000.;
            }
            for target in targets {
                let i = index[&target.point];
                r.extend([
                    x[2 * i] - target.position[0],
                    x[2 * i + 1] - target.position[1],
                ]);
            }
            // Resolve remaining freedom by staying near the gesture's starting geometry.
            r.extend(x.iter().zip(seed.iter()).map(|(a, b)| (a - b) * 0.0001));
            Ok(DVector::from_vec(r))
        };
        let (x, _) = minimize(seed.clone(), equations, 60)?;
        let mut candidate = working;
        for (i, key) in keys.iter().enumerate() {
            candidate
                .points
                .insert(key.clone(), [x[2 * i], x[2 * i + 1]]);
        }
        put_contact_variables(&mut candidate, &contacts, keys.len() * 2, &x)?;
        crate::sketch_blocks::put_rotations(
            &mut candidate,
            &rotations,
            &x.as_slice()[keys.len() * 2 + contacts.len()..],
        )?;
        // Only a strictly validated projection may be returned to the shared draft.
        let report = self.solve_sketch(doc, part, &mut candidate)?;
        *sketch = candidate;
        Ok(report)
    }
    fn solve_assembly(&self, doc: &Document, assembly: &mut Assembly) -> Result<SolveReport> {
        if assembly
            .mates
            .iter()
            .any(|m| m.first_reference.is_some() || m.second_reference.is_some())
        {
            return Err(CadError::Invalid(
                "Associative mates require evaluated reference geometry".into(),
            ));
        }
        solve_assembly_frames(doc, assembly, &BTreeMap::new())
    }
    fn solve_assembly_frames(
        &self,
        doc: &Document,
        assembly: &mut Assembly,
        frames: &BTreeMap<Id, [crate::assembly_geometry::Frame; 2]>,
    ) -> Result<SolveReport> {
        solve_assembly_frames(doc, assembly, frames)
    }
    fn solve_assembly_geometry(
        &self,
        doc: &Document,
        assembly: &mut Assembly,
        frames: &BTreeMap<Id, [crate::assembly_geometry::Frame; 2]>,
        auxiliary: &BTreeMap<Id, Vec<crate::assembly_geometry::ResolvedEndpoint>>,
    ) -> Result<crate::assembly_geometry::AssemblySolution> {
        solve_assembly_geometry(doc, assembly, frames, auxiliary, &[])
    }
    fn manipulate_assembly(
        &self,
        doc: &Document,
        assembly: &mut Assembly,
        frames: &BTreeMap<Id, [crate::assembly_geometry::Frame; 2]>,
        auxiliary: &BTreeMap<Id, Vec<crate::assembly_geometry::ResolvedEndpoint>>,
        goal: &crate::assembly_motion::Goal,
    ) -> Result<crate::assembly_geometry::AssemblySolution> {
        goal.validate()?;
        solve_assembly_geometry(doc, assembly, frames, auxiliary, std::slice::from_ref(goal))
    }
    fn manipulate_assembly_group(
        &self,
        doc: &Document,
        assembly: &mut Assembly,
        frames: &BTreeMap<Id, [crate::assembly_geometry::Frame; 2]>,
        auxiliary: &BTreeMap<Id, Vec<crate::assembly_geometry::ResolvedEndpoint>>,
        goals: &[crate::assembly_motion::Goal],
    ) -> Result<crate::assembly_geometry::AssemblySolution> {
        if goals.is_empty()
            || goals.len() > crate::assembly_motion::MAX_COMPONENTS
            || goals
                .iter()
                .map(|goal| &goal.instance)
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                != goals.len()
        {
            return Err(CadError::Invalid(
                "Simultaneous movement needs 1–128 distinct component goals".into(),
            ));
        }
        for goal in goals {
            goal.validate()?;
        }
        solve_assembly_geometry(doc, assembly, frames, auxiliary, goals)
    }
}

fn solve_assembly_frames(
    doc: &Document,
    assembly: &mut Assembly,
    frames: &BTreeMap<Id, [crate::assembly_geometry::Frame; 2]>,
) -> Result<SolveReport> {
    Ok(solve_assembly_geometry(doc, assembly, frames, &BTreeMap::new(), &[])?.report)
}
fn matrix_rank(j: &DMatrix<f64>, metric: Option<&DVector<f64>>) -> usize {
    if j.nrows() == 0 || j.ncols() == 0 || j.amax() < 1e-9 {
        0
    } else {
        // Nullspace blocks have already been physically scaled and normalized.
        // Renormalizing their near-zero columns would manufacture apparent DOF
        // for a component whose motion is actually locked.
        let scaled = metric
            .map(|metric| jacobian_in_metric(j, Some(metric)).0)
            .unwrap_or_else(|| j.clone());
        let svd = scaled.svd(false, false);
        svd.rank(svd.singular_values.amax() * 1e-8)
    }
}
fn motion_directions(block: &DMatrix<f64>) -> Vec<[f64; 3]> {
    let gram = block * block.transpose();
    let eigen = gram.symmetric_eigen();
    let scale = eigen.eigenvalues.amax().max(1.);
    (0..3)
        .filter(|i| eigen.eigenvalues[*i] > scale * 1e-8)
        .map(|i| std::array::from_fn(|j| eigen.eigenvectors[(j, i)]))
        .collect()
}
fn solve_assembly_geometry(
    doc: &Document,
    assembly: &mut Assembly,
    frames: &BTreeMap<Id, [crate::assembly_geometry::Frame; 2]>,
    auxiliary: &BTreeMap<Id, Vec<crate::assembly_geometry::ResolvedEndpoint>>,
    goals: &[crate::assembly_motion::Goal],
) -> Result<crate::assembly_geometry::AssemblySolution> {
    use crate::assembly_geometry::{
        AssemblySolution, ComponentMotion, CurvePosition, MateDiagnostic, Relationship,
    };
    let mut grounded = assembly
        .instances
        .values()
        .filter(|i| i.fixed)
        .map(|i| i.id.clone())
        .collect::<Vec<_>>();
    if grounded.is_empty() {
        let support = assembly.mates.iter().find_map(|m| {
            if let Some(Relationship::Coupler {
                support: Some(id), ..
            }) = m.relationship.as_deref()
            {
                Some(id)
            } else {
                None
            }
        });
        if let Some(support) = support {
            grounded.push(support.clone());
        }
    }
    let movable = assembly
        .instances
        .keys()
        .filter(|id| !grounded.contains(id))
        .cloned()
        .collect::<Vec<_>>();
    let index: BTreeMap<Id, usize> = movable
        .iter()
        .enumerate()
        .map(|(i, k)| (k.clone(), i))
        .collect();
    let mut start = movable
        .iter()
        .flat_map(|k| {
            assembly.instances[k]
                .placement
                .translation
                .into_iter()
                .chain([0.; 3])
        })
        .collect::<Vec<_>>();
    let mut parameters = BTreeMap::new();
    for m in &assembly.mates {
        if let Some(CurvePosition::Free { seed }) = m
            .relationship
            .as_deref()
            .and_then(Relationship::curve_position)
        {
            parameters.insert(m.id.clone(), start.len());
            start.push(*seed);
        }
    }
    let base = assembly.clone();
    let grouped = |x: &DVector<f64>| -> Result<Vec<(Id, Vec<f64>)>> {
        let mut groups = vec![];
        for mate in &base.mates {
            let a = assembly_matrix(&base, &index, x, &mate.first)?;
            let b = assembly_matrix(&base, &index, x, &mate.second)?;
            let pair = if let Some(p) = frames.get(&mate.id) {
                p.clone()
            } else {
                if mate.first_reference.is_some() || mate.second_reference.is_some() {
                    return Err(CadError::Invalid(
                        "Associative mate reference frames are missing".into(),
                    ));
                }
                [
                    crate::assembly_geometry::Frame::legacy(mate, true)?,
                    crate::assembly_geometry::Frame::legacy(mate, false)?,
                ]
            };
            let first = pair[0].transformed(&a)?;
            let second = pair[1].transformed(&b)?;
            let residual = if let Some(relationship) = &mate.relationship {
                let aux = auxiliary
                    .get(&mate.id)
                    .into_iter()
                    .flatten()
                    .map(|e| {
                        e.frame
                            .transformed(&assembly_matrix(&base, &index, x, &e.instance)?)
                    })
                    .collect::<Result<Vec<_>>>()?;
                if aux.len() != relationship.endpoints().len() {
                    return Err(CadError::Invalid(
                        "Advanced mate auxiliary reference frames are missing".into(),
                    ));
                }
                let support = if let Relationship::Coupler {
                    support: Some(id), ..
                } = relationship.as_ref()
                {
                    Some(assembly_matrix(&base, &index, x, id)?)
                } else {
                    None
                };
                crate::mate_solver::equations(crate::mate_solver::Context {
                    doc,
                    owner: &base.id,
                    mate,
                    first: &first,
                    second: &second,
                    auxiliary: &aux,
                    support: support.as_ref(),
                    parameter: parameters.get(&mate.id).map(|i| x[*i]),
                })
            } else {
                mate_equations(doc, &base.id, mate, &first, &second, &a, &b)
            }
            .map_err(|e| CadError::Invalid(format!("{} mate {}: {e}", mate.kind, mate.id)))?;
            groups.push((mate.id.clone(), residual));
        }
        for instance in base.instances.values() {
            if let Some(expressions) = &instance.translation_expressions {
                let matrix = assembly_matrix(&base, &index, x, &instance.id)?;
                let values = expressions
                    .iter()
                    .enumerate()
                    .map(|(axis, expression)| {
                        crate::expression::value(
                            doc,
                            &base.id,
                            expression,
                            crate::Dimension::Length,
                        )
                        .map(|target| matrix[(axis, 3)] - target)
                    })
                    .collect::<Result<Vec<_>>>()?;
                groups.push((format!("placement:{}", instance.id), values));
            }
        }
        Ok(groups)
    };
    let equations = |x: &DVector<f64>| -> Result<DVector<f64>> {
        Ok(DVector::from_vec(
            grouped(x)?.into_iter().flat_map(|(_, r)| r).collect(),
        ))
    };
    let start = DVector::from_vec(start);
    // Use a physical pose metric, rather than normalizing every derivative
    // column. A free coordinate's nearly-zero numerical derivative must not be
    // amplified into an enormous translation. Minimum-norm increments retain
    // unconstrained pose directions; the rank cutoff matches motion diagnostics.
    let metric = DVector::from_fn(start.len(), |coordinate, _| {
        if coordinate < movable.len() * 6 && coordinate % 6 < 3 {
            1.
        } else {
            100.
        }
    });
    let mut seed = start.clone();
    let mut drive_iterations = 0;
    if !goals.is_empty() {
        for goal in goals {
            if !index.contains_key(&goal.instance) {
                return Err(CadError::Invalid(
                    "The selected component is grounded".into(),
                ));
            }
            // Start at the requested pure translation/rotation, with the selected
            // anchor retained. For an unconstrained component this is already the
            // exact goal, so Move cannot turn into an accidental rotation about a
            // distant picked point. Remaining freedom prefers this requested pose.
            let i = index[&goal.instance];
            let baseline = &base.instances[&goal.instance].placement;
            let preferred = goal.preference(baseline);
            for axis in 0..3 {
                seed[i * 6 + axis] = preferred.translation[axis];
            }
            let baseline_matrix = baseline.matrix();
            let preferred_matrix = preferred.matrix();
            let relative = preferred_matrix.fixed_view::<3, 3>(0, 0)
                * baseline_matrix.fixed_view::<3, 3>(0, 0).transpose();
            let increment = Rotation3::from_matrix_unchecked(relative).scaled_axis();
            for axis in 0..3 {
                seed[i * 6 + 3 + axis] = increment[axis];
            }
        }
        let preference = seed.clone();
        // Engineering mates and travel bounds dominate transient cursor/dial
        // goals. A final strict projection checks every original equation before
        // returning a pose; impossible goals never relax or remove mates.
        let driven = |x: &DVector<f64>| -> Result<DVector<f64>> {
            let mut residual = equations(x)?.as_slice().to_vec();
            for value in &mut residual {
                *value *= 10_000.;
            }
            for goal in goals {
                residual.extend(goal.residual(&assembly_matrix(&base, &index, x, &goal.instance)?));
            }
            residual.extend(
                x.iter()
                    .zip(preference.iter())
                    .map(|(a, b)| (a - b) * 0.0001),
            );
            Ok(DVector::from_vec(residual))
        };
        let initial = if driven(&seed).is_ok() {
            seed
        } else {
            start.clone()
        };
        let (driven, report) = minimize_in_metric(initial, driven, 100, Some(&metric))?;
        seed = driven;
        drive_iterations = report.iterations;
    }
    let (x, mut report) = minimize_in_metric(seed, equations, 160, Some(&metric))?;
    report.iterations += drive_iterations;
    let groups = grouped(&x)?;
    if !report.converged {
        let conflicts = groups
            .iter()
            .filter(|(_, r)| r.iter().any(|v| v.abs() > RESIDUAL_TOLERANCE))
            .map(|(id, _)| {
                base.mates
                    .iter()
                    .find(|m| &m.id == id)
                    .map(|m| format!("{} ({id})", m.kind))
                    .unwrap_or_else(|| {
                        id.strip_prefix("placement:")
                            .and_then(|id| base.instances.get(id))
                            .map(|instance| format!("{} placement expressions", instance.name))
                            .unwrap_or_else(|| id.clone())
                    })
            })
            .collect::<Vec<_>>();
        return Err(CadError::Invalid(format!(
            "Assembly mates did not converge (residual {:.6}): {}",
            report.residual,
            conflicts.join(", ")
        )));
    }
    let residual = equations(&x)?;
    let mut j = jacobian(&x, &residual, &equations)?;
    let mut diagnostics = BTreeMap::new();
    let mut row = 0;
    let mut inequalities = 0;
    for (id, r) in groups {
        let bounds = base
            .mates
            .iter()
            .find(|m| m.id == id)
            .and_then(|m| m.relationship.as_deref())
            .map(|relationship| {
                crate::mate_solver::inequality_rows(doc, &base.id, relationship, r.len())
            })
            .transpose()?
            .unwrap_or_default();
        for bound in &bounds {
            j.row_mut(row + bound).fill(0.);
        }
        inequalities += bounds.len();
        let block = j.rows(row, r.len()).into_owned();
        let rank = matrix_rank(&block, Some(&metric));
        if base.mates.iter().any(|mate| mate.id == id) {
            diagnostics.insert(
                id.clone(),
                MateDiagnostic {
                    maximum_residual: r.iter().map(|v| v.abs()).fold(0., f64::max),
                    equation_count: r.len(),
                    independent_equations: rank,
                    redundant_equations: r.len().saturating_sub(bounds.len()).saturating_sub(rank),
                    inequality_equations: bounds.len(),
                    curve_parameter: parameters.get(&id).map(|i| x[*i]),
                },
            );
        }
        row += r.len();
    }
    let equality_rank = matrix_rank(&j, Some(&metric));
    report.degrees_of_freedom = x.len().saturating_sub(equality_rank);
    report.redundant_equations = residual
        .len()
        .saturating_sub(inequalities)
        .saturating_sub(equality_rank);
    // Project onto the linearized nullspace. Unlike listing unlocked Euler axes,
    // this includes correlated component motion and path parameters.
    let mut projector = DMatrix::identity(x.len(), x.len());
    if !j.is_empty() {
        let (scaled, scale) = jacobian_in_metric(&j, Some(&metric));
        let svd = scaled.svd(false, true);
        let vt = svd.v_t.as_ref().expect("requested right singular vectors");
        let cutoff = svd.singular_values.amax() * 1e-8;
        for k in 0..svd.singular_values.len() {
            if svd.singular_values[k] > cutoff {
                let v = vt.row(k).transpose();
                projector -= &v * v.transpose();
            }
        }
        for i in 0..projector.nrows() {
            projector.row_mut(i).scale_mut(1. / scale[i]);
        }
    }
    // Normalize nullspace generators after undoing variable scaling so diagnostic
    // directions are independent of arbitrary damping/Jacobian column magnitudes.
    for col in 0..projector.ncols() {
        let norm = projector.column(col).norm();
        if norm > 1e-9 {
            projector.column_mut(col).scale_mut(1. / norm);
        } else {
            projector.column_mut(col).fill(0.);
        }
    }
    let mut component_motion = BTreeMap::new();
    for id in base.instances.keys() {
        let motion = if let Some(i) = index.get(id) {
            let rows = projector.rows(i * 6, 6).into_owned();
            let translations = rows.rows(0, 3).into_owned();
            let mut rotation_map = nalgebra::Matrix3::zeros();
            let current = assembly_matrix(&base, &index, &x, id)?;
            for axis in 0..3 {
                let mut shifted = x.clone();
                shifted[i * 6 + 3 + axis] += 1e-6;
                let next = assembly_matrix(&base, &index, &shifted, id)?;
                let r =
                    next.fixed_view::<3, 3>(0, 0) * current.fixed_view::<3, 3>(0, 0).transpose();
                rotation_map.set_column(
                    axis,
                    &(Rotation3::from_matrix_unchecked(r).scaled_axis() / 1e-6),
                );
            }
            let rotational = rotation_map * rows.rows(3, 3);
            let rotations = DMatrix::from_fn(3, rotational.ncols(), |r, c| rotational[(r, c)]);
            ComponentMotion {
                grounded: false,
                translation_directions: motion_directions(&translations),
                rotation_directions: motion_directions(&rotations),
                degrees_of_freedom: matrix_rank(&rows, None),
            }
        } else {
            ComponentMotion {
                grounded: true,
                translation_directions: vec![],
                rotation_directions: vec![],
                degrees_of_freedom: 0,
            }
        };
        component_motion.insert(id.clone(), motion);
    }
    let mut candidate = assembly.clone();
    for (i, key) in movable.iter().enumerate() {
        let matrix = assembly_matrix(&base, &index, &x, key)?;
        let rotation =
            Rotation3::from_matrix_unchecked(matrix.fixed_view::<3, 3>(0, 0).into_owned());
        let (rx, ry, rz) = rotation.euler_angles();
        candidate
            .instances
            .get_mut(key)
            .expect("instance")
            .placement = Placement {
            translation: std::array::from_fn(|j| x[i * 6 + j]),
            rotation_deg: [rx, ry, rz].map(f64::to_degrees),
        };
    }
    *assembly = candidate;
    Ok(AssemblySolution {
        report,
        diagnostics,
        component_motion,
    })
}

pub(crate) fn aligned_axes(
    a: Vector3<f64>,
    b: Vector3<f64>,
    secondary: Vector3<f64>,
    alignment: crate::assembly_geometry::Alignment,
) -> Vector3<f64> {
    use crate::assembly_geometry::Alignment;
    if alignment == Alignment::Either {
        return axis_residual(a, b);
    }
    let b = if alignment == Alignment::Opposed {
        -b
    } else {
        b
    };
    let cross = a.cross(&b);
    let sine = cross.norm();
    let cosine = a.dot(&b).clamp(-1., 1.);
    if sine < 1e-12 {
        if cosine < 0. {
            secondary * std::f64::consts::PI * 100.
        } else {
            cross * 100.
        }
    } else {
        cross * (100. * sine.atan2(cosine) / sine)
    }
}
fn mate_equations(
    doc: &Document,
    assembly: &str,
    mate: &crate::Mate,
    first: &crate::assembly_geometry::Frame,
    second: &crate::assembly_geometry::Frame,
    a: &Matrix4<f64>,
    b: &Matrix4<f64>,
) -> Result<Vec<f64>> {
    use crate::assembly_geometry::{Alignment, FrameKind};
    if mate.kind != "lock"
        && (matches!(first.kind, FrameKind::Curve | FrameKind::Profile)
            || matches!(second.kind, FrameKind::Curve | FrameKind::Profile))
    {
        return Err(CadError::Invalid(
            "Sketch curve references require a Slot, Path, or Cam mate".into(),
        ));
    }
    let av = Vector3::from(first.axis);
    let bv = Vector3::from(second.axis);
    let au = Vector3::from(first.secondary);
    let bu = Vector3::from(second.secondary);
    let delta = Vector3::from(second.origin) - Vector3::from(first.origin);
    let mut residual = Vec::new();
    let plane = if first.kind == FrameKind::Plane {
        Some(av)
    } else if second.kind == FrameKind::Plane {
        Some(bv)
    } else {
        None
    };
    let axis = if first.kind == FrameKind::Axis {
        Some(av)
    } else if second.kind == FrameKind::Axis {
        Some(bv)
    } else {
        None
    };
    let directional = first.kind != FrameKind::Point && second.kind != FrameKind::Point;
    let both_planes = first.kind == FrameKind::Plane && second.kind == FrameKind::Plane;
    let mixed_plane_axis = plane.is_some() && axis.is_some();
    let alignment = aligned_axes(av, bv, au, mate.alignment);
    let orthogonal = (av.cross(&bv).norm().atan2(av.dot(&bv)) - std::f64::consts::FRAC_PI_2) * 100.;
    let offset = if matches!(
        mate.kind.as_str(),
        "coincident" | "concentric" | "distance" | "planar"
    ) {
        expression::value(doc, assembly, &mate.offset, Dimension::Length)?
    } else {
        0.
    };
    match mate.kind.as_str() {
        "coincident" => {
            if let Some(n) = plane {
                residual.push(delta.dot(&n) - offset);
                if both_planes {
                    residual.extend(alignment.iter().copied());
                } else if axis.is_some() {
                    residual.push(orthogonal);
                }
            } else if let Some(n) = axis {
                residual.extend(delta.cross(&n).iter().copied());
                if directional {
                    residual.extend(alignment.iter().copied());
                }
            } else {
                residual.extend((delta - av * offset).iter().copied());
            }
        }
        "planar" => {
            if !directional
                || !matches!(first.kind, FrameKind::Plane | FrameKind::Legacy)
                || !matches!(second.kind, FrameKind::Plane | FrameKind::Legacy)
            {
                return Err(CadError::Invalid(
                    "Planar mate requires two planes or legacy frames".into(),
                ));
            }
            residual.push(delta.dot(&av) - offset);
            residual.extend(alignment.iter().copied());
        }
        "parallel" => {
            if !directional {
                return Err(CadError::Invalid(
                    "Parallel mate requires planes or axes".into(),
                ));
            }
            if mixed_plane_axis {
                residual.push(orthogonal);
            } else {
                residual.extend(alignment.iter().copied());
            }
        }
        "perpendicular" => {
            if !directional {
                return Err(CadError::Invalid(
                    "Perpendicular mate requires planes or axes".into(),
                ));
            }
            if mixed_plane_axis {
                residual.extend(alignment.iter().copied());
            } else {
                residual.push(orthogonal);
            }
        }
        "concentric" => {
            if first.kind == FrameKind::Plane || second.kind == FrameKind::Plane || !directional {
                return Err(CadError::Invalid("Concentric mate requires two axes, straight edges, centerlines, or legacy axis frames".into()));
            }
            residual.extend(delta.cross(&av).iter().copied());
            residual.extend(alignment.iter().copied());
        }
        "distance" => {
            if let Some(n) = plane {
                residual.push(delta.dot(&n) - offset);
                if both_planes {
                    residual.extend(alignment.iter().copied());
                } else if axis.is_some() {
                    residual.push(orthogonal);
                }
            } else {
                if offset < 0. {
                    return Err(CadError::Invalid(
                        "Point/axis distance must be nonnegative".into(),
                    ));
                }
                if let Some(n) = axis {
                    residual.push(delta.cross(&n).norm() - offset);
                    if directional {
                        residual.extend(alignment.iter().copied());
                    }
                } else {
                    residual.push(delta.norm() - offset);
                }
            }
        }
        "angle" => {
            if !directional {
                return Err(CadError::Invalid(
                    "Angle mate requires planes or axes".into(),
                ));
            }
            let target = expression::value(doc, assembly, &mate.offset, Dimension::Angle)?;
            if target < 0. || target > std::f64::consts::PI {
                return Err(CadError::Invalid(
                    "Mate angle must be 0..180 degrees".into(),
                ));
            }
            let bv = if mate.alignment == Alignment::Opposed {
                -bv
            } else {
                bv
            };
            if mixed_plane_axis {
                if target > std::f64::consts::FRAC_PI_2 {
                    return Err(CadError::Invalid(
                        "Line-to-plane mate angle must be 0..90 degrees".into(),
                    ));
                }
                residual.push((av.dot(&bv).abs().atan2(av.cross(&bv).norm()) - target) * 100.);
            } else {
                residual.push((av.cross(&bv).norm().atan2(av.dot(&bv)) - target) * 100.);
            }
        }
        "lock" => {
            let relative = mate.lock_placement.as_ref().ok_or_else(|| {
                CadError::Invalid("Lock mate needs a captured relative component placement".into())
            })?;
            let target = a * relative.matrix();
            let d = b.fixed_view::<3, 1>(0, 3) - target.fixed_view::<3, 1>(0, 3);
            residual.extend(d.iter().copied());
            let r = target.fixed_view::<3, 3>(0, 0).transpose() * b.fixed_view::<3, 3>(0, 0);
            residual.extend(
                (Rotation3::from_matrix_unchecked(r).scaled_axis() * 100.)
                    .iter()
                    .copied(),
            );
        }
        _ => {
            return Err(CadError::Unsupported(format!(
                "Assembly mate {}",
                mate.kind
            )));
        }
    }
    if mate.lock_rotation {
        if mate.kind != "concentric" {
            return Err(CadError::Invalid(
                "Rotation locking currently applies to Concentric mates".into(),
            ));
        }
        let theta = expression::value(doc, assembly, &mate.rotation_offset, Dimension::Angle)?;
        let observed = av.dot(&au.cross(&bu)).atan2(au.dot(&bu));
        let error = (observed - theta + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU)
            - std::f64::consts::PI;
        residual.push(error * 100.);
    }
    Ok(residual)
}
