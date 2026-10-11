//! Strict native STEP surface import and representation ownership.
use crate::adapter::TopologyHistory;
use cad_core::{CadError, NodeRef, Result, TopologyRef};
use monstertruck::{
    modeling::*,
    step::load::Table,
    topology::compress::{CompressedEdge, CompressedFace, CompressedShell},
};
use std::collections::BTreeSet;
use step_p21::{
    ast::{EntityInstance, Exchange, Name, Parameter, Record},
    tables::PlaceHolder,
};

fn invalid(message: impl Into<String>) -> CadError {
    CadError::Invalid(format!("STEP import: {}", message.into()))
}
fn surface_representation(name: &str) -> bool {
    matches!(
        name,
        "MANIFOLD_SURFACE_SHAPE_REPRESENTATION"
            | "GEOMETRICALLY_BOUNDED_SURFACE_SHAPE_REPRESENTATION"
    )
}
fn representation_fields(id: u64, name: &str, parameter: &Parameter) -> Result<()> {
    let Parameter::List(fields) = parameter else {
        return Err(invalid(format!(
            "Representation #{id} ({name}) has no inherited attribute list"
        )));
    };
    if fields.len() != 3
        || !matches!(&fields[0], Parameter::String(_))
        || !matches!(&fields[2], Parameter::Ref(Name::Entity(context)) if *context > 0)
    {
        return Err(invalid(format!(
            "Representation #{id} ({name}) must retain its exact name/items/context fields"
        )));
    }
    let Parameter::List(items) = &fields[1] else {
        return Err(invalid(format!(
            "Representation #{id} ({name}) has no item set"
        )));
    };
    let mut seen = BTreeSet::new();
    if items.is_empty()
        || items.iter().any(|item| {
            let Parameter::Ref(Name::Entity(item)) = item else {
                return true;
            };
            *item == 0 || !seen.insert(*item)
        })
    {
        return Err(invalid(format!(
            "Representation #{id} ({name}) requires a nonempty set of distinct stable item references"
        )));
    }
    Ok(())
}
/// Exact application-owned adapter for the two surface representation subtypes
/// whose inherited representation fields match the native holder. The original
/// source bytes, entity identities, item/context references and product links
/// remain authoritative; this changes only the parsed cache's native dispatch.
///
/// STEP Tools EXPRESS definitions (AP203e2/AP214/AP242):
/// https://www.steptools.com/docs/stp_aim/html/t_manifold_surface_shape_representation.html
/// https://www.steptools.com/stds/stp_aim/html/t_geometrically_bounded_surface_shape_representation.html
pub(crate) fn load_table(source: &str) -> Result<Table> {
    let exchange: Exchange = source
        .parse()
        .map_err(|error: step_p21::error::Error| invalid(error.to_string()))?;
    if exchange.data.is_empty() {
        return Err(invalid("The pinned resource has no DATA section"));
    }
    let mut instances = Vec::new();
    let mut seen = BTreeSet::new();
    let mut adapted = Vec::new();
    for instance in exchange.data.into_iter().flat_map(|data| data.entities) {
        let id = match &instance {
            EntityInstance::Simple { id, .. } | EntityInstance::Complex { id, .. } => *id,
        };
        if id == 0 || !seen.insert(id) {
            return Err(invalid(format!(
                "The pinned resource repeats or has an invalid entity identity #{id}"
            )));
        }
        let instance = match instance {
            EntityInstance::Simple { id, mut record } if surface_representation(&record.name) => {
                representation_fields(id, &record.name, &record.parameter)?;
                adapted.push(id);
                record.name = "SHAPE_REPRESENTATION".into();
                EntityInstance::Simple { id, record }
            }
            EntityInstance::Complex { id, subsuper }
                if subsuper
                    .0
                    .iter()
                    .any(|record| surface_representation(&record.name)) =>
            {
                let subtype = subsuper
                    .0
                    .iter()
                    .filter(|record| surface_representation(&record.name))
                    .collect::<Vec<_>>();
                let base = subsuper
                    .0
                    .iter()
                    .filter(|record| record.name == "REPRESENTATION")
                    .collect::<Vec<_>>();
                let shape = subsuper
                    .0
                    .iter()
                    .filter(|record| record.name == "SHAPE_REPRESENTATION")
                    .collect::<Vec<_>>();
                if subsuper.0.len() != 3
                    || subtype.len() != 1
                    || base.len() != 1
                    || shape.len() != 1
                    || !matches!(&subtype[0].parameter, Parameter::List(fields) if fields.is_empty())
                    || !matches!(&shape[0].parameter, Parameter::List(fields) if fields.is_empty())
                {
                    return Err(CadError::Unsupported(format!(
                        "STEP surface representation #{id} has unsupported complex inheritance; expected only its subtype, SHAPE_REPRESENTATION and three-field REPRESENTATION"
                    )));
                }
                representation_fields(id, &subtype[0].name, &base[0].parameter)?;
                adapted.push(id);
                EntityInstance::Simple {
                    id,
                    record: Record {
                        name: "SHAPE_REPRESENTATION".into(),
                        parameter: base[0].parameter.clone(),
                    },
                }
            }
            instance => instance,
        };
        instances.push(instance);
    }
    // Unlike the upstream string constructor, use every DATA section. Losing
    // another section would silently discard valid definitions or links.
    let table = Table::from_iter(instances.iter());
    if let Some(id) = adapted
        .into_iter()
        .find(|id| !table.shape_representation.contains_key(id))
    {
        return Err(invalid(format!(
            "The native holder refused adapted surface representation #{id}"
        )));
    }
    Ok(table)
}
/// A graph-free file still has representation ownership. Refuse unsupported
/// listed geometry instead of silently collecting only recognized B-reps.
pub(crate) fn unstructured_items(table: &Table, source: &str) -> Result<(Vec<u64>, Vec<u64>)> {
    // Inspect actual parsed entity names, never the loader's Debug-text dummy
    // records. An unheld representation must not turn into a partial import.
    let exchange: Exchange = source
        .parse()
        .map_err(|error: step_p21::error::Error| invalid(error.to_string()))?;
    for instance in exchange.data.into_iter().flat_map(|data| data.entities) {
        let (id, records) = match instance {
            EntityInstance::Simple { id, record } => (id, vec![record]),
            EntityInstance::Complex { id, subsuper } => (id, subsuper.0),
        };
        for record in records {
            if (record.name.ends_with("_SHAPE_REPRESENTATION")
                && !surface_representation(&record.name)
                && !matches!(
                    record.name.as_str(),
                    "ADVANCED_BREP_SHAPE_REPRESENTATION" | "CONTEXT_DEPENDENT_SHAPE_REPRESENTATION"
                ))
                || matches!(
                    record.name.as_str(),
                    "GEOMETRIC_SET"
                        | "GEOMETRIC_CURVE_SET"
                        | "SHELL_BASED_WIREFRAME_MODEL"
                        | "EDGE_BASED_WIREFRAME_MODEL"
                        | "TESSELLATED_SHAPE_REPRESENTATION"
                )
            {
                return Err(CadError::Unsupported(format!(
                    "STEP graph-free representation #{id} uses unsupported geometry type {}",
                    record.name
                )));
            }
        }
    }
    if !table
        .shape_representation_relationship_with_transformation
        .is_empty()
        || !table.context_dependent_shape_representation.is_empty()
    {
        return Err(CadError::Unsupported(
            "STEP transformed representations require a complete product occurrence graph".into(),
        ));
    }
    let mut solids = BTreeSet::new();
    let mut surfaces = BTreeSet::new();
    if table.shape_representation.is_empty() {
        if !table.geometric_curve_set.is_empty() {
            return Err(CadError::Unsupported(
                "STEP geometric curve sets are not supported by whole-file B-rep import".into(),
            ));
        }
        solids.extend(table.manifold_solid_brep.keys().copied());
        surfaces.extend(table.shell_based_surface_model.keys().copied());
    } else {
        let representations = table
            .shape_representation
            .keys()
            .copied()
            .collect::<BTreeSet<_>>();
        if representations.len() > 4096 {
            return Err(CadError::Unsupported(
                "STEP import exceeds 4096 representations".into(),
            ));
        }
        for representation in representations {
            let (bodies, shells) = items(table, &[], Some(representation))?;
            solids.extend(bodies);
            surfaces.extend(shells);
        }
    }
    Ok((solids.into_iter().collect(), surfaces.into_iter().collect()))
}

