//! Exact plane/half-space splits. Finite tools are bounded by positive native
//! control hulls; no displayed mesh or fitted intersection guide supplies intent.
use crate::adapter::{MonstertruckAdapter, Shape, TopologyHistory};
use cad_core::{
    CadError, Dimension, Document, NodeRef, Result, TopologyRef, expression,
    split::{Definition, Keep},
};
use monstertruck::modeling::*;
use std::collections::{BTreeMap, HashMap, HashSet};

const MAX_BODIES: usize = 1024;
const MAX_FACES: usize = 4096;
const MAX_EDGES: usize = 16384;
const MAX_HULL_POINTS: usize = 262144;
const MAX_COORDINATE: f64 = 1e12;
const MAX_RESULT_FACES: usize = 8192;
const MAX_RESULT_EDGES: usize = 32768;
const MAX_RESULT_CONTROLS: usize = 524288;
const MAX_WORK: usize = 64_000_000;
const MAX_REFERENCE_BYTES: usize = 32 * 1024 * 1024;
fn invalid(message: &str) -> CadError {
    CadError::Invalid(format!("Split: {message}"))
}
fn unsupported(message: &str) -> CadError {
    CadError::Unsupported(format!("Split: {message}"))
}
fn finite(point: Point3) -> bool {
    [point.x, point.y, point.z]
        .into_iter()
        .all(|coordinate| coordinate.is_finite() && coordinate.abs() <= MAX_COORDINATE)
}
fn normalized(vector: Vector3) -> Result<Vector3> {
    let length = vector.magnitude();
    if !length.is_finite() || length <= 1e-12 {
        return Err(invalid("A finite regular splitting plane is required"));
    }
    Ok(vector / length)
}
fn key(reference: &TopologyRef) -> Result<String> {
    Ok(serde_json::to_string(
        &crate::adapter::construction_reference(reference)?,
    )?)
}

