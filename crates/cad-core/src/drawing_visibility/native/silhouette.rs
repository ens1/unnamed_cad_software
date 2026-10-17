use super::*;

fn normal(p: [J; 3]) -> I {
    p[0].d[0] * p[1].d[1] - p[0].d[1] * p[1].d[0]
}
fn gradient(p: [J; 3]) -> [I; 2] {
    std::array::from_fn(|i| {
        p[0].dd[0][i] * p[1].d[1] + p[0].d[0] * p[1].dd[1][i]
            - p[0].dd[1][i] * p[1].d[0]
            - p[0].d[1] * p[1].dd[0][i]
    })
}
fn projected(scene: &ProjectedScene, face: &Face, cell: Domain) -> Result<[J; 3]> {
    Ok(scene.project_jet(evaluate::surface(&face.geometry, cell, face.matrix)?))
}
fn root_cell(
    scene: &ProjectedScene,
    face: &Face,
    mut cell: Domain,
    dependent: usize,
) -> Result<Domain> {
    for _ in 0..12 {
        let p = projected(scene, face, cell)?;
        let derivative = gradient(p)[dependent];
        if derivative.contains_zero() {
            return Err(unsupported(
                "silhouette graph derivative lost its certified sign",
            ));
        }
        let mid = I::new(cell[dependent][0], cell[dependent][1]).mid();
        let mut point = cell;
        point[dependent] = [mid, mid];
        let q = I::point(mid) - normal(projected(scene, face, point)?) / derivative;
        let next = [cell[dependent][0].max(q.lo), cell[dependent][1].min(q.hi)];
        if next[0] > next[1] {
            return Err(unsupported("silhouette root enclosure became empty"));
        }
        if next == cell[dependent] {
            break;
        }
        cell[dependent] = next;
    }
    Ok(cell)
}
/// A simple fold has exactly one preimage on its tangent ray inside this
/// rectangle. Holding one projected coordinate fixed, the other coordinate
/// has a strictly convex/concave extremum at the silhouette. This certificate
/// exempts only this rectangle, not the rest of a possibly self-occluding face.
fn simple_fold(p: [J; 3]) -> bool {
    for x in 0..2 {
        let y = 1 - x;
        for u in 0..2 {
            let v = 1 - u;
            if p[x].d[v].lo < 0. && p[x].d[v].hi > 0. {
                continue;
            }
            if p[x].d[u].contains_zero() {
                continue;
            }
            let first = -p[x].d[v] / p[x].d[u];
            let second = -(p[x].dd[v][v]
                + I::point(2.) * p[x].dd[u][v] * first
                + p[x].dd[u][u] * first * first)
                / p[x].d[u];
            let curvature = p[y].dd[v][v]
                + I::point(2.) * p[y].dd[u][v] * first
                + p[y].dd[u][u] * first * first
                + p[y].d[u] * second;
            if !curvature.contains_zero() {
                return true;
            }
        }
    }
    false
}
pub(super) fn build(scene: &mut ProjectedScene) -> Result<Vec<NativeTrace>> {
    let mut out: Vec<NativeTrace> = Vec::new();
    for index in 0..scene.faces.len() {
        let face = scene.faces[index].clone();
        if super::silhouette_analytic::redundant(scene, &face)? {
            continue;
        }
        if matches!(
            face.geometry.carrier,
            drawing_geometry::Carrier::Plane { .. }
        ) {
            continue;
        }
        let mut stack = face
            .seeds
            .iter()
            .copied()
            .map(|d| (d, 0))
            .collect::<Vec<_>>();
        while let Some((cell, depth)) = stack.pop() {
            scene
                .budget
                .charge(super::surface_work(&face.geometry).saturating_mul(16))?;
            let p = projected(scene, &face, cell)?;
            let n = normal(p);
            if !n.contains_zero() {
                continue;
            }
            let region = trim::region(&face.geometry, cell, &mut scene.budget)?;
            if region == Region::Outside {
                continue;
            }
            let g = gradient(p);
            let dependent = (0..2)
                .filter(|i| !g[*i].contains_zero())
                .max_by(|a, b| g[*a].absmax().total_cmp(&g[*b].absmax()));
            let mut accepted = false;
            let mut split_axis = None;
            if let Some(dep) = dependent {
                let independent = 1 - dep;
                let mut low = cell;
                let mut high = cell;
                low[dep] = [cell[dep][0]; 2];
                high[dep] = [cell[dep][1]; 2];
                let a = normal(projected(scene, &face, low)?);
                let b = normal(projected(scene, &face, high)?);
                if (a.hi < 0. && b.lo > 0.)
                    || (b.hi < 0. && a.lo > 0.)
                    || ((a.lo == 0. && a.hi == 0.) && !b.contains_zero())
                    || ((b.lo == 0. && b.hi == 0.) && !a.contains_zero())
                {
                    let root = root_cell(scene, &face, cell, dep)?;
                    let world = evaluate::surface(&face.geometry, root, face.matrix)?;
                    let projected_root = scene.project_jet(world);
                    let dg = gradient(projected_root);
                    let slope = -dg[independent] / dg[dep];
                    let derivative: [I; 3] =
                        std::array::from_fn(|i| world[i].d[independent] + world[i].d[dep] * slope);
                    let span = I::new(cell[independent][0], cell[independent][1]);
                    let error =
                        (I::point(span.width()) * I::point(diameter(derivative)) * I::point(0.5))
                            .hi;
                    let local_region = trim::region(&face.geometry, root, &mut scene.budget)?;
                    if local_region == Region::Outside {
                        continue;
                    }
                    let mut center_cell = root;
                    center_cell[independent] = [span.mid(); 2];
                    center_cell = root_cell(scene, &face, center_cell, dep)?;
                    let center =
                        evaluate::surface(&face.geometry, center_cell, face.matrix)?.map(|v| v.x);
                    let fold = simple_fold(p).then_some((index, cell));
                    let status = if face.ghost {
                        Visibility::Visible
                    } else {
                        scene.visibility(Query {
                            bounds: world.map(|p| p.x),
                            center,
                            derivative,
                            radius: span.radius(),
                            self_contact: fold,
                        })?
                    };
                    let tolerance = scene.accuracy.modeling.linear_mm;
                    let output_tolerance = scene.accuracy.display_mm.max(tolerance);
                    let small = diameter(world.map(|p| p.x)) <= tolerance * 0.25;
                    if error <= output_tolerance * 0.25
                        && (local_region == Region::Inside || small)
                        && (status != Visibility::Uncertain || small)
                    {
                        let mut endpoint_cells = [root; 2];
                        let mut points = Vec::new();
                        let mut endpoint_error: f64 = 0.;
                        for i in 0..2 {
                            endpoint_cells[i][independent] = [cell[independent][i]; 2];
                            endpoint_cells[i] = root_cell(scene, &face, endpoint_cells[i], dep)?;
                            let q =
                                evaluate::surface(&face.geometry, endpoint_cells[i], face.matrix)?
                                    .map(|v| v.x);
                            endpoint_error = endpoint_error.max(diameter(q));
                            points.push(q.map(I::mid));
                        }
                        if (I::point(error) + endpoint_error.into()).hi <= output_tolerance * 0.5 {
                            // Adjacent certified cells share a single source
                            // carrier record; large native control nets are
                            // never cloned once per tessellation point.
                            scene.budget.output(2)?;
                            let hidden = status == Visibility::Hidden;
                            if let Some(last) = out.last_mut()
                                && last.face.as_ref() == Some(&face.reference)
                                && last.hidden == hidden
                                && !last.visibility_event
                                && status != Visibility::Uncertain
                                && local_region == Region::Inside
                                && last.ghost == face.ghost
                                && matches!(last.source, TraceSource::Silhouette)
                                && last.points.last() == points.first()
                            {
                                last.points.push(points[1]);
                                last.endpoint_error_mm = last.endpoint_error_mm.max(endpoint_error);
                                last.cells.push(root);
                            } else {
                                scene.budget.store(payload_size(&face.geometry) + 128)?;
                                out.push(NativeTrace {
                                    face: Some(face.reference.clone()),
                                    edge: None,
                                    carrier: Some(face.geometry.clone()),
                                    placement: face.matrix,
                                    view: scene.view.clone(),
                                    source: TraceSource::Silhouette,
                                    cells: vec![root],
                                    points,
                                    max_error_mm: output_tolerance,
                                    endpoint_error_mm: endpoint_error,
                                    visibility_event: status == Visibility::Uncertain
                                        || local_region != Region::Inside,
                                    collapsed_projection: false,
                                    ghost: face.ghost,
                                    hidden,
                                });
                            }
                            accepted = true;
                        }
                    }
                    split_axis = Some(independent);
                }
            }
            if !accepted {
                if depth >= 52 {
                    return Err(unsupported(
                        "a native silhouette is singular, coincident with a parameter seam, or cannot be certified at the modeling tolerance",
                    ));
                }
                let axis = split_axis.unwrap_or_else(|| {
                    let size = std::array::from_fn::<_, 2, _>(|i| {
                        p.iter().map(|p| p.d[i].absmax()).fold(0., f64::max)
                            * (cell[i][1] - cell[i][0])
                    });
                    usize::from(size[1] > size[0])
                });
                // An asymmetric subdivision avoids repeatedly placing ordinary
                // symmetry-axis silhouette roots on internal cell boundaries.
                let middle = cell[axis][0] * 0.53125 + cell[axis][1] * 0.46875;
                if middle <= cell[axis][0] || middle >= cell[axis][1] {
                    return Err(unsupported(
                        "native silhouette subdivision reached machine precision",
                    ));
                }
                let mut left = cell;
                left[axis][1] = middle;
                let mut right = cell;
                right[axis][0] = middle;
                stack.push((right, depth + 1));
                stack.push((left, depth + 1));
            }
            if out.len() + stack.len() > MAX_PIECES {
                return Err(unsupported("too many certified native silhouette cells"));
            }
        }
    }
    Ok(out)
}
pub(super) fn payload_size(face: &FaceGeometry) -> usize {
    let surface = match &face.carrier {
        drawing_geometry::Carrier::Rational { surface } => {
            64 + surface.knots.iter().map(Vec::len).sum::<usize>()
                + surface
                    .control_points
                    .iter()
                    .map(|r| r.len().saturating_mul(4) + 3)
                    .sum::<usize>()
        }
        drawing_geometry::Carrier::Revolution { profile, .. } => 64 + super::curve3_units(profile),
        _ => 64,
    };
    surface
        + face
            .loops
            .iter()
            .chain(face.uv_chart_loops.iter().flatten())
            .map(|l| 3 + l.iter().map(super::curve2_units).sum::<usize>())
            .sum::<usize>()
}