/// Untransformed representation links share the definition coordinates.
/// Transformed assembly links are handled by the upstream product DAG.
pub(crate) fn items(
    table: &Table,
    direct: &[u64],
    representation: Option<u64>,
) -> Result<(Vec<u64>, Vec<u64>)> {
    let mut all = direct.iter().copied().collect::<BTreeSet<_>>();
    let mut pending = representation.into_iter().collect::<Vec<_>>();
    let mut seen = BTreeSet::new();
    while let Some(id) = pending.pop() {
        if !seen.insert(id) {
            continue;
        }
        if seen.len() > 4096 {
            return Err(CadError::Unsupported(
                "STEP representation dependency graph exceeds 4096 nodes".into(),
            ));
        }
        let rep = table
            .shape_representation
            .get(&id)
            .ok_or_else(|| invalid(format!("Related representation #{id} is missing")))?;
        for item in &rep.items {
            let PlaceHolder::Ref(Name::Entity(item)) = item else {
                return Err(invalid("Inline representation items are unsupported"));
            };
            all.insert(*item);
        }
        for link in table.shape_representation_relationship.values() {
            if matches!(&link.rep_1, PlaceHolder::Ref(Name::Entity(from)) if *from==id) {
                let PlaceHolder::Ref(Name::Entity(to)) = &link.rep_2 else {
                    return Err(invalid(
                        "Related representation has no stable entity identity",
                    ));
                };
                pending.push(*to);
            }
        }
    }
    let mut solids = Vec::new();
    let mut surfaces = Vec::new();
    for id in all {
        if table.manifold_solid_brep.contains_key(&id) {
            solids.push(id);
        } else if table.shell_based_surface_model.contains_key(&id) {
            surfaces.push(id);
        } else if !table.axis2_placement_3d.contains_key(&id) {
            return Err(CadError::Unsupported(format!(
                "STEP representation item #{id} is not a supported solid, shell-based surface model, or placement"
            )));
        }
    }
    Ok((solids, surfaces))
}