fn spline_cost(knots: usize, controls: usize) -> Result<usize> {
    let degree = knots
        .checked_sub(controls + 1)
        .ok_or_else(|| unsupported("The native spline basis is invalid"))?;
    if controls == 0 || degree > 32 {
        return Err(unsupported(
            "Native spline control preflight supports nonempty bases of degree at most 32",
        ));
    }
    Ok(controls)
}
fn curve_cost(curve: &Curve, depth: usize) -> Result<usize> {
    if depth > 16 {
        return Err(unsupported(
            "Native carrier nesting exceeds sixteen bounded levels",
        ));
    }
    match curve {
        Curve::Line(_) => Ok(2),
        Curve::BsplineCurve(curve) => {
            spline_cost(curve.knot_vector().len(), curve.control_points().len())
        }
        Curve::NurbsCurve(curve) => {
            spline_cost(curve.knot_vector().len(), curve.control_points().len())
        }
        Curve::ParameterCurve(curve) => {
            let count = match curve.curve() {
                Curve2D::Line(_) => 2,
                Curve2D::Polyline(points) => points.0.len(),
                Curve2D::Conic(_) => 16,
                Curve2D::BsplineCurve(curve) => {
                    spline_cost(curve.knot_vector().len(), curve.control_points().len())?
                }
                Curve2D::NurbsCurve(curve) => {
                    spline_cost(curve.knot_vector().len(), curve.control_points().len())?
                }
            };
            count
                .checked_add(surface_cost(curve.surface(), depth + 1)?)
                .ok_or_else(|| unsupported("Native carrier control count overflowed"))
        }
        Curve::IntersectionCurve(curve) => {
            let leader = curve_cost(curve.leader(), depth + 1)?;
            let first = surface_cost(curve.surface0(), depth + 1)?;
            let second = surface_cost(curve.surface1(), depth + 1)?;
            leader
                .checked_add(first)
                .and_then(|count| count.checked_add(second))
                .ok_or_else(|| {
                    unsupported("Native intersection carrier controls exceed their bounded count")
                })
        }
    }
}
fn surface_cost(surface: &Surface, depth: usize) -> Result<usize> {
    if depth > 16 {
        return Err(unsupported(
            "Native carrier nesting exceeds sixteen bounded levels",
        ));
    }
    match surface {
        Surface::Plane(_) => Ok(4),
        Surface::BsplineSurface(surface) => {
            let rows = surface.control_points();
            let u = spline_cost(surface.knot_vector_u().len(), rows.len())?;
            let v = spline_cost(
                surface.knot_vector_v().len(),
                rows.first().map_or(0, Vec::len),
            )?;
            if rows.iter().any(|row| row.len() != v) {
                return Err(unsupported(
                    "A native surface control net is not rectangular",
                ));
            }
            u.checked_mul(v)
                .ok_or_else(|| unsupported("Native surface control count overflowed"))
        }
        Surface::NurbsSurface(surface) => {
            let rows = surface.control_points();
            let u = spline_cost(surface.knot_vector_u().len(), rows.len())?;
            let v = spline_cost(
                surface.knot_vector_v().len(),
                rows.first().map_or(0, Vec::len),
            )?;
            if rows.iter().any(|row| row.len() != v) {
                return Err(unsupported(
                    "A native rational surface control net is not rectangular",
                ));
            }
            u.checked_mul(v)
                .ok_or_else(|| unsupported("Native surface control count overflowed"))
        }
        // The exact finite conversion of one revolution uses bounded circular
        // stations; reserve more than its full-turn homogeneous expansion.
        Surface::RevolutionSurface(surface) => {
            curve_cost(surface.entity().entity_curve(), depth + 1)?
                .checked_mul(64)
                .ok_or_else(|| unsupported("Native revolution controls exceed bounded counts"))
        }
        Surface::SphericalSurface(_) | Surface::ToroidalSurface(_) => Ok(1024),
        _ => Err(unsupported(
            "This native surface has no bounded control preflight",
        )),
    }
}
/// Admission counts native controls before any conversion, hull allocation or
/// geometric topology copy. A conservative expansion allowance covers exact
/// analytic carriers; a fitted curve is never a geometric bound.
pub(crate) fn control_cost(solid: &Solid, limit: usize) -> Result<usize> {
    let mut count = 0usize;
    let mut add = |amount: usize| -> Result<()> {
        count = count
            .checked_add(amount)
            .ok_or_else(|| unsupported("Native topology/control count overflowed"))?;
        if count > limit {
            return Err(unsupported(
                "Native carrier controls exceed the remaining materialization budget",
            ));
        }
        Ok(())
    };
    for _ in solid.vertex_iter() {
        add(1)?;
    }
    for edge in solid.edge_iter() {
        add(edge.with_curve(|curve| curve_cost(curve, 0))?)?;
    }
    for face in solid.face_iter() {
        // This kernel exposes borrowed edge curves, but only mapped access to
        // a borrowed face surface. Unit boundaries avoid cloning an unchecked
        // surface control net; their size is already bounded by the edge pass.
        // Returning None prevents constructing the mapped face itself.
        let mut cost = None;
        let _: Option<monstertruck::topology::Face<(), (), ()>> = face.try_mapped(
            |_| Some(()),
            |_| Some(()),
            |surface| {
                cost = Some(surface_cost(surface, 0));
                None
            },
        );
        add(
            cost.ok_or_else(|| unsupported("Native surface preflight did not reach its carrier"))??
        )?;
    }
    Ok(count)
}
fn hull(solid: &Solid) -> Result<Vec<Point3>> {
    crate::native_bounds::solid(solid)
        .map(|faces| faces.iter().flat_map(|face| face.points().iter().copied()).collect())
        .map_err(|error| match error {
            CadError::Unsupported(_) => unsupported(
                "No exact finite native enclosure is available for this stock; intersection guides and display meshes cannot bound a cutter",
            ),
            other => other,
        })
}
struct Budget {
    work: usize,
    faces: usize,
    edges: usize,
    controls: usize,
    reference_bytes: usize,
}
impl Budget {
    fn new() -> Self {
        Self {
            work: MAX_WORK,
            faces: 0,
            edges: 0,
            controls: 0,
            reference_bytes: 0,
        }
    }
    fn spend(&mut self, work: usize) -> Result<()> {
        self.work=self.work.checked_sub(work).ok_or_else(||unsupported("Split exceeds 64 million bounded constructor/predicate work units across its scope"))?;
        Ok(())
    }
    fn result(&mut self, solids: &[Solid]) -> Result<()> {
        for solid in solids {
            let faces = solid.face_iter().take(MAX_RESULT_FACES + 1).count();
            let edges = solid.edge_iter().take(MAX_RESULT_EDGES + 1).count();
            self.faces = self.faces.saturating_add(faces);
            self.edges = self.edges.saturating_add(edges);
            if self.faces > MAX_RESULT_FACES || self.edges > MAX_RESULT_EDGES {
                return Err(unsupported(
                    "Retained split results exceed 8192 faces or 32768 edges before side copying",
                ));
            }
            self.controls = self
                .controls
                .checked_add(control_cost(solid, MAX_RESULT_CONTROLS - self.controls)?)
                .ok_or_else(|| unsupported("Retained control counts overflowed"))?;
            self.spend(faces.saturating_mul(edges + faces).saturating_mul(4))?;
        }
        Ok(())
    }
    fn references(&mut self, history: &TopologyHistory, solids: &[Solid]) -> Result<()> {
        for solid in solids {
            for face in solid.face_iter() {
                self.reference_bytes = self.reference_bytes.saturating_add(
                    history
                        .face_source(face.id())
                        .ok_or_else(|| unsupported("A result face lost its history"))?
                        .role
                        .len(),
                );
            }
            for edge in solid.edge_iter() {
                self.reference_bytes = self.reference_bytes.saturating_add(
                    history
                        .edge_source(edge.id())
                        .ok_or_else(|| unsupported("A result edge lost its history"))?
                        .role
                        .len(),
                );
            }
            if self.reference_bytes > MAX_REFERENCE_BYTES {
                return Err(unsupported(
                    "Split construction references exceed 32 MiB before output copying",
                ));
            }
        }
        Ok(())
    }
}

