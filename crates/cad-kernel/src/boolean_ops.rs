//! Set semantics and admission for the native Boolean boundary.
//!
//! An empty set is a successful Boolean result, but is never stored as an
//! empty B-rep solid. Collections are unions of their member material regions.
use cad_core::{CadError, Result};
use monstertruck::modeling::*;
use std::collections::HashSet;

#[derive(Clone, Copy)]
pub(crate) enum Mode {
    Union,
    Difference,
    Intersection,
}

fn invalid(message: impl std::fmt::Display) -> CadError {
    CadError::Invalid(format!("Geometry: Boolean: {message}"))
}

fn same(a: &Solid, b: &Solid) -> bool {
    let faces = |solid: &Solid| {
        solid
            .face_iter()
            .map(|face| (face.id(), face.orientation()))
            .collect::<HashSet<_>>()
    };
    faces(a) == faces(b)
}

pub(crate) fn disjoint(a: &Solid, b: &Solid) -> bool {
    let enclosure = |body| -> Result<Vec<Point3>> {
        Ok(crate::native_bounds::solid(body)?
            .iter()
            .flat_map(|face| face.points().iter().copied())
            .collect())
    };
    enclosure(a)
        .ok()
        .zip(enclosure(b).ok())
        .is_some_and(|(a, b)| {
            !a.is_empty()
                && !b.is_empty()
                && a.iter()
                    .chain(&b)
                    .all(|point| [point.x, point.y, point.z].iter().all(|x| x.is_finite()))
                && crate::extrusion_surface::bounds_disjoint(&a, &b, 0.)
        })
}

pub(crate) fn binary(a: &Solid, b: &Solid, mode: Mode, tolerance: f64) -> Result<Option<Solid>> {
    binary_impl(a, b, mode, tolerance, None)
}

pub(crate) fn binary_with_history(
    a: &Solid,
    b: &Solid,
    mode: Mode,
    tolerance: f64,
    scope: &std::collections::BTreeSet<cad_core::NodeRef>,
    context: crate::revolution::Combination<'_>,
) -> Result<Option<Solid>> {
    let output = binary_impl(
        a,
        b,
        mode,
        tolerance,
        Some((
            scope,
            crate::revolution::Combination {
                adapter: context.adapter,
                owner: context.owner,
                generated: context.generated,
            },
        )),
    )?;
    if let Some(solid) = &output {
        context.adapter.carry_boolean_history(
            context.owner,
            scope,
            [a, b],
            solid,
            context.generated,
        )?;
    }
    Ok(output)
}

fn binary_impl(
    a: &Solid,
    b: &Solid,
    mode: Mode,
    tolerance: f64,
    context: Option<(
        &std::collections::BTreeSet<cad_core::NodeRef>,
        crate::revolution::Combination<'_>,
    )>,
) -> Result<Option<Solid>> {
    if !tolerance.is_finite() || tolerance <= 0. {
        return Err(invalid("Tolerance must be finite and positive"));
    }
    if a.is_empty() || b.is_empty() {
        let result = match mode {
            Mode::Union => {
                if a.is_empty() {
                    b
                } else {
                    a
                }
            }
            Mode::Difference => a,
            Mode::Intersection => return Ok(None),
        };
        return Ok((!result.is_empty()).then(|| result.clone()));
    }
    if same(a, b) {
        return Ok(match mode {
            Mode::Difference => None,
            Mode::Union | Mode::Intersection => Some(a.clone()),
        });
    }
    if disjoint(a, b) {
        match mode {
            Mode::Difference => return Ok(Some(a.clone())),
            Mode::Intersection => return Ok(None),
            Mode::Union => {
                let shells = a
                    .boundaries()
                    .iter()
                    .chain(b.boundaries())
                    .cloned()
                    .collect();
                return Solid::try_new(shells).map(Some).map_err(invalid);
            }
        }
    }
    // Layered reconstruction creates explicit partitions. Only feature paths
    // carrying their construction history may publish those descendants.
    if let Some((scope, context)) = context {
        if let Some(output) = crate::prismatic_boolean::binary(a, b, mode)? {
            crate::prismatic_boolean::record_history(&output, scope, context)?;
            return Ok(output.solid);
        }
        if matches!(mode, Mode::Difference)
            && let Some(output) = crate::local_pockets::difference(a, b)?
        {
            crate::local_pockets::record_history(&output, scope, context)?;
            return local_stock(output);
        }
    } else if matches!(mode, Mode::Difference)
        && let Some(output) = crate::local_pockets::difference(a, b)?
    {
        return local_stock(output);
    }
    let output = match mode {
        Mode::Union => monstertruck::solid::or(a, b, tolerance),
        Mode::Difference => monstertruck::solid::difference(a, b, tolerance),
        Mode::Intersection => monstertruck::solid::and(a, b, tolerance),
    }
    .map_err(invalid)?;
    if output.is_empty() {
        return Ok(None);
    }
    crate::validation::solid(&output)?;
    Ok(Some(output))
}

fn local_stock(mut output: crate::planar::CutOutput) -> Result<Option<Solid>> {
    if output.bodies.len() != 1 {
        return Err(invalid(
            "A contained local pocket must retain exactly one native stock container",
        ));
    }
    Ok(Some(output.bodies.remove(0)))
}

pub(crate) fn union(a: &Solid, b: &Solid, tolerance: f64) -> Result<Solid> {
    binary(a, b, Mode::Union, tolerance)?
        .ok_or_else(|| invalid("Union of nonempty material regions returned no material"))
}