pub(crate) fn surfaces(
    table: &Table,
    ids: &[u64],
    scale: f64,
    fingerprint: &str,
    owner: &NodeRef,
    history: &mut TopologyHistory,
) -> Result<Vec<Shell>> {
    if ids.is_empty()
        || ids.len() > 4096
        || ids.iter().copied().collect::<BTreeSet<_>>().len() != ids.len()
    {
        return Err(invalid(
            "Surface import requires 1–4096 distinct pinned model identities",
        ));
    }
    if !scale.is_finite() || scale <= 0. {
        return Err(invalid("Length conversion must be finite and positive"));
    }
    let mut result = Vec::new();
    let mut topology_count = 0usize;
    for id in ids {
        let model = table.shell_based_surface_model.get(id).ok_or_else(|| {
            invalid(format!(
                "Surface model #{id} is absent in the pinned resource"
            ))
        })?;
        if model.sbsm_boundary.is_empty() {
            return Err(invalid(format!("Surface model #{id} has no shells")));
        }
        for boundary in &model.sbsm_boundary {
            if result.len() >= 4096 {
                return Err(CadError::Unsupported(
                    "STEP surface import exceeds 4096 shells".into(),
                ));
            }
            let PlaceHolder::Ref(Name::Entity(shell_id)) = boundary else {
                return Err(invalid(
                    "Inline surface shells have no pinned entity identity",
                ));
            };
            let (native, report) = if let Some(shell) = table.shell.get(shell_id) {
                table.to_compressed_shell_reported(shell)
            } else if let Some(shell) = table.oriented_shell.get(shell_id) {
                table.to_compressed_shell_reported(shell)
            } else {
                return Err(invalid(format!("Surface shell #{shell_id} is missing")));
            }
            .map_err(|error| invalid(error.to_string()))?;
            report
                .require_lossless()
                .map_err(|error| invalid(error.to_string()))?;
            topology_count = topology_count
                .saturating_add(native.vertices.len())
                .saturating_add(native.edges.len())
                .saturating_add(native.faces.len());
            if topology_count > 2_000_000 {
                return Err(CadError::Unsupported(
                    "STEP surface import exceeds two million topology entities".into(),
                ));
            }
            let (face_sources, edge_sources) = sources(table, *shell_id)?;
            if face_sources.len() != native.faces.len() || edge_sources.len() != native.edges.len()
            {
                return Err(invalid(
                    "The native converter changed the pinned topology-source correspondence",
                ));
            }
            let bindings = native
                .faces
                .iter()
                .map(|face| {
                    face.boundaries
                        .iter()
                        .map(|wire| wire.iter().map(|edge| edge.index).collect::<Vec<_>>())
                        .collect::<Vec<_>>()
                })
                .collect::<Vec<_>>();
            let shell = native_shell(native)?;
            let shell = builder::transformed(&shell, Matrix4::from_scale(scale));
            for face in &shell {
                match face.surface() {
                    Surface::Plane(_) => finite_plane(&face.surface())?,
                    Surface::BsplineSurface(_) | Surface::NurbsSurface(_) => {
                        crate::tangent_plane::full_rational_patch(face).map_err(|error| match error {
                            CadError::Unsupported(message) => CadError::Unsupported(format!("STEP surface import requires a proven full natural rational patch: {message}")),
                            other => other,
                        })?;
                    }
                    _ => return Err(CadError::Unsupported("STEP surface import does not yet retain periodic or swept per-edge-use UV trim authority".into())),
                }
            }
            crate::validation::surface(&shell)?;
            for ((face, face_source), wires) in shell.iter().zip(face_sources).zip(bindings) {
                history.inherit_face(
                    face.id(),
                    imported_reference(owner, fingerprint, *id, *shell_id, "face", face_source),
                );
                if face.absolute_boundaries().len() != wires.len() {
                    return Err(invalid(
                        "The native constructor changed the pinned face boundaries",
                    ));
                }
                for (wire, indices) in face.absolute_boundaries().iter().zip(wires) {
                    if wire.len() != indices.len() {
                        return Err(invalid(
                            "The native constructor changed the pinned edge uses",
                        ));
                    }
                    for (edge, index) in wire.iter().zip(indices) {
                        let source = *edge_sources
                            .get(index)
                            .ok_or_else(|| invalid("Pinned edge correspondence is missing"))?;
                        let reference =
                            imported_reference(owner, fingerprint, *id, *shell_id, "edge", source);
                        if history.edge_source(edge.id()).is_some_and(|old| {
                            old.producer != reference.producer || old.role != reference.role
                        }) {
                            return Err(invalid(
                                "A native edge has conflicting STEP entity ancestry",
                            ));
                        }
                        history.inherit_edge(edge.id(), reference);
                    }
                }
            }
            result.push(shell);
        }
    }
    Ok(result)
}

