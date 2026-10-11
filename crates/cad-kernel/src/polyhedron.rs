//! Exact supporting-plane operations for convex, planar B-reps.
use cad_core::{CadError, Result};
use monstertruck::modeling::*;
use std::collections::HashMap;

const EPS: f64 = 1e-7;

// Direct editing is interactive and may reconstruct several scoped solids.
// Keep the admission/work contract local to that entry; legacy wall and draft
// callers retain their current support while using the unbounded hull wrapper.
const MAX_DIRECT_FACES: usize = 128;
const MAX_DIRECT_VERTICES: usize = 256;
const MAX_DIRECT_EDGES: usize = 512;
const MAX_DIRECT_BOUNDARY_POINTS: usize = 1024;
const MAX_DIRECT_WORK: usize = 64_000_000;

pub(crate) struct DirectEditBudget {
    remaining: usize,
}

impl DirectEditBudget {
    pub(crate) fn new() -> Self {
        Self {
            remaining: MAX_DIRECT_WORK,
        }
    }

    fn spend(&mut self, work: usize) -> Result<()> {
        self.remaining = self.remaining.checked_sub(work).ok_or_else(|| {
            CadError::Unsupported(
                "Direct Edit: Supporting-plane reconstruction exceeds the 64 million work-unit limit across edited bodies; reduce the edited body set or simplify its supporting faces".into(),
            )
        })?;
        Ok(())
    }

    pub(crate) fn admit(&mut self, solid: &Solid) -> Result<()> {
        let faces = solid.face_iter().take(MAX_DIRECT_FACES + 1).count();
        let vertices = solid.vertex_iter().take(MAX_DIRECT_VERTICES + 1).count();
        let edges = solid.edge_iter().take(MAX_DIRECT_EDGES + 1).count();
        if faces > MAX_DIRECT_FACES || vertices > MAX_DIRECT_VERTICES || edges > MAX_DIRECT_EDGES {
            return Err(CadError::Unsupported(
                "Direct Edit: Each edited source solid currently supports at most 128 supporting faces, 256 native vertices and 512 native edges; simplify this body before editing its faces".into(),
            ));
        }
        // Bound source convexity and duplicate-support checks before reading it.
        self.spend(faces * (vertices + faces) + edges)
    }
}

fn bounded_work(budget: &mut Option<&mut DirectEditBudget>, work: usize) -> Result<()> {
    if let Some(budget) = budget.as_deref_mut() {
        budget.spend(work)?;
    }
    Ok(())
}

fn invalid(message: &str) -> CadError {
    CadError::Invalid(format!("Geometry: {message}"))
}

#[derive(Clone, Copy)]
struct HalfSpace {
    normal: Vector3,
    distance: f64,
}

impl HalfSpace {
    fn signed(self, point: Point3, origin: Point3) -> f64 {
        (point - origin).dot(self.normal) - self.distance
    }
}

struct Polygon {
    points: Vec<Point3>,
    source: Option<Face>,
}

struct Polyhedron {
    origin: Point3,
    planes: Vec<HalfSpace>,
    faces: Vec<Face>,
}

impl Polyhedron {
    fn read(solid: &Solid) -> Result<Self> {
        if solid.boundaries().len() != 1 {
            return Err(CadError::Unsupported(
                "Planar wall offsets require one convex outer boundary without existing cavities"
                    .into(),
            ));
        }
        let points = solid.vertex_iter().map(|v| v.point()).collect::<Vec<_>>();
        if points.is_empty() {
            return Err(invalid("Body has no vertices"));
        }
        let origin = Point3::from_vec(
            points
                .iter()
                .fold(Vector3::zero(), |sum, p| sum + p.to_vec())
                / points.len() as f64,
        );
        let faces = solid.face_iter().cloned().collect::<Vec<_>>();
        let mut planes: Vec<HalfSpace> = Vec::new();
        for face in &faces {
            let wires = face.boundaries();
            if !matches!(face.surface(), Surface::Plane(_))
                || wires.len() != 1
                || wires[0].len() < 3
                || wires[0]
                    .iter()
                    .any(|e| !matches!(e.curve(), Curve::Line(_)))
            {
                return Err(CadError::Unsupported(
                    "Wall offsets currently support convex bodies with straight edges and planar faces; curved faces and existing pockets require a surface-offset adapter".into(),
                ));
            }
            let normal = face.oriented_surface().normal(0., 0.).normalize();
            let distance = (wires[0].front().unwrap().front().point() - origin).dot(normal);
            let plane = HalfSpace { normal, distance };
            if !distance.is_finite() || !normal.magnitude2().is_finite() {
                return Err(invalid("Invalid supporting plane"));
            }
            if points.iter().any(|p| plane.signed(*p, origin) > EPS) {
                return Err(CadError::Unsupported(
                    "Wall offsets for concave bodies require trimming intersecting offset surfaces"
                        .into(),
                ));
            }
            if planes
                .iter()
                .any(|p| p.normal.dot(normal) > 1. - 1e-12 && (p.distance - distance).abs() <= EPS)
            {
                return Err(CadError::Unsupported(
                    "Wall offsets require unsplit supporting faces; this body has coplanar face subdivisions".into(),
                ));
            }
            planes.push(plane);
        }
        Ok(Self {
            origin,
            planes,
            faces,
        })
    }

