use crate::shell_history::ShellHistory;
use cad_core::{CadError, Result, TopologyRef};
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
pub(crate) struct BoundaryPlane {
    pub face: Face,
    pub origin: Point3,
    pub normal: Vector3,
}
impl BoundaryPlane {
    pub fn signed(&self, point: Point3) -> f64 {
        (point - self.origin).dot(self.normal)
    }
}

// Positive rational controls prove planarity of a carrier; no tessellation is
// used to turn a curved face into a plane.
pub(crate) fn boundary_plane(face: &Face) -> Result<Option<BoundaryPlane>> {
    let Some(vertex) = face.vertex_iter().next() else {
        return Ok(None);
    };
    let normal = match face.oriented_surface() {
        Surface::Plane(plane) => plane.normal(),
        Surface::BsplineSurface(surface) => {
            let ((u0, u1), (v0, v1)) = surface.range_tuple();
            surface.normal((u0 + u1) * 0.5, (v0 + v1) * 0.5)
        }
        Surface::NurbsSurface(surface) => {
            let ((u0, u1), (v0, v1)) = surface.range_tuple();
            surface.normal((u0 + u1) * 0.5, (v0 + v1) * 0.5)
        }
        _ => return Ok(None),
    };
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
pub(crate) fn straight_edge(edge: &Edge) -> Result<bool> {
    let a = edge.front().point();
    let delta = edge.back().point() - a;
    let length = delta.magnitude2();
    if length <= EPSILON * EPSILON {
        return Ok(false);
    }
    let hull = match crate::extrusion::curve_hull(&edge.curve()) {
        Ok(hull) => hull,
        Err(CadError::Unsupported(_)) => return Ok(false),
        Err(error) => return Err(error),
    };
    if !hull.iter().all(|p| {
        let t = (*p - a).dot(delta) / length;
        (-1e-9..=1. + 1e-9).contains(&t) && (*p - a - delta * t).magnitude() <= EPSILON
    }) {
        return Ok(false);
    }
    match crate::bounded_carriers::line(&edge.curve()) {
        Ok(()) => Ok(true),
        Err(CadError::Unsupported(_)) => Ok(false),
        Err(error) => Err(error),
    }
}
pub(crate) fn supporting_planes(solid: &Solid) -> Result<Vec<BoundaryPlane>> {
    let enclosures = crate::native_bounds::solid(solid)?;
    let mut planes = Vec::new();
    for face in solid.face_iter() {
        let Some(plane) = boundary_plane(face)? else {
            continue;
        };
        if enclosures
            .iter()
            .map(|enclosure| enclosure.projection(plane.origin, plane.normal))
            .collect::<Result<Vec<_>>>()?
            .iter()
            .any(|(_, high)| *high > EPSILON)
        {
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

// A bevel half-space is computed on the original stock. Clipping the complete
// shared shell, rather than collecting every coplanar polygon segment, makes
// several selected setbacks meet at the same exact corner topology.
#[derive(Clone)]
struct BevelPlane {
    origin: Point3,
    normal: Vector3,
    sources: Vec<TopologyRef>,
    /// A bounded non-convex-stock constructor clips only these source walls
    /// and previously constructed bevel caps. Retained descendants preserve
    /// these authored references while their transient face IDs change.
    local: Option<LocalSetback>,
}
#[derive(Clone)]
struct LocalSetback {
    walls: Vec<TopologyRef>,
    removals: Vec<RemovalBounds>,
}
#[derive(Clone)]
struct RemovalBounds {
    origin: Point3,
    projections: Vec<(Vector3, (f64, f64))>,
}
fn charge_chamfer_proof(work: &mut usize, cost: usize) -> Result<()> {
    *work = work.saturating_add(cost);
    if *work > 4_000_000 {
        return Err(CadError::Unsupported(
            "Chamfer corner separation exceeds the operation's proof budget".into(),
        ));
    }
    Ok(())
}
impl RemovalBounds {
    fn separates(&self, bounds: &crate::native_bounds::Enclosure) -> Result<bool> {
        self.projections
            .iter()
            .try_fold(false, |separated, (direction, removal)| {
                let boundary = bounds.projection(self.origin, *direction)?;
                Ok(separated
                    || boundary.0 > removal.1 + EPSILON
                    || boundary.1 < removal.0 - EPSILON)
            })
    }
}

/// A boss or pocket can make an otherwise ordinary plate non-convex without
/// changing the sharp outer corner. Certify that bounded neighborhood instead
/// of requiring every source face to lie behind both infinite wall planes.
/// The actual setback may change only the two support walls and their real
/// terminal faces. Complete native bounds separate every other face from the
/// bounded removal prism, even when a distant boss crosses the infinite bevel
/// plane. The profile vertices are analytic plane intersections, not samples.
fn local_convex_setback(
    solid: &Solid,
    edge: &Edge,
    walls: &[BoundaryPlane; 2],
    setback: (Point3, Vector3),
    distances: [f64; 2],
    history: &ShellHistory,
    work: &mut usize,
) -> Result<Option<LocalSetback>> {
    let (origin, normal) = setback;
    let occurrence = walls[0]
        .face
        .edge_iter()
        .find(|candidate| candidate.id() == edge.id())
        .ok_or_else(|| CadError::Invalid("Chamfer source edge is absent from its wall".into()))?;
    let direction = (occurrence.back().point() - occurrence.front().point()).normalize();
    // The outward wall normals and native wire direction identify a convex
    // material wedge; nearby control points alone do not establish its side.
    if walls[0].normal.cross(walls[1].normal).dot(direction) <= EPSILON {
        return Ok(None);
    }
    let mut local = walls
        .iter()
        .map(|wall| wall.face.id())
        .collect::<HashSet<_>>();
    let axis = (edge.back().point() - edge.front().point()).normalize();
    let cosine = walls[0].normal.dot(walls[1].normal).clamp(-1., 1.);
    let sine = (1. - cosine * cosine).max(0.).sqrt();
    let offsets = [
        -(walls[1].normal - walls[0].normal * cosine) * (distances[0] / sine),
        -(walls[0].normal - walls[1].normal * cosine) * (distances[1] / sine),
    ];
    let mut terminals = Vec::new();
    let mut vertices = Vec::new();
    for (end, vertex) in [edge.front(), edge.back()].into_iter().enumerate() {
        let mut incident = Vec::new();
        for face in solid.face_iter() {
            for candidate in face.vertex_iter() {
                charge_chamfer_proof(work, 1)?;
                if candidate.id() == vertex.id() {
                    incident.push(face);
                    break;
                }
            }
        }
        if incident.len() != 3 {
            return Ok(None);
        }
        let Some(terminal) = incident
            .iter()
            .find(|face| walls.iter().all(|wall| wall.face.id() != face.id()))
        else {
            return Ok(None);
        };
        let Some(plane) = boundary_plane(terminal)? else {
            return Ok(None);
        };
        let denominator = axis.dot(plane.normal);
        if (end == 0 && denominator >= -EPSILON) || (end == 1 && denominator <= EPSILON) {
            return Ok(None);
        }
        vertices.push(vertex.point());
        for (side, wall) in walls.iter().enumerate() {
            let mut neighbours = Vec::new();
            for candidate in terminal.edge_iter() {
                charge_chamfer_proof(work, 1)?;
                if candidate.front().id() != vertex.id() && candidate.back().id() != vertex.id() {
                    continue;
                }
                for other in wall.face.edge_iter() {
                    charge_chamfer_proof(work, 1)?;
                    if other.id() == candidate.id() {
                        neighbours.push(candidate);
                        break;
                    }
                }
            }
            let [neighbour] = neighbours.as_slice() else {
                return Ok(None);
            };
            if !straight_edge(neighbour)? {
                return Ok(None);
            }
            let offset = offsets[side];
            let point = vertex.point() + offset - axis * (offset.dot(plane.normal) / denominator);
            let other = if neighbour.front().id() == vertex.id() {
                neighbour.back()
            } else {
                neighbour.front()
            };
            let span = other.point() - vertex.point();
            let length = span.magnitude();
            let fraction = (point - vertex.point()).dot(span) / (length * length);
            if ![point.x, point.y, point.z, fraction]
                .iter()
                .all(|value| value.is_finite())
                || fraction <= EPSILON / length
                || fraction >= 1. - EPSILON / length
                || (point - vertex.point() - span * fraction).magnitude() > EPSILON
                || (point - origin).dot(normal).abs() > EPSILON
            {
                return Err(CadError::Invalid(
                    "Chamfer setback consumes or leaves a terminal edge; reduce the distances"
                        .into(),
                ));
            }
            vertices.push(point);
        }
        local.insert(terminal.id());
        terminals.push(plane);
    }
    let supports = walls.iter().chain(&terminals).collect::<Vec<_>>();
    // The four real support planes bound the source endpoint neighborhood.
    // Requiring its complete faces behind those planes excludes a reentrant
    // support that would make a local half-space clip affect another region.
    for face in solid.face_iter() {
        if !local.contains(&face.id()) {
            continue;
        }
        let bounds = crate::native_bounds::face(face)?;
        charge_chamfer_proof(work, bounds.len().saturating_mul(supports.len()))?;
        for support in &supports {
            if bounds.projection(support.origin, support.normal)?.1 > EPSILON {
                return Ok(None);
            }
        }
    }
    let prism = crate::native_bounds::Enclosure::from_hull(vertices)?;
    for support in &supports {
        if prism.projection(support.origin, support.normal)?.1 > EPSILON {
            return Ok(None);
        }
    }
    let projections = [
        Vector3::unit_x(),
        Vector3::unit_y(),
        Vector3::unit_z(),
        normal,
    ]
    .into_iter()
    .chain(supports.iter().map(|plane| plane.normal))
    .map(|direction| {
        Ok((
            direction,
            prism.projection(edge.front().point(), direction)?,
        ))
    })
    .collect::<Result<Vec<_>>>()?;
    let removal = RemovalBounds {
        origin: edge.front().point(),
        projections,
    };
    for face in solid.face_iter() {
        if local.contains(&face.id()) {
            continue;
        }
        let bounds = crate::native_bounds::face(face)?;
        charge_chamfer_proof(work, bounds.len().saturating_mul(removal.projections.len()))?;
        if !removal.separates(&bounds)? {
            return Err(CadError::Unsupported(
                "Chamfer bounded setback reaches an unrelated boundary; reduce the distances or select another edge".into(),
            ));
        }
    }
    Ok(Some(LocalSetback {
        walls: local
            .into_iter()
            .map(|id| history.face(id))
            .collect::<Result<_>>()?,
        removals: vec![removal],
    }))
}

fn simple_polygon(vertices: &[Vertex], normal: Vector3) -> bool {
    let mut seen = HashSet::new();
    if vertices.iter().any(|vertex| !seen.insert(vertex.id())) {
        return false;
    }
    for i in 0..vertices.len() {
        let a = vertices[i].point();
        let b = vertices[(i + 1) % vertices.len()].point();
        let u = b - a;
        let length_u = u.magnitude();
        if length_u <= EPSILON {
            return false;
        }
        for j in i + 1..vertices.len() {
            if j == i + 1 || (i == 0 && j + 1 == vertices.len()) {
                continue;
            }
            let c = vertices[j].point();
            let d = vertices[(j + 1) % vertices.len()].point();
            let v = d - c;
            let length_v = v.magnitude();
            if length_v <= EPSILON {
                return false;
            }
            let denominator = normal.dot(u.cross(v));
            if denominator.abs() > 1e-12 * length_u * length_v {
                let t = normal.dot((c - a).cross(v)) / denominator;
                let s = normal.dot((c - a).cross(u)) / denominator;
                if (-EPSILON / length_u..=1. + EPSILON / length_u).contains(&t)
                    && (-EPSILON / length_v..=1. + EPSILON / length_v).contains(&s)
                {
                    return false;
                }
            } else if (c - a).cross(u).magnitude() <= EPSILON * length_u {
                let c = (c - a).dot(u) / length_u;
                let d = (d - a).dot(u) / length_u;
                if c.min(d).max(0.) <= c.max(d).min(length_u) + EPSILON {
                    return false;
                }
            }
        }
    }
    true
}

fn wholly_inside(face: &Face, origin: Point3, normal: Vector3) -> Result<bool> {
    match crate::native_bounds::face(face) {
        Ok(enclosure) => Ok(enclosure.projection(origin, normal)?.1 <= EPSILON),
        Err(CadError::Unsupported(_)) => Ok(false),
        Err(error) => Err(error),
    }
}

// Open boundaries are the unmatched, oriented edge occurrences of the clipped
// shell. Existing edges exactly on the cut plane occur twice and cancel. This
// also includes a cap edge whose neighboring wall was retained unchanged.
fn bevel_boundaries(shell: &Shell, plane: &BevelPlane) -> Result<Vec<Wire>> {
    let mut occurrences: HashMap<EdgeId, Vec<Edge>> = HashMap::new();
    for edge in shell.edge_iter() {
        occurrences.entry(edge.id()).or_default().push(edge.clone());
    }
    let mut edges = Vec::new();
    for paired in occurrences.into_values() {
        match paired.as_slice() {
            [edge] => {
                if !curve_hull(edge.curve()).is_some_and(|points| {
                    points
                        .iter()
                        .all(|point| (*point - plane.origin).dot(plane.normal).abs() <= EPSILON)
                }) {
                    return Err(CadError::Invalid(
                        "Chamfer produced an open boundary away from its setback plane".into(),
                    ));
                }
                edges.push(edge.inverse());
            }
            [a, b] if a.front().id() == b.back().id() && a.back().id() == b.front().id() => {}
            _ => {
                return Err(CadError::Invalid(
                    "Chamfer setbacks overlap or produce non-manifold corner topology".into(),
                ));
            }
        }
    }
    let mut starts = HashMap::new();
    let mut ends = HashSet::new();
    for edge in edges {
        if starts.insert(edge.front().id(), edge.clone()).is_some()
            || !ends.insert(edge.back().id())
        {
            return Err(CadError::Invalid(
                "Chamfer setbacks collapse into a branching corner; reduce the distances".into(),
            ));
        }
    }
    let mut boundaries = Vec::new();
    while let Some(first) = shell
        .vertex_iter()
        .find_map(|vertex| starts.get(&vertex.id()).cloned())
    {
        let start = first.front().id();
        let mut wire = Wire::new();
        let mut cursor = first;
        loop {
            starts.remove(&cursor.front().id());
            let next = cursor.back().id();
            wire.push_back(cursor);
            if next == start {
                break;
            }
            cursor = starts.remove(&next).ok_or_else(|| {
                CadError::Invalid("Chamfer setback boundary is open; reduce the distances".into())
            })?;
        }
        if wire.len() < 3 {
            return Err(CadError::Invalid(
                "Chamfer setbacks collapse a bevel face; reduce the distances".into(),
            ));
        }
        boundaries.push(wire);
    }
    if !starts.is_empty() {
        return Err(CadError::Invalid(
            "Chamfer boundary contains an unconnected corner".into(),
        ));
    }
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
    for (index, wire) in boundaries.iter().enumerate() {
        let area = polygon_area(
            &wire
                .iter()
                .map(|edge| edge.front().point())
                .collect::<Vec<_>>(),
        )
        .dot(plane.normal);
        if area.abs() <= EPSILON * EPSILON || (index == 0 && area < 0.) {
            return Err(CadError::Invalid(
                "Chamfer setbacks collapse or reverse a bevel face; reduce the distances".into(),
            ));
        }
        if index > 0 && area > 0. {
            return Err(CadError::Unsupported(
                "Chamfer would separate its bevel into several disconnected regions".into(),
            ));
        }
    }
    Ok(boundaries)
}

// Keep the negative half-space on globally supporting stock or on the certified
// local support neighborhood. Straight positive rational carriers are trimmed
// to actual lines; unrelated curved carriers survive only with a full inside
// or bounded-removal separation proof.
fn clip(
    solid: &Solid,
    plane: &BevelPlane,
    history: &mut ShellHistory,
    bevels: &mut HashSet<FaceId>,
    work: &mut usize,
) -> Result<Solid> {
    let signed = |p: Point3| (p - plane.origin).dot(plane.normal);
    let Some(exterior) = solid.boundaries().first() else {
        return Err(CadError::Invalid(
            "Chamfer requires a closed source body".into(),
        ));
    };
    // Enclosed void shells need no modification when their complete carriers are
    // disjoint from the bevel. Intersecting one needs a general curved Boolean.
    for cavity in solid.boundaries().iter().skip(1) {
        if plane.local.is_none()
            && cavity
                .iter()
                .map(|face| wholly_inside(face, plane.origin, plane.normal))
                .collect::<Result<Vec<_>>>()?
                .iter()
                .any(|inside| !inside)
        {
            return Err(CadError::Unsupported(
                "Chamfer intersects an enclosed void boundary; reduce the distances or select another edge".into(),
            ));
        }
    }
    let mut edge_cache = HashMap::new();
    for edge in exterior.edge_iter() {
        edge_cache.insert((edge.front().id(), edge.back().id()), edge.clone());
        edge_cache.insert((edge.back().id(), edge.front().id()), edge.inverse());
    }
    let mut vertices = exterior.vertex_iter().collect::<Vec<_>>();
    let mut intersections = HashMap::new();
    let mut faces = Shell::new();
    let mut changed = false;
    for face in exterior {
        let boundaries = face.boundaries();
        if wholly_inside(face, plane.origin, plane.normal)? {
            faces.push(face.clone());
            continue;
        }
        if let Some(local) = &plane.local
            && !local.walls.contains(&history.face(face.id())?)
        {
            // Original unrelated faces were separated before any clipping.
            // A previous bevel can share this corner or belong to a distant
            // selected edge. Clip it only if its complete bound is not
            // separated from every bounded removal region for this plane.
            let separated = if bevels.contains(&face.id()) {
                let bounds = crate::native_bounds::face(face)?;
                local.removals.iter().try_fold(true, |separated, removal| {
                    charge_chamfer_proof(
                        work,
                        bounds.len().saturating_mul(removal.projections.len()),
                    )?;
                    Ok::<_, CadError>(separated && removal.separates(&bounds)?)
                })?
            } else {
                true
            };
            if separated {
                faces.push(face.clone());
                continue;
            }
        }
        changed = true;
        let carrier = boundary_plane(face)?;
        if carrier.is_none()
            || boundaries[0]
                .iter()
                .map(straight_edge)
                .collect::<Result<Vec<_>>>()?
                .iter()
                .any(|straight| !straight)
            || boundaries.iter().skip(1).flatten().any(|edge| {
                !curve_hull(edge.curve())
                    .is_some_and(|points| points.iter().all(|point| signed(*point) < -EPSILON))
            })
        {
            return Err(CadError::Unsupported(
                "Chamfer intersects a curved wall or pocket boundary; reduce the distances or select another edge".into(),
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
                            vertex_for(
                                &mut vertices,
                                a.point() + (b.point() - a.point()) * (da / (da - db)),
                            )
                        })
                        .clone()
                };
                let (front, back) = if inside_a { (a, &vertex) } else { (&vertex, b) };
                if front.point().distance(back.point()) > EPSILON && history.edge(edge.id()).is_ok()
                {
                    let segment = edge_between(&mut edge_cache, front, back);
                    history.retain_edge(segment.id(), edge.id())?;
                }
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
        if polygon.len() < 3
            || polygon_area(&polygon.iter().map(Vertex::point).collect::<Vec<_>>()).magnitude()
                <= EPSILON * EPSILON
        {
            continue;
        }
        let normal = carrier
            .map(|carrier| carrier.normal)
            .ok_or_else(|| CadError::Unsupported("Chamfer wall is not provably planar".into()))?;
        if !simple_polygon(&polygon, normal) {
            return Err(CadError::Unsupported(
                "Chamfer splits or collapses a concave wall boundary; reduce the distances".into(),
            ));
        }
        let mut wire = Wire::new();
        for index in 0..polygon.len() {
            let a = &polygon[index];
            let b = &polygon[(index + 1) % polygon.len()];
            wire.push_back(edge_between(&mut edge_cache, a, b));
        }
        let mut wires = vec![wire];
        wires.extend(boundaries.into_iter().skip(1));
        let trimmed = Face::try_new(wires, face.oriented_surface())
            .map_err(|error| CadError::Invalid(format!("Chamfer wall trimming: {error}")))?;
        history.retain_face(trimmed.id(), face.id())?;
        if bevels.contains(&face.id()) {
            bevels.insert(trimmed.id());
        }
        faces.push(trimmed);
    }
    if !changed {
        return Ok(solid.clone());
    }
    if faces.is_empty() {
        return Err(CadError::Invalid(
            "Chamfer setbacks consume the whole body; reduce the distances".into(),
        ));
    }
    let boundaries = bevel_boundaries(&faces, plane)?;
    if boundaries.is_empty() {
        return Err(CadError::Invalid(
            "Chamfer setbacks collapse the remaining body; reduce the distances".into(),
        ));
    }
    let cap = builder::try_attach_plane(boundaries)
        .map_err(|error| CadError::Invalid(format!("Chamfer bevel construction: {error}")))?;
    history.construct_face(
        cap.id(),
        &plane.sources,
        serde_json::json!({"operation":"chamfer","constructor":"edge_setback"}),
    )?;
    bevels.insert(cap.id());
    faces.push(cap);
    if faces.connected_components().len() != 1 {
        return Err(CadError::Unsupported(
            "Chamfer would separate this source body into several components".into(),
        ));
    }
    let mut shells = vec![faces];
    shells.extend(solid.boundaries().iter().skip(1).cloned());
    let result = Solid::try_new(shells)
        .map_err(|error| CadError::Invalid(format!("Chamfer corner construction: {error}")))?;
    crate::validation::solid(&result)?;
    Ok(result)
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
    history: &mut ShellHistory,
) -> Result<Option<Solid>> {
    if selected.is_empty() || !spec.distance.is_finite() || spec.distance <= EPSILON {
        return Err(CadError::Invalid(
            "Chamfer requires selected edges and a finite positive distance above the modeling tolerance".into(),
        ));
    }
    if selected.len() > cad_core::edge_features::MAX_SELECTED_EDGES
        || solid.face_iter().take(4097).count() > 4096
    {
        return Err(CadError::Unsupported(
            "Chamfer selection exceeds the planar corner complexity limit".into(),
        ));
    }
    let mut proof_work = 0usize;
    let selected = selected.iter().copied().collect::<HashSet<_>>();
    let mut seen = HashSet::new();
    let mut planes: Vec<BevelPlane> = Vec::new();
    let stock_bounds = match crate::native_bounds::solid(solid) {
        Ok(bounds) => bounds,
        Err(CadError::Unsupported(_)) => return Ok(None),
        Err(error) => return Err(error),
    };
    for edge in solid
        .edge_iter()
        .filter(|e| selected.contains(&e.id()) && seen.insert(e.id()))
    {
        if !straight_edge(&edge)? {
            return Ok(None);
        }
        let mut faces = solid
            .face_iter()
            .filter(|f| f.edge_iter().any(|e| e.id() == edge.id()))
            .collect::<Vec<_>>();
        if faces.len() != 2
            || faces
                .iter()
                .map(|face| boundary_plane(face))
                .collect::<Result<Vec<_>>>()?
                .iter()
                .any(Option::is_none)
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
            let normal = |face: &Face| -> Result<Vector3> {
                boundary_plane(face)?
                    .map(|plane| plane.normal)
                    .ok_or_else(|| {
                        CadError::Unsupported("Chamfer edge must adjoin proven planar faces".into())
                    })
            };
            let a = normal(faces[0])?;
            let b = normal(faces[1])?;
            if a.x
                .total_cmp(&b.x)
                .then(a.y.total_cmp(&b.y))
                .then(a.z.total_cmp(&b.z))
                .is_gt()
            {
                faces.swap(0, 1);
            }
        }
        if spec.flip {
            faces.swap(0, 1);
        }
        let walls = faces
            .iter()
            .map(|face| -> Result<BoundaryPlane> {
                boundary_plane(face)?.ok_or_else(|| {
                    CadError::Unsupported("Chamfer edge must adjoin proven planar faces".into())
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let walls: [BoundaryPlane; 2] = walls
            .try_into()
            .map_err(|_| CadError::Invalid("Chamfer requires exactly two adjacent walls".into()))?;
        let normals = walls.each_ref().map(|wall| wall.normal);
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
        if !second.is_finite() || second <= EPSILON {
            return Err(CadError::Invalid(
                "Chamfer setback must be finite and positive above the modeling tolerance".into(),
            ));
        }
        let sum = normals[0] * spec.distance + normals[1] * second;
        let anchor = edge.front().point();
        let supporting_stock = !stock_bounds
            .iter()
            .map(|bounds| {
                charge_chamfer_proof(&mut proof_work, bounds.len().saturating_mul(normals.len()))?;
                normals.iter().try_fold(false, |outside, normal| {
                    Ok(outside || bounds.projection(anchor, *normal)?.1 > EPSILON)
                })
            })
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .any(|outside| outside);
        let origin = anchor - sum * (spec.distance * second * sine / sum.magnitude2());
        let normal = sum.normalize();
        if ![origin.x, origin.y, origin.z, normal.x, normal.y, normal.z]
            .iter()
            .all(|value| value.is_finite())
        {
            return Err(CadError::Invalid(
                "Chamfer distances are too large to construct a finite setback".into(),
            ));
        }
        let local = if supporting_stock {
            None
        } else {
            let Some(local) = local_convex_setback(
                solid,
                &edge,
                &walls,
                (origin, normal),
                [spec.distance, second],
                history,
                &mut proof_work,
            )?
            else {
                return Ok(None);
            };
            Some(local)
        };
        let mut sources = vec![history.edge(edge.id())?];
        sources.extend(
            faces
                .iter()
                .map(|face| history.face(face.id()))
                .collect::<Result<Vec<_>>>()?,
        );
        if let Some(plane) = planes.iter_mut().find(|plane| {
            plane.normal.dot(normal) > 1. - 1e-12
                && (origin - plane.origin).dot(normal).abs() <= EPSILON
        }) {
            for source in sources {
                if !plane.sources.contains(&source) {
                    plane.sources.push(source);
                }
            }
            match (&mut plane.local, local) {
                (Some(existing), Some(current)) => {
                    for wall in current.walls {
                        if !existing.walls.contains(&wall) {
                            existing.walls.push(wall);
                        }
                    }
                    existing.removals.extend(current.removals);
                }
                _ => plane.local = None,
            }
        } else {
            planes.push(BevelPlane {
                origin,
                normal,
                sources,
                local,
            });
        }
    }
    if seen.len() != selected.len() {
        return Ok(None);
    }
    let mut result = solid.clone();
    let mut bevels = HashSet::new();
    for plane in planes {
        result = clip(&result, &plane, history, &mut bevels, &mut proof_work)?;
    }
    Ok(Some(result))
}
