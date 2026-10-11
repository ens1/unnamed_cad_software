use cad_core::{CadError, Result};
use monstertruck::modeling::*;
use std::collections::{HashMap, HashSet};

const EPSILON: f64 = 1e-7;

pub(crate) struct CutFace {
    pub face: FaceId,
    /// Source identities exist only during construction, never in the native
    /// document or persisted reference role. The caller resolves provenance.
    pub sources: Vec<(FaceId, bool)>,
    pub partition_sources: Vec<FaceId>,
}
pub(crate) struct CutOutput {
    pub bodies: Vec<Solid>,
    pub faces: Vec<CutFace>,
    pub tool: Option<Solid>,
}

#[derive(Clone)]
struct BoundaryPlane {
    face: Face,
    origin: Point3,
    normal: Vector3,
}
impl BoundaryPlane {
    fn signed(&self, point: Point3) -> f64 {
        (point - self.origin).dot(self.normal)
    }
}

// Positive rational controls prove planarity of a carrier; no tessellation is
// used to turn a curved face into a plane.
fn boundary_plane(face: &Face) -> Result<Option<BoundaryPlane>> {
    let Some(vertex) = face.vertex_iter().next() else {
        return Ok(None);
    };
    let normal = face.oriented_surface().normal(0., 0.);
    if !normal.magnitude2().is_finite() || normal.magnitude2() < 1e-18 {
        return Ok(None);
    }
    let plane = BoundaryPlane {
        face: face.clone(),
        origin: vertex.point(),
        normal: normal.normalize(),
    };
    let hull = match crate::extrusion::face_hull(face) {
        Ok(hull) => hull,
        Err(CadError::Unsupported(_)) => return Ok(None),
        Err(error) => return Err(error),
    };
    Ok(hull
        .iter()
        .all(|p| plane.signed(*p).abs() <= EPSILON)
        .then_some(plane))
}
fn straight_edge(edge: &Edge) -> Result<bool> {
    let a = edge.front().point();
    let delta = edge.back().point() - a;
    let length = delta.magnitude2();
    if length <= EPSILON * EPSILON {
        return Ok(false);
    }
    Ok(crate::extrusion::curve_hull(&edge.curve())?
        .iter()
        .all(|p| {
            let t = (*p - a).dot(delta) / length;
            (-1e-9..=1. + 1e-9).contains(&t) && (*p - a - delta * t).magnitude() <= EPSILON
        }))
}
fn supporting_planes(solid: &Solid) -> Result<Vec<BoundaryPlane>> {
    let hull = crate::adapter::solid_control_points(solid)?;
    let mut planes = Vec::new();
    for face in solid.face_iter() {
        let Some(plane) = boundary_plane(face)? else {
            continue;
        };
        if !hull.iter().all(|p| plane.signed(*p) <= EPSILON) {
            continue;
        }
        let boundaries = face.boundaries();
        let outer = &boundaries[0];
        if outer.len() < 3
            || outer
                .iter()
                .map(straight_edge)
                .collect::<Result<Vec<_>>>()?
                .iter()
                .any(|straight| !straight)
        {
            continue;
        }
        let vertices = outer
            .iter()
            .map(|edge| edge.front().point())
            .collect::<Vec<_>>();
        if !outer.iter().all(|edge| {
            let inward = plane
                .normal
                .cross(edge.back().point() - edge.front().point())
                .normalize();
            vertices
                .iter()
                .all(|p| (*p - edge.front().point()).dot(inward) >= -EPSILON)
        }) {
            continue;
        }
        planes.push(plane);
    }
    Ok(planes)
}