    // Work relative to a local origin to avoid cancellation on translated parts.
    fn hull(&self, planes: &[HalfSpace]) -> Result<Vec<Vec<Point3>>> {
        self.hull_with_budget(planes, None)
    }

    fn hull_with_budget(
        &self,
        planes: &[HalfSpace],
        mut budget: Option<&mut DirectEditBudget>,
    ) -> Result<Vec<Vec<Point3>>> {
        // A finite list of plane intersections alone cannot detect an open
        // recession cone. Reject drafts whose half-spaces no longer bound it.
        for (i, a) in planes.iter().enumerate() {
            for b in &planes[i + 1..] {
                bounded_work(&mut budget, 1)?;
                let ray = a.normal.cross(b.normal);
                if ray.magnitude2() <= 1e-20 {
                    continue;
                }
                let ray = ray.normalize();
                for ray in [ray, -ray] {
                    bounded_work(&mut budget, planes.len())?;
                    if planes.iter().all(|p| p.normal.dot(ray) <= 1e-10) {
                        return Err(invalid("Draft angle leaves an unbounded body"));
                    }
                }
            }
        }
        let mut vertices: Vec<Point3> = Vec::new();
        for a in 0..planes.len() {
            for b in a + 1..planes.len() {
                for c in b + 1..planes.len() {
                    bounded_work(&mut budget, 1)?;
                    let (a, b, c) = (planes[a], planes[b], planes[c]);
                    let determinant = a.normal.dot(b.normal.cross(c.normal));
                    if determinant.abs() <= 1e-10 {
                        continue;
                    }
                    let point = self.origin
                        + (b.normal.cross(c.normal) * a.distance
                            + c.normal.cross(a.normal) * b.distance
                            + a.normal.cross(b.normal) * c.distance)
                            / determinant;
                    if !point.x.is_finite() || !point.y.is_finite() || !point.z.is_finite() {
                        return Err(invalid("Offset plane intersection is non-finite"));
                    }
                    bounded_work(&mut budget, planes.len())?;
                    if planes.iter().all(|p| p.signed(point, self.origin) <= EPS) {
                        bounded_work(&mut budget, vertices.len())?;
                        if vertices.iter().all(|v| v.distance(point) > EPS) {
                            if budget.is_some() && vertices.len() == MAX_DIRECT_VERTICES {
                                return Err(CadError::Unsupported(
                                    "Direct Edit: Reconstructed supporting planes exceed the 256-vertex limit; simplify the body or reduce the face edit".into(),
                                ));
                            }
                            vertices.push(point);
                        }
                    }
                }
            }
        }
        if vertices.len() < 4 {
            return Err(invalid(
                "Offset thickness or draft angle collapses the body",
            ));
        }
        let mut polygons = Vec::new();
        for plane in planes {
            bounded_work(&mut budget, vertices.len() * 12)?;
            let mut points = vertices
                .iter()
                .copied()
                .filter(|p| plane.signed(*p, self.origin).abs() <= EPS)
                .collect::<Vec<_>>();
            if points.len() >= 3 {
                let center = Point3::from_vec(
                    points.iter().fold(Vector3::zero(), |s, p| s + p.to_vec())
                        / points.len() as f64,
                );
                let u = (points[0] - center).normalize();
                let v = plane.normal.cross(u);
                points.sort_by(|a, b| {
                    let a = *a - center;
                    let b = *b - center;
                    a.dot(v)
                        .atan2(a.dot(u))
                        .total_cmp(&b.dot(v).atan2(b.dot(u)))
                });
                if area_vector(&points).dot(plane.normal) <= EPS * EPS {
                    points.clear();
                }
            } else {
                points.clear();
            }
            polygons.push(points);
        }
        let volume = polygons
            .iter()
            .flat_map(|p| {
                p.get(1..).unwrap_or_default().windows(2).map(move |q| {
                    (p[0] - self.origin).dot((q[0] - self.origin).cross(q[1] - self.origin)) / 6.
                })
            })
            .sum::<f64>();
        if !volume.is_finite() || volume <= EPS.powi(3) {
            return Err(invalid(
                "Offset thickness or draft angle leaves no enclosed volume",
            ));
        }
        Ok(polygons)
    }
}

