//! Bounded display-only subtraction of tessellated solid boundaries. This
//! cache is approximate; neither its polygons nor its volumes author CAD data.
use cad_core::{
    CadError, NodeRef, Result, TopologyRef,
    protocol::{BodyKind, FaceRange, MeshData},
};
use monstertruck::modeling::{InnerSpace, Vector3, Zero};

const MAX_INPUT_TRIANGLES: usize = 16_384;
const MAX_POLYGONS: usize = 131_072;
const MAX_WORK: usize = 8_000_000;
const MAX_DEPTH: usize = 192;
const MAX_OUTPUT_TRIANGLES: usize = 65_536;

fn unavailable() -> CadError {
    CadError::Unsupported(
        "Localized tessellated preview exceeds its geometry/work bound; the validated model is displayed neutrally".into(),
    )
}

struct Budget {
    remaining: usize,
    generated: usize,
    epsilon: f64,
}
impl Budget {
    fn spend(&mut self, count: usize) -> Result<()> {
        self.remaining = self.remaining.checked_sub(count).ok_or_else(unavailable)?;
        Ok(())
    }
    fn polygon(&mut self) -> Result<()> {
        self.generated += 1;
        if self.generated > MAX_POLYGONS {
            return Err(unavailable());
        }
        Ok(())
    }
}

#[derive(Clone)]
struct Vertex {
    position: Vector3,
    normal: Vector3,
}
impl Vertex {
    fn interpolate(&self, other: &Self, fraction: f64) -> Self {
        Self {
            position: self.position + (other.position - self.position) * fraction,
            normal: self.normal + (other.normal - self.normal) * fraction,
        }
    }
}

#[derive(Clone, Copy)]
struct Plane {
    normal: Vector3,
    offset: f64,
}
impl Plane {
    fn flip(&mut self) {
        self.normal = -self.normal;
        self.offset = -self.offset;
    }

    fn split(
        self,
        polygon: Polygon,
        budget: &mut Budget,
        coplanar_front: &mut Vec<Polygon>,
        coplanar_back: &mut Vec<Polygon>,
        front: &mut Vec<Polygon>,
        back: &mut Vec<Polygon>,
    ) -> Result<()> {
        budget.spend(polygon.vertices.len())?;
        let types = polygon
            .vertices
            .iter()
            .map(|vertex| {
                let distance = self.normal.dot(vertex.position) - self.offset;
                if distance < -budget.epsilon {
                    2
                } else if distance > budget.epsilon {
                    1
                } else {
                    0
                }
            })
            .collect::<Vec<_>>();
        match types.iter().fold(0, |all, side| all | side) {
            0 => {
                if self.normal.dot(polygon.plane.normal) >= 0. {
                    coplanar_front.push(polygon);
                } else {
                    coplanar_back.push(polygon);
                }
            }
            1 => front.push(polygon),
            2 => back.push(polygon),
            _ => {
                let mut f = Vec::new();
                let mut b = Vec::new();
                for i in 0..polygon.vertices.len() {
                    let j = (i + 1) % polygon.vertices.len();
                    let (a, z) = (&polygon.vertices[i], &polygon.vertices[j]);
                    if types[i] != 2 {
                        f.push(a.clone());
                    }
                    if types[i] != 1 {
                        b.push(a.clone());
                    }
                    if types[i] | types[j] == 3 {
                        let denominator = self.normal.dot(z.position - a.position);
                        if denominator.abs() <= f64::EPSILON {
                            return Err(unavailable());
                        }
                        let fraction = ((self.offset - self.normal.dot(a.position)) / denominator)
                            .clamp(0., 1.);
                        let vertex = a.interpolate(z, fraction);
                        f.push(vertex.clone());
                        b.push(vertex);
                    }
                }
                for (vertices, target) in [(f, front), (b, back)] {
                    if let Some(piece) = Polygon::new(vertices, budget.epsilon) {
                        budget.polygon()?;
                        target.push(piece);
                    }
                }
            }
        }
        Ok(())
    }
}