fn imported_reference(
    owner: &NodeRef,
    fingerprint: &str,
    model: u64,
    shell: u64,
    topology: &str,
    entity: u64,
) -> TopologyRef {
    TopologyRef {
        producer: owner.clone(),
        role: format!("step-surface:{fingerprint}:model/{model}:shell/{shell}:{topology}/{entity}"),
        topology: topology.into(),
        signature: None,
    }
}
fn entity<T>(source: &PlaceHolder<T>) -> Result<u64> {
    match source {
        PlaceHolder::Ref(Name::Entity(id)) => Ok(*id),
        _ => Err(invalid(
            "Imported topology needs an explicit STEP entity reference",
        )),
    }
}
/// Mirror the lossless converter's ordered source traversal, retaining actual
/// STEP entities. Compressed indices only join this traversal to the constructor.
fn sources(table: &Table, shell_id: u64) -> Result<(Vec<u64>, Vec<u64>)> {
    let shell_id = table
        .oriented_shell
        .get(&shell_id)
        .map(|shell| entity(&shell.shell_element))
        .transpose()?
        .unwrap_or(shell_id);
    let shell = table
        .shell
        .get(&shell_id)
        .ok_or_else(|| invalid("Pinned source shell is missing"))?;
    let mut faces = Vec::new();
    let mut edges = Vec::new();
    let mut seen = BTreeSet::new();
    for face in &shell.cfs_faces {
        let id = entity(face)?;
        let id = table
            .oriented_face
            .get(&id)
            .map(|face| entity(&face.face_element))
            .transpose()?
            .unwrap_or(id);
        let face = table
            .face_surface
            .get(&id)
            .ok_or_else(|| invalid("Pinned source face is missing"))?;
        faces.push(id);
        for bound in &face.bounds {
            let bound = table
                .face_bound
                .get(&entity(bound)?)
                .ok_or_else(|| invalid("Pinned source face bound is missing"))?;
            let wire = table
                .edge_loop
                .get(&entity(&bound.bound)?)
                .ok_or_else(|| invalid("Pinned source edge loop is missing"))?;
            for edge in &wire.edge_list {
                let id = entity(edge)?;
                let id = table
                    .oriented_edge
                    .get(&id)
                    .map(|edge| entity(&edge.edge_element))
                    .transpose()?
                    .unwrap_or(id);
                if !table.edge_curve.contains_key(&id) {
                    return Err(invalid("Pinned source edge is missing"));
                }
                if seen.insert(id) {
                    edges.push(id);
                }
            }
        }
    }
    Ok((faces, edges))
}

