//! Application-owned engineering properties derived from the evaluated boundary mesh.
//! Values describe that tessellation; they are not an exact B-rep integration certificate.
use crate::{CadError, Document, Id, Result, protocol::MeshData};
use nalgebra::{Matrix3, Vector3};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
pub mod report;

pub const CONTRACT_VERSION: u32 = 1;
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Metrics {
    pub area_mm2: f64,
    pub area_center_mm: [f64; 3],
    pub bounds_mm: [[f64; 3]; 2],
    pub volume_mm3: f64,
    pub volume_center_mm: Option<[f64; 3]>,
    pub mass_kg: Option<f64>,
    pub mass_center_mm: Option<[f64; 3]>,
    /// About the volume centroid, in the definition coordinate system.
    pub inertia_kg_m2: Option<[[f64; 3]; 3]>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Properties {
    pub version: u32,
    pub representation: String,
    pub definition: Id,
    pub density_kg_m3: Option<f64>,
    pub total: Metrics,
    pub bodies: BTreeMap<Id, Metrics>,
}

fn array(v: Vector3<f64>) -> [f64; 3] {
    [v.x, v.y, v.z]
}
fn matrix(m: Matrix3<f64>) -> [[f64; 3]; 3] {
    std::array::from_fn(|r| std::array::from_fn(|c| m[(r, c)]))
}
fn invalid(message: &str) -> CadError {
    CadError::Invalid(message.into())
}
fn validate_metrics(value: &Metrics) -> Result<()> {
    let finite = value
        .bounds_mm
        .iter()
        .flatten()
        .chain(value.area_center_mm.iter())
        .chain(value.volume_center_mm.iter().flatten())
        .chain(value.mass_center_mm.iter().flatten())
        .chain(value.inertia_kg_m2.iter().flatten().flatten())
        .all(|v| v.is_finite());
    if !finite
        || (0..3).any(|axis| value.bounds_mm[0][axis] > value.bounds_mm[1][axis])
        || !value.area_mm2.is_finite()
        || value.area_mm2 <= 0.
        || !value.volume_mm3.is_finite()
        || value.volume_mm3 < 0.
        || value
            .mass_kg
            .is_some_and(|mass| !mass.is_finite() || mass <= 0.)
    {
        return Err(invalid(
            "Engineering property integration or aggregation exceeded finite numeric bounds",
        ));
    }
    Ok(())
}

#[derive(Default)]
struct Sum {
    value: f64,
    correction: f64,
}
impl Sum {
    fn add(&mut self, value: f64) {
        let y = value - self.correction;
        let total = self.value + y;
        self.correction = (total - self.value) - y;
        self.value = total;
    }
}
struct Integral {
    area: Sum,
    area_moment: [Sum; 3],
    volume: Sum,
    moment: [Sum; 3],
    products: [[Sum; 3]; 3],
    bounds: [[f64; 3]; 2],
}
impl Default for Integral {
    fn default() -> Self {
        Self {
            area: Sum::default(),
            area_moment: std::array::from_fn(|_| Sum::default()),
            volume: Sum::default(),
            moment: std::array::from_fn(|_| Sum::default()),
            products: std::array::from_fn(|_| std::array::from_fn(|_| Sum::default())),
            bounds: [[f64::INFINITY; 3], [f64::NEG_INFINITY; 3]],
        }
    }
}
impl Integral {
    fn triangle(&mut self, points: [Vector3<f64>; 3], origin: Vector3<f64>, solid: bool) {
        let area = (points[1] - points[0])
            .cross(&(points[2] - points[0]))
            .norm()
            / 2.;
        self.area.add(area);
        let center = (points[0] + points[1] + points[2]) / 3.;
        for axis in 0..3 {
            self.area_moment[axis].add(area * (center[axis] - origin[axis]));
            for point in points {
                self.bounds[0][axis] = self.bounds[0][axis].min(point[axis]);
                self.bounds[1][axis] = self.bounds[1][axis].max(point[axis]);
            }
        }
        if !solid {
            return;
        }
        let p = points.map(|p| p - origin);
        let volume = p[0].dot(&p[1].cross(&p[2])) / 6.;
        let sum = p[0] + p[1] + p[2];
        let products = (p[0] * p[0].transpose()
            + p[1] * p[1].transpose()
            + p[2] * p[2].transpose()
            + sum * sum.transpose())
            * (volume / 20.);
        self.volume.add(volume);
        for axis in 0..3 {
            self.moment[axis].add(volume * sum[axis] / 4.);
            for other in 0..3 {
                self.products[axis][other].add(products[(axis, other)]);
            }
        }
    }
    fn metrics(&self, origin: Vector3<f64>, density: Option<f64>) -> Result<Metrics> {
        let area = self.area.value;
        if !area.is_finite() || area <= 0. {
            return Err(invalid(
                "Engineering properties require a finite, nonempty boundary",
            ));
        }
        let volume = self.volume.value.abs();
        let sign = self.volume.value.signum();
        let centroid =
            (volume > 1e-18).then(|| Vector3::from_fn(|r, _| self.moment[r].value * sign / volume));
        let inertia = centroid.zip(density).map(|(c, rho)| {
            let products = Matrix3::from_fn(|r, col| self.products[r][col].value * sign)
                - c * c.transpose() * volume;
            matrix((Matrix3::identity() * products.trace() - products) * rho * 1e-15)
        });
        let metrics = Metrics {
            area_mm2: area,
            area_center_mm: array(
                origin + Vector3::from_fn(|r, _| self.area_moment[r].value / area),
            ),
            bounds_mm: self.bounds,
            volume_mm3: volume,
            volume_center_mm: centroid.map(|c| array(origin + c)),
            mass_kg: centroid.and(density).map(|rho| volume * rho * 1e-9),
            mass_center_mm: centroid.zip(density).map(|(c, _)| array(origin + c)),
            inertia_kg_m2: inertia,
        };
        if metrics
            .bounds_mm
            .iter()
            .flatten()
            .chain(metrics.area_center_mm.iter())
            .any(|v| !v.is_finite())
            || !volume.is_finite()
            || metrics
                .volume_center_mm
                .iter()
                .flatten()
                .any(|v| !v.is_finite())
            || metrics
                .inertia_kg_m2
                .iter()
                .flatten()
                .flatten()
                .any(|v| !v.is_finite())
        {
            return Err(invalid(
                "Engineering property integration produced a non-finite value",
            ));
        }
        validate_metrics(&metrics)?;
        Ok(metrics)
    }
}

pub fn evaluate(
    doc: &Document,
    meshes: &BTreeMap<Id, MeshData>,
) -> Result<BTreeMap<Id, Properties>> {
    let mut result = BTreeMap::new();
    for (key, mesh) in meshes {
        if mesh.faces.is_empty() {
            continue;
        }
        let definition = mesh
            .faces
            .iter()
            .find_map(|f| f.body.as_ref().map(|b| b.definition.as_str()))
            .unwrap_or(key);
        let configuration = key.strip_prefix(&format!("{definition}@")).or_else(|| {
            doc.active_configurations
                .get(definition)
                .map(String::as_str)
        });
        let part = crate::configuration::effective_part(doc, definition, configuration)?;
        let density = doc.materials.get(&part.material).map(|m| m.density_kg_m3);
        let origin = Vector3::from_fn(|r, _| (mesh.bounds[0][r] + mesh.bounds[1][r]) / 2.);
        let mut bodies: BTreeMap<Id, Integral> = BTreeMap::new();
        for face in &mesh.faces {
            let owner = face.body.as_ref().ok_or_else(|| {
                invalid("Rebuild geometry to resolve engineering property body ownership")
            })?;
            let solid = face.body_kind == crate::protocol::BodyKind::Solid;
            let end = face
                .start
                .checked_add(face.count)
                .ok_or_else(|| invalid("Invalid boundary index range"))?;
            let indices = mesh
                .indices
                .get(face.start..end)
                .ok_or_else(|| invalid("Invalid boundary index range"))?;
            if indices.len() % 3 != 0 {
                return Err(invalid(
                    "Engineering properties require triangular boundaries",
                ));
            }
            let integral = bodies.entry(owner.node.clone()).or_default();
            for triangle in indices.as_chunks::<3>().0 {
                let [a, b, c] = triangle.map(|i| {
                    mesh.positions
                        .get(i as usize)
                        .copied()
                        .map(Vector3::from)
                        .ok_or_else(|| invalid("Invalid boundary vertex index"))
                });
                let points = [a?, b?, c?];
                if points.iter().flat_map(|p| p.iter()).any(|v| !v.is_finite()) {
                    return Err(invalid(
                        "Engineering properties require finite boundary coordinates",
                    ));
                }
                integral.triangle(points, origin, solid);
            }
        }
        let bodies: BTreeMap<_, _> = bodies
            .into_iter()
            .map(|(id, value)| {
                let body_density = part
                    .features
                    .iter()
                    .find(|f| f.id == id)
                    .and_then(|f| match &f.operation {
                        crate::Operation::WeldBead { definition } => definition.material.as_ref(),
                        _ => None,
                    })
                    .and_then(|id| doc.materials.get(id))
                    .map(|m| m.density_kg_m3)
                    .or(density);
                Ok((id, value.metrics(origin, body_density)?))
            })
            .collect::<Result<_>>()?;
        let total = combine(&bodies.values().cloned().collect::<Vec<_>>())?;
        result.insert(
            key.clone(),
            Properties {
                version: CONTRACT_VERSION,
                representation: "evaluated_boundary_tessellation".into(),
                definition: definition.into(),
                density_kg_m3: if part.features.iter().any(|f| matches!(&f.operation, crate::Operation::WeldBead { definition } if definition.material.as_ref().is_some_and(|id| id != &part.material))) { None } else { density },
                total,
                bodies,
            },
        );
    }
    Ok(result)
}

/// Transform a cached result into assembly coordinates without re-integrating its boundary.
pub fn placed(value: &Metrics, placement: [[f64; 4]; 4]) -> Result<Metrics> {
    validate_metrics(value)?;
    report::validate_rigid(placement)?;
    let rotation = Matrix3::from_fn(|r, c| placement[r][c]);
    let translation = Vector3::from_fn(|r, _| placement[r][3]);
    let point = |p: [f64; 3]| array(rotation * Vector3::from(p) + translation);
    let mut result = value.clone();
    result.area_center_mm = point(value.area_center_mm);
    result.volume_center_mm = value.volume_center_mm.map(point);
    result.mass_center_mm = value.mass_center_mm.map(point);
    result.inertia_kg_m2 = value
        .inertia_kg_m2
        .map(|m| matrix(rotation * Matrix3::from_fn(|r, c| m[r][c]) * rotation.transpose()));
    result.bounds_mm = [[f64::INFINITY; 3], [f64::NEG_INFINITY; 3]];
    for mask in 0..8 {
        let corner = point(std::array::from_fn(|axis| {
            value.bounds_mm[(mask >> axis) & 1][axis]
        }));
        for (axis, v) in corner.into_iter().enumerate() {
            result.bounds_mm[0][axis] = result.bounds_mm[0][axis].min(v);
            result.bounds_mm[1][axis] = result.bounds_mm[1][axis].max(v);
        }
    }
    validate_metrics(&result)?;
    Ok(result)
}

/// Combine distinct bodies or placed components with the parallel-axis theorem.
pub fn combine(values: &[Metrics]) -> Result<Metrics> {
    if values.is_empty() {
        return Err(invalid(
            "Select a body or component with evaluated geometry",
        ));
    }
    for value in values {
        validate_metrics(value)?;
    }
    if values.iter().any(|v| {
        !v.area_mm2.is_finite()
            || v.area_mm2 <= 0.
            || !v.volume_mm3.is_finite()
            || v.volume_mm3 < 0.
            || v.mass_kg
                .is_some_and(|mass| !mass.is_finite() || mass <= 0.)
            || v.mass_kg.is_some() && (v.mass_center_mm.is_none() || v.inertia_kg_m2.is_none())
            || v.volume_mm3 > 1e-18 && v.volume_center_mm.is_none()
    }) {
        return Err(invalid("Cannot combine incomplete engineering properties"));
    }
    let area: f64 = values.iter().map(|v| v.area_mm2).sum();
    let volume: f64 = values.iter().map(|v| v.volume_mm3).sum();
    let area_center = values.iter().fold(Vector3::zeros(), |sum, v| {
        sum + Vector3::from(v.area_center_mm) * v.area_mm2
    }) / area;
    let volume_center = (volume > 1e-18).then(|| {
        values
            .iter()
            .filter_map(|v| v.volume_center_mm.map(|c| (c, v.volume_mm3)))
            .fold(Vector3::zeros(), |sum, (c, w)| sum + Vector3::from(c) * w)
            / volume
    });
    let solids: Vec<_> = values.iter().filter(|v| v.volume_mm3 > 1e-18).collect();
    let mass = (!solids.is_empty())
        .then(|| solids.iter().map(|v| v.mass_kg).collect::<Option<Vec<_>>>())
        .flatten()
        .map(|m| m.iter().sum::<f64>());
    let mass_center = mass.filter(|m| *m > 0.).map(|m| {
        solids.iter().fold(Vector3::zeros(), |sum, v| {
            sum + Vector3::from(v.mass_center_mm.unwrap()) * v.mass_kg.unwrap()
        }) / m
    });
    let inertia = mass_center.map(|center| {
        let tensor = solids.iter().fold(Matrix3::zeros(), |sum, v| {
            let offset = (Vector3::from(v.mass_center_mm.unwrap()) - center) * 0.001;
            let own = Matrix3::from_fn(|r, c| v.inertia_kg_m2.unwrap()[r][c]);
            sum + own
                + (Matrix3::identity() * offset.norm_squared() - offset * offset.transpose())
                    * v.mass_kg.unwrap()
        });
        matrix(tensor)
    });
    let bounds = std::array::from_fn(|side| {
        std::array::from_fn(|axis| {
            values.iter().map(|v| v.bounds_mm[side][axis]).fold(
                if side == 0 {
                    f64::INFINITY
                } else {
                    f64::NEG_INFINITY
                },
                if side == 0 { f64::min } else { f64::max },
            )
        })
    });
    let result = Metrics {
        area_mm2: area,
        area_center_mm: array(area_center),
        bounds_mm: bounds,
        volume_mm3: volume,
        volume_center_mm: volume_center.map(array),
        mass_kg: mass,
        mass_center_mm: mass_center.map(array),
        inertia_kg_m2: inertia,
    };
    validate_metrics(&result)?;
    Ok(result)
}

/// Engineering properties of independently owned assembly geometry.
/// Integration uses the evaluated boundary, with the same numeric limits as parts.
pub fn boundary_metrics(mesh: &MeshData, density: Option<f64>) -> Result<Metrics> {
    let origin = Vector3::from_fn(|r, _| (mesh.bounds[0][r] + mesh.bounds[1][r]) / 2.);
    let mut integral = Integral::default();
    for face in &mesh.faces {
        let end = face
            .start
            .checked_add(face.count)
            .ok_or_else(|| invalid("Invalid boundary index range"))?;
        let indices = mesh
            .indices
            .get(face.start..end)
            .ok_or_else(|| invalid("Invalid boundary index range"))?;
        if indices.len() % 3 != 0 {
            return Err(invalid(
                "Engineering properties require triangular boundaries",
            ));
        }
        for triangle in indices.as_chunks::<3>().0 {
            let [a, b, c] = triangle.map(|i| {
                mesh.positions
                    .get(i as usize)
                    .copied()
                    .map(Vector3::from)
                    .ok_or_else(|| invalid("Invalid boundary vertex index"))
            });
            let points = [a?, b?, c?];
            if points.iter().flat_map(|p| p.iter()).any(|v| !v.is_finite()) {
                return Err(invalid(
                    "Engineering properties require finite boundary coordinates",
                ));
            }
            integral.triangle(
                points,
                origin,
                face.body_kind == crate::protocol::BodyKind::Solid,
            );
        }
    }
    integral.metrics(origin, density)
}