#[derive(Clone)]
struct Polygon {
    vertices: Vec<Vertex>,
    plane: Plane,
}
impl Polygon {
    fn new(mut vertices: Vec<Vertex>, epsilon: f64) -> Option<Self> {
        vertices.dedup_by(|a, b| (a.position - b.position).magnitude2() <= epsilon * epsilon);
        if vertices.len() > 2
            && (vertices[0].position - vertices.last()?.position).magnitude2() <= epsilon * epsilon
        {
            vertices.pop();
        }
        if vertices.len() < 3 {
            return None;
        }
        let first = vertices[0].position;
        let normal = vertices[1..]
            .windows(2)
            .map(|pair| (pair[0].position - first).cross(pair[1].position - first))
            .max_by(|a, b| a.magnitude2().total_cmp(&b.magnitude2()))?;
        if normal.magnitude2() <= epsilon.powi(4) {
            return None;
        }
        let normal = normal.normalize();
        Some(Self {
            vertices,
            plane: Plane {
                normal,
                offset: normal.dot(first),
            },
        })
    }
    fn flip(&mut self) {
        self.vertices.reverse();
        for vertex in &mut self.vertices {
            vertex.normal = -vertex.normal;
        }
        self.plane.flip();
    }
}

#[derive(Default)]
struct Node {
    plane: Option<Plane>,
    polygons: Vec<Polygon>,
    front: Option<Box<Node>>,
    back: Option<Box<Node>>,
}
impl Node {
    fn partition_plane(polygons: &[Polygon], budget: &mut Budget) -> Result<Plane> {
        // A plane is an exact polygon carrier; only its ordering is heuristic.
        // Picking the first triangle produces a nearly linear tree on curved
        // stock and can exhaust depth even for a small bore or fillet preview.
        // Bound both the candidate set and scoring sample independently of
        // input tessellation. The full split still uses every polygon below.
        let candidates = polygons.len().min(12);
        let samples = polygons.len().min(128);
        let mut best = (usize::MAX, polygons[0].plane);
        for candidate in 0..candidates {
            let index = candidate * (polygons.len() - 1) / (candidates - 1).max(1);
            let plane = polygons[index].plane;
            let (mut front, mut back, mut spanning) = (0usize, 0usize, 0usize);
            for sample in 0..samples {
                let polygon = &polygons[sample * (polygons.len() - 1) / (samples - 1).max(1)];
                budget.spend(polygon.vertices.len())?;
                let mut sides = 0;
                for vertex in &polygon.vertices {
                    let distance = plane.normal.dot(vertex.position) - plane.offset;
                    if !distance.is_finite() {
                        return Err(unavailable());
                    }
                    sides |= if distance < -budget.epsilon {
                        2
                    } else if distance > budget.epsilon {
                        1
                    } else {
                        0
                    };
                }
                match sides {
                    1 => front += 1,
                    2 => back += 1,
                    3 => {
                        front += 1;
                        back += 1;
                        spanning += 1;
                    }
                    _ => {}
                }
            }
            let score = spanning * 4 + front.abs_diff(back);
            if score < best.0 {
                best = (score, plane);
            }
        }
        Ok(best.1)
    }

    fn invert(&mut self) {
        for polygon in &mut self.polygons {
            polygon.flip();
        }
        if let Some(plane) = &mut self.plane {
            plane.flip();
        }
        if let Some(front) = &mut self.front {
            front.invert();
        }
        if let Some(back) = &mut self.back {
            back.invert();
        }
        std::mem::swap(&mut self.front, &mut self.back);
    }

    fn build(&mut self, polygons: Vec<Polygon>, budget: &mut Budget, depth: usize) -> Result<()> {
        if polygons.is_empty() {
            return Ok(());
        }
        if depth > MAX_DEPTH || polygons.len() > MAX_POLYGONS {
            return Err(unavailable());
        }
        let plane = match self.plane {
            Some(plane) => plane,
            None => {
                let plane = Self::partition_plane(&polygons, budget)?;
                self.plane = Some(plane);
                plane
            }
        };
        let (mut front, mut back, mut opposite) = (Vec::new(), Vec::new(), Vec::new());
        for polygon in polygons {
            plane.split(
                polygon,
                budget,
                &mut self.polygons,
                &mut opposite,
                &mut front,
                &mut back,
            )?;
        }
        self.polygons.extend(opposite);
        if !front.is_empty() {
            self.front
                .get_or_insert_with(Default::default)
                .build(front, budget, depth + 1)?;
        }
        if !back.is_empty() {
            self.back
                .get_or_insert_with(Default::default)
                .build(back, budget, depth + 1)?;
        }
        Ok(())
    }

