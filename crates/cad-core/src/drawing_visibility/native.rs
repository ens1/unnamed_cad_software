//! Native parametric visibility. Every geometric exclusion and covering proof
//! uses outward-rounded carrier intervals. Display tessellation is not read.
//! Curves retain their original rational or on-surface definition. A native
//! silhouette is the implicit zero set (S_u × S_v)·view = 0, with bounded
//! parameter cells retained alongside its drawing representation.
mod evaluate;
mod interval;
mod silhouette;
mod silhouette_analytic;
mod trim;
use crate::{
    CadError, Result, TopologyRef,
    drawing_geometry::{self, EdgeGeometry, FaceGeometry},
    protocol::MeshData,
    sketch_edit::Curve3,
    tolerance::Accuracy,
};
use interval::{I, J};
use serde::{Deserialize, Serialize};
use trim::Region;
type Domain = [[f64; 2]; 2];
type Matrix = [[f64; 4]; 4];
const MAX_WORK: usize = 16_000_000;
const MAX_PIECES: usize = 500_000;
const MAX_SCENE_UNITS: usize = 4_000_000;
const MAX_TRACE_UNITS: usize = 8_000_000;
fn unsupported(s: &str) -> CadError {
    CadError::Unsupported(format!(
        "Native drawing visibility: {s}. Use Wireframe or explicitly select Display mesh visibility for this view"
    ))
}
struct Budget {
    work: usize,
    output_items: usize,
    storage_units: usize,
}
impl Budget {
    fn store(&mut self, n: usize) -> Result<()> {
        self.storage_units = self.storage_units.saturating_add(n);
        if self.storage_units > MAX_TRACE_UNITS {
            Err(unsupported(
                "native trace metadata exceeded its bounded storage budget",
            ))
        } else {
            Ok(())
        }
    }
    fn output(&mut self, n: usize) -> Result<()> {
        self.store(n.saturating_mul(8))?;
        self.output_items = self.output_items.saturating_add(n);
        if self.output_items > MAX_PIECES {
            Err(unsupported("too many native drawing output points"))
        } else {
            Ok(())
        }
    }
    fn charge(&mut self, n: usize) -> Result<()> {
        self.work = self.work.saturating_add(n);
        if self.work > MAX_WORK {
            Err(unsupported(
                "the bounded native geometry budget was exhausted",
            ))
        } else {
            Ok(())
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TraceSource {
    /// The zero set of the original carrier's view-normal equation.
    Silhouette,
    Spatial {
        curve: drawing_geometry::Curve3,
        range: [f64; 2],
    },
    /// An exact parameter-space boundary on the original native carrier.
    Trim {
        curve: drawing_geometry::Curve2,
        range: [f64; 2],
    },
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NativeTrace {
    pub face: Option<TopologyRef>,
    pub edge: Option<TopologyRef>,
    pub carrier: Option<FaceGeometry>,
    pub placement: Matrix,
    pub view: String,
    pub source: TraceSource,
    pub cells: Vec<Domain>,
    pub points: Vec<[f64; 3]>,
    pub max_error_mm: f64,
    pub endpoint_error_mm: f64,
    pub visibility_event: bool,
    /// True only when the complete original edge, before any adaptive split,
    /// has a projected enclosure below one eighth of the modeling tolerance.
    pub collapsed_projection: bool,
    pub ghost: bool,
    pub hidden: bool,
}
impl NativeTrace {
    /// Conservative eight-byte scalar slots, including vector overhead and
    /// string bytes (charged as whole slots). This is safe to sum per sheet.
    pub fn storage_units(&self) -> usize {
        let source = match &self.source {
            TraceSource::Silhouette => 8,
            TraceSource::Trim { curve, .. } => curve2_units(curve) + 8,
            TraceSource::Spatial { curve, .. } => curve3_units(curve) + 8,
        };
        64 + self.view.len()
            + self.face.as_ref().map_or(0, reference_units)
            + self.edge.as_ref().map_or(0, reference_units)
            + self.carrier.as_ref().map_or(0, silhouette::payload_size)
            + self.points.len().saturating_mul(3)
            + self.cells.len().saturating_mul(4)
            + source
    }
}
fn reference_units(r: &TopologyRef) -> usize {
    16 + r.role.len() + r.topology.len() + r.producer.definition.len() + r.producer.node.len()
}
fn curve2_units(c: &drawing_geometry::Curve2) -> usize {
    match c {
        drawing_geometry::Curve2::Rational {
            knots,
            control_points,
            ..
        } => 16 + knots.len() + control_points.len().saturating_mul(3),
        _ => 32,
    }
}
fn curve3_units(c: &drawing_geometry::Curve3) -> usize {
    match c {
        drawing_geometry::Curve3::Rational {
            knots,
            control_points,
            ..
        } => 16 + knots.len() + control_points.len().saturating_mul(4),
        _ => 16,
    }
}
fn surface_work(face: &FaceGeometry) -> usize {
    match &face.carrier {
        drawing_geometry::Carrier::Rational { surface } => {
            surface
                .control_points
                .iter()
                .map(|r| {
                    r.len().saturating_mul(4) + surface.degree[1].saturating_mul(surface.degree[1])
                })
                .sum::<usize>()
                + surface.degree[0].saturating_mul(surface.degree[0])
                + surface.knots.iter().map(Vec::len).sum::<usize>()
        }
        drawing_geometry::Carrier::Revolution { profile, .. } => curve3_units(profile) + 64,
        drawing_geometry::Carrier::Plane { .. } => 16,
        _ => 96,
    }
}
#[derive(Clone)]
struct Face {
    reference: TopologyRef,
    geometry: FaceGeometry,
    matrix: Matrix,
    ghost: bool,
    seeds: Vec<Domain>,
}
#[derive(Clone)]
struct Edge {
    on_surface: Option<EdgeGeometry>,
    spatial: Option<drawing_geometry::Curve3>,
    matrix: Matrix,
    ghost: bool,
    reference: TopologyRef,
}
#[derive(Default)]
pub(crate) struct Scene {
    faces: Vec<Face>,
    edges: Vec<Edge>,
    storage_units: usize,
}
impl Scene {
    pub(crate) fn add_mesh(
        &mut self,
        mesh: &MeshData,
        matrix: Matrix,
        ghost: bool,
        tolerance: f64,
    ) -> Result<()> {
        if matrix.iter().flatten().any(|x| !x.is_finite()) || matrix[3] != [0., 0., 0., 1.] {
            return Err(unsupported(
                "native drawing placement is not a finite affine transform",
            ));
        }
        let base = self.faces.len();
        if self.faces.len().saturating_add(mesh.faces.len()) > 50_000
            || self.edges.len().saturating_add(mesh.edges.len()) > 100_000
        {
            return Err(unsupported(
                "native drawing scene has too many faces or edges",
            ));
        }
        let added = mesh
            .faces
            .iter()
            .map(|f| {
                f.native_geometry
                    .as_ref()
                    .map_or(0, silhouette::payload_size)
                    + reference_units(&f.reference)
                    + 64
            })
            .chain(mesh.edges.iter().map(|e| {
                e.native_geometry
                    .as_ref()
                    .map_or(0, |g| curve2_units(&g.curve))
                    + e.native_spatial_geometry.as_ref().map_or(0, curve3_units)
                    + reference_units(&e.reference)
                    + 64
            }))
            .fold(0usize, usize::saturating_add);
        self.storage_units = self.storage_units.saturating_add(added);
        if self.storage_units > MAX_SCENE_UNITS {
            return Err(unsupported(
                "native drawing scene exceeded its bounded storage budget",
            ));
        }
        for face in &mesh.faces {
            let geometry = face.native_geometry.clone().ok_or_else(|| {
                unsupported(
                    face.native_geometry_error
                        .as_deref()
                        .unwrap_or("this face has no native drawing geometry"),
                )
            })?;
            if !geometry.uv_closed
                || !geometry.uv_chart_error_mm.is_finite()
                || geometry.uv_chart_error_mm < 0.
                || geometry.uv_chart_error_mm > tolerance / 16.
            {
                return Err(unsupported(
                    "native face trim chart closure or seam error is not certified at this tolerance",
                ));
            }
            if !silhouette_analytic::continuous_normals(&geometry)? {
                return Err(unsupported(
                    "this native carrier has an unresolved tangent-discontinuous internal knot line",
                ));
            }
            let seeds = evaluate::domains(
                &geometry,
                MAX_SCENE_UNITS.saturating_sub(self.storage_units) / 4,
            )?;
            self.storage_units = self
                .storage_units
                .saturating_add(seeds.len().saturating_mul(4));
            self.faces.push(Face {
                reference: face.reference.clone(),
                geometry,
                matrix,
                ghost,
                seeds,
            });
        }
        for edge in &mesh.edges {
            let g = edge.native_geometry.as_ref().map(|g| {
                let mut g = g.clone();
                g.face += base;
                g
            });
            self.edges.push(Edge {
                on_surface: g,
                spatial: edge.native_spatial_geometry.clone(),
                matrix,
                ghost,
                reference: edge.reference.clone(),
            });
        }
        Ok(())
    }
    pub(crate) fn project(&self, view: &str, accuracy: Accuracy) -> Result<ProjectedScene> {
        accuracy.modeling.validate()?;
        let basis: [[f64; 3]; 3] = std::array::from_fn(|i| {
            let mut p = [0.; 3];
            p[i] = 1.;
            super::projected(view, p).unwrap_or([f64::NAN; 3])
        });
        let matrix: [[f64; 3]; 3] = std::array::from_fn(|i| std::array::from_fn(|j| basis[j][i]));
        if matrix.iter().flatten().any(|x| !x.is_finite()) {
            return Err(unsupported("unknown native drawing orientation"));
        }
        Ok(ProjectedScene {
            faces: self.faces.clone(),
            edges: self.edges.clone(),
            view: view.into(),
            basis: matrix,
            accuracy,
            budget: Budget {
                work: 0,
                output_items: 0,
                storage_units: 0,
            },
        })
    }
}
pub(crate) struct ProjectedScene {
    faces: Vec<Face>,
    edges: Vec<Edge>,
    view: String,
    basis: [[f64; 3]; 3],
    accuracy: Accuracy,
    budget: Budget,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Visibility {
    Visible,
    Hidden,
    Uncertain,
}
#[derive(Clone, Copy)]
struct Query {
    bounds: [I; 3],
    center: [I; 3],
    derivative: [I; 3],
    radius: f64,
    self_contact: Option<(usize, Domain)>,
}
impl ProjectedScene {
    pub(crate) fn fidelity(&self) -> super::Fidelity {
        super::Fidelity::NativeSurfaces {
            tolerance_mm: self.accuracy.modeling.linear_mm,
            curve_output_tolerance_mm: self
                .accuracy
                .display_mm
                .max(self.accuracy.modeling.linear_mm),
        }
    }
    fn project_intervals(&self, p: [I; 3]) -> [I; 3] {
        std::array::from_fn(|i| {
            p[0] * self.basis[i][0].into()
                + p[1] * self.basis[i][1].into()
                + p[2] * self.basis[i][2].into()
        })
    }
    fn projected_diameter(&self, p: [I; 3]) -> f64 {
        let q = self.project_intervals(p);
        (I::point(q[0].width()).square() + I::point(q[1].width()).square())
            .sqrt()
            .hi
    }
    fn project_jet(&self, p: [J; 3]) -> [J; 3] {
        std::array::from_fn(|i| {
            p[0] * self.basis[i][0].into()
                + p[1] * self.basis[i][1].into()
                + p[2] * self.basis[i][2].into()
        })
    }
    fn injective_source_cell(
        &self,
        index: usize,
        source: Domain,
    ) -> Result<Option<(usize, Domain)>> {
        let face = &self.faces[index];
        let source: Domain = std::array::from_fn(|i| {
            [
                source[i][0].max(face.geometry.domain[i][0]),
                source[i][1].min(face.geometry.domain[i][1]),
            ]
        });
        for seed in &face.seeds {
            if !(0..2).all(|i| source[i][0] >= seed[i][0] && source[i][1] <= seed[i][1]) {
                continue;
            }
            for fraction in [1., 0.25, 0.0625, 0.015625] {
                let cell = std::array::from_fn(|i| {
                    let padding = (seed[i][1] - seed[i][0]) * fraction;
                    [
                        (source[i][0] - padding).max(seed[i][0]),
                        (source[i][1] + padding).min(seed[i][1]),
                    ]
                });
                let p = self.project_jet(evaluate::surface(&face.geometry, cell, face.matrix)?);
                let j: [[f64; 2]; 2] =
                    std::array::from_fn(|i| std::array::from_fn(|k| p[i].d[k].mid()));
                let determinant = j[0][0] * j[1][1] - j[0][1] * j[1][0];
                if !determinant.is_finite() || determinant == 0. {
                    continue;
                }
                let inverse = [
                    [j[1][1] / determinant, -j[0][1] / determinant],
                    [-j[1][0] / determinant, j[0][0] / determinant],
                ];
                let residual: [[I; 2]; 2] = std::array::from_fn(|i| {
                    std::array::from_fn(|c| {
                        I::point(f64::from(i == c))
                            - I::point(inverse[i][0]) * p[0].d[c]
                            - I::point(inverse[i][1]) * p[1].d[c]
                    })
                });
                // The mean-value map of the preconditioned projection differs
                // from identity by a strict contraction on this convex cell.
                // It is globally injective here, including boundary points.
                if residual
                    .iter()
                    .all(|r| (I::point(r[0].absmax()) + r[1].absmax().into()).hi < 1.)
                {
                    return Ok(Some((index, cell)));
                }
            }
        }
        Ok(None)
    }
    fn visibility(&mut self, world: Query) -> Result<Visibility> {
        let query = Query {
            bounds: self.project_intervals(world.bounds),
            center: self.project_intervals(world.center),
            derivative: self.project_intervals(world.derivative),
            radius: world.radius,
            self_contact: world.self_contact,
        };
        let mut uncertain = false;
        for face_index in 0..self.faces.len() {
            if self.faces[face_index].ghost {
                continue;
            }
            let mut seeds = self.faces[face_index].seeds.clone();
            if let Some((owner, excluded)) = query.self_contact
                && owner == face_index
            {
                seeds = seeds
                    .into_iter()
                    .flat_map(|d| partition(d, excluded))
                    .collect();
            }
            for domain in seeds {
                match self.cell_visibility(face_index, domain, query, 0)? {
                    Visibility::Hidden => return Ok(Visibility::Hidden),
                    Visibility::Uncertain => uncertain = true,
                    Visibility::Visible => (),
                }
            }
        }
        Ok(if uncertain {
            Visibility::Uncertain
        } else {
            Visibility::Visible
        })
    }
    fn cell_visibility(
        &mut self,
        face_index: usize,
        domain: Domain,
        query: Query,
        depth: usize,
    ) -> Result<Visibility> {
        self.budget
            .charge(surface_work(&self.faces[face_index].geometry).saturating_mul(6))?;
        if let Some((owner, excluded)) = query.self_contact
            && owner == face_index
            && (0..2).all(|i| domain[i][0] >= excluded[i][0] && domain[i][1] <= excluded[i][1])
        {
            return Ok(Visibility::Visible);
        }
        let face = &self.faces[face_index];
        let p = self.project_jet(evaluate::surface(&face.geometry, domain, face.matrix)?);
        let tolerance = self.accuracy.modeling.linear_mm;
        if !(0..2).all(|i| p[i].x.overlap(query.bounds[i]))
            || p[2].x.hi <= (I::point(query.bounds[2].lo) + tolerance.into()).lo
        {
            return Ok(Visibility::Visible);
        }
        if depth >= 5 && trim::region(&face.geometry, domain, &mut self.budget)? == Region::Outside
        {
            return Ok(Visibility::Visible);
        }
        // A Krawczyk inclusion proves that every projected query point has an
        // actual carrier preimage in this parameter cell. The entire included
        // domain must be inside the native trim and in front of the curve.
        let midpoint = domain.map(|d| d[0] * 0.5 + d[1] * 0.5);
        let at = self.project_jet(evaluate::surface(
            &face.geometry,
            midpoint.map(|v| [v, v]),
            face.matrix,
        )?);
        let j: [[f64; 2]; 2] = std::array::from_fn(|i| std::array::from_fn(|k| p[i].d[k].mid()));
        let det = j[0][0] * j[1][1] - j[0][1] * j[1][0];
        if det.is_finite() && det != 0. {
            let inverse = [
                [j[1][1] / det, -j[0][1] / det],
                [-j[1][0] / det, j[0][0] / det],
            ];
            let k: [I; 2] = std::array::from_fn(|i| {
                let mut v = I::point(midpoint[i]);
                for r in 0..2 {
                    v = v - I::point(inverse[i][r]) * (at[r].x - query.bounds[r]);
                }
                for c in 0..2 {
                    let mut entry = I::point(f64::from(i == c));
                    for r in 0..2 {
                        entry = entry - I::point(inverse[i][r]) * p[r].d[c];
                    }
                    v = v + entry * (I::new(domain[c][0], domain[c][1]) - midpoint[c].into());
                }
                v
            });
            if (0..2).all(|i| k[i].lo > domain[i][0] && k[i].hi < domain[i][1]) {
                let inside = k.map(|v| [v.lo, v.hi]);
                let residual: [[I; 2]; 2] = std::array::from_fn(|i| {
                    std::array::from_fn(|c| {
                        let mut value = I::point(f64::from(i == c));
                        for r in 0..2 {
                            value = value - I::point(inverse[i][r]) * p[r].d[c];
                        }
                        value
                    })
                });
                if residual
                    .iter()
                    .all(|r| (r[0].absmax() + r[1].absmax()).next_up() < 1.)
                {
                    // The contraction bounds also establish uniqueness over
                    // the complete cell. Preserve the curve/ray correlation:
                    // integrate the derivative of surface depth minus source
                    // depth, instead of subtracting independent XYZ boxes.
                    let center_k: [I; 2] = std::array::from_fn(|i| {
                        let mut value = I::point(midpoint[i]);
                        for r in 0..2 {
                            value = value - I::point(inverse[i][r]) * (at[r].x - query.center[r]);
                        }
                        for (c, row) in domain.iter().enumerate() {
                            value = value
                                + residual[i][c] * (I::new(row[0], row[1]) - midpoint[c].into());
                        }
                        value
                    });
                    if (0..2)
                        .all(|i| center_k[i].lo >= domain[i][0] && center_k[i].hi <= domain[i][1])
                    {
                        let center_p = self.project_jet(evaluate::surface(
                            &face.geometry,
                            center_k.map(|v| [v.lo, v.hi]),
                            face.matrix,
                        )?);
                        let derivative_det = p[0].d[0] * p[1].d[1] - p[0].d[1] * p[1].d[0];
                        if !derivative_det.contains_zero() {
                            let du = (p[1].d[1] * query.derivative[0]
                                - p[0].d[1] * query.derivative[1])
                                / derivative_det;
                            let dv = (p[0].d[0] * query.derivative[1]
                                - p[1].d[0] * query.derivative[0])
                                / derivative_det;
                            let delta_derivative =
                                p[2].d[0] * du + p[2].d[1] * dv - query.derivative[2];
                            let delta = center_p[2].x - query.center[2]
                                + delta_derivative * I::new(-query.radius, query.radius);
                            if delta.hi <= tolerance {
                                return Ok(Visibility::Visible);
                            }
                        }
                    }
                }
                if trim::region(&face.geometry, inside, &mut self.budget)? == Region::Inside {
                    let front =
                        self.project_jet(evaluate::surface(&face.geometry, inside, face.matrix)?);
                    if front[2].x.lo > (I::point(query.bounds[2].hi) + tolerance.into()).hi {
                        return Ok(Visibility::Hidden);
                    }
                }
            }
        }
        if depth >= 24 {
            return Ok(Visibility::Uncertain);
        }
        let sizes = std::array::from_fn::<_, 2, _>(|i| {
            p.iter().map(|p| p.d[i].absmax()).fold(0., f64::max) * (domain[i][1] - domain[i][0])
        });
        let axis = usize::from(sizes[1] > sizes[0]);
        let [a, b] = split_domain(domain, axis)?;
        let first = self.cell_visibility(face_index, a, query, depth + 1)?;
        if first == Visibility::Hidden {
            return Ok(first);
        }
        let second = self.cell_visibility(face_index, b, query, depth + 1)?;
        Ok(if second == Visibility::Hidden {
            second
        } else if first == Visibility::Visible && second == Visibility::Visible {
            Visibility::Visible
        } else {
            Visibility::Uncertain
        })
    }
    pub(crate) fn split(
        &mut self,
        _curve: &Curve3,
        _source_faces: &[usize],
    ) -> Result<Vec<super::Piece>> {
        Err(unsupported(
            "native visibility requires the original homogeneous or on-surface source curve; a converted display curve is not authoritative",
        ))
    }
    pub(crate) fn split_native(&mut self, index: usize) -> Result<Vec<NativeTrace>> {
        let boundary_hidden = silhouette_analytic::boundary_occludes(self, index)?;
        let source = self
            .edges
            .get(index)
            .cloned()
            .ok_or_else(|| unsupported("native edge index is missing"))?;
        let Some(edge) = source.on_surface.clone() else {
            return self.split_spatial(source, boundary_hidden);
        };
        if edge.version != drawing_geometry::VERSION || edge.face >= self.faces.len() {
            return Err(unsupported(
                "native edge geometry version or face index is invalid",
            ));
        }
        let face = self.faces[edge.face].clone();
        let output_tolerance = self
            .accuracy
            .display_mm
            .max(self.accuracy.modeling.linear_mm);
        let mut out: Vec<NativeTrace> = Vec::new();
        let domains = evaluate::curve_domains(&edge.curve)?
            .into_iter()
            .map(|span| [span[0].max(edge.range[0]), span[1].min(edge.range[1])])
            .filter(|span| span[0] < span[1])
            .collect::<Vec<_>>();
        let mut full: Option<[I; 3]> = None;
        for span in &domains {
            self.budget
                .charge(surface_work(&face.geometry) + curve2_units(&edge.curve))?;
            let uv = evaluate::curve2(&edge.curve, J::variable(I::new(span[0], span[1]), 0))?;
            let p = evaluate::surface(&face.geometry, uv.map(|p| [p.x.lo, p.x.hi]), face.matrix)?
                .map(|p| p.x);
            full = Some(full.map_or(p, |a| std::array::from_fn(|i| a[i].hull(p[i]))));
        }
        let collapsed = full.is_some_and(|p| {
            self.projected_diameter(p) <= self.accuracy.modeling.linear_mm * 0.125
        });
        for span in domains {
            let mut stack = vec![(span, 0)];
            while let Some((t, depth)) = stack.pop() {
                self.budget.charge(
                    surface_work(&face.geometry).saturating_mul(6)
                        + curve2_units(&edge.curve).saturating_mul(4),
                )?;
                let uv = evaluate::curve2(&edge.curve, J::variable(I::new(t[0], t[1]), 0))?;
                let cell = uv.map(|v| [v.x.lo, v.x.hi]);
                let p = evaluate::surface(&face.geometry, cell, face.matrix)?;
                let tangent: [I; 3] =
                    std::array::from_fn(|i| p[i].d[0] * uv[0].d[0] + p[i].d[1] * uv[1].d[0]);
                let error = (I::point(I::new(t[0], t[1]).width())
                    * I::point(diameter(tangent))
                    * I::point(0.5))
                .hi;
                let mid = t[0] * 0.5 + t[1] * 0.5;
                let cuv = evaluate::curve2(&edge.curve, J::constant(mid.into()))?;
                let center =
                    evaluate::surface(&face.geometry, cuv.map(|v| [v.x.lo, v.x.hi]), face.matrix)?
                        .map(|v| v.x);
                let self_contact = self.injective_source_cell(edge.face, cell)?;
                let status = if face.ghost || collapsed {
                    Visibility::Visible
                } else if boundary_hidden {
                    Visibility::Hidden
                } else {
                    self.visibility(Query {
                        bounds: p.map(|p| p.x),
                        center,
                        derivative: tangent,
                        radius: I::new(t[0], t[1]).radius(),
                        self_contact,
                    })?
                };
                let mut accepted = false;
                if error <= output_tolerance * 0.25
                    && (status != Visibility::Uncertain
                        || diameter(p.map(|p| p.x)) <= self.accuracy.modeling.linear_mm * 0.125)
                {
                    let enclosures = t
                        .into_iter()
                        .map(|t| {
                            let uv = evaluate::curve2(&edge.curve, J::constant(t.into()))?;
                            let q = evaluate::surface(
                                &face.geometry,
                                uv.map(|p| [p.x.lo, p.x.hi]),
                                face.matrix,
                            )?;
                            Ok(q.map(|v| v.x))
                        })
                        .collect::<Result<Vec<_>>>()?;
                    let endpoint_error = enclosures.iter().map(|p| diameter(*p)).fold(0., f64::max);
                    if (I::point(error) + endpoint_error.into()).hi <= output_tolerance * 0.5 {
                        self.budget.output(2)?;
                        let points = enclosures
                            .into_iter()
                            .map(|p| p.map(I::mid))
                            .collect::<Vec<_>>();
                        let hidden = status == Visibility::Hidden;
                        if let Some(last) = out.last_mut()
                            && last.hidden == hidden
                            && last.collapsed_projection == collapsed
                            && !last.visibility_event
                            && status != Visibility::Uncertain
                            && last.points.last() == points.first()
                            && let TraceSource::Trim { range, .. } = &mut last.source
                            && range[1] == t[0]
                        {
                            range[1] = t[1];
                            last.points.push(points[1]);
                            last.endpoint_error_mm = last.endpoint_error_mm.max(endpoint_error);
                            last.cells.push(cell);
                        } else {
                            self.budget.store(
                                silhouette::payload_size(&face.geometry)
                                    + curve2_units(&edge.curve)
                                    + 128,
                            )?;
                            out.push(NativeTrace {
                                face: Some(face.reference.clone()),
                                edge: Some(source.reference.clone()),
                                carrier: Some(face.geometry.clone()),
                                placement: face.matrix,
                                view: self.view.clone(),
                                source: TraceSource::Trim {
                                    curve: edge.curve.clone(),
                                    range: t,
                                },
                                cells: vec![cell],
                                points,
                                max_error_mm: output_tolerance,
                                endpoint_error_mm: endpoint_error,
                                visibility_event: status == Visibility::Uncertain,
                                collapsed_projection: collapsed,
                                ghost: face.ghost,
                                hidden,
                            });
                        }
                        accepted = true;
                    }
                }
                if !accepted {
                    if depth >= 48 {
                        return Err(unsupported(
                            "an exact on-surface edge could not be enclosed to the modeling tolerance",
                        ));
                    }
                    let mid = t[0] * 0.5 + t[1] * 0.5;
                    stack.push(([mid, t[1]], depth + 1));
                    stack.push(([t[0], mid], depth + 1));
                }
                if out.len() + stack.len() > MAX_PIECES {
                    return Err(unsupported("too many native on-surface edge pieces"));
                }
            }
        }
        self.resolve_events(&out)?;
        Ok(out)
    }
    fn split_spatial(&mut self, source: Edge, boundary_hidden: bool) -> Result<Vec<NativeTrace>> {
        let curve=source.spatial.ok_or_else(||unsupported("this edge has neither an exact on-surface curve nor an original homogeneous spatial carrier"))?;
        let (domains, cost) = match &curve {
            drawing_geometry::Curve3::Line { .. } => (vec![[0., 1.]], 8),
            drawing_geometry::Curve3::Rational {
                degree,
                knots,
                control_points,
            } => {
                if control_points.len() <= *degree
                    || knots.len() != control_points.len() + degree + 1
                    || *degree > 32
                {
                    return Err(unsupported("invalid native spatial spline"));
                }
                (
                    knots[*degree..=control_points.len()]
                        .windows(2)
                        .filter(|p| p[1] > p[0])
                        .map(|p| [p[0], p[1]])
                        .collect(),
                    control_points
                        .len()
                        .saturating_mul(degree.saturating_mul(*degree).max(1)),
                )
            }
        };
        let mut out: Vec<NativeTrace> = Vec::new();
        let tolerance = self.accuracy.modeling.linear_mm;
        let output_tolerance = self.accuracy.display_mm.max(tolerance);
        let mut full: Option<[I; 3]> = None;
        for span in &domains {
            self.budget.charge(cost)?;
            let p = evaluate::affine(
                evaluate::curve3(&curve, J::variable(I::new(span[0], span[1]), 0))?,
                source.matrix,
            )
            .map(|p| p.x);
            full = Some(full.map_or(p, |a| std::array::from_fn(|i| a[i].hull(p[i]))));
        }
        let collapsed = full.is_some_and(|p| self.projected_diameter(p) <= tolerance * 0.125);
        for span in domains {
            let mut stack = vec![(span, 0)];
            while let Some((t, depth)) = stack.pop() {
                self.budget.charge(cost)?;
                let p = evaluate::affine(
                    evaluate::curve3(&curve, J::variable(I::new(t[0], t[1]), 0))?,
                    source.matrix,
                );
                let center = evaluate::affine(
                    evaluate::curve3(&curve, J::constant(I::new(t[0], t[1]).mid().into()))?,
                    source.matrix,
                )
                .map(|p| p.x);
                let derivative = p.map(|p| p.d[0]);
                let error = (I::point(I::new(t[0], t[1]).width())
                    * diameter(derivative).into()
                    * 0.5.into())
                .hi;
                let status = if source.ghost || collapsed {
                    Visibility::Visible
                } else if boundary_hidden {
                    Visibility::Hidden
                } else {
                    self.visibility(Query {
                        bounds: p.map(|p| p.x),
                        center,
                        derivative,
                        radius: I::new(t[0], t[1]).radius(),
                        self_contact: None,
                    })?
                };
                let mut accepted = false;
                if (status != Visibility::Uncertain
                    || diameter(p.map(|p| p.x)) <= tolerance * 0.125)
                    && error <= output_tolerance * 0.25
                {
                    let endpoint = t
                        .into_iter()
                        .map(|t| {
                            Ok(evaluate::affine(
                                evaluate::curve3(&curve, J::constant(t.into()))?,
                                source.matrix,
                            )
                            .map(|p| p.x))
                        })
                        .collect::<Result<Vec<_>>>()?;
                    let endpoint_error = endpoint.iter().map(|p| diameter(*p)).fold(0., f64::max);
                    if (I::point(error) + endpoint_error.into()).hi <= output_tolerance * 0.5 {
                        self.budget.output(2)?;
                        let points = endpoint
                            .into_iter()
                            .map(|p| p.map(I::mid))
                            .collect::<Vec<_>>();
                        let hidden = status == Visibility::Hidden;
                        if let Some(last) = out.last_mut()
                            && last.hidden == hidden
                            && last.collapsed_projection == collapsed
                            && !last.visibility_event
                            && status != Visibility::Uncertain
                            && last.points.last() == points.first()
                            && let TraceSource::Spatial { range, .. } = &mut last.source
                            && range[1] == t[0]
                        {
                            range[1] = t[1];
                            last.points.push(points[1]);
                            last.endpoint_error_mm = last.endpoint_error_mm.max(endpoint_error);
                        } else {
                            self.budget.store(curve3_units(&curve) + 128)?;
                            out.push(NativeTrace {
                                face: None,
                                edge: Some(source.reference.clone()),
                                carrier: None,
                                placement: source.matrix,
                                view: self.view.clone(),
                                source: TraceSource::Spatial {
                                    curve: curve.clone(),
                                    range: t,
                                },
                                cells: Vec::new(),
                                points,
                                max_error_mm: output_tolerance,
                                endpoint_error_mm: endpoint_error,
                                visibility_event: status == Visibility::Uncertain,
                                collapsed_projection: collapsed,
                                ghost: source.ghost,
                                hidden,
                            });
                        }
                        accepted = true;
                    }
                }
                if !accepted {
                    if depth >= 48 {
                        return Err(unsupported(
                            "a native spatial curve could not be enclosed to the modeling tolerance",
                        ));
                    }
                    let mid = I::new(t[0], t[1]).mid();
                    stack.push(([mid, t[1]], depth + 1));
                    stack.push(([t[0], mid], depth + 1));
                }
            }
        }
        self.resolve_events(&out)?;
        Ok(out)
    }
    fn resolve_events(&mut self, traces: &[NativeTrace]) -> Result<()> {
        for trace in traces {
            if trace
                .points
                .iter()
                .flatten()
                .any(|p| !p.is_finite() || p.abs() * f64::EPSILON * 64. > trace.max_error_mm * 0.25)
            {
                return Err(unsupported(
                    "projected output coordinates exceed the certified curve-output arithmetic budget",
                ));
            }
        }
        let visible = traces
            .iter()
            .filter(|t| !t.visibility_event && !t.collapsed_projection)
            .flat_map(|t| {
                [t.points.first(), t.points.last()]
                    .into_iter()
                    .flatten()
                    .map(move |p| (*p, t.endpoint_error_mm, t.hidden))
            })
            .collect::<Vec<_>>();
        for trace in traces
            .iter()
            .filter(|t| t.visibility_event && !t.collapsed_projection)
        {
            let point = trace.points.first().ok_or_else(|| {
                unsupported("native visibility event has no certified curve endpoint")
            })?;
            let mut admitted = false;
            for (other, error, hidden) in &visible {
                if *hidden != trace.hidden {
                    continue;
                }
                self.budget.charge(1)?;
                let distance =
                    std::array::from_fn::<_, 3, _>(|i| I::point(point[i]) - other[i].into())
                        .into_iter()
                        .map(I::square)
                        .fold(I::point(0.), |a, b| a + b)
                        .sqrt();
                let bound = distance
                    + I::point(*error)
                    + trace.endpoint_error_mm.into()
                    + I::point(self.accuracy.modeling.linear_mm * 0.25);
                // A factor of two also encloses the norm of the stored
                // binary64 orthographic XY map, including its rounding.
                if (bound * I::point(2.)).hi <= self.accuracy.modeling.linear_mm {
                    admitted = true;
                    break;
                }
            }
            if !admitted {
                return Err(unsupported(
                    "an isolated native trim or visibility uncertainty has no certified neighboring curve within the modeling tolerance",
                ));
            }
        }
        Ok(())
    }
    pub(crate) fn silhouettes(&mut self) -> Result<Vec<NativeTrace>> {
        let traces = silhouette::build(self)?;
        self.resolve_events(&traces)?;
        Ok(traces)
    }
}
fn diameter(p: [I; 3]) -> f64 {
    p.into_iter()
        .map(|p| I::point(p.width()).square())
        .fold(I::point(0.), |a, b| a + b)
        .sqrt()
        .hi
}
fn split_domain(mut d: Domain, axis: usize) -> Result<[Domain; 2]> {
    let mid = d[axis][0] * 0.5 + d[axis][1] * 0.5;
    if mid <= d[axis][0] || mid >= d[axis][1] {
        return Err(unsupported(
            "native parameter subdivision reached machine precision",
        ));
    }
    let mut b = d;
    b[axis][0] = mid;
    d[axis][1] = mid;
    Ok([d, b])
}

fn partition(d: Domain, c: Domain) -> Vec<Domain> {
    let cuts = std::array::from_fn::<_, 2, _>(|i| {
        let mut x = vec![d[i][0], d[i][1]];
        x.extend(
            c[i].iter()
                .copied()
                .filter(|x| *x > d[i][0] && *x < d[i][1]),
        );
        x.sort_by(f64::total_cmp);
        x.dedup();
        x
    });
    cuts[0]
        .windows(2)
        .flat_map(|a| {
            cuts[1]
                .windows(2)
                .map(move |b| [[a[0], a[1]], [b[0], b[1]]])
        })
        .collect()
}