fn area_vector(points: &[Point3]) -> Vector3 {
    let Some(origin) = points.first() else {
        return Vector3::zero();
    };
    points[1..].windows(2).fold(Vector3::zero(), |sum, p| {
        sum + (p[0] - *origin).cross(p[1] - *origin)
    }) * 0.5
}

pub(crate) enum FaceEdit {
    Offset(f64),
    Move(Matrix4),
    DeleteHeal,
    Replace(cad_core::Plane, bool),
}

/// Reconstruct the exact intersection of edited supporting planes. This retains
/// the native carrier for unchanged faces and proves a finite closed volume.
pub(crate) fn edit_faces(
    solid: &Solid,
    selected: &[FaceId],
    edit: &FaceEdit,
    history: &mut crate::shell_history::ShellHistory,
    budget: &mut DirectEditBudget,
) -> Result<Solid> {
    let poly = Polyhedron::read(solid)?;
    let mut planes = Vec::new();
    let mut sources = Vec::new();
    let mut changed = Vec::new();
    for (face, plane) in poly.faces.iter().zip(&poly.planes) {
        let selected = selected.contains(&face.id());
        if selected && matches!(edit, FaceEdit::DeleteHeal) {
            continue;
        }
        let mut plane = *plane;
        if selected {
            match edit {
                FaceEdit::Offset(distance) => plane.distance += distance,
                FaceEdit::Move(matrix) => {
                    let point = matrix.transform_point(poly.origin + plane.normal * plane.distance);
                    plane.normal = matrix.transform_vector(plane.normal).normalize();
                    plane.distance = (point - poly.origin).dot(plane.normal);
                }
                FaceEdit::Replace(replacement, reverse) => {
                    let u = Vector3::from(replacement.u);
                    let v = Vector3::from(replacement.v);
                    let mut normal = u.cross(v).normalize();
                    if normal.dot(plane.normal) < 0. {
                        normal = -normal;
                    }
                    if *reverse {
                        normal = -normal;
                    }
                    plane.normal = normal;
                    plane.distance = (Point3::from(replacement.origin) - poly.origin).dot(normal);
                }
                FaceEdit::DeleteHeal => unreachable!(),
            }
        }
        if !plane.distance.is_finite() || !plane.normal.magnitude2().is_finite() {
            return Err(invalid("Direct face edit has a nonfinite supporting plane"));
        }
        planes.push(plane);
        sources.push(face);
        changed.push(selected);
    }
    if planes.len() < 4 {
        return Err(invalid(
            "Face healing leaves too few supporting planes to enclose a body",
        ));
    }
    let polygons = poly.hull_with_budget(&planes, Some(&mut *budget))?;
    if polygons.iter().any(|points| points.len() < 3) {
        return Err(invalid(
            "Face edit removes another supporting face; use an explicit delete/heal operation",
        ));
    }
    let boundary_points = polygons.iter().map(Vec::len).sum::<usize>();
    if boundary_points > MAX_DIRECT_BOUNDARY_POINTS {
        return Err(CadError::Unsupported(
            "Direct Edit: Reconstructed boundaries exceed the 1024 polygon-point limit; simplify the body or reduce the face edit".into(),
        ));
    }
    // Charge worst-case welding, collinear split scans and segment ordering
    // before allocating shared topology. The hull limits every vertex set.
    budget.spend(boundary_points * MAX_DIRECT_VERTICES * 12)?;
    let mut tags = Vec::new();
    let mut boundary_polygons = Vec::new();
    for ((points, source), changed) in polygons.into_iter().zip(sources).zip(changed) {
        if changed {
            tags.push(crate::shell_history::FaceTag::Constructed {
                sources:vec![history.face(source.id())?],
                purpose:serde_json::json!({"operation":"direct_edit","constructor":"edited_supporting_plane"}),
            });
        } else {
            tags.push(crate::shell_history::FaceTag::Retained(source.id()));
        }
        boundary_polygons.push(Polygon {
            points,
            source: if changed { None } else { Some(source.clone()) },
        });
    }
    let shell = boundary_traced(boundary_polygons, Some(history), tags)?;
    Solid::try_new(vec![shell]).map_err(|e| invalid(&e.to_string()))
}