pub(crate) fn union_with_history(
    a: &Solid,
    b: &Solid,
    tolerance: f64,
    scope: &std::collections::BTreeSet<cad_core::NodeRef>,
    context: crate::revolution::Combination<'_>,
) -> Result<Solid> {
    binary_with_history(a, b, Mode::Union, tolerance, scope, context)?
        .ok_or_else(|| invalid("Union of nonempty material regions returned no material"))
}

pub(crate) fn sets_with_history(
    left: &[Solid],
    right: &[Solid],
    mode: Mode,
    tolerance: f64,
    scope: &std::collections::BTreeSet<cad_core::NodeRef>,
    context: crate::revolution::Combination<'_>,
) -> Result<Vec<Solid>> {
    if !tolerance.is_finite() || tolerance <= 0. {
        return Err(invalid("Tolerance must be finite and positive"));
    }
    if matches!(mode, Mode::Union) {
        return union_with_contact(left, right, tolerance, scope, context);
    }
    if left.len().saturating_add(right.len()).saturating_pow(2) > 65_536 {
        return Err(CadError::Unsupported(
            "Boolean body collection exceeds its bounded operation work".into(),
        ));
    }
    let mut result = Vec::new();
    for body in left {
        let mut remaining = Some(body.clone());
        for tool in right {
            let stock = if matches!(mode, Mode::Difference) {
                let Some(stock) = remaining.as_ref() else {
                    break;
                };
                stock
            } else {
                body
            };
            let output = binary_with_history(
                stock,
                tool,
                mode,
                tolerance,
                scope,
                crate::revolution::Combination {
                    adapter: context.adapter,
                    owner: context.owner,
                    generated: context.generated,
                },
            )?;
            if matches!(mode, Mode::Difference) {
                remaining = output;
            } else if let Some(output) = output {
                insert_union(&mut result, output, &mut |a, b| {
                    union_with_history(
                        a,
                        b,
                        tolerance,
                        scope,
                        crate::revolution::Combination {
                            adapter: context.adapter,
                            owner: context.owner,
                            generated: context.generated,
                        },
                    )
                })?;
            }
        }
        if matches!(mode, Mode::Difference)
            && let Some(output) = remaining
        {
            insert_union(&mut result, output, &mut |a, b| {
                union_with_history(
                    a,
                    b,
                    tolerance,
                    scope,
                    crate::revolution::Combination {
                        adapter: context.adapter,
                        owner: context.owner,
                        generated: context.generated,
                    },
                )
            })?;
        }
    }
    Ok(result)
}

fn insert_union(
    result: &mut Vec<Solid>,
    mut tool: Solid,
    join: &mut impl FnMut(&Solid, &Solid) -> Result<Solid>,
) -> Result<()> {
    if tool.is_empty() {
        return Ok(());
    }
    let mut retained = Vec::new();
    for body in result.drain(..) {
        if disjoint(&body, &tool) {
            retained.push(body);
        } else {
            tool = join(&body, &tool)?;
        }
    }
    retained.push(tool);
    *result = retained;
    Ok(())
}

pub(crate) fn sets(
    left: &[Solid],
    right: &[Solid],
    mode: Mode,
    tolerance: f64,
) -> Result<Vec<Solid>> {
    sets_using(left, right, mode, tolerance, &mut |a, b| {
        union(a, b, tolerance)
    })
}

pub(crate) fn union_with_contact(
    left: &[Solid],
    right: &[Solid],
    tolerance: f64,
    scope: &std::collections::BTreeSet<cad_core::NodeRef>,
    context: crate::revolution::Combination<'_>,
) -> Result<Vec<Solid>> {
    sets_using(left, right, Mode::Union, tolerance, &mut |a, b| {
        let contact = if let Some(contact) = crate::join::contact(a, b)? {
            Some(contact)
        } else {
            crate::join::contact(b, a)?
        };
        if let Some(contact) = contact {
            crate::join::record_history(
                context.adapter,
                context.owner,
                scope,
                &contact,
                context.generated,
            )?;
            Ok(contact.solid)
        } else {
            union_with_history(
                a,
                b,
                tolerance,
                scope,
                crate::revolution::Combination {
                    adapter: context.adapter,
                    owner: context.owner,
                    generated: context.generated,
                },
            )
        }
    })
}

fn sets_using(
    left: &[Solid],
    right: &[Solid],
    mode: Mode,
    tolerance: f64,
    join: &mut impl FnMut(&Solid, &Solid) -> Result<Solid>,
) -> Result<Vec<Solid>> {
    if !tolerance.is_finite() || tolerance <= 0. {
        return Err(invalid("Tolerance must be finite and positive"));
    }
    let count = left.len().saturating_add(right.len());
    if count.saturating_mul(count) > 65536 {
        return Err(CadError::Unsupported(
            "Boolean body collection exceeds its bounded operation work".into(),
        ));
    }
    let mut result = Vec::new();
    match mode {
        Mode::Union => {
            for body in left.iter().chain(right) {
                insert_union(&mut result, body.clone(), join)?;
            }
        }
        Mode::Difference => {
            for body in left {
                let mut remaining = Some(body.clone());
                for tool in right {
                    let Some(stock) = remaining else { break };
                    remaining = binary(&stock, tool, Mode::Difference, tolerance)?;
                }
                if let Some(body) = remaining {
                    insert_union(&mut result, body, join)?;
                }
            }
        }
        Mode::Intersection => {
            for body in left {
                for tool in right {
                    if let Some(common) = binary(body, tool, Mode::Intersection, tolerance)? {
                        insert_union(&mut result, common, join)?;
                    }
                }
            }
        }
    }
    Ok(result)
}