/// Reconstruct a contained sketch pocket using the stock's actual entry wires.
/// Existing pockets survive when exact positive control bounds prove disjoint
/// material. Exterior caps are cropped onto complete supporting stock faces.
pub(crate) fn insert_cut(
    solid: &Solid,
    lower: &Face,
    upper: &Face,
    axis: Vector3,
) -> Result<Option<CutOutput>> {
    if solid.boundaries().len() != 1 {
        return Ok(None);
    }
    let planes = match supporting_planes(solid) {
        Ok(planes) => planes,
        Err(CadError::Unsupported(_)) => return Ok(None),
        Err(error) => return Err(error),
    };
    if planes.len() < 4 {
        return Ok(None);
    }
    let mut lower = lower.clone();
    let mut upper = upper.clone();
    for (cap, start) in [(&mut lower, true), (&mut upper, false)] {
        let hull = crate::extrusion::face_hull(cap)?;
        for plane in &planes {
            let dot = plane.normal.dot(axis);
            if (start && dot >= -1e-9)
                || (!start && dot <= 1e-9)
                || !hull.iter().all(|p| plane.signed(*p) >= -EPSILON)
            {
                continue;
            }
            let u = plane
                .normal
                .cross(if plane.normal.x.abs() < 0.8 {
                    Vector3::unit_x()
                } else {
                    Vector3::unit_y()
                })
                .normalize();
            let definition = cad_core::Plane {
                origin: plane.origin.into(),
                u: u.into(),
                v: plane.normal.cross(u).into(),
            };
            let cropped = crate::extrusion::project_face(cap, axis, &definition, 0.)?;
            if !crate::extrusion_surface::covered(&plane.face, &cropped)? {
                return Ok(None);
            }
            *cap = cropped;
            break;
        }
    }
    let tool = match crate::extrusion::between_caps(&lower, &upper, axis) {
        Ok(tool) => tool,
        Err(CadError::Unsupported(_) | CadError::Invalid(_)) => return Ok(None),
        Err(error) => return Err(error),
    };
    let tool_hull = crate::adapter::solid_control_points(&tool)?;
    if planes
        .iter()
        .any(|plane| tool_hull.iter().any(|p| plane.signed(*p) > EPSILON))
    {
        return Ok(None);
    }
    // Supporting planes must account for every stock exterior; all remaining
    // faces belong to prior voids and need a strict disjointness proof.
    for face in solid.face_iter() {
        if planes.iter().any(|plane| plane.face.id() == face.id()) {
            continue;
        }
        let hull = match crate::extrusion::face_hull(face) {
            Ok(hull) => hull,
            Err(CadError::Unsupported(_)) => return Ok(None),
            Err(error) => return Err(error),
        };
        if !crate::extrusion_surface::bounds_disjoint(&hull, &tool_hull, EPSILON) {
            return Ok(None);
        }
    }
    let mut replacements: HashMap<FaceId, Vec<Face>> = HashMap::new();
    let mut removed = HashSet::new();
    let mut tags = Vec::new();
    for cap in tool.face_iter() {
        let hull = crate::extrusion::face_hull(cap)?;
        let Some(plane) = planes
            .iter()
            .find(|plane| hull.iter().all(|p| plane.signed(*p).abs() <= EPSILON))
        else {
            continue;
        };
        if cap
            .oriented_surface()
            .normal(0., 0.)
            .normalize()
            .dot(plane.normal)
            < 1. - 1e-9
        {
            return Ok(None);
        }
        if cap.id() != tool.boundaries()[0][0].id()
            && cap.id() != tool.boundaries()[0].last().unwrap().id()
        {
            return Ok(None);
        }
        if !crate::extrusion_surface::covered(&plane.face, cap)? {
            return Ok(None);
        }
        let cap_boundaries = cap.boundaries();
        let mut boundaries = plane.face.boundaries();
        boundaries.push(cap_boundaries[0].inverse());
        let mut faces = vec![
            Face::try_new(boundaries, plane.face.oriented_surface())
                .map_err(|error| CadError::Invalid(error.to_string()))?,
        ];
        for island in cap_boundaries.iter().skip(1) {
            faces.push(
                Face::try_new(vec![island.inverse()], plane.face.oriented_surface())
                    .map_err(|error| CadError::Invalid(error.to_string()))?,
            );
        }
        for face in &faces {
            let boundary = face
                .edge_iter()
                .map(|edge| edge.id())
                .collect::<HashSet<_>>();
            let mut sources = vec![(plane.face.id(), false)];
            sources.extend(
                tool.face_iter()
                    .filter(|wall| {
                        wall.id() != cap.id()
                            && wall.edge_iter().any(|edge| boundary.contains(&edge.id()))
                    })
                    .map(|wall| (wall.id(), true)),
            );
            tags.push(CutFace {
                face: face.id(),
                sources,
                partition_sources: vec![],
            });
        }
        if replacements.insert(plane.face.id(), faces).is_some() {
            return Ok(None);
        }
        removed.insert(cap.id());
    }
    if removed.is_empty() {
        // Entirely internal pockets form a genuine negative inner shell.
        if planes
            .iter()
            .any(|plane| tool_hull.iter().any(|p| plane.signed(*p) >= -EPSILON))
        {
            return Ok(None);
        }
        let mut shells = solid.boundaries().clone();
        shells.extend(
            tool.boundaries()
                .iter()
                .map(|shell| shell.iter().map(Face::inverse).collect::<Shell>()),
        );
        let result =
            Solid::try_new(shells).map_err(|error| CadError::Invalid(error.to_string()))?;
        crate::validation::solid(&result)?;
        return Ok(Some(CutOutput {
            bodies: vec![result],
            faces: vec![],
            tool: Some(tool),
        }));
    }
    let mut shell = Shell::new();
    for face in solid.face_iter() {
        if let Some(faces) = replacements.remove(&face.id()) {
            shell.extend(faces);
        } else {
            shell.push(face.clone());
        }
    }
    shell.extend(
        tool.face_iter()
            .filter(|face| !removed.contains(&face.id()))
            .map(Face::inverse),
    );
    let mut results = Vec::new();
    for component in shell.connected_components() {
        let result = Solid::try_new(vec![component])
            .map_err(|error| CadError::Invalid(error.to_string()))?;
        crate::validation::solid(&result)?;
        results.push(result);
    }
    Ok(Some(CutOutput {
        bodies: results,
        faces: tags,
        tool: Some(tool),
    }))
}