    fn clip_polygons(
        &self,
        polygons: Vec<Polygon>,
        budget: &mut Budget,
        depth: usize,
    ) -> Result<Vec<Polygon>> {
        if depth > MAX_DEPTH || polygons.len() > MAX_POLYGONS {
            return Err(unavailable());
        }
        let Some(plane) = self.plane else {
            return Ok(polygons);
        };
        let (mut front, mut back, mut cf, mut cb) =
            (Vec::new(), Vec::new(), Vec::new(), Vec::new());
        for polygon in polygons {
            plane.split(polygon, budget, &mut cf, &mut cb, &mut front, &mut back)?;
        }
        front.extend(cf);
        back.extend(cb);
        if let Some(node) = &self.front {
            front = node.clip_polygons(front, budget, depth + 1)?;
        }
        if let Some(node) = &self.back {
            back = node.clip_polygons(back, budget, depth + 1)?;
        } else {
            back.clear();
        }
        front.extend(back);
        Ok(front)
    }

    fn clip_to(&mut self, other: &Self, budget: &mut Budget, depth: usize) -> Result<()> {
        self.polygons = other.clip_polygons(std::mem::take(&mut self.polygons), budget, 0)?;
        if depth > MAX_DEPTH {
            return Err(unavailable());
        }
        if let Some(front) = &mut self.front {
            front.clip_to(other, budget, depth + 1)?;
        }
        if let Some(back) = &mut self.back {
            back.clip_to(other, budget, depth + 1)?;
        }
        Ok(())
    }

    fn collect(self, output: &mut Vec<Polygon>) -> Result<()> {
        output.extend(self.polygons);
        if output.len() > MAX_POLYGONS {
            return Err(unavailable());
        }
        if let Some(front) = self.front {
            front.collect(output)?;
        }
        if let Some(back) = self.back {
            back.collect(output)?;
        }
        Ok(())
    }
}

fn polygons(mesh: &MeshData, origin: Vector3, epsilon: f64) -> Result<Vec<Polygon>> {
    if mesh.indices.len() / 3 > MAX_INPUT_TRIANGLES {
        return Err(unavailable());
    }
    let mut polygons = Vec::new();
    for face in &mesh.faces {
        if face.body_kind != BodyKind::Solid {
            return Err(CadError::Unsupported(
                "Open surface meshes are ineligible for material-volume subtraction".into(),
            ));
        }
        for triangle in mesh.indices[face.start..face.start + face.count]
            .as_chunks::<3>()
            .0
        {
            let vertices = triangle
                .iter()
                .map(|index| Vertex {
                    position: Vector3::from(mesh.positions[*index as usize]) - origin,
                    normal: Vector3::from(mesh.normals[*index as usize]),
                })
                .collect();
            if let Some(polygon) = Polygon::new(vertices, epsilon) {
                polygons.push(polygon);
            }
        }
    }
    Ok(polygons)
}

fn subtract(a: Vec<Polygon>, b: Vec<Polygon>, budget: &mut Budget) -> Result<Vec<Polygon>> {
    if a.is_empty() || b.is_empty() {
        return Ok(a);
    }
    let (mut a_node, mut b_node) = (Node::default(), Node::default());
    a_node.build(a, budget, 0)?;
    b_node.build(b, budget, 0)?;
    a_node.invert();
    a_node.clip_to(&b_node, budget, 0)?;
    b_node.clip_to(&a_node, budget, 0)?;
    b_node.invert();
    b_node.clip_to(&a_node, budget, 0)?;
    b_node.invert();
    let mut remaining = Vec::new();
    b_node.collect(&mut remaining)?;
    a_node.build(remaining, budget, 0)?;
    a_node.invert();
    let mut result = Vec::new();
    a_node.collect(&mut result)?;
    Ok(result)
}