fn clip_polygon(points: &[Point3], plane: HalfSpace, origin: Point3, inside: bool) -> Vec<Point3> {
    let mut result = Vec::new();
    for i in 0..points.len() {
        let (a, b) = (points[i], points[(i + 1) % points.len()]);
        let sign = if inside { 1. } else { -1. };
        let (da, db) = (
            plane.signed(a, origin) * sign,
            plane.signed(b, origin) * sign,
        );
        let (ia, ib) = (da <= EPS, db <= EPS);
        if ia {
            result.push(a);
        }
        if ia != ib {
            result.push(if da.abs() <= EPS {
                a
            } else if db.abs() <= EPS {
                b
            } else {
                a + (b - a) * (da / (da - db))
            });
        }
    }
    result.dedup_by(|a, b| a.distance(*b) <= EPS);
    if result
        .first()
        .zip(result.last())
        .is_some_and(|(a, b)| a.distance(*b) <= EPS)
    {
        result.pop();
    }
    if result.len() < 3 || area_vector(&result).magnitude() <= EPS * EPS {
        result.clear();
    }
    result
}

// Weld exact plane intersections once, then split every boundary at shared
// collinear vertices. This also closes adjacent openings without T-junctions.
fn boundary(polygons: Vec<Polygon>) -> Result<Shell> {
    boundary_traced(polygons, None, Vec::new())
}
fn boundary_traced(
    polygons: Vec<Polygon>,
    mut history: Option<&mut crate::shell_history::ShellHistory>,
    tags: Vec<crate::shell_history::FaceTag>,
) -> Result<Shell> {
    let mut tags = tags.into_iter();
    let mut vertices: Vec<Vertex> = Vec::new();
    let mut loops = Vec::new();
    for polygon in &polygons {
        let indices = polygon
            .points
            .iter()
            .map(|point| {
                if let Some(i) = vertices
                    .iter()
                    .position(|v| v.point().distance(*point) <= EPS)
                {
                    i
                } else {
                    vertices.push(builder::vertex(*point));
                    vertices.len() - 1
                }
            })
            .collect::<Vec<_>>();
        loops.push(indices);
    }
    let mut edges: HashMap<(usize, usize), Edge> = HashMap::new();
    let mut faces = Vec::new();
    for (polygon, indices) in polygons.into_iter().zip(loops) {
        let mut wire = Wire::new();
        for i in 0..indices.len() {
            let (a, b) = (indices[i], indices[(i + 1) % indices.len()]);
            let start = vertices[a].point();
            let direction = vertices[b].point() - start;
            if direction.magnitude2() <= EPS * EPS {
                return Err(invalid("Offset creates a collapsed edge"));
            }
            let mut segment = vec![(0., a), (1., b)];
            for (j, vertex) in vertices.iter().enumerate() {
                if j == a || j == b {
                    continue;
                }
                let t = (vertex.point() - start).dot(direction) / direction.magnitude2();
                if t > 0. && t < 1. && vertex.point().distance(start + direction * t) <= EPS {
                    segment.push((t, j));
                }
            }
            segment.sort_by(|a, b| a.0.total_cmp(&b.0));
            for pair in segment.windows(2) {
                let (a, b) = (pair[0].1, pair[1].1);
                let edge = if let Some(edge) = edges.get(&(a, b)) {
                    edge.clone()
                } else {
                    let edge = builder::line(&vertices[a], &vertices[b]);
                    edges.insert((a, b), edge.clone());
                    edges.insert((b, a), edge.inverse());
                    edge
                };
                wire.push_back(edge);
            }
        }
        let face = if let Some(source) = polygon.source {
            let wire = if source.orientation() {
                wire
            } else {
                wire.inverse()
            };
            let face =
                Face::try_new(vec![wire], source.surface()).map_err(|e| invalid(&e.to_string()))?;
            if source.orientation() {
                face
            } else {
                face.inverse()
            }
        } else {
            builder::try_attach_plane(vec![wire]).map_err(|e| invalid(&e.to_string()))?
        };
        if let Some(history) = history.as_deref_mut() {
            tags.next()
                .ok_or_else(|| invalid("Shell polygon constructor tag is missing"))?
                .assign(history, face.id())?;
        }
        faces.push(face);
    }
    Ok(faces.into())
}

