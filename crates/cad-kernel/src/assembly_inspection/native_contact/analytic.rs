//! Exact support signs of native analytic carriers. Rotation is expanded as
//! Rodrigues coefficients; no normalized floating-point axis or sampled angle
//! is used to decide tangency.
use super::*;

fn sum(values: impl IntoIterator<Item = Exact>) -> Exact {
    values.into_iter().fold(Exact::zero(), |a, b| a.add(&b))
}
fn dot(a: &[Exact; 3], b: &[Exact; 3]) -> Exact {
    sum((0..3).map(|i| a[i].mul(&b[i])))
}
fn square(value: &Exact) -> Exact {
    value.mul(value)
}
fn vector(value: Vector3) -> Result<[Exact; 3]> {
    Ok([
        Exact::new(value.x)?,
        Exact::new(value.y)?,
        Exact::new(value.z)?,
    ])
}
fn affine(transform: &Matrix4) -> bool {
    transform.x.w == 0. && transform.y.w == 0. && transform.z.w == 0. && transform.w.w == 1.
}
fn transformed_support(support: &Support, transform: &Matrix4) -> Result<([Exact; 3], Exact)> {
    let mut normal = [Exact::zero(), Exact::zero(), Exact::zero()];
    for i in 0..3 {
        for j in 0..3 {
            normal[i] = normal[i].add(&support.equation[j].mul(&Exact::new(transform[i][j])?));
        }
    }
    let mut offset = support.equation[3].clone();
    for i in 0..3 {
        offset = offset.add(&support.equation[i].mul(&Exact::new(transform.w[i])?));
    }
    Ok((normal, offset))
}
// Compare nonnegative |center| with sqrt(a) + sqrt(b), without computing a
// floating square root. The sign guards make both squarings equivalences.
fn compare_radius(center: &Exact, a: &Exact, b: &Exact) -> Ordering {
    let residual = square(center).sub(a).sub(b);
    if residual.sign() == Ordering::Less {
        return Ordering::Less;
    }
    square(&residual)
        .sub(&a.mul(b).add(&a.mul(b)).add(&a.mul(b)).add(&a.mul(b)))
        .sign()
}
fn range(center: &Exact, a: &Exact, b: &Exact) -> Sides {
    let comparison = compare_radius(center, a, b);
    if a.sign() == Ordering::Equal && b.sign() == Ordering::Equal {
        let mut sides = Sides::default();
        sides.add(center.sign());
        return sides;
    }
    match (center.sign(), comparison) {
        (_, Ordering::Less) | (Ordering::Equal, _) => Sides {
            negative: true,
            positive: true,
            zero: false,
        },
        (Ordering::Less, ordering) => Sides {
            negative: true,
            positive: false,
            zero: ordering == Ordering::Equal,
        },
        (Ordering::Greater, ordering) => Sides {
            negative: false,
            positive: true,
            zero: ordering == Ordering::Equal,
        },
    }
}
fn origin_term(normal: &[Exact; 3], offset: &Exact, point: Point3) -> Result<Exact> {
    Ok(dot(normal, &vector(point.to_vec())?).add(offset))
}

pub(super) fn sides(
    face: &NativeFace,
    support: &Support,
    budget: &mut Budget,
) -> Result<Option<Sides>> {
    match face.face.surface() {
        Surface::SphericalSurface(processor) if affine(processor.transform()) => {
            budget.spend(32)?;
            let (normal, offset) = transformed_support(support, processor.transform())?;
            let center = origin_term(&normal, &offset, processor.entity().center())?;
            let radius = Exact::new(processor.entity().radius())?;
            Ok(Some(range(
                &center,
                &square(&radius).mul(&dot(&normal, &normal)),
                &Exact::zero(),
            )))
        }
        Surface::ToroidalSurface(processor) if affine(processor.transform()) => {
            budget.spend(48)?;
            let (normal, offset) = transformed_support(support, processor.transform())?;
            let center = origin_term(&normal, &offset, processor.entity().center())?;
            let major = Exact::new(processor.entity().large_radius())?;
            let minor = Exact::new(processor.entity().small_radius())?;
            let radial = square(&normal[0]).add(&square(&normal[1]));
            Ok(Some(range(
                &center,
                &square(&major).mul(&radial),
                &square(&minor).mul(&dot(&normal, &normal)),
            )))
        }
        Surface::RevolutionSurface(processor) if affine(processor.transform()) => {
            let (normal, offset) = transformed_support(support, processor.transform())?;
            let axis = vector(processor.entity().axis())?;
            let origin = vector(processor.entity().origin().to_vec())?;
            let center = dot(&normal, &origin).add(&offset);
            let curve = processor.entity().entity_curve();
            let controls = if let Curve::Line(Line(front, back)) = curve {
                // A line carrier is unbounded. Restrict its generator parameter
                // with the native face-domain control hull, including holes.
                let coordinate = usize::from(!processor.orientation());
                let Some(parameters) = generator_controls(face, coordinate, budget)? else {
                    return Ok(None);
                };
                let a = vector(front.to_vec())?;
                let b = vector(back.to_vec())?;
                let mut controls = Vec::new();
                for uv in parameters {
                    budget.spend(12)?;
                    let weight = &uv.0[3];
                    let parameter = &uv.0[coordinate];
                    let mut p =
                        Homogeneous([Exact::zero(), Exact::zero(), Exact::zero(), weight.clone()]);
                    for i in 0..3 {
                        p.0[i] = a[i].mul(weight).add(&b[i].sub(&a[i]).mul(parameter));
                    }
                    controls.push(p);
                }
                controls
            } else {
                let Some(curve) = native_curve(curve, budget)? else {
                    return Ok(None);
                };
                curve.controls
            };
            if controls.is_empty() {
                return Ok(None);
            }
            let mut result = Sides::default();
            for control in controls {
                budget.spend(64)?;
                let v: [Exact; 3] =
                    std::array::from_fn(|i| control.0[i].sub(&origin[i].mul(&control.0[3])));
                let axial = dot(&axis, &v);
                let along: [Exact; 3] = std::array::from_fn(|i| axis[i].mul(&axial));
                let cosine: [Exact; 3] = std::array::from_fn(|i| v[i].sub(&along[i]));
                let sine: [Exact; 3] = std::array::from_fn(|i| {
                    axis[(i + 1) % 3]
                        .mul(&v[(i + 2) % 3])
                        .sub(&axis[(i + 2) % 3].mul(&v[(i + 1) % 3]))
                });
                let p = center.mul(&control.0[3]).add(&dot(&normal, &along));
                let c = dot(&normal, &cosine);
                let s = dot(&normal, &sine);
                result.merge(range(&p, &square(&c).add(&square(&s)), &Exact::zero()));
                if result.straddles() {
                    break;
                }
            }
            Ok(Some(result))
        }
        _ => Ok(None),
    }
}