struct Stock {
    faces: HashMap<FaceId, TopologyRef>,
    edges: HashMap<EdgeId, TopologyRef>,
    carriers: HashMap<String, Vec<FaceId>>,
    closure: serde_json::Value,
}
impl Stock {
    fn new(adapter: &MonstertruckAdapter, body: &NodeRef, solid: &Solid) -> Result<Self> {
        let mut faces = HashMap::new();
        let mut edges = HashMap::new();
        let mut carriers: HashMap<String, Vec<FaceId>> = HashMap::new();
        let mut face_keys = HashSet::new();
        let mut edge_keys = HashSet::new();
        let mut reference_bytes = 0usize;
        for face in solid.face_iter() {
            let reference = adapter
                .face_source(body, face.id())
                .ok_or_else(|| unsupported("A source face has no current construction ancestry"))?;
            reference_bytes = reference_bytes.saturating_add(reference.role.len());
            if reference_bytes > MAX_REFERENCE_BYTES {
                return Err(unsupported(
                    "Source face ancestry exceeds 32 MiB before closure materialization",
                ));
            }
            if !face_keys.insert(key(&reference)?) {
                return Err(unsupported(
                    "Distinct source faces have ambiguous saved construction ancestry",
                ));
            }
            for surface in [face.surface(), face.surface().inverse()] {
                let entries = carriers
                    .entry(serde_json::to_string(&surface)?)
                    .or_default();
                if !entries.contains(&face.id()) {
                    entries.push(face.id());
                }
            }
            faces.insert(face.id(), reference);
            for edge in face.edge_iter() {
                if let std::collections::hash_map::Entry::Vacant(slot) = edges.entry(edge.id()) {
                    let reference = adapter.edge_source(body, edge.id()).ok_or_else(|| {
                        unsupported("A source edge has no current construction ancestry")
                    })?;
                    reference_bytes = reference_bytes.saturating_add(reference.role.len());
                    if reference_bytes > MAX_REFERENCE_BYTES {
                        return Err(unsupported(
                            "Source face and edge ancestry exceeds 32 MiB before closure materialization",
                        ));
                    }
                    if !edge_keys.insert(key(&reference)?) {
                        return Err(unsupported(
                            "Distinct source edges have ambiguous saved construction ancestry",
                        ));
                    }
                    slot.insert(reference);
                }
            }
        }
        let closure = crate::shell_history::closure(&faces.values().cloned().collect::<Vec<_>>())?;
        if serde_json::to_vec(&closure)?.len() > MAX_REFERENCE_BYTES {
            return Err(unsupported("Source construction closure exceeds 32 MiB"));
        }
        Ok(Self {
            faces,
            edges,
            carriers,
            closure,
        })
    }
}
struct Frame {
    origin: Point3,
    u: Vector3,
    v: Vector3,
    n: Vector3,
}
impl Frame {
    fn local(&self, point: Point3) -> Point3 {
        let p = point - self.origin;
        Point3::new(p.dot(self.u), p.dot(self.v), p.dot(self.n))
    }
    fn matrix(&self) -> Matrix4 {
        Matrix4::from_cols(
            self.u.extend(0.),
            self.v.extend(0.),
            self.n.extend(0.),
            self.origin.to_vec().extend(1.),
        )
    }
}
struct Tool {
    solid: Solid,
    cap: FaceId,
    cap_carriers: [String; 2],
    faces: HashSet<FaceId>,
}
fn tool(frame: &Frame, bounds: [[f64; 3]; 2], positive: bool, tolerance: f64) -> Result<Tool> {
    let span = (0..3)
        .map(|axis| bounds[1][axis] - bounds[0][axis])
        .fold(0., f64::max);
    let coordinate = bounds
        .iter()
        .flatten()
        .map(|value| value.abs())
        .fold(frame.origin.to_vec().magnitude(), f64::max);
    let margin = (span * 1e-6)
        .max(tolerance * 64.)
        .max(coordinate * f64::EPSILON * 256.)
        .max(1e-5);
    if !margin.is_finite() || margin <= 0. {
        return Err(unsupported(
            "Finite half-space containment padding cannot be established",
        ));
    }
    let min = Point3::new(
        bounds[0][0] - margin,
        bounds[0][1] - margin,
        if positive { 0. } else { bounds[0][2] - margin },
    );
    let max = Point3::new(
        bounds[1][0] + margin,
        bounds[1][1] + margin,
        if positive { bounds[1][2] + margin } else { 0. },
    );
    if ![min.x, min.y, min.z, max.x, max.y, max.z]
        .into_iter()
        .all(f64::is_finite)
        || max.x <= min.x
        || max.y <= min.y
        || max.z <= min.z
    {
        return Err(unsupported(
            "The finite half-space tool has invalid extents",
        ));
    }
    let solid: Solid = builder::transformed(
        &primitive::cuboid(BoundingBox::from_iter([min, max])),
        frame.matrix(),
    );
    if solid.vertex_iter().any(|vertex| !finite(vertex.point())) {
        return Err(unsupported(
            "The bounded half-space tool exceeds ±1e12 mm native coordinates",
        ));
    }
    // Prove all five artificial exterior planes stay strictly beyond the hull.
    if !(bounds[0][0] > min.x
        && bounds[1][0] < max.x
        && bounds[0][1] > min.y
        && bounds[1][1] < max.y
        && if positive {
            bounds[1][2] < max.z
        } else {
            bounds[0][2] > min.z
        })
    {
        return Err(unsupported(
            "Floating-point containment padding collapsed at this model scale",
        ));
    }
    let caps = solid
        .face_iter()
        .filter(|face| {
            face.vertex_iter()
                .all(|vertex| frame.local(vertex.point()).z.abs() <= tolerance)
        })
        .collect::<Vec<_>>();
    if caps.len() != 1 {
        return Err(unsupported(
            "The native half-space tool has no unique splitting cap at this precision",
        ));
    }
    let cap = caps[0].id();
    let cap_carriers = [
        serde_json::to_string(&caps[0].surface())?,
        serde_json::to_string(&caps[0].surface().inverse())?,
    ];
    let faces = solid.face_iter().map(|face| face.id()).collect();
    crate::validation::solid(&solid)?;
    Ok(Tool {
        solid,
        cap,
        cap_carriers,
        faces,
    })
}

