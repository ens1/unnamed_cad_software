//! Exact ruled and endpoint-controlled lofts with shared section boundaries.
use cad_core::{CadError, Result};
use monstertruck::modeling::*;

fn invalid(message: impl ToString) -> CadError {
    CadError::Invalid(format!("Geometry: {}", message.to_string()))
}

fn anchor(face: &Face) -> Result<Point3> {
    face.vertex_iter()
        .next()
        .map(|vertex| vertex.point())
        .ok_or_else(|| invalid("An empty profile cannot define a solid section"))
}

const EPS: f64 = 1e-7;
fn unsupported(message: &str) -> CadError {
    CadError::Unsupported(format!("Section geometry: {message}"))
}
fn point(h: Vector4) -> Result<Point3> {
    if ![h.x, h.y, h.z, h.w].iter().all(|value| value.is_finite()) || h.w <= 0. {
        return Err(unsupported("finite positive rational weights are required"));
    }
    Ok(Point3::new(h.x / h.w, h.y / h.w, h.z / h.w))
}
fn compatible(a: &Curve, b: &Curve) -> Result<(BsplineCurve<Vector4>, BsplineCurve<Vector4>)> {
    crate::loft_correspondence::compatible(a, b)
}
fn straight(edge: &Edge) -> Result<bool> {
    let a = edge.front().point();
    let d = edge.back().point() - a;
    let length = d.magnitude2();
    if length <= EPS * EPS {
        return Ok(false);
    }
    Ok(crate::extrusion::curve_hull(&edge.curve())?
        .iter()
        .all(|point| {
            let t = (*point - a).dot(d) / length;
            (-1e-9..=1. + 1e-9).contains(&t) && (*point - a - d * t).magnitude() <= EPS
        }))
}
fn determinant_coefficients(
    a0: Vector3,
    a1: Vector3,
    b0: Vector3,
    b1: Vector3,
    n: Vector3,
) -> [f64; 3] {
    [
        a0.cross(b0).dot(n),
        (a0.cross(b1) + a1.cross(b0)).dot(n) / 2.,
        a1.cross(b1).dot(n),
    ]
}
fn convex_family(a: &[Wire], b: &[Wire], normal: Vector3) -> Result<bool> {
    if a.iter()
        .chain(b)
        .flatten()
        .map(straight)
        .collect::<Result<Vec<_>>>()?
        .iter()
        .any(|straight| !straight)
    {
        return Ok(false);
    }
    let pa = a
        .iter()
        .map(|wire| {
            wire.iter()
                .map(|edge| edge.front().point())
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let pb = b
        .iter()
        .map(|wire| {
            wire.iter()
                .map(|edge| edge.front().point())
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    for (contour, (a, b)) in pa.iter().zip(&pb).enumerate() {
        let n = normal * if contour == 0 { 1. } else { -1. };
        for i in 0..a.len() {
            let next = (i + 1) % a.len();
            for j in 0..a.len() {
                if j == i || j == next {
                    continue;
                }
                if determinant_coefficients(
                    a[next] - a[i],
                    b[next] - b[i],
                    a[j] - a[i],
                    b[j] - b[i],
                    n,
                )
                .iter()
                .any(|coefficient| *coefficient < -EPS)
                {
                    return Ok(false);
                }
            }
        }
    }
    for (ha, hb) in pa.iter().zip(&pb).skip(1) {
        for i in 0..pa[0].len() {
            let next = (i + 1) % pa[0].len();
            for (a, b) in ha.iter().zip(hb) {
                if determinant_coefficients(
                    pa[0][next] - pa[0][i],
                    pb[0][next] - pb[0][i],
                    *a - pa[0][i],
                    *b - pb[0][i],
                    normal,
                )
                .iter()
                .any(|coefficient| *coefficient <= EPS)
                {
                    return Ok(false);
                }
            }
        }
    }
    for i in 1..pa.len() {
        for j in i + 1..pa.len() {
            let separated = [
                (&pa[i], &pb[i], &pa[j], &pb[j]),
                (&pa[j], &pb[j], &pa[i], &pb[i]),
            ]
            .iter()
            .any(|(aa, ab, ba, bb)| {
                aa.iter()
                    .zip(aa.iter().cycle().skip(1))
                    .take(aa.len())
                    .chain(ab.iter().zip(ab.iter().cycle().skip(1)).take(ab.len()))
                    .any(|(p, q)| {
                        let axis = normal.cross(*q - *p).normalize();
                        [(aa, ba), (ab, bb)].iter().all(|(a, b)| {
                            a.iter()
                                .map(|p| p.to_vec().dot(axis))
                                .fold(f64::NEG_INFINITY, f64::max)
                                < b.iter()
                                    .map(|p| p.to_vec().dot(axis))
                                    .fold(f64::INFINITY, f64::min)
                                    - EPS
                        })
                    })
            });
            if !separated {
                return Ok(false);
            }
        }
    }
    Ok(true)
}
fn concentric_family(a: &[Wire], b: &[Wire], normal: Vector3) -> Result<bool> {
    let circles = |wires: &[Wire]| {
        wires
            .iter()
            .map(|wire| {
                let first = crate::axis_geometry::circle_geometry(&wire.front()?.curve()).ok()?;
                wire.iter()
                    .all(|edge| {
                        crate::axis_geometry::circle_geometry(&edge.curve())
                            .ok()
                            .is_some_and(|circle| {
                                circle.0.distance(first.0) <= EPS
                                    && (circle.2 - first.2).abs() <= EPS
                                    && circle.1.cross(first.1).magnitude() <= 1e-8
                            })
                    })
                    .then_some(first)
            })
            .collect::<Option<Vec<_>>>()
    };
    let (Some(ca), Some(cb)) = (circles(a), circles(b)) else {
        return Ok(false);
    };
    if ca.len() > 2
        || ![&ca, &cb].iter().all(|circles| {
            circles.iter().skip(1).all(|(center, _, radius)| {
                center.distance(circles[0].0) <= EPS && *radius < circles[0].2 - EPS
            })
        })
    {
        return Ok(false);
    }
    let mut radial = Vec::new();
    for ((a, b), (ca, cb)) in a.iter().zip(b).zip(ca.iter().zip(&cb)) {
        let pairs = a
            .iter()
            .zip(b)
            .map(|(a, b)| compatible(&a.oriented_curve(), &b.oriented_curve()))
            .collect::<Result<Vec<_>>>()?;
        if !affine_family(&pairs, normal)? {
            return Ok(false);
        }
        radial.push((a[0].front().point() - ca.0, b[0].front().point() - cb.0));
    }
    if radial.len() == 2 {
        let (a, b) = radial[0];
        let (c, d) = radial[1];
        if [
            a.magnitude2() - c.magnitude2(),
            a.dot(b) - c.dot(d),
            b.magnitude2() - d.magnitude2(),
        ]
        .iter()
        .any(|coefficient| *coefficient <= EPS * EPS)
        {
            return Ok(false);
        }
    }
    Ok(true)
}
fn affine_family(
    pairs: &[(BsplineCurve<Vector4>, BsplineCurve<Vector4>)],
    normal: Vector3,
) -> Result<bool> {
    let mut a = Vec::new();
    let mut b = Vec::new();
    for (lower, upper) in pairs {
        for (l, u) in lower.control_points().iter().zip(upper.control_points()) {
            a.push(point(*l)?);
            b.push(point(*u)?);
        }
    }
    let u = normal
        .cross(if normal.x.abs() < 0.8 {
            Vector3::unit_x()
        } else {
            Vector3::unit_y()
        })
        .normalize();
    let v = normal.cross(u);
    let xy = |p: Point3, base: Point3| {
        let p = p - base;
        Vector2::new(p.dot(u), p.dot(v))
    };
    let Some(i) = (1..a.len()).find(|i| xy(a[*i], a[0]).magnitude2() > EPS * EPS) else {
        return Ok(false);
    };
    let x1 = xy(a[i], a[0]);
    let y1 = xy(b[i], b[0]);
    let Some(j) = (1..a.len()).find(|j| {
        let x = xy(a[*j], a[0]);
        (x1.x * x.y - x1.y * x.x).abs() > EPS * EPS
    }) else {
        return Ok(false);
    };
    let x2 = xy(a[j], a[0]);
    let y2 = xy(b[j], b[0]);
    let det = x1.x * x2.y - x1.y * x2.x;
    let m00 = (y1.x * x2.y - y2.x * x1.y) / det;
    let m01 = (y2.x * x1.x - y1.x * x2.x) / det;
    let m10 = (y1.y * x2.y - y2.y * x1.y) / det;
    let m11 = (y2.y * x1.x - y1.y * x2.x) / det;
    let trace = m00 + m11;
    let determinant = m00 * m11 - m01 * m10;
    let coefficient_a = determinant - trace + 1.;
    let coefficient_b = trace - 2.;
    let critical = if coefficient_a > 0. {
        -coefficient_b / (2. * coefficient_a)
    } else {
        -1.
    };
    let minimum = if (0. ..1.).contains(&critical) {
        1. + coefficient_b * critical + coefficient_a * critical * critical
    } else {
        1f64.min(determinant)
    };
    if !minimum.is_finite() || minimum <= 1e-10 {
        return Ok(false);
    }
    let abase = a[0];
    let bbase = b[0];
    Ok(a.iter().zip(&b).all(|(a, b)| {
        let x = xy(*a, abase);
        let y = xy(*b, bbase);
        Vector2::new(m00 * x.x + m01 * x.y - y.x, m10 * x.x + m11 * x.y - y.y).magnitude() <= EPS
    }))
}
pub(crate) fn loft(
    faces: &[Face],
    alignments: &[Vec<Option<Point3>>],
    start: Option<Vector3>,
    end: Option<Vector3>,
    origins: &[std::collections::HashMap<EdgeId, crate::section_history::Member>],
    history: &mut crate::section_history::History<'_>,
) -> Result<Solid> {
    if faces.len() < 2 || alignments.len() != faces.len() || origins.len() != faces.len() {
        return Err(invalid(
            "Loft needs two or more compatible aligned sections",
        ));
    }
    let mut normal = faces[0].oriented_surface().normal(0., 0.).normalize();
    if normal.dot(anchor(&faces[1])? - anchor(&faces[0])?) < 0. {
        normal = -normal;
    }
    let sections = faces
        .iter()
        .map(|face| {
            if face.oriented_surface().normal(0., 0.).dot(normal) < 0. {
                face.inverse()
            } else {
                face.clone()
            }
        })
        .collect::<Vec<_>>();
    if sections
        .iter()
        .any(|face| !matches!(face.surface(), Surface::Plane(_)))
    {
        return Err(unsupported(
            "solid loft sections require native planar material contours",
        ));
    }
    let parallel = sections.iter().all(|face| {
        face.oriented_surface()
            .normal(0., 0.)
            .normalize()
            .cross(normal)
            .magnitude()
            <= 1e-9
    });
    let source_wires = sections.iter().map(Face::boundaries).collect::<Vec<_>>();
    let contours = source_wires[0].len();
    if contours == 0 || source_wires.iter().any(|wires| wires.len() != contours) {
        return Err(unsupported(
            "section material regions need the same number of outer/hole contours; edge counts may differ",
        ));
    }
    if source_wires
        .iter()
        .flatten()
        .map(|wire| wire.len())
        .sum::<usize>()
        > 32768
    {
        return Err(unsupported(
            "the complete solid loft exceeds the bounded native section budget",
        ));
    }
    if alignments.iter().any(|points| points.len() > contours) {
        return Err(invalid("A loft connector names a missing native contour"));
    }
    let mut prepared = (0..contours)
        .map(|contour| {
            let wires = source_wires
                .iter()
                .map(|wires| wires[contour].clone())
                .collect::<Vec<_>>();
            let anchors = alignments
                .iter()
                .map(|points| points.get(contour).copied().flatten())
                .collect::<Vec<_>>();
            crate::loft_correspondence::prepare(&wires, &anchors)
        })
        .collect::<Result<Vec<_>>>()?;
    if start.is_none() && end.is_none() {
        for contour in &mut prepared {
            crate::loft_correspondence::refine_ruled_charts(contour)?;
        }
    }
    let wires = (0..sections.len())
        .map(|index| {
            prepared
                .iter()
                .map(|p| p.wires[index].clone())
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let sections = sections
        .iter()
        .zip(&wires)
        .map(|(face, wires)| Face::try_new(wires.clone(), face.oriented_surface()).map_err(invalid))
        .collect::<Result<Vec<_>>>()?;
    let origins=(0..sections.len()).map(|index| {
        let mut result=std::collections::HashMap::new();
        for (contour,prepared) in prepared.iter().enumerate() {
            for (edge,piece) in &prepared.pieces[index] {
                let source=origins[index].get(&piece.source).ok_or_else(||invalid("A split loft boundary lost its native source member"))?;
                result.insert(*edge,crate::section_history::Member {
                    sources:source.sources.clone(),
                    purpose:serde_json::json!({"member":source.purpose,"partition":{"contour":contour,"parameter":piece.interval}}),
                });
            }
        }
        Ok(result)
    }).collect::<Result<Vec<_>>>()?;
    let mut shell = Shell::from(vec![sections[0].inverse()]);
    let sources = origins[0]
        .values()
        .flat_map(|member| member.sources.clone())
        .collect::<Vec<_>>();
    history.face(&shell[0], &sources, serde_json::json!({"end":"start"}))?;
    let mut separation_work = 0;
    let mut pair_ranges = Vec::new();
    let mut needs_global_separation = false;
    for index in 0..sections.len() - 1 {
        let lower = &wires[index];
        let upper = &wires[index + 1];
        if lower.len() != upper.len() || lower.iter().zip(upper).any(|(a, b)| a.len() != b.len()) {
            return Err(unsupported(
                "loft contours need matching boundary/member topology",
            ));
        }
        let distance = (anchor(&sections[index + 1])? - anchor(&sections[index])?).dot(normal);
        if parallel && distance <= EPS {
            return Err(unsupported(
                "loft sections must advance strictly without a fold or reversal",
            ));
        }
        let pairs = lower
            .iter()
            .zip(upper)
            .flat_map(|(a, b)| a.iter().zip(b))
            .map(|(a, b)| compatible(&a.oriented_curve(), &b.oriented_curve()))
            .collect::<Result<Vec<_>>>()?;
        let radial =
            contours == 1 && crate::loft_correspondence::prove_radial_pair(&prepared[0], index)?;
        let known_family = parallel
            && (radial
                || affine_family(&pairs, normal)?
                || convex_family(lower, upper, normal)?
                || concentric_family(lower, upper, normal)?);
        let prove_separation = !known_family && start.is_none() && end.is_none();
        if !known_family && !prove_separation {
            return Err(unsupported(
                "endpoint-influenced section correspondence needs a radial/affine/convex/nested-circle proof against folds and intersections",
            ));
        }
        if prove_separation {
            for contour in &prepared {
                if !crate::loft_correspondence::prove_ruled_pair(contour, index)? {
                    return Err(unsupported(
                        "a concave loft strip has no bounded complete native injectivity certificate",
                    ));
                }
            }
        }
        let tangent_start = if index == 0 { start } else { None };
        let tangent_end = if index + 2 == sections.len() {
            end
        } else {
            None
        };
        let h0 = tangent_start
            .map(|vector| vector.dot(normal))
            .unwrap_or(distance);
        let h1 = tangent_end
            .map(|vector| vector.dot(normal))
            .unwrap_or(distance);
        if (start.is_some() || end.is_some())
            && (h0 <= EPS || h1 <= EPS || h0 + h1 >= 3. * distance - EPS)
        {
            return Err(unsupported(
                "endpoint influences must keep every axial cubic control interval strictly positive",
            ));
        }
        let mut pair_skin = Shell::new();
        for (a, b) in lower.iter().zip(upper) {
            let skin = crate::loft_correspondence::skin(a, b, tangent_start, tangent_end)?;
            for ((a, b), wall) in a.iter().zip(b).zip(&skin) {
                let lower = &origins[index][&a.id()];
                let upper = &origins[index + 1][&b.id()];
                let sources = lower
                    .sources
                    .iter()
                    .chain(&upper.sources)
                    .cloned()
                    .collect::<Vec<_>>();
                history.face(
                    wall,
                    &sources,
                    serde_json::json!({"wall":{"lower":lower.purpose,"upper":upper.purpose}}),
                )?;
            }
            pair_skin.extend(skin);
        }
        if prove_separation {
            crate::loft_correspondence::prove_skin_separation(&pair_skin, &mut separation_work)?;
            needs_global_separation = true;
        }
        pair_ranges.push(shell.len()..shell.len() + pair_skin.len());
        shell.extend(pair_skin);
    }
    if needs_global_separation && sections.len() > 2 {
        // The general nonradial proof covers one pair at a time. Establish
        // exact shared separating planes from the actual constructed skins,
        // including neighboring pairs that used one of the family proofs.
        // Tolerance-flat source wires alone cannot exclude cross-pair overlap.
        let mut native_sections = vec![Vec::new(); sections.len()];
        let mut section_controls = 0usize;
        for (index, range) in pair_ranges.iter().enumerate() {
            for face in &shell[range.clone()] {
                let Surface::NurbsSurface(carrier) = face.surface() else {
                    return Err(invalid("Loft lost its exact rational supporting carrier"));
                };
                for row in carrier.control_points() {
                    section_controls = section_controls.saturating_add(2);
                    if row.len() != 2 || section_controls > 262_144 {
                        return Err(unsupported(
                            "multi-pair concave loft exceeds its ruled native separation-control bound",
                        ));
                    }
                    native_sections[index].push(row[0]);
                    native_sections[index + 1].push(row[1]);
                }
            }
        }
        if crate::loft_correspondence::ordered_ruled_sections(&native_sections).is_err() {
            crate::loft_correspondence::prove_skin_separation(
                &Shell::from(shell[1..].to_vec()),
                &mut separation_work,
            )?;
        }
    }
    if !parallel {
        crate::loft_correspondence::prove_end_caps(
            &Shell::from(shell[1..].to_vec()),
            &sections[0],
            sections.last().unwrap(),
            &mut separation_work,
        )?;
    }
    shell.push(sections.last().unwrap().clone());
    let sources = origins
        .last()
        .unwrap()
        .values()
        .flat_map(|member| member.sources.clone())
        .collect::<Vec<_>>();
    history.face(
        shell.last().unwrap(),
        &sources,
        serde_json::json!({"end":"end"}),
    )?;
    let solid = Solid::try_new(vec![shell]).map_err(invalid)?;
    crate::validation::solid(&solid)?;
    Ok(solid)
}
