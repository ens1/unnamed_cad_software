//! Exact section placement, miter/trim planes, and independent weldment stock solids.
use crate::adapter::{MonstertruckAdapter, Shape};
use cad_core::{CadError, Document, NodeRef, Plane, Result, SketchCurve, weldment::*};
use monstertruck::modeling::*;
use std::collections::BTreeMap;
fn invalid(message: impl ToString) -> CadError {
    CadError::Invalid(format!("Weldment: {}", message.to_string()))
}
fn sections(inputs: &BTreeMap<NodeRef, Shape>, reference: &NodeRef) -> Result<Vec<Face>> {
    match inputs.get(reference) {
        Some(Shape::Profile(face)) => Ok(vec![face.clone()]),
        Some(Shape::Sketch { faces, .. }) if !faces.is_empty() => Ok(faces.clone()),
        _ => Err(invalid(
            "A member section/gusset needs closed sketch contours",
        )),
    }
}
fn plane(origin: Point3, normal: Vector3) -> Result<Plane> {
    if !normal.magnitude2().is_finite() || normal.magnitude2() < 1e-18 {
        return Err(invalid("Miter or trim plane has no valid normal"));
    }
    let normal = normal.normalize();
    let u = normal
        .cross(if normal.z.abs() < 0.9 {
            Vector3::unit_z()
        } else {
            Vector3::unit_y()
        })
        .normalize();
    let v = normal.cross(u);
    Ok(Plane {
        origin: origin.into(),
        u: u.into(),
        v: v.into(),
    })
}
fn project_face(face: &Face, axis: Vector3, target: &Plane) -> Result<Face> {
    let n = Vector3::from(target.u)
        .cross(Vector3::from(target.v))
        .normalize();
    let dot = n.dot(axis);
    if dot.abs() < 1e-9 {
        return Err(invalid("Trim plane is parallel to the member axis"));
    }
    let translation = axis * Point3::from(target.origin).to_vec().dot(n) / dot;
    let matrix = Matrix4::from_cols(
        (Vector3::unit_x() - axis * (n.x / dot)).extend(0.),
        (Vector3::unit_y() - axis * (n.y / dot)).extend(0.),
        (Vector3::unit_z() - axis * (n.z / dot)).extend(0.),
        translation.extend(1.),
    );
    builder::try_attach_plane(
        face.boundaries()
            .iter()
            .map(|wire| builder::transformed(wire, matrix))
            .collect::<Vec<_>>(),
    )
    .map_err(invalid)
}
struct Segment {
    id: String,
    start: String,
    end: String,
    a: Point3,
    b: Point3,
}
struct BuiltMember {
    reference: MemberRef,
    start: Face,
    finish: Face,
    axis: Vector3,
    solid: Solid,
    metadata: EvaluatedMember,
    start_terminal: bool,
    end_terminal: bool,
}
struct Built {
    solids: Vec<Solid>,
    metadata: Vec<EvaluatedMember>,
}
fn group(
    adapter: &MonstertruckAdapter,
    doc: &Document,
    definition: &Group,
    trims: &[Trim],
    inputs: &BTreeMap<NodeRef, Shape>,
) -> Result<Vec<BuiltMember>> {
    let (source, section_plane) =
        crate::extrusion::source(adapter, doc, &definition.section, inputs)?;
    let (path, path_plane) = crate::extrusion::source(adapter, doc, &definition.path, inputs)?;
    let mut faces = sections(inputs, &definition.section)?;
    if faces.len() != 1 {
        return Err(CadError::Unsupported(
            "Structural members require one section material region, with optional inner contours"
                .into(),
        ));
    }
    let face = faces.remove(0);
    let anchor = if let Some(point) = &definition.anchor {
        *source
            .points
            .get(point)
            .ok_or_else(|| invalid("Section locating point was removed"))?
    } else {
        [0.; 2]
    };
    let section_origin = Point3::from(section_plane.origin)
        + Vector3::from(section_plane.u) * anchor[0]
        + Vector3::from(section_plane.v) * anchor[1];
    let section_u = Vector3::from(section_plane.u);
    let section_v = Vector3::from(section_plane.v);
    let section_n = section_u.cross(section_v).normalize();
    let path_n = Vector3::from(path_plane.u)
        .cross(Vector3::from(path_plane.v))
        .normalize();
    let mut segments = vec![];
    for (id, _) in path
        .members
        .iter()
        .filter(|(id, _)| definition.members.is_empty() || definition.members.contains(id))
    {
        let curve = cad_core::sketch_edit::effective_curve(&path, id)?;
        let SketchCurve::Line { start, end } = curve else {
            return Err(CadError::Unsupported("Structural member paths currently require straight sketch members; curved stock needs a transported section and bend schedule".into()));
        };
        let point = |key: &str| {
            let p = path
                .points
                .get(key)
                .ok_or_else(|| invalid("Path endpoint was removed"))?;
            Ok::<_, CadError>(
                Point3::from(path_plane.origin)
                    + Vector3::from(path_plane.u) * p[0]
                    + Vector3::from(path_plane.v) * p[1],
            )
        };
        let a = point(&start)?;
        let b = point(&end)?;
        if (b - a).magnitude2() < 1e-18 {
            return Err(invalid("Path member has zero length"));
        }
        segments.push(Segment {
            id: id.clone(),
            start,
            end,
            a,
            b,
        });
    }
    if segments.is_empty()
        || definition
            .members
            .iter()
            .any(|id| !segments.iter().any(|s| s.id == *id))
    {
        return Err(invalid(
            "Selected path members are missing, construction geometry, or empty",
        ));
    }
    if segments.len() > 1024 {
        return Err(invalid(
            "A member group supports at most 1024 straight segments",
        ));
    }
    let mut built = vec![];
    for segment in &segments {
        let axis = (segment.b - segment.a).normalize();
        let roll = Matrix3::from_axis_angle(axis, Rad(definition.roll_rad));
        let u = roll * path_n.cross(axis).normalize();
        let v = axis.cross(u);
        let anchor = segment.a;
        let map_vector = |vector: Vector3| {
            u * vector.dot(section_u) + v * vector.dot(section_v) + axis * vector.dot(section_n)
        };
        let translation = anchor.to_vec() - map_vector(section_origin.to_vec());
        let matrix = Matrix4::from_cols(
            map_vector(Vector3::unit_x()).extend(0.),
            map_vector(Vector3::unit_y()).extend(0.),
            map_vector(Vector3::unit_z()).extend(0.),
            translation.extend(1.),
        );
        let section = builder::transformed(&face, matrix);
        let joint = |point: &str, position: Point3, away: Vector3| -> Result<(Plane, bool)> {
            let others = segments
                .iter()
                .filter(|s| s.id != segment.id && (s.start == point || s.end == point))
                .collect::<Vec<_>>();
            let normal = if definition.corner == Corner::Miter && !others.is_empty() {
                if others.len() != 1 {
                    return Err(CadError::Unsupported("Automatic miter corners require exactly two incident members; branch junctions need explicit trim planes".into()));
                }
                let other = others[0];
                let direction = if other.start == point {
                    (other.b - other.a).normalize()
                } else {
                    (other.a - other.b).normalize()
                };
                away - direction
            } else {
                axis
            };
            Ok((plane(position, normal)?, others.is_empty()))
        };
        let (mut start, start_terminal) = joint(&segment.start, segment.a, axis)?;
        let (mut finish, end_terminal) = joint(&segment.end, segment.b, -axis)?;
        start.origin = (Point3::from(start.origin) + axis * definition.start_offset_mm).into();
        finish.origin = (Point3::from(finish.origin) - axis * definition.end_offset_mm).into();
        let reference = MemberRef {
            group: definition.source.clone(),
            member: segment.id.clone(),
        };
        let mut start_terminal = start_terminal;
        let mut end_terminal = end_terminal;
        for trim in trims.iter().filter(|trim| {
            trim.groups.contains(&definition.source)
                && (trim.members.is_empty() || trim.members.contains(&reference))
        }) {
            let mut target = adapter.resolve_surface_plane(doc, &trim.plane, inputs)?;
            let n = Vector3::from(target.u)
                .cross(Vector3::from(target.v))
                .normalize();
            target.origin = (Point3::from(target.origin) + n * trim.offset_mm).into();
            if trim.end == End::Start {
                start = target;
                start_terminal = true;
            } else {
                finish = target;
                end_terminal = true;
            }
        }
        let normal = |plane: &Plane| {
            Vector3::from(plane.u)
                .cross(Vector3::from(plane.v))
                .normalize()
        };
        let at = |plane: &Plane| -> Result<Point3> {
            let n = normal(plane);
            let dot = axis.dot(n);
            if dot.abs() < 1e-9 {
                return Err(invalid("Trim plane is parallel to a selected member axis"));
            }
            Ok(anchor + axis * (Point3::from(plane.origin) - anchor).dot(n) / dot)
        };
        let a = at(&start)?;
        let b = at(&finish)?;
        let length = (b - a).dot(axis);
        if length <= 1e-7 {
            return Err(invalid(
                "Member offsets/trim planes collapse or reverse its centerline",
            ));
        }
        let first = project_face(&section, axis, &start)?;
        let last = project_face(&section, axis, &finish)?;
        let solid = crate::extrusion::to_plane(&first, axis, 0., &finish, 0.)?;
        let metadata = EvaluatedMember {
            reference: reference.clone(),
            section: definition.section.clone(),
            start: a.into(),
            end: b.into(),
            centerline_length_mm: length,
            start_cut_angle_rad: normal(&start).dot(axis).abs().clamp(0., 1.).acos(),
            end_cut_angle_rad: normal(&finish).dot(axis).abs().clamp(0., 1.).acos(),
        };
        built.push(BuiltMember {
            reference,
            start: first,
            finish: last,
            axis,
            solid,
            metadata,
            start_terminal,
            end_terminal,
        });
    }
    Ok(built)
}
fn build(
    adapter: &MonstertruckAdapter,
    doc: &Document,
    stock: &Stock,
    inputs: &BTreeMap<NodeRef, Shape>,
) -> Result<Built> {
    let mut members = vec![];
    for definition in &stock.groups {
        members.extend(group(adapter, doc, definition, &stock.trims, inputs)?);
    }
    if stock.trims.iter().any(|trim| {
        trim.members
            .iter()
            .any(|reference| !members.iter().any(|member| member.reference == *reference))
    }) {
        return Err(invalid("Trim refers to a removed member group/segment"));
    }
    let mut extra = vec![];
    for cap in &stock.caps {
        let member = members
            .iter()
            .find(|m| m.reference == cap.member)
            .ok_or_else(|| invalid("End-cap member was removed"))?;
        let (face, terminal) = if cap.end == End::Start {
            (&member.start, member.start_terminal)
        } else {
            (&member.finish, member.end_terminal)
        };
        if !terminal {
            return Err(CadError::Unsupported(
                "End caps attach to terminal member ends; trim an internal joint before closing it"
                    .into(),
            ));
        }
        let mut n = face.oriented_surface().normal(0., 0.).normalize();
        let outward = member.axis * if cap.end == End::Start { -1. } else { 1. };
        if n.dot(outward) < 0. {
            n = -n;
        }
        let wire = face
            .boundaries()
            .first()
            .cloned()
            .ok_or_else(|| invalid("End-cap section has no outer contour"))?;
        let filled = builder::try_attach_plane(vec![wire]).map_err(invalid)?;
        let filled = builder::translated(&filled, n * cap.offset_mm);
        extra.push(crate::adapter::oriented_extrude(
            &filled,
            n * cap.thickness_mm,
        ));
    }
    for plate in &stock.plates {
        let (_, plane) = crate::extrusion::source(adapter, doc, &plate.profile, inputs)?;
        let n = Vector3::from(plane.u)
            .cross(Vector3::from(plane.v))
            .normalize()
            * if plate.side == cad_core::sheet_metal::ThicknessSide::Negative {
                -1.
            } else {
                1.
            };
        let shift = if plate.side == cad_core::sheet_metal::ThicknessSide::MidPlane {
            -n * plate.thickness_mm * 0.5
        } else {
            Vector3::zero()
        };
        for face in sections(inputs, &plate.profile)? {
            extra.push(crate::adapter::oriented_extrude(
                &builder::translated(&face, shift),
                n * plate.thickness_mm,
            ));
        }
    }
    let metadata = members
        .iter()
        .map(|member| member.metadata.clone())
        .collect();
    let mut solids = members
        .into_iter()
        .map(|member| member.solid)
        .collect::<Vec<_>>();
    solids.extend(extra);
    if solids.is_empty() {
        return Err(invalid("Weldment has no members or gussets"));
    }
    for solid in &solids {
        crate::validation::solid(solid)?;
    }
    Ok(Built { solids, metadata })
}
pub(crate) fn operation(
    adapter: &MonstertruckAdapter,
    doc: &Document,
    owner: &NodeRef,
    inputs: &BTreeMap<NodeRef, Shape>,
) -> Result<Shape> {
    Ok(Shape::Bodies(
        build(adapter, doc, &stock(doc, owner)?, inputs)?.solids,
    ))
}
pub(crate) fn metadata(
    adapter: &MonstertruckAdapter,
    doc: &Document,
    owner: &NodeRef,
    inputs: &BTreeMap<NodeRef, Shape>,
) -> Result<Option<EvaluatedWeldment>> {
    if !doc.parts.get(&owner.definition).is_some_and(|part| {
        part.features.iter().any(|f| {
            f.id == owner.node && matches!(f.operation, cad_core::Operation::Weldment { .. })
        })
    }) {
        return Ok(None);
    }
    let stock = stock(doc, owner)?;
    let built = build(adapter, doc, &stock, inputs)?;
    Ok(Some(EvaluatedWeldment {
        version: CONTRACT_VERSION,
        source: owner.clone(),
        members: built.metadata,
        cap_count: stock.caps.len(),
        gusset_count: stock.plates.len(),
    }))
}