fn convex_planes(solid: &Solid) -> Result<Option<Vec<BoundaryPlane>>> {
    if solid.boundaries().len() != 1 {
        return Ok(None);
    }
    let planes = match supporting_planes(solid) {
        Ok(planes) => planes,
        Err(CadError::Unsupported(_)) => return Ok(None),
        Err(error) => return Err(error),
    };
    if planes.len() != solid.face_iter().count()
        || planes
            .iter()
            .any(|plane| plane.face.boundaries().len() != 1)
    {
        return Ok(None);
    }
    Ok(Some(planes))
}
#[derive(Clone)]
struct PolygonPiece {
    points: Vec<Point3>,
    carrier: Surface,
    source: FaceId,
    tool: bool,
    partition: FaceId,
}
fn clip_polygon(points: &[Point3], plane: &BoundaryPlane, keep_inside: bool) -> Vec<Point3> {
    if points.iter().all(|p| plane.signed(*p).abs() <= EPSILON) {
        return if keep_inside {
            points.to_vec()
        } else {
            Vec::new()
        };
    }
    let mut out = Vec::new();
    for (a, b) in points
        .iter()
        .zip(points.iter().cycle().skip(1))
        .take(points.len())
    {
        let da = plane.signed(*a);
        let db = plane.signed(*b);
        let inside_a = if keep_inside {
            da <= EPSILON
        } else {
            da >= -EPSILON
        };
        let inside_b = if keep_inside {
            db <= EPSILON
        } else {
            db >= -EPSILON
        };
        if inside_a != inside_b {
            out.push(if da.abs() <= EPSILON {
                *a
            } else if db.abs() <= EPSILON {
                *b
            } else {
                *a + (*b - *a) * (da / (da - db))
            });
        }
        if inside_b {
            out.push(*b);
        }
    }
    out.dedup_by(|a, b| a.distance(*b) <= EPSILON);
    if out
        .first()
        .zip(out.last())
        .is_some_and(|(a, b)| a.distance(*b) <= EPSILON)
    {
        out.pop();
    }
    if out.len() < 3 || polygon_area(&out).magnitude() <= EPSILON * EPSILON {
        Vec::new()
    } else {
        out
    }
}
fn polygon_area(points: &[Point3]) -> Vector3 {
    let origin = points[0];
    points
        .iter()
        .skip(1)
        .zip(points.iter().skip(2))
        .fold(Vector3::zero(), |area, (a, b)| {
            area + (*a - origin).cross(*b - origin)
        })
}
fn vertex_for(vertices: &mut Vec<Vertex>, point: Point3) -> Vertex {
    if let Some(vertex) = vertices
        .iter()
        .find(|vertex| vertex.point().distance(point) <= EPSILON)
    {
        return vertex.clone();
    }
    let vertex = builder::vertex(point);
    vertices.push(vertex.clone());
    vertex
}
fn polygon_shell(pieces: Vec<PolygonPiece>, stock: &Solid, tool: &Solid) -> Result<CutOutput> {
    if pieces.is_empty() {
        return Ok(CutOutput {
            bodies: vec![],
            faces: vec![],
            tool: None,
        });
    }
    let mut vertices = stock
        .vertex_iter()
        .chain(tool.vertex_iter())
        .collect::<Vec<_>>();
    for piece in &pieces {
        for point in &piece.points {
            vertex_for(&mut vertices, *point);
        }
    }
    let mut edges = HashMap::new();
    let mut shell = Shell::new();
    let mut origins = HashMap::new();
    // Splitting every line at all exact incidence points avoids T-junctions
    // created by the half-space partition while retaining shared edge objects.
    for piece in pieces {
        let mut wire = Wire::new();
        for (a, b) in piece
            .points
            .iter()
            .zip(piece.points.iter().cycle().skip(1))
            .take(piece.points.len())
        {
            let delta = *b - *a;
            let length = delta.magnitude2();
            if length <= EPSILON * EPSILON {
                continue;
            }
            let mut incident = vertices
                .iter()
                .filter_map(|vertex| {
                    let t = (vertex.point() - *a).dot(delta) / length;
                    ((-1e-9..=1. + 1e-9).contains(&t)
                        && (vertex.point() - *a - delta * t).magnitude() <= EPSILON)
                        .then_some((t, vertex.clone()))
                })
                .collect::<Vec<_>>();
            incident.sort_by(|a, b| a.0.total_cmp(&b.0));
            incident.dedup_by(|a, b| a.1.point().distance(b.1.point()) <= EPSILON);
            for pair in incident.windows(2) {
                wire.push_back(edge_between(&mut edges, &pair[0].1, &pair[1].1));
            }
        }
        let face = Face::try_new(vec![wire], piece.carrier)
            .map_err(|error| CadError::Invalid(error.to_string()))?;
        origins.insert(face.id(), (piece.source, piece.tool, piece.partition));
        shell.push(face);
    }
    // Eliminate partition seams on each original carrier before exposing the
    // result. This retains one actual face for a connected trimmed region.
    let mut carriers: std::collections::BTreeMap<String, Shell> = std::collections::BTreeMap::new();
    for face in shell {
        carriers
            .entry(serde_json::to_string(&face.oriented_surface())?)
            .or_default()
            .push(face);
    }
    let mut shell = Shell::new();
    let mut tags = Vec::new();
    for patches in carriers.into_values() {
        for component in patches.connected_components() {
            let mut boundaries = component.extract_boundaries();
            boundaries.sort_by(|a, b| {
                let area = |wire: &Wire| {
                    polygon_area(
                        &wire
                            .iter()
                            .map(|edge| edge.front().point())
                            .collect::<Vec<_>>(),
                    )
                    .magnitude2()
                };
                area(b).total_cmp(&area(a))
            });
            let face = Face::try_new(boundaries, component[0].oriented_surface())
                .map_err(|error| CadError::Invalid(error.to_string()))?;
            let mut sources = Vec::new();
            let mut partitions = Vec::new();
            for patch in &component {
                let (source, tool, partition) = origins[&patch.id()];
                if !sources.contains(&(source, tool)) {
                    sources.push((source, tool));
                }
                partitions.push(partition);
            }
            let mut seen = HashSet::new();
            partitions.retain(|partition| seen.insert(*partition));
            tags.push(CutFace {
                face: face.id(),
                sources,
                partition_sources: partitions,
            });
            shell.push(face);
        }
    }
    let mut positive = Vec::new();
    let mut negative = Vec::new();
    for component in shell.connected_components() {
        // For actual straight planar faces this determinant sum is the exact
        // algebraic polyhedron volume, used only to distinguish inner shells.
        let volume: f64 = component
            .iter()
            .map(|face| {
                face.boundaries()
                    .iter()
                    .map(|wire| {
                        let points = wire
                            .iter()
                            .map(|edge| edge.front().point())
                            .collect::<Vec<_>>();
                        points
                            .iter()
                            .skip(1)
                            .zip(points.iter().skip(2))
                            .map(|(a, b)| points[0].to_vec().dot(a.to_vec().cross(b.to_vec())) / 6.)
                            .sum::<f64>()
                    })
                    .sum::<f64>()
            })
            .sum();
        if volume.abs() <= EPSILON.powi(3) {
            return Err(CadError::Invalid(
                "Planar cut produced a zero-volume boundary".into(),
            ));
        }
        if volume > 0. {
            positive.push(component);
        } else {
            negative.push(component);
        }
    }
    if !negative.is_empty() {
        if positive.len() != 1 {
            return Err(CadError::Unsupported("A planar cut with several bodies and enclosed voids requires an exact containment tree".into()));
        }
        positive.extend(negative);
        let solid =
            Solid::try_new(positive).map_err(|error| CadError::Invalid(error.to_string()))?;
        crate::validation::solid(&solid)?;
        return Ok(CutOutput {
            bodies: vec![solid],
            faces: tags,
            tool: None,
        });
    }
    let bodies = positive
        .into_iter()
        .map(|shell| {
            let solid = Solid::try_new(vec![shell])
                .map_err(|error| CadError::Invalid(error.to_string()))?;
            crate::validation::solid(&solid)?;
            Ok(solid)
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(CutOutput {
        bodies,
        faces: tags,
        tool: None,
    })
}

/// Exact convex polyhedron subtraction, including blind corners, side cuts,
/// oblique caps and complete removals. General curves use the Boolean adapter.
pub(crate) fn convex_difference(stock: &Solid, tool: &Solid) -> Result<Option<CutOutput>> {
    let Some(stock_planes) = convex_planes(stock)? else {
        return Ok(None);
    };
    let Some(tool_planes) = convex_planes(tool)? else {
        return Ok(None);
    };
    let stock_points = stock.vertex_iter().map(|v| v.point()).collect::<Vec<_>>();
    let tool_points = tool.vertex_iter().map(|v| v.point()).collect::<Vec<_>>();
    if stock_planes
        .iter()
        .any(|plane| tool_points.iter().all(|p| plane.signed(*p) >= -EPSILON))
        || tool_planes
            .iter()
            .any(|plane| stock_points.iter().all(|p| plane.signed(*p) >= -EPSILON))
    {
        return Ok(Some(CutOutput {
            bodies: vec![stock.clone()],
            faces: vec![],
            tool: None,
        }));
    }
    let mut pieces = Vec::new();
    for face in stock.face_iter() {
        let mut remaining = face.boundaries()[0]
            .iter()
            .map(|edge| edge.front().point())
            .collect::<Vec<_>>();
        for plane in &tool_planes {
            if remaining.is_empty() {
                break;
            }
            let outside = clip_polygon(&remaining, plane, false);
            if !outside.is_empty() {
                pieces.push(PolygonPiece {
                    points: outside,
                    carrier: face.oriented_surface(),
                    source: face.id(),
                    tool: false,
                    partition: plane.face.id(),
                });
            }
            remaining = clip_polygon(&remaining, plane, true);
        }
    }
    for plane in &tool_planes {
        if stock_planes.iter().any(|stock| {
            stock.normal.dot(plane.normal) > 1. - 1e-9
                && stock.signed(plane.origin).abs() <= EPSILON
        }) {
            continue;
        }
        let mut points = plane.face.boundaries()[0]
            .iter()
            .map(|edge| edge.front().point())
            .collect::<Vec<_>>();
        for stock in &stock_planes {
            points = clip_polygon(&points, stock, true);
            if points.is_empty() {
                break;
            }
        }
        if !points.is_empty() {
            points.reverse();
            pieces.push(PolygonPiece {
                points,
                carrier: plane.face.oriented_surface().inverse(),
                source: plane.face.id(),
                tool: true,
                partition: plane.face.id(),
            });
        }
    }
    polygon_shell(pieces, stock, tool).map(Some)
}

fn curve_hull(curve: Curve) -> Option<Vec<Point3>> {
    match curve {
        Curve::Line(line) => Some(vec![line.subs(0.), line.subs(1.)]),
        Curve::BsplineCurve(curve) => Some(curve.control_points().clone()),
        Curve::NurbsCurve(curve) => curve
            .control_points()
            .iter()
            .map(|p| {
                (p.w > 0. && p.w.is_finite()).then(|| Point3::new(p.x / p.w, p.y / p.w, p.z / p.w))
            })
            .collect(),
        _ => None,
    }
}

fn edge_between(cache: &mut HashMap<(VertexId, VertexId), Edge>, a: &Vertex, b: &Vertex) -> Edge {
    if let Some(edge) = cache.get(&(a.id(), b.id())) {
        return edge.clone();
    }
    let edge = builder::line(a, b);
    cache.insert((a.id(), b.id()), edge.clone());
    cache.insert((b.id(), a.id()), edge.inverse());
    edge
}

// Keep the negative half-space. Intersections are exact line/plane intersections;
// curved faces are retained only when their control hull is wholly inside.
fn clip(solid: &Solid, origin: Point3, normal: Vector3) -> Result<Solid> {
    let signed = |p: Point3| (p - origin).dot(normal);
    let mut edge_cache = HashMap::new();
    for edge in solid.edge_iter() {
        edge_cache.insert((edge.front().id(), edge.back().id()), edge.clone());
        edge_cache.insert((edge.back().id(), edge.front().id()), edge.inverse());
    }
    let mut intersections = HashMap::new();
    let mut faces = Vec::new();
    let mut cap_edges = Vec::new();
    for face in solid.face_iter() {
        let boundaries = face.boundaries();
        let all_inside = boundaries.iter().flatten().all(|edge| {
            curve_hull(edge.curve())
                .is_some_and(|points| points.iter().all(|p| signed(*p) <= EPSILON))
        });
        if all_inside {
            let surface_inside = match face.surface() {
                Surface::Plane(_) => true,
                Surface::BsplineSurface(surface) => surface
                    .control_points()
                    .iter()
                    .flatten()
                    .all(|p| signed(*p) <= EPSILON),
                Surface::NurbsSurface(surface) => {
                    surface.control_points().iter().flatten().all(|p| {
                        p.w > 0. && signed(Point3::new(p.x / p.w, p.y / p.w, p.z / p.w)) <= EPSILON
                    })
                }
                _ => false,
            };
            if surface_inside {
                faces.push(face.clone());
                continue;
            }
        }
        if !matches!(face.surface(), Surface::Plane(_))
            || boundaries[0]
                .iter()
                .any(|e| !matches!(e.curve(), Curve::Line(_)))
            || boundaries.iter().skip(1).flatten().any(|e| {
                !curve_hull(e.curve())
                    .is_some_and(|points| points.iter().all(|p| signed(*p) <= EPSILON))
            })
        {
            return Err(CadError::Unsupported(
                "This planar cut intersects a curved face or an existing pocket boundary".into(),
            ));
        }
        let mut polygon = Vec::new();
        for edge in &boundaries[0] {
            let a = edge.front();
            let b = edge.back();
            let da = signed(a.point());
            let db = signed(b.point());
            let inside_a = da <= EPSILON;
            let inside_b = db <= EPSILON;
            if inside_a != inside_b {
                let vertex = if da.abs() <= EPSILON {
                    a.clone()
                } else if db.abs() <= EPSILON {
                    b.clone()
                } else {
                    intersections
                        .entry(edge.id())
                        .or_insert_with(|| {
                            builder::vertex(a.point() + (b.point() - a.point()) * (da / (da - db)))
                        })
                        .clone()
                };
                polygon.push(vertex);
            }
            if inside_b {
                polygon.push(b.clone());
            }
        }
        polygon.dedup_by(|a, b| a.id() == b.id());
        if polygon
            .first()
            .zip(polygon.last())
            .is_some_and(|(a, b)| a.id() == b.id())
        {
            polygon.pop();
        }
        if polygon.len() < 3 {
            continue;
        }
        let mut wire = Wire::new();
        for index in 0..polygon.len() {
            let a = &polygon[index];
            let b = &polygon[(index + 1) % polygon.len()];
            let edge = edge_between(&mut edge_cache, a, b);
            if signed(a.point()).abs() <= EPSILON && signed(b.point()).abs() <= EPSILON {
                cap_edges.push(edge.inverse());
            }
            wire.push_back(edge);
        }
        let mut wires = vec![wire];
        wires.extend(boundaries.into_iter().skip(1));
        faces.push(
            Face::try_new(wires, face.oriented_surface())
                .map_err(|e| CadError::Invalid(e.to_string()))?,
        );
    }
    if cap_edges.is_empty() {
        if faces.len() == solid.face_iter().count() {
            return Ok(solid.clone());
        }
        return Err(CadError::Unsupported(
            "The planar cut removes the whole body".into(),
        ));
    }
    let mut cap = Wire::from(vec![cap_edges.remove(0)]);
    while !cap_edges.is_empty() {
        let end = cap.back().unwrap().back().id();
        let Some(index) = cap_edges.iter().position(|edge| edge.front().id() == end) else {
            return Err(CadError::Invalid(
                "Planar cut boundary could not be joined".into(),
            ));
        };
        cap.push_back(cap_edges.remove(index));
    }
    if cap.front().unwrap().front().id() != cap.back().unwrap().back().id() {
        return Err(CadError::Invalid("Planar cut boundary is open".into()));
    }
    faces.push(builder::try_attach_plane(vec![cap]).map_err(|e| CadError::Invalid(e.to_string()))?);
    Solid::try_new(vec![faces.into()]).map_err(|e| CadError::Invalid(e.to_string()))
}

pub(crate) struct ChamferSpec {
    pub distance: f64,
    pub second_distance: Option<f64>,
    pub angle: Option<f64>,
    pub flip: bool,
    pub first_face: Option<FaceId>,
}

pub(crate) fn chamfer(
    solid: &Solid,
    selected: &[EdgeId],
    spec: &ChamferSpec,
) -> Result<Option<Solid>> {
    let mut seen = HashSet::new();
    let mut planes = Vec::new();
    for edge in solid
        .edge_iter()
        .filter(|e| selected.contains(&e.id()) && seen.insert(e.id()))
    {
        if !matches!(edge.curve(), Curve::Line(_)) {
            return Ok(None);
        }
        let mut faces = solid
            .face_iter()
            .filter(|f| f.edge_iter().any(|e| e.id() == edge.id()))
            .collect::<Vec<_>>();
        if faces.len() != 2
            || faces
                .iter()
                .any(|f| !matches!(f.surface(), Surface::Plane(_)))
        {
            return Ok(None);
        }
        if let Some(first) = spec.first_face {
            let Some(index) = faces.iter().position(|face| face.id() == first) else {
                return Err(CadError::Invalid(
                    "Distance 1 face must adjoin every selected chamfer edge".into(),
                ));
            };
            faces.swap(0, index);
        } else {
            faces.sort_by(|a, b| {
                let a = a.oriented_surface().normal(0., 0.).normalize();
                let b = b.oriented_surface().normal(0., 0.).normalize();
                a.x.total_cmp(&b.x)
                    .then(a.y.total_cmp(&b.y))
                    .then(a.z.total_cmp(&b.z))
            });
        }
        if spec.flip {
            faces.swap(0, 1);
        }
        let normals = faces
            .iter()
            .map(|f| f.oriented_surface().normal(0., 0.).normalize())
            .collect::<Vec<_>>();
        let cosine = normals[0].dot(normals[1]);
        let sine = (1. - cosine * cosine).max(0.).sqrt();
        if sine < EPSILON {
            return Ok(None);
        }
        let second = if let Some(angle) = spec.angle {
            // In the perpendicular section the material corner is acos(-n0.n1).
            let interior = (-cosine).clamp(-1., 1.).acos();
            if angle <= 0. || angle >= std::f64::consts::PI - interior {
                return Err(CadError::Invalid(format!(
                    "Chamfer angle must be positive and less than {:.6} degrees for this edge",
                    (std::f64::consts::PI - interior).to_degrees()
                )));
            }
            spec.distance * angle.sin() / (interior + angle).sin()
        } else {
            spec.second_distance.unwrap_or(spec.distance)
        };
        if !second.is_finite() || second <= 0. {
            return Err(CadError::Invalid(
                "Chamfer setback must be finite and positive".into(),
            ));
        }
        let sum = normals[0] * spec.distance + normals[1] * second;
        let anchor = edge.front().point();
        if solid.vertex_iter().any(|v| {
            normals
                .iter()
                .any(|n| (v.point() - anchor).dot(*n) > EPSILON)
        }) {
            return Ok(None);
        }
        let origin = anchor - sum * (spec.distance * second * sine / sum.magnitude2());
        planes.push((origin, sum.normalize()));
    }
    if planes.len() != selected.len() {
        return Ok(None);
    }
    let mut result = solid.clone();
    for (origin, normal) in planes {
        result = clip(&result, origin, normal)?;
    }
    Ok(Some(result))
}