fn detached(
    solids: &[Solid],
    history: &TopologyHistory,
    generated: &mut TopologyHistory,
) -> Result<Vec<Solid>> {
    // A cut exactly through an existing edge can share that native object on
    // opposite sides. Independent result bodies need distinct side ancestry.
    let mut shell = Shell::new();
    let mut seen = HashSet::new();
    for face in solids.iter().flat_map(Solid::face_iter) {
        if seen.insert(face.id()) {
            shell.push(face.absolute_clone());
        }
    }
    let copied = builder::transformed(&shell, Matrix4::identity());
    if copied.len() != shell.len() {
        return Err(invalid("Side copying changed the native face constructors"));
    }
    let mut faces = HashMap::new();
    let mut edges = HashMap::new();
    for (source, target) in shell.iter().zip(copied.iter()) {
        generated.inherit_face(
            target.id(),
            history
                .face_source(source.id())
                .ok_or_else(|| unsupported("A split face lost its constructor ancestry"))?,
        );
        faces.insert(source.id(), target.clone());
        let a = source.edge_iter().collect::<Vec<_>>();
        let b = target.edge_iter().collect::<Vec<_>>();
        if a.len() != b.len() {
            return Err(invalid(
                "Side copying changed the native boundary constructors",
            ));
        }
        for (source, target) in a.into_iter().zip(b) {
            if let Some(previous) = edges.insert(source.id(), target.id()) {
                if previous != target.id() {
                    return Err(invalid(
                        "Side copying failed to retain a shared native boundary",
                    ));
                }
            } else {
                generated.inherit_edge(
                    target.id(),
                    history
                        .edge_source(source.id())
                        .ok_or_else(|| unsupported("A split edge lost its constructor ancestry"))?,
                );
            }
        }
    }
    solids
        .iter()
        .map(|solid| {
            let boundaries = solid
                .boundaries()
                .iter()
                .map(|boundary| {
                    boundary
                        .iter()
                        .map(|face| {
                            let mut copy = faces.get(&face.id()).cloned().ok_or_else(|| {
                                invalid("A result face was omitted from its side copy")
                            })?;
                            if copy.orientation() != face.orientation() {
                                copy.invert();
                            }
                            Ok(copy)
                        })
                        .collect::<Result<Shell>>()
                })
                .collect::<Result<Vec<_>>>()?;
            let solid = Solid::try_new(boundaries).map_err(|error| invalid(&error.to_string()))?;
            crate::validation::solid(&solid)?;
            Ok(solid)
        })
        .collect()
}