pub(crate) fn shell(
    solid: &Solid,
    thickness: f64,
    removed: &[FaceId],
    outward: bool,
    face_thicknesses: &HashMap<FaceId, f64>,
    history: &mut crate::shell_history::ShellHistory,
) -> Result<Vec<Solid>> {
    let source = Polyhedron::read(solid)?;
    let opening = source
        .faces
        .iter()
        .map(|f| removed.contains(&f.id()))
        .collect::<Vec<_>>();
    if opening.iter().all(|v| *v) {
        return Err(invalid("Shell needs at least one retained wall"));
    }
    let outer_planes = source
        .planes
        .iter()
        .enumerate()
        .map(|(index, p)| HalfSpace {
            distance: p.distance
                + if outward && !opening[index] {
                    face_thicknesses
                        .get(&source.faces[index].id())
                        .copied()
                        .unwrap_or(thickness)
                } else {
                    0.
                },
            ..*p
        })
        .collect::<Vec<_>>();
    let outer = if outward {
        source.hull(&outer_planes)?
    } else {
        source
            .faces
            .iter()
            .map(|face| {
                face.boundaries()[0]
                    .iter()
                    .map(|edge| edge.front().point())
                    .collect()
            })
            .collect()
    };
    let inner_planes = source
        .planes
        .iter()
        .zip(&opening)
        .enumerate()
        .map(|(index, (p, open))| HalfSpace {
            distance: p.distance
                - if *open || outward {
                    0.
                } else {
                    face_thicknesses
                        .get(&source.faces[index].id())
                        .copied()
                        .unwrap_or(thickness)
                },
            ..*p
        })
        .collect::<Vec<_>>();
    let inner = source.hull(&inner_planes)?;
    let mut polygons = Vec::new();
    let mut tags = Vec::new();
    for (i, face) in source.faces.iter().enumerate() {
        let outer = outer[i].clone();
        if outer.is_empty() {
            return Err(invalid("Shell offset removes a retained supporting wall"));
        }
        if !opening[i] {
            polygons.push(Polygon {
                points: outer,
                source: (!outward).then(|| face.clone()),
            });
            tags.push(if outward {crate::shell_history::FaceTag::Constructed {sources:vec![history.face(face.id())?],purpose:serde_json::json!({"operation":"shell","constructor":"outer_offset_wall"})}} else {crate::shell_history::FaceTag::Retained(face.id())});
            if !inner[i].is_empty() {
                let mut points = inner[i].clone();
                points.reverse();
                polygons.push(Polygon {
                    points,
                    source: outward.then(|| face.inverse()),
                });
                tags.push(if outward {crate::shell_history::FaceTag::Retained(face.id())} else {crate::shell_history::FaceTag::Constructed {sources:vec![history.face(face.id())?],purpose:serde_json::json!({"operation":"shell","constructor":"inner_offset_wall"})}});
            }
            continue;
        }
        if inner[i].is_empty() {
            return Err(invalid(
                "Shell thickness closes a selected opening; reduce thickness",
            ));
        }
        let mut remaining = outer;
        let mut prior = Vec::new();
        for ((plane, open), clipper) in inner_planes.iter().zip(&opening).zip(&source.faces) {
            if *open || remaining.is_empty() {
                continue;
            }
            let rim = clip_polygon(&remaining, *plane, source.origin, false);
            if !rim.is_empty() {
                polygons.push(Polygon {
                    points: rim,
                    source: Some(face.clone()),
                });
                let mut sources = vec![history.face(face.id())?, history.face(clipper.id())?];
                sources.extend(prior.clone());
                tags.push(crate::shell_history::FaceTag::Rim {opening:face.id(),sources,purpose:serde_json::json!({"operation":"shell","constructor":"planar_opening_partition","clipper":crate::adapter::construction_reference(&history.face(clipper.id())?)?,"prior_clippers":crate::shell_history::closure(&prior)?})});
            }
            remaining = clip_polygon(&remaining, *plane, source.origin, true);
            prior.push(history.face(clipper.id())?);
        }
    }
    let components = boundary_traced(polygons, Some(history), tags)?.connected_components();
    if removed.is_empty() {
        return Ok(vec![
            Solid::try_new(components).map_err(|e| invalid(&e.to_string()))?,
        ]);
    }
    components
        .into_iter()
        .map(|s| Solid::try_new(vec![s]).map_err(|e| invalid(&e.to_string())))
        .collect()
}