fn native_shell(
    source: CompressedShell<
        Point3,
        monstertruck::step::load::step_geometry::Curve3D,
        monstertruck::step::load::step_geometry::Surface,
    >,
) -> Result<Shell> {
    let edges = source
        .edges
        .into_iter()
        .map(|edge| {
            Ok(CompressedEdge {
                vertices: edge.vertices,
                curve: strict_curve(&edge.curve, 0)?,
            })
        })
        .collect::<Result<_>>()?;
    let faces = source
        .faces
        .into_iter()
        .map(|face| {
            strict_carrier(&face.surface)?;
            Ok(CompressedFace {
                boundaries: face.boundaries,
                orientation: face.orientation,
                surface: Surface::try_from(&face.surface)
                    .map_err(|error| invalid(error.to_string()))?,
            })
        })
        .collect::<Result<_>>()?;
    Shell::extract(CompressedShell {
        vertices: source.vertices,
        edges,
        faces,
        vertex_stable_ids: source.vertex_stable_ids,
        edge_stable_ids: source.edge_stable_ids,
        face_stable_ids: source.face_stable_ids,
    })
    .map_err(|error| invalid(error.to_string()))
}

fn finite_plane(surface: &Surface) -> Result<()> {
    let point = surface.subs(0., 0.);
    let u = surface.uder(0., 0.);
    let v = surface.vder(0., 0.);
    let ul = u.magnitude();
    let vl = v.magnitude();
    if ![
        point.x, point.y, point.z, u.x, u.y, u.z, v.x, v.y, v.z, ul, vl,
    ]
    .into_iter()
    .all(f64::is_finite)
        || ul <= 0.
        || vl <= 0.
        || (u / ul).cross(v / vl).magnitude() <= 1e-10
    {
        return Err(invalid(
            "A native plane carrier is nonfinite, singular or ill-conditioned",
        ));
    }
    Ok(())
}
fn strict_carrier(surface: &monstertruck::step::load::step_geometry::Surface) -> Result<()> {
    use monstertruck::geometry::prelude::TryIntoHomogeneousBsplineSurface;
    use monstertruck::step::load::step_geometry::{ElementarySurface, Surface as StepSurface};
    match surface {
        StepSurface::ElementarySurface(ElementarySurface::Plane(_)) => {
            let native = Surface::try_from(surface).map_err(|error| invalid(error.to_string()))?;
            finite_plane(&native)
        }
        StepSurface::BsplineSurface(_) | StepSurface::NurbsSurface(_) => {
            let homogeneous = surface.try_into_homogeneous_bspline_surface().ok_or_else(|| invalid("A rational carrier has no exact homogeneous representation"))?;
            let patch = NurbsSurface::new(homogeneous);
            let rows = patch.control_points();
            if rows.len() < 2 || rows[0].len() < 2
                || rows.len().checked_mul(rows[0].len()).is_none_or(|count| count > 1024)
                || rows.iter().any(|row| row.len() != rows[0].len())
                || !patch.is_clamped() {
                return Err(CadError::Unsupported("STEP rational surface carriers require a clamped rectangular net within 1024 controls".into()));
            }
            if patch.knot_vector_u().iter().chain(patch.knot_vector_v().iter()).any(|value| !value.is_finite())
                || rows.iter().flatten().any(|point| ![point.x, point.y, point.z, point.w, point.x / point.w, point.y / point.w, point.z / point.w].into_iter().all(f64::is_finite) || point.w <= 0.) {
                return Err(invalid("A rational surface carrier has nonfinite knots or nonpositive/nonfinite controls"));
            }
            Ok(())
        }
        _ => Err(CadError::Unsupported("STEP surface import currently supports nonperiodic plane and full natural rational patch carriers; analytic periodic and swept surface trims require native per-edge-use UV authority".into())),
    }
}
fn strict_curve(
    source: &monstertruck::step::load::step_geometry::Curve3D,
    depth: usize,
) -> Result<Curve> {
    use monstertruck::step::load::step_geometry::{
        Curve3D, ElementarySurface, Surface as StepSurface, SurfaceCurveAssociatedGeometry,
        SurfaceCurveKind, SurfaceCurveRepresentation,
    };
    if depth > 32 {
        return Err(CadError::Unsupported(
            "STEP surface edge geometry exceeds 32 nested carrier levels".into(),
        ));
    }
    let curve = match source {
        Curve3D::SurfaceCurve(surface_curve) => {
            if surface_curve.kind() == SurfaceCurveKind::SeamCurve
                || surface_curve.master_representation() != SurfaceCurveRepresentation::Curve3D {
                return Err(CadError::Unsupported("STEP seam or pcurve-master edges require preserved per-edge-use UV authority".into()));
            }
            for associated in surface_curve.associated_geometry() {
                let surface = match associated {
                    SurfaceCurveAssociatedGeometry::ParameterCurve(curve) => curve.surface().as_ref(),
                    SurfaceCurveAssociatedGeometry::Surface(surface) => surface.as_ref(),
                };
                strict_carrier(surface)?;
            }
            // The STEP declaration explicitly makes the exact 3D leader the
            // authority. Do not let native sampled association choose a
            // different implicit intersection or a guessed UV surface.
            strict_curve(surface_curve.leader(), depth + 1)?
        }
        Curve3D::ParameterCurve(curve) => {
            if !matches!(curve.surface().as_ref(), StepSurface::ElementarySurface(ElementarySurface::Plane(_))) {
                return Err(CadError::Unsupported("STEP standalone UV edges currently require a nonperiodic plane carrier; rational trim authority remains unsupported".into()));
            }
            strict_carrier(curve.surface())?;
            Curve::try_from(source).map_err(|error| invalid(error.to_string()))?
        }
        Curve3D::IntersectionCurve(_) => return Err(CadError::Unsupported("STEP implicit intersection edges require explicit trim-carrier authority before surface import".into())),
        _ => Curve::try_from(source).map_err(|error| invalid(error.to_string()))?,
    };
    let (start, end) = curve.range_tuple();
    if ![start, end, end - start].into_iter().all(f64::is_finite) || start >= end {
        return Err(invalid(
            "A surface edge has no finite increasing native interval",
        ));
    }
    for parameter in [start, end] {
        let point = curve.subs(parameter);
        if ![point.x, point.y, point.z].into_iter().all(f64::is_finite) {
            return Err(invalid("A surface edge has nonfinite native endpoints"));
        }
    }
    match &curve {
        Curve::BsplineCurve(curve)
            if curve.knot_vector().iter().any(|value| !value.is_finite())
                || curve
                    .control_points()
                    .iter()
                    .any(|point| ![point.x, point.y, point.z].into_iter().all(f64::is_finite)) =>
        {
            return Err(invalid(
                "A surface edge has nonfinite spline controls or knots",
            ));
        }
        Curve::NurbsCurve(curve)
            if curve.knot_vector().iter().any(|value| !value.is_finite())
                || curve.control_points().iter().any(|point| {
                    point.w <= 0.
                        || ![
                            point.x,
                            point.y,
                            point.z,
                            point.w,
                            point.x / point.w,
                            point.y / point.w,
                            point.z / point.w,
                        ]
                        .into_iter()
                        .all(f64::is_finite)
                }) =>
        {
            return Err(invalid(
                "A surface edge needs finite knots and positive finite rational controls",
            ));
        }
        _ => {}
    }
    Ok(curve)
}