fn tag_side(
    owner: &NodeRef,
    definition: &Definition,
    stock: &Stock,
    solids: &[Solid],
    cut: Option<&crate::planar::CutOutput>,
    tool: Option<&Tool>,
    positive: bool,
) -> Result<TopologyHistory> {
    let plane_reference = TopologyRef {
        producer: owner.clone(),
        topology: "plane".into(),
        role: format!(
            "construction:{}",
            serde_json::to_string(
                &serde_json::json!({"sources":[],"topology":"plane","purpose":{"operation":"split_plane","plane_source":definition.plane}})
            )?
        ),
        signature: None,
    };
    let mut origins: HashMap<FaceId, Vec<TopologyRef>> = HashMap::new();
    if let Some(cut) = cut {
        for tag in &cut.faces {
            let mut sources = Vec::new();
            for (source, from_tool) in &tag.sources {
                if *from_tool {
                    if !tool.is_some_and(|tool| *source == tool.cap) {
                        return Err(unsupported(
                            "A finite cutter exterior face unexpectedly entered the split result",
                        ));
                    }
                    sources.push(plane_reference.clone());
                } else {
                    sources.push(stock.faces.get(source).cloned().ok_or_else(|| {
                        unsupported("A reconstructed split face lost its stock ancestry")
                    })?);
                }
            }
            if tool.is_some_and(|tool| tag.partition_sources.contains(&tool.cap)) {
                sources.push(plane_reference.clone());
            }
            origins.insert(tag.face, sources);
        }
    }
    let mut adjacency: HashMap<EdgeId, Vec<FaceId>> = HashMap::new();
    for face in solids.iter().flat_map(Solid::face_iter) {
        if let std::collections::hash_map::Entry::Vacant(slot) = origins.entry(face.id()) {
            let sources = if let Some(reference) = stock.faces.get(&face.id()) {
                vec![reference.clone()]
            } else {
                let carrier = serde_json::to_string(&face.surface())?;
                if tool.is_some_and(|tool| tool.cap_carriers.contains(&carrier)) {
                    vec![plane_reference.clone()]
                } else {
                    let candidates = stock
                        .carriers
                        .get(&carrier)
                        .map(Vec::as_slice)
                        .unwrap_or(&[]);
                    if candidates.len() != 1 {
                        return Err(unsupported(
                            "A Boolean split face has missing or ambiguous exact source-carrier ancestry",
                        ));
                    }
                    vec![stock.faces[&candidates[0]].clone()]
                }
            };
            slot.insert(sources);
        }
        for edge in face.edge_iter() {
            adjacency.entry(edge.id()).or_default().push(face.id());
        }
    }
    let side = if positive { "positive" } else { "negative" };
    let purpose = serde_json::json!({"operation":"split","side":side,"plane_source":definition.plane,"stock_sources":stock.closure});
    let mut history = TopologyHistory::default();
    let mut face_keys = HashSet::new();
    let mut reference_bytes = 0usize;
    for face in solids.iter().flat_map(Solid::face_iter) {
        let mut sources = origins[&face.id()].clone();
        for edge in face.edge_iter() {
            for adjacent in &adjacency[&edge.id()] {
                sources.extend(origins[adjacent].iter().cloned());
            }
            if let Some(reference) = stock.edges.get(&edge.id()) {
                sources.push(reference.clone());
            }
        }
        let mut face_purpose = purpose.clone();
        face_purpose["carrier_sources"] = crate::shell_history::closure(&origins[&face.id()])?;
        let reference = history.tool_face(owner, face, &sources, &face_purpose)?;
        reference_bytes = reference_bytes.saturating_add(reference.role.len());
        if reference_bytes > MAX_REFERENCE_BYTES {
            return Err(unsupported(
                "Side face ancestry exceeds 32 MiB during construction",
            ));
        }
        if !face_keys.insert(key(&reference)?) {
            return Err(unsupported(
                "Disconnected split faces have indistinguishable construction ancestry; explicit segmented source reattachment is required",
            ));
        }
    }
    let mut edge_keys = HashSet::new();
    for (edge, face_ids) in adjacency {
        if face_ids.len() != 2 {
            return Err(unsupported(
                "A split boundary has ambiguous native face incidence",
            ));
        }
        let mut sources = face_ids
            .iter()
            .map(|face| {
                history.face_source(*face).ok_or_else(|| {
                    unsupported("An incident split wall has no constructor ancestry")
                })
            })
            .collect::<Result<Vec<_>>>()?;
        if let Some(reference) = stock.edges.get(&edge) {
            sources.push(reference.clone());
        }
        let reference=history.constructed_edge(owner,edge,&sources,&serde_json::json!({"operation":"split","side":side,"constructor":"shared_boundary","stock_sources":stock.closure}))?;
        reference_bytes = reference_bytes.saturating_add(reference.role.len());
        if reference_bytes > MAX_REFERENCE_BYTES {
            return Err(unsupported(
                "Side boundary ancestry exceeds 32 MiB during construction",
            ));
        }
        if !edge_keys.insert(key(&reference)?) {
            return Err(unsupported(
                "Several split boundaries have indistinguishable source-wall ancestry; segmented edges require explicit exact reattachment",
            ));
        }
    }
    Ok(history)
}