/// Offset a proven convex supporting-plane envelope. Opening planes remain
/// fixed so a following pocket reconstruction and shell rim share the exact
/// source section, rather than a padded or sampled cap.
pub(crate) fn offset_envelope(
    solid: &Solid,
    thickness: f64,
    fixed: &[FaceId],
    outward: bool,
    face_thicknesses: &HashMap<FaceId, f64>,
    history: &mut crate::shell_history::ShellHistory,
) -> Result<Solid> {
    let source = Polyhedron::read(solid)?;
    let planes = source
        .planes
        .iter()
        .zip(&source.faces)
        .map(|(plane, face)| {
            let amount = if fixed.contains(&face.id()) {
                0.
            } else {
                face_thicknesses
                    .get(&face.id())
                    .copied()
                    .unwrap_or(thickness)
            };
            let distance = plane.distance + if outward { amount } else { -amount };
            if !distance.is_finite() || !amount.is_finite() || amount < 0. {
                return Err(invalid(
                    "Wall offset exceeds finite positive thickness coordinates",
                ));
            }
            Ok(HalfSpace { distance, ..*plane })
        })
        .collect::<Result<Vec<_>>>()?;
    let polygons = source
        .hull(&planes)?
        .into_iter()
        .map(|points| {
            if points.is_empty() {
                return Err(invalid("Wall offset removes a supporting face"));
            }
            Ok(Polygon {
                points,
                source: None,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let tags=source.faces.iter().map(|face|Ok(crate::shell_history::FaceTag::Constructed {sources:vec![history.face(face.id())?],purpose:serde_json::json!({"operation":"shell","constructor":if outward {"outer_offset_wall"}else{"inner_offset_wall"}})})).collect::<Result<Vec<_>>>()?;
    let result = Solid::try_new(vec![boundary_traced(polygons, Some(history), tags)?])
        .map_err(|error| invalid(&error.to_string()))?;
    crate::validation::solid(&result)?;
    Ok(result)
}

pub(crate) fn draft(
    solid: &Solid,
    selected: &[FaceId],
    angle: f64,
    origin: Point3,
    normal: Vector3,
) -> Result<Solid> {
    let source = Polyhedron::read(solid)?;
    let mut planes = source.planes.clone();
    for (i, face) in source.faces.iter().enumerate() {
        if !selected.contains(&face.id()) {
            continue;
        }
        let plane = planes[i];
        if plane.normal.dot(normal).abs() > 1e-7 {
            return Err(CadError::Unsupported("Neutral-plane draft currently requires selected planar walls perpendicular to the neutral plane".into()));
        }
        planes[i] = HalfSpace {
            normal: plane.normal * angle.cos() + normal * angle.sin(),
            distance: plane.distance * angle.cos()
                + (origin - source.origin).dot(normal) * angle.sin(),
        };
    }
    let polygons = source
        .hull(&planes)?
        .into_iter()
        .enumerate()
        .filter(|(_, p)| !p.is_empty())
        .map(|(i, points)| Polygon {
            points,
            source: (!selected.contains(&source.faces[i].id())).then(|| source.faces[i].clone()),
        })
        .collect();
    Solid::try_new(vec![boundary(polygons)?]).map_err(|e| invalid(&e.to_string()))
}