fn mesh(
    polygons: Vec<Polygon>,
    owner: &NodeRef,
    origin: Vector3,
    epsilon: f64,
) -> Result<Option<MeshData>> {
    if polygons.is_empty() {
        return Ok(None);
    }
    let mut mesh = MeshData {
        positions: Vec::new(),
        normals: Vec::new(),
        indices: Vec::new(),
        faces: Vec::new(),
        edges: Vec::new(),
        bounds: [[f64::INFINITY; 3], [f64::NEG_INFINITY; 3]],
        volume_mm3: 0.,
        body_counts: None,
    };
    let mut signed_volume = 0.;
    for polygon in polygons {
        for index in 1..polygon.vertices.len() - 1 {
            if mesh.indices.len() / 3 >= MAX_OUTPUT_TRIANGLES {
                return Err(unavailable());
            }
            let triangle = [
                &polygon.vertices[0],
                &polygon.vertices[index],
                &polygon.vertices[index + 1],
            ];
            if (triangle[1].position - triangle[0].position)
                .cross(triangle[2].position - triangle[0].position)
                .magnitude2()
                <= epsilon.powi(4)
            {
                continue;
            }
            signed_volume += triangle[0]
                .position
                .dot(triangle[1].position.cross(triangle[2].position))
                / 6.;
            for vertex in triangle {
                let position = vertex.position + origin;
                let normal = if vertex.normal.magnitude2() > 1e-18 {
                    vertex.normal.normalize()
                } else {
                    polygon.plane.normal
                };
                mesh.indices.push(mesh.positions.len() as u32);
                mesh.positions.push(position.into());
                mesh.normals.push(normal.into());
                for (axis, value) in [position.x, position.y, position.z].into_iter().enumerate() {
                    mesh.bounds[0][axis] = mesh.bounds[0][axis].min(value);
                    mesh.bounds[1][axis] = mesh.bounds[1][axis].max(value);
                }
            }
        }
    }
    if mesh.indices.is_empty() || signed_volume.abs() <= epsilon.powi(3) {
        return Ok(None);
    }
    mesh.faces.push(FaceRange {
        start: 0,
        count: mesh.indices.len(),
        reference: TopologyRef {
            producer: owner.clone(),
            role: "operation-preview:tessellated-display-only".into(),
            topology: "face".into(),
            signature: None,
        },
        body: Some(owner.clone()),
        body_kind: BodyKind::Solid,
        plane: None,
    });
    mesh.volume_mm3 = signed_volume.abs();
    Ok(Some(mesh))
}

pub(crate) fn difference(
    before: &MeshData,
    after: &MeshData,
    owner: &NodeRef,
) -> Result<(Option<MeshData>, Option<MeshData>)> {
    let mut origin = Vector3::zero();
    let mut diameter: f64 = 0.;
    for axis in 0..3 {
        let lo = before.bounds[0][axis].min(after.bounds[0][axis]);
        let hi = before.bounds[1][axis].max(after.bounds[1][axis]);
        origin[axis] = (lo + hi) * 0.5;
        diameter = diameter.max(hi - lo);
    }
    let epsilon = (diameter * 1e-10).max(1e-8);
    let mut budget = Budget {
        remaining: MAX_WORK,
        generated: 0,
        epsilon,
    };
    let before = polygons(before, origin, epsilon)?;
    let after = polygons(after, origin, epsilon)?;
    let volume = |polygons: &[Polygon]| {
        polygons
            .iter()
            .map(|polygon| {
                polygon.vertices[1..]
                    .windows(2)
                    .map(|pair| {
                        polygon.vertices[0]
                            .position
                            .dot(pair[0].position.cross(pair[1].position))
                            / 6.
                    })
                    .sum::<f64>()
            })
            .sum::<f64>()
            .abs()
    };
    let before_volume = volume(&before);
    let after_volume = volume(&after);
    let surface_area: f64 = before
        .iter()
        .chain(&after)
        .map(|polygon| {
            polygon.vertices[1..]
                .windows(2)
                .map(|pair| {
                    (pair[0].position - polygon.vertices[0].position)
                        .cross(pair[1].position - polygon.vertices[0].position)
                        .magnitude()
                        * 0.5
                })
                .sum::<f64>()
        })
        .sum();
    let added = subtract(after.clone(), before.clone(), &mut budget)?;
    let removed = subtract(before, after, &mut budget)?;
    let added_volume = volume(&added);
    let removed_volume = volume(&removed);
    let error_bound = (surface_area * epsilon * 32.)
        .max(before_volume.max(after_volume) * 1e-7)
        .max(epsilon.powi(3));
    if !added_volume.is_finite()
        || !removed_volume.is_finite()
        || added_volume > after_volume + error_bound
        || removed_volume > before_volume + error_bound
        || ((added_volume - removed_volume) - (after_volume - before_volume)).abs() > error_bound
    {
        return Err(CadError::Unsupported(
            "Localized tessellated preview did not preserve the display boundary's material balance; the validated model is displayed neutrally".into(),
        ));
    }
    Ok((
        mesh(added, owner, origin, epsilon)?,
        mesh(removed, owner, origin, epsilon)?,
    ))
}