pub(crate) fn operation(
    adapter: &MonstertruckAdapter,
    doc: &Document,
    owner: &NodeRef,
    definition: &Definition,
    inputs: &BTreeMap<NodeRef, Shape>,
    tolerance: f64,
    generated: &mut TopologyHistory,
) -> Result<Shape> {
    definition.validate()?;
    if definition
        .scope
        .iter()
        .chain(definition.plane.dependencies().iter())
        .any(|source| source.definition != owner.definition)
    {
        return Err(invalid(
            "Split body and plane sources must belong to the current part definition; use Edit in Place before authoring this feature",
        ));
    }
    if !tolerance.is_finite() || tolerance <= 0. {
        return Err(invalid("A finite positive document tolerance is required"));
    }
    let plane = adapter.resolve_surface_plane(doc, &definition.plane, inputs)?;
    let u = normalized(Vector3::from(plane.u))?;
    let base_normal = normalized(u.cross(Vector3::from(plane.v)))?;
    let n = base_normal * if definition.reverse { -1. } else { 1. };
    let offset = expression::value(
        doc,
        &owner.definition,
        &definition.offset,
        Dimension::Length,
    )?;
    if !offset.is_finite() {
        return Err(invalid("The plane offset must be finite"));
    }
    let frame = Frame {
        origin: Point3::from(plane.origin) + base_normal * offset,
        u,
        v: n.cross(u),
        n,
    };
    if !finite(frame.origin) {
        return Err(unsupported("Split plane coordinates exceed ±1e12 mm"));
    }
    let mut result = Vec::new();
    let mut body_count = 0usize;
    let mut face_count = 0usize;
    let mut edge_count = 0usize;
    let mut hull_count = 0usize;
    let mut budget = Budget::new();
    let mut solid_keys: Vec<HashSet<(FaceId, bool)>> = Vec::new();
    let mut face_owners: HashMap<FaceId, Vec<usize>> = HashMap::new();
    for body in &definition.scope {
        let Some(Shape::Bodies(solids)) = inputs.get(body) else {
            return Err(invalid(
                "Every scoped source must produce current native solid bodies",
            ));
        };
        for solid in solids {
            body_count += 1;
            let faces = solid.face_iter().count();
            let edges = solid.edge_iter().count();
            face_count = face_count.saturating_add(faces);
            edge_count = edge_count.saturating_add(edges);
            if body_count > MAX_BODIES
                || faces > 512
                || edges > 2048
                || face_count > MAX_FACES
                || edge_count > MAX_EDGES
            {
                return Err(unsupported(
                    "Split exceeds its source bound: 1024 solids, 4096 faces and 16384 edges in total; each solid supports up to 512 faces and 2048 edges",
                ));
            }
            let identity = solid
                .face_iter()
                .map(|face| (face.id(), face.orientation()))
                .collect::<HashSet<_>>();
            if solid.face_iter().next().is_some_and(|face| {
                face_owners.get(&face.id()).is_some_and(|previous| {
                    previous.iter().any(|index| solid_keys[*index] == identity)
                })
            }) {
                return Err(invalid(
                    "Scoped outputs contain the same exact native solid more than once; remove overlapping source aliases",
                ));
            }
            let identity_index = solid_keys.len();
            for (face, _) in &identity {
                face_owners.entry(*face).or_default().push(identity_index);
            }
            solid_keys.push(identity);
            let source_controls = control_cost(solid, MAX_HULL_POINTS - hull_count)?;
            hull_count = hull_count
                .checked_add(source_controls)
                .ok_or_else(|| unsupported("Source control counts overflowed"))?;
            let vertices = solid.vertex_iter().take(2049).count();
            if vertices > 2048 {
                return Err(unsupported(
                    "Each scoped solid supports up to 2048 native vertices",
                ));
            }
            budget.spend(
                (faces + 6)
                    .saturating_mul(edges + vertices + faces)
                    .saturating_mul(8),
            )?;
            let points = hull(solid)?;
            if points.is_empty() || points.len() > MAX_HULL_POINTS {
                return Err(unsupported(
                    "Split needs a nonempty exact stock enclosure within 262144 points",
                ));
            }
            let mut bounds = [[f64::INFINITY; 3], [f64::NEG_INFINITY; 3]];
            for point in points {
                if !finite(point) {
                    return Err(unsupported(
                        "Stock bounds exceed finite ±1e12 mm coordinates",
                    ));
                }
                let p = frame.local(point);
                for (axis, value) in [p.x, p.y, p.z].into_iter().enumerate() {
                    if !value.is_finite() {
                        return Err(invalid("Stock projection onto the plane is nonfinite"));
                    }
                    bounds[0][axis] = bounds[0][axis].min(value);
                    bounds[1][axis] = bounds[1][axis].max(value);
                }
            }
            if bounds[0][2] == 0. && bounds[1][2] == 0. {
                return Err(invalid(
                    "The source solid is collapsed onto its splitting plane",
                ));
            }
            if bounds[0][2] < 0. && bounds[1][2] > 0. {
                budget.spend(
                    source_controls
                        .saturating_mul(faces * 2 + 12)
                        .saturating_mul(if definition.keep == Keep::Both { 2 } else { 1 }),
                )?;
            }
            let stock = Stock::new(adapter, body, solid)?;
            let mut returned_keys: Vec<HashSet<(FaceId, bool)>> = Vec::new();
            for positive in [true, false] {
                if matches!(
                    (definition.keep, positive),
                    (Keep::Positive, false) | (Keep::Negative, true)
                ) {
                    continue;
                }
                let entirely_positive = bounds[0][2] >= 0.;
                let entirely_negative = bounds[1][2] <= 0.;
                if (entirely_positive && !positive) || (entirely_negative && positive) {
                    continue;
                }
                if entirely_positive || entirely_negative {
                    let solids = std::slice::from_ref(solid);
                    budget.result(solids)?;
                    let history =
                        tag_side(owner, definition, &stock, solids, None, None, positive)?;
                    budget.references(&history, solids)?;
                    result.extend(detached(solids, &history, generated)?);
                    continue;
                }
                // Remove the opposite half-space. Rational control convex hulls
                // prove every artificial tool wall lies outside the stock.
                let cutter = tool(&frame, bounds, !positive, tolerance)?;
                let direct = if faces <= 128
                    && edges <= 512
                    && solid
                        .edge_iter()
                        .all(|edge| matches!(edge.curve(), Curve::Line(_)))
                {
                    crate::planar::convex_difference(solid, &cutter.solid)?
                } else {
                    None
                };
                let solids = if let Some(cut) = &direct {
                    cut.bodies.clone()
                } else {
                    let cut = monstertruck::solid::difference(solid, &cutter.solid, tolerance)
                        .map_err(|error| {
                            invalid(&format!("Native half-space Boolean failed: {error}"))
                        })?;
                    if cut.boundaries().is_empty() {
                        continue;
                    }
                    crate::validation::solid(&cut)?;
                    if cut.boundaries().len() != 1 {
                        return Err(unsupported(
                            "The curved/nonconvex Boolean split currently needs one connected boundary per retained side; separate cavity/disconnected-shell containment requires further exact classification",
                        ));
                    }
                    vec![cut]
                };
                if solids.is_empty() {
                    continue;
                }
                for solid in &solids {
                    let identity = solid
                        .face_iter()
                        .map(|face| (face.id(), face.orientation()))
                        .collect::<HashSet<_>>();
                    if returned_keys.contains(&identity) {
                        return Err(unsupported(
                            "Native precision returned the same material on both retained sides; move the splitting plane farther from the existing boundary",
                        ));
                    }
                    returned_keys.push(identity);
                }
                budget.result(&solids)?;
                if solids.iter().any(|solid| {
                    solid
                        .face_iter()
                        .any(|face| cutter.faces.contains(&face.id()) && face.id() != cutter.cap)
                }) {
                    return Err(unsupported(
                        "An artificial finite cutter wall entered the split result",
                    ));
                }
                let history = tag_side(
                    owner,
                    definition,
                    &stock,
                    &solids,
                    direct.as_ref(),
                    Some(&cutter),
                    positive,
                )?;
                budget.references(&history, &solids)?;
                result.extend(detached(&solids, &history, generated)?);
                if result.len() > MAX_BODIES * 2 {
                    return Err(unsupported("Split exceeds 2048 retained result solids"));
                }
            }
        }
    }
    Ok(Shape::Bodies(result))
}
